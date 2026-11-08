use crate::ui::dock::{Dock, DockLayout, bar_rects};
use crate::app::{APP_ICON_INDEX, CLIPBOARD_VIEWER_ICON_INDEX, SETTINGS_ICON_INDEX, PINNED_ICON_BASE};
use crate::app::Features;
use crate::gfx::{push_icon_batch, ClipGroups, QuadBatch, QuadInstance, QuadRenderer};
use crate::ui::dock::Side;
use crate::gfx::texture::Texture;
use crate::ui::color;
use crate::ui::topbar;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11RenderTargetView, ID3D11Texture2D,
    D3D11_TEXTURE2D_DESC, D3D11_RENDER_TARGET_VIEW_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC,
    D3D11_USAGE_DEFAULT, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_FLAG, D3D11_RESOURCE_MISC_FLAG,
    D3D11_RTV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
use windows_core::Interface;

/// Encapsulates the state needed to render a NeuraDesk dock instance.
///
/// This struct holds all the mutable state that defines a dock's appearance and
/// behavior: the dock's internal state (icons, orders, hover, etc.), the layout
/// (bar thicknesses and dimensions), feature flags, visual settings (colors,
/// tints, etc.), and provides a method to render the dock to a texture.
pub struct DockInstance {
    /// The dock's internal state: icon positions, orders, hover, drag, etc.
    pub dock: Dock,
    /// The layout of the dock: bar thicknesses and total dimensions.
    pub layout: DockLayout,
    /// Feature flags that enable/disable various dock applets.
    pub features: Features,
    /// Whether to use dark mode (affects icon fallback colors, etc.).
    pub dark_mode: bool,
    /// Tint strength for each bar (top, left, right) as opacity values (0-255).
    pub bar_tint: [u8; 4],
    /// Text color for the top bar when automatic color scheme is disabled.
    pub topbar_text_color: [u8; 4],
    /// Whether to automatically derive colors from the actual bar background.
    pub auto_color_scheme: bool,
    /// Réglages de la mini-barre d'horloge plein écran (voir `win32::
    /// fullscreen_clock`, doc de module) RÉSOLUS pour l'écran porté par
    /// cette instance — voir `app::rebuild_secondary_docks` pour la
    /// résolution PRIMAIRE vs réglage manuel de CET écran (DEMANDE
    /// EXPLICITE : "pouvoir régler individuellement les horloges en plein
    /// écran en fonction de l'écran où elles s'affichent"). Toujours
    /// écrasés explicitement par `rebuild_secondary_docks` pour les
    /// instances secondaires — les valeurs par défaut ci-dessous ne
    /// comptent que pour la barre PRIMAIRE, qui elle relit directement les
    /// accesseurs globaux de `win32::fullscreen_clock` (voir `app::run`).
    pub fullscreen_clock_enabled: bool,
    pub fullscreen_clock_position_top: bool,
    pub fullscreen_clock_bar_scale: f32,
    pub fullscreen_clock_text_scale: f32,
    /// Number of pinned applications (used to compute icon indices).
    pub pinned_apps_count: usize,
    /// Pre-loaded textures for pinned applications, indexed like the icons
    /// starting at `PINNED_ICON_BASE` (see `Icon::external_texture`, which
    /// only flags that an icon HAS an external texture — the texture data
    /// itself lives here, not on `Icon`, mirroring the previous per-icon
    /// `Vec<Option<Texture>>` in `app::run`).
    pub pinned_icon_textures: Vec<Option<Texture>>,
    /// Horloge/moniteur système/fréquence d'affichage/titre média — voir
    /// `CachedText`, doc de struct : BUG RÉEL SIGNALÉ ("les textes et
    /// icônes de la barre supérieure des écrans secondaires ne suivent pas
    /// la taille de la barre"). Avant ce champ, `render` recevait ces
    /// quatre textes déjà rendus en bitmap par `app::run` À LA TAILLE DE LA
    /// BARRE PRIMAIRE (voir `DockTextures::clock` et consorts, retirés) et
    /// les réutilisait tels quels pour CHAQUE écran secondaire — leur
    /// taille en pixels ne suivait donc QUE l'épaisseur de la barre
    /// primaire, jamais celle, potentiellement différente, de la barre
    /// secondaire sur laquelle ils étaient positionnés. Chaque instance
    /// régénère maintenant SA PROPRE bitmap, à SA PROPRE taille de police
    /// (voir `sync_text`, `ui::topbar::clock_font_px` et consorts, qui
    /// suivent déjà `layout.top_thickness` — seulement jamais appelés avec
    /// le layout SECONDAIRE jusqu'ici).
    clock: CachedText,
    sys_monitor: CachedText,
    refresh_rate: CachedText,
    /// Verr. num / Verr. maj (voir `ui::verrous`), à la police de CETTE barre.
    verrous: crate::ui::verrous::Textes,
    /// Le débit réseau (voir `ui::debit`), à la police de CETTE barre.
    debit: crate::ui::debit::Textes,
    media_title: CachedText,
    /// Numéros des pastilles de bureaux virtuels de CET écran (voir
    /// `ui::desktop_labels`) — un `CachedText` par bureau, à la taille de
    /// pastille de cet écran (`Dock::desktop_dot_size`, réglable par écran).
    /// Même raison que les quatre ci-dessus : une bitmap par frame et par
    /// pastille ruinerait la boucle à 60 im/s.
    desktop_labels: crate::ui::desktop_labels::DesktopDotLabels,
}

/// Bitmap de texte régénérée paresseusement — ne redemande un rendu GDI/D3D
/// (coûteux) QUE quand le texte affiché OU la taille de police demandée
/// change réellement, exactement le même garde-fou double que `app::run`
/// appliquait déjà en local (voir sa doc sur `clock_font_px`, "la police de
/// l'horloge ne s'adapte pas en temps réel" — un texte inchangé glissé sur
/// le curseur d'épaisseur ne régénérerait sinon la bitmap qu'à la prochaine
/// minute). `font_px: -1` initial (valeur impossible en sortie de
/// `topbar::clock_font_px`, toujours positive) force la toute première
/// génération.
/// `pub(crate)` avec `sync`/`texture` : `ui::desktop_labels::DesktopDotLabels`
/// réutilise ce cache pour les numéros des pastilles de bureaux virtuels —
/// même garde-fou double, même raison (voir sa doc de module). Le motif était
/// déjà recopié deux fois dans `app.rs` ; il ne l'est pas une troisième.
pub(crate) struct CachedText {
    text: String,
    font_px: i32,
    texture: Option<(Texture, f32, f32)>,
}

impl CachedText {
    pub(crate) fn new() -> Self {
        Self { text: String::new(), font_px: -1, texture: None }
    }

    /// Bitmap courante et ses dimensions — `None` tant qu'aucune n'a été
    /// générée (ou que le texte est vide, voir `sync`).
    pub(crate) fn texture(&self) -> Option<(&Texture, f32, f32)> {
        self.texture.as_ref().map(|(texture, w, h)| (texture, *w, *h))
    }

    /// `text` vide : masque simplement (pas de bitmap 0×0) — même
    /// convention que `media_title_texture` côté `app::run`. `bold` :
    /// voir `gfx::text::render_text` — l'horloge est en gras (DEMANDE
    /// EXPLICITE), pas les autres.
    pub(crate) fn sync(&mut self, device: &ID3D11Device, text: &str, font_px: i32, font_name: &str, bold: bool) -> Result<(), Box<dyn std::error::Error>> {
        if text == self.text && font_px == self.font_px {
            return Ok(());
        }
        self.text = text.to_string();
        self.font_px = font_px;
        self.texture = if text.is_empty() {
            None
        } else {
            let (texture, w, h) = crate::gfx::text::render_text(device, text, font_px, font_name, bold)?;
            Some((texture, w as f32, h as f32))
        };
        Ok(())
    }
}

