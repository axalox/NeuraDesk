//! **LE LECTEUR VIDÉO** — ⭐ DEMANDE EXPLICITE (2026-10-18) : « utiliser l'API de
//! VLC pour afficher les vidéos dans le dash dossier » ; « sélectionner Afficher
//! dans le menu fait se lire la vidéo dans une fenêtre pouvant être basculée en
//! plein écran, avec lecture/pause, barre de progression et volume ».
//!
//! **LIBVLC, CELLE DE VLC INSTALLÉ** — décision de l'utilisateur. Elle est chargée
//! À LA DEMANDE (`LoadLibraryExW` sur son chemin complet, voir `api`), jamais liée
//! à l'édition : sans VLC, NeuraDesk démarre et marche comme avant, et « Afficher »
//! sur une vidéo le DIT (`ouvrir` rend la raison au dash). libVLC 3 trouve seule
//! ses `plugins/`, à côté de `libvlc.dll`.
//!
//! **UNE FENÊTRE NATIVE, PAS UNE PAGE CEF** : libVLC peint dans un `HWND`
//! (`libvlc_media_player_set_hwnd`) — ici `image`, l'enfant qui couvre tout sauf la
//! barre de commandes. La barre est dessinée en GDI par ce module : une page CEF,
//! rendue hors écran, ne saurait pas accueillir la vidéo dessous.
//!
//! **LES GESTES** : clic sur l'image = lecture/pause ; double-clic, `F` ou le
//! bouton = plein écran (`Échap` en sort) ; `Espace` = lecture/pause ; `←`/`→` =
//! ±5 s ; `↑`/`↓` et la molette = volume. La progression et le volume se règlent
//! en cliquant ou en glissant sur leur piste.
//!
//! ⭐ **LES FLÈCHES DE VOISINAGE** (⭐ DEMANDE EXPLICITE, 2026-11-03 : « des flèches
//! en surbrillance pour passer à la vidéo suivante ») : `‹` et `›` aux deux bouts de
//! la barre, ou `Ctrl`+`←` / `Ctrl`+`→`. Elles parcourent **les vidéos du même
//! dossier**, dans l'ordre de la moitié du dash dossier qui a ouvert la première —
//! jamais les images, jamais d'autre chose. La fenêtre ne change pas : seule la
//! lecture repart (voir `changer_de_media`).
//!
//! **LA BARRE DE COMMANDES EST SA PROPRE FENÊTRE** (`barre`, sœur de `image`) :
//! sous l'image en fenêtre, PAR-DESSUS en plein écran — où elle se cache, avec le
//! curseur, quand la souris ne bouge plus (⭐ DEMANDE EXPLICITE, 2026-10-19 :
//! « que la barre de commande se cache si la souris ne bouge plus »), sans que
//! l'image change de taille.
//!
//! ⚠️ **LA SOURIS ET LE CLAVIER DE LA VIDÉO SONT COUPÉS CÔTÉ VLC**
//! (`libvlc_video_set_mouse_input` / `_key_input`) : sinon sa propre fenêtre, posée
//! dans `image`, mange les clics et les touches, et aucun geste n'arrive ici.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows::core::{s, w, HSTRING, PCSTR, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateRoundRectRgn,
    CreateSolidBrush,
    CreatePen, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetMonitorInfoW, GetStockObject, InvalidateRect,
    MonitorFromPoint, MonitorFromWindow, Polygon, Polyline, PS_ENDCAP_FLAT, PS_GEOMETRIC, PS_JOIN_ROUND, PS_SOLID, ScreenToClient,
    SelectObject, SetBkMode, SetTextColor, SetWindowRgn, ANTIALIASED_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH,
    DT_LEFT, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_NORMAL, HDC, HFONT, MONITORINFO, MONITOR_DEFAULTTONEAREST, NULL_PEN,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RIGHT, VK_SPACE,
    VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClassNameW, GetClientRect, GetCursorPos, GetParent, GetSystemMetrics,
    GetWindowThreadProcessId, IsWindowVisible,
    GetWindowLongPtrW, GetWindowRect, KillTimer, LoadCursorW, PostMessageW, RegisterClassExW, SetCursor, SetTimer,
    SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, WindowFromPoint, CS_DBLCLKS, CS_HREDRAW,
    CS_VREDRAW, GWL_STYLE, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST, IDC_ARROW, MINMAXINFO, SM_CXPADDEDBORDER, SM_CXSIZEFRAME,
    SWP_FRAMECHANGED, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SW_HIDE, SW_SHOW,
    WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SETCURSOR, WM_SIZE, WM_TIMER, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN,
    WS_CLIPSIBLINGS, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

// ---------------------------------------------------------------------------
// libVLC, chargée à la demande
// ---------------------------------------------------------------------------

/// Les fonctions de libVLC 3 dont le lecteur a besoin (voir `libvlc.h`).
struct Api {
    media_new_path: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    media_release: unsafe extern "C" fn(*mut c_void),
    lecteur_depuis_media: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    lecteur_release: unsafe extern "C" fn(*mut c_void),
    set_hwnd: unsafe extern "C" fn(*mut c_void, *mut c_void),
    play: unsafe extern "C" fn(*mut c_void) -> i32,
    set_pause: unsafe extern "C" fn(*mut c_void, i32),
    stop: unsafe extern "C" fn(*mut c_void),
    is_playing: unsafe extern "C" fn(*mut c_void) -> i32,
    get_length: unsafe extern "C" fn(*mut c_void) -> i64,
    get_time: unsafe extern "C" fn(*mut c_void) -> i64,
    set_time: unsafe extern "C" fn(*mut c_void, i64),
    get_state: unsafe extern "C" fn(*mut c_void) -> i32,
    set_volume: unsafe extern "C" fn(*mut c_void, i32) -> i32,
    souris: unsafe extern "C" fn(*mut c_void, u32),
    clavier: unsafe extern "C" fn(*mut c_void, u32),
    taille_video: unsafe extern "C" fn(*mut c_void, u32, *mut u32, *mut u32) -> i32,
    /// L'instance libVLC, unique pour tout le process (jamais libérée : elle vit
    /// autant que lui, comme la DLL).
    instance: usize,
}

/// `libvlc_state_t` : fin de la lecture, et erreur.
const ETAT_FINI: i32 = 6;
const ETAT_ERREUR: i32 = 7;

/// Les dossiers où chercher `libvlc.dll` : `NEURADESK_VLC` d'abord (un VLC
/// portable), puis un `vlc\` à côté de l'exécutable, puis VLC installé (64 bits).
fn dossiers_vlc() -> Vec<PathBuf> {
    let mut dossiers = Vec::new();
    if let Some(d) = std::env::var_os("NEURADESK_VLC") {
        dossiers.push(PathBuf::from(d));
    }
    if let Some(exe) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        dossiers.push(exe.join("vlc"));
    }
    for variable in ["ProgramW6432", "ProgramFiles"] {
        if let Some(d) = std::env::var_os(variable) {
            dossiers.push(PathBuf::from(d).join("VideoLAN").join("VLC"));
        }
    }
    dossiers
}

/// `libvlc.dll` est-elle là (sans la charger) ? Voir `cef_sous_dock::
/// Demande::AideVideo`.
pub fn vlc_installe() -> bool {
    dossiers_vlc().into_iter().any(|d| d.join("libvlc.dll").is_file())
}

/// libVLC, chargée et initialisée au premier appel. `Err` : la raison, à dire à
/// l'utilisateur — elle est retenue, on ne recherche pas VLC à chaque clic…
/// sauf s'il n'était pas installé : on retente alors, il a pu l'installer depuis.
fn api() -> Result<&'static Api, String> {
    static API: OnceLock<Api> = OnceLock::new();
    if let Some(api) = API.get() {
        return Ok(api);
    }
    let api = charger()?;
    Ok(API.get_or_init(|| api))
}

fn charger() -> Result<Api, String> {
    let dll = dossiers_vlc()
        .into_iter()
        .map(|d| d.join("libvlc.dll"))
        .find(|p| p.is_file())
        .ok_or_else(|| "VLC n'est pas installé : voir l'aide pour l'installer (VLC 64 bits)".to_string())?;
    // `LOAD_WITH_ALTERED_SEARCH_PATH` : `libvlccore.dll` est cherchée à côté de
    // `libvlc.dll`, pas à côté de neuradesk.exe.
    let module = unsafe { LoadLibraryExW(&HSTRING::from(dll.as_os_str()), None, LOAD_WITH_ALTERED_SEARCH_PATH) }
        .map_err(|e| format!("libVLC ne se charge pas ({}) : {e}", dll.display()))?;
    let resoudre = |nom: PCSTR| unsafe { GetProcAddress(module, nom) };
    macro_rules! fonction {
        ($nom:literal) => {
            match resoudre(s!($nom)) {
                // SAFETY: le prototype est celui de `libvlc.h` (libVLC 3).
                Some(f) => unsafe { std::mem::transmute_copy(&f) },
                None => return Err(format!("libVLC trop ancienne ou incomplète : {} manque", $nom)),
            }
        };
    }
    let nouvelle: unsafe extern "C" fn(i32, *const *const c_char) -> *mut c_void = fonction!("libvlc_new");
    // Pas de titre posé sur l'image au démarrage, pas de fenêtre de VLC à lui.
    let options = [c"--no-video-title-show", c"--no-snapshot-preview"];
    let arguments: Vec<*const c_char> = options.iter().map(|o| o.as_ptr()).collect();
    let instance = unsafe { nouvelle(arguments.len() as i32, arguments.as_ptr()) };
    if instance.is_null() {
        return Err("libVLC refuse de démarrer (plugins introuvables ?)".to_string());
    }
    crate::diag!("[lecteur_video] libVLC chargée : {}", dll.display());
    Ok(Api {
        media_new_path: fonction!("libvlc_media_new_path"),
        media_release: fonction!("libvlc_media_release"),
        lecteur_depuis_media: fonction!("libvlc_media_player_new_from_media"),
        lecteur_release: fonction!("libvlc_media_player_release"),
        set_hwnd: fonction!("libvlc_media_player_set_hwnd"),
        play: fonction!("libvlc_media_player_play"),
        set_pause: fonction!("libvlc_media_player_set_pause"),
        stop: fonction!("libvlc_media_player_stop"),
        is_playing: fonction!("libvlc_media_player_is_playing"),
        get_length: fonction!("libvlc_media_player_get_length"),
        get_time: fonction!("libvlc_media_player_get_time"),
        set_time: fonction!("libvlc_media_player_set_time"),
        get_state: fonction!("libvlc_media_player_get_state"),
        set_volume: fonction!("libvlc_audio_set_volume"),
        souris: fonction!("libvlc_video_set_mouse_input"),
        clavier: fonction!("libvlc_video_set_key_input"),
        taille_video: fonction!("libvlc_video_get_size"),
        instance: instance as usize,
    })
}

// ---------------------------------------------------------------------------
// La fenêtre
// ---------------------------------------------------------------------------

/// La hauteur de la barre de commandes, en pixels à 96 ppp.
const HAUTEUR_BARRE: i32 = 44;
/// Le pas des flèches gauche/droite, en millisecondes.
const PAS_MS: i64 = 5000;
/// Le pas du volume (flèches, molette), en pour-cent.
const PAS_VOLUME: i32 = 5;
/// La cadence du rafraîchissement de la barre.
const MINUTEUR_MS: u32 = 250;
const MINUTEUR: usize = 1;

const FOND_BARRE: u32 = 0x0020_1C1C;
const PISTE: u32 = 0x0050_4848;
const ACCENT: u32 = 0x00D4_7800;
const TEXTE: u32 = 0x00EE_EEEE;
/// Une flèche de voisinage **éteinte** : il n'y a rien de ce côté. Elle se peint
/// quand même, à ce ton — une flèche absente ferait croire que la barre n'en a
/// pas, alors qu'un voisin existe peut-être de l'autre côté.
const ETEINTE: u32 = 0x0077_7070;

/// En plein écran, le calme de la souris après lequel la barre et le curseur se
/// cachent.
const DELAI_CACHE: Duration = Duration::from_millis(2500);

// ---------------------------------------------------------------------------
// La SURIMPRESSION sur l'image — DEMANDE DU 2026-11-03 (second retour)
// ---------------------------------------------------------------------------
//
// ⛔ **CE SONT DEUX FENÊTRES POSÉES PAR-DESSUS `image`, PAS DE LA GDI DESSINÉE
// DESSUS.** `libvlc` peint la vidéo dans `image` par une chaîne d'échange D3D11 :
// tout ce qu'on y peindrait en GDI serait effacé à l'image suivante. Une fenêtre
// sœur, remontée après l'image, est composée par DWM par-dessus la chaîne — et c'est
// le hit-test de Windows (`WindowFromPoint`) qui dit si un vrai doigt l'atteint.
//
// Les mesures sont celles de `.bord` dans `assets/popups/visionneuse.html` : les deux
// surfaces ont le MÊME cadre pour le même geste (34 × 46 px CSS à 10 px du bord, rayon
// 7, allumage au survol, `ZONE_BORD`, côté sans voisin absent) et le MÊME glyph — un
// caret TRACÉ FIN, aux mesures du `<span class="ico">` de la page. Le bouton `‹`/`›` de
// la barre, lui, garde sa flèche pleine : c'est une autre affordance, posée ailleurs, et
// elle ne change pas.

/// Le cartel, en px CSS : 34 × 46, posé à 10 px du bord, rayon 7 (`px(CARTEL_RAYON)`).
const CARTEL_L: i32 = 34;
const CARTEL_H: i32 = 46;
const CARTEL_MARGE: i32 = 10;
const CARTEL_RAYON: i32 = 7;
/// La zone au bord de l'image où la souris ALLUME davantage le cartel de ce côté — le
/// `ZONE_BORD` de la page, mesuré depuis le bord de l'image et non depuis le cartel
/// (la page mesure `e.clientX - r.left`).
const ZONE_BORD: i32 = 110;
/// Le fond du cartel et celui du côté visé, avec leur encre : les deux mêmes rôles
/// que `--popup-fond` / `--popup-accent-solid` + `--popup-on-accent` de la page. En
/// GDI le fond est OPAQUE : par-dessus une chaîne d'échange, rien ne transparaît.
const FOND_CARTEL: u32 = FOND_BARRE;
const FOND_CARTEL_VISE: u32 = ACCENT;
const ENCRE_CARTEL: u32 = TEXTE;
const ENCRE_CARTEL_VISE: u32 = 0x0010_0C0C;
/// Le CARET du cartel, aux mesures du glyph de la page : `font-size: 18px` dans un
/// `.ico` de `1.15em`, masque `center / contain` (`style.css`) → une marque d'environ
/// 11 × 7 px CSS, tracée à ~2 px. Ce n'est PAS le triangle plein du bouton de la barre,
/// qui occupe le tiers de sa boîte : à `hauteur / 3` dans un cartel de 46 px, le cartel
/// aurait eu une marque de ~30 × 25 px là où la page en montre une de 11 × 7.
const CARET_H: i32 = 11;
const CARET_L: i32 = 7;
const CARET_TRAIT: i32 = 2;

/// Ce qu'on glisse, souris enfoncée.
#[derive(Clone, Copy, PartialEq)]
enum Glisse {
    Progression,
    Volume,
}

