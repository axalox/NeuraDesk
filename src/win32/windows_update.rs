//! **OUVRIR WINDOWS UPDATE DANS LES PARAMÈTRES DE WINDOWS.**
//!
//! DEMANDE EXPLICITE (2026-09-26) : « je voudrais dans la zone réglages ajouter
//! une applet pour accéder à windows update dans les paramètres de windows ».
//!
//! Une ACTION, comme la file d'impression (voir `win32::printer`) : un clic ouvre
//! la page, rien à relire ni à repeindre.
//!
//! **LA PAGE SE DÉSIGNE PAR SON URI `ms-settings:`** — `ms-settings:windowsupdate`
//! est l'URI documenté par Microsoft (« Launch the Windows Settings app »). On
//! le confie à `explorer.exe`, comme `shell:PrintersFolder` dans `win32::printer` :
//! l'Explorateur résout le protocole et ouvre Paramètres sur la bonne page, sans
//! dépendance nouvelle.

use std::process::Command;

/// La page Windows Update des Paramètres.
const URI: &str = "ms-settings:windowsupdate";
/// L'ACCUEIL des Paramètres : `ms-settings:` sans page.
const URI_ACCUEIL: &str = "ms-settings:";

/// Ouvre Windows Update dans les Paramètres.
///
/// Best-effort assumé, comme `printer::open_queue` : un échec de `spawn` ne remonte
/// pas, mais il est JOURNALISÉ — sans quoi un clic sans effet serait muet.
pub fn ouvrir() {
    crate::diag!("windows update : ouverture de {URI}");
    if let Err(err) = Command::new("explorer.exe").arg(URI).spawn() {
        crate::diag!("windows update : lancement impossible — {err}");
    }
}

/// Ouvre les Paramètres de Windows sur leur accueil — DEMANDE EXPLICITE
/// (2026-10-03) : « Paramètres » au clic droit sur l'icône NeuraDesk, « pour
/// ouvrir les paramètres de Windows ». Même chemin et même best-effort
/// journalisé que [`ouvrir`].
pub fn ouvrir_parametres() {
    crate::diag!("paramètres : ouverture de {URI_ACCUEIL}");
    if let Err(err) = Command::new("explorer.exe").arg(URI_ACCUEIL).spawn() {
        crate::diag!("paramètres : lancement impossible — {err}");
    }
}
