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

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST, GAA_FLAG_SKIP_UNICAST, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

/// `IF_TYPE_SOFTWARE_LOOPBACK` et `IF_TYPE_TUNNEL` : jamais montrées.
const IF_TYPE_LOOPBACK: u32 = 24;
const IF_TYPE_TUNNEL: u32 = 131;
/// Le délai maximal d'attente du PowerShell élevé (il prend ~1 s).
const ATTENTE_MAX_MS: u32 = 60_000;

/// Une carte réseau active et ses serveurs DNS.
#[derive(Clone, Debug, PartialEq)]
pub struct Carte {
    pub index: u32,
    pub nom: String,
    pub description: String,
    pub serveurs: Vec<String>,
    /// `true` : des serveurs posés à la main ; `false` : ceux du DHCP.
    pub manuel: bool,
}

/// Ce qu'on demande à Windows.
#[derive(Clone, Debug, PartialEq)]
pub enum Demande {
    /// Revenir aux serveurs donnés par le réseau (DHCP).
    Automatique(Vec<u32>),
    /// Poser ces serveurs (dans l'ordre) sur ces cartes.
    Serveurs(Vec<u32>, Vec<IpAddr>),
    /// Vider le cache DNS seulement.
    ViderCache,
}

/// L'issue d'une demande, rendue à la page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Issue {
    Reussi,
    Annule,
    Echec,
}

impl Issue {
    pub fn code(self) -> &'static str {
        match self {
            Issue::Reussi => "reussi",
            Issue::Annule => "annule",
            Issue::Echec => "echec",
        }
    }
}

/// Les cartes ACTIVES (connectées), hors bouclage et tunnels.
pub fn cartes() -> Vec<Carte> {
    let drapeaux = GAA_FLAG_SKIP_UNICAST | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_INCLUDE_GATEWAYS;
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
        cartes.push(Carte {
            index,
            nom: unsafe { a.FriendlyName.to_string() }.unwrap_or_default(),
            description: unsafe { a.Description.to_string() }.unwrap_or_default(),
            serveurs,
            manuel: est_manuel(&guid),
        });
    }
    cartes
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
///   figurent dans `connues` (notre propre énumération) ;
/// - `quoi` : `auto`, ou des adresses IP séparées par des virgules (1 à 8).
pub fn lire_demande(cartes: &str, quoi: &str, connues: &[u32]) -> Option<Demande> {
    let mut index = Vec::new();
    for morceau in cartes.split(',') {
        let i: u32 = morceau.trim().parse().ok()?;
        if !connues.contains(&i) {
            return None;
        }
        if !index.contains(&i) {
            index.push(i);
        }
    }
    if index.is_empty() {
        return None;
    }
    if quoi == "auto" {
        return Some(Demande::Automatique(index));
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
    if adresses.is_empty() || adresses.len() > 8 {
        return None;
    }
    Some(Demande::Serveurs(index, adresses))
}

/// Le script PowerShell d'une demande. N'y entrent que des `u32` et des
/// adresses RÉÉCRITES depuis leur valeur parsée.
pub fn script(demande: &Demande) -> String {
    let index = |liste: &[u32]| liste.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    let action = match demande {
        Demande::Automatique(cartes) => {
            format!("Set-DnsClientServerAddress -InterfaceIndex {} -ResetServerAddresses; ", index(cartes))
        }
        Demande::Serveurs(cartes, adresses) => {
            let liste = adresses.iter().map(|ip| format!("'{ip}'")).collect::<Vec<_>>().join(",");
            format!("Set-DnsClientServerAddress -InterfaceIndex {} -ServerAddresses @({liste}); ", index(cartes))
        }
        Demande::ViderCache => String::new(),
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
    if code == 0 {
        Issue::Reussi
    } else {
        Issue::Echec
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_demande_valide_est_lue() {
        let d = lire_demande("12", "1.1.1.1, 1.0.0.1,2606:4700:4700::1111", &[12, 7]).unwrap();
        assert_eq!(
            d,
            Demande::Serveurs(
                vec![12],
                vec!["1.1.1.1".parse().unwrap(), "1.0.0.1".parse().unwrap(), "2606:4700:4700::1111".parse().unwrap()]
            )
        );
        assert_eq!(lire_demande("7,12,7", "auto", &[12, 7]), Some(Demande::Automatique(vec![7, 12])));
    }

    #[test]
    fn une_injection_est_refusee() {
        let connues = [12];
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
            assert_eq!(lire_demande("12", quoi, &connues), None, "accepté à tort : {quoi:?}");
        }
        assert_eq!(lire_demande("13", "1.1.1.1", &connues), None, "carte inconnue");
        assert_eq!(lire_demande("12;calc", "1.1.1.1", &connues), None);
        assert_eq!(lire_demande("", "1.1.1.1", &connues), None);
        let trop = (1..=9).map(|i| format!("10.0.0.{i}")).collect::<Vec<_>>().join(",");
        assert_eq!(lire_demande("12", &trop, &connues), None, "plus de huit serveurs");
    }

    #[test]
    fn le_script_ne_porte_que_des_valeurs_reecrites() {
        let d = lire_demande("12,7", "8.8.8.8,2001:4860:4860::8888", &[7, 12]).unwrap();
        assert_eq!(
            script(&d),
            "$ErrorActionPreference='Stop'; try { Set-DnsClientServerAddress -InterfaceIndex 12,7 \
             -ServerAddresses @('8.8.8.8','2001:4860:4860::8888'); Clear-DnsClientCache; exit 0 } catch { exit 1 }"
        );
        assert!(script(&Demande::Automatique(vec![3])).contains("-InterfaceIndex 3 -ResetServerAddresses"));
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
            println!("{} {} | {} | {:?} manuel={}", c.index, c.nom, c.description, c.serveurs, c.manuel);
        }
    }
}
