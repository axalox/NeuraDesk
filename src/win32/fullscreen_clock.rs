//! Petite barre flottante affichant SEULEMENT l'horloge, montrée par-dessus
//! une application plein écran (jeu, vidéo...) à la place du masquage total
//! du dock — DEMANDE EXPLICITE : "j'aimerais que quand s'affiche une fenêtre
//! en plein écran il y ait la possibilité de n'afficher qu'une petite barre
//! centrée en haut ou en bas de l'écran avec l'horloge uniquement et laisser
//! la possibilité à l'utilisateur d'activer cette fonction, de choisir
//! l'emplacement et de régler la taille de cette barre et du texte de cette
//! horloge". Désactivée par défaut (voir `config::load_fullscreen_clock_enabled`)
//! : tant qu'elle ne l'est pas, le comportement reste celui d'avant ce
//! réglage (masquage total, voir `win32::window::Window::sync_fullscreen_visibility`).
//!
//! DEUXIÈME DEMANDE EXPLICITE ("pour le moment l'horloge ne ressemble pas à
//! la barre supérieure, fais en sorte qu'elle aie l'apparence d'une mini
//! menu qui avec les mêmes réglages que pour le reste des menus") : une
//! première version dessinait un simple pavé GDI opaque (fond plat, texte
//! `DrawTextW`) — remplacée ici par EXACTEMENT le même procédé que
//! `win32::tooltip::TooltipWindow` ("transforme les popup en leur donnant la
//! même apparence que les menus" appliqué là-bas, voir sa doc de module) :
//! fond du bureau capturé et flouté (`gfx::backdrop::capture_blurred_batch`),
//! teinté avec la couleur/force des menus (`win32::menu::menu_accent_rgb`/
//! `menu_tint`), coins vraiment arrondis au MÊME rayon (`win32::menu::
//! menu_corner_radius`) via `SetWindowRgn`. Seules l'activation/l'emplacement
//! (haut/bas) et les deux facteurs d'échelle (barre/texte) restent des
//! réglages PROPRES à cette mini-barre (voir `is_enabled`/`bar_scale`/
//! `text_scale` ci-dessous) — l'apparence (couleur, flou, arrondi), elle,
//! suit désormais fidèlement celle des menus plutôt que d'être dupliquée.
//!
//! TROISIÈME DEMANDE EXPLICITE ("pouvoir régler individuellement les
//! horloges en plein écran en fonction de l'écran où elles s'affichent, je
//! ne veux pas un réglage unique pour toutes les horloges en mode plein
//! écran") : `bar_scale`/`position_top` sont désormais des PARAMÈTRES
//! explicites de `update` (résolus par écran par `app::rebuild_secondary_docks`
//! dans `dock_instance::DockInstance::fullscreen_clock_*`, voir sa doc)
//! plutôt que lus depuis les accesseurs globaux ci-dessous — qui restent la
//! source de vérité UNIQUEMENT pour la barre PRIMAIRE (voir `app::run`) et
//! le réglage par défaut hérité par un écran secondaire sans override. Pour
//! la même raison, `update` prend maintenant `text`/`text_scale` et rend
//! SA PROPRE texture en interne (`CachedText`, même principe que
//! `dock_instance::CachedText`) au lieu de recevoir une texture déjà rendue
//! partagée par tous les écrans — un `text_scale` différent par écran ne
//! peut sinon pas produire une taille de police différente par écran.

use std::cell::Cell;

use windows::core::{w, Result};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct3D11::ID3D11Device;
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, GetMonitorInfoW, MonitorFromWindow, SetWindowRgn,
    HRGN, MONITORINFO, MONITOR_DEFAULTTONEAREST, RGN_OR,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, RegisterClassExW, SetWindowPos, ShowWindow,
    CS_HREDRAW, CS_VREDRAW, HWND_TOPMOST, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::gfx::backdrop;
use crate::gfx::device::GfxDevice;
use crate::gfx::quad_renderer::{QuadBatch, QuadInstance, QuadRenderer};
use crate::gfx::swapchain::SwapChain;
use crate::gfx::texture::Texture;
use crate::ui::dock;
use crate::ui::topbar;
use crate::win32::menu;

/// Couleur du texte — le pôle de texte des menus, cohérent avec le texte des menus/de l'infobulle
/// (`ui::color::menu_fg()`, voir `win32::tooltip::text_color()`), pas dérivé
/// de l'accent qui ne teinte ici que le fond.
///
/// Suit le mode clair (voir `ui::color::menu_fg`) : le fond, teinté par
/// l'accent des menus, y devient une surface claire.
fn text_color() -> [f32; 3] {
    crate::ui::color::menu_fg()
}

// Réglages persistés (voir `config::load_fullscreen_clock_*`), tenus ici en
// mémoire vive — même convention que `win32::tooltip::TOOLTIP_SCALE` : lus
// à chaque frame par `app::run`, écrits par `win32::cef_settings` dès que
// l'utilisateur touche un contrôle du panneau de réglages.
thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static POSITION_TOP: Cell<bool> = const { Cell::new(true) };
    static BAR_SCALE: Cell<f32> = const { Cell::new(1.0) };
    static TEXT_SCALE: Cell<f32> = const { Cell::new(1.0) };
}

pub fn is_enabled() -> bool {
    ENABLED.with(Cell::get)
}

pub fn set_enabled(enabled: bool) {
    ENABLED.with(|c| c.set(enabled));
}

pub fn is_position_top() -> bool {
    POSITION_TOP.with(Cell::get)
}

pub fn set_position_top(top: bool) {
    POSITION_TOP.with(|c| c.set(top));
}

pub fn bar_scale() -> f32 {
    BAR_SCALE.with(Cell::get)
}

/// Bornes `[0.5, 10.0]` — DEMANDE EXPLICITE ("augmente les valeurs des
/// sliders de taille du texte et de la barre jusqu'à 1000%") : même plage
/// que `win32::tooltip::set_tooltip_scale`.
pub fn set_bar_scale(scale: f32) {
    BAR_SCALE.with(|c| c.set(scale.clamp(0.5, 10.0)));
}

