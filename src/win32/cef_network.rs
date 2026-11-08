//! Fenêtre CEF pour l'icône Réseau — Phase 1 de
//! `docs/plans/cef-web-ui-migration.md`. Remplace `PopupKind::Network`
//! (`win32::popup_menu::render_network`) pour cette icône précise.
//!
//! Premier popup de cette migration à avoir besoin d'un canal Rust→JS (voir
//! `push_state`, `win32::cef_app::CefBrowserSurface::execute_js`) : l'état
//! Wi-Fi/Bluetooth (`win32::network::wifi_state`/`bluetooth_state`) est lu
//! côté Windows, PAS calculable en JS pur — voir `assets/popups/network.html` pour
//! le protocole complet (`window.setState({wifi, bluetooth})`).
//!
//! Toute la mécanique fenêtre/rendu/relais souris est dans
//! `win32::cef_popup` — voir `win32::cef_test_window` pour la doc de
//! module complète du pattern suivi ici.

use std::cell::Cell;
use std::path::Path;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::menu;
use crate::win32::network;

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// Voir `app::content_scale_pour`, doc de fonction — mémorisé ici à
    /// l'ouverture (voir `toggle`) et relu à chaque frame par
    /// `render_if_visible`.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefNetworkWindow"),
    window_title: w!("NeuraDesk — Réseau"),
    html_file: "network.html",
    wndproc: Some(wndproc),
    above_a_magnified_dock: true,
    // MENU — apparaît et disparaît en fondant, voir `PopupSpec::fades`.
    fades: true,
    fenetre: false,
};

/// Précharge ce popup au démarrage — fenêtre et navigateur naissent CACHÉS, pour
/// que la page ait fini de charger avant la première ouverture réelle. Voir
/// `win32::cef_popup::preload`, doc de fonction, qui porte le pourquoi complet.
pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// Voir `win32::cef_popup::toggle`. Pousse aussi l'état Wi-Fi/Bluetooth
/// actuel (voir `push_state`) après avoir (re)montré/créé la fenêtre — même
/// si elle vient d'être masquée par cet appel (voir `push_state`, doc de
/// fonction : un `execute_java_script` sur une page cachée est un
/// gaspillage négligeable, pas une erreur, plus simple que de faire
/// remonter la visibilité depuis `cef_popup::toggle`).
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    CONTENT_SCALE.with(|c| c.set(content_scale));
    cef_popup::toggle(&SLOT, &SPEC, rect, gfx);
    push_state();
}

/// Pousse l'état Wi-Fi/Bluetooth ACTUEL (voir `win32::network::wifi_state`/
/// `bluetooth_state`) dans la page via `window.setState` (voir
/// `assets/popups/network.html`) — à appeler à l'ouverture du popup (voir
/// `toggle`) ET après chaque bascule effective (voir `app::run`, dispatch
/// de `drain_actions` juste après `network::toggle_wifi`/
/// `toggle_bluetooth`), pour que l'affichage ne reste jamais périmé.
pub fn push_state() {
    let wifi = js_bool_literal(network::wifi_state());
    let bluetooth = js_bool_literal(network::bluetooth_state());
    cef_popup::execute_js(&SLOT, &format!("window.setState({{wifi: {wifi}, bluetooth: {bluetooth}}})"));
}

/// À appeler à CHAQUE frame tant que ce popup est visible (voir `app::run`,
/// même endroit que `win32::cef_voicemeeter::poll`/`win32::cef_dash::poll`)
/// — élimine PAR CONSTRUCTION la course avec le chargement initial de la
/// page CEF : le `push_state()` appelé par `toggle()` juste après la
/// création de la fenêtre peut arriver AVANT que `window.setState` n'existe
/// côté JS (page `file://` pas encore chargée, voir `win32::cef_app::
/// NeuraDeskLoadHandler`, doc de struct) et être perdu en silence — BUG RÉEL
/// SIGNALÉ ("il faut ouvrir 2 fois les menus pour qu'ils soient remplis") :
/// la seconde ouverture réussissait simplement parce que la page avait eu le
/// temps de charger entretemps. Un `poll()` par frame rattrape l'état dès
/// que la page devient prête, sans avoir à détecter précisément ce moment.
pub fn poll() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

fn js_bool_literal(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    }
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

/// Vide et retourne les actions demandées par `assets/popups/network.html`
/// (voir `win32::cef_popup::drain_actions`) — format
/// `"network/toggle-wifi"`/`"network/toggle-bluetooth"`.
pub fn drain_actions() -> Vec<String> {
    cef_popup::drain_actions(&SLOT)
}

pub fn render_if_visible(tint_rgba: [f32; 4]) {
    let scale = CONTENT_SCALE.with(|c| c.get());
    cef_popup::render_if_visible(&SLOT, tint_rgba, menu::menu_corner_radius(), cef_popup::PopupPalette::Menu { zoom_level: cef_popup::menu_zoom_level(scale) });
}

pub fn debug_dump_backbuffer(path: &Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(result) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