struct Lecteur {
    image: HWND,
    barre: HWND,
    /// **LES DEUX CARTELS DE SURIMPRESSION** posés sur l'image (le chevron de
    /// voisinage, comme `.bord` de la visionneuse), et leur état : quels côtés sont
    /// montrés, et lequel la souris vise. Tenu par `eclaire_les_cartels`, lu par
    /// `peindre_cartel` et `poser_les_cartels`.
    avant: HWND,
    apres: HWND,
    montres: (bool, bool),
    vise: (bool, bool),
    /// `libvlc_media_player_t *`.
    joueur: usize,
    /// **CE QUI EST EN LECTURE, et la SERIE de vidéos que la flèche parcourt dans
    /// son dossier** — DEMANDE EXPLICITE (2026-11-03) : « des flèches en surbrillance
    /// pour passer à la vidéo suivante ». Le rang du fichier dans cette série donne
    /// les deux côtés de la flèche, et un côté vide l'éteint.
    ///
    /// ⛔ **LA SÉRIE EST RETENUE À L'OUVERTURE, et ce n'est pas une preference.**
    /// La version d'avant gardait `precedent`/`suivant` et les recalculait à chaque
    /// appui en relisant le dossier — soit, avec un tri par date ou par taille, un
    /// `read_dir` + un `metadata()` PAR entrée + un tri, sur le fil unique de
    /// `app::run` : trois mille clips, c'est la barre et les pages CEF gelées sous la
    /// main. `cef_visionneuse::Vue.serie` fait pareil pour les images.
    ///
    /// ⚠️ **L'ordre est celui de la moitié du dash qui a cliqué**, retenu une fois :
    /// défiler ne re-choisit pas un ordre, sinon la flèche et la grille défilent
    /// différemment. Le cache est une liste de disque : un fichier supprimé pendant
    /// que la fenêtre est ouverte se dit, et la fenêtre garde ce qu'elle montrait.
    chemin: PathBuf,
    serie: Vec<PathBuf>,
    volume: i32,
    police: HFONT,
    glisse: Option<Glisse>,
    /// La fraction visée pendant un glissé de la progression (affichée à la
    /// place de la position du joueur, qui suit avec retard).
    visee: Option<f32>,
    /// Le style et le cadre d'avant le plein écran.
    plein_ecran: Option<(isize, RECT)>,
    /// La fenêtre a pris la proportion de la vidéo (une fois, à la première image).
    ajustee: bool,
    /// Le dernier mouvement de la souris, et où elle était (en plein écran, la
    /// barre se cache passé `DELAI_CACHE`).
    bouge: (Instant, POINT),
    /// La barre et le curseur sont cachés (plein écran seulement).
    cachee: bool,
}

thread_local! {
    static LECTEURS: RefCell<HashMap<isize, Lecteur>> = RefCell::new(HashMap::new());
}

/// ⛔ **LE DOCK RESTAIT PAR-DESSUS LE PLEIN ÉCRAN** — DÉFAUT SIGNALÉ (2026-10-19) :
/// « quand VLC est en mode plein écran toute l'app reste visible par-dessus ». Le
/// dock ignore SES PROPRES fenêtres en cherchant une application plein écran
/// (voir `win32::window::foreground_fullscreen_window`) : celle-ci, qui en est
/// une, ne le masquait donc jamais, et ses barres topmost, remontées sans cesse,
/// passaient devant. Les lecteurs en plein écran sont tenus ici, hors du fil
/// (une simple liste), pour qu'il en fasse l'exception.
static EN_PLEIN_ECRAN: std::sync::Mutex<Vec<isize>> = std::sync::Mutex::new(Vec::new());

/// `hwnd` est-il un lecteur en plein écran ?
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

/// Le volume des lecteurs, retenu d'une vidéo à l'autre (et d'un lancement à
/// l'autre, voir `config`).
fn volume_retenu() -> i32 {
    crate::config::config_path("lecteur_video_volume.txt")
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|v| v.trim().parse::<i32>().ok())
        .map_or(80, |v| v.clamp(0, 100))
}

fn retenir_volume(volume: i32) {
    crate::config::write_config("lecteur_video_volume.txt", volume.to_string());
}

/// **LA SÉRIE DE VIDÉOS QUE LA FLÈCHE PARCOURT** — le même rang que la grille du
/// dash dossier (`sous_dock::serie_de_media`). Le panneau étant fermé à l'ouverture,
/// c'est la liste relue sur le disque, avec le tri reçu de la moitié qui a cliqué.
///
/// ⚠️ **UNE SEULE FOIS PAR FENÊTRE** : la série est retenue dans `Lecteur.serie` et
/// les appuis de flèche la lisent sans plus rien relire (`cotes_de_la_fleche`).
fn serie_de_videos(chemin: &Path, tri: crate::win32::sous_dock::Tri) -> Vec<PathBuf> {
    crate::win32::sous_dock::serie_de_media(chemin, tri, crate::win32::sous_dock::GenreMedia::Video)
}

/// **LES DEUX CÔTÉS DE LA FLÈCHE, lus sur la série retenue** — zéro appel système et
/// **zéro clone**, parce que ce calcul est payé deux fois par seconde par `peindre` ET
/// à CHAQUE `WM_MOUSEMOVE` par `etat_des_cartels` : des centaines de fois pendant que
/// la souris traverse l'image. Le RANGE suffit ici ; les deux chemins ne sont demandés
/// qu'au clic, dans `voisin_de_la_fleche`.
fn cotes_de_la_fleche(l: &Lecteur) -> (bool, bool) {
    let Some(rang) = crate::win32::sous_dock::rang_dans_serie(&l.serie, &l.chemin) else {
        return (false, false);
    };
    (rang > 0, l.serie.get(rang + 1).is_some())
}

/// **LE FICHIER QUE LA FLÈCHE MÈNE** — `None` aux deux bouts de la série.
fn voisin_de_la_fleche(l: &Lecteur, suivant: bool) -> Option<PathBuf> {
    let (avant, apres) = crate::win32::sous_dock::voisins_dans_serie(&l.serie, &l.chemin);
    if suivant { apres } else { avant }
}

/// Ouvre `chemin` dans une nouvelle fenêtre de lecture. `Err` : la raison, à dire
/// dans le dash (VLC absent, fichier refusé…). `tri` est l'ordre de la moitié du dash
/// dossier qui a cliqué — celui que les flèches de voisinage suivront (voir
/// `serie_de_videos`, et `Lecteur.serie` pour pourquoi il n'est lu qu'ici).
pub fn ouvrir(chemin: &Path, tri: crate::win32::sous_dock::Tri) -> Result<(), String> {
    let api = api()?;
    let instance = classes()?;
    let utf8 = CString::new(chemin.to_string_lossy().as_bytes()).map_err(|_| "Chemin de fichier invalide".to_string())?;
    let titre = chemin.file_name().map_or_else(|| chemin.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());

    // Un cadre de 16/9 au centre de l'écran du curseur, à 70 % au plus de sa
    // zone de travail ; la proportion de la vidéo le corrige à la première image.
    let mut curseur = POINT::default();
    let _ = unsafe { GetCursorPos(&mut curseur) };
    let travail = zone_de_travail(unsafe { MonitorFromPoint(curseur, MONITOR_DEFAULTTONEAREST) });
    let (lt, ht) = (travail.right - travail.left, travail.bottom - travail.top);
    let largeur = (lt * 7 / 10).min(1280);
    let hauteur = (largeur * 9 / 16 + HAUTEUR_BARRE + 40).min(ht * 8 / 10);
    let hwnd = unsafe {
        CreateWindowExW(
            Default::default(),
            w!("NdLecteurVideo"),
            &HSTRING::from(format!("{titre} — NeuraDesk")),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            travail.left + (lt - largeur) / 2,
            travail.top + (ht - hauteur) / 2,
            largeur,
            hauteur,
            None,
            None,
            Some(instance),
            None,
        )
    }
    .map_err(|e| format!("Fenêtre de lecture impossible : {e}"))?;
    // Deux sœurs qui se recouvrent en plein écran : `WS_CLIPSIBLINGS` pour que
    // l'image ne peigne pas sur la barre posée au-dessus d'elle.
    // ⚠️ **L'ORDRE DE CRÉATION EST LE Z-ORDRE** : l'image d'abord, les deux cartels de
    // surimpression ensuite (ils doivent être PAR-DESSUS la vidéo), la barre en
    // dernier (elle doit rester par-dessus les cartels). Les cartels naissent cachés :
    // `WS_VISIBLE` ici montrerait un carré noir de 1 × 1 px avant le premier
    // `poser_les_cartels`.
    let enfant = |classe: PCWSTR, visible: bool| unsafe {
        CreateWindowExW(
            Default::default(),
            classe,
            PCWSTR::null(),
            WS_CHILD | WS_CLIPSIBLINGS | if visible { WS_VISIBLE } else { Default::default() },
            0,
            0,
            1,
            1,
            Some(hwnd),
            None,
            Some(instance),
            None,
        )
    };
    let nes = [
        enfant(w!("NdLecteurVideoImage"), true),
        enfant(w!("NdLecteurSurAvant"), false),
        enfant(w!("NdLecteurSurApres"), false),
        enfant(w!("NdLecteurVideoBarre"), true),
    ];
    let (image, avant, apres, barre) = match &nes {
        [Ok(image), Ok(avant), Ok(apres), Ok(barre)] => (*image, *avant, *apres, *barre),
        _ => {
            let raison = nes.iter().find_map(|r| r.as_ref().err()).map(|e| e.to_string()).unwrap_or_else(|| "fenêtre enfant impossible".to_string());
            let _ = unsafe { DestroyWindow(hwnd) };
            return Err(format!("Fenêtre de lecture impossible : {raison}"));
        }
    };

    let joueur = unsafe {
        let media = (api.media_new_path)(api.instance as *mut c_void, utf8.as_ptr());
        if media.is_null() {
            let _ = DestroyWindow(hwnd);
            return Err("VLC refuse ce fichier".to_string());
        }
        let joueur = (api.lecteur_depuis_media)(media);
        (api.media_release)(media);
        if joueur.is_null() {
            let _ = DestroyWindow(hwnd);
            return Err("VLC ne peut pas lire ce fichier".to_string());
        }
        (api.souris)(joueur, 0);
        (api.clavier)(joueur, 0);
        (api.set_hwnd)(joueur, image.0);
        joueur
    };
    let volume = volume_retenu();
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
    let police = unsafe {
        CreateFontW(
            -(13 * dpi / 96),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            (DEFAULT_PITCH.0 as u32) | (FF_DONTCARE.0 as u32),
            w!("Segoe UI"),
        )
    };
    // LA SEULE LECTURE DU DOSSIER de cette fenêtre (voir `Lecteur.serie`).
    let serie = serie_de_videos(chemin, tri);
    LECTEURS.with(|l| {
        l.borrow_mut().insert(
            hwnd.0 as isize,
            Lecteur {
                image,
                barre,
                avant,
                apres,
                montres: (false, false),
                vise: (false, false),
                joueur: joueur as usize,
                chemin: chemin.to_path_buf(),
                serie,
                volume,
                police,
                glisse: None,
                visee: None,
                plein_ecran: None,
                ajustee: false,
                bouge: (Instant::now(), curseur),
                cachee: false,
            },
        )
    });
    placer(hwnd);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        // ⛔ **ELLE S'OUVRAIT DERRIÈRE** — DÉFAUT SIGNALÉ (2026-10-19) : « afficher
        // une vidéo ne semble pas fonctionner ». La lecture tournait (mesuré :
        // état « lecture », le temps avançait), mais la fenêtre restait sous le
        // navigateur : `SetForegroundWindow` seul est refusé EN SILENCE quand le
        // premier plan est à un autre processus. Le contournement du projet
        // (`AttachThreadInput`) l'y porte, focus clavier compris.
        crate::win32::cef_popup::force_foreground(hwnd);
        SetTimer(Some(hwnd), MINUTEUR, MINUTEUR_MS, None);
        if (api.play)(joueur) != 0 {
            crate::diag!("[lecteur_video] {titre} : la lecture ne démarre pas");
        }
        (api.set_volume)(joueur, volume);
    }
    crate::diag!("[lecteur_video] {titre} : lecture ouverte");
    Ok(())
}

fn classes() -> Result<HINSTANCE, String> {
    static CLASSES: OnceLock<()> = OnceLock::new();
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?.into();
    CLASSES.get_or_init(|| unsafe {
        let curseur = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
        let noir = CreateSolidBrush(COLORREF(0));
        for (nom, procedure) in [
            (w!("NdLecteurVideo"), wndproc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT),
            (w!("NdLecteurVideoImage"), wndproc_image),
            // Les deux cartels de surimpression : DEUX classes, parce que le cartel
            // doit savoir de quel côté il mène et qu'il n'a rien d'autre pour le dire.
            (w!("NdLecteurSurAvant"), wndproc_surimpression_gauche),
            (w!("NdLecteurSurApres"), wndproc_surimpression_droite),
            (w!("NdLecteurVideoBarre"), wndproc_barre),
        ] {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_DBLCLKS | CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(procedure),
                hInstance: instance,
                hCursor: curseur,
                hbrBackground: noir,
                lpszClassName: nom,
                ..Default::default()
            };
            RegisterClassExW(&wc);
        }
    });
    Ok(instance)
}

fn zone_de_travail(ecran: windows::Win32::Graphics::Gdi::HMONITOR) -> RECT {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    let _ = unsafe { GetMonitorInfoW(ecran, &mut info) };
    info.rcWork
}

fn echelle(hwnd: HWND) -> i32 {
    unsafe { GetDpiForWindow(hwnd) }.max(96) as i32
}

/// L'épaisseur cumulée des deux bordures horizontales de la fenêtre, en pixels
/// physiques — ce qui sépare la largeur de la FENÊTRE de celle de sa barre.
///
/// ⚠️ **REPLI SUR LES MÉTRIQUES** tant que le client n'est pas mesurable :
/// `WM_GETMINMAXINFO` est envoyé PENDANT `CreateWindowExW`, avant que le client
/// ait une taille, et une différence calculée sur un client vide rendrait 0 —
/// donc un plancher plus PETIT que la barre, exactement le défaut qu'on pose.
fn bordure_horizontale(hwnd: HWND) -> i32 {
    let (mut cadre, mut client) = (RECT::default(), RECT::default());
    unsafe {
        let _ = GetWindowRect(hwnd, &mut cadre);
        let _ = GetClientRect(hwnd, &mut client);
    }
    let (exterieur, interieur) = (cadre.right - cadre.left, client.right - client.left);
    if interieur > 0 && exterieur > interieur {
        exterieur - interieur
    } else {
        unsafe { 2 * (GetSystemMetrics(SM_CXSIZEFRAME) + GetSystemMetrics(SM_CXPADDEDBORDER)) }
    }
}

/// Les zones de la barre de commandes, en coordonnées de la barre.
struct Zones {
    barre: RECT,
    lecture: RECT,
    /// Les deux flèches de voisinage — de part et d'autre de la barre : `‹` après
    /// lecture/pause, `›` avant plein écran (voir `peindre`).
    precedent: RECT,
    suivant: RECT,
    temps: RECT,
    progression: RECT,
    volume: RECT,
    plein_ecran: RECT,
}

/// Les tailles CSS de la barre, en CONSTANTS nommées. Le plancher de la fenêtre
/// (`LARGEUR_MIN_CSS`) se calcule AVEC elles : deux listes de nombres qu'on
/// recopie finissent par diverger, et le défaut qui en sort — deux zones qui se
/// recouvrent — ne se voit pas à l'œil, il se clique.
const MARGE_BORD: i32 = 8;
const MARGE_FLECHE_G: i32 = 4;
const LARGEUR_FLECHE: i32 = 26;
const MARGE_TEMPS: i32 = 10;
const LARGEUR_TEMPS: i32 = 112;
const MARGE_PISTE: i32 = 6;
const PISTE_MIN: i32 = 40;
const MARGE_PISTE_VOLUME: i32 = 28;
const LARGEUR_VOLUME: i32 = 90;
const MARGE_VOLUME: i32 = 12;
const MARGE_FLECHE_D: i32 = 6;
/// La hauteur utile des boutons carrés : la barre fait `HAUTEUR_BARRE`, rognée de
/// 6 px en haut et en bas par `zones_dans`.
const HAUTEUR_BOUTON: i32 = HAUTEUR_BARRE - 2 * 6;

