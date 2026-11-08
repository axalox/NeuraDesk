//! **LA FENÊTRE DES SYMBOLES MATHÉMATIQUES** — six onglets, et un clic qui insère
//! le symbole dans l'autre programme. La table est `win32::maths`.
//!
//! DEMANDE EXPLICITE (2026-10-12) : « une nouvelle applet dans la zone intégrés
//! pour ouvrir une fenêtre permettant d'insérer les caractères mathématiques ».
//!
//! ⭐ **C'EST LA JUMELLE DE `cef_phonetique`**, et elle en suit la facture : une
//! `fenetre: true` avec sa punaise et sa place, ouverte au centre de l'écran
//! PRIMAIRE, et la MÊME cible de frappe (`win32::cible_de_frappe`). ⛔ Les quatre
//! pièges de cette famille sont écrits dans `cef_caracteres` et s'appliquent ici —
//! surtout : sans `render_if_visible`, la fenêtre est INVISIBLE mais prend le focus.
//!
//! Ce qui change : six onglets au lieu de deux, le NOM du symbole sous lui au lieu
//! d'un exemple, et aucune ligne de source.

use std::cell::{Cell, RefCell};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetSystemMetrics, GetWindowRect, SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SM_CXSCREEN,
    SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WM_CLOSE, WM_NCHITTEST,
};

use crate::gfx::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec, PART_ECRAN};
use crate::win32::maths::{self, ONGLETS};

