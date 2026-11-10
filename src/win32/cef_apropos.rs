//! **LA FENÊTRE « À PROPOS DE NEURADESK »** — `assets/popups/apropos.html`.
//!
//! DEMANDE (2026-11-08) : « une entrée about dans le menu du clic droit sur
//! l'icône NeuraDesk avec des explications, le logo et le numéro de version ».
//!
//! Ouverte par le bouton « À propos de NeuraDesk » du menu de l'icône
//! (`assets/popups/context.html`, action `app/about`, voir `app::run`).
//!
//! ⭐ **LA JUMELLE DES FENÊTRES DE JEU** (`cef_quadrapassel`) : une `fenetre: true`
//! au centre de l'écran PRIMAIRE, avec sa bande de titre qui la déplace et sa
//! croix — pas un panneau de la zone libre, donc aucun contrat de voisinage.
//!
//! Le seul état poussé est la VERSION : `CARGO_PKG_VERSION`, et la révision que
//! `build.rs` (`emettre_la_revision`) tire de `git` au moment du build.

use std::cell::{Cell, RefCell};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetSystemMetrics, GetWindowRect, SM_CXSCREEN, SM_CYSCREEN, WM_CLOSE, WM_NCHITTEST,
};

use crate::gfx::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec, PART_ECRAN};

thread_local! {
    static SLOT: PopupSlot = PopupSlot::new();
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Quelques poussées après l'ouverture : la page peut ne pas avoir encore
    /// défini `window.setState` (même raison que `cef_quadrapassel::RAFALE`).
    static RAFALE: Cell<u32> = const { Cell::new(0) };
    /// Le numéro d'état de `win32::maj` déjà poussé — voir `refresh_if_visible`.
    /// `u64::MAX` = « rien encore », pour forcer la première poussée.
    static DERNIERE_GENERATION_MAJ: Cell<u64> = const { Cell::new(u64::MAX) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskAProposWindow"),
    window_title: w!("À propos de NeuraDesk"),
    html_file: "apropos.html",
    wndproc: Some(wndproc),
    above_a_magnified_dock: false,
    // `fades: false` est OBLIGATOIRE pour une fenêtre (voir `cef_maths::SPEC`).
    fades: false,
    fenetre: true,
};

/// Unités de page → pixels physiques : le zoom `CONTENT_ZOOM_FACTOR` (3×).
const PX_PAR_UNITE: i32 = 3;
/// La taille VISÉE, en unités de page : le logo, la version, la mise à jour et
/// le texte sans ascenseur à la taille de texte par défaut. Plafonnée à
/// `PART_ECRAN`.
const LARGEUR: i32 = 440;
const HAUTEUR: i32 = 860;

fn taille_pour_tout_le_contenu(largeur_ecran: i32, hauteur_ecran: i32) -> [i32; 2] {
    let plafond_l = (largeur_ecran * PART_ECRAN / 100) / PX_PAR_UNITE;
    let plafond_h = (hauteur_ecran * PART_ECRAN / 100) / PX_PAR_UNITE;
    [LARGEUR.min(plafond_l) * PX_PAR_UNITE, HAUTEUR.min(plafond_h) * PX_PAR_UNITE]
}

/// Le rectangle d'ouverture : au centre de l'écran PRIMAIRE.
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

/// Ouvre la fenêtre — ou la ramène devant / la cache si elle l'est déjà (voir
/// `cef_popup::basculer_ou_cacher_si_ouvert`).
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice) {
    if cef_popup::basculer_ou_cacher_si_ouvert(&SLOT) {
        return;
    }
    PAYLOAD.with(|p| *p.borrow_mut() = None);
    DERNIERE_GENERATION_MAJ.with(|g| g.set(u64::MAX));
    cef_popup::regler_zoom_textes(&SLOT, crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES), rect);
    cef_popup::montrer_fenetre(&SLOT, &SPEC, rect, gfx);
    RAFALE.with(|r| r.set(6));
    // Une ouverture rafraîchit l'état des mises à jour, mais pas plus d'une
    // fois toutes les cinq minutes (le module tient l'anti-rebond).
    crate::win32::maj::verifier(false);
    push_state();
}

/// La fenêtre est-elle affichée ? — pour l'entrée « Rechercher les mises à
/// jour » du menu, qui doit la RAMENER sans la refermer (`toggle` bascule).
pub fn est_visible() -> bool {
    cef_popup::is_visible(&SLOT)
}

/// La RAMÈNE DEVANT sans la cacher — voir `est_visible`.
pub fn ramener_devant() {
    if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
        cef_popup::force_foreground(hwnd);
    }
}

pub fn render_if_visible(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// La version affichée : celle du paquet, plus la révision du build.
fn etat_json() -> String {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "revision": env!("NEURADESK_REVISION"),
        "date": env!("NEURADESK_DATE_REVISION"),
        "publication": env!("NEURADESK_PUBLICATION"),
        // La mise à jour a son propre sous-objet : elle change sans que la
        // version compilée change, et la page n'a pas à les mélanger.
        "maj": crate::win32::maj::etat_json(),
    })
    .to_string()
}

