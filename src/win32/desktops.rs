//! Bureaux virtuels Windows : nombre de bureaux et bureau actif (pastilles
//! cliquables de la barre du haut, façon indicateur de bureaux GNOME — voir
//! `ui::topbar::desktop_dot_rect`, `app::run`) et bascule vers un bureau
//! donné.
//!
//! Windows n'expose aucune API publique permettant d'énumérer les bureaux
//! virtuels ni de changer directement de bureau par index — seule
//! `IVirtualDesktopManager` (publique) permet de savoir sur quel bureau se
//! trouve une fenêtre donnée, pas d'en lister le nombre total. On relit
//! donc directement les clés de registre que l'Explorateur utilise en
//! interne pour stocker cette info : non documentées, potentiellement
//! sujettes à changement d'une version de Windows à l'autre (même statut
//! que les ordinaux `uxtheme.dll`, voir `win32::theme`), mais stables
//! depuis Windows 10 1809 et déjà exploitées par plusieurs projets open
//! source (VirtualDesktopAccessor et dérivés). Best-effort : l'indicateur
//! se masque simplement si la lecture échoue (voir `current`).
//!
//! **RATTACHER UNE FENÊTRE À SON BUREAU — l'API PUBLIQUE SUFFIT, et c'est une
//! MESURE, pas une lecture.** `IVirtualDesktopManager::GetWindowDesktopId` rend
//! un `GUID` ; la question était de savoir si c'est le MÊME que ceux de
//! `VirtualDesktopIDs`, au même format d'octets. La sonde
//! `prints_the_real_window_to_desktop_mapping` l'établit sur cette machine :
//! correspondance exacte, octet pour octet (voir `windows_by_desktop`). Aucune
//! interface interne n'est donc nécessaire — contrairement à ce que laissait
//! croire la doc de ce module, corrigée ici.
//!
//! **Deux modes d'échec mesurés, tous deux à ÉCARTER et non à signaler** :
//! `GetWindowDesktopId` rend `0x8002802B` (`TYPE_E_ELEMENTNOTFOUND`) pour les
//! fenêtres que le shell ne gère pas comme fenêtres de bureau — les deux
//! fenêtres de NeuraDesk lui-même, les sous-processus `msedgewebview2.exe`,
//! `TextInputHost.exe`, PowerToys Quick Access ; et un `GUID` qui n'est dans
//! aucune liste pour les fenêtres « affichées sur tous les bureaux ».
//!
//! Pour changer de bureau, on simule le raccourci clavier documenté
//! Win+Ctrl+Flèche (`SendInput`) le nombre de fois nécessaire — Windows ne
//! propose aucune autre façon publique de le faire par index. Ce n'est PAS
//! une simulation de clic/curseur : uniquement une réponse à un vrai clic
//! de l'utilisateur sur nos propres pastilles, au même titre qu'un
//! raccourci clavier déclenché par n'importe quel lanceur/gestionnaire de
//! fenêtres tiers.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RRF_RT_REG_SZ};
use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL,
    VK_LEFT, VK_LWIN, VK_RIGHT,
};

const GUID_SIZE: usize = 16;
/// Nombre maximal de pastilles affichées (large marge : la plupart des
/// utilisateurs ont moins de 10 bureaux virtuels) — évite tout débordement
/// de la barre du haut dans le cas extrême d'une valeur de registre
/// corrompue.
const MAX_DESKTOPS: usize = 20;

/// Bureaux virtuels actuels : leur nombre et l'index (0-based, dans l'ordre
/// stocké par Windows — habituellement l'ordre de Task View) de celui
/// actif.
pub struct VirtualDesktops {
    pub count: usize,
    pub active_index: usize,
}

