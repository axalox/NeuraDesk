//! **LA FENÊTRE DE L'APPLET « ENREGISTRER »** — on y choisit la source (écran,
//! zone, webcam, son seul), le son (PC, micro, les deux), l'incrustation de la
//! webcam, puis on démarre et on arrête. Le pilote est `win32::enregistreur`.
//!
//! DEMANDES (2026-10-31) : « l'écran ou une zone sélectionnée de l'écran ou la
//! webcam avec ou sans le son », « une applet dédiée, utilisons gstreamer »,
//! « n'enregistrer que du son », « l'incrustation de la webcam dans un coin ».
//!
//! ⭐ **UNE `fenetre: true` DE LA FAMILLE DE `cef_maths`** — punaise, place,
//! ouverte au centre de l'écran PRIMAIRE. ⛔ Sans `render_if_visible`, elle est
//! INVISIBLE mais prend le focus (voir `cef_caracteres`).
//!
//! ⭐ **ELLE N'APPARAÎT PAS DANS L'ENREGISTREMENT** : comme toutes les fenêtres
//! de NeuraDesk, elle porte `WDA_EXCLUDEFROMCAPTURE` (voir
//! `gfx::capture_visibility`), que la duplication du bureau respecte — sauf si
//! l'utilisateur a rendu l'interface visible aux captures.

use std::cell::{Cell, RefCell};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetSystemMetrics, GetWindowRect, SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SM_CXSCREEN,
    SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WM_CLOSE, WM_NCHITTEST,
};

use crate::gfx::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec, PART_ECRAN};
use crate::win32::enregistreur::{self, Choix, EcranVu, Etat, EtatListe};

thread_local! {
    static SLOT: PopupSlot = PopupSlot::new();
    /// Le dernier état poussé (voir `cef_caracteres::PAYLOAD`, même garde).
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// La rafale de poussée — la page n'a pas fini de charger à l'ouverture.
    static RAFALE: Cell<u32> = const { Cell::new(0) };
    /// Les choix, lus une fois puis tenus ici — la page les modifie un à un.
    static CHOIX: RefCell<Option<Choix>> = const { RefCell::new(None) };
    /// Les écrans, relevés À L'OUVERTURE (et par « Actualiser ») — pas à chaque
    /// frame : `monitors::enumerate` cherche aussi les barres des tâches.
    static ECRANS: RefCell<Vec<EcranVu>> = const { RefCell::new(Vec::new()) };
    /// `gst::installe`, relevé à l'ouverture — il touche le disque.
    static GST: Cell<bool> = const { Cell::new(false) };
    /// La demande de choisir une zone, que `app::run` consomme (c'est lui qui
    /// tient le sélecteur de `capture_ecran`).
    static ZONE_DEMANDEE: Cell<bool> = const { Cell::new(false) };
    /// Vrai de la demande de zone jusqu'au retour de la fenêtre.
    static ATTEND_ZONE: Cell<bool> = const { Cell::new(false) };
    /// Le levier du harnais, une fois lu (voir `lever_du_harnais`).
    static LEVIER: Cell<Option<u64>> = const { Cell::new(None) };
    /// **OUVERTE AU CLIC DROIT : L'INTERFACE RESTE** (DEMANDE du 2026-11-01 :
    /// « qu'un clic droit dessus permette de lancer l'applet sans cacher
    /// l'interface ») — le choix de la zone ne masque pas les barres, et
    /// l'enregistrement filme les fenêtres de NeuraDesk. Le même partage que la
    /// capture d'écran : clic gauche sans l'interface, clic droit avec.
    static GARDER_INTERFACE: Cell<bool> = const { Cell::new(false) };
    /// Le mode FIGÉ au démarrage de l'enregistrement : rouvrir la fenêtre au
    /// clic gauche pendant qu'il tourne ne doit pas changer ce qui est filmé.
    static FILME_L_INTERFACE: Cell<bool> = const { Cell::new(false) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskEnregistrerWindow"),
    window_title: w!("NeuraDesk — Enregistrer"),
    html_file: "enregistrer.html",
    wndproc: Some(wndproc),
    above_a_magnified_dock: false,
    // `fades: false` est OBLIGATOIRE pour une fenêtre (voir `cef_kaomoji::SPEC`).
    fades: false,
    fenetre: true,
};

/// La taille de la page, en unités CSS — le contenu défile au-delà.
const LARGEUR_PAGE: i32 = 440;
const HAUTEUR_PAGE: i32 = 640;
/// Unités de page → pixels physiques : le zoom `CONTENT_ZOOM_FACTOR` (3×).
const PX_PAR_UNITE: i32 = 3;

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// Voir `cef_kaomoji::recaler_si_il_ne_tient_plus` — ouverte sur la PRIMAIRE.
pub fn recaler_si_il_ne_tient_plus() {
    cef_popup::recaler_si_il_ne_tient_plus(&SLOT);
}

/// La taille de la fenêtre, en pixels physiques, plafonnée à `PART_ECRAN`.
fn taille(largeur_ecran: i32, hauteur_ecran: i32) -> [i32; 2] {
    let plafond_l = (largeur_ecran * PART_ECRAN / 100) / PX_PAR_UNITE;
    let plafond_h = (hauteur_ecran * PART_ECRAN / 100) / PX_PAR_UNITE;
    [LARGEUR_PAGE.min(plafond_l) * PX_PAR_UNITE, HAUTEUR_PAGE.min(plafond_h) * PX_PAR_UNITE]
}

/// **LE RECTANGLE D'OUVERTURE — AU CENTRE DE L'ÉCRAN PRIMAIRE**.
pub fn rect_pour_ecran_primaire() -> [i32; 4] {
    let (largeur, hauteur) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let [l, h] = cef_popup::taille_aux_textes(largeur, hauteur, taille);
    [((largeur - l) / 2).max(0), ((hauteur - h) / 2).max(0), l, h]
}

fn releve() {
    ECRANS.with(|e| *e.borrow_mut() = enregistreur::ecrans());
    GST.with(|g| g.set(crate::win32::gst::installe()));
    if GST.with(Cell::get) {
        enregistreur::demander_la_liste(false);
    }
}

/// Ouvre ou referme la fenêtre. Appelée par le bras de clic GAUCHE de l'applet.
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice) {
    ouvrir_ou_fermer(rect, gfx, false);
}

