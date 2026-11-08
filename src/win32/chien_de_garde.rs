//! Chien de garde de la boucle principale — **DIAGNOSTIC SEULEMENT**.
//!
//! Windows a noté plusieurs `AppHangTransient` pour neuradesk.exe le
//! 2026-10-18 (« l'application ne répond pas », environ toutes les cinq
//! minutes), sans que le journal ne dise où la boucle s'arrête. Ce fil le dit :
//! la boucle bat à chaque frame (`battre`) ; quand le battement manque depuis
//! plus de `SEUIL`, le fil suspend un instant le fil principal, relève sa pile
//! et l'écrit au journal, fonctions et lignes comprises.
//!
//! ⚠️ **RIEN N'EST ALLOUÉ PENDANT QUE LE FIL EST SUSPENDU** : il peut tenir le
//! verrou du tas, et une allocation de notre côté attendrait alors à jamais. Les
//! adresses sont relevées dans un tableau fixe ; les symboles (dbghelp) ne sont
//! résolus qu'une fois le fil reparti.
//!
//! Inactif sans `NEURADESK_DIAG` : le fil n'est même pas lancé.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use std::sync::OnceLock;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Diagnostics::Debug::{
    GetThreadContext, RtlLookupFunctionEntry, RtlVirtualUnwind, SymFromAddr, SymGetLineFromAddr64, SymInitialize,
    SymSetOptions, CONTEXT, CONTEXT_FULL_AMD64, IMAGEHLP_LINE64, SYMBOL_INFO, SYMOPT_DEFERRED_LOADS, SYMOPT_LOAD_LINES,
    SYMOPT_UNDNAME, UNW_FLAG_NHANDLER,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread, THREAD_GET_CONTEXT,
    THREAD_QUERY_INFORMATION, THREAD_SUSPEND_RESUME,
};

/// Au-delà, la boucle est tenue pour arrêtée. Windows parle de « ne répond
/// pas » à cinq secondes ; relever plus tôt montre le début du blocage.
const SEUIL: Duration = Duration::from_millis(1500);
/// Un relevé de plus toutes les `REPRISE` tant que la boucle reste arrêtée :
/// une pile qui ne bouge pas désigne un blocage, une pile qui change, un calcul.
const REPRISE: Duration = Duration::from_secs(2);
const RELEVES_MAX: u32 = 4;
const PROFONDEUR: usize = 40;

static DEBUT: OnceLock<Instant> = OnceLock::new();
static BATTEMENT_MS: AtomicU64 = AtomicU64::new(0);

fn maintenant_ms() -> u64 {
    DEBUT.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// À appeler à chaque tour de la boucle principale.
pub fn battre() {
    BATTEMENT_MS.store(maintenant_ms(), Ordering::Relaxed);
}

/// Lance le fil de garde sur le fil APPELANT (la boucle principale).
pub fn installer() {
    if !crate::diag::enabled() {
        return;
    }
    battre();
    let acces = THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION;
    let Ok(fil) = (unsafe { OpenThread(acces, false, GetCurrentThreadId()) }) else {
        crate::diag!("[chien de garde] fil principal inaccessible");
        return;
    };
    let fil = fil.0 as usize;
    let _ = std::thread::Builder::new().name("chien-de-garde".into()).spawn(move || garder(HANDLE(fil as _)));
}

fn garder(fil: HANDLE) {
    unsafe {
        SymSetOptions(SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS | SYMOPT_LOAD_LINES);
        let _ = SymInitialize(GetCurrentProcess(), None, true);
    }
    let mut arret_depuis: Option<u64> = None;
    let mut releves = 0;
    let mut prochain = 0u64;
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let battement = BATTEMENT_MS.load(Ordering::Relaxed);
        let ecart = maintenant_ms().saturating_sub(battement);
        if ecart < SEUIL.as_millis() as u64 {
            if let Some(debut) = arret_depuis.take() {
                crate::diag!("[chien de garde] boucle repartie apres {} ms", battement.saturating_sub(debut));
            }
            continue;
        }
        if arret_depuis != Some(battement) {
            arret_depuis = Some(battement);
            releves = 0;
            prochain = 0;
        }
        if releves >= RELEVES_MAX || maintenant_ms() < prochain {
            continue;
        }
        releves += 1;
        prochain = maintenant_ms() + REPRISE.as_millis() as u64;
        let mut pile = [0u64; PROFONDEUR];
        let n = unsafe { relever(fil, &mut pile) };
        let mut texte = format!("[chien de garde] boucle arretee depuis {ecart} ms, pile ({releves}/{RELEVES_MAX}) :");
        for &adresse in &pile[..n] {
            texte.push_str("\n    ");
            texte.push_str(&symbole(adresse));
        }
        crate::diag::log_line(&texte);
    }
}

#[repr(C, align(16))]
struct Contexte(CONTEXT);

/// Relève les adresses de retour du fil, SUSPENDU le temps du relevé.
unsafe fn relever(fil: HANDLE, pile: &mut [u64; PROFONDEUR]) -> usize {
    unsafe {
        if SuspendThread(fil) == u32::MAX {
            return 0;
        }
        let mut ctx: Contexte = std::mem::zeroed();
        ctx.0.ContextFlags = CONTEXT_FULL_AMD64;
        let mut n = 0;
        if GetThreadContext(fil, &mut ctx.0).is_ok() {
            while n < PROFONDEUR && ctx.0.Rip != 0 {
                pile[n] = ctx.0.Rip;
                n += 1;
                let mut base = 0u64;
                let entree = RtlLookupFunctionEntry(ctx.0.Rip, &mut base, None);
                if entree.is_null() {
                    // Fonction feuille : l'adresse de retour est au sommet de la pile.
                    ctx.0.Rip = *(ctx.0.Rsp as *const u64);
                    ctx.0.Rsp += 8;
                } else {
                    let mut donnees = std::ptr::null_mut();
                    let mut cadre = 0u64;
                    let _ = RtlVirtualUnwind(UNW_FLAG_NHANDLER, base, ctx.0.Rip, entree, &mut ctx.0, &mut donnees, &mut cadre, None);
                }
            }
        }
        ResumeThread(fil);
        n
    }
}

fn symbole(adresse: u64) -> String {
    #[repr(C)]
    struct Tampon {
        info: SYMBOL_INFO,
        nom: [u8; 512],
    }
    unsafe {
        let mut t: Tampon = std::mem::zeroed();
        t.info.SizeOfStruct = std::mem::size_of::<SYMBOL_INFO>() as u32;
        t.info.MaxNameLen = 512;
        let processus = GetCurrentProcess();
        let mut decalage = 0u64;
        if SymFromAddr(processus, adresse, Some(&mut decalage), &mut t.info).is_err() {
            return format!("{adresse:#x}");
        }
        let nom = std::slice::from_raw_parts(t.info.Name.as_ptr() as *const u8, t.info.NameLen as usize);
        let mut texte = format!("{}+{decalage:#x}", String::from_utf8_lossy(nom));
        let mut ligne = IMAGEHLP_LINE64 { SizeOfStruct: std::mem::size_of::<IMAGEHLP_LINE64>() as u32, ..Default::default() };
        let mut colonne = 0u32;
        if SymGetLineFromAddr64(processus, adresse, &mut colonne, &mut ligne).is_ok() && !ligne.FileName.is_null() {
            texte.push_str(&format!("  ({}:{})", ligne.FileName.to_string().unwrap_or_default(), ligne.LineNumber));
        }
        texte
    }
}