/// Relit `VirtualDesktopIDs`/`CurrentVirtualDesktop` dans le registre (voir
/// doc de module). `None` si la clé est absente, illisible, ou d'un format
/// inattendu — l'appelant masque alors l'indicateur plutôt que d'afficher
/// une valeur fausse (voir `app::run`).
pub fn current() -> Option<VirtualDesktops> {
    // `NEURADESK_DEBUG_DESKTOPS=<n>` : FABRIQUE `n` bureaux au lieu de les
    // lire — même famille que les autres `NEURADESK_DEBUG_*` (voir
    // `win32::debug_capture`, doc de module), et sans effet en usage normal
    // puisque le lanceur habituel ne la pose jamais.
    //
    // **POURQUOI ELLE EXISTE** : les numéros portés par les pastilles changent
    // de taille de police à partir du dixième bureau (voir
    // `ui::desktop_labels`), et ce chemin ne se vérifie autrement qu'en créant
    // dix bureaux virtuels sur la machine — un effet de bord sur une session
    // Windows VIVANTE, ce que ce dépôt s'interdit. Le levier laisse le VRAI
    // moteur de rendu dessiner vingt pastilles, au lieu de faire dessiner un
    // aperçu par une sonde qui ne prouverait rien du rendu réel.
    if let Some(n) = std::env::var("NEURADESK_DEBUG_DESKTOPS").ok().and_then(|v| v.parse::<usize>().ok()) {
        return Some(VirtualDesktops { count: n.min(MAX_DESKTOPS), active_index: 0 });
    }

    let ids = read_binary_value(w!("VirtualDesktopIDs"));
    let actif = bureau_actif(ids.as_deref());
    depuis_le_registre(ids.as_deref(), actif.as_deref())
}

/// Les octets du bureau ACTIF, cherchés là où chaque Windows les met :
///
/// 1. `Explorer\VirtualDesktops\CurrentVirtualDesktop` — Windows 11 ;
/// 2. `Explorer\SessionInfo\<session>\VirtualDesktops\CurrentVirtualDesktop` —
///    Windows 10, qui ne l'écrit qu'APRÈS une première bascule de bureau ;
/// 3. l'API PUBLIQUE : une fenêtre visible sur le bureau courant en donne le
///    `GUID` (même format d'octets que `VirtualDesktopIDs`, voir la doc de
///    module). C'est le seul recours de Windows 10 avant toute bascule.
///
/// MESURÉ (2026-10-31, VM 19045) : `VirtualDesktopIDs` présent avec deux
/// bureaux, ni 1 ni 2 — l'applet était masquée.
fn bureau_actif(ids: Option<&[u8]>) -> Option<Vec<u8>> {
    let un_guid = |valeur: Vec<u8>| (valeur.len() == GUID_SIZE).then_some(valeur);
    read_binary_value(w!("CurrentVirtualDesktop"))
        .and_then(un_guid)
        .or_else(|| {
            let session = session_courante()?;
            read_binary_value_in(&cle_de_session(session), w!("CurrentVirtualDesktop")).and_then(un_guid)
        })
        .or_else(|| bureau_d_une_fenetre_visible(ids?))
}

/// La session Windows de ce processus — la clé `SessionInfo` de Windows 10 en
/// dépend.
fn session_courante() -> Option<u32> {
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    let mut session = 0u32;
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) }.ok()?;
    Some(session)
}

/// Le bureau courant d'après la PREMIÈRE fenêtre visible qui s'y trouve et dont
/// le `GUID` figure dans `ids` — les fenêtres « sur tous les bureaux » et
/// celles que le shell ne gère pas (dont les nôtres) sont écartées d'office
/// (voir la doc de module). `None` si le bureau courant est vide.
fn bureau_d_une_fenetre_visible(ids: &[u8]) -> Option<Vec<u8>> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, IsWindowVisible};

    unsafe extern "system" fn une(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if unsafe { IsWindowVisible(hwnd) }.as_bool() {
            unsafe { &mut *(lparam.0 as *mut Vec<HWND>) }.push(hwnd);
        }
        BOOL(1)
    }

    let manager = unsafe { CoCreateInstance::<_, IVirtualDesktopManager>(&VirtualDesktopManager, None, CLSCTX_ALL) }.ok()?;
    let mut fenetres: Vec<HWND> = Vec::new();
    let _ = unsafe { EnumWindows(Some(une), LPARAM(&mut fenetres as *mut Vec<HWND> as isize)) };
    fenetres.into_iter().find_map(|hwnd| {
        if !unsafe { manager.IsWindowOnCurrentVirtualDesktop(hwnd) }.ok()?.as_bool() {
            return None;
        }
        let brut = guid_bytes(&unsafe { manager.GetWindowDesktopId(hwnd) }.ok()?);
        ids.chunks_exact(GUID_SIZE).any(|guid| guid == brut).then(|| brut.to_vec())
    })
}

