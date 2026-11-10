//! **LA FENÊTRE DU JEU 2048** — un fork web du jeu GNOME 2048, dans la zone Intégrés.
//!
//! DEMANDE (2026-11-07) : « dans Intégrés avoir des applets pour un fork de gnome
//! 2048 et un fork de five-or-more ».
//!
//! ⭐ **C'EST LA JUMELLE DE `cef_maths`**, et elle en suit la facture : une
//! `fenetre: true` avec sa punaise et sa place, ouverte au centre de l'écran
//! PRIMAIRE. ⛔ Les pièges de cette famille sont écrits dans `cef_caracteres` et
//! s'appliquent — surtout : sans `render_if_visible`, la fenêtre est INVISIBLE mais
//! prend le focus.
//!
//! Ce qui change : **le jeu vit entièrement dans `jeu2048.html`** (logique, score,
//! record dans `localStorage`). Rust n'a que la chrome de fenêtre : glisser la bande
//! de titre, épingler, fermer. Aucune table ni index à relire.

use std::cell::{Cell, RefCell};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetSystemMetrics, GetWindowRect, SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SM_CXSCREEN,
    SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WM_CLOSE, WM_NCHITTEST,
};

use crate::gfx::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec, PART_ECRAN};

thread_local! {
    static SLOT: PopupSlot = PopupSlot::new();
    /// Le dernier état poussé (voir `cef_maths::PAYLOAD`, même garde).
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// La rafale de poussée — la page n'a pas fini de charger à l'ouverture.
    static RAFALE: Cell<u32> = const { Cell::new(0) };
}

/// Le réglage « rester au premier plan » de CETTE fenêtre (une `fenetre: true`
/// porte le sien, comme `cef_maths`).
const PREMIER_PLAN: &str = "jeu2048_premier_plan";

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskJeu2048Window"),
    window_title: w!("NeuraDesk — 2048"),
    html_file: "jeu2048.html",
    wndproc: Some(wndproc),
    above_a_magnified_dock: false,
    // `fades: false` est OBLIGATOIRE pour une fenêtre (voir `cef_maths::SPEC`).
    fades: false,
    fenetre: true,
};

/// Unités de page → pixels physiques : le zoom `CONTENT_ZOOM_FACTOR` (3×).
const PX_PAR_UNITE: i32 = 3;
/// La taille VISÉE du jeu, en unités de page — un plateau carré plus la bande de
/// titre, la tête des scores et la ligne d'aide. Plafonnée à `PART_ECRAN`.
const LARGEUR_JEU: i32 = 300;
const HAUTEUR_JEU: i32 = 430;

/// La taille de la fenêtre en pixels physiques, plafonnée à `PART_ECRAN` dans les
/// deux sens.
fn taille_pour_tout_le_contenu(largeur_ecran: i32, hauteur_ecran: i32) -> [i32; 2] {
    let plafond_l = (largeur_ecran * PART_ECRAN / 100) / PX_PAR_UNITE;
    let plafond_h = (hauteur_ecran * PART_ECRAN / 100) / PX_PAR_UNITE;
    [LARGEUR_JEU.min(plafond_l) * PX_PAR_UNITE, HAUTEUR_JEU.min(plafond_h) * PX_PAR_UNITE]
}

/// **LE RECTANGLE D'OUVERTURE — AU CENTRE DE L'ÉCRAN PRIMAIRE** (voir `cef_maths`).
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
    // Déjà ouverte : la remonter au premier plan si elle est enterrée, la cacher
    // si elle y est déjà (voir `cef_popup::basculer_ou_cacher_si_ouvert`).
    if cef_popup::basculer_ou_cacher_si_ouvert(&SLOT) {
        return;
    }
    crate::diag!("[jeu2048] ouverture : rect {rect:?} (x,y,largeur,hauteur)");
    PAYLOAD.with(|p| *p.borrow_mut() = None);
    cef_popup::regler_zoom_textes(&SLOT, crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES), rect);
    cef_popup::montrer_fenetre(&SLOT, &SPEC, rect, gfx);
    RAFALE.with(|r| r.set(6));
    push_state();
}

