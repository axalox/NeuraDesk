//! Intégration CEF (Chromium Embedded Framework) en rendu hors-écran (OSR)
//! accéléré — Phase 0 de `docs/plans/cef-web-ui-migration.md`, à consulter
//! pour le contexte complet (pourquoi CEF OSR, pas WebView2 ; comment la
//! texture partagée D3D11 s'intègre au pipeline de rendu existant).
//!
//! Deux pièges trouvés dans le spike de cette migration (`C:\dev\cef-spike`,
//! hors de ce repo), tous deux des plantages natifs SILENCIEUX
//! (`STATUS_BREAKPOINT`/0x80000003, PAS une exception normale — jamais
//! journalisé par Windows Error Reporting, aucune sortie console) :
//! 1. `cef::api_hash` DOIT être le TOUT PREMIER appel à l'API CEF, avant
//!    même `Args::new()` — voir `dispatch_subprocess`, appelée en tout
//!    premier dans `main()`.
//! 2. La distribution CEF téléchargée est compilée AVEC le bac à sable
//!    Chromium — sans `--no-sandbox` injecté via
//!    `on_before_command_line_processing` (PAS `Args::as_cmd_line()`, qui
//!    n'a aucun effet), même plantage silencieux. Voir `NeuraDeskCefApp`
//!    ci-dessous.

// Import global (`cef::*`) — voir le spike de cette migration : les macros
// `wrap_app!`/`wrap_client!`/`wrap_render_handler!` exigent en scope des
// traits générés (`ImplApp`/`WrapApp`/`ImplClient`/`WrapClient`/
// `ImplRenderHandler`/`WrapRenderHandler`...) qu'il est plus fiable
// d'importer TOUS que d'énumérer un par un (essayé d'abord dans le spike,
// oubliés systématiquement).
use cef::{args::Args, *};
use std::cell::RefCell;
use std::rc::Rc;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11ShaderResourceView, ID3D11Texture2D};
use windows::Win32::System::LibraryLoader::LoadLibraryW;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject, JobObjectExtendedLimitInformation,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::GetCurrentProcess;

/// Dispatch les sous-process CEF (rendu/GPU/réseau/utilitaire — voir la doc
/// de module de `docs/plans/cef-web-ui-migration.md`, "architecture
/// multi-process") : le MÊME exe est ré-exécuté par CEF avec un argument
/// `--type=...` pour chacun. Appelée en tout premier dans `main()`, AVANT
/// TOUT AUTRE CODE (voir le piège n°1 de la doc de module) — si `false`
/// est retourné, `main()` doit s'arrêter immédiatement : ce process EST un
/// sous-process CEF, il ne doit JAMAIS créer la fenêtre du dock.
///
/// Retourne aussi l'objet `App` créé (à réutiliser tel quel pour
/// `cef::initialize`, voir `init` plus bas) : `execute_process`/
/// `initialize` doivent recevoir le MÊME `App`, pas deux instances
/// séparées, sous peine de perdre `on_before_command_line_processing` côté
/// `initialize` (jamais constaté en pratique ici, mais c'est le
/// comportement documenté par l'exemple officiel `cef-rs`, voir sa doc de
/// module).
pub fn dispatch_subprocess() -> Option<(Args, cef::App)> {
    // Voir le piège n°1 de la doc de module — DOIT précéder `Args::new()`.
    let _ = api_hash(cef::sys::CEF_API_VERSION_LAST, 0);

    let args = Args::new();
    let cmd = args.as_cmd_line().unwrap();
    let switch = CefString::from("type");
    let is_browser_process = cmd.has_switch(Some(&switch)) != 1;

    let mut app = NeuraDeskCefAppBuilder::new(NeuraDeskCefApp {});
    let ret = execute_process(Some(args.as_main_args()), Some(&mut app), std::ptr::null_mut());

    if is_browser_process {
        assert_eq!(ret, -1, "CEF: échec du dispatch dans le process navigateur");
        Some((args, app))
    } else {
        // Sous-process CEF : `ret` est son code de sortie réel, à propager
        // tel quel — jamais de fenêtre de dock créée depuis ce chemin (voir
        // la doc de fonction).
        std::process::exit(ret);
    }
}

/// Garantit que TOUS les sous-process de ce process (les sous-process CEF —
/// rendu/GPU/réseau/utilitaire, voir la doc de module — mais aussi
/// n'importe quel autre processus enfant déjà lancé par ce projet, ex.
/// les shells du panneau Terminal, voir `win32::pty`) meurent avec lui, MÊME s'il est
/// arrêté brutalement (crash, Gestionnaire des tâches, `taskkill /F`) —
/// PAS seulement à une sortie propre (`cef_shutdown`, best-effort comme le
/// reste des nettoyages de fin d'`app::run`, sans garantie si le process
/// ne sort jamais proprement).
///
/// BUG RÉEL CONSTATÉ EN PRATIQUE PENDANT CETTE MIGRATION (voir
/// `docs/plans/cef-web-ui-migration.md`) : un sous-process CEF zombie a
/// verrouillé le fichier exe après un `taskkill /F` du process principal
/// pendant les tests — `--no-sandbox` (voir `NeuraDeskCefApp`) désactive
/// aussi une partie de la gestion de conteneurs de processus interne à
/// Chromium normalement liée au bac à sable, laissant les sous-process
/// orphelins livrés à eux-mêmes. Un "Job Object" Windows avec
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` règle ça INDÉPENDAMMENT de CEF :
/// Windows tue lui-même tout processus du job dès que le DERNIER handle
/// vers le job se ferme — ce qui arrive automatiquement à la sortie de CE
/// process, propre OU non (crash, kill externe compris), sans dépendre
/// d'un quelconque code de nettoyage qui aurait besoin de s'exécuter. Les
/// sous-process CEF (créés par CE process via `CreateProcess`) héritent
/// automatiquement de l'appartenance au job de leur parent, sans rien à
/// faire de plus côté CEF.
///
/// À appeler tôt dans `main()`, avant la création de tout sous-process
/// (donc avant `cef_app::init`) — best-effort : si `CreateJobObjectW`/
/// `AssignProcessToJobObject` échouent (improbable), l'app démarre quand
/// même, simplement sans cette garantie supplémentaire.
///
/// BUG RÉEL SIGNALÉ ("le clic sur recharger neuradesk a fermé l'app mais ne
/// l'a pas relancée", voir `app::spawn_replacement_process`) : sans
/// `JOB_OBJECT_LIMIT_BREAKAWAY_OK`, un process enfant hérite
/// automatiquement de l'appartenance à CE job (comportement voulu pour les
/// sous-process CEF, voir doc ci-dessus) — MAIS le nouveau process
/// NeuraDesk lancé par "Recharger" est lui aussi un enfant (`Command::new
/// (current_exe()).spawn()`), donc lui aussi assigné au même job par
/// défaut. Dès que CE process quitte (pour laisser place au nouveau), le
/// job se ferme et `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` tue tout ce qu'il
/// contient encore — y compris la nouvelle instance, avant même qu'elle
/// ait fini de démarrer. `BREAKAWAY_OK` (plutôt que la variante `SILENT_`)
/// exige `CREATE_BREAKAWAY_FROM_JOB` explicite côté appelant pour qu'un
/// enfant échappe au job — voir `app::spawn_replacement_process`, seul
/// endroit qui le demande. Les sous-process CEF, eux, restent contenus
/// sans rien changer : c'est CEF/Chromium qui les crée en interne, jamais
/// avec ce drapeau.
pub fn contain_child_processes() {
    unsafe {
        let Ok(job) = CreateJobObjectW(None, None) else {
            // `diag!` et non `eprintln!` : l'application est lancée détachée et sans console,
            // donc `stderr` ne va NULLE PART (voir `diag`, doc de module). `diag::init` est
            // appelé avant cette fonction (voir `main`), donc la ligne arrive bien au journal.
            crate::diag!("[cef] CreateJobObjectW a echoue — sous-process non contenus en cas d'arret brutal");
            return;
        };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
        let set_ok = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if set_ok.is_err() || AssignProcessToJobObject(job, GetCurrentProcess()).is_err() {
            crate::diag!("[cef] configuration du Job Object echouee — sous-process non contenus en cas d'arret brutal");
            return;
        }
        // `job` DOIT rester ouvert pour toute la durée de vie du process —
        // `windows::Win32::Foundation::HANDLE` n'implémente PAS `Drop` (une
        // simple valeur `Copy`, pas de fermeture automatique en sortie de
        // portée comme `OwnedHandle`), donc RIEN à faire ici pour ça : le
        // simplement laisser sortir de portée ne le ferme PAS. Windows ferme
        // ce handle lui-même à la sortie du process, quelle qu'en soit la
        // cause (propre ou non) — c'est justement CETTE fermeture qui
        // déclenche `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
    }
}

