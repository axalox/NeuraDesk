//! **LA VISIONNEUSE** — afficher un fichier TEXTE ou une IMAGE dans une vraie
//! fenêtre, redimensionnable et épinglable, **jusqu'à quatre à la fois**.
//!
//! ⛔ **C'EST LE PREMIER MODULE À POOL DE CE DÉPÔT, ET C'EST TOUT LE SUJET.**
//! `cef_popup` est bâti autour d'**UN `PopupSlot` par module** : un slot se
//! désigne par un `&'static LocalKey<PopupSlot>`, et chaque `wndproc` nomme le
//! sien en clair. Ici il y en a quatre, donc :
//!
//! - **quatre `thread_local!`** (`SLOT0`…`SLOT3`), jamais un tableau de slots —
//!   `LocalKey` n'est pas une valeur qu'on range ;
//! - **quatre `SPECS`**, avec des noms de classe Win32 DISTINCTS (ils doivent
//!   l'être : `RegisterClassExW` refuse un doublon) ;
//! - **un seul `wndproc`**, qui retrouve son créneau par le `hwnd` — c'est
//!   `cef_popup::slot_pour_hwnd`, ajouté pour ce module, et il ne fait que
//!   parcourir le registre comme le fait déjà `visible_popup_hwnd`.
//!
//! ## Pourquoi une `fenetre`, et pas un panneau
//!
//! Décision de l'utilisateur : « une fenêtre redimensionnable qui puisse rester
//! au premier plan ». `PopupSpec::fenetre` est exactement cela — barre des tâches,
//! Alt-Tab, taille suivie sur la zone cliente, ni topmost ni toolwindow.
//!
//! ⛔ **ET IL FAUT DONC TOUT RÉIMPLÉMENTER SOI-MÊME** : `create_window` pose
//! `WS_POPUP`, donc **ni barre de titre, ni bordure, ni redimensionnement
//! système**. Le modèle est `win32::cef_couleurs` (le Sélecteur de couleurs, seul
//! autre `fenetre: true` du dépôt) : `WM_NCHITTEST` pour les huit bords, et
//! `HTCAPTION` pour le déplacement. Sans ça la fenêtre est FIGÉE, et rien ne le
//! dit.
//!
//! ## Ce qui décide du créneau
//!
//! Le premier libre ; à défaut, **le plus ancien**, et on le DIT au journal — un
//! remplacement silencieux ferait croire qu'on a ouvert une cinquième fenêtre.
//! Un créneau se libère sur `WM_CLOSE` : c'est ce qui empêche le pool de se
//! saturer de fenêtres fermées.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetSystemMetrics, SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SM_CXSCREEN,
    SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WM_CLOSE, WM_NCHITTEST,
};

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};

/// **LE NOMBRE DE FENÊTRES SIMULTANÉES** — décision de l'utilisateur : « pour
/// comparer deux images côte à côte ». Quatre laisse de la marge sans payer
/// quatre navigateurs CEF à tous les lancements (ils sont créés À LA DEMANDE).
const CRENEAUX: usize = 4;

/// Le décalage en cascade entre deux fenêtres ouvertes — sans lui, les quatre se
/// superposeraient EXACTEMENT, et on ne verrait pas qu'il y en a quatre.
const CASCADE: i32 = 28;

/// La largeur de la bande de redimensionnement, en pixels. Copiée du Sélecteur de
/// couleurs (`cef_couleurs::BORDURE_REDIMENSIONNEMENT`) : la fenêtre n'a pas de
/// bordure système, c'est donc à nous de décider où le redimensionnement commence.
const BORDURE: i32 = 6;

/// Taille d'ouverture, bornée à 90 % de l'écran.
const LARGEUR_DEFAUT: i32 = 900;
const HAUTEUR_DEFAUT: i32 = 700;

/// **CE QU'UN CRÉNEAU MONTRE** : le fichier, le numéro d'ordre qui dit depuis
/// quand il est ouvert (le plus ancien cède sa place), et **la SÉRIE que la flèche
/// parcourt**.
///
/// ⚠️ **LE TRI DE LA MOITIÉ QUI A OUVERT VOYAGE JUSQU'ICI, mais il n'est pas
/// retenu** : il ne sert qu'à construire la série, une seule fois, à l'ouverture
/// (`montrer`). Les deux moitiés du dash dossier n'ont pas le même ordre ; une
/// visionneuse qui re-choisirait le sien ferait défiler les images dans un autre
/// ordre que la grille dont elles sortent — le défaut des « deux ordres dans la
/// même fenêtre ».
#[derive(Clone)]
struct Vue {
    chemin: PathBuf,
    numero: u64,
    /// **LA SÉRIE RETENUE À L'OUVERTURE** — les fichiers du même genre du dossier,
    /// dans l'ordre de la moitié qui a ouvert. Elle sert aux DEUX choses qu'une flèche demande :
    /// dire laquelle éteindre, et changer de fichier.
    ///
    /// ⛔ **RETENUE, et c'est un garde de performance, pas une preference.** Sans elle,
    /// chaque appui relisait le dossier DEUX fois : `etat_json` pour les deux
    /// booléens, `changer_de_voisin` pour le voisin. `sous_dock::enfants()` lit
    /// jusqu'à `ENFANTS_MAX` entrées et `Tri::Date`/`Tri::Taille` paient un
    /// `metadata()` PAR entrée (voir `serie_de_media`, où la mesure est écrite) :
    /// trois mille photos triées par date, c'est six mille statistiques sur le fil
    /// unique de `app::run`, donc sur le dock et les pages CEF.
    ///
    /// ⚠️ **ELLE NE SE RAFRAÎCHIT QU'À L'OUVERTURE** : un fichier supprimé pendant
    /// que la fenêtre est ouverte se signale, et la fenêtre garde ce qu'elle
    /// montrait (voir `changer_de_voisin`). Le lecteur fait pareil, à l'ouverture.
    /// Un tri changé DANS la moitié du dash après l'ouverture ne la change pas non
    /// plus : la flèche continue de suivre l'ordre où la grille était au moment du
    /// clic, et c'est moins trompeur que deux ordres dans la même fenêtre.
    serie: Vec<PathBuf>,
}

thread_local! {
    /// ⛔ **QUATRE STATIQUES, PAS UN TABLEAU** : un slot se désigne par son
    /// `LocalKey`, qui n'est pas une valeur rangeable. C'est la contrainte qui
    /// structure tout ce module.
    static SLOT0: PopupSlot = const { PopupSlot::new() };
    static SLOT1: PopupSlot = const { PopupSlot::new() };
    static SLOT2: PopupSlot = const { PopupSlot::new() };
    static SLOT3: PopupSlot = const { PopupSlot::new() };
    /// Ce que chaque créneau montre — voir `Vue` pour le pourquoi du tri.
    static OCCUPATION: RefCell<[Option<Vue>; CRENEAUX]> =
        const { RefCell::new([None, None, None, None]) };
    /// Le compteur d'attribution des créneaux. Il ne redescend jamais : comparer
    /// deux numéros suffit à dire lequel est le plus ancien.
    static ORDRE: Cell<u64> = const { Cell::new(0) };
    /// **L'ÉTAT DÉJÀ SÉRIALISÉ de chaque créneau**, gardé pour être REPOUSSÉ.
    ///
    /// ⛔ **REPOUSSER N'EST PAS UNE PRÉCAUTION, C'EST UNE NÉCESSITÉ MESURÉE.** La
    /// première version poussait UNE fois, juste après la création de la fenêtre —
    /// et la capture a montré une fenêtre parfaitement dessinée (bande de titre,
    /// boutons) et **VIDE** : la poussée était arrivée avant que
    /// `window.setApercu` n'existe côté JS, et `execute_js` ne rend aucun compte
    /// de son échec. C'est le même défaut, mesuré trois fois ailleurs dans ce
    /// dépôt, et le même remède : repousser tant que la page peut ne pas avoir
    /// fini de charger.
    ///
    /// ⚠️ **ON GARDE LA CHAÎNE DÉJÀ CONSTRUITE**, jamais le contenu : un fichier
    /// de 1 Mo resérialisé à 60 Hz serait ruineux, alors que renvoyer une chaîne
    /// qui existe déjà ne coûte qu'une copie.
    static ETAT: RefCell<[Option<String>; CRENEAUX]> = const { RefCell::new([None, None, None, None]) };
    /// Les frames restantes de la rafale de poussée, par créneau.
    static RAFALE: [Cell<u32>; CRENEAUX] =
        [Cell::new(0), Cell::new(0), Cell::new(0), Cell::new(0)];
}

