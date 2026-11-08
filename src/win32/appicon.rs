//! Résolution, lancement et focus des applications épinglées au dock
//! (icônes "W"/navigateur, "M"/musique, "C"/code — voir `ui::dock::Dock::new`)
//! — façon dock macOS de Seelen UI : clic sur une icône lance l'application
//! associée, ou passe simplement au premier plan la fenêtre existante si
//! elle tourne déjà, plutôt que d'en relancer une seconde instance.

use std::path::{Path, PathBuf};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{AssocQueryStringW, ShellExecuteW, ASSOCF_NONE, ASSOCSTR_EXECUTABLE};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOWNORMAL,
};

use super::app_identity::{self, WindowTarget};

/// Lance `path` en le DÉTACHANT du Job Object de NeuraDesk, avec
/// `args` optionnels. Retourne `false` si `CreateProcessW` a échoué — à
/// charge de l'appelant de se rabattre sur `ShellExecuteW`, qui sait
/// résoudre ce que `CreateProcessW` ignore (raccourcis `.lnk`, protocoles,
/// applications empaquetées).
///
/// POURQUOI CE DÉTOUR — DÉFAUT RÉEL, MESURÉ ET REPRODUIT SUR CETTE MACHINE.
/// `win32::cef_app::contain_child_processes` place NeuraDesk dans un Job
/// Object à `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, pour que ses sous-processus
/// CEF meurent avec lui même en cas d'arrêt brutal. Or **tout** processus que
/// NeuraDesk lance HÉRITE de ce Job : sans le drapeau `CREATE_BREAKAWAY_FROM_JOB`
/// ci-dessous, une application ouverte depuis le dock était donc TUÉE dès que
/// NeuraDesk s'arrêtait — que ce soit par plantage ou par fermeture normale.
///
/// Vérifié par comparaison A/B : un Brave lancé depuis le dock mourait à
/// CHAQUE arrêt de NeuraDesk, **sans le moindre vidage de crash** (il était
/// TERMINÉ par la fermeture du Job, pas planté — l'absence de trace avait
/// égaré le diagnostic pendant toute une session), tandis que le même Brave
/// lancé depuis l'Explorateur survivait. Le Job autorise déjà la sortie
/// (`JOB_OBJECT_LIMIT_BREAKAWAY_OK`, voir `contain_child_processes`) : il ne
/// manquait que cette demande, côté processus lancé.
fn create_process_detached(path: &Path, args: Option<&str>) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        CreateProcessW, ResumeThread, CREATE_BREAKAWAY_FROM_JOB, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
        PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, STARTUPINFOW,
    };

    // Un raccourci `.lnk` n'est PAS un exécutable : `CreateProcessW` le
    // refuse (mesuré — c'est ce qui a fait échouer la première version de ce
    // correctif, en repassant silencieusement par `ShellExecuteW`, donc dans
    // le Job). Or c'est le cas le PLUS COURANT : les épingles du dock et de
    // la barre des tâches sont des raccourcis. On le résout donc d'abord vers
    // sa cible, avec `apps::resolve_shortcut` — la même API que celle dont
    // l'Explorateur se sert pour ses info-bulles.
    let mut resolved: PathBuf = path.to_path_buf();
    let mut resolved_args: Option<String> = args.map(str::to_string);
    if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("lnk")) {
        let Some(shortcut) = super::apps::resolve_shortcut(path) else {
            return false;
        };
        let Some(target) = shortcut.target else {
            return false;
        };
        resolved = target;
        // Les arguments DÉCLARÉS par le raccourci s'ajoutent à ceux de
        // l'appelant : un `.lnk` peut en porter (c'est fréquent), et les
        // perdre lancerait l'application sans son contexte.
        let declared = shortcut.arguments.trim();
        resolved_args = match (declared.is_empty(), resolved_args) {
            (true, extra) => extra,
            (false, Some(extra)) if !extra.trim().is_empty() => Some(format!("{declared} {extra}")),
            (false, _) => Some(declared.to_string()),
        };
    }
    let is_firefox = resolved
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("firefox.exe"));
    let resolved_args = with_profile_chooser(&resolved, resolved_args, is_firefox && firefox_selector_enabled());
    // Un jeu reconnu démarre en fenêtre, à la taille de la zone centrale (voir
    // `win32::jeux_fenetres`) ; `jeu` : le dossier où guetter sa fenêtre.
    let (resolved_args, jeu) = match super::jeux_fenetres::preparer(&resolved, resolved_args.as_deref()) {
        Some((args, racine)) => (Some(args), Some(racine)),
        None => (resolved_args, None),
    };
    let args = resolved_args.as_deref();
    let path = resolved.as_path();

    // `CreateProcessW` attend une ligne de commande MUTABLE dont le PREMIER
    // jeton est le programme à lancer (il n'y a pas de `lpApplicationName`
    // ici : cf. l'appel, `PCWSTR::null()`). Le chemin est mis entre
    // guillemets — sans quoi une espace dans un chemin du type
    // `C:\Program Files\...` le couperait en deux.
    let mut cmdline: Vec<u16> = Vec::new();
    cmdline.push(b'"' as u16);
    cmdline.extend(path.as_os_str().encode_wide());
    cmdline.push(b'"' as u16);
    if let Some(args) = args {
        cmdline.push(b' ' as u16);
        cmdline.extend(args.encode_utf16());
    }
    cmdline.push(0);

    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    // ⭐ **CRÉÉ SUSPENDU QUAND L'INJECTION EST ACTIVE** (point 1 de l'effet
    // d'ouverture sans flash) : on charge `window-hook` AVANT que l'application
    // ne crée sa première fenêtre, pour que son crochet de `ShowWindow` la
    // voile à temps. Injection éteinte : lancement normal, chemin externe.
    let injecte = crate::win32::injection::actif();
    let suspendu = if injecte { CREATE_SUSPENDED } else { PROCESS_CREATION_FLAGS(0) };
    let mut info = PROCESS_INFORMATION::default();
    let launched = unsafe {
        CreateProcessW(
            PCWSTR::null(),
            Some(PWSTR(cmdline.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_BREAKAWAY_FROM_JOB | CREATE_UNICODE_ENVIRONMENT | suspendu,
            None,
            PCWSTR::null(),
            &startup,
            &mut info,
        )
    };
    if launched.is_err() {
        // Journal de diagnostic (voir `diag`, doc de module) : sans lui, un
        // échec de `CreateProcessW` est INVISIBLE — l'appelant se rabat
        // silencieusement sur `ShellExecuteW`, et le processus repart dans le
        // Job sans que rien ne le signale. Inerte sans `NEURADESK_DIAG`.
        crate::diag!(
            "lancement DETACHE refuse par CreateProcessW ({:?}) — repli ShellExecuteW (l'application restera dans le Job)",
            path.display()
        );
        return false;
    }
    crate::diag!("lancement DETACHE reussi : {}", path.display());
    if injecte {
        // Pendant qu'il est suspendu : on injecte, on laisse au crochet le
        // temps de se poser (la veille de la DLL l'installe dès sa première
        // itération), puis on reprend. Si l'injection échoue, on reprend quand
        // même — le processus repart simplement sur le chemin externe.
        crate::win32::injection::injecter_au_lancement(info.dwProcessId);
        std::thread::sleep(std::time::Duration::from_millis(25));
        unsafe {
            ResumeThread(info.hThread);
        }
    }
    if let Some(racine) = jeu {
        super::jeux_fenetres::placer_quand_elle_parait(racine);
    }
    // Vérification DIRECTE du détachement — c'est elle qui dit si le drapeau
    // a réellement pris, ou si le processus est malgré tout resté dans le Job
    // (auquel cas il sera tué avec NeuraDesk, exactement comme avant le
    // correctif). `IsProcessInJob` avec un Job NUL répond « appartient à UN
    // job », quel qu'il soit.
    {
        use windows::core::BOOL;
        use windows::Win32::System::JobObjects::IsProcessInJob;
        let mut in_job = BOOL(0);
        if unsafe { IsProcessInJob(info.hProcess, None, &mut in_job) }.is_ok() {
            crate::diag!("  -> toujours dans un job : {}", in_job.as_bool());
        }
    }
    // Les deux handles rendus ne servent à rien ici (on ne suit ni ne
    // contrôle le processus lancé) : ne pas les fermer les fuirait à chaque
    // clic sur une icône du dock.
    unsafe {
        let _ = CloseHandle(info.hProcess);
        let _ = CloseHandle(info.hThread);
    }
    true
}

/// Lance `path` sans arguments (voir `ShellExecuteW`, verbe "open" —
/// laisse Windows gérer l'élévation/l'association comme un double-clic
/// normal dans l'Explorateur).
///
/// Tente d'abord `create_process_detached` (voir sa doc : c'est ce qui
/// empêche l'application lancée d'être tuée avec NeuraDesk), et ne retombe
/// sur `ShellExecuteW` que pour ce que `CreateProcessW` ne sait pas résoudre.
pub fn launch(path: &Path) {
    if create_process_detached(path, None) {
        return;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide_null();
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Délai laissé à une fenêtre lancée par `launch_to_front` pour apparaître.
const LAUNCHED_WINDOW_WAIT: std::time::Duration = std::time::Duration::from_secs(5);
/// Cadence à laquelle `launch_to_front` guette cette fenêtre.
const LAUNCHED_WINDOW_POLL: std::time::Duration = std::time::Duration::from_millis(100);

/// Comme `launch`, puis amène au premier plan la PREMIÈRE fenêtre nouvelle de
/// ce même exécutable.
///
/// DÉFAUT RÉEL SIGNALÉ (2026-09-25) : « la fenêtre de sélecteur de profil
/// s'est affichée sous toutes les fenêtres ouvertes ». Le dock ne s'active
/// jamais (`WS_EX_NOACTIVATE`), donc Windows refuse le premier plan à ce qu'il
/// lance. ⛔ **`AllowSetForegroundWindow(ASFW_ANY)` avant le lancement a été
/// ESSAYÉ, et n'a RIEN changé** — Firefox passe par un processus lanceur qui
/// crée le vrai navigateur. Ce qui marche, c'est ce que fait déjà le clic
/// gauche (`focus_window`) : NeuraDesk vient de recevoir le clic, il peut donc
/// lui-même mettre une fenêtre au premier plan — il suffit d'attendre qu'elle
/// existe.
///
/// Une fenêtre qui n'apparaît pas dans `LAUNCHED_WINDOW_WAIT` est laissée
/// tranquille : rien n'est forcé après coup, sur une fenêtre que
/// l'utilisateur ne regarde peut-être plus.
pub fn launch_to_front(path: &Path) {
    let Some(exe) = path.file_name().map(|name| name.to_string_lossy().into_owned()) else {
        return launch(path);
    };
    let existing = windows_of(&exe);
    launch(path);
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let deadline = std::time::Instant::now() + LAUNCHED_WINDOW_WAIT;
        while std::time::Instant::now() < deadline {
            std::thread::sleep(LAUNCHED_WINDOW_POLL);
            if let Some(hwnd) = windows_of(&exe).into_iter().find(|hwnd| !existing.contains(hwnd)) {
                focus_window(HWND(hwnd as _));
                crate::diag!("fenetre lancee amenee au premier plan : {exe}");
                return;
            }
        }
        crate::diag!("aucune fenetre nouvelle de {exe} en {LAUNCHED_WINDOW_WAIT:?}");
    });
}

/// Les fenêtres ouvertes de l'exécutable `exe`, en entiers pour traverser les
/// fils (`HWND` n'est pas `Send`).
fn windows_of(exe: &str) -> Vec<isize> {
    app_identity::open_windows()
        .into_iter()
        .filter(|w| w.exe.eq_ignore_ascii_case(exe))
        .map(|w| w.hwnd.0 as isize)
        .collect()
}

/// Comme `launch`, mais avec élévation (verbe `"runas"` de `ShellExecuteW`
/// au lieu de `"open"`) — déclenche l'invite de contrôle de compte
/// utilisateur (UAC) de Windows, exactement comme "Exécuter en tant
/// qu'administrateur" dans le menu contextuel de l'Explorateur (la même
/// API, le même verbe). Si l'utilisateur refuse l'élévation dans l'invite
/// UAC, `ShellExecuteW` échoue silencieusement côté Windows lui-même — rien
/// de spécial à détecter ou gérer ici, ni à distinguer d'un simple clic sur
/// "Annuler".
pub fn launch_as_admin(path: &Path) {
    let wide: Vec<u16> = path.as_os_str().encode_wide_null();
    unsafe {
        ShellExecuteW(None, w!("runas"), PCWSTR(wide.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Comme `launch`, mais avec des arguments de ligne de commande (`lpParameters`
/// de `ShellExecuteW`, toujours `null` dans `launch`) — utilisé pour
/// `"--open-settings=<module>"` de PowerToys (voir `win32::dash::
/// open_powertoys_settings`), qui n'a pas d'équivalent accessible sans
/// passer d'argument à l'exécutable.
///
/// Même détachement du Job que `launch` — voir `create_process_detached`.
pub fn launch_with_args(path: &Path, args: &str) {
    if create_process_detached(path, Some(args)) {
        return;
    }
    let path_wide: Vec<u16> = path.as_os_str().encode_wide_null();
    let args_wide: Vec<u16> = args.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(path_wide.as_ptr()),
            PCWSTR(args_wide.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Ouvre `target` (URL, ou chemin virtuel `shell:...`) via le gestionnaire
/// associé de Windows plutôt qu'un exécutable direct.
pub fn open(target: &str) {
    let wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Comme `open`, mais avec élévation (verbe `"runas"`) — utilisé pour
/// élever une application UWP/Store (voir `win32::apps::AppLaunch::
/// launch_as_admin`), en appliquant `"runas"` directement au chemin
/// virtuel `shell:AppsFolder\<AppUserModelId>` déjà utilisé par `open` pour
/// le lancement normal. C'est exactement la technique que l'équipe Windows
/// Terminal a elle-même ajoutée à Windows Terminal pour son propre
/// "Exécuter en tant qu'administrateur" (`ShellExecuteEx`/`"runas"` sur un
/// chemin `shell:AppsFolder\...`, voir
/// https://github.com/microsoft/terminal/pull/14637). Windows sait élever
/// certaines applications empaquetées qui tournent en réalité comme un
/// vrai processus Win32 sous le capot (ex. Windows Terminal, PowerToys —
/// le "Desktop Bridge"), et refuse silencieusement pour les autres
/// (véritablement isolées en bac à sable AppContainer) — dans les deux
/// cas, `ShellExecuteW` ne renvoie ni erreur ni exception à gérer ici,
/// comme un refus d'élévation classique (voir `launch_as_admin`).
pub fn open_as_admin(target: &str) {
    let wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        ShellExecuteW(None, w!("runas"), PCWSTR(wide.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Ramène UNE fenêtre précise au premier plan (la dé-minimise si besoin).
///
/// Distinct de `focus_or`, et pas un paramètre de plus : `focus_or` répond à
/// « l'APPLICATION » (quelle fenêtre viser, et le cycle quand il y en a
/// plusieurs), alors qu'ici l'utilisateur a désigné une fenêtre PRÉCISE — il n'y
/// a rien à choisir, seulement à activer. C'est le geste du clic sur une
/// vignette d'aperçu (voir `win32::window_previews`).
pub fn focus_window(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Donne le focus à la bonne fenêtre de l'application `target`, ou appelle
/// `launch_fn` s'il n'y a aucune fenêtre à qui parler — évite d'ouvrir une
/// seconde instance d'une application déjà ouverte.
///
/// **DEUX GESTES, selon le nombre de fenêtres** — DEMANDE EXPLICITE : « au clic
/// sur le bouton d'une icône de programme ouvert je voudrais simplement que la
/// fenêtre prenne le focus ou que si plusieurs fenêtres sont ouvertes cycler
/// entre elles » :
///
/// 1. **une seule fenêtre** → elle prend le focus (restaurée si elle était
///    réduite) ;
/// 2. **plusieurs** → on CYCLE : si une des fenêtres de l'application est déjà
///    au premier plan, on passe à la SUIVANTE, en boucle ; sinon on prend la
///    première. Cliquer plusieurs fois fait donc le tour des fenêtres, comme le
///    bouton groupé de la barre des tâches Windows — et c'est ce qui rend le
///    cycle utile sans rien avoir à viser.
///
/// **Remplace le geste « réduire si déjà au premier plan »** qui vivait ici :
/// demandé plus tôt, il est devenu gênant dès lors que le clic doit servir à
/// cycler — sur une application à plusieurs fenêtres, réduire empêchait
/// justement d'atteindre la suivante.
///
/// **Le premier plan se lit sur `GetForegroundWindow`** : le dock porte
/// `WS_EX_NOACTIVATE` et ne prend donc JAMAIS le focus (voir
/// `win32::window::create_dock`), si bien que la fenêtre au premier plan reste
/// bien celle de l'application visée, même après un clic sur l'icône.
///
/// **La recherche elle-même n'est plus ici** : « quelle application est quelle
/// fenêtre » est une question à plusieurs réponses selon la nature de
/// l'application, et vit dans `win32::app_identity` (voir sa doc de module —
/// c'est ce qui permet à une épingle du Store d'être reconnue).
pub fn focus_or<F: FnOnce()>(target: &WindowTarget, launch_fn: F) {
    let mut windows: Vec<HWND> = app_identity::open_windows()
        .into_iter()
        .filter(|window| window.matches(target))
        .map(|window| window.hwnd)
        .collect();

    // ⚠️ **L'ORDRE DE `open_windows` N'EST PAS STABLE, ET LE CYCLE NE DOIT PAS EN
    // DÉPENDRE.** Il vient d'`EnumWindows`, donc du Z-ORDER : activer une fenêtre
    // la fait passer EN TÊTE. Un « suivant » calculé par POSITION dans cette liste
    // ping-pongue donc entre les deux fenêtres qui échangent les premières
    // places, et n'atteint JAMAIS les suivantes.
    //
    // DÉFAUT RÉEL SIGNALÉ : « quand je clique sur l'icône je ne cycle que sur
    // 2 fenêtres au lieu de toutes ». MESURÉ sur trois fenêtres Chrome : la liste
    // en contenait bien trois, `SetForegroundWindow` RÉUSSISSAIT à chaque clic
    // (ok=true), et le premier plan alternait entre les deux premières — la
    // troisième, toujours dernière en Z-order, n'a jamais été visée.
    //
    // On trie donc par HANDLE : il ne change pas tant que la fenêtre vit, donc
    // l'ordre est STABLE d'un clic à l'autre et le cycle fait le tour. C'est le
    // même piège que l'ordre des icônes du dock, déjà payé ailleurs dans ce
    // projet (voir `merge_dynamic_order`, `app::unlisted_open_apps`).
    cycle_order(&mut windows);

    let focused = unsafe { GetForegroundWindow() };
    let Some(next) = next_in_cycle(&windows, focused) else {
        return launch_fn();
    };
    focus_window(next);
}

/// L'ORDRE du cycle — **STABLE, donc indépendant de l'ordre d'entrée**.
///
/// `app_identity::open_windows` rend les fenêtres dans l'ordre d'`EnumWindows`,
/// c'est-à-dire du Z-ORDER : activer une fenêtre la fait passer en tête. Un cycle
/// qui suit cet ordre ping-pongue donc entre les deux fenêtres qui échangent les
/// premières places et n'atteint jamais les autres (voir `focus_or`, qui porte la
/// mesure).
///
/// Le tri se fait par HANDLE, qui ne change pas tant que la fenêtre vit : c'est
/// ce qui rend l'ordre reproductible d'un clic à l'autre. **Et c'est exactement
/// ce que vérifie son test** — la même liste donnée dans deux ordres différents
/// doit produire le même cycle, propriété qu'aucun test sur une liste fixe ne
/// pourrait éprouver.
fn cycle_order(windows: &mut [HWND]) {
    windows.sort_unstable_by_key(|hwnd| hwnd.0 as isize);
}

/// La fenêtre SUIVANTE dans le cycle, ou la PREMIÈRE si `focused` n'appartient
/// pas à la liste — c'est le cas « je clique alors que rien de l'application
/// n'est au premier plan », qui doit amorcer le cycle. `None` si la liste est
/// vide (l'appelant lance alors l'application).
///
/// **Fonction PURE, sans aucun appel Win32** : c'est là que vivait le défaut, et
/// c'est ce qui la rend testable sans fenêtre.
fn next_in_cycle(windows: &[HWND], focused: HWND) -> Option<HWND> {
    let first = *windows.first()?;
    Some(
        windows
            .iter()
            .position(|hwnd| *hwnd == focused)
            .map(|index| windows[(index + 1) % windows.len()])
            .unwrap_or(first),
    )
}

/// Petit raccourci pour convertir un `&OsStr` en buffer UTF-16
/// nul-terminé, sans dépendance directe à `std::os::windows::ffi` dans le
/// reste du fichier.
/// Drapeau de Firefox qui fait sauter le RELAIS vers l'instance déjà ouverte.
const FIREFOX_NEW_INSTANCE: &str = "--new-instance";

/// **FONCTION PURE, ET TESTÉE** : les arguments de lancement, complétés de
/// `--new-instance` pour un Firefox lancé sans profil explicite, quand son
/// sélecteur de profils est activé.
///
/// DEMANDE EXPLICITE (2026-09-25) : « il faudrait toujours lancer firefox avec
/// profilmanager sinon je ne peux pas choisir le profil pour le second
/// firefox ».
///
/// ⛔ **PAS `-ProfileManager`** — essayé d'abord, et refusé par l'utilisateur
/// (« firefox ne me propose pas la bonne fenêtre de profil ») : c'est l'ANCIEN
/// gestionnaire, qui ignore les profils à avatars (ils vivent dans
/// `Profile Groups\<StoreID>.sqlite`, pas dans `profiles.ini`). Et en le
/// fermant, il a écrit `StartWithLastProfile=0` dans `profiles.ini`.
///
/// Le NOUVEAU sélecteur (`about:profilemanager`) n'a aucun drapeau à lui.
/// D'après `nsAppRunner.cpp` (`XREMain::XRE_mainStartup`), l'ordre est le
/// suivant :
/// 1. le choix du profil ;
/// 2. le RELAIS de la ligne de commande vers une instance déjà ouverte ;
/// 3. le sélecteur, si le profil par défaut porte `ShowSelector=1` ;
/// 4. le verrou du profil.
///
/// Un second lancement meurt donc à l'étape 2 : sa ligne de commande part vers
/// l'instance ouverte. `--new-instance` saute ce relais, et c'est tout.
///
/// D'où la CONDITION `selector_enabled` (voir `firefox_selector_enabled`) :
/// sans sélecteur, `--new-instance` irait droit au verrou du profil déjà
/// ouvert, et Firefox afficherait « déjà en cours d'exécution ».
///
/// Un lancement qui désigne déjà son profil (`-P nom`, `--profile chemin`)
/// n'est pas touché : c'est le geste d'une icône de profil, qui sait où aller.
fn with_profile_chooser(path: &Path, args: Option<String>, selector_enabled: bool) -> Option<String> {
    let is_firefox = path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("firefox.exe"));
    if !is_firefox || !selector_enabled {
        return args;
    }
    let chooses_profile = args.as_deref().is_some_and(|args| {
        args.split_whitespace().any(|token| {
            let flag = token.trim_start_matches('-').to_ascii_lowercase();
            token.starts_with('-') && matches!(flag.as_str(), "p" | "profile" | "profilemanager" | "new-instance")
        })
    });
    if chooses_profile {
        return args;
    }
    Some(match args.as_deref().map(str::trim) {
        Some(args) if !args.is_empty() => format!("{args} {FIREFOX_NEW_INSTANCE}"),
        _ => FIREFOX_NEW_INSTANCE.to_string(),
    })
}

/// **FONCTION PURE, ET TESTÉE** : vrai si `profiles.ini` active le sélecteur de
/// profils de Firefox (`ShowSelector=1`, posé sur le profil d'un GROUPE — la
/// case « Choisir un profil au démarrage » de Firefox).
///
/// ⚠️ `StartWithLastProfile=0` fait passer l'ANCIEN gestionnaire AVANT lui
/// (`nsAppRunner.cpp`, même fonction) : c'est à Firefox de le régler, pas à nous.
fn profiles_ini_shows_selector(ini: &str) -> bool {
    ini.lines().any(|line| {
        line.split_once('=')
            .is_some_and(|(key, value)| key.trim().eq_ignore_ascii_case("ShowSelector") && value.trim() == "1")
    })
}

/// `profiles_ini_shows_selector` sur le `profiles.ini` de cet utilisateur —
/// relu à chaque lancement (un clic), puisque la case se coche dans Firefox.
fn firefox_selector_enabled() -> bool {
    std::env::var_os("APPDATA")
        .map(|dir| PathBuf::from(dir).join(r"Mozilla\Firefox\profiles.ini"))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|ini| profiles_ini_shows_selector(&ini))
}

trait EncodeWideNull {
    fn encode_wide_null(&self) -> Vec<u16>;
}

impl EncodeWideNull for std::ffi::OsStr {
    fn encode_wide_null(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(std::iter::once(0)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firefox_skips_the_relay_to_reach_its_profile_selector() {
        let firefox = Path::new(r"C:\Program Files\Mozilla Firefox\firefox.exe");
        assert_eq!(with_profile_chooser(firefox, None, true).as_deref(), Some("--new-instance"));
        assert_eq!(with_profile_chooser(firefox, Some("-new-window".into()), true).as_deref(), Some("-new-window --new-instance"));
        assert_eq!(with_profile_chooser(firefox, Some(r#"--profile "C:\p""#.into()), true).as_deref(), Some(r#"--profile "C:\p""#));
        assert_eq!(with_profile_chooser(firefox, Some("-P claude".into()), true).as_deref(), Some("-P claude"));
        assert_eq!(with_profile_chooser(firefox, Some("--ProfileManager".into()), true).as_deref(), Some("--ProfileManager"));
        // Sans sélecteur, `--new-instance` buterait sur le verrou du profil ouvert.
        assert_eq!(with_profile_chooser(firefox, None, false), None);
        let chrome = Path::new(r"C:\x\chrome.exe");
        assert_eq!(with_profile_chooser(chrome, None, true), None);
    }

    /// Le `profiles.ini` MESURÉ le 2026-09-25 (profil de groupe, case cochée).
    #[test]
    fn profiles_ini_shows_selector_reads_the_group_profile_flag() {
        let ini = "[General]\r\nStartWithLastProfile=1\r\n\r\n[Profile0]\r\nName=default-release\r\nStoreID=fcab863e\r\nShowSelector=1\r\n";
        assert!(profiles_ini_shows_selector(ini));
        assert!(!profiles_ini_shows_selector(&ini.replace("ShowSelector=1", "ShowSelector=0")));
        assert!(!profiles_ini_shows_selector("[Profile0]\nName=default\n"));
    }

    /// Trois fenêtres d'essai — des handles FACTICES, jamais déréférencés : tout
    /// ce qui est éprouvé ici est pur.
    fn three() -> (HWND, HWND, HWND) {
        (HWND(1 as *mut _), HWND(2 as *mut _), HWND(3 as *mut _))
    }

    /// **LE TEST DU DÉFAUT, ET IL PORTE SUR LA BONNE PROPRIÉTÉ.** Un test qui
    /// donnerait une liste FIXE à `next_in_cycle` passerait aussi sur l'ancien
    /// code : ce qui était cassé n'est pas l'arithmétique du cycle, c'est sa
    /// dépendance à l'ORDRE D'ENTRÉE. La même liste, donnée dans deux ordres
    /// différents — ce qui arrivait d'un clic à l'autre, `EnumWindows` rendant le
    /// Z-order courant — doit produire le MÊME cycle.
    #[test]
    fn the_cycle_order_does_not_depend_on_the_z_order_it_was_given() {
        let (a, b, c) = three();
        let mut premier = vec![a, b, c];
        let mut second = vec![b, a, c];
        let mut troisieme = vec![c, b, a];
        cycle_order(&mut premier);
        cycle_order(&mut second);
        cycle_order(&mut troisieme);
        assert_eq!(premier, second, "l'ordre du cycle ne doit pas dependre du Z-order");
        assert_eq!(second, troisieme, "ni d'aucune autre permutation");
        assert_eq!(premier, vec![a, b, c], "et il doit etre celui des handles");
    }

    /// Et le cycle fait bien le TOUR COMPLET depuis n'importe où — c'est ce que
    /// l'utilisateur constate : trois fenêtres, trois clics, trois titres.
    #[test]
    fn the_cycle_visits_every_window_exactly_once_per_turn() {
        let (a, b, c) = three();
        let windows = [a, b, c];
        for depart in windows {
            let mut vues = vec![depart];
            let mut courant = depart;
            for _ in 0..windows.len() - 1 {
                courant = next_in_cycle(&windows, courant).expect("une fenetre suivante");
                assert!(!vues.contains(&courant), "une fenetre revue avant le tour complet");
                vues.push(courant);
            }
            for fenetre in windows {
                assert!(vues.contains(&fenetre), "le cycle depuis {depart:?} a manque {fenetre:?}");
            }
            assert_eq!(next_in_cycle(&windows, courant), Some(depart), "le cycle boucle");
        }
    }

    #[test]
    fn the_cycle_starts_at_the_first_window_when_none_is_in_front() {
        let (a, b, c) = three();
        // Aucune des trois n'est au premier plan (l'utilisateur a cliqué en
        // venant d'ailleurs) : on amorce par la première.
        assert_eq!(next_in_cycle(&[a, b, c], HWND(99 as *mut _)), Some(a));
        // Une seule fenêtre retombe sur elle-même — le cas « une fenêtre » n'a
        // donc pas besoin d'être traité à part.
        assert_eq!(next_in_cycle(&[a], a), Some(a));
        // Liste vide : l'appelant lance l'application.
        assert_eq!(next_in_cycle(&[], a), None);
        let _ = (b, c);
    }
}
