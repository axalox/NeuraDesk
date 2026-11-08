//! **LA FENÊTRE DES CARACTÈRES ACCENTUÉS** — DEMANDE EXPLICITE (2026-10-06) :
//! « une applet qui permette de faire apparaître une fenêtre flottante pour insérer
//! les caractères accentués de la langue française en minuscule et majuscule ainsi
//! que la ligature du o et du e », puis « **une fenêtre comme la visionneuse avec la
//! possibilité de la garder au premier plan** ».
//!
//! ⚠️ **C'EST UNE VRAIE FENÊTRE** (`fenetre: true`), comme la visionneuse et le
//! sélecteur de couleurs : barre de titre, Alt-Tab, bords redimensionnables par
//! `WM_NCHITTEST`. **Et elle PREND LE FOCUS** — décision de l'utilisateur, prise en
//! connaissance de ce que ça implique (voir plus bas).
//!
//! ---
//!
//! ## ⛔ CE QUE LE FOCUS COÛTE, ET COMMENT JE LE RATTRAPE
//!
//! La première version de cette applet était une fenêtre native qui ne prenait
//! JAMAIS le focus (`WS_EX_NOACTIVATE`) : les caractères partaient alors tout seuls
//! dans l'application où l'utilisateur écrivait, sans rien à restaurer. **Une vraie
//! fenêtre perd cet avantage** : au moment du clic, c'est ELLE qui a le premier plan.
//!
//! Je rattrape donc par la seule voie qui reste : **retenir la dernière fenêtre
//! ÉTRANGÈRE au premier plan** (voir `noter_la_derniere_fenetre_etrangere`, appelée à
//! chaque frame), la REMETTRE au premier plan au moment du clic, puis frapper.
//!
//! ⚠️ **CETTE VOIE EST CELLE QUE CE DÉPÔT A ABANDONNÉE AILLEURS**, pour une raison
//! mesurée (`cef_clipboard::handle_paste` : « le collage part alors dans la fenêtre
//! élue par la restauration, pas dans celle que l'utilisateur regarde »). Elle est
//! moins sûre que le non-focus, et c'est assumé : **le CLIC DROIT — la copie — est
//! la voie garantie.** Si la frappe n'arrive pas, elle n'arrive pas *en silence*, et
//! le journal le dit.
//!
//! ⚠️ **ET WINDOWS EN REFUSE UNE ENTRE SORTE** : un processus non élevé ne peut pas
//! frapper dans une fenêtre ÉLEVÉE (les onglets administrateur du Terminal, entre
//! autres). Ce refus est silencieux lui aussi — d'où le clic droit.
//!
//! ## LA PUNAISE FAIT LES DEUX
//!
//! Le bouton est injecté par `cef_popup` sur les quinze panneaux — **je n'en écris
//! aucun**. `est_epingle` retient déjà le panneau ouvert ; je m'en sers AUSSI pour
//! appliquer `HWND_TOPMOST`, ce qui est exactement « la garder au premier plan ».
//! Une punaise, un drapeau, deux effets — et rien de plus à retenir.

use std::cell::{Cell, RefCell};

// ⚠️ **NE PAS RETIRER `PCWSTR` SUR LA FOI DE L'AVERTISSEMENT `unused import`.**
// Il n'est consommé que par `classe()` (`-> PCWSTR`), qui n'existe que dans le
// profil de TEST — le `cargo build` le déclare donc inutilisé, et le retirer casse
// `cargo test`. Essayé, mesuré, remis.
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetSystemMetrics, GetWindowRect, SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SM_CXSCREEN,
    SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WM_CLOSE, WM_NCHITTEST,
};

use crate::gfx::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec, PART_ECRAN};

thread_local! {
    static SLOT: PopupSlot = PopupSlot::new();
    /// Le dernier état poussé — sans cette garde, `setState` repartirait à chaque
    /// frame et la page se reconstruirait sous les doigts (même garde que
    /// `cef_meteo`).
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// **LA RAFALE** — la première poussée se perd si la page n'a pas fini de
    /// charger, défaut mesuré trois fois dans ce dépôt (voir `cef_sous_dock::poll`).
    /// On repousse donc quelques frames de plus après l'ouverture, et pas une fois
    /// par frame : `PAYLOAD` rend le renvoi gratuit quand rien n'a changé.
    static RAFALE: Cell<u32> = const { Cell::new(0) };
    // ⛔ **IL Y AVAIT ICI UN `static RECT`, ET IL ÉTAIT MORT.** Il était écrit à
    // chaque ouverture et lu NULLE PART — sa doc affirmait même que
    // `render_if_visible` « en a besoin à CHAQUE frame », ce qui était faux. Il
    // datait d'un temps où le chemin de rendu recevait le rectangle ; le
    // `rect` est aujourd'hui journalisé (`diag!`) et passé à `montrer_fenetre`,
    // qui n'en garde pas de copie ici. Retiré plutôt que corrigé : état mort.
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCaracteresWindow"),
    window_title: w!("NeuraDesk — Caractères"),
    html_file: "caracteres.html",
    wndproc: Some(wndproc),
    // Sans objet pour une fenêtre — ces deux drapeaux départagent des FAMILLES de
    // popups ancrés au dock, et une fenêtre n'y est pas.
    above_a_magnified_dock: false,
    // ⛔ **`fades: false` EST OBLIGATOIRE** : un fondu demande `WS_EX_LAYERED`, qui
    // REMPLACE le comportement d'une vraie fenêtre.
    fades: false,
    fenetre: true,
};

/// **PRÉCHARGE LA FENÊTRE** — à appeler au démarrage, dans `app::preload_queue`.
///
/// ⚠️ **SANS ELLE, LA PAGE SE CHARGE À LA PREMIÈRE OUVERTURE ET PERD LE PREMIER ÉTAT
/// POUSSÉ** : c'est le défaut « il faut ouvrir deux fois les menus », mesuré dans ce
/// dépôt — la grille apparaîtrait vide au premier clic, et pleine au second. La
/// rafale de `refresh_if_visible` masquerait le symptôme la plupart du temps, mais
/// elle ne le répare pas : elle le rend intermittent, ce qui est pire.
pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// Voir `cef_popup::recaler_si_il_ne_tient_plus`, qui porte le défaut et pourquoi
/// il faut RÉARMER la monte. Appelée au changement d'écran : cette fenêtre est
/// ouverte au centre de la PRIMAIRE, donc un changement de résolution la laisserait
/// à un rectangle calculé pour l'ancien écran.
pub fn recaler_si_il_ne_tient_plus() {
    cef_popup::recaler_si_il_ne_tient_plus(&SLOT);
}

