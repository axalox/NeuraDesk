//! **LA FENÊTRE DE L'APPLET « CAPTURE D'ÉCRAN »** — on y choisit quoi capturer
//! (tout l'écran, une zone, la webcam), puis on appuie sur « Capturer ».
//!
//! DEMANDE EXPLICITE (2026-11-04) : « applet capture d'écran même présentation
//! que capture vidéo, proposer capture webcam aussi ». Le clic sur l'applet ne
//! fige donc plus l'écran d'emblée : il ouvre une fenêtre de la MÊME famille que
//! celle de l'applet « Enregistrer » (`cef_enregistrer`, `enregistrer.html`) —
//! bande de titre, cartes, tuiles de source, pied — et c'est « Capturer » qui
//! agit.
//!
//! ⚠️ **LES DEUX CLICS OUVRENT CETTE FENÊTRE** (corrigé le 2026-11-04 : le clic
//! droit exécutait encore la capture immédiate d'avant, d'où « je ne vois pas la
//! fenêtre proposant les choix »). Le GAUCHE la donne sans l'interface, le DROIT
//! la donne **en gardant l'interface** — même partage que l'enregistreur
//! (`cef_enregistrer::GARDER_INTERFACE`) : la photo garde alors les barres et les
//! panneaux ouverts, et passe par `app::attente_du_clic_droit`.
//!
//! Trois sources, et trois chemins différents :
//! - **Écran** : `capture_ecran::demander_un_ecran` puis la phase de capture
//!   habituelle (`app::PhaseCapture`) — l'écran CHOISI, sans sélection ;
//! - **Zone** : la même phase, mais `capture_ecran::lancer` ouvre son sélecteur ;
//! - **Webcam** : une seule image, par GStreamer (`enregistrement::chaine_photo`),
//!   dans un SOUS-PROCESSUS comme l'enregistreur — un pilote de caméra qui plante
//!   ne doit pas emporter les barres.
//!
//! ⭐ **ET L'INCRUSTATION, COMME LA VIDÉO** (DEMANDE du 2026-11-04) : une case
//! « Webcam dans un coin » sur Écran et Zone. La vignette est prise par le même
//! `chaine_photo`, l'écran suit son chemin habituel (`app::run` le DÉTOURNE vers
//! `ecran_pour_incrustation`), et `avancer` assemble les deux avec
//! `enregistrement::composer_la_vignette` — la géométrie EXACTE du compositor de
//! la vidéo (`place_vignette`, proportions gardées).
//!
//! ⭐ **LA PHOTO PASSE PAR LE MÊME CHEMIN QUE LES AUTRES CAPTURES** : `app::run`
//! la draine avec `prendre_photo` et la remet à `livrer_la_capture`, exactement
//! comme un `Resultat::Image` de `capture_ecran`.
//!
//! ⭐ **ET LA CAPTURE PRISE REVIENT DANS CETTE FENÊTRE** (DEMANDE du 2026-11-04 :
//! « une fenêtre dans le même style pour choisir ce que je veux faire de la
//! capture une fois faite ») : `app::livrer_la_capture` appelle
//! `montrer_le_resultat`, et la page montre l'aperçu puis les actions — Copier,
//! Enregistrer, Annoter (éditeur), Reconnaître le texte (OCR), Épingler. Elle
//! REMPLACE la carte d'accès rapide GDI, qui n'existe plus.
//! ⚠️ Le réglage des Réglages garde son nom (`capture_carte`) et son sens :
//! coché, la fenêtre d'actions ; décoché, l'éditeur d'image directement.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;

use crate::win32::enregistrement::Coin;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetSystemMetrics, GetWindowRect, IsWindowVisible, SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST,
    SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WM_CLOSE, WM_NCHITTEST,
};

use crate::gfx::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec, PART_ECRAN};

thread_local! {
    static SLOT: PopupSlot = PopupSlot::new();
    /// Le dernier état poussé (même garde que `cef_enregistrer`).
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// La rafale de poussée — la page n'a pas fini de charger à l'ouverture.
    static RAFALE: Cell<u32> = const { Cell::new(0) };
    /// Les choix, lus une fois puis tenus ici.
    static CHOIX: RefCell<Option<Choix>> = const { RefCell::new(None) };
    /// `gst::installe`, relevé à l'ouverture — il touche le disque.
    static GST: Cell<bool> = const { Cell::new(false) };
    /// Les demandes de `app::run` : Écran et Zone, dans l'ordre des clics.
    static DEMANDES: RefCell<Vec<Demande>> = const { RefCell::new(Vec::new()) };
    /// **OUVERTE AU CLIC DROIT : L'INTERFACE RESTE** — le même partage que
    /// l'enregistreur (voir `cef_enregistrer::GARDER_INTERFACE`). La photo garde
    /// les barres et les panneaux ouverts ; `app::run` choisit alors
    /// `attente_du_clic_droit` au lieu de `Demandee`.
    static GARDER_INTERFACE: Cell<bool> = const { Cell::new(false) };
    /// La capture prête à recevoir ses actions — `None` = la fenêtre montre les
    /// choix de source.
    static RESULTAT: RefCell<Option<ImageCapturee>> = const { RefCell::new(None) };
    /// Le compte rendu de la dernière action (« Copiée »).
    static STATUT: RefCell<Option<String>> = const { RefCell::new(None) };
    /// L'image à ouvrir dans l'éditeur (« Annoter »), une seule fois — `app::run`.
    static A_ANNOTER: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    /// Le réglage « après une capture, proposer les actions » (carte ou éditeur).
    static CARTE_ACTIVE: Cell<Option<bool>> = const { Cell::new(None) };
    /// Les écrans relevés à l'ouverture (voir `relever_les_ecrans`).
    static ECRANS: RefCell<Vec<Ecran>> = const { RefCell::new(Vec::new()) };
}

// ⚠️ **HORS DES `thread_local!`** : la photo est prise sur un fil à part, qui
// écrirait dans SES propres copies — la boucle de l'app ne verrait jamais le
// résultat. Ces deux-là sont partagés par tout le processus.
static PHOTO: Mutex<Option<Result<Vec<u8>, String>>> = Mutex::new(None);
static PHOTO_EN_COURS: AtomicBool = AtomicBool::new(false);
static DERNIERE_ERREUR: Mutex<Option<String>> = Mutex::new(None);

// ⚠️ **L'INCRUSTATION ASSEMBLE DEUX IMAGES VENUES DE DEUX FILS DIFFÉRENTS** : la
// vignette est prise par le fil photo, l'écran arrive par `app::run`. D'où trois
// boîtes partagées et un assemblage fait par `avancer`, dans la boucle.
/// La vignette demandée : son coin, sa taille, et quand elle a commencé d'attendre.
struct IncrustationEnCours {
    coin: Coin,
    taille: u32,
    debut: Instant,
}
static INCRUSTATION: Mutex<Option<IncrustationEnCours>> = Mutex::new(None);
/// La photo de webcam, déposée par son fil.
static VIGNETTE: Mutex<Option<Result<Vec<u8>, String>>> = Mutex::new(None);
/// L'écran, déposé par `app::run` (`ecran_pour_incrustation`), avec sa zone.
static ECRAN_POUR_INCRUSTATION: Mutex<Option<(Vec<u8>, RECT)>> = Mutex::new(None);
/// Où la capture a été prise : la carte d'accès rapide s'y pose, même quand la
/// fenêtre de l'applet s'est refermée entre-temps.
static ZONE_FINALE: Mutex<Option<RECT>> = Mutex::new(None);
/// Au-delà, la vignette est abandonnée : une sélection annulée (Échap) ne laisse
/// jamais d'écran derrière elle, et ses quelques mégaoctets ne doivent pas
/// attendre indéfiniment.
const ATTENTE_INCRUSTATION: Duration = Duration::from_secs(120);

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCaptureWindow"),
    window_title: w!("NeuraDesk — Capture d'écran"),
    html_file: "capture.html",
    wndproc: Some(wndproc),
    above_a_magnified_dock: false,
    // `fades: false` est OBLIGATOIRE pour une fenêtre (voir `cef_kaomoji::SPEC`).
    fades: false,
    fenetre: true,
};

