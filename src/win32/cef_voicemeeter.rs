//! Fenêtre CEF pour l'icône Voicemeeter — Phase 3 de
//! `docs/plans/cef-web-ui-migration.md`. Remplace `PopupKind::Voicemeeter`
//! (`win32::popup_menu::render_voicemeeter`) — LE popup qui a motivé toute
//! cette migration (voir `assets/popups/voicemeeter.html`, doc d'en-tête) :
//! sa table à 5 colonnes alignées souffrait de plusieurs bugs RÉELS
//! d'`egui_extras::TableBuilder`, documentés en détail dans
//! `win32::popup_menu.rs` (recherche `BUG RÉEL` autour de
//! `render_voicemeeter`) — CSS Grid les évite tous nativement.
//!
//! Contrairement à `win32::cef_network`/`win32::cef_sound` (état poussé
//! seulement à l'ouverture et après une action), l'état est poussé À
//! CHAQUE FRAME tant que ce popup est visible (voir `poll`,
//! `win32::cef_popup::is_visible`) — Voicemeeter peut changer depuis lui-
//! même ou une autre application connectée à son API, pas seulement depuis
//! ce popup (même garantie que `win32::popup_menu::render_voicemeeter`,
//! qui relit l'état à chaque frame egui pour la même raison). Le risque
//! qu'un rafraîchissement aussi fréquent casse un glissement de curseur en
//! cours est traité CÔTÉ JS (voir `assets/popups/voicemeeter.html` :
//! reconstruction du DOM seulement si la structure change, mise à jour en
//! place sinon, jamais sur un curseur qui a le focus).
//!
//! Toute la mécanique fenêtre/rendu/relais souris est dans
//! `win32::cef_popup` — voir `win32::cef_test_window` pour la doc de
//! module complète du pattern suivi ici.

use std::cell::Cell;
use std::path::Path;

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::config;
use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::menu;
use crate::win32::voicemeeter;

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// Voir `app::content_scale_pour`, doc de fonction — mémorisé ici à
    /// l'ouverture (voir `toggle`) et relu à chaque frame par
    /// `render_if_visible`.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefVoicemeeterWindow"),
    window_title: w!("NeuraDesk — Voicemeeter"),
    html_file: "voicemeeter.html",
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

/// Voir `win32::cef_popup::toggle`. Pousse aussi l'état actuel (voir
/// `push_state`) — même politique que `win32::cef_network::toggle`/
/// `win32::cef_sound::toggle`. Les rafraîchissements suivants viennent de
/// `poll` (voir sa doc), appelée à chaque frame par `app::run` tant que ce
/// popup reste visible.
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    CONTENT_SCALE.with(|c| c.set(content_scale));
    cef_popup::toggle(&SLOT, &SPEC, rect, gfx);
    push_state();
}

/// À appeler à CHAQUE frame (voir `app::run`, même endroit que les autres
/// popups) — no-op bon marché tant que la fenêtre n'est pas visible (voir
/// `win32::cef_popup::is_visible`). Applique les actions reçues (voir
/// `dispatch_actions`) PUIS pousse un état frais (voir `push_state`), pour
/// que Voicemeeter lui-même ou une autre application connectée à son API
/// reste reflété en temps réel — voir la doc de module.
pub fn poll() {
    if !cef_popup::is_visible(&SLOT) {
        return;
    }
    dispatch_actions();
    push_state();
}

