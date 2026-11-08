//! Découverte des jeux GOG installés via le registre — complète les jeux
//! GOG déjà trouvés par `win32::apps` (les installeurs GOG autonomes créent
//! un raccourci de menu Démarrer classique, voir son scan) pour ceux
//! installés SEULEMENT via GOG Galaxy, qui n'en crée pas systématiquement.
//! Chaque jeu GOG installé (par Galaxy ou par un installeur autonome) écrit
//! une sous-clé sous `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games\<id>` avec son
//! nom et le chemin de son exécutable (vérifié en pratique) — un format
//! bien plus simple et stable que la base SQLite interne de Galaxy
//! (`galaxy-2.0.db`), volontairement pas lue directement ici.
//!
//! Contrairement à Steam (voir `win32::steam_apps`), un vrai chemin de
//! fichier exécutable est disponible : `win32::apps::enumerate_apps`
//! réutilise donc tel quel le même mécanisme de lancement/icône qu'un
//! raccourci classique (voir `AppLaunch::Shortcut`), pas besoin d'un
//! protocole `goggalaxy://` ni d'une variante `AppLaunch` à part.

use std::path::PathBuf;

use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;

use crate::win32::registry_util;

const GOG_GAMES_KEY: &str = r"SOFTWARE\WOW6432Node\GOG.com\Games";

/// Un jeu GOG installé.
pub struct GogApp {
    pub name: String,
    pub exe: PathBuf,
}

/// Énumère les jeux GOG installés (voir doc de module) — best-effort, liste
/// vide si la clé de registre est absente (aucun jeu GOG installé) ou si
/// une étape quelconque échoue pour une sous-clé donnée (celle-là est
/// simplement ignorée, le reste de l'énumération continue).
pub fn enumerate() -> Vec<GogApp> {
    registry_util::enumerate_subkeys(HKEY_LOCAL_MACHINE, GOG_GAMES_KEY)
        .into_iter()
        .filter_map(|id| {
            let subkey = format!(r"{GOG_GAMES_KEY}\{id}");
            let name = registry_util::read_string_value(HKEY_LOCAL_MACHINE, &subkey, "gameName")?;
            let exe = registry_util::read_string_value(HKEY_LOCAL_MACHINE, &subkey, "exe")?;
            if name.trim().is_empty() || exe.trim().is_empty() {
                return None;
            }
            Some(GogApp { name, exe: PathBuf::from(exe) })
        })
        .collect()
}
