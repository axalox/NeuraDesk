//! Aperçu d'un BUREAU VIRTUEL — la miniature du bureau dont on survole la
//! pastille dans la barre du haut : CHAQUE fenêtre de ce bureau, à sa position
//! réelle sur le moniteur, réduite à l'échelle. C'est exactement ce que montre
//! Task View de Windows (DEMANDE EXPLICITE : « une prévisualisation des bureaux
//! virtuels, comme Windows »).
//!
//! **CE QUI CHANGE PAR RAPPORT AU PANNEAU DE VIGNETTES** (`win32::
//! window_previews`, qui continue de servir les icônes du dock) : ici, RIEN
//! n'est capturé. Les fenêtres sont montrées par une vignette DWM
//! (`DwmRegisterThumbnail`) posée dans une fenêtre à nous, à la place que
//! chacune occupe sur son écran — au lieu d'être mises côte à côte dans une
//! liste, chacune capturée par `gfx::wgc`.
//!
//! **UNE VIGNETTE DWM FRANCHIT LES BUREAUX VIRTUELS, ET C'EST MESURÉ.**
//! C'était la seule question ouverte avant d'écrire ce module : `gfx::wgc`
//! (Windows Graphics Capture) ne rend RIEN d'une fenêtre située sur un bureau
//! inactif, Windows gelant son dessin — d'où l'« impasse » que la mémoire du
//! projet donnait pour établie. La sonde `win32::thumb_probe` a établi le
//! contraire pour les vignettes DWM, parce que c'est le COMPOSITEUR qui les
//! dessine et non nous. **Deux faits de cette sonde gouvernent ce fichier** :
//! la vignette se compose PAR-DESSUS le contenu de la fenêtre de destination
//! (d'où l'ordre de `show` : le fond d'abord, les vignettes ensuite), et la
//! fenêtre de destination doit être `WS_EX_TOPMOST`, sans quoi elle naît
//! DERRIÈRE les fenêtres de l'utilisateur — c'est-à-dire derrière celles
//! qu'elle doit précisément montrer.
//!
//! **POURQUOI ELLE SE JUGE À L'ŒIL** : une vignette DWM est composée par DWM,
//! donc elle n'existe dans aucun de nos tampons. Le harnais de capture
//! (`win32::debug_capture`, qui relit des back buffers) ne peut pas la voir, et
//! aucune assertion ne peut porter sur elle — la sonde ci-dessus est elle-même
//! restée un « regardez et dites » pour cette raison.
//!
//! **UN TROISIÈME FAIT, APPRIS À L'USAGE — UNE VIGNETTE REFUSÉE NE DESSINE
//! RIEN.** Le premier essai de ce module a rendu un aperçu où toutes les fenêtres
//! MAXIMISÉES manquaient, et elles seules. La chaîne : `GetWindowRect` rend le
//! rectangle de gestion d'une fenêtre, bords invisibles de redimensionnement
//! compris, donc une fenêtre maximisée DÉBORDE du moniteur de quelques pixels ;
//! mise à l'échelle, sa destination sortait du cadre de l'aperçu ; et une
//! destination hors cadre n'est pas rognée par DWM, elle est REFUSÉE — les
//! valeurs par défaut de `DWM_THUMBNAIL_PROPERTIES` étant `fVisible: FALSE,
//! opacity: 0`, la fenêtre n'est pas mal placée, elle est ABSENTE. D'où les deux
//! règles que porte ce fichier : les bornes viennent de
//! `DWMWA_EXTENDED_FRAME_BOUNDS` (ce qui se VOIT), et toute destination est
//! ramenée dans le cadre avant d'être posée (voir `visible_bounds` et
//! `clip_rect`). Corollaire : ne jamais jeter le résultat d'un
//! `DwmUpdateThumbnailProperties` — c'est l'erreur muette qui a rendu ce
//! diagnostic long.
//!
//! **LE FOND EST LE FOND D'ÉCRAN DU BUREAU, ET RIEN D'AUTRE** (2026-09-20) —
//! DEMANDE EXPLICITE : « je ne vois pas le fond d'écran du bureau ». C'était
//! auparavant le MATÉRIAU DES MENUS (capture du bureau floutée + teinte d'accent,
//! `gfx::backdrop::capture_blurred_batch`), donc une vitre dépoli posée sur ce
//! qu'il y a DERRIÈRE la carte — pas une miniature de bureau. Windows range un
//! fond **par bureau virtuel** (voir `desktops::wallpapers_by_desktop`), et c'est
//! lui qu'on dessine, sans flou ni teinte. Le **repli** est un aplat sombre quand
//! aucun fond n'est lisible. Seul le RAYON reste emprunté aux menus
//! (`win32::menu::menu_corner_radius`, par `SetWindowRgn`, voir `show`) : c'est ce
//! qui range cette fenêtre dans la famille des fenêtres flottantes.
//!
//! ⚠️ **L'IMAGE EST ÉTIRÉE AU CADRE**, et ce cadre a le rapport du **moniteur**.
//! Pour un fond au même rapport que l'écran c'est exact ; un fond d'un autre
//! rapport serait déformé. Le réglage « ajuster » de Windows (Remplir / Ajuster /
//! Mosaïque / Étirer / Centrer / Recouvrir) n'est **pas** lu — lire
//! `IDesktopWallpaper::GetPosition` n'aurait d'ailleurs pas suffi, cette interface
//! ne rendant que le fond du bureau COURANT.
//!
//! **RÉGLAGES — TROIS, ET PROPRES À CETTE FENÊTRE** (2026-09-20) : largeur de la
//! miniature, durée du fondu, délai de survol. Ils vivent dans des cellules de
//! ce module (voir `PREVIEW_WIDTH`) et sont exposés dans une section « Aperçu
//! des bureaux virtuels » des Réglages.
//!
//! **CE QUI SUIT A ÉTÉ VRAI ET NE L'EST PLUS** — gardé parce que la décision
//! inverse avait ses raisons, et qu'on les relira : « le fondu est celui des
//! menus (`fade::menu_fade_ms`), décision assumée plutôt qu'un troisième
//! curseur : cette fenêtre emprunte le matériau des menus, elle en emprunte donc
//! la vitesse — et un réglage de plus pour une fenêtre qui n'apparaît qu'au
//! survol d'une pastille serait un curseur que personne n'irait chercher ».
//! L'utilisateur l'a demandé explicitement : le fondu est désormais propre à la
//! carte, et accélérer les menus ne l'accélère plus.


use windows::core::{w, Result};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmGetWindowAttribute, DwmRegisterThumbnail, DwmUnregisterThumbnail,
    DwmUpdateThumbnailProperties, DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_VISIBLE,
    DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, RegisterClassExW,
    SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, HWND_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::gfx::device::GfxDevice;
use crate::gfx::quad_renderer::{QuadBatch, QuadInstance, QuadRenderer};
use crate::gfx::swapchain::SwapChain;
use crate::gfx::Texture;
use crate::win32::desktops::{wallpapers_by_desktop, DesktopWindow};
use crate::win32::{fade, menu, tooltip};

/// Largeur de la miniature, en pixels — la SEULE dimension choisie : la hauteur
/// en découle par le rapport du moniteur. Les choisir toutes les deux
/// indépendamment déformerait la miniature, donc placerait chaque fenêtre
/// ailleurs que là où elle est.
///
/// C'est le DÉFAUT du réglage : 520 px est la valeur qui a servi tant que la
/// taille était une constante, donc une installation neuve reproduit exactement
/// l'aperçu d'avant.
pub const DEFAULT_PREVIEW_WIDTH: f32 = 520.0;
/// Bornes de la largeur, réglable dans les Réglages.
///
/// Le plancher de 240 px garde la carte lisible.
///
/// **LE PLAFOND A ÉTÉ DOUBLÉ (1000 → 2000) SUR DEMANDE EXPLICITE** : « j'aimerais
/// pouvoir encore augmenter la taille de la fenêtre de prévisualisation du bureau
/// à au moins le double du maximum actuel ». La justification d'origine — garder
/// la carte entière à l'écran sur un petit moniteur — était trop prudente : elle
/// privait l'utilisateur de détail sur une installation où la place ne manque
/// pas. Un plafond plus haut (3000) a été proposé puis ÉCARTÉ par l'utilisateur :
/// le double est ce qu'il demandait.
///
/// ⚠️ **AU-DELÀ DE LA TAILLE DU MONITEUR, LA CARTE DÉBORDE.** `clamp_within`
/// (`:573`) la ramène alors au bord au lieu de paniquer — c'est le cas qu'il a
/// été écrit pour couvrir, et un test le fige. La contrainte qui mord désormais
/// est donc le MONITEUR, pas cette constante : monter plus haut n'ajoute pas de
/// détail, ça rogne la carte.
pub const MIN_PREVIEW_WIDTH: f32 = 240.0;
pub const MAX_PREVIEW_WIDTH: f32 = 2000.0;
/// Plafond de la texture du fond d'écran, en pixels.
///
/// La carte fait au plus `MAX_PREVIEW_WIDTH` de large, donc garder un fond 4K
/// (3840×2160) en mémoire vidéo serait du gaspillage ET un décodage plus lent
/// pour un détail que personne ne verra.
const WALLPAPER_MAX_PX: u32 = 2048;
/// Le fond de repli, quand aucun fond d'écran n'a pu être lu : un **aplat
/// sombre**, choisi par l'utilisateur. Ni flou, ni teinte — dans ce cas la carte
/// ne prétend rien, elle se contente d'être lisible.
const FALLBACK_BACKGROUND: [f32; 4] = [0.06, 0.06, 0.07, 1.0];
/// Durée du fondu de la carte, en millisecondes.
///
/// **ELLE ÉTAIT CELLE DES MENUS** (`fade::menu_fade_ms`) jusqu'au 2026-09-20 :
/// cette fenêtre emprunte leur matériau, elle empruntait donc leur vitesse. Elle
/// a désormais SON curseur, sur demande explicite — accélérer les menus ne doit
/// plus accélérer la carte. Les bornes sont celles de `win32::fade` : `0` est
/// l'ARRÊT du fondu, un état légitime et testé là-bas
/// (`a_zero_duration_lands_on_the_target_in_one_step`), qui redonne
/// l'apparition sèche d'avant ce module.
pub const DEFAULT_FADE_MS: f32 = 150.0;
pub const MIN_FADE_MS: f32 = 0.0;
pub const MAX_FADE_MS: f32 = 1200.0;
/// Délai de survol avant l'apparition, en millisecondes — `0` par défaut, donc
/// le comportement d'avant ce réglage : la carte s'ouvre dès l'entrée sur la
/// pastille.
///
/// ⚠️ **IL S'AJOUTE AU FONDU**, et c'est assumé ici alors que le projet avait
/// RETIRÉ le délai fixe de 350 ms du panneau d'aperçus pour cette raison même
/// (voir la note de `app.rs` au-dessus de `PREVIEW_HIDE_GRACE`) : la différence
/// est qu'ici c'est un réglage nommé et visible, dont le zéro rend exactement le
/// comportement d'origine. Ne pas « corriger » l'addition sans relire ça.
pub const DEFAULT_HOVER_DELAY_MS: f32 = 0.0;
pub const MIN_HOVER_DELAY_MS: f32 = 0.0;
pub const MAX_HOVER_DELAY_MS: f32 = 1500.0;
/// Écart entre le bas de la pastille survolée et le haut de l'aperçu.
const GAP: f32 = 8.0;

