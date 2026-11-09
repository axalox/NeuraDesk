//! Énumération des écrans connectés (`EnumDisplayMonitors`) — utilisée pour
//! proposer à l'utilisateur de placer une copie du dock (barre supérieure
//! seule, ou barre + côtés) sur un écran secondaire, voir
//! `win32::window::create_secondary` et `config::secondary_screen_mode`.
//!
//! Chaque écran est identifié par son `device_name` (ex. `\\.\DISPLAY2`),
//! PAS par un index : l'ordre renvoyé par `EnumDisplayMonitors` peut changer
//! au branchement/débranchement d'un moniteur.
//!
//! ⛔ **MAIS `device_name` PEUT CHANGER D'UN DÉMARRAGE À L'AUTRE** (mesuré le
//! 2026-11-09 : `DISPLAY18` puis `DISPLAY2` pour le même écran). Les réglages
//! restent rangés sous ce nom dans `config.rs` ; c'est `ecrans_identites` qui
//! les fait suivre, grâce à `identifiant_stable`.

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetWindowRect, IsWindowVisible};

/// Un écran connecté.
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub handle: HMONITOR,
    /// Ex. `\\.\DISPLAY2` — identifiant stable utilisé pour la persistance
    /// (voir la doc de module), contrairement à l'index dans le `Vec` rendu.
    pub device_name: String,
    /// Rectangle complet de l'écran en coordonnées virtuelles du bureau
    /// (peut être négatif — un écran secondaire placé à gauche du primaire
    /// a des coordonnées X négatives, voir la doc du module `window`).
    pub rect: RECT,
    /// **Zone de travail** (`MONITORINFO::rcWork`) : `rect` amputé de ce que le
    /// shell réserve — barre des tâches Windows, et **nos propres barres**
    /// (réservations AppBar, voir `window::Window::poser_reservations`).
    ///
    /// ⚠️ **C'EST ELLE QUI EST PUBLIÉE PAR LE SHELL, ET ELLE SE MET À JOUR DE
    /// FAÇON ASYNCHRONE** — un `ABM_SETPOS` rendu « succès » ne signifie pas
    /// qu'elle a déjà bougé. C'est tout l'objet de
    /// `window::ecrans_ont_rendu_la_place`.
    pub work_rect: RECT,
    pub is_primary: bool,
    /// Rectangle de la fenêtre de barre des tâches Windows trouvée sur cet
    /// écran (`Shell_TrayWnd` pour le principal, `Shell_SecondaryTrayWnd`
    /// pour les autres), s'il y en a une — absente si l'utilisateur a
    /// désactivé "Afficher la barre des tâches sur tous les écrans".
    pub taskbar_rect: Option<RECT>,
}

/// Tous les écrans actuellement connectés, moniteur primaire inclus.
pub fn enumerate() -> Vec<MonitorInfo> {
    let mut monitors = collect_monitors();
    let taskbars = tray_windows();
    for monitor in &mut monitors {
        monitor.taskbar_rect = taskbars
            .iter()
            .find(|(_, owner)| *owner == monitor.handle)
            .map(|(rect, _)| *rect);
    }
    monitors
}

/// **L'IDENTITÉ STABLE D'UN MONITEUR** — son chemin de périphérique
/// (`\\?\DISPLAY#SAM092D#5&…&UID4352#{e6f07b5f-…}`), lu par
/// `EnumDisplayDevicesW(device_name, 0, …, EDD_GET_DEVICE_INTERFACE_NAME)`.
///
/// ⛔ **`device_name` N'EST PAS STABLE**, contrairement à ce que disait la doc de
/// module jusqu'au 2026-11-09 : MESURÉ ce jour-là, le même écran physique
/// (même position, -1920,484) s'appelait `\\.\DISPLAY18` la veille et
/// `\\.\DISPLAY2` ce jour-là. Ce chemin, lui, porte le modèle (`SAM092D`) et
/// le connecteur (`UID4352`). Voir `ecrans_identites`, qui s'en sert pour
/// suivre les renommages. `None` si Windows ne le donne pas.
pub fn identifiant_stable(device_name: &str) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW};
    /// `EDD_GET_DEVICE_INTERFACE_NAME` (Microsoft Learn, `EnumDisplayDevicesW`).
    const EDD_GET_DEVICE_INTERFACE_NAME: u32 = 0x0000_0001;
    let nom: Vec<u16> = device_name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut peripherique = DISPLAY_DEVICEW { cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
    let trouve = unsafe {
        EnumDisplayDevicesW(PCWSTR(nom.as_ptr()), 0, &mut peripherique, EDD_GET_DEVICE_INTERFACE_NAME).as_bool()
    };
    if !trouve {
        return None;
    }
    let len = peripherique.DeviceID.iter().position(|&c| c == 0).unwrap_or(peripherique.DeviceID.len());
    let id = String::from_utf16_lossy(&peripherique.DeviceID[..len]);
    (!id.trim().is_empty()).then_some(id)
}