/// Vide les actions demandées par `assets/popups/voicemeeter.html` (format
/// `"voicemeeter/<segments>"`, voir sa fonction `go()`) et les applique via
/// `win32::voicemeeter`/`crate::config` — même effet que chaque
/// `PopupAction::Voicemeeter*` correspondant dans `app::run` (voir
/// `win32::popup_menu::PopupAction`), reproduit ici à l'identique plutôt
/// que par un dispatch central : le format d'action de ce popup (segments
/// `/`-séparés typés par position, voir le `match` ci-dessous) n'a de sens
/// que pour ce module.
fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(rest) = action.strip_prefix("voicemeeter/") else {
            eprintln!("[neuradesk][cef] action voicemeeter inconnue : {action:?}");
            continue;
        };
        let parts: Vec<&str> = rest.split('/').collect();
        match parts.as_slice() {
            ["launch"] => voicemeeter::launch(),
            ["strip", i, "gain", db] => set_gain("Strip", i, db),
            ["strip", i, "mute"] => toggle_mute("Strip", i),
            ["strip", i, "bus", bus_name] => toggle_bus(i, bus_name),
            ["strip", i, "hidden"] => toggle_hidden_strip(i),
            ["bus", i, "gain", db] => set_gain("Bus", i, db),
            ["bus", i, "mute"] => toggle_mute("Bus", i),
            ["bus", i, "hidden"] => toggle_hidden_bus(i),
            _ => eprintln!("[neuradesk][cef] action voicemeeter inconnue : {action:?}"),
        }
    }
}

fn set_gain(kind: &str, index: &str, db: &str) {
    match db.parse::<f32>() {
        Ok(db) => voicemeeter::set_float(&format!("{kind}[{index}].Gain"), db),
        Err(_) => eprintln!("[neuradesk][cef] gain voicemeeter invalide : {db:?}"),
    }
}

fn toggle_mute(kind: &str, index: &str) {
    let param = format!("{kind}[{index}].Mute");
    let muted = voicemeeter::get_float(&param).is_some_and(|v| v != 0.0);
    voicemeeter::set_float(&param, if muted { 0.0 } else { 1.0 });
}

fn toggle_bus(strip_index: &str, bus_name: &str) {
    let param = format!("Strip[{strip_index}].{bus_name}");
    let assigned = voicemeeter::get_float(&param).is_some_and(|v| v != 0.0);
    voicemeeter::set_float(&param, if assigned { 0.0 } else { 1.0 });
}

fn toggle_hidden_strip(index: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let (mut hidden_strips, hidden_buses) = config::load_voicemeeter_hidden_channels();
    match hidden_strips.iter().position(|&h| h == index) {
        Some(pos) => {
            hidden_strips.remove(pos);
        }
        None => hidden_strips.push(index),
    }
    config::save_voicemeeter_hidden_channels(&hidden_strips, &hidden_buses);
}

fn toggle_hidden_bus(index: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let (hidden_strips, mut hidden_buses) = config::load_voicemeeter_hidden_channels();
    match hidden_buses.iter().position(|&h| h == index) {
        Some(pos) => {
            hidden_buses.remove(pos);
        }
        None => hidden_buses.push(index),
    }
    config::save_voicemeeter_hidden_channels(&hidden_strips, &hidden_buses);
}

/// Pousse l'état Voicemeeter ACTUEL dans la page via `window.setState`
/// (voir `assets/popups/voicemeeter.html`) — voir `toggle`/`poll` pour
/// quand cette fonction est appelée.
fn push_state() {
    let payload = match voicemeeter::snapshot() {
        None => json!({ "installed": false }),
        Some(snapshot) if !snapshot.running => json!({ "installed": true, "running": false }),
        Some(snapshot) => {
            let (hidden_strip_ids, hidden_bus_ids) = config::load_voicemeeter_hidden_channels();
            let bus_labels: Vec<String> =
                (0..snapshot.bus_count).map(|i| voicemeeter::bus_label(snapshot.vtype, i)).collect();
            let strips = collect_rows("Strip", snapshot.strip_count, &hidden_strip_ids, &bus_labels, |i| {
                format!("Entrée {}", i + 1)
            });
            let buses = collect_rows("Bus", snapshot.bus_count, &hidden_bus_ids, &[], |i| bus_labels[i].clone());

            // **CE POPUP NE SE REDIMENSIONNE PLUS LUI-MÊME.** Il le faisait, et
            // c'était le remède d'un BUG RÉEL SIGNALÉ (« le menu voicemeeter...
            // ne semble pas se redimensionner automatiquement en hauteur ») ;
            // il est maintenant remplacé par mieux — la PAGE mesure son propre
            // contenu et l'annonce (`menu/height/…`, voir
            // `assets/popups/menu.js` et `cef_popup::drain_actions`).
            //
            // **IL FALLAIT LE RETIRER, pas le laisser en secours** : `app::run`
            // appelle ce `push_state` (via `poll`) AVANT le drainage des actions
            // de la MÊME frame, donc cette estimation aurait gagné à chaque frame
            // et la mesure de la page n'aurait jamais été visible.
            //
            // Ce qui disparaît avec lui : le comptage des canaux VISIBLES et la
            // détection de la ligne « Canaux masqués », qui n'existaient que pour
            // l'estimation. La page, elle, voit son DOM — elle n'a rien à compter.

            json!({ "installed": true, "running": true, "strips": strips, "buses": buses })
        }
    };
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
}

