//! ⭐ **L'EFFET D'OUVERTURE DES FENÊTRES** — DEMANDE EXPLICITE (2026-10-16) : « un
//! effet d'ouverture pour les fenêtres des applis, une fenêtre volante qui
//! tourbillonne en s'agrandissant depuis la zone de l'appli, un peu comme faisait
//! Compiz ».
//!
//! **LE PRINCIPE.** La vraie fenêtre est rendue INVISIBLE dès sa création, avant
//! qu'elle ne paraisse ; son image, capturée par `win32::wgc`, s'envole de l'icône
//! de son application en tournant et en grandissant, dans une surcouche
//! transparente ; arrivée à sa place, la vraie fenêtre est rendue visible
//! dessous et la surcouche s'efface.
//!
//! **TOUT CE QUI SUIT A ÉTÉ MESURÉ** (sonde du 2026-10-16, sur `charmap.exe` et
//! `calc.exe`) :
//!
//! | Moyen de cacher la fenêtre d'un AUTRE processus | Résultat |
//! |---|---|
//! | `DWMWA_CLOAK` | ⛔ `E_ACCESSDENIED` : réservé au processus propriétaire |
//! | hors de l'écran (`SetWindowPos`) | OK, la capture continue |
//! | `WS_EX_LAYERED` + alpha 0 | OK, la capture continue (le contenu, pas le noir) |
//!
//! - La création (`EVENT_OBJECT_CREATE`) arrive ~30 ms après le lancement, la
//!   fenêtre encore INVISIBLE : c'est là qu'on la rend transparente, avant
//!   qu'elle n'ait jamais été vue. Hors de l'écran ne convient pas : l'application
//!   la place elle-même ENTRE sa création et son affichage. Une fenêtre que sa
//!   création ne désigne pas est cachée à son affichage (`EVENT_OBJECT_SHOW`) :
//!   voir `rappel`.
//! - La première image capturée arrive ~150 ms après l'ouverture de la session,
//!   souvent avant que l'application ne se soit dessinée (voir `image_vide`).
//! - ⭐ **LA CAPTURE EST CELLE DE `win32::capture_fenetre`** depuis le
//!   2026-11-04 (`PrintWindow`), et non plus `Windows.Graphics.Capture` :
//!   celle-ci exigeait une fenêtre VISIBLE, refusait jusqu'à ce qu'on lui rende
//!   sa région, ouvrait une session (600 ms sur Chromium) et laissait 15 à
//!   70 Mo de mémoire GPU par fenêtre ouverte. MESURÉ : `PrintWindow` lit une
//!   fenêtre sous alpha nul ET hors de l'écran — l'état où l'effet la tient —
//!   en 10 à 40 ms, image entière dès le premier appel. Elle ne montre donc
//!   RIEN de moins, et ne fait attendre que la réapparition du contenu.
//! - ⛔ **RÉSIDU CONNU, IRRÉDUCTIBLE** : cacher une fenêtre lui ENVOIE un message
//!   (voir `installer`). Une application occupée au moment de son affichage (Table
//!   des caractères qui charge ses polices, Explorateur) ne le traite qu'après :
//!   elle se montre avant de disparaître pour son vol — 10 à 20 ms d'après son
//!   style, mais l'Explorateur reste À L'ÉCRAN ~110 ms (photos du 2026-10-16) :
//!   devenue `LAYERED`, une fenêtre n'est recomposée qu'une fois repeinte.
//! - ⛔ Une application du Store (`ApplicationFrameWindow`) naît CACHÉE par le
//!   shell : elle n'est pas touchée (voir `CLASSES_ECARTEES`).
//!
//! ⭐ **LES FENÊTRES DES APPLETS AUSSI** — DEMANDE EXPLICITE (2026-10-16) : les
//! panneaux de NeuraDesk (`win32::cef_popup`) passent par `animer_interne`. Pour
//! NOS fenêtres, `DWMWA_CLOAK` est permis : elles sont masquées par DWM sans
//! toucher à leur style, et WGC capture une fenêtre masquée (MESURÉ sur la
//! calculatrice, masquée par le shell : 17 images). Le crochet ne les voit pas
//! (`WINEVENT_SKIPOWNPROCESS`). Si l'interface est exclue des captures (Réglages
//! › « Visible dans les captures »), l'image est noire et le panneau paraît sans
//! effet (`image_vide`).
//!
//! ⛔ **UNE FENÊTRE CACHÉE DOIT TOUJOURS ÊTRE RENDUE** : chaque étape a son délai
//! (`ATTENTE_AFFICHAGE`, `ATTENTE_IMAGE`), et `rendre_tout` passe à la fermeture.
//! Une fenêtre qu'on aurait perdue resterait invisible jusqu'à sa fermeture.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::core::{w, Result};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmExtendFrameIntoClientArea, DwmFlush, DwmSetWindowAttribute, DWMWA_CLOAK, DWMWA_CLOAKED, DWMWA_TRANSITIONS_FORCEDISABLED, DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, RedrawWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST, RDW_ALLCHILDREN, RDW_ERASE,
    RDW_FRAME, RDW_INVALIDATE,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MENU};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows, GetWindowThreadProcessId, GetLayeredWindowAttributes, GetMessageW, GetWindow,
    MsgWaitForMultipleObjectsEx, PeekMessageW, WM_APP,
    PostThreadMessageW, TranslateMessage, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, WM_QUIT, GetWindowLongPtrW, IsIconic, IsWindow, IsWindowVisible,
    RegisterClassExW, SendMessageTimeoutW, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow, CS_HREDRAW,
    SMTO_ABORTIFHUNG, SMTO_BLOCK, WM_NULL,
    CS_VREDRAW, EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, EVENT_OBJECT_SHOW, EVENT_SYSTEM_FOREGROUND, GWL_EXSTYLE, GWL_STYLE, GW_OWNER,
    GetForegroundWindow, HWND_TOP, HWND_TOPMOST, LWA_ALPHA, SWP_FRAMECHANGED, OBJID_WINDOW, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS, WNDCLASSEXW, WS_CAPTION, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_MINIMIZEBOX, GetClassNameW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::gfx::device::GfxDevice;
use windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView;
use crate::gfx::quad_renderer::{QuadBatch, QuadInstance, QuadRenderer};
use crate::gfx::swapchain::SwapChain;
use crate::win32::app_identity::WindowTarget;
use crate::win32::capture_fenetre::Capture;

/// Durée du vol, de l'icône à la place de la fenêtre, par défaut — réglable
/// (DEMANDE EXPLICITE : « pouvoir paramétrer la durée de l'effet »).
pub const DUREE_PAR_DEFAUT_MS: u32 = 420;
/// Bornes du réglage de la durée, en millisecondes.
pub const DUREE_MIN_MS: u32 = 150;
pub const DUREE_MAX_MS: u32 = 2000;
/// Tours complets faits pendant le vol.
const TOURS: f32 = 1.0;
/// Une fenêtre cachée à sa création qui ne paraît pas dans ce délai (fenêtre de
/// service, jamais montrée) est rendue telle quelle. Large : une application
/// Electron crée sa fenêtre cachée et ne la montre qu'une fois sa page chargée.
const ATTENTE_AFFICHAGE: Duration = Duration::from_secs(15);
/// Sans première image dessinée dans ce délai, la fenêtre paraît sans effet.
const ATTENTE_IMAGE: Duration = Duration::from_millis(600);
/// Côté du carré, au centre de l'image, qui dit si elle est encore vide.
const SONDE_VIDE: u32 = 16;
/// Une image d'un seul aplat attend son contenu au plus ce délai, compté depuis
/// l'ouverture de la session (voir `image_uniforme`) ; ensuite elle vole telle quelle.
const ATTENTE_CONTENU: Duration = Duration::from_millis(1500);
/// **NOS fenêtres**, ce n'est pas un délai : c'est une horloge fixe. Le voile DWM
/// que l'effet leur pose elles-mêmes exclut la fenêtre de la composition : aucune
/// image NEUVE n'arrive pendant qu'elle est voilée, et `IMMOBILITE` ne peut pas
/// non plus déclencher (rien de neuf à mesurer). Leur première image capturée est
/// pourtant le contenu RÉEL — la fenêtre était dessinée avant d'être masquée.
/// MESURÉ (2026-10-22, sonde `C:/ndwork/probe_fenetres`) : chaque ouverture d'une
/// kaomoji épuisait les 1 500 ms de `ATTENTE_CONTENU` pour une image neuve jamais
/// venue, puis volait 420 ms — **2,04 s** d'écran vide après le clic, deux
/// ouvertures de suite.
const ATTENTE_CONTENU_INTERNE: Duration = Duration::from_millis(250);
/// Un aplat qui ne change plus depuis ce délai est l'application elle-même (un
/// document vide) : il vole sans attendre `ATTENTE_CONTENU`. MESURÉ (2026-10-17) :
/// Edge, en chargeant, reste un aplat jusqu'à 246 ms sans nouvelle image ; le
/// Bloc-notes, dessiné, n'en livre plus avant 470 ms (son curseur).
const IMMOBILITE: Duration = Duration::from_millis(350);
/// Points relevés par côté pour dire si l'image est un aplat (voir `image_uniforme`).
const GRILLE_UNIFORME: u32 = 8;
/// Écart de couleur, sur un canal, au-delà duquel l'image a du contenu.
const ECART_CONTENU: u8 = 24;
/// La surcouche reste sur la fenêtre rendue le temps que celle-ci se repeigne
/// (retirer `WS_EX_LAYERED` recrée sa surface).
const RECOUVREMENT: Duration = Duration::from_millis(60);
/// Une fenêtre qui ne se laisse pas rendre dans ce délai (application figée)
/// voit son image retirée quand même : elle paraîtra quand elle répondra.
const ATTENTE_RENDU: Duration = Duration::from_secs(5);
/// Au repos, le fil se réveille au moins à ce rythme, pour les délais.
const REVEIL: Duration = Duration::from_millis(250);
/// Plus petite fenêtre animée, en pixels physiques de côté.
const COTE_MIN: i32 = 120;
/// Rayon des coins des fenêtres de Windows 11, en pixels logiques.
const RAYON_FENETRE: f32 = 8.0;
/// La texture ENTIÈRE — le sous-rectangle UV d'une image non découpée.
const UV_PLEINE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

static ACTIVE: AtomicBool = AtomicBool::new(false);
static DUREE_MS: AtomicU32 = AtomicU32::new(DUREE_PAR_DEFAUT_MS);

mod effets;
mod fermeture;

/// ⭐ **LES QUATRE MOMENTS ANIMÉS**, chacun avec SA forme
/// (DEMANDE EXPLICITE 2026-11-05 : « choisir n'importe quel effet pour
/// l'ouverture, la fermeture, la mise en avant et la réduction »).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moment {
    Ouverture,
    Fermeture,
    Reduction,
    /// Agrandir ET restaurer : les deux sens d'un même geste.
    Agrandissement,
}

impl Moment {
    pub const TOUS: [Moment; 4] = [Moment::Ouverture, Moment::Fermeture, Moment::Reduction, Moment::Agrandissement];

    /// Ce que les Réglages rangent pour ce moment.
    fn code(self) -> &'static AtomicU32 {
        match self {
            Moment::Ouverture => &EFFET_OUVERTURE_CODE,
            Moment::Fermeture => &EFFET_FERMETURE_CODE,
            Moment::Reduction => &EFFET_REDUCTION_CODE,
            Moment::Agrandissement => &EFFET_AGRANDISSEMENT_CODE,
        }
    }

    /// Les formes que ce moment propose — et parmi lesquelles « au hasard »
    /// tire.
    fn formes(self) -> &'static [effets::Effet] {
        match self {
            Moment::Agrandissement => &effets::AVEC_ETIREMENT,
            _ => &effets::FORMES,
        }
    }
}

/// **LE CODE 255, C'EST « AU HASARD »** : à chaque animation, une forme est
/// tirée parmi celles du moment. Rangé comme les autres dans le fichier du
/// moment — c'est un choix, pas un état.
pub const EFFET_ALEATOIRE: u32 = 255;

static EFFET_OUVERTURE_CODE: AtomicU32 = AtomicU32::new(0);
static EFFET_FERMETURE_CODE: AtomicU32 = AtomicU32::new(0);
static EFFET_REDUCTION_CODE: AtomicU32 = AtomicU32::new(0);
/// L'étirement (5) : la fenêtre passe d'un rectangle à l'autre, elle ne
/// disparaît pas. C'est le défaut, et le seul choix qui ne la cache jamais.
static EFFET_AGRANDISSEMENT_CODE: AtomicU32 = AtomicU32::new(5);

/// Range le choix. Un code qui ne fait pas partie des formes du moment (ou
/// inconnu) rend le défaut : la première forme de la liste. ⛔ Un code inconnu
/// n'efface pas « au hasard ».
pub fn regler_effet(moment: Moment, code: u32) {
    moment.code().store(code_retenu(moment, code), Ordering::Relaxed);
}

/// Le code qu'on range réellement — pur, donc testable sans toucher au réglage.
/// ⛔ Le défaut est la PREMIÈRE forme DU MOMENT : le vol pour un geste qui fait
/// disparaître la fenêtre, l'étirement pour agrandir (il ne la cache pas). Un
/// code inconnu est NORMALISÉ (999 → le code de sa forme, pas 999) : sinon les
/// Réglages rangeraient une valeur qu'aucun bouton ne surligne.
fn code_retenu(moment: Moment, code: u32) -> u32 {
    if code == EFFET_ALEATOIRE {
        return code;
    }
    let effet = effets::Effet::depuis_code(code);
    // ⛔ Un code INCONNU rend le vol dans `depuis_code` : ici, l'inconnu doit
    // rendre le défaut DU MOMENT (l'étirement pour agrandir), d'où le test sur
    // l'aller-retour du code avant celui sur l'appartenance.
    if effet.code() == code && moment.formes().contains(&effet) {
        code
    } else {
        moment.formes()[0].code()
    }
}

/// Le code réglé pour ce moment — `EFFET_ALEATOIRE` compris : les Réglages le
/// renvoient tel quel, sinon le bouton « Aléatoire » ne resterait pas surligné.
pub fn effet_code(moment: Moment) -> u32 {
    moment.code().load(Ordering::Relaxed)
}

pub fn regler_effet_fermeture(code: u32) {
    regler_effet(Moment::Fermeture, code);
}

pub fn effet_fermeture_code() -> u32 {
    effet_code(Moment::Fermeture)
}

/// **LA FORME À JOUER MAINTENANT** — le tirage, quand le réglage dit « au
/// hasard ». Résolue UNE fois par animation : une fenêtre qui s'ouvre ne
/// change pas de forme en plein vol, et l'ouverture comme la fermeture tirent
/// chacune de leur côté.
pub fn effet_du_moment(moment: Moment) -> effets::Effet {
    forme_du_code(moment, effet_code(moment))
}

/// La forme que désigne `code` pour ce moment — pur, hors le tirage.
fn forme_du_code(moment: Moment, code: u32) -> effets::Effet {
    let formes = moment.formes();
    if code == EFFET_ALEATOIRE {
        return au_hasard(formes);
    }
    let effet = effets::Effet::depuis_code(code);
    if formes.contains(&effet) {
        effet
    } else {
        formes[0]
    }
}

/// Un tirage parmi `formes` — xorshift64 semé à l'heure. Pas de crate : il n'y
/// a rien à garantir de plus qu'un effet visuel varié.
fn au_hasard(formes: &[effets::Effet]) -> effets::Effet {
    use std::sync::atomic::AtomicU64;
    static ETAT: AtomicU64 = AtomicU64::new(0);
    let mut x = ETAT.load(Ordering::Relaxed);
    if x == 0 {
        x = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15)
            | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    ETAT.store(x, Ordering::Relaxed);
    formes[(x % formes.len() as u64) as usize]
}

/// Durée du retour d'une fenêtre qui se ferme, par défaut — réglable, mêmes
/// bornes que l'ouverture (voir `win32::ouverture_fenetres::fermeture`).
pub const DUREE_FERMETURE_PAR_DEFAUT_MS: u32 = 300;
static FERMETURE_ACTIVE: AtomicBool = AtomicBool::new(false);
static DUREE_FERMETURE_MS: AtomicU32 = AtomicU32::new(DUREE_FERMETURE_PAR_DEFAUT_MS);
/// Les fenêtres dont on garde l'image, triées (voir `Fermetures::surveillees`) :
/// le crochet ne signale que leur disparition.
static SURVEILLEES: Mutex<Vec<isize>> = Mutex::new(Vec::new());
/// Les fenêtres qui ont un VOL en cours, publiées par le fil de l'animation.
/// ⛔ Le crochet les consulte AVANT de cacher pour une bascule : cacher une
/// seconde fois une fenêtre déjà en vol l'enregistrerait comme « à rendre »
/// alors que son animation est en cours.
static EN_VOL: Mutex<Vec<isize>> = Mutex::new(Vec::new());

/// **L'ANIMATION D'ALT+TAB** (demande explicite 2026-11-05 : « tout, Alt+Tab
/// compris ») : quand le premier plan change PENDANT que Alt est enfoncé —
/// c'est-à-dire une sélection d'Alt+Tab, et non un simple clic — la fenêtre
/// choisie est capturée, cachée, puis présentée en grandissant de 94 % à sa
/// taille (`Effet::Bascule`), comme le « Window switch » du mod Windhawk.
static BASCULE_ACTIVE: AtomicBool = AtomicBool::new(true);
/// Durée de cette présentation — celle du mod (200 ms, réglable là-bas).
pub const DUREE_BASCULE_MS: u32 = 200;

