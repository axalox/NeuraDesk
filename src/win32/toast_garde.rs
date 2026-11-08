//! **LA RÉPARATION DES POPUPS — ce qui reste de l'ancienne suspension.**
//!
//! # Pourquoi ce module existe encore
//!
//! Jusqu'au 2026-11-02, NeuraDesk coupait les popups du shell
//! (`PushNotifications\ToastEnabled = 0`) pour réafficher les toasts dans son
//! bandeau, avec un témoin sur disque et un processus « garde » pour les rendre.
//! ⛔ **MESURÉ CE JOUR-LÀ : la coupure, une fois prise en compte, faisait JETER
//! les toasts par Windows** (ni bannière, ni ligne dans `wpndatabase`). La voie
//! est abandonnée : `win32::toast_natif` déplace le vrai toast, et plus rien ne
//! touche à cette valeur.
//!
//! Mais une ancienne version a pu la LAISSER à `0` — c'est le cas de la machine
//! de développement, témoin daté de la veille. Une machine dans cet état ne
//! reçoit plus AUCUNE notification, et rien ne le dit. D'où la réparation au
//! lancement, gardée seule.
//!
//! # ⛔ LE TÉMOIN EST LA SEULE PREUVE QUE C'EST NOUS
//!
//! `ToastEnabled = 0` est AUSSI ce que pose l'interrupteur « Notifications » des
//! Paramètres de Windows. Sans le témoin, une valeur à `0` est un CHOIX de
//! l'utilisateur, et on n'y touche pas.
//!
//! # ⛔ ÉCRIRE NE SUFFIT PAS : IL FAUT REDÉMARRER LE SERVICE
//!
//! Mesuré : la valeur n'est relue qu'au démarrage de `WpnUserService_<suffixe>`
//! (le service de notifications de la session). Retirer la valeur sans le
//! redémarrer laisserait les popups coupées jusqu'à la prochaine ouverture de
//! session. Le redémarrage se fait sans droits d'administrateur (service de
//! l'utilisateur), et ne perd pas l'historique (base sur disque).

use std::path::PathBuf;
use std::process::Command;

use windows::Win32::System::Registry::HKEY_CURRENT_USER;

/// Le drapeau de l'ancien processus garde. Plus jamais lancé ; reconnu en tête
/// de `main()` pour qu'un garde d'une ANCIENNE version, encore vivant au moment
/// d'une mise à jour, ne puisse pas devenir un second dock s'il est relancé.
pub const DRAPEAU: &str = "--garde-toast";

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const CLE_PUSH: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\PushNotifications";
const VALEUR: &str = "ToastEnabled";

/// Le témoin posé par l'ancienne suspension — à côté de la configuration.
fn temoin() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("NeuraDesk").join("toast_suspendus.txt"))
}

/// **SORT TOUT DE SUITE SI CE PROCESSUS EST UN ANCIEN GARDE** — à appeler en tête
/// de `main()`. Rien à surveiller : la réparation est faite par le dock.
pub fn sortir_si_ancien_garde() {
    if std::env::args_os().nth(1).is_some_and(|argument| argument == DRAPEAU) {
        std::process::exit(0);
    }
}

/// **LA RÉPARATION AU LANCEMENT** — sans condition de réglage : elle concerne
/// l'état laissé par une version précédente.
pub fn reparer_apres_crash() {
    let Some(chemin) = temoin() else { return };
    if !chemin.exists() {
        return;
    }
    crate::diag!("[toast] temoin d'une ancienne suspension trouve — les popups sont rendues");
    let rendu = crate::win32::registry_util::delete_value(HKEY_CURRENT_USER, CLE_PUSH, VALEUR);
    let _ = std::fs::remove_file(&chemin);
    if rendu {
        redemarrer_le_service();
    }
}

/// **REDÉMARRE LE SERVICE DE NOTIFICATIONS DE LA SESSION**, sans attendre.
///
/// Son nom porte un suffixe propre à la session (`WpnUserService_28bb08`) ; le
/// modèle sans suffixe est ARRÊTÉ par construction, d'où le filtre `Running`.
fn redemarrer_le_service() {
    use std::os::windows::process::CommandExt;
    // ⚠️ **LE CHEMIN ABSOLU** : un `powershell.exe` nu se résout par le
    // dossier courant et le PATH — un fichier homonyme posé à côté de l'exe
    // portable serait lancé à sa place.
    let racine = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let powershell = PathBuf::from(racine).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe");
    let lance = Command::new(powershell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-Service -Name 'WpnUserService_*' | Where-Object Status -eq 'Running' | Restart-Service -Force",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    match lance {
        Ok(_) => crate::diag!("[toast] redemarrage du service de notifications demande"),
        Err(erreur) => crate::diag!("[toast] redemarrage du service impossible ({erreur}) — popups rendues a la prochaine session"),
    }
}
