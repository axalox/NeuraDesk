//! **LA SPHÈRE DES FENÊTRES** — DEMANDE du 2026-11-01 : « la même fonctionnalité
//! [que la vue sphère des terminaux] pour choisir vers quel programme ouvert
//! basculer ». Choix de l'utilisateur :
//! - « 1 b » : elle s'ouvre au CLIC MOLETTE sur le bureau (voir
//!   `win32::clic_molette_bureau`) ou sur la barre du bas (`app::run`) ;
//! - « 2 a » : les fenêtres du bureau virtuel COURANT, réduites comprises ;
//! - « 3 » : des aperçus VIVANTS ;
//! - « 4 » : le clic molette sur une carte BASCULE vers sa fenêtre et ferme la
//!   sphère ;
//! - « 5 » : par-dessus tout — et depuis le 2026-11-04, dans la ZONE CENTRALE
//!   de l'écran du clic (l'espace libre entre les barres, le même rectangle que
//!   les dashs), comme les panneaux Terminal/SSH : « ce serait peut-être mieux
//!   d'afficher la visualisation des programmes dans la zone centrale de
//!   NeuraDesk, comme pour les terminaux ».
//!
//! ⭐ **LES APERÇUS SONT DES VIGNETTES DWM** (`DwmRegisterThumbnail`), posées par
//! ce module dans la fenêtre de la page, à l'endroit que la page lui dit (POST
//! `fenetres/rects`, lu à chaque frame). Le compositeur les dessine en direct,
//! sans copie ni capture : vingt aperçus vivants ne coûtent rien, et les
//! fenêtres réduites y gardent leur dernière image (voir
//! `win32::desktop_previews`, où c'est mesuré). WGC aurait coûté une session par
//! fenêtre, et chaque session fermée gonfle le compteur GPU du processus (voir
//! `win32::ouverture_fenetres`).
//!
//! ⛔ **Une vignette DWM est un RECTANGLE DROIT** : elle ne se penche pas. Les
//! cartes de la page sont donc posées sur la sphère FACE À LA CAMÉRA (leur place
//! et leur taille suivent la perspective, pas leur inclinaison) — c'est ce qui
//! rend exact le rectangle donné au compositeur.
//!
//! Actions (`neuradesk://fenetres/…`) : `ready`, `basculer/<hwnd>`, `fermer`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmQueryThumbnailSourceSize, DwmRegisterThumbnail, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties,
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_RECTSOURCE, DWM_TNP_VISIBLE,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, GetWindowThreadProcessId, IsWindow, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE};

use crate::gfx::device::GfxDevice;
use crate::win32::cef_data;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};

/// La route POST où la page dépose les rectangles des aperçus.
const ROUTE_RECTS: &str = "fenetres/rects";
/// La liste des fenêtres se relit à ce rythme tant que la sphère est ouverte :
/// une fenêtre fermée ou ouverte entre-temps entre ou sort.
const RELIRE: Duration = Duration::from_secs(1);
/// Le côté des icônes envoyées à la page.
const COTE_ICONE: u32 = 48;
/// Au plus autant de cartes : au-delà, la sphère serait illisible.
const FENETRES_MAX: usize = 48;

// ---- Les réglages PROPRES à la sphère des programmes (2026-11-04) ----

/// **Le zoom PAR DÉFAUT**, en % : la distance de caméra à l'ouverture, donc la
/// taille des cartes. `100` = la sphère telle qu'elle s'ouvre historiquement
/// (caméra au niveau de la sphère). DEMANDE EXPLICITE : « régler le niveau de
/// zoom par défaut du mode prévisualisation des programmes ».
pub const ZOOM_DEFAUT_MIN: u32 = 50;
pub const ZOOM_DEFAUT_MAX: u32 = 200;
const ZOOM_DEFAUT_PAR_DEFAUT: u32 = 100;
const FICHIER_ZOOM_DEFAUT: &str = "sphere_fenetres_zoom_defaut.txt";

/// **La vitesse du zoom à la molette**, en % : `100` = le pas d'origine.
/// PROPRE à cette sphère depuis le 2026-11-04 (elle empruntait celle de la vue
/// sphère des terminaux). DEMANDE EXPLICITE : « ajoute pour la vue sphère des
/// programmes ouverts un réglage de vitesse du zoom ».
pub const VITESSE_ZOOM_MIN: u32 = 25;
pub const VITESSE_ZOOM_MAX: u32 = 400;
const VITESSE_ZOOM_PAR_DEFAUT: u32 = 100;
const FICHIER_VITESSE_ZOOM: &str = "sphere_fenetres_vitesse_zoom.txt";

/// **Le zoom AUTOMATIQUE** : au lieu de la distance fixe du zoom par défaut,
/// la caméra recule juste assez pour que TOUTES les cartes tiennent dans la
/// scène. DEMANDE EXPLICITE : « un zoom automatique activable dans les
/// réglages qui fasse s'afficher tous les programmes dans l'espace
/// disponible ». Il prime sur le zoom par défaut tant qu'il est coché.
const FICHIER_ZOOM_AUTO: &str = "sphere_fenetres_zoom_auto";

