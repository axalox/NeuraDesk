//! ⭐ **INJECTION AUTOMATIQUE DU MODULE `window-hook`.**
//!
//! DEMANDE EXPLICITE (2026-11-04) : les effets de fermeture ne jouent que dans
//! une application où `window-hook` est chargé — jusqu'ici, à la main. Ce module
//! charge la DLL dans les applications qui ouvrent une fenêtre animable, UNE
//! fois par processus, et **jamais depuis le fil du crochet** (voir
//! `ouverture_fenetres::rappel`, qui ne fait que déposer le pid).
//!
//! ⛔ **DÉSACTIVÉ PAR DÉFAUT** (`effet_injection`) : injecter un module dans les
//! applications de l'utilisateur n'est pas une décision qu'un réglage implicite
//! prend à sa place. La case des Réglages le dit noir sur blanc.
//!
//! ⛔ **LES PROCESSUS DU SYSTÈME ET L'EXPLORATEUR SONT EXCLUS** — une panne du
//! module dans `explorer.exe` emporterait le bureau. La liste est celle des
//! hôtes de l'interface Windows ; tout ce qui n'y est pas et qui possède une
//! fenêtre animable est éligible.
//!
//! ⛔ **UN ÉCHEC EST DÉFINITIF POUR CE PID — PENDANT DIX MINUTES** : un
//! processus 32 bits refusera toujours une DLL 64 bits, un processus protégé
//! toujours `OpenProcess`. On note le pid AVANT d'essayer (voir `proposer`) :
//! réessayer à chaque fenêtre ouvrirait une boucle de tentatives. L'oubli au
//! bout de `OUBLI` n'est pas un démenti — c'est Windows qui REUTILISE les pid,
//! et un pid recyclé doit pouvoir être proposé à nouveau. Un retour du même pid
//! ne recharge pas la DLL deux fois : `LoadLibraryW` rend le handle existant et
//! `DllMain` ne tourne qu'une fois.
//!
//! Rien n'est chargé sans la DLL : sans `window_hook.dll` à côté de l'exe, ce
//! module journalise une fois et ne fait rien — il ne casse jamais le démarrage.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, WAIT_OBJECT_0};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE};
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, OpenMutexW, OpenProcess, WaitForSingleObject, PROCESS_ACCESS_RIGHTS,
    PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_OPERATION,
    PROCESS_VM_READ, PROCESS_VM_WRITE,
};
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::core::PCWSTR;

/// Le module installé, contrôlée par la case des Réglages.
static ACTIF: AtomicBool = AtomicBool::new(false);
/// La retenue écrite pour la DLL (voir `regler_retenue`).
static RETENUE_MS: AtomicU32 = AtomicU32::new(0);
/// La durée du vol d'ouverture écrite pour la DLL (voir `regler_ouverture`) :
/// 0 = effet d'ouverture éteint.
static OUVERTURE_MS: AtomicU32 = AtomicU32::new(0);

/// Les processus à charger, déposés par `proposer` : `(pid, instant de
/// création)`. ⛔ Le second est là pour l'injection : un pid REUTILISÉ entre le
/// dépôt et la tentative ne doit pas recevoir le module (voir `injecter`).
static A_INJECTER: Mutex<VecDeque<(u32, u64)>> = Mutex::new(VecDeque::new());
/// Les pid DÉJÀ proposés — y compris ceux dont l'injection a échoué (voir
/// l'en-tête : un échec est définitif pour ce pid).
static VUS: Mutex<Vec<(u32, Instant)>> = Mutex::new(Vec::new());

/// Au-delà, un pid sort de `VUS` : Windows reuse les pid, et la liste ne doit
/// pas grandir sans fin sur une session de plusieurs jours.
const OUBLI: Duration = Duration::from_secs(600);
const VUS_MAX: usize = 512;
/// Le signal de la pompe : on la réveille quand un pid arrive, sinon elle dort.
/// Une pompe qui interroge la file toutes les 50 ms réveillerait le portable
/// 20 fois par seconde toute la journée.
static POMPE: OnceLock<(Mutex<bool>, std::sync::Condvar)> = OnceLock::new();

pub fn actif() -> bool {
    ACTIF.load(Ordering::Relaxed)
}

