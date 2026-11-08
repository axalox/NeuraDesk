//! Chargement d'une image PNG embarquée dans l'exécutable et upload en
//! texture Direct3D 11, pour afficher une vraie icône (le logo NeuraDesk)
//! plutôt qu'un simple carré de couleur.

use windows::core::Result;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11SamplerState, ID3D11ShaderResourceView, D3D11_BIND_SHADER_RESOURCE,
    D3D11_COMPARISON_NEVER, D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_SAMPLER_DESC,
    D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_USAGE_IMMUTABLE,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC};

/// PNG du logo NeuraDesk (dossier `NeuraDeskIcon/` du projet), embarqué dans
/// le binaire pour ne dépendre d'aucun fichier externe au runtime.
/// `pub(crate)` : réutilisé tel quel par `win32::dash` (pastille "accueil"
/// du Dash) — même image source, décodée une seconde fois côté GDI plutôt
/// que réutilisée depuis la texture D3D déjà chargée ici, puisque cette
/// fenêtre-là n'a pas de pipeline Direct3D (voir sa doc de module).
pub(crate) const LOGO_PNG: &[u8] = include_bytes!("../../NeuraDeskIcon/NeuraDesk-128.png");

/// Variante CIRCULAIRE du logo — DEMANDE EXPLICITE : « avoir le choix entre
/// l'icône carré avec un fond respirant et l'icône cercle qui tourne ». Le dock
/// montre l'une ou l'autre, jamais les deux (voir `ui::dock::logo_is_circle`).
///
/// **LE MASQUE CIRCULAIRE EST DANS L'IMAGE, PAS DANS LE RENDU** : la source de
/// l'utilisateur était un carré à coins très arrondis, et en tournant ce sont
/// ses COINS qu'on aurait vus pivoter. Le détourage est donc fait une fois, à la
/// fabrication de l'asset (voir le script de conversion) — le rendu n'a rien à
/// savoir de tout ça, et il n'y a pas de masque à appliquer par frame.
pub(crate) const CIRCLE_LOGO_PNG: &[u8] = include_bytes!("../../NeuraDeskIcon/NeuraDesk-cercle-128.png");

/// Icônes de la barre du haut/dock (réglages, presse-papiers, son, réseau,
/// alimentation, terminal, notes, afficher le bureau) — rastérisations PNG d'icônes Iconsax
/// (style Outline ; l'alimentation vient de Keyline Icons, comme la cloche), remplaçant les glyphes Segoe MDL2 Assets d'origine
/// (voir `TopbarIcons/SOURCE.md` pour la provenance/attribution exacte de
/// chacune). Chargées comme `LOGO_PNG` ci-dessus via `from_png_bytes` :
/// seul le canal alpha compte pour le rendu (voir `QuadInstance::new_glyph`,
/// `gfx::quad_renderer::TexMode::Glyph`), la couleur affichée vient de la
/// teinte du quad, pas du contenu RGB de ces PNG.
pub(crate) const SETTINGS_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/settings.png");
pub(crate) const CLIPBOARD_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/clipboard.png");
pub(crate) const SOUND_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/sound.png");
pub(crate) const NETWORK_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/network.png");
pub(crate) const POWER_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/power.png");
pub(crate) const TERMINAL_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/terminal.png");
pub(crate) const NOTES_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/notes.png");
/// Icône « basculer le thème clair/sombre de Windows » (voir
/// `win32::theme::toggle_dark_mode`) — DEMANDE EXPLICITE. **Ce n'est PAS un
/// glyphe Iconsax comme les huit ci-dessus** : aucun rastériseur SVG n'est
/// disponible sur la machine de développement, le disque coupé a donc été
/// DESSINÉ (voir `TopbarIcons/SOURCE.md`, qui documente chaque origine). Sans
/// conséquence sur le rendu : seul le canal alpha est échantillonné, donc un
/// glyphe dessiné vaut un glyphe rastérisé.
pub(crate) const THEME_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/theme.png");
/// Icône « basculer l'éclairage nocturne de Windows » (voir
/// `win32::night_light::toggle`) — DEMANDE EXPLICITE : « une applet pour commuter le
/// mode nuit de Windows ». Comme la bascule de thème et pour la même raison, **ce
/// n'est pas un glyphe Iconsax** : il est DESSINÉ (voir `TopbarIcons/SOURCE.md`), un
/// croissant obtenu en retirant un second disque. Sans conséquence sur le rendu :
/// seul le canal alpha est échantillonné.
pub(crate) const NIGHT_LIGHT_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/night_light.png");

/// Icône « file d'impression de Windows » (voir `win32::printer`) — DEMANDE EXPLICITE :
/// « ajoute un applet pour ouvrir le gestionnaire de file d'impression de windows ».
/// Comme la bascule de thème et l'éclairage nocturne, **ce n'est pas un glyphe Iconsax** :
/// il est DESSINÉ (voir `TopbarIcons/SOURCE.md`), et le générateur est la sonde
/// `dessine_le_glyphe_de_l_imprimante` — juste en dessous, dans ce fichier.
pub(crate) const PRINTER_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/printer.png");

/// Icône « scanner » — l'applet qui pilote NAPS2 (voir `win32::scanner`). Comme
/// l'imprimante juste au-dessus, **elle est DESSINÉE** et non rastérisée, et son
/// générateur est la sonde `dessine_le_glyphe_du_scanner`, plus bas dans ce
/// fichier.
///
/// ⚠️ **ELLE DOIT RESTER DISTINGUABLE DE L'IMPRIMANTE** — les deux sont des
/// appareils de bureau, et la règle du dépôt refuse deux icônes indiscernables
/// dans la barre. Ce qui les sépare : le plateau est plus BAS et plus LARGE, et la
/// fente est EN HAUT (là où le document entre) au lieu d'être en bas.
pub(crate) const SCANNER_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/scanner.png");

/// Icône « éditeur de shaders » (voir `win32::cef_shaders`) — les chevrons `code`
/// de Keyline Icons (MIT), rastérisés par resvg (voir `TopbarIcons/SOURCE.md`).
/// Monochrome : seul le canal alpha est échantillonné.
pub(crate) const SHADERS_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/shaders.png");