/// **La durée du fondu de la sphère** (apparition, disparition), en ms —
/// PROPRE à cette fenêtre, sans toucher au réglage global des fenêtres de
/// NeuraDesk (voir `ouverture_fenetres::regler_fondu_de_fenetre`).
pub const FONDU_MIN_MS: u32 = 50;
pub const FONDU_MAX_MS: u32 = 1200;
const FICHIER_FONDU: &str = "sphere_fenetres_fondu.txt";

/// Un entier dans les bornes lu depuis un fichier ou une page — `None` si
/// illisible ou hors bornes (même idiome que `cef_sphere::vitesse_valide`).
fn entier_dans(texte: &str, min: u32, max: u32) -> Option<u32> {
    let v = texte.trim().parse::<f64>().ok().filter(|v| v.is_finite())?;
    Some((v.round() as i64).clamp(i64::from(min), i64::from(max)) as u32)
}

/// Le zoom par défaut, lu une fois puis gardé.
pub fn zoom_defaut() -> u32 {
    if let Some(v) = ZOOM_DEFAUT.with(Cell::get) {
        return v;
    }
    let lu = crate::config::config_dir()
        .and_then(|d| std::fs::read_to_string(d.join(FICHIER_ZOOM_DEFAUT)).ok())
        .and_then(|t| entier_dans(&t, ZOOM_DEFAUT_MIN, ZOOM_DEFAUT_MAX))
        .unwrap_or(ZOOM_DEFAUT_PAR_DEFAUT);
    ZOOM_DEFAUT.with(|c| c.set(Some(lu)));
    lu
}

/// Règle le zoom par défaut (curseur des Réglages), l'enregistre, et le pousse
/// à la page ouverte — il s'appliquera à la PROCHAINE ouverture.
pub fn regler_zoom_defaut(texte: &str) {
    let Some(v) = entier_dans(texte, ZOOM_DEFAUT_MIN, ZOOM_DEFAUT_MAX) else {
        crate::diag!("[fenetres] zoom par defaut illisible : {texte:?}");
        return;
    };
    ZOOM_DEFAUT.with(|c| c.set(Some(v)));
    crate::config::write_config(FICHIER_ZOOM_DEFAUT, v.to_string());
    push_state();
}

/// La vitesse du zoom à la molette, en % — lue une fois, puis gardée.
pub fn vitesse_zoom() -> u32 {
    if let Some(v) = VITESSE_ZOOM.with(Cell::get) {
        return v;
    }
    let lu = crate::config::config_dir()
        .and_then(|d| std::fs::read_to_string(d.join(FICHIER_VITESSE_ZOOM)).ok())
        .and_then(|t| entier_dans(&t, VITESSE_ZOOM_MIN, VITESSE_ZOOM_MAX))
        .unwrap_or(VITESSE_ZOOM_PAR_DEFAUT);
    VITESSE_ZOOM.with(|c| c.set(Some(lu)));
    lu
}

/// Règle la vitesse du zoom à la molette (curseur des Réglages), l'enregistre,
/// et la pousse à la page ouverte — la molette l'applique aussitôt.
pub fn regler_vitesse_zoom(texte: &str) {
    let Some(v) = entier_dans(texte, VITESSE_ZOOM_MIN, VITESSE_ZOOM_MAX) else {
        crate::diag!("[fenetres] vitesse du zoom illisible : {texte:?}");
        return;
    };
    VITESSE_ZOOM.with(|c| c.set(Some(v)));
    crate::config::write_config(FICHIER_VITESSE_ZOOM, v.to_string());
    push_state();
}

/// Le zoom automatique est-il activé ? Lu une fois puis gardé (il ne change
/// que par les Réglages, qui mettent le cache à jour eux-mêmes).
pub fn zoom_auto() -> bool {
    if let Some(v) = ZOOM_AUTO.with(Cell::get) {
        return v;
    }
    let lu = crate::config::load_bool_setting(FICHIER_ZOOM_AUTO);
    ZOOM_AUTO.with(|c| c.set(Some(lu)));
    lu
}

/// Active ou coupe le zoom automatique (case des Réglages) et pousse l'état à
/// la sphère ouverte — elle recadre aussitôt, sans attendre la prochaine
/// ouverture.
pub fn regler_zoom_auto(texte: &str) {
    let actif = match texte.trim() {
        "1" | "true" | "oui" => true,
        "0" | "false" | "non" => false,
        autre => {
            crate::diag!("[fenetres] zoom automatique illisible : {autre:?}");
            return;
        }
    };
    ZOOM_AUTO.with(|c| c.set(Some(actif)));
    crate::config::save_bool_setting(FICHIER_ZOOM_AUTO, actif);
    push_state();
}

/// Les deux durées du fondu propre à la sphère : (apparition, disparition), ms.
pub fn fondu() -> (u32, u32) {
    if let Some(v) = FONDU.with(Cell::get) {
        return v;
    }
    let lu = crate::config::config_dir()
        .and_then(|d| std::fs::read_to_string(d.join(FICHIER_FONDU)).ok())
        .and_then(|t| {
            let mut lignes = t.lines();
            let apparition = lignes.next().and_then(|l| entier_dans(l, FONDU_MIN_MS, FONDU_MAX_MS))?;
            let disparition = lignes.next().and_then(|l| entier_dans(l, FONDU_MIN_MS, FONDU_MAX_MS))?;
            Some((apparition, disparition))
        })
        // Jamais réglées : les durées GLOBALES en vigueur — la sphère garde
        // donc le fondu qu'elle avait avant l'existence de ce réglage, et les
        // curseurs ne font que le AFFINER.
        .unwrap_or_else(|| {
            (
                crate::win32::ouverture_fenetres::duree_fondu_ouverture_ms(),
                crate::win32::ouverture_fenetres::duree_fondu_fermeture_ms(),
            )
        });
    FONDU.with(|c| c.set(Some(lu)));
    lu
}

