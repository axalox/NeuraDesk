//! La **FENÊTRE** du Sélecteur de couleurs (voir `assets/popups/couleurs.html`).
//!
//! **UNE VRAIE FENÊTRE, PAS UN PANNEAU** — demande explicite. C'est le premier
//! `PopupSpec` à mettre `fenetre: true` depuis que l'éditeur d'image est devenu
//! un panneau (2025-09-25) : le mécanisme dormait, gardé exprès (voir la doc du
//! champ, `cef_popup::PopupSpec::fenetre`). Ce module le réveille, et deux
//! conséquences en découlent, chacune délibérée :
//!
//! 1. **`montrer_fenetre`, jamais `toggle`.** Cliquer l'applet d'une fenêtre
//!    ouverte la RAMÈNE devant, elle ne la ferme pas (voir `cef_popup::
//!    montrer_fenetre`). Une fenêtre se ferme par sa croix, Alt-F4, ou le
//!    bouton « Fermer » de la page.
//! 2. **La fenêtre n'est NI dans `app::hide_overlay_windows` NI dans
//!    `app::poll_overlay_windows`.** Ce n'est pas un oubli — voir le commentaire
//!    posé aux deux endroits.
//!
//! Actions reçues (`"couleurs/<verbe>/…"`, segments ENCODÉS) :
//! - `ready` — la page a chargé (voir `PAYLOAD`)
//! - `close`
//!
//! **DEUX FENÊTRES, UNE SEULE PIPETTE** — demande du 2026-10-27 : « quand la
//! pipette est sélectionnée, une fenêtre réduite avec le bouton pour sortir du
//! mode pipette et les couleurs sélectionnées uniquement ». La grande est
//! `couleurs.html`, la réduite `couleurs_pipette.html`, et elles partagent TOUT
//! leur état : `historique`, `actuel`, le fait que la pipette est armée. La
//! réduite n'existe **que** pendant le mode pipette : c'est `pip_update`, appelé
//! dans la frame, qui la monte quand la pipette s'arme et la cache quand elle
//! s'éteint — en rendant la grande à la sortie, si elle était ouverte. Sa croix,
//! Échap et le clic droit sont donc le MÊME geste, et c'est voulu : deux boutons
//! qui sortent du mode pipette s'offrent toujours en doublon douteux.
//!
//! Actions de la réduite (`"couleurs-pip/<verbe>/…"`) : `ready`, `sortir`,
//! `copier`, `actuel`, `historique-retirer`, `deplacer`.
//!
//! ⚠️ **LE CROCHET DÉCLARE DEUX HWND, ET C'EST CE QUI REND « Sortir »
//! PRESSABLE** : un crochet `WH_MOUSE_LL` avale TOUS les clics gauches du
//! bureau, sauf ceux de NOS fenêtres (`clic_nous_appartient`). Une seconde
//! fenêtre qu'on ne lui déclare pas a ses boutons **morts** pendant le mode
//! pipette, et Échap serait le seul recours — le défaut a été repéré avant
//! d'écrire la page, pas après.

use std::cell::{Cell, RefCell};

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DefWindowProcW, GetAncestor, GetSystemMetrics, GetWindowRect, SetWindowPos,
    SetWindowsHookExW, UnhookWindowsHookEx, WindowFromPoint, GA_ROOT, HC_ACTION, HHOOK, HTBOTTOM, HTBOTTOMLEFT,
    HTBOTTOMRIGHT, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, HWND_TOPMOST, MSLLHOOKSTRUCT,
    SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WH_MOUSE_LL, WM_CLOSE, WM_LBUTTONDOWN,
    WM_MOUSEMOVE, WM_NCHITTEST, WM_RBUTTONDOWN,
};

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// La clé du dernier `window.setState(...)` émis — voir
    /// `cef_calculatrice::PAYLOAD` : vidée à l'ouverture ET au `ready`.
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// **UNE MESURE EST DEMANDÉE, ET PAS ENCORE REÇUE** — voir `mesurer` côté
    /// page et le bras `["hauteur", …]` de `dispatch_actions`.
    ///
    /// ⚠️ **LE TÉMOIN EXISTE POUR QUE LA FENÊTRE NE SE REDIMENSIONNE PAS TOUTE
    /// SEULE.** Redimensionner à chaque état reçu écraserait le geste de
    /// l'utilisateur : la page repousse son état à chaque frame où il change
    /// (voir `refresh_if_visible`), et une fenêtre qu'on redimensionne à la main
    /// se remettrait à la taille du contenu au réglage suivant. On demande donc
    /// **une fois par ouverture**, et on referme le témoin en appliquant.
    static MESURER: Cell<bool> = const { Cell::new(false) };

    // --- La pipette. ⛔ **`Cell` UNIQUEMENT POUR CE QUI SUIT**, jamais un
    // `RefCell`, jamais d'allocation, jamais d'`execute_js` : c'est ce qui rend
    // le crochet incapable de paniquer en réentrance, et c'est aussi ce qui le
    // tient sous les 300 ms de `LowLevelHooksTimeout` — dont la sanction, quand
    // on la dépasse, n'est pas une erreur mais un crochet **retiré en silence**
    // (le symptôme serait « la pipette ne marche plus parfois »).
    static ARMEE: Cell<bool> = const { Cell::new(false) };
    static HOOK: Cell<Option<HHOOK>> = const { Cell::new(None) };
    /// Le dernier pixel prélevé, posé par le crochet et ramassé par `poll`.
    static RESULTAT: Cell<Option<[u8; 3]>> = const { Cell::new(None) };
    /// Le dernier point SURVOLÉ, posé par le crochet — c'est lui que la loupe
    /// suit (voir `poll`). Le survol n'est jamais avalé : on l'observe.
    static POINT_SOUS_CURSEUR: Cell<Option<POINT>> = const { Cell::new(None) };
    /// Un clic DROIT a annulé, posé par le crochet. Voir `souris`.
    static CLIC_DROIT_ANNULE: Cell<bool> = const { Cell::new(false) };
    /// Le HWND de notre fenêtre, écrit par `poll` — en `isize` et non en
    /// `HWND` : les dérivations de `HWND` ont bougé d'une version de
    /// `windows-rs` à l'autre, un `Cell` d'entier est sans ambiguïté.
    static NOTRE_HWND: Cell<isize> = const { Cell::new(0) };
    /// La couleur courante, et rien d'autre pour l'instant : l'historique
    /// arrive au lot suivant.
    static ACTUEL: RefCell<Option<[u8; 3]>> = const { RefCell::new(None) };
    /// **LES COULEURS PRÉLEVÉES, EN MÉMOIRE SEULEMENT** — demande explicite.
    /// La plus récente en tête.
    static HISTORIQUE: RefCell<Vec<[u8; 3]>> = const { RefCell::new(Vec::new()) };
    /// **LA PALETTE QU'ON CONSTRUIT** — c'est ELLE qu'on enregistre et qu'on
    /// exporte, et c'est elle qui reçoit ce qu'un `.gpl` chargé apporte.
    ///
    /// ⚠️ **DISTINCTE DE L'HISTORIQUE, ET PAS PAR GOÛT DE LA SÉPARATION** : un
    /// `.gpl` peut porter des centaines de couleurs, l'historique est plafonné à
    /// `HISTORIQUE_MAX`. Les confondre tronquerait un fichier chargé **sans le
    /// dire** — le pire des défauts pour un outil d'échange de palettes, celui
    /// qu'on ne découvre qu'en rouvrant le fichier dans GIMP.
    static PALETTE: RefCell<Vec<[u8; 3]>> = const { RefCell::new(Vec::new()) };
    /// Les noms de la bibliothèque, **RELUS après chaque écriture ou
    /// suppression, jamais à chaque frame**. `etat_json` est appelé à la
    /// fréquence de l'écran : y lire le dossier serait une lecture de disque
    /// par frame.
    static BIBLIOTHEQUE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// **LEVIER DE VÉRIFICATION DU HARNAIS** — `NEURADESK_DEBUG_COULEURS_GPL=<nom>`
    /// charge le `.gpl` de ce nom à la PREMIÈRE ouverture, pour que la capture
    /// montre des pastilles **mémorisées** (avec leur code et leur ✕) : `PALETTE`
    /// ne survit pas au processus, elle ne vit que dans les fichiers, donc un
    /// lancement de harnais part vide et ne dirait RIEN du nouveau rendu.
    ///
    /// ⚠️ **UNE SEULE FOIS** : `montrer` est rappelé à chaque clic sur l'icône, et
    /// `ajouter_gpl` **AJOUTE** — un levier réappliqué doublerait la palette à
    /// chaque ouverture et la capture mentirait sur le contenu.
    static GPL_AMORCE: Cell<bool> = const { Cell::new(false) };

    // --- La fenêtre réduite du mode pipette -----------------------------------
    /// Le slot de la fenêtre réduite — un SECOND navigateur, donc un second
    /// `PopupSlot`, une seconde classe de fenêtre et une seconde file d'actions.
    static SLOT_PIP: PopupSlot = const { PopupSlot::new() };
    /// Sa clé de payload, séparée de `PAYLOAD` : les deux pages ne reçoivent pas
    /// le même JSON, et une clé partagée ferait croire à l'une que l'autre a déjà
    /// reçu l'état.
    static PAYLOAD_PIP: RefCell<Option<String>> = const { RefCell::new(None) };
    /// **La réduite est montée PAR LE MODE PIPETTE.** `Cell` et non
    /// « est-elle visible ? » : `pip_update` doit savoir s'il a DÉJÀ fait le
    /// transition, sans la refaire à chaque frame (elle cacherait la grande
    /// fenêtre que l'utilisateur vient de rouvrir lui-même).
    ///
    /// ⚠️ **`debug_montrer_pip` ne la pose PAS** : le harnais ouvre la réduite
    /// sans armer la pipette, et `pip_update` l'aurait cachée à la frame
    /// suivante sans ce garde — la capture n'aurait rien montré.
    static PIP_MONTEE: Cell<bool> = const { Cell::new(false) };
    /// La grande fenêtre était visible au moment où la pipette s'est armée : la
    /// sortie la rend. Posé par `pip_update`, lu par lui — `desarmer` peut être
    /// appelé depuis un `wndproc`, qui ne doit rien emprunter d'autre.
    static GRAND_ETAIT: Cell<bool> = const { Cell::new(false) };
    /// Le HWND de la fenêtre réduite, écrit par `poll` — le second que le crochet
    /// doit accepter (voir la doc du module).
    static NOTRE_HWND_PIP: Cell<isize> = const { Cell::new(0) };
}

/// Nombre maximal de couleurs gardées dans l'historique. Au-delà, les plus
/// anciennes sont oubliées — l'historique n'est pas une archive.
const HISTORIQUE_MAX: usize = 24;

/// Plafond de la palette construite. Généreux à dessein : c'est un garde-fou
/// contre un fichier absurde, pas une politique — une palette de 300 couleurs
/// est parfaitement normale.
const PALETTE_MAX: usize = 1024;

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCouleurs"),
    window_title: w!("NeuraDesk — Sélecteur de couleurs"),
    html_file: "couleurs.html",
    wndproc: Some(wndproc),
    // Sans objet pour une fenêtre, dont le contrat de pile est celui de
    // Windows : le champ ne parle que du classement face au dock.
    above_a_magnified_dock: false,
    // ⛔ `false`, et ce n'est pas un détail : pour une `fenetre`,
    // `create_window` REMPLACE `ex_style` (donc `WS_EX_LAYERED` disparaît, et
    // avec lui tout fondu possible) mais recopie tout de même le drapeau dans
    // `CefPopupState::fade`. Les laisser diverger donnerait un fondu qui se
    // croit armé.
    fades: false,
    fenetre: true,
};

/// **LA FENÊTRE RÉDUITE DU MODE PIPETTE** — même famille que `SPEC` : une vraie
/// fenêtre (`WS_POPUP`, barre dessinée par la page, redimensionnement rendu par
/// son `wndproc`), avec une `wndproc` à elle parce que `cef_popup` en exige une
/// par classe de fenêtre.
///
/// ⚠️ **ELLE N'EST PAS DANS `app::preload_queue`**, et ce n'est pas un oubli :
/// `preload` CRÉE la fenêtre à `PRELOAD_RECT` (1024×768 hors écran), et
/// `montrer_fenetre` ne repose le `rect` qu'à la première monte — donc une
/// fenêtre préchargée s'ouvrirait au coin en 1024×768, le `rect` calculé étant
/// jeté en silence. Le défaut est décrit en entier dans `cef_popup::
/// montrer_fenetre` ; `cef_couleurs` et `cef_visionneuse` ne le voyaient pas
/// justement parce qu'elles ne préchargent pas.
const SPEC_PIP: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCouleursPip"),
    window_title: w!("NeuraDesk — Pipette"),
    html_file: "couleurs_pipette.html",
    wndproc: Some(wndproc_pip),
    above_a_magnified_dock: false,
    fades: false,
    fenetre: true,
};

