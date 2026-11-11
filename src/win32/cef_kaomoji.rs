//! **LA FENÊTRE DES KAOMOJIS** — sœur de `win32::cef_caracteres`, et délibérément
//! SÉPARÉE : l'utilisateur a voulu pouvoir garder les deux ouvertes en même temps
//! (« oui, deux fenêtres », 2026-10-06), donc deux modules, deux pages, deux
//! punaises, deux places.
//!
//! ⛔ **LES QUATRE PIÈGES DE CETTE FAMILLE DE FENÊTRES SONT DANS
//! `cef_caracteres`, ET ILS S'APPLIQUENT TOUS ICI** — les recopier aurait fait
//! diverger deux textes identiques. En résumé, et ils ont chacun coûté cher :
//!   1. sans `cef_popup::render_if_visible`, la fenêtre est INVISIBLE mais prend le
//!      focus ;
//!   2. `resize_if_needed` à chaque frame annule le redimensionnement ;
//!   3. une `fenetre: true` ne reçoit pas la punaise injectée (elle porte la sienne) ;
//!   4. PRÉCHARGÉE, son `rect` d'ouverture est jeté — il s'applique à la PREMIÈRE
//!      MONTE (voir `cef_popup::montrer_fenetre`).
//!
//! ⚠️ **CE QUI CHANGE VRAIMENT ICI, C'EST LA GÉOMÉTRIE.** Une case de caractère est
//! carrée et toutes les cases sont identiques ; un kaomoji fait de deux à vingt
//! unités de large. La fenêtre ne peut donc pas se dimensionner sur « N colonnes de
//! 50 » — elle se dimensionne sur **le kaomoji le PLUS LARGE**, et la page EMBALLE
//! les familles sur plusieurs lignes. C'est pour ça que la hauteur, elle, est
//! ESTIMÉE : 50 kaomojis ne tiennent pas sur une grille de 7 rangées.

use std::cell::{Cell, RefCell};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetSystemMetrics, GetWindowRect, SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SM_CXSCREEN,
    SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WM_CLOSE, WM_NCHITTEST,
};

use crate::gfx::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec, PART_ECRAN};

thread_local! {
    static SLOT: PopupSlot = PopupSlot::new();
    /// Le dernier état poussé (voir `cef_caracteres::PAYLOAD`, même garde).
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// La rafale de poussée — la page n'a pas fini de charger à l'ouverture.
    static RAFALE: Cell<u32> = const { Cell::new(0) };
    // ⛔ **IL Y AVAIT ICI UN `static RECT`, ET IL ÉTAIT MORT** — jumeau de celui de
    // `cef_caracteres`, mort pour la même raison (écrit à l'ouverture, jamais lu),
    // et les deux se citaient l'un l'autre comme justification. Voir
    // `cef_caracteres`, où la démonstration est écrite.
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskKaomojiWindow"),
    window_title: w!("NeuraDesk — Kaomojis"),
    html_file: "kaomoji.html",
    wndproc: Some(wndproc),
    above_a_magnified_dock: false,
    // `fades: false` est OBLIGATOIRE pour une fenêtre — un fondu demande
    // `WS_EX_LAYERED`, qui REMPLACE le comportement d'une vraie fenêtre.
    fades: false,
    fenetre: true,
};

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// Voir `cef_popup::recaler_si_il_ne_tient_plus` — même raison que
/// `cef_caracteres::recaler_si_il_ne_tient_plus` : ouverte sur la PRIMAIRE, donc
/// un changement d'écran la laisserait sur un rectangle périmé.
pub fn recaler_si_il_ne_tient_plus() {
    cef_popup::recaler_si_il_ne_tient_plus(&SLOT);
}