pub fn activer_bascule(active: bool) {
    BASCULE_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn bascule_active() -> bool {
    BASCULE_ACTIVE.load(Ordering::Relaxed)
}

pub fn activer_fermeture(active: bool) {
    FERMETURE_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn fermeture_active() -> bool {
    FERMETURE_ACTIVE.load(Ordering::Relaxed)
}

pub fn regler_duree_fermeture(ms: u32) {
    DUREE_FERMETURE_MS.store(ms.clamp(DUREE_MIN_MS, DUREE_MAX_MS), Ordering::Relaxed);
}

pub fn duree_fermeture_ms() -> u32 {
    DUREE_FERMETURE_MS.load(Ordering::Relaxed)
}

fn duree_fermeture() -> Duration {
    Duration::from_millis(duree_fermeture_ms() as u64)
}

pub fn regler_duree(ms: u32) {
    DUREE_MS.store(ms.clamp(DUREE_MIN_MS, DUREE_MAX_MS), Ordering::Relaxed);
}

pub fn duree_ms() -> u32 {
    DUREE_MS.load(Ordering::Relaxed)
}

fn duree() -> Duration {
    Duration::from_millis(duree_ms() as u64)
}

pub fn activer(active: bool) {
    ACTIVE.store(active, Ordering::Relaxed);
}

/// ⭐ **LE FONDU DES FENÊTRES DE NEURADESK** — DEMANDE EXPLICITE (2026-10-25) :
/// « un effet de fondu paramétrable à l'ouverture et à la fermeture des fenêtres
/// neuradesk ». Un panneau ou une fenêtre de NeuraDesk (voir `animer_interne`)
/// apparaît et disparaît EN PLACE, en fondant, au lieu de voler de son icône.
/// Même machinerie que le vol : l'image capturée, posée dans la surcouche avec
/// une opacité (`Image::opacite`), pendant que la vraie fenêtre reste voilée.
/// Indépendant de l'effet des fenêtres d'application (`est_active`). Les MENUS
/// gardent leur propre fondu (`win32::fade`).
static FONDU_ACTIF: AtomicBool = AtomicBool::new(false);
pub const DUREE_FONDU_PAR_DEFAUT_MS: u32 = 200;
/// Bornes du réglage des durées du fondu, en millisecondes.
pub const DUREE_FONDU_MIN_MS: u32 = 50;
pub const DUREE_FONDU_MAX_MS: u32 = 1200;
static DUREE_FONDU_OUVERTURE_MS: AtomicU32 = AtomicU32::new(DUREE_FONDU_PAR_DEFAUT_MS);
static DUREE_FONDU_FERMETURE_MS: AtomicU32 = AtomicU32::new(DUREE_FONDU_PAR_DEFAUT_MS);

pub fn activer_fondu(actif: bool) {
    FONDU_ACTIF.store(actif, Ordering::Relaxed);
}

pub fn fondu_actif() -> bool {
    FONDU_ACTIF.load(Ordering::Relaxed)
}

pub fn regler_duree_fondu_ouverture(ms: u32) {
    DUREE_FONDU_OUVERTURE_MS.store(ms.clamp(DUREE_FONDU_MIN_MS, DUREE_FONDU_MAX_MS), Ordering::Relaxed);
}

pub fn duree_fondu_ouverture_ms() -> u32 {
    DUREE_FONDU_OUVERTURE_MS.load(Ordering::Relaxed)
}

pub fn regler_duree_fondu_fermeture(ms: u32) {
    DUREE_FONDU_FERMETURE_MS.store(ms.clamp(DUREE_FONDU_MIN_MS, DUREE_FONDU_MAX_MS), Ordering::Relaxed);
}

pub fn duree_fondu_fermeture_ms() -> u32 {
    DUREE_FONDU_FERMETURE_MS.load(Ordering::Relaxed)
}

/// **LE FONDU PROPRE À CERTAINES FENÊTRES** — aujourd'hui la sphère des
/// programmes (`win32::cef_fenetres`), DEMANDE EXPLICITE du 2026-11-04 :
/// régler la durée du fondu de CE mode « propre à la sphère », sans toucher au
/// réglage global des autres fenêtres de NeuraDesk.
///
/// Une entrée vaut (fenêtre, apparition_ms, disparition_ms). Elle est posée à
/// chaque ouverture et OUBLIÉE à la destruction : un HANDLE réutilisé ne doit
/// pas hériter d'un réglage qui ne le concerne pas.
///
/// Une entrée présente vaut aussi ACTIVATION du fondu pour cette fenêtre, même
/// si le réglage global est éteint — sinon un curseur « propre à la sphère »
/// n'aurait aucun effet visible chez qui a coupé le fondu global.
static FONDU_PROPRE: Mutex<Vec<(isize, u32, u32)>> = Mutex::new(Vec::new());

pub fn regler_fondu_de_fenetre(hwnd: HWND, apparition_ms: u32, disparition_ms: u32) {
    let cle = hwnd.0 as isize;
    let apparition = apparition_ms.clamp(DUREE_FONDU_MIN_MS, DUREE_FONDU_MAX_MS);
    let disparition = disparition_ms.clamp(DUREE_FONDU_MIN_MS, DUREE_FONDU_MAX_MS);
    let mut liste = FONDU_PROPRE.lock().unwrap_or_else(|e| e.into_inner());
    liste.retain(|(h, _, _)| *h != cle);
    liste.push((cle, apparition, disparition));
}

fn oublier_fondu_de_fenetre(hwnd: isize) {
    let mut liste = FONDU_PROPRE.lock().unwrap_or_else(|e| e.into_inner());
    liste.retain(|(h, _, _)| *h != hwnd);
}

pub(super) fn fondu_propre(hwnd: isize) -> Option<(u32, u32)> {
    let liste = FONDU_PROPRE.lock().unwrap_or_else(|e| e.into_inner());
    liste.iter().find(|(h, _, _)| *h == hwnd).map(|(_, a, d)| (*a, *d))
}

/// Vrai si AU MOINS une fenêtre porte un fondu propre — `fermeture::tick` en a
/// besoin pour ne pas couper sa boucle quand le réglage GLOBAL est éteint.
pub(super) fn un_fondu_propre_existe() -> bool {
    let liste = FONDU_PROPRE.lock().unwrap_or_else(|e| e.into_inner());
    !liste.is_empty()
}

/// La durée d'apparition propre à `hwnd`, s'il en a une.
pub(super) fn duree_fondu_apparition_de(hwnd: isize) -> Option<u32> {
    fondu_propre(hwnd).map(|(apparition, _)| apparition)
}

/// La durée de disparition propre à `hwnd`, s'il en a une.
pub(super) fn duree_fondu_fermeture_de(hwnd: isize) -> Option<u32> {
    fondu_propre(hwnd).map(|(_, disparition)| disparition)
}

/// La fenêtre `ex` s'ouvre-t-elle en fondant ? Les nôtres seulement — et le
/// fondu PROPRE d'une fenêtre vaut activation pour elle seule.
fn en_fondu(hwnd: isize, ex: isize) -> bool {
    ex == INTERNE && (fondu_actif() || fondu_propre(hwnd).is_some())
}

/// La durée de l'apparition de la fenêtre `ex` : son fondu (propre d'abord), ou
/// son vol.
fn duree_d_apparition(hwnd: isize, ex: isize) -> Duration {
    if ex == INTERNE {
        if let Some(ms) = duree_fondu_apparition_de(hwnd) {
            return Duration::from_millis(ms as u64);
        }
    }
    if en_fondu(hwnd, ex) {
        Duration::from_millis(duree_fondu_ouverture_ms() as u64)
    } else {
        duree()
    }
}

/// La durée du vol d'une fenêtre : celle de la BASCULE d'Alt+Tab, sinon celle
/// de son apparition (fondu propre, ou ouverture).
fn duree_du_vol(fenetre: &Fenetre, hwnd: isize) -> Duration {
    if fenetre.bascule {
        Duration::from_millis(DUREE_BASCULE_MS as u64)
    } else {
        duree_d_apparition(hwnd, fenetre.ex)
    }
}

/// L'opacité du fondu à l'instant `t` (0 → 1), adoucie aux deux bouts.
fn opacite_du_fondu(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn est_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// ⭐ **LES GESTES QUE LE MODULE INJECTÉ SAIT RETENIR.** Même table que
/// `window-hook/src/lib.rs` : le numéro voyage dans `lParam` du message
/// `NeuraDesk.GesteFenetre` (voir `win32::window::message_de_geste`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Geste {
    /// `WM_CLOSE` : la fenêtre va disparaître.
    Fermer,
    /// `WM_SYSCOMMAND SC_MINIMIZE` : elle va se réduire, et survivre.
    Reduire,
    /// `WM_SYSCOMMAND SC_MAXIMIZE` : elle va occuper son écran de travail.
    Agrandir,
    /// `WM_SYSCOMMAND SC_RESTORE` : elle revient à sa place d'avant.
    /// ⛔ Une fenêtre RÉDUITE qui se restaure n'arrive pas ici : la DLL ne
    /// retient pas ce cas (sa capture est vide, et son retour vient de la barre
    /// des tâches).
    Restaurer,
    /// ⭐ **LANCEMENT** : le module injecté a VOILÉ la fenêtre avant son premier
    /// affichage (`DWMWA_CLOAK`, dans son processus) et nous laisse l'animer —
    /// c'est ce qui supprime le flash de ~0,3 s du chemin externe. On capture à
    /// travers le voile (`PrintWindow`), on fait voler l'image, puis on
    /// renvoie le message de dévoilage à la DLL (voir `devoiler_dll`).
    Lancer,
}

impl Geste {
    /// `None` pour un numéro inconnu — un module plus récent que nous ne doit
    /// pas nous faire animer n'importe quoi.
    pub fn depuis_numero(numero: u32) -> Option<Self> {
        match numero {
            1 => Some(Self::Fermer),
            2 => Some(Self::Reduire),
            3 => Some(Self::Agrandir),
            4 => Some(Self::Restaurer),
            5 => Some(Self::Lancer),
            _ => None,
        }
    }
}

/// Ce que le crochet a fait, relevé à chaque frame par `EffetOuverture::tick`.
enum Evenement {
    /// Rendue transparente à sa création ; `ex` : son style étendu d'origine.
    Cachee { hwnd: isize, ex: isize },
    /// Une fenêtre de NeuraDesk, masquée par DWM ; `depart` : l'icône d'où elle
    /// part, si on la connaît.
    Interne { hwnd: isize, depart: Option<[f32; 4]> },
    /// Montrée. `ex` : son style d'origine si elle vient d'être cachée À
    /// L'AFFICHAGE ; `None` si elle l'était déjà depuis sa création.
    Montree { hwnd: isize, ex: Option<isize> },
    /// **UNE SÉLECTION D'ALT+TAB** : le premier plan vient de changer alors que
    /// Alt était enfoncé (voir `rappel_bascule`). La fenêtre a été cachée SUR LE
    /// FIL DU CROCHET (voir `cacher`, qui n'attend jamais une fenêtre muette) ;
    /// `ex` est son style étendu d'origine. Elle revient en grandissant.
    Bascule { hwnd: isize, ex: isize },
    Detruite(isize),
    /// Une fenêtre dont on garde l'image s'est cachée (voir `fermeture`).
    Masquee(isize),
    /// **RETIRE UNE FENÊTRE DE L'EFFET DE FERMETURE** — voir `oublier`, qui
    /// porte le pourquoi (une capture doit la faire disparaître SANS fantôme).
    Oublier(isize),
    /// Un geste RETENU par le module injecté (`window-hook`) : la fenêtre
    /// allait se fermer (`fermer`) ou se réduire (`reduire`), l'application
    /// attend notre feu vert. `retenue_ms` est le plafond que la DLL s'est
    /// donné.
    Geste { hwnd: isize, geste: Geste, retenue_ms: u64 },
    /// ⭐ **UN LANCEMENT VOILÉ PAR LA DLL** (voir `Geste::Lancer`) : la fenêtre
    /// est cloakée dans son processus, on la capture à travers le voile et on la
    /// fait voler, puis on redemande le dévoilage à la DLL.
    Lancement { hwnd: isize },
}

/// La file du fil du crochet vers la boucle principale.
static EVENEMENTS: Mutex<Vec<Evenement>> = Mutex::new(Vec::new());

fn relever() -> Vec<Evenement> {
    std::mem::take(&mut *EVENEMENTS.lock().unwrap_or_else(|e| e.into_inner()))
}

thread_local! {
    /// Sur le fil du crochet : les fenêtres cachées à leur création, pas encore
    /// montrées — pour ne pas les cacher une seconde fois à l'affichage (le style
    /// « d'origine » relevé alors porterait déjà `WS_EX_LAYERED`).
    static CACHEES_A_LA_CREATION: RefCell<HashMap<isize, Instant>> = RefCell::new(HashMap::new());
}

/// Le fil de l'animation : son identifiant (pour lui demander de s'arrêter) et
/// sa poignée (pour l'attendre).
static FIL: Mutex<Option<(u32, std::thread::JoinHandle<()>)>> = Mutex::new(None);
/// Les identifiants des deux fils, pour se parler (`PostThreadMessageW`).
static FIL_CROCHET: AtomicU32 = AtomicU32::new(0);
static FIL_RENDU: AtomicU32 = AtomicU32::new(0);
/// Au fil du crochet : rends la fenêtre `wParam`, de style d'origine `lParam`.
const WM_RENDRE: u32 = WM_APP + 1;
/// Au fil de l'animation : la file a reçu un évènement.
const WM_REVEIL: u32 = WM_APP + 2;

/// Les icônes d'application du dock principal et leur rectangle ÉCRAN, publiés
/// par la boucle principale (voir `publier_icones`).
static ICONES: Mutex<Vec<(WindowTarget, [f32; 4])>> = Mutex::new(Vec::new());

/// La boucle principale publie les icônes d'où partent les fenêtres — seule
/// chose que l'effet lui demande.
pub fn publier_icones(icones: Vec<(WindowTarget, [f32; 4])>) {
    *ICONES.lock().unwrap_or_else(|e| e.into_inner()) = icones;
}

/// L'icône de l'application de `hwnd`, s'il y en a une dans le dock.
fn icone_de(hwnd: HWND) -> Option<[f32; 4]> {
    let fenetre = crate::win32::app_identity::open_window(hwnd)?;
    let icones = ICONES.lock().unwrap_or_else(|e| e.into_inner());
    icones.iter().find(|(cible, _)| fenetre.matches(cible)).map(|(_, rect)| *rect)
}

/// Lance l'effet, sur DEUX FILS À LUI.
///
/// ⛔ **PAS SUR LE FIL DE LA BOUCLE PRINCIPALE** — MESURÉ (2026-10-16) :
/// - un crochet hors contexte est servi par le pompage de messages du fil qui
///   l'a posé, et la boucle de rendu s'arrête parfois 500 à 860 ms ;
/// - pendant le vol, la boucle a des trous de 45 à 130 ms : l'animation aurait
///   saccadé à chaque ouverture.
///
/// ⛔ **ET LE CROCHET N'EST PAS LE FIL DE L'ANIMATION** — MESURÉ : changer le
/// style d'une fenêtre d'un AUTRE processus lui ENVOIE `WM_STYLECHANGING` et
/// attend qu'elle le traite. La Table des caractères, occupée à charger ses
/// polices, a figé le fil 3 s. C'est donc le fil du crochet qui cache et qui rend,
/// et le fil de l'animation ne fait que LIRE : il guette que la fenêtre soit
/// rendue (`est_rendue`) avant de retirer son image.
///
/// ⛔ **MAIS LE CROCHET N'ATTEND JAMAIS SANS FIN** : une fenêtre qui ne pompe plus
/// l'a figé pour de bon (2026-10-19), et avec lui l'effet de TOUTES les fenêtres.
/// Il ne cache qu'une fenêtre qui répond (`cacher`), et rend sur un fil à part
/// (`rendre_sans_attendre`).
pub fn installer() {
    let crochet = std::thread::Builder::new().name("ouverture-crochet".into()).spawn(|| unsafe {
        FIL_CROCHET.store(GetCurrentThreadId(), Ordering::SeqCst);
        let crochet = SetWinEventHook(
            EVENT_OBJECT_CREATE,
            EVENT_OBJECT_SHOW,
            None,
            Some(rappel),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        if crochet.is_invalid() {
            crate::diag!("[ouverture] crochet impossible");
            return;
        }
        // La FERMETURE : toutes les fenêtres, les nôtres comprises (les panneaux) —
        // filtrées dans `rappel_masquage` sur les deux qu'on surveille.
        let masquage = SetWinEventHook(EVENT_OBJECT_HIDE, EVENT_OBJECT_HIDE, None, Some(rappel_masquage), 0, 0, WINEVENT_OUTOFCONTEXT);
        if masquage.is_invalid() {
            crate::diag!("[fermeture] crochet impossible");
        }
        // ⭐ LE PREMIER PLAN — l'animation d'Alt+Tab (voir `rappel_bascule`).
        // `WINEVENT_SKIPOWNPROCESS` : nos propres panneaux ne se basculent pas.
        let bascule = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(rappel_bascule),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        if bascule.is_invalid() {
            crate::diag!("[bascule] crochet du premier plan impossible");
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_RENDRE {
                rendre_sans_attendre(msg.wParam.0 as isize, msg.lParam.0);
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
    if let Err(erreur) = crochet {
        crate::diag!("[ouverture] fil du crochet impossible : {erreur}");
        return;
    }
    let lance = std::thread::Builder::new().name("ouverture-rendu".into()).spawn(|| unsafe {
        FIL_RENDU.store(GetCurrentThreadId(), Ordering::SeqCst);
        // MTA : un appel COM n'y pompe pas de messages.
        let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
        let gfx = match GfxDevice::new() {
            Ok(gfx) => gfx,
            Err(erreur) => {
                crate::diag!("[ouverture] périphérique impossible : {erreur:?}");
                return;
            }
        };
        let mut effet = EffetOuverture::new();
        // ⛔ MESURÉ : créée au premier vol, la surcouche le figeait 3 s — son
        // `QuadRenderer` compile ses shaders. Elle est donc prête d'avance.
        match Surcouche::new(&gfx) {
            Ok(surcouche) => effet.surcouche = Some(surcouche),
            Err(erreur) => crate::diag!("[ouverture] surcouche impossible : {erreur:?}"),
        }
        let mut msg = MSG::default();
        'fil: loop {
            // Au repos, on dort jusqu'au prochain évènement (au plus `REVEIL`, pour
            // les délais) ; en vol, la présentation cadence la boucle.
            if !effet.anime() {
                MsgWaitForMultipleObjectsEx(None, REVEIL.as_millis() as u32, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            }
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break 'fil;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let debut = Instant::now();
            effet.tick(&gfx);
            // `Present` attend d'ordinaire l'écran ; s'il ne l'a pas fait, on
            // attend la composition suivante plutôt que de tourner à vide.
            if effet.anime() && debut.elapsed() < Duration::from_millis(4) {
                let _ = DwmFlush();
            }
        }
        effet.rendre_tout();
    });
    match lance {
        Ok(fil) => {
            let poignee = std::os::windows::io::AsRawHandle::as_raw_handle(&fil);
            let id = unsafe { windows::Win32::System::Threading::GetThreadId(windows::Win32::Foundation::HANDLE(poignee as _)) };
            *FIL.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, fil));
        }
        Err(erreur) => crate::diag!("[ouverture] fil impossible : {erreur}"),
    }
}

/// **RETIRE UNE FENÊTRE DE L'EFFET DE FERMETURE, AVANT DE LA CACHER** —
/// DEMANDE du 2026-11-04 : « je ne veux pas voir apparaître la fenêtre des
/// paramètres de capture sur la capture ».
///
/// ⛔ **LE FANTÔME, C'ÉTAIT CET EFFET-LÀ, ET LE JOURNAL LE DISAIT** : au clic sur
/// « Capturer », la fenêtre était cachée à `1225.641` (« s'efface en fondu ») et
/// le bureau était figé à `1225.665`, **20 ms plus tard** — en plein fondu. La
/// capture montrait donc l'image de la fenêtre en train de retomber vers son
/// icône. Ni le masquage ni DWM n'étaient en cause : c'est l'ANIMATION qui
/// dessine encore la fenêtre.
///
/// Le crochet de fermeture filtre sur `SURVEILLEES` : en retirant la fenêtre de
/// cette liste AVANT `ShowWindow(SW_HIDE)`, aucun `Masquee` n'est produit. La
/// file reçoit aussi `Oublier`, pour que son image quitte `fermeture` même si un
/// évènement était déjà parti.
pub fn oublier(hwnd: HWND) {
    let cle = hwnd.0 as isize;
    SURVEILLEES.lock().unwrap_or_else(|e| e.into_inner()).retain(|&h| h != cle);
    let fil = FIL_RENDU.load(Ordering::SeqCst);
    if fil == 0 {
        // Aucun fil d'animation : rien à oublier, et la file ne doit pas grossir.
        return;
    }
    EVENEMENTS.lock().unwrap_or_else(|e| e.into_inner()).push(Evenement::Oublier(cle));
    let _ = unsafe { PostThreadMessageW(fil, WM_REVEIL, WPARAM(0), LPARAM(0)) };
}

/// ⭐ **LE MODULE INJECTÉ NOUS ANNONCE UN GESTE RETENU.** Appelée par
/// `win32::window::wndproc` sur le message enregistré `NeuraDesk.GesteFenetre`
/// (voir `message_de_geste`). Elle ne fait que déposer l'évènement : tout le
/// travail (capture, dessin) appartient au fil de l'animation, qui a le
/// périphérique D3D.
pub fn geste_retenu(application: HWND, geste: u32, retenue_ms: u64) {
    let Some(geste) = Geste::depuis_numero(geste) else { return };
    if FIL_RENDU.load(Ordering::SeqCst) == 0 {
        return;
    }
    // ⛔ **LE MESSAGE EST PUBLIC — ON VÉRIFIE LA CIBLE.** voir `geste_plausible`.
    if !geste_plausible(application) {
        crate::diag!(
            "[geste] {:#x} refusé : fenêtre non animable, à nous, ou d'un hôte du bureau",
            application.0 as isize
        );
        return;
    }
    // ⭐ Le lancement suit le pipeline d'OUVERTURE (vol depuis l'icône), pas
    // celui des gestes retenus (`fermeture::animer_geste`) : c'est une fenêtre
    // qui PARAÎT, pas une qui disparaît ou change de taille.
    let evenement = if geste == Geste::Lancer {
        if !est_active() {
            // L'effet d'ouverture est éteint : on dévoile tout de suite, la DLL
            // ayant voilé pour rien (réglage changé après son chargement).
            devoiler_dll(application);
            return;
        }
        Evenement::Lancement { hwnd: application.0 as isize }
    } else {
        Evenement::Geste { hwnd: application.0 as isize, geste, retenue_ms: retenue_ms.clamp(80, 5000) }
    };
    EVENEMENTS.lock().unwrap_or_else(|e| e.into_inner()).push(evenement);
    let _ = unsafe { PostThreadMessageW(FIL_RENDU.load(Ordering::SeqCst), WM_REVEIL, WPARAM(0), LPARAM(0)) };
}

/// ⛔ **`RegisterWindowMessageW("NeuraDesk.GesteFenetre")` EST PUBLIC** : tout
/// processus de même intégrité peut poster ce message à la fenêtre du dock avec
/// N'IMPORTE QUELLE fenêtre en `wParam`. Sans contrôle, on lui ferait capturer
/// et **vider la région** d'une cible arbitraire — la barre des tâches resterait
/// invisible le temps de l'effet. On n'accepte donc que ce qu'on aurait animé
/// soi-même : une fenêtre réelle, de premier plan, réductible, sans
/// propriétaire, d'une autre application, et pas d'un hôte du bureau.
///
/// ⚠️ Ce n'est PAS une authentification : un processus de même intégrité peut
/// déjà manipuler les fenêtres lui-même. C'est ce qui empêche un message
/// accidentel — ou mal intentionné — de toucher le shell.
fn geste_plausible(hwnd: HWND) -> bool {
    if hwnd.0.is_null() || !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid != 0 && pid != std::process::id() && !crate::win32::injection::hote_exclu(pid) && est_animable(hwnd)
}

/// Une fenêtre de NeuraDesk qui va s'ouvrir : masquée tout de suite (DWM), elle
/// s'envolera de `depart` — ou fondra en place (`fondu_actif`) — dès qu'elle
/// sera montrée. Sans effet si les deux options sont coupées ou si le fil de
/// l'animation n'existe pas.
pub fn animer_interne(hwnd: HWND, depart: Option<[f32; 4]>) {
    if !(est_active() || fondu_actif()) || FIL_RENDU.load(Ordering::SeqCst) == 0 {
        return;
    }
    // ⛔ **LE VOILE RESTE.** Essai du 2026-10-22 : sans voile, la fenêtre paraît
    // SOUS sa vignette pendant tout le vol — refusé (« les fenêtres sont visibles
    // pendant que se joue l'animation d'ouverture »). Le rang Z qu'un premier plan
    // posé sous le voile ne remonte pas est rattrapé au dévoilage (`devoiler`).
    if !masquer_par_dwm(hwnd, true) {
        return;
    }
    EVENEMENTS.lock().unwrap_or_else(|e| e.into_inner()).push(Evenement::Interne { hwnd: hwnd.0 as isize, depart });
    let _ = unsafe { PostThreadMessageW(FIL_RENDU.load(Ordering::SeqCst), WM_REVEIL, WPARAM(0), LPARAM(0)) };
}

fn masquer_par_dwm(hwnd: HWND, masquer: bool) -> bool {
    let valeur: i32 = masquer.into();
    unsafe { DwmSetWindowAttribute(hwnd, DWMWA_CLOAK, &valeur as *const _ as _, std::mem::size_of::<i32>() as u32) }.is_ok()
}

/// Dévoile une fenêtre de NeuraDesk et, si elle a gardé le premier plan, la pose
/// en tête de sa bande. MESURÉ (2026-10-22, sonde Z sur la kaomoji) : un premier
/// plan posé sur une fenêtre voilée prend le focus clavier SANS remonter son rang
/// — 107-111 pendant tout le voile, le terminal à 84 par-dessus. Seulement si elle
/// est encore au premier plan : une fenêtre cliquée PENDANT le vol doit rester
/// dessus. `SWP_ASYNCWINDOWPOS` : la fenêtre vit sur le fil de l'interface, et
/// celui-ci (le fil de l'animation) ne doit jamais l'attendre.
fn devoiler(hwnd: HWND) {
    masquer_par_dwm(hwnd, false);
    unsafe {
        if GetForegroundWindow() == hwnd {
            let _ = SetWindowPos(hwnd, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS);
        }
    }
}

fn est_masquee_par_dwm(hwnd: HWND) -> bool {
    let mut masquee = 0u32;
    unsafe { DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut masquee as *mut _ as _, std::mem::size_of::<u32>() as u32) }.is_ok()
        && masquee != 0
}

/// `ex` d'une fenêtre de NeuraDesk : elle n'a pas été touchée dans son style.
const INTERNE: isize = -1;

/// ⭐ `ex` d'une fenêtre VOILÉE PAR LA DLL au lancement (`Geste::Lancer`) : son
/// style n'est pas à nous, elle est cachée par `DWMWA_CLOAK` dans son processus,
/// et c'est la DLL qui la dévoile (on lui en renvoie l'ordre). On ne lui vide
/// donc PAS la région et on ne la gare PAS hors de l'écran — le voile suffit.
const LANCEMENT: isize = -2;

/// Le message par lequel on demande à la DLL de dévoiler une fenêtre de
/// lancement (le vol s'est posé). Même nom que `window-hook::message_devoiler`.
fn message_devoiler() -> u32 {
    static MESSAGE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe {
        windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW(w!("NeuraDesk.DevoilerLancement"))
    })
}

/// Demande à la DLL de dévoiler `hwnd` (`PostMessage`, non bloquant). La DLL a
/// un garde-fou si ce message n'arrive jamais (voir son `tenter_voile_de_lancement`).
fn devoiler_dll(hwnd: HWND) {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(Some(hwnd), message_devoiler(), WPARAM(0), LPARAM(0));
    }
}

/// Une fenêtre de NeuraDesk masquée qui ne paraît pas dans ce délai est rendue.
const ATTENTE_AFFICHAGE_INTERNE: Duration = Duration::from_secs(3);

/// Le délai d'attente de contenu, selon la SORTE de fenêtre — voir
/// `ATTENTE_CONTENU_INTERNE` et le défaut mesuré qu'il porte.
fn delai_attente_contenu(ex: isize) -> Duration {
    if ex == INTERNE {
        ATTENTE_CONTENU_INTERNE
    } else {
        ATTENTE_CONTENU
    }
}

/// L'image capturée est-elle mûre pour voler ? `depuis` : durée depuis l'ouverture
/// de la session de capture ; `derniere` : durée depuis la dernière image NEUVE
/// (`None` tant qu'aucune n'est arrivée — le cas de NOS fenêtres voilées).
/// Sur l'ancien code, ce calcul tenait compte de `ATTENTE_CONTENU` pour TOUTES les
/// fenêtres ; le test `nos_fenetres_ne_attendent_pas_les_1500_ms_d_autrui` échoue
/// sur cette version.
fn contenu_attendu(ex: isize, depuis: Duration, derniere: Option<Duration>) -> bool {
    depuis > delai_attente_contenu(ex) || derniere.is_some_and(|d| d > IMMOBILITE)
}

/// Demande au fil du crochet de rendre la fenêtre ; à défaut, la rend ici.
fn demander_rendu(hwnd: HWND, ex: isize) {
    if ex == INTERNE {
        devoiler(hwnd);
        return;
    }
    if ex == LANCEMENT {
        // La DLL a voilé, la DLL dévoile : on lui en renvoie l'ordre.
        devoiler_dll(hwnd);
        return;
    }
    let fil = FIL_CROCHET.load(Ordering::SeqCst);
    let poste = fil != 0 && unsafe { PostThreadMessageW(fil, WM_RENDRE, WPARAM(hwnd.0 as usize), LPARAM(ex)) }.is_ok();
    if !poste {
        rendre(hwnd, ex);
    }
}

/// La fenêtre est-elle redevenue visible ? Lu sans rien lui envoyer.
fn est_rendue(hwnd: HWND, ex_origine: isize) -> bool {
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return true;
        }
        if ex_origine == INTERNE {
            return !est_masquee_par_dwm(hwnd);
        }
        // Lancement : rendue quand la DLL a retiré son voile (cross-process,
        // `DWMWA_CLOAKED` se lit de l'extérieur).
        if ex_origine == LANCEMENT {
            return !est_masquee_par_dwm(hwnd);
        }
        // Encore garée hors de l'écran : rien n'est rendu, quoi que dise le
        // style. `ramener` la ramène (asynchroniquement) — c'est ce retour-là
        // qu'on attend avant de retirer l'image de la surcouche.
        if ECARTEES.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|e| e.0 == hwnd.0 as isize) {
            return false;
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if ex & WS_EX_LAYERED.0 as isize == 0 {
            return true;
        }
        // Son alpha n'a jamais été le nôtre (`cacher` n'y a pas touché, voir la
        // mesure sur Inkscape) : la rendre, c'est lui rendre sa région et sa
        // place — ce que `rendre` vient de faire. `GetLayeredWindowAttributes`
        // échouerait de toute façon sur une fenêtre en `UpdateLayeredWindow`,
        // et on attendrait 255 pour toujours.
        if ex_origine & WS_EX_LAYERED.0 as isize != 0 {
            return true;
        }
        let mut alpha = 0u8;
        GetLayeredWindowAttributes(hwnd, None, Some(&mut alpha), None).is_ok() && alpha == 255
    }
}