/// Largeur de la réduite, en unités de page : une COLONNE étroite, parce qu'elle
/// ne porte que les prélèvements.
const LARGEUR_PIP_UNITES: i32 = 300;

/// Sa hauteur, en unités, **FIXE** — et c'est la différence avec la grande
/// fenêtre, dont la hauteur est MESURÉE par la page (`appliquer_hauteur`). La
/// réduite ne demande aucune mesure : une fenêtre qui réclame une hauteur lutte
/// contre le redimensionnement de l'utilisateur, et sa liste défile.
///
/// Le compte vient des `HISTORIQUE_MAX` teintes en colonnes de 58 unités : à
/// 300 unités de large, quatre par rangée, six rangées. Si la liste dépasse,
/// `main` défile — ce n'est pas un défaut à corriger par un nombre plus grand.
const HAUTEUR_PIP_UNITES: i32 = 360;

/// Ce qui la décolle des bords, **en pixels physiques** : une marge d'écran, pas
/// une unité de page, parce qu'elle ne grossit pas avec les textes.
const MARGE_PIP: i32 = 16;

/// Taille de la fenêtre à sa **création**, et à ce moment-là seulement :
/// ensuite c'est l'utilisateur qui possède la position et la taille, et
/// `render` suit la zone cliente (voir `PopupSpec::fenetre`).
///
/// ⚠️ **CHANGER CES VALEURS NE REDIMENSIONNE PAS UNE FENÊTRE DÉJÀ OUVERTE** :
/// `montrer_fenetre` ne lit `rect` qu'à la création, et la fenêtre n'est que
/// CACHÉE par sa croix — seul un redémarrage la recrée. Défaut signalé deux
/// fois (« la fenêtre n'est pas assez agrandie à l'ouverture », puis « trop
/// petite quand l'outil pipette s'ouvre ») : la valeur a été relevée les deux
/// fois, et elle ne prend effet qu'au lancement suivant.
/// **EN UNITÉS DE PAGE, PAS EN PIXELS — et c'est tout le correctif du 2026-10-08.**
///
/// ⛔ **CE QUI N'ALLAIT PAS.** `640`/`880` étaient passés TELLES QUELLES à
/// `SetWindowPos`, alors que la page est rendue à `cef_popup::
/// CONTENT_ZOOM_FACTOR` (×3). Une fenêtre de 640×880 **physiques** ne donnait
/// donc au contenu que **213×293 unités**, dont 72 pour la barre de titre — et
/// `main`, seule partie défilante de la page, écopait du reste. D'où le
/// signalement : « la fenêtre ne montre pas tout son contenu ».
///
/// ⚠️ **ET LE NOMBRE AVAIT DÉJÀ ÉTÉ RELEVÉ DEUX FOIS À LA MAIN** (« pas assez
/// agrandie à l'ouverture », puis « trop petite quand l'outil pipette
/// s'ouvre »). Relever un nombre ne corrige pas une **unité** fausse : c'est le
/// défaut qui a coûté la même manche à `cef_caracteres` (voir son test
/// `l_ouverture_montre_toute_la_grille`, qui « échoue sur l'ancien code » pour
/// exactement cette raison) et à `cef_kaomoji`.
///
/// ⭐ **LA LARGEUR EST UN CHOIX, LA HAUTEUR SE MESURE.** Un document qui COULE
/// n'a pas de hauteur calculable ici : elle dépend de l'historique, de la
/// palette et du nombre de groupes. La page se mesure donc elle-même à
/// l'ouverture et renvoie ce que son contenu réclame (voir `mesurer` côté page
/// et le bras `["hauteur", …]` de `dispatch_actions`).
const LARGEUR_UNITES: i32 = 560;

/// La hauteur d'ouverture, en unités — **PROVISOIRE**, le temps que la mesure
/// arrive : elle n'est là que pour que la première image ne soit pas ridicule.
/// Voir `LARGEUR_UNITES`.
const HAUTEUR_UNITES: i32 = 600;

/// **LA BORNE HAUTE DE LA MESURE** — la page annonce sa hauteur, et on la
/// croit… jusqu'ici. Une page qui annoncerait dix millions d'unités produirait
/// une fenêtre impossible à rattraper, et `taille_pour_ecran` la raboterait de
/// toute façon à `PART_MAX` — mais autant refuser le chiffre absurde que de le
/// laisser voyager. **Large** : c'est un garde-fou, pas un réglage.
const HAUTEUR_MAX_UNITES: i32 = 4000;

/// Part maximale de l'écran qu'une fenêtre peut occuper : au-delà, on garde la
/// borne et le contenu défile. Un nombre fixe finit toujours par être trop grand
/// quelque part ; une borne, non.
const PART_MAX: f32 = 0.9;

/// **Fonction PURE** : la taille PHYSIQUE pour un écran et des unités de page
/// donnés. Bornée à `PART_MAX`, et jamais nulle — même pour des entrées absurdes.
pub(crate) fn taille_pour_ecran(
    largeur_ecran: i32,
    hauteur_ecran: i32,
    unites: (i32, i32),
) -> (i32, i32) {
    // ⚠️ **LE FACTEUR EST CELUI DU ZOOM DE LA PAGE, ET IL EST LU, PAS RECOPIÉ** —
    // même règle que `cef_caracteres::PX_PAR_UNITE`, dont un test ancre la valeur
    // au zoom des popups : recopier le 3 rendrait toutes ces tailles fausses d'un
    // facteur entier, en silence, le jour où le zoom bouge.
    let px = |unites: i32| {
        ((unites as f64 * crate::win32::cef_popup::CONTENT_ZOOM_FACTOR).round() as i32).max(1)
    };
    let part = |cote: i32, voulu: i32| voulu.min((cote as f32 * PART_MAX) as i32).max(1);
    (part(largeur_ecran, px(unites.0)), part(hauteur_ecran, px(unites.1)))
}

/// Ouvre la fenêtre, ou la **ramène devant** si elle existe déjà. Rend `true`
/// si elle vient d'être CRÉÉE (voir `cef_popup::montrer_fenetre`, dont la
/// valeur de retour a ce sens-là et pas celui de « visible »).
pub fn montrer(gfx: &GfxDevice, content_scale: f32) -> bool {
    // ⭐ La taille des textes GLOBALE (`taille_panneaux::TEXTES`) : cette fenêtre
    // ne passe pas par `app::content_scale_pour`, elle la prend donc ici.
    let globale = crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES);
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("couleurs") * globale));
    // L'état est repoussé en entier à chaque ouverture : la page a pu survivre
    // à une fermeture, et rien ne garantit qu'elle affiche encore le nôtre.
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    // ⭐ **ON DEMANDE LA MESURE À CHAQUE OUVERTURE** — la page rendra la hauteur
    // que son contenu réclame, et `appliquer_hauteur` refermera ce témoin. C'est
    // le SEUL moment où la fenêtre se redimensionne d'elle-même : une fois par
    // ouverture, jamais après, sinon le geste de l'utilisateur serait écrasé.
    MESURER.with(|c| c.set(true));
    let creee = cef_popup::montrer_fenetre(&SLOT, &SPEC, rect_defaut(), gfx);
    au_dessus();
    // La bibliothèque est relue ICI, à l'ouverture — pas à chaque frame (voir
    // `BIBLIOTHEQUE`). C'est le seul moment où elle a pu changer sans nous.
    rafraichir_bibliotheque();
    // Le levier du harnais, **après** la bibliothèque relue et **avant**
    // `push_state` : il écrit dans `PALETTE`, donc l'état envoyé doit le porter.
    if let Some(nom) = std::env::var("NEURADESK_DEBUG_COULEURS_GPL")
        .ok()
        .filter(|n| !n.is_empty())
    {
        if !GPL_AMORCE.with(|c| c.get()) {
            GPL_AMORCE.with(|c| c.set(true));
            gpl_charger(&nom);
        }
    }
    push_state();
    creee
}

/// Cache la fenêtre sans la détruire — la page survit, avec son historique.
/// Voir `cef_popup::recaler_si_il_ne_tient_plus` — ⚠️ cette fenêtre s'ouvre
/// toujours sur la PRIMAIRE (`rect_defaut`), donc un changement de résolution de
/// celle-ci est le seul cas qui la concerne.
pub fn recaler_si_il_ne_tient_plus() {
    cef_popup::recaler_si_il_ne_tient_plus(&SLOT);
    // La réduite est de la même famille, et son défaut est le même : passée d'un
    // 4K à un 1080 pendant qu'elle était cachée, son `rect` périmé la ferait
    // remonter hors d'atteinte (`montee` empêche `montrer_fenetre` de le reposer).
    cef_popup::recaler_si_il_ne_tient_plus(&SLOT_PIP);
}

pub fn cacher() {
    // ⛔ **LA PIPETTE SE DÉSARME ICI, ET C'EST UNE QUESTION DE SÛRETÉ** : un
    // crochet souris global qui survit à la fenêtre qu'il sert avale les clics
    // de TOUT LE SYSTÈME, sans aucune interface pour l'arrêter.
    desarmer();
    if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
        cef_popup::cacher_fenetre(hwnd);
    }
}

// --- La palette construite, et les fichiers `.gpl` ---------------------------

/// Relit la liste des palettes enregistrées. Appelée à l'ouverture et après
/// chaque écriture ou suppression — **jamais par frame** (voir `BIBLIOTHEQUE`).
fn rafraichir_bibliotheque() {
    let noms = crate::win32::palette_gpl::lister();
    BIBLIOTHEQUE.with(|b| *b.borrow_mut() = noms);
}

/// La palette courante, au format GIMP.
///
/// Les couleurs sont écrites **sans nom** : GIMP accepte un nom vide, et y
/// mettre l'hexadécimal ne dirait rien de plus que la couleur elle-même.
fn contenu_gpl(nom: &str) -> String {
    let couleurs = PALETTE.with(|p| p.borrow().clone());
    crate::win32::palette_gpl::vers_gpl(&crate::win32::palette_gpl::PaletteGpl {
        nom: nom.to_string(),
        colonnes: 16,
        couleurs: couleurs
            .into_iter()
            .map(|rgb| crate::win32::palette_gpl::CouleurGpl { rgb, nom: String::new() })
            .collect(),
    })
}

/// **Ajoute à la palette les couleurs d'un texte `.gpl`** — sans jamais
/// remplacer ce qui est déjà là (demande explicite). Rend le nombre de couleurs
/// réellement ajoutées.
fn ajouter_gpl(texte: &str, origine: &str) -> usize {
    match crate::win32::palette_gpl::depuis_gpl(texte) {
        Ok((palette, rapport)) => {
            // ⚠️ **ON LE DIT.** Perdre des couleurs en silence est le seul vrai
            // défaut de la tolérance du lecteur (voir `palette_gpl`, doc de
            // module) : un fichier de 41 couleurs avec une coquille en charge
            // 40, et il faut que ça se sache.
            if rapport.lignes_ignorees > 0 {
                crate::diag!("[couleurs] {origine} : {} ligne(s) ignorée(s)", rapport.lignes_ignorees);
            }
            let ajout: Vec<[u8; 3]> = palette.couleurs.iter().map(|c| c.rgb).collect();
            let (nouvelle, ajoutees) = PALETTE.with(|p| palette_apres_lot(&p.borrow(), &ajout));
            PALETTE.with(|p| *p.borrow_mut() = nouvelle);
            if ajoutees < ajout.len() {
                crate::diag!("[couleurs] {origine} : {} couleur(s) au-delà du plafond", ajout.len() - ajoutees);
            }
            ajoutees
        }
        Err(erreur) => {
            crate::diag!("[couleurs] {origine} n'est pas une palette : {erreur:?}");
            0
        }
    }
}

/// Ajoute UNE couleur (la courante, ou une pastille d'harmonie).
fn palette_ajouter(hex: &str) {
    let Some(rgb) = rgb_depuis_hex(hex) else {
        crate::diag!("[couleurs] couleur illisible : {hex:?}");
        return;
    };
    // ⛔ **CALCULER AVANT, ÉCRIRE APRÈS.** Écrire `*p.borrow_mut() =
    // palette_apres(&p.borrow(), rgb)` tenait l'emprunt MUTABLE pendant l'appel :
    // `RefCell` panique en « already borrowed », et ici ce fil est celui du
    // crochet et de la boucle — un clic sur « Ajouter la couleur courante » tuait
    // l'app. Mesuré par `palette_ajouter_ne_rentre_pas_dans_sa_propre_emprunt`,
    // qui rouge sur la forme précédente. Deux `with` distincts, comme dans `poll`.
    let nouvelle = PALETTE.with(|p| palette_apres(&p.borrow(), rgb));
    PALETTE.with(|p| *p.borrow_mut() = nouvelle);
}

