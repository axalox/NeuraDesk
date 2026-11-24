//! **LES SUPER-HÉROS** — DEMANDE EXPLICITE (2026-11-23) : « des vidéos du héros
//! qui traverse l'écran en volant et tourbillonnant pour chaque super héros, sur
//! un fond qui permettra de l'incruster à l'écran sans fond, pour en faire une
//! animation qui se déclenche quand il ne se passe rien depuis un temps
//! réglable ». Réglée dans la section « Mascotte » des Réglages.
//!
//! - **Les images** : une planche WebP par héros (`mascottes/heros-NN.webp`),
//!   80 images d'un vol en boucle (une pirouette) rangées en 8 colonnes × 10
//!   lignes. Fabriquées hors de l'application (Wan 2.2 sur fond vert, détourées)
//!   par `tools/super_heros_vol/`. Le héros VOLE SUR PLACE dans la vidéo :
//!   c'est cette fenêtre qui le fait traverser l'écran, à la vitesse et dans le
//!   sens voulus (le vol vers la gauche est le miroir du vol vers la droite).
//! - **Le déclenchement** : `GetLastInputInfo`, sondé toutes les secondes. Après
//!   `heros_delai` minutes sans clavier ni souris, un héros au hasard part d'un
//!   bord de l'écran du pointeur ; tant que rien ne bouge, un autre repart à
//!   chaque nouveau délai. **Toute entrée l'arrête net.**
//! - **Jamais devant un film** : pas de départ si une application est en plein
//!   écran sur cet écran (`window::fullscreen_window_on_monitor`).
//! - **La fenêtre** : `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`,
//!   à la taille du héros (pas un calque plein écran, qui coûterait cher à
//!   chaque image), montrée par `SW_SHOWNOACTIVATE` : elle ne prend ni le clic
//!   ni le premier plan.
//! - **La mémoire** : seul le héros en vol est décodé, dans un fil à part (la
//!   planche fait ~4000×3000 px) ; il est libéré à l'atterrissage.
//! - **Deux animations** (`ModeHeros`, au choix dans les Réglages) : la
//!   traversée ci-dessus, et l'atterrissage (`atterrissage.rs`) — il arrive de
//!   loin en haut à droite, se pose en bas au centre, repart à pied.

mod atterrissage;

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::win32::mascotte::{ModeHeros, StyleHeros};
use atterrissage::Atterrisseur;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW, MonitorFromPoint,
    ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    DIB_RGB_COLORS, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, RegisterClassExW, SetTimer, SetWindowPos, ShowWindow,
    UpdateLayeredWindow, HTTRANSPARENT, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE,
    ULW_ALPHA, WM_NCHITTEST, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

/// La planche : 8 colonnes × 10 lignes = 80 images, à 16 images par seconde
/// (la cadence de la vidéo d'origine : une pirouette dure 5 s).
pub const COLONNES: u32 = 8;
pub const LIGNES: u32 = 10;
const IMAGES_PAR_SECONDE: f32 = 16.0;
/// Hauteur du héros à l'écran, en pixels LOGIQUES (× l'échelle de l'écran).
const HAUTEUR_LOGIQUE: u32 = 200;
/// Hauteur du héros DEBOUT une fois posé (atterrissage), en pixels logiques.
const DEBOUT_LOGIQUE: u32 = 180;
/// Agrandi, le héros ne dépasse jamais cette part de la hauteur de l'écran.
const HAUTEUR_MAX_ECRAN: f32 = 0.8;
/// Durée d'une traversée, d'un bord à l'autre.
const TRAVERSEE: Duration = Duration::from_secs(8);
/// Le vol ondule : amplitude (en hauteurs de héros) et nombre d'ondulations.
const ONDULATION: f32 = 0.12;
const ONDULATIONS: f32 = 1.5;

const MINUTERIE: usize = 1;
const SONDE_MS: u32 = 1000;
const ANIME_MS: u32 = 16;

// ---------------------------------------------------------------- calculs purs

/// Faut-il lancer un vol ? `inactif` : temps depuis la dernière entrée ;
/// `dernier_vol` : temps depuis la fin du dernier vol, s'il a eu lieu PENDANT
/// cette même inactivité (sinon `None`).
pub fn doit_partir(inactif: Duration, delai: Duration, dernier_vol: Option<Duration>) -> bool {
    inactif >= delai && dernier_vol.is_none_or(|d| d >= delai)
}

/// Le coin haut-gauche du héros à l'instant `t` (0..=1) de la traversée de
/// l'écran `ecran` : il entre et sort ENTIÈREMENT hors de l'écran, et ondule
/// autour de la hauteur `y`.
pub fn position(t: f32, ecran: RECT, (l, h): (u32, u32), vers_la_droite: bool, y: i32) -> POINT {
    let (depart, arrivee) = (ecran.left - l as i32, ecran.right);
    let avance = depart as f32 + (arrivee - depart) as f32 * t.clamp(0.0, 1.0);
    // Vers la gauche : le même chemin, parcouru à l'envers.
    let x = if vers_la_droite { avance } else { (ecran.left + ecran.right - l as i32) as f32 - avance };
    let vague = (t * ONDULATIONS * std::f32::consts::TAU).sin() * ONDULATION * h as f32;
    POINT { x: x.round() as i32, y: y + vague.round() as i32 }
}