pub fn text_scale() -> f32 {
    TEXT_SCALE.with(Cell::get)
}

/// Voir `set_bar_scale` — mêmes bornes.
pub fn set_text_scale(scale: f32) {
    TEXT_SCALE.with(|c| c.set(scale.clamp(0.5, 10.0)));
}

/// **LE DÉCALAGE DE L'HORLOGE** — DEMANDE (2026-10-24) : « gérer leurs décalages
/// horizontal et vertical par rapport à leur zone d'affichage sélectionnée ».
/// Par écran : `ECRAN_PRINCIPAL` ou le nom de périphérique d'un secondaire ; un
/// secondaire sans décalage à lui prend celui du principal (voir
/// `decalage_de`). En pixels LOGIQUES (× DPI de l'écran au placement) :
/// `[horizontal, vertical]`, vers la droite, et en s'ÉLOIGNANT du bord choisi
/// (vers le centre de l'écran, que l'horloge soit en haut ou en bas).
pub const ECRAN_PRINCIPAL: &str = "principal";
pub const DECALAGE_X_MAX: i32 = 1500;
pub const DECALAGE_Y_MAX: i32 = 1500;

thread_local! {
    static DECALAGES: std::cell::RefCell<Option<std::collections::BTreeMap<String, [i32; 2]>>> =
        const { std::cell::RefCell::new(None) };
}

fn decalages<R>(f: impl FnOnce(&mut std::collections::BTreeMap<String, [i32; 2]>) -> R) -> R {
    DECALAGES.with(|d| f(d.borrow_mut().get_or_insert_with(crate::config::load_fullscreen_clock_decalages)))
}

/// Le décalage réglé pour `ecran` lui-même, `None` s'il n'en a pas.
pub fn decalage_propre(ecran: &str) -> Option<[i32; 2]> {
    decalages(|d| d.get(ecran).copied())
}

/// Le décalage appliqué sur `ecran` : le sien, sinon celui du principal.
pub fn decalage_de(ecran: &str) -> [i32; 2] {
    decalage_propre(ecran).or_else(|| decalage_propre(ECRAN_PRINCIPAL)).unwrap_or([0, 0])
}

/// Règle un axe (`0` horizontal, `1` vertical) du décalage de `ecran`, borné, et
/// l'enregistre. `None` : l'écran (secondaire) revient à celui du principal.
pub fn set_decalage(ecran: &str, valeur: Option<(usize, i32)>) {
    decalages(|d| {
        match valeur {
            Some((axe, v)) => {
                let mut courant = d.get(ecran).copied().unwrap_or_else(|| {
                    if ecran == ECRAN_PRINCIPAL { [0, 0] } else { d.get(ECRAN_PRINCIPAL).copied().unwrap_or([0, 0]) }
                });
                courant[axe.min(1)] = borner_decalage(axe, v);
                d.insert(ecran.to_string(), courant);
            }
            None => {
                d.remove(ecran);
            }
        }
        crate::config::save_fullscreen_clock_decalages(d);
    });
}

fn borner_decalage(axe: usize, v: i32) -> i32 {
    if axe == 0 { v.clamp(-DECALAGE_X_MAX, DECALAGE_X_MAX) } else { v.clamp(0, DECALAGE_Y_MAX) }
}

/// La pilule `[x, y, l, h]` (écran) décalée de `decalage` (pixels PHYSIQUES),
/// le vertical vers le centre, puis ramenée dans le moniteur `[g, h, d, b]`.
fn pilule_decalee(pill: [i32; 4], decalage: [i32; 2], position_top: bool, monitor: [i32; 4]) -> [i32; 4] {
    let dy = if position_top { decalage[1] } else { -decalage[1] };
    let x = (pill[0] + decalage[0]).clamp(monitor[0], (monitor[2] - pill[2]).max(monitor[0]));
    let y = (pill[1] + dy).clamp(monitor[1], (monitor[3] - pill[3]).max(monitor[1]));
    [x, y, pill[2], pill[3]]
}

/// Dimensions de départ (échelle 1.0) — voir `update`.
const BASE_BAR_HEIGHT_PX: f32 = 36.0;
const BASE_HORIZONTAL_PADDING_PX: f32 = 20.0;
/// Taille de police de base (avant `text_scale`) — publique : consommée par
/// `app::run` pour rendre la texture de texte passée à `update` (même
/// principe que `app::TOOLTIP_FONT_PX`/`win32::tooltip::tooltip_scale`), le
/// rendu de texte lui-même restant hors de ce module (pas de dépendance à
/// `gfx::text` ici, juste au type `Texture` déjà rendu).
pub const BASE_FONT_PX: i32 = 20;
/// Marge fixe entre la barre et le bord de l'écran — volontairement PAS
/// mise à l'échelle par `bar_scale` (juste un espacement visuel constant,
/// pas une dimension de la barre elle-même).
const SCREEN_EDGE_MARGIN_PX: i32 = 6;

/// Rectangle écran (coordonnées absolues) du moniteur portant
/// `hwnd_on_monitor` — une `FullscreenClockWindow` par écran (voir
/// `app::run`, une instance pour le dock principal, une par
/// `SecondaryDock`) ; chacune se positionne sur SON PROPRE écran en passant
/// ici le HWND du dock (principal ou secondaire) correspondant, plutôt que
/// de viser toujours l'écran principal (bug initial : "je ne vois pas
/// l'horloge" côté écran secondaire, faute d'une mini-barre dédiée à cet
/// écran).
fn monitor_rect_of(hwnd_on_monitor: HWND) -> RECT {
    unsafe {
        let monitor = MonitorFromWindow(hwnd_on_monitor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut info);
        info.rcMonitor
    }
}

