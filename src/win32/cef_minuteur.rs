//! Le PANNEAU du Minuteur — comptes à rebours et alarmes (voir
//! `win32::minuteur`, qui porte le modèle, et `assets/popups/minuteur.html`).
//!
//! Un PANNEAU, pas un menu (voir la mémoire « panneau ou menu ») : il remplit
//! la zone libre du dock comme le calendrier, dont il reprend la structure, et
//! il a le droit d'émettre une action au clic.
//!
//! **LA PAGE DÉCOMPTE ELLE-MÊME.** Rust ne pousse l'état que lorsqu'il CHANGE
//! (voir `PAYLOAD`) ; un compte en cours part avec son instant de fin et
//! l'instant de la poussée, et la page en déduit le reste à chaque seconde.
//! Pousser soixante fois par seconde pour faire bouger un chiffre par seconde
//! reconstruirait la liste sous les doigts de l'utilisateur.
//!
//! Actions reçues (`"minuteur/<verbe>/…"`, segments ENCODÉS) :
//! - `ready` — la page a chargé (voir `PAYLOAD`)
//! - `close`
//! - `compte-ajouter/<secondes>[/<nom>]`
//! - `compte-demarrer|compte-pause|compte-reinitialiser|compte-supprimer/<id>`
//! - `alarme-enregistrer/<id|nouvelle>/<HH:MM>/<jours>/<son|defaut>[/<nom>]`
//! - `alarme-basculer/<id>`, `alarme-supprimer/<id>`
//! - `son-comptes/<son|defaut>`, `ecouter/<son>`
//! - `chrono-demarrer|chrono-pause|chrono-tour|chrono-reinitialiser`
//! - `arreter`, `repeter`
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
use crate::win32::minuteur::{self, Minuteur};
use crate::win32::son;

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// La clé du dernier `window.setState(...)` émis — voir `push_state`.
    /// Vidée à l'ouverture ET au `ready` de la page : la fenêtre est
    /// PRÉCHARGÉE, et un `execute_js` émis avant que la page ait fini de
    /// charger se perd en silence.
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefMinuteurWindow"),
    window_title: w!("NeuraDesk — Minuteur"),
    html_file: "minuteur.html",
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
    // Même « zone libre » que le Dash — même liste que
    // `win32::cef_calendar_applet::toggle`, plus le calendrier lui-même.
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
    crate::win32::cef_calculatrice::hide_if_visible();
    crate::win32::cef_recherche::hide_if_visible();
    crate::win32::cef_automatisations::hide_if_visible();
    crate::win32::cef_meteo::hide_if_visible();
    crate::win32::cef_aide::hide_if_visible();
    crate::win32::bandeau::hide_if_visible();
    crate::win32::cef_ssh::hide_if_visible();
    crate::win32::cef_terminal::hide_if_visible();
}

pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    // L'échelle d'écran × le réglage « Taille du contenu » du panneau.
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("minuteur")));
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, masquer_les_voisins);
    push_state();
}

/// **OUVRE** le panneau s'il est fermé — au clic sur l'icône qui clignote à
/// droite de la cloche (voir `app::run`). La sonnerie seule ne l'ouvre PAS
/// (DEMANDE EXPLICITE, 2026-09-26).
pub fn ouvrir(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    if !cef_popup::is_visible(&SLOT) {
        toggle(rect, gfx, content_scale);
    }
}

/// Repousse l'état si la fenêtre est visible — à chaque frame (voir
/// `app::run`) ; `PAYLOAD` rend l'appel gratuit tant que rien ne change.
pub fn refresh_if_visible() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