/// **LA TAILLE D'UNE TOUCHE, EN UNITÉS DE PAGE** — 46 px de côté plus 2 × 2 px de
/// marge, soit EXACTEMENT la règle `.case` de `caracteres.html`.
///
/// ⛔ **C'EST UNE DUPLICATION, ET ELLE EST GARDÉE PAR UN TEST** —
/// `les_unites_de_la_page_sont_celles_du_calcul_rust` relit la page et échoue si
/// les deux divergent. Sans ce garde, changer la taille d'une touche dans la page
/// laisserait la fenêtre s'ouvrir trop petite, **en silence**.
const TOUCHE: i32 = 50;
/// La marge intérieure de la page (le `padding` de `#grille`). C'est ELLE qui
/// « facilite le redimensionnement » : sans elle, la première touche touche le bord
/// et viser le bord vise une lettre. Même garde que `TOUCHE`.
const MARGE: i32 = 8;
/// La bande de titre, en unités de page — boutons de 24 px, `padding` 6 px, filet
/// de 1 px, plus la marge du haut de la grille. Volontairement un peu large : une
/// bande trop haute ne se voit pas, une bande trop basse fait apparaître une barre
/// de défilement à l'ouverture.
const BANDE: i32 = 40;
/// **UNITÉS DE PAGE → PIXELS PHYSIQUES.** La page est rendue au zoom
/// `CONTENT_ZOOM_FACTOR` (3×, voir `cef_popup`), et CEF n'applique AUCUN facteur
/// d'échelle de périphérique en plus — MESURÉ sur la capture du harnais : une
/// touche de 50 unités y occupe ~150 px.
const PX_PAR_UNITE: i32 = 3;
/// **LA MARGE DE SÉCURITÉ, en unités de page, de chaque côté.**
///
/// ⚠️ **MESURÉE, PAS PRUDENTE.** Le premier essai taillait la fenêtre EXACTEMENT
/// sur le contenu (466 × 356 unités) — et la capture a montré **une barre de
/// défilement horizontale**, alors que le calcul tombait juste. Le zoom ×3 ne
/// retombe pas sur des pixels entiers pour toutes les valeurs : un dépassement
/// d'un fragment de pixel fait apparaître la barre, qui rétrécit la zone cliente,
/// qui en fait apparaître une SECONDE. Deux unités (6 px) suffisent — assez pour
/// absorber l'arrondi, trop peu pour se voir.
const SOUPLESSE: i32 = 2;
// ⚠️ **`PART_ECRAN` N'EST PLUS DÉFINI ICI** — sa définition, et tout le pourquoi
// du chiffre, vivent dans `cef_popup` (voir son import ci-dessus). Il était
// recopié dans ce fichier ET dans `cef_kaomoji`, sans aucun garde : changer l'un
// laissait les tests de l'autre verts et les deux commentaires faux.

/// **LA FENÊTRE QUI MONTRE TOUT LE CONTENU**, en pixels physiques — la taille est
/// DÉRIVÉE du contenu, jamais choisie : le nombre de rangées vient des familles
/// elles-mêmes, et la largeur de la PLUS LARGE d'entre elles (les rangées n'ont pas
/// toutes le même nombre de touches — 9, puis 7).
///
/// ⛔ **C'EST LA RÉPONSE À « la fenêtre ne semble pas afficher tout son contenu au
/// démarrage » (2026-10-06).** La fenêtre s'ouvrait à 480×340 physiques, soit un
/// viewport de 160 unités de page pour un contenu qui en réclame 466 : on n'en
/// voyait que le TIERS. La taille avait été choisie à la main, à l'aveugle — elle
/// est maintenant calculée, et un test la compare à la page.
/// ⛔ **ET ELLE EST PLAFONNÉE À `PART_ECRAN` DE L'ÉCRAN, SUR LES DEUX AXES.**
/// Sans ce plafond, un 1920×1080 ne la contenait pas : 1230 px de haut pour 1080
/// d'écran, donc `centrer_sur_ecran` la collait à `y = 0` et **150 px passaient
/// sous le bord bas — INATTEIGNABLES même en défilant**, puisque c'est la FENÊTRE
/// qui sort de l'écran, pas son contenu.
///
/// ⚠️ **C'EST UN ARBITRAGE, ET IL EST ASSUMÉ — IL RENVERSE LE PRÉCÉDENT.** La doc
/// ci-dessus disait le contraire : « la fenêtre est passée de 1080 à 1230 de haut,
/// donc un 1920×1080 ne la contient plus. C'est le prix de “montrer tout son
/// contenu” ». Ce prix était plus cher qu'il n'y paraissait — il rendait une
/// partie du contenu INATTEIGNABLE, ce qui est pire que de devoir défiler.
///
/// ⚠️ **CAR CETTE GRILLE NE SE REPLIE PAS** : ses touches ont une taille fixe
/// (`.case` à 46 px). Une fenêtre plafonnée montre donc moins de colonnes — et le
/// DÉFILEMENT rend le reste atteignable (`#grille { overflow: auto }`, prévu pour
/// ça). « Afficher tout son contenu » reste tenu : tout est ATTEIGNABLE, à défaut
/// d'être visible d'un coup. C'est la différence avec `cef_kaomoji`, dont les
/// familles s'ENROULENT — là-bas, réduire la largeur ne cache rien.
///
/// ⚠️ **LE PLAFOND MORD QUAND L'ÉCRAN PASSE SOUS ~2 189 px DE LARGE OU ~1 448 DE
/// HAUT** (`1860 / 0,85` et `1230 / 0,85`). Conséquence à ne pas prendre pour un
/// défaut : un **2560×1440** perd **6 px de haut** (1 440 × 85 % = 1 224 < 1 230),
/// et rien en largeur. Un 3840×2160, lui, n'est touché sur AUCUN axe.
fn taille_pour_tout_le_contenu(largeur_ecran: i32, hauteur_ecran: i32) -> [i32; 2] {
    let colonnes = super::caracteres::FAMILLES.iter().map(|ligne| ligne.len()).max().unwrap_or(1) as i32;
    let rangees = super::caracteres::FAMILLES.len() as i32;
    [
        ((colonnes * TOUCHE + 2 * (MARGE + SOUPLESSE)) * PX_PAR_UNITE).min(largeur_ecran * PART_ECRAN / 100),
        ((BANDE + rangees * TOUCHE + 2 * (MARGE + SOUPLESSE)) * PX_PAR_UNITE).min(hauteur_ecran * PART_ECRAN / 100),
    ]
}