thread_local! {
    /// Le cadre d'avant de chaque créneau AGRANDI (voir `basculer_l_agrandissement`),
    /// `None` : à sa taille.
    static AGRANDIE: RefCell<[Option<windows::Win32::Foundation::RECT>; CRENEAUX]> = const { RefCell::new([None; CRENEAUX]) };
}

/// ⭐ **LES VISIONNEUSES EN PLEIN ÉCRAN, pour le dock** — même raison que
/// `lecteur_video::EN_PLEIN_ECRAN` : le dock ignore les fenêtres de l'application
/// en cherchant un plein écran, et ses barres topmost passeraient devant l'image.
/// Hors du fil (une simple liste), pour `window::foreground_fullscreen_window`.
static EN_PLEIN_ECRAN: std::sync::Mutex<Vec<isize>> = std::sync::Mutex::new(Vec::new());

/// `hwnd` est-il une visionneuse en plein écran ?
pub fn est_en_plein_ecran(hwnd: HWND) -> bool {
    EN_PLEIN_ECRAN.lock().unwrap_or_else(|e| e.into_inner()).contains(&(hwnd.0 as isize))
}

fn noter_plein_ecran(hwnd: HWND, plein: bool) {
    let mut liste = EN_PLEIN_ECRAN.lock().unwrap_or_else(|e| e.into_inner());
    liste.retain(|&h| h != hwnd.0 as isize);
    if plein {
        liste.push(hwnd.0 as isize);
    }
}

/// Le créneau `rang` est-il agrandi ?
fn est_agrandie(rang: usize) -> bool {
    AGRANDIE.with(|a| a.borrow()[rang].is_some())
}

/// Où agrandir : la ZONE CENTRALE (la zone de travail, que les barres du dock
/// réservent) quand l'application y met les pleins écrans (Réglages, voir
/// `plein_ecran_zone`), sinon l'écran entier. `ecran`, `travail` : ceux de la
/// fenêtre ; le booléen dit si c'est l'écran entier.
fn cadre_agrandi(ecran: windows::Win32::Foundation::RECT, travail: windows::Win32::Foundation::RECT, dans_la_zone: bool) -> (windows::Win32::Foundation::RECT, bool) {
    if dans_la_zone { (travail, false) } else { (ecran, true) }
}

/// ⭐ **DOUBLE CLIC SUR UNE IMAGE : EN GRAND, OU DE RETOUR** — DEMANDE EXPLICITE
/// (2026-10-19) : « qu'une image affichée puisse être affichée en plein écran ou
/// dans la zone centrale suivant les réglages de l'app par un double clic ». Le
/// cadre d'avant est gardé pour le retour ; en grand, la page cache sa bande de
/// titre, zoome à la molette et se parcourt en glissant (voir `visionneuse.html`,
/// `setGrand`).
fn basculer_l_agrandissement(rang: usize) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, HWND_TOP};
    let slot = SLOTS[rang];
    let Some(hwnd) = cef_popup::hwnd(slot) else { return };
    let avant = AGRANDIE.with(|a| a.borrow_mut()[rang].take());
    match avant {
        Some(cadre) => unsafe {
            noter_plein_ecran(hwnd, false);
            let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), cadre.left, cadre.top, cadre.right - cadre.left, cadre.bottom - cadre.top, Default::default());
            appliquer_l_epinglage(slot);
        },
        None => unsafe {
            let mut cadre = RECT::default();
            let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetWindowRect(hwnd, &mut cadre).is_err() || !GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
                return;
            }
            let (cible, plein_ecran) = cadre_agrandi(info.rcMonitor, info.rcWork, crate::win32::plein_ecran_zone::est_active());
            AGRANDIE.with(|a| a.borrow_mut()[rang] = Some(cadre));
            noter_plein_ecran(hwnd, plein_ecran);
            // En plein écran, au-dessus de tout, barres du dock comprises.
            let rang_z = if plein_ecran { HWND_TOPMOST } else { HWND_TOP };
            let _ = SetWindowPos(hwnd, Some(rang_z), cible.left, cible.top, cible.right - cible.left, cible.bottom - cible.top, Default::default());
        },
    }
    let grand = est_agrandie(rang);
    crate::diag!("[visionneuse] creneau {rang} : agrandie = {grand}");
    cef_popup::execute_js(slot, &format!("if (window.setGrand) window.setGrand({grand})"));
}

/// Combien de frames on repousse après un changement — une seconde à 60 Hz. Même
/// valeur et même raison que `cef_sous_dock::RAFALE_DE_POUSSEE`, dont la doc porte
/// la mesure : la poussée sert à survivre au CHARGEMENT de la page, elle n'a
/// aucune raison de durer ensuite.
const RAFALE_DE_POUSSEE: u32 = 60;

/// Les frames restantes de la rafale d'un créneau. ⚠️ **UN ACCESSEUR, ET PAS UN
/// `RAFALE[i]`** : un `thread_local!` est une `LocalKey`, pas la valeur — il n'est
/// pas indexable, et `with` est le seul chemin.
fn rafale_restante(rang: usize) -> u32 {
    RAFALE.with(|rafale| rafale[rang].get())
}

fn armer_la_rafale(rang: usize) {
    RAFALE.with(|rafale| rafale[rang].set(RAFALE_DE_POUSSEE));
}

fn decroitre_la_rafale(rang: usize) {
    RAFALE.with(|rafale| rafale[rang].set(rafale[rang].get().saturating_sub(1)));
}

