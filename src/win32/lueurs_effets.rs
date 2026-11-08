//! **LA BIBLIOTHÈQUE DES EFFETS DE LUEURS** — DEMANDE EXPLICITE (2026-10-14) :
//! « pouvoir sauvegarder et importer mes effets de lueurs et les activer depuis
//! la visibilité dans les captures ».
//!
//! Un effet est une SCÈNE entière de `gfx::lueur`, écrite dans son format
//! (`lueur::texte_de_la_scene`) dans `%APPDATA%\NeuraDesk\lueurs\<nom>.lueurs`.
//! Le dossier est aussi un point d'échange : un `.lueurs` exporté s'importe tel
//! quel, ou s'y dépose à la main.
//!
//! ⚠️ **LA LISTE EST EN CACHE** (`NOMS`) : l'état des Réglages et du dash est
//! recalculé à CHAQUE frame, et lire le dossier à 60 Hz est exactement le défaut
//! « un chemin résolu par frame » déjà payé une fois dans ce dépôt. Le cache est
//! refait par chaque écriture, suppression ou import — et au démarrage.
//!
//! ⚠️ Les noms passent par `palette_gpl::nom_de_fichier_valide` : mêmes règles
//! (ce que Windows refuse, un point final qui serait retiré en silence).

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::HWND;

use crate::gfx::lueur;
use crate::win32::palette_gpl::nom_de_fichier_valide;

pub const EXTENSION: &str = "lueurs";

thread_local! {
    /// `None` : jamais lue (voir la doc de module).
    static NOMS: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

fn dossier() -> Option<PathBuf> {
    let mut path = crate::config::config_dir()?;
    path.push("lueurs");
    Some(path)
}

fn chemin(nom: &str) -> Option<PathBuf> {
    let nom = nom_de_fichier_valide(nom)?;
    let mut path = dossier()?;
    path.push(format!("{nom}.{EXTENSION}"));
    Some(path)
}

/// Le nom d'effet d'un nom de fichier — `None` si ce n'est pas un `.lueurs`.
fn nom_depuis_fichier(fichier: &str) -> Option<String> {
    let (base, extension) = fichier.rsplit_once('.')?;
    (extension.eq_ignore_ascii_case(EXTENSION) && !base.is_empty()).then(|| base.to_string())
}

/// Les effets enregistrés, triés sans tenir compte de la casse.
pub fn noms() -> Vec<String> {
    NOMS.with(|n| n.borrow_mut().get_or_insert_with(lire_le_dossier).clone())
}

fn lire_le_dossier() -> Vec<String> {
    let Some(entrees) = dossier().and_then(|d| std::fs::read_dir(d).ok()) else { return Vec::new() };
    let mut noms: Vec<String> = entrees
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| nom_depuis_fichier(e.file_name().to_str()?))
        .collect();
    noms.sort_by_key(|n| n.to_lowercase());
    noms
}

fn oublier_la_liste() {
    NOMS.with(|n| *n.borrow_mut() = None);
}

/// L'effet chargé en dernier (`lueurs_effet.txt`) — celui que les Réglages
/// montrent choisi.
pub fn actuel() -> Option<String> {
    crate::config::load_lueurs_effet()
}

/// Enregistre la scène ACTUELLE sous `nom` (écrase un effet du même nom).
pub fn enregistrer(nom: &str) -> bool {
    let Some(path) = chemin(nom) else {
        crate::diag!("[lueurs] nom d'effet refusé : {nom:?}");
        return false;
    };
    let fait = crate::config::write_atomique(&path, lueur::texte_de_la_scene());
    oublier_la_liste();
    if fait {
        crate::config::save_lueurs_effet(nom.trim());
    }
    fait
}

/// Charge l'effet `nom` et l'ALLUME (voir `lueur::remplacer_la_scene`).
pub fn charger(nom: &str) -> bool {
    let Some(texte) = chemin(nom).and_then(|p| std::fs::read_to_string(p).ok()) else {
        crate::diag!("[lueurs] effet introuvable : {nom:?}");
        oublier_la_liste();
        return false;
    };
    if !lueur::remplacer_la_scene(&texte) {
        crate::diag!("[lueurs] effet illisible : {nom:?}");
        return false;
    }
    crate::config::save_lueurs_effet(nom.trim());
    true
}

