//! **DÉMARRER ET ARRÊTER LE NARRATEUR DE WINDOWS** (`Narrator.exe`).
//!
//! DEMANDE EXPLICITE (2026-10-15) : « dans la zone réglages de la barre de droite
//! une applet d'accès rapide au narrateur ». Un clic BASCULE : il démarre le
//! Narrateur s'il est arrêté, il l'arrête s'il tourne — le geste de Ctrl + Win +
//! Entrée, sans clavier.
//!
//! ## ⛔ LE DÉMARRAGE PASSE PAR `ShellExecuteW`
//!
//! Le manifeste EMBARQUÉ de `C:\Windows\System32\Narrator.exe` (relu sur la
//! machine) déclare `level="asInvoker" uiAccess="true"`. Un exécutable `uiAccess`
//! se lance par le service AppInfo — donc par le SHELL : `CreateProcess` (ce
//! qu'appelle `Command::spawn`) échoue en `ERROR_ELEVATION_REQUIRED` (740), sans
//! fenêtre (voir `win32::gestionnaire_peripheriques`, même piège).
//!
//! ## ⛔ L'ARRÊT PASSE PAR `TerminateProcess` — MESURÉ, PAS CHOISI PAR DÉFAUT
//!
//! Essayé le 2026-10-15 sur la machine, dans cet ordre :
//! - `WM_CLOSE` posté à sa fenêtre `NarratorUIClass` : `PostMessageW` rend 0 —
//!   l'isolation des privilèges d'interface (UIPI) protège un processus
//!   `uiAccess` des messages d'un processus ordinaire ;
//! - `taskkill /PID` sans `/F` (le même `WM_CLOSE`) : « Accès refusé » ;
//! - relancer `Narrator.exe` : il ne s'arrête PAS (12 s de relevés) ;
//! - `OpenProcess(PROCESS_TERMINATE)` + `TerminateProcess` : arrêté, sans reste.
//!
//! C'est aussi le moyen de Windows : `atbroker.exe`, qui lance et arrête les
//! outils d'accessibilité, importe `TerminateProcess` pour les arrêter. Le
//! Narrateur ne tient aucune donnée de l'utilisateur à enregistrer.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Le nom du processus, tel que Toolhelp le donne (comparé sans casse).
const EXECUTABLE: &str = "narrator.exe";

/// Ce que fait le clic.
#[derive(Debug, PartialEq)]
enum Geste {
    Demarrer,
    /// Arrêter ces processus (un seul en pratique).
    Arreter(Vec<u32>),
}

/// PURE : le geste du clic selon les Narrateurs qui tournent.
fn geste(en_cours: Vec<u32>) -> Geste {
    if en_cours.is_empty() {
        Geste::Demarrer
    } else {
        Geste::Arreter(en_cours)
    }
}

/// PURE : le PID des processus nommés `EXECUTABLE`, parmi `(nom, pid)`.
fn pids_du_narrateur<'a>(processus: impl Iterator<Item = (&'a str, u32)>) -> Vec<u32> {
    processus.filter(|(nom, _)| nom.eq_ignore_ascii_case(EXECUTABLE)).map(|(_, pid)| pid).collect()
}

/// Les processus en cours, `(nom de l'exécutable, PID)` — un instantané Toolhelp.
fn processus() -> Vec<(String, u32)> {
    let mut liste = Vec::new();
    // SAFETY : l'instantané est fermé en fin de fonction ; `entree` porte sa taille.
    unsafe {
        let Ok(instantane) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return liste };
        let mut entree = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut suite = Process32FirstW(instantane, &mut entree).is_ok();
        while suite {
            let longueur = entree.szExeFile.iter().position(|&c| c == 0).unwrap_or(entree.szExeFile.len());
            liste.push((String::from_utf16_lossy(&entree.szExeFile[..longueur]), entree.th32ProcessID));
            suite = Process32NextW(instantane, &mut entree).is_ok();
        }
        let _ = CloseHandle(instantane);
    }
    liste
}

/// Démarre le Narrateur s'il est arrêté, l'arrête s'il tourne.
///
/// Best-effort assumé, comme les autres applets ACTION : un échec ne remonte
/// pas, mais il est JOURNALISÉ — sans quoi un clic sans effet serait muet.
pub fn basculer() {
    let processus = processus();
    match geste(pids_du_narrateur(processus.iter().map(|(nom, pid)| (nom.as_str(), *pid)))) {
        Geste::Demarrer => {
            crate::diag!("narrateur : démarrage");
            // SAFETY : chaînes littérales terminées par zéro (`w!`), aucune fenêtre parente.
            let resultat = unsafe {
                ShellExecuteW(None, w!("open"), w!("Narrator.exe"), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL)
            };
            // `ShellExecuteW` rend une valeur > 32 en cas de succès, un code d'erreur sinon.
            if resultat.0 as isize <= 32 {
                crate::diag!("narrateur : lancement impossible — code {}", resultat.0 as isize);
            }
        }
        Geste::Arreter(pids) => {
            for pid in pids {
                crate::diag!("narrateur : arrêt du processus {pid}");
                // SAFETY : la poignée ouverte est fermée juste après usage.
                unsafe {
                    match OpenProcess(PROCESS_TERMINATE, false, pid) {
                        Ok(poignee) => {
                            if let Err(err) = TerminateProcess(poignee, 0) {
                                crate::diag!("narrateur : arrêt impossible — {err}");
                            }
                            let _ = CloseHandle(poignee);
                        }
                        Err(err) => crate::diag!("narrateur : processus {pid} inaccessible — {err}"),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Arrêté : le clic le démarre ; en cours : il l'arrête — tous ses processus,
    /// et eux seuls, quelle que soit la casse du nom.
    #[test]
    fn le_clic_demarre_ou_arrete_le_narrateur() {
        let rien = [("explorer.exe", 10), ("neuradesk.exe", 11)];
        assert_eq!(geste(pids_du_narrateur(rien.into_iter())), Geste::Demarrer);
        let en_cours = [("explorer.exe", 10), ("Narrator.exe", 42), ("NarratorHelper.exe", 43)];
        assert_eq!(geste(pids_du_narrateur(en_cours.into_iter())), Geste::Arreter(vec![42]));
        let deux = [("narrator.EXE", 7), ("Narrator.exe", 8)];
        assert_eq!(geste(pids_du_narrateur(deux.into_iter())), Geste::Arreter(vec![7, 8]));
    }
}
