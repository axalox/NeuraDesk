//! **PAYLOAD INJECTÉ DANS UNE APPLICATION — ÉTAPE 1 : OBSERVER, NE RIEN CHANGER.**
//!
//! DEMANDE EXPLICITE (2026-11-04) : « proposer ces effets » — la voie **B**,
//! celle où NeuraDesk anime lui-même les fenêtres au lieu de laisser NeuraDesk
//! regarder de l'extérieur ce qu'une autre application fait.
//!
//! **POURQUOI IL FAUT ENTRER DANS LE PROCESSUS.** Mesuré (voir
//! `win32::ouverture_fenetres`) : depuis l'extérieur, on ne peut qu'OBSERVER
//! l'affichage d'une fenêtre après qu'elle a eu lieu, puis la cacher — d'où le
//! clignotement de 10 à 200 ms qu'aucun réglage ne supprime. Pour l'empêcher,
//! il faut être dans l'application **avant** qu'elle montre sa fenêtre.
//!
//! **CE MODULE NE MODIFIE RIEN**, exactement comme `shell-hook` à son étape 1.
//! Il sous-classe les fenêtres de premier niveau du processus et journalise les
//! messages qui décideront de la suite : `WM_SHOWWINDOW` (l'affichage),
//! `WM_SYSCOMMAND` (`SC_MINIMIZE` / `SC_RESTORE` / `SC_CLOSE`), `WM_CLOSE`,
//! `WM_DESTROY`. On saura alors **par quel message** chaque geste passe, et
//! s'il est possible de le retenir — avant d'écrire une ligne qui retienne quoi
//! que ce soit.
//!
//! # Pourquoi SOUS-CLASSER plutôt que crocheter `ShowWindow`
//!
//! Le sous-classement (`SetWindowLongPtrW(GWLP_WNDPROC)`) ne touche **aucun
//! octet de code** : il détourne la procédure de la fenêtre, et Windows nous
//! envoie les messages au même titre qu'elle. Un crochet inline patche le
//! prologue de `user32!ShowWindow`, avec un trampoline et un désassembleur de
//! longueur d'instruction — une erreur d'un octet tue l'hôte. C'est la leçon de
//! `shell-hook`, payée deux fois.
//!
//! # Les trois façons dont ce code peut mal tourner
//!
//! 1. **Une panique dans notre proc tuerait l'application hôte** —
//!    `catch_unwind` autour de tout, et le proc d'origine appelé QUOI QU'IL
//!    ARRIVE.
//! 2. **Un proc pointant sur une DLL déchargée** tuerait l'hôte à la première
//!    souris — on défait toutes les sous-classes AVANT de rendre la main, puis
//!    on se décharge depuis notre propre fil (`FreeLibraryAndExitThread`).
//! 3. **Un interblocage** — aucun verrou n'est tenu pendant l'appel au proc
//!    d'origine.
//!
//! Mesure : voir `README.md` du dossier — injecter, lancer les gestes, lire
//! `C:\ndtarget\window-hook-<pid>.log`.

use std::io::Write;
use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::Mutex;

use windows::core::w;
use windows::Win32::Foundation::{GetLastError, HANDLE, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, WPARAM, ERROR_ALREADY_EXISTS};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK, DWMWA_TRANSITIONS_FORCEDISABLED};
use windows::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, FreeLibraryAndExitThread};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, EnumWindows, GetClassNameW, GetPropW, GetWindow, GetWindowLongPtrW, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, IsZoomed, PostMessageW, RemovePropW, SetPropW, SetWindowLongPtrW, GWLP_WNDPROC, GWL_EXSTYLE, GWL_STYLE,
    GW_OWNER, WNDPROC, WS_CAPTION, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_MINIMIZEBOX,
    RegisterWindowMessageW, SC_MAXIMIZE, SC_MINIMIZE, SC_RESTORE, SWP_SHOWWINDOW, WINDOWPOS, WM_CLOSE, WM_DESTROY,
    WM_NCDESTROY, WM_SHOWWINDOW, WM_SYSCOMMAND, WM_WINDOWPOSCHANGING,
};

use minhook::MinHook;

/// Où l'hôte peut nous arrêter, et où il nous parle. Sur `C:` et non dans
/// `%TEMP%` : les deux côtés doivent le voir au même endroit.
const DOSSIER: &str = r"C:\ndtarget";
const ARRET: &str = r"C:\ndtarget\window-hook.stop";

static MODULE: AtomicIsize = AtomicIsize::new(0);
static JOURNAL: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// ⛔ **UNE SEULE COPIE PAR PROCESSUS.** Après une reconstruction, la DLL change
/// d'EMPREINTE et donc de NOM (`window_hook-<empreinte>.dll`) : une nouvelle
/// injection dans une application DÉJÀ vivante charge une SECONDE copie, avec
/// ses propres listes. Deux chaînes de sous-classe sur la même fenêtre =
/// chaque geste traité deux fois (la libération de l'une est retenue par
/// l'autre). Le mutex nommé, pris dès `DllMain`, fait taire les suivantes ;
/// on garde sa poignée jusqu'à la fin du processus.
static UNIQUE: Mutex<Option<isize>> = Mutex::new(None);

