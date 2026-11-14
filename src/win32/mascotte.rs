//! **LA MASCOTTE** — DEMANDE EXPLICITE (2026-11-14) : « afficher un petit avatar
//! en surimpression à l'écran que je puisse déplacer où je veux et l'utiliser en
//! tant qu'applet », puis « tous les 52, réaction au clic, affichable en plein
//! écran suivant les réglages ».
//!
//! Les personnages et leur comportement viennent de **page-mascot** (Kamran
//! Ahmed, licence MIT, `nilbuild/page-mascot`) : deux planches 3×3 par
//! personnage, l'une pour les NEUF DIRECTIONS du regard, l'autre pour NEUF
//! RÉACTIONS. Les constantes du regard (huit secteurs, hystérésis, zone morte) et
//! du « boop » (clin d'œil → cœur/étincelles/ravi, étourdi après quatre clics
//! rapides, écrasement élastique de 420 ms) sont celles de `src/mascot.tsx`.
//!
//! **PAS DE CEF ICI**, comme `win32::epingle` : une fenêtre `WS_EX_LAYERED`
//! peinte par `UpdateLayeredWindow` depuis un DIB 32 bits. Au repos elle ne
//! coûte qu'une lecture de curseur toutes les 50 ms ; elle ne se REPEINT que
//! quand l'image change (une direction, une réaction, une image de l'écrasement).
//!
//! - **Glisser la déplace** (sa place est retenue) ; un clic SANS glisser la fait
//!   réagir ; la molette change sa taille.
//! - **Plein écran** : par défaut elle s'efface devant une application plein
//!   écran sur SON écran, comme les barres ; le réglage « rester visible » la
//!   garde au-dessus (pas au-dessus d'un jeu en plein écran EXCLUSIF, que
//!   Windows ne laisse recouvrir par personne).
//! - Les planches sont des fichiers (`assets/popups/mascottes/`), lus à la
//!   demande : la page des Réglages s'en sert aussi pour ses vignettes.

use std::cell::RefCell;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW, MonitorFromPoint,
    MonitorFromWindow, ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER,
    BLENDFUNCTION, DIB_RGB_COLORS, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL,
    MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetSystemMetrics, KillTimer, LoadCursorW,
    RegisterClassExW, SetTimer, SetWindowPos, ShowWindow, UpdateLayeredWindow, HWND_TOPMOST, IDC_HAND,
    MA_NOACTIVATE, SM_CXDRAG, SM_CYDRAG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE,
    SW_SHOWNOACTIVATE, ULW_ALPHA, WM_CAPTURECHANGED, WM_DPICHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

/// Un personnage : son nom de fichier (celui de page-mascot), son libellé, sa
/// famille (les groupes de la page des Réglages).
pub struct Personnage {
    pub nom: &'static str,
    pub libelle: &'static str,
    pub famille: &'static str,
}

const fn p(nom: &'static str, libelle: &'static str, famille: &'static str) -> Personnage {
    Personnage { nom, libelle, famille }
}

/// **LES 52 PERSONNAGES** de page-mascot (sa liste `reference/characters.md`
/// plus la souris de `site/mascots.ts` ; le portrait de l'auteur n'en fait pas
/// partie), puis le renard dans ses cinq autres styles de dessin.
pub const PERSONNAGES: &[Personnage] = &[
    p("bear", "Ours", "Animaux"),
    p("bunny", "Lapin", "Animaux"),
    p("cat", "Chat", "Animaux"),
    p("deer", "Cerf", "Animaux"),
    p("dino", "Dinosaure", "Animaux"),
    p("fox", "Renard", "Animaux"),
    p("frog", "Grenouille", "Animaux"),
    p("hamster", "Hamster", "Animaux"),
    p("hedgehog", "Hérisson", "Animaux"),
    p("koala", "Koala", "Animaux"),
    p("mouse", "Souris", "Animaux"),
    p("otter", "Loutre", "Animaux"),
    p("owl", "Hibou", "Animaux"),
    p("panda", "Panda", "Animaux"),
    p("penguin", "Manchot", "Animaux"),
    p("pug", "Carlin", "Animaux"),
    p("raccoon", "Raton laveur", "Animaux"),
    p("redpanda", "Panda roux", "Animaux"),
    p("sheep", "Mouton", "Animaux"),
    p("sloth", "Paresseux", "Animaux"),
    p("tiger", "Tigre", "Animaux"),
    p("afro", "Afro", "Personnes"),
    p("astronaut", "Astronaute", "Personnes"),
    p("bald", "Crâne rasé", "Personnes"),
    p("ballerina", "Ballerine", "Personnes"),
    p("beard", "Barbu", "Personnes"),
    p("builder", "Ouvrier", "Personnes"),
    p("cap", "Casquette", "Personnes"),
    p("chef", "Cuisinier", "Personnes"),
    p("glasses", "Lunettes", "Personnes"),
    p("grandpa", "Grand-père", "Personnes"),
    p("granny", "Grand-mère", "Personnes"),
    p("hijabi", "Hijab", "Personnes"),
    p("nurse", "Infirmière", "Personnes"),
    p("pirate", "Pirate", "Personnes"),
    p("scientist", "Scientifique", "Personnes"),
    p("sikh", "Turban", "Personnes"),
    p("skater", "Skateur", "Personnes"),
    p("wizard", "Magicien", "Personnes"),
    p("clockwork", "Automate", "Robots et objets"),
    p("crt", "Écran cathodique", "Robots et objets"),
    p("cube", "Cube", "Robots et objets"),
    p("drone", "Drone", "Robots et objets"),
    p("gearbot", "Robot à engrenages", "Robots et objets"),
    p("knight", "Chevalier", "Robots et objets"),
    p("lantern", "Lanterne", "Robots et objets"),
    p("postbot", "Robot postier", "Robots et objets"),
    p("radio", "Radio", "Robots et objets"),
    p("rocket", "Fusée", "Robots et objets"),
    p("scout", "Éclaireur", "Robots et objets"),
    p("toaster", "Grille-pain", "Robots et objets"),
    p("tv", "Télévision", "Robots et objets"),
    p("fox-ink", "Renard à l'encre", "Styles du renard"),
    p("fox-sketch", "Renard au crayon", "Styles du renard"),
    p("fox-riso", "Renard en risographie", "Styles du renard"),
    p("fox-paper", "Renard en papier découpé", "Styles du renard"),
    p("fox-pixel", "Renard en pixels", "Styles du renard"),
];

