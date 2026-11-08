//! **SONDE, et uniquement une sonde** — même statut que `tray_probe` : ce
//! module ne contient AUCUN code de production, n'existe que dans les binaires
//! de test, et n'alourdit donc pas l'application.
//!
//! **CE QU'ELLE MESURE, ET POURQUOI ELLE EXISTE.** La fiche
//! `neuradesk-shell-injection-exploration` consigne que la voie
//! « `CTray::v_WndProc` + `WM_COPYDATA` + signature `0x34753423` » est **MORTE**
//! sur la barre XAML de Windows 11 — donc que l'identité d'une icône de la zone
//! de notification (son `hWnd` et son `uID`) n'est plus atteignable par ce
//! protocole. Or la crate `systray-util` (glzr-io/zebar, MIT, v0.2.0) fait
//! reposer **tout son support Windows 11** sur ce protocole, et son propre code
//! dit que l'autre voie (`ToolbarWindow32` + `ReadProcessMemory`) est
//! **réservée à Windows 10** :
//!
//! > `/// Only available on Windows 10, since tray windows are XAML islands in`
//! > `/// Windows 11.`
//!
//! au point que l'échec est journalisé comme attendu : *« Failed to retrieve
//! initial tray icons. This is expected on W11. »*
//!
//! **Deux verdicts incompatibles.** Celui de ce dépôt visait un **crochet
//! depuis l'intérieur d'`explorer.exe`** ; celui de zebar n'injecte rien : il
//! **enregistre une classe nommée `Shell_TrayWnd`** et gagne le `FindWindow` du
//! shell en se maintenant **au premier rang du Z-order** (un `SetTimer` de
//! 100 ms qui réaffirme `HWND_TOPMOST`). Ce sont **deux techniques distinctes**,
//! et c'est très probablement pourquoi les deux mesures ne se contredisent pas :
//! c'est notre *technique* qui était morte, pas le *protocole*.
//!
//! ⚠️ **ELLE N'EST PAS EN LECTURE SEULE, ET IL FAUT LE DIRE.** Pour recevoir
//! quoi que ce soit, la fenêtre **doit gagner le `FindWindow` du shell** — donc
//! elle **intercepte** le flux, et c'est le **relais** vers la vraie barre qui
//! protège celle de l'utilisateur. Deux garde-fous :
//!
//! * le relais est **inconditionnel et fidèle** — et notamment la branche
//!   `dwData == 3` (`Shell_NotifyIconGetRect`) **rend la réponse de la vraie
//!   barre**. `systray-util`, lui, répond **la position du curseur** à cet
//!   endroit (« `Util::cursor_position` »), ce qui suffit à déplacer le menu
//!   contextuel des applications qui s'en servent — **ce mensonge n'est pas
//!   recopié** ;
//! * le relais suit la **liste blanche de zebar** (`doit_relayer`), et non un
//!   « tout relayer » : envoyer à la vraie barre les messages de gestion de
//!   fenêtre (`WM_NCCALCSIZE`, `WM_PAINT`, `WM_WINDOWPOSCHANGING`…) serait
//!   précisément ce qui pourrait l'abîmer.
//!
//! ⚠️ **CE QU'ELLE NE FAIT PAS, DÉLIBÉRÉMENT** : elle ne diffuse **pas**
//! `TaskbarCreated`. C'est ce que fait zebar pour énumérer les icônes déjà
//! présentes sur Windows 11 — un message global à toutes les fenêtres du
//! bureau, auquel certaines applications ne survivent pas (leur issue #287 :
//! « Some systray icons not detected »). C'est une décision qui revient à
//! l'utilisateur, et elle fera l'objet d'une passe séparée.
//!
//! **Le témoin d'activité, pour ne pas relire une absence comme une preuve.**
//! Une passe passive peut très légitimement ne rien voir — aucune application
//! n'a de raison de toucher son icône pendant douze secondes. Un silence serait
//! alors **ambigu**, et c'est exactement le piège que ce dépôt a payé quatre
//! fois (voir la fiche `absence-de-trace-nest-pas-une-preuve`). La sonde se
//! donne donc son propre trafic : elle appelle `Shell_NotifyIcon(NIM_ADD)` sur
//! **sa propre icône**, marquée **`NIS_HIDDEN`** (donc rangée dans le panneau
//! de débordement, jamais visible dans la zone), puis `NIM_DELETE`. Si cette
//! icône-là revient par `WM_COPYDATA`, le protocole est vivant — et cela ne
//! dépend d'aucune application tierce.
//!
//! Mesure : `cargo test --release --bins -- --ignored --nocapture
//! listens_to_the_tray_message_stream`

