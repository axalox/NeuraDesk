//! Thèmes d'icônes — l'axe « icônes » du système de thèmes façon KDE (voir
//! `docs/plans/styles-couleurs-icones.md`, lot 5) : les glyphes des applets de
//! la barre (réglages, son, réseau…), indépendants du STYLE (`ui::style`) et
//! des COULEURS (`ui::color`).
//!
//! Cinq thèmes intégrés :
//! - « NeuraDesk » : les PNG de `TopbarIcons/` (voir `gfx::texture`) ;
//! - « Fluent » : les glyphes de Segoe Fluent Icons, la police d'icônes de
//!   Windows 11, lue depuis le système et jamais redistribuée ;
//!
//! ⛔ **LE THÈME « SYMBOLES » A ÉTÉ RETIRÉ LE 2026-10-28**, et c'est une décision
//! de licence, pas de goût : ses 41 PNG étaient des rendus de `seguiemj.ttf`, la
//! police emoji de Windows. Des images de glyphes d'une police Microsoft ne se
//! redistribuent pas comme jeu d'actifs, et le dépôt devient public sous GPLv3.
//! Les deux thèmes « Fluent Emoji » (MIT) restent les thèmes en couleur ; leurs
//! deux dessins d'appoint (`scanner`, `alimentation`) vivent dans
//! `TopbarIcons/dessins/`.
//! - « Fluent Emoji » et « Fluent Emoji 3D » : les emoji de Microsoft (licence
//!   MIT), de `TopbarIcons/fluent_couleur/` et `TopbarIcons/fluent_3d/` ;
//! - « Energy Icons » : les icônes au trait de Sam Passmore (licence MIT), de
//!   `TopbarIcons/energy/` — MONOCHROMES, teintées par la barre comme les PNG
//!   de la racine.
//!
//! ⚠️ **LES TROIS DERNIERS PORTENT LEUR PROPRE COULEUR.** Les deux premiers
//! sont monochromes par construction, et c'est la barre qui les teinte. Un thème
//! en couleur ne peut donc pas passer par le même mode de dessin — voir
//! `Dock::external_glyph_instance`, qui interroge [`est_couleur`].
//!
//! Un glyphe de police est rendu par `gfx::text` puis RECENTRÉ dans un carré de
//! 128 px à la même taille d'encre que les PNG (voir `poser_au_centre`) : une
//! boîte de ligne de police n'est pas le carré d'une icône, et l'afficher telle
//! quelle décalerait chaque glyphe d'une quantité différente. Seul le canal
//! alpha compte ensuite (`QuadInstance::new_glyph`), comme pour les PNG.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use windows::core::Result;
use windows::Win32::Graphics::Direct3D11::ID3D11Device;

use crate::gfx::text;
use crate::gfx::texture::{self, Texture};

/// Nom du fichier de `%APPDATA%\NeuraDesk\` qui retient le thème choisi.
const FICHIER_CHOIX: &str = "icones.txt";

pub const THEME_BASE: &str = "neuradesk";
const THEME_FLUENT: &str = "fluent";
const THEME_FLUENT_EMOJI: &str = "fluent-emoji";
const THEME_FLUENT_EMOJI_3D: &str = "fluent-emoji-3d";
const THEME_ENERGY: &str = "energy";

/// `(identifiant, nom affiché)`, dans l'ordre des boutons de Réglages.
///
/// ⚠️ **LA LONGUEUR EST DANS LE TYPE** : ajouter un thème ici ne compile pas
/// tant qu'on n'a pas écrit le `5`. C'est ce qui fait trouver la rangée de
/// Réglages — elle itère cette table, la page ne la connaît pas.
pub const INTEGRES: [(&str, &str); 5] = [
    (THEME_BASE, "NeuraDesk"),
    (THEME_FLUENT, "Fluent"),
    (THEME_FLUENT_EMOJI, "Fluent Emoji"),
    (THEME_FLUENT_EMOJI_3D, "Fluent Emoji 3D"),
    (THEME_ENERGY, "Energy Icons"),
];

