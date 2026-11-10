// NeuraDesk - a Unity-launcher-style dock overlay for Windows 11.
//
// Garde une console en debug (pratique pour les eprintln! de diagnostic),
// la masque en release pour un vrai overlay silencieux.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// `win32::cef_settings` construit un unique `json!` géant (l'état complet du
// panneau de Réglages, voir `SettingsRefs`) — chaque nouveau champ ajouté au
// fil des fonctionnalités approfondit sa macro-expansion récursive jusqu'à
// dépasser la limite du compilateur ; relevée ici plutôt que de fragmenter ce
// `json!` (qui reflète fidèlement, en un seul bloc, l'état poussé au JS).
//
// **L'ÉCHELLE, POUR SAVOIR QUAND S'INQUIÉTER** : 128 (défaut) → 256 (premier
// dépassement, `win32::fullscreen_clock`) → **512** (2026-09-20, atteint par
// l'accumulation des réglages d'aperçus, du fond d'écran des bureaux et du logo
// animé — une douzaine de clés en une session). Ce n'est PAS une métrique de
// qualité : c'est le prix, connu et accepté, d'un état poussé en un seul bloc.
#![recursion_limit = "512"]

mod alloc_trace;
mod app;
mod config;
mod config_initiale;
mod diag;
mod plantages;
mod dock_instance;
mod ecrans_identites;
mod gfx;
mod sauvegarde_config;
mod ui;
mod win32;

/// **L'ALLOCATEUR INSTRUMENTÉ** — voir `alloc_trace`, doc de module. Éteint tant
/// que `NEURADESK_TRACE_ALLOC` n'est pas positionnée : le chemin se réduit alors à
/// une lecture d'atome.
#[global_allocator]
static ALLOCATEUR: alloc_trace::Traceur = alloc_trace::Traceur;