/// Icône « symboles phonétiques » (voir `win32::cef_phonetique`) — un schwa `ə`,
/// DESSINÉ en Segoe UI Gras (voir `tools/rasteriser_phonetique.py`). Monochrome.
pub(crate) const PHONETIQUE_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/phonetique.png");
/// Icône « symboles mathématiques » (voir `win32::cef_maths`) — une somme `∑`,
/// DESSINÉE en Segoe UI Gras (voir `tools/rasteriser_maths.py`). Monochrome.
pub(crate) const MATHS_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/maths.png");
/// Icône « éditeur des lueurs » (voir `win32::cef_lueurs`) — un soleil, DESSINÉ
/// (voir `C:/ndwork/scripts/icones_lueurs.py`). Monochrome.
pub(crate) const LUEURS_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/lueurs.png");
/// Icône « éditeur des textures » (voir `win32::cef_textures`) — un carré veiné,
/// DESSINÉ (voir `C:/ndwork/scripts/icones_textures.py`). Monochrome.
pub(crate) const TEXTURES_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/textures.png");
/// Icône « Narrateur » (voir `win32::narrateur`) — une tête de profil qui parle,
/// DESSINÉE (voir `C:/ndwork/scripts/icones_narrateur.py`). Monochrome.
pub(crate) const NARRATEUR_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/narrateur.png");
/// Icône « Loupe » (voir `win32::loupe_windows`) — une loupe marquée d'un « + »,
/// DESSINÉE (voir `C:/ndwork/scripts/icones_loupe.py`). Monochrome.
pub(crate) const LOUPE_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/loupe.png");
/// Icône « Moniteur système » (voir `win32::cef_moniteur`) — un cadre traversé
/// d'un tracé de pouls, DESSINÉ (voir `C:/ndwork/scripts/icones_moniteur.py`).
pub(crate) const MONITEUR_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/moniteur.png");