/// Les quatre slots, dans l'ORDRE des créneaux — index et place coïncident.
const SLOTS: [&'static std::thread::LocalKey<PopupSlot>; CRENEAUX] =
    [&SLOT0, &SLOT1, &SLOT2, &SLOT3];

/// ⚠️ **QUATRE NOMS DE CLASSE DISTINCTS** : `RegisterClassExW` REFUSE un doublon,
/// et le refus serait silencieux ici (la fenêtre ne s'ouvrirait pas, sans un mot).
macro_rules! spec_visionneuse {
    ($classe:literal, $titre:literal) => {
        PopupSpec {
            class_name: w!($classe),
            window_title: w!($titre),
            html_file: "visionneuse.html",
            wndproc: Some(wndproc),
            // Sans objet pour une fenêtre — ces deux drapeaux départagent des
            // FAMILLES de popups ancrés au dock, et une fenêtre n'y est pas.
            above_a_magnified_dock: false,
            // ⛔ **`fades: false` EST OBLIGATOIRE** : un fondu demande
            // `WS_EX_LAYERED`, qui REMPLACE le comportement d'une vraie fenêtre.
            fades: false,
            fenetre: true,
        }
    };
}

const SPECS: [PopupSpec; CRENEAUX] = [
    spec_visionneuse!("NeuraDeskVisionneuse0", "NeuraDesk — Aperçu 1"),
    spec_visionneuse!("NeuraDeskVisionneuse1", "NeuraDesk — Aperçu 2"),
    spec_visionneuse!("NeuraDeskVisionneuse2", "NeuraDesk — Aperçu 3"),
    spec_visionneuse!("NeuraDeskVisionneuse3", "NeuraDesk — Aperçu 4"),
];

/// **LE CRÉNEAU D'UN SLOT** — la seule correspondance entre le registre de
/// `cef_popup` (qui ne connaît que des `LocalKey`) et notre indexation.
fn creneau(slot: &'static std::thread::LocalKey<PopupSlot>) -> Option<usize> {
    SLOTS.iter().position(|connu| std::ptr::eq(*connu, slot))
}

/// L'état d'une visionneuse — ce que la page reçoit.
///
/// ⚠️ **LE TEXTE VOYAGE DANS L'ÉTAT**, à la différence des icônes du dash dossier :
/// il est borné à `texte_fichier::TAILLE_MAX` (1 Mo), il n'est poussé qu'à
/// l'ouverture et au rafraîchissement, et une visionneuse n'a pas la cadence d'une
/// grille qu'on survole. Un canal séparé serait de la machinerie pour rien.
/// `serie` est la série RETENUE du créneau (voir `Vue`) : l'état ne relit
/// JAMAIS le dossier, il lit deux fois le même tableau en mémoire.
fn etat_json(chemin: &Path, charge: &Charge, serie: &[PathBuf]) -> String {
    let nom = chemin
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| chemin.display().to_string());
    let base = serde_json::json!({
        "chemin": chemin.to_string_lossy(),
        "nom": nom,
        "epingle": crate::config::load_visionneuse_premier_plan(),
        // La TAILLE DU CONTENU (Réglages) — prise à l'ouverture et au
        // rafraîchissement, les deux seuls moments où cet état part.
        // La taille des textes GLOBALE s'y ajoute (`taille_panneaux::TEXTES`).
        "echelle": crate::win32::taille_panneaux::facteur("visionneuse") * crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES),
    });
    let mut objet = base.as_object().cloned().unwrap_or_default();
    match charge {
        Charge::Texte(lu) => {
            objet.insert("genre".into(), "texte".into());
            objet.insert("texte".into(), lu.contenu.clone().into());
            objet.insert("encodage".into(), lu.encodage.nom().into());
            objet.insert("approximatif".into(), lu.encodage.approximatif().into());
            objet.insert("tronque".into(), lu.tronque.into());
        }
        Charge::Image { url } => {
            objet.insert("genre".into(), "image".into());
            objet.insert("image".into(), url.clone().into());
        }
        Charge::Illisible { raison } => {
            objet.insert("genre".into(), "erreur".into());
            objet.insert("erreur".into(), raison.clone().into());
        }
    }
    // ⭐ **LES FLÈCHES DE VOISINAGE** — DEMANDE EXPLICITE (2026-11-03) : « des
    // flèches en surbrillance pour passer à la photo suivante ». La page ne reçoit
    // que DEUX BOOLÉENS et les noms pour l'infobulle : elle ne connaît ni le
    // dossier ni l'ordre, et deux listes qu'on range à deux endroits finissent par
    // diverger (voir `Vue`, où le tri est écrit). Rien du tout pour un fichier
    // qu'aucune flèche ne parcourt — un texte, un `.pdf`.
    let (avant, apres) = crate::win32::sous_dock::voisins_dans_serie(serie, chemin);
    let nom = |chemin: &Option<PathBuf>| {
        chemin.as_ref().and_then(|c| c.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    };
    objet.insert("precedent".into(), avant.is_some().into());
    objet.insert("suivant".into(), apres.is_some().into());
    objet.insert("nom_precedent".into(), nom(&avant).into());
    objet.insert("nom_suivant".into(), nom(&apres).into());
    serde_json::Value::Object(objet).to_string()
}

/// Ce qu'on a réussi à lire — **y compris l'ÉCHEC, qui se montre**.
///
/// ⛔ Un fichier illisible n'est pas un cas qu'on peut éviter : binaire,
/// verrouillé, disparu entre le clic et la lecture. Le taire laisserait une
/// fenêtre vide dont personne ne saurait dire pourquoi.
enum Charge {
    Texte(crate::win32::texte_fichier::TexteLu),
    /// L'adresse où la page la lit (voir `cef_data::publier`).
    Image { url: String },
    Illisible { raison: String },
}

/// **LIT LE FICHIER ET DÉCIDE DE SA FORME.** Le choix se fait sur l'EXTENSION,
/// et lui seul : deviner au contenu demanderait d'ouvrir tous les fichiers.
fn charger(chemin: &Path) -> Charge {
    let extension = chemin
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if crate::win32::texte_fichier::est_une_image(&extension) {
        let Some(mime) = crate::win32::texte_fichier::mime_pour(&extension) else {
            return Charge::Illisible { raison: format!("format « .{extension} » inconnu") };
        };
        // ⛔ **PLUS DE `data:` DANS L'ÉTAT** — les grosses images ne s'affichaient
        // pas (voir `cef_data::FICHIERS`). La page les lit par une adresse ; le
        // fichier n'est qu'OUVERT ici, pour qu'un fichier disparu se dise.
        return match std::fs::File::open(chemin) {
            Ok(_) => Charge::Image { url: crate::win32::cef_data::publier(chemin, mime) },
            Err(erreur) => Charge::Illisible { raison: format!("lecture impossible : {erreur}") },
        };
    }
    match crate::win32::texte_fichier::lire(chemin) {
        Ok(lu) => Charge::Texte(lu),
        Err(raison) => Charge::Illisible { raison: format!("lecture impossible : {raison}") },
    }
}

/// **LE CRÉNEAU À PRENDRE** — le premier libre, sinon le plus ancien.
///
/// ⚠️ **UN CRÉNEAU EST LIBRE QUAND SA FENÊTRE EST FERMÉE**, pas quand elle est
/// cachée : `WM_CLOSE` remet `OCCUPATION` à `None` (voir `wndproc`), et c'est la
/// seule chose qui rende le pool utilisable au-delà de quatre fichiers.
fn creneau_a_prendre() -> (usize, Option<PathBuf>) {
    OCCUPATION.with(|occupation| {
        let occupation = occupation.borrow();
        if let Some(libre) = occupation.iter().position(|entree| entree.is_none()) {
            return (libre, None);
        }
        // Les quatre sont pris : on reprend le PLUS ANCIEN, et on dit lequel.
        let (rang, _) = occupation
            .iter()
            .enumerate()
            .filter_map(|(rang, entree)| entree.as_ref().map(|vue| (rang, vue.numero)))
            .min_by_key(|(_, ordre)| *ordre)
            .unwrap_or((0, 0));
        (rang, occupation[rang].as_ref().map(|vue| vue.chemin.clone()))
    })
}

