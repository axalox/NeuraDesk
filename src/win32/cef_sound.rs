//! Fenêtre CEF pour l'icône Son — Phase 1 de
//! `docs/plans/cef-web-ui-migration.md`. Remplace `PopupKind::Sound`
//! (`win32::popup_menu::render_sound`) pour cette icône précise — le plus
//! gros popup de cette phase (listes de périphériques dynamiques, volume,
//! mute, sortie ET entrée).
//!
//! Comme `win32::cef_network`, a besoin d'un canal Rust→JS (`push_state`) :
//! l'état audio (`win32::audio`) n'est pas calculable en JS pur.
//! Contrairement à Network (deux booléens), l'état ici est structuré
//! (listes de périphériques, IDs opaques Windows) — sérialisé en JSON via
//! `serde_json` (déjà une dépendance directe, voir `Cargo.toml`) plutôt
//! qu'une interpolation de chaîne à la main.
//!
//! Les IDs de périphérique peuvent contenir des caractères invalides tels
//! quels dans une URL (accolades, points, deux-points — voir
//! `win32::audio::AudioDevice`) : encodés côté JS (`encodeURIComponent`,
//! voir `assets/popups/sound.html::go`), décodés ici (voir
//! `dispatch_actions`, `win32::cef_app::percent_decode`) — UNIQUEMENT sur
//! le segment concerné, jamais sur l'action entière (voir la doc de
//! `percent_decode`).
//!
//! Toute la mécanique fenêtre/rendu/relais souris est dans
//! `win32::cef_popup` — voir `win32::cef_test_window` pour la doc de
//! module complète du pattern suivi ici.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::audio;
use crate::win32::cef_app::percent_decode;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::menu;

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// Voir `app::content_scale_pour`, doc de fonction — mémorisé ici à
    /// l'ouverture (voir `toggle`) et relu à chaque frame par
    /// `render_if_visible`, `PopupSlot` n'ayant pas connaissance de la
    /// résolution des écrans (module `app`, pas `win32::cef_popup`).
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// Les applications de la sortie par défaut, relues au plus toutes les
    /// `APPS_TTL` : `poll` appelle `push_state` à CHAQUE frame, et énumérer
    /// les sessions WASAPI + lire les noms d'exécutables à cette cadence
    /// serait du gaspillage pur.
    static APPS: RefCell<Option<(Instant, Vec<audio::AudioApp>)>> = RefCell::new(None);
}

/// Intervalle de relecture du mélangeur par application (voir `APPS`).
const APPS_TTL: Duration = Duration::from_millis(500);

/// Les apps de la sortie par défaut, depuis le cache sauf si `forcer`.
fn apps_actuelles(forcer: bool) -> Vec<audio::AudioApp> {
    APPS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !forcer {
            if let Some((quand, apps)) = cache.as_ref() {
                if quand.elapsed() < APPS_TTL {
                    return apps.clone();
                }
            }
        }
        let apps = audio::apps_on_default_output();
        *cache = Some((Instant::now(), apps.clone()));
        apps
    })
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefSoundWindow"),
    window_title: w!("NeuraDesk — Son"),
    html_file: "sound.html",
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

/// Voir `win32::cef_popup::toggle`. Pousse aussi l'état audio actuel (voir
/// `push_state`) après avoir (re)montré/créé/masqué la fenêtre — même
/// politique que `win32::cef_network::toggle` (doc détaillée là-bas : un
/// `execute_java_script` sur une page cachée est un gaspillage négligeable,
/// pas une erreur).
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    CONTENT_SCALE.with(|c| c.set(content_scale));
    cef_popup::toggle(&SLOT, &SPEC, rect, gfx);
    push_state();
}

/// Pousse l'état audio ACTUEL (voir `win32::audio`) dans la page via
/// `window.setState` (voir `assets/popups/sound.html`) — à appeler à
/// l'ouverture du popup (voir `toggle`) ET après chaque action effective
/// (voir `dispatch_actions`), pour que l'affichage ne reste jamais périmé.
///
/// Les APPLICATIONS, elles, passent par `apps_actuelles` (cache de
/// `APPS_TTL`) : c'est la seule partie de cet état dont la lecture coûte
/// vraiment quelque chose.
pub fn push_state() {
    pousser(false);
}

