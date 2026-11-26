//! **LES INSECTES** — DEMANDE EXPLICITE (2026-11-24) : « ajouter dans les
//! mascottes une mouche, un moustique et un papillon qui volent et se posent
//! aléatoirement à l'écran et qui s'envolent quand le curseur de souris les
//! frôle, en ajoutant le son de la mouche ou du moustique quand ils volent ;
//! il faudrait pouvoir les activer individuellement ». Réglés dans la section
//! « Mascotte » des Réglages.
//!
//! - **Une fenêtre par insecte**, comme les super-héros : `WS_EX_LAYERED |
//!   WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`, à la taille de la bête, montrée par
//!   `SW_SHOWNOACTIVATE`. Le clic passe au travers : « frôler », c'est le
//!   pointeur (`GetCursorPos`) qui passe près, rien de plus.
//! - **Le cycle** : absent → entre par un bord → vole (en zigzag pour la
//!   mouche) → se pose au hasard dans la zone de travail d'un écran, loin du
//!   pointeur → repart après un moment, vers un autre point ou hors de l'écran
//!   pour revenir plus tard. Le pointeur trop près : il s'envole aussitôt.
//! - **Le dessin** (`dessin.rs`) : des photos réalistes vues de dessus,
//!   tournées selon le cap, refaites à chaque image en vol ;
//!   posé, seulement quand il change (la mouche trottine, le papillon ouvre et
//!   ferme ses ailes).
//! - **Le son** (`bourdon.rs`) : un bourdonnement synthétisé, un canal par
//!   insecte, joué seulement en vol. Le papillon est muet.
//! - **Jamais devant un film** : une application plein écran sur son écran,
//!   il s'en va (`window::fullscreen_window_on_monitor`).
//! - **Le coût** : une minuterie à 16 ms tant qu'un insecte vole, 40 ms s'ils
//!   sont posés, 500 ms s'ils sont tous partis, arrêtée si aucun n'est activé.

mod bourdon;
mod dessin;
mod vol;

use std::cell::RefCell;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use bourdon::Bourdon;
use dessin::Moment;
#[cfg(test)]
use dessin::Photos;
use vol::Zone;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, MonitorFromPoint, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, DIB_RGB_COLORS, HMONITOR,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, KillTimer, RegisterClassExW, SetTimer, SetWindowPos, ShowWindow,
    UpdateLayeredWindow, HTTRANSPARENT, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE,
    SW_SHOWNOACTIVATE, ULW_ALPHA, WM_NCHITTEST, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Espece {
    /// Une des `PAR_FAMILLE` mouches — DEMANDE EXPLICITE (2026-11-24) :
    /// « jusqu'à 5 mouches et 5 moustiques ». Domestique, verte, bleue, à
    /// damier, syrphe.
    Mouche(u8),
    /// Commun, tigre, anophèle, annelé, Aedes vexans.
    Moustique(u8),
    /// Un des papillons, chacun activé à part — DEMANDE EXPLICITE
    /// (2026-11-24) : « activer jusqu'à 5 papillons différents ». Paon du
    /// jour, vulcain, machaon, citron, azuré.
    Papillon(u8),
}

/// Le nombre d'insectes de chaque famille (mouches, moustiques, papillons).
pub const PAR_FAMILLE: usize = 5;
pub const NB_ESPECES: usize = 3 * PAR_FAMILLE;

pub const ESPECES: [Espece; NB_ESPECES] = [
    Espece::Mouche(0),
    Espece::Mouche(1),
    Espece::Mouche(2),
    Espece::Mouche(3),
    Espece::Mouche(4),
    Espece::Moustique(0),
    Espece::Moustique(1),
    Espece::Moustique(2),
    Espece::Moustique(3),
    Espece::Moustique(4),
    Espece::Papillon(0),
    Espece::Papillon(1),
    Espece::Papillon(2),
    Espece::Papillon(3),
    Espece::Papillon(4),
];

/// Les noms, dans l'ordre de `ESPECES` : ceux des photos
/// `insecte-<nom>[-pose|-vol].webp`, des réglages et des verbes de la page.
const NOMS: [&str; NB_ESPECES] = [
    "mouche-1",
    "mouche-2",
    "mouche-3",
    "mouche-4",
    "mouche-5",
    "moustique-1",
    "moustique-2",
    "moustique-3",
    "moustique-4",
    "moustique-5",
    "papillon-1",
    "papillon-2",
    "papillon-3",
    "papillon-4",
    "papillon-5",
];