/// Le clic DROIT : la même fenêtre, l'interface gardée (voir `GARDER_INTERFACE`).
pub fn toggle_en_gardant_l_interface(rect: [i32; 4], gfx: &GfxDevice) {
    ouvrir_ou_fermer(rect, gfx, true);
}

/// Un clic dans le MÊME mode referme la fenêtre ouverte ; dans l'AUTRE, il
/// change de mode et la laisse ouverte — sinon passer du gauche au droit
/// demanderait deux clics, le premier fermant ce qu'on voulait garder.
fn ouvrir_ou_fermer(rect: [i32; 4], gfx: &GfxDevice, garder: bool) {
    let meme_mode = GARDER_INTERFACE.with(|g| g.replace(garder)) == garder;
    if cef_popup::is_visible(&SLOT) {
        if meme_mode {
            if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                cef_popup::cacher_fenetre(hwnd);
            }
        } else {
            push_state();
        }
        return;
    }
    ouvrir(rect, gfx);
}

/// Vrai si la fenêtre a été ouverte au clic droit.
pub fn garde_l_interface() -> bool {
    GARDER_INTERFACE.with(Cell::get)
}

/// Vrai tant qu'un enregistrement lancé au clic droit tourne : l'interface doit
/// rester capturable (voir `capture_visibility::forcer_capturable`).
pub fn interface_dans_l_enregistrement() -> bool {
    enregistreur::en_cours() && FILME_L_INTERFACE.with(Cell::get)
}

fn ouvrir(rect: [i32; 4], gfx: &GfxDevice) {
    crate::diag!("[enregistrer] ouverture : rect {rect:?} (x,y,largeur,hauteur), interface gardee {}", garde_l_interface());
    releve();
    PAYLOAD.with(|p| *p.borrow_mut() = None);
    cef_popup::regler_zoom_textes(&SLOT, crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES), rect);
    cef_popup::montrer_fenetre(&SLOT, &SPEC, rect, gfx);
    RAFALE.with(|r| r.set(6));
    push_state();
    lever_du_harnais();
}