/// Icône « calendrier » — l'applet de la zone Intégrés, et la fenêtre qu'elle
/// ouvre (voir `win32::fetes`, qui porte les données qu'elle affiche). Comme les
/// trois précédentes, **elle est DESSINÉE** et non rastérisée : le générateur est
/// la sonde `dessine_les_glyphes_du_calendrier` (voir `TopbarIcons/SOURCE.md`).
pub(crate) const CALENDAR_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/calendar.png");
/// Icône « éditeur d'image » — l'applet de la zone Intégrés qui ouvre
/// `win32::cef_image_editor`. DESSINÉE, comme les précédentes : le générateur est la
/// sonde `dessine_le_glyphe_de_l_editeur_d_image` (voir `TopbarIcons/SOURCE.md`).
pub(crate) const IMAGE_EDITOR_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/image_editor.png");
/// Icône « capture d'écran » — l'applet qui fige l'écran (voir
/// `win32::capture_ecran`). RASTÉRISÉE, comme l'alimentation : `scan` de Keyline
/// Icons (MIT), les quatre coins d'une sélection (voir `TopbarIcons/SOURCE.md`).
pub(crate) const SCREEN_CAPTURE_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/screen_capture.png");
/// Icône « Enregistrer » — une caméra vidéo, Keyline Icons `video` (MIT),
/// même recette que la capture d'écran (voir `TopbarIcons/SOURCE.md`).
pub(crate) const ENREGISTRER_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/enregistrer.png");
/// Icône « 2048 » — le symbolique de GNOME 2048 (voir `TopbarIcons/SOURCE.md`). Monochrome.
pub(crate) const JEU2048_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/jeu2048.png");
/// Icône « Cinq ou plus » — le symbolique de GNOME Five or More. Monochrome.
pub(crate) const CINQ_OU_PLUS_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/cinq_ou_plus.png");
pub(crate) const QUADRAPASSEL_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/quadrapassel.png");
/// Icône « Lights Off » — le symbolique de GNOME Lights Off (l'interrupteur). Monochrome.
pub(crate) const LIGHTSOFF_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/lightsoff.png");
/// Icône « minuteur » — comptes à rebours et alarmes (voir `win32::minuteur`).
/// RASTÉRISÉE depuis Keyline Icons (MIT), comme la capture d'écran (voir
/// `TopbarIcons/SOURCE.md`).
pub(crate) const MINUTEUR_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/minuteur.png");
/// Icône « surveillance » — ping et HTTP de serveurs (voir `win32::surveillance`).
/// RASTÉRISÉE depuis Keyline Icons (MIT), comme le minuteur (voir
/// `TopbarIcons/SOURCE.md`).
pub(crate) const SURVEILLANCE_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/surveillance.png");
/// Icône « Windows Update » — ouvre la page des Paramètres (voir
/// `win32::windows_update`). RASTÉRISÉE depuis Keyline Icons (MIT), comme la
/// surveillance (voir `TopbarIcons/SOURCE.md`).
pub(crate) const WINDOWS_UPDATE_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/windows_update.png");
/// Icône « Gestionnaire de périphériques » — ouvre la console MMC (voir
/// `win32::gestionnaire_peripheriques`). RASTÉRISÉE depuis Keyline Icons (MIT),
/// comme Windows Update (voir `TopbarIcons/SOURCE.md`).
pub(crate) const DEVICE_MANAGER_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/gestionnaire_peripheriques.png");
/// Icône « Réglages de l'écran » — ouvre la page Écran des Paramètres (voir
/// `win32::reglages_ecran`). RASTÉRISÉE depuis Keyline Icons (MIT), comme
/// Windows Update (voir `TopbarIcons/SOURCE.md`).
pub(crate) const DISPLAY_SETTINGS_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/reglages_ecran.png");
/// Icône « calculatrice » (voir `win32::calculatrice`). RASTÉRISÉE depuis
/// Keyline Icons (MIT), comme les réglages de l'écran (voir `TopbarIcons/SOURCE.md`).
pub(crate) const CALCULATRICE_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/calculatrice.png");
/// Icône « serveurs SSH » (voir `win32::cef_ssh`). RASTÉRISÉE depuis Keyline
/// Icons (MIT), comme la calculatrice (voir `TopbarIcons/SOURCE.md`).
pub(crate) const SSH_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/ssh.png");
/// Icône « sélecteur de couleurs » — une PIPETTE, l'applet de la zone Intégrés
/// qui ouvre `win32::cef_couleurs`. DESSINÉE, comme l'éditeur d'image : le
/// générateur est la sonde `dessine_le_glyphe_de_la_pipette`, et c'est son
/// ASCII qui a fait corriger deux fois la silhouette (une hache, puis une
/// déchirure de 4,7 px au raccord du bulbe).
pub(crate) const COULEURS_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/couleurs.png");
/// Icône « Fonctionnalités facultatives » — ouvre la page des Paramètres (voir
/// `win32::fonctionnalites_facultatives`). RASTÉRISÉE depuis Keyline Icons (MIT),
/// comme les réglages de l'écran (voir `TopbarIcons/SOURCE.md`).
pub(crate) const OPTIONAL_FEATURES_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/fonctionnalites_facultatives.png");
/// Icône « Notifications » — ouvre le centre de notifications (voir
/// `win32::centre_notifications`). RASTÉRISÉE depuis Keyline Icons (MIT) — une
/// BULLE, pas une cloche : la cloche est déjà celle des rappels (voir
/// `TopbarIcons/SOURCE.md`).
pub(crate) const NOTIFICATIONS_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/notifications.png");
/// Icône « Recherche » — ouvre le dash de recherche (voir `win32::cef_recherche`).
/// RASTÉRISÉE depuis Keyline Icons (MIT) — un FICHIER sous la loupe (voir
/// `TopbarIcons/SOURCE.md`).
pub(crate) const RECHERCHE_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/recherche.png");
/// Icône « Automatisations » — ouvre le dash des automatisations « quand…
/// alors… » (voir `win32::cef_automatisations`). RASTÉRISÉE depuis Keyline Icons
/// (MIT) — un chemin à embranchement, qui dit « d'ici vers là » (voir
/// `TopbarIcons/SOURCE.md`).
pub(crate) const AUTOMATISATIONS_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/automatisations.png");
/// Icône « Aide » — ouvre le dash du mode d'emploi (voir `win32::cef_aide`).
/// RASTÉRISÉE depuis Keyline Icons (MIT) : `message-question`, style `stroke` —
/// une bulle portant un « ? » (voir `TopbarIcons/SOURCE.md`).
pub(crate) const AIDE_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/aide.png");
/// Icône « Météo » — ouvre le dash des prévisions Open-Meteo (voir
/// `win32::cef_meteo`). RASTÉRISÉE depuis Keyline Icons (MIT) — un SOLEIL derrière
/// un NUAGE (`cloud-sun`, voir `TopbarIcons/SOURCE.md`).
pub(crate) const METEO_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/meteo.png");
/// Icône « Verrouiller la session » — verrouille la session Windows (voir
/// `win32::power::verrouiller_avec_confirmation`). RASTÉRISÉE depuis Keyline Icons
/// (MIT) — un CADENAS FERMÉ (voir `TopbarIcons/SOURCE.md`).
pub(crate) const VERROUILLAGE_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/verrouillage.png");
/// Icône « Caractères » — ouvre la fenêtre flottante des caractères accentués (voir
/// `win32::caracteres`). **DESSINÉE, PAS CHOISIE** : une lettre « é », parce
/// qu'aucun pictogramme ne dit « accents » en une image (voir
/// `tools/rasteriser_caracteres.py`).
pub(crate) const CARACTERES_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/caracteres.png");
/// Icône « Kaomojis » — ouvre la fenêtre flottante des kaomojis (voir
/// `win32::kaomoji`). **DESSINÉE, PAS CHOISIE** : un visage `^_^`, le plus court des
/// kaomojis, qui dit exactement ce que la fenêtre contient — là où une emoji dirait
/// « émoticône », qui est l'autre onglet du panneau de Windows (voir
/// `tools/rasteriser_kaomoji.py`).
pub(crate) const KAOMOJI_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/kaomoji.png");
/// Icône « Propriétés système » — ouvre `sysdm.cpl` (voir
/// `win32::proprietes_systeme`). RASTÉRISÉE depuis Keyline Icons (MIT) :
/// `sliders-horizontal`, trois CURSEURS — le vocabulaire des propriétés, et
/// **délibérément distinct de l'engrenage de `SETTINGS_ICON_PNG`** (voir
/// `TopbarIcons/SOURCE.md`).
pub(crate) const PROPRIETES_SYSTEME_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/proprietes_systeme.png");
/// **La CLOCHE** — l'indicateur de rappel de la barre du haut, à droite de
/// l'horloge (DEMANDE EXPLICITE : « une icône de cloche clignotante dans un
/// cycle de couleurs placée à droite de l'horloge dans les barres du haut »).
///
/// Elle est dessinée par la MÊME sonde que le calendrier, et pour la même
/// raison. ⚠️ **C'est la seule icône du dépôt dont la COULEUR ne vient pas du
/// thème** : elle clignote, donc elle est teintée par le cycle de
/// `ui::dock::notification_blink_color` (voir `ui::topbar::bell_instance`).
pub(crate) const BELL_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/bell.png");
/// Icône "afficher le bureau" — widget PROPRE À LA BARRE DU HAUT (au sens de
/// `ui::topbar`), et non une `Dock::Icon` comme les sept ci-dessus : elle est
/// dessinée par `ui::topbar::show_desktop_instance`, à l'extrémité gauche de la
/// barre (voir `topbar::SHOW_DESKTOP_MARGIN_LEFT`).
pub(crate) const SHOW_DESKTOP_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/monitor.png");

/// L'applet « Masquer l'interface » : l'œil BARRÉ (`ED1A` de Segoe Fluent
/// Icons) rastérisé au gabarit des autres icônes — le glyphe qu'elle dessinait
/// avant d'entrer dans les thèmes d'icônes (2026-10-12).
pub(crate) const MASQUE_ICON_PNG: &[u8] = include_bytes!("../../TopbarIcons/masque.png");

#[derive(Clone)]
pub struct Texture {
    pub srv: ID3D11ShaderResourceView,
}

impl Texture {
    /// Décode le logo NeuraDesk embarqué et l'upload en texture GPU.
    pub fn load_logo(device: &ID3D11Device) -> Result<Self> {
        Self::from_png_bytes(device, LOGO_PNG)
    }

    /// Décode la variante CIRCULAIRE du logo — voir `CIRCLE_LOGO_PNG`.
    pub fn load_circle_logo(device: &ID3D11Device) -> Result<Self> {
        Self::from_png_bytes(device, CIRCLE_LOGO_PNG)
    }

    /// Décode un PNG embarqué arbitraire et l'upload en texture GPU — même
    /// traitement que `load_logo`, mutualisé ici pour les icônes de
    /// `TopbarIcons/` ci-dessus (une seule implémentation de décodage PNG →
    /// RGBA8 → texture, au lieu d'une copie par appelant).
    pub fn from_png_bytes(device: &ID3D11Device, png: &[u8]) -> Result<Self> {
        let rgba = image::load_from_memory(png)
            .expect("icône embarquée invalide")
            .to_rgba8();
        let (width, height) = rgba.dimensions();
        Self::from_rgba(device, width, height, &rgba.into_raw())
    }