fn etat_json(m: &Minuteur, maintenant_ms: i64, sons: &[String]) -> serde_json::Value {
    let comptes: Vec<serde_json::Value> = m
        .comptes
        .iter()
        .map(|c| {
            json!({
                "id": c.id, "nom": c.nom, "dureeMs": c.duree_ms, "resteMs": c.reste_ms,
                "finMs": c.fin_ms, "sonne": c.sonne, "ephemere": c.ephemere,
            })
        })
        .collect();
    let alarmes: Vec<serde_json::Value> = m
        .alarmes
        .iter()
        .map(|a| {
            json!({
                "id": a.id, "nom": a.nom, "heure": format!("{:02}:{:02}", a.heure.0, a.heure.1),
                "jours": a.jours, "active": a.active, "son": a.son, "sonne": a.sonne,
            })
        })
        .collect();
    json!({
        "maintenantMs": maintenant_ms,
        "comptes": comptes,
        "alarmes": alarmes,
        "sonComptes": m.son_comptes,
        "sons": sons,
        "quiSonne": m.noms_qui_sonnent(),
        // ⭐ LE POMODORO (voir `minuteur::Pomodoro`) — la page le décompte comme
        // un compte : `finMs` quand il tourne, `resteMs` sinon.
        "pomodoro": {
            "phase": m.pomodoro.phase.cle(),
            "nom": m.pomodoro.phase.nom(),
            "seances": m.pomodoro.seances,
            "seancesAvantLongue": minuteur::SEANCES_AVANT_LONGUE,
            "resteMs": m.pomodoro.reste_ms,
            "finMs": m.pomodoro.fin_ms,
            "dureeMs": m.pomodoro.duree_ms(m.pomodoro.phase),
            "sonne": m.pomodoro.sonne,
            "travailMin": m.pomodoro.travail_min,
            "pauseMin": m.pomodoro.pause_min,
            "longueMin": m.pomodoro.longue_min,
            "minMinutes": minuteur::POMODORO_MIN_MINUTES,
            "maxMinutes": minuteur::POMODORO_MAX_MINUTES,
        },
        // ⭐ LE CHRONOMÈTRE (voir `minuteur::Chronometre`) — la page le compte
        // VERS LE HAUT : `origineMs` (= `marche`) quand il tourne, `accumuleMs`
        // sinon. `marche` étant un instant FIXE, la clé de `push_state` ne change
        // pas pendant qu'il tourne : la page anime son affichage elle-même.
        "chrono": {
            "origineMs": m.chrono.marche,
            "accumuleMs": m.chrono.accumule_ms,
            "enMarche": m.chrono.en_marche(),
            "tours": m.chrono.tours,
        },
        "contentScale": CONTENT_SCALE.with(Cell::get),
    })
}

fn push_state() {
    let sons: Vec<String> = son::fichiers().iter().map(|p| son::nom(p)).collect();
    // ⚠️ **L'INSTANT N'ENTRE PAS DANS LA CLÉ** — sinon elle changerait à chaque
    // frame et la page serait reconstruite soixante fois par seconde. La page
    // n'a besoin de l'instant que pour recaler son horloge sur celle de Rust,
    // et celui de la poussée suffit.
    let cle = minuteur::lire(|m| format!("{m:?}|{sons:?}|{}", CONTENT_SCALE.with(Cell::get)));
    if PAYLOAD.with(|c| c.borrow().as_deref() == Some(cle.as_str())) {
        return;
    }
    let payload = minuteur::lire(|m| etat_json(m, minuteur::maintenant_ms(), &sons));
    PAYLOAD.with(|c| *c.borrow_mut() = Some(cle));
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
}