/// Ouvre `chemin` dans une visionneuse — un créneau libre, ou le plus ancien.
///
/// `tri` est l'ordre de la moitié du dash dossier qui a ouvert le fichier : la
/// flèche de voisinage s'y range (voir `Vue`). `Tri::default()` convient à qui ne
/// vient pas d'une moitié, comme le harnais de capture.
///
/// Rend `Err` avec une raison quand la fenêtre n'a pas pu être créée : l'appelant
/// la montre, parce qu'un clic qui n'ouvre rien sans un mot est le pire des cas.
pub fn montrer(gfx: &GfxDevice, chemin: &Path, tri: crate::win32::sous_dock::Tri) -> Result<(), String> {
    let (rang, remplace) = creneau_a_prendre();
    if let Some(ancien) = remplace {
        // ⛔ **ON LE DIT.** Un remplacement silencieux ferait croire à une
        // cinquième fenêtre, et l'utilisateur chercherait la première.
        crate::diag!(
            "[visionneuse] les {CRENEAUX} creneaux sont pris : le plus ancien ({}) cede la place",
            ancien.display()
        );
    }
    let slot = SLOTS[rang];
    let rect = rect_pour(rang);
    // ⛔ **`montrer_fenetre` REND « CRÉÉE », PAS « RÉUSSIE »** — `false` pour une
    // fenêtre RÉUTILISÉE. DÉFAUT SIGNALÉ (2026-10-23) : « marche une fois puis
    // affiche toujours la même image ». Lu comme un échec, ce `false` sortait
    // AVANT `pousser` : la fenêtre, déjà remontrée, gardait l'image d'avant
    // (journal : « n'a pas pu être créée » puis « montrée 0x6e21202 »). L'échec
    // réel, c'est qu'il n'y ait AUCUNE fenêtre après l'appel.
    cef_popup::montrer_fenetre(slot, &SPECS[rang], rect, gfx);
    if cef_popup::hwnd(slot).is_none() {
        return Err("la fenêtre d'aperçu n'a pas pu être créée".to_string());
    }
    ORDRE.with(|ordre| ordre.set(ordre.get() + 1));
    let numero = ORDRE.with(Cell::get);
    // LA SEULE LECTURE DU DOSSIER de cette fenêtre : la série est retenue et les
    // flèches la liront sans plus rien relire (voir `Vue.serie`).
    let serie = crate::win32::sous_dock::genre_de_media(chemin)
        .map(|genre| crate::win32::sous_dock::serie_de_media(chemin, tri, genre))
        .unwrap_or_default();
    OCCUPATION.with(|occupation| {
        occupation.borrow_mut()[rang] = Some(Vue { chemin: chemin.to_path_buf(), numero, serie });
    });
    pousser(rang, chemin);
    // ⛔ **L'ÉPINGLAGE S'APPLIQUE À CHAQUE OUVERTURE**, jamais seulement à la
    // création : `WS_EX_APPWINDOW` REMPLACE `WS_EX_TOPMOST` dans `create_window`,
    // donc une fenêtre réutilisée a perdu le sien.
    appliquer_l_epinglage(slot);
    Ok(())
}

/// **OÙ POSER LA FENÊTRE DU CRÉNEAU `rang`** — centrée, puis décalée en cascade.
///
/// ⚠️ **SÉPARÉE DE LA LECTURE WIN32 POUR ÊTRE VÉRIFIABLE** — même raison que
/// `cef_caracteres::centrer_sur_ecran` : `GetSystemMetrics` n'est pas injectable,
/// donc tant que le calcul vivait avec lui, aucun test ne pouvait l'éprouver sur
/// un autre écran que celui de la machine. C'est exactement là qu'il se trompait.
///
/// ⛔ **LE DÉCALAGE N'ÉTAIT BORNÉ QUE PAR LE BAS** (`.max(0)`), jamais par le bord.
/// Sur un écran dont la hauteur est proche de celle de la fenêtre, la marge du
/// centrage `(hauteur - h) / 2` devient plus PETITE que `CASCADE × rang`, et les
/// dernières fenêtres sortaient par le bas — **45 px sur un 1366×768, 48 px sur un
/// 1280×720**. Le plafond de TAILLE (90 % de l'écran, déjà présent) ne suffisait
/// pas : il garantit que la fenêtre tient SEULE, pas que le centrage PLUS le
/// décalage tiennent.
///
/// ⛔ **ET ON RÉDUIT LE PAS, ON NE ROGNE PAS LE DÉCALAGE.** Un `.clamp` sur la
/// position aurait ramené les derniers crans sur la MÊME coordonnée — deux
/// fenêtres exactement superposées, c'est-à-dire la cascade ANNULÉE, alors que
/// c'est toute sa raison d'être (voir `les_fenetres_sont_decalees_en_cascade`).
/// Le pas se rétracte donc à ce que la marge la plus étroite accepte.
///
/// ⚠️ **LE SEUIL EST 868 px DE HAUT *ET* 1 068 px DE LARGE** — c'est l'axe le plus
/// COURT qui commande (`marge = min(cx, cy)`), donc une hauteur seule ne suffit
/// pas à conclure. Un **1024×2160** a une hauteur très au-dessus du seuil et n'est
/// pourtant PAS un no-op : sa largeur donne `cx = 62`, donc un pas de 20 au lieu
/// de 28. Au-dessus des DEUX seuils la cascade tient telle quelle et cette
/// fonction est un NO-OP : c'est ce que fige
/// `la_cascade_ne_bouge_pas_quand_elle_tient_deja`.
///
/// ⚠️ **LA GARANTIE « ÇA TIENT » SUPPOSE UN ÉCRAN D'AU MOINS 240 × 160** — les
/// planchers de `l` et `h`, qui existent pour qu'une fenêtre reste utilisable. En
/// dessous, `l` peut dépasser `largeur` et la fenêtre déborde — le comportement
/// d'AVANT ce correctif, à l'identique : ce n'est pas une régression, mais ce
/// n'est pas non plus l'invariant que la doc promettait, et aucun écran réel
/// n'est dans ce cas.
///
/// ⚠️ **ET LA CASCADE ELLE-MÊME SE CONFOND QUAND LA MARGE TOMBE SOUS
/// `CRENEAUX - 1`** (3 px) : le pas devient nul et les quatre fenêtres se
/// superposent. C'est le cas des écrans de moins de ~245 px sur un axe — aucun
/// écran réel non plus, mais c'est une CONDITION du code, pas une conséquence.
fn rect_pour_ecran(rang: usize, largeur: i32, hauteur: i32) -> [i32; 4] {
    // Le pas divise par `CRENEAUX - 1` : le dire une fois pour toutes plutôt que
    // de porter un `else` que la constante rend inatteignable (voir la revue).
    const _: () = assert!(CRENEAUX > 1, "un pas de cascade se divise par CRENEAUX - 1");
    // ⚠️ **LA BORNE `rang < CRENEAUX` EST UNE CONDITION DE L'INVARIANT, pas un
    // détail** : hors de là, `decalage` peut dépasser la marge et la fenêtre sort
    // de l'écran. Les trois appelants la respectent ; le calcul étant désormais
    // adressable par son nom, autant qu'il le dise.
    debug_assert!(rang < CRENEAUX, "rang {rang} hors des {CRENEAUX} créneaux");
    let l = LARGEUR_DEFAUT.min(largeur * 9 / 10).max(240);
    let h = HAUTEUR_DEFAUT.min(hauteur * 9 / 10).max(160);
    let (cx, cy) = ((largeur - l) / 2, (hauteur - h) / 2);
    // La marge la plus étroite des deux axes : le décalage s'applique aux DEUX,
    // donc c'est elle qui commande.
    let marge = cx.min(cy).max(0);
    let pas = CASCADE.min(marge / (CRENEAUX as i32 - 1));
    let decalage = pas * rang as i32;
    [(cx + decalage).max(0), (cy + decalage).max(0), l, h]
}

/// Où poser la fenêtre du créneau `rang` — **l'écran PRIMAIRE**, en pixels
/// physiques (voir `rect_pour_ecran`, qui porte le calcul et ses raisons).
fn rect_pour(rang: usize) -> [i32; 4] {
    let (largeur, hauteur) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    rect_pour_ecran(rang, largeur, hauteur)
}