/// Le calcul de `current`, à partir des deux valeurs lues (`None` : absente).
///
/// **SANS `VirtualDesktopIDs`, IL Y A UN BUREAU, PAS ZÉRO.** MESURÉ (2026-10-17)
/// sur un PC qui n'a jamais créé de second bureau : la clé `VirtualDesktops`
/// existe, mais ni `VirtualDesktopIDs` ni `CurrentVirtualDesktop` — Windows ne
/// les écrit qu'à la création d'un deuxième bureau. Rendre `None` masquait
/// l'applet sur toute machine neuve. Une pastille unique est rendue ; la
/// relecture périodique fait paraître les suivantes dès que l'utilisateur
/// ajoute un bureau (Win+Ctrl+D).
///
/// **UNE LISTE SANS BUREAU ACTIF DÉSIGNE LE PREMIER**, au lieu de masquer
/// l'applet : c'est Windows 10 avant toute bascule (voir `bureau_actif`), où
/// le bureau d'ouverture de session est le premier.
fn depuis_le_registre(ids: Option<&[u8]>, current: Option<&[u8]>) -> Option<VirtualDesktops> {
    let Some(ids) = ids else {
        return Some(VirtualDesktops { count: 1, active_index: 0 });
    };
    if ids.is_empty() || ids.len() % GUID_SIZE != 0 {
        return None;
    }
    let count = (ids.len() / GUID_SIZE).min(MAX_DESKTOPS);
    let Some(current) = current else {
        return Some(VirtualDesktops { count, active_index: 0 });
    };
    if current.len() != GUID_SIZE {
        return None;
    }

    let active_index = ids.chunks_exact(GUID_SIZE).position(|guid| guid == current)?;
    if active_index >= count {
        return None;
    }
    Some(VirtualDesktops { count, active_index })
}

/// Le contenu BRUT de `VirtualDesktopIDs` — la liste des `GUID` de bureaux, dans
/// l'ordre des pastilles (16 octets par bureau).
///
/// **`pub(crate)` pour la seule sonde `win32::thumb_probe`**, qui a besoin du
/// `GUID` d'un bureau précis pour y DÉPLACER une fenêtre à elle
/// (`IVirtualDesktopManager::MoveWindowToDesktop`) : c'est ce déplacement qui
/// crée le cas à mesurer, une fenêtre sur un bureau inactif. Rien d'autre ne
/// l'appelle — le reste du module passe par `current`/`windows_by_desktop`.
#[cfg(test)]
pub(crate) fn raw_desktop_ids() -> Option<Vec<u8>> {
    read_binary_value(w!("VirtualDesktopIDs"))
}

/// Une fenêtre RATTACHÉE à son bureau — la matière de la carte des bureaux
/// (voir `windows_by_desktop`). Volontairement pauvre : ce que la carte
/// AFFICHE, et rien de plus (le `HWND` n'est là que pour permettre plus tard
/// d'activer la fenêtre depuis la carte).
pub struct DesktopWindow {
    pub hwnd: HWND,
    pub title: String,
    pub exe: String,
    /// Chemin complet de l'exécutable — c'est LUI qui donne l'icône de la
    /// ligne à la carte (voir `win32::cef_desktop_preview`), le seul nom de
    /// fichier ne permettant pas de la retrouver.
    pub exe_path: String,
}

