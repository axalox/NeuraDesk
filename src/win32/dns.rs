//! **LE DNS DE WINDOWS** — lire les serveurs DNS des cartes réseau actives et
//! les changer (voir `win32::cef_dns`, la fenêtre de l'applet).
//!
//! DEMANDE (2026-11-22) : « dans intégrés je voudrais une applet pour modifier
//! le dns de windows ».
//!
//! - **Lire** ne demande aucun droit : `GetAdaptersAddresses` donne les cartes
//!   et leurs serveurs ; le MODE (automatique / manuel) se lit dans le registre
//!   (`NameServer` non vide sous `Tcpip` ou `Tcpip6` = serveurs posés à la main).
//! - **Changer** demande l'administrateur : un PowerShell élevé par l'UAC
//!   (`ShellExecuteExW` + `runas`), caché, qui lance
//!   `Set-DnsClientServerAddress` puis vide le cache. Une invite UAC par
//!   application ; un refus rend `Annule`, pas une erreur.
//!
//! ⛔ **CE QUI GARDE LA LIGNE DE COMMANDE ÉLEVÉE** : rien de la page n'y entre
//! tel quel. Les index de carte sont des `u32` RELUS dans notre propre
//! énumération, les adresses sont PARSÉES (`std::net::IpAddr`) et réécrites
//! depuis la valeur parsée ; le script part en `-EncodedCommand` (base64
//! UTF-16), donc aucun guillemet à échapper.
//!
//! DEMANDE (2026-11-24) : les autres réglages réseau de Windows dans la même
//! fenêtre. La carte porte maintenant tout ce que les onglets montrent (GUID,
//! adresse MAC, IPv6, catégorie, coût, DNS chiffré) et `Demande` les gestes
//! de chaque onglet : `dns_doh`, `reseau_ip`, `reseau_connexion`,
//! `reseau_mac` (le proxy, sans droit, ne passe pas par ici :
//! `reseau_proxy`). ⛔ Tout se LIT sans PowerShell (`relire` tourne sur le fil
//! de l'interface) ; seul l'ÉCRIT passe par le PowerShell élevé.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    IP_ADAPTER_DHCP_ENABLED,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

/// `IF_TYPE_SOFTWARE_LOOPBACK` et `IF_TYPE_TUNNEL` : jamais montrées.
const IF_TYPE_LOOPBACK: u32 = 24;
const IF_TYPE_TUNNEL: u32 = 131;
/// Le délai maximal d'attente du PowerShell élevé (~1 s d'ordinaire ; un
/// renouvellement DHCP sans réponse ou une carte qui redémarre prennent plus).
const ATTENTE_MAX_MS: u32 = 120_000;

/// Une carte réseau active et ses serveurs DNS.
#[derive(Clone, Debug, PartialEq)]
pub struct Carte {
    pub index: u32,
    pub nom: String,
    pub description: String,
    pub serveurs: Vec<String>,
    /// `true` : des serveurs posés à la main ; `false` : ceux du DHCP.
    pub manuel: bool,
    /// `true` : l'adresse IPv4 vient du DHCP (onglet « Adresse IP »).
    pub dhcp: bool,
    /// L'adresse IPv4 et sa longueur de préfixe (hors lien local 169.254).
    pub ipv4: Option<(Ipv4Addr, u8)>,
    /// La première passerelle IPv4.
    pub passerelle: Option<Ipv4Addr>,
    /// `{XXXXXXXX-…}`, validé (`guid_valide`) — vide sinon.
    pub guid: String,
    /// `IfType` : 6 = câble, 71 = Wi-Fi (voir `reseau_connexion`).
    pub type_if: u32,
    /// L'adresse MAC actuelle (`AA:BB:…`).
    pub mac: Option<String>,
    /// L'adresse posée à la main dans le registre (`reseau_mac`), s'il y en a.
    pub mac_posee: Option<String>,
    /// L'adresse IPv6 globale montrée (une posée à la main d'abord).
    pub ipv6: Option<(Ipv6Addr, u8)>,
    /// La première passerelle IPv6 (souvent de lien local).
    pub passerelle6: Option<Ipv6Addr>,
    /// `true` : une adresse IPv6 posée à la main.
    pub ipv6_manuel: bool,
    /// Les serveurs que cette carte chiffre (`dns_doh`).
    pub doh: Vec<String>,
    /// Ce que le gestionnaire des réseaux en dit (`reseau_connexion`).
    pub connexion: Option<crate::win32::reseau_connexion::EtatConnexion>,
}