/// Découpe la planche (RGBA droit, `l` pixels de large) en ses `combien`
/// premières cases de `cl`×`ch`, rangées par `COLONNES`, ligne par ligne. La
/// planche doit contenir ces cases.
pub fn decouper(rgba: &[u8], l: u32, (cl, ch): (u32, u32), combien: u32) -> Vec<Vec<u8>> {
    (0..combien)
        .map(|i| {
            let (x0, y0) = ((i % COLONNES) * cl, (i / COLONNES) * ch);
            let mut image = Vec::with_capacity((cl * ch * 4) as usize);
            for y in 0..ch {
                let debut = (((y0 + y) * l + x0) * 4) as usize;
                image.extend_from_slice(&rgba[debut..debut + (cl * 4) as usize]);
            }
            image
        })
        .collect()
}

/// Le miroir gauche-droite d'une image 4 octets par pixel.
pub fn miroir(px: &[u8], l: u32) -> Vec<u8> {
    px.chunks_exact((l * 4) as usize)
        .flat_map(|ligne| ligne.chunks_exact(4).rev().flatten().copied().collect::<Vec<_>>())
        .collect()
}

/// Les numéros des héros présents dans `noms` (`heros-NN.webp`), triés.
pub fn heros_presents<'a>(noms: impl Iterator<Item = &'a str>) -> Vec<u32> {
    let mut v: Vec<u32> = noms
        .filter_map(|n| n.strip_prefix("heros-")?.strip_suffix(".webp")?.parse().ok())
        .collect();
    v.sort_unstable();
    v
}

/// L'animation à jouer : `Some(true)` pour l'atterrissage, `Some(false)` pour
/// la traversée, `None` s'il n'y a de planches pour aucune. Au hasard, `tirage`
/// (0..1) décide ; s'il manque les planches d'une animation, c'est l'autre.
pub fn choisir_animation(mode: ModeHeros, tirage: f32, traverseurs: bool, atterrisseurs: bool) -> Option<bool> {
    let voulu = match mode {
        ModeHeros::Traversee => false,
        ModeHeros::Atterrissage => true,
        ModeHeros::Hasard => tirage < 0.5,
    };
    match (voulu, traverseurs, atterrisseurs) {
        (true, _, true) | (false, false, true) => Some(true),
        (false, true, _) | (true, true, false) => Some(false),
        _ => None,
    }
}

/// Le héros à faire partir : `choix` s'il est présent, sinon un au hasard
/// (`tirage` dans 0..1). `presents` n'est pas vide.
pub fn choisir_numero(choix: u32, presents: &[u32], tirage: f32) -> u32 {
    if presents.contains(&choix) {
        return choix;
    }
    presents[((tirage * presents.len() as f32) as usize).min(presents.len() - 1)]
}

/// La hauteur du héros à l'écran, en pixels physiques : `base` pixels logiques
/// à `pour_cent` %, au `dpi` de l'écran, jamais plus de `HAUTEUR_MAX_ECRAN` de
/// sa `hauteur_ecran`.
pub fn hauteur_a_l_ecran(base: u32, pour_cent: u32, dpi: u32, hauteur_ecran: i32) -> u32 {
    let voulue = (base as u64 * pour_cent as u64 * dpi as u64).div_ceil(100 * 96) as u32;
    let plafond = (hauteur_ecran.max(0) as f32 * HAUTEUR_MAX_ECRAN) as u32;
    voulue.min(plafond.max(16)).max(16)
}

/// Met une image 4 octets par pixel (prémultipliée) de `l`×`h` à `nl`×`nh` :
/// la moyenne en réduction, le BILINÉAIRE en agrandissement (le plus proche
/// voisin de `epingle::reechantillonner` ferait des escaliers sur un héros
/// agrandi).
pub fn mettre_a_taille(src: &[u8], (l, h): (u32, u32), (nl, nh): (u32, u32)) -> Vec<u8> {
    if (l, h) == (nl, nh) {
        return src.to_vec();
    }
    if nl <= l && nh <= h {
        return crate::win32::epingle::reechantillonner(src, l, h, nl, nh);
    }
    let mut dst = vec![0u8; (nl * nh * 4) as usize];
    // Centres de pixels alignés : la source `(x + 0.5) * l / nl - 0.5`.
    let axe = |i: u32, n: u32, taille: u32| {
        let v = ((i as f32 + 0.5) * taille as f32 / n as f32 - 0.5).clamp(0.0, (taille - 1) as f32);
        let a = v.floor() as u32;
        (a, (a + 1).min(taille - 1), v - a as f32)
    };
    let colonnes: Vec<(u32, u32, f32)> = (0..nl).map(|x| axe(x, nl, l)).collect();
    for y in 0..nh {
        let (y0, y1, fy) = axe(y, nh, h);
        let (l0, l1) = ((y0 * l) as usize * 4, (y1 * l) as usize * 4);
        let sortie = &mut dst[(y * nl) as usize * 4..((y + 1) * nl) as usize * 4];
        for (x, &(x0, x1, fx)) in colonnes.iter().enumerate() {
            let (c0, c1) = (x0 as usize * 4, x1 as usize * 4);
            for k in 0..4 {
                let haut = src[l0 + c0 + k] as f32 * (1.0 - fx) + src[l0 + c1 + k] as f32 * fx;
                let bas = src[l1 + c0 + k] as f32 * (1.0 - fx) + src[l1 + c1 + k] as f32 * fx;
                sortie[x * 4 + k] = (haut * (1.0 - fy) + bas * fy + 0.5) as u8;
            }
        }
    }
    dst
}