    /// **COMME `from_png_bytes`, MAIS REND `None` AU LIEU DE PANIQUER SUR UNE IMAGE
    /// ILLISIBLE** — la variante des images qui viennent de l'UTILISATEUR.
    ///
    /// ⛔ **CE N'EST PAS UN CONFORT, C'EST UN DÉFAUT MESURÉ — ET IL EXISTAIT AVANT
    /// L'ÉTAPE 4.** L'`.expect("icône embarquée invalide")` a été écrit pour les PNG
    /// embarqués dans le binaire, où l'échec est une erreur de programmation qu'on veut
    /// **bruyante**. Mais une vignette de dossier est un fichier fourni par
    /// l'utilisateur (`config::store_dossier_vignette`), et **l'appelant ne peut pas
    /// s'en protéger** : `app.rs` écrivait `Texture::from_png_bytes(..).ok()` — or le
    /// `.ok()` ne sert à rien, le `panic` est **à l'intérieur**. Avec
    /// `panic = "abort"` (voir `Cargo.toml`), une vignette corrompue tuait donc
    /// l'application **au lancement**, sans pile, sans journal, sans rien à lire.
    ///
    /// Le décodage est identique ; seule la sortie change de forme.
    pub fn from_png_bytes_souple(device: &ID3D11Device, png: &[u8]) -> Option<Self> {
        let rgba = image::load_from_memory(png).ok()?.to_rgba8();
        let (width, height) = rgba.dimensions();
        Self::from_rgba(device, width, height, &rgba.into_raw()).ok()
    }

    /// Upload une image RGBA8 déjà décodée en mémoire (4 octets/pixel,
    /// ordre R, G, B, A) vers une texture D3D11 immuable. Utilisé pour le
    /// logo (PNG décodé, voir `load_logo`) comme pour le texte rendu à la
    /// volée (l'horloge de la barre du haut, voir `gfx::text`).
    pub fn from_rgba(device: &ID3D11Device, width: u32, height: u32, rgba: &[u8]) -> Result<Self> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_IMMUTABLE,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let data = D3D11_SUBRESOURCE_DATA {
            pSysMem: rgba.as_ptr() as *const _,
            SysMemPitch: width * 4,
            SysMemSlicePitch: 0,
        };

        let mut texture = None;
        unsafe { device.CreateTexture2D(&desc, Some(&data), Some(&mut texture))? };
        let texture = texture.unwrap();

        let mut srv = None;
        unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut srv))? };

        Ok(Self { srv: srv.unwrap() })
    }
}

/// Sampler bilinéaire, bords "clamp" : adapté à une icône affichée à peu
/// près à sa résolution native, sans répétition aux bords.
pub fn create_sampler_state(device: &ID3D11Device) -> Result<ID3D11SamplerState> {
    let desc = D3D11_SAMPLER_DESC {
        Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
        MipLODBias: 0.0,
        MaxAnisotropy: 1,
        ComparisonFunc: D3D11_COMPARISON_NEVER,
        BorderColor: [0.0; 4],
        MinLOD: 0.0,
        MaxLOD: f32::MAX,
    };
    let mut sampler = None;
    unsafe { device.CreateSamplerState(&desc, Some(&mut sampler))? };
    Ok(sampler.unwrap())
}

#[cfg(test)]
mod tests {
    /// **GÉNÉRATEUR DU GLYPHE `printer.png` — une SONDE, pas un test de suite.**
    ///
    /// `#[ignore]` parce qu'elle ÉCRIT un fichier du dépôt : elle se lance à la main,
    /// quand le dessin change, exactement comme la sonde qui a produit
    /// `night_light.png` (voir `TopbarIcons/SOURCE.md`, qui porte la justification du
    /// dessin plutôt que du rastérissage — **aucun rastériseur SVG n'existe sur cette
    /// machine**, et le dépôt n'a pas de GDI+).
    ///
    /// Lancer : `cargo test --bins -- --ignored --nocapture dessine_le_glyphe`
    ///
    /// **SEUL LE CANAL ALPHA EST ÉCHANTILLONNÉ AU RENDU** (voir `TopbarIcons/SOURCE.md`
    /// et `ui::dock::external_glyph_instance`, qui teinte le glyphe) : le dessin est donc
    /// des formes BLANCHES pleines, et tout détail est un TROU (alpha 0), jamais une
    /// nuance. C'est ce qui oblige à séparer les éléments par des vides — en niveaux de
    /// gris ils se confondraient en une tache à la taille d'une icône de dock.
    ///
    /// Métriques **relevées sur les voisines**, pas choisies : canevas 128×128, cadre
    /// utile de 116×116 (6 px de marge de chaque côté).
    #[test]
    #[ignore = "ecrit TopbarIcons/printer.png — a lancer a la main quand le dessin change"]
    fn dessine_le_glyphe_de_l_imprimante() {
        const TAILLE: u32 = 128;
        /// Suréchantillonnage : sans lui, les bords arrondis sortent en escalier à
        /// 128 px, et le dock les réduit encore. On moyenne 4×4 sous-échantillons.
        const SS: u32 = 4;

        /// Rectangle ARRONDI : le point est dedans si son éloignement au rectangle
        /// « dégonflé » des coins reste sous le rayon.
        fn rond(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
            if x < x0 || x >= x1 || y < y0 || y >= y1 {
                return false;
            }
            let (cx, cy) = (x.clamp(x0 + r, x1 - r), y.clamp(y0 + r, y1 - r));
            let (dx, dy) = (x - cx, y - cy);
            dx * dx + dy * dy <= r * r
        }

        /// **LA FORME, EN UNE SEULE QUESTION — « ce point est-il plein ? »**
        ///
        /// L'ordre des tests EST le dessin : le corps d'abord, puis la feuille qui
        /// entre par le haut, puis la FENTE qui le traverse, et enfin la feuille qui
        /// sort — redessinée PAR-DESSUS la fente, ce qui ne laisse de celle-ci que les
        /// deux encoches de part et d'autre. C'est ce qui fait lire « imprimante »
        /// plutôt que « boîte ».
        fn plein(x: f32, y: f32) -> bool {
            // La feuille qui SORT, devant : elle recouvre la fente et le bas du corps.
            if (34.0..94.0).contains(&x) && (80.0..124.0).contains(&y) {
                return true;
            }
            // La feuille qui ENTRE, par le haut.
            if (32.0..96.0).contains(&x) && (10.0..46.0).contains(&y) {
                return true;
            }
            if rond(x, y, 8.0, 40.0, 120.0, 104.0, 14.0) {
                // **LA FENTE EST BASSE, ET C'EST TOUT CE QUI FAIT LIRE « IMPRIMANTE ».**
                // Au premier essai elle était à mi-hauteur du corps, et le rendu la
                // donnait pour deux trous flanquant une barre — on lisait une boîte
                // percée, pas une imprimante. Elle est maintenant LÀ OÙ LA FEUILLE
                // SORT, c'est-à-dire au ras du bas du corps : les deux encoches qui
                // restent de part et d'autre de la feuille disent alors la fente de
                // sortie, et non un détail au milieu de la façade.
                let fente = (16.0..112.0).contains(&x) && (76.0..88.0).contains(&y);
                return !fente;
            }
            false
        }

        let mut rgba = vec![0u8; (TAILLE * TAILLE * 4) as usize];
        for py in 0..TAILLE {
            for px in 0..TAILLE {
                let mut couverts = 0u32;
                for sy in 0..SS {
                    for sx in 0..SS {
                        let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                        let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                        if plein(x, y) {
                            couverts += 1;
                        }
                    }
                }
                let alpha = (couverts * 255 / (SS * SS)) as u8;
                let o = ((py * TAILLE + px) * 4) as usize;
                rgba[o] = 255;
                rgba[o + 1] = 255;
                rgba[o + 2] = 255;
                rgba[o + 3] = alpha;
            }
        }

        let png = crate::win32::clipboard::encode_png(TAILLE, TAILLE, &rgba).expect("encodage PNG");
        let chemin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("TopbarIcons").join("printer.png");
        std::fs::write(&chemin, &png).expect("ecriture du PNG");
        println!("ecrit {} ({} octets)", chemin.display(), png.len());
    }

