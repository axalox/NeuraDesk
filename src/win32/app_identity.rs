//! **Qui est qui, du point de vue de Windows** : l'identité d'une application,
//! pour répondre aux deux questions que le dock se pose sans arrêt — « est-ce
//! ouvert ? » et « où est sa fenêtre, pour basculer dessus ? ».
//!
//! Pendant longtemps la réponse a tenu dans le seul nom de fichier de
//! l'exécutable, et ça suffit pour une application de bureau ordinaire. Ça ne
//! suffit plus depuis que le dock affiche les épingles de la VRAIE barre des
//! tâches Windows (voir `win32::taskbar_pins`) : une application du Store n'a
//! pas d'exécutable à elle, et un raccourci peut pointer sur un LANCEUR plutôt
//! que sur l'application.
//!
//! Windows a une notion d'identité unique prévue pour ça : l'**AppUserModelID**
//! (AUMID). Le piège est qu'il n'est PAS rangé au même endroit selon la nature
//! de l'application. Deux sources, toutes deux MESURÉES sur la machine de
//! l'utilisateur (2026-09-15, sonde `SHGetPropertyStoreForWindow` /
//! `GetApplicationUserModelId` sur les 20 fenêtres ouvertes) :
//!
//! | Source | Rend l'AUMID de | Ce qu'elle donne ici | Ce qu'elle rate |
//! | --- | --- | --- | --- |
//! | `PKEY_AppUserModel_ID`, sur la FENÊTRE | ce que le processus a déclaré pour ses fenêtres | `Chrome`, `Brave`, `Valve.Steam.Client` | Terminal : propriété **absente** |
//! | `GetApplicationUserModelId`, sur le PROCESSUS | l'identité de **paquet** | `Microsoft.WindowsTerminal_8wekyb3d8bbwe!App`, `5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App` | `ERROR_NOT_FOUND` pour tout ce qui n'est pas empaqueté (Chrome, Discord, Brave…) |
//!
//! **Les deux se COMPLÈTENT exactement** : chacune rate ce que l'autre trouve,
//! et aucune ne recouvre l'autre. C'est toute la raison d'être de ce module :
//! `OpenWindow::matches` interroge les DEUX, jamais une seule. S'appuyer sur
//! `PKEY_AppUserModel_ID` seul — ce que laissait croire la documentation
//! Windows — aurait laissé Terminal cassé, puisque sa fenêtre ne porte aucune
//! propriété d'identité.
//!
//! **Le cas « File Explorer », qui est à part.** Ses fenêtres ne portent NI
//! propriété de fenêtre NI identité de paquet — mesuré, son magasin de
//! propriétés est même entièrement VIDE — donc les deux sources ci-dessus sont
//! muettes, et aucune API ne rend l'identité implicite (`Microsoft.Windows.
//! Explorer`) que Windows utilise en interne pour la regrouper : sa
//! documentation ne mentionne nulle part cette valeur, qui n'est lisible que
//! parce que Windows la DÉCLARE sur son propre raccourci épinglé. Cette épingle
//! tire donc son identité de ce qu'elle déclare elle-même, et de rien d'autre —
//! voir `win32::taskbar_pins::process_name_from_icon_path`.
//!
//! D'où `DESKTOP_CLASSES`, juste en dessous : dès lors qu'une épingle peut être
//! associée à `explorer.exe`, le BUREAU devient une fenêtre comme une autre
//! pour ce module, et il l'aurait été en permanence.

use std::path::Path;

use windows::core::{BOOL, GUID, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HANDLE, HWND, LPARAM, MAX_PATH, PROPERTYKEY,
};
use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetClassNameW, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW, InternalGetWindowText,
    GetWindowTextW, GetWindowThreadProcessId, GWL_EXSTYLE, GW_OWNER, IsWindowVisible,
    WS_EX_TOOLWINDOW,
};

/// `PKEY_AppUserModel_ID` = `{9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}`, propriété 5.
///
/// Reconstituée ici plutôt qu'importée : `windows` ne l'expose que dans la
/// fonctionnalité `Win32_Storage_EnhancedStorage`, qui n'a rien à voir — même
/// convention que `win32::audio` pour `PKEY_Device_FriendlyName`.
const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 5,
};

/// Les trois autres propriétés « Relaunch » du même jeu (`PKEY_AppUserModel_…`),
/// posées par les navigateurs Chromium sur CHAQUE fenêtre — MESURÉ le 2026-09-25
/// sur Chrome (trois profils) et Brave :
/// - 2, `RelaunchCommand` : `chrome.exe --profile-directory="Profile 1"` ;
/// - 3, `RelaunchIconResource` : `…\User Data\Profile 1\Google Profile.ico,0`.
///
/// C'est ce que lit la barre des tâches pour séparer les profils (voir
/// `BrowserProfile`).
const PKEY_APP_USER_MODEL_RELAUNCH_COMMAND: PROPERTYKEY = PROPERTYKEY {
    fmtid: PKEY_APP_USER_MODEL_ID.fmtid,
    pid: 2,
};
const PKEY_APP_USER_MODEL_RELAUNCH_ICON: PROPERTYKEY = PROPERTYKEY {
    fmtid: PKEY_APP_USER_MODEL_ID.fmtid,
    pid: 3,
};

/// Préfixe des clés d'icône dynamique qui désignent un PROFIL de navigateur
/// plutôt qu'un exécutable — voir `OpenWindow::dynamic_key`.
const PROFILE_KEY_PREFIX: &str = "profil:";

/// **Un profil de navigateur AUTRE que celui par défaut** — DEMANDE EXPLICITE
/// (2026-09-25) : « que les fenêtres des navigateurs soient rattachées par profil
/// comme le fait Windows ».
///
/// MESURÉ : chaque profil Chromium pose sur ses fenêtres son propre AUMID
/// (`Chrome`, `Chrome.UserData.Profile1`, `Chrome.UserData.Profile2`), une
/// commande de relance `--profile-directory=…` et une icône `Google Profile.ico`
/// — qui est DÉJÀ le logo du navigateur badgé de l'avatar du profil, celle que
/// montre la barre des tâches.
///
/// Le profil `Default` n'en est pas un ici : ses fenêtres restent celles du
/// navigateur, rejointes par son épingle, comme dans la barre des tâches.
///
/// Firefox ne déclare rien de tout cela : son profil se lit dans le TITRE
/// (voir `win32::firefox_profiles`), et son icône se compose d'un avatar.
#[derive(Clone, Debug, PartialEq)]
pub struct BrowserProfile {
    /// Identité stable du profil, clé de son icône (voir
    /// `OpenWindow::dynamic_key`) — l'AUMID de ses fenêtres pour Chromium.
    pub id: String,
    /// Arguments qui ouvrent une fenêtre DANS ce profil (le clic droit) —
    /// `None` : relancer le navigateur tel quel, donc son sélecteur de profils
    /// (Firefox, voir `win32::firefox_profiles::window_profile`).
    pub launch_args: Option<String>,
    /// Fichier `.ico` du profil, `None` si la ressource n'en désigne pas un.
    pub icon: Option<String>,
    /// Avatar à poser sur l'icône du navigateur, faute de `.ico` (Firefox).
    pub badge: Option<super::firefox_profiles::ProfileBadge>,
}