/// Les fenêtres qu'on a sous-classées : `hwnd → proc d'origine`. `HWND` n'est
/// pas `Send`, on garde des `isize`.
static SOUS_CLASSEES: Mutex<Vec<(isize, isize)>> = Mutex::new(Vec::new());

/// Cadence de la veille : assez lente pour ne rien coûter, assez rapide pour
/// prendre une fenêtre avant qu'elle n'ait servi.
const PERIODE_MS: u64 = 200;

fn journaliser(ligne: &str) {
    let mut garde = match JOURNAL.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    if let Some(fichier) = garde.as_mut() {
        let _ = writeln!(fichier, "[{:>8}] {ligne}", horodatage());
        let _ = fichier.flush();
    }
}

/// Millisecondes depuis le démarrage du système — un horodatage comparable
/// entre processus, sans dépendre du fuseau ni du format de date.
fn horodatage() -> u64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
}

fn nom_de_message(message: u32) -> Option<&'static str> {
    Some(match message {
        WM_SHOWWINDOW => "WM_SHOWWINDOW",
        WM_SYSCOMMAND => "WM_SYSCOMMAND",
        WM_CLOSE => "WM_CLOSE",
        WM_DESTROY => "WM_DESTROY",
        WM_NCDESTROY => "WM_NCDESTROY",
        WM_WINDOWPOSCHANGING => "WM_WINDOWPOSCHANGING",
        _ => return None,
    })
}

fn classe(hwnd: HWND) -> String {
    let mut tampon = [0u16; 96];
    let n = unsafe { GetClassNameW(hwnd, &mut tampon) }.max(0) as usize;
    String::from_utf16_lossy(&tampon[..n])
}

/// Une fenêtre qui mérite d'être suivie : de premier niveau, sans propriétaire,
/// avec une barre de titre ou un bouton de réduction — la même règle que
/// l'effet d'ouverture de NeuraDesk (`ouverture_fenetres::est_candidate`).
fn est_candidate(hwnd: HWND) -> bool {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let sans_proprietaire = GetWindow(hwnd, GW_OWNER).map_or(true, |p| p.0.is_null());
        let exclus = WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0;
        let se_reduit = style & WS_CAPTION.0 == WS_CAPTION.0 || style & WS_MINIMIZEBOX.0 != 0;
        sans_proprietaire && style & WS_CHILD.0 == 0 && se_reduit && ex & exclus == 0
    }
}

/// **RÉGLAGE DE LA RETENUE.** Fichier d'un nombre : `<ms>`. Absent, la DLL
/// n'est qu'un observateur — c'est l'état par défaut, et celui dans lequel elle
/// est livrée.
///
/// ⭐ **LE CHEMIN DE PRODUCTION EST CELUI DE NEURADESK** :
/// `%APPDATA%\NeuraDesk\window-hook.hold`, écrit par `win32::injection` à partir
/// de la durée réglée pour la fermeture. `REGLAGE` (le dossier de mesure) reste
/// en repli : les harnais de `C:/ndwork` n'ont pas à changer.
const REGLAGE: &str = r"C:\ndtarget\window-hook.hold";

/// Durée de retenue lue dans `REGLAGE`, bornée. Un délai qu'on pourrait écrire
/// sans borne est une application qu'on pourrait empêcher de se fermer.
/// ⭐ **0 VEUT DIRE « OBSERVATEUR »** : c'est ce que NeuraDesk écrit quand les
/// effets de fermeture sont éteints dans les Réglages — la DLL ne retient alors
/// rien du tout, plutôt que de retenir 0 ms (ce qui préviendrait NeuraDesk pour
/// un effet aussitôt refusé).
const RETENUE_MIN_MS: u64 = 0;
const RETENUE_MAX_MS: u64 = 5000;

/// Les libérations qu'on a SOI-MÊME postées, et qui ne doivent donc pas être
/// retenues une seconde fois : `(fenêtre, message, wparam)`. Sans cette liste,
/// chaque libération repasserait par notre proc et se retiendrait à l'infini.
static LIBERATIONS: Mutex<Vec<(isize, u32, usize)>> = Mutex::new(Vec::new());

/// ⛔ **L'APPLICATION REDEMANDE PARFOIS LE GESTE EN TRAITANT LE NÔTRE** —
/// MESURÉ (2026-11-05, Bloc-notes, journal `window-hook-56332.log`) : à la
/// libération, notre `WM_CLOSE` est bien reconnu (« ← LA NÔTRE »), mais un
/// SECOND `WM_CLOSE` arrive à la même milliseconde, envoyé par la fenêtre
/// elle-même : il n'est pas à nous, donc il était retenu à son tour — deux
/// effets de fermeture de 1,6 s, et une fenêtre qui mettait 3,2 s à
/// disparaître. Un geste reçu juste après une libération n'est pas un nouveau
/// geste : on le TRANSMET sans le retenir.
static LIBERES_RECENTS: Mutex<Vec<(isize, std::time::Instant)>> = Mutex::new(Vec::new());

/// Au-delà, un geste est un vrai nouveau geste (l'utilisateur reclique la
/// croix) et se retient normalement.
const SUITE_DE_LA_NOTRE: std::time::Duration = std::time::Duration::from_millis(500);