pub fn supprimer(nom: &str) -> bool {
    let fait = chemin(nom).is_some_and(|p| std::fs::remove_file(p).is_ok());
    oublier_la_liste();
    if fait && actuel().as_deref() == Some(nom.trim()) {
        crate::config::save_lueurs_effet("");
    }
    fait
}

/// **EXPORTE** l'effet `nom` vers un fichier choisi par « Enregistrer sous ».
pub fn exporter(proprietaire: HWND, nom: &str) {
    let Some(texte) = chemin(nom).and_then(|p| std::fs::read_to_string(p).ok()) else { return };
    let Some(cible) = crate::win32::file_dialog::enregistrer_sous(
        proprietaire,
        "Exporter l'effet de lueurs",
        &format!("{}.{EXTENSION}", nom.trim()),
        EXTENSION,
        "Effet de lueurs (*.lueurs)",
    ) else {
        return; // Annuler n'est pas une erreur.
    };
    if let Err(erreur) = std::fs::write(&cible, texte) {
        crate::diag!("[lueurs] export impossible : {erreur}");
    }
}

/// **IMPORTE** un `.lueurs` choisi par « Ouvrir » : il rejoint la bibliothèque
/// (sous un nom libre), puis il est chargé. Rend le nom retenu.
pub fn importer(proprietaire: HWND) -> Option<String> {
    let source = crate::win32::file_dialog::ouvrir(proprietaire, "Importer un effet de lueurs", EXTENSION, "Effet de lueurs (*.lueurs)")?;
    let texte = match std::fs::read(&source) {
        Ok(octets) => String::from_utf8_lossy(&octets).into_owned(),
        Err(erreur) => {
            crate::diag!("[lueurs] import impossible : {erreur}");
            return None;
        }
    };
    if !lueur::est_une_scene(&texte) {
        crate::diag!("[lueurs] {} n'est pas un effet de lueurs", source.display());
        return None;
    }
    let nom = nom_libre(&base_du_fichier(&source), &noms());
    let path = chemin(&nom)?;
    if !crate::config::write_atomique(&path, texte) {
        return None;
    }
    oublier_la_liste();
    charger(&nom).then_some(nom)
}

fn base_du_fichier(path: &Path) -> String {
    let base = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Effet importé");
    nom_de_fichier_valide(base).unwrap_or_else(|| "Effet importé".to_string())
}

/// `base`, ou `base (2)`, `base (3)`… — le premier que `pris` n'a pas (sans
/// tenir compte de la casse : Windows ne la distingue pas).
pub(crate) fn nom_libre(base: &str, pris: &[String]) -> String {
    let libre = |n: &str| !pris.iter().any(|p| p.eq_ignore_ascii_case(n));
    if libre(base) {
        return base.to_string();
    }
    (2..).map(|k| format!("{base} ({k})")).find(|n| libre(n)).unwrap_or_else(|| base.to_string())
}

// ⚠️ **AUCUN TEST NE TOUCHE LE DOSSIER** — le dépôt interdit à ses tests d'écrire
// dans `%APPDATA%` (voir `palette_gpl`, même règle). Ce qui est testable est pur.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seul_un_point_lueurs_est_un_effet() {
        assert_eq!(nom_depuis_fichier("Aurore.lueurs").as_deref(), Some("Aurore"));
        assert_eq!(nom_depuis_fichier("Aurore.LUEURS").as_deref(), Some("Aurore"));
        assert_eq!(nom_depuis_fichier("Aurore.txt"), None);
        assert_eq!(nom_depuis_fichier(".lueurs"), None);
        assert_eq!(nom_depuis_fichier("sans extension"), None);
    }

    #[test]
    fn un_import_ne_remplace_jamais_un_effet_existant() {
        let pris = vec!["Aurore".to_string(), "aurore (2)".to_string()];
        assert_eq!(nom_libre("Nuit", &pris), "Nuit");
        assert_eq!(nom_libre("AURORE", &pris), "AURORE (3)", "la casse ne distingue pas deux fichiers");
    }
}