/// **FONCTION PURE, ET TESTÉE** : le dossier de profil d'une commande de relance
/// — `None` sans `--profile-directory`, ou pour le profil `Default`.
fn profile_directory(relaunch_command: &str) -> Option<String> {
    let (_, rest) = relaunch_command.split_once("--profile-directory=")?;
    let directory = match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next().unwrap_or_default(),
        None => rest.split_whitespace().next().unwrap_or_default(),
    };
    (!directory.is_empty() && !directory.eq_ignore_ascii_case("Default")).then(|| directory.to_string())
}

/// **FONCTION PURE, ET TESTÉE** : le fichier `.ico` d'une ressource d'icône
/// (`chemin,index`) — `None` si elle désigne autre chose (une icône dans un
/// exécutable, qu'on sait déjà extraire autrement).
fn icon_file(resource: &str) -> Option<String> {
    let path = match resource.rsplit_once(',') {
        Some((path, index)) if index.trim().parse::<i32>().is_ok() => path,
        _ => resource,
    };
    let path = path.trim().trim_matches('"');
    path.to_ascii_lowercase().ends_with(".ico").then(|| path.to_string())
}

/// **L'HÔTE des applications empaquetées** (UWP/Store) : sa fenêtre porte le
/// HWND, mais l'application affichée est une AUTRE.
///
/// **POURQUOI CE N'EST PAS UN PROCESSUS D'INFRASTRUCTURE**, contrairement à ce
/// que la liste `app::WINDOWS_INFRASTRUCTURE_EXES` a longtemps supposé —
/// MESURÉ le 2026-09-20 (sonde `prints_the_identity_of_a_packaged_app_frame`) :
/// la fenêtre des Paramètres de Windows franchit **toutes** les règles de
/// `collect_window` (visible, titrée « Paramètres », sans propriétaire, sans
/// `WS_EX_TOOLWINDOW`), et son exécutable réel — `SystemSettings.exe`,
/// pid 40392 — n'a **aucune fenêtre à lui** (`MainWindowHandle` = 0). Traiter
/// l'hôte comme de l'infrastructure rendait donc **invisible toute application
/// du Store** dans la liste des programmes ouverts du dock.
///
/// Les deux autres fenêtres du même hôte (« MSCTFIME UI », « Default IME »)
/// sont, elles, déjà écartées par `GW_OWNER` et `IsWindowVisible` — mesuré
/// aussi. Retirer l'exclusion par nom ne fait donc remonter que les vraies
/// frames d'application.
pub(crate) const PACKAGED_APP_HOST_EXE: &str = "ApplicationFrameHost.exe";

/// Classe de la fenêtre qui porte l'application hébergée, sous la frame de
/// l'hôte — mesurée sur les Paramètres de Windows. C'est le descendant dont le
/// processus est celui de l'application (`SystemSettings.exe`), et dont le
/// titre est celui de la frame.
///
/// Sert de **préférence**, pas de critère unique : `pick_hosted_child` se
/// rabat sur « le premier descendant d'un autre processus » si cette classe
/// n'est pas là, pour ne pas dépendre d'un détail d'implémentation de Windows.
const HOSTED_APP_WINDOW_CLASS: &str = "Windows.UI.Core.CoreWindow";

/// **L'application qu'on cherche**, telle qu'une épingle la décrit.
///
/// Deux formes seulement, parce qu'il n'y a que deux façons de savoir qui est
/// qui — et le choix entre les deux se fait à la LECTURE de l'épingle, pas
/// ici : un raccourci `.lnk` donne un exécutable, une épingle du Store donne
/// un AppUserModelID (voir `win32::taskbar_pins`).
#[derive(Clone)]
pub enum WindowTarget {
    /// Nom de fichier de l'exécutable (`brave.exe`), insensible à la casse.
    Process(String),
    /// AppUserModelID, comparé aux DEUX identités de la fenêtre — voir
    /// `OpenWindow::matches`.
    Aumid(String),
    /// Identité d'un profil de navigateur (`BrowserProfile::id`) — la cible
    /// d'une icône de profil, jamais d'une épingle.
    Profile(String),
}

impl WindowTarget {
    pub fn process(name: impl Into<String>) -> Self {
        Self::Process(name.into())
    }

    pub fn aumid(aumid: impl Into<String>) -> Self {
        Self::Aumid(aumid.into())
    }
}

/// Une fenêtre de premier niveau actuellement ouverte, avec les identités
/// qu'elle expose. Relevée par `open_windows`.
pub struct OpenWindow {
    pub hwnd: HWND,
    /// Titre de la fenêtre — étiquette de sa vignette d'aperçu (voir
    /// `win32::window_previews`), et ce qui permet à l'utilisateur de
    /// distinguer deux fenêtres de la même application.
    pub title: String,
    /// Nom de fichier de l'exécutable **de l'application que cette fenêtre
    /// AFFICHE** — pas forcément le processus propriétaire du `hwnd` : une
    /// fenêtre d'hôte d'applications empaquetées porte l'exécutable d'un AUTRE
    /// processus (voir `PACKAGED_APP_HOST_EXE` et `hosted_app_identity`).
    pub exe: String,
    /// Chemin COMPLET de cet exécutable — sert à extraire son icône pour
    /// l'icône d'une application ouverte mais non épinglée (voir
    /// `app::unlisted_open_apps`), que le seul nom de fichier ne permet pas.
    /// Comme `exe`, il décrit l'application affichée, jamais l'hôte.
    pub exe_path: String,
    /// AUMID déclaré par le processus POUR CETTE FENÊTRE — voir la doc de
    /// module. `None` quand la propriété est absente (cas de Terminal).
    ///
    /// **Déjà correct pour une fenêtre d'hôte**, et c'est mesuré : l'hôte pose
    /// sur sa fenêtre l'AUMID de l'application qu'il affiche (les Paramètres y
    /// déclarent `windows.immersivecontrolpanel_…`), alors que son propre
    /// processus n'est pas empaqueté.
    pub window_aumid: Option<String>,
    /// AUMID de l'identité de PAQUET du processus propriétaire — voir la doc
    /// de module. `None` dès que le processus n'est pas empaqueté ; pour une
    /// fenêtre d'hôte, c'est celui de l'application HÉBERGÉE (l'hôte, lui,
    /// n'étant pas empaqueté, n'en a aucun — voir `hosted_app_identity`).
    pub process_aumid: Option<String>,
    /// Profil de navigateur NON par défaut — voir `BrowserProfile`. Jamais sans
    /// `window_aumid` : c'est lui qui désigne le profil.
    pub profile: Option<BrowserProfile>,
}