fn note_la_liberation(cle: isize) {
    let mut liste = LIBERES_RECENTS.lock().unwrap_or_else(|e| e.into_inner());
    liste.retain(|(h, d)| *h != cle && d.elapsed() < SUITE_DE_LA_NOTRE);
    liste.push((cle, std::time::Instant::now()));
}

fn suit_une_liberation(cle: isize) -> bool {
    let mut liste = LIBERES_RECENTS.lock().unwrap_or_else(|e| e.into_inner());
    // ⛔ **NE PAS ÉLIMINER LA FENÊTRE QU'ON CHERCHE** — MESURÉ (2026-11-05) :
    // le filtre gardait « celles qui ne sont PAS cette fenêtre », donc il
    // supprimait l'entrée qu'on venait interroger ; la réponse était TOUJOURS
    // non, et la protection n'a jamais rien empêché (six fermetures retenues
    // d'affilée sur WhatsApp, deux sur le Bloc-notes). On n'élague que le TEMPS.
    liste.retain(|(_, d)| d.elapsed() < SUITE_DE_LA_NOTRE);
    liste.iter().any(|(h, _)| *h == cle)
}

fn chemins_de_retenue() -> [Option<std::path::PathBuf>; 2] {
    let production = std::env::var_os("APPDATA")
        .map(|base| std::path::PathBuf::from(base).join("NeuraDesk").join("window-hook.hold"));
    [production, Some(std::path::PathBuf::from(REGLAGE))]
}

fn retenue_ms() -> Option<u64> {
    for chemin in chemins_de_retenue().into_iter().flatten() {
        let Ok(texte) = std::fs::read_to_string(&chemin) else { continue };
        let Ok(ms) = texte.trim().parse::<u64>() else { continue };
        return Some(ms.clamp(RETENUE_MIN_MS, RETENUE_MAX_MS));
    }
    None
}

/// Ce message est-il un geste qu'on sait retenir, et que NeuraDesk saura
/// animer ? `WM_CLOSE` (fermer), `SC_MINIMIZE` (réduire), `SC_MAXIMIZE`
/// (agrandir), `SC_RESTORE` (restaurer).
///
/// `SC_CLOSE` est délibérément EXCLU : la croix d'une fenêtre envoie
/// `WM_CLOSE`, pas `SC_CLOSE`, et retenir le menu système est le chemin le plus
/// sûr de bloquer une application qui attend une réponse.
///
/// ⛔ **LE GESTE DOIT CHANGER QUELQUE CHOSE.** `SC_MAXIMIZE` sur une fenêtre
/// déjà agrandie et `SC_RESTORE` sur celle qui ne l'est pas ne feraient
/// qu'ajouter une retenue — une fermeture d'application qui traîne pour rien.
/// Et une fenêtre RÉDUITE qui se restaure (`SC_RESTORE` de la barre des tâches)
/// n'est pas retenue : sa capture est vide, son retour ne part pas d'un
/// rectangle mais de l'icône.
fn est_geste(hwnd: HWND, message: u32, wparam: WPARAM) -> bool {
    geste_de(hwnd, message, wparam) != 0
}

/// ⭐ **LE MESSAGE PAR LEQUEL ON PRÉVIENT NEURADESK.** Enregistré des deux
/// côtés par le même NOM (voir `win32::window::message_de_geste`) : un
/// `WM_APP + n` en dur entrerait en collision avec celui d'une autre
/// application. `wParam` = la fenêtre ; `lParam` = `geste | (ms << 8)`.
fn message_de_geste() -> u32 {
    static MESSAGE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe {
        RegisterWindowMessageW(windows::core::w!("NeuraDesk.GesteFenetre"))
    })
}

/// Numéro du geste dans `lParam` — MÊME TABLE des deux côtés (voir
/// `win32::ouverture_fenetres::Geste::depuis_numero`).
const GESTE_FERMER: isize = 1;
const GESTE_REDUIRE: isize = 2;
const GESTE_AGRANDIR: isize = 3;
const GESTE_RESTAURER: isize = 4;

/// Le numéro du geste, ou `0` si ce message n'en est pas un QUI CHANGE QUELQUE
/// CHOSE (voir `est_geste`).
fn geste_de(hwnd: HWND, message: u32, wparam: WPARAM) -> isize {
    match message {
        WM_CLOSE => GESTE_FERMER,
        WM_SYSCOMMAND => match wparam.0 & 0xFFF0 {
            code if code == SC_MINIMIZE as usize => GESTE_REDUIRE,
            code if code == SC_MAXIMIZE as usize && !unsafe { IsZoomed(hwnd) }.as_bool() => GESTE_AGRANDIR,
            code if code == SC_RESTORE as usize
                && !unsafe { IsIconic(hwnd) }.as_bool()
                && unsafe { IsZoomed(hwnd) }.as_bool() =>
            {
                GESTE_RESTAURER
            }
            _ => 0,
        },
        _ => 0,
    }
}