/// Hauteur d'une case, en unités de page. Plus courte que celle d'un caractère
/// (50) : un kaomoji se lit, il ne se vise pas — et 50 entrées à 50 unités feraient
/// une fenêtre plus haute que l'écran.
const HAUTEUR_CASE: i32 = 36;
/// **UNE LIGNE D'ONGLETS**, en unités de page : le bouton `.onglet` (24) et l'écart
/// entre deux lignes (`gap` de `#onglets`, 4). Voir `hauteur_des_onglets`.
const HAUTEUR_ONGLET: i32 = 28;
/// L'écart horizontal entre deux onglets — le `gap` de `#onglets`.
const ESPACE_ONGLET: i32 = 4;
/// **LE NOM DE L'ONGLET PERSONNEL** — écrit aussi dans `kaomoji.html`, où il
/// n'apparaît que si `state.personnels` est non vide. Un test compare les deux.
const FAMILLE_PERSO: &str = "Mes kaomojis";
/// Ce que la rangée d'onglets ajoute autour de ses lignes : `padding` 6 + 6, le
/// filet du bas, moins l'écart de la dernière ligne qui n'en a pas.
const MARGE_ONGLETS: i32 = 9;
/// La largeur moyenne d'un caractère du nom d'un onglet (police de 12 px), et le
/// `padding` horizontal du bouton (10 + 10) — une ESTIMATION, comme
/// `largeur_kaomoji`, prise large pour ne pas sous-compter les lignes : à 7, le
/// calcul donnait 4 lignes là où Edge en rend 5 (MESURÉ, 2026-10-23).
const LARGEUR_CAR_ONGLET: i32 = 8;
const PADDING_ONGLET: i32 = 20;
/// **LA LARGEUR VISÉE DE LA FENÊTRE**, en unités de page — la page emballe les
/// familles dans cette largeur. C'est un CHOIX, pas un calcul : quatre kaomojis par
/// ligne est le compromis qui donne une fenêtre plus large que haute.
const LARGEUR_CIBLE: i32 = 720;
/// La largeur d'une unité de demi-chasse, en unités de page. Un caractère pleine
/// chasse (CJK, `＾`, `□`) en vaut DEUX — c'est ainsi que les rend la police, et
/// compter chaque caractère pour un seul sous-estimerait les kaomojis japonais.
const LARGEUR_UNITE: i32 = 11;
/// La bande de titre (boutons de 24 px, `padding` 6 px, filet) — même rôle que
/// `cef_caracteres::BANDE`.
const BANDE: i32 = 40;
const MARGE: i32 = 8;
const SOUPLESSE: i32 = 2;
// ⚠️ **`PART_ECRAN` N'EST PLUS DÉFINI ICI** — sa définition, et tout le pourquoi
// du chiffre, vivent dans `cef_popup` (voir son import ci-dessus). Il était
// recopié dans ce fichier ET dans `cef_caracteres`, sans aucun garde : changer
// l'un laissait les tests de l'autre verts et les deux commentaires faux.
/// **UNITÉS DE PAGE → PIXELS PHYSIQUES** — le zoom `CONTENT_ZOOM_FACTOR` (3×), et
/// rien d'autre. Un test le compare à la constante de `cef_popup`.
const PX_PAR_UNITE: i32 = 3;
/// L'espace horizontal entre deux cases, en unités de page — il doit être le même
/// dans la page (`gap` de `.rang`) et ici, sinon l'emballage calculé ment.
const ESPACE: i32 = 8;
/// La largeur d'une case : le texte, ses marges, et un peu de respiration.
const PADDING_CASE: i32 = 16;

/// **LA LARGEUR D'UN KAOMOJI**, en unités de page — pleine chasse comptée double.
///
/// ⚠️ **C'EST UNE ESTIMATION, ET ELLE EST ASSUMÉE** : Rust ne mesure pas de texte.
/// Elle sert à EMBALLER (combien de cases par ligne) et à dimensionner la fenêtre ;
/// si elle se trompe d'un rien, la page emballe un peu autrement, et la barre de
/// défilement — qui existe — rattrape. Ce qui serait grave serait de la sous-estimer
/// franchement, pas de la rater de quelques unités.
fn largeur_kaomoji(k: &str) -> i32 {
    let unites: i32 = k.chars().map(|c| if (c as u32) >= 0x1100 { 2 } else { 1 }).sum();
    unites * LARGEUR_UNITE + PADDING_CASE
}

/// **LA LARGEUR D'EMBALLAGE, EN UNITÉS DE PAGE** — la plus étroite de
/// `LARGEUR_CIBLE` (le choix) et de ce que l'écran peut porter.
///
/// ⛔ **LA LARGEUR N'ÉTAIT BORNÉE PAR RIEN.** Elle valait `LARGEUR_CIBLE` quelle
/// que soit la machine, donc 2220 px : sur un 1920, `centrer_sur_ecran` la posait
/// à `x = 0` et **300 px sortaient à droite** — le `✕` de la bande de titre
/// INATTEIGNABLE, sans que rien ne le dise.
///
/// ⭐ **C'EST AUSSI LA LARGEUR D'EMBALLAGE**, et pas seulement celle de la
/// fenêtre : la page enroule ses rangées à la largeur qu'on lui donne. Calculer
/// la hauteur à `LARGEUR_CIBLE` tout en posant une fenêtre plus étroite donnerait
/// une fenêtre trop COURTE — le défilement cacherait les derniers groupes, soit
/// exactement le défaut qu'on répare.
fn largeur_unites(largeur_ecran: i32) -> i32 {
    // ⚠️ **LA MARGE EST RETIRÉE DU BUDGET AVANT LA DIVISION** : c'est la fenêtre
    // FINIE qui doit tenir dans `PART_ECRAN`, pas la seule zone de contenu.
    let budget = largeur_ecran * PART_ECRAN / 100 - 2 * (MARGE + SOUPLESSE) * PX_PAR_UNITE;
    LARGEUR_CIBLE.min(budget.max(0) / PX_PAR_UNITE)
}

