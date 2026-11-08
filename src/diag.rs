//! Journal de diagnostic du dock — ACTIVÉ PAR VARIABLE D'ENVIRONNEMENT
//! (`NEURADESK_DIAG=1`), donc totalement inerte en usage normal.
//!
//! POURQUOI CE MODULE : le plantage qui a motivé son écriture ne laisse
//! AUCUNE trace exploitable. L'application est compilée avec
//! `panic = "abort"` (voir `Cargo.toml`, `[profile.release]`), donc un index
//! hors limites tue le processus sur-le-champ ; et elle est en sous-système
//! `WINDOWS_GUI`, donc sans console — son `stderr` part dans le vide tant
//! qu'il n'est pas explicitement redirigé. Le vidage Windows (`CrashDumps`)
//! ne contient que la pile machine, sans aucun symbole de CE code (`strip =
//! true`), ce qui a déjà coûté une session entière d'analyse.
//!
//! D'où ce journal : un FICHIER, horodaté, où l'application note ce qu'elle
//! traite. Après un plantage, les dernières lignes écrites disent ce qui était
//! en cours — c'est exactement ce que ni le vidage ni `stderr` ne donnaient.
//!
//! COÛT — n'écrit QUE si `NEURADESK_DIAG` est positionnée et différente de
//! `"0"`. Le `format!` lui-même est conditionné (voir la macro `diag!`), donc
//! le désactiver ne coûte pas même une allocation. Il ne faut PAS en mettre
//! dans la boucle de rendu (60 Hz) : la cible, ce sont les changements d'état
//! (cadence 1 Hz, actions, ouvertures de panneaux), pas chaque image.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// `true` dès que `init()` a ouvert le fichier — lu par la macro `diag!`
/// AVANT de formater quoi que ce soit.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Fichier de sortie, ouvert une fois pour toutes par `init()`.
///
/// `Mutex` plutôt qu'un `RefCell` : contrairement au reste de ce projet (dont
/// l'état vit sur le thread principal), les lignes de journal peuvent venir de
/// n'importe quel thread — ne serait-ce que du `panic` d'un thread de travail.
static LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// Instant du démarrage, pour des horodatages RELATIFS (« +12.345 s »).
///
/// Relatif et non absolu à dessein : ce qui compte ici est l'ORDRE et
/// l'écart entre événements, pas l'heure de la journée — et un relatif évite
/// toute dépendance à `GetLocalTime` dans un chemin appelé en situation de
/// plantage.
static START: OnceLock<Instant> = OnceLock::new();

/// Vrai si le journal est actif — consulté par la macro `diag!`.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Ouvre le journal si `NEURADESK_DIAG` est positionnée, et installe un
/// `panic` hook qui écrit la trace AVANT que `abort` ne tue le processus.
///
/// À appeler UNE fois, au début de `main()` (voir son appel).
pub fn init() {
    let wanted = std::env::var("NEURADESK_DIAG")
        .map(|v| v != "0" && !v.is_empty())
        .unwrap_or(false);
    if !wanted {
        return;
    }

    let path = log_path();
    // **ROTATION AU LIEU DE TRONCATURE, ET C'EST UNE LEÇON PAYÉE DEUX FOIS.**
    //
    // Le journal était RECRÉÉ à chaque lancement : celui d'un plantage était donc
    // TOUJOURS perdu dès qu'on relançait l'application pour la remettre en
    // route — c'est-à-dire dans le seul cas où on en a besoin. Constaté les deux
    // fois pendant l'enquête du 2026-09-23 (plantages CEF de 03:30 et 08:26) :
    // les dernières lignes de la session morte n'étaient lisibles QUE tant qu'on
    // n'avait pas relancé, ce qui oblige à lire le journal avant de rendre son
    // dock à l'utilisateur. Une rotation garde la session précédente sans rien
    // changer à la lecture courante.
    rotate_previous(&path);
    let file = match std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(&path)
    {
        Ok(file) => file,
        Err(err) => {
            eprintln!("[neuradesk][diag] journal indisponible ({}): {err}", path.display());
            return;
        }
    };

    let _ = START.set(Instant::now());
    if let Ok(mut guard) = LOG.lock() {
        *guard = Some(file);
    }
    ENABLED.store(true, Ordering::Relaxed);

    // Le `panic` hook par défaut écrit sur `stderr` — donc nulle part ici.
    // On l'enchaîne APRÈS le nôtre : le message reste écrit dans le journal
    // même quand le processus n'a aucune console.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log_line(&format!("=========== PANIC ==========="));
        log_line(&format!("{info}"));
        // `force_capture` : la pile est capturée même sans `RUST_BACKTRACE`.
        log_line(&format!("{}", std::backtrace::Backtrace::force_capture()));
        log_line("========= FIN PANIC =========");
        previous(info);
    }));

    log_line(&format!("=== journal de diagnostic ouvert : {} ===", path.display()));
    log_line(&format!("pid {} — NEURADESK_DIAG=1", std::process::id()));
}

