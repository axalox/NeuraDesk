//! Palette inspirée du thème "Ambiance" d'Ubuntu et du launcher Unity :
//! aubergine sombre translucide pour le fond du dock, orange Ubuntu pour
//! les accents/indicateurs.
//!
//! # Éléments et sous-éléments — ce que GNOME/libadwaita font différemment
//!
//! Demande explicite : "cherches comment fonctionnent les thèmes de couleurs
//! de gnome, trouves l'algorithme pour gérer les éléments et sous éléments,
//! améliores tes algorithmes de gestion de couleurs". Recherche menée contre
//! la doc officielle de libadwaita (`Adw – 1: CSS Variables`,
//! <https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.2/css-variables.html>) —
//! le système de couleurs nommées qui remplace le thème GTK CSS classique
//! depuis GNOME 42/libadwaita 1.0.
//!
//! Le principe central de ce système : chaque "rôle" de surface (fenêtre,
//! vue, en-tête, barre latérale, carte, boîte de dialogue, popover) ET
//! chaque couleur sémantique (accent, destructive, succès, avertissement,
//! erreur) est défini comme une PAIRE fond/texte NOMMÉE et TOUJOURS utilisée
//! ENSEMBLE (`--accent-bg-color`/`--accent-fg-color`,
//! `--card-bg-color`/`--card-fg-color`, `--popover-bg-color`/
//! `--popover-fg-color`...) — jamais un texte pensé pour un rôle réutilisé
//! sur le fond d'un AUTRE rôle. C'est exactement le principe déjà en place
//! ici pour `AdaptiveZones`/`Palette::zone_bg`+`zone_text` (une zone et son
//! texte calculés ENSEMBLE, voir `ensure_min_contrast`) — cette recherche a
//! surtout mis en évidence un rôle qui NE respectait PAS encore ce principe
//! dans ce module : `Palette::accent_fg` (voir sa doc), ajouté suite à
//! cette recherche pour combler cet écart — `v.widgets.active.bg_fill`/
//! `v.selection.bg_fill` (`egui_overlay::apply_theme`) réutilisaient
//! jusqu'ici `accent_text` (calculé pour le fond RÉEL, souvent transparent,
//! de toute la fenêtre — voir sa doc) au lieu d'un texte pensé pour LEUR
//! propre fond (un aplat plein et opaque de l'accent lui-même) — deux fonds
//! différents, silencieusement confondus sous un seul nom.
//!
//! Le reste de la hiérarchie GNOME (fenêtre → vue/carte → popover, chaque
//! niveau "posé" sur le précédent avec un pas de luminosité mesuré) est
//! DÉJÀ couvert ici, sous d'autres noms : `accent_text` (texte posé sur le
//! fond RÉEL de la fenêtre, potentiellement mélangé à une opacité inconnue
//! avec le flou du bureau — voir `readable_text_rgb_guaranteed`) correspond
//! au rôle "fenêtre" ; `zone_bg`/`zone_text` (voir `adaptive_zones`)
//! correspond au rôle "carte" (bouton, case, curseur, tuile) ; `emphasis`
//! correspond aux textes peints dans l'accent lui-même mais posés sur un
//! fond de la même famille (`accent_emphasis_rgb`). Contrairement à
//! libadwaita — qui associe une poignée de couleurs D'ACCENT CURATÉES À LA
//! MAIN (9 teintes fixes) à des paires bg/fg elles aussi choisies à la
//! main — ce module doit garantir un contraste WCAG correct pour N'IMPORTE
//! QUELLE couleur choisie par l'utilisateur (voir `ensure_min_contrast`/
//! `push_until`) : la contrepartie ALGORITHMIQUE du même principe, pas une
//! simple table de correspondance.

/// Opacité "nominale" associée à une couleur de fond choisie/détectée.
/// N'affecte plus directement le rendu des barres (voir `soften_tint` et
/// `gfx::backdrop` : elles sont maintenant composées à partir d'un
/// arrière-plan flouté teinté, toujours opaque en sortie), mais reste
/// utilisée comme 4e composante de la couleur persistée
/// (`config::save_background_color`) pour rester compatible avec ce format.
pub const DOCK_ALPHA: f32 = 0.9;

/// Fond translucide de repli si la couleur d'accentuation Windows ne peut
/// pas être détectée (voir `win32::theme::detect_accent_color`) — aubergine
/// sombre.
pub const DOCK_BACKGROUND_FALLBACK: [f32; 4] = [0.071, 0.008, 0.075, DOCK_ALPHA];

/// Couleur de mise en avant INDIVIDUELLE d'une des trois fenêtres `egui`
/// (Dash, panneau de réglages, visionneuse de presse-papiers — voir
/// `config::load_window_accent`/`load_window_palette_accent`) — DEUX rôles
/// séparés, demande explicite : "dans la gestion individuelle de la couleur
/// des dashs il faut choisir la couleur de fond mais aussi une couleur pour
/// la palette automatique des éléments".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowAccent {
    /// Couleur du fond flouté teinté de LA FENÊTRE elle-même (`tint_rgba`,
    /// voir `win32::dash`/`win32::clipboard_viewer`/`win32::settings_dash`).
    pub background: [f32; 3],
    /// `true` : `background` sert AUSSI de fond à l'icône associée dans le
    /// dock (voir `ui::dock::icon_zone_instance`) — inchangé par
    /// l'introduction de `palette` ci-dessous, cette case reste liée au
    /// FOND, pas à la palette d'éléments.
    pub icon_bg: bool,
    /// Couleur SÉPARÉE qui alimente la palette automatique des éléments
    /// (zones, texte, emphase — voir `compute_palette`, `egui_overlay::
    /// apply_theme`) DE CETTE FENÊTRE — `None` (réglage normal, case à
    /// cocher désactivée dans `win32::settings_dash::render_accent_section`)
    /// : suit AUTOMATIQUEMENT `background`, comportement identique à avant
    /// l'introduction de ce champ. `Some(rgb)` : couleur fixe et
    /// INDÉPENDANTE choisie explicitement.
    pub palette: Option<[f32; 3]>,
}

impl WindowAccent {
    /// Charge la couleur ET l'option "fond d'icône" (voir `config::
    /// load_window_accent`) ainsi que la couleur de palette séparée (voir
    /// `config::load_window_palette_accent`) d'une fenêtre — `window` :
    /// même convention que ces deux fonctions (`"dash"`/`"settings"`/
    /// `"clipboard"`). Repli sur `default_background`/fond d'icône
    /// désactivé/palette suivant automatiquement le fond si jamais
    /// personnalisée.
    pub fn load(window: &str, default_background: [f32; 3]) -> Self {
        let (background, icon_bg) =
            crate::config::load_window_accent(window).unwrap_or((default_background, false));
        let palette = crate::config::load_window_palette_accent(window);
        Self { background, icon_bg, palette }
    }

    /// Couleur RÉELLEMENT utilisée pour la palette automatique des éléments
    /// de cette fenêtre — `palette` si personnalisée, sinon `background`
    /// (voir la doc du champ `palette`).
    pub fn effective_palette(&self) -> [f32; 3] {
        self.palette.unwrap_or(self.background)
    }
}

/// Couleur/transparence du dock forcée manuellement, `[r, g, b, a]` en
/// 0.0..1.0 chacun. `None` (par défaut) : le dock suit automatiquement la
/// couleur d'accentuation Windows actuelle — la même que celle de la barre
/// des tâches si "Afficher la couleur d'accentuation sur la barre des
/// tâches et le menu Démarrer" est activé dans Paramètres >
/// Personnalisation > Couleurs. Mettre `Some([r, g, b, a])` ici pour
/// imposer une couleur fixe à la place.
pub const CUSTOM_BACKGROUND: Option<[f32; 4]> = None;

/// Neutre sombre utilisé pour adoucir la teinte des barres en thème sombre
/// (voir `soften_tint`) — la barre des tâches Windows ne montre jamais la
/// couleur d'accent à l'état brut, elle la mélange avec une base neutre
/// pour un rendu plus "habillé", façon matériau Mica/Acrylic.
const NEUTRAL_DARK: [f32; 3] = [0.09, 0.09, 0.10];
/// Neutre clair, même usage en thème clair.
const NEUTRAL_LIGHT: [f32; 3] = [0.94, 0.94, 0.95];
/// Proportion de couleur conservée lors de l'adoucissement (le reste vient
/// du neutre `NEUTRAL_DARK`/`NEUTRAL_LIGHT`) — remontée depuis 0.35 (BUG
/// RÉEL SIGNALÉ EN PRATIQUE : à cette valeur, la barre ne conservait que
/// 35% de la couleur choisie même à 0% de transparence, donnant un rendu
/// nettement "moins lumineux" que la couleur réellement sélectionnée dans
/// le sélecteur). Toujours un mélange (pas 1.0 pur) pour garder un peu de
/// l'effet Mica/Acrylic recherché, mais la couleur choisie reste
/// maintenant clairement reconnaissable.
const TINT_SOFTEN_STRENGTH: f32 = 0.85;
/// Proportion par défaut de la teinte adoucie mélangée à l'arrière-plan
/// flouté brut de chaque barre (voir `gfx::backdrop`,
/// `QuadInstance::new_backdrop`) — le reste laisse transparaître le flou du
/// bureau capturé. Réglable individuellement par barre via le menu
/// contextuel de l'icône NeuraDesk (voir `win32::menu::Bar`,
/// `config::load_bar_tint`), cette valeur ne sert qu'au tout premier
/// lancement.
pub const DEFAULT_BACKDROP_TINT_STRENGTH: f32 = 0.85;

/// Mélange `rgb` avec un neutre sombre ou clair (selon `dark_mode`) dans la
/// proportion `TINT_SOFTEN_STRENGTH`, façon teinte Mica/Acrylic de la barre
/// des tâches Windows plutôt qu'une couleur d'accent brute.
pub fn soften_tint(rgb: [f32; 3], dark_mode: bool) -> [f32; 3] {
    let neutral = if dark_mode { NEUTRAL_DARK } else { NEUTRAL_LIGHT };
    [
        neutral[0] + (rgb[0] - neutral[0]) * TINT_SOFTEN_STRENGTH,
        neutral[1] + (rgb[1] - neutral[1]) * TINT_SOFTEN_STRENGTH,
        neutral[2] + (rgb[2] - neutral[2]) * TINT_SOFTEN_STRENGTH,
    ]
}

const fn rgb(r: u8, g: u8, b: u8) -> [f32; 4] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}

/// La palette "Couleurs Windows" du sélecteur natif (Paramètres >
/// Personnalisation > Couleurs > Couleur d'accentuation > Manuel), extraite
/// pixel par pixel d'une capture d'écran de ce sélecteur — Microsoft ne
/// publie pas ces valeurs, donc autant les prendre exactes plutôt que de
/// deviner. Proposée dans le menu contextuel de l'icône NeuraDesk (voir
/// `win32::menu`) en plus du choix d'une couleur totalement libre.
pub const WINDOWS_COLORS: [[f32; 4]; 48] = [
    // Rangée 1
    rgb(0xFF, 0xB9, 0x00), rgb(0xFF, 0x8C, 0x00), rgb(0xF7, 0x63, 0x0C), rgb(0xCA, 0x50, 0x10),
    rgb(0xDA, 0x3B, 0x01), rgb(0xEF, 0x69, 0x50), rgb(0xD1, 0x34, 0x38), rgb(0xFF, 0x43, 0x43),
    rgb(0xE7, 0x48, 0x56),
    // Rangée 2
    rgb(0xE8, 0x11, 0x23), rgb(0xEA, 0x00, 0x5E), rgb(0xC3, 0x00, 0x52), rgb(0xE3, 0x00, 0x8C),
    rgb(0xBF, 0x00, 0x77), rgb(0xC2, 0x39, 0xB3), rgb(0x9A, 0x00, 0x89), rgb(0x00, 0x78, 0xD4),
    rgb(0x00, 0x63, 0xB1),
    // Rangée 3
    rgb(0x8E, 0x8C, 0xD8), rgb(0x6B, 0x69, 0xD6), rgb(0x87, 0x64, 0xB8), rgb(0x74, 0x4D, 0xA9),
    rgb(0xB1, 0x46, 0xC2), rgb(0x88, 0x17, 0x98), rgb(0x00, 0x99, 0xBC), rgb(0x2D, 0x7D, 0x9A),
    rgb(0x00, 0xB7, 0xC3),
    // Rangée 4
    rgb(0x03, 0x83, 0x87), rgb(0x00, 0xB2, 0x94), rgb(0x01, 0x85, 0x74), rgb(0x00, 0xCC, 0x6A),
    rgb(0x10, 0x89, 0x3E), rgb(0x7A, 0x75, 0x74), rgb(0x5D, 0x5A, 0x58), rgb(0x68, 0x76, 0x8A),
    rgb(0x51, 0x5C, 0x6B),
    // Rangée 5
    rgb(0x56, 0x7C, 0x73), rgb(0x48, 0x68, 0x60), rgb(0x49, 0x82, 0x05), rgb(0x10, 0x7C, 0x10),
    rgb(0x76, 0x76, 0x76), rgb(0x4C, 0x4A, 0x48), rgb(0x69, 0x79, 0x7E), rgb(0x4A, 0x54, 0x59),
    rgb(0x64, 0x7C, 0x64),
    // Rangée 6
    rgb(0x52, 0x5E, 0x54), rgb(0x84, 0x75, 0x45), rgb(0x7E, 0x73, 0x5F),
];

const fn rgb3(r: u8, g: u8, b: u8) -> [f32; 3] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
}

/// 120 thèmes prêts à l'emploi (voir `win32::settings_dash::
/// render_themes_section`) — demande explicite : "120 thèmes sombres et très
/// sombres", après une première liste de 20 (clair + sombre mélangés) puis
/// une de 50 (uniquement sombre, 2 profondeurs). UNE SEULE couleur par
/// thème (pas un jeu de plusieurs couleurs assorties) : le système de
/// colorisation automatique de ce module fait déjà dériver TOUT le reste
/// (zones, texte, icônes de repli — voir `compute_palette`/`icon_palette`)
/// d'un seul accent — un "thème" ici, c'est donc juste "applique CET accent
/// PARTOUT" (voir `render_themes_section`), pas une notion différente.
///
/// 40 teintes à intervalles réguliers sur tout le cercle chromatique (9°
/// chacune), déclinées CHACUNE en 3 profondeurs — luminosité HSL 17%
/// ("nuit", la plus sombre), 25% ("profond") et 42% (la plus claire des
/// trois, encore nettement sombre) — jamais assemblées au hasard : générées
/// via un générateur de palettes de couleurs (MCP `color-scheme-generator`,
/// schéma "monochrome sombre" à 40 graines réparties sur le cercle, appelé
/// à DEUX luminosités de départ différentes par teinte — 50% donne les
/// profondeurs 25%/42%, 25% donne la profondeur 17% — pour extraire les
/// trois profondeurs utiles de chaque teinte plutôt qu'une seule). Noms
/// français : une famille de couleur par teinte (ex. "Bleu marine"), déclinée
/// avec le suffixe "nuit" (17%) ou "profond" (25%) ou sans suffixe (42%,
/// la teinte "de base" de la famille) — garantit 120 noms tous distincts.
pub const THEMES: [(&str, [f32; 3]); 120] = [
    ("Rouge nuit", rgb3(0x4A, 0x0B, 0x0B)), ("Rouge profond", rgb3(0x6E, 0x12, 0x12)), ("Rouge", rgb3(0xB9, 0x1C, 0x1C)),
    ("Rouge-brique nuit", rgb3(0x4A, 0x14, 0x0B)), ("Rouge-brique profond", rgb3(0x6E, 0x20, 0x12)), ("Rouge-brique", rgb3(0xB9, 0x33, 0x1C)),
    ("Brique nuit", rgb3(0x4A, 0x1E, 0x0B)), ("Brique profond", rgb3(0x6E, 0x2D, 0x12)), ("Brique", rgb3(0xB9, 0x4B, 0x1C)),
    ("Terracotta nuit", rgb3(0x4A, 0x27, 0x0B)), ("Terracotta profond", rgb3(0x6E, 0x3B, 0x12)), ("Terracotta", rgb3(0xB9, 0x62, 0x1C)),
    ("Cannelle nuit", rgb3(0x4A, 0x31, 0x0B)), ("Cannelle profond", rgb3(0x6E, 0x49, 0x12)), ("Cannelle", rgb3(0xB9, 0x7A, 0x1C)),
    ("Ambre nuit", rgb3(0x4A, 0x3B, 0x0B)), ("Ambre profond", rgb3(0x6E, 0x57, 0x12)), ("Ambre", rgb3(0xB9, 0x91, 0x1C)),
    ("Ambre olive nuit", rgb3(0x4A, 0x44, 0x0B)), ("Ambre olive profond", rgb3(0x6E, 0x65, 0x12)), ("Ambre olive", rgb3(0xB9, 0xA9, 0x1C)),
    ("Olive doré nuit", rgb3(0x47, 0x4A, 0x0B)), ("Olive doré profond", rgb3(0x6A, 0x6E, 0x12)), ("Olive doré", rgb3(0xB2, 0xB9, 0x1C)),
    ("Olive nuit", rgb3(0x3D, 0x4A, 0x0B)), ("Olive profond", rgb3(0x5C, 0x6E, 0x12)), ("Olive", rgb3(0x9A, 0xB9, 0x1C)),
    ("Olive vert nuit", rgb3(0x34, 0x4A, 0x0B)), ("Olive vert profond", rgb3(0x4E, 0x6E, 0x12)), ("Olive vert", rgb3(0x82, 0xB9, 0x1C)),
    ("Vert olive nuit", rgb3(0x2B, 0x4A, 0x0B)), ("Vert olive profond", rgb3(0x40, 0x6E, 0x12)), ("Vert olive", rgb3(0x6B, 0xB9, 0x1C)),
    ("Vert mousse nuit", rgb3(0x21, 0x4A, 0x0B)), ("Vert mousse profond", rgb3(0x32, 0x6E, 0x12)), ("Vert mousse", rgb3(0x53, 0xB9, 0x1C)),
    ("Vert sapin nuit", rgb3(0x18, 0x4A, 0x0B)), ("Vert sapin profond", rgb3(0x24, 0x6E, 0x12)), ("Vert sapin", rgb3(0x3B, 0xB9, 0x1C)),
    ("Vert forêt nuit", rgb3(0x0E, 0x4A, 0x0B)), ("Vert forêt profond", rgb3(0x16, 0x6E, 0x12)), ("Vert forêt", rgb3(0x23, 0xB9, 0x1C)),
    ("Vert émeraude nuit", rgb3(0x0B, 0x4A, 0x11)), ("Vert émeraude profond", rgb3(0x12, 0x6E, 0x1B)), ("Vert émeraude", rgb3(0x1C, 0xB9, 0x2B)),
    ("Émeraude nuit", rgb3(0x0B, 0x4A, 0x1B)), ("Émeraude profond", rgb3(0x12, 0x6E, 0x29)), ("Émeraude", rgb3(0x1C, 0xB9, 0x43)),
    ("Vert menthe nuit", rgb3(0x0B, 0x4A, 0x24)), ("Vert menthe profond", rgb3(0x12, 0x6E, 0x36)), ("Vert menthe", rgb3(0x1C, 0xB9, 0x5A)),
    ("Menthe nuit", rgb3(0x0B, 0x4A, 0x2E)), ("Menthe profond", rgb3(0x12, 0x6E, 0x44)), ("Menthe", rgb3(0x1C, 0xB9, 0x72)),
    ("Jade nuit", rgb3(0x0B, 0x4A, 0x37)), ("Jade profond", rgb3(0x12, 0x6E, 0x52)), ("Jade", rgb3(0x1C, 0xB9, 0x8A)),
    ("Sarcelle nuit", rgb3(0x0B, 0x4A, 0x41)), ("Sarcelle profond", rgb3(0x12, 0x6E, 0x60)), ("Sarcelle", rgb3(0x1C, 0xB9, 0xA1)),
    ("Turquoise nuit", rgb3(0x0B, 0x4A, 0x4A)), ("Turquoise profond", rgb3(0x12, 0x6E, 0x6E)), ("Turquoise", rgb3(0x1C, 0xB9, 0xB9)),
    ("Cyan nuit", rgb3(0x0B, 0x41, 0x4A)), ("Cyan profond", rgb3(0x12, 0x60, 0x6E)), ("Cyan", rgb3(0x1C, 0xA1, 0xB9)),
    ("Bleu pétrole nuit", rgb3(0x0B, 0x37, 0x4A)), ("Bleu pétrole profond", rgb3(0x12, 0x52, 0x6E)), ("Bleu pétrole", rgb3(0x1C, 0x8A, 0xB9)),
    ("Bleu ardoise nuit", rgb3(0x0B, 0x2E, 0x4A)), ("Bleu ardoise profond", rgb3(0x12, 0x44, 0x6E)), ("Bleu ardoise", rgb3(0x1C, 0x72, 0xB9)),
    ("Bleu marine nuit", rgb3(0x0B, 0x24, 0x4A)), ("Bleu marine profond", rgb3(0x12, 0x36, 0x6E)), ("Bleu marine", rgb3(0x1C, 0x5A, 0xB9)),
    ("Bleu roi nuit", rgb3(0x0B, 0x1B, 0x4A)), ("Bleu roi profond", rgb3(0x12, 0x29, 0x6E)), ("Bleu roi", rgb3(0x1C, 0x43, 0xB9)),
    ("Bleu cobalt nuit", rgb3(0x0B, 0x11, 0x4A)), ("Bleu cobalt profond", rgb3(0x12, 0x1B, 0x6E)), ("Bleu cobalt", rgb3(0x1C, 0x2B, 0xB9)),
    ("Indigo nuit", rgb3(0x0E, 0x0B, 0x4A)), ("Indigo profond", rgb3(0x16, 0x12, 0x6E)), ("Indigo", rgb3(0x23, 0x1C, 0xB9)),
    ("Indigo violet nuit", rgb3(0x18, 0x0B, 0x4A)), ("Indigo violet profond", rgb3(0x24, 0x12, 0x6E)), ("Indigo violet", rgb3(0x3B, 0x1C, 0xB9)),
    ("Violet nuit", rgb3(0x21, 0x0B, 0x4A)), ("Violet profond", rgb3(0x32, 0x12, 0x6E)), ("Violet", rgb3(0x53, 0x1C, 0xB9)),
    ("Violet royal nuit", rgb3(0x2B, 0x0B, 0x4A)), ("Violet royal profond", rgb3(0x40, 0x12, 0x6E)), ("Violet royal", rgb3(0x6B, 0x1C, 0xB9)),
    ("Pourpre nuit", rgb3(0x34, 0x0B, 0x4A)), ("Pourpre profond", rgb3(0x4E, 0x12, 0x6E)), ("Pourpre", rgb3(0x82, 0x1C, 0xB9)),
    ("Orchidée nuit", rgb3(0x3D, 0x0B, 0x4A)), ("Orchidée profond", rgb3(0x5C, 0x12, 0x6E)), ("Orchidée", rgb3(0x9A, 0x1C, 0xB9)),
    ("Magenta nuit", rgb3(0x47, 0x0B, 0x4A)), ("Magenta profond", rgb3(0x6A, 0x12, 0x6E)), ("Magenta", rgb3(0xB2, 0x1C, 0xB9)),
    ("Fuchsia nuit", rgb3(0x4A, 0x0B, 0x44)), ("Fuchsia profond", rgb3(0x6E, 0x12, 0x65)), ("Fuchsia", rgb3(0xB9, 0x1C, 0xA9)),
    ("Rose fuchsia nuit", rgb3(0x4A, 0x0B, 0x3B)), ("Rose fuchsia profond", rgb3(0x6E, 0x12, 0x57)), ("Rose fuchsia", rgb3(0xB9, 0x1C, 0x91)),
    ("Rose grenat nuit", rgb3(0x4A, 0x0B, 0x31)), ("Rose grenat profond", rgb3(0x6E, 0x12, 0x49)), ("Rose grenat", rgb3(0xB9, 0x1C, 0x7A)),
    ("Grenat nuit", rgb3(0x4A, 0x0B, 0x27)), ("Grenat profond", rgb3(0x6E, 0x12, 0x3B)), ("Grenat", rgb3(0xB9, 0x1C, 0x62)),
    ("Bordeaux nuit", rgb3(0x4A, 0x0B, 0x1E)), ("Bordeaux profond", rgb3(0x6E, 0x12, 0x2D)), ("Bordeaux", rgb3(0xB9, 0x1C, 0x4B)),
    ("Cardinal nuit", rgb3(0x4A, 0x0B, 0x14)), ("Cardinal profond", rgb3(0x6E, 0x12, 0x20)), ("Cardinal", rgb3(0xB9, 0x1C, 0x33)),
];