    /// **LE GLYPHE DE L'APPLET SCANNER** — même montage que l'imprimante
    /// ci-dessus, dont il reprend la leçon : *la fente se pose LÀ OÙ LA FEUILLE
    /// PASSE*, sinon elle se lit comme un trou au milieu d'une façade.
    ///
    /// ⚠️ **CE QUI LE SÉPARE DE L'IMPRIMANTE, ET C'EST LE SEUL ÉCART QUI COMPTE** :
    /// le corps est un **PLATEAU** (large et bas, 44 px de haut contre 64), et la
    /// fente est **EN HAUT**, là où le document entre — l'imprimante a la sienne au
    /// bas, là où la feuille sort. Deux silhouettes de même famille, distinguables
    /// à la taille d'une icône de dock.
    ///
    /// Lancer : `cargo test --bins -- --ignored --nocapture dessine_le_glyphe_du_scanner`
    #[test]
    #[ignore = "ecrit TopbarIcons/scanner.png — a lancer a la main quand le dessin change"]
    fn dessine_le_glyphe_du_scanner() {
        const TAILLE: u32 = 128;
        const SS: u32 = 4;

        fn rond(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
            if x < x0 || x >= x1 || y < y0 || y >= y1 {
                return false;
            }
            let (cx, cy) = (x.clamp(x0 + r, x1 - r), y.clamp(y0 + r, y1 - r));
            let (dx, dy) = (x - cx, y - cy);
            dx * dx + dy * dy <= r * r
        }

        /// **LA FORME, EN UNE SEULE QUESTION.** L'ordre des tests EST le dessin :
        /// le plateau, puis la fente qui l'ouvre, et enfin la feuille redessinée
        /// PAR-DESSUS — ce qui ne laisse de la fente que les deux encoches de part
        /// et d'autre.
        fn plein(x: f32, y: f32) -> bool {
            // La feuille qui entre, par le haut : elle traverse la fente et
            // descend dans l'appareil.
            //
            // ⚠️ **ÉTROITE ET COURTE, ET C'EST UNE LEÇON DE LA PREMIÈRE ESQUISSE** :
            // à 60 × 74 px sur un canevas de 128, elle DOMINAIT le plateau — on
            // lisait « une feuille dans une fente », pas un scanner. À 48 × 68,
            // l'appareil redevient le sujet et la feuille son document.
            if (40.0..88.0).contains(&x) && (16.0..84.0).contains(&y) {
                return true;
            }
            if rond(x, y, 4.0, 62.0, 124.0, 114.0, 12.0) {
                // La fente, au ras du HAUT du plateau.
                let fente = (16.0..112.0).contains(&x) && (62.0..72.0).contains(&y);
                return !fente;
            }
            false
        }

        let mut rgba = vec![0u8; (TAILLE * TAILLE * 4) as usize];
        for py in 0..TAILLE {
            for px in 0..TAILLE {
                let mut couverts = 0u32;
                for sy in 0..SS {
                    for sx in 0..SS {
                        let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                        let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                        if plein(x, y) {
                            couverts += 1;
                        }
                    }
                }
                let alpha = (couverts * 255 / (SS * SS)) as u8;
                let o = ((py * TAILLE + px) * 4) as usize;
                rgba[o] = 255;
                rgba[o + 1] = 255;
                rgba[o + 2] = 255;
                rgba[o + 3] = alpha;
            }
        }

        let png = crate::win32::clipboard::encode_png(TAILLE, TAILLE, &rgba).expect("encodage PNG");
        let chemin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("TopbarIcons").join("scanner.png");
        std::fs::write(&chemin, &png).expect("ecriture du PNG");
        println!("ecrit {} ({} octets)", chemin.display(), png.len());
    }

