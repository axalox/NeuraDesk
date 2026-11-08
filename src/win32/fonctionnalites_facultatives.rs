//! **OUVRIR LES FONCTIONNALITÉS FACULTATIVES DANS LES PARAMÈTRES DE WINDOWS.**
//!
//! DEMANDE EXPLICITE (2026-10-03) : « une applet dans la zone réglages de la
//! barre de droite pour ouvrir : Fonctionnalités facultatives Windows ».
//!
//! Une ACTION, jumelle des réglages de l'écran (voir `win32::reglages_ecran`) : la
//! page se désigne par son URI `ms-settings:` — `ms-settings:optionalfeatures`
//! (Système > Fonctionnalités facultatives), documenté par Microsoft — confié à
//! `explorer.exe`, qui résout le protocole. Paramètres ne demande pas
//! d'élévation : le piège de `mmc.exe` (voir `win32::gestionnaire_peripheriques`)
//! ne s'applique pas ici.

use std::process::Command;

/// La page Fonctionnalités facultatives des Paramètres.
const URI: &str = "ms-settings:optionalfeatures";

/// Ouvre les fonctionnalités facultatives dans les Paramètres.
///
/// Best-effort assumé, comme `reglages_ecran::ouvrir` : un échec de `spawn` ne
/// remonte pas, mais il est JOURNALISÉ — sans quoi un clic sans effet serait muet.
pub fn ouvrir() {
    crate::diag!("fonctionnalités facultatives : ouverture de {URI}");
    if let Err(err) = Command::new("explorer.exe").arg(URI).spawn() {
        crate::diag!("fonctionnalités facultatives : lancement impossible — {err}");
    }
}