/// D'où viennent les icônes du thème actif.
///
/// ⚠️ **UNE TABLE PLUTÔT QUE DEUX `if`** : la version précédente testait
/// `id_actif() == THEME_FLUENT` en deux endroits, et un thème de plus serait
/// retombé sur les PNG **en silence**. Ici, ajouter un thème force à décider.
#[derive(Clone, Copy)]
enum Source {
    /// Le PNG monochrome de `TopbarIcons/` — teinté par la barre.
    Png,
    /// Un glyphe de la police d'icônes — teinté par la barre.
    Police(&'static str),
    /// Un PNG **COULEUR** — dessiné TEL QUEL. La fonction choisit le jeu.
    Couleur(fn(&Icone) -> &'static [u8]),
    /// Un PNG **MONOCHROME** d'un autre jeu que la racine, teinté comme elle.
    Monochrome(fn(&Icone) -> &'static [u8]),
}

/// La source du thème actif. `police_d_icones()` touche le disque : on la
/// résout UNE fois, à la construction du jeu d'icônes.
fn source() -> Source {
    source_de(id_actif())
}

fn source_de(id: &str) -> Source {
    match id {
        THEME_FLUENT_EMOJI => Source::Couleur(|i| i.fluent_couleur),
        THEME_FLUENT_EMOJI_3D => Source::Couleur(|i| i.fluent_3d),
        THEME_ENERGY => Source::Monochrome(|i| i.energy),
        // ⚠️ **SANS DIAGNOSTIC ICI** : c'est `charger` qui le dit, à un seul
        // endroit, et qui sait distinguer « Fluent demandé sans police » de
        // « thème de base », lequel n'attend rien du tout.
        THEME_FLUENT => police_d_icones().map(Source::Police).unwrap_or(Source::Png),
        _ => Source::Png,
    }
}

/// Côté des PNG de `TopbarIcons/` et plus grand côté de leur encre, mesurés
/// sur les 20 fichiers (116 px est la valeur dominante) : un glyphe Fluent est
/// rendu dans le même carré, à la même taille d'encre.
const COTE: u32 = 128;
const ENCRE: u32 = 116;

/// Une icône d'applet et ses TROIS sources.
#[derive(Clone, Copy)]
struct Icone {
    png: &'static [u8],
    /// Point de code dans Segoe Fluent Icons (les mêmes existent dans Segoe
    /// MDL2 Assets, la police de Windows 10).
    fluent: char,
    /// Les PNG des deux thèmes Fluent Emoji — voir `jeu_couleur!`.
    fluent_couleur: &'static [u8],
    fluent_3d: &'static [u8],
    /// Le PNG MONOCHROME du thème « Energy Icons » — voir `jeu_couleur!`.
    energy: &'static [u8],
}

/// **LES DEUX THÈMES FLUENT EMOJI** — DEMANDE EXPLICITE (2026-10-11) : « Fluent
/// Emoji, en version couleur et aussi la version 3D ». Les emoji de Microsoft
/// (https://github.com/microsoft/fluentui-emoji, licence MIT), produits par
/// `tools/rasteriser_fluent_emoji.py` aux MÊMES noms de fichier que les autres
/// jeux — d'où une seule liste pour les trois dossiers.
///
/// ⚠️ Ils sont EN COULEUR (`TexMode::Texture`), et le scanner y est un DESSIN
/// (`TopbarIcons/dessins/scanner.png`) : Unicode n'a pas d'emoji « scanner ».
macro_rules! jeu_couleur {
    ($module:ident, $dossier:literal) => {
        mod $module {
        pub const REGLAGES: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/reglages.png"));
        pub const PRESSE_PAPIERS: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/presse_papiers.png"));
        pub const SON: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/son.png"));
        pub const RESEAU: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/reseau.png"));
        pub const ALIMENTATION: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/alimentation.png"));
        pub const TERMINAL: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/terminal.png"));
        pub const NOTES: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/notes.png"));
        pub const THEME: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/theme.png"));
        pub const MODE_NUIT: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/mode_nuit.png"));
        pub const IMPRIMANTE: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/imprimante.png"));
        pub const CALENDRIER: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/calendrier.png"));
        pub const EDITEUR_IMAGE: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/editeur_image.png"));
        pub const CAPTURE_ECRAN: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/capture_ecran.png"));
        pub const ENREGISTRER: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/enregistrer.png"));
        pub const MINUTEUR: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/minuteur.png"));
        pub const SURVEILLANCE: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/surveillance.png"));
        pub const WINDOWS_UPDATE: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/windows_update.png"));
        pub const PERIPHERIQUES: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/peripheriques.png"));
        pub const REGLAGES_ECRAN: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/reglages_ecran.png"));
        pub const CALCULATRICE: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/calculatrice.png"));
        pub const SSH: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/ssh.png"));
        pub const COULEURS: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/couleurs.png"));
        pub const FONCTIONNALITES: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/fonctionnalites.png"));
        pub const NOTIFICATIONS: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/notifications.png"));
        pub const RECHERCHE: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/recherche.png"));
        pub const AUTOMATISATIONS: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/automatisations.png"));
        pub const METEO: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/meteo.png"));
        pub const AIDE: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/aide.png"));
        pub const AFFICHER_BUREAU: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/afficher_bureau.png"));
        pub const CLOCHE: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/cloche.png"));
        pub const VERROUILLAGE: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/verrouillage.png"));
        pub const CARACTERES: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/caracteres.png"));
        pub const KAOMOJI: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/kaomoji.png"));
        pub const PROPRIETES_SYSTEME: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/proprietes_systeme.png"));
        pub const SCANNER: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/scanner.png"));
        pub const SHADERS: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/shaders.png"));
        pub const PHONETIQUE: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/phonetique.png"));
        pub const MASQUE: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/masque.png"));
        pub const MATHS: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/maths.png"));
        pub const LUEURS: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/lueurs.png"));
        pub const TEXTURES: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/textures.png"));
        pub const NARRATEUR: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/narrateur.png"));
        pub const LOUPE: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/loupe.png"));
        pub const MONITEUR: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/moniteur.png"));
        pub const JEU2048: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/jeu2048.png"));
        pub const CINQ_OU_PLUS: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/cinq_ou_plus.png"));
        pub const QUADRAPASSEL: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/quadrapassel.png"));
        pub const LIGHTSOFF: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/lightsoff.png"));
        pub const REVERSI: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/reversi.png"));
        pub const BOVO: &[u8] = include_bytes!(concat!("../../TopbarIcons/", $dossier, "/bovo.png"));
        }
    };
}
jeu_couleur!(fluent_couleur, "fluent_couleur");
jeu_couleur!(fluent_3d, "fluent_3d");
// ⚠️ **LE MÊME GABARIT DE MODULE, MAIS PAS EN COULEUR** : « Energy Icons » est
// monochrome (encre blanche, forme dans l'alpha) — c'est `Source::Monochrome`
// qui le dit, pas le nom de la macro. Mêmes 43 noms de fichier, produits par
// `tools/rasteriser_energy.py`.
jeu_couleur!(energy, "energy");

const REGLAGES: Icone = Icone { png: texture::SETTINGS_ICON_PNG, fluent: '\u{E713}', fluent_couleur: fluent_couleur::REGLAGES, fluent_3d: fluent_3d::REGLAGES, energy: energy::REGLAGES };
const PRESSE_PAPIERS: Icone = Icone { png: texture::CLIPBOARD_ICON_PNG, fluent: '\u{E77F}', fluent_couleur: fluent_couleur::PRESSE_PAPIERS, fluent_3d: fluent_3d::PRESSE_PAPIERS, energy: energy::PRESSE_PAPIERS };
const SON: Icone = Icone { png: texture::SOUND_ICON_PNG, fluent: '\u{E767}', fluent_couleur: fluent_couleur::SON, fluent_3d: fluent_3d::SON, energy: energy::SON };
const RESEAU: Icone = Icone { png: texture::NETWORK_ICON_PNG, fluent: '\u{E701}', fluent_couleur: fluent_couleur::RESEAU, fluent_3d: fluent_3d::RESEAU, energy: energy::RESEAU };
const ALIMENTATION: Icone = Icone { png: texture::POWER_ICON_PNG, fluent: '\u{E7E8}', fluent_couleur: fluent_couleur::ALIMENTATION, fluent_3d: fluent_3d::ALIMENTATION, energy: energy::ALIMENTATION };
const TERMINAL: Icone = Icone { png: texture::TERMINAL_ICON_PNG, fluent: '\u{E756}', fluent_couleur: fluent_couleur::TERMINAL, fluent_3d: fluent_3d::TERMINAL, energy: energy::TERMINAL };
const NOTES: Icone = Icone { png: texture::NOTES_ICON_PNG, fluent: '\u{E70B}', fluent_couleur: fluent_couleur::NOTES, fluent_3d: fluent_3d::NOTES, energy: energy::NOTES };
const THEME: Icone = Icone { png: texture::THEME_ICON_PNG, fluent: '\u{E7A1}', fluent_couleur: fluent_couleur::THEME, fluent_3d: fluent_3d::THEME, energy: energy::THEME };
const MODE_NUIT: Icone = Icone { png: texture::NIGHT_LIGHT_ICON_PNG, fluent: '\u{E708}', fluent_couleur: fluent_couleur::MODE_NUIT, fluent_3d: fluent_3d::MODE_NUIT, energy: energy::MODE_NUIT };
const IMPRIMANTE: Icone = Icone { png: texture::PRINTER_ICON_PNG, fluent: '\u{E749}', fluent_couleur: fluent_couleur::IMPRIMANTE, fluent_3d: fluent_3d::IMPRIMANTE, energy: energy::IMPRIMANTE };
const CALENDRIER: Icone = Icone { png: texture::CALENDAR_ICON_PNG, fluent: '\u{E787}', fluent_couleur: fluent_couleur::CALENDRIER, fluent_3d: fluent_3d::CALENDRIER, energy: energy::CALENDRIER };
const EDITEUR_IMAGE: Icone = Icone { png: texture::IMAGE_EDITOR_ICON_PNG, fluent: '\u{EB9F}', fluent_couleur: fluent_couleur::EDITEUR_IMAGE, fluent_3d: fluent_3d::EDITEUR_IMAGE, energy: energy::EDITEUR_IMAGE };
const CAPTURE_ECRAN: Icone = Icone { png: texture::SCREEN_CAPTURE_ICON_PNG, fluent: '\u{E9A6}', fluent_couleur: fluent_couleur::CAPTURE_ECRAN, fluent_3d: fluent_3d::CAPTURE_ECRAN, energy: energy::CAPTURE_ECRAN };
const MINUTEUR: Icone = Icone { png: texture::MINUTEUR_ICON_PNG, fluent: '\u{E916}', fluent_couleur: fluent_couleur::MINUTEUR, fluent_3d: fluent_3d::MINUTEUR, energy: energy::MINUTEUR };
const SURVEILLANCE: Icone = Icone { png: texture::SURVEILLANCE_ICON_PNG, fluent: '\u{E968}', fluent_couleur: fluent_couleur::SURVEILLANCE, fluent_3d: fluent_3d::SURVEILLANCE, energy: energy::SURVEILLANCE };
const WINDOWS_UPDATE: Icone = Icone { png: texture::WINDOWS_UPDATE_ICON_PNG, fluent: '\u{E895}', fluent_couleur: fluent_couleur::WINDOWS_UPDATE, fluent_3d: fluent_3d::WINDOWS_UPDATE, energy: energy::WINDOWS_UPDATE };
/// ⚠️ **ART PROVISOIRE** : l'observateur d'événements n'a pas encore son dessin, et
/// réutilise donc celui de son frère (une console MMC de diagnostic, dans la même zone).
/// Le jour où un PNG existe, il suffit de remplacer les cinq sources ici.
const OBSERVATEUR_EVENEMENTS: Icone = Icone { png: texture::DEVICE_MANAGER_ICON_PNG, fluent: char::from_u32(0xE7C3).unwrap(), fluent_couleur: fluent_couleur::PERIPHERIQUES, fluent_3d: fluent_3d::PERIPHERIQUES, energy: energy::PERIPHERIQUES };
/// « Enregistrer » : une CAMÉRA VIDÉO dans tous les thèmes (DEMANDE du 2026-11-01 :
/// « ne peux-tu pas mettre l'icône d'une caméra ? »). Vidéo et non photo : la
/// capture d'écran, sa voisine, montre déjà un appareil PHOTO dans les thèmes
/// Fluent Emoji et Energy. Segoe Fluent E714 est « Video », la même idée.
const ENREGISTRER: Icone = Icone { png: texture::ENREGISTRER_ICON_PNG, fluent: char::from_u32(0xE714).unwrap(), fluent_couleur: fluent_couleur::ENREGISTRER, fluent_3d: fluent_3d::ENREGISTRER, energy: energy::ENREGISTRER };
/// ⚠️ **ART PROVISOIRE** : le clavier visuel emprunte le dessin de l'applet
/// « Caractères » (le clavier tactile), avec un glyphe Segoe Fluent distinct
/// (E144, « KeyboardClassic »).
const CLAVIER: Icone = Icone { png: texture::CARACTERES_ICON_PNG, fluent: char::from_u32(0xE144).unwrap(), fluent_couleur: fluent_couleur::CARACTERES, fluent_3d: fluent_3d::CARACTERES, energy: energy::CARACTERES };
/// **2048** — les ICÔNES GNOME D'ORIGINE (voir `TopbarIcons/SOURCE.md`) : le
/// symbolique monochrome pour les thèmes NeuraDesk/Energy, l'icône couleur pour
/// les thèmes Fluent. Le glyphe de police reste un pictogramme de manette
/// (E7FC, « Game ») — aucun équivalent dans Segoe Fluent.
const JEU2048: Icone = Icone { png: texture::JEU2048_ICON_PNG, fluent: char::from_u32(0xE7FC).unwrap(), fluent_couleur: fluent_couleur::JEU2048, fluent_3d: fluent_3d::JEU2048, energy: energy::JEU2048 };
/// **CINQ OU PLUS** — les ICÔNES GNOME D'ORIGINE (Five or More) : symbolique
/// monochrome pour NeuraDesk/Energy, l'icône couleur (les billes) pour Fluent.
/// Le glyphe de police reste générique (E90B) — aucun équivalent Segoe Fluent.
const CINQ_OU_PLUS: Icone = Icone { png: texture::CINQ_OU_PLUS_ICON_PNG, fluent: char::from_u32(0xE90B).unwrap(), fluent_couleur: fluent_couleur::CINQ_OU_PLUS, fluent_3d: fluent_3d::CINQ_OU_PLUS, energy: energy::CINQ_OU_PLUS };
/// ⭐ **QUADRAPASSEL** — art d'origine GNOME (tétromino symbolique → thèmes
/// monochromes ; icône couleur → thèmes Fluent), glyphe Segoe Fluent de jeu.
const QUADRAPASSEL: Icone = Icone { png: texture::QUADRAPASSEL_ICON_PNG, fluent: char::from_u32(0xE7FB).unwrap(), fluent_couleur: fluent_couleur::QUADRAPASSEL, fluent_3d: fluent_3d::QUADRAPASSEL, energy: energy::QUADRAPASSEL };
/// ⭐ **LIGHTS OFF** — art d'origine GNOME (interrupteur symbolique → thèmes
/// monochromes ; icône couleur → thèmes Fluent), glyphe Segoe Fluent distinct (E7FA).
const LIGHTSOFF: Icone = Icone { png: texture::LIGHTSOFF_ICON_PNG, fluent: char::from_u32(0xE7FA).unwrap(), fluent_couleur: fluent_couleur::LIGHTSOFF, fluent_3d: fluent_3d::LIGHTSOFF, energy: energy::LIGHTSOFF };
/// ⭐ **REVERSI** — art d'origine iagno (GNOME Reversi : symbolique → thèmes
/// monochromes ; icône couleur → thèmes Fluent), glyphe Segoe Fluent distinct (E7F9).
const REVERSI: Icone = Icone { png: texture::REVERSI_ICON_PNG, fluent: char::from_u32(0xE7F9).unwrap(), fluent_couleur: fluent_couleur::REVERSI, fluent_3d: fluent_3d::REVERSI, energy: energy::REVERSI };
/// ⭐ **BOVO** — art d'origine Breeze (KDE), encre tirée de sa luminosité pour
/// les thèmes monochromes, glyphe Segoe Fluent distinct (E7F7).
const BOVO: Icone = Icone { png: texture::BOVO_ICON_PNG, fluent: char::from_u32(0xE7F7).unwrap(), fluent_couleur: fluent_couleur::BOVO, fluent_3d: fluent_3d::BOVO, energy: energy::BOVO };
const PERIPHERIQUES: Icone = Icone { png: texture::DEVICE_MANAGER_ICON_PNG, fluent: '\u{EEA1}', fluent_couleur: fluent_couleur::PERIPHERIQUES, fluent_3d: fluent_3d::PERIPHERIQUES, energy: energy::PERIPHERIQUES };
const REGLAGES_ECRAN: Icone = Icone { png: texture::DISPLAY_SETTINGS_ICON_PNG, fluent: '\u{E7F4}', fluent_couleur: fluent_couleur::REGLAGES_ECRAN, fluent_3d: fluent_3d::REGLAGES_ECRAN, energy: energy::REGLAGES_ECRAN };
const CALCULATRICE: Icone = Icone { png: texture::CALCULATRICE_ICON_PNG, fluent: '\u{E8EF}', fluent_couleur: fluent_couleur::CALCULATRICE, fluent_3d: fluent_3d::CALCULATRICE, energy: energy::CALCULATRICE };
const SSH: Icone = Icone { png: texture::SSH_ICON_PNG, fluent: '\u{E8D7}', fluent_couleur: fluent_couleur::SSH, fluent_3d: fluent_3d::SSH, energy: energy::SSH };
/// Le sélecteur de couleurs (une pipette). `\u{EF3C}` — la SEULE pipette de la
/// police : corps oblique, bulbe large en haut à droite, pointe effilée en bas
/// à gauche. Le choix a été fait **en regardant les glyphes rendus**, pas sur
/// leur nom — `\u{E790}` (« Color ») est une PALETTE circulaire, et c'est elle
/// que l'applet affichait sous le thème Fluent ; `\u{E70F}` est un crayon
/// (corps régulier, bout plat). Voir la sonde `sonde_des_glyphes`.
const COULEURS: Icone = Icone { png: texture::COULEURS_ICON_PNG, fluent: '\u{EF3C}', fluent_couleur: fluent_couleur::COULEURS, fluent_3d: fluent_3d::COULEURS, energy: energy::COULEURS };
/// Les fonctionnalités facultatives : `\u{ECAA}` (« AppIconDefaultAdd »), une
/// grille d'applications avec un « + » — la même idée que le PNG.
const FONCTIONNALITES: Icone = Icone { png: texture::OPTIONAL_FEATURES_ICON_PNG, fluent: '\u{ECAA}', fluent_couleur: fluent_couleur::FONCTIONNALITES, fluent_3d: fluent_3d::FONCTIONNALITES, energy: energy::FONCTIONNALITES };
/// Les notifications : `\u{E91C}` (« ActionCenterNotification »), la bulle du
/// centre de notifications — distincte de la CLOCHE des rappels (`\u{EA8F}`).
const NOTIFICATIONS: Icone = Icone { png: texture::NOTIFICATIONS_ICON_PNG, fluent: '\u{E91C}', fluent_couleur: fluent_couleur::NOTIFICATIONS, fluent_3d: fluent_3d::NOTIFICATIONS, energy: energy::NOTIFICATIONS };
/// La recherche : `\u{E721}` (« Search »), la loupe de Windows.
const RECHERCHE: Icone = Icone { png: texture::RECHERCHE_ICON_PNG, fluent: '\u{E721}', fluent_couleur: fluent_couleur::RECHERCHE, fluent_3d: fluent_3d::RECHERCHE, energy: energy::RECHERCHE };
/// Les automatisations : `\u{E945}` (« LightningBolt »), l'éclair. Le jeu
/// Keyline n'a AUCUN éclair — le PNG est donc une ROUTE (un chemin à
/// embranchement, « quand → alors »), et les deux sources disent la même idée
/// par deux images. `\u{E945}` a été retenu **en regardant les quatorze
/// candidats rendus** par la sonde `sonde_des_glyphes` : c'est le seul qui lise
/// comme un déclencheur.
const AUTOMATISATIONS: Icone = Icone { png: texture::AUTOMATISATIONS_ICON_PNG, fluent: '\u{E945}', fluent_couleur: fluent_couleur::AUTOMATISATIONS, fluent_3d: fluent_3d::AUTOMATISATIONS, energy: energy::AUTOMATISATIONS };
/// La météo : `\u{E753}` (« Cloud »), le nuage de Windows. Le jeu Keyline n'a pas
/// de glyphe « soleil derrière un nuage » côté police — c'est le PNG qui le porte,
/// et le nuage est ce qui s'en rapproche le plus.
const METEO: Icone = Icone { png: texture::METEO_ICON_PNG, fluent: '\u{E753}', fluent_couleur: fluent_couleur::METEO, fluent_3d: fluent_3d::METEO, energy: energy::METEO };
/// ⭐ **L'AIDE.** Le glyphe Segoe Fluent `E897` (« Help ») — **VÉRIFIÉ LIBRE** dans
/// le dépôt, et la sonde des glyphes écarte de toute façon les doublons parfaits
/// (voir `neuradesk-sonde-des-glyphes`).
const AIDE: Icone = Icone { png: texture::AIDE_ICON_PNG, fluent: '\u{E897}', fluent_couleur: fluent_couleur::AIDE, fluent_3d: fluent_3d::AIDE, energy: energy::AIDE };
const AFFICHER_BUREAU: Icone = Icone { png: texture::SHOW_DESKTOP_ICON_PNG, fluent: '\u{E7F8}', fluent_couleur: fluent_couleur::AFFICHER_BUREAU, fluent_3d: fluent_3d::AFFICHER_BUREAU, energy: energy::AFFICHER_BUREAU };
const CLOCHE: Icone = Icone { png: texture::BELL_ICON_PNG, fluent: '\u{EA8F}', fluent_couleur: fluent_couleur::CLOCHE, fluent_3d: fluent_3d::CLOCHE, energy: energy::CLOCHE };
/// Le verrouillage de session : `\u{E72E}`, le cadenas **FERMÉ** — voisin
/// d'`\u{E785}` (le cadenas OUVERT) dans la galerie de `ui::vignettes_dossiers`,
/// et c'est bien le fermé que ce geste veut dire.
const VERROUILLAGE: Icone = Icone { png: texture::VERROUILLAGE_ICON_PNG, fluent: '\u{E72E}', fluent_couleur: fluent_couleur::VERROUILLAGE, fluent_3d: fluent_3d::VERROUILLAGE, energy: energy::VERROUILLAGE };
/// Le clavier de lettres latines : `\u{E765}`, le glyphe du **clavier tactile** —
/// relevé dans le journal de CETTE machine (« Clavier tactile ← glyphe du
/// shell:U+E765 », section des provenances de la zone de notification), et non
/// deviné. La sonde des glyphes en proposerait d'autres, mais elle demande une
/// construction, et un pictogramme ne vaut pas d'attendre.
const CARACTERES: Icone = Icone { png: texture::CARACTERES_ICON_PNG, fluent: '\u{E765}', fluent_couleur: fluent_couleur::CARACTERES, fluent_3d: fluent_3d::CARACTERES, energy: energy::CARACTERES };
/// Le visage `^_^` dessine, et une emoji en couleur (voir `tools/rasteriser_kaomoji.py`).
const KAOMOJI: Icone = Icone { png: texture::KAOMOJI_ICON_PNG, fluent: '\u{E76E}', fluent_couleur: fluent_couleur::KAOMOJI, fluent_3d: fluent_3d::KAOMOJI, energy: energy::KAOMOJI };
/// ⭐ **LES PROPRIÉTÉS SYSTÈME.** Le glyphe Segoe Fluent `E9E9` — **deux curseurs
/// à bouton**, choisi **EN LE REGARDANT** (même méthode que la pipette et
/// l'éclair) : c'est le seul candidat examiné qui dise « des réglages qu'on
/// ajuste » plutôt qu'un objet, et il s'accorde avec les trois curseurs du PNG.
/// ⚠️ **Aucun des points déjà pris n'en approche** — l'engrenage `E713` est celui
/// de Réglages, et `chaque_icone_a_un_glyphe_prive_distinct` refuse un doublon.
const PROPRIETES_SYSTEME: Icone = Icone { png: texture::PROPRIETES_SYSTEME_ICON_PNG, fluent: '\u{E9E9}', fluent_couleur: fluent_couleur::PROPRIETES_SYSTEME, fluent_3d: fluent_3d::PROPRIETES_SYSTEME, energy: energy::PROPRIETES_SYSTEME };

/// **L'APPLET SCANNER** (voir `win32::scanner`, `win32::cef_scanner`).
///
/// ⭐ **LE GLYPHE FLUENT `E8FE` A ÉTÉ CHOISI EN LE REGARDANT**
/// (`sonde_des_glyphes`) : c'est une **ligne de balayage diagonale qui entre dans
/// un boîtier** — un scanner. Aucun catalogue ne le dit ; le rendu ASCII, si.
/// ⛔ `E749` (imprimante) et `E9A6` (capture d'écran) sont déjà pris, et
/// `chaque_icone_a_un_glyphe_prive_distinct` refuse un doublon.
const SCANNER: Icone = Icone { png: texture::SCANNER_ICON_PNG, fluent: '\u{E8FE}', fluent_couleur: fluent_couleur::SCANNER, fluent_3d: fluent_3d::SCANNER, energy: energy::SCANNER };

/// **L'ÉDITEUR DE SHADERS** (voir `win32::cef_shaders`). Le glyphe Segoe Fluent
/// `E943` (« Code ») — REGARDÉ par `sonde_des_glyphes` : des ACCOLADES `{ }`,
/// la même idée que les chevrons du PNG Keyline.
const SHADERS: Icone = Icone { png: texture::SHADERS_ICON_PNG, fluent: '\u{E943}', fluent_couleur: fluent_couleur::SHADERS, fluent_3d: fluent_3d::SHADERS, energy: energy::SHADERS };

/// **LES SYMBOLES PHONÉTIQUES** (voir `win32::cef_phonetique`). Le glyphe Segoe
/// Fluent `F2B7` — REGARDÉ sur une planche (2026-10-11) : un globe et « A字 », les
/// LANGUES ; l'applet a un onglet par langue.
const PHONETIQUE: Icone = Icone { png: texture::PHONETIQUE_ICON_PNG, fluent: '\u{F2B7}', fluent_couleur: fluent_couleur::PHONETIQUE, fluent_3d: fluent_3d::PHONETIQUE, energy: energy::PHONETIQUE };
/// L'applet « Masquer l'interface » — DÉFAUT SIGNALÉ (2026-10-12) : « l'icône de
/// l'applet pour masquer l'interface ne respecte pas le style d'icône choisi ».
/// Elle était un glyphe de police rendu UNE fois au lancement, hors des thèmes.
const MASQUE: Icone = Icone { png: texture::MASQUE_ICON_PNG, fluent: '\u{ED1A}', fluent_couleur: fluent_couleur::MASQUE, fluent_3d: fluent_3d::MASQUE, energy: energy::MASQUE };
/// **LES SYMBOLES MATHÉMATIQUES** (voir `win32::cef_maths`). Le glyphe Segoe
/// Fluent `E94B` est la RACINE CARRÉE de la famille des opérateurs (`E947`-`E94E`).
const MATHS: Icone = Icone { png: texture::MATHS_ICON_PNG, fluent: '\u{E94B}', fluent_couleur: fluent_couleur::MATHS, fluent_3d: fluent_3d::MATHS, energy: energy::MATHS };
/// **L'ÉDITEUR DES LUEURS** (voir `win32::cef_lueurs`). Le glyphe Segoe Fluent
/// `E706` (« Brightness ») : un soleil.
const LUEURS: Icone = Icone { png: texture::LUEURS_ICON_PNG, fluent: '\u{E706}', fluent_couleur: fluent_couleur::LUEURS, fluent_3d: fluent_3d::LUEURS, energy: energy::LUEURS };
/// **L'ÉDITEUR DES TEXTURES** (voir `win32::cef_textures`). Le glyphe Segoe Fluent
/// `E771` (« Personalize ») : un pinceau.
const TEXTURES: Icone = Icone { png: texture::TEXTURES_ICON_PNG, fluent: '\u{E771}', fluent_couleur: fluent_couleur::TEXTURES, fluent_3d: fluent_3d::TEXTURES, energy: energy::TEXTURES };
/// **LE NARRATEUR** (voir `win32::narrateur`). Le glyphe Segoe Fluent `E776`
/// (« Ease of access ») : le personnage de l'accessibilité — VU à la sonde
/// (`sonde_des_glyphes`), pas choisi sur son nom.
const NARRATEUR: Icone = Icone { png: texture::NARRATEUR_ICON_PNG, fluent: '\u{E776}', fluent_couleur: fluent_couleur::NARRATEUR, fluent_3d: fluent_3d::NARRATEUR, energy: energy::NARRATEUR };
/// **LA LOUPE** (voir `win32::loupe_windows`). Le glyphe Segoe Fluent `E8A3` (« ZoomIn ») :
/// la loupe marquée d'un « + » — VU au rendu, et distinct de `E721` (la loupe
/// nue de la recherche).
const LOUPE: Icone = Icone { png: texture::LOUPE_ICON_PNG, fluent: '\u{E8A3}', fluent_couleur: fluent_couleur::LOUPE, fluent_3d: fluent_3d::LOUPE, energy: energy::LOUPE };
/// **LE MONITEUR SYSTÈME** (voir `win32::cef_moniteur`). Le glyphe Segoe Fluent
/// `E9D9` (« Diagnostic ») : un écran traversé d'un tracé de pouls — VU au rendu.
const MONITEUR: Icone = Icone { png: texture::MONITEUR_ICON_PNG, fluent: '\u{E9D9}', fluent_couleur: fluent_couleur::MONITEUR, fluent_3d: fluent_3d::MONITEUR, energy: energy::MONITEUR };

/// Les cinquante-deux icônes, pour les tests qui les parcourent toutes.
#[cfg(test)]
const TOUTES: [Icone; 52] = [
    REGLAGES,
    PRESSE_PAPIERS,
    SON,
    RESEAU,
    ALIMENTATION,
    TERMINAL,
    NOTES,
    THEME,
    MODE_NUIT,
    IMPRIMANTE,
    CALENDRIER,
    EDITEUR_IMAGE,
    CAPTURE_ECRAN,
    MINUTEUR,
    SURVEILLANCE,
    WINDOWS_UPDATE,
    PERIPHERIQUES,
    OBSERVATEUR_EVENEMENTS,
    ENREGISTRER,
    CLAVIER,
    JEU2048,
    CINQ_OU_PLUS,
    QUADRAPASSEL,
    LIGHTSOFF,
    REVERSI,
    BOVO,
    REGLAGES_ECRAN,
    CALCULATRICE,
    SSH,
    COULEURS,
    FONCTIONNALITES,
    NOTIFICATIONS,
    RECHERCHE,
    AUTOMATISATIONS,
    METEO,
    AFFICHER_BUREAU,
    CLOCHE,
    VERROUILLAGE,
    CARACTERES,
    KAOMOJI,
    AIDE,
    PROPRIETES_SYSTEME,
    SCANNER,
    SHADERS,
    PHONETIQUE,
    MASQUE,
    MATHS,
    LUEURS,
    TEXTURES,
    NARRATEUR,
    LOUPE,
    MONITEUR,
];

/// Les textures des applets dans le thème actif — refaites par `app::run`
/// quand `generation` change, et partagées telles quelles avec les écrans
/// secondaires (`DockTextures`).
pub struct JeuIcones {
    pub power: Texture,
    pub terminal: Texture,
    pub sound: Texture,
    pub clipboard: Texture,
    pub network: Texture,
    pub settings: Texture,
    pub notes: Texture,
    pub theme: Texture,
    pub night_light: Texture,
    pub printer: Texture,
    pub calendar: Texture,
    pub image_editor: Texture,
    pub screen_capture: Texture,
    pub minuteur: Texture,
    pub surveillance: Texture,
    pub windows_update: Texture,
    pub device_manager: Texture,
    pub observateur_evenements: Texture,
    pub enregistrer: Texture,
    pub clavier: Texture,
    pub jeu2048: Texture,
    pub cinq_ou_plus: Texture,
    pub quadrapassel: Texture,
    pub lightsoff: Texture,
    pub reversi: Texture,
    pub bovo: Texture,
    pub display_settings: Texture,
    pub calculatrice: Texture,
    pub ssh: Texture,
    pub couleurs: Texture,
    pub optional_features: Texture,
    pub notifications: Texture,
    pub recherche: Texture,
    pub automatisations: Texture,
    pub meteo: Texture,
    pub aide: Texture,
    pub show_desktop: Texture,
    pub bell: Texture,
    pub verrouillage: Texture,
    pub caracteres: Texture,
    pub kaomoji: Texture,
    pub proprietes_systeme: Texture,
    pub scanner: Texture,
    pub shaders: Texture,
    pub phonetique: Texture,
    pub masque: Texture,
    pub maths: Texture,
    pub lueurs: Texture,
    pub textures: Texture,
    pub narrateur: Texture,
    pub loupe: Texture,
    pub moniteur: Texture,
}

impl JeuIcones {
    pub fn charger(device: &ID3D11Device) -> Result<Self> {
        let source = source();
        // Le seul cas qui mérite une ligne : Fluent DEMANDÉ, aucune police
        // installée. « Symboles » n'attend aucune police, donc il ne dit rien.
        if id_actif() == THEME_FLUENT && matches!(source, Source::Png) {
            crate::diag!("icônes : aucune police d'icônes installée, retour aux PNG");
        }
        let charger = |icone: Icone| texture_de(device, icone, source);
        Ok(Self {
            power: charger(ALIMENTATION)?,
            terminal: charger(TERMINAL)?,
            sound: charger(SON)?,
            clipboard: charger(PRESSE_PAPIERS)?,
            network: charger(RESEAU)?,
            settings: charger(REGLAGES)?,
            notes: charger(NOTES)?,
            theme: charger(THEME)?,
            night_light: charger(MODE_NUIT)?,
            printer: charger(IMPRIMANTE)?,
            calendar: charger(CALENDRIER)?,
            image_editor: charger(EDITEUR_IMAGE)?,
            screen_capture: charger(CAPTURE_ECRAN)?,
            minuteur: charger(MINUTEUR)?,
            surveillance: charger(SURVEILLANCE)?,
            windows_update: charger(WINDOWS_UPDATE)?,
            device_manager: charger(PERIPHERIQUES)?,
            observateur_evenements: charger(OBSERVATEUR_EVENEMENTS)?,
            enregistrer: charger(ENREGISTRER)?,
            clavier: charger(CLAVIER)?,
            jeu2048: charger(JEU2048)?,
            cinq_ou_plus: charger(CINQ_OU_PLUS)?,
            quadrapassel: charger(QUADRAPASSEL)?,
            lightsoff: charger(LIGHTSOFF)?,
            reversi: charger(REVERSI)?,
            bovo: charger(BOVO)?,
            display_settings: charger(REGLAGES_ECRAN)?,
            calculatrice: charger(CALCULATRICE)?,
            ssh: charger(SSH)?,
            couleurs: charger(COULEURS)?,
            optional_features: charger(FONCTIONNALITES)?,
            notifications: charger(NOTIFICATIONS)?,
            recherche: charger(RECHERCHE)?,
            automatisations: charger(AUTOMATISATIONS)?,
            meteo: charger(METEO)?,
            aide: charger(AIDE)?,
            show_desktop: charger(AFFICHER_BUREAU)?,
            bell: charger(CLOCHE)?,
            verrouillage: charger(VERROUILLAGE)?,
            caracteres: charger(CARACTERES)?,
            kaomoji: charger(KAOMOJI)?,
            proprietes_systeme: charger(PROPRIETES_SYSTEME)?,
            scanner: charger(SCANNER)?,
            shaders: charger(SHADERS)?,
            phonetique: charger(PHONETIQUE)?,
            masque: charger(MASQUE)?,
            maths: charger(MATHS)?,
            lueurs: charger(LUEURS)?,
            textures: charger(TEXTURES)?,
            narrateur: charger(NARRATEUR)?,
            loupe: charger(LOUPE)?,
            moniteur: charger(MONITEUR)?,
        })
    }
}

/// Le glyphe dans `police` si elle est donnée et que le rendu a laissé de
/// l'encre, le PNG sinon — un point de code absent d'une police ne doit pas
/// laisser une case vide dans la barre.
fn texture_de(device: &ID3D11Device, icone: Icone, source: Source) -> Result<Texture> {
    match source {
        // **LE PNG COULEUR, TEL QUEL.** On ne fait que le CHARGER : c'est
        // `TexMode::Texture`, du côté du dock, qui le dessinera sans le teinter
        // (voir `Dock::external_glyph_instance`).
        Source::Couleur(png) => Texture::from_png_bytes(device, png(&icone)),
        Source::Png => Texture::from_png_bytes(device, icone.png),
        Source::Monochrome(png) => Texture::from_png_bytes(device, png(&icone)),
        // Un point de code absent de la police ne doit pas laisser une case vide
        // dans la barre : on retombe sur le PNG.
        Source::Police(police) => match glyphe_rgba(icone.fluent, police) {
            Some(rgba) => Texture::from_rgba(device, COTE, COTE, &rgba),
            None => Texture::from_png_bytes(device, icone.png),
        },
    }
}

/// La police d'icônes du système : Segoe Fluent Icons (Windows 11), sinon
/// Segoe MDL2 Assets (Windows 10), sinon aucune. Vérifiée par son FICHIER :
/// GDI remplace sans rien dire une police absente, et le glyphe rendu serait
/// alors un caractère quelconque.
pub(crate) fn police_d_icones() -> Option<&'static str> {
    [("SegoeIcons.ttf", "Segoe Fluent Icons"), ("segmdl2.ttf", "Segoe MDL2 Assets")]
        .into_iter()
        .find(|(fichier, _)| police_installee(fichier))
        .map(|(_, nom)| nom)
}

/// Le fichier de police `fichier` est-il installé — pour toute la machine
/// (`%SystemRoot%\Fonts`) ou pour l'utilisateur seul
/// (`%LOCALAPPDATA%\Microsoft\Windows\Fonts`, là où Windows installe une
/// police sans droits d'administrateur, et que GDI voit aussi) ?
pub(crate) fn police_installee(fichier: &str) -> bool {
    let machine = std::env::var_os("SystemRoot").map(|racine| std::path::PathBuf::from(racine).join("Fonts"));
    let utilisateur =
        std::env::var_os("LOCALAPPDATA").map(|racine| std::path::PathBuf::from(racine).join(r"Microsoft\Windows\Fonts"));
    [machine, utilisateur].into_iter().flatten().any(|dossier| dossier.join(fichier).is_file())
}

/// Le glyphe `point` rendu dans un carré de `COTE` px, son encre centrée et
/// son plus grand côté ramené à `ENCRE` px. Deux rendus : le premier mesure
/// l'encre, le second est fait à la taille qui la met à `ENCRE`.
fn glyphe_rgba(point: char, police: &str) -> Option<Vec<u8>> {
    let texte = point.to_string();
    let plus_grand_cote = |w: u32, h: u32, rgba: &[u8]| {
        text::boite_de_l_encre(rgba, w, h).map(|(x0, y0, x1, y1)| (x1 - x0 + 1).max(y1 - y0 + 1))
    };
    let (w, h, rgba) = text::rendre_rgba(&texte, COTE as i32, police, false).ok()?;
    let mesure = plus_grand_cote(w, h, &rgba)?;
    let px = (COTE * ENCRE / mesure) as i32;
    let (w, h, rgba) = text::rendre_rgba(&texte, px, police, false).ok()?;
    let boite = text::boite_de_l_encre(&rgba, w, h)?;
    Some(poser_au_centre(&rgba, w, boite, COTE))
}

/// Copie l'encre `boite` (coins inclus) de `rgba` (largeur `w`) au centre d'un
/// carré transparent de `cote` px. Une encre plus grande que le carré est
/// rognée également des deux côtés.
fn poser_au_centre(rgba: &[u8], w: u32, boite: (u32, u32, u32, u32), cote: u32) -> Vec<u8> {
    let (x0, y0, x1, y1) = boite;
    let (lw, lh) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut sortie = vec![0u8; (cote * cote * 4) as usize];
    // Décalage signé de la source vers la destination : négatif quand l'encre
    // déborde du carré.
    let dx = (cote as i64 - lw as i64) / 2;
    let dy = (cote as i64 - lh as i64) / 2;
    for sy in 0..lh {
        let ty = sy as i64 + dy;
        if !(0..cote as i64).contains(&ty) {
            continue;
        }
        for sx in 0..lw {
            let tx = sx as i64 + dx;
            if !(0..cote as i64).contains(&tx) {
                continue;
            }
            let src = (((y0 + sy) * w + x0 + sx) * 4) as usize;
            let dst = ((ty as u32 * cote + tx as u32) * 4) as usize;
            sortie[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
        }
    }
    sortie
}

/// Le thème actif. `None` jusqu'au premier `activer` : le thème de base.
static ACTIF: Mutex<Option<&'static str>> = Mutex::new(None);

/// Numéro du thème actif, augmenté à chaque `activer` : `app::run` le compare
/// pour savoir quand refaire les textures.
static GENERATION: AtomicU32 = AtomicU32::new(0);

/// **LE THÈME ACTIF REND-IL DES ICÔNES EN COULEUR ?**
///
/// Un ATOMIQUE, et pas un `id_actif()` : `Dock::external_glyph_instance` le
/// demande **une fois par applet et par frame** — soit ~1700 fois par seconde —
/// et `id_actif` prend un verrou. La valeur est posée par `activer`, le seul
/// endroit qui change de thème.
static COULEUR: AtomicBool = AtomicBool::new(false);

/// Vrai quand le thème actif livre ses icônes **déjà colorées**, et que la barre
/// ne doit donc PAS les teinter. Voir `Dock::external_glyph_instance`.
pub fn est_couleur() -> bool {
    COULEUR.load(Ordering::Relaxed)
}

/// **L'ICÔNE DE L'ONGLET DE L'INTERFACE MASQUÉE** (`win32::onglet_masque`), si
/// le thème actif est EN COULEUR : le PNG du masquage de ce thème. `None` pour
/// un thème monochrome — l'onglet garde alors son œil OUVERT, teinté.
///
/// DÉFAUT SIGNALÉ (2026-10-12) : « c'est toujours l'ancienne icône quand l'app
/// est masquée ».
pub fn masque_en_couleur() -> Option<&'static [u8]> {
    match source() {
        Source::Couleur(png) => Some(png(&MASQUE)),
        Source::Png | Source::Monochrome(_) | Source::Police(_) => None,
    }
}

fn est_un_theme_couleur(id: &str) -> bool {
    matches!(id, THEME_FLUENT_EMOJI | THEME_FLUENT_EMOJI_3D)
}

pub fn id_actif() -> &'static str {
    ACTIF.lock().unwrap_or_else(|e| e.into_inner()).unwrap_or(THEME_BASE)
}

pub fn generation() -> u32 {
    GENERATION.load(Ordering::Relaxed)
}

fn trouver(id: &str) -> Option<&'static str> {
    INTEGRES.iter().map(|(i, _)| *i).find(|i| *i == id)
}