/// À la fermeture de l'app : le fil rend les fenêtres qu'il cache encore, et on
/// l'attend un instant — sans quoi elles resteraient INVISIBLES.
pub fn arreter() {
    let Some((id, fil)) = FIL.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
    let _ = unsafe { PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0)) };
    let _ = unsafe { PostThreadMessageW(FIL_CROCHET.load(Ordering::SeqCst), WM_QUIT, WPARAM(0), LPARAM(0)) };
    let limite = Instant::now() + Duration::from_millis(500);
    while !fil.is_finished() && Instant::now() < limite {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// **LE CROCHET DU PREMIER PLAN** — le seul signal qui dise « on vient de
/// choisir cette fenêtre » (voir `Evenement::Bascule`).
/// ⛔ **ALT DOIT ÊTRE ENFONCÉ** : Windows active la fenêtre visée à CHAQUE
/// appui sur Tab, le sélecteur encore affiché ; un changement de premier plan
/// sans Alt serait un clic ou une activation de programme, qu'on n'anime pas.
unsafe extern "system" fn rappel_bascule(_: HWINEVENTHOOK, evenement: u32, hwnd: HWND, objet: i32, enfant: i32, _: u32, _: u32) {
    if evenement != EVENT_SYSTEM_FOREGROUND || objet != OBJID_WINDOW.0 || enfant != 0 || hwnd.0.is_null() {
        return;
    }
    if !bascule_active() || !candidate(hwnd) {
        return;
    }
    // Les nôtres : la bascule du dock sur ses propres panneaux n'a pas à jouer.
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == std::process::id() {
        return;
    }
    if GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000 == 0 {
        return;
    }
    // ⛔ **CACHER ICI, SUR LE FIL DU CROCHET** — comme à la création (voir
    // `cacher`) : changer le style d'une fenêtre d'un autre processus peut
    // l'attendre, et le fil de l'animation ne doit jamais être retenu par une
    // application. `cacher` refuse d'ailleurs une fenêtre qui ne répond pas.
    // Une fenêtre DÉJÀ en vol garde son animation : ne pas la cacher encore.
    if EN_VOL.lock().unwrap_or_else(|e| e.into_inner()).contains(&(hwnd.0 as isize)) {
        return;
    }
    let Some(ex) = cacher(hwnd) else { return };
    EVENEMENTS.lock().unwrap_or_else(|e| e.into_inner()).push(Evenement::Bascule { hwnd: hwnd.0 as isize, ex });
    let _ = PostThreadMessageW(FIL_RENDU.load(Ordering::SeqCst), WM_REVEIL, WPARAM(0), LPARAM(0));
}

unsafe extern "system" fn rappel_masquage(_: HWINEVENTHOOK, _: u32, hwnd: HWND, objet: i32, enfant: i32, _: u32, _: u32) {
    let cle = hwnd.0 as isize;
    if objet != OBJID_WINDOW.0 || enfant != 0 || cle == 0 || SURVEILLEES.lock().unwrap_or_else(|e| e.into_inner()).binary_search(&cle).is_err() {
        return;
    }
    EVENEMENTS.lock().unwrap_or_else(|e| e.into_inner()).push(Evenement::Masquee(cle));
    let _ = PostThreadMessageW(FIL_RENDU.load(Ordering::SeqCst), WM_REVEIL, WPARAM(0), LPARAM(0));
}

unsafe extern "system" fn rappel(_: HWINEVENTHOOK, evenement: u32, hwnd: HWND, objet: i32, enfant: i32, _: u32, _: u32) {
    if objet != OBJID_WINDOW.0 || enfant != 0 || hwnd.0.is_null() {
        return;
    }
    // ⭐ LA CRÉATION D'UNE FENÊTRE EST AUSSI LE MOMENT D'INJECTER `window-hook`
    // dans son application (voir `win32::injection`) : c'est 30 ms après le
    // lancement qu'on le sait, et l'application n'a encore rien demandé.
    // `proposer` ne fait que filtrer et déposer un pid.
    if evenement == EVENT_OBJECT_CREATE {
        crate::win32::injection::proposer(hwnd);
    }
    let cle = hwnd.0 as isize;
    let fait = match evenement {
        EVENT_OBJECT_CREATE if est_active() && candidate(hwnd) && !IsWindowVisible(hwnd).as_bool() && !dll_pilote(hwnd) => cacher(hwnd).map(|ex| {
            CACHEES_A_LA_CREATION.with(|c| {
                let mut c = c.borrow_mut();
                // Une fenêtre de service n'est jamais montrée : la liste ne doit
                // pas grossir sans fin.
                c.retain(|_, depuis| depuis.elapsed() < ATTENTE_AFFICHAGE);
                c.insert(cle, Instant::now());
            });
            Evenement::Cachee { hwnd: cle, ex }
        }),
        // ⭐ L'AFFICHAGE AUSSI — MESURÉ : explorer crée sa fenêtre de dossier
        // (`CabinetWClass`) à l'avance, SANS barre de titre ; sa création ne la
        // désigne pas. Cachée dans les millisecondes qui suivent son affichage,
        // elle n'a pas encore été composée.
        EVENT_OBJECT_SHOW => {
            let recu = Instant::now();
            let montree = if CACHEES_A_LA_CREATION.with(|c| c.borrow_mut().remove(&cle)).is_some() {
                Some(Evenement::Montree { hwnd: cle, ex: None })
            } else if est_active() && candidate(hwnd) && !IsIconic(hwnd).as_bool() && !dll_pilote(hwnd) {
                cacher(hwnd).map(|ex| Evenement::Montree { hwnd: cle, ex: Some(ex) })
            } else {
                None
            };
            if montree.is_some() {
                crate::diag!("[sonde] affichage {cle:#x} traité en {:?}", recu.elapsed());
                liberer_la_region(cle);
            }
            montree
        }
        EVENT_OBJECT_DESTROY => {
            CACHEES_A_LA_CREATION.with(|c| c.borrow_mut().remove(&cle));
            REGIONS_VIDEES.lock().unwrap_or_else(|e| e.into_inner()).retain(|&h| h != cle);
            ECARTEES.lock().unwrap_or_else(|e| e.into_inner()).retain(|e| e.0 != cle);
            Some(Evenement::Detruite(cle))
        }
        _ => None,
    };
    if let Some(fait) = fait {
        EVENEMENTS.lock().unwrap_or_else(|e| e.into_inner()).push(fait);
        let _ = PostThreadMessageW(FIL_RENDU.load(Ordering::SeqCst), WM_REVEIL, WPARAM(0), LPARAM(0));
    }
}

/// Une fenêtre d'application qui va paraître : de premier niveau, sans
/// propriétaire (ni dialogue), pas une fenêtre outil, et qui se réduit — barre
/// de titre de Windows, ou bouton de réduction d'un cadre dessiné par
/// l'application (Voicemeeter, Voicemod).
///
/// ⛔ Écartée : une fenêtre déjà `WS_EX_LAYERED` l'était — « son alpha lui
/// appartient ». ⭐ **ELLE EST ANIMÉE DEPUIS (2026-11-05)**, sans qu'on touche à
/// son style : voir `cacher` (région vidée + hors écran, alpha intact) et
/// `rendre`. MESURÉ sur Inkscape / GTK, qui naît `LAYERED`.
///
/// ⭐ **`WS_EX_NOREDIRECTIONBITMAP` N'EST PLUS ÉCARTÉE** — MESURÉ (2026-10-16,
/// sonde sur une fenêtre d'Edge) : l'alpha de fenêtre la fait bien disparaître
/// (écart de 35 niveaux avec la fenêtre visible) et la rend au pixel près. La
/// première version les écartait sur une supposition, et l'effet ne touchait
/// alors NI Chrome, NI Brave, NI Discord, NI Terminal — « ça n'a pas l'air de
/// marcher ». Seule l'application du Store reste à part (`CLASSES_ECARTEES`).
fn candidate(hwnd: HWND) -> bool {
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
    let ex = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    let sans_proprietaire = unsafe { GetWindow(hwnd, GW_OWNER) }.map_or(true, |p| p.0.is_null());
    // ⛔ MESURÉ : explorer crée un `WorkerW` TITRÉ en ouvrant une fenêtre de
    // dossier — une fenêtre du FOND D'ÉCRAN, qu'on ne doit jamais cacher.
    est_candidate(style, ex, sans_proprietaire) && !crate::win32::app_identity::is_desktop_window(hwnd) && !classe_ecartee(hwnd)
}

/// La MÊME fenêtre qu'on animerait — pour `win32::injection`, qui décide
/// d'injecter `window-hook` dans l'application qui vient d'en ouvrir une.
pub(crate) fn est_animable(hwnd: HWND) -> bool {
    candidate(hwnd)
}

/// ⭐ La fenêtre appartient-elle à un processus où la DLL pilote le lancement
/// (voir `injection::dll_pilote_le_lancement`) ? Si oui, le chemin EXTERNE la
/// laisse entièrement à la DLL : c'est elle qui voile la fenêtre avant son
/// affichage (donc sans le flash de ~0,3 s), et nous l'animons sur le geste
/// `Lancer`. Sans cette garde, les deux chemins videraient/voileraient la même
/// fenêtre — et la région vide casserait la capture `PrintWindow` de la DLL.
fn dll_pilote(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid != 0 && crate::win32::injection::dll_pilote_le_lancement(pid)
}

fn est_candidate(style: u32, ex: u32, sans_proprietaire: bool) -> bool {
    let exclus = WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0;
    // ⭐ **GTK DESSINE SON PROPRE CADRE.** MESURÉ (2026-11-05, Inkscape 1.4.3) :
    // son `gdkWindowToplevel` a `style = 0x16000000` — ni `WS_CAPTION`, ni
    // `WS_MINIMIZEBOX`, ni `WS_POPUP`, ni `WS_THICKFRAME` : rien de ce qui
    // désigne d'ordinaire une fenêtre d'application. L'effet ne s'y appliquait
    // donc qu'au hasard du moment où l'évènement tombait, pendant que GTK
    // complétait son style. Une fenêtre de PREMIER NIVEAU qui n'est pas un
    // `WS_POPUP` en est une : les écrans d'ouverture et les infobulles, eux,
    // se déclarent popups (et ceux qui ne le font pas sont des fenêtres
    // outil, déjà écartées).
    let se_reduit = style & WS_CAPTION.0 == WS_CAPTION.0
        || style & WS_MINIMIZEBOX.0 != 0
        || style & WS_POPUP.0 == 0;
    sans_proprietaire && style & WS_CHILD.0 == 0 && se_reduit && ex & exclus == 0
}

/// L'application du Store naît cachée par le shell, qui la montre lui-même
/// (voir la doc du module) ; une `CoreWindow`, ou le mandataire d'onglet qu'explorer
/// crée pour chaque onglet d'Edge (titré, sans propriétaire, jamais à l'écran),
/// n'ont rien à animer.
const CLASSES_ECARTEES: [&str; 3] =
    ["ApplicationFrameWindow", "Windows.UI.Core.CoreWindow", "Windows.Internal.Shell.TabProxyWindow"];

fn classe_ecartee(hwnd: HWND) -> bool {
    let mut tampon = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut tampon) }.max(0) as usize;
    let classe = String::from_utf16_lossy(&tampon[..n]);
    CLASSES_ECARTEES.iter().any(|c| classe == *c)
}