/// La taille de la page, en unités CSS — le contenu défile au-delà.
const LARGEUR_PAGE: i32 = 420;
const HAUTEUR_PAGE: i32 = 470;
/// Unités de page → pixels physiques : le zoom `CONTENT_ZOOM_FACTOR` (3×).
const PX_PAR_UNITE: i32 = 3;
/// Le fichier des choix (`clé=valeur`, une par ligne — même format que l'enregistreur).
const FICHIER_CHOIX: &str = "capture_applet.txt";
/// Le nom du réglage « rester au premier plan ».
const REGLAGE_EPINGLE: &str = "capture_applet_premier_plan";

// ---------------------------------------------------------------------------
// LES CHOIX
// ---------------------------------------------------------------------------

/// Ce qu'on capture, tel que la page le nomme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Ecran,
    Zone,
    Webcam,
}

impl Source {
    const TOUTES: [Source; 3] = [Source::Ecran, Source::Zone, Source::Webcam];

    pub fn nom(self) -> &'static str {
        match self {
            Source::Ecran => "ecran",
            Source::Zone => "zone",
            Source::Webcam => "webcam",
        }
    }

    fn depuis_nom(nom: &str) -> Option<Source> {
        Self::TOUTES.into_iter().find(|s| s.nom() == nom)
    }
}

/// Les choix persistés : la source, et la webcam retenue.
#[derive(Clone, Debug, PartialEq)]
pub struct Choix {
    pub source: Source,
    /// Le NOM de la webcam ; vide = la première trouvée.
    pub webcam: String,
    /// L'écran à capturer, par son `device_name` (`\\.\DISPLAY2`) — l'ordre de
    /// `EnumDisplayMonitors` bouge, le nom non (voir `win32::monitors`). Vide =
    /// le primaire.
    pub ecran: String,
    /// **LA WEBCAM DANS UN COIN DE LA PHOTO** (DEMANDE du 2026-11-04) — les MÊMES
    /// réglages que la vidéo (`enregistreur::Choix`) : la case, le coin, la taille.
    pub incruster: bool,
    pub coin: Coin,
    /// Largeur de la vignette, en pour cent de la largeur de l'image.
    pub taille: u32,
}

impl Default for Choix {
    fn default() -> Self {
        Self {
            source: Source::Zone,
            webcam: String::new(),
            ecran: String::new(),
            incruster: false,
            coin: Coin::BasDroit,
            taille: 25,
        }
    }
}

impl Choix {
    fn charger() -> Choix {
        let mut choix = Choix::default();
        let Some(texte) = crate::config::config_dir()
            .map(|d| d.join(FICHIER_CHOIX))
            .and_then(|p| std::fs::read_to_string(p).ok())
        else {
            return choix;
        };
        for ligne in texte.lines() {
            let Some((cle, valeur)) = ligne.split_once('=') else { continue };
            if cle == "source" {
                if let Some(source) = Source::depuis_nom(valeur.trim()) {
                    choix.source = source;
                }
            } else if cle == "webcam" {
                choix.webcam = valeur.trim().to_string();
            } else if cle == "ecran" {
                choix.ecran = valeur.trim().to_string();
            } else if cle == "incruster" {
                choix.incruster = matches!(valeur.trim(), "1" | "true" | "oui");
            } else if cle == "coin" {
                if let Some(coin) = Coin::depuis_nom(valeur.trim()) {
                    choix.coin = coin;
                }
            } else if cle == "taille" {
                if let Some(taille) = valeur.trim().parse().ok().filter(|t| crate::win32::enregistreur::TAILLES_VIGNETTE.contains(t)) {
                    choix.taille = taille;
                }
            }
        }
        choix
    }

    /// Enregistre, et rend `false` si le disque a refusé (l'appelant le DIT).
    fn enregistrer(&self) -> bool {
        let texte = format!(
            "source={}\nwebcam={}\necran={}\nincruster={}\ncoin={}\ntaille={}\n",
            self.source.nom(),
            self.webcam,
            self.ecran,
            self.incruster as u8,
            self.coin.nom(),
            self.taille
        );
        crate::config::config_dir()
            .map(|d| d.join(FICHIER_CHOIX))
            .is_some_and(|p| std::fs::write(p, texte).is_ok())
    }

    /// Applique un réglage de la page ; `false` si la clé ou la valeur est refusée.
    fn regler(&mut self, cle: &str, valeur: &str) -> bool {
        match cle {
            "source" => match Source::depuis_nom(valeur) {
                Some(s) => {
                    self.source = s;
                    true
                }
                None => false,
            },
            "webcam" => {
                self.webcam = valeur.to_string();
                true
            }
            "ecran" => {
                self.ecran = valeur.to_string();
                true
            }
            "incruster" => {
                self.incruster = matches!(valeur, "1" | "true" | "oui");
                true
            }
            "coin" => match Coin::depuis_nom(valeur) {
                Some(coin) => {
                    self.coin = coin;
                    true
                }
                None => false,
            },
            "taille" => match valeur.parse().ok().filter(|t| crate::win32::enregistreur::TAILLES_VIGNETTE.contains(t)) {
                Some(taille) => {
                    self.taille = taille;
                    true
                }
                None => false,
            },
            _ => false,
        }
    }
}

/// Une demande de `app::run`, une par clic sur « Capturer ».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Demande {
    /// Un écran précis, `[x, y, largeur, hauteur]` physiques — DEMANDE du
    /// 2026-11-04 : « je ne peux pas choisir quel écran et ils sont tous
    /// capturés ». Le rectangle voyage AVEC la demande ; il n'y a plus de
    /// « bureau virtuel entier » implicite.
    Ecran([i32; 4]),
    Zone,
}

// ---------------------------------------------------------------------------
// LA CAPTURE PRISE : LA MÊME FENÊTRE MONTRE SES ACTIONS
// ---------------------------------------------------------------------------

/// **LA CAPTURE PRISE, GARDÉE ICI** (DEMANDE du 2026-11-04 : « une fenêtre dans
/// le même style pour choisir ce que je veux faire de la capture une fois
/// faite »). Elle remplace la carte d'accès rapide GDI : c'est la MÊME fenêtre
/// que celle des choix, rouverte en mode résultat — un aperçu, et les actions.
#[derive(Clone)]
struct ImageCapturee {
    /// Le PNG d'origine, tel qu'il partira à l'enregistrement ou à l'éditeur.
    png: Vec<u8>,
    /// Le BGRA décodé : le presse-papiers et l'épingle le demandent.
    rgba: Vec<u8>,
    largeur: u32,
    hauteur: u32,
    /// L'aperçu en `data:` URI — une RÉDUCTION, jamais le 4K entier : une page
    /// CEF n'a pas à recevoir vingt mégaoctets de base64 pour 900 pixels de large.
    apercu: String,
}

/// Le côté max de l'aperçu, en pixels physiques.
const APERCU_LARGEUR_MAX: u32 = 900;
const APERCU_HAUTEUR_MAX: u32 = 506;

/// Un écran proposé par la source « Écran » (voir `relever_les_ecrans`).
#[derive(Clone, Debug)]
struct Ecran {
    /// Le `device_name` (`\\.\DISPLAY2`) — stable, lui, quand l'index ne l'est pas.
    id: String,
    /// « Écran 1 », « Écran 2 »… dans l'ordre de Windows.
    nom: String,
    /// `[x, y, largeur, hauteur]` dans le bureau virtuel.
    rect: [i32; 4],
    principal: bool,
}

// ---------------------------------------------------------------------------
// LA FENÊTRE
// ---------------------------------------------------------------------------

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// Voir `cef_kaomoji::recaler_si_il_ne_tient_plus` — ouverte sur la PRIMAIRE.
pub fn recaler_si_il_ne_tient_plus() {
    cef_popup::recaler_si_il_ne_tient_plus(&SLOT);
}

