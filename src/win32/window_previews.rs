//! **Panneau d'aperçus vivants des fenêtres** — DEMANDE EXPLICITE : « je
//! voudrais avoir plusieurs fenêtres réduites d'une taille ajustable dans les
//! réglages qui affichent le contenu des fenêtres du programme ouvert pour
//! pouvoir choisir quelle fenêtre du programme j'ouvre ou fait basculer ».
//!
//! Apparaît au **survol** de l'icône d'une application ouverte (comme les
//! aperçus de la barre des tâches Windows) : une vignette vivante par fenêtre,
//! cliquable pour activer celle-là. Le clic sur l'icône elle-même garde son
//! geste — mettre au premier plan, ou passer à la fenêtre suivante s'il y en a
//! plusieurs.
//!
//! **UNE SEULE fenêtre suffit** — le seuil de deux fenêtres qui existait ici a
//! été RETIRÉ : DEMANDE EXPLICITE, après avoir constaté que l'aperçu manquait
//! pour la plupart des applications (une seule fenêtre ouverte, c'est le cas
//! ordinaire). Windows fait de même : son aperçu de la barre des tâches
//! apparaît même pour une fenêtre unique.
//!
//! **Fenêtre À PART, pas un popup CEF.** L'infobulle (`win32::tooltip`) est le
//! gabarit le plus proche et fournit tout le matériel — topmost, sans prise de
//! focus, swapchain et `QuadRenderer` propres, coins arrondis par région,
//! positionnement borné au moniteur, fond flouté recapturé — mais deux écarts
//! imposent un module neuf plutôt qu'une généralisation :
//!
//! 1. l'infobulle est `WS_EX_TRANSPARENT` (insensible à la souris, exprès : elle
//!    ne doit jamais intercepter un clic destiné au dock) alors que ce panneau
//!    doit être CLIQUABLE — et ce style se pose à l'enregistrement de la CLASSE,
//!    pas par appel ;
//! 2. elle ne dessine qu'UNE texture en mode « glyphe », qui FORCE le blanc :
//!    une vignette y perdrait ses couleurs (`new_textured`, pas `new_glyph`).
//!
//! Le contenu des vignettes vient de `win32::wgc` (Windows.Graphics.Capture) —
//! voir sa doc de module, qui porte les mesures ayant validé la voie.

use std::cell::RefCell;

use windows::core::{w, Result};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, GetWindowLongPtrW, IsWindow, LoadCursorW, RegisterClassExW,
    SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, GWL_EXSTYLE,
    HWND_TOPMOST, IDC_ARROW, LWA_ALPHA, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE, WM_LBUTTONUP, WM_SETCURSOR,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::gfx::{backdrop, text, GfxDevice, QuadBatch, QuadInstance, QuadRenderer, SwapChain, Texture};
use crate::win32::menu;
use crate::win32::tooltip::monitor_bounds;
use crate::win32::wgc::CaptureSession;

/// Nombre maximal de vignettes. Plafond DUR : chaque vignette est une session de
/// capture, donc un moteur de capture et une copie GPU par frame à la
/// résolution SOURCE de la fenêtre. Au-delà, le coût n'est plus justifiable pour
/// un panneau qui ne vit que quelques secondes.
/// Durée par DÉFAUT du fondu d'apparition et de disparition du panneau (voir
/// `tick`) — DEMANDE EXPLICITE : « ajouter un effet de fondu […] », puis « l'effet
/// de fondu est trop rapide, donnes moi la possibilité de régler sa vitesse ».
/// La valeur de départ a donc été relevée : elle était de 160 ms.
pub const DEFAULT_FADE_MS: f32 = 300.0;
/// Bornes du curseur « Durée du fondu » des Réglages. Le minimum garde le fondu
/// perceptible (en dessous, il se lit comme un saut) ; le maximum reste court
/// devant un survol, sans quoi le panneau mettrait plus de temps à apparaître
/// qu'il n'en met à disparaître.
pub const MIN_FADE_MS: f32 = 80.0;
pub const MAX_FADE_MS: f32 = 1200.0;

thread_local! {
    /// Durée du fondu, en millisecondes — réglable dans les Réglages. Même
    /// mécanisme que `ui::dock::hover_scale` : une cellule lue par `tick` et
    /// bornée par son accesseur, plutôt qu'un champ de `PreviewPanel` qui
    /// obligerait le panneau Réglages à traverser `SettingsRefs`.
    static FADE_MS: std::cell::Cell<f32> = const { std::cell::Cell::new(DEFAULT_FADE_MS) };
}

/// Durée actuelle du fondu, en millisecondes — voir `FADE_MS`.
pub fn fade_ms() -> f32 {
    FADE_MS.with(std::cell::Cell::get)
}

/// Change la durée du fondu, bornée à [`MIN_FADE_MS`, `MAX_FADE_MS`] — le
/// BORNAGE est ici, comme `ui::dock::set_hover_scale`, pour qu'un fichier de
/// configuration corrompu ne puisse pas figer le panneau à l'opacité zéro.
pub fn set_fade_ms(ms: f32) {
    FADE_MS.with(|cell| cell.set(ms.clamp(MIN_FADE_MS, MAX_FADE_MS)));
}

pub const MAX_CELLS: usize = 6;
/// Taille par défaut d'une vignette (largeur, en pixels) — voir
/// `PreviewPanel::set_thumb_width`, réglable dans les Réglages.
pub const DEFAULT_THUMB_WIDTH: f32 = 240.0;
/// Bornes du curseur « taille des vignettes » des Réglages — DEMANDE EXPLICITE :
/// « je voudrais que la valeur max dans le curseur de la taille des aperçus soit
/// triplée » (480 -> 1440).
pub const MIN_THUMB_WIDTH: f32 = 120.0;
pub const MAX_THUMB_WIDTH: f32 = 1440.0;

