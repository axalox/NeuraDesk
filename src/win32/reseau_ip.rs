//! **L'ADRESSE IP D'UNE CARTE** — automatique (DHCP) ou manuelle (adresse,
//! masque, passerelle). Onglet « Adresse IP » de l'applet « Modifier réglages
//! réseau » (voir `win32::cef_dns`, `win32::dns` pour l'énumération et
//! l'application élevée).
//!
//! DEMANDE (2026-11-23) : « applet DNS, ajouter un onglet pour régler l'adresse
//! ip manuellement ou automatiquement, et modification de l'adresse de la
//! passerelle, renommer applet en Modifier réglages réseau ».
//!
//! - IPv4 seulement : c'est elle qu'on règle à la main sur un réseau local.
//! - ⛔ **PASSER EN MANUEL COUPE LES DNS DU DHCP** : sans DHCP, Windows oublie
//!   les serveurs que la box donnait (`DhcpNameServer`) et plus aucun nom ne se
//!   résout. Si la carte était en DNS automatique, ses serveurs actuels sont
//!   donc POSÉS à la main dans le même script.
//! - ⛔ **UN ÉCHEC À MI-CHEMIN LAISSERAIT LA CARTE SANS ADRESSE** (l'ancienne
//!   est retirée avant de poser la nouvelle) : le `catch` remet le DHCP.
//! - Revenir en automatique ne retire la route par défaut QUE si le DHCP était
//!   coupé : retirer celle d'un DHCP actif la perdrait jusqu'au prochain bail.
//!
//! ⛔ Même garde que `dns` : rien de la page n'entre tel quel dans le script —
//! index relu dans notre énumération, adresses parsées en `Ipv4Addr` et
//! réécrites, préfixe en `u8`.
//!
//! DEMANDE (2026-11-24) : « peux tu ajouter ces réglages » — dont l'adresse
//! IPv6 manuelle. Même schéma que l'IPv4, avec deux différences :
//! - « automatique » ne retire QUE les adresses posées à la main
//!   (`PrefixOrigin Manual`) et la route par défaut posée à la main
//!   (`Protocol NetMgmt`) : celles que le routeur annonce (SLAAC, adresses
//!   temporaires) ne sont jamais touchées ;
//! - la passerelle est facultative et peut être une adresse de LIEN LOCAL
//!   (`fe80::1` : c'est le cas de la plupart des box).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Ce qu'on demande pour l'adresse IPv4 d'UNE carte.
#[derive(Clone, Debug, PartialEq)]
pub enum DemandeIp {
    /// Adresse, masque et passerelle donnés par le réseau.
    Automatique { index: u32 },
    /// Adresse fixe. `dns` : les serveurs à poser à la main (vide = garder
    /// ceux déjà posés à la main).
    Manuelle { index: u32, adresse: Ipv4Addr, prefixe: u8, passerelle: Ipv4Addr, dns: Vec<IpAddr> },
    /// IPv6 : seulement les adresses que le réseau annonce.
    Automatique6 { index: u32 },
    /// IPv6 : une adresse fixe, et une passerelle si elle est donnée.
    Manuelle6 { index: u32, adresse: Ipv6Addr, prefixe: u8, passerelle: Option<Ipv6Addr> },
}

/// Ce que `lire_demande` doit savoir d'une carte connue.
pub struct CarteConnue<'a> {
    pub index: u32,
    /// Les serveurs DNS actuels (textes de notre propre énumération).
    pub serveurs: &'a [String],
    /// `true` : ses DNS sont déjà posés à la main.
    pub dns_manuel: bool,
}

/// Un masque en longueur de préfixe : `24`, `/24` ou `255.255.255.0`.
pub fn prefixe_du_masque(s: &str) -> Option<u8> {
    let s = s.trim();
    let s = s.strip_prefix('/').unwrap_or(s);
    if let Ok(n) = s.parse::<u8>() {
        return (1..=30).contains(&n).then_some(n);
    }
    let masque = u32::from(s.parse::<Ipv4Addr>().ok()?);
    let n = masque.leading_ones();
    // Des 1 PUIS des 0 : `255.0.255.0` n'est pas un masque.
    (masque.checked_shl(n).unwrap_or(0) == 0 && (1..=30).contains(&n)).then_some(n as u8)
}

