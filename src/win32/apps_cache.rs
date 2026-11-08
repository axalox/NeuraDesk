//! Persistance sur disque de l'énumération des applications (voir
//! `win32::apps::enumerate_apps`) et des icônes déjà extraites/encodées
//! (voir `win32::dash::ICON_CACHE`) — pour ne plus payer le coût d'un scan
//! complet (parcours du menu Démarrer, énumération UWP, bibliothèques
//! Steam/GOG/Epic) NI celui de l'extraction des icônes (`SHGetFileInfoW`/
//! `IImageList`, la partie la plus coûteuse en pratique, voir
//! `win32::dash::jumbo_icon_rgba`) à CHAQUE ouverture du Dash — seulement au
//! tout premier lancement de NeuraDesk (aucun cache encore sur le disque),
//! après un changement de catégorie (qui a besoin d'un résultat à jour,
//! voir `win32::dash::toggle_category` et consorts) et quand l'utilisateur
//! clique explicitement sur le bouton d'actualisation à côté de la barre de
//! recherche du Dash (voir `dash_template.html`, `win32::dash::refresh_apps`).
//!
//! Deux fichiers séparés dans `%APPDATA%\NeuraDesk\` :
//! - `apps_cache.toml` : la liste elle-même (nom, description, catégories,
//!   mécanisme de lancement) — JAMAIS les icônes (voir ci-dessous), pour
//!   rester un fichier texte de taille raisonnable.
//! - `icons_cache.toml` : les icônes déjà résolues, encodées en data URI
//!   base64 PNG (voir `win32::dash::icon_data_uri`/`icon_cache_key`) — à
//!   part parce que c'est la donnée qui pèse le plus lourd (plusieurs
//!   dizaines de Ko par icône) ; un simple `Vec<AppEntry>` sans icônes
//!   suffit à reconstruire une page fonctionnelle (les icônes manquantes se
//!   comportent comme n'importe quel échec d'extraction, voir
//!   `build_apps_json`), donc pas la peine de faire échouer TOUT le cache
//!   applications si ce second fichier est absent/corrompu.
//!
//! Aucune icône (`icon_rgba`) n'est reconstruite depuis `apps_cache.toml` —
//! volontairement `None` pour chaque `AppEntry` désérialisée : `icon_path`
//! (conservé, lui) suffit à `icon_data_uri` pour retrouver l'icône d'un jeu
//! Steam/GOG/Epic en cas d'absence dans `icons_cache.toml`, et une
//! application UWP (dont l'icône vient normalement d'`icon_rgba`, jamais
//! d'`icon_path`) retombe simplement sans icône dans ce même cas rare — un
//! clic sur "Actualiser" (voir `win32::dash::refresh_apps`) revient alors à
//! un scan complet, qui la recalcule.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::config::{read_cache, write_cache};
use crate::win32::apps::{AppEntry, AppLaunch};

/// Relit la liste d'applications mise en cache — `None` si le fichier est
/// absent, illisible, mal formé, OU VIDE (un cache vide n'a aucune raison
/// de représenter un vrai système Windows : mieux vaut retomber sur un scan
/// complet que présenter indéfiniment un Dash sans aucune application).
/// L'appelant (`win32::dash::push_apps_update`) déclenche alors ce scan
/// lui-même.
pub fn load_apps() -> Option<Vec<AppEntry>> {
    let content = read_cache("apps_cache.toml")?;
    let table = content.parse::<toml::Table>().ok()?;
    let entries = table.get("apps")?.as_array()?;
    let apps: Vec<AppEntry> = entries.iter().filter_map(|v| v.as_table().and_then(app_from_toml)).collect();
    // ⛔ Un cache écrit avant `apps::cible_msi` lance des LEURRES (un .ico) : il
    // est rescanné, comme un cache absent.
    (!apps.is_empty() && !contient_un_leurre_msi(&apps)).then_some(apps)
}

/// Une entrée de `apps` lance-t-elle le leurre d'un raccourci MSI (voir
/// `apps::est_un_leurre_msi`) ?
fn contient_un_leurre_msi(apps: &[AppEntry]) -> bool {
    apps.iter().any(|a| matches!(&a.launch, AppLaunch::Shortcut { target, .. } if crate::win32::apps::est_un_leurre_msi(target)))
}

/// Sauvegarde `apps` — réécrit le fichier entier à chaque appel (même
/// discipline que `config::categories.toml`/`usage.toml`), best-effort
/// silencieux si `%APPDATA%` est inaccessible en écriture.
pub fn save_apps(apps: &[AppEntry]) {
    let entries: Vec<toml::Value> = apps.iter().map(|app| toml::Value::Table(app_to_toml(app))).collect();
    let mut table = toml::Table::new();
    table.insert("apps".to_string(), toml::Value::Array(entries));
    write_cache("apps_cache.toml", table.to_string());
}

