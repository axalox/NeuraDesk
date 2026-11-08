//! Journal des ARRÊTS — TOUJOURS actif, contrairement à `diag`.
//!
//! **POURQUOI (2026-10-31)** : sur la VM Windows 10, NeuraDesk a disparu deux
//! fois sans laisser la moindre trace. Le profil release est en
//! `panic = "abort"`, l'application n'a pas de console (son `stderr` ne va
//! nulle part), le `panic` hook de `diag` n'existe qu'avec `NEURADESK_DIAG=1`,
//! et un abandon de ce genre n'écrit pas toujours d'événement dans le journal
//! Windows. Une panique en usage normal était donc INDISCERNABLE d'un arrêt
//! demandé.
//!
//! Ce fichier, `neuradesk-plantages.log` à côté de l'exécutable, reçoit en
//! AJOUT, avec la date et l'heure locales :
//! - toute panique (message, emplacement, fil, pile) ;
//! - l'erreur fatale que `app::run` rend à `main` ;
//! - la fin NORMALE de l'application — c'est elle qui permet de conclure :
//!   un lancement suivi ni d'une panique ni d'une fin normale est mort AUTREMENT
//!   (abandon natif dans CEF ou un pilote, processus tué, session fermée).
//!
//! Coût nul en marche : rien n'est ouvert tant que rien n'est écrit, et on
//! n'écrit qu'au démarrage, à la sortie, et à la panique.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Taille au-delà de laquelle le journal passe en `.1.log` (un seul ancien
/// gardé) : il ne doit jamais grossir sans fin sur une machine qui relance
/// l'application tous les jours.
const TAILLE_MAX: u64 = 256 * 1024;

const NOM: &str = "neuradesk-plantages.log";

/// Sérialise les écritures : une panique peut venir de n'importe quel fil.
static VERROU: Mutex<()> = Mutex::new(());

/// Installe le `panic` hook permanent et note le démarrage. À appeler UNE fois,
/// depuis le processus PRINCIPAL seulement (après `dispatch_subprocess`), AVANT
/// `diag::init` : le hook de `diag` s'enchaîne alors sur celui-ci.
pub fn installer() {
    let precedent = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let fil = std::thread::current();
        let pile = std::backtrace::Backtrace::force_capture();
        noter(&format!("PANIQUE dans le fil « {} » : {info}\n{pile}", fil.name().unwrap_or("sans nom")));
        precedent(info);
    }));
    noter(&format!("démarrage (pid {}, {})", std::process::id(), env!("CARGO_PKG_VERSION")));
}

/// Ajoute une entrée datée au journal. Best-effort : un échec d'écriture ne
/// doit jamais aggraver la situation (on est souvent en pleine panique).
pub fn noter(message: &str) {
    let Some(chemin) = chemin() else { return };
    let _garde = VERROU.lock().unwrap_or_else(|empoisonne| empoisonne.into_inner());
    let _ = ecrire_dans(&chemin, &entree(&date_locale(), std::process::id(), message), TAILLE_MAX);
}

fn chemin() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(|dossier| dossier.join(NOM))
}