// Valeurs courantes des trois réglages — même mécanisme que
// `window_previews::FADE_MS` et `fade::MENU_FADE_MS` : la cellule **est** le
// stockage, relue à chaque frame, ce qui fait agir un curseur à chaud sans que
// `SettingsRefs` ait à porter quoi que ce soit.
thread_local! {
    static PREVIEW_WIDTH: std::cell::Cell<f32> = const { std::cell::Cell::new(DEFAULT_PREVIEW_WIDTH) };
    static FADE_MS: std::cell::Cell<f32> = const { std::cell::Cell::new(DEFAULT_FADE_MS) };
    static HOVER_DELAY_MS: std::cell::Cell<f32> = const { std::cell::Cell::new(DEFAULT_HOVER_DELAY_MS) };
}

/// Largeur de la miniature, en pixels — voir `PREVIEW_WIDTH`.
pub fn preview_width() -> f32 {
    PREVIEW_WIDTH.with(std::cell::Cell::get)
}

/// Change la largeur de la miniature, bornée à [`MIN_PREVIEW_WIDTH`,
/// `MAX_PREVIEW_WIDTH`].
///
/// **LE GARDE `NaN` N'EST PAS DÉCORATIF** : `"nan".parse::<f32>()` réussit, donc
/// un fichier de configuration écrit à la main peut porter `nan` ; or `f32::clamp`
/// PROPAGE `NaN` au lieu de le borner, et une largeur `NaN` donne une échelle
/// `NaN`, une boîte `NaN` et une fenêtre de taille nulle — un défaut qui se lit
/// « la fonctionnalité est cassée », pas « le fichier est faux ». Même garde que
/// `ui::dock::set_hover_scale` ; l'infini, lui, doit continuer de SATURER.
pub fn set_preview_width(width: f32) {
    if !width.is_nan() {
        PREVIEW_WIDTH.with(|cell| cell.set(width.clamp(MIN_PREVIEW_WIDTH, MAX_PREVIEW_WIDTH)));
    }
}

/// Durée du fondu de la carte, en millisecondes — voir `FADE_MS`.
pub fn fade_ms() -> f32 {
    FADE_MS.with(std::cell::Cell::get)
}

/// Change la durée du fondu, bornée à [`MIN_FADE_MS`, `MAX_FADE_MS`]. Garde
/// `NaN` identique à `set_preview_width`, même raison.
pub fn set_fade_ms(ms: f32) {
    if !ms.is_nan() {
        FADE_MS.with(|cell| cell.set(ms.clamp(MIN_FADE_MS, MAX_FADE_MS)));
    }
}

/// Délai de survol, en millisecondes — voir `HOVER_DELAY_MS`.
pub fn hover_delay_ms() -> f32 {
    HOVER_DELAY_MS.with(std::cell::Cell::get)
}

/// Change le délai de survol, borné à [`MIN_HOVER_DELAY_MS`,
/// `MAX_HOVER_DELAY_MS`]. Garde `NaN` identique à `set_preview_width`.
pub fn set_hover_delay_ms(ms: f32) {
    if !ms.is_nan() {
        HOVER_DELAY_MS.with(|cell| cell.set(ms.clamp(MIN_HOVER_DELAY_MS, MAX_HOVER_DELAY_MS)));
    }
}

/// Le délai de survol sous la forme qu'attendent les comparaisons d'`Instant`.
///
/// `from_millis` et non `from_secs_f32` : cette dernière PANIQUE sur `NaN`, et
/// un délai est le genre de valeur qu'un fichier de configuration peut porter.
/// La cellule est déjà bornée et non-`NaN`, mais la conversion ne prend aucun
/// risque pour autant.
pub fn hover_delay() -> std::time::Duration {
    std::time::Duration::from_millis(hover_delay_ms().max(0.0) as u64)
}

/// Bornes de la LARGEUR — **SEULE AUTORITÉ** : la page des Réglages les reçoit
/// par l'état poussé (`cef_settings::push_state`) et n'en recopie aucune. Même
/// règle, et même forme, que `ui::dock::bar_thickness_bounds`.
pub fn preview_width_bounds() -> (f32, f32) {
    (MIN_PREVIEW_WIDTH, MAX_PREVIEW_WIDTH)
}

/// Bornes de la DURÉE DU FONDU — voir `preview_width_bounds`.
pub fn fade_bounds() -> (f32, f32) {
    (MIN_FADE_MS, MAX_FADE_MS)
}

/// Bornes du DÉLAI DE SURVOL — voir `preview_width_bounds`.
pub fn hover_delay_bounds() -> (f32, f32) {
    (MIN_HOVER_DELAY_MS, MAX_HOVER_DELAY_MS)
}

/// Fenêtre unique, créée une fois par `app::run` et réutilisée à chaque frame —
/// même choix que `TooltipWindow`, et pour la même raison : un
/// `RegisterClassExW`/`CreateWindowExW` par survol serait inutilement coûteux
/// pour une fenêtre qui se rouvre à chaque pastille visitée.
pub struct DesktopPreviewWindow {
    hwnd: HWND,
    swapchain: Option<SwapChain>,
    renderer: QuadRenderer,
    /// Les vignettes VIVANTES, couplées à leur fenêtre SOURCE.
    ///
    /// **Le couplage n'est pas décoratif** : `DwmUnregisterThumbnail` ne prend
    /// que le `HTHUMBNAIL`, jamais la fenêtre, donc sans la source on ne saurait
    /// plus laquelle retirer quand une fenêtre disparaît de l'instantané (voir
    /// `sync_thumbnails`). **Une vignette oubliée est une fuite** : DWM garde la
    /// fenêtre source référencée aussi longtemps qu'elle vit.
    thumbnails: Vec<(HWND, isize)>,
    /// Le bureau que `thumbnails` représente — `None` dès qu'il n'y en a plus
    /// aucune. **C'est lui qui déclenche la libération GROUPÉE** : changer de
    /// bureau invalide toutes les vignettes d'un coup (elles montrent les
    /// fenêtres du bureau précédent), et les libérer une à une au fil des
    /// réenregistrements laisserait les anciennes à l'écran entre-temps.
    desktop: Option<usize>,
    /// (largeur, hauteur, rayon en bits) de la dernière région arrondie posée
    /// via `SetWindowRgn` — même garde que `win32::tooltip::
    /// TooltipWindow::applied_region` : éviter de reposer une région identique à
    /// chaque frame.
    applied_region: Option<(i32, i32, u32)>,
    /// L'opacité animée — voir la doc de module pour la durée retenue.
    fade: fade::WindowFade,
    /// Vrai tant que la fenêtre est AFFICHÉE (elle ne l'est pas en permanence).
    /// Distinct de `fade`, qui décrit son opacité : c'est `shown` qui dit si
    /// `ShowWindow(SW_HIDE)` a quelque chose à faire, et c'est lui qui décide si
    /// une apparition doit REPARTIR de zéro ou reprendre un fondu de sortie
    /// interrompu (voir `show`).
    shown: bool,
    /// Position et taille ÉCRAN de l'aperçu — c'est ce qui permet de savoir si
    /// le curseur est DESSUS, donc s'il doit rester ouvert pendant que le
    /// pointeur descend de la pastille vers lui (voir `contains_screen_point`).
    screen_rect: [f32; 4],
    /// Le fond d'écran du bureau affiché : son CHEMIN et sa texture, gardés
    /// ensemble.
    ///
    /// **LE CHEMIN EST GARDÉ, PAS SEULEMENT LA TEXTURE** : c'est lui qui dit si
    /// le fond a changé — l'utilisateur peut en changer, et Windows en range un
    /// par bureau virtuel (voir `desktops::wallpapers_by_desktop`). Comparer
    /// deux chemins coûte une comparaison de chaînes ; redécoder coûte un
    /// décodage d'image 4K. On ne redécode donc que sur changement RÉEL.
    ///
    /// `None` = aucun fond lisible (bureau sans fond propre, fichier disparu,
    /// format non décodable) : c'est le cas qui donne le repli uni sombre.
    wallpaper: Option<(std::path::PathBuf, Texture)>,
}