/// La taille de la fenêtre, en pixels physiques, plafonnée à `PART_ECRAN`.
fn taille(largeur_ecran: i32, hauteur_ecran: i32) -> [i32; 2] {
    let plafond_l = (largeur_ecran * PART_ECRAN / 100) / PX_PAR_UNITE;
    let plafond_h = (hauteur_ecran * PART_ECRAN / 100) / PX_PAR_UNITE;
    [LARGEUR_PAGE.min(plafond_l) * PX_PAR_UNITE, HAUTEUR_PAGE.min(plafond_h) * PX_PAR_UNITE]
}

/// **LE RECTANGLE D'OUVERTURE — AU CENTRE DE L'ÉCRAN PRIMAIRE**.
pub fn rect_pour_ecran_primaire() -> [i32; 4] {
    let (largeur, hauteur) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let [l, h] = cef_popup::taille_aux_textes(largeur, hauteur, taille);
    [((largeur - l) / 2).max(0), ((hauteur - h) / 2).max(0), l, h]
}

/// **AU CENTRE DE L'ÉCRAN OÙ LA CAPTURE A ÉTÉ PRISE** — la fenêtre des actions
/// se montre là où l'on vient de capturer, pas sur l'écran primaire (même règle
/// que la carte d'accès rapide qu'elle remplace).
fn rect_pour_zone(zone: RECT) -> [i32; 4] {
    let centre = POINT { x: (zone.left + zone.right) / 2, y: (zone.top + zone.bottom) / 2 };
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    let trouve = unsafe { GetMonitorInfoW(MonitorFromPoint(centre, MONITOR_DEFAULTTONEAREST), &mut info) };
    let travail = if trouve.as_bool() { info.rcWork } else { RECT { left: 0, top: 0, right: unsafe { GetSystemMetrics(SM_CXSCREEN) }, bottom: unsafe { GetSystemMetrics(SM_CYSCREEN) } } };
    let [l, h] = cef_popup::taille_aux_textes(travail.right - travail.left, travail.bottom - travail.top, taille);
    let x = travail.left + ((travail.right - travail.left - l) / 2).max(0);
    let y = travail.top + ((travail.bottom - travail.top - h) / 2).max(0);
    [x, y, l, h]
}

/// **APRÈS UNE CAPTURE, MONTRE-T-ON LA FENÊTRE DES ACTIONS ?** — sinon la capture
/// s'ouvre dans l'éditeur, comme avant. Le réglage garde son nom (`capture_carte`)
/// et son interrupteur dans les Réglages.
const REGLAGE_ACTIONS: &str = "capture_carte";

pub fn active() -> bool {
    CARTE_ACTIVE.with(|a| {
        a.get().unwrap_or_else(|| {
            let lu = crate::config::load_bool_setting_or(REGLAGE_ACTIONS, true);
            a.set(Some(lu));
            lu
        })
    })
}

pub fn activer(oui: bool) {
    CARTE_ACTIVE.with(|a| a.set(Some(oui)));
    crate::config::save_bool_setting(REGLAGE_ACTIONS, oui);
}

/// **LA FENÊTRE S'OUVRE SUR LA CAPTURE PRISE** — `app::livrer_la_capture` l'appelle
/// juste après le gel (ou la photo de webcam), à la place de la carte d'accès
/// rapide.
pub fn montrer_le_resultat(png: Vec<u8>, zone: RECT, gfx: &GfxDevice) {
    let Some((largeur, hauteur, rgba)) = crate::win32::clipboard::decode_png(&png) else {
        // ⛔ **UNE IMAGE ILLISIBLE NE DOIT PAS DISPARAÎTRE EN SILENCE** : le
        // journal dit ce qu'on perd, et la fenêtre n'affiche rien.
        crate::diag!("[capture-applet] PNG de capture illisible ({} octets)", png.len());
        return;
    };
    let apercu = data_uri_apercu(&rgba, largeur, hauteur);
    RESULTAT.with(|r| *r.borrow_mut() = Some(ImageCapturee { png, rgba, largeur, hauteur, apercu }));
    STATUT.with(|s| *s.borrow_mut() = None);
    let rect = rect_pour_zone(zone);
    crate::diag!("[capture-applet] resultat {largeur}×{hauteur} montre a {rect:?}");
    cef_popup::regler_zoom_textes(&SLOT, crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES), rect);
    cef_popup::montrer_fenetre(&SLOT, &SPEC, rect, gfx);
    RAFALE.with(|r| r.set(6));
    push_state();
}

/// La taille de l'aperçu : proportions gardées, jamais agrandi.
fn taille_apercu(largeur: u32, hauteur: u32) -> (u32, u32) {
    let echelle = (APERCU_LARGEUR_MAX as f32 / largeur.max(1) as f32)
        .min(APERCU_HAUTEUR_MAX as f32 / hauteur.max(1) as f32)
        .min(1.0);
    (((largeur as f32 * echelle).round() as u32).max(1), ((hauteur as f32 * echelle).round() as u32).max(1))
}

/// L'aperçu réduit, encodé en PNG puis en `data:` URI (voir `charger_octets` de
/// l'éditeur d'image : le même mécanisme, pour la même raison).
fn data_uri_apercu(rgba: &[u8], largeur: u32, hauteur: u32) -> String {
    let (l, h) = taille_apercu(largeur, hauteur);
    let reduit = crate::win32::epingle::reechantillonner(rgba, largeur, hauteur, l, h);
    let Some(image) = image::RgbaImage::from_raw(l, h, reduit) else { return String::new() };
    let mut octets = std::io::Cursor::new(Vec::new());
    if image.write_to(&mut octets, image::ImageFormat::Png).is_err() {
        return String::new();
    }
    format!("data:image/png;base64,{}", BASE64.encode(octets.into_inner()))
}

/// Le nom proposé à l'enregistrement, horodaté — celui de l'éditeur.
fn nom_de_capture() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("Capture {}-{:02}-{:02} {:02}-{:02}-{:02}.png", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

fn avec_image<R>(f: impl FnOnce(&ImageCapturee) -> R) -> Option<R> {
    RESULTAT.with(|r| r.borrow().as_ref().map(f))
}

fn poser_le_statut(texte: &str) {
    STATUT.with(|s| *s.borrow_mut() = Some(texte.to_string()));
    push_state();
}

/// L'image à ouvrir dans l'éditeur (« Annoter »), une seule fois — `app::run`.
pub fn prendre_resultat_a_annoter() -> Option<Vec<u8>> {
    A_ANNOTER.with(|a| a.try_borrow_mut().ok()?.take())
}

fn releve() {
    GST.with(|g| g.set(crate::win32::gst::installe()));
    if GST.with(Cell::get) {
        crate::win32::enregistreur::demander_la_liste(false);
    }
    relever_les_ecrans();
}

/// **LES ÉCRANS, RELUS ICI ET PAS À CHAQUE FRAME** — `monitors::enumerate`
/// énumère aussi les barres des tâches (`EnumWindows`) : le faire dans un état
/// reconstruit soixante fois par seconde serait du gaspillage pur, et l'état ne
/// change pas. On relit à l'ouverture de la fenêtre et sur « Actualiser ».
fn relever_les_ecrans() {
    let ecrans: Vec<Ecran> = crate::win32::monitors::enumerate()
        .into_iter()
        .enumerate()
        .map(|(i, m)| Ecran {
            id: m.device_name.clone(),
            nom: format!("Écran {}", i + 1),
            rect: [m.rect.left, m.rect.top, m.rect.right - m.rect.left, m.rect.bottom - m.rect.top],
            principal: m.is_primary,
        })
        .collect();
    if !ecrans.is_empty() {
        ECRANS.with(|e| *e.borrow_mut() = ecrans);
    }
}

/// Le rectangle de l'écran choisi — celui du réglage s'il est encore branché,
/// sinon le primaire, sinon le premier, sinon l'écran primaire de Windows.
///
/// ⚠️ **UN ÉCRAN DÉBRANCHÉ NE DOIT PAS FAIRE ÉCHOUER LA CAPTURE** : on retombe
/// sur le primaire plutôt que de capturer un rectangle fantôme.
fn zone_de_l_ecran_choisi() -> [i32; 4] {
    let c = choix();
    let ecrans = ECRANS.with(|e| e.borrow().clone());
    if !c.ecran.is_empty() {
        if let Some(e) = ecrans.iter().find(|e| e.id == c.ecran) {
            return e.rect;
        }
    }
    if let Some(e) = ecrans.iter().find(|e| e.principal).or_else(|| ecrans.first()) {
        return e.rect;
    }
    let (largeur, hauteur) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    [0, 0, largeur, hauteur]
}

/// Ouvre ou referme la fenêtre. Appelée par le bras de clic GAUCHE de l'applet.
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice) {
    ouvrir_ou_fermer(rect, gfx, false);
}