/// Taille plancher d'une cellule quand le panneau ne tient pas sur le moniteur :
/// en dessous, la vignette ne montrerait plus rien d'utile (voir `fit_cell_width`).
const MIN_FIT_CELL_WIDTH: f32 = 96.0;

/// Où poser le panneau par rapport à l'icône survolée, **et comment ranger les
/// vignettes** — DEMANDE EXPLICITE : « les aperçus s'affichant à la vertical dans
/// le cas où l'icône du programme est dans une barre verticale et à l'horizontal
/// dans le cas où l'icône est dans une barre horizontale ».
///
/// Déduit par l'appelant de la BARRE qui porte l'icône (`Dock::icon_side`),
/// exactement comme `win32::tooltip::Anchor` l'est pour les infobulles — et pour
/// la même raison : le panneau doit s'écarter de la barre, jamais la recouvrir.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Icône de la barre du HAUT : vignettes en RANGÉE, sous l'icône.
    Below,
    /// Icône de la barre du BAS : vignettes en RANGÉE, AU-DESSUS de l'icône —
    /// le panneau s'écarte toujours de sa barre, et celle-ci est en bas de
    /// l'écran, donc la place est au-dessus (voir `show`).
    Above,
    /// Icône de la barre de GAUCHE : vignettes en COLONNE, à droite de l'icône.
    RightOf,
    /// Icône de la barre de DROITE : vignettes en COLONNE, à gauche de l'icône.
    LeftOf,
}

impl Placement {
    /// Vrai pour une barre LATÉRALE : les vignettes s'empilent alors
    /// verticalement.
    fn is_vertical(self) -> bool {
        !matches!(self, Placement::Below | Placement::Above)
    }
}

/// Marge intérieure du panneau et écart entre deux vignettes.
const PADDING: f32 = 10.0;
const CELL_GAP: f32 = 8.0;
/// Rayon des coins des vignettes (le panneau lui-même suit le rayon des menus).
/// `rayon_controle` du style actif (`ui::style`, 6 px pour « NeuraDesk »).
fn thumb_radius() -> f32 {
    crate::ui::style::rayon_controle()
}
/// Écart entre une vignette et son titre.
const TITLE_GAP: f32 = 4.0;
/// Écart entre l'icône survolée et le panneau.
const GAP: f32 = 12.0;
/// Opacité de la surbrillance de la cellule survolée.
const HOVER_ALPHA: f32 = 0.22;
/// Couleur des titres et de la surbrillance. Suit le mode clair (voir `ui::color::menu_fg`) : le fond, teinté par
/// l'accent des menus, y devient une surface claire.
fn title_color() -> [f32; 3] {
    crate::ui::color::menu_fg()
}

