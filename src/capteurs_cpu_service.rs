//! `neuradesk-capteurs.exe` — le SEUL exécutable élevé des capteurs du
//! processeur (voir `win32::capteurs_cpu`, doc de module, pour le pourquoi).
//! Pas de fenêtre, pas de CEF, rien d'installé.
//!
//! Ligne de commande : `--servir <pid de NeuraDesk>`. Lancé par l'invite UAC,
//! il lit le processeur chaque seconde par PawnIO, publie dans la section
//! partagée, et s'arrête dès que le processus `<pid>` se termine.
//!
//! ⚠️ **IL NE LIT RIEN D'AUTRE QUE CE PID** : aucune entrée depuis la section
//! (qu'il est seul à écrire), les modules PawnIO EMBARQUÉS dans l'exe,
//! `PawnIOLib.dll` chargée par son chemin complet sous `Program Files`, et
//! seulement des LECTURES de registres (température, énergie).

#![windows_subsystem = "windows"]

#[allow(dead_code)]
#[path = "win32/capteurs_cpu.rs"]
mod capteurs_cpu;

use std::time::{Duration, Instant};

use capteurs_cpu::{en_dixiemes, Publication, MAGIE, SECTION, VERSION};
use windows::core::{s, w, HSTRING, PCSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_ALREADY_EXISTS, HANDLE, HLOCAL, INVALID_HANDLE_VALUE, WAIT_TIMEOUT,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH};
use windows::Win32::System::Memory::{CreateFileMappingW, MapViewOfFile, FILE_MAP_WRITE, PAGE_READWRITE};
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcess, OpenProcess, OpenProcessToken, WaitForSingleObject, PROCESS_SYNCHRONIZE,
};

/// Les modules signés de PawnIO (LGPL-2.1, `vendor/pawnio/`).
const MODULE_AMD: &[u8] = include_bytes!("../vendor/pawnio/AMDFamily17.bin");
const MODULE_INTEL: &[u8] = include_bytes!("../vendor/pawnio/IntelMSR.bin");

const SMN_THM_TCON_CUR_TMP: u64 = 0x0005_9800;
const MSR_AMD_PWR_UNIT: u64 = 0xC001_0299;
const MSR_AMD_PKG_ENERGY: u64 = 0xC001_029B;
const MSR_INTEL_TEMPERATURE_TARGET: u64 = 0x1A2;
const MSR_INTEL_PACKAGE_THERM_STATUS: u64 = 0x1B1;
const MSR_INTEL_RAPL_POWER_UNIT: u64 = 0x606;
const MSR_INTEL_PKG_ENERGY: u64 = 0x611;

const PERIODE: Duration = Duration::from_secs(1);

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let resultat = match arguments.as_slice() {
        [mode, pid] if mode == "--servir" => pid.parse::<u32>().map_err(|_| "pid".to_string()).and_then(servir),
        _ => Err("arguments".into()),
    };
    std::process::exit(if resultat.is_ok() { 0 } else { 1 });
}

// ---------------------------------------------------------------------------
// PAWNIO
// ---------------------------------------------------------------------------

type Ouvrir = unsafe extern "system" fn(*mut HANDLE) -> i32;
type Charger = unsafe extern "system" fn(HANDLE, *const u8, usize) -> i32;
type Executer = unsafe extern "system" fn(HANDLE, PCSTR, *const u64, usize, *mut u64, usize, *mut usize) -> i32;

struct PawnIo {
    poignee: HANDLE,
    executer: Executer,
}

impl PawnIo {
    fn ouvrir(module: &[u8]) -> Result<PawnIo, String> {
        let chemin = capteurs_cpu::pawnio_lib().ok_or("PawnIO absent")?;
        // SAFETY : chemin complet sous Program Files ; signatures de `PawnIOLib.h`.
        unsafe {
            let m = LoadLibraryExW(&HSTRING::from(chemin.as_os_str()), None, LOAD_WITH_ALTERED_SEARCH_PATH)
                .map_err(|e| format!("PawnIOLib : {e}"))?;
            let ouvrir: Ouvrir = std::mem::transmute(GetProcAddress(m, s!("pawnio_open")).ok_or("pawnio_open")?);
            let charger: Charger = std::mem::transmute(GetProcAddress(m, s!("pawnio_load")).ok_or("pawnio_load")?);
            let executer: Executer =
                std::mem::transmute(GetProcAddress(m, s!("pawnio_execute")).ok_or("pawnio_execute")?);
            let mut poignee = HANDLE::default();
            let hr = ouvrir(&mut poignee);
            if hr < 0 {
                return Err(format!("pawnio_open : {hr:#x}"));
            }
            let hr = charger(poignee, module.as_ptr(), module.len());
            if hr < 0 {
                return Err(format!("pawnio_load : {hr:#x}"));
            }
            Ok(PawnIo { poignee, executer })
        }
    }