/// Une carte visée par une demande DNS : son index ET son GUID (validé), pour
/// les clés du DNS chiffré.
#[derive(Clone, Debug, PartialEq)]
pub struct Cible {
    pub index: u32,
    pub guid: String,
}

/// `{8-4-4-4-12}` en chiffres hexadécimaux : le SEUL format de GUID qui entre
/// dans un script ou une clé de registre.
pub fn guid_valide(g: &str) -> bool {
    let Some(corps) = g.strip_prefix('{').and_then(|r| r.strip_suffix('}')) else { return false };
    let longueurs: Vec<usize> = corps.split('-').map(str::len).collect();
    longueurs == [8, 4, 4, 4, 12] && corps.chars().all(|c| c == '-' || c.is_ascii_hexdigit())
}

/// Ce qu'on demande à Windows.
#[derive(Clone, Debug, PartialEq)]
pub enum Demande {
    /// Revenir aux serveurs donnés par le réseau (DHCP), sans chiffrement.
    Automatique(Vec<Cible>),
    /// Poser ces serveurs (dans l'ordre) sur ces cartes ; `true` : chiffrés.
    Serveurs(Vec<Cible>, Vec<IpAddr>, bool),
    /// Vider le cache DNS seulement.
    ViderCache,
    /// L'adresse IPv4 ou IPv6 d'une carte (voir `win32::reseau_ip`).
    Ip(crate::win32::reseau_ip::DemandeIp),
    /// Catégorie, coût, renouvellement (voir `win32::reseau_connexion`).
    Connexion(crate::win32::reseau_connexion::DemandeConnexion),
    /// L'adresse MAC (voir `win32::reseau_mac`).
    Mac(crate::win32::reseau_mac::DemandeMac),
}

/// L'issue d'une demande, rendue à la page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Issue {
    Reussi,
    Annule,
    Echec,
    /// Le pilote a ignoré l'adresse MAC (`reseau_mac::CODE_IGNOREE`).
    Ignoree,
}

impl Issue {
    pub fn code(self) -> &'static str {
        match self {
            Issue::Reussi => "reussi",
            Issue::Annule => "annule",
            Issue::Echec => "echec",
            Issue::Ignoree => "ignoree",
        }
    }
}

/// Les cartes ACTIVES (connectées), hors bouclage et tunnels.
pub fn cartes() -> Vec<Carte> {
    let drapeaux = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_INCLUDE_GATEWAYS;
    let mut taille: u32 = 16 * 1024;
    let mut tampon: Vec<u64> = Vec::new();
    for _ in 0..4 {
        tampon = vec![0u64; (taille as usize).div_ceil(8)];
        let premiere = tampon.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        let code = unsafe { GetAdaptersAddresses(0, drapeaux, None, Some(premiere), &mut taille) };
        match code {
            0 => break,
            111 => continue, // ERROR_BUFFER_OVERFLOW : `taille` a été mise à jour
            _ => return Vec::new(),
        }
    }
    let mut cartes = Vec::new();
    let etats = crate::win32::reseau_connexion::etats();
    let mut courante = tampon.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !courante.is_null() {
        let a = unsafe { &*courante };
        courante = a.Next;
        // Sans PASSERELLE, une carte « Up » ne mène nulle part (VirtualBox,
        // WSL, un VPN au repos) : elle encombrerait la liste.
        if a.OperStatus != IfOperStatusUp || a.IfType == IF_TYPE_LOOPBACK || a.IfType == IF_TYPE_TUNNEL || a.FirstGatewayAddress.is_null() {
            continue;
        }
        let index = unsafe { a.Anonymous1.Anonymous.IfIndex };
        let guid = unsafe { a.AdapterName.to_string() }.unwrap_or_default();
        let mut serveurs = Vec::new();
        let mut s = a.FirstDnsServerAddress;
        while !s.is_null() {
            let serveur = unsafe { &*s };
            if let Some(ip) = unsafe { adresse(serveur.Address.lpSockaddr as *const u8, serveur.Address.iSockaddrLength) } {
                if !est_place_vide(&ip) {
                    serveurs.push(ip.to_string());
                }
            }
            s = serveur.Next;
        }
        let mut ipv4 = None;
        let mut u = a.FirstUnicastAddress;
        while !u.is_null() && ipv4.is_none() {
            let unicast = unsafe { &*u };
            if let Some(IpAddr::V4(v4)) = unsafe { adresse(unicast.Address.lpSockaddr as *const u8, unicast.Address.iSockaddrLength) } {
                if !v4.is_link_local() {
                    ipv4 = Some((v4, unicast.OnLinkPrefixLength));
                }
            }
            u = unicast.Next;
        }
        let (ipv6, ipv6_manuel) = unsafe { ipv6_de(a.FirstUnicastAddress) };
        let mut passerelle = None;
        let mut passerelle6 = None;
        let mut g = a.FirstGatewayAddress;
        while !g.is_null() {
            let porte = unsafe { &*g };
            match unsafe { adresse(porte.Address.lpSockaddr as *const u8, porte.Address.iSockaddrLength) } {
                Some(IpAddr::V4(v4)) if passerelle.is_none() => passerelle = Some(v4),
                Some(IpAddr::V6(v6)) if passerelle6.is_none() => passerelle6 = Some(v6),
                _ => {}
            }
            g = porte.Next;
        }
        let guid = if guid_valide(&guid) { guid.to_ascii_uppercase() } else { String::new() };
        let longueur_mac = (a.PhysicalAddressLength as usize).min(a.PhysicalAddress.len());
        let mac = (longueur_mac == 6).then(|| crate::win32::reseau_mac::texte(&a.PhysicalAddress[..6]));
        let connexion = etats.iter().find(|(g, _)| g.eq_ignore_ascii_case(&guid)).map(|(_, e)| *e);
        cartes.push(Carte {
            index,
            nom: unsafe { a.FriendlyName.to_string() }.unwrap_or_default(),
            description: unsafe { a.Description.to_string() }.unwrap_or_default(),
            serveurs,
            manuel: est_manuel(&guid),
            dhcp: unsafe { a.Anonymous2.Flags } & IP_ADAPTER_DHCP_ENABLED != 0,
            ipv4,
            passerelle,
            type_if: a.IfType,
            mac,
            mac_posee: crate::win32::reseau_mac::adresse_posee(&guid),
            ipv6,
            passerelle6,
            ipv6_manuel,
            doh: crate::win32::dns_doh::serveurs_chiffres(&guid),
            connexion,
            guid,
        });
    }
    cartes
}