/// **LE RECTANGLE D'OUVERTURE — LA GRILLE AU CENTRE DE L'ÉCRAN PRIMAIRE.**
///
/// Même recette que `cef_visionneuse::rect_pour` : `SM_CXSCREEN`/`SM_CYSCREEN`,
/// c'est-à-dire les métriques du moniteur PRIMAIRE, en pixels PHYSIQUES (le
/// processus est `PER_MONITOR_AWARE_V2`). **Décision de l'utilisateur, 2026-10-06 :
/// « toujours le primaire »** — la fenêtre a été voulue « comme la visionneuse »,
/// et la visionneuse ne s'ouvre nulle part ailleurs.
///
/// ⛔ **ET SURTOUT PAS `active_layout.total_width`** — c'est la taille de la ZONE
/// CLIENTE du dock, dont l'origine vaut `(0, 0)` dans le repère DU DOCK, pas dans
/// celui du bureau virtuel. Le résultat tombait sur le primaire, mais **décalé de
/// toute la largeur de l'autre écran** dès que l'applet était cliquée sur un écran
/// secondaire (celui de cette machine est à `x = -1920`) : « la fenêtre devrait
/// s'afficher au centre de l'écran », signalé le 2026-10-06. Les deux nombres ne
/// sont pas interchangeables — 2088 (hauteur cliente du dock) n'est pas 2160
/// (hauteur de l'écran).
pub fn rect_pour_ecran_primaire() -> [i32; 4] {
    let (largeur, hauteur) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    // ⚠️ **LES DEUX MÉTRIQUES, PAS SEULEMENT LA LARGEUR** : l'écran borne
    // désormais la taille demandée (voir `taille_pour_tout_le_contenu`). Les deux
    // étaient DÉJÀ lues ici — servir de l'une seule est ce qui laissait 150 px
    // sortir par le bas d'un 1920×1080.
    let [l, h] = cef_popup::taille_aux_textes(largeur, hauteur, taille_pour_tout_le_contenu);
    centrer_sur_ecran(largeur, hauteur, [l, h])
}

/// Le centre, séparé de la lecture Win32 pour être VÉRIFIABLE par un test — le
/// rectangle qu'il remplace ne l'était pas.
///
/// ⚠️ **`max(0)` RESTE, MÊME SI LA PRODUCTION NE PEUT PLUS L'ATTEINDRE.** Depuis
/// le plafond `PART_ECRAN`, la taille demandée tient toujours dans l'écran, donc
/// `(largeur - l) / 2` est positif pour tout écran réel — et
/// `rect_pour_ecran_primaire` est l'unique appelant de production. La garde reste
/// pour deux raisons : c'est le SEUL endroit qui empêche une coordonnée négative
/// si on lui passait un jour une boîte plus grande que l'écran (c'est ce que fait
/// le test), et la visionneuse, elle, a en plus son décalage en cascade à
/// rattraper.
///
/// ⚠️ **ET LA JUSTIFICATION D'AVANT (« c'est le seul cas où le calcul peut rendre
/// une coordonnée négative ») ÉTAIT DEVENUE FAUSSE** en posant ce plafond : dans
/// ce dépôt, un commentaire périmé est un défaut au même titre qu'un test périmé.
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
    // **LES NOMBRES, PAS LA CONCLUSION** — l'écran est un fait de la machine, pas
    // du code : sans cette ligne, « la fenêtre n'est pas centrée » ne se distingue
    // pas d'« elle est centrée sur le mauvais écran ».
    crate::diag!("[caracteres] ouverture : rect {rect:?} (x,y,largeur,hauteur)");
    PAYLOAD.with(|p| *p.borrow_mut() = None);
    // ⛔ **NE PAS LIRE CE RETOUR COMME UN ÉCHEC** — `montrer_fenetre` rend « a-t-elle
    // été CRÉÉE », pas « a-t-elle réussi » : une fenêtre PRÉCHARGÉE existe déjà et
    // rend donc `false` alors qu'elle vient d'être montrée. J'ai écrit le contraire,
    // et le journal a annoncé « la fenetre n'a pas pu etre creee » sur une fenêtre
    // parfaitement ouverte. La création est déjà nommée par `create_window`.
    // Le zoom de la page suit la taille que `rect_pour_ecran_primaire` a donnée.
    cef_popup::regler_zoom_textes(&SLOT, crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES), rect);
    cef_popup::montrer_fenetre(&SLOT, &SPEC, rect, gfx);
    // La rafale : la page n'est pas encore chargée à cet instant.
    RAFALE.with(|r| r.set(6));
    push_state();
}