/// ⛔ **LA LARGEUR MINIMALE DE LA BARRE, EN PX CSS** — tout ce qu'elle pose, les
/// intervalles compris, plus `PISTE_MIN`. En dessous, `temps` et `volume` se
/// recouvrent : la piste tombe à `left == right`, un clic sur l'heure reglerait le
/// son et chercher une position deviendrait impossible.
///
/// ⚠️ **CE PLANCHER EXISTE PARCE QUE LES FLÈCHES ONT ROGNE LA PLACE.** Avant
/// elles, le seuil était `2 × HAUTEUR_BOUTON + 266` ; les deux flèches et leurs
/// marges l'ont monté à 440. Une fenêtre sans plancher se rogne toute seule :
/// `ajuster_a_la_video` suit la proportion de la vidéo, donc un clip 9:16 sur un
/// écran 1280×720 donnait une barre d'environ 274 px — le défaut, attrapé.
const LARGEUR_MIN_CSS: i32 = 2 * MARGE_BORD + 2 * HAUTEUR_BOUTON + MARGE_FLECHE_G + LARGEUR_FLECHE + MARGE_TEMPS
    + LARGEUR_TEMPS + MARGE_PISTE + PISTE_MIN + MARGE_PISTE_VOLUME + LARGEUR_VOLUME + MARGE_VOLUME + MARGE_FLECHE_D
    + LARGEUR_FLECHE;

/// Le plancher en pixels PHYSIQUES de l'écran de la fenêtre. ⚠️ **CONVERTI** :
/// poser le nombre CSS tel quel est une faute commise DEUX fois dans ce dépôt
/// (planchers en pixels physiques, ou qui ne suivent pas la conversion).
fn largeur_min(dpi: i32) -> i32 {
    LARGEUR_MIN_CSS * dpi / 96
}

/// Les zones de la barre, **calculées à partir du seul rectangle client et du DPI**
/// — la version pure de `zones`, parce que le chevauchement de deux zones est un
/// défaut qu'on ne voit pas : cliquer `›` reglerait le son au lieu de changer de
/// vidéo (DEMANDE EXPLICITE 2026-11-03 : les flèches de voisinage). Testé à plusieurs
/// largeurs, y compris au plancher et en DPI 144 (voir `mod tests`).
fn zones_dans(client: RECT, dpi: i32) -> Zones {
    let px = |v: i32| v * dpi / 96;
    let (haut, bas) = (client.top + px(6), client.bottom - px(6));
    let lecture = RECT { left: px(MARGE_BORD), top: haut, right: px(MARGE_BORD) + (bas - haut), bottom: bas };
    let plein_ecran =
        RECT { left: client.right - px(MARGE_BORD) - (bas - haut), top: haut, right: client.right - px(MARGE_BORD), bottom: bas };
    // Les flèches sont aux deux bouts qui restent, le temps et la progression
    // se logent entre elles. Le volume recule d'une flèche : sinon il tomberait
    // dessus et un clic sur `›` reglerait le son.
    let precedent = RECT {
        left: lecture.right + px(MARGE_FLECHE_G),
        top: haut,
        right: lecture.right + px(MARGE_FLECHE_G) + px(LARGEUR_FLECHE),
        bottom: bas,
    };
    let suivant = RECT {
        left: plein_ecran.left - px(MARGE_FLECHE_D) - px(LARGEUR_FLECHE),
        top: haut,
        right: plein_ecran.left - px(MARGE_FLECHE_D),
        bottom: bas,
    };
    let volume = RECT {
        left: suivant.left - px(MARGE_VOLUME) - px(LARGEUR_VOLUME),
        top: haut,
        right: suivant.left - px(MARGE_VOLUME),
        bottom: bas,
    };
    let temps = RECT {
        left: precedent.right + px(MARGE_TEMPS),
        top: haut,
        right: precedent.right + px(MARGE_TEMPS) + px(LARGEUR_TEMPS),
        bottom: bas,
    };
    let progression = RECT {
        left: temps.right + px(MARGE_PISTE),
        top: haut,
        right: (volume.left - px(MARGE_PISTE_VOLUME)).max(temps.right + px(MARGE_PISTE)),
        bottom: bas,
    };
    Zones { barre: client, lecture, precedent, suivant, temps, progression, volume, plein_ecran }
}

/// Les zones de la barre du lecteur `barre` — voir `zones_dans`, la version pure.
fn zones(barre: HWND) -> Zones {
    let mut client = RECT::default();
    let _ = unsafe { GetClientRect(barre, &mut client) };
    zones_dans(client, echelle(barre))
}