/// **LEVIER DU HARNAIS** : `NEURADESK_DEBUG_ENREGISTRER=<secondes>` démarre un
/// enregistrement à la première ouverture (les choix ENREGISTRÉS, mais le
/// fichier dans `NEURADESK_CAPTURE`, jamais dans les Vidéos de l'utilisateur),
/// et `refresh_if_visible` l'arrête au bout de `<secondes>`. Tout le chemin réel
/// — sous-processus, `en-cours`, `stop`, `fini` — sans un clic.
fn lever_du_harnais() {
    if LEVIER.with(|l| l.get()).is_some() {
        return;
    }
    let Some(secondes) = std::env::var("NEURADESK_DEBUG_ENREGISTRER").ok().and_then(|v| v.parse::<u64>().ok()) else {
        return;
    };
    LEVIER.with(|l| l.set(Some(secondes)));
    let mut c = choix();
    if let Ok(dossier) = std::env::var("NEURADESK_CAPTURE") {
        c.dossier = dossier;
    }
    let resultat = enregistreur::demarrer(&c);
    crate::diag!("[enregistrer] levier du harnais : {secondes} s -> {resultat:?}");
}

/// Vrai tant que la fenêtre attend la fin du choix d'une zone.
/// Le panneau est-il à l'écran ? S'il l'est, la fin s'y lit déjà
/// (« Enregistré : … Montrer ») et le toast ferait doublon.
pub fn est_visible() -> bool {
    cef_popup::is_visible(&SLOT)
}

pub fn attend_une_zone() -> bool {
    ATTEND_ZONE.with(Cell::get)
}

/// Rouvre la fenêtre si elle est cachée (après le choix d'une zone).
pub fn montrer(gfx: &GfxDevice) {
    ATTEND_ZONE.with(|a| a.set(false));
    // `ouvrir`, pas `toggle` : le retour après la zone GARDE le mode.
    if !cef_popup::is_visible(&SLOT) {
        ouvrir(rect_pour_ecran_primaire(), gfx);
    }
}

