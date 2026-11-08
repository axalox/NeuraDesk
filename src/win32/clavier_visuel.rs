//! **AFFICHER OU MASQUER LE CLAVIER VISUEL DE WINDOWS** (`osk.exe`).
//!
//! DEMANDE (2026-11-07) : « une applet pour faire apparaître et disparaître le
//! clavier visuel de Windows ». Un clic BASCULE : si le clavier est ouvert, on
//! le ferme ; sinon on le lance. Une ACTION, comme la Loupe (voir
//! `win32::loupe_windows`) et le Narrateur (voir `win32::narrateur`) : rien à
//! relire ni à repeindre.
//!
//! ## ⛔ LE LANCEMENT PASSE PAR `ShellExecuteW`
//!
//! Le manifeste embarqué de `C:\Windows\System32\osk.exe` déclare
//! `uiAccess="true"` — même cas que la Loupe et le Narrateur : `CreateProcess`
//! (ce qu'appelle `Command::spawn`) échouerait en `ERROR_ELEVATION_REQUIRED`
//! (740), sans fenêtre.
//!
//! ## LA FERMETURE PASSE PAR `WM_CLOSE`
//!
//! Le clavier visuel porte la classe de fenêtre `OSKMainClass` : on la retrouve
//! par `FindWindowW` et on lui POSTE `WM_CLOSE`, le geste propre de fermeture
//! (le même que `win32::window` pour fermer une fenêtre tierce).

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, SW_SHOWNORMAL, WM_CLOSE};

/// La classe de fenêtre du clavier visuel de Windows (`osk.exe`).
const CLASSE_OSK: PCWSTR = w!("OSKMainClass");

/// Bascule le clavier visuel : le ferme s'il est déjà ouvert, le lance sinon.
///
/// Best-effort assumé, comme les autres applets ACTION (voir
/// `win32::loupe_windows`) : un échec ne remonte pas, mais il est JOURNALISÉ —
/// sans quoi un clic sans effet serait muet.
pub fn basculer() {
    // SAFETY : classe littérale terminée par zéro (`w!`). `FindWindowW` rend
    // `Err` quand aucune fenêtre ne porte cette classe (HWND nul).
    if let Ok(hwnd) = unsafe { FindWindowW(CLASSE_OSK, PCWSTR::null()) } {
        crate::diag!("clavier visuel : déjà ouvert, fermeture");
        // SAFETY : HWND valide rendu par `FindWindowW`.
        if let Err(e) = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) } {
            crate::diag!("clavier visuel : fermeture impossible — {e:?}");
        }
        return;
    }
    crate::diag!("clavier visuel : ouverture");
    // SAFETY : chaînes littérales terminées par zéro (`w!`), aucune fenêtre parente.
    let resultat = unsafe { ShellExecuteW(None, w!("open"), w!("osk.exe"), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    // `ShellExecuteW` rend une valeur > 32 en cas de succès, un code d'erreur sinon.
    if resultat.0 as isize <= 32 {
        crate::diag!("clavier visuel : lancement impossible — code {}", resultat.0 as isize);
    }
}