impl Espece {
    pub fn nom(self) -> &'static str {
        NOMS[self.rang()]
    }

    /// Sa place dans `ESPECES` (et dans les tableaux de `Reglages`).
    pub fn rang(self) -> usize {
        let (famille, n) = match self {
            Espece::Mouche(n) => (0, n),
            Espece::Moustique(n) => (1, n),
            Espece::Papillon(n) => (2, n),
        };
        famille * PAR_FAMILLE + n as usize % PAR_FAMILLE
    }

    pub fn est_papillon(self) -> bool {
        matches!(self, Espece::Papillon(_))
    }

    pub fn est_mouche(self) -> bool {
        matches!(self, Espece::Mouche(_))
    }

    /// Relit un nom ; les noms d'avant les familles (`mouche`, `moustique`,
    /// `papillon` : un seul de chaque) désignent le premier.
    pub fn lire(nom: &str) -> Option<Espece> {
        match nom {
            "mouche" => Some(Espece::Mouche(0)),
            "moustique" => Some(Espece::Moustique(0)),
            "papillon" => Some(Espece::Papillon(0)),
            _ => ESPECES.into_iter().find(|e| e.nom() == nom),
        }
    }

    fn bourdonne(self) -> bool {
        !self.est_papillon()
    }
}

const MINUTERIE: usize = 1;
const VOL_MS: u32 = 16;
const POSE_MS: u32 = 40;
const ABSENT_MS: u32 = 500;
/// ⭐ LE BATTEMENT DE LA MOUCHE ET DU MOUSTIQUE, EN TOURS PAR IMAGE PEINTE —
/// DEMANDE EXPLICITE (2026-11-25) : « il faut faire bouger les ailes plus
/// vite ». La minuterie volante bat à ~62 images par seconde, et une aile qui
/// ferait PLUS d'un demi-tour entre deux images ne paraîtrait pas plus rapide :
/// elle filerait à l'envers ou au hasard (effet de roue de chariot). À 0,47,
/// elle est juste sous cette limite quel que soit le nombre d'insectes : c'est
/// le plus vite que l'écran peut montrer, et l'écart est le même à chaque
/// image — donc pas de ralentissement quand l'appli est chargée. Le papillon,
/// lui, garde `Allure::battement_vol` (par seconde), lent et lisible.
const BATTEMENT_PAR_IMAGE: f32 = 0.47;
/// Le plus long vol avant de changer d'idée (il tournait autour d'un point
/// inaccessible, ou l'écran a changé).
const VOL_MAX: Duration = Duration::from_secs(25);
/// Une chance sur cinq de quitter l'écran après une pose.
const CHANCE_DE_PARTIR: f32 = 0.2;
/// Le temps passé hors de l'écran avant de revenir, en secondes.
const ABSENCE: (f32, f32) = (15.0, 60.0);
/// Le plein écran est vérifié une fois par seconde.
const SONDE_PLEIN_ECRAN: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Eteint,
    Absent { retour: Instant },
    Vol { sortir: bool },
    Pose { fin: Instant },
}

/// Un petit pas de la mouche posée.
#[derive(Clone, Copy, Debug)]
struct Pas {
    de: (f32, f32),
    vers: (f32, f32),
    debut: Instant,
    duree: f32,
}

struct Insecte {
    espece: Espece,
    hwnd: HWND,
    phase: Phase,
    pos: (f32, f32),
    vit: (f32, f32),
    cap: f32,
    cible: (f32, f32),
    /// Pixels par pixel logique sur l'écran où il est, taille réglée comprise :
    /// sa vitesse, ses pas et la distance où il fuit le pointeur suivent.
    echelle: f32,
    /// La taille réglée, en fraction (1 = normale).
    taille: f32,
    cote: u32,
    moniteur: HMONITOR,
    ecran: Zone,
    travail: Zone,
    deviation: f32,
    prochain_virage: Instant,
    fuite_jusqu_a: Instant,
    debut_vol: Instant,
    battement: f32,
    pas: Option<Pas>,
    prochain_pas: Instant,
    bourdon: Option<Bourdon>,
    /// La clé de la dernière image peinte (rien de peint : caché).
    peinte: Option<(i32, i32, bool, u32)>,
}

struct Etat {
    insectes: Vec<Insecte>,
    son: bool,
    minuterie: u32,
    dernier_tic: Instant,
}

/// ⛔ **UN BOURDON NE SE REFERME JAMAIS DANS LA DESTRUCTION DU `thread_local`**
/// — MESURÉ le 2026-11-24 : dix insectes bourdonnants, l'appli fermée restait
/// « Ne répond pas », UN fil, 0 % de processeur. Le `thread_local` se détruit à
/// la sortie du fil, sous le verrou du chargeur de Windows, et
/// `waveOutReset`/`waveOutClose` y attendent le fil audio de winmm : interblocage.
/// `arreter` (appelée à la fin de `app::run`) les referme AVANT ; ceux qui
/// resteraient (sortie par panique) sont abandonnés au système, qui libère tout
/// à la fin du processus.
impl Drop for Etat {
    fn drop(&mut self) {
        for i in &mut self.insectes {
            if let Some(b) = i.bourdon.take() {
                std::mem::forget(b);
            }
        }
    }
}

thread_local! {
    static ETAT: RefCell<Option<Etat>> = const { RefCell::new(None) };
}

/// À la fermeture de l'appli : referme les bourdons pendant que le fil vit
/// encore (voir `impl Drop for Etat`).
pub fn arreter() {
    avec(|e| {
        for i in &mut e.insectes {
            i.bourdon = None;
        }
    });
}