/// Règle une des deux durées (`direction` : `ouverture` ou `fermeture`),
/// l'enregistre, et la pose sur la fenêtre si elle est déjà ouverte — la
/// disparition en profitera, l'apparition ne peut plus.
pub fn regler_fondu(direction: &str, texte: &str) {
    let Some(v) = entier_dans(texte, FONDU_MIN_MS, FONDU_MAX_MS) else {
        crate::diag!("[fenetres] duree de fondu illisible : {texte:?}");
        return;
    };
    let (mut apparition, mut disparition) = fondu();
    match direction {
        "ouverture" => apparition = v,
        "fermeture" => disparition = v,
        autre => {
            crate::diag!("[fenetres] direction de fondu inconnue : {autre:?}");
            return;
        }
    }
    FONDU.with(|c| c.set(Some((apparition, disparition))));
    crate::config::write_config(FICHIER_FONDU, format!("{apparition}\n{disparition}"));
    if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
        crate::win32::ouverture_fenetres::regler_fondu_de_fenetre(hwnd, apparition, disparition);
    }
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefFenetresWindow"),
    window_title: w!("NeuraDesk — Fenêtres"),
    html_file: "fenetres.html",
    wndproc: Some(wndproc),
    // « Par-dessus tout » : DEVANT le dock, comme les menus (voir
    // `cef_popup::sous_le_dock`, qui range les panneaux sous lui).
    above_a_magnified_dock: true,
    fades: false,
    fenetre: false,
};

/// Une fenêtre de la sphère.
#[derive(Debug, Clone, PartialEq)]
struct Fenetre {
    hwnd: isize,
    titre: String,
    exe: String,
    exe_path: String,
    reduite: bool,
}

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    static FENETRES: RefCell<Vec<Fenetre>> = const { RefCell::new(Vec::new()) };
    /// Les vignettes DWM vivantes : fenêtre source → vignette.
    static VIGNETTES: RefCell<Vec<(isize, isize)>> = const { RefCell::new(Vec::new()) };
    static ICONES: RefCell<HashMap<String, Option<String>>> = RefCell::new(HashMap::new());
    static DERNIERE_LECTURE: Cell<Option<Instant>> = const { Cell::new(None) };
    /// La fenêtre au premier plan AVANT l'ouverture : la carte choisie d'abord.
    static AVANT: Cell<isize> = const { Cell::new(0) };
    /// Jusqu'à quand réclamer le clavier après l'ouverture : un clic molette
    /// sur le bureau laisse l'Explorateur au premier plan, et Windows peut
    /// refuser le premier essai.
    /// Le dernier compte des vignettes posées (posées, visibles).
    static POSEES: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    static CLAVIER_JUSQUA: Cell<Option<Instant>> = const { Cell::new(None) };
    /// Les deux réglages propres à la sphère, lus une fois puis gardés.
    static ZOOM_DEFAUT: Cell<Option<u32>> = const { Cell::new(None) };
    static ZOOM_AUTO: Cell<Option<bool>> = const { Cell::new(None) };
    static VITESSE_ZOOM: Cell<Option<u32>> = const { Cell::new(None) };
    static FONDU: Cell<Option<(u32, u32)>> = const { Cell::new(None) };
}

/// La durée pendant laquelle la sphère réclame le clavier après l'ouverture.
const RECLAMER_LE_CLAVIER: Duration = Duration::from_millis(1500);

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

pub fn is_visible() -> bool {
    cef_popup::is_visible(&SLOT)
}

/// Le rectangle COMPLET de l'écran sous `(x, y)` — barres comprises.
/// Ouvre la sphère dans `zone` — la ZONE CENTRALE de l'écran du clic, calculée
/// par l'appelant avec `dash_rect` (voir `app::zone_centrale_pour_dock`) — ou la
/// ferme si elle est déjà ouverte.
pub fn ouvrir_ou_fermer(zone: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    if is_visible() {
        fermer();
        return;
    }
    AVANT.with(|a| a.set(unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }.0 as isize));
    CONTENT_SCALE.with(|c| c.set(content_scale));
    PAYLOAD.with(|p| *p.borrow_mut() = None);
    relire();
    cef_popup::toggle_with(&SLOT, &SPEC, zone, gfx, || {});
    if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
        // ⚠️ AVANT la première frame de l'animation : le fondu d'apparition lit
        // cette durée à chaque tick (voir `ouverture_fenetres`), et l'évènement
        // `Interne` n'est consommé qu'à la frame suivante.
        let (apparition, disparition) = fondu();
        crate::win32::ouverture_fenetres::regler_fondu_de_fenetre(hwnd, apparition, disparition);
        let _ = unsafe { SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
    }
    push_state();
    appeler("ouvrir", &[]);
    // ⛔ Pas sous le harnais : il tourne à côté de la session vivante, et
    // volerait le clavier de l'utilisateur.
    if std::env::var_os("NEURADESK_CAPTURE_FRAMES").is_none() {
        CLAVIER_JUSQUA.with(|c| c.set(Some(Instant::now() + RECLAMER_LE_CLAVIER)));
        reclamer_le_clavier();
    }
}

