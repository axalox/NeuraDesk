//! **LA FENÊTRE DU DNS** — changer les serveurs DNS de Windows, dans la zone
//! Intégrés (voir `win32::dns` pour la lecture et l'application).
//!
//! DEMANDE (2026-11-22) : « dans intégrés je voudrais une applet pour modifier
//! le dns de windows ».
//!
//! ⭐ **LA JUMELLE DE `cef_patience`** pour la chrome (une `fenetre: true` au
//! centre de l'écran PRIMAIRE, punaise, croix, bande de titre). La page montre
//! les cartes actives et leurs serveurs, des préréglages (automatique,
//! Cloudflare, Google, Quad9…) et deux champs libres.
//!
//! ⚠️ **L'APPLICATION BLOQUE** (invite UAC puis PowerShell) : elle part sur un
//! FIL DE TRAVAIL. `SLOT` est `thread_local` — le fil ne peut pas parler à la
//! page ; il dépose son issue dans `ISSUE`, que `refresh_if_visible` relève
//! (relecture des cartes, puis `push_state`).

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetSystemMetrics, GetWindowRect, SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SM_CXSCREEN,
    SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WM_CLOSE, WM_NCHITTEST,
};

use crate::gfx::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec, PART_ECRAN};
use crate::win32::dns;

thread_local! {
    static SLOT: PopupSlot = PopupSlot::new();
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    static RAFALE: Cell<u32> = const { Cell::new(0) };
    static CARTES: RefCell<Vec<dns::Carte>> = const { RefCell::new(Vec::new()) };
    /// La dernière issue montrée par la page (`""` = aucune).
    static MESSAGE: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Une application en cours sur le fil de travail.
static EN_COURS: AtomicBool = AtomicBool::new(false);
/// L'issue déposée par le fil, relevée par `refresh_if_visible`.
static ISSUE: Mutex<Option<dns::Issue>> = Mutex::new(None);

/// Le réglage « rester au premier plan » de CETTE fenêtre (voir `cef_jeu2048`).
const PREMIER_PLAN: &str = "dns_premier_plan";

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskDnsWindow"),
    window_title: w!("NeuraDesk — DNS"),
    html_file: "dns.html",
    wndproc: Some(wndproc),
    above_a_magnified_dock: false,
    // `fades: false` est OBLIGATOIRE pour une fenêtre (voir `cef_maths::SPEC`).
    fades: false,
    fenetre: true,
};

/// Unités de page → pixels physiques : le zoom `CONTENT_ZOOM_FACTOR` (3×).
const PX_PAR_UNITE: i32 = 3;
/// La taille VISÉE, en unités de page : la liste des cartes, les préréglages,
/// les deux champs et les boutons. Plafonnée à `PART_ECRAN`.
const LARGEUR: i32 = 460;
const HAUTEUR: i32 = 600;

fn taille_pour_tout_le_contenu(largeur_ecran: i32, hauteur_ecran: i32) -> [i32; 2] {
    let plafond_l = (largeur_ecran * PART_ECRAN / 100) / PX_PAR_UNITE;
    let plafond_h = (hauteur_ecran * PART_ECRAN / 100) / PX_PAR_UNITE;
    [LARGEUR.min(plafond_l) * PX_PAR_UNITE, HAUTEUR.min(plafond_h) * PX_PAR_UNITE]
}

/// **LE RECTANGLE D'OUVERTURE — AU CENTRE DE L'ÉCRAN PRIMAIRE** (voir `cef_jeu2048`).
pub fn rect_pour_ecran_primaire() -> [i32; 4] {
    let (largeur, hauteur) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let [l, h] = cef_popup::taille_aux_textes(largeur, hauteur, taille_pour_tout_le_contenu);
    [((largeur - l) / 2).max(0), ((hauteur - h) / 2).max(0), l, h]
}

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// Voir `cef_maths::recaler_si_il_ne_tient_plus` — ouverte sur la PRIMAIRE.
pub fn recaler_si_il_ne_tient_plus() {
    cef_popup::recaler_si_il_ne_tient_plus(&SLOT);
}

/// Ouvre ou referme la fenêtre. Appelée par le bras de clic de l'applet.
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice) {
    if cef_popup::basculer_ou_cacher_si_ouvert(&SLOT) {
        return;
    }
    crate::diag!("[dns] ouverture : rect {rect:?} (x,y,largeur,hauteur)");
    relire();
    MESSAGE.with(|m| m.borrow_mut().clear());
    PAYLOAD.with(|p| *p.borrow_mut() = None);
    cef_popup::regler_zoom_textes(&SLOT, crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES), rect);
    cef_popup::montrer_fenetre(&SLOT, &SPEC, rect, gfx);
    RAFALE.with(|r| r.set(6));
    push_state();
}