fn avec<R>(f: impl FnOnce(&mut Etat) -> R) -> Option<R> {
    ETAT.with(|e| e.try_borrow_mut().ok().and_then(|mut e| e.as_mut().map(f)))
}

fn hasard() -> f32 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static GRAINE: AtomicU64 = AtomicU64::new(0);
    let mut x = GRAINE.load(Ordering::Relaxed);
    if x == 0 {
        x = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1)
            | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    GRAINE.store(x, Ordering::Relaxed);
    (x >> 40) as f32 / (1u64 << 24) as f32
}

fn entre((a, b): (f32, f32)) -> f32 {
    a + hasard() * (b - a)
}

fn apres(secondes: f32) -> Instant {
    Instant::now() + Duration::from_secs_f32(secondes.max(0.0))
}

fn curseur() -> (f32, f32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x as f32, p.y as f32)
}

fn zone(r: windows::Win32::Foundation::RECT) -> Zone {
    Zone { gauche: r.left as f32, haut: r.top as f32, droite: r.right as f32, bas: r.bottom as f32 }
}

// ---------------------------------------------------------------- l'interface

/// Au lancement : les fenêtres (cachées), et ceux qui sont activés partent.
pub fn demarrer() {
    let r = crate::win32::mascotte::reglages();
    let maintenant = Instant::now();
    let mut insectes = Vec::new();
    for espece in ESPECES {
        let Some(hwnd) = creer_fenetre() else { return };
        crate::gfx::capture_visibility::apply_to_window(hwnd);
        let actif = r.insecte(espece);
        let taille = r.insecte_taille(espece) as f32 / 100.0;
        insectes.push(Insecte {
            espece,
            hwnd,
            phase: if actif { Phase::Absent { retour: apres(entre((1.0, 4.0))) } } else { Phase::Eteint },
            pos: (0.0, 0.0),
            vit: (0.0, 0.0),
            cap: 0.0,
            cible: (0.0, 0.0),
            echelle: 1.0,
            taille,
            cote: 1,
            moniteur: HMONITOR::default(),
            ecran: Zone { gauche: 0.0, haut: 0.0, droite: 1.0, bas: 1.0 },
            travail: Zone { gauche: 0.0, haut: 0.0, droite: 1.0, bas: 1.0 },
            deviation: 0.0,
            prochain_virage: maintenant,
            fuite_jusqu_a: maintenant,
            debut_vol: maintenant,
            battement: 0.0,
            pas: None,
            prochain_pas: maintenant,
            bourdon: None,
            peinte: None,
        });
    }
    ETAT.with(|e| {
        if let Ok(mut e) = e.try_borrow_mut() {
            *e = Some(Etat { insectes, son: r.insectes_son, minuterie: 0, dernier_tic: maintenant });
        }
    });
    regler_minuterie();
}