/// **LE DESSIN — SANS LUI, LA FENÊTRE EST INVISIBLE.**
pub fn render_if_visible(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

/// La copie du back buffer pour le harnais (voir `cef_kaomoji::debug_dump_backbuffer`).
pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

fn choix() -> Choix {
    CHOIX.with(|c| c.borrow_mut().get_or_insert_with(Choix::charger).clone())
}

fn modifier(f: impl FnOnce(&mut Choix)) {
    let mut nouveau = choix();
    f(&mut nouveau);
    if !nouveau.enregistrer() {
        crate::diag!("[enregistrer] reglages NON ecrits sur le disque");
    }
    CHOIX.with(|c| *c.borrow_mut() = Some(nouveau));
}

/// Le libellé d'un écran : « Écran 2 — 3840 × 2160 (principal) ».
fn libelle_ecran(rang: usize, e: &EcranVu) -> String {
    let principal = if e.principal { " (principal)" } else { "" };
    format!("Écran {} — {} × {}{principal}", rang + 1, e.rect[2], e.rect[3])
}

fn etat_en_json(etat: &Etat) -> serde_json::Value {
    match etat {
        Etat::Repos => serde_json::json!({ "nom": "repos" }),
        Etat::Demarrage => serde_json::json!({ "nom": "demarrage" }),
        Etat::EnCours { depuis } => serde_json::json!({ "nom": "en-cours", "secondes": depuis.elapsed().as_secs() }),
        Etat::Arret => serde_json::json!({ "nom": "arret" }),
        Etat::Fini { fichier } => serde_json::json!({
            "nom": "fini",
            "fichier": fichier.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        }),
        Etat::Erreur(message) => serde_json::json!({ "nom": "erreur", "message": message }),
    }
}

fn liste_en_json(liste: &EtatListe) -> serde_json::Value {
    let peripheriques = |p: &[enregistreur::Peripherique]| -> Vec<serde_json::Value> {
        p.iter().map(|p| serde_json::json!({ "nom": p.nom, "id": p.id })).collect()
    };
    match liste {
        EtatListe::Jamais | EtatListe::EnCours => serde_json::json!({ "etat": "en-cours", "webcams": [], "micros": [] }),
        EtatListe::Prete(p) => serde_json::json!({
            "etat": "prete",
            "webcams": peripheriques(&p.webcams),
            "micros": peripheriques(&p.micros),
        }),
        EtatListe::Echec(e) => serde_json::json!({ "etat": "echec", "message": e, "webcams": [], "micros": [] }),
    }
}

/// L'état de la page. ⚠️ Les listes proposées (cadences, tailles) viennent
/// d'ICI : une seconde copie dans la page divergerait.
fn etat_json() -> String {
    let c = choix();
    let ecrans: Vec<serde_json::Value> = ECRANS.with(|e| {
        e.borrow()
            .iter()
            .enumerate()
            .map(|(rang, e)| serde_json::json!({ "nom": e.nom, "libelle": libelle_ecran(rang, e), "principal": e.principal }))
            .collect()
    });
    serde_json::json!({
        "epingle": crate::config::load_enregistrer_premier_plan(),
        "gst": GST.with(Cell::get),
        "choix": {
            "source": c.source.nom(),
            "ecran": c.ecran,
            "zone": c.zone,
            "webcam": c.webcam,
            "son_pc": c.son_pc,
            "micro": c.micro,
            "micro_id": c.micro_id,
            "curseur": c.curseur,
            "incruster": c.incruster,
            "coin": c.coin.nom(),
            "taille": c.taille,
            "ips": c.ips,
            "qualite": c.qualite.nom(),
            "format_son": c.format_son.extension(),
        },
        "dossier": c.dossier_effectif().to_string_lossy(),
        "ecrans": ecrans,
        "liste": liste_en_json(&enregistreur::liste()),
        "etat": etat_en_json(&enregistreur::etat()),
        "garderInterface": garde_l_interface(),
        "tailles": enregistreur::TAILLES_VIGNETTE,
        "cadences": enregistreur::CADENCES,
    })
    .to_string()
}

/// Pousse l'état s'il a changé.
pub fn push_state() {
    let json = etat_json();
    let change = PAYLOAD.with(|p| {
        let mut p = p.borrow_mut();
        if p.as_deref() == Some(json.as_str()) {
            false
        } else {
            *p = Some(json.clone());
            true
        }
    });
    if change {
        cef_popup::execute_js(&SLOT, &format!("if (window.setState) window.setState({json})"));
    }
}

/// **À APPELER À CHAQUE FRAME** — le premier plan, et l'état (la durée change
/// chaque seconde, la liste des périphériques arrive en tâche de fond).
pub fn refresh_if_visible() {
    if !cef_popup::is_visible(&SLOT) {
        return;
    }
    appliquer_le_premier_plan();
    if let (Some(secondes), Some(duree)) = (LEVIER.with(Cell::get), enregistreur::duree()) {
        if duree.as_secs() >= secondes {
            enregistreur::arreter();
        }
    }
    let restante = RAFALE.with(|r| r.get());
    if restante > 0 {
        RAFALE.with(|r| r.set(restante - 1));
        PAYLOAD.with(|p| *p.borrow_mut() = None);
    }
    push_state();
}

fn appliquer_le_premier_plan() {
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return };
    let rang = if crate::config::load_enregistrer_premier_plan() { Some(HWND_TOPMOST) } else { Some(HWND_NOTOPMOST) };
    unsafe {
        let _ = SetWindowPos(hwnd, rang, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// `app::run` demande s'il faut ouvrir le sélecteur de zone — et consomme la demande.
pub fn prendre_la_demande_de_zone() -> bool {
    ZONE_DEMANDEE.with(|z| z.replace(false))
}

/// La zone choisie par le sélecteur, en pixels physiques du bureau virtuel.
pub fn poser_la_zone(zone: [i32; 4]) {
    crate::diag!("[enregistrer] zone choisie : {zone:?}");
    modifier(|c| {
        c.zone = Some(zone);
        c.source = enregistreur::SourceChoisie::Zone;
    });
}

/// **CE QUE LA PAGE DEMANDE.**
pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("enregistrer/") else { continue };
        let mut morceaux = reste.split('/');
        let verbe = morceaux.next().unwrap_or("");
        match verbe {
            "deplacer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::deplacer_par_le_systeme(hwnd, "enregistrer");
                }
            }
            "fermer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::cacher_fenetre(hwnd);
                }
            }
            "epingler" => {
                let actif = !crate::config::load_enregistrer_premier_plan();
                crate::config::save_enregistrer_premier_plan(actif);
                appliquer_le_premier_plan();
            }
            "regler" => {
                // ⚠️ La VALEUR voyage percent-encodée (un nom de webcam porte des
                // espaces, un identifiant WASAPI des accolades).
                let cle = morceaux.next().unwrap_or("").to_string();
                let valeur = crate::win32::cef_app::percent_decode(morceaux.next().unwrap_or(""));
                let mut accepte = false;
                modifier(|c| accepte = c.regler(&cle, &valeur));
                if !accepte {
                    crate::diag!("[enregistrer] reglage refuse : {cle}={valeur:?}");
                }
            }
            "demarrer" => {
                FILME_L_INTERFACE.with(|f| f.set(garde_l_interface()));
                let _ = enregistreur::demarrer(&choix());
            }
            "arreter" => {
                enregistreur::arreter();
            }
            "zone" => {
                // Le sélecteur prend tout l'écran : la fenêtre s'efface, et
                // `app::run` la rouvre une fois la zone posée.
                ZONE_DEMANDEE.with(|z| z.set(true));
                ATTEND_ZONE.with(|a| a.set(true));
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::cacher_fenetre(hwnd);
                }
            }
            "dossier" => choisir_le_dossier(),
            "ouvrir" => {
                let cible = match enregistreur::etat() {
                    Etat::Fini { fichier } => fichier,
                    _ => choix().dossier_effectif(),
                };
                enregistreur::montrer(&cible);
            }
            "lire" => lire_le_fichier_enregistre(),
            "actualiser" => {
                releve();
                enregistreur::demander_la_liste(true);
            }
            autre => crate::diag!("[enregistrer] verbe inconnu : {autre:?}"),
        }
    }
}