pub fn render_if_visible(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

fn relire() {
    let cartes = dns::cartes();
    crate::diag!("[dns] {} carte(s) active(s)", cartes.len());
    CARTES.with(|c| *c.borrow_mut() = cartes);
}

fn etat_json() -> String {
    let cartes: Vec<serde_json::Value> = CARTES.with(|c| {
        c.borrow()
            .iter()
            .map(|carte| {
                serde_json::json!({
                    "index": carte.index,
                    "nom": carte.nom,
                    "description": carte.description,
                    "serveurs": carte.serveurs,
                    "manuel": carte.manuel,
                })
            })
            .collect()
    });
    serde_json::json!({
        "epingle": crate::config::load_bool_setting_or(PREMIER_PLAN, false),
        "cartes": cartes,
        "enCours": EN_COURS.load(Ordering::SeqCst),
        "message": MESSAGE.with(|m| m.borrow().clone()),
    })
    .to_string()
}

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

pub fn refresh_if_visible() {
    // L'issue d'une application se relève même fenêtre cachée : elle est
    // montrée à la prochaine ouverture si l'utilisateur a fermé entre-temps.
    let issue = ISSUE.lock().ok().and_then(|mut i| i.take());
    if let Some(issue) = issue {
        EN_COURS.store(false, Ordering::SeqCst);
        MESSAGE.with(|m| *m.borrow_mut() = issue.code().to_string());
        relire();
        push_state();
    }
    if !cef_popup::is_visible(&SLOT) {
        return;
    }
    appliquer_le_premier_plan();
    let restante = RAFALE.with(|r| r.get());
    if restante > 0 {
        RAFALE.with(|r| r.set(restante - 1));
        push_state();
    }
}

fn appliquer_le_premier_plan() {
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return };
    let rang = if crate::config::load_bool_setting_or(PREMIER_PLAN, false) { Some(HWND_TOPMOST) } else { Some(HWND_NOTOPMOST) };
    unsafe {
        let _ = SetWindowPos(hwnd, rang, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Lance une demande sur un fil de travail (une seule à la fois).
fn lancer(demande: dns::Demande) {
    if EN_COURS.swap(true, Ordering::SeqCst) {
        crate::diag!("[dns] une application est déjà en cours");
        return;
    }
    crate::diag!("[dns] demande : {demande:?}");
    MESSAGE.with(|m| m.borrow_mut().clear());
    push_state();
    let lance = std::thread::Builder::new().name("dns-appliquer".into()).spawn(move || {
        let issue = dns::appliquer(&demande);
        if let Ok(mut i) = ISSUE.lock() {
            *i = Some(issue);
        }
    });
    if lance.is_err() {
        EN_COURS.store(false, Ordering::SeqCst);
        MESSAGE.with(|m| *m.borrow_mut() = dns::Issue::Echec.code().to_string());
        push_state();
    }
}

/// **CE QUE LA PAGE DEMANDE** : les trois gestes de la fenêtre, plus
/// `appliquer/<cartes>/<auto|adresses>`, `vider` et `actualiser`.
pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(verbe) = action.strip_prefix("dns/") else { continue };
        let mut morceaux = verbe.splitn(3, '/');
        match morceaux.next().unwrap_or("") {
            "deplacer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::deplacer_par_le_systeme(hwnd, "dns");
                }
            }
            "fermer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::cacher_fenetre(hwnd);
                }
            }
            "epingler" => {
                let actif = !crate::config::load_bool_setting_or(PREMIER_PLAN, false);
                crate::config::save_bool_setting(PREMIER_PLAN, actif);
                appliquer_le_premier_plan();
                push_state();
            }
            // La page est prête : l'état poussé AVANT son script (à
            // l'ouverture) a été perdu — oublier le dernier envoi pour le
            // renvoyer en entier.
            "ready" => {
                PAYLOAD.with(|p| *p.borrow_mut() = None);
                push_state();
            }
            "actualiser" => {
                relire();
                MESSAGE.with(|m| m.borrow_mut().clear());
                push_state();
            }
            "vider" => lancer(dns::Demande::ViderCache),
            "appliquer" => {
                let (cartes, quoi) = (morceaux.next().unwrap_or(""), morceaux.next().unwrap_or(""));
                // Relue AVANT de valider : une carte débranchée entre-temps est refusée.
                relire();
                let connues: Vec<u32> = CARTES.with(|c| c.borrow().iter().map(|carte| carte.index).collect());
                match dns::lire_demande(cartes, &decoder(quoi), &connues) {
                    Some(demande) => lancer(demande),
                    None => {
                        crate::diag!("[dns] demande refusée : {cartes:?} {quoi:?}");
                        MESSAGE.with(|m| *m.borrow_mut() = "invalide".to_string());
                        push_state();
                    }
                }
            }
            autre => crate::diag!("[dns] verbe inconnu : {autre:?}"),
        }
    }
}