/// Le clic DROIT : la même fenêtre, l'interface gardée (voir `GARDER_INTERFACE`).
pub fn toggle_en_gardant_l_interface(rect: [i32; 4], gfx: &GfxDevice) {
    ouvrir_ou_fermer(rect, gfx, true);
}

/// Un clic dans le MÊME mode referme la fenêtre ouverte ; dans l'AUTRE, il
/// change de mode et la laisse ouverte — la règle déjà portée par
/// `cef_enregistrer::ouvrir_ou_fermer`.
fn ouvrir_ou_fermer(rect: [i32; 4], gfx: &GfxDevice, garder: bool) {
    let meme_mode = GARDER_INTERFACE.with(|g| g.replace(garder)) == garder;
    if cef_popup::is_visible(&SLOT) {
        if meme_mode {
            if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                cef_popup::cacher_fenetre(hwnd);
            }
        } else {
            push_state();
        }
        return;
    }
    ouvrir(rect, gfx);
}

/// Vrai si la fenêtre a été ouverte au clic droit : la photo garde alors
/// l'interface (voir `app::attente_du_clic_droit`).
pub fn garde_l_interface() -> bool {
    GARDER_INTERFACE.with(Cell::get)
}

pub fn hide_if_visible() {
    if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
        // ⛔ **SANS L'ANIMATION DE FERMETURE** — DEMANDE du 2026-11-04 (« je ne veux
        // pas voir apparaître la fenêtre des paramètres de capture sur la
        // capture »). Le gel suit de quelques millisecondes (mesuré : 20 ms), et
        // le fondu de fermeture dessine ENCORE la fenêtre pendant ~150 ms : elle
        // se retrouvait en fantôme dans l'image. Voir `ouverture_fenetres::oublier`.
        crate::win32::ouverture_fenetres::oublier(hwnd);
        cef_popup::cacher_fenetre(hwnd);
        // ⛔ **LE MASQUAGE SE VÉRIFIE** : c'est lui qui décide si la fenêtre est
        // dans l'image (le gel a lieu dans `capture_ecran::lancer`, APRÈS ceci).
        crate::diag!(
            "[capture-applet] masquee : hwnd {hwnd:?}, visible apres {}",
            unsafe { IsWindowVisible(hwnd).as_bool() }
        );
    }
}

/// **SONDE** : l'état de la fenêtre à l'instant du gel — `(visible, rectangle)`.
/// Appelée par `capture_ecran::copier_ecran` juste avant la copie : elle dit si
/// l'image peut la contenir, au lieu de le supposer.
pub fn etat_de_la_fenetre() -> Option<(bool, [i32; 4])> {
    let hwnd = cef_popup::hwnd(&SLOT)?;
    let mut cadre = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut cadre) }.ok()?;
    Some((
        unsafe { IsWindowVisible(hwnd).as_bool() },
        [cadre.left, cadre.top, cadre.right - cadre.left, cadre.bottom - cadre.top],
    ))
}

fn ouvrir(rect: [i32; 4], gfx: &GfxDevice) {
    crate::diag!("[capture-applet] ouverture : rect {rect:?}, interface gardee {}", garde_l_interface());
    // ⚠️ Une incrustation qui attendait encore son écran (sélection annulée) est
    // abandonnée : la fenêtre qui la demandait revient, c'est un nouveau départ.
    oublier_la_vignette();
    // ⚠️ La fenêtre revient sur ses CHOIX : une capture d'hier n'a plus rien à
    // faire sous la main, et son PNG n'a pas à rester en mémoire.
    RESULTAT.with(|r| *r.borrow_mut() = None);
    STATUT.with(|s| *s.borrow_mut() = None);
    releve();
    PAYLOAD.with(|p| *p.borrow_mut() = None);
    cef_popup::regler_zoom_textes(&SLOT, crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES), rect);
    cef_popup::montrer_fenetre(&SLOT, &SPEC, rect, gfx);
    RAFALE.with(|r| r.set(6));
    push_state();
}

fn choix() -> Choix {
    CHOIX.with(|c| c.borrow_mut().get_or_insert_with(Choix::charger).clone())
}

fn derniere_erreur() -> Option<String> {
    DERNIERE_ERREUR.lock().ok().and_then(|e| e.clone())
}

fn poser_l_erreur(message: Option<String>) {
    if let Ok(mut e) = DERNIERE_ERREUR.lock() {
        *e = message;
    }
}

fn modifier(f: impl FnOnce(&mut Choix)) {
    let mut nouveau = choix();
    f(&mut nouveau);
    if !nouveau.enregistrer() {
        crate::diag!("[capture-applet] reglages NON ecrits sur le disque");
    }
    CHOIX.with(|c| *c.borrow_mut() = Some(nouveau));
}