/// **« MONTRER » LE FICHIER ENREGISTRÉ** — DEMANDE (2026-11-01) : « Montrer la
/// vidéo devrait utiliser la fenêtre afficher du dash dossier ». Une vidéo
/// s'ouvre donc dans le MÊME lecteur que « Afficher » (`lecteur_video`, libVLC),
/// avec la même règle d'aiguillage (`sous_dock::est_une_video`) ; un son seul,
/// que ce lecteur ne prend pas, reste montré dans l'Explorateur.
fn lire_le_fichier_enregistre() {
    let Etat::Fini { fichier } = enregistreur::etat() else { return };
    if crate::win32::sous_dock::est_une_video(&fichier) {
        // Un échec du lecteur ne laisse pas le clic sans effet : l'Explorateur
        // montre au moins le fichier, et le journal dit pourquoi.
        // L'enregistrement n'est ouvert d'aucune moitié du dash dossier : l'ordre
        // par défaut, qui servira aux flèches de voisinage dans son dossier.
        if let Err(raison) = crate::win32::lecteur_video::ouvrir(&fichier, crate::win32::sous_dock::Tri::default()) {
            crate::diag!("[enregistrer] lecteur : {raison}");
            enregistreur::montrer(&fichier);
        }
    } else {
        enregistreur::montrer(&fichier);
    }
}

fn choisir_le_dossier() {
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return };
    // ⚠️ Le dialogue est MODAL et gèle le redessin du dock (voir `file_dialog`).
    let Some(chemin) = crate::win32::file_dialog::choisir_dossier(hwnd, "Dossier des enregistrements") else {
        return;
    };
    modifier(|c| c.dossier = chemin.to_string_lossy().into_owned());
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCHITTEST {
        let mut cadre = windows::Win32::Foundation::RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut cadre) }.is_ok() {
            let rect = [cadre.left, cadre.top, cadre.right - cadre.left, cadre.bottom - cadre.top];
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            return LRESULT(code_pour_point(rect, (x, y)) as isize);
        }
    }
    if msg == WM_CLOSE {
        cef_popup::cacher_fenetre(hwnd);
        return LRESULT(0);
    }
    if let Some(resultat) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return resultat;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Le code de `WM_NCHITTEST` : les huit bords et coins, sinon le client. ⚠️ Une