impl DesktopPreviewWindow {
    pub fn new(gfx: &GfxDevice) -> Result<Self> {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None)?.into();
            let class_name = w!("NeuraDeskDesktopPreview");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(desktop_preview_wndproc),
                hInstance: instance,
                lpszClassName: class_name,
                ..Default::default()
            };
            // Une seule `DesktopPreviewWindow` est créée par processus (voir
            // `app::run`) — pas besoin de tolérer un double enregistrement de
            // classe, comme `win32::window::create_dock` doit le faire.
            RegisterClassExW(&wc);

            // `WS_EX_TOPMOST` N'EST PAS UN CONFORT ICI, c'est une condition
            // MESURÉE de la vignette (voir la doc de module) : sans lui la
            // fenêtre naîtrait derrière les fenêtres de l'utilisateur, donc
            // derrière celles qu'elle doit montrer.
            //
            // `WS_EX_TOOLWINDOW` la tient hors d'Alt+Tab et de la barre des
            // tâches : ce n'est pas une fenêtre de l'application.
            //
            // `WS_EX_NOACTIVATE` : survoler une pastille ne doit pas voler le
            // focus de l'application au premier plan (invariant du dock, voir
            // `appicon::focus_or`).
            //
            // `WS_EX_TRANSPARENT` : la fenêtre est INSENSIBLE À LA SOURIS —
            // elle ne mange ni le clic, ni la molette, ni le survol de ce
            // qu'elle recouvre. C'est un PANNEAU, pas une cible ; sans ce style
            // il intercepterait les clics destinés au bureau qu'il recouvre. Le
            // suivi du pointeur qui la fait vivre ne passe pas par elle
            // (`GetCursorPos`, voir `contains_screen_point`), donc rien n'en
            // dépend.
            //
            // `WS_EX_LAYERED` : c'est ce qui autorise
            // `SetLayeredWindowAttributes(LWA_ALPHA)`, donc le FONDU — voir
            // `fade` et `tick`.
            let ex_style = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED;
            let hwnd = CreateWindowExW(
                ex_style,
                class_name,
                w!("NeuraDesk Desktop Preview"),
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

            // Transparence réellement composée par DWM, et politique de capture
            // — mêmes deux appels que `win32::tooltip::TooltipWindow::new`,
            // pour la même raison (voir `capture_visibility::apply_to_window`).
            let margins = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
            crate::gfx::capture_visibility::apply_to_window(hwnd);

            Ok(Self {
                hwnd,
                swapchain: None,
                renderer: QuadRenderer::new(&gfx.device)?,
                thumbnails: Vec::new(),
                desktop: None,
                applied_region: None,
                // Un fondu NEUF est opaque et sans rien de posé (voir
                // `WindowFade::new`) : c'est `show` qui le fait repartir de zéro
                // à la première apparition.
                fade: fade::WindowFade::new(),
                shown: false,
                screen_rect: [0.0; 4],
                wallpaper: None,
            })
        }
    }

    /// S'assure que `wallpaper` porte le fond d'écran du bureau `desktop`,
    /// en ne faisant le travail que si le chemin a changé.
    ///
    /// **À N'APPELER QU'AU CHANGEMENT DE BUREAU**, jamais par frame : `show` est
    /// rappelé à chaque frame tant que la carte est ouverte (mesuré : 248 fois
    /// par ouverture), et la règle du dépôt est explicite — une lecture de
    /// registre ou un décodage d'image ne se fait pas dans la boucle de rendu.
    ///
    /// Le décodage est PLAFONNÉ à `WALLPAPER_MAX_PX` : la carte fait au plus
    /// `MAX_PREVIEW_WIDTH` de large, donc garder un fond 4K en texture serait de
    /// la mémoire gaspillée — et un JPEG 4K se décode lentement. Un fond lu pour
    /// la PREMIÈRE fois coûte donc une fraction de seconde, une seule fois.
    fn reload_wallpaper(&mut self, gfx: &GfxDevice, desktop: usize) {
        let path = wallpapers_by_desktop().into_iter().nth(desktop).flatten();
        // Rien à faire si c'est DÉJÀ ce fond-là — y compris quand les deux sont
        // `None`, qui est un état stable et non un échec à réessayer.
        if self.wallpaper.as_ref().map(|(known, _)| known.clone()) == path {
            return;
        }
        self.wallpaper = path.and_then(|path| {
            let bytes = std::fs::read(&path).ok()?;
            let image = image::load_from_memory(&bytes).ok()?;
            let image = if image.width().max(image.height()) > WALLPAPER_MAX_PX {
                image.thumbnail(WALLPAPER_MAX_PX, WALLPAPER_MAX_PX)
            } else {
                image
            };
            let rgba = image.to_rgba8();
            let texture = Texture::from_rgba(&gfx.device, rgba.width(), rgba.height(), &rgba).ok()?;
            Some((path, texture))
        });
    }

    /// Affiche la miniature du bureau `desktop`, ancrée sous `dot_rect`
    /// (coordonnées ÉCRAN de la pastille survolée).
    ///
    /// `windows` vient de `desktops::windows_by_desktop`. **UN BUREAU VIDE
    /// AFFICHE QUAND MÊME SON CADRE** — le fond, sans aucune vignette — et c'est
    /// ce que fait Windows : son aperçu de bureau est le bureau lui-même, fenêtres
    /// ou pas. Ne rien ouvrir pour un bureau vide laissait au contraire croire
    /// que le survol n'avait pas pris (défaut constaté à l'usage).
    pub fn show(
        &mut self,
        gfx: &GfxDevice,
        desktop: usize,
        windows: &[DesktopWindow],
        dot_rect: [i32; 4],
        libre: [i32; 2],
    ) -> Result<()> {
        // LE BUREAU A-T-IL CHANGÉ ? Si oui, TOUTES les vignettes partent AVANT
        // que les nouvelles n'arrivent : elles montrent les fenêtres du bureau
        // précédent, donc les garder le temps des réenregistrements laisserait
        // un mélange des deux à l'écran.
        // **ET LE FOND D'ÉCRAN CHANGE AVEC LUI** : Windows en range un PAR BUREAU
        // (voir `desktops::wallpapers_by_desktop`), et c'est ICI qu'on le relit —
        // au changement de bureau, jamais par frame.
        if self.desktop != Some(desktop) {
            self.release_thumbnails();
            self.desktop = Some(desktop);
            self.reload_wallpaper(gfx, desktop);
        }

        // GÉOMÉTRIE — la miniature représente le MONITEUR ENTIER qui porte la
        // pastille (et pas forcément l'écran principal : le dock s'affiche aussi
        // sur les écrans secondaires). C'est ce choix qui rend la position de
        // chaque fenêtre directement lisible : tout le moniteur est mis à
        // l'échelle, donc rien n'est ni recadré ni recentré.
        let (mon_x, mon_y, mon_w, mon_h) = tooltip::monitor_bounds(dot_rect[0] + dot_rect[2] / 2, dot_rect[1] + dot_rect[3] / 2);
        let (scale, miniature) = preview_box_entre(dot_rect, [mon_x, mon_y, mon_w, mon_h], preview_width(), libre);
        let [box_x, box_y, box_w, box_h] = miniature;
        let width = box_w as u32;
        let height = box_h as u32;
        self.screen_rect = miniature;

        unsafe {
            // `HWND_TOPMOST` (et non `SWP_NOZORDER`) pour RÉAFFIRMER le rang
            // topmost à chaque apparition — entre fenêtres qui l'ont toutes, la
            // pile réelle dépend de qui a été positionné en dernier. **Mais
            // jamais au-dessus d'un MENU ouvert** : on s'insère sous lui quand
            // il y en a un, seul moyen d'exprimer « juste en dessous » (voir
            // `win32::tooltip::TooltipWindow::show`, même règle, et
            // `reassert_on_top`, sa jumelle).
            let insert_after = crate::win32::cef_popup::visible_popup_hwnd().or(Some(HWND_TOPMOST));
            SetWindowPos(
                self.hwnd,
                insert_after,
                box_x as i32,
                box_y as i32,
                width as i32,
                height as i32,
                SWP_NOACTIVATE,
            )?;
        }

        match &mut self.swapchain {
            Some(sc) => sc.resize(&gfx.device, width, height)?,
            // BitBlt, donc alpha composé par DWM — même raison que les popups
            // CEF : au repli `sans_fond` (loupe indisponible, voir
            // `gfx::quad_renderer`) c'est cet alpha qui laisse voir le bureau à
            // travers le panneau ; en modèle *flip* il serait ignoré et le
            // panneau resterait un aplat opaque assombri.
            None => self.swapchain = Some(SwapChain::new(&gfx.device, self.hwnd, width, height, true)?),
        }
        let swapchain = self.swapchain.as_ref().expect("swapchain vient d'être créé/redimensionné ci-dessus");

        // Coins VRAIMENT arrondis — même remède que `win32::tooltip::show` et
        // `win32::cef_popup::render` pour le même défaut (« derrière les arrondis
        // des menus du noir apparait ») : le fond est rendu en modèle *flip*
        // (voir `gfx::swapchain::SwapChain::new`), dont les pixels jamais
        // dessinés sont NOIRS et opaques — l'alpha du shader ne suffit donc pas
        // à détourer les coins, un `SetWindowRgn` réel si.
        let radius = menu::menu_corner_radius();
        let region_key = (width as i32, height as i32, radius.to_bits());
        if self.applied_region != Some(region_key) {
            self.applied_region = Some(region_key);
            unsafe {
                if radius > 0.0 {
                    let d = (radius * 2.0).round() as i32;
                    let region = CreateRoundRectRgn(0, 0, width as i32, height as i32, d, d);
                    // `SetWindowRgn` prend possession de `region` — pas de
                    // `DeleteObject` ici (voir `win32::window::set_dock_region`).
                    let _ = SetWindowRgn(self.hwnd, Some(region), true);
                } else {
                    let _ = SetWindowRgn(self.hwnd, None, true);
                }
            }
        }

        // LE FOND, PEINT AVANT LES VIGNETTES — et c'est l'ORDRE qui compte : DWM
        // compose les vignettes PAR-DESSUS le contenu de cette fenêtre (mesuré,
        // voir la doc de module), donc tout ce qui doit se voir SOUS elles est
        // présenté avant qu'elles ne soient posées. Fond flouté du bureau, teinté
        // comme les menus, recapturé à chaque frame comme l'infobulle
        // (`win32::cef_popup::render`, même raison : le bureau vit derrière une
        // fenêtre qui reste affichée plusieurs secondes).
        // **LE FOND EST LE FOND D'ÉCRAN DU BUREAU, ET RIEN D'AUTRE** — DEMANDE
        // EXPLICITE : « je ne vois pas le fond d'écran du bureau ». Avant, c'était
        // le matériau des menus (capture floutée + teinte d'accent) : une vitre
        // dépoli posée sur ce qu'il y a DERRIÈRE la carte, donc pas une miniature
        // de bureau du tout. `new_textured` force `color = [1,1,1,1]` et le mode
        // Texture, donc l'image passe SANS teinte — exactement ce qui était
        // demandé. Le RAYON, lui, reste : la région arrondie est posée plus haut.
        //
        // LE REPLI est un aplat sombre, choisi par l'utilisateur : aucun fond
        // lisible (bureau sans fond propre, fichier disparu, format indécodable)
        // donne un fond uni — jamais une carte vide, qu'on lirait comme un bug.
        let background = if self.wallpaper.is_some() {
            QuadInstance::new_textured([0.0, 0.0, box_w, box_h], radius, 1.0)
        } else {
            QuadInstance::new([0.0, 0.0, box_w, box_h], FALLBACK_BACKGROUND, radius, 1.0)
        };
        let background_texture = self.wallpaper.as_ref().map(|(_, texture)| &texture.srv);
        let batches: Vec<QuadBatch> = vec![QuadBatch::new(background_texture, std::slice::from_ref(&background))];
        self.renderer.render(
            &gfx.context,
            swapchain.rtv().expect("rtv toujours présente après resize/new"),
            box_w,
            box_h,
            &batches,
        )?;
        swapchain.present()?;

        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        // Voir `TooltipWindow::show`, même piège : `show` est appelé à CHAQUE
        // frame tant que l'aperçu est dû, donc repartir de zéro à chaque appel
        // le ferait clignoter indéfiniment. On ne repart de zéro que si la
        // fenêtre était CACHÉE ; sinon on se contente de viser la pleine
        // opacité, ce qui laisse un aperçu en train de s'éteindre se rallumer
        // doucement.
        if !self.shown {
            self.fade.restart();
        }
        self.fade.fade_in();
        // L'OPACITÉ EST POSÉE DANS LA FOULÉE : la fenêtre vient d'être montrée,
        // et une fenêtre `LAYERED` neuve porte l'alpha OPAQUE par défaut — sans
        // ce `push`, l'aperçu se composerait entier pendant la frame qui précède
        // le premier pas de son fondu, donc un éclair au lieu d'une apparition.
        self.fade.push(self.hwnd);
        self.shown = true;

        // LES VIGNETTES, EN DERNIER (voir le commentaire du fond ci-dessus).
        // La taille CLIENT leur est donnée : c'est le cadre dans lequel chacune
        // doit être ramenée (voir `thumbnail_destination`).
        self.sync_thumbnails(windows, scale, [mon_x, mon_y], [width as i32, height as i32]);
        Ok(())
    }

    /// Réaffirme le rang de l'aperçu SANS le déplacer, le redimensionner ni
    /// l'activer — à appeler APRÈS tout `Window::raise_to_top` du dock.
    ///
    /// **POURQUOI ELLE EXISTE, alors que `show` pose déjà ce rang** : c'est
    /// exactement le défaut qui a fait ajouter `TooltipWindow::reassert_on_top`,
    /// et il se reproduit ici par construction — l'aperçu est déclenché par le
    /// survol d'une PASTILLE, donc d'un élément qui se MAGNIFIE, donc dans le
    /// seul cas où `app::run` fait remonter le dock en fin de frame (voir
    /// `last_applied_overflow`). Sans ce rappel, la boîte agrandie de la
    /// pastille repasserait devant le haut de l'aperçu.
    pub fn reassert_on_top(&self) {
        let insert_after = crate::win32::cef_popup::visible_popup_hwnd().or(Some(HWND_TOPMOST));
        unsafe {
            let _ = SetWindowPos(self.hwnd, insert_after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    /// L'aperçu est-il à l'écran ? — lue par `app::run` pour ne réaffirmer le
    /// rang du dock que lorsque quelque chose a pu passer devant (voir le
    /// commentaire de ce bloc : la remontée coûte ~6 ms, mesurée).
    pub fn is_visible(&self) -> bool {
        unsafe { IsWindowVisible(self.hwnd).as_bool() }
    }

    /// Engage la DISPARITION de l'aperçu — la fenêtre n'est PAS cachée ici :
    /// elle s'éteint d'abord, et c'est `tick` qui la cachera une fois l'opacité
    /// à zéro. Idempotent, et c'est nécessaire : `app::run` appelle `hide` à
    /// chaque frame où il n'y a rien à afficher.
    pub fn hide(&mut self) {
        // Les vignettes, elles, partent TOUT DE SUITE — comme les sessions de
        // capture du panneau d'aperçus (`PreviewPanel::hide`) : elles ne sont
        // plus dessinées de toute façon (la fenêtre s'en va), et **une vignette
        // vivante garde sa fenêtre source référencée**, donc la laisser traîner
        // pendant toute la disparition est un coût pour rien.
        self.release_thumbnails();
        self.fade.fade_out();
    }

    /// Avance le fondu et cache la fenêtre quand il est terminé — **À APPELER À
    /// CHAQUE FRAME**, même quand aucun aperçu n'est dû : c'est ce pas-là qui
    /// termine une disparition, donc ce qui finit par cacher la fenêtre.
    pub fn tick(&mut self) {
        self.fade.tick(self.hwnd, fade_ms());
        if self.shown && self.fade.is_hidden() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.shown = false;
        }
    }

    /// Vrai si le point (coordonnées ÉCRAN) est sur l'aperçu — c'est ce qui le
    /// garde ouvert pendant que le pointeur descend de la pastille vers lui.
    pub fn contains_screen_point(&self, x: i32, y: i32) -> bool {
        if !self.shown {
            return false;
        }
        let [px, py, pw, ph] = self.screen_rect;
        (x as f32) >= px && (x as f32) < px + pw && (y as f32) >= py && (y as f32) < py + ph
    }

    /// Enregistre les vignettes manquantes, libère celles des fenêtres qui ne
    /// sont plus là, et repose la géométrie de TOUTES.
    ///
    /// **La géométrie est reposée à chaque appel, l'enregistrement non** :
    /// `DwmRegisterThumbnail` coûte un aller-retour DWM, alors que
    /// `DwmUpdateThumbnailProperties` est la seule façon de suivre un
    /// changement — et il y en a toujours un possible ici, l'instantané des
    /// fenêtres n'étant relu qu'à 1 Hz (voir `app::run`).
    ///
    /// `client` : taille de la fenêtre d'aperçu, qui est le cadre de découpe des
    /// destinations (voir `thumbnail_destination`).
    fn sync_thumbnails(&mut self, windows: &[DesktopWindow], scale: f32, monitor: [i32; 2], client: [i32; 2]) {
        // Les fenêtres disparues de l'instantané : leur vignette n'a plus rien à
        // montrer, et DWM la garderait sinon vivante et référencée.
        self.thumbnails.retain(|(hwnd, thumbnail)| {
            if windows.iter().any(|window| window.hwnd == *hwnd) {
                return true;
            }
            let _ = unsafe { DwmUnregisterThumbnail(*thumbnail) };
            false
        });

        // **L'ORDRE EST INVERSÉ, ET C'EST LE CŒUR DU CORRECTIF DU 2026-09-20.**
        //
        // `windows` est en Z-order RÉEL, du DESSUS vers le DESSOUS — c'est l'ordre
        // d'`EnumWindows`, que `windows_by_desktop` conserve. Or **DWM compose les
        // vignettes dans leur ordre d'ENREGISTREMENT, la dernière posée par-dessus
        // les précédentes**. Enregistrer du dessus vers le dessous INVERSait donc
        // la pile : la fenêtre du premier plan se retrouvait tout en bas de la
        // miniature, et tout ce qui la recouvrait réellement passait devant elle.
        //
        // **LA DÉDUCTION NE DEMANDE AUCUNE DOCUMENTATION, ET C'EST CE QUI LA REND
        // SÛRE** : elle tient à une alternative. Si DWM posait la dernière vignette
        // SOUS les précédentes, l'ancien ordre aurait mis la fenêtre du dessus…
        // au-dessus, et une fenêtre du premier plan couvrant toute la miniature
        // serait apparue. Or une fenêtre de GIMP au rang 2, dont la destination
        // `(31,29)-(969,544)` était entièrement CONTENUE dans celles des rangs
        // suivants, restait INVISIBLE — mesuré. Donc la dernière posée est bien
        // celle du dessus, et l'ancien ordre inversait la pile.
        //
        // Conséquence la plus visible, et c'est celle qui a été signalée : dès que
        // plusieurs fenêtres sont MAXIMISÉES, elles occupent toutes la miniature
        // entière et se recouvrent exactement ; seule la plus haute de la pile se
        // voyait, et toutes celles du dessous disparaissaient.
        // **CE QUI EST RAPPORTÉ, ET POURQUOI CE N'EST PAS « CE QUI A ÉCHOUÉ »**
        //
        // Mesuré le 2026-09-20 : une frame sur plusieurs CENTAINES écarte les
        // 22 fenêtres d'un bureau d'un coup — avec un cadre pourtant parfaitement
        // sain (1000x563, échelle 0,26042, donc la géométrie n'était PAS en cause)
        // — et la frame suivante est normale. **Une seule frame transitoire suffit
        // à empoisonner un journal**, parce qu'une déduplication la fige comme si
        // c'était l'état stable. C'est mot pour mot « une sonde qui répond à
        // côté », le défaut que la mémoire du projet dit être pire qu'une question
        // bien posée.
        //
        // On ne rapporte donc que ce qui PERSISTE : sans vignette à la frame
        // courante ET à la précédente. Le transitoire est écarté sans qu'on ait eu
        // besoin de connaître sa cause — **qui reste non établie, et qu'on ne
        // devine pas** (deux déductions successives se sont déjà révélées fausses
        // ici).
        let mut missing_now: Vec<(HWND, &str, &'static str, i32)> = Vec::new();
        for window in windows.iter().rev() {
            let Some(destination) = thumbnail_destination(window.hwnd, scale, monitor, client) else {
                // ÉCARTÉE — le cas le plus fréquent de très loin : celui des
                // fenêtres RÉDUITES, dont le rectangle restauré tombe hors du
                // cadre. Écarté à raison, mais plus en silence : c'est exactement
                // la question « pourquoi cette fenêtre n'y est-elle pas ? » qu'on a
                // mis six hypothèses à se poser.
                missing_now.push((window.hwnd, &window.exe, "destination nulle (reduite, ou hors du cadre)", 0));
                continue;
            };
            let thumbnail = match self.thumbnails.iter().find(|(hwnd, _)| *hwnd == window.hwnd) {
                Some((_, thumbnail)) => *thumbnail,
                None => match unsafe { DwmRegisterThumbnail(self.hwnd, window.hwnd) } {
                    Ok(thumbnail) => {
                        self.thumbnails.push((window.hwnd, thumbnail));
                        thumbnail
                    }
                    // Refus : fenêtre détruite entre l'instantané (1 Hz) et
                    // maintenant, ou fenêtre que DWM ne sait pas composer.
                    Err(error) => {
                        missing_now.push((
                            window.hwnd,
                            &window.exe,
                            "DWM refuse l'enregistrement de la vignette",
                            error.code().0,
                        ));
                        continue;
                    }
                },
            };
            // `rcSource` LAISSÉ À ZÉRO, et `DWM_TNP_RECTSOURCE` absent de
            // `dwFlags` : c'est la sémantique de DWM pour « la fenêtre
            // ENTIÈRE ». Poser un rectangle source, même égal à la fenêtre, ne
            // dit pas la même chose.
            let properties = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY,
                rcDestination: destination,
                opacity: 255,
                fVisible: true.into(),
                ..Default::default()
            };
            // **CET ÉCHEC-LÀ NE SE TAIT PAS** — et c'est une leçon, pas une
            // coquetterie : une vignette ENREGISTRÉE dont les propriétés n'ont
            // pas pu être posées ne dessine RIEN, parce que les valeurs par
            // défaut de `DWM_THUMBNAIL_PROPERTIES` sont `fVisible: FALSE,
            // opacity: 0`. La fenêtre n'est donc pas mal placée : elle est
            // ABSENTE, sans le moindre signe. C'est exactement ce qui a rendu
            // silencieuses les fenêtres maximisées (voir `thumbnail_destination`),
            // et un `eprintln!` est ce qui aurait donné le diagnostic du premier
            // coup.
            //
            // ⚠️ **ET IL NE SUFFISAIT PAS** : mesuré le 2026-09-20, une application
            // lancée DÉTACHÉE n'a pas de console, donc cette ligne ne va nulle
            // part. C'est la seule sortie non silencieuse de ce chemin, et elle
            // restait invisible dans le cas d'usage réel. Elle est donc DOUBLÉE au
            // journal — `eprintln!` conservé pour le lancement attaché, le rapport
            // ci-dessous pour le lancement détaché, qui est la façon dont ce dock
            // démarre.
            if let Err(error) = unsafe { DwmUpdateThumbnailProperties(thumbnail, &properties) } {
                missing_now.push((
                    window.hwnd,
                    &window.exe,
                    "vignette enregistree mais proprietes non posees (donc invisible)",
                    error.code().0,
                ));
                eprintln!("Aperçu de bureau : vignette non posée ({error:?}) — fenêtre ignorée");
            }
        }

        // **UN ÉTAT OÙ TOUT MANQUE N'EST PAS LA RÉPONSE À LA QUESTION POSÉE.**
        //
        // Mesuré trois fois : au démarrage d'une carte, une à deux frames écartent
        // TOUTES les fenêtres d'un bureau d'un coup, avec un cadre, une origine et
        // une échelle tous sains (1000x563, (0,0), 0,26042 — donc ni la géométrie
        // ni le moniteur). **La cause n'est PAS établie** : trois hypothèses
        // successives ont été tuées par la mesure, et je n'en propose pas une
        // quatrième. Ce qui EST établi, c'est sa FORME : elle est **TOTALE**. Une
        // absence légitime, elle, est **PARTIELLE** — une fenêtre réduite parmi
        // vingt-cinq.
        //
        // Les deux sont donc rapportées différemment, sans que rien soit caché :
        // « tout » donne UNE ligne qui dit ce qu'elle dit, « quelques-unes » donne
        // le détail par fenêtre — la vraie réponse à « pourquoi celle-là n'y
        // est-elle pas ? ».
        if !windows.is_empty() && missing_now.len() == windows.len() {
            // Une seule ligne pour un état TOTAL : il n'y a aucune fenêtre
            // particulière à désigner, donc rien à détailler.
            if !REPORTED_EMPTY_MAP.with(|reported| reported.replace(true)) {
                crate::diag!(
                    "carte : AUCUNE vignette posee cette frame ({} fenetres) [cadre {}x{} origine ({},{}) echelle {scale:.5}]",
                    windows.len(),
                    client[0],
                    client[1],
                    monitor[0],
                    monitor[1]
                );
            }
        } else {
            // RAPPORT DES SEULES ABSENCES PERSISTANTES — voir le commentaire en
            // tête de fonction, qui porte la mesure justifiant « deux frames de
            // suite ».
            for (hwnd, exe, reason, code) in missing_now
                .iter()
                .filter(|(hwnd, _, _, _)| MISSING_LAST.with(|last| last.borrow().contains(&(hwnd.0 as isize))))
            {
                if first_missing_report(*hwnd) {
                    // **LES BORNES SONT LE DERNIER CHIFFRE INCONNU.** Les trois
                    // autres entrées du calcul sont mesurées et saines — cadre,
                    // origine, échelle — donc le `None` vient de l'une des deux
                    // dernières portes : `visible_bounds`, ou `clip_rect` sur ce
                    // qu'elle rend. Ce champ les départage, et c'est une MESURE de
                    // plus, pas une hypothèse de plus (trois ont déjà été tuées).
                    let bornes = match visible_bounds(*hwnd) {
                        Some(b) => format!("({},{})-({},{})", b[0], b[1], b[2], b[3]),
                        None => String::from("NONE"),
                    };
                    crate::diag!(
                        "carte : fenetre {:?} ({exe}) SANS VIGNETTE — {reason} [code={code:#010x}] [cadre {}x{} origine ({},{}) echelle {scale:.5}] [bornes {bornes}]",
                        hwnd.0,
                        client[0],
                        client[1],
                        monitor[0],
                        monitor[1]
                    );
                }
            }
        }
        MISSING_LAST.with(|last| {
            let mut last = last.borrow_mut();
            last.clear();
            last.extend(missing_now.iter().map(|(hwnd, _, _, _)| hwnd.0 as isize));
        });
    }

    /// Libère TOUTES les vignettes — libération groupée (changement de bureau)
    /// comme terminale (`hide`, `Drop`). Voir le champ `thumbnails` pour ce
    /// qu'une vignette oubliée coûte.
    fn release_thumbnails(&mut self) {
        for (_, thumbnail) in self.thumbnails.drain(..) {
            let _ = unsafe { DwmUnregisterThumbnail(thumbnail) };
        }
        // Plus aucune vignette ne décrit un bureau : l'état doit le dire, sinon
        // un `show` sur le MÊME bureau croirait les vignettes encore bonnes.
        self.desktop = None;
        // Les fenêtres déjà signalées le sont pour CETTE carte : la suivante
        // repart de zéro et les re-signalera, ce qui est le comportement voulu —
        // une carte qu'on rouvre est une nouvelle question posée.
        REPORTED_MISSING.with(|reported| reported.borrow_mut().clear());
        MISSING_LAST.with(|last| last.borrow_mut().clear());
        REPORTED_EMPTY_MAP.with(|reported| reported.set(false));
    }
}

/// **LE `Drop` EST LA DERNIÈRE LIGNE, PAS LA PREMIÈRE** : `hide` libère déjà les
/// vignettes en usage normal, mais une fermeture de l'application — ou un `?`
/// qui remonte depuis `show` — ferait sortir la fenêtre de la boucle sans
/// repasser par lui, et DWM garderait chaque fenêtre source référencée jusqu'à
/// la fin du processus. Même discipline que `win32::cef_popup::release_all`.
impl Drop for DesktopPreviewWindow {
    fn drop(&mut self) {
        self.release_thumbnails();
    }
}

/// Où poser la miniature ENTIÈRE, et à quelle échelle — le calcul PUR de `show`
/// (le moniteur lui est donné, aucun appel Win32), donc testable sans fenêtre.
///
/// Rend `(échelle, [x, y, largeur, hauteur])`. L'échelle est le rapport
/// `width` / largeur du moniteur ; la boîte est centrée sur la pastille puis
/// RAMENÉE DANS LE MONITEUR — sans quoi une pastille des extrémités pousserait
/// l'aperçu hors de l'écran.
///
/// **`width` EST UN PARAMÈTRE, PAS UNE LECTURE DE CELLULE** : la fonction reste
/// pure et testable sans fenêtre, et c'est l'appelant qui porte la valeur — le
/// style du dépôt. `show` lui passe `preview_width()`.
#[cfg(test)]
fn preview_box(dot_rect: [i32; 4], monitor: [i32; 4], width: f32) -> (f32, [f32; 4]) {
    preview_box_entre(dot_rect, monitor, width, [monitor[0], monitor[0] + monitor[2]])
}

/// `preview_box`, ramenée ENTRE `libre` (`[gauche, droite]`, pixels écran) et
/// non dans tout le moniteur — ⛔ DEMANDE EXPLICITE (2026-10-15) : l'aperçu ne
/// recouvre JAMAIS une barre latérale. `show` y passe l'espace entre les barres
/// de gauche et de droite du dock.
///
/// Plus large que cet espace, il part du bord de la barre de GAUCHE et déborde
/// à droite (même règle que `clamp_within` : la taille demandée, sans rognage).
fn preview_box_entre(dot_rect: [i32; 4], monitor: [i32; 4], width: f32, libre: [i32; 2]) -> (f32, [f32; 4]) {
    let [dx, dy, dw, dh] = dot_rect;
    let [mon_x, mon_y, mon_w, mon_h] = monitor;
    let gauche = libre[0].max(mon_x);
    let droite = libre[1].min(mon_x + mon_w).max(gauche);
    // `max(1)` : `monitor_bounds` a un repli qui ne devrait jamais rendre zéro,
    // mais une division par zéro donnerait une échelle infinie — donc une
    // fenêtre de taille absurde plutôt qu'un échec lisible.
    let scale = width / mon_w.max(1) as f32;
    let box_w = (mon_w as f32 * scale).round().max(1.0);
    let box_h = (mon_h as f32 * scale).round().max(1.0);
    let box_x = clamp_within((dx + dw / 2) as f32 - box_w * 0.5, gauche as f32, (droite - gauche) as f32, box_w);
    let box_y = clamp_within((dy + dh) as f32 + GAP, mon_y as f32, mon_h as f32, box_h);
    (scale, [box_x, box_y, box_w, box_h])
}

/// Ramène `value` dans `[start, start + span − size]`.
///
/// **Écrit à la main parce que `f32::clamp` PANIQUE quand la borne basse dépasse
/// la haute** — ce qui arrive exactement quand `size` (la miniature) est plus
/// grand que `span` (l'écran) : un moniteur étroit, ou un moniteur rendu
/// absurde par un repli. Dans ce cas la valeur retombe au bord `start`, et la
/// fenêtre déborde de l'autre côté : c'est laid, mais c'est un aperçu, et ça ne
/// fait pas tomber l'application.
fn clamp_within(value: f32, start: f32, span: f32, size: f32) -> f32 {
    let high = (start + span - size).max(start);
    value.clamp(start, high)
}

// Fenêtres dont on a DÉJÀ dit, pour la session d'aperçu en cours, qu'elles
// n'avaient pas de vignette — et pourquoi. Remis à zéro par
// `release_thumbnails`, qui marque la fin d'une carte (fermeture, ou changement
// de bureau).
//
// **INDISPENSABLE, ET POUR UNE RAISON DE VOLUME** : `sync_thumbnails` est
// rappelé à CHAQUE FRAME tant que la carte est ouverte — `show` n'a aucune garde
// « déjà ouverte », il repose la géométrie à chaque passage (mesuré : 248 appels
// sur une seule ouverture). Journaliser l'état ferait donc écrire la même ligne
// une soixantaine de fois par seconde. On ne journalise que le CHANGEMENT.
thread_local! {
    static REPORTED_MISSING: std::cell::RefCell<Vec<isize>> = const { std::cell::RefCell::new(Vec::new()) };
    // Fenêtres sans vignette à la frame PRÉCÉDENTE de cette carte — c'est la
    // comparaison avec cette liste qui distingue une absence PERSISTANTE d'un
    // transitoire d'une frame (voir le commentaire en tête de `sync_thumbnails`,
    // qui porte la mesure).
    static MISSING_LAST: std::cell::RefCell<Vec<isize>> = const { std::cell::RefCell::new(Vec::new()) };
    // Vrai dès qu'une frame « aucune vignette posée » a été signalée pour cette
    // carte — voir le rapport, en fin de `sync_thumbnails`.
    static REPORTED_EMPTY_MAP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Vrai si c'est la PREMIÈRE fois de la session d'aperçu qu'on signale cette
/// fenêtre — l'appelant ne construit son message que dans ce cas, ce qui évite
/// toute allocation quand le journal est inactif.
///
/// **LE SILENCE QUE CES DEUX LIGNES RÉPARENT A COÛTÉ CHER.** Une vignette écartée
/// ne laissait aucune trace, donc « pourquoi cette fenêtre n'est-elle pas dans la
/// carte ? » n'avait pas de réponse : il a fallu six hypothèses mesurées de
/// l'extérieur — puis la découverte que la vraie faute était AILLEURS
/// (l'ordre de composition) — avant d'instrumenter une sortie qui aurait dû
/// parler depuis le début. C'est la même leçon que le `eprintln!` des
/// propriétés, un peu plus bas.
fn first_missing_report(hwnd: HWND) -> bool {
    if !crate::diag::enabled() {
        return false;
    }
    REPORTED_MISSING.with(|reported| {
        let mut reported = reported.borrow_mut();
        if reported.contains(&(hwnd.0 as isize)) {
            return false;
        }
        reported.push(hwnd.0 as isize);
        true
    })
}

/// Où poser la vignette de `hwnd`, en coordonnées CLIENT de la fenêtre d'aperçu
/// — `None` si cette fenêtre ne doit pas y figurer, ou si sa place tombe
/// entièrement hors du cadre.
///
/// `scale` : largeur de la miniature / largeur du moniteur. `monitor` : origine
/// du moniteur, en coordonnées écran. `client` : taille de la fenêtre d'aperçu.
fn thumbnail_destination(hwnd: HWND, scale: f32, monitor: [i32; 2], client: [i32; 2]) -> Option<RECT> {
    // **PAS DE FILTRE « RÉDUITE » ICI, ET C'EST UNE CORRECTION** : `IsIconic` a
    // d'abord été mis là, en supposant qu'une fenêtre réduite n'a plus de
    // contenu. C'est faux — DWM en garde le plus souvent la dernière image, et
    // c'est même ce que Task View affiche. Une fenêtre réduite garde d'ailleurs
    // son rectangle restauré, donc sa vignette se pose à la place qu'elle
    // occupera en revenant : rien à écarter. Si DWM n'a vraiment plus rien à
    // dessiner, il ne dessine rien, et ce n'est pas une raison de l'ôter.
    //
    // NOTRE PROPRE PROCESSUS, ET C'EST VITAL : sans ce filtre l'aperçu se montre
    // LUI-MÊME (ainsi que le dock et nos popups), donc la vignette contient la
    // fenêtre qui la porte — la récursion n'est pas une hypothèse, c'est la
    // conséquence directe de la composition. Le filtre porte sur le PID et non
    // sur cette seule fenêtre : TOUTES nos fenêtres sont à écarter.
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == std::process::id() {
        return None;
    }
    let bounds = visible_bounds(hwnd)?;
    // Taille nulle : fenêtres en cours de création ou de destruction. Une
    // vignette de largeur nulle n'est pas fausse, elle est inutile.
    if bounds[2] <= bounds[0] || bounds[3] <= bounds[1] {
        return None;
    }
    let scaled = scaled_rect(bounds, scale, monitor);
    // **RAMENÉE DANS LE CADRE, PAR FENÊTRE** — le point qui a coûté le plus
    // cher : un `rcDestination` qui sort de la fenêtre de destination n'est pas
    // rogné par DWM, il est REFUSÉ, et une vignette refusée ne dessine rien du
    // tout (voir `sync_thumbnails`). Or déborder est la norme, pas l'exception :
    // une fenêtre MAXIMISÉE dépasse le moniteur de quelques pixels de chaque
    // côté (ses bords invisibles de redimensionnement), et une fenêtre à cheval
    // sur deux écrans déborde forcément de la miniature. L'intersection vide,
    // elle, se traite en amont : on n'enregistre pas de vignette du tout.
    let [left, top, right, bottom] = clip_rect(scaled, client)?;
    Some(RECT { left, top, right, bottom })
}

/// Les bornes VISIBLES de `hwnd`, en coordonnées écran — `None` si la fenêtre
/// n'en a plus.
///
/// **`DWMWA_EXTENDED_FRAME_BOUNDS` ET NON `GetWindowRect`, ET C'EST LE CŒUR DU
/// CORRECTIF** : `GetWindowRect` rend le rectangle de la fenêtre *telle que
/// Windows la gère*, bords invisibles de redimensionnement compris (une dizaine
/// de pixels de chaque côté depuis Vista, et davantage quand DWM compose). Une
/// fenêtre MAXIMISÉE ne s'arrête donc pas au bord de l'écran : son rectangle en
/// SORT. C'est ce qui rendait invisibles WhatsApp ou GIMP maximisés — leur
/// vignette, calculée sur ce rectangle débordant, sortait du cadre de l'aperçu à
/// tous les coups (voir `thumbnail_destination`). L'attribut DWM, lui, rend ce
/// qui se VOIT, ombre portée et bords invisibles exclus : c'est exactement ce
/// qu'une miniature doit montrer, et la raison pour laquelle il existe.
///
/// **Repli sur `GetWindowRect`** : l'attribut peut échouer (fenêtre détruite
/// entre-temps, ou cas que DWM ne connaît pas — une fenêtre sans composition).
/// Un rectangle un peu trop grand vaut alors mieux que pas de vignette, et
/// `clip_rect` le ramènera de toute façon dans le cadre.
fn visible_bounds(hwnd: HWND) -> Option<[i32; 4]> {
    let mut rect = RECT::default();
    let extended = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut rect).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
    };
    if extended.is_err() {
        rect = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
            return None;
        }
    }
    Some([rect.left, rect.top, rect.right, rect.bottom])
}

