//! Le PANNEAU de la Calculatrice (voir `win32::calculatrice`, qui porte
//! l'évaluateur et l'historique, et `assets/popups/calculatrice.html`).
//!
//! Un PANNEAU, pas un menu (voir la mémoire « panneau ou menu ») : il remplit
//! la zone libre du dock comme le Minuteur, dont il reprend la structure, et il
//! a le droit d'émettre une action au clic.
//!
//! **LA PAGE N'ÉVALUE RIEN** : elle envoie l'expression telle qu'écrite, Rust
//! rend le résultat dans `dernier` (voir `calculatrice::Dernier`), que la page
//! reconnaît à son numéro.
//!
//! Actions reçues (`"calculatrice/<verbe>/…"`, segments ENCODÉS) :
//! - `ready` — la page a chargé (voir `PAYLOAD`)
//! - `close`
//! - `calculer/<expression>`
//! - `angle/<deg|rad>`
//! - `historique-supprimer/<index>`, `historique-vider`

use std::cell::{Cell, RefCell};

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::calculatrice::{self, Angle, Calculatrice};
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// La clé du dernier `window.setState(...)` émis — voir
    /// `cef_minuteur::PAYLOAD` : vidée à l'ouverture ET au `ready`.
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefCalculatriceWindow"),
    window_title: w!("NeuraDesk — Calculatrice"),
    html_file: "calculatrice.html",
    wndproc: Some(wndproc),
    // PANNEAU — même famille que le calendrier.
    above_a_magnified_dock: false,
    fades: false,
    fenetre: false,
};

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

fn masquer_les_voisins() {
    // Même « zone libre » que le Dash — même liste que `cef_minuteur`, plus
    // le Minuteur lui-même.
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
    crate::win32::cef_recherche::hide_if_visible();
    crate::win32::cef_automatisations::hide_if_visible();
    crate::win32::cef_meteo::hide_if_visible();
    crate::win32::cef_aide::hide_if_visible();
    crate::win32::bandeau::hide_if_visible();
}

pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    // L'échelle d'écran × le réglage « Taille du contenu » du panneau.
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("calculatrice")));
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, masquer_les_voisins);
    push_state();
}

/// Repousse l'état si la fenêtre est visible — à chaque frame (voir
/// `app::run`) ; `PAYLOAD` rend l'appel gratuit tant que rien ne change.
pub fn refresh_if_visible() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

fn etat_json(c: &Calculatrice) -> serde_json::Value {
    let historique: Vec<serde_json::Value> =
        c.historique.iter().map(|e| json!({ "expression": e.expression, "resultat": e.resultat })).collect();
    let dernier = c.dernier.as_ref().map(|d| {
        let (resultat, erreur) = match &d.reponse {
            Ok(r) => (Some(r.as_str()), None),
            Err(e) => (None, Some(e.as_str())),
        };
        json!({ "numero": d.numero, "expression": d.expression, "resultat": resultat, "erreur": erreur })
    });
    json!({
        "historique": historique,
        "angle": c.angle.nom(),
        "dernier": dernier,
        "contentScale": CONTENT_SCALE.with(Cell::get),
    })
}

fn push_state() {
    let cle = calculatrice::lire(|c| format!("{c:?}|{}", CONTENT_SCALE.with(Cell::get)));
    if PAYLOAD.with(|p| p.borrow().as_deref() == Some(cle.as_str())) {
        return;
    }
    let payload = calculatrice::lire(etat_json);
    PAYLOAD.with(|p| *p.borrow_mut() = Some(cle));
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
}

pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("calculatrice/") else {
            crate::diag!("[calculatrice] action inconnue : {action:?}");
            continue;
        };
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["ready"] => PAYLOAD.with(|c| *c.borrow_mut() = None),
            ["close"] => cef_popup::fermer_par_geste(&SLOT),
            // La page encode le « / » d'une division ; s'il arrivait décodé en
            // route, les morceaux se recollent ici plutôt que de perdre le calcul.
            ["calculer", morceaux @ ..] if !morceaux.is_empty() => {
                let expression = morceaux.join("/");
                calculatrice::modifier(|c| c.calculer(&expression));
            }
            ["angle", nom] => match Angle::depuis_nom(nom) {
                Some(angle) => calculatrice::modifier(|c| c.angle = angle),
                None => crate::diag!("[calculatrice] unité d'angle inconnue : {nom:?}"),
            },
            ["historique-supprimer", index] => match index.parse::<usize>() {
                Ok(index) => calculatrice::modifier(|c| c.supprimer(index)),
                Err(_) => crate::diag!("[calculatrice] index illisible : {index:?}"),
            },
            ["historique-vider"] => calculatrice::modifier(Calculatrice::vider),
            _ => crate::diag!("[calculatrice] action inconnue : {action:?}"),
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
/// DEUX listes à tenir (`NEURADESK_DEBUG_OPEN=calculatrice`).
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
    /// Les verbes que `dispatch_actions` accepte — même lecture que
    /// `cef_minuteur::tests::verbes_du_dispatch`.
    fn verbes_du_dispatch() -> Vec<String> {
        let source = include_str!("cef_calculatrice.rs");
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
        const SOURCE: &str = include_str!("../../assets/popups/calculatrice.html");
        SOURCE
            .match_indices("go(")
            .filter_map(|(i, _)| {
                let reste = &SOURCE[i + 3..];
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
}