/// Active ou retire une espèce.
pub fn choisir(espece: Espece, oui: bool) {
    let mut r = crate::win32::mascotte::reglages();
    r.choisir_insecte(espece, oui);
    crate::win32::mascotte::enregistrer(&r);
    let cacher = avec(|e| {
        let i = e.insectes.iter_mut().find(|i| i.espece == espece)?;
        match (oui, i.phase) {
            (true, Phase::Eteint) => {
                i.phase = Phase::Absent { retour: apres(0.5) };
                None
            }
            (false, Phase::Eteint) | (true, _) => None,
            (false, _) => {
                i.phase = Phase::Eteint;
                i.bourdon = None;
                i.peinte = None;
                Some(i.hwnd)
            }
        }
    })
    .flatten();
    if let Some(hwnd) = cacher {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
    crate::diag!("[insectes] {} : {}", espece.nom(), if oui { "activé" } else { "retiré" });
    regler_minuterie();
}

/// La taille d'une espèce, en pour cent — DEMANDE EXPLICITE (2026-11-24) :
/// « agrandir les insectes individuellement ». Appliquée tout de suite.
pub fn choisir_taille(espece: Espece, pour_cent: u32) {
    let mut r = crate::win32::mascotte::reglages();
    r.choisir_insecte_taille(espece, pour_cent);
    let taille = r.insecte_taille(espece) as f32 / 100.0;
    crate::win32::mascotte::enregistrer(&r);
    avec(|e| {
        let Some(i) = e.insectes.iter_mut().find(|i| i.espece == espece) else { return };
        i.taille = taille;
        if !matches!(i.phase, Phase::Eteint | Phase::Absent { .. }) {
            adopter_ecran(i, i.pos);
            i.peinte = None;
            // Posé, il n'est repeint qu'à son prochain changement : tout de suite.
            if let Travail::Peindre { hwnd, coin, cote, pixels, .. } = peindre(i) {
                poser_image(hwnd, coin, cote, &pixels);
            }
        }
    });
}

/// Le bourdonnement en vol, pour la mouche et le moustique.
pub fn choisir_son(oui: bool) {
    let mut r = crate::win32::mascotte::reglages();
    r.insectes_son = oui;
    crate::win32::mascotte::enregistrer(&r);
    avec(|e| {
        e.son = oui;
        for i in &mut e.insectes {
            i.bourdon = None;
            if oui && matches!(i.phase, Phase::Vol { .. }) && i.espece.bourdonne() {
                i.bourdon = Bourdon::jouer(i.espece);
            }
        }
    });
}

/// La cadence de la minuterie, d'après ce que font les insectes.
fn regler_minuterie() {
    let Some((hwnd, voulu, actuel)) = avec(|e| {
        let voulu = e.insectes.iter().fold(0u32, |m, i| {
            let ms = match i.phase {
                Phase::Eteint => 0,
                Phase::Absent { .. } => ABSENT_MS,
                Phase::Pose { .. } => POSE_MS,
                Phase::Vol { .. } => VOL_MS,
            };
            if m == 0 || (ms != 0 && ms < m) {
                ms
            } else {
                m
            }
        });
        (e.insectes.first().map(|i| i.hwnd), voulu, e.minuterie)
    }) else {
        return;
    };
    let Some(hwnd) = hwnd else { return };
    if voulu == actuel {
        return;
    }
    unsafe {
        if voulu == 0 {
            let _ = KillTimer(Some(hwnd), MINUTERIE);
        } else {
            SetTimer(Some(hwnd), MINUTERIE, voulu, None);
        }
    }
    avec(|e| e.minuterie = voulu);
}

// ---------------------------------------------------------------- le comportement

/// Ce que la fenêtre d'un insecte doit faire, hors de l'emprunt de l'état.
enum Travail {
    Rien,
    Cacher(HWND),
    Deplacer(HWND, POINT),
    Peindre { hwnd: HWND, coin: POINT, cote: u32, pixels: Vec<u8>, montrer: bool },
}

fn tic() {
    let souris = curseur();
    let travaux = avec(|e| {
        let maintenant = Instant::now();
        let dt = maintenant.duration_since(e.dernier_tic).as_secs_f32().min(0.05);
        e.dernier_tic = maintenant;
        let son = e.son;
        e.insectes.iter_mut().map(|i| avancer(i, maintenant, dt, souris, son)).collect::<Vec<_>>()
    })
    .unwrap_or_default();
    for t in travaux {
        match t {
            Travail::Rien => {}
            Travail::Cacher(hwnd) => unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            },
            Travail::Deplacer(hwnd, coin) => unsafe {
                let _ = SetWindowPos(hwnd, None, coin.x, coin.y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER);
            },
            Travail::Peindre { hwnd, coin, cote, pixels, montrer } => {
                poser_image(hwnd, coin, cote, &pixels);
                if montrer {
                    unsafe {
                        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE);
                    }
                }
            }
        }
    }
    regler_minuterie();
}

/// L'écran où se trouve `pos` : `(moniteur, écran, zone de travail, échelle)`.
fn ecran_de(pos: (f32, f32)) -> (HMONITOR, Zone, Zone, f32) {
    let p = POINT { x: pos.0 as i32, y: pos.1 as i32 };
    let moniteur = unsafe { MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST) };
    let ecrans = crate::win32::monitors::enumerate();
    let (ecran, travail) = ecrans
        .iter()
        .find(|m| m.handle == moniteur)
        .map(|m| (zone(m.rect), zone(m.work_rect)))
        .unwrap_or((Zone { gauche: 0.0, haut: 0.0, droite: 1920.0, bas: 1080.0 }, Zone { gauche: 0.0, haut: 0.0, droite: 1920.0, bas: 1040.0 }));
    let echelle = crate::win32::dpi::get_dpi_for_monitor(moniteur) as f32 / 96.0;
    (moniteur, ecran, travail, echelle.max(0.5))
}

/// Les écrans où une fenêtre est en plein écran, relus au plus toutes les
/// `SONDE_PLEIN_ECRAN` (une énumération des fenêtres pour tous les insectes).
fn ecrans_occupes() -> Vec<HMONITOR> {
    thread_local! {
        static CACHE: RefCell<Option<(Instant, Vec<HMONITOR>)>> = const { RefCell::new(None) };
    }
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let maintenant = Instant::now();
        match c.as_ref() {
            Some((quand, v)) if maintenant.duration_since(*quand) < SONDE_PLEIN_ECRAN => v.clone(),
            _ => {
                let v = crate::win32::window::ecrans_en_plein_ecran();
                *c = Some((maintenant, v.clone()));
                v
            }
        }
    })
}

fn moniteur_de(pos: (f32, f32)) -> HMONITOR {
    unsafe { MonitorFromPoint(POINT { x: pos.0 as i32, y: pos.1 as i32 }, MONITOR_DEFAULTTONEAREST) }
}

/// Une fenêtre plein écran occupe l'écran de ce point.
fn occupe(pos: (f32, f32)) -> bool {
    ecrans_occupes().contains(&moniteur_de(pos))
}