/// Le masque pointé d'une longueur de préfixe (pour l'afficher).
pub fn masque_du_prefixe(prefixe: u8) -> Ipv4Addr {
    Ipv4Addr::from(u32::MAX.checked_shl(32 - u32::from(prefixe.min(32))).unwrap_or(0))
}

/// Une adresse d'HÔTE de ce sous-réseau : ni réservée, ni l'adresse du
/// réseau, ni sa diffusion.
fn est_un_hote(ip: Ipv4Addr, prefixe: u8) -> bool {
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() || ip.is_broadcast() || ip.is_link_local() {
        return false;
    }
    let masque = u32::from(masque_du_prefixe(prefixe));
    let hote = u32::from(ip) & !masque;
    hote != 0 && hote != !masque
}

/// **LIRE CE QUE LA PAGE DEMANDE**, sans rien en croire :
/// - `index` : UN index de carte, gardé seulement s'il est dans `connues` ;
/// - `quoi` : `auto`, ou `adresse,masque,passerelle`.
pub fn lire_demande(index: &str, quoi: &str, connues: &[CarteConnue]) -> Option<DemandeIp> {
    let index: u32 = index.trim().parse().ok()?;
    let carte = connues.iter().find(|c| c.index == index)?;
    if quoi == "auto" {
        return Some(DemandeIp::Automatique { index });
    }
    let mut morceaux = quoi.split(',');
    let (adresse, masque, passerelle) = (morceaux.next()?, morceaux.next()?, morceaux.next()?);
    if morceaux.next().is_some() {
        return None;
    }
    // `Ipv4Addr::from_str` refuse espaces, guillemets, points-virgules.
    let adresse: Ipv4Addr = adresse.trim().parse().ok()?;
    let passerelle: Ipv4Addr = passerelle.trim().parse().ok()?;
    let prefixe = prefixe_du_masque(masque)?;
    let masque = u32::from(masque_du_prefixe(prefixe));
    let meme_reseau = u32::from(adresse) & masque == u32::from(passerelle) & masque;
    if !est_un_hote(adresse, prefixe) || !est_un_hote(passerelle, prefixe) || !meme_reseau || adresse == passerelle {
        return None;
    }
    let dns = if carte.dns_manuel {
        Vec::new()
    } else {
        let mut dns: Vec<IpAddr> = Vec::new();
        for ip in carte.serveurs.iter().filter_map(|s| s.parse::<IpAddr>().ok()) {
            if !ip.is_unspecified() && !ip.is_multicast() && !dns.contains(&ip) && dns.len() < 8 {
                dns.push(ip);
            }
        }
        dns
    };
    Some(DemandeIp::Manuelle { index, adresse, prefixe, passerelle, dns })
}

/// `fe80::/10`.
fn est_lien_local6(ip: &Ipv6Addr) -> bool {
    ip.segments()[0] & 0xffc0 == 0xfe80
}

/// **LIRE UNE DEMANDE IPv6** : `quoi` = `auto`, ou `adresse,préfixe,passerelle`
/// (passerelle vide permise). Même garde que `lire_demande`.
pub fn lire_demande6(index: &str, quoi: &str, connues: &[CarteConnue]) -> Option<DemandeIp> {
    let index: u32 = index.trim().parse().ok()?;
    connues.iter().find(|c| c.index == index)?;
    if quoi == "auto" {
        return Some(DemandeIp::Automatique6 { index });
    }
    let mut morceaux = quoi.split(',');
    let (adresse, prefixe, passerelle) = (morceaux.next()?, morceaux.next()?, morceaux.next()?);
    if morceaux.next().is_some() {
        return None;
    }
    // `Ipv6Addr::from_str` refuse espaces, guillemets et zones `%`.
    let adresse: Ipv6Addr = adresse.trim().parse().ok()?;
    let prefixe: u8 = prefixe.trim().trim_start_matches('/').parse().ok()?;
    if !(1..=128).contains(&prefixe) {
        return None;
    }
    let speciale = |ip: &Ipv6Addr| ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() || ip.to_ipv4_mapped().is_some();
    if speciale(&adresse) || est_lien_local6(&adresse) {
        return None;
    }
    let passerelle = match passerelle.trim() {
        "" => None,
        p => {
            let p: Ipv6Addr = p.parse().ok()?;
            if speciale(&p) || p == adresse {
                return None;
            }
            Some(p)
        }
    };
    Some(DemandeIp::Manuelle6 { index, adresse, prefixe, passerelle })
}