/// **LE DESSIN — SANS LUI, LA FENÊTRE EST INVISIBLE.** C'est `cef_popup::render` qui
/// peint le fond (le bureau flouté) et la page par-dessus ; un module qui ne
/// l'appelle pas crée une fenêtre, la montre, et n'y dessine **jamais rien**. C'est
/// exactement ce que j'avais fait, et le symptôme est trompeur : la fenêtre EXISTE
/// (elle prend même le focus), mais rien ne se voit.
///
/// Appelée à chaque frame par `app::run` — comme le météo, la calculatrice et les
/// autres panneaux.
/// ⛔ **ET SURTOUT PAS `resize_if_needed`** — la visionneuse ne l'appelle PAS non
/// plus, et c'est ce qui la laisse redimensionnable. Le forcer à chaque frame
/// ramenait la fenêtre à sa taille d'ouverture dès que l'utilisateur en tirait un
/// bord : signalé le 2026-10-06, « quand je redimensionne la fenêtre elle retourne à
/// sa taille d'origine ». La taille se pose à la CRÉATION (le `rect` de `toggle`), et
/// ensuite elle appartient à l'utilisateur.
pub fn render_if_visible(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

/// **LA COPIE DU BACK BUFFER POUR LE HARNAIS** — voir
/// `win32::cef_meteo::debug_dump_backbuffer`, dont c'est la copie.
///
/// ⛔ **SANS ELLE, LA FENÊTRE EST INVÉRIFIABLE, ET ÇA A COÛTÉ CHER.** L'application
/// est `WDA_EXCLUDEFROMCAPTURE` : aucune capture d'écran externe ne la voit, et
/// celle-ci ne l'était pas non plus par le harnais — le bras manquait à la
/// QUATRIÈME liste de `app::run` (`opened_window_dump`), celle qui ÉCHOUE EN
/// SILENCE (voir [[neuradesk-ajouter-une-page-cef]]). `NEURADESK_DEBUG_OPEN=caracteres`
/// ouvrait donc une fenêtre dont aucun PNG ne sortait jamais, et un run vert ne
/// disait rien.
pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// L'état de la page : les 60 caractères, PAR FAMILLES.
///
/// ⚠️ **LA GRILLE VIENT D'ICI, PAS DE LA PAGE** : les caractères sont décidés par
/// l'utilisateur, et les écrire aussi dans `caracteres.html` en ferait une seconde
/// source de vérité — celle qui divergerait au premier ajout (voir
/// `super::caracteres::FAMILLES`).
fn etat_json() -> String {
    let familles: Vec<Vec<String>> = super::caracteres::FAMILLES
        .iter()
        .map(|ligne| ligne.iter().map(|c| c.to_string()).collect())
        .collect();
    // La punaise voyage AVEC l'état : le bouton doit montrer son état courant, et
    // `PAYLOAD` re-pousse donc dès qu'elle change (il compare le JSON entier).
    serde_json::json!({ "familles": familles, "epingle": crate::config::load_caracteres_premier_plan() }).to_string()
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

/// **À APPELER À CHAQUE FRAME** — entretient la rafale de la page, et applique le
/// premier plan demandé par la punaise.
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

/// **LA PUNAISE FAIT LES DEUX** : `cef_popup::est_epingle` est le drapeau posé par
/// le bouton que Rust injecte sur tous les panneaux — il retient déjà le panneau
/// ouvert. On s'en sert AUSSI pour `HWND_TOPMOST`, ce qui est exactement « la garder
/// au premier plan ». Un drapeau, deux effets, rien de plus à retenir.
fn appliquer_le_premier_plan() {
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return };
    // ⚠️ **LE RÉGLAGE EST LE NÔTRE, PAS `cef_popup::est_epingle`** : ce drapeau-là
    // vient du bouton que Rust injecte sur les panneaux ancrés, et une
    // `fenetre: true` ne le reçoit pas (voir `config::load_caracteres_premier_plan`).
    let au_dessus = crate::config::load_caracteres_premier_plan();
    let rang = if au_dessus { Some(HWND_TOPMOST) } else { Some(HWND_NOTOPMOST) };
    unsafe {
        let _ = SetWindowPos(hwnd, rang, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// **CE QUE LA PAGE DEMANDE.** Deux verbes, et rien d'autre : frapper, copier.
///
/// ⚠️ **LA PUNAISE N'EST PAS ICI** : `cef_popup::drain_actions` retire tout ce qui
/// finit par `/epingle` AVANT que le moindre module ne le voie (voir sa doc) — le
/// bouton marche donc sans une ligne de plus ici.
pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("caracteres/") else { continue };
        let mut morceaux = reste.split('/');
        let (Some(verbe), code) = (morceaux.next(), morceaux.next()) else { continue };
        match verbe {
            // --- LES TROIS GESTES DE LA FENÊTRE — même famille que la visionneuse ---
            "deplacer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::deplacer_par_le_systeme(hwnd, "caracteres");
                }
            }
            "fermer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::cacher_fenetre(hwnd);
                }
            }
            "epingler" => {
                let actif = !crate::config::load_caracteres_premier_plan();
                crate::config::save_caracteres_premier_plan(actif);
                crate::diag!("[caracteres] premier plan = {actif}");
                appliquer_le_premier_plan();
                // ⚠️ **L'ÉTAT EST REPOUSSÉ** : le bouton doit montrer son nouvel état,
                // sinon un clic parfaitement réussi se lirait comme un clic sans effet
                // (même raison que la visionneuse, qui la porte aussi).
                push_state();
            }
            // --- LES DEUX GESTES DE LA GRILLE ---
            "frapper" | "copier" => {
                let Some(code) = code else { continue };
                // Le code arrive en HEXADÉCIMAL (voir `caracteres.html`) : de l'ASCII,
                // donc rien à échapper dans l'URL, et aucune ambiguïté à la relecture.
                let Ok(pointeur) = u32::from_str_radix(code, 16) else {
                    crate::diag!("[caracteres] code illisible : {code:?}");
                    continue;
                };
                let Some(caractere) = char::from_u32(pointeur) else {
                    crate::diag!("[caracteres] code hors alphabet : {pointeur:#X}");
                    continue;
                };
                if verbe == "frapper" {
                    // Une chaîne d'un seul caractère : la primitive est partagée avec
                    // la fenêtre des kaomojis (voir `win32::cible_de_frappe`).
                    crate::win32::cible_de_frappe::frapper_dans_la_cible(&caractere.to_string());
                } else {
                    crate::win32::clipboard::write_text(&caractere.to_string());
                    crate::diag!("[caracteres] copie : {caractere:?}");
                }
            }
            autre => crate::diag!("[caracteres] verbe inconnu : {autre:?}"),
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // ⛔ **LA FENÊTRE N'A NI BORDURE NI BARRE DE TITRE** : le système ne fournit donc
    // pas le redimensionnement par les bords, et c'est ici qu'on le rend (même code
    // que `cef_visionneuse`, dont cette fenêtre reprend le comportement).
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
        // Fermer ne détruit rien : la fenêtre est cachée et réutilisée.
        cef_popup::cacher_fenetre(hwnd);
        return LRESULT(0);
    }
    if let Some(resultat) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return resultat;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Le code de `WM_NCHITTEST` pour un point : les huit bords et coins, sinon le
