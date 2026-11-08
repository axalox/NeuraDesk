//! **LE FLUX DE LA ZONE DE NOTIFICATION — l'icône EXACTE de chaque application.**
//!
//! **CE QU'IL APPORTE, ET POURQUOI.** La zone de notification miroir
//! (`tray_icons`) ne voit les icônes que de l'EXTÉRIEUR : UI Automation donne un
//! nom et un rectangle, jamais un pixel ; le registre garde une image prise à
//! l'ENREGISTREMENT (périmée — Discord y est encore un logo, alors qu'il affiche
//! un cercle blanc) ; la capture d'écran n'existe que barre révélée. Les
//! applications, elles, envoient à `Shell_TrayWnd` leur `NOTIFYICONDATA` — avec
//! leur `HICON` du moment. Ce module le reçoit.
//!
//! **LA TECHNIQUE EST CELLE DE `tray_spy_probe`, MESURÉE VIVANTE LE 2026-09-21**
//! sur cette machine (voir sa doc, et la fiche
//! `neuradesk-tray-copydata-protocole-vivant`) : une fenêtre de classe
//! `Shell_TrayWnd`, maintenue au premier rang par un minuteur de 100 ms, gagne le
//! `FindWindow` des applications, et **relaie TOUT ce qu'elle reçoit à la vraie
//! barre** — la liste blanche de zebar, et `Shell_NotifyIconGetRect` reçoit la
//! VRAIE réponse (pas le mensonge de `systray-util`). Le relais est recopié de la
//! sonde sans changement : c'est lui qui protège la barre de l'utilisateur.
//!
//! **LA DIFFUSION DE `TaskbarCreated` — DÉCIDÉE PAR L'UTILISATEUR (2026-10-02,
//! « lance l'étape 3 »).** Sans elle, on ne voit que les icônes AJOUTÉES ou
//! MODIFIÉES après le lancement (2 au premier essai). Ce message est celui
//! qu'Explorer diffuse à son redémarrage : chaque application y répond en
//! renvoyant son icône — à NOUS, puisqu'on a gagné le `FindWindow`.
//!
//! ⚠️ **LE RISQUE, CONNU ET ACCEPTÉ** : une application qui y répond mal peut
//! perdre son icône dans la vraie barre jusqu'à son redémarrage (l'issue #287 de
//! zebar). D'où trois gardes : **UNE seule diffusion** par lancement ; **jamais
//! avant d'avoir gagné** le `FindWindow` (sinon les icônes repartiraient vers la
//! vraie barre sans qu'on les voie, pour rien) ; et `SendNotifyMessageW`, qui
//! n'attend aucune application. Réglage `tray_flux_diffusion = false` pour la
//! couper sans éteindre le flux.
//!
//! ⚠️ **NEURADESK CHERCHE LUI-MÊME LA BARRE PAR SON NOM** (`FindWindowW`) : avec
//! cette fenêtre active, il trouverait LA NÔTRE. Tout appel de ce genre passe
//! donc par `vraie_barre`, qui la saute.
//!
//! ⛔ **LE `wndproc` EST APPELÉ PAR UN `SendMessage` INTER-PROCESSUS** : chaque
//! microseconde qu'on y passe bloque l'émetteur (l'application, ou
//! `explorer.exe`). Il ne fait donc que relayer, copier l'icône (`CopyIcon`) et
//! empiler. La conversion en pixels et le nom du processus se font APRÈS, dans
//! la boucle du fil.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::Mutex;

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CopyIcon, CreateWindowExW, DefWindowProcW, DestroyIcon, DispatchMessageW, FindWindowExW, FindWindowW,
    GetMessageW, GetWindowThreadProcessId, KillTimer, PostMessageW, RegisterClassW, RegisterWindowMessageW,
    SendMessageW, SendNotifyMessageW, SetTimer, SetWindowPos, HWND_BROADCAST,
    TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, HICON, HWND_TOPMOST, MSG, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, WM_ACTIVATEAPP, WM_COMMAND, WM_COPYDATA, WM_TIMER, WM_USER, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

