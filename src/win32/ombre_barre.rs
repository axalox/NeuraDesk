//! **L'OMBRE SOUS LA BARRE DU HAUT, SUR TOUTE SA LONGUEUR** — DEMANDE EXPLICITE
//! (2026-10-15) : « utiliser cet effet sur toute la longueur de la barre du haut,
//! sur les dashs, menus et fenêtres de programmes ».
//!
//! Une fenêtre par barre du haut visible, collée sous elle, peinte par
//! `UpdateLayeredWindow` (comme `win32::coins_ecran`) : du noir dont l'alpha
//! décroît de `ui::dock::ombre_barre_haut_force()` à rien, sur
//! `ui::dock::ombre_barre_haut()` pixels — sur la zone libre ; sur les barres,
//! c'est le dock qui la peint (voir plus bas). Elle est **transparente aux clics**
//! (`WS_EX_TRANSPARENT`) et ne prend jamais le focus.
//!
//! **Une fenêtre à part, et pas des quads du dock** : la région du dock rogne
//! tout ce qui est dessiné hors de ses barres (voir `win32::window::
//! set_dock_region`) — l'ombre ne pourrait jamais tomber sur la zone libre.
//!
//! **Au-dessus de tout** : elle est remontée avec les coins d'écran à chaque
//! remontée du dock (`Window::raise_to_top`), et régulièrement par
//! `synchroniser` — un menu ou un dash ouvert après elle passe dessous en moins
//! de `REMONTEE`.
//!
//! ⭐ **SAUF QUAND UNE ICÔNE EST AGRANDIE** — DEMANDE EXPLICITE (2026-11-11) :
//! « l'effet de magnification doit faire apparaître les icônes au-dessus des
//! ombrages, des dashs et des menus ». L'icône agrandie est dessinée par la
//! fenêtre du dock ; elle ne peut donc passer devant les ombres que si le dock
//! passe devant elles. Tant que `ui::dock::magnification_en_cours` est vrai,
//! `ranger` remonte les docks en tête et pose OMBRES ET MENUS dessous (voir
//! `rangement_magnifie`) ; la magnification retombée, `app::run` rappelle
//! `remonter` et l'ordre normal revient. ⛔ **Les ombres ne sont plus possédées
//! par le dock** (mais par `socle`) : une fenêtre possédée est toujours
//! AU-DESSUS de son propriétaire, donc aucun `SetWindowPos` n'aurait pu les
//! faire passer sous lui.
//!
//! **Elle suit la barre** : chaque rendu de dock DÉCLARE sa barre (`declarer`),
//! et une barre dont la fenêtre est masquée (plein écran, barre escamotée)
//! n'a pas d'ombre.
//!
//! ⭐ **LES AUTRES BARRES AUSSI** — DEMANDE EXPLICITE (2026-10-16) : « un effet
//! d'ombrage paramétrable pour tout le reste de la zone intérieure, désactivable
//! dans les réglages ». Les barres de gauche, de droite et du bas jettent la même
//! ombre vers l'intérieur (`Cote`), une fenêtre chacune, avec leurs réglages
//! communs (`ReglagesCotes`) : largeur, force, et une case par barre. ÉTEINTES par
//! défaut, comme tout effet permanent. Elles ne dépendent pas de « sur toute la
//! longueur », qui ne concerne que la barre du haut.
//!
//! ⭐ **À LA COULEUR DE SA BARRE** — DEMANDE EXPLICITE (2026-10-18) : « une option
//! pour que l'effet d'ombrage des barres et des coins arrondis intérieurs prennent
//! la couleur de la barre auquel il est rattaché ». Cochée (`couleur_des_barres`),
//! chaque pixel prend la teinte de la barre qui l'ombre le plus fort — celle du
//! haut sous la barre du haut, sa colonne dans un congé du haut, la barre du bas
//! dans un arc du bas. Décochée, du noir, comme avant.
//!
//! ⭐ **SEULEMENT SUR L'INTERFACE** — PRÉCISIONS DEMANDÉES (2026-10-18) : « l'ombre
//! colorée ne doit apparaître que sur les autres éléments de l'interface, sinon
//! l'effet doit être l'ombre où l'interface n'est pas recouverte ». La teinte vaut
//! là où l'ombre tombe sur une autre barre ou sur un congé (`Interieur::
//! sur_l_interface`, `sur_l_interface_haut`) ; sur la zone libre, du noir.
//!
//! ⭐ **LA COULEUR RENDUE, PAS LA TEINTE RÉGLÉE** — DÉFAUT SIGNALÉ (2026-10-19) :
//! « l'ombre de couleur qui vient de la barre du haut ne suit pas la couleur de la
//! barre du haut automatiquement ». Chaque barre porte un `Profil` de couleurs
//! RELUES dans le rendu du dock (voir `gfx::releve_barres`) : le bureau flouté,
//! les lueurs et le dégradé y sont, et la teinte varie le long de la barre. La
//! teinte réglée ne sert plus qu'en attendant le premier relevé.
//!
//! ⭐ **OU LA COULEUR RÉGLÉE, AU CHOIX** — DEMANDE EXPLICITE (2026-10-19) : « les
//! ombres de couleur ne semblent pas utiliser la couleur réglée dans l'app pour
//! les barres » ; mesuré : barre du haut réglée (232, 17, 35), rendue et donc
//! ombrée (204, 38, 78). Choix retenu : « ajoute un choix ». Couleur affichée
//! (`couleur_affichee`, par défaut : le comportement ci-dessus) ou couleur réglée,
//! telle quelle, sans relevé (voir `couleurs_d_ombre`).
//!
//! ⭐ **DEUX OMBRES, DEUX JEUX DE RÉGLAGES COMPLETS** — DEMANDE EXPLICITE
//! (2026-10-19) : « je veux des réglages complets pour chaque type d'ombre, noire
//! et couleur ». La noire (réglages historiques : `ui::dock::ombre_barre_haut`,
//! `ReglagesCotes`, `INTENSITE_NOIRE`) et la colorée (`ReglagesCouleur`,
//! `INTENSITE_COULEUR`) ont chacune leurs largeurs, leurs forces et leurs barres,
//! et leurs PROPRES fenêtres (`Couche`) : la colorée posée d'abord, la noire
//! ensuite, qui passe au-dessus (voir `composer`).
//!
//! ⭐ **SOUS LES ICÔNES ET LES TEXTES DES BARRES** — DEMANDE EXPLICITE
//! (2026-10-19) : « les 2 types d'ombre doivent apparaître sous les icônes et
//! textes des barres ». Les fenêtres d'ombre sont posées SUR le dock : elles
//! couvraient tout ce qu'il peint. La part des ombres qui tombe sur l'interface
//! est donc peinte PAR LE DOCK : un calque à la taille de sa fenêtre (`Calque`),
//! que son shader pose sur les fonds de barre seulement, sous le contenu
//! (`OmbresTex`, voir `gfx::quad_renderer`). Les fenêtres ne gardent que la part
//! NOIRE sur la zone libre (`Portion::Libre`) — la colorée n'y tombe jamais.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU16, Ordering};
use std::time::{Duration, Instant};

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER,
    BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, DIB_RGB_COLORS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetTopWindow, GetWindow, IsWindowVisible, GW_HWNDNEXT, RegisterClassExW, SetWindowDisplayAffinity, SetWindowPos,
    ShowWindow, UpdateLayeredWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_CLOSE,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use windows::Win32::Graphics::Direct3D11::{ID3D11ShaderResourceView, ID3D11Texture2D};

use crate::gfx::releve_barres::{Profil, SEGMENTS};
use crate::ui::dock::DockLayout;

/// L'écart (par canal) en deçà duquel un nouveau relevé ne repeint pas les
/// ombres : le grain de la texture et le tramage font bouger la médiane d'un
/// ou deux niveaux d'un relevé à l'autre.
const TOLERANCE: u8 = 4;

/// Le plus long qu'un menu ou un dash ouvert après l'ombre reste au-dessus d'elle.
const REMONTEE: Duration = Duration::from_millis(250);

/// ⭐ **OÙ TOMBE L'OMBRE NOIRE, BARRE PAR BARRE** — DEMANDE EXPLICITE
/// (2026-10-19) : « gérer où les ombres noires s'appliquent, sur les autres
/// barres, sur la zone centrale pour chaque barre ». Par barre `[haut, gauche,
/// droite, bas]` (voir `Cote::rang`) : sur les autres barres et les coins arrondis
/// (le calque du dock), sur la zone centrale (les fenêtres d'ombre, au-dessus des
/// dashs, menus et fenêtres). Dans un coin, un pixel suit la barre qui l'ombre le
/// plus fort. `zone[0]` est l'ancienne case « sur toute la longueur ». La colorée
/// n'en dépend pas : elle ne tombe que sur les barres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Portee {
    pub barres: [bool; 4],
    pub zone: [bool; 4],
}

impl Portee {
    pub const DEFAUT: Self = Self { barres: [true; 4], zone: [true; 4] };

    /// Les huit cases en 0/1 : `barres` puis `zone`.
    pub fn serialiser(&self) -> String {
        self.barres.iter().chain(&self.zone).map(|&v| if v { "1" } else { "0" }).collect::<Vec<_>>().join(",")
    }

    /// Relit `serialiser` ; une case absente ou illisible garde celle de `defaut`.
    pub fn charger(contenu: &str, defaut: Self) -> Self {
        let cases: Vec<Option<bool>> = contenu
            .trim()
            .split(',')
            .map(|c| match c.trim() {
                "1" => Some(true),
                "0" => Some(false),
                _ => None,
            })
            .collect();
        let case = |i: usize, d: bool| cases.get(i).copied().flatten().unwrap_or(d);
        Self {
            barres: std::array::from_fn(|i| case(i, defaut.barres[i])),
            zone: std::array::from_fn(|i| case(4 + i, defaut.zone[i])),
        }
    }
}

static PORTEE: std::sync::Mutex<Portee> = std::sync::Mutex::new(Portee::DEFAUT);

/// ⭐ **LES OMBRES D'UN ÉCRAN SECONDAIRE, RÉGLÉES À PART** — DEMANDE EXPLICITE
/// (2026-10-19) : « il faudrait que l'effet d'ombre des écrans secondaires soit
/// réglable ». Un écran secondaire n'a que sa barre du haut : l'ombre noire sous
/// elle `[largeur px, force 0..=1]` et si elle tombe sur la zone centrale, l'ombre
/// colorée `[largeur, force]`. Un écran absent de la table suit l'écran principal ;
/// les intensités, la couleur et la texture restent communes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReglagesEcran {
    pub noire: [f32; 2],
    pub zone: bool,
    pub couleur: [f32; 2],
}

impl ReglagesEcran {
    /// Ceux de l'écran principal, sous sa barre du haut : le point de départ d'un
    /// écran qu'on règle à part.
    pub fn du_principal() -> Self {
        let c = reglages_couleur();
        Self {
            noire: [crate::ui::dock::ombre_barre_haut(), crate::ui::dock::ombre_barre_haut_force()],
            zone: portee().zone[0],
            couleur: [c.largeur_haut, c.force_haut],
        }
    }

    fn bornes(self) -> Self {
        let trait_ = |[l, f]: [f32; 2], largeur_max: f32, d: [f32; 2]| {
            let ok = |v: f32, max: f32, d: f32| if v.is_finite() { v.clamp(0.0, max) } else { d };
            [ok(l, largeur_max, d[0]), ok(f, 1.0, d[1])]
        };
        let d = [crate::ui::dock::DEFAULT_OMBRE_BARRE_HAUT, crate::ui::dock::DEFAULT_OMBRE_BARRE_HAUT_FORCE];
        Self {
            noire: trait_(self.noire, crate::ui::dock::MAX_OMBRE_BARRE_HAUT, d),
            couleur: trait_(self.couleur, MAX_LARGEUR_COULEUR, d),
            ..self
        }
    }

    /// « largeur,force,zone,largeur,force » (noire puis colorée), la case en 0/1.
    pub fn serialiser(&self) -> String {
        let [nl, nf] = self.noire;
        let [cl, cf] = self.couleur;
        format!("{nl},{nf},{},{cl},{cf}", if self.zone { 1 } else { 0 })
    }

    /// Relit `serialiser` ; `None` s'il manque un champ.
    pub fn charger(contenu: &str) -> Option<Self> {
        let v: Vec<f32> = contenu.split(',').map(|c| c.trim().parse::<f32>().ok()).collect::<Option<_>>()?;
        let [nl, nf, zone, cl, cf] = v[..] else { return None };
        Some(Self { noire: [nl, nf], zone: zone >= 0.5, couleur: [cl, cf] }.bornes())
    }
}

/// Les écrans secondaires réglés à part, par nom de périphérique (`\\.\DISPLAY2`).
static ECRANS: std::sync::Mutex<Vec<(String, ReglagesEcran)>> = std::sync::Mutex::new(Vec::new());

/// Les réglages de l'écran `ecran`, `None` s'il suit l'écran principal.
pub fn reglages_ecran(ecran: &str) -> Option<ReglagesEcran> {
    ECRANS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(nom, _)| nom == ecran).map(|(_, r)| *r)
}

/// Règle l'écran `ecran` à part (`Some`, bornés) ou le rend à l'écran principal.
pub fn poser_reglages_ecran(ecran: &str, reglages: Option<ReglagesEcran>) {
    let mut ecrans = ECRANS.lock().unwrap_or_else(|e| e.into_inner());
    ecrans.retain(|(nom, _)| nom != ecran);
    if let Some(r) = reglages {
        ecrans.push((ecran.to_string(), r.bornes()));
    }
}

/// Une ligne `écran,réglages` par écran réglé à part (voir `config`).
pub fn serialiser_ecrans() -> String {
    ECRANS.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|(nom, r)| format!("{nom},{}", r.serialiser())).collect::<Vec<_>>().join("\n")
}

pub fn charger_ecrans(contenu: &str) {
    let ecrans = contenu
        .lines()
        .filter_map(|ligne| {
            let (nom, reste) = ligne.split_once(',')?;
            Some((nom.trim().to_string(), ReglagesEcran::charger(reste)?))
        })
        .collect();
    *ECRANS.lock().unwrap_or_else(|e| e.into_inner()) = ecrans;
}

pub fn portee() -> Portee {
    *PORTEE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn poser_portee(portee: Portee) {
    *PORTEE.lock().unwrap_or_else(|e| e.into_inner()) = portee;
}

/// Les ombres prennent-elles la couleur de leur barre (au lieu du noir) ?
/// Éteinte par défaut.
static COULEUR_DES_BARRES: AtomicBool = AtomicBool::new(false);

pub fn couleur_des_barres() -> bool {
    COULEUR_DES_BARRES.load(Ordering::Relaxed)
}

pub fn activer_couleur_des_barres(active: bool) {
    COULEUR_DES_BARRES.store(active, Ordering::Relaxed);
}

/// Les ombres colorées prennent-elles la couleur AFFICHÉE de leur barre (relue
/// dans le rendu), ou sa couleur RÉGLÉE ? Affichée par défaut.
static COULEUR_AFFICHEE: AtomicBool = AtomicBool::new(true);

pub fn couleur_affichee() -> bool {
    COULEUR_AFFICHEE.load(Ordering::Relaxed)
}

pub fn choisir_couleur_affichee(affichee: bool) {
    COULEUR_AFFICHEE.store(affichee, Ordering::Relaxed);
}

/// ⭐ **LA NOIRE DE LA ZONE CENTRALE, À LA COULEUR DE SA BARRE** — DEMANDE
/// EXPLICITE (2026-10-23) : « remplacer les ombres noires de la zone centrale par
/// la couleur de la barre adjacente en tenant compte des ombrages de couleur s'ils
/// sont activés ». Revient, sur option, sur la règle du 2026-10-18 (« sur la zone
/// libre, du noir ») : la part LIBRE de la noire (ses fenêtres) prend la teinte de
/// la barre qui l'ombre (`Barre::teintes_zone`) ; sa part sur les barres reste
/// noire. Avec l'ombre de couleur, c'est SA couleur (affichée relue ou réglée,
/// voir `profil_de_zone`) ; sans elle, la couleur de la barre. Éteinte par défaut.
static ZONE_EN_COULEUR: AtomicBool = AtomicBool::new(false);

pub fn zone_en_couleur() -> bool {
    ZONE_EN_COULEUR.load(Ordering::Relaxed)
}

pub fn activer_zone_en_couleur(active: bool) {
    ZONE_EN_COULEUR.store(active, Ordering::Relaxed);
}

/// Faut-il relire la couleur rendue des barres (voir `gfx::releve_barres`) ?
pub fn releve_utile() -> bool {
    couleur_des_barres() && couleur_affichee()
}

/// Les couleurs à passer à `declarer` : les `reglees` (RGB 0..=1, celles des
/// réglages, telles quelles) ou les `peintes` (après `color::soften_tint`, la
/// référence du relevé), selon `couleur_affichee`.
pub fn couleurs_d_ombre(reglees: [[f32; 3]; 4], peintes: [[f32; 3]; 4]) -> [[f32; 3]; 4] {
    if couleur_affichee() { peintes } else { reglees }
}

/// ⭐ **L'INTENSITÉ DES OMBRES COLORÉES** — DEMANDE EXPLICITE (2026-10-18) :
/// « régler les intensités des ombres colorées indépendamment des ombres
/// noires ». Un facteur sur l'opacité de la part TEINTÉE seulement (là où l'ombre
/// tombe sur l'interface) ; la part noire garde sa force. En pour-cent, 100 :
/// la même force que le noir.
static INTENSITE_COULEUR: AtomicU16 = AtomicU16::new(100);

/// Le plafond du réglage, en pour-cent.
pub const MAX_INTENSITE_COULEUR: u16 = 300;

pub fn intensite_couleur() -> u16 {
    INTENSITE_COULEUR.load(Ordering::Relaxed)
}

pub fn regler_intensite_couleur(pc: f32) {
    INTENSITE_COULEUR.store(borner_intensite(pc), Ordering::Relaxed);
}

/// ⭐ **L'OMBRE NOIRE ET L'OMBRE DE COULEUR, CHACUNE LA SIENNE** — DEMANDE
/// EXPLICITE (2026-10-19) : « pouvoir gérer individuellement l'ombre noire et
/// l'ombre de couleur et que l'ombre noire soit au-dessus de l'ombre de
/// couleur ». Deux couches sur la même courbe : la COLORÉE, sur l'interface
/// seulement (`couleur_des_barres`, à `INTENSITE_COULEUR`), et la NOIRE, partout,
/// posée PAR-DESSUS (à cette intensité). En pour-cent ; 0 : pas d'ombre noire.
static INTENSITE_NOIRE: AtomicU16 = AtomicU16::new(100);

pub fn intensite_noire() -> u16 {
    INTENSITE_NOIRE.load(Ordering::Relaxed)
}

pub fn regler_intensite_noire(pc: f32) {
    INTENSITE_NOIRE.store(borner_intensite(pc), Ordering::Relaxed);
}

fn borner_intensite(pc: f32) -> u16 {
    if pc.is_finite() { pc.round().clamp(0.0, MAX_INTENSITE_COULEUR as f32) as u16 } else { 100 }
}

/// ⭐ **LA TEXTURE PAR-DESSUS L'OMBRE COLORÉE** — DEMANDE EXPLICITE (2026-10-18) :
/// « il faudrait que la texture apparaisse au-dessus des dégradés de couleur ».
/// Ces ombres sont des fenêtres posées SUR les barres : leur teinte recouvrait la
/// texture que le shader y avait peinte (voir `gfx::texture_interface`). La part
/// teintée la reçoit donc à son tour — le même motif, au même pixel d'écran, rendu
/// par le même shader (`texture_interface::calque_rgba_a`) avec le device du dock
/// (`poser_gpu`).
struct Gpu {
    device: windows::Win32::Graphics::Direct3D11::ID3D11Device,
    ctx: windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    /// Compilé à la première ombre texturée seulement.
    renderer: Option<crate::gfx::QuadRenderer>,
}

thread_local! {
    static GPU: RefCell<Option<Gpu>> = const { RefCell::new(None) };
}

/// Rend au device tout ce que les ombres en détiennent — **À APPELER AVANT LA
/// DESTRUCTION DU DEVICE** (voir `app::GpuCacheGuard`).
///
/// ⛔ Sans elle, `GPU` (une COPIE du device) et les textures de `CALQUES` ne
/// mouraient qu'au démontage du processus, avec les `thread_local!` : DXGI levait
/// alors l'exception `0x87A` et NeuraDesk sortait en **code 122** à CHAQUE
/// fermeture (relevé au débogueur le 2026-10-25 : `dxgi!CDXGIFactory::FinalRelease`
/// sous `LdrShutdownProcess`). Le défaut du 2026-09-16, revenu avec ce module.
///
/// L'ORDRE est porteur : les enfants (textures, `QuadRenderer`) avant le device.
pub fn liberer_gpu() {
    CALQUES.with(|c| {
        if let Ok(mut c) = c.try_borrow_mut() {
            c.clear();
        }
    });
    GPU.with(|g| {
        if let Ok(mut g) = g.try_borrow_mut() {
            if let Some(mut gpu) = g.take() {
                gpu.renderer = None;
            }
        }
    });
}

/// Le device du dock, pour rendre la texture des ombres (même fil que
/// `synchroniser`).
pub fn poser_gpu(device: &windows::Win32::Graphics::Direct3D11::ID3D11Device, ctx: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext) {
    GPU.with(|g| *g.borrow_mut() = Some(Gpu { device: device.clone(), ctx: ctx.clone(), renderer: None }));
}

/// L'empreinte de la texture de l'interface, pour repeindre les ombres quand
/// elle change ; 0 : pas de texture à poser (aucune, ou ombres noires).
fn empreinte_texture() -> u64 {
    if !couleur_des_barres() { 0 } else { empreinte_de_la_texture() }
}

/// L'empreinte de la texture de l'interface, qu'il y ait des ombres colorées ou
/// non (le centre de la barre du haut la reçoit toujours) ; 0 : aucune texture.
fn empreinte_de_la_texture() -> u64 {
    use std::hash::{Hash, Hasher};
    let texture = &crate::gfx::texture_interface::INTERFACE;
    if texture.nature() == 0 {
        return 0;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    texture.serialiser().hash(&mut h);
    crate::gfx::texture_perso::INTERFACE.serialiser().hash(&mut h);
    h.finish() | 1
}

/// Le calque de la texture (RGBA à alpha droit, voir `QuadInstance::
/// new_calque_texture`) sur le rectangle d'une ombre. `None` : pas de GPU, ou
/// il a refusé — l'ombre reste sans texture.
fn calque_de(ombre: &Ombre) -> Option<Vec<u8>> {
    calque_de_texture((ombre.x, ombre.y), (ombre.largeur, ombre.hauteur))
}

/// Le calque de la texture sur le rectangle `taille` posé à `origine` (écran).
fn calque_de_texture(origine: (i32, i32), (largeur, hauteur): (i32, i32)) -> Option<Vec<u8>> {
    GPU.with(|g| {
        let mut g = g.try_borrow_mut().ok()?;
        let gpu = g.as_mut()?;
        if gpu.renderer.is_none() {
            gpu.renderer = crate::gfx::QuadRenderer::new(&gpu.device).ok();
        }
        let renderer = gpu.renderer.as_ref()?;
        let taille = (largeur.max(0) as u32, hauteur.max(0) as u32);
        crate::gfx::texture_interface::calque_rgba_a(
            &gpu.device,
            &gpu.ctx,
            renderer,
            &crate::gfx::texture_interface::INTERFACE,
            &crate::gfx::texture_perso::INTERFACE,
            taille,
            origine,
        )
    })
    .filter(|rgba| rgba.len() == (largeur.max(0) * hauteur.max(0) * 4) as usize)
}

/// Une couche d'ombre : la NOIRE, partout, ou la COLORÉE, sur l'interface
/// seulement — chacune ses fenêtres, à son intensité en pour-cent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Couche {
    coloree: bool,
    intensite: u16,
    /// Où elle tombe (la noire seulement, voir `Portee`).
    portee: Portee,
    /// La noire seulement : sa part sur la zone libre prend la teinte de sa barre
    /// (voir `zone_en_couleur`).
    teintee_zone: bool,
}

impl Couche {
    const NOIRE: Self = Self { coloree: false, intensite: 100, portee: Portee::DEFAUT, teintee_zone: false };
    #[cfg(test)]
    const COLOREE: Self = Self { coloree: true, intensite: 100, portee: Portee::DEFAUT, teintee_zone: false };

    /// Cette couche peint-elle sa teinte sur `portion` ? La colorée, partout où
    /// elle tombe (l'interface) ; la noire, sur la zone libre si `teintee_zone`.
    fn teinte_sur(self, portion: Portion) -> bool {
        self.coloree || (self.teintee_zone && portion == Portion::Libre)
    }

    /// Les parts `(sur les barres, sur la zone centrale)` d'une ombre jetée par
    /// `barre` : la colorée sur les barres seulement ; la noire selon sa portée.
    fn parts(self, barre: Cote) -> (f32, f32) {
        let i = barre.rang();
        let oui = |v: bool| if v { 1.0 } else { 0.0 };
        if self.coloree { (1.0, 0.0) } else { (oui(self.portee.barres[i]), oui(self.portee.zone[i])) }
    }
}

/// La part d'une ombre à peindre : sur la zone LIBRE (sa fenêtre, la noire
/// seulement), ou sur l'INTERFACE (le calque du dock, voir `Calque`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Portion {
    Libre,
    Interface,
    /// Les deux à la fois : la forme entière de l'ombre, pour les tests.
    #[cfg(test)]
    Toute,
}

