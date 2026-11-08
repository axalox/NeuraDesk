//! Le PANNEAU de l'applet Scanner — voir `win32::scanner`, qui porte le modèle
//! (les réglages, la ligne de commande de NAPS2, le lancement) et
//! `assets/popups/scanner.html`.
//!
//! Un PANNEAU, pas un menu (voir la mémoire « panneau ou menu ») : même famille
//! que le calendrier et la surveillance, il remplit la zone libre du dock.
//!
//! **DEUX ONGLETS** (voir `assets/popups/scanner.html`) : « Réglages » porte ce
//! qui existait déjà, et « Scan » porte les commandes et **la liste des pages**.
//! ⚠️ **Le changement d'onglet ne passe PAS par ici** : c'est un affichage, la
//! page le fait seule — un verbe de plus serait un verbe à tenir des deux côtés
//! pour rien.
//!
//! Actions reçues (`"scanner/<verbe>/…"`, segments ENCODÉS) :
//! - `ready` — la page a chargé (voir `PAYLOAD`)
//! - `close`
//! - un verbe par réglage : `profil/<nom>`, `source/<nom>`, `dpi/<n>`,
//!   `couleur/<nom>`, `format/<nom>`, `extension/<nom>`,
//!   `deskew/<0|1>`, `ocr/<0|1>`
//! - `destination` — ouvre le sélecteur de dossier natif
//! - `destination-defaut` — revient au dossier proposé par le système
//! - `page` — numérise UNE page et l'ajoute à la session
//! - `retirer/<rang>` — retire la page de ce rang (1-based)
//! - `terminer` — assemble les pages en un document et vide la session
//! - `enregistrer-sous` — le même document, mais nommé et placé par l'utilisateur
//! - `abandonner` — jette les pages temporaires
//! - `annuler` — interrompt la numérisation en cours
//! - `ouvrir-dossier` — ouvre le dossier des scans dans l'explorateur
//!
//! ⚠️ **LE PANNEAU N'EST PAS LE SEUL À POUVOIR LANCER UN SCAN, MAIS C'EST LE
//! SEUL À LE MONTRER** : `refresh_if_visible()` est appelé à chaque frame par
//! `app::run`, même panneau fermé — sans quoi un scan terminé pendant que le
//! panneau est fermé laisserait un processus non relevé, et **sa page ne
//! rejoindrait jamais la session**.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::scanner::{self, Couleur, Extension, Reglages, Scan, Session, Source, Verdict};

/// **CE QU'ON FERA DU SCAN QUI TOURNE.**
///
/// ⚠️ **LE SCAN ET SON RÔLE SONT INSÉPARABLES** : deux `thread_local` les
/// désynchroniseraient à la première sortie qui n'en remet qu'un — et une page
/// numérisée n'arriverait jamais dans la liste, sans que rien ne le dise.
enum Travail {
    /// Une page : son fichier rejoint la session si le scan réussit, et il est
    /// **effacé** sinon (une page à moitié écrite n'a rien à faire dans la liste).
    Page { sortie: PathBuf },
    /// L'assemblage du document : la session se vide si le fichier est écrit, et
    /// **le document à moitié écrit est effacé** sinon.
    Assemblage { sortie: PathBuf },
}

/// **LE CÔTÉ DE LA PLUS GRANDE VIGNETTE DE PAGE**, en pixels.
///
/// ⚠️ **PLUS PETIT QUE LES 440 DU PRESSE-PAPIERS** (`CLIPBOARD_THUMB_MAX_SIDE`) :
/// là-bas la vignette est LA vue, ici c'est une pastille de 64 px à côté d'un
/// libellé. Une vignette trop grande coûte à décoder, à encoder et à pousser,
/// pour des pixels que personne ne voit.
const VIGNETTE_MAX_SIDE: u32 = 220;

/// **LE BUDGET DE POUSSÉE DES VIGNETTES**, copié de `cef_sous_dock`
/// (`ICONES_PAR_FRAME` / `ICONES_MS_PAR_FRAME`) et pour la même raison : N
/// vignettes envoyées d'un coup feraient payer l'ouverture du panneau à la frame
/// qui l'ouvre. Une vignette au minimum par frame, quoi qu'il arrive.
const VIGNETTES_PAR_FRAME: usize = 4;
const VIGNETTES_MS_PAR_FRAME: f64 = 8.0;

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// La clé du dernier `window.setState(...)` émis — vidée au `ready`, même
    /// raison que `cef_surveillance::PAYLOAD`.
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// **LES RÉGLAGES DU PANNEAU**, relus du disque à la PREMIÈRE ouverture et
    /// réécrits à chaque changement.
    static REGLAGES: RefCell<Option<Reglages>> = const { RefCell::new(None) };
    /// **LA SESSION DE PAGES** — créée à la première demande, et c'est là qu'on
    /// purge les résidus d'un lancement précédent (voir `scanner::purger_residus`).
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    /// Le scan en cours **et ce qu'on en fera** — `try_wait` à chaque frame (voir
    /// `win32::scanner`, doc de module).
    static EN_COURS: RefCell<Option<(Scan, Travail)>> = const { RefCell::new(None) };
    /// Le verdict du dernier scan terminé.
    static DERNIER: RefCell<Option<Verdict>> = const { RefCell::new(None) };
    /// **LES PROFILS NAPS2, LUS UNE FOIS.** Les relire à chaque frame ferait une
    /// lecture de fichier soixante fois par seconde pour un contenu qui ne change
    /// que quand l'utilisateur ouvre NAPS2.
    static PROFILS: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
    /// **LA VIGNETTE DE CHAQUE PAGE, PAR NOM DE FICHIER.** `None` veut dire
    /// « celle-là est illisible, on l'a DIT » — on ne la recalcule pas soixante
    /// fois par seconde pour la redire.
    ///
    /// ⚠️ **LA CLÉ EST LE NOM DU FICHIER, JAMAIS LE RANG.** `nommes` n'est jamais
    /// remis à zéro par un retrait : après `retirer(2)` puis un scan, la nouvelle
    /// page occupe le **rang 2** mais s'appelle `page-4.jpg`. Indexer par rang
    /// afficherait la vignette **de la page retirée** sous la nouvelle.
    static VIGNETTES: RefCell<HashMap<PathBuf, Option<String>>> = RefCell::new(HashMap::new());
    /// **LES PAGES DONT LA VIGNETTE N'A PAS ENCORE ÉTÉ POUSSÉE.** Y entrer deux
    /// fois ferait renvoyer deux fois la même image.
    static VIGNETTES_EN_FILE: RefCell<VecDeque<PathBuf>> = RefCell::new(VecDeque::new());
    /// **LE PANNEAU MONTRE-T-IL LA PAGE EN GRAND ?**
    ///
    /// ⚠️ **UN DRAPEAU À LUI, PAS CELUI DU MODÈLE** : la page courante est
    /// partagée avec la fenêtre (`cef_page_scan::PAGE`), mais « je la regarde
    /// dans le panneau » ne regarde que le panneau. Les confondre ferait qu'un
    /// « Retour » ici viderait la fenêtre là-bas.
    static VUE_PAGE: Cell<bool> = const { Cell::new(false) };
    /// **LA PAGE DONT LE PANNEAU A DÉJÀ POUSSÉ L'IMAGE** — même rôle que
    /// `cef_page_scan::ENVOYEE`, mais **par hôte** : chacun a son navigateur, donc
    /// chacun doit pousser pour son compte.
    static IMAGE_ENVOYEE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefScannerWindow"),
    window_title: w!("NeuraDesk — Scanner"),
    html_file: "scanner.html",
    wndproc: Some(wndproc),
    // PANNEAU — même famille que le calendrier et la surveillance.
    above_a_magnified_dock: false,
    fades: false,
    fenetre: false,
};