    fn un(&self, fonction: PCSTR, entree: u64) -> Option<u64> {
        let mut sortie = 0u64;
        let mut rendu = 0usize;
        // SAFETY : une entrée, une sortie, comme les `DEFINE_IOCTL_SIZED(…, 1, 1)` des modules.
        let hr = unsafe { (self.executer)(self.poignee, fonction, &entree, 1, &mut sortie, 1, &mut rendu) };
        (hr >= 0 && rendu == 1).then_some(sortie)
    }

    fn msr(&self, numero: u64) -> Option<u64> {
        self.un(s!("ioctl_read_msr"), numero)
    }

    fn smn(&self, adresse: u64) -> Option<u32> {
        self.un(s!("ioctl_read_smn"), adresse).map(|v| v as u32)
    }
}

// ---------------------------------------------------------------------------
// LE PROCESSEUR
// ---------------------------------------------------------------------------

enum Fabricant {
    Amd { decalage: f32 },
    Intel { cible: u64 },
}

/// `(AMD ?, nom commercial)` d'après CPUID.
fn identifier() -> Option<(bool, String)> {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::__cpuid;
        // CPUID existe sur tout x86_64 (`__cpuid` est sûr depuis Rust 1.87).
        let (vendeur, nom) = {
            let r = __cpuid(0);
            let vendeur: Vec<u8> = [r.ebx, r.edx, r.ecx].iter().flat_map(|m| m.to_le_bytes()).collect();
            let nom: Vec<u8> = (0x8000_0002u32..=0x8000_0004)
                .flat_map(|f| {
                    let r = __cpuid(f);
                    [r.eax, r.ebx, r.ecx, r.edx].into_iter().flat_map(|m| m.to_le_bytes())
                })
                .collect();
            (vendeur, nom)
        };
        let nom = String::from_utf8_lossy(&nom).trim_matches(char::from(0)).trim().to_string();
        match vendeur.as_slice() {
            b"AuthenticAMD" => Some((true, nom)),
            b"GenuineIntel" => Some((false, nom)),
            _ => None,
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    None
}

struct Lecture {
    pawnio: PawnIo,
    fabricant: Fabricant,
    unite: Option<u64>,
    energie: Option<(u64, Instant)>,
}

impl Lecture {
    fn ouvrir() -> Result<Lecture, String> {
        let (amd, nom) = identifier().ok_or("processeur ni AMD ni Intel")?;
        let pawnio = PawnIo::ouvrir(if amd { MODULE_AMD } else { MODULE_INTEL })?;
        let (fabricant, unite) = if amd {
            (Fabricant::Amd { decalage: capteurs_cpu::decalage_tdie_amd(&nom) }, pawnio.msr(MSR_AMD_PWR_UNIT))
        } else {
            let cible = pawnio.msr(MSR_INTEL_TEMPERATURE_TARGET).unwrap_or(0);
            (Fabricant::Intel { cible }, pawnio.msr(MSR_INTEL_RAPL_POWER_UNIT))
        };
        Ok(Lecture { pawnio, fabricant, unite, energie: None })
    }

    fn temperature(&self) -> Option<f32> {
        match self.fabricant {
            Fabricant::Amd { decalage } => {
                self.pawnio.smn(SMN_THM_TCON_CUR_TMP).map(|v| capteurs_cpu::tctl_amd(v) - decalage)
            }
            Fabricant::Intel { cible } => {
                self.pawnio.msr(MSR_INTEL_PACKAGE_THERM_STATUS).map(|v| capteurs_cpu::temperature_intel(v, cible))
            }
        }
    }

    fn puissance(&mut self) -> Option<f32> {
        let unite = self.unite?;
        let msr = match self.fabricant {
            Fabricant::Amd { .. } => MSR_AMD_PKG_ENERGY,
            Fabricant::Intel { .. } => MSR_INTEL_PKG_ENERGY,
        };
        let maintenant = Instant::now();
        let energie = self.pawnio.msr(msr)?;
        let (avant, quand) = self.energie.replace((energie, maintenant))?;
        capteurs_cpu::puissance_w(avant, energie, unite, maintenant.duration_since(quand).as_secs_f64())
    }
}

// ---------------------------------------------------------------------------
// LE SERVICE
// ---------------------------------------------------------------------------

/// Le SID de l'utilisateur (le même, élevé ou non, sous l'UAC).
fn sid_de_l_utilisateur() -> Result<String, String> {
    // SAFETY : jeton du processus courant, tampon de la taille annoncée ;
    // chaîne rendue par le système, libérée ci-dessous.
    unsafe {
        let mut jeton = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut jeton).map_err(|e| format!("jeton : {e}"))?;
        let mut tampon = vec![0u8; 256];
        let mut rendu = 0u32;
        let r = GetTokenInformation(jeton, TokenUser, Some(tampon.as_mut_ptr().cast()), tampon.len() as u32, &mut rendu);
        let _ = CloseHandle(jeton);
        r.map_err(|e| format!("jeton : {e}"))?;
        let utilisateur = &*(tampon.as_ptr() as *const TOKEN_USER);
        let mut texte = PWSTR::null();
        ConvertSidToStringSidW(utilisateur.User.Sid, &mut texte).map_err(|e| format!("SID : {e}"))?;
        let sid = texte.to_string().map_err(|e| format!("SID : {e}"));
        let _ = LocalFree(Some(HLOCAL(texte.0.cast())));
        sid
    }
}

