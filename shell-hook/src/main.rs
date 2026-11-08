//! **INJECTEUR DE LA SONDE D'ÉTAPE 1** — voir `lib.rs` pour ce qu'elle fait.
//!
//!     shell-hook-inject inject <chemin de la DLL>
//!     shell-hook-inject stop
//!
//! `stop` ne fait qu'ÉCRIRE le fichier d'arrêt : c'est la DLL elle-même, depuis
//! son propre fil, qui défait ses sous-classes puis se décharge. Un déchargement
//! forcé de l'extérieur laisserait des fenêtres pointant sur du code libéré, et
//! explorer mourrait à la première souris.
//!
//! L'injection est la plus banale qui soit — `VirtualAllocEx` + `WriteProcessMemory`
//! + `CreateRemoteThread(LoadLibraryW)` — écrite ici avec le crate `windows`
//! plutôt que confiée à une bibliothèque : elle tient en cinquante lignes, et
//! c'est du code qu'il vaut mieux lire en entier quand il vise le shell.

use std::ffi::c_void;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject, PROCESS_ALL_ACCESS,
};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};

const CHEMIN_ARRET: &str = r"C:\ndtarget\shell-hook.stop";

/// Le pid d'explorer, obtenu par SA fenêtre de barre des tâches — la seule prise
/// dont on soit sûr, un `CreateToolhelp32Snapshot` pouvant en trouver plusieurs.
fn pid_du_shell() -> Option<u32> {
    let fenetre = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.ok()?;
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(fenetre, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

fn injecter(chemin_dll: &str) -> Result<(), String> {
    let pid = pid_du_shell().ok_or("pas de Shell_TrayWnd — explorer tourne-t-il ?")?;
    let chemin = std::fs::canonicalize(chemin_dll)
        .map_err(|erreur| format!("chemin illisible : {erreur}"))?;
    if !chemin.is_file() {
        return Err(format!("pas un fichier : {}", chemin.display()));
    }
    let chemin_large: Vec<u16> = chemin
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let octets = chemin_large.len() * 2;

    unsafe {
        let processus = OpenProcess(PROCESS_ALL_ACCESS, false, pid)
            .map_err(|erreur| format!("OpenProcess({pid}) : {erreur}"))?;

        let adresse = VirtualAllocEx(
            processus,
            None,
            octets,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if adresse.is_null() {
            let _ = CloseHandle(processus);
            return Err("VirtualAllocEx a échoué".to_string());
        }

        let mut ecrits = 0usize;
        let ecriture = WriteProcessMemory(
            processus,
            adresse,
            chemin_large.as_ptr() as *const c_void,
            octets,
            Some(&mut ecrits),
        );

        let resultat = (|| -> Result<(), String> {
            ecriture.map_err(|erreur| format!("WriteProcessMemory : {erreur}"))?;
            if ecrits != octets {
                return Err(format!("écriture partielle : {ecrits}/{octets} octets"));
            }

            // `LoadLibraryW` est LE point d'entrée : il prend un `LPCWSTR`, ce
            // qui est exactement la signature d'un `LPTHREAD_START_ROUTINE`.
            let noyau = GetModuleHandleW(w!("kernel32.dll"))
                .map_err(|erreur| format!("GetModuleHandleW(kernel32) : {erreur}"))?;
            let charger = GetProcAddress(noyau, windows::core::s!("LoadLibraryW"))
                .ok_or("LoadLibraryW introuvable")?;

            let fil = CreateRemoteThread(
                processus,
                None,
                0,
                Some(std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "system" fn(*mut c_void) -> u32,
                >(charger)),
                Some(adresse),
                0,
                None,
            )
            .map_err(|erreur| format!("CreateRemoteThread : {erreur}"))?;

            let attente = WaitForSingleObject(fil, 10_000);
            let mut code = 0u32;
            let _ = GetExitCodeThread(fil, &mut code);
            let _ = CloseHandle(fil);

            if attente != WAIT_OBJECT_0 {
                return Err("le fil distant n'a pas rendu la main en 10 s".to_string());
            }
            if code == 0 {
                // `LoadLibraryW` rend le HMODULE : zéro veut dire « refusée ».
                // L'antivirus est la première hypothèse, et c'est à le dire.
                return Err(
                    "LoadLibraryW a rendu 0 — la DLL a été REFUSÉE (antivirus, ou \
                     architecture incompatible)"
                        .to_string(),
                );
            }
            println!("DLL chargée dans explorer (pid {pid}), module à {code:#x}");
            Ok(())
        })();

        let _ = VirtualFreeEx(processus, adresse, 0, MEM_RELEASE);
        let _ = CloseHandle(processus);
        resultat
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let resultat = match arguments.get(1).map(String::as_str) {
        Some("inject") => match arguments.get(2) {
            Some(chemin) => injecter(chemin),
            None => Err("usage : shell-hook-inject inject <chemin de la DLL>".to_string()),
        },
        Some("stop") => std::fs::write(CHEMIN_ARRET, b"stop")
            .map(|()| println!("fichier d'arrêt écrit : la DLL va se défaire puis se décharger"))
            .map_err(|erreur| format!("écriture de {CHEMIN_ARRET} : {erreur}")),
        _ => Err(
            "usage :\n  shell-hook-inject inject <chemin de la DLL>\n  shell-hook-inject stop"
                .to_string(),
        ),
    };
    if let Err(erreur) = resultat {
        eprintln!("ECHEC : {erreur}");
        std::process::exit(1);
    }
}