/// Clé du cache de région (voir `FullscreenClockWindow::applied_region`) :
/// elle doit décrire **TOUTES les entrées d'`apply_region`**, et rien d'autre.
///
/// **La position en fait partie, et ce n'est pas une précaution de principe** :
/// avant l'effet de survol, la région était posée à l'origine de la fenêtre
/// (`CreateRoundRectRgn(0, 0, width, height, ..)`), donc indépendante de
/// l'endroit où la fenêtre se trouve — une clé sur la seule TAILLE suffisait.
/// `apply_region` consomme maintenant `pill_local`, qui dépend de l'origine de la
/// fenêtre : une clé qui l'ignorerait laisserait la région d'un emplacement
/// précédent en place, et comme **le shell rogne à la région posée, pas au
/// dessin**, l'horloge disparaîtrait purement et simplement (même classe de
/// défaut que « je ne vois pas l'horloge côté écran secondaire »).
///
/// L'échelle de magnification RÉGLÉE en fait partie pour la même raison : la
/// boîte du texte (`text_box`) se calcule à l'échelle visée, donc la changer
/// pendant un survol doit reposer la région — sinon le texte grossi est rogné.
///
/// (position x, y, largeur, hauteur, rayon en bits, en survol, échelle visée en bits)
type RegionKey = (i32, i32, i32, i32, u32, bool, u32);

/// Marge à réserver de chaque côté de la pilule pour que son texte puisse
/// grandir jusqu'à la magnification MAXIMALE **et** tourner sans jamais être
/// ramené par `dock::presented_rect_in`.
///
/// **Aucun paramètre d'échelle, et c'est tout l'intérêt** : c'est ce qui rend la
/// fenêtre — donc sa swap chain et sa région — CONSTANTE du repos à la
/// magnification maximale. Sans cela il faudrait un `SetWindowPos` ET un
/// `ResizeBuffers` par frame d'animation, sur une fenêtre topmost posée
/// au-dessus d'un jeu, c'est-à-dire à l'endroit le plus visible qui soit.
///
/// `dock::max_clock_magnified_growth()` est le majorant de la boîte OCCUPÉE du
/// texte (plafond du réglage DES HORLOGES × ce qu'un texte occupe en balançant à
/// ±6°) : la marge vaut donc la moitié de ce que le texte gagne en grandissant,
/// de chaque côté.
///
/// **C'est bien le majorant de l'HORLOGE qu'il faut lire ici, pas celui des
/// icônes** : les deux réglages sont distincts, et le plafond de l'horloge (350 %)
/// est plus haut que celui des icônes (250 %). S'y tromper donnerait une fenêtre
/// trop petite pour le texte magnifié — donc un texte rogné, sans recours, cette
/// fenêtre n'étant jamais redimensionnée une fois créée.
pub fn overscan_padding(text_w: f32, text_h: f32) -> (f32, f32) {
    let grow = dock::max_clock_magnified_growth();
    ((text_w * (grow - 1.0) * 0.5).ceil(), (text_h * (grow - 1.0) * 0.5).ceil())
}

/// Fenêtre à réserver pour la mini-barre : la pilule (coordonnées ÉCRAN) élargie
/// de `padding`, **RAMENÉE DANS LE MONITEUR** — la croissance part donc vers
/// l'intérieur de l'écran, et la pilule ne bouge pas d'un pixel. C'est le même
/// esprit que le rappel-à-l'écran des icônes du dock (`clamp_by_translation`) :
/// au bord haut, tout le rab passe EN DESSOUS.
///
/// Rend `(fenêtre en coordonnées écran, pilule en coordonnées FENÊTRE)` — c'est
/// la seconde valeur que le rendu consomme (voir `update`), et l'écart entre les
/// deux est EXACTEMENT le décalage que tout doit subir : fond flouté, texte,
/// région. Un seul de ces trois oublié donne un flou en forme de blob, un texte
/// décalé, ou une région qui rogne.
///
/// Bornes exprimées en `monitor.left/top/right/bottom`, **jamais `0,0`** : un
/// écran secondaire placé à gauche du principal a une origine NÉGATIVE.
pub fn overscan_window(
    pill: [i32; 4],
    padding: (f32, f32),
    monitor: [i32; 4],
    position_top: bool,
) -> ([i32; 4], [f32; 4]) {
    let pad_x = padding.0.round() as i32;
    let pad_y = padding.1.round() as i32;
    let (w, h) = (pill[2] + 2 * pad_x, pill[3] + 2 * pad_y);

    let max_x = (monitor[2] - w).max(monitor[0]);
    let max_y = (monitor[3] - h).max(monitor[1]);
    let win_x = (pill[0] - pad_x).clamp(monitor[0], max_x);
    let win_y = if position_top {
        (pill[1] - pad_y).clamp(monitor[1], max_y)
    } else {
        (pill[1] + pill[3] + pad_y - h).clamp(monitor[1], max_y)
    };

    let pill_local =
        [(pill[0] - win_x) as f32, (pill[1] - win_y) as f32, pill[2] as f32, pill[3] as f32];
    ([win_x, win_y, w, h], pill_local)
}

/// Pose la région de la fenêtre : la pilule (coins arrondis au rayon des menus)
/// RÉUNIE à la boîte occupée du texte magnifié, s'il y en a une.
///
/// `SetWindowRgn` prend possession de la région qu'on lui donne ; `CombineRgn`,
/// lui, ne consomme PAS ses sources, qui restent nôtres et doivent être
/// libérées — une fuite ici s'accumule à CHAQUE SURVOL, pas à chaque frame, ce
/// qui la rend d'autant plus difficile à voir (même règle que
/// `win32::window::set_dock_region`).
unsafe fn apply_region(hwnd: HWND, pill: [f32; 4], radius: f32, text_box: Option<[f32; 4]>) {
    // Ni texte magnifié ni arrondi : aucune région du tout, exactement le
    // comportement d'avant.
    if text_box.is_none() && radius <= 0.0 {
        let _ = SetWindowRgn(hwnd, None, true);
        return;
    }

    let x = pill[0].round() as i32;
    let y = pill[1].round() as i32;
    let right = (pill[0] + pill[2]).round() as i32;
    let bottom = (pill[1] + pill[3]).round() as i32;

    let combined = CreateRectRgn(0, 0, 0, 0);
    let pill_rgn: HRGN = if radius > 0.0 {
        let d = (radius * 2.0).round() as i32;
        CreateRoundRectRgn(x, y, right, bottom, d, d)
    } else {
        CreateRectRgn(x, y, right, bottom)
    };
    CombineRgn(Some(combined), Some(combined), Some(pill_rgn), RGN_OR);
    let _ = DeleteObject(pill_rgn.into());

    if let Some(text_rect) = text_box {
        let text_rgn = CreateRectRgn(
            text_rect[0].floor() as i32,
            text_rect[1].floor() as i32,
            (text_rect[0] + text_rect[2]).ceil() as i32,
            (text_rect[1] + text_rect[3]).ceil() as i32,
        );
        CombineRgn(Some(combined), Some(combined), Some(text_rgn), RGN_OR);
        let _ = DeleteObject(text_rgn.into());
    }

    // `SetWindowRgn` prend possession de `combined` ; les sources, non (voir
    // ci-dessus).
    let _ = SetWindowRgn(hwnd, Some(combined), true);
}