    /// **LE SYMBOLE COULEUR DU SCANNER — DESSINÉ, PAS RASTERISÉ.**
    ///
    /// ⛔ **POURQUOI PAS UNE EMOJI, COMME LES 33 AUTRES.** Il n'existe **aucun
    /// émoji « scanner »** dans Unicode, et l'imprimante (U+1F5A8) est déjà prise
    /// par « File d'impression ». Le fax (U+1F4E0) a été essayé — défaut SIGNALÉ
    /// par l'utilisateur : « l'icône apparaît mais en icône symbole c'est un fax
    /// pas un scanner ». Un appareil qui n'est pas le bon vaut moins qu'un dessin.
    ///
    /// ⚠️ **CE FICHIER VIT DANS `TopbarIcons/dessins/`, ET PLUS DANS UN JEU DE
    /// POLICE.** Il a été déplacé le 2026-10-28 avec le retrait du thème
    /// « Symboles » (voir `ui::icones`) : c'est un dessin de ce dépôt, pas un
    /// rendu de `seguiemj.ttf`, et les deux thèmes Fluent Emoji en copient une
    /// version à leur gabarit (`tools/rasteriser_fluent_emoji.py`).
    ///
    /// ⚠️ **IL PORTE SA PROPRE COULEUR** (`TexMode::Texture`, voir
    /// `TopbarIcons/SOURCE.md`) : c'est ce qui le range dans la famille des
    /// thèmes en couleur, et
    /// `ui::icones::tests::les_themes_en_couleur_sont_en_couleur_et_a_la_bonne_taille`
    /// exige plusieurs teintes — un dessin monochrome y échouerait.
    ///
    /// Lancer : `cargo test --bins -- --ignored --nocapture dessine_le_symbole_du_scanner`
    #[test]
    #[ignore = "ecrit TopbarIcons/dessins/scanner.png — a lancer a la main quand le dessin change"]
    fn dessine_le_symbole_du_scanner() {
        const TAILLE: u32 = 128;
        const SS: u32 = 4;
        /// Le plateau — le même bleu-gris que les appareils de bureau.
        const PLATEAU: [u8; 3] = [0x5B, 0x6B, 0x7C];
        /// La feuille, blanche.
        const FEUILLE: [u8; 3] = [0xF2, 0xF4, 0xF7];
        /// **LA LIGNE DE BALAYAGE** — c'est elle qui dit « scanner » plutôt que
        /// « boîte » : la lumière qui parcourt la vitre.
        const BALAYAGE: [u8; 3] = [0x3B, 0x82, 0xF6];

        fn rond(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
            if x < x0 || x >= x1 || y < y0 || y >= y1 {
                return false;
            }
            let (cx, cy) = (x.clamp(x0 + r, x1 - r), y.clamp(y0 + r, y1 - r));
            let (dx, dy) = (x - cx, y - cy);
            dx * dx + dy * dy <= r * r
        }

        /// **LA COULEUR DE CE POINT, OU RIEN.** Même géométrie que la version
        /// monochrome (`dessine_le_glyphe_du_scanner`) : les deux thèmes montrent
        /// donc le MÊME appareil, ce qui est le but.
        fn couleur(x: f32, y: f32) -> Option<[u8; 3]> {
            // La feuille, qui entre par le haut — dessinée la première pour être
            // recouverte par le plateau, puis redessinée dans la fente.
            let feuille = (40.0..88.0).contains(&x) && (16.0..84.0).contains(&y);
            let dans_plateau = rond(x, y, 4.0, 62.0, 124.0, 114.0, 12.0);
            let dans_fente = (16.0..112.0).contains(&x) && (62.0..72.0).contains(&y);
            if dans_plateau && !dans_fente {
                // La vitre, sous la fente : trois pixels de balayage.
                if (76.0..82.0).contains(&y) && (16.0..112.0).contains(&x) {
                    return Some(BALAYAGE);
                }
                return Some(PLATEAU);
            }
            if feuille {
                return Some(FEUILLE);
            }
            None
        }

        let mut rgba = vec![0u8; (TAILLE * TAILLE * 4) as usize];
        for py in 0..TAILLE {
            for px in 0..TAILLE {
                let (mut r, mut v, mut b, mut couverts) = (0u32, 0u32, 0u32, 0u32);
                for sy in 0..SS {
                    for sx in 0..SS {
                        let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                        let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                        if let Some(c) = couleur(x, y) {
                            r += c[0] as u32;
                            v += c[1] as u32;
                            b += c[2] as u32;
                            couverts += 1;
                        }
                    }
                }
                let o = ((py * TAILLE + px) * 4) as usize;
                if couverts > 0 {
                    // Moyenne des sous-échantillons COUVERTS seulement : moyenner
                    // avec le vide assombrirait les bords et cernerait le dessin
                    // d'un liseré sombre.
                    rgba[o] = (r / couverts) as u8;
                    rgba[o + 1] = (v / couverts) as u8;
                    rgba[o + 2] = (b / couverts) as u8;
                    rgba[o + 3] = (couverts * 255 / (SS * SS)) as u8;
                }
            }
        }

        let png = crate::win32::clipboard::encode_png(TAILLE, TAILLE, &rgba).expect("encodage PNG");
        let chemin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("TopbarIcons").join("dessins").join("scanner.png");
        std::fs::write(&chemin, &png).expect("ecriture du PNG");
        println!("ecrit {} ({} octets)", chemin.display(), png.len());
    }