/// Cadence de réaffirmation du rang — voir `tray_spy_probe::PERIODE_TIMER_MS`.
const PERIODE_TIMER_MS: u32 = 100;
/// Minuteur du rang (périodique) et minuteur de la diffusion (une fois).
const MINUTEUR_RANG: usize = 1;
const MINUTEUR_DIFFUSION: usize = 2;
/// Délai avant la diffusion : le temps que le minuteur du rang nous ait mis au
/// premier plan. La garde (`a_gagne`) est vérifiée de toute façon.
const DELAI_DIFFUSION_MS: u32 = 1500;
/// La signature des messages d'icône (`dwData == 1`) — mesurée sur les seize
/// messages de la sonde.
const SIGNATURE: u32 = 0x3475_3423;
/// `NOTIFYICONDATAW::uFlags`.
const NIF_MESSAGE: u32 = 0x1;
const NIF_ICON: u32 = 0x2;
const NIF_TIP: u32 = 0x4;
const NIF_STATE: u32 = 0x8;
const NIF_GUID: u32 = 0x20;
/// `NIS_HIDDEN` — l'APPLICATION cache son icône (≠ le choix d'épinglage de
/// l'utilisateur, qui vit dans le registre).
const NIS_HIDDEN: u32 = 0x1;
/// `NIM_*` — le type du message, à l'octet 4.
const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIM_SETVERSION: u32 = 4;
/// Décalages dans le message (en-tête de 8 octets + `NOTIFYICONDATAW` de 32 bits)
/// — ceux de `systray-util` (`ShellTrayMessage`), recoupés avec la sonde :
/// rappel (24), état (288), version (808), GUID (944).
const OFF_RAPPEL: usize = 24;
const OFF_ETAT: usize = 288;
const OFF_VERSION: usize = 808;
const OFF_GUID: usize = 944;

/// Notre fenêtre — `0` tant qu'elle n'existe pas. Lue par `vraie_barre`, depuis
/// n'importe quel fil.
static NOTRE_HWND: AtomicIsize = AtomicIsize::new(0);
/// Monte à chaque changement de `ICONES` : le fil principal ne relit la table
/// que quand ce nombre a bougé.
static REVISION: AtomicU64 = AtomicU64::new(0);
/// Les icônes connues, par identité `(fenêtre, uid)` — **le couple, jamais l'un
/// des deux seul** : `uid = 0` est courant (mesuré sur `nextcloud.exe` et
/// `MEGAsync.exe`).
static ICONES: Mutex<Option<HashMap<(u32, u32), IconeFlux>>> = Mutex::new(None);

/// Une icône telle que son application l'a envoyée.
#[derive(Clone, Debug, PartialEq)]
pub struct IconeFlux {
    /// **L'IDENTITÉ** — la fenêtre et le numéro que l'application a donnés à son
    /// icône. Avec eux, le shell la situe (`rect_de`) sans rien deviner.
    pub fenetre: u32,
    pub uid: u32,
    /// Le GUID, si l'application en a donné un (`NIF_GUID`) — l'identité
    /// prioritaire chez `systray-util`.
    pub guid: Option<windows::core::GUID>,
    /// Le message que l'application attend pour ses clics (`NIF_MESSAGE`) — ce
    /// qui permettra de cliquer SANS simuler la souris.
    pub rappel: Option<u32>,
    /// La version du protocole (`NIM_SETVERSION`, 1 à 4) : elle fixe la forme des
    /// paramètres d'un clic.
    pub version: Option<u32>,
    /// L'application a caché son icône (`NIS_HIDDEN`).
    pub masquee: bool,
    /// Le `szTip` — c'est par lui que le miroir rapproche (UI Automation nomme
    /// une icône par son infobulle).
    pub tip: String,
    /// L'exécutable propriétaire (nom de fichier seul), `""` s'il est illisible.
    pub exe: String,
    /// Les pixels de l'icône, déjà convertis (`gfx::icon::icon_to_rgba`, qui sait
    /// lire une icône sans alpha par son masque).
    pub rgba: Option<(u32, u32, Vec<u8>)>,
}

/// Ce que le `wndproc` empile pour la boucle — le strict nécessaire, lu sans
/// rien appeler.
struct Brut {
    type_msg: u32,
    fenetre: u32,
    uid: u32,
    flags: u32,
    rappel: Option<u32>,
    etat: Option<u32>,
    version: Option<u32>,
    guid: Option<windows::core::GUID>,
    /// Une COPIE (`CopyIcon`) : l'application peut détruire la sienne dès que son
    /// `Shell_NotifyIcon` a rendu la main.
    icone: Option<HICON>,
    tip: Option<String>,
}

thread_local! {
    /// Rempli par le `wndproc`, vidé par la boucle — **le même fil**, donc sans
    /// verrou. `try_borrow_mut` malgré tout : un `wndproc` peut être réentré
    /// (voir la fiche sur la réentrance de `RefCell`).
    static EN_ATTENTE: RefCell<Vec<Brut>> = const { RefCell::new(Vec::new()) };
}

/// **LA VRAIE BARRE DES TÂCHES**, en sautant la nôtre. À utiliser PARTOUT où
/// NeuraDesk cherchait `Shell_TrayWnd` par `FindWindowW`.
pub fn vraie_barre() -> Option<HWND> {
    let nous = NOTRE_HWND.load(Ordering::Relaxed);
    let mut courant = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.ok()?;
    while nous != 0 && courant.0 as isize == nous {
        courant = unsafe { FindWindowExW(None, Some(courant), w!("Shell_TrayWnd"), PCWSTR::null()) }.ok()?;
    }
    (!courant.0.is_null()).then_some(courant)
}