/// ⛔ **LA CLASSE SE COMPARE EXACTEMENT**, pas par préfixe — MESURÉ : le
/// processus porte des dizaines de fenêtres `NeuraDesk…` (`CaptureWindow`,
/// `TooltipWindow`, `Cef…`), dont la première trouvée par `EnumWindows` n'a
/// évidemment pas notre proc. C'est `NeuraDeskDockWindow` — celle de la barre
/// principale — qui traite le message (voir `win32::window::wndproc`).
///
/// On la relit à chaque geste : une poignée périmée ne coûte qu'un
/// `PostMessage` sans effet, alors que la mettre en cache coûterait la
/// notification après un changement d'écran.
fn fenetre_neuradesk() -> Option<HWND> {
    unsafe extern "system" fn chercher(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let trouvee = &mut *(lparam.0 as *mut Option<HWND>);
        if trouvee.is_some() {
            return false.into();
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid != std::process::id() && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            let mut tampon = [0u16; 96];
            let n = unsafe { GetClassNameW(hwnd, &mut tampon) }.max(0) as usize;
            let classe = String::from_utf16_lossy(&tampon[..n]);
            if classe == "NeuraDeskDockWindow" {
                *trouvee = Some(hwnd);
                return false.into();
            }
        }
        true.into()
    }
    let mut trouvee: Option<HWND> = None;
    unsafe {
        let _ = EnumWindows(Some(chercher), LPARAM(&mut trouvee as *mut _ as isize));
    }
    trouvee
}

/// Annonce le geste retenu ; sans effet si NeuraDesk ne tourne pas (le module
/// reste alors strictement un observateur qui retient et libère).
fn annoncer(hwnd: HWND, gesture: isize, ms: u64) {
    let Some(cible) = fenetre_neuradesk() else { return };
    let mots = (gesture & 0xFF) | ((ms.min(5000) as isize) << 8);
    unsafe {
        let _ = PostMessageW(Some(cible), message_de_geste(), WPARAM(hwnd.0 as usize), LPARAM(mots));
    }
}

// ─────────────────────────────────────────────────────────────────────────
// ⭐ VOILE DE LANCEMENT — supprimer le flash de ~0,3 s avant le vol.
//
// DEPUIS L'EXTÉRIEUR, NeuraDesk ne peut cacher une fenêtre neuve qu'APRÈS son
// affichage (`DWMWA_CLOAK` = `E_ACCESSDENIED`, `EVENT_OBJECT_CREATE` livré
// 241 ms en retard) : la vraie fenêtre paraît ~272 ms le temps que
// l'application pompe, avant que le vol ne commence. Windhawk n'a pas ce flash
// parce qu'il vit DANS le processus et voile la fenêtre AVANT `ShowWindow`.
//
// Ici de même : on crochète `user32!ShowWindow` (MinHook) et, au PREMIER
// affichage d'une fenêtre d'application, on la voile par `DWMWA_CLOAK` (PERMIS
// dans son propre processus) avant de laisser l'affichage avoir lieu, on pose
// une propriété que NeuraDesk lit pour laisser le voile à NOTRE charge, et on
// prévient NeuraDesk (geste `LANCER`). NeuraDesk capture par `PrintWindow`
// (le contenu se peint sous le voile — mesuré), fait voler l'image, puis nous
// renvoie le message de dévoilage quand elle se pose. Un garde-fou dévoile de
// toute façon si NeuraDesk ne répond pas.
// ─────────────────────────────────────────────────────────────────────────

/// Le geste « lancement » dans `lParam` — MÊME TABLE que
/// `ouverture_fenetres::Geste` côté NeuraDesk (1..=4 déjà pris).
const GESTE_LANCER: isize = 5;

/// La propriété que porte une fenêtre voilée par nous : NeuraDesk la lit
/// (`GetPropW`, inter-processus) pour laisser à ce module le soin de la cacher
/// plutôt que de la vider de sa région (ce qui casserait `PrintWindow`).
const PROP_VOILE: windows::core::PCWSTR = w!("NeuraDesk.VoileLancement");

/// L'effet d'OUVERTURE côté NeuraDesk : 0 = éteint, sinon la durée du vol en
/// ms. Lu au fichier par la veille (`lire_ouverture`), consulté par le crochet
/// — pas d'I/O sur le chemin chaud de `ShowWindow`.
static OUVERTURE_MS: AtomicU64 = AtomicU64::new(0);

/// Les fenêtres déjà voilées à leur premier affichage : une seule fois chacune
/// (une application rappelle `ShowWindow` souvent).
static LANCEES: Mutex<Vec<isize>> = Mutex::new(Vec::new());

/// Le pointeur d'origine de `ShowWindow`, rendu par MinHook.
static SHOW_WINDOW_ORIG: AtomicIsize = AtomicIsize::new(0);

/// Le message par lequel NeuraDesk nous demande de dévoiler (le vol s'est
/// posé). Même nom des deux côtés (voir `ouverture_fenetres`).
fn message_devoiler() -> u32 {
    static MESSAGE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("NeuraDesk.DevoilerLancement")) })
}

/// Les commandes de `ShowWindow` qui RENDENT une fenêtre visible (premier
/// affichage d'un lancement). `SW_SHOWNORMAL`=1, `SW_SHOWMAXIMIZED`=3,
/// `SW_SHOWNOACTIVATE`=4, `SW_SHOW`=5, `SW_SHOWNA`=8, `SW_RESTORE`=9,
/// `SW_SHOWDEFAULT`=10.
fn montre(cmd: i32) -> bool {
    matches!(cmd, 1 | 3 | 4 | 5 | 8 | 9 | 10)
}