fn appliquer_le_premier_plan() {
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return };
    let rang = if crate::config::load_bool_setting(REGLAGE_EPINGLE) { Some(HWND_TOPMOST) } else { Some(HWND_NOTOPMOST) };
    unsafe {
        let _ = SetWindowPos(hwnd, rang, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

// ---------------------------------------------------------------------------
// L'ÉTAT DE LA PAGE
// ---------------------------------------------------------------------------

fn liste_en_json() -> serde_json::Value {
    match crate::win32::enregistreur::liste() {
        crate::win32::enregistreur::EtatListe::Prete(p) => serde_json::json!({
            "etat": "prete",
            "webcams": p.webcams.iter().map(|w| serde_json::json!({ "nom": w.nom, "id": w.id })).collect::<Vec<_>>(),
        }),
        crate::win32::enregistreur::EtatListe::Echec(e) => {
            serde_json::json!({ "etat": "echec", "message": e, "webcams": [] })
        }
        _ => serde_json::json!({ "etat": "en-cours", "webcams": [] }),
    }
}

fn etat_json() -> String {
    let c = choix();
    serde_json::json!({
        "epingle": crate::config::load_bool_setting(REGLAGE_EPINGLE),
        "gst": GST.with(Cell::get),
        "choix": {
            "source": c.source.nom(),
            "webcam": c.webcam,
            "ecran": c.ecran,
            "incruster": c.incruster,
            "coin": c.coin.nom(),
            "taille": c.taille,
        },
        "ecrans": ECRANS.with(|e| e.borrow().iter().map(|e| serde_json::json!({
            "id": &e.id,
            "nom": &e.nom,
            "largeur": e.rect[2],
            "hauteur": e.rect[3],
            "principal": e.principal,
        })).collect::<Vec<_>>()),
        "tailles": crate::win32::enregistreur::TAILLES_VIGNETTE,
        "liste": liste_en_json(),
        "enCours": PHOTO_EN_COURS.load(Ordering::SeqCst),
        "erreur": derniere_erreur(),
        "garderInterface": garde_l_interface(),
        // La capture prête, s'il y en a une : `null` = la fenêtre montre les choix.
        // ⛔ **SANS L'APERÇU** : l'état est reconstruit À CHAQUE FRAME pour savoir
        // s'il a changé, et y glisser un demi-mégaoctet de base64 ferait sérialiser
        // et comparer cette chaîne soixante fois par seconde. L'image part UNE
        // fois, à la demande de la page (verbe « apercu »).
        "resultat": RESULTAT.with(|r| r.borrow().as_ref().map(|i| serde_json::json!({
            "largeur": i.largeur,
            "hauteur": i.hauteur,
        }))),
        "statut": STATUT.with(|s| s.borrow().clone()),
    })
    .to_string()
}

/// Pousse l'état s'il a changé.
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

/// **À APPELER À CHAQUE FRAME** — la liste des webcams arrive en tâche de fond.
pub fn refresh_if_visible() {
    if !cef_popup::is_visible(&SLOT) {
        return;
    }
    appliquer_le_premier_plan();
    let restante = RAFALE.with(|r| r.get());
    if restante > 0 {
        RAFALE.with(|r| r.set(restante - 1));
        PAYLOAD.with(|p| *p.borrow_mut() = None);
    }
    push_state();
}

/// **LE DESSIN — SANS LUI, LA FENÊTRE EST INVISIBLE.**
pub fn render_if_visible(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

// ---------------------------------------------------------------------------
// CE QUE LA PAGE DEMANDE
// ---------------------------------------------------------------------------

/// **CE QUE LA PAGE DEMANDE.**
pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("capture/") else { continue };
        let mut morceaux = reste.split('/');
        let verbe = morceaux.next().unwrap_or("");
        match verbe {
            "deplacer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::deplacer_par_le_systeme(hwnd, "capture");
                }
            }
            "fermer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::cacher_fenetre(hwnd);
                }
            }
            "epingler" => {
                let actif = !crate::config::load_bool_setting(REGLAGE_EPINGLE);
                crate::config::save_bool_setting(REGLAGE_EPINGLE, actif);
                appliquer_le_premier_plan();
            }
            "choisir" => {
                // ⚠️ La VALEUR voyage percent-encodée (un nom de webcam porte
                // des espaces).
                let cle = morceaux.next().unwrap_or("").to_string();
                let valeur = crate::win32::cef_app::percent_decode(morceaux.next().unwrap_or(""));
                let mut accepte = false;
                modifier(|c| accepte = c.regler(&cle, &valeur));
                if !accepte {
                    crate::diag!("[capture-applet] reglage refuse : {cle}={valeur:?}");
                }
            }
            "actualiser" => {
                releve();
                crate::win32::enregistreur::demander_la_liste(true);
            }
            "capturer" => capturer(),
            // ── Les actions sur la capture PRISE (voir `ImageCapturee`) ──
            "copier" => {
                if avec_image(|i| crate::win32::clipboard::write_image(i.largeur, i.hauteur, &i.rgba)).is_some() {
                    poser_le_statut("Copiée dans le presse-papiers");
                }
            }
            "enregistrer" => enregistrer_le_resultat(),
            // L'aperçu, à la demande de la page — voir `etat_json` (hors du JSON).
            "apercu" => {
                if let Some(uri) = avec_image(|i| i.apercu.clone()).filter(|u| !u.is_empty()) {
                    cef_popup::execute_js(
                        &SLOT,
                        &format!("window.captureApercu && window.captureApercu({})", serde_json::json!(uri)),
                    );
                }
            }
            "annoter" => {
                if let Some(png) = avec_image(|i| i.png.clone()) {
                    A_ANNOTER.with(|a| *a.borrow_mut() = Some(png));
                    hide_if_visible();
                }
            }
            "texte" => {
                if let Some(png) = avec_image(|i| i.png.clone()) {
                    crate::win32::texte_capture::lancer(&png);
                    hide_if_visible();
                }
            }
            // ⚠️ **UN VERBE À PART** — « epingler » (juste au-dessus) colle la
            // FENÊTRE au premier plan : c'est la capture qu'on épingle ici, pas
            // la fenêtre.
            "epingler-capture" => {
                if avec_image(|i| crate::win32::epingle::epingler(i.largeur, i.hauteur, &i.rgba)).is_some() {
                    hide_if_visible();
                }
            }
            "ready" => push_state(),
            autre => crate::diag!("[capture-applet] verbe inconnu : {autre:?}"),
        }
    }
}

/// Le bouton « Capturer » : la demande part vers `app::run`, ou la photo démarre
/// ici même.
fn capturer() {
    if PHOTO_EN_COURS.load(Ordering::SeqCst) {
        return;
    }
    let c = choix();
    // ⛔ **TOUTE CAPTURE REPART DE ZÉRO** : sans cette remise à zéro, une vignette
    // restée d'une capture annulée s'approprierait (`ecran_pour_incrustation`)
    // l'écran de la capture SUIVANTE, qui ne l'a pas demandée.
    oublier_la_vignette();
    match c.source {
        // **L'ÉCRAN AVEC LA WEBCAM DANS UN COIN** — la vignette part d'abord (elle
        // met une à deux secondes à démarrer), l'écran se demande en même temps :
        // `avancer` assemble les deux quand elles sont là.
        Source::Ecran if c.incruster => match demarrer_la_vignette(&c) {
            Some(()) => DEMANDES.with(|d| d.borrow_mut().push(Demande::Ecran(zone_de_l_ecran_choisi()))),
            None => return,
        },
        Source::Zone if c.incruster => match demarrer_la_vignette(&c) {
            Some(()) => DEMANDES.with(|d| d.borrow_mut().push(Demande::Zone)),
            None => return,
        },
        Source::Ecran => {
            DEMANDES.with(|d| d.borrow_mut().push(Demande::Ecran(zone_de_l_ecran_choisi())));
        }
        Source::Zone => {
            DEMANDES.with(|d| d.borrow_mut().push(Demande::Zone));
        }
        Source::Webcam => {
            if !GST.with(Cell::get) {
                poser_l_erreur(Some("GStreamer n'est pas installé : la webcam en dépend.".to_string()));
                push_state();
                return;
            }
            let Some(nom) = webcam_choisie(&c) else {
                poser_l_erreur(Some("Aucune webcam trouvée.".to_string()));
                push_state();
                return;
            };
            poser_l_erreur(None);
            prendre_la_photo(nom);
        }
    }
}

/// Enregistre la capture en PNG, sous le nom proposé par l'éditeur de fichiers.
fn enregistrer_le_resultat() {
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return };
    let Some(chemin) = crate::win32::file_dialog::enregistrer_sous(
        hwnd,
        "Enregistrer la capture",
        &nom_de_capture(),
        "png",
        "Image PNG",
    ) else {
        return;
    };
    let Some(png) = avec_image(|i| i.png.clone()) else { return };
    match std::fs::write(&chemin, &png) {
        Ok(()) => poser_le_statut("Enregistrée"),
        Err(erreur) => {
            crate::diag!("[capture-applet] enregistrement impossible : {erreur}");
            poser_le_statut("Impossible d'enregistrer");
        }
    }
}

/// La webcam : celle qui est choisie si elle est encore branchée, sinon la
/// première — même règle que l'enregistreur (`enregistreur::webcam_choisie`).
fn webcam_choisie(c: &Choix) -> Option<String> {
    let crate::win32::enregistreur::EtatListe::Prete(p) = crate::win32::enregistreur::liste() else {
        return None;
    };
    if !c.webcam.is_empty() {
        if let Some(w) = p.webcams.iter().find(|w| w.id == c.webcam || w.nom == c.webcam) {
            return Some(w.nom.clone());
        }
    }
    p.webcams.first().map(|w| w.nom.clone())
}

/// Les demandes en attente — `app::run` les consomme une par frame.
pub fn prendre_demande() -> Option<Demande> {
    DEMANDES.with(|d| {
        let mut d = d.borrow_mut();
        if d.is_empty() {
            None
        } else {
            Some(d.remove(0))
        }
    })
}

/// La photo terminée, s'il y en a une — `app::run` la poste sur la carte
/// d'accès rapide ou dans l'éditeur.
pub fn prendre_photo() -> Option<Result<Vec<u8>, String>> {
    PHOTO.lock().ok()?.take()
}

// ---------------------------------------------------------------------------
// L'INCRUSTATION DE LA WEBCAM
// ---------------------------------------------------------------------------

