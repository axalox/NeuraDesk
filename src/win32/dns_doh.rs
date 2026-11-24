//! **LE DNS CHIFFRÉ (DNS over HTTPS)** d'une carte — onglet DNS de l'applet
//! « Modifier réglages réseau » (voir `win32::dns`, `win32::cef_dns`).
//!
//! DEMANDE (2026-11-24) : « peux tu ajouter ces réglages dans la fenêtre de
//! l'applet » — dont le DNS chiffré que Windows 11 propose avec les serveurs.
//!
//! Ce que fait Windows 11 (relevé, aucune API publique ne le pose) :
//! - une table GLOBALE adresse → modèle d'URL (`Add-DnsClientDohServerAddress`) ;
//!   Cloudflare, Google et Quad9 y sont déjà ;
//! - par CARTE, une clé `Dnscache\InterfaceSpecificParameters\{guid}\
//!   DohInterfaceSettings\Doh\<ip>` (`Doh6` en IPv6) avec `DohFlags` = 1 (QWORD,
//!   « modèle automatique »). SANS cette valeur le chiffrement reste éteint :
//!   ni `netsh` ni les cmdlets ne la posent.
//!   Source : github.com/helgeklein/Win-DNS-DoH-PoSh (README, scripts Enable-*).
//!
//! ⛔ **LES ANCIENNES CLÉS PAR ADRESSE RESTENT** si on ne les efface pas :
//! changer de serveurs ou revenir en automatique efface `Doh` et `Doh6` de la
//! carte avant de poser les nouvelles.
//!
//! ⛔ Seuls les serveurs de `MODELES` se chiffrent : la page n'envoie qu'un
//! drapeau, jamais d'URL. Les clés sont écrites par `Microsoft.Win32.Registry`
//! (.NET) : un chemin de fournisseur PowerShell lirait `:` d'une adresse IPv6.

use std::net::IpAddr;

use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;

/// Les modèles DoH des serveurs des préréglages (adresse → URL).
const MODELES: &[(&str, &str)] = &[
    ("1.1.1.1", "https://cloudflare-dns.com/dns-query"),
    ("1.0.0.1", "https://cloudflare-dns.com/dns-query"),
    ("2606:4700:4700::1111", "https://cloudflare-dns.com/dns-query"),
    ("2606:4700:4700::1001", "https://cloudflare-dns.com/dns-query"),
    ("8.8.8.8", "https://dns.google/dns-query"),
    ("8.8.4.4", "https://dns.google/dns-query"),
    ("2001:4860:4860::8888", "https://dns.google/dns-query"),
    ("2001:4860:4860::8844", "https://dns.google/dns-query"),
    ("9.9.9.9", "https://dns.quad9.net/dns-query"),
    ("149.112.112.112", "https://dns.quad9.net/dns-query"),
    ("2620:fe::fe", "https://dns.quad9.net/dns-query"),
    ("2620:fe::9", "https://dns.quad9.net/dns-query"),
    ("94.140.14.14", "https://dns.adguard-dns.com/dns-query"),
    ("94.140.15.15", "https://dns.adguard-dns.com/dns-query"),
    ("2a10:50c0::ad1:ff", "https://dns.adguard-dns.com/dns-query"),
    ("2a10:50c0::ad2:ff", "https://dns.adguard-dns.com/dns-query"),
    ("208.67.222.222", "https://doh.opendns.com/dns-query"),
    ("208.67.220.220", "https://doh.opendns.com/dns-query"),
    ("2620:119:35::35", "https://doh.opendns.com/dns-query"),
    ("2620:119:53::53", "https://doh.opendns.com/dns-query"),
];

/// La clé (sous HKLM) des réglages DNS propres à une carte.
const RACINE: &str = r"SYSTEM\CurrentControlSet\Services\Dnscache\InterfaceSpecificParameters";

/// Le modèle DoH connu d'une adresse.
pub fn modele(ip: &IpAddr) -> Option<&'static str> {
    MODELES.iter().find(|(a, _)| a.parse::<IpAddr>().ok().as_ref() == Some(ip)).map(|(_, m)| *m)
}

/// Toutes ces adresses ont un modèle (une liste vide ne se chiffre pas).
pub fn chiffrable(adresses: &[IpAddr]) -> bool {
    !adresses.is_empty() && adresses.iter().all(|ip| modele(ip).is_some())
}