/// Prépare l'état de ce créneau, le RETIENT, et commence la rafale.
fn pousser(rang: usize, chemin: &Path) {
    let charge = charger(chemin);
    // La série vient du créneau, jamais d'un réglage global : c'est le dossier et
    // le tri de la moitié qui a ouvert CE fichier. `montrer` l'a écrite avant de
    // pousser, `changer_de_voisin` la garde telle quelle.
    //
    // ⛔ **UN CRÉNEAU SANS OCCUPANT SE DIT** : pousser là rendrait deux flèches
    // éteintes sur une fenêtre qui montre quelque chose — l'état serait faux sans
    // aucune trace. `unwrap_or_default()` seul masquait l'incohérence d'état.
    let serie = OCCUPATION.with(|o| match o.borrow()[rang].as_ref() {
        Some(vue) => vue.serie.clone(),
        None => {
            crate::diag!("[visionneuse] pousser sur le creneau {rang} sans occupant : {:?} montre sans serie", chemin.file_name());
            Vec::new()
        }
    });
    let json = etat_json(chemin, &charge, &serie);
    ETAT.with(|etat| etat.borrow_mut()[rang] = Some(json));
    armer_la_rafale(rang);
    envoyer(rang);
}

/// Envoie l'état RETENU de ce créneau — sans le reconstruire.
fn envoyer(rang: usize) {
    let Some(json) = ETAT.with(|etat| etat.borrow()[rang].clone()) else { return };
    let expression = format!("if (window.setApercu) window.setApercu({json})");
    cef_popup::execute_js(SLOTS[rang], &expression);
}

/// **À APPELER À CHAQUE FRAME** — repousse l'état des créneaux dont la page peut
/// ne pas avoir fini de charger (voir `ETAT`, où le défaut mesuré est écrit).
///
/// ⚠️ **UN CRÉNEAU CACHÉ N'EST PAS REPOUSSÉ** : rouvrir une fenêtre déjà chargée
/// n'a pas besoin de rafale, et c'est `montrer` qui en redémarre une.
pub fn poll() {
    for rang in 0..CRENEAUX {
        if rafale_restante(rang) == 0 {
            continue;
        }
        if !cef_popup::is_visible(SLOTS[rang]) {
            continue;
        }
        decroitre_la_rafale(rang);
        envoyer(rang);
    }
}

/// Le sens de la flèche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sens {
    Precedent,
    Suivant,
}

/// **LA FLÈCHE : LE CRÉNEAU SE ROUVRE SUR LE VOISIN.**
///
/// Rien n'est recréé — même fenêtre, même cadre, même épinglage : seul le fichier
/// change, et `pousser` fait le reste (relire, republier l'image par
/// `cef_data::publier`, repousser l'état). `numero` ne bouge pas : défiler n'est
/// pas rouvrir, et le créneau le plus ancien doit rester celui qui cède la place.
///
/// ⛔ **UN VOISIN QUI A DISPARU SE DIT, ET LA FENÊTRE GARDE CE QU'ELLE MONTRAIT.**
/// La page n'émet ce verbe que si l'état lui a annoncé un voisin : n'y arriver que
/// maintenant veut dire que le disque a changé entre la poussée et le clic. Vider
/// la fenêtre pour un fichier manquant serait pire que n'avoir pas de flèche.
fn changer_de_voisin(rang: usize, sens: Sens) {
    let Some(vue) = OCCUPATION.with(|o| o.borrow()[rang].clone()) else { return };
    // Le genre ne sert qu'à le DIRE : un fichier qu'aucune flèche ne parcourt
    // (un texte, un `.pdf`) a sa série vide à l'ouverture, donc son voisin est déjà
    // introuvable — mais le message ci-dessous nomme la cause plutôt qu'un rang.
    if crate::win32::sous_dock::genre_de_media(&vue.chemin).is_none() {
        crate::diag!("[visionneuse] une fleche a ete demandee sur un fichier qu'aucune fleche ne parcourt : {:?}", vue.chemin);
        return;
    }
    // ZÉRO lecture de dossier ici : la série du créneau a été relue à l'ouverture
    // (voir `Vue.serie`, où le coût évité est écrit).
    let (avant, apres) = crate::win32::sous_dock::voisins_dans_serie(&vue.serie, &vue.chemin);
    let Some(nouveau) = (match sens { Sens::Precedent => avant, Sens::Suivant => apres }) else {
        crate::diag!("[visionneuse] {} de {:?} introuvable dans {:?}", match sens { Sens::Precedent => "precedent", Sens::Suivant => "suivant" }, vue.chemin, vue.chemin.parent());
        return;
    };
    OCCUPATION.with(|o| {
        if let Some(vue) = o.borrow_mut()[rang].as_mut() {
            vue.chemin = nouveau.clone();
        }
    });
    pousser(rang, &nouveau);
    crate::diag!("[visionneuse] fleche {sens:?} : {:?}", nouveau.file_name());
}

/// **UN CLIC DANS LA VISIONNEUSE, SANS SOURIS NI `SendInput`** — le MÊME événement
/// CEF que `cef_popup::debug_click` : l'appui et le relâchement partent du
/// navigateur, jamais du système (voir la contrainte de session « jamais d'entrée
/// simulée »). Sert à éprouver les flèches de voisinage **de bout en bout** — la
/// page clique, émet son verbe, le dispatch change de fichier, et la capture
/// d'après montre l'image suivante.
///
/// ⚠️ **`x`/`y` sont des pixels PHYSIQUES**, comme partout dans ces leviers : le
/// chemin passe par `ScreenToClient` sans division par le zoom, donc un rect mesuré
/// en CSS doit être multiplié par `devicePixelRatio`.
pub fn debug_click(x: i32, y: i32) {
    // Le créneau visé est celui qui est À L'ÉCRAN : un run de capture n'ouvre
    // qu'une fenêtre, mais le pool en a quatre et le premier libre n'est pas
    // forcément le zéro.
    let Some(rang) = (0..CRENEAUX).find(|&rang| cef_popup::is_visible(SLOTS[rang])) else {
        crate::diag!("[visionneuse] debug_click : aucun creneau visible");
        return;
    };
    // Le survol D'ABORD : c'est le geste RÉEL (un clic arrive toujours après des
    // `WM_MOUSEMOVE`), et cette page n'allume ses chevrons de bord que sur
    // `mousemove` (`body.fleches` — `:hover` n'est pas tenu en OSR). Ce n'est PAS la
    // cause du clic muet mesuré le 2026-11-03 : la cause était le `transform` d'un
    // `button:active` dans `style.css`, qui déplaçait le cartel entre l'appui et le
    // relâchement (voir `assets/popups/visionneuse.html`).
    cef_popup::debug_hover(SLOTS[rang], x, y);
    cef_popup::debug_click(SLOTS[rang], x, y);
}