/// Ramène `rect` dans `[0, 0, largeur, hauteur]` — `None` si l'intersection est
/// VIDE. Le calcul PUR de `thumbnail_destination` (aucun appel Win32), donc
/// testable sans fenêtre.
fn clip_rect(rect: [i32; 4], client: [i32; 2]) -> Option<[i32; 4]> {
    let [left, top, right, bottom] = rect;
    let clipped = [left.max(0), top.max(0), right.min(client[0]), bottom.min(client[1])];
    if clipped[2] <= clipped[0] || clipped[3] <= clipped[1] {
        return None;
    }
    Some(clipped)
}

/// Le rectangle d'UNE fenêtre dans la miniature, en coordonnées client de la
/// fenêtre d'aperçu — le calcul PUR de `thumbnail_destination` (aucun appel
/// Win32), donc testable sans fenêtre.
///
/// **IL N'Y A PAS DE TERME D'ORIGINE DE FENÊTRE, ET C'EST LE POINT** : la
/// miniature du moniteur occupe TOUTE la fenêtre d'aperçu, à partir de son coin
/// client (0, 0). Une fenêtre est donc à `(sa position − origine du moniteur) ×
/// échelle` — point. Y retrancher la position ÉCRAN de l'aperçu serait une
/// erreur invisible au centre de l'écran (où les deux coïncident presque) et
/// grossière sur les bords : les vignettes sortiraient de la fenêtre dès que la
/// boîte est ramenée dans l'écran par `clamp_within`.
fn scaled_rect(rect: [i32; 4], scale: f32, monitor: [i32; 2]) -> [i32; 4] {
    let [left, top, right, bottom] = rect;
    let x = (left - monitor[0]) as f32 * scale;
    let y = (top - monitor[1]) as f32 * scale;
    let w = (right - left) as f32 * scale;
    let h = (bottom - top) as f32 * scale;
    [x.round() as i32, y.round() as i32, (x + w).round() as i32, (y + h).round() as i32]
}

