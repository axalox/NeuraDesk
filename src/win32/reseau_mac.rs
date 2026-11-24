//! **L'ADRESSE MAC D'UNE CARTE** — onglet « Adresse MAC » de l'applet
//! « Modifier réglages réseau » (voir `win32::cef_dns`, `win32::dns`).
//!
//! DEMANDE (2026-11-24) : « ajouter la possibilité de changer l'adresse mac des
//! cartes réseau en cherchant sur github si nécessaire ».
//!
//! La méthode de tous les outils relevés sur GitHub (davidbombal/red-python-
//! scripts `change-windows10-mac-address.py`, angusshire/mac-spoofer,
//! Ivenzzz/mac-changer, sietseringers/MACAddressTool) :
//! - la sous-clé `Control\Class\{4d36e972-…}\NNNN` dont `NetCfgInstanceId` est le
//!   GUID de la carte reçoit `NetworkAddress` = 12 chiffres hexadécimaux ;
//!   l'effacer rend l'adresse d'usine ;
//! - la carte est redémarrée (désactivée puis réactivée) pour la prendre.
//!
//! ⛔ **LE 2ᵉ CHIFFRE DOIT ÊTRE 2, 6, A OU E** (adresse « administrée
//! localement », monodiffusion) : depuis Windows 10 la plupart des pilotes Wi-Fi
//! et plusieurs pilotes Intel IGNORENT EN SILENCE toute autre valeur. Imposé ici
//! sur toutes les cartes.
//!
//! ⛔ **UN PILOTE PEUT IGNORER `NetworkAddress`** : le script relit l'adresse
//! après le redémarrage et sort en 3 si elle n'a pas changé → `Issue::Ignoree`,
//! jamais un « réussi » mensonger.

use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;

/// La classe « cartes réseau » du registre.
const CLASSE: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e972-e325-11ce-bfc1-08002be10318}";
/// Le code de sortie du script quand le pilote a ignoré l'adresse.
pub const CODE_IGNOREE: u32 = 3;
/// Les secondes laissées à la carte pour revenir avec sa nouvelle adresse.
const RETOUR_MAX_S: u32 = 20;

/// Ce qu'on demande pour l'adresse MAC d'UNE carte.
#[derive(Clone, Debug, PartialEq)]
pub enum DemandeMac {
    /// Poser cette adresse.
    Poser { guid: String, adresse: [u8; 6] },
    /// Revenir à l'adresse d'usine.
    Origine { guid: String },
}