/// Donne le clavier à la sphère tant que le délai d'ouverture court et
/// qu'elle ne l'a pas.
fn reclamer_le_clavier() {
    let Some(jusqua) = CLAVIER_JUSQUA.with(Cell::get) else { return };
    if Instant::now() > jusqua || cef_popup::donner_le_clavier(&SLOT) {
        CLAVIER_JUSQUA.with(|c| c.set(None));
    }
}

/// Ferme la sphère, sans rien changer d'autre.
pub fn fermer() {
    CLAVIER_JUSQUA.with(|c| c.set(None));
    liberer_les_vignettes();
    cef_popup::hide_if_visible(&SLOT);
}

pub fn hide_if_visible() {
    fermer();
}

/// Un Alt-Tab, ou un clic dans une autre fenêtre, ferme la sphère (les
/// vignettes suivent à la frame d'après, voir `dispatch_actions`).
pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

/// Les fenêtres de la barre des tâches du bureau virtuel courant, sans les
/// nôtres, dans l'ordre de la pile (la plus récente d'abord).
fn fenetres_du_bureau() -> Vec<Fenetre> {
    let notre_pid = std::process::id();
    let gestionnaire = unsafe { CoCreateInstance::<_, IVirtualDesktopManager>(&VirtualDesktopManager, None, CLSCTX_ALL) }.ok();
    crate::win32::app_identity::open_windows()
        .into_iter()
        .filter(|f| {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(f.hwnd, Some(&mut pid)) };
            pid != notre_pid
        })
        .filter(|f| match &gestionnaire {
            // Sans gestionnaire (rare) : toutes, plutôt qu'aucune.
            None => true,
            Some(g) => unsafe { g.IsWindowOnCurrentVirtualDesktop(f.hwnd) }.map(|v| v.as_bool()).unwrap_or(true),
        })
        .take(FENETRES_MAX)
        .map(|f| Fenetre {
            hwnd: f.hwnd.0 as isize,
            reduite: unsafe { windows::Win32::UI::WindowsAndMessaging::IsIconic(f.hwnd) }.as_bool(),
            titre: f.title,
            exe: f.exe,
            exe_path: f.exe_path,
        })
        .collect()
}

fn icone(exe_path: &str) -> Option<String> {
    if exe_path.is_empty() {
        return None;
    }
    if let Some(trouvee) = ICONES.with(|i| i.borrow().get(exe_path).cloned()) {
        return trouvee;
    }
    let uri = crate::win32::cef_dash::file_icon_rgba(std::path::Path::new(exe_path), COTE_ICONE)
        .and_then(|(l, h, octets)| crate::win32::cef_dash::encode_rgba_to_data_uri(l, h, &octets));
    ICONES.with(|i| i.borrow_mut().insert(exe_path.to_string(), uri.clone()));
    uri
}

/// Relit la liste, puis tient les vignettes au même ensemble.
fn relire() {
    let fenetres = fenetres_du_bureau();
    FENETRES.with(|f| *f.borrow_mut() = fenetres);
    DERNIERE_LECTURE.with(|d| d.set(Some(Instant::now())));
    synchroniser_les_vignettes();
}

fn synchroniser_les_vignettes() {
    let Some(destination) = cef_popup::hwnd(&SLOT) else { return };
    let voulues: Vec<isize> = FENETRES.with(|f| f.borrow().iter().map(|f| f.hwnd).collect());
    VIGNETTES.with(|v| {
        let mut v = v.borrow_mut();
        v.retain(|(source, vignette)| {
            let garder = voulues.contains(source);
            if !garder {
                let _ = unsafe { DwmUnregisterThumbnail(*vignette) };
            }
            garder
        });
        for source in voulues {
            if v.iter().any(|(s, _)| *s == source) {
                continue;
            }
            match unsafe { DwmRegisterThumbnail(destination, HWND(source as _)) } {
                Ok(vignette) => v.push((source, vignette)),
                Err(erreur) => crate::diag!("[fenetres] vignette refusée pour {source:#x} : {erreur:?}"),
            }
        }
    });
}

fn liberer_les_vignettes() {
    VIGNETTES.with(|v| {
        for (_, vignette) in v.borrow_mut().drain(..) {
            let _ = unsafe { DwmUnregisterThumbnail(vignette) };
        }
    });
}