/// Le numéro de la table : il change quand une icône arrive, change ou part.
pub fn revision() -> u64 {
    REVISION.load(Ordering::Relaxed)
}

/// Une copie de la table — à n'appeler que quand `revision` a bougé.
pub fn icones() -> Vec<IconeFlux> {
    ICONES.lock().ok().and_then(|table| table.as_ref().map(|t| t.values().cloned().collect())).unwrap_or_default()
}

/// **LANCE LE FLUX**, sur un fil à lui. Sans effet si la fenêtre ne peut pas
/// être créée — on le dit, et le miroir garde ses sources d'avant.
pub fn demarrer() {
    let lance = std::thread::Builder::new().name("tray-flux".into()).spawn(|| {
        if let Err(raison) = boucle() {
            crate::diag!("[tray-flux] non demarre : {raison}");
        }
    });
    if let Err(e) = lance {
        crate::diag!("[tray-flux] fil non cree : {e}");
    }
}

fn boucle() -> Result<(), String> {
    let hinstance = unsafe { GetModuleHandleW(PCWSTR::null()) }.map(|m| HINSTANCE(m.0)).map_err(|e| e.to_string())?;
    let classe = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        hInstance: hinstance,
        lpszClassName: w!("Shell_TrayWnd"),
        ..Default::default()
    };
    if unsafe { RegisterClassW(&classe) } == 0 {
        return Err("RegisterClassW a echoue".into());
    }
    // `WS_EX_TOOLWINDOW` (hors Alt-Tab et hors barre), jamais `WS_EX_APPWINDOW` ;
    // taille nulle, jamais affichée — voir `tray_spy_probe`.
    let nous = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            w!("Shell_TrayWnd"),
            w!("Shell_TrayWnd"),
            WS_POPUP,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            0,
            0,
            None,
            None,
            Some(hinstance),
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    NOTRE_HWND.store(nous.0 as isize, Ordering::Relaxed);
    unsafe { SetTimer(Some(nous), MINUTEUR_RANG, PERIODE_TIMER_MS, None) };
    if crate::config::load_bool_setting_or("tray_flux_diffusion", true) {
        unsafe { SetTimer(Some(nous), MINUTEUR_DIFFUSION, DELAI_DIFFUSION_MS, None) };
    }
    crate::diag!("[tray-flux] demarre — fenetre {:?}", nous.0);

    let mut exes: HashMap<u32, String> = HashMap::new();
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        let _ = unsafe { TranslateMessage(&msg) };
        unsafe { DispatchMessageW(&msg) };
        traiter_l_attente(&mut exes);
    }
    Ok(())
}

/// Convertit ce que le `wndproc` a empilé, et met la table à jour.
fn traiter_l_attente(exes: &mut HashMap<u32, String>) {
    let lot = EN_ATTENTE.with(|a| a.try_borrow_mut().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default());
    if lot.is_empty() {
        return;
    }
    let Ok(mut table) = ICONES.lock() else { return };
    let table = table.get_or_insert_with(HashMap::new);
    let mut change = false;
    for brut in lot {
        let cle = (brut.fenetre, brut.uid);
        if brut.type_msg == NIM_DELETE {
            change |= table.remove(&cle).is_some();
            continue;
        }
        let rgba = brut.icone.and_then(|icone| {
            let pixels = unsafe { crate::gfx::icon::icon_to_rgba(icone) };
            let _ = unsafe { DestroyIcon(icone) };
            pixels
        });
        let exe = exes.entry(pid_de(brut.fenetre)).or_insert_with_key(|&pid| exe_du_pid(pid)).clone();
        let entree = table.entry(cle).or_insert_with(|| IconeFlux {
            fenetre: brut.fenetre,
            uid: brut.uid,
            guid: None,
            rappel: None,
            version: None,
            masquee: false,
            tip: String::new(),
            exe: String::new(),
            rgba: None,
        });
        // ⚠️ **UN CHANGEMENT N'EST COMPTÉ QUE S'IL EN EST UN** — mesuré : des
        // applications renvoient la MÊME icône chaque seconde, et chaque révision
        // fait reconstruire des textures sur le fil principal.
        let avant = entree.clone();
        entree.exe = exe;
        if brut.flags & NIF_TIP != 0 {
            if let Some(tip) = brut.tip {
                entree.tip = tip;
            }
        }
        if rgba.is_some() {
            entree.rgba = rgba;
        }
        // Comme `systray-util` : un champ absent du message ne remplace pas la
        // valeur connue.
        entree.rappel = brut.rappel.or(entree.rappel);
        entree.version = brut.version.or(entree.version);
        entree.guid = brut.guid.or(entree.guid);
        if let Some(etat) = brut.etat {
            entree.masquee = etat & NIS_HIDDEN != 0;
        }
        change |= *entree != avant;
    }
    if change {
        REVISION.fetch_add(1, Ordering::Relaxed);
    }
}

