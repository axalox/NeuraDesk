//! **LE VRAI TOAST DE WINDOWS, DÉPLACÉ SOUS LA BARRE DU HAUT — sans injection.**
//!
//! DEMANDE (2026-10-08) : « faire s'afficher les notifications sous l'horloge au
//! lieu qu'elles s'affichent en bas à droite ». La première réponse coupait les
//! popups (`ToastEnabled = 0`) et les réaffichait dans le bandeau.
//! ⛔ **MESURÉ LE 2026-11-02 : ELLE NE POUVAIT PAS MARCHER.** Une fois la valeur
//! prise en compte (au redémarrage de `WpnUserService_*`), Windows JETTE les
//! toasts : ni bannière, ni ligne dans `wpndatabase`, donc rien à réafficher.
//!
//! # Ce qui est mesuré, et sur quoi ce module repose
//!
//! Piste relevée dans Windhawk (`toast-auto-dismiss`, `notifications-placement`),
//! puis MESURÉE sur la build 26200 par les sondes de `tests` ci-dessous :
//!
//! 1. la bannière est une `Windows.UI.Core.CoreWindow` de
//!    `ShellExperienceHost.exe`, titrée « Nouvelle notification », 396 × 165 en
//!    bas à droite, et la MÊME fenêtre resert d'un toast à l'autre ;
//! 2. ⛔ **`EnumWindows` NE LA REND PAS** ; `FindWindowEx` sur le bureau, si. Les
//!    événements WinEvent, eux, la nomment directement ;
//! 3. ⭐ **UN `SetWindowPos` DEPUIS NOTRE PROCESSUS LA DÉPLACE, DESSIN COMPRIS** —
//!    vu à l'œil sous l'horloge, et tenu jusqu'à l'expiration du toast sans que
//!    le shell ne réaffirme sa position. (Les menus XAML d'explorer, eux, ne
//!    suivaient pas leur fenêtre : ce n'est PAS le même hôte.)
//! 4. à l'expiration, le shell la range en bas à droite avec une HAUTEUR NULLE —
//!    le filtre de taille l'écarte alors, et on ne la touche plus.
//!
//! # Le mécanisme
//!
//! Un fil dédié écoute `EVENT_OBJECT_SHOW` et `EVENT_OBJECT_LOCATIONCHANGE`
//! (hors contexte : RIEN n'est injecté) ; dès que la bannière paraît ou que le
//! shell la repose (un second toast l'agrandit), elle est reposée à la cible.
//! La cible est publiée par `app::run` à chaque frame (`publier_la_cible`), à
//! la même place que le bandeau — même réglage `bandeau::Position`.
//!
//! ⚠️ **NeuraDesk fermé, rien ne se passe** : les toasts restent en bas à droite,
//! comme sur un Windows ordinaire. Aucun état du système n'est modifié.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Mutex, Once};

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetClassNameW, GetMessageW, GetWindowRect, GetWindowThreadProcessId,
    InternalGetWindowText, IsWindowVisible, SetTimer, SetWindowPos, TranslateMessage, EVENT_OBJECT_LOCATIONCHANGE,
    EVENT_OBJECT_SHOW, MSG, OBJID_WINDOW, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS,
};

use crate::win32::bandeau::Position;

/// La classe de la bannière — mesurée.
const CLASSE: &str = "Windows.UI.Core.CoreWindow";
/// Son processus — mesuré, lancé À LA DEMANDE par le shell au premier toast.
const PROCESSUS: &str = "ShellExperienceHost.exe";
/// ⚠️ **LE TITRE EST TRADUIT** (ressource `\ActionCenter\AC_ToastCenter_Title`
/// de `Windows.UI.ShellCommon.<langue>.pri`). Il est ce qui distingue la
/// bannière du CENTRE de notifications, hébergé par le même processus et de la
/// même classe : sans lui, ouvrir le centre le ferait sauter sous l'horloge.
/// Le français est MESURÉ ; l'anglais est celui que relève Windhawk. Une autre
/// langue n'est simplement pas déplacée — l'erreur penche du côté sûr.
const TITRES: [&str; 2] = ["Nouvelle notification", "New notification"];