/// Fenêtre de la mini-barre d'horloge plein écran — une seule instance par
/// écran, créée au démarrage (voir `SecondaryDock::fullscreen_clock` pour
/// les écrans secondaires) et conservée pour toute la durée de vie de
/// l'appli, montrée/masquée/repositionnée/repeinte à chaque frame via
/// `update`. Même technique que `win32::tooltip::TooltipWindow` (voir la doc
/// de module) : un `WS_POPUP` séparé avec son propre swap chain D3D11,
/// PAS un troisième rendu CEF (au-dessus d'un jeu en plein écran, le
/// minimum de dépendances est justement ce qu'on veut).
pub struct FullscreenClockWindow {
    hwnd: HWND,
    swapchain: Option<SwapChain>,
    renderer: QuadRenderer,
    /// Dernière région arrondie posée via `SetWindowRgn` — voir `win32::tooltip::
    /// TooltipWindow::applied_region`, même principe : évite de reposer une région
    /// identique à chaque frame. Voir `RegionKey`, qui porte TOUTES les entrées
    /// d'`apply_region` — une seule oubliée, et la région reste celle d'un état
    /// précédent, ce qui se voit comme un rognage ou une horloge invisible.
    applied_region: Option<RegionKey>,
    visible: bool,
    /// Bitmap du texte affiché, régénérée seulement quand le texte ou la
    /// taille de police change — même garde-fou que `dock_instance::
    /// CachedText` (voir la doc de module, "rend SA PROPRE texture en
    /// interne"). `font_px: -1` initial force la toute première génération.
    text: String,
    font_px: i32,
    text_texture: Option<(Texture, f32, f32)>,
    /// Magnification du texte au survol, une instance PAR ÉCRAN comme le reste
    /// de cette fenêtre. Alimentée par un sondage du CURSEUR (voir `update`) et
    /// non par des événements souris : cette fenêtre est click-through et doit le
    /// rester.
    ///
    /// Le FACTEUR est celui des horloges (`dock::clock_hover_scale`), réglage
    /// distinct de celui des icônes — c'est pourquoi `update` passe par
    /// `update_scaled` et non par `update` —, tandis que la VITESSE d'approche
    /// reste commune avec elles (`dock::hover_ease_speed`).
    hover: dock::HoverAnimator,
}

impl FullscreenClockWindow {
    /// `device` sert uniquement à créer `QuadRenderer` (voir sa doc) — le
    /// rendu proprement dit, dans `update`, a besoin en plus du contexte
    /// D3D11 (`GfxDevice::context`), passé là séparément.
    pub fn new(device: &ID3D11Device) -> Result<Self> {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None)?.into();
            let class_name = w!("NeuraDeskFullscreenClockWindow");