/// Voir `push_state`. `forcer` relit les sessions tout de suite — c'est le cas
/// après une action de l'utilisateur, dont l'effet doit se voir immédiatement.
fn pousser(forcer: bool) {
    let output = audio::default_output();
    let input = audio::default_input();
    let output_volume = audio::output_volume_percent();
    let output_muted = audio::output_muted().unwrap_or(false);
    let input_muted = audio::input_muted().unwrap_or(false);

    let output_json =
        output.map(|d| json!({ "id": d.id, "name": d.name, "volume": output_volume, "muted": output_muted }));
    let input_json = input.map(|d| json!({ "id": d.id, "name": d.name, "muted": input_muted }));
    let outputs_json: Vec<_> = audio::list_outputs().into_iter().map(|d| json!({ "id": d.id, "name": d.name })).collect();
    let inputs_json: Vec<_> = audio::list_inputs().into_iter().map(|d| json!({ "id": d.id, "name": d.name })).collect();
    let apps_json: Vec<_> = apps_actuelles(forcer)
        .into_iter()
        .map(|app| {
            json!({
                "pid": app.pid,
                "name": app.name,
                "volume": app.volume,
                "muted": app.muted,
                "active": app.active,
            })
        })
        .collect();

    let payload = json!({
        "output": output_json,
        "outputs": outputs_json,
        "input": input_json,
        "inputs": inputs_json,
        "apps": apps_json,
    });
    // `payload` (un `serde_json::Value`) affiche du JSON valide via `Display`
    // — un objet littéral JS ordinaire, `window.setState` n'a besoin de rien
    // de plus.
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
}

/// À appeler à CHAQUE frame tant que ce popup est visible (voir `app::run`,
/// même endroit que `win32::cef_voicemeeter::poll`/`win32::cef_network::
/// poll`) — même remède au même bug : le `push_state()` de `toggle()`,
/// appelé juste après la création de la fenêtre, peut arriver avant que
/// `window.setState` n'existe côté JS et être perdu en silence (BUG RÉEL
/// SIGNALÉ : "il faut ouvrir 2 fois les menus pour qu'ils soient remplis").
pub fn poll() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