/// Les réglages complets d'une couche : son ombre du haut `[hauteur, force]` et
/// celles des autres barres.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Reglages {
    haut: [f32; 2],
    cotes: ReglagesCotes,
    couche: Couche,
}

/// Les deux couches en vigueur, dans l'ordre où elles se POSENT : la colorée
/// (intensité nulle sans l'option), puis la noire, par-dessus.
fn reglages_en_vigueur() -> [Reglages; 2] {
    let c = reglages_couleur();
    let intensite = if couleur_des_barres() { intensite_couleur() } else { 0 };
    [
        Reglages { haut: [c.largeur_haut, c.force_haut], cotes: c.cotes, couche: Couche { coloree: true, intensite, portee: Portee::DEFAUT, teintee_zone: false } },
        Reglages {
            haut: [crate::ui::dock::ombre_barre_haut(), crate::ui::dock::ombre_barre_haut_force()],
            cotes: reglages_cotes(),
            couche: Couche { coloree: false, intensite: intensite_noire(), portee: portee(), teintee_zone: zone_en_couleur() },
        },
    ]
}

/// La teinte d'une ombre pour `couleur` (RGB 0..=1, la couleur PEINTE de la
/// barre) : du noir si l'option est éteinte.
fn teinte(couleur: [f32; 3]) -> [u8; 3] {
    if !couleur_des_barres() {
        return [0; 3];
    }
    couleur.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Le profil d'une barre : celui RELU dans le rendu (`releve`, en couleur
/// affichée), sinon `couleur` d'un bout à l'autre ; du noir si l'option est éteinte.
fn profil_de(releve: Option<Profil>, couleur: [f32; 3]) -> Profil {
    match releve {
        Some(profil) if releve_utile() => profil,
        _ => [teinte(couleur); SEGMENTS],
    }
}

/// Le profil de la noire sur la zone centrale (voir `zone_en_couleur`) : avec
/// l'ombre de couleur, le sien (`profil_de` — relevé ou réglé) ; sans elle, la
/// couleur de la barre, que `teinte` aurait éteinte en noir.
fn profil_de_zone(releve: Option<Profil>, couleur: [f32; 3]) -> Profil {
    if couleur_des_barres() {
        return profil_de(releve, couleur);
    }
    [couleur.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8); SEGMENTS]
}

/// ⛔ **LA TEXTURE COMPTAIT DEUX FOIS** — DÉFAUT SIGNALÉ (2026-10-19, capture
/// `image14.png`) : « l'ombre de couleur semble s'appliquer 2 fois ». Contre la
/// barre, l'ombre était PLUS CLAIRE qu'elle (+12 par canal) : une seconde bande.
/// La couleur RELUE contient déjà l'effet moyen de la texture (sa chroma éclaircit
/// toujours, voir `QuadRenderer`), et le calque l'ajoutait encore. Le calque est
/// donc CENTRÉ : il garde son motif, plus son décalage moyen.
///
/// ⛔ **TOUJOURS, PAS SEULEMENT SUR UNE OMBRE RELUE** — DÉFAUT SIGNALÉ (2026-10-19) :
/// en couleur réglée (8, 85, 177), l'ombre mesurait (20, 101, 196) — le même
/// décalage. L'ombre a la couleur choisie, au motif de la texture près.
/// `calque` : RGBA à alpha droit (voir `calque_de`), réencodé sur place.
fn centrer(calque: &mut [u8]) {
    let ecart = |p: &[u8], k: usize| (2.0 * p[k] as f32 / 255.0 - 1.0) * p[3] as f32;
    let n = (calque.len() / 4).max(1) as f32;
    let moyenne: [f32; 3] = std::array::from_fn(|k| calque.chunks_exact(4).map(|p| ecart(p, k)).sum::<f32>() / n);
    for p in calque.chunks_exact_mut(4) {
        let e: [f32; 3] = std::array::from_fn(|k| ecart(p, k) - moyenne[k]);
        let a = e.iter().fold(0.0f32, |m, v| m.max(v.abs())).min(255.0);
        for k in 0..3 {
            p[k] = if a > 0.0 { (255.0 * (e[k] / a + 1.0) / 2.0).round().clamp(0.0, 255.0) as u8 } else { 128 };
        }
        p[3] = a.round() as u8;
    }
}

/// Les deux profils sont-ils indiscernables (à `TOLERANCE` près par canal) ?
fn proches(a: &Profil, b: &Profil) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| x.abs_diff(*y) <= TOLERANCE)
}

/// La teinte de `profil` à la position `t` (`0..=1`) le long de sa barre,
/// interpolée entre les centres des segments.
fn teinte_le_long(profil: &Profil, t: f32) -> [u8; 3] {
    let p = (t.clamp(0.0, 1.0) * SEGMENTS as f32 - 0.5).clamp(0.0, (SEGMENTS - 1) as f32);
    let (i, f) = (p.floor() as usize, p.fract());
    let j = (i + 1).min(SEGMENTS - 1);
    std::array::from_fn(|k| (profil[i][k] as f32 * (1.0 - f) + profil[j][k] as f32 * f).round() as u8)
}

/// Les barres d'une fenêtre de dock, déclarées par son rendu : le coin
/// haut-gauche de la fenêtre à l'écran, sa taille, le bas de la barre du haut
/// (`DockLayout::haut_colonnes` — 0 : pas de barre du haut), l'épaisseur VISIBLE
/// des trois autres (0 : pas de barre, ou masquée) et le rayon des congés de la
/// zone libre (`ui::dock::rayon_conge`, 0 : coins droits), et le profil de leur
/// ombre (`profil_de` : du noir sans l'option), dans l'ordre de `Cote`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Barre {
    origine: (i32, i32),
    largeur: f32,
    hauteur: f32,
    bas: f32,
    gauche: f32,
    droite: f32,
    dessous: f32,
    rayon: f32,
    teintes: [Profil; 4],
    /// Le profil de la noire sur la zone centrale (`profil_de_zone`).
    teintes_zone: [Profil; 4],
    /// Voir `empreinte_texture`.
    texture: u64,
    /// Un écran secondaire réglé à part (voir `ReglagesEcran`).
    ecran: Option<ReglagesEcran>,
}

/// La barre qui jette l'ombre : l'ombre est la plus forte contre elle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cote {
    Haut,
    Gauche,
    Droite,
    Bas,
}

impl Cote {
    /// Son rang dans `[haut, gauche, droite, bas]`.
    fn rang(self) -> usize {
        self as usize
    }
}

/// Une ombre à poser, en pixels écran ; `force` en millièmes (comparable).
/// `interieur` : pour les ombres des côtés, la zone libre qu'elles bordent (voir
/// `Interieur`), d'où vient la distance de chaque pixel ; `None` pour celle du
/// haut, une simple bande.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ombre {
    x: i32,
    y: i32,
    largeur: i32,
    hauteur: i32,
    force: u16,
    cote: Cote,
    interieur: Option<Interieur>,
    /// Le profil de chaque barre, dans l'ordre de `Cote` (voir `Barre`).
    teintes: [Profil; 4],
    /// L'étendue de chaque barre le long d'elle-même, `[début, longueur]` en
    /// pixels ÉCRAN (x pour le haut et le bas, y pour les colonnes) : celle que
    /// couvre son profil (voir `releve_barres::etendues`).
    etendues: [[i32; 2]; 4],
    /// La couche de cette ombre, et son intensité.
    couche: Couche,
    /// La texture à poser sur la part teintée (`empreinte_texture`, 0 : aucune).
    texture: u64,
    /// Pour l'ombre du haut : le bord intérieur (x écran) des colonnes gauche et
    /// droite, et le rayon de leurs congés — voir `sur_l_interface_haut`. `None` :
    /// pas de colonne.
    colonnes: [Option<i32>; 2],
    rayon: i32,
}

impl Ombre {
    /// Cette ombre dans la couche `couche` : une noire n'a ni teinte ni texture —
    /// un nouveau relevé des couleurs ne la repeint pas.
    fn en(self, couche: Couche) -> Self {
        if couche.coloree {
            Self { couche, ..self }
        } else {
            Self { couche, teintes: [[[0; 3]; SEGMENTS]; 4], texture: 0, ..self }
        }
    }

    /// La teinte de la barre `barre` au pixel d'écran `(sx, sy)` : son profil, lu
    /// à la position du pixel le long de la barre — borné à ses bouts au-delà
    /// (un congé du haut prend le haut de sa colonne).
    fn teinte_en(&self, barre: Cote, sx: f32, sy: f32) -> [u8; 3] {
        let [debut, longueur] = self.etendues[barre as usize];
        let le_long = match barre {
            Cote::Haut | Cote::Bas => sx,
            Cote::Gauche | Cote::Droite => sy,
        };
        teinte_le_long(&self.teintes[barre as usize], (le_long - debut as f32) / longueur.max(1) as f32)
    }
}

impl Barre {
    /// Voir `Ombre::etendues`.
    fn etendues(&self) -> [[i32; 2]; 4] {
        let (ox, oy) = self.origine;
        let etendues = crate::gfx::releve_barres::etendues(self.largeur, self.hauteur, self.bas, self.gauche, self.droite);
        let origines = [ox, oy, oy, ox];
        std::array::from_fn(|i| [origines[i] + etendues[i][0].round() as i32, etendues[i][1].round() as i32])
    }
}

/// La part d'un pixel hors d'un arc de rayon `r` (`dx, dy` depuis son centre),
/// lissée sur un pixel — comme le congé que peint le dock.
fn hors_de_l_arc(dx: f32, dy: f32, r: f32) -> f32 {
    ((dx * dx + dy * dy).sqrt() - r + 0.5).clamp(0.0, 1.0)
}

/// La part du pixel `(x, y)` (écran) où l'ombre du haut tombe sur l'interface
/// (`0..=1`) : sur une colonne, ou sur le congé du haut qui la prolonge — `haut`
/// est le bas de la barre du haut, où commencent les congés.
fn sur_l_interface_haut(colonnes: [Option<i32>; 2], rayon: i32, haut: f32, x: f32, y: f32) -> f32 {
    let r = rayon as f32;
    let sur_le_conge = |cx: f32| if r > 0.0 && y < haut + r { hors_de_l_arc(x - cx, y - (haut + r), r) } else { 0.0 };
    let [gauche, droite] = colonnes;
    let depuis_gauche = gauche.map_or(0.0, |g| {
        let g = g as f32;
        if x < g { 1.0 } else if x < g + r { sur_le_conge(g + r) } else { 0.0 }
    });
    let depuis_droite = droite.map_or(0.0, |d| {
        let d = d as f32;
        if x > d { 1.0 } else if x > d - r { sur_le_conge(d - r) } else { 0.0 }
    });
    depuis_gauche.max(depuis_droite)
}

/// ⭐ **LA ZONE LIBRE QUE BORDENT LES OMBRES DES CÔTÉS**, en pixels écran — DÉFAUT
/// SIGNALÉ (2026-10-16) : « il faudrait que l'ombre de la barre du bas ne
/// s'applique pas sur les barres latérales quand elles sont visibles, les coins
/// arrondis du bas ont disparu ». L'ombre est celle d'un rectangle ARRONDI : la
/// distance d'un pixel est celle au bord le plus proche parmi les barres qui
/// ombrent, et dans un coin celle à l'arc du congé — un congé, qui prolonge une
/// barre, ne reçoit rien. Les bandes ne se chevauchent pas : celle du bas prend les
/// coins, les latérales s'arrêtent au-dessus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Interieur {
    gauche: i32,
    droite: i32,
    haut: i32,
    bas: i32,
    rayon: i32,
    /// Les bords qui ombrent : gauche, droite, bas.
    ombrent: [bool; 3],
    /// Les coins arrondis : haut-gauche, haut-droite, bas-gauche, bas-droite.
    coins: [bool; 4],
    /// Les colonnes (gauche, droite) qui ombrent la barre du bas — et donc aussi
    /// son congé, qui la prolonge (voir `alpha`).
    sur_le_bas: [bool; 2],
    /// L'ombre des colonnes, et celle de la barre du bas — réglables À PART
    /// (demande du 2026-10-16, voir `ReglagesCotes::largeur_bas`).
    laterales: Trait,
    dessous: Trait,
}

/// Une ombre : sa largeur en pixels et sa force en millièmes (comparables).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Trait {
    largeur: i32,
    force: u16,
}

impl Trait {
    fn de(largeur: f32, force: f32) -> Self {
        Self { largeur: largeur.round().max(0.0) as i32, force: (force.clamp(0.0, 1.0) * 1000.0).round() as u16 }
    }

    /// L'opacité à la distance `d` du bord : la courbe du shader ; rien dans un
    /// congé (`d` négative) ni au-delà de la largeur.
    fn alpha(self, d: f32) -> f32 {
        if d < 0.0 || self.largeur <= 0 {
            return 0.0;
        }
        let t = 1.0 - (d / self.largeur as f32).clamp(0.0, 1.0);
        self.force as f32 / 1000.0 * t * t
    }
}

impl Interieur {
    /// L'opacité de l'ombre en `(x, y)` (centre de pixel, écran) : la plus forte
    /// des ombres des bords qui ombrent — chacun avec SON trait — et, dans un coin
    /// arrondi, celle de l'arc du congé.
    #[cfg(test)]
    fn alpha(&self, x: f32, y: f32) -> f32 {
        self.alpha_et_barre(x, y).0
    }

    /// La part du pixel `(x, y)` qui tombe sur l'interface (`0..=1`) : sur un congé
    /// — dans le carré d'un coin arrondi, hors de son arc, lissée au bord —, ou
    /// hors de la zone libre.
    fn sur_l_interface(&self, x: f32, y: f32) -> f32 {
        let r = self.rayon as f32;
        let (g, d, h, b) = (self.gauche as f32, self.droite as f32, self.haut as f32, self.bas as f32);
        if x < g || x > d || y < h || y > b {
            return 1.0;
        }
        let centres = [(g + r, h + r), (d - r, h + r), (g + r, b - r), (d - r, b - r)];
        centres
            .into_iter()
            .enumerate()
            .filter(|&(k, (cx, cy))| {
                let dedans_x = if k % 2 == 0 { x < cx } else { x > cx };
                let dedans_y = if k < 2 { y < cy } else { y > cy };
                self.coins[k] && r > 0.0 && dedans_x && dedans_y
            })
            .map(|(_, (cx, cy))| hors_de_l_arc(x - cx, y - cy, r))
            .fold(0.0, f32::max)
    }