            // Une instance PAR écran (voir `app::run`/`SecondaryDock::
            // fullscreen_clock`) : la classe ne doit être enregistrée qu'une
            // seule fois pour tout le processus, sinon `RegisterClassExW`
            // échoue au deuxième appel (BUG RÉEL VÉCU : "l'interface
            // secondaire s'ouvre et se ferme sans cesse" — l'échec faisait
            // rater `FullscreenClockWindow::new()` pour l'écran secondaire,
            // et `rebuild_secondary_docks` retirait alors tout son dock)
            // — même remède que `SecondaryWindow::create`/`win32::window`,
            // voir leur doc.
            static CLASS_REGISTERED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
            CLASS_REGISTERED.get_or_init(|| {
                let wc = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(wndproc),
                    hInstance: instance,
                    lpszClassName: class_name,
                    ..Default::default()
                };
                RegisterClassExW(&wc);
            });

            // `WS_EX_TRANSPARENT` : jamais d'entrée souris/clavier réelle sur
            // cette barre (même politique que l'infobulle) — inutile qu'elle
            // intercepte quoi que ce soit par-dessus l'application plein
            // écran qu'elle surplombe.
            let ex_style = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT;
            let hwnd = CreateWindowExW(
                ex_style,
                class_name,
                w!("NeuraDesk"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance),
                None,
            )?;

            // Transparence réelle composée par DWM et politique de capture —
            // même politique que le reste de l'interface (voir
            // `win32::cef_popup::make_transparent_and_uncapturable`) :
            // `capture_visibility::apply_to_window` pose les DEUX attributs
            // (affinité d'affichage + exclusion de la Desktop Duplication),
            // voir sa doc.
            let margins = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
            crate::gfx::capture_visibility::apply_to_window(hwnd);

            Ok(Self {
                hwnd,
                swapchain: None,
                renderer: QuadRenderer::new(device)?,
                applied_region: None,
                visible: false,
                text: String::new(),
                font_px: -1,
                text_texture: None,
                hover: dock::HoverAnimator::new(),
            })
        }
    }

    /// Appelée à chaque frame depuis `app::run`, juste après
    /// `Window::sync_fullscreen_visibility` (dock principal) ou
    /// `SecondaryWindow::sync_fullscreen_visibility` (dock d'écran secondaire,
    /// une `FullscreenClockWindow` par `SecondaryDock`) — `show` indique si le
    /// dock concerné est actuellement masqué pour cause de plein écran ET que
    /// la fonction est activée dans les réglages (voir `is_enabled`) ; sinon
    /// la barre reste masquée, exactement le comportement d'avant ce réglage.
    /// `monitor_hwnd_hint` est le HWND du dock (principal ou secondaire)
    /// concerné : il sert UNIQUEMENT à retrouver le moniteur sur lequel
    /// positionner la barre (voir `monitor_rect_of`), jamais à interagir avec
    /// cette fenêtre elle-même. `text` est l'heure déjà formatée par
    /// l'appelant (voir `app::run`, `clock_text`) ; `bar_scale`/
    /// `position_top`/`text_scale` sont RÉSOLUS PAR ÉCRAN par l'appelant
    /// (voir la doc de module, "réglages individuels par écran" — la barre
    /// PRIMAIRE passe les accesseurs globaux `bar_scale()`/`is_position_top()`/
    /// `text_scale()`, chaque `SecondaryDock` passe ses champs `DockInstance::
    /// fullscreen_clock_*` résolus).
    ///
    /// `dt` est le pas de temps de la frame, pour l'animation du survol (voir
    /// `dock::HoverAnimator`) — le même `dt` que `Dock::update`, fourni par
    /// `app::run`.
    pub fn update(
        &mut self,
        gfx: &GfxDevice,
        show: bool,
        monitor_hwnd_hint: HWND,
        text: &str,
        bar_scale: f32,
        position_top: bool,
        text_scale: f32,
        // Le décalage, en pixels LOGIQUES (voir `decalage_de`).
        decalage: [i32; 2],
        dt: f32,
    ) -> Result<()> {
        if !show {
            self.hide();
            // Le survol est RÉINITIALISÉ avec le masquage : une échelle > 1.0
            // laissée en l'état ferait réapparaître le texte DÉJÀ grossi sous une
            // région, elle, reposée au repos (voir la clé d'`applied_region`) —
            // texte rogné net. Un tableau de rectangles VIDE remet toutes les
            // échelles à 1.0 (voir `HoverAnimator::scale`).
            //
            // La cible est passée explicitement (`update_scaled`) pour qu'AUCUN
            // animateur de ce fichier ne lise le global des ICÔNES : le texte de
            // cette barre a son propre réglage (voir `dock::clock_hover_scale`).
            self.hover.update_scaled(&[], None, dt, dock::clock_hover_scale());
            return Ok(());
        }

        let font_px = (BASE_FONT_PX as f32 * text_scale).round() as i32;
        if text != self.text || font_px != self.font_px {
            self.text = text.to_string();
            self.font_px = font_px;
            let (texture, w, h) = crate::gfx::text::render_text(&gfx.device, &self.text, font_px, topbar::CLOCK_FONT_NAME, true)?;
            self.text_texture = Some((texture, w as f32, h as f32));
        }
        let Some((text_texture, text_w, text_h)) = &self.text_texture else {
            self.hide();
            return Ok(());
        };
        let (text_w, text_h) = (*text_w, *text_h);

        let scale = bar_scale;
        let padding_x = BASE_HORIZONTAL_PADDING_PX * scale;
        let box_w = text_w + padding_x * 2.0;
        // DEMANDE EXPLICITE ("fais en sorte que la taille de la barre ...
        // respecte les proportions du contenu") : `BASE_BAR_HEIGHT_PX` est
        // mis à l'échelle AVANT le `max`, pas après (comme c'était le cas
        // avant ce correctif) — sinon un `bar_scale` réduit (ex. 50%)
        // combiné à un `text_scale` élevé (ex. 500%) pouvait ramener la
        // barre EN DESSOUS de la hauteur réelle du texte (`(text_h + 8) *
        // scale` < `text_h`), rognant l'heure affichée. Avec ce calcul, la
        // barre ne peut plus jamais être plus basse que son contenu + une
        // marge verticale elle-même mise à l'échelle.
        let vertical_padding = 8.0 * scale;
        let box_h = (BASE_BAR_HEIGHT_PX * scale).max(text_h + vertical_padding);

        let monitor = monitor_rect_of(monitor_hwnd_hint);
        let box_x = monitor.left + (((monitor.right - monitor.left) as f32 - box_w) / 2.0).round() as i32;
        let box_y = if position_top {
            monitor.top + SCREEN_EDGE_MARGIN_PX
        } else {
            monitor.bottom - box_h.round() as i32 - SCREEN_EDGE_MARGIN_PX
        };
        let pill = [box_x, box_y, (box_w.round() as i32).max(1), (box_h.round() as i32).max(1)];
        let dpi = unsafe { crate::win32::dpi::get_dpi_for_monitor(MonitorFromWindow(monitor_hwnd_hint, MONITOR_DEFAULTTONEAREST)) } as f32;
        let physique = decalage.map(|v| (v as f32 * dpi / crate::win32::dpi::STANDARD_DPI as f32).round() as i32);
        let pill = pilule_decalee(pill, physique, position_top, [monitor.left, monitor.top, monitor.right, monitor.bottom]);

        // La FENÊTRE n'est plus la pilule : elle est pré-dimensionnée une fois
        // pour la magnification MAXIMALE (voir `overscan_padding`/
        // `overscan_window`), de sorte qu'AUCUNE VALEUR ne change pendant
        // l'animation. `SetWindowPos` et `resize` restent appelés à chaque frame
        // (le premier réaffirme le rang topmost, voir `TooltipWindow::show`) mais
        // toujours avec les mêmes nombres, et la région n'est reposée qu'aux
        // transitions (voir `RegionKey`). La pilule, elle, ne bouge pas d'un
        // pixel : c'est `pill_local` qui la replace dans la fenêtre agrandie.
        let (window, pill_local) = overscan_window(
            pill,
            overscan_padding(text_w, text_h),
            [monitor.left, monitor.top, monitor.right, monitor.bottom],
            position_top,
        );
        let width = window[2].max(1) as u32;
        let height = window[3].max(1) as u32;

        unsafe {
            // Voir `win32::tooltip::TooltipWindow::show` : `HWND_TOPMOST`
            // réaffirme le rang topmost à chaque apparition plutôt que de se
            // fier à un ordre déjà en place.
            SetWindowPos(self.hwnd, Some(HWND_TOPMOST), window[0], window[1], width as i32, height as i32, SWP_NOACTIVATE)?;
        }

        match &mut self.swapchain {
            Some(sc) => sc.resize(&gfx.device, width, height)?,
            // `transparent: true` (modèle BitBlt) — INDISPENSABLE depuis que la
            // fenêtre est plus grande que ce qui est dessiné : en modèle *flip*,
            // qui IGNORE l'alpha, la zone entre la pilule et le texte magnifié
            // serait un rectangle NOIR et opaque. Même bascule, et même raison,
            // que la fenêtre du dock (voir `gfx::swapchain`, doc de module).
            None => self.swapchain = Some(SwapChain::new(&gfx.device, self.hwnd, width, height, true)?),
        }
        let swapchain = self.swapchain.as_ref().expect("swapchain vient d'être créé/redimensionné ci-dessus");

        // --- Survol du texte -------------------------------------------------
        // Le texte AU REPOS, en coordonnées FENÊTRE : c'est lui que l'animateur
        // teste, jamais la boîte grossie — elle finirait par se contenir
        // elle-même et le survol oscillerait.
        let text_rest = [
            pill_local[0] + (box_w - text_w) / 2.0,
            pill_local[1] + (box_h - text_h) / 2.0,
            text_w,
            text_h,
        ];
        // Sondage du CURSEUR plutôt que des événements souris : cette fenêtre est
        // click-through (`WS_EX_TRANSPARENT`) et DOIT le rester — elle ne doit
        // jamais voler un clic à l'application plein écran qu'elle surplombe.
        // Même technique que `win32::window_previews::hover_cell`. Nécessaire de
        // toute façon : tant que cette barre est visible, le dock est en
        // `SW_HIDE`, donc `Dock::mouse_pos()` n'est plus alimenté.
        let cursor = {
            let mut point = POINT::default();
            unsafe { GetCursorPos(&mut point) }
                .ok()
                .map(|_| (point.x as f32 - window[0] as f32, point.y as f32 - window[1] as f32))
        };
        self.hover.update_scaled(
            std::slice::from_ref(&text_rest),
            cursor,
            dt,
            dock::clock_hover_scale(),
        );
        let hover_scale = self.hover.scale(0);
        let presented = dock::magnified_box_centred(text_rest, hover_scale);
        // La RÉGION, elle, se calcule à l'échelle VISÉE (celle du réglage) et non
        // à l'échelle animée : c'est un majorant CONSTANT pendant toute
        // l'animation, donc elle n'est reposée qu'à l'entrée et à la sortie du
        // survol — jamais à chaque frame. Même argument, et même choix, que
        // `dock::magnified_boxes`.
        //
        // L'échelle lue est celle des HORLOGES (voir `dock::clock_hover_scale`),
        // et c'est la MÊME liaison locale qui alimente la boîte ET la clé de
        // région juste en dessous : la clé doit décrire les entrées d'
        // `apply_region`, donc changer de source pour l'une sans l'autre
        // laisserait une région périmée jusqu'au prochain survol.
        // ⚠️ **LE BALANCEMENT COMPTE AUSSI** (voir `dock::doit_deborder`) : à
        // « Magnification des horloges » = 100 %, l'échelle ne bouge pas, mais le
        // texte TOURNE quand même — et ses coins penchés seraient coupés net par
        // la région de la pilule sans cette boîte.
        let target = dock::clock_hover_scale();
        let text_box = dock::doit_deborder(hover_scale, self.hover.hover(0)).then(|| {
            dock::occupied_text_rect_for(dock::magnified_box_centred(text_rest, target))
        });

        // Coins vraiment arrondis, même rayon que les menus — voir la doc de
        // module et `win32::tooltip::TooltipWindow::show`. Depuis que le texte
        // peut déborder de la pilule, la région les couvre TOUS LES DEUX : sans
        // elle, ses coins penchés seraient coupés net. Au repos, elle redevient
        // exactement celle d'avant cette fonctionnalité.
        let radius = menu::menu_corner_radius();
        let hovering = text_box.is_some();
        let region_key: RegionKey =
            (window[0], window[1], window[2], window[3], radius.to_bits(), hovering, target.to_bits());
        if self.applied_region != Some(region_key) {
            self.applied_region = Some(region_key);
            unsafe { apply_region(self.hwnd, pill_local, radius, text_box) };
        }

        // Fond flouté du bureau sous la PILULE — et non sous la fenêtre, qui est
        // plus grande qu'elle : capturer la fenêtre donnerait un flou en forme de
        // blob autour de la barre. Même teinte/couleur que les menus, recapturée
        // à chaque frame comme `TooltipWindow::show` (même raison : suivre le
        // bureau en temps réel, cette barre pouvant rester affichée très
        // longtemps).
        let backdrop_tex = backdrop::capture_blurred_batch(&gfx.device, &[pill]).into_iter().next().and_then(Result::ok);

        let menu_rgb = menu::menu_accent_rgb();
        let tint_rgba = [menu_rgb[0], menu_rgb[1], menu_rgb[2], menu::menu_tint()];
        let backdrop_instance = QuadInstance::new_backdrop(pill_local, tint_rgba, radius);
        // Le texte est construit sur sa boîte PRÉSENTÉE, avec `scale = 1.0` : la
        // boîte porte déjà la magnification, comme partout ailleurs (voir
        // `dock::external_glyph_instance`) — et le balancement est posé à part.
        let mut text_instance = QuadInstance::new_glyph_scaled(presented, text_color(), 1.0);
        // L'avancement du SURVOL, pas l'échelle : voir
        // `dock::hover_text_wobble_angle` — le texte balance même sans grossir.
        text_instance.rotation = dock::hover_text_wobble_angle(self.hover.hover(0));
        let mut batches: Vec<QuadBatch> = Vec::new();
        if let Some(backdrop_tex) = &backdrop_tex {
            batches.push(QuadBatch::new(Some(&backdrop_tex.srv), std::slice::from_ref(&backdrop_instance)));
        }
        batches.push(QuadBatch::new(Some(&text_texture.srv), std::slice::from_ref(&text_instance)));

        // Les dimensions de la FENÊTRE : `render` efface tout le viewport en
        // (0,0,0,0), et c'est ce qui rend vraiment transparent tout ce qui n'est
        // pas dessiné (la pilule, elle, ne couvre que sa propre boîte).
        let origin = crate::win32::window::hwnd_screen_origin(self.hwnd).unwrap_or((0, 0));
        self.renderer.render_at(&gfx.context, swapchain.rtv().expect("rtv toujours présente après resize/new"), width as f32, height as f32, &batches, origin)?;
        swapchain.present()?;

        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        self.visible = true;
        Ok(())
    }

    fn hide(&mut self) {
        if self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
        }
    }
}