pub const PAR_DEFAUT: &str = "fox";
/// Taille en pixels LOGIQUES (multipliée par l'échelle de l'écran).
pub const TAILLE_MIN: u32 = 64;
pub const TAILLE_MAX: u32 = 320;
pub const TAILLE_DEFAUT: u32 = 140;
const PAS_MOLETTE: u32 = 10;

pub fn existe(nom: &str) -> bool {
    PERSONNAGES.iter().any(|p| p.nom == nom)
}

// ---------------------------------------------------------------- réglages

/// Les réglages de la mascotte, un fichier `cle=valeur` (`mascotte.txt`).
#[derive(Clone, Debug, PartialEq)]
pub struct Reglages {
    pub personnage: String,
    pub taille: u32,
    /// Rester visible par-dessus une application plein écran.
    pub plein_ecran: bool,
    /// Affichée — retenu pour la retrouver au lancement suivant.
    pub visible: bool,
    /// Coin haut-gauche de la fenêtre, en pixels d'écran.
    pub position: Option<(i32, i32)>,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { personnage: PAR_DEFAUT.into(), taille: TAILLE_DEFAUT, plein_ecran: false, visible: false, position: None }
    }
}

/// Lit `mascotte.txt` ; toute ligne mal formée garde la valeur par défaut.
pub fn lire_reglages(texte: &str) -> Reglages {
    let mut r = Reglages::default();
    let (mut x, mut y) = (None, None);
    for ligne in texte.lines() {
        let Some((cle, valeur)) = ligne.split_once('=') else { continue };
        let valeur = valeur.trim();
        match cle.trim() {
            "personnage" if existe(valeur) => r.personnage = valeur.into(),
            "taille" => {
                if let Ok(t) = valeur.parse::<u32>() {
                    r.taille = t.clamp(TAILLE_MIN, TAILLE_MAX);
                }
            }
            "plein_ecran" => r.plein_ecran = valeur == "1",
            "visible" => r.visible = valeur == "1",
            "x" => x = valeur.parse::<i32>().ok(),
            "y" => y = valeur.parse::<i32>().ok(),
            _ => {}
        }
    }
    r.position = x.zip(y);
    r
}

pub fn ecrire_reglages(r: &Reglages) -> String {
    let mut s = format!(
        "personnage={}\ntaille={}\nplein_ecran={}\nvisible={}\n",
        r.personnage,
        r.taille,
        u8::from(r.plein_ecran),
        u8::from(r.visible)
    );
    if let Some((x, y)) = r.position {
        s.push_str(&format!("x={x}\ny={y}\n"));
    }
    s
}

pub fn reglages() -> Reglages {
    crate::config::load_mascotte().map(|t| lire_reglages(&t)).unwrap_or_default()
}

fn enregistrer(r: &Reglages) {
    crate::config::save_mascotte(&ecrire_reglages(r));
}

// ---------------------------------------------------------------- le regard

/// Les cases des planches, dans l'ordre de page-mascot (ligne par ligne).
const CENTRE: usize = 4;
/// Les réactions utilisées ici (index dans la planche des réactions).
const CLIGNE: usize = 0;
const ETONNE: usize = 3;
const ENDORMI: usize = 6;
const ETOURDI: usize = 7;
/// cœur, étincelles, ravi.
const RECOMPENSES: [usize; 3] = [1, 2, 8];

/// Dans le sens des aiguilles d'une montre depuis la droite (y vers le bas,
/// comme `atan2`) : droite, bas-droite, bas, bas-gauche, gauche, haut-gauche,
/// haut, haut-droite — en cases de la planche des directions.
const HORAIRE: [usize; 8] = [5, 8, 7, 6, 3, 0, 1, 2];
const SECTEUR: f32 = std::f32::consts::TAU / 8.0;
const HYSTERESIS: f32 = 0.12;
/// La zone morte de page-mascot : 70 px pour une mascotte de 140 — la moitié
/// de sa taille, rapportée ici à la taille affichée.
const ZONE_MORTE_PART: f32 = 0.5;

fn replier(angle: f32) -> f32 {
    angle.sin().atan2(angle.cos())
}

/// Où regarder pour un curseur à (`dx`, `dy`) du centre : rend le secteur
/// retenu (`None` = le centre) et la case. Le secteur courant TIENT tant que le
/// curseur n'a pas franchi son bord de `HYSTERESIS` — pas de tremblement entre
/// deux cases.
fn viser(dx: f32, dy: f32, zone_morte: f32, secteur: Option<usize>) -> (Option<usize>, usize) {
    if dx.hypot(dy) < zone_morte {
        return (None, CENTRE);
    }
    let angle = dy.atan2(dx);
    if let Some(s) = secteur {
        if replier(angle - s as f32 * SECTEUR).abs() < SECTEUR / 2.0 + HYSTERESIS {
            return (Some(s), HORAIRE[s]);
        }
    }
    let s = ((angle / SECTEUR).round() as i32).rem_euclid(8) as usize;
    (Some(s), HORAIRE[s])
}

// ---------------------------------------------------------------- le boop

const BOOP_RECOMPENSE: Duration = Duration::from_millis(120);
const BOOP_FIN: Duration = Duration::from_millis(560);
const ETOURDI_APRES: u32 = 4;
const ETOURDI_FENETRE: Duration = Duration::from_millis(1600);
const ETOURDI_FIN: Duration = Duration::from_millis(1100);
const ECRASEMENT: Duration = Duration::from_millis(420);
/// Sans bouger la souris pendant ce temps, la mascotte s'endort (réaction
/// « endormie ») — elle se réveille au premier mouvement.
const SOMMEIL: Duration = Duration::from_secs(120);

#[derive(Default)]
struct Boops {
    compte: u32,
    dernier: Option<Instant>,
}