/// Les fenêtres de CHAQUE bureau, indexées comme les pastilles (0 = premier
/// bureau dans l'ordre de Windows, celui de `VirtualDesktopIDs`).
///
/// **C'est la seule fonction qui a besoin de `IVirtualDesktopManager`**, et le
/// pont qu'elle établit est une mesure (voir la doc de module) : le `GUID` rendu
/// par `GetWindowDesktopId` se compare OCTET POUR OCTET aux morceaux de
/// `VirtualDesktopIDs`, exactement comme `current` compare `CurrentVirtualDesktop`
/// à ces mêmes morceaux. Aucune conversion n'est faite — en introduire une
/// (endianness, `to_u128`) risquerait d'inverser la convention et de tout
/// déclarer « hors liste ».
///
/// **Vecteur VIDE en cas d'échec**, jamais une erreur : l'appelant n'affiche
/// alors simplement pas de carte (même politique que `current`, qui masque les
/// pastilles). Les fenêtres qui ne se rattachent à aucun bureau sont ÉCARTÉES
/// en silence, et c'est voulu : elles sont légitimement hors sujet (voir les
/// deux modes d'échec mesurés, doc de module), donc les signaler noierait le
/// journal à chaque frame sans rien apprendre.
///
/// **Coût** : une création COM et un appel par fenêtre. À n'appeler qu'à la
/// cadence de l'instantané des fenêtres (1 Hz, voir `app::run`), jamais par
/// frame.
pub fn windows_by_desktop(windows: &[crate::win32::app_identity::OpenWindow]) -> Vec<Vec<DesktopWindow>> {
    let Some(ids) = read_binary_value(w!("VirtualDesktopIDs")) else { return Vec::new() };
    let count = ids.len() / GUID_SIZE;
    if count == 0 {
        return Vec::new();
    }
    // COM est déjà initialisé en STA sur le thread principal (voir `main`) —
    // cette fonction n'est appelée que de là.
    let Ok(manager) = (unsafe { CoCreateInstance::<_, IVirtualDesktopManager>(&VirtualDesktopManager, None, CLSCTX_ALL) })
    else {
        return Vec::new();
    };

    let mut per_desktop: Vec<Vec<DesktopWindow>> = (0..count).map(|_| Vec::new()).collect();
    for window in windows {
        let Ok(id) = (unsafe { manager.GetWindowDesktopId(window.hwnd) }) else { continue };
        let raw = guid_bytes(&id);
        let Some(index) = ids.chunks_exact(GUID_SIZE).position(|chunk| chunk == raw) else { continue };
        per_desktop[index].push(DesktopWindow {
            hwnd: window.hwnd,
            title: window.title.clone(),
            exe: window.exe.clone(),
            exe_path: window.exe_path.clone(),
        });
    }
    per_desktop
}

/// Les OCTETS BRUTS d'un `GUID` COM, dans l'ordre où ils se trouvent en
/// mémoire — c'est ce qui les rend comparables aux morceaux de
/// `VirtualDesktopIDs`, qui ne sont rien d'autre qu'une suite de `GUID` tels
/// que le shell les détient.
///
/// **On ne convertit RIEN** (`to_u128`, `from_u128`) : une conversion
/// introduirait une convention d'octets qui pourrait être l'inverse de celle du
/// registre, et la comparaison échouerait — ou pire, réussirait par accident
/// sur un GUID symétrique. C'est la comparaison d'octets bruts qui répond,
/// vérifiée par la sonde de ce module.
/// Les OCTETS du bureau ACTIF — exactement ce que `MoveWindowToDesktop` attend,
/// et l'inverse de `guid_bytes` (copie brute, AUCUNE conversion d'octets : voir
/// sa doc, et la doctrine du module).
///
/// **Volontairement SÉPARÉE de `current()`**, qui rend un INDEX : faire suivre
/// le dock demande les OCTETS du bureau, pas son rang dans les pastilles.
pub fn current_guid() -> Option<Vec<u8>> {
    bureau_actif(read_binary_value(w!("VirtualDesktopIDs")).as_deref())
}

/// Inverse exact de `guid_bytes` : seize octets bruts redeviennent une `GUID`.
fn guid_from_bytes(bytes: &[u8]) -> Option<windows::core::GUID> {
    if bytes.len() < GUID_SIZE {
        return None;
    }
    let mut guid: windows::core::GUID = unsafe { std::mem::zeroed() };
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), &mut guid as *mut _ as *mut u8, GUID_SIZE) };
    Some(guid)
}

/// **DÉPLACE UNE FENÊTRE VERS LE BUREAU VIRTUEL INDIQUÉ.**
///
/// C'est le SEUL moyen, avec l'API PUBLIQUE, de faire suivre le dock à
/// l'utilisateur qui change de bureau : `IVirtualDesktopManager` ne sait pas
/// « afficher sur tous les bureaux » — la doc de module le dit noir sur blanc —
/// elle ne sait que DÉPLACER. On la rappelle donc à chaque bascule (voir
/// `app::run`, qui compare le bureau courant d'un relevé à l'autre).
///
/// ⚠️ **LA FENÊTRE SUIT, ELLE N'EST PAS ÉPINGLÉE** : entre la bascule et cet
/// appel, le dock n'est plus visible — d'où l'intérêt de le faire à une cadence
/// plus rapide que l'instantané d'une seconde.
///
/// `false` en cas d'échec, jamais une panique : au pire le dock reste où il
/// est, c'est-à-dire l'état d'avant ce correctif.
pub fn move_window_to_desktop(hwnd: HWND, desktop: &[u8]) -> bool {
    let Some(guid) = guid_from_bytes(desktop) else {
        return false;
    };
    let Ok(manager) = (unsafe { CoCreateInstance::<_, IVirtualDesktopManager>(&VirtualDesktopManager, None, CLSCTX_ALL) }) else {
        return false;
    };
    unsafe { manager.MoveWindowToDesktop(hwnd, &guid) }.is_ok()
}