fn pid_de(fenetre: u32) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(HWND(fenetre as usize as *mut core::ffi::c_void), Some(&mut pid)) };
    pid
}

fn exe_du_pid(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    let Ok(processus) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return String::new();
    };
    let mut tampon = [0u16; 512];
    let mut taille = tampon.len() as u32;
    let lu = unsafe { QueryFullProcessImageNameW(processus, PROCESS_NAME_WIN32, PWSTR(tampon.as_mut_ptr()), &mut taille) };
    let _ = unsafe { CloseHandle(processus) };
    if lu.is_err() {
        return String::new();
    }
    let chemin = String::from_utf16_lossy(&tampon[..taille as usize]);
    chemin.rsplit(['\\', '/']).next().unwrap_or("").to_string()
}

/// Lit un `u32` à `offset`, **seulement s'il tient dans `cb`** — les octets
/// viennent d'un autre processus, rien ne garantit leur taille ni leur
/// alignement.
unsafe fn lit_u32(base: *const u8, offset: usize, cb: usize) -> Option<u32> {
    if base.is_null() || cb < offset + 4 {
        return None;
    }
    Some(unsafe { std::ptr::read_unaligned(base.add(offset) as *const u32) })
}

/// Un GUID (16 octets) à `offset` — même garde de taille.
unsafe fn lit_guid(base: *const u8, offset: usize, cb: usize) -> Option<windows::core::GUID> {
    if base.is_null() || cb < offset + 16 {
        return None;
    }
    Some(unsafe { std::ptr::read_unaligned(base.add(offset) as *const windows::core::GUID) })
}

/// **OÙ LE SHELL AFFICHE CETTE ICÔNE** — par son identité exacte : le GUID s'il
/// existe, sinon le couple (fenêtre, numéro). `Shell_NotifyIconGetRect` est
/// documentée et n'interroge que (voir `tray_icons::rect_of_guid`).
///
/// ⚠️ **À APPELER HORS DU FIL DU FLUX** : la requête passe par NOTRE fenêtre
/// (elle a gagné le `FindWindow`), qui la relaie à la vraie barre — le fil
/// appelant attend donc le fil du flux, jamais l'inverse.
pub fn rect_de(icone: &IconeFlux) -> Option<windows::Win32::Foundation::RECT> {
    use windows::Win32::UI::Shell::{Shell_NotifyIconGetRect, NOTIFYICONIDENTIFIER};
    let identifiant = match icone.guid {
        Some(guid) => NOTIFYICONIDENTIFIER {
            cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: HWND::default(),
            uID: 0,
            guidItem: guid,
        },
        None => NOTIFYICONIDENTIFIER {
            cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: HWND(icone.fenetre as usize as *mut core::ffi::c_void),
            uID: icone.uid,
            guidItem: windows::core::GUID::zeroed(),
        },
    };
    unsafe { Shell_NotifyIconGetRect(&identifiant) }.ok()
}

/// Le `szTip` (128 `u16`) à `offset` — même garde de taille.
unsafe fn lit_tip(base: *const u8, offset: usize, cb: usize) -> Option<String> {
    const MOTS: usize = 128;
    if base.is_null() || cb < offset + MOTS * 2 {
        return None;
    }
    let mut buf = [0u16; MOTS];
    for (i, case) in buf.iter_mut().enumerate() {
        *case = unsafe { std::ptr::read_unaligned(base.add(offset + i * 2) as *const u16) };
    }
    let longueur = buf.iter().position(|&c| c == 0).unwrap_or(MOTS);
    Some(String::from_utf16_lossy(&buf[..longueur]).replace('\r', ""))
}