/// Un clic : rend la suite des réactions (délai depuis le clic, réaction ou
/// `None` = fin). Quatre clics en moins de 1,6 s la rendent étourdie.
fn boop(b: &mut Boops, maintenant: Instant) -> Vec<(Duration, Option<usize>)> {
    let proche = b.dernier.is_some_and(|d| maintenant.duration_since(d) < ETOURDI_FENETRE);
    b.compte = if proche { b.compte + 1 } else { 1 };
    b.dernier = Some(maintenant);
    if b.compte >= ETOURDI_APRES {
        b.compte = 0;
        return vec![(Duration::ZERO, Some(ETOURDI)), (ETOURDI_FIN, None)];
    }
    let recompense = RECOMPENSES[(b.compte as usize - 1) % RECOMPENSES.len()];
    vec![(Duration::ZERO, Some(CLIGNE)), (BOOP_RECOMPENSE, Some(recompense)), (BOOP_FIN, None)]
}

/// Courbe de Bézier CSS `cubic-bezier(x1, y1, x2, y2)` évaluée en `x`.
fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let courbe = |a: f32, b: f32, t: f32| 3.0 * (1.0 - t) * (1.0 - t) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t;
    let (mut bas, mut haut) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let milieu = (bas + haut) / 2.0;
        if courbe(x1, x2, milieu) < x {
            bas = milieu;
        } else {
            haut = milieu;
        }
    }
    courbe(y1, y2, (bas + haut) / 2.0)
}

/// L'écrasement de page-mascot : (échelle x, échelle y) à la fraction `t` de
/// ses 420 ms. Chaque segment a SA courbe (ease-in, ease-out, ease-in-out ×2).
fn ecrasement(t: f32) -> (f32, f32) {
    const CLES: [(f32, f32, f32); 5] = [(0.0, 1.0, 1.0), (0.18, 1.10, 0.86), (0.45, 0.95, 1.08), (0.72, 1.03, 0.97), (1.0, 1.0, 1.0)];
    const COURBES: [(f32, f32, f32, f32); 4] = [(0.42, 0.0, 1.0, 1.0), (0.0, 0.0, 0.58, 1.0), (0.42, 0.0, 0.58, 1.0), (0.42, 0.0, 0.58, 1.0)];
    let t = t.clamp(0.0, 1.0);
    let i = CLES.windows(2).position(|c| t <= c[1].0).unwrap_or(3);
    let (a, b) = (CLES[i], CLES[i + 1]);
    let (x1, y1, x2, y2) = COURBES[i];
    let k = bezier(x1, y1, x2, y2, (t - a.0) / (b.0 - a.0));
    (a.1 + (b.1 - a.1) * k, a.2 + (b.2 - a.2) * k)
}

/// L'écrasement pivote autour de ce point de la case (`transform-origin: 50% 78%`).
const PIVOT_Y: f32 = 0.78;
/// Marge autour de la case pour que l'écrasement (jusqu'à ×1,10 en largeur,
/// ×1,08 en hauteur autour du pivot) ne soit jamais rogné : 6,3 % au plus, plus
/// le pixel que la lecture bilinéaire mord au-delà du bord.
const MARGE_PART: f32 = 0.09;

fn marge(cote: u32) -> u32 {
    (cote as f32 * MARGE_PART).ceil() as u32
}

// ---------------------------------------------------------------- les images

/// Le dossier des planches, à côté de l'exe (copié par `build.rs`).
fn dossier() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok()?.parent().map(|d| d.join("assets/popups/mascottes"))
}

/// Une planche décodée, en RGBA droit, et son côté.
fn lire_planche(nom: &str, sorte: &str) -> Option<(Vec<u8>, u32)> {
    let chemin = dossier()?.join(format!("{nom}-{sorte}.webp"));
    let octets = std::fs::read(&chemin).map_err(|e| crate::diag!("[mascotte] {} illisible : {e}", chemin.display())).ok()?;
    let image = image::load_from_memory_with_format(&octets, image::ImageFormat::WebP)
        .map_err(|e| crate::diag!("[mascotte] {} non décodée : {e}", chemin.display()))
        .ok()?
        .to_rgba8();
    let (l, h) = image.dimensions();
    (l == h && l >= 3).then(|| (image.into_raw(), l))
}

/// Découpe une planche 3×3 (RGBA droit, `cote`×`cote`) en neuf cases BGRA
/// prémultipliées, ramenées à `taille` pixels de côté.
fn decouper(rgba: &[u8], cote: u32, taille: u32) -> Vec<Vec<u8>> {
    let case = cote / 3;
    (0..9u32)
        .map(|i| {
            let (cx, cy) = ((i % 3) * case, (i / 3) * case);
            let mut morceau = Vec::with_capacity((case * case * 4) as usize);
            for y in 0..case {
                let debut = (((cy + y) * cote + cx) * 4) as usize;
                morceau.extend_from_slice(&rgba[debut..debut + (case * 4) as usize]);
            }
            let bgra = crate::win32::epingle::premultiplier_bgra(&morceau);
            a_la_taille(&bgra, case, taille)
        })
        .collect()
}

/// Réduction par moyenne de surface (nette), agrandissement bilinéaire (sans
/// marches d'escalier).
fn a_la_taille(src: &[u8], cote: u32, taille: u32) -> Vec<u8> {
    if taille <= cote {
        return crate::win32::epingle::reechantillonner(src, cote, cote, taille, taille);
    }
    let k = cote as f32 / taille as f32;
    let mut dst = vec![0u8; (taille * taille * 4) as usize];
    for y in 0..taille {
        for x in 0..taille {
            let px = echantillon(src, cote, (x as f32 + 0.5) * k - 0.5, (y as f32 + 0.5) * k - 0.5, true);
            let o = ((y * taille + x) * 4) as usize;
            dst[o..o + 4].copy_from_slice(&px);
        }
    }
    dst
}