thread_local! {
    /// Fenêtre cliquée, déposée par le wndproc et vidée par `app::run` : le
    /// wndproc n'a pas accès à l'état du panneau, d'où ce relais (même famille
    /// que `cef_popup::drain_actions`, en plus court).
    static CLICKED: RefCell<Option<HWND>> = const { RefCell::new(None) };
    /// Rectangles cliquables COURANTS (coordonnées FENÊTRE du panneau) avec la
    /// fenêtre qu'ils activent. Remplis à chaque rendu.
    static HIT_CELLS: RefCell<Vec<([f32; 4], HWND)>> = const { RefCell::new(Vec::new()) };
    /// Fermeture demandée depuis un chemin qui n'a PAS accès à l'instance —
    /// voir `request_hide`. Un drapeau plutôt qu'un `ShowWindow` direct : c'est
    /// `app::run` qui doit refermer l'état (sessions, cellules) en même temps
    /// que la fenêtre, sinon la machine à états croirait le panneau encore
    /// ouvert.
    static HIDE_REQUESTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Demande la fermeture du panneau — voir `HIDE_REQUESTED`. Appelée par
/// `app::hide_overlay_windows`, qui masque tous les menus et doit masquer
/// celui-ci comme les autres.
pub fn request_hide() {
    HIDE_REQUESTED.with(|flag| flag.set(true));
}

/// La demande de fermeture en attente, s'il y en a une.
pub fn take_hide_request() -> bool {
    HIDE_REQUESTED.with(|flag| flag.replace(false))
}

/// La fenêtre désignée par un clic depuis le dernier appel — à activer par
/// l'appelant (`appicon::focus_window`).
pub fn take_clicked_window() -> Option<HWND> {
    CLICKED.with(|c| c.borrow_mut().take())
}

/// Le panneau et l'état de ses vignettes.
pub struct PreviewPanel {
    hwnd: HWND,
    renderer: QuadRenderer,
    swapchain: Option<SwapChain>,
    /// Une session de capture par fenêtre prévisualisée, dans l'ordre
    /// d'affichage. **Conservées d'un rendu à l'autre** : les rouvrir à chaque
    /// frame relancerait un moteur de capture soixante fois par seconde.
    sessions: Vec<(HWND, CaptureSession)>,
    /// Les fenêtres prévisualisées, **dans l'ordre des cellules** — c'est la
    /// source de vérité de la disposition, pas la liste des sessions. Une
    /// cellule existe donc même si sa capture a échoué (fenêtre réduite,
    /// protégée) : elle reste CLIQUABLE, ce qui permet justement de restaurer
    /// une fenêtre réduite — et c'est là une bonne part de l'intérêt.
    targets: Vec<(HWND, String)>,
    /// Titres rendus en bitmap, mémorisés par (texte, taille de police) — même
    /// garde-fou double que `tooltip_texture` dans `app::run`.
    titles: Vec<(String, i32, Texture, f32, f32)>,
    thumb_width: f32,
    /// Côté de l'icône ancrée, et donc sens de rangement des vignettes — voir
    /// `Placement`. Conservé pour pouvoir redessiner (`refresh`) sans que
    /// l'appelant ait à le repasser.
    placement: Placement,
    /// Rectangle FENÊTRE de la cellule survolée (voir `hover_cell`).
    hover: Option<[f32; 4]>,
    applied_region: Option<(i32, i32, u32)>,
    visible: bool,
    /// Opacité ANIMÉE du panneau (0.0 = invisible, 1.0 = pleine) — DEMANDE
    /// EXPLICITE : « ajouter un effet de fondu à l'apparition et à la disparition
    /// des fenêtres de prévisualisation ». Animée par `tick`.
    fade: f32,
    /// Opacité VISÉE : 1.0 tant que le panneau doit être là, 0.0 dès qu'il part
    /// — c'est `tick` qui décide de le cacher pour de bon quand le fondu de
    /// sortie est terminé, jamais `hide` directement.
    fade_target: f32,
    /// Dernière opacité réellement POSÉE sur la fenêtre — évite un appel Win32
    /// par frame alors que la valeur ne bouge pas.
    applied_fade: f32,
    /// Position et taille ÉCRAN du panneau — c'est ce qui permet de savoir si
    /// le curseur est dessus, donc s'il doit RESTER ouvert pendant que le
    /// pointeur va de l'icône vers lui.
    screen_rect: [f32; 4],
    /// Rectangle ÉCRAN de l'icône ancrée, conservé pour redessiner sans le
    /// recalculer (voir `refresh`).
    last_anchor: [f32; 4],
}

impl PreviewPanel {
    pub fn new(gfx: &GfxDevice) -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None)?.into();
            let class_name = w!("NeuraDeskPreviewWindow");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(preview_wndproc),
                hInstance: instance,
                // Curseur standard, explicitement : sans lui la classe n'en a
                // aucun et le curseur garde la forme de la fenêtre survolée
                // précédemment.
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: class_name,
                ..Default::default()
            };
            RegisterClassExW(&wc);

            // `WS_EX_TRANSPARENT` VOLONTAIREMENT ABSENT (le panneau reçoit les
            // clics). `WS_EX_NOACTIVATE` est ESSENTIEL : sans lui, cliquer une
            // vignette prendrait le focus, ce qui casserait l'invariant
            // documenté de `appicon::focus_or` (« le dock ne prend jamais le
            // focus, donc `GetForegroundWindow` reste l'application visée »).
            // `WS_EX_LAYERED` : c'est ce qui autorise `SetLayeredWindowAttributes`
            // (`LWA_ALPHA`), donc le FONDU d'apparition et de disparition — voir
            // `tick`, et `fade` pour le pourquoi d'un fondu de FENÊTRE plutôt que
            // de contenu.
            let ex_style = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED;
            let hwnd = CreateWindowExW(
                ex_style,
                class_name,
                w!("NeuraDesk Previews"),
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

            let margins = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
            // Même politique de capture que le reste de l'interface — les DEUX
            // attributs ensemble, jamais l'un sans l'autre (voir sa doc).
            crate::gfx::capture_visibility::apply_to_window(hwnd);

            Ok(Self {
                hwnd,
                renderer: QuadRenderer::new(&gfx.device)?,
                swapchain: None,
                sessions: Vec::new(),
                targets: Vec::new(),
                titles: Vec::new(),
                thumb_width: DEFAULT_THUMB_WIDTH,
                placement: Placement::Below,
                hover: None,
                applied_region: None,
                visible: false,
                // Voir `fade` : un panneau neuf n'est pas encore là. `applied_fade`
                // démarre à une valeur IMPOSSIBLE, pour forcer la première pose.
                fade: 0.0,
                fade_target: 0.0,
                applied_fade: -1.0,
                screen_rect: [0.0; 4],
                last_anchor: [0.0; 4],
            })
        }
    }

    /// Largeur d'une vignette — réglage « taille des vignettes » des Réglages.
    pub fn set_thumb_width(&mut self, width: f32) {
        self.thumb_width = width.clamp(MIN_THUMB_WIDTH, MAX_THUMB_WIDTH);
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Vrai si le point (coordonnées ÉCRAN) est sur le panneau — voir la doc du
    /// champ `screen_rect`.
    pub fn contains_screen_point(&self, x: i32, y: i32) -> bool {
        if !self.visible {
            return false;
        }
        let [px, py, pw, ph] = self.screen_rect;
        (x as f32) >= px && (x as f32) < px + pw && (y as f32) >= py && (y as f32) < py + ph
    }

    /// Affiche le panneau pour `targets` (HWND + titre), ancré à `icon_rect`
    /// (coordonnées ÉCRAN de l'icône survolée). Ouvre — ou réutilise — une
    /// session de capture par fenêtre. Ne fait rien si la liste est vide.
    /// `placement` : côté de l'icône, donc sens de rangement des vignettes —
    /// voir `Placement`.
    pub fn show(
        &mut self,
        gfx: &GfxDevice,
        icon_rect: [f32; 4],
        targets: &[(HWND, String)],
        placement: Placement,
    ) -> Result<()> {
        if targets.is_empty() {
            self.hide();
            return Ok(());
        }
        self.placement = placement;
        self.targets = targets.iter().take(MAX_CELLS).cloned().collect();
        // FONDU D'APPARITION (voir `fade`) : on part de transparent pour que le
        // fondu ait quelque chose à animer — mais SEULEMENT à la première
        // apparition, sinon le panneau rejouerait un fondu à chaque changement
        // d'icône survolée alors qu'il est déjà là.
        if !self.visible {
            self.fade = 0.0;
            self.applied_fade = -1.0;
        }
        self.fade_target = 1.0;
        self.set_click_through(false);
        self.sync_sessions(gfx, targets);
        self.layout_and_render(gfx, icon_rect)
    }

    /// Sonde les captures et redessine si quelque chose a bougé. À appeler à
    /// chaque frame tant que le panneau est visible.
    pub fn refresh(&mut self, gfx: &GfxDevice) -> Result<()> {
        if !self.visible || self.targets.is_empty() {
            return Ok(());
        }
        // Une fenêtre fermée pendant l'aperçu : la retirer de la disposition, et
        // fermer sa session. S'il ne reste plus rien à montrer, plus de panneau.
        self.targets.retain(|(hwnd, _)| unsafe { IsWindow(Some(*hwnd)).as_bool() });
        self.sessions.retain(|(hwnd, _)| self.targets.iter().any(|(t, _)| t == hwnd));
        if self.targets.is_empty() {
            self.hide();
            return Ok(());
        }

        let mut changed = false;
        for (_, session) in self.sessions.iter_mut() {
            changed |= session.poll(&gfx.device);
        }
        // Ne redessiner que si une frame est arrivée ou si la surbrillance
        // change : présenter 144 fois par seconde un contenu à 30 Hz serait du
        // gaspillage, et un swapchain flip-model conserve la dernière image.
        let hover = self.hover_cell();
        if changed || hover != self.hover {
            self.hover = hover;
            self.layout_and_render(gfx, self.last_anchor)?;
        }
        Ok(())
    }

    /// Demande la DISPARITION du panneau — qui se fait en FONDU (voir `tick`) :
    /// la fenêtre reste affichée le temps qu'elle s'éteigne, et c'est `tick` qui
    /// la cachera pour de bon, jamais cette fonction. DEMANDE EXPLICITE :
    /// « ajouter un effet de fondu à l'apparition et à la disparition des
    /// fenêtres de prévisualisation ».
    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.fade_target = 0.0;
        // INSENSIBLE À LA SOURIS DÈS MAINTENANT, pas à la fin du fondu. La
        // fenêtre reste affichée le temps de s'éteindre (délai de grâce + fondu,
        // plus d'une demi-seconde) et elle est GRANDE — vignettes à 1440 px chez
        // l'utilisateur, soit ~973×576 à côté du dock. Un clic sur le bureau
        // juste après avoir quitté le dock tombait sur ce panneau presque
        // transparent, entre deux vignettes, et ne faisait RIEN — SIGNALÉ le
        // 2026-09-26 (« je n'ai plus d'interaction avec le bureau depuis les
        // clics de souris », intermittent). `show` rend les clics au panneau.
        self.set_click_through(true);
        // Les sessions, elles, se ferment TOUT DE SUITE : le panneau peut rester
        // caché longuement, et une session qui tourne dans le vide coûte une
        // copie GPU par frame ET empêche la fenêtre source de se throttler. La
        // dernière image DÉJÀ PRÉSENTÉE suffit au fondu — rien n'est redessiné
        // pendant la disparition.
        self.sessions.clear();
        self.targets.clear();
        self.titles.clear();
        HIT_CELLS.with(|cells| cells.borrow_mut().clear());
    }

    /// Pose ou retire `WS_EX_TRANSPARENT` — voir `hide` pour le pourquoi.
    fn set_click_through(&self, click_through: bool) {
        unsafe {
            let current = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let updated = click_through_applied(current, click_through);
            if updated != current {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, updated);
            }
        }
    }

    /// Cache la fenêtre POUR DE BON — appelé par `tick` à la fin du fondu de
    /// sortie, jamais par les appelants (voir `hide`).
    fn hide_now(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        self.visible = false;
    }

    /// Avance le fondu — **À APPELER À CHAQUE FRAME, même quand le panneau est
    /// caché** : c'est ce qui termine un fondu de sortie.
    ///
    /// L'opacité est celle de la FENÊTRE ENTIÈRE (`WS_EX_LAYERED` +
    /// `LWA_ALPHA`) et non de son contenu : une seule valeur à animer, aucun
    /// travail de rendu par frame, et la dernière image présentée se fond
    /// d'elle-même — exactement ce qu'il faut pendant la disparition, où plus
    /// rien n'est redessiné.
    pub fn tick(&mut self, dt: f32) {
        // Durée RÉGLABLE (voir `fade_ms`) : le pas est ce qu'il faut parcourir
        // en `dt` secondes pour faire le trajet complet en `fade_ms`.
        let step = (dt / (fade_ms() / 1000.0)).max(0.0);
        self.fade += (self.fade_target - self.fade).clamp(-step, step);
        if self.fade != self.applied_fade {
            self.applied_fade = self.fade;
            unsafe {
                let _ = SetLayeredWindowAttributes(
                    self.hwnd,
                    COLORREF(0),
                    (self.fade.clamp(0.0, 1.0) * 255.0).round() as u8,
                    LWA_ALPHA,
                );
            }
        }
        if self.visible && self.fade_target == 0.0 && self.fade == 0.0 {
            self.hide_now();
        }
    }

    /// La cellule survolée, s'il y en a une.
    fn hover_cell(&self) -> Option<[f32; 4]> {
        let mut point = POINT::default();
        if unsafe { GetCursorPos(&mut point) }.is_err() || !self.contains_screen_point(point.x, point.y) {
            return None;
        }
        let (x, y) = (point.x as f32 - self.screen_rect[0], point.y as f32 - self.screen_rect[1]);
        HIT_CELLS.with(|cells| {
            cells.borrow().iter().map(|(rect, _)| *rect).find(|rect| {
                x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3]
            })
        })
    }

    /// Ouvre les sessions manquantes et ferme celles devenues inutiles, sans
    /// toucher aux autres — pour ne pas relancer un moteur de capture à chaque
    /// frame.
    fn sync_sessions(&mut self, gfx: &GfxDevice, targets: &[(HWND, String)]) {
        self.sessions.retain(|(hwnd, _)| targets.iter().any(|(t, _)| t == hwnd));
        for (hwnd, _) in targets.iter().take(MAX_CELLS) {
            if self.sessions.iter().any(|(existing, _)| existing == hwnd) {
                continue;
            }
            // `None` est le cas NORMAL d'une fenêtre qui refuse la capture
            // (réduite, protégée) — voir la doc de `CaptureSession::start` : on
            // n'affiche alors simplement pas de vignette pour elle.
            if let Some(session) = CaptureSession::start(&gfx.device, *hwnd) {
                self.sessions.push((*hwnd, session));
            }
        }
    }

    /// Calcule la disposition, (re)positionne la fenêtre et dessine — en UN SEUL
    /// appel à `QuadRenderer::render`, qui commence par un
    /// `ClearRenderTargetView` : deux appels s'effaceraient l'un l'autre.
    fn layout_and_render(&mut self, gfx: &GfxDevice, icon_rect: [f32; 4]) -> Result<()> {
        self.last_anchor = icon_rect;
        let count = self.targets.len().min(MAX_CELLS);
        if count == 0 {
            self.hide();
            return Ok(());
        }

        // Géométrie : une SEULE rangée de vignettes, comme les aperçus de la
        // barre des tâches. Chaque cellule est une vignette 16/9 et son titre.
        // Le moniteur QUI PORTE l'icône survolée (une icône de barre secondaire
        // peut être sur un autre écran) : il borne à la fois la taille des
        // cellules et la position du panneau.
        let [ix, iy, iw, ih] = icon_rect;
        let (mx, my, mw, mh) = monitor_bounds((ix + iw * 0.5) as i32, (iy + ih * 0.5) as i32);
        let vertical = self.placement.is_vertical();
        // Réduite si la rangée/colonne ne tient pas sur le moniteur — voir
        // `fit_cell_width`.
        let cell_w = fit_cell_width(self.thumb_width, count, vertical, mw as f32, mh as f32);
        let font_px = title_font_px(cell_w);
        let thumb_h = (cell_w * 9.0 / 16.0).round();
        let cell_h = cell_height_for(cell_w);
        let (box_w, box_h) = panel_size_for(cell_w, count, vertical);

        // Position : le panneau s'écarte TOUJOURS de la barre qui porte l'icône,
        // jamais par-dessus (même principe que `tooltip::Anchor`). Rangée sous
        // une icône de la barre du haut ; colonne à droite d'une icône de la
        // barre de gauche, à gauche d'une icône de la barre de droite. Le long
        // de la barre, le panneau est centré sur l'icône et ramené dans
        // l'écran.
        let clamp = |value: f32, min: f32, max: f32| value.clamp(min, max.max(min));
        let (box_x, box_y) = match self.placement {
            Placement::Below => {
                let below = iy + ih + GAP;
                let y = if below + box_h <= (my + mh) as f32 {
                    below
                } else {
                    (iy - box_h - GAP).max(my as f32)
                };
                let x = ix + iw * 0.5 - box_w * 0.5;
                (clamp(x, mx as f32, (mx + mw) as f32 - box_w), y)
            }
            Placement::Above => {
                let above = iy - box_h - GAP;
                let y = if above >= my as f32 {
                    above
                } else {
                    (iy + ih + GAP).min((my + mh) as f32 - box_h)
                };
                let x = ix + iw * 0.5 - box_w * 0.5;
                (clamp(x, mx as f32, (mx + mw) as f32 - box_w), y)
            }
            Placement::RightOf => (
                clamp(ix + iw + GAP, mx as f32, (mx + mw) as f32 - box_w),
                clamp(iy + ih * 0.5 - box_h * 0.5, my as f32, (my + mh) as f32 - box_h),
            ),
            Placement::LeftOf => (
                clamp(ix - box_w - GAP, mx as f32, (mx + mw) as f32 - box_w),
                clamp(iy + ih * 0.5 - box_h * 0.5, my as f32, (my + mh) as f32 - box_h),
            ),
        };
        let width = (box_w.round() as u32).max(1);
        let height = (box_h.round() as u32).max(1);
        self.screen_rect = [box_x, box_y, box_w, box_h];

        unsafe {
            // `HWND_TOPMOST` réaffirmé à chaque apparition — même remarque que
            // `tooltip::show` : entre fenêtres toutes topmost (dock, popups
            // CEF), l'ordre réel dépend de qui a été positionné en dernier.
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                box_x.round() as i32,
                box_y.round() as i32,
                width as i32,
                height as i32,
                SWP_NOACTIVATE,
            )?;
        }

        match &mut self.swapchain {
            Some(sc) => sc.resize(&gfx.device, width, height)?,
            None => self.swapchain = Some(SwapChain::new(&gfx.device, self.hwnd, width, height, false)?),
        }
        let swapchain = self.swapchain.as_ref().expect("swapchain créé/redimensionné juste au-dessus");

        // Coins vraiment arrondis par RÉGION, pas seulement par l'alpha du
        // shader — même remède que l'infobulle et les popups CEF pour le même
        // défaut (« derrière les arrondis des menus du noir apparaît »).
        let radius = menu::menu_corner_radius();
        let region_key = (width as i32, height as i32, radius.to_bits());
        if self.applied_region != Some(region_key) {
            self.applied_region = Some(region_key);
            unsafe {
                if radius > 0.0 {
                    let d = (radius * 2.0).round() as i32;
                    // `SetWindowRgn` prend possession de la région : pas de
                    // `DeleteObject` (voir `window::set_dock_region`).
                    let region = CreateRoundRectRgn(0, 0, width as i32, height as i32, d, d);
                    let _ = SetWindowRgn(self.hwnd, Some(region), true);
                } else {
                    let _ = SetWindowRgn(self.hwnd, None, true);
                }
            }
        }

        // Titres : rendus (et mémorisés) seulement quand le texte ou la taille
        // de police change — même garde-fou que les infobulles.
        let wanted: Vec<String> = self.targets.iter().take(count).map(|(_, name)| name.clone()).collect();
        self.titles
            .retain(|(text, px, ..)| wanted.contains(text) && *px == font_px);
        for text in wanted.iter() {
            if self.titles.iter().any(|(t, px, ..)| t == text && *px == font_px) {
                continue;
            }
            let (texture, w, h) = text::render_text(&gfx.device, text, font_px, &crate::ui::style::police(), false)?;
            self.titles.push((text.clone(), font_px, texture, w as f32, h as f32));
        }

        // Fond flouté du bureau, teinté comme les popups « menu », recapturé à
        // chaque frame (le bureau bouge derrière un panneau qui reste affiché).
        let backdrop_tex = backdrop::capture_blurred_batch(
            &gfx.device,
            &[[box_x.round() as i32, box_y.round() as i32, width as i32, height as i32]],
        )
        .into_iter()
        .next()
        .and_then(Result::ok);
        let menu_rgb = menu::menu_accent_rgb();
        let tint = [menu_rgb[0], menu_rgb[1], menu_rgb[2], menu::menu_tint()];
        let background = QuadInstance::new_backdrop([0.0, 0.0, box_w, box_h], tint, radius);

        let mut batches: Vec<QuadBatch> = Vec::new();
        if let Some(backdrop_tex) = &backdrop_tex {
            batches.push(QuadBatch::new(Some(&backdrop_tex.srv), std::slice::from_ref(&background)));
        }

        // Cellules : rectangles cliquables, plus la surbrillance de celle qui
        // est survolée (un aplat, dessiné SOUS sa vignette).
        let mut cells: Vec<([f32; 4], HWND)> = Vec::with_capacity(count);
        let mut highlights: Vec<QuadInstance> = Vec::new();
        for (i, (hwnd, _)) in self.targets.iter().take(count).enumerate() {
            let cell = cell_rect(i, cell_w, cell_h, vertical);
            cells.push((cell, *hwnd));
            if self.hover == Some(cell) {
                let thumb = [cell[0], cell[1], cell_w, thumb_h];
                highlights.push({
                    let [r, g, b] = title_color();
                    QuadInstance::new(thumb, [r, g, b, HOVER_ALPHA], thumb_radius(), 1.0)
                });
            }
        }
        if !highlights.is_empty() {
            batches.push(QuadBatch::new(None, highlights.as_slice()));
        }

        // Les instances doivent VIVRE jusqu'à l'appel à `render` : elles sont
        // donc nommées ici plutôt que déclarées dans la boucle — même contrainte
        // d'emprunt que `app::run`, qui déclare ses `*_render_instance` avant de
        // construire ses lots.
        //
        // Une vignette par lot : chaque session a SA texture, et le pipeline ne
        // lie qu'une texture à la fois. Une session sans frame encore arrivée
        // n'a pas d'instance du tout — sa cellule reste vide.
        let mut thumb_instances: Vec<QuadInstance> = Vec::new();
        let mut thumb_srvs: Vec<&windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView> = Vec::new();
        for i in 0..count {
            // Une fenêtre SANS session — capture refusée (réduite, protégée) —
            // n'a pas de vignette : sa cellule reste vide, mais cliquable.
            let Some(session) =
                self.sessions.iter().find(|(hwnd, _)| *hwnd == self.targets[i].0).map(|(_, s)| s)
            else {
                continue;
            };
            let Some(srv) = session.srv() else {
                continue;
            };
            let (cw, ch) = session.size();
            let cell = cell_rect(i, cell_w, cell_h, vertical);
            let thumb = [cell[0], cell[1], cell_w, thumb_h];
            // `fit_rect` : letterbox, le contenu garde son rapport d'aspect au
            // lieu d'être étiré.
            let rect = fit_rect(cw as f32, ch as f32, thumb);
            thumb_instances.push(QuadInstance::new_textured(rect, thumb_radius(), 1.0));
            thumb_srvs.push(srv);
        }
        for (srv, instance) in thumb_srvs.iter().zip(thumb_instances.iter()) {
            batches.push(QuadBatch::new(Some(srv), std::slice::from_ref(instance)));
        }

        // Titres, sous leur vignette — même contrainte de durée de vie.
        let mut title_instances: Vec<QuadInstance> = Vec::new();
        let mut title_srvs: Vec<&windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView> = Vec::new();
        for (i, (_, text)) in self.targets.iter().enumerate().take(count) {
            let Some((_, _, texture, w, h)) =
                self.titles.iter().find(|(t, px, ..)| t == text && *px == font_px)
            else {
                continue;
            };
            let cell = cell_rect(i, cell_w, cell_h, vertical);
            let rect = [cell[0] + 2.0, cell[1] + thumb_h + TITLE_GAP, *w, *h];
            title_instances.push(QuadInstance::new_glyph(rect, title_color()));
            title_srvs.push(&texture.srv);
        }
        for (srv, instance) in title_srvs.iter().zip(title_instances.iter()) {
            batches.push(QuadBatch::new(Some(srv), std::slice::from_ref(instance)));
        }

        self.renderer.render_at(
            &gfx.context,
            swapchain.rtv().expect("rtv toujours présente après resize/new"),
            box_w,
            box_h,
            &batches,
            crate::win32::window::hwnd_screen_origin(self.hwnd).unwrap_or((0, 0)),
        )?;
        swapchain.present()?;

        HIT_CELLS.with(|hit| *hit.borrow_mut() = cells);
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        self.visible = true;
        Ok(())
    }
}

