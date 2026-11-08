//! `neuradesk-admin.exe` — le RELAIS des onglets administrateur du panneau
//! Terminal (voir `win32::pty::ouvrir_admin`). DEMANDE EXPLICITE (2026-09-27) :
//! des shells administrateur DANS le panneau, « sans lancer neuradesk en
//! administrateur ». Un processus non élevé ne peut pas héberger la
//! pseudo-console d'un shell élevé : c'est ce petit programme, et lui seul,
//! que l'UAC élève. Pas de fenêtre, pas de CEF — il fait tourner UN shell
//! dans une pseudo-console et relaie ses octets par deux tubes nommés que
//! l'application a créés.
//!
//! ⚠️ **Ce qui le garde** (l'utilisateur a accepté le risque résiduel : un
//! programme qui s'injecterait dans NeuraDesk pourrait taper dans l'onglet) :
//! - il n'ouvre que des tubes `neuradesk-admin-…` et ne lance qu'un shell
//!   désigné par son IDENTIFIANT — jamais une commande reçue ;
//! - il vérifie que le serveur des deux tubes est bien le processus
//!   NeuraDesk qui l'a lancé (`GetNamedPipeServerProcessId`) ;
//! - il s'y connecte en `SECURITY_IDENTIFICATION` : le serveur, non élevé, ne
//!   peut PAS emprunter son jeton administrateur ;
//! - il meurt dès que l'application ferme l'onglet ou disparaît (tube
//!   cassé), et emporte son shell (Job `KILL_ON_JOB_CLOSE`).
//!
//! Ligne de commande : `<shell> <colonnes> <lignes> <nom de tube> <pid>`.

#![windows_subsystem = "windows"]

#[allow(dead_code)]
#[path = "win32/conpty.rs"]
mod conpty;

use std::sync::{Arc, Mutex};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_NONE, OPEN_EXISTING, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows::Win32::System::Console::ResizePseudoConsole;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows::Win32::System::Threading::{GetCurrentProcess, WaitForSingleObject, INFINITE};

use conpty::{Poignee, Pseudo, Shell};

fn main() {
    std::process::exit(if executer().is_ok() { 0 } else { 1 });
}

/// Un nom de tube que l'application a pu créer — rien d'autre n'est ouvert.
fn nom_de_tube_valide(nom: &str) -> bool {
    nom.len() < 100
        && nom.strip_prefix("neuradesk-admin-").is_some_and(|reste| {
            !reste.is_empty() && reste.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

fn executer() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [shell, colonnes, lignes, nom, pid] = arguments.as_slice() else {
        return Err("arguments".into());
    };
    let shell = Shell::depuis_id(shell).ok_or("shell")?;
    let colonnes: u32 = colonnes.parse().map_err(|_| "colonnes")?;
    let lignes: u32 = lignes.parse().map_err(|_| "lignes")?;
    let pid: u32 = pid.parse().map_err(|_| "pid")?;
    if !nom_de_tube_valide(nom) {
        return Err("nom de tube".into());
    }
    contenir_les_enfants();
    let entree = ouvrir_tube(nom, "entree", GENERIC_READ.0, pid)?;
    let sortie = ouvrir_tube(nom, "sortie", GENERIC_WRITE.0, pid)?;
    let (pseudo, vers_shell, depuis_shell, processus) =
        conpty::lancer(&shell.ligne_de_commande(), conpty::taille(colonnes, lignes))?;
    let pseudo = Arc::new(Mutex::new(Some(pseudo)));

    // Shell → application.
    let copie = std::thread::spawn(move || {
        // Les poignées ENTIÈRES (une fermeture 2021 ne capturerait que `.0`).
        let (depuis_shell, sortie) = (depuis_shell, sortie);
        let mut tampon = vec![0u8; 16 * 1024];
        while let Some(lus) = conpty::lire_une_fois(depuis_shell.0, &mut tampon) {
            if !conpty::ecrire_tout(sortie.0, &tampon[..lus]) {
                break;
            }
        }
    });
    // Application → shell. Tube cassé = onglet fermé ou application partie :
    // sortir emporte le shell (Job).
    {
        let pseudo = Arc::clone(&pseudo);
        std::thread::spawn(move || {
            recevoir(&entree, &vers_shell, &pseudo);
            std::process::exit(0);
        });
    }

    // SAFETY : poignée de processus valide.
    unsafe { WaitForSingleObject(processus.0, INFINITE) };
    // Le shell est fini : refermer la console casse `depuis_shell`, et la copie
    // se termine après avoir tout versé — l'application voit alors la fin.
    let console = pseudo.lock().unwrap_or_else(|e| e.into_inner()).take();
    drop(console);
    let _ = copie.join();
    Ok(())
}

fn recevoir(entree: &Poignee, vers_shell: &Poignee, pseudo: &Mutex<Option<Pseudo>>) {
    let mut decoupeur = conpty::Decoupeur::default();
    let mut tampon = vec![0u8; 16 * 1024];
    while let Some(lus) = conpty::lire_une_fois(entree.0, &mut tampon) {
        let Ok(trames) = decoupeur.pousser(&tampon[..lus]) else { return };
        for (genre, charge) in trames {
            match genre {
                conpty::TRAME_FRAPPES => {
                    if !conpty::ecrire_tout(vers_shell.0, &charge) {
                        return;
                    }
                }
                conpty::TRAME_TAILLE => {
                    if let (Some((colonnes, lignes)), Some(console)) =
                        (conpty::lire_taille(&charge), pseudo.lock().unwrap_or_else(|e| e.into_inner()).as_ref())
                    {
                        // SAFETY : pseudo-console vivante sous le verrou.
                        let _ = unsafe { ResizePseudoConsole(console.0, conpty::taille(colonnes, lignes)) };
                    }
                }
                _ => {}
            }
        }
    }
}

/// Ouvre `\\.\pipe\<nom>-<sens>` et vérifie que son serveur est `pid`.
fn ouvrir_tube(nom: &str, sens: &str, acces: u32, pid: u32) -> Result<Poignee, String> {
    let chemin: Vec<u16> = format!(r"\\.\pipe\{nom}-{sens}").encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY : chemin terminé par un zéro. `SECURITY_IDENTIFICATION` : le
    // serveur ne peut pas agir sous NOTRE identité (élevée).
    let poignee = unsafe {
        CreateFileW(
            PCWSTR(chemin.as_ptr()),
            acces,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            None,
        )
    }
    .map_err(|e| format!("tube {sens} : {e}"))?;
    let poignee = Poignee(poignee);
    let mut serveur = 0u32;
    // SAFETY : poignée de tube valide.
    unsafe { GetNamedPipeServerProcessId(poignee.0, &mut serveur) }.map_err(|e| format!("serveur {sens} : {e}"))?;
    if serveur != pid {
        return Err(format!("serveur {sens} inattendu"));
    }
    Ok(poignee)
}

/// Le shell (et tout ce qu'il lance) meurt avec le relais.
fn contenir_les_enfants() {
    // SAFETY : appels Win32 sur des structures locales valides ; en cas
    // d'échec, `ClosePseudoConsole` à la sortie reste le filet.
    unsafe {
        let Ok(job) = CreateJobObjectW(None, PCWSTR::null()) else { return };
        let mut limites = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limites.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let _ = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limites as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        let _ = AssignProcessToJobObject(job, GetCurrentProcess());
        // La poignée du Job reste ouverte jusqu'à la sortie : c'est sa
        // fermeture qui tue les enfants.
    }
}