fn date_locale() -> String {
    // SAFETY: `GetLocalTime` ne fait que remplir une structure rendue par
    // valeur ; aucun pointeur ni état partagé.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}

/// Une entrée : `[date] pid N — message`, terminée par un saut de ligne.
fn entree(date: &str, pid: u32, message: &str) -> String {
    format!("[{date}] pid {pid} — {message}\n")
}

/// Ajoute `texte` à `chemin`, après avoir déplacé un journal devenu plus gros
/// que `taille_max` vers `<nom>.1.log` (l'ancien `.1` est écrasé).
fn ecrire_dans(chemin: &Path, texte: &str, taille_max: u64) -> std::io::Result<()> {
    if std::fs::metadata(chemin).map(|m| m.len() > taille_max).unwrap_or(false) {
        let _ = std::fs::rename(chemin, chemin.with_extension("1.log"));
    }
    let mut fichier = std::fs::OpenOptions::new().create(true).append(true).open(chemin)?;
    fichier.write_all(texte.as_bytes())?;
    fichier.flush()
}

// ---------------------------------------------------------------------------
// PERTE DU PÉRIPHÉRIQUE GRAPHIQUE : RELANCER AU LIEU DE DISPARAÎTRE
// ---------------------------------------------------------------------------
//
// **MESURÉ (2026-10-31, VM VirtualBox Windows 10)** : à 11:39:41 `dwm.exe`
// plante, à 11:39:42 Explorer plante dans `VBoxDX.dll` (le pilote 3D de
// VirtualBox), Windows relance Explorer — et NeuraDesk disparaît dix secondes
// plus tard, sans panique ni événement de plantage. Le chemin lu dans le code :
// `swapchain.present()?` rend une erreur DXGI, le `?` sort de `app::run`, et
// `main` quittait en code 1. Explorer est relancé par Windows ; NeuraDesk,
// personne ne le relançait.
//
// Le correctif relance l'application ENTIÈRE (même chemin que « Recharger ») :
// recréer à chaud chaque ressource D3D, chaque swap chain et chaque texture CEF
// serait bien plus lourd, pour un événement rare.

/// Au plus `RELANCES_MAX` relances dans `FENETRE_RELANCES_S` secondes : au-delà,
/// le pilote est en panne durable, et relancer en boucle ne ferait que clignoter.
const RELANCES_MAX: usize = 3;
const FENETRE_RELANCES_S: u64 = 300;

/// Les familles d'erreurs (« facility ») de DXGI et de Direct3D 11. Une erreur
/// de ces familles qui remonte jusqu'à `main` vient du rendu : périphérique
/// retiré (`0x887A0005`), bloqué (`…0006`), réinitialisé (`…0007`), erreur
/// interne du pilote (`…0020`)… Le code exact de la VM n'est pas encore relevé :
/// on prend donc les deux familles ENTIÈRES plutôt qu'une liste qui pourrait
/// manquer le bon.
const FAMILLES_GPU: [u32; 2] = [0x887A_0000, 0x887C_0000];

fn est_un_code_gpu(code: i32) -> bool {
    FAMILLES_GPU.contains(&((code as u32) & 0xFFFF_0000))
}

/// Vrai si l'erreur, ou l'une de ses causes, est une erreur DXGI/Direct3D.
pub fn est_une_perte_du_gpu(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut courante = Some(err);
    while let Some(e) = courante {
        if let Some(w) = e.downcast_ref::<windows::core::Error>() {
            if est_un_code_gpu(w.code().0) {
                return true;
            }
        }
        courante = e.source();
    }
    false
}

/// Décide si une relance est permise à l'instant `maintenant` (secondes Unix),
/// d'après les relances précédentes. Rend la décision et le NOUVEL historique :
/// les relances trop anciennes en sont retirées, et celle-ci y est ajoutée si
/// elle est permise.
fn decider_la_relance(historique: &[u64], maintenant: u64) -> (bool, Vec<u64>) {
    let recentes: Vec<u64> = historique
        .iter()
        .copied()
        .filter(|&t| t <= maintenant && maintenant - t < FENETRE_RELANCES_S)
        .collect();
    if recentes.len() >= RELANCES_MAX {
        return (false, recentes);
    }
    let avec_celle_ci = recentes.into_iter().chain(std::iter::once(maintenant)).collect();
    (true, avec_celle_ci)
}

/// Lit et met à jour `neuradesk-relances.txt` (une date Unix par ligne) à côté
/// de l'exécutable. Un fichier illisible vaut un historique vide.
pub fn relance_autorisee() -> bool {
    let Some(chemin) = chemin().map(|c| c.with_file_name("neuradesk-relances.txt")) else { return false };
    let maintenant = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let historique: Vec<u64> = std::fs::read_to_string(&chemin)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect();
    let (permise, nouvel) = decider_la_relance(&historique, maintenant);
    let texte: String = nouvel.iter().map(|t| format!("{t}\n")).collect();
    let _ = std::fs::write(&chemin, texte);
    permise
}

// ─── PLANTAGE NATIF ─────────────────────────────────────────────────────────
//
// ⭐ DÉFAUT SIGNALÉ (2026-10-31) : « l'app ne tourne pas sur la vm ». Sur la VM
// Windows 10, le pilote 3D de VirtualBox perd le périphérique toutes les ~5 min
// 30 ; `app::run` rend alors 0x887A0005 et `main` relance (voir plus haut). Mais
// à 19:46:40, la perte s'est faite DANS le pilote : violation d'accès dans
// `VBoxDX.dll` (WER, événement 1000). Aucune erreur remontée, aucune panique :
// le processus est mort sans relève, et ce journal n'en disait RIEN.
//
// Le filtre ci-dessous note le plantage (code, module, décalage) et lance une
// instance de relève — sous la même limite que la relance après perte du GPU
// (`relance_autorisee`). La relève ATTEND la mort de celle-ci
// (`attendre_si_demande`) : tant que WER tient le processus planté, son cache
// CEF est verrouillé, et une instance qui démarre trop tôt échoue.

/// Le PID de l'instance plantée que la relève doit attendre.
const ATTENDRE_PID: &str = "NEURADESK_ATTENDRE_PID";

/// Combien de temps la relève attend au plus l'instance plantée (WER peut la
/// garder plusieurs secondes le temps d'écrire son rapport).
const ATTENTE_MAX_MS: u32 = 60_000;

/// Installe le filtre des exceptions NON GÉRÉES. À appeler APRÈS
/// `cef_app::init` : CEF peut installer le sien en s'initialisant, et le
/// dernier installé gagne.
pub fn installer_le_filtre_natif() {
    use windows::Win32::System::Diagnostics::Debug::SetUnhandledExceptionFilter;
    // SAFETY : `filtre` a la signature attendue et vit tout le programme.
    unsafe { SetUnhandledExceptionFilter(Some(filtre)) };
    levier_de_plantage();
}

/// ⚠️ **LEVIER DE VÉRIFICATION** — `NEURADESK_DEBUG_PLANTAGE_NATIF=<secondes>`
/// lève une violation d'accès NON GÉRÉE après ce délai, sur un fil à part :
/// le seul moyen de voir le filtre travailler sans attendre que le pilote de la
/// VM plante.
fn levier_de_plantage() {
    let Some(secondes) =
        std::env::var("NEURADESK_DEBUG_PLANTAGE_NATIF").ok().and_then(|s| s.trim().parse::<u64>().ok())
    else {
        return;
    };
    let _ = std::thread::Builder::new().name("levier-plantage".into()).spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(secondes));
        // SAFETY : lève volontairement une exception non continuable ; aucun
        // pointeur n'est déréférencé.
        unsafe {
            windows::Win32::System::Diagnostics::Debug::RaiseException(
                0xC000_0005,
                1, // EXCEPTION_NONCONTINUABLE
                None,
            )
        };
    });
}

