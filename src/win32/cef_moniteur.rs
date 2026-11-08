//! Le PANNEAU du moniteur système — les courbes et les valeurs de l'instant
//! (voir `win32::moniteur_systeme`, qui porte les mesures, le fil et
//! l'historique, et `assets/popups/moniteur.html`).
//!
//! Un PANNEAU, pas un menu (voir la mémoire « panneau ou menu ») : même famille
//! que la Surveillance, il remplit la zone libre du dock.
//!
//! **LA PAGE NE REÇOIT QUE DES POINTS** : sept jours de minutes pèsent ~10 000
//! enregistrements ; Rust les range dans les points de la plage choisie (voir
//! `moniteur_systeme::regrouper`) et n'envoie que ceux-ci.
//!
//! Actions reçues (`"moniteur/<verbe>/…"`) :
//! - `ready` — la page a chargé (voir `PAYLOAD`)
//! - `close`
//! - `plage/<5min|1h|24h|7j>`
//! - `couleur/<série>/<#rrggbb>` — la couleur d'une courbe ; vide : celle par défaut

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::moniteur_systeme::{self, Moniteur, Plage, Serie};
use crate::win32::moniteur_taches;

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// La clé du dernier `window.setState(...)` émis — voir `push_state`, et
    /// `cef_surveillance::PAYLOAD`, même raison de la vider au `ready`.
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Les couleurs choisies des courbes, par série — lues au premier usage
    /// (voir `config::load_moniteur_couleurs`).
    static COULEURS: RefCell<Option<BTreeMap<String, String>>> = const { RefCell::new(None) };
}

fn couleurs<R>(f: impl FnOnce(&mut BTreeMap<String, String>) -> R) -> R {
    COULEURS.with(|c| f(c.borrow_mut().get_or_insert_with(crate::config::load_moniteur_couleurs)))
}

/// `couleur/<série>/<#rrggbb>` : retient la couleur, ou l'oublie si elle est vide.
fn choisir_couleur(serie: &str, couleur: &str) {
    if !crate::config::serie_de_moniteur_valide(serie) || !(couleur.is_empty() || crate::config::couleur_hex_valide(couleur)) {
        crate::diag!("[moniteur] couleur refusée : {serie:?} {couleur:?}");
        return;
    }
    couleurs(|c| {
        if couleur.is_empty() {
            c.remove(serie);
        } else {
            c.insert(serie.to_string(), couleur.to_ascii_lowercase());
        }
        crate::config::save_moniteur_couleurs(c);
    });
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    push_state();
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefMoniteurWindow"),
    window_title: w!("NeuraDesk — Moniteur système"),
    html_file: "moniteur.html",
    wndproc: Some(wndproc),
    // PANNEAU — même famille que la Surveillance.
    above_a_magnified_dock: false,
    fades: false,
    fenetre: false,
};

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

fn masquer_les_voisins() {
    // Même « zone libre » que le Dash — la liste de `cef_surveillance`, plus
    // la Surveillance elle-même.
    crate::win32::cef_dash::hide_if_visible();
    crate::win32::cef_settings::hide_if_visible();
    crate::win32::cef_notes::hide_if_visible();
    crate::win32::cef_image_editor::hide_if_visible();
    crate::win32::cef_calendar_applet::hide_if_visible();
    crate::win32::cef_clipboard::hide_if_visible();
    crate::win32::cef_minuteur::hide_if_visible();
    crate::win32::cef_calculatrice::hide_if_visible();
    crate::win32::cef_recherche::hide_if_visible();
    crate::win32::cef_scanner::hide_if_visible();
    crate::win32::cef_shaders::hide_if_visible();
    crate::win32::cef_lueurs::hide_if_visible();
    crate::win32::cef_textures::hide_if_visible();
    crate::win32::cef_automatisations::hide_if_visible();
    crate::win32::cef_meteo::hide_if_visible();
    crate::win32::cef_aide::hide_if_visible();
    crate::win32::bandeau::hide_if_visible();
    crate::win32::cef_ssh::hide_if_visible();
    crate::win32::cef_terminal::hide_if_visible();
    crate::win32::cef_surveillance::hide_if_visible();
}

pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    // L'échelle d'écran × le réglage « Taille du contenu » du panneau.
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("moniteur")));
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, masquer_les_voisins);
    push_state();
}

/// Recueille les mesures — à chaque frame (voir `app::run`), MÊME panneau
/// fermé : un canal qu'on ne vide pas garde tout en mémoire — et repousse
/// l'état si la fenêtre est visible ; `PAYLOAD` rend l'appel gratuit tant que
/// rien ne change.
pub fn refresh_if_visible() {
    moniteur_systeme::avancer();
    if cef_popup::is_visible(&SLOT) {
        moniteur_taches::demander(moniteur_systeme::maintenant_s());
        push_state();
    }
}

/// Une valeur pour la page : arrondie (des pourcentages au dixième, des octets
/// à l'unité), `null` sans mesure.
///
/// ⛔ **L'ARRONDI SE FAIT EN `f64`** : `12.3_f32` n'existe pas exactement, et
/// sérialisé il s'écrit `12.300000190734863` (vu par le test).
fn arrondi(serie: Serie, v: f32) -> serde_json::Value {
    if !v.is_finite() {
        return serde_json::Value::Null;
    }
    let v = f64::from(v);
    match serie {
        Serie::Cpu | Serie::Ram | Serie::Disque | Serie::Gpu => json!((v * 10.0).round() / 10.0),
        _ => json!(v.round() as u64),
    }
}

/// `{ <nom>: { actuel, moy, max } }` pour `series`, d'après les points d'une source.
fn series_json(series: &[Serie], plage: Plage, points: &[moniteur_systeme::Point], actuel: &moniteur_systeme::Valeurs) -> serde_json::Value {
    let series: serde_json::Map<String, serde_json::Value> = series
        .iter()
        .map(|&serie| {
            let s = serie as usize;
            let moyennes: Vec<_> = points.iter().map(|p| arrondi(serie, p.moyennes[s])).collect();
            // Le maximum n'apprend rien quand un point est une seule seconde.
            let max: Option<Vec<_>> = plage.lit_les_minutes().then(|| points.iter().map(|p| arrondi(serie, p.max[s])).collect());
            (serie.nom().to_string(), json!({ "actuel": arrondi(serie, actuel[s]), "moy": moyennes, "max": max }))
        })
        .collect();
    serde_json::Value::Object(series)
}

/// L'état envoyé à la page. `fin_s` est la fin (exclue) de la plage.
fn etat_json(m: &Moniteur, fin_s: u32) -> serde_json::Value {
    let plage = m.plage();
    let (duree_s, nb) = plage.decoupage();
    let appareils: Vec<_> = m
        .appareils_presents(fin_s.saturating_sub(1))
        .into_iter()
        .map(|a| {
            json!({
                "cle": a.info.cle,
                "sorte": a.info.sorte.nom(),
                "nom": a.info.nom,
                "precision": a.info.precision,
                "memoireTotale": a.info.memoire_totale,
                "series": series_json(a.info.sorte.series(), plage, &a.historique.points(plage, fin_s), &a.historique.actuel()),
            })
        })
        .collect();
    json!({
        "plage": plage.nom(),
        "debutS": fin_s.saturating_sub(duree_s),
        "pasS": duree_s / nb as u32,
        "series": series_json(&Serie::TOUTES, plage, &m.points(fin_s), &m.actuel()),
        "appareils": appareils,
        "coeurs": m.detail.coeurs.iter().map(|&c| arrondi(Serie::Cpu, c)).collect::<Vec<_>>(),
        "ramUtilisee": m.detail.ram_utilisee,
        "ramTotale": m.detail.ram_totale,
        "taches": m.detail.taches.iter().map(|t| json!({ "nom": t.nom, "instances": t.instances, "cpu": arrondi(Serie::Cpu, t.cpu) })).collect::<Vec<_>>(),
        "contentScale": CONTENT_SCALE.with(Cell::get),
        "couleurs": couleurs(|c| c.clone()),
    })
}

