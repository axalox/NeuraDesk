//! Fenêtre CEF pour le menu contextuel de l'icône NeuraDesk (clic droit) —
//! Phase 1 de `docs/plans/cef-web-ui-migration.md`. Remplace
//! `PopupKind::Context` (`win32::popup_menu::render_context`) pour cette
//! icône précise. Son bouton "Fermer NeuraDesk" réutilise l'action IPC
//! `app/quit` déjà câblée pour `win32::cef_test_window` (voir `app::run`,
//! dispatch des actions `drain_actions`) — pas de nouveau code de dispatch
//! nécessaire pour celui-là.
//!
//! Deuxième rôle ajouté ensuite, DEMANDE EXPLICITE ("une option... qui
//! permette de choisir la fréquence de rafraîchissement de l'app pour les
//! fréquences les plus courantes") : la liste des fréquences (voir
//! `push_state`, `assets/popups/context.html`) réutilise le même canal
//! Rust→JS que `win32::cef_network` (`window.setState`), pour la même
//! raison — la fréquence choisie (`config::load_app_refresh_rate_cap`) ET
//! la fréquence réelle de l'écran (`win32::display::refresh_rate_hz`, pour
//! l'étiquette "Automatique (144 Hz)") ne sont calculables que côté Rust.
//!
//! Troisième rôle, DEMANDE EXPLICITE ("mets le choix des fréquences dans
//! un sous-menu") : la liste de fréquences est repliée par défaut derrière
//! une ligne "Fréquence de rafraîchissement" (voir `FREQUENCY_MENU_EXPANDED`,
//! `toggle_frequency_menu`) — même raisonnement que `win32::cef_voicemeeter::
//! push_state` pour le redimensionnement de la fenêtre (voir sa doc,
//! `win32::cef_popup::resize_to_content`) : Rust reste seul
//! propriétaire de "combien de boutons sont actuellement visibles",
//! jamais un aller-retour JS pour mesurer une hauteur.
//!
//! Toute la mécanique fenêtre/rendu/relais souris est dans
//! `win32::cef_popup` — voir `win32::cef_test_window` pour la doc de
//! module complète du pattern suivi ici.

use std::cell::Cell;
use std::path::Path;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::config;
use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::{display, menu};

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// État du sous-menu "Fréquence de rafraîchissement" (voir
    /// `assets/popups/context.html`) — DEMANDE EXPLICITE ("mets le choix
    /// des fréquences dans un sous-menu"). Repart TOUJOURS replié à
    /// chaque ouverture FRAÎCHE du menu (voir `toggle`, `on_show` de
    /// `cef_popup::toggle_with`) — un vrai sous-menu Windows ne reste
    /// jamais déplié d'une fois sur l'autre, même si cette fenêtre CEF
    /// persiste (cachée, pas détruite) entre deux clics sur l'icône.
    static FREQUENCY_MENU_EXPANDED: Cell<bool> = const { Cell::new(false) };
    /// Voir `app::content_scale_pour`, doc de fonction — mémorisé ici à
    /// l'ouverture (voir `toggle`) et relu à chaque frame par
    /// `render_if_visible`.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
}

/// Nombre de boutons affichés QUAND LE SOUS-MENU DES FRÉQUENCES EST
/// REPLIÉ : "Paramètres" (ajouté le 2026-10-03, DEMANDE EXPLICITE),
/// "Recharger NeuraDesk" (DEMANDE EXPLICITE, voir
/// `assets/popups/context.html`) + la ligne "Fréquence de
/// rafraîchissement" elle-même (bascule le sous-menu, voir
/// `toggle_frequency_menu`) — PAS les fréquences, invisibles tant qu'on ne
/// l'a pas déplié (voir `EXPANDED_BUTTON_COUNT` pour le compte une fois
/// déplié).
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
pub const COLLAPSED_BUTTON_COUNT: usize = 5;

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

pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32, hwnd: HWND) {
    CONTENT_SCALE.with(|c| c.set(content_scale));
    // `on_show` : voir la doc de `FREQUENCY_MENU_EXPANDED` — repart
    // TOUJOURS replié à l'ouverture, jamais restauré d'une fois sur
    // l'autre (contrairement à `cap`/`native`, qui eux doivent refléter
    // un choix persisté).
    // `rect` porte la hauteur du sous-menu REPLIÉ (le cas courant), et c'est tout
    // ce qu'il y a à décider ici : le PLAFOND, lui, n'est plus une affaire de ce
    // popup — `cef_popup::resize_to_content` borne à la place réellement
    // disponible sous la fenêtre. Le dépliage du sous-menu des fréquences peut
    // donc toujours grandir la fenêtre, sans que ce module ait à en connaître la
    // taille (voir `cef_popup::toggle_with`, doc, pour le saut de 2000 px que
    // l'ancien plafond — confondu avec la hauteur demandée — causait).
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, || {
        FREQUENCY_MENU_EXPANDED.with(|e| e.set(false));
    });
    push_state(hwnd);
}

