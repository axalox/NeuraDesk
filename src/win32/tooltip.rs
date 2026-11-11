//! Fenêtre flottante dédiée à l'infobulle d'icône — QUAND elle apparaît et si
//! elle apparaît du tout se règlent par `hover_delay`/`enabled`, ci-dessous
//! (DEMANDE EXPLICITE : « réduire le temps d'affichage des infobulles à 1
//! seconde par défaut et laisser la possibilité à l'utilisateur de les activer
//! et de choisir le temps avant qu'elles apparaissent avec un curseur gradué de
//! 1 à 5 secondes »).
//!
//! Première version : dessinée DANS le rendu de la fenêtre du dock — ne
//! fonctionnait pas ("je ne vois pas les infos bulles elles semblent bloquer
//! dans les barres"), car cette fenêtre n'affiche que l'union de ses trois
//! barres (`SetWindowRgn`, voir `win32::window::set_dock_region`) : tout ce
//! qui est dessiné dans la "zone libre" entre les barres — exactement là où
//! une infobulle a besoin d'apparaître pour une icône de barre latérale — y
//! est purement et simplement rogné par Windows, quel que soit le contenu du
//! back buffer.
//!
//! DEMANDE EXPLICITE ("fais les apparaitre comme si c'était des menus") :
//! cette fenêtre-ci est un `WS_POPUP` séparé, comme les popups CEF (voir
//! `win32::cef_popup::make_transparent_and_uncapturable`) mais sans CEF —
//! juste un petit swap chain D3D11. `WS_EX_TRANSPARENT` la rend insensible à
//! la souris (le survol qui la déclenche est déjà suivi par `Dock::mouse_pos`,
//! côté fenêtre du dock — cette fenêtre-ci ne doit jamais intercepter le
//! moindre clic) et `WS_EX_NOACTIVATE` l'empêche de voler le focus.
//!
//! DEUXIÈME DEMANDE EXPLICITE ("transforme les popup en leur donnant la même
//! apparence que les menus") : même matériau que les popups "menu" (menus
//! contextuels, Réseau/Son/Voicemeeter/Calendrier...) plutôt qu'un simple
//! aplat sombre — fond du bureau capturé et flouté sous l'infobulle
//! (`gfx::backdrop::capture_blurred_batch`, coordonnées écran), teinté avec
//! la MÊME couleur/teinte que les menus (`win32::menu::menu_accent_rgb`/
//! `menu_tint`, réglées ensemble dans "Apparence des menus"), et détourée en
//! coins vraiment arrondis via `SetWindowRgn` avec le MÊME rayon
//! (`win32::menu::menu_corner_radius`) — voir `win32::cef_popup::render`,
//! dont ce module reprend exactement le procédé (juste sans CEF : le
//! contenu textuel est un glyphe `QuadInstance` au lieu d'une texture de
//! navigateur).

use std::cell::Cell;
use std::time::{Duration, Instant};