fn push_state() {
    let fin_s = moniteur_systeme::maintenant_s() + 1;
    let cle = moniteur_systeme::lire(|m| format!("{}|{}|{}", m.version, m.plage().nom(), CONTENT_SCALE.with(Cell::get)));
    if PAYLOAD.with(|c| c.borrow().as_deref() == Some(cle.as_str())) {
        return;
    }
    let payload = moniteur_systeme::lire(|m| etat_json(m, fin_s));
    PAYLOAD.with(|c| *c.borrow_mut() = Some(cle));
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
}

pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("moniteur/") else {
            crate::diag!("[moniteur] action inconnue : {action:?}");
            continue;
        };
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["ready"] => PAYLOAD.with(|c| *c.borrow_mut() = None),
            ["close"] => cef_popup::fermer_par_geste(&SLOT),
            ["plage", nom] => match Plage::depuis_nom(nom) {
                Some(plage) => moniteur_systeme::choisir_plage(plage),
                None => crate::diag!("[moniteur] plage inconnue : {nom:?}"),
            },
            ["couleur", serie, couleur] => choisir_couleur(serie, couleur),
            _ => crate::diag!("[moniteur] action inconnue : {action:?}"),
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
/// DEUX listes à tenir (`NEURADESK_DEBUG_OPEN=moniteur`).
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

    const PAGE: &str = include_str!("../../assets/popups/moniteur.html");

    /// Les verbes que `dispatch_actions` accepte — voir
    /// `cef_surveillance::tests::verbes_du_dispatch`, même lecture.
    fn verbes_du_dispatch() -> Vec<String> {
        let source = include_str!("cef_moniteur.rs");
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

    #[test]
    fn la_page_propose_les_plages_de_rust() {
        for plage in Plage::TOUTES {
            assert!(PAGE.contains(&format!("data-plage=\"{}\"", plage.nom())), "{}", plage.nom());
        }
    }

    /// Chaque série que Rust envoie a sa carte dans la page — celles d'un disque
    /// ou d'une carte graphique dans la carte de CHAQUE appareil (depuis le
    /// 2026-10-20 : elles y remplacent les totaux).
    #[test]
    fn la_page_dessine_chaque_serie() {
        use crate::win32::moniteur_peripheriques::Sorte;
        for serie in Serie::TOUTES {
            let nom = serie.nom();
            let d_appareil = [Sorte::Disque, Sorte::Gpu].iter().any(|s| s.series().contains(&serie));
            let lue = PAGE.contains(&format!("\"{nom}\"")) || (d_appareil && PAGE.contains(&format!("a.series.{nom}")));
            assert!(lue, "{nom}");
        }
    }

    #[test]
    fn l_etat_porte_un_point_par_tranche_et_null_sans_mesure() {
        let m = Moniteur::default();
        let etat = etat_json(&m, 100_000);
        assert_eq!(etat["plage"], "5min");
        assert_eq!(etat["pasS"], 1);
        assert_eq!(etat["series"]["cpu"]["moy"].as_array().map(Vec::len), Some(300));
        assert_eq!(etat["series"]["cpu"]["actuel"], serde_json::Value::Null, "rien de mesuré : null, pas zéro");
        assert_eq!(etat["series"]["cpu"]["max"], serde_json::Value::Null, "une seconde n'a pas de maximum à montrer");
        assert_eq!(etat["debutS"], 100_000 - 300);
    }

    #[test]
    fn une_couleur_de_courbe_n_accepte_que_du_hex() {
        assert!(crate::config::couleur_hex_valide("#A1b2C3"));
        for mauvaise in ["", "#abc", "a1b2c3d", "#12345g", "red;x", "#1234567"] {
            assert!(!crate::config::couleur_hex_valide(mauvaise), "{mauvaise:?}");
        }
        assert!(crate::config::serie_de_moniteur_valide("memoirePct"));
        assert!(!crate::config::serie_de_moniteur_valide("../x"));
    }

    /// Chaque série que la page colore est un nom que Rust accepte.
    #[test]
    fn la_page_colore_des_series_que_rust_accepte() {
        for nom in ["cpu", "ram", "disque", "reception", "envoi", "gpu", "memoirePct"] {
            assert!(PAGE.contains(&format!("\"{nom}\"")), "{nom}");
            assert!(crate::config::serie_de_moniteur_valide(nom), "{nom}");
        }
    }

    #[test]
    fn un_pourcentage_garde_une_decimale_et_un_debit_aucune() {
        assert_eq!(arrondi(Serie::Cpu, 12.345), json!(12.3));
        assert_eq!(arrondi(Serie::Envoi, 1234.6), json!(1235));
        assert_eq!(arrondi(Serie::Gpu, f32::NAN), serde_json::Value::Null);
    }

    #[test]
    fn un_appareil_n_envoie_que_les_series_de_sa_sorte() {
        let disque = moniteur_systeme::Serie::Disque;
        let series = series_json(crate::win32::moniteur_peripheriques::Sorte::Disque.series(), Plage::CinqMinutes, &[], &moniteur_systeme::VIDE);
        let noms: Vec<_> = series.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        assert_eq!(noms, ["disque", "ecriture", "lecture"], "{noms:?}");
        assert_eq!(series[disque.nom()]["actuel"], serde_json::Value::Null);
        for champ in ["state.appareils", "a.memoireTotale", "a.precision", "a.series", "gpuMemoire", "memoirePct"] {
            assert!(PAGE.contains(champ), "la page ne lit pas `{champ}`");
        }
    }

    #[test]
    fn les_taches_arrivent_a_la_page_avec_les_champs_qu_elle_lit() {
        let mut m = Moniteur::default();
        m.detail.taches = vec![moniteur_taches::Tache { nom: "brave".into(), instances: 17, cpu: 2.14 }];
        let etat = etat_json(&m, 100_000);
        assert_eq!(etat["taches"], json!([{ "nom": "brave", "instances": 17, "cpu": 2.1 }]));
        for champ in ["state.taches", "t.nom", "t.instances", "t.cpu"] {
            assert!(PAGE.contains(champ), "la page ne lit pas `{champ}`");
        }
    }

    /// ⛔ **LES VOISINS, DANS LES DEUX SENS** — voir `cef_textures`, même test.
    #[test]
    fn chaque_voisin_nous_masque_en_retour() {
        let voisins: [(&str, &str); 20] = [
            ("cef_dash", include_str!("cef_dash.rs")),
            ("cef_settings", include_str!("cef_settings.rs")),
            ("cef_notes", include_str!("cef_notes.rs")),
            ("cef_image_editor", include_str!("cef_image_editor.rs")),
            ("cef_calendar_applet", include_str!("cef_calendar_applet.rs")),
            ("cef_clipboard", include_str!("cef_clipboard.rs")),
            ("cef_surveillance", include_str!("cef_surveillance.rs")),
            ("cef_scanner", include_str!("cef_scanner.rs")),
            ("cef_shaders", include_str!("cef_shaders.rs")),
            ("cef_lueurs", include_str!("cef_lueurs.rs")),
            ("cef_textures", include_str!("cef_textures.rs")),
            ("cef_ssh", include_str!("cef_ssh.rs")),
            ("cef_terminal", include_str!("cef_terminal.rs")),
            ("cef_minuteur", include_str!("cef_minuteur.rs")),
            ("cef_calculatrice", include_str!("cef_calculatrice.rs")),
            ("cef_recherche", include_str!("cef_recherche.rs")),
            ("cef_automatisations", include_str!("cef_automatisations.rs")),
            ("cef_meteo", include_str!("cef_meteo.rs")),
            ("cef_aide", include_str!("cef_aide.rs")),
            ("bandeau", include_str!("bandeau.rs")),
        ];
        let nous = include_str!("cef_moniteur.rs");
        for (nom, source) in voisins {
            assert!(nous.contains(&format!("crate::win32::{nom}::hide_if_visible();")), "nous ne masquons pas {nom}");
            assert!(source.contains("crate::win32::cef_moniteur::hide_if_visible()"), "{nom} ne nous masque pas");
        }
    }
}