/// Au plus ce délai pour qu'une fenêtre étrangère réponde avant qu'on la cache.
const DELAI_REPONSE_MS: u32 = 1000;

/// La fenêtre traite-t-elle ses messages ? Au plus `DELAI_REPONSE_MS`, et sans
/// attendre du tout une fenêtre que Windows tient déjà pour figée.
fn repond(hwnd: HWND) -> bool {
    unsafe {
        SendMessageTimeoutW(hwnd, WM_NULL, WPARAM(0), LPARAM(0), SMTO_ABORTIFHUNG | SMTO_BLOCK, DELAI_REPONSE_MS, None).0 != 0
    }
}

/// Rend la fenêtre transparente ; son style étendu d'origine, ou `None` si
/// Windows a refusé (fenêtre d'un processus élevé : UIPI) ou si elle ne répond
/// pas.
///
/// ⛔ **DÉFAUT RÉEL (2026-10-19) : « l'animation de fermeture ne se joue plus ».**
/// `SetWindowLongPtrW` sur la fenêtre d'un autre processus attend qu'elle traite
/// `WM_STYLECHANGING`, SANS DÉLAI : une fenêtre dont le fil ne pompe plus fige le
/// fil du crochet POUR DE BON — mesuré : 0 ms de processeur pendant l'ouverture
/// d'une appli, plus aucun évènement, donc plus d'ouverture ni de fermeture pour
/// aucune fenêtre. On s'assure donc d'abord qu'elle répond (`repond`).
fn cacher(hwnd: HWND) -> Option<isize> {
    // SONDE (temporaire) : où passe le délai avant le voile ?
    let debut = Instant::now();
    if !repond(hwnd) {
        crate::diag!(
            "[ouverture] {} ne répond pas : laissée telle quelle (après {:?})",
            crate::win32::plein_ecran_zone::decrire(hwnd),
            debut.elapsed()
        );
        return None;
    }
    let apres_repond = debut.elapsed();
    // ⛔ **L'ALPHA D'UNE FENÊTRE DÉJÀ `LAYERED` N'EST PAS À NOUS.** MESURÉ
    // (2026-11-05, Inkscape 1.4.3 / GTK) : `gdkWindowToplevel` naît `LAYERED`
    // et son alpha passe de 0 à 255 — c'est le fondu de son écran d'ouverture,
    // posé par `UpdateLayeredWindow` (`GetLayeredWindowAttributes` échoue
    // dessus). Écraser cet alpha à 0 puis le rendre à 255 en plein fondu
    // laissait Inkscape sur un écran d'ouverture qui ne se termine pas : un
    // blocage au lancement.
    //
    // On l'anime donc SANS toucher au style : la région vidée la cache sur-le-
    // champ, `ecarter` la gare hors de l'écran, et `rendre` ne lui rend que sa
    // région et sa place (voir `rendre`, qui ne défait que ce qu'on a fait).
    let deja_voilee = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } & WS_EX_LAYERED.0 as isize != 0;
    // La région D'ABORD : elle cache sur-le-champ (voir `vider_la_region`) ;
    // une fenêtre déjà visible à son évènement (Edge naît `WS_VISIBLE`) est à
    // l'écran pendant chaque aller-retour qui la précède.
    vider_la_region(hwnd);
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if deja_voilee {
            crate::diag!(
                "[ouverture] {} : son alpha lui appartient — cachée par la région et l'écran seuls",
                crate::win32::plein_ecran_zone::decrire(hwnd)
            );
            return Some(ex);
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as isize);
        if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_LAYERED.0 as isize == 0 {
            crate::diag!("[ouverture] {} : style LAYERED refusé — pas d'effet", crate::win32::plein_ecran_zone::decrire(hwnd));
            rendre_la_region(hwnd);
            return None;
        }
        if SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA).is_err() {
            crate::diag!("[ouverture] {} : alpha 0 refusé — pas d'effet", crate::win32::plein_ecran_zone::decrire(hwnd));
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
            rendre_la_region(hwnd);
            return None;
        }
        // SONDE (temporaire) : le détail du temps passé avant le voile.
        crate::diag!(
            "[sonde] voile : repond {:?}, reste {:?}",
            apres_repond,
            debut.elapsed().saturating_sub(apres_repond)
        );
        Some(ex)
    }
}

/// Les fenêtres dont on a VIDÉ la région (voir `vider_la_region`).
static REGIONS_VIDEES: Mutex<Vec<isize>> = Mutex::new(Vec::new());

/// Au plus ce délai pour qu'une fenêtre montrée se peigne avant qu'on lui
/// rende sa région (voir `vider_la_region`).
const ATTENTE_REGION: Duration = Duration::from_millis(1500);

/// ⭐ **LA RÉGION VIDE : CACHÉE À L'ÉCRAN DÈS L'AFFICHAGE.** DEMANDE EXPLICITE
/// (2026-10-24) : « je vois toujours un glitch visuel quand les fenêtres
/// apparaissent ». MESURÉ (photos de l'Explorateur, ~33 ms) : rendue
/// transparente 16 ms AVANT son affichage, elle paraît pourtant ~200 ms — grise,
/// vide, en fondu — puis disparaît, et seulement ensuite vient le vol. L'alpha
/// d'une fenêtre `LAYERED` ne vaut qu'une fois qu'elle s'est PEINTE, et
/// l'Explorateur ne pompe plus ses messages pendant qu'il se construit.
/// ⛔ `DWMWA_TRANSITIONS_FORCEDISABLED` (accepté sur une fenêtre étrangère) n'y
/// change RIEN.
///
/// Une région vide, elle, est appliquée sans que la fenêtre ait à se peindre
/// (sonde `sonde_region_vide_et_capture`) — mais WGC ne voit plus rien non plus.
/// Elle lui est donc RENDUE dès qu'elle répond à nouveau, donc qu'elle s'est
/// construite (`liberer_la_region`) : l'alpha nul la cache alors, et la capture
/// reçoit son contenu.
///
/// Une fenêtre qui a déjà SA région (cadre dessiné) n'y passe pas : on ne
/// saurait pas la lui rendre telle quelle.
fn vider_la_region(hwnd: HWND) {
    use windows::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject, GetWindowRgn, SetWindowRgn, RGN_ERROR};
    unsafe {
        let sonde = CreateRectRgn(0, 0, 0, 0);
        let a_la_sienne = GetWindowRgn(hwnd, sonde) != RGN_ERROR;
        let _ = DeleteObject(sonde.into());
        // Le système prend possession de la région passée à `SetWindowRgn`.
        if a_la_sienne || SetWindowRgn(hwnd, Some(CreateRectRgn(0, 0, 0, 0)), false) == 0 {
            return;
        }
    }
    REGIONS_VIDEES.lock().unwrap_or_else(|e| e.into_inner()).push(hwnd.0 as isize);
}

/// ⭐ **HORS DE L'ÉCRAN LE TEMPS DE SE PEINDRE.** DEMANDE EXPLICITE
/// (2026-10-25) : « vivaldi opera et edge ont encore leur fenêtre qui apparaît
/// avant l'animation ». MESURÉ (photos d'Edge) : sa région rendue, la fenêtre
/// paraît OPAQUE ~200 ms, puis disparaît — même rendue 500 ms plus tard, donc ce
/// n'est pas l'animation d'ouverture de Windows. Une fenêtre qui ne s'est pas
/// encore peinte depuis qu'elle est transparente est composée sans son alpha.
/// Elle se peint donc HORS DE L'ÉCRAN (où la capture continue, voir l'en-tête),
/// et revient à sa place au départ du vol (`ramener`) : elle s'est peinte, son
/// alpha nul vaut.
///
/// Fenêtre → (son rectangle d'origine, son rectangle visible d'origine).
static ECARTEES: Mutex<Vec<(isize, RECT, RECT)>> = Mutex::new(Vec::new());

/// La place hors de l'écran : un rectangle de la taille de la fenêtre qui ne
/// touche aucun écran mais dont l'écran le plus proche reste LE SIEN — sinon
/// Windows lui appliquerait l'échelle d'un autre écran (`WM_DPICHANGED`), et
/// elle changerait de taille. `None` si aucun bord n'en offre.
fn place_hors_ecran(rect: RECT) -> Option<(i32, i32)> {
    use windows::Win32::Graphics::Gdi::{MonitorFromRect, MONITOR_DEFAULTTONULL};
    let ecran = unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if !unsafe { GetMonitorInfoW(ecran, &mut info) }.as_bool() {
        return None;
    }
    let m = info.rcMonitor;
    let (l, h) = (rect.right - rect.left, rect.bottom - rect.top);
    const MARGE: i32 = 64;
    [
        (rect.left, m.bottom + MARGE),
        (rect.left, m.top - h - MARGE),
        (m.right + MARGE, rect.top),
        (m.left - l - MARGE, rect.top),
    ]
    .into_iter()
    .find(|&(x, y)| {
        let essai = RECT { left: x, top: y, right: x + l, bottom: y + h };
        unsafe { MonitorFromRect(&essai, MONITOR_DEFAULTTONULL) }.is_invalid()
            && unsafe { MonitorFromRect(&essai, MONITOR_DEFAULTTONEAREST) } == ecran
    })
}

/// Met la fenêtre hors de l'écran (voir `ECARTEES`) ; sans effet si elle est
/// réduite, ou si aucune place ne convient.
///
/// ⭐ **AGRANDIE AUSSI** — DEMANDE EXPLICITE (2026-10-25) : MESURÉ (Edge
/// `--start-maximized`), elle restait à l'écran ~250 ms avant son vol.
/// `SetWindowPos` la déplace sans lui ôter son état agrandi, et `ramener` la
/// remet au pixel près (vérifié au retour : `IsZoomed`, rectangle).
fn ecarter(hwnd: HWND) {
    ecarter_avec(hwnd, false);
}

/// `ecarter`, mais sans attendre la fenêtre (`SWP_ASYNCWINDOWPOS`) : rend la main
/// tout de suite. Pour `Etape::Vol`, où l'appelant fait avancer l'image — un
/// `SetWindowPos` qui attend une fenêtre occupée figerait le vol.
fn ecarter_sans_attendre(hwnd: HWND) {
    ecarter_avec(hwnd, true);
}

