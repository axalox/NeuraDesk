//! Le PANNEAU DE LA MÉTÉO — la page `assets/popups/meteo.html`.
//!
//! Il ne sait rien du réseau : il lit `win32::meteo` et pousse son état. Toute la
//! conversation avec Open-Meteo vit là-bas, sur un fil dédié — même partage que
//! `cef_surveillance` avec `surveillance`.
//!
//! ## Le contrat avec la page
//!
//! - `window.setState({ contentScale, lieu, chargement, erreur, candidats, bulletin })`
//!   — poussé à chaque frame où quelque chose a changé (voir `push_state`).
//! - La page émet `neuradesk://meteo/<verbe>[/…]` :
//!   `ready` · `close` · `chercher/<nom>` · `choisir/<rang>` · `rafraichir`.
//!
//! ⚠️ **`choisir` PORTE UN RANG, PAS DES COORDONNÉES.** La page a déjà la liste
//! des villes dans son état ; renvoyer latitude et longitude par l'URL les ferait
//! voyager en texte pour rien, et surtout reposerait à la page un choix que Rust a
//! déjà fait. Elle dit « la deuxième », Rust retrouve la deuxième.
//!
//! ⚠️ Les verbes sont tenus par un test de parité page ↔ `dispatch_actions` —
//! l'ajouter d'un seul côté échoue, au lieu de rendre le bouton muet.

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
    /// Le dernier état poussé — la même garde que partout ailleurs : sans elle,
    /// `setState` repartirait à chaque frame et la page se reconstruirait sous les
    /// doigts de l'utilisateur (voir le champ de ville dans `meteo.html`).
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Les versions de ce que `PAYLOAD` a poussé — voir `push_state`.
    static CLE: Cell<Option<(u64, u64, u32)>> = const { Cell::new(None) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefMeteoWindow"),
    window_title: w!("NeuraDesk — Météo"),
    html_file: "meteo.html",
    wndproc: Some(wndproc),
    // PANNEAU — même famille que la calculatrice et la recherche.
    above_a_magnified_dock: false,
    fades: false,
    fenetre: false,
};

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// **LES PANNEAUX QUI PARTAGENT LA ZONE LIBRE.**
///
/// ⚠️ **TREIZE, PAS ONZE.** La liste de `cef_recherche` — dont celle-ci s'inspire —
/// est elle-même EN RETARD d'une applet : `cef_automatisations` et `bandeau` la
/// masquent sans être masqués par elle. Les recopier telles quelles aurait laissé
/// deux panneaux se superposer dans la zone libre.
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
    crate::win32::cef_automatisations::hide_if_visible();
    crate::win32::bandeau::hide_if_visible();
}

pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("meteo")));
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, masquer_les_voisins);
    push_state();
    // ⚠️ **ON NE PREND PAS LE FOCUS.** Le panneau s'ouvre pour LIRE ; `recherche`
    // vole le focus à l'ouverture parce que son propos EST la saisie, ici ce n'est
    // pas le cas. Le champ de ville prend le focus quand on clique dedans.
}

/// Repousse l'état si la fenêtre est visible — à chaque frame (voir `app::run`) ;
/// `PAYLOAD` rend l'appel gratuit tant que rien ne change.
pub fn refresh_if_visible() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

fn push_state() {
    // `meteo::json()` rend un OBJET : on y ajoute l'échelle du texte plutôt que
    // d'imbriquer un second niveau, pour que la page lise `next.bulletin` comme
    // `calendar.html` lit `next.mois`.
    // ⚠️ **RIEN N'A CHANGÉ, RIEN NE SE SÉRIALISE** : `refresh_if_visible` appelle
    // ceci à chaque frame, et la grille de la carte fait ~100 Ko de nombres. Les
    // deux modules incrémentent une version à chaque nouvelle ; `PAYLOAD` vidé
    // (ouverture, page rechargée) force la poussée.
    let cle = (
        crate::win32::meteo::lire(|m| m.version),
        crate::win32::meteo_carte::version(),
        CONTENT_SCALE.with(Cell::get).to_bits(),
    );
    if CLE.with(Cell::get) == Some(cle) && PAYLOAD.with(|p| p.borrow().is_some()) {
        return;
    }
    CLE.with(|c| c.set(Some(cle)));
    let mut valeur = crate::win32::meteo::json();
    if let Some(objet) = valeur.as_object_mut() {
        objet.insert("contentScale".to_string(), json!(CONTENT_SCALE.with(Cell::get)));
        // La carte (voir `win32::meteo_carte`) — la grille pèse ~100 Ko de nombres,
        // d'où la clé de `push_state` qui évite de la sérialiser à chaque frame.
        objet.insert("carte".to_string(), crate::win32::meteo_carte::json());
    }
    let payload = valeur.to_string();
    if PAYLOAD.with(|p| p.borrow().as_deref() == Some(payload.as_str())) {
        return;
    }
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
    PAYLOAD.with(|p| *p.borrow_mut() = Some(payload));
}

pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("meteo/") else {
            crate::diag!("[meteo] action inconnue : {action:?}");
            continue;
        };
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            // La page vient de (re)charger : son cache d'état est vide, il faut
            // lui repousser le nôtre.
            ["ready"] => PAYLOAD.with(|c| *c.borrow_mut() = None),
            ["close"] => cef_popup::fermer_par_geste(&SLOT),
            // Un nom de ville peut contenir une barre après décodage : les
            // morceaux se recollent, comme pour les chemins de `recherche`.
            ["chercher", morceaux @ ..] if !morceaux.is_empty() => {
                crate::win32::meteo::chercher_ville(&morceaux.join("/"));
            }
            ["choisir", rang] => match rang.parse::<usize>() {
                // ⚠️ **LE RANG VIENT DE LA PAGE, DONC IL PEUT ÊTRE HORS BORNES** —
                // un état plus vieux que la liste affichée, et `candidats[9]` ferait
                // tomber le fil d'interface. On relit la liste au moment du clic.
                Ok(rang) => match crate::win32::meteo::lire(|m| m.candidats.get(rang).cloned()) {
                    Some(candidat) => crate::win32::meteo::choisir_ville(candidat),
                    None => crate::diag!("[meteo] ville {rang} hors de la liste"),
                },
                Err(_) => crate::diag!("[meteo] rang de ville illisible : {rang:?}"),
            },
            ["rafraichir"] => crate::win32::meteo::rafraichir(),
            // Un onglet de carte s'ouvre : la carte du LIEU AFFICHÉ, chargée ou
            // complétée (rien n'est téléchargé avant ce premier geste).
            ["carte"] => match crate::win32::meteo::lire(|m| m.lieu.as_ref().map(|l| (l.latitude, l.longitude))) {
                Some((lat, lon)) => crate::win32::meteo_carte::demander(lat, lon),
                None => crate::diag!("[meteo] carte demandée sans lieu"),
            },
            // **LES FAVORIS — c'est ici que se joue le « en un clic ».** Bascule sur
            // le lieu `rang` de la rangée, sans repasser par une recherche.
            ["favori", rang] => match rang.parse::<usize>() {
                Ok(rang) => crate::win32::meteo::choisir_favori(rang),
                Err(_) => crate::diag!("[meteo] rang de favori illisible : {rang:?}"),
            },
            // Retient le lieu COURANT — le bouton n'est montré que quand il manque.
            ["ajouter"] => crate::win32::meteo::ajouter_favori(),
            ["retirer", rang] => match rang.parse::<usize>() {
                Ok(rang) => crate::win32::meteo::retirer_favori(rang),
                Err(_) => crate::diag!("[meteo] rang de favori illisible : {rang:?}"),
            },
            _ => crate::diag!("[meteo] action inconnue : {action:?}"),
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

/// Voir `win32::cef_calendar_applet::debug_dump_backbuffer` — le harnais a DEUX
/// listes à tenir (`NEURADESK_DEBUG_OPEN=meteo`).
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
    const PAGE: &str = include_str!("../../assets/popups/meteo.html");

    /// Les verbes que `dispatch_actions` accepte — même lecture que
    /// `cef_recherche::tests`.
    fn verbes_du_dispatch() -> Vec<String> {
        let source = include_str!("cef_meteo.rs");
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

    /// Les fonctions que Rust appelle dans la page doivent y exister — sinon les
    /// prévisions se perdraient EN SILENCE.
    #[test]
    fn la_page_definit_ce_que_rust_appelle() {
        for fonction in ["window.setState", "function rendre"] {
            assert!(PAGE.contains(fonction), "{fonction} absente de meteo.html");
        }
    }

}
