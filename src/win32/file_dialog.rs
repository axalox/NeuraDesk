//! **LA FENÊTRE « ENREGISTRER SOUS » DE WINDOWS** — le premier dialogue de
//! fichier de cette application.
//!
//! DEMANDE EXPLICITE (2026-09-24) : « une vraie fenêtre Enregistrer sous », pour
//! exporter une note. Aucun composant de ce genre n'existait : tout le reste de
//! l'interface passe par des panneaux CEF ou des menus natifs (`win32::menu`), et
//! un choix de FICHIER ne se fait ni dans une page — elle n'a aucun accès au
//! système de fichiers, c'est la règle de toutes les pages de ce dépôt — ni dans
//! un menu, qui ne sait pas demander un nom.
//!
//! ⚠️ **`IFileSaveDialog`, PAS `GetSaveFileNameW`.** L'API historique (commdlg)
//! fonctionne encore, mais elle repose sur un état GLOBAL partagé et sur une
//! structure dont la taille dépend de la version de Windows — c'est la voie que
//! Microsoft documente comme obsolète. Le dialogue moderne est un objet COM, et
//! **l'appartement de ce thread est déjà STA** (`main.rs` l'initialise pour
//! l'énumération des périphériques audio) : c'est exactement ce que ce dialogue
//! demande, donc rien à initialiser de plus ici.
//!
//! ⚠️ **CE DIALOGUE EST MODAL, DONC LA BARRE NE SE REDESSINE PAS PENDANT QU'IL EST
//! OUVERT.** C'est le comportement normal d'une boîte de dialogue Windows, et
//! c'est assumé : le geste est court, explicite, et l'utilisateur a le clavier et
//! la souris. Une version non bloquante demanderait un fil et un rappel — pour un
//! export de note, ce serait payer cher une gêne qui n'en est pas une.

use std::path::PathBuf;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, FileSaveDialog, IFileOpenDialog, IFileSaveDialog, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM,
    FOS_OVERWRITEPROMPT, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
};

/// Une chaîne Rust en chaîne large terminée par zéro — ce que l'API attend.
fn large(texte: &str) -> Vec<u16> {
    texte.encode_utf16().chain(std::iter::once(0)).collect()
}

/// **DEMANDE UN CHEMIN D'ENREGISTREMENT.** Rend `None` quand l'utilisateur
/// annule — ce qui n'est **pas** une erreur, et ne doit donc rien journaliser :
/// ouvrir une boîte et se raviser est un geste normal.
///
/// `proprietaire` : la fenêtre au-dessus de laquelle la boîte s'ouvre. Elle doit
/// être celle de la barre — une boîte sans propriétaire peut passer DERRIÈRE, et
/// l'application paraîtrait figée alors qu'elle attend un choix invisible.
pub fn enregistrer_sous(
    proprietaire: HWND,
    titre: &str,
    nom_propose: &str,
    extension: &str,
    libelle_filtre: &str,
) -> Option<PathBuf> {
    let titre_w = large(titre);
    let nom_w = large(nom_propose);
    let extension_w = large(extension);
    let libelle_w = large(libelle_filtre);
    let motif_w = large(&format!("*.{extension}"));
    // ⚠️ Les trois chaînes ci-dessous doivent VIVRE jusqu'à la fin de l'appel :
    // `COMDLG_FILTERSPEC` ne porte que des pointeurs. Les garder dans des `Vec`
    // liés ici est ce qui l'assure — les construire dans l'appel les ferait
    // tomber avant que Windows ne les lise.
    let types = [COMDLG_FILTERSPEC {
        pszName: PCWSTR(libelle_w.as_ptr()),
        pszSpec: PCWSTR(motif_w.as_ptr()),
    }];
    unsafe {
        let dialogue: IFileSaveDialog =
            CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let _ = dialogue.SetTitle(PCWSTR(titre_w.as_ptr()));
        let _ = dialogue.SetFileName(PCWSTR(nom_w.as_ptr()));
        let _ = dialogue.SetDefaultExtension(PCWSTR(extension_w.as_ptr()));
        let _ = dialogue.SetFileTypes(&types);
        // `FOS_FORCEFILESYSTEM` : on veut un vrai chemin, pas un identifiant
        // virtuel (bibliothèque, « Ce PC ») — l'export écrit un fichier.
        // `FOS_OVERWRITEPROMPT` : Windows demande confirmation avant d'écraser,
        // plutôt que de le faire parce qu'on a cliqué trop vite.
        let _ = dialogue.SetOptions(FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT);
        // ⚠️ **L'ANNULATION PASSE PAR ICI**, et c'est le cas le plus fréquent :
        // `Show` rend `HRESULT_FROM_WIN32(ERROR_CANCELLED)`, que `ok()` transforme
        // en `None`. Aucun journal, donc — se raviser n'est pas un échec.
        dialogue.Show(Some(proprietaire)).ok()?;
        let item = dialogue.GetResult().ok()?;
        let brut: PWSTR = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let chemin = brut.to_string().ok();
        // La chaîne rendue par `GetDisplayName` est allouée par COM : c'est à
        // l'appelant de la libérer.
        CoTaskMemFree(Some(brut.0 as *const core::ffi::c_void));
        Some(PathBuf::from(chemin?))
    }
}

