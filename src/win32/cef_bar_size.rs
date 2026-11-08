//! Fenêtre CEF pour le curseur de taille d'une barre (clic droit sur une
//! zone libre d'une barre) — Phase 2 de
//! `docs/plans/cef-web-ui-migration.md`. Remplace `PopupKind::BarSize`
//! (`win32::popup_menu::render_bar_size`).
//!
//! Même principe que `win32::cef_spacer_size` (curseur natif `<input
//! type="range">`, événement `input` à chaque pas — voir sa doc de module,
//! non répétée ici) mais réutilisé pour TROIS barres différentes (voir
//! `win32::menu::Bar`) : `push_state` pousse aussi `kind`
//! ("top"/"left"/"right", voir `assets/popups/bar_size.html`) pour que la
//! page affiche le bon libellé — QUELLE barre reste connue CÔTÉ RUST
//! (`CURRENT_KIND`, fixé par `toggle`), l'action reçue en retour
//! (`"bar/size/<px>"`) n'a besoin de porter que la valeur.
//!
//! **L'état est repoussé à CHAQUE frame** (`poll`, voir sa doc) et non poussé
//! une seule fois à l'ouverture : ce module avait le MÊME défaut que
//! `win32::cef_spacer_size` — mesuré là-bas, et de même forme ici — et reçoit
//! le même remède, celui déjà en place pour `cef_network`/`cef_sound`.
//!
//! Toute la mécanique fenêtre/rendu/relais souris est dans
//! `win32::cef_popup` — voir `win32::cef_test_window` pour la doc de
//! module complète du pattern suivi ici.

use std::cell::{Cell, RefCell};
use std::path::Path;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::menu::{self, Bar};

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// Quelle barre la fenêtre actuellement affichée contrôle — voir la doc
    /// de module. Valeur par défaut (`Top`) sans conséquence : jamais lue
    /// avant qu'un premier `toggle` ne l'ait fixée.
    static CURRENT_KIND: Cell<Bar> = const { Cell::new(Bar::Top) };
    /// `device_name` (voir `win32::monitors::MonitorInfo`) de l'écran
    /// SECONDAIRE pour lequel ce curseur a été ouvert, `None` pour la barre
    /// PRIMAIRE — fixé par `toggle`, lu par `dispatch_actions` : BUG RÉEL
    /// SIGNALÉ ("le popup de la barre... apparait sur l'écran principal")
    /// — sans ça, `app::run` n'avait aucun moyen de savoir à QUELLE barre
    /// (primaire ou quelle copie secondaire) réappliquer un changement de
    /// taille reçu de la page, et l'appliquait donc toujours à la primaire.
    static CURRENT_TARGET: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Voir `app::content_scale_pour`, doc de fonction — mémorisé ici à
    /// l'ouverture (voir `toggle`) et relu à chaque frame par
    /// `render_if_visible`.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// Dernières valeurs poussées — `(barre, taille)`. **Retenues ici parce
    /// que `poll` n'a aucun moyen de les relire** : il est appelé à chaque
    /// frame et ne reçoit rien, ce module n'ayant pas accès au dock. Même
    /// raison que `cef_spacer_size::CURRENT_STATE`.
    static CURRENT_STATE: Cell<(Bar, f32)> = const { Cell::new((Bar::Top, 0.0)) };
    /// Numéro d'OUVERTURE, incrémenté à chaque `toggle` — voir `push_state`
    /// et `assets/popups/bar_size.html`, doc de `appliedOpen`. Même rôle que
    /// `cef_spacer_size::OPEN_COUNT` : la page applique l'état d'une ouverture
    /// UNE SEULE FOIS, tout en recevant la poussée répétée qui la fait
    /// survivre au chargement de la page.
    static OPEN_COUNT: Cell<u64> = const { Cell::new(0) };
    /// `true` entre le premier `"bar/size/<px>"` et le `"bar/commit"` du
    /// relâchement — voir `glissement_en_cours`.
    static GLISSEMENT: Cell<bool> = const { Cell::new(false) };
}