    /// **LE GLYPHE DU CALENDRIER, ET SA PREUVE VISUELLE.**
    ///
    /// DEMANDE EXPLICITE (2026-09-23) : une applet calendrier dans la barre de
    /// droite. Il fallait alors DEUX glyphes — `calendar.png` pour l'applet,
    /// `bell.png` pour l'indicateur de rappel.
    ///
    /// ⛔ **`bell.png` N'EST PLUS DESSINÉE ICI, ET C'EST UN PIÈGE QU'ON DÉSARME.**
    /// La cloche est désormais **rastérisée** depuis le SVG de Keyline Icons
    /// (MIT) — voir `TopbarIcons/SOURCE.md`, qui porte la provenance et les
    /// mesures. **La laisser dans la boucle d'écriture de cette sonde aurait
    /// écrasé la cloche rastérisée par l'ancienne dessinée**, en silence, au
    /// prochain `cargo test -- --ignored` : un fichier de production remplacé
    /// par une sonde de test, sans que rien ne le dise.
    ///
    /// ⚠️ **CE QUI REND CETTE SONDE DIFFÉRENTE DES AUTRES : ELLE IMPRIME AUSSI LE
    /// GLYPHE EN ASCII.** Un dessin qu'on ne peut pas voir ne se vérifie pas en
    /// le regardant dans un PNG — et « ça devrait ressembler à un calendrier »
    /// n'est pas une mesure. L'ASCII, lui, se lit dans la sortie du test — et il
    /// a SERVI : la première cloche (aujourd'hui remplacée) avait un corps TROP
    /// ÉTROIT en haut et se lisait comme une poire, ce qu'on a vu en lisant
    /// l'ASCII, pas en regardant le PNG.
    ///
    /// Lancer : `cargo test --bins -- --ignored --nocapture dessine_les_glyphes`
    #[test]
    #[ignore = "ecrit TopbarIcons/calendar.png — a lancer a la main quand le dessin change"]
    fn dessine_les_glyphes_du_calendrier() {
        const TAILLE: u32 = 128;
        const SS: u32 = 4;

        fn rond(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
            if x < x0 || x >= x1 || y < y0 || y >= y1 {
                return false;
            }
            let (cx, cy) = (x.clamp(x0 + r, x1 - r), y.clamp(y0 + r, y1 - r));
            let (dx, dy) = (x - cx, y - cy);
            dx * dx + dy * dy <= r * r
        }
        // ⚠️ `disque` a été RETIRÉ avec la cloche : c'était son seul usage.
        // Le garder aurait produit un avertissement de code mort — et ce dépôt
        // compte ses avertissements (81 avant ce chantier).

        /// LE CALENDRIER. Un corps arrondi, une BANDE d'en-tête pleine, et une
        /// grille de TROUS pour les jours — les trous sont ce qui dit
        /// « calendrier » plutôt que « bloc ».
        fn calendrier(x: f32, y: f32) -> bool {
            // Les deux anneaux, dessinés d'abord : le corps les recouvre en
            // partie, ce qui les fait dépasser par le haut.
            if ((30.0..40.0).contains(&x) || (88.0..98.0).contains(&x)) && (8.0..40.0).contains(&y) {
                return true;
            }
            if rond(x, y, 12.0, 24.0, 116.0, 120.0, 12.0) {
                // Bande d'en-tête : pleine. Séparée du corps par un VIDE, sinon
                // elle ne se lit pas comme une bande.
                if (24.0..56.0).contains(&y) {
                    return true;
                }
                if (56.0..60.0).contains(&y) {
                    return false;
                }
                // Grille de jours : 3 × 3 trous.
                for ligne in 0..3 {
                    for col in 0..3 {
                        let cx = 34.0 + col as f32 * 30.0;
                        let cy = 72.0 + ligne as f32 * 18.0;
                        if (cx - 6.0..cx + 6.0).contains(&x) && (cy - 4.0..cy + 4.0).contains(&y) {
                            return false;
                        }
                    }
                }
                return true;
            }
            false
        }

        fn rend(glyphe: fn(f32, f32) -> bool) -> Vec<u8> {
            let mut rgba = vec![0u8; (TAILLE * TAILLE * 4) as usize];
            for py in 0..TAILLE {
                for px in 0..TAILLE {
                    let mut couverts = 0u32;
                    for sy in 0..SS {
                        for sx in 0..SS {
                            let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                            let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                            if glyphe(x, y) {
                                couverts += 1;
                            }
                        }
                    }
                    let o = ((py * TAILLE + px) * 4) as usize;
                    rgba[o] = 255;
                    rgba[o + 1] = 255;
                    rgba[o + 2] = 255;
                    rgba[o + 3] = (couverts * 255 / (SS * SS)) as u8;
                }
            }
            rgba
        }

        /// **LE GLYPHE EN ASCII** — la seule mesure possible d'un dessin qu'on ne
        /// peut pas regarder. Un caractère tous les 2 px : assez fin pour juger
        /// une silhouette, assez court pour tenir dans une sortie de test.
        fn ascii(rgba: &[u8]) -> String {
            let mut out = String::new();
            for py in (0..TAILLE).step_by(2) {
                for px in (0..TAILLE).step_by(2) {
                    let a = rgba[((py * TAILLE + px) * 4 + 3) as usize];
                    out.push(match a {
                        0..=63 => ' ',
                        64..=191 => '.',
                        _ => '#',
                    });
                }
                out.push('\n');
            }
            out
        }

        let dossier = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("TopbarIcons");
        // ⚠️ **UNE SEULE ENTRÉE, ET C'EST VOLONTAIRE** : `bell.png` a été retirée
        // de cette boucle le 2026-09-23 le jour où la cloche est devenue un SVG
        // rastérisé. L'y laisser l'aurait ÉCRASÉE par l'ancien dessin. Voir la
        // doc de cette sonde.
        for (nom, glyphe) in [("calendar.png", calendrier as fn(f32, f32) -> bool)] {
            let rgba = rend(glyphe);
            println!("\n=== {nom} ===\n{}", ascii(&rgba));
            let png = crate::win32::clipboard::encode_png(TAILLE, TAILLE, &rgba).expect("encodage PNG");
            let chemin = dossier.join(nom);
            std::fs::write(&chemin, &png).expect("ecriture du PNG");
            println!("ecrit {} ({} octets)", chemin.display(), png.len());
        }
    }

    /// **LE GLYPHE DE L'ÉDITEUR D'IMAGE** — un cadre, un soleil et deux montagnes :
    /// la vignette « image » que tout le monde lit. Même méthode que les deux sondes
    /// ci-dessus (formes pleines, suréchantillonnage 4×4, détails en TROUS) et même
    /// preuve : l'ASCII dans la sortie du test.
    ///
    /// Lancer : `cargo test --bins -- --ignored --nocapture dessine_le_glyphe_de_l_editeur`
    #[test]
    #[ignore = "ecrit TopbarIcons/image_editor.png — a lancer a la main quand le dessin change"]
    fn dessine_le_glyphe_de_l_editeur_d_image() {
        const TAILLE: u32 = 128;
        const SS: u32 = 4;

        fn rond(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
            if x < x0 || x >= x1 || y < y0 || y >= y1 {
                return false;
            }
            let (cx, cy) = (x.clamp(x0 + r, x1 - r), y.clamp(y0 + r, y1 - r));
            let (dx, dy) = (x - cx, y - cy);
            dx * dx + dy * dy <= r * r
        }

        /// Sous la ligne qui joint le sommet `(sx, sy)` à la base `y = base`,
        /// de demi-largeur `demi` — un triangle isocèle.
        fn montagne(x: f32, y: f32, sx: f32, sy: f32, demi: f32, base: f32) -> bool {
            y < base && y >= sy && (x - sx).abs() <= demi * (y - sy) / (base - sy)
        }

        fn plein(x: f32, y: f32) -> bool {
            // Le CADRE : un rectangle arrondi évidé, trait de 10 px.
            let dehors = rond(x, y, 6.0, 16.0, 122.0, 112.0, 14.0);
            let dedans = rond(x, y, 16.0, 26.0, 112.0, 102.0, 6.0);
            if dehors && !dedans {
                return true;
            }
            if !dedans {
                return false;
            }
            // Le SOLEIL, en haut à droite.
            let (dx, dy) = (x - 86.0, y - 46.0);
            if dx * dx + dy * dy <= 10.0 * 10.0 {
                return true;
            }
            // Deux MONTAGNES posées sur le bas du cadre — la petite derrière, à droite.
            montagne(x, y, 48.0, 50.0, 40.0, 102.0) || montagne(x, y, 86.0, 68.0, 30.0, 102.0)
        }

        let mut rgba = vec![0u8; (TAILLE * TAILLE * 4) as usize];
        for py in 0..TAILLE {
            for px in 0..TAILLE {
                let mut couverts = 0u32;
                for sy in 0..SS {
                    for sx in 0..SS {
                        let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                        let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                        if plein(x, y) {
                            couverts += 1;
                        }
                    }
                }
                let o = ((py * TAILLE + px) * 4) as usize;
                rgba[o..o + 3].fill(255);
                rgba[o + 3] = (couverts * 255 / (SS * SS)) as u8;
            }
        }

        let mut ascii = String::new();
        for py in (0..TAILLE).step_by(4) {
            for px in (0..TAILLE).step_by(2) {
                ascii.push(match rgba[((py * TAILLE + px) * 4 + 3) as usize] {
                    0..=63 => ' ',
                    64..=191 => '.',
                    _ => '#',
                });
            }
            ascii.push('\n');
        }
        println!("\n=== image_editor.png ===\n{ascii}");

        let png = crate::win32::clipboard::encode_png(TAILLE, TAILLE, &rgba).expect("encodage PNG");
        let chemin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("TopbarIcons").join("image_editor.png");
        std::fs::write(&chemin, &png).expect("ecriture du PNG");
        println!("ecrit {} ({} octets)", chemin.display(), png.len());
    }

