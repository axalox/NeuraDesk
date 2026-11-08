//! **Les icônes de la ZONE DE NOTIFICATION de Windows** (celle de la barre des
//! tâches), lues pour être affichées en miroir dans une zone du dock —
//! DEMANDE EXPLICITE : « je voudrais que dans la grille de la zone systèmes
//! s'affichent les icônes de la barre systèmes ».
//!
//! # Ce que la mesure a établi, et qui n'est PAS ce que dit la littérature
//!
//! Trois faits, tous mesurés sur cette machine (Windows 11 25H2, build 26200) par
//! la sonde `win32::tray_probe` — voir sa doc, et son en-tête pour la commande :
//!
//! 1. **Il n'y a AUCUN `ToolbarWindow32` sous `TrayNotifyWnd`.** La barre des
//!    tâches de Windows 11 est **en XAML**, pas un toolbar Win32. Toute la
//!    littérature sur le sujet (`TB_GETBUTTON`, `VirtualAllocEx`, la structure
//!    `TRAYDATA`, les décalages 32/64 bits) **ne s'applique donc pas ici** — et
//!    c'est une bonne nouvelle : cette voie est celle dont la littérature dit
//!    qu'elle peut faire planter `explorer.exe`.
//! 2. **L'arbre UI Automation est exposé par
//!    `Windows.UI.Composition.DesktopWindowContentBridge`**, PAS par
//!    `Shell_TrayWnd` (2 éléments) ni `TrayNotifyWnd` (aucun). Interroger
//!    `Shell_TrayWnd` — le réflexe naturel — ne rend **rien** de la zone.
//! 3. **La zone est parfaitement énumérable, barre masquée comprise** : nom,
//!    rectangle et ordre de chaque élément. **Coût MESURÉ (2026-09-21, build
//!    26200) : ~60 ms de médiane** — 63,9 ms sur le fil principal (n=33),
//!    59,5 ms sur un fil de travail (n=60), minimum 24,9 ms. ⚠️ **Le premier
//!    appel coûte ~1,2 s**, sur l'un comme sur l'autre : c'est un
//!    **échauffement**, pas un régime, et le prendre pour tel a déjà coûté une
//!    session (voir `tray_worker::APARTMENT`). Le « 42 ms » que cette ligne a
//!    porté longtemps était **périmé** : à 1 Hz, c'est ~1,5× ce qu'il annonçait.
//!
//! # Ce que ce module NE PEUT PAS faire, et qu'il ne faut pas lui demander
//!
//! - **Aucune icône.** UI Automation n'expose aucun pixel. Les images affichées
//!   sont donc fournies par l'appelant (voir `app::run`), à partir des glyphes du
//!   dock et du catalogue d'applications.
//! - **Aucune identité d'application** : MESURÉ, `CurrentProcessId()` rend
//!   `explorer.exe` pour **tous** les éléments — l'arbre XAML appartient au
//!   shell. Le seul discriminant est le couple (classe, nom).
//! - **Aucun clic** : décision explicite de l'utilisateur, l'affichage d'abord.
//!
//! # Ce qu'il fait quand ça échoue
//!
//! **Rien de fatal.** Une liste vide et un `diag!` **une seule fois** — jamais
//! un `None` muet (même choix que `win32::notification`). Une zone vide se lit
//! comme une zone vide, pas comme un plantage.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomationElement, TreeScope_Descendants, CUIAutomation,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, FindWindowW, GetClassNameW, GetDesktopWindow, GetSystemMetrics,
    GetWindow, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, SetWindowPos, GW_CHILD,
    GW_HWNDNEXT, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
};

use windows::core::w;

/// Classe de la fenêtre qui **EXPOSE** l'arbre XAML de la barre des tâches —
/// voir le point 2 de la doc de module : ce n'est PAS `Shell_TrayWnd`.
const COMPOSITION_BRIDGE_CLASS: &str = "Windows.UI.Composition.DesktopWindowContentBridge";

/// Tous les éléments de la zone de notification portent une classe préfixée
/// ainsi — MESURÉ : `SystemTray.NormalButton`, `SystemTray.AccentButton`,
/// `SystemTray.AccentText`, `SystemTray.OmniButtonLeft`, `SystemTray.OmniButtonRight`,
/// `SystemTray.ShowDesktopButton`. Le reste de la barre porte
/// `Taskbar.TaskListButtonAutomationPeer` ou `ToggleButton`.
const TRAY_CLASS_PREFIX: &str = "SystemTray.";

/// Valeur d'`AutomationId` qui désigne une icône **du shell lui-même** — MESURÉ
/// (voir `TrayIcon::from_shell`). L'autre valeur observée est `NotifyItemIcon`,
/// pour une icône enregistrée par une application.
const SHELL_AUTOMATION_ID: &str = "SystemTrayIcon";

/// **WINDOWS 10 : LA ZONE EST UN `ToolbarWindow32` WIN32, PAS UNE ÎLE XAML.**
///
/// MESURÉ le 2026-10-30 sur la VM Windows 10 22H2 (build 19045), sonde
/// `sonde-tray-win10` : l'UI Automation NATIVE, posée sur
/// `Shell_TrayWnd > TrayNotifyWnd > SysPager > ToolbarWindow32`, rend les icônes
/// promues (4 boutons, nom, rectangle, `Invoke`) ; posée sur
/// `NotifyIconOverflowWindow > ToolbarWindow32`, elle rend les MASQUÉES (3) —
/// ⭐ **PANNEAU FERMÉ**, rectangle `0x0`. Tous les descendants sont des boutons,
/// et leur classe UIA est VIDE : c'est cette valeur-ci qui les nomme dans
/// `TrayIcon::class`. Aucune lecture de la mémoire d'explorer.
const CLASSIC_TRAY_CLASS: &str = "ToolbarWindow32";

/// **Les icônes du SHELL sur Windows 10** — MESURÉ : leur `AutomationId` est un
/// GUID de la famille `{7820AE7x-23E3-4229-82C1-E41CB67D5B9C}` (Haut-parleurs
/// `…AE73`, Réseau `…AE74`, « Démarrer une réunion » `…AE83`). Les applications
/// ont un autre GUID (Sécurité Windows), ou aucun (OneDrive, VirtualBox).
const CLASSIC_SHELL_GUID_SUFFIX: &str = "-23E3-4229-82C1-E41CB67D5B9C}";

/// L'icône appartient-elle au shell ? — les deux conventions, Windows 11
/// (`SystemTrayIcon`) et Windows 10 (le suffixe de GUID).
fn est_du_shell(automation_id: &str) -> bool {
    automation_id == SHELL_AUTOMATION_ID
        || automation_id.to_ascii_uppercase().ends_with(CLASSIC_SHELL_GUID_SUFFIX)
}

/// **Entrées écartées par leur CLASSE — donc indépendamment de la langue.**
///
/// « Afficher le Bureau » n'est pas une icône de notification mais un bouton de
/// la barre : décision explicite de l'utilisateur.
const EXCLUDED_CLASSES: &[&str] = &["SystemTray.ShowDesktopButton"];

/// **Entrées écartées par leur NOM — donc DÉPENDANT DE LA LANGUE DE WINDOWS.**
///
/// ⚠️ **C'est une fragilité ASSUMÉE, et elle est ici plutôt que découverte
/// ailleurs.** La classe ne suffit pas à les distinguer : MESURÉ,
/// `SystemTray.AccentButton` couvre **aussi bien le Réseau que la cloche de
/// Notifications**, et `SystemTray.NormalButton` couvre aussi bien OneDrive que
/// le chevron des icônes cachées. Ces trois libellés sont ceux relevés sur une
/// installation **française** ; sur une autre langue, ces entrées reviendraient
/// dans la zone. Décision explicite de l'utilisateur : ni l'Horloge, ni la
/// cloche de Notifications, ni le chevron.
const EXCLUDED_NAME_PREFIXES: &[&str] = &["Afficher les icônes cachées", "Notifications", "Horloge"];

/// D'où vient une entrée — les DEUX zones de la barre des tâches.
///
/// Mesuré : les icônes VISIBLES vivent sous `Shell_TrayWnd`, et les MASQUÉES
/// (derrière le chevron) sous `TopLevelWindowForOverflowXamlIsland` — une autre
/// fenêtre, avec son propre arbre XAML.
///
/// ⛔ **CETTE LIGNE A LONGTEMPS DIT « présente même non affichée » — C'EST FAUX,
/// ET C'EST MESURÉ LE 2026-10-08.** `FindWindowW` sur cette classe **échoue**
/// panneau fermé. C'est une XAML island créée À LA DEMANDE : absente au repos,
/// elle n'apparaît qu'à l'ouverture du panneau — et elle disparaît quand il se
/// referme. Toute lecture de la zone masquée qui n'ouvre pas le panneau rend donc
/// une liste VIDE, ce qui est l'état NORMAL et non une panne. C'est tout l'objet
/// de `enumerate_avec_le_panneau`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayZone {
    Visible,
    Hidden,
}

/// Une icône de la zone de notification. **L'identité EST le couple
/// (classe, nom)** — c'est tout ce qu'UI Automation donne, et c'est ce qui
/// servira à choisir l'image à afficher (voir `app::tray_icon_texture`).
#[derive(Clone, Debug, PartialEq)]
pub struct TrayIcon {
    /// Texte d'infobulle, tel que Windows l'expose — il CHANGE au fil du temps
    /// (« Volume … : 100% », le SSID du réseau, l'heure). Il ne doit donc jamais
    /// servir de clé persistée, seulement de critère d'affichage et de
    /// rapprochement.
    pub name: String,
    /// Classe UI Automation, préfixée `SystemTray.` (voir `TRAY_CLASS_PREFIX`).
    pub class: String,
    /// Rectangle à l'écran — l'ORDRE de la zone s'en déduit (tri par abscisse).
    pub rect: RECT,
    /// Zone d'origine (voir `TrayZone`).
    pub zone: TrayZone,
    /// **L'icône appartient-elle au SHELL, ou à une application ?**
    ///
    /// MESURÉ, et c'est un discriminant plus FIN que le nom : `AutomationId`
    /// vaut `SystemTrayIcon` pour les icônes que le shell dessine lui-même
    /// (Volume, Réseau, Horloge, Clavier tactile, Emojis, Indicateur d'entrée,
    /// Confidentialité du micro, chevron) et `NotifyItemIcon` pour celles qu'une
    /// **application** a enregistrées (WhatsApp, Discord, mais aussi
    /// « Avertissement », « Mobile connecté », « Sécurité Windows »…).
    ///
    /// C'est ce qui permet de traiter les deux familles différemment : une icône
    /// du shell n'a **aucune application** dont prendre l'image — son glyphe EST
    /// l'artwork de Windows — tandis qu'une icône d'application doit être
    /// cherchée (instantané, application installée, exécutable).
    pub from_shell: bool,
}

/// **L'INTERRUPTEUR VIVANT** — voir `app::run`, qui l'initialise depuis le
/// réglage enregistré, et `win32::cef_settings`, qui le bascule À CHAUD (la
/// lecture se fait à chaque frame, donc cocher la case agit sans redémarrer).
///
/// `Cell` et non `RefCell` : un booléen se copie, il n'y a rien à emprunter — et
/// c'est aussi ce qui écarte la classe de défaut de réentrance que ce dépôt a
/// déjà payée deux fois.
///
/// Défaut à `true` : c'est une DEMANDE EXPLICITE de l'utilisateur, pas une
/// fonctionnalité à découvrir. Le réglage enregistré (`tray_mirror.txt`) prend le
/// dessus dès qu'il existe, y compris pour la désactiver.
///
/// ⚠️ **GLOBAL ET ATOMIQUE, ET PLUS `thread_local` — c'est un PRÉREQUIS, pas un
/// goût.** Le travail lent de la zone de notification déménage sur un fil de
/// travail, qui doit lire ce réglage pour savoir s'il travaille. Un
/// `thread_local` lui rendrait le **défaut** (`true`) et ignorerait le choix de
/// l'utilisateur — d'autant que `win32::cef_settings` bascule la valeur **à
/// chaud**, sans prévenir personne qui pourrait la propager.
///
/// Le dépôt a déjà tranché ce cas exact ailleurs, avec la même raison :
/// `gfx::capture_visibility` (`« état tenu dans un AtomicBool GLOBAL (pas
/// thread_local) »`). Les signatures ne changent pas — aucun appelant ne bouge.
static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// **LE FILTRE, PUR ET ÉPROUVABLE** : garde-t-on cette entrée ?
///
/// Séparé de la lecture UIA à dessein : c'est la SEULE partie de ce module qui
/// puisse être vérifiée sans écran ni explorer, et c'est aussi celle qui porte la
/// fragilité du nom (voir `EXCLUDED_NAME_PREFIXES`).
fn keep(name: &str, class: &str, apply_exclusions: bool) -> bool {
    if !class.starts_with(TRAY_CLASS_PREFIX) && class != CLASSIC_TRAY_CLASS {
        return false;
    }
    if !apply_exclusions {
        // La fenêtre de DÉBORDEMENT ne contient QUE des icônes de notification —
        // pas d'horloge, pas de chevron, pas de « Afficher le Bureau ». MESURÉ :
        // ses 66 descendants sont tous des `SystemTray.NormalButton`.
        return true;
    }
    if EXCLUDED_CLASSES.iter().any(|excluded| class.eq_ignore_ascii_case(excluded)) {
        return false;
    }
    !EXCLUDED_NAME_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

/// La **passerelle de composition** SOUS la fenêtre de classe `root_class` —
/// c'est ELLE qui expose l'arbre XAML, jamais la fenêtre racine (MESURÉ :
/// `Shell_TrayWnd` ne rend que 2 éléments, `TrayNotifyWnd` aucun).
///
/// Deux racines possibles, pour les deux zones (voir `TrayZone`) :
/// `Shell_TrayWnd` pour les visibles, `TopLevelWindowForOverflowXamlIsland` pour
/// les masquées.
fn find_bridge(root_class: &str) -> Option<HWND> {
    let root_wide: Vec<u16> = root_class.encode_utf16().chain(std::iter::once(0)).collect();
    // ⚠️ **LA BARRE PRINCIPALE PAR `vraie_barre`** : le flux de notification
    // installe une fenêtre de MÊME classe (voir `tray_flux`), sans enfants.
    let root = if root_class == "Shell_TrayWnd" {
        crate::win32::tray_flux::vraie_barre()?
    } else {
        unsafe { FindWindowW(PCWSTR(root_wide.as_ptr()), PCWSTR::null()) }.ok()?
    };
    if root.0.is_null() {
        return None;
    }
    let mut stack = vec![root];
    while let Some(hwnd) = stack.pop() {
        if class_name(hwnd).starts_with(COMPOSITION_BRIDGE_CLASS) {
            return Some(hwnd);
        }
        let mut child = unsafe { GetWindow(hwnd, GW_CHILD) }.unwrap_or_default();
        while !child.0.is_null() {
            stack.push(child);
            child = unsafe { GetWindow(child, GW_HWNDNEXT) }.unwrap_or_default();
        }
    }
    None
}

/// **WINDOWS 10 : la barre d'outils Win32 d'une zone** (voir `CLASSIC_TRAY_CLASS`).
fn barre_classique(zone: TrayZone) -> Option<HWND> {
    let enfant = |parent: HWND, classe: PCWSTR| {
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::FindWindowExW(
                Some(parent),
                None,
                classe,
                PCWSTR::null(),
            )
        }
        .ok()
    };
    match zone {
        TrayZone::Visible => {
            let barre = crate::win32::tray_flux::vraie_barre()?;
            let notif = enfant(barre, w!("TrayNotifyWnd"))?;
            let pager = enfant(notif, w!("SysPager"))?;
            enfant(pager, w!("ToolbarWindow32"))
        }
        TrayZone::Hidden => {
            let debordement =
                unsafe { FindWindowW(w!("NotifyIconOverflowWindow"), PCWSTR::null()) }.ok()?;
            enfant(debordement, w!("ToolbarWindow32"))
        }
    }
}

/// **LA FENÊTRE DONT L'ARBRE UIA PORTE LES ICÔNES D'UNE ZONE**, selon la lignée :
/// la passerelle de composition sous `root_class` sur Windows 11, la barre
/// d'outils classique sur Windows 10. Le booléen dit laquelle — il décide du
/// filtre des éléments (voir `classe_d_icone`).
fn racine_de_zone(root_class: &str, zone: TrayZone) -> Option<(HWND, bool)> {
    if crate::win32::os_version::os_windows_10() {
        return barre_classique(zone).map(|hwnd| (hwnd, true));
    }
    find_bridge(root_class).map(|hwnd| (hwnd, false))
}

/// **La classe retenue pour un élément de la zone, ou `None` s'il n'est pas une
/// icône.** Sous une barre classique tous les descendants sont des icônes
/// (mesuré, voir `CLASSIC_TRAY_CLASS`) ; sous une passerelle, seuls les
/// `SystemTray.*` le sont.
fn classe_d_icone(element: &IUIAutomationElement, classique: bool) -> Option<String> {
    if classique {
        return Some(CLASSIC_TRAY_CLASS.to_string());
    }
    let class = current_class(element);
    class.starts_with(TRAY_CLASS_PREFIX).then_some(class)
}

/// La classe de la fenêtre qui porte les icônes **MASQUÉES** (derrière le
/// chevron).
///
/// ⛔ **ELLE N'EXISTE QUE PANNEAU OUVERT — MESURÉ LE 2026-10-08.** Les deux
/// commentaires qui précédaient affirmaient l'inverse (« présente même non
/// affichée », « 66 descendants lisibles ») : ils étaient FAUX, et c'est ce qui a
/// coûté une session entière de diagnostic. `FindWindowW` sur cette classe rend
/// **une erreur** panneau fermé (relevé : `HRESULT(0x00000000)` — c'est
/// `GetLastError` resté à zéro sur un handle nul). La littérature disait vrai.
///
/// ⚠️ **Ce que ça implique, et qui n'est PAS une erreur** : `find_bridge` rend
/// `None`, `enumerate_zone` rend une liste vide, et **en silence** — l'absence de
/// cette fenêtre est l'état de repos normal (voir la garde de zone dans
/// `enumerate_zone`). Un relevé « 0 masquée » fait panneau fermé ne mesure donc
/// RIEN : c'est le cas où la sonde est muette par construction.
const OVERFLOW_ROOT_CLASS: &str = "TopLevelWindowForOverflowXamlIsland";

/// Signale un échec **une seule fois** — au-delà, une lecture ratée à chaque
/// seconde noierait le journal sous la même ligne.
fn warn_once(message: &str) {
    static WARNED: Once = Once::new();
    WARNED.call_once(|| crate::diag!("[tray_icons] {message}"));
}

/// **CE QUE LA ZONE MASQUÉE A RENDU LA DERNIÈRE FOIS QU'ELLE ÉTAIT LISIBLE.**
///
/// ⛔ **POURQUOI UNE RETENUE, ET PAS SEULEMENT UNE LECTURE.** La fenêtre de
/// débordement n'existe que panneau OUVERT (voir `OVERFLOW_ROOT_CLASS`) : au
/// repos, chaque relevé rend 0, **en silence**, et c'est l'état NORMAL. Sans
/// retenue, la zone masquée vaudrait donc 0 en régime — pire, le prédicat
/// `tray_worker::zone_eteinte`, qui refuse de publier une zone pleine devenue
/// vide, verrait une zone s'éteindre à chaque cycle et **cesserait de publier le
/// miroir**.
#[derive(Default)]
struct Retenue {
    /// La dernière liste COMPLÈTE relevée panneau ouvert.
    icones: Vec<TrayIcon>,
    /// Quand elle a été relevée.
    quand: Option<Instant>,
    /// Quand on a tenté le dernier aller-retour — la borne de cadence.
    dernier_aller_retour: Option<Instant>,
}

static RETENUE: Mutex<Retenue> =
    Mutex::new(Retenue { icones: Vec::new(), quand: None, dernier_aller_retour: None });

/// **L'ÂGE MAXIMAL DE LA DERNIÈRE LECTURE RÉUSSIE** avant qu'on accepte de
/// rouvrir le panneau.
///
/// ⛔ **CE N'EST PAS UNE CADENCE D'OUVERTURE — ET C'EST MESURÉ, PAS SUPPOSÉ.**
/// L'île XAML du débordement est créée à la demande, puis **ELLE PERSISTE** :
/// refermée, elle reste présente et **entièrement lisible** (`visible = false`).
/// Relevé du 2026-10-08, après une seule ouverture : `FindWindowW` la trouve,
/// `enumerate()` rend **47 = 11 visibles + 36 masquées**, six fois de suite.
///
/// Conséquence, et elle est heureuse : **en régime, le panneau ne se rouvre
/// JAMAIS.** Chaque relevé réussit, donc `retenir` rafraîchit cette date à
/// chaque cycle, donc ce délai n'expire pas. Il ne peut expirer que si les
/// lectures se mettent à ÉCHOUER — l'île détruite, par exemple après un
/// redémarrage d'`explorer.exe` — et il est alors le seul à autoriser une
/// réouverture. C'est un filet, pas une horloge.
///
/// ⚠️ **LA VALEUR EST DONC PEU SENSIBLE** : elle ne décide que du délai de
/// réparation après une disparition de l'île. 30 minutes est un compromis entre
/// « se répare vite » et « ne rouvre pas le panneau pour un transitoire ».
const PEREMPTION_MASQUEES: Duration = Duration::from_secs(1800);

/// **LE DÉLAI MINIMUM ENTRE DEUX ALLER-RETOURS** — un garde-fou de cadence, pas
/// la politique de rafraîchissement (celle-ci est `PEREMPTION_MASQUEES`).
///
/// ⚠️ **IL EXISTE PARCE QUE `enumerate_avec_le_panneau` TOURNE À 1 Hz.** Sans
/// lui, une suite d'échecs — ou une liste qui revient vide — ferait ouvrir et
/// refermer le panneau de l'utilisateur **une fois par seconde**.
const REFROIDISSEMENT_PANNEAU: Duration = Duration::from_secs(60);

/// **Vrai pendant qu'un aller-retour est en cours.** Deux ouvertures simultanées
/// se **TOGGLERAIENT** — le bouton du shell est une bascule — et le panneau
/// resterait dans l'état inverse de celui voulu, c'est-à-dire OUVERT.
static ALLER_RETOUR_EN_COURS: AtomicBool = AtomicBool::new(false);

/// Remet le drapeau à `false` quoi qu'il arrive — y compris sur un `?` qui sort
/// en erreur au milieu de l'aller-retour.
struct GardeAllerRetour;

impl Drop for GardeAllerRetour {
    fn drop(&mut self) {
        ALLER_RETOUR_EN_COURS.store(false, Ordering::SeqCst);
    }
}

/// Enregistre une liste COMPLÈTE relevée panneau ouvert.
fn retenir(icones: &[TrayIcon]) {
    if let Ok(mut retenue) = RETENUE.lock() {
        retenue.icones = icones.to_vec();
        retenue.quand = Some(Instant::now());
    }
}

/// **TOUTES les icônes de la zone de notification : les VISIBLES, puis les
/// MASQUÉES.**
///
/// ⛔ **LECTURE SEULE — ELLE N'OUVRE JAMAIS LE PANNEAU DE DÉBORDEMENT.** C'est la
/// fonction des SONDES, et une sonde n'agit pas sur le bureau de l'utilisateur.
/// Elle rend donc **0 masquée au repos**, ce qui est l'état NORMAL et non une
/// panne (voir `OVERFLOW_ROOT_CLASS`). Le miroir, lui, appelle
/// `enumerate_avec_le_panneau`.
///
/// Rend une liste VIDE si quoi que ce soit échoue (pas de barre, COM
/// indisponible, passerelle absente) — jamais d'erreur remontée, jamais de
/// panique : voir la doc de module. Une zone absente n'empêche pas l'autre.
pub fn enumerate() -> Vec<TrayIcon> {
    enumerate_impl(false)
}

/// **LA LISTE DU MIROIR — celle qui a le droit d'ouvrir le panneau.**
///
/// Même chose que `enumerate`, plus la zone MASQUÉE : lue si le panneau est
/// ouvert, **ouverte puis refermée** sinon, et retenue entre deux relevés (voir
/// `Retenue`, `PEREMPTION_MASQUEES`). C'est le seul appelant qui a le droit
/// d'agir sur l'écran.
pub fn enumerate_avec_le_panneau() -> Vec<TrayIcon> {
    enumerate_impl(true)
}

fn enumerate_impl(ouvrir_si_besoin: bool) -> Vec<TrayIcon> {
    // Best-effort : l'échec signifie « déjà initialisé, ou dans un autre mode »,
    // ce qui n'empêche pas COM de fonctionner (même choix que la sonde).
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
    let automation: IUIAutomation = match unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &CUIAutomation,
            None,
            windows::Win32::System::Com::CLSCTX_ALL,
        )
    } {
        Ok(value) => value,
        Err(error) => {
            warn_once(&format!("UI Automation indisponible : {error:?}"));
            return Vec::new();
        }
    };
    // ⛔ **LES VISIBLES D'ABORD, ET CE N'EST PAS UN DÉTAIL D'ORDRE.** Panneau de
    // débordement OUVERT, la passerelle de composition de `Shell_TrayWnd`
    // **DISPARAÎT** : l'énumération ne rend alors plus que les masquées et
    // **zéro visible** (mesuré — voir le récit de `clic_reel`). On lit donc la
    // barre AVANT de toucher au panneau.
    let mut found = enumerate_zone(&automation, "Shell_TrayWnd", TrayZone::Visible, true);
    found.extend(masquees(&automation, ouvrir_si_besoin));
    found
}