/// Les planches « en pixels » sont faites de blocs de `BLOC_PIXELS` pixels
/// ancrés au coin de chaque case — ⛔ la même valeur que `BLOC` de
/// `tools/super_heros_vol/styles_heros.py`.
pub const BLOC_PIXELS: u32 = 5;

/// Une case « en pixels » ramenée à sa grille : un pixel par bloc, lu au
/// centre du bloc (le bloc du bord, entamé, au centre de ce qui en reste).
/// L'affichage l'agrandit ensuite au plus proche voisin : des blocs NETS à
/// toutes les tailles, ce que la moyenne et le bilinéaire brouilleraient.
pub fn en_blocs(rgba: &[u8], (l, h): (u32, u32), bloc: u32) -> (Vec<u8>, (u32, u32)) {
    let (gl, gh) = (l.div_ceil(bloc), h.div_ceil(bloc));
    let centre = |i: u32, taille: u32| ((i * bloc + (i * bloc + bloc).min(taille)) / 2).min(taille - 1);
    let mut grille = Vec::with_capacity((gl * gh * 4) as usize);
    for gy in 0..gh {
        let y = centre(gy, h);
        for gx in 0..gl {
            let i = ((y * l + centre(gx, l)) * 4) as usize;
            grille.extend_from_slice(&rgba[i..i + 4]);
        }
    }
    (grille, (gl, gh))
}

/// `mettre_a_taille`, ou le plus proche voisin pour une image `nette` (pixels).
pub fn a_taille(src: &[u8], de: (u32, u32), vers: (u32, u32), nette: bool) -> Vec<u8> {
    if nette {
        crate::win32::epingle::reechantillonner(src, de.0, de.1, vers.0, vers.1)
    } else {
        mettre_a_taille(src, de, vers)
    }
}

/// Le chemin de la planche `heros-NN<style>[-clip].webp`, ou celle d'origine
/// si ce style n'a pas été fabriqué pour ce héros. Rend aussi le style
/// réellement servi.
pub fn chemin_planche(dossier: &std::path::Path, n: u32, style: StyleHeros, clip: &str) -> (std::path::PathBuf, StyleHeros) {
    let chemin = dossier.join(format!("heros-{n:02}{}{clip}.webp", style.suffixe()));
    if style == StyleHeros::Origine || chemin.exists() {
        (chemin, style)
    } else {
        (dossier.join(format!("heros-{n:02}{clip}.webp")), StyleHeros::Origine)
    }
}

/// Un tirage dans 0..1, sans dépendance (xorshift sur l'horloge).
fn hasard() -> f32 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static ETAT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let mut x = ETAT.load(Ordering::Relaxed) ^ nanos ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    ETAT.store(x, Ordering::Relaxed);
    (x >> 40) as f32 / (1u64 << 24) as f32
}

// ---------------------------------------------------------------- les images

fn dossier() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok()?.parent().map(|d| d.join("assets/popups/mascottes"))
}

/// Le héros prêt à voler : ses images BGRA prémultipliées. Gardées à la taille
/// `source` (jamais plus grandes que la planche : 80 images agrandies
/// coûteraient des centaines de Mo) et mises à la `taille` de l'écran image
/// par image.
struct Heros {
    images: Vec<Vec<u8>>,
    source: (u32, u32),
    taille: (u32, u32),
    /// En pixels : agrandi au plus proche voisin.
    nette: bool,
}

/// Décode et prépare le héros `n` à `hauteur` pixels physiques (dans un fil à part).
fn preparer(n: u32, hauteur: u32, vers_la_droite: bool, style: StyleHeros) -> Option<Heros> {
    let (chemin, style) = chemin_planche(&dossier()?, n, style, "");
    let octets = std::fs::read(&chemin).map_err(|e| crate::diag!("[heros] {} illisible : {e}", chemin.display())).ok()?;
    let planche = image::load_from_memory_with_format(&octets, image::ImageFormat::WebP)
        .map_err(|e| crate::diag!("[heros] {} non décodée : {e}", chemin.display()))
        .ok()?
        .to_rgba8();
    let (l, h) = planche.dimensions();
    let (cl, ch) = (l / COLONNES, h / LIGNES);
    let images = decouper(planche.as_raw(), l, (cl, ch), COLONNES * LIGNES);
    // Gardées au plus à la taille de la planche ; agrandies à l'affichage.
    let taille = ((cl as u64 * hauteur as u64 / ch as u64).max(16) as u32, hauteur.max(16));
    if style == StyleHeros::Pixels {
        let mut source = (1, 1);
        let images = images
            .iter()
            .map(|rgba| {
                let (grille, g) = en_blocs(rgba, (cl, ch), BLOC_PIXELS);
                source = g;
                let bgra = crate::win32::epingle::premultiplier_bgra(&grille);
                if vers_la_droite { bgra } else { miroir(&bgra, g.0) }
            })
            .collect();
        return Some(Heros { images, source, taille, nette: true });
    }
    let nh = hauteur.min(ch).max(16);
    let nl = (cl as u64 * nh as u64 / ch as u64).max(16) as u32;
    let images = images
        .iter()
        .map(|rgba| {
            let bgra = crate::win32::epingle::premultiplier_bgra(rgba);
            let petite = crate::win32::epingle::reechantillonner(&bgra, cl, ch, nl, nh);
            if vers_la_droite { petite } else { miroir(&petite, nl) }
        })
        .collect();
    Some(Heros { images, source: (nl, nh), taille, nette: false })
}