/// Un point de pose sur un écran SANS plein écran : le sien si possible,
/// parfois (`ailleurs`) un autre ; `None` si tous sont occupés.
fn pose_sur_ecran_libre(i: &Insecte, souris: (f32, f32), ailleurs: bool) -> Option<(f32, f32)> {
    let occupes = ecrans_occupes();
    let ecrans = crate::win32::monitors::enumerate();
    let libres: Vec<_> = ecrans.iter().filter(|m| !occupes.contains(&m.handle)).collect();
    let le_sien = libres.iter().any(|m| m.handle == i.moniteur);
    if le_sien && !(ailleurs && libres.len() > 1) {
        return Some(nouvelle_pose(i, souris));
    }
    let m = libres.get((hasard() * libres.len() as f32) as usize % libres.len().max(1))?;
    let a = vol::allure(i.espece);
    Some(vol::point_de_pose(zone(m.work_rect).retrecie(i.cote as f32), souris, a.rayon_fuite * i.echelle * 3.0, &mut hasard))
}

/// Se place sur l'écran `pos` : échelle, taille de la fenêtre, zones.
fn adopter_ecran(i: &mut Insecte, pos: (f32, f32)) {
    let (moniteur, ecran, travail, echelle) = ecran_de(pos);
    i.moniteur = moniteur;
    i.ecran = ecran;
    i.travail = travail;
    i.echelle = echelle * i.taille;
    i.cote = cote_fenetre(i.espece, i.echelle);
}

/// Le côté de la fenêtre pour une échelle donnée — ⛔ l'échelle COMPLÈTE
/// (écran × taille réglée) : calculée sans la taille, la fenêtre restait à
/// 100 % et coupait les papillons agrandis (signalé le 2026-11-24).
fn cote_fenetre(espece: Espece, echelle: f32) -> u32 {
    (dessin::cote_logique(espece) * echelle).ceil() as u32
}

fn decoller(i: &mut Insecte, maintenant: Instant, cible: (f32, f32), sortir: bool, son: bool) {
    i.phase = Phase::Vol { sortir };
    i.cible = cible;
    i.debut_vol = maintenant;
    i.pas = None;
    if son && i.espece.bourdonne() && i.bourdon.is_none() {
        i.bourdon = Bourdon::jouer(i.espece);
    }
}

fn nouvelle_pose(i: &Insecte, souris: (f32, f32)) -> (f32, f32) {
    let a = vol::allure(i.espece);
    let marge = i.cote as f32;
    vol::point_de_pose(i.travail.retrecie(marge), souris, a.rayon_fuite * i.echelle * 3.0, &mut hasard)
}

/// Le pas du battement pour un tic : un tour par IMAGE pour la mouche et le
/// moustique (le plus vite que l'écran peut montrer sans roue de chariot), la
/// cadence en secondes pour le papillon.
fn pas_du_battement(espece: Espece, allure: &vol::Allure, dt: f32) -> f32 {
    if espece.est_papillon() {
        allure.battement_vol * dt
    } else {
        BATTEMENT_PAR_IMAGE
    }
}

fn avancer(i: &mut Insecte, maintenant: Instant, dt: f32, souris: (f32, f32), son: bool) -> Travail {
    let a = vol::allure(i.espece);
    match i.phase {
        Phase::Eteint => Travail::Rien,
        Phase::Absent { retour } => {
            if maintenant < retour {
                return Travail::Rien;
            }
            entrer(i, maintenant, souris, son);
            Travail::Rien
        }
        Phase::Pose { fin } => {
            let rayon = a.rayon_fuite * i.echelle;
            if vol::frole(i.pos, souris, rayon) {
                // Frôlée : elle file, à l'opposé du pointeur.
                let cible = vol::point_de_fuite(i.pos, souris, i.travail.retrecie(i.cote as f32), rayon * 6.0);
                let (fx, fy) = vol::direction_de_fuite(i.pos, souris);
                i.vit = (fx * a.vitesse_fuite * i.echelle * 0.6, fy * a.vitesse_fuite * i.echelle * 0.6);
                i.fuite_jusqu_a = maintenant + Duration::from_millis(900);
                decoller(i, maintenant, cible, false, son);
            } else if maintenant >= fin || occupe(i.pos) {
                // Un plein écran sous lui : il s'en va aussitôt, vers un écran
                // libre ou hors de l'écran ; parfois, il change d'écran.
                let pose = if hasard() < CHANCE_DE_PARTIR { None } else { pose_sur_ecran_libre(i, souris, hasard() < 0.25) };
                let sortir = pose.is_none();
                let cible = pose.unwrap_or_else(|| vol::point_de_sortie(i.pos, i.ecran, i.cote as f32 * 2.0));
                decoller(i, maintenant, cible, sortir, son);
            } else {
                trottiner(i, maintenant, &a);
                i.battement = (i.battement + a.battement_pose * dt).fract();
            }
            peindre(i)
        }
        Phase::Vol { sortir } => voler(i, maintenant, dt, souris, sortir, &a),
    }
}