/// **FAUT-IL OUVRIR LE PANNEAU ?** — la décision, PURE, donc testable.
///
/// Les deux âges sont ceux de la retenue et de la dernière tentative, ou `None`
/// quand il n'y en a jamais eu. L'ordre des deux refus compte : une retenue
/// fraîche rend le second inutile, et c'est le cas de régime — **une retenue
/// fraîche ne coûte rien, jamais une ouverture**.
///
/// ⛔ **LES DEUX BORNES SONT INDÉPENDANTES, ET C'EST VOULU.** `PEREMPTION_MASQUEES`
/// dit à quelle fréquence on VEUT rafraîchir ; `REFROIDISSEMENT_PANNEAU` dit à
/// quelle fréquence on a le DROIT de réessayer après un échec. Les confondre
/// ferait, d'un échec, une cadence.
fn faut_il_ouvrir(age_retenue: Option<Duration>, age_essai: Option<Duration>) -> bool {
    if let Some(age) = age_retenue {
        if age < PEREMPTION_MASQUEES {
            return false;
        }
    }
    if let Some(age) = age_essai {
        if age < REFROIDISSEMENT_PANNEAU {
            return false;
        }
    }
    true
}

/// **LA ZONE MASQUÉE — lue si elle est là, OUVERTE si besoin.**
///
/// Trois cas, dans cet ordre, et l'ordre est la correction :
///
/// 1. **Le panneau est ouvert** (par l'utilisateur, ou par un aller-retour
///    précédent) : la lecture directe est la meilleure source et elle est
///    gratuite. **On n'y touche pas** — c'est ce que veut dire « on ne referme
///    jamais un panneau que l'utilisateur a ouvert ».
/// 2. **Panneau fermé, et la retenue est encore fraîche** : elle est la seule
///    source, et elle est rendue telle quelle.
/// 3. **Sinon** : UN aller-retour — ouvrir, laisser s'installer, relire,
///    refermer — borné par `REFROIDISSEMENT_PANNEAU`.
fn masquees(automation: &IUIAutomation, ouvrir_si_besoin: bool) -> Vec<TrayIcon> {
    let vivantes = enumerate_zone(automation, OVERFLOW_ROOT_CLASS, TrayZone::Hidden, false);
    if !vivantes.is_empty() {
        retenir(&vivantes);
        return vivantes;
    }
    // Windows 10 : la barre d'outils du débordement se lit panneau FERMÉ (voir
    // `CLASSIC_TRAY_CLASS`). Vide, elle dit qu'il n'y a pas de masquée — et le
    // panneau XAML qu'on ouvrirait n'existe pas.
    if !ouvrir_si_besoin || crate::win32::os_version::os_windows_10() {
        return Vec::new();
    }

    let (retenues, quand, dernier) = match RETENUE.lock() {
        Ok(retenue) => (retenue.icones.clone(), retenue.quand, retenue.dernier_aller_retour),
        Err(_) => (Vec::new(), None, None),
    };
    // ⭐ **LA DÉCISION EST UNE FONCTION PURE, ET C'EST DÉLIBÉRÉ** : c'est ici que
    // vit la contrainte qui compte — « un aller-retour, JAMAIS un par cycle » —
    // et une contrainte qui n'est pas testable n'est pas tenue.
    let age_retenue = quand.map(|releve| releve.elapsed());
    let age_essai = dernier.map(|essai| essai.elapsed());
    if !faut_il_ouvrir(age_retenue, age_essai) {
        return retenues;
    }

    match ouvrir_lire_refermer(automation) {
        Ok(lues) if !lues.is_empty() => {
            crate::diag!(
                "[tray_icons] panneau ouvert : {} icône(s) MASQUÉE(S) relevée(s), panneau refermé",
                lues.len()
            );
            retenir(&lues);
            lues
        }
        // ⛔ **LE TÉMOIN QUI DISTINGUE LES DEUX ÉCHECS.** « Ouvert mais vide » et
        // « pas ouvert » se lisaient pareil avant : les deux rendaient 0, sans un
        // mot. Si ce cas apparaît, c'est que `OVERFLOW_ROOT_CLASS` a changé de nom
        // dans Windows — et c'est la première chose à vérifier.
        Ok(_) => {
            crate::diag!(
                "[tray_icons] panneau ouvert mais zone masquée TOUJOURS VIDE — la classe \
                 {OVERFLOW_ROOT_CLASS} a-t-elle changé ?"
            );
            retenues
        }
        Err(raison) => {
            crate::diag!("[tray_icons] panneau de débordement NON ouvert ({raison})");
            retenues
        }
    }
}

/// **L'ALLER-RETOUR : ouvrir, relire, refermer — et ne JAMAIS refermer ce qu'on
/// n'a pas ouvert.**
///
/// ⚠️ **L'HORODATAGE EST POSÉ AVANT LA TENTATIVE, PAS APRÈS** : un échec compte
/// comme une tentative, sinon une panne rejouerait l'aller-retour à chaque cycle.
fn ouvrir_lire_refermer(automation: &IUIAutomation) -> Result<Vec<TrayIcon>, String> {
    // Le drapeau est posé d'abord : deux bascules simultanées laisseraient le
    // panneau ouvert (voir `ALLER_RETOUR_EN_COURS`).
    if ALLER_RETOUR_EN_COURS.swap(true, Ordering::SeqCst) {
        return Err("un autre aller-retour est déjà en cours".to_string());
    }
    let _garde = GardeAllerRetour;
    if let Ok(mut retenue) = RETENUE.lock() {
        retenue.dernier_aller_retour = Some(Instant::now());
    }

    if panneau_debordement_visible() {
        // Ouvert par l'utilisateur : on LIT, et on ne referme RIEN.
        return Ok(enumerate_zone(automation, OVERFLOW_ROOT_CLASS, TrayZone::Hidden, false));
    }
    ouvrir_le_panneau_masque()?;
    // Le panneau XAML se compose après avoir été demandé — même ordre de grandeur
    // que `MENU_APPEARANCE_MS`, voir `PANNEAU_OUVERTURE_MS`.
    std::thread::sleep(Duration::from_millis(PANNEAU_OUVERTURE_MS));
    let lues = enumerate_zone(automation, OVERFLOW_ROOT_CLASS, TrayZone::Hidden, false);
    std::thread::sleep(Duration::from_millis(PANNEAU_REFERMETURE_MS));
    // ⛔ **ON REFERME CE QU'ON A OUVERT — SINON LE DOCK PERD SES ICÔNES VISIBLES.**
    // Défaut réel du 2026-10-06 : panneau ouvert, plus de passerelle sous
    // `Shell_TrayWnd`, « je ne vois plus du tout les icônes de Windows ». Le
    // même `Invoke` referme, le bouton du shell étant une bascule.
    if let Err(raison) = ouvrir_le_panneau_masque() {
        crate::diag!(
            "[tray_icons] panneau NON refermé ({raison}) — le dock peut perdre ses icônes visibles"
        );
    }
    Ok(lues)
}

/// Une des deux zones : la passerelle de composition sous `root_class`, puis ses
/// éléments `SystemTray.*` (voir `keep`, qui porte les exclusions — appliquées
/// à la barre seulement, la fenêtre de débordement n'ayant par nature que des
/// icônes de notification).
fn enumerate_zone(
    automation: &IUIAutomation,
    root_class: &str,
    zone: TrayZone,
    apply_exclusions: bool,
) -> Vec<TrayIcon> {
    let Some((bridge, classique)) = racine_de_zone(root_class, zone) else {
        // Silencieux pour le débordement : son absence est un cas NORMAL (voir
        // `OVERFLOW_ROOT_CLASS`), pas un défaut à signaler.
        if zone == TrayZone::Visible {
            // La lignée nomme ce qui manque — voir `win32::os_version`, qui est
            // la seule à distinguer les deux (le majeur vaut 10 des deux côtés).
            if crate::win32::os_version::os_windows_11() {
                warn_once("passerelle de composition introuvable sous Shell_TrayWnd");
            } else if crate::win32::os_version::os_windows_10() {
                warn_once("zone de notification : Windows 10 — barre d'outils Shell_TrayWnd > TrayNotifyWnd > SysPager > ToolbarWindow32 INTROUVABLE");
            } else {
                warn_once("passerelle de composition introuvable sous Shell_TrayWnd (lignée de Windows ILLISIBLE)");
            }
        }
        return Vec::new();
    };
    let Ok(element) = (unsafe { automation.ElementFromHandle(bridge) }) else {
        warn_once("ElementFromHandle a échoué sur la passerelle");
        return Vec::new();
    };
    let Ok(condition) = (unsafe { automation.CreateTrueCondition() }) else {
        warn_once("CreateTrueCondition a échoué");
        return Vec::new();
    };
    let Ok(children) = (unsafe { element.FindAll(TreeScope_Descendants, &condition) }) else {
        warn_once("FindAll a échoué sur la passerelle");
        return Vec::new();
    };

    let mut found: Vec<TrayIcon> = Vec::new();
    for index in 0..unsafe { children.Length() }.unwrap_or(0) {
        let Ok(child) = (unsafe { children.GetElement(index) }) else { continue };
        let Some(class) = classe_d_icone(&child, classique) else { continue };
        let name = unsafe { child.CurrentName() }.map(|n| n.to_string()).unwrap_or_default();
        if !keep(&name, &class, apply_exclusions) {
            continue;
        }
        let Ok(rect) = (unsafe { child.CurrentBoundingRectangle() }) else { continue };
        // L'`AutomationId` — mesuré, et c'est le discriminant le plus fin que
        // Windows expose (voir `TrayIcon::from_shell`).
        let automation_id =
            unsafe { child.CurrentAutomationId() }.map(|id| id.to_string()).unwrap_or_default();
        found.push(TrayIcon {
            name,
            class,
            rect,
            zone,
            from_shell: est_du_shell(&automation_id),
        });
    }
    // L'ORDRE EST CELUI DE L'ÉCRAN : UIA rend les éléments dans l'ordre de
    // l'arbre XAML, qui n'est pas garanti être celui de l'affichage. Le tri par
    // abscisse, lui, l'est.
    //
    // ⚠️ Pour le DÉBORDEMENT, les abscisses sont celles du panneau déroulant
    // (2 colonnes ou plus), pas une rangée — le tri reste un ordre STABLE et
    // reproductible, mais il ne prétend pas être l'ordre de lecture.
    found.sort_by_key(|icon| (icon.rect.top, icon.rect.left));
    found
}

fn current_class(element: &IUIAutomationElement) -> String {
    unsafe { element.CurrentClassName() }.map(|c| c.to_string()).unwrap_or_default()
}

/// **L'ARTWORK DU SYSTÈME, PRIS À WINDOWS LUI-MÊME.**
///
/// Mesuré — planche `glyphes-segoe.png`, glyphes rendus depuis *Segoe Fluent
/// Icons* (`SegoeIcons.ttf`) puis REGARDÉS : les icônes système de la zone de
/// notification ne sont pas des ressources de DLL mais des **glyphes de
/// police**, et cette police est installée sur la machine. On les rend donc par
/// le même chemin que le reste du texte du dock (`gfx::text::render_text`), ce
/// qui donne l'artwork **exact** de Windows 11 — et non une approximation.
///
/// ⚠️ **CHAQUE POINT DE CODE A ÉTÉ CONFIRMÉ À L'ŒIL**, pas recopié d'un tableau
/// de correspondance : c'est tout l'objet de la planche de la sonde.
///
/// ⚠️ **LES PRÉFIXES SONT EN FRANÇAIS** (comme `EXCLUDED_NAME_PREFIXES`) : sur
/// une installation dans une autre langue, ces entrées retomberaient sur les
/// niveaux suivants (catalogue d'applications, puis initiale). C'est une
/// fragilité assumée, écrite ici plutôt que découverte ailleurs.
const SYSTEM_GLYPHS: &[(&str, char)] = &[
    ("Volume", '\u{E767}'),
    // Le même haut-parleur sous Windows 10, qui le nomme autrement — MESURÉ sur
    // la VM 22H2 : « Haut-parleurs : 67% ».
    ("Haut-parleurs", '\u{E767}'),
    ("Réseau", '\u{E701}'),
    ("Confidentialité", '\u{E720}'),
    ("Clavier", '\u{E765}'),
    ("Ajouter des emojis", '\u{E76E}'),
    // Le triangle d'avertissement — MESURÉ, et c'est LUI qui corrige un vrai
    // défaut : sans cette entrée, « Avertissement » se rapprochait de
    // l'instantané de `BrStMonW.exe` (le *Brother Status Monitor*), dont
    // l'image est le LOGO MONITEUR de l'application — l'utilisateur voyait un
    // écran d'ordinateur là où Windows dessine un triangle.
    ("Avertissement", '\u{E7BA}'),
    // Protection et sécurité — mêmes raisons : ces icônes d'ÉTAT appartiennent
    // au shell, n'existent dans AUCUN instantané du registre (vérifié), et
    // méritent donc un glyphe plutôt qu'une initiale.
    ("Vous êtes protégé", '\u{EA18}'),
    // ⚠️ **« Sécurité » ET « Mobile » ONT ÉTÉ RETIRÉS DE CETTE TABLE**, et c'est
    // un progrès : « Sécurité Windows » et « Mobile connecté » sont les noms
    // AFFICHÉS d'applications du Store (Windows Security, Phone Link — mesuré),
    // donc `match_installed_app` les identifie tout seul et leur donne leur VRAIE
    // icône. Les garder ici les aurait privées de mieux, un glyphe générique
    // passant avant.
    ("Synchronisation", '\u{E895}'),
    // L'indicateur d'entrée (la langue de clavier) : Windows y affiche
    // normalement un SIGLE (« FRA »), qu'on ne sait pas extraire proprement du
    // nom. Le globe est le glyphe que le système utilise lui-même ailleurs pour
    // cette notion — un substitut assumé, pas une équivalence.
    ("Indicateur", '\u{E774}'),
];

/// **Le glyphe de repli d'une entrée SANS NOM** — MESURÉ : la zone de
/// l'utilisateur en contient une, dont l'infobulle est vide, donc sans même une
/// initiale à afficher.
///
/// Un **point discret** plutôt qu'un « ? » : il dit « entrée non identifiée »
/// sans prétendre apporter une information, et sans se confondre avec une icône
/// réelle.
pub const GENERIC_GLYPH: char = '\u{E915}';

/// **Le glyphe système de `name`**, s'il y en a un — comparaison par PRÉFIXE,
/// sur le nom débarrassé de ses espaces de tête (l'infobulle d'OneDrive commence
/// par une espace, MESURÉ).
///
/// Fonction PURE : c'est la deuxième des trois tables de correspondance (voir
/// `keep`, puis le catalogue d'applications côté appelant).
pub fn system_glyph(name: &str) -> Option<char> {
    let trimmed = name.trim_start();
    SYSTEM_GLYPHS
        .iter()
        .find(|(prefix, _)| trimmed.starts_with(prefix))
        .map(|(_, glyph)| *glyph)
}

/// **L'ICÔNE RÉSEAU DU SHELL ?** — DEMANDE EXPLICITE (2026-10-10) : « un clic droit
/// sur l'applet réseau ouvre les paramètres réseau de Windows ». Le journal a
/// montré que ce clic tombait sur l'icône réseau du MIROIR (`clic droit réel …
/// "Réseau Freebox-68B345_FAST"`), relayée au menu de Windows ; elle prend donc le
/// même geste que l'applet réseau du dock.
///
/// Le shell seul : une APPLICATION dont l'infobulle commencerait par « Réseau »
/// garde son propre menu.
pub fn est_l_icone_reseau(icon: &TrayIcon) -> bool {
    icon.from_shell && system_glyph(&icon.name) == system_glyph("Réseau")
}

/// Longueur minimale d'une correspondance — en dessous, le nom d'exécutable
/// d'une application courte se retrouverait dans n'importe quelle infobulle.
const MIN_MATCH_CHARS: usize = 4;

/// Longueur minimale du PREMIER MOT pour la règle d'égalité de mot (voir
/// `match_snapshot`). Plus bas que `MIN_MATCH_CHARS`, et c'est sûr : un mot
/// ENTIER est un signal bien plus fort qu'un fragment de même longueur.
const MIN_WORD_CHARS: usize = 3;

/// Ne garde que les caractères alphanumériques, en minuscules.
///
/// **C'est ce qui permet de rapprocher « MSI Afterburner » de
/// `MSIAfterburner.exe`** — MESURÉ : c'est le cas réel de la zone de
/// l'utilisateur, et la seule comparaison de casse ne le rapprocherait pas.
fn normalized(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// **L'APPLICATION OUVERTE dont le nom apparaît dans l'infobulle d'une entrée de
/// notification** — rend son INDEX dans `exe_names`.
///
/// **POURQUOI CE RAPPROCHEMENT, ET PAS UN CATALOGUE D'APPLICATIONS** : une icône
/// de notification appartient par définition à un processus QUI TOURNE, et la
/// liste des fenêtres ouvertes est **déjà collectée** à 1 Hz par `app::run` —
/// donc aucun balayage supplémentaire, aucun cache à tenir, et les noms
/// d'exécutables sont exactement ceux que la barre des tâches manipule.
///
/// Le rapprochement retient la **PLUS LONGUE** correspondance, pour qu'une
/// infobulle ne soit pas attribuée à un exécutable court alors qu'un plus
/// spécifique est aussi ouvert.
///
/// **Il ne trouve pas tout, et c'est normal** : une infobulle qui ne nomme pas
/// son application (« Vous êtes protégé » pour NordVPN, MESURÉ) ne peut pas être
/// rapprochée. L'appelant a un troisième niveau pour ce cas.
pub fn match_open_app(name: &str, exe_names: &[String]) -> Option<usize> {
    let needle = normalized(name);
    if needle.is_empty() {
        return None;
    }
    let mut best: Option<(usize, usize)> = None;
    for (index, exe) in exe_names.iter().enumerate() {
        let stem = exe
            .strip_suffix(".exe")
            .or_else(|| exe.strip_suffix(".EXE"))
            .unwrap_or(exe.as_str());
        let key = normalized(stem);
        if key.len() < MIN_MATCH_CHARS || !needle.contains(&key) {
            continue;
        }
        if best.is_none_or(|(_, length)| key.len() > length) {
            best = Some((index, key.len()));
        }
    }
    best.map(|(index, _)| index)
}

/// **L'ICÔNE ELLE-MÊME, telle que le SHELL l'a enregistrée** (voir
/// `read_snapshots`) — l'infobulle et l'exécutable servent à la retrouver, et le
/// PNG est ce qu'on affiche.
#[derive(Clone, Debug, PartialEq)]
pub struct TrayIconSnapshot {
    /// Infobulle telle qu'elle était À L'ENREGISTREMENT — elle peut différer de
    /// l'infobulle vivante (une application y met sa version, un état…), d'où un
    /// rapprochement SOUPLE plutôt qu'une égalité.
    pub tooltip: String,
    /// Chemin d'exécutable de l'application propriétaire.
    pub executable: String,
    /// **L'image, en PNG** — MESURÉ : les 141 sous-clés de cette machine en
    /// portent un, et chacun commence par la signature PNG (`\x89PNG`), en
    /// 32×32.
    pub png: Vec<u8>,
    /// **L'identité que l'application a donnée à son icône** (`NIF_GUID`), telle
    /// que le registre la porte — `None` pour les entrées identifiées par
    /// `(hWnd, uID)`.
    ///
    /// MESURÉ : **38 des 142 entrées** de cette machine. C'est le seul champ qui
    /// permette d'interroger le shell sur une icône qu'on ne possède pas (voir
    /// `rect_of_guid`) : les entrées à GUID n'ont **ni `UID` ni `InitialTooltip`**,
    /// ce qui explique que l'identification par image et par élimination
    /// échouaient sur Discord — elles cherchaient la mauvaise clé.
    pub guid: Option<windows::core::GUID>,
    /// Le NOM de la sous-clé, un entier — c'est par lui que `UIOrderList`
    /// désigne l'entrée (voir `read_ui_order`). `0` si le nom n'est pas un
    /// entier.
    pub id: u64,
}

/// Sous-clé du registre où le shell tient **les icônes de sa zone de
/// notification** — MESURÉ sur cette machine : 141 sous-clés, chacune avec
/// `IconSnapshot` (le PNG), `ExecutablePath`, `InitialTooltip`, `UID` ou
/// `IconGuid`, et `IsPromoted` pour celles qui s'affichent dans la barre.
///
/// **C'est une source PASSIVE**, du même genre que le blob d'épingles de
/// `win32::taskbar_pins` : une lecture de registre, sans API à interroger, sans
/// mémoire d'un autre processus à lire, et sans le moindre risque pour explorer.
/// C'est aussi **la SEULE source des vraies images** : la zone de notification
/// n'expose aucun pixel par UI Automation, et la barre des tâches de
/// l'utilisateur étant en auto-masquage, aucune capture n'est possible.
const SNAPSHOT_KEY: &str = "Control Panel\\NotifyIconSettings";

/// Lit les icônes enregistrées par le shell (voir `TrayIconSnapshot`).
///
/// **À APPELER QUAND LA LISTE CHANGE, PAS À CHAQUE FRAME** : cela ouvre une clé
/// et lit **quatre** valeurs par sous-clé — 142 sous-clés ici, donc ~570 lectures
/// de registre. C'est acceptable ponctuellement, jamais à 1 Hz.
///
/// Rend une liste vide si la clé n'existe pas (elle est apparue avec Windows 11,
/// et peut manquer) : ce n'est pas une erreur, seulement une source de moins —
/// `app::tray_icon_texture` a des replis.
pub fn read_snapshots() -> Vec<TrayIconSnapshot> {
    use windows::Win32::System::Registry::HKEY_CURRENT_USER;

    let mut snapshots = Vec::new();
    for name in crate::win32::registry_util::enumerate_subkeys(HKEY_CURRENT_USER, SNAPSHOT_KEY) {
        let subkey = format!("{SNAPSHOT_KEY}\\{name}");
        let Some(png) = crate::win32::registry_util::read_binary_value(
            HKEY_CURRENT_USER,
            &subkey,
            "IconSnapshot",
        ) else {
            continue;
        };
        // La signature PNG est VÉRIFIÉE plutôt que supposée : une valeur d'un
        // autre type serait ignorée, pas transmise au décodeur d'images.
        if png.len() < 8 || png[0] != 0x89 || png[1..4] != *b"PNG" {
            continue;
        }
        snapshots.push(TrayIconSnapshot {
            tooltip: crate::win32::registry_util::read_string_value(
                HKEY_CURRENT_USER,
                &subkey,
                "InitialTooltip",
            )
            .unwrap_or_default(),
            executable: crate::win32::registry_util::read_string_value(
                HKEY_CURRENT_USER,
                &subkey,
                "ExecutablePath",
            )
            .unwrap_or_default(),
            guid: crate::win32::registry_util::read_string_value(
                HKEY_CURRENT_USER,
                &subkey,
                "IconGuid",
            )
            .and_then(|brut| guid_from_registry_text(&brut)),
            png,
            id: name.parse().unwrap_or(0),
        });
    }
    snapshots
}

thread_local! {
    /// L'exécutable de chaque sous-clé déjà lue, par son numéro — une sous-clé ne
    /// change pas d'application, et `read_ui_order` est appelée à chaque
    /// révision du flux : relire 146 valeurs à chaque fois serait le coût que
    /// `read_snapshots` s'interdit déjà.
    static EXE_PAR_SOUS_CLE: std::cell::RefCell<std::collections::HashMap<u64, String>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// **L'ORDRE DES ICÔNES TEL QUE LE SHELL LE RANGE** — chaque entrée de
/// `UIOrderList`, dans l'ordre de la valeur : le nom de sa sous-clé (voir
/// `TrayIconSnapshot::id`) et son exécutable (forme `exe_normalise`).
///
/// MESURÉ le 2026-10-02 : `UIOrderList` est une suite d'entiers de 64 bits
/// (petit-boutistes), chacun étant le NOM d'une sous-clé de `NotifyIconSettings`
/// — 146 sur 146 retrouvées. Réduite aux applications présentes, elle donne
/// **exactement l'ordre des cases du panneau**, chevron FERMÉ : c'est la seule
/// source de cet ordre qui ne demande pas d'ouvrir le panneau. Elle contient
/// aussi des entrées PÉRIMÉES (20 `WhatsApp.Root` ici) : l'appelant ne doit se
/// fier qu'aux exécutables qui n'y figurent qu'une fois.
///
/// Rend une liste vide si la valeur manque ou est malformée.
pub fn read_ui_order() -> Vec<(u64, String)> {
    use windows::Win32::System::Registry::HKEY_CURRENT_USER;

    let Some(brut) = crate::win32::registry_util::read_binary_value(HKEY_CURRENT_USER, SNAPSHOT_KEY, "UIOrderList")
    else {
        return Vec::new();
    };
    if brut.len() % 8 != 0 {
        return Vec::new();
    }
    EXE_PAR_SOUS_CLE.with(|cache| {
        let mut cache = cache.borrow_mut();
        brut.chunks_exact(8)
            .map(|octets| {
                let id = u64::from_le_bytes(octets.try_into().unwrap_or_default());
                let exe = cache
                    .entry(id)
                    .or_insert_with(|| {
                        let chemin = crate::win32::registry_util::read_string_value(
                            HKEY_CURRENT_USER,
                            &format!("{SNAPSHOT_KEY}\\{id}"),
                            "ExecutablePath",
                        )
                        .unwrap_or_default();
                        exe_normalise(&chemin)
                    })
                    .clone();
                (id, exe)
            })
            .collect()
    })
}

/// **CETTE ENTRÉE EST-ELLE SUR LA BARRE ?** — `IsPromoted` de sa sous-clé
/// (`None` si la valeur manque : MESURÉ, 126 entrées sur 146 n'en ont pas). Lu
/// à chaque appel, sans cache : l'utilisateur peut promouvoir une icône à tout
/// moment.
pub fn is_promoted(id: u64) -> Option<bool> {
    use windows::Win32::System::Registry::HKEY_CURRENT_USER;
    crate::win32::registry_util::read_dword_value(HKEY_CURRENT_USER, &format!("{SNAPSHOT_KEY}\\{id}"), "IsPromoted")
        .map(|v| v != 0)
}

/// **LES EXÉCUTABLES EN COURS** (forme `exe_normalise`) — un instantané
/// Toolhelp, sans ouvrir aucun processus.
///
/// C'est ce qui distingue, dans `UIOrderList`, une entrée VIVANTE d'une entrée
/// périmée quand le flux est éteint. Mesuré le 2026-10-02 : entre « Power
/// Display » et « Terminal », le registre place droidcam, DayZ et Signal — seul
/// Signal tourne, et c'est la case anonyme de ce rang.
pub fn executables_en_cours() -> std::collections::HashSet<String> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    let mut noms = std::collections::HashSet::new();
    let Ok(instantane) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return noms;
    };
    let mut entree = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut suite = unsafe { Process32FirstW(instantane, &mut entree) }.is_ok();
    while suite {
        let longueur = entree.szExeFile.iter().position(|&c| c == 0).unwrap_or(entree.szExeFile.len());
        noms.insert(exe_normalise(&String::from_utf16_lossy(&entree.szExeFile[..longueur])));
        suite = unsafe { Process32NextW(instantane, &mut entree) }.is_ok();
    }
    let _ = unsafe { windows::Win32::Foundation::CloseHandle(instantane) };
    noms
}