/// Initialise CEF pour de bon — à appeler UNE SEULE FOIS dans le process
/// navigateur, après `dispatch_subprocess`, avant toute création de fenêtre
/// CEF (`CefOverlay::new`). `app` : celui retourné par `dispatch_subprocess`
/// (voir sa doc).
/// **LA LANGUE DE L'INTERFACE DE WINDOWS**, au format que CEF attend
/// (`"fr-FR"`). Rend `"fr-FR"` en repli si Windows ne répond pas — jamais une
/// chaîne vide : CEF reprendrait alors l'anglais, c'est-à-dire le défaut qu'on
/// corrige.
fn langue_du_systeme() -> String {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buffer = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
    let n = unsafe { GetUserDefaultLocaleName(&mut buffer) };
    if n <= 1 {
        return "fr-FR".to_string();
    }
    String::from_utf16_lossy(&buffer[..(n as usize - 1)])
}

pub fn init(args: &Args, app: &mut cef::App) {
    // ⚠️ **LA LANGUE DE CEF N'EST PAS DÉCORATIVE** — mesuré à la capture le
    // 2026-09-24 : sans elle, les champs `<input type="date">` et
    // `<input type="time">` du dash Notes se rendaient au format AMÉRICAIN
    // (`09/25/2026`, `03:30 PM`), dans une application dont tout le reste est en
    // français.
    // ⛔ **ET `lang="fr"` SUR LA PAGE N'Y CHANGE RIEN** : Chromium suit la langue
    // de SON interface, pas l'attribut de l'élément — l'attribut a été posé, la
    // capture l'a démenti. C'est donc ici, et nulle part ailleurs.
    // On lit celle de WINDOWS plutôt que d'écrire « fr » en dur : l'application
    // n'a pas à décider à la place du système.
    // ⛔ **LE CACHE DE CEF DOIT ÊTRE LE NÔTRE — MESURÉ LE 2026-10-28 SUR LA VM
    // WINDOWS 10, ET C'EST CE QUI A EMPÊCHÉ TOUT REDÉMARRAGE.** Sans
    // `root_cache_path`, CEF retombe sur `%LOCALAPPDATA%\CEF\User Data` (son
    // propre en-tête le documente) et y pose un **verrou de singleton** partagé
    // par TOUTES les applications CEF de la machine. Le crash d'une version
    // précédente y a laissé ce verrou : chaque lancement suivant a rendu `0`,
    // donc « NeuraDesk tourne déjà » — alors qu'aucun processus ne tournait — et
    // le verrou ne se libère pas tout seul (il a fallu le chercher).
    //
    // Notre dossier est nommé, isolé, et effaçable sans toucher à la
    // configuration : `%LOCALAPPDATA%\NeuraDesk\cef` (voir `config::cache_dir`).
    // ⛔ **LE RUNTIME VC++ D'ABORD, PARCE QUE SON ABSENCE NE DIT RIEN.**
    // Mesuré le 2026-10-28 sur une VM Windows 10 22H2 sans Visual Studio : sans
    // `msvcp140.dll`, `libcef.dll` ne se charge pas, `CefInitialize` rend 0, et
    // le seul message qu'on en pouvait tirer (« NeuraDesk tourne deja ») était
    // FAUX. Depuis le 2026-10-31, l'archive portable livre `msvcp140.dll` (et
    // `vcruntime140_1.dll`, qu'il importe) à côté de l'exe, et ce chargement les
    // y trouve ; ici, on NOMME le manque si l'un d'eux a disparu. ⚠️ Aucun binaire
    // de l'archive n'importe `msvcp140.dll` (tables d'imports lues le même jour) :
    // la cause de 2026-10-28 n'est pas reprouvée. Best-effort : on continue, CEF
    // dira le reste.
    if unsafe { LoadLibraryW(windows::core::w!("msvcp140.dll")) }.is_err() {
        crate::diag!(
            "runtime Visual C++ INTROUVABLE (msvcp140.dll) — installer « Visual C++ 2015-2022 x64 » \
             (vc_redist.x64.exe) ; sans lui, libcef.dll ne se charge pas et NeuraDesk ne peut pas demarrer"
        );
    }

    let cache = crate::config::cache_dir().map(|d| d.join("cef"));
    let cache_texte = cache.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let settings = Settings {
        windowless_rendering_enabled: true as _,
        external_message_pump: true as _,
        locale: cef::CefString::from(langue_du_systeme().as_str()),
        root_cache_path: cef::CefString::from(cache_texte.as_str()),
        ..Default::default()
    };
    let ok = initialize(Some(args.as_main_args()), Some(&settings), Some(app), std::ptr::null_mut());
    // Un `0` EST le cas d'un second lancement (verrou pris, le lancement est
    // transmis à l'instance qui tourne — voir `on_already_running_app_relaunch`),
    // mais c'est aussi le code d'un ÉCHEC d'initialisation : le message nomme les
    // deux, et le dossier où le verrou se trouve.
    if ok != 1 {
        let ou = if cache_texte.is_empty() { "%LOCALAPPDATA%\\CEF\\User Data (défaut de CEF)".to_string() } else { cache_texte };
        crate::diag!(
            "CefInitialize a rendu 0 — instance déjà lancée, OU échec d'initialisation. \
             Verrou de singleton dans {ou} : si aucun NeuraDesk ne tourne, supprimer ce dossier. Arret de ce lancement."
        );
        std::process::exit(0);
    }
}

/// À appeler à CHAQUE itération de la boucle principale existante
/// (`app::run`, `'main: loop`) — voir `Settings::external_message_pump`
/// dans `init` : CEF ne pompe pas ses propres messages tout seul, il faut
/// le lui demander explicitement, exactement comme `Window::pump_messages`
/// le fait déjà pour la boucle de messages Win32 du dock.
pub fn pump() {
    do_message_loop_work();
}

pub fn cef_shutdown() {
    shutdown();
}

#[derive(Clone)]
struct NeuraDeskCefApp {}