/// Le script PowerShell (élevé) d'une demande. N'y entrent qu'un `u32`, un
/// `u8` et des adresses RÉÉCRITES depuis leur valeur parsée.
pub fn script(demande: &DemandeIp) -> String {
    match demande {
        DemandeIp::Automatique { index } => format!(
            "$ErrorActionPreference='Stop'; try {{ \
             if ((Get-NetIPInterface -InterfaceIndex {index} -AddressFamily IPv4).Dhcp -eq 'Disabled') {{ \
             Get-NetIPAddress -InterfaceIndex {index} -AddressFamily IPv4 -ErrorAction SilentlyContinue | Remove-NetIPAddress -Confirm:$false; \
             Get-NetRoute -InterfaceIndex {index} -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue | Remove-NetRoute -Confirm:$false; \
             Set-NetIPInterface -InterfaceIndex {index} -AddressFamily IPv4 -Dhcp Enabled }}; \
             exit 0 }} catch {{ exit 1 }}"
        ),
        DemandeIp::Manuelle { index, adresse, prefixe, passerelle, dns } => {
            let dns = if dns.is_empty() {
                String::new()
            } else {
                let liste = dns.iter().map(|ip| format!("'{ip}'")).collect::<Vec<_>>().join(",");
                format!("Set-DnsClientServerAddress -InterfaceIndex {index} -ServerAddresses @({liste}); ")
            };
            format!(
                "$ErrorActionPreference='Stop'; try {{ \
                 {dns}\
                 Set-NetIPInterface -InterfaceIndex {index} -AddressFamily IPv4 -Dhcp Disabled; \
                 Get-NetIPAddress -InterfaceIndex {index} -AddressFamily IPv4 -ErrorAction SilentlyContinue | Remove-NetIPAddress -Confirm:$false; \
                 Get-NetRoute -InterfaceIndex {index} -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue | Remove-NetRoute -Confirm:$false; \
                 New-NetIPAddress -InterfaceIndex {index} -AddressFamily IPv4 -IPAddress '{adresse}' -PrefixLength {prefixe} -DefaultGateway '{passerelle}' | Out-Null; \
                 exit 0 }} catch {{ try {{ Set-NetIPInterface -InterfaceIndex {index} -AddressFamily IPv4 -Dhcp Enabled }} catch {{ }}; exit 1 }}"
            )
        }
        DemandeIp::Automatique6 { index } => {
            format!("$ErrorActionPreference='Stop'; try {{ {} exit 0 }} catch {{ exit 1 }}", retirer_le_manuel6(*index))
        }
        DemandeIp::Manuelle6 { index, adresse, prefixe, passerelle } => {
            // ⛔ PAS de `-DefaultGateway` : la route `::/0` que le routeur annonce
            // passe souvent par la MÊME passerelle, et la recréer échouerait
            // (« existe déjà »). La route n'est posée que si elle manque.
            let porte = passerelle
                .map(|p| {
                    format!(
                        "if (-not (Get-NetRoute -InterfaceIndex {index} -DestinationPrefix '::/0' -NextHop '{p}' -ErrorAction SilentlyContinue)) {{ \
                         New-NetRoute -InterfaceIndex {index} -DestinationPrefix '::/0' -NextHop '{p}' | Out-Null }}; "
                    )
                })
                .unwrap_or_default();
            let retirer = retirer_le_manuel6(*index);
            format!(
                "$ErrorActionPreference='Stop'; try {{ {retirer} \
                 New-NetIPAddress -InterfaceIndex {index} -AddressFamily IPv6 -IPAddress '{adresse}' -PrefixLength {prefixe} | Out-Null; \
                 {porte}exit 0 }} catch {{ try {{ {retirer} }} catch {{ }}; exit 1 }}"
            )
        }
    }
}