/// Procédure de fenêtre de l'aperçu — un simple relais vers `DefWindowProcW`
/// (voir `win32::tooltip::tooltip_wndproc`, même principe) : `WS_EX_TRANSPARENT`
/// fait que cette fenêtre ne reçoit de toute façon jamais d'entrée réelle.
unsafe extern "system" fn desktop_preview_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Moniteur 1920×1080 à l'origine : le cas courant, et celui sur lequel les
    /// valeurs attendues se calculent de tête.
    const HD: [i32; 4] = [0, 0, 1920, 1080];

    #[test]
    fn the_preview_is_the_whole_monitor_at_scale() {
        // Une pastille au milieu de la barre du haut.
        let (scale, miniature) = preview_box([900, 0, 20, 20], HD, DEFAULT_PREVIEW_WIDTH);
        assert!((scale - 520.0 / 1920.0).abs() < 1e-6, "l'échelle est le rapport des largeurs : {scale}");
        // La hauteur DÉCOULE de l'échelle (520 × 1080/1920 = 292,5 → 293) :
        // c'est elle qui garantit que le moniteur entier tient dans l'aperçu,
        // donc qu'aucune fenêtre n'est recadrée.
        assert_eq!(miniature[2], 520.0);
        assert_eq!(miniature[3], 293.0);
        // Centrée sur la pastille, et sous elle (GAP).
        assert_eq!(miniature[0], 900.0 + 10.0 - 260.0);
        assert_eq!(miniature[1], 20.0 + GAP);
    }

    /// ⛔ **L'APERÇU NE RECOUVRE JAMAIS UNE BARRE LATÉRALE** — défaut SIGNALÉ
    /// (2026-10-15) : « les fenêtres de prévisualisation de l'applet pour
    /// changer de bureau s'affichent sur la barre de gauche ». Les pastilles sont
    /// tout près du bord gauche : centré sous elles, l'aperçu n'était ramené que
    /// dans l'ÉCRAN, donc par-dessus la barre de gauche.
    #[test]
    fn l_apercu_reste_entre_les_barres_laterales() {
        // Barres de 140 px à gauche et à droite d'un écran 1920.
        let libre = [140, 1920 - 140];
        let (_, gauche) = preview_box_entre([200, 0, 20, 20], HD, DEFAULT_PREVIEW_WIDTH, libre);
        assert_eq!(gauche[0], 140.0, "contre la barre de gauche, pas dessus");
        let (_, droite) = preview_box_entre([1850, 0, 20, 20], HD, DEFAULT_PREVIEW_WIDTH, libre);
        assert_eq!(droite[0] + droite[2], 1780.0, "contre la barre de droite, pas dessus");
        // Au milieu, rien ne change : centré sous la pastille.
        let (_, milieu) = preview_box_entre([900, 0, 20, 20], HD, DEFAULT_PREVIEW_WIDTH, libre);
        assert_eq!(milieu[0], 900.0 + 10.0 - 260.0);
        // Plus large que l'espace entre les barres : il part du bord de la barre
        // de GAUCHE et déborde à droite — jamais sur la gauche.
        let (_, large) = preview_box_entre([200, 0, 20, 20], HD, MAX_PREVIEW_WIDTH, libre);
        assert_eq!(large[0], 140.0);
        // Un écran SECONDAIRE à gauche : ses bornes sont les siennes.
        let (_, secondaire) = preview_box_entre([-1900, 0, 20, 20], [-1920, 0, 1920, 1080], DEFAULT_PREVIEW_WIDTH, [-1920 + 100, -100]);
        assert_eq!(secondaire[0], -1820.0);
    }

    #[test]
    fn a_dot_near_the_edge_keeps_the_preview_on_screen() {
        // Pastille de l'extrémité droite : centrée, l'aperçu sortirait de
        // l'écran de 250 px — le clamp le ramène, et c'est la SEULE chose que
        // ce clamp fait (il ne change jamais la taille, donc jamais le
        // placement des fenêtres dedans).
        let (_, miniature) = preview_box([1900, 0, 20, 20], HD, DEFAULT_PREVIEW_WIDTH);
        assert_eq!(miniature[0], 1920.0 - 520.0);
        assert_eq!(miniature[0] + miniature[2], 1920.0);
    }

    #[test]
    fn a_window_keeps_its_place_in_the_miniature() {
        // Une fenêtre de 200×100 au centre du moniteur : elle doit se retrouver
        // au centre de la miniature (260, 146), et sa taille doit être mise à
        // l'échelle (200 × 0,2708 = 54,2 → 54 ; 100 × 0,2708 = 27,1 → 27).
        let (scale, _) = preview_box([900, 0, 20, 20], HD, DEFAULT_PREVIEW_WIDTH);
        let rect = scaled_rect([860, 490, 1060, 590], scale, [0, 0]);
        assert_eq!(rect, [233, 133, 287, 160]);
        assert_eq!(rect[2] - rect[0], 54);
        assert_eq!(rect[3] - rect[1], 27);
    }

    #[test]
    fn a_secondary_monitor_is_measured_from_its_own_corner() {
        // Écran secondaire à GAUCHE du principal : ses coordonnées écran sont
        // négatives. Une fenêtre à son coin supérieur gauche doit tomber au
        // coin (0, 0) de la miniature — c'est le décalage par l'origine du
        // moniteur, et non par 0, qui le garantit.
        let (scale, _) = preview_box([-960, 0, 20, 20], [-1920, 0, 1920, 1080], DEFAULT_PREVIEW_WIDTH);
        let rect = scaled_rect([-1920, 0, -1720, 200], scale, [-1920, 0]);
        assert_eq!(rect, [0, 0, 54, 54]);
    }

    #[test]
    fn a_monitor_narrower_than_the_preview_does_not_panic() {
        // Le cas qui ferait PANIQUER `f32::clamp` (borne basse au-dessus de la
        // haute) : la miniature est plus large que l'écran. On retombe au bord,
        // et on déborde — jamais on ne tombe.
        let (_, miniature) = preview_box([0, 0, 10, 10], [0, 0, 320, 240], DEFAULT_PREVIEW_WIDTH);
        assert_eq!(miniature[0], 0.0);
        assert_eq!(miniature[1], 0.0);
    }

    /// Taille de la fenêtre d'aperçu pour un moniteur HD — le cadre dans lequel
    /// chaque destination est ramenée.
    const CLIENT: [i32; 2] = [520, 293];

    #[test]
    fn a_maximized_window_fills_the_preview_instead_of_disappearing() {
        // LE DÉFAUT QUI A COÛTÉ CE CORRECTIF, reproduit en chiffres : les bornes
        // VISIBLES d'une fenêtre maximisée (celles que rend
        // `DWMWA_EXTENDED_FRAME_BOUNDS`) dépassent le moniteur de 8 px de chaque
        // côté. Sans découpe, la destination sortait du cadre de l'aperçu et DWM
        // la REFUSAIT — d'où une fenêtre maximisée totalement absente.
        let (scale, miniature) = preview_box([900, 0, 20, 20], HD, DEFAULT_PREVIEW_WIDTH);
        let scaled = scaled_rect([-8, -8, 1928, 1088], scale, [0, 0]);
        // Elle déborde bel et bien de la miniature, dans les quatre directions.
        assert!(scaled[0] < 0 && scaled[1] < 0);
        assert!(scaled[2] > miniature[2] as i32 && scaled[3] > miniature[3] as i32);
        // Et la voici ramenée : la miniature ENTIÈRE, ce que montre Windows.
        assert_eq!(clip_rect(scaled, CLIENT), Some([0, 0, 520, 293]));
    }

    #[test]
    fn a_window_hanging_off_the_edge_keeps_its_visible_part() {
        // Une fenêtre à cheval sur le bord droit du moniteur : on garde ce qui
        // tombe DANS la miniature, et rien de plus (le reste n'existe pas).
        let (scale, _) = preview_box([900, 0, 20, 20], HD, DEFAULT_PREVIEW_WIDTH);
        let scaled = scaled_rect([1800, 100, 2200, 300], scale, [0, 0]);
        assert_eq!(clip_rect(scaled, CLIENT), Some([488, 27, 520, 81]));
    }

    #[test]
    fn a_window_that_misses_the_preview_entirely_is_refused() {
        // Fenêtre d'un AUTRE écran (aucune portion dans la miniature) : `None`,
        // donc pas de vignette enregistrée du tout — l'enregistrer puis la
        // laisser invisible serait le même gaspillage qu'un rectangle refusé.
        let (scale, _) = preview_box([900, 0, 20, 20], HD, DEFAULT_PREVIEW_WIDTH);
        let scaled = scaled_rect([3000, 200, 3200, 400], scale, [0, 0]);
        assert_eq!(clip_rect(scaled, CLIENT), None);
        // Et le cas limite : une intersection d'un seul pixel de large est
        // toujours une vignette valable (DWM ne refuse que le vide).
        assert_eq!(clip_rect([519, 10, 900, 200], CLIENT), Some([519, 10, 520, 200]));
        assert_eq!(clip_rect([520, 10, 900, 200], CLIENT), None);
    }

    // ---- RÉGLAGES : largeur, fondu, délai de survol ------------------------

    /// Remet les trois réglages à leur défaut. **Indispensable** : ce sont des
    /// `thread_local`, donc ils survivent d'un test à l'autre sur le même
    /// thread — un test qui déplace une valeur la déplacerait pour le suivant.
    fn reset_desktop_preview_settings() {
        set_preview_width(DEFAULT_PREVIEW_WIDTH);
        set_fade_ms(DEFAULT_FADE_MS);
        set_hover_delay_ms(DEFAULT_HOVER_DELAY_MS);
    }

    #[test]
    fn the_miniature_follows_the_width_it_is_given() {
        // La hauteur DÉCOULE du rapport du moniteur, quelle que soit la largeur :
        // c'est la propriété qui garantit qu'aucune fenêtre n'est recadrée, et
        // donc tout l'intérêt d'un réglage de largeur SEUL (voir le doc de
        // `DEFAULT_PREVIEW_WIDTH`).
        let (scale, miniature) = preview_box([900, 0, 20, 20], HD, 1040.0);
        assert!((scale - 1040.0 / 1920.0).abs() < 1e-6, "l'échelle suit la largeur : {scale}");
        assert_eq!(miniature[2], 1040.0);
        assert_eq!(miniature[3], 585.0); // 1040 × 1080/1920
        // Les deux bornes du réglage passent par la même formule.
        let (_, petit) = preview_box([900, 0, 20, 20], HD, MIN_PREVIEW_WIDTH);
        assert_eq!(petit[2], 240.0);
        assert_eq!(petit[3], 135.0); // 240 × 1080/1920
        let (_, grand) = preview_box([900, 0, 20, 20], HD, MAX_PREVIEW_WIDTH);
        assert_eq!(grand[2], MAX_PREVIEW_WIDTH);
    }

    #[test]
    fn the_default_setting_keeps_the_preview_on_screen() {
        // Le DÉFAUT doit tenir entièrement sur un petit moniteur — c'est lui qui
        // garantit qu'une installation neuve ne montre pas une carte rognée.
        let small = [0, 0, 1280, 720];
        let (_, miniature) = preview_box([600, 0, 20, 20], small, DEFAULT_PREVIEW_WIDTH);
        assert_eq!(miniature[2], 520.0);
        assert_eq!(miniature[3], 293.0); // 520 × 720/1280 = 292,5 → 293
        assert!(miniature[0] >= 0.0 && miniature[0] + miniature[2] <= 1280.0);
        assert!(miniature[1] >= 0.0 && miniature[1] + miniature[3] <= 720.0);
    }

    #[test]
    fn the_widest_setting_overflows_without_panicking() {
        // **LE PLAFOND N'EST PLUS CHOISI POUR TENIR À L'ÉCRAN** (il a été triplé
        // sur demande explicite, voir `MAX_PREVIEW_WIDTH`) : sur un moniteur plus
        // étroit que la carte, celle-ci est RAMENÉE AU BORD et déborde. Ce test
        // fige les deux propriétés qui comptent alors — elle ne panique pas (le
        // cas que `clamp_within` a été écrit pour couvrir), et sa TAILLE est
        // celle demandée, sans rognage silencieux.
        let small = [0, 0, 1280, 720];
        let (_, miniature) = preview_box([600, 0, 20, 20], small, MAX_PREVIEW_WIDTH);
        assert_eq!(miniature[2], MAX_PREVIEW_WIDTH);
        assert_eq!(miniature[3], 1125.0); // 2000 × 720/1280 = 1125
        assert_eq!(miniature[0], 0.0, "ramenée au bord, pas centrée");
        assert_eq!(miniature[1], 0.0);
    }

    #[test]
    fn desktop_preview_settings_clamp_to_their_bounds() {
        reset_desktop_preview_settings();
        set_preview_width(1.0e6);
        assert_eq!(preview_width(), MAX_PREVIEW_WIDTH);
        set_preview_width(-10.0);
        assert_eq!(preview_width(), MIN_PREVIEW_WIDTH);
        set_fade_ms(1.0e6);
        assert_eq!(fade_ms(), MAX_FADE_MS);
        set_fade_ms(-1.0);
        assert_eq!(fade_ms(), MIN_FADE_MS);
        set_hover_delay_ms(1.0e6);
        assert_eq!(hover_delay_ms(), MAX_HOVER_DELAY_MS);
        set_hover_delay_ms(-1.0);
        assert_eq!(hover_delay_ms(), MIN_HOVER_DELAY_MS);
        // Le délai est CONSOMMÉ sous forme de `Duration`, jamais de `f32` : c'est
        // la conversion qui doit rester sûre (`from_millis`, pas `from_secs_f32`
        // qui panique sur `NaN`).
        set_hover_delay_ms(250.0);
        assert_eq!(hover_delay(), std::time::Duration::from_millis(250));
        reset_desktop_preview_settings();
    }

    #[test]
    fn desktop_preview_settings_ignore_a_nan_value() {
        // `f32::clamp` PROPAGE `NaN` au lieu de le borner, et `"nan".parse::<f32>()`
        // réussit : sans le garde des accesseurs, un fichier de configuration
        // portant `nan` donnerait une échelle `NaN` et une fenêtre de taille
        // nulle. On vérifie que la valeur PRÉCÉDENTE SURVIT — et non qu'on
        // retombe sur zéro, ce qui serait un autre défaut.
        reset_desktop_preview_settings();
        set_preview_width(777.0);
        set_preview_width(f32::NAN);
        assert_eq!(preview_width(), 777.0, "NaN doit être IGNORÉ, pas appliqué ni remis à zéro");
        set_fade_ms(321.0);
        set_fade_ms(f32::NAN);
        assert_eq!(fade_ms(), 321.0);
        set_hover_delay_ms(123.0);
        set_hover_delay_ms(f32::NAN);
        assert_eq!(hover_delay_ms(), 123.0);
        // L'INFINI, lui, doit continuer de SATURER : le garde ne doit pas avoir
        // cassé le bornage.
        set_preview_width(f32::INFINITY);
        assert_eq!(preview_width(), MAX_PREVIEW_WIDTH);
        reset_desktop_preview_settings();
    }

    #[test]
    fn desktop_preview_bounds_are_the_single_authority() {
        // LE test qui rend la page des Réglages sûre : elle reçoit ces trois
        // paires par l'état poussé et n'en recopie AUCUNE. Si une borne bouge
        // ici, la page suit sans être touchée — et si quelqu'un remet un
        // littéral dans `settings.html`, c'est ce test qui dit laquelle des deux
        // valeurs fait foi. Même rôle que
        // `ui::dock::bar_thickness_bounds_are_the_single_authority`.
        assert_eq!(preview_width_bounds(), (MIN_PREVIEW_WIDTH, MAX_PREVIEW_WIDTH));
        assert_eq!(fade_bounds(), (MIN_FADE_MS, MAX_FADE_MS));
        assert_eq!(hover_delay_bounds(), (MIN_HOVER_DELAY_MS, MAX_HOVER_DELAY_MS));
        // Et chaque DÉFAUT tombe dans ses bornes — sans quoi une installation
        // neuve serait corrigée dès le premier lancement.
        let (w0, w1) = preview_width_bounds();
        assert!(w0 <= DEFAULT_PREVIEW_WIDTH && DEFAULT_PREVIEW_WIDTH <= w1);
        let (f0, f1) = fade_bounds();
        assert!(f0 <= DEFAULT_FADE_MS && DEFAULT_FADE_MS <= f1);
        let (d0, d1) = hover_delay_bounds();
        assert!(d0 <= DEFAULT_HOVER_DELAY_MS && DEFAULT_HOVER_DELAY_MS <= d1);
    }
}