/// Verse un GROUPE d'harmonies entier dans la palette.
///
/// ⚠️ **UN SEUL aller-retour, jamais un par couleur** : deux `location.href`
/// dans la même tâche se TÉLESCOPENT **en silence** (le dépôt l'a déjà payé —
/// voir la fiche « panneau ou menu »), donc seule la dernière couleur du groupe
/// arriverait. La page envoie la liste jointe par des virgules, et c'est ici
/// qu'on la découpe.
fn palette_ajouter_groupe(teintes: &str) {
    let ajout: Vec<[u8; 3]> = teintes.split(',').filter_map(rgb_depuis_hex).collect();
    let (nouvelle, _) = PALETTE.with(|p| palette_apres_lot(&p.borrow(), &ajout));
    PALETTE.with(|p| *p.borrow_mut() = nouvelle);
}

fn palette_retirer(index: &str) {
    let Ok(index) = index.parse::<usize>() else {
        crate::diag!("[couleurs] index illisible : {index:?}");
        return;
    };
    PALETTE.with(|p| {
        let mut p = p.borrow_mut();
        if index < p.len() {
            p.remove(index);
        }
    });
}

/// Retire UNE couleur de l'historique des prélèvements. La palette avait son ✕,
/// l'historique en était dépourvu : on ne pouvait qu'en subir le plafond de
/// `HISTORIQUE_MAX` — y compris quand la couleur qu'on voudrait garder vient
/// d'arriver et qu'une ancienne encombre la liste.
fn historique_retirer(index: &str) {
    let Ok(index) = index.parse::<usize>() else {
        crate::diag!("[couleurs] index illisible : {index:?}");
        return;
    };
    // Même garde qu'à l'ajout : CALCULER AVANT, écrire après. Le `borrow_mut` ne
    // doit jamais tenir pendant un `borrow` du même cell.
    let nouvelle = HISTORIQUE.with(|h| historique_sans(&h.borrow(), index));
    HISTORIQUE.with(|h| *h.borrow_mut() = nouvelle);
}

/// **AVANT TOUTE BOÎTE DE DIALOGUE** — et elle sert à DEUX choses.
///
/// On désarme la pipette : le dialogue est **bloquant** et pompe ses propres
/// messages, donc le crochet recevrait du mouvement de souris pendant qu'il est
/// ouvert, alors que l'utilisateur ne vise plus rien. La loupe s'en va avec lui
/// (voir `retirer_le_crochet`).
///
/// Et on rend le propriétaire : une boîte **sans propriétaire** peut s'ouvrir
/// DERRIÈRE la fenêtre, et l'application paraît alors figée pendant qu'elle
/// attend un choix invisible. `None` avant la première ouverture — on refuse
/// plutôt que d'ouvrir une boîte orpheline.
fn avant_dialogue() -> Option<HWND> {
    desarmer();
    let hwnd = cef_popup::hwnd(&SLOT);
    if hwnd.is_none() {
        crate::diag!("[couleurs] boite de dialogue refusee : la fenetre n'existe pas encore");
    }
    hwnd
}

fn gpl_enregistrer(nom: &str) {
    if crate::win32::palette_gpl::ecrire(nom, &contenu_gpl(nom)) {
        rafraichir_bibliotheque();
    } else {
        crate::diag!("[couleurs] palette {nom:?} refusee (nom invalide) ou non ecrite");
    }
}

fn gpl_charger(nom: &str) {
    match crate::win32::palette_gpl::lire(nom) {
        Some(texte) => {
            ajouter_gpl(&texte, nom);
        }
        None => crate::diag!("[couleurs] palette {nom:?} introuvable"),
    }
}

fn gpl_supprimer(nom: &str) {
    if crate::win32::palette_gpl::supprimer(nom) {
        rafraichir_bibliotheque();
    } else {
        crate::diag!("[couleurs] palette {nom:?} : suppression impossible");
    }
}

fn gpl_importer() {
    let Some(hwnd) = avant_dialogue() else { return };
    let Some(chemin) =
        crate::win32::file_dialog::ouvrir(hwnd, "Importer une palette", "gpl", "Palette GIMP (*.gpl)")
    else {
        return; // Annuler n'est pas une erreur : rien à journaliser.
    };
    match std::fs::read(&chemin) {
        // Lu en OCTETS puis `from_utf8_lossy`, comme la bibliothèque : un nom
        // Latin-1 écrit par un vieil Inkscape ne doit pas coûter la palette.
        Ok(octets) => {
            ajouter_gpl(&String::from_utf8_lossy(&octets), &chemin.display().to_string());
        }
        Err(erreur) => crate::diag!("[couleurs] import impossible : {erreur}"),
    }
}

fn gpl_exporter(nom: &str) {
    let Some(hwnd) = avant_dialogue() else { return };
    let propose = format!("{nom}.gpl");
    let Some(chemin) = crate::win32::file_dialog::enregistrer_sous(
        hwnd,
        "Exporter la palette",
        &propose,
        "gpl",
        "Palette GIMP (*.gpl)",
    ) else {
        return;
    };
    if let Err(erreur) = std::fs::write(&chemin, contenu_gpl(nom)) {
        crate::diag!("[couleurs] export impossible : {erreur}");
    }
}