/// Lance la photo de webcam qui servira de vignette. Rend `None` (après avoir
/// POSÉ l'erreur) si elle ne peut pas partir.
fn demarrer_la_vignette(c: &Choix) -> Option<()> {
    if !GST.with(Cell::get) {
        poser_l_erreur(Some("GStreamer n'est pas installé : l'incrustation en dépend.".to_string()));
        push_state();
        return None;
    }
    let Some(nom) = webcam_choisie(c) else {
        poser_l_erreur(Some("Aucune webcam trouvée pour l'incrustation.".to_string()));
        push_state();
        return None;
    };
    poser_l_erreur(None);
    if let Ok(mut i) = INCRUSTATION.lock() {
        *i = Some(IncrustationEnCours { coin: c.coin, taille: c.taille, debut: Instant::now() });
    }
    PHOTO_EN_COURS.store(true, Ordering::SeqCst);
    push_state();
    std::thread::Builder::new()
        .name("capture-vignette".into())
        .spawn(move || {
            let resultat = photo_du_nom(&nom);
            if let Err(erreur) = &resultat {
                crate::diag!("[capture-applet] vignette : {erreur}");
                poser_l_erreur(Some(erreur.clone()));
            }
            if let Ok(mut v) = VIGNETTE.lock() {
                *v = Some(resultat);
            }
            // La photo est finie : le bouton redevient actif même si l'écran, lui,
            // attend encore d'être choisi (une zone peut prendre une minute).
            PHOTO_EN_COURS.store(false, Ordering::SeqCst);
        })
        .ok();
    Some(())
}

/// Abandonne une incrustation en attente, quel qu'en soit le stade.
fn oublier_la_vignette() {
    if let Ok(mut i) = INCRUSTATION.lock() {
        *i = None;
    }
    if let Ok(mut v) = VIGNETTE.lock() {
        *v = None;
    }
    if let Ok(mut e) = ECRAN_POUR_INCRUSTATION.lock() {
        *e = None;
    }
}

/// **L'ÉCRAN ARRIVE ICI QUAND UNE VIGNETTE L'ATTEND** — `app::run` appelle ceci
/// pour chaque capture d'écran avant de la livrer. Rend l'image telle quelle
/// quand aucune incrustation n'est en cours ; sinon l'empoche (avec sa zone) et
/// rend `None` : c'est `avancer` qui livrera l'image composée, plus tard.
pub fn ecran_pour_incrustation(png: Vec<u8>, zone: RECT) -> Option<Vec<u8>> {
    let attend = INCRUSTATION.lock().map(|i| i.is_some()).unwrap_or(false);
    if !attend {
        return Some(png);
    }
    if let Ok(mut e) = ECRAN_POUR_INCRUSTATION.lock() {
        *e = Some((png, zone));
    }
    None
}

/// **L'ASSEMBLAGE — À APPELER À CHAQUE FRAME.** Quand la vignette ET l'écran sont
/// arrivés, la photo composée part par `prendre_photo`, le même chemin que les
/// autres captures.
pub fn avancer() {
    let Some(enc) = INCRUSTATION.lock().ok().and_then(|mut i| i.take()) else { return };
    if enc.debut.elapsed() > ATTENTE_INCRUSTATION {
        // ⛔ Une sélection annulée (Échap) ne rend jamais d'écran : sans cette
        // borne, la vignette et son PNG resteraient en mémoire pour toujours.
        if let Ok(mut v) = VIGNETTE.lock() {
            *v = None;
        }
        if let Ok(mut e) = ECRAN_POUR_INCRUSTATION.lock() {
            *e = None;
        }
        crate::diag!("[capture-applet] incrustation abandonnee : aucun ecran apres {} s", ATTENTE_INCRUSTATION.as_secs());
        return;
    }
    let vignette_prete = VIGNETTE.lock().map(|v| v.is_some()).unwrap_or(false);
    let ecran_pret = ECRAN_POUR_INCRUSTATION.lock().map(|e| e.is_some()).unwrap_or(false);
    if !vignette_prete || !ecran_pret {
        if let Ok(mut i) = INCRUSTATION.lock() {
            *i = Some(enc);
        }
        return;
    }
    let vignette = VIGNETTE.lock().ok().and_then(|mut v| v.take());
    let ecran = ECRAN_POUR_INCRUSTATION.lock().ok().and_then(|mut e| e.take());
    let (Some(vignette), Some((ecran, zone))) = (vignette, ecran) else { return };
    let image = match vignette {
        Ok(v) => match crate::win32::enregistrement::composer_la_vignette(&ecran, &v, enc.coin, enc.taille) {
            Ok(png) => png,
            Err(erreur) => {
                // ⛔ **UNE INCRUSTATION RATÉE NE PERD PAS LA CAPTURE** : l'écran
                // part seul, et l'erreur se lira à la prochaine ouverture.
                crate::diag!("[capture-applet] incrustation : {erreur}");
                poser_l_erreur(Some(format!("Incrustation : {erreur}")));
                ecran
            }
        },
        Err(erreur) => {
            crate::diag!("[capture-applet] vignette indisponible : {erreur}");
            poser_l_erreur(Some(erreur));
            ecran
        }
    };
    if let Ok(mut z) = ZONE_FINALE.lock() {
        *z = Some(zone);
    }
    if let Ok(mut p) = PHOTO.lock() {
        *p = Some(Ok(image));
    }
}

// ---------------------------------------------------------------------------
// LA PHOTO DE WEBCAM
// ---------------------------------------------------------------------------

/// Démarre la photo dans un FIL À PART : la caméra met une à deux secondes à
/// démarrer, et la boucle de l'app ne doit pas l'attendre.
fn prendre_la_photo(nom: String) {
    PHOTO_EN_COURS.store(true, Ordering::SeqCst);
    push_state();
    std::thread::Builder::new()
        .name("capture-photo".into())
        .spawn(move || {
            let resultat = photo_du_nom(&nom);
            if let Err(erreur) = &resultat {
                crate::diag!("[capture-applet] photo : {erreur}");
                poser_l_erreur(Some(erreur.clone()));
            }
            if let Ok(mut p) = PHOTO.lock() {
                *p = Some(resultat);
            }
            PHOTO_EN_COURS.store(false, Ordering::SeqCst);
        })
        .ok();
}

/// Une seule image de la webcam, par le moteur GStreamer déjà embarqué.
fn photo_du_nom(nom: &str) -> Result<Vec<u8>, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let identifiant = format!("{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    let dossier = std::env::temp_dir();
    let image = dossier.join(format!("neuradesk-photo-{identifiant}.png"));
    let chaine = dossier.join(format!("neuradesk-photo-{identifiant}.txt"));
    let _ = std::fs::remove_file(&image);
    std::fs::write(&chaine, crate::win32::enregistrement::chaine_photo(nom, &image.to_string_lossy()))
        .map_err(|e| format!("chaîne non écrite : {e}"))?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut enfant = std::process::Command::new(exe)
        .arg(crate::win32::gst::DRAPEAU)
        .arg(&chaine)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB)
        .spawn()
        .map_err(|e| format!("moteur non lancé : {e}"))?;
    // ⛔ **GARDER L'ENTRÉE OUVERTE.** `gst::faire_tourner` traduit la fermeture
    // de l'entrée standard par « stop » : la refermer couperait le flux avant la
    // première image. C'est le fin de flux (`num-buffers=1`) qui termine.
    let _entree = enfant.stdin.take();
    let debut = std::time::Instant::now();
    let delai = std::time::Duration::from_secs(15);
    loop {
        match enfant.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if debut.elapsed() < delai => std::thread::sleep(std::time::Duration::from_millis(50)),
            Ok(None) => {
                let _ = enfant.kill();
                let _ = enfant.wait();
                let _ = std::fs::remove_file(&chaine);
                return Err("la webcam n'a pas répondu (15 s)".to_string());
            }
            Err(e) => {
                let _ = std::fs::remove_file(&chaine);
                return Err(e.to_string());
            }
        }
    }
    let _ = std::fs::remove_file(&chaine);
    let octets = std::fs::read(&image).map_err(|e| format!("aucune image : {e}"));
    let _ = std::fs::remove_file(&image);
    let octets = octets?;
    if octets.len() < 8 || &octets[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("l'image n'est pas un PNG".to_string());
    }
    Ok(octets)
}