/// Compteur incrémenté à chaque appel de `next_random_u64` — évite deux
/// tirages IDENTIQUES lors de deux clics rapprochés sur "Thème aléatoire"
/// (voir sa doc) : sans lui, deux appels dans la même nanoseconde d'horloge
/// système produiraient exactement la même graine.
static RANDOM_CALL_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Générateur pseudo-aléatoire xorshift64*, sans dépendance externe (voir
/// `random_dark_theme`, demande explicite : "ajouter la possibilité de
/// générer des thèmes aléatoires") — la crate `rand` n'était pas déjà une
/// dépendance de ce projet, et cet usage (une couleur de temps en temps, pas
/// de cryptographie) ne le justifie pas. Graine dérivée de l'horloge système
/// ET de `RANDOM_CALL_COUNT` (voir sa doc) : deux tirages successifs restent
/// distincts même appelés dans la même nanoseconde.
fn next_random_u64() -> u64 {
    let counter = RANDOM_CALL_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = nanos ^ counter.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

/// `next_random_u64` normalisé en 0.0..1.0 — étape de calcul de
/// `random_dark_theme`.
fn random_unit() -> f32 {
    (next_random_u64() >> 11) as f32 / (1u64 << 53) as f32
}

/// Génère une couleur de thème sombre ALÉATOIRE, dans la même famille
/// visuelle que `THEMES` (voir sa doc — teinte quelconque, mais saturation
/// et luminosité bornées à un intervalle "sombre et agréable" plutôt qu'un
/// tirage totalement libre qui retomberait parfois sur un blanc criard ou un
/// gris terne) — voir `win32::settings_dash::render_themes_section`, demande
/// explicite : "ajouter la possibilité de générer des thèmes aléatoires en
/// laissant à l'utilisateur de les sauvegarder ou les effacer".
///
/// Luminosité 0%..45% — PAS 22%..45% (première version) : demande explicite
/// — "modifier la logique du thème aléatoire pour pouvoir aussi obtenir des
/// thèmes proches du noir" — même borne haute que `dark_theme_from_base_color`
/// (rester dans la famille "sombre") mais bas jusqu'à 0% (noir pur inclus,
/// voir sa doc : `compute_palette`/`adaptive_zones` gèrent déjà correctement
/// un accent noir pur, aucune raison de s'en priver ici non plus). Tirage
/// UNIFORME sur tout l'intervalle (pas seulement une chance résiduelle de
/// noir) : chaque profondeur, du noir quasi jusqu'au sombre "moyen", a
/// autant de chances d'être tirée.
pub fn random_dark_theme() -> [f32; 3] {
    let hue = random_unit() * 360.0;
    let saturation = 0.55 + random_unit() * 0.35;
    let lightness = random_unit() * 0.45;
    hsl_to_rgb(hue, saturation, lightness)
}

/// Dérive un thème sombre à partir d'une couleur CHOISIE par l'utilisateur
/// (voir `win32::settings_dash::render_themes_section`, sélecteur "Couleur
/// de base") — demande explicite : "choisir la couleur de base du thème
/// depuis un sélecteur de couleur et que le thème qui en découle soit généré
/// automatiquement". Contrairement à `random_dark_theme` (teinte ET
/// profondeur tirées au hasard), ici la teinte ET LA LUMINOSITÉ viennent de
/// la couleur choisie (voir `rgb_to_hsl`) — seule la saturation reste bornée
/// à un intervalle "agréable" (voir plus bas). DÉTERMINISTE (contrairement
/// au tirage aléatoire) — la même couleur de base donne toujours le même
/// thème, pour que l'utilisateur puisse itérer sur le sélecteur et voir
/// l'app suivre en temps réel (voir `apply_theme`, rappelée à chaque
/// changement de couleur de base).
///
/// Luminosité de la couleur choisie ramenée dans 0%..45% — PAS figée à une
/// profondeur unique : BUG RÉEL SIGNALÉ EN PRATIQUE avec une première
/// version qui bornait TOUJOURS à 25% quelle que soit la couleur choisie
/// ("je n'arrive pas à obtenir des couleurs très sombres") — viser une
/// couleur PRESQUE NOIRE au sélecteur ne changeait alors STRICTEMENT RIEN
/// au résultat. Reprendre la luminosité CHOISIE (bornée) permet de piloter
/// la profondeur du thème depuis le sélecteur, pas seulement sa teinte.
///
/// Borne BASSE à 0% (PAS 15%, essayé ensuite) : demande explicite — "je
/// voudrais bien pouvoir aller jusqu'à choisir un vrai noir comme couleur de
/// base du thème" — un plancher à 15% avait été ajouté par excès de
/// prudence (garder un peu de couleur perceptible), mais `compute_palette`/
/// `adaptive_zones` gèrent DÉJÀ correctement un accent noir pur sans lui
/// (voir `push_until`, doc détaillée : bascule de sens quand assombrir
/// davantage un noir déjà pur ne peut rien changer) — ce plancher n'était
/// donc qu'une limite ARTIFICIELLE empêchant exactement ce que cette
/// fonction est censée permettre. Borne HAUTE gardée à 45% : au-delà, le
/// résultat s'écarterait de la famille "sombre" que ce sélecteur promet.
///
/// Saturation de la couleur choisie reprise TELLE QUELLE si déjà dans
/// 55%..90% (respecte un choix délibéré "je veux cette teinte moins
/// saturée"), sinon ramenée au bord le plus proche — jamais la couleur
/// choisie tout à fait telle quelle : elle peut être criarde (saturation
/// à 100%), tout l'intérêt de cette fonction est de rester "agréable". Sans
/// incidence sur un choix de noir/gris pur (`hsl_to_rgb` avec luminosité 0
/// renvoie du noir quelle que soit la saturation).
/// **LES BORNES PAR DÉFAUT** — celles qui étaient écrites en dur, et qui restent
/// le comportement tant que l'utilisateur n'y touche pas (voir `ReglagesTheme`).
pub const SATURATION_PLANCHER: f32 = 0.55;
pub const SATURATION_PLAFOND: f32 = 0.90;
pub const LUMINOSITE_PLAFOND: f32 = 0.45;

/// ⭐ **LES DEUX BORNES SONT DÉSORMAIS RÉGLABLES** — DEMANDE (2026-10-10) :
/// « je voudrais que ce soit paramétrable, luminosité et saturation ».
///
/// - **`plancher_saturation` à 0** : la couleur choisie passe **telle quelle**,
///   sans être « accentuée ». C'est ce que la demande visait ;
/// - **`plafond_luminosite`** : jusqu'où le thème assombrit. ⚠️ **Le pousser à
///   1.0 produit un thème CLAIR** — la fonction s'appelle toujours « dark » et
///   c'est l'appelant qui décide, mais le nom ne le promet plus. Écrit ici pour
///   que personne ne s'en aperçoive par surprise.
pub fn dark_theme_borne(base_rgb: [f32; 3], plancher_saturation: f32, plafond_luminosite: f32) -> [f32; 3] {
    let (hue, saturation, lightness) = rgb_to_hsl(base_rgb);
    // ⚠️ **LES BORNES SONT ELLE-MÊMES BORNÉES, ET DANS LE BON ORDRE** : un
    // plancher au-dessus du plafond ferait paniquer `clamp` (début > fin).
    let plancher = plancher_saturation.clamp(0.0, SATURATION_PLAFOND);
    let saturation = saturation.clamp(plancher, SATURATION_PLAFOND);
    let lightness = lightness.clamp(0.0, plafond_luminosite.clamp(0.0, 1.0));
    hsl_to_rgb(hue, saturation, lightness)
}

/// **LA MÊME CHOSE, AVEC LES BORNES PAR DÉFAUT** — l'enveloppe historique, gardée
/// pour que les appelants qui n'ont pas encore de réglage compilent et se
/// comportent exactement comme avant.
pub fn dark_theme_from_base_color(base_rgb: [f32; 3]) -> [f32; 3] {
    dark_theme_borne(base_rgb, SATURATION_PLANCHER, LUMINOSITE_PLAFOND)
}

/// **LA FORCE NEUTRE** des deux curseurs du thème : la couleur telle qu'elle a
/// été choisie.
pub const FORCE_NEUTRE: f32 = 1.0;
pub const FORCE_MAX: f32 = 2.0;

/// ⛔ **LA SATURATION NE MONTE PAS AU-DESSUS DU NEUTRE** — mesuré le 2026-10-10 :
/// sur `02152C` (déjà saturé à 91 %), 100 % → 200 % donnait `021225` → `001227`,
/// invisible ; l'utilisateur l'a vue « bloquée à partir d'un certain seuil ». Le
/// curseur ne fait donc plus que RETIRER de la saturation (choix de l'utilisateur,
/// préféré à un curseur en valeur absolue).
pub const FORCE_SATURATION_MAX: f32 = FORCE_NEUTRE;

/// ⭐ **LES DEUX CURSEURS DU THÈME AGISSENT SUR TOUTE LEUR COURSE.**
///
/// ⛔ Défaut réel signalé le 2026-10-10 : « le curseur de saturation n'a aucun
/// effet ». Il réglait un PLANCHER (`dark_theme_borne`) : sur *bleu nuit*
/// (saturation 0,74), tout plancher sous 0,74 ne changeait rien — et le plafond
/// de luminosité (0,40) ne touchait pas une couleur à 0,17. Des bornes ne
/// retiennent que ce qui déborde ; ici, chaque force DÉPLACE la valeur :
///
/// - **`FORCE_NEUTRE` (1)** : la valeur choisie, inchangée — à l'octet près,
///   sans aller-retour HSL ;
/// - **sous 1** : vers 0 (le gris, le noir), proportionnellement ;
/// - **au-dessus de 1** : vers 1 (la couleur pure, le blanc), d'autant.
pub fn ajuster_theme(base_rgb: [f32; 3], force_saturation: f32, force_luminosite: f32) -> [f32; 3] {
    if force_saturation == FORCE_NEUTRE && force_luminosite == FORCE_NEUTRE {
        return base_rgb;
    }
    let (hue, saturation, lightness) = rgb_to_hsl(base_rgb);
    hsl_to_rgb(hue, pousser(saturation, force_saturation), pousser(lightness, force_luminosite))
}

/// Déplace `valeur` (0..1) selon `force` (0..2) — voir `ajuster_theme`.
fn pousser(valeur: f32, force: f32) -> f32 {
    let force = force.clamp(0.0, FORCE_MAX);
    let resultat = if force <= FORCE_NEUTRE {
        valeur * force
    } else {
        valeur + (1.0 - valeur) * (force - FORCE_NEUTRE)
    };
    resultat.clamp(0.0, 1.0)
}

/// Luminance relative WCAG d'une couleur sRGB (0.0..1.0 par canal) — voir
/// https://www.w3.org/TR/WCAG21/#dfn-relative-luminance. Étape de calcul de
/// `readable_text_rgb`, la recommandation graphique standard pour garantir
/// la lisibilité d'un texte quel que soit le fond.
fn relative_luminance(rgb: [f32; 3]) -> f32 {
    fn linearize(c: f32) -> f32 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linearize(rgb[0]) + 0.7152 * linearize(rgb[1]) + 0.0722 * linearize(rgb[2])
}

/// Ratio de contraste WCAG entre deux couleurs : 1.0 (aucun contraste) à 21.0
/// (noir sur blanc, le maximum théorique) — voir `relative_luminance`.
pub(crate) fn contrast_ratio(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Couleur EFFECTIVE d'un aplat translucide `fg` posé sur le fond OPAQUE `bg` —
/// le mélange alpha « droit » (non prémultiplié), même convention que
/// `gfx::QuadInstance::color`.
///
/// Sert à juger ce que l'œil VOIT d'un élément translucide, et non sa couleur
/// déclarée : une pastille de bureaux virtuels inactive est blanche (ou noire) à
/// 35 % d'opacité (`ui::topbar::DESKTOP_DOT_INACTIVE_ALPHA`), donc la couleur
/// qu'il faut passer à `readable_text_rgb` pour un texte posé dessus est ce
/// mélange — jamais la couleur brute. Voir `ui::desktop_labels::color_for`.
pub fn composite_over(fg: [f32; 4], bg: [f32; 3]) -> [f32; 3] {
    let a = fg[3].clamp(0.0, 1.0);
    [
        fg[0] * a + bg[0] * (1.0 - a),
        fg[1] * a + bg[1] * (1.0 - a),
        fg[2] * a + bg[2] * (1.0 - a),
    ]
}

/// Presque blanc / presque noir (pas du blanc/noir pur — un peu plus doux
/// visuellement, cohérent avec le reste de cette palette) — les deux choix
/// possibles de `readable_text_rgb`.
const READABLE_LIGHT: [f32; 3] = [0.949, 0.949, 0.949];
const READABLE_DARK: [f32; 3] = [0.078, 0.078, 0.078];

/// Couleur de texte la plus lisible sur un fond `bg` : blanc ou noir (quasi),
/// celui des deux qui offre le MEILLEUR ratio de contraste WCAG — la
/// recommandation graphique standard pour garantir la lisibilité, plutôt
/// qu'un texte toujours blanc quelle que soit la couleur choisie (BUG RÉEL
/// SIGNALÉ EN PRATIQUE : illisible sur les couleurs claires/vives de
/// `WINDOWS_COLORS`, ex. jaune #FFB900 ou vert #00CC6A). `bg` : la couleur de
/// mise en avant CHOISIE elle-même (voir `win32::dash`/`win32::
/// clipboard_viewer`/`win32::settings_dash`, `style.visuals.
/// override_text_color`) — chaque fenêtre dérive ainsi son propre schéma de
/// sa propre couleur, "le dash lié" (demande explicite) plutôt qu'un
/// réglage partagé. N'affecte que le texte SANS couleur explicite
/// (`RichText::color`, ex. les titres de catégorie du Dash, volontairement
/// mis en avant dans la couleur d'accentuation elle-même) — un texte avec
/// couleur explicite reste inchangé.
pub fn readable_text_rgb(bg: [f32; 3]) -> [f32; 3] {
    if contrast_ratio(bg, READABLE_LIGHT) >= contrast_ratio(bg, READABLE_DARK) {
        READABLE_LIGHT
    } else {
        READABLE_DARK
    }
}

/// Fraction MINIMALE de `accent` réellement présente dans ce que l'œil voit
/// derrière le texte "flottant" (`Palette::accent_text`) — le reste peut
/// être n'importe quoi, du blanc au noir, selon le flou du bureau qui
/// transparaît derrière. Calé sur le plus BAS des alphas réellement
/// utilisés pour ce texte à travers les trois fenêtres `egui`
/// (`tinted_section_fill(accent, 130)` dans `win32::clipboard_viewer`,
/// 130/255 ≈ 51% — le plus faible des trois ; les fonds de section de
/// `win32::dash`/`win32::settings_dash` restent au-dessus). Sert de
/// garde-fou : GARANTIT un contraste correct même quand "Transparence"
/// laisse passer beaucoup de flou brut, pas seulement dans le cas le plus
/// favorable (fond intégralement `accent`).
pub const ACCENT_TEXT_MIN_OPACITY: f32 = 130.0 / 255.0;

/// Mélange `over` PAR-DESSUS `under` à l'opacité `alpha` (0.0..1.0) —
/// composition alpha standard (`under` à travers `1.0 - alpha`, `over` à
/// travers `alpha`), utilisée pour reconstruire un fond RÉEL connu quand un
/// calque semi-transparent (survol, sélection...) est peint par-dessus un
/// autre — voir `readable_text_rgb_guaranteed`, `overlays`.
fn blend_over(under: [f32; 3], over: [f32; 3], alpha: f32) -> [f32; 3] {
    [
        under[0] * (1.0 - alpha) + over[0] * alpha,
        under[1] * (1.0 - alpha) + over[1] * alpha,
        under[2] * (1.0 - alpha) + over[2] * alpha,
    ]
}

/// Comme `readable_text_rgb`, mais garantit le MEILLEUR contraste MINIMUM
/// simultanément sur PLUSIEURS fonds réels (au lieu d'un seul) — le cas d'un
/// texte peint UNE SEULE FOIS (`ui.label` sans état, voir les appelants) par
/// dessus une zone qui peut ensuite recevoir DIFFÉRENTS calques selon son
/// état (survol, sélection...), sans jamais revoir sa propre couleur après
/// coup. BUG RÉEL SIGNALÉ EN PRATIQUE ("superposition de couleur... rendant
/// la lecture des zones de texte difficile") : une couleur de texte
/// calculée pour un SEUL fond restait fixe même une fois un calque de
/// survol/sélection semi-transparent peint par-dessus, sans plus aucune
/// garantie sur ce nouveau mélange.
fn readable_text_rgb_multi(backgrounds: &[[f32; 3]]) -> [f32; 3] {
    let light_min = backgrounds.iter().map(|&bg| contrast_ratio(bg, READABLE_LIGHT)).fold(f32::INFINITY, f32::min);
    let dark_min = backgrounds.iter().map(|&bg| contrast_ratio(bg, READABLE_DARK)).fold(f32::INFINITY, f32::min);
    if light_min >= dark_min {
        READABLE_LIGHT
    } else {
        READABLE_DARK
    }
}

/// Comme `readable_text_rgb`, mais pour un texte qui ne flotte PAS sur `bg`
/// pur — `bg` mélangé à `min_opacity` avec un arrière-plan RÉEL inconnu
/// (voir `ACCENT_TEXT_MIN_OPACITY`). BUG RÉEL SIGNALÉ EN PRATIQUE ("le dash
/// presse-papiers est noir, les textes sont illisibles") : `readable_text_rgb`
/// pur choisissait le blanc en supposant un fond `bg` À 100% opaque
/// (contraste noir/blanc maximal, 21:1) — mais le fond RÉELLEMENT affiché
/// n'est jamais purement `bg`, toujours mélangé à une opacité MOINDRE
/// (`tinted_section_fill`/`dash_tint`) avec le flou dynamique du bureau,
/// dont la couleur n'est PAS prévisible à l'avance (documents, wallpaper,
/// fenêtres visibles derrière...). Pour un accent sombre (noir, bleu
/// marine...), si ce flou s'avère clair, le contraste RÉEL peut tomber bien
/// en dessous de ce que le calcul contre `bg` pur laissait supposer.
///
/// Calcule le contraste contre les DEUX mélanges extrêmes possibles (`bg`
/// mélangé à du blanc pur, `bg` mélangé à du noir pur — les deux bornes de
/// "n'importe quoi peut être derrière") et choisit la couleur qui garde le
/// MEILLEUR contraste MINIMUM sur ces deux cas, garantissant une lisibilité
/// correcte quel que soit ce qui se trouve réellement derrière, plutôt
/// qu'un contraste seulement "probable" dans le cas le plus favorable.
///
pub fn readable_text_rgb_guaranteed(bg: [f32; 3], min_opacity: f32) -> [f32; 3] {
    readable_text_rgb_guaranteed_with_overlays(bg, min_opacity, &[])
}

/// Comme `readable_text_rgb_guaranteed`, avec des calques SUPPLÉMENTAIRES —
/// `overlays` : calques RÉELS et CONNUS (couleur, opacité 0.0..1.0) qui
/// peuvent ensuite être peints PAR-DESSUS `bg` selon l'état (survol,
/// sélection...) — voir `win32::dash::tile`/`win32::clipboard_viewer`,
/// demande explicite ("superposition de couleur... rendant la lecture des
/// zones de texte difficile") : la couleur retournée reste garantie lisible
/// sur `bg` SEUL ET sur `bg` + CHACUN de ces calques pris isolément (jamais
/// empilés entre eux — passer les états qui se peignent RÉELLEMENT un par un,
/// jamais simultanément, voir les appelants). Vide (`&[]`) : équivalente à
/// `readable_text_rgb_guaranteed` seule.
pub fn readable_text_rgb_guaranteed_with_overlays(
    bg: [f32; 3],
    min_opacity: f32,
    overlays: &[([f32; 3], f32)],
) -> [f32; 3] {
    let blended_with = |other: [f32; 3]| -> [f32; 3] { blend_over(other, bg, min_opacity) };
    let worst_toward_white = blended_with([1.0, 1.0, 1.0]);
    let worst_toward_black = blended_with([0.0, 0.0, 0.0]);
    let mut backgrounds = vec![worst_toward_white, worst_toward_black];
    for &(overlay_rgb, alpha) in overlays {
        backgrounds.push(blend_over(worst_toward_white, overlay_rgb, alpha));
        backgrounds.push(blend_over(worst_toward_black, overlay_rgb, alpha));
    }
    readable_text_rgb_multi(&backgrounds)
}

/// Convertit RGB (0.0..1.0 par canal) en HSL (teinte 0.0..360.0, saturation
/// et luminosité 0.0..1.0) — étape de calcul de `tone`. `pub(crate)` :
/// réutilisée telle quelle par `win32::wallpaper_color` (même conversion,
/// pas de raison de la dupliquer).
pub(crate) fn rgb_to_hsl(rgb: [f32; 3]) -> (f32, f32, f32) {
    let [r, g, b] = rgb;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let delta = max - min;
    if delta.abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 { delta / (2.0 - max - min) } else { delta / (max + min) };
    let h = if max == r {
        (g - b) / delta + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    (h * 60.0, s, l)
}

/// Inverse de `rgb_to_hsl` — étape de calcul de `tone`.
pub(crate) fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s.abs() < 1e-6 {
        return [l, l, l];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let h = h / 360.0;
    let hue_to_rgb = |p: f32, q: f32, t: f32| {
        let t = if t < 0.0 { t + 1.0 } else if t > 1.0 { t - 1.0 } else { t };
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 1.0 / 2.0 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [hue_to_rgb(p, q, h + 1.0 / 3.0), hue_to_rgb(p, q, h), hue_to_rgb(p, q, h - 1.0 / 3.0)]
}

/// Décale la LUMINOSITÉ HSL de `rgb` de `delta` (positif = plus clair,
/// négatif = plus sombre, résultat toujours borné à 0.0..1.0) — teinte ET
/// saturation inchangées. Même principe que les 6 variantes de couleur
/// d'accentuation (`AccentColorLight1/2/3`, `AccentColorDark1/2/3`) que
/// Windows lui-même dérive d'une seule couleur système — voir "Color in
/// Windows" (https://learn.microsoft.com/windows/apps/design/signature-experiences/color) :
/// "dans les deux modes de couleur, les couleurs plus sombres indiquent des
/// surfaces d'arrière-plan de moindre importance ; les surfaces importantes
/// sont mises en avant par des couleurs plus claires/vives" — et les
/// "palettes tonales" de Material Design 3, qui dérivent tous les rôles de
/// couleur d'une interface (surface, contenant, texte...) d'une seule
/// couleur source EN NE FAISANT VARIER QUE LA LUMINOSITÉ
/// (https://m3.material.io/styles/color/roles) : garder teinte/saturation
/// fixes est ce qui garde un jeu de couleurs dérivées "de la même famille"
/// visuellement, plutôt que des gris neutres sans rapport avec la couleur
/// choisie par l'utilisateur.
pub fn tone(rgb: [f32; 3], delta: f32) -> [f32; 3] {
    let (h, s, l) = rgb_to_hsl(rgb);
    hsl_to_rgb(h, s, (l + delta).clamp(0.0, 1.0))
}

// ─────────────────────────────────────────────────────────────────────────────
// PALETTES HARMONIEUSES — applet « Sélecteur de couleurs »
//
// DEMANDE EXPLICITE (2026-09-27) : une palette esthétique dérivée de la couleur
// choisie. Les schémas sont ceux, classiques, de n'importe quel générateur de
// palette — et le principe est celui que `icon_palette` applique déjà plus bas
// (teinte de départ + copies décalées à intervalle FIXE), étendu ici à tout le
// cercle plutôt qu'à six icônes de repli.
//
// **POURQUOI CES FONCTIONS VIVENT DANS CE FICHIER** et non dans l'applet :
// `hsl_to_rgb` est PRIVÉE, et c'est délibéré — la doc de `compute_palette`
// rappelle qu'un refactor a justement servi à supprimer un second chemin de
// dérivation des mêmes rôles. Une copie locale de la conversion, ou son
// élargissement à `pub(crate)`, rouvrirait exactement ce qu'on a fermé. Ici,
// `hsl_to_rgb` reste privée et la teinte ne se dérive qu'à un seul endroit.

/// Un groupe de la palette : son nom affiché, et ses teintes.
pub struct GroupeTeintes {
    pub nom: &'static str,
    pub teintes: Vec<[f32; 3]>,
}

/// Écart, en degrés, des deux teintes ANALOGUES de part et d'autre de la base.
/// 30° est le pas usuel des schémas analogiques — assez pour que les trois
/// couleurs se distinguent, assez peu pour qu'elles se lisent comme une
/// famille. (`gfx::degrade` utilise ±0,07 tour en HSV pour son propre tirage
/// au hasard, soit 25° : même ordre de grandeur, autre espace, autre usage.)
const ANALOGUES_ECART: f32 = 30.0;

/// Palier de luminosité du groupe monochrome — cinq marches de 0,15, du plus
/// clair au plus foncé (voir `tone`).
const MONOCHROME_PALIER: f32 = 0.15;

/// **LES CINQ SCHÉMAS HARMONIEUX DÉRIVÉS D'UNE COULEUR**, dans un ORDRE FIXE :
/// « Complémentaire », « Analogues », « Triade », « Quadriade »,
/// « Monochrome » (clair → foncé).
///
/// La couleurs de départ figure dans « Triade » et « Quadriade », où elle fait
/// partie du schéma — un triadique EST la base plus ses deux décalages. Elle
/// n'est donc PAS répétée dans « Complémentaire » ni « Analogues », qui ne
/// nomment que ce qu'ils ajoutent. `aplatir` dédoublonne pour l'export.
///
/// **TOTALE** : jamais de panique, et toujours cinq groupes, y compris sur un
/// gris — dont la teinte est indéfinie, et sur lequel les décalages de teinte
/// rendent donc tous la même couleur. C'est le comportement juste : un gris
/// n'a pas de complémentaire.
pub fn harmonies(base: [f32; 3]) -> Vec<GroupeTeintes> {
    let teinte = |degres: f32| teinte_decalee(base, degres);
    vec![
        GroupeTeintes { nom: "Complémentaire", teintes: vec![teinte(180.0)] },
        GroupeTeintes {
            nom: "Analogues",
            teintes: vec![teinte(-ANALOGUES_ECART), teinte(ANALOGUES_ECART)],
        },
        GroupeTeintes { nom: "Triade", teintes: vec![base, teinte(120.0), teinte(240.0)] },
        GroupeTeintes {
            nom: "Quadriade",
            teintes: vec![base, teinte(90.0), teinte(180.0), teinte(270.0)],
        },
        GroupeTeintes {
            nom: "Monochrome",
            teintes: [2.0, 1.0, 0.0, -1.0, -2.0]
                .iter()
                .map(|marche| tone(base, marche * MONOCHROME_PALIER))
                .collect(),
        },
    ]
}

/// La couleur de départ, puis toutes les teintes des groupes, **sans
/// doublon** — ce qu'un export `.gpl` écrit.
#[cfg(test)]
pub fn aplatir(base: [f32; 3], groupes: &[GroupeTeintes]) -> Vec<[f32; 3]> {
    let mut out = vec![base];
    for c in groupes.iter().flat_map(|g| g.teintes.iter().copied()) {
        if !out.iter().any(|d| proche(*d, c)) {
            out.push(c);
        }
    }
    out
}

/// Décale la TEINTE HSL de `degres`, luminosité et saturation inchangées —
/// pendant chromatique de `tone`, qui ne déplace que la luminosité.
fn teinte_decalee(base: [f32; 3], degres: f32) -> [f32; 3] {
    let (h, s, l) = rgb_to_hsl(base);
    hsl_to_rgb((h + degres).rem_euclid(360.0), s, l)
}

/// Deux couleurs assez proches pour être LA MÊME à l'affichage. La tolérance
/// n'est pas de la coquetterie : `tone(base, 0.0)` fait un aller-retour
/// RGB → HSL → RGB qui ne rend pas exactement `base` en f32, et sans elle la
/// couleur de départ apparaîtrait deux fois dans `aplatir`. Elle reste très
/// en deçà de l'écart entre deux teintes réellement distinctes (30° de teinte
/// séparent des couleurs bien plus éloignées que 1/1000).
#[cfg(test)]
fn proche(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

/// Fond de zone (tuile/bouton/ligne de liste) au repos et au survol, plus le
/// texte le plus lisible par-dessus — dérivés ENTIÈREMENT de la couleur de
/// mise en avant choisie par l'utilisateur (voir `tone`, `readable_text_rgb`).
pub struct AdaptiveZones {
    /// Fond d'une zone au repos (tuile, bouton, ligne de liste...).
    pub bg: [f32; 3],
    /// Fond de la même zone au survol — décalage deux fois plus faible que
    /// `bg` vers `accent_rgb`, donc visuellement "entre les deux".
    pub bg_hovered: [f32; 3],
    /// Texte le plus lisible par-dessus `bg` (voir `readable_text_rgb`) —
    /// PAS forcément le même choix (blanc/noir quasi) que le texte posé
    /// directement sur `accent_rgb` : `bg` est volontairement plus sombre
    /// (thème sombre) ou plus clair (thème clair) que l'accent brut.
    pub text: [f32; 3],
    /// Texte le plus lisible par-dessus `bg_hovered` — CALCULÉ SÉPARÉMENT de
    /// `text` (BUG RÉEL SIGNALÉ EN PRATIQUE avec une seule couleur réutilisée
    /// pour les deux : "dans certains cas la zone sombre derrière le texte
    /// rend le texte ou certaine icône illisible" — `bg` et `bg_hovered`
    /// n'ont pas la même luminosité, rien ne garantit qu'ils partagent le
    /// même choix optimal blanc quasi/noir quasi ; réutiliser `text` au
    /// survol pouvait alors tomber du mauvais côté du seuil de contraste
    /// WCAG précisément sur les boutons/glyphes-icônes — 🔄/✖/🗑 compris,
    /// peints avec la même couleur que le texte via `WidgetVisuals::
    /// fg_stroke`, voir `egui_overlay::apply_adaptive_theme`).
    pub text_hovered: [f32; 3],
}

/// Contraste WCAG minimal (texte normal, niveau AA — voir
/// https://www.w3.org/TR/WCAG21/#contrast-minimum) EXIGÉ entre un fond de
/// zone et son texte, PAS seulement "le meilleur des deux choix possibles"
/// (voir `readable_text_rgb`, qui reste limité à choisir entre blanc/noir
/// quasi — il ne peut rien garantir tout seul si les DEUX options sont
/// médiocres). Demande explicite : "améliore l'algorithme... en pensant aux
/// daltoniens" — pour la plupart des formes de daltonisme (deutéranopie/
/// protanopie, la teinte rouge-vert), c'est justement le contraste de
/// LUMINANCE (indépendant de la perception des teintes) qui reste le seul
/// repère fiable ; un delta de teinte fixe (voir l'ancienne version de
/// `adaptive_zones`) pouvait laisser passer un accent proche du gris moyen
/// avec un contraste à peine meilleur que 3:1, "techniquement" le meilleur
/// choix mais insuffisant en pratique. Voir `ensure_min_contrast`, qui fait
/// respecter ce plancher.
const MIN_ZONE_TEXT_CONTRAST: f32 = 4.5;

/// Contraste WCAG minimal exigé entre un fond de zone et le fond AMBIANT sur
/// lequel elle est posée (l'accent lui-même, dont dérivent `window_fill`/le
/// fond de section — voir `win32::egui_overlay::tinted_section_fill`) — voir
/// SC 1.4.11 "Non-text Contrast" (https://www.w3.org/TR/WCAG21/#non-text-contrast),
/// qui vise 3:1 pour les limites de composants d'interface. SANS CE PLANCHER
/// (BUG RÉEL CONSTATÉ PAR CAPTURE D'ÉCRAN DIRECTE DU BACK BUFFER, PAS
/// SEULEMENT PAR LECTURE DE CODE — voir la conversation, "vérifie tes
/// réglages tu verras...") : `MIN_ZONE_TEXT_CONTRAST` ci-dessus garantit
/// qu'un bon texte existe SUR `bg`, mais rien ne garantissait que `bg`
/// LUI-MÊME se distingue de ce qui l'entoure — une case à cocher pouvait
/// rester quasiment invisible en tant que carré (seule sa coche restait
/// perceptible), l'accent choisi et `bg` finissant trop proches en pratique.
pub(crate) const MIN_ZONE_BOUNDARY_CONTRAST: f32 = 3.0;

/// Pousse la luminosité HSL de `base` (voir `tone`) dans le sens
/// d'`initial_delta`, par pas de 0.05 (12 au plus), jusqu'à ce que
/// `satisfies` accepte le résultat — PUIS, si ce sens SATURE à 0.0/1.0 sans
/// jamais satisfaire `satisfies` (`base` déjà à l'extrémité de ce sens —
/// ex. noir pur en thème sombre, `tone` ne peut alors produire AUCUN résultat
/// différent), RÉESSAIE dans le sens OPPOSÉ, par la même méthode. BUG RÉEL
/// CONSTATÉ EN PRATIQUE (capture d'écran directe) avec un accent NOIR PUR
/// choisi en thème sombre — voir la conversation : "en sélectionnant un fond
/// noir certains textes des dash sont difficilement lisibles". Les deux
/// appelants (`ensure_min_contrast`, `ensure_contrast_against`) poussaient
/// TOUJOURS dans le sens dicté par `dark_mode` (plus sombre en thème sombre),
/// mais le noir pur ne peut PHYSIQUEMENT PAS s'assombrir davantage — sans ce
/// repli, le résultat restait alors IDENTIQUE à `base` (contraste 1:1 avec
/// lui-même), l'exact opposé de la garantie visée par ces deux fonctions.
pub(crate) fn push_until(base: [f32; 3], initial_delta: f32, satisfies: impl Fn([f32; 3]) -> bool) -> [f32; 3] {
    let magnitude = initial_delta.abs();
    let mut best = base;
    for sign in [initial_delta.signum(), -initial_delta.signum()] {
        let mut delta = sign * magnitude;
        let mut result = tone(base, delta);
        for _ in 0..12 {
            best = result;
            if satisfies(result) {
                return result;
            }
            delta += sign * 0.05;
            let next = tone(base, delta);
            if next == result {
                break; // saturé dans CE sens : la boucle `for sign` essaie l'autre
            }
            result = next;
        }
    }
    // Ni un sens ni l'autre n'a suffi (cas extrême : `base` déjà à michemin
    // d'un contraste maximal insuffisant des deux côtés — en pratique
    // n'arrive jamais pour `MIN_ZONE_TEXT_CONTRAST`/`MIN_ZONE_BOUNDARY_CONTRAST`,
    // chacun des deux sens atteint 0.0 ou 1.0, contraste maximal, bien avant
    // la fin des 12 pas) : le dernier résultat calculé reste le meilleur
    // disponible.
    best
}

/// Pousse `tone(base, delta)` plus loin (voir `push_until`) jusqu'à ce que
/// DEUX conditions soient vraies à la fois : un bon texte existe dessus
/// (`MIN_ZONE_TEXT_CONTRAST`, voir `readable_text_rgb`) ET la zone se
/// distingue de `base` elle-même (`MIN_ZONE_BOUNDARY_CONTRAST`, voir sa doc).
fn ensure_min_contrast(base: [f32; 3], initial_delta: f32) -> [f32; 3] {
    push_until(base, initial_delta, |bg| {
        contrast_ratio(bg, readable_text_rgb(bg)) >= MIN_ZONE_TEXT_CONTRAST
            && contrast_ratio(bg, base) >= MIN_ZONE_BOUNDARY_CONTRAST
    })
}

/// Calcule `AdaptiveZones` pour la couleur de mise en avant `accent_rgb` —
/// demande explicite : "quand je choisis une couleur... les couleurs des
/// textes ET DES ZONES s'adaptent automatiquement pour être lisibles et
/// agréables visuellement" (voir `win32::egui_overlay::apply_adaptive_theme`,
/// qui applique ce résultat à un `egui::Style` complet). `dark_mode` : sens
/// du décalage de luminosité (voir `tone`) — zones plus SOMBRES que l'accent
/// en thème sombre, plus CLAIRES en thème clair, comme Windows lui-même
/// (même principe que `soften_tint`, voir sa doc). Le contraste texte/fond
/// est GARANTI (voir `ensure_min_contrast`), pas seulement "probable" —
/// important pour les daltoniens (voir sa doc).
pub fn adaptive_zones(accent_rgb: [f32; 3], dark_mode: bool) -> AdaptiveZones {
    let delta = if dark_mode { -0.32 } else { 0.32 };
    let bg = ensure_min_contrast(accent_rgb, delta);
    let bg_hovered = ensure_min_contrast(accent_rgb, delta * 0.5);
    AdaptiveZones { bg, bg_hovered, text: readable_text_rgb(bg), text_hovered: readable_text_rgb(bg_hovered) }
}

/// Variante de `accent_rgb`, MÊME teinte (voir `tone`), poussée JUSTE ASSEZ
/// en luminosité pour rester lisible même posée sur un fond qui est
/// LUI-MÊME teinté par ce même accent — le cas du `window_fill` des trois
/// fenêtres `egui` (voir `win32::dash`/`win32::clipboard_viewer`,
/// `EguiOverlay::frame`, `tint_rgba`). BUG RÉEL SIGNALÉ EN PRATIQUE : "certains
/// mix de couleur rendent les textes difficiles à lire" — un texte peint
/// dans `accent_rgb` PLEIN (`RichText::color`, ex. les titres de catégorie
/// du Dash) posé sur un fond de la MÊME famille de couleur (même teinte,
/// juste une opacité différente) a un contraste insuffisant PAR
/// CONSTRUCTION, quel que soit l'accent choisi — même deux couleurs qui
/// "paraissent différentes" peuvent partager presque la même luminance (voir
/// https://webaim.org/articles/contrast/, "same hue" : le contraste WCAG
/// dépend de la LUMINANCE, pas de la teinte perçue).
///
/// Volontairement plus PROCHE de l'accent d'origine que `adaptive_zones.bg`
/// (delta de départ 0.12 au lieu de 0.32) : contrairement à un fond de zone,
/// qui doit se distinguer nettement de l'accent pour former une vraie
/// "surface", cette couleur doit encore SE LIRE comme "l'accent" — juste le
/// minimum garanti nécessaire pour rester lisible sur un fond de la même
/// famille. `dark_mode` : même convention de sens que `adaptive_zones`.
///
/// Utilise `ensure_contrast_against` (PAS `ensure_min_contrast`, qui vérifie
/// une propriété différente : que le résultat ait lui-même un BON texte
/// possible dessus, pas qu'il soit LUI-MÊME lisible contre `accent_rgb`).
pub fn accent_emphasis_rgb(accent_rgb: [f32; 3], dark_mode: bool) -> [f32; 3] {
    accent_emphasis_rgb_against(accent_rgb, accent_rgb, dark_mode)
}

/// Généralisation d'`accent_emphasis_rgb` : le texte est peint dans
/// `color_rgb` (SA propre teinte, voir `tone`), pas nécessairement CELLE du
/// fond réel sur lequel il flotte (`reference_rgb`) — nécessaire depuis
/// `ui::color::WindowAccent` (couleur de fond ET couleur de palette
/// SÉPARÉES, demande explicite : "il faut choisir la couleur de fond mais
/// aussi une couleur pour la palette automatique des éléments") : le
/// `window_fill` réel derrière un titre de catégorie (voir `win32::dash`)
/// suit maintenant `WindowAccent::background`, tandis que le titre lui-même
/// doit rester dans la couleur de PALETTE — deux couleurs potentiellement
/// différentes, alors qu'`accent_emphasis_rgb` suppose encore les deux
/// identiques (son usage RESTE correct partout où c'est le cas, ex.
/// `win32::popup_menu`, qui n'a pas cette distinction fond/palette).
pub fn accent_emphasis_rgb_against(color_rgb: [f32; 3], reference_rgb: [f32; 3], dark_mode: bool) -> [f32; 3] {
    let delta = if dark_mode { -0.12 } else { 0.12 };
    ensure_contrast_against(color_rgb, reference_rgb, delta)
}

/// Pôle "avant-plan" FIXE des popups CEF de type "menu" (voir
/// `win32::cef_popup::push_menu_palette`) — DEMANDE EXPLICITE ("les menus
/// ont du texte noir et ça ne me plait pas, simplifie les possibilités,
/// limites-les à du blanc, du gris clair, du noir et du gris foncé, les
/// menus devraient s'adapter automatiquement").
///
/// DEUX tentatives précédentes ont toutes les deux produit du noir que
/// l'utilisateur n'aimait pas : (1) `compute_palette`/`readable_text_rgb*`
/// dérivaient ce choix du CONTRASTE contre l'accent brut choisi par
/// l'utilisateur — retombe sur du noir dès que cet accent est assez clair ;
/// (2) une version choisissant selon `win32::theme::prefers_dark_mode`
/// (thème clair/sombre WINDOWS) — retombe sur du noir dès que Windows dit
/// "clair", alors que le fond RÉEL de ces popups est un flou teinté par
/// `win32::menu::menu_accent_rgb` (une couleur choisie librement par
/// l'utilisateur, SANS AUCUN rapport avec le thème système) — DEMANDE
/// EXPLICITE de suite : "j'ai un thème clair mais des éléments en noir dans
/// les menus, cela ne me plait pas". Les DEUX dépendaient d'un signal qui ne
/// correspond pas à ce qui est réellement peint derrière le texte.
///
/// Ce module ne gère qu'UN thème de fond pour ces popups (glace teintée,
/// jamais une carte blanche pleine) : `menu_fg` reste donc FIXE, blanc
/// quasi (`READABLE_LIGHT`) — le pôle qui reste lisible sur la quasi-
/// totalité des couleurs d'accentuation Windows réelles (teintes moyennes à
/// saturées, rarement pastel). `menu_on_fg` (noir quasi,
/// `READABLE_DARK`) reste réservé au SEUL cas où du texte doit se lire
/// PAR-DESSUS un aplat plein de `menu_fg` lui-même (voir
/// `push_menu_palette`, `--menu-on-accent-rgb` : la pastille "aujourd'hui"
/// du calendrier) — jamais utilisé comme couleur de texte/élément
/// principale. "Gris clair" (respectivement "gris foncé") n'est pas un ton
/// RVB séparé : c'est `menu_fg` (resp. `menu_on_fg`) vu à travers
/// l'alpha déjà réglé au cas par cas par chaque popup (`assets/popups/
/// *.html`, ex. `rgba(var(--menu-zone-rgb), 0.15)` pour un survol) sur le
/// fond flouté — inutile d'introduire un troisième/quatrième ton RVB pour
/// obtenir les quatre teintes demandées.
///
/// **SAUF EN MODE CLAIR** (voir `mode_clair`) : le fond n'y est plus une glace
/// teintée mais une surface claire quasi opaque, et les deux pôles
/// S'ÉCHANGENT — d'où des fonctions et non plus des constantes.
pub fn menu_fg() -> [f32; 3] {
    poles(mode_clair()).0
}

/// Pôle opposé à `menu_fg` — voir sa doc.
pub fn menu_on_fg() -> [f32; 3] {
    poles(mode_clair()).1
}

/// `(texte, texte posé sur un aplat de texte)` selon le mode. **FONCTION PURE,
/// ET TESTÉE** : c'est elle qui décide si le texte se lit.
fn poles(clair: bool) -> ([f32; 3], [f32; 3]) {
    if clair {
        (READABLE_DARK, READABLE_LIGHT)
    } else {
        (READABLE_LIGHT, READABLE_DARK)
    }
}

/// **LE MODE CLAIR DE NEURADESK** — DEMANDE EXPLICITE (2026-09-26) : « ajoutes
/// 2 thèmes sélectionnables dans les réglages qui correspondent au thème clair
/// et au thème sombre de windows ». Posé par le thème « Windows clair » (voir
/// `win32::cef_settings`, action `theme windows`), retiré par tout autre thème.
///
/// Un GLOBAL et non un paramètre : le pôle de texte est lu par des dizaines de
/// sites (barres, infobulles, aperçus, pages CEF) qui n'ont aucun état commun —
/// même choix que `win32::menu::menu_accent_rgb`. Tout tourne sur le fil
/// d'interface, l'atomique ne sert qu'à éviter un `static mut`.
///
/// Persisté sous `CLE_MODE_CLAIR` (voir `config::save_bool_setting`), relu au
/// démarrage par `app::run`.
static MODE_CLAIR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Nom du réglage booléen qui persiste `MODE_CLAIR`.
pub const CLE_MODE_CLAIR: &str = "theme_windows_clair";

pub fn mode_clair() -> bool {
    MODE_CLAIR.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_mode_clair(clair: bool) {
    MODE_CLAIR.store(clair, std::sync::atomic::Ordering::Relaxed);
}

/// L'ACCENT de la famille de thèmes choisie (voir `FAMILLES_THEMES`), `None`
/// hors famille. DEMANDE EXPLICITE (2026-10-11) : « que les nouveaux thèmes
/// aient plus d'influence sur les éléments de l'interface » — les barres ne
/// voyaient que la SURFACE de la famille : l'indicateur d'application lancée et
/// la pastille du bureau actif prenaient la couleur de la barre elle-même, à
/// peine visibles dessus. Global et non par `Dock` : les docks des écrans
/// secondaires ne recopient pas ces couleurs (voir `app::run`).
static ACCENT_DU_THEME: std::sync::Mutex<Option<[f32; 3]>> = std::sync::Mutex::new(None);

pub fn accent_du_theme() -> Option<[f32; 3]> {
    *ACCENT_DU_THEME.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_accent_du_theme(accent: Option<[f32; 3]>) {
    *ACCENT_DU_THEME.lock().unwrap_or_else(|e| e.into_inner()) = accent;
}

/// Le `dark_mode` que doit voir le dessin : celui de Windows, SAUF en mode
/// clair, qui l'emporte (un fond clair sous des palettes calculées « sombre »
/// donnerait des accents illisibles).
pub fn sombre_effectif(windows_sombre: bool) -> bool {
    sombre_effectif_selon(windows_sombre, mode_clair())
}

fn sombre_effectif_selon(windows_sombre: bool, clair: bool) -> bool {
    windows_sombre && !clair
}

/// Opacité MINIMALE de la teinte des fonds en mode clair. Une surface claire
/// ne tient que si elle masque le bureau : à la transparence habituelle, un
/// fond d'écran sombre traverserait et le texte sombre deviendrait illisible.
pub const TEINTE_MIN_MODE_CLAIR: f32 = 0.88;

/// Les surfaces des thèmes Windows : `#F3F3F3` (clair) et `#202020` (sombre),
/// les fonds Mica de Windows 11.
pub const SURFACE_WINDOWS_CLAIR: [f32; 3] = [0xF3 as f32 / 255.0, 0xF3 as f32 / 255.0, 0xF3 as f32 / 255.0];
pub const SURFACE_WINDOWS_SOMBRE: [f32; 3] = [0x20 as f32 / 255.0, 0x20 as f32 / 255.0, 0x20 as f32 / 255.0];

/// **UNE FAMILLE DE THÈMES : UNE VARIANTE CLAIRE ET UNE SOMBRE** — DEMANDE
/// EXPLICITE (2026-10-11) : « des variations du thème clair et du thème sombre
/// de l'app », chaque famille avec « sa propre couleur d'accentuation ».
/// L'applet de bascule du thème de Windows passe d'une variante à l'autre de la
/// famille choisie (voir `cef_settings::follow_windows_toggle`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FamilleTheme {
    pub id: &'static str,
    pub nom: &'static str,
    /// `None` : pas de variante claire (« Noir profond »).
    pub clair: Option<[f32; 3]>,
    pub sombre: [f32; 3],
    /// `None` : la couleur d'accentuation de Windows (famille « Windows »).
    pub accent: Option<[f32; 3]>,
}

/// ⚠️ Les contrastes sont TESTÉS (`les_familles_se_lisent`) : texte ≥ 4,5 sur
/// chaque surface, accent ≥ 3 sur chacune.
pub const FAMILLES_THEMES: [FamilleTheme; 228] = [
    FamilleTheme { id: "windows", nom: "Windows", clair: Some(SURFACE_WINDOWS_CLAIR), sombre: SURFACE_WINDOWS_SOMBRE, accent: None },
    FamilleTheme { id: "papier", nom: "Papier", clair: Some(rgb3(0xF6, 0xF1, 0xE7)), sombre: rgb3(0x1C, 0x1A, 0x17), accent: Some(rgb3(0xC0, 0x60, 0x2F)) },
    FamilleTheme { id: "ardoise", nom: "Ardoise", clair: Some(rgb3(0xEE, 0xF1, 0xF5)), sombre: rgb3(0x1E, 0x23, 0x2B), accent: Some(rgb3(0x3D, 0x7F, 0xD6)) },
    FamilleTheme { id: "sauge", nom: "Sauge", clair: Some(rgb3(0xEE, 0xF3, 0xEE)), sombre: rgb3(0x1B, 0x22, 0x1D), accent: Some(rgb3(0x3F, 0x8F, 0x5B)) },
    FamilleTheme { id: "lavande", nom: "Lavande", clair: Some(rgb3(0xF2, 0xF0, 0xF7)), sombre: rgb3(0x20, 0x1D, 0x28), accent: Some(rgb3(0x84, 0x66, 0xD1)) },
    FamilleTheme { id: "noir", nom: "Noir profond", clair: None, sombre: rgb3(0x0A, 0x0A, 0x0A), accent: Some(rgb3(0x2E, 0xA8, 0xD8)) },
    // Des palettes connues des éditeurs de code, aux couleurs de leurs auteurs
    // (l'accent est parfois foncé pour tenir le contraste sur les deux surfaces).
    FamilleTheme { id: "nord", nom: "Nord", clair: Some(rgb3(0xEC, 0xEF, 0xF4)), sombre: rgb3(0x2E, 0x34, 0x40), accent: Some(rgb3(0x5E, 0x81, 0xAC)) },
    FamilleTheme { id: "solarized", nom: "Solarized", clair: Some(rgb3(0xFD, 0xF6, 0xE3)), sombre: rgb3(0x00, 0x2B, 0x36), accent: Some(rgb3(0x26, 0x8B, 0xD2)) },
    FamilleTheme { id: "gruvbox", nom: "Gruvbox", clair: Some(rgb3(0xFB, 0xF1, 0xC7)), sombre: rgb3(0x28, 0x28, 0x28), accent: Some(rgb3(0xD6, 0x5D, 0x0E)) },
    FamilleTheme { id: "catppuccin", nom: "Catppuccin", clair: Some(rgb3(0xEF, 0xF1, 0xF5)), sombre: rgb3(0x1E, 0x1E, 0x2E), accent: Some(rgb3(0x9A, 0x4D, 0xF5)) },
    FamilleTheme { id: "tokyo", nom: "Tokyo Night", clair: Some(rgb3(0xE1, 0xE2, 0xE7)), sombre: rgb3(0x1A, 0x1B, 0x26), accent: Some(rgb3(0x2E, 0x7D, 0xE9)) },
    FamilleTheme { id: "rosepine", nom: "Rosé Pine", clair: Some(rgb3(0xFA, 0xF4, 0xED)), sombre: rgb3(0x19, 0x17, 0x24), accent: Some(rgb3(0xB4, 0x63, 0x7A)) },
    FamilleTheme { id: "everforest", nom: "Everforest", clair: Some(rgb3(0xFD, 0xF6, 0xE3)), sombre: rgb3(0x2D, 0x35, 0x3B), accent: Some(rgb3(0x7A, 0x9A, 0x01)) },
    FamilleTheme { id: "github", nom: "GitHub", clair: Some(rgb3(0xFF, 0xFF, 0xFF)), sombre: rgb3(0x0D, 0x11, 0x17), accent: Some(rgb3(0x09, 0x69, 0xDA)) },
    FamilleTheme { id: "one", nom: "One", clair: Some(rgb3(0xFA, 0xFA, 0xFA)), sombre: rgb3(0x28, 0x2C, 0x34), accent: Some(rgb3(0x40, 0x78, 0xF2)) },
    FamilleTheme { id: "sepia", nom: "Sépia", clair: Some(rgb3(0xF4, 0xEC, 0xD8)), sombre: rgb3(0x2B, 0x21, 0x18), accent: Some(rgb3(0x9C, 0x6B, 0x30)) },
    FamilleTheme { id: "dracula", nom: "Dracula", clair: None, sombre: rgb3(0x28, 0x2A, 0x36), accent: Some(rgb3(0xBD, 0x93, 0xF9)) },
    FamilleTheme { id: "breeze", nom: "Breeze", clair: Some(rgb3(0xEF, 0xF0, 0xF1)), sombre: rgb3(0x20, 0x23, 0x26), accent: Some(rgb3(0x18, 0x91, 0xD1)) },
    // DEMANDE EXPLICITE (2026-10-12) : « un style gnome comme sur linux » — les
    // fonds de fenêtre de libadwaita (GNOME 47) et son accent bleu par défaut,
    // à combiner avec le style « GNOME ».
    FamilleTheme { id: "adwaita", nom: "Adwaita", clair: Some(rgb3(0xFA, 0xFA, 0xFB)), sombre: rgb3(0x22, 0x22, 0x26), accent: Some(rgb3(0x35, 0x84, 0xE4)) },
    // DEMANDE EXPLICITE (2026-10-12) : COSMIC, elementary et Cinnamon « chacun
    // avec sa famille de couleurs, plus la famille Yaru seule ». Un accent sert
    // les DEUX variantes : le cyan de COSMIC (#63D0DF) n'atteint que 1,5 sur
    // le fond clair, d'où sa nuance foncée ; même raison pour le vert de Mint.
    FamilleTheme { id: "cosmic", nom: "COSMIC", clair: Some(rgb3(0xE7, 0xE7, 0xE7)), sombre: rgb3(0x1B, 0x1B, 0x1B), accent: Some(rgb3(0x00, 0x90, 0xA3)) },
    FamilleTheme { id: "elementary", nom: "elementary", clair: Some(rgb3(0xF6, 0xF4, 0xEF)), sombre: rgb3(0x27, 0x34, 0x45), accent: Some(rgb3(0x36, 0x89, 0xE6)) },
    FamilleTheme { id: "mint", nom: "Mint-Y", clair: Some(rgb3(0xF0, 0xF0, 0xF0)), sombre: rgb3(0x2E, 0x2E, 0x2E), accent: Some(rgb3(0x2E, 0x9A, 0x4A)) },
    // Ubuntu : l'aubergine en sombre, l'orange d'Ubuntu en accent.
    // DEMANDE EXPLICITE (2026-10-12) : « le thème android » — les surfaces de
    // la palette de référence de Material 3. Son violet #6750A4 n'atteint que
    // 2,9 sur le fond sombre : un ton plus clair, lisible sur les deux.
    FamilleTheme { id: "materialyou", nom: "Material You", clair: Some(rgb3(0xFE, 0xF7, 0xFF)), sombre: rgb3(0x14, 0x12, 0x18), accent: Some(rgb3(0x79, 0x65, 0xAF)) },
    FamilleTheme { id: "yaru", nom: "Yaru", clair: Some(rgb3(0xFA, 0xFA, 0xFA)), sombre: rgb3(0x2C, 0x00, 0x1E), accent: Some(rgb3(0xE9, 0x54, 0x20)) },
    // DEMANDE EXPLICITE (2026-10-12) : « un style deepin ainsi que ses thèmes ».
    // Les fonds de fenêtre de DTK, clair et sombre, et le bleu deepin par défaut
    // (#0081FF), qui atteint 3:1 sur les deux.
    FamilleTheme { id: "deepin", nom: "deepin", clair: Some(rgb3(0xF8, 0xF8, 0xF8)), sombre: rgb3(0x25, 0x25, 0x25), accent: Some(rgb3(0x00, 0x81, 0xFF)) },
    // DEMANDE EXPLICITE (2026-10-12) : « les thèmes des distributions linux
    // principales » — chacune à la couleur de sa marque (Ubuntu, Mint,
    // elementary et deepin sont déjà plus haut). ⚠️ Un accent sert les DEUX
    // variantes et doit tenir 3:1 sur chacune : les verts d'openSUSE (#73BA25)
    // et de Manjaro (#35BF5C), le cyan de Pop!_OS (#48B9C7) et le bleu de Zorin
    // (#15A6F0) sont foncés d'un ton ; le violet d'EndeavourOS (#7F3FBF) et
    // celui de Gentoo (#54487A) éclaircis.
    FamilleTheme { id: "fedora", nom: "Fedora", clair: Some(rgb3(0xF6, 0xF8, 0xFB)), sombre: rgb3(0x1C, 0x24, 0x33), accent: Some(rgb3(0x3C, 0x6E, 0xB4)) },
    FamilleTheme { id: "debian", nom: "Debian", clair: Some(rgb3(0xF6, 0xF5, 0xF4)), sombre: rgb3(0x24, 0x1F, 0x31), accent: Some(rgb3(0xD7, 0x0A, 0x53)) },
    FamilleTheme { id: "arch", nom: "Arch", clair: Some(rgb3(0xF5, 0xF5, 0xF5)), sombre: rgb3(0x17, 0x1A, 0x21), accent: Some(rgb3(0x17, 0x93, 0xD1)) },
    FamilleTheme { id: "opensuse", nom: "openSUSE", clair: Some(rgb3(0xF5, 0xF7, 0xF6)), sombre: rgb3(0x17, 0x3F, 0x4F), accent: Some(rgb3(0x5A, 0x9A, 0x1E)) },
    FamilleTheme { id: "manjaro", nom: "Manjaro", clair: Some(rgb3(0xF5, 0xF5, 0xF5)), sombre: rgb3(0x2C, 0x2C, 0x2C), accent: Some(rgb3(0x2A, 0x9D, 0x4A)) },
    FamilleTheme { id: "popos", nom: "Pop!_OS", clair: Some(rgb3(0xFA, 0xFA, 0xFA)), sombre: rgb3(0x33, 0x31, 0x30), accent: Some(rgb3(0x2B, 0x9A, 0xA8)) },
    FamilleTheme { id: "zorin", nom: "Zorin", clair: Some(rgb3(0xF5, 0xF7, 0xFA)), sombre: rgb3(0x1E, 0x22, 0x28), accent: Some(rgb3(0x0C, 0x8A, 0xD0)) },
    FamilleTheme { id: "kali", nom: "Kali", clair: Some(rgb3(0xF5, 0xF5, 0xF5)), sombre: rgb3(0x23, 0x25, 0x2E), accent: Some(rgb3(0x36, 0x7B, 0xF0)) },
    FamilleTheme { id: "endeavouros", nom: "EndeavourOS", clair: Some(rgb3(0xF7, 0xF5, 0xFB)), sombre: rgb3(0x1E, 0x1A, 0x2E), accent: Some(rgb3(0x8C, 0x4F, 0xD6)) },
    FamilleTheme { id: "nixos", nom: "NixOS", clair: Some(rgb3(0xF3, 0xF6, 0xFB)), sombre: rgb3(0x1A, 0x22, 0x36), accent: Some(rgb3(0x52, 0x77, 0xC3)) },
    FamilleTheme { id: "redhat", nom: "Red Hat", clair: Some(rgb3(0xFF, 0xFF, 0xFF)), sombre: rgb3(0x15, 0x15, 0x15), accent: Some(rgb3(0xEE, 0x00, 0x00)) },
    FamilleTheme { id: "gentoo", nom: "Gentoo", clair: Some(rgb3(0xF4, 0xF2, 0xF8)), sombre: rgb3(0x22, 0x1E, 0x2E), accent: Some(rgb3(0x7A, 0x6C, 0xB8)) },
    FamilleTheme { id: "monokai", nom: "Monokai", clair: None, sombre: rgb3(0x27, 0x28, 0x22), accent: Some(rgb3(0xF9, 0x26, 0x72)) },
    FamilleTheme { id: "ayu", nom: "Ayu", clair: Some(rgb3(0xFA, 0xFA, 0xFA)), sombre: rgb3(0x0B, 0x0E, 0x14), accent: Some(rgb3(0xF0, 0x63, 0x00)) },
    FamilleTheme { id: "material", nom: "Material", clair: Some(rgb3(0xFA, 0xFA, 0xFA)), sombre: rgb3(0x26, 0x32, 0x38), accent: Some(rgb3(0x00, 0x96, 0x88)) },
    FamilleTheme { id: "kanagawa", nom: "Kanagawa", clair: Some(rgb3(0xF2, 0xEC, 0xBC)), sombre: rgb3(0x1F, 0x1F, 0x28), accent: Some(rgb3(0x5F, 0x84, 0xCF)) },
    FamilleTheme { id: "nightowl", nom: "Night Owl", clair: Some(rgb3(0xFB, 0xFB, 0xFB)), sombre: rgb3(0x01, 0x16, 0x27), accent: Some(rgb3(0x54, 0x8B, 0xFF)) },
    FamilleTheme { id: "nightfox", nom: "Nightfox", clair: Some(rgb3(0xF6, 0xF2, 0xEE)), sombre: rgb3(0x19, 0x23, 0x30), accent: Some(rgb3(0x5B, 0x8D, 0xD0)) },
    FamilleTheme { id: "horizon", nom: "Horizon", clair: Some(rgb3(0xFD, 0xF0, 0xED)), sombre: rgb3(0x1C, 0x1E, 0x26), accent: Some(rgb3(0xE9, 0x56, 0x78)) },
    FamilleTheme { id: "tomorrow", nom: "Tomorrow", clair: Some(rgb3(0xFF, 0xFF, 0xFF)), sombre: rgb3(0x1D, 0x1F, 0x21), accent: Some(rgb3(0x42, 0x71, 0xAE)) },
    FamilleTheme { id: "palenight", nom: "Palenight", clair: None, sombre: rgb3(0x29, 0x2D, 0x3E), accent: Some(rgb3(0xC7, 0x92, 0xEA)) },
    FamilleTheme { id: "cobalt", nom: "Cobalt", clair: None, sombre: rgb3(0x19, 0x35, 0x49), accent: Some(rgb3(0xFF, 0xC6, 0x00)) },
    FamilleTheme { id: "oceanic", nom: "Oceanic", clair: None, sombre: rgb3(0x1B, 0x2B, 0x34), accent: Some(rgb3(0x66, 0x99, 0xCC)) },
    FamilleTheme { id: "synthwave", nom: "Synthwave '84", clair: None, sombre: rgb3(0x26, 0x23, 0x35), accent: Some(rgb3(0xFF, 0x7E, 0xDB)) },
    FamilleTheme { id: "zenburn", nom: "Zenburn", clair: None, sombre: rgb3(0x3F, 0x3F, 0x3F), accent: Some(rgb3(0x8C, 0xD0, 0xD3)) },
    // Des ambiances maison, nommées d'après leur teinte.
    FamilleTheme { id: "cafe", nom: "Café", clair: Some(rgb3(0xF5, 0xEF, 0xE6)), sombre: rgb3(0x24, 0x1C, 0x17), accent: Some(rgb3(0xB5, 0x65, 0x1D)) },
    FamilleTheme { id: "ocean", nom: "Océan", clair: Some(rgb3(0xEA, 0xF4, 0xF8)), sombre: rgb3(0x0F, 0x24, 0x30), accent: Some(rgb3(0x1E, 0x88, 0xB5)) },
    FamilleTheme { id: "foret", nom: "Forêt", clair: Some(rgb3(0xED, 0xF2, 0xEA)), sombre: rgb3(0x16, 0x21, 0x1A), accent: Some(rgb3(0x2E, 0x7D, 0x32)) },
    FamilleTheme { id: "rose", nom: "Rose", clair: Some(rgb3(0xFB, 0xEF, 0xF2)), sombre: rgb3(0x2A, 0x1A, 0x20), accent: Some(rgb3(0xCD, 0x19, 0x60)) },
    FamilleTheme { id: "sable", nom: "Sable", clair: Some(rgb3(0xF7, 0xF1, 0xE1)), sombre: rgb3(0x2A, 0x26, 0x20), accent: Some(rgb3(0xB1, 0x81, 0x0B)) },
    FamilleTheme { id: "menthe", nom: "Menthe", clair: Some(rgb3(0xEA, 0xF7, 0xF2)), sombre: rgb3(0x13, 0x24, 0x1E), accent: Some(rgb3(0x13, 0x9E, 0x7B)) },
    FamilleTheme { id: "bordeaux", nom: "Bordeaux", clair: Some(rgb3(0xF6, 0xEC, 0xEE)), sombre: rgb3(0x2A, 0x14, 0x18), accent: Some(rgb3(0xC2, 0x2B, 0x46)) },
    FamilleTheme { id: "minuit", nom: "Minuit", clair: None, sombre: rgb3(0x0E, 0x15, 0x25), accent: Some(rgb3(0x5C, 0x8D, 0xFF)) },
    // DEMANDE EXPLICITE (2026-10-12) : « 20 thèmes automnaux ». Des fonds crème
    // et des bruns profonds ; chaque accent a été ramené à la clarté la plus
    // PROCHE de sa teinte d'origine qui tienne 3:1 sur les deux variantes.
    FamilleTheme { id: "citrouille", nom: "Citrouille", clair: Some(rgb3(0xFB, 0xF2, 0xE8)), sombre: rgb3(0x2A, 0x1C, 0x12), accent: Some(rgb3(0xD3, 0x72, 0x1E)) },
    FamilleTheme { id: "erable", nom: "Érable", clair: Some(rgb3(0xFA, 0xEF, 0xEA)), sombre: rgb3(0x2B, 0x17, 0x14), accent: Some(rgb3(0xC8, 0x40, 0x2A)) },
    FamilleTheme { id: "chataigne", nom: "Châtaigne", clair: Some(rgb3(0xF6, 0xEF, 0xE8)), sombre: rgb3(0x26, 0x1B, 0x16), accent: Some(rgb3(0x92, 0x5C, 0x3E)) },
    FamilleTheme { id: "noisette", nom: "Noisette", clair: Some(rgb3(0xF5, 0xEE, 0xE4)), sombre: rgb3(0x27, 0x20, 0x1A), accent: Some(rgb3(0xA8, 0x74, 0x45)) },
    FamilleTheme { id: "cidre", nom: "Cidre", clair: Some(rgb3(0xFB, 0xF5, 0xE6)), sombre: rgb3(0x2A, 0x22, 0x14), accent: Some(rgb3(0xBD, 0x81, 0x1C)) },
    FamilleTheme { id: "cannelle", nom: "Cannelle", clair: Some(rgb3(0xF8, 0xEE, 0xE6)), sombre: rgb3(0x2A, 0x1B, 0x15), accent: Some(rgb3(0xB5, 0x65, 0x2F)) },
    FamilleTheme { id: "brume", nom: "Brume d'octobre", clair: Some(rgb3(0xEE, 0xF0, 0xF0)), sombre: rgb3(0x1E, 0x22, 0x24), accent: Some(rgb3(0x7C, 0x8C, 0x8E)) },
    FamilleTheme { id: "sousbois", nom: "Sous-bois", clair: Some(rgb3(0xEE, 0xF1, 0xE8)), sombre: rgb3(0x1A, 0x20, 0x17), accent: Some(rgb3(0x6B, 0x7F, 0x3A)) },
    FamilleTheme { id: "champignon", nom: "Champignon", clair: Some(rgb3(0xF2, 0xEE, 0xE9)), sombre: rgb3(0x24, 0x20, 0x1C), accent: Some(rgb3(0x8C, 0x75, 0x60)) },
    FamilleTheme { id: "bruyere", nom: "Bruyère", clair: Some(rgb3(0xF5, 0xEE, 0xF3)), sombre: rgb3(0x24, 0x1A, 0x22), accent: Some(rgb3(0x9C, 0x4F, 0x84)) },
    FamilleTheme { id: "vendange", nom: "Vendange", clair: Some(rgb3(0xF6, 0xEC, 0xF0)), sombre: rgb3(0x26, 0x14, 0x1C), accent: Some(rgb3(0xB5, 0x35, 0x64)) },
    FamilleTheme { id: "coing", nom: "Coing", clair: Some(rgb3(0xF8, 0xF5, 0xE4)), sombre: rgb3(0x24, 0x23, 0x1A), accent: Some(rgb3(0x9B, 0x8D, 0x39)) },
    FamilleTheme { id: "feuillemorte", nom: "Feuille morte", clair: Some(rgb3(0xF8, 0xF0, 0xE2)), sombre: rgb3(0x2A, 0x21, 0x16), accent: Some(rgb3(0xB8, 0x74, 0x2A)) },
    FamilleTheme { id: "figue", nom: "Figue", clair: Some(rgb3(0xF3, 0xED, 0xF2)), sombre: rgb3(0x22, 0x19, 0x23), accent: Some(rgb3(0x90, 0x51, 0x91)) },
    FamilleTheme { id: "ambre", nom: "Ambre", clair: Some(rgb3(0xFC, 0xF4, 0xE2)), sombre: rgb3(0x2B, 0x21, 0x12), accent: Some(rgb3(0xB5, 0x84, 0x14)) },
    FamilleTheme { id: "rouille", nom: "Rouille", clair: Some(rgb3(0xF7, 0xED, 0xE7)), sombre: rgb3(0x28, 0x18, 0x14), accent: Some(rgb3(0xB0, 0x4A, 0x26)) },
    FamilleTheme { id: "pommedapi", nom: "Pomme d'api", clair: Some(rgb3(0xFA, 0xEE, 0xEC)), sombre: rgb3(0x2A, 0x16, 0x16), accent: Some(rgb3(0xC2, 0x3B, 0x3B)) },
    FamilleTheme { id: "mousse", nom: "Mousse", clair: Some(rgb3(0xEE, 0xF2, 0xE9)), sombre: rgb3(0x18, 0x21, 0x17), accent: Some(rgb3(0x5F, 0x8A, 0x3A)) },
    FamilleTheme { id: "crepuscule", nom: "Crépuscule d'automne", clair: Some(rgb3(0xF6, 0xEE, 0xEA)), sombre: rgb3(0x1F, 0x1A, 0x24), accent: Some(rgb3(0xD0, 0x68, 0x3A)) },
    FamilleTheme { id: "tisane", nom: "Tisane", clair: Some(rgb3(0xF5, 0xF1, 0xE6)), sombre: rgb3(0x22, 0x21, 0x1A), accent: Some(rgb3(0x8A, 0x8A, 0x3E)) },
    // ⭐ DEMANDE EXPLICITE (2026-10-15) : « 50 nouveaux thèmes moins clairs ou moins
    // sombres et colorés ». Une teinte par famille, réparties sur tout le cercle.
    // Les variantes SOMBRES sont nettement colorées et moins sombres (luminosité
    // HSL 17 à 24 %, saturation 55 %, contre environ 8 à 14 % pour les familles
    // ci-dessus) ; les CLAIRES sont teintées (saturation 72 %). Elles restent
    // claires (88 à 96 %) par nécessité : un même accent doit tenir 3:1 sur les
    // DEUX surfaces (`les_familles_se_lisent`), ce qui impose
    // L(claire) + 0,05 ≥ 9 × (L(sombre) + 0,05) en luminance relative — plus la
    // sombre monte, plus la claire doit monter. Pour chaque teinte, la sombre la
    // plus claire puis la claire la plus sombre qui laissent un accent à 3,1:1.
    FamilleTheme { id: "coquelicot", nom: "Coquelicot", clair: Some(rgb3(0xF9, 0xDA, 0xDA)), sombre: rgb3(0x5F, 0x1C, 0x1C), accent: Some(rgb3(0xDA, 0x4E, 0x4E)) },
    FamilleTheme { id: "corail", nom: "Corail", clair: Some(rgb3(0xFA, 0xE4, 0xE0)), sombre: rgb3(0x5F, 0x25, 0x1C), accent: Some(rgb3(0xEE, 0x45, 0x2B)) },
    FamilleTheme { id: "capucine", nom: "Capucine", clair: Some(rgb3(0xFB, 0xEB, 0xE5)), sombre: rgb3(0x5F, 0x2D, 0x1C), accent: Some(rgb3(0xD6, 0x66, 0x3D)) },
    FamilleTheme { id: "papaye", nom: "Papaye", clair: Some(rgb3(0xFC, 0xF2, 0xEB)), sombre: rgb3(0x5D, 0x34, 0x1B), accent: Some(rgb3(0xE5, 0x63, 0x13)) },
    FamilleTheme { id: "mandarine", nom: "Mandarine", clair: Some(rgb3(0xFC, 0xF4, 0xEB)), sombre: rgb3(0x57, 0x38, 0x19), accent: Some(rgb3(0xCD, 0x75, 0x1D)) },
    FamilleTheme { id: "abricot", nom: "Abricot", clair: Some(rgb3(0xFC, 0xF5, 0xEB)), sombre: rgb3(0x53, 0x3B, 0x18), accent: Some(rgb3(0xC4, 0x7C, 0x10)) },
    FamilleTheme { id: "mangue", nom: "Mangue", clair: Some(rgb3(0xFB, 0xF5, 0xE7)), sombre: rgb3(0x4D, 0x3D, 0x16), accent: Some(rgb3(0xAF, 0x85, 0x25)) },
    FamilleTheme { id: "safran", nom: "Safran", clair: Some(rgb3(0xFB, 0xF8, 0xE9)), sombre: rgb3(0x4B, 0x40, 0x16), accent: Some(rgb3(0xA7, 0x8B, 0x18)) },
    FamilleTheme { id: "tournesol", nom: "Tournesol", clair: Some(rgb3(0xFB, 0xF9, 0xE9)), sombre: rgb3(0x47, 0x41, 0x15), accent: Some(rgb3(0x9E, 0x8F, 0x17)) },
    FamilleTheme { id: "mimosa", nom: "Mimosa", clair: Some(rgb3(0xFA, 0xF9, 0xDE)), sombre: rgb3(0x43, 0x42, 0x14), accent: Some(rgb3(0x95, 0x91, 0x15)) },
    FamilleTheme { id: "citron", nom: "Citron", clair: Some(rgb3(0xF8, 0xFA, 0xDE)), sombre: rgb3(0x41, 0x43, 0x14), accent: Some(rgb3(0x8D, 0x93, 0x1F)) },
    FamilleTheme { id: "chartreuse", nom: "Chartreuse", clair: Some(rgb3(0xF6, 0xFB, 0xE5)), sombre: rgb3(0x3B, 0x45, 0x14), accent: Some(rgb3(0x7E, 0x98, 0x16)) },
    FamilleTheme { id: "tilleul", nom: "Tilleul", clair: Some(rgb3(0xF6, 0xFC, 0xEB)), sombre: rgb3(0x36, 0x47, 0x15), accent: Some(rgb3(0x70, 0x9C, 0x16)) },
    FamilleTheme { id: "kiwi", nom: "Kiwi", clair: Some(rgb3(0xF0, 0xFB, 0xE5)), sombre: rgb3(0x30, 0x47, 0x15), accent: Some(rgb3(0x5A, 0x9E, 0x0D)) },
    FamilleTheme { id: "pistache", nom: "Pistache", clair: Some(rgb3(0xED, 0xFB, 0xE5)), sombre: rgb3(0x29, 0x47, 0x15), accent: Some(rgb3(0x53, 0x9E, 0x21)) },
    FamilleTheme { id: "anis", nom: "Anis", clair: Some(rgb3(0xE5, 0xFA, 0xDE)), sombre: rgb3(0x21, 0x47, 0x15), accent: Some(rgb3(0x41, 0x9E, 0x21)) },
    FamilleTheme { id: "bambou", nom: "Bambou", clair: Some(rgb3(0xED, 0xFC, 0xEB)), sombre: rgb3(0x1A, 0x49, 0x15), accent: Some(rgb3(0x1C, 0xA3, 0x0D)) },
    FamilleTheme { id: "trefle", nom: "Trèfle", clair: Some(rgb3(0xE9, 0xFB, 0xEA)), sombre: rgb3(0x15, 0x49, 0x18), accent: Some(rgb3(0x0D, 0xA3, 0x15)) },
    FamilleTheme { id: "emeraude", nom: "Émeraude", clair: Some(rgb3(0xEB, 0xFC, 0xF1)), sombre: rgb3(0x15, 0x49, 0x27), accent: Some(rgb3(0x0D, 0xA3, 0x3F)) },
    FamilleTheme { id: "jade", nom: "Jade", clair: Some(rgb3(0xE9, 0xFB, 0xF2)), sombre: rgb3(0x15, 0x49, 0x2F), accent: Some(rgb3(0x17, 0xA1, 0x5C)) },
    FamilleTheme { id: "eucalyptus", nom: "Eucalyptus", clair: Some(rgb3(0xEB, 0xFC, 0xF6)), sombre: rgb3(0x15, 0x49, 0x38), accent: Some(rgb3(0x17, 0xA1, 0x73)) },
    FamilleTheme { id: "malachite", nom: "Malachite", clair: Some(rgb3(0xE5, 0xFB, 0xF6)), sombre: rgb3(0x15, 0x47, 0x3D), accent: Some(rgb3(0x0D, 0x9E, 0x81)) },
    FamilleTheme { id: "lagon", nom: "Lagon", clair: Some(rgb3(0xE9, 0xFB, 0xFA)), sombre: rgb3(0x15, 0x47, 0x44), accent: Some(rgb3(0x0D, 0x9E, 0x94)) },
    FamilleTheme { id: "turquoise", nom: "Turquoise", clair: Some(rgb3(0xE9, 0xFB, 0xFB)), sombre: rgb3(0x15, 0x47, 0x49), accent: Some(rgb3(0x17, 0x9C, 0xA1)) },
    FamilleTheme { id: "caraibes", nom: "Caraïbes", clair: Some(rgb3(0xEB, 0xFA, 0xFC)), sombre: rgb3(0x16, 0x46, 0x4D), accent: Some(rgb3(0x19, 0x9A, 0xAE)) },
    FamilleTheme { id: "paon", nom: "Paon", clair: Some(rgb3(0xEB, 0xF8, 0xFC)), sombre: rgb3(0x18, 0x44, 0x51), accent: Some(rgb3(0x27, 0x97, 0xB9)) },
    FamilleTheme { id: "lagune", nom: "Lagune", clair: Some(rgb3(0xEB, 0xF7, 0xFC)), sombre: rgb3(0x19, 0x43, 0x57), accent: Some(rgb3(0x1D, 0x94, 0xCB)) },
    FamilleTheme { id: "cieldete", nom: "Ciel d'été", clair: Some(rgb3(0xE9, 0xF4, 0xFB)), sombre: rgb3(0x1A, 0x41, 0x5B), accent: Some(rgb3(0x12, 0x8F, 0xE2)) },
    FamilleTheme { id: "azur", nom: "Azur", clair: Some(rgb3(0xE7, 0xF1, 0xFB)), sombre: rgb3(0x1C, 0x3D, 0x5F), accent: Some(rgb3(0x32, 0x8A, 0xE2)) },
    FamilleTheme { id: "ceruleen", nom: "Céruléen", clair: Some(rgb3(0xE0, 0xEB, 0xFA)), sombre: rgb3(0x1C, 0x38, 0x5F), accent: Some(rgb3(0x34, 0x82, 0xEF)) },
    FamilleTheme { id: "bleuet", nom: "Bleuet", clair: Some(rgb3(0xD5, 0xE0, 0xF8)), sombre: rgb3(0x1C, 0x30, 0x5F), accent: Some(rgb3(0x42, 0x76, 0xF0)) },
    FamilleTheme { id: "saphir", nom: "Saphir", clair: Some(rgb3(0xD1, 0xD9, 0xF8)), sombre: rgb3(0x1C, 0x29, 0x5F), accent: Some(rgb3(0x4C, 0x6D, 0xF0)) },
    FamilleTheme { id: "outremer", nom: "Outremer", clair: Some(rgb3(0xCD, 0xD1, 0xF7)), sombre: rgb3(0x1C, 0x22, 0x5F), accent: Some(rgb3(0x55, 0x65, 0xF1)) },
    FamilleTheme { id: "myrtille", nom: "Myrtille", clair: Some(rgb3(0xCD, 0xCD, 0xF7)), sombre: rgb3(0x1C, 0x1C, 0x5F), accent: Some(rgb3(0x5F, 0x5F, 0xF2)) },
    FamilleTheme { id: "pervenche", nom: "Pervenche", clair: Some(rgb3(0xD1, 0xCD, 0xF7)), sombre: rgb3(0x22, 0x1C, 0x5F), accent: Some(rgb3(0x6B, 0x5C, 0xF2)) },
    FamilleTheme { id: "iris", nom: "Iris", clair: Some(rgb3(0xD6, 0xCD, 0xF7)), sombre: rgb3(0x2A, 0x1C, 0x5F), accent: Some(rgb3(0x79, 0x58, 0xF1)) },
    FamilleTheme { id: "glycine", nom: "Glycine", clair: Some(rgb3(0xDC, 0xCF, 0xF7)), sombre: rgb3(0x32, 0x1C, 0x5F), accent: Some(rgb3(0x88, 0x53, 0xF1)) },
    FamilleTheme { id: "amethyste", nom: "Améthyste", clair: Some(rgb3(0xE3, 0xD1, 0xF8)), sombre: rgb3(0x3B, 0x1C, 0x5F), accent: Some(rgb3(0x9A, 0x4E, 0xF1)) },
    FamilleTheme { id: "lilas", nom: "Lilas", clair: Some(rgb3(0xEA, 0xD5, 0xF8)), sombre: rgb3(0x44, 0x1C, 0x5F), accent: Some(rgb3(0xAC, 0x47, 0xF0)) },
    FamilleTheme { id: "violette", nom: "Violette", clair: Some(rgb3(0xEF, 0xD8, 0xF9)), sombre: rgb3(0x4C, 0x1C, 0x5F), accent: Some(rgb3(0xBD, 0x3E, 0xEF)) },
    FamilleTheme { id: "mauve", nom: "Mauve", clair: Some(rgb3(0xF5, 0xDE, 0xFA)), sombre: rgb3(0x54, 0x1C, 0x5F), accent: Some(rgb3(0xCF, 0x32, 0xEE)) },
    FamilleTheme { id: "prune", nom: "Prune", clair: Some(rgb3(0xFA, 0xE5, 0xFB)), sombre: rgb3(0x5D, 0x1C, 0x5F), accent: Some(rgb3(0xE5, 0x13, 0xEC)) },
    FamilleTheme { id: "cassis", nom: "Cassis", clair: Some(rgb3(0xFA, 0xE2, 0xF8)), sombre: rgb3(0x5F, 0x1C, 0x59), accent: Some(rgb3(0xD7, 0x44, 0xCB)) },
    FamilleTheme { id: "hibiscus", nom: "Hibiscus", clair: Some(rgb3(0xFA, 0xE0, 0xF5)), sombre: rgb3(0x5F, 0x1C, 0x51), accent: Some(rgb3(0xEC, 0x1A, 0xC2)) },
    FamilleTheme { id: "pivoine", nom: "Pivoine", clair: Some(rgb3(0xFA, 0xE0, 0xF1)), sombre: rgb3(0x5F, 0x1C, 0x48), accent: Some(rgb3(0xED, 0x24, 0xAA)) },
    FamilleTheme { id: "bougainvillier", nom: "Bougainvillier", clair: Some(rgb3(0xFA, 0xDE, 0xED)), sombre: rgb3(0x5F, 0x1C, 0x41), accent: Some(rgb3(0xEE, 0x28, 0x95)) },
    FamilleTheme { id: "framboise", nom: "Framboise", clair: Some(rgb3(0xFA, 0xDE, 0xEA)), sombre: rgb3(0x5F, 0x1C, 0x39), accent: Some(rgb3(0xEE, 0x2F, 0x82)) },
    FamilleTheme { id: "litchi", nom: "Litchi", clair: Some(rgb3(0xF9, 0xDC, 0xE6)), sombre: rgb3(0x5F, 0x1C, 0x32), accent: Some(rgb3(0xEE, 0x2F, 0x6F)) },
    FamilleTheme { id: "grenadine", nom: "Grenadine", clair: Some(rgb3(0xF9, 0xDC, 0xE3)), sombre: rgb3(0x5F, 0x1C, 0x2B), accent: Some(rgb3(0xEE, 0x32, 0x5E)) },
    FamilleTheme { id: "fraise", nom: "Fraise", clair: Some(rgb3(0xF9, 0xDC, 0xDF)), sombre: rgb3(0x5F, 0x1C, 0x23), accent: Some(rgb3(0xEF, 0x34, 0x4A)) },
    // ⭐ DEMANDE EXPLICITE (2026-10-15) : « 100 nouveaux thèmes clairs et sombres et
    // colorés, [les couleurs choisies] par leur représentation psychologique, celles
    // qui sont positives ». Seules les teintes associées à des émotions positives
    // en psychologie des couleurs ; pas de bruns, d'olives, de gris ni de rouges
    // sang. ⚠️ Un jaune ou un orange SOMBRE devient brun, un vert-jaune sombre
    // olive : leur variante sombre prend une nuit complémentaire (bleu nuit,
    // violet, bleu canard, aubergine) ou un vert forêt, et leur accent un orange
    // doré vif. Même calcul que les 50 familles ci-dessus (accent 3,1:1 sur les
    // deux surfaces) ; générées par un script, une famille par ligne.
    // Les JAUNES — joie, optimisme, lumière.
    FamilleTheme { id: "soleil", nom: "Soleil", clair: Some(rgb3(0xF3, 0xE4, 0x9B)), sombre: rgb3(0x19, 0x24, 0x6B), accent: Some(rgb3(0xBB, 0x65, 0x0A)) },
    FamilleTheme { id: "boutondor", nom: "Bouton-d'or", clair: Some(rgb3(0xF3, 0xE7, 0x9B)), sombre: rgb3(0x37, 0x19, 0x6B), accent: Some(rgb3(0xBD, 0x66, 0x0A)) },
    FamilleTheme { id: "pissenlit", nom: "Pissenlit", clair: Some(rgb3(0xFB, 0xF9, 0xE1)), sombre: rgb3(0x16, 0x45, 0x5D), accent: Some(rgb3(0xD1, 0x77, 0x17)) },
    FamilleTheme { id: "miel", nom: "Miel", clair: Some(rgb3(0xF3, 0xDB, 0x9B)), sombre: rgb3(0x19, 0x24, 0x6B), accent: Some(rgb3(0xB8, 0x64, 0x0A)) },
    FamilleTheme { id: "canari", nom: "Canari", clair: Some(rgb3(0xF3, 0xED, 0x9B)), sombre: rgb3(0x37, 0x19, 0x6B), accent: Some(rgb3(0xBD, 0x66, 0x0A)) },
    FamilleTheme { id: "jonquille", nom: "Jonquille", clair: Some(rgb3(0xFC, 0xF9, 0xE8)), sombre: rgb3(0x16, 0x45, 0x5D), accent: Some(rgb3(0xD1, 0x77, 0x17)) },
    FamilleTheme { id: "primevere", nom: "Primevère", clair: Some(rgb3(0xF3, 0xF0, 0x9B)), sombre: rgb3(0x19, 0x24, 0x6B), accent: Some(rgb3(0xBF, 0x68, 0x0A)) },
    FamilleTheme { id: "topaze", nom: "Topaze", clair: Some(rgb3(0xF3, 0xDE, 0x9B)), sombre: rgb3(0x37, 0x19, 0x6B), accent: Some(rgb3(0xB8, 0x64, 0x0A)) },
    FamilleTheme { id: "ble", nom: "Blé d'or", clair: Some(rgb3(0xFC, 0xF8, 0xE8)), sombre: rgb3(0x16, 0x45, 0x5D), accent: Some(rgb3(0xD1, 0x77, 0x17)) },
    FamilleTheme { id: "dore", nom: "Doré", clair: Some(rgb3(0xF3, 0xE1, 0x9B)), sombre: rgb3(0x19, 0x24, 0x6B), accent: Some(rgb3(0xB8, 0x64, 0x0A)) },
    FamilleTheme { id: "etincelle", nom: "Étincelle", clair: Some(rgb3(0xF3, 0xE8, 0x9B)), sombre: rgb3(0x37, 0x19, 0x6B), accent: Some(rgb3(0xBD, 0x66, 0x0A)) },
    FamilleTheme { id: "citrine", nom: "Citrine", clair: Some(rgb3(0xFC, 0xF9, 0xE6)), sombre: rgb3(0x16, 0x45, 0x5D), accent: Some(rgb3(0xD1, 0x77, 0x17)) },
    FamilleTheme { id: "ete", nom: "Été", clair: Some(rgb3(0xF3, 0xDD, 0x9B)), sombre: rgb3(0x19, 0x24, 0x6B), accent: Some(rgb3(0xB8, 0x64, 0x0A)) },
    FamilleTheme { id: "lumiere", nom: "Lumière", clair: Some(rgb3(0xF3, 0xEE, 0x9B)), sombre: rgb3(0x37, 0x19, 0x6B), accent: Some(rgb3(0xBF, 0x68, 0x0A)) },
    // Les ORANGES et les PÊCHES — enthousiasme, chaleur, convivialité.
    FamilleTheme { id: "peche", nom: "Pêche", clair: Some(rgb3(0xFC, 0xF2, 0xEB)), sombre: rgb3(0x14, 0x43, 0x55), accent: Some(rgb3(0xDF, 0x68, 0x19)) },
    FamilleTheme { id: "clementine", nom: "Clémentine", clair: Some(rgb3(0xFB, 0xEA, 0xDB)), sombre: rgb3(0x60, 0x19, 0x6B), accent: Some(rgb3(0xD8, 0x64, 0x0B)) },
    FamilleTheme { id: "melon", nom: "Melon", clair: Some(rgb3(0xF8, 0xE1, 0xC6)), sombre: rgb3(0x19, 0x32, 0x6B), accent: Some(rgb3(0xCA, 0x64, 0x16)) },
    FamilleTheme { id: "nectarine", nom: "Nectarine", clair: Some(rgb3(0xFC, 0xF1, 0xEB)), sombre: rgb3(0x14, 0x43, 0x55), accent: Some(rgb3(0xE6, 0x61, 0x1E)) },
    FamilleTheme { id: "kumquat", nom: "Kumquat", clair: Some(rgb3(0xFA, 0xEB, 0xD6)), sombre: rgb3(0x60, 0x19, 0x6B), accent: Some(rgb3(0xD8, 0x64, 0x0B)) },
    FamilleTheme { id: "tangerine", nom: "Tangerine", clair: Some(rgb3(0xF9, 0xE0, 0xCD)), sombre: rgb3(0x19, 0x32, 0x6B), accent: Some(rgb3(0xC3, 0x68, 0x22)) },
    FamilleTheme { id: "saumon", nom: "Saumon", clair: Some(rgb3(0xFC, 0xEC, 0xE8)), sombre: rgb3(0x13, 0x40, 0x51), accent: Some(rgb3(0xF3, 0x4C, 0x23)) },
    FamilleTheme { id: "goyave", nom: "Goyave", clair: Some(rgb3(0xFC, 0xE9, 0xE4)), sombre: rgb3(0x60, 0x19, 0x6B), accent: Some(rgb3(0xF3, 0x48, 0x14)) },
    FamilleTheme { id: "couchant", nom: "Coucher de soleil", clair: Some(rgb3(0xFA, 0xE1, 0xD6)), sombre: rgb3(0x19, 0x32, 0x6B), accent: Some(rgb3(0xE4, 0x4D, 0x0C)) },
    FamilleTheme { id: "aurore", nom: "Aurore", clair: Some(rgb3(0xFC, 0xF0, 0xE8)), sombre: rgb3(0x13, 0x42, 0x53), accent: Some(rgb3(0xE9, 0x5D, 0x0C)) },
    FamilleTheme { id: "physalis", nom: "Physalis", clair: Some(rgb3(0xF9, 0xEB, 0xD2)), sombre: rgb3(0x60, 0x19, 0x6B), accent: Some(rgb3(0xD8, 0x64, 0x0B)) },
    FamilleTheme { id: "agrume", nom: "Agrume", clair: Some(rgb3(0xF7, 0xE3, 0xBB)), sombre: rgb3(0x19, 0x32, 0x6B), accent: Some(rgb3(0xD0, 0x60, 0x0B)) },
    // Les ROUGES CHALEUREUX — vitalité, élan, passion douce.
    FamilleTheme { id: "cerise", nom: "Cerise", clair: Some(rgb3(0xFC, 0xE4, 0xE7)), sombre: rgb3(0x6B, 0x19, 0x24), accent: Some(rgb3(0xF5, 0x38, 0x51)) },
    FamilleTheme { id: "rubis", nom: "Rubis", clair: Some(rgb3(0xFC, 0xE4, 0xE9)), sombre: rgb3(0x6B, 0x19, 0x2A), accent: Some(rgb3(0xF5, 0x38, 0x5E)) },
    FamilleTheme { id: "groseille", nom: "Groseille", clair: Some(rgb3(0xFC, 0xE4, 0xE5)), sombre: rgb3(0x6B, 0x19, 0x1F), accent: Some(rgb3(0xF5, 0x38, 0x45)) },
    FamilleTheme { id: "pasteque", nom: "Pastèque", clair: Some(rgb3(0xFC, 0xE5, 0xE4)), sombre: rgb3(0x6B, 0x1C, 0x19), accent: Some(rgb3(0xE0, 0x56, 0x52)) },
    FamilleTheme { id: "canneberge", nom: "Canneberge", clair: Some(rgb3(0xFC, 0xE6, 0xEC)), sombre: rgb3(0x6B, 0x19, 0x2F), accent: Some(rgb3(0xF5, 0x38, 0x6B)) },
    FamilleTheme { id: "vermillon", nom: "Vermillon", clair: Some(rgb3(0xFC, 0xEA, 0xE8)), sombre: rgb3(0x6B, 0x21, 0x19), accent: Some(rgb3(0xF4, 0x47, 0x34)) },
    // Les ROSES — tendresse, douceur, bienveillance.
    FamilleTheme { id: "sakura", nom: "Sakura", clair: Some(rgb3(0xFC, 0xE4, 0xEB)), sombre: rgb3(0x6B, 0x19, 0x32), accent: Some(rgb3(0xEA, 0x45, 0x77)) },
    FamilleTheme { id: "flamant", nom: "Flamant rose", clair: Some(rgb3(0xFC, 0xE4, 0xE8)), sombre: rgb3(0x6B, 0x19, 0x27), accent: Some(rgb3(0xF5, 0x38, 0x58)) },
    FamilleTheme { id: "magnolia", nom: "Magnolia", clair: Some(rgb3(0xFC, 0xE8, 0xF2)), sombre: rgb3(0x6B, 0x19, 0x42), accent: Some(rgb3(0xF4, 0x34, 0x94)) },
    FamilleTheme { id: "camelia", nom: "Camélia", clair: Some(rgb3(0xFC, 0xE6, 0xEE)), sombre: rgb3(0x6B, 0x19, 0x37), accent: Some(rgb3(0xF4, 0x36, 0x7C)) },
    FamilleTheme { id: "azalee", nom: "Azalée", clair: Some(rgb3(0xFC, 0xE8, 0xF4)), sombre: rgb3(0x6B, 0x19, 0x49), accent: Some(rgb3(0xF4, 0x2F, 0xA2)) },
    FamilleTheme { id: "orchidee", nom: "Orchidée", clair: Some(rgb3(0xFC, 0xEB, 0xFC)), sombre: rgb3(0x67, 0x18, 0x65), accent: Some(rgb3(0xF0, 0x0D, 0xE8)) },
    FamilleTheme { id: "guimauve", nom: "Guimauve", clair: Some(rgb3(0xFC, 0xE6, 0xEF)), sombre: rgb3(0x6B, 0x19, 0x3B), accent: Some(rgb3(0xF4, 0x34, 0x84)) },
    FamilleTheme { id: "dragee", nom: "Dragée", clair: Some(rgb3(0xFC, 0xE6, 0xEC)), sombre: rgb3(0x6B, 0x19, 0x2E), accent: Some(rgb3(0xF5, 0x38, 0x67)) },
    FamilleTheme { id: "fuchsia", nom: "Fuchsia", clair: Some(rgb3(0xFC, 0xEB, 0xF7)), sombre: rgb3(0x6B, 0x19, 0x53), accent: Some(rgb3(0xF4, 0x2C, 0xB8)) },
    FamilleTheme { id: "bonbon", nom: "Bonbon", clair: Some(rgb3(0xFC, 0xE6, 0xF0)), sombre: rgb3(0x6B, 0x19, 0x40), accent: Some(rgb3(0xEA, 0x43, 0x91)) },
    FamilleTheme { id: "begonia", nom: "Bégonia", clair: Some(rgb3(0xFC, 0xE4, 0xE6)), sombre: rgb3(0x6B, 0x19, 0x20), accent: Some(rgb3(0xF5, 0x38, 0x48)) },
    FamilleTheme { id: "tulipe", nom: "Tulipe", clair: Some(rgb3(0xFC, 0xE6, 0xED)), sombre: rgb3(0x6B, 0x19, 0x35), accent: Some(rgb3(0xF4, 0x36, 0x75)) },
    // Les VIOLETS et les LAVANDES — créativité, imagination, sérénité.
    FamilleTheme { id: "parme", nom: "Parme", clair: Some(rgb3(0xE9, 0xD6, 0xFA)), sombre: rgb3(0x45, 0x19, 0x6B), accent: Some(rgb3(0xA6, 0x4C, 0xF6)) },
    FamilleTheme { id: "crocus", nom: "Crocus", clair: Some(rgb3(0xE4, 0xD4, 0xFA)), sombre: rgb3(0x3D, 0x19, 0x6B), accent: Some(rgb3(0x98, 0x51, 0xF6)) },
    FamilleTheme { id: "jacinthe", nom: "Jacinthe", clair: Some(rgb3(0xDE, 0xD2, 0xF9)), sombre: rgb3(0x32, 0x19, 0x6B), accent: Some(rgb3(0x86, 0x55, 0xF6)) },
    FamilleTheme { id: "heliotrope", nom: "Héliotrope", clair: Some(rgb3(0xF1, 0xDD, 0xFB)), sombre: rgb3(0x50, 0x19, 0x6B), accent: Some(rgb3(0xB7, 0x4E, 0xEB)) },
    FamilleTheme { id: "mure", nom: "Mûre", clair: Some(rgb3(0xF7, 0xE4, 0xFC)), sombre: rgb3(0x5B, 0x19, 0x6B), accent: Some(rgb3(0xCF, 0x3B, 0xF5)) },
    FamilleTheme { id: "raisin", nom: "Raisin", clair: Some(rgb3(0xEF, 0xDB, 0xFB)), sombre: rgb3(0x4D, 0x19, 0x6B), accent: Some(rgb3(0xB5, 0x47, 0xF5)) },
    FamilleTheme { id: "nebuleuse", nom: "Nébuleuse", clair: Some(rgb3(0xE0, 0xD2, 0xF9)), sombre: rgb3(0x37, 0x19, 0x6B), accent: Some(rgb3(0x8F, 0x53, 0xF6)) },
    FamilleTheme { id: "licorne", nom: "Licorne", clair: Some(rgb3(0xFB, 0xEB, 0xFC)), sombre: rgb3(0x64, 0x19, 0x69), accent: Some(rgb3(0xE5, 0x23, 0xF3)) },
    FamilleTheme { id: "anemone", nom: "Anémone", clair: Some(rgb3(0xF4, 0xE1, 0xFB)), sombre: rgb3(0x55, 0x19, 0x6B), accent: Some(rgb3(0xC5, 0x40, 0xF5)) },
    FamilleTheme { id: "ancolie", nom: "Ancolie", clair: Some(rgb3(0xE8, 0xD6, 0xFA)), sombre: rgb3(0x42, 0x19, 0x6B), accent: Some(rgb3(0xA2, 0x4E, 0xF6)) },
    FamilleTheme { id: "pensee", nom: "Pensée", clair: Some(rgb3(0xE2, 0xD2, 0xF9)), sombre: rgb3(0x3A, 0x19, 0x6B), accent: Some(rgb3(0x93, 0x51, 0xF6)) },
    FamilleTheme { id: "clematite", nom: "Clématite", clair: Some(rgb3(0xFA, 0xE8, 0xFC)), sombre: rgb3(0x60, 0x19, 0x6B), accent: Some(rgb3(0xDB, 0x34, 0xF4)) },
    // Les BLEUS — calme, confiance, sérénité.
    FamilleTheme { id: "myosotis", nom: "Myosotis", clair: Some(rgb3(0xE8, 0xF2, 0xFC)), sombre: rgb3(0x18, 0x3D, 0x67), accent: Some(rgb3(0x2A, 0x88, 0xF4)) },
    FamilleTheme { id: "ciel", nom: "Ciel", clair: Some(rgb3(0xEB, 0xF5, 0xFC)), sombre: rgb3(0x17, 0x41, 0x61), accent: Some(rgb3(0x1E, 0x8F, 0xE6)) },
    FamilleTheme { id: "glacier", nom: "Glacier", clair: Some(rgb3(0xE8, 0xF6, 0xFC)), sombre: rgb3(0x15, 0x42, 0x59), accent: Some(rgb3(0x0B, 0x92, 0xD5)) },
    FamilleTheme { id: "lapis", nom: "Lapis-lazuli", clair: Some(rgb3(0xD2, 0xD8, 0xF9)), sombre: rgb3(0x19, 0x27, 0x6B), accent: Some(rgb3(0x58, 0x6F, 0xE2)) },
    FamilleTheme { id: "denim", nom: "Denim", clair: Some(rgb3(0xE1, 0xEB, 0xFB)), sombre: rgb3(0x19, 0x37, 0x6B), accent: Some(rgb3(0x45, 0x82, 0xEA)) },
    FamilleTheme { id: "porcelaine", nom: "Porcelaine", clair: Some(rgb3(0xDB, 0xE3, 0xFB)), sombre: rgb3(0x19, 0x2F, 0x6B), accent: Some(rgb3(0x49, 0x77, 0xF5)) },
    FamilleTheme { id: "hortensia", nom: "Hortensia", clair: Some(rgb3(0xCF, 0xD2, 0xF9)), sombre: rgb3(0x19, 0x1F, 0x6B), accent: Some(rgb3(0x5C, 0x66, 0xED)) },
    FamilleTheme { id: "grandlarge", nom: "Grand large", clair: Some(rgb3(0xEB, 0xF4, 0xFC)), sombre: rgb3(0x17, 0x40, 0x63), accent: Some(rgb3(0x16, 0x8C, 0xF3)) },
    FamilleTheme { id: "baltique", nom: "Baltique", clair: Some(rgb3(0xEB, 0xF2, 0xFC)), sombre: rgb3(0x19, 0x3D, 0x6B), accent: Some(rgb3(0x36, 0x88, 0xF4)) },
    FamilleTheme { id: "fjord", nom: "Fjord", clair: Some(rgb3(0xEB, 0xF6, 0xFC)), sombre: rgb3(0x16, 0x42, 0x5D), accent: Some(rgb3(0x18, 0x91, 0xDC)) },
    FamilleTheme { id: "mediterranee", nom: "Méditerranée", clair: Some(rgb3(0xEB, 0xF4, 0xFC)), sombre: rgb3(0x18, 0x40, 0x65), accent: Some(rgb3(0x2C, 0x8D, 0xE8)) },
    FamilleTheme { id: "ondine", nom: "Ondine", clair: Some(rgb3(0xDF, 0xE9, 0xFB)), sombre: rgb3(0x19, 0x35, 0x6B), accent: Some(rgb3(0x42, 0x7E, 0xF5)) },
    FamilleTheme { id: "alize", nom: "Alizé", clair: Some(rgb3(0xE4, 0xF5, 0xFC)), sombre: rgb3(0x14, 0x42, 0x55), accent: Some(rgb3(0x0B, 0x93, 0xC9)) },
    FamilleTheme { id: "zenith", nom: "Zénith", clair: Some(rgb3(0xD6, 0xDF, 0xFA)), sombre: rgb3(0x19, 0x2C, 0x6B), accent: Some(rgb3(0x4C, 0x73, 0xF6)) },
    FamilleTheme { id: "celeste", nom: "Céleste", clair: Some(rgb3(0xE6, 0xEF, 0xFC)), sombre: rgb3(0x19, 0x3A, 0x6B), accent: Some(rgb3(0x38, 0x84, 0xF5)) },
    FamilleTheme { id: "bleuroi", nom: "Bleu roi", clair: Some(rgb3(0xCF, 0xD5, 0xF9)), sombre: rgb3(0x19, 0x24, 0x6B), accent: Some(rgb3(0x5A, 0x6C, 0xE2)) },
    // Les TURQUOISES — clarté, fraîcheur, apaisement.
    FamilleTheme { id: "aiguemarine", nom: "Aigue-marine", clair: Some(rgb3(0xE4, 0xFC, 0xF8)), sombre: rgb3(0x11, 0x48, 0x41), accent: Some(rgb3(0x12, 0x9E, 0x8C)) },
    FamilleTheme { id: "atoll", nom: "Atoll", clair: Some(rgb3(0xEB, 0xFC, 0xFC)), sombre: rgb3(0x11, 0x48, 0x46), accent: Some(rgb3(0x12, 0x9E, 0x9A)) },
    FamilleTheme { id: "maldives", nom: "Maldives", clair: Some(rgb3(0xEB, 0xFB, 0xFC)), sombre: rgb3(0x11, 0x47, 0x4A), accent: Some(rgb3(0x1D, 0x9C, 0xA5)) },
    FamilleTheme { id: "piscine", nom: "Piscine", clair: Some(rgb3(0xE1, 0xF7, 0xFB)), sombre: rgb3(0x12, 0x44, 0x4E), accent: Some(rgb3(0x09, 0x97, 0xB3)) },
    FamilleTheme { id: "embruns", nom: "Embruns", clair: Some(rgb3(0xEB, 0xFB, 0xFC)), sombre: rgb3(0x12, 0x47, 0x4C), accent: Some(rgb3(0x09, 0x9C, 0xAC)) },
    FamilleTheme { id: "cristal", nom: "Cristal", clair: Some(rgb3(0xE6, 0xF8, 0xFC)), sombre: rgb3(0x13, 0x44, 0x51), accent: Some(rgb3(0x0A, 0x97, 0xBB)) },
    FamilleTheme { id: "ruisseau", nom: "Ruisseau", clair: Some(rgb3(0xDB, 0xFB, 0xFB)), sombre: rgb3(0x10, 0x46, 0x46), accent: Some(rgb3(0x08, 0x9B, 0x9B)) },
    FamilleTheme { id: "source", nom: "Source", clair: Some(rgb3(0xE6, 0xFC, 0xFA)), sombre: rgb3(0x11, 0x48, 0x44), accent: Some(rgb3(0x12, 0x9E, 0x93)) },
    FamilleTheme { id: "celadon", nom: "Céladon", clair: Some(rgb3(0xE4, 0xFC, 0xF6)), sombre: rgb3(0x11, 0x48, 0x3A), accent: Some(rgb3(0x1C, 0x9E, 0x7E)) },
    FamilleTheme { id: "oasis", nom: "Oasis", clair: Some(rgb3(0xE4, 0xFC, 0xF8)), sombre: rgb3(0x11, 0x48, 0x3F), accent: Some(rgb3(0x1C, 0x9E, 0x89)) },
    // Les VERTS — harmonie, croissance, espoir, équilibre.
    FamilleTheme { id: "printemps", nom: "Printemps", clair: Some(rgb3(0xEE, 0xFC, 0xEB)), sombre: rgb3(0x1B, 0x4A, 0x11), accent: Some(rgb3(0x2A, 0xA3, 0x12)) },
    FamilleTheme { id: "prairie", nom: "Prairie", clair: Some(rgb3(0xEA, 0xFC, 0xE4)), sombre: rgb3(0x11, 0x4A, 0x24), accent: Some(rgb3(0x12, 0xA3, 0x28)) },
    FamilleTheme { id: "fougere", nom: "Fougère", clair: Some(rgb3(0xE8, 0xFC, 0xEA)), sombre: rgb3(0x11, 0x4A, 0x16), accent: Some(rgb3(0x12, 0xA3, 0x1E)) },
    FamilleTheme { id: "basilic", nom: "Basilic", clair: Some(rgb3(0xE8, 0xFC, 0xEC)), sombre: rgb3(0x11, 0x4A, 0x1B), accent: Some(rgb3(0x12, 0xA3, 0x2A)) },
    FamilleTheme { id: "pommeverte", nom: "Pomme verte", clair: Some(rgb3(0xEE, 0xFC, 0xE4)), sombre: rgb3(0x11, 0x4A, 0x24), accent: Some(rgb3(0x1D, 0xA3, 0x1D)) },
    FamilleTheme { id: "bourgeon", nom: "Bourgeon", clair: Some(rgb3(0xF0, 0xFC, 0xE4)), sombre: rgb3(0x11, 0x4A, 0x24), accent: Some(rgb3(0x1E, 0xA3, 0x12)) },
    FamilleTheme { id: "gazon", nom: "Gazon", clair: Some(rgb3(0xE9, 0xFC, 0xE8)), sombre: rgb3(0x13, 0x4A, 0x11), accent: Some(rgb3(0x17, 0xA3, 0x12)) },
    FamilleTheme { id: "citronvert", nom: "Citron vert", clair: Some(rgb3(0xF2, 0xFC, 0xE4)), sombre: rgb3(0x11, 0x4A, 0x24), accent: Some(rgb3(0x2A, 0xA3, 0x12)) },
    FamilleTheme { id: "matcha", nom: "Matcha", clair: Some(rgb3(0xEC, 0xFC, 0xE4)), sombre: rgb3(0x11, 0x4A, 0x24), accent: Some(rgb3(0x1D, 0xA3, 0x28)) },
    FamilleTheme { id: "sapin", nom: "Sapin", clair: Some(rgb3(0xE4, 0xFC, 0xEE)), sombre: rgb3(0x11, 0x48, 0x28), accent: Some(rgb3(0x09, 0xA2, 0x49)) },
    FamilleTheme { id: "verveine", nom: "Verveine", clair: Some(rgb3(0xEA, 0xFC, 0xE8)), sombre: rgb3(0x16, 0x4A, 0x11), accent: Some(rgb3(0x1E, 0xA3, 0x12)) },
    FamilleTheme { id: "cactus", nom: "Cactus", clair: Some(rgb3(0xEB, 0xFC, 0xEF)), sombre: rgb3(0x11, 0x4A, 0x20), accent: Some(rgb3(0x12, 0xA3, 0x36)) },
    FamilleTheme { id: "lierre", nom: "Lierre", clair: Some(rgb3(0xEB, 0xFC, 0xF0)), sombre: rgb3(0x11, 0x4A, 0x23), accent: Some(rgb3(0x12, 0xA3, 0x3E)) },
    FamilleTheme { id: "amazonie", nom: "Amazonie", clair: Some(rgb3(0xDF, 0xFB, 0xED)), sombre: rgb3(0x11, 0x48, 0x2D), accent: Some(rgb3(0x08, 0xA0, 0x54)) },
    FamilleTheme { id: "peridot", nom: "Péridot", clair: Some(rgb3(0xF0, 0xFB, 0xDB)), sombre: rgb3(0x11, 0x4A, 0x24), accent: Some(rgb3(0x2F, 0xA2, 0x09)) },
    FamilleTheme { id: "nenuphar", nom: "Nénuphar", clair: Some(rgb3(0xE1, 0xFB, 0xF1)), sombre: rgb3(0x11, 0x48, 0x31), accent: Some(rgb3(0x08, 0xA0, 0x61)) },
    FamilleTheme { id: "jardin", nom: "Jardin", clair: Some(rgb3(0xE8, 0xFC, 0xE9)), sombre: rgb3(0x11, 0x4A, 0x13), accent: Some(rgb3(0x12, 0xA3, 0x17)) },
    FamilleTheme { id: "melisse", nom: "Mélisse", clair: Some(rgb3(0xEE, 0xFC, 0xEB)), sombre: rgb3(0x1D, 0x4A, 0x11), accent: Some(rgb3(0x2F, 0xA3, 0x12)) },
];

pub fn famille_theme(id: &str) -> Option<&'static FamilleTheme> {
    FAMILLES_THEMES.iter().find(|f| f.id == id)
}

/// L'identifiant de la famille qui SUIT LE FOND D'ÉCRAN — hors de
/// `FAMILLES_THEMES`, ses couleurs ne sont connues qu'à l'exécution (voir
/// `cef_settings::famille_par_id`).
pub const ID_FOND_D_ECRAN: &str = "materialyou-fond";

/// **MATERIAL YOU DEPUIS UNE COULEUR SOURCE** — DEMANDE EXPLICITE (2026-10-12) :
/// « le mode Material You qui suit le fond d'écran ». Comme Android 12 : une
/// couleur SOURCE tirée du fond d'écran (`wallpaper_color`, le profil
/// « Vibrant » de la Palette d'Android), puis des rôles qui n'en gardent que la
/// TEINTE à des tons fixés — surfaces très claires ou très sombres à peine
/// teintées (ton 98 / 6 de Material 3, chroma faible), accent franc.
///
/// L'accent sert les DEUX variantes (même contrainte que les familles fixes,
/// testée par `les_familles_se_lisent`) : sa luminosité est CHERCHÉE pour
/// atteindre 3:1 sur les deux surfaces, en partant de la sienne.
pub fn famille_material_you(source: [f32; 3]) -> FamilleTheme {
    let (h, s, l) = rgb_to_hsl(source);
    // Une source grise (fond d'écran noir et blanc) reste grise : inventer
    // une teinte trahirait le fond d'écran.
    let s_surface = if s < 0.08 { 0.0 } else { 0.35 };
    let s_accent = if s < 0.08 { 0.0 } else { s.clamp(0.45, 0.8) };
    let clair = hsl_to_rgb(h, s_surface, 0.975);
    let sombre = hsl_to_rgb(h, s_surface * 0.6, 0.075);
    let lisible = |l: f32| {
        let a = hsl_to_rgb(h, s_accent, l);
        (contrast_ratio(a, clair).min(contrast_ratio(a, sombre)), a)
    };
    // De la luminosité de la source vers celle qui équilibre les deux
    // contrastes : la première qui passe, au pas de 1 %.
    let depart = l.clamp(0.3, 0.7);
    let accent = (0..=40)
        .flat_map(|i| [depart - i as f32 * 0.01, depart + i as f32 * 0.01])
        .map(lisible)
        .find(|(ratio, _)| *ratio >= 3.05)
        .map_or_else(|| lisible(0.45).1, |(_, a)| a);
    FamilleTheme { id: ID_FOND_D_ECRAN, nom: "Material You (fond d'écran)", clair: Some(clair), sombre, accent: Some(accent) }
}

/// Convertit une composante couleur `0.0..1.0` en octet `0..255` — arrondi,
/// bornage défensif (une composante calculée peut légèrement dépasser cette
/// plage selon le chemin, voir `push_until`). Partagé par tout code qui
/// sérialise une couleur pour l'UI plutôt que pour le rendu Direct3D
/// (sélecteurs hex de `win32::cef_settings::hex`, variables CSS
/// `--menu-*-rgb` poussées par `win32::cef_popup` — voir sa doc).
pub fn to_u8(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Palette COMPLÈTE dérivée d'un seul accent pour une fenêtre `egui` — les
/// rôles nommés que l'outil de vérification "Contrast Ledger" (skill
/// `design-system`, voir la conversation) affiche en colonnes : un SEUL
/// calcul, une SEULE source de vérité par fenêtre/par frame, plutôt que
/// `readable_text_rgb`/`adaptive_zones`/`accent_emphasis_rgb` rappelées
/// séparément par chacun des quatre appelants (`win32::egui_overlay`,
/// `win32::dash`, `win32::clipboard_viewer`, `win32::settings_dash`) —
/// `readable_text_rgb(accent_rgb)` seul était recalculé jusqu'à cinq fois par
/// frame avant ce regroupement, un vrai risque de divergence si l'un des
/// appels avait fini par utiliser une variante légèrement différente sans
/// que ce soit voulu. Ne reprend PAS `accent_rgb` lui-même (chaque appelant
/// l'a déjà sous la main, voir `compute_palette`) — uniquement les rôles
/// DÉRIVÉS.
pub struct Palette {
    /// Texte flottant sur `window_fill`/le fond de section — voir
    /// `readable_text_rgb_guaranteed`, PAS `readable_text_rgb` seul : ce
    /// fond n'est jamais l'accent pur, toujours mélangé avec le flou du
    /// bureau à une opacité moindre (voir `ACCENT_TEXT_MIN_OPACITY`).
    pub accent_text: [f32; 3],
    /// Fond d'une zone au repos (bouton, case à cocher, tuile...).
    pub zone_bg: [f32; 3],
    /// Fond de la même zone au survol.
    pub zone_bg_hovered: [f32; 3],
    /// Texte le plus lisible sur `zone_bg`.
    pub zone_text: [f32; 3],
    /// Texte le plus lisible sur `zone_bg_hovered`.
    pub zone_text_hovered: [f32; 3],
    /// Variante de l'accent conservant sa teinte, mise en avant (titres de
    /// catégorie du Dash...) — voir `accent_emphasis_rgb`.
    pub emphasis: [f32; 3],
    /// Texte/icône posé sur un APLAT PLEINEMENT OPAQUE de `accent_rgb`
    /// LUI-MÊME (`v.selection.bg_fill`/`v.widgets.active.bg_fill` dans
    /// `egui_overlay::apply_theme`) — inspiré de `--accent-fg-color` dans
    /// libadwaita/GNOME (voir la doc de module, "Éléments et sous-éléments")
    /// : GNOME associe TOUJOURS un rôle de fond à son propre rôle de texte
    /// EN PAIRE (`--accent-bg-color`/`--accent-fg-color`,
    /// `--card-bg-color`/`--card-fg-color`...), jamais un texte pensé pour
    /// UN fond réutilisé sur un AUTRE. `accent_text` ci-dessus est pensé
    /// pour un fond RÉEL SOUVENT TRANSPARENT (mélangé au flou du bureau à
    /// une opacité inconnue, voir sa doc) — PAS pour cet aplat-ci, plein et
    /// opaque. Utilise `readable_text_rgb` SEULE (pas la variante
    /// `_guaranteed`) : cet aplat est un simple `Color32` peint par `egui`,
    /// jamais mélangé à une opacité incertaine, contrairement au fond réel
    /// d'une fenêtre.
    pub accent_fg: [f32; 3],
}

/// Calcule la `Palette` complète de `accent_rgb` — voir sa doc. `dark_mode` :
/// même convention de sens que `adaptive_zones`/`accent_emphasis_rgb`.
pub fn compute_palette(accent_rgb: [f32; 3], dark_mode: bool) -> Palette {
    let zones = adaptive_zones(accent_rgb, dark_mode);
    Palette {
        accent_text: readable_text_rgb_guaranteed(accent_rgb, ACCENT_TEXT_MIN_OPACITY),
        zone_bg: zones.bg,
        zone_bg_hovered: zones.bg_hovered,
        zone_text: zones.text,
        zone_text_hovered: zones.text_hovered,
        emphasis: accent_emphasis_rgb(accent_rgb, dark_mode),
        accent_fg: readable_text_rgb(accent_rgb),
    }
}

/// Pousse la LUMINOSITÉ HSL de `rgb` (voir `push_until`) jusqu'à ce que le
/// contraste WCAG contre `reference` (FIXE, contrairement à
/// `ensure_min_contrast` où la cible change avec chaque pas) atteigne
/// `MIN_ZONE_TEXT_CONTRAST`.
fn ensure_contrast_against(rgb: [f32; 3], reference: [f32; 3], initial_delta: f32) -> [f32; 3] {
    push_until(rgb, initial_delta, |result| contrast_ratio(result, reference) >= MIN_ZONE_TEXT_CONTRAST)
}

/// Comme `ensure_contrast_against`, mais exige seulement le plancher "limite
/// de composant d'interface" (`MIN_ZONE_BOUNDARY_CONTRAST`, 3:1) plutôt que
/// le plancher "texte" (`MIN_ZONE_TEXT_CONTRAST`, 4.5:1) — trop strict pour
/// un simple aplat de tuile d'icône (voir `icon_palette`), qui doit rester
/// visible face à son fond, pas forcément aussi contrasté qu'un texte.
fn ensure_boundary_contrast_against(rgb: [f32; 3], reference: [f32; 3], initial_delta: f32) -> [f32; 3] {
    push_until(rgb, initial_delta, |result| contrast_ratio(result, reference) >= MIN_ZONE_BOUNDARY_CONTRAST)
}

/// Écart de teinte HSL entre deux icônes consécutives de `icon_palette` —
/// six teintes réparties À ÉGALITÉ sur tout le cercle chromatique (360° / 6),
/// même principe de composition qu'un schéma "triade" (3 teintes espacées de
/// 120°) ou "quadrant" (4 teintes espacées de 90°) d'un générateur de palette
/// de couleurs : des teintes régulièrement espacées restent perceptiblement
/// distinctes tout en restant réellement liées à la teinte de départ (chacune
/// est un multiple exact de son décalage), plutôt que des couleurs choisies
/// au hasard. Vérifié empiriquement contre le schéma "triade"/"quadrant"
/// d'un générateur de palette de couleurs (MCP `color-scheme-generator`)
/// avant d'écrire cette fonction : ces deux schémas déclinent un même
/// principe (teinte de départ + copies décalées à intervalle FIXE), ce
/// module l'étend simplement à 6 teintes pour couvrir chaque icône de repli
/// du dock.
const ICON_HUE_STEP: f32 = 360.0 / 6.0;

/// Palette de repli des icônes du dock (voir `ui::dock::Dock::new`) —
/// affichée tant qu'une icône n'a pas de vraie texture extraite (échec
/// d'extraction, ou icône sans texture propre, voir `ui::dock::Icon::
/// external_texture`). Dérivée ENTIÈREMENT de la couleur de mise en
/// avant `accent_rgb` choisie par l'utilisateur (voir `ICON_HUE_STEP`),
/// remplaçant six couleurs arbitraires figées en dur qui ne suivaient PAS
/// cette couleur — le seul maillon du système de colorisation automatique
/// (voir la doc de ce module) qui restait déconnecté de l'accent choisi,
/// alors que tout le reste (zones, texte, teinte des barres...) en dérive
/// déjà. Saturation reprise de `accent_rgb` (bornée à un intervalle qui reste
/// lisible en tuile pleine, ni grisâtre ni criarde) ; luminosité pousée
/// jusqu'à un contraste minimal garanti contre le fond RÉEL du dock
/// (`soften_tint(accent_rgb, dark_mode)`, voir sa doc) via
/// `ensure_boundary_contrast_against`, pour qu'aucune tuile ne se fonde dans
/// le fond quel que soit l'accent choisi — même plancher WCAG
/// (`MIN_ZONE_BOUNDARY_CONTRAST`) que les cases à cocher/zones des fenêtres
/// `egui` (voir `adaptive_zones`).
pub fn icon_palette(accent_rgb: [f32; 3], dark_mode: bool) -> [[f32; 4]; 6] {
    let (accent_hue, accent_sat, _) = rgb_to_hsl(accent_rgb);
    let sat = accent_sat.clamp(0.45, 0.85);
    let dock_bg = soften_tint(accent_rgb, dark_mode);
    let delta = if dark_mode { -0.28 } else { 0.28 };
    let mut palette = [[0.0f32; 4]; 6];
    for (i, tile) in palette.iter_mut().enumerate() {
        let hue = (accent_hue + ICON_HUE_STEP * i as f32) % 360.0;
        let base = hsl_to_rgb(hue, sat, 0.5);
        let [r, g, b] = ensure_boundary_contrast_against(base, dock_bg, delta);
        *tile = [r, g, b, 1.0];
    }
    palette
}

#[cfg(test)]
mod tests {
    use super::*;

    /// En mode clair le texte doit être SOMBRE (et l'inverse) : un pôle resté
    /// blanc sur la surface `#F3F3F3` serait illisible — contraste mesuré ici,
    /// pas supposé.
    #[test]
    fn les_poles_suivent_le_mode_et_se_lisent_sur_leur_surface() {
        let (texte_clair, sur_texte_clair) = poles(true);
        assert_eq!(texte_clair, READABLE_DARK);
        assert_eq!(sur_texte_clair, READABLE_LIGHT);
        assert!(contrast_ratio(SURFACE_WINDOWS_CLAIR, texte_clair) >= 4.5);
        let (texte_sombre, _) = poles(false);
        assert_eq!(texte_sombre, READABLE_LIGHT);
        assert!(contrast_ratio(SURFACE_WINDOWS_SOMBRE, texte_sombre) >= 4.5);
    }

    /// Quelle que soit la couleur du fond d'écran — vive, sombre, claire,
    /// grise —, la famille qui en sort se lit comme les familles fixes.
    #[test]
    fn material_you_se_lit_quel_que_soit_le_fond_d_ecran() {
        let (texte_clair, _) = poles(true);
        let (texte_sombre, _) = poles(false);
        let mut sources = vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.5, 0.5, 0.5], [1.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        for i in 0..36 {
            for l in [0.15, 0.5, 0.85] {
                sources.push(hsl_to_rgb(i as f32 * 10.0, 0.9, l));
            }
        }
        for source in sources {
            let f = famille_material_you(source);
            let (clair, accent) = (f.clair.unwrap(), f.accent.unwrap());
            assert!(contrast_ratio(f.sombre, texte_sombre) >= 4.5, "{source:?} sombre");
            assert!(contrast_ratio(clair, texte_clair) >= 4.5, "{source:?} clair");
            assert!(contrast_ratio(accent, f.sombre) >= 3.0, "{source:?} accent sur sombre");
            assert!(contrast_ratio(accent, clair) >= 3.0, "{source:?} accent sur clair");
        }
        // La teinte du fond d'écran se retrouve dans l'accent.
        let bleu = famille_material_you([0.1, 0.3, 0.9]).accent.unwrap();
        assert!(bleu[2] > bleu[0] && bleu[2] > bleu[1], "{bleu:?}");
    }

    #[test]
    fn les_familles_se_lisent() {
        let (texte_clair, _) = poles(true);
        let (texte_sombre, _) = poles(false);
        for f in &FAMILLES_THEMES {
            assert!(contrast_ratio(f.sombre, texte_sombre) >= 4.5, "{} sombre", f.id);
            if let Some(clair) = f.clair {
                assert!(contrast_ratio(clair, texte_clair) >= 4.5, "{} clair", f.id);
            }
            if let Some(accent) = f.accent {
                assert!(contrast_ratio(accent, f.sombre) >= 3.0, "{} accent sur sombre", f.id);
                if let Some(clair) = f.clair {
                    assert!(contrast_ratio(accent, clair) >= 3.0, "{} accent sur clair", f.id);
                }
            }
            assert_eq!(famille_theme(f.id).map(|x| x.nom), Some(f.nom));
        }
    }

    /// Le mode clair l'emporte sur un Windows sombre ; hors mode clair, c'est
    /// Windows qui décide, comme avant.
    #[test]
    fn le_mode_clair_eteint_le_sombre_de_windows() {
        assert!(!sombre_effectif_selon(true, true));
        assert!(!sombre_effectif_selon(false, true));
        assert!(sombre_effectif_selon(true, false));
        assert!(!sombre_effectif_selon(false, false));
    }

    /// Vérification hors-ligne écrite en diagnostiquant "l'icône de
    /// fermeture des dashs est cassée" (voir la conversation) — la VRAIE
    /// cause s'est révélée être le glyphe "✕" (U+2715) lui-même, absent des
    /// trois polices intégrées à `egui` (vérifié directement dans leurs
    /// tables `cmap` : ni Ubuntu-Light, ni NotoEmoji-Regular, ni
    /// emoji-icon-font ne le couvrent — d'où le carré de repli "tofu"
    /// signalé, un bug préexistant sans lien avec `THEMES`, remplacé par
    /// "✖"/U+2716 dans `win32::dash`/`clipboard_viewer`/`settings_dash`, bien
    /// couvert par NotoEmoji-Regular ET emoji-icon-font). Ce test-ci reste
    /// utile pour autre chose : il recalcule, pour CHAQUE thème et les deux
    /// modes clair/sombre, exactement ce que `egui_overlay::
    /// apply_adaptive_theme` applique aux boutons (`zone_bg`/`zone_text`) et
    /// vérifie le contraste WCAG RÉELLEMENT obtenu — pas seulement supposé
    /// garanti par construction — en garde-fou pour de futurs thèmes.
    #[test]
    fn themes_keep_readable_close_button() {
        for (name, accent) in THEMES {
            for dark_mode in [true, false] {
                let palette = compute_palette(accent, dark_mode);
                let ratio = contrast_ratio(palette.zone_bg, palette.zone_text);
                println!(
                    "{name} (dark={dark_mode}): zone_bg={:?} zone_text={:?} contrast={ratio:.2}",
                    palette.zone_bg, palette.zone_text
                );
                assert!(
                    ratio >= MIN_ZONE_TEXT_CONTRAST,
                    "{name} (dark={dark_mode}) : contraste {ratio:.2} < {MIN_ZONE_TEXT_CONTRAST} \
                     (zone_bg={:?}, zone_text={:?})",
                    palette.zone_bg,
                    palette.zone_text
                );
            }
        }
    }

    /// Même vérification que `themes_keep_readable_close_button`, mais pour
    /// `dark_theme_from_base_color` — n'importe quelle couleur de base
    /// choisie par l'utilisateur (voir `win32::settings_dash::
    /// render_themes_section`, sélecteur "Couleur de base") doit produire un
    /// thème tout aussi lisible que les 120 thèmes intégrés, y compris pour
    /// des couleurs de base EXTRÊMES (blanc, noir, gris pur sans teinte).
    #[test]
    fn base_color_themes_keep_readable_close_button() {
        let base_colors: [(&str, [f32; 3]); 8] = [
            ("Blanc", [1.0, 1.0, 1.0]),
            ("Noir", [0.0, 0.0, 0.0]),
            ("Gris pur", [0.5, 0.5, 0.5]),
            ("Rouge vif", [1.0, 0.0, 0.0]),
            ("Vert vif", [0.0, 1.0, 0.0]),
            ("Bleu vif", [0.0, 0.0, 1.0]),
            ("Jaune pastel", [0.98, 0.95, 0.7]),
            ("Rose pastel", [0.98, 0.8, 0.85]),
        ];
        for (name, base) in base_colors {
            let theme = dark_theme_from_base_color(base);
            for dark_mode in [true, false] {
                let palette = compute_palette(theme, dark_mode);
                let ratio = contrast_ratio(palette.zone_bg, palette.zone_text);
                assert!(
                    ratio >= MIN_ZONE_TEXT_CONTRAST,
                    "base \"{name}\" -> thème {theme:?} (dark={dark_mode}) : contraste {ratio:.2} < \
                     {MIN_ZONE_TEXT_CONTRAST} (zone_bg={:?}, zone_text={:?})",
                    palette.zone_bg,
                    palette.zone_text
                );
            }
        }
    }

    // --- Palettes harmonieuses (applet « Sélecteur de couleurs ») ---

    /// La complémentaire d'un rouge vif est un cyan vif : le cas le plus
    /// simple à vérifier à la main, donc celui qui prouve que le décalage est
    /// bien de 180° et pas autre chose.
    #[test]
    fn la_complementaire_est_la_couleur_opposee() {
        let g = harmonies([1.0, 0.0, 0.0]);
        assert_eq!(g[0].nom, "Complémentaire");
        let c = g[0].teintes[0];
        assert!(c[0].abs() < 1e-3 && (c[1] - 1.0).abs() < 1e-3 && (c[2] - 1.0).abs() < 1e-3, "{c:?}");
    }

    /// Les cinq groupes, dans l'ordre annoncé — c'est ce que la page affiche
    /// et ce que le `.gpl` exporte, donc l'ordre fait partie du contrat.
    #[test]
    fn les_cinq_groupes_sont_dans_l_ordre_annonce() {
        let g = harmonies([0.2, 0.5, 0.9]);
        let noms: Vec<&str> = g.iter().map(|x| x.nom).collect();
        assert_eq!(noms, ["Complémentaire", "Analogues", "Triade", "Quadriade", "Monochrome"]);
        assert_eq!(g[0].teintes.len(), 1);
        assert_eq!(g[1].teintes.len(), 2);
        assert_eq!(g[2].teintes.len(), 3);
        assert_eq!(g[3].teintes.len(), 4);
        assert_eq!(g[4].teintes.len(), 5);
    }

    /// Les décalages sont relus en TEINTE sur le résultat, pas supposés depuis
    /// l'arithmétique qui les a produits.
    #[test]
    fn les_teintes_sont_aux_angles_des_schemas() {
        let g = harmonies([1.0, 0.0, 0.0]);
        let teinte = |c: [f32; 3]| rgb_to_hsl(c).0;
        let proche_de = |a: f32, b: f32| {
            let d = (a - b).abs();
            d < 1e-2 || (360.0 - d) < 1e-2
        };
        assert!(proche_de(teinte(g[0].teintes[0]), 180.0), "complémentaire");
        assert!(proche_de(teinte(g[1].teintes[0]), 330.0), "analogue −30°");
        assert!(proche_de(teinte(g[1].teintes[1]), 30.0), "analogue +30°");
        assert!(proche_de(teinte(g[2].teintes[0]), 0.0), "triade, la base");
        assert!(proche_de(teinte(g[2].teintes[1]), 120.0), "triade +120°");
        assert!(proche_de(teinte(g[2].teintes[2]), 240.0), "triade +240°");
        assert!(proche_de(teinte(g[3].teintes[1]), 90.0), "quadriade +90°");
        assert!(proche_de(teinte(g[3].teintes[2]), 180.0), "quadriade +180°");
        assert!(proche_de(teinte(g[3].teintes[3]), 270.0), "quadriade +270°");
    }

    /// **TOTALE** : un gris n'a pas de teinte, et pourtant on ne panique pas et
    /// on rend bien cinq groupes. Même exigence sur noir et blanc, où les
    /// marches de luminosité viennent buter sur les bornes.
    #[test]
    fn la_palette_est_totale_sur_le_gris_le_noir_et_le_blanc() {
        for base in [[0.5, 0.5, 0.5], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]] {
            let g = harmonies(base);
            assert_eq!(g.len(), 5, "base {base:?}");
            for groupe in &g {
                assert!(!groupe.teintes.is_empty(), "{} est vide", groupe.nom);
                for c in &groupe.teintes {
                    assert!(
                        c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                        "{} : {c:?} hors bornes pour {base:?}",
                        groupe.nom
                    );
                }
            }
        }
    }

    /// Le monochrome ne change QUE la luminosité — c'est toute la raison pour
    /// laquelle il passe par `tone` plutôt que de fabriquer des gris.
    #[test]
    fn le_monochrome_garde_teinte_et_saturation_et_va_du_clair_au_fonce() {
        let base = [0.2, 0.5, 0.9];
        let (h0, s0, _) = rgb_to_hsl(base);
        let g = &harmonies(base)[4];
        let mut precedente = f32::INFINITY;
        for c in &g.teintes {
            let (h, s, l) = rgb_to_hsl(*c);
            assert!((s - s0).abs() < 1e-3, "saturation {s} au lieu de {s0}");
            assert!((h - h0).abs() < 1e-3, "teinte {h} au lieu de {h0}");
            assert!(l < precedente, "la clarté doit décroître : {l} après {precedente}");
            precedente = l;
        }
    }

    /// `aplatir` ne répète pas la couleur de départ, qui figure pourtant dans
    /// trois groupes sur cinq — c'est ce qui rend un `.gpl` exporté lisible.
    #[test]
    fn aplatir_ne_repete_pas_la_couleur_de_depart() {
        let base = [1.0, 0.0, 0.0];
        let plat = aplatir(base, &harmonies(base));
        assert_eq!(plat[0], base, "la couleur de départ vient en tête");
        let combien = plat.iter().filter(|c| proche(**c, base)).count();
        assert_eq!(combien, 1, "la base ne doit apparaître qu'une fois : {plat:?}");
        // 1 base + 1 complémentaire + 2 analogues + 2 nouvelles (triade) +
        // 2 nouvelles (quadriade, son +180 étant déjà la complémentaire) +
        // 4 nouvelles (monochrome, sa marche du milieu étant la base) = 12.
        assert_eq!(plat.len(), 12);
    }
}

#[cfg(test)]
mod tests_bornes_du_theme {
    use super::*;

    /// ⭐ **LES BORNES PAR DÉFAUT NE CHANGENT RIEN.** C'est ce qui rend ce réglage
    /// sûr : tant que l'utilisateur n'y touche pas, l'application se comporte
    /// exactement comme avant — et c'est vérifiable, pas supposé.
    #[test]
    fn les_bornes_par_defaut_ne_changent_rien() {
        for base in [[0.2, 0.9, 0.4], [0.9, 0.1, 0.5], [0.5, 0.5, 0.5], [0.95, 0.93, 0.97]] {
            assert_eq!(
                dark_theme_borne(base, SATURATION_PLANCHER, LUMINOSITE_PLAFOND),
                dark_theme_from_base_color(base),
                "l'enveloppe doit rendre EXACTEMENT l'ancien resultat"
            );
        }
    }

    /// ⛔ **UN PLANCHER À ZÉRO REND LA COULEUR TELLE QUELLE** — c'est la demande
    /// meme : « desactiver cette amplification ». On le verifie sur la
    /// SATURATION, parce que c'est elle qui est accentuee.
    #[test]
    fn un_plancher_nul_laisse_la_saturation_telle_quelle() {
        // Un temoin PALE : le plancher par defaut le remonterait.
        let pale = [0.80, 0.78, 0.82];
        let (_, sat_pale, _) = rgb_to_hsl(pale);
        assert!(sat_pale < SATURATION_PLANCHER, "le temoin doit etre SOUS le plancher par defaut");

        let (_, sat_ajustee, _) = rgb_to_hsl(dark_theme_borne(pale, SATURATION_PLANCHER, LUMINOSITE_PLAFOND));
        assert!((sat_ajustee - SATURATION_PLANCHER).abs() < 0.01, "par defaut, elle est REMONTEE");

        let (_, sat_libre, _) = rgb_to_hsl(dark_theme_borne(pale, 0.0, LUMINOSITE_PLAFOND));
        assert!((sat_libre - sat_pale).abs() < 0.01, "a zero, elle est LAISSEE telle quelle");
    }

    /// ⛔ **DES BORNES INVERSÉES NE DOIVENT PAS PANIQUER.** La page enverra deux
    /// nombres, et `f32::clamp` PANIQUE quand le debut depasse la fin — avec
    /// `panic = "abort"`, l'application mourrait sur un reglage absurde.
    #[test]
    fn des_bornes_inversees_ne_font_pas_paniquer() {
        let _ = dark_theme_borne([0.5, 0.5, 0.5], 1.0, 0.0);
        let _ = dark_theme_borne([0.5, 0.5, 0.5], 5.0, -3.0);
        let _ = dark_theme_borne([0.5, 0.5, 0.5], 0.99, 100.0);
    }

    /// ⭐ **LE PLAFOND DE LUMINOSITE AGIT, LUI AUSSI** — sans quoi la moitie de
    /// la demande (« luminosite ») ne serait pas tenue.
    #[test]
    fn le_plafond_de_luminosite_agit() {
        let vif = [0.9, 0.2, 0.2];
        let (_, _, clair) = rgb_to_hsl(dark_theme_borne(vif, SATURATION_PLANCHER, 1.0));
        let (_, _, sombre) = rgb_to_hsl(dark_theme_borne(vif, SATURATION_PLANCHER, 0.10));
        assert!(clair > sombre, "un plafond plus haut doit garder plus de lumiere");
        assert!(clair <= 1.0 && sombre >= 0.0);
    }
}

#[cfg(test)]
mod tests_ajuster_theme {
    use super::*;

    /// Le témoin du défaut : *bleu nuit*, saturation 0,74, luminosité 0,17.
    const BLEU_NUIT: [f32; 3] = [0x0E as f32 / 255.0, 0x0B as f32 / 255.0, 0x4A as f32 / 255.0];

    /// ⭐ **AU NEUTRE, LA COULEUR EST RENDUE À L'OCTET PRÈS** — cliquer une
    /// pastille applique toujours le thème prévu.
    #[test]
    fn au_neutre_la_couleur_est_inchangee() {
        assert_eq!(ajuster_theme(BLEU_NUIT, FORCE_NEUTRE, FORCE_NEUTRE), BLEU_NUIT);
    }

    /// ⛔ **LE DÉFAUT** : sur bleu nuit, le curseur ne faisait rien entre 0 et
    /// 0,74. Chaque cran de la course doit maintenant changer la saturation.
    #[test]
    fn la_saturation_bouge_sur_toute_la_course_de_bleu_nuit() {
        let mut precedente = -1.0;
        for cran in 0..=20 {
            let force = cran as f32 * 0.1;
            let (_, sat, _) = rgb_to_hsl(ajuster_theme(BLEU_NUIT, force, FORCE_NEUTRE));
            assert!(sat > precedente || sat >= 0.999, "cran {force}: {sat} <= {precedente}");
            precedente = sat;
        }
    }

    /// Même chose pour la luminosité, qui ne touchait pas une couleur à 0,17.
    #[test]
    fn la_luminosite_bouge_sur_toute_la_course_de_bleu_nuit() {
        let (_, _, sombre) = rgb_to_hsl(ajuster_theme(BLEU_NUIT, FORCE_NEUTRE, 0.5));
        let (_, _, neutre) = rgb_to_hsl(BLEU_NUIT);
        let (_, _, clair) = rgb_to_hsl(ajuster_theme(BLEU_NUIT, FORCE_NEUTRE, 1.5));
        assert!(sombre < neutre - 0.05 && clair > neutre + 0.05, "{sombre} {neutre} {clair}");
    }

    /// Les extrêmes : force 0 = gris, force max = pleinement saturé ; la teinte
    /// ne bouge pas.
    #[test]
    fn les_extremes_et_la_teinte() {
        let (h0, _, _) = rgb_to_hsl(BLEU_NUIT);
        let (_, gris, _) = rgb_to_hsl(ajuster_theme(BLEU_NUIT, 0.0, FORCE_NEUTRE));
        let (h, pur, _) = rgb_to_hsl(ajuster_theme(BLEU_NUIT, FORCE_MAX, FORCE_NEUTRE));
        assert!(gris < 0.01, "{gris}");
        assert!(pur > 0.99, "{pur}");
        assert!((h - h0).abs() < 0.01, "{h} {h0}");
    }

    /// Une force hors course ne doit ni paniquer ni sortir de 0..1.
    #[test]
    fn une_force_hors_course_reste_bornee() {
        for (s, l) in [(-3.0, 9.0), (50.0, -1.0), (f32::MAX, f32::MIN)] {
            for c in ajuster_theme(BLEU_NUIT, s, l) {
                assert!((0.0..=1.0).contains(&c), "{s} {l} -> {c}");
            }
        }
    }
}