/// `C:\…\Discord.exe` → `discord` : la forme sous laquelle deux sources
/// différentes (registre, flux) désignent le même exécutable.
pub fn exe_normalise(chemin: &str) -> String {
    let nom = chemin.trim().rsplit(['\\', '/']).next().unwrap_or("").to_lowercase();
    nom.strip_suffix(".exe").unwrap_or(&nom).to_string()
}

/// Convertit la valeur `IconGuid` du registre — `{699E423D-73DA-42BA-B94E-881FF8672467}`
/// — en `GUID`. **`None` si la forme n'est pas canonique** : une valeur
/// malformée ne doit pas produire un GUID inventé, qui désignerait une icône
/// qui n'existe pas.
///
/// Vit ici, et pas dans `registry_util` : c'est le décodage d'une valeur **de
/// cette clé-là**, pas une opération de registre générique.
pub fn guid_from_registry_text(texte: &str) -> Option<windows::core::GUID> {
    let nu = texte.trim().trim_matches(|c| c == '{' || c == '}');
    windows::core::GUID::try_from(nu).ok()
}

/// **OÙ LE SHELL SITUE UNE ICÔNE IDENTIFIÉE PAR SON GUID.**
///
/// `Shell_NotifyIconGetRect` est une API **documentée**, appelable pour un GUID
/// **qu'on ne possède pas** : elle interroge, elle ne modifie rien, n'installe
/// aucune fenêtre, n'ajoute ni ne retire aucune icône, et ne demande aucun
/// privilège. `hWnd` nul + `uID` nul + `guidItem` : **jamais de fenêtre
/// imposteur** (c'est `tray_spy_probe`, et lui seul, qui en installe une).
///
/// MESURÉ le 2026-09-21 sur cette machine, et c'est le résultat qui porte tout
/// ce chemin :
///
/// * **chevron FERMÉ** — une icône **masquée** rend le rectangle **du chevron**,
///   le même pour toutes (les GUIDs situés se partageaient un unique rectangle) ;
/// * **chevron OUVERT** — chaque GUID rend **son** cellule du panneau, distincte
///   des autres, à ±1 px en abscisse de ce qu'UI Automation rapporte ;
/// * une icône **promue** rend son emplacement propre dans la barre, chevron
///   fermé comme ouvert (vérifié en aller-retour : promouvoir puis rétablir fait
///   suivre le rectangle dans les deux sens).
///
/// Rend `None` quand le shell refuse — c'est le cas des **33 entrées périmées**
/// du registre (applications non lancées), et ce n'est **pas** une erreur à
/// journaliser 142 fois par changement de liste.
pub fn rect_of_guid(guid: windows::core::GUID) -> Option<RECT> {
    use windows::Win32::UI::Shell::{Shell_NotifyIconGetRect, NOTIFYICONIDENTIFIER};

    let identifiant = NOTIFYICONIDENTIFIER {
        cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: windows::Win32::Foundation::HWND::default(),
        uID: 0,
        guidItem: guid,
    };
    unsafe { Shell_NotifyIconGetRect(&identifiant) }.ok()
}

/// Écart maximal, en pixels, entre deux rectangles qu'on croit au même endroit
/// **une fois le décalage systématique retiré** (voir `estimate_panel_offset`).
///
/// **CALIBRÉ, PAS DEVINÉ** — même statut que `ECART_IMAGE_MAX`. Mesuré le
/// 2026-09-21 sur les cinq GUIDs situés de cette machine : abscisse à **±1 px**,
/// et l'écart résiduel après retrait du décalage monte à **7 px** (Sécurité
/// Windows, décalée de +45 quand les autres le sont de +38). **Une tolérance de
/// 4, essayée d'abord, rejetait donc une correspondance JUSTE.**
///
/// 10 couvre les 7 px mesurés avec un peu de marge, et reste **six fois sous le
/// pas de la grille du panneau (60 px)** — c'est cette distance-là qui garantit
/// qu'une tolérance ne peut pas faire glisser une icône sur sa voisine.
pub const RECT_TOLERANCE: i32 = 10;

/// Nombre minimal de GUIDs qu'un décalage doit expliquer pour qu'on lui fasse
/// confiance. **À deux, ce n'est plus une coïncidence** ; à un seul, rien ne
/// distingue le bon décalage d'une rencontre fortuite — et se tromper d'identité
/// est pire que de n'en pas donner (même doctrine que `identify_by_elimination`).
pub const MIN_VOTES: usize = 2;

/// **LA CAPTURE DU PANNEAU DE DÉBORDEMENT — quand le chevron est ouvert.**
///
/// MESURÉ, ET C'EST LE SIGNAL QUI MANQUAIT : `IsWindowVisible` sur cette fenêtre
/// **est** valide — `false` chevron fermé, `true` ouvert. Une première conclusion
/// « toujours vrai » était FAUSSE : le chevron se trouvait simplement ouvert à ce
/// moment-là.
///
/// Le rectangle est celui du panneau : **234×314** en coordonnées virtualisées,
/// soit **351×471** en réel — la taille d'une grille de cinq colonnes d'icônes de
/// 40 px, et non celle du panneau d'emojis (qui se pose au même endroit et qu'on
/// photographiait à sa place).
///
/// ⚠️ **LE PROCESSUS APPELANT DOIT ÊTRE DPI-AWARE**, sinon `GetWindowRect` rend
/// des coordonnées virtualisées quand `grab_screen` capture l'écran réel, et la
/// capture tombe ailleurs. C'est le piège DPI que ce dépôt documente déjà ;
/// l'application l'est, une sonde `cargo test` ne l'est pas.
///
/// Rend `None` — sans erreur — quand le panneau est fermé : c'est le cas normal,
/// et l'appelant retente à la seconde suivante.
pub fn grab_overflow_panel() -> Option<(RECT, (u32, u32, Vec<u8>))> {
    let fenetre =
        unsafe { FindWindowW(w!("TopLevelWindowForOverflowXamlIsland"), PCWSTR::null()) }.ok()?;
    if !unsafe { IsWindowVisible(fenetre) }.as_bool() {
        return None;
    }
    let mut rect = RECT::default();
    unsafe { GetWindowRect(fenetre, &mut rect) }.ok()?;
    let pixels = grab_screen(rect)?;
    Some((rect, pixels))
}

/// Écart moyen entre une icône CAPTURÉE et un instantané du registre, **sur les
/// seuls pixels où l'instantané est OPAQUE**.
///
/// POURQUOI PAS UNE COMPARAISON PLEINE : le fond d'un instantané est
/// **transparent**, celui de la capture est la **teinte du panneau**. Comparer
/// les deux fonds écraserait tout le reste — c'est le logo qu'on veut comparer,
/// pas le fond. Les deux images sont d'abord ramenées à une même petite taille,
/// ce qui rend la mesure insensible aux quelques pixels de différence de découpe.
fn ecart_a_l_instantane(capture: &image::RgbaImage, instantane: &image::RgbaImage) -> Option<u32> {
    const COTE: u32 = 16;
    let petit = image::imageops::resize(capture, COTE, COTE, image::imageops::FilterType::Triangle);
    let reference =
        image::imageops::resize(instantane, COTE, COTE, image::imageops::FilterType::Triangle);
    let (mut somme, mut compte) = (0u32, 0u32);
    for (pixel, attendu) in petit.pixels().zip(reference.pixels()) {
        if attendu[3] < 128 {
            continue;
        }
        for canal in 0..3 {
            somme += (pixel[canal] as i32 - attendu[canal] as i32).unsigned_abs();
        }
        compte += 1;
    }
    // Trop peu de pixels opaques : l'instantané n'a pas de logo exploitable, et
    // un écart calculé sur trois pixels ne voudrait rien dire.
    (compte > 8).then(|| somme / (compte * 3))
}

/// **IDENTIFIER UNE ICÔNE PAR SON IMAGE, FAUTE DE NOM.**
///
/// MESURÉ : trois des trente et une entrées MASQUÉES n'ont **aucun** nom dans UI
/// Automation — ni aide, ni description, ni valeur : tout est vide. Windows n'en
/// expose aucun, le rapprochement par nom ne peut donc rien pour elles, et le
/// registre en porte pourtant l'icône **et** l'exécutable.
///
/// L'image capturée du panneau leur en donne une. La comparer aux instantanés du
/// registre, et rendre le plus ressemblant, **identifie sans deviner** : c'est une
/// mesure, pas une supposition sur ce qui tourne.
///
/// Rend `(index, écart)` — l'appelant décide si l'écart est assez petit pour être
/// cru (voir `ECART_IMAGE_MAX`).
pub fn match_snapshot_by_image(
    icone: &(u32, u32, Vec<u8>),
    snapshots: &[TrayIconSnapshot],
) -> Option<(usize, u32)> {
    let (largeur, hauteur, rgba) = icone;
    let capture = image::RgbaImage::from_raw(*largeur, *hauteur, rgba.clone())?;
    let mut meilleur: Option<(usize, u32)> = None;
    for (index, snapshot) in snapshots.iter().enumerate() {
        let Ok(charge) = image::load_from_memory(&snapshot.png) else { continue };
        let Some(ecart) = ecart_a_l_instantane(&capture, &charge.to_rgba8()) else { continue };
        if meilleur.map_or(true, |(_, meilleur_ecart)| ecart < meilleur_ecart) {
            meilleur = Some((index, ecart));
        }
    }
    meilleur
}

/// **IDENTIFIER PAR ÉLIMINATION — quand l'IMAGE ne peut pas suffire.**
///
/// MESURÉ, et c'est le cas de Discord : son icône dans la zone de notification
/// est un **cercle blanc**, alors que son instantané du registre est **le logo
/// Discord** (pris à l'ENREGISTREMENT, et l'application a changé d'icône depuis).
/// Aucune comparaison d'images ne peut rapprocher les deux, et la comparaison l'a
/// donc **correctement rejeté** — un faux nom serait pire qu'un cercle blanc.
///
/// ⚠️ **CE N'EST PAS UNE MESURE, ET LE CODE LE DIT.** On croise les exécutables
/// des instantanés avec ceux des FENÊTRES OUVERTES (une application qui a une
/// icône de notification est en train de tourner), on retire ceux qu'une entrée
/// DÉJÀ NOMMÉE réclame, et **on ne nomme que s'il ne reste QU'UN SEUL
/// candidat**. À deux candidats ou plus, on rend `None` : le point reste un
/// point, et le journal dit lesquels restaient — ce qui vaut mieux qu'un nom
/// choisi au hasard.
///
/// Rend l'index de l'instantané retenu, et la liste des candidats — même quand
/// elle est ambiguë, pour que l'ambiguïté se LISE au lieu de se taire.
pub fn identify_by_elimination(
    noms_deja_pris: &[String],
    exes_ouverts: &[String],
    snapshots: &[TrayIconSnapshot],
) -> (Option<usize>, Vec<String>) {
    let normaliser = |texte: &str| texte.trim().to_ascii_lowercase();
    let pris: Vec<String> = noms_deja_pris.iter().map(|nom| normaliser(nom)).collect();
    let ouverts: Vec<String> = exes_ouverts
        .iter()
        .map(|exe| normaliser(exe.trim_end_matches(".exe")))
        .collect();

    let mut candidats: Vec<(usize, String)> = Vec::new();
    for (index, snapshot) in snapshots.iter().enumerate() {
        let Some(tige) = std::path::Path::new(&snapshot.executable)
            .file_stem()
            .map(|tige| normaliser(&tige.to_string_lossy()))
        else {
            continue;
        };
        if tige.is_empty() || !ouverts.contains(&tige) || pris.contains(&tige) {
            continue;
        }
        candidats.push((index, tige));
    }
    candidats.dedup_by(|a, b| a.1 == b.1);
    let liste: Vec<String> = candidats.iter().map(|(_, nom)| nom.clone()).collect();
    // **UN SEUL, SINON RIEN.** C'est le fond du garde-fou : à plusieurs, nommer
    // serait choisir au hasard parmi des applications qui tournent toutes.
    match candidats.as_slice() {
        [(index, _)] => (Some(*index), liste),
        _ => (None, liste),
    }
}

/// **LE DÉCALAGE SYSTÉMATIQUE entre le rectangle rendu par le shell et celui
/// d'UI Automation — ESTIMÉ PAR VOTE, jamais toléré à l'aveugle.**
///
/// C'est le point où l'intuition se trompe, et la mesure le dit. Le shell ne
/// rend pas le rectangle de l'icône : il rend le sien, décalé. **MESURÉ le
/// 2026-09-21, sur les cinq GUIDs situés de cette machine : abscisse à ±1 px,
/// ordonnée décalée de +38 à +45.**
///
/// ⚠️ **Une tolérance aveugle ne peut PAS marcher ici**, et il faut le dire :
/// la grille du panneau a un pas de 60 px. Pour Phone Link, la cellule située
/// une rangée plus bas est à **22 px** de la réponse du shell, quand la BONNE
/// est à **38**. La règle « la plus proche » choisit donc systématiquement la
/// mauvaise — et une tolérance assez large pour attraper 38 attraperait aussi
/// les deux rangées voisines.
///
/// La seule règle qui tienne est donc d'**estimer** le décalage : on essaie
/// chaque couple (rectangle du shell, icône) comme hypothèse, et on retient
/// celui qui en explique le **plus grand nombre**. À `MIN_VOTES` au moins.
pub fn estimate_panel_offset(places: &[(usize, RECT)], icons: &[TrayIcon]) -> Option<(i32, i32)> {
    let mut meilleur: Option<(i32, i32, usize)> = None;
    for (_, rect) in places {
        for icone in icons {
            let dx = icone.rect.left - rect.left;
            let dy = icone.rect.top - rect.top;
            // On ne vote que sur les COLONNES : c'est la coordonnée que la
            // mesure donne exacte, et elle écarte d'emblée les rencontres
            // fortuites entre deux colonnes voisines.
            if dx.abs() > RECT_TOLERANCE {
                continue;
            }
            let voix = places
                .iter()
                .filter(|(_, autre)| {
                    icons.iter().any(|i| {
                        (i.rect.left - (autre.left + dx)).abs() <= RECT_TOLERANCE
                            && (i.rect.top - (autre.top + dy)).abs() <= RECT_TOLERANCE
                    })
                })
                .count();
            let remplace = match meilleur {
                None => true,
                Some((mdx, mdy, v)) => {
                    // À égalité de voix, on garde le décalage le PLUS PETIT :
                    // c'est le moins susceptible d'avoir rapproché deux icônes
                    // distinctes.
                    voix > v || (voix == v && dx.abs() + dy.abs() < mdx.abs() + mdy.abs())
                }
            };
            if remplace {
                meilleur = Some((dx, dy, voix));
            }
        }
    }
    match meilleur {
        Some((dx, dy, voix)) if voix >= MIN_VOTES => Some((dx, dy)),
        _ => None,
    }
}

/// L'icône que le décalage retenu place sous ce rectangle — `None` si aucune,
/// **ou si PLUSIEURS** (deux icônes au même endroit : anomalie d'UI Automation).
/// On ne choisit pas au hasard : on refuse.
pub fn icon_at_offset(rect: &RECT, decalage: (i32, i32), icons: &[TrayIcon]) -> Option<usize> {
    let cible = (rect.left + decalage.0, rect.top + decalage.1);
    let mut trouves = icons.iter().enumerate().filter(|(_, icone)| {
        (icone.rect.left - cible.0).abs() <= RECT_TOLERANCE
            && (icone.rect.top - cible.1).abs() <= RECT_TOLERANCE
    });
    let (premier, _) = trouves.next()?;
    if trouves.next().is_some() {
        return None;
    }
    Some(premier)
}

/// **QUELLE ICÔNE AFFICHÉE EST QUELLE APPLICATION** — les couples
/// `(index d'icône, index d'instantané)`, et le nombre de GUIDs que le shell a
/// **situés** (ce second nombre distingue « le shell a refusé » de « rien ne
/// correspond », et c'est la seule chose qu'on pourra lire dans le journal).
///
/// ⚠️ **LE CHEVRON DOIT ÊTRE OUVERT**, et c'est mesuré : panneau fermé, les
/// entrées masquées d'`enumerate()` rendent un rectangle **dégénéré**
/// (`0,0 0x0`), et le shell rend à toutes le rectangle du chevron. Panneau
/// ouvert, chaque GUID rend sa cellule. L'appelant doit donc n'appeler ceci que
/// dans la fenêtre où le panneau est déployé.
pub fn identify_icons_by_guid(
    icons: &[TrayIcon],
    snapshots: &[TrayIconSnapshot],
) -> (Vec<(usize, usize)>, usize) {
    let mut places: Vec<(usize, RECT)> = Vec::new();
    for (index, instantane) in snapshots.iter().enumerate() {
        if instantane.executable.trim().is_empty() {
            continue;
        }
        let Some(guid) = instantane.guid else { continue };
        let Some(rect) = rect_of_guid(guid) else { continue };
        places.push((index, rect));
    }
    let situes = places.len();
    let Some(decalage) = estimate_panel_offset(&places, icons) else {
        return (Vec::new(), situes);
    };
    let mut couples: Vec<(usize, usize)> = Vec::new();
    for (index, rect) in &places {
        if let Some(icone) = icon_at_offset(rect, decalage, icons) {
            // Une icône déjà identifiée par un autre GUID n'est pas reprise :
            // le premier gagne, et le doublon est un fait à journaliser, pas à
            // écraser en silence.
            if !couples.iter().any(|(deja, _)| *deja == icone) {
                couples.push((icone, *index));
            }
        }
    }
    (couples, situes)
}

/// **LE NOM AFFICHABLE D'UNE APPLICATION, DÉDUIT DE SON EXÉCUTABLE.**
///
/// `…\WindowsApps\…\WhatsApp.Root.exe` → `WhatsApp.Root`. `None` si le chemin
/// est vide ou sans nom de fichier — **et dans ce cas l'appelant NE NOMME PAS.**
///
/// On ne lit **pas** les `VERSIONINFO` (`FileDescription`, `ProductName`) alors
/// que `exe_metadata` sait le faire : l'`ExecutablePath` du registre est souvent
/// un chemin **pseudo** à `KNOWNFOLDERID` (`{6D809377-…}\WindowsApps\…`, mesuré)
/// que `GetFileVersionInfoSizeW` ne peut pas ouvrir — la source la plus riche
/// échouerait précisément sur les applications empaquetées qui en auraient le
/// plus besoin. Et **pas de repli sur `InitialTooltip`** : une infobulle est
/// souvent un ÉTAT (« Vous êtes protégé » est mesuré), et nommer à partir d'elle
/// produirait un nom qui ment.
pub fn name_from_executable(executable: &str) -> Option<String> {
    let chemin = executable.trim();
    // ⚠️ Un chemin qui FINIT par un séparateur désigne un DOSSIER, pas un
    // exécutable — et `Path::file_stem` en tirerait quand même le nom du
    // dossier (`C:\dossier\` → `dossier`). Nommer une icône d'après un dossier
    // serait un nom qui ment, et ce module refuse déjà les noms qui mentent.
    if chemin.is_empty() || chemin.ends_with('\\') || chemin.ends_with('/') {
        return None;
    }
    let tige = std::path::Path::new(chemin).file_stem()?;
    let nom = tige.to_string_lossy().trim().to_string();
    (!nom.is_empty()).then_some(nom)
}

/// **L'écart d'image en dessous duquel on croit à l'identification.** À mesurer
/// sur la machine : un vrai logo tombe près de zéro, un faux reste haut. Le
/// laisser TROP BAS ferait échouer des identifications justes ; trop haut, il
/// nommerait n'importe quoi.
pub const ECART_IMAGE_MAX: u32 = 40;

/// **L'ICÔNE ENREGISTRÉE qui correspond à une entrée vivante** — son index dans
/// `snapshots`.
///
/// Fonction PURE (les instantanés sont des données) : c'est la partie de ce
/// module qui décide QUELLE IMAGE s'affiche, donc celle qui mérite des tests.
///
/// Trois niveaux, du plus sûr au plus souple :
/// 1. **égalité** de l'infobulle normalisée — le cas des applications qui ne
///    changent pas la leur ;
/// 2. **inclusion** dans un sens ou dans l'autre, à partir de
///    `MIN_MATCH_CHARS` — MESURÉ : l'infobulle vivante porte souvent une version
///    ou un état que l'instantané n'a pas (« MSI Afterburner v4.6.6.16757 » contre
///    « MSI Afterburner ») ;
/// 3. **nom d'exécutable** contenu dans l'infobulle — le dernier recours, pour
///    une application qui nomme son exécutable mais pas son produit.
///
/// ⚠️ **Il ne trouve pas tout, et c'est irréductible** : une infobulle qui ne
/// nomme rien (« Vous êtes protégé », MESURÉ) ne peut être rapprochée d'aucune
/// entrée par son texte. L'appelant a d'autres replis.
pub fn match_snapshot(name: &str, snapshots: &[TrayIconSnapshot]) -> Option<usize> {
    let first_line = name.lines().next().unwrap_or(name);
    let needle = normalized(first_line);
    if needle.is_empty() {
        return None;
    }
    // Le PREMIER MOT du nom vivant — voir la règle 4 ci-dessous.
    let needle_word = normalized(first_line.split_whitespace().next().unwrap_or(""));

    let mut best: Option<(usize, usize)> = None;
    for (index, snapshot) in snapshots.iter().enumerate() {
        let tooltip = normalized(&snapshot.tooltip);
        let stem = snapshot.executable.rsplit(['\\', '/']).next().unwrap_or(&snapshot.executable);
        let stem = stem
            .strip_suffix(".exe")
            .or_else(|| stem.strip_suffix(".EXE"))
            .unwrap_or(stem);
        let key = normalized(stem);

        let mut score = 0usize;
        // 1. L'ÉGALITÉ l'emporte TOUJOURS : elle ne peut pas être un hasard,
        //    alors qu'une inclusion peut en être un.
        if !tooltip.is_empty() && tooltip == needle {
            score = 10_000;
        }
        // 2. ÉGALITÉ DU PREMIER MOT — MESURÉ : c'est ce qui rattrape GCC, dont
        //    l'infobulle porte une VERSION dans les deux cas mais pas la même
        //    (« GCC Ver26.08.28.01 » contre « GCC Ver25.07.21.01 »), donc aucune
        //    inclusion ne les rapproche. Le nom du PRODUIT, lui, est identique.
        //    Le seuil est de TROIS caractères ici, et non quatre : c'est un mot
        //    entier, pas un fragment — c'est ce qui rend le seuil plus bas sûr.
        if score == 0 && needle_word.len() >= MIN_WORD_CHARS {
            let tooltip_word = normalized(snapshot.tooltip.split_whitespace().next().unwrap_or(""));
            let stem_word = normalized(stem.split(|c: char| !c.is_alphanumeric()).next().unwrap_or(""));
            if tooltip_word == needle_word || stem_word == needle_word {
                score = 500;
            }
        }
        // 2 bis. LE PREMIER MOT EST UN PRÉFIXE DU NOM D'EXÉCUTABLE — le dernier
        //    recours avant l'initiale, et MESURÉ : « AMD Software: Adrenalin
        //    Edition » n'a AUCUNE correspondance par les règles ci-dessus (les
        //    trois entrées AMD du registre — `RadeonSoftware.exe`,
        //    `AMDInstallManager.exe`, `MoNotificationUx.exe` — ont toutes une
        //    infobulle VIDE et un nom qui ne recoupe pas celui du produit),
        //    alors que `amd` est le préfixe de `AMDInstallManager`. Le score est
        //    le PLUS FAIBLE de tous : il ne doit l'emporter que si rien d'autre
        //    n'a parlé, car il donne l'icône d'une AUTRE application du même
        //    éditeur — mieux qu'une lettre, mais moins qu'une correspondance.
        if score == 0 && needle_word.len() >= MIN_WORD_CHARS {
            let stem_word = normalized(stem.split(|c: char| !c.is_alphanumeric()).next().unwrap_or(""));
            if stem_word.starts_with(&needle_word) {
                score = 100;
            }
        }
        // 3. INCLUSION de l'infobulle, **DANS LES DEUX SENS** — MESURÉ : une
        //    infobulle vivante porte souvent une version ou un état que
        //    l'instantané n'a pas (« MSI Afterburner v4.6.6.16757 » contre « MSI
        //    Afterburner »), mais l'inverse arrive aussi.
        if score == 0
            && tooltip.len() >= MIN_MATCH_CHARS
            && (needle.contains(&tooltip) || tooltip.contains(&needle))
        {
            score = tooltip.len();
        }
        // 4. INCLUSION DU NOM D'EXÉCUTABLE, **DANS LES DEUX SENS AUSSI** — et le
        //    sens inverse est un CORRECTIF : MESURÉ, WhatsApp n'a pas
        //    d'exécutable `WhatsApp.exe` mais `WhatsApp.Root.exe` (application
        //    empaquetée), donc son nom ne CONTIENT pas la racine de son
        //    exécutable ; c'est la racine qui contient son nom. Sans ce sens, il
        //    n'obtenait aucune correspondance et affichait une lettre.
        if score == 0 && key.len() >= MIN_MATCH_CHARS && (needle.contains(&key) || key.contains(&needle)) {
            score = key.len();
        }

        if score > 0 && best.is_none_or(|(_, best_score)| score > best_score) {
            best = Some((index, score));
        }
    }
    best.map(|(index, _)| index)
}