/// Bascule le sous-menu des fréquences (action `"app/refresh_rate_menu/
/// toggle"`, voir `assets/popups/context.html`, bouton `#freqToggle`) —
/// DEMANDE EXPLICITE ("mets le choix des fréquences dans un sous-menu").
/// `push_state` se charge ENSUITE de redimensionner la fenêtre et de
/// pousser le nouvel état à la page (voir sa doc).
pub fn toggle_frequency_menu(hwnd: HWND) {
    FREQUENCY_MENU_EXPANDED.with(|e| e.set(!e.get()));
    push_state(hwnd);
}

/// Pousse la fréquence de rafraîchissement CHOISIE (voir `config::
/// load_app_refresh_rate_cap`, `null` pour "Automatique"), la fréquence
/// RÉELLE de l'écran sous `hwnd` (voir `win32::display::refresh_rate_hz`,
/// pour l'étiquette "Automatique (144 Hz)") ET l'état du sous-menu (voir
/// `FREQUENCY_MENU_EXPANDED`) dans la page via `window.setState`. Même
/// raisonnement que `win32::cef_network::push_state` : rien de tout ça
/// n'est calculable en JS pur. `hwnd` reste celui du dock (toujours sur
/// l'écran où la barre se trouve, voir `win32::display::refresh_rate_hz`,
/// doc de fonction) — le popup lui-même peut s'ouvrir ailleurs, mais c'est
/// bien la fréquence DE LA BARRE qui est demandée ici, pas celle du popup.
///
/// Redimensionne AUSSI la fenêtre en fonction du sous-menu (voir
/// `win32::cef_popup::resize_to_content`, doc de fonction pour le
/// pourquoi de ce calcul côté Rust plutôt qu'un aller-retour JS) — même
/// endroit que `win32::cef_voicemeeter::push_state`, même raisonnement.
pub fn push_state(hwnd: HWND) {
    let cap = match config::load_app_refresh_rate_cap() {
        Some(hz) => hz.to_string(),
        None => "null".to_string(),
    };
    let native = match display::refresh_rate_hz(hwnd) {
        Some(hz) => hz.to_string(),
        None => "null".to_string(),
    };
    let expanded = FREQUENCY_MENU_EXPANDED.with(Cell::get);
    // **CE POPUP NE SE REDIMENSIONNE PLUS LUI-MÊME** — la hauteur est mesurée par
    // la PAGE et annoncée par `menu/height/…`, que `cef_popup::drain_actions`
    // applique (voir `assets/popups/menu.js`).
    //
    // Il le faisait, et il FALLAIT le retirer plutôt que le laisser : `app::run`
    // appelle ce `push_state` (via `poll`) AVANT le drainage des actions de la
    // MÊME frame, donc l'estimation aurait gagné à chaque frame et la mesure de
    // la page n'aurait jamais été visible. Ce n'était pas de l'hygiène, c'était
    // la condition pour que le mécanisme fonctionne.
    //
    // Corollaire agréable : le dépliage du sous-menu des fréquences ci-dessous
    // n'a plus à être recalculé ici — la page le voit (l'observateur de
    // `menu.js`) et se mesure elle-même.
    cef_popup::execute_js(&SLOT, &format!("window.setState({{cap: {cap}, native: {native}, expanded: {expanded}}})"));
}

/// À appeler à chaque frame tant que ce popup est visible — même
/// raisonnement que `win32::cef_network::poll`, doc de fonction (élimine la
/// course avec le chargement initial de la page CEF).
pub fn poll(hwnd: HWND) {
    if cef_popup::is_visible(&SLOT) {
        push_state(hwnd);
    }
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
/// `"app/quit"` rejoint le dispatch partagé avec `win32::cef_test_window::
/// drain_actions` ; `"app/refresh_rate/<hz|auto>"` et `"app/
/// refresh_rate_menu/toggle"` sont propres à ce popup (voir `app::run`,
/// boucle sur cet appel).
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