// `estimated_content_height` A ÉTÉ SUPPRIMÉE le 2026-09-19, avec ses trois tests.
//
// Elle estimait la hauteur à partir du NOMBRE de lignes, avec des constantes
// calibrées sur UNE mesure réelle (1108 px pour 5 tranches + 3 bus + la ligne
// « Canaux masqués »). C'était le remède d'un bug réel (« le menu voicemeeter ne
// semble pas se redimensionner automatiquement en hauteur »), mais un remède
// aveugle : il ne pouvait pas savoir ce que la page affichait vraiment — un dépli
// de la section des canaux masqués lui échappait, et c'est le CSS qui rattrapait
// (`overflow-y: auto`).
//
// La page se mesure désormais elle-même (`assets/popups/menu.js`,
// `menu/height/…`, appliqué par `cef_popup::drain_actions`) : plus de constantes
// à recalibrer, et le dépli est vu comme le reste.

/// Relit `count` canaux `{kind}[0..count]` et les sérialise pour
/// `push_state` — voir `win32::popup_menu::collect_voicemeeter_rows`, même
/// principe (un seul aller-retour groupé par `kind`, réutilisé pour les
/// strips ET les bus) porté ici en JSON plutôt qu'en `Vec<
/// VoicemeeterChannelRow>` egui. `bus_labels` : les noms de bus à
/// interroger pour l'assignation (`Strip[i].{nom}`) — VIDE pour les bus
/// eux-mêmes, comme côté egui.
fn collect_rows(
    kind: &str,
    count: usize,
    hidden_ids: &[usize],
    bus_labels: &[String],
    generic_label: impl Fn(usize) -> String,
) -> Vec<serde_json::Value> {
    (0..count)
        .map(|index| {
            let label = voicemeeter::get_string(&format!("{kind}[{index}].Label"))
                .or_else(|| voicemeeter::get_string(&format!("{kind}[{index}].device.name")))
                .unwrap_or_else(|| generic_label(index));
            let gain = voicemeeter::get_float(&format!("{kind}[{index}].Gain")).unwrap_or(0.0);
            let muted = voicemeeter::get_float(&format!("{kind}[{index}].Mute")).is_some_and(|v| v != 0.0);
            let bus_assignments: Vec<(String, bool)> = bus_labels
                .iter()
                .map(|bus_name| {
                    let active = voicemeeter::get_float(&format!("{kind}[{index}].{bus_name}")).is_some_and(|v| v != 0.0);
                    (bus_name.clone(), active)
                })
                .collect();
            json!({
                "index": index,
                "label": label,
                "gain": gain,
                "muted": muted,
                "hidden": hidden_ids.contains(&index),
                "busAssignments": bus_assignments,
            })
        })
        .collect()
}

// PAS de `hide_if_focus_elsewhere` ici — voir `app::poll_overlay_windows`,
// doc du site d'appel omis : exception permanente et délibérée (demande
// explicite reconduite depuis `win32::popup_menu::hide_if_focus_elsewhere`),
// pas un oubli — ce popup ne se masque QUE via un second clic sur son
// icône (`toggle`) ou `hide_if_visible` (une autre fenêtre prend le
// premier plan), jamais sur une simple perte de focus passive.

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
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