/// Absent depuis assez longtemps : entre par un bord d'un écran libre.
fn entrer(i: &mut Insecte, maintenant: Instant, souris: (f32, f32), son: bool) {
    // Sans ses photos (`assets/popups/mascottes/insecte-*.webp`), il ne vient pas.
    if dessin::photos().is_none() {
        crate::diag!("[insectes] photos absentes : {} ne vient pas", i.espece.nom());
        i.phase = Phase::Eteint;
        return;
    }
    let ecrans = crate::win32::monitors::enumerate();
    let occupes = ecrans_occupes();
    let libres: Vec<_> = ecrans.iter().filter(|m| !occupes.contains(&m.handle)).collect();
    if libres.is_empty() {
        i.phase = Phase::Absent { retour: apres(5.0) };
        return;
    }
    let m = libres[(hasard() * libres.len() as f32) as usize % libres.len()];
    let centre = ((m.rect.left + m.rect.right) as f32 / 2.0, (m.rect.top + m.rect.bottom) as f32 / 2.0);
    adopter_ecran(i, centre);
    let bord = (hasard() * 4.0) as u32;
    i.pos = vol::point_d_entree(i.ecran, bord, entre((0.15, 0.85)), i.cote as f32);
    i.vit = (0.0, 0.0);
    i.cible = nouvelle_pose(i, souris);
    i.cap = (i.cible.1 - i.pos.1).atan2(i.cible.0 - i.pos.0);
    i.peinte = None;
    decoller(i, maintenant, i.cible, false, son);
    crate::diag!("[insectes] {} entre à l'écran ({:.0}, {:.0})", i.espece.nom(), i.pos.0, i.pos.1);
}

/// La mouche posée fait de temps en temps quelques pas.
fn trottiner(i: &mut Insecte, maintenant: Instant, a: &vol::Allure) {
    if !a.marche {
        return;
    }
    if let Some(p) = i.pas {
        let t = (maintenant.duration_since(p.debut).as_secs_f32() / p.duree).min(1.0);
        i.pos = (p.de.0 + (p.vers.0 - p.de.0) * t, p.de.1 + (p.vers.1 - p.de.1) * t);
        if t >= 1.0 {
            i.pas = None;
            i.prochain_pas = apres(entre((1.2, 4.0)));
        }
        return;
    }
    if maintenant < i.prochain_pas {
        return;
    }
    i.cap += (hasard() * 2.0 - 1.0) * 1.4;
    let longueur = entre((6.0, 18.0)) * i.echelle;
    let vers = (i.pos.0 + i.cap.cos() * longueur, i.pos.1 + i.cap.sin() * longueur);
    let zone = i.travail.retrecie(i.cote as f32 / 2.0);
    if !zone.contient(vers) {
        // Face au bord : demi-tour sur place, il repartira de l'autre côté.
        i.cap += std::f32::consts::PI;
        i.prochain_pas = apres(entre((0.4, 1.0)));
        return;
    }
    i.pas = Some(Pas { de: i.pos, vers, debut: maintenant, duree: longueur / (40.0 * i.echelle) });
}

fn voler(i: &mut Insecte, maintenant: Instant, dt: f32, souris: (f32, f32), sortir: bool, a: &vol::Allure) -> Travail {
    if maintenant >= i.prochain_virage {
        i.deviation = (hasard() * 2.0 - 1.0) * a.virage;
        i.prochain_virage = apres(entre(a.entre_virages));
    }
    let rayon = a.rayon_fuite * i.echelle;
    // Le pointeur s'est posé sur sa cible : elle en choisit une autre.
    if !sortir && vol::frole(i.cible, souris, rayon * 1.5) {
        i.cible = nouvelle_pose(i, souris);
    }
    // Sa cible est sur un écran devenu plein écran : un autre point, sur un
    // écran libre, ou il s'en va.
    if !sortir && occupe(i.cible) {
        match pose_sur_ecran_libre(i, souris, false).filter(|p| !occupe(*p)) {
            Some(p) => i.cible = p,
            None => {
                i.cible = vol::point_de_sortie(i.pos, i.ecran, i.cote as f32 * 2.0);
                i.phase = Phase::Vol { sortir: true };
            }
        }
    }
    if maintenant.duration_since(i.debut_vol) > VOL_MAX {
        i.cible = vol::point_de_sortie(i.pos, i.ecran, i.cote as f32 * 2.0);
        i.phase = Phase::Vol { sortir: true };
        i.debut_vol = maintenant;
    }
    let sortir = matches!(i.phase, Phase::Vol { sortir: true });
    let vitesse = if maintenant < i.fuite_jusqu_a { a.vitesse_fuite } else { a.vitesse } * i.echelle;
    i.vit = vol::piloter(i.pos, i.vit, i.cible, vitesse, a.agilite, i.deviation, souris, rayon, dt);
    i.pos = (i.pos.0 + i.vit.0 * dt, i.pos.1 + i.vit.1 * dt);
    let v = i.vit.0.hypot(i.vit.1);
    if v > 5.0 * i.echelle {
        i.cap = vol::approcher_angle(i.cap, i.vit.1.atan2(i.vit.0), 10.0 * dt);
    }
    // ⭐ Mouche et moustique : un tour par image (voir BATTEMENT_PAR_IMAGE) ;
    // le papillon garde sa cadence en secondes, lisible.
    let pas = pas_du_battement(i.espece, &a, dt);
    i.battement = (i.battement + pas).fract();

    let d = (i.pos.0 - i.cible.0).hypot(i.pos.1 - i.cible.1);
    if sortir {
        let dehors = i.ecran.retrecie(-(i.cote as f32));
        if !dehors.contient(i.pos) {
            i.phase = Phase::Absent { retour: apres(entre(ABSENCE)) };
            i.bourdon = None;
            i.peinte = None;
            crate::diag!("[insectes] {} quitte l'écran", i.espece.nom());
            return Travail::Cacher(i.hwnd);
        }
    } else if d < (4.0 * i.echelle).max(v * dt * 1.5) && !vol::frole(i.pos, souris, rayon) {
        // Posée : l'écran d'arrivée donne la taille.
        i.pos = i.cible;
        i.vit = (0.0, 0.0);
        i.bourdon = None;
        let ancien = i.cote;
        adopter_ecran(i, i.pos);
        if i.cote != ancien {
            i.peinte = None;
        }
        i.phase = Phase::Pose { fin: apres(entre(a.pose)) };
        i.prochain_pas = apres(entre((0.8, 3.0)));
        i.battement = 0.0;
    }
    peindre(i)
}