/// L'adresse IPv6 globale à montrer (une posée à la main d'abord, puis une
/// stable, puis n'importe laquelle hors lien local) et « une est manuelle ».
unsafe fn ipv6_de(mut u: *mut windows::Win32::NetworkManagement::IpHelper::IP_ADAPTER_UNICAST_ADDRESS_LH) -> (Option<(Ipv6Addr, u8)>, bool) {
    // `IpPrefixOriginManual` = 1 ; `IpSuffixOriginRandom` = 5 (temporaire).
    let (mut manuelle, mut stable, mut autre) = (None, None, None);
    while !u.is_null() {
        let unicast = unsafe { &*u };
        if let Some(IpAddr::V6(v6)) = unsafe { adresse(unicast.Address.lpSockaddr as *const u8, unicast.Address.iSockaddrLength) } {
            if v6.segments()[0] & 0xffc0 != 0xfe80 && !v6.is_loopback() {
                let une = Some((v6, unicast.OnLinkPrefixLength));
                if unicast.PrefixOrigin.0 == 1 {
                    manuelle = manuelle.or(une);
                } else if unicast.SuffixOrigin.0 != 5 {
                    stable = stable.or(une);
                } else {
                    autre = autre.or(une);
                }
            }
        }
        u = unicast.Next;
    }
    (manuelle.or(stable).or(autre), manuelle.is_some())
}

/// L'adresse d'un `SOCKADDR` lu en octets : famille aux octets 0..2, IPv4 aux
/// octets 4..8, IPv6 aux octets 8..24.
unsafe fn adresse(p: *const u8, longueur: i32) -> Option<IpAddr> {
    if p.is_null() || longueur < 8 {
        return None;
    }
    let octets = unsafe { std::slice::from_raw_parts(p, longueur as usize) };
    match u16::from_le_bytes([octets[0], octets[1]]) {
        2 if octets.len() >= 8 => Some(IpAddr::V4(Ipv4Addr::new(octets[4], octets[5], octets[6], octets[7]))),
        23 if octets.len() >= 24 => {
            let mut a = [0u8; 16];
            a.copy_from_slice(&octets[8..24]);
            Some(IpAddr::V6(Ipv6Addr::from(a)))
        }
        _ => None,
    }
}