/// client. `HTLEFT`/`HTTOP`… valent 10 à 17 — les valeurs sont écrites ici plutôt
/// qu'importées pour que la table se lise d'un coup d'œil.
fn code_pour_point(rect: [i32; 4], (x, y): (i32, i32)) -> i32 {
    /// **LA BANDE DE PRÉHENSION**, en pixels physiques — « une petite marge qui
    /// facilite son redimensionnement » (demande explicite, 2026-10-06).
    ///
    /// 6 px physiques (les 4 px logiques de la visionneuse) sont justes mais
    /// MINCES : sur cet écran à 150 %, viser 4 px logiques avec la souris est un
    /// exercice. 12 px physiques = 8 logiques, la largeur d'une barre de
    /// défilement — la main la trouve sans y penser.
    ///
    /// ⚠️ **ELLE N'EST PAS LA SEULE MARGE** : la page en a une AUSSI, à l'intérieur
    /// (`#grille { padding }`, comptée par `MARGE`). Les deux servent la même
    /// demande : celle de la page empêche de viser une LETTRE quand on vise le bord.
    const BORD: i32 = 12;
    let (gauche, haut) = (x < rect[0] + BORD, y < rect[1] + BORD);
    let (droite, bas) = (x >= rect[0] + rect[2] - BORD, y >= rect[1] + rect[3] - BORD);
    match (gauche, droite, haut, bas) {
        (true, _, true, _) => 13,   // HTTOPLEFT
        (_, true, true, _) => 14,   // HTTOPRIGHT
        (true, _, _, true) => 16,   // HTBOTTOMLEFT
        (_, true, _, true) => 17,   // HTBOTTOMRIGHT
        (true, _, _, _) => 10,      // HTLEFT
        (_, true, _, _) => 11,      // HTRIGHT
        (_, _, true, _) => 12,      // HTTOP
        (_, _, _, true) => 15,      // HTBOTTOM
        _ => 1,                     // HTCLIENT
    }
}