fn voiler(hwnd: HWND, oui: bool) -> bool {
    let v: i32 = oui.into();
    unsafe { DwmSetWindowAttribute(hwnd, DWMWA_CLOAK, &v as *const _ as _, std::mem::size_of::<i32>() as u32) }.is_ok()
}

/// Coupe (ou rétablit) les transitions de DWM sur la fenêtre — sinon elle FOND
/// à l'écran quand on la dévoile, par-dessus la fin du vol (Windhawk fait de
/// même : `UpdateDwmTransitions(FALSE)` au voile, `TRUE` au dévoilage).
fn transitions(hwnd: HWND, coupees: bool) {
    let v: i32 = coupees.into();
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            &v as *const _ as _,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

fn porte_le_voile(hwnd: HWND) -> bool {
    !unsafe { GetPropW(hwnd, PROP_VOILE) }.0.is_null()
}

/// Dévoile une fenêtre voilée par nous et retire sa propriété ; sans effet
/// sinon. Idempotent : le message de NeuraDesk et le garde-fou peuvent tous
/// deux l'appeler.
fn devoiler_lancement(hwnd: HWND) {
    if !porte_le_voile(hwnd) {
        return;
    }
    unsafe {
        let _ = RemovePropW(hwnd, PROP_VOILE);
    }
    voiler(hwnd, false);
    // Rétablir les transitions coupées au voile (voir `transitions`).
    transitions(hwnd, false);
    journaliser(&format!("{:#x} dévoilée (lancement)", hwnd.0 as isize));
}

/// Le crochet de `ShowWindow` : au premier affichage d'une candidate, voile
/// avant de laisser l'affichage se faire. Le chemin chaud (hors lancement) ne
/// fait qu'appeler l'original.
unsafe extern "system" fn show_window_crochet(hwnd: HWND, cmd: i32) -> windows::core::BOOL {
    if OUVERTURE_MS.load(Ordering::Relaxed) != 0 && montre(cmd) {
        // ⛔ Une panique ne doit jamais traverser l'ABI de `user32` : elle
        // tuerait l'hôte. On attrape tout, puis on appelle l'original QUOI
        // QU'IL ARRIVE.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tenter_voile_de_lancement(hwnd)));
    }
    let orig: unsafe extern "system" fn(HWND, i32) -> windows::core::BOOL =
        unsafe { std::mem::transmute(SHOW_WINDOW_ORIG.load(Ordering::SeqCst)) };
    unsafe { orig(hwnd, cmd) }
}

fn tenter_voile_de_lancement(hwnd: HWND) {
    // ⛔ **PREMIER AFFICHAGE D'UNE VRAIE FENÊTRE SEULEMENT** (mêmes gardes que
    // `PrepareLaunchAnim` de Windhawk) :
    // - candidate (de premier niveau, avec cadre ou réductible) ;
    // - pas déjà visible (un ré-affichage n'est pas un lancement) ;
    // - pas RÉDUITE : un `SW_RESTORE` depuis la barre des tâches est dans notre
    //   jeu `montre`, mais c'est une restauration, pas un lancement — elle a son
    //   propre geste (`SC_RESTORE`) ;
    // - pas déjà `WS_EX_LAYERED` : l'application gère alors son propre alpha
    //   (écran d'ouverture GTK/Inkscape) ; Windhawk la laisse aussi de côté.
    if !est_candidate(hwnd)
        || unsafe { IsWindowVisible(hwnd) }.as_bool()
        || unsafe { IsIconic(hwnd) }.as_bool()
    {
        return;
    }
    if unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_LAYERED.0 != 0 {
        return;
    }
    // ⛔ **GECKO CRÉE UNE SECONDE FENÊTRE SOUS VOILE** (noté par Windhawk) : le
    // voile avant affichage casse la restauration de session de Firefox. On la
    // laisse au chemin externe de NeuraDesk, qui sait l'animer autrement.
    if classe(hwnd) == "MozillaWindowClass" {
        return;
    }
    let cle = hwnd.0 as isize;
    {
        let mut vues = LANCEES.lock().unwrap_or_else(|e| e.into_inner());
        if vues.contains(&cle) {
            return;
        }
        if vues.len() >= 256 {
            vues.remove(0);
        }
        vues.push(cle);
    }
    let ms = OUVERTURE_MS.load(Ordering::Relaxed);
    if ms == 0 {
        return;
    }
    // Couper les transitions AVANT le voile : le dévoilage ne fera pas fondre
    // la fenêtre par-dessus la fin du vol (rétabli dans `devoiler_lancement`).
    transitions(hwnd, true);
    if !voiler(hwnd, true) {
        transitions(hwnd, false);
        journaliser(&format!("{cle:#x} voile de lancement refusé"));
        return;
    }
    unsafe {
        let _ = SetPropW(hwnd, PROP_VOILE, Some(HANDLE(1 as *mut _)));
    }
    // Pour recevoir le message de dévoilage sans attendre le tour de veille.
    sous_classer(hwnd);
    annoncer(hwnd, GESTE_LANCER, ms);
    journaliser(&format!("{cle:#x} {:<12} voilée au lancement ({ms} ms)", classe(hwnd)));
    // Garde-fou : si NeuraDesk ne répond pas (fermé, ou effet refusé), on
    // dévoile de toute façon — jamais une fenêtre d'utilisateur invisible pour
    // de bon.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(ms + 2500));
        let hwnd = HWND(cle as *mut std::ffi::c_void);
        if unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            devoiler_lancement(hwnd);
        }
        LANCEES.lock().unwrap_or_else(|e| e.into_inner()).retain(|&h| h != cle);
    });
}

