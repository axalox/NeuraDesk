//! Lecture de la fréquence de rafraîchissement (Hz) de l'écran sur lequel se
//! trouve une fenêtre donnée — pour le petit indicateur de la barre du haut
//! (voir `ui::topbar::refresh_rate_instance`, `app::run`). DEMANDE EXPLICITE
//! ("une applet qui affiche la fréquence de l'écran sur lequel se trouve la
//! barre"). Même esprit que `win32::sysinfo` : uniquement des API Win32 déjà
//! disponibles (`MonitorFromWindow`/`GetMonitorInfoW`/`EnumDisplaySettingsW`),
//! sans dépendance externe.

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    EnumDisplaySettingsW, GetMonitorInfoW, MonitorFromWindow, DEVMODEW, ENUM_CURRENT_SETTINGS, MONITORINFOEXW,
    MONITOR_DEFAULTTONEAREST,
};

/// Fréquence de rafraîchissement (Hz) de l'écran contenant `hwnd` — le
/// moniteur le plus proche si `hwnd` chevauche plusieurs écrans ou est
/// momentanément hors écran (`MONITOR_DEFAULTTONEAREST`, même choix que
/// `win32::window::foreground_window_is_fullscreen`). La barre elle-même
/// reste aujourd'hui toujours sur le moniteur PRIMAIRE (voir
/// `win32::window::create_dock`, `GetSystemMetrics(SM_CXSCREEN/SM_CYSCREEN)`)
/// — passer son HWND ici plutôt que de supposer directement le moniteur
/// primaire reste correct dans les deux cas, et le devient aussi si un jour
/// la barre peut être déplacée sur un autre écran. `None` si l'un des appels
/// Win32 échoue (ne devrait pratiquement jamais arriver en usage normal).
pub fn refresh_rate_hz(hwnd: HWND) -> Option<u32> {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);

        // `GetMonitorInfoW` ne prend qu'un `*mut MONITORINFO` — passer un
        // `MONITORINFOEXW` (mêmes champs en tête, `szDevice` en plus) via son
        // premier champ `monitorInfo` (identique en adresse à la structure
        // entière, `repr(C)`, voir `MONITORINFOEXW`) fait remplir `szDevice`
        // aussi, TANT QUE `cbSize` annonce la taille ÉTENDUE avant l'appel —
        // technique Win32 standard pour ce genre de structure "EX".
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(monitor, &mut info.monitorInfo).as_bool() {
            return None;
        }

        let mut devmode = DEVMODEW::default();
        devmode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        let device_name = PCWSTR(info.szDevice.as_ptr());
        if !EnumDisplaySettingsW(device_name, ENUM_CURRENT_SETTINGS, &mut devmode).as_bool() {
            return None;
        }
        Some(devmode.dmDisplayFrequency)
    }
}

/// Bornes RÉELLES du moniteur contenant `hwnd` — `[x, y, largeur, hauteur]` en
/// pixels physiques (`MonitorFromWindow`/`GetMonitorInfoW`,
/// `MONITOR_DEFAULTTONEAREST`, le rectangle du MONITEUR et non sa zone de
/// travail : nos fenêtres sont au-dessus de la barre des tâches, rien ne
/// justifie de les y confiner).
///
/// **DÉPLACÉE ICI depuis `app::hwnd_monitor_bounds` le 2026-09-19**, qui délègue
/// désormais : `win32::cef_popup` en a besoin pour le plafond d'une hauteur
/// pilotée par la page, et l'appeler depuis `cef_popup` aurait fait remonter le
/// module BAS vers `app`, qui est la couche AU-DESSUS. C'est le module qui parle
/// déjà à `MonitorFromWindow` pour la fréquence d'affichage, donc sa place.
pub fn monitor_bounds(hwnd: HWND) -> [i32; 4] {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let r = info.rcMonitor;
            [r.left, r.top, r.right - r.left, r.bottom - r.top]
        } else {
            [0, 0, 0, 0]
        }
    }
}