/// Rend `id` actif (sans le persister — voir `choisir`). Un identifiant
/// inconnu active le thème de base.
pub fn activer(id: &str) {
    let retenu = trouver(id).unwrap_or(THEME_BASE);
    *ACTIF.lock().unwrap_or_else(|e| e.into_inner()) = Some(retenu);
    // ⚠️ **LE DRAPEAU SE POSE ICI, LE SEUL ENDROIT QUI CHANGE DE THÈME** — le
    // laisser à `charger` le rendrait faux pour tout lecteur qui n'a pas encore
    // reconstruit ses textures (voir `est_couleur`).
    COULEUR.store(est_un_theme_couleur(retenu), Ordering::Relaxed);
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Relit le thème choisi au démarrage. `NEURADESK_DEBUG_ICONES=<id>` impose un
/// thème SANS l'enregistrer, comme `NEURADESK_DEBUG_STYLE`.
pub fn charger_choix() {
    if let Ok(id) = std::env::var("NEURADESK_DEBUG_ICONES") {
        activer(&id);
        crate::diag!("icônes : {} imposé par NEURADESK_DEBUG_ICONES", id_actif());
        return;
    }
    let id = crate::config::config_path(FICHIER_CHOIX)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| t.trim().to_string())
        .unwrap_or_default();
    activer(&id);
    crate::diag!("icônes : {} actif au démarrage", id_actif());
}

