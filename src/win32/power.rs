//! Actions du menu d'alimentation (bouton de la barre du haut) :
//! verrouiller, suspendre, redémarrer, éteindre, se déconnecter — les
//! mêmes options que le menu système de GNOME. Comme GNOME, verrouiller et
//! suspendre restent immédiats ; redémarrer/éteindre/se déconnecter
//! demandent confirmation (`MessageBoxW` natif) avant d'agir, ces actions
//! étant destructrices (perte du travail non enregistré dans les autres
//! applications).

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::{
    ExitWindowsEx, LockWorkStation, EWX_LOGOFF, EWX_POWEROFF, EWX_REBOOT, EWX_SHUTDOWN, SHUTDOWN_REASON,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONWARNING, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
};

/// Action choisie dans le menu d'alimentation (voir `win32::menu`).
#[derive(Debug, Clone, Copy)]
pub enum PowerAction {
    Lock,
    Suspend,
    Restart,
    ShutDown,
    LogOff,
}

/// Exécute `action`. Demande confirmation d'abord pour les actions qui
/// interrompent la session.
pub fn execute(hwnd: HWND, action: PowerAction) {
    let confirmation_message: Option<PCWSTR> = match action {
        PowerAction::Lock | PowerAction::Suspend => None,
        PowerAction::Restart => Some(w!("Redémarrer l'ordinateur maintenant ?")),
        PowerAction::ShutDown => Some(w!("Éteindre l'ordinateur maintenant ?")),
        PowerAction::LogOff => Some(w!("Fermer la session maintenant ?")),
    };

    if let Some(message) = confirmation_message {
        if !confirm(hwnd, message) {
            return;
        }
    }

    unsafe {
        match action {
            PowerAction::Lock => {
                let _ = LockWorkStation();
            }
            PowerAction::Suspend => {
                let _ = SetSuspendState(false, false, false);
            }
            PowerAction::Restart => {
                if enable_shutdown_privilege() {
                    let _ = ExitWindowsEx(EWX_REBOOT, SHUTDOWN_REASON(0));
                }
            }
            PowerAction::ShutDown => {
                if enable_shutdown_privilege() {
                    let _ = ExitWindowsEx(EWX_SHUTDOWN | EWX_POWEROFF, SHUTDOWN_REASON(0));
                }
            }
            PowerAction::LogOff => {
                // Contrairement à Redémarrer/Éteindre, se déconnecter de sa
                // propre session ne nécessite pas SeShutdownPrivilege.
                let _ = ExitWindowsEx(EWX_LOGOFF, SHUTDOWN_REASON(0));
            }
        }
    }
}

/// **VERROUILLER EN DEMANDANT CONFIRMATION — le geste de l'APPLET de barre, pas
/// celui du menu Alimentation.**
///
/// ⛔ **C'EST UNE ASYMÉTRIE ASSUMÉE, ET ELLE EST DATÉE.** Décision de l'utilisateur
/// (2026-10-05) : le menu Alimentation garde son verrouillage IMMÉDIAT (voir
/// `execute`, où `Lock` ne demande rien), et l'icône de la barre demande. Deux
/// comportements pour le même geste, donc — ce que `app.rs` déconseille ailleurs
/// (« un geste ne peut plus avoir deux comportements selon le chemin qui
/// l'exécute »). **Ne pas « corriger » cela sans lui demander** : c'est un choix,
/// pas un oubli.
///
/// Le corps ne fait que RÉUTILISER `confirm` et `LockWorkStation` : une seconde
/// copie de l'appel système serait une seconde source de vérité pour
/// « verrouiller », et les deux divergeraient au premier changement.
///
/// Rend `true` si la session a été verrouillée (c'est-à-dire si l'utilisateur a
/// confirmé).
pub fn verrouiller_avec_confirmation(hwnd: HWND) -> bool {
    if !confirm(hwnd, w!("Verrouiller la session maintenant ?")) {
        return false;
    }
    unsafe {
        let _ = LockWorkStation();
    }
    true
}

fn confirm(hwnd: HWND, message: PCWSTR) -> bool {
    unsafe {
        MessageBoxW(
            Some(hwnd),
            message,
            w!("NeuraDesk"),
            MB_YESNO | MB_ICONWARNING | MB_TOPMOST | MB_SETFOREGROUND,
        ) == IDYES
    }
}

/// Active le privilège `SeShutdownPrivilege` sur le process courant,
/// nécessaire à `ExitWindowsEx` pour redémarrer/éteindre. Retourne `false`
/// si l'obtention échoue (l'appelant renonce alors à l'action plutôt que
/// de tenter un `ExitWindowsEx` voué à l'échec).
fn enable_shutdown_privilege() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token).is_err() {
            return false;
        }

        let mut luid = LUID::default();
        let luid_ok = LookupPrivilegeValueW(None, w!("SeShutdownPrivilege"), &mut luid).is_ok();
        let adjust_ok = luid_ok
            && AdjustTokenPrivileges(
                token,
                false,
                Some(&TOKEN_PRIVILEGES {
                    PrivilegeCount: 1,
                    Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
                }),
                0,
                None,
                None,
            )
            .is_ok();

        let _ = CloseHandle(token);
        adjust_ok
    }
}