/// Le fonctionnement est-il demandé (réglage `toast_veille::REGLAGE`) ?
static ACTIF: AtomicBool = AtomicBool::new(false);
static INSTALLE: Once = Once::new();
/// La dernière bannière vue (son `HWND`), `0` sinon — suivie par le tic de la durée.
static SUIVIE: AtomicIsize = AtomicIsize::new(0);
/// La période du tic de la durée.
const TIC_MS: u32 = 250;

/// **LA CIBLE**, publiée par `app::run` à chaque frame, en pixels physiques.
/// ⚠️ **UN SEUL VERROU POUR LES SIX VALEURS** : des atomiques séparées
/// laisseraient le rappel lire un mélange de deux frames (nouveau bord gauche,
/// ancien bord droit). `None` tant qu'aucune frame ne l'a posée : sans cible, on
/// ne déplace rien.
#[derive(Clone, Copy)]
struct Cible {
    position: Position,
    gauche_ecran: i32,
    bord_droit: i32,
    axe_horloge: i32,
    haut: i32,
    marge: i32,
}
static CIBLE: Mutex<Option<Cible>> = Mutex::new(None);

/// **ACTIVE OU ÉTEINT** — à chaud. Le premier appel actif installe le fil ;
/// éteindre ne fait que baisser le drapeau (le crochet hors contexte ne coûte
/// qu'un test par événement, et Windows le retire à la sortie).
pub fn set_actif(actif: bool) {
    ACTIF.store(actif, Ordering::SeqCst);
    if actif {
        assurer_le_crochet();
    }
}

/// **INSTALLE LE FIL ET SES CROCHETS, UNE FOIS** — appelé par le déplacement
/// ET par la durée (`toast_duree`), qui ont chacun besoin de suivre la bannière.
pub fn assurer_le_crochet() {
    INSTALLE.call_once(installer);
}

pub fn est_actif() -> bool {
    ACTIF.load(Ordering::SeqCst)
}

/// **LA CIBLE DE LA FRAME** — appelée par `app::run`, au même endroit que le
/// rectangle du bandeau, avec les mêmes grandeurs.
pub fn publier_la_cible(position: Position, gauche_ecran: i32, bord_droit: i32, axe_horloge: i32, haut: i32, marge: i32) {
    *CIBLE.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(Cible { position, gauche_ecran, bord_droit, axe_horloge, haut, marge });
}

/// **OÙ POSER UNE BANNIÈRE DE `largeur` PIXELS** — pure, testée. Même règle que
/// le bandeau (`app::run`) : à gauche, sous l'horloge (milieu), ou à droite, et
/// TOUJOURS dans l'écran.
pub(crate) fn coin_cible(
    position: Position,
    gauche_ecran: i32,
    bord_droit: i32,
    axe_horloge: i32,
    haut: i32,
    marge: i32,
    largeur: i32,
) -> (i32, i32) {
    let x = match position {
        Position::Gauche => gauche_ecran + marge,
        Position::Droite => bord_droit - largeur - marge,
        Position::Horloge => axe_horloge - largeur / 2,
    };
    let x = x.clamp(gauche_ecran + marge, (bord_droit - largeur - marge).max(gauche_ecran + marge));
    (x, haut)
}

/// **LA TAILLE EST-ELLE CELLE D'UNE BANNIÈRE ?** — pure, testée. Bornes de
/// Windhawk, en pixels effectifs (100 %) convertis au DPI de la fenêtre : au
/// plus la moitié de l'écran en hauteur, et la largeur d'une notification.
/// ⭐ C'est ce qui écarte la fenêtre d'un toast EXPIRÉ (hauteur nulle) et la
/// fenêtre de service de 1 × 1 du même processus.
pub(crate) fn taille_de_banniere(largeur: i32, hauteur: i32, dpi: u32, hauteur_ecran: i32) -> bool {
    let px = |v: i32| v * dpi as i32 / 96;
    let plafond = px(700).min(hauteur_ecran / 2);
    (px(80)..=plafond).contains(&hauteur) && (px(150)..=px(600)).contains(&largeur)
}

pub(crate) fn titre_de_banniere(titre: &str) -> bool {
    TITRES.contains(&titre)
}