    /// `alpha`, et la barre dont vient cette ombre — celle qui ombre le plus fort,
    /// dont le pixel prend la teinte (voir `couleur_des_barres`).
    fn alpha_et_barre(&self, x: f32, y: f32) -> (f32, Cote) {
        let r = self.rayon as f32;
        let (g, d, h, b) = (self.gauche as f32, self.droite as f32, self.haut as f32, self.bas as f32);
        let [og, od, ob] = self.ombrent;
        // Dans le carré d'un coin arrondi : l'arc.
        let centres = [(g + r, h + r), (d - r, h + r), (g + r, b - r), (d - r, b - r)];
        for (k, (cx, cy)) in centres.into_iter().enumerate() {
            let dedans_x = if k % 2 == 0 { x < cx } else { x > cx };
            let dedans_y = if k < 2 { y < cy } else { y > cy };
            if self.coins[k] && r > 0.0 && dedans_x && dedans_y {
                let a_l_arc = r - ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
                // ⭐ **LE BORD DE L'ARC EST LISSÉ** — DÉFAUT SIGNALÉ (2026-10-18) : « un
                // défaut d'ombrage sur les arrondis intérieurs du bas ». L'ombre s'y
                // arrêtait d'un pixel à l'autre, en escalier, contre un congé que le
                // dock peint lissé — et la teinte des barres rendait la marche criante.
                // `libre` : la part du pixel dans la zone libre.
                let libre = (a_l_arc + 0.5).clamp(0.0, 1.0);
                let laterale_ombre = if k % 2 == 0 { og } else { od };
                // ⭐ **LA COLONNE OMBRE TOUT LE CARRÉ DU COIN** — DÉFAUT SIGNALÉ
                // (2026-10-16) : « les arrondis de la barre du bas semblent avoir un
                // bug visuel ». Seul l'arc ombrait ce carré : avec une ombre du bas
                // plus étroite que celle des colonnes, il tranchait en carré CLAIR
                // dans l'ombre de la colonne. Dedans, la colonne borde la zone libre
                // comme ailleurs ; sur le congé du bas, elle l'ombre comme la barre
                // du bas qu'il prolonge (« les arrondis du bas doivent aussi voir
                // l'effet d'ombrage qui vient des barres latérales »).
                //
                // ⚠️ **PAS LE CONGÉ DU HAUT** — DÉFAUT SIGNALÉ (2026-10-18, capture
                // image13) : l'ombrer le rendait presque noir à côté de sa colonne,
                // « il faudrait qu'il soit au même niveau que les barres latérales ».
                // Il prolonge la colonne, qui ne reçoit pas sa propre ombre.
                let sur_le_conge = k >= 2 && self.sur_le_bas[k - 2];
                let sa_colonne = if k % 2 == 0 { Cote::Gauche } else { Cote::Droite };
                // ⭐ **LES COINS DU BAS ÉPOUSENT L'ARC** — DÉFAUT SIGNALÉ (2026-10-18) :
                // « toujours un défaut avec l'ombre noire et les arrondis du bas ».
                // Colonne et barre du bas s'y prenaient au plus fort des deux : un pli
                // en diagonale, l'ombre faisait un angle au lieu de suivre l'arrondi.
                // L'arc jette maintenant son ombre à pleine force, celle de la colonne
                // là où il lui fait face, celle de la barre du bas en bas
                // (`vers_le_bas`) ; s'y superpose (`superposer`) l'ombre de l'autre
                // bord droit, dosée pour rejoindre exactement celle d'à côté aux
                // limites du carré.
                if k >= 2 {
                    let rayon_ici = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
                    let vers_le_bas = if rayon_ici > 0.0 { ((y - cy) / rayon_ici).clamp(0.0, 1.0) } else { 1.0 };
                    let dans_l_arc = a_l_arc.max(0.0);
                    let depuis_la_colonne = if k % 2 == 0 { x - g } else { d - x };
                    let lat = |v: f32| if laterale_ombre { self.laterales.alpha(v) } else { 0.0 };
                    let dessous = |v: f32| if ob { self.dessous.alpha(v) } else { 0.0 };
                    let (colonne, du_bas) = (
                        lat(dans_l_arc) * (1.0 - vers_le_bas) + lat(depuis_la_colonne) * vers_le_bas,
                        dessous(dans_l_arc) * vers_le_bas + dessous(b - y) * (1.0 - vers_le_bas),
                    );
                    let de_l_arc = lat(dans_l_arc) * (1.0 - vers_le_bas) + dessous(dans_l_arc) * vers_le_bas;
                    // ⛔ **L'AUTRE BORD, À LA DISTANCE DU RAYON** — DÉFAUT SIGNALÉ
                    // (2026-10-31) : un croissant NOIR contre l'arc. Les distances
                    // réelles aux bords (`depuis_la_colonne`, `b − y`) y comptaient
                    // le bord que l'arc REMPLACE : au milieu de l'arc, à 0,29 r de
                    // la colonne ET du bas, deux ombres presque pleines. Aux limites
                    // du carré, l'autre bord est à `r` exactement : c'est la valeur
                    // qui raccorde sans marche, et elle reste douce dedans.
                    let des_bords = lat(r) * vers_le_bas + dessous(r) * (1.0 - vers_le_bas);
                    let conge = if sur_le_conge { lat(depuis_la_colonne) } else { 0.0 };
                    // Le plancher de la colonne (voir plus haut) : la plus forte des
                    // deux ombres des bords droits SEULE, jamais superposée — à traits
                    // égaux elle ne dépasse pas celle de l'arc, donc pas de croissant.
                    let plancher = lat(depuis_la_colonne).max(dessous(b - y));
                    let alpha = superposer(de_l_arc, des_bords).max(plancher) * libre + conge * (1.0 - libre);
                    let barre = if du_bas * libre > colonne * libre + conge * (1.0 - libre) { Cote::Bas } else { sa_colonne };
                    return (alpha, barre);
                }
                let colonne = if laterale_ombre {
                    let ombre = self.laterales.alpha(if k % 2 == 0 { x - g } else { d - x });
                    if sur_le_conge { ombre } else { ombre * libre }
                } else {
                    0.0
                };
                // L'arc d'un coin du bas prolonge la barre du bas, ceux du haut
                // leur colonne (voir `ui::dock::conges`).
                let trait_de_l_arc = match (k >= 2 && ob, laterale_ombre) {
                    (true, _) => Some((self.dessous, Cote::Bas)),
                    (false, true) => Some((self.laterales, sa_colonne)),
                    (false, false) => None,
                };
                // ⭐ **L'ARC DU HAUT S'EFFACE VERS LA BARRE DU HAUT** — même capture :
                // une ombre pleine le long de l'arc suivait aussi le bord de la barre
                // du haut, puis s'arrêtait net au bout du carré du coin (une couture
                // verticale). L'ombre de l'arc y est celle de la COLONNE : pleine
                // face à elle, nulle là où l'arc rejoint la barre du haut.
                let poids = if k < 2 {
                    let rayon_ici = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
                    if rayon_ici > 0.0 { (x - cx).abs() / rayon_ici } else { 0.0 }
                } else {
                    1.0
                };
                let (arc, barre_de_l_arc) =
                    trait_de_l_arc.map_or((0.0, sa_colonne), |(t, c)| (t.alpha(a_l_arc.max(0.0)) * poids * libre, c));
                return if arc > colonne { (arc, barre_de_l_arc) } else { (colonne, sa_colonne) };
            }
        }
        // Hors des coins, les ombres se superposent aussi — sans quoi la limite du
        // carré d'un coin du bas ferait une marche.
        let (total, (_, barre)) = [(og, self.laterales, x - g, Cote::Gauche), (od, self.laterales, d - x, Cote::Droite), (ob, self.dessous, b - y, Cote::Bas)]
            .into_iter()
            .filter(|(o, _, _, _)| *o)
            .map(|(_, t, v, c)| (t.alpha(v), c))
            .fold((0.0, (0.0, Cote::Bas)), |(total, plus), ici| (superposer(total, ici.0), if ici.0 > plus.0 { ici } else { plus }));
        (total, barre)

    }
}

/// Deux ombres l'une sur l'autre : chaque couche noire laisse passer `1 − a` de
/// ce qu'elle couvre — comme l'ombre du haut, sa propre fenêtre, sur celles des
/// colonnes.
fn superposer(a: f32, b: f32) -> f32 {
    let (a, b) = (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0));
    // Exact quand l'une est nulle : une seule ombre garde sa valeur.
    if a == 0.0 || b == 0.0 { a.max(b) } else { 1.0 - (1.0 - a) * (1.0 - b) }
}

/// Les réglages de l'ombre des barres de gauche, de droite et du bas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReglagesCotes {
    pub active: bool,
    /// En pixels, comme la hauteur de l'ombre du haut.
    pub largeur: f32,
    /// `0..=1`, comme celle du haut.
    pub force: f32,
    pub gauche: bool,
    pub droite: bool,
    pub bas: bool,
    /// Les latérales ombrent AUSSI la barre du bas, entre elles (voir
    /// `ombres_des_cotes`).
    pub sur_le_bas: bool,
    /// ⭐ L'ombre qui part de la barre du bas, réglable À PART — DEMANDE EXPLICITE
    /// (2026-10-16) : « je voudrais pouvoir paramétrer l'effet d'ombrage qui part de
    /// la barre du bas individuellement ». `largeur` et `force` restent celles des
    /// colonnes.
    pub largeur_bas: f32,
    pub force_bas: f32,
}

impl ReglagesCotes {
    /// Éteinte ; la largeur et la force par défaut de l'ombre du haut, les trois
    /// barres cochées.
    pub const DEFAUT: Self = Self {
        active: false,
        largeur: crate::ui::dock::DEFAULT_OMBRE_BARRE_HAUT,
        force: crate::ui::dock::DEFAULT_OMBRE_BARRE_HAUT_FORCE,
        gauche: true,
        droite: true,
        bas: true,
        sur_le_bas: true,
        largeur_bas: crate::ui::dock::DEFAULT_OMBRE_BARRE_HAUT,
        force_bas: crate::ui::dock::DEFAULT_OMBRE_BARRE_HAUT_FORCE,
    };

    /// Les largeurs bornées à `largeur_max` (celui de leur ombre, noire ou
    /// colorée) ; un `NaN` garde la valeur par défaut.
    fn bornes(self, largeur_max: f32) -> Self {
        let borne = |v: f32, max: f32, defaut: f32| if v.is_finite() { v.clamp(0.0, max) } else { defaut };
        Self {
            largeur: borne(self.largeur, largeur_max, Self::DEFAUT.largeur),
            force: borne(self.force, 1.0, Self::DEFAUT.force),
            largeur_bas: borne(self.largeur_bas, largeur_max, Self::DEFAUT.largeur_bas),
            force_bas: borne(self.force_bas, 1.0, Self::DEFAUT.force_bas),
            ..self
        }
    }

    /// « active,largeur,force,gauche,droite,bas,sur_le_bas,largeur_bas,force_bas »,
    /// les cases en 0/1 — des virgules : un seul écrivain.
    pub fn serialiser(&self) -> String {
        let b = |v: bool| if v { "1" } else { "0" };
        let (a, g, d, bas, sur) = (b(self.active), b(self.gauche), b(self.droite), b(self.bas), b(self.sur_le_bas));
        format!("{a},{},{},{g},{d},{bas},{sur},{},{}", self.largeur, self.force, self.largeur_bas, self.force_bas)
    }

    /// Relit `serialiser` (ceux de l'ombre noire) ; un champ absent ou illisible
    /// garde sa valeur par défaut.
    pub fn charger(contenu: &str) -> Self {
        Self::lire(contenu).bornes(crate::ui::dock::MAX_OMBRE_BARRE_HAUT)
    }

    /// `charger` sans borne : la colorée borne les siens plus loin.
    fn lire(contenu: &str) -> Self {
        let champs: Vec<Option<f32>> = contenu.trim().split(',').map(|c| c.trim().parse::<f32>().ok()).collect();
        let champ = |i: usize| champs.get(i).copied().flatten();
        let case = |i: usize, defaut: bool| champ(i).map_or(defaut, |v| v >= 0.5);
        let d = Self::DEFAUT;
        let (largeur, force) = (champ(1).unwrap_or(d.largeur), champ(2).unwrap_or(d.force));
        Self {
            active: case(0, d.active),
            largeur,
            force,
            gauche: case(3, d.gauche),
            droite: case(4, d.droite),
            bas: case(5, d.bas),
            sur_le_bas: case(6, d.sur_le_bas),
            // Un fichier d'avant le réglage séparé : l'ombre du bas gardait celle
            // des colonnes.
            largeur_bas: champ(7).unwrap_or(largeur),
            force_bas: champ(8).unwrap_or(force),
        }
    }
}

static COTES: std::sync::Mutex<ReglagesCotes> = std::sync::Mutex::new(ReglagesCotes::DEFAUT);

pub fn reglages_cotes() -> ReglagesCotes {
    *COTES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Change les réglages (bornés), et rend ceux retenus — à enregistrer.
pub fn modifier_cotes(modifier: impl FnOnce(&mut ReglagesCotes)) -> ReglagesCotes {
    let mut reglages = COTES.lock().unwrap_or_else(|e| e.into_inner());
    modifier(&mut reglages);
    *reglages = reglages.bornes(crate::ui::dock::MAX_OMBRE_BARRE_HAUT);
    *reglages
}

/// ⭐ **LA LARGEUR D'UNE OMBRE COLORÉE VA JUSQU'À 2000 PX** — DEMANDE EXPLICITE
/// (2026-10-20) : « la largeur des ombres colorées devrait aller jusqu'à 2000 par
/// palier de 10 ». La noire garde `ui::dock::MAX_OMBRE_BARRE_HAUT`.
pub const MAX_LARGEUR_COULEUR: f32 = 2000.0;

/// Les réglages de l'ombre COLORÉE (voir la doc de module) : son ombre du haut
/// et celles des autres barres, comme la noire. L'option et l'intensité restent
/// `couleur_des_barres` et `INTENSITE_COULEUR`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReglagesCouleur {
    /// En pixels ; 0 : pas d'ombre colorée sous la barre du haut.
    pub largeur_haut: f32,
    /// `0..=1`.
    pub force_haut: f32,
    pub cotes: ReglagesCotes,
}

impl ReglagesCouleur {
    pub const DEFAUT: Self = Self {
        largeur_haut: crate::ui::dock::DEFAULT_OMBRE_BARRE_HAUT,
        force_haut: crate::ui::dock::DEFAULT_OMBRE_BARRE_HAUT_FORCE,
        cotes: ReglagesCotes::DEFAUT,
    };

    /// Ceux de l'ombre noire : sans fichier (avant les réglages séparés), l'ombre
    /// colorée garde la forme qu'elle partageait avec elle.
    pub fn de_la_noire() -> Self {
        Self { largeur_haut: crate::ui::dock::ombre_barre_haut(), force_haut: crate::ui::dock::ombre_barre_haut_force(), cotes: reglages_cotes() }
    }

    fn bornes(self) -> Self {
        let borne = |v: f32, max: f32, defaut: f32| if v.is_finite() { v.clamp(0.0, max) } else { defaut };
        Self {
            largeur_haut: borne(self.largeur_haut, MAX_LARGEUR_COULEUR, Self::DEFAUT.largeur_haut),
            force_haut: borne(self.force_haut, 1.0, Self::DEFAUT.force_haut),
            cotes: self.cotes.bornes(MAX_LARGEUR_COULEUR),
        }
    }

    /// « largeur_haut,force_haut;<`ReglagesCotes::serialiser`> ».
    pub fn serialiser(&self) -> String {
        format!("{},{};{}", self.largeur_haut, self.force_haut, self.cotes.serialiser())
    }

    /// Relit `serialiser` ; un champ absent ou illisible garde sa valeur par défaut.
    pub fn charger(contenu: &str) -> Self {
        let (haut, cotes) = contenu.trim().split_once(';').unwrap_or((contenu, ""));
        let champs: Vec<Option<f32>> = haut.split(',').map(|c| c.trim().parse::<f32>().ok()).collect();
        let champ = |i: usize, defaut: f32| champs.get(i).copied().flatten().unwrap_or(defaut);
        Self {
            largeur_haut: champ(0, Self::DEFAUT.largeur_haut),
            force_haut: champ(1, Self::DEFAUT.force_haut),
            cotes: ReglagesCotes::lire(cotes),
        }
        .bornes()
    }
}

static COULEUR: std::sync::Mutex<ReglagesCouleur> = std::sync::Mutex::new(ReglagesCouleur::DEFAUT);

pub fn reglages_couleur() -> ReglagesCouleur {
    *COULEUR.lock().unwrap_or_else(|e| e.into_inner())
}

/// Change les réglages de l'ombre colorée (bornés), et rend ceux retenus — à
/// enregistrer.
pub fn modifier_couleur(modifier: impl FnOnce(&mut ReglagesCouleur)) -> ReglagesCouleur {
    let mut reglages = COULEUR.lock().unwrap_or_else(|e| e.into_inner());
    modifier(&mut reglages);
    *reglages = reglages.bornes();
    *reglages
}

thread_local! {
    /// Les barres déclarées, par fenêtre de dock.
    static BARRES: RefCell<Vec<(HWND, Barre)>> = const { RefCell::new(Vec::new()) };
    /// Les ombres posées, leurs fenêtres, l'affinité de capture et la dernière remontée.
    static POSEES: RefCell<(Vec<Ombre>, Vec<HWND>, bool, Option<Instant>)> = const { RefCell::new((Vec::new(), Vec::new(), false, None)) };
}

/// Déclare les barres de la fenêtre de dock `hwnd` — à appeler à chaque rendu de
/// dock, avec le coin de la fenêtre à l'écran, les épaisseurs VISIBLES `[haut,
/// gauche, droite, bas]` (une barre masquée n'ombre pas, et l'ombre du bas court
/// alors sous elle) et le rayon des congés.
///
/// `couleurs` : la couleur de chaque barre `[haut, gauche, droite, bas]` (voir
/// `couleurs_d_ombre`), que l'ombre prend avec `couleur_des_barres` — en couleur
/// affichée, tant que `gfx::releve_barres` n'a pas relu la couleur RENDUE.
/// `ecran` : un écran secondaire réglé à part (`reglages_ecran`), `None` sinon.
pub fn declarer(
    hwnd: HWND,
    origine: (i32, i32),
    layout: &DockLayout,
    [_, gauche, droite, dessous]: [f32; 4],
    rayon: f32,
    couleurs: [[f32; 3]; 4],
    ecran: Option<ReglagesEcran>,
) {
    let releves = crate::gfx::releve_barres::profils(hwnd.0 as isize);
    let mut barre = Barre {
        origine,
        largeur: layout.total_width,
        hauteur: layout.total_height,
        bas: layout.haut_colonnes,
        gauche: gauche.min(layout.left_width),
        droite: droite.min(layout.right_width),
        dessous: dessous.min(layout.bottom_thickness),
        rayon,
        teintes: std::array::from_fn(|i| profil_de(releves[i], couleurs[i])),
        teintes_zone: std::array::from_fn(|i| profil_de_zone(releves[i], couleurs[i])),
        texture: empreinte_texture(),
        ecran,
    };
    BARRES.with(|b| {
        let Ok(mut barres) = b.try_borrow_mut() else { return };
        match barres.iter_mut().find(|(h, _)| *h == hwnd) {
            Some(entree) => {
                // Un profil à peine différent garde l'ancien : sinon chaque relevé
                // repeindrait les ombres pour un écart invisible.
                let anciens = entree.1.teintes.iter().chain(&entree.1.teintes_zone);
                for (nouveau, ancien) in barre.teintes.iter_mut().chain(barre.teintes_zone.iter_mut()).zip(anciens) {
                    if proches(nouveau, ancien) {
                        *nouveau = *ancien;
                    }
                }
                entree.1 = barre;
            }
            None => barres.push((hwnd, barre)),
        }
    });
}

/// L'ombre sous `barre`, à `hauteur` pixels et `force` (0..=1). `None` : pas de
/// barre du haut, ou ombre éteinte.
fn ombre_de(barre: &Barre, hauteur: f32, force: f32) -> Option<Ombre> {
    let (hauteur, largeur) = (hauteur.round() as i32, barre.largeur.round() as i32);
    (barre.bas > 0.0 && hauteur > 0 && largeur > 0 && force > 0.0).then(|| Ombre {
        x: barre.origine.0,
        y: barre.origine.1 + barre.bas.round() as i32,
        largeur,
        hauteur,
        force: (force.clamp(0.0, 1.0) * 1000.0).round() as u16,
        cote: Cote::Haut,
        interieur: None,
        teintes: barre.teintes,
        etendues: barre.etendues(),
        couche: Couche::NOIRE,
        texture: barre.texture,
        colonnes: [
            (barre.gauche > 0.0).then(|| barre.origine.0 + barre.gauche.round() as i32),
            (barre.droite > 0.0).then(|| barre.origine.0 + (barre.largeur - barre.droite).round() as i32),
        ],
        rayon: barre.rayon.round() as i32,
    })
}

