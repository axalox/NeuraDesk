//! État global "Visible dans les captures d'écran" — DEMANDE EXPLICITE
//! ("j'aimerais avoir une option dans les réglages qui permette de rendre
//! l'interface visible sur les captures d'écran et qui mettent les
//! transparences à 0% pour éviter les bugs visuels dus à la transparence").
//!
//! Par défaut DÉSACTIVÉ (comportement d'origine, voir la doc de
//! `gfx::capture`/`gfx::backdrop`) : chaque fenêtre de l'interface
//! (`win32::window::Window`/`SecondaryWindow`, popups CEF, tooltip,
//! horloge plein écran) s'exclut elle-même de toute capture d'écran via
//! `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)`. Activé,
//! l'utilisateur accepte que l'interface apparaisse dans un enregistrement/
//! partage d'écran.
//!
//! ⭐ **LA TRANSPARENCE ET LE FLOU RESTENT CEUX RÉGLÉS** — DEMANDES EXPLICITES
//! du 2026-11-12 : « rendre l'app visible sur les captures même quand la
//! transparence est activée », puis « quand Rendre l'interface visible sur les
//! captures d'écran est activé, le flou doit aussi être activé ». Les versions
//! antérieures forçaient toutes les teintes à une opacité de 100 % et le flou
//! à zéro dans ce mode. Ce compromis est RETIRÉ.
//!
//! Le fond flouté vient de `gfx::mag_capture`, une loupe dont le filtre de
//! fenêtres écarte NOS fenêtres de NOTRE capture uniquement : les captureurs
//! externes (OBS, Capture d'écran) continuent de voir l'interface, et le fond
//! flouté est le vrai bureau. Le rendu est donc celui du mode normal (fond
//! flouté + teinte réglée). Si cette loupe est indisponible sur la machine,
//! le shader retombe sur la teinte seule avec l'alpha de l'utilisateur
//! (drapeau `Ombres.y`, voir `gfx::quad_renderer`).
//!
//! Chaque fenêtre lit `window_display_affinity()` à sa CRÉATION, mais une
//! fenêtre déjà ouverte ne peut pas être recréée à chaque bascule du réglage :
//! la bascule à chaud est donc complétée par `reapply_to_all_windows()`,
//! appelée depuis `app::run` (voir sa doc pour le bug réel qu'elle corrige).
//!
//! **Second levier, indissociable du premier** : `WDA_NONE` seul ne suffit
//! PAS à apparaître dans la liste des fenêtres d'OBS — les fenêtres
//! TRANSPARENTES de l'interface (menus ancrés, infobulle, horloge plein écran)
//! sont créées avec `WS_EX_TOOLWINDOW`, et ce style les exclut AUSSI de cette
//! liste, quelle que soit leur affinité. Le réglage actif le retire donc
//! (`set_toolwindow`), et le remet à la désactivation.
//! DEMANDE EXPLICITE : « j'aimerais que l'app apparaisse comme un programme
//! normal pour la voir dans la liste des programmes ouverts d'obs ».
//!
//! Les fenêtres de BARRES (dock primaire et secondaires) ne participent PAS à
//! ce va-et-vient : elles sont créées sans `WS_EX_TOOLWINDOW` et avec
//! `WS_EX_APPWINDOW` (`win32::window::DOCK_EX_STYLE`), et `set_toolwindow` les
//! laisse telles quelles (voir `toolwindow_applied`). Retirer le style suffisait
//! pour OBS, mais laissait une barre `WS_EX_NOACTIVATE` absente d'Alt-Tab ET de
//! la barre des tâches — un défaut, voir `DOCK_EX_STYLE`.
//!
//! État tenu dans un `AtomicBool` GLOBAL (pas `thread_local` comme
//! `blur::BLUR_PASSES`/`win32::menu::MENU_TINT`) : contrairement à ces
//! deux-là, lus uniquement depuis le thread de rendu, `is_enabled` doit
//! aussi être consulté au moment de la création d'une fenêtre CEF (popups),
//! qui peut se produire sur un thread distinct du thread principal (voir
//! `win32::cef_app`, doc de module). Vit dans `gfx` (pas `win32`, malgré
//! `SetWindowDisplayAffinity`/l'essentiel de ses appelants) pour que
//! `gfx::blur`/`gfx::backdrop` — qui n'ont aucune dépendance vers `win32`,
//! sens de dépendance à sens unique dans ce projet — puissent le consulter
//! directement sans créer de dépendance circulaire.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowLongPtrW, GetWindowThreadProcessId, SetWindowDisplayAffinity,
    SetWindowLongPtrW, GWL_EXSTYLE, WDA_EXCLUDEFROMCAPTURE, WDA_NONE, WINDOW_DISPLAY_AFFINITY, WS_EX_APPWINDOW,
    WS_EX_TOOLWINDOW,
};