fn installer() {
    let lance = std::thread::Builder::new().name("toast-natif".into()).spawn(|| unsafe {
        // ⚠️ **DEUX CROCHETS EXACTS, PAS UNE PLAGE** : `SHOW..LOCATIONCHANGE`
        // couvrirait aussi HIDE, REORDER, FOCUS, SELECTION… — autant d'appels
        // pour rien. `SKIPOWNPROCESS` : nos fenêtres ne sont jamais la bannière.
        let mut poses = 0;
        for evenement in [EVENT_OBJECT_SHOW, EVENT_OBJECT_LOCATIONCHANGE] {
            let crochet = SetWinEventHook(
                evenement,
                evenement,
                None,
                Some(rappel),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            if crochet.is_invalid() {
                crate::diag!("[toast-natif] crochet {evenement:#x} impossible");
            } else {
                poses += 1;
            }
        }
        if poses == 0 {
            return;
        }
        crate::diag!("[toast-natif] crochets installes : {poses}");
        // Le tic de la durée (`toast_duree`) : un minuteur DE FIL, sans fenêtre.
        let _ = SetTimer(None, 0, TIC_MS, Some(tic));
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
    if let Err(erreur) = lance {
        crate::diag!("[toast-natif] fil impossible : {erreur}");
    }
}

unsafe extern "system" fn rappel(_: HWINEVENTHOOK, evenement: u32, hwnd: HWND, objet: i32, enfant: i32, _: u32, _: u32) {
    if (evenement != EVENT_OBJECT_SHOW && evenement != EVENT_OBJECT_LOCATIONCHANGE)
        || objet != OBJID_WINDOW.0
        || enfant != 0
        || hwnd.0.is_null()
    {
        return;
    }
    let deplacer = ACTIF.load(Ordering::Relaxed);
    if !deplacer && crate::win32::toast_duree::duree() == 0 {
        return;
    }
    let Some(rect) = rect_si_banniere(hwnd) else { return };
    // La DURÉE la suit, même quand elle n'est pas déplacée.
    SUIVIE.store(hwnd.0 as isize, Ordering::Relaxed);
    if !deplacer {
        return;
    }
    let Some(cible) = *CIBLE.lock().unwrap_or_else(|e| e.into_inner()) else { return };
    reposer(hwnd, rect, cible);
}

/// **LE TIC DE LA DURÉE**, quatre fois par seconde sur le fil du crochet : la
/// bannière suivie l'est-elle encore ? (Le toast expiré garde sa fenêtre, à
/// hauteur nulle : le filtre de taille dit alors non.)
unsafe extern "system" fn tic(_: HWND, _: u32, _: usize, _: u32) {
    if crate::win32::toast_duree::duree() == 0 {
        return;
    }
    let suivie = SUIVIE.load(Ordering::Relaxed);
    let banniere = (suivie != 0)
        .then(|| HWND(suivie as *mut _))
        .and_then(|hwnd| rect_si_banniere(hwnd).map(|rect| (hwnd, rect)));
    if banniere.is_none() {
        SUIVIE.store(0, Ordering::Relaxed);
    }
    crate::win32::toast_duree::tic(banniere);
}

/// **CETTE FENÊTRE EST-ELLE LA BANNIÈRE ?** — son rectangle si oui. Les filtres
/// vont du moins cher au plus cher : classe, visibilité, TITRE, processus (il
/// ouvre un handle), voile, taille. Pas de cache par pid : un pid réutilisé
/// hériterait du verdict d'un autre processus, et le filtre de titre laisse
/// passer si peu de fenêtres que l'ouverture ne coûte rien.
fn rect_si_banniere(hwnd: HWND) -> Option<RECT> {
    let mut tampon = [0u16; 40];
    let n = unsafe { GetClassNameW(hwnd, &mut tampon) };
    if String::from_utf16_lossy(&tampon[..n.max(0) as usize]) != CLASSE {
        return None;
    }
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return None;
    }
    let mut titre = [0u16; 64];
    let n = unsafe { InternalGetWindowText(hwnd, &mut titre) };
    if !titre_de_banniere(&String::from_utf16_lossy(&titre[..n.max(0) as usize])) {
        return None;
    }
    if !est_du_processus(hwnd) {
        return None;
    }
    let mut voile = 0u32;
    unsafe {
        let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut voile as *mut u32 as *mut _, 4);
    }
    if voile != 0 {
        return None;
    }
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let hauteur_ecran = hauteur_de_son_ecran(hwnd)?;
    taille_de_banniere(rect.right - rect.left, rect.bottom - rect.top, dpi, hauteur_ecran).then_some(rect)
}

fn est_du_processus(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid != 0
        && crate::win32::app_identity::process_image_name(pid).is_some_and(|(exe, _)| exe.eq_ignore_ascii_case(PROCESSUS))
}

/// La hauteur de l'écran qui porte la fenêtre — `None` si Windows ne la dit pas.
fn hauteur_de_son_ecran(hwnd: HWND) -> Option<i32> {
    let ecran = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { GetMonitorInfoW(ecran, &mut info) }
        .as_bool()
        .then(|| info.rcMonitor.bottom - info.rcMonitor.top)
}

fn reposer(hwnd: HWND, rect: RECT, cible: Cible) {
    let (largeur, hauteur) = (rect.right - rect.left, rect.bottom - rect.top);
    let (x, y) = coin_cible(
        cible.position,
        cible.gauche_ecran,
        cible.bord_droit,
        cible.axe_horloge,
        cible.haut,
        cible.marge,
        largeur,
    );
    // ⭐ **NOTRE PROPRE DÉPLACEMENT REVIENT EN `LOCATIONCHANGE`** : déjà à la
    // cible, on s'arrête — sans cela, une boucle. (Mesuré : le shell ne
    // réaffirme PAS sa position pendant la vie du toast, donc pas de ping-pong.)
    if (rect.left, rect.top) == (x, y) {
        return;
    }
    let pose = unsafe { SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
    crate::diag!("[toast-natif] banniere {}x{} reposee de ({},{}) a ({x},{y}) : {pose:?}", largeur, hauteur, rect.left, rect.top);
}

#[cfg(test)]
mod decisions {
    use super::*;

    #[test]
    fn sous_l_horloge_la_banniere_est_centree_sur_l_axe() {
        assert_eq!(coin_cible(Position::Horloge, 0, 2560, 1280, 60, 16, 396), (1082, 60));
    }

    #[test]
    fn a_droite_elle_garde_la_marge_du_bord() {
        assert_eq!(coin_cible(Position::Droite, 0, 2560, 1280, 60, 16, 396), (2560 - 396 - 16, 60));
    }

    #[test]
    fn a_gauche_elle_part_de_la_marge() {
        assert_eq!(coin_cible(Position::Gauche, -1280, 0, -640, 40, 16, 396), (-1264, 40));
    }

    /// Une dalle plus étroite que la bannière ne la pose jamais hors champ à gauche.
    #[test]
    fn elle_reste_dans_l_ecran() {
        assert_eq!(coin_cible(Position::Horloge, 0, 300, 150, 60, 16, 396), (16, 60));
    }

    /// Mesures du 2026-11-02 : 396 × 165 au DPI de l'écran principal (120).
    #[test]
    fn la_banniere_mesuree_passe_le_filtre() {
        assert!(taille_de_banniere(396, 165, 120, 1440));
    }

    /// ⛔ Le toast EXPIRÉ (hauteur nulle) et la fenêtre de service 1 × 1 ne sont
    /// jamais déplacés.
    #[test]
    fn le_toast_expire_et_la_fenetre_de_service_sont_ecartes() {
        assert!(!taille_de_banniere(396, 0, 120, 1440));
        assert!(!taille_de_banniere(1, 1, 120, 1440));
    }

    /// Une bande pleine hauteur du même processus (relevée par Windhawk) non plus.
    #[test]
    fn une_bande_pleine_hauteur_est_ecartee() {
        assert!(!taille_de_banniere(396, 1400, 120, 1440));
    }

    #[test]
    fn seul_le_titre_de_la_banniere_est_retenu() {
        assert!(titre_de_banniere("Nouvelle notification"));
        assert!(!titre_de_banniere("Centre de notifications"));
        assert!(!titre_de_banniere(""));
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows::Win32::System::Registry::HKEY_CURRENT_USER;
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationInvokePattern, TreeScope_Descendants,
        UIA_InvokePatternId,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetWindowRect, GetWindowThreadProcessId, InternalGetWindowText,
        IsWindow, IsWindowVisible, SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
    };
    use windows_core::Interface;

    const CLE_PUSH: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\PushNotifications";
    const VALEUR: &str = "ToastEnabled";

    /// Rend `ToastEnabled` tel qu'il était, quoi qu'il arrive dans la sonde
    /// (panique comprise).
    struct RendreLaValeur(Option<u32>);
    impl Drop for RendreLaValeur {
        fn drop(&mut self) {
            let rendu = match self.0 {
                Some(v) => crate::win32::registry_util::write_dword_value(HKEY_CURRENT_USER, CLE_PUSH, VALEUR, v),
                None => crate::win32::registry_util::delete_value(HKEY_CURRENT_USER, CLE_PUSH, VALEUR),
            };
            println!("[sonde] ToastEnabled rendu a {:?} : {rendu}", self.0);
        }
    }

    #[derive(Debug, Clone)]
    struct Candidate {
        hwnd: isize,
        exe: String,
        classe: String,
        titre: String,
        rect: RECT,
        visible: bool,
        voile: u32,
    }

    fn candidates() -> Vec<Candidate> {
        // ⛔ PAS `EnumWindows` : il ne rend PAS la bannière sur cette build
        // (mesuré, et noté par Windhawk) ; `FindWindowEx` sur le bureau, si.
        let mut fenetres: Vec<HWND> = Vec::new();
        let mut h: Option<HWND> = None;
        while let Ok(suivant) = unsafe {
            windows::Win32::UI::WindowsAndMessaging::FindWindowExW(None, h, windows::core::PCWSTR::null(), windows::core::PCWSTR::null())
        } {
            if suivant.0.is_null() { break; }
            fenetres.push(suivant);
            h = Some(suivant);
        }
        let mut found = Vec::new();
        for hwnd in fenetres {
            let mut b = [0u16; 64];
            let n = unsafe { GetClassNameW(hwnd, &mut b) };
            let classe = String::from_utf16_lossy(&b[..n.max(0) as usize]);
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            let exe = crate::win32::app_identity::process_image_name(pid).map(|(e, _)| e).unwrap_or_default();
            let mut t = [0u16; 256];
            let n = unsafe { InternalGetWindowText(hwnd, &mut t) };
            let mut rect = RECT::default();
            unsafe { let _ = GetWindowRect(hwnd, &mut rect); }
            let mut voile = 0u32;
            unsafe {
                let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut voile as *mut u32 as *mut _, 4);
            }
            found.push(Candidate {
                hwnd: hwnd.0 as isize,
                exe,
                classe,
                titre: String::from_utf16_lossy(&t[..n.max(0) as usize]),
                rect,
                visible: unsafe { IsWindowVisible(hwnd) }.as_bool(),
                voile,
            });
        }
        found
    }

    fn etat_shell() -> std::collections::HashMap<isize, (String, String, String, bool, u32, RECT)> {
        candidates()
            .into_iter()
            .filter(|c| { let e = c.exe.to_ascii_lowercase(); e.contains("shell") || e == "explorer.exe" || e.contains("experience") })
            .map(|c| (c.hwnd, (c.exe, c.classe, c.titre, c.visible, c.voile, c.rect)))
            .collect()
    }

    pub(super) fn qui_affiche() {
        let origine = crate::win32::registry_util::read_dword_value(HKEY_CURRENT_USER, CLE_PUSH, VALEUR);
        let _rendre = RendreLaValeur(origine);
        let avant = etat_shell();
        crate::win32::registry_util::delete_value(HKEY_CURRENT_USER, CLE_PUSH, VALEUR);
        std::thread::sleep(Duration::from_millis(1500));
        println!("[qui] envoi : {}", crate::win32::bulle::notifier("Sonde NeuraDesk", "Le voyez-vous ?"));
        let debut = Instant::now();
        let mut vus = std::collections::HashSet::new();
        while debut.elapsed() < Duration::from_secs(12) {
            for (h, etat) in etat_shell() {
                let change = avant.get(&h).map(|a| a.3 != etat.3 || a.4 != etat.4 || a.5 != etat.5).unwrap_or(true);
                if change && vus.insert((h, etat.3, etat.4, etat.5.left, etat.5.top, etat.5.right, etat.5.bottom)) {
                    println!("[qui] +{} ms {h} {:?}", debut.elapsed().as_millis(), etat);
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub(super) fn seh_pendant() {
        let uia = automation();
        println!("[seh] envoi : {}", crate::win32::bulle::notifier("Sonde NeuraDesk", "Seconde passe"));
        let debut = Instant::now();
        let mut fait_arbre = false;
        while debut.elapsed() < Duration::from_secs(8) {
            for c in candidates().into_iter().filter(|c| c.exe.eq_ignore_ascii_case("ShellExperienceHost.exe") && c.classe == "Windows.UI.Core.CoreWindow") {
                println!("[seh] +{} ms {} {:?} {:?} visible={} voile={}", debut.elapsed().as_millis(), c.hwnd, c.titre, c.rect, c.visible, c.voile);
                if !fait_arbre && debut.elapsed() > Duration::from_millis(1500) && c.visible {
                    let b = arbre_et_bouton(&uia, HWND(c.hwnd as *mut _));
                    println!("[seh] DismissButton dans {} : {}", c.hwnd, b.is_some());
                }
            }
            if debut.elapsed() > Duration::from_millis(1500) { fait_arbre = true; }
            std::thread::sleep(Duration::from_millis(400));
        }
    }

    pub(super) fn toutes_pendant() {
        let cle = |c: &Candidate| (c.visible, c.voile, c.rect.left, c.rect.top, c.rect.right, c.rect.bottom);
        let avant: std::collections::HashMap<isize, _> = candidates().iter().map(|c| (c.hwnd, cle(c))).collect();
        println!("[tout] envoi : {}", crate::win32::bulle::notifier("Sonde NeuraDesk", "Troisieme passe"));
        let debut = Instant::now();
        let mut vus = std::collections::HashSet::new();
        while debut.elapsed() < Duration::from_secs(7) {
            for c in candidates() {
                if c.exe.eq_ignore_ascii_case("neuradesk.exe") { continue; }
                if avant.get(&c.hwnd) == Some(&cle(&c)) { continue; }
                if vus.insert((c.hwnd, cle(&c))) {
                    let mut pid = 0u32;
                    unsafe { GetWindowThreadProcessId(HWND(c.hwnd as *mut _), Some(&mut pid)) };
                    println!("[tout] +{} ms pid={pid} {:?}", debut.elapsed().as_millis(), c);
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub(super) fn deplacer_sous_l_horloge() {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN};
        println!("[dep] envoi : {}", crate::win32::bulle::notifier("Sonde NeuraDesk", "Je dois etre SOUS L'HORLOGE"));
        let debut = Instant::now();
        let mut banniere = None;
        while debut.elapsed() < Duration::from_secs(8) {
            if let Some(c) = candidates().into_iter().find(est_banniere) {
                banniere = Some(c);
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let Some(c) = banniere else {
            println!("[dep] aucune banniere");
            return;
        };
        let hwnd = HWND(c.hwnd as *mut _);
        let largeur = c.rect.right - c.rect.left;
        let x = (unsafe { GetSystemMetrics(SM_CXSCREEN) } - largeur) / 2;
        let y = 60;
        println!("[dep] vue apres {} ms a {:?} ; cible ({x},{y})", debut.elapsed().as_millis(), c.rect);
        let r = unsafe { SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
        println!("[dep] SetWindowPos : {r:?}");
        let tenue = Instant::now();
        let mut dernier = RECT::default();
        while tenue.elapsed() < Duration::from_secs(10) {
            let mut rr = RECT::default();
            unsafe { let _ = GetWindowRect(hwnd, &mut rr); }
            if rr != dernier {
                println!("[dep]   +{} ms rect {rr:?}", tenue.elapsed().as_millis());
                dernier = rr;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let uia = automation();
        if let Some(b) = arbre_et_bouton(&uia, hwnd) {
            let appel = unsafe { b.GetCurrentPattern(UIA_InvokePatternId) }
                .and_then(|m| m.cast::<IUIAutomationInvokePattern>())
                .map(|m| unsafe { m.Invoke() });
            println!("[dep] fermeture : {appel:?}");
        }
    }

    pub(super) fn inventaire() {
        for c in candidates() {
            let e = c.exe.to_ascii_lowercase();
            if e.contains("shell") || e.contains("explorer") || c.classe.contains("CoreWindow") || c.classe.contains("Toast") {
                println!("[inv] {} {} {:?} {:?} {:?} visible={} voile={}", c.hwnd, c.exe, c.classe, c.titre, c.rect, c.visible, c.voile);
            }
        }
    }

    /// Le filtre de Windhawk (`toast-auto-dismiss`) : CoreWindow visible, non
    /// voilée, de ShellExperienceHost, de taille de toast. Rien d'autre ne
    /// peut passer, donc aucune autre fenêtre n'est jamais touchée.
    fn est_banniere(c: &Candidate) -> bool {
        let l = c.rect.right - c.rect.left;
        let h = c.rect.bottom - c.rect.top;
        est_affichee(c)
            && c.classe == "Windows.UI.Core.CoreWindow"
            && c.exe.eq_ignore_ascii_case("ShellExperienceHost.exe")
            && (80..=1400).contains(&h)
            && (150..=1200).contains(&l)
    }

    fn est_affichee(c: &Candidate) -> bool {
        c.visible && c.voile == 0 && c.rect.right > c.rect.left && c.rect.bottom > c.rect.top
    }

    fn automation() -> IUIAutomation {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
            windows::Win32::System::Com::CoCreateInstance(&CUIAutomation, None, windows::Win32::System::Com::CLSCTX_ALL)
                .expect("UIA")
        }
    }

    /// Décrit l'arbre UIA de la fenêtre et rend le `DismissButton` s'il existe.
    fn arbre_et_bouton(uia: &IUIAutomation, hwnd: HWND) -> Option<windows::Win32::UI::Accessibility::IUIAutomationElement> {
        let racine = unsafe { uia.ElementFromHandle(hwnd) }.ok()?;
        let tout = unsafe { uia.CreateTrueCondition() }.ok()?;
        let liste = unsafe { racine.FindAll(TreeScope_Descendants, &tout) }.ok()?;
        let mut bouton = None;
        for i in 0..unsafe { liste.Length() }.unwrap_or(0) {
            let Ok(e) = (unsafe { liste.GetElement(i) }) else { continue };
            let id = unsafe { e.CurrentAutomationId() }.map(|s| s.to_string()).unwrap_or_default();
            let nom = unsafe { e.CurrentName() }.map(|s| s.to_string()).unwrap_or_default();
            let classe = unsafe { e.CurrentClassName() }.map(|s| s.to_string()).unwrap_or_default();
            let genre = unsafe { e.CurrentControlType() }.map(|t| t.0).unwrap_or(0);
            println!("[sonde]   uia #{i} id={id:?} nom={nom:?} classe={classe:?} type={genre}");
            if id == "DismissButton" && bouton.is_none() {
                bouton = Some(e);
            }
        }
        bouton
    }

    /// **LA SONDE** — `cargo test --release -- --ignored --nocapture sonde_banniere_toast`.
    ///
    /// ⚠️ EFFET DE BORD ANNONCÉ : `ToastEnabled` est retiré (popups actives)
    /// pendant ~10 s puis rendu à sa valeur d'origine. Un toast « Sonde
    /// NeuraDesk » est envoyé, déplacé de 300 px vers la gauche, puis fermé.
    #[test]
    #[ignore]
    fn sonde_banniere_toast() {
        let origine = crate::win32::registry_util::read_dword_value(HKEY_CURRENT_USER, CLE_PUSH, VALEUR);
        println!("[sonde] ToastEnabled d'origine : {origine:?}");
        let _rendre = RendreLaValeur(origine);

        let avant: Vec<isize> = candidates().iter().filter(|c| est_affichee(c)).map(|c| c.hwnd).collect();
        println!("[sonde] fenetres affichees avant : {}", avant.len());
        crate::win32::registry_util::delete_value(HKEY_CURRENT_USER, CLE_PUSH, VALEUR);
        std::thread::sleep(Duration::from_millis(800));

        let pris = crate::win32::bulle::notifier("Sonde NeuraDesk", "Fermeture automatique du toast natif");
        println!("[sonde] toast pris par Windows : {pris}");
        let envoi = Instant::now();

        let mut banniere = None;
        while envoi.elapsed() < Duration::from_secs(8) {
            if let Some(c) = candidates().into_iter().find(est_banniere) {
                println!("[sonde] banniere vue apres {} ms : {c:?}", envoi.elapsed().as_millis());
                banniere = Some(c);
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let Some(c) = banniere else {
            println!("[sonde] AUCUNE fenetre nouvelle affichee en 8 s");
            for c in candidates().iter().filter(|c| c.exe.to_ascii_lowercase().contains("shell")) { println!("[sonde]   shell : {c:?}"); }
            return;
        };
        let hwnd = HWND(c.hwnd as *mut _);
        let uia = automation();

        let debut = Instant::now();
        let bouton = arbre_et_bouton(&uia, hwnd);
        println!("[sonde] arbre lu en {} ms ; DismissButton trouve : {}", debut.elapsed().as_millis(), bouton.is_some());

        // Déplacement externe : la fenêtre suit-elle, et le shell la remet-il ?
        let deplace = unsafe {
            SetWindowPos(hwnd, None, c.rect.left - 300, c.rect.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE)
        };
        println!("[sonde] SetWindowPos externe : {deplace:?}");
        for attente in [50u64, 300, 1000] {
            std::thread::sleep(Duration::from_millis(attente));
            let mut r = RECT::default();
            unsafe { let _ = GetWindowRect(hwnd, &mut r); }
            println!("[sonde]   rect apres +{attente} ms : {r:?} (origine {:?})", c.rect);
        }

        let Some(bouton) = bouton else { return };
        let motif = unsafe { bouton.GetCurrentPattern(UIA_InvokePatternId) }
            .and_then(|m| m.cast::<IUIAutomationInvokePattern>());
        let appel = motif.map(|m| unsafe { m.Invoke() });
        println!("[sonde] Invoke(DismissButton) : {appel:?}");
        let fermeture = Instant::now();
        while fermeture.elapsed() < Duration::from_secs(3) {
            let encore = candidates().into_iter().any(|x| x.hwnd == c.hwnd && est_affichee(&x));
            if !encore {
                println!("[sonde] banniere disparue apres {} ms (IsWindow={})", fermeture.elapsed().as_millis(), unsafe { IsWindow(Some(hwnd)) }.as_bool());
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        println!("[sonde] la banniere est TOUJOURS affichee 3 s apres Invoke");
    }
}

#[cfg(test)]
mod inventaire {
    /// `cargo test --release -- --ignored --nocapture sonde_deplacer_sous_l_horloge`
    #[test]
    #[ignore]
    fn sonde_deplacer_sous_l_horloge() {
        super::tests::deplacer_sous_l_horloge();
    }

    /// `cargo test --release -- --ignored --nocapture sonde_toutes_fenetres_pendant_le_toast`
    #[test]
    #[ignore]
    fn sonde_toutes_fenetres_pendant_le_toast() {
        super::tests::toutes_pendant();
    }

    /// `cargo test --release -- --ignored --nocapture sonde_seh_pendant_le_toast`
    #[test]
    #[ignore]
    fn sonde_seh_pendant_le_toast() {
        super::tests::seh_pendant();
    }

    /// `cargo test --release -- --ignored --nocapture sonde_qui_affiche_le_toast`
    #[test]
    #[ignore]
    fn sonde_qui_affiche_le_toast() {
        super::tests::qui_affiche();
    }

    /// `cargo test --release -- --ignored --nocapture inventaire_fenetres_shell`
    #[test]
    #[ignore]
    fn inventaire_fenetres_shell() {
        super::tests::inventaire();
    }
}

#[cfg(test)]
mod base {
    /// `cargo test --release -- --ignored --nocapture sonde_derniers_toasts_de_la_base`
    #[test]
    #[ignore]
    fn sonde_derniers_toasts_de_la_base() {
        let tous = crate::win32::notification::toasts_depuis(0);
        println!("[base] lecture : {}", if tous.is_some() { "ok" } else { "ILLISIBLE" });
        let tous = tous.unwrap_or_default();
        println!("[base] {} toasts", tous.len());
        for t in tous.iter().rev().take(8) {
            println!("[base] ordre {} arrivee {} aumid={:?} titre={:?} message={} car.", t.order, t.arrivee, t.aumid, t.titre, t.message.chars().count());
        }
    }
}
