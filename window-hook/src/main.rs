//! **INJECTEUR** — pose `neuradesk-windowhook.dll` sur un processus.
//!
//!     window-hook-inject <exe|pid> <chemin de la DLL>
//!     window-hook-inject stop
//!
//! `stop` ne fait qu'ÉCRIRE le fichier d'arrêt : c'est la DLL elle-même, depuis
//! son propre fil, qui défait ses sous-classes puis se décharge. Un déchargement
//! forcé de l'extérieur laisserait des fenêtres pointant sur du code libéré, et
//! l'application mourrait à la première souris.
//!
//! L'injection est la plus banale — `VirtualAllocEx` + `WriteProcessMemory` +
//! `CreateRemoteThread(LoadLibraryW)` —, la même que `shell-hook/src/main.rs`,
//! étendue à un processus choisi : c'est du code qu'il vaut mieux lire en entier
//! quand il vise une application de l'utilisateur.

use std::ffi::c_void;

use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE};
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject, PROCESS_ALL_ACCESS,
};

const ARRET: &str = r"C:\ndtarget\window-hook.stop";

/// `pid` d'un exécutable, par son nom (sans le `.exe`, insensible à la casse).
fn pid_par_nom(nom: &str) -> Option<u32> {
    let cible = nom.trim_end_matches(".exe").to_lowercase();
    unsafe {
        let instantane = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entree = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut trouve = None;
        if Process32FirstW(instantane, &mut entree).is_ok() {
            loop {
                let fin = entree.szExeFile.iter().position(|&c| c == 0).unwrap_or(entree.szExeFile.len());
                let exe = String::from_utf16_lossy(&entree.szExeFile[..fin]).trim_end_matches(".exe").to_lowercase();
                if exe == cible {
                    trouve = Some(entree.th32ProcessID);
                    break;
                }
                if Process32NextW(instantane, &mut entree).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(instantane);
        trouve
    }
}

fn injecter(pid: u32, chemin_dll: &str) -> Result<(), String> {
    let chemin = std::fs::canonicalize(chemin_dll).map_err(|e| format!("chemin illisible : {e}"))?;
    if !chemin.is_file() {
        return Err(format!("pas un fichier : {}", chemin.display()));
    }
    let chemin_large: Vec<u16> = chemin.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    let octets = chemin_large.len() * 2;

    unsafe {
        let processus = OpenProcess(PROCESS_ALL_ACCESS, false, pid).map_err(|e| format!("OpenProcess({pid}) : {e}"))?;
        let adresse = VirtualAllocEx(processus, None, octets, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if adresse.is_null() {
            let _ = CloseHandle(processus);
            return Err("VirtualAllocEx a échoué".into());
        }
        let mut ecrits = 0usize;
        let ecriture = WriteProcessMemory(processus, adresse, chemin_large.as_ptr() as *const c_void, octets, Some(&mut ecrits));

        let resultat = (|| -> Result<(), String> {
            if ecriture.is_err() || ecrits != octets {
                // Un processus 32 bits depuis un binaire 64 bits, ou une
                // protection qui refuse l'écriture.
                return Err(format!("WriteProcessMemory a écrit {ecrits}/{octets} octets"));
            }
            let noyau = GetModuleHandleW(windows::core::w!("kernel32.dll")).map_err(|e| format!("kernel32 : {e}"))?;
            let charge = GetProcAddress(noyau, windows::core::s!("LoadLibraryW"))
                .ok_or("LoadLibraryW introuvable dans kernel32")?;
            // `LoadLibraryW` est LE point d'entrée : il prend un `LPCWSTR`, ce
            // qui est exactement la signature d'un `LPTHREAD_START_ROUTINE`.
            let fil = CreateRemoteThread(
                processus,
                None,
                0,
                Some(std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "system" fn(*mut c_void) -> u32,
                >(charge)),
                Some(adresse),
                0,
                None,
            )
            .map_err(|e| format!("CreateRemoteThread : {e}"))?;
            let attente = WaitForSingleObject(fil, 5000);
            let mut code = 0u32;
            let _ = GetExitCodeThread(fil, &mut code);
            let _ = CloseHandle(fil);
            if attente != WAIT_OBJECT_0 {
                return Err("le fil distant n'a pas répondu en 5 s (LoadLibraryW bloqué ?)".into());
            }
            // `LoadLibraryW` rend le HMODULE : 0 = échec (mauvaise architecture,
            // DLL absente de la cible, ou chargeur qui refuse).
            if code == 0 {
                return Err("LoadLibraryW a rendu 0 dans le processus cible".into());
            }
            Ok(())
        })();

        let _ = VirtualFreeEx(processus, adresse, 0, MEM_RELEASE);
        let _ = CloseHandle(processus);
        resultat
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [ordre] if ordre == "stop" => {
            let _ = std::fs::create_dir_all(r"C:\ndtarget");
            match std::fs::write(ARRET, b"stop") {
                Ok(()) => println!("arrêt demandé : {ARRET}"),
                Err(e) => eprintln!("impossible d'écrire {ARRET} : {e}"),
            }
        }
        [cible, dll] => {
            let pid = match cible.parse::<u32>() {
                Ok(pid) => Some(pid),
                Err(_) => pid_par_nom(cible),
            };
            let Some(pid) = pid else {
                eprintln!("processus introuvable : {cible}");
                std::process::exit(2);
            };
            match injecter(pid, dll) {
                Ok(()) => println!("injecté dans {pid}"),
                Err(e) => {
                    eprintln!("échec : {e}");
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("usage : window-hook-inject <exe|pid> <chemin de la DLL> | stop");
            std::process::exit(2);
        }
    }
}