// ---------------------------------------------------------------- l'état

/// Ce que le fil de chargement a préparé.
enum Pret {
    Traversee(Heros),
    Atterrissage(Atterrisseur),
}

struct Vol {
    heros: Pret,
    debut: Instant,
    ecran: RECT,
    /// L'aire de travail de l'écran : l'atterrissage se pose sur son bas.
    zone: RECT,
    vers_la_droite: bool,
    y: i32,
    /// L'entrée (`GetLastInputInfo`) au départ : si elle change, le vol s'arrête.
    entree: u32,
    /// Lancé par « Essayer » : ne s'arrête pas à la première entrée.
    essai: bool,
    /// La clé de l'image peinte (voir `cadre_du_vol`).
    image_peinte: Option<u64>,
}

/// Le vol demandé, en préparation dans un autre fil.
struct Demande {
    ecran: RECT,
    zone: RECT,
    vers_la_droite: bool,
    y: i32,
    entree: u32,
    essai: bool,
}

struct Etat {
    hwnd: HWND,
    actif: bool,
    delai: Duration,
    mode: ModeHeros,
    /// Le héros choisi, ou 0 : au hasard.
    choix: u32,
    /// La taille, en pour cent.
    taille: u32,
    vol: Option<Vol>,
    demande: Option<Demande>,
    /// Fin du dernier vol et l'entrée d'alors (un nouveau vol attend un délai
    /// de plus tant que cette entrée n'a pas changé).
    dernier_vol: Option<(Instant, u32)>,
    rapide: bool,
}

thread_local! {
    static ETAT: RefCell<Option<Etat>> = const { RefCell::new(None) };
}

/// Le héros préparé par le fil de chargement, en attente d'être pris par `tic`.
static PRET: Mutex<Option<Option<Pret>>> = Mutex::new(None);

fn avec<R>(f: impl FnOnce(&mut Etat) -> R) -> Option<R> {
    ETAT.with(|e| e.try_borrow_mut().ok().and_then(|mut e| e.as_mut().map(f)))
}

/// `(GetTickCount de la dernière entrée, millisecondes d'inactivité)`.
fn derniere_entree() -> (u32, Duration) {
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return (0, Duration::ZERO);
    }
    let maintenant = unsafe { GetTickCount() };
    (info.dwTime, Duration::from_millis(maintenant.wrapping_sub(info.dwTime) as u64))
}

// ---------------------------------------------------------------- l'interface

/// Au lancement : crée la fenêtre (cachée) et sa sonde d'inactivité.
pub fn demarrer() {
    let r = crate::win32::mascotte::reglages();
    let Some(hwnd) = creer_fenetre() else { return };
    crate::gfx::capture_visibility::apply_to_window(hwnd);
    let etat = Etat {
        hwnd,
        actif: r.heros,
        delai: Duration::from_secs(r.heros_delai as u64 * 60),
        mode: r.heros_mode,
        choix: r.heros_choix,
        taille: r.heros_taille,
        vol: None,
        demande: None,
        dernier_vol: None,
        rapide: false,
    };
    ETAT.with(|e| {
        if let Ok(mut e) = e.try_borrow_mut() {
            *e = Some(etat);
        }
    });
    unsafe { SetTimer(Some(hwnd), MINUTERIE, SONDE_MS, None) };
}

pub fn choisir_actif(oui: bool) {
    let mut r = crate::win32::mascotte::reglages();
    r.heros = oui;
    crate::win32::mascotte::enregistrer(&r);
    avec(|e| e.actif = oui);
}

pub fn choisir_delai(minutes: u32) {
    let mut r = crate::win32::mascotte::reglages();
    r.heros_delai = minutes.clamp(crate::win32::mascotte::HEROS_DELAI_MIN, crate::win32::mascotte::HEROS_DELAI_MAX);
    let delai = Duration::from_secs(r.heros_delai as u64 * 60);
    crate::win32::mascotte::enregistrer(&r);
    avec(|e| e.delai = delai);
}

pub fn choisir_mode(mode: ModeHeros) {
    let mut r = crate::win32::mascotte::reglages();
    r.heros_mode = mode;
    crate::win32::mascotte::enregistrer(&r);
    avec(|e| e.mode = mode);
}

/// Lu au départ de chaque vol (`lancer`) : rien à garder dans l'état.
pub fn choisir_style(style: StyleHeros) {
    let mut r = crate::win32::mascotte::reglages();
    r.heros_style = style;
    crate::win32::mascotte::enregistrer(&r);
}