static CAPTURE_VISIBLE: AtomicBool = AtomicBool::new(false);

pub fn is_enabled() -> bool {
    CAPTURE_VISIBLE.load(Ordering::Relaxed)
}

/// **L'INTERFACE FILMÉE LE TEMPS D'UN ENREGISTREMENT** — posé par l'applet
/// Enregistrer lancée au clic DROIT (voir `cef_enregistrer::
/// interface_dans_l_enregistrement`). Il ne touche QUE l'affinité : ni
/// l'opacité, ni le flou, ni `WS_EX_TOOLWINDOW` — ceux-là suivent le réglage
/// « visible dans les captures », que ce forçage ne doit pas imiter (les
/// fenêtres apparaîtraient dans Alt-Tab le temps d'une vidéo).
static FORCE_CAPTURABLE: AtomicBool = AtomicBool::new(false);

pub fn est_force() -> bool {
    FORCE_CAPTURABLE.load(Ordering::Relaxed)
}

/// À suivre de `reapply_to_all_windows` : les fenêtres ouvertes ne relisent
/// pas l'affinité d'elles-mêmes.
pub fn forcer_capturable(force: bool) {
    FORCE_CAPTURABLE.store(force, Ordering::Relaxed);
}

pub fn set_enabled(enabled: bool) {
    CAPTURE_VISIBLE.store(enabled, Ordering::Relaxed);
}

/// Nos fenêtres sont-elles visibles pour les captureurs EXTERNES ? Vrai si le
/// réglage est actif, ou si l'applet Enregistrer force la visibilité le temps
/// d'une vidéo (voir `est_force`).
///
/// C'est CE prédicat — pas `is_enabled` seul — que doivent consulter le fond
/// flouté et le shader : dans les deux cas les fenêtres sont en `WDA_NONE`,
/// donc notre propre Desktop Duplication les verrait et se relirait en blanc.
pub fn est_capturable() -> bool {
    is_enabled() || est_force()
}

/// Affinité à appliquer à une fenêtre de l'interface fraîchement créée (voir
/// tous les appels `SetWindowDisplayAffinity` du projet) — centralise le
/// choix ici pour qu'une future fenêtre de l'interface suive automatiquement
/// ce réglage sans avoir à réimplémenter la même condition.
pub fn window_display_affinity() -> WINDOW_DISPLAY_AFFINITY {
    if est_capturable() {
        WDA_NONE
    } else {
        WDA_EXCLUDEFROMCAPTURE
    }
}

/// Repose sur `hwnd` les DEUX attributs de capture que ce réglage pilote —
/// affinité d'affichage ET exclusion de la Desktop Duplication (voir la doc
/// de module). Point d'application UNIQUE à utiliser partout où une fenêtre de
/// l'interface vient d'être créée ; les deux attributs doivent toujours être
/// posés ensemble, un site qui n'en poserait qu'un laisserait la fenêtre
/// capturable par notre propre capture de bureau.
///
/// Les sites de création doivent passer par ici plutôt que d'appeler
/// `SetWindowDisplayAffinity` directement — c'est cette omission, à l'origine,
/// qui rendait le rectangle parasite possible au démarrage avec le réglage
/// déjà actif (voir `reapply_to_all_windows`, qui couvre la bascule à chaud).
pub fn apply_to_window(hwnd: HWND) {
    let _ = unsafe { SetWindowDisplayAffinity(hwnd, window_display_affinity()) };
    if is_enabled() {
        set_toolwindow(hwnd, false);
    }
}