/// **L'APPLICATION INSTALLÉE dont le nom correspond à celui d'une entrée de
/// notification** — son index dans `names`.
///
/// **C'EST L'IDENTIFICATION AUTOMATIQUE**, et elle vient du menu Démarrer.
/// MESURÉ : « Mobile connecté » est le nom affiché de Phone Link
/// (`Microsoft.YourPhone_8wekyb3d8bbwe!App`), et « Sécurité Windows » celui de
/// Windows Security (`Microsoft.SecHealthUI_8wekyb3d8bbwe!SecHealthUI`). Ni UI
/// Automation ni le registre ne donnent cette correspondance (leurs propriétés
/// de description et de valeur sont vides — mesuré) ; **le nom affiché de
/// l'application, lui, EST l'infobulle de son icône de notification**, et
/// `win32::apps` lit déjà cette liste pour le Dash.
///
/// Mêmes règles que `match_snapshot` — égalité, premier mot, inclusion : ce sont
/// les MÊMES noms, vus des deux côtés.
pub fn match_installed_app(name: &str, names: &[String]) -> Option<usize> {
    let first_line = name.lines().next().unwrap_or(name);
    let needle = normalized(first_line);
    if needle.is_empty() {
        return None;
    }
    let needle_word = normalized(first_line.split_whitespace().next().unwrap_or(""));
    let mut best: Option<(usize, usize)> = None;
    for (index, candidate) in names.iter().enumerate() {
        let key = normalized(candidate);
        if key.is_empty() {
            continue;
        }
        let mut score = 0usize;
        if key == needle {
            score = 10_000;
        } else if needle_word.len() >= MIN_WORD_CHARS {
            let key_word = normalized(candidate.split_whitespace().next().unwrap_or(""));
            if key_word == needle_word {
                score = 500;
            }
        }
        if score == 0 && key.len() >= MIN_MATCH_CHARS && (needle.contains(&key) || key.contains(&needle)) {
            score = key.len();
        }
        if score > 0 && best.is_none_or(|(_, best_score)| score > best_score) {
            best = Some((index, score));
        }
    }
    best.map(|(index, _)| index)
}

/// **LE RECTANGLE DE LA BARRE DES TÂCHES, SI ELLE EST ACTUELLEMENT DESSINÉE.**
///
/// `None` tant qu'elle est masquée — et c'est le cas normal : elle est en
/// AUTO-MASQUAGE, donc sa fenêtre est garée HORS ÉCRAN (MESURÉ : `y = 1439` sur
/// un écran de 1440) et **rien n'est peint nulle part**.
///
/// **POURQUOI CETTE FONCTION EXISTE.** Tant que la barre est masquée, ses icônes
/// ne sont pas capturables : mesuré, `Windows.Graphics.Capture` ouvre bien une
/// session sur `Shell_TrayWnd` et livre une frame, mais **intégralement noire**
/// (une seule couleur sur 276 480 pixels) — le compositeur ne la dessine pas.
/// Dès que l'utilisateur l'amène au bord, elle est dessinée, et **une capture
/// d'écran ordinaire donne alors son ÉTAT COURANT** — ce que ni le registre
/// (instantané pris à l'enregistrement) ni UI Automation (aucun pixel) ne
/// donnent.
pub fn revealed_taskbar_rect() -> Option<RECT> {
    // `vraie_barre` et non `FindWindowW` : voir `tray_flux`.
    let taskbar = crate::win32::tray_flux::vraie_barre()?;
    if taskbar.0.is_null() {
        return None;
    }
    let mut rect = RECT::default();
    unsafe { GetWindowRect(taskbar, &mut rect) }.ok()?;
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return None;
    }
    // **LE TEST EST LA POSITION, PAS LA VISIBILITÉ** : `IsWindowVisible` rend
    // `true` même garée hors écran (MESURÉ) — elle ne dit donc rien. Ce qui
    // compte est que la barre soit DANS l'écran, donc effectivement peinte.
    let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let on_screen = rect.top >= 0
        && rect.left >= 0
        && rect.right <= screen_width
        && rect.bottom <= screen_height;
    on_screen.then_some(rect)
}

/// **RELIT UNE RÉGION DE L'ÉCRAN EN RGBA** — la capture qui sert quand la barre
/// est révélée (voir `revealed_taskbar_rect`).
///
/// GDI et non `Windows.Graphics.Capture` : ce qu'on veut est ce qui est PEINT À
/// L'ÉCRAN, et GDI le donne directement, sans session, sans frame pool, et sans
/// le coût d'un périphérique de composition. WGC n'a été qu'un détour — il
/// fonctionne, mais rend une surface noire tant que la barre est masquée.
pub fn grab_screen(rect: RECT) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
    };
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return None;
    }
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        // NÉGATIF : DIB « top-down », donc les lignes se lisent dans l'ordre
        // naturel — rien à retourner à la main.
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let info = BITMAPINFO { bmiHeader: header, ..Default::default() };
    let mut pixels: *mut std::ffi::c_void = std::ptr::null_mut();
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut pixels, None, 0);
        let Ok(bitmap) = bitmap else {
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            return None;
        };
        let previous = SelectObject(memory, bitmap.into());
        let copied =
            BitBlt(memory, 0, 0, width, height, Some(screen), rect.left, rect.top, SRCCOPY).is_ok();
        SelectObject(memory, previous);
        if !copied || pixels.is_null() {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            return None;
        }
        // BGRA -> RGBA. L'alpha est forcé à 255 : une capture d'écran n'en a
        // pas, et laisser le canal tel quel donnerait des icônes transparentes.
        let count = (width * height) as usize;
        let raw = std::slice::from_raw_parts(pixels as *const u8, count * 4);
        let mut rgba = Vec::with_capacity(count * 4);
        for pixel in raw.chunks_exact(4) {
            rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
        }
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        Some((width as u32, height as u32, rgba))
    }
}

/// Teinte maximale (sur les trois canaux) d'une couleur qu'on accepte de traiter
/// comme un FOND NOIR à détacher — voir `trim_png`.
const MAX_BACKDROP_CHANNEL: u8 = 40;

/// **DÉTACHE UN FOND NOIR OPAQUE** — MESURÉ : sur les 141 instantanés du registre
/// de cette machine, **11 sont SANS canal alpha**, avec un coin à `(0,0,0)`,
/// `(13,13,13)`, `(16,16,16)`, `(29,30,30)` ou `(31,31,31)`. C'est le shell qui
/// les a enregistrés ainsi — et c'est ce que l'utilisateur voit comme « des
/// icônes sur un fond noir » (DEMANDE EXPLICITE : « retire le fond noir »).
///
/// ⚠️ **ON NE DÉTACHE QUE LE NOIR, ET C'EST UN GARDE-FOU MESURÉ.** Les autres
/// instantanés opaques ont un fond qui EST leur logo : `Macro Buttons` et
/// `Voicemeeter` ont un coin à `(250,71,48)` — un carré rouge. Détacher « la
/// couleur dominante du bord » sans autre critère effacerait leur marque. On
/// n'agit donc que si cette couleur est **noire ou quasi noire**, ce qui est le
/// seul cas observé où c'est un fond et non une marque.
///
/// Le détachement lui-même est celui de `key_out_background` — couleur dominante
/// de l'anneau, et on renonce si elle ne couvre pas l'essentiel du bord.
fn key_out_opaque_dark_background(rgba: &mut [u8], width: u32, height: u32) -> Option<(u8, u8, u8)> {
    if width < 4 || height < 4 {
        return None;
    }
    // Rien à faire si l'image a DÉJÀ de la transparence : c'est une bonne icône.
    let opaque = rgba.chunks_exact(4).all(|pixel| pixel[3] == 255);
    if !opaque {
        return None;
    }
    let mut ring: Vec<(u8, u8, u8)> = Vec::new();
    let mut push = |x: u32, y: u32| {
        let offset = ((y * width + x) * 4) as usize;
        ring.push((rgba[offset], rgba[offset + 1], rgba[offset + 2]));
    };
    for x in 0..width {
        push(x, 0);
        push(x, height - 1);
    }
    for y in 1..height - 1 {
        push(0, y);
        push(width - 1, y);
    }
    let mut counts: std::collections::HashMap<(u8, u8, u8), usize> = std::collections::HashMap::new();
    for colour in &ring {
        *counts.entry(*colour).or_default() += 1;
    }
    let Some((&dominant, &hits)) = counts.iter().max_by_key(|(_, count)| **count) else {
        return None;
    };
    // LE GARDE-FOU : noir ou quasi noir, et présent sur l'essentiel du bord.
    let dark = dominant.0 <= MAX_BACKDROP_CHANNEL
        && dominant.1 <= MAX_BACKDROP_CHANNEL
        && dominant.2 <= MAX_BACKDROP_CHANNEL;
    if !dark || (hits as f64) < (ring.len() as f64) * 0.35 {
        return None;
    }
    key_out_background(rgba, width, height)
}

/// Composante en dessous de laquelle un pixel compte pour NOIR.
const NOIR_MAX: u8 = 40;

/// **UN INSTANTANÉ ENTIÈREMENT NOIR N'EST PAS UNE ICÔNE** — fonction PURE.
/// Vrai si TOUS les pixels visibles (alpha > 128) sont noirs.
///
/// ⛔ MESURÉ le 2026-10-02 : l'instantané de Touch Portal est un 24×24 dont 57 %
/// des pixels sont visibles, et 100 % d'entre eux noirs — l'application
/// enregistre une icône puis la remplace, et le registre a gardé la première.
/// Affiché, c'était un pavé sombre au lieu du logo. Le seuil est TOTAL, et c'est
/// voulu : sur les 47 instantanés des applications en cours, le plus sombre
/// après lui est celui de NVIDIA, à 78 % de noir, et c'est un vrai logo sombre.
fn entierement_noire(rgba: &[u8]) -> bool {
    let mut visibles = rgba.chunks_exact(4).filter(|p| p[3] > 128).peekable();
    visibles.peek().is_some() && visibles.all(|p| p[0] < NOIR_MAX && p[1] < NOIR_MAX && p[2] < NOIR_MAX)
}

/// **DÉCODE UN PNG D'ICÔNE ET LE ROGNE À SON CONTENU** (voir `trim_to_content`).
///
/// **POUR QUE TOUTES LES SOURCES REMPLISSENT LEUR CASE DE LA MÊME FAÇON.** Les
/// PNG du registre sont des instantanés qui portent souvent leur propre
/// rembourrage transparent, alors que ceux issus d'une CAPTURE n'en ont plus
/// (voir `crop_icon`, qui rogne) : sans cette fonction, deux icônes voisines de
/// la même grille n'auraient pas la même taille apparente, et la marge réglée par
/// l'utilisateur s'ajouterait à un vide qui ne dit pas son nom.
///
/// ...et **il détache d'abord un fond NOIR OPAQUE** s'il y en a un (voir
/// `key_out_opaque_dark_background`) : onze instantanés de cette machine en
/// portent un.
///
/// Rend `None` si les octets ne sont pas une image décodable — l'appelant a ses
/// replis — **ou si l'image est ENTIÈREMENT NOIRE** (voir `entierement_noire`).
pub fn trim_png(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let decoded = image::load_from_memory(png).ok()?.to_rgba8();
    let (width, height) = decoded.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let mut rgba = decoded.into_raw();
    if entierement_noire(&rgba) {
        return None;
    }
    let background = key_out_opaque_dark_background(&mut rgba, width, height);
    Some(trim_to_content(&rgba, width, height, background))
}

/// **L'ICÔNE D'UNE ENTRÉE, DÉCOUPÉE DANS UNE CAPTURE DE LA BARRE** — le
/// rectangle de l'entrée, ramené dans les coordonnées de la bande capturée.
///
/// Rend `None` si l'entrée tombe hors de la bande (barre déplacée entre la
/// mesure UIA et la capture, écran de géométrie différente) : mieux vaut ne pas
/// capturer que de découper au mauvais endroit.
pub fn crop_icon(
    band: RECT,
    band_pixels: &(u32, u32, Vec<u8>),
    icon: &TrayIcon,
) -> Option<(u32, u32, Vec<u8>)> {
    let (band_width, band_height, rgba) = band_pixels;
    let x = icon.rect.left - band.left;
    let y = icon.rect.top - band.top;
    let width = icon.rect.right - icon.rect.left;
    let height = icon.rect.bottom - icon.rect.top;
    if width <= 0 || height <= 0 || x < 0 || y < 0 {
        return None;
    }
    if (x + width) as u32 > *band_width || (y + height) as u32 > *band_height {
        return None;
    }
    let mut cropped = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height {
        let start = (((y + row) * *band_width as i32 + x) * 4) as usize;
        cropped.extend_from_slice(&rgba[start..start + (width * 4) as usize]);
    }
    let _ = key_out_background(&mut cropped, width as u32, height as u32);
    // ROGNAGE AU CONTENU — voir `trim_capture_to_content`, ET NON `trim_to_content`.
    // **C'est la différence qui fait la taille des icônes VISIBLES** : une capture
    // d'écran a pour fond la BARRE DES TÂCHES FLOUTÉE, dont la teinte varie d'un
    // bord à l'autre, donc une référence unique fait compter les bords comme du
    // contenu et la boîte reste celle du bouton entier (mesuré : 48×48 pour un
    // rect de 36×72, l'icône occupant la moitié). Le rognage local à chaque ligne
    // enlève ce rembourrage, sans quoi la marge réglée par l'utilisateur s'y
    // AJOUTAIT et les icônes capturées paraissaient deux fois plus petites.
    Some(trim_capture_to_content(&cropped, width as u32, height as u32))
}

/// **DÉTACHE LE FOND D'UNE ICÔNE CAPTURÉE** — une capture d'écran n'a pas de
/// canal alpha : l'icône arrive avec un carré de la couleur de la barre des
/// tâches, qui se verrait dans le dock (transparent).
///
/// **LA COULEUR VIENT DU BORD, ET DU PLUS FRÉQUENT.** Le premier jet comparait
/// les QUATRE COINS entre eux et renonçait dès qu'ils différaient — MESURÉ, et
/// signalé par l'utilisateur : « certaines des icônes […] sont sur un fond
/// noir ». Il suffit qu'une icône touche un coin, ou que la barre ait la moindre
/// variation, pour que le détachement soit abandonné et que le fond RESTE. On
/// prend donc la couleur dominante de l'anneau de bord, qui est le fond même
/// quand un coin est occupé.
///
/// **ON RENONCE SI ELLE N'EST PAS DOMINANTE** (moins de `MIN_BORDER_SHARE` de
/// l'anneau) : mieux vaut un carré visible qu'une icône trouée au hasard. Et la
/// tolérance reste serrée — elle doit couvrir le bruit de la composition, pas
/// manger les pixels de l'icône qui ressembleraient au fond.
fn key_out_background(rgba: &mut [u8], width: u32, height: u32) -> Option<(u8, u8, u8)> {
    const TOLERANCE: i32 = 20;
    /// Part de l'anneau que la couleur dominante doit couvrir pour qu'on la
    /// considère comme le fond.
    const MIN_BORDER_SHARE: f64 = 0.35;
    if width < 4 || height < 4 {
        return None;
    }
    // L'ANNEAU DE BORD : première et dernière lignes, première et dernière
    // colonnes.
    let mut ring: Vec<(u8, u8, u8)> = Vec::new();
    let mut push = |x: u32, y: u32| {
        let offset = ((y * width + x) * 4) as usize;
        ring.push((rgba[offset], rgba[offset + 1], rgba[offset + 2]));
    };
    for x in 0..width {
        push(x, 0);
        push(x, height - 1);
    }
    for y in 1..height - 1 {
        push(0, y);
        push(width - 1, y);
    }
    let mut counts: std::collections::HashMap<(u8, u8, u8), usize> = std::collections::HashMap::new();
    for colour in &ring {
        *counts.entry(*colour).or_default() += 1;
    }
    let Some((&dominant, &hits)) = counts.iter().max_by_key(|(_, count)| **count) else {
        return None;
    };
    if (hits as f64) < (ring.len() as f64) * MIN_BORDER_SHARE {
        return None;
    }
    let (background_r, background_g, background_b) = (dominant.0 as i32, dominant.1 as i32, dominant.2 as i32);
    for pixel in rgba.chunks_exact_mut(4) {
        let close = (pixel[0] as i32 - background_r).abs() <= TOLERANCE
            && (pixel[1] as i32 - background_g).abs() <= TOLERANCE
            && (pixel[2] as i32 - background_b).abs() <= TOLERANCE;
        if close {
            pixel[3] = 0;
        }
    }
    Some(dominant)
}

/// **ROGNE UNE ICÔNE À SON CONTENU** — rend le sous-rectangle qui porte au moins
/// un pixel opaque, et rien de plus.
///
/// **POURQUOI C'EST NÉCESSAIRE.** Le rectangle qu'UI Automation donne pour une
/// icône de notification est celui de son BOUTON, qui porte un rembourrage : le
/// glyphe n'en occupe que le centre. Sans rognage, ce vide partait dans la
/// texture et s'ajoutait à la marge réglée par l'utilisateur (DEMANDE EXPLICITE :
/// « retire […] la marge qui ne devrait pas exister ») — les icônes capturées
/// paraissaient alors deux fois plus espacées que celles venues du registre,
/// dont les PNG sont déjà ajustés.
///
/// Si rien n'est opaque (détachement raté, case vide), on rend la case ENTIÈRE
/// plutôt qu'un rectangle dégénéré : l'appelant a un repli pour une texture
/// vide, il n'en a pas pour une géométrie absurde.
/// **ROGNER AU CONTENU QUAND LE FOND VARIE — le cas d'une CAPTURE D'ÉCRAN.**
///
/// MESURÉ, ET C'EST LE DÉFAUT RÉEL : les icônes **VISIBLES** de la zone de
/// notification sont **capturées sur la barre des tâches**, dont le fond est
/// FLOUTÉ et **change de teinte d'un bord à l'autre de la capture**. Avec une
/// référence de fond unique, les pixels des bords différaient de plus que la
/// marge, comptaient comme du CONTENU, et la boîte restait celle du bouton
/// entier : mesuré, `Réseau` (rect 36×72) et `Avertissement` (rect 48×72)
/// rendaient tous deux une texture de **48×48** — la largeur COMPLÈTE de la
/// capture, alors que l'icône n'en occupe que la moitié. Elle était donc dessinée
/// **deux fois trop petite** dans sa case — et **seules les visibles**, les
/// masquées venant d'un PNG à fond transparent où le rognage fonctionne.
///
/// Ici la référence est prise **sur CHAQUE LIGNE, à ses deux extrémités** : une
/// barre floutée varie lentement horizontalement, donc chaque ligne a son propre
/// fond, et le contenu se détache quand même. L'écart toléré est plus large que
/// dans `trim_to_content` — il doit absorber le grain du flou, pas un logo.
fn trim_capture_to_content(rgba: &[u8], width: u32, height: u32) -> (u32, u32, Vec<u8>) {
    const MARGE: i32 = 40;
    let contenu = |x: u32, y: u32| -> bool {
        let offset = ((y * width + x) * 4) as usize;
        let debut = (y * width * 4) as usize;
        let fin = ((y * width + width - 1) * 4) as usize;
        for canal in 0..3 {
            let reference = (rgba[debut + canal] as i32 + rgba[fin + canal] as i32) / 2;
            if (rgba[offset + canal] as i32 - reference).abs() > MARGE {
                return true;
            }
        }
        false
    };
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0u32, 0u32);
    for y in 0..height {
        for x in 0..width {
            if contenu(x, y) {
                left = left.min(x);
                top = top.min(y);
                right = right.max(x);
                bottom = bottom.max(y);
            }
        }
    }
    if right < left || bottom < top {
        return (width, height, rgba.to_vec());
    }
    carre_autour_de(rgba, width, left, top, right, bottom)
}

/// **LE CONTENU, CENTRÉ SUR UN CARRÉ TRANSPARENT.** Extrait pour être partagé par
/// les deux rognages — voir `trim_to_content` pour pourquoi le carré est
/// indispensable (le dessin ÉTIRE la texture à la case).
pub(crate) fn carre_autour_de(
    rgba: &[u8],
    width: u32,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
) -> (u32, u32, Vec<u8>) {
    let content_width = right - left + 1;
    let content_height = bottom - top + 1;
    let side = content_width.max(content_height);
    let offset_x = (side - content_width) / 2;
    let offset_y = (side - content_height) / 2;
    let mut squared = vec![0u8; (side * side * 4) as usize];
    for y in 0..content_height {
        let source = (((top + y) * width + left) * 4) as usize;
        let destination = (((y + offset_y) * side + offset_x) * 4) as usize;
        squared[destination..destination + (content_width * 4) as usize]
            .copy_from_slice(&rgba[source..source + (content_width * 4) as usize]);
    }
    (side, side, squared)
}

pub(crate) fn trim_to_content(
    rgba: &[u8],
    width: u32,
    height: u32,
    background: Option<(u8, u8, u8)>,
) -> (u32, u32, Vec<u8>) {
    // **CE QUI COMPTE COMME CONTENU.** Un pixel de FOND rescapé du détachement
    // (une frange d'anti-aliasing, une variation du fond) ne doit PAS compter :
    // sinon la boîte englobante reste celle du canevas entier, le rognage ne
    // rogne rien, et le logo — petit au milieu d'un canevas transparent étiré à
    // la case — paraît PLUS PETIT que les autres icônes. C'est MESURÉ : `MK205
    // Gaming Keyboard` restait en 16×16 après détachement, et l'utilisateur l'a
    // vu (« les icônes pour lesquelles tu as retiré le fond noir sont trop
    // petites par rapport aux autres »).
    const BACKGROUND_MARGIN: i32 = 26;
    // **UN ALPHA IMPERCEPTIBLE N'EST PAS DU CONTENU.** Exiger un alpha EXACTEMENT
    // nul laissait compter un liseré d'anti-aliasing ou une ombre à peine visible
    // (alpha 1 à 15) : la boîte englobante s'élargissait d'autant, le complément à
    // un carré grandissait avec elle, et **le logo occupait une part plus petite de
    // sa case** — « certaines icônes semblent avoir une marge invisible qui fait
    // qu'elles sont plus petites que les autres ». Le seuil est bas À DESSEIN : il
    // ne retire que ce que l'œil ne voit pas, jamais le bord utile d'un dessin.
    const ALPHA_MIN_CONTENU: u8 = 16;
    let is_content = |offset: usize| -> bool {
        if rgba[offset + 3] < ALPHA_MIN_CONTENU {
            return false;
        }
        let Some((r, g, b)) = background else { return true };
        let far = (rgba[offset] as i32 - r as i32).abs() > BACKGROUND_MARGIN
            || (rgba[offset + 1] as i32 - g as i32).abs() > BACKGROUND_MARGIN
            || (rgba[offset + 2] as i32 - b as i32).abs() > BACKGROUND_MARGIN;
        far
    };
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0u32, 0u32);
    for y in 0..height {
        for x in 0..width {
            if !is_content(((y * width + x) * 4) as usize) {
                continue;
            }
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    if right < left || bottom < top {
        return (width, height, rgba.to_vec());
    }
    // **COMPLÉMENT À UN CARRÉ, ET C'EST INDISPENSABLE ICI.** Le dessin ÉTIRE la
    // texture à la case (`Dock::external_icon_instance`) : une texture 200×128
    // serait donc DÉFORMÉE dans une case carrée. Centrée sur un carré
    // transparent, l'icône garde ses proportions ET occupe sa case au maximum —
    // c'est la plus grande taille possible sans la déformer.
    carre_autour_de(rgba, width, left, top, right, bottom)
}

/// **CLASSE DE L'OMBRE PORTÉE PAR LE SYSTÈME** — une fenêtre à part, que Windows
/// tient alignée sur SA fenêtre. On ne la déplace jamais seule : visée comme un
/// menu, elle se DÉTACHERAIT de celui qu'elle ombre. Mesurée derrière le menu Qt
/// de MEGAsync, cinq pixels plus grande que lui et au même endroit.
const SHADOW_WINDOW_CLASS: &str = "SysShadow";

/// **CLASSE DES POPUPS XAML DU SHELL** — le menu de l'icône « Réseau » en est un.
///
/// MESURÉ : le shell **reprend la pose** de cette fenêtre, à chaque tentative —
/// quatre poses reprises de 1188 px d'affilée, puis une pose tardive qui n'a tenu
/// qu'une fois sur deux. Elle n'est donc pas déplaçable de façon fiable : on n'y
/// touche pas (voir `place_new_menu`, qui détaille les trois tentatives). Nom
/// interne de Windows, donc fragile par nature ; s'il change un jour, le pire qui
/// arrive est qu'on se remette à déplacer un popup du shell — c'est-à-dire le
/// comportement d'avant, qui clignotait.
const XAML_POPUP_CLASS: &str = "Xaml_WindowedPopupClass";

/// Les fenêtres de **PREMIER NIVEAU** visibles, avec leur classe.
///
/// C'est la population où chercher un menu (voir `new_menu_window`), et le
/// premier niveau est essentiel : l'île XAML de la barre des tâches
/// (`DesktopWindowContentBridge`) est un **enfant**, et elle devient visible à
/// chaque clic quand la barre est en auto-masquage — la prendre pour un menu
/// déplacerait la barre des tâches elle-même.
fn visible_toplevel() -> Vec<(HWND, String)> {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let out = &mut *(lparam.0 as *mut Vec<(HWND, String)>);
        if IsWindowVisible(hwnd).as_bool() {
            out.push((hwnd, class_name(hwnd)));
        }
        windows::core::BOOL(1)
    }
    let mut out: Vec<(HWND, String)> = Vec::new();
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut out as *mut _ as isize)) };
    out
}

