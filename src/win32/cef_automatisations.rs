//! Le DASH des AUTOMATISATIONS « quand… alors… » — DEMANDE EXPLICITE
//! (2026-10-04) : « des automatisations simples du type *quand… alors…* », dans
//! « une applet avec son propre dash, dans la zone Réglages ».
//!
//! Un PANNEAU, comme la Recherche dont il reprend la structure : il remplit la
//! zone libre du dock. Il ne porte AUCUN geste — le moteur vit dans
//! `win32::automatisations` (modèle, fichier, déclencheurs, exécution), et cette
//! page ne fait que MONTRER cet état et le MODIFIER.
//!
//! # L'état va et vient par deux chemins différents, et c'est voulu
//!
//! * **Rust → page** : `window.setState({...})`, à chaque frame où le panneau est
//!   visible, avec la liste ET le journal (`automatisations::json()`). Une
//!   automatisation qui se déclenche pendant que le dash est ouvert se voit donc
//!   sans rien faire.
//! * **page → Rust** : la liste ENTIÈRE part par `cef_data` (un POST détourné,
//!   voir `win32::cef_data`) sur `automatisations/liste`, puis la page émet
//!   `editer` — même motif que le volet des commandes (`commandes::remplacer_depuis`).
//!   ⚠️ **C'est la LISTE ENTIÈRE, jamais une entrée** : le moteur remplace son
//!   état d'un coup, donc il n'y a pas de fenêtre où il évaluerait une liste à
//!   moitié écrite.
//!
//! Actions reçues (`"automatisations/<verbe>/…"`, segments ENCODÉS) :
//! - `ready` — la page a chargé (voir `PAYLOAD`)
//! - `close`
//! - `editer` — le corps est arrivé par `cef_data`
//! - `tester/<nom>` — exécute les actions tout de suite
//!
//! ⚠️ **« Tester » N'EXÉCUTE RIEN ICI** : `automatisations::tester` met les
//! actions dans la MÊME file que le moteur, et c'est `app::run` qui les applique
//! — le test emprunte donc le chemin d'exécution réel (voir la doc de
//! `automatisations::prendre_a_executer`).

use std::cell::{Cell, RefCell};

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};

thread_local! {
    static SLOT: PopupSlot = PopupSlot::new();
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// La dernière charge utile POUSSÉE — la comparaison rend l'appel gratuit
    /// tant que rien n'a changé (le moteur, lui, tourne à 60 Hz).
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// **LA ROUTE DU CORPS**, côté `cef_data` — la page y dépose sa liste.
const ROUTE: &str = "automatisations/liste";

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefAutomatisationsWindow"),
    window_title: w!("NeuraDesk — Automatisations"),
    html_file: "automatisations.html",
    wndproc: Some(wndproc),
    // PANNEAU — même famille que la recherche et la calculatrice.
    above_a_magnified_dock: false,
    fades: false,
    fenetre: false,
};

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// Masque la « zone libre » avant d'occuper la place — les douze panneaux qui la
/// partagent, et réciproquement (chacun des douze nous masque).
fn masquer_les_voisins() {
    crate::win32::cef_dash::hide_if_visible();
    crate::win32::cef_settings::hide_if_visible();
    crate::win32::cef_notes::hide_if_visible();
    crate::win32::cef_image_editor::hide_if_visible();
    crate::win32::cef_calendar_applet::hide_if_visible();
    crate::win32::cef_clipboard::hide_if_visible();
    crate::win32::cef_surveillance::hide_if_visible();
    crate::win32::cef_scanner::hide_if_visible();
    crate::win32::cef_shaders::hide_if_visible();
    crate::win32::cef_lueurs::hide_if_visible();
    crate::win32::cef_textures::hide_if_visible();
    crate::win32::cef_moniteur::hide_if_visible();
    crate::win32::cef_ssh::hide_if_visible();
    crate::win32::cef_terminal::hide_if_visible();
    crate::win32::cef_minuteur::hide_if_visible();
    crate::win32::cef_calculatrice::hide_if_visible();
    crate::win32::cef_recherche::hide_if_visible();
    crate::win32::bandeau::hide_if_visible();
    // La Météo est arrivée après cette liste — la nommer est ce qui la fait
    // céder devant nous.
    crate::win32::cef_meteo::hide_if_visible();
    crate::win32::cef_aide::hide_if_visible();
}

pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("automatisations")));
    // Une réouverture doit TOUJOURS repousser l'état : la page a pu être
    // rechargée, ou l'état avoir changé pendant qu'elle était fermée.
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, masquer_les_voisins);
    push_state();
    if cef_popup::is_visible(&SLOT) {
        cef_popup::execute_js(&SLOT, "window.automatisations && window.automatisations.montre()");
    }
}

/// Repousse l'état si la fenêtre est visible — à chaque frame (voir `app::run`) ;
/// `PAYLOAD` rend l'appel gratuit tant que rien ne change.
pub fn refresh_if_visible() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

fn push_state() {
    let mut etat = crate::win32::automatisations::json();
    etat["contentScale"] = json!(CONTENT_SCALE.with(Cell::get));
    let payload = etat.to_string();
    if PAYLOAD.with(|p| p.borrow().as_deref() == Some(payload.as_str())) {
        return;
    }
    // Le JSON passe par `serde_json` : un nom d'automatisation ou un message
    // porte des guillemets et des antislashs, qu'un littéral écrit à la main
    // perdrait.
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
    PAYLOAD.with(|p| *p.borrow_mut() = Some(payload));
}

pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("automatisations/") else {
            crate::diag!("[automatisations] action inconnue : {action:?}");
            continue;
        };
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["ready"] => PAYLOAD.with(|c| *c.borrow_mut() = None),
            ["close"] => cef_popup::fermer_par_geste(&SLOT),
            // La liste est arrivée par `cef_data`, AVANT cette action (voir la
            // doc de module) : le moteur la relit, la valide, l'écrit, et la
            // frame suivante la repousse à la page.
            ["editer"] => crate::win32::automatisations::remplacer_depuis(ROUTE),
            // Le nom peut porter des « / » : les morceaux se recollent ici.
            ["tester", morceaux @ ..] if !morceaux.is_empty() => {
                if let Err(message) = crate::win32::automatisations::tester(&morceaux.join("/")) {
                    crate::diag!("[automatisations] {message}");
                }
            }
            _ => crate::diag!("[automatisations] action inconnue : {action:?}"),
        }
    }
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

pub fn render_if_visible(rect: [i32; 4], tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::resize_if_needed(&SLOT, rect);
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

/// Voir `win32::cef_calendar_applet::debug_dump_backbuffer` — le harnais a
/// DEUX listes à tenir (`NEURADESK_DEBUG_OPEN=automatisations`).
pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
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

    const PAGE: &str = include_str!("../../assets/popups/automatisations.html");

    /// Les verbes que `dispatch_actions` accepte — même lecture que
    /// `cef_recherche::tests`.
    fn verbes_du_dispatch() -> Vec<String> {
        let source = include_str!("cef_automatisations.rs");
        source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(source)
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//"))
            .filter_map(|l| {
                let reste = l.strip_prefix("[\"")?;
                let (verbe, fin) = reste.split_once('"')?;
                matches!(fin.chars().next(), Some(']') | Some(',')).then(|| verbe.to_string())
            })
            .collect()
    }

    fn verbes_de_la_page() -> Vec<String> {
        PAGE.match_indices("go(")
            .filter_map(|(i, _)| {
                let reste = &PAGE[i + 3..];
                let guillemet = reste.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                reste[1..].split(guillemet).next().map(str::to_string)
            })
            .collect()
    }

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

    /// Les fonctions que Rust appelle dans la page doivent y exister — sinon
    /// l'état se perdrait EN SILENCE.
    #[test]
    fn la_page_definit_ce_que_rust_appelle() {
        for fonction in ["montre(", "window.setState"] {
            assert!(PAGE.contains(fonction), "{fonction} absente de automatisations.html");
        }
    }

    /// ⛔ **LE CORPS PART PAR `cef_data`, ET LA ROUTE DOIT ÊTRE LA MÊME DES DEUX
    /// CÔTÉS.** La page écrit la sienne en clair dans son JS ; celle-ci est
    /// employée par `remplacer_depuis`. Deux chaînes qui divergent ne produisent
    /// aucune erreur — juste une liste qui ne s'enregistre jamais.
    #[test]
    fn la_route_du_corps_est_la_meme_dans_la_page_et_dans_rust() {
        assert!(PAGE.contains(ROUTE), "la page n'écrit pas sur {ROUTE}");
    }
}