/// Lit `AA:BB:CC:DD:EE:FF`, `AA-BB-…` ou `AABBCCDDEEFF`.
pub fn lire_adresse(s: &str) -> Option<[u8; 6]> {
    let chiffres: String = s.trim().chars().filter(|c| *c != ':' && *c != '-').collect();
    if chiffres.len() != 12 || !chiffres.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    // Des séparateurs : seulement entre des paires (« A:ABB… » est refusé).
    let brut = s.trim();
    if brut.len() != 12 && !(brut.len() == 17 && brut.char_indices().all(|(i, c)| if i % 3 == 2 { c == ':' || c == '-' } else { c.is_ascii_hexdigit() })) {
        return None;
    }
    let mut octets = [0u8; 6];
    for (i, o) in octets.iter_mut().enumerate() {
        *o = u8::from_str_radix(&chiffres[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(octets)
}

/// Une adresse qu'on peut poser : monodiffusion, administrée localement
/// (2ᵉ chiffre 2, 6, A ou E), ni nulle ni la diffusion.
pub fn est_posable(a: &[u8; 6]) -> bool {
    a[0] & 0b11 == 0b10 && *a != [0; 6] && *a != [0xff; 6]
}

/// L'écriture affichée : `AA:BB:CC:DD:EE:FF`.
pub fn texte(a: &[u8]) -> String {
    a.iter().map(|o| format!("{o:02X}")).collect::<Vec<_>>().join(":")
}

/// L'adresse posée à la main sur cette carte (registre), s'il y en a une.
pub fn adresse_posee(guid: &str) -> Option<String> {
    if !crate::win32::dns::guid_valide(guid) {
        return None;
    }
    let reg = crate::win32::registry_util::read_string_value;
    crate::win32::registry_util::enumerate_subkeys(HKEY_LOCAL_MACHINE, CLASSE)
        .into_iter()
        .filter(|n| n.len() == 4 && n.chars().all(|c| c.is_ascii_digit()))
        .find(|n| reg(HKEY_LOCAL_MACHINE, &format!("{CLASSE}\\{n}"), "NetCfgInstanceId").is_some_and(|id| id.eq_ignore_ascii_case(guid)))
        .and_then(|n| reg(HKEY_LOCAL_MACHINE, &format!("{CLASSE}\\{n}"), "NetworkAddress"))
        .filter(|v| !v.trim().is_empty())
}

/// **LIRE CE QUE LA PAGE DEMANDE** : `quoi` = une adresse, ou `origine`.
/// `guid` vient de NOTRE énumération (validé par `dns::guid_valide`).
pub fn lire_demande(guid: &str, quoi: &str) -> Option<DemandeMac> {
    if !crate::win32::dns::guid_valide(guid) {
        return None;
    }
    if quoi == "origine" {
        return Some(DemandeMac::Origine { guid: guid.to_string() });
    }
    let adresse = lire_adresse(quoi)?;
    est_posable(&adresse).then(|| DemandeMac::Poser { guid: guid.to_string(), adresse })
}

/// Le script PowerShell (élevé). N'y entrent qu'un GUID validé et 12 chiffres
/// réécrits depuis les octets.
pub fn script(demande: &DemandeMac) -> String {
    let (guid, ecrire, attendue) = match demande {
        DemandeMac::Poser { guid, adresse } => {
            let hex: String = adresse.iter().map(|o| format!("{o:02X}")).collect();
            (guid, format!("Set-ItemProperty -LiteralPath $cle.PSPath -Name NetworkAddress -Value '{hex}' -Type String; "), format!("'{hex}'"))
        }
        DemandeMac::Origine { guid } => (
            guid,
            "Remove-ItemProperty -LiteralPath $cle.PSPath -Name NetworkAddress -ErrorAction SilentlyContinue; ".to_string(),
            "$carte.PermanentAddress".to_string(),
        ),
    };
    format!(
        "$ErrorActionPreference='Stop'; try {{ \
         $cle = Get-ChildItem -LiteralPath 'HKLM:\\{CLASSE}' -ErrorAction SilentlyContinue | Where-Object {{ \
         (Get-ItemProperty -LiteralPath $_.PSPath -Name NetCfgInstanceId -ErrorAction SilentlyContinue).NetCfgInstanceId -eq '{guid}' }} | Select-Object -First 1; \
         $carte = Get-NetAdapter -IncludeHidden | Where-Object {{ $_.InterfaceGuid -eq '{guid}' }} | Select-Object -First 1; \
         if (-not $cle -or -not $carte) {{ exit 1 }}; \
         $attendue = {attendue}; \
         {ecrire}\
         Restart-NetAdapter -InputObject $carte -Confirm:$false; \
         if (-not $attendue) {{ exit 0 }}; \
         for ($i = 0; $i -lt {RETOUR_MAX_S}; $i++) {{ \
         $lue = ((Get-NetAdapter -IncludeHidden | Where-Object {{ $_.InterfaceGuid -eq '{guid}' }} | Select-Object -First 1).MacAddress) -replace '-', ''; \
         if ($lue -eq $attendue) {{ exit 0 }}; Start-Sleep -Seconds 1 }}; \
         exit {CODE_IGNOREE} }} catch {{ exit 1 }}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUID: &str = "{01234567-89AB-CDEF-0123-456789ABCDEF}";

    #[test]
    fn une_adresse_se_lit_sous_trois_formes() {
        let attendue = Some([0x02, 0x1a, 0x2b, 0x3c, 0x4d, 0x5e]);
        assert_eq!(lire_adresse("02:1A:2B:3C:4D:5E"), attendue);
        assert_eq!(lire_adresse("02-1a-2b-3c-4d-5e"), attendue);
        assert_eq!(lire_adresse("021A2B3C4D5E"), attendue);
        for faux in ["02:1A:2B:3C:4D", "02:1A:2B:3C:4D:5E:6F", "0:21A:2B:3C:4D:5E", "02:1A:2B:3C:4D:5G", "02 1A 2B 3C 4D 5E", "021A2B3C4D5E'; calc"] {
            assert_eq!(lire_adresse(faux), None, "accepté à tort : {faux:?}");
        }
    }

    #[test]
    fn seule_une_adresse_locale_en_monodiffusion_se_pose() {
        for bon in ["02:00:00:00:00:01", "06:11:22:33:44:55", "0A:11:22:33:44:55", "DE:AD:BE:EF:00:01"] {
            assert!(est_posable(&lire_adresse(bon).unwrap()), "refusée à tort : {bon}");
        }
        // 2ᵉ chiffre 0 (adresse d'usine), 1/3 (multidiffusion), diffusion, nulle.
        for faux in ["00:11:22:33:44:55", "01:00:5E:00:00:01", "03:11:22:33:44:55", "FF:FF:FF:FF:FF:FF", "00:00:00:00:00:00", "04:11:22:33:44:55"] {
            assert!(!est_posable(&lire_adresse(faux).unwrap()), "acceptée à tort : {faux}");
        }
    }

    #[test]
    fn une_demande_est_lue_sans_rien_croire() {
        assert_eq!(lire_demande(GUID, "origine"), Some(DemandeMac::Origine { guid: GUID.into() }));
        assert_eq!(
            lire_demande(GUID, "02:1A:2B:3C:4D:5E"),
            Some(DemandeMac::Poser { guid: GUID.into(), adresse: [0x02, 0x1a, 0x2b, 0x3c, 0x4d, 0x5e] })
        );
        assert_eq!(lire_demande(GUID, "00:1A:2B:3C:4D:5E"), None, "adresse d'usine");
        assert_eq!(lire_demande("{0123'; calc}", "origine"), None, "GUID invalide");
    }

    #[test]
    fn le_script_pose_redemarre_et_verifie() {
        let s = script(&lire_demande(GUID, "02:1a:2b:3c:4d:5e").unwrap());
        assert!(s.contains("-Name NetworkAddress -Value '021A2B3C4D5E' -Type String"));
        assert!(s.contains(&format!("NetCfgInstanceId -eq '{GUID}'")));
        assert!(s.find("NetworkAddress -Value").unwrap() < s.find("Restart-NetAdapter").unwrap());
        assert!(s.contains("$attendue = '021A2B3C4D5E'"));
        assert!(s.contains(&format!("exit {CODE_IGNOREE} }}")), "un pilote qui ignore l'adresse rend 3");
        let o = script(&DemandeMac::Origine { guid: GUID.into() });
        assert!(o.contains("Remove-ItemProperty -LiteralPath $cle.PSPath -Name NetworkAddress"));
        assert!(o.contains("$attendue = $carte.PermanentAddress"));
    }

    #[test]
    fn le_texte_est_en_paires_majuscules() {
        assert_eq!(texte(&[0x02, 0x1a, 0, 0xff, 0x10, 0x0b]), "02:1A:00:FF:10:0B");
    }
}