impl OpenWindow {
    /// Vrai si cette fenêtre appartient à `target`.
    ///
    /// `Process` : égalité de nom de fichier, comme avant ce module.
    ///
    /// `Aumid` : l'une **OU** l'autre des deux identités suffit. C'est le cœur
    /// du module — mesuré, Terminal n'expose que l'identité de processus et
    /// Chrome que celle de fenêtre ; exiger les deux, ou n'en regarder qu'une,
    /// casse l'un des deux.
    /// Vrai si cette fenêtre DÉCLARE cet identifiant d'application — l'une **OU**
    /// l'autre de ses deux identités (voir `matches`, qui délègue ici).
    ///
    /// Existe pour que l'appelant puisse éprouver un identifiant SANS allouer un
    /// `WindowTarget::Aumid` par couple (fenêtre × identifiant) : c'est le cas de
    /// la résolution des notifications (voir `win32::notification`), qui confronte
    /// chaque identifiant non lu à toutes les fenêtres ouvertes.
    pub fn matches_aumid(&self, aumid: &str) -> bool {
        self.window_aumid.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(aumid))
            || self.process_aumid.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(aumid))
    }

    /// `Process` ne rejoint PAS les fenêtres d'un profil de navigateur : elles
    /// ont leur propre icône (voir `BrowserProfile`), et l'épingle `chrome.exe`
    /// les avalerait toutes — pastille, aperçus et bascule compris.
    pub fn matches(&self, target: &WindowTarget) -> bool {
        match target {
            WindowTarget::Process(name) => self.exe.eq_ignore_ascii_case(name) && self.profile.is_none(),
            WindowTarget::Aumid(aumid) => self.matches_aumid(aumid),
            WindowTarget::Profile(id) => self.profile.as_ref().is_some_and(|p| p.id == *id),
        }
    }
}

impl OpenWindow {
    /// **La clé d'icône dynamique** de cette fenêtre (voir
    /// `app::unlisted_open_apps`) : son exécutable, ou pour un profil de
    /// navigateur son identité préfixée — une icône PAR PROFIL, comme la barre
    /// des tâches.
    pub fn dynamic_key(&self) -> String {
        match &self.profile {
            Some(profile) => format!("{PROFILE_KEY_PREFIX}{}", profile.id),
            None => self.exe.clone(),
        }
    }

    /// Vrai si cette fenêtre relève de l'icône dynamique `key`.
    pub fn has_dynamic_key(&self, key: &str) -> bool {
        self.matches(&dynamic_target(key))
    }
}

/// La cible d'une clé d'icône dynamique (voir `OpenWindow::dynamic_key`).
pub fn dynamic_target(key: &str) -> WindowTarget {
    match key.strip_prefix(PROFILE_KEY_PREFIX) {
        Some(id) => WindowTarget::Profile(id.to_string()),
        None => WindowTarget::process(key),
    }
}

/// Vrai si la clé désigne un profil de navigateur.
pub fn is_profile_key(key: &str) -> bool {
    key.starts_with(PROFILE_KEY_PREFIX)
}

/// Vrai si `target` a au moins une fenêtre dans `windows` — le prédicat que le
/// dock évalue une fois par seconde et par icône.
///
/// Prend l'instantané en paramètre plutôt que de le construire : l'appelant en
/// fait UN pour toutes les icônes (voir `app::run`), là où un `open_windows()`
/// par icône multiplierait l'énumération par le nombre d'épingles.
pub fn is_open(windows: &[OpenWindow], target: &WindowTarget) -> bool {
    windows.iter().any(|w| w.matches(target))
}

/// Énumère les fenêtres de premier niveau ouvertes, avec leurs identités.
///
/// **Coût** : un `OpenProcess` + un `SHGetPropertyStoreForWindow` par fenêtre.
/// À appeler à cadence lente (une fois par seconde dans `app::run`), jamais par
/// frame — d'où la forme « instantané » plutôt qu'une recherche par
/// application.
///
/// Mêmes filtres que la recherche d'origine : fenêtre visible et portant un
/// titre. Une fenêtre d'application réelle a toujours les deux, et c'est ce qui
/// écarte les innombrables fenêtres utilitaires de Windows.
pub fn open_windows() -> Vec<OpenWindow> {
    let mut windows: Vec<OpenWindow> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect_window), LPARAM(&mut windows as *mut _ as isize));
    }
    windows
}

/// UNE fenêtre, passée aux mêmes filtres que `open_windows` — `None` si elle
/// n'est pas une fenêtre d'application (voir `win32::ouverture_fenetres`, qui
/// cherche l'icône d'une fenêtre qui s'ouvre).
pub fn open_window(hwnd: HWND) -> Option<OpenWindow> {
    let mut windows: Vec<OpenWindow> = Vec::new();
    unsafe {
        let _ = collect_window(hwnd, LPARAM(&mut windows as *mut _ as isize));
    }
    windows.pop()
}

/// Classes du BUREAU — le fond de l'écran, qui n'est pas une fenêtre
/// d'application.
///
/// MESURÉ, et c'est ce qui a motivé cette exclusion : `Progman` (« Program
/// Manager ») est VISIBLE et PORTE UN TITRE, donc elle passait le filtre
/// « visible + titrée » ci-dessous. Or elle appartient à `explorer.exe`, qui
/// tourne en permanence : dès que l'épingle « File Explorer » a reçu une
/// identité (voir `win32::taskbar_pins`), le bureau l'aurait comptée comme une
/// fenêtre ouverte et la pastille aurait été ALLUMÉE EN PERMANENCE, sans rien
/// apprendre à personne. `WorkerW` est l'autre classe du même fond d'écran.
///
/// Une fenêtre de dossier (`CabinetWClass`) reste, elle, une vraie fenêtre :
/// c'est exactement ce que la pastille doit signaler.
const DESKTOP_CLASSES: [&str; 2] = ["Progman", "WorkerW"];

/// Vrai si `hwnd` est le fond de l'écran — voir `DESKTOP_CLASSES`.
pub fn is_desktop_window(hwnd: HWND) -> bool {
    let mut buffer = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    if len <= 0 {
        return false;
    }
    let class = String::from_utf16_lossy(&buffer[..len as usize]);
    DESKTOP_CLASSES.iter().any(|desktop| class.eq_ignore_ascii_case(desktop))
}