/// **CE QUE LA PAGE DEMANDE** — épingler, fermer, agrandir, ou changer de fichier.
pub fn dispatch_actions() {
    for rang in 0..CRENEAUX {
        let slot = SLOTS[rang];
        for action in cef_popup::drain_actions(slot) {
            match action.as_str() {
                "visionneuse/fermer" => {
                    if let Some(hwnd) = cef_popup::hwnd(slot) {
                        cef_popup::cacher_fenetre(hwnd);
                    }
                    liberer(rang);
                }
                "visionneuse/agrandir" => basculer_l_agrandissement(rang),
                // ⭐ **LA FLÈCHE DE VOISINAGE** — DEMANDE EXPLICITE (2026-11-03) :
                // « des flèches en surbrillance pour passer à la photo suivante ».
                // ⛔ **LA PAGE NE DIT PAS OÙ ALLER** : elle ne connaît ni le dossier
                // ni l'ordre, et n'envoie ce verbe que quand l'état lui a dit qu'un
                // voisin existe. Rust choisit le chemin, avec le tri de `Vue`.
                "visionneuse/precedent" => changer_de_voisin(rang, Sens::Precedent),
                "visionneuse/suivant" => changer_de_voisin(rang, Sens::Suivant),
                "visionneuse/epingler" => {
                    // Le réglage est GLOBAL (voir `config`) : le bouton d'UNE
                    // fenêtre vaut pour toutes, et il est écrit tout de suite —
                    // l'application peut être terminée par un arrêt de session.
                    let actif = !crate::config::load_visionneuse_premier_plan();
                    crate::config::save_visionneuse_premier_plan(actif);
                    crate::diag!("[visionneuse] epinglage : premier plan = {actif}");
                    for autre in 0..CRENEAUX {
                        appliquer_l_epinglage(SLOTS[autre]);
                        // ⚠️ **L'ÉTAT EST REPOUSSÉ** : le bouton doit montrer son
                        // nouvel état, sinon un clic parfaitement réussi se
                        // lirait comme un clic sans effet.
                        if let Some(chemin) = OCCUPATION.with(|o| o.borrow()[autre].as_ref().map(|vue| vue.chemin.clone())) {
                            pousser(autre, &chemin);
                        }
                    }
                }
                "visionneuse/deplacer" => {
                    // ⛔ **LE DÉPLACEMENT PASSE PAR LE GESTE NATIF.** La fenêtre
                    // n'a pas de barre de titre : on demande au système de
                    // démarrer un glissement de zone NON CLIENTE, exactement
                    // comme s'il y en avait une. Le refaire à la main (suivre la
                    // souris et déplacer) oublierait l'aimantation aux bords,
                    // l'accrochage et le double-clic — trois choses que
                    // l'utilisateur attend sans les demander.
                    //
                    // En grand, la fenêtre ne se déplace pas.
                    if let Some(hwnd) = cef_popup::hwnd(slot).filter(|_| !est_agrandie(rang)) {
                        cef_popup::deplacer_par_le_systeme(hwnd, "visionneuse");
                    }
                }
                autre => eprintln!("[neuradesk][cef] action visionneuse inconnue : {autre:?}"),
            }
        }
    }
}