thread_local! {
    static SLOT: PopupSlot = PopupSlot::new();
    /// Le dernier état poussé (voir `cef_caracteres::PAYLOAD`, même garde).
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// La rafale de poussée — la page n'a pas fini de charger à l'ouverture.
    static RAFALE: Cell<u32> = const { Cell::new(0) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskMathsWindow"),
    window_title: w!("NeuraDesk — Symboles mathématiques"),
    html_file: "maths.html",
    wndproc: Some(wndproc),
    above_a_magnified_dock: false,
    // `fades: false` est OBLIGATOIRE pour une fenêtre (voir `cef_kaomoji::SPEC`).
    fades: false,
    fenetre: true,
};

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// Voir `cef_kaomoji::recaler_si_il_ne_tient_plus` — ouverte sur la PRIMAIRE.
pub fn recaler_si_il_ne_tient_plus() {
    cef_popup::recaler_si_il_ne_tient_plus(&SLOT);
}

/// ⛔ **CES CHIFFRES SONT AUSSI ÉCRITS DANS `maths.html`**, et un écart fait une
/// fenêtre trop courte en silence : `.case` (largeur, hauteur), le `gap` de `.grille`,
/// `.titre-groupe` (18 de ligne + 6 de marge), la `margin-bottom` de `.groupe`, la
/// bande de titre et la rangée d'onglets.
const LARGEUR_CASE: i32 = 112;
const HAUTEUR_CASE: i32 = 50;
const ESPACE: i32 = 6;
const HAUTEUR_TITRE: i32 = 24;
const ESPACE_GROUPE: i32 = 10;
const BANDE: i32 = 40;
const HAUTEUR_ONGLETS: i32 = 38;
/// Les colonnes VISÉES — huit cases par ligne tiennent les minuscules grecques en
/// trois lignes.
const COLONNES_CIBLE: i32 = 8;
const MARGE: i32 = 8;
/// La barre de défilement prend sa largeur à la grille (voir `cef_phonetique`).
const DEFILEMENT: i32 = 10;
const SOUPLESSE: i32 = 2;
/// Unités de page → pixels physiques : le zoom `CONTENT_ZOOM_FACTOR` (3×).
const PX_PAR_UNITE: i32 = 3;

/// **LES COLONNES QUE L'ÉCRAN PORTE** — au plus `COLONNES_CIBLE`, au moins deux.
/// Même règle que `cef_phonetique::colonnes`.
fn colonnes(largeur_ecran: i32) -> i32 {
    let budget = (largeur_ecran * PART_ECRAN / 100) / PX_PAR_UNITE - 2 * (MARGE + SOUPLESSE) - DEFILEMENT;
    ((budget + ESPACE) / (LARGEUR_CASE + ESPACE)).clamp(2, COLONNES_CIBLE)
}

/// La hauteur d'un onglet, en unités de page, pour `colonnes` colonnes.
fn hauteur_onglet(onglet: &maths::Onglet, colonnes: i32) -> i32 {
    onglet
        .groupes
        .iter()
        .map(|g| {
            let lignes = (g.symboles.len() as i32 + colonnes - 1) / colonnes;
            HAUTEUR_TITRE + lignes * HAUTEUR_CASE + (lignes - 1).max(0) * ESPACE + ESPACE_GROUPE
        })
        .sum()
}

/// **LA TAILLE DE LA FENÊTRE**, en pixels physiques : la grille de l'onglet le plus
/// haut, plafonnée à `PART_ECRAN` dans les deux sens — la page défile au-delà.
fn taille_pour_tout_le_contenu(largeur_ecran: i32, hauteur_ecran: i32) -> [i32; 2] {
    let n = colonnes(largeur_ecran);
    let largeur = n * LARGEUR_CASE + (n - 1) * ESPACE + 2 * (MARGE + SOUPLESSE) + DEFILEMENT;
    let contenu = ONGLETS.iter().map(|o| hauteur_onglet(o, n)).max().unwrap_or(0);
    let hauteur = BANDE + HAUTEUR_ONGLETS + contenu + 2 * (MARGE + SOUPLESSE);
    let plafond = (hauteur_ecran * PART_ECRAN / 100) / PX_PAR_UNITE;
    [largeur * PX_PAR_UNITE, hauteur.min(plafond) * PX_PAR_UNITE]
}

/// **LE RECTANGLE D'OUVERTURE — AU CENTRE DE L'ÉCRAN PRIMAIRE**.
pub fn rect_pour_ecran_primaire() -> [i32; 4] {
    let (largeur, hauteur) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let [l, h] = cef_popup::taille_aux_textes(largeur, hauteur, taille_pour_tout_le_contenu);
    [((largeur - l) / 2).max(0), ((hauteur - h) / 2).max(0), l, h]
}

/// Ouvre ou referme la fenêtre. Appelée par le bras de clic de l'applet.
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice) {
    // Déjà ouverte : la remonter au premier plan si elle est enterrée, la cacher
    // si elle y est déjà (voir `cef_popup::basculer_ou_cacher_si_ouvert`).
    if cef_popup::basculer_ou_cacher_si_ouvert(&SLOT) {
        return;
    }
    crate::diag!("[maths] ouverture : rect {rect:?} (x,y,largeur,hauteur)");
    PAYLOAD.with(|p| *p.borrow_mut() = None);
    // Le zoom de la page suit la taille que `rect_pour_ecran_primaire` a donnée.
    cef_popup::regler_zoom_textes(&SLOT, crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES), rect);
    cef_popup::montrer_fenetre(&SLOT, &SPEC, rect, gfx);
    RAFALE.with(|r| r.set(6));
    push_state();
}

