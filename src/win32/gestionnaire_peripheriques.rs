//! **OUVRIR LE GESTIONNAIRE DE PÉRIPHÉRIQUES DE WINDOWS.**
//!
//! DEMANDE EXPLICITE (2026-09-26) : « j'aimerais une nouvelle applet dans la zone
//! réglages pour ouvrir le gestionnaire de périphériques de windows ».
//!
//! Une ACTION, comme Windows Update (voir `win32::windows_update`) : un clic ouvre
//! la console, rien à relire ni à repeindre.
//!
//! **LE GESTIONNAIRE EST UN COMPOSANT LOGICIEL ENFICHABLE MMC** — `devmgmt.msc`,
//! ouvert par `mmc.exe`, qui le trouve dans `System32`.
//!
//! ⛔ **PAS DE `std::process::Command` ICI** — DÉFAUT RÉEL SIGNALÉ (2026-09-26) :
//! « le gestionnaire de périphériques ne s'ouvre pas ». Le manifeste de `mmc.exe`
//! demande `requestedExecutionLevel level="highestAvailable"` : pour un compte
//! administrateur sous UAC, `CreateProcess` (ce qu'appelle `Command::spawn`)
//! échoue alors en `ERROR_ELEVATION_REQUIRED` (740), sans aucune fenêtre. Seul
//! le SHELL sait élever — `ShellExecuteW` affiche l'invite UAC puis ouvre la
//! console (en lecture seule si l'élévation est refusée).

use windows::core::{w, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Ouvre le Gestionnaire de périphériques.
///
/// Best-effort assumé, comme `windows_update::ouvrir` : un échec ne remonte pas,
/// mais il est JOURNALISÉ — sans quoi un clic sans effet serait muet.
pub fn ouvrir() {
    crate::diag!("gestionnaire de périphériques : ouverture de devmgmt.msc");
    // SAFETY : chaînes littérales terminées par zéro (`w!`), aucune fenêtre parente.
    let resultat = unsafe {
        ShellExecuteW(None, w!("open"), w!("mmc.exe"), w!("devmgmt.msc"), PCWSTR::null(), SW_SHOWNORMAL)
    };
    // `ShellExecuteW` rend une valeur > 32 en cas de succès, un code d'erreur sinon.
    if resultat.0 as isize <= 32 {
        crate::diag!("gestionnaire de périphériques : lancement impossible — code {}", resultat.0 as isize);
    }
}