/// Lit la durée de l'effet d'ouverture écrite par NeuraDesk ; 0 (éteint) si
/// absente ou illisible. Même paire de chemins que la retenue.
fn lire_ouverture() -> u64 {
    for chemin in chemins_ouverture().into_iter().flatten() {
        let Ok(texte) = std::fs::read_to_string(&chemin) else { continue };
        let Ok(ms) = texte.trim().parse::<u64>() else { continue };
        return ms.min(RETENUE_MAX_MS);
    }
    0
}

fn chemins_ouverture() -> [Option<std::path::PathBuf>; 2] {
    let production = std::env::var_os("APPDATA")
        .map(|base| std::path::PathBuf::from(base).join("NeuraDesk").join("window-hook.open"));
    [production, Some(std::path::PathBuf::from(r"C:\ndtarget\window-hook.open"))]
}

/// Pose le crochet de `ShowWindow` (MinHook). Appelé par la veille, hors du
/// verrou du chargeur (jamais dans `DllMain`).
fn poser_le_crochet_de_lancement() {
    match unsafe { MinHook::create_hook_api("user32", "ShowWindow", show_window_crochet as *mut std::ffi::c_void) } {
        Ok(orig) => {
            SHOW_WINDOW_ORIG.store(orig as isize, Ordering::SeqCst);
            match unsafe { MinHook::enable_all_hooks() } {
                Ok(()) => journaliser("crochet ShowWindow posé"),
                Err(e) => journaliser(&format!("activation du crochet ShowWindow refusée : {e:?}")),
            }
        }
        Err(e) => journaliser(&format!("crochet ShowWindow impossible : {e:?}")),
    }
}

/// Poste la libération après `ms` et note qu'elle est À NOUS.
///
/// ⛔ **ELLE EST POSTÉE, PAS RENDUE** : le proc doit rendre la main tout de
/// suite, sinon le fil de l'application attend notre sommeil et l'application se
/// fige. Et si la fenêtre a disparu entre-temps, on ne poste rien.
fn retenir_puis_liberer(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, ms: u64) {
    let cle = hwnd.0 as isize;
    let geste = geste_de(hwnd, message, wparam);
    annoncer(hwnd, geste, ms);
    LIBERATIONS.lock().unwrap_or_else(|e| e.into_inner()).push((cle, message, wparam.0));
    // ⭐ **VOILER COMME À L'OUVERTURE** — MESURÉ (2026-11-06, Notepad++) : sans
    // ça, la vraie fenêtre RESTE à l'écran pendant toute la retenue, sous notre
    // image qui s'envole — une « fenêtre fantôme ». On ne voile que les gestes
    // qui FONT DISPARAÎTRE la fenêtre (fermer, réduire) ; agrandir/restaurer la
    // font réapparaître dans un autre rectangle, le voile n'y a pas sa place.
    // `PrintWindow(PW_RENDERFULLCONTENT)` (la capture de `fermeture`) rend le
    // contenu d'une fenêtre voilée — c'est justement ce qui rend ce voile sûr.
    let voile_de_geste = geste == GESTE_FERMER || geste == GESTE_REDUIRE;
    if voile_de_geste {
        transitions(hwnd, true);
        voiler(hwnd, true);
    }
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        let hwnd = HWND(cle as *mut std::ffi::c_void);
        if unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            unsafe {
                let _ = PostMessageW(Some(hwnd), message, wparam, lparam);
            }
            // ⛔ **LA MARQUE EST POSÉE ICI, PAS SEULEMENT QUAND ELLE REPASSE** —
            // MESURÉ (2026-11-05, WhatsApp, `window-hook-14808.log`) : à la
            // libération, l'application envoie son PROPRE `WM_CLOSE`, traité
            // AVANT le nôtre (nos deux messages « ← LA NÔTRE » n'arrivent que
            // 1,6 s plus tard). La marque posée au passage du nôtre arrivait donc
            // trop tard : chaque libération en retenait une de plus — six effets
            // de 1,6 s pour une seule fermeture. Posée au moment du POST, elle
            // couvre le geste de l'application, quel que soit l'ordre.
            // ⛔ **NE RIEN GLISSER AVANT CETTE MARQUE** (surtout pas un `sleep`) :
            // le dévoilage de garde-fou vient APRÈS, exprès.
            note_la_liberation(cle);
            journaliser(&format!("{cle:#x} LIBÉRÉ après {ms} ms : {message:#06x}"));
            // ⭐ **GARDE-FOU DU VOILE DE GESTE** : la fermeture réussie détruit la
            // fenêtre, qui meurt voilée (jamais de clignotement de fin). Mais si
            // elle SURVIT — fermeture arrêtée par un « Enregistrer ? », ou simple
            // réduction qu'on pourra restaurer — on la dévoile, sinon elle
            // resterait invisible pour de bon.
            if voile_de_geste {
                std::thread::sleep(std::time::Duration::from_millis(400));
                if unsafe { IsWindow(Some(hwnd)) }.as_bool() {
                    transitions(hwnd, false);
                    voiler(hwnd, false);
                }
            }
        } else {
            LIBERATIONS.lock().unwrap_or_else(|e| e.into_inner()).retain(|e| *e != (cle, message, wparam.0));
            journaliser(&format!("{cle:#x} disparue avant sa libération — rien posté"));
        }
    });
}