/// **FONCTION PURE, ET TESTÉE** : parmi les descendants d'une frame d'hôte,
/// choisit celui qui porte l'application hébergée, et rend son INDEX.
///
/// Deux règles, dans cet ordre :
/// 1. un descendant de la CLASSE connue (voir `HOSTED_APP_WINDOW_CLASS`) qui
///    n'appartient PAS au processus de la frame ;
/// 2. à défaut, le premier descendant d'un AUTRE processus.
///
/// La seconde règle n'est pas un doublon de la première : la frame peuple ses
/// descendants directs (`ApplicationFrameTitleBarWindow`,
/// `ApplicationFrameInputSinkWindow` — mesurés), donc « un autre processus »
/// est le seul critère qui survive à un changement de classe côté Windows.
///
/// Reçoit des `(classe, pid)` et non des `HWND` : la décision ne dépend que de
/// ces deux valeurs, ce qui la rend éprouvable sans ouvrir la moindre fenêtre.
fn pick_hosted_child(children: &[(&str, u32)], frame_pid: u32) -> Option<usize> {
    let known = children.iter().position(|(class, pid)| {
        *pid != frame_pid && class.eq_ignore_ascii_case(HOSTED_APP_WINDOW_CLASS)
    });
    known.or_else(|| children.iter().position(|(_, pid)| *pid != frame_pid))
}

/// Un descendant de la frame, relevé par `EnumChildWindows` — juste ce qu'il
/// faut pour trancher, jamais conservé au-delà de `hosted_app_identity`.
struct HostedChild {
    class: String,
    pid: u32,
}

unsafe extern "system" fn collect_child(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let children = &mut *(lparam.0 as *mut Vec<HostedChild>);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return BOOL(1);
    }
    let mut buffer = [0u16; 256];
    let len = GetClassNameW(hwnd, &mut buffer);
    children.push(HostedChild {
        class: String::from_utf16_lossy(&buffer[..len.max(0) as usize]),
        pid,
    });
    BOOL(1)
}

/// **L'identité de l'application RÉELLEMENT affichée** par une fenêtre d'hôte
/// d'applications empaquetées : `(exe, exe_path, process_aumid)`, les trois
/// pris sur le MÊME processus — celui du descendant porteur.
///
/// **POURQUOI LES TROIS ENSEMBLE, ET PAS EN DÉSORDRE.** `exe` est la clé de
/// tout le pipeline des icônes dynamiques : dédup, ordre d'affichage,
/// reconstruction de la liste, extraction de la texture et lancement. Un `exe`
/// venu de l'application hébergée avec un `exe_path` resté celui de l'hôte
/// afficherait l'icône de l'hôte ; avec un `process_aumid` resté celui de
/// l'hôte, il relancerait `ApplicationFrameHost.exe`. Les trois sortent donc de
/// la même résolution — c'est la raison d'être de cette fonction.
///
/// **L'AUMID DE LA FENÊTRE N'EST PAS LU ICI, ET C'EST MESURÉ.** La frame porte
/// DÉJÀ `PKEY_AppUserModel_ID` de l'application hébergée — lu sur la fenêtre
/// des Paramètres : `windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.
/// windows.immersivecontrolpanel` — alors que son processus, lui, n'est pas
/// empaqueté (`process_aumid(pid_frame)` = `None`). `collect_window` garde donc
/// `window_aumid(hwnd)` tel quel : cette lecture est juste, et gratuite.
///
/// **`None` veut dire « on ne sait pas qui c'est »**, et l'appelant ÉCARTE
/// alors la fenêtre plutôt que de lui laisser le nom de l'hôte — voir
/// `collect_window`, et pourquoi le nom de l'hôte serait pire que l'absence.
fn hosted_app_identity(frame_hwnd: HWND, frame_pid: u32) -> Option<(String, String, Option<String>)> {
    unsafe {
        let mut children: Vec<HostedChild> = Vec::new();
        let _ = EnumChildWindows(
            Some(frame_hwnd),
            Some(collect_child),
            LPARAM(&mut children as *mut _ as isize),
        );
        let candidates: Vec<(&str, u32)> =
            children.iter().map(|child| (child.class.as_str(), child.pid)).collect();
        let hosted_pid = match pick_hosted_child(&candidates, frame_pid) {
            Some(i) => children[i].pid,
            None => pid_detache(frame_hwnd)?,
        };
        let (exe, exe_path) = process_image_name(hosted_pid)?;
        Some((exe, exe_path, process_aumid(hosted_pid)))
    }
}

/// ⭐ **UNE APPLICATION DU STORE RÉDUITE N'A PLUS D'ENFANT DANS SA FRAME** —
/// DÉFAUT SIGNALÉ (2026-10-24) : « si la fenêtre des Paramètres de Windows est
/// réduite je n'arrive pas à la faire réapparaître depuis son icône ». Mesuré par
/// `sonde_fenetres_du_store_reduites`, Paramètres réduits :
///
/// - la frame (`ApplicationFrameWindow`, réduite) ne garde que ses deux barres de
///   titre et son `InputSink` — **le `CoreWindow` en est sorti** ;
/// - le `CoreWindow` devient une fenêtre de PREMIER NIVEAU de
///   `SystemSettings.exe`, visible, titrée, et **voilée par le shell**
///   (`DWMWA_CLOAKED` = 2).
///
/// D'où le défaut, en deux temps : la frame n'avait plus d'identité (écartée), et
/// le `CoreWindow` détaché passait pour LA fenêtre des Paramètres — le clic
/// activait une fenêtre voilée. `collect_window` écarte désormais le second, et la
/// frame retrouve son application ICI : son `PKEY_AppUserModel_ID` (que la frame
/// porte, voir plus haut) est l'AUMID du processus du `CoreWindow` détaché.
fn pid_detache(frame_hwnd: HWND) -> Option<u32> {
    let aumid = window_identity(frame_hwnd).0?;
    let mut detaches: Vec<HWND> = Vec::new();
    unsafe extern "system" fn core_windows(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if est_core_window(hwnd) {
            (*(lparam.0 as *mut Vec<HWND>)).push(hwnd);
        }
        BOOL(1)
    }
    unsafe {
        let _ = EnumWindows(Some(core_windows), LPARAM(&mut detaches as *mut _ as isize));
    }
    detaches.into_iter().find_map(|hwnd| {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        (pid != 0 && process_aumid(pid).is_some_and(|a| a.eq_ignore_ascii_case(&aumid))).then_some(pid)
    })
}

/// La fenêtre d'une application du Store, `Windows.UI.Core.CoreWindow`.
fn est_core_window(hwnd: HWND) -> bool {
    let mut buffer = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize]).eq_ignore_ascii_case(HOSTED_APP_WINDOW_CLASS)
}

unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let windows = &mut *(lparam.0 as *mut Vec<OpenWindow>);

    // ⚠️ **`InternalGetWindowText`, JAMAIS `GetWindowTextLengthW`** : ce dernier
    // ENVOIE `WM_GETTEXTLENGTH` et attend la réponse. Une seule fenêtre figée
    // (d'un autre programme, ou d'un de nos fils occupé) suspendait ainsi la
    // boucle principale — c'étaient les « NeuraDesk ne répond pas » du
    // 2026-10-18, relevés par `win32::chien_de_garde`. Celui-ci lit le titre que
    // Windows garde, sans rien envoyer.
    let mut title_buffer = [0u16; 512];
    let title_len = InternalGetWindowText(hwnd, &mut title_buffer);
    if !IsWindowVisible(hwnd).as_bool() || title_len <= 0 {
        return BOOL(1);
    }
    if is_desktop_window(hwnd) {
        return BOOL(1);
    }
    // **LES DEUX RÈGLES DE LA BARRE DES TÂCHES.** DEFECTS RÉELS SIGNALÉS : sans
    // elles, WhatsApp montrait DEUX fenêtres d'aperçu (son hôte et une fenêtre
    // d'aide interne — c'est une application WebView2). Windows n'inscrit dans
    // Alt-Tab que les fenêtres SANS PROPRIÉTAIRE (`GW_OWNER` : écarte les
    // dialogues et les écrans d'ouverture) ET SANS `WS_EX_TOOLWINDOW` (écarte
    // les fenêtres d'aide, dont celles de CEF et de `msedgewebview2.exe`).
    //
    // ⚠️ NE PAS remplacer par un filtre par PID, par classe, ou par « fenêtre
    // cachée » : le rectangle blanc de `capture_visibility` venait d'un filtre
    // par PID, et « fenêtre cachée » casserait les popups ouverts avant la
    // bascule.
    //
    // ⚠️ **CE FILTRE N'EST PAS LA CAUSE DE L'ABSENCE DE GIMP, ET LA MESURE A
    // CORRIGÉ CE QUI ÉTAIT ÉCRIT ICI.** Sa fenêtre principale
    // (`gdkWindowToplevel`) n'a ni propriétaire ni `WS_EX_TOOLWINDOW`, elle est
    // absente des aperçus AVANT comme APRÈS ce filtre — mais surtout elle
    // **FRANCHIT cette porte** : instrumenté, `collect_window` la RETIENT, et
    // elle figure aussi bien dans `open_windows` que dans la carte des bureaux.
    // La version précédente de ce commentaire affirmait que la perte venait
    // « d'ailleurs, en amont » : c'était FAUX, et ça a coûté une hypothèse de
    // plus. La perte est en AVAL — résolution de cible du côté de l'aperçu du
    // dock, rendu de la vignette du côté de la mini-carte. Ne pas rouvrir la
    // piste du filtre.
    if unsafe { GetWindow(hwnd, GW_OWNER) }.map(|owner| !owner.0.is_null()).unwrap_or(false) {
        return BOOL(1);
    }
    if unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOOLWINDOW.0 != 0 {
        return BOOL(1);
    }
    // ⛔ Un `CoreWindow` de PREMIER NIVEAU est celui d'une application du Store
    // RÉDUITE, sorti de sa frame et voilé : ce n'est jamais lui que la barre des
    // tâches montre, c'est la frame (voir `pid_detache`).
    if est_core_window(hwnd) {
        return BOOL(1);
    }
    let title = String::from_utf16_lossy(&title_buffer[..title_len.max(0) as usize]);

    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return BOOL(1);
    }
    // Sans nom d'exécutable, la fenêtre n'est de toute façon identifiable par
    // aucune des deux formes de `WindowTarget` — on l'écarte plutôt que de
    // garder une entrée inerte.
    let Some((exe, exe_path)) = process_image_name(pid) else {
        return BOOL(1);
    };

    // **Une fenêtre d'HÔTE porte l'application d'un AUTRE processus** (voir
    // `PACKAGED_APP_HOST_EXE`) : on lui substitue l'identité de l'application
    // hébergée, en gardant CE HWND — mesuré comme étant le bon : c'est la seule
    // vraie fenêtre de premier plan (l'activer est ce que fait la barre des
    // tâches ; activer l'enfant ne marcherait pas), et c'est elle qui porte le
    // titre et l'`PKEY_AppUserModel_ID` de l'application.
    //
    // **ÉCHEC ⇒ ON ÉCARTE, on ne retombe PAS sur le nom de l'hôte.** `exe` est
    // à la fois la clé de dédup, la clé d'ordre, la clé d'extraction d'icône et
    // la clé de lancement des icônes dynamiques (`app::unlisted_open_apps`,
    // `app::run`) : rendre « ApplicationFrameHost.exe » ferait **une seule
    // icône pour toutes les applications du Store**, portant l'icône de l'hôte.
    // Rendre `None` fait disparaître la fenêtre une ou deux secondes, le temps
    // que le descendant de l'application soit créé — borné, et sans oscillation
    // entre deux noms, qui relancerait le rechargement de toutes les textures.
    let (exe, exe_path, process_aumid) = if exe.eq_ignore_ascii_case(PACKAGED_APP_HOST_EXE) {
        let Some((hosted_exe, hosted_path, hosted_aumid)) = hosted_app_identity(hwnd, pid) else {
            return BOOL(1);
        };
        (hosted_exe, hosted_path, hosted_aumid)
    } else {
        let aumid = process_aumid(pid);
        (exe, exe_path, aumid)
    };

    let (window_aumid, profile) = window_identity(hwnd);
    let profile = profile.or_else(|| super::firefox_profiles::window_profile(&exe, &title));
    windows.push(OpenWindow {
        hwnd,
        title,
        exe,
        exe_path,
        window_aumid,
        process_aumid,
        profile,
    });
    BOOL(1)
}

/// AppUserModelID déclaré par le processus POUR CETTE FENÊTRE
/// (`PKEY_AppUserModel_ID`).
///
/// `None` quand la propriété est absente — ce qui est le cas de la plupart des
/// applications, y compris empaquetées : c'est une déclaration FACULTATIVE
/// (`SetCurrentProcessExplicitAppUserModelID`), que Chrome ou Brave font et que
/// Terminal ne fait pas. Voir `process_aumid` pour l'autre source.
#[cfg(test)]
fn window_aumid(hwnd: HWND) -> Option<String> {
    window_identity(hwnd).0
}

/// `window_aumid` et le profil de navigateur (voir `BrowserProfile`), lus sur
/// le MÊME magasin de propriétés — un seul `SHGetPropertyStoreForWindow` par
/// fenêtre et par seconde.
fn window_identity(hwnd: HWND) -> (Option<String>, Option<BrowserProfile>) {
    let Ok(store) = (unsafe { SHGetPropertyStoreForWindow::<IPropertyStore>(hwnd) }) else {
        return (None, None);
    };
    // `GetValue` réussit en rendant un PROPVARIANT VIDE (`VT_EMPTY`) quand
    // la propriété n'existe pas — l'absence se lit donc sur la chaîne, pas
    // sur le `Result`. Mesuré : c'est le cas de 20 fenêtres sur 20 ici,
    // sauf Chrome, Brave et Steam.
    let read = |key: &PROPERTYKEY| -> Option<String> {
        let value = unsafe { store.GetValue(key) }.ok()?.to_string();
        (!value.is_empty()).then_some(value)
    };
    let Some(aumid) = read(&PKEY_APP_USER_MODEL_ID) else {
        return (None, None);
    };
    let profile = read(&PKEY_APP_USER_MODEL_RELAUNCH_COMMAND)
        .and_then(|command| profile_directory(&command))
        .map(|directory| BrowserProfile {
            id: aumid.clone(),
            launch_args: Some(format!("--profile-directory=\"{directory}\"")),
            icon: read(&PKEY_APP_USER_MODEL_RELAUNCH_ICON).and_then(|resource| icon_file(&resource)),
            badge: None,
        });
    (Some(aumid), profile)
}