fn guid_bytes(guid: &windows::core::GUID) -> [u8; GUID_SIZE] {
    let mut out = [0u8; GUID_SIZE];
    unsafe {
        std::ptr::copy_nonoverlapping(guid as *const _ as *const u8, out.as_mut_ptr(), GUID_SIZE);
    }
    out
}

/// Le fond d'écran de CHAQUE bureau virtuel, indexé comme les pastilles —
/// `None` pour un bureau qui n'en a pas de propre.
///
/// **WINDOWS RANGE BIEN UN FOND PAR BUREAU, ET C'EST LISIBLE** (mesuré le
/// 2026-09-20 sur cette machine : les trois bureaux actifs avaient chacun leur
/// valeur, et les trois fichiers existaient) :
///
/// ```text
/// HKCU\...\Explorer\VirtualDesktops\Desktops\{GUID}\Wallpaper
/// ```
///
/// **PAS D'API COM ICI, ET C'EST DÉLIBÉRÉ** : `IDesktopWallpaper` (documentée)
/// ne rend que le fond du bureau **COURANT** — elle ne sait pas répondre « quel
/// fond pour le bureau 2 ? », qui est exactement la question de la carte. La
/// valeur de registre, elle, répond bureau par bureau.
///
/// **AUCUNE CONVERSION DE `GUID` N'EST INTRODUITE** : les octets viennent de
/// `VirtualDesktopIDs`, les mêmes que `windows_by_desktop` compare octet pour
/// octet, et la mise en forme du nom de sous-clé est celle qui a été vérifiée à
/// la main (les sous-clés existent bel et bien sous cette forme).
///
/// **Vecteur VIDE en cas d'échec**, jamais une erreur — même politique que
/// `windows_by_desktop` : l'appelant montre alors un fond uni plutôt que rien.
pub fn wallpapers_by_desktop() -> Vec<Option<std::path::PathBuf>> {
    let Some(ids) = read_binary_value(w!("VirtualDesktopIDs")) else { return Vec::new() };
    if ids.is_empty() || ids.len() % GUID_SIZE != 0 {
        return Vec::new();
    }
    let count = (ids.len() / GUID_SIZE).min(MAX_DESKTOPS);
    ids.chunks_exact(GUID_SIZE)
        .take(count)
        .map(|guid| {
            let name = desktop_key_name(guid)?;
            let subkey = format!("{DESKTOPS_SUBKEY}\\{name}");
            read_wallpaper(&subkey)
        })
        .collect()
}

/// Sous-clé sous laquelle Windows range les données de chaque bureau virtuel.
const DESKTOPS_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VirtualDesktops\\Desktops";

/// Le nom de sous-clé d'un bureau, à partir de ses 16 octets bruts :
/// `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`, en MAJUSCULES (la forme que Windows
/// emploie).
///
/// **L'ORDRE DES OCTETS EST CELUI D'UN `GUID` EN MÉMOIRE** — `Data1`, `Data2` et
/// `Data3` en little-endian, les huit derniers octets dans l'ordre. C'est la
/// conversion employée lors de la mesure, et c'est elle qui a retrouvé les trois
/// sous-clés : une inversion d'endianness donnerait un nom plausible mais
/// introuvable, donc un `None` silencieux.
fn desktop_key_name(guid: &[u8]) -> Option<String> {
    if guid.len() != GUID_SIZE {
        return None;
    }
    Some(format!(
        "{{{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        guid[3], guid[2], guid[1], guid[0], guid[5], guid[4], guid[7], guid[6], guid[8], guid[9], guid[10], guid[11],
        guid[12], guid[13], guid[14], guid[15],
    ))
}

