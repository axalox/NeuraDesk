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

use std::net::{IpAddr, Ipv4Addr};

/// Ce qu'on demande pour l'adresse IPv4 d'UNE carte.
#[derive(Clone, Debug, PartialEq)]
pub enum DemandeIp {
    /// Adresse, masque et passerelle donnés par le réseau.
    Automatique { index: u32 },
    /// Adresse fixe. `dns` : les serveurs à poser à la main (vide = garder
    /// ceux déjà posés à la main).
    Manuelle { index: u32, adresse: Ipv4Addr, prefixe: u8, passerelle: Ipv4Addr, dns: Vec<IpAddr> },
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
    }
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
}