/// `"12:05"` → `(12, 5)`.
fn analyser_heure(texte: &str) -> Option<(u32, u32)> {
    let (h, m) = texte.split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

/// Un nom de son venu de la page — gardé SEULEMENT s'il désigne un fichier du
/// dossier `son/` (voir `minuteur::chemin_du_son`) ; `defaut` ou un inconnu
/// rendent `None`, c'est-à-dire le son par défaut.
fn analyser_son(texte: &str) -> Option<String> {
    minuteur::chemin_du_son(texte).map(|p| son::nom(&p))
}

/// La durée maximale d'un compte : 24 h. Au-delà, c'est une alarme qu'on veut.
const DUREE_MAX_S: i64 = 24 * 3600;

fn ajouter_compte(secondes: &str, nom: &str) {
    match secondes.parse::<i64>() {
        Ok(s) if (1..=DUREE_MAX_S).contains(&s) => {
            minuteur::modifier(|m| m.ajouter_compte(nom, s * 1000, minuteur::maintenant_ms()));
        }
        _ => crate::diag!("[minuteur] durée refusée : {secondes:?}"),
    }
}

fn enregistrer_alarme(id: &str, heure: &str, jours: &str, son_choisi: &str, nom: &str) {
    let id = match id {
        "nouvelle" => None,
        autre => match autre.parse::<u64>() {
            Ok(id) => Some(id),
            Err(_) => return crate::diag!("[minuteur] alarme refusée (id illisible) : {autre:?}"),
        },
    };
    let (Some(heure), Ok(jours)) = (analyser_heure(heure), jours.parse::<u8>()) else {
        return crate::diag!("[minuteur] alarme refusée : {heure:?} {jours:?}");
    };
    let son_choisi = analyser_son(son_choisi);
    let (jour, maintenant) = minuteur::jour_et_heure();
    minuteur::modifier(|m| m.enregistrer_alarme(id, nom, heure, jours, son_choisi, jour, maintenant));
}

/// Applique `f` à l'identifiant `id` s'il est lisible.
fn avec_id(id: &str, f: impl FnOnce(&mut Minuteur, u64)) {
    match id.parse::<u64>() {
        Ok(id) => minuteur::modifier(|m| f(m, id)),
        Err(_) => crate::diag!("[minuteur] identifiant illisible : {id:?}"),
    }
}

pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("minuteur/") else {
            crate::diag!("[minuteur] action inconnue : {action:?}");
            continue;
        };
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        let maintenant = minuteur::maintenant_ms();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["ready"] => PAYLOAD.with(|c| *c.borrow_mut() = None),
            ["close"] => cef_popup::fermer_par_geste(&SLOT),
            ["compte-ajouter", secondes] => ajouter_compte(secondes, ""),
            ["compte-ajouter", secondes, nom] => ajouter_compte(secondes, nom),
            ["compte-demarrer", id] => avec_id(id, |m, id| m.demarrer(id, maintenant)),
            ["compte-pause", id] => avec_id(id, |m, id| m.pause(id, maintenant)),
            ["compte-reinitialiser", id] => avec_id(id, Minuteur::reinitialiser),
            ["compte-supprimer", id] => avec_id(id, Minuteur::supprimer_compte),
            ["alarme-enregistrer", id, heure, jours, son_choisi] => enregistrer_alarme(id, heure, jours, son_choisi, ""),
            ["alarme-enregistrer", id, heure, jours, son_choisi, nom] => enregistrer_alarme(id, heure, jours, son_choisi, nom),
            ["alarme-basculer", id] => {
                let (jour, heure) = minuteur::jour_et_heure();
                avec_id(id, |m, id| m.basculer_alarme(id, jour, heure));
            }
            ["alarme-supprimer", id] => avec_id(id, Minuteur::supprimer_alarme),
            ["son-comptes", son_choisi] => {
                let son_choisi = analyser_son(son_choisi);
                minuteur::modifier(|m| m.son_comptes = son_choisi);
            }
            // L'écoute passe par son PROPRE alias (voir `son::jouer_ecoute`) :
            // elle ne coupe pas une sonnerie en cours.
            ["ecouter", son_choisi] => {
                if let Some(chemin) = minuteur::chemin_du_son(son_choisi) {
                    if let Err(e) = son::jouer_ecoute(&chemin) {
                        crate::diag!("[minuteur] écoute impossible : {e}");
                    }
                }
            }
            ["pomodoro-demarrer"] => minuteur::modifier(|m| m.pomodoro.demarrer(maintenant)),
            ["pomodoro-pause"] => minuteur::modifier(|m| m.pomodoro.pause(maintenant)),
            ["pomodoro-passer"] => minuteur::modifier(|m| m.pomodoro.passer(maintenant)),
            ["pomodoro-reinitialiser"] => minuteur::modifier(|m| m.pomodoro.reinitialiser()),
            ["pomodoro-duree", phase, minutes] => {
                match (minuteur::PhasePomodoro::depuis_cle(phase), minutes.parse::<u32>()) {
                    (Some(phase), Ok(minutes)) => minuteur::modifier(|m| m.pomodoro.regler(phase, minutes)),
                    _ => crate::diag!("[minuteur] durée de pomodoro illisible : {phase:?} {minutes:?}"),
                }
            }
            ["chrono-demarrer"] => minuteur::modifier(|m| m.chrono.demarrer(maintenant)),
            ["chrono-pause"] => minuteur::modifier(|m| m.chrono.pause(maintenant)),
            ["chrono-tour"] => minuteur::modifier(|m| m.chrono.tour(maintenant)),
            ["chrono-reinitialiser"] => minuteur::modifier(|m| m.chrono.reinitialiser()),
            ["arreter"] => minuteur::modifier(Minuteur::arreter),
            ["repeter"] => minuteur::modifier(|m| m.repeter(maintenant)),
            _ => crate::diag!("[minuteur] action inconnue : {action:?}"),
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
/// DEUX listes à tenir (`NEURADESK_DEBUG_OPEN=minuteur`).
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
    /// `cef_calendar_applet::tests::verbes_du_dispatch`, même lecture, bornée
    /// à avant `#[cfg(test)]`.
    fn verbes_du_dispatch() -> Vec<String> {
        let source = include_str!("cef_minuteur.rs");
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
        const SOURCE: &str = include_str!("../../assets/popups/minuteur.html");
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

    #[test]
    fn une_heure_se_lit_ou_se_refuse() {
        assert_eq!(analyser_heure("07:05"), Some((7, 5)));
        assert_eq!(analyser_heure("24:00"), None);
        assert_eq!(analyser_heure("7h05"), None);
    }
}