/// **LA CASE DES RÉGLAGES.** Allumée : la pompe démarre et la retenue est
/// écrite pour la DLL ; éteinte : la retenue est remise à zéro (voir `arreter`),
/// pour qu'un module resté chargé dans une application ne retienne plus rien.
pub fn activer(actif: bool) {
    ACTIF.store(actif, Ordering::Relaxed);
    if actif {
        demarrer();
        ecrire_la_retenue();
    } else {
        arreter();
        crate::diag!("[injection] désactivée");
    }
    // Le fichier d'ouverture suit l'état : allumé → la durée réglée, éteint → 0
    // (sans DLL active, personne ne doit voiler une fenêtre au lancement).
    ecrire_ouverture();
}

/// **À LA FERMETURE DE NEURADESK** — et quand la case est décochée. ⛔ **LA DLL
/// LIT LA RETENUE DANS UN FICHIER** : le laisser derrière nous ferait retarder
/// de `retenue` ms chaque fermeture et chaque réduction des applications où le
/// module est resté chargé — sans la moindre animation, la nôtre n'existant
/// plus.
///
/// ⛔ **ON ÉCRIT 0, ON NE SUPPRIME PAS** : la DLL essaie ses chemins dans
/// l'ordre et retient celui du PREMIER lisible ; un fichier absent laisserait
/// gagner le repli de mesure `C:\ndtarget\window-hook.hold`, qui retiendrait
/// encore (il existait sur cette machine). Zéro ne retient rien.
pub fn arreter() {
    if let Some(chemin) = fichier_de_retenue() {
        let _ = std::fs::write(chemin, "0");
    }
    // ⛔ Même raison pour l'ouverture : une DLL restée chargée ne doit plus
    // voiler aucune fenêtre au lancement une fois NeuraDesk parti.
    if let Some(chemin) = fichier_ouverture() {
        let _ = std::fs::write(chemin, "0");
    }
}

/// **LA DURÉE DU VOL D'OUVERTURE QUE LA DLL LIRA** (`window-hook.open`). Non
/// nulle, la DLL voile la fenêtre d'une application AVANT son premier affichage
/// (`DWMWA_CLOAK`, permis dans son processus) puis nous prévient par le geste
/// `Lancer` — c'est ce qui supprime le flash de ~0,3 s du chemin externe. 0 =
/// l'effet d'ouverture est éteint.
pub fn regler_ouverture(ms: u32) {
    OUVERTURE_MS.store(ms, Ordering::Relaxed);
    ecrire_ouverture();
}

fn ecrire_ouverture() {
    let Some(chemin) = fichier_ouverture() else { return };
    if let Some(parent) = chemin.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // ⛔ 0 tant que l'injection est éteinte : sans DLL active, aucune fenêtre
    // ne doit être voilée — sinon une appli lancée resterait invisible.
    let valeur = if actif() { OUVERTURE_MS.load(Ordering::Relaxed) } else { 0 };
    let _ = std::fs::write(&chemin, valeur.to_string());
}

/// `%APPDATA%\NeuraDesk\window-hook.open` — à côté de `window-hook.hold`.
fn fichier_ouverture() -> Option<PathBuf> {
    Some(crate::config::config_dir()?.join("window-hook.open"))
}

/// ⭐ **LA DLL PILOTE-T-ELLE LE LANCEMENT DANS CE PROCESSUS ?** Vrai si
/// l'injection est active ET si le module y est chargé — décelé par le mutex
/// nommé que `DllMain` crée (`Local\NeuraDesk.WindowHook.<pid>`), lisible
/// depuis NeuraDesk puisque même session. Race-libre : le mutex existe dès
/// l'attachement de la DLL, donc avant toute fenêtre.
///
/// `ouverture_fenetres` s'en sert pour que son chemin EXTERNE laisse la fenêtre
/// à la DLL (qui la voile sans flash) au lieu de la vider de sa région.
pub(crate) fn dll_pilote_le_lancement(pid: u32) -> bool {
    actif() && module_charge_dans(pid)
}

fn module_charge_dans(pid: u32) -> bool {
    use windows::Win32::System::Threading::SYNCHRONIZATION_ACCESS_RIGHTS;
    const SYNCHRONIZE: SYNCHRONIZATION_ACCESS_RIGHTS = SYNCHRONIZATION_ACCESS_RIGHTS(0x0010_0000);
    let nom: Vec<u16> = format!("Local\\NeuraDesk.WindowHook.{pid}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        match OpenMutexW(SYNCHRONIZE, false, PCWSTR(nom.as_ptr())) {
            Ok(handle) => {
                let _ = CloseHandle(handle);
                true
            }
            Err(_) => false,
        }
    }
}