unsafe extern "system" fn filtre(info: *const windows::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS) -> i32 {
    use std::sync::atomic::{AtomicBool, Ordering};
    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;
    static DEJA: AtomicBool = AtomicBool::new(false);
    // Un seul passage : un second plantage pendant celui-ci ne relance pas deux fois.
    if DEJA.swap(true, Ordering::SeqCst) {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    // SAFETY : Windows fournit `info` et son enregistrement, valides pendant l'appel.
    let (code, adresse) = unsafe {
        match info.as_ref().and_then(|i| i.ExceptionRecord.as_ref()) {
            Some(r) => (r.ExceptionCode.0 as u32, r.ExceptionAddress as usize),
            None => (0, 0),
        }
    };
    let (module, base) = module_de(adresse);
    noter(&format!("PLANTAGE NATIF 0x{code:08X} dans {module} (+0x{:x})", adresse.wrapping_sub(base)));
    if relance_autorisee() {
        noter("plantage natif : relance de l'application");
        relancer_apres_moi();
    } else {
        noter("plantage natif : trop de relances récentes, abandon");
    }
    // WER poursuit : rapport, puis fin du processus.
    EXCEPTION_CONTINUE_SEARCH
}

/// Le fichier du module qui contient `adresse`, et sa base.
fn module_de(adresse: usize) -> (String, usize) {
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{
        GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    };
    let mut module = HMODULE::default();
    // SAFETY : `adresse` n'est lue que comme clé de recherche par le chargeur.
    let trouve = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            windows::core::PCWSTR(adresse as *const u16),
            &mut module,
        )
    };
    if trouve.is_err() {
        return ("un module inconnu".into(), 0);
    }
    let mut nom = [0u16; 260];
    // SAFETY : tampon local de taille connue.
    let n = unsafe { GetModuleFileNameW(Some(module), &mut nom) } as usize;
    let chemin = String::from_utf16_lossy(&nom[..n.min(nom.len())]);
    let fichier = chemin.rsplit('\\').next().unwrap_or(&chemin).to_string();
    (fichier, module.0 as usize)
}