/// Rectangle de la i-ème cellule — en COLONNE (barres latérales) ou en RANGÉE
/// (barre du haut), voir `Placement`.
fn cell_rect(i: usize, cell_w: f32, cell_h: f32, vertical: bool) -> [f32; 4] {
    let offset = i as f32 * (if vertical { cell_h + CELL_GAP } else { cell_w + CELL_GAP });
    if vertical {
        [PADDING, PADDING + offset, cell_w, cell_h]
    } else {
        [PADDING + offset, PADDING, cell_w, cell_h]
    }
}

/// Rectangle de la vignette DANS sa cellule — letterbox : le contenu garde son
/// rapport d'aspect et est centré, plutôt qu'étiré.
///
/// Fonction PURE, et testée : c'est la seule géométrie du panneau qui se trompe
/// silencieusement (une vignette étirée reste « une image », juste fausse).
fn fit_rect(content_w: f32, content_h: f32, cell: [f32; 4]) -> [f32; 4] {
    let [cx, cy, cw, ch] = cell;
    if content_w <= 0.0 || content_h <= 0.0 || cw <= 0.0 || ch <= 0.0 {
        return cell;
    }
    let scale = (cw / content_w).min(ch / content_h);
    let (w, h) = (content_w * scale, content_h * scale);
    [cx + (cw - w) * 0.5, cy + (ch - h) * 0.5, w, h]
}