fn ecarter_avec(hwnd: HWND, sans_attendre: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, SWP_ASYNCWINDOWPOS, SWP_NOZORDER};
    unsafe {
        if IsIconic(hwnd).as_bool() {
            return;
        }
        let cle = hwnd.0 as isize;
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return;
        }
        // ⛔ **APPELABLE PLUSIEURS FOIS** — MESURÉ (2026-11-04) : un dialogue
        // VOILÉ par DWM (la Table des caractères, classe `#32770`) se replace
        // LUI-MÊME au centre quand DWM le dévoile, ~300 ms après qu'on l'a
        // écarté — il restait à l'écran 800 ms avant son vol. Le second appel
        // doit donc repartir de la place d'ORIGINE, pas de celle qu'on lui
        // avait donnée : c'est elle que `ramener` devra rendre.
        let deja = ECARTEES.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|e| e.0 == cle).copied();
        // Déjà hors de l'écran : rien à faire (un `SetWindowPos` par frame
        // attendrait une fenêtre occupée pour rien).
        if deja.is_some() && !touche_le_bureau(rect, bureau_virtuel()) {
            return;
        }
        let (origine, visible) = match deja {
            Some((_, o, v)) => (o, v),
            None => match rect_visible(hwnd) {
                Some(v) => (rect, v),
                None => {
                    crate::diag!("[ouverture] {cle:#x} pas écartée : rect {rect:?}, place {:?}", place_hors_ecran(rect));
                    return;
                }
            },
        };
        let Some((x, y)) = place_hors_ecran(origine) else {
            crate::diag!("[ouverture] {cle:#x} pas écartée : aucune place hors de l'écran pour {origine:?}");
            return;
        };
        crate::diag!(
            "[ouverture] {cle:#x} écartée en ({x}, {y}) depuis {origine:?} (agrandie {}, repris {})",
            windows::Win32::UI::WindowsAndMessaging::IsZoomed(hwnd).as_bool(),
            deja.is_some()
        );
        if deja.is_none() {
            ECARTEES.lock().unwrap_or_else(|e| e.into_inner()).push((cle, origine, visible));
        }
        let drapeaux = SWP_NOSIZE
            | SWP_NOZORDER
            | SWP_NOACTIVATE
            | if sans_attendre { SWP_ASYNCWINDOWPOS } else { Default::default() };
        if SetWindowPos(hwnd, None, x, y, 0, 0, drapeaux).is_err() && deja.is_none() {
            ECARTEES.lock().unwrap_or_else(|e| e.into_inner()).retain(|e| e.0 != cle);
        }
    }
}

/// **Journal CADENCÉ du vol** — au plus quatre lignes par seconde, et seulement
/// si `NEURADESK_DIAG` est active. Sert à mesurer ce que la surcouche DESSINE
/// (rectangles et opacité par partie), pas seulement la forme tirée : c'est ce
/// qui départage « la forme est fausse » de « l'image est décalée ».
static TEMOIN: Mutex<Option<Instant>> = Mutex::new(None);

fn temoin_du_vol(cle: isize, t: f32, effet: effets::Effet, parties: &[effets::Partie]) {
    if !crate::diag::enabled() {
        return;
    }
    let mut dernier = TEMOIN.lock().unwrap_or_else(|e| e.into_inner());
    if dernier.is_some_and(|d| d.elapsed() < Duration::from_millis(250)) {
        return;
    }
    *dernier = Some(Instant::now());
    // ⛔ **PAS TOUTES LES CASES** : depuis `MAXI_CASES = 20_000` (2026-11-05),
    // une dissolution en dessine 15 000 ; les écrire toutes faisait une ligne
    // d'un mégaoctet, quatre fois par seconde. On dit COMBIEN, et on montre les
    // douze premières — de quoi juger la forme et le décalage, pas le volume.
    const TEMOIN_MAX: usize = 12;
    let echantillon: Vec<_> =
        parties.iter().take(TEMOIN_MAX).map(|p| (p.pose.rect, p.pose.opacite)).collect();
    crate::diag!(
        "[ouverture] témoin {cle:#x} t={t:.2} {effet:?} : {} parties, dont {echantillon:?}",
        parties.len()
    );
}

/// Ramène à sa place une fenêtre mise hors de l'écran — sans l'attendre
/// (`SWP_ASYNCWINDOWPOS`) ; sans effet sinon.
fn ramener(hwnd: HWND) {    use windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;
    let cle = hwnd.0 as isize;
    let origine = {
        let mut ecartees = ECARTEES.lock().unwrap_or_else(|e| e.into_inner());
        let trouvee = ecartees.iter().find(|e| e.0 == cle).map(|e| e.1);
        ecartees.retain(|e| e.0 != cle);
        trouvee
    };
    if let Some(r) = origine {
        unsafe {
            let _ = SetWindowPos(hwnd, None, r.left, r.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS);
        }
    }
}

/// Rend sa région (aucune) à une fenêtre dont on l'avait vidée ; sans effet
/// sinon.
fn rendre_la_region(hwnd: HWND) {
    let cle = hwnd.0 as isize;
    let videe = {
        let mut videes = REGIONS_VIDEES.lock().unwrap_or_else(|e| e.into_inner());
        let avant = videes.len();
        videes.retain(|&h| h != cle);
        videes.len() != avant
    };
    if videe && unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        unsafe { windows::Win32::Graphics::Gdi::SetWindowRgn(hwnd, None, true) };
    }
}

/// Rend sa région à la fenêtre montrée `hwnd` dès qu'elle répond — sur un fil à
/// part : l'attente peut durer, et `SetWindowRgn` attend la fenêtre comme
/// `cacher`.
fn liberer_la_region(hwnd: isize) {
    if !REGIONS_VIDEES.lock().unwrap_or_else(|e| e.into_inner()).contains(&hwnd) {
        return;
    }
    let lance = std::thread::Builder::new().name("ouverture-region".into()).spawn(move || {
        let debut = Instant::now();
        while !repond(HWND(hwnd as _)) && debut.elapsed() < ATTENTE_REGION {}
        ecarter(HWND(hwnd as _));
        crate::diag!("[ouverture] région rendue à {hwnd:#x} après {:?}", debut.elapsed());
        rendre_la_region(HWND(hwnd as _));
    });
    if let Err(erreur) = lance {
        crate::diag!("[ouverture] fil de région de {hwnd:#x} impossible : {erreur} — rendue tout de suite");
        rendre_la_region(HWND(hwnd as _));
    }
}

/// `rendre`, pour le fil du crochet : jamais sur lui (voir `cacher`, même
/// attente sans délai). Une fenêtre invisible DOIT être rendue, quoi qu'il en
/// coûte : si elle ne répond pas, c'est un fil à part qui attend — un par
/// fenêtre figée, et le crochet reste servi.
fn rendre_sans_attendre(hwnd: isize, ex_origine: isize) {
    if ex_origine == INTERNE || ex_origine == LANCEMENT {
        // Rien de bloquant : `devoiler`/`devoiler_dll` ne font que poster.
        rendre(HWND(hwnd as _), ex_origine);
        return;
    }
    let lance = std::thread::Builder::new().name("ouverture-rendre".into()).spawn(move || rendre(HWND(hwnd as _), ex_origine));
    if let Err(erreur) = lance {
        crate::diag!("[ouverture] fil de rendu de {hwnd:#x} impossible : {erreur} — rendue sur le crochet");
        rendre(HWND(hwnd as _), ex_origine);
    }
}

/// Rend la fenêtre visible et lui retire `WS_EX_LAYERED` si elle ne l'avait pas.
fn rendre(hwnd: HWND, ex_origine: isize) {
    if ex_origine == INTERNE {
        devoiler(hwnd);
        return;
    }
    if ex_origine == LANCEMENT {
        devoiler_dll(hwnd);
        return;
    }
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return;
        }
        rendre_la_region(hwnd);
        ramener(hwnd);
        if ex_origine & WS_EX_LAYERED.0 as isize == 0 {
            // Rien de tout ceci pour une fenêtre DÉJÀ `LAYERED` à son arrivée :
            // son alpha appartient à l'application (GTK, Inkscape), et le lui
            // rendre à 255 en plein fondu la laissait bloquée sur son écran
            // d'ouverture. On ne défait donc que ce qu'on a fait — voir `cacher`.
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex & !(WS_EX_LAYERED.0 as isize));
            // ⛔ **LE CADRE SE RECALCULE** — MESURÉ (2026-11-05, Signal lancé par
            // l'utilisateur) : après l'effet, la fenêtre gardait `WS_CAPTION`,
            // n'était ni LAYERED, ni voilée, sans région — mais DWM ne dessinait
            // plus sa barre de titre. Le style a été changé pendant que
            // l'application calculait son cadre ; `SWP_FRAMECHANGED` redemande ce
            // calcul (il envoie `WM_NCCALCSIZE`).
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE
                    | SWP_NOSIZE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                    | SWP_NOACTIVATE
                    | SWP_FRAMECHANGED
                    | SWP_ASYNCWINDOWPOS,
            );
            let _ = RedrawWindow(Some(hwnd), None, None, RDW_ERASE | RDW_INVALIDATE | RDW_FRAME | RDW_ALLCHILDREN);
        }
    }
}

/// Le rectangle VISIBLE de la fenêtre (sans les bords invisibles de
/// redimensionnement) — celui de l'image capturée.
fn rect_visible(hwnd: HWND) -> Option<RECT> {
    // Hors de l'écran le temps de se peindre : sa VRAIE place (voir `ECARTEES`).
    if let Some(e) = ECARTEES.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|e| e.0 == hwnd.0 as isize) {
        return Some(e.2);
    }
    let mut rect = RECT::default();
    unsafe { DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut _ as _, std::mem::size_of::<RECT>() as u32) }
        .ok()?;
    (rect.right - rect.left >= COTE_MIN && rect.bottom - rect.top >= COTE_MIN && touche_le_bureau(rect, bureau_virtuel())).then_some(rect)
}

/// Le rectangle de tous les écrans réunis.
fn bureau_virtuel() -> RECT {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};
    unsafe {
        let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        RECT { left: x, top: y, right: x + GetSystemMetrics(SM_CXVIRTUALSCREEN), bottom: y + GetSystemMetrics(SM_CYVIRTUALSCREEN) }
    }
}

/// ⛔ **PLANTAGE DU 2026-11-01** (« j'ai cliqué sur l'applet ssh et l'app a
/// crash ») : une fenêtre montrée l'est encore, un instant, à sa place de GARAGE
/// (`cef_popup::PRELOAD_RECT`, −32000) — lue comme sa place, elle étirait la
/// surcouche sur plus de 32 000 px, au-delà de ce que D3D11 accepte (16 384) :
/// `ResizeBuffers` échouait en `E_INVALIDARG`. Une fenêtre qui ne touche aucun
/// écran n'est pas encore à sa place : on attend.
fn touche_le_bureau(r: RECT, bureau: RECT) -> bool {
    r.left < bureau.right && r.right > bureau.left && r.top < bureau.bottom && r.bottom > bureau.top
}

fn ecran_de(hwnd: HWND) -> Option<RECT> {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info) }.as_bool().then_some(info.rcMonitor)
}

/// ⭐ **LE RECTANGLE QU'UNE FENÊTRE AGRANDIE OCCUPERA** — l'écran de travail de
/// son moniteur, sans la barre des tâches ni les barres de NeuraDesk. C'est la
/// cible de `Geste::Agrandir` (voir `fermeture::animer_geste`).
pub(super) fn ecran_de_travail(hwnd: HWND) -> Option<RECT> {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info) }.as_bool().then_some(info.rcWork)
}

/// ⭐ **LE RECTANGLE QU'ELLE REPRENDRA EN SE RESTAURANT** — `rcNormalPosition`
/// de `GetWindowPlacement`, la place qu'elle occupait avant d'être agrandie.
/// `None` si la fenêtre refuse de répondre.
/// Le rectangle de repos d'une fenêtre agrandie — l'arrivée d'un « restaurer ».
///
/// ⚠️ **LA DOCUMENTATION DIT « COORDONNÉES DE L'ESPACE DE TRAVAIL »**, pas
/// écran. MESURÉ ici (2026-11-05) : l'espace de travail commence en (106, 50)
/// — barres de NeuraDesk et barre des tâches — et pourtant `rcNormalPosition`
/// vaut EXACTEMENT `GetWindowRect` sur onze fenêtres de premier plan : en
/// pratique, c'est de l'écran. ⛔ Ne pas ajouter l'origine de la zone de
/// travail sans mesure : ce serait décaler un cas qui se pose juste.
pub(super) fn rect_restaure(hwnd: HWND) -> Option<RECT> {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT};
    let mut placement = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    unsafe { GetWindowPlacement(hwnd, &mut placement) }.is_ok().then_some(placement.rcNormalPosition)
}

enum Etape {
    /// Cachée, pas encore montrée.
    Creee,
    /// Montrée, mais pas encore capturable — voilée par son application, ou
    /// capture refusée (voir `ATTENTE_VOILE`) ; retentée à chaque frame.
    Voilee { depuis: Instant },
    /// Montrée ; on attend sa première image.
    /// `derniere` : l'arrivée de la dernière image (voir `IMMOBILITE`).
    Capture { session: Capture, depuis: Instant, derniere: Option<Instant> },
    /// En vol.
    Vol { session: Capture, depart: [f32; 4], debut: Instant, effet: effets::Effet },
    /// Rendue (demandé à `demandee`) ; la surcouche la recouvre jusqu'à ce
    /// qu'elle paraisse (`visible`), puis encore un instant.
    Rendue { session: Capture, cible: [f32; 4], demandee: Instant, visible: Option<Instant> },
}

struct Fenetre {
    /// Style étendu d'origine, ou `INTERNE`.
    ex: isize,
    nee: Instant,
    etape: Etape,
    /// L'icône de départ, connue d'avance (fenêtre de NeuraDesk).
    depart: Option<[f32; 4]>,
    /// ⭐ **UNE BASCULE D'ALT+TAB** : la fenêtre n'apparaît pas, elle REVIENT
    /// au premier plan — forme et durée sont celles de la bascule, pas celles
    /// de l'ouverture (voir `Evenement::Bascule`).
    bascule: bool,
}

/// ⭐ **FIREFOX VOILE SA FENÊTRE JUSQU'À SA PREMIÈRE PEINTURE.** DEMANDE
/// EXPLICITE (2026-10-25) : « l'animation ne marche pas avec firefox ». MESURÉ :
/// sa fenêtre principale est montrée VOILÉE par DWM (`DWMWA_CLOAKED`), et la
/// capture ne s'ouvre pas sur une fenêtre voilée — elle était rendue sans vol.
/// On attend donc qu'elle se dévoile, au plus ce délai.
///
/// ⭐ **ET UNE CAPTURE REFUSÉE EST RETENTÉE** — MESURÉ (2026-10-25, sélecteur de
/// profil de Firefox, « il apparaît sans l'animation ») : ni voilée, ni réduite,
/// rectangle valide, mais la capture refusée — montrée 20 ms AVANT qu'on lui
/// rende sa région (`liberer_la_region`). Elle était rendue sans vol.
const ATTENTE_VOILE: Duration = Duration::from_secs(3);

/// Les fenêtres qu'on a tenté de réécarter récemment, et quand (voir
/// `garder_hors_ecran`).
static TENTATIVES: Mutex<Vec<(isize, Instant)>> = Mutex::new(Vec::new());

/// ⛔ **LA FENÊTRE SE REMET À SA PLACE TOUTE SEULE.** MESURÉ (2026-11-04, photos
/// de la Table des caractères) : écartée à son affichage, elle revenait à sa
/// place ~300 ms plus tard — au moment précis où DWM la dévoilait — et restait
/// À L'ÉCRAN plus d'une seconde avant son vol. C'est le clignotement qu'on
/// cherchait. On vérifie donc à chaque frame, tant que la fenêtre n'a pas pris
/// son vol, qu'elle ne touche encore aucun écran ; `ecarter` garde son
/// rectangle d'origine et ne redéplace rien quand elle est déjà hors de vue.
///
/// Au plus une tentative toutes les `REVEIL` : une fenêtre qu'aucune place
/// n'écarte ne doit pas envoyer un `SetWindowPos` soixante fois par seconde.
fn garder_hors_ecran(hwnd: HWND, ex: isize) {
    if a_reecarter(hwnd, ex) && repond(hwnd) {
        ecarter(hwnd);
    }
}

/// La DÉCISION seule (voir `garder_hors_ecran`) : la fenêtre touche-t-elle encore
/// l'écran, et ne l'a-t-on pas déjà tenté depuis `REVEIL` ? Enregistre la
/// tentative quand elle répond oui — l'appelant peut alors agir SANS attendre
/// (voir `Etape::Vol`, où l'écarter sur place figerait le vol).
fn a_reecarter(hwnd: HWND, ex: isize) -> bool {
    // ⛔ Jamais pour une fenêtre de NeuraDesk (INTERNE) ni pour un lancement
    // voilé par la DLL (LANCEMENT) : le voile la cache déjà, la déplacer hors
    // de l'écran ne ferait que la secouer pour rien.
    if ex == INTERNE || ex == LANCEMENT || unsafe { IsIconic(hwnd) }.as_bool() {
        return false;
    }
    let mut r = RECT::default();
    if unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut r) }.is_err()
        || !touche_le_bureau(r, bureau_virtuel())
    {
        return false;
    }
    let cle = hwnd.0 as isize;
    let maintenant = Instant::now();
    let mut tentatives = TENTATIVES.lock().unwrap_or_else(|e| e.into_inner());
    tentatives.retain(|(_, d)| maintenant - *d < Duration::from_secs(60));
    if tentatives.iter().any(|(h, d)| *h == cle && maintenant - *d < REVEIL) {
        return false;
    }
    tentatives.push((cle, maintenant));
    true
}