/// Les trois `fec0:0:0:ffff::1..3` que Windows annonce quand AUCUN serveur
/// IPv6 n'est connu : des places vides, pas des serveurs.
fn est_place_vide(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V6(v6) => v6.segments()[..4] == [0xfec0, 0, 0, 0xffff],
        IpAddr::V4(_) => false,
    }
}

fn est_manuel(guid: &str) -> bool {
    if guid.is_empty() {
        return false;
    }
    ["Tcpip", "Tcpip6"].iter().any(|pile| {
        let cle = format!("SYSTEM\\CurrentControlSet\\Services\\{pile}\\Parameters\\Interfaces\\{guid}");
        crate::win32::registry_util::read_string_value(HKEY_LOCAL_MACHINE, &cle, "NameServer")
            .is_some_and(|v| !v.trim().is_empty())
    })
}

/// **LIRE CE QUE LA PAGE DEMANDE**, sans rien en croire :
/// - `cartes` : des index séparés par des virgules, gardés seulement s'ils
///   figurent dans `connues` (notre propre énumération, GUID validé) ;
/// - `quoi` : `auto`, ou des adresses IP séparées par des virgules (1 à 8) ;
/// - `chiffrer` : `1` pour le DNS chiffré — seulement si TOUTES les adresses
///   ont un modèle connu (`dns_doh`), `0` sinon ; rien d'autre.
pub fn lire_demande(cartes: &str, quoi: &str, chiffrer: &str, connues: &[Cible]) -> Option<Demande> {
    let mut cibles: Vec<Cible> = Vec::new();
    for morceau in cartes.split(',') {
        let i: u32 = morceau.trim().parse().ok()?;
        let carte = connues.iter().find(|c| c.index == i && guid_valide(&c.guid))?;
        if !cibles.iter().any(|c| c.index == i) {
            cibles.push(carte.clone());
        }
    }
    if cibles.is_empty() {
        return None;
    }
    let chiffrer = match chiffrer {
        "1" => true,
        "0" => false,
        _ => return None,
    };
    if quoi == "auto" {
        return (!chiffrer).then_some(Demande::Automatique(cibles));
    }
    let mut adresses = Vec::new();
    for morceau in quoi.split(',') {
        let morceau = morceau.trim();
        if morceau.is_empty() {
            continue;
        }
        // `IpAddr::from_str` refuse tout ce qui n'est pas une adresse : pas
        // d'espace, pas de guillemet, pas de point-virgule, pas de zone `%`.
        let ip: IpAddr = morceau.parse().ok()?;
        if ip.is_unspecified() || ip.is_multicast() {
            return None;
        }
        if !adresses.contains(&ip) {
            adresses.push(ip);
        }
    }
    if adresses.is_empty() || adresses.len() > 8 || (chiffrer && !crate::win32::dns_doh::chiffrable(&adresses)) {
        return None;
    }
    Some(Demande::Serveurs(cibles, adresses, chiffrer))
}

/// Le script PowerShell d'une demande. N'y entrent que des `u32` et des
/// adresses RÉÉCRITES depuis leur valeur parsée.
pub fn script(demande: &Demande) -> String {
    let index = |liste: &[Cible]| liste.iter().map(|c| c.index.to_string()).collect::<Vec<_>>().join(",");
    let guids = |liste: &[Cible]| liste.iter().map(|c| c.guid.clone()).collect::<Vec<_>>();
    let action = match demande {
        Demande::Automatique(cartes) => {
            let doh = crate::win32::dns_doh::script(&guids(cartes).iter().map(String::as_str).collect::<Vec<_>>(), &[], false);
            format!("Set-DnsClientServerAddress -InterfaceIndex {} -ResetServerAddresses; {doh}", index(cartes))
        }
        Demande::Serveurs(cartes, adresses, chiffrer) => {
            let liste = adresses.iter().map(|ip| format!("'{ip}'")).collect::<Vec<_>>().join(",");
            let doh = crate::win32::dns_doh::script(&guids(cartes).iter().map(String::as_str).collect::<Vec<_>>(), adresses, *chiffrer);
            format!("Set-DnsClientServerAddress -InterfaceIndex {} -ServerAddresses @({liste}); {doh}", index(cartes))
        }
        Demande::ViderCache => String::new(),
        // Des scripts à part : ils ne vident pas le cache DNS.
        Demande::Ip(ip) => return crate::win32::reseau_ip::script(ip),
        Demande::Connexion(c) => return crate::win32::reseau_connexion::script(c),
        Demande::Mac(m) => return crate::win32::reseau_mac::script(m),
    };
    format!("$ErrorActionPreference='Stop'; try {{ {action}Clear-DnsClientCache; exit 0 }} catch {{ exit 1 }}")
}