/// **MET OU RETIRE LA FENÊTRE AU-DESSUS DE TOUT**, selon le réglage.
///
/// ⚠️ **SANS EFFET SUR UNE FENÊTRE QUI N'EXISTE PAS** — un créneau jamais ouvert
/// n'a pas de `hwnd`, et c'est le cas normal.
fn appliquer_l_epinglage(slot: &'static std::thread::LocalKey<PopupSlot>) {
    if cef_popup::hwnd(slot).is_none() {
        return;
    }
    let Some(hwnd) = cef_popup::hwnd(slot) else { return };
    let au_dessus = crate::config::load_visionneuse_premier_plan();
    let rang = if au_dessus { Some(HWND_TOPMOST) } else { Some(HWND_NOTOPMOST) };
    unsafe {
        let _ = SetWindowPos(hwnd, rang, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Libère le créneau d'un indice — appelé à la FERMETURE, jamais au masquage.
/// Une fenêtre agrandie reprend son cadre d'avant : rouverte, elle revient à sa
/// taille.
fn liberer(rang: usize) {
    OCCUPATION.with(|occupation| occupation.borrow_mut()[rang] = None);
    if est_agrandie(rang) {
        basculer_l_agrandissement(rang);
    }
}

/// À appeler à chaque frame : dessine chaque visionneuse VISIBLE.
pub fn render_if_visible(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    for slot in SLOTS {
        cef_popup::render_if_visible(slot, tint_rgba, 0.0, cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
    }
}

/// Ferme les quatre — pour le harnais, et pour un futur « tout fermer ».
/// Voir `cef_popup::recaler_si_il_ne_tient_plus` — les QUATRE créneaux y passent,
/// pas seulement le premier : chacun a son rectangle et sa propre monte.
pub fn recaler_si_il_ne_tient_plus() {
    for slot in SLOTS {
        cef_popup::recaler_si_il_ne_tient_plus(slot);
    }
}

/// Le premier créneau VISIBLE — ce que le harnais de capture sait photographier,
/// puisqu'il ne connaît qu'UN nom par page.
pub fn debug_dump_backbuffer(path: &Path) -> bool {
    SLOTS
        .iter()
        .find(|slot| cef_popup::is_visible(slot))
        .is_some_and(|slot| cef_popup::debug_dump_backbuffer(slot, path))
}

// ---- LA FENÊTRE -------------------------------------------------------------

/// Les coordonnées ÉCRAN d'un message souris, en SIGNÉ — un simple masquage lirait
/// un écran à gauche du principal comme 65500 (copié de `cef_couleurs`, où le
/// pourquoi est écrit).
fn point_du_message(lparam: LPARAM) -> POINT {
    let valeur = lparam.0 as i32;
    POINT { x: (valeur << 16) >> 16, y: valeur >> 16 }
}

/// Quel curseur de redimensionnement pour ce point ? **Fonction PURE**, testable —
/// la comparer à `GetWindowRect` est la seule partie qui touche Win32.
fn code_pour_point(cadre: [i32; 4], point: POINT) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{
        HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT,
        HTTOPRIGHT,
    };
    let [g, h, l, b] = [cadre[0], cadre[1], cadre[0] + cadre[2], cadre[1] + cadre[3]];
    let gauche = point.x - g < BORDURE;
    let droite = l - point.x < BORDURE;
    let haut = point.y - h < BORDURE;
    let bas = b - point.y < BORDURE;
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

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // ⛔ **LA FENÊTRE N'A NI BORDURE NI BARRE DE TITRE** (voir le module) : le
    // système ne fournit donc PLUS le redimensionnement par les bords, et c'est
    // ici qu'on le rend. Sans ce message, la fenêtre serait FIGÉE à sa taille de
    // création — et rien ne le dirait.
    // En grand, ni bord ni redimensionnement : toute la fenêtre est l'image.
    if msg == WM_NCHITTEST && !cef_popup::slot_pour_hwnd(hwnd).and_then(creneau).is_some_and(est_agrandie) {
        let mut cadre = windows::Win32::Foundation::RECT::default();
        if unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut cadre) }.is_ok() {
            let rect = [cadre.left, cadre.top, cadre.right - cadre.left, cadre.bottom - cadre.top];
            return LRESULT(code_pour_point(rect, point_du_message(lparam)) as isize);
        }
    }
    if msg == WM_CLOSE {
        // **FERMER LIBÈRE LE CRÉNEAU** — c'est ce qui rend le pool réutilisable.
        if let Some(slot) = cef_popup::slot_pour_hwnd(hwnd) {
            if let Some(rang) = creneau(slot) {
                liberer(rang);
            }
        }
        cef_popup::cacher_fenetre(hwnd);
        return LRESULT(0);
    }
    // ⚠️ **UN SEUL `wndproc` POUR QUATRE FENÊTRES** : on retrouve la bonne par le
    // `hwnd`, sinon les messages de la deuxième iraient à la première.
    if let Some(slot) = cef_popup::slot_pour_hwnd(hwnd) {
        if let Some(resultat) = cef_popup::dispatch_input(slot, msg, wparam, lparam) {
            return resultat;
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐ En grand : la zone centrale quand l'application y met les pleins écrans,
    /// sinon l'écran entier — et seul l'écran entier est un plein écran pour le dock.
    #[test]
    fn l_agrandissement_suit_le_reglage_de_la_zone() {
        use windows::Win32::Foundation::RECT;
        let ecran = RECT { left: 0, top: 0, right: 3840, bottom: 2160 };
        let travail = RECT { left: 124, top: 60, right: 3720, bottom: 2084 };
        assert_eq!(cadre_agrandi(ecran, travail, true), (travail, false));
        assert_eq!(cadre_agrandi(ecran, travail, false), (ecran, true));
    }

    /// La liste des visionneuses en plein écran suit l'entrée et la sortie.
    #[test]
    fn le_plein_ecran_d_une_visionneuse_se_note() {
        let h = HWND(0x7e11 as *mut std::ffi::c_void);
        assert!(!est_en_plein_ecran(h));
        noter_plein_ecran(h, true);
        noter_plein_ecran(h, true);
        assert!(est_en_plein_ecran(h));
        noter_plein_ecran(h, false);
        assert!(!est_en_plein_ecran(h), "une seule entrée, même notée deux fois");
    }

    /// ⛔ **LES HUIT BORDS REDIMENSIONNENT, LE CENTRE NON.** Un coin oublié se
    /// voit à l'usage — « cette fenêtre ne s'agrandit pas par en bas à droite » —
    /// et ne se devine pas en lisant le `match`.
    #[test]
    fn code_pour_point_donne_les_huit_bords() {
        use windows::Win32::UI::WindowsAndMessaging::{
            HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT,
            HTTOPRIGHT,
        };
        let cadre = [100, 200, 400, 300]; // gauche, haut, largeur, hauteur
        let coin = |x, y| code_pour_point(cadre, POINT { x, y });

        assert_eq!(coin(100, 200), HTTOPLEFT);
        assert_eq!(coin(499, 200), HTTOPRIGHT);
        assert_eq!(coin(100, 499), HTBOTTOMLEFT);
        assert_eq!(coin(499, 499), HTBOTTOMRIGHT);
        assert_eq!(coin(100, 350), HTLEFT);
        assert_eq!(coin(499, 350), HTRIGHT);
        assert_eq!(coin(300, 200), HTTOP);
        assert_eq!(coin(300, 499), HTBOTTOM);
        // Le CENTRE appartient à la PAGE — c'est ce qui la rend cliquable.
        assert_eq!(coin(300, 350), HTCLIENT, "le centre doit rester au client");
    }

    /// ⛔ **LES QUATRE NOMS DE CLASSE SONT DISTINCTS.** `RegisterClassExW` REFUSE
    /// un doublon, et le refus serait SILENCIEUX ici : la fenêtre ne s'ouvrirait
    /// pas, sans un mot. C'est le genre de faute qu'on ne trouve qu'en la
    /// cherchant.
    #[test]
    fn les_quatre_fenetres_ont_des_noms_de_classe_distincts() {
        let noms: Vec<String> = SPECS
            .iter()
            .map(|spec| unsafe { spec.class_name.to_string() }.unwrap_or_default())
            .collect();
        assert_eq!(noms.len(), CRENEAUX);
        for (rang, nom) in noms.iter().enumerate() {
            assert!(!nom.is_empty(), "le creneau {rang} n'a pas de nom de classe");
            assert_eq!(
                noms.iter().filter(|autre| *autre == nom).count(),
                1,
                "le nom de classe {nom:?} est utilise deux fois — RegisterClassExW le refusera"
            );
        }
    }

    /// ⛔ **LES QUATRE SONT DES FENÊTRES, PAS DES PANNEAUX.** Le contraire les
    /// rendrait incapables de recevoir un redimensionnement et les cacherait de la
    /// barre des tâches — c'est-à-dire tout ce que l'utilisateur a demandé.
    #[test]
    fn les_quatre_sont_de_vraies_fenetres() {
        for (rang, spec) in SPECS.iter().enumerate() {
            assert!(spec.fenetre, "le creneau {rang} doit etre une fenetre");
            assert!(!spec.fades, "un fondu poserait WS_EX_LAYERED, qui remplace la fenetre");
            assert!(!spec.above_a_magnified_dock, "sans objet pour une fenetre");
            assert_eq!(spec.html_file, "visionneuse.html");
        }
    }

    /// **LE CRÉNEAU SE PREND DANS L'ORDRE, PUIS RECYCLE LE PLUS ANCIEN.** C'est la
    /// propriété qui rend le pool utilisable au-delà de quatre fichiers — sans
    /// elle, la cinquième ouverture ne ferait rien, et l'utilisateur croirait
    /// l'application cassée.
    #[test]
    fn le_pool_prend_les_creneaux_libres_puis_le_plus_ancien() {
        // Table rase : d'autres tests du même fil peuvent avoir laissé un état.
        OCCUPATION.with(|o| *o.borrow_mut() = [None, None, None, None]);
        ORDRE.with(|o| o.set(0));

        // Les quatre premiers prennent 0, 1, 2, 3 — dans cet ordre.
        for attendu in 0..CRENEAUX {
            let (rang, remplace) = creneau_a_prendre();
            assert_eq!(rang, attendu);
            assert!(remplace.is_none(), "un creneau libre ne remplace personne");
            ORDRE.with(|o| o.set(o.get() + 1));
            let numero = ORDRE.with(Cell::get);
            OCCUPATION.with(|o| o.borrow_mut()[rang] = Some(Vue {
                chemin: PathBuf::from(format!("f{attendu}")),
                numero,
                serie: Vec::new(),
            }));
        }

        // Les quatre sont pris : le plus ancien (0) cède sa place, et on DIT qui.
        let (rang, remplace) = creneau_a_prendre();
        assert_eq!(rang, 0, "le plus ancien doit ceder sa place");
        assert_eq!(remplace, Some(PathBuf::from("f0")), "et son nom doit etre rendu a l'appelant");

        // ⛔ **ET UN CRÉNEAU LIBÉRÉ SE REPREND** — c'est `WM_CLOSE` qui le libère.
        liberer(2);
        OCCUPATION.with(|o| {
            let occupation = o.borrow();
            assert!(occupation[2].is_none());
            assert!(occupation[1].is_some() && occupation[3].is_some(), "les autres ne bougent pas");
        });
        assert_eq!(creneau_a_prendre().0, 2, "le creneau libere est reprit en premier");

        OCCUPATION.with(|o| *o.borrow_mut() = [None, None, None, None]);
    }

    /// La cascade décale les quatre fenêtres : sans elle, elles se
    /// superposeraient EXACTEMENT et on ne verrait pas qu'il y en a quatre.
    #[test]
    fn les_fenetres_sont_decalees_en_cascade() {
        let premier = rect_pour(0);
        let dernier = rect_pour(CRENEAUX - 1);
        assert_ne!(premier[0], dernier[0], "les fenetres doivent etre decalees");
        assert_ne!(premier[1], dernier[1]);
        assert_eq!(premier[2], dernier[2], "et de la MEME taille");
        assert!(premier[2] > 0 && premier[3] > 0, "une taille nulle ne montrerait rien");
    }

    /// ⛔ **AUCUNE FENÊTRE DE LA CASCADE NE SORT DE L'ÉCRAN** — pour tout écran d'au
    /// moins **240 × 160**, les planchers de `l` et `h`. ⚠️ La réserve n'est pas
    /// rhétorique : en dessous, `l` peut dépasser `largeur` et la fenêtre déborde —
    /// le comportement d'AVANT ce correctif, à l'identique. La doc disait d'abord
    /// « sur AUCUN écran », ce qui était faux.
    ///
    /// Le décalage n'était borné que par le bas (`.max(0)`) : sur un écran dont la
    /// hauteur est proche de celle de la fenêtre, la marge du centrage devient plus
    /// petite que `CASCADE × rang`, et les dernières fenêtres sortaient par le bas.
    ///
    /// ⛔ **ÉCHOUE SUR L'ANCIEN CODE** : à 1366×768, le rang 3 finissait à **813**
    /// pour 768 d'écran ; à 1280×720, à 768 pour 720.
    #[test]
    fn aucune_fenetre_de_la_cascade_ne_sort_de_l_ecran() {
        for (largeur, hauteur) in
            [(3840, 2160), (2560, 1440), (1920, 1080), (1600, 900), (1366, 768), (1280, 720), (1024, 768)]
        {
            let mut vues = Vec::new();
            for rang in 0..CRENEAUX {
                let [x, y, l, h] = rect_pour_ecran(rang, largeur, hauteur);
                // ⚠️ **CES DEUX-LÀ SONT DES PRÉCONDITIONS, PAS DES PROPRIÉTÉS.** Elles
                // ne peuvent pas échouer tant que `.max(240)`/`.max(160)` et le
                // `.max(0)` final sont là — elles ne comptent donc PAS comme
                // couverture. Les deux suivantes, elles, sont mesurées.
                assert!(l > 0 && h > 0, "taille nulle au rang {rang} sur {largeur}×{hauteur}");
                assert!(x >= 0 && y >= 0, "position négative ({x},{y}) au rang {rang} sur {largeur}×{hauteur}");
                assert!(x + l <= largeur, "rang {rang} : {x}+{l} dépasse {largeur}");
                assert!(y + h <= hauteur, "rang {rang} : {y}+{h} dépasse {hauteur}");
                vues.push((x, y));
            }
            // ⭐ **ET LA CASCADE RESTE UNE CASCADE.** C'est la moitié qui interdit
            // le correctif paresseux : un simple `.clamp` aurait ramené les derniers
            // crans sur la MÊME coordonnée — deux fenêtres exactement superposées,
            // donc invisibles l'une sous l'autre, et le décalage annulé.
            //
            // ⚠️ **C'EST UNE CONDITION, PAS UNE GARANTIE ABSOLUE** : elle tient ici
            // parce que ces sept écrans dépassent tous 245 px sur chaque axe, donc
            // `marge >= CRENEAUX - 1` et le pas ne s'annule pas. Sous ~245 px sur un
            // axe, le pas tombe à 0 et les quatre crans se confondent — aucun écran
            // réel n'est dans ce cas, mais le code ne l'interdit pas.
            for i in 0..vues.len() {
                for j in (i + 1)..vues.len() {
                    assert_ne!(vues[i], vues[j], "crans {i} et {j} confondus sur {largeur}×{hauteur}");
                }
            }
        }
    }

    /// ⭐ **LÀ OÙ LA CASCADE TENAIT DÉJÀ, RIEN NE BOUGE** — les valeurs sont celles
    /// du calcul d'AVANT (centre + `CASCADE × rang`), écrites en clair pour que ce
    /// test tombe si le pas change là où il n'y avait rien à corriger.
    #[test]
    fn la_cascade_ne_bouge_pas_quand_elle_tient_deja() {
        for (largeur, hauteur, attendu) in [
            (3840, 2160, [[1470, 730], [1498, 758], [1526, 786], [1554, 814]]),
            (1920, 1080, [[510, 190], [538, 218], [566, 246], [594, 274]]),
        ] {
            for (rang, [x, y]) in attendu.into_iter().enumerate() {
                let [rx, ry, rl, rh] = rect_pour_ecran(rang, largeur, hauteur);
                assert_eq!([rx, ry], [x, y], "rang {rang} sur {largeur}×{hauteur}");
                assert_eq!([rl, rh], [900, 700], "et la taille ne bouge pas");
            }
        }
    }

    /// ⚠️ **ET LE PAS SE RÉTRACTE AU LIEU DE DISPARAÎTRE** : sur un 1366×768 la
    /// marge verticale ne vaut que 38 px pour trois crans, donc le pas tombe de 28
    /// à 12 — les quatre fenêtres restent distinctes au lieu d'être rognées les
    /// unes sur les autres.
    #[test]
    fn le_pas_se_retracte_sur_un_ecran_court() {
        let dernier = rect_pour_ecran(CRENEAUX - 1, 1366, 768);
        let bas = dernier[1] + dernier[3];
        assert!(bas <= 768, "la dernière sort de l'écran : {bas} sur 768");
        // ⛔ **PAS « EXACTEMENT AU BORD », ET C'EST UNE CORRECTION DE MA PART** :
        // j'avais écrit `assert_eq!(bas, 768)`, par intuition. C'est FAUX — la
        // division entière du pas perd le reste (`38 / 3 = 12`, et `12 × 3 = 36`
        // pour 38 disponibles), donc la dernière s'arrête à 765. La marge perdue
        // est bornée par le nombre de crans, ce qui est la seule chose à exiger.
        assert!(768 - bas < CRENEAUX as i32, "marge perdue inattendue : {} px", 768 - bas);
        let premier = rect_pour_ecran(0, 1366, 768);
        assert!(dernier[1] > premier[1], "et elle est bien DÉCALÉE de la première");
        // ⚠️ Le pas vaut 12 ici, contre `CASCADE` = 28 : la rétraction a bien eu lieu.
        assert!(dernier[1] - premier[1] < CASCADE * (CRENEAUX as i32 - 1), "le pas ne s'est pas rétracté");
    }

    // ---- LA GARDE DE PARITÉ page↔dispatch ----------------------------------
    // Le motif des modules frères (`cef_couleurs`, `cef_calculatrice`). Les deux
    // flèches neuves sont exactement le genre de verbe qui se perd d'un côté : la
    // page émet `suivant` et, sans garde, le `match` journalise et oublie — un
    // bouton qui ne fait RIEN, en silence.

    const PAGE: &str = include_str!("../../assets/popups/visionneuse.html");
    const SOURCE: &str = include_str!("cef_visionneuse.rs");

    /// Le premier guillemet après chaque `go(`. ⚠️ **Le scan prend TOUT le texte,
    /// commentaires compris** : un commentaire qui cite un appel avec son verbe
    /// entre guillemets ajoute un verbe FANTÔME (arrivé le 2026-10-27 dans
    /// `cef_couleurs`) — d'où les commentaires de ce module, écrits sans littéral
    /// d'appel.
    fn verbes_de_la_page() -> Vec<String> {
        PAGE.match_indices("go(")
            .filter_map(|(i, _)| {
                let reste = &PAGE[i + 3..];
                let guillemet = reste.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                reste[1..].split(guillemet).next().map(str::to_string)
            })
            .collect()
    }

    /// Les bras de `dispatch_actions`, SCOPÉS à la fonction : le module n'en a
    /// qu'un, mais la découpe est le garde que `cef_couleurs` a payé deux fois —
    /// lire le fichier entier comparerait la page à des verbes d'ailleurs. Une
    /// fonction se termine à la colonne zéro, comme partout ici.
    ///
    /// ⚠️ **LE PRÉFIXE EST ÔTÉ** : la page envoie un segment nu (`go` le précède de
    /// l'hôte), le dispatch lit le verbe complet. Les deux côtés se ramènent à
    /// l'ensemble des verbes, pas à leurs chaînes telles quelles.
    fn verbes_du_dispatch() -> Vec<String> {
        SOURCE
            .split("pub fn dispatch_actions() {")
            .nth(1)
            .unwrap_or_default()
            .split("\n}")
            .next()
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|ligne| !ligne.starts_with("//"))
            .filter_map(|ligne| {
                let reste = ligne.strip_prefix("\"visionneuse/")?;
                let (verbe, fin) = reste.split_once('"')?;
                fin.trim_start().starts_with("=>").then(|| verbe.to_string())
            })
            .collect()
    }

    fn trie(mut v: Vec<String>) -> Vec<String> {
        v.sort();
        v.dedup();
        v
    }

    #[test]
    fn la_page_et_le_dispatch_acceptent_les_memes_verbes() {
        let page = trie(verbes_de_la_page());
        let rust = trie(verbes_du_dispatch());
        // ⚠️ **LES DEUX PLANCHERS, le garde-fou du garde.** Deux listes vides se
        // ressemblent : si le scan de la page ne voyait plus `go`, ou si la découpe
        // ne trouvait plus la fonction, la parité passerait EN SILENCE.
        assert!(page.len() >= 6, "scan de la page muet : {page:?}");
        assert!(rust.len() >= 6, "scan du dispatch muet : {rust:?}");
        assert_eq!(page, rust, "la page et le dispatch n'ont pas les memes verbes");
    }
}