/// Lit la valeur `Wallpaper` d'un bureau — `None` si elle est absente, illisible,
/// ou si ce n'est **pas un fichier**.
///
/// ⚠️ **LE TEST « C'EST UN FICHIER » N'EST PAS UNE COQUETTERIE** : un fond défini
/// par **diaporama** désigne un DOSSIER, pas une image. Le refuser ici, d'un seul
/// test, évite d'avoir à traiter ce cas partout ailleurs.
fn read_wallpaper(subkey: &str) -> Option<std::path::PathBuf> {
    let subkey: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let subkey = PCWSTR(subkey.as_ptr());
    unsafe {
        let mut size: u32 = 0;
        if RegGetValueW(HKEY_CURRENT_USER, subkey, w!("Wallpaper"), RRF_RT_REG_SZ, None, None, Some(&mut size))
            != ERROR_SUCCESS
            || size == 0
        {
            return None;
        }
        let mut buffer = vec![0u8; size as usize];
        if RegGetValueW(
            HKEY_CURRENT_USER,
            subkey,
            w!("Wallpaper"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr() as *mut _),
            Some(&mut size),
        ) != ERROR_SUCCESS
        {
            return None;
        }
        buffer.truncate(size as usize);
        // `REG_SZ` : UTF-16, terminé par un zéro.
        let units: Vec<u16> = buffer.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        let end = units.iter().position(|&c| c == 0).unwrap_or(units.len());
        if end == 0 {
            return None;
        }
        let path = std::path::PathBuf::from(String::from_utf16_lossy(&units[..end]));
        path.is_file().then_some(path)
    }
}

/// Lit une valeur binaire de taille variable sous la clé
/// `...\Explorer\VirtualDesktops` : un premier appel pour connaître la
/// taille nécessaire, un second pour la remplir (le nombre de bureaux, et
/// donc la taille de `VirtualDesktopIDs`, n'est pas connu à l'avance).
fn read_binary_value(name: PCWSTR) -> Option<Vec<u8>> {
    read_binary_value_in(CLE_BUREAUX, name)
}

/// Clé commune des bureaux virtuels : `VirtualDesktopIDs` partout, et
/// `CurrentVirtualDesktop` sous Windows 11.
const CLE_BUREAUX: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VirtualDesktops";

/// Clé où Windows 10 range `CurrentVirtualDesktop` : une par SESSION.
fn cle_de_session(session: u32) -> String {
    format!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\SessionInfo\\{session}\\VirtualDesktops")
}

fn read_binary_value_in(subkey: &str, name: PCWSTR) -> Option<Vec<u8>> {
    let subkey = windows::core::HSTRING::from(subkey);
    let subkey = PCWSTR(subkey.as_ptr());
    unsafe {
        let mut size: u32 = 0;
        let status = RegGetValueW(HKEY_CURRENT_USER, subkey, name, RRF_RT_REG_BINARY, None, None, Some(&mut size));
        if status != ERROR_SUCCESS || size == 0 {
            return None;
        }

        let mut buffer = vec![0u8; size as usize];
        let status = RegGetValueW(
            HKEY_CURRENT_USER,
            subkey,
            name,
            RRF_RT_REG_BINARY,
            None,
            Some(buffer.as_mut_ptr() as *mut _),
            Some(&mut size),
        );
        if status != ERROR_SUCCESS {
            return None;
        }
        buffer.truncate(size as usize);
        Some(buffer)
    }
}

/// Bascule vers le bureau virtuel `target_index` en simulant Win+Ctrl+Droite
/// ou Win+Ctrl+Gauche (raccourci documenté de Windows) autant de fois que
/// nécessaire depuis `current_index`. Aucun effet si les deux index sont
/// déjà égaux.
pub fn switch_to(current_index: usize, target_index: usize) {
    let (key, steps) = if target_index > current_index {
        (VK_RIGHT, target_index - current_index)
    } else {
        (VK_LEFT, current_index - target_index)
    };
    for _ in 0..steps {
        send_shortcut(key);
    }
}