/// Écrit UNE ligne horodatée. Appelée par la macro `diag!`, ou directement
/// depuis le `panic` hook (qui ne peut pas passer par la macro).
pub fn log_line(message: &str) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let stamp = START.get().map(|start| start.elapsed().as_secs_f64()).unwrap_or(0.0);
    // `lock()` peut échouer si un autre thread a paniqué EN TENANT le verrou :
    // dans ce cas on abandonne la ligne plutôt que de paniquer à notre tour
    // (et masquer le vrai plantage).
    if let Ok(mut guard) = LOG.lock() {
        if let Some(file) = guard.as_mut() {
            let _ = writeln!(file, "[{stamp:>10.3}] {message}");
            // Sans `flush`, une écriture tamponnée serait PERDUE au plantage —
            // exactement le cas d'usage de ce journal.
            let _ = file.flush();
        }
    }
}

/// Nombre de journaux PRÉCÉDENTS conservés (voir `rotate_previous`).
const ROTATIONS: usize = 3;

/// Renomme le journal existant en `neuradesk-diag.1.log` et fait glisser les
/// précédents — `.1` est donc toujours la session IMMÉDIATEMENT précédente,
/// `.ROTATIONS` la plus ancienne gardée. Le plus ancien est supprimé.
///
/// Best-effort de bout en bout : un journal introuvable, verrouillé (OneDrive,
/// antivirus) ou non renommable ne doit JAMAIS empêcher l'application de
/// démarrer ni l'écriture du journal courant.
fn rotate_previous(path: &std::path::Path) {
    // Un lancement qui n'a rien écrit ne mérite pas de place.
    if !std::fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false) {
        return;
    }
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let named = |n: usize| dir.join(format!("neuradesk-diag.{n}.log"));
    // Du plus ANCIEN au plus récent, pour ne jamais écraser une rotation qu'on
    // veut garder : `.2` devient `.3` AVANT que `.1` ne devienne `.2`.
    let _ = std::fs::remove_file(named(ROTATIONS));
    for n in (1..ROTATIONS).rev() {
        let from = named(n);
        if from.exists() {
            let _ = std::fs::rename(&from, named(n + 1));
        }
    }
    let _ = std::fs::rename(path, named(1));
}

/// Emplacement du journal : à côté de l'exécutable, jamais dans le dossier
/// courant (qui dépend de la façon dont l'application a été lancée).
fn log_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("neuradesk-diag.log")))
        .unwrap_or_else(|| std::path::PathBuf::from("neuradesk-diag.log"))
}

/// Note une ligne au journal, SANS RIEN FORMATER si le journal est inactif —
/// c'est ce qui rend la macro utilisable dans du code appelé souvent.
#[macro_export]
macro_rules! diag {
    ($($arg:tt)*) => {
        if $crate::diag::enabled() {
            $crate::diag::log_line(&::std::format!($($arg)*));
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **LA ROTATION GARDE LES TROIS DERNIÈRES SESSIONS, DANS L'ORDRE.**
    ///
    /// C'est la garantie qui rend un plantage analysable : sans elle, relancer
    /// l'application pour la remettre en route efface le journal de la session
    /// morte — c'est-à-dire le seul cas où on en a besoin. Constaté deux fois le
    /// 2026-09-23, pendant l'enquête sur les plantages CEF.
    ///
    /// Le glissement `.2`→`.3` AVANT `.1`→`.2` est la partie qui se trompe
    /// facilement (l'ordre inverse écrase une session qu'on veut garder), d'où
    /// ce test : il écrit QUATRE sessions et vérifie laquelle tombe.
    #[test]
    fn la_rotation_garde_les_trois_dernieres_sessions() {
        let dir = std::env::temp_dir().join(format!("nd-diag-rotation-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dossier temporaire");
        let log = dir.join("neuradesk-diag.log");
        let named = |n: usize| dir.join(format!("neuradesk-diag.{n}.log"));

        for session in 1..=4 {
            std::fs::write(&log, format!("session {session}")).expect("ecriture");
            rotate_previous(&log);
        }

        assert_eq!(std::fs::read_to_string(named(1)).unwrap(), "session 4", "`.1` est la session IMMEDIATEMENT precedente");
        assert_eq!(std::fs::read_to_string(named(2)).unwrap(), "session 3");
        assert_eq!(std::fs::read_to_string(named(3)).unwrap(), "session 2");
        assert!(!named(4).exists(), "on n'en garde que {ROTATIONS}");
        assert!(!log.exists(), "le journal courant a bien ete renomme, pas copie");

        // Un lancement qui n'a RIEN ecrit ne consomme pas de place : sinon
        // relancer dix fois l'application chasserait la session interessee.
        std::fs::write(&log, "").expect("ecriture");
        rotate_previous(&log);
        assert_eq!(
            std::fs::read_to_string(named(1)).unwrap(),
            "session 4",
            "un journal VIDE ne doit pas decaler la rotation"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
