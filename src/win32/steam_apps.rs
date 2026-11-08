//! Découverte des jeux installés via Steam — comme pour les applications
//! UWP/Store (voir `win32::uwp_apps`), Steam ne crée PAS de raccourci de
//! menu Démarrer par jeu (juste un raccourci vers Steam.exe lui-même,
//! vérifié en pratique) : ses centaines de jeux sont donc invisibles à
//! `win32::apps::enumerate_apps`, qui ne scanne que le menu Démarrer. Steam
//! gère sa propre bibliothèque dans des fichiers texte au format VDF
//! ("Valve Data Format"/KeyValues, vérifié en pratique sur une installation
//! réelle) — `steamapps/libraryfolders.vdf` (sous le dossier d'installation
//! de Steam lui-même) liste les dossiers de bibliothèque configurés (un jeu
//! peut être installé sur n'importe lequel des disques ajoutés), et chaque
//! dossier `<bibliothèque>/steamapps/appmanifest_<id>.acf` décrit un jeu
//! installé (identifiant et nom).
//!
//! Icône (voir `SteamApp::icon_path`, `AppEntry::icon_path`) : Steam met en
//! cache localement un `.ico` par jeu dans `<SteamPath>\steam\games\`, dont
//! le nom de fichier est le hash `"clienticon"` de ce jeu — une valeur
//! propre au catalogue Steam (PAS calculable, il faut la lire), stockée
//! dans `appcache\appinfo.vdf`, un cache BINAIRE (magie `0x07564429`,
//! format KeyValues binaire de Valve, table de chaînes partagée) que
//! `steam-vdf-parser` sait lire — vérifié en pratique : `common.clienticon`
//! correspond exactement au fichier caché localement, contrairement à
//! `common.icon` (une chaîne différente, pour l'image de la page boutique,
//! jamais mise en cache localement sous ce nom). Absent pour les entrées
//! qui ne sont pas de vrais jeux lançables (redistribuables partagés...) ou
//! si le fichier n'a jamais été mis en cache localement — dans ce cas,
//! `icon_path` reste `None` et la tuile n'a simplement pas d'icône (voir
//! doc de `win32::dash::icon_data_uri`, qui gère déjà ce cas pour toute
//! application).
//!
//! Lancement : `steam://rungameid/<appid>`, le même protocole que Steam
//! utilise lui-même pour ses raccourcis de bureau/menu Démarrer — PAS le
//! même chemin que l'icône (voir `AppEntry::icon_path`, séparé de
//! `AppLaunch` précisément pour ce genre de cas où la source de l'icône et
//! le mécanisme de lancement n'ont rien à voir l'un avec l'autre).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use windows::Win32::System::Registry::HKEY_CURRENT_USER;

use crate::win32::registry_util;

/// Un jeu Steam installé.
pub struct SteamApp {
    pub name: String,
    pub appid: String,
    /// Voir doc de module — `None` si pas de `.ico` mis en cache localement
    /// pour ce jeu (redistribuable partagé, cache jamais peuplé...).
    pub icon_path: Option<PathBuf>,
}

const STEAM_REGISTRY_KEY: &str = r"Software\Valve\Steam";
const STEAM_PATH_VALUE: &str = "SteamPath";

/// Énumère les jeux installés dans toutes les bibliothèques Steam
/// configurées (voir doc de module) — best-effort : liste vide si Steam
/// n'est pas installé ou si une étape échoue, jamais une erreur qui ferait
/// échouer tout le scan (voir `apps::enumerate_apps`, qui fusionne ce
/// résultat avec les autres sources).
pub fn enumerate() -> Vec<SteamApp> {
    let Some(steam_path) = registry_util::read_string_value(HKEY_CURRENT_USER, STEAM_REGISTRY_KEY, STEAM_PATH_VALUE)
    else {
        return Vec::new();
    };
    // Steam écrit `SteamPath` avec des SLASHS AVANT (`c:/program files
    // (x86)/steam`, vérifié en pratique), pas des antislashs comme
    // n'importe quel autre chemin Windows — `PathBuf::join` ne les convertit
    // PAS, ce qui donnerait un chemin aux séparateurs mélangés une fois
    // "steam"/"games"/"<hash>.ico" ajoutés (voir `clienticon_path`). Les
    // API `std::fs` (donc `is_file`) tolèrent très bien ce mélange, mais PAS
    // `SHGetFileInfoW` (voir `win32::dash::jumbo_icon_rgba`/
    // `large_icon_rgba`, qui l'utilisent aussi sur ce chemin) : vérifié en
    // pratique, elle échoue silencieusement (renvoie `None`) sur un chemin
    // aux séparateurs mélangés alors qu'elle réussit sur le même chemin
    // entièrement en antislashs — d'où cette normalisation, faite une seule
    // fois ici plutôt que dans chaque fonction qui consomme ensuite ce
    // chemin.
    let steam_path = PathBuf::from(steam_path.replace('/', "\\"));

    // Lu une seule fois (voir sa doc — c'est un cache binaire de plusieurs
    // Mo couvrant tout le catalogue Steam, pas seulement les jeux
    // installés), puis interrogé jeu par jeu ci-dessous plutôt que
    // ré-analysé à chaque itération.
    let appinfo_data = std::fs::read(steam_path.join("appcache").join("appinfo.vdf")).ok();
    let appinfo = appinfo_data.as_deref().and_then(|data| steam_vdf_parser::parse_appinfo(data).ok());

    let mut apps = Vec::new();
    let mut seen_appids = HashSet::new();
    for library in library_folders(&steam_path) {
        let steamapps_dir = library.join("steamapps");
        let Ok(entries) = std::fs::read_dir(&steamapps_dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_manifest =
                path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("appmanifest_") && n.ends_with(".acf"));
            if !is_manifest {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            let Some(appid) = extract_vdf_value(&content, "appid") else { continue };
            let Some(name) = extract_vdf_value(&content, "name") else { continue };
            if name.trim().is_empty() || !seen_appids.insert(appid.clone()) {
                continue;
            }
            let icon_path = appinfo.as_ref().and_then(|vdf| clienticon_path(vdf, &steam_path, &appid));
            apps.push(SteamApp { name, appid, icon_path });
        }
    }
    apps
}