/// Taille de police des titres, pour une largeur de vignette donnée.
fn title_font_px(thumb_width: f32) -> i32 {
    (thumb_width / 14.0).round().clamp(11.0, 22.0) as i32
}

/// Hauteur de la bande de titre d'une cellule — **la seule formule**, utilisée
/// aussi bien par la disposition que par `fit_cell_width` : deux formules
/// divergentes feraient osciller la taille des vignettes d'un rendu à l'autre.
fn title_height_for(cell_w: f32) -> f32 {
    title_font_px(cell_w) as f32 * 1.35 + TITLE_GAP
}

/// Hauteur d'une cellule (vignette 16/9 + son titre).
fn cell_height_for(cell_w: f32) -> f32 {
    (cell_w * 9.0 / 16.0).round() + title_height_for(cell_w)
}

/// Taille (largeur, hauteur) du panneau pour `count` cellules de largeur
/// `cell_w` — fonction PURE, et testée : c'est la géométrie qui décide si le
/// panneau tient sur l'écran.
fn panel_size_for(cell_w: f32, count: usize, vertical: bool) -> (f32, f32) {
    let count = count as f32;
    let cell_h = cell_height_for(cell_w);
    if vertical {
        (PADDING * 2.0 + cell_w, PADDING * 2.0 + cell_h * count + CELL_GAP * (count - 1.0))
    } else {
        (PADDING * 2.0 + cell_w * count + CELL_GAP * (count - 1.0), PADDING * 2.0 + cell_h)
    }
}