/// Ce qu'un `WM_COPYDATA` d'icône transporte — disposition du `NOTIFYICONDATAW`
/// de 32 bits après un en-tête de 8 octets, telle que la sonde l'a lue :
/// signature (0), type (4), fenêtre (12), uid (16), drapeaux (20), icône (28),
/// infobulle (32).
unsafe fn empiler(lparam: LPARAM) {
    let Some(copie) = (unsafe { (lparam.0 as *const COPYDATASTRUCT).as_ref() }) else { return };
    if copie.dwData != 1 {
        return;
    }
    let cb = copie.cbData as usize;
    let base = copie.lpData as *const u8;
    if unsafe { lit_u32(base, 0, cb) } != Some(SIGNATURE) {
        return;
    }
    let (Some(type_msg), Some(fenetre), Some(uid), Some(flags)) =
        (unsafe { (lit_u32(base, 4, cb), lit_u32(base, 12, cb), lit_u32(base, 16, cb), lit_u32(base, 20, cb)) })
    else {
        return;
    };
    if !matches!(type_msg, NIM_ADD | NIM_MODIFY | NIM_DELETE | NIM_SETVERSION) {
        return;
    }
    let icone = (flags & NIF_ICON != 0 && type_msg != NIM_DELETE)
        .then(|| unsafe { lit_u32(base, 28, cb) })
        .flatten()
        .filter(|&h| h != 0)
        .and_then(|h| unsafe { CopyIcon(HICON(h as usize as *mut core::ffi::c_void)) }.ok());
    let tip = unsafe { lit_tip(base, 32, cb) };
    let rappel = (flags & NIF_MESSAGE != 0).then(|| unsafe { lit_u32(base, OFF_RAPPEL, cb) }).flatten();
    let etat = (flags & NIF_STATE != 0).then(|| unsafe { lit_u32(base, OFF_ETAT, cb) }).flatten();
    // La version n'a de sens que dans un `NIM_SETVERSION` ; ailleurs le champ est
    // une union avec le délai d'une bulle. 1 à 4, comme `systray-util`.
    let version = (type_msg == NIM_SETVERSION)
        .then(|| unsafe { lit_u32(base, OFF_VERSION, cb) })
        .flatten()
        .filter(|v| (1..=4).contains(v));
    let guid = (flags & NIF_GUID != 0)
        .then(|| unsafe { lit_guid(base, OFF_GUID, cb) })
        .flatten()
        .filter(|g| *g != windows::core::GUID::zeroed());
    // Un `NIM_SETVERSION` ne porte ni icône ni texte : on le traite comme une
    // modification qui ne change que la version.
    let type_msg = if type_msg == NIM_SETVERSION { NIM_MODIFY } else { type_msg };
    let brut = Brut { type_msg, fenetre, uid, flags, rappel, etat, version, guid, icone, tip };
    EN_ATTENTE.with(|a| match a.try_borrow_mut() {
        Ok(mut v) => v.push(brut),
        // Réentrance : on perd CET événement plutôt que l'application — et on
        // rend la copie de l'icône, sinon elle fuirait.
        Err(_) => {
            if let Some(icone) = brut.icone {
                let _ = unsafe { DestroyIcon(icone) };
            }
        }
    });
}

/// **DIFFUSE `TaskbarCreated`** — seulement si `FindWindow` rend bien NOTRE
/// fenêtre (voir la doc du module). Appelée par un `WM_TIMER`, donc POSTÉ : le
/// journal ne bloque personne ici, à la différence du `WM_COPYDATA`.
fn diffuser(nous: HWND) {
    let gagnant = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.unwrap_or_default();
    if gagnant.0 != nous.0 {
        crate::diag!("[tray-flux] diffusion ANNULEE : FindWindow rend {:?}, pas la notre", gagnant.0);
        return;
    }
    let message = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    if message == 0 {
        crate::diag!("[tray-flux] diffusion ANNULEE : RegisterWindowMessageW a echoue");
        return;
    }
    let avant = ICONES.lock().ok().and_then(|t| t.as_ref().map(HashMap::len)).unwrap_or(0);
    let envoye = unsafe { SendNotifyMessageW(HWND_BROADCAST, message, WPARAM(0), LPARAM(0)) }.is_ok();
    crate::diag!("[tray-flux] TaskbarCreated diffuse : {envoye} ({avant} icone(s) connue(s) avant)");
}

/// La liste blanche de zebar — voir `tray_spy_probe::doit_relayer`.
fn doit_relayer(msg: u32) -> bool {
    msg == WM_COPYDATA || msg == WM_ACTIVATEAPP || msg == WM_COMMAND || msg >= WM_USER
}

/// Relaie à la vraie barre et rend SA réponse — recopié de `tray_spy_probe`.
unsafe fn relayer(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(cible) = vraie_barre() else {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };
    if msg > WM_USER {
        let _ = unsafe { PostMessageW(Some(cible), msg, wparam, lparam) };
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    } else {
        unsafe { SendMessageW(cible, msg, Some(wparam), Some(lparam)) }
    }
}

thread_local! {
    /// Combien de fois chaque SORTE de message a été vue — voir `tracer`.
    static VUS: RefCell<HashMap<(u32, usize, usize), u32>> = RefCell::new(HashMap::new());
}