/// Le pid qui possède une fenêtre.
fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// **LE MENU QUI VIENT DE S'OUVRIR — QUELLE QUE SOIT SA CLASSE, et c'est tout le
/// correctif.**
///
/// MESURÉ, et c'est ce qui a fait échouer la première version : **le menu d'une
/// icône de notification n'est pas toujours un menu Win32.** MEGAsync (Qt)
/// affiche le sien dans une fenêtre **à lui**, de classe
/// `Qt51516QWindowPopupDropShadowSaveBits`, propriété de `MEGAsync.exe` — un
/// filtre sur `#32768` ne la voyait jamais, et le menu restait où il était né.
///
/// La règle est donc : **une fenêtre de PREMIER NIVEAU, VISIBLE, APPARUE depuis
/// le clic, et qui n'est pas à nous.** Trois exclusions, chacune mesurée :
///
/// - **pas à nous** : l'infobulle du dock (`NeuraDeskTooltipWindow`) naît à
///   chaque survol d'icône, et se ferait prendre pour un menu pour rien ;
/// - **pas une ombre** (voir `SHADOW_WINDOW_CLASS`) ;
/// - **premier niveau seulement** : l'île XAML de la barre des tâches est un
///   enfant, et elle devient visible à chaque clic quand la barre est en
///   auto-masquage.
///
/// Rien en vue veut dire **rien à déplacer**, et ce n'est pas une erreur : une
/// icône du SHELL n'ouvre aucune fenêtre par `ShowContextMenu` (mesuré) — voir
/// la limite écrite dans `show_context_menu`.
fn new_menu_window(avant: &[isize]) -> Option<HWND> {
    let notre_pid = std::process::id();
    visible_toplevel()
        .into_iter()
        .find(|(hwnd, classe)| {
            !avant.contains(&(hwnd.0 as isize))
                && window_pid(*hwnd) != notre_pid
                && classe != SHADOW_WINDOW_CLASS
        })
        .map(|(hwnd, _)| hwnd)
}

/// **TOUTES les fenêtres VISIBLES de la session**, premier niveau ET descendants,
/// avec leur classe.
///
/// **Sert à répondre à « aucun menu n'est apparu, alors QU'EST-CE QUI EST
/// apparu ? »** — la question qui a débloqué tout ce chantier. Un menu de la zone
/// de notification n'est pas toujours une fenêtre de premier niveau : le popup
/// XAML du shell et l'île qui l'héberge peuvent être des **enfants**, que
/// `EnumWindows` ne verrait pas. D'où le parcours de tout l'arbre — réservé au
/// chemin d'ÉCHEC (une fois par clic infructueux), jamais à la pose.
fn visible_windows() -> Vec<(HWND, String)> {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let out = &mut *(lparam.0 as *mut Vec<(HWND, String)>);
        if IsWindowVisible(hwnd).as_bool() {
            out.push((hwnd, class_name(hwnd)));
        }
        windows::core::BOOL(1)
    }
    let mut out: Vec<(HWND, String)> = Vec::new();
    let _ = unsafe {
        EnumChildWindows(
            Some(GetDesktopWindow()),
            Some(collect),
            LPARAM(&mut out as *mut _ as isize),
        )
    };
    out
}

/// **DÉPLACE LE MENU QUI VIENT DE S'OUVRIR DEVANT LA BARRE DE DROITE.**
///
/// DEMANDE EXPLICITE : « déplacer le menu à gauche de la barre de droite au
/// niveau de l'icône ». C'est le seul moyen : l'application décide de
/// l'emplacement de SON menu, et Windows n'offre aucun paramètre pour le lui
/// demander — mais son menu est une **fenêtre**, et une fenêtre se déplace.
///
/// **ON LE RETROUVE PAR DIFFÉRENCE** : les fenêtres visibles au moment du clic
/// sont notées, et celle qui apparaît ensuite est la bonne — quelle que soit sa
/// classe (voir `new_menu_window`, qui porte la mesure). Un balayage par « la
/// première trouvée » prendrait un menu resté ouvert ailleurs.
///
/// **LA RECHERCHE EST BORNÉE DANS LE TEMPS** (voir `MENU_APPEARANCE_MS`) : un
/// menu s'affiche de façon asynchrone, mais une application peut aussi refuser
/// d'en ouvrir un — et on n'attend pas indéfiniment un menu qui ne viendra pas.
///
/// Le menu est posé **à gauche** de `menu_right` et **centré** sur
/// `menu_centre_y`, puis RAMENÉ DANS L'ÉCRAN : un menu plus haut que l'écran, ou
/// plus large que la place disponible à gauche, serait autrement inatteignable.
///
/// **Tourne sur son PROPRE FIL** (voir `show_context_menu`), et reçoit pour cela
/// les fenêtres en `isize` : `HWND` n'est pas `Send`.
fn place_new_menu(avant: &[isize], nom: &str, menu_right: i32, menu_centre_y: i32) {
    // 1000 et non 400 : ce fil compte DEPUIS LE CLIC, alors que l'ancien
    // décompte partait du retour de `ShowContextMenu` — or c'est le CLIC qui
    // précède l'apparition (mesurée ~260 ms après lui, dont 233 ms passées dans
    // l'appel). Le fil ne fait que dormir : un budget large ne coûte rien.
    const MENU_APPEARANCE_MS: u64 = 1000;
    /// Combien de temps on observe après la pose, et l'écart toléré entre la
    /// position demandée et la position relue.
    const POSE_SETTLE_MS: u64 = 300;
    const POSE_TOLERANCE_PX: i32 = 8;
    /// **Combien de fois on repose le menu** — voir la boucle de pose : un menu
    /// peut s'AGRANDIR après avoir été posé (mesuré : pCloud, 200×307 puis
    /// 300×461), et `SetWindowPos` ne règle que la position.
    const POSE_ATTEMPTS: u32 = 3;
    // Chronomètre de diagnostic (voir `show_context_menu`).
    let debut = std::time::Instant::now();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(MENU_APPEARANCE_MS);
    while std::time::Instant::now() < deadline {
        if let Some(menu) = new_menu_window(avant) {
            // **UN POPUP XAML DU SHELL SE POSE TARD, ET UNE SEULE FOIS —
            // MESURÉ.** Le shell reprend la pose du sien pendant environ 900 ms
            // après l'ouverture : sur son menu « Réseau », deux poses de suite ont
            // été reprises de 1188 px, la troisième a tenu. Poser tôt ne fait donc
            // que faire CLIGNOTER le menu entre notre cible et la sienne —
            // constaté par l'utilisateur : « tantôt au bon endroit, tantôt en haut
            // de la barre des tâches ». On laisse donc passer ce délai avant de
            // poser, et on ne pose qu'une fois (choix explicite de l'utilisateur).
            //
            // Les autres familles — menu Win32 `#32768`, menu Qt d'une
            // application — tiennent du premier coup et ne paient AUCUN délai.
            let classe = class_name(menu);
            if classe == XAML_POPUP_CLASS {
                // **ON NE LE DÉPLACE PAS — MESURÉ, ET APRÈS AVOIR ESSAYÉ LE
                // CONTRAIRE.** Trois comportements ont été tentés sur cette
                // famille, et le troisième est le bon par élimination :
                //
                // 1. **Poser tout de suite** → le shell reprend la pose, quatre
                //    fois de suite ; le menu CLIGNOTE entre notre cible et la
                //    sienne, et selon l'instant où l'on s'arrête il finit au bon
                //    endroit ou non.
                // 2. **Poser tard, une seule fois** (le shell se calme après
                //    ~900 ms) → ça tient PARFOIS, mais l'utilisateur voit le menu
                //    « changer de place tout seul », et sur un second essai la
                //    cible a encore dérivé de 1061 px.
                // 3. **Ne pas y toucher.** Aucun clignotement, aucun saut ; le
                //    menu reste là où Windows l'a mis. C'est une LIMITE assumée,
                //    pas un défaut déguisé — et elle ne concerne QUE cette classe.
                //
                // ⚠️ **« LES MENUS QT TIENNENT » EST FAUX, MAIS ON LES POSE
                // QUAND MÊME — MESURÉ LE 2026-10-06.** Les familles qui tiennent
                // VRAIMENT la pose : le menu Win32 (`#32768`) et le menu WPF de
                // pCloud — G HUB l'a confirmé (« 1 pose, au bord »). Le popup **Qt**
                // de Battle.net, lui, ne l'atteint JAMAIS : visé à 3633, il finit à
                // 2386 après trois poses. **On continue pourtant de le poser**, et
                // c'est un choix mesuré, pas un reste : sans poses il s'ouvre à
                // `x = 56` (bord GAUCHE de l'écran, sous la barre de gauche) ; avec,
                // il arrive à mi-écran. Aucun des deux n'est « à côté du dock », mais
                // le second est VISIBLE.
                //
                // ⛔ **ET IL N'Y A PAS DE MEILLEURE PRISE — CHERCHÉ.** Le motif UIA
                // `TransformPattern`, par lequel Qt déplacerait SA PROPRE fenêtre,
                // est **ABSENT** de ce popup (mesuré : `transform=false
                // window=true`), exactement comme sur le popup du shell. Il ne reste
                // donc que `SetWindowPos`, que le propriétaire rattrape — même
                // conclusion que pour le shell, même cause : **le propriétaire garde
                // la main sur sa fenêtre.**
                crate::diag!(
                    "menu contextuel : {nom:?} — popup XAML du shell ({classe}) : le shell en reprend la pose, on ne le déplace pas (limite assumée)"
                );
                // **UNE SECONDE PISTE A ÉTÉ MESURÉE PUIS ÉCARTÉE, et c'est écrit
                // ici pour qu'aucune session future ne la rouvre** : le motif UIA
                // `TransformPattern` — par lequel le PROPRIÉTAIRE déplacerait sa
                // propre fenêtre, comme il affiche son propre menu — est
                // simplement **ABSENT** de ce popup (deux essais). Il ne reste
                // donc aucune prise : `SetWindowPos` est annulé par le shell, et
                // le motif qui aurait fait faire le travail au shell n'existe pas.
                return;
            }
            // **CE QU'ON VÉRIFIE EST LE BORD DROIT, PAS LA POSITION — MESURÉ.**
            //
            // Un menu WPF (`HwndWrapper[pCloud;…]`) est trouvé à sa taille par
            // DÉFAUT alors qu'il est encore hors écran (`x = -200`), puis
            // **s'agrandit en s'affichant** : visé à 200×307, il finit à
            // **300×461**. `SetWindowPos` ne règle QUE la position, donc son bord
            // droit partait 100 px SOUS la barre de droite — « en partie caché par
            // la barre ».
            //
            // Attendre que la taille se stabilise AVANT de viser ne suffit pas :
            // mesuré aussi, deux lectures identiques à 50 ms d'intervalle PUIS la
            // croissance. La seule règle qui tienne est donc de **reposer tant que
            // le bord droit n'est pas au bord de la barre**, en recalculant la
            // cible depuis la taille COURANTE.
            let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
            let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
            let mut rect = RECT::default();
            let mut apres = RECT::default();
            let mut posees = 0u32;
            for _ in 1..=POSE_ATTEMPTS {
                if unsafe { GetWindowRect(menu, &mut rect) }.is_err() {
                    crate::diag!("menu contextuel : {nom:?} — fenêtre DISPARUE avant la pose");
                    return;
                }
                let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
                if width <= 0 || height <= 0 {
                    // Pas encore dessinée : viser sa taille serait viser zéro.
                    std::thread::sleep(std::time::Duration::from_millis(POSE_SETTLE_MS));
                    continue;
                }
                // Déjà au bord de la barre ET centré sur l'icône ? Rien à faire.
                let au_bord = (rect.right - menu_right).abs() <= POSE_TOLERANCE_PX;
                let centre = (rect.top + height / 2 - menu_centre_y).abs() <= POSE_TOLERANCE_PX;
                if au_bord && centre {
                    break;
                }
                let x = (menu_right - width).clamp(0, (screen_width - width).max(0));
                let y = (menu_centre_y - height / 2).clamp(0, (screen_height - height).max(0));
                // `SWP_NOSIZE` : on ne change QUE la position. `SWP_NOACTIVATE` : le
                // menu a déjà pris le premier plan par lui-même, le réactiver serait
                // un vol de focus (voir [[neuradesk-live-input-safety]] pour la même
                // règle appliquée aux fenêtres du dock).
                let pose = unsafe {
                    SetWindowPos(menu, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE)
                };
                if let Err(refus) = &pose {
                    // On nomme le PROPRIÉTAIRE : c'est lui qui explique l'« Accès
                    // refusé » (UIPI, voir la doc de `show_context_menu`), pas notre
                    // cible.
                    let (proprietaire, eleve) = crate::win32::app_identity::window_owner(menu);
                    crate::diag!(
                        "menu contextuel : {nom:?} — SetWindowPos REFUSÉ ({refus:?}) — rect avant ({},{}) {width}x{height}, cible ({x},{y}) ; ce menu appartient à {proprietaire} (élevé : {eleve:?})",
                        rect.left, rect.top
                    );
                    return;
                }
                posees += 1;
                std::thread::sleep(std::time::Duration::from_millis(POSE_SETTLE_MS));
            }
            if unsafe { GetWindowRect(menu, &mut apres) }.is_err() {
                crate::diag!("menu contextuel : {nom:?} — fenêtre DISPARUE après la pose");
                return;
            }
            let (largeur_apres, hauteur_apres) = (apres.right - apres.left, apres.bottom - apres.top);
            let bord = apres.left + largeur_apres;
            // Le BORD DROIT est écrit en clair, face à celui de la barre : c'est ce
            // qui rend « trop à droite » lisible dans le journal, au lieu d'une
            // position et d'une largeur à recalculer mentalement.
            crate::diag!(
                "menu contextuel : {nom:?} trouvé en {} ms — classe {classe}, {posees} pose(s) : ({},{}) {largeur_apres}x{hauteur_apres}, bord droit {bord} (barre à {menu_right}){}",
                debut.elapsed().as_millis(),
                apres.left, apres.top,
                if (bord - menu_right).abs() <= POSE_TOLERANCE_PX {
                    " — au bord".to_string()
                } else {
                    format!(" — DÉBORDE de {} px", bord - menu_right)
                },
            );
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // RIEN DU TOUT. On liste donc tout ce qui est apparu depuis le clic : c'est
    // ce qui a montré qu'un menu d'application peut être une fenêtre Qt, et
    // qu'une icône du shell peut n'ouvrir AUCUNE fenêtre.
    let apparus: Vec<String> = visible_windows()
        .into_iter()
        .filter(|(hwnd, _)| !avant.contains(&(hwnd.0 as isize)))
        .map(|(hwnd, classe)| {
            let mut rect = RECT::default();
            let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
            let (proprietaire, _) = crate::win32::app_identity::window_owner(hwnd);
            format!(
                "{classe} ({},{}) {}x{} <- {proprietaire}",
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top
            )
        })
        .collect();
    crate::diag!(
        "menu contextuel : {nom:?} — AUCUNE fenêtre nouvelle en {} ms (avant={}) — fenêtres APPARUES depuis le clic : {apparus:?} ; cause connue : un menu laissé OUVERT empêche le shell d'en ouvrir un second",
        debut.elapsed().as_millis(),
        avant.len()
    );
}

/// **LE PANNEAU DE DÉBORDEMENT EST-IL OUVERT ?** — le témoin qui dit si un clic a
/// été consommé : activer une icône du panneau le referme.
///
/// ⚠️ **`IsWindowVisible` SUFFIT ICI, ET CE N'EST PAS CONTREDIT PAR LA LEÇON DE
/// `revealed_taskbar_rect`.** Là-bas, `IsWindowVisible` ne dit rien parce que la
/// barre GARÉE reste « visible » — sa POSITION est la seule qui compte. Ici on ne
/// cherche pas où est le panneau, mais s'il a été REFERMÉ : le shell le cache
/// vraiment, et `IsWindowVisible` bascule alors. Deux questions, deux sondes.
fn panneau_debordement_visible() -> bool {
    let Ok(fenetre) =
        (unsafe { FindWindowW(w!("TopLevelWindowForOverflowXamlIsland"), PCWSTR::null()) })
    else {
        return false;
    };
    unsafe { IsWindowVisible(fenetre) }.as_bool()
}

/// **LE TEMPS LAISSÉ AU PANNEAU DE DÉBORDEMENT POUR S'INSTALLER**, en
/// millisecondes — avant de relire la position de l'icône qu'il vient de rendre
/// visible. Même ordre de grandeur que `MENU_APPEARANCE_MS` (1 000 ms), et pour la
/// même raison : c'est un panneau XAML, il se compose après avoir été demandé.
/// 500 ms est un compromis assumé — plus court, on relirait un rectangle encore
/// vide ; plus long, le clic droit se ferait attendre.
const PANNEAU_OUVERTURE_MS: u64 = 500;

/// **LE TEMPS LAISSÉ AU CLIC POUR ÊTRE CONSOMMÉ AVANT DE REFERMER LE PANNEAU.**
/// Le refermer trop tôt annulerait le menu que l'application vient d'ouvrir.
const PANNEAU_REFERMETURE_MS: u64 = 250;

/// **NOTRE PROPRE DOCK RECOUVRE-T-IL LA BANDE ?** — et si oui, la capture ne vaut
/// RIEN.
///
/// ⛔⛔ **LE DÉFAUT, MESURÉ LE 2026-10-06.** `revealed_taskbar_rect` rend bien le
/// rectangle de la barre des tâches de Windows. Mais **notre dock occupe le MÊME
/// bas d'écran, et il est au-dessus** : la capture photographie donc **notre propre
/// dock**. Le journal l'a montré noir sur blanc — la bande écrite dans
/// `bande-barre.png` contenait un cadenas, une étoile, une caméra, des dossiers,
/// une imprimante et le `+` de la barre du bas du dock, sur le fond violet du
/// thème. **Aucune icône de Windows.**
///
/// Conséquence : chaque « icône » découpée dans cette bande était un morceau de
/// notre propre fond — **un carré** — et écrasait le bon glyphe que le dock venait
/// de calculer (`glyphe du shell:U+E701 64x64` dans le même journal).
///
/// ⚠️ **ON DEMANDE AU SYSTÈME QUI EST PEINT, AU LIEU DE CALCULER UNE GÉOMÉTRIE** :
/// c'est la question posée, sans supposer quoi que ce soit de la disposition — ni
/// de la taille de la barre du bas, ni de l'écran où elle vit.
pub fn bande_couverte_par_notre_dock(bande: RECT) -> bool {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::WindowFromPoint;

    let centre = POINT { x: (bande.left + bande.right) / 2, y: (bande.top + bande.bottom) / 2 };
    let dessus = unsafe { WindowFromPoint(centre) };
    if dessus.0.is_null() {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(dessus, Some(&mut pid)) };
    pid == std::process::id()
}

/// **OUVRE LE PANNEAU DE DÉBORDEMENT — ET LE FAIT FAIRE PAR LE SHELL.**
///
/// ⛔ **POURQUOI C'EST NÉCESSAIRE** : une icône de la zone MASQUÉE n'a **aucune
/// position à l'écran** tant que le panneau est fermé — son rectangle vaut `0x0`
/// (mesuré, 2026-10-06). Or un vrai clic a besoin d'une position. Sans ouvrir le
/// panneau, les icônes du `^` sont **inatteignables par un clic**, quoi qu'on
/// fasse.
///
/// ⭐ **ON ENVOIE `Invoke` AU BOUTON DU SHELL, PAS UN `ShowWindow`.** La
/// littérature est unanime sur ce point (voir la doc de `clic_reel`) :
/// « ShowWindow » rend la fenêtre visible **sans que l'OS sache qu'il l'a
/// affichée** — le panneau ne se dimensionne plus correctement et **cesse de
/// s'auto-masquer** jusqu'à ce qu'un clic manuel sur le chevron resynchronise
/// l'état. Le bouton « Afficher les icônes cachées » est un élément UI Automation
/// de `Shell_TrayWnd`, il porte `InvokePattern` (mesuré : `invoke=OUI`, action par
/// défaut « Appuyer »), et **c'est donc le shell qui ouvre son propre panneau** —
/// il sait ce qu'il fait, et son état reste cohérent.
fn ouvrir_le_panneau_masque() -> Result<(), String> {
    use windows::Win32::UI::Accessibility::{IUIAutomationInvokePattern, UIA_InvokePatternId};
    use windows_core::Interface;

    // Le nom vient de `EXCLUDED_NAME_PREFIXES` : c'est celui que le shell donne à
    // son bouton, et il est FRANÇAIS sur cette machine. Un préfixe, pas une
    // égalité : le nom porte la suite de l'infobulle.
    let nom_bouton = EXCLUDED_NAME_PREFIXES[0];

    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
    let automation: IUIAutomation = unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &CUIAutomation,
            None,
            windows::Win32::System::Com::CLSCTX_ALL,
        )
    }
    .map_err(|error| format!("UI Automation indisponible : {error:?}"))?;
    let bridge =
        find_bridge("Shell_TrayWnd").ok_or("passerelle de composition introuvable dans la barre")?;
    let element = unsafe { automation.ElementFromHandle(bridge) }
        .map_err(|error| format!("ElementFromHandle : {error:?}"))?;
    let condition = unsafe { automation.CreateTrueCondition() }
        .map_err(|error| format!("CreateTrueCondition : {error:?}"))?;
    let children = unsafe { element.FindAll(TreeScope_Descendants, &condition) }
        .map_err(|error| format!("FindAll : {error:?}"))?;
    let mut vu: Vec<String> = Vec::new();
    for index in 0..unsafe { children.Length() }.unwrap_or(0) {
        let Ok(child) = (unsafe { children.GetElement(index) }) else { continue };
        if !current_class(&child).starts_with(TRAY_CLASS_PREFIX) {
            continue;
        }
        let nom = unsafe { child.CurrentName() }.map(|n| n.to_string()).unwrap_or_default();
        if !nom.starts_with(nom_bouton) {
            vu.push(nom.lines().next().unwrap_or("").trim().to_string());
            continue;
        }
        let motif = unsafe { child.GetCurrentPattern(UIA_InvokePatternId) }
            .map_err(|error| format!("le bouton {nom_bouton:?} n'a pas de motif Invoke : {error:?}"))?;
        let invoke = motif
            .cast::<IUIAutomationInvokePattern>()
            .map_err(|error| format!("cast IUIAutomationInvokePattern : {error:?}"))?;
        let appel = unsafe { invoke.Invoke() };
        crate::diag!("panneau masqué : Invoke sur {nom_bouton:?} a rendu {appel:?}");
        return appel.map_err(|error| format!("Invoke du bouton : {error:?}"));
    }
    Err(format!(
        "bouton {nom_bouton:?} INTROUVABLE dans la barre ; SystemTray.* vus : {:?}",
        &vu[..vu.len().min(6)]
    ))
}