/// Le proc qui remplace celui de la fenêtre. Il note, il retient quand le
/// réglage le demande, puis transmet.
unsafe extern "system" fn proc_sous_classe(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // ⛔ **AUCUN VERROU ICI.** Le proc d'origine peut rappeler notre fenêtre par
    // réentrance sur le même fil : prendre le verrou des sous-classes avant lui
    // serait un interblocage.
    let ancien = {
        let liste = SOUS_CLASSEES.lock().unwrap_or_else(|e| e.into_inner());
        liste.iter().find(|(h, _)| *h == hwnd.0 as isize).map(|(_, p)| *p)
    };
    if let Some(ancien) = ancien {
        // ⭐ NeuraDesk nous demande de dévoiler : le vol s'est posé. On le fait,
        // puis on transmet (message privé enregistré, inoffensif pour l'hôte).
        if message == message_devoiler() {
            let _ = std::panic::catch_unwind(|| devoiler_lancement(hwnd));
            return unsafe {
                CallWindowProcW(std::mem::transmute::<isize, WNDPROC>(ancien), hwnd, message, wparam, lparam)
            };
        }
        // ⭐ **LE VRAI POINT D'ACCROCHE DU VOILE** : beaucoup d'applications
        // (Notepad++, mesuré le 2026-11-06) n'affichent PAS leur fenêtre par
        // `user32!ShowWindow` — le crochet d'API ne les voit jamais. Mais tout
        // chemin d'affichage passe par le proc de la fenêtre : `WM_SHOWWINDOW`
        // (voie `ShowWindow`) ou `WM_WINDOWPOSCHANGING` portant `SWP_SHOWWINDOW`
        // (voie `SetWindowPos`), reçus AVANT que la fenêtre ne devienne visible.
        // On voile là, avant de transmettre. `tenter_voile_de_lancement` est
        // idempotent (garde `LANCEES` + `porte_le_voile`) : le crochet d'API
        // reste en secours sans risque de double voile.
        if OUVERTURE_MS.load(Ordering::Relaxed) != 0 {
            let affichage = match message {
                WM_SHOWWINDOW => wparam.0 != 0,
                WM_WINDOWPOSCHANGING if lparam.0 != 0 => {
                    let wp = unsafe { &*(lparam.0 as *const WINDOWPOS) };
                    wp.flags.0 & SWP_SHOWWINDOW.0 != 0
                }
                _ => false,
            };
            if affichage {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tenter_voile_de_lancement(hwnd)));
            }
        }
        // Une panique ne doit jamais remonter : elle traverserait le proc
        // d'origine et tuerait l'hôte.
        let retenu = std::panic::catch_unwind(|| {
            let geste = est_geste(hwnd, message, wparam);
            let a_nous = geste && {
                let mut liberations = LIBERATIONS.lock().unwrap_or_else(|e| e.into_inner());
                match liberations.iter().position(|e| *e == (hwnd.0 as isize, message, wparam.0)) {
                    Some(i) => {
                        liberations.remove(i);
                        note_la_liberation(hwnd.0 as isize);
                        true
                    }
                    None => false,
                }
            };
            if let Some(nom) = nom_de_message(message) {
                let visible = IsWindowVisible(hwnd).as_bool();
                let quoi = match message {
                    WM_SYSCOMMAND => format!("SC=0x{:04x}", wparam.0 & 0xFFF0),
                    WM_SHOWWINDOW => format!("montre={}", wparam.0 != 0),
                    _ => String::new(),
                };
                journaliser(&format!(
                    "{:#x} {:<12} {:<24} {} (visible={visible}){}",
                    hwnd.0 as isize,
                    classe(hwnd),
                    nom,
                    quoi,
                    if a_nous { " ← LA NÔTRE" } else { "" }
                ));
            }
            if geste && !a_nous {
                if suit_une_liberation(hwnd.0 as isize) {
                    journaliser(&format!(
                        "{:#x} geste reçu juste après notre libération : transmis",
                        hwnd.0 as isize
                    ));
                } else if let Some(ms) = retenue_ms().filter(|ms| *ms > 0) {
                    retenir_puis_liberer(hwnd, message, wparam, lparam, ms);
                    journaliser(&format!("{:#x} RETENU {ms} ms", hwnd.0 as isize));
                    return true;
                }
            }
            false
        })
        .unwrap_or(false);
        if retenu {
            // Le geste n'a PAS été transmis : c'est nous qui le posterons.
            return LRESULT(0);
        }
        return unsafe { CallWindowProcW(std::mem::transmute::<isize, WNDPROC>(ancien), hwnd, message, wparam, lparam) };
    }
    unsafe { windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// Sous-classe `hwnd` si elle est une candidate de NOTRE processus pas encore
/// prise. Appelée par la veille (`enumerer`) ET par le crochet de lancement
/// (`tenter_voile_de_lancement`), qui doit poser son proc TOUT DE SUITE pour
/// recevoir le message de dévoilage sans attendre le prochain tour de veille.
fn sous_classer(hwnd: HWND) {
    let deja = {
        let liste = SOUS_CLASSEES.lock().unwrap_or_else(|e| e.into_inner());
        liste.iter().any(|(h, _)| *h == hwnd.0 as isize)
    };
    if deja {
        return;
    }
    let ancien = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, proc_sous_classe as usize as isize) };
    if ancien != 0 {
        SOUS_CLASSEES.lock().unwrap_or_else(|e| e.into_inner()).push((hwnd.0 as isize, ancien));
        journaliser(&format!("{:#x} {:<12} sous-classée", hwnd.0 as isize, classe(hwnd)));
    }
}