wrap_app! {
    pub struct NeuraDeskCefAppBuilder {
        app: NeuraDeskCefApp,
    }

    impl App {
        // Voir le piège n°2 de la doc de module.
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&cef::CefStringUtf16>,
            command_line: Option<&mut cef::CommandLine>,
        ) {
            if let Some(command_line) = command_line {
                command_line.append_switch(Some(&"no-sandbox".into()));
                // ⭐ **UN PROCESSUS DE RENDU PAR SITE, PAS UN PAR PANNEAU.**
                //
                // MESURÉ le 2026-10-08 : les 28 panneaux préchargés ouvraient
                // **29 processus de rendu**, ~17 Mo RÉSIDENTS chacun — soit
                // **~500 Mo sur les 760 Mo** d'empreinte réelle de
                // l'application. Et ce coût n'est NI `PRELOAD_RECT`, NI la
                // swapchain, NI le `QuadRenderer` : créer un panneau n'ajoute
                // **rien** au processus principal (mesuré : 2 206,3 Mo à 28
                // panneaux, 2 206,4 Mo à zéro). Le coût unitaire EST le
                // processus Chromium.
                //
                // Toutes nos pages sont en `file://` — **une seule famille de
                // sites** — donc ce commutateur les fait tenir dans DEUX
                // processus :
                //
                //     témoin                    29 renderers   759,6 Mo
                //     + renderer-process-limit=4    13          563,6 Mo
                //     + process-per-site SEUL        2          393,1 Mo
                //
                // ⚠️ **`--renderer-process-limit=4` A ÉTÉ RETIRÉ APRÈS MESURE** :
                // il donnait 13 seul, et 2 en compagnie de celui-ci — donc
                // `process-per-site` fait tout le travail, et livrer le plafond
                // en plus ne serait qu'un réglage de plus sans effet.
                //
                // ⚠️ **CE QU'ON ÉCHANGE, ET IL FAUT LE SAVOIR** : les pages d'un
                // même renderer partagent **un fil JS** et **un domaine de
                // plantage** — une page qui bloque retarde les autres, un crash
                // les emporte ensemble. Accepté : ce sont nos propres pages
                // locales, et le préchargement est CONSERVÉ (aucun risque de
                // rouvrir le défaut « il faut ouvrir deux fois les menus »).
                // ⚠️ L'isolation tient là où elle compte : une page DISTANTE
                // (l'image de fond optionnelle des Réglages) est un autre site,
                // donc elle garde son propre processus.
                //
                // ⛔ **ET LA MANIÈRE DE POSER UNE VALEUR A COÛTÉ UNE MANCHE** :
                // `AppendSwitch("nom=valeur")` est **rejeté EN SILENCE** — CEF
                // refuse un nom contenant `=`. Mesuré : `0 / 35` processus
                // portaient le commutateur alors que `--no-sandbox`, posé dans le
                // MÊME bloc, y était. C'est `append_switch_with_value` qui sépare
                // nom et valeur. (Et ce commutateur-ci n'a pas de valeur, donc
                // `append_switch` convient.)
                command_line.append_switch(Some(&"process-per-site".into()));
                // SONDE : `NEURADESK_CEF_SWITCHES="disable-gpu,use-angle=swiftshader"`
                // ajoute des commutateurs Chromium, pour reproduire ici le rendu
                // d'une machine sans carte graphique (une machine virtuelle).
                if let Ok(liste) = std::env::var("NEURADESK_CEF_SWITCHES") {
                    for commutateur in liste.split(',').map(str::trim).filter(|c| !c.is_empty()) {
                        match commutateur.split_once('=') {
                            Some((nom, valeur)) => {
                                command_line.append_switch_with_value(Some(&nom.into()), Some(&valeur.into()))
                            }
                            None => command_line.append_switch(Some(&commutateur.into())),
                        }
                    }
                }
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(NeuraDeskBrowserProcessHandler::new())
        }
    }
}

wrap_browser_process_handler! {
    struct NeuraDeskBrowserProcessHandler;

    impl BrowserProcessHandler {
        /// ⛔ **UNE SECONDE INSTANCE TUAIT LA PREMIÈRE.** Une instance qui démarre
        /// trouve le verrou du cache CEF déjà pris : elle transmet son lancement à
        /// celle qui tourne, puis s'arrête. Sans réponse ici, CEF ouvre une
        /// fenêtre Chrome par défaut — impossible dans une application sans
        /// fenêtre de navigateur — et l'instance qui tournait PLANTE dans
        /// `libcef.dll` (`0x80000003`, sans rien au journal). Mesuré le
        /// 2026-10-12 : quatre plantages, chacun une à deux secondes après un
        /// nouveau lancement. Le lancement est ignoré.
        fn on_already_running_app_relaunch(
            &self,
            _command_line: Option<&mut CommandLine>,
            _current_directory: Option<&CefString>,
        ) -> ::std::os::raw::c_int {
            crate::diag!("un second lancement de NeuraDesk a ete ignore (deja en cours)");
            true as _
        }
    }
}

/// État partagé entre `RenderHandler` (rappelé par CEF sur son propre
/// thread de composition interne — en pratique le même thread UI que le
/// reste de cette app tant que `external_message_pump` est utilisé, voir
/// `Settings`) et `CefOverlay` (qui lit `imported` à chaque frame de rendu).
/// `Rc<RefCell<...>>`, pas de synchronisation atomique : CEF avec
/// `external_message_pump` rappelle ses handlers SUR LE MÊME thread que
/// celui qui appelle `do_message_loop_work` (voir `pump` ci-dessus) — pas
/// de vrai multi-threading côté Rust ici, contrairement aux sous-process
/// CEF (des process À PART, pas des threads).
struct RenderState {
    size: (i32, i32),
    imported: Option<(ID3D11Texture2D, ID3D11ShaderResourceView)>,
    /// **COMBIEN D'IMAGES CETTE PAGE A PEINTES** — incrémenté à chaque
    /// `on_paint`/`on_accelerated_paint` réussi. `win32::cef_popup::render`
    /// compare ce compteur à celui de la dernière frame et prévient `app::run`
    /// qu'un popup VISIBLE a une image neuve : c'est ce qui fait présenter le
    /// dock. ⚠️ Le compteur des peintures ne suffit PAS à lui seul : les
    /// treize pages PRÉCHARGÉES peignent aussi, fenêtres cachées — compter
    /// toutes les peintures gardait le dock en train de présenter en continu.
    peintures: u64,
    /// Actions demandées par la page HTML/JS (voir `NeuraDeskRequestHandler`,
    /// doc plus bas) — un bouton fait `location.href =
    /// "neuradesk://<action>"`, interceptée AVANT toute navigation réelle et
    /// poussée ici plutôt qu'exécutée directement dans le callback CEF :
    /// même principe que `Window::drain_events` (voir `win32::window`) pour
    /// les messages Win32 — laisse l'APPELANT (chaque fenêtre popup, voir
    /// `win32::cef_test_window`) décider CE QUE chaque action signifie
    /// (`power::execute`, fermer la fenêtre...), ce module CEF générique
    /// n'a besoin de rien en savoir.
    actions: Vec<String>,
    /// Dernier curseur demandé par la page via CSS `cursor:` (voir
    /// `NeuraDeskDisplayHandler::on_cursor_change` ci-dessous) — `HCURSOR`
    /// de `cef_dll_sys` est un simple pointeur opaque (`*mut HICON__`,
    /// alias `HICON`), PAS le même type que `windows::…::HCURSOR` (une
    /// struct qui l'enveloppe) ; stocké ici en `isize` brut pour éviter
    /// toute ambiguïté entre les deux (`use cef::{args::Args, *};` ci-dessus
    /// importe `cef::HCURSOR` sous ce nom nu, voir la doc de module) —
    /// reconverti côté appelant (`win32::cef_popup::dispatch_input`) au
    /// moment de l'utiliser. `None` tant qu'aucun changement de curseur n'a
    /// encore été signalé (page pas encore chargée, ou CEF n'a jamais quitté
    /// le curseur système par défaut).
    cursor: Option<isize>,
    /// Infobulle demandée par la page (attribut HTML `title` survolé, voir
    /// `NeuraDeskDisplayHandler::on_tooltip`) — `None` quand il n'y en a pas.
    tooltip: Option<String>,
}

struct NeuraDeskRenderHandler {
    state: Rc<RefCell<RenderState>>,
    device: ID3D11Device,
}