pub fn choisir_heros(n: u32) {
    let mut r = crate::win32::mascotte::reglages();
    r.heros_choix = n;
    crate::win32::mascotte::enregistrer(&r);
    avec(|e| e.choix = n);
}

pub fn choisir_taille(pour_cent: u32) {
    let mut r = crate::win32::mascotte::reglages();
    r.heros_taille = pour_cent.clamp(crate::win32::mascotte::HEROS_TAILLE_MIN, crate::win32::mascotte::HEROS_TAILLE_MAX);
    let taille = r.heros_taille;
    crate::win32::mascotte::enregistrer(&r);
    avec(|e| e.taille = taille);
}

/// Les héros livrés (`heros-NN.webp` ou `heros-NN-atterrissage.txt`), pour la
/// grille des Réglages.
pub fn tous_les_heros() -> Vec<u32> {
    let noms: Vec<String> = dossier()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|it| it.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    let mut v = heros_presents(noms.iter().map(String::as_str));
    v.extend(atterrissage::heros_atterrisseurs(noms.iter().map(String::as_str)));
    v.sort_unstable();
    v.dedup();
    v
}

/// « Essayer » : un vol tout de suite, qui ne s'arrête pas au premier geste.
pub fn essayer() {
    let Some(hwnd) = avec(|e| e.hwnd) else { return };
    let (entree, _) = derniere_entree();
    lancer(hwnd, entree, true);
}

// ---------------------------------------------------------------- le vol

fn ecran_du_pointeur() -> (HMONITOR, RECT, RECT) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
        let ecran = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(ecran, &mut info);
        (ecran, info.rcMonitor, info.rcWork)
    }
}

/// Choisit un héros, un sens, une hauteur, et lance sa préparation.
fn lancer(hwnd: HWND, entree: u32, essai: bool) {
    if avec(|e| e.vol.is_some() || e.demande.is_some()).unwrap_or(true) {
        return;
    }
    let Some(dossier) = dossier() else { return };
    let noms: Vec<String> = std::fs::read_dir(&dossier)
        .map(|it| it.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    let traverseurs = heros_presents(noms.iter().map(String::as_str));
    let atterrisseurs = atterrissage::heros_atterrisseurs(noms.iter().map(String::as_str));
    let (mode, choix, pour_cent) = avec(|e| (e.mode, e.choix, e.taille)).unwrap_or((ModeHeros::Hasard, 0, 100));
    let style = crate::win32::mascotte::reglages().heros_style;
    // Le héros choisi n'a peut-être qu'une des deux animations : on joue celle-là.
    let (traverse, atterrit_aussi) = if choix != 0 && (traverseurs.contains(&choix) || atterrisseurs.contains(&choix)) {
        (traverseurs.contains(&choix), atterrisseurs.contains(&choix))
    } else {
        (!traverseurs.is_empty(), !atterrisseurs.is_empty())
    };
    let Some(atterrit) = choisir_animation(mode, hasard(), traverse, atterrit_aussi) else {
        crate::diag!("[heros] aucune planche de super-héros : pas de vol");
        return;
    };
    let presents = if atterrit { &atterrisseurs } else { &traverseurs };
    let n = choisir_numero(choix, presents, hasard());
    let (ecran, rect, zone) = ecran_du_pointeur();
    let dpi = crate::win32::dpi::get_dpi_for_monitor(ecran);
    let base = if atterrit { DEBOUT_LOGIQUE } else { HAUTEUR_LOGIQUE };
    let hauteur = hauteur_a_l_ecran(base, pour_cent, dpi, zone.bottom - zone.top);
    let vers_la_droite = hasard() < 0.5;
    let haut = rect.top + (rect.bottom - rect.top) / 10;
    let bas = rect.bottom - hauteur as i32 - (rect.bottom - rect.top) / 5;
    let y = haut + (hasard() * (bas - haut).max(0) as f32) as i32;
    avec(|e| e.demande = Some(Demande { ecran: rect, zone, vers_la_droite, y, entree, essai }));
    crate::diag!(
        "[heros] départ du héros {n} ({}, {}, {hauteur} px, vers la {})",
        if atterrit { "atterrissage" } else { "traversée" },
        style.nom(),
        if vers_la_droite { "droite" } else { "gauche" }
    );
    std::thread::spawn(move || {
        let heros = if atterrit {
            atterrissage::preparer(&dossier, n, hauteur, vers_la_droite, style).map(Pret::Atterrissage)
        } else {
            preparer(n, hauteur, vers_la_droite, style).map(Pret::Traversee)
        };
        if let Ok(mut p) = PRET.lock() {
            *p = Some(heros);
        }
    });
    // Le fil répond vite : on le guette au rythme de l'animation.
    unsafe { SetTimer(Some(hwnd), MINUTERIE, ANIME_MS, None) };
    avec(|e| e.rapide = true);
}

fn tic() {
    let maintenant = Instant::now();
    let (entree, inactif) = derniere_entree();

    // Un héros vient d'être préparé : il décolle.
    let pret = PRET.lock().ok().and_then(|mut p| p.take());
    if let Some(heros) = pret {
        avec(|e| {
            let d = e.demande.take();
            match (heros, d) {
                (Some(heros), Some(d)) => {
                    e.vol = Some(Vol {
                        heros,
                        debut: maintenant,
                        ecran: d.ecran,
                        zone: d.zone,
                        vers_la_droite: d.vers_la_droite,
                        y: d.y,
                        entree: d.entree,
                        essai: d.essai,
                        image_peinte: None,
                    })
                }
                _ => e.dernier_vol = Some((maintenant, entree)),
            }
        });
    }

    let Some(travail) = avec(|e| avancer(e, maintenant, entree, inactif)) else { return };
    match travail {
        Travail::Rien => {}
        Travail::Lancer(hwnd) => lancer(hwnd, entree, false),
        Travail::Peindre { hwnd, position, taille, pixels, montrer } => unsafe {
            poser_image(hwnd, position, taille, &pixels);
            if montrer {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE);
            }
        },
        Travail::Deplacer { hwnd, position } => unsafe {
            let _ = SetWindowPos(hwnd, None, position.x, position.y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER);
        },
        Travail::Atterrir(hwnd) => unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
            SetTimer(Some(hwnd), MINUTERIE, SONDE_MS, None);
        },
    }
}