/// Point d'appel unique de l'alpha de toutes les teintes de fond de
/// l'interface (`app::run`, `dock_instance::DockInstance::render`).
///
/// ⭐ **LE MODE CAPTURABLE N'Y TOUCHE PLUS** (DEMANDE EXPLICITE 2026-11-12,
/// voir la doc de module) : la transparence réglée reste effective, c'est le
/// shader qui compose avec le bureau derrière elle.
///
/// Ce qui reste ici, c'est le PLANCHER du mode clair (voir
/// `ui::color::TEINTE_MIN_MODE_CLAIR`) — sans rien persister : la transparence
/// réglée revient telle quelle dès qu'on quitte le thème « Windows clair ».
pub fn tint_alpha(configured: f32) -> f32 {
    tint_alpha_selon(configured, crate::ui::color::mode_clair())
}

fn tint_alpha_selon(configured: f32, mode_clair: bool) -> f32 {
    if mode_clair {
        configured.max(crate::ui::color::TEINTE_MIN_MODE_CLAIR)
    } else {
        configured
    }
}

/// Affinité à (re)poser pendant l'énumération de `reapply_to_all_windows` —
/// `EnumWindows` n'accepte qu'un pointeur de fonction, jamais une closure
/// capturante, donc la valeur à appliquer transite par ce global plutôt que
/// par un paramètre.
static PENDING_AFFINITY: AtomicU32 = AtomicU32::new(WDA_EXCLUDEFROMCAPTURE.0);

/// Idem pour le retrait de `WS_EX_TOOLWINDOW` (voir `set_toolwindow`).
static PENDING_CAPTURABLE: AtomicBool = AtomicBool::new(false);

/// Ajoute ou retire `WS_EX_TOOLWINDOW` sur `hwnd` — le style qui EXCLUT une
/// fenêtre d'Alt-Tab, de la barre des tâches et **de la liste des fenêtres
/// d'OBS** (DEMANDE EXPLICITE : « j'aimerais que l'app apparaisse comme un
/// programme normal pour la voir dans la liste des programmes ouverts
/// d'obs »).
///
/// Les fenêtres TRANSPARENTES de l'interface (menus ancrés, infobulle, horloge
/// plein écran) sont créées avec ce style : c'est ce qui les empêche de polluer
/// Alt-Tab/la barre des tâches en usage normal. Mais tant qu'il est posé, OBS
/// ne les liste PAS du tout — quelle que soit leur affinité d'affichage. Ce
/// réglage le retire donc le temps qu'il est actif, et le remet ensuite.
///
/// ⚠️ **CE N'EST PAS CE CHEMIN QUI FAIT ENTRER UNE FENÊTRE DANS ALT-TAB, ET
/// L'AVOIR CRU A COÛTÉ UN DÉFAUT RÉEL.** Ce commentaire annonçait ici un
/// « effet de bord assumé » : le temps du mode, les fenêtres de l'interface
/// apparaîtraient dans Alt-Tab et la barre des tâches. **C'est FAUX pour toute
/// fenêtre qui porte `WS_EX_NOACTIVATE`** — et l'affirmation n'avait jamais été
/// mesurée. Le MSDN dit de ce style : « *The window does not appear on the
/// taskbar by default. To force the window to appear on the taskbar, use the
/// `WS_EX_APPWINDOW` style* » — un levier que ce module ne pose jamais.
/// SIGNALÉ PAR L'UTILISATEUR le 2026-09-20 (« je ne vois pas neuradesk dans le
/// alt-tab »). Retirer `WS_EX_TOOLWINDOW` reste NÉCESSAIRE pour OBS, mais c'est
/// INSUFFISANT pour le shell. Les barres ne dépendent donc plus du tout de ce
/// chemin : voir `win32::window::DOCK_EX_STYLE`, et `toolwindow_applied`
/// ci-dessous, qui refuse désormais de toucher à une telle fenêtre.
///
/// **AUCUN `ShowWindow` de rafraîchissement ici.** La version initiale forçait
/// un cycle `SW_HIDE`/`SW_SHOWNOACTIVATE` après le changement de style, en
/// croyant que le shell n'en tenait compte qu'après un changement de
/// visibilité — ce qui ré-affichait chaque fenêtre visible à chaque bascule,
/// pour un scintillement gratuit.
///
/// Ce cycle a été retiré en croyant corriger le « rectangle blanc » ; **ce
/// n'était PAS la cause** — le blanc est resté après ce retrait. La vraie
/// cause est documentée sur `is_own_ui_window` (l'affinité d'affichage
/// appliquée par PID à une fenêtre interne cachée de CEF).
///
/// Le retrait est conservé parce qu'il est sans perte : la liste des fenêtres
/// d'OBS se lit directement sur les styles, sans rafraîchissement du shell —
/// vérifié après coup, l'app apparaît toujours dans OBS.
///
/// Conséquence acceptée : la barre des tâches / Alt-Tab peuvent ne pas
/// refléter le changement avant un événement shell ultérieur — sans effet sur
/// la visibilité dans OBS, qui est la seule raison d'être de ce style ici.
fn set_toolwindow(hwnd: HWND, enabled: bool) {
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let updated = toolwindow_applied(current, enabled);
        if updated != current {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, updated);
        }
    }
}