/// Les ombres des barres de gauche, de droite et du bas, vers l'intérieur de la
/// zone libre — et JAMAIS sur une barre visible (voir `Interieur`). Celle du bas
/// va d'une colonne à l'autre (au bord de l'écran sous une colonne masquée) et
/// prend les coins ; les latérales vont de la barre du haut à elle.
fn ombres_des_cotes(barre: &Barre, r: &ReglagesCotes) -> Vec<Ombre> {
    let (laterales, dessous) = (Trait::de(r.largeur, r.force), Trait::de(r.largeur_bas, r.force_bas));
    let eteint = |t: Trait| t.largeur <= 0 || t.force == 0;
    if !r.active {
        return Vec::new();
    }
    let l = laterales.largeur;
    let (ox, oy) = barre.origine;
    let px = |v: f32| v.round() as i32;
    let (gauche, droite) = (ox + px(barre.gauche), ox + px(barre.largeur - barre.droite));
    let (haut, bas) = (oy + px(barre.bas), oy + px(barre.hauteur - barre.dessous));
    if droite - gauche <= 0 || bas - haut <= 0 {
        return Vec::new();
    }
    // Les mêmes congés que ceux que peint le dock (`ui::dock::conges`) : un coin
    // existe s'il a une de ses deux barres, et son rayon est borné à la zone.
    let rayon = barre.rayon.min((droite - gauche) as f32 / 2.0).min((bas - haut) as f32 / 2.0);
    let rayon = if rayon >= 1.0 { px(rayon) } else { 0 };
    let (a_haut, a_gauche, a_droite, a_bas) = (barre.bas > 0.0, barre.gauche > 0.0, barre.droite > 0.0, barre.dessous > 0.0);
    let ombrent = [r.gauche && a_gauche && !eteint(laterales), r.droite && a_droite && !eteint(laterales), r.bas && a_bas && !eteint(dessous)];
    let interieur = Interieur {
        gauche,
        droite,
        haut,
        bas,
        rayon,
        ombrent,
        coins: [a_haut || a_gauche, a_haut || a_droite, a_bas || a_gauche, a_bas || a_droite],
        sur_le_bas: [r.sur_le_bas && a_bas && ombrent[0], r.sur_le_bas && a_bas && ombrent[1]],
        laterales,
        dessous,
    };
    let mut ombres = Vec::new();
    // Les bandes couvrent AUSSI le carré de leurs congés : avec deux largeurs
    // différentes, l'ombre d'un arc peut dépasser celle d'un bord droit. Les
    // latérales s'arrêtent où commence celle du bas.
    let hauteur_bas = dessous.largeur.max(rayon).min(bas - haut);
    let fin_laterales = if ombrent[2] { bas - hauteur_bas } else { bas };
    let force = laterales.force.max(dessous.force);
    let (teintes, etendues) = (barre.teintes, barre.etendues());
    let ombre = |x: i32, y: i32, largeur: i32, hauteur: i32, cote: Cote| Ombre {
        x,
        y,
        largeur,
        hauteur,
        force,
        cote,
        interieur: Some(interieur),
        teintes,
        etendues,
        couche: Couche::NOIRE,
        texture: barre.texture,
        colonnes: [None; 2],
        rayon,
    };
    let largeur_laterale = l.max(rayon).min(droite - gauche);
    if ombrent[0] && fin_laterales > haut {
        ombres.push(ombre(gauche, haut, largeur_laterale, fin_laterales - haut, Cote::Gauche));
    }
    if ombrent[1] && fin_laterales > haut {
        ombres.push(ombre(droite - largeur_laterale, haut, largeur_laterale, fin_laterales - haut, Cote::Droite));
    }
    if ombrent[2] {
        ombres.push(ombre(gauche, bas - hauteur_bas, droite - gauche, hauteur_bas, Cote::Bas));
    }
    let (force, largeur_laterale) = (laterales.force, l.min(droite - gauche));
    // ⭐ **LES LATÉRALES OMBRENT LA BARRE DU BAS** — DEMANDE EXPLICITE (2026-10-16) :
    // « un effet d'ombre qui parte des barres latérales pour aller sur la barre du
    // bas si elle est visible ». La barre du bas vit ENTRE les colonnes (voir
    // `ui::dock::bar_rects`) : chaque colonne visible y jette sa bande jusqu'au bas
    // de l'écran, une simple bande — aucun congé n'y tombe.
    let pied = oy + px(barre.hauteur);
    if r.sur_le_bas && a_bas && pied > bas {
        // Sur la barre du bas, contre une colonne : une jonction sur toute sa
        // longueur, la teinte y est pleine.
        let simple = |x: i32, cote: Cote| Ombre {
            x,
            y: bas,
            largeur: largeur_laterale,
            hauteur: pied - bas,
            force,
            cote,
            interieur: None,
            teintes,
            etendues,
            couche: Couche::NOIRE,
            texture: barre.texture,
                colonnes: [None; 2],
            rayon: 0,
        };
        if ombrent[0] {
            ombres.push(simple(gauche, Cote::Gauche));
        }
        if ombrent[1] {
            ombres.push(simple(droite - largeur_laterale, Cote::Droite));
        }
    }
    ombres
}

/// `barre` dont la barre du haut a la couleur de son centre (voir
/// `centre_haut::Bloc::teinter`) : le profil de son ombre colorée, et celui de
/// la noire teintée sur la zone (`zone_en_couleur`), qui suit la même couleur.
/// Le calque, lui, se compose avec la barre telle que déclarée.
fn avec_le_centre(barre: &Barre, centre: &crate::win32::centre_haut::ReglagesCentre) -> Barre {
    let Some(bloc) = centre.bloc(barre.largeur.round() as i32, barre.bas.round() as i32) else { return *barre };
    let mut teinte = *barre;
    teinte.teintes[Cote::Haut.rang()] = bloc.teinter(&barre.teintes[Cote::Haut.rang()]);
    teinte.teintes_zone[Cote::Haut.rang()] = bloc.teinter(&barre.teintes_zone[Cote::Haut.rang()]);
    teinte
}

/// Les ombres d'une fenêtre de dock, couche par couche dans l'ordre de `couches`
/// — la colorée d'abord : la noire, composée après, passe au-dessus (voir
/// `composer`). Une couche à 0 % n'a pas d'ombre.
fn ombres_de_la_barre(barre: &Barre, couches: &[Reglages; 2]) -> Vec<Ombre> {
    let mut couches = *couches;
    if let Some(e) = barre.ecran {
        let [coloree, noire] = &mut couches;
        coloree.haut = e.couleur;
        noire.haut = e.noire;
        noire.couche.portee.zone[0] = e.zone;
    }
    let ombres: Vec<Ombre> = couches
        .iter()
        .filter(|r| r.couche.intensite > 0)
        .flat_map(|r| {
            let [hauteur, force] = r.haut;
            ombre_de(barre, hauteur, force).into_iter().chain(ombres_des_cotes(barre, &r.cotes)).map(move |o| o.en(r.couche))
        })
        .collect();
    // La noire teintée sur la zone libre (`zone_en_couleur`) : `en` lui a retiré
    // ses teintes, elle reçoit celles de la zone — sous les colorées des AUTRES
    // barres qui couvrent la sienne (`teintes_zone_sous_la_couleur`).
    if !ombres.iter().any(|o| o.couche.teintee_zone) {
        return ombres;
    }
    let colorees: Vec<Ombre> = ombres.iter().filter(|o| o.couche.coloree).copied().collect();
    let teintes = teintes_zone_sous_la_couleur(barre, &colorees);
    ombres.into_iter().map(|o| if o.couche.teintee_zone { Ombre { teintes, ..o } } else { o }).collect()
}

/// ⭐ **LA ZONE PREND LA COULEUR QUE MONTRE LE BORD DE LA BARRE** — DEMANDE
/// EXPLICITE (2026-10-23) : « il faut que les ombres de couleur centrales
/// appliquent la couleur des ombres de couleur qui chevauchent les barres
/// sous-jacentes des autres barres ». MESURÉ (capture 4K, colonne gauche, sous la
/// barre du haut) : le bord de la colonne montrait (193, 113, 16), l'ORANGE de
/// l'ombre colorée du haut posée sur elle ; la zone, 10 px plus loin, (103, 41,
/// 107) — le violet de la colonne seule. Une rupture nette au bord.
///
/// Chaque segment du profil de zone est donc recomposé au BORD INTÉRIEUR de sa
/// barre : le profil de zone, puis les colorées des AUTRES barres qui tombent sur
/// ce point (leur part sur l'interface, `pixel_de`), dans l'ordre où le calque
/// les pose. La colorée d'une barre ne compte pas pour elle-même : elle part de
/// son bord vers la zone, elle n'est pas SOUS lui.
fn teintes_zone_sous_la_couleur(barre: &Barre, colorees: &[Ombre]) -> [Profil; 4] {
    let etendues = barre.etendues();
    let (ox, oy) = (barre.origine.0 as f32, barre.origine.1 as f32);
    // Le centre du pixel de la barre contre la zone, à `le_long` (écran).
    let bord = |cote: Cote, le_long: f32| -> Option<(f32, f32)> {
        match cote {
            Cote::Haut => (barre.bas > 0.0).then(|| (le_long, oy + barre.bas - 0.5)),
            Cote::Gauche => (barre.gauche > 0.0).then(|| (ox + barre.gauche - 0.5, le_long)),
            Cote::Droite => (barre.droite > 0.0).then(|| (ox + barre.largeur - barre.droite + 0.5, le_long)),
            Cote::Bas => (barre.dessous > 0.0).then(|| (le_long, oy + barre.hauteur - barre.dessous + 0.5)),
        }
    };
    std::array::from_fn(|i| {
        let cote = [Cote::Haut, Cote::Gauche, Cote::Droite, Cote::Bas][i];
        let [debut, longueur] = etendues[i];
        let mut profil = barre.teintes_zone[i];
        for (s, teinte) in profil.iter_mut().enumerate() {
            let le_long = debut as f32 + (s as f32 + 0.5) / SEGMENTS as f32 * longueur as f32;
            let Some((sx, sy)) = bord(cote, le_long) else { continue };
            let mut c = teinte.map(|v| v as f32);
            for o in colorees.iter().filter(|o| o.cote != cote) {
                let (x, y) = ((sx - o.x as f32).floor() as i32, (sy - o.y as f32).floor() as i32);
                if x < 0 || y < 0 || x >= o.largeur || y >= o.hauteur {
                    continue;
                }
                // BGRA prémultiplié : la colorée par-dessus ce qui est dessous.
                let [b, g, r, a] = pixel_de(o, Portion::Interface, x, y);
                let reste = 1.0 - a as f32 / 255.0;
                c = [r as f32 + c[0] * reste, g as f32 + c[1] * reste, b as f32 + c[2] * reste];
            }
            *teinte = c.map(|v| v.round().clamp(0.0, 255.0) as u8);
        }
        profil
    })
}

/// L'ombre a-t-elle une fenêtre (sa part sur la zone libre) ? La noire
/// seulement, et si sa portée la laisse tomber sur la zone centrale (voir
/// `Portee`) — celle du haut par `zone[0]`, celles des côtés, qui se partagent
/// les coins, par l'une des trois autres ; et pas une simple bande sur la barre
/// du bas, toute sur l'interface.
fn a_une_fenetre(ombre: &Ombre) -> bool {
    let zone = ombre.couche.portee.zone;
    !ombre.couche.coloree
        && match ombre.interieur {
            Some(_) => zone[1..].iter().any(|&z| z),
            None => ombre.cote == Cote::Haut && zone[0],
        }
}

/// Le calque des ombres d'une fenêtre de dock : leur part sur l'interface,
/// composée en BGRA PRÉMULTIPLIÉ à la taille de la fenêtre, et la texture où
/// son shader la lit (voir la doc de module).
struct Calque {
    ombres: Vec<Ombre>,
    /// Le centre de la barre du haut (`centre_haut`), peint SOUS les ombres, et
    /// l'empreinte de la texture qu'il a reçue (`empreinte_de_la_texture`).
    centre: Option<(crate::win32::centre_haut::Bloc, u64)>,
    origine: (i32, i32),
    taille: (i32, i32),
    pixels: Vec<u8>,
    vue: Option<(ID3D11Texture2D, ID3D11ShaderResourceView)>,
}

impl Calque {
    /// Ni ombre ni centre : rien à poser sur les fonds.
    fn est_vide(&self) -> bool {
        self.ombres.is_empty() && self.centre.is_none()
    }
}

thread_local! {
    /// Un calque par fenêtre de dock (la clé : son `HWND`).
    static CALQUES: RefCell<HashMap<isize, Calque>> = RefCell::new(HashMap::new());
}

/// Le calque des ombres de la fenêtre de dock `hwnd`, à poser sur ses fonds de
/// barre (`QuadRenderer::poser_ombres`) ; `None` : aucune ombre.
pub fn vue_des_ombres(hwnd: HWND) -> Option<ID3D11ShaderResourceView> {
    CALQUES.with(|c| {
        let calques = c.try_borrow().ok()?;
        let calque = calques.get(&(hwnd.0 as isize))?;
        if calque.est_vide() { None } else { calque.vue.as_ref().map(|(_, vue)| vue.clone()) }
    })
}

/// Le pixel `(x, y)` (de la fenêtre de dock `cle`) est-il sous une ombre ?
/// `gfx::releve_barres` l'écarte : la couleur d'une barre se relit HORS de
/// l'ombre que le dock y peint, sans quoi chaque relevé l'assombrirait.
pub fn ombree(cle: isize, x: u32, y: u32) -> bool {
    CALQUES.with(|c| {
        let Ok(calques) = c.try_borrow() else { return false };
        let Some(calque) = calques.get(&cle).filter(|c| !c.est_vide()) else { return false };
        let (l, h) = calque.taille;
        (x as i32) < l && (y as i32) < h && calque.pixels.get(((y as i32 * l + x as i32) * 4 + 3) as usize).is_some_and(|&a| a > 2)
    })
}

/// Le rectangle `[x0, y0, x1, y1]` d'une ombre dans la fenêtre d'origine
/// `origine` et de taille `taille`, borné à elle ; `None` : hors de la fenêtre.
fn rect_dans(ombre: &Ombre, origine: (i32, i32), (l, h): (i32, i32)) -> Option<[i32; 4]> {
    let (x0, y0) = ((ombre.x - origine.0).max(0), (ombre.y - origine.1).max(0));
    let (x1, y1) = ((ombre.x - origine.0 + ombre.largeur).min(l), (ombre.y - origine.1 + ombre.hauteur).min(h));
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1, y1])
}

/// Compose la part sur l'interface de `ombres`, DANS L'ORDRE (chacune par-dessus
/// les précédentes : la noire sur la colorée), dans `pixels` (BGRA prémultiplié,
/// fenêtre de taille `taille` à `origine`). Rend les rectangles touchés.
fn composer(pixels: &mut [u8], origine: (i32, i32), taille: (i32, i32), ombres: &[Ombre]) -> Vec<[i32; 4]> {
    let mut touches = Vec::new();
    for ombre in ombres {
        let Some([x0, y0, x1, y1]) = rect_dans(ombre, origine, taille) else { continue };
        let source = pixels_de(ombre, Portion::Interface);
        let (dx, dy) = (ombre.x - origine.0, ombre.y - origine.1);
        for y in y0..y1 {
            for x in x0..x1 {
                let s = (((y - dy) * ombre.largeur + (x - dx)) * 4) as usize;
                let d = ((y * taille.0 + x) * 4) as usize;
                let reste = 255 - source[s + 3] as u32;
                for k in 0..4 {
                    pixels[d + k] = (source[s + k] as u32 + (pixels[d + k] as u32 * reste + 127) / 255).min(255) as u8;
                }
            }
        }
        touches.push([x0, y0, x1, y1]);
    }
    touches
}

/// Met le calque de chaque fenêtre de dock en accord avec ses ombres : recomposé
/// seulement quand elles changent, et renvoyé au GPU par rectangles (ceux des
/// anciennes ombres, effacés, et ceux des nouvelles).
fn poser_les_calques(par_dock: &[(HWND, Barre, Vec<Ombre>)]) {
    let centre = crate::win32::centre_haut::reglages();
    CALQUES.with(|c| {
        let Ok(mut calques) = c.try_borrow_mut() else { return };
        calques.retain(|cle, _| par_dock.iter().any(|(h, _, _)| h.0 as isize == *cle));
        for (hwnd, barre, ombres) in par_dock {
            let taille = (barre.largeur.round() as i32, barre.hauteur.round() as i32);
            let bloc = centre.bloc(taille.0, barre.bas.round() as i32).map(|b| (b, empreinte_de_la_texture()));
            let calque = calques
                .entry(hwnd.0 as isize)
                .or_insert_with(|| Calque { ombres: Vec::new(), centre: None, origine: barre.origine, taille: (0, 0), pixels: Vec::new(), vue: None });
            let inchange = calque.ombres == *ombres && calque.centre == bloc && calque.origine == barre.origine && calque.taille == taille;
            // Rien à poser, et rien de posé.
            if inchange || taille.0 <= 0 || taille.1 <= 0 || (ombres.is_empty() && bloc.is_none() && calque.vue.is_none()) {
                continue;
            }
            let neuf = calque.taille != taille || calque.vue.is_none();
            let mut rects = Vec::new();
            if neuf {
                calque.pixels = vec![0; (taille.0 * taille.1 * 4) as usize];
                calque.taille = taille;
            } else {
                for o in &calque.ombres {
                    if let Some([x0, y0, x1, y1]) = rect_dans(o, calque.origine, taille) {
                        for y in y0..y1 {
                            calque.pixels[((y * taille.0 + x0) * 4) as usize..((y * taille.0 + x1) * 4) as usize].fill(0);
                        }
                        rects.push([x0, y0, x1, y1]);
                    }
                }
                // L'ancien centre s'efface avec la bande qu'il couvrait.
                if let Some([x0, y0, x1, y1]) = calque.centre.map(|(b, _)| b.rect()) {
                    let x1 = x1.min(taille.0);
                    for y in y0..y1.min(taille.1) {
                        calque.pixels[((y * taille.0 + x0) * 4) as usize..((y * taille.0 + x1) * 4) as usize].fill(0);
                    }
                    rects.push([x0, y0, x1, y1.min(taille.1)]);
                }
            }
            // ⛔ Le centre est REPEINT à chaque changement, ombres comprises : les
            // ombres se composent PAR-DESSUS ce qui est là, et l'effacement des
            // anciennes a pu en entamer une partie.
            if let Some((b, empreinte)) = bloc {
                let [_, _, l, h] = b.rect();
                let texture = if empreinte != 0 { calque_de_texture(barre.origine, (l, h)) } else { None };
                b.peindre(&mut calque.pixels, taille, texture.as_deref());
                let [x0, y0, x1, y1] = b.rect();
                rects.push([x0, y0, x1.min(taille.0), y1.min(taille.1)]);
            }
            rects.extend(composer(&mut calque.pixels, barre.origine, taille, ombres));
            calque.centre = bloc;
            calque.ombres = ombres.clone();
            calque.origine = barre.origine;
            envoyer(calque, &rects, neuf);
        }
    });
}

/// Renvoie le calque au GPU : la texture entière quand elle est `neuve` (créée
/// à sa taille), sinon les rectangles `rects`.
fn envoyer(calque: &mut Calque, rects: &[[i32; 4]], neuve: bool) {
    use windows::Win32::Graphics::Direct3D11::{D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT};
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
    let (l, h) = calque.taille;
    GPU.with(|g| {
        let Ok(g) = g.try_borrow() else { return };
        let Some(gpu) = g.as_ref() else { return };
        if neuve {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: l as u32,
                Height: h as u32,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                ..Default::default()
            };
            let donnees = D3D11_SUBRESOURCE_DATA { pSysMem: calque.pixels.as_ptr().cast(), SysMemPitch: (l * 4) as u32, SysMemSlicePitch: 0 };
            let mut texture = None;
            let mut vue = None;
            let creee = unsafe { gpu.device.CreateTexture2D(&desc, Some(&donnees), Some(&mut texture)) };
            calque.vue = match (creee, texture) {
                (Ok(()), Some(texture)) => unsafe { gpu.device.CreateShaderResourceView(&texture, None, Some(&mut vue)) }.ok().and(vue).map(|v| (texture, v)),
                _ => None,
            };
            if calque.vue.is_none() {
                crate::diag!("[ombre_barre] calque des ombres {l}×{h} refusé par le GPU");
            }
            return;
        }
        let Some((texture, _)) = &calque.vue else { return };
        for &[x0, y0, x1, y1] in rects {
            let zone = D3D11_BOX { left: x0 as u32, top: y0 as u32, front: 0, right: x1 as u32, bottom: y1 as u32, back: 1 };
            let debut = ((y0 * l + x0) * 4) as usize;
            unsafe { gpu.ctx.UpdateSubresource(texture, 0, Some(&zone), calque.pixels[debut..].as_ptr().cast(), (l * 4) as u32, 0) };
        }
    });
}