/// L'image de l'insecte, repeinte seulement si elle change.
fn peindre(i: &mut Insecte) -> Travail {
    let en_vol = matches!(i.phase, Phase::Vol { .. });
    // Le papillon monte et descend au rythme de ses ailes.
    let houle = if en_vol && i.espece.est_papillon() {
        (i.battement * std::f32::consts::TAU).sin() * 2.5 * i.echelle
    } else {
        0.0
    };
    let demi = i.cote as f32 / 2.0;
    let coin = POINT { x: (i.pos.0 - demi).round() as i32, y: (i.pos.1 - demi + houle).round() as i32 };
    let pas_battement = if en_vol { 16.0 } else { 24.0 };
    let cle = (
        (i.cap.to_degrees() / 2.0).round() as i32,
        (i.battement * pas_battement).floor() as i32,
        en_vol,
        i.cote,
    );
    if i.peinte == Some(cle) {
        return Travail::Deplacer(i.hwnd, coin);
    }
    let montrer = i.peinte.is_none();
    i.peinte = Some(cle);
    let m = Moment { cap: i.cap, en_vol, battement: i.battement };
    let Some(photos) = dessin::photos() else { return Travail::Rien };
    let pixels = dessin::dessiner(photos, i.espece, &m, i.echelle, i.cote);
    Travail::Peindre { hwnd: i.hwnd, coin, cote: i.cote, pixels, montrer }
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
                lpszClassName: w!("NeuraDeskInsecte"),
                ..Default::default()
            };
            RegisterClassExW(&wc);
        });
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("NeuraDeskInsecte"),
            w!("NeuraDesk — Insecte"),
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
        .map_err(|err| crate::diag!("[insectes] CreateWindowExW a échoué : {err}"))
        .ok()
    }
}

