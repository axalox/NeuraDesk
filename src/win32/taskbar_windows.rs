//! Énumération des fenêtres de la VRAIE barre des tâches Windows
//! (`Shell_TrayWnd`/`Shell_SecondaryTrayWnd` — PAS les barres de NeuraDesk,
//! voir `ui::dock`).
//!
//! Ce module ne contenait à l'origine que la teinte de la barre des tâches
//! (`SetWindowCompositionAttribute` + `WCA_ACCENT_POLICY`, non documenté par
//! Microsoft). Ce réglage a été **retiré** — il ne fonctionnait pas de façon
//! fiable — et avec lui `gfx::composition`, qui n'existait que pour lui.
//!
//! Le module survit parce que `win32::taskbar_autohide` (masquage automatique
//! de la barre des tâches, une AUTRE fonctionnalité, conservée) a besoin de
//! `tray_windows` pour adresser la barre de chaque écran. D'où le nom :
//! il n'énumère plus que des fenêtres.

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW};

/// Toutes les fenêtres de barre des tâches actuellement affichées : la
/// principale (`Shell_TrayWnd`, toujours présente) et une
/// `Shell_SecondaryTrayWnd` par écran additionnel si "Afficher la barre des
/// tâches sur tous les écrans" est actif — sans ça, seul l'écran principal
/// serait adressé sur une configuration multi-écrans.
pub(crate) fn tray_windows() -> Vec<HWND> {
    let mut windows = Vec::new();
    unsafe {
        // `vraie_barre` et non `FindWindowW` : voir `tray_flux`.
        if let Some(primary) = crate::win32::tray_flux::vraie_barre() {
            windows.push(primary);
        }
        let _ = EnumWindows(Some(enum_secondary_tray_proc), LPARAM(&mut windows as *mut Vec<HWND> as isize));
    }
    windows
}

unsafe extern "system" fn enum_secondary_tray_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let windows = &mut *(lparam.0 as *mut Vec<HWND>);
    let mut class_name_buf = [0u16; 64];
    let len = GetClassNameW(hwnd, &mut class_name_buf);
    if len > 0 && String::from_utf16_lossy(&class_name_buf[..len as usize]) == "Shell_SecondaryTrayWnd" {
        windows.push(hwnd);
    }
    BOOL(1) // continue l'énumération
}