/// **LE POINT D'ENTRÉE « UNE FOIS PAR LANCEMENT »** — `app::run` l'appelle au
/// démarrage, avant que quiconque ouvre le panneau.
///
/// ⭐ **C'EST ICI QUE SE PURGENT LES PAGES D'UN LANCEMENT PRÉCÉDENT** (voir
/// `scanner::purger_residus`) : une session interrompue par un arrêt brutal
/// laisserait dans `%TEMP%` les pages d'un document jamais enregistré.
///
/// ⚠️ **PAS DANS `avec_session`** : les tests lisent l'état du panneau, donc ils
/// créeraient une session — et un `cargo test` lancerait la purge sur le dossier
/// de l'application ouverte. Ici, seule une vraie instance de l'application purge.
pub fn preload(gfx: &GfxDevice) {
    scanner::purger_residus();
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

fn masquer_les_voisins() {
    // Même « zone libre » que le Dash — la liste de `cef_surveillance`, plus la
    // surveillance elle-même (elle est arrivée après).
    crate::win32::cef_dash::hide_if_visible();
    crate::win32::cef_settings::hide_if_visible();
    crate::win32::cef_notes::hide_if_visible();
    crate::win32::cef_image_editor::hide_if_visible();
    crate::win32::cef_calendar_applet::hide_if_visible();
    crate::win32::cef_clipboard::hide_if_visible();
    crate::win32::cef_minuteur::hide_if_visible();
    crate::win32::cef_calculatrice::hide_if_visible();
    crate::win32::cef_recherche::hide_if_visible();
    crate::win32::cef_automatisations::hide_if_visible();
    crate::win32::cef_meteo::hide_if_visible();
    crate::win32::cef_shaders::hide_if_visible();
    crate::win32::cef_lueurs::hide_if_visible();
    crate::win32::cef_textures::hide_if_visible();
    crate::win32::cef_moniteur::hide_if_visible();
    crate::win32::cef_aide::hide_if_visible();
    crate::win32::bandeau::hide_if_visible();
    crate::win32::cef_ssh::hide_if_visible();
    crate::win32::cef_terminal::hide_if_visible();
    crate::win32::cef_surveillance::hide_if_visible();
}

pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("scanner")));
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    // ⚠️ **ON REMET AUSSI LES VIGNETTES EN FILE.** Une page CEF déjà chargée ne
    // rejoue pas son `ready` quand le panneau se rouvre : sans cette remise, les
    // vignettes de la session ne seraient plus jamais envoyées.
    remettre_les_vignettes_en_file();
    // ⚠️ **ET L'IMAGE DE LA PAGE AUSSI** : si le panneau se rouvre sur une page
    // déjà affichée, sa vignette serait repoussée mais pas la page elle-même.
    IMAGE_ENVOYEE.with(|e| *e.borrow_mut() = None);
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, masquer_les_voisins);
    push_state();
}

/// **LES RÉGLAGES COURANTS** — relus du disque à la première demande, puis tenus
/// en mémoire. `neufs()` porte le dossier que le système propose, donc un premier
/// scan part sans rien régler.
fn reglages() -> Reglages {
    REGLAGES.with(|r| {
        let mut cache = r.borrow_mut();
        cache
            .get_or_insert_with(|| {
                let defaut = Reglages::neufs();
                crate::config::load_reglages_scanner()
                    .map(|texte| Reglages::depuis_texte(&texte, &defaut))
                    .unwrap_or(defaut)
            })
            .clone()
    })
}

/// Applique `modifie` aux réglages, **les écrit sur le disque**, et repousse
/// l'état.
///
/// ⭐ **L'ÉCHEC D'ÉCRITURE SE DIT, ICI ET PAS CHEZ L'APPELANT.** Ce commentaire a
/// promis « et l'appelant le DIT » alors qu'**aucun des neuf appelants ne lit le
/// retour** (`modifier(...);` sec) : un disque plein laissait donc le panneau
/// repousser un état comme si le réglage était retenu, c'est-à-dire exactement le
/// motif que ce dépôt refuse. Le `bool` reste rendu pour qui voudra s'en servir,
/// mais la phrase à l'écran ne dépend plus de la discipline des appelants.
fn modifier(modifie: impl FnOnce(&mut Reglages)) -> bool {
    let nouveaux = REGLAGES.with(|r| {
        let mut cache = r.borrow_mut();
        let reglages = cache.get_or_insert_with(Reglages::neufs);
        modifie(reglages);
        reglages.clone()
    });
    let ecrit = crate::config::save_reglages_scanner(&nouveaux.vers_texte());
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    if !ecrit {
        dire("Le réglage n'a pas pu être enregistré — il ne vaudra que pour cette session.".to_string());
    }
    ecrit
}

/// Les profils NAPS2, lus une fois — voir `PROFILS`.
fn profils() -> Vec<String> {
    PROFILS.with(|p| p.borrow_mut().get_or_insert_with(scanner::profils_naps2).clone())
}

/// **LA SESSION DE PAGES** — créée à la première demande.
///
/// ⚠️ **ELLE NE PURGE RIEN ELLE-MÊME** : la purge des résidus est au `preload`
/// (voir là-bas). La mettre ici ferait que **le moindre `cargo test` effacerait
/// les pages de l'application ouverte** — les tests lisent l'état, donc ils
/// créeraient une session, donc ils effaceraient un dossier qui n'est pas le
/// leur. Un test ne doit pas pouvoir détruire les données d'un autre processus.
fn avec_session<R>(travail: impl FnOnce(&mut Session) -> R) -> R {
    SESSION.with(|s| {
        let mut cache = s.borrow_mut();
        travail(cache.get_or_insert_with(Session::default))
    })
}

/// **LE FICHIER D'UNE PAGE A CHANGÉ SOUS NOUS** — une ROTATION, aujourd'hui.
///
/// ⚠️ **RIEN NE LE RAFRAÎCHIRAIT TOUT SEUL** : la clé de cache du panneau ne
/// regarde que la SESSION (inchangée), et celle de la fenêtre que la page
/// courante (inchangée aussi). Sans cet appel, tourner une page laisserait la
/// vignette et la vue montrant l'ANCIENNE orientation — le défaut « le bouton ne
/// fait rien », à un pixel près.
pub(crate) fn rafraichir_la_vignette(page: &Path) {
    oublier_la_vignette(page);
    mettre_la_vignette_en_file(page);
    IMAGE_ENVOYEE.with(|e| *e.borrow_mut() = None);
    invalider_payload();
}

/// **PRÉVENIR LE PANNEAU QU'IL Y A DU NEUF** — appelée par `cef_page_scan` quand
/// le modèle partagé bouge (page changée, OCR rendu, message posé).
///
/// ⚠️ **SANS ELLE, LE PANNEAU AFFICHERAIT UN ÉTAT PÉRIMÉ** : sa clé de cache est
/// à lui, et rien ne la viderait — le défaut « il faut rouvrir le panneau pour
/// voir le résultat ».
pub(crate) fn invalider_payload() {
    PAYLOAD.with(|c| *c.borrow_mut() = None);
}

/// **LES PAGES DE LA SESSION** — pour la fenêtre de page, qui navigue dedans.
///
/// ⚠️ **ELLE LES LIT, ELLE NE LES GARDE PAS** : une copie vieillirait, et la
/// fenêtre continuerait de proposer une page qu'on vient de retirer dans le
/// panneau.
pub(crate) fn pages_de_la_session() -> Vec<PathBuf> {
    avec_session(|session| session.pages().to_vec())
}

/// **LA PAGE D'UN RANG (1-based)** — **PURE**, donc testable sans ouvrir quoi que
/// ce soit : c'est la seule partie de l'ouverture qui décide quelque chose.
///
/// ⚠️ **UN RANG HORS BORNES REND `None`** : il vient de la page, donc la liste a
/// pu changer entre l'affichage et le clic.
fn page_du_rang(pages: &[PathBuf], rang: usize) -> Option<PathBuf> {
    if rang == 0 {
        return None;
    }
    pages.get(rang - 1).cloned()
}

/// **MONTRE UNE PAGE EN GRAND — DANS LE PANNEAU.** C'est le geste par défaut du
/// clic sur une vignette.
///
/// ⭐ **DANS LE PANNEAU, ET C'EST LE CORRECTIF D'UN VRAI DÉFAUT** : ouvrir la
/// fenêtre faisait perdre le focus au panneau, qui se referme
/// (`hide_if_focus_elsewhere`) — il fallait donc rouvrir l'applet pour
/// rescanner. Ici on reste dans la zone libre du dock, et « Retour » ramène à la
/// liste sans rien rouvrir.
fn voir_la_page(rang: &str) {
    let Some(page) = page_demandee(rang) else { return };
    VUE_PAGE.with(|v| v.set(true));
    crate::win32::cef_page_scan::voir(page);
    crate::diag!("[scanner] page {rang} affichee dans le panneau");
}