/// Relit les icônes mises en cache (clé voir `win32::dash::icon_cache_key`,
/// valeur une data URI base64 déjà prête à l'emploi) — table vide si le
/// fichier est absent/illisible/mal formé, jamais un échec bloquant : une
/// icône manquante ici se comporte simplement comme un échec d'extraction
/// (voir doc de module).
pub fn load_icons() -> HashMap<String, String> {
    let Some(content) = read_cache("icons_cache.toml") else { return HashMap::new() };
    let Ok(table) = content.parse::<toml::Table>() else { return HashMap::new() };
    let Some(icons) = table.get("icons").and_then(|v| v.as_table()) else { return HashMap::new() };
    icons.iter().filter_map(|(key, value)| value.as_str().map(|uri| (key.clone(), uri.to_string()))).collect()
}

/// Sauvegarde `icons` — réécrit le fichier entier à chaque appel, comme
/// `save_apps`. `toml::Table` (pas un format fait main) : les clés sont des
/// chemins de fichier/identifiants UWP/URI arbitraires (voir
/// `win32::dash::icon_cache_key`), potentiellement porteurs de caractères
/// qu'une sérialisation TOML correcte doit échapper — même raisonnement que
/// `config::categories.toml` pour ses clés de noms d'application.
pub fn save_icons(icons: &HashMap<String, String>) {
    let mut icons_table = toml::Table::new();
    for (key, uri) in icons {
        icons_table.insert(key.clone(), toml::Value::String(uri.clone()));
    }
    let mut table = toml::Table::new();
    table.insert("icons".to_string(), toml::Value::Table(icons_table));
    write_cache("icons_cache.toml", table.to_string());
}

/// Sérialise une `AppEntry` — jamais `icon_rgba` (voir doc de module), et
/// `icon_path` seulement s'il est présent (voir sa doc dans `apps.rs` :
/// absent pour la quasi-totalité des applications, uniquement utilisé pour
/// les jeux Steam/GOG/Epic dont l'icône ne vient pas de `AppLaunch`).
fn app_to_toml(app: &AppEntry) -> toml::Table {
    let mut row = toml::Table::new();
    row.insert("name".to_string(), toml::Value::String(app.name.clone()));
    row.insert("description".to_string(), toml::Value::String(app.description.clone()));
    row.insert(
        "categories".to_string(),
        toml::Value::Array(app.categories.iter().cloned().map(toml::Value::String).collect()),
    );
    row.insert("overridden".to_string(), toml::Value::Boolean(app.overridden));
    if let Some(icon_path) = &app.icon_path {
        row.insert("icon_path".to_string(), toml::Value::String(icon_path.to_string_lossy().into_owned()));
    }
    match &app.launch {
        AppLaunch::Shortcut { target, shortcut_path } => {
            row.insert("launch_kind".to_string(), toml::Value::String("shortcut".to_string()));
            row.insert("launch_target".to_string(), toml::Value::String(target.to_string_lossy().into_owned()));
            row.insert(
                "launch_shortcut_path".to_string(),
                toml::Value::String(shortcut_path.to_string_lossy().into_owned()),
            );
        }
        AppLaunch::Uwp { app_user_model_id } => {
            row.insert("launch_kind".to_string(), toml::Value::String("uwp".to_string()));
            row.insert("launch_app_user_model_id".to_string(), toml::Value::String(app_user_model_id.clone()));
        }
        AppLaunch::UriProtocol { uri } => {
            row.insert("launch_kind".to_string(), toml::Value::String("uri".to_string()));
            row.insert("launch_uri".to_string(), toml::Value::String(uri.clone()));
        }
    }
    row
}

/// Reconstruit une `AppEntry` depuis une ligne de `apps_cache.toml` — `None`
/// si un champ obligatoire manque ou est mal typé (fichier corrompu/écrit
/// par une version incompatible) : cette seule entrée est alors omise (voir
/// `load_apps`, `filter_map`) plutôt que de faire échouer tout le
/// rechargement.
fn app_from_toml(row: &toml::Table) -> Option<AppEntry> {
    let name = row.get("name")?.as_str()?.to_string();
    let description = row.get("description").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let categories: Vec<String> = row
        .get("categories")
        .and_then(|v| v.as_array())
        .map(|entries| entries.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let overridden = row.get("overridden").and_then(|v| v.as_bool()).unwrap_or(false);
    let icon_path = row.get("icon_path").and_then(|v| v.as_str()).map(PathBuf::from);

    let launch = match row.get("launch_kind")?.as_str()? {
        "shortcut" => AppLaunch::Shortcut {
            target: PathBuf::from(row.get("launch_target")?.as_str()?),
            shortcut_path: PathBuf::from(row.get("launch_shortcut_path")?.as_str()?),
        },
        "uwp" => AppLaunch::Uwp { app_user_model_id: row.get("launch_app_user_model_id")?.as_str()?.to_string() },
        "uri" => AppLaunch::UriProtocol { uri: row.get("launch_uri")?.as_str()?.to_string() },
        _ => return None,
    };

    // `categories` ne doit jamais être vide (voir sa doc dans `apps.rs`) —
    // une ligne qui le serait (fichier corrompu) est rejetée plutôt que de
    // laisser passer une `AppEntry` qui violerait cette invariant.
    if categories.is_empty() {
        return None;
    }

    Some(AppEntry { name, description, categories, overridden, launch, icon_rgba: None, icon_path })
}