/// Ouvre la capture d'une fenêtre qui vient d'être montrée ; sinon la rend, et
/// rend `false`. Une fenêtre étrangère encore voilée attend (`Etape::Voilee`).
fn commencer_la_capture(gfx: &GfxDevice, hwnd: HWND, fenetre: &mut Fenetre) -> bool {
    // ⭐ Un lancement voilé par la DLL EST cloaké à dessein : on capture à
    // travers le voile (`PrintWindow` le lit — mesuré), sans l'attendre comme
    // une fenêtre voilée par son application (Firefox).
    if fenetre.ex != INTERNE && fenetre.ex != LANCEMENT && est_masquee_par_dwm(hwnd) {
        if !matches!(fenetre.etape, Etape::Voilee { .. }) {
            crate::diag!("[ouverture] montrée {:#x} mais voilée : attend son dévoilement", hwnd.0 as isize);
            fenetre.etape = Etape::Voilee { depuis: Instant::now() };
        }
        return true;
    }
    // ⛔ **UNE RÉGION VIDÉE NE REND QU'UN APLAT** — MESURÉ (2026-11-04) : la
    // région découpe le rendu, donc `PrintWindow` ne lit plus que le vide. La
    // capture attend donc que `liberer_la_region` la lui ait rendue — c'est le
    // comportement qu'avait `wgc`, qui REFUSAIT la fenêtre.
    if REGIONS_VIDEES.lock().unwrap_or_else(|e| e.into_inner()).contains(&(hwnd.0 as isize)) {
        if !matches!(fenetre.etape, Etape::Voilee { .. }) {
            crate::diag!("[ouverture] montrée {:#x} avec sa région vidée : attend qu'elle lui soit rendue", hwnd.0 as isize);
            fenetre.etape = Etape::Voilee { depuis: Instant::now() };
        }
        return true;
    }
    // ⭐ **LA CAPTURE NE PASSE PLUS PAR `Windows.Graphics.Capture`** — DEMANDE
    // EXPLICITE (2026-11-04) : « je voudrais capturer les fenêtres pour éviter
    // l'effet de clignotement quand on ajoute des effets ». MESURÉ (sonde
    // `win32::capture_probe`) : `PrintWindow` rend le contenu d'une fenêtre sous
    // alpha nul ET hors de l'écran — exactement l'état où l'effet la tient —
    // en 10 à 40 ms, sans session à ouvrir (600 ms sur un Chromium), sans
    // première image NOIRE, et sans les 15 à 70 Mo de mémoire GPU que laisse
    // chaque session fermée (voir `wgc`). Le suivi reste soumis au même
    // `rect_visible`, qui écarte une fenêtre garée hors du bureau.
    let session = (!unsafe { IsIconic(hwnd) }.as_bool() && rect_visible(hwnd).is_some())
        .then(|| Capture::start(hwnd))
        .flatten();
    // Une seule ligne par fenêtre, pas une par nouvel essai.
    let en_attente = matches!(fenetre.etape, Etape::Voilee { .. });
    if session.is_some() || !en_attente {
        crate::diag!(
            "[ouverture] montrée {:#x}, capture {}{} (réduite {}, rect {:?})",
            hwnd.0 as isize,
            session.is_some(),
            if en_attente { " après attente" } else { "" },
            unsafe { IsIconic(hwnd) }.as_bool(),
            rect_visible(hwnd)
        );
    }
    match session {
        // ⛔ Le délai part d'ICI, pas de l'évènement d'affichage — MESURÉ : quand
        // la capture passait par `wgc`, `CaptureSession::start` prenait plus de
        // 600 ms sur une fenêtre d'Edge, et l'attente était déjà épuisée avant
        // de commencer. `PrintWindow`, lui, n'ouvre rien.
        Some(session) => {
            fenetre.etape = Etape::Capture { session, depuis: Instant::now(), derniere: None };
            true
        }
        // Une fenêtre étrangère montrée dont la capture est refusée : retentée
        // (voir `ATTENTE_VOILE`).
        None if fenetre.ex != INTERNE && !unsafe { IsIconic(hwnd) }.as_bool() && unsafe { IsWindow(Some(hwnd)) }.as_bool() => {
            if !matches!(fenetre.etape, Etape::Voilee { .. }) {
                fenetre.etape = Etape::Voilee { depuis: Instant::now() };
            }
            true
        }
        None => {
            demander_rendu(hwnd, fenetre.ex);
            false
        }
    }
}

/// Le vol à l'instant `t` (0 → 1) : rectangle (x, y, largeur, hauteur) et angle.
/// Le départ est l'icône, l'arrivée la fenêtre ; la rotation ralentit en
/// approchant, et s'annule à l'arrivée.
fn vol(depart: [f32; 4], arrivee: [f32; 4], t: f32) -> ([f32; 4], f32) {
    let t = t.clamp(0.0, 1.0);
    let e = 1.0 - (1.0 - t).powi(3);
    let centre = |r: [f32; 4]| (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    let (ax, ay) = centre(depart);
    let (bx, by) = centre(arrivee);
    let (cx, cy) = (ax + (bx - ax) * e, ay + (by - ay) * e);
    let (l, h) = (depart[2] + (arrivee[2] - depart[2]) * e, depart[3] + (arrivee[3] - depart[3]) * e);
    let angle = -(1.0 - e) * TOURS * std::f32::consts::TAU;
    ([cx - l / 2.0, cy - h / 2.0, l, h], angle)
}

/// L'image capturée est-elle encore VIDE (noir pur au centre) ? — MESURÉ
/// (2026-10-16, photos du vol du Bloc-notes) : la première image arrive avant
/// que l'application ne se soit dessinée, et un rectangle noir volait la
/// première moitié du trajet. Un thème sombre n'est jamais du noir PUR.
fn image_vide(gfx: &GfxDevice, srv: Option<&ID3D11ShaderResourceView>) -> bool {
    use windows::Win32::Graphics::Direct3D11::*;
    let Some(srv) = srv else { return true };
    unsafe {
        let Ok(ressource) = srv.GetResource() else { return false };
        let Ok(texture) = windows::core::Interface::cast::<ID3D11Texture2D>(&ressource) else { return false };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);
        if desc.Width < SONDE_VIDE || desc.Height < SONDE_VIDE {
            return false;
        }
        let sonde = D3D11_TEXTURE2D_DESC {
            Width: SONDE_VIDE,
            Height: SONDE_VIDE,
            MipLevels: 1,
            ArraySize: 1,
            Format: desc.Format,
            SampleDesc: desc.SampleDesc,
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut copie = None;
        if gfx.device.CreateTexture2D(&sonde, None, Some(&mut copie)).is_err() {
            return false;
        }
        let Some(copie) = copie else { return false };
        let (x, y) = ((desc.Width - SONDE_VIDE) / 2, (desc.Height - SONDE_VIDE) / 2);
        let zone = D3D11_BOX { left: x, top: y, front: 0, right: x + SONDE_VIDE, bottom: y + SONDE_VIDE, back: 1 };
        gfx.context.CopySubresourceRegion(&copie, 0, 0, 0, 0, &texture, 0, Some(&zone));
        let mut lu = D3D11_MAPPED_SUBRESOURCE::default();
        if gfx.context.Map(&copie, 0, D3D11_MAP_READ, 0, Some(&mut lu)).is_err() {
            return false;
        }
        let vide = (0..SONDE_VIDE as usize).all(|ligne| {
            let pixels = std::slice::from_raw_parts((lu.pData as *const u8).add(ligne * lu.RowPitch as usize), SONDE_VIDE as usize * 4);
            pixels.chunks_exact(4).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0)
        });
        gfx.context.Unmap(&copie, 0);
        vide
    }
}

/// L'image capturée n'est-elle qu'un APLAT (le fond de l'application, pas encore
/// son contenu) ? Relève une grille de `GRILLE_UNIFORME` × `GRILLE_UNIFORME`
/// points sur toute l'image : un aplat n'y a aucun écart de plus de
/// `ECART_CONTENU`. La barre de titre compte — le Bloc-notes, dont le document
/// est vide, n'attend pas.
fn image_uniforme(gfx: &GfxDevice, srv: Option<&ID3D11ShaderResourceView>, taille: (u32, u32)) -> bool {
    use windows::Win32::Graphics::Direct3D11::*;
    let Some(srv) = srv else { return false };
    let (largeur, hauteur) = taille;
    unsafe {
        let Ok(ressource) = srv.GetResource() else { return false };
        let Ok(texture) = windows::core::Interface::cast::<ID3D11Texture2D>(&ressource) else { return false };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);
        let (largeur, hauteur) = (largeur.min(desc.Width), hauteur.min(desc.Height));
        if largeur < GRILLE_UNIFORME || hauteur < GRILLE_UNIFORME {
            return false;
        }
        let sonde = D3D11_TEXTURE2D_DESC {
            Width: GRILLE_UNIFORME,
            Height: GRILLE_UNIFORME,
            MipLevels: 1,
            ArraySize: 1,
            Format: desc.Format,
            SampleDesc: desc.SampleDesc,
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut copie = None;
        if gfx.device.CreateTexture2D(&sonde, None, Some(&mut copie)).is_err() {
            return false;
        }
        let Some(copie) = copie else { return false };
        // Le centre de chaque case de la grille.
        for j in 0..GRILLE_UNIFORME {
            for i in 0..GRILLE_UNIFORME {
                let x = (2 * i + 1) * largeur / (2 * GRILLE_UNIFORME);
                let y = (2 * j + 1) * hauteur / (2 * GRILLE_UNIFORME);
                let point = D3D11_BOX { left: x, top: y, front: 0, right: x + 1, bottom: y + 1, back: 1 };
                gfx.context.CopySubresourceRegion(&copie, 0, i, j, 0, &texture, 0, Some(&point));
            }
        }
        let mut lu = D3D11_MAPPED_SUBRESOURCE::default();
        if gfx.context.Map(&copie, 0, D3D11_MAP_READ, 0, Some(&mut lu)).is_err() {
            return false;
        }
        let mut pixels = Vec::with_capacity((GRILLE_UNIFORME * GRILLE_UNIFORME) as usize);
        for ligne in 0..GRILLE_UNIFORME as usize {
            let rang = std::slice::from_raw_parts((lu.pData as *const u8).add(ligne * lu.RowPitch as usize), GRILLE_UNIFORME as usize * 4);
            pixels.extend(rang.chunks_exact(4).map(|p| [p[0], p[1], p[2]]));
        }
        gfx.context.Unmap(&copie, 0);
        est_un_aplat(&pixels)
    }
}

/// Aucun canal ne varie de plus de `ECART_CONTENU` entre ces pixels.
fn est_un_aplat(pixels: &[[u8; 3]]) -> bool {
    (0..3).all(|canal| {
        let (min, max) = pixels.iter().fold((u8::MAX, u8::MIN), |(a, b), p| (a.min(p[canal]), b.max(p[canal])));
        max.saturating_sub(min) <= ECART_CONTENU
    })
}

/// Sans icône, la fenêtre part de son propre centre, au dixième de sa taille.
fn depart_au_centre(cible: [f32; 4]) -> [f32; 4] {
    let (l, h) = (cible[2] / 10.0, cible[3] / 10.0);
    [cible[0] + (cible[2] - l) / 2.0, cible[1] + (cible[3] - h) / 2.0, l, h]
}

fn en_f32(r: RECT) -> [f32; 4] {
    [r.left as f32, r.top as f32, (r.right - r.left) as f32, (r.bottom - r.top) as f32]
}

/// L'effet, porté par son fil (voir `installer`).
struct EffetOuverture {
    fenetres: HashMap<isize, Fenetre>,
    surcouche: Option<Surcouche>,
    fermetures: fermeture::Fermetures,
}

/// Une image à dessiner dans la surcouche.
struct Image<'a> {
    srv: &'a ID3D11ShaderResourceView,
    rect: [f32; 4],
    angle: f32,
    /// L'écran de la fenêtre (la surcouche le couvre).
    ecran: RECT,
    /// 1 : opaque ; moins pendant un fondu (voir `fondu_actif`).
    opacite: f32,
    /// Le sous-rectangle de la texture montré par cette image (`(u0, v0, u1,
    /// v1)`), et le rayon de ses coins, en pixels : une fenêtre entière d'un
    /// seul quadrilatère pour la plupart des effets, une TUILE (« éclatement »)
    /// ou une BANDE (« génie ») pour ceux qui découpent — voir `effets`).
    uv: [f32; 4],
    rayon: f32,
}

impl<'a> Image<'a> {
    fn de_la_fenetre(session: &'a Capture, rect: [f32; 4], angle: f32, hwnd: HWND) -> Option<Self> {
        let echelle = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
        Some(Self {
            srv: session.srv()?,
            rect,
            angle,
            ecran: ecran_de(hwnd)?,
            opacite: 1.0,
            uv: UV_PLEINE,
            rayon: RAYON_FENETRE * echelle,
        })
    }

    fn avec_opacite(self, opacite: f32) -> Self {
        Self { opacite, ..self }
    }

    /// Les images d'un effet qui DÉCOUPE la texture (« éclatement », « génie »,
    /// …) : une par partie, chacune avec son sous-rectangle `uv`.
    fn de_parties(session: &'a Capture, hwnd: HWND, parties: Vec<effets::Partie>) -> Vec<Self> {
        let Some(srv) = session.srv() else { return Vec::new() };
        let Some(ecran) = ecran_de(hwnd) else { return Vec::new() };
        parties
            .into_iter()
            .map(|partie| Self {
                srv,
                rect: partie.pose.rect,
                angle: partie.pose.angle,
                ecran,
                opacite: partie.pose.opacite,
                uv: partie.uv,
                rayon: partie.rayon,
            })
            .collect()
    }
}

impl EffetOuverture {
    fn new() -> Self {
        Self { fenetres: HashMap::new(), surcouche: None, fermetures: fermeture::Fermetures::new() }
    }

    /// Vrai tant qu'une image est en vol ou recouvre sa fenêtre — ou qu'une
    /// fenêtre de NeuraDesk attend d'être montrée (aucun évènement ne le dira).
    fn anime(&self) -> bool {
        self.fenetres.values().any(|f| !matches!(f.etape, Etape::Creee) || f.ex == INTERNE) || self.fermetures.anime()
    }

