//! Le PANNEAU de la Surveillance — les cibles, leur état et leur historique
//! (voir `win32::surveillance`, qui porte le modèle et le fil de mesure, et
//! `assets/popups/surveillance.html`).
//!
//! Un PANNEAU, pas un menu (voir la mémoire « panneau ou menu ») : même famille
//! que le calendrier et le minuteur, il remplit la zone libre du dock.
//!
//! **LA PAGE NE REÇOIT QUE DES BARRES** : un an de mesures d'une cible pèse
//! ~500 000 enregistrements ; Rust les range dans les barres de la plage choisie
//! (voir `surveillance::agreger`) et n'envoie que celles-ci.
//!
//! ⚠️ **LE PANNEAU NE S'OUVRE JAMAIS TOUT SEUL** — une panne fait clignoter
//! l'icône, rien de plus (DEMANDE EXPLICITE, même règle que le minuteur).
//!
//! Actions reçues (`"surveillance/<verbe>/…"`, segments ENCODÉS) :
//! - `ready` — la page a chargé (voir `PAYLOAD`)
//! - `close`
//! - `enregistrer/<id|nouvelle>/<adresse>/<0|1>[/<nom>]` — `1` = vérifier aussi en HTTP
//! - `supprimer/<id>`
//! - `plage/<1h|24h|7j|30j|1an>`
//! - `verifier` — tout mesurer maintenant
//! - `outils/<id>` — ouvre la section « Outils » de la cible (voir
//!   `win32::outils_reseau`) ; `outils-fermer` la ferme
//! - `outil/<id>/<traceroute|dns|ports|certificat|familles>[/<ports>]` — lance
//!   un outil ; la liste de ports n'accompagne que `ports`
//!
//! ⚠️ **UN SEGMENT FINAL VIDE EST MANGÉ** en route : le nom, facultatif, a donc
//! ses DEUX bras (avec et sans), et la page n'émet jamais de segment vide.

use std::cell::{Cell, RefCell};

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::outils_reseau::{self, Outil};
use crate::win32::surveillance::{self, Plage, Surveillance};

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// La clé du dernier `window.setState(...)` émis — voir `push_state` et
    /// `cef_minuteur::PAYLOAD`, même raison de la vider au `ready`.
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefSurveillanceWindow"),
    window_title: w!("NeuraDesk — Surveillance"),
    html_file: "surveillance.html",
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
    // le minuteur lui-même.
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
    crate::win32::cef_moniteur::hide_if_visible();
    crate::win32::cef_automatisations::hide_if_visible();
    crate::win32::cef_meteo::hide_if_visible();
    crate::win32::cef_aide::hide_if_visible();
    crate::win32::bandeau::hide_if_visible();
    crate::win32::cef_ssh::hide_if_visible();
    crate::win32::cef_terminal::hide_if_visible();
}

pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    // L'échelle d'écran × le réglage « Taille du contenu » du panneau.
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("surveillance")));
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, masquer_les_voisins);
    outils_reseau::lancer_depuis_env();
    push_state();
}

