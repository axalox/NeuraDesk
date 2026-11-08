//! **OUVRIR LA LOUPE DE WINDOWS** (`Magnify.exe`).
//!
//! DEMANDE EXPLICITE (2026-10-19) : « une nouvelle applet dans la zone réglages
//! qui permette d'ouvrir le zoom windows ». Un clic OUVRE la Loupe — le geste de
//! Win + « + », sans clavier. Une ACTION, comme les propriétés système : rien à
//! relire ni à repeindre.
//!
//! ## ⛔ LE LANCEMENT PASSE PAR `ShellExecuteW`
//!
//! Le manifeste EMBARQUÉ de `C:\Windows\System32\Magnify.exe` (relu sur la
//! machine) déclare `level="asInvoker" uiAccess="true"` — le même cas que le
//! Narrateur (voir `win32::narrateur`) : `CreateProcess` (ce qu'appelle
//! `Command::spawn`) échouerait en `ERROR_ELEVATION_REQUIRED` (740), sans fenêtre.
//!
//! ## POURQUOI ELLE N'ARRÊTE PAS LA LOUPE
//!
//! La demande est d'OUVRIR. La Loupe se ferme d'elle-même (sa barre d'outils,
//! Win + Échap), et l'arrêter de l'extérieur demanderait `TerminateProcess` sur un
//! processus qui grossit tout l'écran. Rouvrir une Loupe déjà ouverte la ramène
//! simplement au premier plan.

use windows::core::{w, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Ouvre la Loupe de Windows.
///
/// Best-effort assumé, comme les autres applets ACTION : un échec ne remonte
/// pas, mais il est JOURNALISÉ — sans quoi un clic sans effet serait muet.
pub fn ouvrir() {
    crate::diag!("loupe : ouverture");
    // SAFETY : chaînes littérales terminées par zéro (`w!`), aucune fenêtre parente.
    let resultat = unsafe { ShellExecuteW(None, w!("open"), w!("Magnify.exe"), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    // `ShellExecuteW` rend une valeur > 32 en cas de succès, un code d'erreur sinon.
    if resultat.0 as isize <= 32 {
        crate::diag!("loupe : lancement impossible — code {}", resultat.0 as isize);
    }
}