    /// Une frame.
    fn tick(&mut self, gfx: &GfxDevice) {
        let evenements = relever();
        let maintenant = Instant::now();
        for evenement in evenements {
            match evenement {
                Evenement::Cachee { hwnd, ex } => {
                    crate::diag!(
                        "[ouverture] cachée {hwnd:#x} {} (visible {})",
                        crate::win32::plein_ecran_zone::decrire(HWND(hwnd as _)),
                        unsafe { IsWindowVisible(HWND(hwnd as _)) }.as_bool()
                    );
                    self.fenetres.insert(hwnd, Fenetre { ex, nee: maintenant, etape: Etape::Creee, depart: None, bascule: false });
                }
                Evenement::Interne { hwnd, depart } => {
                    self.fermetures.internes.insert(hwnd);
                    // Rouverte pendant son propre vol : le vol repart.
                    self.fenetres.insert(hwnd, Fenetre { ex: INTERNE, nee: maintenant, etape: Etape::Creee, depart, bascule: false });
                }
                Evenement::Montree { hwnd, ex } => {
                    if let Some(ex) = ex {
                        self.fenetres.insert(hwnd, Fenetre { ex, nee: maintenant, etape: Etape::Creee, depart: None, bascule: false });
                    }
                    let Some(fenetre) = self.fenetres.get_mut(&hwnd) else { continue };
                    if !matches!(fenetre.etape, Etape::Creee) {
                        continue;
                    }
                    if !commencer_la_capture(gfx, HWND(hwnd as _), fenetre) {
                        self.fenetres.remove(&hwnd);
                    }
                }
                Evenement::Bascule { hwnd, ex } => {
                    // Une fenêtre DÉJÀ en vol (ouverture en cours) garde son
                    // animation : on ne la cache pas deux fois.
                    if self.fenetres.contains_key(&hwnd) {
                        continue;
                    }
                    self.fenetres.insert(
                        hwnd,
                        Fenetre { ex, nee: maintenant, etape: Etape::Creee, depart: None, bascule: true },
                    );
                    let Some(fenetre) = self.fenetres.get_mut(&hwnd) else { continue };
                    if commencer_la_capture(gfx, HWND(hwnd as _), fenetre) {
                        crate::diag!("[bascule] {hwnd:#x} capturée : elle revient en grandissant");
                    } else {
                        self.fenetres.remove(&hwnd);
                    }
                }
                Evenement::Detruite(hwnd) => {
                    self.fenetres.remove(&hwnd);
                    self.fermetures.oublier(hwnd);
                    oublier_fondu_de_fenetre(hwnd);
                }
                Evenement::Oublier(hwnd) => {
                    self.fermetures.oublier(hwnd);
                    oublier_fondu_de_fenetre(hwnd);
                }
                Evenement::Masquee(hwnd) => {
                    // Une fenêtre encore en vol d'ouverture n'a pas d'image à rendre.
                    if !self.fenetres.contains_key(&hwnd) {
                        self.fermetures.masquee(hwnd);
                    }
                }
                Evenement::Geste { hwnd, geste, retenue_ms } => {
                    // Une fenêtre encore en vol d'ouverture : son geste passe
                    // par le rendu normal, pas par un second effet.
                    if !self.fenetres.contains_key(&hwnd) {
                        self.fermetures.animer_geste(gfx, HWND(hwnd as _), geste, retenue_ms);
                    }
                }
                Evenement::Lancement { hwnd } => {
                    // Déjà suivie (évènement ré-émis) : on ne relance pas.
                    if self.fenetres.contains_key(&hwnd) {
                        continue;
                    }
                    let h = HWND(hwnd as _);
                    self.fenetres.insert(
                        hwnd,
                        Fenetre { ex: LANCEMENT, nee: maintenant, etape: Etape::Creee, depart: None, bascule: false },
                    );
                    let Some(fenetre) = self.fenetres.get_mut(&hwnd) else { continue };
                    if commencer_la_capture(gfx, h, fenetre) {
                        crate::diag!("[ouverture] lancement {hwnd:#x} : capture à travers le voile de la DLL");
                    } else {
                        // Capture impossible : on dévoile, la DLL ayant voilé en vain.
                        devoiler_dll(h);
                        self.fenetres.remove(&hwnd);
                    }
                }
            }
        }
        // La fermeture : suit le premier plan, avance les retours.
        let occupe = !self.fenetres.is_empty();
        let fenetres = &self.fenetres;
        self.fermetures.tick(gfx, occupe, &|h| fenetres.contains_key(&h));
        let surveillees = self.fermetures.surveillees();
        let mut publiees = SURVEILLEES.lock().unwrap_or_else(|e| e.into_inner());
        if *publiees != surveillees {
            *publiees = surveillees;
        }
        drop(publiees);

        let mut finies = Vec::new();
        for (&cle, fenetre) in self.fenetres.iter_mut() {
            let hwnd = HWND(cle as _);
            // Une fenêtre MONTÉE qui s'est remise à sa place avant son vol est
            // réécartée (voir `garder_hors_ecran`). Pas avant son affichage :
            // une fenêtre de service jamais montrée n'a pas à être déplacée.
            //
            // ⛔ **ET PENDANT LE VOL** — SIGNALÉ (2026-11-05) : « la vraie
            // fenêtre reste visible pendant tout l'effet ». La fenêtre qui se
            // replace ne le fait pas qu'avant son vol : Notepad++ restaure sa
            // position de session APRÈS s'être montré, donc APRÈS qu'on l'a
            // écartée. Rien ne la réécartait pendant le vol (seules les étapes
            // `Voilee` et `Capture` y passaient), et elle restait à l'écran sous
            // son image jusqu'à `rendre`. `rendre_la_region`, qui lui rend sa
            // région pour la capture, la remet aussi en jeu.
            match &fenetre.etape {
                Etape::Voilee { .. } | Etape::Capture { .. } => garder_hors_ecran(hwnd, fenetre.ex),
                // Pendant le vol, écarter SUR PLACE figerait l'image : `repond`
                // puis `ecarter` attendent la fenêtre, et c'est elle qui bouge
                // — jusqu'à une seconde. Le geste part donc sur un fil, et le
                // vol continue d'avancer.
                Etape::Vol { .. } if a_reecarter(hwnd, fenetre.ex) => {
                    // Sans attendre : le vol s'avance sur CE fil, et `rendre`
                    // (appelé un peu plus bas quand le vol s'achève) poste son
                    // propre `SetWindowPos` APRÈS celui-ci — l'ordre est donc
                    // bon, et la fenêtre n'est jamais garée après avoir été
                    // rendue.
                    let cle = hwnd.0 as isize;
                    crate::diag!("[ouverture] {cle:#x} revenue à l'écran pendant son vol : réécartée");
                    ecarter_sans_attendre(hwnd);
                }
                _ => {}
            }
            let etape = std::mem::replace(&mut fenetre.etape, Etape::Creee);
            fenetre.etape = match etape {
                // Une fenêtre de NeuraDesk ne passe pas par le crochet : on guette
                // son affichage ici (voir `anime`, qui tient le fil éveillé).
                Etape::Creee if fenetre.ex == INTERNE && unsafe { IsWindowVisible(hwnd) }.as_bool() => {
                    if !commencer_la_capture(gfx, hwnd, fenetre) {
                        finies.push(cle);
                    }
                    std::mem::replace(&mut fenetre.etape, Etape::Creee)
                }
                Etape::Creee if fenetre.ex == INTERNE && maintenant - fenetre.nee > ATTENTE_AFFICHAGE_INTERNE => {
                    demander_rendu(hwnd, fenetre.ex);
                    finies.push(cle);
                    Etape::Creee
                }
                Etape::Creee if maintenant - fenetre.nee > ATTENTE_AFFICHAGE => {
                    crate::diag!("[ouverture] jamais montrée {cle:#x} : rendue");
                    demander_rendu(hwnd, fenetre.ex);
                    finies.push(cle);
                    Etape::Creee
                }
                Etape::Creee => Etape::Creee,
                Etape::Voilee { depuis } if maintenant - depuis > ATTENTE_VOILE => {
                    crate::diag!("[ouverture] {cle:#x} toujours voilée : rendue");
                    demander_rendu(hwnd, fenetre.ex);
                    finies.push(cle);
                    Etape::Creee
                }
                Etape::Voilee { depuis } => {
                    fenetre.etape = Etape::Voilee { depuis };
                    if !commencer_la_capture(gfx, hwnd, fenetre) {
                        finies.push(cle);
                    }
                    std::mem::replace(&mut fenetre.etape, Etape::Creee)
                }
                Etape::Capture { mut session, depuis, derniere } => {
                    let nouvelle = session.poll(&gfx.device);
                    let derniere = if nouvelle { Some(maintenant) } else { derniere };
                    let attendu = contenu_attendu(fenetre.ex, maintenant - depuis, derniere.map(|d| maintenant - d));
                    // ⭐ DEMANDE EXPLICITE (2026-10-17) : « il y a toujours une image
                    // fantôme des programmes qui apparaît à l'ouverture ». MESURÉ
                    // (photos d'Edge) : la première image est un APLAT — brun, puis
                    // blanc — et la page n'arrive qu'une seconde plus tard ; c'est cet
                    // aplat qui volait. Il attend donc son contenu (`ATTENTE_CONTENU`).
                    let prete = session.srv().is_some()
                        && (nouvelle || attendu)
                        && !image_vide(gfx, session.srv())
                        && (attendu || !image_uniforme(gfx, session.srv(), session.size()));
                    if prete {
                        let depart = rect_visible(hwnd)
                            .map(en_f32)
                            .map(|cible| fenetre.depart.or_else(|| icone_de(hwnd)).unwrap_or_else(|| depart_au_centre(cible)));
                        // ⛔ **PAS DE `ramener` ICI.** MESURÉ (2026-11-05,
                        // Inkscape / GTK, fenêtre `LAYERED` par l'application) :
                        // la ramener au début du vol la REMET À L'ÉCRAN pendant
                        // toute l'animation — on voit la vraie fenêtre, puis
                        // l'effet par-dessus, comme s'il arrivait après. Pire :
                        // `ramener` est ASYNCHRONE (`SWP_ASYNCWINDOWPOS`), la
                        // fenêtre restait garée le temps que l'application
                        // pompe, `rect_visible` ne la trouvait plus sur aucun
                        // écran et le vol s'ABANDONNAIT (`cible` nulle).
                        // Elle reste donc garée jusqu'à `rendre`, qui la ramène
                        // — et `est_rendue` attend qu'elle soit vraiment revenue.
                        match depart {
                            Some(depart) => {
                                // ⛔ **LE VOILE EST VÉRIFIÉ AU DÉPART DU VOL.**
                                // SIGNALÉ (2026-11-05) : « la vraie fenêtre se
                                // voit » pendant l'animation d'un panneau.
                                // `animer_interne` voile à la création, mais
                                // rien ne vérifie ensuite que le voile tient :
                                // ce contrôle (`est_masquee_par_dwm`) est sauté
                                // pour NOS fenêtres dans `commencer_la_capture`,
                                // et `DwmSetWindowAttribute` peut répondre OK
                                // sans que DWM voile. On le repose, et on le dit.
                                if fenetre.ex == INTERNE && !est_masquee_par_dwm(hwnd) {
                                    crate::diag!("[ouverture] {cle:#x} paraissait sous son image : revoilée");
                                    masquer_par_dwm(hwnd, true);
                                }
                                self.fermetures.origines.insert(cle, depart);
                                // ⛔ La forme est TIRÉE ICI, une fois pour ce
                                // vol : avec « au hasard », elle ne doit pas
                                // changer d'une frame à l'autre.
                                let effet = if fenetre.bascule {
                                    effets::Effet::Bascule
                                } else {
                                    effet_du_moment(Moment::Ouverture)
                                };
                                crate::diag!("[ouverture] envol {cle:#x} depuis {depart:?}, forme {effet:?}, après {:?}", maintenant - depuis);
                                Etape::Vol {
                                    session,
                                    depart,
                                    debut: maintenant,
                                    effet,
                                }
                            }
                            None => {
                                demander_rendu(hwnd, fenetre.ex);
                                finies.push(cle);
                                Etape::Creee
                            }
                        }
                    } else if maintenant - depuis > ATTENTE_IMAGE && image_vide(gfx, session.srv()) {
                        crate::diag!("[ouverture] pas d'image pour {} : rendue sans effet", crate::win32::plein_ecran_zone::decrire(hwnd));
                        demander_rendu(hwnd, fenetre.ex);
                        finies.push(cle);
                        Etape::Creee
                    } else {
                        Etape::Capture { session, depuis, derniere }
                    }
                }
                Etape::Vol { mut session, depart, debut, effet } => {
                    session.poll(&gfx.device);
                    // Le voile tient-il toujours ? Une fenêtre de NeuraDesk
                    // dévoilée par un `ShowWindow` pendant son vol paraîtrait
                    // sous sa propre image : on le lui rend à chaque image.
                    if fenetre.ex == INTERNE && !est_masquee_par_dwm(hwnd) {
                        masquer_par_dwm(hwnd, true);
                    }
                    let cible = rect_visible(hwnd).map(en_f32);
                    match cible {
                        Some(cible) if maintenant - debut >= duree_du_vol(fenetre, cle) => {
                            demander_rendu(hwnd, fenetre.ex);
                            Etape::Rendue { session, cible, demandee: maintenant, visible: None }
                        }
                        Some(_) => Etape::Vol { session, depart, debut, effet },
                        None => {
                            demander_rendu(hwnd, fenetre.ex);
                            finies.push(cle);
                            Etape::Creee
                        }
                    }
                }
                Etape::Rendue { session, cible, demandee, visible } => {
                    let visible = visible.or_else(|| est_rendue(hwnd, fenetre.ex).then_some(maintenant));
                    let recouverte = visible.is_some_and(|depuis| maintenant - depuis > RECOUVREMENT);
                    if recouverte || maintenant - demandee > ATTENTE_RENDU {
                        finies.push(cle);
                    }
                    Etape::Rendue { session, cible, demandee, visible }
                }
            };
        }
        for cle in finies {
            // Une fenêtre qui vient de se poser passe en tête de file pour
            // l'effet de fermeture (voir `fermeture::adopter`).
            if let Some(Fenetre { etape: Etape::Rendue { .. }, .. }) = self.fenetres.remove(&cle) {
                self.fermetures.adopter(HWND(cle as _));
            }
        }
        // Ce que le crochet doit éviter de cacher une seconde fois.
        *EN_VOL.lock().unwrap_or_else(|e| e.into_inner()) = self.fenetres.keys().copied().collect();

        // Ce que la surcouche dessine : une image par fenêtre en vol ou rendue.
        let mut images: Vec<Image> = self.fermetures.images();
        for (&cle, fenetre) in &self.fenetres {
            let hwnd = HWND(cle as _);
            match &fenetre.etape {
                Etape::Vol { session, depart, debut, effet } => {
                    let Some(cible) = rect_visible(hwnd).map(en_f32) else { continue };
                    let t = (maintenant - *debut).as_secs_f32() / duree_du_vol(fenetre, cle).as_secs_f32();
                    if en_fondu(cle, fenetre.ex) {
                        images.extend(Image::de_la_fenetre(session, cible, 0.0, hwnd).map(|i| i.avec_opacite(opacite_du_fondu(t))));
                    } else if *effet == effets::Effet::Retour {
                        // Le tourbillon historique (une vrille pendant le vol) :
                        // c'est LUI la forme « vers l'icône » à l'ouverture.
                        let (rect, angle) = vol(*depart, cible, t);
                        images.extend(Image::de_la_fenetre(session, rect, angle, hwnd));
                    } else {
                        // ⭐ **L'OUVERTURE EST LA FERMETURE À L'ENVERS** : à
                        // `1 - t`, la forme part de la fenêtre intacte (t=1) et
                        // finit à l'icône (t=0). Elle se pose donc exactement
                        // sur la fenêtre à la fin du vol, sans à-coup.
                        let echelle = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
                        let mut parties = effet.parties(1.0 - t, cible, *depart, RAYON_FENETRE * echelle);
                        // ⛔ Une forme dont l'opacité ne sert qu'à DISPARAÎTRE
                        // rendrait, rejouée à l'envers, une fenêtre fantôme
                        // pendant toute l'ouverture (voir `opaque_a_l_ouverture`).
                        if effet.opaque_a_l_ouverture() {
                            for partie in &mut parties {
                                partie.pose.opacite = 1.0;
                            }
                        }
                        temoin_du_vol(cle, t, *effet, &parties);
                        images.extend(Image::de_parties(session, hwnd, parties));
                    }
                }
                Etape::Rendue { session, cible, .. } => images.extend(Image::de_la_fenetre(session, *cible, 0.0, hwnd)),
                _ => {}
            }
        }
        if images.is_empty() {
            if let Some(surcouche) = &mut self.surcouche {
                surcouche.cacher();
            }
            return;
        }
        if self.surcouche.is_none() {
            match Surcouche::new(gfx) {
                Ok(surcouche) => self.surcouche = Some(surcouche),
                Err(erreur) => {
                    crate::diag!("[ouverture] surcouche impossible : {erreur:?}");
                    return;
                }
            }
        }
        if let Some(surcouche) = &mut self.surcouche {
            if let Err(erreur) = surcouche.dessiner(gfx, &images) {
                crate::diag!("[ouverture] dessin impossible : {erreur:?}");
            }
        }
    }

    /// À l'arrêt du fil : aucune fenêtre ne doit rester invisible.
    fn rendre_tout(&mut self) {
        for evenement in relever() {
            match evenement {
                Evenement::Cachee { hwnd, ex } | Evenement::Montree { hwnd, ex: Some(ex) } => rendre(HWND(hwnd as _), ex),
                Evenement::Interne { hwnd, .. } => rendre(HWND(hwnd as _), INTERNE),
                _ => {}
            }
        }
        for (cle, fenetre) in self.fenetres.drain() {
            if !matches!(fenetre.etape, Etape::Rendue { .. }) {
                rendre(HWND(cle as _), fenetre.ex);
            }
        }
        if let Some(surcouche) = &mut self.surcouche {
            surcouche.cacher();
        }
    }
}

impl Drop for EffetOuverture {
    fn drop(&mut self) {
        self.rendre_tout();
    }
}

/// La fenêtre transparente où vole l'image : au-dessus de tout, traversée par la
/// souris, sur l'écran de la fenêtre qui s'ouvre (et l'icône d'où elle part).
struct Surcouche {
    hwnd: HWND,
    swapchain: Option<SwapChain>,
    renderer: QuadRenderer,
    visible: bool,
}

impl Surcouche {
    fn new(gfx: &GfxDevice) -> Result<Self> {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None)?.into();
            let classe = w!("NeuraDeskOuvertureFenetre");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                lpszClassName: classe,
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let ex = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT;
            let hwnd = CreateWindowExW(ex, classe, w!("NeuraDesk"), WS_POPUP, 0, 0, 1, 1, None, None, Some(instance), None)?;
            let marges = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &marges);
            // Sans les transitions de DWM (fondu à l'affichage et au masquage) : la
            // surcouche doit paraître et disparaître d'un coup, sur l'image exacte de
            // la fenêtre qu'elle relaie. ⚠️ Comparée image par image (duplication du
            // bureau, Edge, 2026-10-17), la différence n'est PAS mesurable.
            let sans_transition: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_TRANSITIONS_FORCEDISABLED,
                &sans_transition as *const _ as _,
                std::mem::size_of::<i32>() as u32,
            );
            crate::gfx::capture_visibility::apply_to_window(hwnd);
            Ok(Self { hwnd, swapchain: None, renderer: QuadRenderer::new(&gfx.device)?, visible: false })
        }
    }

    fn dessiner(&mut self, gfx: &GfxDevice, images: &[Image]) -> Result<()> {
        // L'écran de chaque fenêtre, et chaque icône de départ : l'union est la
        // surcouche (deux écrans seulement si deux fenêtres s'ouvrent sur deux
        // écrans à la fois).
        let mut zone: Option<RECT> = None;
        let mut etendre = |r: RECT| {
            zone = Some(match zone {
                None => r,
                Some(z) => RECT { left: z.left.min(r.left), top: z.top.min(r.top), right: z.right.max(r.right), bottom: z.bottom.max(r.bottom) },
            });
        };
        for image in images {
            let rect = image.rect;
            etendre(image.ecran);
            etendre(RECT { left: rect[0] as i32, top: rect[1] as i32, right: (rect[0] + rect[2]) as i32, bottom: (rect[1] + rect[3]) as i32 });
        }
        let Some(zone) = zone else { return Ok(()) };
        // Rien ne se dessine hors des écrans : la surcouche ne les déborde pas
        // (voir `touche_le_bureau` — une icône de départ peut l'être aussi).
        let bureau = bureau_virtuel();
        let zone = RECT {
            left: zone.left.max(bureau.left),
            top: zone.top.max(bureau.top),
            right: zone.right.min(bureau.right),
            bottom: zone.bottom.min(bureau.bottom),
        };
        if zone.right <= zone.left || zone.bottom <= zone.top {
            return Ok(());
        }
        let (largeur, hauteur) = ((zone.right - zone.left).max(1) as u32, (zone.bottom - zone.top).max(1) as u32);
        unsafe {
            let rang = sous_l_interface(self.hwnd).unwrap_or(HWND_TOPMOST);
            SetWindowPos(self.hwnd, Some(rang), zone.left, zone.top, largeur as i32, hauteur as i32, SWP_NOACTIVATE)?;
        }
        match &mut self.swapchain {
            Some(sc) if sc.width == largeur && sc.height == hauteur => {}
            Some(sc) => sc.resize(&gfx.device, largeur, hauteur)?,
            None => self.swapchain = Some(SwapChain::new(&gfx.device, self.hwnd, largeur, hauteur, true)?),
        }
        let swapchain = self.swapchain.as_ref().expect("créée juste au-dessus");

        let instances: Vec<(QuadInstance, &ID3D11ShaderResourceView)> = images
            .iter()
            .map(|image| {
                let rect = image.rect;
                let local = [rect[0] - zone.left as f32, rect[1] - zone.top as f32, rect[2], rect[3]];
                let mut instance = QuadInstance::new_textured(local, image.rayon, 1.0);
                instance.rotation = image.angle;
                instance.uv = image.uv;
                // L'opacité d'une texture : `color.a` (voir `PSMain`, mode Texture).
                instance.color[3] = image.opacite;
                (instance, image.srv)
            })
            .collect();
        let batches: Vec<QuadBatch> = instances
            .iter()
            .map(|(instance, srv)| QuadBatch::new(Some(*srv), std::slice::from_ref(instance)))
            .collect();
        // Sans cible (un redimensionnement a échoué) : une swapchain neuve à la
        // prochaine image plutôt qu'un plantage.
        let Some(rtv) = swapchain.rtv() else {
            self.swapchain = None;
            return Ok(());
        };
        self.renderer.render_at(&gfx.context, rtv, largeur as f32, hauteur as f32, &batches, (zone.left, zone.top))?;
        swapchain.present()?;
        if !self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
            self.visible = true;
        }
        Ok(())
    }

    fn cacher(&mut self) {
        if self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
        }
    }
}

