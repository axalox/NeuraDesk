//! **LE DÉCODEUR ISOLÉ** des fichiers à calques (PSD, XCF) que l'éditeur
//! d'image ouvre.
//!
//! ⛔ **POURQUOI UN AUTRE PROCESSUS** : le dock est compilé en `panic = "abort"`
//! (voir `Cargo.toml`), et un fichier ouvert par l'utilisateur est une donnée
//! NON FIABLE — `ag-psd` n'a jamais promis de ne pas paniquer sur un fichier
//! corrompu. Décodé dans le dock, un tel fichier TUERAIT le dock. Ici, c'est le
//! même exécutable relancé avec [`DRAPEAU`] qui décode : s'il meurt, seul lui
//! meurt, et l'éditeur dit « fichier illisible ». Bonus : un gros fichier ne
//! gèle pas l'interface pendant le décodage.
//!
//! Le sous-processus écrit le JSON de la page (`document_image::vers_page`) — ou
//! `{"erreur": …}` — dans un fichier temporaire, puis sort. Le dock le lance
//! ([`lancer`]) et le relève à chaque frame ([`EnCours::relever`]) : l'objet job
//! de `cef_app::contain_child_processes` l'emporte si le dock s'arrête avant.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use serde_json::{json, Value};

use super::document_image::Document;

/// Le premier argument qui fait de l'exécutable un décodeur.
pub const DRAPEAU: &str = "--decoder-document";

/// Pas de fenêtre de console pour le sous-processus.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Les extensions confiées au décodeur.
pub fn est_un_document(chemin: &Path) -> bool {
    chemin.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("psd") || e.eq_ignore_ascii_case("xcf"))
}

/// Décode `octets`, d'après leur signature et non l'extension.
pub fn decoder(octets: &[u8]) -> Result<Document, String> {
    if octets.starts_with(b"8BPS") {
        super::format_psd::lire(octets)
    } else if octets.starts_with(b"gimp xcf ") {
        super::format_xcf::lire(octets)
    } else {
        Err("ni un fichier Photoshop, ni un fichier GIMP".into())
    }
}

/// À appeler EN TÊTE de `main()` : si le processus a été lancé en décodeur, fait
/// le travail et SORT, sans jamais revenir.
pub fn executer_si_demande() {
    let arguments: Vec<_> = std::env::args_os().collect();
    if arguments.get(1).is_none_or(|a| a != DRAPEAU) {
        return;
    }
    let (Some(entree), Some(sortie)) = (arguments.get(2), arguments.get(3)) else {
        std::process::exit(2);
    };
    let resultat = std::fs::read(entree)
        .map_err(|e| format!("fichier illisible : {e}"))
        .and_then(|octets| decoder(&octets))
        .and_then(|document| super::document_image::vers_page(&document));
    let json = resultat.unwrap_or_else(|erreur| json!({ "erreur": erreur }));
    let ecrit = std::fs::write(sortie, json.to_string()).is_ok();
    std::process::exit(if ecrit { 0 } else { 1 });
}

/// Un décodage lancé, pas encore relevé.
pub struct EnCours {
    enfant: Child,
    sortie: PathBuf,
}

/// Lance le décodage de `chemin` dans un sous-processus.
pub fn lancer(chemin: &Path) -> Result<EnCours, String> {
    use std::os::windows::process::CommandExt;
    let exe = std::env::current_exe().map_err(|e| format!("exécutable introuvable : {e}"))?;
    let sortie = std::env::temp_dir().join(format!("neuradesk-document-{}.json", std::process::id()));
    // Un reste d'un décodage précédent ne doit pas passer pour le résultat.
    let _ = std::fs::remove_file(&sortie);
    let enfant = Command::new(exe)
        .arg(DRAPEAU)
        .arg(chemin)
        .arg(&sortie)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("décodeur non lancé : {e}"))?;
    Ok(EnCours { enfant, sortie })
}

impl EnCours {
    /// `None` tant que le sous-processus travaille ; sinon le JSON de la page, ou
    /// l'erreur à montrer.
    pub fn relever(&mut self) -> Option<Result<Value, String>> {
        let statut = match self.enfant.try_wait() {
            Ok(None) => return None,
            Ok(Some(statut)) => statut,
            Err(e) => return Some(Err(format!("décodeur perdu : {e}"))),
        };
        let lu = std::fs::read(&self.sortie);
        let _ = std::fs::remove_file(&self.sortie);
        Some(match lu {
            // Pas de fichier : le décodeur est mort en route (une panique).
            Err(_) => Err(format!("fichier illisible (le décodeur s'est arrêté : {statut})")),
            Ok(octets) => match serde_json::from_slice::<Value>(&octets) {
                Ok(v) => match v["erreur"].as_str() {
                    Some(erreur) => Err(erreur.to_string()),
                    None => Ok(v),
                },
                Err(e) => Err(format!("réponse du décodeur illisible : {e}")),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::win32::document_image::tests::exemple;

    #[test]
    fn la_signature_choisit_le_format() {
        let xcf = crate::win32::format_xcf::ecrire(&exemple()).unwrap();
        assert_eq!(decoder(&xcf).unwrap(), exemple());
        assert!(decoder(b"\x89PNG....").is_err());
        assert!(est_un_document(Path::new("C:/a/b.PSD")));
        assert!(est_un_document(Path::new("b.xcf")));
        assert!(!est_un_document(Path::new("b.png")));
    }
}