/// **LE DESSIN — SANS LUI, LA FENÊTRE EST INVISIBLE.** ⛔ Et surtout PAS
/// `resize_if_needed` (voir `cef_caracteres`).
pub fn render_if_visible(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

/// La copie du back buffer pour le harnais (voir `cef_kaomoji::debug_dump_backbuffer`).
pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// L'état de la page : les onglets, leurs groupes et leurs symboles. ⚠️ **LA TABLE
/// VIENT D'ICI, PAS DE LA PAGE** — une seconde copie dans `maths.html` divergerait
/// au premier symbole ajouté.
fn etat_json() -> String {
    let onglets: Vec<serde_json::Value> = ONGLETS
        .iter()
        .map(|o| {
            let groupes: Vec<serde_json::Value> = o
                .groupes
                .iter()
                .map(|g| {
                    let symboles: Vec<serde_json::Value> =
                        g.symboles.iter().map(|e| serde_json::json!({ "s": e.symbole, "nom": e.nom })).collect();
                    serde_json::json!({ "nom": g.nom, "symboles": symboles })
                })
                .collect();
            serde_json::json!({ "nom": o.nom, "groupes": groupes })
        })
        .collect();
    serde_json::json!({ "onglets": onglets, "epingle": crate::config::load_maths_premier_plan() }).to_string()
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
    let rang = if crate::config::load_maths_premier_plan() { Some(HWND_TOPMOST) } else { Some(HWND_NOTOPMOST) };
    unsafe {
        let _ = SetWindowPos(hwnd, rang, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// **CE QUE LA PAGE DEMANDE** : les trois gestes de la fenêtre, les deux de la grille.
pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("maths/") else { continue };
        let mut morceaux = reste.split('/');
        let (Some(verbe), index) = (morceaux.next(), morceaux.next()) else { continue };
        match verbe {
            "deplacer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::deplacer_par_le_systeme(hwnd, "maths");
                }
            }
            "fermer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::cacher_fenetre(hwnd);
                }
            }
            "epingler" => {
                let actif = !crate::config::load_maths_premier_plan();
                crate::config::save_maths_premier_plan(actif);
                crate::diag!("[maths] premier plan = {actif}");
                appliquer_le_premier_plan();
                push_state();
            }
            "frapper" | "copier" => {
                // ⚠️ **L'INDEX, PAS LE SYMBOLE** — même raison que les kaomojis : un
                // symbole dans une URL demanderait un échappement, et une erreur
                // d'échappement serait SILENCIEUSE.
                let Some(index) = index.and_then(|i| i.parse::<usize>().ok()) else {
                    crate::diag!("[maths] index illisible");
                    continue;
                };
                let Some(symbole) = symbole_par_index(index) else {
                    crate::diag!("[maths] index hors table : {index}");
                    continue;
                };
                if verbe == "frapper" {
                    crate::win32::cible_de_frappe::frapper_dans_la_cible(symbole);
                } else {
                    crate::win32::clipboard::write_text(symbole);
                    crate::diag!("[maths] copie : {symbole:?}");
                }
            }
            autre => crate::diag!("[maths] verbe inconnu : {autre:?}"),
        }
    }
}

/// **LE SYMBOLE D'UN INDEX GLOBAL** — onglets, puis groupes, dans l'ordre de la page.
fn symbole_par_index(index: usize) -> Option<&'static str> {
    maths::entrees().nth(index).map(|e| e.symbole)
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