/// Met les fenêtres en accord avec les barres déclarées et les réglages. À
/// appeler à chaque tour de boucle : sans changement, il ne fait que remonter
/// les fenêtres, au plus toutes les `REMONTEE`.
pub fn synchroniser() {
    let couches = reglages_en_vigueur();
    let centre = crate::win32::centre_haut::reglages();
    let par_dock: Vec<(HWND, Barre, Vec<Ombre>)> = BARRES.with(|b| {
        let Ok(mut barres) = b.try_borrow_mut() else { return Vec::new() };
        // Une fenêtre détruite (écran débranché) ou masquée n'a pas d'ombre.
        barres.retain(|(h, _)| unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(*h)) }.as_bool());
        barres
            .iter()
            .filter(|(h, _)| unsafe { IsWindowVisible(*h) }.as_bool())
            .map(|(h, barre)| (*h, *barre, ombres_de_la_barre(&avec_le_centre(barre, &centre), &couches)))
            .collect()
    });
    poser_les_calques(&par_dock);
    let voulues: Vec<Ombre> = par_dock.iter().flat_map(|(_, _, ombres)| ombres.iter().copied().filter(a_une_fenetre)).collect();
    let capturables = crate::gfx::capture_visibility::is_enabled();
    POSEES.with(|p| {
        let Ok(mut posees) = p.try_borrow_mut() else { return };
        if posees.0 == voulues && posees.2 == capturables {
            if posees.3.is_none_or(|t| t.elapsed() >= REMONTEE) {
                posees.3 = Some(Instant::now());
                drop(posees);
                remonter();
            }
            return;
        }
        // Autant de fenêtres que d'ombres : on garde les existantes (repeintes,
        // donc déplacées), on crée ou détruit l'écart.
        while posees.1.len() > voulues.len() {
            if let Some(hwnd) = posees.1.pop() {
                let _ = unsafe { DestroyWindow(hwnd) };
            }
        }
        while posees.1.len() < voulues.len() {
            match creer() {
                Some(hwnd) => posees.1.push(hwnd),
                None => break,
            }
        }
        let affinite = crate::gfx::capture_visibility::window_display_affinity();
        for (hwnd, ombre) in posees.1.iter().zip(&voulues) {
            let _ = unsafe { SetWindowDisplayAffinity(*hwnd, affinite) };
            peindre(*hwnd, ombre);
            let _ = unsafe { ShowWindow(*hwnd, SW_SHOWNOACTIVATE) };
        }
        posees.0 = voulues;
        posees.2 = capturables;
        posees.3 = Some(Instant::now());
        drop(posees);
        remonter();
    });
}

/// Repasse les ombres en tête des fenêtres topmost, puis les MENUS visibles
/// au-dessus d'elles — appelé après chaque remontée du dock
/// (`Window::raise_to_top`) et par `synchroniser`.
///
/// ⭐ **ET PENDANT UNE MAGNIFICATION, L'ORDRE S'INVERSE** — le dock passe
/// DEVANT les ombres et les menus (`rangement_magnifie`), pour que l'icône
/// agrandie ne soit recouverte par rien. Voir `ui::dock::magnification_en_cours`.
///
/// ⭐ **LES MENUS, ET RIEN QU'EUX** — DEMANDE EXPLICITE (2026-10-23) : « je ne
/// veux que les menus au-dessus des ombres ». Les panneaux et les fenêtres
/// (Dash, Réglages, applets) restent SOUS les ombres, comme les autres
/// applications. Une famille se reconnaît à sa palette (voir
/// `cef_popup::menus_visibles`).
///
/// ⛔ Deux essais du même jour à ne pas refaire : « juste au-dessus du dock »
/// laissait les ombres devant les menus (MESURÉ : le dock couvre l'écran et
/// les menus sont SOUS lui dans la pile) ; « en bas des topmost » passait
/// AUSSI les panneaux au-dessus, ce qui n'était pas voulu.
///
/// ⛔ **ET PAS EN TÊTE DE PILE** — BUG RÉEL SIGNALÉ (2026-10-23) : « encore
/// l'ombre qui apparaît périodiquement au-dessus des menus ». Remontées en tête
/// toutes les `REMONTEE`, elles passaient aussi devant les menus qui ne sont pas
/// les nôtres (clic droit des icônes de notification, menus Windows), que rien
/// ne remontait ensuite. Elles se posent maintenant JUSTE AU-DESSUS du plus
/// haut de leur SOCLE — le dock et nos panneaux visibles : tout ce qui est
/// au-dessus du socle (menus, les nôtres comme ceux des autres) reste devant.
///
/// ⛔ **LE DOCK PASSE AU-DESSUS DES PANNEAUX, EXPLICITEMENT** — BUG RÉEL SIGNALÉ
/// (2026-10-23) : « les coins arrondis de la zone centrale n'apparaissent pas
/// tout de suite quand j'ouvre un dash ». Les congés sont peints par la fenêtre
/// du dock ; un Dash ouvert passe devant elle. C'était rattrapé SANS LE DIRE :
/// les ombres et les coins appartiennent au dock, et leur `SetWindowPos` sans
/// `SWP_NOOWNERZORDER` remontait le dock avec eux, toutes les `REMONTEE`. Ce
/// drapeau ajouté (il remontait aussi le dock devant les menus), la remontée du
/// dock doit être faite ici, et à la bonne place : juste au-dessus du plus haut
/// panneau, jamais devant un menu.
pub fn remonter() {
    let docks: Vec<HWND> = BARRES.with(|b| b.try_borrow().map(|b| b.iter().map(|(h, _)| *h).collect()).unwrap_or_default());
    let panneaux = crate::win32::cef_popup::panneaux_visibles();
    POSEES.with(|p| {
        let Ok(posees) = p.try_borrow() else { return };
        ranger(&posees.1, &crate::win32::cef_popup::menus_visibles(), &docks, &panneaux);
    });
}

/// ⛔ **LES MENUS D'ABORD, LES OMBRES ENSUITE, SOUS EUX** — BUG RÉEL SIGNALÉ
/// (2026-10-23) : « les ombres clignotent au-dessus des menus ». L'ordre
/// inverse (ombres en tête, PUIS menus) laissait les ombres devant les menus
/// entre les deux appels, toutes les `REMONTEE` : un clignotement visible.
/// Ici les ombres ne passent jamais en tête quand un menu est visible : elles
/// s'insèrent directement sous le plus bas des menus (`SetWindowPos` pose une
/// fenêtre SOUS celle qu'on lui donne), remontés juste avant.
fn ranger(ombres: &[HWND], menus: &[HWND], docks: &[HWND], panneaux: &[HWND]) {
    let poser = |hwnd, sous| {
        let _ = unsafe { SetWindowPos(hwnd, Some(sous), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER) };
    };
    // ⭐ **UNE ICÔNE AGRANDIE PASSE DEVANT TOUT** — voir `rangement_magnifie` et
    // `ui::dock::magnification_en_cours`, qui porte la demande explicite.
    if crate::ui::dock::magnification_en_cours() {
        rangement_magnifie(&poser, ombres, menus, docks);
        return;
    }
    for &menu in menus {
        poser(menu, HWND_TOPMOST);
    }
    // Lue APRÈS la remontée des menus : l'ancre est alors sous eux. Les docks
    // et les ombres en sont exclus : ce sont eux qu'on déplace.
    let socle: Vec<HWND> = docks.iter().chain(panneaux).copied().collect();
    let deplaces: Vec<HWND> = ombres.iter().chain(docks).copied().collect();
    let ancre = au_dessus_du_socle(&socle, &deplaces);
    for (hwnd, sous) in placements(ombres, menus, docks, ancre) {
        poser(hwnd, sous);
    }
}

/// **L'ORDRE PENDANT LA MAGNIFICATION : DOCK EN TÊTE, PUIS MENUS, PUIS OMBRES.**
///
/// DEMANDE EXPLICITE (2026-11-11) : « l'effet de magnification doit faire
/// apparaître les icônes au-dessus des ombrages, des dashs et des menus ».
/// L'icône agrandie est dans la fenêtre du dock : elle n'est devant que si le
/// dock l'est, donc les docks passent EN TÊTE de la bande topmost, et tout ce
/// qui les recouvrirait d'habitude (ombres, menus) vient SOUS eux.
///
/// ⚠️ **L'ANCRE EST `ancre_de_magnification`**, le premier dock de `BARRES` —
/// celui qu'on remonte en premier, donc le PLUS BAS des docks. Poser un menu
/// dessous le met sous TOUS les docks (chaque `SetWindowPos` remonte à son tour
/// le dock suivant au-dessus).
///
/// ⚠️ **OMBRES D'ABORD, MENUS ENSUITE** : chaque insertion se fait JUSTE sous
/// l'ancre, donc la dernière posée finit la plus haute. Les menus restent donc
/// devant les ombres, comme au repos (demande du 2026-10-23) — il n'y a que le
/// dock qui change de camp.
fn rangement_magnifie(poser: &impl Fn(HWND, HWND), ombres: &[HWND], menus: &[HWND], docks: &[HWND]) {
    for &dock in docks {
        poser(dock, HWND_TOPMOST);
    }
    // Le dock le plus bas de ceux qu'on vient de remonter : `BARRES` est la
    // source de l'ordre des remontées ci-dessus, donc son premier élément est
    // le premier remonté, donc le plus bas. À défaut (aucune barre déclarée —
    // tout début de vie, ou test), le premier dock reçu fait le même office.
    let Some(ancre) = ancre_de_magnification().or_else(|| docks.first().copied()) else { return };
    for &ombre in ombres {
        poser(ombre, ancre);
    }
    for &menu in menus {
        poser(menu, ancre);
    }
}

/// Le dock SOUS lequel se posent menus et ombres pendant la magnification —
/// le PREMIER de `BARRES`, c'est-à-dire le plus bas après que `rangement_magnifie`
/// les a tous remontés dans cet ordre. `None` : aucune barre déclarée.
///
/// ⚠️ **LE MÊME ANCRE SERT À `cef_popup::raise_visible_popups`** : les menus y
/// sont reposés à chaque image pendant la magnification, et deux ancres
/// différentes les feraient sauter d'un rang à l'autre.
pub fn ancre_de_magnification() -> Option<HWND> {
    BARRES.with(|b| b.try_borrow().ok().and_then(|b| b.first().map(|(h, _)| *h)))
}

/// Les `SetWindowPos` de `ranger`, DANS L'ORDRE : les docks puis les ombres,
/// chacun posé SOUS `ancre` (la fenêtre juste au-dessus du socle) — la dernière
/// posée finit juste sous l'ancre, donc les ombres au-dessus des docks, et les
/// docks au-dessus des panneaux. À défaut d'ancre, sous le plus bas des menus ;
/// à défaut, en tête. Jamais en tête quand un menu est visible — c'était le
/// clignotement.
fn placements(ombres: &[HWND], menus: &[HWND], docks: &[HWND], ancre: Option<HWND>) -> Vec<(HWND, HWND)> {
    let sous = ancre.or(menus.first().copied()).unwrap_or(HWND_TOPMOST);
    docks.iter().chain(ombres).map(|&h| (h, sous)).collect()
}

/// La fenêtre JUSTE AU-DESSUS du plus haut du socle, nos ombres exceptées
/// (`SetWindowPos` pose une fenêtre SOUS celle qu'on lui donne). `None` : le
/// socle est en tête de pile, ou introuvable.
fn au_dessus_du_socle(socle: &[HWND], ombres: &[HWND]) -> Option<HWND> {
    let mut ancre = None;
    let mut courante = unsafe { GetTopWindow(None) }.ok();
    while let Some(h) = courante {
        if socle.contains(&h) {
            return ancre;
        }
        if !ombres.contains(&h) {
            ancre = Some(h);
        }
        courante = unsafe { GetWindow(h, GW_HWNDNEXT) }.ok();
    }
    None
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Une ombre ne se ferme pas seule : c'est `synchroniser` qui les détruit.
    if msg == WM_CLOSE {
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn classe() -> Option<HINSTANCE> {
    static CLASSE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }.ok()?.into();
    CLASSE.get_or_init(|| {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            // Hors du préfixe `NeuraDesk` : même raison que `win32::coins_ecran`.
            lpszClassName: w!("NdOmbreBarre"),
            ..Default::default()
        };
        unsafe { RegisterClassExW(&wc) };
    });
    Some(instance)
}

/// **LE PROPRIÉTAIRE DES OMBRES — UN SOCLE INVISIBLE, POSSÉDÉ PAR LE DOCK.**
///
/// ⛔ **POURQUOI PAS LE DOCK LUI-MÊME** (c'était le cas jusqu'au 2026-11-11) :
/// Windows impose qu'une fenêtre possédée soit **toujours AU-DESSUS de son
/// propriétaire**. Les ombres ne pouvaient donc PAS passer sous le dock, et
/// l'icône agrandie — dessinée par le dock — restait sous la bande d'ombre :
/// c'est le défaut que la demande « l'effet de magnification doit faire
/// apparaître les icônes au-dessus des ombrages » fait disparaître. Avec ce
/// socle pour propriétaire, plus rien ne contraint leur rang relatif au dock :
/// `ranger` le décide seul (voir `rangement_magnifie`).
///
/// **La durée de vie ne change pas** : le socle est possédé par le dock, donc
/// il meurt avec lui, et les ombres meurent avec le socle.
///
/// Créé à la PREMIÈRE ombre, gardé ensuite. `None` tant que le dock n'existe
/// pas (tests) : les ombres sont alors sans propriétaire, comme avant.
fn socle() -> Option<HWND> {
    static SOCOLE: AtomicIsize = AtomicIsize::new(0);
    let existant = HWND(SOCOLE.load(Ordering::Relaxed) as *mut std::ffi::c_void);
    if !existant.0.is_null() {
        return Some(existant);
    }
    // Le dock doit déjà exister : c'est lui qui garde le socle en vie.
    let proprietaire = crate::win32::coins_ecran::proprietaire()?;
    let instance = classe()?;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("NdOmbreBarre"),
            w!("NeuraDesk — socle des ombres"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            Some(proprietaire),
            None,
            Some(instance),
            None,
        )
    }
    .ok()?;
    SOCOLE.store(hwnd.0 as isize, Ordering::Relaxed);
    Some(hwnd)
}

fn creer() -> Option<HWND> {
    let instance = classe()?;
    // Possédée par le SOCLE, pas par le dock : voir `socle`, qui porte la raison
    // (une fenêtre possédée ne peut jamais passer sous son propriétaire).
    let proprietaire = socle();
    match unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("NdOmbreBarre"),
            w!("NeuraDesk — ombre d'une barre"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            proprietaire,
            None,
            Some(instance),
            None,
        )
    } {
        Ok(hwnd) => Some(hwnd),
        Err(err) => {
            crate::diag!("[ombre_barre] CreateWindowExW échoué : {err}");
            None
        }
    }
}

/// BGRA prémultiplié de l'ombre : du noir, l'alpha vaut `force · (1 − t)²` avec
/// `t` la position du centre du pixel dans l'ÉPAISSEUR de l'ombre, comptée depuis
/// la barre qui la jette (`cote`) (une couche
/// noire d'alpha `a` multiplie ce qu'elle couvre par `1 − a`, exactement comme le
/// shader).
/// `teinte(x, y)` : la teinte au centre du pixel (coordonnées de la fenêtre).
/// `rect` : `[x, y, largeur, hauteur]`, le morceau de l'ombre à rendre (voir
/// `pixels_dans`) — l'ombre entière : `[0, 0, largeur, hauteur]`.
fn pixels_de_l_ombre(
    largeur: i32,
    hauteur: i32,
    force: f32,
    couche: Couche,
    portion: Portion,
    cote: Cote,
    teinte: impl Fn(f32, f32) -> [u8; 3],
    texture: Option<&[u8]>,
    [rx, ry, rl, rh]: [i32; 4],
) -> Vec<u8> {
    let epaisseur = match cote {
        Cote::Haut | Cote::Bas => hauteur,
        Cote::Gauche | Cote::Droite => largeur,
    };
    pixels_par_distance(rl, rh, force, couche, portion, epaisseur as f32, texture, |x, y| {
        let (x, y) = (x + rx as f32, y + ry as f32);
        let d = match cote {
            Cote::Haut => y,
            Cote::Bas => hauteur as f32 - y,
            Cote::Gauche => x,
            Cote::Droite => largeur as f32 - x,
        };
        // Une simple bande ne tombe que sur l'interface (une barre).
        (d, teinte(x, y), 1.0, cote)
    })
}

/// Le corps commun : `distance(x, y)` au centre de chaque pixel (coordonnées de la
/// fenêtre), la courbe sur `epaisseur` ; une distance négative (un congé) ne
/// reçoit rien. Avec la distance viennent la teinte et la part du pixel sur
/// l'interface (`0..=1`). Une couche (voir `Couche`) : la COLORÉE, sur cette part
/// seulement ; la NOIRE, partout, sans teinte — de cette ombre, la `portion`
/// sur la zone libre ou sur l'interface. `texture` : le calque de la texture
/// de l'interface sur ce rectangle (RGBA à alpha droit), que la couche colorée
/// reçoit à son tour (voir `Gpu`). Les pixels sont en BGRA PRÉMULTIPLIÉ (`AC_SRC_ALPHA`).
fn pixels_par_distance(
    largeur: i32,
    hauteur: i32,
    force: f32,
    couche: Couche,
    portion: Portion,
    epaisseur: f32,
    texture: Option<&[u8]>,
    distance: impl Fn(f32, f32) -> (f32, [u8; 3], f32, Cote),
) -> Vec<u8> {
    let intensite = couche.intensite as f32 / 100.0;
    let mut px = Vec::with_capacity((largeur * hauteur * 4) as usize);
    for y in 0..hauteur {
        for x in 0..largeur {
            let (d, teinte, part, barre) = distance(x as f32 + 0.5, y as f32 + 0.5);
            let noir = if d < 0.0 {
                0.0
            } else {
                let t = 1.0 - (d / epaisseur).clamp(0.0, 1.0);
                force * t * t
            };
            let ombre = (noir * intensite).clamp(0.0, 1.0);
            // La colorée ne tombe que sur l'interface ; la noire, là où sa
            // portée le permet (voir `Couche::parts`).
            let part = part.clamp(0.0, 1.0);
            let (sur_barres, sur_zone) = couche.parts(barre);
            let opacite = ombre
                * match portion {
                    Portion::Interface => part * sur_barres,
                    Portion::Libre => (1.0 - part) * sur_zone,
                    #[cfg(test)]
                    Portion::Toute => part * sur_barres + (1.0 - part) * sur_zone,
                };
            let a = (opacite * 255.0).round().clamp(0.0, 255.0) as u8;
            // L'écart de la texture par canal, comme le shader l'ajoute aux fonds
            // (`rgb += delta`) : le calque le code en `0.5 + 0.5 · delta / écart`,
            // l'alpha valant l'écart.
            let i = ((y * largeur + x) * 4) as usize;
            let ecart = |k: usize| {
                texture.and_then(|t| t.get(i..i + 4)).map_or(0.0, |t| (2.0 * t[k] as f32 / 255.0 - 1.0) * t[3] as f32)
            };
            // Prémultiplié : la teinte n'apporte que sa part, à son opacité. La
            // noire n'en a pas — sauf sur la zone libre, teintée (`teinte_sur`).
            let pre = |c: u8, k: usize| {
                if couche.teinte_sur(portion) { ((c as f32 + ecart(k)) * opacite).round().clamp(0.0, a as f32) as u8 } else { 0 }
            };
            let [r, g, b] = teinte;
            px.extend_from_slice(&[pre(b, 2), pre(g, 1), pre(r, 0), a]);
        }
    }
    px
}

/// Les pixels d'une ombre posée : par la distance à la zone libre qu'elle borde
/// pour les côtés, en simple bande pour le haut.
fn pixels_de(ombre: &Ombre, portion: Portion) -> Vec<u8> {
    let mut calque = if ombre.texture != 0 { calque_de(ombre) } else { None };
    if let Some(c) = calque.as_mut() {
        centrer(c);
    }
    pixels_dans(ombre, portion, calque.as_deref(), [0, 0, ombre.largeur, ombre.hauteur])
}

/// Le pixel `(x, y)` de l'ombre (coordonnées de l'ombre), sans sa texture —
/// le même calcul que `pixels_de`, en un point (voir `teintes_zone_sous_la_couleur`).
fn pixel_de(ombre: &Ombre, portion: Portion, x: i32, y: i32) -> [u8; 4] {
    let px = pixels_dans(ombre, portion, None, [x, y, 1, 1]);
    [px[0], px[1], px[2], px[3]]
}

/// Le morceau `rect` (`[x, y, largeur, hauteur]`, coordonnées de l'ombre) des
/// pixels de l'ombre ; `texture` couvre ce morceau.
fn pixels_dans(ombre: &Ombre, portion: Portion, texture: Option<&[u8]>, rect: [i32; 4]) -> Vec<u8> {
    let (force, couche) = (ombre.force as f32 / 1000.0, ombre.couche);
    let [rx, ry, rl, rh] = rect;
    match ombre.interieur {
        // `alpha` porte déjà la force : la courbe de `pixels_par_distance` est
        // neutralisée (force 1, distance 1 − √alpha sur une épaisseur 1).
        Some(interieur) => pixels_par_distance(rl, rh, 1.0, couche, portion, 1.0, texture, |x, y| {
            let (sx, sy) = (ombre.x as f32 + rx as f32 + x, ombre.y as f32 + ry as f32 + y);
            let (alpha, barre) = interieur.alpha_et_barre(sx, sy);
            (1.0 - alpha.max(0.0).sqrt(), ombre.teinte_en(barre, sx, sy), interieur.sur_l_interface(sx, sy), barre)
        }),
        // L'ombre du haut : teintée sur les colonnes et leurs congés seulement.
        None if ombre.cote == Cote::Haut => {
            pixels_par_distance(rl, rh, force, couche, portion, ombre.hauteur as f32, texture, |x, y| {
                let y = y + ry as f32;
                let (sx, sy, haut) = (ombre.x as f32 + rx as f32 + x, ombre.y as f32 + y, ombre.y as f32);
                (y, ombre.teinte_en(Cote::Haut, sx, sy), sur_l_interface_haut(ombre.colonnes, ombre.rayon, haut, sx, sy), Cote::Haut)
            })
        }
        None => {
            let teinte = |x: f32, y: f32| ombre.teinte_en(ombre.cote, ombre.x as f32 + x, ombre.y as f32 + y);
            pixels_de_l_ombre(ombre.largeur, ombre.hauteur, force, couche, portion, ombre.cote, teinte, texture, rect)
        }
    }
}