/// Procédure de fenêtre de la mini-barre — un simple relais vers
/// `DefWindowProcW` (voir `win32::tooltip::tooltip_wndproc`, même
/// principe) : `WS_EX_TRANSPARENT` fait que cette fenêtre ne reçoit de toute
/// façon jamais d'entrée souris/clavier réelle, et le contenu est
/// entièrement dessiné via le swap chain D3D11 plutôt que par `WM_PAINT`.
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un écran 1920×1080 à l'origine (0,0).
    const MONITOR: [i32; 4] = [0, 0, 1920, 1080];

    /// La pilule telle que `update` la calcule, pour un texte donné : largeur =
    /// texte + deux marges horizontales, hauteur = `BASE_BAR_HEIGHT_PX` mais
    /// jamais moins que le texte + 8 px (voir la doc de `box_h`).
    fn pill(text_w: f32, text_h: f32, position_top: bool) -> [i32; 4] {
        let box_w = (text_w + BASE_HORIZONTAL_PADDING_PX * 2.0).round() as i32;
        let box_h = BASE_BAR_HEIGHT_PX.max(text_h + 8.0).round() as i32;
        let x = (MONITOR[2] - box_w) / 2;
        let y = if position_top {
            MONITOR[1] + SCREEN_EDGE_MARGIN_PX
        } else {
            MONITOR[3] - box_h - SCREEN_EDGE_MARGIN_PX
        };
        [x, y, box_w, box_h]
    }

    /// La marge réservée doit couvrir ce que le texte gagne en grandissant
    /// jusqu'au PLAFOND du réglage, rotation comprise — sans quoi la fenêtre
    /// serait trop petite pour le texte magnifié et il serait rogné.
    /// Le vertical s'éloigne du bord choisi, en haut comme en bas ; la pilule ne
    /// sort jamais de l'écran (y compris à gauche du principal).
    #[test]
    fn le_decalage_part_vers_le_centre_et_reste_dans_l_ecran() {
        let ecran = [-1920, 0, 0, 1080];
        let pilule = [-1010, 6, 100, 36];
        assert_eq!(pilule_decalee(pilule, [50, 20], true, ecran), [-960, 26, 100, 36]);
        let en_bas = [-1010, 1038, 100, 36];
        assert_eq!(pilule_decalee(en_bas, [0, 20], false, ecran), [-1010, 1018, 100, 36]);
        assert_eq!(pilule_decalee(pilule, [5000, 5000], true, ecran), [-100, 1044, 100, 36]);
        assert_eq!(pilule_decalee(pilule, [-5000, 0], true, ecran), [-1920, 6, 100, 36]);
    }

    #[test]
    fn overscan_padding_leaves_room_for_the_max_scale_and_wobble() {
        for (w, h) in [(120.0_f32, 28.0_f32), (60.0, 20.0), (400.0, 48.0)] {
            let (pad_x, pad_y) = overscan_padding(w, h);
            let grow = dock::max_clock_magnified_growth();
            assert!(w + 2.0 * pad_x >= w * grow, "largeur {w} : marge {pad_x}");
            assert!(h + 2.0 * pad_y >= h * grow, "hauteur {h} : marge {pad_y}");
        }
    }

    /// La fenêtre ne sort JAMAIS du moniteur : au bord haut toute la marge passe
    /// en dessous de la pilule (même esprit que le rappel-à-l'écran des icônes du
    /// dock), au bord bas tout passe au-dessus.
    #[test]
    fn overscan_window_stays_inside_the_monitor() {
        let padding = overscan_padding(120.0, 28.0);

        let (window, _) = overscan_window(pill(120.0, 28.0, true), padding, MONITOR, true);
        assert_eq!(window[1], MONITOR[1], "collée au bord haut : {window:?}");
        assert!(window[1] + window[3] <= MONITOR[3], "pas plus bas que l'écran : {window:?}");

        let (window, _) = overscan_window(pill(120.0, 28.0, false), padding, MONITOR, false);
        assert_eq!(window[1] + window[3], MONITOR[3], "collée au bord bas : {window:?}");
        assert!(window[1] >= MONITOR[1], "pas plus haut que l'écran : {window:?}");

        for top in [true, false] {
            let (window, _) = overscan_window(pill(120.0, 28.0, top), padding, MONITOR, top);
            assert!(window[0] >= MONITOR[0] && window[0] + window[2] <= MONITOR[2], "{window:?}");
        }
    }

    /// Un écran secondaire placé à gauche du principal a une origine NÉGATIVE :
    /// toute la géométrie part de `monitor.left/top`, jamais de `0,0`.
    #[test]
    fn overscan_window_handles_a_negative_origin() {
        let monitor = [-1920, -200, 0, 880];
        let padding = overscan_padding(120.0, 28.0);
        let original = [-960, monitor[1] + SCREEN_EDGE_MARGIN_PX, 200, 36];

        let (window, pill_local) = overscan_window(original, padding, monitor, true);
        assert_eq!(window[1], monitor[1], "dans le moniteur : {window:?}");
        assert!(window[0] >= monitor[0] && window[0] + window[2] <= monitor[2], "{window:?}");
        assert_eq!(window[0] + pill_local[0].round() as i32, original[0]);
        assert_eq!(window[1] + pill_local[1].round() as i32, original[1]);
    }

    /// **La pilule ne bouge pas d'un pixel** : `window + pill_local` rend la
    /// pilule d'origine, dans les deux positions. C'est ce contrat qui autorise à
    /// pré-dimensionner la fenêtre sans que rien ne se déplace à l'écran.
    #[test]
    fn overscan_window_keeps_the_pill_where_it_was() {
        let padding = overscan_padding(120.0, 28.0);
        for top in [true, false] {
            let original = pill(120.0, 28.0, top);
            let (window, pill_local) = overscan_window(original, padding, MONITOR, top);
            assert_eq!(window[0] + pill_local[0].round() as i32, original[0], "{top}");
            assert_eq!(window[1] + pill_local[1].round() as i32, original[1], "{top}");
            assert_eq!(pill_local[2].round() as i32, original[2], "{top}");
            assert_eq!(pill_local[3].round() as i32, original[3], "{top}");
        }
    }

    /// **Le test qui compte** : à la magnification maximale, la boîte OCCUPÉE du
    /// texte (rotation comprise) tient dans la fenêtre — c'est ce que
    /// `overscan_padding` achète, et c'est ce qui garantit qu'aucun
    /// redimensionnement n'est nécessaire pendant l'animation.
    ///
    /// Vérifié aux deux positions et pour un texte PLUS HAUT que la pilule (le
    /// cas d'un `text_scale` élevé, voir la doc de `box_h`).
    ///
    /// **Un bord de fenêtre PLAQUÉ contre le bord du moniteur est exempté**, et
    /// c'est une limite PHYSIQUE, pas une insuffisance de la marge : une fenêtre
    /// ne s'étend pas au-delà de l'écran. Or la pilule est collée au bord haut ou
    /// bas (c'est sa position), et le texte grossit SYMÉTRIQUEMENT autour de son
    /// centre sans jamais se décaler (DEMANDE EXPLICITE, voir
    /// `magnified_box_centred`) : passé un certain facteur, son côté extérieur
    /// sort donc de l'écran, quelle que soit la marge — aucune marge ne dessine
    /// hors du moniteur. Ce que ce test verrouille, c'est que la marge est bien
    /// POSÉE partout où il y avait de la place pour elle.
    ///
    /// Ce test a longtemps été tautologique : il passait par `presented_rect_in`,
    /// la version QUI BORNE, laquelle ramenait la boîte dans la fenêtre et
    /// garantissait donc l'assertion par construction. La production, elle,
    /// utilise `magnified_box_centred` depuis le passage au grossissement sans
    /// déplacement — le test ne mesurait donc plus ce qui est dessiné.
    #[test]
    fn the_presented_text_stays_on_screen_at_max_scale() {
        for (text_w, text_h) in [(120.0_f32, 28.0_f32), (300.0, 40.0), (40.0, 60.0)] {
            for top in [true, false] {
                let original = pill(text_w, text_h, top);
                let (window, pill_local) = overscan_window(original, overscan_padding(text_w, text_h), MONITOR, top);
                let (width, height) = (window[2] as f32, window[3] as f32);
                let text_rest = [
                    pill_local[0] + (pill_local[2] - text_w) / 2.0,
                    pill_local[1] + (pill_local[3] - text_h) / 2.0,
                    text_w,
                    text_h,
                ];

                // Grossissement PUR, sans rappel dans des bornes — c'est ce que
                // fait la production pour ce texte (voir `magnified_box_centred`,
                // et la demande explicite qui l'a imposé). Utiliser ici la version
                // QUI BORNE affaiblirait le test : elle ramènerait la boîte à
                // l'intérieur de la fenêtre, et l'assertion « la boîte tient dans
                // la fenêtre » passerait même si la marge était trop petite.
                let presented = dock::magnified_box_centred(text_rest, dock::MAX_CLOCK_HOVER_SCALE);
                // Grandi, jamais redimensionné autrement (voir
                // `presented_rect_in_never_resizes`, côté dock).
                assert!((presented[2] - text_w * dock::MAX_CLOCK_HOVER_SCALE).abs() < 0.1, "{presented:?}");
                assert!((presented[3] - text_h * dock::MAX_CLOCK_HOVER_SCALE).abs() < 0.1, "{presented:?}");

                let occupied = dock::occupied_text_rect_for(presented);
                // Chaque bord est jugé SÉPARÉMENT, et n'est vérifié que si la
                // fenêtre avait de la place pour poser sa marge de ce côté (voir
                // la doc ci-dessus) : sur l'axe où elle est plaquée contre le
                // moniteur, elle ne peut pas s'étendre, et le dépassement est
                // inévitable. Trois des quatre bords sont donc éprouvés dans
                // chaque cas — jamais aucun.
                if window[0] > MONITOR[0] {
                    assert!(occupied[0] >= -0.01, "bord gauche : {occupied:?} dans {width}×{height}");
                }
                if window[0] + window[2] < MONITOR[2] {
                    assert!(occupied[0] + occupied[2] <= width + 0.01, "bord droit : {occupied:?}");
                }
                if window[1] > MONITOR[1] {
                    assert!(occupied[1] >= -0.01, "bord haut : {occupied:?}");
                }
                if window[1] + window[3] < MONITOR[3] {
                    assert!(occupied[1] + occupied[3] <= height + 0.01, "bord bas : {occupied:?}");
                }

                // Aucun décalage HORIZONTAL : le grossissement est PUR (voir
                // `magnified_box_centred`), donc le centre du texte ne bouge pas
                // d'un pixel. C'est ce qui distingue une marge juste d'une marge
                // « assez grande par chance ».
                let rest_center_x = text_rest[0] + text_w / 2.0;
                let occupied_center_x = occupied[0] + occupied[2] / 2.0;
                assert!((occupied_center_x - rest_center_x).abs() < 0.01, "décalé en X : {occupied:?}");
            }
        }
    }
}