/// Vrai si `ex_style` désigne une fenêtre qui a DÉLIBÉRÉMENT demandé à rester
/// une fenêtre normale pour le shell (`WS_EX_APPWINDOW`, voir
/// `win32::window::DOCK_EX_STYLE`) — aujourd'hui les barres, et elles seules.
///
/// Ces fenêtres sont hors du va-et-vient de `WS_EX_TOOLWINDOW`, dans les DEUX
/// sens. Le sens « on remet le style » est le moins évident, et c'est celui qui
/// compte : une bascule du réglage VERS LA DÉSACTIVATION aurait replacé
/// `WS_EX_TOOLWINDOW` sur les barres et les aurait fait disparaître d'Alt-Tab et
/// de la barre des tâches — alors que la demande est qu'elles y soient
/// TOUJOURS (voir `DOCK_EX_STYLE`).
///
/// S'appuie sur le STYLE, pas sur une liste de noms de classes : la règle est
/// « une fenêtre qui a demandé à être vue du shell ne se laisse pas re-cacher »,
/// ce qui reste vrai pour toute fenêtre future qui poserait ce style, sans liste
/// à tenir à jour.
fn keeps_shell_presence(ex_style: isize) -> bool {
    ex_style as u32 & WS_EX_APPWINDOW.0 != 0
}

/// Résultat de la pose/retrait de `WS_EX_TOOLWINDOW` sur un style EXISTANT —
/// extrait de `set_toolwindow` pour que la règle, exception comprise, soit
/// testable sans fenêtre (idiome du dépôt : la fonction est pure, l'appelant
/// porte la valeur). Rend `current` INCHANGÉ pour une fenêtre qui garde sa
/// présence auprès du shell (voir `keeps_shell_presence`).
///
/// L'ancienne version calculait `WS_EX_TOOLWINDOW & !WS_EX_APPWINDOW` — un
/// **no-op**, les deux bits étant disjoints, donc un masque qui ne masquait
/// rien : la trace qu'`APPWINDOW` avait été envisagé sans jamais être posé.
fn toolwindow_applied(current: isize, enabled: bool) -> isize {
    if keeps_shell_presence(current) {
        return current;
    }
    if enabled {
        current | WS_EX_TOOLWINDOW.0 as isize
    } else {
        current & !(WS_EX_TOOLWINDOW.0 as isize)
    }
}

