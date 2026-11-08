//! **OUVRIR LE GESTIONNAIRE DE FILE D'IMPRESSION DE WINDOWS.**
//!
//! DEMANDE EXPLICITE (2026-09-23) : « ajoute un applet pour ouvrir le gestionnaire de
//! file d'impression de windows ».
//!
//! ⚠️ **IL FAUT NOMMER L'IMPRIMANTE, ET ÇA S'EST APPRIS À LA MESURE.** `PrintUIEntry`
//! accepte plusieurs paramètres de base, et `/o` est celui qui AFFICHE LA FILE
//! (`/p` afficherait les PROPRIÉTÉS, ce qui n'est pas la même fenêtre — vérifié dans la
//! documentation Microsoft AVANT d'écrire, pas deviné).
//!
//! **MAIS `/o` SEUL ÉCHOUE** : première version lancée sans `/n`, et l'utilisateur a reçu
//! « Impossible de terminer cette opération (erreur 0x00000709) — Vérifiez que le nom de
//! l'imprimante est correct ». **`PrintUIEntry` ne retombe donc PAS sur l'imprimante par
//! défaut** — c'était mon hypothèse, elle est fausse. On la lui donne donc explicitement.
//!
//! **D'OÙ VIENT CE NOM** : de `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Windows`,
//! valeur `Device`, qui vaut `Nom,winspool,Ne01:` — c'est la forme historique, et elle a
//! été CONFRONTÉE à `Win32_Printer` sur cette machine avant d'être utilisée : les deux
//! sources donnent « Brother MFC-J480DW Printer ». On préfère cette lecture à
//! `GetDefaultPrinterW` parce qu'elle n'ajoute aucune dépendance (la feature
//! `Win32_Graphics_Printing` n'est pas activée dans `Cargo.toml`) et que
//! `registry_util::read_string_value` existe déjà.
//!
//! ⚠️ **SANS IMPRIMANTE PAR DÉFAUT, ON NE RENONCE PAS** : on ouvre le DOSSIER des
//! imprimantes (`shell:PrintersFolder`), qui les liste toutes. Une applet qui ne ferait
//! rien sur une machine sans imprimante par défaut serait un clic muet de plus.

use std::process::Command;

use windows::Win32::System::Registry::HKEY_CURRENT_USER;

/// Où Windows range l'imprimante par défaut.
const DEFAUT_CLE: &str = "Software\\Microsoft\\Windows NT\\CurrentVersion\\Windows";
const DEFAUT_VALEUR: &str = "Device";

/// **LE NOM SEUL, EXTRAIT DE LA VALEUR `Device`** — pure, donc testable.
///
/// `Device` vaut `Nom,winspool,Ne01:` : le nom est ce qui précède la PREMIÈRE virgule, et
/// il peut contenir des espaces (« Brother MFC-J480DW Printer »). Un nom vide ou une
/// valeur absente rendent `None` — on ne lance jamais un `/n ""`.
fn nom_depuis_device(brut: &str) -> Option<String> {
    let nom = brut.split(',').next()?.trim();
    if nom.is_empty() {
        None
    } else {
        Some(nom.to_string())
    }
}

/// L'imprimante par défaut de l'utilisateur, si Windows en connaît une.
pub fn default_printer() -> Option<String> {
    nom_depuis_device(&super::registry_util::read_string_value(HKEY_CURRENT_USER, DEFAUT_CLE, DEFAUT_VALEUR)?)
}

/// Ouvre la file d'impression de l'imprimante par défaut — ou, à défaut, la liste des
/// imprimantes.
///
/// Best-effort assumé, comme les autres lanceurs de l'application : un échec de `spawn`
/// (jamais observé pour un exécutable système) ne remonte pas — l'applet n'a rien à
/// afficher. Le journal dit en revanche QUELLE branche a été prise, sans quoi un clic sans
/// effet ne se distinguerait pas d'un clic qui n'a rien trouvé.
pub fn open_queue() {
    match default_printer() {
        Some(nom) => {
            crate::diag!("file d'impression : ouverture de la file de « {nom} »");
            // Arguments SÉPARÉS, jamais une seule chaîne : Windows ne doit pas
            // réinterpréter `printui.dll,PrintUIEntry`. Le nom, lui, contient des espaces
            // et `Command` le met entre guillemets tout seul.
            let _ = Command::new("rundll32.exe")
                .arg("printui.dll,PrintUIEntry")
                .arg("/o")
                .arg("/n")
                .arg(&nom)
                .spawn();
        }
        None => {
            crate::diag!("file d'impression : AUCUNE imprimante par defaut — repli sur la liste des imprimantes");
            let _ = Command::new("explorer.exe").arg("shell:PrintersFolder").spawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **LE NOM SEUL, ET IL PEUT CONTENIR DES ESPACES.** C'est la seule partie de ce
    /// module qui peut se tromper : le reste n'est qu'un lancement de commande.
    #[test]
    fn le_nom_se_prend_avant_la_premiere_virgule() {
        assert_eq!(nom_depuis_device("Brother MFC-J480DW Printer,winspool,Ne01:").as_deref(), Some("Brother MFC-J480DW Printer"));
        assert_eq!(nom_depuis_device("Microsoft Print to PDF,winspool,Ne02:").as_deref(), Some("Microsoft Print to PDF"));
        // Un nom SANS virgule reste un nom — la valeur n'est pas toujours suffixée.
        assert_eq!(nom_depuis_device("Imprimante").as_deref(), Some("Imprimante"));
        // ⚠️ ET LE CAS QUI COMPTE : une valeur vide ne doit PAS produire `"/n \"\""`,
        // qui redonnerait exactement l'erreur 0x709 signalée par l'utilisateur.
        assert_eq!(nom_depuis_device(""), None);
        assert_eq!(nom_depuis_device(",winspool,Ne01:"), None);
        assert_eq!(nom_depuis_device("   ,winspool,Ne01:"), None);
    }
}