/// **LA TAILLE DE LA FENÊTRE QUI MONTRE TOUT LE CONTENU**, en pixels physiques.
///
/// La LARGEUR est `LARGEUR_CIBLE`, ramenée à ce que l'écran peut porter (voir
/// `largeur_unites`). La HAUTEUR, elle, est CALCULÉE par le même emballage que
/// celui de la page : la rangée d'onglets, puis le PLUS GRAND groupe emballé à
/// cette largeur-là — un seul groupe est montré à la fois (DEMANDE EXPLICITE du
/// 2026-10-23 : « les catégories rangées par onglet »), donc la fenêtre n'a pas à
/// les tenir tous : elle les tient chacun.
///
/// ⚠️ **LES DEUX CÔTÉS SONT PLAFONNÉS À `PART_ECRAN`** — sans le plafond de
/// hauteur, une liste qui grandit (l'utilisateur en ajoute) ferait une fenêtre
/// plus haute que l'écran, donc des kaomojis INATTEIGNABLES ; sans celui de
/// largeur, la fenêtre sortait par la droite. Le défilement est là pour ça.
///
/// ⚠️ **LE PLAFOND DE LARGEUR NE MORD QU'EN DESSOUS DE 2 612 px** — c'est
/// `(LARGEUR_CIBLE + 2 × marges) × PX_PAR_UNITE` divisé par `PART_ECRAN`. Sur un
/// 3840 comme sur un 2560 il ne fait donc rien, et la largeur reste celle qu'on
/// VISE ; c'est ce que fige `la_largeur_demandee_ne_deborde_jamais_de_l_ecran`.
fn taille_pour_tout_le_contenu(largeur_ecran: i32, hauteur_ecran: i32) -> [i32; 2] {
    let largeur = largeur_unites(largeur_ecran);
    let mut plus_grand =
        super::kaomoji::FAMILLES.iter().map(|f| lignes_de(f.kaomojis, largeur)).max().unwrap_or(0);
    // ⚠️ **LES KAOMOJIS PERSONNELS COMPTENT AUSSI** : l'un d'eux peut être plus
    // large que tous les intégrés, et l'oublier ferait une fenêtre trop étroite
    // pour sa propre case.
    if let Some(lignes) = lignes_des_personnels(largeur) {
        plus_grand = plus_grand.max(lignes);
    }
    // ⚠️ **LES ONGLETS S'ENROULENT À LA MÊME LARGEUR** : sur un écran étroit ils
    // prennent une ligne de plus, et ne pas la compter couperait le bas du groupe.
    let hauteur = BANDE + hauteur_des_onglets(largeur) + plus_grand * HAUTEUR_CASE;
    let hauteur_unites = hauteur + 2 * (MARGE + SOUPLESSE);
    let plafond = (hauteur_ecran * PART_ECRAN / 100) / PX_PAR_UNITE;
    let hauteur_unites = hauteur_unites.min(plafond);
    [(largeur + 2 * (MARGE + SOUPLESSE)) * PX_PAR_UNITE, hauteur_unites * PX_PAR_UNITE]
}

/// **LA HAUTEUR DE LA RANGÉE D'ONGLETS**, en unités de page, une fois ses noms
/// enroulés dans `largeur` — même emballage que `lignes_de`, mêmes raisons.
fn hauteur_des_onglets(largeur: i32) -> i32 {
    let mut lignes = 0;
    let mut reste = 0;
    // ⚠️ **L'ONGLET « Mes kaomojis » N'EXISTE QUE S'IL Y A DES ENTRÉES** — la page
    // le cache quand la liste personnelle est vide, et le compter quand même
    // laisserait une rangée d'onglets vide en haut de la fenêtre.
    let personnels = super::kaomoji_perso::tous();
    let noms = super::kaomoji::FAMILLES
        .iter()
        .map(|f| f.nom)
        .chain(std::iter::once(FAMILLE_PERSO).filter(|_| !personnels.is_empty()));
    for nom in noms {
        let onglet = largeur_onglet(nom) + ESPACE_ONGLET;
        if reste > 0 && reste + onglet > largeur {
            lignes += 1;
            reste = 0;
        }
        reste += onglet;
    }
    if reste > 0 {
        lignes += 1;
    }
    lignes * HAUTEUR_ONGLET + MARGE_ONGLETS
}

/// Le nombre de lignes de l'onglet personnel, ou `None` s'il est vide.
fn lignes_des_personnels(largeur_emballage: i32) -> Option<i32> {
    let personnels = super::kaomoji_perso::tous();
    if personnels.is_empty() {
        return None;
    }
    let textes: Vec<&str> = personnels.iter().map(|k| k.texte.as_str()).collect();
    Some(lignes_de(&textes, largeur_emballage))
}

/// La largeur estimée d'un onglet, en unités de page.
fn largeur_onglet(nom: &str) -> i32 {
    nom.chars().count() as i32 * LARGEUR_CAR_ONGLET + PADDING_ONGLET
}

/// **LE NOMBRE DE LIGNES QU'UN GROUPE OCCUPE**, une fois emballé dans
/// `largeur_emballage` (voir `largeur_unites` — c'est la largeur RÉELLE de la
/// fenêtre, qui n'est `LARGEUR_CIBLE` que sur un écran assez large).
///
/// ⚠️ **SÉPARÉ DE `taille_pour_tout_le_contenu` POUR ÊTRE VÉRIFIABLE** : c'est la seule
/// partie du calcul qui puisse se tromper sans que rien ne le dise.
fn lignes_de(kaomojis: &[&str], largeur_emballage: i32) -> i32 {
    let mut lignes = 0;
    let mut reste = 0;
    for &k in kaomojis {
        let case = largeur_kaomoji(k) + ESPACE;
        if reste > 0 && reste + case > largeur_emballage {
            lignes += 1;
            reste = 0;
        }
        reste += case;
    }
    if reste > 0 {
        lignes += 1;
    }
    lignes
}