/// Vrai si `hwnd` est une fenêtre de NOTRE interface — c'est-à-dire de classe
/// `NeuraDesk*` : dock, docks secondaires, popups CEF, infobulle, horloge
/// plein écran, fenêtres d'aide AppBar. Toutes les fenêtres VISIBLES de
/// l'interface sont dans ce cas ; c'est l'invariant qui rend ce filtre sûr.
///
/// POURQUOI CE FILTRE — bug réel corrigé ici (« un grand rectangle blanc
/// apparaît sur mon écran principal quand j'active la visibilité dans les
/// captures »). `reapply_to_all_windows` ne filtrait que par PID, donc
/// touchait AUSSI les fenêtres internes de CEF (`Chrome_WidgetWin_0`,
/// `Chrome_StatusTrayWindow`) et celles que le système crée dans notre
/// processus (`IME`, `MSCTFIME UI`, `Base_PowerMessageWindow`). Or changer
/// l'affinité d'affichage d'une fenêtre force DWM à recréer sa surface de
/// redirection ; pour `Chrome_WidgetWin_0` — cachée en permanence, jamais
/// peinte — cette surface est vide, donc BLANCHE, et DWM la compose malgré
/// tout. D'où un rectangle blanc correspondant à l'aire CLIENTE de cette
/// fenêtre (mesuré `(468,504)-(3324,1980)` en physique, écart de 12 px de
/// bordure et 48 px de barre de titre avec son rectangle), visible à l'œil et
/// dans OBS, mais absent d'`EnumWindows` (la fenêtre est cachée) et du
/// hit-test (une fenêtre cachée ne reçoit aucun clic).
///
/// Établi par élimination : une instrumentation TEMPORAIRE du rendu du dock
/// (journalisant tout quad de plus de 200 000 px², depuis retirée) a montré
/// que sur toute une bascule, le seul quad dessiné est la barre du haut
/// `(0,0) 3840x72` — AUCUN quad ne couvre la zone blanche, alors qu'un tel
/// quad (aire 1,87 M) aurait forcément été journalisé. Le blanc ne venait donc
/// pas de notre rendu, mais de ce que nous faisions à une fenêtre qui n'est
/// pas la nôtre.
///
/// Confirmé après correctif : quatre bascules consécutives ne reproduisent
/// plus le rectangle — 0 pixel quasi-blanc sur 2627 échantillonnés dans la
/// zone, contre un blanc pur et continu avant.
///
/// Aucune perte : ces fenêtres ne s'affichent jamais, leur affinité
/// d'affichage est donc sans objet. Ce filtre par CLASSE est délibéré — un
/// simple « ne pas toucher aux fenêtres cachées » aurait été plus court mais
/// faux : il aurait réintroduit le bug des popups ouverts AVANT l'activation
/// du réglage, qui restent cachés au moment de la bascule (voir
/// `reapply_to_all_windows`).
fn is_own_ui_window(hwnd: HWND) -> bool {
    let mut class_name_buf = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut class_name_buf) };
    if len <= 0 {
        return false;
    }
    let class_name = String::from_utf16_lossy(&class_name_buf[..len as usize]);
    class_name.starts_with("NeuraDesk")
}

/// TOUTES nos fenêtres d'interface actuellement vivantes (dock, docks
/// secondaires, popups CEF, menus, infobulle, horloge, aperçus) — voir
/// `is_own_ui_window` pour le critère, et `reapply_to_all_windows` pour
/// pourquoi une énumération est le seul moyen de les atteindre toutes.
///
/// Sert au filtre de la capture par loupe (`gfx::mag_capture`) : c'est cette
/// liste-là qui retranche nos fenêtres de NOTRE capture de bureau, sans les
/// retirer de celles des autres applications.
pub(crate) fn own_ui_windows() -> Vec<HWND> {
    let mut fenetres: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(collecter_fenetres_proc),
            LPARAM(&mut fenetres as *mut Vec<HWND> as isize),
        );
    }
    fenetres
}

