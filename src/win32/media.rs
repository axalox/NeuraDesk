//! Contrôles multimédias universels affichés directement dans la barre du
//! haut — façon module "Media" de Seelen UI ou barre supérieure GNOME :
//! titre en cours et précédent/lecture-pause/suivant, valables pour
//! n'importe quel lecteur (Spotify, navigateur, VLC, lecteur intégré...)
//! sans configuration par application, via l'API système SMTC
//! (`GlobalSystemMediaTransportControlsSessionManager`) — la même que
//! Windows utilise pour son propre widget média/l'overlay volume+média.
//!
//! **`now_playing()` NE BLOQUE JAMAIS le thread appelant** (voir
//! `spawn_poll`/`PENDING`/`LAST`) — BUG RÉEL CONSTATÉ EN PRATIQUE sans ça :
//! l'ancienne version appelait `IAsyncOperation::get()` (voir l'historique,
//! `GetBlocking::get_blocking`) qui attend le service système NPSMSvc
//! ("Now Playing Session Manager") SANS AUCUNE LIMITE DE TEMPS. `app::run`
//! appelle `now_playing()` UNE FOIS PAR SECONDE (voir `app::
//! SYS_MONITOR_REFRESH_INTERVAL`) depuis son thread principal, celui qui
//! pompe TOUTE l'application (dock ET Dash/réglages/presse-papiers) —
//! confirmé en usage réel via "Analyser la chaîne d'attente" du
//! Gestionnaire des tâches : `neuradesk.exe` bloqué sur un `svchost.exe`
//! (RPC), lui-même bloqué sur un second `svchost.exe` hébergeant
//! `NPSMSvc`. Si ce service se bloque (constaté en pratique, sans lien
//! avec NeuraDesk lui-même), l'appel suivant restait bloqué indéfiniment,
//! gelant tout le thread — sablier au survol de N'IMPORTE QUELLE fenêtre
//! de l'application, puis fermeture forcée par Windows ("Application
//! Hang", Event ID 1002) au bout de quelques secondes. `now_playing()`
//! sonde maintenant un résultat déjà prêt (voir `PENDING`, `LAST`) et
//! relance une tentative sur un THREAD À PART si aucune n'est en vol — un
//! cycle de rafraîchissement (1s) peut renvoyer une info d'UN cycle de
//! retard si le service traîne, jamais bloquer indéfiniment.
//!
//! Nécessite que l'appartement COM du process ait été initialisé
//! (`CoInitializeEx`, voir `main::main`) : les API WinRT (dont celle-ci)
//! reposent dessus même si on ne s'en sert pas autrement. Le thread à part
//! (voir `spawn_poll`/`spawn_command`) initialise le SIEN séparément
//! (`COINIT_MULTITHREADED` — il ne possède aucune fenêtre, pas besoin de
//! pomper de messages, contrairement au thread principal, voir
//! `win32::dash::spawn_rescan`, même raisonnement).

use std::cell::RefCell;
use std::sync::mpsc::Receiver;

use windows_future::IAsyncOperation;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as SessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};

/// Morceau/contenu actuellement rapporté par l'application active la plus
/// pertinente (au sens de Windows — généralement la dernière à avoir été
/// utilisée). Titre/artiste peuvent être vides si l'application ne fournit
/// pas ces métadonnées (certains lecteurs ne renseignent que le titre).
/// `Clone` : voir `LAST`, la dernière valeur connue est réutilisée telle
/// quelle tant qu'un nouveau sondage n'a pas abouti.
#[derive(Clone)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub is_playing: bool,
}

thread_local! {
    /// Sondage en cours sur un thread à part (voir `spawn_poll`) — `None`
    /// tant qu'aucun n'est en vol. `now_playing` le sonde (`try_recv`, non
    /// bloquant) à chaque appel ; `LAST` n'est mis à jour qu'à réception.
    static PENDING: RefCell<Option<Receiver<Option<NowPlaying>>>> = const { RefCell::new(None) };
    /// Dernière valeur obtenue avec succès — renvoyée telle quelle par
    /// `now_playing` tant qu'aucun sondage plus récent n'a encore abouti
    /// (voir la doc de module : mieux vaut une info vieille d'un cycle
    /// qu'un thread principal bloqué).
    static LAST: RefCell<Option<NowPlaying>> = const { RefCell::new(None) };
}