/// Lecture bilinéaire d'une image carrée prémultipliée. Hors de l'image :
/// transparent, ou le bord le plus proche si `bord`.
fn echantillon(src: &[u8], cote: u32, u: f32, v: f32, bord: bool) -> [u8; 4] {
    let n = cote as i32;
    let (x0, y0) = (u.floor() as i32, v.floor() as i32);
    let (fx, fy) = (u - x0 as f32, v - y0 as f32);
    let lire = |x: i32, y: i32| -> [f32; 4] {
        let (x, y) = if bord { (x.clamp(0, n - 1), y.clamp(0, n - 1)) } else { (x, y) };
        if x < 0 || y < 0 || x >= n || y >= n {
            return [0.0; 4];
        }
        let i = ((y * n + x) * 4) as usize;
        [src[i] as f32, src[i + 1] as f32, src[i + 2] as f32, src[i + 3] as f32]
    };
    let (a, b, c, d) = (lire(x0, y0), lire(x0 + 1, y0), lire(x0, y0 + 1), lire(x0 + 1, y0 + 1));
    let mut px = [0u8; 4];
    for k in 0..4 {
        let haut = a[k] + (b[k] - a[k]) * fx;
        let bas = c[k] + (d[k] - c[k]) * fx;
        px[k] = (haut + (bas - haut) * fy).round().clamp(0.0, 255.0) as u8;
    }
    px
}

/// L'image de la fenêtre (`cote + 2·marge` de côté) : la case au centre,
/// écrasée de (`sx`, `sy`) autour de son pivot.
fn composer(case: &[u8], cote: u32, (sx, sy): (f32, f32)) -> Vec<u8> {
    let m = marge(cote);
    let fenetre = cote + 2 * m;
    let mut dst = vec![0u8; (fenetre * fenetre * 4) as usize];
    if (sx - 1.0).abs() < 1e-4 && (sy - 1.0).abs() < 1e-4 {
        for y in 0..cote {
            let s = (y * cote * 4) as usize;
            let d = (((y + m) * fenetre + m) * 4) as usize;
            dst[d..d + (cote * 4) as usize].copy_from_slice(&case[s..s + (cote * 4) as usize]);
        }
        return dst;
    }
    let (px, py) = (cote as f32 * 0.5, cote as f32 * PIVOT_Y);
    for y in 0..fenetre {
        for x in 0..fenetre {
            // Le centre du pixel, ramené dans la case non écrasée.
            let u = (x as f32 + 0.5 - m as f32 - px) / sx + px - 0.5;
            let v = (y as f32 + 0.5 - m as f32 - py) / sy + py - 0.5;
            let o = ((y * fenetre + x) * 4) as usize;
            dst[o..o + 4].copy_from_slice(&echantillon(case, cote, u, v, false));
        }
    }
    dst
}

// ---------------------------------------------------------------- la fenêtre

struct Appui {
    curseur: POINT,
    fenetre: POINT,
    glisse: bool,
}

struct Mascotte {
    hwnd: HWND,
    reglages: Reglages,
    /// Côté de la case affichée, en pixels physiques.
    cote: u32,
    directions: Vec<Vec<u8>>,
    reactions: Vec<Vec<u8>>,
    position: POINT,
    secteur: Option<usize>,
    regard: usize,
    reaction: Option<usize>,
    plan: Vec<(Instant, Option<usize>)>,
    boops: Boops,
    ecrase_depuis: Option<Instant>,
    appui: Option<Appui>,
    curseur: POINT,
    immobile_depuis: Instant,
    cachee_plein_ecran: bool,
    dernier_controle: Option<Instant>,
    /// La dernière image peinte : (planche des réactions ?, case, image d'écrasement).
    peinte: Option<(bool, usize, Option<u32>)>,
    rapide: bool,
}

thread_local! {
    static MASCOTTE: RefCell<Option<Mascotte>> = const { RefCell::new(None) };
}

const MINUTERIE: usize = 1;
const RYTHME_REPOS_MS: u32 = 50;
const RYTHME_ANIME_MS: u32 = 16;
/// Le plein écran se vérifie à ce rythme (il lit le premier plan et sa géométrie).
const CONTROLE_PLEIN_ECRAN: Duration = Duration::from_millis(250);

fn avec<R>(f: impl FnOnce(&mut Mascotte) -> R) -> Option<R> {
    // `try_borrow_mut` : jamais `borrow` dans un wndproc (voir la mémoire
    // « RefCell reentrancy »).
    MASCOTTE.with(|m| m.try_borrow_mut().ok().and_then(|mut m| m.as_mut().map(f)))
}

pub fn est_affichee() -> bool {
    MASCOTTE.with(|m| m.try_borrow().map(|m| m.is_some()).unwrap_or(false))
}

/// Au lancement : la remet à l'écran si elle l'était à la fermeture.
pub fn demarrer() {
    if reglages().visible {
        montrer();
    }
}

/// Le clic sur l'applet : l'affiche ou la retire.
pub fn basculer() {
    afficher(!est_affichee());
}

pub fn afficher(oui: bool) {
    let mut r = reglages();
    r.visible = oui;
    enregistrer(&r);
    if oui && !est_affichee() {
        montrer();
    } else if !oui {
        fermer();
    }
}

pub fn choisir_personnage(nom: &str) {
    if !existe(nom) {
        crate::diag!("[mascotte] personnage inconnu : {nom:?}");
        return;
    }
    let mut r = reglages();
    r.personnage = nom.into();
    enregistrer(&r);
    recharger(r);
}

pub fn choisir_taille(taille: u32) {
    let mut r = reglages();
    r.taille = taille.clamp(TAILLE_MIN, TAILLE_MAX);
    enregistrer(&r);
    recharger(r);
}

pub fn choisir_plein_ecran(oui: bool) {
    let mut r = reglages();
    r.plein_ecran = oui;
    enregistrer(&r);
    avec(|m| {
        m.reglages.plein_ecran = oui;
        m.dernier_controle = None;
    });
}

fn fermer() {
    let hwnd = MASCOTTE.with(|m| m.try_borrow().ok().and_then(|m| m.as_ref().map(|m| m.hwnd)));
    if let Some(hwnd) = hwnd {
        // `WM_NCDESTROY` vide l'état.
        let _ = unsafe { DestroyWindow(hwnd) };
    }
}

/// Le côté de la case en pixels physiques pour l'échelle de `hwnd`.
fn cote_physique(hwnd: HWND, taille: u32) -> u32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    (taille * dpi).div_ceil(96).max(16)
}