unsafe extern "system" fn enumerer(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    let pid_moi = std::process::id();
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == pid_moi && est_candidate(hwnd) {
        sous_classer(hwnd);
    }
    let _ = lparam;
    true.into()
}

/// Défait TOUTES les sous-classes — la seule façon de se décharger sans laisser
/// une fenêtre pointant sur du code libéré.
fn defaire_tout() {
    let prises: Vec<(isize, isize)> = std::mem::take(&mut *SOUS_CLASSEES.lock().unwrap_or_else(|e| e.into_inner()));
    for (h, ancien) in prises {
        let hwnd = HWND(h as *mut std::ffi::c_void);
        if unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ancien) };
        }
    }
}

/// ⛔ **L'ARRÊT EST DATÉ, PAS SEULEMENT PRÉSENT** — MESURÉ (2026-11-05) : la
/// première version SUPPRIMAIT le fichier d'arrêt à chaque chargement, pour
/// qu'un fichier oublié d'une session précédente ne nous fasse pas sortir
/// aussitôt. Conséquence : la moindre injection suivante annulait une demande
/// d'arrêt en cours — sept applications tenaient encore le module après deux
/// demandes, et la DLL de l'exe restait verrouillée. On retient donc l'instant
/// de notre arrivée : seuls les arrêts ÉCRITS APRÈS comptent.
static ARRIVEE: std::sync::OnceLock<std::time::SystemTime> = std::sync::OnceLock::new();

fn arret_demande() -> bool {
    let Ok(meta) = std::fs::metadata(ARRET) else { return false };
    let Ok(ecrit) = meta.modified() else { return false };
    ecrit > *ARRIVEE.get_or_init(std::time::SystemTime::now)
}

fn surveiller() {
    journaliser(&format!("=== veille démarrée — pid {} ===", std::process::id()));
    // Hors du verrou du chargeur (jamais dans `DllMain`) : MinHook alloue et
    // réécrit du code.
    OUVERTURE_MS.store(lire_ouverture(), Ordering::Relaxed);
    poser_le_crochet_de_lancement();
    loop {
        if arret_demande() {
            journaliser("=== arrêt demandé : sous-classes défaites ===");
            defaire_tout();
            let module = MODULE.load(Ordering::SeqCst);
            if module != 0 {
                unsafe { FreeLibraryAndExitThread(HMODULE(module as *mut _), 0) };
            }
            return;
        }
        // La durée de l'effet d'ouverture peut changer à chaud (Réglages).
        OUVERTURE_MS.store(lire_ouverture(), Ordering::Relaxed);
        unsafe {
            let _ = EnumWindows(Some(enumerer), LPARAM(0));
        }
        std::thread::sleep(std::time::Duration::from_millis(PERIODE_MS));
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(module: HINSTANCE, raison: u32, _reserve: *mut std::ffi::c_void) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    const DLL_PROCESS_DETACH: u32 = 0;
    match raison {
        DLL_PROCESS_ATTACH => {
            MODULE.store(module.0 as isize, Ordering::SeqCst);
            // ⛔ PREMIÈRE CHOSE : sommes-nous la seule copie ? (voir `UNIQUE`)
            let nom: Vec<u16> = format!("Local\\NeuraDesk.WindowHook.{}", std::process::id())
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let verrou = unsafe { CreateMutexW(None, true, windows::core::PCWSTR(nom.as_ptr())) };
            let deja = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
            if let Ok(mut garde) = UNIQUE.lock() {
                *garde = verrou.ok().map(|h| h.0 as isize);
            }
            if deja {
                return 1;
            }
            let _ = std::fs::create_dir_all(DOSSIER);
            // L'instant de notre arrivée : un fichier d'arrêt ANTÉRIEUR est
            // celui d'une session précédente, et ne compte pas (voir
            // `arret_demande`). ⛔ On ne le SUPPRIME plus : c'était ce qui
            // annulait une demande d'arrêt en cours.
            let _ = ARRIVEE.set(std::time::SystemTime::now());
            let chemin = format!(r"{DOSSIER}\window-hook-{}.log", std::process::id());
            let fichier = std::fs::OpenOptions::new().create(true).append(true).open(chemin).ok();
            if let Ok(mut garde) = JOURNAL.lock() {
                *garde = fichier;
            }
            unsafe {
                let _ = DisableThreadLibraryCalls(HMODULE(module.0));
            }
            // `DllMain` ne fait RIEN d'autre que lancer ce fil : sous le verrou
            // du chargeur, tout appel bloquant fige le processus.
            std::thread::spawn(surveiller);
        }
        DLL_PROCESS_DETACH => {
            defaire_tout();
        }
        _ => {}
    }
    1
}