fn dedans(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

/// **LE CARTEL DE SURIMPRESSION DE CE CÔTÉ**, en coordonnées client de L'IMAGE (le
/// repère où il est posé), la version pure — testée, parce qu'un cartel qui recouvre
/// le milieu de l'image vole le clic : cliquer la vidéo met en pause, et le cartel ne
/// le lui rend pas.
///
/// Trois gardes, et ce ne sont pas des goûts :
/// - la largeur est rognée au QUART de l'image : deux cartels prennent la moitié,
///   l'autre moitié reste cliquable, et ils ne se touchent jamais — au plus près, à 96
///   ppp sur une image de 41 px de large, ils laissent 1 px entre eux (un clip 9:16 sur
///   un écran étroit est exactement le cas où la fenêtre est la plus courte,
///   `ajuster_a_la_video` suivant la proportion de la vidéo) ;
/// - une image plus étroite que QUATRE MARGES rend un rectangle VIDÉ (`right <= left`) :
///   les deux cartels n'ont plus place l'un et l'autre ;
/// - une image plus courte que la MOITIÉ du cartel (23 px CSS) rend aussi un rectangle
///   vide, `poser_les_cartels` cachant la fenêtre. Entre cette moitié et la hauteur CSS,
///   le cartel est RACCOURCI à la hauteur de l'image : il tient dedans, et son plancher
///   reste 23 px CSS — jamais « une fenêtre de quelques pixels ».
fn cartel_dans(image: RECT, dpi: i32, a_gauche: bool) -> RECT {
    let px = |v: i32| v * dpi / 96;
    let (largeur, hauteur) = (image.right - image.left, image.bottom - image.top);
    if largeur <= 4 * px(CARTEL_MARGE) || hauteur <= px(CARTEL_H) / 2 {
        return RECT::default();
    }
    let (l, h) = (px(CARTEL_L).min(largeur / 4), px(CARTEL_H).min(hauteur));
    let y = (hauteur - h) / 2;
    if a_gauche {
        RECT { left: px(CARTEL_MARGE), top: y, right: px(CARTEL_MARGE) + l, bottom: y + h }
    } else {
        RECT { left: image.right - px(CARTEL_MARGE) - l, top: y, right: image.right - px(CARTEL_MARGE), bottom: y + h }
    }
}

/// **LA GÉOMÉTRIE ET LA VISIBILITÉ DES DEUX CARTELS.** Appelé par `placer` (l'image
/// change de taille, le plein écran, la barre qui se cache) et par
/// `eclaire_les_cartels` (la souris change d'état).
///
/// ⛔ **REMONTÉS SOUS LA BARRE** : `Some(l.barre)` place chaque cartel juste sous elle
/// dans le Z-ordre, parce qu'en plein écran la barre est posée SUR l'image et qu'un
/// cartel par-dessus elle mangerait le clic de `‹`/`›`, de la piste et du volume — un
/// défaut que la géométrie des zones ne montre pas, et que seul le hit-test verrait.
///
/// ⭐ **Mesuré au harnais (`debug_surimpression`)**, sur deux points : au CENTRE DU
/// CARTEL, `WindowFromPoint` répond `NdLecteurSurAvant` / `NdLecteurSurApres` — le
/// cartel est bien composé par-dessus la chaîne d'échange de libVLC, et un vrai doigt
/// l'atteint (`ShowWindow` seul ne le prouverait pas) ; au centre du bouton lecture, la
/// réponse est `NdLecteurVideoBarre`. ⚠️ **Ces deux points ne se recouvrent jamais** :
/// en fenêtre la barre est posée SOUS l'image, et en plein écran l'image prend toute la
/// hauteur tandis que les cartels sont centrés. La seconde mesure dit donc que la barre
/// est au-dessus de L'IMAGE — pas au-dessus du cartel, dont l'ordre n'est pas
/// mesurable tel quel : il vient du contrat de `SetWindowPos(.., Some(barre), ..)`, qui
/// insère juste sous elle.
///
/// ⚠️ Le rectangle du RGN est `l + 1` / `h + 1` : Windows compte le dernier pixel dans
/// le rectangle d'une région.
fn poser_les_cartels(hwnd: HWND) {
    let Some((image, barre, avant, apres, montres, cachee)) =
        avec(hwnd, |l| (l.image, l.barre, l.avant, l.apres, l.montres, l.cachee))
    else {
        return;
    };
    let mut client = RECT::default();
    let _ = unsafe { GetClientRect(image, &mut client) };
    // ⚠️ L'échelle est lue sur L'IMAGE, la même fenêtre que `etat_des_cartels` et
    // `sous_un_cartel` : le cartel est posé dans les pixels du client de l'image, donc
    // c'est son DPI qui convertit ses mesures CSS — et les trois chemins tombent
    // d'accord sans qu'on suppose rien sur le DPI d'une fenêtre enfant.
    let dpi = echelle(image);
    let px = |v: i32| v * dpi / 96;
    for (fenetre, a_gauche, montre) in [(avant, true, montres.0), (apres, false, montres.1)] {
        let carte = cartel_dans(client, dpi, a_gauche);
        // `cachee` ici, et pas seulement dans `etat_des_cartels` : `cacher` passe par
        // `placer`, et l'état de la souris n'a pas encore été recalculé.
        if carte.right <= carte.left || !montre || cachee {
            let _ = unsafe { ShowWindow(fenetre, SW_HIDE) };
            continue;
        }
        let (l, h) = (carte.right - carte.left, carte.bottom - carte.top);
        // SAFETY : `fenetre` est une fenêtre enfant créée ici même, dans `ouvrir`, sur ce
        // fil ; `rayon` est un objet GDI rendu par `CreateRoundRectRgn`. Réussi,
        // `SetWindowRgn` le POSSEDE (il détruit au passage le RGN qu'il remplace) — il ne
        // faut donc pas le `DeleteObject` nous-mêmes ; refusé, il reste à nous.
        unsafe {
            // Les coins hors du RGN ne sont pas peints : la vidéo les montre, comme le
            // `border-radius` de la page.
            let rayon = CreateRoundRectRgn(0, 0, l + 1, h + 1, px(CARTEL_RAYON) * 2, px(CARTEL_RAYON) * 2);
            // ⚠️ `SetWindowRgn` rend `0` quand il refuse (c'est un BOOL, pas un
            // `Result`) : le RGN n'est alors PAS possédé par la fenêtre, et sans ce
            // `Delete` chaque changement d'état laisserait un objet GDI derrière lui.
            if SetWindowRgn(fenetre, Some(rayon), true) == 0 {
                let _ = DeleteObject(rayon.into());
            }
            let _ = SetWindowPos(fenetre, Some(barre), carte.left, carte.top, l, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
    }
}

/// **L'ÉTAT DE LA SURIMPRESSION, lu sur la position d'écran de la souris** :
/// `(les côtés montrés, les côtés visés)`. La règle est celle de la page : dans la
/// zone d'image les deux chevrons s'allument, celui du côté où la souris va
/// (`ZONE_BORD`) s'allume davantage, et le côté SANS VOISIN ne s'allume pas du tout —
/// une flèche grisée se lit « attendez », alors qu'il n'y a rien à cliquer.
///
/// ⚠️ Le repère est le CLIENT DE L'IMAGE, pas celui de la fenêtre : le cartel y est
/// posé, et en plein écran l'image couvre la barre.
///
/// ⚠️ L'état suit la GÉOMÉTRIE, pas ce que `WindowFromPoint` répondrait : une fenêtre
/// d'une autre application posée sur l'image laisse les cartels allumés alors que le
/// clic n'y arriverait pas. C'est cosmétique (rien n'est cliquable sous une fenêtre
/// d'un autre processus), et le prix de le savoir serait un `WindowFromPoint` par pixel
/// parcouru.
fn etat_des_cartels(l: &Lecteur, ou: POINT) -> ((bool, bool), (bool, bool)) {
    let mut local = ou;
    let mut client = RECT::default();
    unsafe {
        let _ = ScreenToClient(l.image, &mut local);
        let _ = GetClientRect(l.image, &mut client);
    }
    let dedans = local.x >= 0 && local.x < client.right && local.y >= 0 && local.y < client.bottom;
    let (avant, apres) = cotes_de_la_fleche(l);
    let px = |v: i32| v * echelle(l.image) / 96;
    // ⛔ `cachee` : en plein écran la barre et le curseur se cachent après
    // `DELAI_CACHE`, et la surimpression part avec eux — un cartel tout seul posé sur
    // une image « sans interface » serait la seule chose visible de la fenêtre.
    let allumes = dedans && (avant || apres) && !l.cachee;
    let montres = (allumes && avant, allumes && apres);
    (
        montres,
        (montres.0 && local.x <= px(ZONE_BORD), montres.1 && client.right - local.x <= px(ZONE_BORD)),
    )
}

/// **ALLUME OU ÉTEINT les cartels.** ⚠️ RIEN N'EST REPEINT QUAND RIEN NE CHANGE :
/// `WM_MOUSEMOVE` arrive à chaque pixel parcouru et le minuteur sonde la souris
/// vingt-cinq fois par seconde ; `SetWindowRgn` et `InvalidateRect` ne sont payés
/// qu'au changement d'état.
fn eclaire_les_cartels(hwnd: HWND, ou: POINT) {
    // ⚠️ UNE SEULE ENTRÉE DE TABLE : l'état, et les deux cartes. Les prendre dans un
    // second `avec` avec un `unwrap_or(HWND::default())` rendrait `HWND(0)` quand la
    // fenêtre s'est fermée entre les deux, et `InvalidateRect(NULL)` n'est pas « ce
    // cartel » — MSDN lui donne un sens (la fenêtre du thread). Ici, plus de table =
    // on s'arrête.
    let Some((change, avant, apres)) = avec(hwnd, |l| {
        let (montres, vise) = etat_des_cartels(l, ou);
        let change = (l.montres, l.vise) != (montres, vise);
        l.montres = montres;
        l.vise = vise;
        (change, l.avant, l.apres)
    }) else {
        return;
    };
    if !change {
        return;
    }
    poser_les_cartels(hwnd);
    unsafe {
        // Les deux, même quand un seul change : `vise` peut basculer de l'un à l'autre
        // dans le même geste, et repeindre un cartel caché ne coûte rien.
        let _ = InvalidateRect(Some(avant), None, true);
        let _ = InvalidateRect(Some(apres), None, true);
    }
}

/// Le point `(x, y)` du client de l'image est-il sur un cartel **MONTRÉ** ? La version
/// pure de `sous_un_cartel`, testée : un côté ÉTEINT ne retient rien, parce qu'il n'y a
/// rien sous la souris — et c'est précisément le cas où l'interface doit se cacher.
fn sous_le_cartel_dans(client: RECT, dpi: i32, montres: (bool, bool), x: i32, y: i32) -> bool {
    for (a_gauche, montre) in [(true, montres.0), (false, montres.1)] {
        if montre && dedans(&cartel_dans(client, dpi, a_gauche), x, y) {
            return true;
        }
    }
    false
}

/// La souris est-elle posée sur un cartel, dans les coordonnées d'écran ?
fn sous_un_cartel(l: &Lecteur, ou: POINT) -> bool {
    let mut local = ou;
    let mut client = RECT::default();
    unsafe {
        let _ = ScreenToClient(l.image, &mut local);
        let _ = GetClientRect(l.image, &mut client);
    }
    sous_le_cartel_dans(client, echelle(l.image), l.montres, local.x, local.y)
}

/// L'image et la barre : l'une sous l'autre en fenêtre ; en plein écran, l'image
/// prend tout et la barre se pose en bas, par-dessus — visible ou cachée.
fn placer(hwnd: HWND) {
    let mut client = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut client) };
    let h = HAUTEUR_BARRE * echelle(hwnd) / 96;
    avec(hwnd, |l| {
        let par_dessus = l.plein_ecran.is_some();
        let hauteur_image = if par_dessus { client.bottom } else { (client.bottom - h).max(0) };
        let montrer = if l.cachee { SWP_HIDEWINDOW } else { SWP_SHOWWINDOW };
        unsafe {
            let _ = SetWindowPos(l.image, None, 0, 0, client.right, hauteur_image, SWP_NOZORDER | SWP_NOACTIVATE);
            let _ = SetWindowPos(l.barre, Some(HWND_TOP), 0, client.bottom - h, client.right, h, SWP_NOACTIVATE | montrer);
        }
    });
    // Les cartels suivent l'image : même cadre, et la barre cachée en plein écran les
    // emporte (voir `poser_les_cartels`).
    poser_les_cartels(hwnd);
    redessiner(hwnd);
}

/// Montre ou cache la barre et le curseur (plein écran).
fn cacher(hwnd: HWND, cachee: bool) {
    let change = avec(hwnd, |l| std::mem::replace(&mut l.cachee, cachee) != cachee).unwrap_or(false);
    if change {
        placer(hwnd);
        // Le curseur suit la barre : sans attendre un `WM_SETCURSOR`, que la
        // fenêtre de VLC posée dans l'image n'envoie pas toujours.
        unsafe { SetCursor(if cachee { None } else { LoadCursorW(None, IDC_ARROW).ok() }) };
    }
}

/// La souris a bougé : la barre revient, le compte à rebours repart.
fn la_souris_bouge(hwnd: HWND, ou: POINT) {
    let bouge = avec(hwnd, |l| {
        let bouge = (l.bouge.1.x, l.bouge.1.y) != (ou.x, ou.y);
        if bouge {
            l.bouge = (Instant::now(), ou);
        }
        bouge
    })
    .unwrap_or(false);
    if bouge {
        cacher(hwnd, false);
    }
}

/// À chaque tour du minuteur, en plein écran : la souris a-t-elle bougé (lue à
/// l'écran — l'image de VLC ne relaie pas toujours ses mouvements), et sinon,
/// est-il temps de cacher la barre ? Jamais pendant un glissé ni sous la souris.
fn surveiller_la_souris(hwnd: HWND) {
    if !avec(hwnd, |l| l.plein_ecran.is_some()).unwrap_or(false) {
        return;
    }
    let mut ou = POINT::default();
    let _ = unsafe { GetCursorPos(&mut ou) };
    la_souris_bouge(hwnd, ou);
    let Some((calme, glisse, barre, sous_un_cartel)) =
        avec(hwnd, |l| (l.bouge.0.elapsed(), l.glisse.is_some(), l.barre, sous_un_cartel(l, ou)))
    else {
        return;
    };
    let mut local = ou;
    let _ = unsafe { ScreenToClient(barre, &mut local) };
    // La barre, OU un cartel montré : sans le second, le calme de `DELAI_CACHE`
    // éteindrait la surimpression SOUS la souris qui la tient, et la moindre
    // secousse la rallumerait — un clignotement en plein écran.
    if calme >= DELAI_CACHE && !glisse && !sous_un_cartel && !dedans(&zones(barre).barre, local.x, local.y) {
        cacher(hwnd, true);
    }
}

/// Le lecteur de `hwnd`, s'il existe encore.
///
/// ⛔ **LE `borrow_mut` EST TENU PENDANT TOUTE LA CLOSURE.** `f` ne doit donc appeler
/// AUCUNE fonction qui rentre dans `avec` (`poser_les_cartels`, `placer`, `cacher`,
/// `redessiner`, `eclaire_les_cartels`, `peindre`, `joueur`, `sous_un_cartel` sur une
/// autre fenêtre…) ni rien qui pompe des messages. Une closure qui le fait rend
/// « already mutably borrowed », et `panic = "abort"` en release (Cargo.toml) transforme
/// cet abort en `extern "system"` en crash du processus qu'on ne peut pas tuer — c'est
/// arrivé deux fois dans ce fichier. Les closures d'ici ne font que copier des champs ou
/// appeler des Win32 qui ne dispatchent pas (`ScreenToClient`, `GetClientRect`,
/// `GetDpiForWindow`) ; ce qui MODIFIE une fenêtre (`SetWindowPos`, `ShowWindow`,
/// `SetWindowRgn`, `InvalidateRect`, `SetFocus`, `SetWindowTextW`, `SetCursor`) est
/// toujours APRÈS le retour de `avec`.
fn avec<R>(hwnd: HWND, f: impl FnOnce(&mut Lecteur) -> R) -> Option<R> {
    LECTEURS.with(|l| l.borrow_mut().get_mut(&(hwnd.0 as isize)).map(f))
}

fn joueur(hwnd: HWND) -> Option<(*mut c_void, &'static Api)> {
    let api = api().ok()?;
    avec(hwnd, |l| l.joueur as *mut c_void).map(|j| (j, api))
}

fn basculer_la_lecture(hwnd: HWND) {
    let Some((j, api)) = joueur(hwnd) else { return };
    unsafe {
        let etat = (api.get_state)(j);
        if etat == ETAT_FINI || etat == ETAT_ERREUR {
            // Fini : on rejoue depuis le début.
            (api.stop)(j);
            (api.play)(j);
        } else {
            (api.set_pause)(j, (api.is_playing)(j));
        }
    }
    redessiner(hwnd);
}

/// **LE MÉDIA DU NEUVEAU, PUIS L'ANCIEN DÉTACHÉ, PUIS LE NEUVEAU BRANCHÉ** — les
/// deux ordres sont des gardes, pas des préférences ; `changer_de_media` les
/// explique, ce découpage ne fait que les rendre lisibles.
///
/// ⛔ **L'ANCIEN DÉTACHÉ DE `image` AVANT QUE LE NEUVEAU L'OCCUPE.** Le plan
/// d'avant créait le nouveau lecteur, le branchait sur `image`, puis lâchait
/// l'ancien. Les deux joueurs de libVLC se branchent sur le MÊME HWND et libVLC
/// sous-classe ce parent ; `RemoveWindowSubclass` matche par `(proc, id)`, pas par
/// instance — donc lâcher l'ancien pouvait retirer le crochet du nouveau, et la
/// fenêtre restait sans image pendant que le son continuait. `stop` détruit la
/// sortie vidéo de l'ancien, donc son sous-classement : à ce moment-là il ne reste
/// qu'un occupant de `image`. ⚠️ **NON PHOTOGRAPHIABLE** (fenêtre exclue de la
/// capture externe) : ce qui vérifie, c'est le journal de libVLC sur stderr.
///
/// Rend le pointeur `libvlc_media_player_t *` du nouveau lecteur, **déjà en
/// lecture**. L'ancien est arrêté mais PAS lâché ici : c'est l'appelant qui le
/// lâche, après l'avoir remplacé dans `LECTEURS`, pour que la table ne porte jamais
/// un pointeur lâché (`peindre` et le minuteur interrogent `l.joueur`).
///
/// ⚠️ **NI `media_new_path` NI `play` NE VOIENT UN MAUVAIS FICHIER** — mesuré le
/// 2026-11-03 sur la DLL embarquée : un `.mp4` de 4096 octets de bruit et un
/// `.mp4` QUI N'EXISTE PAS donnent tous deux un média valide et un `play` à **0**
/// (journal : « lecture ouverte »). Ce qui le dit, c'est l'ÉTAT du lecteur :
/// `peindre` affiche « Lecture impossible » quand il tombe à `ETAT_ERREUR`. Ce
/// repli-ci n'attrape donc que les échecs où `play` répond autre chose que 0 (sortie
/// absente, instance morte) — c'est un garde, pas la détection du fichier cassé.
/// Sur ce chemin, l'ancien **repart** et la raison est dite, au journal et au titre :
/// la fenêtre est exclue de la capture externe, et le journal seul est le « clic qui
/// n'obtient rien sans un mot » que ce dépôt refuse. La version d'avant promettait
/// de détecter le refus sur le média, avant le `stop` : elle lâchait l'ancien et
/// laissait une fenêtre muette, en répondant `Ok`.
///
/// ⚠️ **L'ANCIEN EST ARRÊTÉ AVANT QUE LE NEUF JOUE** (et non lâché) : voir le
/// sous-classage plus haut. Un lecteur qui n'a jamais joué n'a jamais sous-classé
/// `image`, donc le relâcher ne peut pas retirer le crochet d'un autre.
///
/// SAFETY : `api` est la table chargée une fois (`Api`), `ancien` est le pointeur
/// que `LECTEURS` tient pour cette fenêtre, `image` un HWND enfant vivant tant que
/// la fenêtre l'est. Aucun des deux joueurs n'est relâché ailleurs pendant cet appel
/// (l'ancien l'est par l'appelant, ici même) : tout se tient dans la boucle de
/// messages de la fenêtre, le minuteur ne s'interleave pas. ⚠️ `stop` et `play` sont
/// BLOQUANTS : un fichier sur un partage lent tient la boucle de messages pendant
/// leur démarrage — c'est déjà le cas à l'ouverture, et le remplacement n'y ajoute
/// qu'un second démarrage.
fn remplacer_le_lecteur(
    api: &Api,
    ancien: usize,
    image: HWND,
    volume: i32,
    utf8: &CString,
) -> Result<*mut c_void, String> {
    unsafe {
        let media = (api.media_new_path)(api.instance as *mut c_void, utf8.as_ptr());
        if media.is_null() {
            return Err("VLC refuse ce fichier".to_string());
        }
        let joueur = (api.lecteur_depuis_media)(media);
        (api.media_release)(media);
        if joueur.is_null() {
            return Err("VLC ne peut pas lire ce fichier".to_string());
        }
        // L'ancien quitte `image` (et son sous-classement) avant que le neuf l'occupe.
        (api.stop)(ancien as *mut c_void);
        (api.souris)(joueur, 0);
        (api.clavier)(joueur, 0);
        (api.set_hwnd)(joueur, image.0);
        (api.set_volume)(joueur, volume);
        if (api.play)(joueur) != 0 {
            (api.lecteur_release)(joueur);
            // La vidéo d'avant continue : `stop` lui a retiré sa sortie, donc il
            // faut lui rendre `image` avant de la relancer.
            (api.set_hwnd)(ancien as *mut c_void, image.0);
            if (api.play)(ancien as *mut c_void) != 0 {
                crate::diag!("[lecteur_video] le lecteur precedent ne repart pas non plus");
            }
            return Err("la lecture ne demarre pas".to_string());
        }
        Ok(joueur)
    }
}

/// ⭐ **LA FENÊTRE SE ROUVRE SUR UNE AUTRE VIDÉO, SANS CHANGER DE FENÊTRE** —
/// DEMANDE EXPLICITE (2026-11-03) : « des flèches en surbrillance pour passer à la
/// vidéo suivante ». La taille, la position, le plein écran et le volume restent
/// acquis : seul le média repart, posé sur le même `image` que VLC occupait déjà.
/// Recréer la fenêtre perdrait le cadre que l'utilisateur a réglé, et son plein
/// écran avec.
///
/// ⚠️ **RIEN N'EST TOUCHÉ TANT QUE LE NEUF N'A PAS DÉMARRÉ** : `Api` ne porte pas
/// `libvlc_media_player_set_media` (la table n'est faite que des symboles réellement
/// trouvés dans la DLL), donc le média se refait. Sur le seul échec que
/// `remplacer_le_lecteur` puisse voir (`play` autre que 0), l'ancien repart, le titre
/// et le chemin restent les anciens, et la raison est dite. **Un fichier cassé ou
/// disparu n'est pas cet échec** — ni `media_new_path` ni `play` ne le voient (mesuré,
/// dans la doc de `remplacer_le_lecteur`) : c'est l'état du lecteur qui l'affiche, en
/// barre, par « Lecture impossible ».
///
/// ⚠️ **LA SÉRIE NE CHANGE PAS** : la flèche reste dans le même dossier et le même
/// genre, donc le rang du nouveau fichier se lit sur la série déjà retenue. Zéro
/// lecture de disque ici — c'est pour ça qu'elle est retenue à l'ouverture.
fn changer_de_media(hwnd: HWND, nouveau: &Path) -> Result<(), String> {
    let api = api()?;
    let Some((ancien, image, volume)) = avec(hwnd, |l| (l.joueur, l.image, l.volume)) else {
        return Err("la fenetre de lecture n'existe plus".to_string());
    };
    let utf8 = CString::new(nouveau.to_string_lossy().as_bytes()).map_err(|_| "Chemin de fichier invalide".to_string())?;
    let titre = nouveau.file_name().map_or_else(|| nouveau.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
    let joueur = match remplacer_le_lecteur(api, ancien, image, volume, &utf8) {
        Ok(joueur) => joueur,
        Err(raison) => {
            // ⛔ **LA FENÊTRE LE DIT ELLE-MÊME.** Elle est exclue de la capture
            // externe et son journal ne se voit pas : un clic qui n'obtient rien
            // sans un mot est le pire des cas, et c'était le défaut d'avant — une
            // fenêtre muette à 0:00, sans raison. Le nom du fichier qui n'a pas pu
            // démarrer passe au titre, et la vidéo d'avant continue (voir
            // `remplacer_le_lecteur`).
            crate::diag!("[lecteur_video] fleche vers {titre} : {raison}");
            unsafe {
                let _ = SetWindowTextW(hwnd, &HSTRING::from(format!("{titre} : {raison} — NeuraDesk")));
            }
            redessiner(hwnd);
            return Err(raison);
        }
    };
    avec(hwnd, |l| {
        l.joueur = joueur as usize;
        l.chemin = nouveau.to_path_buf();
        // ⛔ **LE GLISSÉ ET LA FRACTION VISÉE SE VIDANT ICI.** La barre tient
        // `SetCapture` pendant un glissé de la progression, et `WM_KEYDOWN` est
        // relayé depuis la barre : tenir `Ctrl`+`→` pendant qu'on glisse garderait
        // la fraction de l'ANCIENNE vidéo, `peindre` afficherait
        // `longueur_nouvelle × visee_ancienne`, et le premier `WM_MOUSEMOVE`
        // suivants la poserait sur la nouvelle — la position saute sous la main.
        l.glisse = None;
        l.visee = None;
        // ⚠️ **`ajustee` RESTE VRAIE — la fenêtre ne bouge pas d'un cran.**
        // `ouvrir` a déjà pris la proportion de la première vidéo ; relancer
        // `ajuster_a_la_video` à chaque flèche la redimensionnerait ET la
        // re-centrerait (la fonction déplace la fenêtre, pas seulement sa
        // taille) : défiler dix clips ferait voyager la fenêtre sous la main.
        // VLC lettre-boxe la nouvelle dans le cadre gardé — c'est ce que fait
        // tout lecteur quand on change de fichier.
        l.ajustee = true;
    });
    // ⛔ **L'ANCIEN N'EST LÂCHÉ QU'ICI** : `l.joueur` porte le neuf depuis le bloc
    // ci-dessus, donc `LECTEURS` n'a jamais porté un pointeur relâché — le minuteur
    // de 250 ms et `peindre` (quatre par seconde) interrogent `l.joueur`, et un
    // `get_length` sur mémoire lâchée ne se signalerait pas.
    unsafe {
        (api.lecteur_release)(ancien as *mut c_void);
        let _ = SetWindowTextW(hwnd, &HSTRING::from(format!("{titre} — NeuraDesk")));
    }
    crate::diag!("[lecteur_video] fleche : {titre} en lecture");
    // ⚠️ **LES CARTELS SE RALLUMENTENT ICI** : le nouveau fichier peut être le DERNIER
    // de la série, et son cartel de droite doit s'éteindre aussitôt — sinon il reste
    // posé sur l'image, cliquable, et le clic n'obtient rien d'autre qu'une ligne au
    // journal (« aucun voisin annoncé »), alors que la barre, elle, s'est éteinte.
    eclaire_les_cartels(hwnd, souris_ecran());
    // ⚠️ `poser_les_cartels` AUSSI, et pas seulement l'état : `eclaire_les_cartels`
    // s'arrête quand l'état n'a pas changé, et un média neuf remplace le lecteur qui
    // peint dans `image`. Le Z-ordre des cartes ne doit donc pas dépendre du survol.
    poser_les_cartels(hwnd);
    redessiner(hwnd);
    Ok(())
}

/// **LA FLÈCHE DU LECTEUR.** Pas de voisin annoncé = rien à faire : la barre l'a
/// déjà éteinte, et le journal le dit (le disque a pu changer entre le rendu et le
/// clic). Un voisin qui ne se lit pas est dit PAR LA FENÊTRE, dans
/// `changer_de_media` — pas seulement au journal.
fn aller_au_voisin(hwnd: HWND, suivant: bool) {
    let annonce = if suivant { "suivant" } else { "precedent" };
    let Some(nouveau) = avec(hwnd, |l| voisin_de_la_fleche(l, suivant)).flatten() else {
        crate::diag!("[lecteur_video] {annonce} : aucun voisin annonce");
        return;
    };
    // `changer_de_media` a déjà dit la raison — au journal ET au titre de la fenêtre.
    let _ = changer_de_media(hwnd, &nouveau);
}

/// **UNE FLÈCHE DE VOISINAGE, SANS SOURIS NI `SendInput`** — l'appel DIRECT de
/// `aller_au_voisin`, la même fonction que la branche de clic sur `‹`/`›` de la barre
/// et que `Ctrl`+`←`/`→` (voir la doc du module). Réservé au harnais de vérification
/// (`NEURADESK_DEBUG_OPEN=lecteur`) : cette fenêtre est peinte en GDI et exclue de
/// toute capture externe (`WDA_EXCLUDEFROMCAPTURE`), donc le seul moyen d'éprouver le
/// changement de média est de le DEMANDER et de lire le journal — ce que
/// `changer_de_media` y écrit (`[lecteur_video] fleche : <titre> en lecture`).
///
/// Rend `true` si un lecteur répondait. `Faux` veut dire que la fenêtre n'était pas
/// ouverte à la frame du levier, ou qu'elle n'est pas celle du harnais (voir
/// `lecteur_du_harnais`) — pas que la flèche est cassée.
pub fn debug_fleche(suivant: bool) -> bool {
    let Some(hwnd) = lecteur_du_harnais("debug_fleche") else {
        return false;
    };
    aller_au_voisin(hwnd, suivant);
    true
}

/// ⚠️ **LE LECTEUR QUE LE HARN AIS A OUVERT, PAS « LE PREMIER VENU ».** `LECTEURS` est une
/// HashMap et l'application est VIVANTE pendant un run de capture : `keys().next()`
/// pouvait tomber sur une fenêtre que l'utilisateur a ouverte dans le dash dossier, et le
/// clic POSTÉ de `debug_surimpression` changeait alors un média de SON écran. Le levier
/// `NEURADESK_DEBUG_FICHIER` dit lequel est au harnais — `app.rs` ouvre le lecteur depuis
/// cette même variable — et `sous_dock::est_le_meme_fichier` tolère les formes du chemin.
/// `None` : la mesure s'arrête et le dit, au lieu de frapper une fenêtre qui n'est pas la
/// sienne.
fn lecteur_du_harnais(de_quoi: &str) -> Option<HWND> {
    let Some(demande) = std::env::var("NEURADESK_DEBUG_FICHIER").ok().map(PathBuf::from) else {
        crate::diag!("[lecteur_video] {de_quoi} : NEURADESK_DEBUG_FICHIER est vide — aucun lecteur désigné");
        return None;
    };
    let connus = LECTEURS.with(|l| {
        l.borrow()
            .iter()
            .filter(|(_, lecteur)| crate::win32::sous_dock::est_le_meme_fichier(&lecteur.chemin, &demande))
            .map(|(hwnd, _)| HWND(*hwnd as *mut c_void))
            .collect::<Vec<HWND>>()
    });
    match connus.as_slice() {
        [] => {
            crate::diag!("[lecteur_video] {de_quoi} : aucun lecteur de {} ouvert", demande.display());
            None
        }
        [seul] => Some(*seul),
        plusieurs => {
            crate::diag!("[lecteur_video] {de_quoi} : {} lecteurs du même fichier — mesure ambiguë, rien n'est posté", plusieurs.len());
            None
        }
    }
}

/// **L'ÉPREUVE D'UN CARTEL DE SURIMPRESSION, sans souris ni `SendInput`** — la fenêtre
/// est exclue de la capture externe, donc le cartel ne se photographie pas : l'épreuve
/// est au journal, en trois temps, et le deuxième est le seul qui compte.
///
/// 1. les DEUX RECTS du cartel, dans le client de l'image ;
/// 2. ⭐ `WindowFromPoint` au CENTRE du cartel visé : le hit-test RÉEL de Windows. C'est
///    lui qui dit si un vrai doigt l'atteint — `ShowWindow` seul ne le prouverait pas,
///    parce qu'un cartel affiché mais DERRIÈRE la chaîne d'échange de libVLC se voit
///    sans se cliquer. Un second point, pris sur le bouton lecture de la barre, dit que
///    la barre répond au-dessus de L'IMAGE — ⚠️ ces deux points ne se recouvrent jamais
///    (en fenêtre la barre est sous l'image, en plein écran les cartels sont centrés),
///    donc l'ordre cartel/barre N'EST PAS mesuré ici : il vient du contrat de
///    `SetWindowPos(.., Some(barre), ..)` ;
/// 3. le clic POSTÉ (`WM_LBUTTONDOWN`) au cartel, le message du vrai geste — la preuve
///    est la ligne que `changer_de_media` écrit derrière.
///
/// ⚠️ L'état est calculé sur un POINT SYNTHÉTIQUE au centre du cartel, comme si la souris
/// y était : `GetCursorPos` répondrait n'importe où ailleurs, et le harnais ne déplace
/// rien et ne clique rien.
///
/// ⚠️ **Le lecteur désigné par `NEURADESK_DEBUG_FICHIER`** (voir `lecteur_du_harnais`) :
/// le clic posté ne doit jamais tomber sur une fenêtre ouverte par l'utilisateur pendant
/// le run.
///
/// Rend `true` si le cartel a été TROUVÉ DESSUS et si le message a été posté — ce n'est
/// PAS « un voisin a changé de média » : la borne de la série rend `false` (le cartel de
/// ce côté est éteint, `WindowFromPoint` répond l'image), et un `true` suivi d'aucune
/// ligne `fleche :` reste à vérifier dans le journal.
pub fn debug_surimpression(suivant: bool) -> bool {
    let Some(hwnd) = lecteur_du_harnais("debug_surimpression") else {
        return false;
    };
    let Some((image, barre, avant, apres)) = avec(hwnd, |l| (l.image, l.barre, l.avant, l.apres)) else {
        crate::diag!("[lecteur_video] debug_surimpression : lecteur connu mais absent");
        return false;
    };
    // ⚠️ **NOTRE FENÊTRE EST REMONTÉE (`HWND_TOP`, PAS `HWND_TOPMOST`) AVANT DE SONDER.** En
    // fenêtre le lecteur n'est pas topmost, et un run de capture dure ~48 s : le temps qu'une
    // fenêtre d'UNE AUTRE APPLICATION se pose par-dessus (mesuré trois fois : WhatsApp.Root,
    // un `HwndWrapper` tiers, Chrome), et `WindowFromPoint` répond ALORS CETTE FENÊTRE-LÀ aux
    // deux points — la mesure ne dirait plus rien du cartel, et « LE CARTEL N'EST PAS DESSUS »
    // serait lu comme un défaut de la surimpression. Remonter SA PROPRE fenêtre ne simule
    // aucun geste ; si une fenêtre tierce répond encore après ça, elle est vraiment au-
    // dessus (topmost) et la classe + le PID le disent.
    let _ = unsafe { SetWindowPos(hwnd, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
    let mut client = RECT::default();
    let _ = unsafe { GetClientRect(image, &mut client) };
    // ⚠️ L'échelle de L'IMAGE, comme les trois chemins de production (`poser_les_cartels`,
    // `etat_des_cartels`, `sous_un_cartel`) : mesurer le cartel avec le DPI du PARENT
    // rendrait des rectanges que le code réel ne pose jamais, et un
    // « LE CARTEL N'EST PAS DESSUS » faux sur un écran secondaire à autre échelle.
    let dpi = echelle(image);
    let (gauche, droite) = (cartel_dans(client, dpi, true), cartel_dans(client, dpi, false));
    crate::diag!(
        "[lecteur_video] surimpression : image {}x{}, avant=({},{}) {}x{}, apres=({},{}) {}x{}",
        client.right,
        client.bottom,
        gauche.left,
        gauche.top,
        gauche.right - gauche.left,
        gauche.bottom - gauche.top,
        droite.left,
        droite.top,
        droite.right - droite.left,
        droite.bottom - droite.top,
    );
    // Le centre du cartel visé, en coordonnées d'écran : le point où la souris serait.
    let cible = if suivant { droite } else { gauche };
    if cible.right <= cible.left {
        crate::diag!("[lecteur_video] surimpression : cartel trop petit pour être éprouvé");
        return false;
    }
    let (mut centre, mut coin) = (
        POINT { x: (cible.left + cible.right) / 2, y: (cible.top + cible.bottom) / 2 },
        POINT { x: cible.left, y: cible.top },
    );
    unsafe {
        let _ = ClientToScreen(image, &mut centre);
        let _ = ClientToScreen(image, &mut coin);
    }
    eclaire_les_cartels(hwnd, centre);
    let attendu = cible_hwnd(suivant, avant, apres);
    let (sous, classe, pid) = sous_le_point(centre);
    let (montres, vise) = avec(hwnd, |l| (l.montres, l.vise)).unwrap_or(((false, false), (false, false)));
    crate::diag!(
        "[lecteur_video] surimpression : WindowFromPoint({}, {}) = {:?} « {} » pid {pid}, attendu {:?} (côté {}, notre pid {}), montres={:?} vise={:?}",
        centre.x,
        centre.y,
        sous,
        classe,
        attendu,
        if suivant { "apres" } else { "avant" },
        std::process::id(),
        montres,
        vise,
    );
    // ⚠️ **Et si le cartel est MONTRÉ — mesure directe, qui ne dépend de rien d'autre à
    // l'écran.** Le hit-test, lui, peut être bloqué par une fenêtre TIERCE TOPMOST (mesuré :
    // `WhatsApp.Root` recouvrait les deux points avec un autre pid) : « LE CARTEL N'EST PAS
    // DESSUS » se lirait alors comme un défaut de la surimpression. Une fenêtre CACHÉE ne peut
    // pas recevoir le clic, donc la borne de la série se prouve ici : le cartel du côté sans
    // voisin est caché, et le clic de ce point va à l'image — qui met en pause.
    let (vu_avant, vu_apres) = (unsafe { IsWindowVisible(avant).as_bool() }, unsafe { IsWindowVisible(apres).as_bool() });
    crate::diag!("[lecteur_video] surimpression : cartels affichés = avant {vu_avant}, apres {vu_apres} — un cartel caché ne peut pas recevoir le clic");
    let z = zones(barre);
    let mut sur_barre = POINT { x: (z.lecture.left + z.lecture.right) / 2, y: (z.lecture.top + z.lecture.bottom) / 2 };
    unsafe {
        let _ = ClientToScreen(barre, &mut sur_barre);
    }
    let (sous_barre, classe_barre, pid_barre) = sous_le_point(sur_barre);
    crate::diag!("[lecteur_video] surimpression : sous le bouton lecture de la barre = {:?} « {} » pid {pid_barre}, attendu {:?}", sous_barre, classe_barre, barre);
    if sous != attendu {
        crate::diag!("[lecteur_video] surimpression : LE CARTEL N'EST PAS DESSUS — le clic n'est pas posté");
        return false;
    }
    // Le message du vrai geste, en coordonnées CLIENT du cartel (fenêtre enfant sans
    // bordure : son origine client est `coin`, à l'écran).
    let (x, y) = (centre.x - coin.x, centre.y - coin.y);
    let lparam = LPARAM(((x as u32 as usize) | ((y as u32 as usize) << 16)) as isize);
    crate::diag!("[lecteur_video] surimpression : WM_LBUTTONDOWN posté au cartel à ({x},{y}) client");
    let _ = unsafe { PostMessageW(Some(attendu), WM_LBUTTONDOWN, WPARAM(0), lparam) };
    true
}

/// Le cartel qui mène au voisin `suivant` : `avant` pointe vers la GAUCHE et mène au
/// précédent, `apres` vers la droite et au suivant (voir `wndproc_surimpression`).
fn cible_hwnd(suivant: bool, avant: HWND, apres: HWND) -> HWND {
    if suivant {
        apres
    } else {
        avant
    }
}

/// Ce que `WindowFromPoint` trouve à un point d'écran, AVEC sa classe et son PID.
/// ⚠️ Le seul `HWND` ne suffit pas à lire la mesure : « le cartel n'est pas dessus » a
/// DEUX causes très différentes — le cartel resté derrière la chaîne d'échange de
/// libVLC (le défaut qu'on cherche), ou une AUTRE fenêtre posée sur la fenêtre de
/// lecture, la nôtre ou celle d'un voisin. Le point est mesuré À L'ÉCRAN : le harnais
/// ne remonte rien, et le lecteur n'est au premier plan que ce que le shell lui laisse.
fn sous_le_point(ou: POINT) -> (HWND, String, u32) {
    let sous = unsafe { WindowFromPoint(ou) };
    let mut nom = [0u16; 64];
    let n = unsafe { GetClassNameW(sous, &mut nom) }.max(0) as usize;
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(sous, Some(&mut pid)) };
    (sous, String::from_utf16_lossy(&nom[..n]), pid)
}

fn sauter(hwnd: HWND, ecart_ms: i64) {
    let Some((j, api)) = joueur(hwnd) else { return };
    unsafe {
        let (t, longueur) = ((api.get_time)(j), (api.get_length)(j));
        if longueur > 0 {
            (api.set_time)(j, (t + ecart_ms).clamp(0, longueur - 1));
        }
    }
    redessiner(hwnd);
}

fn chercher_a(hwnd: HWND, fraction: f32) {
    let Some((j, api)) = joueur(hwnd) else { return };
    unsafe {
        let longueur = (api.get_length)(j);
        if longueur > 0 {
            (api.set_time)(j, ((longueur as f32 * fraction.clamp(0.0, 1.0)) as i64).min(longueur - 1));
        }
    }
}

fn regler_volume(hwnd: HWND, volume: i32) {
    let Some((j, api)) = joueur(hwnd) else { return };
    let volume = volume.clamp(0, 100);
    avec(hwnd, |l| l.volume = volume);
    unsafe { (api.set_volume)(j, volume) };
    retenir_volume(volume);
    redessiner(hwnd);
}

fn fraction_dans(r: &RECT, x: i32) -> f32 {
    ((x - r.left) as f32 / (r.right - r.left).max(1) as f32).clamp(0.0, 1.0)
}

fn basculer_le_plein_ecran(hwnd: HWND) {
    let avant = avec(hwnd, |l| {
        l.cachee = false;
        l.bouge.0 = Instant::now();
        l.plein_ecran.take()
    })
    .flatten();
    unsafe {
        match avant {
            Some((style, cadre)) => {
                SetWindowLongPtrW(hwnd, GWL_STYLE, style);
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_NOTOPMOST),
                    cadre.left,
                    cadre.top,
                    cadre.right - cadre.left,
                    cadre.bottom - cadre.top,
                    SWP_FRAMECHANGED | SWP_NOOWNERZORDER,
                );
                SetCursor(LoadCursorW(None, IDC_ARROW).ok());
                noter_plein_ecran(hwnd, false);
            }
            None => {
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
                let mut cadre = RECT::default();
                let _ = GetWindowRect(hwnd, &mut cadre);
                let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                let _ = GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info);
                let ecran = info.rcMonitor;
                avec(hwnd, |l| l.plein_ecran = Some((style, cadre)));
                noter_plein_ecran(hwnd, true);
                SetWindowLongPtrW(hwnd, GWL_STYLE, style & !(WS_OVERLAPPEDWINDOW.0 as isize));
                // Au-dessus de tout, barres du dock comprises.
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    ecran.left,
                    ecran.top,
                    ecran.right - ecran.left,
                    ecran.bottom - ecran.top,
                    SWP_FRAMECHANGED | SWP_NOOWNERZORDER,
                );
            }
        }
    }
    // `WM_SIZE` suit, mais pas toujours (même taille) : la barre doit changer de
    // place dans tous les cas.
    placer(hwnd);
}