/// **LE RECTANGLE D'OUVERTURE — AU CENTRE DE L'ÉCRAN PRIMAIRE.** Même recette que
/// `cef_caracteres::rect_pour_ecran_primaire`, dont la doc porte la mesure : ce sont
/// les métriques du moniteur PRIMAIRE (`SM_CXSCREEN`/`SM_CYSCREEN`), en pixels
/// physiques, et c'est une décision de l'utilisateur — « toujours le primaire ».
pub fn rect_pour_ecran_primaire() -> [i32; 4] {
    let (largeur, hauteur) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    // ⚠️ **LES DEUX MÉTRIQUES, PAS SEULEMENT LA HAUTEUR** : c'est la largeur de
    // l'écran qui borne désormais la largeur de la fenêtre (voir
    // `largeur_unites`). Les deux étaient DÉJÀ lues ici — seule la hauteur
    // servait, et c'est ce qui laissait sortir 300 px par la droite sur un 1920.
    let [l, h] = cef_popup::taille_aux_textes(largeur, hauteur, taille_pour_tout_le_contenu);
    centrer_sur_ecran(largeur, hauteur, [l, h])
}

/// Le centre, séparé de la lecture Win32 pour être vérifiable par un test.
fn centrer_sur_ecran(largeur: i32, hauteur: i32, [l, h]: [i32; 2]) -> [i32; 4] {
    [((largeur - l) / 2).max(0), ((hauteur - h) / 2).max(0), l, h]
}

/// Ouvre ou referme la fenêtre. Appelée par le bras de clic de l'applet.
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice) {
    // Déjà ouverte : la remonter au premier plan si elle est enterrée, la cacher
    // si elle y est déjà (voir `cef_popup::basculer_ou_cacher_si_ouvert`).
    if cef_popup::basculer_ou_cacher_si_ouvert(&SLOT) {
        return;
    }
    crate::diag!("[kaomoji] ouverture : rect {rect:?} (x,y,largeur,hauteur)");
    PAYLOAD.with(|p| *p.borrow_mut() = None);
    // Le zoom de la page suit la taille que `rect_pour_ecran_primaire` a donnée.
    cef_popup::regler_zoom_textes(&SLOT, crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES), rect);
    cef_popup::montrer_fenetre(&SLOT, &SPEC, rect, gfx);
    RAFALE.with(|r| r.set(6));
    push_state();
}