/// Largeur de cellule RÉELLEMENT utilisable : celle demandée, **réduite** si la
/// rangée (ou la colonne) ne tient pas sur le moniteur.
///
/// Le curseur des Réglages monte à 1440 px (DEMANDE EXPLICITE) : à cette taille,
/// six vignettes en rangée feraient plus de 8000 px de large, donc un panneau
/// majoritairement hors de l'écran. Réduire est ici préférable à déborder — et
/// c'est la seule réduction appliquée : la valeur du réglage, elle, n'est pas
/// touchée, donc remettre une taille plus petite restaure exactement ce qui a
/// été choisi.
fn fit_cell_width(requested: f32, count: usize, vertical: bool, monitor_w: f32, monitor_h: f32) -> f32 {
    let mut cell_w = requested;
    while cell_w > MIN_FIT_CELL_WIDTH {
        let (w, h) = panel_size_for(cell_w, count, vertical);
        if w <= monitor_w && h <= monitor_h {
            break;
        }
        cell_w -= 4.0;
    }
    cell_w.max(MIN_FIT_CELL_WIDTH)
}

/// `current` avec `WS_EX_TRANSPARENT` posé ou retiré, le reste intact — extrait
/// de `PreviewPanel::set_click_through` pour être testable sans fenêtre.
fn click_through_applied(current: isize, click_through: bool) -> isize {
    let bit = WS_EX_TRANSPARENT.0 as isize;
    if click_through {
        current | bit
    } else {
        current & !bit
    }
}