/// Simule Win+Ctrl+`arrow` (appui puis relâchement dans l'ordre inverse).
fn send_shortcut(arrow: VIRTUAL_KEY) {
    let inputs = [
        keyboard_input(VK_LWIN, false),
        keyboard_input(VK_CONTROL, false),
        keyboard_input(arrow, false),
        keyboard_input(arrow, true),
        keyboard_input(VK_CONTROL, true),
        keyboard_input(VK_LWIN, true),
    ];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// `pub(crate)` : réutilisée par `win32::clipboard_viewer` (bouton "Coller",
/// simule Ctrl+V) — même brique Win32 de base, pas la peine de la dupliquer
/// pour un raccourci différent.
pub(crate) fn keyboard_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if key_up { KEYEVENTF_KEYUP } else { Default::default() },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02X}")).collect()
    }

    /// Un PC qui n'a jamais créé de second bureau n'a pas `VirtualDesktopIDs` :
    /// c'est UN bureau, et l'applet reste visible.
    #[test]
    fn sans_liste_de_bureaux_il_y_en_a_un() {
        let vd = depuis_le_registre(None, None).expect("une pastille");
        assert_eq!((vd.count, vd.active_index), (1, 0));

        let ids: Vec<u8> = (0..2 * GUID_SIZE as u8).collect();
        let vd = depuis_le_registre(Some(&ids), Some(&ids[GUID_SIZE..])).expect("deux bureaux");
        assert_eq!((vd.count, vd.active_index), (2, 1));

        // Une liste présente mais illisible reste masquée plutôt que fausse.
        assert!(depuis_le_registre(Some(&ids[..5]), Some(&ids[..GUID_SIZE])).is_none());
    }

    /// **WINDOWS 10 : UNE LISTE DE BUREAUX SANS BUREAU ACTIF RESTE VISIBLE.**
    /// MESURÉ (2026-10-31, VM 19045) : `VirtualDesktopIDs` liste deux bureaux,
    /// mais aucun `CurrentVirtualDesktop` nulle part — ni sous
    /// `Explorer\VirtualDesktops` (où le met Windows 11), ni encore sous
    /// `SessionInfo\<session>\VirtualDesktops`. L'ancien `current?` masquait
    /// l'applet entière.
    #[test]
    fn une_liste_sans_bureau_actif_designe_le_premier() {
        let ids: Vec<u8> = (0..2 * GUID_SIZE as u8).collect();
        let vd = depuis_le_registre(Some(&ids), None).expect("deux pastilles");
        assert_eq!((vd.count, vd.active_index), (2, 0));
    }

    /// Windows 10 range le bureau actif PAR SESSION, pas sous la clé commune.
    #[test]
    fn la_cle_de_session_de_windows_10() {
        assert_eq!(
            cle_de_session(1),
            "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\SessionInfo\\1\\VirtualDesktops"
        );
    }

    /// **SONDE — imprime la correspondance RÉELLE fenêtre -> bureau virtuel de
    /// cette machine, SANS lancer l'application** (même motif que
    /// `notification::tests::reads_the_real_database` et
    /// `taskbar_pins::tests::read_pins_prints_the_real_taskbar_order`) :
    ///
    /// `cargo test --bins -- --ignored --nocapture window_to_desktop`
    ///
    /// **CE QU'ELLE ÉTABLIT, et qui conditionne toute la fonctionnalité** : les
    /// aperçus par bureau n'ont de contenu que si l'on sait rattacher une
    /// fenêtre à SON bureau. La doc de module affirmait que l'interface
    /// PUBLIQUE ne répond que « est-elle sur le bureau courant ? » — or
    /// `IVirtualDesktopManager::GetWindowDesktopId` rend un `GUID`, et la
    /// question ouverte est de savoir si c'est le MÊME que ceux de
    /// `VirtualDesktopIDs`, au même format d'octets. Cette sonde le MESURE au
    /// lieu de le supposer, et compte les fenêtres par bureau : sans elle, on
    /// écrirait un affichage sur une hypothèse.
    ///
    /// Lecture seule : COM est interrogé, aucune fenêtre n'est déplacée, aucun
    /// bureau n'est changé (voir [[neuradesk-live-input-safety]]).
    #[test]
    #[ignore = "lit l'état réel des bureaux virtuels de la machine"]
    fn prints_the_real_window_to_desktop_mapping() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let ids = read_binary_value(windows::core::w!("VirtualDesktopIDs")).unwrap_or_default();
        let current = read_binary_value(windows::core::w!("CurrentVirtualDesktop")).unwrap_or_default();
        let count = ids.len() / GUID_SIZE;
        let active = ids.chunks_exact(GUID_SIZE).position(|chunk| chunk == current.as_slice());
        println!("== registre ==");
        println!("{count} bureau(x), bureau actif = {active:?}");
        for index in 0..count {
            println!("  bureau {index} : {}", hex(&ids[index * GUID_SIZE..(index + 1) * GUID_SIZE]));
        }

        let manager = unsafe { CoCreateInstance::<_, IVirtualDesktopManager>(&VirtualDesktopManager, None, CLSCTX_ALL) };
        let Ok(manager) = manager else {
            println!("!! IVirtualDesktopManager INDISPONIBLE : {manager:?}");
            return;
        };
        println!("== IVirtualDesktopManager obtenu ==");

        let windows = crate::win32::app_identity::open_windows();
        let mut per_desktop = vec![0usize; count];
        let mut off_list = 0usize;
        for window in &windows {
            let id = unsafe { manager.GetWindowDesktopId(window.hwnd) };
            let on_current = unsafe { manager.IsWindowOnCurrentVirtualDesktop(window.hwnd) };
            let on_current = on_current.map(|value| value.as_bool()).unwrap_or(false);
            let (label, bytes) = match &id {
                Ok(guid) => {
                    let raw = guid_bytes(guid);
                    match ids.chunks_exact(GUID_SIZE).position(|chunk| chunk == raw) {
                        Some(index) => {
                            per_desktop[index] += 1;
                            (format!("bureau {index}"), hex(&raw))
                        }
                        None => {
                            off_list += 1;
                            ("HORS LISTE".to_string(), hex(&raw))
                        }
                    }
                }
                Err(error) => {
                    off_list += 1;
                    (format!("ERREUR {error:?}"), "-".to_string())
                }
            };
            println!("  {label:12} actif={on_current:5} {bytes}  [{}] {}", window.exe, window.title);
        }
        println!("== bilan ==");
        println!("{} fenetres ; par bureau : {per_desktop:?} ; hors liste : {off_list}", windows.len());

        // LA FONCTION DE PRODUCTION, telle que `app::run` l'appelle — la sonde
        // ci-dessus en est une RÉÉCRITURE, donc elle ne prouve rien sur elle.
        let grouped = windows_by_desktop(&windows);
        println!("== windows_by_desktop ==");
        println!("{} bureau(x) ; par bureau : {:?}", grouped.len(), grouped.iter().map(Vec::len).collect::<Vec<_>>());
    }

    /// **SONDE — le fond d'écran de chaque bureau virtuel, lu sur la machine
    /// réelle.**
    ///
    /// C'est elle qui a établi la faisabilité de la miniature de bureau : Windows
    /// range une valeur `Wallpaper` par bureau, sous
    /// `…\VirtualDesktops\Desktops\{GUID}`, et la mise en forme du `GUID` en nom
    /// de sous-clé est celle qu'emploie `desktop_key_name`. **Si cette sonde
    /// n'imprime que des `None`, c'est la conversion d'octets qu'il faut
    /// suspecter en premier** — l'endianness d'un `GUID` se trompe facilement et
    /// donne un nom plausible mais introuvable, donc un échec silencieux.
    ///
    /// Lecture seule : registre et système de fichiers seulement.
    #[test]
    #[ignore = "lit les fonds d'écran réels de la machine"]
    fn prints_the_real_wallpaper_of_each_desktop() {
        let ids = read_binary_value(windows::core::w!("VirtualDesktopIDs")).unwrap_or_default();
        let count = ids.len() / GUID_SIZE;
        let wallpapers = wallpapers_by_desktop();
        println!("== fonds d'écran par bureau ==");
        println!("{count} bureau(x) dans le registre ; {} entree(s) lue(s)", wallpapers.len());
        for index in 0..count {
            let name = desktop_key_name(&ids[index * GUID_SIZE..(index + 1) * GUID_SIZE]);
            match wallpapers.get(index) {
                Some(Some(path)) => println!("  bureau {index} : {name:?}\n      {}", path.display()),
                Some(None) => println!("  bureau {index} : {name:?}\n      (aucun fond propre, ou ce n'est pas un fichier)"),
                None => println!("  bureau {index} : {name:?}\n      ABSENT du vecteur rendu"),
            }
        }
    }
}