/// **LE DESSIN — SANS LUI, LA FENÊTRE EST INVISIBLE.** Voir `cef_caracteres`, dont
/// la doc porte le pourquoi et la mesure. ⛔ Et surtout PAS `resize_if_needed`.
pub fn render_if_visible(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

/// La copie du back buffer pour le harnais — ⛔ **SANS ELLE LA FENÊTRE EST
/// INVÉRIFIABLE** (l'application est `WDA_EXCLUDEFROMCAPTURE`), et le bras manquant
/// de `opened_window_dump` échoue en SILENCE. Voir `cef_caracteres`, qui a payé.
pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// L'état de la page : les kaomojis **PAR GROUPE NOMMÉ**. ⚠️ **LA TABLE VIENT D'ICI,
/// PAS DE LA PAGE** — l'écrire aussi dans `kaomoji.html` en ferait une seconde source
/// de vérité (voir `super::kaomoji::FAMILLES`).
///
/// ⚠️ **LE NOM VOYAGE AVEC LA LISTE**, et pas dans un second tableau : deux tableaux
/// pourraient se désaligner, et un titre posé sur le mauvais groupe ne se verrait
/// qu'à l'usage — c'est-à-dire quand l'utilisateur clique sur un visage triste rangé
/// sous « Joie ».
fn etat_json() -> String {
    let familles: Vec<serde_json::Value> = super::kaomoji::FAMILLES
        .iter()
        .map(|f| {
            let kaomojis: Vec<String> = f.kaomojis.iter().map(|k| k.to_string()).collect();
            serde_json::json!({ "nom": f.nom, "kaomojis": kaomojis })
        })
        .collect();
    // ⭐ **LES PERSONNELS SONT UN SECOND TABLEAU, PAS UNE FAMILLE DE PLUS** : la
    // page doit pouvoir les distinguer pour passer leur `id` et non leur index,
    // et la rangée d'onglets les ajoute elle-même à la fin (voir `kaomoji.html`).
    let personnels: Vec<serde_json::Value> = super::kaomoji_perso::tous()
        .iter()
        .map(|k| serde_json::json!({ "id": k.id, "s": k.texte }))
        .collect();
    serde_json::json!({
        "familles": familles,
        "personnels": personnels,
        "famillePerso": FAMILLE_PERSO,
        "epingle": crate::config::load_kaomoji_premier_plan(),
    })
    .to_string()
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

/// **LA PUNAISE FAIT LES DEUX** — voir `cef_caracteres::appliquer_le_premier_plan`,
/// dont c'est la copie : le réglage est le nôtre (une `fenetre: true` ne reçoit pas
/// la punaise que Rust injecte sur les panneaux).
fn appliquer_le_premier_plan() {
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return };
    let au_dessus = crate::config::load_kaomoji_premier_plan();
    let rang = if au_dessus { Some(HWND_TOPMOST) } else { Some(HWND_NOTOPMOST) };
    unsafe {
        let _ = SetWindowPos(hwnd, rang, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// **CE QUE LA PAGE DEMANDE** : les trois gestes de la fenêtre, les deux de la
/// liste, et les trois de l'éditeur.
/// ⚠️ **LA PUNAISE N'EST PAS ICI** — `cef_popup::drain_actions` retire tout ce qui
/// finit par `/epingle` avant que le moindre module ne le voie.
pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("kaomoji/") else { continue };
        let mut morceaux = reste.split('/');
        let Some(verbe) = morceaux.next() else { continue };
        let premier = morceaux.next();
        match verbe {
            "deplacer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::deplacer_par_le_systeme(hwnd, "kaomoji");
                }
            }
            "fermer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::cacher_fenetre(hwnd);
                }
            }
            "epingler" => {
                let actif = !crate::config::load_kaomoji_premier_plan();
                crate::config::save_kaomoji_premier_plan(actif);
                crate::diag!("[kaomoji] premier plan = {actif}");
                appliquer_le_premier_plan();
                push_state();
            }
            // --- LES DEUX GESTES DE LA LISTE ---
            "frapper" | "copier" => {
                // ⚠️ **L'INDEX, PAS LE KAOMOJI** : un kaomoji contient des barres
                // obliques (`¯\_(ツ)_/¯`, `＼(º □ º l|l)/`) et des parenthèses — le
                // faire voyager dans l'URL demanderait de l'échapper, et une erreur
                // d'échappement serait SILENCIEUSE. L'index est de l'ASCII, et Rust
                // relit la table qu'il connaît déjà (même raison que le code
                // hexadécimal des caractères).
                let Some(index) = premier.and_then(|i| i.parse::<usize>().ok()) else {
                    crate::diag!("[kaomoji] index illisible");
                    continue;
                };
                let Some(k) = kaomoji_par_index(index) else {
                    crate::diag!("[kaomoji] index hors table : {index}");
                    continue;
                };
                frapper_ou_copier(verbe, k);
            }
            // ⭐ **LES PERSONNELS PAR IDENTIFIANT** : leur table peut changer
            // pendant que la fenêtre est ouverte, donc un index y serait instable.
            "frapper-perso" | "copier-perso" => {
                let Some(id) = premier.and_then(|i| i.parse::<u32>().ok()) else {
                    crate::diag!("[kaomoji] id personnel illisible");
                    continue;
                };
                let Some(k) = kaomoji_perso_par_id(id) else {
                    crate::diag!("[kaomoji] id personnel inconnu : {id}");
                    continue;
                };
                frapper_ou_copier(verbe, &k);
            }
            // --- LES TROIS GESTES DE L'ÉDITEUR ---
            "perso-ajouter" => {
                let Some(texte) = premier else { continue };
                let texte = crate::win32::cef_app::percent_decode(texte);
                match super::kaomoji_perso::ajouter(&texte) {
                    Some(id) => {
                        crate::diag!("[kaomoji] personnel ajouté : id {id}");
                        apres_mutation();
                    }
                    None => crate::diag!("[kaomoji] ajout refusé : {texte:?}"),
                }
            }
            "perso-modifier" => {
                let Some(id) = premier.and_then(|i| i.parse::<u32>().ok()) else {
                    crate::diag!("[kaomoji] id personnel illisible");
                    continue;
                };
                // ⚠️ **DÉCOUPER AVANT DE DÉCODER** : un `%2F` dans le texte ne doit
                // pas être pris pour un séparateur (voir `cef_app::percent_decode`).
                let Some(texte) = morceaux.next() else { continue };
                let texte = crate::win32::cef_app::percent_decode(texte);
                if super::kaomoji_perso::modifier(id, &texte) {
                    crate::diag!("[kaomoji] personnel {id} modifié");
                    apres_mutation();
                } else {
                    crate::diag!("[kaomoji] modification refusée : id {id}, {texte:?}");
                }
            }
            "perso-supprimer" => {
                let Some(id) = premier.and_then(|i| i.parse::<u32>().ok()) else {
                    crate::diag!("[kaomoji] id personnel illisible");
                    continue;
                };
                if super::kaomoji_perso::supprimer(id) {
                    crate::diag!("[kaomoji] personnel {id} supprimé");
                    apres_mutation();
                } else {
                    crate::diag!("[kaomoji] suppression refusée : id {id}");
                }
            }
            autre => crate::diag!("[kaomoji] verbe inconnu : {autre:?}"),
        }
    }
}

fn frapper_ou_copier(verbe: &str, k: &str) {
    if verbe.starts_with("frapper") {
        crate::win32::cible_de_frappe::frapper_dans_la_cible(k);
    } else {
        crate::win32::clipboard::write_text(k);
        crate::diag!("[kaomoji] copie : {k:?}");
    }
}

/// **APRÈS UNE MUTATION** : l'état (la page) ET la taille (le kaomoji le plus
/// large peut avoir changé). `resize_if_needed` est fait pour un redimensionnement
/// À LA DEMANDE — ce n'est pas l'appel par frame que la doc de module interdit.
fn apres_mutation() {
    push_state();
    recaler_apres_edition();
}

fn recaler_apres_edition() {
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return };
    let (largeur, hauteur) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let [l, h] = cef_popup::taille_aux_textes(largeur, hauteur, taille_pour_tout_le_contenu);
    let mut cadre = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut cadre) }.is_err() {
        return;
    }
    let x = cadre.left.clamp(0, (largeur - l).max(0));
    let y = cadre.top.clamp(0, (hauteur - h).max(0));
    cef_popup::resize_if_needed(&SLOT, [x, y, l, h]);
}

/// Le kaomoji personnel d'un identifiant. Rend une COPIE : l'appelant la garde
/// le temps de la frappe, sans tenir le cache.
fn kaomoji_perso_par_id(id: u32) -> Option<String> {
    super::kaomoji_perso::tous().into_iter().find(|k| k.id == id).map(|k| k.texte)
}