/// Vide les actions demandées par `assets/popups/sound.html` (voir
/// `assets/popups/sound.html::go`, format `"sound/<verbe>[/<segment
/// encodé>]"`) et les applique via `win32::audio` — à appeler à chaque
/// frame (voir `app::run`, même endroit que les autres popups). Pousse un
/// état rafraîchi (voir `push_state`) si au moins une action a été reçue,
/// pour que l'affichage reflète immédiatement le nouvel état sans attendre
/// une fermeture/réouverture du popup.
pub fn dispatch_actions() {
    let actions = cef_popup::drain_actions(&SLOT);
    if actions.is_empty() {
        return;
    }
    // ⏱ **UN CURSEUR GLISSÉ ENVOIE UNE ACTION PAR ÉVÉNEMENT.** Ne garder que
    // la DERNIÈRE valeur par application : appliquer les précédentes serait
    // autant d'énumérations de sessions pour un état déjà périmé.
    let mut gardees: Vec<String> = Vec::with_capacity(actions.len());
    for action in actions {
        if let Some(reste) = action.strip_prefix("sound/set-app-volume/") {
            if let Some(pid) = reste.split('/').next() {
                gardees.retain(|autre| {
                    autre.strip_prefix("sound/set-app-volume/").and_then(|r| r.split('/').next()) != Some(pid)
                });
            }
        }
        gardees.push(action);
    }

    // La liste des applications n'est relue TOUT DE SUITE que si l'action
    // change quelque chose que le curseur ne reflète pas déjà : `pousser(true)`
    // relit sessions ET noms d'exécutables, à ne pas faire à chaque événement.
    let mut forcer = false;
    for action in &gardees {
        let Some(rest) = action.strip_prefix("sound/") else {
            eprintln!("[neuradesk][cef] action son inconnue : {action:?}");
            continue;
        };
        // Scinde AVANT de décoder quoi que ce soit (voir la doc de
        // `win32::cef_app::percent_decode`) : seul le segment après le
        // premier '/' peut contenir un ID encodé, jamais le verbe lui-même.
        let (verb, arg) = match rest.split_once('/') {
            Some((verb, arg)) => (verb, Some(arg)),
            None => (rest, None),
        };
        match (verb, arg) {
            ("toggle-output-mute", _) => audio::toggle_output_mute(),
            ("toggle-input-mute", _) => audio::toggle_input_mute(),
            ("set-output-volume", Some(percent)) => match percent_decode(percent).parse::<u8>() {
                Ok(percent) => audio::set_output_volume_percent(percent),
                Err(_) => eprintln!("[neuradesk][cef] volume de sortie invalide : {percent:?}"),
            },
            ("set-output-device", Some(id)) => audio::set_default_output(&percent_decode(id)),
            ("set-input-device", Some(id)) => audio::set_default_input(&percent_decode(id)),
            // `set-app-volume/<pid>/<pourcentage>` — DEUX numéros, découpés
            // avant tout décodage (même règle que ci-dessus, voir
            // `percent_decode`). Les deux sont des chiffres : rien à décoder.
            ("set-app-volume", Some(reste)) => match reste.split_once('/') {
                Some((pid, percent)) => match (pid.parse::<u32>(), percent.parse::<u8>()) {
                    (Ok(pid), Ok(percent)) => audio::set_app_volume_percent(pid, percent),
                    _ => eprintln!("[neuradesk][cef] volume d'application invalide : {reste:?}"),
                },
                None => eprintln!("[neuradesk][cef] volume d'application sans pourcentage : {reste:?}"),
            },
            ("toggle-app-mute", Some(pid)) => match pid.parse::<u32>() {
                Ok(pid) => audio::toggle_app_mute(pid),
                Err(_) => eprintln!("[neuradesk][cef] application invalide : {pid:?}"),
            },
            _ => eprintln!("[neuradesk][cef] action son inconnue : {action:?}"),
        }
        if verb != "set-app-volume" {
            forcer = true;
        }
    }
    pousser(forcer);
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

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

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = include_str!("../../assets/popups/sound.html");

    /// Les verbes que `dispatch_actions` accepte — même lecture que
    /// `cef_automatisations::tests::verbes_du_dispatch`.
    fn verbes_du_dispatch() -> Vec<String> {
        let source = include_str!("cef_sound.rs");
        source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(source)
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//"))
            .filter_map(|l| {
                let reste = l.strip_prefix("(\"")?;
                let (verbe, fin) = reste.split_once('"')?;
                matches!(fin.chars().next(), Some(')') | Some(',')).then(|| verbe.to_string())
            })
            .collect()
    }

    /// Les verbes que la page envoie via `go("…")`.
    fn verbes_de_la_page() -> Vec<String> {
        PAGE.match_indices("go(")
            .filter_map(|(i, _)| {
                let reste = &PAGE[i + 3..];
                let guillemet = reste.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                reste[1..].split(guillemet).next().map(str::to_string)
            })
            .collect()
    }

    /// ⛔ **LE SEUL TEST QUI GARDE CETTE PAGE.** Un verbe ajouté d'un côté
    /// seulement est mort en silence : la page l'envoie dans le vide, ou le
    /// dispatch attend une action que personne n'envoie.
    #[test]
    fn la_page_et_le_dispatch_acceptent_les_memes_verbes() {
        let mut page = verbes_de_la_page();
        let mut rust = verbes_du_dispatch();
        page.sort();
        page.dedup();
        rust.sort();
        rust.dedup();
        assert_eq!(page, rust);
    }

    /// Ce que Rust appelle dans la page doit y exister, sinon l'état se perd
    /// EN SILENCE (même essai que `cef_automatisations`).
    #[test]
    fn la_page_definit_ce_que_rust_appelle() {
        assert!(PAGE.contains("window.setState"), "sound.html doit définir window.setState");
    }
}