/// **LE DESSIN — SANS LUI, LA FENÊTRE EST INVISIBLE** (voir `cef_maths`).
pub fn render_if_visible(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

/// La copie du back buffer pour le harnais (voir `cef_maths::debug_dump_backbuffer`).
pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// L'état de fenêtre : seulement la punaise (le jeu lui-même vit dans la page).
fn etat_json() -> String {
    serde_json::json!({ "epingle": crate::config::load_bool_setting_or(PREMIER_PLAN, false) }).to_string()
}

/// Repousse l'état si la page ne l'a pas encore reçu (voir `RAFALE`).
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

/// **À APPELER À CHAQUE FRAME** — entretient la rafale et applique le premier plan.
pub fn refresh_if_visible() {
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

/// La punaise — le réglage est le nôtre (une `fenetre: true` porte la sienne).
fn appliquer_le_premier_plan() {
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return };
    let rang = if crate::config::load_bool_setting_or(PREMIER_PLAN, false) { Some(HWND_TOPMOST) } else { Some(HWND_NOTOPMOST) };
    unsafe {
        let _ = SetWindowPos(hwnd, rang, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// **CE QUE LA PAGE DEMANDE** : les trois gestes de la fenêtre, rien d'autre.
pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(verbe) = action.strip_prefix("jeu2048/") else { continue };
        match verbe {
            "deplacer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::deplacer_par_le_systeme(hwnd, "jeu2048");
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
                crate::diag!("[jeu2048] premier plan = {actif}");
                appliquer_le_premier_plan();
                push_state();
            }
            autre => crate::diag!("[jeu2048] verbe inconnu : {autre:?}"),
        }
    }
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

/// Le code de `WM_NCHITTEST` : les huit bords et coins, sinon le client (voir
/// `cef_maths::code_pour_point`).
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

    fn verbes_connus() -> [&'static str; 3] {
        ["deplacer", "epingler", "fermer"]
    }

    /// ⭐ **LA PAGE ET RUST PARLENT LE MÊME LANGAGE** — les seuls verbes de la page
    /// sont les trois gestes de fenêtre ; le jeu lui-même n'émet rien vers Rust.
    #[test]
    fn les_verbes_de_la_page_sont_exactement_ceux_du_dispatch() {
        let page = include_str!("../../assets/popups/jeu2048.html");
        const PREFIXE: &str = "neuradesk://jeu2048/";
        for (i, _) in page.match_indices(PREFIXE) {
            let reste = &page[i + PREFIXE.len()..];
            let verbe = reste.split('/').next().unwrap_or("");
            if !verbes_connus().contains(&verbe) {
                assert!(reste.starts_with("\" +"), "verbe inconnu dans la page : {verbe:?}");
            }
        }
        let mut vus: Vec<&str> = Vec::new();
        for morceau in page.split("go(\"").skip(1) {
            let verbe = morceau.split('"').next().unwrap_or("");
            assert!(verbes_connus().contains(&verbe), "verbe inconnu passé à `go` : {verbe:?}");
            vus.push(verbe);
        }
        for attendu in verbes_connus() {
            assert!(vus.contains(&attendu), "la page n'appelle jamais `{attendu}`");
        }
    }

    /// ⭐ **LA FENÊTRE TIENT SUR L'ÉCRAN** — en largeur comme en hauteur.
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
    fn les_huit_bords_sont_reconnus() {
        let rect = [100, 200, 400, 300];
        assert_eq!(code_pour_point(rect, (101, 201)), 13, "coin haut-gauche");
        assert_eq!(code_pour_point(rect, (499, 499)), 17, "coin bas-droit");
        assert_eq!(code_pour_point(rect, (300, 350)), 1, "le CLIENT, pas un bord");
    }

    /// ⭐ **« ANNULER » EST CÂBLÉ DE BOUT EN BOUT DANS LA PAGE** — le bouton, son
    /// écouteur et la fonction qu'il appelle. La règle, elle, se vérifie sans CEF
    /// par `C:/ndwork/scripts/essai_2048.js` (19 vérifications).
    ///
    /// ⚠️ **LE BOUTON VIT DANS LA BARRE, PAS DANS L'EN-TÊTE** : ajouté en texte à
    /// côté de « Nouvelle partie », il poussait celle-ci HORS de la fenêtre (capture
    /// du 2026-11-10, `jeu2048_0400.png` — l'en-tête débordait déjà à la taille de
    /// texte de l'utilisateur). Une icône de 24 px dans la barre ne coûte rien.
    #[test]
    fn la_page_porte_le_bouton_annuler_et_son_ecouteur() {
        let page = include_str!("../../assets/popups/jeu2048.html");
        assert!(page.contains(r#"<button id="annuler" class="bouton" disabled"#), "aucun bouton Annuler dans la barre, ou allumé au départ");
        assert!(page.contains(r#"document.getElementById("annuler").addEventListener("click", annuler)"#), "le bouton n'est pas branché");
        assert!(page.contains("function annuler()"), "aucune fonction `annuler`");
        assert!(page.contains("historique.push(avant)"), "aucun coup n'entre dans l'historique");
        // La règle d'or de l'en-tête : il ne reçoit AUCUN contrôle de plus.
        let tete = &page[page.find(r#"<div id="tete">"#).expect("en-tête absent")..];
        let tete = &tete[..tete.find("</div>\n  <div id=\"scene\">").unwrap_or(tete.len())];
        assert_eq!(tete.matches("<button").count(), 1, "l'en-tête doit garder SES DEUX pastilles et UN bouton : {tete}");
    }
}