impl DockInstance {
    /// Creates a new dock instance with default state.
    ///
    /// The dock is initialized with a background color and space for pinned
    /// applications (as in `Dock::new`). The layout is initialized to zero
    /// dimensions and must be set via `set_layout` before rendering.
    /// Features are loaded from the user's configuration.
    /// Visual settings are initialized to sensible defaults.
    ///
    /// `pinned_count` : le nombre d'applications épinglées du dock PRIMAIRE
    /// (`app::run`, `pinned_apps.len()`). Il n'est PAS facultatif, et c'est
    /// ce qui donne à cette instance le MÊME espace d'index que le primaire —
    /// voir le corps de la fonction.
    pub fn new(pinned_count: usize) -> Self {
        // **L'ESPACE D'INDEX D'UN DOCK SECONDAIRE DOIT ÊTRE CELUI DU PRIMAIRE.**
        //
        // `pinned_count` valait `0` (« TODO: load from config ») jusqu'au
        // 2026-09-23. Or TOUT le code qui adresse les applets de ce dock les
        // cite dans l'espace du PRIMAIRE, calculé sur `pinned_apps.len()` :
        // `app::rebuild_secondary_docks` (`apply_topbar_features`, et le
        // `set_icon_order` par défaut `power..power+5`) et `app::run`, qui
        // rappelle la même `apply_topbar_features` **à chaque frame**. Les
        // deux espaces divergeaient donc de `pinned_apps.len()`, et chaque
        // réglage par écran atteignait l'applet VOISINE.
        //
        // MESURÉ sur cette machine (`secondary_screen_features.txt` portant
        // `network` pour DISPLAY2, et le journal du primaire donnant
        // `applets : alimentation 6, … eclairage nocturne 13, pins 3..5`) :
        // cet index 9 tombait sur `theme` au lieu de `network`, l'index 7 sur
        // `voicemeeter` au lieu de `terminal`, et l'index 10 sur `night_light`
        // — le défaut rapporté, « chaque applet porte l'icône de sa voisine ».
        //
        // Le dock PRIMAIRE n'a jamais eu ce défaut : `app::run` le construit
        // avec le vrai `pinned_apps.len()`. C'est donc ici, et seulement ici,
        // que les deux espaces se rejoignent.
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], pinned_count);
        // Les épingles de la barre des tâches Windows (voir `win32::
        // taskbar_pins`) ne sont PAS répliquées sur les écrans secondaires :
        // `Dock::ensure_winpin_capacity` n'est appelée que par `app::run` pour
        // le dock primaire. Leur PLACE est pourtant RÉSERVÉE ci-dessus — c'est
        // elle qui fait tenir l'espace d'index — donc leurs `pinned_count`
        // créneaux existent bel et bien sur un écran secondaire, et
        // s'afficheraient en carrés vides sans ceci. `Icon::hidden` (voir sa
        // doc) est exactement le mécanisme voulu : la place reste dans les
        // listes d'ordre — `set_icon_order` ne réintègre jamais une icône qui
        // y figure — mais elle ne consomme aucun créneau (`ordre_visible`
        // l'écarte à la lecture).
        //
        // Posé ICI plutôt que dans `rebuild_secondary_docks` parce que c'est le
        // seul endroit que le rappel PAR FRAME ne peut pas défaire : il ne
        // touche que les index `0`, `1` et `6..13`, jamais `3..5`.
        for index in PINNED_ICON_BASE..PINNED_ICON_BASE + pinned_count {
            dock.set_icon_hidden(index, true);
        }
        let layout = DockLayout {
            total_width: 0.0,
            total_height: 0.0,
            top_thickness: 0.0, haut_colonnes: 0.0,
            left_width: 0.0,
            right_width: 0.0,
            // Un dock SECONDAIRE n'a pas de barre du bas (voir `Side::Bottom`,
            // `BarThicknesses::derive_for_screen`).
            bottom_thickness: 0.0,
        };
        let features = Features::load();
        let dark_mode = false; // TODO: detect from system settings
        let bar_tint = [128, 128, 128, 128]; // default tint (50% opacity)
        let topbar_text_color = [255, 255, 255, 255]; // white
        let auto_color_scheme = true;

        Self {
            dock,
            layout,
            features,
            dark_mode,
            bar_tint,
            topbar_text_color,
            auto_color_scheme,
            fullscreen_clock_enabled: false,
            fullscreen_clock_position_top: true,
            fullscreen_clock_bar_scale: 1.0,
            fullscreen_clock_text_scale: 1.0,
            pinned_apps_count: pinned_count,
            pinned_icon_textures: Vec::new(),
            clock: CachedText::new(),
            sys_monitor: CachedText::new(),
            refresh_rate: CachedText::new(),
            verrous: crate::ui::verrous::Textes::new(),
            debit: crate::ui::debit::Textes::new(),
            media_title: CachedText::new(),
            desktop_labels: crate::ui::desktop_labels::DesktopDotLabels::new(),
        }
    }

    /// Updates the dock's layout dimensions and bar thicknesses.
    ///
    /// This should be called whenever the dock's size or bar thicknesses change
    /// (e.g., when the window is resized or the user adjusts the thickness
    /// sliders).
    pub fn set_layout(
        &mut self,
        width: f32,
        height: f32,
        top_thickness: f32,
        left_width: f32,
        right_width: f32,
    ) {
        self.layout.total_width = width;
        self.layout.total_height = height;
        self.layout.top_thickness = top_thickness;
        // Un écran secondaire n'a pas de colonnes latérales à faire monter.
        self.layout.haut_colonnes = top_thickness;
        self.layout.left_width = left_width;
        self.layout.right_width = right_width;
    }

    /// Taille (largeur, hauteur) de la bitmap d'horloge DE CETTE INSTANCE
    /// (voir `sync_text`, `self.clock`), `None` tant qu'elle n'a pas encore
    /// été générée — exposée pour que `app::run` puisse faire le test de
    /// clic sur l'horloge d'un écran SECONDAIRE avec SA PROPRE taille de
    /// police (voir `topbar::clock_hit`/`clock_rect`, qui en dépendent),
    /// pas celle, potentiellement différente, de la barre primaire (`clock`
    /// est un champ privé du module : pas d'accès direct depuis `app.rs`).
    pub fn clock_size(&self) -> Option<(f32, f32)> {
        self.clock.texture.as_ref().map(|(_, w, h)| (*w, *h))
    }

    /// Régénère (si besoin, voir `CachedText::sync`) les bitmaps de
    /// l'horloge/du moniteur système/de la fréquence d'affichage/du titre
    /// média À LA TAILLE DE POLICE DE **CETTE** INSTANCE (`self.layout.
    /// top_thickness`, voir `ui::topbar::clock_font_px` et consorts) — voir
    /// la doc de `CachedText` pour le bug que ça corrige. À appeler une
    /// fois par frame AVANT `render` pour chaque `DockInstance` secondaire
    /// (voir `app::render_secondary_docks`), avec les MÊMES textes déjà
    /// calculés pour la barre primaire (`clock_text`/`sys_monitor_text`/
    /// `media_title_text` : des statistiques globales, pas propres à un
    /// écran) sauf `refresh_rate_text`, propre à CET écran (voir
    /// l'appelant : `win32::display::refresh_rate_hz` prend le HWND de la
    /// fenêtre secondaire concernée, pas celui de la primaire).
    /// `dot_count` : le nombre de bureaux virtuels À NUMÉROTER, tel que
    /// `win32::desktops::VirtualDesktops::count` — passé par l'appelant plutôt
    /// que lu ici, parce que `DockInstance` ne connaît pas les bureaux virtuels
    /// (c'est `render` qui les reçoit, en LECTURE seule, et il ne peut donc pas
    /// remplir un cache). Un compte à zéro libère simplement les numéros.
    pub fn sync_text(
        &mut self,
        device: &ID3D11Device,
        clock_text: &str,
        sys_monitor_text: &str,
        refresh_rate_text: &str,
        media_title_text: &str,
        dot_count: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Taille de police des textes de la barre du haut de CET écran (voir
        // `ui::dock::Dock::topbar_text_size`), lue UNE fois : elle est la même
        // pour les quatre textes, et la lire ici évite de réemprunter
        // `self.dock` à chaque appel alors que `self.clock`/`sys_monitor`/
        // `refresh_rate`/`media_title` sont empruntés mutuellement juste après.
        // `None` = automatique, déduite de l'épaisseur de CETTE barre.
        let text_size = self.dock.topbar_text_size();
        self.clock.sync(device, clock_text, topbar::clock_font_px(&self.layout, text_size), topbar::CLOCK_FONT_NAME, true)?;
        self.sys_monitor.sync(device, sys_monitor_text, topbar::sys_monitor_font_px(&self.layout, text_size), topbar::SYS_MONITOR_FONT_NAME, false)?;
        self.refresh_rate.sync(device, refresh_rate_text, topbar::refresh_rate_font_px(&self.layout, text_size), topbar::REFRESH_RATE_FONT_NAME, false)?;
        self.media_title.sync(device, media_title_text, topbar::media_title_font_px(&self.layout, text_size), topbar::MEDIA_TITLE_FONT_NAME, false)?;
        self.verrous.sync(device, &self.layout, text_size)?;
        self.debit.sync(device, &self.layout, text_size)?;
        // La taille de pastille est celle de CET écran (voir
        // `Dock::desktop_dot_size`) : un écran secondaire qui règle ses
        // pastilles plus petites voit donc ses numéros suivre — et disparaître
        // sous le seuil de `ui::desktop_labels`, ce qui est le comportement
        // voulu, pas un effet de bord.
        let dot_size = self.dock.desktop_dot_size();
        self.desktop_labels.sync(device, dot_count, dot_size)?;
        Ok(())
    }

    /// Dimensions du TITRE du bloc média de CET écran, `None` quand le bloc n'y
    /// est pas dessiné (fonctionnalité éteinte, ou aucune session multimédia).
    ///
    /// **C'est ce qui rend le geste des séparateurs de rangée possible sur un
    /// écran secondaire** : la position du troisième emplacement dépend du bord
    /// DROIT de ce bloc (voir `app::media_block_right`), et chaque écran a le
    /// sien — le titre y est régénéré à la taille de police de SA barre (voir
    /// `sync_text`), donc les dimensions diffèrent d'un écran à l'autre.
    ///
    /// **Des DIMENSIONS et non la texture** : la géométrie n'a besoin que de la
    /// largeur et de la hauteur, et `media_title`/`CachedText` sont PRIVÉS —
    /// `app::run` ne peut donc pas les lire directement, et c'est cet accesseur
    /// qui les lui donne. Il doit être appelé AVANT que l'appelant n'emprunte
    /// `dock` mutuellement (voir `app::run`, bras `Event::RightClick`).
    pub fn media_block_dims(&self) -> Option<(f32, f32)> {
        // `features.media` compte : sans lui, le bloc n'est pas dessiné (voir
        // `render`), donc un emplacement « après le média » n'aurait pas
        // d'ancre — et l'annoncer ferait viser un marqueur qui n'existe pas.
        if !self.features.media {
            return None;
        }
        self.media_title.texture.as_ref().map(|(_, w, h)| (*w, *h))
    }

    /// Renders the dock UI to an off-screen texture.
    ///
    /// # Arguments
    /// * `device` - The Direct3D 11 device.
    /// * `ctx` - The device context for rendering.
    /// * `width` - The width of the render target texture.
    /// * `height` - The height of the render target texture.
    /// * `textures` - A bundle of shared texture resources used by the dock.
    /// * `bar_backdrops` - The rendered background textures for the three bars.
    ///
    /// # Returns
    /// The shader resource view of the rendered dock UI, or an error if
    /// rendering failed.
    /// La couleur PEINTE de chaque barre `[haut, gauche, droite, bas]` — celle des
    /// fonds de `render` (`color::soften_tint`), que les ombres peuvent prendre
    /// (voir `win32::ombre_barre::couleur_des_barres`).
    pub fn couleurs_reglees(&self) -> [[f32; 3]; 4] {
        std::array::from_fn(|i| {
            let [r, g, b, _] = self.dock.bar_color(i);
            [r, g, b]
        })
    }

    pub fn couleurs_des_barres(&self) -> [[f32; 3]; 4] {
        std::array::from_fn(|i| {
            let [r, g, b, _] = self.dock.bar_color(i);
            color::soften_tint([r, g, b], self.dark_mode)
        })
    }

    pub fn render(
        &self,
        device: &ID3D11Device,
        ctx: &ID3D11DeviceContext,
        width: u32,
        height: u32,
        textures: &DockTextures,
        bar_backdrops: &BarBackdrops,
        // Le relief DESSINÉ du style actif (voir `gfx::relief`) — `None` pour un
        // style qui n'en porte pas. ⚠️ Il est PASSÉ ici plutôt que rechargé : un
        // écran secondaire se redessine à chaque frame, et décoder deux PNG par
        // frame serait payer le relief soixante fois par seconde.
        relief: Option<&crate::gfx::relief::JeuRelief>,
        virtual_desktops: Option<&crate::win32::desktops::VirtualDesktops>,
        // Coin supérieur gauche de la fenêtre, pour le dégradé animé (voir
        // `QuadRenderer::render_at`).
        screen_origin: (i32, i32),
        // Le calque des ombres des barres (voir `win32::ombre_barre::vue_des_ombres`).
        ombres: Option<ID3D11ShaderResourceView>,
    ) -> Result<ID3D11ShaderResourceView, Box<dyn std::error::Error>> {
        use windows::Win32::Graphics::Direct3D11::{
            ID3D11RenderTargetView, ID3D11Texture2D, D3D11_TEXTURE2D_DESC,
            D3D11_RENDER_TARGET_VIEW_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC,
            D3D11_USAGE_DEFAULT, D3D11_BIND_FLAG, D3D11_CPU_ACCESS_FLAG,
            D3D11_RESOURCE_MISC_FLAG,
        };

        // Create render target texture
        let mut tex_desc = D3D11_TEXTURE2D_DESC::default();
        tex_desc.Width = width as u32;
        tex_desc.Height = height as u32;
        tex_desc.MipLevels = 1;
        tex_desc.ArraySize = 1;
        tex_desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
        tex_desc.SampleDesc.Count = 1;
        tex_desc.Usage = D3D11_USAGE_DEFAULT;
        tex_desc.BindFlags = (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32;
        tex_desc.CPUAccessFlags = 0;
        tex_desc.MiscFlags = 0;

        let mut render_target_texture: Option<ID3D11Texture2D> = None;
        let mut render_target_view: Option<ID3D11RenderTargetView> = None;
        let mut shader_resource_view: Option<ID3D11ShaderResourceView> = None;

        unsafe {
            device.CreateTexture2D(&tex_desc, None, Some(&mut render_target_texture))?;

            let mut rtv_desc = D3D11_RENDER_TARGET_VIEW_DESC::default();
            rtv_desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
            rtv_desc.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2D;
            rtv_desc.Anonymous.Texture2D.MipSlice = 0;
            device.CreateRenderTargetView(
                render_target_texture.as_ref().unwrap(),
                Some(&rtv_desc as *const _),
                Some(&mut render_target_view),
            )?;

            let mut srv_desc = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
            srv_desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
            srv_desc.ViewDimension = windows::Win32::Graphics::Direct3D::D3D11_SRV_DIMENSION_TEXTURE2D;
            srv_desc.Anonymous.Texture2D.MostDetailedMip = 0;
            srv_desc.Anonymous.Texture2D.MipLevels = 1;
            device.CreateShaderResourceView(
                render_target_texture.as_ref().unwrap(),
                Some(&srv_desc as *const _),
                Some(&mut shader_resource_view),
            )?;
        }

        let render_target_texture = render_target_texture.ok_or("Failed to create render target texture")?;
        let render_target_view = render_target_view.ok_or("Failed to create render target view")?;
        let shader_resource_view = shader_resource_view.ok_or("Failed to create shader resource view")?;

        // Clear render target to transparent black
        let clear_color = [0.0, 0.0, 0.0, 0.0];
        unsafe {
            ctx.ClearRenderTargetView(
                &render_target_view,
                &clear_color,
            );
        }

        // Get dock background color
        let background = self.dock.background();

        // Create layout from instance state
        let layout = self.layout;

        // Calculate top left offset (for topbar icons)
        let top_left_offset = self.dock.top_zone_widths(&layout).0;

        // Determine effective topbar text color
        // Même règle que `app::run` : le mode clair impose son pôle, sans quoi
        // un texte manuel blanc disparaîtrait sur la surface claire.
        let effective_topbar_text_color = if self.auto_color_scheme || crate::ui::color::mode_clair() {
            crate::ui::color::menu_fg()
        } else {
            // Convert from [u8; 4] to [f32; 3] (alpha ignored for now)
            [
                self.topbar_text_color[0] as f32 / 255.0,
                self.topbar_text_color[1] as f32 / 255.0,
                self.topbar_text_color[2] as f32 / 255.0,
            ]
        };

        // Couleur des séparateurs (voir `Dock::separator_color`) : couleur
        // choisie par l'utilisateur pour CET écran s'il y en a une (voir
        // `app::rebuild_secondary_docks`, qui y bascule soit l'override
        // propre à cet écran soit celui, potentiellement lui-même absent, de
        // la barre PRIMAIRE), sinon la couleur principale du texte de CETTE
        // barre — DEMANDE EXPLICITE ("les séparateurs d'icones soit de la
        // couleur principale du texte de la barre à laquelle ils sont
        // ratachés... pour l'interface principale et les barres
        // secondaires").
        let texte_barre = [effective_topbar_text_color[0], effective_topbar_text_color[1], effective_topbar_text_color[2], 1.0];
        let separator_color = self.dock.separator_color().unwrap_or(texte_barre);
        // **LA COULEUR DU TRAIT NE SUIT PAS UNE TRANSPARENCE VOLONTAIRE** — même
        // règle que dans `app::run`, qui porte la raison complète : un marqueur
        // transparent laisse un blanc qui sépare déjà, un TRAIT transparent ne
        // dessine RIEN. Deux chemins de dessin, une seule règle — c'est ce qui
        // empêche une sorte de ne s'appliquer que sur un écran.
        let separator_line_color = if separator_color[3] > 0.0 { separator_color } else { texte_barre };
        // Taille des séparateurs (voir `ui::dock::Dock::separator_size`) :
        // déjà résolue pour CET écran par `app::rebuild_secondary_docks`
        // (override propre ou repli sur la barre PRIMAIRE) — DEMANDE
        // EXPLICITE ("ajoute la possibilité de modifier la taille des
        // séparateurs par interface"), même logique que `separator_color`
        // juste au-dessus.
        let separator_size = self.dock.separator_size();

        // Build rendering batches (adapted from app::run)
        let mut batches = Vec::new();

        // Découpage des barres LATÉRALES — même correctif que dans `app::run`
        // (voir `Dock::lateral_clip`, qui documente l'hypothèse fausse) : ces
        // barres défilent (`app.rs` route la molette vers `scroll_bar` même
        // pour un dock secondaire), donc leur contenu peut sortir par le haut
        // et se dessiner sur la barre du haut. Et découpage des ZONES, plus
        // serré (voir `Dock::zone_clips`), exactement comme dans `app::run` —
        // les deux chemins doivent découper PAREIL, sous peine qu'une zone se
        // comporte autrement sur un écran secondaire.
        let lateral_clip = Dock::lateral_clip(&layout);
        let zone_clips = self.dock.zone_clips(&layout);
        let clip_of = |index: Option<usize>, side: Option<Side>| -> Option<[f32; 4]> {
            if let Some(clip) = index.and_then(|i| zone_clips.get(i).copied().flatten()) {
                return Some(clip);
            }
            side.filter(|side| side.is_lateral()).map(|_| lateral_clip)
        };
        let clipped_for = |index: usize| clip_of(Some(index), self.dock.icon_side(index));

        // Bar backgrounds (opaques, drawn first) - apply bar tint using dock's bar_color and color::soften_tint
        let bar_rects = bar_rects(&layout);
        let bar_instances = std::array::from_fn::<_, 4, _>(|i| {
            let [r, g, b, _] = self.dock.bar_color(i);
            let softened = color::soften_tint([r, g, b], self.dark_mode);
            // "Visible dans les captures d'écran" (voir `gfx::
            // capture_visibility`, DEMANDE EXPLICITE) — même principe que
            // `app::run` (barre PRIMAIRE) : opacité forcée à 100% pour le
            // RENDU uniquement, `self.bar_tint[i]` inchangé.
            let alpha = crate::gfx::capture_visibility::tint_alpha(self.bar_tint[i] as f32 / 255.0);
            let tint = [softened[0], softened[1], softened[2], alpha];
            QuadInstance::new_backdrop(bar_rects[i], tint, 0.0)
        });
        batches.push(QuadBatch::new(Some(&bar_backdrops.top.srv), std::slice::from_ref(&bar_instances[0])));
        batches.push(QuadBatch::new(Some(&bar_backdrops.left.srv), std::slice::from_ref(&bar_instances[1])));
        batches.push(QuadBatch::new(Some(&bar_backdrops.right.srv), std::slice::from_ref(&bar_instances[2])));
        batches.push(QuadBatch::new(Some(&bar_backdrops.bottom.srv), std::slice::from_ref(&bar_instances[3])));
        // LE RELIEF DESSINÉ — les deux bandes de chaque barre, posées SUR le fond
        // flouté et SOUS les icônes. C'est le MÊME dessin que sur la barre primaire
        // (voir `app::run`, qui porte le pourquoi) : deux écrans qui ne montreraient
        // pas le même relief se verraient immédiatement.
        let relief_instances: Vec<QuadInstance> = match relief {
            Some(jeu) => bar_rects
                .iter()
                .flat_map(|rect| {
                    let [haut, bas] = jeu.bandes(*rect);
                    [QuadInstance::new_textured(haut, 0.0, 1.0), QuadInstance::new_textured(bas, 0.0, 1.0)]
                })
                .collect(),
            None => Vec::new(),
        };
        if let Some(jeu) = relief {
            for i in 0..bar_rects.len() {
                batches.push(QuadBatch::new(Some(&jeu.haut.srv), std::slice::from_ref(&relief_instances[2 * i])));
                batches.push(QuadBatch::new(Some(&jeu.bas.srv), std::slice::from_ref(&relief_instances[2 * i + 1])));
            }
        }

        // Plain background (for dock icons area)
        // ⚠️ **S'ARRÊTE AU-DESSUS DE LA BARRE DU BAS** — même règle que
        // `dock_region_rects` et `ui::dock::bar_rects` : la zone libre est ce
        // que le cadre laisse au milieu.
        let plain_rect = [
            layout.left_width,
            layout.top_thickness,
            layout.total_width - layout.left_width - layout.right_width,
            layout.total_height - layout.top_thickness - layout.bottom_thickness,
        ];
        let plain_instance = QuadInstance::new(
            plain_rect,
            [background[0], background[1], background[2], background[3]],
            0.0,
            1.0,
        );
        batches.push(QuadBatch::new(None, std::slice::from_ref(&plain_instance)));

        // Espaces invisibles (voir `Icon::is_spacer`) : petit cercle
        // indicateur, voir `Dock::spacer_instances` — BUG RÉEL CONSTATÉ
        // ("les séparateurs d'icones... pour l'interface principale et les
        // barres secondaires") : jusqu'ici absent des barres secondaires,
        // qui construisent chaque icône nommée à la main plutôt que par un
        // seul passage générique comme la barre primaire (voir
        // `Dock::render_instances`, `app::run`).
        // ÉCLATÉS PAR DÉCOUPAGE, comme dans `app::run` (voir `ClipGroups`) :
        // ceux des barres latérales sont découpés (leur barre défile), ceux
        // d'une ZONE le sont à sa boîte, et ceux de la barre du haut pas du
        // tout — voir `Dock::lateral_clip`/`zone_clips`, et le défaut qu'ils
        // corrigent. L'index vient avec l'instance (voir `spacer_instances`) :
        // c'est lui qui rattache un marqueur à sa zone.
        let mut spacers = ClipGroups::new();
        for (index, side, instance) in self.dock.spacer_instances(&layout, separator_color, separator_line_color, separator_size) {
            spacers.push(clip_of(Some(index), Some(side)), instance);
        }
        spacers.push_batches(None, &mut batches);

        // Logo (icône `APP_ICON_INDEX`, comme n'importe quelle autre icône
        // du dock — voir `app::run`, même appel `icon_rect`) — BUG RÉEL
        // SIGNALÉ ("les icônes de la barre supérieure des écrans
        // secondaires ne suivent pas la taille de la barre") : ce rectangle
        // était jusqu'ici un carré 64×64 FIXE, centré dans toute la zone
        // libre sous la barre du haut plutôt que posé dans la barre elle-
        // même — ni la taille ni la position ne suivaient `layout.
        // top_thickness` (voir `Dock::icon_rect`/`icon_size_for`, déjà
        // utilisés par toutes les AUTRES icônes ci-dessous, qui n'avaient
        // donc jamais ce problème).
        // **LE MÊME CHOIX CARRÉ/CERCLE QUE LE DOCK PRIMAIRE** (voir
        // `ui::dock::logo_is_circle`) — et c'est la deuxième fois de la session
        // qu'un élément se dessine par DEUX chemins : celui-ci doit suivre, sinon
        // un écran secondaire garderait le carré pendant que le primaire tourne.
        // La rotation REMPLACE celle du balancement, comme dans `app::run`.
        // ⚠️ L'INSTANCE RESTE DÉCLARÉE ICI, HORS DU `if let` : `push_icon_batch`
        // en retient une RÉFÉRENCE, qui doit vivre jusqu'à `render`. La lier par
        // valeur dans le bloc la ferait mourir trop tôt — le compilateur le dit,
        // mais c'est la contrainte à connaître de ce chemin.
        let mut logo_instance = self.dock.external_icon_instance(APP_ICON_INDEX, &layout);
        let circle = crate::ui::dock::logo_is_circle();
        if let Some(instance) = logo_instance.as_mut() {
            if circle {
                instance.rotation = crate::ui::dock::logo_rotation();
            }
            // Taille et zoom du logo — MÊME réglage que `app::run` (voir
            // `ui::dock::logo_draw_scale`).
            let logo_scale = crate::ui::dock::logo_draw_scale();
            instance.rect = crate::ui::dock::scale_rect_centered(instance.rect, logo_scale);
            instance.radius *= logo_scale;
            let texture = if circle { &textures.circle_logo } else { &textures.logo };
            push_icon_batch(&mut batches, texture, instance, clipped_for(APP_ICON_INDEX));
        }

        // Feature-enabled icons from top bar (power, terminal, etc.)
        let power_icon_index = PINNED_ICON_BASE + self.pinned_apps_count;
        let power_instance = self.dock.external_glyph_instance(
            power_icon_index,
            &layout,
            effective_topbar_text_color,
        );
        if let Some(instance) = &power_instance {
            push_icon_batch(&mut batches, &textures.power, instance, clipped_for(power_icon_index));
        }

        let topbar_terminal_icon_index = power_icon_index + 1;
        let terminal_instance = self.dock.external_glyph_instance(
            topbar_terminal_icon_index,
            &layout,
            effective_topbar_text_color,
        );
        if let Some(instance) = &terminal_instance {
            push_icon_batch(&mut batches, &textures.terminal, instance, clipped_for(topbar_terminal_icon_index));
        }

        let sound_icon_index = topbar_terminal_icon_index + 1;
        let sound_instance = self.dock.external_glyph_instance(
            sound_icon_index,
            &layout,
            effective_topbar_text_color,
        );
        if let Some(instance) = &sound_instance {
            push_icon_batch(&mut batches, &textures.sound, instance, clipped_for(sound_icon_index));
        }

        let network_icon_index = sound_icon_index + 1;
        let network_instance = self.dock.external_glyph_instance(
            network_icon_index,
            &layout,
            effective_topbar_text_color,
        );
        if let Some(instance) = &network_instance {
            push_icon_batch(&mut batches, &textures.network, instance, clipped_for(network_icon_index));
        }

        let voicemeeter_icon_index = network_icon_index + 1;
        let voicemeeter_instance = self.dock.external_glyph_instance(
            voicemeeter_icon_index,
            &layout,
            effective_topbar_text_color,
        );
        if let Some(instance) = &voicemeeter_instance {
            push_icon_batch(&mut batches, &textures.sound, instance, clipped_for(voicemeeter_icon_index)); // Reuses sound texture
        }

        let notes_icon_index = voicemeeter_icon_index + 1;
        let notes_instance = self.dock.external_glyph_instance(
            notes_icon_index,
            &layout,
            effective_topbar_text_color,
        );
        if let Some(instance) = &notes_instance {
            push_icon_batch(&mut batches, &textures.notes, instance, clipped_for(notes_icon_index));
        }

        let theme_toggle_icon_index = notes_icon_index + 1;
        let theme_instance = self.dock.external_glyph_instance(
            theme_toggle_icon_index,
            &layout,
            effective_topbar_text_color,
        );
        if let Some(instance) = &theme_instance {
            push_icon_batch(&mut batches, &textures.theme, instance, clipped_for(theme_toggle_icon_index));
        }

        // Éclairage nocturne — **APRÈS la bascule de thème**, comme dans `app::run` :
        // l'applet est la DERNIÈRE du bloc, donc son index vaut le précédent plus un, et
        // une place ajoutée en fin de bloc ne décale aucun index existant.
        let night_light_icon_index = theme_toggle_icon_index + 1;
        let night_light_instance = self.dock.external_glyph_instance(
            night_light_icon_index,
            &layout,
            effective_topbar_text_color,
        );
        if let Some(instance) = &night_light_instance {
            push_icon_batch(&mut batches, &textures.night_light, instance, clipped_for(night_light_icon_index));
        }

        // File d'impression — ENCORE EN DERNIER, même raison que ci-dessus.
        let printer_icon_index = night_light_icon_index + 1;
        let printer_instance = self.dock.external_glyph_instance(printer_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &printer_instance {
            push_icon_batch(&mut batches, &textures.printer, instance, clipped_for(printer_icon_index));
        }

        // Calendrier — ENCORE EN DERNIER, même raison que ci-dessus.
        let calendar_icon_index = printer_icon_index + 1;
        let calendar_instance = self.dock.external_glyph_instance(calendar_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &calendar_instance {
            push_icon_batch(&mut batches, &textures.calendar, instance, clipped_for(calendar_icon_index));
        }

        // Éditeur d'image — ENCORE EN DERNIER, même raison que ci-dessus.
        let image_editor_icon_index = calendar_icon_index + 1;
        let image_editor_instance = self.dock.external_glyph_instance(image_editor_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &image_editor_instance {
            push_icon_batch(&mut batches, &textures.image_editor, instance, clipped_for(image_editor_icon_index));
        }
        // Capture d'écran ENCORE EN DERNIER, même raison que ci-dessus.
        let screen_capture_icon_index = image_editor_icon_index + 1;
        let screen_capture_instance = self.dock.external_glyph_instance(screen_capture_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &screen_capture_instance {
            push_icon_batch(&mut batches, &textures.screen_capture, instance, clipped_for(screen_capture_icon_index));
        }
        // Minuteur ENCORE EN DERNIER, même raison — et il clignote ici aussi
        // quand quelque chose sonne (l'état vit dans `win32::minuteur`).
        let minuteur_icon_index = screen_capture_icon_index + 1;
        let minuteur_color = if crate::win32::minuteur::sonne() {
            let c = crate::ui::dock::notification_blink_color();
            [c[0], c[1], c[2]]
        } else {
            effective_topbar_text_color
        };
        let minuteur_instance = self.dock.external_glyph_instance(minuteur_icon_index, &layout, minuteur_color);
        if let Some(instance) = &minuteur_instance {
            push_icon_batch(&mut batches, &textures.minuteur, instance, clipped_for(minuteur_icon_index));
        }
        // Surveillance ENCORE EN DERNIER, même raison — et elle clignote en
        // rouge ici aussi tant qu'une cible est injoignable.
        let surveillance_icon_index = minuteur_icon_index + 1;
        let surveillance_color = crate::win32::surveillance::couleur_icone(effective_topbar_text_color);
        let surveillance_instance = self.dock.external_glyph_instance(surveillance_icon_index, &layout, surveillance_color);
        if let Some(instance) = &surveillance_instance {
            push_icon_batch(&mut batches, &textures.surveillance, instance, clipped_for(surveillance_icon_index));
        }
        // Windows Update ENCORE EN DERNIER, même raison.
        let windows_update_icon_index = surveillance_icon_index + 1;
        let windows_update_instance = self.dock.external_glyph_instance(windows_update_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &windows_update_instance {
            push_icon_batch(&mut batches, &textures.windows_update, instance, clipped_for(windows_update_icon_index));
        }
        // Gestionnaire de périphériques ENCORE EN DERNIER, même raison.
        let device_manager_icon_index = windows_update_icon_index + 1;
        let device_manager_instance = self.dock.external_glyph_instance(device_manager_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &device_manager_instance {
            push_icon_batch(&mut batches, &textures.device_manager, instance, clipped_for(device_manager_icon_index));
        }
        // Réglages de l'écran ENCORE EN DERNIER, même raison.
        let display_settings_icon_index = device_manager_icon_index + 1;
        let display_settings_instance = self.dock.external_glyph_instance(display_settings_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &display_settings_instance {
            push_icon_batch(&mut batches, &textures.display_settings, instance, clipped_for(display_settings_icon_index));
        }
        // Calculatrice ENCORE EN DERNIER, même raison.
        let calculatrice_icon_index = display_settings_icon_index + 1;
        let calculatrice_instance = self.dock.external_glyph_instance(calculatrice_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &calculatrice_instance {
            push_icon_batch(&mut batches, &textures.calculatrice, instance, clipped_for(calculatrice_icon_index));
        }
        // Serveurs SSH ENCORE EN DERNIER, même raison.
        let ssh_icon_index = calculatrice_icon_index + 1;
        let ssh_instance = self.dock.external_glyph_instance(ssh_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &ssh_instance {
            push_icon_batch(&mut batches, &textures.ssh, instance, clipped_for(ssh_icon_index));
        }
        // Sélecteur de couleurs ENCORE EN DERNIER, même raison.
        let couleurs_icon_index = ssh_icon_index + 1;
        let couleurs_instance = self.dock.external_glyph_instance(couleurs_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &couleurs_instance {
            push_icon_batch(&mut batches, &textures.couleurs, instance, clipped_for(couleurs_icon_index));
        }
        // Fonctionnalités facultatives ENCORE EN DERNIER, même raison.
        let optional_features_icon_index = couleurs_icon_index + 1;
        let optional_features_instance = self.dock.external_glyph_instance(optional_features_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &optional_features_instance {
            push_icon_batch(&mut batches, &textures.optional_features, instance, clipped_for(optional_features_icon_index));
        }
        // Notifications ENCORE EN DERNIER, même raison.
        let notifications_icon_index = optional_features_icon_index + 1;
        let notifications_instance = self.dock.external_glyph_instance(notifications_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &notifications_instance {
            push_icon_batch(&mut batches, &textures.notifications, instance, clipped_for(notifications_icon_index));
        }
        // Recherche ENCORE EN DERNIER, même raison.
        let recherche_icon_index = notifications_icon_index + 1;
        let recherche_instance = self.dock.external_glyph_instance(recherche_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &recherche_instance {
            push_icon_batch(&mut batches, &textures.recherche, instance, clipped_for(recherche_icon_index));
        }
        // Automatisations ENCORE EN DERNIER, même raison.
        let automatisations_icon_index = recherche_icon_index + 1;
        let automatisations_instance = self.dock.external_glyph_instance(automatisations_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &automatisations_instance {
            push_icon_batch(&mut batches, &textures.automatisations, instance, clipped_for(automatisations_icon_index));
        }
        // Météo ENCORE EN DERNIER, même raison.
        let meteo_icon_index = automatisations_icon_index + 1;
        let meteo_instance = self.dock.external_glyph_instance(meteo_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &meteo_instance {
            push_icon_batch(&mut batches, &textures.meteo, instance, clipped_for(meteo_icon_index));
        }
        // Verrouiller la session ENCORE EN DERNIER, même raison — DEMANDE EXPLICITE
        // (2026-10-05). ⚠️ Elle est ÉTEINTE d'office sur un écran secondaire (voir
        // `app::SECONDARY_DEFAULT_DISABLED_FEATURES`) : son dessin ici est donc une
        // ceinture, la barre de droite n'existant pas là-bas.
        let verrouillage_icon_index = meteo_icon_index + 1;
        let verrouillage_instance = self.dock.external_glyph_instance(verrouillage_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &verrouillage_instance {
            push_icon_batch(&mut batches, &textures.verrouillage, instance, clipped_for(verrouillage_icon_index));
        }
        // Caractères accentués ENCORE EN DERNIER, même raison — DEMANDE EXPLICITE
        // (2026-10-06). ⚠️ ÉTEINTE d'office sur un écran secondaire (voir
        // `app::SECONDARY_DEFAULT_DISABLED_FEATURES`) : son dessin ici est une
        // ceinture, la barre de droite n'existant pas là-bas.
        let caracteres_icon_index = verrouillage_icon_index + 1;
        let caracteres_instance = self.dock.external_glyph_instance(caracteres_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &caracteres_instance {
            push_icon_batch(&mut batches, &textures.caracteres, instance, clipped_for(caracteres_icon_index));
        }
        // Kaomojis ENCORE EN DERNIER, même raison — DEMANDE EXPLICITE (2026-10-06).
        // ⚠️ ÉTEINTE d'office sur un écran secondaire, comme les caractères : son
        // dessin ici est une ceinture, la barre de droite n'existant pas là-bas.
        let kaomoji_icon_index = caracteres_icon_index + 1;
        let kaomoji_instance = self.dock.external_glyph_instance(kaomoji_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &kaomoji_instance {
            push_icon_batch(&mut batches, &textures.kaomoji, instance, clipped_for(kaomoji_icon_index));
        }
        // Propriétés système ENCORE EN DERNIER, même raison — DEMANDE EXPLICITE
        // (2026-10-08). ⚠️ ÉTEINTE d'office sur un écran secondaire, comme les
        // caractères et les kaomojis : son dessin ici est une ceinture, la barre
        // de droite n'existant pas là-bas.
        //
        // ⛔ **L'AIDE OCCUPE UN INDEX SANS ÊTRE DESSINÉE ICI — ET ON L'ÉCRIT.**
        // La chaîne ci-dessus est POSITIONNELLE sur le dock ENTIER, pas sur ce
        // qu'on dessine : `Dock::new` pousse kaomojis → **AIDE** → propriétés
        // système, et ce fichier n'a **aucun** bloc de rendu pour l'aide
        // (`textures.aide` n'y est jamais lu — c'est le sens de l'avertissement du
        // compilateur, « field `aide` is never read »). Sauter ce maillon ferait
        // viser **la case de l'aide**, y dessinerait notre icône, et laisserait la
        // nôtre vide.
        //
        // ⚠️ **CE DÉFAUT SERAIT INVISIBLE** : l'aide et les propriétés système sont
        // TOUTES DEUX dans `app::SECONDARY_DEFAULT_DISABLED_FEATURES`, donc leurs
        // deux cases sont vides tant que rien ne les rallume — mais un
        // `secondary_screen_features.txt` qui réactive l'aide ferait apparaître
        // l'icône des propriétés **à sa place**.
        //
        // ⚠️ **POURQUOI UNE VARIABLE `_` PLUTÔT QU'UN `+ 2`** : un décalage écrit en
        // clair se lit comme un chiffre magique, et se recopie faux à la prochaine
        // applet. Ici le saut est NOMMÉ, donc visible — et le prochain ajoutera
        // `proprietes_systeme_icon_index + 1`, qui sera juste.
        //
        // ⚠️ **AUCUN TEST NE GARDE CETTE ARITHMÉTIQUE** : les `label` de `Dock::new`
        // ne sont PAS uniques (`'f'` et `'g'` y sont chacun utilisés DEUX FOIS —
        // mesuré), donc ils ne peuvent pas servir d'identité, et `kaomoji_icon_index`
        // est local à cette fonction. Un test bâti sur l'unicité des labels a été
        // écrit puis RETIRÉ pour cette raison.
        //
        // ⭐ **CE QUI EN TIENT LIEU, ET QUI SE MESURE** : sur les 28 maillons de
        // cette chaîne, les **27 premiers sont identiques à ceux d'`app.rs`
        // (`apply_topbar_features` et `run`, elles-mêmes jumelles)**, et le seul
        // écart est ce dernier maillon — `aide` y est simplement renommée ici.
        // Les deux chaînes partent donc de la même base et arrivent au MÊME index,
        // qui est celui que `Dock::new` donne à cette applet. Comparer les deux
        // listes est la vérification ; il n'y a pas de raccourci.
        let _aide_occupe_une_case = kaomoji_icon_index + 1;
        let proprietes_systeme_icon_index = _aide_occupe_une_case + 1;
        let proprietes_systeme_instance = self.dock.external_glyph_instance(proprietes_systeme_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &proprietes_systeme_instance {
            push_icon_batch(&mut batches, &textures.proprietes_systeme, instance, clipped_for(proprietes_systeme_icon_index));
        }
        // ⭐ **L'APPLET SCANNER** (voir `win32::scanner`) — MÊME RÈGLE QUE
        // L'APPLET PRÉCÉDENTE : elle occupe un index sur le dock ENTIER, et cette
        // chaîne doit compter ce qu'elle ne dessine pas. La nommer plutôt
        // qu'écrire `+ 2` : le prochain ajoutera `+ 1` et sera juste.
        let _scanner_occupe_une_case = proprietes_systeme_icon_index + 1;
        let scanner_instance = self.dock.external_glyph_instance(_scanner_occupe_une_case, &layout, effective_topbar_text_color);
        if let Some(instance) = &scanner_instance {
            push_icon_batch(&mut batches, &textures.scanner, instance, clipped_for(_scanner_occupe_une_case));
        }
        // L'ÉDITEUR DE SHADERS (voir `win32::cef_shaders`) — même règle, `+ 1`.
        let shaders_icon_index = _scanner_occupe_une_case + 1;
        let shaders_instance = self.dock.external_glyph_instance(shaders_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &shaders_instance {
            push_icon_batch(&mut batches, &textures.shaders, instance, clipped_for(shaders_icon_index));
        }
        // LES SYMBOLES PHONÉTIQUES (voir `win32::cef_phonetique`) — même règle, `+ 1`.
        let phonetique_icon_index = shaders_icon_index + 1;
        let phonetique_instance = self.dock.external_glyph_instance(phonetique_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &phonetique_instance {
            push_icon_batch(&mut batches, &textures.phonetique, instance, clipped_for(phonetique_icon_index));
        }
        // LES SYMBOLES MATHÉMATIQUES (voir `win32::cef_maths`) — même règle, `+ 1`.
        let maths_icon_index = phonetique_icon_index + 1;
        let maths_instance = self.dock.external_glyph_instance(maths_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &maths_instance {
            push_icon_batch(&mut batches, &textures.maths, instance, clipped_for(maths_icon_index));
        }
        // L'ÉDITEUR DES LUEURS (voir `win32::cef_lueurs`) — même règle, `+ 1`.
        let lueurs_icon_index = maths_icon_index + 1;
        let lueurs_instance = self.dock.external_glyph_instance(lueurs_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &lueurs_instance {
            push_icon_batch(&mut batches, &textures.lueurs, instance, clipped_for(lueurs_icon_index));
        }
        // L'ÉDITEUR DES TEXTURES (voir `win32::cef_textures`) — même règle, `+ 1`.
        let textures_icon_index = lueurs_icon_index + 1;
        let textures_instance = self.dock.external_glyph_instance(textures_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &textures_instance {
            push_icon_batch(&mut batches, &textures.textures, instance, clipped_for(textures_icon_index));
        }
        // LE NARRATEUR (voir `win32::narrateur`) — même règle, `+ 1`.
        let narrateur_icon_index = textures_icon_index + 1;
        let narrateur_instance = self.dock.external_glyph_instance(narrateur_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &narrateur_instance {
            push_icon_batch(&mut batches, &textures.narrateur, instance, clipped_for(narrateur_icon_index));
        }
        // LA LOUPE (voir `win32::loupe_windows`) — même règle, `+ 1`.
        let loupe_icon_index = narrateur_icon_index + 1;
        let loupe_instance = self.dock.external_glyph_instance(loupe_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &loupe_instance {
            push_icon_batch(&mut batches, &textures.loupe, instance, clipped_for(loupe_icon_index));
        }
        // LE MONITEUR SYSTÈME (voir `win32::cef_moniteur`) — même règle, `+ 1`.
        let moniteur_icon_index = loupe_icon_index + 1;
        let observateur_evenements_icon_index = moniteur_icon_index + 1;
        let observateur_evenements_instance =
            self.dock.external_glyph_instance(observateur_evenements_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &observateur_evenements_instance {
            push_icon_batch(&mut batches, &textures.observateur_evenements, instance, clipped_for(observateur_evenements_icon_index));
        }
        // ENREGISTRER (voir `win32::cef_enregistrer`) — même règle, `+ 1`.
        let enregistrer_icon_index = observateur_evenements_icon_index + 1;
        let enregistrer_instance = self.dock.external_glyph_instance(enregistrer_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &enregistrer_instance {
            push_icon_batch(&mut batches, &textures.enregistrer, instance, clipped_for(enregistrer_icon_index));
        }
        // CLAVIER VISUEL (voir `win32::clavier_visuel`) — même règle, `+ 1`.
        let clavier_icon_index = enregistrer_icon_index + 1;
        let clavier_instance = self.dock.external_glyph_instance(clavier_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &clavier_instance {
            push_icon_batch(&mut batches, &textures.clavier, instance, clipped_for(clavier_icon_index));
        }
        // 2048 (voir `win32::cef_jeu2048`) — même règle, `+ 1`.
        let jeu2048_icon_index = clavier_icon_index + 1;
        let jeu2048_instance = self.dock.external_glyph_instance(jeu2048_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &jeu2048_instance {
            push_icon_batch(&mut batches, &textures.jeu2048, instance, clipped_for(jeu2048_icon_index));
        }
        // CINQ OU PLUS (voir `win32::cef_cinq_ou_plus`) — même règle, `+ 1`.
        let cinq_ou_plus_icon_index = jeu2048_icon_index + 1;
        let cinq_ou_plus_instance = self.dock.external_glyph_instance(cinq_ou_plus_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &cinq_ou_plus_instance {
            push_icon_batch(&mut batches, &textures.cinq_ou_plus, instance, clipped_for(cinq_ou_plus_icon_index));
        }
        // QUADRAPASSEL (voir `win32::cef_quadrapassel`) — même règle, `+ 1`.
        let quadrapassel_icon_index = cinq_ou_plus_icon_index + 1;
        let quadrapassel_instance = self.dock.external_glyph_instance(quadrapassel_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &quadrapassel_instance {
            push_icon_batch(&mut batches, &textures.quadrapassel, instance, clipped_for(quadrapassel_icon_index));
        }
        // LIGHTSOFF (voir `win32::cef_lightsoff`) — même règle, `+ 1`.
        let lightsoff_icon_index = quadrapassel_icon_index + 1;
        let lightsoff_instance = self.dock.external_glyph_instance(lightsoff_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &lightsoff_instance {
            push_icon_batch(&mut batches, &textures.lightsoff, instance, clipped_for(lightsoff_icon_index));
        }
        let moniteur_instance = self.dock.external_glyph_instance(moniteur_icon_index, &layout, effective_topbar_text_color);
        if let Some(instance) = &moniteur_instance {
            push_icon_batch(&mut batches, &textures.moniteur, instance, clipped_for(moniteur_icon_index));
        }

        // Icône "réglages" : BUG RÉEL SIGNALÉ ("toutes les icones ne
        // s'affichent pas sur l'interface secondaire") — contrairement à
        // `src/app.rs` (voir `settings_render_instance` côté barre
        // principale), ce bloc n'existait pas du tout ici : l'icône
        // Réglages n'était donc JAMAIS dessinée sur les barres secondaires,
        // bien qu'elle reste toujours cliquable au même endroit (voir
        // `SETTINGS_ICON_INDEX`, index fixe, pas dérivé de
        // `pinned_apps_count` comme power/terminal/... ci-dessus).
        let settings_instance = self.dock.external_glyph_instance(
            SETTINGS_ICON_INDEX,
            &layout,
            effective_topbar_text_color,
        );
        if let Some(instance) = &settings_instance {
            push_icon_batch(&mut batches, &textures.settings, instance, clipped_for(SETTINGS_ICON_INDEX));
        }

        // Clock (instance hoisted to this scope, not the narrower `if let`
        // block below, so the reference pushed into `batches` still lives
        // when `batches` is consumed by `renderer.render` further down —
        // see the pinned-apps loop above for the same constraint).
        //
        // `if self.features.clock` : BUG RÉEL SIGNALÉ ("sélectionner ou
        // déselectionner des éléments des barres secondaires pour les
        // rendre visibles ou invisibles") — contrairement à `refresh_rate`/
        // `media` juste en dessous, l'horloge ne vérifiait ici AUCUN
        // `Features`, elle restait donc toujours visible sur un écran
        // secondaire quel que soit le réglage (global ou par écran, voir
        // `app::rebuild_secondary_docks`).
        let clock_instance = if self.features.clock {
            self.clock.texture.as_ref().map(|(clock_texture, clock_w, clock_h)| {
                // Échelle 1.0 et aucune rotation : un dock SECONDAIRE ne magnifie
                // pas sa barre du haut — même règle que les boutons média juste
                // en dessous, et que `app::topbar_hit_scales`, qui rend
                // `[1.0; 4]` pour un écran secondaire.
                (clock_texture, crate::ui::topbar::clock_instance(&layout, *clock_w, *clock_h, effective_topbar_text_color, 1.0, 0.0))
            })
        } else {
            None
        };
        if let Some((clock_texture, instance)) = &clock_instance {
            batches.push(QuadBatch::new(Some(&clock_texture.srv), std::slice::from_ref(instance)));
        }

        // System monitor - properly handle texture dimensions
        //
        // `if self.features.sys_monitor` : DEMANDE EXPLICITE ("le moniteur
        // systeme n'est pas désactivable, il n'y a pas d'entrée pour le
        // désactiver") — comme `clock_instance`/`refresh_rate_instance` dans
        // ce même fichier, ce widget ne vérifiait jusqu'ici AUCUN `Features`.
        // ⭐ LE DÉBIT RÉSEAU (voir `ui::debit`) — même chaînage que `app::run` :
        // les textes de la moitié droite s'ancrent sur SON bord gauche.
        let bord_reseau = self.dock.bord_gauche_reseau(&layout);
        let bord_droit = self.debit.bord_gauche(self.dock.top_right_zone_left_edge(&layout), bord_reseau, &layout);
        let debit_instance = self.debit.instance(bord_reseau, &layout, effective_topbar_text_color);
        if let Some((texture, instance)) = &debit_instance {
            batches.push(QuadBatch::new(Some(&texture.srv), std::slice::from_ref(instance)));
        }
        let sys_monitor_instance = if self.features.sys_monitor {
            self.sys_monitor.texture.as_ref().map(|(sys_monitor_texture, sys_monitor_w, sys_monitor_h)| {
                let instance = crate::ui::topbar::sys_monitor_instance(
                    bord_droit,
                    &layout,
                    *sys_monitor_w,
                    *sys_monitor_h,
                    effective_topbar_text_color,
                );
                (sys_monitor_texture, instance)
            })
        } else {
            None
        };
        if let Some((sys_monitor_texture, instance)) = &sys_monitor_instance {
            batches.push(QuadBatch::new(Some(&sys_monitor_texture.srv), std::slice::from_ref(instance)));
        }

        // Refresh rate - properly handle texture dimensions
        //
        // BUG RÉEL CORRIGÉ ("un élément texte qui est superposé" dans la
        // barre supérieure d'un écran secondaire) : s'accrochait directement
        // à `top_right_zone_left_edge` comme le moniteur système juste
        // au-dessus, au lieu de s'accrocher juste AVANT lui (voir
        // `topbar::sys_monitor_left_edge`, prévu pour ça) — les deux textes
        // se retrouvaient ancrés au même bord droit et se chevauchaient.
        // Même chaînage que `app::run` côté fenêtre primaire.
        let refresh_rate_instance = if self.features.refresh_rate {
            self.refresh_rate.texture.as_ref().map(|(refresh_rate_texture, refresh_rate_w, refresh_rate_h)| {
                let icons_zone_left_edge = bord_droit;
                // `.filter(|_| self.features.sys_monitor)` : voir la doc de
                // `sys_monitor_instance` juste au-dessus, même correctif que
                // côté `app::run` (fenêtre primaire) — sa texture continue
                // d'exister même désactivé, ne s'y ancrer QUE s'il est
                // réellement affiché.
                let next_left_edge = self
                    .sys_monitor
                    .texture
                    .as_ref()
                    .filter(|_| self.features.sys_monitor)
                    .map_or(icons_zone_left_edge, |(_, sw, sh)| {
                        crate::ui::topbar::sys_monitor_left_edge(icons_zone_left_edge, &layout, *sw, *sh)
                    });
                let instance = crate::ui::topbar::refresh_rate_instance(
                    next_left_edge,
                    &layout,
                    *refresh_rate_w,
                    *refresh_rate_h,
                    effective_topbar_text_color,
                );
                (refresh_rate_texture, instance)
            })
        } else {
            None
        };
        if let Some((refresh_rate_texture, instance)) = &refresh_rate_instance {
            batches.push(QuadBatch::new(Some(&refresh_rate_texture.srv), std::slice::from_ref(instance)));
        }

        // ⭐ Verr. num / Verr. maj (voir `ui::verrous`) — même chaînage que
        // `app::run` : à gauche de la fréquence d'affichage si elle est affichée.
        let verrous_instances = {
            let icons_zone_left_edge = bord_droit;
            let ancre_frequence = self
                .sys_monitor
                .texture
                .as_ref()
                .filter(|_| self.features.sys_monitor)
                .map_or(icons_zone_left_edge, |(_, sw, sh)| crate::ui::topbar::sys_monitor_left_edge(icons_zone_left_edge, &layout, *sw, *sh));
            let ancre = self
                .refresh_rate
                .texture
                .as_ref()
                .filter(|_| self.features.refresh_rate)
                .map_or(ancre_frequence, |(_, w, h)| crate::ui::topbar::refresh_rate_rect(ancre_frequence, &layout, *w, *h)[0]);
            self.verrous.instances(ancre, &layout, effective_topbar_text_color)
        };
        if let Some(paires) = &verrous_instances {
            for (i, (fond, texte)) in paires.iter().enumerate() {
                batches.push(QuadBatch::new(None, std::slice::from_ref(fond)));
                if let Some(texture) = self.verrous.texture(i) {
                    batches.push(QuadBatch::new(Some(&texture.srv), std::slice::from_ref(texte)));
                }
            }
        }

        // Media controls — titre ET boutons précédent/lecture-pause/suivant
        // (voir `app::run`, même trio de fonctions `topbar::media_*_rect`/
        // `_instance`, génériques sur `layout`/`top_left_offset` donc déjà
        // réutilisables telles quelles ici). DEMANDE EXPLICITE : "je voudrais
        // que soit présents [...] le controle medias [...] dans l'interface
        // secondaire". Masqués tant qu'aucune session multimédia active
        // n'est rapportée (`self.media_title.texture` vide, voir
        // `app::run`, même convention) plutôt qu'affichés sans effet
        // perceptible. Pas d'animation de survol ici (`topbar_hover`,
        // spécifique à la souris de la fenêtre PRIMAIRE) — échelle fixe.
        let media_instances = if self.features.media {
            self.media_title.texture.as_ref().map(|(media_title_texture, media_title_w, media_title_h)| {
                (
                    topbar::media_button_instance(topbar::media_previous_rect(&layout, crate::app::media_left_offset(self.dock.slot_widths(), top_left_offset)), effective_topbar_text_color, 1.0),
                    topbar::media_button_instance(topbar::media_play_pause_rect(&layout, crate::app::media_left_offset(self.dock.slot_widths(), top_left_offset)), effective_topbar_text_color, 1.0),
                    topbar::media_button_instance(topbar::media_next_rect(&layout, crate::app::media_left_offset(self.dock.slot_widths(), top_left_offset)), effective_topbar_text_color, 1.0),
                    topbar::media_title_instance(&layout, *media_title_w, *media_title_h, effective_topbar_text_color, crate::app::media_left_offset(self.dock.slot_widths(), top_left_offset)),
                    media_title_texture,
                )
            })
        } else {
            None
        };
        if let Some((prev, play_pause, next, title_instance, media_title_texture)) = &media_instances {
            batches.push(QuadBatch::new(Some(&textures.media_previous.srv), std::slice::from_ref(prev)));
            batches.push(QuadBatch::new(Some(&textures.media_play_pause.srv), std::slice::from_ref(play_pause)));
            batches.push(QuadBatch::new(Some(&textures.media_next.srv), std::slice::from_ref(next)));
            batches.push(QuadBatch::new(Some(&media_title_texture.srv), std::slice::from_ref(title_instance)));
        }

        // Icône « afficher le bureau » — même widget que sur la barre PRIMAIRE
        // (voir `ui::topbar::show_desktop_rect`), dessiné ici pour la même
        // raison que les pastilles juste en dessous. Le test de clic, lui, est
        // résolu PAR ÉCRAN dans `app::run` (voir sa branche `MouseDown`) : une
        // icône non dessinée ici y laisserait une zone cliquable INVISIBLE.
        let show_desktop_instance = self.features.show_desktop.then(|| {
            // Échelle 1.0 EN DUR : un dock SECONDAIRE ne magnifie jamais, faute de
            // souris primaire (voir `app::topbar_hit_scales`, qui rend `[1.0; 8]`
            // dans ce cas). Le rectangle au repos est donc le rectangle dessiné.
            topbar::show_desktop_instance(topbar::show_desktop_rect(&layout, top_left_offset), effective_topbar_text_color, 1.0)
        });
        if let Some(instance) = &show_desktop_instance {
            batches.push(QuadBatch::new(Some(&textures.show_desktop.srv), std::slice::from_ref(instance)));
        }

        // LA CLOCHE DE RAPPEL — à DROITE de l'horloge (voir `ui::topbar::bell_rect`).
        //
        // ⚠️ **GARDÉE PAR `clock_size()`**, comme partout ailleurs : `bell_rect`
        // s'ancre sur l'horloge, et une horloge de largeur nulle la poserait au
        // CENTRE de la barre. `clock_size` n'est pas gardé par `features.clock`
        // (c'est la texture d'un texte, pas un réglage) : éteindre l'horloge
        // n'emporte donc pas la cloche, exactement comme sur la barre primaire.
        //
        // ⚠️ **ELLE CLIGNOTE ICI AUSSI.** C'est la demande explicite — « sur tous
        // les écrans, comme l'horloge » — et la décision est RELUE depuis
        // `fetes::cloche_clignote`, que `app::run` publie à chaque frame (les
        // dates saisies et les rappels de fêtes ne vivent que là). Une cloche
        // éteinte sur un écran secondaire serait un rappel manqué.
        // ⚠️ **`cloche_visible()` EN PLUS DU RÉGLAGE, ET C'ÉTAIT UN DÉFAUT.** La
        // barre PRIMAIRE ne pousse rien quand il n'y a aucun rappel (voir
        // `app::run`), donc la cloche y DISPARAÎT — c'est la demande explicite
        // (« s'il n'y a aucun rappel, rendre cette cloche transparente »). Ici,
        // seul `features.bell` était consulté : sur un écran secondaire, la
        // cloche restait donc affichée EN PERMANENCE, sans rien à signaler.
        // Constaté en câblant l'icône de note, qui héritait du même oubli.
        let bell_instance = (self.features.bell && crate::win32::fetes::cloche_visible())
            .then(|| {
                self.clock_size().map(|(w, h)| {
                    let couleur = if crate::win32::fetes::cloche_clignote() {
                        let c = crate::ui::dock::notification_blink_color();
                        [c[0], c[1], c[2]]
                    } else {
                        // Au repos, la couleur du texte de la barre — comme ses
                        // voisines.
                        effective_topbar_text_color
                    };
                    topbar::bell_instance(
                        // Échelle 1.0 EN DUR, comme `show_desktop_instance`
                        // ci-dessus : un écran secondaire ne magnifie jamais.
                        topbar::presented_bell_rect(topbar::bell_rect(&layout, w, h), 1.0, &layout),
                        couleur,
                        1.0,
                    )
                })
            })
            .flatten();
        if let Some(instance) = &bell_instance {
            batches.push(QuadBatch::new(Some(&textures.bell.srv), std::slice::from_ref(instance)));
        }

        // L'ICÔNE DE NOTE — à GAUCHE de l'horloge (voir `ui::topbar::note_rect`).
        // Mêmes trois règles que la cloche juste au-dessus, et pour les mêmes
        // raisons : gardée par `clock_size()` (une horloge de largeur nulle la
        // poserait AU CENTRE, à une marge de la cloche qui y est aussi), pas
        // gardée par `features.clock` (éteindre l'horloge n'emporte pas le
        // rappel), et **elle clignote ici aussi** — la décision est relue depuis
        // `fetes::note_clignote`, que `app::run` publie à chaque frame.
        //
        // ⚠️ **ELLE EST LE SEUL SIGNAL DES RAPPELS DE NOTES SUR CET ÉCRAN** :
        // la cloche ne les couvre plus (voir `app::run`), donc l'oublier ici
        // rendrait une alarme de note parfaitement MUETTE sur un écran
        // secondaire — pas seulement discrète.
        // ⚠️ **`note_visible()` EN PLUS DU RÉGLAGE**, exactement comme la cloche
        // ci-dessus — et pour la même raison, qui était un défaut : sans elle,
        // l'icône resterait affichée en permanence sur cet écran, sans aucun
        // rappel de note à signaler.
        let note_instance = (self.features.rappel_notes && crate::win32::fetes::note_visible())
            .then(|| {
                self.clock_size().map(|(w, h)| {
                    let couleur = if crate::win32::fetes::note_clignote() {
                        let c = crate::ui::dock::notification_blink_color();
                        [c[0], c[1], c[2]]
                    } else {
                        effective_topbar_text_color
                    };
                    topbar::note_instance(
                        // Échelle 1.0 EN DUR : un écran secondaire ne magnifie jamais.
                        topbar::presented_note_rect(topbar::note_rect(&layout, w, h), 1.0, &layout),
                        couleur,
                        1.0,
                    )
                })
            })
            .flatten();
        if let Some(instance) = &note_instance {
            batches.push(QuadBatch::new(Some(&textures.note_rappel.srv), std::slice::from_ref(instance)));
        }

        // L'ICÔNE DU MINUTEUR — à droite de la cloche (voir `ui::topbar::
        // minuteur_rect`), sur CHAQUE barre comme la cloche, tant que quelque chose
        // sonne. Gardée par `clock_size()` pour la même raison ; échelle 1.0 en dur.
        let minuteur_topbar_instance = crate::win32::minuteur::sonne()
            .then(|| {
                self.clock_size().map(|(w, h)| {
                    let c = crate::ui::dock::notification_blink_color();
                    topbar::minuteur_instance(
                        topbar::presented_minuteur_rect(topbar::minuteur_rect(&layout, w, h), 1.0, &layout),
                        [c[0], c[1], c[2]],
                        1.0,
                    )
                })
            })
            .flatten();
        if let Some(instance) = &minuteur_topbar_instance {
            batches.push(QuadBatch::new(Some(&textures.minuteur.srv), std::slice::from_ref(instance)));
        }
        // L'APPLET DE MASQUAGE — sur CHAQUE barre (le masquage vaut pour tous les
        // écrans), à droite de l'emplacement du minuteur ; échelle 1.0 en dur.
        let masque_topbar_instance = self.clock_size().filter(|_| self.features.masque).map(|(w, h)| {
            topbar::minuteur_instance(
                topbar::presented_masque_rect(topbar::masque_rect(&layout, w, h), 1.0, &layout),
                effective_topbar_text_color,
                1.0,
            )
        });
        if let Some(instance) = &masque_topbar_instance {
            batches.push(QuadBatch::new(Some(&textures.masque.srv), std::slice::from_ref(instance)));
        }
        // LE VOYANT D'ENREGISTREMENT — sur CHAQUE barre, comme le minuteur : le
        // clic l'arrête depuis n'importe quel écran (voir `app::run`), il faut donc
        // le VOIR partout. Échelle 1.0 en dur.
        let rec_topbar_instance = crate::win32::enregistreur::en_cours()
            .then(|| {
                self.clock_size().map(|(w, h)| {
                    topbar::rec_instances(
                        topbar::presented_rec_rect(topbar::rec_rect(&layout, w, h), 1.0, &layout),
                        topbar::rec_color(crate::win32::enregistreur::duree().is_some()),
                        effective_topbar_text_color,
                    )
                })
            })
            .flatten();
        if let Some(instances) = &rec_topbar_instance {
            batches.push(QuadBatch::new(None, instances));
        }

        // Pastilles de bureaux virtuels (voir `ui::topbar::desktop_dot_
        // instance`, même fonction générique que la barre PRIMAIRE) —
        // DEMANDE EXPLICITE : "je voudrais que soit présents les pastilles
        // de bureaux virtuels [...] dans l'interface secondaire". Couleur
        // unie (pas de texture) : ajoutées à `batches` via `None` comme le
        // fond `plain_instance` plus haut.
        let desktop_dot_instances: Vec<QuadInstance> = if self.features.desktop_dots {
            virtual_desktops
                .map(|vd| {
                    (0..vd.count)
                        .map(|i| {
                            topbar::desktop_dot_instance(
                                &layout,
                                i,
                                i == vd.active_index,
                                background,
                                self.dark_mode,
                                // Cette copie-ci du dock a ses propres
                                // `features` (voir `app::resolve_secondary_
                                // features`) : le décalage doit être calculé
                                // avec LES SIENNES, sinon l'icône et les
                                // pastilles se recouvrent dès que l'écran
                                // secondaire a un réglage à lui.
                                crate::app::dots_left_offset(self.dock.slot_widths(), &layout, &self.features, top_left_offset),
                                self.dock.desktop_dot_size(),
                                // Échelle 1.0 EN DUR : un dock SECONDAIRE ne magnifie
                                // jamais, faute de souris primaire — voir
                                // `app::dot_hover_scales`, qui rend un tableau vide
                                // dans ce cas. Le rectangle au repos est donc celui
                                // qui est dessiné.
                                1.0,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        if !desktop_dot_instances.is_empty() {
            batches.push(QuadBatch::new(None, desktop_dot_instances.as_slice()));
        }

        // NUMÉRO des pastilles (voir `ui::desktop_labels`) — poussé JUSTE APRÈS
        // elles, donc par-dessus, et AVANT le reste de la barre. Le numéro est un
        // texte, donc un lot TEXTURÉ : une texture par chiffre, et l'instance
        // doit survivre jusqu'à la fin de la fonction pour que `batches` puisse
        // l'emprunter (même contrainte que l'horloge plus haut).
        //
        // Le rectangle vient de la pastille DÉJÀ CONSTRUITE (`dot.rect`, qui est
        // la boîte présentée) : recalculer le rectangle ici le ferait diverger
        // dès que la formule de placement des pastilles change.
        //
        // Le fond à juger pour le contraste est celui de la BARRE DU HAUT de CET
        // écran (`bar_color(0)`), pas la couleur partagée passée aux pastilles —
        // voir `ui::desktop_labels::color_for`, qui explique pourquoi il faut
        // composer la pastille translucide sur ce fond avant de choisir.
        let backdrop_rgb = {
            let bar = self.dock.bar_color(0);
            color::soften_tint([bar[0], bar[1], bar[2]], self.dark_mode)
        };
        let label_instances: Vec<(&Texture, QuadInstance)> = desktop_dot_instances
            .iter()
            .enumerate()
            .filter_map(|(i, dot)| {
                let (texture, w, h) = self.desktop_labels.texture(i)?;
                let active = virtual_desktops.map(|vd| i == vd.active_index).unwrap_or(false);
                let color = crate::ui::desktop_labels::color_for(active, background, self.dark_mode, backdrop_rgb);
                let font_px = crate::ui::desktop_labels::font_px(self.dock.desktop_dot_size(), i);
                let mut instance = QuadInstance::new_glyph(crate::ui::desktop_labels::rect_for(dot.rect, w, h, font_px), color);
                // MÊME rotation que la pastille : le numéro est POSÉ dessus, il
                // doit balayer avec elle et non rester droit pendant qu'elle
                // penche.
                instance.rotation = dot.rotation;
                Some((texture, instance))
            })
            .collect();
        for (texture, instance) in &label_instances {
            batches.push(QuadBatch::new(Some(&texture.srv), std::slice::from_ref(instance)));
        }

        // MARQUEURS DES SÉPARATEURS DE LA RANGÉE DE WIDGETS — la MÊME fonction
        // que la barre PRIMAIRE (`app::slot_marker_instances`), donc le repère et
        // son trait de survol sont au même endroit sur les deux.
        //
        // **Sans ce bloc, un séparateur posé sur un écran secondaire serait un
        // blanc SANS marqueur et SANS geste** : `DockInstance::render` ne le
        // dessinait pas, et c'était le défaut signalé (« ça ne devrait pas être
        // le cas, chaque interface doit avoir ses propres réglages sur chaque
        // écran »).
        //
        // `self.dock.mouse_pos()` : la position de la souris EST alimentée sur un
        // écran secondaire (voir `app::run`, `active_dock.on_mouse_move`), donc
        // le survol y fonctionne comme sur la primaire. La couleur et le diamètre
        // sont ceux résolus pour CETTE instance.
        let slot_marker_instances = crate::app::slot_marker_instances(
            self.dock.slot_widths(),
            &layout,
            &self.features,
            top_left_offset,
            self.media_block_dims(),
            separator_size,
            separator_color,
            self.dock.mouse_pos(),
        );
        if !slot_marker_instances.is_empty() {
            batches.push(QuadBatch::new(None, slot_marker_instances.as_slice()));
        }

        // Clipboard viewer (left bar)
        let clipboard_viewer_render_instance = self.dock.external_glyph_instance(
            CLIPBOARD_VIEWER_ICON_INDEX,
            &layout,
            effective_topbar_text_color,
        );
        if let Some(instance) = &clipboard_viewer_render_instance {
            push_icon_batch(&mut batches, &textures.clipboard, instance, clipped_for(CLIPBOARD_VIEWER_ICON_INDEX));
        }

        // Pinned apps
        let pinned_render_instances: Vec<Option<crate::gfx::QuadInstance>> = (0..self.pinned_apps_count)
            .map(|i| {
                let icon_index = PINNED_ICON_BASE + i;
                if self.dock.icons[icon_index].external_texture {
                    self.dock.external_icon_instance(icon_index, &layout)
                } else {
                    None
                }
            })
            .collect();
        for (i, instance) in pinned_render_instances.iter().enumerate() {
            if let (Some(Some(texture)), Some(instance)) = (self.pinned_icon_textures.get(i), instance) {
                push_icon_batch(&mut batches, texture, instance, clipped_for(PINNED_ICON_BASE + i));
            }
        }

        // Drag preview icon (drawn last) — VOLONTAIREMENT NON DÉCOUPÉE, comme
        // dans `app::run` : elle suit le curseur, et le geste « déplacer une
        // icône vers la barre du haut » l'amène justement par-dessus.
        let drag_preview_instance = self.dock.drag_preview_instance(&layout, separator_color, separator_size);
        if let Some(instance) = &drag_preview_instance {
            batches.push(QuadBatch::new(None, std::slice::from_ref(instance)));
        }

        // Render batches to the render target
        let renderer = crate::gfx::QuadRenderer::new(device)?;
        // Les mêmes fondus que la barre principale (voir `ui::dock::bandes_de_fondu`).
        renderer.poser_fondu(crate::ui::dock::bandes_de_fondu(&layout));
        renderer.poser_ombres(ombres);
        renderer.render_at(
            ctx,
            &render_target_view,
            width as f32,
            height as f32,
            &batches,
            screen_origin,
        )?;

        Ok(shader_resource_view)
    }
}

/// Bundle of shared texture resources used by the dock UI.
///
/// These textures are loaded once at startup and are shared across all dock
/// instances (primary and secondary). They are passed to the render method
/// to avoid duplicating them in each instance.
pub struct DockTextures {
    pub plain: Texture,
    pub logo: Texture,
    /// Variante CIRCULAIRE du logo — voir `gfx::texture::CIRCLE_LOGO_PNG` et
    /// `ui::dock::logo_is_circle`. Les deux sont chargées en permanence : le
    /// choix se fait au dessin, donc basculer le réglage ne recharge rien.
    pub circle_logo: Texture,
    pub textured: Texture,
    pub power: Texture,
    pub terminal: Texture,
    pub sound: Texture,
    pub clipboard: Texture,
    pub notes: Texture,
    pub settings: Texture,
    pub network: Texture, // Added network texture
    // `clock`/`sys_monitor`/`refresh_rate`/`media_title` ont été retirés
    // d'ici : c'était un bundle PARTAGÉ construit une seule fois à partir
    // de la taille de la barre PRIMAIRE (voir `app::run`), donc ces
    // bitmaps texte ne suivaient jamais l'épaisseur propre d'un écran
    // secondaire — BUG RÉEL SIGNALÉ ("les textes ... de la barre
    // supérieure des écrans secondaires ne suivent pas la taille de la
    // barre"). Chaque `DockInstance` régénère maintenant les siens via
    // `sync_text`/`CachedText`, à sa propre taille de police.
    pub media_previous: Texture,
    pub media_play_pause: Texture,
    pub media_next: Texture,
    /// Icône « afficher le bureau » (voir `ui::topbar::show_desktop_rect`) —
    /// widget de la barre du haut, contrairement aux icônes de dock ci-dessus.
    /// Elle est dessinée sur les écrans SECONDAIRES aussi, comme les pastilles
    /// de bureaux virtuels : le test de clic, lui, est résolu par écran dans
    /// `app::run`, et une icône non dessinée y laisserait une zone cliquable
    /// INVISIBLE.
    pub show_desktop: Texture,
    /// **La CLOCHE DE RAPPEL** (voir `ui::topbar::bell_rect`) — widget de barre du
    /// haut, comme `show_desktop` juste au-dessus, et dessinée sur les écrans
    /// secondaires pour la même raison (une cloche non dessinée y laisserait une
    /// zone cliquable INVISIBLE). DEMANDE EXPLICITE de l'utilisateur : « sur tous
    /// les écrans, comme l'horloge ».
    ///
    /// Sa COULEUR, elle, ne vient pas d'ici : elle clignote ou non selon
    /// `fetes::cloche_clignote`, publié chaque frame par `app::run` (voir cette
    /// cellule, qui porte le pourquoi).
    pub bell: Texture,
    /// **L'ICÔNE DE NOTE** (voir `ui::topbar::note_rect`) — widget de barre du
    /// haut, à GAUCHE de l'horloge, dessiné sur les écrans secondaires pour la
    /// même raison que la cloche juste au-dessus.
    ///
    /// ⚠️ **C'est le MÊME PNG que l'applet Notes de la barre**
    /// (`gfx::texture::NOTES_ICON_PNG`) : une seule image à embarquer, et une
    /// seule à remplacer si l'icône change.
    ///
    /// Sa COULEUR ne vient pas d'ici non plus : `fetes::note_clignote`, publié
    /// chaque frame par `app::run`.
    pub note_rappel: Texture,
    /// Icône « basculer le thème clair/sombre de Windows » (voir
    /// `ui::dock::Dock::new`, `win32::theme::toggle_dark_mode`) — glyphe DÉDIÉ
    /// (voir `TopbarIcons/SOURCE.md`), comme `notes`.
    ///
    /// Elle vit par défaut dans la barre de DROITE — **qu'un écran secondaire
    /// n'a pas** (`app::run` lui passe une liste d'ordre vide) : elle n'y est
    /// donc pas dessinée. Le bloc de `DockInstance::render` qui la traite sert
    /// au cas où l'utilisateur l'a GLISSÉE dans une barre que les écrans
    /// secondaires ont, comme pour les autres icônes de fonctionnalité.
    pub theme: Texture,
    /// Glyphe « éclairage nocturne » (voir `win32::night_light`) — même famille
    /// que la bascule de thème ci-dessus : une texture dédiée, teintée au dessin.
    pub night_light: Texture,
    /// Glyphe « file d'impression » (voir `win32::printer`) — même famille : une
    /// texture dédiée, dessinée pour cette fonctionnalité, teintée au dessin.
    pub printer: Texture,
    /// Glyphe « calendrier » (voir `win32::fetes`) — même famille : une texture
    /// dédiée, dessinée pour cette fonctionnalité, teintée au dessin.
    pub calendar: Texture,
    /// Glyphe « éditeur d'image » (voir `win32::cef_image_editor`) — même famille.
    pub image_editor: Texture,
    /// Glyphe « capture d'écran » (voir `win32::capture_ecran`) — même famille.
    pub screen_capture: Texture,
    /// Glyphe « minuteur » (voir `win32::minuteur`) — même famille.
    pub minuteur: Texture,
    /// Icône de l'applet de masquage, du thème d'icônes (voir `ui::icones`).
    pub masque: Texture,
    /// Glyphe « surveillance » (voir `win32::surveillance`) — même famille.
    pub surveillance: Texture,
    /// Glyphe « Windows Update » (voir `win32::windows_update`) — même famille.
    pub windows_update: Texture,
    /// Glyphe « Gestionnaire de périphériques » (voir
    /// `win32::gestionnaire_peripheriques`) — même famille.
    pub device_manager: Texture,
    /// Glyphe « Réglages de l'écran » (voir `win32::reglages_ecran`) — même famille.
    pub display_settings: Texture,
    /// Glyphe « calculatrice » (voir `win32::calculatrice`) — même famille.
    pub calculatrice: Texture,
    /// Glyphe « serveurs SSH » (voir `win32::cef_ssh`) — même famille.
    pub ssh: Texture,
    /// Glyphe « sélecteur de couleurs » (voir `win32::cef_couleurs`) — même famille.
    pub couleurs: Texture,
    /// Glyphe « Fonctionnalités facultatives » (voir
    /// `win32::fonctionnalites_facultatives`) — même famille.
    pub optional_features: Texture,
    /// Glyphe « Notifications » (voir `win32::centre_notifications`) — même famille.
    pub notifications: Texture,
    /// Glyphe « Recherche » (voir `win32::cef_recherche`) — même famille.
    pub recherche: Texture,
    /// Glyphe « Automatisations » (voir `win32::cef_automatisations`) — même famille.
    pub automatisations: Texture,
    /// Glyphe « Météo » (voir `win32::cef_meteo`) — même famille.
    pub meteo: Texture,
    /// ⭐ L'AIDE (voir `win32::cef_aide`).
    pub aide: Texture,
    /// Glyphe « Verrouiller la session » (voir
    /// `win32::power::verrouiller_avec_confirmation`) — même famille.
    pub verrouillage: Texture,
    /// Glyphe « Caractères français » (voir `win32::caracteres`) — même famille.
    pub caracteres: Texture,
    /// Glyphe « Kaomojis » (voir `win32::kaomoji`) — même famille.
    pub kaomoji: Texture,
    /// Glyphe « Propriétés système » (voir `win32::proprietes_systeme`) — même famille.
    pub proprietes_systeme: Texture,
    /// Glyphe « Scanner » (voir `win32::scanner`) — même famille.
    pub scanner: Texture,
    /// Glyphe « Éditeur de shaders » (voir `win32::cef_shaders`) — même famille.
    pub shaders: Texture,
    /// Glyphe « Symboles phonétiques » (voir `win32::cef_phonetique`) — même famille.
    pub phonetique: Texture,
    /// Glyphe « Symboles mathématiques » (voir `win32::cef_maths`) — même famille.
    pub maths: Texture,
    /// Glyphe « Éditeur des lueurs » (voir `win32::cef_lueurs`) — même famille.
    pub lueurs: Texture,
    /// Glyphe « Éditeur des textures » (voir `win32::cef_textures`) — même famille.
    pub textures: Texture,
    /// Glyphe « Narrateur » (voir `win32::narrateur`) — même famille.
    pub narrateur: Texture,
    /// Glyphe « Loupe » (voir `win32::loupe_windows`) — même famille.
    pub loupe: Texture,
    /// Glyphe « Moniteur système » (voir `win32::cef_moniteur`) — même famille.
    pub moniteur: Texture,
    pub observateur_evenements: Texture,
    pub enregistrer: Texture,
    pub clavier: Texture,
    pub jeu2048: Texture,
    pub cinq_ou_plus: Texture,
    pub quadrapassel: Texture,
    pub lightsoff: Texture,
}

/// Bundle of rendered bar background textures.
///
/// These are generated each frame from the dock's background colors and
/// the current blur effect.
pub struct BarBackdrops {
    pub top: Texture,
    pub left: Texture,
    pub right: Texture,
    /// ⚠️ **Vaut une texture de 1 px sur un écran SECONDAIRE**, dont la barre du
    /// bas a une épaisseur nulle (voir `BarThicknesses::derive_for_screen`) :
    /// `capture_bar_backdrops` ramène les rectangles dégénérés à 1 px pour que la
    /// capture réussisse — sans quoi l'échec priverait TOUTES les barres de cet
    /// écran de leur fond. C'est l'INSTANCE de dessin qui reste dégénérée, donc
    /// rien n'est peint dans cette bande.
    pub bottom: Texture,
    /// Le fond des CONGÉS de la zone libre (voir `ui::dock::conges`), un par
    /// congé et dans le même ordre — vide sans coins arrondis.
    pub conges: Vec<Texture>,
    /// Le fond des BOUTS de barre du bas (voir `ui::dock::bouts_de_barre_du_bas`),
    /// un par bout et dans le même ordre — vide sans colonne masquée.
    pub bouts: Vec<Texture>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les applets, DANS L'ORDRE de `Dock::new` — c'est la table de
    /// résolution de tout ce fichier, et de `apply_topbar_features`.
    const APPLETS: [&str; 46] = [
        "alimentation",
        "terminal",
        "son",
        "reseau",
        "voicemeeter",
        "notes",
        "theme",
        "eclairage nocturne",
        "file d'impression",
        "calendrier",
        "editeur d'image",
        "capture d'ecran",
        "minuteur",
        "surveillance",
        "windows update",
        "gestionnaire de périphériques",
        "réglages de l'écran",
        "calculatrice",
        "serveurs ssh",
        "sélecteur de couleurs",
        "fonctionnalités facultatives",
        "notifications",
        "recherche",
        "automatisations",
        "météo",
        "verrouiller la session",
        "caractères français",
        "kaomojis",
        // ⭐ L'AIDE (2026-10-07) — EN FIN DE LISTE, parce qu'elle est en fin de
        // chaîne d'index (voir `app::run`). ⚠️ **L'ORDRE DE CETTE LISTE EST CELUI
        // DES INDEX** : la rallonger ailleurs qu'à la fin ferait désigner la
        // mauvaise applet par chaque rang.
        "aide",
        // ⭐ LES PROPRIÉTÉS SYSTÈME (2026-10-08) — même règle, EN FIN.
        "proprietes_systeme",
        // ⭐ LE SCANNER (2026-10-09) — même règle, EN FIN.
        "scanner",
        // ⭐ L'ÉDITEUR DE SHADERS (2026-10-10) — même règle, EN FIN.
        "shaders",
        // ⭐ LES SYMBOLES PHONÉTIQUES (2026-10-11) — même règle, EN FIN.
        "phonetique",
        // ⭐ LES SYMBOLES MATHÉMATIQUES (2026-10-12) — même règle, EN FIN.
        "maths",
        // ⭐ L'ÉDITEUR DES LUEURS (2026-10-14) — même règle, EN FIN.
        "lueurs",
        // ⭐ L'ÉDITEUR DES TEXTURES (2026-10-15) — même règle, EN FIN.
        "textures",
        // ⭐ LE NARRATEUR (2026-10-15) — même règle, EN FIN.
        "narrateur",
        // ⭐ LA LOUPE (2026-10-19) — même règle, EN FIN.
        "loupe",
        // ⭐ LE MONITEUR SYSTÈME (2026-10-20) — même règle, EN FIN.
        "moniteur",
        "observateur_evenements",
        // ENREGISTRER (2026-10-31) — même règle, EN FIN.
        "enregistrer",
        // CLAVIER VISUEL (2026-11-07) — même règle, EN FIN.
        "clavier",
        // 2048 (2026-11-07) — même règle, EN FIN.
        "jeu2048",
        // CINQ OU PLUS (2026-11-07) — même règle, EN FIN.
        "cinq_ou_plus",
        // QUADRAPASSEL (2026-11-07) — même règle, EN FIN.
        "quadrapassel",
        // LIGHTSOFF (2026-11-08) — même règle, EN FIN.
        "lightsoff",
    ];

    /// **LE TEST QUI AURAIT ATTRAPÉ LE DÉFAUT DE DISPLAY2 (2026-09-23).**
    ///
    /// Un dock SECONDAIRE doit vivre dans l'ESPACE D'INDEX DU PRIMAIRE : c'est
    /// ce qu'assument TOUS les sites qui adressent ses applets —
    /// `apply_topbar_features` (appelée ici même, puis rappelée à chaque frame
    /// par `app::run`), `DockInstance::render` juste plus haut, et le
    /// `set_icon_order` par défaut de `rebuild_secondary_docks`. Tous calculent
    /// `PINNED_ICON_BASE + pinned_apps_count + k`, sur `pinned_apps.len()`.
    ///
    /// Tant que `DockInstance::new` valait `pinned_count = 0`, les deux espaces
    /// divergeaient de `pinned_apps.len()` : sur DISPLAY2, l'index 9 (réseau)
    /// tombait sur `theme` et l'index 7 (terminal) sur `voicemeeter` — le défaut
    /// rapporté, « chaque applet porte l'icône de sa voisine ».
    ///
    /// MUTATION (faite, et le test tombe dans les deux cas) : repasser `0` dans
    /// `DockInstance::new` ; retirer la boucle de masquage des épingles.
    #[test]
    fn un_dock_secondaire_a_l_espace_d_index_du_primaire() {
        const PINS: usize = 3;
        let instance = DockInstance::new(PINS);

        assert_eq!(instance.pinned_apps_count, PINS);

        // La place des épingles est RÉSERVÉE — c'est elle qui fait tenir
        // l'espace d'index — mais elle n'occupe aucun créneau : un écran
        // secondaire ne réplique pas les épingles de la barre des tâches.
        for index in PINNED_ICON_BASE..PINNED_ICON_BASE + PINS {
            assert!(instance.dock.icons[index].hidden, "l'épingle {index} doit être masquée (réservée, sans créneau)");
        }

        let base = PINNED_ICON_BASE + PINS;
        for (k, nom) in APPLETS.iter().enumerate() {
            let icon = &instance.dock.icons[base + k];
            assert!(icon.external_texture, "l'applet {nom} (index {}) doit porter un glyphe", base + k);
            assert!(!icon.is_spacer, "l'applet {nom} (index {}) ne doit pas être un espace", base + k);
            assert!(!icon.hidden, "l'applet {nom} (index {}) doit être visible par défaut", base + k);
        }

        // Et RIEN APRÈS : sans cette borne, une place ajoutée en trop passerait
        // inaperçue, et le prochain `power + 8` désignerait un espace.
        assert_eq!(instance.dock.icons.len(), base + APPLETS.len(), "un dock secondaire s'arrête à ses huit applets");
    }

    /// **UN RÉGLAGE PAR ÉCRAN DOIT ATTEINDRE SON APPLET, PAS SA VOISINE.**
    ///
    /// Reproduit le défaut RÉEL de DISPLAY2 : son
    /// `secondary_screen_features.txt` porte `network`, donc `network` est
    /// basculé par rapport au défaut secondaire tandis que `terminal` y reste
    /// (`SECONDARY_DEFAULT_DISABLED_FEATURES`). Avec l'ancien espace d'index,
    /// ces deux réglages masquaient `voicemeeter` et révélaient `theme`.
    #[test]
    fn un_reglage_par_ecran_atteint_la_bonne_applet() {
        const PINS: usize = 3;
        let mut instance = DockInstance::new(PINS);
        let base = PINNED_ICON_BASE + PINS;

        // Littéral EXPLICITE, et pas un `..Default::default()` : `Features` n'a
        // pas de `Default`, précisément pour que tout champ ajouté plus tard soit
        // signalé ici par le compilateur au lieu d'être oublié en silence.
        let features = Features {
            power: true,
            terminal: false,
            sound: true,
            network: true,
            voicemeeter: true,
            media: false,
            clock: false,
            desktop_dots: false,
            dash: false,
            clipboard_viewer: true,
            notes: true,
            refresh_rate: false,
            sys_monitor: false,
            show_desktop: false,
            bell: false,
            rappel_notes: true,
            masque: true,
            reglages_secours: false,
            theme_toggle: true,
            night_light: true,
            printer: true,
            calendar: true,
            image_editor: true,
            screen_capture: true,
            minuteur: true,
            surveillance: true,
            windows_update: true,
            device_manager: true,
            display_settings: true,
            calculatrice: true,
            ssh: true,
            couleurs: true,
            optional_features: true,
            notifications: true,
            recherche: true,
            automatisations: true,
            meteo: true,
            aide: true,
            verrouillage: true,
            caracteres: true,
            kaomoji: true,
            proprietes_systeme: true,
            scanner: true,
            shaders: true,
            phonetique: true,
            maths: true,
            lueurs: true,
            textures: true,
            narrateur: true,
            loupe: true,
            moniteur: true,
            observateur_evenements: true,
            enregistrer: true,
            clavier: true,
            jeu2048: true,
            cinq_ou_plus: true,
            quadrapassel: true,
            lightsoff: true,
        };
        crate::app::apply_topbar_features(&mut instance.dock, PINS, &features);

        assert!(instance.dock.icons[base + 1].hidden, "terminal éteint doit masquer LE TERMINAL, index {}", base + 1);
        assert!(!instance.dock.icons[base + 3].hidden, "réseau allumé doit laisser LE RÉSEAU visible, index {}", base + 3);
        // Les applets voisines ne doivent PAS bouger : c'est exactement ce que
        // l'ancien espace d'index faisait, et ce que l'utilisateur voyait.
        for k in [0usize, 2, 4, 5, 6, 7] {
            assert!(
                !instance.dock.icons[base + k].hidden,
                "{} (index {}) ne doit pas être touchée par le réglage d'une autre applet",
                APPLETS[k],
                base + k
            );
        }
    }
}