/// La zone d'écran qui sert à choisir l'écran de la carte d'accès rapide : le
/// centre de la fenêtre de l'applet (elle est sur l'écran qu'on regarde).
pub fn zone_de_la_fenetre() -> RECT {
    // ⭐ **LA CAPTURE INCRUSTÉE EST PRISE AILLEURS, ET SA ZONE EST LÀ OÙ ELLE A
    // ÉTÉ PRISE** — la fenêtre de l'applet, elle, s'est refermée avant le gel
    // (voir `app::run`) : la carte d'accès rapide se poserait au coin de l'écran.
    // Consommée : la photo suivante repart de la fenêtre.
    if let Some(zone) = ZONE_FINALE.lock().ok().and_then(|mut z| z.take()) {
        return zone;
    }
    if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
        let mut cadre = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut cadre) }.is_ok() {
            return cadre;
        }
    }
    RECT::default()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCHITTEST {
        let mut cadre = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut cadre) }.is_ok() {
            let rect = [cadre.left, cadre.top, cadre.right - cadre.left, cadre.bottom - cadre.top];
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            return LRESULT(code_pour_point(rect, (x, y)) as isize);
        }
    }
    if msg == WM_CLOSE {
        cef_popup::cacher_fenetre(hwnd);
        return LRESULT(0);
    }
    if let Some(resultat) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return resultat;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Le code de `WM_NCHITTEST` : les huit bords et coins, sinon le client. ⚠️ Une