/// **LE KAOMOJI D'UN INDEX GLOBAL** — la table mise à plat, groupes dans l'ordre, et
/// DANS CHAQUE GROUPE l'ordre de la page. Rend `None` au-delà : un index hors table
/// veut dire que la page et Rust ne sont plus d'accord, et ça se dit plutôt que de ne
/// rien faire.
fn kaomoji_par_index(index: usize) -> Option<&'static str> {
    super::kaomoji::FAMILLES.iter().flat_map(|f| f.kaomojis.iter().copied()).nth(index)
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

/// Le code de `WM_NCHITTEST` : les huit bords et coins, sinon le client.
///
/// ⚠️ **C'EST LA QUATRIÈME COPIE DE CETTE FONCTION** (`cef_visionneuse`,
/// `cef_couleurs`, `cef_caracteres`) : le dépôt la tolère ainsi depuis le début —
/// chaque `fenetre: true` a la sienne. La factoriser est un vrai nettoyage, mais il
/// toucherait TROIS modules livrés pour une trentaine de lignes ; il se fera quand
/// ces fenêtres bougeront pour une autre raison. La BANDE DE PRÉHENSION, elle, vaut
/// 12 px physiques (8 logiques) — mesurée comme celle des caractères.
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

    fn verbes_connus() -> &'static [&'static str] {
        &[
            // La fenêtre et les kaomojis intégrés.
            "frapper",
            "copier",
            "deplacer",
            "epingler",
            "fermer",
            // Les personnels, par identifiant, et les trois gestes de l'éditeur.
            "frapper-perso",
            "copier-perso",
            "perso-ajouter",
            "perso-modifier",
            "perso-supprimer",
        ]
    }

    /// ⭐ **LA PAGE ET RUST DOIVENT PARLER LE MÊME LANGAGE** — même contrat que
    /// `cef_caracteres`, et même piège : la page émet ses verbes de DEUX façons (l'URL
    /// entière pour la liste, `go("…")` pour la fenêtre).
    #[test]
    fn les_verbes_de_la_page_sont_exactement_ceux_du_dispatch() {
        let page = include_str!("../../assets/popups/kaomoji.html");
        const PREFIXE: &str = "neuradesk://kaomoji/";
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
        for attendu in verbes_connus().iter().copied() {
            assert!(vus.contains(&attendu), "la page n'appelle jamais `{attendu}`");
        }
    }

    /// **L'ÉDITEUR ET LA PAGE PARLENT LA MÊME LANGUE** : le tableau `personnels`,
    /// le nom de leur onglet, et une liste VIDE sous `cargo test` (voir la doc de
    /// `kaomoji_perso` — la géométrie doit rester déterministe).
    #[test]
    fn l_etat_porte_la_liste_personnelle_et_son_nom() {
        let json: serde_json::Value = serde_json::from_str(&etat_json()).expect("un JSON valide");
        assert_eq!(json["personnels"].as_array().map(|l| l.len()), Some(0));
        assert_eq!(json["famillePerso"], FAMILLE_PERSO);
        let page = include_str!("../../assets/popups/kaomoji.html");
        assert!(page.contains("dernierEtat.personnels"), "la page lit `personnels`");
        assert!(page.contains("famillePerso"), "la page lit le nom de l'onglet personnel");
        assert_eq!(lignes_des_personnels(500), None, "un onglet vide n'occupe aucune ligne");
    }

    /// **L'ÉTAT PORTE TOUS LES KAOMOJIS, DANS L'ORDRE DE LA TABLE, AVEC LEUR GROUPE.**
    /// Et l'index global doit désigner EXACTEMENT ce que la page affichera au même
    /// rang — sinon un clic écrirait le kaomoji du voisin, ce qui ne se verrait qu'à
    /// l'usage.
    #[test]
    fn l_etat_porte_tous_les_kaomojis_et_l_index_designe_le_bon() {
        let json: serde_json::Value = serde_json::from_str(&etat_json()).expect("un JSON valide");
        let familles = json["familles"].as_array().expect("un tableau de groupes");
        assert_eq!(familles.len(), super::super::kaomoji::FAMILLES.len());
        let total: usize =
            familles.iter().filter_map(|f| f["kaomojis"].as_array()).map(|f| f.len()).sum();
        // 795 entrées : les 50 du classement public, les 50 classiques, et les 695
        // autres rangés dans les thèmes (dont 412 de Gboard) — plus d'entrées que de
        // kaomojis distincts,
        // parce qu'un kaomoji qui tient à DEUX thèmes y compte DEUX fois. C'est voulu
        // (voir `kaomoji`), et le haussement d'épaules, lui, ne compte qu'UNE fois :
        // il est dans « Les populaires » et nulle part ailleurs.
        assert_eq!(total, 795);
        // Le nom voyage AVEC la liste, et c'est celui de la table.
        let mut rang = 0;
        for (i, famille) in super::super::kaomoji::FAMILLES.iter().enumerate() {
            assert_eq!(familles[i]["nom"].as_str(), Some(famille.nom), "le groupe {i} a perdu son nom");
            assert_eq!(
                familles[i]["kaomojis"].as_array().map(|l| l.len()),
                Some(famille.kaomojis.len()),
                "le groupe {i} n'a pas le bon nombre de kaomojis"
            );
            for (j, &attendu) in famille.kaomojis.iter().enumerate() {
                assert_eq!(familles[i]["kaomojis"][j], attendu, "le groupe {i} diverge au rang {j}");
                assert_eq!(kaomoji_par_index(rang), Some(attendu), "l'index {rang} ne désigne pas le bon");
                rang += 1;
            }
        }
        assert_eq!(rang, total);
        assert_eq!(kaomoji_par_index(total), None, "au-delà de la table, on le DIT");
    }

    /// ⭐ **UN ONGLET PAR GROUPE : LA FENÊTRE TIENT LE PLUS GRAND, PAS LA SOMME.**
    ///
    /// La hauteur est la bande, la rangée d'onglets, et les lignes du PLUS GRAND
    /// groupe — exactement. ⛔ **ÉCHOUE SUR L'ANCIEN CODE**, qui empilait tous les
    /// groupes : sa hauteur était la SOMME, plusieurs fois celle-ci.
    #[test]
    fn la_hauteur_est_celle_du_plus_grand_groupe_et_des_onglets() {
        // Un écran assez grand pour que le plafond des 85 % ne morde pas.
        let [_, h] = taille_pour_tout_le_contenu(100_000, 100_000);
        let emballage = largeur_unites(100_000);
        let familles = super::super::kaomoji::FAMILLES;
        let plus_grand = familles.iter().map(|f| lignes_de(f.kaomojis, emballage)).max().unwrap();
        let somme: i32 = familles.iter().map(|f| lignes_de(f.kaomojis, emballage)).sum();
        let attendu = (BANDE + hauteur_des_onglets(emballage) + plus_grand * HAUTEUR_CASE + 2 * (MARGE + SOUPLESSE)) * PX_PAR_UNITE;
        assert_eq!(h, attendu, "bande + onglets + le plus grand groupe");
        assert!(h < somme * HAUTEUR_CASE * PX_PAR_UNITE, "plus courte que tous les groupes empilés");
    }

    /// ⚠️ **LES ONGLETS S'ENROULENT** : 23 noms ne tiennent pas sur une ligne de
    /// fenêtre, et un écran plus étroit en demande davantage.
    #[test]
    fn les_onglets_s_enroulent_et_plus_sur_un_ecran_etroit() {
        let large = hauteur_des_onglets(largeur_unites(3840));
        let etroit = hauteur_des_onglets(largeur_unites(1366));
        assert!(large > HAUTEUR_ONGLET + MARGE_ONGLETS, "plus d'une ligne d'onglets : {large}");
        assert!(etroit > large, "écran étroit, plus de lignes : {etroit} contre {large}");
    }

    /// ⭐ **LA FENÊTRE MONTRE TOUT SON CONTENU, ET NE DÉPASSE PAS L'ÉCRAN.** Les deux
    /// moitiés comptent : une fenêtre trop courte cache des kaomojis, une fenêtre plus
    /// haute que l'écran les rend INATTEIGNABLES — la barre de défilement elle-même
    /// serait hors de l'écran.
    #[test]
    fn la_fenetre_tient_sur_l_ecran_et_sa_largeur_est_celle_visee() {
        // ⚠️ **3840 EN LARGEUR, ET C'EST DÉLIBÉRÉ** : c'est le cas où
        // `LARGEUR_CIBLE` n'est PAS rognée — la machine de développement, et le
        // seul où la largeur VISÉE soit encore la largeur rendue. Le cas rogné
        // est couvert par `la_fenetre_ne_deborde_jamais_en_largeur` et
        // `un_ecran_plus_etroit_enroule_plus_de_lignes`.
        let (l_ecran, h_ecran) = (3840, 2160);
        let [l, h] = taille_pour_tout_le_contenu(l_ecran, h_ecran);
        assert_eq!(l, (LARGEUR_CIBLE + 2 * (MARGE + SOUPLESSE)) * PX_PAR_UNITE, "la largeur est celle qu'on vise");
        assert!(h <= h_ecran * PART_ECRAN / 100, "jamais plus de {PART_ECRAN} % de l'écran : {h} sur {h_ecran}");
        assert!(h > BANDE * PX_PAR_UNITE, "et elle a au moins sa bande de titre");
        // Le rectangle d'ouverture la centre, et la garde dans l'écran.
        let [x, y, l2, h2] = centrer_sur_ecran(l_ecran, h_ecran, [l, h]);
        assert_eq!((l2, h2), (l, h));
        assert!(x >= 0 && x + l2 <= l_ecran, "horizontalement dans l'écran : {x} + {l2}");
        assert!(y >= 0 && y + h2 <= h_ecran, "verticalement dans l'écran : {y} + {h2}");
    }

    /// ⛔ **LA LARGEUR ÉTAIT LE SEUL CÔTÉ SANS PLAFOND.**
    ///
    /// `taille_pour_tout_le_contenu` ne recevait que la HAUTEUR de l'écran : la
    /// largeur valait `LARGEUR_CIBLE` quelle que soit la machine — **2220 px**.
    /// Sur un 1920, `centrer_sur_ecran` fait `.max(0)`, donc le `x` tombe à 0 et
    /// **300 px sortent à droite** : le `✕` de la bande de titre devient
    /// INATTEIGNABLE, et rien ne le dit.
    ///
    /// ⛔ **CE TEST ÉCHOUE SUR L'ANCIEN CODE** : à `l_ecran = 1920` il rendait
    /// 2220. **CONSTATÉ, pas supposé** — c'est la sortie du rouge qui a ouvert ce
    /// lot : « la fenêtre déborde en largeur : 2220 sur 1920 ».
    #[test]
    fn la_largeur_demandee_ne_deborde_jamais_de_l_ecran() {
        // ⚠️ **LES LARGEURS SONT ÉCRITES EN CLAIR, ET C'EST DÉLIBÉRÉ.**
        // `assert!(l <= l_ecran * PART_ECRAN / 100)` serait une TAUTOLOGIE : `l`
        // est construite À PARTIR de ce budget (`largeur_unites` en retire déjà les
        // marges, puis multiplie par `PX_PAR_UNITE`), donc `min(a, b) <= b` est vrai
        // par construction, avec la MÊME constante des deux côtés — muter
        // `PART_ECRAN` ne ferait rien tomber. Des littéraux, eux, échouent.
        for (l_ecran, h_ecran, attendu_l) in [
            (3840, 2160, 2220), // le plafond ne mord pas : `LARGEUR_CIBLE` gagne
            (2560, 1440, 2175),
            (1920, 1080, 1632),
            (1366, 768, 1161),
        ] {
            let [l, h] = taille_pour_tout_le_contenu(l_ecran, h_ecran);
            assert_eq!(l, attendu_l, "largeur demandée sur {l_ecran}");
            assert!(h <= h_ecran * PART_ECRAN / 100, "la fenêtre déborde en hauteur : {h} sur {h_ecran}");
            // ⚠️ Et elle reste LISIBLE : borner ne doit pas rendre la fenêtre
            // plus étroite qu'un seul groupe ne peut s'y loger, sinon on aurait
            // échangé un `✕` hors écran contre une colonne de un kaomoji.
            assert!(l >= l_ecran / 2, "la fenêtre est devenue trop étroite : {l} sur {l_ecran}");
        }
        // ⚠️ Le plafond de HAUTEUR, lui, mord fort sur un 1080 : le contenu réclame
        // bien plus que 918, donc c'est le plafond qui décide — un littéral, pas une
        // relecture de la constante.
        let [_, h_1080] = taille_pour_tout_le_contenu(1920, 1080);
        assert_eq!(h_1080, 918, "1080 × 85 % = 918");
    }

    /// ⭐ **L'EMBALLAGE SUIT LA LARGEUR** — et c'est la moitié qu'on oublie.
    ///
    /// Borner la largeur de la FENÊTRE sans recalculer la HAUTEUR à cette
    /// largeur-là laisse une fenêtre trop COURTE : la page enroule plus de
    /// rangées qu'on n'en a compté, et les derniers groupes passent sous le pli.
    /// Sur un écran étroit la fenêtre doit donc être **plus HAUTE**, pas égale.
    ///
    /// ⛔ **ÉCHOUE SUR L'ANCIEN CODE** : il n'y avait qu'une seule largeur
    /// d'emballage, donc les deux hauteurs étaient IDENTIQUES.
    #[test]
    fn un_ecran_plus_etroit_enroule_plus_de_lignes() {
        // ⚠️ Hauteur d'écran DÉMESURÉE (100 000) des deux côtés : sinon c'est le
        // plafond de hauteur qui décide, et le test mesurerait le plafond au lieu
        // de l'emballage.
        let [l_large, h_large] = taille_pour_tout_le_contenu(3840, 100_000);
        let [l_etroit, h_etroit] = taille_pour_tout_le_contenu(1920, 100_000);
        assert!(l_etroit < l_large, "la fenêtre ne s'est pas rétrécie : {l_etroit} vs {l_large}");
        assert!(
            h_etroit > h_large,
            "emballage plus étroit, donc PLUS de lignes attendues : {h_etroit} vs {h_large}"
        );
    }

    /// ⚠️ **LA PLEINE CHASSE COMPTE DOUBLE** — sans ça, un kaomoji japonais serait
    /// estimé deux fois trop court et la fenêtre s'ouvrirait sur des barres de
    /// défilement partout.
    #[test]
    fn un_caractere_pleine_chasse_compte_double() {
        assert_eq!(largeur_kaomoji("((("), 3 * LARGEUR_UNITE + PADDING_CASE);
        assert_eq!(largeur_kaomoji("＾＾＾"), 6 * LARGEUR_UNITE + PADDING_CASE, "trois pleine chasse");
    }

    #[test]
    fn le_facteur_de_la_page_est_bien_celui_du_zoom() {
        assert_eq!(PX_PAR_UNITE as f64, cef_popup::CONTENT_ZOOM_FACTOR);
    }

    /// La bande de préhension : 12 px, et ce test la fixe.
    #[test]
    fn les_huit_bords_sont_reconnus() {
        let rect = [100, 200, 400, 300];
        assert_eq!(code_pour_point(rect, (101, 201)), 13, "coin haut-gauche");
        assert_eq!(code_pour_point(rect, (499, 499)), 17, "coin bas-droit");
        assert_eq!(code_pour_point(rect, (111, 350)), 10, "encore dans la bande");
        assert_eq!(code_pour_point(rect, (113, 350)), 1, "déjà dans le client");
        assert_eq!(code_pour_point(rect, (300, 350)), 1, "le CLIENT, pas un bord");
    }
}