use std::sync::atomic::{AtomicIsize, AtomicU8, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_STATE, NIF_TIP, NIM_ADD,
    NIM_DELETE, NIS_HIDDEN, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    FindWindowExW, FindWindowW, GetWindowThreadProcessId, KillTimer, LoadIconW,
    PeekMessageW, PostMessageW, RegisterClassW, SendMessageW, SetTimer,
    SetWindowPos, TranslateMessage, UnregisterClassW, CS_HREDRAW, CS_VREDRAW,
    CW_USEDEFAULT, HWND_TOPMOST, IDI_APPLICATION, MSG, PM_REMOVE,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WM_ACTIVATEAPP, WM_APP, WM_COMMAND,
    WM_COPYDATA, WM_TIMER, WM_USER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

/// Durée de la passe **passive** : on écoute le trafic réel des applications.
const DUREE_PASSIVE: Duration = Duration::from_secs(8);
/// Durée de la passe d'**auto-test**, après l'ajout de notre propre icône.
const DUREE_AUTO_TEST: Duration = Duration::from_secs(4);
/// Cadence de réaffirmation du rang. C'est **ce geste** qui gagne le
/// `FindWindow` du shell : sans lui, la première fenêtre passée au premier plan
/// nous reprendrait la place et le flux cesserait de nous parvenir.
const PERIODE_TIMER_MS: u32 = 100;

/// Notre propre fenêtre, telle que le `wndproc` la reconnaît.
static NOTRE_HWND: AtomicIsize = AtomicIsize::new(0);
/// Passe courante : 0 = passive, 1 = auto-test. Sert à dater chaque événement.
static PHASE: AtomicU8 = AtomicU8::new(0);
static TOTAL_COPYDATA: AtomicUsize = AtomicUsize::new(0);
static COPIES_DW1: AtomicUsize = AtomicUsize::new(0);
static COPIES_DW3: AtomicUsize = AtomicUsize::new(0);
static COPIES_AUTRES: AtomicUsize = AtomicUsize::new(0);
static RELAIS_AUTRES_MESSAGES: AtomicUsize = AtomicUsize::new(0);
/// Relais impossibles : c'est la seule ligne qui dirait qu'on a privé la vraie
/// barre de quelque chose. Elle doit rester à zéro.
static RELAIS_ECHOUES: AtomicUsize = AtomicUsize::new(0);

/// Les événements relevés, **en mémoire seulement**. Aucune entrée-sortie dans
/// le `wndproc` : il est appelé par un `SendMessage` **inter-processus**, donc
/// chaque écriture qu'on y ferait **bloquerait l'émetteur** (`explorer.exe` ou
/// l'application). C'est la leçon du premier plantage d'`explorer` — un
/// récepteur appelé par un processus hôte doit être **gratuit**.
static RECU: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Lit un `u32` à un décalage donné, **seulement s'il tient dans `cbData`**.
///
/// Les octets viennent d'un `WM_COPYDATA` : c'est le noyau qui les a recopiés
/// dans notre espace d'adressage, mais **rien ne garantit leur taille** — un
/// émetteur peut envoyer moins que ce qu'on attend. `read_unaligned` parce que
/// rien ne garantit non plus leur alignement.
unsafe fn lit_u32(base: *const u8, offset: usize, cb: usize) -> Option<u32> {
    if base.is_null() || cb < offset + 4 {
        return None;
    }
    Some(unsafe { std::ptr::read_unaligned(base.add(offset) as *const u32) })
}

/// Lit le `szTip` (128 `u16`) du `NOTIFYICONDATAW` de 32 bits, à `offset`.
///
/// C'est **le champ qui nous intéresse le plus** : c'est le nom de l'icône, et
/// c'est exactement ce qu'UIA rend vide pour les trois entrées sans nom
/// (voir la fiche `neuradesk-tray-icon-identification`).
unsafe fn lit_tip(base: *const u8, offset: usize, cb: usize) -> Option<String> {
    const MOTS: usize = 128;
    if base.is_null() || cb < offset + MOTS * 2 {
        return None;
    }
    let mut buf = [0u16; MOTS];
    for (i, case) in buf.iter_mut().enumerate() {
        *case = unsafe {
            std::ptr::read_unaligned(base.add(offset + i * 2) as *const u16)
        };
    }
    let longueur = buf.iter().position(|&c| c == 0).unwrap_or(0);
    let texte = String::from_utf16_lossy(&buf[..longueur]).replace('\r', "");
    (!texte.is_empty()).then_some(texte)
}

/// Nom du processus qui possède une fenêtre — **c'est le gain concret** : un
/// `window_handle` d'icône nomme enfin l'application qui la possède, là où
/// l'image et l'élimination ont toutes deux échoué sur Discord.
fn nom_du_processus(fenetre: u32) -> String {
    if fenetre == 0 {
        return String::from("(pas de fenêtre)");
    }
    let hwnd = HWND(fenetre as usize as *mut core::ffi::c_void);
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return String::from("(pid inconnu)");
    }

    let Ok(processus) =
        (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
    else {
        return format!("pid {pid} (accès refusé)");
    };

    let mut tampon = [0u16; 512];
    let mut taille = tampon.len() as u32;
    let lu = unsafe {
        QueryFullProcessImageNameW(
            processus,
            PROCESS_NAME_WIN32,
            PWSTR(tampon.as_mut_ptr()),
            &mut taille,
        )
    };
    let _ = unsafe { CloseHandle(processus) };

    if lu.is_err() {
        return format!("pid {pid} (chemin illisible)");
    }
    let chemin = String::from_utf16_lossy(&tampon[..taille as usize]);
    chemin
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(&chemin)
        .to_string()
}

/// La **vraie** barre des tâches, en sautant la nôtre si c'est elle que
/// `FindWindow` rend. On descend le Z-order tant qu'on tombe sur nous-mêmes.
///
/// Résolue **à chaque message** (comme le fait zebar) plutôt qu'une fois : si
/// `explorer.exe` redémarre pendant la mesure, une cible figée deviendrait
/// périmée — et un relais vers une fenêtre morte est exactement ce qui priverait
/// la barre de ses icônes.
fn vraie_barre(nous: isize) -> Option<HWND> {
    let mut courant = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.ok()?;
    while nous != 0 && courant.0 as isize == nous {
        courant =
            unsafe { FindWindowExW(None, Some(courant), w!("Shell_TrayWnd"), PCWSTR::null()) }
                .ok()?;
    }
    Some(courant)
}

/// La liste blanche de zebar, **recopiée telle quelle**. Elle est le fruit de
/// leur mise au point, et l'enjeu — ne pas abîmer la barre de l'utilisateur —
/// justifie de ne pas faire mieux qu'eux à l'aveugle.
fn doit_relayer(msg: u32) -> bool {
    msg == WM_COPYDATA || msg == WM_ACTIVATEAPP || msg == WM_COMMAND || msg >= WM_USER
}

/// Relaie un message à la vraie barre et **rend sa réponse**.
///
/// `WM_COPYDATA` part en `SendMessageW` (synchrone) : c'est ce qui permet à
/// `Shell_NotifyIconGetRect` de recevoir **la vraie** position de l'icône, et
/// non un mensonge.
unsafe fn relayer(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let nous = NOTRE_HWND.load(Ordering::Relaxed);
    let Some(cible) = vraie_barre(nous) else {
        RELAIS_ECHOUES.fetch_add(1, Ordering::Relaxed);
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };

    if msg > WM_USER {
        // Asynchrone : ce sont des messages « feu et oublie », et un
        // `SendMessage` inter-processus y risquerait un blocage mutuel.
        if unsafe { PostMessageW(Some(cible), msg, wparam, lparam) }.is_err() {
            RELAIS_ECHOUES.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    } else {
        unsafe { SendMessageW(cible, msg, Some(wparam), Some(lparam)) }
    }
}

/// Note ce qu'un `WM_COPYDATA` transporte. **Aucune entrée-sortie** — on ne fait
/// que pousser une chaîne dans un `Vec`.
unsafe fn noter(lparam: LPARAM) {
    let Some(copie) = (unsafe { (lparam.0 as *const COPYDATASTRUCT).as_ref() }) else {
        return;
    };
    let dw = copie.dwData;
    let cb = copie.cbData as usize;
    let base = copie.lpData as *const u8;
    let phase = if PHASE.load(Ordering::Relaxed) == 0 {
        "passive"
    } else {
        "auto-test"
    };

    let ligne = match dw {
        1 => {
            let magie = unsafe { lit_u32(base, 0, cb) };
            let type_msg = unsafe { lit_u32(base, 4, cb) };
            let fenetre = unsafe { lit_u32(base, 12, cb) };
            let uid = unsafe { lit_u32(base, 16, cb) };
            let tip = unsafe { lit_tip(base, 32, cb) };
            COPIES_DW1.fetch_add(1, Ordering::Relaxed);

            // `magie` est le cœur de la question : 0x34753423 est la signature
            // que la fiche déclare morte.
            let verdict = match magie {
                Some(0x3475_3423) => "SIGNATURE CONFORME",
                Some(_) => "signature INATTENDUE",
                None => "en-tête tronqué",
            };

            format!(
                "[{phase}] dwData=1 {verdict} — magie={:?} type={} fenêtre={:?} uid={:?} \
                 processus={:?} info={:?} (cbData={cb})",
                magie.map(|m| format!("0x{m:08X}")),
                nom_nim(type_msg.unwrap_or(999)),
                fenetre,
                uid,
                fenetre.map(nom_du_processus).unwrap_or_default(),
                tip,
            )
        }
        3 => {
            COPIES_DW3.fetch_add(1, Ordering::Relaxed);
            format!("[{phase}] dwData=3 (Shell_NotifyIconGetRect) — relayé, vraie réponse rendue")
        }
        autre => {
            COPIES_AUTRES.fetch_add(1, Ordering::Relaxed);
            format!("[{phase}] dwData={autre} (inconnu, relayé)")
        }
    };

    if let Ok(mut recu) = RECU.lock() {
        recu.push(ligne);
    }
}

fn nom_nim(t: u32) -> &'static str {
    match t {
        0 => "NIM_ADD",
        1 => "NIM_MODIFY",
        2 => "NIM_DELETE",
        3 => "NIM_SETFOCUS",
        4 => "NIM_SETVERSION",
        _ => "(inconnu)",
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            // Réaffirme le premier rang : c'est CE geste qui gagne le
            // `FindWindow` du shell, donc la livraison du flux.
            let _ = unsafe {
                SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
            };
            LRESULT(0)
        }
        WM_COPYDATA => {
            TOTAL_COPYDATA.fetch_add(1, Ordering::Relaxed);
            // **Relayer D'ABORD, noter ensuite** : la vraie barre est le chemin
            // critique, notre journal ne l'est pas.
            let reponse = unsafe { relayer(hwnd, msg, wparam, lparam) };
            unsafe { noter(lparam) };
            reponse
        }
        _ => {
            if doit_relayer(msg) {
                RELAIS_AUTRES_MESSAGES.fetch_add(1, Ordering::Relaxed);
                unsafe { relayer(hwnd, msg, wparam, lparam) }
            } else {
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
        }
    }
}

/// Pompe les messages jusqu'à l'échéance. Bornée par le temps, jamais par un
/// `GetMessageW` bloquant : une sonde qui attend indéfiniment est une sonde
/// qu'on ne peut pas arrêter.
fn boucler(duree: Duration) {
    let echeance = Instant::now() + duree;
    while Instant::now() < echeance {
        let mut msg = MSG::default();
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            let _ = unsafe { TranslateMessage(&msg) };
            unsafe { DispatchMessageW(&msg) };
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Ajoute **notre propre** icône, en **`NIS_HIDDEN`** : elle est rangée dans le
/// panneau de débordement, jamais dans la zone visible. Elle existe le temps de
/// la passe d'auto-test, puis est retirée — et de toute façon le mécanisme
/// Windows la retire à la mort du processus.
fn ajouter_icone(fenetre: HWND) -> bool {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = fenetre;
    nid.uID = 0x4E44; // "ND"
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_STATE;
    nid.uCallbackMessage = WM_APP + 1;
    nid.hIcon = unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default();
    for (i, c) in "NeuraDesk — sonde".encode_utf16().enumerate() {
        if i < nid.szTip.len() {
            nid.szTip[i] = c;
        }
    }
    nid.dwState = NIS_HIDDEN;
    nid.dwStateMask = NIS_HIDDEN;
    unsafe { Shell_NotifyIconW(NIM_ADD, &nid) }.as_bool()
}

fn retirer_icone(fenetre: HWND) {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = fenetre;
    nid.uID = 0x4E44;
    let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
}

/// **LE PROTOCOLE `WM_COPYDATA` PORTE-T-IL ENCORE L'IDENTITÉ DES ICÔNES SUR
/// CETTE BUILD DE WINDOWS 11 ?**
///
/// Trois phases, dans cet ordre, et l'ordre compte :
///
/// 1. la vraie barre est résolue **avant** toute prise de rang, donc sans
///    ambiguïté possible avec notre propre fenêtre ;
/// 2. la classe `Shell_TrayWnd` est enregistrée et la fenêtre créée, puis
///    maintenue au premier rang — **c'est la prise de contrôle**, et le relais
///    est ce qui la rend transparente ;
/// 3. passe **passive** (trafic réel des applications), puis **auto-test**
///    (notre propre icône, masquée) qui garantit qu'un silence éventuel ne soit
///    pas lu comme une preuve.
///
/// Mesure : `cargo test --release --bins -- --ignored --nocapture
/// listens_to_the_tray_message_stream`
#[test]
#[ignore]
fn listens_to_the_tray_message_stream() {
    // ── 0. La vraie barre, AVANT toute prise de rang. ────────────────────────
    let Some(barre) = vraie_barre(0) else {
        println!("AUCUNE `Shell_TrayWnd` trouvée — rien à mesurer.");
        return;
    };
    let mut pid_barre = 0u32;
    unsafe { GetWindowThreadProcessId(barre, Some(&mut pid_barre)) };
    println!("vraie barre : hwnd={:?} pid={pid_barre}", barre.0);

    let hinstance = match unsafe { GetModuleHandleW(PCWSTR::null()) } {
        Ok(m) => HINSTANCE(m.0),
        Err(e) => {
            println!("GetModuleHandleW a échoué : {e}");
            return;
        }
    };

    // ── 1. La classe `Shell_TrayWnd`, la nôtre. ──────────────────────────────
    let classe = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        hInstance: hinstance,
        lpszClassName: w!("Shell_TrayWnd"),
        ..Default::default()
    };
    let atome = unsafe { RegisterClassW(&classe) };
    if atome == 0 {
        println!("RegisterClassW a échoué — la classe est peut-être déjà prise.");
        return;
    }

    // `WS_EX_TOOLWINDOW` (hors Alt-Tab et hors barre), **pas** `WS_EX_APPWINDOW`.
    // `WS_POPUP` + pas de parent = fenêtre de premier niveau, ce qu'exige
    // `FindWindow`. Jamais affichée : elle n'a rien à montrer.
    let nous = match unsafe {
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
    } {
        Ok(h) => h,
        Err(e) => {
            println!("CreateWindowExW a échoué : {e}");
            let _ = unsafe { UnregisterClassW(w!("Shell_TrayWnd"), Some(hinstance)) };
            return;
        }
    };
    NOTRE_HWND.store(nous.0 as isize, Ordering::Relaxed);

    unsafe { SetTimer(Some(nous), 1, PERIODE_TIMER_MS, None) };
    let _ = unsafe {
        SetWindowPos(
            nous,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };

    // ── 2. Avons-nous réellement gagné le `FindWindow` du shell ? ────────────
    let gagnant = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }
        .unwrap_or_default();
    let pris = gagnant.0 as isize == nous.0 as isize;
    println!(
        "notre fenêtre : {:?} — prise de rang : {}",
        nous.0,
        if pris {
            "OUI (FindWindow rend la nôtre — le flux nous arrive)"
        } else {
            "NON (le shell voit toujours la vraie barre — on n'entendra rien)"
        }
    );

    // ── 3. Passe passive : le trafic réel des applications. ──────────────────
    PHASE.store(0, Ordering::Relaxed);
    println!("\n── passe PASSIVE ({:?}) ─────────────────────────", DUREE_PASSIVE);
    boucler(DUREE_PASSIVE);
    let passifs = RECU.lock().map(|r| r.len()).unwrap_or(0);

    // ── 4. Auto-test : notre propre icône, masquée. ──────────────────────────
    PHASE.store(1, Ordering::Relaxed);
    let ajoutee = ajouter_icone(nous);
    println!(
        "\n── auto-test ({:?}) — notre icône (NIS_HIDDEN) ajoutée : {} ──",
        DUREE_AUTO_TEST, ajoutee
    );
    boucler(DUREE_AUTO_TEST);
    retirer_icone(nous);

    // ── 5. Démontage — dans l'ordre inverse de la pose. ──────────────────────
    let _ = unsafe { KillTimer(Some(nous), 1) };
    let _ = unsafe { DestroyWindow(nous) };
    let _ = unsafe { UnregisterClassW(w!("Shell_TrayWnd"), Some(hinstance)) };

    // ── 6. Rapport. ─────────────────────────────────────────────────────────
    let total = TOTAL_COPYDATA.load(Ordering::Relaxed);
    let dw1 = COPIES_DW1.load(Ordering::Relaxed);
    let dw3 = COPIES_DW3.load(Ordering::Relaxed);
    let autres = COPIES_AUTRES.load(Ordering::Relaxed);

    println!("\n══════════════ RÉSULTAT ══════════════");
    println!("prise de rang                 : {}", if pris { "OUI" } else { "NON" });
    println!("WM_COPYDATA reçus             : {total}");
    println!("  dwData=1 (icônes)           : {dw1}");
    println!("  dwData=3 (GetRect)          : {dw3}");
    println!("  autres dwData               : {autres}");
    println!("autres messages relayés       : {}", RELAIS_AUTRES_MESSAGES.load(Ordering::Relaxed));
    println!("relais ÉCHOUÉS (doit être 0)  : {}", RELAIS_ECHOUES.load(Ordering::Relaxed));

    match RECU.lock() {
        Ok(recu) if !recu.is_empty() => {
            println!("\n{} événement(s) :", recu.len());
            for ligne in recu.iter() {
                println!("  {ligne}");
            }
        }
        _ => println!("\naucun événement relevé"),
    }

    println!("\n── LECTURE ──");
    if passifs > 0 {
        println!("Le trafic RÉEL des applications est passé : le protocole est VIVANT,");
        println!("et il porte bien l'identité (fenêtre, uid, info) — donc la fiche");
        println!("`neuradesk-shell-injection-exploration` vise une AUTRE voie.");
    } else if dw1 > 0 {
        println!("Seul notre témoin est revenu : le protocole répond, mais aucune");
        println!("application n'a touché son icône pendant la passe passive.");
        println!("C'est un résultat POSITIF sur le protocole, et il ne dit rien du");
        println!("trafic réel — pour l'énumération il faudrait `TaskbarCreated`.");
    } else {
        println!("Aucun message, témoin compris. Le silence est AMBIGU : soit le");
        println!("protocole ne nous parvient pas (prise de rang ?), soit rien ne l'a");
        println!("sollicité. Chercher d'abord dans les sondes ci-dessus si la prise");
        println!("de rang a bien eu lieu — ne pas conclure « MORT » sans cela.");
    }
}