/// ⛔ **SONDE : LA MAXIMISATION CASSÉE** (2026-10-02). Mesuré : avec cette
/// fenêtre, et même SANS diffusion, les fenêtres des autres applications ne
/// prennent plus la bonne taille en plein écran ; sans elle, si. Windows nous
/// adresse donc un message dont la réponse relayée est fausse. Chaque sorte
/// (message, `dwData`, taille) est tracée à ses 3 premières occurrences, avec
/// ce qui a été rendu et, pour `WM_COPYDATA`, le début du contenu brut.
fn tracer(msg: u32, wparam: WPARAM, lparam: LPARAM, reponse: LRESULT, relaye: bool) {
    let (dw_data, cb, apercu) = if msg == WM_COPYDATA {
        match unsafe { (lparam.0 as *const COPYDATASTRUCT).as_ref() } {
            Some(c) => {
                let base = c.lpData as *const u8;
                let mots: Vec<String> = (0..(c.cbData as usize / 4).min(16))
                    .filter_map(|i| unsafe { lit_u32(base, i * 4, c.cbData as usize) })
                    .map(|v| format!("{v:x}"))
                    .collect();
                (c.dwData, c.cbData as usize, mots.join(" "))
            }
            None => (0, 0, String::new()),
        }
    } else {
        (0, 0, String::new())
    };
    if msg == WM_COPYDATA && dw_data == 1 {
        return; // les icônes : connues, et nombreuses
    }
    let n = VUS.with(|v| {
        let Ok(mut v) = v.try_borrow_mut() else { return u32::MAX };
        let n = v.entry((msg, dw_data, cb)).or_insert(0);
        *n += 1;
        *n
    });
    if n <= 3 {
        crate::diag!(
            "[tray-flux/msg] 0x{msg:04x} wParam={:x} lParam={:x} dwData={dw_data} cb={cb} relaye={relaye} -> {:x} [{apercu}]",
            wparam.0,
            lparam.0,
            reponse.0
        );
    }
}

/// **SONDE — LES MESSAGES DE RAPPEL QUE LE FLUX CONNAÎT DÉJÀ.**
///
/// ⭐ **LA DONNÉE EST DÉJÀ LÀ, ET ELLE DORT.** `IconeFlux::rappel` est lu depuis
/// `NIF_MESSAGE` (octet 24 de la structure) et **n'est utilisé nulle part** —
/// alors que sa propre doc dit « ce qui permettra de cliquer SANS simuler la
/// souris ». Le clic droit passerait pourtant par **le mécanisme même du shell** :
///
///     PostMessage(hWnd, uCallbackMessage, uID, MAKELPARAM(WM_RBUTTONUP, 0))
///
/// — l'application croit qu'on a cliqué son icône et ouvre SON menu, où elle
/// veut. **Ni UI Automation, ni position, ni `SendInput`, ni ouverture du
/// panneau de débordement, ni placement** : c'est la voie qui marcherait là où
/// le clic synthétique échoue (l'île XAML du `^` l'ignore, mesuré).
///
/// ⚠️ **CE QU'IL RESTE À VÉRIFIER, ET C'EST TOUT L'OBJET DE CETTE SONDE** : que
/// les applications remplissent vraiment `uCallbackMessage` (`NIF_MESSAGE`), et
/// que les valeurs ressemblent à des messages (`WM_USER + n`, soit `0x400`–`0x800`
/// pour l'essentiel). Sans cette mesure, l'hypothèse resterait une déduction —
/// et cette session a montré ce que valent mes déductions.
///
/// ⚠️ **ET LA COUVERTURE EST PARTIELLE, QUOI QU'IL ARRIVE** : le flux ne voit que
/// les icônes enregistrées PENDANT qu'il écoute, et **les icônes du shell
/// (Volume, Réseau, Horloge) n'y passent jamais**.
pub fn journalise_les_rappels() {
    let Ok(garde) = ICONES.lock() else { return };
    let Some(table) = garde.as_ref() else {
        crate::diag!("[tray-flux/rappel] table VIDÉE — le flux n'a encore rien reçu");
        return;
    };
    crate::diag!("[tray-flux/rappel] {} icone(s) connue(s) du flux :", table.len());
    let mut connus = 0usize;
    for ((fenetre, uid), icone) in table.iter() {
        if icone.rappel.is_some() {
            connus += 1;
        }
        crate::diag!(
            "[tray-flux/rappel]   0x{fenetre:x}/{uid} rappel={:?}{} version={:?} masquee={} exe={:?}",
            icone.rappel,
            match icone.rappel {
                Some(r) if (0x400..=0x800).contains(&r) => "  <- WM_USER+n, plausible",
                Some(_) => "  <- HORS de la plage WM_USER",
                None => "  <- NIF_MESSAGE NON FOURNI",
            },
            icone.version,
            icone.masquee,
            icone.exe
        );
    }
    crate::diag!("[tray-flux/rappel] -> {connus} sur {} portent un message de rappel", table.len());
}

thread_local! {
    static DERNIERE_ZONE: std::cell::Cell<Option<(i32, i32, i32, i32)>> = const { std::cell::Cell::new(None) };
}