enum Travail {
    Rien,
    Lancer(HWND),
    Peindre { hwnd: HWND, position: POINT, taille: (u32, u32), pixels: Vec<u8>, montrer: bool },
    Deplacer { hwnd: HWND, position: POINT },
    Atterrir(HWND),
}

fn avancer(e: &mut Etat, maintenant: Instant, entree: u32, inactif: Duration) -> Travail {
    let Some(vol) = e.vol.as_mut() else {
        if e.demande.is_some() || !e.actif {
            return Travail::Rien;
        }
        let depuis_vol = e.dernier_vol.filter(|(_, ent)| *ent == entree).map(|(fin, _)| maintenant.duration_since(fin));
        if !doit_partir(inactif, e.delai, depuis_vol) {
            return Travail::Rien;
        }
        // Jamais devant un film ou un jeu.
        let (ecran, _, _) = ecran_du_pointeur();
        if crate::win32::window::fullscreen_window_on_monitor(e.hwnd, ecran).is_some() {
            return Travail::Rien;
        }
        return Travail::Lancer(e.hwnd);
    };

    let ecoule = maintenant.duration_since(vol.debut).as_secs_f32();
    let interrompu = !vol.essai && entree != vol.entree;
    let cadre = if interrompu { None } else { cadre_du_vol(vol, ecoule) };
    let Some((cle, position, taille, pixels)) = cadre else {
        e.vol = None;
        e.dernier_vol = Some((maintenant, entree));
        e.rapide = false;
        return Travail::Atterrir(e.hwnd);
    };
    // Copiés seulement s'ils changent ; la copie libère l'emprunt de `vol`.
    let meme_image = vol.image_peinte == Some(cle);
    let pixels = if meme_image { Vec::new() } else { pixels.into_owned() };
    if meme_image {
        return Travail::Deplacer { hwnd: e.hwnd, position };
    }
    let montrer = vol.image_peinte.is_none();
    vol.image_peinte = Some(cle);
    Travail::Peindre { hwnd: e.hwnd, position, taille, pixels, montrer }
}

/// L'image du vol à `ecoule` secondes : `(clé, coin haut-gauche, taille,
/// pixels)`, ou `None` quand il est fini. La clé change avec les pixels.
fn cadre_du_vol(vol: &Vol, ecoule: f32) -> Option<(u64, POINT, (u32, u32), std::borrow::Cow<'_, [u8]>)> {
    match &vol.heros {
        Pret::Traversee(heros) => {
            let t = ecoule / TRAVERSEE.as_secs_f32();
            if t >= 1.0 {
                return None;
            }
            let pos = position(t, vol.ecran, heros.taille, vol.vers_la_droite, vol.y);
            let image = (ecoule * IMAGES_PAR_SECONDE) as usize % heros.images.len();
            let px = &heros.images[image];
            let px = if heros.source == heros.taille {
                std::borrow::Cow::Borrowed(px.as_slice())
            } else {
                std::borrow::Cow::Owned(a_taille(px, heros.source, heros.taille, heros.nette))
            };
            Some((image as u64, pos, heros.taille, px))
        }
        Pret::Atterrissage(a) => atterrissage::cadre(a, ecoule, vol.zone, vol.ecran, vol.vers_la_droite),
    }
}

// ---------------------------------------------------------------- la fenêtre

fn creer_fenetre() -> Option<HWND> {
    static CLASSE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).ok()?.into();
        CLASSE.get_or_init(|| {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                lpszClassName: w!("NeuraDeskSuperHeros"),
                ..Default::default()
            };
            RegisterClassExW(&wc);
        });
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("NeuraDeskSuperHeros"),
            w!("NeuraDesk — Super-héros"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance),
            None,
        )
        .map_err(|err| crate::diag!("[heros] CreateWindowExW a échoué : {err}"))
        .ok()
    }
}

