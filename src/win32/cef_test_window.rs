//! Fenêtre CEF pour l'icône Alimentation (voir
//! `docs/plans/cef-web-ui-migration.md`) — née comme fenêtre de test Phase
//! 0 (déclenchée UNIQUEMENT par `NEURADESK_DEBUG_OPEN=cef-test`), sert
//! maintenant AUSSI le vrai clic sur l'icône Alimentation du dock (voir
//! `app::run`) : preuve de bout en bout que le pipeline CEF OSR accéléré
//! compose correctement dans le rendu D3D11 existant, EN CONDITIONS
//! RÉELLES. L'ancien `PopupKind::Power` (`win32::popup_menu::render_power`)
//! reste dans le code, inutilisé pour cette icône — voir la doc de
//! `toggle` pour pourquoi.
//!
//! Toute la mécanique fenêtre/rendu/relais souris est dans
//! `win32::cef_popup` (voir sa doc de module) — ce fichier ne fait que
//! déclarer l'identité de CE popup (`SPEC`) et son propre `thread_local`/
//! wndproc, comme `win32::cef_calendar`/`win32::cef_context`/
//! `win32::cef_network`.

use std::cell::{Cell, RefCell};
use std::path::Path;

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::eveil;
use crate::win32::menu;

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// Voir `app::content_scale_pour`, doc de fonction — mémorisé ici à
    /// l'ouverture (voir `toggle`) et relu à chaque frame par
    /// `render_if_visible`.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// La clé du dernier `window.setState(...)` émis — voir `push_state`.
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefTestWindow"),
    window_title: w!("NeuraDesk — Alimentation"),
    html_file: "power.html",
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

/// Voir `win32::cef_popup::toggle`, doc de fonction — même bascule RÉELLE
/// (masque si déjà visible, montre + repositionne sinon), utilisée à la
/// fois par le harnais de vérification (`NEURADESK_DEBUG_OPEN=cef-test`) ET
/// par un vrai clic sur l'icône Alimentation (voir `app::run`) depuis que
/// cette fenêtre a remplacé le popup `egui` `PopupKind::Power` pour cette
/// icône précise (Phase 0 de `docs/plans/cef-web-ui-migration.md`) —
/// L'ANCIEN `PopupKind::Power` (`win32::popup_menu::render_power`) reste
/// dans le code, INUTILISÉ pour l'instant : retiré seulement une fois la
/// migration des popups restants terminée, pas avant, pour garder un
/// chemin de retour simple en cas de régression.
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    CONTENT_SCALE.with(|c| c.set(content_scale));
    cef_popup::toggle(&SLOT, &SPEC, rect, gfx);
    push_state();
}

/// Repousse l'état « Garder éveillé » (voir `push_state`) tant que la fenêtre
/// est visible — à appeler à CHAQUE frame (voir `app::run`). `PAYLOAD` rend
/// l'appel gratuit tant que rien de VISIBLE ne change (la clé n'avance qu'à la
/// minute près, pas à la frame).
pub fn refresh_if_visible() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