/// Rend `id` actif et le retient pour les prochains lancements. Un
/// identifiant qui ne désigne aucun thème est refusé (il vient d'une page).
pub fn choisir(id: &str) -> bool {
    let Some(id) = trouver(id) else {
        crate::diag!("icônes : {id:?} inconnu, refusé");
        return false;
    };
    activer(id);
    crate::config::write_config(FICHIER_CHOIX, id.to_string());
    crate::diag!("icônes : {id} choisi");
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **CHOISIR UN GLYPHE FLUENT EN LE REGARDANT.** Rend une liste de points
    /// de code de la police d'icônes et les affiche en ASCII — aucun catalogue
    /// ne dit ce que Segoe Fluent Icons dessine réellement, et un nom de
    /// glyphe est une supposition, pas une mesure.
    ///
    /// C'est cette sonde qui a montré que `\u{E790}` (« Color ») est une
    /// PALETTE et non une pipette — l'applet l'affichait sous le thème Fluent
    /// alors que son icône dessinée, elle, est bien une pipette.
    ///
    /// Lancer : `cargo test --bins -- --ignored --nocapture sonde_des_glyphes`
    #[test]
    #[ignore = "sonde a la main — n'ecrit rien"]
    fn sonde_des_glyphes() {
        let Some(police) = police_d_icones() else {
            println!("aucune police d'icones installee");
            return;
        };
        // E790 et E8EF sont des REPÈRES CONNUS (palette, calculatrice) : ils
        // calibrent la lecture de l'ASCII.
        // ⭐ **LES SIX DERNIERS ONT ÉTÉ AJOUTÉS POUR L'APPLET SCANNER**
        // (2026-10-09) : il n'existe aucun catalogue qui dise lequel de ces points
        // de code dessine un SCANNER — et « E8FE » était cité dans une liste de
        // candidats du dépôt sans que rien ne prouve sa forme. On les regarde.
        const A_ESSAYER: [u32; 21] = [
            0xE790, 0xE8EF, 0xEF3C, 0xE7E6, 0xE8D3, 0xE70F, 0xE71C, 0xF0E2, 0xEB9F, 0xE9A6, 0xE7A1, 0xE708,
            0xEC4A, 0xE8A5, // candidats « scanner »
            0xE8FE, 0xE91B, 0xE7B3, 0xE7E8, 0xE838, 0xE945,
            0xE943, // candidat « éditeur de shaders »
        ];
        for point in A_ESSAYER {
            let Some(c) = char::from_u32(point) else { continue };
            let Some(rgba) = glyphe_rgba(c, police) else {
                println!("\n=== U+{point:04X} : AUCUNE ENCRE ===\n");
                continue;
            };
            let mut ascii = String::new();
            for py in (0..COTE).step_by(8) {
                for px in (0..COTE).step_by(4) {
                    ascii.push(match rgba[((py * COTE + px) * 4 + 3) as usize] {
                        0..=63 => ' ',
                        64..=191 => '.',
                        _ => '#',
                    });
                }
                ascii.push('\n');
            }
            println!("\n=== U+{point:04X} ===\n{ascii}");
        }
    }

    #[test]
    fn chaque_icone_a_un_glyphe_prive_distinct() {
        let mut vus = std::collections::HashSet::new();
        for icone in TOUTES {
            // Zone d'usage privé : là où Segoe Fluent Icons range ses glyphes.
            assert!(('\u{E000}'..='\u{F8FF}').contains(&icone.fluent), "{:X}", icone.fluent as u32);
            assert!(vus.insert(icone.fluent), "glyphe en double : {:X}", icone.fluent as u32);
        }
    }

    #[test]
    fn les_png_ont_le_cote_attendu() {
        for icone in TOUTES {
            let image = image::load_from_memory(icone.png).unwrap();
            assert_eq!((image.width(), image.height()), (COTE, COTE));
        }
    }

    /// **LES PNG DES THÈMES EN COULEUR SONT BIEN EN COULEUR — ET C'EST LE
    /// CONTRÔLE QUI COMPTE.**
    ///
    /// ⚠️ Tous les autres PNG du dépôt sont MONOCHROMES : le shader n'en lit que
    /// l'alpha et les repeint dans la teinte du thème. Si l'un de ceux-ci
    /// ressortait d'une seule teinte, il passerait inaperçu à l'œil **et** à la
    /// capture — il serait simplement BLANC dans la barre, ce qui est exactement
    /// ce que ces thèmes ne doivent pas faire.
    ///
    /// ⚠️ **La taille aussi** : `poser_au_centre` des glyphes de police vise
    /// `ENCRE` px dans un carré de `COTE`, et `tools/rasteriser_fluent_emoji.py`
    /// fait de même. Une icône d'une autre taille se dessinerait à une échelle
    /// différente des autres.
    ///
    /// ⛔ **LE THÈME « SYMBOLES » N'EST PLUS DANS CETTE BOUCLE** : ses PNG venaient
    /// de Segoe UI Emoji et sont partis avec lui (voir l'en-tête du module).
    #[test]
    fn les_themes_en_couleur_sont_en_couleur_et_a_la_bonne_taille() {
        for png in TOUTES.iter().flat_map(|i| [i.fluent_couleur, i.fluent_3d]) {
            let image = image::load_from_memory(png).unwrap().to_rgba8();
            assert_eq!((image.width(), image.height()), (COTE, COTE), "cote du symbole");

            let mut teintes = std::collections::HashSet::new();
            for pixel in image.pixels() {
                // ⚠️ **ON IGNORE LE FOND TRANSPARENT** : ses pixels portent le
                // noir du PNG, et le compter comme une teinte ferait passer un
                // glyphe monochrome pour coloré.
                if pixel.0[3] > 40 {
                    teintes.insert([pixel.0[0], pixel.0[1], pixel.0[2]]);
                }
            }
            assert!(teintes.len() > 1, "un symbole d'une seule teinte n'est pas en couleur");
        }
    }

    /// ⭐ L'ALIMENTATION EST UN BOUTON POWER DANS TOUS LES THÈMES — DEMANDE
    /// EXPLICITE (2026-10-15). Les trois thèmes en couleur montraient la PRISE 🔌
    /// (violette) ; `tools/dessiner_bouton_power.py` les remplace par un disque
    /// ROUGE portant le symbole en blanc. Une régénération qui oublierait de
    /// l'appeler ferait revenir la prise : ce test la verrait.
    #[test]
    fn l_alimentation_en_couleur_est_le_bouton_power_rouge() {
        for png in [ALIMENTATION.fluent_couleur, ALIMENTATION.fluent_3d] {
            let image = image::load_from_memory(png).unwrap().to_rgba8();
            let (mut rouges, mut blancs, mut opaques) = (0, 0, 0);
            for p in image.pixels().filter(|p| p.0[3] > 200) {
                let [r, v, b, _] = p.0.map(i32::from);
                opaques += 1;
                if r > v + 60 && r > b + 60 {
                    rouges += 1;
                } else if r > 220 && v > 220 && b > 220 {
                    blancs += 1;
                }
            }
            assert!(rouges * 10 > opaques * 6, "le disque est ROUGE : {rouges}/{opaques}");
            assert!(blancs * 100 > opaques * 5, "et porte le symbole en BLANC : {blancs}/{opaques}");
        }
        assert_eq!(ALIMENTATION.fluent, '\u{E7E8}', "Fluent : le glyphe PowerButton");
    }

    /// Les deux thèmes en couleur sont dessinés tels quels, les trois autres
    /// teintés par la barre.
    #[test]
    fn seuls_les_themes_en_couleur_le_disent() {
        for (id, _) in INTEGRES {
            assert_eq!(est_un_theme_couleur(id), matches!(source_de(id), Source::Couleur(_)), "{id}");
        }
        assert_eq!(INTEGRES.iter().filter(|(id, _)| est_un_theme_couleur(id)).count(), 2);
    }

    /// **LES PNG D'« ENERGY ICONS » SONT MONOCHROMES ET AU GABARIT** — l'inverse
    /// exact des thèmes en couleur : une seule teinte (le BLANC que la barre repeint), et
    /// l'encre ramenée à `ENCRE` px, centrée, comme les glyphes Fluent.
    #[test]
    fn les_icones_energy_sont_blanches_et_au_gabarit() {
        for icone in TOUTES {
            let image = image::load_from_memory(icone.energy).unwrap().to_rgba8();
            assert_eq!((image.width(), image.height()), (COTE, COTE));
            assert!(
                image.pixels().filter(|p| p.0[3] > 0).all(|p| p.0[..3] == [255, 255, 255]),
                "encre non blanche : la barre ne la teinterait pas"
            );
            let (x0, y0, x1, y1) = text::boite_de_l_encre(image.as_raw(), COTE, COTE).expect("de l'encre");
            let cote = (x1 - x0 + 1).max(y1 - y0 + 1);
            assert!(cote.abs_diff(ENCRE) <= 2, "encre de {cote} px");
            assert!((x0 + x1).abs_diff(COTE - 1) <= 2 && (y0 + y1).abs_diff(COTE - 1) <= 2, "décentrée");
        }
        assert!(matches!(source_de(THEME_ENERGY), Source::Monochrome(_)));
    }

    #[test]
    fn l_encre_est_posee_au_centre() {
        // Une encre de 2×4 px en (1, 1) d'une source de 6 px de large.
        let (w, h) = (6u32, 6u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 1..5 {
            for x in 1..3 {
                rgba[((y * w + x) * 4 + 3) as usize] = 255;
            }
        }
        let sortie = poser_au_centre(&rgba, w, (1, 1, 2, 4), 8);
        assert_eq!(text::boite_de_l_encre(&sortie, 8, 8), Some((3, 2, 4, 5)));
    }

    #[test]
    fn une_encre_trop_grande_est_rognee_sans_paniquer() {
        let (w, h) = (10u32, 10u32);
        let rgba = vec![255u8; (w * h * 4) as usize];
        let sortie = poser_au_centre(&rgba, w, (0, 0, 9, 9), 4);
        assert_eq!(text::boite_de_l_encre(&sortie, 4, 4), Some((0, 0, 3, 3)));
    }

    /// La mesure réelle, sur la police du système : chaque glyphe rendu tient
    /// dans le carré à la taille d'encre des PNG, à un pixel près.
    #[test]
    fn chaque_glyphe_fluent_est_rendu_a_la_taille_des_png() {
        let Some(police) = police_d_icones() else { return };
        for icone in TOUTES {
            let rgba = glyphe_rgba(icone.fluent, police).unwrap_or_else(|| panic!("{:X} sans encre", icone.fluent as u32));
            let (x0, y0, x1, y1) = text::boite_de_l_encre(&rgba, COTE, COTE).unwrap();
            let cote = (x1 - x0 + 1).max(y1 - y0 + 1);
            assert!(cote.abs_diff(ENCRE) <= 2, "{:X} : encre de {cote} px", icone.fluent as u32);
            let (cx, cy) = (x0 + x1, y0 + y1);
            assert!(cx.abs_diff(COTE - 1) <= 2 && cy.abs_diff(COTE - 1) <= 2, "{:X} décentré", icone.fluent as u32);
        }
    }

    #[test]
    fn un_theme_inconnu_est_refuse_et_retombe_sur_la_base() {
        assert!(trouver("inexistant").is_none());
        assert_eq!(trouver("fluent"), Some(THEME_FLUENT));
    }
}
