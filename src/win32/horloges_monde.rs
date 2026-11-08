//! ⭐ LES HORLOGES DU MONDE — DEMANDE EXPLICITE (2026-10-24) : « horloges du
//! monde : quelques fuseaux dans l'infobulle ou le menu de l'horloge ».
//!
//! Une liste de fuseaux IANA (« America/New_York »), réglée dans les Réglages
//! et montrée sous la météo, dans le menu qui s'ouvre au clic sur l'horloge
//! (voir `cef_calendar::push_state`).
//!
//! ⚠️ **RUST NE CALCULE AUCUNE HEURE.** Il garde la liste ; c'est la page qui
//! convertit, par `Intl.DateTimeFormat(…, { timeZone })` — Chromium embarque la
//! base des fuseaux (ICU) et ses changements d'heure, Windows ne nous donnerait
//! que ses propres noms de fuseaux, sans correspondance simple avec IANA.
//! Le nom de fuseau n'est donc vérifié ici que dans sa FORME (voir
//! `zone_valide`) : la page n'en propose que de connus d'`Intl`, et un nom
//! inconnu qui arriverait quand même serait simplement ignoré à l'affichage.

use std::cell::RefCell;

use serde_json::{json, Value};

/// Une ligne par horloge : le fuseau IANA, rien d'autre.
const FICHIER: &str = "horloges_monde.txt";

/// Au-delà, le menu de l'horloge s'allongerait plus que le calendrier lui-même.
pub const MAX_HORLOGES: usize = 8;

thread_local! {
    static ZONES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Change à chaque modification — c'est la clé du cache de `cef_calendar`.
    static VERSION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// « Europe/Paris », « America/Argentina/Buenos_Aires », « Etc/GMT+5 », « UTC ».
fn zone_valide(zone: &str) -> bool {
    !zone.is_empty()
        && zone.len() <= 64
        && !zone.starts_with('/')
        && !zone.ends_with('/')
        && !zone.contains("//")
        && !zone.contains("..")
        && zone.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '+'))
}

/// Le nom montré : la ville, sans ses soulignés (« Buenos Aires »).
pub fn nom(zone: &str) -> String {
    zone.rsplit('/').next().unwrap_or(zone).replace('_', " ")
}

fn depuis_texte(texte: &str) -> Vec<String> {
    let mut zones: Vec<String> = Vec::new();
    for ligne in texte.lines().map(str::trim) {
        if zone_valide(ligne) && !zones.iter().any(|z| z == ligne) && zones.len() < MAX_HORLOGES {
            zones.push(ligne.to_string());
        }
    }
    zones
}

pub fn charger() {
    let zones = crate::config::config_path(FICHIER)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| depuis_texte(&t))
        .unwrap_or_default();
    ZONES.with(|z| *z.borrow_mut() = zones);
}

fn remplacer(zones: Vec<String>) {
    crate::config::write_config(FICHIER, zones.join("\n"));
    ZONES.with(|z| *z.borrow_mut() = zones);
    VERSION.with(|v| v.set(v.get() + 1));
}

/// Ajoute un fuseau en fin de liste — sans effet s'il y est déjà, si la forme
/// est invalide ou si la liste est pleine.
pub fn ajouter(zone: &str) {
    let zone = zone.trim();
    let mut zones = zones();
    if !zone_valide(zone) || zones.iter().any(|z| z == zone) || zones.len() >= MAX_HORLOGES {
        return;
    }
    zones.push(zone.to_string());
    remplacer(zones);
}

pub fn retirer(rang: usize) {
    let mut zones = zones();
    if rang < zones.len() {
        zones.remove(rang);
        remplacer(zones);
    }
}

/// Monte (`-1`) ou descend (`+1`) une horloge d'un cran.
pub fn deplacer(rang: usize, vers_le_haut: bool) {
    let mut zones = zones();
    let autre = if vers_le_haut { rang.checked_sub(1) } else { Some(rang + 1) };
    if let Some(autre) = autre.filter(|&a| a < zones.len() && rang < zones.len()) {
        zones.swap(rang, autre);
        remplacer(zones);
    }
}

pub fn zones() -> Vec<String> {
    ZONES.with(|z| z.borrow().clone())
}

pub fn version() -> u64 {
    VERSION.with(std::cell::Cell::get)
}

/// `[{ zone, nom }]`, dans l'ordre de l'utilisateur.
pub fn json() -> Value {
    Value::Array(zones().iter().map(|z| json!({ "zone": z, "nom": nom(z) })).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_forme_d_un_fuseau_est_verifiee() {
        for bon in ["Europe/Paris", "America/Argentina/Buenos_Aires", "Etc/GMT+5", "UTC", "America/Port-au-Prince"] {
            assert!(zone_valide(bon), "{bon}");
        }
        for mauvais in ["", "/Paris", "Europe/", "Europe//Paris", "../x", "Europe/Paris;rm", "Europe Paris", "\"><script>"] {
            assert!(!zone_valide(mauvais), "{mauvais}");
        }
    }

    #[test]
    fn le_fichier_ecarte_les_doublons_les_lignes_invalides_et_le_surplus() {
        let mut texte = String::from("Europe/Paris\n  Asia/Tokyo \nEurope/Paris\nn'importe quoi\n\n");
        for i in 0..20 {
            texte.push_str(&format!("Etc/GMT+{}\n", i % 12));
        }
        let zones = depuis_texte(&texte);
        assert_eq!(&zones[..2], ["Europe/Paris", "Asia/Tokyo"]);
        assert_eq!(zones.len(), MAX_HORLOGES);
    }

    #[test]
    fn le_nom_est_la_ville_sans_ses_soulignes() {
        assert_eq!(nom("America/Argentina/Buenos_Aires"), "Buenos Aires");
        assert_eq!(nom("UTC"), "UTC");
    }
}
