//! Découverte des jeux installés via l'Epic Games Launcher — comme Steam
//! (voir `win32::steam_apps`) et GOG Galaxy (voir `win32::gog_apps`), Epic
//! ne crée pas de raccourci de menu Démarrer par jeu. Chaque jeu installé a
//! un fichier de "manifeste" JSON dans `%ProgramData%\Epic\
//! EpicGamesLauncher\Data\Manifests\*.item` (format constaté en pratique,
//! aucune API officielle documentée par Epic) donnant le dossier
//! d'installation et l'exécutable de lancement.
//!
//! Contrairement à Steam, un vrai chemin de fichier exécutable est
//! disponible : `win32::apps::enumerate_apps` réutilise donc tel quel le
//! même mécanisme de lancement/icône qu'un raccourci classique (voir
//! `AppLaunch::Shortcut`), pas besoin d'un protocole `com.epicgames.launcher://`
//! ni de deviner un identifiant de catalogue.

use std::path::PathBuf;

/// Un jeu Epic Games installé.
pub struct EpicApp {
    pub name: String,
    pub exe: PathBuf,
}

/// Énumère les jeux Epic Games installés (voir doc de module) — best-effort,
/// liste vide si le launcher n'est pas installé ou si le dossier de
/// manifestes est absent/illisible.
pub fn enumerate() -> Vec<EpicApp> {
    let Some(program_data) = std::env::var_os("ProgramData") else { return Vec::new() };
    let manifests_dir = PathBuf::from(program_data).join(r"Epic\EpicGamesLauncher\Data\Manifests");
    let Ok(entries) = std::fs::read_dir(&manifests_dir) else { return Vec::new() };

    entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|e| e.eq_ignore_ascii_case("item")))
        .filter_map(|entry| parse_manifest(&entry.path()))
        .collect()
}

/// Lit et interprète un fichier `.item` (JSON) — `None` si illisible, mal
/// formé, s'il manque un champ essentiel (voir les noms de champs
/// littéraux ci-dessous, ceux qu'Epic écrit dans ses manifestes), ou si
/// l'exécutable qu'il désigne n'existe plus sur le disque (désinstallation
/// partielle, manifeste orphelin...).
fn parse_manifest(path: &std::path::Path) -> Option<EpicApp> {
    let content = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;

    let name = json.get("DisplayName")?.as_str()?.trim().to_string();
    let install_location = json.get("InstallLocation")?.as_str()?;
    let launch_executable = json.get("LaunchExecutable")?.as_str()?;
    if name.is_empty() || install_location.is_empty() || launch_executable.is_empty() {
        return None;
    }

    let exe = PathBuf::from(install_location).join(launch_executable);
    exe.is_file().then_some(EpicApp { name, exe })
}