/// Le curseur de ce popup est-il TENU ? Lu par `app::run` pour figer les panneaux
/// de la zone libre (Dash, Réglages…) pendant le glissement — DEMANDE EXPLICITE
/// (2026-10-14) : « quand je règle l'épaisseur de la barre du haut tout bouge, je
/// voudrais que le dash reste en place le temps que je règle le slider ».
///
/// ⚠️ **GARDÉ PAR LA VISIBILITÉ** : un popup refermé pendant le glissement (clic
/// ailleurs) n'enverrait jamais son `"bar/commit"`, et les panneaux resteraient
/// figés pour toujours.
pub fn glissement_en_cours() -> bool {
    GLISSEMENT.with(|g| g.get()) && cef_popup::is_visible(&SLOT)
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefBarSizeWindow"),
    window_title: w!("NeuraDesk — Taille de la barre"),
    html_file: "bar_size.html",
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

/// Voir `win32::cef_popup::toggle`. `kind`/`current_size` : QUELLE barre et
/// sa taille actuelle — retenus/poussés à la page (voir `push_state`,
/// `CURRENT_KIND`), même si la fenêtre existe déjà et n'est que
/// repositionnée (voir la doc de module).
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32, kind: Bar, current_size: f32, target: Option<&str>) {
    CURRENT_KIND.with(|c| c.set(kind));
    CURRENT_TARGET.with(|t| *t.borrow_mut() = target.map(str::to_owned));
    CONTENT_SCALE.with(|c| c.set(content_scale));
    CURRENT_STATE.with(|c| c.set((kind, current_size)));
    // Une ouverture de plus : la page appliquera cet état, MÊME si elle a déjà
    // appliqué le précédent (voir `OPEN_COUNT`).
    OPEN_COUNT.with(|c| c.set(c.get() + 1));
    cef_popup::toggle(&SLOT, &SPEC, rect, gfx);
    // Tentative immédiate — perdue en silence si la page n'a pas fini de
    // charger, c'est `poll` qui la rattrape (voir sa doc).
    push_state();
}

/// Pousse l'état de l'ouverture courante dans la page via `window.setState`
/// (voir `assets/popups/bar_size.html`).
fn push_state() {
    let (kind, size) = CURRENT_STATE.with(Cell::get);
    let open = OPEN_COUNT.with(Cell::get);
    // `[haut, gauche, droite]` — le MÊME ordre que `ui::dock::BAR_THICKNESS_MIN_4K`
    // et que l'index de la barre passée par `app::run`.
    let (kind_str, bars) = match kind {
        Bar::Top => ("top", 0),
        Bar::Left => ("left", 1),
        Bar::Right => ("right", 2),
        Bar::Bottom => ("bottom", 3),
    };
    // LES BORNES SONT POUSSÉES, jamais recopiées dans la page : c'est
    // `ui::dock::bar_thickness_bounds` qui les décide, pour ce popup COMME
    // pour le curseur des Réglages (voir sa doc — les deux littéraux d'origine
    // avaient divergé, 160 contre 500, sans que rien ne le signale).
    let (min, max) = crate::ui::dock::bar_thickness_bounds(bars);
    cef_popup::execute_js(
        &SLOT,
        &format!("window.setState({{open: {open}, kind: \"{kind_str}\", size: {size}, min: {min}, max: {max}}})"),
    );
}

/// À appeler à CHAQUE frame tant que ce popup est visible (voir `app::run`,
/// même endroit que `win32::cef_network::poll`/`win32::cef_sound::poll`) —
/// même remède au même bug que `win32::cef_spacer_size::poll`, dont la doc
/// porte le détail de la mesure : le `push_state()` de `toggle()`, appelé
/// juste après la création de la fenêtre, peut arriver AVANT que
/// `window.setState` n'existe côté JS et être perdu en silence (la page
/// s'affiche alors avec sa valeur par défaut, et avec le libellé par défaut).
///
/// **La page n'applique pourtant cet état QU'UNE FOIS par ouverture** (voir
/// son `appliedOpen`) : repousser la même ouverture à chaque frame ne doit pas
/// réécrire une poignée que l'utilisateur est en train de glisser.
pub fn poll() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

/// Vide les actions demandées par `assets/popups/bar_size.html` (format
/// `"bar/size/<px>"`) et retourne `(kind, taille)` pour LA BARRE
/// COURAMMENT OUVERTE (voir `CURRENT_KIND`) si au moins une action a été
/// reçue depuis le dernier appel — `None` sinon. Voir
/// `win32::cef_spacer_size::dispatch_actions`, doc de fonction : même
/// raisonnement pour l'absence de rafraîchissement en retour.
pub fn dispatch_actions() -> Option<(Bar, f32, Option<String>)> {
    let mut result = None;
    for action in cef_popup::drain_actions(&SLOT) {
        if action == "bar/commit" {
            GLISSEMENT.with(|g| g.set(false));
            continue;
        }
        let Some(px) = action.strip_prefix("bar/size/") else {
            eprintln!("[neuradesk][cef] action barre inconnue : {action:?}");
            continue;
        };
        match px.parse::<f32>() {
            Ok(size) => {
                GLISSEMENT.with(|g| g.set(true));
                result = Some((CURRENT_KIND.with(|c| c.get()), size, CURRENT_TARGET.with(|t| t.borrow().clone())));
            }
            Err(_) => eprintln!("[neuradesk][cef] taille de barre invalide : {px:?}"),
        }
    }
    result
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