/// La première image venue, la fenêtre prend la proportion de la vidéo (la
/// largeur gardée, à 85 % au plus de la hauteur de travail).
fn ajuster_a_la_video(hwnd: HWND) {
    let Some((j, api)) = joueur(hwnd) else { return };
    if avec(hwnd, |l| l.ajustee || l.plein_ecran.is_some()).unwrap_or(true) {
        return;
    }
    let (mut lv, mut hv) = (0u32, 0u32);
    if unsafe { (api.taille_video)(j, 0, &mut lv, &mut hv) } != 0 || lv == 0 || hv == 0 {
        return;
    }
    avec(hwnd, |l| l.ajustee = true);
    let (mut cadre, mut client) = (RECT::default(), RECT::default());
    unsafe {
        let _ = GetWindowRect(hwnd, &mut cadre);
        let _ = GetClientRect(hwnd, &mut client);
    }
    let travail = zone_de_travail(unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) });
    let (bord_l, bord_h) = ((cadre.right - cadre.left) - client.right, (cadre.bottom - cadre.top) - client.bottom);
    let barre = HAUTEUR_BARRE * echelle(hwnd) / 96;
    // ⛔ **LA LARGEUR NE DESCEND PAS SOUS LE BUDGET DE LA BARRE** (voir
    // `LARGEUR_MIN_CSS`). Suivre la proportion de la vidéo sans ce garde rogne la
    // fenêtre jusqu'à ce que l'heure se peigne sous le volume ; borné aussi par la
    // zone de travail, pour qu'un écran plus étroit que le plancher ne la fasse
    // pas sortir du bord.
    let mut largeur = client.right.max(largeur_min(echelle(hwnd))).min(travail.right - travail.left);
    let mut hauteur_image = (largeur as i64 * hv as i64 / lv as i64) as i32;
    let plafond = (travail.bottom - travail.top) * 85 / 100 - barre - bord_h;
    if hauteur_image > plafond {
        hauteur_image = plafond.max(1);
        largeur = (hauteur_image as i64 * lv as i64 / hv as i64) as i32;
    }
    let (l, h) = (largeur + bord_l, hauteur_image + barre + bord_h);
    let centre = ((cadre.left + cadre.right) / 2, (cadre.top + cadre.bottom) / 2);
    let x = (centre.0 - l / 2).clamp(travail.left, (travail.right - l).max(travail.left));
    let y = (centre.1 - h / 2).clamp(travail.top, (travail.bottom - h).max(travail.top));
    let _ = unsafe { SetWindowPos(hwnd, None, x, y, l, h, SWP_NOZORDER | SWP_NOACTIVATE) };
}