/// Relit l'état de lecture actuel — voir la doc de module, NE BLOQUE
/// JAMAIS : renvoie la dernière valeur connue (voir `LAST`) et relance un
/// sondage en arrière-plan si aucun n'est déjà en vol (voir `PENDING`).
/// `None` si aucune application ne rapporte de session multimédia (rien en
/// cours, API indisponible, ou tout premier appel avant que le premier
/// sondage n'ait eu le temps d'aboutir) — l'appelant masque alors le widget
/// plutôt que d'afficher un état inventé (voir `app::run`).
pub fn now_playing() -> Option<NowPlaying> {
    PENDING.with(|pending| {
        let mut pending_ref = pending.borrow_mut();
        let done = match &*pending_ref {
            Some(rx) => match rx.try_recv() {
                Ok(result) => {
                    LAST.with(|l| *l.borrow_mut() = result);
                    true
                }
                // Pas encore fini cette frame : rien à faire, on relit
                // `LAST` tel quel plus bas.
                Err(std::sync::mpsc::TryRecvError::Empty) => false,
                // Thread mort sans envoyer (best-effort silencieux, comme
                // tout le reste de ce module) — libère `PENDING` pour
                // qu'un futur cycle puisse relancer un sondage.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => true,
            },
            None => true,
        };
        if done {
            *pending_ref = Some(spawn_poll());
        }
    });
    LAST.with(|l| l.borrow().clone())
}

/// Lance un sondage SMTC complet sur un thread à part — voir la doc de
/// module. Jamais plus d'un en vol à la fois (voir `now_playing`).
fn spawn_poll() -> Receiver<Option<NowPlaying>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_MULTITHREADED,
            );
        }
        let result = poll_now_playing();
        unsafe {
            windows::Win32::System::Com::CoUninitialize();
        }
        let _ = tx.send(result);
    });
    rx
}

/// Le VRAI travail bloquant (voir la doc de module) — tourne UNIQUEMENT
/// sur le thread de `spawn_poll`, jamais sur le thread principal.
fn poll_now_playing() -> Option<NowPlaying> {
    let session = current_session()?;
    let props = session.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
    let playback = session.GetPlaybackInfo().ok()?;
    let status = playback.PlaybackStatus().ok()?;
    Some(NowPlaying {
        title: props.Title().map(|s| s.to_string()).unwrap_or_default(),
        artist: props.Artist().map(|s| s.to_string()).unwrap_or_default(),
        is_playing: status == PlaybackStatus::Playing,
    })
}

/// Bascule lecture/pause de la session active. Best-effort : sans effet
/// silencieux si aucune session n'est disponible ou si l'appli ne supporte
/// pas cette commande (certaines n'exposent que play OU pause, jamais les
/// deux — `TryTogglePlayPauseAsync` gère ce cas côté Windows). Voir la doc
/// de module : envoyée depuis un thread à part, jamais bloquant pour
/// l'appelant — un clic sur ce bouton ne doit pas pouvoir geler l'appli si
/// NPSMSvc est dans le même état bloqué que celui qui affectait
/// `now_playing`.
pub fn play_pause() {
    spawn_command(|s| s.TryTogglePlayPauseAsync());
}

pub fn next() {
    spawn_command(|s| s.TrySkipNextAsync());
}

pub fn previous() {
    spawn_command(|s| s.TrySkipPreviousAsync());
}

fn current_session() -> Option<Session> {
    let manager = SessionManager::RequestAsync().ok()?.join().ok()?;
    manager.GetCurrentSession().ok()
}

/// Exécute `f` (une commande SMTC) sur un thread à part, fire-and-forget —
/// aucun résultat à récupérer (voir les trois appelants), donc pas besoin
/// du mécanisme `PENDING`/`LAST` de `now_playing` : ce thread se termine
/// tout seul dès que `f`/son attente aboutit, sans jamais faire attendre
/// l'appelant sur le thread principal.
fn spawn_command<F>(f: F)
where
    F: FnOnce(&Session) -> windows::core::Result<IAsyncOperation<bool>> + Send + 'static,
{
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_MULTITHREADED,
            );
        }
        if let Some(session) = current_session() {
            if let Ok(op) = f(&session) {
                let _ = op.join();
            }
        }
        unsafe {
            windows::Win32::System::Com::CoUninitialize();
        }
    });
}