/// **UN VRAI CLIC DROIT SUR L'ICÔNE RÉELLE DE LA BARRE DES TÂCHES.**
///
/// DEMANDE EXPLICITE (2026-10-06), après que la voie UI Automation a été mesurée
/// insuffisante sur Battle.net : son menu est un popup Qt qui se pose au bord
/// GAUCHE de l'écran, n'expose **aucun motif de déplacement** (`transform=false`,
/// mesuré) et ne tient pas la pose qu'on lui impose. Avec un vrai clic,
/// l'application reçoit un clic **à une position** et place son menu **au
/// curseur** — comme quand l'utilisateur clique sur la vraie icône. Ça marche pour
/// toutes les applications, sans exception.
///
/// ## ⚠️ `SendInput` ICI, ET POURQUOI C'EST LÉGITIME
///
/// **CE N'EST PAS DE LA SIMULATION D'ENTRÉE AU SENS DE CE DÉPÔT.** La règle
/// (« on ne simule jamais une entrée sur le bureau réel ») vise les **OUTILS DE
/// VÉRIFICATION**, qui agiraient dans le dos de l'utilisateur. Ici c'est un
/// **GESTE DE L'UTILISATEUR** — il fait un clic droit sur l'icône, le clic part —
/// exactement comme `caracteres::frapper` pour une lettre, ou `win32::desktops`
/// pour Win+Ctrl+Flèche. Voir la doc de `caracteres`, qui porte cette distinction.
///
/// ⛔ **LE COMMENTAIRE DE `show_context_menu` DISAIT LE CONTRAIRE** (« ce que ce
/// dépôt s'interdit ») : il appliquait à un geste d'utilisateur une règle écrite
/// pour les sondes. Corrigé le 2026-10-06.
///
/// ## ⚠️ CE QUE ÇA COÛTE, ET QUI A ÉTÉ ACCEPTÉ EN CONNAISSANCE DE CAUSE
///
/// **Le curseur de l'utilisateur quitte sa place** le temps de l'injection, puis
/// **y revient** — position relevée avant, restaurée après. C'est le prix, il a
/// été exposé avant d'être payé, et l'utilisateur a choisi cette voie.
///
/// ⚠️ **ET LE MENU S'OUVRE PRÈS DE LA ZONE DE NOTIFICATION**, pas à côté du dock :
/// c'est le prix de la même décision. L'ancienne voie (`ShowContextMenu` + pose)
/// tentait de l'amener contre la barre — ce qui marchait pour les menus Win32 et
/// WPF, et jamais pour le popup Qt.
///
/// Rend la **RAISON** de l'échec, pour que l'appelant puisse retomber sur l'ancien
/// chemin plutôt que de ne rien faire.
fn clic_reel(icon: &TrayIcon) -> Result<(), String> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE,
        MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, SM_CXVIRTUALSCREEN,
        SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    // ⚠️ **LA POSITION EST RELEVÉE À L'INSTANT**, par la même recherche UIA que le
    // clic droit historique : le rectangle de `TrayIcon` peut dater de la dernière
    // énumération, et la barre des tâches oscille entre deux états qui déplacent
    // toutes ses icônes (mesuré : 70 px). Viser un rectangle périmé, c'est cliquer
    // à côté — sur le bureau, ou sur l'icône VOISINE.
    // ⛔⛔ **UNE ICÔNE MASQUÉE N'A NI PASSERELLE NI POSITION TANT QUE LE PANNEAU
    // EST FERMÉ — DONC ON L'OUVRE AVANT LA PREMIÈRE LECTURE.**
    //
    // Défaut RÉEL, mesuré le 2026-10-08 : `element_de_l_icone` cherche la
    // passerelle de `TopLevelWindowForOverflowXamlIsland`, qui **n'existe pas**
    // panneau fermé (voir `OVERFLOW_ROOT_CLASS`). Elle échouait donc **à sa
    // première ligne**, et le `?` sortait de cette fonction **avant** d'atteindre
    // le bloc « rectangle vide → ouvrir le panneau » plus bas : ce bloc était
    // INATTEIGNABLE pour une icône masquée, et le clic droit sur une icône du `^`
    // ne pouvait pas fonctionner.
    // Windows 10 : le panneau qu'on ouvrirait est XAML, il n'existe pas — et une
    // masquée n'a pas de position débordement fermé (`0x0`, mesuré). L'appelant
    // retombe sur `ShowContextMenu`, qui n'en demande pas.
    if icon.zone == TrayZone::Hidden && crate::win32::os_version::os_windows_10() {
        return Err("Windows 10 : icône masquée, sans position tant que le débordement est fermé".to_string());
    }
    let mut ouvert_par_nous = false;
    if icon.zone == TrayZone::Hidden && !panneau_debordement_visible() {
        ouvrir_le_panneau_masque()?;
        ouvert_par_nous = true;
        // Le panneau XAML se compose après avoir été demandé — voir
        // `PANNEAU_OUVERTURE_MS`.
        std::thread::sleep(std::time::Duration::from_millis(PANNEAU_OUVERTURE_MS));
    }
    let mut rect = element_de_l_icone(icon)?.2;
    // ⛔⛔ **UN RECTANGLE VIDE N'EST PAS UNE POSITION — ET CLIQUER QUAND MÊME
    // ENVERRAIT LE CLIC À (0,0), SUR LE BUREAU DE L'UTILISATEUR.** Défaut RÉEL,
    // mesuré le 2026-10-06 dès le premier essai : le journal a écrit
    // « envoyé au centre (0,0) … (rect 0x0) » pour Battle.net.
    //
    // **CE QU'ON NE SAVAIT PAS** : une icône de la zone MASQUÉE (le panneau de
    // débordement, fermé) rend un rectangle **0x0** — elle n'a pas de position à
    // l'écran tant que le panneau n'est pas ouvert. `ShowContextMenu` s'en
    // accommodait parce qu'il ne demande AUCUNE position ; un vrai clic, si.
    //
    // ⭐ **ET C'EST RÉPARABLE, PAS SEULEMENT REFUSABLE** : on OUVRE le panneau
    // (voir `ouvrir_le_panneau_masque`, qui le fait faire au shell), on laisse le
    // shell l'installer, et on RELIT la position. C'est la seule façon d'atteindre
    // une icône du `^` par un clic — et c'est ce qui rend Battle.net possible.
    let vide = |r: RECT| r.right - r.left <= 0 || r.bottom - r.top <= 0;
    if vide(rect) {
        crate::diag!(
            "clic droit réel : pas de position pour {:?} — ouverture du panneau de débordement",
            icon.name.lines().next().unwrap_or("")
        );
        // ⛔ **NE JAMAIS TOGGLER UN PANNEAU DÉJÀ OUVERT** : le bouton du shell est
        // une BASCULE, donc un `Invoke` de trop **referme** — soit celui qu'on
        // vient d'ouvrir plus haut (et la relecture qui suit rend un rectangle
        // vide de plus), soit, pire, celui que l'UTILISATEUR avait ouvert
        // lui-même.
        if !ouvert_par_nous && !panneau_debordement_visible() {
            ouvrir_le_panneau_masque()?;
            ouvert_par_nous = true;
        }
        // Le panneau XAML s'installe en quelques centaines de millisecondes — le
        // même ordre de grandeur que `MENU_APPEARANCE_MS`, et pour la même raison.
        std::thread::sleep(std::time::Duration::from_millis(PANNEAU_OUVERTURE_MS));
        rect = element_de_l_icone(icon)?.2;
        if vide(rect) {
            return Err(format!(
                "rectangle vide ({0}x{1}) MÊME PANNEAU OUVERT — l'icône n'a toujours pas de position",
                rect.right - rect.left,
                rect.bottom - rect.top
            ));
        }
    }
    let (cx, cy) = ((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2);
    // ⛔ **ET LE CENTRE DOIT ÊTRE DANS L'ÉCRAN — DEUXIÈME GARDE, ET ELLE A SERVI
    // DÈS LE PREMIER ESSAI.** Mesuré le 2026-10-06 : pour « MEGAsync », le
    // rectangle rendu était `(3219,2158)-(3267,2230)` alors que l'écran fait
    // **2160** de haut — centre à `y = 2194`, **sous l'écran**. Le `SendInput` a
    // donc cliqué dans le vide.
    //
    // **LA CAUSE EST DOCUMENTÉE DEPUIS LONGTEMPS** : la barre des tâches oscille
    // entre deux états — **révélée** et **garée hors écran** — qui déplacent toutes
    // ses icônes de la même quantité (mesuré : 70 px en Y). UIA rend la position de
    // l'état COURANT, et quand la barre est masquée, cette position n'est pas
    // cliquable. On le DIT au lieu de cliquer au hasard.
    let (vx, vy) = unsafe {
        (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN))
    };
    let (vw, vh) = unsafe {
        (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN))
    };
    if cx < vx || cy < vy || cx >= vx + vw || cy >= vy + vh {
        return Err(format!(
            "centre ({cx},{cy}) HORS de l'écran {vw}x{vh} — la barre des tâches est garée hors écran, ses icônes n'ont pas de position cliquable"
        ));
    }

    // Le curseur, gardé pour être remis exactement où il était.
    let mut avant = POINT::default();
    let connu = unsafe { GetCursorPos(&mut avant) }.is_ok();

    // **COORDONNÉES ABSOLUES = VIRTUELLES**, sur 0..65535 : `SM_CXSCREEN` seul
    // serait faux dès qu'un écran secondaire est à gauche ou au-dessus du primaire.
    // (Les bornes virtuelles sont relevées PLUS HAUT, avec la garde du centre.)
    if vw <= 1 || vh <= 1 {
        return Err(format!("écran virtuel absurde : {vw}x{vh}"));
    }
    let absolu = |x: i32, y: i32| {
        let ax = ((x - vx) as i64 * 65535 / (vw as i64 - 1)) as i32;
        let ay = ((y - vy) as i64 * 65535 / (vh as i64 - 1)) as i32;
        (ax.clamp(0, 65535) as i32, ay.clamp(0, 65535) as i32)
    };

    let souris = |drapeaux, dx, dy| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy, mouseData: 0, dwFlags: drapeaux, time: 0, dwExtraInfo: 0 },
        },
    };

    // ⛔ **LE DÉPLACEMENT ET LE CLIC SONT SÉPARÉS, ET C'EST UNE RUPTURE DÉLIBÉRÉE
    // AVEC `caracteres::frapper`** — qui, lui, envoie tout d'un bloc pour qu'aucune
    // entrée ne s'intercale entre l'appui et le relâchement d'une TOUCHE.
    //
    // Ici le problème est l'inverse : **un bouton XAML ne s'arme qu'après avoir vu
    // le pointeur ENTRER.** Teleporter le curseur et presser dans le même souffle
    // laisse le panneau traiter les deux sans jamais marquer le bouton « chaud » —
    // et le clic n'est pas consommé. Mesuré : le clic partait à `(3040,1793)` sur un
    // rectangle `60x60`, `SendInput` insérait tout, **et aucun menu ne s'ouvrait.**
    //
    // Ce qui compte pour une frappe est l'ATOMICITÉ ; ce qui compte pour un clic
    // sur une interface XAML est de laisser le survol être TRAITÉ. Deux exigences
    // opposées, deux gestes différents.
    let (ax, ay) = absolu(cx, cy);
    // ⛔⛔ **LE PANNEAU DE DÉBORDEMENT N'ACCEPTE PAS UN CLIC SYNTHÉTIQUE — MESURÉ,
    // ET TOUT LE RESTE A ÉTÉ ÉCARTÉ.** Sept mesures, toutes conduites le
    // 2026-10-06, et toutes concluantes SAUF la dernière :
    //
    //   1. **la région visée contient bien l'icône** — photographiée et regardée
    //      (`cible.png` : le logo bleu de Battle.net, centré dans les 60x60) ;
    //   2. **le point appartient au panneau lui-même** — `WindowFromPoint` rend
    //      `TopLevelWindowForOverflowXamlIsland`, propriétaire `explorer.exe`,
    //      **non élevé** ;
    //   3. **le panneau était DÉJÀ au premier plan** avant qu'on l'active ;
    //   4. **le survol** — 150 ms entre le déplacement et le clic : sans effet ;
    //   5. **l'activation** — `SetForegroundWindow` sur le panneau : sans effet,
    //      et le journal a montré qu'elle était déjà acquise ;
    //   6. **l'espace de coordonnées** — `MOUSEEVENTF_VIRTUALDESK` (qui manquait, et
    //      qui reste : c'est un vrai correctif pour un écran secondaire à origine
    //      négative) : sans effet sur ce défaut ;
    //   7. **le bouton et la livraison** — clic droit, `SendInput` insère TOUS ses
    //      événements.
    //
    // **ET LE CLIC N'EST PAS CONSOMMÉ.** Le témoin est sans ambiguïté : activer une
    // icône du panneau le REFERME, et il reste ouvert.
    //
    // ⇒ **C'EST UNE IMPASSE DU CÔTÉ DE WINDOWS, PAS DE CE CODE.** L'île XAML du
    // panneau de débordement ignore l'entrée synthétique, quelle que soit sa
    // provenance. La littérature communautaire le dit à sa manière — les projets
    // qui y arrivent s'injectent dans `explorer.exe`, ce que ce dépôt refuse.
    //
    // Les tentatives 4 et 5 ont été RETIRÉES : elles n'ont rien changé et
    // ajoutaient 270 ms d'attente. Un mécanisme qui ne sert pas ne reste pas.
    let deplacement = [souris(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, ax, ay)];
    // **TOUT PART EN UN SEUL `SendInput`** — meme regle que `caracteres::frapper` :
    // des envois separes laisseraient une autre entree s'intercaler entre le
    // deplacement et le clic, et le clic partirait ailleurs.
    let mut entrees = vec![
        deplacement[0],
        souris(MOUSEEVENTF_RIGHTDOWN, 0, 0),
        souris(MOUSEEVENTF_RIGHTUP, 0, 0),
    ];
    if connu {
        let (rx, ry) = absolu(avant.x, avant.y);
        entrees.push(souris(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, rx, ry));
    }
    let envoyes = unsafe { SendInput(&entrees, std::mem::size_of::<INPUT>() as i32) };
    if envoyes as usize != entrees.len() {
        return Err(format!("SendInput a insere {envoyes} evenement(s) sur {}", entrees.len()));
    }
    // ⭐ **LE SEUL TÉMOIN QUE LE CLIC A ÉTÉ CONSOMMÉ — MESURÉ, PAS SUPPOSÉ.** Activer
    // une icône du panneau le REFERME : si le panneau est encore ouvert après le
    // clic, c'est que le bouton a absorbé le geste sans rien faire. Sans ce témoin,
    // « pas de menu » ne distingue pas « le clic n'est pas parti » de « il est parti
    // et n'a rien activé » — les deux se lisent pareil, et c'est ce qui a coûté
    // plusieurs manches.
    let encore = panneau_debordement_visible();
    crate::diag!(
        "clic droit réel : après le clic, panneau de débordement {}",
        if encore {
            "ENCORE OUVERT — le clic n'a PAS activé l'icône"
        } else {
            "refermé — le clic a été CONSOMMÉ"
        }
    );
    if connu {
        let (rx, ry) = absolu(avant.x, avant.y);
        let retour = [souris(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, rx, ry)];
        let _ = unsafe { SendInput(&retour, std::mem::size_of::<INPUT>() as i32) };
    }
    // ⛔⛔ **« ENVOYÉ » N'EST PAS « CONSOMMÉ » — ET LE REPLI DÉPEND DE CETTE
    // DISTINCTION.** Défaut RÉEL de ce code, vu par l'utilisateur le 2026-10-06 :
    // « je ne vois toujours pas de menu au clic droit ».
    //
    // `SendInput` insère TOUS ses événements → cette fonction rendait `Ok` **même
    // quand le panneau n'avait rien fait**. L'appelant croyait donc avoir ouvert un
    // menu, et **le repli `ShowContextMenu` ne se déclenchait JAMAIS** — alors que
    // c'est lui qui, pour une icône masquée, ouvre bel et bien le menu (mal placé,
    // mais ouvert).
    //
    // **LE TÉMOIN DÉCIDE DONC, AU LIEU DE SEULEMENT JOURNALISER** : si on a ouvert
    // le panneau et qu'il est ENCORE LÀ après le clic, le clic n'a pas été consommé.
    // On le dit, et l'appelant retombe sur la voie qui marche.
    // ⛔⛔ **ON REFERME LE PANNEAU QU'ON A OUVERT — SINON LE DOCK SE VIDE.**
    //
    // Défaut RÉEL, vu par l'utilisateur le 2026-10-06 : « je ne vois plus du tout
    // les icônes de Windows ». Le journal disait tout :
    //
    //     [tray_icons] passerelle de composition introuvable sous Shell_TrayWnd
    //     zone de notification : panneau ouvert — rectangles des 38 masquée(s) repris
    //
    // **PANNEAU OUVERT = PLUS DE PASSERELLE SOUS `Shell_TrayWnd`** : l'énumération
    // ne rend alors que les icônes MASQUÉES, et **zéro icône visible** — Volume,
    // Réseau, Horloge disparaissent du dock. Ce n'est pas une conséquence de la
    // capture, c'est l'énumération elle-même qui échoue.
    //
    // ⚠️ **LE CLIC SUR L'ICÔNE NE REFERME PAS LE PANNEAU QUAND IL NE « PREND »
    // PAS** — et c'est justement le cas de Battle.net, dont le menu ne s'ouvre
    // pas. Le panneau restait donc ouvert indéfiniment.
    //
    // On le referme donc NOUS-MÊMES, par le même `Invoke` (le bouton bascule), et
    // seulement si c'est NOUS qui l'avons ouvert — un panneau que l'utilisateur
    // avait déjà ouvert ne doit pas se fermer sous ses doigts.
    if ouvert_par_nous {
        // Le temps que le clic soit consommé : refermer trop tôt annulerait le
        // menu que l'application vient d'ouvrir.
        std::thread::sleep(std::time::Duration::from_millis(PANNEAU_REFERMETURE_MS));
        match ouvrir_le_panneau_masque() {
            Ok(()) => crate::diag!("clic droit réel : panneau de débordement refermé"),
            Err(raison) => {
                crate::diag!("clic droit réel : panneau NON refermé ({raison}) — le dock peut perdre ses icônes visibles")
            }
        }
    }
    if ouvert_par_nous && encore {
        return Err(
            "le panneau de débordement n'a pas consommé le clic — l'île XAML ignore l'entrée synthétique"
                .to_string(),
        );
    }
    crate::diag!(
        "clic droit réel : envoyé au centre ({cx},{cy}) de l'icône {:?} (rect {}x{}), curseur {}",
        icon.name.lines().next().unwrap_or(""),
        rect.right - rect.left,
        rect.bottom - rect.top,
        if connu { "remis en place" } else { "NON remis (position illisible)" }
    );
    Ok(())
}

/// **OUVRE LE MENU D'UNE ICÔNE DE NOTIFICATION** — le point d'entrée du clic droit
/// du dock.
///
/// ⭐ **UN VRAI CLIC D'ABORD, `ShowContextMenu` EN REPLI.** Le vrai clic marche
/// partout (l'application place son menu au curseur) ; le repli garde le chemin
/// historique pour le cas où la position serait introuvable — et il n'est PAS du
/// code mort, c'est la raison pour laquelle `place_new_menu` reste vivant.
///
/// ⚠️ **SUR UN FIL** : la recherche UIA coûte ~60 ms, et `ShowContextMenu` a été
/// chronométré jusqu'à **30,9 s** (voir sa doc). Rien de tout cela ne doit passer
/// sur le fil d'interface, qui gèle le dock.
pub fn ouvrir_menu_de_l_icone(icon: &TrayIcon, menu_right: i32, menu_centre_y: i32) {
    let icone = icon.clone();
    std::thread::spawn(move || {
        if let Err(raison) = clic_reel(&icone) {
            crate::diag!("clic droit : clic réel impossible ({raison}) — repli sur ShowContextMenu");
            show_context_menu(&icone, menu_right, menu_centre_y);
        }
    });
}

/// **OUVRE LE MENU CONTEXTUEL PROPRE À UNE ENTRÉE DE NOTIFICATION** — le VRAI
/// menu de l'application, avec ses VRAIES actions.
///
/// DEMANDE EXPLICITE : « un clic droit sur une icône de la zone système de
/// droite permette d'ouvrir un menu identique à celui qu'on obtient avec le clic
/// droit sur les icônes dans la zone de notifications et de lancer les actions
/// proposées ».
///
/// **POURQUOI `ShowContextMenu` EXISTE ENCORE.** La littérature est explicite sur
/// la barre XAML de Windows 11 : `SendMessage(WM_CONTEXTMENU)` **échoue** sur ces
/// contrôles. `SendInput` marcherait, mais il exige une fenêtre **visible et
/// focalisée** — donc déplacer le curseur jusque sur la barre des tâches.
///
/// ⛔ **CE COMMENTAIRE A LONGTEMPS DIT « ce que ce dépôt s'interdit », ET C'ÉTAIT
/// FAUX — CORRIGÉ LE 2026-10-06.** La règle qu'il citait
/// ([[neuradesk-live-input-safety]]) vise les **OUTILS DE VÉRIFICATION**, qui
/// agiraient dans le dos de l'utilisateur ; elle ne vise pas un **GESTE DE
/// L'UTILISATEUR**. Le dépôt envoie déjà des frappes par `SendInput` pour cette
/// exacte raison (voir `caracteres`, dont la doc porte la distinction). **Un clic
/// droit de l'utilisateur sur une icône est un geste de l'utilisateur** — d'où
/// `clic_reel`, qui est désormais la voie normale, celle-ci n'étant plus que le
/// repli quand la position de l'icône est introuvable.
/// `IUIAutomationElement3::ShowContextMenu()` est **la voie prévue pour ça** :
/// c'est l'application elle-même qui affiche son menu, donc rien n'est simulé et
/// **aucune action n'est reproduite** — ce que l'utilisateur choisit dans le menu
/// est exécuté par son propriétaire, exactement comme depuis la barre.
///
/// ⚠️ **LE MENU S'OUVRE LÀ OÙ EST L'ICÔNE** — au bas de l'écran, sur la barre des
/// tâches, et non sous le curseur du dock : c'est le comportement de l'API, et il
/// est cohérent avec la demande (« un menu identique à celui du clic droit sur la
/// barre des tâches »).
///
/// ⚠️ **L'APPEL BLOQUE LE FIL QUI LE FAIT — MESURÉ, ET C'EST TOUT L'OBJET DE
/// CETTE FONCTION.** `ShowContextMenu` a été chronométré de **15 ms à
/// 30 917 ms** (30,9 s !), le cas long survenant quand un menu est **déjà
/// ouvert** : le shell commence par terminer le sien. Faire cet appel sur le fil
/// d'interface **gelait le dock** — il ne se repeignait plus, et les clics de
/// l'utilisateur s'empilaient dans la file de messages pour être rejoués d'un
/// bloc au déblocage (mesuré : huit clics en une seconde, que le journal date
/// tous à la milliseconde où le fil s'est libéré). D'où : **tout ce travail
/// part sur des fils**, et cette fonction rend la main immédiatement.
///
/// **DEUX FILS, ET LEUR ORDRE EST LE FOND DU CORRECTIF** : celui de POSE est
/// lancé **en premier**, pour qu'il surveille déjà quand l'autre ouvre le menu.
/// Un seul fil qui appellerait PUIS surveillerait ne verrait rien dans le cas
/// long — le menu, s'il apparaît, apparaît pendant que l'appel est encore
/// bloqué, et il a disparu quand l'appel rend enfin la main.
///
/// ⚠️ **DEUX LIMITES MESURÉES — et ce ne sont PAS des défauts de ce code.**
///
/// 1. **Le menu d'une icône du SHELL n'obéit pas.** Deux mesures : sur
///    « Clavier tactile », `ShowContextMenu` rend `Ok(())` en 231 ms et **rien
///    n'apparaît** — le diff de tout l'arbre ne montre que l'île XAML de la
///    barre des tâches et l'infobulle du dock. Sur « Réseau », il apparaît bien
///    (classe `Xaml_WindowedPopupClass`) mais **son propriétaire en reprend la
///    pose**. On ne le déplace donc pas du tout — trois comportements essayés,
///    voir `place_new_menu` : le menu reste là où Windows le met.
///
/// 1 bis. **Un menu DÉJÀ OUVERT empêche le suivant.** Mesuré sur pCloud : un
///    premier menu ouvert, puis deux clics sur la même icône — **aucune fenêtre
///    n'apparaît**, deux fois de suite. Le shell n'ouvre pas un second menu, et
///    aucun code de ce module n'y peut rien : c'est le clic qui ne produit rien,
///    pas la pose qui échoue.
///
/// 1 ter. ⚠️ **LA VOIE « NOTRE PROPRE MENU » A ÉTÉ EXPLORÉE PUIS ABANDONNÉE** —
///    mesurée de bout en bout, et **à ne pas rouvrir sans une fermeture qui ne
///    casse rien**. L'idée : ouvrir le popup du shell, lire ses entrées par UI
///    Automation, le fermer, afficher le nôtre, et appeler `InvokePattern` sur
///    l'entrée choisie pour que le PROPRIÉTAIRE exécute la vraie action. Trois
///    mesures, trois verdicts :
///    - les entrées **SONT énumérables et INVOCABLES** (menu « Réseau » :
///      8 descendants dont 3 invocables — « Diagnostiquer les problèmes
///      réseau », « Effectuer un test de vitesse », « Paramètres réseau et
///      Internet ») ;
///    - leur **contenu arrive APRÈS la fenêtre** : une lecture dès la détection
///      rend 3 descendants et 0 invocable, la même fenêtre en rend 8 quelques
///      centaines de millisecondes plus tard — lire une fois ne suffit pas ;
///    - **`WM_CLOSE` referme bien le popup, mais casse ensuite NOTRE chemin vers
///      cette icône** : le shell ne rouvre plus son menu pour elle,
///      `ShowContextMenu` rendant `Ok(())` sans rien créer (mesuré trois fois).
///      La VRAIE barre des tâches, elle, continue de fonctionner : le shell n'est
///      pas abîmé, c'est notre demande qui est refusée.
///    Le remède coûtait donc PLUS CHER que le défaut : une icône dont le menu ne
///    s'ouvre plus depuis NeuraDesk, contre un menu posé ailleurs.
///
/// 1 quater. **La seule voie qui reste vit DANS `explorer.exe`.** Les projets
///    C++ qui repositionnent réellement ces menus volants s'injectent dans le
///    shell — Windhawk (son mod « Vertical Taskbar » croche `SetWindowPos` et
///    `TrayUI_GetDockedRect`) et ExplorerPatcher (patch de `TrackPopupMenu`)
///    font exactement ça. Le crochet voit alors la pose **au moment où elle est
///    décidée**, ce qui est précisément ce qui manque depuis l'extérieur :
///    `SetWindowPos` en C++ échouerait ici comme en Rust. Ce dépôt refuse
///    l'injection — DLL épinglée définitivement dans explorer, plantage possible
///    du shell, et un antivirus qui bloque déjà `Add-Type` sur cette machine ne
///    l'accueillerait pas.
/// 2. **Une application ÉLEVÉE est hors d'atteinte.** Mesuré sur
///    « MSI Afterburner » : son menu existe bien, mais `SetWindowPos` rend
///    `0x80070005 Accès refusé`, et le journal nomme le propriétaire —
///    `MSIAfterburner.exe (élevé : true)`. C'est l'**UIPI** : un processus
///    d'intégrité moyenne ne peut pas déplacer la fenêtre d'un processus plus
///    privilégié. Le menu s'affiche alors là où son propriétaire l'a voulu, et
///    **aucune correction de ce côté ne peut y changer quoi que ce soit**.
///
/// Les handles de menu traversent en `isize` et non en `HWND` : `HWND` enveloppe
/// un `*mut c_void` et **n'est pas `Send`**.
pub fn show_context_menu(icon: &TrayIcon, menu_right: i32, menu_centre_y: i32) {
    // Le monde AVANT le clic, pour ne déplacer que ce qui apparaîtra — relevé
    // ICI, sur le fil appelant : c'est le seul instant où l'on est assuré
    // qu'aucun menu nouveau n'existe encore.
    let avant: Vec<isize> = visible_windows().iter().map(|(hwnd, _)| hwnd.0 as isize).collect();
    // Le nom voyage AVEC la pose : sans lui, le journal dit qu'un menu a été
    // posé, jamais DE QUELLE ICÔNE — et rien ne peut être corrélé.
    let nom_pose = icon.name.lines().next().unwrap_or("").to_string();
    let icone = icon.clone();

    std::thread::spawn(move || place_new_menu(&avant, &nom_pose, menu_right, menu_centre_y));

    std::thread::spawn(move || {
        if let Err(raison) = open_menu_of(&icone) {
            crate::diag!(
                "menu contextuel NON ouvert pour {:?} : {raison}",
                icone.name.lines().next().unwrap_or("")
            );
        }
    });
}