fn redessiner(hwnd: HWND) {
    if let Some(barre) = avec(hwnd, |l| l.barre) {
        let _ = unsafe { InvalidateRect(Some(barre), None, false) };
    }
}

fn format_temps(ms: i64) -> String {
    let s = (ms.max(0) / 1000) as u64;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

unsafe fn remplir(dc: HDC, r: RECT, couleur: u32) {
    let pinceau = unsafe { CreateSolidBrush(COLORREF(couleur)) };
    unsafe {
        FillRect(dc, &r, pinceau);
        let _ = DeleteObject(pinceau.into());
    }
}

/// La GÉOMÉTRIE d'un chevron dans `r` : le centre, le demi-height, la pointe et la
/// base du triangle. Partagée par `fleche` (les boutons de la barre) et par `chevron`
/// (le cartel de surimpression), pour que les deux surfaces dessinent le MÊME triangle
/// au lieu de deux versions qui divergent.
struct Chevron {
    cy: i32,
    h: i32,
    pointe: i32,
    base: i32,
}

fn chevron_dans(r: RECT, vers_la_gauche: bool) -> Chevron {
    let cx = (r.left + r.right) / 2;
    let h = ((r.bottom - r.top) / 3).max(4);
    let (pointe, base) = if vers_la_gauche { (cx - h, cx + h * 2 / 3) } else { (cx + h, cx - h * 2 / 3) };
    Chevron { cy: (r.top + r.bottom) / 2, h, pointe, base }
}

/// Le TRIANGLE PLEIN seul, sans la barre du « saut au bord » : le glyph du bouton
/// `‹`/`›` de la barre, qui occupe le tiers de sa boîte. Le cartel de surimpression,
/// lui, peint un caret tracé fin aux mesures de la page (`caret`) — voir `CARET_H`.
unsafe fn chevron(dc: HDC, r: RECT, vers_la_gauche: bool, couleur: u32) {
    let c = chevron_dans(r, vers_la_gauche);
    polygone(dc, &[POINT { x: c.pointe, y: c.cy - c.h }, POINT { x: c.base, y: c.cy }, POINT { x: c.pointe, y: c.cy + c.h }], couleur);
}

/// **UNE FLÈCHE DE VOISINAGE** : un triangle, et la barre du « saut au bord » du
/// côté opposé à la pointe. Dessinée à la main comme tout le reste de la barre —
/// VLC peint dans `image`, pas ici.
unsafe fn fleche(dc: HDC, r: RECT, vers_la_gauche: bool, couleur: u32) {
    let c = chevron_dans(r, vers_la_gauche);
    chevron(dc, r, vers_la_gauche, couleur);
    let (barre, largeur) = (if vers_la_gauche { c.base } else { c.base - c.h / 4 }, (c.h / 4).max(2));
    remplir(dc, RECT { left: barre, top: c.cy - c.h, right: barre + largeur, bottom: c.cy + c.h }, couleur);
}

/// **LES TROIS POINTS DU CARET DU CARTEL** — la version pure, testée : deux ailes et la
/// pointe, aux mesures de la page (`CARET_H`, `CARET_L`), rognées au TIERS de la boîte.
/// Un cartel raccourci à la moitié de sa hauteur CSS (`cartel_dans`) garde donc un caret
/// qui tient dedans.
fn caret_dans(r: RECT, dpi: i32, vers_la_gauche: bool) -> [POINT; 3] {
    let px = |v: i32| v * dpi / 96;
    let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    let h = px(CARET_H).min((r.bottom - r.top) / 3).max(2);
    let l = (h * 3 / 5).min(px(CARET_L)).max(1);
    // La LARGEUR totale fait `l` (et non 2`l`) : la pointe d'un côté, les ailes de
    // l'autre, autour du centre. La HAUTEUR fait `h` malgré l'arrondi entier : une aile
    // prend le pixel de reste.
    let (haut, bas) = (cy - h / 2, cy + (h + 1) / 2);
    let (aile, pointe) = if vers_la_gauche { (cx + (l + 1) / 2, cx - l / 2) } else { (cx - (l + 1) / 2, cx + l / 2) };
    [POINT { x: aile, y: haut }, POINT { x: pointe, y: cy }, POINT { x: aile, y: bas }]
}

/// **LE CARET DU CARTEL** : deux traits qui se rejoignent, comme le glyph
/// `ico-caret-left` de la page — PAS le triangle plein du bouton de la barre, qui ferait
/// une marque de ~30 × 25 px là où la page en montre une de 11 × 7.
/// ⚠️ `PS_GEOMETRIC` : un stylo cosmétique impose une largeur de 1 et IGNORE les
/// extrémités et les jonctions, donc à 144 ppp le trait resterait fin pendant que le
/// cartel grandit. `PS_JOIN_ROUND` évite la pointe cassée à la jonction.
unsafe fn caret(dc: HDC, r: RECT, dpi: i32, vers_la_gauche: bool, couleur: u32) {
    let points = caret_dans(r, dpi, vers_la_gauche);
    // Le trait ne doit pas déborder du cartel : rogné à la moitié de la hauteur du caret.
    let largeur = (CARET_TRAIT * dpi / 96).max(1).min(((points[2].y - points[0].y) / 2).max(1));
    // SAFETY : `stylo` est un objet GDI rendu ici, restauré avant d'être détruit ;
    // `points` est un tableau local, valide pendant l'appel ; `dc` est le tampon du
    // peignant, vivant pendant l'appel.
    unsafe {
        let stylo = CreatePen(PS_GEOMETRIC | PS_SOLID | PS_ENDCAP_FLAT | PS_JOIN_ROUND, largeur, COLORREF(couleur));
        let ancien_stylo = SelectObject(dc, stylo.into());
        let _ = Polyline(dc, &points);
        SelectObject(dc, ancien_stylo);
        let _ = DeleteObject(stylo.into());
    }
}

unsafe fn polygone(dc: HDC, points: &[POINT], couleur: u32) {
    unsafe {
        let pinceau = CreateSolidBrush(COLORREF(couleur));
        let ancien_pinceau = SelectObject(dc, pinceau.into());
        let ancien_stylo = SelectObject(dc, GetStockObject(NULL_PEN));
        let _ = Polygon(dc, points);
        SelectObject(dc, ancien_stylo);
        SelectObject(dc, ancien_pinceau);
        let _ = DeleteObject(pinceau.into());
    }
}

/// Une piste (progression ou volume) : le fond, la part remplie, le bouton.
unsafe fn piste(dc: HDC, r: RECT, fraction: f32, px: impl Fn(i32) -> i32) {
    let milieu = (r.top + r.bottom) / 2;
    let fond = RECT { left: r.left, top: milieu - px(2), right: r.right, bottom: milieu + px(2) };
    let x = r.left + ((r.right - r.left) as f32 * fraction.clamp(0.0, 1.0)) as i32;
    unsafe {
        remplir(dc, fond, PISTE);
        remplir(dc, RECT { right: x, ..fond }, ACCENT);
        remplir(dc, RECT { left: x - px(5), top: milieu - px(5), right: x + px(5), bottom: milieu + px(5) }, TEXTE);
    }
}

/// Peint la barre `barre` du lecteur `hwnd`.
fn peindre(hwnd: HWND, barre: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let dc = unsafe { BeginPaint(barre, &mut ps) };
    let z = zones(barre);
    let dpi = echelle(barre);
    let px = |v: i32| v * dpi / 96;
    let (l, h) = (z.barre.right, z.barre.bottom);
    let etat = joueur(hwnd).map(|(j, api)| unsafe {
        ((api.is_playing)(j) != 0, (api.get_time)(j), (api.get_length)(j), (api.get_state)(j))
    });
    let (lit, temps, longueur, code) = etat.unwrap_or((false, 0, 0, 0));
    let (volume, police, visee, avant, apres) = avec(hwnd, |l| {
        let (avant, apres) = cotes_de_la_fleche(l);
        (l.volume, l.police, l.visee, avant, apres)
    })
    .unwrap_or((0, HFONT::default(), None, false, false));
    if l > 0 && h > 0 {
        unsafe {
            // Double tampon : la barre est repeinte quatre fois par seconde.
            let tampon = CreateCompatibleDC(Some(dc));
            let image = CreateCompatibleBitmap(dc, l, h);
            let ancienne_image = SelectObject(tampon, image.into());
            remplir(tampon, RECT { left: 0, top: 0, right: l, bottom: h }, FOND_BARRE);

            // Lecture / pause.
            let b = z.lecture;
            let (cx, cy, r) = ((b.left + b.right) / 2, (b.top + b.bottom) / 2, (b.bottom - b.top) / 3);
            if lit {
                remplir(tampon, RECT { left: cx - r + px(1), top: cy - r, right: cx - px(2), bottom: cy + r }, TEXTE);
                remplir(tampon, RECT { left: cx + px(2), top: cy - r, right: cx + r - px(1), bottom: cy + r }, TEXTE);
            } else {
                polygone(tampon, &[POINT { x: cx - r + px(2), y: cy - r }, POINT { x: cx + r, y: cy }, POINT { x: cx - r + px(2), y: cy + r }], TEXTE);
            }

            // ⭐ **LES FLÈCHES DE VOISINAGE** — DEMANDE EXPLICITE (2026-11-03) :
            // « des flèches en surbrillance pour passer à la vidéo suivante ». Elles
            // sont aux deux bouts de la barre, et elles se peignent même éteintes
            // (voir `ETEINTE`) : la barre suit la souris en plein écran, donc ce
            // qu'on voit quand elle reparaît doit dire où l'on peut aller.
            fleche(tampon, z.precedent, true, if avant { TEXTE } else { ETEINTE });
            fleche(tampon, z.suivant, false, if apres { TEXTE } else { ETEINTE });

            // Le temps, ou ce qui ne va pas.
            let ancienne_police = SelectObject(tampon, police.into());
            SetBkMode(tampon, TRANSPARENT);
            SetTextColor(tampon, COLORREF(TEXTE));
            let affiche = match visee {
                Some(f) => (longueur as f32 * f) as i64,
                None => temps,
            };
            let texte = if code == ETAT_ERREUR {
                "Lecture impossible".to_string()
            } else {
                format!("{} / {}", format_temps(affiche), format_temps(longueur))
            };
            let mut texte: Vec<u16> = texte.encode_utf16().collect();
            let mut boite = z.temps;
            DrawTextW(tampon, &mut texte, &mut boite, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

            // Progression.
            let fraction = visee.unwrap_or(if longueur > 0 { temps as f32 / longueur as f32 } else { 0.0 });
            if z.progression.right > z.progression.left {
                piste(tampon, z.progression, fraction, px);
            }

            // Volume : un haut-parleur, puis sa piste.
            let v = z.volume;
            let (hx, hy) = (v.left - px(22), (v.top + v.bottom) / 2);
            remplir(tampon, RECT { left: hx, top: hy - px(4), right: hx + px(5), bottom: hy + px(4) }, TEXTE);
            polygone(tampon, &[POINT { x: hx + px(4), y: hy - px(4) }, POINT { x: hx + px(11), y: hy - px(9) }, POINT { x: hx + px(11), y: hy + px(9) }, POINT { x: hx + px(4), y: hy + px(4) }], TEXTE);
            piste(tampon, v, volume as f32 / 100.0, px);

            // Plein écran : quatre coins.
            let p = z.plein_ecran;
            let (m, e, c) = (px(7), px(2), px(6));
            let (g, d, hh, bb) = (p.left + m, p.right - m, p.top + m, p.bottom - m);
            for (x, y, sx, sy) in [(g, hh, 1, 1), (d, hh, -1, 1), (g, bb, 1, -1), (d, bb, -1, -1)] {
                let horiz = RECT { left: x.min(x + sx * c), top: y.min(y + sy * e), right: x.max(x + sx * c), bottom: y.max(y + sy * e) };
                let vert = RECT { left: x.min(x + sx * e), top: y.min(y + sy * c), right: x.max(x + sx * e), bottom: y.max(y + sy * c) };
                remplir(tampon, horiz, TEXTE);
                remplir(tampon, vert, TEXTE);
            }

            SelectObject(tampon, ancienne_police);
            let _ = BitBlt(dc, 0, 0, l, h, Some(tampon), 0, 0, SRCCOPY);
            SelectObject(tampon, ancienne_image);
            let _ = DeleteObject(image.into());
            let _ = DeleteDC(tampon);
        }
    }
    let _ = unsafe { EndPaint(barre, &ps) };
}

/// **PEINT UN CARTEL DE SURIMPRESSION** : un fond et le caret tracé fin de la page, le
/// côté visé par la souris en accent avec l'encre sombre (les deux rôles de
/// `--popup-accent-solid` + `--popup-on-accent` de la page). Double tampon comme la
/// barre : le cartel se repeint au survol, posé sur une vidéo qui bouge, et un cartel
/// peint à moitié clignote.
fn peindre_cartel(cartel: HWND, hwnd: HWND, vers_la_gauche: bool) {
    let mut ps = PAINTSTRUCT::default();
    let dc = unsafe { BeginPaint(cartel, &mut ps) };
    let mut client = RECT::default();
    let _ = unsafe { GetClientRect(cartel, &mut client) };
    // ⚠️ L'échelle est relue sur L'IMAGE, la même que celle qui a POSÉ le cartel
    // (`poser_les_cartels`) : sinon le glyph serait converti dans une autre unité que la
    // boîte qui le contient ([[plancher-dans-les-mauvaises-unites]]).
    let (vise, dpi) = avec(hwnd, |l| (if vers_la_gauche { l.vise.0 } else { l.vise.1 }, echelle(l.image)))
        .unwrap_or((false, 96));
    if client.right > 0 && client.bottom > 0 {
        unsafe {
            let tampon = CreateCompatibleDC(Some(dc));
            let image = CreateCompatibleBitmap(dc, client.right, client.bottom);
            let ancienne_image = SelectObject(tampon, image.into());
            remplir(tampon, client, if vise { FOND_CARTEL_VISE } else { FOND_CARTEL });
            caret(tampon, client, dpi, vers_la_gauche, if vise { ENCRE_CARTEL_VISE } else { ENCRE_CARTEL });
            let _ = BitBlt(dc, 0, 0, client.right, client.bottom, Some(tampon), 0, 0, SRCCOPY);
            SelectObject(tampon, ancienne_image);
            let _ = DeleteObject(image.into());
            let _ = DeleteDC(tampon);
        }
    }
    let _ = unsafe { EndPaint(cartel, &ps) };
}

fn fermer(hwnd: HWND) {
    noter_plein_ecran(hwnd, false);
    let lecteur = LECTEURS.with(|l| l.borrow_mut().remove(&(hwnd.0 as isize)));
    if let Some(l) = lecteur {
        unsafe {
            let _ = KillTimer(Some(hwnd), MINUTEUR);
            if let Ok(api) = api() {
                let j = l.joueur as *mut c_void;
                (api.stop)(j);
                (api.lecteur_release)(j);
            }
            let _ = DeleteObject(l.police.into());
        }
    }
}

fn xy(lparam: LPARAM) -> (i32, i32) {
    ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32)
}

/// **Ctrl est-elle enfoncée ?** Lue à l'état, pas à l'événement : `WM_KEYDOWN` ne
/// dit rien du modificateur et la fenêtre ne reçoit qu'une touche. Le bit
/// `0x8000` est « enfoncée maintenant » (le bit de poids faible, lui, dit « venait
/// d'être enfoncée » — il ne nous sert pas).
fn controle_appuye() -> bool {
    unsafe { (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16) & 0x8000 != 0 }
}

/// La position de la souris, en coordonnées d'écran.
fn souris_ecran() -> POINT {
    let mut ou = POINT::default();
    let _ = unsafe { GetCursorPos(&mut ou) };
    ou
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_GETMINMAXINFO => {
            // ⛔ **LE PLANCHER DE LARGEUR DE LA FENÊTRE** — la barre a un budget
            // (voir `LARGEUR_MIN_CSS`) : sous 440 px CSS, `temps` et `volume` se
            // recouvrent et la piste de progression tombe à `left == right`. Sans ce
            // message, l'utilisateur comme `ajuster_a_la_video` rognent la fenêtre
            // sous ce budget — et le chevauchement ne se voit pas, il se clique.
            //
            // ⚠️ `ptMinTrackSize` porte la FENÊTRE, bordures comprises, en pixels
            // PHYSIQUES : la conversion est ici, pas dans la constante CSS.
            let infos = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            infos.ptMinTrackSize.x = largeur_min(echelle(hwnd)) + bordure_horizontale(hwnd);
            LRESULT(0)
        }
        WM_SIZE => {
            placer(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            // Tout est couvert par l'image et la barre : rien à peindre ici.
            let mut ps = PAINTSTRUCT::default();
            unsafe {
                BeginPaint(hwnd, &mut ps);
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            ajuster_a_la_video(hwnd);
            surveiller_la_souris(hwnd);
            // ⚠️ **La surimpression est SONDÉE, pas seulement relayée.** La souris
            // peut quitter l'image sans qu'aucun `WM_MOUSEMOVE` n'arrive jusqu'à nos
            // fenêtres : elle a glissé sur le dock, sur une autre application, ou est
            // sortie de l'écran. Sans ce sondage, les cartels resteraient allumés sur
            // une image que plus personne ne survole.
            eclaire_les_cartels(hwnd, souris_ecran());
            redessiner(hwnd);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            la_souris_bouge(hwnd, souris_ecran());
            let cran = ((wparam.0 >> 16) & 0xFFFF) as i16 as i32 / 120;
            let volume = avec(hwnd, |l| l.volume).unwrap_or(0);
            regler_volume(hwnd, volume + cran * PAS_VOLUME);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let touche = wparam.0 as u16;
            let volume = avec(hwnd, |l| l.volume).unwrap_or(0);
            match touche {
                t if t == VK_SPACE.0 => basculer_la_lecture(hwnd),
                // ⭐ **Ctrl + `←` / `→` change de vidéo** — DEMANDE EXPLICITE
                // (2026-11-03). `←`/`→` seuls sont déjà pris par le saut de ±5 s,
                // et les deux gestes se ressemblent trop pour être confondus :
                // la flèche de la barre et Ctrl font la vidéo d'à côté.
                t if t == VK_LEFT.0 && controle_appuye() => aller_au_voisin(hwnd, false),
                t if t == VK_RIGHT.0 && controle_appuye() => aller_au_voisin(hwnd, true),
                t if t == VK_LEFT.0 => sauter(hwnd, -PAS_MS),
                t if t == VK_RIGHT.0 => sauter(hwnd, PAS_MS),
                t if t == VK_UP.0 => regler_volume(hwnd, volume + PAS_VOLUME),
                t if t == VK_DOWN.0 => regler_volume(hwnd, volume - PAS_VOLUME),
                t if t == b'F' as u16 => basculer_le_plein_ecran(hwnd),
                t if t == VK_ESCAPE.0 => {
                    if avec(hwnd, |l| l.plein_ecran.is_some()).unwrap_or(false) {
                        basculer_le_plein_ecran(hwnd);
                    }
                }
                _ => return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            fermer(hwnd);
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            // Fermée autrement que par `WM_CLOSE` (fin du process) : rien ne doit
            // rester accroché à une fenêtre morte.
            fermer(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// L'image : un clic met en pause ou relance, un double-clic bascule le plein
/// écran (et annule la pause de son premier clic). Barre cachée, le curseur
/// l'est aussi.
unsafe extern "system" fn wndproc_image(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let parent = unsafe { GetParent(hwnd) }.unwrap_or_default();
    match msg {
        WM_LBUTTONDOWN => {
            let _ = unsafe { SetFocus(Some(parent)) };
            la_souris_bouge(parent, souris_ecran());
            basculer_la_lecture(parent);
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            basculer_la_lecture(parent);
            basculer_le_plein_ecran(parent);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let ou = souris_ecran();
            la_souris_bouge(parent, ou);
            eclaire_les_cartels(parent, ou);
            LRESULT(0)
        }
        WM_SETCURSOR if avec(parent, |l| l.cachee).unwrap_or(false) => {
            unsafe { SetCursor(None) };
            LRESULT(1)
        }
        WM_MOUSEWHEEL | WM_KEYDOWN => unsafe { wndproc(parent, msg, wparam, lparam) },
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// **UN CARTEL DE SURIMPRESSION** : le clic mène au voisin de son côté. Le corps est
/// partagé, les deux classes (`NdLecteurSurAvant`, `NdLecteurSurApres`) ne diffèrent
/// que par `vers_la_gauche` — c'est le seul moyen pour le cartel de savoir de quel côté
/// il mène, il ne porte rien d'autre.
///
/// ⛔ **UN SEUL DES TROIS MESSAGES DU CLIC EST TRAITÉ.** La classe a `CS_DBLCLKS`, donc
/// un double clic envoie `WM_LBUTTONDOWN`, `WM_LBUTTONDBLCLK` et `WM_LBUTTONUP` :
/// traiter deux de ces trois ferait sauter DEUX vidéos d'un seul geste, et le voisin
/// d'après serait demandé deux fois. `WM_LBUTTONDOWN` seul, comme les boutons de la
/// barre.
///
/// ⚠️ Il ne fait PAS `basculer_la_lecture` : la fenêtre mange le clic, donc l'image ne
/// le voit pas, et un cartel qui mettrait en pause en même temps qu'il changerait de
/// vidéo arrêterait la nouvelle à la première image.
unsafe extern "system" fn wndproc_surimpression(cartel: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, vers_la_gauche: bool) -> LRESULT {
    let parent = unsafe { GetParent(cartel) }.unwrap_or_default();
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            peindre_cartel(cartel, parent, vers_la_gauche);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            // Le focus à la fenêtre parente : `Ctrl`+`←`/`→` et `Espace` sont reçus par
            // elle, pas par le cartel.
            let _ = unsafe { SetFocus(Some(parent)) };
            la_souris_bouge(parent, souris_ecran());
            aller_au_voisin(parent, !vers_la_gauche);
            LRESULT(0)
        }
        WM_LBUTTONUP | WM_LBUTTONDBLCLK => LRESULT(0),
        WM_MOUSEMOVE => {
            // Le survol passe par ICI quand la souris est sur le cartel, et non plus sur
            // l'image : sans ces deux lignes, la barre ne reviendrait pas en plein écran
            // et le cartel s'éteindrait sous la souris qui l'a allumé.
            let ou = souris_ecran();
            la_souris_bouge(parent, ou);
            eclaire_les_cartels(parent, ou);
            LRESULT(0)
        }
        // Pas de `WM_SETCURSOR` ici : un cartel n'est jamais montré pendant `cachee`
        // (`poser_les_cartels` les éteint avec la barre), et une fenêtre cachée ne reçoit
        // pas le curseur. Le curseur coupé en plein écran est tenu par `cacher` et par
        // `wndproc_image`.
        WM_MOUSEWHEEL | WM_KEYDOWN => unsafe { wndproc(parent, msg, wparam, lparam) },
        _ => unsafe { DefWindowProcW(cartel, msg, wparam, lparam) },
    }
}

unsafe extern "system" fn wndproc_surimpression_gauche(cartel: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    wndproc_surimpression(cartel, msg, wparam, lparam, true)
}

unsafe extern "system" fn wndproc_surimpression_droite(cartel: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    wndproc_surimpression(cartel, msg, wparam, lparam, false)
}

/// La barre de commandes : boutons, pistes, et ses glissés.
unsafe extern "system" fn wndproc_barre(barre: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let hwnd = unsafe { GetParent(barre) }.unwrap_or_default();
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            peindre(hwnd, barre);
            LRESULT(0)
        }
        // ⛔ **UN DOUBLE CLIC SUR UNE FLÈCHE NE SAUTE PAS DEUX CLIPS.** La classe a
        // `CS_DBLCLKS`, donc le geste envoie `WM_LBUTTONDOWN` PUIS `WM_LBUTTONDBLCLK` :
        // traiter les deux sur `‹`/`›` demanderait deux voisins et le deuxième fichier
        // ne serait jamais vu — le cartel de surimpression ne traite déjà que DOWN (voir
        // `wndproc_surimpression`), et la règle doit être la même des deux côtés de la
        // fenêtre. Ailleurs le double clic garde son sens : rejouer (`lecture`), chercher
        // sur la piste, viser le volume — d'où le renvoi vers `WM_LBUTTONDOWN`.
        WM_LBUTTONDBLCLK => {
            let (x, y) = xy(lparam);
            let z = zones(barre);
            if dedans(&z.precedent, x, y) || dedans(&z.suivant, x, y) {
                LRESULT(0)
            } else {
                unsafe { wndproc_barre(barre, WM_LBUTTONDOWN, wparam, lparam) }
            }
        }
        WM_LBUTTONDOWN => {
            let _ = unsafe { SetFocus(Some(hwnd)) };
            la_souris_bouge(hwnd, souris_ecran());
            let (x, y) = xy(lparam);
            let z = zones(barre);
            if dedans(&z.lecture, x, y) {
                basculer_la_lecture(hwnd);
            } else if dedans(&z.plein_ecran, x, y) {
                basculer_le_plein_ecran(hwnd);
            } else if dedans(&z.precedent, x, y) {
                aller_au_voisin(hwnd, false);
            } else if dedans(&z.suivant, x, y) {
                aller_au_voisin(hwnd, true);
            } else if dedans(&z.progression, x, y) {
                let f = fraction_dans(&z.progression, x);
                avec(hwnd, |l| {
                    l.glisse = Some(Glisse::Progression);
                    l.visee = Some(f);
                });
                chercher_a(hwnd, f);
                unsafe { SetCapture(barre) };
            } else if dedans(&RECT { left: z.volume.left - 24 * echelle(barre) / 96, ..z.volume }, x, y) {
                avec(hwnd, |l| l.glisse = Some(Glisse::Volume));
                regler_volume(hwnd, (fraction_dans(&z.volume, x) * 100.0).round() as i32);
                unsafe { SetCapture(barre) };
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            la_souris_bouge(hwnd, souris_ecran());
            let (x, _) = xy(lparam);
            let z = zones(barre);
            match avec(hwnd, |l| l.glisse).flatten() {
                Some(Glisse::Progression) => {
                    let f = fraction_dans(&z.progression, x);
                    avec(hwnd, |l| l.visee = Some(f));
                    chercher_a(hwnd, f);
                    redessiner(hwnd);
                }
                Some(Glisse::Volume) => regler_volume(hwnd, (fraction_dans(&z.volume, x) * 100.0).round() as i32),
                None => {}
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if avec(hwnd, |l| l.glisse.take().is_some()).unwrap_or(false) {
                let _ = unsafe { ReleaseCapture() };
                avec(hwnd, |l| l.visee = None);
                redessiner(hwnd);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL | WM_KEYDOWN => unsafe { wndproc(hwnd, msg, wparam, lparam) },
        _ => unsafe { DefWindowProcW(barre, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_temps_s_ecrit_en_minutes_ou_en_heures() {
        assert_eq!(format_temps(0), "00:00");
        assert_eq!(format_temps(65_400), "01:05");
        assert_eq!(format_temps(3_725_000), "1:02:05");
        assert_eq!(format_temps(-5), "00:00");
    }

    /// Sonde à la main, VLC installé : `cargo test --release -- --ignored libvlc_se_charge`.
    #[test]
    #[ignore]
    fn libvlc_se_charge() {
        let api = api().expect("libVLC");
        assert_ne!(api.instance, 0);
    }

    /// La liste des lecteurs en plein écran suit l'entrée, la sortie et la
    /// fermeture.
    #[test]
    fn le_plein_ecran_d_un_lecteur_se_note() {
        let h = HWND(0x5eed as *mut c_void);
        assert!(!est_en_plein_ecran(h));
        noter_plein_ecran(h, true);
        noter_plein_ecran(h, true);
        assert!(est_en_plein_ecran(h));
        noter_plein_ecran(h, false);
        assert!(!est_en_plein_ecran(h), "une seule entrée, même notée deux fois");
        noter_plein_ecran(h, true);
        fermer(h);
        assert!(!est_en_plein_ecran(h), "fermé : plus en plein écran");
    }

    #[test]
    fn la_fraction_d_une_piste_est_bornee() {
        let r = RECT { left: 100, top: 0, right: 300, bottom: 10 };
        assert_eq!(fraction_dans(&r, 200), 0.5);
        assert_eq!(fraction_dans(&r, 50), 0.0);
        assert_eq!(fraction_dans(&r, 400), 1.0);
    }

    // ---- LES ZONES DE LA BARRE, ET LES FLÈCHES DE VOISINAGE ------------------
    // DEMANDE EXPLICITE (2026-11-03) : « des flèches en surbrillance pour passer à
    // la vidéo suivante ». Deux flèches de plus dans une barre qui en avait déjà
    // cinq, et le clic est ROUTÉ par une chaîne de `else if` : la zone qui perd le
    // premier `dedans` ne répond JAMAIS, et rien ne se voit à l'œil — les deux zones
    // se peignent l'une sur l'autre. C'est le seul défaut de la barre qu'un test
    // peut attraper sans VLC ni fenêtre.

    /// Une zone vide (`left == right`) ne chevauche rien : `dedans` exige
    /// `x >= left && x < right`, et une piste réduite à rien ne se clique pas.
    fn pleine(r: &RECT) -> bool {
        r.left < r.right && r.top < r.bottom
    }

    fn se_chevauchent(a: &RECT, b: &RECT) -> bool {
        pleine(a) && pleine(b) && a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
    }

    fn barre(largeur: i32, dpi: i32) -> Zones {
        // La hauteur réelle de la barre : `placer` la pose à `HAUTEUR_BARRE` scaled.
        let hauteur = HAUTEUR_BARRE * dpi / 96;
        zones_dans(RECT { left: 0, top: 0, right: largeur, bottom: hauteur }, dpi)
    }

    /// **DEPUIS LE PLANCHER, RIEN NE SE RECOUVRE ET TOUT SE CLIQUE.** La table est
    /// bâtie SUR `largeur_min(dpi)` et non sur des nombres au choix : une barre de
    /// 400 px à 96 dpi ne se recouvrait pas, mais sa piste était déjà réduite à
    /// rien — un garde qui laisse passer ça ne garde rien (revu 2026-11-03).
    /// Le DPI est éprouvé à 96, 120, 144 et 168 : les largeurs de la barre sont en
    /// CSS et se convertissent, un plancher qui ne suit pas la conversion est une
    /// faute déjà commise deux fois ici.
    #[test]
    fn les_zones_de_la_barre_ne_se_chevauchent_pas_depuis_le_plancher() {
        for dpi in [96, 120, 144, 168] {
            for extra_css in [0, 60, 400, 2000] {
                let largeur = largeur_min(dpi) + extra_css * dpi / 96;
                let z = barre(largeur, dpi);
                let toutes = [
                    ("lecture", &z.lecture),
                    ("precedent", &z.precedent),
                    ("suivant", &z.suivant),
                    ("temps", &z.temps),
                    ("progression", &z.progression),
                    ("volume", &z.volume),
                    ("plein_ecran", &z.plein_ecran),
                ];
                for i in 0..toutes.len() {
                    for j in (i + 1)..toutes.len() {
                        assert!(
                            !se_chevauchent(toutes[i].1, toutes[j].1),
                            "barre de {} px a {} dpi : {} et {} se chevauchent",
                            largeur,
                            dpi,
                            toutes[i].0,
                            toutes[j].0
                        );
                    }
                }
                // Une zone vide n'est pas « ça ne se recouvre pas » : elle ne se clique pas.
                for (nom, r) in &toutes {
                    assert!(pleine(r), "barre de {} px a {} dpi : {} est reduite a rien", largeur, dpi, nom);
                }
                assert!(
                    z.progression.right - z.progression.left >= PISTE_MIN * dpi / 96,
                    "barre de {} px a {} dpi : la piste de progression fait moins de {} px, on ne peut plus y chercher une position",
                    largeur,
                    dpi,
                    PISTE_MIN
                );
            }
        }
    }

    /// ⛔ **UN CRAN SOUS LE PLANCHER, LE DÉFAUT REVIENT.** Ce test ne vérifie pas la
    /// barre : il vérifie le PLANCHER. Descendre `LARGEUR_MIN_CSS` sous le budget
    /// réel de `zones_dans` rougit ici, donc le plancher ne peut pas être un nombre
    /// choisi au feeling ni une constante qui dérive silencieusement.
    #[test]
    fn le_plancher_est_le_budget_reel_de_la_barre() {
        let sous = largeur_min(96) - PISTE_MIN - 1;
        let z = barre(sous, 96);
        assert!(!pleine(&z.progression), "sous le plancher, la piste devrait être réduite à rien : le plancher est plus large que le besoin");
        let z2 = barre(largeur_min(96) - 75, 96);
        assert!(se_chevauchent(&z2.temps, &z2.volume), "a 75 px sous le plancher, l'heure et le volume devraient se recouvrir");
    }

    #[test]
    fn les_fleches_sont_aux_deux_bouts_et_le_volume_ne_tombe_dessus() {
        let z = barre(1200, 96);
        assert!(z.precedent.left >= z.lecture.right, "la fleche de gauche empiete sur lecture");
        assert!(z.suivant.right <= z.plein_ecran.left, "la fleche de droite empiete sur plein ecran");
        // Le garde qui a fait reculer le volume d'une fleche : sinon un clic sur `›`
        // reglerait le son au lieu de changer de vidéo.
        assert!(z.volume.right <= z.suivant.left, "le volume tombe sur la fleche de droite");
        assert!(z.precedent.left >= 0 && z.suivant.right <= 1200, "une fleche sort de la barre");
    }

    /// **AU PLANCHER, LE MILIEU DE `›` CLIQUE `›`** — pas le volume, pas plein
    /// écran, pas la piste. La version d'avant éprouvait une barre de 200 px, une
    /// largeur que la fenêtre ne peut plus atteindre depuis que `WM_GETMINMAXINFO`
    /// pose le plancher : y vérifier le clic laissait passer le vrai cas, celui où
    /// la fenêtre se rogne sur un clip 9:16.
    #[test]
    fn les_fleches_restaient_cliquables_au_plancher() {
        for dpi in [96, 144] {
            let z = barre(largeur_min(dpi), dpi);
            assert!(pleine(&z.precedent) && pleine(&z.suivant), "une fleche s'est reduite a rien a {} dpi", dpi);
            let (x, y) = ((z.suivant.left + z.suivant.right) / 2, (z.suivant.top + z.suivant.bottom) / 2);
            assert!(dedans(&z.suivant, x, y), "le milieu de la fleche de droite ne la clique pas");
            for (nom, autre) in [("volume", &z.volume), ("plein ecran", &z.plein_ecran), ("piste", &z.progression), ("heure", &z.temps)] {
                assert!(!dedans(autre, x, y), "le milieu de `›` clique {} a {} dpi", nom, dpi);
            }
            let (x, y) = ((z.precedent.left + z.precedent.right) / 2, (z.precedent.top + z.precedent.bottom) / 2);
            assert!(dedans(&z.precedent, x, y), "le milieu de la fleche de gauche ne la clique pas");
            for (nom, autre) in [("lecture", &z.lecture), ("heure", &z.temps)] {
                assert!(!dedans(autre, x, y), "le milieu de `‹` clique {} a {} dpi", nom, dpi);
            }
        }
    }

    /// Le client de l'image du lecteur, en haut à gauche : une fenêtre enfant sans
    /// bordure a son client pour cadre, et `cartel_dans` travaille dans ce repère.
    fn image(largeur: i32, hauteur: i32) -> RECT {
        RECT { left: 0, top: 0, right: largeur, bottom: hauteur }
    }

    /// **LE CARTEL DE SURIMPRESSION, aux mesures de `.bord`** (34 × 46 px CSS, à 10 px
    /// du bord, centré) — les deux surfaces ont le même cartel, pas deux formes pour le
    /// même geste. Et le DPI est CONVERTI, pas recopié.
    #[test]
    fn les_cartels_de_surimpression_sont_aux_deux_bords_de_l_image() {
        let c = image(800, 450);
        let gauche = cartel_dans(c, 96, true);
        let droite = cartel_dans(c, 96, false);
        assert_eq!((gauche.left, gauche.right - gauche.left), (10, 34), "34 px CSS à 10 px du bord");
        assert_eq!((droite.right, droite.right - droite.left), (790, 34), "miroir exact du côté gauche");
        assert_eq!((gauche.top, gauche.bottom - gauche.top), (202, 46), "centré, 46 px de haut");
        assert_eq!(droite.top, gauche.top, "les deux cartels à la même hauteur");
        let a144 = cartel_dans(c, 144, true);
        assert_eq!((a144.left, a144.right - a144.left, a144.bottom - a144.top), (15, 51, 69), "à 144 ppp, tout est converti");
    }

    /// ⛔ **RIEN NE RESTE PAS SUR LE MILIEU DE L'IMAGE** : cliquer la vidéo met en
    /// pause, et un cartel posé au milieu le lui vole sans le lui rendre. Le rognage au
    /// QUART de la largeur est le garde — un clip 9:16 rogné par `ajuster_a_la_video`
    /// est exactement le cas où l'image est la plus étroite.
    #[test]
    fn les_cartels_ne_recouvrent_pas_le_milieu_de_l_image() {
        for largeur in [88, 120, 300] {
            let c = image(largeur, 300);
            let gauche = cartel_dans(c, 96, true);
            let droite = cartel_dans(c, 96, false);
            assert!(gauche.right < droite.left, "les deux cartels se touchent à {largeur} px de large");
            assert!(droite.left - gauche.right >= 1, "rien de cliquable ne reste entre eux à {largeur} px");
            assert!(!dedans(&gauche, largeur / 2, 150) && !dedans(&droite, largeur / 2, 150), "le milieu de l'image est sous un cartel à {largeur} px");
            assert!(gauche.left >= 0 && droite.right <= largeur, "un cartel sort de l'image à {largeur} px");
        }
        // Le rognage, mesuré : à 88 px, le cartel fait 22 px et non 34.
        assert_eq!(cartel_dans(image(88, 300), 96, true).right - 10, 88 / 4, "le cartel n'est pas rogné au quart de l'image");
    }

    /// Une image trop étroite pour les deux cartels PLUS un milieu, ou trop courte pour
    /// un cartel de 46 px, rend un rectangle VIDÉ : `poser_les_cartels` cache alors la
    /// fenêtre au lieu d'en montrer une de quelques pixels.
    #[test]
    fn une_image_trop_petite_n_a_pas_de_cartel() {
        for c in [image(40, 300), image(800, 20), image(0, 0)] {
            for a_gauche in [true, false] {
                let carte = cartel_dans(c, 96, a_gauche);
                assert!(carte.right <= carte.left, "un cartel quand même sur une image de {}x{}", c.right, c.bottom);
            }
        }
    }

    /// **LE SURVOL QUI RETIENT L'INTERFACE** — en plein écran, la barre et les cartels se
    /// cachent après `DELAI_CACHE`. Un cartel montré doit retenir l'interface (sinon elle
    /// s'éteint sous la souris qui la tient, et la moindre secousse la rallume : un
    /// clignotement) ; un côté ÉTEINT ne doit rien retenir, puisqu'il n'y a rien sous le
    /// doigt, et le milieu de l'image non plus.
    #[test]
    fn un_cartel_montre_retient_le_survol_pas_un_cartel_eteint() {
        let c = image(800, 450);
        let (gauche, droite) = (cartel_dans(c, 96, true), cartel_dans(c, 96, false));
        let (gx, gy) = ((gauche.left + gauche.right) / 2, (gauche.top + gauche.bottom) / 2);
        let (dx, dy) = ((droite.left + droite.right) / 2, (droite.top + droite.bottom) / 2);
        assert!(sous_le_cartel_dans(c, 96, (true, true), gx, gy), "les deux côtés allumés, la gauche retient");
        assert!(sous_le_cartel_dans(c, 96, (false, true), dx, dy), "la droite allumée seule retient à droite");
        assert!(!sous_le_cartel_dans(c, 96, (false, true), gx, gy), "le côté éteint ne retient rien : il n'y a rien sous la souris");
        assert!(!sous_le_cartel_dans(c, 96, (true, true), 400, 225), "le milieu de l'image n'est sur aucun cartel");
        assert!(!sous_le_cartel_dans(c, 96, (false, false), gx, gy), "rien d'allumé : rien à retenir");
    }

    /// ⛔ **LE CARET DU CARTEL AUX MESURES DU GLYPH DE LA PAGE** (≈ 11 × 7 px CSS, tracé
    /// fin), PAS au tiers de la boîte : dans un cartel de 46 px, `hauteur / 3` donnait une
    /// marque de ~30 × 25 px là où `.bord` en montre une de 11 × 7 — deux formes pour le
    /// même geste, ce que la surimpression refuse. Le caret tient dans son cartel, avec son
    /// trait de 2 px, et se MIROIR de l'autre côté.
    #[test]
    fn le_caret_du_cartel_aux_mesures_du_glyph_de_la_page() {
        let boite = RECT { left: 10, top: 202, right: 44, bottom: 248 };
        let gauche = caret_dans(boite, 96, true);
        let (largeur, hauteur) = (gauche[0].x - gauche[1].x, gauche[2].y - gauche[0].y);
        assert_eq!((largeur, hauteur), (6, 11), "11 px de haut, ~7 de large : le glyph de la page");
        assert!(largeur < 12 && hauteur < 24, "ce n'est pas le triangle plein du bouton de la barre");
        for p in gauche {
            assert!(dedans(&boite, p.x - 1, p.y - 1) && dedans(&boite, p.x + 1, p.y + 1), "le trait de 2 px sort du cartel à ({},{})", p.x, p.y);
        }
        let droite = caret_dans(boite, 96, false);
        assert_eq!(droite[1].x, gauche[1].x + largeur, "la pointe de l'autre côté, à la même distance du centre");
        assert_eq!((droite[0].y, droite[2].y), (gauche[0].y, gauche[2].y), "miroir exact, même hauteur");
        // Un cartel raccourci à la moitié de sa hauteur CSS rogne le caret avec lui.
        let courte = RECT { left: 0, top: 0, right: 34, bottom: 23 };
        assert_eq!(caret_dans(courte, 96, true)[2].y - caret_dans(courte, 96, true)[0].y, 23 / 3, "le caret ne déborde pas d'un cartel raccourci");
        // À 144 ppp, la boîte est le cartel CONVERTI (51 × 69), pas la boîte CSS : le
        // caret y est converti lui aussi, et il tient dedans.
        let converti = cartel_dans(image(800, 450), 144, true);
        let a144 = caret_dans(converti, 144, true);
        assert_eq!(a144[2].y - a144[0].y, 11 * 144 / 96, "à 144 ppp le caret est CONVERTI");
        assert!(a144[0].x - a144[1].x > 6, "à 144 ppp sa largeur est convertie aussi");
        for p in a144 {
            assert!(dedans(&converti, p.x - 2, p.y - 2) && dedans(&converti, p.x + 2, p.y + 2), "le trait converti sort du cartel à ({},{})", p.x, p.y);
        }
    }

    /// ⛔ **AU PLUS PRÈS : UN PIXEL ENTRE LES DEUX CARTELS, et rien de plus.** La plus
    /// petite image vivante fait 41 px de large à 96 ppp (le garde tombe à 40), le rognage
    /// au quart les laisse à 1 px l'un de l'autre, et ce pixel reste cliquable — cliquer
    /// la vidéo met en pause. À 40 px, plus de cartel du tout. Faute déjà commise ici : les
    /// bords lus à l'envers.
    #[test]
    fn la_plus_petite_image_vivante_laisse_un_pixel_entre_les_deux_cartels() {
        let c = image(41, 300);
        let gauche = cartel_dans(c, 96, true);
        let droite = cartel_dans(c, 96, false);
        assert_eq!((gauche.left, gauche.right), (10, 20), "rogé au quart : 10 px, pas 34");
        assert_eq!((droite.left, droite.right), (21, 31), "le miroir, à 1 px du cartel gauche");
        assert_eq!(droite.left - gauche.right, 1, "le milieu n'est pas mangé");
        assert!(!dedans(&gauche, 20, 150) && !dedans(&droite, 20, 150), "le pixel entre les deux est CLICABLE (pause)");
        for a_gauche in [true, false] {
            let carte = cartel_dans(image(40, 300), 96, a_gauche);
            assert!(carte.right <= carte.left, "un cartel posé à 40 px de large, où les deux n'ont plus place");
        }
    }

    /// ⚠️ **LE CARTEL QUE LE HARN AIS ÉPROUVE EST DU CÔTÉ DEMANDÉ** : `avant` pointe à
    /// gauche et mène au PRÉCÉDENT, `apres` à droite et au suivant. Inverser les deux rend
    /// un run vert qui a mesuré l'autre côté — faute déjà commise, en gauche/droite, dans
    /// ce dépôt.
    #[test]
    fn le_cartel_eprouve_par_le_harnais_est_du_cote_demande() {
        let avant = HWND(1 as *mut std::ffi::c_void);
        let apres = HWND(2 as *mut std::ffi::c_void);
        assert_eq!(cible_hwnd(true, avant, apres), apres, "le côté « suivant » est le cartel de droite");
        assert_eq!(cible_hwnd(false, avant, apres), avant, "le côté « précédent » est le cartel de gauche");
    }
}