wrap_render_handler! {
    pub struct RenderHandlerBuilder {
        handler: Rc<NeuraDeskRenderHandler>,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                let (w, h) = self.handler.state.borrow().size;
                rect.x = 0;
                rect.y = 0;
                rect.width = w;
                rect.height = h;
            }
        }

        fn screen_info(
            &self,
            _browser: Option<&mut Browser>,
            screen_info: Option<&mut ScreenInfo>,
        ) -> ::std::os::raw::c_int {
            if let Some(screen_info) = screen_info {
                // ⛔ **CE `1.0` EST UN REPORT DÉLIBÉRÉ, PAS UN OUBLI — ET IL FAIT
                // PAIRE AVEC `CONTENT_ZOOM_FACTOR`.**
                //
                // La taille PHYSIQUE d'un élément vaut `css × zoom × ce facteur`.
                // Le couple actuel — `1.0` ici, `3.0` dans `cef_popup` — donne donc
                // ×3, ce qui est la demande explicite (« multiplie par 3 la taille
                // des menus et des dashs »), étalonnée sur cette machine
                // (3840×2160 à 150 %).
                //
                // ⛔ **MONTER CE FACTEUR SEUL AGRANDIT TOUT.** Le passer à
                // l'échelle réelle de l'écran — 1.5 ici — sans diviser le zoom
                // d'autant donnerait ×4.5, et tous les panneaux déborderaient. Les
                // deux vont ENSEMBLE : `(1.0, 3.0)` ici équivaut à `(1.5, 2.0)`, et
                // c'est CE couple-là qui rendrait la taille apparente constante
                // d'un écran à l'autre. Aujourd'hui, à résolution LOGIQUE égale, un
                // écran à 100 % de mise à l'échelle rend les mêmes menus **1,5×
                // plus gros** que cette machine — c'est le point 5 de l'audit du
                // 2026-10-06, et il n'a jamais été mesuré.
                //
                // ⚠️ **LA VOIE CITÉE JADIS N'EXISTE PLUS** : ce commentaire renvoyait
                // à `win32::egui_overlay::pixels_per_point_for`, parti avec `egui`
                // (Phase 7 — le module n'est plus dans le dépôt). Le mécanisme
                // survivant est `win32::dpi::get_dpi_for_monitor`
                // (`MDT_EFFECTIVE_DPI`), à interroger PAR ÉCRAN — donc à faire
                // descendre jusqu'ici, ce que ce `GetScreenInfo` ne peut pas faire
                // seul : il ne reçoit aucun `hmonitor`.
                screen_info.device_scale_factor = 1.0;
                return true as _;
            }
            false as _
        }

        /// ⛔ **LE RENDU LOGICIEL : SANS LUI, UNE MACHINE VIRTUELLE N'A AUCUN
        /// TEXTE.** Sans carte graphique, Chromium ne peut pas offrir de texture
        /// partagée : il n'appelle JAMAIS `on_accelerated_paint`, mais ceci, avec
        /// l'image en mémoire. Reproduit ici avec
        /// `NEURADESK_CEF_SWITCHES=disable-gpu` : chaque panneau restait VIDE.
        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: ::std::os::raw::c_int,
            height: ::std::os::raw::c_int,
        ) {
            if type_ != PaintElementType::default() || buffer.is_null() || width <= 0 || height <= 0 {
                return;
            }
            if !SOFTWARE_PAINT_SEEN.swap(true, std::sync::atomic::Ordering::Relaxed) {
                crate::diag!("rendu CEF LOGICIEL (pas de texture partagee) : {width}x{height}");
            }
            let (width, height) = (width as u32, height as u32);
            // SAFETY : CEF garantit `width * height * 4` octets BGRA, valides
            // pendant toute la durée de cet appel.
            let bgra = unsafe { std::slice::from_raw_parts(buffer, (width * height * 4) as usize) };
            match crate::gfx::cef_texture::texture_depuis_bgra(&self.handler.device, width, height, bgra) {
                Ok(texture) => {
                    let mut etat = self.handler.state.borrow_mut();
                    etat.imported = Some(texture);
                    etat.peintures += 1;
                }
                Err(err) => {
                    let count = IMPORT_FAILURES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    IMPORT_WAS_FAILING.store(true, std::sync::atomic::Ordering::Relaxed);
                    if count == 1 || count % 600 == 0 {
                        crate::diag!("texture du rendu LOGICIEL ECHOUEE ({count}e fois) : {err}");
                    }
                }
            }
        }

        fn on_accelerated_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            info: Option<&cef::AcceleratedPaintInfo>,
        ) {
            let Some(info) = info else { return };
            if type_ != PaintElementType::default() {
                return;
            }
            let handle = crate::gfx::cef_texture::to_windows_handle(info.shared_texture_handle as isize);
            match crate::gfx::cef_texture::import_shared_texture(&self.handler.device, handle) {
                Ok(imported) => {
                    // **LE RÉTABLISSEMENT EST AUSSI INTÉRESSANT QUE L'ÉCHEC** :
                    // sans cette ligne, un import qui repart après une série
                    // d'échecs passerait inaperçu, et on ne saurait pas DATER la
                    // fin de l'incident.
                    if IMPORT_WAS_FAILING.swap(false, std::sync::atomic::Ordering::Relaxed) {
                        crate::diag!(
                            "import texture partagee RETABLI apres {} echec(s)",
                            IMPORT_FAILURES.swap(0, std::sync::atomic::Ordering::Relaxed)
                        );
                    }
                    let mut etat = self.handler.state.borrow_mut();
                    etat.imported = Some(imported);
                    etat.peintures += 1;
                }
                Err(err) => {
                    let count = IMPORT_FAILURES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    IMPORT_WAS_FAILING.store(true, std::sync::atomic::Ordering::Relaxed);
                    // RARÉFIÉ, JAMAIS MUET : le premier échec est noté, puis un
                    // rappel toutes les 600 frames (~10 s à 60 Hz).
                    if count == 1 || count % 600 == 0 {
                        crate::diag!(
                            "import texture partagee ECHOUE ({count}e fois) : {err} — device : {}",
                            device_removed_reason(&self.handler.device)
                        );
                    }
                }
            }
        }
    }
}

/// Échecs CONSÉCUTIFS d'import de la texture partagée, et « le dernier essai a
/// échoué » — voir `on_accelerated_paint`, qui les tient à jour.
///
/// **POURQUOI UN ÉTAT, ET PAS UN COMPTEUR PAR APPEL** : cet appel a lieu à
/// CHAQUE FRAME et pour CHAQUE navigateur (treize popups préchargés), donc
/// journaliser chaque échec noierait le journal en une seconde — ce que `diag`
/// interdit explicitement (« pas dans la boucle de rendu »). Ce qu'on veut
/// savoir, c'est QUAND ça commence et QUAND ça s'arrête.
static IMPORT_FAILURES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static IMPORT_WAS_FAILING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// « CEF a déjà peint en LOGICIEL » — pour le noter UNE fois au journal : c'est
/// la signature d'une machine sans carte graphique (voir `on_paint`).
static SOFTWARE_PAINT_SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// **DISTINGUE LES DEUX CAUSES D'UN ÉCHEC D'IMPORT, ET C'EST TOUT SON INTÉRÊT.**
///
/// Un import qui échoue peut vouloir dire deux choses très différentes :
/// **NOTRE** device D3D11 est perdu (pilote, réinitialisation, GPU retiré) —
/// auquel cas `GetDeviceRemovedReason` rend un code d'erreur — ou bien notre
/// device va parfaitement bien et c'est la texture de CEF qui n'est plus
/// valide, c'est-à-dire **le processus GPU de CEF qui est mort**. Sans cette
/// distinction, le journal dirait « l'import échoue » sans jamais dire de quel
/// côté, et l'enquête s'arrêterait là.
///
/// ⚠️ Ce chemin n'existait pas avant le 2026-09-23 : l'application ne testait la
/// perte de device **NULLE PART** (aucun `GetDeviceRemovedReason`,
/// `DEVICE_REMOVED` ni `DEVICE_RESET` dans tout le dépôt), et l'échec d'import
/// partait dans un `eprintln!` — c'est-à-dire dans le vide, l'application étant
/// lancée détachée et sans console.
fn device_removed_reason(device: &ID3D11Device) -> String {
    match unsafe { device.GetDeviceRemovedReason() } {
        Ok(()) => "S_OK — NOTRE device va bien, le probleme vient de CEF".to_string(),
        Err(err) => format!("{err:?}"),
    }
}