/// Associe chaque barre des tâches Windows à l'écran qui la POSSÈDE
/// réellement, via `MonitorFromWindow` — l'API Windows prévue pour ça, déjà
/// utilisée ailleurs dans le projet (`win32::display`, `win32::
/// fullscreen_clock`).
///
/// BUG RÉEL CORRIGÉ ("la barre supérieure de l'interface secondaire s'est
/// déplacée, elle n'est plus en haut de l'écran secondaire") : l'association
/// se faisait auparavant par géométrie ("le CENTRE du rectangle de la barre
/// tombe dans le rectangle de l'écran"), ce qui est FAUX dès que deux écrans
/// SE CHEVAUCHENT sur un axe. Cas concret de ce poste : principal
/// 2560x1440 (barre des tâches en bas, y 1392→1440) et secondaire
/// 1920x1080 positionné à y 484→1564 — le bas du secondaire DÉBORDE sous
/// celui du principal, donc le centre de la barre du PRINCIPAL (1280, 1416)
/// tombait dans le rectangle du SECONDAIRE, qui se croyait alors doté d'une
/// barre des tâches. `detect_secondary_taskbar` la classait "en haut" (son
/// `top` est très au-dessus du `top` du moniteur), et le dock secondaire se
/// décalait de son épaisseur pour ne pas la chevaucher : la bande
/// apparaissait 40 px sous le haut de l'écran.
///
/// `MonitorFromWindow(MONITOR_DEFAULTTONEAREST)` répond à la vraie question
/// ("à quel écran appartient cette fenêtre ?") et reste correct quel que soit
/// l'agencement, y compris chevauchant — c'est le shell qui décide, pas nous.
fn owning_monitor(hwnd: HWND) -> HMONITOR {
    unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }
}

fn collect_monitors() -> Vec<MonitorInfo> {
    let mut monitors: Vec<MonitorInfo> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(enum_monitor_proc), LPARAM(&mut monitors as *mut Vec<MonitorInfo> as isize));
    }
    monitors
}

unsafe extern "system" fn enum_monitor_proc(hmonitor: HMONITOR, _hdc: HDC, _rect: *mut RECT, lparam: LPARAM) -> BOOL {
    let monitors = &mut *(lparam.0 as *mut Vec<MonitorInfo>);
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(hmonitor, &mut info.monitorInfo as *mut _ as *mut _).as_bool() {
        let len = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
        let device_name = String::from_utf16_lossy(&info.szDevice[..len]);
        monitors.push(MonitorInfo {
            handle: hmonitor,
            device_name,
            rect: info.monitorInfo.rcMonitor,
            work_rect: info.monitorInfo.rcWork,
            // `MONITORINFOF_PRIMARY` (bit 0 de `dwFlags`) n'est pas exposé
            // comme constante par cette version du crate `windows` — valeur
            // documentée par Microsoft Learn (`MONITORINFO.dwFlags`).
            is_primary: (info.monitorInfo.dwFlags & 1) != 0,
            taskbar_rect: None,
        });
    }
    BOOL(1) // continue l'énumération
}

/// Rectangles de toutes les fenêtres de barre des tâches actuellement
/// affichées — même filtre de classe que `taskbar_windows::tray_windows`, mais
/// on a besoin ici du rectangle (pour l'associer à un écran) plutôt que du
/// HWND, donc pas de réutilisation directe.
/// Rectangle ET écran propriétaire (voir `owning_monitor`) de chaque barre
/// des tâches actuellement affichée.
fn tray_windows() -> Vec<(RECT, HMONITOR)> {
    let mut found = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_tray_proc), LPARAM(&mut found as *mut Vec<(RECT, HMONITOR)> as isize));
    }
    found
}

unsafe extern "system" fn enum_tray_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let found = &mut *(lparam.0 as *mut Vec<(RECT, HMONITOR)>);
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }
    let mut class_name_buf = [0u16; 64];
    let len = GetClassNameW(hwnd, &mut class_name_buf);
    if len > 0 {
        let class_name = String::from_utf16_lossy(&class_name_buf[..len as usize]);
        if class_name == "Shell_TrayWnd" || class_name == "Shell_SecondaryTrayWnd" {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                found.push((rect, owning_monitor(hwnd)));
            }
        }
    }
    BOOL(1) // continue l'énumération
}