fn push_state() {
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
    if !cef_popup::is_visible(&SLOT) {
        return;
    }
    // ⚠️ **LA MISE À JOUR CHANGE SANS GESTE DE L'UTILISATEUR** (téléchargement,
    // extraction, vérification silencieuse) : ce numéro est ce qui fait pousser
    // l'état à la page même quand `RAFALE` est épuisée. Sans lui, la barre de
    // progression resterait figée jusqu'au prochain clic.
    let generation = crate::win32::maj::generation();
    let maj_changee = DERNIERE_GENERATION_MAJ.with(|g| {
        if g.get() == generation {
            false
        } else {
            g.set(generation);
            true
        }
    });
    let restante = RAFALE.with(|r| r.get());
    if restante > 0 {
        RAFALE.with(|r| r.set(restante - 1));
        PAYLOAD.with(|p| *p.borrow_mut() = None);
        push_state();
    } else if maj_changee {
        push_state();
    }
}

/// **CE QUE LA PAGE DEMANDE** : déplacer, fermer, ouvrir l'aide.
pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(verbe) = action.strip_prefix("apropos/") else { continue };
        match verbe {
            "deplacer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::deplacer_par_le_systeme(hwnd, "apropos");
                }
            }
            "fermer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::cacher_fenetre(hwnd);
                }
            }
            "aide" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::cacher_fenetre(hwnd);
                }
                // Ouverte EN HAUT (ancre vide) à la prochaine frame par `app::run`.
                crate::win32::cef_aide::demander("");
            }
            // **LES QUATRE GESTES DE LA MISE À JOUR** (voir `win32::maj`) — la
            // page ne connaît que ces verbes, et c'est Rust qui décide de ce
            // qu'ils engagent.
            "maj_verifier" => crate::win32::maj::verifier(true),
            "maj_installer" => crate::win32::maj::installer(),
            "maj_annuler" => crate::win32::maj::annuler(),
            "maj_redemarrer" => crate::win32::maj::redemarrer_pour_installer(),
            autre => crate::diag!("[apropos] verbe inconnu : {autre:?}"),
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

/// Les bords redimensionnent (codes `HT*`), l'intérieur reste le client.
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

    const VERBES: [&str; 7] = [
        "deplacer",
        "fermer",
        "aide",
        "maj_verifier",
        "maj_installer",
        "maj_annuler",
        "maj_redemarrer",
    ];

    #[test]
    fn les_verbes_de_la_page_sont_exactement_ceux_du_dispatch() {
        let page = include_str!("../../assets/popups/apropos.html");
        let mut vus: Vec<&str> = Vec::new();
        for morceau in page.split("go(\"").skip(1) {
            let verbe = morceau.split('"').next().unwrap_or("");
            assert!(VERBES.contains(&verbe), "verbe inconnu passé à `go` : {verbe:?}");
            vus.push(verbe);
        }
        for attendu in VERBES {
            assert!(vus.contains(&attendu), "la page n'appelle jamais `{attendu}`");
        }
        assert!(page.contains("\"neuradesk://apropos/\""), "le préfixe des verbes a changé");
    }

    #[test]
    fn la_page_affiche_le_logo_et_les_quatre_champs_de_version() {
        let page = include_str!("../../assets/popups/apropos.html");
        assert!(page.contains("src=\"logo_neuradesk.png\""));
        for champ in ["version", "revision", "date", "publication"] {
            assert!(page.contains(&format!("state.{champ}")), "la page n'affiche pas `{champ}`");
        }
    }

    #[test]
    fn la_page_porte_la_section_de_mise_a_jour() {
        let page = include_str!("../../assets/popups/apropos.html");
        for id in ["maj-etat", "maj-verifier", "maj-installer", "maj-annuler", "maj-redemarrer"] {
            assert!(page.contains(&format!("id=\"{id}\"")), "la section de mise à jour n'a pas `{id}`");
        }
        assert!(page.contains("state.maj"), "la page n'affiche pas l'état `maj` poussé par Rust");
        assert!(page.contains("progress"), "aucune barre de progression");
    }

    #[test]
    fn l_etat_porte_la_version_du_paquet() {
        let etat: serde_json::Value = serde_json::from_str(&etat_json()).expect("JSON valide");
        assert_eq!(etat["version"], env!("CARGO_PKG_VERSION"));
        assert!(etat["revision"].as_str().is_some_and(|r| !r.is_empty()));
    }

    #[test]
    fn la_fenetre_tient_sur_l_ecran() {
        for (l_ecran, h_ecran) in [(3840, 2160), (2560, 1440), (1920, 1080), (1366, 768)] {
            let [l, h] = taille_pour_tout_le_contenu(l_ecran, h_ecran);
            assert!(l <= l_ecran * PART_ECRAN / 100 && h <= h_ecran * PART_ECRAN / 100);
            assert!(l > 0 && h > 0);
        }
    }

    #[test]
    fn le_facteur_de_la_page_est_bien_celui_du_zoom() {
        assert_eq!(PX_PAR_UNITE as f64, cef_popup::CONTENT_ZOOM_FACTOR);
    }
}