/// ⭐ **INJECTE LA DLL DANS UN PROCESSUS QU'ON VIENT DE CRÉER SUSPENDU**, avant
/// qu'il n'ait créé sa première fenêtre — pour que le crochet de `ShowWindow`
/// soit en place à temps et voile la fenêtre d'ouverture sans flash.
/// Synchrone : l'appelant REPREND le processus après (voir
/// `appicon::create_process_detached`). Sans effet si l'injection est éteinte
/// ou si la DLL est absente — l'appelant reprend quand même le processus, qui
/// repart alors sur le chemin externe.
pub fn injecter_au_lancement(pid: u32) -> bool {
    if !actif() {
        return false;
    }
    let Some(module) = module_a_charger() else {
        crate::diag!("[injection] lancement {pid} : window_hook.dll absente — chemin externe");
        return false;
    };
    // `creation = 0` : pas de contrôle de recyclage de pid — on vient de le
    // créer, c'est forcément lui.
    match injecter(pid, 0, &module) {
        Ok(()) => {
            // ⛔ Évite une SECONDE injection quand `proposer` verra le CREATE de
            // sa première fenêtre.
            let mut vus = VUS.lock().unwrap_or_else(|e| e.into_inner());
            if !vus.iter().any(|(p, _)| *p == pid) {
                vus.push((pid, Instant::now()));
            }
            crate::diag!("[injection] module chargé au lancement dans {pid}");
            true
        }
        Err(e) => {
            crate::diag!("[injection] lancement {pid} : {e} — chemin externe");
            false
        }
    }
}

/// **LA RETENUE QUE LA DLL LIRA** pour chaque geste. Elle est le plafond de
/// l'effet : `animer_geste` joue `retenue - 60 ms` (voir `fermeture.rs`). Elle
/// suit la durée réglée pour la fermeture, plus une marge pour la capture.
pub fn regler_retenue(ms: u32) {
    // ⭐ **0 VEUT DIRE « NE RETIENS RIEN »** (effets éteints, ou arrêt) : la
    // marge de capture et le plancher de 150 ms n'ont de sens que pour un effet
    // qu'on va jouer. MESURÉ (2026-11-05) : sans ce cas, 0 devenait 150 et la
    // DLL retenait quand même chaque fenêtre 150 ms pour un effet refusé.
    let valeur = if ms == 0 { 0 } else { ms.saturating_add(MARGE_RETENUE_MS).clamp(150, 5000) };
    RETENUE_MS.store(valeur, Ordering::Relaxed);
    if actif() {
        ecrire_la_retenue();
    }
}

/// La marge au-dessus de la durée de l'effet : capture (`PrintWindow` : 9 à
/// 38 ms mesurés) + une frame de la surcouche.
const MARGE_RETENUE_MS: u32 = 120;

fn ecrire_la_retenue() {
    let Some(chemin) = fichier_de_retenue() else { return };
    if let Some(parent) = chemin.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // ⛔ La DLL relit ce fichier à CHAQUE geste : il doit exister AVANT la
    // première application injectée.
    match std::fs::write(&chemin, RETENUE_MS.load(Ordering::Relaxed).to_string()) {
        Ok(()) => crate::diag!("[injection] retenue {} ms écrite dans {}", RETENUE_MS.load(Ordering::Relaxed), chemin.display()),
        Err(e) => crate::diag!("[injection] retenue illisible ({e}) : {}", chemin.display()),
    }
}

/// `%APPDATA%\NeuraDesk\window-hook.hold` — le MÊME dossier que le reste de la
/// configuration de l'utilisateur (voir `config::config_dir`).
fn fichier_de_retenue() -> Option<PathBuf> {
    Some(crate::config::config_dir()?.join("window-hook.hold"))
}