unsafe extern "system" fn collecter_fenetres_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let mut window_pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut window_pid)) };
    if window_pid == std::process::id() && is_own_ui_window(hwnd) {
        // Le vecteur vit dans l'appelant (voir `own_ui_windows`), seul moyen
        // de faire remonter plus qu'un booléen par un callback Win32.
        unsafe { (*(lparam.0 as *mut Vec<HWND>)).push(hwnd) };
    }
    BOOL(1)
}

/// Callback d'énumération de `reapply_to_all_windows`.
unsafe extern "system" fn reapply_affinity_proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
    let mut window_pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut window_pid)) };
    if window_pid == std::process::id() && is_own_ui_window(hwnd) {
        let _ = unsafe {
            SetWindowDisplayAffinity(hwnd, WINDOW_DISPLAY_AFFINITY(PENDING_AFFINITY.load(Ordering::Relaxed)))
        };
        // `set_toolwindow(hwnd, !capturable)` : le style doit être RETIRÉ
        // quand le réglage est actif et REMIS quand il ne l'est plus — les
        // deux sens comptent, une réapplication à sens unique laisserait les
        // fenêtres dans Alt-Tab/la barre des tâches après désactivation.
        set_toolwindow(hwnd, !PENDING_CAPTURABLE.load(Ordering::Relaxed));
    }
    // Continue l'énumération — jamais d'arrêt anticipé, TOUTES les fenêtres de
    // l'interface doivent être traitées.
    BOOL(1)
}