fn charger(nom: &str, cote: u32) -> Option<(Vec<Vec<u8>>, Vec<Vec<u8>>)> {
    let (d, n) = lire_planche(nom, "directions")?;
    let (r, nr) = lire_planche(nom, "reactions")?;
    Some((decouper(&d, n, cote), decouper(&r, nr, cote)))
}

/// Le coin bas-droit de l'aire de travail de l'écran principal, à une marge.
fn position_par_defaut(fenetre: i32) -> POINT {
    unsafe {
        let ecran = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(ecran, &mut info);
        let t = info.rcWork;
        POINT { x: t.right - fenetre - 48, y: t.bottom - fenetre - 48 }
    }
}

/// La place retenue, si son centre est encore sur un écran (un écran débranché
/// ne doit pas emporter la mascotte hors de vue).
fn position_valide(position: Option<(i32, i32)>, fenetre: i32) -> Option<POINT> {
    let (x, y) = position?;
    let centre = POINT { x: x + fenetre / 2, y: y + fenetre / 2 };
    let ecran = unsafe { MonitorFromPoint(centre, MONITOR_DEFAULTTONULL) };
    (!ecran.is_invalid()).then_some(POINT { x, y })
}

fn creer_fenetre(position: POINT) -> Option<HWND> {
    static CLASSE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).ok()?.into();
        CLASSE.get_or_init(|| {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_HAND).unwrap_or_default(),
                lpszClassName: w!("NeuraDeskMascotte"),
                ..Default::default()
            };
            RegisterClassExW(&wc);
        });
        match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("NeuraDeskMascotte"),
            w!("NeuraDesk — Mascotte"),
            WS_POPUP,
            position.x,
            position.y,
            1,
            1,
            None,
            None,
            Some(instance),
            None,
        ) {
            Ok(hwnd) => Some(hwnd),
            Err(err) => {
                crate::diag!("[mascotte] CreateWindowExW a échoué : {err}");
                None
            }
        }
    }
}

fn montrer() {
    if est_affichee() {
        return;
    }
    let r = reglages();
    // La taille physique dépend de l'écran : on crée la fenêtre à sa place
    // (ou au coin par défaut, estimé à l'échelle 1), puis on la mesure.
    let estimee = (r.taille + 2 * marge(r.taille)) as i32;
    let position = position_valide(r.position, estimee).unwrap_or_else(|| position_par_defaut(estimee));
    let Some(hwnd) = creer_fenetre(position) else { return };
    crate::gfx::capture_visibility::apply_to_window(hwnd);
    let cote = cote_physique(hwnd, r.taille);
    let images = charger(&r.personnage, cote).or_else(|| charger(PAR_DEFAUT, cote));
    let Some((directions, reactions)) = images else {
        crate::diag!("[mascotte] aucune planche lisible : la mascotte reste fermée");
        let _ = unsafe { DestroyWindow(hwnd) };
        return;
    };
    let fenetre = (cote + 2 * marge(cote)) as i32;
    let position = if r.position.is_some() { position } else { position_par_defaut(fenetre) };
    let mut curseur = POINT::default();
    let _ = unsafe { GetCursorPos(&mut curseur) };
    let etat = Mascotte {
        hwnd,
        reglages: r,
        cote,
        directions,
        reactions,
        position,
        secteur: None,
        regard: CENTRE,
        reaction: None,
        plan: Vec::new(),
        boops: Boops::default(),
        ecrase_depuis: None,
        appui: None,
        curseur,
        immobile_depuis: Instant::now(),
        cachee_plein_ecran: false,
        dernier_controle: None,
        peinte: None,
        rapide: false,
    };
    MASCOTTE.with(|m| {
        if let Ok(mut m) = m.try_borrow_mut() {
            *m = Some(etat);
        }
    });
    unsafe { SetTimer(Some(hwnd), MINUTERIE, RYTHME_REPOS_MS, None) };
    tic();
    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    crate::diag!("[mascotte] affichée ({cote} px)");
}

/// Recharge les planches après un changement de personnage ou de taille, en
/// gardant le CENTRE de la mascotte immobile.
fn recharger(r: Reglages) {
    let Some(hwnd) = avec(|m| m.hwnd) else { return };
    let cote = cote_physique(hwnd, r.taille);
    let Some((directions, reactions)) = charger(&r.personnage, cote) else { return };
    avec(|m| {
        let avant = (m.cote + 2 * marge(m.cote)) as i32;
        let apres = (cote + 2 * marge(cote)) as i32;
        m.position.x += (avant - apres) / 2;
        m.position.y += (avant - apres) / 2;
        m.cote = cote;
        m.directions = directions;
        m.reactions = reactions;
        m.reglages.personnage = r.personnage.clone();
        m.reglages.taille = r.taille;
        m.peinte = None;
    });
    tic();
}

