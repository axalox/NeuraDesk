//! Harnais de vérification visuelle (voir `gfx::capture`, doc de module) —
//! cette application est invisible à toute capture d'écran externe
//! (`SetWindowDisplayAffinity(..., WDA_EXCLUDEFROMCAPTURE)`), donc la seule
//! façon de VOIR ce qui est réellement rendu est de relire le back buffer
//! D3D11 depuis le processus lui-même, déclenché par ces variables
//! d'environnement (voir `app::run`, où elles sont lues). Aucune d'elles n'a
//! d'effet en usage normal (aucune n'est jamais positionnée par le
//! lanceur/raccourci habituel).
//!
//! - `NEURADESK_CAPTURE=<dossier>` : active le harnais, dossier de sortie
//!   des PNG (créé s'il n'existe pas).
//! - `NEURADESK_CAPTURE_FRAMES=0,5,20` : indices de frame (itérations de la
//!   boucle principale, `app::run`) à capturer — le dock à chaque frame
//!   listée, PLUS la fenêtre demandée par `NEURADESK_DEBUG_OPEN` si elle est
//!   ouverte à ce moment-là. Le process quitte proprement juste après avoir
//!   capturé la dernière frame listée (voir `is_last_frame`) plutôt que de
//!   tourner indéfiniment, pour un run scriptable.
//! - `NEURADESK_DEBUG_OPEN=dash|settings|clipboard|cef-…` : ouvre la fenêtre
//!   nommée par un appel de fonction DIRECT (`dash::toggle`/`settings_dash::
//!   toggle`/`clipboard_viewer::toggle`/`cef_context::toggle`…), jamais par une
//!   simulation d'entrée (`SendInput` etc.) — voir la contrainte de sécurité de
//!   session documentée dans la mémoire projet ("ne jamais simuler d'entrée
//!   pour tester cette app").
//! - `NEURADESK_DEBUG_OPEN_FRAME=40` : à partir de quelle frame l'ouvrir (défaut
//!   `0`, c'est-à-dire la première, le comportement historique).
//!   **NÉCESSAIRE POUR ÉPROUVER LE PRÉCHARGEMENT** (voir `win32::cef_popup::
//!   preload`) : les popups CEF sont préchargés un par frame à partir de la
//!   30ᵉ (`app::run`), donc un run qui ouvre à la frame 0 mesure le chemin NON
//!   préchargé — c'est-à-dire exactement celui que le préchargement remplace.
//!   Comparer les deux runs du même popup est ce qui rend le gain mesurable.
//! - `NEURADESK_DEBUG_OPEN_ANCRE=<id>` : pour `NEURADESK_DEBUG_OPEN=aide`
//!   seulement, amène la page sur la section dont le titre porte cet `id`, par
//!   `win32::cef_aide::ouvrir_a` — le CHEMIN DES BOUTONS DES RÉGLAGES qui
//!   renvoient vers l'aide, pas un chemin de faveur. Sans lui l'aide s'ouvre en
//!   haut et toute section au-delà du premier écran est hors champ : `#corps` est
//!   un ascenseur interne, et la molette du harnais ne fait PAS défiler ces
//!   ascenseurs (mesuré sur Réglages, cause jamais identifiée — voir
//!   `NEURADESK_DEBUG_SCROLL`, consommé par `settings` seul). C'est le seul moyen
//!   de PHOTOGRAPHIER une section basse de l'aide sans simuler d'entrée.
//! - `NEURADESK_DEBUG_FERMER_FRAME=300` : à cette frame, masquer les panneaux
//!   ouverts (`app::hide_overlay_windows`) — pour voir l'effet de fermeture
//!   (`win32::ouverture_fenetres::fermeture`) sans aucun clic.
//! - `NEURADESK_DEBUG_REOUVRIR_FRAME=400` : à cette frame, ROUVRIR le popup de
//!   `NEURADESK_DEBUG_OPEN` (même bras de code, une seconde fois). Sert à
//!   comparer la PREMIÈRE ouverture — qui part de l'estimation d'`app::menu_rect`
//!   — à la SECONDE, qui part de la taille que la page a demandée (voir
//!   `cef_popup::taille_rappelee`) : c'est le seul moyen de rendre ce contraste
//!   photographiable, aucun clic n'étant simulé. À combiner avec
//!   `NEURADESK_DEBUG_FERMER_FRAME`, placé AVANT.
//! - `NEURADESK_DEBUG_HOVER=<index>` : survole EN PERMANENCE l'icône du dock de
//!   cet index, en poussant directement sa position à `Dock::on_mouse_move` —
//!   même famille d'appel de fonction direct que `NEURADESK_DEBUG_OPEN`, jamais
//!   une simulation d'entrée. Sert à capturer une icône MAGNIFIÉE (et son
//!   balancement, voir `ui::dock::hover_wobble_angle`) sans qu'un doigt touche
//!   la souris.
//! - `NEURADESK_DEBUG_SCROLL=x,y,delta` : fait défiler la fenêtre ouverte par
//!   `NEURADESK_DEBUG_OPEN=settings` (seule consommatrice pour l'instant, voir
//!   `app::run`) via `win32::cef_popup::debug_scroll` — même famille
//!   d'appel de fonction direct que `debug_hover`/`debug_click`, pour
//!   vérifier du contenu qui dépasse la hauteur visible.
//! - `NEURADESK_DEBUG_CLICK=x,y` : clic direct (voir `win32::cef_popup::
//!   debug_click`) sur la fenêtre ouverte par `NEURADESK_DEBUG_OPEN=settings`
//!   (une seule fois à la frame 70, voir `app::run`) — pour vérifier qu'une case
//!   à cocher fait bien apparaître du contenu conditionnel sans avoir à deviner
//!   l'état par inspection du code seul. Le Terminal rejoint la liste (voir
//!   `win32::cef_terminal::debug_click`) pour cliquer une puce d'une autre sorte,
//!   un onglet, le ✕ d'un onglet. ⚠️ Il n'est **PAS** nécessaire pour avoir du
//!   texte à photographier : la page du Terminal ouvre un shell TOUTE SEULE quand
//!   elle devient visible (`window.terminal.montre` → `nouvelOnglet(state.shells[0].id)`),
//!   donc une capture vide est une capture prise AVANT `terminal/ready`, jamais le
//!   manque d'un clic. Avec `NEURADESK_DEBUG_BOUTON=milieu`, le clic du Terminal est un
//!   clic MOLETTE (la vue sphère, voir `win32::cef_popup::debug_middle_click`).
//!   **La Visionneuse rejoint la liste** (voir `win32::cef_visionneuse::debug_click`) :
//!   c'est le seul moyen d'éprouver ses flèches de voisinage de BOUT EN BOUT — la page
//!   reçoit l'événement, émet son verbe, le dispatch change de fichier. Elle clique à
//!   `open_frame + 900`, et `NEURADESK_DEBUG_FICHIER` doit désigner un fichier qui a
//!   des voisins **du même genre** dans son dossier, sinon il n'y a rien à cliquer.
//!   ⚠️ Comme partout ici, `x`/`y` sont des pixels **PHYSIQUES** (le chemin passe par
//!   `ScreenToClient`, sans division par le zoom) : un rect mesuré en CSS se multiplie
//!   par `devicePixelRatio`.
//!   **Le Lecteur de vidéos rejoint la liste** avec `NEURADESK_DEBUG_OPEN=lecteur` :
//!   la fenêtre est native (GDI + libVLC), donc **PAS photographiable** — elle est
//!   exclue de la capture externe (`WDA_EXCLUDEFROMCAPTURE`) et le harnais ne relire
//!   que les surfaces D3D11. Ce qui s'y éprouve est le VOISINAGE : à la même frame
//!   que la visionneuse, `NEURADESK_DEBUG_CLICK` est ignoré et
//!   `win32::lecteur_video::debug_fleche` est appelée directement (sans souris ni
//!   `SendInput`) ; `NEURADESK_DEBUG_BOUTON=gauche` demande le précédent, sinon le suivant. La preuve
//!   est dans le journal : `[lecteur_video] fleche : <titre> en lecture`. Il faut
//!   `NEURADESK_DEBUG_FICHIER` sur une vidéo qui a des voisins du même genre, et
//!   `NEURADESK_VLC` quand `libvlc.dll` n'est ni à côté de l'exe ni dans le VLC
//!   installé (voir `dossiers_vlc`). ⛔ **CE FICHIER DÉSIGNE LE LECTEUR ÉPROUVÉ**
//!   (`win32::lecteur_video::lecteur_du_harnais`) : la table des lecteurs est une HashMap
//!   et l'application est VIVANTE pendant un run — « le premier venu » pouvait être une
//!   fenêtre ouverte dans le dash dossier, et le click posté de `debug_surimpression`
//!   aurait changé un média de l'écran de l'utilisateur. Sans levier, ou si la seule
//!   fenêtre connue n'est pas ce fichier, la mesure s'arrête et le dit.
//!   ⭐ **`NEURADESK_DEBUG_CARTEL=gauche|droite` éprouve le CARTEL de surimpression**
//!   posé sur la vidéo (`win32::lecteur_video::debug_surimpression`) : il calcule l'état
//!   sur un point synthétique au centre du cartel, puis écrit au journal le `WindowFromPoint`
//!   de ce point AVEC la classe et le PID de ce qu'il trouve, plus le `WindowFromPoint`
//!   sous le bouton lecture de la barre — le hit-test RÉEL, seul à dire si un vrai doigt
//!   atteint le cartel par-dessus la chaîne d'échange de libVLC — ⚠️ la sonde REMONTE AU
//!   PRÉALABLE notre propre fenêtre (`HWND_TOP`, pas topmost) : le lecteur ne l'est pas en
//!   fenêtre, et sur les ~48 s d'un run une fenêtre tierce a le temps de se poser par-dessus
//!   (mesuré : WhatsApp.Root, un `HwndWrapper` tiers, Chrome) — `WindowFromPoint` répondait
//!   alors cette fenêtre aux DEUX points et « LE CARTEL N'EST PAS DESSUS » se lisait comme un
//!   défaut. Le journal écrit AUSSI si les deux cartels sont AFFICHÉS (`IsWindowVisible`) :
//!   mesure directe, que rien d'extérieur ne peut troubler — une fenêtre cachée ne reçoit pas
//!   le clic, donc la borne de la série (« le clic va à l'image, qui met en pause ») se prouve
//!   même quand le hit-test est bloqué. ⚠️ **Les deux points ne se
//!   recouvrent jamais** (en fenêtre la barre est SOUS l'image, en plein écran les cartels
//!   sont centrés) : la seconde mesure dit que la barre est devant L'IMAGE, pas devant le
//!   cartel — l'ordre cartel/barre n'est pas mesurable tel quel. Si la réponse est le cartel, `WM_LBUTTONDOWN` est POSTÉ à sa fenêtre (le
//!   message du vrai geste) et la preuve est la ligne de `changer_de_media` derrière ; si
//!   ce n'est pas lui, rien n'est posté et le journal le dit. Le cas limite se mesure en
//!   rejouant la MÊME frame sur le dernier fichier de la série : `montres=(true, false)`,
//!   `WindowFromPoint` répond `NdLecteurVideoImage` (un cartel caché ne vole pas le clic
//!   de la vidéo) et aucune ligne `fleche :`. Script : `C:/ndwork/harnais/cartel_run.ps1`.
//! - `NEURADESK_DEBUG_CLICK_FRAME=n` : la frame de ce clic. Indispensable pour le
//!   Terminal, dont la fenêtre s'ouvre à `NEURADESK_DEBUG_OPEN_FRAME` — la frame
//!   70 du bloc générique est antérieure à son ouverture. Sans cette variable, le
//!   Terminal clique à `open_frame + 1200`.
//! - `NEURADESK_DEBUG_DROIT=x,y` : **appui + relâchement du BOUTON DROIT** d'un coup
//!   (voir `cef_popup::debug_right_click`, qui reprend le geste réel de
//!   `WM_RBUTTONDOWN` : le `UP` n'arrive jamais aux fenêtres de popup, donc les deux
//!   sont émis ensemble et la page reçoit un vrai `contextmenu`). Il ne tire que pour
//!   `NEURADESK_DEBUG_OPEN=aide`, à `open_frame + 900`, donc AVANT le clic gauche :
//!   l'objet à vérifier est le menu que ce clic droit ouvre, et il faut ensuite
//!   cliquer SON élément avec `NEURADESK_DEBUG_CLICK`. Aucun input simulé
//!   (`SendInput`) : la même garantie que les autres leviers
//!   ([[neuradesk-live-input-safety]]).
//! - `NEURADESK_DEBUG_GLISSE=x0,y0,x1,y1` : **un GLISSEMENT qui SÉLECTIONNE du
//!   texte** (voir `cef_popup::debug_drag_select` : appui gauche en `x0,y0`, huit
//!   déplacements portant `EVENTFLAG_LEFT_MOUSE_BUTTON`, relâchement en `x1,y1`).
//!   C'est la sélection À LA SOURIS elle-même — avant ce levier, on ne pouvait prouver
//!   qu'une règle `user-select`, jamais le geste. Il ne tire que pour
//!   `NEURADESK_DEBUG_OPEN=aide`, à `open_frame + 600`, donc AVANT le bouton droit :
//!   le menu « Copier » n'existe que SUR une sélection.
//! - `NEURADESK_DEBUG_ZONE=right:0:120` : pose un SÉPARATEUR DE ZONE (voir
//!   `ui::dock::Icon::zone_height`) après l'élément de rang 0 de cette barre,
//!   avec cette cote — même famille d'appel de fonction direct que
//!   `debug_hover`/`debug_click`. Seul moyen de VOIR une zone rendue : elle ne
//!   se crée qu'au Maj + clic droit, et aucune entrée n'est jamais simulée ici.
//!   Sans effet sur la barre du haut (un espace n'y a pas de hauteur propre).
//!   Rien n'est persisté — l'espace et sa cote disparaissent à la fermeture.
//! - `NEURADESK_DEBUG_MAINTIEN=1` : **maintient le dash dossier OUVERT** dès le
//!   départ (la punaise de sa barre du haut, voir `win32::cef_sous_dock`).
//!   ⛔ **SANS LUI, CE PANNEAU NE SE CAPTURE PAS** : il se referme tout seul dès
//!   que le curseur réel n'est plus dessus (grâce de `PREVIEW_HIDE_GRACE`), donc
//!   la capture ne montre que le dernier rendu avant fermeture — et une campagne
//!   peut contredire la précédente sans que rien ne l'explique. C'est ce levier
//!   qui rend ce panneau enfin vérifiable, sans simuler la moindre entrée.
//! - `NEURADESK_DEBUG_DESKTOPS=<n>` : fabrique `n` bureaux virtuels au lieu de
//!   les lire du registre (voir `win32::desktops::current`, qui porte le
//!   levier) — le VRAI rendu dessine donc vingt pastilles, avec les numéros que
//!   `ui::desktop_labels` leur donne à partir du dixième. Sans elle, ce chemin
//!   ne se vérifie qu'en créant réellement dix bureaux, c'est-à-dire par un
//!   effet de bord sur une session Windows vivante.
//! - `NEURADESK_NO_APPBAR=1` : voir `win32::appbar::AppBar::register`, doc
//!   détaillée — sans elle, `guard_single_instance` refuse de démarrer un
//!   run de capture si une autre instance de NeuraDesk semble déjà active,
//!   plutôt que de risquer de rétrécir la zone de travail d'une VRAIE
//!   session Windows en cours.

