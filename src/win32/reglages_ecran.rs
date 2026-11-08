//! **OUVRIR LES RÉGLAGES DE L'ÉCRAN DANS LES PARAMÈTRES DE WINDOWS.**
//!
//! DEMANDE EXPLICITE (2026-09-26) : « je voudrais une applet dans la zone
//! réglages pour ouvrir les réglages écran dans les paramètres windows ».
//!
//! Une ACTION, jumelle de Windows Update (voir `win32::windows_update`) : la page
//! se désigne par son URI `ms-settings:` — `ms-settings:display` (Système >
//! Écran), documenté par Microsoft — confié à `explorer.exe`, qui résout le
//! protocole. Paramètres ne demande pas d'élévation : le piège de `mmc.exe`
//! (voir `win32::gestionnaire_peripheriques`) ne s'applique pas ici.

use std::process::Command;

/// La page Écran des Paramètres.
const URI: &str = "ms-settings:display";

/// Ouvre les réglages de l'écran dans les Paramètres.
///
/// Best-effort assumé, comme `windows_update::ouvrir` : un échec de `spawn` ne
/// remonte pas, mais il est JOURNALISÉ — sans quoi un clic sans effet serait muet.
pub fn ouvrir() {
    crate::diag!("réglages écran : ouverture de {URI}");
    if let Err(err) = Command::new("explorer.exe").arg(URI).spawn() {
        crate::diag!("réglages écran : lancement impossible — {err}");
    }
}