/// **DEMANDE UN FICHIER À OUVRIR** — le pendant de `enregistrer_sous`, et même
/// structure. Rend `None` quand l'utilisateur annule, ce qui n'est pas une erreur.
///
/// ⚠️ **`FOS_FILEMUSTEXIST`** : on importe un fichier qui existe, donc le dialogue
/// le vérifie lui-même — taper un nom qui n'existe pas est refusé par Windows,
/// avec son propre message, plutôt que par nous après coup.
///
/// `extensions` : une seule (`"md"`) ou plusieurs séparées par `;`
/// (`"png;jpg"`) — voir [`motif_du_filtre`].
pub fn ouvrir(
    proprietaire: HWND,
    titre: &str,
    extensions: &str,
    libelle_filtre: &str,
) -> Option<PathBuf> {
    let titre_w = large(titre);
    let libelle_w = large(libelle_filtre);
    let motif_w = large(&motif_du_filtre(extensions));
    let types = [COMDLG_FILTERSPEC {
        pszName: PCWSTR(libelle_w.as_ptr()),
        pszSpec: PCWSTR(motif_w.as_ptr()),
    }];
    unsafe {
        let dialogue: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let _ = dialogue.SetTitle(PCWSTR(titre_w.as_ptr()));
        let _ = dialogue.SetFileTypes(&types);
        let _ = dialogue.SetOptions(FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST);
        dialogue.Show(Some(proprietaire)).ok()?;
        let item = dialogue.GetResult().ok()?;
        let brut: PWSTR = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let chemin = brut.to_string().ok();
        CoTaskMemFree(Some(brut.0 as *const core::ffi::c_void));
        Some(PathBuf::from(chemin?))
    }
}

/// **DEMANDE UN DOSSIER** — la seule voie du dépôt pour en désigner un, et donc
/// celle des raccourcis de la barre du bas (voir `config::DossierEpingle`).
///
/// ⚠️ **`ouvrir` NE SAIT PAS FAIRE CECI** : elle filtre par EXTENSIONS
/// (`SetFileTypes`) et exige `FOS_FILEMUSTEXIST`, ce qui désigne un fichier. Un
/// dossier se demande avec **`FOS_PICKFOLDERS`**, qui change la nature même de la
/// boîte — d'où une fonction distincte plutôt qu'un drapeau de plus sur
/// `ouvrir`, dont les six appelants n'ont jamais eu à s'en soucier.
///
/// On ne pose AUCUN `SetFileTypes` : combiné à `FOS_PICKFOLDERS`, il fait
/// disparaître les dossiers de la liste sur certaines versions de Windows.
///
/// Rend `None` quand l'utilisateur annule — même règle que `ouvrir`.
pub fn choisir_dossier(proprietaire: HWND, titre: &str) -> Option<PathBuf> {
    let titre_w = large(titre);
    unsafe {
        let dialogue: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let _ = dialogue.SetTitle(PCWSTR(titre_w.as_ptr()));
        let _ = dialogue.SetOptions(FOS_FORCEFILESYSTEM | FOS_PICKFOLDERS);
        dialogue.Show(Some(proprietaire)).ok()?;
        let item = dialogue.GetResult().ok()?;
        let brut: PWSTR = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let chemin = brut.to_string().ok();
        CoTaskMemFree(Some(brut.0 as *const core::ffi::c_void));
        Some(PathBuf::from(chemin?))
    }
}

/// Le motif d'un filtre de dialogue : `"png;jpg"` → `"*.png;*.jpg"` — la forme
/// que `COMDLG_FILTERSPEC::pszSpec` attend pour plusieurs extensions.
fn motif_du_filtre(extensions: &str) -> String {
    extensions.split(';').map(|e| format!("*.{e}")).collect::<Vec<_>>().join(";")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_ou_plusieurs_extensions_font_un_motif_de_filtre() {
        assert_eq!(motif_du_filtre("md"), "*.md");
        assert_eq!(motif_du_filtre("png;jpg;jpeg"), "*.png;*.jpg;*.jpeg");
    }
}