fn peindre(hwnd: HWND, ombre: &Ombre) {
    let pixels = pixels_de(ombre, Portion::Libre);
    unsafe {
        let ecran = GetDC(None);
        let memoire = CreateCompatibleDC(Some(ecran));
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: ombre.largeur,
            biHeight: -ombre.hauteur,
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        if let Ok(bitmap) = CreateDIBSection(Some(memoire), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            if !bits.is_null() {
                std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
            }
            let ancien = SelectObject(memoire, bitmap.into());
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let taille = SIZE { cx: ombre.largeur, cy: ombre.hauteur };
            let source = POINT::default();
            let position = POINT { x: ombre.x, y: ombre.y };
            if let Err(err) = UpdateLayeredWindow(
                hwnd,
                Some(ecran),
                Some(&position),
                Some(&taille),
                Some(memoire),
                Some(&source),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            ) {
                crate::diag!("[ombre_barre] UpdateLayeredWindow échoué : {err}");
            }
            SelectObject(memoire, ancien);
            let _ = DeleteObject(bitmap.into());
        }
        let _ = DeleteDC(memoire);
        ReleaseDC(None, ecran);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un écran à gauche de la primaire (coordonnées négatives), 1920×1080 : une
    /// barre du haut de 72, des colonnes de 124 et 120, une barre du bas de 60.
    fn barre_d_exemple() -> Barre {
        Barre { origine: (-1920, 100), largeur: 1920.0, hauteur: 1080.0, bas: 72.0, gauche: 124.0, droite: 120.0, dessous: 60.0, rayon: 0.0, teintes: [[[0; 3]; SEGMENTS]; 4], teintes_zone: [[[0; 3]; SEGMENTS]; 4], texture: 0, ecran: None }
    }

    /// ⭐ Le centre de la barre du haut teinte SON ombre colorée et rien d'autre
    /// (demande du 2026-10-31) ; éteint, la barre reste telle quelle.
    #[test]
    fn le_centre_teinte_le_profil_de_la_barre_du_haut_seulement() {
        use crate::win32::centre_haut::ReglagesCentre;
        let barre = barre_d_exemple();
        let centre = ReglagesCentre { actif: true, largeur: 50.0, couleur: [255, 0, 0], ..ReglagesCentre::DEFAUT };
        let teintee = avec_le_centre(&barre, &centre);
        assert_eq!(teintee.teintes[Cote::Haut.rang()][SEGMENTS / 2], [255, 0, 0], "sous le bloc");
        assert_eq!(teintee.teintes_zone[Cote::Haut.rang()][SEGMENTS / 2], [255, 0, 0], "la zone en couleur suit");
        assert_eq!(teintee.teintes[1..], barre.teintes[1..], "les autres barres intactes");
        assert_eq!(avec_le_centre(&barre, &ReglagesCentre::DEFAUT), barre, "éteint");
        assert_eq!(avec_le_centre(&Barre { bas: 0.0, ..barre }, &centre), Barre { bas: 0.0, ..barre }, "sans barre du haut");
    }

    /// ⭐ Le choix de la couleur (demande du 2026-10-19) : la couleur réglée,
    /// telle quelle, ou la couleur peinte, référence du relevé.
    #[test]
    fn l_ombre_prend_la_couleur_reglee_ou_affichee() {
        let reglees = [[0.9, 0.07, 0.14]; 4];
        let peintes = [[0.8, 0.15, 0.3]; 4];
        choisir_couleur_affichee(false);
        let en_reglee = couleurs_d_ombre(reglees, peintes);
        choisir_couleur_affichee(true);
        assert_eq!(en_reglee, reglees);
        assert_eq!(couleurs_d_ombre(reglees, peintes), peintes);
    }

    /// ⭐ Les latérales prolongent leur ombre SUR la barre du bas, jusqu'au pied
    /// de l'écran (demande du 2026-10-16) ; ni sans barre du bas, ni case
    /// décochée, ni sous une colonne masquée.
    #[test]
    fn les_laterales_ombrent_la_barre_du_bas() {
        let r = ReglagesCotes { active: true, largeur: 30.0, force: 0.5, ..ReglagesCotes::DEFAUT };
        let sur_le_bas = |barre: &Barre, r: &ReglagesCotes| {
            ombres_des_cotes(barre, r).into_iter().filter(|o| o.interieur.is_none()).map(|o| (o.cote, o.x, o.y, o.largeur, o.hauteur)).collect::<Vec<_>>()
        };
        let (gauche, droite, bas, pied) = (-1920 + 124, -1920 + 1920 - 120, 100 + 1020, 100 + 1080);
        assert_eq!(sur_le_bas(&barre_d_exemple(), &r), vec![
            (Cote::Gauche, gauche, bas, 30, pied - bas),
            (Cote::Droite, droite - 30, bas, 30, pied - bas),
        ]);
        assert!(sur_le_bas(&barre_d_exemple(), &ReglagesCotes { sur_le_bas: false, ..r }).is_empty(), "case décochée");
        assert!(sur_le_bas(&Barre { dessous: 0.0, ..barre_d_exemple() }, &r).is_empty(), "pas de barre du bas");
        assert_eq!(sur_le_bas(&Barre { gauche: 0.0, ..barre_d_exemple() }, &r).len(), 1, "colonne masquée : pas d'ombre");
        // La bande la plus sombre contre la colonne.
        let o = ombres_des_cotes(&barre_d_exemple(), &r).into_iter().find(|o| o.interieur.is_none() && o.cote == Cote::Droite).unwrap();
        let px = pixels_de(&o, Portion::Toute);
        assert!(px[(o.largeur - 1) as usize * 4 + 3] > px[3], "contre la colonne de droite");
    }

    /// ⭐ Les trois ombres collées CONTRE leur barre, vers l'intérieur, sans se
    /// chevaucher : celle du bas d'une colonne à l'autre — ⛔ pas sur les colonnes
    /// (défaut signalé le 2026-10-16) —, les latérales au-dessus d'elle.
    #[test]
    fn les_ombres_des_cotes_sont_collees_a_leur_barre() {
        let r = ReglagesCotes { active: true, largeur: 30.0, force: 0.5, sur_le_bas: false, largeur_bas: 30.0, force_bas: 0.5, ..ReglagesCotes::DEFAUT };
        let ombres = ombres_des_cotes(&barre_d_exemple(), &r);
        let rect = |o: &Ombre| (o.cote, o.x, o.y, o.largeur, o.hauteur, o.force);
        let (gauche, droite, haut, bas) = (-1920 + 124, -1920 + 1920 - 120, 172, 100 + 1020);
        assert_eq!(ombres.iter().map(rect).collect::<Vec<_>>(), vec![
            (Cote::Gauche, gauche, haut, 30, bas - 30 - haut, 500),
            (Cote::Droite, droite - 30, haut, 30, bas - 30 - haut, 500),
            (Cote::Bas, gauche, bas - 30, droite - gauche, 30, 500),
        ]);
        // Une case décochée, une barre absente (écran secondaire : pas de barre du bas).
        let sans_gauche = ombres_des_cotes(&barre_d_exemple(), &ReglagesCotes { gauche: false, ..r });
        assert!(sans_gauche.iter().all(|o| o.cote != Cote::Gauche));
        let secondaire = Barre { dessous: 0.0, ..barre_d_exemple() };
        let ombres = ombres_des_cotes(&secondaire, &r);
        assert!(ombres.iter().all(|o| o.cote != Cote::Bas));
        assert_eq!(ombres[0].hauteur, 1080 - 72, "jusqu'en bas de l'écran");
        // Une colonne MASQUÉE : l'ombre du bas court jusqu'au bord de l'écran.
        let masquee = ombres_des_cotes(&Barre { gauche: 0.0, ..barre_d_exemple() }, &r);
        let bas_masquee = masquee.iter().find(|o| o.cote == Cote::Bas).unwrap();
        assert_eq!((bas_masquee.x, bas_masquee.largeur), (-1920, 1920 - 120));
        assert!(masquee.iter().all(|o| o.cote != Cote::Gauche), "une colonne masquée n'ombre pas");
        // Éteinte par défaut ; à largeur ou force nulle, les colonnes s'éteignent
        // et le bas, réglé à part, reste.
        assert!(ombres_des_cotes(&barre_d_exemple(), &ReglagesCotes::DEFAUT).is_empty(), "éteinte par défaut");
        let cotes = |r: ReglagesCotes| ombres_des_cotes(&barre_d_exemple(), &r).iter().map(|o| o.cote).collect::<Vec<_>>();
        assert_eq!(cotes(ReglagesCotes { largeur: 0.0, ..r }), [Cote::Bas]);
        assert_eq!(cotes(ReglagesCotes { force: 0.0, ..r }), [Cote::Bas]);
        assert!(cotes(ReglagesCotes { largeur: 0.0, largeur_bas: 0.0, ..r }).is_empty());
    }

    /// La courbe part de la barre qui jette l'ombre : la plus sombre contre elle.
    #[test]
    fn chaque_ombre_est_la_plus_sombre_contre_sa_barre() {
        let a = |px: &[u8], largeur: i32, x: i32, y: i32| px[((y * largeur + x) * 4 + 3) as usize];
        let gauche = pixels_de_l_ombre(10, 3, 0.5, Couche::NOIRE, Portion::Toute, Cote::Gauche, |_, _| [0, 0, 0], None, [0, 0, 10, 3]);
        assert!(a(&gauche, 10, 0, 1) > a(&gauche, 10, 9, 1) && a(&gauche, 10, 0, 0) == a(&gauche, 10, 0, 2), "de gauche à droite");
        let droite = pixels_de_l_ombre(10, 3, 0.5, Couche::NOIRE, Portion::Toute, Cote::Droite, |_, _| [0, 0, 0], None, [0, 0, 10, 3]);
        assert_eq!(a(&droite, 10, 9, 1), a(&gauche, 10, 0, 1), "en miroir");
        let bas = pixels_de_l_ombre(3, 10, 0.5, Couche::NOIRE, Portion::Toute, Cote::Bas, |_, _| [0, 0, 0], None, [0, 0, 3, 10]);
        assert_eq!(a(&bas, 3, 1, 9), a(&gauche, 10, 0, 1), "contre la barre du bas");
        assert!(a(&bas, 3, 1, 0) <= 1);
    }

    /// ⭐ **À LA COULEUR DE SA BARRE** (demande du 2026-10-18) : chaque pixel prend
    /// la barre qui l'ombre le plus fort — sa colonne dans un congé du haut, la
    /// barre du bas dans un arc du bas.
    #[test]
    fn chaque_pixel_prend_la_barre_qui_l_ombre() {
        let i = Interieur { gauche: 0, droite: 1000, haut: 0, bas: 500, rayon: 40, ombrent: [true, true, true], coins: [true; 4], sur_le_bas: [false; 2], laterales: Trait { largeur: 30, force: 1000 }, dessous: Trait { largeur: 30, force: 1000 } };
        assert_eq!(i.alpha_et_barre(5.0, 250.0).1, Cote::Gauche, "le long de la colonne de gauche");
        assert_eq!(i.alpha_et_barre(995.0, 250.0).1, Cote::Droite, "le long de celle de droite");
        assert_eq!(i.alpha_et_barre(500.0, 495.0).1, Cote::Bas, "le long de la barre du bas");
        assert_eq!(i.alpha_et_barre(15.0, 15.0).1, Cote::Gauche, "congé du haut-gauche : sa colonne");
        assert_eq!(i.alpha_et_barre(985.0, 15.0).1, Cote::Droite, "congé du haut-droite : sa colonne");
        assert_eq!(i.alpha_et_barre(30.0, 492.0).1, Cote::Bas, "arc du bas : la barre du bas");
    }

    /// ⭐ La teinte seulement sur l'interface (précision du 2026-10-18) : sur un
    /// congé ou une autre barre ; sur la zone libre, du noir.
    #[test]
    fn la_teinte_ne_tombe_que_sur_l_interface() {
        let i = Interieur { gauche: 0, droite: 1000, haut: 0, bas: 500, rayon: 40, ombrent: [true, true, true], coins: [true; 4], sur_le_bas: [false; 2], laterales: Trait { largeur: 30, force: 1000 }, dessous: Trait { largeur: 30, force: 1000 } };
        assert_eq!(i.sur_l_interface(2.0, 2.0), 1.0, "sur le congé du haut-gauche");
        assert_eq!(i.sur_l_interface(2.0, 498.0), 1.0, "sur le congé du bas-gauche");
        assert_eq!(i.sur_l_interface(30.0, 30.0), 0.0, "dans l'arc : la zone libre");
        assert_eq!(i.sur_l_interface(5.0, 250.0), 0.0, "le long de la colonne : la zone libre");
        assert_eq!(i.sur_l_interface(500.0, 501.0), 1.0, "sur la barre du bas");
        assert_eq!(Interieur { coins: [false; 4], ..i }.sur_l_interface(2.0, 2.0), 0.0, "coin droit : la zone libre");

        let colonnes = [Some(100), Some(1800)];
        assert_eq!(sur_l_interface_haut(colonnes, 20, 72.0, 50.0, 80.0), 1.0, "sur la colonne de gauche");
        assert_eq!(sur_l_interface_haut(colonnes, 20, 72.0, 1850.0, 80.0), 1.0, "sur celle de droite");
        assert_eq!(sur_l_interface_haut(colonnes, 20, 72.0, 101.0, 73.0), 1.0, "sur le congé du haut-gauche");
        assert_eq!(sur_l_interface_haut(colonnes, 20, 72.0, 115.0, 88.0), 0.0, "dans l'arc");
        assert_eq!(sur_l_interface_haut(colonnes, 20, 72.0, 950.0, 80.0), 0.0, "au milieu");
        assert_eq!(sur_l_interface_haut([None; 2], 20, 72.0, 50.0, 80.0), 0.0, "sans colonne");
    }

    /// ⭐ **LE BORD DE L'ARC EST LISSÉ** (défaut signalé le 2026-10-18 sur les
    /// arrondis du bas) : sur un pixel à cheval sur l'arc, l'ombre et la teinte
    /// passent par une valeur intermédiaire, au lieu de sauter d'un pixel à l'autre.
    #[test]
    fn le_bord_de_l_arc_est_lisse() {
        let i = Interieur { gauche: 0, droite: 1000, haut: 0, bas: 500, rayon: 40, ombrent: [false, false, true], coins: [true; 4], sur_le_bas: [false; 2], laterales: Trait { largeur: 30, force: 1000 }, dessous: Trait { largeur: 30, force: 1000 } };
        // Sur l'arc du coin bas-gauche (centre (40, 460), rayon 40), sur sa diagonale.
        let demi = 40.0 / std::f32::consts::SQRT_2;
        let (x, y) = (40.0 - demi, 460.0 + demi);
        let a = i.alpha_et_barre(x, y).0;
        let part = i.sur_l_interface(x, y);
        assert!(a > 0.1 && a < 0.9, "ombre à mi-chemin sur l'arc : {a}");
        assert!(part > 0.1 && part < 0.9, "teinte à mi-chemin sur l'arc : {part}");
        let (dedans, dehors) = (i.alpha_et_barre(x + 2.0, y - 2.0).0, i.alpha_et_barre(x - 2.0, y + 2.0).0);
        assert!(dedans > a && dehors < a, "l'ombre décroît vers le congé : {dedans} > {a} > {dehors}");
    }

    /// ⭐ **LA COULEUR RENDUE, LE LONG DE LA BARRE** (défaut signalé le
    /// 2026-10-19) : la barre du haut rose à gauche, violette à droite — l'ombre
    /// prend la rose sur la colonne de gauche, la violette sur celle de droite.
    #[test]
    fn l_ombre_du_haut_prend_la_couleur_rendue_au_dessus_de_chaque_colonne() {
        let mut profil = [[145, 90, 80]; SEGMENTS];
        profil[SEGMENTS / 2..].fill([90, 40, 140]);
        let noir = [[0; 3]; SEGMENTS];
        let barre = Barre { teintes: [profil, noir, noir, noir], ..barre_d_exemple() };
        let ombre = ombre_de(&barre, 24.0, 1.0).unwrap().en(Couche::COLOREE);
        let px = pixels_de(&ombre, Portion::Toute);
        let rgb = |x: i32| {
            let i = (x * 4) as usize;
            [px[i + 2], px[i + 1], px[i]]
        };
        let ([r, _, b], [r2, _, b2]) = (rgb(10), rgb(1910));
        assert!(r > b, "colonne de gauche, sous la partie rose : {:?}", rgb(10));
        assert!(b2 > r2, "colonne de droite, sous la partie violette : {:?}", rgb(1910));
    }

    /// ⛔ **UN PROFIL COUVRE SA BARRE, PAS LA FENÊTRE** (défaut signalé le
    /// 2026-10-19 : « les autres ombres colorées ne suivent pas ») : le haut du
    /// profil d'une colonne est le haut de la COLONNE, sous la barre du haut — le
    /// congé du haut, coloré par elle, en prend la couleur et non celle du haut ; et
    /// le début du profil du bas est son bord contre la colonne de gauche.
    #[test]
    fn chaque_profil_couvre_l_etendue_de_sa_barre() {
        let barre = barre_d_exemple();
        // Le premier segment rouge, le dernier bleu, le reste gris : un profil
        // calé sur la fenêtre lirait du gris contre la barre du haut.
        let mut colonne = [[90, 90, 90]; SEGMENTS];
        colonne[0] = [200, 0, 0];
        colonne[SEGMENTS - 1] = [0, 0, 200];
        let ombre = Ombre { teintes: [[[0, 200, 0]; SEGMENTS], colonne, colonne, colonne], ..ombre_de(&barre, 24.0, 1.0).unwrap() };
        let (gauche, haut) = (-1920 + 124, 100 + 72);
        assert_eq!(ombre.teinte_en(Cote::Gauche, gauche as f32 + 2.0, haut as f32 + 2.0), [200, 0, 0], "le congé du haut : la colonne");
        assert_eq!(ombre.teinte_en(Cote::Gauche, gauche as f32, haut as f32 - 30.0), [200, 0, 0], "au-dessus, borné au bout");
        assert_eq!(ombre.teinte_en(Cote::Gauche, gauche as f32, 100.0 + 1079.0), [0, 0, 200], "le pied de la colonne");
        assert_eq!(ombre.teinte_en(Cote::Bas, gauche as f32 + 1.0, 1150.0), [200, 0, 0], "le bas commence contre la colonne");
    }

    /// ⛔ La texture d'une ombre relue est CENTRÉE (défaut signalé le 2026-10-19 :
    /// l'ombre démarrait plus claire que sa barre) : son motif reste, son
    /// décalage moyen disparaît.
    #[test]
    fn le_calque_centre_garde_le_motif_sans_son_decalage() {
        // Deux pixels : +30 et +10 sur chaque canal (moyenne +20).
        let mut calque = vec![255, 255, 255, 30, 255, 255, 255, 10];
        centrer(&mut calque);
        let ecart = |i: usize| (2.0 * calque[i] as f32 / 255.0 - 1.0) * calque[i - (i % 4) + 3] as f32;
        assert!((ecart(0) - 10.0).abs() < 0.6 && (ecart(4) + 10.0).abs() < 0.6, "{calque:?}");
        // Le motif garde son écart d'un pixel à l'autre.
        assert!((ecart(0) - ecart(4) - 20.0).abs() < 1.0);
    }

    /// Le profil s'interpole entre les centres des segments, et se borne à ses bouts.
    #[test]
    fn le_profil_s_interpole_le_long_de_la_barre() {
        let profil: Profil = std::array::from_fn(|k| [(k * 8) as u8, 0, 0]);
        assert_eq!(teinte_le_long(&profil, 0.0), [0, 0, 0]);
        assert_eq!(teinte_le_long(&profil, 1.0), [((SEGMENTS - 1) * 8) as u8, 0, 0]);
        assert_eq!(teinte_le_long(&profil, 1.0 / SEGMENTS as f32), [4, 0, 0], "entre les deux premiers centres");
        assert!(proches(&profil, &profil.map(|[r, g, b]| [r.saturating_add(TOLERANCE), g, b])));
        assert!(!proches(&profil, &profil.map(|[r, g, b]| [r.saturating_add(TOLERANCE + 1), g, b])));
    }

    /// La teinte part prémultipliée par l'alpha (`AC_SRC_ALPHA`), en BGRA.
    #[test]
    fn la_teinte_est_premultipliee() {
        let px = pixels_de_l_ombre(1, 20, 1.0, Couche::COLOREE, Portion::Toute, Cote::Haut, |_, _| [255, 128, 0], None, [0, 0, 1, 20]);
        let [b, g, r, a] = [px[0], px[1], px[2], px[3]];
        assert!(a > 200);
        assert!(r.abs_diff(a) <= 1 && b == 0, "rouge plein, prémultiplié ; pas de bleu");
        assert!(g.abs_diff(((128 * a as u32 + 127) / 255) as u8) <= 1);
    }

    /// ⭐ **CHAQUE COUCHE NE PORTE QUE LA SIENNE** (demande du 2026-10-19) : la
    /// noire tombe partout, sans teinte ; la colorée seulement sur l'interface ;
    /// chacune à son intensité.
    #[test]
    fn chaque_couche_ne_porte_que_la_sienne() {
        let pixel = |couche: Couche, part: f32| {
            let px = pixels_par_distance(1, 1, 0.4, couche, Portion::Toute, 1.0, None, |_, _| (0.0, [255, 0, 0], part, Cote::Haut));
            [px[0], px[1], px[2], px[3]]
        };
        let (noire, coloree) = (Couche::NOIRE, Couche::COLOREE);
        assert_eq!(pixel(noire, 0.0), [0, 0, 0, 102], "zone libre : du noir à 40 %");
        assert_eq!(pixel(noire, 1.0), [0, 0, 0, 102], "sur l'interface aussi, et sans teinte");
        assert_eq!(pixel(coloree, 0.0), [0; 4], "la couleur ne tombe pas sur la zone libre");
        assert_eq!(pixel(coloree, 1.0), [0, 0, 102, 102], "la couleur, sur l'interface");
        assert_eq!(pixel(Couche { intensite: 200, ..coloree }, 1.0), [0, 0, 204, 204], "deux fois plus opaque");
        assert_eq!(pixel(Couche { intensite: 0, ..noire }, 0.0), [0; 4], "0 % : plus d'ombre");
        assert_eq!(pixel(Couche { intensite: 500, ..coloree }, 1.0)[3], 255, "plafonnée à l'opacité pleine");
    }

    /// ⭐ La texture par-dessus l'ombre colorée (demande du 2026-10-18) : la couche
    /// colorée reçoit l'écart du calque, comme un fond de barre ; la noire, non.
    #[test]
    fn la_part_coloree_recoit_la_texture() {
        let pixel = |calque: [u8; 4], couche: Couche, part: f32| {
            let px = pixels_par_distance(1, 1, 1.0, couche, Portion::Toute, 1.0, Some(&calque), |_, _| (0.0, [100, 100, 100], part, Cote::Haut));
            [px[0], px[1], px[2], px[3]]
        };
        let c = Couche::COLOREE;
        // Un calque blanc à 20 : le canal éclaircit de 20 ; noir, il assombrit.
        assert_eq!(pixel([255, 255, 255, 20], c, 1.0), [120, 120, 120, 255]);
        assert_eq!(pixel([0, 0, 0, 20], c, 1.0), [80, 80, 80, 255]);
        assert_eq!(pixel([255, 255, 255, 0], c, 1.0), [100, 100, 100, 255], "sans écart, la teinte seule");
        assert_eq!(pixel([255, 255, 255, 20], Couche::NOIRE, 1.0), [0, 0, 0, 255], "la noire n'en reçoit pas");
    }

    /// ⭐ **DEUX JEUX DE RÉGLAGES COMPLETS** (demande du 2026-10-19 : « des
    /// réglages complets pour chaque type d'ombre, noire et couleur ») : chaque
    /// couche a sa largeur, sa force et ses barres ; la colorée est posée d'abord,
    /// la noire par-dessus ; la noire n'a pas de teinte ; une couche à 0 % n'a pas
    /// de fenêtre.
    #[test]
    fn chaque_couche_a_ses_propres_reglages() {
        let barre = Barre { teintes: [[[200, 0, 0]; SEGMENTS]; 4], ..barre_d_exemple() };
        let cotes = ReglagesCotes { active: true, largeur: 50.0, force: 0.6, droite: false, bas: false, sur_le_bas: false, ..ReglagesCotes::DEFAUT };
        let coloree = Reglages { haut: [40.0, 0.8], cotes, couche: Couche::COLOREE };
        let noire = Reglages { haut: [10.0, 0.3], cotes: ReglagesCotes::DEFAUT, couche: Couche::NOIRE };
        let ombres = ombres_de_la_barre(&barre, &[coloree, noire]);
        let vues: Vec<_> = ombres.iter().map(|o| (o.couche.coloree, o.cote, o.largeur.min(o.hauteur), o.force)).collect();
        assert_eq!(vues, vec![(true, Cote::Haut, 40, 800), (true, Cote::Gauche, 50, 600), (false, Cote::Haut, 10, 300)]);
        assert_eq!(ombres[0].teintes[0][0], [200, 0, 0], "la colorée garde sa teinte");
        assert_eq!(ombres[2].teintes, [[[0; 3]; SEGMENTS]; 4], "la noire n'en a pas");
        let sans_couleur = Reglages { couche: Couche { intensite: 0, ..Couche::COLOREE }, ..coloree };
        assert_eq!(ombres_de_la_barre(&barre, &[sans_couleur, noire]).len(), 1, "couche à 0 % : pas d'ombre");
    }

    /// ⭐ **SOUS LES ICÔNES ET LES TEXTES** (demande du 2026-10-19) : une ombre se
    /// partage entre sa fenêtre (la zone libre) et le calque du dock (l'interface),
    /// sans rien perdre ni doubler ; la colorée n'a rien sur la zone libre.
    #[test]
    fn la_fenetre_et_le_calque_se_partagent_l_ombre() {
        let r = ReglagesCotes { active: true, largeur: 30.0, force: 0.8, largeur_bas: 30.0, force_bas: 0.8, ..ReglagesCotes::DEFAUT };
        let barre = Barre { rayon: 20.0, ..barre_d_exemple() };
        let bas = ombres_des_cotes(&barre, &r).into_iter().find(|o| o.cote == Cote::Bas && o.interieur.is_some()).unwrap();
        let alphas = |o: &Ombre, portion: Portion| pixels_de(o, portion).chunks_exact(4).map(|p| p[3] as i32).collect::<Vec<_>>();
        let (libre, interface, toute) = (alphas(&bas, Portion::Libre), alphas(&bas, Portion::Interface), alphas(&bas, Portion::Toute));
        for i in 0..toute.len() {
            assert!((libre[i] + interface[i] - toute[i]).abs() <= 1, "pixel {i} : {} + {} ≠ {}", libre[i], interface[i], toute[i]);
        }
        assert!(interface.iter().any(|&a| a > 0) && libre.iter().any(|&a| a > 0), "les congés au calque, la zone libre à la fenêtre");
        assert!(alphas(&bas.en(Couche::COLOREE), Portion::Libre).iter().all(|&a| a == 0), "la colorée : rien sur la zone libre");
    }

    /// Qui a une fenêtre : la noire seulement ; l'ombre du haut si elle tombe sur
    /// la zone centrale ; celles des côtés si l'une des trois autres y tombe ;
    /// jamais une simple bande sur la barre du bas.
    #[test]
    fn seule_la_noire_sur_la_zone_libre_a_une_fenetre() {
        let barre = barre_d_exemple();
        let noire = |zone: [bool; 4]| Couche { portee: Portee { zone, ..Portee::DEFAUT }, ..Couche::NOIRE };
        let haut = ombre_de(&barre, 24.0, 0.5).unwrap();
        assert!(a_une_fenetre(&haut.en(noire([true; 4]))) && !a_une_fenetre(&haut.en(noire([false, true, true, true]))));
        assert!(!a_une_fenetre(&haut.en(Couche::COLOREE)), "la colorée : au calque seulement");
        let r = ReglagesCotes { active: true, ..ReglagesCotes::DEFAUT };
        let ombres = ombres_des_cotes(&barre, &r);
        let cotes = || ombres.iter().filter(|o| o.interieur.is_some());
        assert!(cotes().all(|o| a_une_fenetre(&o.en(noire([false, false, true, false])))), "une seule barre suffit : les coins sont partagés");
        assert!(cotes().all(|o| !a_une_fenetre(&o.en(noire([true, false, false, false])))), "seul le haut : pas les côtés");
        assert!(ombres.iter().filter(|o| o.interieur.is_none()).all(|o| !a_une_fenetre(o)), "sur la barre du bas");
    }

    /// ⭐ **OÙ TOMBE L'OMBRE NOIRE, BARRE PAR BARRE** (demande du 2026-10-19) :
    /// chaque barre a sa case « sur les autres barres » et sa case « sur la zone
    /// centrale » ; un pixel suit la barre qui l'ombre ; la colorée n'en dépend pas.
    #[test]
    fn la_portee_de_la_noire_se_regle_par_barre() {
        let pixel = |couche: Couche, portion: Portion, part: f32, barre: Cote| {
            pixels_par_distance(1, 1, 0.4, couche, portion, 1.0, None, |_, _| (0.0, [255, 0, 0], part, barre))[3]
        };
        let portee = Portee { barres: [true, false, true, true], zone: [true, true, true, false] };
        let noire = Couche { portee, ..Couche::NOIRE };
        assert_eq!(pixel(noire, Portion::Interface, 1.0, Cote::Gauche), 0, "la gauche : pas sur les autres barres");
        assert_eq!(pixel(noire, Portion::Interface, 1.0, Cote::Droite), 102, "la droite : sur les autres barres");
        assert_eq!(pixel(noire, Portion::Libre, 0.0, Cote::Gauche), 102, "la gauche : sur la zone centrale");
        assert_eq!(pixel(noire, Portion::Libre, 0.0, Cote::Bas), 0, "le bas : pas sur la zone centrale");
        let coloree = Couche { portee, ..Couche::COLOREE };
        assert_eq!(pixel(coloree, Portion::Interface, 1.0, Cote::Gauche), 102, "la colorée n'en dépend pas");
    }

    /// ⭐ **UN ÉCRAN SECONDAIRE RÉGLÉ À PART** (demande du 2026-10-19) : ses
    /// réglages remplacent ceux du principal sous sa barre du haut, noire et
    /// colorée ; sans eux, il suit le principal.
    #[test]
    fn un_ecran_secondaire_a_ses_propres_ombres() {
        let noire = Reglages { haut: [24.0, 0.5], cotes: ReglagesCotes::DEFAUT, couche: Couche::NOIRE };
        let coloree = Reglages { couche: Couche::COLOREE, ..noire };
        let ombres = |ecran| ombres_de_la_barre(&Barre { ecran, ..barre_d_exemple() }, &[coloree, noire]);
        let haut = |o: &Ombre| (o.couche.coloree, o.hauteur, o.force, a_une_fenetre(o));
        assert_eq!(ombres(None).iter().map(haut).collect::<Vec<_>>(), vec![(true, 24, 500, false), (false, 24, 500, true)]);
        let e = ReglagesEcran { noire: [40.0, 0.8], zone: false, couleur: [10.0, 0.3] };
        assert_eq!(ombres(Some(e)).iter().map(haut).collect::<Vec<_>>(), vec![(true, 10, 300, false), (false, 40, 800, false)]);
    }

    /// Les réglages d'un écran se relisent ; une ligne incomplète est ignorée.
    #[test]
    fn les_reglages_d_un_ecran_se_relisent() {
        let e = ReglagesEcran { noire: [40.0, 0.8], zone: false, couleur: [10.0, 0.25] };
        assert_eq!(ReglagesEcran::charger(&e.serialiser()), Some(e));
        assert_eq!(ReglagesEcran::charger("40,0.8"), None);
        assert_eq!(ReglagesEcran::charger("40,7,1,10,0.25").map(|r| r.noire[1]), Some(1.0), "force bornée");
    }

    /// Les huit cases se relisent ; un fichier absent ou incomplet garde le défaut.
    #[test]
    fn la_portee_se_relit() {
        let p = Portee { barres: [true, false, true, false], zone: [false, true, true, false] };
        assert_eq!(p.serialiser(), "1,0,1,0,0,1,1,0");
        assert_eq!(Portee::charger(&p.serialiser(), Portee::DEFAUT), p);
        let defaut = Portee { zone: [false, true, true, true], ..Portee::DEFAUT };
        assert_eq!(Portee::charger("", defaut), defaut);
        assert_eq!(Portee::charger("0,1", defaut).barres, [false, true, true, true]);
    }

    /// ⭐ La noire « teintée » (`zone_en_couleur`) prend sur la ZONE LIBRE la teinte
    /// de sa barre — et reste noire sur les colonnes ; décochée, noire partout,
    /// comme avant. Passe par `ombres_de_la_barre`, qui lui donne `teintes_zone`.
    /// Sur l'ancien code, la part libre d'une noire n'avait JAMAIS de teinte.
    #[test]
    fn la_noire_de_la_zone_libre_prend_la_couleur_de_sa_barre() {
        let barre = Barre { teintes_zone: [[[200, 0, 0]; SEGMENTS]; 4], ..barre_d_exemple() };
        let couches = |teintee_zone| {
            let reglages = |couche| Reglages { haut: [24.0, 1.0], cotes: ReglagesCotes::DEFAUT, couche };
            [reglages(Couche { intensite: 0, ..Couche::COLOREE }), reglages(Couche { teintee_zone, ..Couche::NOIRE })]
        };
        // BGRA prémultiplié du pixel (x, 0), sur la première rangée de l'ombre du haut.
        let pixel = |teintee_zone, portion, x: i32| {
            let ombres = ombres_de_la_barre(&barre, &couches(teintee_zone));
            let haut = ombres.iter().find(|o| o.cote == Cote::Haut).copied().unwrap();
            let px = pixels_de(&haut, portion);
            let i = (x * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        let (milieu, colonne) = (960, 10);
        let libre = pixel(true, Portion::Libre, milieu);
        assert!(libre[3] > 200, "l'ombre est là : {libre:?}");
        assert!(libre[2] > 150 && libre[0] == 0 && libre[1] == 0, "rouge comme sa barre : {libre:?}");
        let sur_colonne = pixel(true, Portion::Interface, colonne);
        assert!(sur_colonne[3] > 200 && sur_colonne[..3] == [0, 0, 0], "noire sur la colonne : {sur_colonne:?}");
        assert_eq!(pixel(false, Portion::Libre, milieu)[..3], [0, 0, 0], "décochée : noire comme avant");
    }

    /// ⭐ Demande du 2026-10-23 : la zone continue la couleur que montre le BORD de
    /// la colonne — l'ombre colorée de la barre du haut posée sur elle. Haut rouge,
    /// colonne gauche bleue, colorée du haut sur 100 px : contre la colonne, juste
    /// sous la barre du haut, la noire teintée tire au ROUGE ; plus bas, hors de la
    /// colorée du haut, elle reste BLEUE. Sur la version précédente (profil de la
    /// colonne seule), le premier point était bleu.
    #[test]
    fn la_zone_continue_la_couleur_posee_sur_le_bord_de_la_colonne() {
        let (rouge, bleu) = ([[220, 0, 0]; SEGMENTS], [[0, 0, 220]; SEGMENTS]);
        let teintes = [rouge, bleu, bleu, bleu];
        let barre = Barre { teintes, teintes_zone: teintes, ..barre_d_exemple() };
        let cotes = ReglagesCotes { active: true, largeur: 30.0, force: 1.0, ..ReglagesCotes::DEFAUT };
        let couches = [
            Reglages { haut: [100.0, 1.0], cotes: ReglagesCotes::DEFAUT, couche: Couche::COLOREE },
            Reglages { haut: [0.0, 0.0], cotes, couche: Couche { teintee_zone: true, ..Couche::NOIRE } },
        ];
        let ombres = ombres_de_la_barre(&barre, &couches);
        let gauche = ombres.iter().find(|o| o.cote == Cote::Gauche && o.interieur.is_some() && !o.couche.coloree).unwrap();
        // Contre le bord intérieur de la colonne gauche (x écran −1796), à `sy`.
        let pixel = |sy: i32| pixel_de(gauche, Portion::Libre, -1796 + 1 - gauche.x, sy - gauche.y);
        let haut_de_la_zone = 100 + 72;
        let [b, _, r, a] = pixel(haut_de_la_zone + 10);
        assert!(a > 150 && r > b, "sous la barre du haut : l'orange posé sur la colonne, ici le rouge — {:?}", [b, r, a]);
        let [b, _, r, _] = pixel(haut_de_la_zone + 500);
        assert!(b > 150 && r == 0, "hors de la colorée du haut : la colonne seule — {:?}", [b, r]);
    }

    /// Le calque compose la part sur l'interface : sur une colonne sous la barre
    /// du haut, la colorée assombrie par la noire posée PAR-DESSUS ; rien sur la
    /// zone libre, que la fenêtre couvre.
    #[test]
    fn le_calque_pose_la_noire_sur_la_coloree() {
        let barre = Barre { teintes: [[[200, 0, 0]; SEGMENTS]; 4], ..barre_d_exemple() };
        let haut = ombre_de(&barre, 24.0, 0.5).unwrap();
        let (coloree, noire) = (haut.en(Couche::COLOREE), haut.en(Couche::NOIRE));
        let taille = (1920, 1080);
        let compose = |ombres: &[Ombre]| {
            let mut pixels = vec![0u8; 1920 * 1080 * 4];
            let rects = composer(&mut pixels, barre.origine, taille, ombres);
            (pixels, rects)
        };
        let (px, rects) = compose(&[coloree, noire]);
        assert_eq!(rects, vec![[0, 72, 1920, 96]; 2]);
        let en = |px: &[u8], x: usize, y: usize| { let i = (y * 1920 + x) * 4; [px[i + 2], px[i + 1], px[i], px[i + 3]] };
        let [r, _, _, a] = en(&px, 10, 72);
        let (seule, _) = compose(&[coloree]);
        assert!(a > en(&seule, 10, 72)[3] && r > 0 && r < en(&seule, 10, 72)[0], "la noire assombrit la colorée : {:?}", en(&px, 10, 72));
        let (inverse, _) = compose(&[noire, coloree]);
        assert!(en(&inverse, 10, 72)[0] > r, "l'ordre compte : la dernière par-dessus");
        assert_eq!(en(&px, 960, 72), [0; 4], "la zone libre : à la fenêtre");
    }

    /// Les réglages de l'ombre colorée se relisent ; un fichier abîmé garde les
    /// valeurs par défaut, bornées.
    #[test]
    fn les_reglages_de_la_couleur_se_relisent() {
        let r = ReglagesCouleur { largeur_haut: 36.0, force_haut: 0.7, cotes: ReglagesCotes { active: true, largeur: 12.0, gauche: false, ..ReglagesCotes::DEFAUT } };
        assert_eq!(ReglagesCouleur::charger(&r.serialiser()), r);
        assert_eq!(ReglagesCouleur::charger(""), ReglagesCouleur::DEFAUT);
        let abime = ReglagesCouleur::charger("9999,5;");
        assert_eq!((abime.largeur_haut, abime.force_haut), (MAX_LARGEUR_COULEUR, 1.0));
    }

    /// ⭐ Les largeurs de l'ombre COLORÉE vont jusqu'à 2000 px — partout : sous la
    /// barre du haut, le long des autres barres, sur un écran secondaire. La noire
    /// reste bornée à `MAX_OMBRE_BARRE_HAUT`.
    #[test]
    fn les_largeurs_de_la_coloree_vont_jusqu_a_2000() {
        let c = ReglagesCouleur::charger("1500,0.5;1,1600,0.5,1,1,1,1,1700,0.5");
        assert_eq!((c.largeur_haut, c.cotes.largeur, c.cotes.largeur_bas), (1500.0, 1600.0, 1700.0));
        assert_eq!(ReglagesCouleur::charger("2500,0.5;1,2500").cotes.largeur, MAX_LARGEUR_COULEUR);
        let e = ReglagesEcran::charger("1500,0.5,1,1500,0.5").unwrap();
        assert_eq!((e.noire[0], e.couleur[0]), (crate::ui::dock::MAX_OMBRE_BARRE_HAUT, 1500.0));
        assert_eq!(ReglagesCotes::charger("1,1500").largeur, crate::ui::dock::MAX_OMBRE_BARRE_HAUT, "la noire");
    }

    /// ⭐ **L'ARC DU HAUT N'A PAS DE COUTURE** (capture du 2026-10-18) : contre la
    /// barre du haut, l'ombre de l'arc s'éteint, comme celle de la colonne juste
    /// après le carré du coin ; face à la colonne, elle reste pleine.
    #[test]
    fn l_arc_du_haut_s_efface_vers_la_barre_du_haut() {
        let i = Interieur { gauche: 0, droite: 1000, haut: 0, bas: 500, rayon: 40, ombrent: [true, true, true], coins: [true; 4], sur_le_bas: [false; 2], laterales: Trait { largeur: 30, force: 1000 }, dessous: Trait { largeur: 30, force: 1000 } };
        let (dedans, dehors) = (i.alpha(39.5, 0.5), i.alpha(40.5, 0.5));
        assert!((dedans - dehors).abs() < 0.02, "pas de saut au bout du carré : {dedans} / {dehors}");
        let face = i.alpha(0.5 + 1.0, 39.5);
        assert!(face > 0.8, "face à la colonne, l'arc ombre comme elle : {face}");
    }

    /// ⭐ **LES COINS DU BAS ÉPOUSENT L'ARC** (défaut signalé le 2026-10-18) : pas
    /// de marche aux limites du carré d'un coin du bas — même quand la barre du bas
    /// ombre plus fort que la colonne —, et le long de l'arc une ombre qui va
    /// doucement de celle de la colonne à celle du bas.
    #[test]
    fn les_coins_du_bas_n_ont_pas_de_marche() {
        let i = Interieur { gauche: 0, droite: 1000, haut: 0, bas: 500, rayon: 40, ombrent: [true, true, true], coins: [true; 4], sur_le_bas: [true; 2], laterales: Trait { largeur: 70, force: 400 }, dessous: Trait { largeur: 70, force: 800 } };
        for v in [41.0, 45.0, 55.0, 70.0, 79.0] {
            // Le haut du carré (y = 460) et son côté (x = 40), de part et d'autre.
            let (dedans, dehors) = (i.alpha(v - 40.0, 460.2), i.alpha(v - 40.0, 459.8));
            assert!((dedans - dehors).abs() < 0.01, "haut du carré, x = {}: {dedans} / {dehors}", v - 40.0);
            let (dedans, dehors) = (i.alpha(39.8, v + 420.0), i.alpha(40.2, v + 420.0));
            assert!((dedans - dehors).abs() < 0.01, "côté du carré, y = {}: {dedans} / {dehors}", v + 420.0);
        }
        let le_long = |deg: f32| {
            let t = deg.to_radians();
            i.alpha(40.0 - 38.0 * t.cos(), 460.0 + 38.0 * t.sin())
        };
        let (cote, milieu, bas) = (le_long(5.0), le_long(45.0), le_long(85.0));
        assert!(cote < milieu && milieu < bas, "de la colonne au bas : {cote} < {milieu} < {bas}");
    }

    /// ⛔ **PAS DE CROISSANT NOIR LE LONG DE L'ARC DU BAS** — DÉFAUT SIGNALÉ
    /// (2026-10-31) : « les ombres noires portées par les angles arrondis dans la
    /// zone centrale sont cassées ». Mesuré sur l'écran 4K : ~10 px de noir PUR
    /// contre l'arc des coins du bas, nulle part ailleurs. L'ombre de l'arc s'y
    /// superposait à celle du bord droit le plus proche — que l'arc REMPLACE. À
    /// même retrait de l'arc et traits égaux, la même ombre tout du long.
    #[test]
    fn l_arc_du_bas_garde_la_meme_ombre_tout_du_long() {
        let t = Trait { largeur: 70, force: 800 };
        let i = Interieur { gauche: 0, droite: 1000, haut: 0, bas: 500, rayon: 40, ombrent: [true, true, true], coins: [true; 4], sur_le_bas: [true; 2], laterales: t, dessous: t };
        for retrait in [1.0, 5.0, 15.0] {
            let le_long = |deg: f32| {
                let a = deg.to_radians();
                i.alpha(40.0 - (40.0 - retrait) * a.cos(), 460.0 + (40.0 - retrait) * a.sin())
            };
            let (cote, milieu, bas) = (le_long(1.0), le_long(45.0), le_long(89.0));
            assert!((milieu - cote).abs() < 0.01 && (milieu - bas).abs() < 0.01, "à {retrait} px de l'arc : {cote} / {milieu} / {bas}");
        }
    }

    /// ⭐ L'ombre suit l'ARRONDI des coins (défaut signalé le 2026-10-16 : « les
    /// coins arrondis du bas ont disparu ») : un congé ne reçoit rien, et le long
    /// de son arc l'ombre garde la même force que le long d'un bord droit.
    #[test]
    fn l_ombre_suit_l_arrondi_des_coins() {
        let i = Interieur { gauche: 0, droite: 1000, haut: 0, bas: 500, rayon: 40, ombrent: [true, true, true], coins: [true; 4], sur_le_bas: [false; 2], laterales: Trait { largeur: 30, force: 1000 }, dessous: Trait { largeur: 30, force: 1000 } };
        let a_distance = |d: f32| (1.0 - d / 30.0).powi(2);
        // Le coin du congé (bas-gauche), hors de l'arc : rien.
        assert_eq!(i.alpha(2.0, 498.0), 0.0);
        // Contre l'arc, sur sa diagonale : l'ombre vient de l'arc, pas des bords.
        let s = 40.0 - 40.0 / 2f32.sqrt();
        assert!(i.alpha(s + 1.0, 500.0 - s - 1.0) > a_distance(1.6));
        // Loin des coins : le bord le plus proche parmi ceux qui ombrent.
        assert_eq!(i.alpha(10.0, 250.0), a_distance(10.0));
        assert_eq!(i.alpha(500.0, 490.0), a_distance(10.0));
        let sans_bas = Interieur { ombrent: [true, true, false], ..i };
        assert_eq!(sans_bas.alpha(500.0, 490.0), 0.0, "le bas n'ombre pas");
        // Les pixels d'une bande du bas : le congé à 0, la ligne contre la barre, sombre.
        let r = ReglagesCotes { active: true, largeur: 30.0, force: 0.5, sur_le_bas: false, largeur_bas: 30.0, force_bas: 0.5, ..ReglagesCotes::DEFAUT };
        let barre = Barre { origine: (0, 0), largeur: 1000.0, hauteur: 560.0, bas: 0.0, gauche: 0.0, droite: 0.0, dessous: 60.0, rayon: 20.0, teintes: [[[0; 3]; SEGMENTS]; 4], teintes_zone: [[[0; 3]; SEGMENTS]; 4], texture: 0, ecran: None };
        let bas = ombres_des_cotes(&barre, &r).into_iter().find(|o| o.cote == Cote::Bas).unwrap();
        let px = pixels_de(&bas, Portion::Toute);
        let a = |x: i32, y: i32| px[((y * bas.largeur + x) * 4 + 3) as usize];
        assert_eq!(a(0, 29), 0, "le coin du congé");
        assert!(a(500, 29) > 100, "contre la barre du bas, au milieu");
    }

    /// ⭐ Le congé du bas prolonge la barre du bas : quand les colonnes ombrent
    /// celle-ci, il reçoit la même ombre, à l'horizontale depuis sa colonne
    /// (demande du 2026-10-16) ; sans la case, il reste net.
    #[test]
    fn le_conge_du_bas_recoit_l_ombre_de_sa_colonne() {
        let i = Interieur { gauche: 0, droite: 1000, haut: 0, bas: 500, rayon: 40, ombrent: [true, true, true], coins: [true; 4], sur_le_bas: [true, true], laterales: Trait { largeur: 30, force: 1000 }, dessous: Trait { largeur: 30, force: 1000 } };
        let a_distance = |d: f32| (1.0 - d / 30.0).powi(2);
        assert_eq!(i.alpha(2.0, 498.0), a_distance(2.0), "bas-gauche : depuis la colonne de gauche");
        assert_eq!(i.alpha(995.0, 498.0), a_distance(5.0), "bas-droite : depuis celle de droite");
        assert_eq!(i.alpha(2.0, 2.0), 0.0, "le congé du HAUT reste au niveau de sa colonne");
        assert_eq!(Interieur { sur_le_bas: [false; 2], ..i }.alpha(2.0, 498.0), 0.0);
        // Par les réglages : la case, la barre du bas et la colonne visibles.
        let r = ReglagesCotes { active: true, largeur: 60.0, force: 0.5, largeur_bas: 60.0, force_bas: 0.5, ..ReglagesCotes::DEFAUT };
        let barre = Barre { rayon: 40.0, ..barre_d_exemple() };
        let bas = ombres_des_cotes(&barre, &r).into_iter().find(|o| o.cote == Cote::Bas).unwrap();
        let px = pixels_de(&bas, Portion::Toute);
        let a = |x: i32, y: i32| px[((y * bas.largeur + x) * 4 + 3) as usize];
        assert!(a(1, bas.hauteur - 1) > 100, "le coin du congé, contre la colonne : sombre");
        let sans = ombres_des_cotes(&barre, &ReglagesCotes { sur_le_bas: false, ..r }).into_iter().find(|o| o.cote == Cote::Bas).unwrap();
        assert_eq!(pixels_de(&sans, Portion::Toute)[((sans.hauteur - 1) * sans.largeur * 4 + 3) as usize], 0);
    }

    /// ⭐ L'ombre du bas a SON réglage (demande du 2026-10-16) : sa largeur et sa
    /// force ne touchent pas celles des colonnes, et l'inverse.
    #[test]
    fn l_ombre_du_bas_se_regle_a_part() {
        let r = ReglagesCotes { active: true, largeur: 30.0, force: 0.5, largeur_bas: 80.0, force_bas: 0.9, sur_le_bas: false, ..ReglagesCotes::DEFAUT };
        let ombres = ombres_des_cotes(&barre_d_exemple(), &r);
        let bas = ombres.iter().find(|o| o.cote == Cote::Bas).unwrap();
        let gauche = ombres.iter().find(|o| o.cote == Cote::Gauche).unwrap();
        assert_eq!((bas.hauteur, gauche.largeur), (80, 30));
        let i = bas.interieur.unwrap();
        let milieu = (i.gauche + i.droite) as f32 / 2.0;
        assert!((i.alpha(milieu, i.bas as f32 - 0.5) - 0.9 * (1.0 - 0.5 / 80.0f32).powi(2)).abs() < 1e-4, "la force du bas");
        assert!(i.alpha(milieu, i.bas as f32 - 60.0) > 0.0, "à 60 px : encore dans les 80 du bas");
        assert_eq!(i.alpha(i.gauche as f32 + 40.0, 300.0), 0.0, "à 40 px : hors des 30 de la colonne");
        // Le bas éteint (largeur 0) : les colonnes vont jusqu'en bas.
        let sans_bas = ombres_des_cotes(&barre_d_exemple(), &ReglagesCotes { largeur_bas: 0.0, ..r });
        assert!(sans_bas.iter().all(|o| o.cote != Cote::Bas));
    }

    /// ⛔ Le carré du coin ne tranche pas dans l'ombre de la colonne (défaut signalé
    /// le 2026-10-16 : un carré CLAIR, l'ombre du bas plus étroite que celle des
    /// colonnes) : de part et d'autre de son bord haut, la même ombre.
    #[test]
    fn le_carre_du_coin_garde_l_ombre_de_la_colonne() {
        let i = Interieur {
            gauche: 0, droite: 1000, haut: 0, bas: 500, rayon: 33,
            ombrent: [true, true, true], coins: [true; 4], sur_le_bas: [true, true],
            laterales: Trait { largeur: 64, force: 700 }, dessous: Trait { largeur: 34, force: 700 },
        };
        for x in [1.0, 6.0, 20.0, 27.0] {
            let (dessus, dedans) = (i.alpha(x, 500.0 - 33.5), i.alpha(x, 500.0 - 32.5));
            assert!(dedans >= dessus - 0.02, "x = {x} : {dedans} dans le carré contre {dessus} au-dessus");
        }
        // Le congé lui-même : l'ombre de la colonne, pas plus.
        assert_eq!(i.alpha(2.0, 498.0), Trait { largeur: 64, force: 700 }.alpha(2.0));
        // Sans la case, le congé reste net, mais la zone libre du carré garde la colonne.
        let sans = Interieur { sur_le_bas: [false; 2], ..i };
        assert_eq!(sans.alpha(2.0, 498.0), 0.0);
        assert!(sans.alpha(20.0, 470.0) >= Trait { largeur: 64, force: 700 }.alpha(20.0));
    }

    /// Le fichier se relit ; abîmé ou ancien, il garde les valeurs par défaut.
    #[test]
    fn les_reglages_des_cotes_se_relisent() {
        let r = ReglagesCotes { active: true, largeur: 40.0, force: 0.6, gauche: false, droite: true, bas: false, sur_le_bas: false, largeur_bas: 12.0, force_bas: 0.25 };
        assert_eq!(r.serialiser(), "1,40,0.6,0,1,0,0,12,0.25");
        assert_eq!(ReglagesCotes::charger(&r.serialiser()), r);
        // Les anciens fichiers : la nouvelle case garde son défaut, et l'ombre du
        // bas reprend celle des colonnes.
        let ancien = ReglagesCotes { largeur_bas: 40.0, force_bas: 0.6, ..r };
        assert_eq!(ReglagesCotes::charger("1,40,0.6,0,1,0,0"), ancien);
        assert_eq!(ReglagesCotes::charger("1,40,0.6,0,1,0"), ReglagesCotes { sur_le_bas: true, ..ancien });
        assert_eq!(ReglagesCotes::charger(""), ReglagesCotes::DEFAUT);
        assert_eq!(ReglagesCotes::charger("1, 999, x"), ReglagesCotes { active: true, largeur: 200.0, largeur_bas: 200.0, ..ReglagesCotes::DEFAUT }, "borné");
    }

    /// L'ombre est collée SOUS la barre, sur toute la largeur de la fenêtre du
    /// dock — y compris un écran à gauche de la primaire (coordonnées négatives).
    #[test]
    fn l_ombre_est_collee_sous_la_barre_sur_toute_sa_largeur() {
        let barre = Barre { bas: 72.0, ..barre_d_exemple() };
        assert_eq!(ombre_de(&barre, 24.0, 0.35), Some(Ombre {
            x: -1920,
            y: 172,
            largeur: 1920,
            hauteur: 24,
            force: 350,
            cote: Cote::Haut,
            interieur: None,
            teintes: [[[0; 3]; SEGMENTS]; 4],
            etendues: [[-1920, 1920], [172, 1008], [172, 1008], [-1920 + 124, 1920 - 124 - 120]],
            couche: Couche::NOIRE,
            texture: 0,
            colonnes: [Some(-1920 + 124), Some(-1920 + 1920 - 120)],
            rayon: 0,
        }));
        assert_eq!(ombre_de(&barre, 0.0, 0.35), None, "hauteur 0 : éteinte");
        assert_eq!(ombre_de(&barre, 24.0, 0.0), None, "force 0 : éteinte");
        assert_eq!(ombre_de(&Barre { bas: 0.0, ..barre }, 24.0, 0.35), None, "pas de barre du haut");
    }

    /// Du noir prémultiplié, le plus opaque contre la barre, décroissant, et la
    /// même valeur sur toute une ligne — la courbe du shader.
    #[test]
    fn l_ombre_decroit_comme_dans_le_shader() {
        let (largeur, hauteur, force) = (5, 24, 0.35);
        let px = pixels_de_l_ombre(largeur, hauteur, force, Couche::NOIRE, Portion::Toute, Cote::Haut, |_, _| [0, 0, 0], None, [0, 0, largeur, hauteur]);
        assert_eq!(px.len(), (largeur * hauteur * 4) as usize);
        assert!(px.chunks_exact(4).all(|p| p[..3] == [0, 0, 0]), "du noir, prémultiplié");
        let alpha = |y: i32| px[((y * largeur) * 4 + 3) as usize];
        let attendu = |y: i32| {
            let t = 1.0 - (y as f32 + 0.5) / hauteur as f32;
            (force * t * t * 255.0).round() as u8
        };
        assert_eq!(alpha(0), attendu(0));
        assert_eq!(alpha(0), 86, "35 % contre la barre, à une demi-ligne près");
        assert!((0..hauteur - 1).all(|y| alpha(y) >= alpha(y + 1)), "décroissante");
        assert!(alpha(hauteur - 1) <= 1, "presque rien en bas");
        assert!((0..largeur).all(|x| px[(x * 4 + 3) as usize] == alpha(0)), "uniforme sur la ligne");
    }
}

#[cfg(test)]
mod tests_rang {
    use super::*;

    /// ⭐ Seuls les menus passent au-dessus des ombres — les nôtres comme ceux
    /// des autres applications ; le dock et nos panneaux restent dessous.
    #[test]
    fn seuls_les_menus_passent_au_dessus_des_ombres() {
        let [dock, panneau, menu, etranger, ombre] = [(); 5].map(|_| creer().unwrap());
        let haut = |h| unsafe { SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
        // L'ombre en tête, comme après l'ancienne remontée ; le panneau (un
        // Dash qu'on vient d'ouvrir) au-dessus du dock.
        let _ = (haut(dock), haut(panneau), haut(menu), haut(etranger), haut(ombre));

        ranger(&[ombre], &[menu], &[dock], &[panneau]);

        // Le clignotement : une ombre posée en tête, même un instant, passe
        // devant les menus. Aucune ne doit l'être quand un menu est visible.
        assert!(
            placements(&[ombre], &[menu], &[dock], None).iter().all(|&(_, sous)| sous != HWND_TOPMOST),
            "l'ombre ne passe jamais en tête pendant le rangement"
        );
        // Rang dans la pile, du haut vers le bas. La fenêtre IME du fil peut
        // s'intercaler entre le menu et l'ombre : on compare des RANGS.
        let rang = |cible| {
            let mut c = unsafe { windows::Win32::UI::WindowsAndMessaging::GetTopWindow(None) }.ok();
            let mut i = 0;
            while let Some(h) = c {
                if h == cible {
                    return i;
                }
                i += 1;
                c = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindow(h, windows::Win32::UI::WindowsAndMessaging::GW_HWNDNEXT) }.ok();
            }
            usize::MAX
        };
        assert!(rang(menu) < rang(ombre), "notre menu passe au-dessus de l'ombre");
        assert!(rang(etranger) < rang(ombre), "le menu d'une autre application aussi");
        assert!(rang(ombre) < rang(dock), "l'ombre au-dessus du dock");
        assert!(rang(dock) < rang(panneau), "le dock (et ses congés) au-dessus du Dash ouvert");
        for h in [dock, panneau, menu, etranger, ombre] {
            let _ = unsafe { DestroyWindow(h) };
        }
    }

    /// ⭐ **PENDANT UNE MAGNIFICATION, LE DOCK PASSE DEVANT TOUT** — DEMANDE
    /// EXPLICITE (2026-11-11) : « l'effet de magnification doit faire apparaître
    /// les icônes au-dessus des ombrages, des dashs et des menus ». L'icône
    /// agrandie est dessinée par la fenêtre du dock : elle n'est devant que si le
    /// dock l'est. Les menus restent devant les ombres, et le panneau reste
    /// derrière tout le monde.
    ///
    /// **MUTATION** : retirer le drapeau (l'ordre d'avant revient) — le dock
    /// retombe derrière l'ombre et le menu, et ce test le nomme.
    #[test]
    fn une_icone_agrandie_passe_devant_les_ombres_et_les_menus() {
        let [dock, autre_dock, panneau, menu, ombre] = [(); 5].map(|_| creer().unwrap());
        let haut = |h| unsafe { SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
        // L'état d'avant : l'ombre devant le dock, le panneau entre les deux.
        let _ = (haut(ombre), haut(menu), haut(panneau), haut(dock), haut(autre_dock));

        let ancien = crate::ui::dock::set_magnification_en_cours(true);
        ranger(&[ombre], &[menu], &[dock, autre_dock], &[panneau]);
        crate::ui::dock::set_magnification_en_cours(ancien);

        let rang = |cible| {
            let mut c = unsafe { windows::Win32::UI::WindowsAndMessaging::GetTopWindow(None) }.ok();
            let mut i = 0;
            while let Some(h) = c {
                if h == cible {
                    return i;
                }
                i += 1;
                c = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindow(h, windows::Win32::UI::WindowsAndMessaging::GW_HWNDNEXT) }.ok();
            }
            usize::MAX
        };
        // Les DEUX docks remontés, et l'icône (dans le dock) devant ombre, menu
        // et panneau — c'est la demande, dans les trois directions.
        assert!(rang(dock) < rang(menu), "un menu recouvre encore l'icône agrandie");
        assert!(rang(dock) < rang(ombre), "une ombre recouvre encore l'icône agrandie");
        assert!(rang(dock) < rang(panneau), "un dash recouvre encore l'icône agrandie");
        assert!(rang(autre_dock) < rang(menu), "le rangement ne vaut que pour un dock");
        // L'ordre demandé le 2026-10-23 tient toujours : menus devant les ombres.
        assert!(rang(menu) < rang(ombre), "les ombres sont repassées devant un menu");
        for h in [dock, autre_dock, panneau, menu, ombre] {
            let _ = unsafe { DestroyWindow(h) };
        }
    }
}