/// **DÉTACHE LA PAGE DANS SA FENÊTRE** — pour la garder visible pendant qu'on
/// travaille ailleurs, ce qu'un panneau ne peut pas faire puisqu'il se referme.
fn detacher_la_page(gfx: &GfxDevice, rang: Option<&str>) {
    let page = match rang {
        Some(rang) => page_demandee(rang),
        // Pas de rang : on détache CELLE QU'ON REGARDE.
        None => crate::win32::cef_page_scan::page_courante(),
    };
    let Some(page) = page else {
        return crate::diag!("[scanner] aucune page a detacher");
    };
    crate::win32::cef_page_scan::montrer(gfx, CONTENT_SCALE.with(Cell::get), page);
}

/// Le chemin d'une page par son rang affiché — avec le refus JOURNALISÉ, commun
/// aux deux façons de la montrer.
fn page_demandee(rang: &str) -> Option<PathBuf> {
    let Ok(rang) = rang.parse::<usize>() else {
        crate::diag!("[scanner] rang illisible : {rang:?}");
        return None;
    };
    match page_du_rang(&pages_de_la_session(), rang) {
        Some(page) => Some(page),
        None => {
            crate::diag!("[scanner] rang hors de la session : {rang}");
            None
        }
    }
}

/// **PRÉPARE UNE SESSION POUR LE HARNAIS** (`NEURADESK_DEBUG_PAGE=<image>`) —
/// elle peuple la liste d'une page, et `NEURADESK_DEBUG_MODE=page` bascule en
/// plus sur la vue en grand.
///
/// ⚠️ **SANS ELLE, LE MODE PAGE DU PANNEAU N'EST PHOTOGRAPHIABLE PAR PERSONNE** :
/// il faut une session, donc une vraie numérisation, et le harnais n'a qu'un clic
/// AVEUGLE. Même levier que la fenêtre (`NEURADESK_DEBUG_PAGE`), appliqué à
/// l'autre hôte — sans quoi on ne vérifierait que la moitié de la fonctionnalité.
pub fn preparer_en_debug() {
    let Some(chemin) = std::env::var_os("NEURADESK_DEBUG_PAGE") else { return };
    let chemin = PathBuf::from(chemin);
    if !chemin.is_file() {
        return crate::diag!("[scanner] NEURADESK_DEBUG_PAGE : {} est absent", chemin.display());
    }
    // **UNE PAGE DANS LA SESSION**, pour que la LISTE ait une ligne cliquable.
    // ⚠️ Sans elle, ni la vignette ni le clic sur la vignette ne sont
    // observables : les deux demandent une session, donc une vraie numérisation.
    if avec_session(|session| session.pages().is_empty()) {
        avec_session(|session| session.ajouter(chemin.clone()));
        mettre_la_vignette_en_file(&chemin);
    }
    // **ET LE MODE PAGE, SI ON LE DEMANDE** — `NEURADESK_DEBUG_MODE=page` bascule
    // directement sur la page, au lieu de laisser la liste.
    if std::env::var_os("NEURADESK_DEBUG_MODE").is_some() {
        VUE_PAGE.with(|v| v.set(true));
        crate::win32::cef_page_scan::voir(chemin.clone());
    }
    // **ET LE RÉGLAGE D'AMÉLIORATION, SI ON LE DEMANDE**
    // (`NEURADESK_DEBUG_AMELIORATION=<lum>,<con>,<sat>`) — sans ce levier, la barre
    // d'outils se photographie avec ses trois curseurs à zéro et la page intacte :
    // on prouverait que les commandes existent, jamais qu'elles éclaircissent
    // quelque chose. Le levier est lu par la page elle-même, donc les DEUX hôtes
    // (panneau et fenêtre détachée) le portent pareillement.
    crate::win32::cef_page_scan::preset_depuis_levier(&chemin);
}

/// **REVIENT À LA LISTE** — sans toucher à la page courante : la fenêtre, si elle
/// est ouverte, continue de la montrer.
fn revenir_a_la_liste() {
    VUE_PAGE.with(|v| v.set(false));
    PAYLOAD.with(|c| *c.borrow_mut() = None);
}

/// **POUSSE L'IMAGE DE LA PAGE AU PANNEAU**, si le mode « page » est ouvert et que
/// ce n'est pas déjà fait.
///
/// ⛔ **ELLE N'EST PAS DANS L'ÉTAT** : une `data:` URI de page pèse des
/// mégaoctets, et l'état est sérialisé **à chaque frame** pour calculer sa clé de
/// cache. C'est la même raison que pour la fenêtre.
fn pousser_l_image_de_la_page() {
    if !VUE_PAGE.with(Cell::get) {
        return;
    }
    let Some(page) = crate::win32::cef_page_scan::page_courante() else { return };
    if IMAGE_ENVOYEE.with(|e| e.borrow().as_deref() == Some(page.as_path())) {
        return;
    }
    // ⚠️ **LE TÉMOIN EST POSÉ MÊME SI LA LECTURE ÉCHOUE** : sans ça, une page
    // disparue serait relue — et son échec redit — soixante fois par seconde.
    IMAGE_ENVOYEE.with(|e| *e.borrow_mut() = Some(page.clone()));
    let uri = crate::win32::cef_page_scan::image_data_uri(&page);
    cef_popup::execute_js(&SLOT, &format!("window.setImage({})", json!(uri)));
}

/// **DIRE POURQUOI RIEN NE SE PASSE.** Un bouton qui refuse en silence est le
/// défaut que ce dépôt refuse : tout refus passe par la ligne d'état du panneau.
fn dire(message: String) {
    crate::diag!("[scanner] {message}");
    DERNIER.with(|d| *d.borrow_mut() = Some(Verdict { ok: false, message, fichier: None }));
    PAYLOAD.with(|c| *c.borrow_mut() = None);
}