use std::path::PathBuf;

use crate::ui::dock::Side;
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;

pub struct DebugCapture {
    dir: Option<PathBuf>,
    frames: Vec<u64>,
    pub open: Option<String>,
    /// `id` d'une section de l'aide sur laquelle amener la page — voir
    /// `NEURADESK_DEBUG_OPEN_ANCRE`, doc de module.
    pub open_ancre: Option<String>,
    pub scroll: Option<(i32, i32, i32)>,
    pub click: Option<(i32, i32)>,
    /// Index d'icône du DOCK à survoler en permanence — voir
    /// `NEURADESK_DEBUG_HOVER`, doc de module.
    pub hover: Option<usize>,
    /// `barre:rang:cote` — pose un SÉPARATEUR DE ZONE à ce rang de cette
    /// barre, voir `NEURADESK_DEBUG_ZONE`, doc de module.
    pub zone: Option<(Side, usize, f32)>,
    /// **MAINTIENT LE DASH DOSSIER OUVERT** — voir `NEURADESK_DEBUG_MAINTIEN`,
    /// doc de module. Sans lui, ce panneau se referme tout seul et sa capture ne
    /// montre que le dernier rendu.
    pub maintien: bool,
    /// Frame à partir de laquelle ouvrir la fenêtre de `NEURADESK_DEBUG_OPEN` —
    /// voir cette variable, doc de module, et pourquoi elle existe (éprouver le
    /// préchargement). `0` = dès la première frame.
    pub open_frame: u64,
    /// Frame à laquelle MASQUER les panneaux ouverts — voir
    /// `NEURADESK_DEBUG_FERMER_FRAME`, doc de module : le seul moyen de voir
    /// l'effet de fermeture, puisque le harnais ne simule aucun clic.
    pub fermer_frame: Option<u64>,
    /// Frame à laquelle ROUVRIR le popup de `NEURADESK_DEBUG_OPEN` — voir
    /// `NEURADESK_DEBUG_REOUVRIR_FRAME`, doc de module. La première ouverture
    /// part de l'ESTIMATION (aucune mesure antérieure) ; la seconde part de la
    /// taille que la page a demandée — c'est ce contraste que ce levier existe
    /// pour rendre capturable.
    pub reouvrir_frame: Option<u64>,
    /// Frame du clic `NEURADESK_DEBUG_CLICK` — voir la doc de ce levier : les
    /// fenêtres qui s'ouvrent à `open_frame` ont besoin d'un clic APRÈS leur
    /// ouverture, et la frame 70 du bloc générique ne suffit pas.
    pub click_frame: Option<u64>,
    /// `NEURADESK_DEBUG_DROIT`, doc de module : le bouton droit, appui et
    /// relâchement d'un coup, pour faire naître un vrai `contextmenu` en page.
    pub droit: Option<(i32, i32)>,
    /// `NEURADESK_DEBUG_GLISSE=x0,y0,x1,y1`, doc de module : un GLISSEMENT qui
    /// sélectionne du texte en page — la sélection à la souris elle-même, sans souris.
    pub glisse: Option<(i32, i32, i32, i32)>,
}