/// copie de plus (voir `cef_kaomoji::code_pour_point`).
fn code_pour_point(rect: [i32; 4], (x, y): (i32, i32)) -> i32 {
    const BORD: i32 = 12;
    let (gauche, haut) = (x < rect[0] + BORD, y < rect[1] + BORD);
    let (droite, bas) = (x >= rect[0] + rect[2] - BORD, y >= rect[1] + rect[3] - BORD);
    match (gauche, droite, haut, bas) {
        (true, _, true, _) => 13,
        (_, true, true, _) => 14,
        (true, _, _, true) => 16,
        (_, true, _, true) => 17,
        (true, _, _, _) => 10,
        (_, true, _, _) => 11,
        (_, _, true, _) => 12,
        (_, _, _, true) => 15,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le mode « interface gardée » voyage dans l'état et la page le lit ; il
    /// n'est FILMÉ que pendant un enregistrement (au repos : rien à forcer).
    #[test]
    fn le_mode_interface_gardee_va_jusqu_a_la_page() {
        GARDER_INTERFACE.with(|g| g.set(true));
        let etat: serde_json::Value = serde_json::from_str(&etat_json()).unwrap();
        assert_eq!(etat["garderInterface"], serde_json::json!(true));
        GARDER_INTERFACE.with(|g| g.set(false));
        let etat: serde_json::Value = serde_json::from_str(&etat_json()).unwrap();
        assert_eq!(etat["garderInterface"], serde_json::json!(false));
        let page = include_str!("../../assets/popups/enregistrer.html");
        assert!(page.contains("state.garderInterface"));
        FILME_L_INTERFACE.with(|f| f.set(true));
        assert!(!interface_dans_l_enregistrement(), "au repos, rien n'est filmé");
        FILME_L_INTERFACE.with(|f| f.set(false));
    }

    const VERBES: [&str; 11] =
        ["deplacer", "fermer", "epingler", "regler", "demarrer", "arreter", "zone", "dossier", "ouvrir", "lire", "actualiser"];

    /// Les verbes que la page passe à `go("…")` — le premier segment.
    fn verbes_de_la_page() -> Vec<String> {
        let page = include_str!("../../assets/popups/enregistrer.html");
        page.split("go(\"")
            .skip(1)
            .map(|m| m.split(['"', '/']).next().unwrap_or("").to_string())
            .collect()
    }

    /// ⭐ **LA PAGE ET RUST PARLENT LE MÊME LANGAGE** — chaque verbe de la page
    /// a son bras, et chaque bras est appelé par la page.
    #[test]
    fn les_verbes_de_la_page_sont_exactement_ceux_du_dispatch() {
        let vus = verbes_de_la_page();
        for verbe in &vus {
            assert!(VERBES.contains(&verbe.as_str()), "verbe inconnu passé à `go` : {verbe:?}");
        }
        for attendu in &VERBES {
            assert!(vus.iter().any(|v| v == attendu), "la page n'appelle jamais `{attendu}`");
        }
        let source = include_str!("cef_enregistrer.rs");
        let dispatch = &source[source.find("pub fn dispatch_actions").unwrap()..source.find("fn choisir_le_dossier").unwrap()];
        for attendu in &VERBES {
            assert!(dispatch.contains(&format!("\"{attendu}\"")), "aucun bras pour `{attendu}`");
        }
    }

    /// Chaque clé que la page règle est acceptée par `Choix::regler`.
    #[test]
    fn chaque_cle_reglee_par_la_page_est_connue() {
        let page = include_str!("../../assets/popups/enregistrer.html");
        let mut vues = 0;
        for morceau in page.split("regler(\"").skip(1) {
            let cle = morceau.split('"').next().unwrap_or("");
            let mut c = Choix::default();
            // Une valeur valable pour chaque clé — seule la CLÉ est testée ici.
            let valeur = match cle {
                "source" => "zone",
                "coin" => "haut-gauche",
                "taille" => "15",
                "ips" => "60",
                "qualite" => "haute",
                "format_son" => "mp3",
                "ecran" | "webcam" | "micro_id" => "x",
                _ => "1",
            };
            assert!(c.regler(cle, valeur), "clé refusée : {cle:?}");
            vues += 1;
        }
        assert!(vues >= 10, "la page règle trop peu de clés : {vues}");
    }

    #[test]
    fn l_etat_est_un_json_lisible() {
        let json: serde_json::Value = serde_json::from_str(&etat_json()).expect("un JSON valide");
        assert_eq!(json["choix"]["source"].as_str().map(|s| s.len() > 1), Some(true));
        assert_eq!(json["tailles"].as_array().map(Vec::len), Some(3));
    }

    #[test]
    fn la_fenetre_tient_sur_l_ecran() {
        for (l_ecran, h_ecran) in [(3840, 2160), (1920, 1080), (1366, 768)] {
            let [l, h] = taille(l_ecran, h_ecran);
            assert!(l <= l_ecran * PART_ECRAN / 100 && h <= h_ecran * PART_ECRAN / 100);
        }
        // Un écran assez grand porte la page entière, sans plafond.
        assert_eq!(taille(5120, 2880), [LARGEUR_PAGE * 3, HAUTEUR_PAGE * 3]);
    }

    #[test]
    fn le_facteur_de_la_page_est_bien_celui_du_zoom() {
        assert_eq!(PX_PAR_UNITE as f64, cef_popup::CONTENT_ZOOM_FACTOR);
    }
}