/// Résout le chemin du `.ico` mis en cache localement pour `appid` (voir
/// doc de module) — `None` si `appid` n'a pas de champ `clienticon` dans
/// `appinfo`, ou si le fichier qu'il désigne n'existe pas/plus localement
/// (jamais mis en cache, ou cache nettoyé entretemps).
fn clienticon_path(appinfo: &steam_vdf_parser::Vdf, steam_path: &Path, appid: &str) -> Option<PathBuf> {
    let hash = appinfo.get_str(&[appid, "appinfo", "common", "clienticon"])?;
    let ico_path = steam_path.join("steam").join("games").join(format!("{hash}.ico"));
    ico_path.is_file().then_some(ico_path)
}

/// Dossiers de bibliothèque Steam configurés (voir doc de module). Le
/// dossier d'installation de Steam lui-même (`steam_path`) EST TOUJOURS une
/// bibliothèque valide en pratique (`libraryfolders.vdf` liste d'ailleurs
/// une entrée "0" qui y pointe déjà), mais on l'ajoute explicitement pour
/// rester correct même si jamais absente du fichier, plutôt que de dépendre
/// entièrement de son contenu.
fn library_folders(steam_path: &Path) -> Vec<PathBuf> {
    let mut folders = vec![steam_path.to_path_buf()];
    let vdf_path = steam_path.join("steamapps").join("libraryfolders.vdf");
    if let Ok(content) = std::fs::read_to_string(&vdf_path) {
        for raw_path in extract_vdf_values(&content, "path") {
            let path = PathBuf::from(unescape_vdf(&raw_path));
            if !folders.contains(&path) {
                folders.push(path);
            }
        }
    }
    folders
}

/// Défait l'échappement VDF des antislashs (`\\` → `\`) — la seule séquence
/// d'échappement que ce module a besoin de comprendre pour le VDF TEXTE
/// (voir `extract_vdf_values` — `appinfo.vdf`, lui, est binaire et lu via
/// `steam-vdf-parser`, pas concerné) : les chemins Windows dans
/// `libraryfolders.vdf` sont écrits avec des antislashs doublés (ex.
/// `"C:\\Program Files (x86)\\Steam"`).
fn unescape_vdf(s: &str) -> String {
    s.replace("\\\\", "\\")
}

/// Première valeur associée à `key` (voir `extract_vdf_values`) — pour les
/// champs qui n'apparaissent qu'une fois par fichier (`"appid"`, `"name"`
/// d'un `appmanifest_*.acf`).
fn extract_vdf_value(content: &str, key: &str) -> Option<String> {
    extract_vdf_values(content, key).into_iter().next()
}

/// Extrait toutes les valeurs associées à la clé `key` dans un contenu VDF
/// TEXTE (format "Valve Data Format"/KeyValues — voir `clienticon_path`
/// pour le pendant BINAIRE, un format entièrement différent) — PAS un vrai
/// parseur VDF (la structure `{}` imbriquée est entièrement ignorée) : une
/// simple lecture ligne par ligne qui reconnaît le motif
/// `"clé"    "valeur"`, largement suffisante pour les deux seuls usages de
/// ce module (`"path"` dans `libraryfolders.vdf`, un par bibliothèque
/// configurée ; `"appid"`/`"name"` dans un `appmanifest_*.acf`, chacun une
/// seule fois) : ces clés n'apparaissent jamais nichées sous un autre sens
/// que celui voulu dans ces fichiers précis — écrire un vrai parseur
/// récursif pour un besoin aussi ciblé serait de la complexité pure perte.
fn extract_vdf_values(content: &str, key: &str) -> Vec<String> {
    let quoted_key = format!("\"{key}\"");
    content
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix(&quoted_key)?.trim_start();
            let rest = rest.strip_prefix('"')?;
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        })
        .collect()
}