/// La page encode la liste (`encodeURIComponent`) : `:` et `,` y passent en
/// `%3A` et `%2C`. Un `%` mal formé donne une chaîne que `lire_demande` refuse.
fn decoder(s: &str) -> String {
    let octets = s.as_bytes();
    let mut sortie = Vec::with_capacity(octets.len());
    let mut i = 0;
    while i < octets.len() {
        if octets[i] == b'%' && i + 2 < octets.len() {
            // Lu en OCTETS : un caractère non ASCII après `%` ne coupe pas une chaîne.
            let hex = std::str::from_utf8(&octets[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                sortie.push(v);
                i += 3;
                continue;
            }
        }
        sortie.push(octets[i]);
        i += 1;
    }
    String::from_utf8_lossy(&sortie).into_owned()
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

    fn verbes_connus() -> [&'static str; 7] {
        ["deplacer", "epingler", "fermer", "ready", "actualiser", "vider", "appliquer"]
    }

    #[test]
    fn les_verbes_de_la_page_sont_exactement_ceux_du_dispatch() {
        let page = include_str!("../../assets/popups/dns.html");
        let mut vus: Vec<&str> = Vec::new();
        for morceau in page.split("go(\"").skip(1) {
            let verbe = morceau.split(['"', '/']).next().unwrap_or("");
            assert!(verbes_connus().contains(&verbe), "verbe inconnu passé à `go` : {verbe:?}");
            vus.push(verbe);
        }
        for attendu in verbes_connus() {
            assert!(vus.contains(&attendu), "la page n'appelle jamais `{attendu}`");
        }
        assert!(page.contains("neuradesk://dns/"), "la page ne parle pas à `dns/`");
    }

    #[test]
    fn la_liste_encodee_se_decode() {
        assert_eq!(decoder("1.1.1.1%2C2606%3A4700%3A4700%3A%3A1111"), "1.1.1.1,2606:4700:4700::1111");
        assert_eq!(decoder("auto"), "auto");
        assert_eq!(decoder("1.1.1.1%"), "1.1.1.1%");
        assert_eq!(decoder("%zz"), "%zz");
        assert_eq!(decoder("%é1"), "%é1");
    }

    #[test]
    fn la_fenetre_tient_sur_l_ecran() {
        for (l_ecran, h_ecran) in [(3840, 2160), (2560, 1440), (1920, 1080), (1366, 768)] {
            let [l, h] = taille_pour_tout_le_contenu(l_ecran, h_ecran);
            assert!(l <= l_ecran * PART_ECRAN / 100, "déborde en largeur : {l} sur {l_ecran}");
            assert!(h <= h_ecran * PART_ECRAN / 100, "déborde en hauteur : {h} sur {h_ecran}");
            assert!(l > 0 && h > 0, "une taille positive");
        }
    }

    #[test]
    fn le_facteur_de_la_page_est_bien_celui_du_zoom() {
        assert_eq!(PX_PAR_UNITE as f64, cef_popup::CONTENT_ZOOM_FACTOR);
    }

    #[test]
    fn les_preglages_de_la_page_sont_des_adresses_valides() {
        let page = include_str!("../../assets/popups/dns.html");
        let debut = page.find("const PREREGLAGES = [").expect("aucun préréglage");
        let fin = debut + page[debut..].find("];").unwrap();
        let mut n = 0;
        for ligne in page[debut..fin].lines().filter(|l| l.contains("adresses:")) {
            let liste = ligne.split("adresses: [").nth(1).unwrap().split(']').next().unwrap();
            let liste = liste.replace(['"', ' '], "");
            assert!(dns::lire_demande("1", &liste, &[1]).is_some(), "préréglage invalide : {ligne}");
            n += 1;
        }
        assert!(n >= 4, "au moins quatre préréglages, vu {n}");
    }
}