/// AppUserModelID de l'identité de PAQUET du processus `pid`
/// (`GetApplicationUserModelId`).
///
/// C'est la seule source qui connaisse une application du Store : Terminal
/// rend `Microsoft.WindowsTerminal_8wekyb3d8bbwe!App`, WhatsApp
/// `5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App`. Pour un processus NON
/// empaqueté, l'appel échoue proprement par `ERROR_NOT_FOUND` — ce n'est pas
/// une erreur à signaler, c'est la réponse normale (mesuré sur 17 fenêtres
/// sur 20), d'où un simple `None`.
fn process_aumid(pid: u32) -> Option<String> {
    unsafe {
        // `PROCESS_QUERY_LIMITED_INFORMATION` suffit, et c'est le niveau le
        // plus bas qui marche encore sur les processus d'autrui.
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let aumid = read_process_aumid(handle);
        let _ = CloseHandle(handle);
        aumid
    }
}

/// Le corps de `process_aumid`, une fois le processus ouvert.
///
/// L'appel se fait en DEUX temps : longueur seule, puis lecture — les
/// identifiants dépassent `MAX_PATH` (celui de Terminal fait 44 caractères,
/// mais rien ne garantit cette taille). Mesuré : le premier appel rend
/// `ERROR_INSUFFICIENT_BUFFER` avec la longueur attendue, jamais
/// `ERROR_SUCCESS`.
unsafe fn read_process_aumid(handle: HANDLE) -> Option<String> {
    let mut len: u32 = 0;
    if GetApplicationUserModelId(handle, &mut len, None) != ERROR_INSUFFICIENT_BUFFER || len == 0 {
        return None;
    }
    let mut buffer = vec![0u16; len as usize];
    if GetApplicationUserModelId(handle, &mut len, Some(PWSTR(buffer.as_mut_ptr()))) != ERROR_SUCCESS {
        return None;
    }
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let aumid = String::from_utf16_lossy(&buffer[..end]);
    (!aumid.is_empty()).then_some(aumid)
}

/// Nom de fichier (sans le chemin) de l'exécutable du processus `pid`, ou
/// `None` si son ouverture échoue (droits insuffisants, processus système,
/// terminé entre-temps...).
///
/// Venu de `win32::appicon`, qui s'en servait pour la même recherche — la
/// correspondance par nom d'exécutable est maintenant résolue ici.
pub(crate) fn process_image_name(pid: u32) -> Option<(String, String)> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; MAX_PATH as usize];
        let mut len = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(handle);
        result.ok()?;
        let full_path = String::from_utf16_lossy(&buffer[..len as usize]);
        let name = Path::new(&full_path).file_name().map(|n| n.to_string_lossy().into_owned())?;
        Some((name, full_path))
    }
}

/// **QUI POSSÈDE CETTE FENÊTRE — nom de l'exécutable, et privilège.**
///
/// Écrit pour NOMMER la cause d'un `SetWindowPos` qui échoue en « Accès
/// refusé » : `SetWindowPos` envoie `WM_WINDOWPOSCHANGING` à la fenêtre visée,
/// et la protection contre l'usurpation d'interface (UIPI) interdit à un
/// processus d'intégrité moyenne d'écrire à un processus plus privilégié. Le
/// contraste est parlant : sur cette machine, des menus appartenant à
/// `explorer.exe` se déplacent sans difficulté, et ceux qui refusent
/// appartiennent à une application élevée.
///
/// Le second champ vaut `None` quand le jeton n'a pas pu être ouvert, et jamais
/// `Some(false)` par défaut : ce serait affirmer une mesure qui n'a pas eu lieu.
pub(crate) fn window_owner(hwnd: HWND) -> (String, Option<bool>) {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let name = process_image_name(pid)
        .map(|(name, _)| name)
        .unwrap_or_else(|| format!("pid {pid}"));
    (name, process_is_elevated(pid))
}