/// `-EncodedCommand` : le script en base64 de son UTF-16 petit-boutiste.
fn encoder(script: &str) -> String {
    use base64::Engine;
    let octets: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    base64::engine::general_purpose::STANDARD.encode(octets)
}

/// **APPLIQUER** — BLOQUANT (attend l'invite UAC puis PowerShell) : à appeler
/// depuis un fil de travail, jamais depuis la boucle d'images.
pub fn appliquer(demande: &Demande) -> Issue {
    let parametres = format!(
        "-NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -EncodedCommand {}",
        encoder(&script(demande))
    );
    let large = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (verbe, fichier, parametres) = (large("runas"), large("powershell.exe"), large(&parametres));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(verbe.as_ptr()),
        lpFile: PCWSTR(fichier.as_ptr()),
        lpParameters: PCWSTR(parametres.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
        // ERROR_CANCELLED (1223) : l'utilisateur a refusé l'invite UAC.
        let annule = e.code().0 as u32 == 0x8007_04C7;
        crate::diag!("[dns] ShellExecuteExW : {e:?}");
        return if annule { Issue::Annule } else { Issue::Echec };
    }
    if info.hProcess.is_invalid() {
        return Issue::Echec;
    }
    let mut code = 1u32;
    unsafe {
        if WaitForSingleObject(info.hProcess, ATTENTE_MAX_MS) == WAIT_OBJECT_0 {
            let _ = GetExitCodeProcess(info.hProcess, &mut code);
        }
        let _ = CloseHandle(info.hProcess);
    }
    crate::diag!("[dns] PowerShell élevé : code {code}");
    match code {
        0 => Issue::Reussi,
        crate::win32::reseau_mac::CODE_IGNOREE if matches!(demande, Demande::Mac(_)) => Issue::Ignoree,
        _ => Issue::Echec,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G12: &str = "{01234567-89AB-CDEF-0123-456789ABCD12}";
    const G7: &str = "{01234567-89AB-CDEF-0123-456789ABCD07}";

    fn cible(index: u32, guid: &str) -> Cible {
        Cible { index, guid: guid.to_string() }
    }

    fn connues() -> Vec<Cible> {
        vec![cible(12, G12), cible(7, G7)]
    }

    #[test]
    fn une_demande_valide_est_lue() {
        let d = lire_demande("12", "1.1.1.1, 1.0.0.1,2606:4700:4700::1111", "0", &connues()).unwrap();
        assert_eq!(
            d,
            Demande::Serveurs(
                vec![cible(12, G12)],
                vec!["1.1.1.1".parse().unwrap(), "1.0.0.1".parse().unwrap(), "2606:4700:4700::1111".parse().unwrap()],
                false
            )
        );
        assert_eq!(lire_demande("7,12,7", "auto", "0", &connues()), Some(Demande::Automatique(vec![cible(7, G7), cible(12, G12)])));
    }

    #[test]
    fn le_chiffrement_ne_vaut_que_pour_des_serveurs_connus() {
        assert!(matches!(lire_demande("12", "9.9.9.9,149.112.112.112", "1", &connues()), Some(Demande::Serveurs(_, _, true))));
        assert_eq!(lire_demande("12", "9.9.9.9,192.168.1.1", "1", &connues()), None, "serveur sans modèle");
        assert_eq!(lire_demande("12", "auto", "1", &connues()), None, "l'automatique ne se chiffre pas");
        assert_eq!(lire_demande("12", "9.9.9.9", "oui", &connues()), None);
        // Une carte sans GUID lisible ne reçoit rien.
        assert_eq!(lire_demande("3", "9.9.9.9", "0", &[cible(3, "")]), None);
    }

    #[test]
    fn un_guid_a_la_forme_exacte() {
        assert!(guid_valide(G12));
        assert!(guid_valide("{abcdef01-2345-6789-abcd-ef0123456789}"));
        for faux in ["01234567-89AB-CDEF-0123-456789ABCD12", "{01234567-89AB-CDEF-0123-456789ABCD1}", "{0123456789AB-CDEF-0123-456789ABCD12-}", "{01234567-89AB-CDEF-0123-456789ABCD1Z}", "{'; calc; '}", ""] {
            assert!(!guid_valide(faux), "accepté à tort : {faux:?}");
        }
    }

    #[test]
    fn une_injection_est_refusee() {
        let connues = [cible(12, G12)];
        for quoi in [
            "1.1.1.1'; Remove-Item C:\\ -Recurse; '",
            "1.1.1.1;calc",
            "\"1.1.1.1\"",
            "1.1.1.1 8.8.8.8",
            "fe80::1%12",
            "１.１.１.１",
            "0.0.0.0",
            "224.0.0.1",
            "",
            "localhost",
        ] {
            assert_eq!(lire_demande("12", quoi, "0", &connues), None, "accepté à tort : {quoi:?}");
        }
        assert_eq!(lire_demande("13", "1.1.1.1", "0", &connues), None, "carte inconnue");
        assert_eq!(lire_demande("12;calc", "1.1.1.1", "0", &connues), None);
        assert_eq!(lire_demande("", "1.1.1.1", "0", &connues), None);
        let trop = (1..=9).map(|i| format!("10.0.0.{i}")).collect::<Vec<_>>().join(",");
        assert_eq!(lire_demande("12", &trop, "0", &connues), None, "plus de huit serveurs");
    }

    #[test]
    fn le_script_ne_porte_que_des_valeurs_reecrites() {
        let d = lire_demande("12,7", "8.8.8.8,2001:4860:4860::8888", "0", &connues()).unwrap();
        let s = script(&d);
        assert!(s.starts_with(
            "$ErrorActionPreference='Stop'; try { Set-DnsClientServerAddress -InterfaceIndex 12,7 \
             -ServerAddresses @('8.8.8.8','2001:4860:4860::8888'); "
        ));
        // Sans chiffrement : l'ancien chiffrement des DEUX cartes est effacé.
        assert!(s.contains(&format!("{G12}\\DohInterfaceSettings\\Doh'")) && s.contains(&format!("{G7}\\DohInterfaceSettings\\Doh6'")));
        assert!(!s.contains("CreateSubKey"));
        assert!(s.ends_with("Clear-DnsClientCache; exit 0 } catch { exit 1 }"));
        let c = script(&lire_demande("12", "8.8.8.8", "1", &connues()).unwrap());
        assert!(c.contains(&format!("{G12}\\DohInterfaceSettings\\Doh\\8.8.8.8')")));
        assert!(script(&Demande::Automatique(vec![cible(3, G7)])).contains("-InterfaceIndex 3 -ResetServerAddresses; [Microsoft.Win32.Registry]"));
        assert!(script(&Demande::ViderCache).contains("try { Clear-DnsClientCache; exit 0 }"));
    }

    #[test]
    fn l_encodage_est_celui_de_powershell() {
        // `powershell -EncodedCommand` attend le base64 de l'UTF-16LE.
        assert_eq!(encoder("exit 0"), "ZQB4AGkAdAAgADAA");
    }

    #[test]
    fn les_places_vides_ipv6_sont_ecartees() {
        assert!(est_place_vide(&"fec0:0:0:ffff::1".parse().unwrap()));
        assert!(!est_place_vide(&"2606:4700:4700::1111".parse().unwrap()));
        assert!(!est_place_vide(&"1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn une_adresse_se_lit_dans_un_sockaddr() {
        let v4 = [2u8, 0, 0, 53, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(unsafe { adresse(v4.as_ptr(), 16) }, Some("1.1.1.1".parse().unwrap()));
        let mut v6 = [0u8; 28];
        v6[0] = 23;
        v6[8..24].copy_from_slice(&"2620:fe::fe".parse::<Ipv6Addr>().unwrap().octets());
        assert_eq!(unsafe { adresse(v6.as_ptr(), 28) }, Some("2620:fe::fe".parse().unwrap()));
        assert_eq!(unsafe { adresse(std::ptr::null(), 16) }, None);
    }
}

#[cfg(test)]
mod sonde {
    /// Sonde manuelle : `cargo test -- dns::sonde --ignored --nocapture`.
    #[test]
    #[ignore]
    fn lister_les_cartes() {
        for c in super::cartes() {
            println!(
                "{} {} | {} | {:?} manuel={} | dhcp={} {:?} passerelle {:?}\n   guid {} type {} mac {:?} posée {:?} | v6 {:?} manuel={} passerelle {:?} | doh {:?} | {:?}",
                c.index, c.nom, c.description, c.serveurs, c.manuel, c.dhcp, c.ipv4, c.passerelle,
                c.guid, c.type_if, c.mac, c.mac_posee, c.ipv6, c.ipv6_manuel, c.passerelle6, c.doh, c.connexion
            );
        }
    }
}