/// Retire les adresses IPv6 et la route par défaut POSÉES À LA MAIN — jamais
/// celles que le routeur annonce.
fn retirer_le_manuel6(index: u32) -> String {
    format!(
        "Get-NetIPAddress -InterfaceIndex {index} -AddressFamily IPv6 -ErrorAction SilentlyContinue | \
         Where-Object {{ $_.PrefixOrigin -eq 'Manual' }} | Remove-NetIPAddress -Confirm:$false; \
         Get-NetRoute -InterfaceIndex {index} -DestinationPrefix '::/0' -ErrorAction SilentlyContinue | \
         Where-Object {{ $_.Protocol -eq 'NetMgmt' }} | Remove-NetRoute -Confirm:$false;"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carte(index: u32, serveurs: &[String], dns_manuel: bool) -> CarteConnue<'_> {
        CarteConnue { index, serveurs, dns_manuel }
    }

    #[test]
    fn un_masque_se_lit_sous_trois_formes() {
        assert_eq!(prefixe_du_masque("24"), Some(24));
        assert_eq!(prefixe_du_masque("/16"), Some(16));
        assert_eq!(prefixe_du_masque("255.255.255.0"), Some(24));
        assert_eq!(prefixe_du_masque("255.255.252.0"), Some(22));
        assert_eq!(prefixe_du_masque("255.0.255.0"), None, "des 1 non contigus");
        assert_eq!(prefixe_du_masque("0"), None);
        assert_eq!(prefixe_du_masque("31"), None, "aucune place pour un hôte et une passerelle");
        assert_eq!(prefixe_du_masque("24; calc"), None);
        assert_eq!(masque_du_prefixe(24), "255.255.255.0".parse::<Ipv4Addr>().unwrap());
        assert_eq!(masque_du_prefixe(22), "255.255.252.0".parse::<Ipv4Addr>().unwrap());
    }

    #[test]
    fn une_adresse_manuelle_garde_les_dns_du_dhcp() {
        let serveurs = ["192.168.1.188".to_string(), "8.8.8.8".to_string()];
        let d = lire_demande("12", "192.168.1.50,255.255.255.0,192.168.1.1", &[carte(12, &serveurs, false)]).unwrap();
        assert_eq!(
            d,
            DemandeIp::Manuelle {
                index: 12,
                adresse: "192.168.1.50".parse().unwrap(),
                prefixe: 24,
                passerelle: "192.168.1.1".parse().unwrap(),
                dns: vec!["192.168.1.188".parse().unwrap(), "8.8.8.8".parse().unwrap()],
            }
        );
        // Des DNS déjà posés à la main : on n'y touche pas.
        let d = lire_demande("12", "192.168.1.50,24,192.168.1.1", &[carte(12, &serveurs, true)]).unwrap();
        assert!(matches!(d, DemandeIp::Manuelle { ref dns, .. } if dns.is_empty()));
        assert_eq!(lire_demande("12", "auto", &[carte(12, &serveurs, false)]), Some(DemandeIp::Automatique { index: 12 }));
    }

    #[test]
    fn une_demande_fausse_ou_hostile_est_refusee() {
        let connues = [carte(12, &[], false)];
        for quoi in [
            "192.168.1.50,24,10.0.0.1",                    // passerelle hors du réseau
            "192.168.1.50,24,192.168.1.50",                // passerelle = adresse
            "192.168.1.0,24,192.168.1.1",                  // adresse du réseau
            "192.168.1.255,24,192.168.1.1",                // diffusion
            "169.254.3.4,16,169.254.0.1",                  // lien local
            "127.0.0.2,8,127.0.0.1",                       // bouclage
            "192.168.1.50,24",                             // pas de passerelle
            "192.168.1.50,24,192.168.1.1,8.8.8.8",         // un morceau de trop
            "192.168.1.50',24,192.168.1.1",                // guillemet
            "192.168.1.50; Remove-Item C:\\,24,192.168.1.1",
            "fe80::1,64,fe80::2",
            "",
        ] {
            assert_eq!(lire_demande("12", quoi, &connues), None, "accepté à tort : {quoi:?}");
        }
        assert_eq!(lire_demande("13", "auto", &connues), None, "carte inconnue");
        assert_eq!(lire_demande("12,7", "auto", &connues), None, "une seule carte à la fois");
        assert_eq!(lire_demande("", "auto", &connues), None);
    }

    #[test]
    fn le_script_ne_porte_que_des_valeurs_reecrites() {
        let serveurs = ["192.168.1.188".to_string()];
        let d = lire_demande("7", "192.168.1.050,24,192.168.1.1", &[carte(7, &serveurs, false)]);
        // `Ipv4Addr` refuse les zéros de tête : rien d'ambigu n'arrive au script.
        assert_eq!(d, None);
        let d = lire_demande("7", " 192.168.1.50 , /24 , 192.168.1.1 ", &[carte(7, &serveurs, false)]).unwrap();
        let s = script(&d);
        assert!(s.contains("Set-DnsClientServerAddress -InterfaceIndex 7 -ServerAddresses @('192.168.1.188'); "));
        assert!(s.contains("Set-NetIPInterface -InterfaceIndex 7 -AddressFamily IPv4 -Dhcp Disabled; "));
        assert!(s.contains("-IPAddress '192.168.1.50' -PrefixLength 24 -DefaultGateway '192.168.1.1'"));
        // Les DNS sont posés AVANT de couper le DHCP, le DHCP revient en cas d'échec.
        assert!(s.find("Set-DnsClientServerAddress").unwrap() < s.find("-Dhcp Disabled").unwrap());
        assert!(s.contains("catch { try { Set-NetIPInterface -InterfaceIndex 7 -AddressFamily IPv4 -Dhcp Enabled }"));
        let auto = script(&DemandeIp::Automatique { index: 7 });
        assert!(auto.contains(".Dhcp -eq 'Disabled') {"), "la route n'est retirée que si le DHCP était coupé");
        assert!(auto.contains("-Dhcp Enabled"));
    }

    #[test]
    fn une_demande_ipv6_est_lue_sans_rien_croire() {
        let connues = [carte(12, &[], false)];
        assert_eq!(
            lire_demande6("12", "2001:db8::50,64,fe80::1", &connues),
            Some(DemandeIp::Manuelle6 {
                index: 12,
                adresse: "2001:db8::50".parse().unwrap(),
                prefixe: 64,
                passerelle: Some("fe80::1".parse().unwrap())
            })
        );
        assert!(matches!(lire_demande6("12", "2001:db8::50,/64,", &connues), Some(DemandeIp::Manuelle6 { passerelle: None, .. })));
        assert_eq!(lire_demande6("12", "auto", &connues), Some(DemandeIp::Automatique6 { index: 12 }));
        for quoi in [
            "fe80::50,64,fe80::1",          // lien local
            "::1,128,",                     // bouclage
            "ff02::1,64,",                  // multidiffusion
            "::ffff:192.168.1.5,96,",       // IPv4 déguisée
            "2001:db8::50,0,",              // préfixe nul
            "2001:db8::50,129,",            // préfixe trop long
            "2001:db8::50,64,2001:db8::50", // passerelle = adresse
            "2001:db8::50%12,64,",          // zone
            "2001:db8::50,64",              // morceau manquant
            "2001:db8::50',64,",
            "192.168.1.5,24,192.168.1.1",
        ] {
            assert_eq!(lire_demande6("12", quoi, &connues), None, "accepté à tort : {quoi:?}");
        }
        assert_eq!(lire_demande6("13", "auto", &connues), None, "carte inconnue");
    }

    #[test]
    fn le_script_ipv6_ne_touche_pas_aux_adresses_annoncees() {
        let auto = script(&DemandeIp::Automatique6 { index: 7 });
        assert!(auto.contains("Where-Object { $_.PrefixOrigin -eq 'Manual' }"));
        assert!(auto.contains("Where-Object { $_.Protocol -eq 'NetMgmt' }"));
        assert!(!auto.contains("-Dhcp"), "le DHCP IPv4 n'est pas concerné");
        let d = lire_demande6("7", "2001:db8::50,64,fe80::1", &[carte(7, &[], false)]).unwrap();
        let s = script(&d);
        assert!(s.contains("-AddressFamily IPv6 -IPAddress '2001:db8::50' -PrefixLength 64 | Out-Null"));
        assert!(!s.contains("-DefaultGateway"), "la route du routeur existe souvent déjà");
        assert!(s.contains("if (-not (Get-NetRoute -InterfaceIndex 7 -DestinationPrefix '::/0' -NextHop 'fe80::1' -ErrorAction SilentlyContinue)) {"));
        assert!(s.contains("New-NetRoute -InterfaceIndex 7 -DestinationPrefix '::/0' -NextHop 'fe80::1'"));
        assert!(s.find("PrefixOrigin").unwrap() < s.find("New-NetIPAddress").unwrap(), "l'ancienne adresse manuelle part d'abord");
        let sans = script(&lire_demande6("7", "2001:db8::50,64,", &[carte(7, &[], false)]).unwrap());
        assert!(!sans.contains("New-NetRoute"));
    }
}
