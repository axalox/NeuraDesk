//! **OUVRIR L'OBSERVATEUR D'ÉVÉNEMENTS DE WINDOWS.**
//!
//! DEMANDE EXPLICITE (2026-10-30) : « ajouter une applet dans la zone réglages de la
//! barre de droite pour lancer l'observateur d'évènements de windows ».
//!
//! Une ACTION, comme le Gestionnaire de périphériques (voir
//! `win32::gestionnaire_peripheriques`) : un clic ouvre la console, rien à relire ni à
//! repeindre.
//!
//! **C'EST LE MÊME COMPOSANT ENFICHABLE MMC** — `eventvwr.msc`, ouvert par `mmc.exe`, qui
//! le trouve dans `System32`. Les deux applets sont frères, et partagent donc le même
//! piège d'élévation, d'où le même chemin d'appel.
//!
//! ⛔ **PAS DE `std::process::Command` ICI** : `mmc.exe` demande
//! `requestedExecutionLevel level="highestAvailable"`, donc `CreateProcess` échoue en
//! `ERROR_ELEVATION_REQUIRED` (740) pour un compte administrateur sous UAC — sans aucune
//! fenêtre. Seul le SHELL sait élever.

use windows::core::{w, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Ouvre l'Observateur d'événements.
///
/// Best-effort assumé, comme `windows_update::ouvrir` et
/// `gestionnaire_peripheriques::ouvrir` : un échec ne remonte pas, mais il est
/// JOURNALISÉ — sans quoi un clic sans effet serait muet.
pub fn ouvrir() {
    crate::diag!("observateur d'événements : ouverture de eventvwr.msc");
    // SAFETY : chaînes littérales terminées par zéro (`w!`), aucune fenêtre parente.
    let resultat =
        unsafe { ShellExecuteW(None, w!("open"), w!("mmc.exe"), w!("eventvwr.msc"), PCWSTR::null(), SW_SHOWNORMAL) };
    // `ShellExecuteW` rend une valeur > 32 en cas de succès, un code d'erreur sinon.
    if resultat.0 as isize <= 32 {
        crate::diag!("observateur d'événements : lancement impossible — code {}", resultat.0 as isize);
    }
}