/// Capte les changements de curseur CSS (`cursor: col-resize`/`crosshair`/…)
/// demandés par la page — DEMANDE EXPLICITE ("le curseur ne se transforme
/// pas au survol du séparateur du presse-papier") : en rendu CEF hors-écran
/// (OSR), le navigateur ne dessine PAS lui-même son curseur, contrairement
/// au mode fenêtré normal — c'est à l'appli hôte d'appeler `SetCursor()`
/// Win32 (voir `win32::cef_popup::dispatch_input`, cas `WM_SETCURSOR`, seul
/// endroit qui lit `RenderState::cursor`). Sans ce handler ni ce cas
/// `WM_SETCURSOR`, la classe de fenêtre de chaque popup (voir
/// `win32::cef_popup::create_window`) garde son `hCursor` FIXE (`IDC_ARROW`)
/// et `DefWindowProcW` le réapplique à chaque survol, quelle que soit la
/// valeur CSS `cursor:` de l'élément sous le pointeur.
struct NeuraDeskDisplayHandler {
    state: Rc<RefCell<RenderState>>,
}

wrap_display_handler! {
    pub struct DisplayHandlerBuilder {
        handler: Rc<NeuraDeskDisplayHandler>,
    }

    impl DisplayHandler {
        fn on_cursor_change(
            &self,
            _browser: Option<&mut Browser>,
            // `cef::sys::HCURSOR` : le type nu `HCURSOR` n'est PAS
            // ré-exporté à la racine du crate `cef` (seulement importé en
            // privé dans son module `bindings`, voir `cef_dll_sys::*`) —
            // atteignable seulement via son module `sys` (alias public de
            // `cef_dll_sys`, voir `cef::sys`). Un simple pointeur opaque
            // (`*mut HICON__`), PAS le même type que `windows::…::HCURSOR`
            // (voir la doc de `RenderState::cursor`).
            cursor: cef::sys::HCURSOR,
            _type_: CursorType,
            _custom_cursor_info: Option<&CursorInfo>,
        ) -> ::std::os::raw::c_int {
            self.handler.state.borrow_mut().cursor = Some(cursor as isize);
            // `true` : signale à CEF que l'appli hôte prend en charge
            // l'affichage du curseur (voir la doc de struct) — CEF ne
            // tente alors pas sa propre gestion (sans effet visible en OSR
            // de toute façon, mais c'est la sémantique documentée du
            // retour de `on_cursor_change`).
            true as _
        }

        /// ⭐ **LES INFOBULLES `title` DES PAGES** — DEMANDE EXPLICITE
        /// (2026-10-25) : « je ne vois pas le nom des pastilles ni les infos
        /// bulle sur les dashs ». Elles ne passaient pas SOUS le dash : en rendu
        /// hors écran, **CEF ne les dessine jamais**, il ne fait que transmettre
        /// le texte ici, et rien ne l'affichait. On le retient, comme le curseur ;
        /// c'est `app::run` qui l'affiche dans la fenêtre d'infobulle du dock
        /// (voir `win32::cef_popup::infobulle_de_page`). Texte vide = plus
        /// d'infobulle.
        fn on_tooltip(&self, _browser: Option<&mut Browser>, text: Option<&mut CefString>) -> ::std::os::raw::c_int {
            let texte = text.map(|t| t.to_string()).filter(|t| !t.trim().is_empty());
            self.handler.state.borrow_mut().tooltip = texte;
            // `true` : l'hôte se charge de l'affichage.
            true as _
        }
    }
}

impl DisplayHandlerBuilder {
    fn build(state: Rc<RefCell<RenderState>>) -> DisplayHandler {
        Self::new(Rc::new(NeuraDeskDisplayHandler { state }))
    }
}

/// Applique le zoom de PAGE voulu (voir `win32::cef_popup::
/// CONTENT_ZOOM_FACTOR`) une fois le chargement INITIAL terminé —
/// PAS juste après `browser_host_create_browser_sync` (essayé en premier,
/// BUG RÉEL TROUVÉ PAR CAPTURE : la navigation initiale, déjà en cours au
/// moment où `CefBrowserSurface::new` retourne, réinitialise le zoom au
/// niveau par défaut de son "host" — `file://` vide — au moment où elle
/// COMMIT, après notre appel ; confirmé par le forum CEF, voir la doc de
/// `win32::cef_popup::content_zoom_level` — recommande `OnLoadingStateChange`/
/// `OnLoadEnd` plutôt qu'un appel unique à la création). `on_load_end` se
/// déclenche une fois par frame chargée ; nos pages n'ont ni iframe ni
/// navigation ultérieure, donc un seul appel en pratique — mais idempotent
/// si jamais rappelé (remettre le même zoom est un no-op visuel).
struct NeuraDeskLoadHandler {
    zoom_level: f64,
}

wrap_load_handler! {
    pub struct LoadHandlerBuilder {
        handler: Rc<NeuraDeskLoadHandler>,
    }

    impl LoadHandler {
        fn on_load_end(&self, browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _http_status_code: ::std::os::raw::c_int) {
            if let Some(host) = browser.and_then(|b| b.host()) {
                host.set_zoom_level(self.handler.zoom_level);
            }
        }
    }
}

impl LoadHandlerBuilder {
    fn build(zoom_level: f64) -> LoadHandler {
        Self::new(Rc::new(NeuraDeskLoadHandler { zoom_level }))
    }
}

/// Intercepte les liens `neuradesk://<action>` cliqués dans la page (voir
/// la doc de `RenderState::actions`) — le SEUL canal de communication
/// JS→Rust de cette intégration CEF, volontairement le plus simple
/// possible (une redirection interceptée) plutôt qu'une vraie liaison de
/// fonction JavaScript (`V8Handler`, qui vivrait dans le PROCESS DE RENDU,
/// à part du process navigateur — bien plus de machinerie pour un besoin
/// aussi simple que "quel bouton a été cliqué").
struct NeuraDeskRequestHandler {
    state: Rc<RefCell<RenderState>>,
}

wrap_request_handler! {
    pub struct RequestHandlerBuilder {
        handler: Rc<NeuraDeskRequestHandler>,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: ::std::os::raw::c_int,
            _is_redirect: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let Some(request) = request else { return false as _ };
            let url = cef::CefString::from(&request.url()).to_string();
            let Some(action) = url.strip_prefix("neuradesk://") else {
                // Pas un de nos liens (le tout premier chargement de la
                // page `file://` elle-même passe aussi par ici) — laisse
                // la navigation se faire normalement.
                return false as _;
            };
            // Commun à toutes les pages, donc traité ICI et pas par le dispatch
            // de chaque popup : voir `editer`.
            if let Some(commande) = action.strip_prefix("edition/") {
                if let Some(frame) = frame {
                    editer(frame, commande);
                }
                return true as _;
            }
            // PAS de décodage percent-encoding ici : l'action reste
            // volontairement RAW (voir `percent_decode`, doc de fonction) —
            // un ID de périphérique encodé (`win32::cef_sound`) peut
            // contenir un `/` littéral transformé en `%2F` ; le décoder AVANT
            // que l'appelant n'ait scindé l'action sur ses VRAIS séparateurs
            // '/' le rendrait indiscernable d'un séparateur, corrompant le
            // découpage. Chaque popup qui encode un identifiant décode donc
            // lui-même le SEUL segment concerné, après avoir scindé la
            // chaîne encore encodée (voir `win32::cef_sound`, doc de son
            // dispatch).
            self.handler.state.borrow_mut().actions.push(action.trim_end_matches('/').to_string());
            // Annule la navigation : `neuradesk://...` n'est pas une VRAIE
            // page à charger, seulement un message encodé dans une URL.
            true as _
        }

        /// Le canal des grosses données (voir `win32::cef_data`). ⚠️ Appelé
        /// sur le fil IO de CEF : ne JAMAIS toucher `self.handler.state` ici.
        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _is_navigation: ::std::os::raw::c_int,
            _is_download: ::std::os::raw::c_int,
            _request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut ::std::os::raw::c_int>,
        ) -> Option<ResourceRequestHandler> {
            let url = cef::CefString::from(&request?.url()).to_string();
            crate::win32::cef_data::gestionnaire(&url)
        }
    }
}