/// Le code de `WM_NCHITTEST` : les huit bords et coins, sinon le client. ⚠️ La
/// SIXIÈME copie (voir `cef_kaomoji::code_pour_point`, qui dit pourquoi elle
/// n'est pas encore factorisée).
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

    fn verbes_connus() -> [&'static str; 5] {
        ["frapper", "copier", "deplacer", "epingler", "fermer"]
    }

    /// ⭐ **LA PAGE ET RUST PARLENT LE MÊME LANGAGE** — même contrat que
    /// `cef_phonetique` : l'URL entière pour la grille, `go("…")` pour la fenêtre.
    #[test]
    fn les_verbes_de_la_page_sont_exactement_ceux_du_dispatch() {
        let page = include_str!("../../assets/popups/maths.html");
        const PREFIXE: &str = "neuradesk://maths/";
        let mut vus: Vec<&str> = Vec::new();
        for (i, _) in page.match_indices(PREFIXE) {
            let reste = &page[i + PREFIXE.len()..];
            let verbe = reste.split('/').next().unwrap_or("");
            if verbes_connus().contains(&verbe) {
                vus.push(verbe);
            } else {
                assert!(reste.starts_with("\" +"), "verbe inconnu dans la page : {verbe:?}");
            }
        }
        for morceau in page.split("go(\"").skip(1) {
            let verbe = morceau.split('"').next().unwrap_or("");
            assert!(verbes_connus().contains(&verbe), "verbe inconnu passé à `go` : {verbe:?}");
            vus.push(verbe);
        }
        for attendu in verbes_connus() {
            assert!(vus.contains(&attendu), "la page n'appelle jamais `{attendu}`");
        }
    }

    /// **L'INDEX GLOBAL DÉSIGNE EXACTEMENT CE QUE LA PAGE AFFICHE AU MÊME RANG** —
    /// onglets puis groupes, dans l'ordre du JSON. Sinon un clic écrirait le symbole
    /// du voisin, ce qui ne se verrait qu'à l'usage.
    #[test]
    fn l_etat_porte_les_six_onglets_et_l_index_designe_le_bon() {
        let json: serde_json::Value = serde_json::from_str(&etat_json()).expect("un JSON valide");
        let onglets = json["onglets"].as_array().expect("un tableau d'onglets");
        let noms: Vec<&str> = onglets.iter().filter_map(|o| o["nom"].as_str()).collect();
        assert_eq!(noms, ["Opérateurs", "Relations", "Ensembles", "Grec", "Exposants", "Divers"]);
        let mut rang = 0;
        for onglet in onglets {
            for groupe in onglet["groupes"].as_array().expect("des groupes") {
                for symbole in groupe["symboles"].as_array().expect("des symboles") {
                    assert_eq!(symbole_par_index(rang), symbole["s"].as_str(), "l'index {rang} ne désigne pas le bon");
                    rang += 1;
                }
            }
        }
        assert_eq!(rang, maths::entrees().count());
        assert_eq!(symbole_par_index(rang), None, "au-delà de la table, on le DIT");
    }

    /// ⭐ **LA FENÊTRE TIENT SUR L'ÉCRAN** — en largeur comme en hauteur — et sur un
    /// grand écran elle a ses huit colonnes.
    #[test]
    fn la_fenetre_tient_sur_l_ecran() {
        for (l_ecran, h_ecran) in [(3840, 2160), (2560, 1440), (1920, 1080), (1366, 768)] {
            let [l, h] = taille_pour_tout_le_contenu(l_ecran, h_ecran);
            assert!(l <= l_ecran * PART_ECRAN / 100, "déborde en largeur : {l} sur {l_ecran}");
            assert!(h <= h_ecran * PART_ECRAN / 100, "déborde en hauteur : {h} sur {h_ecran}");
            assert!(h > (BANDE + HAUTEUR_ONGLETS) * PX_PAR_UNITE, "et elle a sa bande et ses onglets");
        }
        assert_eq!(colonnes(3840), COLONNES_CIBLE);
        assert_eq!(colonnes(1920), 4);
    }

    /// La hauteur d'un onglet compte ses titres et ses lignes — le grec, à huit
    /// colonnes : 24 minuscules (3 lignes), 6 variantes (1), 11 majuscules (2).
    #[test]
    fn la_hauteur_d_un_onglet_compte_titres_et_lignes() {
        let lignes = 3 + 1 + 2;
        let espaces_internes = (2 + 0 + 1) * ESPACE;
        let attendu = 3 * (HAUTEUR_TITRE + ESPACE_GROUPE) + lignes * HAUTEUR_CASE + espaces_internes;
        assert_eq!(hauteur_onglet(&ONGLETS[3], 8), attendu);
    }

    #[test]
    fn le_facteur_de_la_page_est_bien_celui_du_zoom() {
        assert_eq!(PX_PAR_UNITE as f64, cef_popup::CONTENT_ZOOM_FACTOR);
    }

    /// Les chiffres de la géométrie sont ceux de la page — un écart y ferait une
    /// fenêtre trop courte en silence.
    #[test]
    fn la_page_porte_les_memes_chiffres() {
        let page = include_str!("../../assets/popups/maths.html");
        for attendu in [
            format!("width: {LARGEUR_CASE}px"),
            format!("height: {HAUTEUR_CASE}px"),
            format!("gap: {ESPACE}px"),
            format!("margin-bottom: {ESPACE_GROUPE}px"),
            format!("repeat(auto-fill, {LARGEUR_CASE}px)"),
        ] {
            assert!(page.contains(&attendu), "la page ne porte pas « {attendu} »");
        }
        let style = include_str!("../../assets/popups/style.css");
        assert!(page.contains(r#"href="style.css""#));
        assert!(style.contains(&format!("\n::-webkit-scrollbar {{ width: {DEFILEMENT}px")));
    }

    #[test]
    fn les_huit_bords_sont_reconnus() {
        let rect = [100, 200, 400, 300];
        assert_eq!(code_pour_point(rect, (101, 201)), 13, "coin haut-gauche");
        assert_eq!(code_pour_point(rect, (499, 499)), 17, "coin bas-droit");
        assert_eq!(code_pour_point(rect, (300, 350)), 1, "le CLIENT, pas un bord");
    }
}