/// Chemin du wndproc du panneau : relais pour tout, sauf le clic, qui désigne la
/// fenêtre à activer.
unsafe extern "system" fn preview_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_LBUTTONUP => {
            // Les coordonnées d'un message souris sont en pixels CLIENT, déjà
            // dans le repère de `HIT_CELLS`.
            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
            let clicked = HIT_CELLS.with(|cells| {
                cells
                    .borrow()
                    .iter()
                    .find(|(rect, _)| {
                        x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3]
                    })
                    .map(|(_, target)| *target)
            });
            if clicked.is_some() {
                CLICKED.with(|c| *c.borrow_mut() = clicked);
            } else {
                // Un clic ENTRE les vignettes visait ce qu'il y a dessous (le
                // bureau, le plus souvent) : fermer le panneau, pour que ce clic
                // soit le seul perdu.
                request_hide();
            }
            LRESULT(0)
        }
        // Sans ça, le curseur garde la forme de la fenêtre survolée avant.
        WM_SETCURSOR => LRESULT(1),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le panneau qui disparaît laisse passer les clics, et les reprend en
    /// réapparaissant — sans toucher aux autres styles (`NOACTIVATE` surtout).
    #[test]
    fn click_through_toggles_only_transparent() {
        let base = (WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED).0 as isize;
        let through = click_through_applied(base, true);
        assert_eq!(through, base | WS_EX_TRANSPARENT.0 as isize);
        assert_eq!(click_through_applied(through, false), base);
        assert_eq!(click_through_applied(base, false), base);
    }

    /// Le contenu plus LARGE que la cellule est bridé par la largeur, et centré
    /// verticalement : c'est le cas ordinaire d'une fenêtre 16/9.
    #[test]
    fn fit_rect_letterboxes_a_wider_content() {
        let cell = [10.0, 20.0, 200.0, 100.0];
        let rect = fit_rect(400.0, 200.0, cell);
        assert_eq!(rect, [10.0, 20.0, 200.0, 100.0]); // même rapport 2:1
    }

    /// Le contenu plus HAUT que la cellule est bridé par la hauteur, et centré
    /// horizontalement — les bandes vides vont sur les côtés.
    #[test]
    fn fit_rect_centers_a_taller_content() {
        let cell = [0.0, 0.0, 200.0, 100.0];
        let rect = fit_rect(100.0, 200.0, cell);
        assert_eq!(rect, [75.0, 0.0, 50.0, 100.0]);
    }

    /// Une rangée s'élargit avec le nombre de vignettes, une colonne s'allonge :
    /// c'est toute la différence entre les deux orientations demandées.
    #[test]
    fn panel_size_grows_along_the_layout_axis() {
        let (row_w, row_h) = panel_size_for(200.0, 4, false);
        let (col_w, col_h) = panel_size_for(200.0, 4, true);
        assert!(row_w > col_w, "la rangée est plus large que la colonne");
        assert!(col_h > row_h, "la colonne est plus haute que la rangée");
        // La colonne fait exactement la largeur d'UNE cellule (+ marges).
        assert_eq!(col_w, 200.0 + PADDING * 2.0);
    }

    /// Le curseur monte à 1440 px (DEMANDE EXPLICITE) : à cette taille, six
    /// vignettes ne tiennent sur aucun écran — la taille est donc RÉDUITE
    /// jusqu'à ce que le panneau rentre, plutôt que de le laisser déborder.
    #[test]
    fn fit_cell_width_shrinks_a_row_that_would_not_fit() {
        let fitted = fit_cell_width(1440.0, 6, false, 1920.0, 1080.0);
        assert!(fitted < 1440.0);
        let (w, _) = panel_size_for(fitted, 6, false);
        assert!(w <= 1920.0, "le panneau doit rentrer dans le moniteur, w = {w}");
    }

    /// Une taille qui tient déjà n'est JAMAIS touchée : le réglage de
    /// l'utilisateur est respecté tant que c'est possible.
    #[test]
    fn fit_cell_width_keeps_a_size_that_fits() {
        let requested = 300.0;
        assert_eq!(fit_cell_width(requested, 2, false, 3840.0, 2160.0), requested);
    }

    /// Les cellules s'empilent dans le bon axe, sans recouvrement.
    #[test]
    fn cell_rect_stacks_along_the_layout_axis() {
        let first = cell_rect(0, 200.0, 150.0, false);
        let second = cell_rect(1, 200.0, 150.0, false);
        assert_eq!(first, [PADDING, PADDING, 200.0, 150.0]);
        assert_eq!(second[0], PADDING + 200.0 + CELL_GAP, "décalage horizontal en rangée");
        assert_eq!(second[1], PADDING, "même hauteur en rangée");

        let below = cell_rect(1, 200.0, 150.0, true);
        assert_eq!(below[0], PADDING, "même colonne en colonne");
        assert_eq!(below[1], PADDING + 150.0 + CELL_GAP, "décalage vertical en colonne");
    }

    /// Contenu ou cellule dégénérés : on rend la cellule telle quelle plutôt
    /// qu'une géométrie inventée (une session de capture sans frame, ou une
    /// fenêtre d'une taille nulle pendant sa fermeture).
    #[test]
    fn fit_rect_leaves_degenerate_input_alone() {
        let cell = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(fit_rect(0.0, 100.0, cell), cell);
        assert_eq!(fit_rect(100.0, 0.0, cell), cell);
        assert_eq!(fit_rect(100.0, 100.0, [1.0, 2.0, 0.0, 4.0]), [1.0, 2.0, 0.0, 4.0]);
    }
}