/// **LE DERNIER SEGMENT DU CHEMIN** — c'est lui qui identifie une page auprès de
/// la page CEF, et donc la clé de `VIGNETTES`.
fn nom_de_fichier(page: &Path) -> String {
    page.file_name().map(|nom| nom.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Une session de pages, sous la forme que la page dessine : **le rang affiché**
/// et **le nom du fichier** (voir `VIGNETTES` pour pourquoi pas le rang seul).
fn etat_session() -> Vec<(usize, String)> {
    avec_session(|session| {
        session.pages().iter().enumerate().map(|(rang, page)| (rang + 1, nom_de_fichier(page))).collect()
    })
}

// ---------------------------------------------------------------------------
// LES VIGNETTES — elles passent À CÔTÉ de l'état, jamais dedans
// ---------------------------------------------------------------------------

/// **LA VIGNETTE D'UNE PAGE, PRÊTE À ENVOYER** — lue, réduite, encodée.
///
/// ⛔ **AUCUN `unwrap`, ET C'EST UNE OBLIGATION, PAS UN STYLE** : le binaire est
/// compilé avec `panic = "abort"`, donc une image tronquée, d'un format inconnu ou
/// d'une taille absurde tuerait le dock ENTIER. `image::open` rend un `Result` et
/// `make_thumbnail` un triplet vide : les deux sont traités.
///
/// ⚠️ **ET L'ÉCHEC SE DIT.** `clipboard::make_thumbnail` rend `(0, 0, vec![])`
/// sans un mot ; une vignette vide sans explication est exactement le défaut que
/// ce dépôt refuse. Le `None` qui sort d'ici **est envoyé à la page**, qui le
/// montre — il n'est pas avalé.
fn vignette_data_uri(page: &Path) -> Option<String> {
    // ⚠️ **PAS `clipboard::decode_png`** : il n'accepte QUE du PNG, et une page de
    // session est un JPEG.
    let image = match image::open(page) {
        Ok(image) => image.to_rgba8(),
        Err(erreur) => {
            crate::diag!("[scanner] vignette illisible pour {} : {erreur}", page.display());
            return None;
        }
    };
    let (largeur, hauteur) = (image.width(), image.height());
    let (l, h, rgba) = crate::win32::clipboard::make_thumbnail(largeur, hauteur, image.as_raw(), VIGNETTE_MAX_SIDE);
    if l == 0 || h == 0 {
        crate::diag!("[scanner] vignette impossible pour {} ({largeur}x{hauteur})", page.display());
        return None;
    }
    crate::win32::cef_dash::encode_rgba_to_data_uri(l, h, &rgba)
}

/// **MET UNE PAGE EN FILE** — une seule fois : une page déjà servie, ou déjà sue
/// illisible, n'y retourne pas.
fn mettre_la_vignette_en_file(page: &Path) {
    VIGNETTES_EN_FILE.with(|file| {
        let mut file = file.borrow_mut();
        if !file.iter().any(|connu| connu == page) {
            file.push_back(page.to_path_buf());
        }
    });
}

/// **OUBLIE UNE PAGE** — sa vignette et sa place dans la file. Appelée quand on
/// retire une page : sans ça le cache garderait l'image d'un fichier effacé.
fn oublier_la_vignette(page: &Path) {
    VIGNETTES.with(|v| {
        v.borrow_mut().remove(page);
    });
    VIGNETTES_EN_FILE.with(|file| file.borrow_mut().retain(|connu| connu != page));
}

/// **REMET TOUTES LES VIGNETTES EN FILE** — appelée au `ready` de la page.
///
/// ⛔ **INDISPENSABLE, PARCE QUE `execute_js` NE SIGNALE JAMAIS SON ÉCHEC** : une
/// vignette poussée avant que la page n'écoute est perdue sans un mot, et ses
/// lignes resteraient grises POUR TOUJOURS. Le `ready` est le seul instant où
/// l'on sait que la page est là.
fn remettre_les_vignettes_en_file() {
    let pages = avec_session(|session| session.pages().to_vec());
    VIGNETTES_EN_FILE.with(|file| {
        let mut file = file.borrow_mut();
        file.clear();
        file.extend(pages);
    });
}

/// **POUSSE QUELQUES VIGNETTES, DANS LE BUDGET D'UNE FRAME.** Même discipline que
/// `cef_sous_dock::envoyer_des_icones` : on sort **un** chemin de la file, on
/// calcule **hors de tout emprunt**, et **le budget se teste APRÈS le calcul** —
/// sinon une frame chargée pourrait n'en servir aucune, et la file ne se viderait
/// jamais.
fn envoyer_des_vignettes() {
    let debut = Instant::now();
    let mut lot: Vec<(String, Option<String>)> = Vec::new();
    loop {
        let Some(page) = VIGNETTES_EN_FILE.with(|file| file.borrow_mut().pop_front()) else { break };
        // ⚠️ **LE CALCUL EST HORS EMPRUNT.** Il lit un fichier et décode une
        // image : le tenir pendant un `borrow` ferait un `BorrowMutError` sur la
        // première tentative de rangement — et dans ce dépôt, un `RefCell` déjà
        // emprunté échoue EN SILENCE.
        let connue = VIGNETTES.with(|v| v.borrow().get(&page).cloned());
        let uri = connue.unwrap_or_else(|| vignette_data_uri(&page));
        VIGNETTES.with(|v| {
            v.borrow_mut().insert(page.clone(), uri.clone());
        });
        lot.push((nom_de_fichier(&page), uri));
        if lot.len() >= VIGNETTES_PAR_FRAME || debut.elapsed().as_secs_f64() * 1000.0 >= VIGNETTES_MS_PAR_FRAME {
            break;
        }
    }
    if lot.is_empty() {
        return;
    }
    // ⛔ **UN NOM, UNE VALEUR — ET `null` EST UNE VALEUR.** Une page illisible
    // reçoit `null` : la page cesse de l'attendre et le DIT, au lieu d'afficher
    // une case grise sans explication.
    let appels = lot
        .iter()
        .map(|(nom, uri)| format!("window.setPageVignette({}, {})", json!(nom), json!(uri)))
        .collect::<Vec<_>>()
        .join(";");
    cef_popup::execute_js(&SLOT, &appels);
}

fn etat_json() -> serde_json::Value {
    let reglages = reglages();
    let dernier = DERNIER.with(|d| d.borrow().clone());
    let pages = etat_session();
    // **NAPS2 EST-IL LÀ ?** Sans cette information, une machine sans NAPS2
    // montrerait un panneau parfaitement normal dont le bouton échouerait — le
    // défaut « ça ne dit rien » que le dépôt refuse.
    let naps2 = scanner::chemin_console().is_some();
    json!({
        "contentScale": CONTENT_SCALE.with(Cell::get),
        "profils": profils(),
        "profil": reglages.profil,
        "source": reglages.source.nom(),
        "sources": Source::TOUTES.iter().map(|(v, nom, libelle)| json!([nom, libelle, reglages.source == *v])).collect::<Vec<_>>(),
        "dpi": reglages.dpi,
        "resolutions": scanner::RESOLUTIONS,
        "couleur": reglages.couleur.nom(),
        "couleurs": Couleur::TOUTES.iter().map(|(v, nom, libelle)| json!([nom, libelle, reglages.couleur == *v])).collect::<Vec<_>>(),
        "format": reglages.format,
        "formats": scanner::FORMATS,
        "extension": reglages.extension.nom(),
        "extensions": Extension::TOUTES.iter().map(|(v, nom, libelle)| json!([nom, libelle, reglages.extension == *v])).collect::<Vec<_>>(),
        "deskew": reglages.deskew,
        "ocr": reglages.ocr,
        // ⚠️ **`pages` PORTE LE RANG **ET** LE NOM DU FICHIER.** Le nom est la clé
        // des vignettes (voir `VIGNETTES`) ; le rang est ce que l'utilisateur lit
        // et ce que la page renvoie pour « retirer » et « ouvrir ». Les deux sont
        // nécessaires, et les confondre afficherait la vignette d'une autre page.
        "pages": pages.iter().map(|(rang, nom)| json!({"rang": rang, "nom": nom})).collect::<Vec<_>>(),
        // ⭐ **LA VUE DE PAGE, QUAND ELLE EST OUVERTE** — `null` veut dire « on est
        // sur la liste ». C'est **exactement** l'état que pousse la fenêtre
        // (`cef_page_scan::etat_page`) : les deux hôtes ne peuvent donc pas
        // montrer deux choses différentes de la même page.
        "page": if VUE_PAGE.with(Cell::get) { crate::win32::cef_page_scan::etat_page() } else { serde_json::Value::Null },
        "destination": reglages.destination.display().to_string(),
        "destinationDefaut": scanner::dossier_par_defaut().display().to_string(),
        // **NAPS2 EST-IL LÀ ?** Sans cette information, une machine sans NAPS2
        // montrait un panneau parfaitement normal dont le bouton échouait — le
        // défaut « ça ne dit rien » que le dépôt refuse.
        "naps2": naps2,
        "enCours": EN_COURS.with(|e| e.borrow().is_some()),
        "dernier": dernier.map(|v| json!({
            "ok": v.ok,
            "message": v.message,
            "fichier": v.fichier.map(|f| f.display().to_string()),
        })),
    })
}

fn push_state() {
    let payload = etat_json();
    let cle = payload.to_string();
    if PAYLOAD.with(|c| c.borrow().as_deref() == Some(cle.as_str())) {
        return;
    }
    PAYLOAD.with(|c| *c.borrow_mut() = Some(cle));
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
}

/// **RELEVER LE SCAN, PUIS REPOUSSER L'ÉTAT.** Appelé à chaque frame par
/// `app::run`, **même panneau fermé** : un scan terminé pendant que le panneau
/// est fermé doit être relevé quand même, sinon le processus reste non attendu et
/// son verdict est perdu.
pub fn refresh_if_visible() {
    // ⚠️ **ON SORT LE TRAVAIL DU CACHE AVANT DE RELEVER.** Le traitement d'un
    // verdict touche `SESSION` et `PAYLOAD` : garder l'emprunt de `EN_COURS`
    // ouvert pendant ce temps ferait un `BorrowMutError` si l'un d'eux y
    // retouchait — et un `RefCell` déjà emprunté échoue EN SILENCE dans ce dépôt
    // (voir la mémoire « RefCell reentrancy »).
    if let Some((mut scan, travail)) = EN_COURS.with(|e| e.borrow_mut().take()) {
        match scan.relever() {
            Some(verdict) => {
                crate::diag!("[scanner] {}", verdict.message);
                let verdict = appliquer(travail, verdict);
                DERNIER.with(|d| *d.borrow_mut() = Some(verdict));
                PAYLOAD.with(|c| *c.borrow_mut() = None);
            }
            // Toujours en train de tourner : on le remet tel quel.
            None => EN_COURS.with(|e| *e.borrow_mut() = Some((scan, travail))),
        }
    }
    // ⛔ **ON NE POUSSE QUE SI LE PANNEAU EST VISIBLE.** `execute_js` sur un
    // navigateur qui n'existe pas ne dit rien et ne fait rien : servir la file
    // panneau fermé la viderait **pour de bon**, et les vignettes seraient
    // perdues sans un mot. La file attend, elle ne se perd pas — et `toggle`
    // comme `ready` la reconstituent de toute façon.
    if cef_popup::is_visible(&SLOT) {
        // ⚠️ **L'IMAGE AVANT L'ÉTAT** : la page doit être là quand la vue
        // s'ouvre, sinon on verrait une case vide le temps d'une frame.
        pousser_l_image_de_la_page();
        envoyer_des_vignettes();
        push_state();
    }
}

/// **CE QUE LE VERDICT DÉCLENCHE.** Une page qui a réussi entre dans la session ;
/// un assemblage qui a réussi vide la session **et efface ses temporaires**.
///
/// ⚠️ **ON N'EFFACE QUE SUR UN SUCCÈS** : rater l'assemblage ne doit pas coûter
/// à l'utilisateur les pages qu'il vient de numériser une par une.
fn appliquer(travail: Travail, verdict: Verdict) -> Verdict {
    match travail {
        Travail::Page { sortie } => {
            if verdict.ok {
                let rang = avec_session(|session| {
                    session.ajouter(sortie.clone());
                    session.pages().len()
                });
                // ⭐ **LA VIGNETTE PART D'ICI, EN FILE** — et pas dans l'état : une
                // `data:` URI pèse des dizaines de Ko, et l'état est re-sérialisé
                // et comparé à CHAQUE frame.
                mettre_la_vignette_en_file(&sortie);
                // ⭐ **LA LIGNE D'ÉTAT DIT OÙ EN EST LA SESSION**, pas le chemin
                // d'un fichier temporaire dont l'utilisateur n'a rien à faire.
                //
                // ⛔ **ET `fichier: None`, SINON « OUVRIR LE DOSSIER » MENT** : le
                // verdict porte le chemin TEMPORAIRE, et le bouton ouvre le
                // dossier de DESTINATION — où la page n'est pas encore. Le
                // document n'existe qu'à « Terminer ».
                Verdict {
                    ok: true,
                    message: format!("Page {rang} scannée — {rang} page(s) dans la session."),
                    fichier: None,
                }
            } else {
                // Une page à moitié écrite n'a rien à faire dans la liste.
                scanner::effacer_page(&sortie);
                verdict
            }
        }
        Travail::Assemblage { sortie } => {
            if verdict.ok {
                for page in avec_session(Session::vider) {
                    scanner::effacer_page(&page);
                }
            } else {
                // ⚠️ **UN DOCUMENT À MOITIÉ ÉCRIT PORTE LE NOM D'UN VRAI.** `-f`
                // est passé à NAPS2, donc un assemblage interrompu peut avoir
                // laissé un PDF tronqué dans le dossier de l'utilisateur — et son
                // verdict a `fichier: None`, donc RIEN ne pourrait le signaler.
                // On l'efface ; **les pages, elles, sont conservées** (c'est la
                // ligne du dessus du `match` qui décide, pas celle-ci).
                scanner::effacer_page(&sortie);
            }
            verdict
        }
    }
}

/// **NUMÉRISE UNE PAGE** et l'ajoute à la session. **Ne bloque pas** :
/// `scanner::lancer_la_page` démarre le sous-processus et rend la main, le relevé
/// se fait à chaque frame (voir `refresh_if_visible`).
fn scanner_une_page() {
    if EN_COURS.with(|e| e.borrow().is_some()) {
        return crate::diag!("[scanner] un scan est deja en cours");
    }
    let reglages = reglages();
    // ⚠️ **LE FORMAT DU RÉGLAGE N'ENTRE PAS ICI** : une page est TOUJOURS une
    // image (voir `scanner::EXTENSION_PAGE`) — c'est ce qui la rend montrable. Le
    // réglage ne décide que du DOCUMENT, et c'est « Terminer » qui le lit.
    let sortie = avec_session(|session| session.chemin_a_venir());
    match scanner::lancer_la_page(&reglages, &sortie) {
        Ok(scan) => {
            EN_COURS.with(|e| *e.borrow_mut() = Some((scan, Travail::Page { sortie })));
            DERNIER.with(|d| *d.borrow_mut() = None);
        }
        // L'échec du LANCEMENT est un verdict comme un autre : la page a une
        // seule ligne d'état, et elle doit pouvoir dire « NAPS2 est introuvable »
        // aussi bien que « le scan a échoué ».
        Err(message) => dire(message),
    }
    PAYLOAD.with(|c| *c.borrow_mut() = None);
}

/// **RETIRE UNE PAGE DE LA SESSION** — et efface son fichier temporaire, sans
/// quoi le dossier grossirait à chaque hésitation.
///
/// ⛔ **REFUSÉ PENDANT UNE NUMÉRISATION OU UN ASSEMBLAGE.** Les deux lisent des
/// fichiers de la session : NAPS2 assemble **en lisant les pages une à une**, donc
/// effacer celle qu'il est en train d'ouvrir ferait échouer l'assemblage — et
/// l'utilisateur perdrait le document qu'il vient de composer page par page. La
/// page grise déjà ses croix ; ce refus est la seconde barrière.
fn retirer_une_page(rang: &str) {
    if EN_COURS.with(|e| e.borrow().is_some()) {
        return crate::diag!("[scanner] retrait refuse : un scan ou un assemblage tourne");
    }
    let Ok(rang) = rang.parse::<usize>() else {
        return crate::diag!("[scanner] rang illisible : {rang:?}");
    };
    match avec_session(|session| session.retirer(rang)) {
        Some(page) => {
            scanner::effacer_page(&page);
            // ⚠️ **ET SA VIGNETTE PART AVEC ELLE** : sans ça le cache garderait
            // l'image d'un fichier effacé, pour un nom que `nommes` ne réutilisera
            // jamais — donc une fuite qui ne se voit pas.
            oublier_la_vignette(&page);
            crate::diag!("[scanner] page {rang} retiree");
        }
        // ⚠️ Un rang hors bornes vient d'un affichage périmé : ce n'est pas une
        // erreur de l'utilisateur, donc on le journalise sans le lui reprocher.
        None => crate::diag!("[scanner] rang hors de la session : {rang}"),
    }
    PAYLOAD.with(|c| *c.borrow_mut() = None);
}

/// **« TERMINER » — LE DOCUMENT NAÎT ICI.**
///
/// ⭐ **LE PDF S'ASSEMBLE, LE JPEG S'ÉCRIT PAGE À PAGE** : `-i` combine des
/// fichiers, mais un JPEG ne porte qu'une image (voir `scanner::copier_les_pages`).
///
/// ⭐ **C'EST ICI, ET NULLE PART AILLEURS, QUE LE FORMAT SE CHOISIT** — le réglage
/// est lu maintenant, pas à la première page. Les pages de la session sont des
/// images de toute façon, donc les deux formats restent possibles jusqu'au bout.
fn terminer() {
    produire_le_document(None);
}

/// **« ENREGISTRER SOUS » — le même document, mais NOMMÉ ET PLACÉ PAR L'UTILISATEUR.**
///
/// ⭐ **C'est « Terminer », avec le choix du chemin** : la production est la même
/// (assemblage en PDF, ou écriture une à une en JPEG), et la seule différence est
/// d'où vient la sortie. Les deux passent donc par `produire_le_document` — deux
/// copies de cette logique divergeraient au premier changement.
fn enregistrer_sous() {
    if EN_COURS.with(|e| e.borrow().is_some()) {
        return crate::diag!("[scanner] enregistrer sous refuse : un scan tourne");
    }
    // ⚠️ **LE PROPRIÉTAIRE DE LA BOÎTE EST LE PANNEAU** : une boîte sans
    // propriétaire peut passer DERRIÈRE, et l'application paraîtrait figée alors
    // qu'elle attend un choix invisible.
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else {
        return crate::diag!("[scanner] boite de dialogue refusee : le panneau n'existe pas encore");
    };
    let extension = reglages().extension;
    let propose = scanner::nom_de_document(extension);
    let Some(chemin) = crate::win32::file_dialog::enregistrer_sous(
        hwnd,
        "Enregistrer le document numérisé",
        &propose,
        extension.nom(),
        match extension {
            Extension::Pdf => "Document PDF (*.pdf)",
            Extension::Jpeg => "Image JPEG (*.jpg)",
        },
    ) else {
        return; // Annuler n'est pas une erreur : rien à journaliser.
    };
    produire_le_document(Some(chemin));
}

/// **LA PRODUCTION DU DOCUMENT — le chemin est la SEULE chose qui change.**
///
/// `cible` : `Some(chemin)` pour « Enregistrer sous », `None` pour « Terminer »,
/// qui compose alors le nom dans le dossier de destination.
fn produire_le_document(cible: Option<PathBuf>) {
    if EN_COURS.with(|e| e.borrow().is_some()) {
        return crate::diag!("[scanner] terminer refuse : un scan tourne");
    }
    let reglages = reglages();
    let pages = avec_session(|session| session.pages().to_vec());
    if pages.is_empty() {
        return dire("Aucune page à enregistrer : numérisez-en au moins une.".to_string());
    }
    let extension = reglages.extension;
    // ⚠️ **LE DOSSIER EST VÉRIFIÉ ICI, PAS SEULEMENT DANS `lancer_avec`** : un
    // dossier vide produirait sinon un chemin relatif, donc un document écrit dans
    // le répertoire de travail du processus — que personne ne connaît.
    // ⚠️ **ELLE NE VAUT QUE POUR « TERMINER »** : « Enregistrer sous » tient son
    // chemin de la boîte de dialogue, et exiger en plus un dossier de destination
    // refuserait un geste parfaitement légitime.
    if cible.is_none() && reglages.destination.as_os_str().is_empty() {
        return dire("Choisissez d'abord un dossier de destination.".to_string());
    }
    match extension {
        Extension::Pdf => {
            let sortie = cible.clone().unwrap_or_else(|| reglages.destination.join(scanner::nom_de_document(extension)));
            // ⭐ **L'OCR SE JOUE ICI** : `--ocrlang` est IGNORÉ par NAPS2 quand la
            // sortie est une image (mesuré), et produit une vraie couche texte
            // quand la sortie est un PDF. La case des Réglages agit donc sur le
            // DOCUMENT, et c'est le seul endroit où elle peut dire vrai.
            match scanner::lancer_l_assemblage(&pages, &sortie, reglages.ocr) {
                Ok(scan) => {
                    // Le chemin voyage avec le travail : c'est ce qui permettra
                    // d'effacer un document tronqué si l'assemblage échoue.
                    EN_COURS.with(|e| *e.borrow_mut() = Some((scan, Travail::Assemblage { sortie })));
                    DERNIER.with(|d| *d.borrow_mut() = None);
                }
                Err(message) => return dire(message),
            }
        }
        Extension::Jpeg => match cible.as_deref().map_or_else(
            || scanner::copier_les_pages(&pages, &reglages.destination, extension),
            |chemin| scanner::copier_les_pages_vers(&pages, chemin, extension),
        ) {
            Ok(ecrits) => {
                let nombre = ecrits.len();
                for page in avec_session(Session::vider) {
                    scanner::effacer_page(&page);
                }
                DERNIER.with(|d| {
                    *d.borrow_mut() = Some(Verdict {
                        ok: true,
                        // ⚠️ **LE DOSSIER RÉELLEMENT ÉCRIT**, pas celui des Réglages :
                        // avec « Enregistrer sous », les deux diffèrent et le
                        // message enverrait l'utilisateur au mauvais endroit.
                        message: format!(
                            "{nombre} page(s) enregistrée(s) dans {}",
                            ecrits.first().and_then(|f| f.parent()).unwrap_or(&reglages.destination).display()
                        ),
                        fichier: ecrits.into_iter().next(),
                    })
                });
            }
            Err(message) => return dire(message),
        },
    }
    PAYLOAD.with(|c| *c.borrow_mut() = None);
}

/// **« ABANDONNER » — jette les pages temporaires.** Le panneau ne montre ce
/// bouton que s'il y a quelque chose à jeter, et la ligne d'état **dit ce qui est
/// parti** : une suppression qui ne se raconte pas est une suppression qu'on
/// découvre trop tard.
fn abandonner() {
    if EN_COURS.with(|e| e.borrow().is_some()) {
        return crate::diag!("[scanner] abandon refuse : un scan tourne");
    }
    let jetees = avec_session(Session::vider);
    if jetees.is_empty() {
        return crate::diag!("[scanner] abandon sans effet : la session est vide");
    }
    let nombre = jetees.len();
    for page in &jetees {
        scanner::effacer_page(page);
    }
    DERNIER.with(|d| {
        *d.borrow_mut() = Some(Verdict {
            ok: true,
            message: format!("Session abandonnée : {nombre} page(s) temporaire(s) supprimée(s)."),
            fichier: None,
        })
    });
    PAYLOAD.with(|c| *c.borrow_mut() = None);
}

/// La fenêtre du panneau, si elle existe — même garde que `cef_settings`, qui
/// porte le pourquoi : un dialogue sans propriétaire se perd derrière le dock.
fn dialogue_possible() -> Option<HWND> {
    let hwnd = cef_popup::hwnd(&SLOT);
    if hwnd.is_none() {
        crate::diag!("[scanner] boite de dialogue refusee : la fenetre n'existe pas encore");
    }
    hwnd
}

fn choisir_destination() {
    let Some(hwnd) = dialogue_possible() else { return };
    // ⚠️ Le dialogue est MODAL et gèle le redessin du dock pendant qu'il est
    // ouvert — assumé, documenté dans `win32::file_dialog`.
    let Some(chemin) = crate::win32::file_dialog::choisir_dossier(hwnd, "Dossier où enregistrer les scans") else {
        return; // Annuler n'est pas une erreur : rien à journaliser.
    };
    modifier(|reglages| reglages.destination = chemin);
}

/// ⚠️ **`gfx` EST ARRIVÉ AVEC LE VERBE `ouvrir`** : ouvrir la page en grand crée
/// une FENÊTRE CEF, et une fenêtre a besoin du device. C'est explicite plutôt
/// qu'un `thread_local` qui donnerait un second propriétaire au device.
pub fn dispatch_actions(gfx: &GfxDevice) {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("scanner/") else {
            crate::diag!("[scanner] action inconnue : {action:?}");
            continue;
        };
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["ready"] => {
                PAYLOAD.with(|c| *c.borrow_mut() = None);
                // ⭐ **LA PAGE VIENT DE (RE)CHARGER, DONC SA TABLE DE VIGNETTES EST
                // VIDE.** `execute_js` ne signale jamais son échec : ce qui a été
                // poussé avant ce `ready` est perdu sans un mot, et sans cette
                // remise les lignes resteraient grises POUR TOUJOURS.
                remettre_les_vignettes_en_file();
                // ⚠️ **PAS de `relever_les_ocr` ICI** : c'est
                // `cef_page_scan::refresh_if_visible` qui relève, **à chaque
                // frame et panneau fermé compris** — le faire aussi ici laisserait
                // croire que c'est le panneau qui pilote la reconnaissance.
                IMAGE_ENVOYEE.with(|e| *e.borrow_mut() = None);
            }
            ["close"] => cef_popup::fermer_par_geste(&SLOT),
            // **UN RÉGLAGE ILLISIBLE EST REFUSÉ EN LE DISANT** (règle du dépôt) :
            // il vient d'une page, donc d'un code qui a divergé — pas d'un geste.
            // ⚠️ **UN SEGMENT FINAL VIDE EST MANGÉ EN ROUTE** : le bouton « Le plus
            // récent » envoie un profil VIDE, donc ses DEUX bras existent — avec
            // et sans le segment. Même piège que `cef_surveillance::enregistrer`.
            ["profil"] => {
                modifier(|r| r.profil.clear());
            }
            ["profil", nom] => {
                modifier(|r| r.profil = nom.to_string());
            }
            ["source", nom] => match Source::depuis_nom(nom) {
                Some(source) => {
                    modifier(|r| r.source = source);
                }
                None => crate::diag!("[scanner] source inconnue : {nom:?}"),
            },
            ["couleur", nom] => match Couleur::depuis_nom(nom) {
                Some(couleur) => {
                    modifier(|r| r.couleur = couleur);
                }
                None => crate::diag!("[scanner] couleur inconnue : {nom:?}"),
            },
            // ⭐ **LE FORMAT SE CHOISIT LIBREMENT, MÊME EN COURS DE SESSION** : les
            // pages sont des IMAGES, donc il ne décide que de ce que « Terminer »
            // produira — et il est lu à ce moment-là. Le refus « figé par la
            // première page » a disparu avec la raison qui le justifiait.
            ["extension", nom] => match Extension::depuis_nom(nom) {
                Some(extension) => {
                    modifier(|r| r.extension = extension);
                }
                None => crate::diag!("[scanner] extension inconnue : {nom:?}"),
            },
            ["dpi", valeur] => match valeur.parse::<u32>() {
                Ok(dpi) if scanner::RESOLUTIONS.contains(&dpi) => {
                    modifier(|r| r.dpi = dpi);
                }
                _ => crate::diag!("[scanner] resolution non proposee : {valeur:?}"),
            },
            ["format", valeur] => {
                if scanner::FORMATS.iter().any(|f| f.eq_ignore_ascii_case(valeur)) {
                    modifier(|r| r.format = valeur.to_string());
                } else {
                    crate::diag!("[scanner] format non propose : {valeur:?}");
                }
            }
            ["deskew", valeur] => {
                modifier(|r| r.deskew = *valeur == "1");
            }
            ["ocr", valeur] => {
                modifier(|r| r.ocr = *valeur == "1");
            }
            ["destination"] => choisir_destination(),
            ["destination-defaut"] => {
                modifier(|r| r.destination = scanner::dossier_par_defaut());
            }
            ["page"] => scanner_une_page(),
            ["retirer", rang] => retirer_une_page(rang),
            // ⭐ **LA PAGE EN GRAND, DANS LE PANNEAU** — le geste par défaut.
            ["voir", rang] => voir_la_page(rang),
            ["retour"] => revenir_a_la_liste(),
            ["fenetre"] => detacher_la_page(gfx, None),
            // ⚠️ **LES MÊMES VERBES QUE LA FENÊTRE, ET LES MÊMES FONCTIONS** : le
            // modèle vit dans `cef_page_scan`, donc les deux hôtes ne peuvent pas
            // diverger sur ce que « reconnaître » veut dire.
            ["precedente"] => crate::win32::cef_page_scan::naviguer(-1),
            ["suivante"] => crate::win32::cef_page_scan::naviguer(1),
            ["reconnaitre", moteur] => crate::win32::cef_page_scan::lancer_ocr(moteur),
            ["tourner", sens] => match crate::win32::cef_page_scan::Sens::depuis_nom(sens) {
                Some(sens) => crate::win32::cef_page_scan::tourner(sens),
                None => crate::diag!("[page-scan] sens de rotation inconnu : {sens:?}"),
            },
            ["comparer"] => {
                for (_moteur, nom, _libelle) in crate::win32::ocr::Moteur::TOUS {
                    crate::win32::cef_page_scan::lancer_ocr(nom);
                }
            }
            ["copier", moteur] => crate::win32::cef_page_scan::copier_le_texte(moteur),
            // ⭐ **L'AMÉLIORATION D'IMAGE : LES MÊMES FONCTIONS QUE LA FENÊTRE.** Le
            // réglage vit dans `cef_page_scan`, donc le panneau et la fenêtre
            // montrent le même filtre de la même page — et « Enregistrer la copie »
            // écrit le même fichier, quel que soit l'hôte qui l'a demandé.
            ["ameliorer-auto"] => crate::win32::cef_page_scan::ameliorer_auto(),
            ["ameliorer-regler", luminosite, contraste, saturation] => {
                crate::win32::cef_page_scan::ameliorer_regler(luminosite, contraste, saturation)
            }
            ["ameliorer-zero"] => crate::win32::cef_page_scan::ameliorer_zero(),
            ["ameliorer-enregistrer"] => crate::win32::cef_page_scan::ameliorer_enregistrer(),
            // ⚠️ **LE PROPRIÉTAIRE DE LA BOÎTE EST LE PANNEAU ICI**, la fenêtre
            // là-bas : une boîte sans propriétaire peut passer derrière.
            ["enregistrer", moteur] => match cef_popup::hwnd(&SLOT) {
                Some(hwnd) => crate::win32::cef_page_scan::enregistrer_le_texte(hwnd, moteur),
                None => crate::diag!("[scanner] boite refusee : le panneau n'existe pas encore"),
            },
            ["terminer"] => terminer(),
            // ⚠️ **ARITÉ 1, DONC AUCUNE COLLISION** avec `["enregistrer", moteur]`
            // (le texte d'OCR) — c'est le garde `aucun_verbe_n_a_deux_bras_de_meme_arite`
            // qui le garantit, et il le vérifie.
            ["enregistrer-sous"] => enregistrer_sous(),
            ["abandonner"] => abandonner(),
            ["annuler"] => {
                // ⚠️ **ON SORT LE TRAVAIL DU CACHE, ET ON LE REMET S'IL VIT
                // ENCORE.** `kill()` ÉCHOUE quand le processus vient de se
                // terminer tout seul : vider `EN_COURS` sans condition jetterait
                // alors le `Scan` sans jamais le relever — **une page numérisée
                // avec succès serait perdue**, et son temporaire resterait
                // orphelin. (Voir la mémoire « une absence de trace n'est pas une
                // preuve » : l'échec de `kill` est ici le SEUL signal.)
                //
                // ⚠️ Et surtout PAS de `return` dans ce bras : il sortirait de
                // `dispatch_actions` et jetterait les actions encore en file.
                let en_cours = EN_COURS.with(|e| e.borrow_mut().take());
                if let Some((mut scan, travail)) = en_cours {
                    if scan.annuler() {
                        // ⛔ **CE QUI ÉTAIT À MOITIÉ ÉCRIT EST EFFACÉ** : une page
                        // tronquée dans la liste serait pire que pas de page, et
                        // un PDF tronqué porterait le nom d'un vrai document.
                        match &travail {
                            Travail::Page { sortie, .. } | Travail::Assemblage { sortie } => {
                                scanner::effacer_page(sortie);
                            }
                        }
                        DERNIER.with(|d| {
                            *d.borrow_mut() = Some(Verdict {
                                ok: false,
                                message: "Scan abandonné.".to_string(),
                                fichier: None,
                            })
                        });
                    } else {
                        // Terminé entre l'affichage et le clic : la frame suivante
                        // relèvera son verdict, et rien ne sera perdu.
                        EN_COURS.with(|e| *e.borrow_mut() = Some((scan, travail)));
                    }
                } else {
                    crate::diag!("[scanner] annuler sans effet : rien ne tourne");
                }
                PAYLOAD.with(|c| *c.borrow_mut() = None);
            }
            ["ouvrir-dossier"] => {
                // ⚠️ **LE DOSSIER PEUT NE PAS EXISTER** (aucun scan encore) :
                // `explorer.exe` sur un chemin absent ouvre « Ce PC », ce qui
                // serait trompeur. On le crée avant, comme le fait `lancer`.
                let dossier = reglages().destination;
                let _ = std::fs::create_dir_all(&dossier);
                if let Err(erreur) = std::process::Command::new("explorer.exe").arg(&dossier).spawn() {
                    crate::diag!("[scanner] explorateur non lance : {erreur}");
                }
            }
            _ => crate::diag!("[scanner] action inconnue : {action:?}"),
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

/// **UN CLIC DIRECT DANS LE PANNEAU, SANS SOURIS** (`NEURADESK_DEBUG_CLICK=x,y`).
///
/// ⛔ **PAS `SendInput`** — un appel de fonction direct (voir
/// `cef_popup::debug_click`), la règle de sûreté du dépôt. C'est le seul moyen de
/// vérifier que **cliquer une vignette ouvre bien la page** : la souris réelle,
/// elle, ne peut pas être commandée.
pub fn debug_click(x: i32, y: i32) {
    cef_popup::debug_click(&SLOT, x, y);
}

/// Voir `win32::cef_calendar_applet::debug_dump_backbuffer` — le harnais a
/// DEUX listes à tenir (`NEURADESK_DEBUG_OPEN=scanner`).
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
    /// `cef_surveillance::tests::verbes_du_dispatch`, même lecture.
    fn verbes_du_dispatch() -> Vec<String> {
        let source = include_str!("cef_scanner.rs");
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
        const SOURCE: &str = include_str!("../../assets/popups/scanner.html");
        SOURCE
            .match_indices("go(")
            .filter_map(|(i, _)| {
                let reste = &SOURCE[i + 3..];
                let guillemet = reste.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                reste[1..].split(guillemet).next().map(str::to_string)
            })
            .collect()
    }

    /// **LES VERBES ET LEUR ARITÉ**, tels que le `match` les distingue.
    fn signatures_du_dispatch() -> Vec<String> {
        let source = include_str!("cef_scanner.rs");
        source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(source)
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//"))
            .filter_map(|l| {
                // ⚠️ **ON LIT LE VERBE COMME LE FAIT `verbes_du_dispatch`** : après
                // le guillemet fermant vient `]` (un seul élément) ou `,` (davantage).
                // ⛔ **UNE PREMIÈRE VERSION CHERCHAIT LA SÉQUENCE `"]` — ELLE
                // N'EXISTE PAS DANS UN BRAS À DEUX ÉLÉMENTS** (`["ocr", valeur]` :
                // après `ocr"` vient `,`). Tous ces bras étaient donc ignorés, et le
                // garde laissait passer exactement la collision qu'il devait
                // attraper — vérifié en le faisant tourner sur l'ancien code, où il
                // passait au vert.
                let reste = l.strip_prefix("[\"")?;
                let (verbe, fin) = reste.split_once('"')?;
                match fin.chars().next()? {
                    ']' => Some(format!("{verbe}/1")),
                    ',' => {
                        let jusqua = fin.find(']')?;
                        Some(format!("{verbe}/{}", fin[..jusqua].split(',').count()))
                    }
                    _ => None,
                }
            })
            .collect()
    }

    /// ⛔ **DEUX BRAS POUR LE MÊME VERBE SE MASQUENT EN SILENCE — ET LE TEST DE
    /// PARITÉ NE PEUT PAS LE VOIR, PARCE QU'IL DÉDOUBLONNE.**
    ///
    /// Défaut réel du 2026-10-09 : `["ocr", valeur]` — la case « OCR français »
    /// de l'onglet Réglages — capturait le clic sur « Reconnaître » de la vue de
    /// page. Le panneau réglait donc un RÉGLAGE au lieu de lancer une
    /// reconnaissance : l'écran ne bougeait pas, et le rapport était « quand je
    /// clique sur reconnaître, rien ne se passe ». Le journal montrait l'action
    /// arrivée ; la fonction visée, jamais appelée. Seule l'arité ne suffisait
    /// pas à les distinguer — les deux bras en avaient deux.
    ///
    /// ⚠️ **C'EST L'ARITÉ QUI SÉPARE LES CAS LÉGITIMES** : `["profil"]` et
    /// `["profil", nom]` sont deux bras VOULUS (le profil vide est mangé en route,
    /// voir le commentaire du `match`). Deux bras de MÊME arité, en revanche, ne
    /// peuvent pas coexister sans que le second soit mort.
    ///
    /// **Ce test ÉCHOUE sur l'ancien code**, exactement sur `ocr/2` en double.
    #[test]
    fn aucun_verbe_n_a_deux_bras_de_meme_arite() {
        let mut signatures = signatures_du_dispatch();
        signatures.sort();
        let avant = signatures.len();
        signatures.dedup();
        assert_eq!(signatures.len(), avant, "deux bras pour le meme verbe : {signatures:?}");
    }

    /// ⭐ **LE MÊME CONTRAT QUE PARTOUT** : une page qui émet un verbe que Rust ne
    /// connaît pas ne produit aucune erreur — juste un clic sans effet.
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

    /// ⭐ **L'ÉTAT PORTE-T-IL LA PAGE QUAND ELLE EST OUVERTE ?**
    ///
    /// ⛔ **C'EST LE TEST QUI AURAIT DÛ EXISTER DÈS LE DÉBUT.** Un `if` posé en
    /// VALEUR dans un `json!` est exactement le motif que ce dépôt s'est déjà
    /// interdit une fois (`json!` tt-munché lit une accolade comme un OBJET) : le
    /// code compile, et la clé ne vaut pas ce qu'on croit. Ici l'enjeu est direct
    /// — si `page` reste `null`, le panneau ne bascule JAMAIS sur la page, et le
    /// clic sur une vignette ne fait RIEN, sans un mot.
    #[test]
    fn le_bandeau_de_page_suit_le_drapeau() {
        VUE_PAGE.with(|v| v.set(false));
        assert!(etat_json()["page"].is_null(), "sans page ouverte, l'etat doit porter null");

        VUE_PAGE.with(|v| v.set(true));
        let ouvert = etat_json()["page"].clone();
        VUE_PAGE.with(|v| v.set(false));

        assert!(ouvert.is_object(), "le drapeau pose, l'etat doit porter la PAGE, pas {ouvert:?}");
        // ⚠️ **`amelioration` EST UNE CLÉ COMME LES AUTRES** : le panneau la lit pour
        // poser le filtre sur l'image et placer les trois curseurs. Absente, il
        // afficherait la page sans réglage pendant que la fenêtre, elle, l'affiche
        // éclaircie — les deux hôtes en désaccord sur la même page.
        for cle in ["nom", "rang", "total", "ocr", "message", "amelioration"] {
            assert!(ouvert.get(cle).is_some(), "la page poussee ne porte pas {cle}");
        }
        assert_eq!(ouvert["ocr"].as_array().map(Vec::len), Some(crate::win32::ocr::Moteur::TOUS.len()));
    }

    /// **SONDE — `cargo test --bins -- --ignored --nocapture sonde_vignette`**
    ///
    /// ⛔ **CE QU'ELLE PROUVE, ET QU'AUCUNE CAPTURE NE PEUT PROUVER.** Les
    /// vignettes du panneau demandent une SESSION, donc une vraie numérisation :
    /// le harnais ne peut pas les photographier. Or c'est le seul chemin de ce
    /// dépôt qui **décode un JPEG** (`image::open`, et non `clipboard::decode_png`
    /// qui n'accepte que du PNG) avant de le réduire et de l'encoder.
    ///
    /// Elle fabrique donc la vignette d'une image donnée et dit ce qu'elle obtient
    /// — y compris quand ça échoue, ce qui est le cas intéressant.
    ///
    /// `NEURADESK_SONDE_OCR=<chemin d'une image>` (même levier que les sondes
    /// d'OCR : c'est la même « image de sonde »).
    #[test]
    #[ignore = "sonde : fabrique une vignette, NEURADESK_SONDE_OCR=<image>"]
    fn sonde_vignette() {
        let Some(chemin) = std::env::var_os("NEURADESK_SONDE_OCR").map(PathBuf::from) else {
            eprintln!("Posez NEURADESK_SONDE_OCR=<chemin d'une image> pour sonder.");
            return;
        };
        if !chemin.is_file() {
            eprintln!("{} est absent.", chemin.display());
            return;
        }
        match vignette_data_uri(&chemin) {
            Some(uri) => {
                let attendu = uri.starts_with("data:image/png;base64,");
                eprintln!("vignette : {} octets, prefixe PNG correct : {attendu}", uri.len());
                assert!(attendu, "la vignette doit etre un PNG en data URI");
            }
            // ⚠️ **C'EST LE CAS INTÉRESSANT** : un échec ici veut dire qu'une page
            // scannée afficherait une case grise sans vignette — et le journal
            // doit déjà porter la raison (voir `vignette_data_uri`).
            None => eprintln!("AUCUNE VIGNETTE pour {} — voir la ligne [scanner] ci-dessus.", chemin.display()),
        }
    }

    /// **L'ÉTAT PORTE TOUT CE QUE LA PAGE DESSINE.** Un champ oublié ici ne casse
    /// rien : la page lirait `undefined` et dessinerait un réglage faux — le
    /// défaut que ce test rend impossible.
    #[test]
    fn l_etat_porte_chaque_reglage_et_ses_choix() {
        let etat = etat_json();
        for cle in [
            "contentScale", "profils", "profil", "source", "sources", "dpi", "resolutions", "couleur", "couleurs",
            "format", "formats", "extension", "extensions", "deskew", "ocr", "pages",
            "destination", "destinationDefaut", "naps2", "enCours", "dernier",
        ] {
            assert!(etat.get(cle).is_some(), "l'etat ne porte pas {cle}");
        }
        // ⚠️ **`pages` EST UNE LISTE D'OBJETS, PLUS UN NOMBRE NI UNE LISTE DE
        // RANGS** : chaque entrée porte le rang AFFICHÉ et le nom du FICHIER, qui
        // est la clé des vignettes. Un rang seul ferait afficher la vignette d'une
        // autre page après un retrait au milieu.
        assert!(etat["pages"].is_array(), "pages est la LISTE de la session");
        assert_eq!(etat["sources"].as_array().map(Vec::len), Some(Source::TOUTES.len()));
        assert_eq!(etat["couleurs"].as_array().map(Vec::len), Some(Couleur::TOUTES.len()));
        assert_eq!(etat["formats"].as_array().map(Vec::len), Some(scanner::FORMATS.len()));
        assert_eq!(etat["resolutions"].as_array().map(Vec::len), Some(scanner::RESOLUTIONS.len()));
    }
}