    /// **LA PIPETTE** — l'applet « Sélecteur de couleurs » (voir
    /// `win32::cef_couleurs`).
    ///
    /// DESSINÉE, comme l'éditeur d'image et le calendrier : un corps diagonal,
    /// un collier, un bulbe et une pointe. La silhouette est ce qui la fait
    /// lire — un corps à largeur constante donnerait un thermomètre, et un
    /// bulbe sans collier donnerait une cuillère.
    ///
    /// Lancer : `cargo test --bins -- --ignored --nocapture dessine_le_glyphe_de_la_pipette`
    #[test]
    #[ignore = "ecrit TopbarIcons/couleurs.png — a lancer a la main quand le dessin change"]
    fn dessine_le_glyphe_de_la_pipette() {
        const TAILLE: u32 = 128;
        const SS: u32 = 4;

        fn plein(x: f32, y: f32) -> bool {
            /// Cosinus et sinus de 45° — l'axe de la pipette.
            const C: f32 = 0.707_106_77;
            /// Demi-largeur du corps le long de l'axe, `None` au-delà des
            /// extrémités.
            ///
            /// ⚠️ **LES TRANSITIONS SONT LINÉAIRES, JAMAIS DES MARCHES.** Une
            /// première version passait de 4 à 8 puis revenait à 4 par sauts :
            /// sur une ligne HORIZONTALE, `s` et `w` varient tous les deux, et
            /// chaque saut se voyait comme un TROU dans la silhouette
            /// (`###### ##`) et une encoche au raccord du bulbe — constaté dans
            /// l'ASCII ci-dessous, pas deviné.
            fn demi_largeur(s: f32) -> Option<f32> {
                const CORPS: f32 = 5.0;
                const BULBE: f32 = 16.5; // le rayon du cercle, plus bas
                Some(match s {
                    s if s < 0.0 => return None,
                    s if s < 20.0 => s * (CORPS / 20.0), // pointe conique, nulle au bout
                    s if s < 58.0 => CORPS,
                    // ÉVASEMENT CONTINU jusque dans le bulbe : c'est lui qui
                    // ferme le raccord. Un collier à MARCHES laissait une
                    // déchirure de 4,7 px entre le corps et le cercle — mesurée
                    // dans l'ASCII, à la ligne du bas du bulbe.
                    s if s < 90.0 => CORPS + (s - 58.0) * ((BULBE - CORPS) / 32.0),
                    _ => return None,
                })
            }
            // Repère le long de l'axe : `s` progresse de la POINTE (0) vers le
            // BULBE, `w` est la distance perpendiculaire signée.
            let (tx, ty) = (24.0, 104.0);
            let s = (x - tx) * C + (y - ty) * -C;
            let w = (x - tx) * C + (y - ty) * C;
            // Le BULBE : un cercle au bout de l'axe. Vérifié EN PREMIER, parce
            // qu'il déborde des tranches ci-dessous.
            let (cx, cy) = (tx + C * 90.0, ty - C * 90.0);
            let (dx, dy) = (x - cx, y - cy);
            if dx * dx + dy * dy <= 16.5 * 16.5 {
                return true;
            }
            // ⚠️ Le CORPS EST FIN (4) ET LE BULBE LARGE (33 de diamètre) : c'est
            // ce rapport-là qui fait lire une pipette. À 7 contre 30, la
            // première version donnait une hache.
            match demi_largeur(s) {
                Some(demi) => w.abs() <= demi,
                None => false,
            }
        }

        let mut rgba = vec![0u8; (TAILLE * TAILLE * 4) as usize];
        for py in 0..TAILLE {
            for px in 0..TAILLE {
                let mut couverts = 0u32;
                for sy in 0..SS {
                    for sx in 0..SS {
                        let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                        let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                        if plein(x, y) {
                            couverts += 1;
                        }
                    }
                }
                let o = ((py * TAILLE + px) * 4) as usize;
                rgba[o..o + 3].fill(255);
                rgba[o + 3] = (couverts * 255 / (SS * SS)) as u8;
            }
        }

        let mut ascii = String::new();
        for py in (0..TAILLE).step_by(4) {
            for px in (0..TAILLE).step_by(2) {
                ascii.push(match rgba[((py * TAILLE + px) * 4 + 3) as usize] {
                    0..=63 => ' ',
                    64..=191 => '.',
                    _ => '#',
                });
            }
            ascii.push('\n');
        }
        println!("\n=== couleurs.png ===\n{ascii}");

        let png = crate::win32::clipboard::encode_png(TAILLE, TAILLE, &rgba).expect("encodage PNG");
        let chemin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("TopbarIcons").join("couleurs.png");
        std::fs::write(&chemin, &png).expect("ecriture du PNG");
        println!("ecrit {} ({} octets)", chemin.display(), png.len());
    }
}