/// Repousse l'état si la fenêtre est visible — à chaque frame (voir
/// `app::run`) ; `PAYLOAD` rend l'appel gratuit tant que rien ne change.
pub fn refresh_if_visible() {
    // Les rapports des outils se recueillent MÊME panneau fermé : un canal
    // qu'on ne vide pas garde tout en mémoire.
    outils_reseau::avancer();
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

/// L'état envoyé à la page. `fin_s` est la fin de la plage, arrondie à la
/// minute SUIVANTE : les barres ne glissent qu'une fois par minute, et la
/// mesure de la minute en cours est déjà dans la dernière.
fn etat_json(s: &Surveillance, fin_s: u32) -> serde_json::Value {
    let plage = s.plage();
    let (duree_s, nb) = plage.decoupage();
    let cibles: Vec<serde_json::Value> = s
        .cibles
        .iter()
        .map(|c| {
            let mesures = s.historiques.get(&c.id).map(Vec::as_slice).unwrap_or(&[]);
            let seaux = surveillance::agreger(mesures, fin_s, duree_s, nb);
            let derniere = s.derniere(c.id).map(|m| {
                json!({
                    "instantS": m.instant_s,
                    "enLigne": m.en_ligne(),
                    "ping": m.ping_ok().then_some(m.ping_ms),
                    "httpMesure": m.http_mesure(),
                    "http": m.http_ok().then_some(m.http_ms),
                    "statut": m.statut,
                })
            });
            // Une barre = `[mesures, en ligne, latence moyenne|null, latence max]` :
            // des tableaux plutôt que des objets, 365 barres par cible.
            let barres: Vec<serde_json::Value> = seaux
                .iter()
                .map(|b| json!([b.mesures, b.en_ligne, b.latence_moyenne(), b.latence_max]))
                .collect();
            json!({
                "id": c.id, "nom": c.nom, "adresse": c.adresse, "http": c.http, "alerte": c.alerte,
                "derniere": derniere,
                "disponibilite": surveillance::disponibilite(&seaux),
                "barres": barres,
                "portsProposes": outils_reseau::ports_proposes(&c.adresse),
            })
        })
        .collect();
    json!({
        "cibles": cibles,
        "plage": plage.nom(),
        "debutS": fin_s.saturating_sub(duree_s),
        "pasS": duree_s / nb as u32,
        "erreur": s.erreur,
        "alertesCoupees": s.alertes_coupees,
        "outils": outils_reseau::lire(|r| r.map(|r| json!({
            "cibleId": r.cible_id,
            "outil": r.outil.map(Outil::nom),
            "enCours": r.en_cours,
            "lignes": r.lignes.iter().map(|l| json!([l.niveau.nom(), l.texte])).collect::<Vec<_>>(),
        }))),
        "portsMax": outils_reseau::PORTS_MAX,
        "max": surveillance::CIBLES_MAX,
        "contentScale": CONTENT_SCALE.with(Cell::get),
    })
}

fn push_state() {
    let fin_s = (surveillance::maintenant_s() / 60 + 1) * 60;
    let cle = surveillance::lire(|s| {
        format!("{}|{}|{}|{fin_s}|{}", s.version, outils_reseau::version(), s.plage().nom(), CONTENT_SCALE.with(Cell::get))
    });
    if PAYLOAD.with(|c| c.borrow().as_deref() == Some(cle.as_str())) {
        return;
    }
    let payload = surveillance::lire(|s| etat_json(s, fin_s));
    PAYLOAD.with(|c| *c.borrow_mut() = Some(cle));
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
}

/// `id` : `nouvelle`, ou l'identifiant d'une cible existante. `http` : `0` ou `1`.
fn enregistrer(id: &str, adresse: &str, http: &str, nom: &str) {
    let id = match id {
        "nouvelle" => None,
        autre => match autre.parse::<u64>() {
            Ok(id) => Some(id),
            Err(_) => return crate::diag!("[surveillance] identifiant illisible : {autre:?}"),
        },
    };
    let http = match http {
        "0" => false,
        "1" => true,
        autre => return crate::diag!("[surveillance] option HTTP illisible : {autre:?}"),
    };
    surveillance::enregistrer_cible(id, adresse, http, nom);
}

/// Seul `ports` accepte un argument : la liste saisie, validée par
/// `outils_reseau::analyser_ports`.
fn lancer_outil(id: &str, nom: &str, ports: Option<&str>) {
    let (Ok(id), Some(outil)) = (id.parse::<u64>(), Outil::depuis_nom(nom)) else {
        return crate::diag!("[surveillance] outil illisible : {id:?} {nom:?}");
    };
    if ports.is_some() && outil != Outil::Ports {
        return crate::diag!("[surveillance] argument inattendu pour {nom}");
    }
    outils_reseau::lancer(id, outil, ports);
}

pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("surveillance/") else {
            crate::diag!("[surveillance] action inconnue : {action:?}");
            continue;
        };
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["ready"] => PAYLOAD.with(|c| *c.borrow_mut() = None),
            ["close"] => cef_popup::fermer_par_geste(&SLOT),
            ["enregistrer", id, adresse, http] => enregistrer(id, adresse, http, ""),
            ["enregistrer", id, adresse, http, nom] => enregistrer(id, adresse, http, nom),
            ["supprimer", id] => match id.parse::<u64>() {
                Ok(id) => surveillance::supprimer_cible(id),
                Err(_) => crate::diag!("[surveillance] identifiant illisible : {id:?}"),
            },
            ["plage", nom] => match Plage::depuis_nom(nom) {
                Some(plage) => surveillance::choisir_plage(plage),
                None => crate::diag!("[surveillance] plage inconnue : {nom:?}"),
            },
            ["verifier"] => surveillance::tout_mesurer(),
            // Les ALERTES — la mesure continue dans les deux cas.
            ["alerte", id, flag] => match id.parse::<u64>() {
                Ok(id) => surveillance::regler_alerte(id, *flag == "1"),
                Err(_) => crate::diag!("[surveillance] alerte : identifiant illisible {id:?}"),
            },
            ["alertes", flag] => surveillance::couper_les_alertes(*flag == "0"),
            ["outils", id] => match id.parse::<u64>() {
                Ok(id) => outils_reseau::ouvrir(id),
                Err(_) => crate::diag!("[surveillance] identifiant illisible : {id:?}"),
            },
            ["outils-fermer"] => outils_reseau::fermer(),
            ["outil", id, nom] => lancer_outil(id, nom, None),
            ["outil", id, nom, ports] => lancer_outil(id, nom, Some(ports)),
            _ => crate::diag!("[surveillance] action inconnue : {action:?}"),
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
/// DEUX listes à tenir (`NEURADESK_DEBUG_OPEN=surveillance`).
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

    /// Les verbes que `dispatch_actions` accepte — voir
    /// `cef_minuteur::tests::verbes_du_dispatch`, même lecture.
    fn verbes_du_dispatch() -> Vec<String> {
        let source = include_str!("cef_surveillance.rs");
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
        const SOURCE: &str = include_str!("../../assets/popups/surveillance.html");
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

    /// Les plages que la page propose sont exactement celles de Rust.
    #[test]
    fn la_page_propose_les_plages_de_rust() {
        const SOURCE: &str = include_str!("../../assets/popups/surveillance.html");
        for plage in Plage::TOUTES {
            assert!(SOURCE.contains(&format!("data-plage=\"{}\"", plage.nom())), "{}", plage.nom());
        }
    }

    /// Les outils que la page propose sont exactement ceux de Rust.
    #[test]
    fn la_page_propose_les_outils_de_rust() {
        const SOURCE: &str = include_str!("../../assets/popups/surveillance.html");
        let page: Vec<&str> = SOURCE.match_indices("data-outil=\"").map(|(i, m)| SOURCE[i + m.len()..].split('"').next().unwrap_or("")).collect();
        let rust: Vec<&str> = Outil::TOUS.iter().map(|o| o.nom()).collect();
        assert_eq!(page, rust);
    }

    #[test]
    fn l_etat_porte_une_barre_par_tranche() {
        let mut s = Surveillance::default();
        s.cibles = vec![surveillance::Cible { id: 3, nom: "NAS".into(), adresse: "10.0.0.2".into(), http: false, alerte: true }];
        let etat = etat_json(&s, 100_000);
        assert_eq!(etat["plage"], "24h");
        assert_eq!(etat["cibles"][0]["barres"].as_array().map(Vec::len), Some(96));
        assert_eq!(etat["cibles"][0]["disponibilite"], serde_json::Value::Null);
        assert_eq!(etat["pasS"], 900);
        assert_eq!(etat["cibles"][0]["portsProposes"], crate::win32::outils_reseau::ports_proposes("10.0.0.2"));
        assert_eq!(etat["outils"], serde_json::Value::Null);
    }
}