/// Lance l'instance de relève, qui attendra la mort de celle-ci.
fn relancer_apres_moi() {
    use std::os::windows::process::CommandExt;
    // Hors du job de l'application, qui tue ses enfants avec elle (voir
    // `app::spawn_replacement_process`).
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let lance = std::env::current_exe().and_then(|exe| {
        std::process::Command::new(exe)
            .env(ATTENDRE_PID, std::process::id().to_string())
            // Le levier ne se transmet pas : la relève planterait à son tour.
            .env_remove("NEURADESK_DEBUG_PLANTAGE_NATIF")
            .creation_flags(CREATE_BREAKAWAY_FROM_JOB)
            .spawn()
    });
    if let Err(e) = lance {
        noter(&format!("plantage natif : la relève n'a pas pu être lancée : {e}"));
    }
}

/// Dans une instance de RELÈVE (voir `relancer_apres_moi`) : attend que
/// l'instance plantée soit morte. À appeler avant `cef_app::init`. La variable
/// est retirée, pour que les sous-processus CEF n'en héritent pas.
pub fn attendre_si_demande() {
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    let Some(pid) = std::env::var(ATTENDRE_PID).ok().and_then(|s| s.trim().parse::<u32>().ok()) else {
        return;
    };
    std::env::remove_var(ATTENDRE_PID);
    // SAFETY : poignée ouverte puis fermée ici ; un PID déjà mort échoue à l'ouverture.
    unsafe {
        if let Ok(processus) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            let _ = WaitForSingleObject(processus, ATTENTE_MAX_MS);
            let _ = windows::Win32::Foundation::CloseHandle(processus);
        }
    }
    noter(&format!("relève de l'instance plantée {pid}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_erreurs_dxgi_et_d3d11_sont_des_pertes_du_gpu() {
        for code in [0x887A_0005u32, 0x887A_0006, 0x887A_0007, 0x887A_0020, 0x887C_0002] {
            let err: Box<dyn std::error::Error> =
                Box::new(windows::core::Error::from_hresult(windows::core::HRESULT(code as i32)));
            assert!(est_une_perte_du_gpu(err.as_ref()), "{code:#x}");
        }
    }

    #[test]
    fn les_autres_erreurs_ne_relancent_pas() {
        let acces: Box<dyn std::error::Error> =
            Box::new(windows::core::Error::from_hresult(windows::core::HRESULT(0x8007_0005u32 as i32)));
        assert!(!est_une_perte_du_gpu(acces.as_ref()), "E_ACCESSDENIED");
        let io: Box<dyn std::error::Error> = Box::new(std::io::Error::other("fichier"));
        assert!(!est_une_perte_du_gpu(io.as_ref()));
        let texte: Box<dyn std::error::Error> = "config illisible".into();
        assert!(!est_une_perte_du_gpu(texte.as_ref()));
    }

    #[test]
    fn trois_relances_en_cinq_minutes_puis_on_abandonne() {
        let (p1, h) = decider_la_relance(&[], 1000);
        let (p2, h) = decider_la_relance(&h, 1060);
        let (p3, h) = decider_la_relance(&h, 1120);
        let (p4, h) = decider_la_relance(&h, 1180);
        assert_eq!((p1, p2, p3, p4), (true, true, true, false));
        assert_eq!(h, vec![1000, 1060, 1120], "un refus n'entre pas dans l'historique");
        // La plus ancienne sort de la fenêtre à 1000 + 300 : une relance redevient permise.
        let (p5, h) = decider_la_relance(&h, 1300);
        assert!(p5);
        assert_eq!(h, vec![1060, 1120, 1300]);
    }

    #[test]
    fn une_date_dans_le_futur_est_ignoree() {
        // Horloge reculée : des dates « futures » ne doivent pas bloquer à jamais.
        let (permise, h) = decider_la_relance(&[5000, 5001, 5002], 1000);
        assert!(permise);
        assert_eq!(h, vec![1000]);
    }

    fn dossier(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nd-plantages-{nom}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dossier temporaire");
        d
    }

    #[test]
    fn une_entree_porte_la_date_le_pid_et_le_message() {
        assert_eq!(
            entree("2026-10-31 11:45:12", 9304, "arrêt normal"),
            "[2026-10-31 11:45:12] pid 9304 — arrêt normal\n"
        );
    }

    /// Le journal sert APRÈS coup : relancer l'application ne doit pas effacer
    /// la trace de la session morte.
    #[test]
    fn les_entrees_s_ajoutent_sans_effacer_les_precedentes() {
        let d = dossier("ajout");
        let f = d.join(NOM);
        ecrire_dans(&f, "a\n", TAILLE_MAX).expect("écriture 1");
        ecrire_dans(&f, "b\n", TAILLE_MAX).expect("écriture 2");
        assert_eq!(std::fs::read_to_string(&f).expect("lecture"), "a\nb\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn un_journal_trop_gros_passe_en_point_1_et_repart_vide() {
        let d = dossier("taille");
        let f = d.join(NOM);
        std::fs::write(&f, "x".repeat(20)).expect("préparation");
        ecrire_dans(&f, "neuf\n", 10).expect("écriture");
        assert_eq!(std::fs::read_to_string(&f).expect("lecture"), "neuf\n");
        assert_eq!(
            std::fs::read_to_string(d.join("neuradesk-plantages.1.log")).expect("ancien journal"),
            "x".repeat(20)
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// LE CAS QUI MOTIVE LE MODULE : une panique dans un fil de travail, sans
    /// `NEURADESK_DIAG`, doit laisser son message et son fil dans le journal.
    #[test]
    fn une_panique_dans_un_fil_est_ecrite_au_journal() {
        let f = chemin().expect("dossier de l'exécutable de test");
        installer();
        let marque = format!("panique d'essai {}", std::process::id());
        let m = marque.clone();
        let _ = std::thread::Builder::new()
            .name("essai-plantages".into())
            .spawn(move || panic!("{m}"))
            .expect("fil d'essai")
            .join();
        // Rend le hook par défaut aux autres tests du binaire.
        let _ = std::panic::take_hook();
        let texte = std::fs::read_to_string(&f).expect("journal écrit");
        assert!(
            texte.contains("PANIQUE dans le fil « essai-plantages » : ") &&texte.contains(&marque),
            "journal sans la panique : {}",
            &texte[texte.len().saturating_sub(600)..]
        );
    }

    #[test]
    fn une_date_locale_a_le_format_attendu() {
        let date = date_locale();
        assert_eq!(date.len(), 19, "{date}");
        assert_eq!(&date[4..5], "-");
        assert_eq!(&date[13..14], ":");
    }
}