/// Peint `pixels` (BGRA prémultiplié, `cote`×`cote`) avec le coin haut-gauche en `coin`.
fn poser_image(hwnd: HWND, coin: POINT, cote: u32, pixels: &[u8]) {
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
                if let Err(err) = UpdateLayeredWindow(
                    hwnd,
                    Some(ecran),
                    Some(&coin),
                    Some(&SIZE { cx: c, cy: c }),
                    Some(memoire),
                    Some(&POINT::default()),
                    COLORREF(0),
                    Some(&fusion),
                    ULW_ALPHA,
                ) {
                    crate::diag!("[insectes] UpdateLayeredWindow a échoué : {err}");
                }
                SelectObject(memoire, ancien);
                let _ = DeleteObject(bitmap.into());
            }
            _ => crate::diag!("[insectes] CreateDIBSection {c}×{c} a échoué"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chaque_espece_a_un_nom_relu() {
        for e in ESPECES {
            assert_eq!(Espece::lire(e.nom()), Some(e));
        }
        assert_eq!(Espece::lire("guepe"), None);
        assert!(!Espece::Papillon(2).bourdonne(), "le papillon est muet");
        // La fenêtre grandit avec la taille réglée, comme le dessin.
        assert_eq!(cote_fenetre(Espece::Papillon(0), 1.5 * 2.5), 465);
        for (k, e) in ESPECES.into_iter().enumerate() {
            assert_eq!(e.rang(), k);
        }
        assert_eq!(Espece::lire("papillon-5"), Some(Espece::Papillon(4)));
        assert_eq!(Espece::lire("moustique-3"), Some(Espece::Moustique(2)));
        // Les réglages d'avant les familles : le premier de chaque.
        assert_eq!(Espece::lire("mouche"), Some(Espece::Mouche(0)));
        assert_eq!(Espece::lire("moustique"), Some(Espece::Moustique(0)));
        assert!(Espece::Mouche(4).bourdonne() && Espece::Moustique(4).bourdonne());
    }

    /// ⭐ DEMANDE EXPLICITE (2026-11-25) : « il faut faire bouger les ailes plus
    /// vite ». La mouche et le moustique avancent d'un tour PAR IMAGE — le plus
    /// vite montrable sans roue de chariot —, même quand une image dure plus
    /// longtemps ; le papillon garde sa cadence en secondes.
    #[test]
    fn la_mouche_bat_au_maximum_de_l_ecran_et_le_papillon_a_son_rythme() {
        assert!(BATTEMENT_PAR_IMAGE < 0.5, "au-delà d'un demi-tour par image, l'aile repart à l'envers");
        let (mouche, papillon) = (vol::allure(Espece::Mouche(0)), vol::allure(Espece::Papillon(0)));
        for dt in [0.016f32, 0.05] {
            assert_eq!(pas_du_battement(Espece::Mouche(0), &mouche, dt), BATTEMENT_PAR_IMAGE);
            assert_eq!(pas_du_battement(Espece::Moustique(0), &mouche, dt), BATTEMENT_PAR_IMAGE);
        }
        let pas = pas_du_battement(Espece::Papillon(0), &papillon, 0.05);
        assert!((pas - papillon.battement_vol * 0.05).abs() < 1e-6, "papillon : {pas}");
    }

    /// Une planche de revue (posés puis en vol, agrandis ×5 sur fond clair) et
    /// les deux bourdonnements en WAV, dans `NEURADESK_REVUE_INSECTES`.
    /// Les écrans que la détection voit occupés, sur la machine réelle.
    #[test]
    #[ignore = "sonde à la main"]
    fn sonde_ecrans_en_plein_ecran() {
        let occupes = crate::win32::window::ecrans_en_plein_ecran();
        for m in crate::win32::monitors::enumerate() {
            eprintln!("{} {:?} plein écran : {}", m.device_name, m.rect, occupes.contains(&m.handle));
        }
    }

    #[test]
    #[ignore = "revue à la main"]
    fn planche_de_revue() {
        let Ok(dossier) = std::env::var("NEURADESK_REVUE_INSECTES") else { return };
        let dossier = std::path::Path::new(&dossier);
        let photos = Photos::charger(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/popups/mascottes")).unwrap();
        let echelle = 3.0;
        let cote = ESPECES.iter().map(|e| (dessin::cote_logique(*e) * echelle).ceil() as u32).max().unwrap();
        let colonnes = 6u32;
        let mut planche = image::RgbaImage::from_pixel(cote * colonnes, cote * ESPECES.len() as u32, image::Rgba([236, 232, 222, 255]));
        for (ligne, espece) in ESPECES.into_iter().enumerate() {
            for col in 0..colonnes {
                let m = Moment { cap: -0.5 + col as f32 * 0.35, en_vol: col >= 2, battement: col as f32 * 0.17 };
                let c = (dessin::cote_logique(espece) * echelle).ceil() as u32;
                let px = dessin::dessiner(&photos, espece, &m, echelle, c);
                for y in 0..c {
                    for x in 0..c {
                        let i = ((y * c + x) * 4) as usize;
                        let a = px[i + 3] as f32 / 255.0;
                        let fond = planche.get_pixel(col * cote + x, ligne as u32 * cote + y).0;
                        let mel = |k: usize, f: u8| (px[i + k] as f32 + f as f32 * (1.0 - a)) as u8;
                        planche.put_pixel(col * cote + x, ligne as u32 * cote + y, image::Rgba([mel(2, fond[0]), mel(1, fond[1]), mel(0, fond[2]), 255]));
                    }
                }
            }
        }
        planche.save(dossier.join("insectes-revue.png")).unwrap();
        for espece in [Espece::Mouche(0), Espece::Moustique(0)] {
            let b = bourdon::boucle(espece);
            let mut wav = Vec::new();
            let octets = (b.len() * 2 * 2) as u32; // deux tours de boucle
            wav.extend_from_slice(b"RIFF");
            wav.extend_from_slice(&(36 + octets).to_le_bytes());
            wav.extend_from_slice(b"WAVEfmt ");
            wav.extend_from_slice(&16u32.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes());
            wav.extend_from_slice(&bourdon::FREQUENCE.to_le_bytes());
            wav.extend_from_slice(&(bourdon::FREQUENCE * 2).to_le_bytes());
            wav.extend_from_slice(&2u16.to_le_bytes());
            wav.extend_from_slice(&16u16.to_le_bytes());
            wav.extend_from_slice(b"data");
            wav.extend_from_slice(&octets.to_le_bytes());
            for _ in 0..2 {
                for x in b {
                    wav.extend_from_slice(&x.to_le_bytes());
                }
            }
            std::fs::write(dossier.join(format!("bourdon-{}.wav", espece.nom())), wav).unwrap();
        }
    }
}