/// La section : tout pour le système et les administrateurs, LECTURE seule
/// pour l'utilisateur.
fn creer_la_section() -> Result<*mut Publication, String> {
    let sddl = HSTRING::from(format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GR;;;{})", sid_de_l_utilisateur()?));
    // SAFETY : descripteur alloué par le système (gardé pour la vie du
    // processus, comme la section) ; vue de la taille d'une `Publication`.
    unsafe {
        let mut descripteur = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(sddl.as_ptr()), SDDL_REVISION_1, &mut descripteur, None)
            .map_err(|e| format!("descripteur : {e}"))?;
        let attributs = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descripteur.0,
            bInheritHandle: false.into(),
        };
        let section = CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            Some(&attributs),
            PAGE_READWRITE,
            0,
            std::mem::size_of::<Publication>() as u32,
            &HSTRING::from(SECTION),
        )
        .map_err(|e| format!("section : {e}"))?;
        let vue = MapViewOfFile(section, FILE_MAP_WRITE, 0, 0, std::mem::size_of::<Publication>());
        if vue.Value.is_null() {
            return Err("vue de la section".into());
        }
        Ok(vue.Value as *mut Publication)
    }
}

fn servir(pid: u32) -> Result<(), String> {
    // SAFETY : sans précondition ; la poignée vit jusqu'à la fin du processus.
    let neuradesk = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.map_err(|e| format!("processus {pid} : {e}"))?;
    // Un seul service à la fois (un second clic sur le bouton du panneau).
    // SAFETY : nom littéral ; la poignée vit jusqu'à la fin du processus.
    let _unique = unsafe { CreateMutexW(None, true, w!("Local\\NeuraDesk-capteurs-cpu-service")) }
        .map_err(|e| format!("mutex : {e}"))?;
    // SAFETY : sans précondition.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return Ok(());
    }
    let publication = creer_la_section()?;
    // Sans lecture possible, on publie quand même (des mesures absentes) :
    // le panneau n'affiche alors rien, au lieu d'attendre.
    let mut lecture = Lecture::ouvrir().ok();
    let mut compteur = 0u64;
    loop {
        let (temperature, puissance) = match lecture.as_mut() {
            Some(l) => (l.temperature(), l.puissance()),
            None => (None, None),
        };
        compteur += 1;
        let p = Publication {
            magie: MAGIE,
            version: VERSION,
            compteur,
            temperature_dixiemes: en_dixiemes(temperature, -20.0, 150.0),
            puissance_dixiemes: en_dixiemes(puissance, 0.0, 2000.0),
        };
        // SAFETY : la vue couvre une `Publication`.
        unsafe { std::ptr::write_volatile(publication, p) };
        // Attend une seconde, ou la fin de NeuraDesk.
        // SAFETY : poignée de processus valide.
        if unsafe { WaitForSingleObject(neuradesk, PERIODE.as_millis() as u32) } != WAIT_TIMEOUT {
            return Ok(());
        }
    }
}
