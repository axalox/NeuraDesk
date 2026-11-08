//! ⭐ **LE DÉBIT RÉSEAU** — DEMANDE EXPLICITE (2026-10-14) : « une applet
//! netspeed qui affiche la vitesse d'envoi et la vitesse de réception du réseau à
//! la gauche de l'icône réseau de la barre du haut ». L'affichage est dans
//! `ui::debit`.
//!
//! `GetIfTable2` rend des compteurs d'octets CUMULÉS par interface : le débit est
//! la différence entre deux relevés, divisée par le temps écoulé (même principe
//! que `sysinfo::CpuSampler`). Relevé à la cadence du moniteur système (voir
//! `app::SYS_MONITOR_REFRESH_INTERVAL`).
//!
//! ⚠️ **SEULES LES INTERFACES MATÉRIELLES, HORS FILTRES, COMPTENT.** La table
//! contient aussi les sous-interfaces de FILTRE (QoS, WFP, pilotes de
//! virtualisation) empilées sur la carte réelle : elles portent les MÊMES
//! octets, et les sommer doublerait ou triplerait le débit. La boucle locale
//! (`IF_TYPE_SOFTWARE_LOOPBACK`) est exclue de même.

use std::time::Instant;

use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;

/// `IF_TYPE_SOFTWARE_LOOPBACK` (ipifcons.h).
const TYPE_BOUCLE_LOCALE: u32 = 24;
/// Bits de `InterfaceAndOperStatusFlags` (netioapi.h) : `HardwareInterface`,
/// puis `FilterInterface`.
const DRAPEAU_MATERIEL: u8 = 1 << 0;
const DRAPEAU_FILTRE: u8 = 1 << 1;

/// `(envoyés, reçus)` cumulés, en octets, sur les interfaces matérielles
/// actives. `None` si l'appel échoue.
fn compteurs() -> Option<(u64, u64)> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    if unsafe { GetIfTable2(&mut table) }.is_err() || table.is_null() {
        return None;
    }
    // SAFETY : `table` est valide jusqu'à `FreeMibTable` ; `Table` est un tableau
    // de longueur variable de `NumEntries` lignes (le `[_; 1]` est la convention C).
    let somme = unsafe {
        let n = (*table).NumEntries as usize;
        let lignes = std::slice::from_raw_parts((*table).Table.as_ptr(), n);
        lignes
            .iter()
            .filter(|l| {
                let d = l.InterfaceAndOperStatusFlags._bitfield;
                d & DRAPEAU_MATERIEL != 0 && d & DRAPEAU_FILTRE == 0 && l.Type != TYPE_BOUCLE_LOCALE && l.OperStatus == IfOperStatusUp
            })
            .fold((0u64, 0u64), |(e, r), l| (e.wrapping_add(l.OutOctets), r.wrapping_add(l.InOctets)))
    };
    unsafe { FreeMibTable(table as *const _) };
    Some(somme)
}

/// Échantillonneur : garde le relevé précédent.
#[derive(Default)]
pub struct Echantillonneur {
    precedent: Option<(Instant, u64, u64)>,
}

impl Echantillonneur {
    pub fn new() -> Self {
        Self::default()
    }

    /// `(envoi, réception)` en octets par seconde depuis le dernier appel —
    /// `None` au premier appel (pas encore de référence) ou si l'appel échoue.
    pub fn echantillonner(&mut self) -> Option<(f64, f64)> {
        let (envoyes, recus) = compteurs()?;
        let maintenant = Instant::now();
        let precedent = self.precedent.replace((maintenant, envoyes, recus));
        let (avant, e0, r0) = precedent?;
        Some(debits(maintenant.duration_since(avant).as_secs_f64(), (e0, r0), (envoyes, recus)))
    }
}

/// Le débit entre deux relevés — PURE. Un compteur qui RECULE (carte
/// débranchée puis rebranchée, interface disparue) donne 0, jamais un débit
/// géant ; un intervalle nul aussi.
pub fn debits(secondes: f64, (e0, r0): (u64, u64), (e1, r1): (u64, u64)) -> (f64, f64) {
    if secondes <= 0.0 {
        return (0.0, 0.0);
    }
    (e1.saturating_sub(e0) as f64 / secondes, r1.saturating_sub(r0) as f64 / secondes)
}

/// Un débit en octets par seconde, en unités françaises (Ko/s, Mo/s, Go/s, en
/// puissances de 1024) avec TROIS chiffres significatifs — PURE. Jamais en
/// dessous du Ko/s : la largeur reste à peu près stable (voir `ui::debit`).
pub fn formater(octets_par_s: f64) -> String {
    let mut v = octets_par_s.max(0.0) / 1024.0;
    let mut unite = 0;
    const UNITES: [&str; 3] = ["Ko/s", "Mo/s", "Go/s"];
    // 999,5 s'arrondirait à « 1000 » : on passe à l'unité suivante AVANT.
    while v >= 999.5 && unite + 1 < UNITES.len() {
        v /= 1024.0;
        unite += 1;
    }
    let decimales = if v < 9.995 { 2 } else if v < 99.95 { 1 } else { 0 };
    format!("{:.*} {}", decimales, v, UNITES[unite]).replace('.', ",")
}

/// Le texte affiché : l'envoi PUIS la réception, dans l'ordre de la demande.
pub fn texte((envoi, reception): (f64, f64)) -> String {
    format!("↑ {}  ↓ {}", formater(envoi), formater(reception))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trois_chiffres_significatifs_et_la_bonne_unite() {
        assert_eq!(formater(0.0), "0,00 Ko/s");
        assert_eq!(formater(1536.0), "1,50 Ko/s");
        assert_eq!(formater(12.34 * 1024.0), "12,3 Ko/s");
        assert_eq!(formater(512.0 * 1024.0), "512 Ko/s");
        assert_eq!(formater(999.7 * 1024.0), "0,98 Mo/s", "pas de « 1000 Ko/s »");
        assert_eq!(formater(3.0 * 1024.0 * 1024.0 * 1024.0), "3,00 Go/s");
        assert_eq!(formater(-5.0), "0,00 Ko/s");
    }

    #[test]
    fn un_compteur_qui_recule_donne_zero() {
        assert_eq!(debits(1.0, (1000, 5000), (3048, 4000)), (2048.0, 0.0));
        assert_eq!(debits(0.0, (0, 0), (10, 10)), (0.0, 0.0));
        assert_eq!(debits(2.0, (0, 0), (4096, 1024)), (2048.0, 512.0));
    }

    #[test]
    fn l_envoi_puis_la_reception() {
        assert_eq!(texte((1024.0, 2048.0)), "↑ 1,00 Ko/s  ↓ 2,00 Ko/s");
    }

    /// Sonde : les interfaces comptées et le débit sur deux secondes.
    #[test]
    #[ignore]
    fn sonde_debit_reel() {
        let mut e = Echantillonneur::new();
        assert!(e.echantillonner().is_none());
        std::thread::sleep(std::time::Duration::from_secs(2));
        let d = e.echantillonner().expect("GetIfTable2");
        println!("compteurs {:?} — {}", compteurs(), texte(d));
    }
}