/// Les adresses que cette carte chiffre (sans droit : lecture de HKLM).
pub fn serveurs_chiffres(guid: &str) -> Vec<String> {
    if !crate::win32::dns::guid_valide(guid) {
        return Vec::new();
    }
    ["Doh", "Doh6"]
        .iter()
        .flat_map(|famille| {
            crate::win32::registry_util::enumerate_subkeys(HKEY_LOCAL_MACHINE, &format!("{RACINE}\\{guid}\\DohInterfaceSettings\\{famille}"))
        })
        .filter(|ip| ip.parse::<IpAddr>().is_ok())
        .collect()
}

/// Le morceau de script (élevé) : efface le chiffrement des cartes, puis le
/// pose pour `adresses` si `chiffrer`. Les GUID sont VALIDÉS par l'appelant
/// (`dns::lire_demande`), les adresses réécrites depuis leur valeur parsée.
pub fn script(guids: &[&str], adresses: &[IpAddr], chiffrer: bool) -> String {
    let mut s = String::new();
    for guid in guids {
        for famille in ["Doh", "Doh6"] {
            s.push_str(&format!(
                "[Microsoft.Win32.Registry]::LocalMachine.DeleteSubKeyTree('{RACINE}\\{guid}\\DohInterfaceSettings\\{famille}', $false); "
            ));
        }
    }
    if !chiffrer {
        return s;
    }
    for ip in adresses {
        let Some(modele) = modele(ip) else { continue };
        s.push_str(&format!(
            "if (-not (Get-DnsClientDohServerAddress -ServerAddress '{ip}' -ErrorAction SilentlyContinue)) {{ \
             Add-DnsClientDohServerAddress -ServerAddress '{ip}' -DohTemplate '{modele}' -AllowFallbackToUdp $False -AutoUpgrade $True | Out-Null }}; "
        ));
    }
    for guid in guids {
        for ip in adresses {
            let famille = if ip.is_ipv6() { "Doh6" } else { "Doh" };
            s.push_str(&format!(
                "$k = [Microsoft.Win32.Registry]::LocalMachine.CreateSubKey('{RACINE}\\{guid}\\DohInterfaceSettings\\{famille}\\{ip}'); \
                 $k.SetValue('DohFlags', [long]1, 'QWord'); $k.Close(); "
            ));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chaque_modele_est_une_adresse_et_une_url_https() {
        for (ip, url) in MODELES {
            assert!(ip.parse::<IpAddr>().is_ok(), "adresse illisible : {ip}");
            assert!(url.starts_with("https://") && url.ends_with("/dns-query"), "modèle inattendu : {url}");
            assert!(url.chars().all(|c| c.is_ascii_alphanumeric() || "/:.-".contains(c)), "caractère suspect : {url}");
        }
    }

    #[test]
    fn seuls_les_serveurs_connus_se_chiffrent() {
        let cf: Vec<IpAddr> = vec!["1.1.1.1".parse().unwrap(), "2606:4700:4700::1111".parse().unwrap()];
        assert!(chiffrable(&cf));
        // Écrite autrement, la même adresse IPv6 est reconnue.
        assert!(modele(&"2606:4700:4700:0:0:0:0:1111".parse().unwrap()).is_some());
        assert!(!chiffrable(&["1.1.1.1".parse().unwrap(), "192.168.1.1".parse().unwrap()]));
        assert!(!chiffrable(&[]));
    }

    #[test]
    fn le_script_efface_puis_pose_par_famille() {
        let guid = "{01234567-89AB-CDEF-0123-456789ABCDEF}";
        let adresses: Vec<IpAddr> = vec!["9.9.9.9".parse().unwrap(), "2620:fe::fe".parse().unwrap()];
        let s = script(&[guid], &adresses, true);
        let efface = s.find("DeleteSubKeyTree").unwrap();
        assert!(efface < s.find("CreateSubKey").unwrap(), "l'ancien chiffrement est effacé AVANT");
        assert!(s.contains(&format!("{guid}\\DohInterfaceSettings\\Doh\\9.9.9.9')")));
        assert!(s.contains(&format!("{guid}\\DohInterfaceSettings\\Doh6\\2620:fe::fe')")));
        assert!(s.contains("-DohTemplate 'https://dns.quad9.net/dns-query'"));
        assert!(s.contains("$k.SetValue('DohFlags', [long]1, 'QWord')"));
        let sans = script(&[guid], &adresses, false);
        assert!(sans.contains("DeleteSubKeyTree") && !sans.contains("CreateSubKey"), "sans chiffrement : effacer seulement");
    }
}