/// `Some(true)` si le processus tourne élevé, `None` si son jeton est illisible.
fn process_is_elevated(pid: u32) -> Option<bool> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut token = HANDLE::default();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token);
        let _ = CloseHandle(process);
        opened.ok()?;
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let queried = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);
        queried.ok()?;
        Some(elevation.TokenIsElevated != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

    /// Les commandes de relance MESURÉES le 2026-09-25 (Chrome, Brave).
    #[test]
    fn profile_directory_reads_chromium_relaunch_commands() {
        let chrome = r#""C:\Program Files\Google\Chrome\Application\chrome.exe""#;
        assert_eq!(profile_directory(&format!(r#"{chrome} --profile-directory="Profile 1""#)), Some("Profile 1".into()));
        assert_eq!(profile_directory(&format!("{chrome} --profile-directory=Profile2 --x")), Some("Profile2".into()));
        assert_eq!(profile_directory(&format!("{chrome} --profile-directory=Default")), None);
        assert_eq!(profile_directory(chrome), None);
        assert_eq!(profile_directory("--profile-directory="), None);
    }

    #[test]
    fn icon_file_keeps_only_ico_files() {
        assert_eq!(
            icon_file(r"C:\Users\a\User Data\Profile 1\Google Profile.ico,0"),
            Some(r"C:\Users\a\User Data\Profile 1\Google Profile.ico".into())
        );
        assert_eq!(icon_file(r"C:\x\a.ICO"), Some(r"C:\x\a.ICO".into()));
        assert_eq!(icon_file(r"C:\x\chrome.exe,-101"), None);
    }

    fn window(exe: &str, aumid: Option<&str>, profile: bool) -> OpenWindow {
        OpenWindow {
            hwnd: HWND::default(),
            title: String::new(),
            exe: exe.into(),
            exe_path: String::new(),
            window_aumid: aumid.map(Into::into),
            process_aumid: None,
            profile: profile.then(|| BrowserProfile {
                id: aumid.unwrap_or_default().into(),
                launch_args: None,
                icon: None,
                badge: None,
            }),
        }
    }

    /// Un profil a SA clé, et l'épingle de l'exécutable ne l'avale plus.
    #[test]
    fn a_browser_profile_is_its_own_dynamic_icon() {
        let default = window("chrome.exe", Some("Chrome"), false);
        let profile = window("chrome.exe", Some("Chrome.UserData.Profile1"), true);
        assert_eq!(default.dynamic_key(), "chrome.exe");
        let key = profile.dynamic_key();
        assert!(is_profile_key(&key));
        assert!(profile.has_dynamic_key(&key));
        assert!(!default.has_dynamic_key(&key));
        assert!(!profile.has_dynamic_key("chrome.exe"));
        assert!(!profile.matches(&WindowTarget::process("chrome.exe")));
        assert!(profile.matches(&WindowTarget::aumid("Chrome.UserData.Profile1")));
        assert!(profile.matches(&WindowTarget::Profile("Chrome.UserData.Profile1".into())));
    }

    /// Nom de classe d'une fenêtre, tronqué à 256 caractères — largement
    /// au-delà de tout ce que Windows enregistre.
    fn class_name(hwnd: HWND) -> String {
        let mut buffer = [0u16; 256];
        let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
        String::from_utf16_lossy(&buffer[..len.max(0) as usize])
    }

    /// Titre d'une fenêtre (chaîne vide s'il n'y en a pas).
    fn window_title(hwnd: HWND) -> String {
        let len = unsafe { GetWindowTextLengthW(hwnd) };
        if len <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; len as usize + 1];
        let len = unsafe { GetWindowTextW(hwnd, &mut buffer) };
        String::from_utf16_lossy(&buffer[..len.max(0) as usize])
    }

    unsafe extern "system" fn gather_frames(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = &mut *(lparam.0 as *mut Vec<HWND>);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return BOOL(1);
        }
        if let Some((exe, _)) = process_image_name(pid) {
            if exe.eq_ignore_ascii_case(PACKAGED_APP_HOST_EXE) {
                out.push(hwnd);
            }
        }
        BOOL(1)
    }

    struct ChildInfo {
        class: String,
        pid: u32,
        exe: Option<String>,
        title: String,
    }

    unsafe extern "system" fn gather_children(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = &mut *(lparam.0 as *mut Vec<ChildInfo>);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        out.push(ChildInfo {
            class: class_name(hwnd),
            pid,
            exe: process_image_name(pid).map(|(name, _)| name),
            title: window_title(hwnd),
        });
        BOOL(1)
    }

    /// Les descendants RÉELS d'une frame des Paramètres, tels que MESURÉS le
    /// 2026-09-20 : la barre de titre et le puits d'entrée appartiennent à la
    /// frame, seul le `Windows.UI.Core.CoreWindow` appartient à l'application
    /// (`SystemSettings.exe`).
    const MEASURED_CHILDREN: &[(&str, u32)] = &[
        ("ApplicationFrameTitleBarWindow", 48748),
        ("Windows.UI.Core.CoreWindow", 40392),
        ("ApplicationFrameTitleBarWindow", 48748),
        ("ApplicationFrameInputSinkWindow", 48748),
    ];

    #[test]
    fn picks_the_known_class_from_another_process() {
        assert_eq!(pick_hosted_child(MEASURED_CHILDREN, 48748), Some(1));
    }

    /// La préférence de CLASSE l'emporte sur la POSITION : un descendant d'un
    /// autre processus rencontré plus tôt ne doit pas voler la place du
    /// `CoreWindow`, qui est le porteur identifié.
    #[test]
    fn prefers_the_known_class_over_an_earlier_other_process() {
        let children = &[
            ("ApplicationFrameTitleBarWindow", 48748),
            ("UneFenetreDUnAutreProcessus", 999),
            ("Windows.UI.Core.CoreWindow", 40392),
        ];
        assert_eq!(pick_hosted_child(children, 48748), Some(2));
    }

    /// Repli : sans la classe connue, « un autre processus » suffit — c'est ce
    /// qui évite de dépendre d'un détail d'implémentation de Windows.
    #[test]
    fn falls_back_to_the_first_other_process() {
        let children = &[("ApplicationFrameTitleBarWindow", 48748), ("UneClasseInconnue", 40392)];
        assert_eq!(pick_hosted_child(children, 48748), Some(1));
    }

    /// Aucun descendant d'un autre processus : on ne devine pas, on rend
    /// `None` — et `collect_window` écarte alors la fenêtre plutôt que de lui
    /// laisser le nom de l'hôte.
    #[test]
    fn refuses_to_guess_when_every_child_belongs_to_the_frame() {
        let children = &[
            ("ApplicationFrameTitleBarWindow", 48748),
            ("ApplicationFrameInputSinkWindow", 48748),
        ];
        assert_eq!(pick_hosted_child(children, 48748), None);
    }

    #[test]
    fn matches_the_known_class_case_insensitively() {
        let children = &[("windows.ui.core.corewindow", 40392)];
        assert_eq!(pick_hosted_child(children, 48748), Some(0));
    }

    /// **SONDE — et pourquoi elle RESTE dans le dépôt.**
    ///
    /// Un hôte d'applications empaquetées (`ApplicationFrameHost.exe`) affiche
    /// la fenêtre d'une AUTRE application : c'est lui qui possède le HWND, et
    /// les Paramètres de Windows (`SystemSettings.exe`) n'ont aucune fenêtre à
    /// eux — mesuré, `MainWindowHandle` = 0. Toute la question est de savoir
    /// **d'où vient l'identité de l'application réellement affichée**, et la
    /// réponse ne se déduit d'aucune documentation : les deux sources du
    /// module (`PKEY_AppUserModel_ID` sur la fenêtre, `GetApplicationUserModelId`
    /// sur le processus) sont lues sur l'HÔTE quand on les interroge sur lui.
    ///
    /// Ce que la sonde tranche :
    ///
    /// 1. **La frame franchit-elle les filtres de `collect_window` ?** Si elle
    ///    portait `WS_EX_TOOLWINDOW` ou un propriétaire, elle n'atteindrait
    ///    jamais `unlisted_open_apps` et l'exclusion par nom serait innocente —
    ///    tout le diagnostic serait à refaire. On imprime donc les DEUX règles
    ///    de la barre des tâches, pas seulement l'identité.
    /// 2. Quelle CLASSE porte l'enfant du vrai processus (le candidat à viser).
    /// 3. `window_aumid(frame)` et `process_aumid(pid_de_la_frame)` : rendent-ils
    ///    l'AUMID de l'hôte ou celui de l'application ?
    /// 4. `process_aumid(enfant)` — et ce que `shell_item_info` en tire (nom
    ///    affiché ET icône), c'est-à-dire la chaîne complète de l'icône.
    ///
    /// ⚠️ **Réserve de méthode** : `SHGetPropertyStoreForWindow` passe par COM,
    /// et la mémoire du projet note qu'un thread de test peut fausser une
    /// lecture d'identité par COM (artefact de contexte). Un `window_aumid`
    /// vide ici n'est donc PAS une preuve d'absence — c'est `process_aumid`,
    /// qui n'utilise pas COM, qui fait foi.
    ///
    /// Lecture seule : aucune fenêtre n'est créée, déplacée ni activée, aucune
    /// entrée n'est simulée (voir [[neuradesk-live-input-safety]]).
    ///
    /// **POURQUOI ELLE N'EST PAS UN INSTRUMENT JETABLE.** Le correctif bâti sur
    /// cette mesure est SILENCIEUX quand il échoue : si la classe du descendant
    /// changeait, `hosted_app_identity` rendrait `None`, `collect_window`
    /// écarterait la fenêtre, et **les applications du Store disparaîtraient de
    /// la liste des programmes ouverts sans erreur ni journal** — le défaut
    /// qu'on vient de corriger, par une autre porte. Cette sonde est la
    /// commande qui le diagnostique, avec les Paramètres de Windows ouverts :
    ///
    /// ```text
    /// cargo test --release --bins -- --ignored --nocapture \
    ///     prints_the_identity_of_a_packaged_app_frame
    /// ```
    #[test]
    #[ignore = "lit l'état réel de l'hôte d'applications empaquetées de la machine"]
    fn prints_the_identity_of_a_packaged_app_frame() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let mut frames: Vec<HWND> = Vec::new();
            let _ = EnumWindows(Some(gather_frames), LPARAM(&mut frames as *mut _ as isize));

            println!(
                "\n=== {PACKAGED_APP_HOST_EXE} : {} fenêtre(s) de premier niveau ===",
                frames.len()
            );
            for frame in frames {
                let mut pid = 0u32;
                GetWindowThreadProcessId(frame, Some(&mut pid));
                let ex_style = GetWindowLongPtrW(frame, GWL_EXSTYLE) as u32;
                let owner = GetWindow(frame, GW_OWNER);
                let has_owner = owner.map(|o| !o.0.is_null()).unwrap_or(false);
                let is_visible = IsWindowVisible(frame).as_bool();
                let toolwindow = ex_style & WS_EX_TOOLWINDOW.0 != 0;

                println!(
                    "\n--- frame {:?} : pid {pid}, titre {:?} ---",
                    frame.0,
                    window_title(frame)
                );
                println!("    exe                        : {:?}", process_image_name(pid).map(|(n, _)| n));
                println!("    exstyle                    : 0x{ex_style:X}  WS_EX_TOOLWINDOW={toolwindow}");
                println!("    propriétaire (GW_OWNER)    : {has_owner}");
                println!("    IsWindowVisible            : {is_visible}");
                println!(
                    "    FRANCHIT collect_window ?  : {}",
                    if is_visible && !toolwindow && !has_owner && !window_title(frame).is_empty() {
                        "OUI — l'exclusion par nom est bien ce qui le retire"
                    } else {
                        "NON — écarté AVANT la liste d'infrastructure, diagnostic à refaire"
                    }
                );
                println!("    window_aumid(frame)        : {:?}", window_aumid(frame));
                println!("    process_aumid(pid_frame)   : {:?}", process_aumid(pid));

                let mut children: Vec<ChildInfo> = Vec::new();
                let _ = EnumChildWindows(
                    Some(frame),
                    Some(gather_children),
                    LPARAM(&mut children as *mut _ as isize),
                );
                println!("    --- {} descendant(s) ---", children.len());
                for child in children {
                    let hosted = child.pid != pid;
                    println!(
                        "      {:<34} pid {:<6} {} exe {:?} titre {:?}",
                        child.class,
                        child.pid,
                        if hosted { "[AUTRE PROCESSUS]" } else { "[la frame]      " },
                        child.exe,
                        child.title
                    );
                    if hosted {
                        let aumid = process_aumid(child.pid);
                        println!("        process_aumid          : {aumid:?}");
                        if let Some(aumid) = aumid {
                            let shell_path = format!(r"shell:AppsFolder\{aumid}");
                            let info = crate::win32::taskbar_pins::shell_item_info(&shell_path);
                            match info {
                                Some((name, icon)) => println!(
                                    "        shell_item_info        : nom {name:?}, icône {}",
                                    match icon {
                                        Some((w, h, ref px)) => format!("{w}x{h}, {} octets", px.len()),
                                        None => "ABSENTE".to_string(),
                                    }
                                ),
                                None => println!("        shell_item_info        : ÉCHEC (aucune entrée)"),
                            }
                        }
                    }
                }
            }
            println!();
        }
    }

    /// **SONDE — les fenêtres d'une application du Store, réduite ou non.**
    /// Défaut signalé (2026-10-24) : les Paramètres réduits ne reviennent pas
    /// depuis leur icône. Imprime, pour chaque fenêtre de premier niveau de
    /// `ApplicationFrameHost.exe` et de `SystemSettings.exe` : classe, titre,
    /// visible, réduite, voilée (DWM), enfants et leur processus.
    #[test]
    #[ignore]
    fn sonde_fenetres_du_store_reduites() {
        use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
        use windows::Win32::UI::WindowsAndMessaging::IsIconic;
        unsafe extern "system" fn toutes(hwnd: HWND, lparam: LPARAM) -> BOOL {
            (*(lparam.0 as *mut Vec<HWND>)).push(hwnd);
            BOOL(1)
        }
        let classe = |hwnd: HWND| unsafe {
            let mut b = [0u16; 256];
            let n = GetClassNameW(hwnd, &mut b);
            String::from_utf16_lossy(&b[..n.max(0) as usize])
        };
        let pid_de = |hwnd: HWND| unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            pid
        };
        let mut fenetres: Vec<HWND> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(toutes), LPARAM(&mut fenetres as *mut _ as isize));
        }
        for hwnd in fenetres {
            let pid = pid_de(hwnd);
            let Some((exe, _)) = process_image_name(pid) else { continue };
            if !exe.eq_ignore_ascii_case(PACKAGED_APP_HOST_EXE) && !exe.eq_ignore_ascii_case("SystemSettings.exe") {
                continue;
            }
            let mut titre = [0u16; 256];
            let n = unsafe { InternalGetWindowText(hwnd, &mut titre) };
            let mut voile = 0u32;
            unsafe {
                let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut voile as *mut u32 as *mut _, 4);
            }
            let proprio = unsafe { GetWindow(hwnd, GW_OWNER) }.unwrap_or_default();
            println!(
                "{exe} {:?} classe={} titre={:?} visible={} reduite={} voile={voile} proprio={:?}",
                hwnd.0,
                classe(hwnd),
                String::from_utf16_lossy(&titre[..n.max(0) as usize]),
                unsafe { IsWindowVisible(hwnd) }.as_bool(),
                unsafe { IsIconic(hwnd) }.as_bool(),
                proprio.0,
            );
            let mut enfants: Vec<HWND> = Vec::new();
            unsafe {
                let _ = EnumChildWindows(Some(hwnd), Some(toutes), LPARAM(&mut enfants as *mut _ as isize));
            }
            for e in enfants {
                println!("    enfant {:?} classe={} pid={}", e.0, classe(e), pid_de(e));
            }
            println!("    window_identity={:?} process_aumid={:?}", window_identity(hwnd).0, process_aumid(pid));
        }
        let ouvertes = open_windows();
        println!("open_windows : {:?}", ouvertes.iter().filter(|w| w.exe.eq_ignore_ascii_case("SystemSettings.exe")).map(|w| (w.hwnd.0, &w.title)).collect::<Vec<_>>());
    }
}