/// copie de plus (voir `cef_kaomoji::code_pour_point`).
fn code_pour_point(rect: [i32; 4], (x, y): (i32, i32)) -> i32 {
    const BORD: i32 = 12;
    let (gauche, haut) = (x < rect[0] + BORD, y < rect[1] + BORD);
    let (droite, bas) = (x >= rect[0] + rect[2] - BORD, y >= rect[1] + rect[3] - BORD);
    match (gauche, droite, haut, bas) {
        (true, _, true, _) => 13,
        (_, true, true, _) => 14,
        (true, _, _, true) => 16,
        (_, true, _, true) => 17,
        (true, _, _, _) => 10,
        (_, true, _, _) => 11,
        (_, _, true, _) => 12,
        (_, _, _, true) => 15,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = include_str!("../../assets/popups/capture.html");

    /// ⭐ **LA PAGE ET RUST PARLENT LE MÊME LANGAGE** — chaque verbe de la page a
    /// son bras, et chaque bras est appelé par la page.
    #[test]
    fn les_verbes_de_la_page_sont_exactement_ceux_du_dispatch() {
        const VERBES: [&str; 13] = [
            "deplacer", "fermer", "epingler", "choisir", "actualiser", "capturer", "copier", "enregistrer", "apercu",
            "annoter", "texte", "epingler-capture", "ready",
        ];
        let mut vus: Vec<String> = Vec::new();
        for morceau in PAGE.split("go(\"").skip(1) {
            let verbe = morceau.split(['"', '/']).next().unwrap_or("").to_string();
            if !vus.contains(&verbe) {
                vus.push(verbe);
            }
        }
        for verbe in &vus {
            assert!(VERBES.contains(&verbe.as_str()), "verbe inconnu passé à `go` : {verbe:?}");
        }
        let source = include_str!("cef_capture.rs");
        let dispatch = &source[source.find("pub fn dispatch_actions").unwrap()..source.find("/// Le bouton « Capturer »").unwrap()];
        for attendu in &VERBES {
            assert!(dispatch.contains(&format!("\"{attendu}\"")), "aucun bras pour `{attendu}`");
            assert!(PAGE.contains(&format!("go(\"{attendu}\"")), "la page n'emet jamais `{attendu}`");
        }
    }

    /// **LES DEUX CLICS OUVRENT LA FENÊTRE** — le gauche sans l'interface, le
    /// droit avec. Le mode voyage dans l'état, la page le lit, et `app::run`
    /// choisit l'attente en conséquence.
    #[test]
    fn le_mode_interface_gardee_va_jusqu_a_la_page() {
        GARDER_INTERFACE.with(|g| g.set(true));
        assert!(garde_l_interface());
        let etat: serde_json::Value = serde_json::from_str(&etat_json()).unwrap();
        assert_eq!(etat["garderInterface"], serde_json::json!(true));
        GARDER_INTERFACE.with(|g| g.set(false));
        let etat: serde_json::Value = serde_json::from_str(&etat_json()).unwrap();
        assert_eq!(etat["garderInterface"], serde_json::json!(false));
        assert!(PAGE.contains("state.garderInterface"), "la page ignore le mode");
        // ⛔ **LE DÉFAUT RÉEL** (2026-11-04) : le clic DROIT sautait la fenêtre.
        // Les deux bras de clic doivent exister, et la demande doit choisir
        // l'attente selon le mode — sans quoi le droit refige l'écran d'emblée.
        let app = include_str!("../app.rs");
        assert!(app.contains("cef_capture::toggle("), "le clic gauche n'ouvre plus la fenêtre");
        assert!(app.contains("cef_capture::toggle_en_gardant_l_interface("), "le clic droit saute la fenêtre");
        assert!(app.contains("cef_capture::garde_l_interface()"), "la demande ignore le mode");
        assert!(app.contains("attente_du_clic_droit(Instant::now())"), "l'attente du clic droit a disparu");
    }

    /// **LA FENÊTRE NE DOIT PAS LAISSER DE FANTÔME DANS LA CAPTURE** — le gel
    /// suit le masquage de quelques millisecondes, et l'effet de fermeture la
    /// dessine encore (mesuré : 20 ms). Elle est donc retirée de l'animation
    /// AVANT d'être cachée.
    #[test]
    fn la_fenetre_ne_laisse_pas_de_fantome_dans_la_capture() {
        let source = include_str!("cef_capture.rs");
        let debut = source.find("pub fn hide_if_visible").expect("le masquage a disparu");
        let fin = source[debut..].find("fn etat_de_la_fenetre").map(|i| debut + i).unwrap_or(source.len());
        let masquage = &source[debut..fin];
        let oublier = masquage.find("ouverture_fenetres::oublier(hwnd)").expect("la fenêtre reste dans l'animation de fermeture");
        let cacher = masquage.find("cacher_fenetre(hwnd)").expect("le masquage a disparu");
        assert!(oublier < cacher, "il faut OUBLIER l'animation AVANT de cacher la fenêtre");
        // Et le gel mesure l'état réel de la fenêtre : la preuve, pas la croyance.
        let ecran = include_str!("capture_ecran.rs");
        assert!(ecran.contains("cef_capture::etat_de_la_fenetre()"), "le gel ne dit pas si la fenêtre est encore visible");
        // Et l'image lue est celle d'après la composition, pas celle d'avant.
        assert!(ecran.contains("DwmFlush"), "le gel peut lire une composition antérieure au masquage");
    }

    /// **LE CHOIX DE L'ÉCRAN ET LES RACCOURCIS D'AVANT** (DEMANDE du 2026-11-04) :
    /// « je ne peux pas choisir quel écran et ils sont tous capturés » ; et
    /// « ctrl + clic gauche ou ctrl + clic droit » pour l'ancien mécanisme.
    #[test]
    fn l_ecran_se_choisit_et_les_raccourcis_d_avant_reviennent() {
        // ⭐ La demande PORTE le rectangle : plus de « bureau virtuel entier ».
        match Demande::Ecran([-1920, 0, 1920, 1080]) {
            Demande::Ecran([x, y, l, h]) => assert_eq!((x, y, l, h), (-1920, 0, 1920, 1080)),
            Demande::Zone => panic!("mauvaise demande"),
        }
        assert!(PAGE.contains("detailEcran") && PAGE.contains("state.ecrans"), "la page ne propose aucun écran");
        assert!(PAGE.contains("choisir(\"ecran\""), "aucune ligne n'envoie le choix d'écran");
        let app = include_str!("../app.rs");
        assert!(app.contains("capture_ecran::demander_un_ecran("), "l'écran demandé n'est jamais transmis");
        // ⭐ Ctrl + clic : l'ancien mécanisme, sans passer par la fenêtre.
        assert!(app.contains("if ctrl_held {"), "Ctrl + clic gauche n'a pas son bras");
        assert!(app.contains("attente_du_clic_droit(Instant::now())"), "Ctrl + clic droit n'a pas son bras");
        // ⛔ **ET IL DOIT PASSER AVANT LE BRAS DES ESPACES INVISIBLES** — celui-ci
        // avale TOUS les clics droits avec Ctrl ; sans cette exception, le geste
        // sur l'icône de capture ne faisait rien (défaut réel signalé).
        let capture = app.find("else if ctrl_sur_la_capture").expect("Ctrl + clic droit n'est pas intercepté");
        let espaces = app.find("} else if ctrl_held {").expect("le bras des espaces invisibles a disparu");
        assert!(capture < espaces, "l'espace invisible avale encore le clic droit de la capture");
        // Et les écrans ne sont pas énumérés dans l'état de chaque frame.
        assert!(include_str!("cef_capture.rs").contains("fn relever_les_ecrans"), "les écrans ne sont jamais relus");
    }

    /// **LA CAPTURE PRISE MONTRE SES ACTIONS DANS LA MÊME FENÊTRE** (demande du
    /// 2026-11-04) : aperçu + cinq actions, et l'aiguillage vient bien d'ici.
    #[test]
    fn la_capture_prise_offre_ses_actions_dans_la_meme_fenetre() {
        assert!(PAGE.contains("afficherResultat") && PAGE.contains("state.resultat"), "la page ne montre pas la capture");
        for verbe in ["copier", "enregistrer", "annoter", "texte", "epingler-capture"] {
            assert!(PAGE.contains(&format!("go(\"{verbe}\"")), "action absente de la page : {verbe}");
        }
        // ⚠️ L'aperçu est une RÉDUCTION : la page ne reçoit jamais le 4K entier —
        // et elle le DEMANDE, parce qu'il ne voyage pas dans l'état.
        assert!(PAGE.contains("r.largeur") && PAGE.contains("r.hauteur"), "les dimensions ne sont pas dites");
        assert!(PAGE.contains("go(\"apercu\")") && PAGE.contains("captureApercu"), "l'aperçu n'est jamais demandé");
        // ⛔ L'ÉTAT EST RECONSTRUIT À CHAQUE FRAME : pas d'aperçu dedans.
        RESULTAT.with(|r| {
            *r.borrow_mut() = Some(ImageCapturee {
                png: vec![1, 2, 3],
                rgba: vec![0; 4],
                largeur: 7,
                hauteur: 9,
                apercu: "data:image/png;base64,AAAA".to_string(),
            })
        });
        let etat = etat_json();
        assert!(!etat.contains("data:image"), "l'aperçu ne doit pas entrer dans l'état de chaque frame");
        assert!(etat.contains("\"largeur\":7") && etat.contains("\"hauteur\":9"), "les dimensions manquent : {etat}");
        RESULTAT.with(|r| *r.borrow_mut() = None);
        let app = include_str!("../app.rs");
        assert!(app.contains("cef_capture::montrer_le_resultat("), "une capture ne montre pas ses actions");
        assert!(app.contains("cef_capture::prendre_resultat_a_annoter()"), "Annoter n'ouvre pas l'éditeur");
        assert!(app.contains("cef_capture::active()"), "le réglage n'est pas lu");
        assert_eq!(taille_apercu(3840, 2160), (900, 506), "l'aperçu doit tenir dans ses bornes");
        assert_eq!(taille_apercu(320, 200), (320, 200), "un petit aperçu n'est jamais agrandi");
    }

    /// **LA WEBCAM DANS UN COIN** — les mêmes réglages que la vidéo : la case, le
    /// coin, la taille ; et la page les lit.
    #[test]
    fn l_incrustation_fait_l_aller_retour_par_le_fichier() {
        let mut c = Choix::default();
        assert!(!c.incruster, "off par défaut");
        assert_eq!((c.coin, c.taille), (Coin::BasDroit, 25), "les défauts de la vidéo");
        for coin in Coin::TOUS {
            assert!(c.regler("coin", coin.nom()));
            assert_eq!(c.coin, coin);
        }
        assert!(!c.regler("coin", "milieu"));
        assert!(c.regler("incruster", "1"));
        assert!(c.incruster);
        assert!(c.regler("incruster", "0"));
        assert!(!c.incruster);
        for taille in crate::win32::enregistreur::TAILLES_VIGNETTE {
            assert!(c.regler("taille", &taille.to_string()));
            assert_eq!(c.taille, taille);
        }
        assert!(!c.regler("taille", "99"), "une taille hors des choix doit être refusée");
        assert!(PAGE.contains("carteIncrustation") && PAGE.contains("state.tailles"), "la page n'offre pas la vignette");
        let source = include_str!("cef_capture.rs");
        let app = include_str!("../app.rs");
        assert!(app.contains("cef_capture::ecran_pour_incrustation("), "l'écran n'est jamais détourné vers la vignette");
        assert!(app.contains("cef_capture::avancer()"), "rien n'assemble la vignette");
        assert!(source.contains("composer_la_vignette"), "aucune composition");
    }

    /// La page porte les TROIS sources, et son bouton les envoie toutes.
    #[test]
    fn la_page_propose_les_trois_sources() {
        for source in Source::TOUTES {
            assert!(PAGE.contains(source.nom()), "la source {} manque", source.nom());
        }
        assert!(PAGE.contains("go(\"capturer\")"), "aucun bouton Capturer");
        assert!(PAGE.contains("state.liste"), "la liste des webcams n'est pas lue");
    }

    /// Les trois sources survivent au voyage par le fichier de choix.
    #[test]
    fn les_sources_font_l_aller_retour_par_le_fichier() {
        for source in Source::TOUTES {
            assert_eq!(Source::depuis_nom(source.nom()), Some(source));
            let mut c = Choix::default();
            assert!(c.regler("source", source.nom()));
            assert_eq!(c.source, source);
        }
        assert_eq!(Source::depuis_nom("tout"), None);
        let mut c = Choix::default();
        assert!(!c.regler("inconnue", "x"));
        assert!(c.regler("webcam", "Web Camera"));
        assert_eq!(c.webcam, "Web Camera");
    }

    /// La fenêtre tient sur l'écran, comme celle de l'enregistreur.
    #[test]
    fn la_fenetre_tient_sur_l_ecran() {
        for (l_ecran, h_ecran) in [(3840, 2160), (1920, 1080), (1366, 768)] {
            let [l, h] = taille(l_ecran, h_ecran);
            assert!(l <= l_ecran * PART_ECRAN / 100 && h <= h_ecran * PART_ECRAN / 100);
        }
    }

    /// Le drapeau du sous-processus photo : une seule image, PNG, et l'entrée
    /// standard qui reste OUVERTE (voir `photo_du_nom`).
    #[test]
    fn la_photo_est_une_seule_image_png() {
        let chaine = crate::win32::enregistrement::chaine_photo("Web Camera", "C:/temp/photo.png");
        assert!(chaine.contains("num-buffers=1"), "la caméra ne rendrait pas la main : {chaine}");
        assert!(chaine.contains("pngenc"), "pas de PNG : {chaine}");
        assert!(chaine.contains("mfvideosrc device-name="), "une autre source que la webcam : {chaine}");
        let source = include_str!("cef_capture.rs");
        assert!(source.contains("enfant.stdin.take()"), "l'entrée fermée ferait un « stop » immédiat");
    }
}