fn push_state() {
    let fenetres: Vec<serde_json::Value> = FENETRES.with(|f| {
        f.borrow()
            .iter()
            .map(|f| {
                json!({
                    "hwnd": f.hwnd.to_string(),
                    "titre": f.titre,
                    "exe": f.exe,
                    "reduite": f.reduite,
                    "icone": icone(&f.exe_path),
                })
            })
            .collect()
    });
    let payload = json!({
        "fenetres": fenetres,
        "avant": AVANT.with(Cell::get).to_string(),
        "contentScale": CONTENT_SCALE.with(Cell::get),
        // La vitesse du zoom à la molette, PROPRE à cette sphère (réglage).
        "sphereZoom": vitesse_zoom(),
        // Le niveau de zoom PAR DÉFAUT de cette sphère (réglage propre).
        "sphereZoomDefaut": zoom_defaut(),
        // Recadrer toutes les cartes dans la scène, plutôt que de se poser à
        // distance fixe.
        "sphereZoomAuto": zoom_auto(),
    })
    .to_string();
    if PAYLOAD.with(|p| p.borrow().as_deref() == Some(payload.as_str())) {
        return;
    }
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
    PAYLOAD.with(|p| *p.borrow_mut() = Some(payload));
}

fn appeler(fonction: &str, arguments: &[serde_json::Value]) {
    let arguments: Vec<String> = arguments.iter().map(serde_json::Value::to_string).collect();
    cef_popup::execute_js(&SLOT, &format!("window.fenetres && window.fenetres.{fonction}({})", arguments.join(",")));
}

/// Un rectangle d'aperçu donné par la page : en pixels de la fenêtre.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Trou {
    hwnd: isize,
    rect: [f32; 4],
    visible: bool,
}

/// **CE QUE LA PAGE ENVOIE** : les trous, et le rectangle de la SCÈNE (en
/// pixels de la fenêtre).
///
/// Pourquoi la scène : la page la découpe en HTML (`#scene`, `overflow:
/// hidden`), mais une vignette DWM est composée PAR-DESSUS la fenêtre entière
/// et **ignore ce découpage** — une carte qui passe sous le pied de page y
/// dessine quand même son aperçu, par-dessus le texte d'aide (DÉFAUT SIGNALÉ
/// le 2026-11-04 : « le contenu des fenêtres passe au-dessus de la barre où
/// est le texte »). Rouge à nous de la rogner, voir `rogner`.
#[derive(Debug, Default, Clone, PartialEq)]
struct Trous {
    /// `[x, y, largeur, hauteur]` de la scène, en pixels de la fenêtre.
    clip: Option<[f32; 4]>,
    trous: Vec<Trou>,
}

