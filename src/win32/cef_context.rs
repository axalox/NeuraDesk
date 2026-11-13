//! Fenêtre CEF pour le menu contextuel de l'icône NeuraDesk (clic droit) —
//! Phase 1 de `docs/plans/cef-web-ui-migration.md`. Remplace
//! `PopupKind::Context` (`win32::popup_menu::render_context`) pour cette
//! icône précise. Son bouton "Fermer NeuraDesk" réutilise l'action IPC
//! `app/quit` déjà câblée pour `win32::cef_test_window` (voir `app::run`,
//! dispatch des actions `drain_actions`) — pas de nouveau code de dispatch
//! nécessaire pour celui-là.
//!
//! Le sous-menu « Fréquence de rafraîchissement » qu'il portait a été RETIRÉ
//! (DEMANDE EXPLICITE du 2026-11-13 : « ne garde que la possibilité de régler
//! depuis le dash ») : son plafond (`app_refresh_rate.txt`) dormait à la fin de
//! CHAQUE frame et passait outre le réglage Performances — tout saccadait quels
//! que soient les réglages. La cadence se règle uniquement dans `ui::performances`.
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

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// Voir `app::content_scale_pour`, doc de fonction — mémorisé ici à
    /// l'ouverture (voir `toggle`) et relu à chaque frame par
    /// `render_if_visible`.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
}

/// Nombre de boutons du menu — sert à estimer la hauteur de la fenêtre à
/// l'ouverture (la page se mesure ensuite elle-même, voir `menu.js`).
///
/// **IL EST PASSÉ DE 3 À 2 LE 2026-09-20** : « Fermer NeuraDesk » a été retiré
/// de ce menu sur demande de l'utilisateur. Le compte et le HTML ne sont liés par
/// RIEN — le retrait d'un bouton doit se faire des deux côtés, sinon la fenêtre
/// naît à une hauteur estimée fausse (la mesure de la page corrige à la frame
/// suivante, mais l'ouverture saute).
///
/// **PASSÉ DE 3 À 4 LE 2026-11-08** : « À propos de NeuraDesk » (voir
/// `win32::cef_apropos`).
///
/// **PASSÉ DE 4 À 5 LE 2026-11-10** : « Rechercher les mises à jour » (voir
/// `win32::maj`, action `app/update`).
///
/// **PASSÉ DE 5 À 4 LE 2026-11-13** : les 5 comptaient la ligne « Fréquence de
/// rafraîchissement », retirée avec son séparateur — l'appelant ne compte plus
/// de séparateur non plus.
pub const COLLAPSED_BUTTON_COUNT: usize = 4;

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefContextWindow"),
    window_title: w!("NeuraDesk — Menu"),
    html_file: "context.html",
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

pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    CONTENT_SCALE.with(|c| c.set(content_scale));
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, || {});
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

/// Voir `win32::cef_popup::debug_click` — réservé au harnais de
/// vérification.
pub fn debug_click(x: i32, y: i32) {
    cef_popup::debug_click(&SLOT, x, y);
}

/// Vide et retourne les actions demandées par `assets/popups/context.html`
/// (voir `win32::cef_popup::drain_actions`) — à appeler à chaque frame.
/// Les actions rejoignent le dispatch d'`app::run` (boucle sur cet appel).
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