/// Le `PCWSTR` de la classe — pour les tests qui vérifient le contrat.
#[cfg(test)]
pub fn classe() -> PCWSTR {
    SPEC.class_name
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐ **LA PAGE ET RUST DOIVENT PARLER LE MÊME LANGAGE.** Les verbes écrits dans
    /// `caracteres.html` sont la seule chose qui relie les deux, et une faute de
    /// frappe d'un côté se traduirait par un bouton qui ne fait RIEN — sans erreur,
    /// sans journal, et sans que rien ne le signale.
    #[test]
    fn les_verbes_de_la_page_sont_exactement_ceux_du_dispatch() {
        let page = include_str!("../../assets/popups/caracteres.html");
        // ⚠️ **LA PAGE ÉMET SES VERBES DE DEUX FAÇONS, ET IL FAUT LIRE LES DEUX.**
        // Les deux gestes de la grille écrivent l'URL ENTIÈRE ; les trois gestes de
        // la fenêtre passent par `go("…")`, qui CONCATÈNE le préfixe à l'exécution —
        // ce préfixe est donc dans la page sans verbe derrière lui. Ma première
        // version ne lisait que la première forme et prenait le préfixe pour un verbe.
        const PREFIXE: &str = "neuradesk://caracteres/";
        let mut vus: Vec<&str> = Vec::new();
        // Forme 1 — l'URL écrite en entier.
        for (i, _) in page.match_indices(PREFIXE) {
            let reste = &page[i + PREFIXE.len()..];
            let verbe = reste.split('/').next().unwrap_or("");
            if verbes_connus().contains(&verbe) {
                vus.push(verbe);
            } else {
                // Le seul cas toléré : le préfixe suivi d'une concaténation.
                assert!(
                    reste.starts_with("\" +"),
                    "verbe inconnu dans la page : {verbe:?} — un bouton qui n'appelle rien"
                );
            }
        }
        // Forme 2 — l'argument de `go("…")`.
        for morceau in page.split("go(\"").skip(1) {
            let verbe = morceau.split('"').next().unwrap_or("");
            assert!(verbes_connus().contains(&verbe), "verbe inconnu passé à `go` : {verbe:?}");
            vus.push(verbe);
        }
        // ⭐ **LES CINQ VERBES DOIVENT ÊTRE LÀ** — c'est ce qui attrape un bouton
        // ajouté à la page et oublié dans le `dispatch`.
        for attendu in verbes_connus() {
            assert!(vus.contains(&attendu), "la page n'appelle jamais `{attendu}`");
        }
    }

    fn verbes_connus() -> [&'static str; 5] {
        ["frapper", "copier", "deplacer", "epingler", "fermer"]
    }

    /// ⛔ **LA GRILLE EST VIDÉE AVANT D'ÊTRE RECONSTRUITE.**
    ///
    /// `setState` **ajoutait** ses rangées sans jamais vider : chaque poussée d'état
    /// ajoutait une grille ENTIÈRE sous la précédente. Défaut signalé le 2026-10-06 :
    /// « j'ai plusieurs fois le même contenu quand je clique sur le bouton pour la
    /// garder au premier plan ». ⚠️ **Et la punaise est le SEUL geste qui le montre**,
    /// parce que c'est la seule chose qui change l'état — côté Rust, `PAYLOAD` rend
    /// toutes les autres poussées gratuites.
    ///
    /// ⚠️ **CE TEST LIT LA PAGE, ET C'EST ASSUMÉ** : aucune sonde ne sait cliquer la
    /// punaise, et la capture ne prouve que l'affichage, jamais la réaction (voir
    /// `neuradesk-cef-ce-que-la-capture-ne-voit-pas`). Un défaut qu'aucune mesure ne
    /// peut atteindre mérite au moins un garde-fou de source — même famille que
    /// `les_verbes_de_la_page_sont_exactement_ceux_du_dispatch` juste au-dessus.
    ///
    /// ⛔ **IL ÉCHOUE SUR L'ANCIENNE PAGE** : elle n'appelait jamais `replaceChildren`.
    #[test]
    fn la_grille_est_videe_avant_d_etre_reconstruite() {
        let page = include_str!("../../assets/popups/caracteres.html");
        let corps = page.split("window.setState = function").nth(1).expect("la fonction `setState`");
        let corps = &corps[..corps.find("\n  };").unwrap_or(corps.len())];
        // ⚠️ **ON CHERCHE LA FORME D'APPEL, PAS LE NOM.** Le premier jet cherchait
        // `appendChild` nu — et le COMMENTAIRE qui explique ce correctif le cite
        // (« `appendChild` AJOUTE : … »), donc il était trouvé AVANT l'appel et le
        // test échouait sur une page juste. Le point qui précède suffit à les
        // distinguer, et c'est le même piège que la sonde qui montrait le début
        // d'une chaîne au lieu de la fin.
        let vidage = corps.find("replaceChildren(").expect("`setState` doit VIDER la grille");
        let ajout = corps.find(".appendChild(").expect("`setState` doit y ajouter les rangées");
        assert!(vidage < ajout, "la grille doit être vidée AVANT le premier ajout");
    }

    /// **L'ÉTAT PORTE LES 48 CARACTÈRES, ET LA BONNE FORME.** La page les reçoit par
    /// `setState` : une forme inattendue (un tableau plat, ou des lignes vides) la
    /// laisserait dessiner une grille vide, sans erreur.
    #[test]
    fn l_etat_porte_tous_les_caracteres() {
        let json: serde_json::Value = serde_json::from_str(&etat_json()).expect("un JSON valide");
        let familles = json["familles"].as_array().expect("un tableau de familles");
        let total: usize = familles.iter().filter_map(|f| f.as_array()).map(|f| f.len()).sum();
        assert_eq!(total, 60, "48 décidés le 2026-10-06, + 12 spéciaux");
        assert_eq!(familles.len(), 7, "sept lignes, dans l'ordre validé");
        assert_eq!(familles[0][0], "à");
        // ⚠️ **LA DERNIÈRE LIGNE EST LA PLUS LARGE**, et c'est elle qui fixe la
        // largeur de la fenêtre : si elle cessait de l'être, la fenêtre resterait
        // large sans raison, mais surtout le test de taille cesserait de décrire la
        // vraie contrainte.
        let derniere = familles.last().and_then(|f| f.as_array()).expect("la ligne des spéciaux");
        assert_eq!(derniere.len(), 12, "douze spéciaux");
        assert_eq!(derniere[0], ",");
        assert_eq!(derniere[7], "\\", "l'antislash, écrit deux fois côté Rust");
    }

    /// Le code de `WM_NCHITTEST` : les huit bords, et le client au milieu.
    #[test]
    fn les_huit_bords_sont_reconnus() {
        let rect = [100, 200, 400, 300];
        assert_eq!(code_pour_point(rect, (101, 201)), 13, "coin haut-gauche");
        assert_eq!(code_pour_point(rect, (499, 201)), 14, "coin haut-droit");
        assert_eq!(code_pour_point(rect, (101, 499)), 16, "coin bas-gauche");
        assert_eq!(code_pour_point(rect, (499, 499)), 17, "coin bas-droit");
        assert_eq!(code_pour_point(rect, (101, 350)), 10, "bord gauche");
        assert_eq!(code_pour_point(rect, (499, 350)), 11, "bord droit");
        assert_eq!(code_pour_point(rect, (300, 201)), 12, "bord haut");
        assert_eq!(code_pour_point(rect, (300, 499)), 15, "bord bas");
        assert_eq!(code_pour_point(rect, (300, 350)), 1, "le CLIENT, pas un bord");
        // ⭐ **LA BANDE DE PRÉHENSION FAIT 12 px, ET CE TEST LA FIXE.** À 11 px du
        // bord on étire, à 13 on clique une touche — c'est toute la différence entre
        // « une marge qui facilite le redimensionnement » et une marge qui laisse
        // passer les clics.
        assert_eq!(code_pour_point(rect, (111, 350)), 10, "encore dans la bande");
        assert_eq!(code_pour_point(rect, (113, 350)), 1, "déjà dans le client");
        assert_eq!(code_pour_point(rect, (300, 211)), 12, "la bande du haut");
        assert_eq!(code_pour_point(rect, (300, 213)), 1, "déjà dans le client");
        assert_eq!(code_pour_point(rect, (300, 488)), 15, "la bande du bas");
        assert_eq!(code_pour_point(rect, (300, 486)), 1, "déjà dans le client");
    }

    /// ⭐ **LA GRILLE EST AU CENTRE DE L'ÉCRAN PRIMAIRE.**
    ///
    /// ⛔ **CE TEST ÉCHOUE SUR L'ANCIEN CALCUL**, et c'est tout son intérêt : il
    /// centrait sur `active_layout.total_width`/`total_height`, soit la ZONE CLIENTE
    /// du dock — `3840×2088` ici, alors que l'écran fait `3840×2160` : un `y` de
    /// `874` au lieu de `910`. Tant que l'applet était cliquée sur le primaire
    /// l'écart était invisible ; cliquée sur un écran secondaire, la fenêtre partait
    /// sur le primaire, décalée de toute la largeur de l'autre.
    #[test]
    fn la_fenetre_est_centree_sur_l_ecran_primaire() {
        // ⚠️ **3840×2160 : LE PLAFOND NE MORD PAS ICI**, donc `boite` est bien
        // celle, pleine, du CONTENU — c'est ce que ce test-ci mesure. Le cas où le
        // plafond mord a son propre test.
        let boite = taille_pour_tout_le_contenu(3840, 2160);
        // Les nombres, en clair : 12 colonnes (la ligne des spéciaux) de 50 unités
        // + 2 × (8 + 2) = 620 ; 40 de bande + 7 rangées de 50 + 2 × (8 + 2) = 410 ;
        // le tout × 3.
        assert_eq!(boite, [1860, 1230], "la fenêtre qui montre les 12 × 7 touches");
        // L'écran primaire de la machine où le défaut a été signalé : 3840×2160 en
        // pixels physiques (un 2560×1440 à 150 %).
        assert_eq!(centrer_sur_ecran(3840, 2160, boite), [990, 465, 1860, 1230]);
        // ⛔ La hauteur du DOCK, elle, ne doit PAS donner la bonne réponse : c'est
        // la confusion qui a produit le défaut.
        assert_ne!(centrer_sur_ecran(3840, 2088, boite), centrer_sur_ecran(3840, 2160, boite));
        // Un écran plus petit que la grille : collée au bord, jamais hors de l'écran.
        assert_eq!(centrer_sur_ecran(320, 200, boite), [0, 0, boite[0], boite[1]]);
        // La propriété qui vaut pour tout écran OÙ ELLE TIENT : mêmes marges.
        //
        // ⚠️ **CE BLOC EST UN TEST DE `centrer_sur_ecran`, PAS UNE COUVERTURE DE
        // CE QUI EST LIVRÉ.** Il lui donne la boîte NON PLAFONNÉE du 4K (1860×1230)
        // et les deux dernières lignes — 2560×1440, 1920×1400 — sont des
        // combinaisons que `rect_pour_ecran_primaire` ne peut plus produire depuis
        // le plafond `PART_ECRAN`. Le centrage et le collage au bord restent bon à
        // figer ici ; la taille RÉELLE, elle, est figée par
        // `la_taille_demandee_ne_depasse_jamais_le_plafond_de_l_ecran`.
        for (largeur, hauteur) in [(3840, 2160), (2560, 1440), (1920, 1400)] {
            let [x, y, l, h] = centrer_sur_ecran(largeur, hauteur, boite);
            let (gauche, droite) = (x, largeur - l - x);
            let (haut, bas) = (y, hauteur - h - y);
            assert!(gauche.abs_diff(droite) <= 1, "{largeur} de large : {gauche} contre {droite}");
            assert!(haut.abs_diff(bas) <= 1, "{hauteur} de haut : {haut} contre {bas}");
        }
        // ⭐ **SUR UN 1920×1080, ELLE TIENT MAINTENANT EN ENTIER** — c'est le
        // plafond `PART_ECRAN` qui l'y fait tenir (voir
        // `la_fenetre_ne_depasse_jamais_le_plafond_de_l_ecran`).
        //
        // ⛔ **AVANT, CE N'ÉTAIT PAS LE CAS, ET C'ÉTAIT PIRE QU'UN DÉFAUT DE TAILLE** :
        // la fenêtre demandée faisait 1860×**1230** pour 1080 d'écran,
        // `centrer_sur_ecran` la collait à `y = 0`, et **150 px passaient sous le
        // bord bas — INATTEIGNABLES même en défilant**, puisque c'est la FENÊTRE qui
        // sortait de l'écran, pas son contenu. C'est ce que l'audit du 2026-10-06
        // appelait « ~2 rangées sur 7 hors écran en bas ».
        //
        // ⚠️ L'ANCIEN COMMENTAIRE JUSTIFIAIT CE CAS (« c'est le prix de “montrer
        // tout son contenu” ») : le prix était plus cher qu'il n'y paraissait, et le
        // défilement rend le contenu atteignable là où la fenêtre hors écran ne le
        // rendait pas.
        let boite_1080 = taille_pour_tout_le_contenu(1920, 1080);
        assert_eq!(boite_1080, [1632, 918], "85 % de 1920 × 85 % de 1080");
        let [x, y, l, h] = centrer_sur_ecran(1920, 1080, boite_1080);
        assert!(x >= 0 && y >= 0 && x + l <= 1920 && y + h <= 1080, "hors écran : ({x},{y}) {l}×{h}");
        assert!(x.abs_diff(1920 - l - x) <= 1, "centrée en largeur : marge {x}");
        assert!(y.abs_diff(1080 - h - y) <= 1, "centrée en hauteur : marge {y}");
    }

    /// ⛔ **LA TAILLE DEMANDÉE NE DÉPASSE JAMAIS `PART_ECRAN` DE L'ÉCRAN.**
    ///
    /// Elle est DÉRIVÉE du contenu, donc RIEN ne la bornait à l'écran : sur un
    /// 1920×1080 elle faisait 1230 px de haut pour 1080 d'écran, et
    /// `centrer_sur_ecran` la collait à `y = 0` — **150 px sous le bord bas,
    /// inatteignables même en défilant**, puisque c'est la FENÊTRE qui sort de
    /// l'écran et non son contenu.
    ///
    /// ⚠️ **« DEMANDÉE » EST LE MOT QUI COMPTE, ET IL A ÉTÉ AJOUTÉ APRÈS REVUE.**
    /// Ce test porte sur `taille_pour_tout_le_contenu`, une fonction PURE. La
    /// fenêtre réellement montée, elle, ne reçoit ce rectangle qu'à sa PREMIÈRE
    /// monte (`cef_popup::faut_appliquer_le_rect` — « la place appartient à
    /// l'utilisateur »). Une fenêtre déjà ouverte quand l'écran change garde donc
    /// son ancien rectangle : le plafond n'est PAS une garantie sur la fenêtre
    /// montée, et le nom du test ne doit pas le laisser croire.
    ///
    /// ⛔ **ÉCHOUE SUR L'ANCIEN CODE** : à 1920×1080 il rendait `[1860, 1230]`.
    #[test]
    fn la_taille_demandee_ne_depasse_jamais_le_plafond_de_l_ecran() {
        // ⚠️ **LES TAILLES SONT ÉCRITES EN CLAIR, ET C'EST DÉLIBÉRÉ.**
        // `assert!(l <= l_ecran * PART_ECRAN / 100)` serait une TAUTOLOGIE : `l`
        // est construite À PARTIR de ce budget, donc `min(a, b) <= b` est vrai par
        // construction, avec la MÊME constante des deux côtés — muter `PART_ECRAN`
        // ne ferait rien tomber. Des littéraux, eux, échouent si le plafond change.
        for (l_ecran, h_ecran, attendu) in [
            (3840, 2160, [1860, 1230]), // le plafond ne mord pas
            (2560, 1440, [1860, 1224]), // 1440 × 85 % = 1224 : il mord de 6 px
            (1920, 1080, [1632, 918]),
            (1366, 768, [1161, 652]),
        ] {
            let [l, h] = taille_pour_tout_le_contenu(l_ecran, h_ecran);
            assert_eq!([l, h], attendu, "taille demandée sur {l_ecran}×{h_ecran}");
            // ⭐ **ET LA FENÊTRE POSÉE TIENT ENTIÈREMENT DANS L'ÉCRAN** — c'est la
            // propriété qui compte, et c'est elle qui n'était vraie nulle part
            // avant ce plafond.
            let [x, y, l2, h2] = centrer_sur_ecran(l_ecran, h_ecran, [l, h]);
            assert!(x >= 0 && y >= 0 && x + l2 <= l_ecran && y + h2 <= h_ecran, "hors écran : ({x},{y}) {l2}×{h2} sur {l_ecran}×{h_ecran}");
            // ⚠️ Et elle reste LISIBLE : borner ne doit pas rendre la fenêtre plus
            // étroite que deux touches, sinon on aurait échangé un débordement
            // contre une colonne unique. (Propriété des quatre résolutions
            // ci-dessus, PAS une garantie du code : un écran de 320 px rendrait
            // 272 px, soit moins de deux touches.)
            assert!(l >= 2 * TOUCHE * PX_PAR_UNITE, "trop étroite pour être utile : {l}");
        }
    }

    /// ⭐ **LA FENÊTRE CONTIENT TOUTE LA GRILLE.** C'est la demande explicite du
    /// 2026-10-06 : « faire s'afficher la fenêtre de façon à ce qu'elle affiche tout
    /// son contenu » — la fenêtre s'ouvrait à 480×340 pour un contenu qui réclame
    /// 466×356 unités de page, donc on n'en voyait que le tiers.
    ///
    /// ⛔ **CE TEST ÉCHOUE SUR L'ANCIEN CODE** : `LARGEUR`/`HAUTEUR` valaient
    /// `480`/`340` **physiques**, soit 160×113 unités de page.
    #[test]
    fn l_ouverture_montre_toute_la_grille() {
        let colonnes = crate::win32::caracteres::FAMILLES.iter().map(|l| l.len()).max().unwrap_or(0) as i32;
        let rangees = crate::win32::caracteres::FAMILLES.len() as i32;
        // ⚠️ **3840×2160, DONC LE PLAFOND NE MORD PAS** : ce test mesure le
        // CONTENU, pas le plafond — lequel a son propre test.
        let [l, h] = taille_pour_tout_le_contenu(3840, 2160);
        let marge = 2 * (MARGE + SOUPLESSE);
        assert_eq!([l, h], [(colonnes * TOUCHE + marge) * PX_PAR_UNITE, (BANDE + rangees * TOUCHE + marge) * PX_PAR_UNITE]);
        // La grille seule, en pixels : elle doit tenir dans la fenêtre, sous la bande.
        let (grille_l, grille_h) = (colonnes * TOUCHE * PX_PAR_UNITE, rangees * TOUCHE * PX_PAR_UNITE);
        assert!(grille_l + 2 * MARGE * PX_PAR_UNITE <= l, "trop étroite : {grille_l} dans {l}");
        assert!(BANDE * PX_PAR_UNITE + grille_h + 2 * MARGE * PX_PAR_UNITE <= h, "trop basse");
        // ⭐ Et la taille demandée est CELLE D'UN CONTENU, pas un chiffre rond : si
        // quelqu'un la re-figeait à la main, la grille la plus large cesserait de
        // définir la largeur, et l'égalité ci-dessus tomberait.
        assert_eq!(l / PX_PAR_UNITE, colonnes * TOUCHE + marge);
        // ⚠️ **ET LA SOUPLESSE EST INDISPENSABLE** : une fenêtre taillée EXACTEMENT
        // sur le contenu rouvre la barre de défilement que ce correctif ferme.
        assert!(SOUPLESSE > 0, "sans elle, l'arrondi du zoom ramène une barre de défilement");
    }

    /// ⚠️ **LE FACTEUR UNITÉS DE PAGE → PIXELS EST CELUI DU ZOOM DE LA PAGE.** Les
    /// deux sont écrits à deux endroits (une constante `f64` dans `cef_popup`, un
    /// entier ici) : sans ce test, changer le zoom des popups rendrait TOUTES les
    /// tailles de cette fenêtre fausses d'un facteur entier, en silence.
    #[test]
    fn le_facteur_de_la_page_est_bien_celui_du_zoom() {
        assert_eq!(PX_PAR_UNITE as f64, cef_popup::CONTENT_ZOOM_FACTOR);
    }

    /// ⭐ **LA PAGE ET RUST DÉCRIVENT LA MÊME TOUCHE.** `TOUCHE` et `MARGE` sont une
    /// duplication de la règle `.case` de `caracteres.html` — et une duplication
    /// **gardée** : ce test lit la page et échoue si les deux divergent.
    ///
    /// ⛔ **SANS LUI, LE DÉFAUT SERAIT SILENCIEUX** : rétrécir une touche dans la page
    /// ferait juste une fenêtre un peu trop grande (visible), mais l'AGRANDIR ferait
    /// réapparaître une grille qui déborde — la barre de défilement à l'ouverture, le
    /// défaut qu'on vient de corriger, sans qu'aucun test ne le dise.
    #[test]
    fn les_unites_de_la_page_sont_celles_du_calcul_rust() {
        let page = include_str!("../../assets/popups/caracteres.html");
        // La règle `.case {` — pas `.case:hover {`, que ce préfixe ne matche pas.
        let bloc = page.split(".case {").nth(1).expect("la règle `.case` de la page");
        let bloc = bloc.split('}').next().expect("une règle fermée");
        let px = |champ: &str| -> i32 {
            let apres = bloc.split(&format!("{champ}:")).nth(1).unwrap_or_else(|| panic!("`{champ}` absent de `.case`"));
            apres.trim_start().split("px").next().unwrap().trim().parse().unwrap_or_else(|_| panic!("`{champ}` sans px"))
        };
        assert_eq!(px("width") + 2 * px("margin"), TOUCHE, "la touche de la page a changé : mettre `TOUCHE` à jour");
        // La marge de la page, elle, doit être celle que le calcul compte.
        let grille = page.split("#grille {").nth(1).and_then(|b| b.split('}').next()).expect("la règle `#grille`");
        let apres = grille.split("padding:").nth(1).expect("le `padding` de `#grille`");
        let marge_page: i32 = apres.trim_start().split("px").next().unwrap().trim().parse().expect("un padding en px");
        assert_eq!(marge_page, MARGE, "la marge de la page a changé : mettre `MARGE` à jour");
    }
}