impl DebugCapture {
    pub fn from_env() -> Self {
        let dir = std::env::var_os("NEURADESK_CAPTURE").map(PathBuf::from);
        let frames = std::env::var("NEURADESK_CAPTURE_FRAMES")
            .ok()
            .map(|raw| raw.split(',').filter_map(|f| f.trim().parse::<u64>().ok()).collect())
            .unwrap_or_default();
        let open = std::env::var("NEURADESK_DEBUG_OPEN").ok();
        let open_ancre = std::env::var("NEURADESK_DEBUG_OPEN_ANCRE").ok().filter(|v| !v.trim().is_empty());
        let scroll = std::env::var("NEURADESK_DEBUG_SCROLL").ok().and_then(|raw| {
            let mut parts = raw.split(',').filter_map(|p| p.trim().parse::<i32>().ok());
            Some((parts.next()?, parts.next()?, parts.next()?))
        });
        let click = std::env::var("NEURADESK_DEBUG_CLICK").ok().and_then(|raw| {
            let mut parts = raw.split(',').filter_map(|p| p.trim().parse::<i32>().ok());
            Some((parts.next()?, parts.next()?))
        });
        let droit = std::env::var("NEURADESK_DEBUG_DROIT").ok().and_then(|raw| {
            let mut parts = raw.split(',').filter_map(|p| p.trim().parse::<i32>().ok());
            Some((parts.next()?, parts.next()?))
        });
        let glisse = std::env::var("NEURADESK_DEBUG_GLISSE").ok().and_then(|raw| {
            let mut parts = raw.split(',').filter_map(|p| p.trim().parse::<i32>().ok());
            Some((parts.next()?, parts.next()?, parts.next()?, parts.next()?))
        });
        let hover = std::env::var("NEURADESK_DEBUG_HOVER").ok().and_then(|raw| raw.trim().parse::<usize>().ok());
        let zone = std::env::var("NEURADESK_DEBUG_ZONE").ok().and_then(|raw| {
            let mut parts = raw.split(':').map(|part| part.trim().to_string());
            let side = match parts.next()?.to_ascii_lowercase().as_str() {
                "left" | "gauche" => Side::Left,
                "right" | "droite" => Side::Right,
                _ => return None,
            };
            Some((side, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
        });
        let open_frame = std::env::var("NEURADESK_DEBUG_OPEN_FRAME")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .unwrap_or(0);
        let maintien = std::env::var("NEURADESK_DEBUG_MAINTIEN").is_ok_and(|v| v.trim() == "1");
        let fermer_frame = std::env::var("NEURADESK_DEBUG_FERMER_FRAME").ok().and_then(|raw| raw.trim().parse::<u64>().ok());
        let reouvrir_frame = std::env::var("NEURADESK_DEBUG_REOUVRIR_FRAME").ok().and_then(|raw| raw.trim().parse::<u64>().ok());
        let click_frame = std::env::var("NEURADESK_DEBUG_CLICK_FRAME").ok().and_then(|raw| raw.trim().parse::<u64>().ok());
        Self {
            dir,
            frames,
            open,
            open_ancre,
            open_frame,
            fermer_frame,
            reouvrir_frame,
            click_frame,
            droit,
            glisse,
            scroll,
            click,
            hover,
            zone,
            maintien,
        }
    }

    pub fn active(&self) -> bool {
        self.dir.is_some()
    }

    pub fn should_capture(&self, frame: u64) -> bool {
        self.active() && self.frames.contains(&frame)
    }

    /// `true` une fois la DERNIÈRE frame demandée passée — signal pour
    /// `app::run` de quitter proprement plutôt que de laisser tourner un
    /// process de test indéfiniment (voir la doc de module).
    pub fn is_last_frame(&self, frame: u64) -> bool {
        self.active() && self.frames.iter().max().is_some_and(|&max| frame >= max)
    }

    /// Chemin de sortie pour la capture de `window_name` ("dock"/"dash"/
    /// "settings"/"clipboard") à `frame` — `None` si le harnais n'est pas
    /// actif.
    pub fn frame_path(&self, window_name: &str, frame: u64) -> Option<PathBuf> {
        self.dir.as_ref().map(|dir| dir.join(format!("{window_name}_{frame:04}.png")))
    }
}

/// Refuse de démarrer si une autre instance de NeuraDesk semble déjà en
/// cours d'exécution ET que `NEURADESK_NO_APPBAR` n'est PAS positionnée —
/// voir la doc de module. Sans cette garde, un run de capture lancé pendant
/// une vraie session enregistrerait un second jeu d'AppBars (voir
/// `win32::appbar`) et rétrécirait la zone de travail Windows de cette
/// session — un effet de bord live non voulu. Réservé aux runs de capture :
/// `app::run` ne l'appelle QUE si `DebugCapture::active()`, jamais lors d'un
/// lancement normal.
///
/// Best-effort : `CreateMutexW` échoue rarement (permissions...), auquel
/// cas on laisse démarrer plutôt que de bloquer un run légitime sur une
/// incertitude — la vraie garantie de sécurité reste `NEURADESK_NO_APPBAR`
/// lui-même (voir `AppBar::register`), cette fonction n'est qu'un filet
/// supplémentaire pour l'usage courant du harnais.
pub fn guard_single_instance() {
    if std::env::var_os("NEURADESK_NO_APPBAR").is_some() {
        return;
    }
    unsafe {
        let Ok(handle) = CreateMutexW(None, false, w!("NeuraDeskSingleInstanceGuard")) else {
            return;
        };
        // `CreateMutexW` réussit ET renvoie un handle valide même si le
        // mutex nommé existait déjà (créé par une autre instance) — c'est
        // `GetLastError` qui distingue les deux cas, PAS le `Result` lui-même.
        let already_running = windows::Win32::Foundation::GetLastError() == ERROR_ALREADY_EXISTS;
        let _ = CloseHandle(handle);
        if already_running {
            eprintln!(
                "[neuradesk][capture] une autre instance de NeuraDesk semble déjà en cours : \
                 lancer un run de capture sans NEURADESK_NO_APPBAR=1 rétrécirait la zone de \
                 travail de cette session réelle. Refus de démarrer — relancez avec \
                 NEURADESK_NO_APPBAR=1."
            );
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    /// Un levier PARSE mais jamais CONSOMMÉ est une « ligne fantôme qui compile »
    /// ([[cablage-applet-remplacement-octet]]) : la variable d'environnement est lue,
    /// le champ existe, et le geste ne part jamais. Ce test lit la SOURCE de `app.rs` et
    /// exige que les trois gestes de la chaîne « sélectionner → clic droit → Copier » de
    /// l'aide soient chacun branchés sur son champ ET sur sa fonction.
    #[test]
    fn les_trois_gestes_de_la_chaine_copier_sont_cables() {
        let source = include_str!("../app.rs");
        // LE BLOC, PAS LE FICHIER : `debug_capture.click` est lu par d'autres blocs
        // (Réglages, Terminal, Scanner), donc un scan global passerait même si le bloc
        // de l'aide disparaissait (piège [[neuradesk-l-argument-entre-deux-tests]]).
        let apres = source
            .split(r#"debug_capture.open.as_deref() == Some("aide")"#)
            .nth(1)
            .unwrap_or_default();
        let bloc: String = apres.lines().take(20).collect();
        for (champ, fonction) in [
            ("debug_capture.glisse", "cef_aide::debug_glisse"),
            ("debug_capture.droit", "cef_aide::debug_clic_droit"),
            ("debug_capture.click", "cef_aide::debug_clic_gauche"),
        ] {
            assert!(bloc.contains(champ), "le champ {champ} n'est pas lu dans le bloc de l'aide : le levier serait parse mais jamais consommé");
            assert!(bloc.contains(fonction), "{fonction} n'est pas appelé dans le bloc de l'aide");
        }
        // L'ORDRE DES FRAMES EST LE GESTE HUMAIN, mesuré en page par la sonde : le menu
        // « Copier » n'existe QUE sur une sélection (`menu_contextuel.js` teste
        // `getSelection()`), donc le glissement part avant le bouton droit ; et le clic
        // gauche vise L'ÉLÉMENT DU MENU, donc après lui.
        let place = |cote: &str| bloc.find(cote).unwrap_or(usize::MAX);
        assert!(place("+ 600") < place("+ 900") && place("+ 900") < place("+ 1200"), "les frames des trois gestes ne sont pas dans l'ordre glisse(600) → droit(900) → clic(1200)");
    }

    /// Le harnais ne se découvre QUE par cette doc de module : un levier qui n'y est
    /// pas écrit est un levier perdu pour la prochaine session.
    #[test]
    fn chaque_levier_de_souris_est_cite_dans_la_doc() {
        let doc = include_str!("debug_capture.rs").split("use std::path::PathBuf;").next().unwrap_or_default();
        for var in ["NEURADESK_DEBUG_GLISSE", "NEURADESK_DEBUG_DROIT", "NEURADESK_DEBUG_CLICK", "NEURADESK_DEBUG_CLICK_FRAME"] {
            assert!(doc.contains(var), "la doc de module ne cite pas {var} — le harnais ne le retrouvera pas");
        }
    }
}