/// Décodage percent-encoding minimal (RFC 3986, `%XX` → l'octet
/// correspondant) — les popups qui encodent un identifiant arbitraire dans
/// UN SEUL segment d'une action (ex. `win32::cef_sound`, IDs de
/// périphériques audio Windows contenant des accolades/points/deux-points,
/// invalides tels quels dans une URL) le font via `encodeURIComponent` côté
/// JS ; décodé ici plutôt que d'ajouter une dépendance (`url`/
/// `percent-encoding`, déjà présentes TRANSITIVEMENT via d'autres crates,
/// voir `Cargo.lock` — mais pas des dépendances directes) pour une poignée
/// de lignes. À appeler UNIQUEMENT sur le segment encodé lui-même, APRÈS
/// avoir scindé l'action encore RAW sur ses vrais séparateurs `/` (voir
/// `NeuraDeskRequestHandler::on_before_browse`, doc de fonction : décoder
/// AVANT de scinder rendrait un `/` encodé indiscernable d'un séparateur).
/// No-op sur une chaîne sans `%XX` (le cas commun : les segments fixes comme
/// `"toggle-output-mute"`). Opère sur les OCTETS, jamais sur des indices de
/// `&str` tranchés à l'aveugle : une URL vient de l'extérieur (la page
/// chargée), jamais garantie bien formée.
pub fn percent_decode(input: &str) -> String {
    fn hex_val(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl RequestHandlerBuilder {
    fn build(state: Rc<RefCell<RenderState>>) -> RequestHandler {
        Self::new(Rc::new(NeuraDeskRequestHandler { state }))
    }
}

wrap_client! {
    pub struct NeuraDeskCefClientBuilder {
        render_handler: RenderHandler,
        request_handler: RequestHandler,
        load_handler: LoadHandler,
        display_handler: DisplayHandler,
        context_menu_handler: ContextMenuHandler,
    }

    impl Client {
        fn render_handler(&self) -> Option<cef::RenderHandler> {
            Some(self.render_handler.clone())
        }

        fn request_handler(&self) -> Option<cef::RequestHandler> {
            Some(self.request_handler.clone())
        }

        fn load_handler(&self) -> Option<cef::LoadHandler> {
            Some(self.load_handler.clone())
        }

        fn display_handler(&self) -> Option<cef::DisplayHandler> {
            Some(self.display_handler.clone())
        }

        fn context_menu_handler(&self) -> Option<cef::ContextMenuHandler> {
            Some(self.context_menu_handler.clone())
        }
    }
}

// Vide le menu natif de Chromium : le bouton droit est transmis à la page (voir
// `win32::cef_popup::dispatch_input`, `WM_RBUTTONDOWN`), qui dessine son propre
// menu (`assets/popups/menu_contextuel.js`). Sans ce vidage, un `contextmenu` que
// la page n'annule pas ouvrirait Précédent / Recharger / Imprimer — un menu
// vide, CEF ne l'affiche pas.
wrap_context_menu_handler! {
    pub struct NeuraDeskContextMenuHandler;

    impl ContextMenuHandler {
        fn on_before_context_menu(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _params: Option<&mut ContextMenuParams>,
            model: Option<&mut MenuModel>,
        ) {
            if let Some(model) = model {
                model.clear();
            }
        }
    }
}

/// Les commandes d'édition du menu contextuel des pages
/// (`neuradesk://edition/<commande>`, voir `assets/popups/menu_contextuel.js`).
/// Vérifiées par `tests_edition` contre la page et contre `editer`.
#[cfg(test)]
const COMMANDES_EDITION: [&str; 7] = ["annuler", "retablir", "couper", "copier", "coller", "supprimer", "tout-selectionner"];

/// Exécute une commande d'édition sur le cadre, donc sur l'élément qui a le
/// focus — `execCommand("paste")` est refusé par Chromium, `Paste` ne l'est pas.
fn editer(frame: &Frame, commande: &str) {
    match commande {
        "annuler" => frame.undo(),
        "retablir" => frame.redo(),
        "couper" => frame.cut(),
        "copier" => frame.copy(),
        "coller" => frame.paste(),
        "supprimer" => frame.del(),
        "tout-selectionner" => frame.select_all(),
        _ => {}
    }
}

/// Fenêtre CEF OSR — équivalent, pour du contenu HTML/CSS, de
/// `win32::egui_overlay::EguiOverlay` pour du contenu `egui`. Ne possède PAS
/// sa propre fenêtre Win32/swap chain (voir la doc de module et le plan) :
/// ATTACHÉE à une fenêtre/swap chain existante, créée par l'appelant
/// (même répartition des responsabilités que `EguiOverlay`) — cette struct
/// ne gère QUE le navigateur CEF et l'import de sa texture, pas le cycle de
/// vie Win32.
pub struct CefBrowserSurface {
    browser: Browser,
    state: Rc<RefCell<RenderState>>,
}

impl CefBrowserSurface {
    /// `device` : le `ID3D11Device` DANS LEQUEL la texture partagée sera
    /// importée à chaque frame (voir `gfx::cef_texture`) — DOIT être celui
    /// utilisé pour le reste du rendu de la fenêtre hôte. `url` : `file://`
    /// vers une page HTML locale (voir `assets/popups/`, Phase 0).
    /// `zoom_level` : voir `NeuraDeskLoadHandler`, doc de struct — appliqué
    /// une fois le chargement initial terminé, PAS ici (trop tôt, voir sa
    /// doc).
    pub fn new(device: ID3D11Device, width: i32, height: i32, url: &str, zoom_level: f64) -> Self {
        let state = Rc::new(RefCell::new(RenderState { size: (width, height), imported: None, peintures: 0, actions: Vec::new(), cursor: None, tooltip: None }));
        let render_handler = RenderHandlerBuilder::new(Rc::new(NeuraDeskRenderHandler { state: state.clone(), device }));
        let request_handler = RequestHandlerBuilder::build(state.clone());
        let load_handler = LoadHandlerBuilder::build(zoom_level);
        let display_handler = DisplayHandlerBuilder::build(state.clone());
        let context_menu_handler = NeuraDeskContextMenuHandler::new();
        let mut client =
            NeuraDeskCefClientBuilder::new(render_handler, request_handler, load_handler, display_handler, context_menu_handler);

        // `shared_texture_enabled` : CORRIGÉ après un premier essai raté
        // (voir docs/plans/cef-web-ui-migration.md) — SANS lui, CEF envoie
        // `on_paint` (buffer CPU, jamais implémenté ici) au lieu
        // d'`on_accelerated_paint` : le fond flouté composait bien mais
        // AUCUN contenu HTML n'apparaissait, sans la moindre erreur (le
        // `RenderHandler` n'était simplement jamais rappelé avec une
        // texture). `external_begin_frame_enabled` volontairement PAS
        // activé : cette fenêtre de test ne pilote pas CEF frame par frame
        // (voir `windowless_frame_rate` ci-dessous — CEF peint sur son
        // propre rythme automatique).
        let window_info = WindowInfo {
            windowless_rendering_enabled: 1,
            shared_texture_enabled: 1,
            ..Default::default()
        };
        let browser_settings = BrowserSettings {
            windowless_frame_rate: 60,
            ..Default::default()
        };
        let browser = browser_host_create_browser_sync(
            Some(&window_info),
            Some(&mut client),
            Some(&CefString::from(url)),
            Some(&browser_settings),
            None,
            None,
        );
        let browser = browser.expect("échec de création du navigateur CEF OSR");

        Self { browser, state }
    }

    /// La texture D3D11 la plus récente importée depuis CEF (voir
    /// `on_accelerated_paint`) — `None` tant qu'aucune frame n'est encore
    /// arrivée (chargement de la page en cours). L'appelant (voir
    /// `win32::cef_app`, doc de module) la dessine via
    /// `QuadInstance::new_textured`/`TexMode::Texture` (alpha réelle de la
    /// texture, voir `gfx::quad_renderer`) — PAS `new_backdrop`, ce mode
    /// forcerait `alpha = 1.0`.
    pub fn latest_texture(&self) -> Option<ID3D11ShaderResourceView> {
        self.state.borrow().imported.as_ref().map(|(_, srv)| srv.clone())
    }

    /// Le nombre d'images que cette page a peintes — voir `RenderState::peintures`.
    pub fn peintures(&self) -> u64 {
        self.state.borrow().peintures
    }

    /// Dit à Chromium que ce navigateur est CACHÉ (ou redevenu visible).
    ///
    /// ⛔ **SANS CETTE LIGNE, UNE PAGE CACHÉE CONTINUE DE PEINDRE.** Les treize
    /// pages PRÉCHARGÉES peignaient ~70 images/s, fenêtres hors écran : du travail
    /// dans le processus navigateur et des textures importées pour rien. `was_hidden`
    /// est l'API prévue pour ça — Chromium cesse alors de composer et d'animer.
    /// Appelé par `win32::cef_popup::render_if_visible` UNIQUEMENT sur changement
    /// d'état (voir `CefPopupState::visible_notifiee`).
    pub fn set_visible(&self, visible: bool) {
        if let Some(host) = self.browser.host() {
            host.was_hidden((!visible) as _);
        }
    }

    /// Dernier curseur demandé par la page via CSS `cursor:` (voir
    /// `NeuraDeskDisplayHandler::on_cursor_change`) — `None` tant qu'aucun
    /// changement n'a encore été signalé (curseur système par défaut).
    /// Retourne le handle Win32 BRUT (`isize`, voir la doc de
    /// `RenderState::cursor`) : à l'appelant (`win32::cef_popup::
    /// dispatch_input`) de le reconstituer en `windows::…::HCURSOR` au
    /// moment de l'utiliser.
    pub fn latest_cursor(&self) -> Option<isize> {
        self.state.borrow().cursor
    }

    /// Infobulle `title` actuellement demandée par la page (voir
    /// `NeuraDeskDisplayHandler::on_tooltip`).
    pub fn latest_tooltip(&self) -> Option<String> {
        self.state.borrow().tooltip.clone()
    }

    /// Vide et retourne les actions demandées par la page depuis le dernier
    /// appel (voir `RenderState::actions`, doc de champ) — À appeler à
    /// chaque frame par la fenêtre popup propriétaire (voir
    /// `win32::cef_test_window`), même principe que `Window::drain_events`
    /// pour les messages Win32 : vide TOUJOURS la file, même si
    /// l'appelant ignore le résultat, pour ne jamais laisser une action
    /// s'accumuler indéfiniment si personne ne la lit.
    pub fn drain_actions(&self) -> Vec<String> {
        std::mem::take(&mut self.state.borrow_mut().actions)
    }

    /// `x`/`y` : pixels PHYSIQUES de la fenêtre hôte (voir `WM_MOUSEMOVE`,
    /// tel quel — cette fenêtre de test n'a pas encore de mise à l'échelle
    /// DPI, voir `screen_info` ci-dessus). `modifiers` : voir
    /// `win32::cef_popup::input_modifiers` — DOIT porter
    /// `EVENTFLAG_LEFT_MOUSE_BUTTON` (16) quand le bouton gauche est tenu,
    /// sinon Blink traite ce déplacement comme un simple survol plutôt
    /// qu'un glissement en cours (BUG RÉEL : sans ce bit, un `<input
    /// type="range">` ne suit jamais le curseur pendant qu'on le glisse,
    /// même si `WM_MOUSEMOVE` atteint bien cette fenêtre). `leave` :
    /// `true` sur `WM_MOUSELEAVE`, voir `send_mouse_move_event`.
    pub fn on_mouse_move(&self, x: i32, y: i32, modifiers: u32, leave: bool) {
        if let Some(host) = self.browser.host() {
            let event = cef::MouseEvent { x, y, modifiers };
            host.send_mouse_move_event(Some(&event), leave as _);
        }
    }

    /// `pressed` : `true` sur `WM_LBUTTONDOWN`, `false` sur `WM_LBUTTONUP`
    /// — voir `send_mouse_click_event` (`mouse_up` est l'INVERSE de
    /// `pressed`). `click_count` : 2 = double-clic (sans lui, jamais de
    /// `dblclick`), compté par `win32::cef_popup::dispatch_input` ;
    /// `modifiers` : voir `on_mouse_move` (sans lui, ni `ctrlKey` ni
    /// `shiftKey` sur un `click`).
    pub fn on_mouse_left_button(&self, x: i32, y: i32, pressed: bool, modifiers: u32, click_count: i32) {
        if let Some(host) = self.browser.host() {
            let event = cef::MouseEvent { x, y, modifiers };
            host.send_mouse_click_event(Some(&event), cef::MouseButtonType::LEFT, (!pressed) as _, click_count);
        }
    }

    /// Un clic droit COMPLET (appui puis relâchement) : voir
    /// `win32::cef_popup::dispatch_input`, `WM_RBUTTONDOWN` — ces fenêtres ne
    /// reçoivent jamais `WM_RBUTTONUP`. Sous Windows, Blink émet `contextmenu`
    /// au RELÂCHEMENT : l'appui seul ne produirait rien.
    /// Un clic MOLETTE complet (appui puis relâchement) — même modèle que
    /// [`Self::on_mouse_right_click`] : la page reçoit `mousedown`, `mouseup`
    /// et `auxclick` avec `button === 1` (vue sphère des terminaux, fermeture
    /// d'un onglet au clic molette).
    pub fn on_mouse_middle_click(&self, x: i32, y: i32, modifiers: u32) {
        const EVENTFLAG_MIDDLE_MOUSE_BUTTON: u32 = 32;
        if let Some(host) = self.browser.host() {
            let appui = cef::MouseEvent { x, y, modifiers: modifiers | EVENTFLAG_MIDDLE_MOUSE_BUTTON };
            host.send_mouse_click_event(Some(&appui), cef::MouseButtonType::MIDDLE, 0, 1);
            let relache = cef::MouseEvent { x, y, modifiers };
            host.send_mouse_click_event(Some(&relache), cef::MouseButtonType::MIDDLE, 1, 1);
        }
    }

    pub fn on_mouse_right_click(&self, x: i32, y: i32, modifiers: u32) {
        const EVENTFLAG_RIGHT_MOUSE_BUTTON: u32 = 64;
        if let Some(host) = self.browser.host() {
            let appui = cef::MouseEvent { x, y, modifiers: modifiers | EVENTFLAG_RIGHT_MOUSE_BUTTON };
            host.send_mouse_click_event(Some(&appui), cef::MouseButtonType::RIGHT, 0, 1);
            let relache = cef::MouseEvent { x, y, modifiers };
            host.send_mouse_click_event(Some(&relache), cef::MouseButtonType::RIGHT, 1, 1);
        }
    }

    /// Voir `WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL` — DEMANDE EXPLICITE ("je ne
    /// peux pas faire défiler avec la molette") : jamais câblé jusqu'ici,
    /// aucun popup n'avait encore de contenu qui déborde de sa fenêtre
    /// (Dash/Réglages scrollent maintenant que leur zoom ×3 laisse moins
    /// tenir à l'écran, voir `win32::cef_popup::CONTENT_ZOOM_FACTOR`).
    /// `x`/`y` : coordonnées de VUE (client), PAS les coordonnées ÉCRAN
    /// brutes de `WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL` — conversion à charge de
    /// l'appelant (voir `win32::cef_popup::dispatch_input`, seule
    /// particularité Win32 propre à CES deux messages). `delta_x`/
    /// `delta_y` : incréments `WHEEL_DELTA` (120) bruts, transmis tels
    /// quels — CEF fait sa propre mise à l'échelle interne.
    /// `modifiers` : voir `on_mouse_move` — sans lui la page ne voit jamais
    /// `ctrlKey` sur un `wheel` (le zoom de `editeur_image.html`).
    pub fn on_mouse_wheel(&self, x: i32, y: i32, modifiers: u32, delta_x: i32, delta_y: i32) {
        if let Some(host) = self.browser.host() {
            let event = cef::MouseEvent { x, y, modifiers };
            host.send_mouse_wheel_event(Some(&event), delta_x, delta_y);
        }
    }

    /// Voir `WM_SETFOCUS`/`WM_KILLFOCUS` — sans ça, CEF ne sait jamais que
    /// cette fenêtre a le focus clavier (pas de fenêtre Win32 propre côté
    /// CEF en OSR, voir la doc de module).
    pub fn set_focus(&self, focused: bool) {
        if let Some(host) = self.browser.host() {
            host.set_focus(focused as _);
        }
    }

    /// Exécute `code` dans le contexte de la frame principale de cette page
    /// — canal Rust→JS (voir `win32::cef_popup::execute_js`, doc d'appelant)
    /// pour les popups qui doivent pousser un état lu côté Rust vers leur
    /// page (ex. `win32::cef_network` pour l'état Wi-Fi/Bluetooth, ou
    /// `win32::cef_calendar` pour les évènements du mois — aucun des deux n'est
    /// calculable en JS pur).
    /// No-op silencieux si la frame principale n'est pas encore prête
    /// (page en cours de premier chargement) — cohérent avec le reste de
    /// cette struct (`latest_texture` retourne aussi `None` dans ce cas).
    pub fn execute_js(&self, code: &str) {
        if let Some(frame) = self.browser.main_frame() {
            frame.execute_java_script(Some(&CefString::from(code)), Some(&CefString::from("")), 0);
        }
    }

    /// Voir `WM_KEYDOWN`/`WM_SYSKEYDOWN` — `vk` : le code de touche virtuelle
    /// Win32 brut (`wparam`), CEF utilise directement les mêmes valeurs
    /// `VK_*` sur Windows (voir `KeyEvent::windows_key_code`, doc). Premier
    /// popup à en avoir besoin : `win32::cef_dash`, dont le champ de
    /// recherche est un vrai `<input>` HTML — un curseur de gain
    /// (`win32::cef_voicemeeter`) ou de taille (`win32::cef_spacer_size`)
    /// en profite aussi gratuitement (flèches gauche/droite quand il a le
    /// focus, comportement natif du navigateur).
    pub fn on_key_down(&self, vk: i32, modifiers: u32, is_system_key: bool) {
        if let Some(host) = self.browser.host() {
            let event = cef::KeyEvent {
                type_: cef::KeyEventType::RAWKEYDOWN,
                windows_key_code: vk,
                modifiers,
                is_system_key: is_system_key as _,
                ..Default::default()
            };
            host.send_key_event(Some(&event));
        }
    }

    pub fn on_key_up(&self, vk: i32, modifiers: u32, is_system_key: bool) {
        if let Some(host) = self.browser.host() {
            let event = cef::KeyEvent {
                type_: cef::KeyEventType::KEYUP,
                windows_key_code: vk,
                modifiers,
                is_system_key: is_system_key as _,
                ..Default::default()
            };
            host.send_key_event(Some(&event));
        }
    }

    /// Voir `WM_CHAR`/`WM_SYSCHAR` — `ch` : le caractère Unicode déjà
    /// traduit par Windows (`wparam`), PAS un code de touche virtuelle
    /// (voir `on_key_down`) — c'est CE message, pas `WM_KEYDOWN`, que CEF
    /// utilise pour insérer du texte dans un champ (même distinction que
    /// Win32 lui-même entre les deux messages).
    pub fn on_char(&self, ch: u16, modifiers: u32, is_system_key: bool) {
        if let Some(host) = self.browser.host() {
            let event = cef::KeyEvent {
                type_: cef::KeyEventType::CHAR,
                windows_key_code: ch as i32,
                character: ch,
                unmodified_character: ch,
                modifiers,
                is_system_key: is_system_key as _,
                ..Default::default()
            };
            host.send_key_event(Some(&event));
        }
    }

    pub fn resize(&mut self, width: i32, height: i32) {
        self.state.borrow_mut().size = (width, height);
        if let Some(host) = self.browser.host() {
            host.was_resized();
        }
    }

    /// Change le niveau de zoom de PAGE de ce popup immédiatement — même
    /// appel que `NeuraDeskLoadHandler::on_load_end` (voir sa doc pour
    /// pourquoi PAS à la création), mais utilisable À TOUT MOMENT après
    /// coup, PAS juste au chargement initial. Prend `&self` (pas `&mut`) :
    /// `host.set_zoom_level` ne touche que l'état interne CEF, rien côté
    /// Rust n'a besoin d'être mis à jour ici, comme `on_mouse_move`/
    /// `on_key_down` etc. ci-dessus. Idempotent si rappelé avec la même
    /// valeur (remettre le même zoom est un no-op visuel, même raison que
    /// `on_load_end`) — voir `win32::cef_popup::render`, qui rappelle ceci
    /// À CHAQUE FRAME pour les popups de type "menu" : le curseur "Taille
    /// des menus" du panneau de réglages met donc à jour le ZOOM d'un menu
    /// déjà ouvert immédiatement, MAIS pas la taille de sa fenêtre Win32
    /// (figée à l'ouverture, voir `app::menu_rect`) — un cas d'usage
    /// marginal (avoir Réglages ET un menu ouverts en même temps en bougeant
    /// ce curseur précis), qui se résorbe de lui-même à la prochaine
    /// ouverture du menu.
    pub fn set_zoom_level(&self, level: f64) {
        if let Some(host) = self.browser.host() {
            host.set_zoom_level(level);
        }
    }
}

#[cfg(test)]
mod tests_edition {
    use super::COMMANDES_EDITION;

    #[test]
    fn chaque_commande_du_menu_contextuel_est_connue() {
        let page = include_str!("../../assets/popups/menu_contextuel.js");
        let envoyees: Vec<&str> = page.split("editer(\"").skip(1).filter_map(|s| s.split('"').next()).collect();
        assert!(envoyees.len() >= COMMANDES_EDITION.len(), "{envoyees:?}");
        for commande in envoyees {
            assert!(COMMANDES_EDITION.contains(&commande), "commande inconnue de `editer` : {commande}");
        }
    }

    #[test]
    fn editer_traite_chaque_commande_declaree() {
        let source = include_str!("cef_app.rs");
        let corps = source.split("fn editer(").nth(1).unwrap().split("\n}\n").next().unwrap();
        for commande in COMMANDES_EDITION {
            assert!(corps.contains(&format!("\"{commande}\" =>")), "`editer` ne traite pas {commande}");
        }
    }
}