/// La zone de travail de l'écran PRINCIPAL, tracée à chaque changement — même
/// sonde que `tracer`.
fn tracer_la_zone_de_travail() {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
    let ecran = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if !unsafe { GetMonitorInfoW(ecran, &mut info) }.as_bool() {
        return;
    }
    let r = info.rcWork;
    let zone = (r.left, r.top, r.right, r.bottom);
    if DERNIERE_ZONE.with(|d| d.replace(Some(zone))) != Some(zone) {
        let m = info.rcMonitor;
        crate::diag!(
            "[tray-flux/zone] travail ({},{})-({},{}) sur l'ecran ({},{})-({},{})",
            r.left, r.top, r.right, r.bottom, m.left, m.top, m.right, m.bottom
        );
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg != WM_TIMER {
        let relaye = doit_relayer(msg);
        let reponse = wndproc_interne(hwnd, msg, wparam, lparam);
        tracer(msg, wparam, lparam, reponse, relaye);
        return reponse;
    }
    wndproc_interne(hwnd, msg, wparam, lparam)
}

fn wndproc_interne(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER if wparam.0 == MINUTEUR_DIFFUSION => {
            // UNE fois : le minuteur est tué avant tout, quoi qu'il arrive ensuite.
            let _ = unsafe { KillTimer(Some(hwnd), MINUTEUR_DIFFUSION) };
            diffuser(hwnd);
            LRESULT(0)
        }
        WM_TIMER => {
            tracer_la_zone_de_travail();
            let _ = unsafe { SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
            LRESULT(0)
        }
        WM_COPYDATA => {
            // **RELAYER D'ABORD** : la vraie barre est le chemin critique.
            let reponse = unsafe { relayer(hwnd, msg, wparam, lparam) };
            unsafe { empiler(lparam) };
            reponse
        }
        _ if doit_relayer(msg) => unsafe { relayer(hwnd, msg, wparam, lparam) },
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// **QUELLE ICÔNE DU FLUX POUR CETTE ENTRÉE DU MIROIR ?** — fonction PURE,
/// testable. UI Automation nomme une icône par son infobulle : on compare la
/// PREMIÈRE LIGNE des deux, sans casse ni espaces de bord.
///
/// ⚠️ **UNE SEULE CORRESPONDANCE, OU AUCUNE** : deux icônes au même texte
/// (deux instances, deux comptes) seraient indiscernables, et prêter l'image de
/// l'une à l'autre vaut moins que garder la source d'avant.
pub fn correspondance(nom: &str, tips: &[&str]) -> Option<usize> {
    let premiere = |texte: &str| texte.lines().next().unwrap_or("").trim().to_lowercase();
    let cible = premiere(nom);
    if cible.is_empty() {
        return None;
    }
    let unique = |trouvees: Vec<usize>| (trouvees.len() == 1).then(|| trouvees[0]);
    let egales: Vec<usize> = tips.iter().enumerate().filter(|(_, tip)| premiere(tip) == cible).map(|(i, _)| i).collect();
    if !egales.is_empty() {
        return unique(egales);
    }
    // **INCLUSION, quand rien n'est égal** — mesuré : UI Automation colle parfois
    // DEUX textes (« PowerToys Awake PowerToys Awake », « Sécurité Windows -
    // Aucune action requise. Sécurité Windows - Actions recommandées. »), et
    // l'infobulle du jour n'en est qu'un morceau. Huit caractères au moins : un
    // fragment court (« Steam ») se retrouverait dans trop de noms.
    const MIN_INCLUSION: usize = 8;
    let incluses: Vec<usize> = tips
        .iter()
        .enumerate()
        .filter(|(_, tip)| {
            let tip = premiere(tip);
            tip.chars().count() >= MIN_INCLUSION && cible.contains(&tip)
        })
        .map(|(i, _)| i)
        .collect();
    unique(incluses)
}

/// **SECOND RAPPROCHEMENT : PAR L'EXÉCUTABLE** — fonction PURE, testable.
///
/// ⛔ **LE TEXTE NE SUFFIT PAS, ET C'EST MESURÉ** (2026-10-02) : UI Automation
/// nomme la case par l'infobulle de l'ENREGISTREMENT, l'application envoie
/// celle d'AUJOURD'HUI — « MEGAsync 5.16.0 » contre « MEGA 5.16.0 », « G HUB
/// Logitech G HUB » contre « Logitech G HUB ». L'exécutable, lui, ne change pas :
/// le miroir le connaît par l'instantané du registre qu'il a rattaché à la case,
/// le flux par la fenêtre de l'icône.
///
/// `exe_case` : l'exécutable rattaché à CETTE case. `exes_cases` : celui de
/// TOUTES les cases (pour refuser un exécutable porté par plusieurs). `exes_flux` :
/// celui de chaque icône du flux. ⚠️ **UNIQUE DES DEUX CÔTÉS, OU RIEN** : une
/// application à deux icônes ne permet pas de savoir laquelle va où.
pub fn correspondance_par_exe(exe_case: &str, exes_cases: &[&str], exes_flux: &[&str]) -> Option<usize> {
    let exe = exe_case.trim().to_lowercase();
    if exe.is_empty() || exes_cases.iter().filter(|e| e.trim().to_lowercase() == exe).count() != 1 {
        return None;
    }
    let mut trouvees = exes_flux.iter().enumerate().filter(|(_, e)| e.trim().to_lowercase() == exe).map(|(i, _)| i);
    let premiere = trouvees.next()?;
    trouvees.next().is_none().then_some(premiere)
}

/// **LE NOM D'UNE CASE ET L'INFOBULLE D'UNE ICÔNE DÉSIGNENT-ILS LA MÊME
/// APPLICATION ?** — fonction PURE, le témoin de `app::situer_une_famille`.
/// Même premier mot, ou l'un contient l'autre (UI Automation colle parfois deux
/// textes) — sans casse. Large exprès : elle sert à REFUSER un relevé faux, pas
/// à rapprocher.
pub fn noms_concordent(nom: &str, tip: &str) -> bool {
    let (n, t) = (nom.trim().to_lowercase(), tip.trim().to_lowercase());
    let premier = |s: &str| s.split_whitespace().next().unwrap_or("").to_string();
    let (pn, pt) = (premier(&n), premier(&t));
    // Un premier mot qui en PRÉFIXE un autre — mesuré : « MEGAsync » contre
    // « MEGA ». Trois caractères au moins, sinon « G » préfixerait « GOG ».
    let prefixe = pn.len().min(pt.len()) >= 3 && (pn.starts_with(&pt) || pt.starts_with(&pn));
    !n.is_empty() && !t.is_empty() && (pn == pt || prefixe || n.contains(&t) || t.contains(&n))
}

#[cfg(test)]
mod tests {
    use super::{correspondance, correspondance_par_exe, noms_concordent};

    /// Les cas MESURÉS : les vrais couples concordent, ceux du relevé faux non.
    #[test]
    fn le_temoin_reconnait_les_vrais_couples_et_refuse_les_faux() {
        assert!(noms_concordent("MEGAsync 5.16.0", "MEGA 5.16.0"));
        assert!(noms_concordent("G HUB Logitech G HUB", "Logitech G HUB"));
        assert!(noms_concordent("Sécurité Windows - Aucune action requise. Sécurité Windows - Actions recommandées.", "Sécurité Windows - Actions recommandées."));
        assert!(noms_concordent("PowerToys Awake PowerToys Awake", "PowerToys Awake"));
        assert!(noms_concordent("Touch Portal 4.6 build 2", "Touch Portal 4.6 build 2"));
        assert!(!noms_concordent("Everything", "Battle.net"));
        assert!(!noms_concordent("Paramètres NVIDIA", "Terminal"));
        assert!(!noms_concordent("WhatsApp", "CPU Set Setter"));
    }

    /// Le cas MESURÉ : textes différents, même exécutable.
    #[test]
    fn l_executable_rapproche_quand_le_texte_a_change() {
        let cases = ["MEGAsync.exe", "lghub_system_tray.exe", ""];
        let flux = ["lghub_system_tray.exe", "MEGAsync.exe"];
        assert_eq!(correspondance_par_exe("megasync.exe", &cases, &flux), Some(1));
        assert_eq!(correspondance_par_exe("", &cases, &flux), None);
    }

    /// ⛔ **UN EXÉCUTABLE PORTÉ PAR DEUX CASES OU DEUX ICÔNES : RIEN.**
    #[test]
    fn un_executable_en_double_ne_rapproche_rien() {
        assert_eq!(correspondance_par_exe("a.exe", &["a.exe", "a.exe"], &["a.exe"]), None);
        assert_eq!(correspondance_par_exe("a.exe", &["a.exe"], &["a.exe", "a.exe"]), None);
    }

    #[test]
    fn l_infobulle_rapproche_sur_sa_premiere_ligne_sans_casse() {
        let tips = ["Nextcloud", "MEGAsync 5.16.0\nÀ jour", "G HUB"];
        assert_eq!(correspondance("megasync 5.16.0", &tips), Some(1));
        assert_eq!(correspondance("  Nextcloud  ", &tips), Some(0));
    }

    /// Le cas MESURÉ : UIA colle deux textes, l'infobulle n'en est qu'un.
    #[test]
    fn une_infobulle_incluse_dans_le_nom_rapproche() {
        let tips = ["PowerToys Awake
Désactivé", "PowerToys v0.101", "Steam"];
        assert_eq!(correspondance("PowerToys Awake PowerToys Awake", &tips), Some(0));
        // Trop court pour une inclusion : « Steam » ne rapproche pas « Steam Deck Tools ».
        assert_eq!(correspondance("Steam Deck Tools", &tips), None);
    }

    /// ⛔ **DEUX ICÔNES AU MÊME TEXTE : AUCUNE** — prêter l'image de l'une à
    /// l'autre serait pire que la source d'avant.
    #[test]
    fn un_texte_ambigu_ou_vide_ne_rapproche_rien() {
        let tips = ["Discord", "Discord", "Signal"];
        assert_eq!(correspondance("Discord", &tips), None);
        assert_eq!(correspondance("", &tips), None);
        assert_eq!(correspondance("Absent", &tips), None);
    }
}
