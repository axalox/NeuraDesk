//! Les PROGRAMMES PORTABLES du Dash — DEMANDE EXPLICITE (2026-10-11) :
//! « j'aimerais dans le dash du dock pouvoir ajouter un programme portable à la
//! liste des programmes ».
//!
//! Un programme portable n'a pas de raccourci dans le menu Démarrer : le
//! balayage de `win32::apps` ne peut pas le trouver. On garde donc la liste des
//! exécutables choisis par l'utilisateur dans `programmes_portables.toml`, et
//! `apps::enumerate_apps` les ajoute comme des raccourcis classiques
//! (`AppLaunch::Shortcut`, l'exécutable servant aussi de source d'icône — comme
//! GOG et Epic). Ils se classent, s'épinglent et se lancent comme les autres.

use std::path::{Path, PathBuf};

use crate::config;

const FICHIER: &str = "programmes_portables.toml";
const MAX_PROGRAMMES: usize = 200;

/// Le nom affiché : celui du fichier, sans `.exe`.
pub fn nom(chemin: &Path) -> String {
    chemin.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn meme_chemin(a: &Path, b: &Path) -> bool {
    a.as_os_str().to_string_lossy().eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
}

fn depuis_toml(texte: &str) -> Vec<PathBuf> {
    let Ok(table) = texte.parse::<toml::Table>() else { return Vec::new() };
    let mut chemins: Vec<PathBuf> = Vec::new();
    let lignes = table.get("programmes").and_then(|v| v.as_array()).map(Vec::as_slice).unwrap_or_default();
    for chemin in lignes.iter().filter_map(|v| v.get("chemin").and_then(|c| c.as_str())) {
        let chemin = PathBuf::from(chemin.trim());
        if chemins.len() < MAX_PROGRAMMES && !chemin.as_os_str().is_empty() && !chemins.iter().any(|c| meme_chemin(c, &chemin)) {
            chemins.push(chemin);
        }
    }
    chemins
}

fn vers_toml(chemins: &[PathBuf]) -> String {
    use toml::Value;
    let lignes = chemins
        .iter()
        .map(|c| {
            let mut t = toml::Table::new();
            t.insert("chemin".into(), Value::String(c.to_string_lossy().into_owned()));
            Value::Table(t)
        })
        .collect();
    let mut table = toml::Table::new();
    table.insert("programmes".into(), Value::Array(lignes));
    toml::to_string(&table).unwrap_or_default()
}

pub fn charger() -> Vec<PathBuf> {
    config::config_path(FICHIER).and_then(|p| std::fs::read_to_string(p).ok()).map(|t| depuis_toml(&t)).unwrap_or_default()
}

fn ecrire(chemins: &[PathBuf]) -> bool {
    config::write_config_atomique(FICHIER, vers_toml(chemins))
}

/// `liste` plus `chemin`, s'il n'y est pas déjà ; `None` sinon.
fn avec(liste: &[PathBuf], chemin: &Path) -> Option<Vec<PathBuf>> {
    if liste.len() >= MAX_PROGRAMMES || liste.iter().any(|c| meme_chemin(c, chemin)) {
        return None;
    }
    Some(liste.iter().cloned().chain(std::iter::once(chemin.to_path_buf())).collect())
}

/// `liste` sans `chemin` ; `None` s'il n'y était pas.
fn sans(liste: &[PathBuf], chemin: &Path) -> Option<Vec<PathBuf>> {
    let reste: Vec<PathBuf> = liste.iter().filter(|c| !meme_chemin(c, chemin)).cloned().collect();
    (reste.len() != liste.len()).then_some(reste)
}

/// Ajoute `chemin` ; `false` s'il y était déjà ou n'a pas pu être écrit.
pub fn ajouter(chemin: &Path) -> bool {
    avec(&charger(), chemin).is_some_and(|l| ecrire(&l))
}

/// Retire `chemin` ; `false` s'il n'y était pas ou n'a pas pu être écrit.
pub fn retirer(chemin: &Path) -> bool {
    sans(&charger(), chemin).is_some_and(|l| ecrire(&l))
}

/// `chemin` est-il un programme portable de `liste` ?
pub fn contient(liste: &[PathBuf], chemin: &Path) -> bool {
    liste.iter().any(|c| meme_chemin(c, chemin))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_fait_l_aller_retour_sans_doublon() {
        let l = vec![PathBuf::from(r"D:\Outils\a b\Prog.exe"), PathBuf::from(r"E:\x'y\z.exe")];
        assert_eq!(depuis_toml(&vers_toml(&l)), l);
        let double = "[[programmes]]\nchemin = 'C:\\A.exe'\n[[programmes]]\nchemin = 'c:\\a.EXE'\n[[programmes]]\nchemin = ' '\n";
        assert_eq!(depuis_toml(double), vec![PathBuf::from(r"C:\A.exe")]);
        assert!(depuis_toml("[[[ cassé").is_empty());
    }

    #[test]
    fn ajouter_et_retirer_ignorent_la_casse() {
        let l = vec![PathBuf::from(r"D:\P\Outil.exe")];
        assert!(avec(&l, Path::new(r"d:\p\outil.exe")).is_none());
        assert_eq!(avec(&l, Path::new(r"D:\Q.exe")).unwrap().len(), 2);
        assert_eq!(sans(&l, Path::new(r"d:\P\OUTIL.exe")).unwrap(), Vec::<PathBuf>::new());
        assert!(sans(&l, Path::new(r"D:\Q.exe")).is_none());
        assert!(contient(&l, Path::new(r"D:\p\outil.EXE")));
    }

    #[test]
    fn le_nom_est_celui_du_fichier() {
        assert_eq!(nom(Path::new(r"D:\Outils\KeePassXC.exe")), "KeePassXC");
    }
}