/// Repose l'affinité de capture actuelle sur TOUTES les fenêtres de
/// l'interface déjà existantes — indispensable à la bascule live du réglage,
/// voir la doc de module de `gfx::capture_visibility` et le commentaire
/// d'appel dans `app::run`.
///
/// POURQUOI une énumération plutôt que les `HWND` déjà connus d'`app::run` :
/// `Window::create_dock`/`SecondaryWindow::create` posent bien leur affinité
/// à la création, mais les fenêtres créées À LA DEMANDE (popups CEF Dash/
/// Réglages/Presse-papiers/Calendrier/Contexte/Réseau/Son/Voicemeeter/
/// SpacerSize/BarSize, infobulle, horloge plein écran) ne la relisent JAMAIS
/// après coup, et leurs `HWND` vivent dans un `thread_local! PopupSlot` par
/// popup (voir `win32::cef_popup`) — sans registre central, seul
/// `EnumWindows` les atteint toutes.
///
/// BUG RÉEL CORRIGÉ ICI (DEMANDE EXPLICITE : « je voudrais que l'option :
/// rendre visible dans les captures d'écran, permette de tout capturer,
/// actuellement il n'y a que l'interface principale qui est prise en compte
/// quand je veux capturer une vidéo avec obs ») : la réapplication ne
/// couvrait que les deux fenêtres de dock, si bien qu'un popup déjà ouvert
/// gardait l'affinité de son instant de création — un popup ouvert AVANT
/// l'activation du réglage restait donc invisible dans OBS après la bascule.
/// Vérifié par mesure directe : `NeuraDeskCefSettingsWindow` était en
/// `WDA_NONE` (capturable) alors que le réglage était DÉSACTIVÉ.
///
/// Le filtre est `PID == le nôtre` **ET** classe `NeuraDesk*` (voir
/// `is_own_ui_window`, qui documente le rectangle blanc que le filtre par PID
/// seul laissait passer). Il inclut donc `NeuraDeskAppBarHelperWindow` (voir
/// `win32::window::create_appbar_helper`) : invisible et sans surface de
/// rendu, elle est neutre ici.
pub fn reapply_to_all_windows() {
    PENDING_AFFINITY.store(window_display_affinity().0, Ordering::Relaxed);
    // Figé ici, comme l'affinité — voir le commentaire du callback.
    PENDING_CAPTURABLE.store(is_enabled(), Ordering::Relaxed);
    let _ = unsafe { EnumWindows(Some(reapply_affinity_proc), LPARAM(0)) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{WS_EX_NOACTIVATE, WS_EX_TOPMOST};

    /// Le forçage de l'applet Enregistrer rend l'interface capturable SANS
    /// toucher au réglage (opacité, flou, Alt-Tab le lisent, eux).
    #[test]
    fn le_forcage_rend_capturable_sans_toucher_au_reglage() {
        let avant = is_enabled();
        set_enabled(false);
        assert_eq!(window_display_affinity(), WDA_EXCLUDEFROMCAPTURE);
        forcer_capturable(true);
        assert_eq!(window_display_affinity(), WDA_NONE);
        assert!(!is_enabled(), "le réglage reste éteint");
        forcer_capturable(false);
        assert_eq!(window_display_affinity(), WDA_EXCLUDEFROMCAPTURE);
        set_enabled(avant);
    }

    /// Le mode clair RELÈVE une teinte trop transparente sans jamais baisser
    /// une teinte plus opaque — et il ne dépend plus du tout du mode
    /// capturable, qui ne doit plus écraser la transparence réglée
    /// (DEMANDE EXPLICITE 2026-11-12).
    #[test]
    fn le_mode_clair_pose_un_plancher_d_opacite_sans_regarder_la_capture() {
        let plancher = crate::ui::color::TEINTE_MIN_MODE_CLAIR;
        assert_eq!(tint_alpha_selon(0.3, true), plancher);
        assert_eq!(tint_alpha_selon(0.95, true), 0.95);
        assert_eq!(tint_alpha_selon(0.3, false), 0.3);
        // Le mode capturable n'est plus un paramètre : la transparence réglée
        // ne peut plus être écrasée par lui.
    }

    /// Le style d'une BARRE (voir `win32::window::DOCK_EX_STYLE`) : au-dessus,
    /// jamais activée, et déclarée fenêtre normale pour le shell.
    const DOCK: isize = (WS_EX_TOPMOST.0 | WS_EX_NOACTIVATE.0 | WS_EX_APPWINDOW.0) as isize;

    /// Celui d'une fenêtre TRANSPARENTE : menu ancré, infobulle, horloge.
    const TRANSIENT: isize = (WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) as isize;

    /// La règle du correctif du 2026-09-20. Retirer `WS_EX_TOOLWINDOW` est
    /// nécessaire (la liste des fenêtres d'OBS se lit sur les styles) mais
    /// INSUFFISANT pour le shell : une barre garde `WS_EX_NOACTIVATE`, donc il
    /// faut `WS_EX_APPWINDOW` — et alors ce module ne doit plus la toucher.
    #[test]
    fn a_dock_is_left_alone_by_the_capture_setting() {
        assert_eq!(
            toolwindow_applied(DOCK, false),
            DOCK,
            "activer le mode capture ne doit rien changer à une barre"
        );
        assert_eq!(
            toolwindow_applied(DOCK, true),
            DOCK,
            "ni le désactiver — sinon les barres quitteraient Alt-Tab et la barre des tâches, \
             alors qu'elles doivent y être TOUJOURS"
        );
    }

    /// Le cas nominal reste intact dans les DEUX sens : un popup perd le style
    /// quand le mode est actif, et le retrouve quand il ne l'est plus.
    #[test]
    fn a_transient_window_still_follows_the_setting() {
        assert_eq!(
            toolwindow_applied(TRANSIENT, false),
            (WS_EX_TOPMOST.0 | WS_EX_NOACTIVATE.0) as isize,
            "mode actif : le style part, la fenêtre entre dans la liste d'OBS"
        );
        assert_eq!(
            toolwindow_applied(TRANSIENT, true),
            TRANSIENT,
            "mode inactif : le style revient"
        );
    }

    /// Le masque qui ne masquait rien — l'ancien code calculait
    /// `WS_EX_TOOLWINDOW & !WS_EX_APPWINDOW`, or les deux bits sont DISJOINTS :
    /// l'expression valait toujours `WS_EX_TOOLWINDOW`. C'était la trace
    /// d'`APPWINDOW` envisagé puis jamais posé.
    #[test]
    fn the_two_styles_do_not_overlap() {
        assert_eq!(WS_EX_TOOLWINDOW.0 & WS_EX_APPWINDOW.0, 0);
    }
}