fn main() {
    // ⚠️ **AVANT TOUT LE RESTE** : `alloc_trace::init` lit l'environnement, ce
    // qui ALLOUE — et le faire depuis `alloc` re-entrerait dans l'allocateur
    // pendant son initialisation (voir le piège n°2 de `alloc_trace`).
    alloc_trace::init();

    // Le décodeur isolé des fichiers à calques de l'éditeur d'image (voir
    // `win32::decodeur_document`) : décode et SORT. Avant `dispatch_subprocess`
    // sans le contredire — il ne lit que la ligne de commande, aucun appel CEF.
    win32::decodeur_document::executer_si_demande();

    // L'ENREGISTREUR (voir `win32::gst`) : enregistre et SORT — même discipline.
    win32::gst::executer_si_demande();

    // L'ANCIEN GARDE DES TOASTS (voir `win32::toast_garde`, doc de module) n'est
    // plus lancé ; un processus qui porte encore son drapeau sort aussitôt — il
    // ne doit JAMAIS atteindre la création de la fenêtre du dock.
    win32::toast_garde::sortir_si_ancien_garde();

    // La copie de la configuration pour le portable (voir `config_initiale`) :
    // ecrit et SORT, comme les deux ci-dessus.
    config_initiale::exporter_si_demande();

    // **L'ASSISTANT DE MISE À JOUR** (voir `win32::maj`, doc de module) : ce
    // processus est une COPIE de l'exécutable, lancée après la fermeture de
    // l'application ; il échange les dossiers et relance, puis SORT. Avant tout
    // appel CEF, comme les trois modes ci-dessus.
    win32::maj::executer_si_demande();

    // DOIT être le TOUT PREMIER appel CEF de `main()`, avant même le reste de
    // l'initialisation Win32 ci-dessous — voir `win32::cef_app`, doc de
    // module ("piège n°1") : `cef::api_hash` doit précéder tout autre appel
    // à l'API CEF, et un sous-process CEF (résultat `None` ici) ne doit
    // JAMAIS atteindre `app::run()`/créer la fenêtre du dock — voir la doc
    // de `dispatch_subprocess`. Phase 0 de
    // `docs/plans/cef-web-ui-migration.md`.
    let Some((cef_args, mut cef_app)) = win32::cef_app::dispatch_subprocess() else {
        return;
    };

    // Journal de diagnostic (voir `diag`, doc de module) — APRÈS
    // `dispatch_subprocess` À DESSEIN : un sous-processus CEF sort par le
    // `return` ci-dessus, et n'a donc jamais à ouvrir ce fichier. Sans effet
    // tant que `NEURADESK_DIAG` n'est pas positionnée.
    // Journal des ARRÊTS, toujours actif (voir `plantages`, doc de module) —
    // AVANT `diag::init`, dont le `panic` hook s'enchaîne sur le sien.
    plantages::installer();
    // Une instance de RELÈVE attend que la plantée soit morte (voir
    // `plantages`, section PLANTAGE NATIF) — avant `cef_app::init`.
    plantages::attendre_si_demande();
    diag::init();

    // QUELLE LIGNÉE DE WINDOWS — après `diag::init` (avant, la ligne serait
    // perdue), et avant toute décision de comportement : c'est la seule source
    // qui distingue Windows 10 de Windows 11, le majeur valant 10 des deux
    // côtés (voir `win32::os_version`).
    win32::os_version::journal_windows();

    // Un import de configuration confirmé, que le rechargement n'aurait pas
    // appliqué (voir `sauvegarde_config`) — AVANT toute lecture de réglage.
    sauvegarde_config::appliquer_si_confirme();

    // Voir `win32::cef_app::contain_child_processes`, doc de fonction — tôt,
    // avant tout sous-process (CEF ou autre) créé par ce projet.
    win32::cef_app::contain_child_processes();

    // Rend le process "DPI aware" par moniteur : indispensable pour que le
    // dock garde une taille physique cohérente sur les écrans mis à l'échelle
    // (125%, 150%, ...), comme le fait Unity/GNOME sous Linux.
    win32::dpi::make_process_dpi_aware();

    // La configuration initiale du portable, ou celle d'un import, ADAPTEE a
    // l'ecran (voir `config_initiale`) — apres `make_process_dpi_aware` (sinon
    // l'ecran mesure est virtualise) et avant toute lecture de reglage.
    config_initiale::au_lancement();

    // L'historique d'images du presse-papiers quitte le dossier ITINÉRANT pour
    // celui des caches (2026-10-29) : AVANT toute lecture de l'historique, donc
    // avant `app::run`. Sans effet les fois suivantes.
    config::migrer_clipboard_images();

    // Requis par les API WinRT (contrôles multimédias, voir `win32::media`)
    // et par certains objets COM classiques (énumération des périphériques
    // audio, voir `win32::audio`) — sans appartement COM initialisé sur ce
    // thread, leurs appels échouent silencieusement ou renvoient
    // CO_E_NOTINITIALIZED. Appartement à thread unique (STA) : adapté à une
    // application GUI classique comme celle-ci.
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }

    // Doit être appelé avant la création de la première fenêtre/du premier
    // menu pour que les menus contextuels suivent le thème sombre de
    // Windows (voir sa doc).
    win32::theme::enable_dark_menus();

    // `cef_args`/`cef_app` DOIVENT rester en vie jusqu'à `cef_shutdown()`
    // (CEF garde une référence interne) — voir `win32::cef_app::init`.
    win32::cef_app::init(&cef_args, &mut cef_app);
    // APRÈS CEF, qui peut installer le sien (voir `plantages`, PLANTAGE NATIF).
    plantages::installer_le_filtre_natif();

    // L'ÉCRAN D'ACCUEIL (voir `win32::ecran_accueil`) — APRÈS `cef_app::init`,
    // qui fait sortir une seconde instance : elle ne doit pas montrer le logo.
    // Sur son propre fil : ne retarde pas `app::run`.
    win32::ecran_accueil::afficher();

    let result = app::run();

    // `win32::cef_app::pump()` est déjà appelée à chaque itération de la
    // boucle principale (voir `app::run`) — l'arrêt se fait après coup,
    // best-effort comme le reste des nettoyages de fin de `app::run` (voir
    // sa doc).
    win32::cef_app::cef_shutdown();

    // Voir `app::spawn_replacement_process`, doc de fonction (bug réel
    // constaté) : DOIT venir APRÈS `cef_shutdown()` ci-dessus — CEF garde
    // un verrou façon "SingletonLock" sur son répertoire de cache tant que
    // CE process ne l'a pas relâché, ce qui fait échouer `cef::initialize`
    // dans la nouvelle instance si elle démarre trop tôt.
    if app::reload_was_requested() {
        // L'import confirmé s'applique ICI : `app::run` est fini, plus rien ne
        // réécrira ses fichiers (voir `sauvegarde_config`).
        sauvegarde_config::appliquer_si_confirme();
        // **UNE MISE À JOUR PRÉPARÉE PREND LE PAS SUR UNE SIMPLE RELANCE** :
        // c'est l'assistant qui relancera l'application, après avoir échangé
        // les dossiers. `false` = aucune mise à jour demandée, ou son lancement
        // a échoué — on retombe alors sur le rechargement ordinaire.
        if !win32::maj::lancer_l_installation_si_demandee() {
            app::spawn_replacement_process();
        }
    }

    match &result {
        Ok(()) if app::reload_was_requested() => plantages::noter("fin normale, relance demandée"),
        Ok(()) => plantages::noter("fin normale"),
        Err(err) => plantages::noter(&format!("ERREUR FATALE rendue par app::run : {err}")),
    }
    if let Err(err) = result {
        // Perte du périphérique graphique (voir `plantages`, section du même
        // nom) : relancer, APRÈS `cef_shutdown()` ci-dessus comme « Recharger »,
        // sinon le verrou du cache CEF fait échouer la nouvelle instance.
        if plantages::est_une_perte_du_gpu(err.as_ref()) {
            if plantages::relance_autorisee() {
                plantages::noter("périphérique graphique perdu : relance de l'application");
                app::spawn_replacement_process();
            } else {
                plantages::noter("périphérique graphique perdu : trop de relances récentes, abandon");
            }
        }
        eprintln!("[neuradesk] erreur fatale: {err}");
        std::process::exit(1);
    }
}