/// Pousse l'état dans `assets/popups/power.html` via `window.setState` — le
/// SEUL moyen pour cette page de savoir si « Garder éveillé » est actif (voir
/// `win32::eveil`, qui porte la demande Windows elle-même).
fn push_state() {
    let cle = eveil::cle();
    if PAYLOAD.with(|p| p.borrow().as_deref() == Some(cle.as_str())) {
        return;
    }
    let payload = json!({ "eveil": eveil::json() });
    PAYLOAD.with(|p| *p.borrow_mut() = Some(cle));
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

/// Voir `win32::cef_popup::debug_hover` — réservé au harnais de
/// vérification (`NEURADESK_DEBUG_OPEN=cef-test`).
pub fn debug_hover(x: i32, y: i32) {
    cef_popup::debug_hover(&SLOT, x, y);
}

/// Voir `win32::cef_popup::debug_click` — réservé au harnais de
/// vérification.
pub fn debug_click(x: i32, y: i32) {
    cef_popup::debug_click(&SLOT, x, y);
}

/// Voir `win32::cef_popup::execute_js` — réservé au harnais de vérification
/// (`NEURADESK_DEBUG_SONDE_CANAL`, voir `win32::cef_data::SONDE_JS`).
pub fn debug_execute_js(code: &str) {
    cef_popup::execute_js(&SLOT, code);
}

/// Vide et retourne les actions demandées par `assets/popups/power.html`
/// (voir `win32::cef_popup::drain_actions`) — À appeler à chaque frame
/// (voir `app::run`, juste après `render_if_visible`), format
/// `"power/<action>"`/`"app/quit"`.
pub fn drain_actions() -> Vec<String> {
    cef_popup::drain_actions(&SLOT)
}

/// BUG RÉEL SIGNALÉ ("le menu d'alimentation... ne suit pas l'apparence de
/// l'app") : ce popup a la même géométrie/ancrage (`anchor_i32` + `MENU_GAP`
/// sous l'icône Alimentation) que les autres popups "menu"
/// (Calendrier/Contexte/Réseau/Son/Voicemeeter...), PAS celle des panneaux
/// plein écran type Dash/Réglages/Presse-papiers (voir `win32::cef_dash`,
/// avec son propre système d'accent séparé) — la référence à `cef_dash`
/// qui justifiait `0.0, None` ici était erronée. Il suit donc désormais le
/// même rayon d'arrondi et le même zoom que ses pairs "menu".
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

#[cfg(test)]
mod tests {
    const PAGE: &str = include_str!("../../assets/popups/power.html");
    /// Le dispatch des actions de ce popup est dans `app::run` (voir
    /// `dispatch_power_or_quit_action`) — même lecture que `cef_network`.
    const APP: &str = include_str!("../app.rs");

    /// Les verbes `power/…` que la page envoie. `power/eveil/<minutes>` est
    /// ramené au PRÉFIXE `power/eveil/`, que le dispatch traite comme tel.
    fn verbes_de_la_page() -> Vec<String> {
        let mut v: Vec<String> = PAGE
            .match_indices("neuradesk://")
            .filter_map(|(i, _)| {
                let reste = &PAGE[i + "neuradesk://".len()..];
                let fin = reste.find(['"', '\''])?;
                Some(reste[..fin].to_string())
            })
            .filter(|v| v.starts_with("power/"))
            .map(|v| {
                if v.starts_with("power/eveil/") {
                    "power/eveil/".to_string()
                } else {
                    v
                }
            })
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// Les verbes du bras de dispatch, lus dans `app.rs`.
    fn verbes_du_dispatch() -> Vec<String> {
        let debut = APP.find("fn dispatch_power_or_quit_action").expect("dispatch introuvable");
        let apres = &APP[debut..];
        let fin = apres.find("\npub fn reload_was_requested").expect("fin du dispatch introuvable");
        let corps = &apres[..fin];
        let mut v: Vec<String> = corps
            .match_indices("\"power/")
            .map(|(i, _)| {
                let reste = &corps[i + 1..];
                reste[..reste.find('"').expect("littéral non terminé")].to_string()
            })
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// ⛔ Un verbe présent d'un seul côté est mort en silence : la page
    /// l'envoie dans le vide, ou le dispatch attend une action que personne
    /// n'envoie.
    #[test]
    fn la_page_et_le_dispatch_acceptent_les_memes_verbes() {
        assert_eq!(
            verbes_de_la_page(),
            verbes_du_dispatch(),
            "la page et `dispatch_power_or_quit_action` doivent citer les mêmes verbes"
        );
    }

    /// Ce que Rust appelle dans la page doit y exister, sinon l'état se perd
    /// EN SILENCE (même essai que `cef_network`/`cef_sound`).
    #[test]
    fn la_page_definit_ce_que_rust_appelle() {
        assert!(PAGE.contains("window.setState"), "power.html doit définir window.setState");
        for champ in ["eveil", "actif", "sansLimite", "resteMin"] {
            assert!(PAGE.contains(champ), "la page n'exploite pas `{champ}`, que `push_state` envoie");
        }
    }

    /// **LA HAUTEUR DE LA FENÊTRE EST ESTIMÉE, PAS MESURÉE** (voir
    /// `cef_popup::button_list_height`) : un bouton ajouté à la page sans
    /// toucher aux deux appels d'`app.rs` le pousse sous le pli, hors de portée
    /// d'un clic. Le compte fait foi des DEUX côtés.
    #[test]
    fn la_hauteur_estimee_suit_le_contenu_de_la_page() {
        assert_eq!(PAGE.matches("</button>").count(), 10, "compter les boutons RÉELS (les commentaires citent `<button`)");
        assert_eq!(PAGE.matches("<hr>").count(), 2);
        assert_eq!(
            APP.matches("button_list_height(10, 2, 1)").count(),
            2,
            "le harnais (cef-test) ET le vrai clic sur l'icône Alimentation l'appellent"
        );
    }
}