/// **TOUJOURS AU-DESSUS** — demandé explicitement.
///
/// `fenetre: true` donne délibérément une fenêtre NI topmost NI toolwindow
/// (voir `PopupSpec::fenetre`) ; on rétablit ici le seul des deux qui compte
/// pour une pipette : sans lui, la fenêtre disparaîtrait sous la première
/// application qu'on vient échantillonner, et l'on ne verrait plus la couleur
/// qu'on est en train de choisir.
///
/// Le style n'est **pas** ajouté dans `cef_popup::create_window` : c'est le
/// mécanisme partagé de treize popups, et un réglage propre à une applet n'a
/// rien à y faire. Posé à CHAQUE ouverture et non seulement à la création —
/// l'appel est idempotent, et il remet la fenêtre au-dessus si quelque chose
/// l'a déclassée entre-temps.
fn au_dessus() {
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return };
    unsafe {
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// La réduite, **au-dessus de la grande** : pendant le mode pipette c'est elle
/// qu'on regarde, et la même raison qu'à `au_dessus` s'applique — sans topmost,
/// elle passerait sous l'application qu'on vient échantillonner.
fn au_dessus_pip() {
    let Some(hwnd) = cef_popup::hwnd(&SLOT_PIP) else { return };
    unsafe {
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// La largeur, en unités, à la taille du contenu : la page est zoomée de
/// `CONTENT_SCALE`, sa fenêtre doit l'être autant (la hauteur, elle, est mesurée
/// par la page, déjà multipliée — voir `mesurer` côté page).
fn largeur_unites() -> i32 {
    ((LARGEUR_UNITES as f32 * CONTENT_SCALE.with(Cell::get)).round() as i32).max(1)
}

/// Une fenêtre centrée sur l'écran PRINCIPAL. **Pas `dash_rect`** : une fenêtre
/// n'occupe pas la zone libre du dock, elle a sa propre place.
fn rect_defaut() -> [i32; 4] {
    let (largeur, hauteur) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let (l, h) = taille_pour_ecran(largeur, hauteur, (largeur_unites(), HAUTEUR_UNITES));
    [((largeur - l) / 2).max(0), ((hauteur - h) / 2).max(0), l, h]
}

/// La largeur de la réduite en unités, à la taille du contenu — la même règle
/// que `largeur_unites` pour la grande fenêtre : la page est zoomée de
/// `CONTENT_SCALE`, sa fenêtre doit l'être autant.
fn largeur_pip_unites() -> i32 {
    ((LARGEUR_PIP_UNITES as f32 * CONTENT_SCALE.with(Cell::get)).round() as i32).max(1)
}

/// **Fonction PURE** : le `[x, y, largeur, hauteur]` PHYSIQUE de la fenêtre
/// réduite, collée en **bas à droite**, à `MARGE_PIP` du bord.
///
/// ⭐ **POURQUOI CE COIN, ET PAS LE CENTRE DE `rect_defaut`.** Cette fenêtre
/// sert à prélever des pixels n'importe où sur le bureau. Une fenêtre centrée
/// recouvrirait justement ce qu'on vise, et le prélèvement rendrait la couleur
/// de SA bordure — le pire des défauts pour une pipette, parce qu'il donne une
/// valeur plausible et fausse. Le bas à droite est le coin où l'on vise le moins
/// souvent, et sa barre la déplace pour la séance.
///
/// Les `.max(0)` ne sont pas du décor : sur un écran plus petit que la fenêtre,
/// la marge ferait passer le coin en négatif, et une fenêtre à coordonnées
/// négatives est inaccessible — il n'y aurait plus que la fermer.
fn rect_pip_pour(largeur_ecran: i32, hauteur_ecran: i32, unites: (i32, i32)) -> [i32; 4] {
    let (l, h) = taille_pour_ecran(largeur_ecran, hauteur_ecran, unites);
    [
        (largeur_ecran - l - MARGE_PIP).max(0),
        (hauteur_ecran - h - MARGE_PIP).max(0),
        l,
        h,
    ]
}

fn rect_pip() -> [i32; 4] {
    let (largeur, hauteur) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    rect_pip_pour(largeur, hauteur, (largeur_pip_unites(), HAUTEUR_PIP_UNITES))
}

/// Repousse l'état si **l'une des deux** fenêtres est visible — à chaque frame
/// (voir `app::run`) ; les clés de payload rendent l'appel gratuit tant que rien
/// ne change. Les deux fenêtres partagent le même état, donc un seul appel suffit.
pub fn refresh_if_visible() {
    if cef_popup::is_visible(&SLOT) || cef_popup::is_visible(&SLOT_PIP) {
        push_state();
    }
}

// --- LA PIPETTE ---------------------------------------------------------------
//
// Un crochet souris BAS NIVEAU (`WH_MOUSE_LL`), comme celui des Réglages, et
// pour la raison que sa doc d'origine donne : un simple sondage de
// `GetCursorPos`/`GetAsyncKeyState` se contente d'OBSERVER qu'un clic a eu
// lieu, il ne peut pas empêcher ce même clic d'atteindre AUSSI la fenêtre
// réellement sous le curseur — un vrai risque d'effet de bord (cliquer un lien,
// un bouton…) juste pour choisir une couleur. Le crochet, lui, s'exécute AVANT
// que Windows ne distribue le clic, et peut l'avaler complètement.
//
// ⚠️ **CE MODULE EST LE SECOND `WH_MOUSE_LL` DU DÉPÔT**, l'autre étant celui
// des Réglages. Si les deux pipettes sont armées en même temps, les deux
// crochets tournent et enregistrent tous deux le clic. Bénin et auto-limité
// (chaque prélèvement désarme la Réglages), et non corrigeable sans toucher un
// fichier hors périmètre.

/// **TOTALE, ET TESTABLE** : le clic appartient-il à l'UNE DE NOS FENÊTRES ?
///
/// `racine` est ce que rend `GetAncestor(WindowFromPoint(pt), GA_ROOT)` — la
/// normalisation est faite par l'appelant, parce qu'elle touche Win32 et ne se
/// prouve donc qu'à la main, en cliquant. `0` = aucune fenêtre sous le point
/// (ou pas encore de fenêtre chez nous) : le clic ne nous appartient pas.
///
/// ⛔ **DEUX HWND, ET LE SECOND N'EST PAS UN DÉTAIL.** Le module a deux fenêtres
/// pendant le mode pipette : la grande (rouverte par la sortie) et la réduite,
/// qui porte le bouton « Sortir ». Accepter un seul HWND rendait tous les
/// boutons de la seconde **morts** pendant le mode pipette — le crochet ayant
/// juré d'avaler tous les clics gauches du bureau, un clic sur sa propre
/// fenêtre réduite serait avalé comme les autres.
pub(crate) fn clic_nous_appartient(nos_hwnds: [isize; 2], racine: isize) -> bool {
    nos_hwnds.iter().any(|hwnd| *hwnd != 0 && racine == *hwnd)
}

/// `#RRGGBB` — **AVEC le dièse** : c'est la forme qu'on colle dans un éditeur,
/// une feuille de style ou une boîte de dialogue. Le `hex()` interne du dépôt
/// l'omet (`cef_settings.rs:2604`) parce qu'il alimente des fichiers de
/// configuration ; ce n'est pas le cas ici.
pub(crate) fn hex_depuis_rgb(rgb: [u8; 3]) -> String {
    let [r, g, b] = rgb;
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// Teinte en degrés (0..360), saturation et clarté en pour cent — pour
/// l'AFFICHAGE seulement.
///
/// **Réutilise `ui::color::rgb_to_hsl`**, seule conversion HSL du dépôt. Une
/// seconde implémentation donnerait d'autres nombres pour la même couleur, et
/// c'est précisément ce que le refactor de `compute_palette` a fermé.
pub(crate) fn hsl_depuis_rgb(rgb: [u8; 3]) -> (f32, f32, f32) {
    let (h, s, l) = crate::ui::color::rgb_to_hsl(rgb.map(|c| c as f32 / 255.0));
    (h, s * 100.0, l * 100.0)
}

/// `[f32; 3]` (canaux 0..1) → `[u8; 3]` — pour ramener au 8 bits ce que
/// `ui::color::harmonies` rend en flottants.
fn rgb_u8(c: [f32; 3]) -> [u8; 3] {
    c.map(crate::ui::color::to_u8)
}

/// **L'HISTORIQUE APRÈS UN PRÉLÈVEMENT — FONCTION PURE.**
///
/// Un doublon **REMONTE en tête** au lieu d'apparaître deux fois : c'est ce
/// qu'on attend d'un historique de prélèvement, où l'on reprend souvent la même
/// couleur juste après l'avoir prise (pour la comparer, ou après un aller-retour
/// dans une autre application). Au-delà du plafond, la plus ancienne tombe.
pub(crate) fn historique_apres(liste: &[[u8; 3]], rgb: [u8; 3]) -> Vec<[u8; 3]> {
    let mut out = liste.to_vec();
    out.retain(|c| *c != rgb);
    out.insert(0, rgb);
    out.truncate(HISTORIQUE_MAX);
    out
}

/// **L'HISTORIQUE APRÈS QU'ON EN RETIRE UNE COULEUR — FONCTION PURE.** Rend la
/// liste INCHANGÉE si l'index n'y est plus : la page envoie un index lu sur un
/// état qui a pu changer entre-temps (un prélèvement REMONTE la liste, voir
/// `historique_apres`), et dans ce cas « retirer » ne doit pas devenir « retirer
/// la dernière » — un clic mal placé effacerait une couleur qu'on visait pas.
pub(crate) fn historique_sans(liste: &[[u8; 3]], index: usize) -> Vec<[u8; 3]> {
    let mut out = liste.to_vec();
    if index < out.len() {
        out.remove(index);
    }
    out
}

/// **LES GROUPES D'HARMONIES, EN HEXA — FONCTION PURE.** Rend `(nom, teintes)`
/// dans l'ordre fixe de `ui::color::harmonies`, ou rien du tout tant qu'aucune
/// couleur n'a été prélevée.
///
/// La conversion vit ICI et pas dans la page : elle y serait une seconde
/// implémentation de l'arrondi 0..1 → 0..255, qui divergerait de celle du
/// dépôt sans que rien ne le signale.
pub(crate) fn groupes_pour(rgb: Option<[u8; 3]>) -> Vec<(String, Vec<String>)> {
    let Some(rgb) = rgb else { return Vec::new() };
    crate::ui::color::harmonies(rgb.map(|c| c as f32 / 255.0))
        .into_iter()
        .map(|g| (g.nom.to_string(), g.teintes.iter().map(|t| hex_depuis_rgb(rgb_u8(*t))).collect()))
        .collect()
}

/// **LA PALETTE APRÈS UN AJOUT — FONCTION PURE.** Rend la liste INCHANGÉE si le
/// plafond est atteint : ajouter en silence en jetant une autre couleur serait
/// pire que ne rien faire, parce que ça se verrait sans qu'on sache pourquoi.
pub(crate) fn palette_apres(liste: &[[u8; 3]], rgb: [u8; 3]) -> Vec<[u8; 3]> {
    palette_apres_lot(liste, &[rgb]).0
}

/// **LA PALETTE APRÈS UN AJOUT DE PLUSIEURS COULEURS — FONCTION PURE.** C'est
/// ce qu'apporte un `.gpl` chargé. Rend la nouvelle liste ET le nombre de
/// couleurs réellement ajoutées — tronquées au plafond, donc pas forcément
/// celui qu'on a demandé.
pub(crate) fn palette_apres_lot(liste: &[[u8; 3]], ajout: &[[u8; 3]]) -> (Vec<[u8; 3]>, usize) {
    let mut out = liste.to_vec();
    let avant = out.len();
    out.extend_from_slice(ajout);
    out.truncate(PALETTE_MAX);
    let ajoutees = out.len() - avant;
    (out, ajoutees)
}

fn colorref_vers_rgb(c: COLORREF) -> [u8; 3] {
    let v = c.0;
    [(v & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, ((v >> 16) & 0xFF) as u8]
}

/// Relit `#RRGGBB` ou `RRGGBB` (la page peut envoyer l'un ou l'autre). `None`
/// si ce n'est pas exactement six chiffres hexadécimaux — **validation à la
/// frontière**, la valeur venant de la page.
pub(crate) fn rgb_depuis_hex(texte: &str) -> Option<[u8; 3]> {
    let s = texte.strip_prefix('#').unwrap_or(texte);
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let octet = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([octet(0)?, octet(2)?, octet(4)?])
}

pub fn est_armee() -> bool {
    ARMEE.with(Cell::get)
}

/// Retire le crochet et remet les cellules à zéro. Idempotent.
fn retirer_le_crochet() {
    ARMEE.with(|a| a.set(false));
    RESULTAT.with(|r| r.set(None));
    CLIC_DROIT_ANNULE.with(|c| c.set(false));
    POINT_SOUS_CURSEUR.with(|p| p.set(None));
    // La loupe s'en va avec la pipette : une loupe qui survit au crochet
    // resterait collée à l'écran, sans rien pour la rappeler.
    crate::win32::loupe::desarmer();
    if let Some(hook) = HOOK.with(Cell::take) {
        unsafe {
            let _ = UnhookWindowsHookEx(hook);
        }
    }
}

/// Arme la pipette. **Best-effort** : si le crochet ne peut pas être installé
/// (rare), la pipette reste simplement sans effet — jamais bloquante.
pub fn armer() {
    retirer_le_crochet();
    ARMEE.with(|a| a.set(true));
    // La loupe s'arme AVEC la pipette : elle n'a aucun sens toute seule, et
    // les deux se désarment au même endroit.
    crate::win32::loupe::armer();
    unsafe {
        if let Ok(hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(souris), None, 0) {
            HOOK.with(|h| h.set(Some(hook)));
        }
    }
    push_state();
}

pub fn desarmer() {
    if !est_armee() && HOOK.with(|h| h.get()).is_none() {
        return;
    }
    retirer_le_crochet();
    push_state();
}

/// Le crochet. ⛔ **IL N'ÉCRIT QUE DES `Cell`** — aucun emprunt, aucune
/// allocation, aucun `execute_js`, aucune capture d'écran. C'est ce qui le rend
/// incapable de paniquer en réentrance (voir la doctrine du dépôt) et ce qui le
/// tient sous les 300 ms de `LowLevelHooksTimeout`.
unsafe extern "system" fn souris(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code != HC_ACTION as i32 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let data = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
    match wparam.0 as u32 {
        // Le survol n'est JAMAIS avalé : on l'observe, c'est tout — et c'est ce
        // point que la loupe suit (voir `poll`).
        WM_MOUSEMOVE => {
            POINT_SOUS_CURSEUR.with(|p| p.set(Some(data.pt)));
            CallNextHookEx(None, code, wparam, lparam)
        }
        // **LE CLIC DROIT ANNULE, ET IL EST AVALÉ.** Le crochet des Réglages
        // l'ignore, donc un clic droit pendant qu'on vise ouvre le menu
        // contextuel de l'application DU DESSOUS — exactement le risque d'effet
        // de bord qui justifie d'avaler le clic gauche.
        WM_RBUTTONDOWN => {
            CLIC_DROIT_ANNULE.with(|c| c.set(true));
            LRESULT(1)
        }
        WM_LBUTTONDOWN => {
            // **UN CLIC SUR NOTRE PROPRE FENÊTRE LUI APPARTIENT.** Sans cette
            // règle, aucun bouton de l'applet ne serait pressable tant que la
            // pipette est armée : le crochet avale TOUS les clics gauches.
            //
            // `WindowFromPoint` et NON un rectangle en cache : un rect dirait
            // « dedans » alors que la fenêtre est partiellement recouverte, et
            // le clic traverserait jusqu'à nous pour atterrir sur ce qui nous
            // couvre — l'utilisateur perdrait son clic sans rien prélever. Et
            // `GetAncestor(GA_ROOT)` parce que CEF crée des fenêtres FILLES :
            // la comparaison brute échouerait et on avalerait un clic destiné à
            // notre propre page.
            // ⚠️ **LES DEUX FENÊTRES, PAS UNE.** Le crochet ne lit que des
            // `Cell` : `poll` y écrit les deux HWND à chaque frame, avant son
            // retour anticipé. Oublier le second rendait « Sortir » inpressable.
            let nos_hwnds = [NOTRE_HWND.with(Cell::get), NOTRE_HWND_PIP.with(Cell::get)];
            let racine = {
                let sous = WindowFromPoint(data.pt);
                if sous.0.is_null() { 0 } else { GetAncestor(sous, GA_ROOT).0 as isize }
            };
            if clic_nous_appartient(nos_hwnds, racine) {
                return CallNextHookEx(None, code, wparam, lparam);
            }
            let ecran = GetDC(None);
            let colorref = GetPixel(ecran, data.pt.x, data.pt.y);
            let _ = ReleaseDC(None, ecran);
            // ⛔ **`CLR_INVALID` VAUT `0xFFFF_FFFF`, PAS `0x00FF_FFFF`** — ce
            // dernier est du BLANC PUR. La pipette des Réglages teste le mauvais
            // (`cef_settings.rs:2647`) : elle jette un vrai pixel blanc et fait
            // passer un échec réel pour du blanc. La faute n'est pas recopiée
            // ici, et ce fichier-là n'est pas touché.
            if colorref.0 != 0xFFFF_FFFF {
                RESULTAT.with(|r| r.set(Some(colorref_vers_rgb(colorref))));
            }
            LRESULT(1)
        }
        _ => CallNextHookEx(None, code, wparam, lparam),
    }
}

/// À appeler **à chaque frame** (voir `app::run`), après `cef_pump`.
///
/// Le crochet s'exécute pendant le pompage des messages Win32, qui est EN HAUT
/// de la frame : ce qu'il a posé est donc toujours visible ici, dans la même
/// frame.
pub fn poll() {
    // **C'EST ICI, ET NULLE PART AILLEURS, QUE LE CROCHET APPREND NOTRE HWND.**
    // Il ne peut pas lire le `PopupSlot`, qui est un `RefCell` : il ne touche
    // que des `Cell`, et celle-ci est la seule qui porte une information venue
    // du dehors.
    NOTRE_HWND.with(|h| h.set(cef_popup::hwnd(&SLOT).map_or(0, |hwnd| hwnd.0 as isize)));
    // ⚠️ **AVANT LE RETOUR ANTICIPÉ** : la réduite est la fenêtre qu'on a sous le
    // nez pendant le mode pipette, mais elle doit être déclarée AUSSI quand la
    // pipette est éteinte — c'est le cas où l'on clique « Sortir » puis le
    // bouton « Pipette » de la grande fenêtre, dans la même frame.
    NOTRE_HWND_PIP.with(|h| h.set(cef_popup::hwnd(&SLOT_PIP).map_or(0, |hwnd| hwnd.0 as isize)));

    if !est_armee() {
        return;
    }
    // Échap annule — même sonde que la pipette des Réglages : le crochet n'a
    // AUCUN bras clavier, un `WH_KEYBOARD_LL` de plus serait un second risque
    // pour rien.
    let echap = unsafe { (GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16) & 0x8000 != 0 };
    if echap || CLIC_DROIT_ANNULE.with(|c| c.replace(false)) {
        desarmer();
        return;
    }
    // **LA LOUPE SUIT LE CURSEUR** — et `suivre` ne fait RIEN tant que le point
    // n'a pas changé, donc cet appel est gratuit sur un curseur immobile.
    if let Some(point) = POINT_SOUS_CURSEUR.with(Cell::get) {
        crate::win32::loupe::suivre(point);
    }
    if let Some(rgb) = RESULTAT.with(Cell::take) {
        ACTUEL.with(|c| *c.borrow_mut() = Some(rgb));
        // ⚠️ **LA PIPETTE RESTE ARMÉE** (demande explicite) : on enchaîne les
        // prélèvements sans rien réarmer. Elle ne se désarme donc PAS ici —
        // seulement par Échap, le clic droit, ou la fermeture de la fenêtre.
        //
        // ⚠️ **SEUL UN PRÉLÈVEMENT ENTRE DANS L'HISTORIQUE** — pas un clic sur
        // une pastille (voir l'action `actuel`), qui ne fait que désigner une
        // couleur déjà connue. L'historique dit d'où viennent les couleurs ;
        // l'y verser aussi les pastilles lui ferait perdre ce qu'il raconte.
        let nouveau = HISTORIQUE.with(|h| historique_apres(&h.borrow(), rgb));
        HISTORIQUE.with(|h| *h.borrow_mut() = nouveau);
        push_state();
    }
}

fn etat_json() -> serde_json::Value {
    let actuel = ACTUEL.with(|c| *c.borrow());
    let (hex, rgb, hsl) = match actuel {
        Some(v) => {
            let (h, s, l) = hsl_depuis_rgb(v);
            (
                json!(hex_depuis_rgb(v)),
                json!(format!("{}, {}, {}", v[0], v[1], v[2])),
                json!(format!("{}°, {} %, {} %", h.round(), s.round(), l.round())),
            )
        }
        None => (serde_json::Value::Null, serde_json::Value::Null, serde_json::Value::Null),
    };
    let historique: Vec<String> = HISTORIQUE.with(|h| h.borrow().iter().map(|c| hex_depuis_rgb(*c)).collect());
    let palette: Vec<String> = PALETTE.with(|p| p.borrow().iter().map(|c| hex_depuis_rgb(*c)).collect());
    let bibliotheque = BIBLIOTHEQUE.with(|b| b.borrow().clone());
    let groupes: Vec<serde_json::Value> = groupes_pour(actuel)
        .into_iter()
        .map(|(nom, teintes)| json!({ "nom": nom, "teintes": teintes }))
        .collect();
    json!({
        "contentScale": CONTENT_SCALE.with(Cell::get),
        // ⭐ **LA PAGE SE MESURE QUAND CE DRAPEAU EST VRAI** — voir `appliquer_hauteur`.
        "mesurer": MESURER.with(Cell::get),
        "pipette": est_armee(),
        "actuel": hex,
        "rgb": rgb,
        "hsl": hsl,
        "historique": historique,
        "palette": palette,
        "bibliotheque": bibliotheque,
        "groupes": groupes,
    })
}

fn push_state() {
    // **LA CLÉ EST LE PAYLOAD LUI-MÊME.** L'idiome du dépôt (`cef_calculatrice`)
    // calcule une clé courte AVANT de construire le JSON, pour l'économie ; ici
    // le payload porte une liste et cinq groupes de teintes, donc une clé
    // écrite à la main finirait par oublier un champ — et un champ oublié, dans
    // cette famille de code, c'est exactement le « il faut rouvrir le popup deux
    // fois » que le dépôt a déjà payé plusieurs fois. Le payload fait quelques
    // centaines d'octets : le sérialiser à chaque frame est le prix de ne
    // jamais pouvoir se désynchroniser.
    let payload = etat_json();
    let cle = payload.to_string();
    if PAYLOAD.with(|p| p.borrow().as_deref() == Some(cle.as_str())) {
        return;
    }
    PAYLOAD.with(|p| *p.borrow_mut() = Some(cle));
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
    push_state_pip();
}

/// **L'état de la fenêtre réduite : les prélèvements, la couleur courante, et
/// rien d'autre** — ni la palette, ni les harmonies, ni la bibliothèque de
/// fichiers. C'est la demande du 2026-10-27, et ce n'est pas une réduction par
/// goût : la réduite sert pendant qu'on vise, donc ce qu'elle montre doit tenir
/// dans un coin d'écran sans cacher ce qu'on vise.
///
/// Elle ne porte pas non plus le drapeau `mesurer` : sa taille est FIXE (voir
/// `HAUTEUR_PIP_UNITES`), et une fenêtre qui réclame une hauteur lutte contre le
/// redimensionnement de l'utilisateur.
fn etat_pip_json() -> serde_json::Value {
    let actuel = ACTUEL.with(|c| *c.borrow());
    json!({
        "contentScale": CONTENT_SCALE.with(Cell::get),
        "actuel": match actuel {
            Some(v) => json!(hex_depuis_rgb(v)),
            None => serde_json::Value::Null,
        },
        "historique": HISTORIQUE.with(|h| h.borrow().iter().map(|c| hex_depuis_rgb(*c)).collect::<Vec<String>>()),
    })
}

/// Même garde-fou que `push_state`, sur l'autre fenêtre : **la clé est le
/// payload lui-même**, parce qu'un champ oublié dans une clé écrite à la main est
/// exactement le « il faut rouvrir deux fois » que ce dépôt a déjà payé.
fn push_state_pip() {
    // Rien à pousser tant que la fenêtre n'existe pas : `execute_js` sur un slot
    // sans navigateur ne fait RIEN (c'est le même silence qu'à `montrer`), et sa
    // clé ne doit pas se remplir pour une page qui n'écoute encore personne.
    if cef_popup::hwnd(&SLOT_PIP).is_none() {
        return;
    }
    let payload = etat_pip_json();
    let cle = payload.to_string();
    if PAYLOAD_PIP.with(|p| p.borrow().as_deref() == Some(cle.as_str())) {
        return;
    }
    PAYLOAD_PIP.with(|p| *p.borrow_mut() = Some(cle));
    // Le journal est la seule trace de ce qui a été POUSSÉ : sans lui, une capture
    // vide laisse choisir entre « la page ne rend pas », « l'état n'est jamais
    // arrivé » et « la liste était vide » — trois pannes différentes.
    crate::diag!("[couleurs-pip] etat pousse : {} teintes", payload["historique"].as_array().map_or(0, |a| a.len()));
    cef_popup::execute_js(&SLOT_PIP, &format!("window.setState({payload})"));
}

/// **ON NE CROIT PAS LA PAGE.** Elle envoie une couleur ; on la RELIT comme une
/// couleur, et c'est la relecture qui est copiée — jamais la chaîne reçue telle
/// quelle, qui pourrait être n'importe quoi.
fn copier(hex: &str) {
    let Some(rgb) = rgb_depuis_hex(hex) else {
        crate::diag!("[couleurs] couleur illisible : {hex:?}");
        return;
    };
    crate::win32::clipboard::write_text(&hex_depuis_rgb(rgb));
}

/// Démarre un déplacement NATIF de la fenêtre, comme le ferait sa barre de
/// titre si elle en avait une (voir l'action `deplacer`).
fn deplacer() {
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return };
    cef_popup::deplacer_par_le_systeme(hwnd, "couleurs");
}

/// **REDIMENSIONNE LA FENÊTRE À LA HAUTEUR QUE SON CONTENU RÉCLAME.**
///
/// Seuls autres acteurs : `taille_pour_ecran` (la conversion), le `mesurer` de
/// `couleurs.html` (qui produit le chiffre) et `montrer` (qui pose le témoin).
///
/// ⭐ **POURQUOI LA HAUTEUR VIENT DE LA PAGE, ET LA LARGEUR NON.** Le contenu de
/// ce panneau est un document qui COULE : sa hauteur dépend de l'historique, de
/// la palette et du nombre de groupes — trois choses que Rust ne connaît pas
/// dans la même unité que la page. La largeur, elle, est un choix de conception
/// (`LARGEUR_UNITES`).
///
/// ⚠️ **LA FENÊTRE SE REDIMENSIONNE SUR PLACE** (`SWP_NOMOVE`) : la recentrer la
/// ferait sauter sous le curseur à l'instant même où l'utilisateur la regarde
/// s'ouvrir.
fn appliquer_hauteur(unites: &str) {
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return };
    let Ok(hauteur) = unites.trim().parse::<i32>() else {
        crate::diag!("[couleurs] hauteur illisible : {unites:?}");
        MESURER.with(|c| c.set(false));
        return;
    };
    let hauteur = hauteur.clamp(1, HAUTEUR_MAX_UNITES);
    let (ecran_l, ecran_h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let (largeur, hauteur_px) = taille_pour_ecran(ecran_l, ecran_h, (largeur_unites(), hauteur));
    // ⚠️ **LE JOURNAL EST LA SEULE TRACE DE CETTE MESURE** — c'est lui qui dit si
    // la fenêtre a été rabotée par le plafond de l'écran, auquel cas le contenu
    // défile encore et ce n'est pas un défaut du calcul.
    crate::diag!(
        "[couleurs] hauteur mesurée : {hauteur} unités -> {hauteur_px} px (écran {ecran_h}, plafond {})",
        (ecran_h as f32 * PART_MAX) as i32
    );
    unsafe {
        let _ = SetWindowPos(hwnd, None, 0, 0, largeur, hauteur_px, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
    MESURER.with(|c| c.set(false));
}

pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("couleurs/") else {
            crate::diag!("[couleurs] action inconnue : {action:?}");
            continue;
        };
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["ready"] => PAYLOAD.with(|c| *c.borrow_mut() = None),
            ["close"] => cacher(),
            // ⭐ **LA HAUTEUR QUE LE CONTENU RÉCLAME** — voir `appliquer_hauteur`
            // et le `mesurer` de `couleurs.html`, seuls autres acteurs.
            ["hauteur", unites] => appliquer_hauteur(unites),
            // **BASCULE.** La page n'envoie pas « armer » ni « désarmer », elle
            // envoie le GESTE : elle ne sait pas dans quel état on est, et lui
            // faire porter un état qu'elle devrait deviner serait la porte
            // ouverte aux deux moitiés qui se désynchronisent.
            ["pipette"] => {
                if est_armee() {
                    desarmer();
                } else {
                    armer();
                }
            }
            ["copier", hex] => copier(hex),
            // **DEVENIR LA COULEUR COURANTE** — le clic DROIT sur une pastille.
            // On relit la couleur reçue au lieu de la croire, même règle que
            // pour la copie : la page n'est pas une source de confiance.
            ["actuel", hex] => match rgb_depuis_hex(hex) {
                Some(rgb) => ACTUEL.with(|c| *c.borrow_mut() = Some(rgb)),
                None => crate::diag!("[couleurs] couleur illisible : {hex:?}"),
            },
            // **RETIRER DE L'HISTORIQUE.** La page envoie l'INDEX de la liste
            // qu'elle affiche, jamais la teinte : l'historique et la palette
            // peuvent porter la MÊME couleur, et un index se contrôle dans la
            // liste que l'utilisateur a sous les yeux.
            ["historique-retirer", index] => historique_retirer(index),
            // **LE DÉPLACEMENT PAR LA BARRE DE TITRE.** La fenêtre n'ayant plus
            // de barre Windows (voir `cef_popup::create_window`), c'est la page
            // qui la dessine — et qui dit ici « on m'a saisi sur l'en-tête ».
            //
            // On ne déplace PAS nous-mêmes : on rend la main à Windows
            // (`HTCAPTION`), qui fait alors exactement ce qu'il fait pour
            // n'importe quelle barre de titre — aimantation aux bords,
            // double-clic, glisser sur un autre écran. Réimplémenter ça à la
            // main donnerait une fenêtre qui se traîne et qui n'a plus aucun
            // des comportements que l'utilisateur attend sans y penser.
            ["deplacer"] => deplacer(),
            // --- La palette construite, et les fichiers `.gpl` ---
            ["palette-ajouter", hex] => palette_ajouter(hex),
            ["palette-ajouter-groupe", teintes] => palette_ajouter_groupe(teintes),
            ["palette-retirer", index] => palette_retirer(index),
            ["palette-vider"] => PALETTE.with(|p| p.borrow_mut().clear()),
            ["gpl-enregistrer", nom] => gpl_enregistrer(nom),
            ["gpl-charger", nom] => gpl_charger(nom),
            ["gpl-supprimer", nom] => gpl_supprimer(nom),
            ["gpl-importer"] => gpl_importer(),
            ["gpl-exporter", nom] => gpl_exporter(nom),
            _ => crate::diag!("[couleurs] action inconnue : {action:?}"),
        }
    }
}

/// Démarre le déplacement NATIF de la **fenêtre réduite** — le même geste que
/// `deplacer` pour la grande : on rend la main à Windows (`HTCAPTION`), on ne
/// réimplémente pas l'aimantation aux bords ni le passage d'un écran à l'autre.
fn deplacer_pip() {
    let Some(hwnd) = cef_popup::hwnd(&SLOT_PIP) else { return };
    cef_popup::deplacer_par_le_systeme(hwnd, "couleurs-pip");
}

/// Les actions de la fenêtre réduite. **Deuxième dispatch du module**, donc un
/// second jeu de verbes — le test
/// `la_fenetre_reduite_et_le_dispatch_acceptent_les_memes_verbes` le garde, et
/// `verbes_du_dispatch` est scopée à la fonction pour que les deux jeux ne se
/// mélangent pas.
///
/// Ce qu'elle ne fait PAS : aucune action sur les fichiers `.gpl`, aucune sur la
/// palette, aucune harmonie. La réduite sert pendant qu'on vise ; tout ce qui
/// demande de saisir du texte appartient à la grande fenêtre.
pub fn dispatch_pip() {
    for action in cef_popup::drain_actions(&SLOT_PIP) {
        let Some(reste) = action.strip_prefix("couleurs-pip/") else {
            crate::diag!("[couleurs-pip] action inconnue : {action:?}");
            continue;
        };
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["ready"] => {
                crate::diag!("[couleurs-pip] page prete");
                PAYLOAD_PIP.with(|c| *c.borrow_mut() = None);
            }
            // **SORTIR = DÉSARMER.** La page ne dit pas « cache ta fenêtre »,
            // elle dit le GESTE ; `pip_update` en tire la conséquence (la réduite
            // se cache, la grande revient). C'est aussi ce que font Échap, le
            // clic droit et Alt-F4 — un seul état, jamais deux moitiés qui se
            // désynchronisent.
            ["sortir"] => desarmer(),
            ["copier", hex] => copier(hex),
            // **DEVENIR LA COULEUR COURANTE** — le clic DROIT sur une teinte. On
            // RELIT la couleur au lieu de la croire, même règle que partout ici.
            ["actuel", hex] => match rgb_depuis_hex(hex) {
                Some(rgb) => ACTUEL.with(|c| *c.borrow_mut() = Some(rgb)),
                None => crate::diag!("[couleurs-pip] couleur illisible : {hex:?}"),
            },
            // L'INDEX de la liste affichée, jamais la teinte : `historique` et
            // `palette` peuvent porter la MÊME couleur, et un index se contrôle
            // dans la liste que l'utilisateur a sous les yeux.
            ["historique-retirer", index] => historique_retirer(index),
            ["deplacer"] => deplacer_pip(),
            _ => crate::diag!("[couleurs-pip] action inconnue : {action:?}"),
        }
    }
}

/// ⛔ **PAS de `resize_if_needed` ici**, contrairement à tous les panneaux :
/// pour une `fenetre`, la taille est celle de la ZONE CLIENTE, relue à chaque
/// frame par `render`. L'appeler ferait entrer la géométrie du dock en lutte
/// avec le redimensionnement de l'utilisateur.
pub fn render_if_visible(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    // Rayon 0 : une fenêtre n'a pas de coins arrondis en verre, sa page est
    // opaque (voir `PopupSpec::fenetre`).
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

/// Voir `win32::cef_calculatrice::debug_dump_backbuffer` — le harnais a DEUX
/// listes à tenir (`NEURADESK_DEBUG_OPEN=couleurs`).
pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

pub fn render_if_visible_pip(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::render_if_visible(&SLOT_PIP, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

pub fn debug_dump_backbuffer_pip(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT_PIP, path)
}

/// **LEVIER DU HARNAIS** (`NEURADESK_DEBUG_OPEN=couleurs-pip`) : ouvre la fenêtre
/// réduite **sans armer la pipette**.
///
/// ⛔ **ET C'EST LE SENS MÊME DU LEVIER.** Armer installe un crochet souris
/// global qui avale tous les clics gauches du bureau : le faire pendant un run de
/// capture, c'est geler le poste de la personne pendant 600 frames, sans aucun
/// moyen de le sortir (`neuradesk-live-input-safety`). La capture prouve que la
/// page **s'affiche**, jamais qu'elle réagit — c'est déjà tout ce qu'on peut
/// demander d'une fenêtre dont le geste exige un vrai clic.
///
/// ⚠️ Elle ne pose PAS `PIP_MONTEE` : `pip_update` ne gouverne que la fenêtre que
/// LE MODE PIPETTE a montée, et il aurait caché celle-ci à la frame suivante.
/// **LEVIER DE VÉRIFICATION** — `NEURADESK_DEBUG_COULEURS_HISTORIQUE=#RRGGBB,#RRGGBB,…`
///
/// ⛔ **POURQUOI IL EST NECESSAIRE.** La fenêtre réduite ne montre QUE
/// l'historique, et l'historique ne se remplit que d'un VRAI prélèvement — donc
/// d'un clic, interdit en session vivante. Un run de harnais part avec la liste
/// vide et la capture ne dirait rien du nouveau rendu, **sans le dire** : le
/// même piège que `PALETTE` le 2026-10-27, décrit dans la recette du harnais.
/// Un vide n'est pas une preuve.
///
/// ⚠️ Il **REMPLACE** la liste (et n'ajoute rien), et il n'est appelé que depuis
/// le chemin du harnais : une session normale ne le voit jamais.
fn amorcer_historique_de_debug() {
    let Ok(texte) = std::env::var("NEURADESK_DEBUG_COULEURS_HISTORIQUE") else { return };
    let couleurs: Vec<[u8; 3]> = texte.split(',').filter_map(|s| rgb_depuis_hex(s.trim())).collect();
    if couleurs.is_empty() {
        // Le texte reçu ne contenait AUCUNE teinte lisible. Le dire, c'est ce qui
        // distingue « la page ne rend pas » de « la variable d'environnement était
        // mal écrite » — sans cette ligne, les deux se ressemblent.
        crate::diag!("[couleurs-pip] levequier ignore : {texte:?} n'a aucune teinte lisible");
        return;
    }
    let nombre = couleurs.len();
    HISTORIQUE.with(|h| *h.borrow_mut() = couleurs);
    crate::diag!("[couleurs-pip] historique amorce : {nombre} teintes");
}

pub fn debug_montrer_pip(gfx: &GfxDevice, content_scale: f32) -> bool {
    let globale = crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES);
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("couleurs") * globale));
    PAYLOAD_PIP.with(|c| *c.borrow_mut() = None);
    amorcer_historique_de_debug();
    let creee = cef_popup::montrer_fenetre(&SLOT_PIP, &SPEC_PIP, rect_pip(), gfx);
    au_dessus_pip();
    push_state_pip();
    creee
}

/// **La fenêtre réduite vit avec le mode pipette** : elle naît quand il s'arme,
/// elle meurt quand il s'éteint, et la grande fenêtre revient à la sortie si elle
/// était ouverte. À appeler **à chaque frame** (voir `app::run`).
///
/// ⭐ **C'EST LA FRAME QUI LA MONTE, PAS `armer`.** Monter une fenêtre demande le
/// `GfxDevice` — sa swapchain et son navigateur — et `armer` est appelé depuis le
/// dispatch, depuis `poll` et depuis un `wndproc` : trois endroits qui ne l'ont
/// pas, et le dernier ne doit rien emprunter (doctrine du dépôt, `cef_popup.rs`
/// sur la réentrance). `armer` ne pose donc que des `Cell`, et ceci en tire la
/// conséquence une frame plus tard — 16 ms, ce qui ne se voit pas.
pub fn pip_update(gfx: &GfxDevice) {
    if est_armee() {
        if PIP_MONTEE.with(Cell::get) {
            return;
        }
        // La grande cède la place (demande explicite), mais on RETIENT si elle
        // était là : la sortie la rend, et une pipette qui ferme la fenêtre d'où
        // on l'a lancée sans la rendre est une pipette qui fait disparaître
        // l'outil.
        GRAND_ETAIT.with(|c| c.set(cef_popup::is_visible(&SLOT)));
        if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
            cef_popup::cacher_fenetre(hwnd);
        }
        cef_popup::montrer_fenetre(&SLOT_PIP, &SPEC_PIP, rect_pip(), gfx);
        if cef_popup::hwnd(&SLOT_PIP).is_none() {
            // Le seul échec réel : aucune fenêtre après l'appel (`montrer_fenetre`
            // rend « créée », pas « réussie » — la mémoire de la visionneuse).
            // Pas de témoin posé : la frame suivante réessaiera.
            crate::diag!("[couleurs-pip] la fenetre reduite n'a pas pu etre creee");
            return;
        }
        au_dessus_pip();
        PIP_MONTEE.with(|c| c.set(true));
        push_state();
        return;
    }
    if !PIP_MONTEE.with(Cell::take) {
        return;
    }
    if let Some(hwnd) = cef_popup::hwnd(&SLOT_PIP) {
        cef_popup::cacher_fenetre(hwnd);
    }
    if GRAND_ETAIT.with(Cell::take) {
        // La grande revient **où l'utilisateur l'avait laissée** :
        // `montrer_fenetre` ne repose le `rect` qu'à la première monte (voir
        // `cef_popup::faut_appliquer_le_rect`). On n'appelle PAS `montrer` ici :
        // il remultiplierait l'échelle déjà stockée dans `CONTENT_SCALE` et
        // rouvrirait la mesure de la hauteur, ce qui écraserait la taille que
        // l'utilisateur avait réglée à la main.
        cef_popup::montrer_fenetre(&SLOT, &SPEC, rect_defaut(), gfx);
        au_dessus();
    }
    push_state();
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // ⛔ **LA FENÊTRE N'A PLUS DE BORDURE NI DE BARRE DE TITRE WINDOWS** (voir
    // `cef_popup::create_window`, `WS_POPUP` pour un `fenetre`) : le système ne
    // fournit donc PLUS le redimensionnement par les bords, et c'est ici qu'on
    // le rend. Sans ce message, la fenêtre serait figée à sa taille de création.
    if msg == WM_NCHITTEST {
        return code_de_redimensionnement(hwnd, point_du_message(lparam));
    }
    if msg == WM_CLOSE {
        // ⛔ **`retirer_le_crochet`, PAS `desarmer`** : `desarmer` pousse l'état,
        // donc `execute_js`, donc `borrow()` — qui PANIQUE si l'appelant tient
        // déjà un `borrow_mut` (doctrine du dépôt, `cef_popup.rs:1730-1744`).
        // Dans un `wndproc`, on ne touche que des `Cell`.
        retirer_le_crochet();
        cef_popup::cacher_fenetre(hwnd);
        return LRESULT(0);
    }
    if let Some(result) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// La `wndproc` de la fenêtre réduite. Elle a la sienne parce que chaque classe
/// de fenêtre en exige une (`PopupSpec::wndproc`) — la factoriser serait le même
/// nettoyage que celui que `cef_kaomoji` refuse déjà à mots comptés.
unsafe extern "system" fn wndproc_pip(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCHITTEST {
        return code_de_redimensionnement(hwnd, point_du_message(lparam));
    }
    if msg == WM_CLOSE {
        // Alt-F4 sur la réduite = sortir du mode pipette. ⛔ **`retirer_le_crochet`
        // et non `desarmer`** : `desarmer` pousse l'état, donc `execute_js`, donc
        // `borrow()` — qui PANIQUE si l'appelant tient déjà un `borrow_mut`. Dans
        // un `wndproc`, on ne touche que des `Cell`. La grande fenêtre est rendue
        // par `pip_update`, dans la frame, qui lit `GRAND_ETAIT`.
        retirer_le_crochet();
        cef_popup::cacher_fenetre(hwnd);
        return LRESULT(0);
    }
    if let Some(result) = cef_popup::dispatch_input(&SLOT_PIP, msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Épaisseur, en pixels, de la bande de redimensionnement sur les bords — la
/// fenêtre n'ayant plus de bordure système, c'est à nous de la définir.
const BORDURE_REDIMENSIONNEMENT: i32 = 6;

/// Les coordonnées ÉCRAN d'un message souris. `GET_X_LPARAM`/`GET_Y_LPARAM` :
/// deux mots de 16 bits SIGNÉS — un simple masquage donnerait un x négatif
/// (écran à gauche du principal) lu comme 65500.
fn point_du_message(lparam: LPARAM) -> POINT {
    let v = lparam.0 as i32;
    POINT { x: (v << 16) >> 16, y: v >> 16 }
}

/// Quel curseur de redimensionnement pour ce point ? **Fonction PURE**, donc
/// testable : la comparer à `GetWindowRect` est la seule partie qui touche
/// Win32.
fn code_pour_point(cadre: [i32; 4], point: POINT) -> u32 {
    let [g, h, l, b] = [cadre[0], cadre[1], cadre[0] + cadre[2], cadre[1] + cadre[3]];
    let gauche = point.x - g < BORDURE_REDIMENSIONNEMENT;
    let droite = l - point.x < BORDURE_REDIMENSIONNEMENT;
    let haut = point.y - h < BORDURE_REDIMENSIONNEMENT;
    let bas = b - point.y < BORDURE_REDIMENSIONNEMENT;
    match (gauche, droite, haut, bas) {
        (true, _, true, _) => HTTOPLEFT,
        (_, true, true, _) => HTTOPRIGHT,
        (true, _, _, true) => HTBOTTOMLEFT,
        (_, true, _, true) => HTBOTTOMRIGHT,
        (true, _, _, _) => HTLEFT,
        (_, true, _, _) => HTRIGHT,
        (_, _, true, _) => HTTOP,
        (_, _, _, true) => HTBOTTOM,
        _ => HTCLIENT,
    }
}

fn code_de_redimensionnement(hwnd: HWND, point: POINT) -> LRESULT {
    let mut cadre = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut cadre) }.is_err() {
        return LRESULT(HTCLIENT as isize);
    }
    let [g, h] = [cadre.left, cadre.top];
    LRESULT(code_pour_point([g, h, cadre.right - g, cadre.bottom - h], point) as isize)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les verbes qu'UNE fonction de dispatch accepte — même lecture que
    /// `cef_calculatrice::tests::verbes_du_dispatch`, **scopée à cette fonction**.
    ///
    /// ⛔ **LA SCOPPE N'EST PAS DU GOÛT.** Le module a DEUX dispatch depuis le
    /// 2026-10-27 (la grande fenêtre, la réduite) : lire le fichier entier
    /// comparerait les verbes d'UNE page à ceux des DEUX fonctions, et ferait
    /// rougir le garde **sans aucun défaut réel**. Le remède de facilité — ajouter
    /// à la liste de la grande page les verbes de l'autre — donnerait un garde qui
    /// ne garde plus rien. La découpe s'arrête au premier `\n}` : une fonction se
    /// termine à la colonne zéro, comme partout dans ce fichier.
    fn verbes_du_dispatch(nom: &str) -> Vec<String> {
        let source = include_str!("cef_couleurs.rs");
        let corps = source
            .split(&format!("pub fn {nom}() {{"))
            .nth(1)
            .unwrap_or_default()
            .split("\n}")
            .next()
            .unwrap_or_default();
        corps
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

    /// Les deux pages du module, lues comme du **texte brut**. `include_str!`
    /// exige un littéral, d'où deux constantes plutôt qu'un chemin en argument.
    const PAGE_GRANDE: &str = include_str!("../../assets/popups/couleurs.html");
    const PAGE_REDUITE: &str = include_str!("../../assets/popups/couleurs_pipette.html");

    /// Les verbes qu'UNE page émet — le premier guillemet après chaque `go(`.
    ///
    /// ⚠️ **Le scan prend TOUT le texte, commentaires compris** : un commentaire
    /// qui cite un appel avec son verbe entre guillemets ajoute un verbe FANTÔME
    /// à la liste. C'est arrivé le 2026-10-27, et le garde a rougi pour une
    /// raison fausse — d'où ces commentaires écrits sans littéral d'appel.
    fn verbes_de_la_page(source: &str) -> Vec<String> {
        source
            .match_indices("go(")
            .filter_map(|(i, _)| {
                let reste = &source[i + 3..];
                let guillemet = reste.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                reste[1..].split(guillemet).next().map(str::to_string)
            })
            .collect()
    }

    /// ⛔ **LES DEUX MOITIÉS DOIVENT RESTER ENSEMBLE.** Un verbe que la page
    /// émet et que `dispatch_actions` ne traite pas est une action qui disparaît
    /// en silence (le `match` la journalise et l'oublie) ; un verbe traité que
    /// la page n'émet jamais est du code mort. C'est la panne décrite par la
    /// mémoire « arguments de page encodés », où une forme à deux moitiés dont
    /// une seule marchait a coûté un aller-retour.
    /// Trier et dédoubler des deux côtés : les listes n'arrivent pas dans le même
    /// ordre, et ce qui compte est l'ENSEMBLE.
    fn tries(mut v: Vec<String>) -> Vec<String> {
        v.sort();
        v.dedup();
        v
    }

    #[test]
    fn la_page_et_le_dispatch_acceptent_les_memes_verbes() {
        assert_eq!(tries(verbes_de_la_page(PAGE_GRANDE)), tries(verbes_du_dispatch("dispatch_actions")));
    }

    /// **LA MÊME GARDE, POUR LA SECONDE FENÊTRE.** Un verbe que la réduite émet et
    /// que `dispatch_pip` ne traite pas est un bouton qui ne fait RIEN, en silence
    /// — et ici ce bouton s'appelle « Sortir » : sans garde, le seul moyen de
    /// quitter le mode pipette serait Échap, ce que personne ne devine.
    /// ⚠️ **LES DEUX PLANCHERS, ET C'EST LE GARDE-FOU DU GARDE.** Une liste vide
    /// de chaque côté se ressemble : si le scan de la page ne voyait plus `go`
    /// (déplacé dans un fichier JS partagé, par exemple), ou si la découpe de
    /// `verbes_du_dispatch` ne trouvait plus la fonction, la parité passerait
    /// **en silence** — le faux positif que `cef_popup::tests_couleur_des_pages`
    /// paie déjà d'un plancher. Les deux côtés sont donc comptés, et le message
    /// dit lequel est muet.
    #[test]
    fn la_fenetre_reduite_et_le_dispatch_acceptent_les_memes_verbes() {
        let page = tries(verbes_de_la_page(PAGE_REDUITE));
        let rust = tries(verbes_du_dispatch("dispatch_pip"));
        assert!(page.len() >= 5, "scan de la page muet : {page:?}");
        assert!(rust.len() >= 5, "scan du dispatch muet : {rust:?}");
        assert_eq!(page, rust);
    }

    /// La fenêtre réduite tient dans l'écran et reste **collée en bas à droite** :
    /// c'est le coin où l'on vise le moins, parce qu'une fenêtre centrée
    /// recouvrirait le pixel qu'on est en train de prélever.
    #[test]
    fn la_fenetre_reduite_tient_dans_l_ecran_et_ne_sort_pas_en_negatif() {
        let [x, y, l, h] = rect_pip_pour(1920, 1080, (LARGEUR_PIP_UNITES, HAUTEUR_PIP_UNITES));
        assert!(x >= 0 && y >= 0, "coin hors écran : {x},{y}");
        assert!(x + l <= 1920 && y + h <= 1080, "déborde : {x},{y} {l}×{h}");
        assert_eq!(x + l, 1920 - MARGE_PIP, "pas collée au bord droit");
        assert_eq!(y + h, 1080 - MARGE_PIP, "pas collée au bord bas");

        // **RÉDUITE, mesurée comme telle** : à ce même écran, sa surface doit
        // rester nettement sous celle de la grande fenêtre. Le tiers vient de la
        // mesure (0,54 sur cet écran, la grande étant rabotée en hauteur par
        // `PART_MAX`), pas d'une intuition.
        let grande = taille_pour_ecran(1920, 1080, (LARGEUR_UNITES, HAUTEUR_UNITES));
        assert!(l < grande.0 && l * h < grande.0 * grande.1 * 2 / 3, "{l}×{h} contre {}×{}", grande.0, grande.1);

        // Un écran PLUS PETIT qu'elle : le coin ne passe jamais en négatif (une
        // fenêtre à coordonnées négatives est inaccessible), et la taille reste
        // positive (une fenêtre de 0 px est invisible ET inattrapable).
        let [x2, y2, l2, h2] = rect_pip_pour(100, 80, (LARGEUR_PIP_UNITES, HAUTEUR_PIP_UNITES));
        assert!(x2 >= 0 && y2 >= 0, "coin négatif : {x2},{y2}");
        assert!(l2 >= 1 && h2 >= 1, "{l2}×{h2}");
    }

    /// La fenêtre est centrée : les deux coins restent dans l'écran.
    #[test]
    fn la_fenetre_par_defaut_ne_deborde_pas_par_la_gauche_ni_le_haut() {
        let [x, y, l, h] = rect_defaut();
        assert!(x >= 0 && y >= 0, "coin hors écran : {x},{y}");
        assert!(l > 0 && h > 0);
    }

    /// **LA TAILLE D'OUVERTURE NE DÉPASSE JAMAIS L'ÉCRAN.** Un nombre fixe finit
    /// par être trop grand quelque part — un portable en 768 lignes, un écran
    /// secondaire — et une fenêtre plus haute que le bureau cacherait sa propre
    /// barre de titre.
    #[test]
    fn la_taille_d_ouverture_ne_depasse_jamais_l_ecran() {
        // Écran plus BAS que la taille voulue : les deux côtés se rabattent sur
        // la part de l'écran.
        assert_eq!(taille_pour_ecran(800, 600, (LARGEUR_UNITES, HAUTEUR_UNITES)), (720, 540));
        // Un écran absurde ne rend jamais une fenêtre de taille nulle : une
        // fenêtre de 0 px est invisible ET impossible à rattraper.
        let (l, h) = taille_pour_ecran(0, 0, (LARGEUR_UNITES, HAUTEUR_UNITES));
        assert!(l >= 1 && h >= 1, "{l}×{h}");
    }

    /// ⭐ **LA FENÊTRE DONNE AU CONTENU LES UNITÉS QU'ELLE ANNONCE** — c'est
    /// exactement ce que la demande du 2026-10-08 exigeait : « la fenêtre
    /// affiche tout son contenu ».
    ///
    /// ⛔ **CE TEST ÉCHOUE SUR L'ANCIEN CODE** : les unités y étaient passées
    /// TELLES QUELLES à `SetWindowPos`, donc la fenêtre valait 640×880
    /// **physiques** et le contenu n'obtenait que 213×293 unités de page — un
    /// tiers de ce qu'il lui faut, la barre de titre en prenant 72.
    #[test]
    fn la_fenetre_donne_au_contenu_les_unites_qu_elle_annonce() {
        let unites = (LARGEUR_UNITES, HAUTEUR_UNITES);
        // Un écran assez grand pour que le plafond ne morde pas : on mesure la
        // CONVERSION, pas la borne (qui a son propre test au-dessus).
        let (l, h) = taille_pour_ecran(3840, 2160, unites);
        let zoom = crate::win32::cef_popup::CONTENT_ZOOM_FACTOR;
        assert_eq!(
            (l, h),
            (
                (unites.0 as f64 * zoom).round() as i32,
                (unites.1 as f64 * zoom).round() as i32
            ),
            "la taille physique doit être les unités × le zoom de la page"
        );
        assert!(
            l > unites.0 && h > unites.1,
            "les unités doivent être MULTIPLIÉES par le zoom, jamais passées telles quelles"
        );
    }

    // --- La pipette ---------------------------------------------------------

    /// **LA RÈGLE QUI REND LES BOUTONS DE L'APPLET PRESSABLES.** Le crochet
    /// avale TOUS les clics gauches ; sans cette règle, aucun bouton ne
    /// répondrait tant que la pipette est armée.
    ///
    /// ⚠️ Ce qui est testé ici est la COMPARAISON. Ce qui ne l'est pas — et qui
    /// ne peut pas l'être — est la normalisation `GetAncestor(..., GA_ROOT)`
    /// faite par l'appelant : elle ne se prouve qu'à la main, en cliquant sur
    /// un bouton de la fenêtre pendant que la pipette est armée.
    ///
    /// ⭐ **ET LE SECOND ÉLÉMENT DU TABLEAU EST L'OBJET DU TEST.** Le 2026-10-27
    /// ajoute une seconde fenêtre (la réduite du mode pipette) : sa comparaison
    /// est exactement ce qui rend son bouton « Sortir » pressable. Sur l'ancien
    /// code, qui ne comparait qu'un seul HWND, la ligne `clic_nous_appartient`
    /// avec le second HWND n'aurait même pas compilé — c'est le signal qu'on
    /// veut, pas un accident.
    #[test]
    fn un_clic_nous_appartient_quand_la_racine_est_l_une_de_nos_deux_fenetres() {
        let (grande, reduite) = (0x1234_isize, 0x5678_isize);
        // Notre propre fenêtre, ou une de ses FILLES ramenée à sa racine.
        assert!(clic_nous_appartient([grande, 0], grande));
        // **La réduite** — le cas qui n'existait pas avant la fenêtre réduite :
        // sans lui, le crochet avalerait le clic sur « Sortir » et le mode
        // pipette n'aurait plus qu'Échap pour sortir.
        assert!(clic_nous_appartient([grande, reduite], reduite));
        // Une autre application, y compris avec les deux HWND posés.
        assert!(!clic_nous_appartient([grande, reduite], 0x9abc));
        // Une fenêtre qui n'existe pas encore (HWND `0`) n'accepte RIEN : sinon
        // le `0` du bureau et le `0` d'une fenêtre absente se confondraient, et
        // un clic sur le bureau serait avalé pour rien.
        assert!(!clic_nous_appartient([0, reduite], 0));
        assert!(!clic_nous_appartient([grande, 0], reduite));
        assert!(!clic_nous_appartient([0, 0], 0));
    }

    /// L'hexa de l'applet porte le dièse — c'est ce qu'on colle ailleurs.
    #[test]
    fn l_hexa_porte_le_diese_et_se_relit() {
        assert_eq!(hex_depuis_rgb([0, 0, 0]), "#000000");
        assert_eq!(hex_depuis_rgb([255, 255, 255]), "#FFFFFF");
        assert_eq!(hex_depuis_rgb([18, 52, 86]), "#123456");
        // Le remplissage à deux chiffres, qui casse tout si on l'oublie.
        assert_eq!(hex_depuis_rgb([1, 2, 3]), "#010203");
        for rgb in [[0u8, 0, 0], [255, 0, 128], [1, 2, 3], [200, 199, 198]] {
            assert_eq!(rgb_depuis_hex(&hex_depuis_rgb(rgb)), Some(rgb));
        }
    }

    /// **ON NE CROIT PAS LA PAGE** : ce qui n'est pas six chiffres hexa est
    /// refusé, jamais copié tel quel.
    #[test]
    fn une_couleur_illisible_est_refusee() {
        // Les deux écritures sont acceptées.
        assert_eq!(rgb_depuis_hex("#0A141E"), Some([10, 20, 30]));
        assert_eq!(rgb_depuis_hex("0A141E"), Some([10, 20, 30]));
        for mauvais in ["", "#", "0A141", "0A141E2", "GGGGGG", "#0A141Z", "rouge", "#0A 41E", "0a141e0"] {
            assert_eq!(rgb_depuis_hex(mauvais), None, "{mauvais:?} doit être refusé");
        }
    }

    /// L'HSL affiché est celui du dépôt, à l'échelle du pourcentage — confronté
    /// à `ui::color::rgb_to_hsl`, PAS à des constantes recopiées (deux
    /// conversions divergeraient en silence).
    #[test]
    fn l_hsl_affiche_est_celui_du_depot() {
        for rgb in [[255u8, 0, 0], [0, 128, 255], [18, 52, 86], [200, 199, 198]] {
            let (h, s, l) = hsl_depuis_rgb(rgb);
            let (h0, s0, l0) = crate::ui::color::rgb_to_hsl(rgb.map(|c| c as f32 / 255.0));
            assert!((h - h0).abs() < 1e-2, "teinte {h} au lieu de {h0}");
            assert!((s - s0 * 100.0).abs() < 1e-2, "saturation {s} au lieu de {}", s0 * 100.0);
            assert!((l - l0 * 100.0).abs() < 1e-2, "clarté {l} au lieu de {}", l0 * 100.0);
        }
        // Le rouge vif, vérifiable à la main.
        let (h, s, l) = hsl_depuis_rgb([255, 0, 0]);
        assert!(h.abs() < 1e-2 && (s - 100.0).abs() < 1e-2 && (l - 50.0).abs() < 1e-2);
        // Un gris n'a pas de teinte : saturation nulle, pas de NaN.
        let (_, s, l) = hsl_depuis_rgb([128, 128, 128]);
        assert!(s.abs() < 1e-2 && (l - 50.196).abs() < 0.1);
    }

    // --- La fenêtre sans bordure -------------------------------------------

    /// **LE REDIMENSIONNEMENT QUE LE SYSTÈME NE FOURNIT PLUS.** La fenêtre étant
    /// `WS_POPUP` (voir `cef_popup::create_window`), ses bords n'existent que
    /// par ce calcul.
    #[test]
    fn les_bords_rendent_le_bon_code_de_redimensionnement() {
        // Cadre 100..600 × 100..500, en [x, y, largeur, hauteur].
        let cadre = [100, 100, 500, 400];
        let au = |x: i32, y: i32| code_pour_point(cadre, POINT { x, y });
        assert_eq!(au(100, 300), HTLEFT);
        assert_eq!(au(599, 300), HTRIGHT);
        assert_eq!(au(300, 100), HTTOP);
        assert_eq!(au(300, 499), HTBOTTOM);
        assert_eq!(au(100, 100), HTTOPLEFT);
        assert_eq!(au(599, 100), HTTOPRIGHT);
        assert_eq!(au(100, 499), HTBOTTOMLEFT);
        assert_eq!(au(599, 499), HTBOTTOMRIGHT);
        // La bande fait `BORDURE_REDIMENSIONNEMENT` px : son dernier pixel est
        // un bord, le suivant est déjà la page. Sans cette borne, la fenêtre
        // aurait une zone morte où l'on ne pourrait plus cliquer dans la page.
        assert_eq!(au(100 + BORDURE_REDIMENSIONNEMENT - 1, 300), HTLEFT);
        assert_eq!(au(100 + BORDURE_REDIMENSIONNEMENT, 300), HTCLIENT);
        // Au centre : la page.
        assert_eq!(au(300, 300), HTCLIENT);
    }

    /// **UN ÉCRAN À GAUCHE DU PRINCIPAL.** `WM_NCHITTEST` livre ses
    /// coordonnées en deux mots de 16 bits SIGNÉS : un simple masquage lirait
    /// x = -100 comme 65436, et la bande de redimensionnement se retrouverait
    /// du mauvais côté de l'écran.
    #[test]
    fn les_coordonnees_negatives_sont_relues_correctement() {
        let lparam = LPARAM((((-100i32 as u32) & 0xFFFF) | ((-200i32 as u32) << 16)) as isize);
        let p = point_du_message(lparam);
        assert_eq!((p.x, p.y), (-100, -200));
        let lparam = LPARAM((300u32 | (400u32 << 16)) as isize);
        let p = point_du_message(lparam);
        assert_eq!((p.x, p.y), (300, 400));
    }

    // --- L'historique et les harmonies -------------------------------------

    /// **UN DOUBLON REMONTE EN TÊTE, IL NE SE DÉDOUBLE PAS.** On reprend
    /// souvent la même couleur juste après l'avoir prise — pour la comparer,
    /// ou après un aller-retour dans une autre application.
    #[test]
    fn l_historique_remonte_le_doublon_et_borne_la_liste() {
        let mut h: Vec<[u8; 3]> = Vec::new();
        for c in [[1, 1, 1], [2, 2, 2], [3, 3, 3]] {
            h = historique_apres(&h, c);
        }
        assert_eq!(h, vec![[3, 3, 3], [2, 2, 2], [1, 1, 1]], "le plus récent en tête");
        h = historique_apres(&h, [1, 1, 1]);
        assert_eq!(h, vec![[1, 1, 1], [3, 3, 3], [2, 2, 2]]);
        assert_eq!(h.iter().filter(|c| **c == [1, 1, 1]).count(), 1, "aucun doublon");
        // Au-delà du plafond, la plus ANCIENNE tombe — l'historique n'est pas
        // une archive.
        let mut plein: Vec<[u8; 3]> = Vec::new();
        for i in 0..(HISTORIQUE_MAX as u8 + 5) {
            plein = historique_apres(&plein, [i, 0, 0]);
        }
        assert_eq!(plein.len(), HISTORIQUE_MAX);
        assert_eq!(plein[0], [HISTORIQUE_MAX as u8 + 4, 0, 0], "la plus récente en tête");
        assert!(!plein.contains(&[0, 0, 0]), "la toute première est tombée");
    }

    /// **RETIRER NE DÉPLACE RIEN D'AUTRE, ET UN INDEX HORS LISTE NE JETTE RIEN.**
    /// Un prélèvement survenu entre le rendu et le clic REMONTE la liste
    /// (`historique_apres`) : l'index lu par la page peut ne plus désigner la
    /// même teinte. Dans ce cas on ne retire pas « la dernière » — on ne retire
    /// rien, et la liste d'origine n'est pas touchée.
    #[test]
    fn retirer_une_couleur_de_l_historique_laisse_les_autres_dans_l_ordre() {
        let liste = [[1u8, 2, 3], [4, 5, 6], [7, 8, 9]];
        assert_eq!(historique_sans(&liste, 1), vec![[1, 2, 3], [7, 8, 9]], "les autres gardent l'ordre");
        assert_eq!(historique_sans(&liste, 3), liste, "un index hors liste ne change rien");
        assert_eq!(historique_sans(&liste, 90), liste, "un index tres hors liste non plus");
        assert_eq!(historique_sans(&[], 0), Vec::<[u8; 3]>::new(), "vide reste vide");
        assert_eq!(liste.len(), 3, "la liste d'origine est inchangée");
    }

    /// Rien tant qu'aucune couleur n'a été prélevée — la page n'affiche alors
    /// aucune section, plutôt que cinq groupes de noir. Et les teintes sortent
    /// en hexa **RELISIBLE** : c'est ce que la page pose en `background`, où une
    /// chaîne mal formée serait ignorée en silence.
    #[test]
    fn les_groupes_sont_vides_sans_couleur_et_dans_l_ordre_annonce() {
        assert!(groupes_pour(None).is_empty());
        let groupes = groupes_pour(Some([255, 0, 0]));
        let noms: Vec<&str> = groupes.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(noms, ["Complémentaire", "Analogues", "Triade", "Quadriade", "Monochrome"]);
        for (nom, teintes) in &groupes {
            assert!(!teintes.is_empty(), "{nom} est vide");
            for t in teintes {
                assert!(rgb_depuis_hex(t).is_some(), "{nom} : {t:?} n'est pas relisible");
            }
        }
        // La complémentaire d'un rouge vif est un cyan vif — le cas qu'on peut
        // vérifier de tête, donc celui qui prouve que la chaîne complète
        // (harmonies → flottants → 8 bits → hexa) ne s'est pas décalée.
        assert_eq!(groupes[0].1[0], "#00FFFF");
    }

    // --- La palette construite ---------------------------------------------

    /// **LA PALETTE N'EST PAS UN HISTORIQUE** : elle accueille les doublons,
    /// parce que c'est une liste qu'on COMPOSE et qu'on retire au ✕. Et au
    /// plafond, elle ne jette rien en silence — elle rend la liste inchangée.
    #[test]
    fn la_palette_ajoute_sans_dedoublonner_et_ne_perd_rien_au_plafond() {
        assert_eq!(palette_apres(&[], [1, 2, 3]), vec![[1, 2, 3]]);
        let deux = palette_apres(&palette_apres(&[], [1, 2, 3]), [1, 2, 3]);
        assert_eq!(deux, vec![[1, 2, 3], [1, 2, 3]], "la palette n'est pas un historique");
        // Un LOT s'ajoute d'un bloc, et le compte rendu dit ce qui est entré.
        let (liste, ajoutees) = palette_apres_lot(&[[9, 9, 9]], &[[1, 1, 1], [2, 2, 2]]);
        assert_eq!(liste, vec![[9, 9, 9], [1, 1, 1], [2, 2, 2]]);
        assert_eq!(ajoutees, 2);
        // Plein : rien ne bouge, et le compte rendu le dit.
        let plein: Vec<[u8; 3]> = vec![[7, 7, 7]; PALETTE_MAX];
        let (liste, ajoutees) = palette_apres_lot(&plein, &[[1, 1, 1]]);
        assert_eq!((liste.len(), ajoutees), (PALETTE_MAX, 0));
        assert_eq!(liste, plein);
        // Un lot qui déborde n'entre que pour la place disponible — et le
        // compte rendu est ce qui permet de le DIRE à l'utilisateur.
        let presque: Vec<[u8; 3]> = vec![[7, 7, 7]; PALETTE_MAX - 1];
        let (liste, ajoutees) = palette_apres_lot(&presque, &[[1, 1, 1], [2, 2, 2]]);
        assert_eq!((liste.len(), ajoutees), (PALETTE_MAX, 1));
        assert_eq!(liste[PALETTE_MAX - 1], [1, 1, 1]);
    }

    /// **« AJOUTER LA COULEUR COURANTE » NE DOIT PAS PANIQUER.** Écrire
    /// `*p.borrow_mut() = palette_apres(&p.borrow(), rgb)` tient l'emprunt
    /// MUTABLE pendant qu'on rappelle `borrow` du MÊME `RefCell` — la réentrance
    /// déjà payée dans ce dépôt (`neuradesk-refcell-reentrancy`). Le test rouge
    /// sur la forme actuelle, vert quand l'ordre est corrigé : calculer AVANT,
    /// écrire après.
    #[test]
    fn palette_ajouter_ne_rentre_pas_dans_sa_propre_emprunt() {
        palette_ajouter("#0A0B0C");
        assert_eq!(PALETTE.with(|p| p.borrow().clone()), vec![[0x0A, 0x0B, 0x0C]], "la couleur est entrée");
    }

    /// **L'ALLER-RETOUR QUI COMPTE** : ce qu'on enregistre doit se relire à
    /// l'identique — c'est tout l'intérêt d'échanger avec GIMP et Inkscape.
    /// Éprouvé sur la FORME RÉELLE que ce module écrit (noms de couleur vides),
    /// qui est justement le cas un peu particulier du format.
    #[test]
    fn la_palette_fait_l_aller_retour_par_le_gpl() {
        use crate::win32::palette_gpl::{CouleurGpl, PaletteGpl, depuis_gpl, vers_gpl};
        let couleurs = vec![[0u8, 0, 0], [255, 255, 255], [18, 52, 86], [1, 2, 3]];
        let texte = vers_gpl(&PaletteGpl {
            nom: "Test".to_string(),
            colonnes: 16,
            couleurs: couleurs.iter().map(|rgb| CouleurGpl { rgb: *rgb, nom: String::new() }).collect(),
        });
        let (relue, rapport) = depuis_gpl(&texte).expect("notre propre ecriture doit se relire");
        assert_eq!(rapport.lignes_ignorees, 0, "aucune couleur perdue a la relecture");
        assert_eq!(relue.couleurs.iter().map(|c| c.rgb).collect::<Vec<_>>(), couleurs);
    }
}