/// **APPELÉE PAR LE CROCHET** (`ouverture_fenetres::rappel`), sur son fil : elle
/// ne fait que filtrer et déposer un pid.
pub fn proposer(hwnd: HWND) {
    if !actif() {
        return;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 || pid == std::process::id() {
        return;
    }
    // La même fenêtre qu'on animerait : de premier niveau, réductible, d'une
    // application (voir `candidate`).
    if !crate::win32::ouverture_fenetres::est_animable(hwnd) {
        return;
    }
    let deja_vu = {
        let mut vus = VUS.lock().unwrap_or_else(|e| e.into_inner());
        vus.retain(|(_, quand)| quand.elapsed() < OUBLI);
        if vus.iter().any(|(p, _)| *p == pid) {
            true
        } else {
            if vus.len() >= VUS_MAX {
                vus.remove(0);
            }
            vus.push((pid, Instant::now()));
            false
        }
    };
    if deja_vu {
        return;
    }
    let mut file = A_INJECTER.lock().unwrap_or_else(|e| e.into_inner());
    if !file.iter().any(|(p, _)| *p == pid) {
        // L'instant de création est lu MAINTENANT : c'est lui qui dira, au
        // moment d'injecter, que le pid désigne encore le même processus.
        let creation = creation_du_processus(pid).unwrap_or(0);
        file.push_back((pid, creation));
        drop(file);
        if let Some((verrou, condvar)) = POMPE.get() {
            *verrou.lock().unwrap_or_else(|e| e.into_inner()) = true;
            condvar.notify_one();
        }
    }
}

fn demarrer() {
    static DEMARRE: OnceLock<()> = OnceLock::new();
    let _ = DEMARRE.get_or_init(|| {
        let lance = std::thread::Builder::new().name("injection-window-hook".into()).spawn(pomper);
        if let Err(e) = lance {
            crate::diag!("[injection] fil impossible : {e}");
        }
        crate::diag!("[injection] active — module {:?}", source_du_module());
    });
}

fn pomper() {
    let signal = POMPE.get_or_init(|| (Mutex::new(false), std::sync::Condvar::new()));
    loop {
        let demande = A_INJECTER.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
        match demande {
            Some((pid, creation)) => charger(pid, creation),
            None => {
                let mut reveille = signal.0.lock().unwrap_or_else(|e| e.into_inner());
                while !*reveille {
                    reveille = signal.1.wait(reveille).unwrap_or_else(|e| e.into_inner());
                }
                *reveille = false;
            }
        }
    }
}

/// Le module, à côté de l'exe (ou désigné par `NEURADESK_WINDOW_HOOK` pour un
/// poste de mesure). `None` : il n'a pas été construit — la case peut rester
/// cochée sans rien casser.
fn source_du_module() -> Option<PathBuf> {
    if let Ok(designee) = std::env::var("NEURADESK_WINDOW_HOOK") {
        let designee = PathBuf::from(designee);
        if designee.is_file() {
            return Some(designee);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let dossier = exe.parent()?;
    [dossier.join("window_hook.dll"), dossier.join("assets/window_hook.dll")]
        .into_iter()
        .find(|chemin| chemin.is_file())
}

/// ⛔ **ON N'INJECTE PAS LE FICHIER DE L'EXE, MAIS UNE COPIE PAR EMPREINTE.**
/// MESURÉ (2026-11-05) : une DLL chargée dans une application est VERROUILLÉE —
/// `cargo build` ne peut plus la remplacer à côté de l'exe (« Device or resource
/// busy »), et la copie de `build.rs` échoue en silence. La copie porte donc
/// l'empreinte de son contenu : une nouvelle version prend un nouveau nom, les
/// applications déjà injectées gardent l'ancienne (sans conséquence — elles
/// finiront par se fermer), et la suivante charge la nouvelle.
fn module_a_charger() -> Option<PathBuf> {
    static CHEMIN: OnceLock<Option<PathBuf>> = OnceLock::new();
    CHEMIN
        .get_or_init(|| {
            let source = source_du_module()?;
            let octets = std::fs::read(&source).ok()?;
            let empreinte = empreinte(&octets);
            // `%LOCALAPPDATA%` et non Roaming : une DLL n'a rien à faire dans
            // un dossier qui se synchronise entre machines.
            let base = std::env::var_os("LOCALAPPDATA")?;
            let dossier = PathBuf::from(base).join("NeuraDesk").join("window-hook");
            std::fs::create_dir_all(&dossier).ok()?;
            let copie = dossier.join(format!("window_hook-{empreinte:016x}.dll"));
            if !copie.is_file() {
                std::fs::write(&copie, &octets).ok()?;
                // Le ménage des versions précédentes : celles qu'une
                // application tient encore refusent la suppression — sans
                // importance, elles ne serviront plus.
                if let Ok(entrees) = std::fs::read_dir(&dossier) {
                    for entree in entrees.flatten() {
                        if entree.path() != copie {
                            let _ = std::fs::remove_file(entree.path());
                        }
                    }
                }
            }
            Some(copie)
        })
        .clone()
}

/// FNV-1a 64 bits — même empreinte que `capture_fenetre`, assez pour nommer.
fn empreinte(octets: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in octets {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn charger(pid: u32, creation: u64) {
    let Some(module) = module_a_charger() else {
        crate::diag!("[injection] {pid} : window_hook.dll introuvable à côté de l'exe — rien fait");
        return;
    };
    match injecter(pid, creation, &module) {
        Ok(()) => crate::diag!("[injection] module chargé dans {pid}"),
        Err(e) => crate::diag!("[injection] {pid} : {e} — ce processus ne sera plus proposé"),
    }
}

/// ⛔ **LES HÔTES DE L'INTERFACE WINDOWS ET SES COMPOSANTS.** Le module est fait
/// pour des applications ; une faute dans l'un de ceux-ci emporterait le bureau
/// ou le shell. Comparés en minuscules, sans chemin.
///
/// ⚠️ **`prevhost.exe` A ÉTÉ MESURÉ DANS LA LISTE DES CIBLES** (2026-11-04) : au
/// démarrage de NeuraDesk, il crée une fenêtre pour l'aperçu de l'Explorateur —
/// de premier niveau, avec cadre, sans propriétaire — donc « animable » pour
/// `candidate`. Le module n'a rien à y faire.
const EXCLUS: &[&str] = &[
    "explorer.exe",
    "dwm.exe",
    "csrss.exe",
    "winlogon.exe",
    "wininit.exe",
    "services.exe",
    "lsass.exe",
    "svchost.exe",
    "sihost.exe",
    "taskhostw.exe",
    "shellhost.exe",
    "searchhost.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "textinputhost.exe",
    "applicationframehost.exe",
    "runtimebroker.exe",
    "backgroundtaskhost.exe",
    "prevhost.exe",
    "dllhost.exe",
    "dllhst3g.exe",
    "rundll32.exe",
    "conhost.exe",
    "openwith.exe",
    "securityhealthsystray.exe",
    "ctfmon.exe",
];

fn nom_du_processus(pid: u32) -> Option<String> {
    unsafe {
        let instantane = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entree = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut trouve = None;
        if Process32FirstW(instantane, &mut entree).is_ok() {
            loop {
                if entree.th32ProcessID == pid {
                    let fin = entree.szExeFile.iter().position(|&c| c == 0).unwrap_or(entree.szExeFile.len());
                    trouve = Some(String::from_utf16_lossy(&entree.szExeFile[..fin]).to_lowercase());
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

/// **CE PID EST-IL UN HÔTE DE L'INTERFACE ?** (voir `EXCLUS`) — demandé par
/// `ouverture_fenetres::geste_retenu` : le message de geste étant public, une
/// cible du shell ne doit jamais être capturée ni cachée.
pub(crate) fn hote_exclu(pid: u32) -> bool {
    nom_du_processus(pid).is_some_and(|nom| EXCLUS.contains(&nom.as_str()))
}

/// L'instant de création d'un processus, en 100 ns depuis 1601 (l'unité de
/// `FILETIME`), ou `None` s'il est illisible. ⛔ **POUR NE PAS INJECTER DANS UN
/// AUTRE PROCESSUS QUE CELUI QUI A CRÉÉ LA FENÊTRE** : Windows recycle les pid,
/// et le pid voyage dans une file avant l'injection ; deux lectures identiques
/// disent que c'est bien le même processus.
fn creation_du_handle(processus: HANDLE) -> Option<u64> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::GetProcessTimes;
    let (mut creation, mut sortie, mut noyau, mut utilisateur) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetProcessTimes(processus, &mut creation, &mut sortie, &mut noyau, &mut utilisateur) }
        .is_ok()
        .then(|| ((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64)
}

/// La même lecture, par pid — pour `proposer`, qui n'a pas de handle.
fn creation_du_processus(pid: u32) -> Option<u64> {
    unsafe {
        let processus = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let creation = creation_du_handle(processus);
        let _ = CloseHandle(processus);
        creation
    }
}

/// `LoadLibraryW` dans le processus `pid` — la même injection que
/// `window-hook/src/main.rs`, resserrée : seuls les droits nécessaires sont
/// demandés, ce qui échoue proprement sur un processus protégé ou d'une autre
/// session.
fn injecter(pid: u32, creation: u64, module: &Path) -> Result<(), String> {
    let nom = nom_du_processus(pid).ok_or_else(|| "processus introuvable".to_string())?;
    if EXCLUS.contains(&nom.as_str()) {
        return Err(format!("{nom} exclu"));
    }
    let chemin = std::fs::canonicalize(module).map_err(|e| format!("chemin illisible : {e}"))?;
    let large: Vec<u16> = chemin.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    let octets = large.len() * 2;
    let droits: PROCESS_ACCESS_RIGHTS =
        PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION | PROCESS_VM_WRITE | PROCESS_VM_READ;

    unsafe {
        let processus = OpenProcess(droits, false, pid).map_err(|e| format!("OpenProcess : {e}"))?;
        // ⛔ **LE PID A-T-IL CHANGÉ DE PROPRIÉTAIRE ?** Windows les recycle ; la
        // vérification se fait sur LE handle par lequel on va injecter, pas
        // avant (un second `OpenProcess` laisserait une fenêtre entre les deux).
        if creation != 0 && creation_du_handle(processus).is_some_and(|vu| vu != creation) {
            let _ = CloseHandle(processus);
            return Err("le pid désigne un AUTRE processus qu'au dépôt — rien injecté".into());
        }
        let adresse = VirtualAllocEx(processus, None, octets, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if adresse.is_null() {
            let _ = CloseHandle(processus);
            return Err("VirtualAllocEx a échoué".into());
        }
        let mut ecrits = 0usize;
        let ecriture = WriteProcessMemory(processus, adresse, large.as_ptr() as *const c_void, octets, Some(&mut ecrits));
        // Libération de la mémoire distante (voir plus bas) : seulement si le
        // fil distant a FINI d'utiliser le chemin qu'elle porte.
        let liberable = std::cell::Cell::new(true);

        let resultat = (|| -> Result<(), String> {
            if ecriture.is_err() || ecrits != octets {
                return Err(format!("WriteProcessMemory a écrit {ecrits}/{octets} octets (processus 32 bits ?)"));
            }
            let noyau = GetModuleHandleW(windows::core::w!("kernel32.dll")).map_err(|e| format!("kernel32 : {e}"))?;
            let charge = GetProcAddress(noyau, windows::core::s!("LoadLibraryW"))
                .ok_or("LoadLibraryW introuvable dans kernel32")?;
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
                // ⛔ **LE FIL VIT ENCORE** : fermer son handle ne l'arrête pas,
                // et `LoadLibraryW` lirait le chemin qu'on s'apprêterait à
                // libérer. On laisse le bloc au processus fautif (cinq secondes
                // déjà écoulées, l'application ne répond pas) : une fuite de
                // quelques dizaines d'octets vaut mieux qu'un accès invalide
                // dans l'application d'un utilisateur.
                liberable.set(false);
                return Err("le fil distant n'a pas répondu en 5 s".into());
            }
            if code == 0 {
                return Err("LoadLibraryW a rendu 0".into());
            }
            Ok(())
        })();

        if liberable.get() {
            let _ = VirtualFreeEx(processus, adresse, 0, MEM_RELEASE);
        }
        let _ = CloseHandle(processus);
        resultat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les hôtes de l'interface sont exclus — c'est la liste qui protège le
    /// bureau, et une faute de frappe y serait silencieuse.
    #[test]
    fn les_hotes_du_bureau_sont_exclus() {
        for nom in ["explorer.exe", "dwm.exe", "winlogon.exe", "csrss.exe", "sihost.exe"] {
            assert!(EXCLUS.contains(&nom), "{nom} doit être exclu");
        }
    }

    /// ⛔ Aucune exclusion en majuscules : `nom_du_processus` rend des
    /// minuscules, et une entrée `"Explorer.exe"` ne serait jamais comparée
    /// égale.
    #[test]
    fn la_liste_des_exclusions_est_en_minuscules() {
        for nom in EXCLUS {
            assert_eq!(*nom, nom.to_lowercase(), "{nom} n'est pas en minuscules");
        }
    }

    /// La marge de retenue laisse le temps à la capture SANS allonger l'effet
    /// au-delà du raisonnable — et **0 reste 0** : c'est « ne retiens rien »
    /// (effets éteints), que la DLL reconnaît (voir `window-hook`).
    #[test]
    fn la_retenue_ecrite_est_bornee() {
        regler_retenue(0);
        assert_eq!(RETENUE_MS.load(Ordering::Relaxed), 0);
        regler_retenue(30_000);
        assert_eq!(RETENUE_MS.load(Ordering::Relaxed), 5000);
        regler_retenue(300);
        assert_eq!(RETENUE_MS.load(Ordering::Relaxed), 420);
    }
}
