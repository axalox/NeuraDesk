//! Active/désactive le masquage automatique de la VRAIE barre des tâches
//! Windows (`Shell_TrayWnd`/`Shell_SecondaryTrayWnd`, voir `win32::
//! taskbar_windows::tray_windows` — PAS les barres de NeuraDesk, voir
//! `win32::window::SecondaryWindow::set_band_reveal` pour l'équivalent
//! NeuraDesk) — demande explicite : "depuis le dash réglage [...] activer
//! le masquage automatique de la barre des tâches de windows".
//!
//! Utilise `SHAppBarMessage(ABM_SETSTATE, ...)` — la MÊME API publique
//! documentée que Windows lui-même appelle quand l'utilisateur coche la
//! case "Masquer automatiquement la barre des tâches" dans ses propres
//! réglages (Paramètres > Personnalisation > Barre des tâches). AUCUNE
//! rétro-ingénierie ni écriture de registre ici : c'est le comportement
//! NATIF, donc immédiat (pas de redémarrage d'`explorer.exe` nécessaire) et
//! persisté par Windows lui-même exactement comme si l'utilisateur avait
//! coché la case à la main — `app::run` n'a donc PAS besoin de restaurer
//! quoi que ce soit à la fermeture de NeuraDesk.

use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETSTATE, ABM_SETSTATE, ABS_AUTOHIDE, APPBARDATA};

use super::taskbar_windows::tray_windows;

fn base_data(hwnd: windows::Win32::Foundation::HWND) -> APPBARDATA {
    APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, hWnd: hwnd, ..Default::default() }
}

/// État actuel du masquage automatique de la barre PRINCIPALE
/// (`Shell_TrayWnd`, toujours présente, contrairement aux `Shell_
/// SecondaryTrayWnd` qui ne le sont que si "Afficher sur tous les écrans"
/// est actif) — sert à initialiser la case à cocher du panneau de réglages
/// sur l'état RÉEL de Windows plutôt que de suivre un booléen persisté par
/// NeuraDesk séparément (qui pourrait diverger si l'utilisateur change ce
/// réglage directement depuis Windows entre deux lancements).
pub fn is_enabled() -> bool {
    // `vraie_barre` et non `FindWindowW` : le flux de notification installe une
    // fenêtre de MÊME classe (voir `tray_flux`).
    let Some(hwnd) = crate::win32::tray_flux::vraie_barre() else { return false };
    let mut data = base_data(hwnd);
    let state = unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) };
    (state as u32 & ABS_AUTOHIDE) != 0
}

/// Active/désactive le masquage automatique sur TOUTES les barres des
/// tâches actuellement affichées (voir `tray_windows`) — une configuration
/// multi-écrans avec "Afficher la barre des tâches sur tous les écrans"
/// active a une `Shell_SecondaryTrayWnd` par écran additionnel, chacune
/// avec son PROPRE état d'auto-masquage côté `SHAppBarMessage`.
pub fn set_enabled(enabled: bool) {
    let new_state = if enabled { ABS_AUTOHIDE } else { 0 };
    for hwnd in tray_windows() {
        let mut data = base_data(hwnd);
        data.lParam = windows::Win32::Foundation::LPARAM(new_state as isize);
        unsafe {
            SHAppBarMessage(ABM_SETSTATE, &mut data);
        }
    }
}