/// Un battement : lit le curseur, avance les réactions, vérifie le plein
/// écran, et repeint SI l'image a changé.
fn tic() {
    let maintenant = Instant::now();
    let mut curseur = POINT::default();
    let _ = unsafe { GetCursorPos(&mut curseur) };
    let Some(travail) = avec(|m| avancer(m, maintenant, curseur)) else { return };
    let Travail { hwnd, montrer, cacher, remonter, image, rythme } = travail;
    unsafe {
        if cacher {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        if let Some((position, cote, pixels)) = image {
            poser_image(hwnd, position, cote, &pixels);
        }
        if montrer {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        if remonter {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        if let Some(ms) = rythme {
            SetTimer(Some(hwnd), MINUTERIE, ms, None);
        }
    }
}

struct Travail {
    hwnd: HWND,
    montrer: bool,
    cacher: bool,
    remonter: bool,
    image: Option<(POINT, u32, Vec<u8>)>,
    rythme: Option<u32>,
}

fn avancer(m: &mut Mascotte, maintenant: Instant, curseur: POINT) -> Travail {
    let mut t = Travail { hwnd: m.hwnd, montrer: false, cacher: false, remonter: false, image: None, rythme: None };

    // Le plein écran sur l'écran de la mascotte.
    if m.dernier_controle.is_none_or(|d| maintenant.duration_since(d) >= CONTROLE_PLEIN_ECRAN) {
        m.dernier_controle = Some(maintenant);
        let ecran = unsafe { MonitorFromWindow(m.hwnd, MONITOR_DEFAULTTONEAREST) };
        let plein = crate::win32::window::fullscreen_window_on_monitor(m.hwnd, ecran).is_some();
        let cacher = plein && !m.reglages.plein_ecran;
        if cacher != m.cachee_plein_ecran {
            m.cachee_plein_ecran = cacher;
            t.cacher = cacher;
            t.montrer = !cacher;
            t.remonter = !cacher;
        } else if plein && !cacher {
            // Rester AU-DESSUS d'une application plein écran qui se serait
            // elle-même posée tout en haut.
            t.remonter = true;
        }
    }

    // Le sommeil : la souris immobile depuis longtemps.
    if curseur.x != m.curseur.x || curseur.y != m.curseur.y {
        m.curseur = curseur;
        m.immobile_depuis = maintenant;
    }

    // Les réactions programmées.
    while m.plan.first().is_some_and(|(quand, _)| *quand <= maintenant) {
        m.reaction = m.plan.remove(0).1;
    }

    // Le regard, quand rien d'autre ne l'occupe.
    let fenetre = (m.cote + 2 * marge(m.cote)) as i32;
    let dx = (curseur.x - (m.position.x + fenetre / 2)) as f32;
    let dy = (curseur.y - (m.position.y + fenetre / 2)) as f32;
    let (secteur, regard) = viser(dx, dy, m.cote as f32 * ZONE_MORTE_PART, m.secteur);
    m.secteur = secteur;
    m.regard = regard;

    let glisse = m.appui.as_ref().is_some_and(|a| a.glisse);
    let endormi = maintenant.duration_since(m.immobile_depuis) >= SOMMEIL;
    let (planche_reactions, case) = match (glisse, m.reaction) {
        (true, _) => (true, ETONNE),
        (false, Some(r)) => (true, r),
        (false, None) if endormi => (true, ENDORMI),
        _ => (false, m.regard),
    };
    let phase = m.ecrase_depuis.map(|d| maintenant.duration_since(d)).filter(|d| *d < ECRASEMENT);
    if phase.is_none() {
        m.ecrase_depuis = None;
    }
    let image_ecrasement = phase.map(|d| (d.as_millis() / RYTHME_ANIME_MS as u128) as u32);
    let cle = (planche_reactions, case, image_ecrasement);
    if m.peinte != Some(cle) && !m.cachee_plein_ecran {
        m.peinte = Some(cle);
        let echelle = phase.map_or((1.0, 1.0), |d| ecrasement(d.as_secs_f32() / ECRASEMENT.as_secs_f32()));
        let planche = if planche_reactions { &m.reactions } else { &m.directions };
        t.image = Some((m.position, m.cote + 2 * marge(m.cote), composer(&planche[case], m.cote, echelle)));
    }

    let rapide = phase.is_some() || !m.plan.is_empty();
    if rapide != m.rapide {
        m.rapide = rapide;
        t.rythme = Some(if rapide { RYTHME_ANIME_MS } else { RYTHME_REPOS_MS });
    }
    t
}

/// Peint `pixels` (BGRA prémultiplié, `cote`×`cote`) avec le coin haut-gauche en `position`.
fn poser_image(hwnd: HWND, position: POINT, cote: u32, pixels: &[u8]) {
    let c = cote as i32;
    unsafe {
        let ecran = GetDC(None);
        let memoire = CreateCompatibleDC(Some(ecran));
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: c,
            biHeight: -c,
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
                let taille = SIZE { cx: c, cy: c };
                let origine = POINT::default();
                if let Err(err) = UpdateLayeredWindow(
                    hwnd,
                    Some(ecran),
                    Some(&position),
                    Some(&taille),
                    Some(memoire),
                    Some(&origine),
                    COLORREF(0),
                    Some(&fusion),
                    ULW_ALPHA,
                ) {
                    crate::diag!("[mascotte] UpdateLayeredWindow a échoué : {err}");
                }
                SelectObject(memoire, ancien);
                let _ = DeleteObject(bitmap.into());
            }
            _ => crate::diag!("[mascotte] CreateDIBSection {c}×{c} a échoué"),
        }
        let _ = DeleteDC(memoire);
        ReleaseDC(None, ecran);
    }
}

fn curseur() -> POINT {
    let mut p = POINT::default();
    let _ = unsafe { GetCursorPos(&mut p) };
    p
}

fn appuyer(hwnd: HWND) {
    let c = curseur();
    avec(|m| m.appui = Some(Appui { curseur: c, fenetre: m.position, glisse: false }));
    unsafe { SetCapture(hwnd) };
}

fn bouger(hwnd: HWND) {
    let c = curseur();
    let (seuil_x, seuil_y) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
    let deplacer = avec(|m| {
        let a = m.appui.as_mut()?;
        let (dx, dy) = (c.x - a.curseur.x, c.y - a.curseur.y);
        if !a.glisse && (dx.abs() > seuil_x || dy.abs() > seuil_y) {
            a.glisse = true;
            m.plan.clear();
            m.reaction = None;
        }
        if !a.glisse {
            return None;
        }
        m.position = POINT { x: a.fenetre.x + dx, y: a.fenetre.y + dy };
        Some(m.position)
    })
    .flatten();
    if let Some(p) = deplacer {
        let _ = unsafe { SetWindowPos(hwnd, None, p.x, p.y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
        tic();
    }
}

/// Fin d'un appui : un glissé retient la place, un clic fait réagir.
fn relacher(boop_si_clic: bool) {
    let maintenant = Instant::now();
    let fini = avec(|m| {
        let a = m.appui.take()?;
        if a.glisse {
            let mut r = reglages();
            r.position = Some((m.position.x, m.position.y));
            m.reglages.position = r.position;
            enregistrer(&r);
            crate::diag!("[mascotte] déplacée en ({}, {})", m.position.x, m.position.y);
        } else if boop_si_clic {
            m.plan = boop(&mut m.boops, maintenant).into_iter().map(|(d, r)| (maintenant + d, r)).collect();
            m.ecrase_depuis = Some(maintenant);
            m.immobile_depuis = maintenant;
        }
        Some(())
    });
    if fini.flatten().is_some() {
        tic();
    }
}

fn molette(vers_le_haut: bool) {
    let mut r = reglages();
    let t = if vers_le_haut { r.taille + PAS_MOLETTE } else { r.taille.saturating_sub(PAS_MOLETTE) };
    r.taille = t.clamp(TAILLE_MIN, TAILLE_MAX);
    enregistrer(&r);
    recharger(r);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_TIMER if wparam.0 == MINUTERIE => {
            tic();
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            appuyer(hwnd);
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            bouger(hwnd);
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            // `ReleaseCapture` envoie `WM_CAPTURECHANGED` : l'appui est déjà
            // consommé quand il arrive, il ne fait donc rien.
            relacher(true);
            let _ = unsafe { ReleaseCapture() };
            return LRESULT(0);
        }
        // Capture perdue en plein glissé (Alt+Tab…) : la place est retenue,
        // pas de réaction.
        WM_CAPTURECHANGED => relacher(false),
        WM_MOUSEWHEEL => {
            molette(((wparam.0 >> 16) as u16 as i16) > 0);
            return LRESULT(0);
        }
        // Passée sur un écran d'une autre échelle : même taille LOGIQUE.
        WM_DPICHANGED => {
            recharger(reglages());
            return LRESULT(0);
        }
        WM_NCDESTROY => {
            let _ = unsafe { KillTimer(Some(hwnd), MINUTERIE) };
            let videe = MASCOTTE.with(|m| m.try_borrow_mut().map(|mut m| *m = None).is_ok());
            if !videe {
                crate::diag!("[mascotte] détruite pendant un emprunt : son état reste en mémoire");
            }
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Le rectangle de la mascotte à l'écran (pour le harnais de capture).
#[allow(dead_code)]
pub fn rectangle() -> Option<RECT> {
    avec(|m| {
        let c = (m.cote + 2 * marge(m.cote)) as i32;
        RECT { left: m.position.x, top: m.position.y, right: m.position.x + c, bottom: m.position.y + c }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cinquante_deux_personnages_et_cinq_styles_tous_distincts() {
        let styles = PERSONNAGES.iter().filter(|p| p.famille == "Styles du renard").count();
        assert_eq!(styles, 5);
        assert_eq!(PERSONNAGES.len() - styles, 52);
        let mut noms: Vec<_> = PERSONNAGES.iter().map(|p| p.nom).collect();
        noms.sort_unstable();
        noms.dedup();
        assert_eq!(noms.len(), PERSONNAGES.len(), "un nom en double");
        assert!(existe(PAR_DEFAUT));
    }

    /// Chaque personnage a ses DEUX planches dans le dépôt, 3×3 carrées, et
    /// elles se décodent (le module `webp` d'`image` est bien activé).
    #[test]
    fn chaque_personnage_a_ses_deux_planches_decodables() {
        let dossier = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/popups/mascottes");
        for p in PERSONNAGES {
            for sorte in ["directions", "reactions"] {
                let chemin = dossier.join(format!("{}-{sorte}.webp", p.nom));
                let octets = std::fs::read(&chemin).unwrap_or_else(|e| panic!("{} : {e}", chemin.display()));
                let image = image::load_from_memory_with_format(&octets, image::ImageFormat::WebP)
                    .unwrap_or_else(|e| panic!("{} : {e}", chemin.display()));
                assert_eq!(image.width(), image.height(), "{}", chemin.display());
                assert_eq!(image.width() % 3, 0, "{}", chemin.display());
            }
        }
    }

    #[test]
    fn les_reglages_font_l_aller_retour_et_bornent_la_taille() {
        let r = Reglages { personnage: "owl".into(), taille: 200, plein_ecran: true, visible: true, position: Some((-1200, 340)) };
        assert_eq!(lire_reglages(&ecrire_reglages(&r)), r);
        let lu = lire_reglages("personnage=inconnu\ntaille=9999\nx=5\n");
        assert_eq!(lu.personnage, PAR_DEFAUT, "un nom inconnu garde le défaut");
        assert_eq!(lu.taille, TAILLE_MAX);
        assert_eq!(lu.position, None, "x sans y ne fait pas une place");
        assert_eq!(lire_reglages(""), Reglages::default());
    }

    #[test]
    fn le_regard_suit_les_huit_secteurs_et_la_zone_morte() {
        assert_eq!(viser(10.0, 0.0, 70.0, None), (None, CENTRE));
        assert_eq!(viser(200.0, 0.0, 70.0, None).1, 5, "droite");
        assert_eq!(viser(0.0, 200.0, 70.0, None).1, 7, "bas (y vers le bas)");
        assert_eq!(viser(-200.0, 0.0, 70.0, None).1, 3, "gauche");
        assert_eq!(viser(0.0, -200.0, 70.0, None).1, 1, "haut");
        assert_eq!(viser(-150.0, -150.0, 70.0, None).1, 0, "haut-gauche");
        assert_eq!(viser(150.0, 150.0, 70.0, None).1, 8, "bas-droite");
    }

    /// Juste au-delà du bord d'un secteur, le secteur courant TIENT ; bien
    /// au-delà de l'hystérésis, il change.
    #[test]
    fn le_secteur_courant_tient_dans_l_hysteresis() {
        let bord = SECTEUR / 2.0 + 0.05;
        let (dx, dy) = (200.0 * bord.cos(), 200.0 * bord.sin());
        assert_eq!(viser(dx, dy, 70.0, Some(0)), (Some(0), 5), "reste à droite");
        assert_eq!(viser(dx, dy, 70.0, None).0, Some(1), "sans secteur : bas-droite");
        let loin = SECTEUR / 2.0 + HYSTERESIS + 0.05;
        assert_eq!(viser(200.0 * loin.cos(), 200.0 * loin.sin(), 70.0, Some(0)).0, Some(1));
    }

    #[test]
    fn un_clic_cligne_puis_recompense_et_quatre_clics_rapides_etourdissent() {
        let t0 = Instant::now();
        let mut b = Boops::default();
        let premier = boop(&mut b, t0);
        assert_eq!(premier, vec![(Duration::ZERO, Some(CLIGNE)), (BOOP_RECOMPENSE, Some(1)), (BOOP_FIN, None)]);
        assert_eq!(boop(&mut b, t0 + Duration::from_millis(300))[1].1, Some(2), "étincelles");
        assert_eq!(boop(&mut b, t0 + Duration::from_millis(600))[1].1, Some(8), "ravi");
        let quatrieme = boop(&mut b, t0 + Duration::from_millis(900));
        assert_eq!(quatrieme, vec![(Duration::ZERO, Some(ETOURDI)), (ETOURDI_FIN, None)]);
        // Après une pause, on recommence au cœur.
        assert_eq!(boop(&mut b, t0 + Duration::from_secs(10))[1].1, Some(1));
    }

    #[test]
    fn l_ecrasement_passe_par_ses_images_cles_et_revient_a_un() {
        let proche = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3;
        assert!(proche(ecrasement(0.0), (1.0, 1.0)));
        assert!(proche(ecrasement(0.18), (1.10, 0.86)));
        assert!(proche(ecrasement(0.45), (0.95, 1.08)));
        assert!(proche(ecrasement(0.72), (1.03, 0.97)));
        assert!(proche(ecrasement(1.0), (1.0, 1.0)));
        for i in 0..=100 {
            let (sx, sy) = ecrasement(i as f32 / 100.0);
            assert!((0.94..=1.11).contains(&sx) && (0.85..=1.09).contains(&sy), "{i} : {sx} {sy}");
        }
    }

    /// La marge suffit : à l'écrasement le plus large et le plus haut, rien ne
    /// déborde de la fenêtre (la case est pleine, ses bords doivent tenir).
    #[test]
    fn l_ecrasement_ne_deborde_jamais_de_la_fenetre() {
        let cote = 100;
        let case = vec![255u8; (cote * cote * 4) as usize];
        for echelle in [(1.10, 0.86), (0.95, 1.08)] {
            let image = composer(&case, cote, echelle);
            let f = cote + 2 * marge(cote);
            for i in 0..f {
                for (x, y) in [(i, 0), (i, f - 1), (0, i), (f - 1, i)] {
                    assert_eq!(image[((y * f + x) * 4 + 3) as usize], 0, "bord ({x}, {y}) peint pour {echelle:?}");
                }
            }
        }
    }

    #[test]
    fn sans_ecrasement_la_case_est_posee_telle_quelle_au_centre() {
        let cote = 4;
        let case: Vec<u8> = (0..cote * cote * 4).map(|i| i as u8).collect();
        let image = composer(&case, cote, (1.0, 1.0));
        let m = marge(cote);
        let f = cote + 2 * m;
        let o = ((m * f + m) * 4) as usize;
        assert_eq!(&image[o..o + 16], &case[..16]);
        assert_eq!(image[0..4], [0, 0, 0, 0]);
    }

    /// **SONDE** (à la main) : rend quelques images de la mascotte par le MÊME
    /// chemin que la fenêtre (découpe, mise à la taille, écrasement) et les
    /// pose côte à côte sur un fond sombre dans `$NEURADESK_SONDE_MASCOTTE`.
    #[test]
    #[ignore = "sonde a la main — ecrit une image"]
    fn sonde_des_images_de_la_mascotte() {
        let sortie = std::env::var("NEURADESK_SONDE_MASCOTTE").expect("NEURADESK_SONDE_MASCOTTE");
        let dossier = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/popups/mascottes");
        let planche = |nom: &str, sorte: &str| {
            let octets = std::fs::read(dossier.join(format!("{nom}-{sorte}.webp"))).unwrap();
            let image = image::load_from_memory(&octets).unwrap().to_rgba8();
            let cote = image.width();
            (image.into_raw(), cote)
        };
        let cote = 210;
        let mut images = Vec::new();
        for nom in ["fox", "owl", "robot-absent"].into_iter().filter(|n| existe(n)) {
            let (d, n) = planche(nom, "directions");
            let (r, nr) = planche(nom, "reactions");
            let (d, r) = (decouper(&d, n, cote), decouper(&r, nr, cote));
            images.push(composer(&d[CENTRE], cote, (1.0, 1.0)));
            images.push(composer(&d[5], cote, (1.0, 1.0)));
            images.push(composer(&d[0], cote, (1.0, 1.0)));
            images.push(composer(&r[1], cote, ecrasement(0.18)));
            images.push(composer(&r[ETOURDI], cote, ecrasement(0.45)));
        }
        let f = cote + 2 * marge(cote);
        let mut toile = image::RgbaImage::from_pixel(f * 5, f * (images.len() as u32 / 5), image::Rgba([40, 44, 60, 255]));
        for (k, img) in images.iter().enumerate() {
            let (ox, oy) = ((k as u32 % 5) * f, (k as u32 / 5) * f);
            for y in 0..f {
                for x in 0..f {
                    let i = ((y * f + x) * 4) as usize;
                    let a = img[i + 3] as u32;
                    let fond = toile.get_pixel(ox + x, oy + y).0;
                    // BGRA prémultiplié par-dessus le fond.
                    let c = |v: u8, b: u8| (v as u32 + b as u32 * (255 - a) / 255).min(255) as u8;
                    toile.put_pixel(ox + x, oy + y, image::Rgba([c(img[i + 2], fond[0]), c(img[i + 1], fond[1]), c(img[i], fond[2]), 255]));
                }
            }
        }
        toile.save(&sortie).unwrap();
    }

    #[test]
    fn la_planche_se_decoupe_ligne_par_ligne() {
        // Une planche 6×6 : chaque case 2×2 porte son numéro dans le rouge.
        let mut rgba = vec![0u8; 6 * 6 * 4];
        for y in 0..6 {
            for x in 0..6 {
                let i = (y * 6 + x) * 4;
                rgba[i] = ((y / 2) * 3 + x / 2) as u8 * 10;
                rgba[i + 3] = 255;
            }
        }
        let cases = decouper(&rgba, 6, 2);
        for (n, case) in cases.iter().enumerate() {
            // BGRA : le rouge est le troisième octet.
            assert_eq!(case[2], n as u8 * 10, "case {n}");
        }
    }
}