/// **L'ÉLÉMENT UI AUTOMATION D'UNE ICÔNE DE NOTIFICATION** — le travail commun au
/// clic DROIT (`open_menu_of`, `ShowContextMenu`) et au clic GAUCHE
/// (`invoke_of`, `InvokePattern`) : retrouver, dans la zone de l'icône, l'élément
/// qui porte son nom **et** sa position.
///
/// ⚠️ **LA ZONE EST RÉ-ÉNUMÉRÉE À CHAQUE APPEL**, et l'élément choisi par
/// `nearest_named` : ni le nom seul (deux entrées peuvent le partager) ni le
/// rectangle seul (la barre oscille entre deux états) ne suffisent — voir sa doc,
/// qui porte la mesure.
///
/// Rend **la RAISON** de l'échec avec l'élément, jamais un booléen : les trois
/// chemins muets d'autrefois se lisaient tous de la même façon dans le journal,
/// ce qui a coûté une manche de mesure (voir `open_menu_of`).
fn element_de_l_icone(icon: &TrayIcon) -> Result<(IUIAutomationElement, i32, RECT), String> {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
    let automation: IUIAutomation = unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &CUIAutomation,
            None,
            windows::Win32::System::Com::CLSCTX_ALL,
        )
    }
    .map_err(|error| format!("UI Automation indisponible : {error:?}"))?;
    let root_class = match icon.zone {
        TrayZone::Visible => "Shell_TrayWnd",
        TrayZone::Hidden => OVERFLOW_ROOT_CLASS,
    };
    let (bridge, classique) = racine_de_zone(root_class, icon.zone)
        .ok_or("racine de la zone introuvable (passerelle, ou barre d'outils sous Windows 10)")?;
    let element = unsafe { automation.ElementFromHandle(bridge) }
        .map_err(|error| format!("ElementFromHandle : {error:?}"))?;
    let condition = unsafe { automation.CreateTrueCondition() }
        .map_err(|error| format!("CreateTrueCondition : {error:?}"))?;
    let children = unsafe { element.FindAll(TreeScope_Descendants, &condition) }
        .map_err(|error| format!("FindAll : {error:?}"))?;
    // On relève d'abord TOUS les éléments de la zone, puis on choisit — c'est
    // `nearest_named` qui tranche (pure, donc testée).
    let mut observed: Vec<(String, RECT)> = Vec::new();
    // `i32` : c'est le type que `IUIAutomationElementArray::GetElement` attend.
    let mut indices: Vec<i32> = Vec::new();
    for index in 0..unsafe { children.Length() }.unwrap_or(0) {
        let Ok(child) = (unsafe { children.GetElement(index) }) else { continue };
        if classe_d_icone(&child, classique).is_none() {
            continue;
        }
        let name = unsafe { child.CurrentName() }.map(|n| n.to_string()).unwrap_or_default();
        let Ok(rect) = (unsafe { child.CurrentBoundingRectangle() }) else { continue };
        observed.push((name, rect));
        indices.push(index);
    }
    let Some((rang, ecart)) = nearest_named(&observed, &icon.name, icon.rect) else {
        let apercu: Vec<&str> = observed.iter().take(3).map(|(nom, _)| nom.as_str()).collect();
        return Err(format!(
            "élément NON RETROUVÉ — aucun des {} élément(s) SystemTray.* ne porte le nom {:?} ; vus : {apercu:?}",
            observed.len(),
            icon.name,
        ));
    };
    if ecart > MAX_TRAY_ICON_SHIFT_PX {
        // ⛔ **LA GARDE NE VAUT QUE SI L'ON A UNE POSITION DE RÉFÉRENCE.** Elle
        // existe pour refuser de viser une AUTRE icône quand la barre a glissé
        // (70 px mesurés). Mais quand l'icône vient d'une zone MASQUÉE, sa
        // position mémorisée vaut `0x0` : l'écart avec la position fraîche est
        // ÉNORME **et parfaitement légitime** — c'est le panneau qu'on vient
        // d'ouvrir qui la lui a donnée.
        //
        // Défaut RÉEL, mesuré le 2026-10-06 : « élément "Battle.net" trouvé, mais
        // à 4773 px de la position relevée (tolérance 200) — refus de viser une
        // autre icône », alors que c'ÉTAIT la bonne, et la seule.
        //
        // ⚠️ **CE QU'ON PERD** : sans rectangle de référence, le NOM reste le seul
        // discriminant. Deux icônes qui le partagent ne peuvent plus être
        // départagées par la position — c'est le prix, et il est plus petit que
        // celui de ne jamais pouvoir cliquer une icône masquée.
        let reference = icon.rect.right > icon.rect.left && icon.rect.bottom > icon.rect.top;
        if reference {
            return Err(format!(
                "élément {:?} trouvé, mais à {ecart} px de la position relevée (tolérance {MAX_TRAY_ICON_SHIFT_PX}) — refus de viser une autre icône",
                icon.name,
            ));
        }
    }
    let index = indices[rang];
    let child = unsafe { children.GetElement(index) }
        .map_err(|error| format!("GetElement({index}) : {error:?}"))?;
    // ⚠️ **LE RECTANGLE EST RENDU AVEC L'ÉLÉMENT, ET C'EST LE FRAIS** : celui de
    // `observed`, relevé à l'instant, pas celui de `TrayIcon` qui peut dater de la
    // dernière énumération. Le clic RÉEL en a besoin pour viser juste.
    let rect = observed[rang].1;
    Ok((child, ecart, rect))
}

/// **DÉCLENCHE UNE ICÔNE DE NOTIFICATION — LE GESTE DU CLIC GAUCHE.**
///
/// DEMANDE EXPLICITE (2026-10-06) : « lorsque je fais un clic gauche sur une icône
/// de la zone de notification de l'app, l'icône de la zone de notification de la
/// barre des tâches de Windows reçoive un double clic gauche ».
///
/// ⛔ **`InvokePattern`, ET PAS UNE SIMULATION DE SOURIS — MÊME RAISON QUE LE CLIC
/// DROIT** (voir `show_context_menu`, qui porte la mesure) : `SendInput` exige une
/// fenêtre visible et focalisée, donc déplacer le curseur de l'utilisateur jusque
/// sur la barre des tâches, ce que ce dépôt s'interdit. Ici, c'est
/// **l'application propriétaire qui exécute sa propre action**.
///
/// ⭐ **TOUTES LES ICÔNES DE CETTE MACHINE SONT INVOCABLES — MESURÉ** (sonde
/// `tray_probe::une_icone_de_notification_est_elle_invocable`, 2026-10-06) : **37
/// sur 37**, quinze dans la barre et vingt-deux dans le débordement, toutes avec
/// l'action par défaut « Appuyer ». La sonde est `#[ignore]` et **en lecture
/// seule** — elle interroge des propriétés, elle ne déclenche rien.
///
/// ⚠️ **CE N'EST PAS UN DOUBLE CLIC AU SENS STRICT** : `Invoke` déclenche l'action
/// **par défaut** de l'élément. Pour la plupart des applications c'est le même
/// geste (ouvrir la fenêtre), mais une application qui distingue le clic simple du
/// double clic peut réagir autrement. C'est un CHOIX de l'utilisateur, informé de
/// cette limite, entre ceci et le déplacement de son curseur.
///
/// Rend la **RAISON** de l'échec, comme `open_menu_of`.
fn invoke_of(icon: &TrayIcon) -> Result<(), String> {
    use windows::Win32::UI::Accessibility::{
        IUIAutomationInvokePattern, UIA_InvokePatternId,
    };
    use windows_core::Interface;

    let debut = std::time::Instant::now();
    let (child, ecart, _) = element_de_l_icone(icon)?;
    let motif = unsafe { child.GetCurrentPattern(UIA_InvokePatternId) }
        .map_err(|error| format!("motif Invoke absent : {error:?}"))?;
    let invoke = motif
        .cast::<IUIAutomationInvokePattern>()
        .map_err(|error| format!("cast IUIAutomationInvokePattern : {error:?}"))?;
    let debut_appel = std::time::Instant::now();
    let appel = unsafe { invoke.Invoke() };
    crate::diag!(
        "clic gauche :Invoke a rendu {appel:?} en {} ms (recherche {} ms, élément à {ecart} px de la position relevée)",
        debut_appel.elapsed().as_millis(),
        debut.elapsed().as_millis(),
    );
    appel.map_err(|error| format!("Invoke : {error:?}"))
}

/// **DÉCLENCHE L'ICÔNE DEPUIS UN FIL** — appelée par le bras de clic gauche du
/// dock.
///
/// ⚠️ **SUR UN FIL POUR LA MÊME RAISON QUE LE CLIC DROIT** : les appels UI
/// Automation de la barre XAML **bloquent le fil appelant**, mesuré de 15 ms à
/// 30,9 s pour `ShowContextMenu` (voir sa doc — faire cet appel sur le fil
/// d'interface gelait le dock, et les clics de l'utilisateur s'empilaient pour
/// être rejoués d'un bloc au déblocage). `Invoke` n'a pas été chronométré aussi
/// long, mais rien ne garantit qu'il ne le sera pas : **le dock ne prend pas le
/// risque.**
pub fn invoke_icon(icon: &TrayIcon) {
    let icone = icon.clone();
    std::thread::spawn(move || {
        if let Err(raison) = invoke_of(&icone) {
            crate::diag!(
                "clic gauche NON transmis pour {:?} : {raison}",
                icone.name.lines().next().unwrap_or("")
            );
        }
    });
}

/// Le travail d'ouverture proprement dit — **sur un fil de travail** : voir
/// `show_context_menu` pour la mesure qui l'impose.
///
/// L'élément est retrouvé par `element_de_l_icone`, qui ré-énumère la zone et
/// compare (classe, nom, rectangle) : c'est l'identité même que `enumerate` a
/// relevée, et une comparaison exacte vaut mieux qu'un rapprochement par nom
/// seul, deux entrées pouvant porter le même.
///
/// Rend la **RAISON** de l'échec, et non un simple `false` : les trois chemins
/// muets d'autrefois (élément non retrouvé, `cast` raté, `ShowContextMenu` en
/// erreur) se lisaient tous de la même façon dans le journal, ce qui a coûté
/// une manche de mesure.
fn open_menu_of(icon: &TrayIcon) -> Result<(), String> {
    use windows::Win32::UI::Accessibility::IUIAutomationElement3;
    use windows_core::Interface;

    let (child, ecart, _) = element_de_l_icone(icon)?;
    let contextual = child
        .cast::<IUIAutomationElement3>()
        .map_err(|error| format!("cast IUIAutomationElement3 : {error:?}"))?;
    let debut_appel = std::time::Instant::now();
    let appel = unsafe { contextual.ShowContextMenu() };
    crate::diag!(
        "menu contextuel :ShowContextMenu a rendu {appel:?} en {} ms (élément à {ecart} px de la position relevée)",
        debut_appel.elapsed().as_millis()
    );
    // LE MENU EST OUVERT — sa POSE est le travail du fil de pose, lancé AVANT
    // celui-ci (voir `show_context_menu` : l'ordre est le correctif).
    appel.map_err(|error| format!("ShowContextMenu : {error:?}"))
}

/// **Écart de position maximal entre l'icône relevée et l'élément visé**, en
/// pixels cumulés sur les deux axes. Au-delà, on préfère NE PAS ouvrir de menu
/// plutôt que de viser une icône qui n'est pas la bonne.
///
/// 200 px couvre largement le décalage MESURÉ de l'état de la barre des tâches
/// (**70 px** d'épaisseur, voir `open_menu_of`) et les reptations d'une zone de
/// notification qui se recompose, sans jamais atteindre l'icône voisine.
const MAX_TRAY_ICON_SHIFT_PX: i32 = 200;