/// Les fenêtres de l'INTERFACE de NeuraDesk — barres, leurs ombres et les coins
/// de l'écran : l'image qui vole passe SOUS elles (DEMANDE EXPLICITE,
/// 2026-10-16 : « toutes les fenêtres qui utilisent l'effet doivent apparaître
/// sous l'interface de l'app »).
const CLASSES_INTERFACE: [&str; 4] = ["NeuraDeskDockWindow", "NeuraDeskSecondaryDockWindow", "NdOmbreBarre", "NdCoinEcran"];

/// La plus BASSE des fenêtres visibles de l'interface, dans l'ordre d'empilement :
/// la surcouche se range juste dessous (et reste au premier plan, puisqu'elles le
/// sont). Relue à chaque frame — une barre se réaffirme au premier plan quand elle
/// veut. `None` : aucune barre visible (plein écran).
fn sous_l_interface(surcouche: HWND) -> Option<HWND> {
    struct Recherche {
        pid: u32,
        surcouche: HWND,
        plus_basse: Option<HWND>,
    }
    unsafe extern "system" fn chaque(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let r = &mut *(lparam.0 as *mut Recherche);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == r.pid && hwnd != r.surcouche && IsWindowVisible(hwnd).as_bool() {
            let mut tampon = [0u16; 64];
            let n = GetClassNameW(hwnd, &mut tampon).max(0) as usize;
            let classe = String::from_utf16_lossy(&tampon[..n]);
            if CLASSES_INTERFACE.iter().any(|c| classe == *c) {
                // L'énumération va du haut vers le bas : la dernière vue est la plus basse.
                r.plus_basse = Some(hwnd);
            }
        }
        windows::core::BOOL(1)
    }
    let mut r = Recherche { pid: unsafe { windows::Win32::System::Threading::GetCurrentProcessId() }, surcouche, plus_basse: None };
    unsafe {
        let _ = EnumWindows(Some(chaque), LPARAM(&mut r as *mut _ as isize));
    }
    r.plus_basse
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::WS_EX_NOREDIRECTIONBITMAP;

    const ICONE: [f32; 4] = [2500.0, 400.0, 48.0, 48.0];

    /// SONDE : une REGION VIDE cache-t-elle la fenetre a l'ecran SANS qu'elle se
    /// repeigne, et WGC voit-il encore son contenu ? Fenetre OUTIL (le crochet
    /// de l'app ouverte l'ignore), affichee 300 ms puis retiree.
    #[test]
    #[ignore]
    fn sonde_region_vide_et_capture() {
        use windows::Win32::Graphics::Gdi::{CreateRectRgn, GetDC, GetPixel, ReleaseDC, SetWindowRgn, HRGN};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, SW_SHOWNOACTIVATE,
            WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
        };
        fn pomper(ms: u64) {
            let fin = Instant::now() + Duration::from_millis(ms);
            while Instant::now() < fin {
                let mut msg = MSG::default();
                while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                    unsafe {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        fn pixel() -> u32 {
            unsafe {
                let dc = GetDC(None);
                let c = GetPixel(dc, 500, 400).0;
                ReleaseDC(None, dc);
                c
            }
        }
        let gfx = GfxDevice::new().expect("gfx");
        let avant = pixel();
        let hwnd = unsafe {
            CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST, w!("STATIC"), w!("sonde"), WS_POPUP, 300, 200, 500, 400, None, None, None, None)
        }
        .expect("fenetre");
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        pomper(300);
        let montree = pixel();
        unsafe {
            SetWindowRgn(hwnd, Some(CreateRectRgn(0, 0, 0, 0)), false);
        }
        // SANS pomper : la fenetre ne se repeint pas.
        unsafe {
            let _ = DwmFlush();
        }
        let _ = unsafe { DwmFlush() };
        let region_sans_pompe = pixel();
        println!("rect_visible avec region vide : {:?}", rect_visible(hwnd));
        let mut session = crate::win32::wgc::CaptureSession::start(&gfx.device, hwnd);
        let mut images = 0;
        let fin = Instant::now() + Duration::from_millis(800);
        while Instant::now() < fin {
            if let Some(s) = session.as_mut() {
                if s.poll(&gfx.device) {
                    images += 1;
                }
            }
            pomper(20);
        }
        let vide = session.as_ref().map(|s| image_vide(&gfx, s.srv()));
        let taille = session.as_ref().map(|s| s.size());
        unsafe {
            SetWindowRgn(hwnd, Some(HRGN::default()), true);
        }
        pomper(150);
        let rendue = pixel();
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        let _ = WS_VISIBLE;
        println!("pixel avant {avant:#x} montree {montree:#x} region(sans pompe) {region_sans_pompe:#x} rendue {rendue:#x}");
        println!("capture : session {} images {images} taille {taille:?} image_vide {vide:?}", session.is_some());
    }

    /// SONDE : `DWMWA_TRANSITIONS_FORCEDISABLED` est-il accepte sur la fenetre
    /// d'un AUTRE processus ? Pose la valeur PAR DEFAUT (0) : sans effet.
    #[test]
    #[ignore]
    fn sonde_transitions_fenetre_etrangere() {
        use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId};
        unsafe extern "system" fn une(hwnd: HWND, l: LPARAM) -> windows::core::BOOL {
            let liste = &mut *(l.0 as *mut Vec<HWND>);
            if unsafe { IsWindowVisible(hwnd) }.as_bool() && candidate(hwnd) {
                liste.push(hwnd);
            }
            true.into()
        }
        let mut liste: Vec<HWND> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(une), LPARAM(&mut liste as *mut _ as isize));
        }
        let moi = std::process::id();
        for hwnd in liste {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid == moi {
                continue;
            }
            let zero: i32 = 0;
            let r = unsafe {
                DwmSetWindowAttribute(hwnd, DWMWA_TRANSITIONS_FORCEDISABLED, &zero as *const _ as _, 4)
            };
            let mut tampon = [0u16; 64];
            let n = unsafe { GetClassNameW(hwnd, &mut tampon) }.max(0) as usize;
            println!("{:#x} {} -> {:?}", hwnd.0 as isize, String::from_utf16_lossy(&tampon[..n]), r);
        }
    }
    const FENETRE: [f32; 4] = [300.0, 200.0, 1200.0, 800.0];

    /// ⭐ **CHAQUE MOMENT A SES FORMES** (DEMANDE EXPLICITE 2026-11-05) : les
    /// trois gestes qui font disparaître la fenêtre offrent les sept formes ;
    /// agrandir et restaurer offrent en plus l'ÉTIREMENT, et c'est lui leur
    /// défaut — sans lui, un agrandissement cacherait la fenêtre.
    #[test]
    fn chaque_moment_propose_les_formes_attendues() {
        for moment in [Moment::Ouverture, Moment::Fermeture, Moment::Reduction] {
            assert_eq!(moment.formes(), &effets::FORMES, "{moment:?}");
            assert_eq!(code_retenu(moment, 5), 0, "{moment:?} : l'étirement n'a pas de sens ici");
        }
        assert_eq!(Moment::Agrandissement.formes(), &effets::AVEC_ETIREMENT);
        assert_eq!(code_retenu(Moment::Agrandissement, 5), 5);
        // Le défaut d'un code inconnu est la première forme du moment.
        assert_eq!(code_retenu(Moment::Fermeture, 999), 0);
        assert_eq!(code_retenu(Moment::Agrandissement, 999), 5);
        // « Au hasard » survit à la validation.
        for moment in Moment::TOUS {
            assert_eq!(code_retenu(moment, EFFET_ALEATOIRE), EFFET_ALEATOIRE, "{moment:?}");
        }
    }

    /// ⭐ Le geste de LANCEMENT (code 5) de la DLL se décode, et son sentinel
    /// `ex` ne se confond pas avec celui d'une fenêtre de NeuraDesk — sinon un
    /// lancement serait dévoilé par `devoiler` (DWM local) au lieu de rendre la
    /// main à la DLL. La table des numéros doit rester commune aux deux côtés
    /// (voir `window-hook::GESTE_LANCER = 5`).
    #[test]
    fn le_geste_de_lancement_se_decode_et_son_sentinel_est_distinct() {
        assert_eq!(Geste::depuis_numero(5), Some(Geste::Lancer));
        assert_eq!(Geste::depuis_numero(1), Some(Geste::Fermer));
        assert_eq!(Geste::depuis_numero(6), None, "un code inconnu n'anime rien");
        assert_ne!(LANCEMENT, INTERNE, "les deux sentinels doivent différer");
    }

    /// « Au hasard » tire VRAIMENT, et jamais hors des formes du moment.
    #[test]
    fn le_hasard_ne_sort_jamais_des_formes_du_moment() {
        let mut vus = std::collections::HashSet::new();
        for _ in 0..64 {
            let effet = forme_du_code(Moment::Fermeture, EFFET_ALEATOIRE);
            assert!(effets::FORMES.contains(&effet), "{effet:?} n'est pas une forme de fermeture");
            vus.insert(effet.code());
        }
        assert!(vus.len() >= 4, "le tirage se répète : {vus:?}");
        for _ in 0..32 {
            let effet = forme_du_code(Moment::Agrandissement, EFFET_ALEATOIRE);
            assert!(effets::AVEC_ETIREMENT.contains(&effet), "{effet:?} n'est pas une forme d'agrandissement");
        }
    }

    /// Les codes réglés désignent bien leur forme — y compris 5, l'étirement,
    /// qui jusqu'ici n'avait pas d'aller-retour.
    #[test]
    fn chaque_code_designe_sa_forme() {
        for effet in effets::AVEC_ETIREMENT {
            assert_eq!(forme_du_code(Moment::Agrandissement, effet.code()), effet, "{effet:?}");
        }
        for effet in effets::FORMES {
            assert_eq!(forme_du_code(Moment::Fermeture, effet.code()), effet, "{effet:?}");
        }
        assert_eq!(forme_du_code(Moment::Fermeture, 5), effets::Effet::Retour, "l'étirement retombe sur le vol");
    }

    /// Le fondu part de l'invisible, finit opaque, monte sans revenir en arrière,
    /// et reste borné hors de [0, 1].
    #[test]
    fn le_fondu_va_de_l_invisible_a_l_opaque() {
        assert_eq!(opacite_du_fondu(0.0), 0.0);
        assert_eq!(opacite_du_fondu(1.0), 1.0);
        assert_eq!((opacite_du_fondu(-1.0), opacite_du_fondu(2.0)), (0.0, 1.0));
        let pas: Vec<f32> = (0..=20).map(|i| opacite_du_fondu(i as f32 / 20.0)).collect();
        assert!(pas.windows(2).all(|p| p[1] >= p[0]), "{pas:?}");
    }

    /// Un aplat (le fond d'Edge avant sa page) attend ; une barre de titre ou un
    /// texte suffit à partir.
    #[test]
    fn un_aplat_attend_son_contenu() {
        let brun = [40u8, 30, 60];
        assert!(est_un_aplat(&[brun; 64]));
        let mut presque = [brun; 64];
        presque[10] = [50, 40, 70];
        assert!(est_un_aplat(&presque), "un dégradé léger reste un aplat");
        let mut titre = [brun; 64];
        titre[3] = [230, 230, 230];
        assert!(!est_un_aplat(&titre));
    }

    /// Le vol part EXACTEMENT de l'icône et arrive EXACTEMENT sur la fenêtre,
    /// droite — sans quoi la vraie fenêtre, rendue à l'arrivée, sauterait.
    #[test]
    fn le_vol_part_de_l_icone_et_arrive_droit_sur_la_fenetre() {
        let (rect, angle) = vol(ICONE, FENETRE, 0.0);
        assert_eq!(rect, ICONE);
        assert!((DUREE_MIN_MS..=DUREE_MAX_MS).contains(&DUREE_PAR_DEFAUT_MS));
        assert!((angle.abs() - TOURS * std::f32::consts::TAU).abs() < 1e-4);
        let (rect, angle) = vol(ICONE, FENETRE, 1.0);
        assert_eq!(rect, FENETRE);
        assert_eq!(angle, 0.0);
        assert_eq!(vol(ICONE, FENETRE, 7.0), vol(ICONE, FENETRE, 1.0), "borné");
    }

    /// Elle grandit sans jamais rapetisser, et tourne de moins en moins.
    #[test]
    fn la_fenetre_grandit_et_ralentit() {
        let mut avant = vol(ICONE, FENETRE, 0.0);
        for pas in 1..=20 {
            let apres = vol(ICONE, FENETRE, pas as f32 / 20.0);
            assert!(apres.0[2] >= avant.0[2] && apres.0[3] >= avant.0[3]);
            assert!(apres.1.abs() <= avant.1.abs());
            avant = apres;
        }
    }

    #[test]
    fn sans_icone_elle_part_de_son_centre() {
        let depart = depart_au_centre(FENETRE);
        let centre = |r: [f32; 4]| (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        assert_eq!(centre(depart), centre(FENETRE));
        assert_eq!((depart[2], depart[3]), (FENETRE[2] / 10.0, FENETRE[3] / 10.0));
    }

    /// Seule une fenêtre d'application est cachée : titrée, sans propriétaire, et
    /// dont l'alpha de fenêtre a prise.
    #[test]
    fn seules_les_fenetres_d_application_sont_cachees() {
        let titree = WS_CAPTION.0;
        assert!(est_candidate(titree, 0, true));
        assert!(!est_candidate(titree, 0, false), "un dialogue");
        assert!(!est_candidate(WS_POPUP.0, 0, true), "sans barre de titre (écran d'ouverture)");
        assert!(!est_candidate(titree | WS_CHILD.0, 0, true));
        assert!(!est_candidate(titree, WS_EX_TOOLWINDOW.0, true));
        assert!(est_candidate(titree, WS_EX_LAYERED.0, true), "son alpha lui appartient — mais on l'anime sans y toucher (Inkscape, mesuré)");
        assert!(est_candidate(titree, WS_EX_NOREDIRECTIONBITMAP.0, true), "Chrome, Discord, Terminal : mesuré");
        assert!(est_candidate(WS_POPUP.0 | WS_MINIMIZEBOX.0, 0, true), "cadre dessiné par l'application");
        // GTK : `style = 0x16000000`, ni légende ni bouton de réduction, mais
        // pas un popup — c'est le toplevel d'Inkscape (mesuré le 2026-11-05).
        assert!(est_candidate(0x1600_0000, WS_EX_LAYERED.0, true), "GTK dessine son propre cadre");
    }

    /// NOS fenêtres volent à 250 ms ; une fenêtre d'emprunt attend toujours ses
    /// 1 500 ms (ou l'immobilité de l'image). Sur l'ancien code — le même calcul
    /// avec `ATTENTE_CONTENU` pour tout le monde — la deuxième assertion du volet
    /// interne échouait : c'est exactement ce qui produisait les 2,04 s mesurées
    /// le 2026-10-22 sur l'ouverture des kaomojis.
    #[test]
    fn nos_fenetres_ne_attendent_pas_les_1500_ms_d_autrui() {
        assert!(!contenu_attendu(INTERNE, Duration::from_millis(249), None), "les 250 ms ne sont pas encore passées");
        assert!(contenu_attendu(INTERNE, Duration::from_millis(251), None), "et aucune image neuve n'est exigée : le voile gèle la capture");
        assert!(!contenu_attendu(0, Duration::from_millis(1499), None), "une fenêtre d'emprunt, inchangée");
        assert!(contenu_attendu(0, Duration::from_millis(1501), None));
        assert!(contenu_attendu(0, Duration::from_millis(100), Some(IMMOBILITE + Duration::from_millis(1))), "une image qui ne bouge plus écourte l attente (Edge, mesuré)");
    }

    /// Une fenêtre encore à sa place de garage n'est pas à sa place : la lire
    /// étirait la surcouche au-delà de 16 384 px, et l'app plantait.
    #[test]
    fn une_fenetre_garee_hors_ecran_n_est_pas_encore_placee() {
        let bureau = RECT { left: 0, top: 0, right: 3840, bottom: 2160 };
        let [x, y, l, h] = crate::win32::cef_popup::PRELOAD_RECT;
        assert!(!touche_le_bureau(RECT { left: x, top: y, right: x + l, bottom: y + h }, bureau));
        assert!(touche_le_bureau(RECT { left: 106, top: 50, right: 3720, bottom: 2100 }, bureau));
        assert!(touche_le_bureau(RECT { left: -200, top: 100, right: 50, bottom: 400 }, bureau), "à cheval sur le bord");
        assert!(!touche_le_bureau(RECT { left: 3840, top: 0, right: 4000, bottom: 300 }, bureau), "collée au bord, dehors");
    }
}