/// `{"clip":[0,0,3840,1900],"trous":[{"hwnd":"123","x":..,"y":..,"l":..,"h":..,"v":true}, …]}`
/// — une entrée illisible tombe, le reste passe.
fn lire_les_trous(octets: &[u8]) -> Trous {
    let Ok(serde_json::Value::Object(racine)) = serde_json::from_slice::<serde_json::Value>(octets) else { return Trous::default() };
    let nombre = |v: &serde_json::Value| v.as_f64().filter(|n| n.is_finite()).map(|n| n as f32);
    let clip = racine.get("clip").and_then(|c| {
        let x = nombre(c.get(0)?)?;
        let y = nombre(c.get(1)?)?;
        let largeur = nombre(c.get(2)?)?;
        let hauteur = nombre(c.get(3)?)?;
        (largeur > 0.0 && hauteur > 0.0).then_some([x, y, largeur, hauteur])
    });
    let trous = racine
        .get("trous")
        .and_then(serde_json::Value::as_array)
        .map(|liste| {
            liste
                .iter()
                .filter_map(|t| {
                    let hwnd = t.get("hwnd")?.as_str()?.parse::<isize>().ok()?;
                    let n = |cle: &str| t.get(cle).and_then(serde_json::Value::as_f64).filter(|v| v.is_finite()).map(|v| v as f32);
                    let rect = [n("x")?, n("y")?, n("l")?, n("h")?];
                    Some(Trou {
                        hwnd,
                        rect,
                        visible: t.get("v").and_then(serde_json::Value::as_bool).unwrap_or(false)
                            && rect[2] >= 1.0
                            && rect[3] >= 1.0,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Trous { clip, trous }
}

/// L'aperçu d'une fenêtre de `source` (largeur × hauteur) dans le trou `rect` :
/// à ses proportions, centré.
fn ajuster(source: (i32, i32), rect: [f32; 4]) -> RECT {
    let (sl, sh) = (source.0.max(1) as f32, source.1.max(1) as f32);
    let k = (rect[2] / sl).min(rect[3] / sh);
    let (l, h) = (sl * k, sh * k);
    let x = rect[0] + (rect[2] - l) / 2.0;
    let y = rect[1] + (rect[3] - h) / 2.0;
    RECT { left: x.round() as i32, top: y.round() as i32, right: (x + l).round() as i32, bottom: (y + h).round() as i32 }
}

/// **ROGNE LA VIGNETTE AU RECTANGLE DE LA SCÈNE, EN GARDANT L'ÉCHELLE** : la
/// destination est réduite, et la SOURCE l'est de la MÊME fraction. Réduire la
/// seule destination ferait ÉTIRER l'image par DWM pour la faire tenir dedans —
/// l'aperçu sortirait comprimé au lieu d'être coupé.
///
/// Rend `None` si rien ne reste visible (vignette masquée).
///
/// ⛔ **C'EST LE SEUL MOYEN DE RESPECTER UN DÉCOUPAGE HTML** : une vignette
/// DWM ne connaît pas le contenu de la fenêtre de destination, elle est
/// composée par-dessus — `overflow: hidden` sur `#scene` ne la coupe pas
/// (voir `Trous`).
fn rogner(destination: RECT, source: (i32, i32), clip: [f32; 4]) -> Option<(RECT, RECT)> {
    let (gauche, haut) = (clip[0], clip[1]);
    let (droite, bas) = (clip[0] + clip[2], clip[1] + clip[3]);
    let left = (destination.left as f32).max(gauche).ceil() as i32;
    let top = (destination.top as f32).max(haut).ceil() as i32;
    let right = (destination.right as f32).min(droite).floor() as i32;
    let bottom = (destination.bottom as f32).min(bas).floor() as i32;
    if right <= left || bottom <= top {
        return None;
    }
    let (largeur, hauteur) = ((destination.right - destination.left) as f32, (destination.bottom - destination.top) as f32);
    if largeur <= 0.0 || hauteur <= 0.0 {
        return None;
    }
    let (sl, sh) = (source.0.max(1), source.1.max(1));
    // Fraction d'un bord de la destination, reportée sur la source.
    let part = |bord: i32, origine: i32, total: f32, taille: i32| {
        let f = ((bord - origine) as f32 / total).clamp(0.0, 1.0);
        (taille as f32 * f).round() as i32
    };
    let brut = RECT {
        left: part(left, destination.left, largeur, sl),
        top: part(top, destination.top, hauteur, sh),
        right: part(right, destination.left, largeur, sl),
        bottom: part(bottom, destination.top, hauteur, sh),
    };
    // L'arrondi peut vider la source d'un côté ; DWM refuse alors la vignette.
    let origine = RECT {
        left: brut.left.clamp(0, sl - 1),
        top: brut.top.clamp(0, sh - 1),
        right: brut.right.clamp(brut.left + 1, sl),
        bottom: brut.bottom.clamp(brut.top + 1, sh),
    };
    Some((RECT { left, top, right, bottom }, origine))
}

fn poser_les_vignettes(envoi: &Trous) {
    let mut posees = (0usize, 0usize);
    VIGNETTES.with(|v| {
        for (source, vignette) in v.borrow().iter() {
            let trou = envoi.trous.iter().find(|t| t.hwnd == *source);
            let taille = unsafe { DwmQueryThumbnailSourceSize(*vignette) }.unwrap_or_default();
            let taille = (taille.cx.max(1), taille.cy.max(1));
            let plein = RECT { left: 0, top: 0, right: taille.0, bottom: taille.1 };
            let (visible, destination, origine) = match trou {
                Some(t) if t.visible => {
                    let destination = ajuster(taille, t.rect);
                    match envoi.clip.and_then(|clip| rogner(destination, taille, clip)) {
                        Some((destination, origine)) => (true, destination, origine),
                        // Rien de l'aperçu ne tombe dans la scène : il sort de
                        // l'écran (carte retournée), la vignette se cache.
                        None => (false, RECT::default(), plein),
                    }
                }
                _ => (false, RECT::default(), plein),
            };
            let proprietes = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_RECTSOURCE | DWM_TNP_VISIBLE | DWM_TNP_OPACITY,
                rcDestination: destination,
                // TOUJOURS posée, même sans rognage : une vignette qui a été
                // rognée une frame garderait sinon sa source réduite.
                rcSource: origine,
                opacity: 255,
                fVisible: visible.into(),
                ..Default::default()
            };
            match unsafe { DwmUpdateThumbnailProperties(*vignette, &proprietes) } {
                Ok(()) => {
                    posees.0 += 1;
                    posees.1 += usize::from(visible);
                }
                Err(erreur) => crate::diag!("[fenetres] vignette non posée pour {source:#x} : {erreur:?}"),
            }
        }
    });
    // La MESURE (voir [[absence-de-trace-nest-pas-une-preuve]]) : une ligne
    // quand le compte change, pas une par image.
    if POSEES.with(|p| p.replace(posees)) != posees {
        crate::diag!("[fenetres] vignettes posées : {} dont {} visibles", posees.0, posees.1);
    }
}

/// Bascule vers la fenêtre `hwnd` et ferme la sphère.
fn basculer(hwnd: isize) {
    let connue = FENETRES.with(|f| f.borrow().iter().any(|f| f.hwnd == hwnd));
    fermer();
    if connue && unsafe { IsWindow(Some(HWND(hwnd as _))) }.as_bool() {
        crate::win32::appicon::focus_window(HWND(hwnd as _));
    } else {
        crate::diag!("[fenetres] fenêtre inconnue ou disparue : {hwnd:#x}");
    }
}

/// À CHAQUE FRAME : les actions de la page, les rectangles des aperçus, et la
/// liste relue chaque seconde tant que la sphère est ouverte.
pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("fenetres/") else {
            crate::diag!("[fenetres] action inconnue : {action:?}");
            continue;
        };
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["ready"] => {
                PAYLOAD.with(|c| *c.borrow_mut() = None);
                push_state();
                if is_visible() {
                    appeler("ouvrir", &[]);
                }
            }
            ["basculer", hwnd] => match hwnd.parse::<isize>() {
                Ok(h) => basculer(h),
                Err(_) => crate::diag!("[fenetres] fenêtre illisible : {action:?}"),
            },
            ["fermer"] => fermer(),
            _ => crate::diag!("[fenetres] action inconnue : {action:?}"),
        }
    }
    if !is_visible() {
        if VIGNETTES.with(|v| !v.borrow().is_empty()) {
            liberer_les_vignettes();
        }
        return;
    }
    reclamer_le_clavier();
    if DERNIERE_LECTURE.with(Cell::get).is_none_or(|d| d.elapsed() >= RELIRE) {
        relire();
        push_state();
    }
    if let Some(octets) = cef_data::prendre(ROUTE_RECTS) {
        poser_les_vignettes(&lire_les_trous(&octets));
    }
}

pub fn render_if_visible(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::render_if_visible(&SLOT, tint_rgba, 0.0, cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

/// Voir `win32::cef_calendar_applet::debug_dump_backbuffer`
/// (`NEURADESK_DEBUG_OPEN=fenetres`).
pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// Clic SOURIS CEF direct, réservé au harnais (voir `cef_popup::debug_click`).
pub fn debug_click(x: i32, y: i32) {
    cef_popup::debug_click(&SLOT, x, y);
}

pub fn debug_middle_click(x: i32, y: i32) {
    cef_popup::debug_middle_click(&SLOT, x, y);
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

    const PAGE: &str = include_str!("../../assets/popups/fenetres.html");

    #[test]
    fn les_trous_illisibles_tombent() {
        let lus = lire_les_trous(
            br#"{"clip":[0,0,900,700],"trous":[{"hwnd":"66","x":10,"y":20,"l":300,"h":200,"v":true},{"hwnd":"x","x":1,"y":1,"l":1,"h":1},{"x":1},{"hwnd":"7","x":0,"y":0,"l":0,"h":5,"v":true}]}"#,
        );
        assert_eq!(lus.clip, Some([0.0, 0.0, 900.0, 700.0]));
        assert_eq!(lus.trous.len(), 2);
        assert_eq!(lus.trous[0], Trou { hwnd: 66, rect: [10.0, 20.0, 300.0, 200.0], visible: true });
        assert!(!lus.trous[1].visible, "un trou vide ne se montre pas");
        assert_eq!(lire_les_trous(b"pas du json"), Trous::default());
        // Un clip dégénéré s'ignore : mieux vaut ne rien rogner que tout cacher.
        assert_eq!(lire_les_trous(br#"{"clip":[0,0,0,10],"trous":[]}"#).clip, None);
    }

    /// ⛔ **LE DÉFAUT DU 2026-11-04** : une vignette DWM n'est pas coupée par
    /// la scène HTML, donc une carte qui passait sous le pied de page y
    /// dessinait son aperçu par-dessus le texte d'aide. Le rognage réduit la
    /// destination ET la source de la même fraction — réduire la seule
    /// destination ferait ÉTIRER l'image par DWM.
    #[test]
    fn un_apercu_qui_sort_de_la_scene_est_rogne_sans_etre_etire() {
        let dest = RECT { left: 100, top: 100, right: 300, bottom: 300 };
        // Entièrement dedans : rien ne bouge, source entière.
        let (d, s) = rogner(dest, (200, 200), [0.0, 0.0, 1000.0, 1000.0]).expect("visible");
        assert_eq!(d, dest);
        assert_eq!((s.left, s.top, s.right, s.bottom), (0, 0, 200, 200));
        // La scène s'arrête à y = 200 : on garde la moitié HAUTE de l'image,
        // et la destination garde sa largeur — coupée, pas comprimée.
        let (d, s) = rogner(dest, (200, 200), [0.0, 0.0, 1000.0, 200.0]).expect("visible");
        assert_eq!((d.left, d.top, d.right, d.bottom), (100, 100, 300, 200));
        assert_eq!((s.left, s.top, s.right, s.bottom), (0, 0, 200, 100));
        // Entièrement dehors : rien à montrer.
        assert_eq!(rogner(dest, (200, 200), [0.0, 0.0, 1000.0, 50.0]), None);
        assert_eq!(rogner(dest, (200, 200), [400.0, 0.0, 100.0, 1000.0]), None);
    }

    /// L'aperçu garde les proportions de la fenêtre et se centre dans son trou.
    #[test]
    fn l_apercu_garde_ses_proportions() {
        let r = ajuster((1920, 1080), [100.0, 50.0, 400.0, 400.0]);
        assert_eq!((r.left, r.right), (100, 500));
        assert_eq!(r.bottom - r.top, 225);
        assert_eq!(r.top, 138); // 50 + 87,5, arrondi
        let r = ajuster((500, 1000), [0.0, 0.0, 400.0, 400.0]);
        assert_eq!((r.right - r.left, r.bottom - r.top), (200, 400));
    }

    /// Les verbes émis par la page sont ceux du dispatch, et inversement.
    #[test]
    fn la_page_et_le_dispatch_parlent_la_meme_langue() {
        let dispatch = include_str!("cef_fenetres.rs");
        for verbe in ["ready", "basculer", "fermer"] {
            assert!(PAGE.contains(&format!("go(\"{verbe}\"")), "la page n'émet pas {verbe}");
            assert!(dispatch.contains(&format!("[\"{verbe}\"")), "le dispatch ne reçoit pas {verbe}");
        }
        assert!(PAGE.contains(&format!("poster(\"{ROUTE_RECTS}\"")), "la page ne donne pas ses rectangles");
        assert!(PAGE.contains("clip:"), "la page ne donne pas le rectangle de la scène à rogner");
        assert!(PAGE.contains("window.fenetres = {"));
        assert!(PAGE.contains("ouvrir()"), "Rust appelle window.fenetres.ouvrir");
    }

    /// Le clic molette sur une carte BASCULE (demande « 4 ») ; Échap ferme.
    #[test]
    fn le_clic_molette_bascule_et_echap_ferme() {
        assert!(PAGE.contains("e.button === 1"), "le clic molette n'est pas lu");
        assert!(PAGE.contains("\"Escape\""));
        // Une vignette DWM est un rectangle droit : aucune carte ne se penche.
        assert!(!PAGE.contains("rotateY("), "une carte inclinée ne coïnciderait plus avec son aperçu");
        assert!(!PAGE.contains("preserve-3d"));
    }

    /// Les deux réglages propres à la sphère sont câblés **DES DEUX CÔTÉS** :
    /// les curseurs de `settings.html`, les verbes du dispatch, et les clefs
    /// que la page de Réglages lit. Un curseur sans verbe, ou l'inverse, est
    /// un réglage qui ne fait rien — vu trois fois dans ce dépôt.
    #[test]
    fn les_reglages_propres_a_la_sphere_sont_cables_des_deux_cotes() {
        let reglages = include_str!("../../assets/popups/settings.html");
        let dispatch = include_str!("cef_settings.rs");
        for (cle, verbe) in [
            ("sphereFenetresZoom", "sphere-fenetres-zoom"),
            ("sphereFenetresVitesseZoom", "sphere-fenetres-vitesse-zoom"),
            ("sphereFenetresZoomAuto", "sphere-fenetres-zoom-auto"),
            ("sphereFenetresFonduOuverture", "sphere-fenetres-fondu"),
            ("sphereFenetresFonduFermeture", "sphere-fenetres-fondu"),
        ] {
            assert!(reglages.contains(cle), "la page de Réglages ne lit pas {cle}");
            assert!(reglages.contains(&format!("go(\"{verbe}\"")), "la page n'émet pas {verbe}");
            assert!(dispatch.contains(&format!("[\"{verbe}\"")), "le dispatch ne reçoit pas {verbe}");
        }
        assert!(
            PAGE.contains("state.sphereZoomAuto && cartes.size") && PAGE.contains("Math.min(zoomParDefaut(), cadrageAuto())"),
            "l'ouverture n'arbitre pas entre recadrage automatique et zoom par défaut"
        );
        assert!(PAGE.contains("state.sphereZoomDefaut"), "la sphère ne lit pas son zoom par défaut");
        // Le zoom automatique : lu de l'état, pris à l'ouverture (à la place du
        // zoom par défaut), et réappliqué quand la scène change.
        assert!(PAGE.contains("state.sphereZoomAuto"), "la sphère ne lit pas le zoom automatique");
        assert!(PAGE.contains("cadrageAuto()"), "l'ouverture n'applique pas le recadrage automatique");
        assert!(PAGE.contains("recadrerSiAuto()"), "la scène ne se recadre pas quand elle change");
        // La vitesse de la molette est désormais PROPRE à cette sphère : c'est
        // son réglage qui part dans l'état, plus celui des terminaux. (On lit le
        // fichier SANS ses tests : la présente assertion nomme l'ancien appel.)
        let complet = include_str!("cef_fenetres.rs");
        let rust = complet.split("#[cfg(test)]").next().unwrap_or(complet);
        assert!(rust.contains(r#""sphereZoom": vitesse_zoom()"#), "la vitesse n'est pas poussée");
        assert!(!rust.contains("cef_sphere::vitesse_zoom"), "la sphère des programmes emprunte encore la vitesse des terminaux");
        assert!(PAGE.contains("e.deltaY * rayon / 1500 * vitesseZoom()"), "la molette ignore la vitesse");
    }

    /// Les valeurs reçues sont BORNÉES, comme celles de la vue des terminaux :
    /// un fichier ou une page ne doit pas pouvoir régler un fondu absurde.
    #[test]
    fn les_reglages_propres_sont_bornees() {
        assert_eq!(entier_dans("120", 50, 200), Some(120));
        assert_eq!(entier_dans(" 500 ", 50, 200), Some(200), "au-delà du maximum");
        assert_eq!(entier_dans("-10", 50, 200), Some(50), "sous le minimum");
        assert_eq!(entier_dans("abc", 50, 200), None);
        assert_eq!(entier_dans("", 50, 200), None);
        assert_eq!(entier_dans("NaN", 50, 200), None);
    }

    /// Les fenêtres de NeuraDesk ne s'affichent pas (la sphère se montrerait
    /// elle-même), et seulement celles du bureau courant.
    #[test]
    fn ni_nos_fenetres_ni_celles_des_autres_bureaux() {
        let source = include_str!("cef_fenetres.rs");
        assert!(source.contains("pid != notre_pid"));
        assert!(source.contains("IsWindowOnCurrentVirtualDesktop"));
    }
}