/// **LE MEILLEUR CANDIDAT PARMI LES ÉLÉMENTS QUI PORTENT LE NOM CHERCHÉ** — le
/// plus proche de la position relevée. Rend `(rang dans `observed`, écart)`, ou
/// `None` si aucun élément ne porte ce nom.
///
/// POURQUOI PAS UNE ÉGALITÉ DE RECTANGLE, et c'est le défaut qui a coûté une
/// manche de mesure : **la barre des tâches oscille entre deux états — révélée
/// et garée hors écran — qui déplacent TOUTES ses icônes de la même quantité**
/// (mesuré : 70 px en Y). La barre peut donc changer d'état entre l'énumération
/// et le clic, et la comparaison à l'identique rejetait l'élément juste. Le
/// journal le montrait noir sur blanc : « MEME NOM, rect vu (3219,2088,3267,2160)
/// vs attendu (3219,2158,3267,2230) ».
///
/// Le NOM reste le discriminant — deux entrées peuvent le partager, c'est
/// pourquoi une simple correspondance par nom ne suffit pas non plus ; la
/// position ne fait que **départager les homonymes**.
///
/// PURE, et c'est voulu : la règle qui choisit une icône parmi d'autres se teste
/// sans UI Automation.
fn nearest_named(
    observed: &[(String, RECT)],
    wanted_name: &str,
    wanted_rect: RECT,
) -> Option<(usize, i32)> {
    let mut best: Option<(usize, i32)> = None;
    for (rang, (nom, rect)) in observed.iter().enumerate() {
        if nom != wanted_name {
            continue;
        }
        let ecart = (rect.left - wanted_rect.left).abs() + (rect.top - wanted_rect.top).abs();
        if best.map_or(true, |(_, meilleur)| ecart < meilleur) {
            best = Some((rang, ecart));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⛔ **LA CONTRAINTE QUI A FAIT ÉCARTER LE CORRECTIF FACILE.** La mémoire du
    /// dépôt la portait déjà : « `enumerate()` tourne à la seconde ; un
    /// aller-retour ouverture/fermeture par seconde serait intolérable ».
    ///
    /// On simule donc **une heure à 1 Hz** avec l'horloge de la décision, et on
    /// compte les ouvertures. C'est la seule chose qui prouve que le panneau de
    /// l'utilisateur ne s'ouvre pas 3 600 fois — et c'est le genre de régression
    /// qu'un « il suffit de retirer la retenue » réintroduirait sans un mot.
    #[test]
    fn une_heure_a_un_hertz_en_regime_n_ouvre_jamais_le_panneau() {
        let pas = Duration::from_secs(1);
        let mut age_retenue = Some(Duration::ZERO);
        let age_essai = Some(Duration::ZERO);
        let mut ouvertures = 0usize;
        for _ in 0..3600 {
            if faut_il_ouvrir(age_retenue, age_essai) {
                ouvertures += 1;
            }
            // **La lecture a réussi CE cycle** : `retenir` remet la date à zéro,
            // puis une seconde passe jusqu'au cycle suivant. C'est le régime
            // MESURÉ — l'île persiste après la première ouverture.
            age_retenue = Some(pas);
        }
        assert_eq!(
            ouvertures, 0,
            "{ouvertures} ouverture(s) alors que l'île était lisible à chaque cycle"
        );
    }

    /// **SI L'ÎLE DISPARAÎT** — `explorer.exe` redémarré, par exemple — la retenue
    /// cesse d'être rafraîchie et vieillit. On rouvre alors, mais **au plus une
    /// fois par période** : c'est le filet, et il doit rester borné.
    #[test]
    fn si_l_ile_disparait_on_rouvre_au_plus_une_fois_par_periode() {
        let pas = Duration::from_secs(1);
        let mut age_retenue = Some(Duration::ZERO);
        let mut age_essai = Some(Duration::ZERO);
        let mut ouvertures = 0usize;
        for _ in 0..3600 {
            if faut_il_ouvrir(age_retenue, age_essai) {
                ouvertures += 1;
                age_essai = Some(Duration::ZERO);
                // La réouverture a RÉUSSI : la retenue redevient fraîche.
                age_retenue = Some(Duration::ZERO);
            }
            age_retenue = age_retenue.map(|age| age + pas);
            age_essai = age_essai.map(|age| age + pas);
        }
        let attendu = (3600 / PEREMPTION_MASQUEES.as_secs()) as usize;
        assert!(
            ouvertures <= attendu + 1,
            "{ouvertures} ouverture(s) en une heure d'île absente, attendu au plus {} — \
             le panneau s'ouvrirait sur l'écran de l'utilisateur",
            attendu + 1
        );
        assert!(ouvertures > 0, "aucune réouverture : la zone masquée ne se réparerait jamais");
    }

    /// ⚠️ **LE CAS QUE LA FRAÎCHEUR NE COUVRE PAS** : quand la lecture échoue, la
    /// retenue reste VIDE — donc l'âge de la retenue ne borne rien, et seul le
    /// refroidissement empêche de réessayer à chaque cycle.
    #[test]
    fn un_echec_n_est_pas_rejoue_a_chaque_cycle() {
        assert!(faut_il_ouvrir(None, None), "au premier cycle, on tente");
        assert!(!faut_il_ouvrir(None, Some(Duration::from_secs(1))), "pas une seconde plus tard");
        assert!(
            !faut_il_ouvrir(None, Some(REFROIDISSEMENT_PANNEAU - Duration::from_secs(1))),
            "et pas juste avant le refroidissement"
        );
        assert!(faut_il_ouvrir(None, Some(REFROIDISSEMENT_PANNEAU)), "refroidissement écoulé");
    }

    /// **L'INVARIANT DES DEUX BORNES** : la péremption est la POLITIQUE et le
    /// refroidissement le GARDE-FOU. Si la péremption passait en dessous, c'est le
    /// refroidissement qui mènerait — et la constante documentée comme « le seul
    /// réglage à tourner » ne serait plus celle qui agit.
    #[test]
    fn la_peremption_ne_descend_pas_sous_le_refroidissement() {
        assert!(
            PEREMPTION_MASQUEES >= REFROIDISSEMENT_PANNEAU,
            "péremption {} < refroidissement {} : le réglage documenté ne serait plus celui qui agit",
            PEREMPTION_MASQUEES.as_secs(),
            REFROIDISSEMENT_PANNEAU.as_secs()
        );
    }

    /// **LE DÉFAUT QUI A COÛTÉ UNE MANCHE DE MESURE** : la barre des tâches
    /// oscille entre deux états qui translatent toutes ses icônes (mesuré : 70 px
    /// en Y), donc le rectangle relevé à l'énumération n'est plus celui du clic —
    /// alors que le nom, lui, n'a pas bougé.
    #[test]
    fn the_icon_is_found_even_when_the_taskbar_changed_state() {
        let observed = vec![(
            "Avertissement".to_string(),
            RECT { left: 3123, top: 2158, right: 3171, bottom: 2230 },
        )];
        let wanted = RECT { left: 3123, top: 2088, right: 3171, bottom: 2160 };

        assert_eq!(
            nearest_named(&observed, "Avertissement", wanted),
            Some((0, 70)),
            "même nom, barre garée 70 px plus bas : l'élément doit être retenu"
        );
    }

    /// **UN FOND QUI VARIE NE DOIT PAS ÊTRE PRIS POUR DU CONTENU** — c'est le cas
    /// réel des icônes VISIBLES, capturées sur la barre des tâches FLOUTÉE dont la
    /// teinte change d'un bord à l'autre. Avec une référence unique, les bords
    /// comptaient comme du contenu et la boîte restait celle du bouton entier.
    #[test]
    fn a_varying_background_does_not_widen_the_box() {
        let (largeur, hauteur) = (60u32, 30u32);
        let mut image = vec![0u8; (largeur * hauteur * 4) as usize];
        for y in 0..hauteur {
            for x in 0..largeur {
                let offset = ((y * largeur + x) * 4) as usize;
                let dans_le_carre = (24..36).contains(&x) && (9..21).contains(&y);
                if dans_le_carre {
                    image[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
                } else {
                    // Fond qui VARIE horizontalement, de 30 à 90 : soixante unités
                    // d'écart entre les deux bords, bien au-delà de toute marge.
                    let v = 30 + (x * 60 / largeur) as u8;
                    image[offset..offset + 4].copy_from_slice(&[v, v, v, 255]);
                }
            }
        }

        let (cote, _, _) = trim_capture_to_content(&image, largeur, hauteur);

        assert_eq!(
            cote, 12,
            "le fond qui varie ne compte pas : la boîte doit être le carré de 12, \
             sinon l'icône capturée est dessinée deux fois trop petite"
        );
    }

    /// **UN LISERÉ IMPERCEPTIBLE NE DOIT PAS ÉLARGIR LA BOÎTE.** Défaut réel
    /// signalé : « certaines icônes semblent avoir une marge invisible qui fait
    /// qu'elles sont plus petites que les autres ». Un alpha de 1 à 15 comptait
    /// comme du contenu, la boîte englobante grandissait avec lui, et le complément
    /// à un carré **rapetissait le logo dans sa propre case**.
    #[test]
    fn an_imperceptible_fringe_does_not_widen_the_box() {
        // 20×20 transparent, un carré opaque de 8×8 au centre, et un liseré
        // d'alpha 4 qui court jusqu'aux bords — invisible à l'œil.
        let (largeur, hauteur) = (20u32, 20u32);
        let mut image = vec![0u8; (largeur * hauteur * 4) as usize];
        for y in 0..hauteur {
            for x in 0..largeur {
                let offset = ((y * largeur + x) * 4) as usize;
                let dans_le_carre = (6..14).contains(&x) && (6..14).contains(&y);
                let sur_le_lisere =
                    x == 0 || y == 0 || x == largeur - 1 || y == hauteur - 1;
                if dans_le_carre {
                    image[offset..offset + 4].copy_from_slice(&[255, 0, 0, 255]);
                } else if sur_le_lisere {
                    image[offset..offset + 4].copy_from_slice(&[255, 0, 0, 4]);
                }
            }
        }

        let (cote, _, _) = trim_to_content(&image, largeur, hauteur, None);

        assert_eq!(
            cote, 8,
            "le liseré d'alpha 4 ne compte pas : la boîte doit être celle du carré de 8, \
             sinon le complément à un carré rapetisse le logo dans sa case"
        );
    }

    /// **LE NOM SEUL NE SUFFIT PAS** — la doc de `open_menu_of` l'affirme, ce test
    /// le tient : entre deux homonymes, c'est la POSITION qui départage.
    #[test]
    fn the_closest_of_two_homonyms_wins() {
        let observed = vec![
            ("Voicemeeter".to_string(), RECT { left: 3387, top: 1545, right: 3840, bottom: 1733 }),
            ("Voicemeeter".to_string(), RECT { left: 100, top: 100, right: 553, bottom: 288 }),
        ];
        let wanted = RECT { left: 3400, top: 1550, right: 3853, bottom: 1738 };

        assert_eq!(nearest_named(&observed, "Voicemeeter", wanted), Some((0, 18)));
        assert_eq!(
            nearest_named(&observed, "Aucun", wanted),
            None,
            "un nom absent ne rend rien — l'appelant s'en sert pour refuser d'ouvrir"
        );
    }

    #[test]
    fn seule_l_icone_reseau_du_shell_ouvre_les_parametres_reseau() {
        let icone = |name: &str, from_shell| TrayIcon {
            name: name.to_string(),
            class: "SystemTray.AccentButton".to_string(),
            rect: RECT::default(),
            zone: TrayZone::Visible,
            from_shell,
        };
        assert!(est_l_icone_reseau(&icone("Réseau Freebox-68B345_FAST", true)));
        assert!(!est_l_icone_reseau(&icone("Réseau Freebox-68B345_FAST", false)), "une application garde son menu");
        assert!(!est_l_icone_reseau(&icone("Volume Voicemeeter Input (VB-Audio): 100%", true)));
        assert!(!est_l_icone_reseau(&icone("", true)), "une entrée sans nom n'est pas le réseau");
    }

    /// **LA TABLE SYSTÈME, ÉPROUVÉE SUR LES NOMS RÉELLEMENT MESURÉS** — ceux que
    /// la sonde a relevés sur la machine de l'utilisateur, zone affichée.
    #[test]
    fn recognises_the_system_entries_by_their_measured_names() {
        // Les préfixes viennent du relevé, avec leurs bizarreries : espace de
        // tête, pluriel, apostrophe typographique.
        assert_eq!(system_glyph("Volume Voicemeeter Input (VB-Audio): 100%"), Some('\u{E767}'));
        assert_eq!(system_glyph("Réseau Freebox-68B345_FAST\nAccès Internet"), Some('\u{E701}'));
        assert_eq!(system_glyph("Confidentialité Microphone utilisé par :"), Some('\u{E720}'));
        assert_eq!(system_glyph("Clavier tactile"), Some('\u{E765}'));
        assert_eq!(system_glyph("Ajouter des emojis, des images GIF"), Some('\u{E76E}'));
        assert_eq!(system_glyph("Avertissement"), Some('\u{E7BA}'));
        // L'apostrophe est TYPOGRAPHIQUE dans le relevé (U+2019) : le préfixe
        // s'arrête avant, pour ne pas en dépendre.
        assert_eq!(system_glyph("Indicateur d\u{2019}entrée dans la barre d\u{2019}état"), Some('\u{E774}'));
        // Les icônes d'ÉTAT — celles qui n'ont AUCUN instantané dans le registre
        // (vérifié), et qui affichaient donc une initiale. DEMANDE EXPLICITE de
        // l'utilisateur après les avoir vues : « il y a V, S, ? ».
        assert_eq!(system_glyph("Vous êtes protégé"), Some('\u{EA18}'));
        assert_eq!(system_glyph("Synchronisation terminée avec succès"), Some('\u{E895}'));
        assert_eq!(system_glyph("Steam - Synchronisation"), None, "une APPLICATION garde son instantané");
        // ⚠️ CES DEUX-LÀ N'ONT PLUS DE GLYPHE, ET C'EST UN PROGRÈS : leur nom est
        // celui d'une application du STORE (« Sécurité Windows » →
        // Windows Security, « Mobile connecté » → Phone Link, MESURÉ), donc
        // `match_installed_app` les identifie et leur donne leur VRAIE icône. Un
        // glyphe les aurait privées de mieux, puisqu'il passe avant.
        assert_eq!(system_glyph("Sécurité Windows - Aucune action requise."), None);
        assert_eq!(system_glyph("Mobile connecté - Connecté"), None);

        // Les entrées d'APPLICATION ne reçoivent AUCUN glyphe système : elles
        // passent au niveau suivant (l'instantané du registre, puis
        // l'exécutable), et c'est ce qui leur donne leur vraie icône.
        //
        // ⚠️ « Vous êtes protégé » a été RETIRÉ de cette liste, et c'est un
        // changement de comportement assumé : cette entrée n'a aucun instantané
        // dans le registre (vérifié), donc elle affichait une INITIALE — « V »,
        // que l'utilisateur a explicitement signalée. Elle reçoit désormais le
        // bouclier, ce qu'un test écrit avant cette décision ne peut plus
        // affirmer.
        for app in [
            "MSI Afterburner v4.6.6.16757",
            "MEGAsync 5.16.0",
            " OneDrive - Personnel\nSauvegardé et synchronisé",
            "Discord",
            "Flameshot",
        ] {
            assert_eq!(system_glyph(app), None, "{app:?} ne doit pas recevoir de glyphe système");
        }
    }

    #[test]
    fn tolerates_a_leading_space_in_the_tooltip() {
        assert_eq!(system_glyph("  Volume : 100%"), Some('\u{E767}'));
    }

    /// **LES DEUX DÉFAUTS DE RAPPROCHEMENT RÉELLEMENT MESURÉS**, et chacun a
    /// coûté une icône fausse à l'écran — une LETTRE à la place du logo.
    ///
    /// Les instantanés de ce test sont recopiés du registre de la machine, avec
    /// leurs bizarreries : c'est ce qui rend le test utile.
    /// Touch Portal (mesuré) : tout ce qui est visible est noir — écarté. Un logo
    /// sombre avec un seul pixel clair, ou une image vide, ne l'est pas.
    #[test]
    fn un_instantane_entierement_noir_est_ecarte() {
        let noir = [5, 5, 5, 255];
        let clair = [200, 120, 40, 255];
        let transparent = [255, 255, 255, 0];
        assert!(entierement_noire(&[noir, transparent, noir].concat()));
        assert!(!entierement_noire(&[noir, noir, clair].concat()));
        assert!(!entierement_noire(&[transparent, transparent].concat()));
    }

    #[test]
    fn matches_whatsapp_and_gcc_whose_registry_entries_are_awkward() {
        let snapshots = |pairs: &[(&str, &str)]| -> Vec<TrayIconSnapshot> {
            pairs
                .iter()
                .map(|(tooltip, executable)| TrayIconSnapshot {
                    tooltip: (*tooltip).to_string(),
                    executable: (*executable).to_string(),
                    png: Vec::new(),
                    guid: None,
                    id: 0,
                })
                .collect()
        };

        // WHATSAPP : application EMPAQUETÉE, donc son exécutable s'appelle
        // `WhatsApp.Root.exe` et son infobulle est VIDE. Le nom vivant ne
        // contient donc pas la racine de l'exécutable — c'est l'inverse. Sans le
        // sens inverse, il n'obtenait aucune correspondance.
        let whatsapp = snapshots(&[(
            "",
            r"{6D809377-6AF0-444B-8957-A3773F02200E}\WindowsApps\5319275A.WhatsAppDesktop_2.2632.100.0_x64__cv1g1gvanyjgm\WhatsApp.Root.exe",
        )]);
        assert_eq!(match_snapshot("WhatsApp", &whatsapp), Some(0));

        // GCC : l'infobulle du registre porte une AUTRE VERSION
        // (« Ver25.07.21.01 » contre « Ver26.08.28.01 » vivante), donc aucune
        // inclusion ne les rapproche ; c'est l'égalité du PREMIER MOT qui le
        // fait. Et son exécutable — `GCC.exe` — a une racine de trois
        // caractères, sous le seuil d'inclusion.
        let gcc = snapshots(&[("GCC Ver25.07.21.01", r"{6D809377-…}\GIGABYTE\Control Center\GCC.exe")]);
        assert_eq!(match_snapshot("GCC Ver26.08.28.01", &gcc), Some(0));

        // LE GARDE-FOU : un exécutable à la racine TROP COURTE ne doit pas être
        // happé par l'inclusion inverse. `app.exe` est contenu dans « whatsapp »
        // — c'est précisément le faux positif que le seuil écarte.
        let short = snapshots(&[("", r"C:\outils\app.exe")]);
        assert_eq!(match_snapshot("WhatsApp", &short), None);

        // AMD SOFTWARE : le dernier recours, et il ne doit SERVIR QUE de dernier
        // recours — la même liste contient une correspondance FORTE, qui doit
        // gagner malgré un préfixe tout aussi valable.
        let amd = snapshots(&[
            ("", r"{6D809377-…}\AMD\AMDInstallManager\AMDInstallManager.exe"),
            ("AMD Software: Adrenalin Edition 25.9.1", r"{6D809377-…}\AMD\CNext\RadeonSoftware.exe"),
        ]);
        assert_eq!(
            match_snapshot("AMD Software: Adrenalin Edition", &amd),
            Some(1),
            "l'égalité doit l'emporter sur le préfixe"
        );
        // Le même, sans la correspondance forte : le préfixe prend le relais
        // plutôt que de laisser une lettre.
        let amd_only_prefix = snapshots(&[("", r"{6D809377-…}\AMD\AMDInstallManager\AMDInstallManager.exe")]);
        assert_eq!(match_snapshot("AMD Software: Adrenalin Edition", &amd_only_prefix), Some(0));
    }

    /// **L'IDENTIFICATION AUTOMATIQUE PAR LE MENU DÉMARRER** — c'est elle qui
    /// donne leur vraie icône aux entrées dont le nom ne dit pas le programme.
    /// Les couples nom→AUMID de ce test sont ceux RELEVÉS sur la machine
    /// (`Get-StartApps`) : ce sont les noms affichés des applications du Store.
    #[test]
    fn identifies_a_notification_by_the_display_name_of_an_installed_app() {
        let installed: Vec<String> = [
            "Paramètres",
            "Mobile connecté",
            "Sécurité Windows",
            "WhatsApp",
            "Brave",
            "Calculatrice",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        // L'infobulle VIVANTE porte un état en plus du nom de l'application.
        assert_eq!(match_installed_app("Mobile connecté - Connecté", &installed), Some(1));
        assert_eq!(
            match_installed_app("Sécurité Windows - Aucune action requise. Sécurité Windows - Actions recommandées.", &installed),
            Some(2)
        );
        // Une correspondance directe continue de marcher.
        assert_eq!(match_installed_app("WhatsApp", &installed), Some(3));
        // Et ce qui n'est PAS une application installée ne doit rien rendre : ni
        // une entrée du shell, ni un nom d'état sans application.
        assert_eq!(match_installed_app("Volume Voicemeeter Input: 100%", &installed), None);
        assert_eq!(match_installed_app("Vous êtes protégé", &installed), None);
    }

    /// **LE RAPPROCHEMENT AVEC LES APPLICATIONS OUVERTES, ÉPROUVÉ SUR LES
    /// INFOBULLES RÉELLEMENT MESURÉES.** C'est ce qui donne à ces entrées leur
    /// vraie icône, faute de pouvoir la lire dans Windows (voir la doc de
    /// module : les pixels sont inaccessibles, la barre étant en auto-masquage).
    #[test]
    fn matches_a_notification_to_a_running_application_by_its_tooltip() {
        let running: Vec<String> = [
            "explorer.exe",
            "OneDrive.exe",
            "MEGAsync.exe",
            "MSIAfterburner.exe",
            "NordVPN.exe",
            "brave.exe",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        // Cas directs.
        assert_eq!(match_open_app(" OneDrive - Personnel\nSauvegardé", &running), Some(1));
        assert_eq!(match_open_app("MEGAsync 5.16.0", &running), Some(2));
        // LE cas qui justifie la normalisation : l'infobulle écrit « MSI
        // Afterburner » avec une ESPACE, l'exécutable s'appelle
        // `MSIAfterburner.exe` — aucune comparaison de casse ne les rapproche.
        assert_eq!(match_open_app("MSI Afterburner v4.6.6.16757", &running), Some(3));

        // Ce qui NE SE RAPPROCHE PAS, et c'est une information : l'infobulle de
        // NordVPN ne nomme pas son application. L'appelant a un troisième niveau
        // (l'initiale) pour ce cas.
        assert_eq!(match_open_app("Vous êtes protégé", &running), None);
        // Un nom système ne doit jamais être attribué à une application : ni
        // « Volume » ni « Clavier tactile » ne contiennent un nom d'exécutable.
        assert_eq!(match_open_app("Volume Voicemeeter Input: 100%", &running), None);
        assert_eq!(match_open_app("Clavier tactile", &running), None);
    }

    /// La correspondance la PLUS LONGUE gagne : une infobulle ne doit pas être
    /// attribuée à un exécutable court quand un plus spécifique est ouvert.
    #[test]
    fn prefers_the_longest_matching_executable_name() {
        let running: Vec<String> =
            ["Code.exe", "CodeSetup.exe", "MSTeams.exe", "Teams.exe"].iter().map(|s| s.to_string()).collect();
        // « CodeSetup » est plus long que « Code » : c'est lui qui gagne.
        assert_eq!(match_open_app("CodeSetup 1.2", &running), Some(1));
        // Et un exécutable trop court n'est jamais retenu, même présent.
        let tiny: Vec<String> = ["ab.exe"].iter().map(|s| s.to_string()).collect();
        assert_eq!(match_open_app("ab", &tiny), None);
    }

    /// **LE FILTRE, ÉPROUVÉ SUR LES COUPLES RÉELLEMENT MESURÉS** — pas sur des
    /// exemples inventés. Ces (classe, nom) sont ceux que la sonde a relevés sur
    /// la machine de l'utilisateur, zone de notification affichée.
    #[test]
    fn keeps_the_notification_icons_and_drops_the_taskbar_furniture() {
        // Les icônes de notification — gardées.
        for (class, name) in [
            ("SystemTray.NormalButton", "MSI Afterburner v4.6.6.16757"),
            ("SystemTray.NormalButton", " OneDrive - Personnel\nSauvegardé et synchronisé"),
            ("SystemTray.NormalButton", "MEGAsync 5.16.0"),
            ("SystemTray.AccentButton", "Réseau Freebox-68B345_FAST\nAccès Internet"),
            ("SystemTray.OmniButtonRight", "Volume Voicemeeter Input (VB-Audio): 100%"),
            ("SystemTray.AccentText", "Confidentialité Microphone utilisé par :"),
            ("SystemTray.NormalButton", "Indicateur d’entrée dans la barre d’état"),
            ("SystemTray.NormalButton", "Clavier tactile"),
        ] {
            assert!(keep(name, class, true), "devrait être gardé : {name:?} / {class}");
        }

        // Le mobilier de la barre — écarté, décision de l'utilisateur.
        // « Afficher le Bureau » par sa CLASSE : indépendant de la langue.
        assert!(!keep("Afficher le Bureau", "SystemTray.ShowDesktopButton", true));
        // L'Horloge, la cloche et le chevron par leur NOM — la fragilité assumée.
        assert!(!keep("Horloge 00:07\n\u{200e}21/\u{200e}09/\u{200e}2026", "SystemTray.OmniButtonLeft", true));
        assert!(!keep("Notifications 36 nouvelles notifications", "SystemTray.AccentButton", true));
        assert!(!keep("Afficher les icônes cachées", "SystemTray.NormalButton", true));

        // Et ce qui n'est PAS de la zone de notification n'y entre pas, même si
        // le nom ressemble à une icône : c'est la CLASSE qui fait foi.
        assert!(!keep("Brave - 1 fenêtre en cours d’exécution épinglé", "Taskbar.TaskListButtonAutomationPeer", true));
        assert!(!keep("Démarrer", "ToggleButton", true));
        assert!(!keep("Volume", "SystemTrayFake.NormalButton", true), "le préfixe doit être exact");
    }

    /// **LA ZONE MASQUÉE N'A PAS D'EXCLUSIONS** — et c'est mesuré, pas supposé :
    /// les 66 descendants de la fenêtre de débordement sont TOUS des
    /// `SystemTray.NormalButton`, aucun n'est l'horloge ni le chevron. Appliquer
    /// les exclusions de la barre y retirerait donc des icônes légitimes.
    #[test]
    fn the_hidden_zone_keeps_everything_in_the_notification_prefix() {
        // Les noms relevés dans la fenêtre de débordement de l'utilisateur.
        for name in [
            "WhatsApp",
            "DeepL",
            "Paramètres NVIDIA",
            "Everything",
            "NordVPN Déconnecté",
            "Steam - Synchronisation",
            "PowerToys Awake PowerToys Awake",
        ] {
            assert!(keep(name, "SystemTray.NormalButton", false), "la zone masquée garde {name:?}");
        }
        // Le PREFIXE de classe, lui, reste la règle : la fenêtre de débordement
        // ne contient que des icônes, mais on ne l'ouvre pas pour autant à tout.
        assert!(!keep("quelque chose", "Taskbar.TaskListButtonAutomationPeer", false));
        // Et la démonstration de la différence : un nom « exclu » de la barre
        // serait GARDÉ dans la zone masquée — ce qui n'arrive pas en pratique
        // (mesuré), mais montre que la règle y est bien différente.
        assert!(!keep("Horloge", "SystemTray.OmniButtonLeft", true));
        assert!(keep("Horloge", "SystemTray.OmniButtonLeft", false));
    }

    /// **WINDOWS 10 : LES ICÔNES DE LA BARRE CLASSIQUE SONT GARDÉES** — les noms
    /// relevés sur la VM 22H2 le 2026-10-30. Avant, `keep` ne connaissait que le
    /// préfixe `SystemTray.` et la zone de Windows 10 restait VIDE.
    #[test]
    fn the_windows_10_classic_toolbar_icons_are_kept() {
        for name in ["Démarrer une réunion", "OneDrive - Personnel\nNon connecté", "Réseau\nAccès Internet", "Haut-parleurs : 67%"] {
            assert!(keep(name, CLASSIC_TRAY_CLASS, true), "la barre classique garde {name:?}");
        }
        for name in ["Sécurité Windows - Actions recommandées.", "Oracle VirtualBox Guest Additions 7.2.22r175245"] {
            assert!(keep(name, CLASSIC_TRAY_CLASS, false), "le débordement classique garde {name:?}");
        }
    }

    /// **LES ICÔNES DU SHELL, DANS LES DEUX LIGNÉES** — les `AutomationId` relevés.
    #[test]
    fn the_shell_icons_are_recognised_on_both_windows_lineages() {
        assert!(est_du_shell("SystemTrayIcon"));
        assert!(!est_du_shell("NotifyItemIcon"));
        // Windows 10 : Haut-parleurs, Réseau, « Démarrer une réunion ».
        assert!(est_du_shell("{7820AE73-23E3-4229-82C1-E41CB67D5B9C}"));
        assert!(est_du_shell("{7820AE74-23E3-4229-82C1-E41CB67D5B9C}"));
        assert!(est_du_shell("{7820AE83-23E3-4229-82C1-E41CB67D5B9C}"));
        // Des applications : un autre GUID (Sécurité Windows, Copilot), ou rien.
        assert!(!est_du_shell("{BA82E2DC-F405-47AD-B032-CF0FAA0E3933}"));
        assert!(!est_du_shell("{B8F3F6B2-4A1D-4E2C-9F3A-1B2C3D4E5F60}"));
        assert!(!est_du_shell(""));
    }

    #[test]
    fn the_windows_10_speaker_gets_the_volume_glyph() {
        assert_eq!(system_glyph("Haut-parleurs : 67%"), system_glyph("Volume"));
        assert!(system_glyph("Haut-parleurs : 67%").is_some());
    }

    /// Fabrique une entrée du miroir — seuls comptent ici le nom et le rectangle.
    fn icone(nom: &str, left: i32, top: i32) -> TrayIcon {
        TrayIcon {
            name: nom.to_string(),
            class: "SystemTray.NotifyItemIcon".to_string(),
            rect: RECT {
                left,
                top,
                right: left + 60,
                bottom: top + 60,
            },
            zone: TrayZone::Hidden,
            from_shell: false,
        }
    }

    fn rect(left: i32, top: i32, largeur: i32, hauteur: i32) -> RECT {
        RECT {
            left,
            top,
            right: left + largeur,
            bottom: top + hauteur,
        }
    }

    /// **LE TEST QUI COMPTE, ET SES VALEURS SONT RECOPIÉES DU RELEVÉ DU
    /// 2026-09-21** — pas inventées (même méthode que les infobulles réelles de
    /// `matches_whatsapp_and_gcc_whose_registry_entries_are_awkward`).
    ///
    /// Ce qu'il protège, et pourquoi il mérite d'exister : le shell ne rend PAS
    /// le rectangle de l'icône, il rend le sien, **décalé de +38 à +45 en
    /// ordonnée**. Or la grille du panneau a un pas de **60 px**.
    ///
    /// ⚠️ **« La cellule la plus proche » se trompe donc SYSTÉMATIQUEMENT.**
    /// Pour Phone Link (shell `y=1931`), la cellule située une rangée plus bas
    /// (« Flameshot », `y=1953`) est à **22 px**, quand la bonne (« Mobile
    /// connecté », `y=1893`) est à **38**. Le leurre est dans le jeu d'essai
    /// exprès : sans lui, le test ne prouverait rien.
    #[test]
    fn estimates_the_offset_the_shell_adds_instead_of_tolerating_it() {
        let icons = vec![
            icone("WhatsApp", 2949, 1658),
            icone("AMD Software: Adrenalin Edition", 2949, 1953),
            icone("Mobile connecté - Connecté", 3010, 1893),
            icone("Flameshot", 3010, 1953), // LE LEURRE, 22 px sous Phone Link
            icone("Sécurité Windows", 3130, 2013),
        ];
        // Les rectangles rendus par le shell, recopiés tels quels.
        let places = vec![
            (0usize, rect(2950, 1698, 60, 59)), // WhatsApp.Root.exe       (+1, +40)
            (1, rect(2950, 1991, 60, 59)),      // RadeonSoftware.exe      (+1, +38)
            (2, rect(3010, 1931, 59, 60)),      // PhoneExperienceHost.exe ( 0, +38)
            (3, rect(3130, 2058, 60, 60)),      // SecurityHealthSystray   ( 0, +45)
        ];

        let decalage =
            estimate_panel_offset(&places, &icons).expect("le décalage aurait dû être estimé");

        // LES QUATRE tombent sur la bonne icône — y compris celle décalée de
        // +45, qui avait fait échouer une tolérance de 4.
        assert_eq!(icon_at_offset(&places[0].1, decalage, &icons), Some(0));
        assert_eq!(icon_at_offset(&places[1].1, decalage, &icons), Some(1));
        assert_eq!(icon_at_offset(&places[2].1, decalage, &icons), Some(2));
        assert_eq!(icon_at_offset(&places[3].1, decalage, &icons), Some(4));

        // Et le leurre ne prend la place de personne.
        assert_ne!(icon_at_offset(&places[2].1, decalage, &icons), Some(3));

        // Le décalage retenu est PETIT : il ne doit jamais ressembler à un pas
        // de grille, sinon il pourrait faire glisser une icône sur sa voisine.
        assert!(
            decalage.1.abs() < 60,
            "un décalage d'une rangée entière serait un glissement, pas un décalage"
        );
    }

    /// **UN DÉCALAGE QU'UN SEUL GUID EXPLIQUE EST REFUSÉ.** Rien ne distingue
    /// alors le bon décalage d'une rencontre fortuite — et **se tromper
    /// d'identité est pire que de n'en pas donner** (même doctrine que
    /// `identify_by_elimination`, qui refuse à plus d'un candidat).
    #[test]
    fn refuses_an_offset_that_only_one_guid_corroborates() {
        let icons = vec![icone("Discord", 2949, 2013)];
        let places = vec![(0usize, rect(2950, 2051, 60, 60))];
        assert_eq!(estimate_panel_offset(&places, &icons), None);
    }

    /// Deux icônes au même endroit : anomalie d'UI Automation. **On ne choisit
    /// pas au hasard — on refuse.**
    #[test]
    fn refuses_to_choose_when_two_icons_share_a_place() {
        let icons = vec![icone("A", 100, 100), icone("B", 100, 100)];
        assert_eq!(icon_at_offset(&rect(100, 100, 60, 60), (0, 0), &icons), None);
    }

    /// **LA VALEUR `IconGuid` DU REGISTRE SE LIT, ET UNE VALEUR MALFORMÉE NE
    /// PRODUIT PAS UN GUID INVENTÉ.**
    ///
    /// La valeur entre accolades est celle de Discord, recopiée du registre.
    /// Elle est vérifiée sur ses CHAMPS et non via `Debug`, dont le formatage
    /// est un choix d'affichage et non la valeur.
    #[test]
    fn reads_the_guid_of_a_registry_entry_and_ignores_a_malformed_one() {
        let discord = guid_from_registry_text("{699E423D-73DA-42BA-B94E-881FF8672467}")
            .expect("le GUID mesuré aurait dû être lu");
        assert_eq!(discord.data1, 0x699E_423D);
        assert_eq!(discord.data2, 0x73DA);
        assert_eq!(discord.data3, 0x42BA);
        assert_eq!(
            discord.data4,
            [0xB9, 0x4E, 0x88, 0x1F, 0xF8, 0x67, 0x24, 0x67]
        );

        // Les espaces autour sont tolérés (le registre n'en met pas, mais une
        // valeur recopiée à la main en porterait).
        assert!(guid_from_registry_text("  {699E423D-73DA-42BA-B94E-881FF8672467}  ").is_some());

        for malforme in [
            "",
            "pas un guid",
            "{699E423D-73DA-42BA-B94E-881FF867246",  // tronqué d'un caractère
            "{}",
            "699E423D73DA42BAB94E881FF8672467",      // sans tirets : le registre n'en produit jamais
        ] {
            assert_eq!(
                guid_from_registry_text(malforme),
                None,
                "une valeur malformée ne doit pas produire un GUID : {malforme:?}"
            );
        }
    }

    /// **LE NOM VIENT DE L'EXÉCUTABLE, ET UN CHEMIN SANS NOM DE FICHIER N'EN
    /// DONNE AUCUN** — auquel cas l'appelant ne nomme pas.
    #[test]
    fn derives_a_display_name_from_an_executable_path() {
        assert_eq!(
            name_from_executable(r"C:\Users\utilisateur\AppData\Local\Discord\app-1.0.9258\Discord.exe")
                .as_deref(),
            Some("Discord")
        );
        // Chemin PSEUDO à `KNOWNFOLDERID` — mesuré sur cette machine, et c'est
        // précisément lui qui interdit de lire les `VERSIONINFO`.
        assert_eq!(
            name_from_executable(
                r"{6D809377-6AF0-444B-8957-A3773F02200E}\WindowsApps\5319275A.WhatsAppDesktop\WhatsApp.Root.exe"
            )
            .as_deref(),
            Some("WhatsApp.Root")
        );
        // Seule la DERNIÈRE extension tombe — comportement déjà livré ailleurs.
        assert_eq!(
            name_from_executable(r"C:\outils\mon.app.exe").as_deref(),
            Some("mon.app")
        );

        for sans_nom in ["", "   ", r"C:\dossier\", r"{6D809377-6AF0-444B-8957-A3773F02200E}\"] {
            assert_eq!(
                name_from_executable(sans_nom),
                None,
                "un chemin sans nom de fichier ne doit rien produire : {sans_nom:?}"
            );
        }
    }
}