/// Peint `pixels` (BGRA prémultiplié, `l`×`h`) avec le coin haut-gauche en `position`.
fn poser_image(hwnd: HWND, position: POINT, (l, h): (u32, u32), pixels: &[u8]) {
    let (l, h) = (l as i32, h as i32);
    unsafe {
        let ecran = GetDC(None);
        let memoire = CreateCompatibleDC(Some(ecran));
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: l,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        match CreateDIBSection(Some(memoire), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bitmap) if !bits.is_null() => {
                std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
                let ancien = SelectObject(memoire, bitmap.into());
                let fusion = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                if let Err(err) = UpdateLayeredWindow(
                    hwnd,
                    Some(ecran),
                    Some(&position),
                    Some(&SIZE { cx: l, cy: h }),
                    Some(memoire),
                    Some(&POINT::default()),
                    COLORREF(0),
                    Some(&fusion),
                    ULW_ALPHA,
                ) {
                    crate::diag!("[heros] UpdateLayeredWindow a échoué : {err}");
                }
                SelectObject(memoire, ancien);
                let _ = DeleteObject(bitmap.into());
            }
            _ => crate::diag!("[heros] CreateDIBSection {l}×{h} a échoué"),
        }
        let _ = DeleteDC(memoire);
        ReleaseDC(None, ecran);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            tic();
            LRESULT(0)
        }
        // Le clic passe au travers, même hors de `WS_EX_TRANSPARENT`.
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    #[test]
    fn le_vol_part_apres_le_delai_puis_attend_un_delai_de_plus() {
        assert!(!doit_partir(MIN * 4, MIN * 5, None), "avant le délai : rien");
        assert!(doit_partir(MIN * 5, MIN * 5, None), "au délai : départ");
        assert!(!doit_partir(MIN * 9, MIN * 5, Some(MIN * 2)), "un vol fini il y a 2 min : on attend");
        assert!(doit_partir(MIN * 11, MIN * 5, Some(MIN * 5)), "un délai après le dernier vol : on repart");
    }

    #[test]
    fn la_traversee_part_et_finit_entierement_hors_de_l_ecran() {
        let ecran = RECT { left: 1920, top: 0, right: 1920 + 3840, bottom: 2160 };
        let taille = (500, 300);
        // Vers la droite : il commence à gauche de l'écran, finit à droite.
        let a = position(0.0, ecran, taille, true, 400);
        let b = position(1.0, ecran, taille, true, 400);
        assert_eq!(a.x + 500, ecran.left, "entièrement hors de l'écran au départ");
        assert_eq!(b.x, ecran.right, "entièrement hors de l'écran à l'arrivée");
        // Vers la gauche : le chemin inverse, sur le même écran.
        let c = position(0.0, ecran, taille, false, 400);
        let d = position(1.0, ecran, taille, false, 400);
        assert_eq!(c.x, ecran.right);
        assert_eq!(d.x + 500, ecran.left);
        // L'ondulation reste bornée.
        for k in 0..=100 {
            let p = position(k as f32 / 100.0, ecran, taille, true, 400);
            assert!((p.y - 400).abs() <= (ONDULATION * 300.0).ceil() as i32);
        }
    }

    #[test]
    fn la_planche_se_decoupe_en_quatre_vingts_images_ligne_par_ligne() {
        let (cl, ch) = (3u32, 2u32);
        let (l, h) = (cl * COLONNES, ch * LIGNES);
        // Chaque pixel porte le numéro de son image.
        let mut rgba = vec![0u8; (l * h * 4) as usize];
        for y in 0..h {
            for x in 0..l {
                let n = (y / ch) * COLONNES + x / cl;
                rgba[((y * l + x) * 4) as usize] = n as u8;
            }
        }
        let images = decouper(&rgba, l, (cl, ch), COLONNES * LIGNES);
        assert_eq!(images.len(), 80);
        for (n, im) in images.iter().enumerate() {
            assert!(im.chunks_exact(4).all(|p| p[0] == n as u8), "image {n} mal découpée");
        }
    }

    #[test]
    fn l_animation_suit_le_choix_et_se_rabat_sur_celle_qui_existe() {
        use ModeHeros::*;
        assert_eq!(choisir_animation(Traversee, 0.1, true, true), Some(false));
        assert_eq!(choisir_animation(Atterrissage, 0.9, true, true), Some(true));
        assert_eq!(choisir_animation(Hasard, 0.1, true, true), Some(true));
        assert_eq!(choisir_animation(Hasard, 0.9, true, true), Some(false));
        assert_eq!(choisir_animation(Atterrissage, 0.0, true, false), Some(false), "pas encore d'atterrisseur");
        assert_eq!(choisir_animation(Traversee, 0.0, false, true), Some(true));
        assert_eq!(choisir_animation(Hasard, 0.0, false, false), None);
    }

    #[test]
    fn le_heros_choisi_part_sinon_un_au_hasard() {
        let presents = [1, 2, 3, 17];
        assert_eq!(choisir_numero(17, &presents, 0.0), 17);
        assert_eq!(choisir_numero(0, &presents, 0.0), 1, "0 : au hasard");
        assert_eq!(choisir_numero(0, &presents, 0.99), 17);
        assert_eq!(choisir_numero(42, &presents, 0.5), 3, "absent : au hasard");
    }

    #[test]
    fn la_taille_suit_le_pour_cent_et_le_dpi_sans_deborder_de_l_ecran() {
        assert_eq!(hauteur_a_l_ecran(200, 100, 96, 1080), 200);
        assert_eq!(hauteur_a_l_ecran(200, 100, 144, 2160), 300);
        assert_eq!(hauteur_a_l_ecran(200, 250, 144, 2160), 750);
        assert_eq!(hauteur_a_l_ecran(200, 50, 96, 1080), 100);
        assert_eq!(hauteur_a_l_ecran(200, 300, 192, 1080), 864, "plafonné à 80 % de l'écran");
    }

    #[test]
    fn agrandir_garde_un_aplat_et_lisse_un_bord() {
        // Un aplat reste un aplat.
        let aplat: Vec<u8> = [10, 20, 30, 255].repeat(4);
        let grand = mettre_a_taille(&aplat, (2, 2), (5, 7));
        assert_eq!(grand.len(), 5 * 7 * 4);
        assert!(grand.chunks_exact(4).all(|p| p == [10, 20, 30, 255]));
        // Un bord noir|blanc devient une rampe : pas d'escalier.
        let bord = [0, 0, 0, 255, 255, 255, 255, 255];
        let rampe = mettre_a_taille(&bord, (2, 1), (8, 1));
        let v: Vec<u8> = rampe.chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!((v[0], v[7]), (0, 255), "les bords restent purs");
        assert!(v.windows(2).all(|w| w[0] <= w[1]), "croissante : {v:?}");
        assert!(v.iter().any(|&c| c > 0 && c < 255), "des tons intermédiaires : {v:?}");
        // Réduire passe par la moyenne.
        assert_eq!(mettre_a_taille(&bord, (2, 1), (1, 1))[0], 128);
    }

    /// Le coût d'un agrandissement au pire cas (300 %, écran 4K) : il se fait
    /// sur le fil de l'interface, 16 fois par seconde.
    #[test]
    #[ignore = "mesure à la main"]
    fn mesure_de_l_agrandissement() {
        let src = vec![128u8; 400 * 320 * 4];
        let debut = Instant::now();
        for _ in 0..20 {
            std::hint::black_box(mettre_a_taille(&src, (400, 320), (1080, 864)));
        }
        eprintln!("agrandir 400×320 → 1080×864 : {:.2} ms", debut.elapsed().as_secs_f64() * 1000.0 / 20.0);
    }

    #[test]
    fn le_miroir_retourne_chaque_ligne() {
        let px = [1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0];
        let m = miroir(&px, 3);
        let premiers: Vec<u8> = m.chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!(premiers, [3, 2, 1, 6, 5, 4]);
    }

    #[test]
    fn seuls_les_fichiers_heros_nn_comptent() {
        let noms = ["heros-02.webp", "fox-directions.webp", "heros-10.webp", "heros-xx.webp", "heros-01.png"];
        assert_eq!(heros_presents(noms.into_iter()), [2, 10]);
    }

    /// Les planches livrées : chacune se décode et se découpe en images non vides.
    #[test]
    fn chaque_planche_livree_se_decoupe() {
        let dossier = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/popups/mascottes");
        let noms: Vec<String> = std::fs::read_dir(&dossier)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        let presents = heros_presents(noms.iter().map(String::as_str));
        for n in presents {
            let mut origine = None;
            for style in [StyleHeros::Origine, StyleHeros::Encre, StyleHeros::Pixels] {
                let (chemin, servi) = chemin_planche(&dossier, n, style, "");
                assert_eq!(servi, style, "héros {n} : style {style:?} pas livré");
                let octets = std::fs::read(chemin).unwrap();
                let planche = image::load_from_memory(&octets).unwrap().to_rgba8();
                let (l, h) = planche.dimensions();
                assert_eq!((l % COLONNES, h % LIGNES), (0, 0), "héros {n} : planche {l}×{h} pas en 8×10");
                assert_eq!(*origine.get_or_insert((l, h)), (l, h), "héros {n} {style:?} : pas aux dimensions d'origine");
                let images = decouper(planche.as_raw(), l, (l / COLONNES, h / LIGNES), COLONNES * LIGNES);
                for (k, im) in images.iter().enumerate() {
                    assert!(im.chunks_exact(4).any(|p| p[3] > 128), "héros {n} {style:?} : image {k} vide");
                }
            }
        }
    }

    #[test]
    fn la_grille_des_pixels_lit_le_centre_de_chaque_bloc() {
        // 7×3 en blocs de 3 : grille 3×1 ; le dernier bloc (1 colonne) lu sur elle.
        let px: Vec<u8> = (0..7 * 3).flat_map(|i| [(i % 7) as u8, (i / 7) as u8, 0, 255]).collect();
        let (g, taille) = en_blocs(&px, (7, 3), 3);
        assert_eq!(taille, (3, 1));
        let lus: Vec<(u8, u8)> = g.chunks_exact(4).map(|p| (p[0], p[1])).collect();
        assert_eq!(lus, [(1, 1), (4, 1), (6, 1)]);
        // Agrandie au plus proche voisin : des blocs nets, sans valeur intermédiaire.
        let grand = a_taille(&g, (3, 1), (9, 3), true);
        assert!(grand.chunks_exact(4).all(|p| [1, 4, 6].contains(&p[0])));
    }
}