use windows::core::{w, Interface, Result};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows::Win32::Graphics::Gdi::{
    CreateRoundRectRgn, GetMonitorInfoW, MonitorFromPoint, SetWindowRgn, HMONITOR, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassExW, SetWindowPos, ShowWindow,
    CS_HREDRAW, CS_VREDRAW, HWND_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::gfx::backdrop;
use crate::gfx::device::GfxDevice;
use crate::gfx::quad_renderer::{QuadBatch, QuadInstance, QuadRenderer};
use crate::gfx::swapchain::SwapChain;
use crate::gfx::texture::Texture;
use crate::win32::fade;
use crate::win32::menu;

/// Intervalle minimal entre deux captures du fond de l'infobulle — voir
/// `TooltipWindow::show`, qui porte la mesure : une capture par frame rend
/// chaque image différente, donc impose un `Present` par frame — et ce
/// `Present` coûte ~6 ms.
const BACKDROP_REFRESH: Duration = Duration::from_millis(250);
/// Déplacement (en pixels écran) au-delà duquel le fond est recapturé SANS
/// attendre `BACKDROP_REFRESH` — l'infobulle suit l'icône pendant que celle-ci
/// s'anime, et un fond pris pour un autre emplacement se verrait.
const BACKDROP_TOLERANCE_PX: i32 = 24;

/// Le fond de l'infobulle doit-il être recapturé — voir `TooltipWindow::show`,
/// qui en fait toutes les captures et porte la mesure.
///
/// `dernier` : le rectangle et l'instant de la dernière capture retenue.
/// `affichee` : l'infobulle était-elle déjà à l'écran AVANT cet appel (une
/// apparition repart d'un fond neuf). Le temps est passé en paramètre plutôt
/// que lu ici, pour que la décision se teste sans fenêtre.
fn fond_perime(
    dernier: Option<([i32; 4], Instant)>,
    affichee: bool,
    rect: [i32; 4],
    maintenant: Instant,
) -> bool {
    let Some((ancien, pris_a)) = dernier else { return true };
    !affichee
        || maintenant.saturating_duration_since(pris_a) >= BACKDROP_REFRESH
        || ancien[2] != rect[2]
        || ancien[3] != rect[3]
        || (ancien[0] - rect[0]).abs() > BACKDROP_TOLERANCE_PX
        || (ancien[1] - rect[1]).abs() > BACKDROP_TOLERANCE_PX
}

/// Espace entre l'infobulle et l'icône survolée.
const GAP: f32 = 8.0;
/// Marge entre le texte et le bord de l'infobulle.
const PADDING: f32 = 8.0;
/// Couleur du texte de l'infobulle — le pôle de texte des menus, cohérent avec le texte des menus
/// (`ui::color::menu_fg()`, voir `win32::cef_popup::push_menu_palette`),
/// pas dérivé de l'accent qui ne teinte ici que le fond.
///
/// Suit le mode clair (voir `ui::color::menu_fg`) : le fond, teinté par
/// l'accent des menus, y devient une surface claire.
fn text_color() -> [f32; 3] {
    crate::ui::color::menu_fg()
}

thread_local! {
    /// Facteur d'échelle de l'infobulle — DEMANDE EXPLICITE ("donnes la
    /// possibilité à l'utilisateur de régler la taille de ces popup") : même
    /// convention que `win32::menu::MENU_SCALE` (1.0 = taille actuelle),
    /// mais indépendante de celle-ci — cette infobulle n'est pas un popup
    /// "menu" comme les autres (pas de contenu CEF/zoom, voir la doc de
    /// module), un réglage séparé plutôt que de la faire suivre
    /// `menu_scale` sans que l'utilisateur l'ait demandé.
    static TOOLTIP_SCALE: Cell<f32> = const { Cell::new(1.0) };
}

/// Facteur d'échelle actuel — voir `TOOLTIP_SCALE`. Consommé par `app::run`
/// pour la taille de police passée à `gfx::text::render_text`, et par
/// `TooltipWindow::show` pour les marges (`PADDING`/`GAP` ci-dessus).
pub fn tooltip_scale() -> f32 {
    TOOLTIP_SCALE.with(Cell::get)
}

/// Change le facteur d'échelle de l'infobulle, pris en compte dès la
/// prochaine apparition — appelé par `win32::cef_settings` (curseur "Taille
/// des infobulles") et `app::run` au démarrage (voir `config::
/// load_tooltip_scale`). Bornes `[0.5, 10.0]` — DEMANDE EXPLICITE ("augmente
/// la valeur max du slider pour les infobulles jusqu'à 1000 %") : plage
/// délibérément plus large que `win32::menu::set_menu_scale` (`[0.5, 2.0]`),
/// une infobulle isolée pouvant utilement grossir bien au-delà d'un menu
/// entier sans déborder d'un écran.
pub fn set_tooltip_scale(scale: f32) {
    TOOLTIP_SCALE.with(|s| s.set(scale.clamp(0.5, 10.0)));
}

/// Bornes du délai de survol, en SECONDES — DEMANDE EXPLICITE : « un curseur
/// gradué de 1 à 5 secondes ». Elles vivent ICI, avec la valeur, pour que le
/// curseur du panneau de réglages ne puisse pas promettre une plage que le
/// réglage refuserait (même discipline que `MIN_SEPARATOR_SIZE`/
/// `MAX_SEPARATOR_SIZE` côté dock, et que les bornes de `tooltip_scale`).
pub const MIN_HOVER_DELAY_SECS: f32 = 1.0;
pub const MAX_HOVER_DELAY_SECS: f32 = 5.0;
/// Délai par DÉFAUT — DEMANDE EXPLICITE : « réduire le temps d'affichage des
/// infobulles à 1 seconde par défaut ». Il valait 2 s, en dur, dans
/// `app::TOOLTIP_HOVER_DELAY` (constante remplacée par ce réglage).
pub const DEFAULT_HOVER_DELAY_SECS: f32 = 1.0;

thread_local! {
    /// Délai de survol CONTINU avant qu'une infobulle n'apparaisse, en secondes
    /// (voir `app::run`, qui l'applique) — relu à chaque frame, comme
    /// `TOOLTIP_SCALE` et pour la même raison : un curseur relâché pendant
    /// qu'une infobulle est à l'écran doit se voir tout de suite.
    static HOVER_DELAY_SECS: Cell<f32> = const { Cell::new(DEFAULT_HOVER_DELAY_SECS) };
    /// Les infobulles sont-elles affichées du tout — DEMANDE EXPLICITE :
    /// « laisser la possibilité à l'utilisateur de les activer ». VRAI par
    /// défaut : elles existaient avant ce réglage, c'est donc les DÉSACTIVER qui
    /// est le geste nouveau.
    ///
    /// **Ne concerne PAS le chiffre d'un espace en cours de redimensionnement**
    /// (voir `ui::dock::Dock::spacer_readout`, et `app::run` qui le dessine avec
    /// la même fenêtre) : ce n'est pas une explication mais la SEULE mesure
    /// disponible pendant un geste où le marqueur ne suit pas la souris. Le
    /// couper avec les infobulles rendrait un réglage aveugle.
    static ENABLED: Cell<bool> = const { Cell::new(true) };
}

/// Délai de survol actuel — voir `HOVER_DELAY_SECS`. Consommé par `app::run`
/// pour décider quand une infobulle devient due.
pub fn hover_delay() -> std::time::Duration {
    std::time::Duration::from_secs_f32(HOVER_DELAY_SECS.with(Cell::get))
}

/// Change le délai de survol, en secondes — borné à
/// `[MIN_HOVER_DELAY_SECS, MAX_HOVER_DELAY_SECS]`, donc un cran de curseur ne
/// peut pas donner un délai que l'interface ne propose pas.
pub fn set_hover_delay_secs(secs: f32) {
    HOVER_DELAY_SECS.with(|d| d.set(secs.clamp(MIN_HOVER_DELAY_SECS, MAX_HOVER_DELAY_SECS)));
}

/// Les infobulles sont-elles affichées — voir `ENABLED`.
pub fn enabled() -> bool {
    ENABLED.with(Cell::get)
}

/// Active ou désactive les infobulles — appelé par `win32::cef_settings`
/// (« Afficher les infobulles ») et par `app::run` au démarrage (voir
/// `config::load_bool_setting_or`).
pub fn set_enabled(enabled: bool) {
    ENABLED.with(|e| e.set(enabled));
}

/// Côté de la barre qui porte l'icône survolée — détermine de quel côté
/// l'infobulle apparaît, TOUJOURS à l'opposé de la barre elle-même (pour ne
/// jamais recouvrir l'icône survolée, DEMANDE EXPLICITE).
pub enum Anchor {
    /// Icône de la barre du haut : l'infobulle apparaît en dessous.
    Below,
    /// Icône de la barre du BAS : l'infobulle apparaît AU-DESSUS — même règle
    /// que les aperçus, l'infobulle s'écarte toujours de sa barre (voir
    /// `tooltip_anchor`).
    Above,
    /// Icône de la barre de gauche : l'infobulle apparaît à droite.
    RightOf,
    /// Icône de la barre de droite : l'infobulle apparaît à gauche.
    LeftOf,
}

/// Fenêtre unique, créée une fois par `app::run` et réutilisée à chaque
/// frame (redimensionnée/repositionnée selon le texte affiché) plutôt que
/// recréée — un `CreateWindowExW`/`RegisterClassExW` par frame serait
/// inutilement coûteux pour une infobulle qui peut rester affichée plusieurs
/// secondes d'affilée.
pub struct TooltipWindow {
    hwnd: HWND,
    swapchain: Option<SwapChain>,
    renderer: QuadRenderer,
    /// (largeur, hauteur, rayon en bits) de la dernière région arrondie
    /// posée via `SetWindowRgn` — voir `win32::cef_popup::CefPopupState::
    /// applied_region`, même principe : évite de reposer une région
    /// identique à chaque frame.
    applied_region: Option<(i32, i32, u32)>,
    /// L'opacité animée — DEMANDE EXPLICITE : « le fondu des infobulles et des
    /// menus ». Voir `win32::fade`, qui porte la mécanique et le pourquoi d'un
    /// fondu de FENÊTRE (`WS_EX_LAYERED` + `LWA_ALPHA`) plutôt que de contenu.
    /// Sa durée est celle des infobulles (`fade::tooltip_fade_ms`), distincte de
    /// celle des menus — l'infobulle a déjà ses propres réglages.
    fade: fade::WindowFade,
    /// Vrai tant que la fenêtre est AFFICHÉE (elle ne l'est pas en permanence :
    /// voir `show`/`hide`). Distinct de `fade`, qui décrit son opacité : c'est
    /// `shown` qui dit si `ShowWindow(SW_HIDE)` a quelque chose à faire, et
    /// c'est lui qui décide si une apparition doit REPARTIR de zéro ou reprendre
    /// un fondu de sortie interrompu (voir `show`). Même couple que
    /// `window_previews::PreviewPanel::visible`/`fade`.
    shown: bool,
    /// Vrai pour une infobulle de PAGE (voir `cef_popup::infobulle_de_page`) :
    /// elle doit passer DEVANT le popup qui la porte, là où celle d'une icône
    /// passe sous un menu ouvert (voir `show`). Retenu pour `reassert_on_top`.
    au_dessus_des_popups: bool,
    /// Le dernier fond capturé et flouté (voir `show`), gardé d'une frame à
    /// l'autre — `None` tant qu'aucune capture n'a réussi.
    fond: Option<Texture>,
    /// Rectangle ÉCRAN de `fond` et instant de sa capture (voir
    /// `BACKDROP_REFRESH`/`BACKDROP_TOLERANCE_PX`).
    fond_rect: Option<([i32; 4], Instant)>,
    /// Numéro de la dernière capture RÉUSSIE de `fond` — entre dans
    /// `dernier_dessin`, donc dans la décision de redessiner (voir `show`).
    fond_seq: u64,
    /// Ce que la dernière image composée DÉCRIT, ou `None` si plus rien de
    /// valide n'est à l'écran — voir `show`, qui ne redessine que si cela
    /// change.
    dernier_dessin: Option<DessinRetenu>,
}

/// Signature de la dernière image composée dans le swap chain de l'infobulle
/// (voir `TooltipWindow::show`) : taille de la fenêtre, texture du texte, et
/// numéro de la capture de fond dessinée. La POSITION n'y figure pas à
/// dessein : `SetWindowPos` déplace la fenêtre avec son contenu, sans qu'il
/// faille redessiner.
#[derive(Clone, Copy, PartialEq, Eq)]
struct DessinRetenu {
    largeur: u32,
    hauteur: u32,
    texte: usize,
    fond_seq: u64,
}

impl TooltipWindow {
    pub fn new(gfx: &GfxDevice) -> Result<Self> {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None)?.into();
            let class_name = w!("NeuraDeskTooltipWindow");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(tooltip_wndproc),
                hInstance: instance,
                lpszClassName: class_name,
                ..Default::default()
            };
            // Une seule `TooltipWindow` est créée par processus (voir
            // `app::run`) — pas besoin de tolérer un double enregistrement de
            // classe comme `win32::window::create_dock`.
            RegisterClassExW(&wc);

            // `WS_EX_LAYERED` : c'est ce qui autorise `SetLayeredWindowAttributes`
            // (`LWA_ALPHA`), donc le FONDU d'apparition et de disparition — voir
            // `fade` et `tick`. Même ajout que `window_previews`, qui fond depuis
            // toujours. Sans conséquence sur le survol : cette fenêtre est
            // `WS_EX_TRANSPARENT`, donc insensible à la souris de toute façon, et
            // `LWA_ALPHA` ne change de toute manière pas le test de survol (voir
            // `win32::fade`, doc de module).
            let ex_style = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED;
            let hwnd = CreateWindowExW(
                ex_style,
                class_name,
                w!("NeuraDesk Tooltip"),
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

            // Transparence réelle composée par DWM (voir la doc de module) et
            // politique de capture — même politique que le reste de
            // l'interface (voir `win32::cef_popup::make_transparent_and_uncapturable`) :
            // `capture_visibility::apply_to_window` pose les DEUX attributs
            // (affinité d'affichage + exclusion de la Desktop Duplication),
            // voir sa doc.
            let margins = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
            crate::gfx::capture_visibility::apply_to_window(hwnd);

            Ok(Self {
                hwnd,
                swapchain: None,
                renderer: QuadRenderer::new(&gfx.device)?,
                applied_region: None,
                // Un fondu NEUF est opaque et sans rien de posé (voir
                // `WindowFade::new`) : c'est `show` qui le fait repartir de zéro
                // à la première apparition.
                fade: fade::WindowFade::new(),
                shown: false,
                au_dessus_des_popups: false,
                fond: None,
                fond_rect: None,
                fond_seq: 0,
                dernier_dessin: None,
            })
        }
    }

    /// Affiche l'infobulle `text_texture` (déjà rendue via `gfx::text::render_text`,
    /// `text_w`/`text_h` ses dimensions en pixels) juste à côté de `icon_rect`
    /// (rectangle de l'icône survolée en coordonnées ÉCRAN, `[x, y, w, h]`),
    /// du côté indiqué par `anchor`, avec le même matériau que les popups
    /// "menu" (voir la doc de module).
    ///
    /// Bornée au moniteur qui porte l'icône survolée (et pas forcément
    /// l'écran principal — une icône de barre secondaire peut être sur un
    /// autre moniteur), DEMANDE EXPLICITE : "en faisant attention à ce
    /// qu'elles n'apparaissent hors écran". Le côté choisi par `anchor` place
    /// déjà l'infobulle à l'opposé de la barre portant l'icône, donc jamais
    /// par-dessus elle (deuxième demande explicite) — le clamp ci-dessous ne
    /// fait que la ramener dans l'écran, sans jamais la faire passer de
    /// l'autre côté de l'icône.
    /// `bar_edge` : la coordonnée ÉCRAN de l'**arête de la barre qui fait face à la
    /// zone libre**, sur l'axe de `anchor` — son HAUT pour une barre du bas, son BAS
    /// pour celle du haut, sa DROITE pour celle de gauche, sa GAUCHE pour celle de
    /// droite. Voir pourquoi le décalage part de LÀ et non du bord de la case.
    pub fn show(
        &mut self,
        gfx: &GfxDevice,
        icon_rect: [f32; 4],
        anchor: Anchor,
        bar_edge: f32,
        text_texture: &Texture,
        text_w: f32,
        text_h: f32,
        au_dessus_des_popups: bool,
    ) -> Result<()> {
        self.au_dessus_des_popups = au_dessus_des_popups;
        // LE FONDU D'APPARITION (voir `fade`) — et l'ORDRE de ces deux appels
        // est le sujet : `show` est appelé à CHAQUE frame tant que l'infobulle
        // est due, donc repartir de zéro à chaque appel la ferait clignoter
        // indéfiniment. On ne repart de zéro que si la fenêtre était CACHÉE
        // (`!shown`) ; sinon on se contente de viser la pleine opacité, ce qui
        // laisse une infobulle en train de s'éteindre se rallumer doucement au
        // lieu de claquer. Même couple que `window_previews::PreviewPanel::show`.
        if !self.shown {
            self.fade.restart();
        }
        self.fade.fade_in();
        // `text_w`/`text_h` sont DÉJÀ à l'échelle (voir `app::run`, qui passe
        // `TOOLTIP_FONT_PX * tooltip_scale()` à `gfx::text::render_text`) —
        // seules les marges ci-dessous ont besoin d'être mises à l'échelle
        // ici pour rester proportionnées au texte.
        let scale = tooltip_scale();
        let padding = PADDING * scale;
        let gap = GAP * scale;
        let box_w = text_w + padding * 2.0;
        let box_h = text_h + padding * 2.0;
        let [ix, iy, iw, ih] = icon_rect;
        // ⚠️ **LE DÉCALAGE PART DE L'ARÊTE DE LA BARRE, PAS DU BORD DE LA CASE.**
        //
        // La case d'une icône est **CENTRÉE dans l'épaisseur de sa barre** (voir
        // `layout_bottom_side` et `layout_side`) : son bord est donc **À L'INTÉRIEUR**
        // de la bande, de `(épaisseur − case) / 2` — 12 px pour une barre de 80 et une
        // case de 56. Une infobulle posée à `iy − gap` mordait donc **sur la barre**
        // de `12 − gap` pixels. Défaut signalé : « l'infobulle « Espace 18 px » se
        // place encore au-dessus des points de séparateur, empêche-la de venir sur les
        // barres ».
        //
        // ⇒ On se décale depuis l'ARÊTE (`bar_edge`), qui est la seule à dire où la
        // barre finit — la case, elle, ne le sait pas. Les quatre côtés en profitaient
        // du même travers : une icône de barre LATÉRALE est centrée dans sa colonne,
        // donc son infobulle naissait elle aussi dans la barre.
        let (mut box_x, mut box_y) = match anchor {
            Anchor::Above => (ix + iw * 0.5 - box_w * 0.5, bar_edge - box_h - gap),
            Anchor::Below => (ix + iw * 0.5 - box_w * 0.5, bar_edge + gap),
            Anchor::RightOf => (bar_edge + gap, iy + ih * 0.5 - box_h * 0.5),
            Anchor::LeftOf => (bar_edge - box_w - gap, iy + ih * 0.5 - box_h * 0.5),
        };

        let (mon_x, mon_y, mon_w, mon_h) = monitor_bounds((ix + iw * 0.5) as i32, (iy + ih * 0.5) as i32);
        box_x = box_x.clamp(mon_x as f32, (mon_x + mon_w) as f32 - box_w);
        box_y = box_y.clamp(mon_y as f32, (mon_y + mon_h) as f32 - box_h);

        let width = (box_w.round() as u32).max(1);
        let height = (box_h.round() as u32).max(1);

        unsafe {
            // DEMANDE EXPLICITE ("fais en sorte que les infobulles se placent
            // au premier plan par rapport à l'interface de l'app") :
            // `HWND_TOPMOST` (plutôt que `SWP_NOZORDER`, qui ne fait que
            // conserver la position actuelle dans la pile) réaffirme le rang
            // topmost à CHAQUE apparition — entre fenêtres qui ont toutes
            // `WS_EX_TOPMOST` (dock, popups CEF), l'ordre réel dépend de qui a
            // été positionné en dernier, pas seulement du style ; sans ce
            // ré-appel l'infobulle pouvait rester sous le dock ou un popup
            // déjà topmost.
            //
            // **MAIS JAMAIS AU-DESSUS D'UN MENU OUVERT** — DEMANDE EXPLICITE,
            // seconde : « le menu passe toujours au-dessus des infobulles ».
            // Le ré-appel ci-dessus, tel quel, plaçait l'infobulle en TÊTE de la
            // bande topmost, donc au-dessus de tout, menus compris : DÉFAUT RÉEL
            // SIGNALÉ (« quand un élément affiche une infobulle, le menu associé
            // se trouve masqué par l'infobulle »). Quand un popup est visible, on
            // donne donc son HWND en `hWndInsertAfter` : c'est le seul moyen
            // d'insérer la fenêtre **juste en dessous** d'une autre, là où aucun
            // style ne sait l'exprimer. Sans popup, on réaffirme `HWND_TOPMOST`
            // comme avant.
            //
            // **SAUF L'INFOBULLE D'UNE PAGE** (`au_dessus_des_popups`) : elle
            // appartient au popup survolé, et passer sous lui la rendait
            // invisible — elle va donc en tête de la bande topmost.
            let insert_after = self.insertion();
            SetWindowPos(
                self.hwnd,
                insert_after,
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
        let swapchain = self.swapchain.as_ref().expect("swapchain vient d'être créé/redimensionné ci-dessus");

        // Coins VRAIMENT arrondis (pas seulement via l'alpha du shader) —
        // même remède que `win32::cef_popup::render` pour le même bug
        // ("derrière les arrondis des menus du noir apparait") : le mélange
        // alpha DWM seul aux coins est fragile, un `SetWindowRgn` réel ne
        // dépend d'aucune composition pour être net.
        let radius = menu::menu_corner_radius();
        let region_key = (width as i32, height as i32, radius.to_bits());
        if self.applied_region != Some(region_key) {
            self.applied_region = Some(region_key);
            unsafe {
                if radius > 0.0 {
                    let d = (radius * 2.0).round() as i32;
                    let region = CreateRoundRectRgn(0, 0, width as i32, height as i32, d, d);
                    // `SetWindowRgn` prend possession de `region` — voir
                    // `win32::window::set_dock_region`, même remarque : pas
                    // de `DeleteObject` ici.
                    let _ = SetWindowRgn(self.hwnd, Some(region), true);
                } else {
                    let _ = SetWindowRgn(self.hwnd, None, true);
                }
            }
        }
        // Fond flouté du bureau sous l'infobulle, teinté avec la MÊME
        // couleur/force que les popups "menu" — voir la doc de module.
        //
        // ⛔ **LE FOND N'EST PLUS RECAPTURÉ À CHAQUE FRAME** — MESURÉ le
        // 2026-11-11. La capture elle-même n'est pas le coût (~100 µs, son
        // `AcquireNextFrame` est à délai NUL) : c'est **son effet** — un fond
        // neuf à chaque frame rend chaque image DIFFÉRENTE, donc imposait un
        // redessin et un `Present` à chaque frame, et un `Present` sur CETTE
        // fenêtre coûte **~6 ms**, même en `Present(0)` (voir
        // `SwapChain::present_sans_attente`). La boucle tombait de 143 à
        // 68 images/s et la magnification SACCADAIT — DÉFAUT SIGNALÉ : « les
        // animations saccadent même en mettant les performances sur qualité ».
        // (Contrôle : infobulles coupées, la même magnification tient
        // 136 images/s sur le même écran.) Le fond est donc GARDÉ, et
        // rafraîchi à `BACKDROP_REFRESH` près — ou tout de suite s'il a changé
        // de taille, s'il a bougé de plus de `BACKDROP_TOLERANCE_PX`, ou si
        // l'infobulle vient de réapparaître (`!self.shown`).
        let screen_rect = [box_x.round() as i32, box_y.round() as i32, width as i32, height as i32];
        if fond_perime(self.fond_rect, self.shown, screen_rect, Instant::now()) {
            // ⚠️ L'INSTANT EST NOTÉ MÊME QUAND LA CAPTURE ÉCHOUE : sans cela un
            // échec persistant retenterait une capture (et, en cas de succès,
            // un redessin) à chaque frame. L'ancien fond reste dessiné
            // entre-temps.
            if let Some(tex) =
                backdrop::capture_blurred_batch(&gfx.device, &[screen_rect]).into_iter().next().and_then(Result::ok)
            {
                self.fond = Some(tex);
                self.fond_seq += 1;
            }
            self.fond_rect = Some((screen_rect, Instant::now()));
        }

        let menu_rgb = menu::menu_accent_rgb();
        let tint_rgba = [menu_rgb[0], menu_rgb[1], menu_rgb[2], menu::menu_tint()];
        let backdrop_instance = QuadInstance::new_backdrop([0.0, 0.0, box_w, box_h], tint_rgba, radius);
        let text_instance = QuadInstance::new_glyph([padding, padding, text_w, text_h], text_color());
        let mut batches: Vec<QuadBatch> = Vec::new();
        if let Some(fond) = &self.fond {
            batches.push(QuadBatch::new(Some(&fond.srv), std::slice::from_ref(&backdrop_instance)));
        }
        batches.push(QuadBatch::new(Some(&text_texture.srv), std::slice::from_ref(&text_instance)));

        // ⛔ **ON NE REDESSINE QUE QUAND L'IMAGE CHANGE** — MESURÉ le
        // 2026-11-11 : un `Present` sur CETTE fenêtre coûte ~6 ms même sans
        // synchronisation (`present_sans_attente`, essayé), soit une seconde
        // frame perdue derrière le dock à chaque frame de la boucle. Or rien
        // ne change tant que l'infobulle reste sur la même icône : le fondu
        // passe par `SetLayeredWindowAttributes` (`fade.push`), la position
        // par `SetWindowPos` — ni l'un ni l'autre ne demande un nouveau
        // dessin. Seuls comptent la taille, le TEXTE et la capture de fond.
        let signature = DessinRetenu {
            largeur: width,
            hauteur: height,
            texte: text_texture.srv.as_raw() as usize,
            fond_seq: self.fond_seq,
        };
        if self.dernier_dessin != Some(signature) {
            let origin = crate::win32::window::hwnd_screen_origin(self.hwnd).unwrap_or((0, 0));
            self.renderer.render_at(&gfx.context, swapchain.rtv().expect("rtv toujours présente après resize/new"), box_w, box_h, &batches, origin)?;
            swapchain.present_sans_attente()?;
            self.dernier_dessin = Some(signature);
        }

        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        // L'OPACITÉ EST POSÉE DANS LA FOULÉE (voir `WindowFade::push`) : la
        // fenêtre vient d'être montrée, et une fenêtre `LAYERED` neuve porte
        // l'alpha OPAQUE par défaut — sans ce `push`, l'infobulle se composerait
        // entière pendant la frame qui précède le premier pas de son fondu, donc
        // un éclair au lieu d'une apparition.
        self.fade.push(self.hwnd);
        self.shown = true;
        Ok(())
    }

    /// Réaffirme le rang de l'infobulle SANS la déplacer, la redimensionner ni
    /// l'activer — à appeler APRÈS tout `Window::raise_to_top` du dock.
    ///
    /// **POURQUOI ELLE EXISTE, alors que `show` pose déjà ce rang** : `app::run`
    /// appelle `raise_to_top` PLUS BAS dans la même frame, dès qu'un élément est
    /// agrandi. Le dock passait donc devant une infobulle DÉJÀ affichée, et elle
    /// disparaissait sous les barres. DÉFAUT RÉEL SIGNALÉ : « les infobulles
    /// apparaissent de nouveau sous les barres de droite et de gauche au lieu de
    /// s'afficher par-dessus ». **L'ordre est tout l'objet du correctif**, comme
    /// pour `cef_popup::raise_visible_popups`, sa jumelle.
    ///
    /// Elle réutilise la même insertion que `show` — donc SOUS un menu visible
    /// (voir `visible_popup_hwnd`) : l'ordre menus > infobulle > dock est préservé,
    /// et c'est bien lui qu'on veut.
    pub fn reassert_on_top(&self) {
        let insert_after = self.insertion();
        unsafe {
            let _ = SetWindowPos(self.hwnd, insert_after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    /// `hWndInsertAfter` de l'infobulle — voir `show`.
    fn insertion(&self) -> Option<HWND> {
        if self.au_dessus_des_popups {
            return Some(HWND_TOPMOST);
        }
        crate::win32::cef_popup::visible_popup_hwnd().or(Some(HWND_TOPMOST))
    }

    /// Engage la DISPARITION de l'infobulle — DEMANDE EXPLICITE : « le fondu
    /// des infobulles et des menus ».
    ///
    /// **La fenêtre n'est PAS cachée ici** : elle s'éteint d'abord, et c'est
    /// `tick` qui la cachera une fois l'opacité à zéro. Idempotent, et c'est
    /// nécessaire : `app::run` appelle `hide` à CHAQUE frame où il n'y a rien à
    /// afficher (voir le `match` de la boucle principale).
    pub fn hide(&mut self) {
        self.fade.fade_out();
    }

    /// Avance le fondu et cache la fenêtre quand il est terminé — **À APPELER À
    /// CHAQUE FRAME**, même quand aucune infobulle n'est due : c'est ce pas-là
    /// qui termine une disparition, donc ce qui finit par cacher la fenêtre.
    ///
    /// Elle ne prend AUCUNE durée écoulée : le fondu mesure lui-même le temps
    /// entre deux pas (voir `WindowFade::tick`), ce qui évite de faire descendre
    /// un `dt` jusqu'ici depuis la boucle principale.
    pub fn tick(&mut self) {
        self.fade.tick(self.hwnd, fade::tooltip_fade_ms());
        if self.shown && self.fade.is_hidden() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.shown = false;
        }
    }

    /// Copie du back buffer pour le harnais de capture (voir
    /// `win32::debug_capture`) — même contrat que `cef_popup::
    /// debug_dump_backbuffer` : `false` si l'infobulle n'a jamais rien affiché.
    ///
    /// **Le device est PASSÉ, contrairement aux popups CEF** qui le conservent
    /// dans leur état : cette fenêtre-ci n'en a aucun autre besoin, elle reçoit
    /// un `&GfxDevice` à chaque `show`, et le garder serait un champ de plus
    /// pour rien.
    ///
    /// Elle manquait : l'infobulle était la SEULE fenêtre du projet sans copie
    /// de back buffer, donc la seule dont le contenu — et la cote d'un
    /// séparateur de zone s'y affiche (voir `ui::dock::Dock::spacer_readout`) —
    /// n'était vérifiable que par un test unitaire de la DÉCISION, jamais du
    /// rendu.
    pub fn debug_dump_backbuffer(&self, gfx: &GfxDevice, path: &std::path::Path) -> bool {
        let Some(swapchain) = &self.swapchain else { return false };
        let Ok(back_buffer) = swapchain.back_buffer() else { return false };
        crate::gfx::capture::dump_backbuffer_composited(&gfx.device, &gfx.context, &back_buffer, path).is_ok()
    }
}

/// Rectangle (coordonnées écran) du moniteur le plus proche du point donné —
/// voir `TooltipWindow::show`.
pub(crate) fn monitor_bounds(x: i32, y: i32) -> (i32, i32, i32, i32) {
    unsafe {
        let monitor: HMONITOR = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let r = info.rcMonitor;
            (r.left, r.top, r.right - r.left, r.bottom - r.top)
        } else {
            // Ne devrait jamais arriver en pratique (`MonitorFromPoint` avec
            // `MONITOR_DEFAULTTONEAREST` renvoie toujours un moniteur
            // valide) — repli sur l'écran principal.
            use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
            (0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
        }
    }
}

/// Procédure de fenêtre de l'infobulle — un simple relais vers
/// `DefWindowProcW` (voir `win32::window::appbar_helper_wndproc`, même
/// principe) : `WS_EX_TRANSPARENT` fait que cette fenêtre ne reçoit de toute
/// façon jamais d'entrée souris/clavier réelle.
unsafe extern "system" fn tooltip_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECT: [i32; 4] = [100, 200, 160, 40];

    /// Le défaut mesuré le 2026-11-11 : une infobulle immobile doit garder son
    /// fond, sinon chaque fond neuf impose un redessin et un `Present` (~6 ms
    /// chacun) à chaque frame, et la magnification tombe de 143 à 68 i/s.
    #[test]
    fn un_fond_recent_et_immobile_n_est_pas_recapture() {
        let maintenant = Instant::now();
        assert!(!fond_perime(Some((RECT, maintenant)), true, RECT, maintenant));
    }

    #[test]
    fn le_fond_expire_apres_le_delai() {
        let pris_a = Instant::now();
        let maintenant = pris_a + BACKDROP_REFRESH;
        assert!(fond_perime(Some((RECT, pris_a)), true, RECT, maintenant));
    }

    /// L'infobulle suit l'icône pendant la magnification : un fond pris pour
    /// un autre emplacement se verrait, mais un déplacement de quelques pixels
    /// ne justifie pas les ~6 ms d'une nouvelle capture.
    #[test]
    fn un_petit_deplacement_garde_le_fond_un_grand_le_rafraichit() {
        let maintenant = Instant::now();
        let proche = [RECT[0] + BACKDROP_TOLERANCE_PX, RECT[1] - BACKDROP_TOLERANCE_PX, RECT[2], RECT[3]];
        assert!(!fond_perime(Some((RECT, maintenant)), true, proche, maintenant));
        let loin = [RECT[0] + BACKDROP_TOLERANCE_PX + 1, RECT[1], RECT[2], RECT[3]];
        assert!(fond_perime(Some((RECT, maintenant)), true, loin, maintenant));
    }

    #[test]
    fn un_changement_de_taille_ou_une_reapparition_rafraichit_le_fond() {
        let maintenant = Instant::now();
        let autre_taille = [RECT[0], RECT[1], RECT[2] + 1, RECT[3]];
        assert!(fond_perime(Some((RECT, maintenant)), true, autre_taille, maintenant));
        assert!(fond_perime(Some((RECT, maintenant)), false, RECT, maintenant));
        assert!(fond_perime(None, true, RECT, maintenant));
    }
}
