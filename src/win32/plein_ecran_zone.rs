//! ⭐ **LE PLEIN ÉCRAN REDIRIGÉ DANS LA ZONE CENTRALE** — DEMANDE EXPLICITE
//! (2026-10-16) : « une option qui permette de rediriger les jeux en plein écran
//! dans la zone centrale de l'app », pour « tout ce qui passe en plein écran »,
//! avec « une notification proposant de passer le jeu en “fenêtré sans bordure”
//! dans ses options » quand c'est impossible.
//!
//! Au lieu de masquer le dock (`Window::sync_fullscreen_visibility`), la fenêtre
//! plein écran est replacée dans la ZONE DE TRAVAIL de son écran — exactement la
//! zone libre, puisque nos barres la réservent comme AppBar (mesuré le
//! 2026-10-16 : travail (83, 40, 2480, 1387) = la zone libre du primaire).
//!
//! # Ce qui ne se redirige pas
//!
//! - **Une fenêtre qui se REMET en plein écran** — c'est ainsi que se trahit un
//!   plein écran exclusif : `ESSAIS_MAX` replacements en `FENETRE_ESSAIS`, puis on
//!   lâche (le dock se masque comme avant) et on prévient — sans quoi on lutterait
//!   contre elle à chaque frame.
//!
//! ⛔ **`SHQueryUserNotificationState` NE DIT PAS « EXCLUSIF »** — MESURÉ le
//! 2026-10-16 sur Project Zomboid (Java/OpenGL, fenêtre `GLFW30`, `borderless=true`
//! dans son `options.ini`) : replacé deux fois sans peine, puis
//! `QUNS_RUNNING_D3D_FULL_SCREEN` est tombé et la première version l'a LÂCHÉ pour
//! toute sa session. Windows rend ce signal pour une simple fenêtre sans bordure
//! à la taille de l'écran. On ne le consulte donc plus : on ESSAIE, et c'est la
//! fenêtre qui dit, en se remettant en plein écran, qu'elle ne se laisse pas
//! placer.
//! - **Une fenêtre plus privilégiée** (jeu lancé en administrateur) : UIPI refuse
//!   `SetWindowPos` ; on lâche et on le note au journal.
//!
//! - ⛔ **Une fenêtre OUTIL** (`WS_EX_TOOLWINDOW`) — MESURÉ le 2026-10-16 : la
//!   sélection de Flameshot (classe `Qt693QWindowIcon`, `ex` 0x88 = outil +
//!   toujours au premier plan) couvre l'écran et était replacée dans la zone : on
//!   ne pouvait plus capturer tout l'écran. Une fenêtre outil n'a pas de bouton
//!   dans la barre des tâches : c'est une surcouche (capture, sélection,
//!   lanceur), jamais une application qu'on regarde.
//!
//! La notification part une fois par exécutable et par session ; pendant un jeu,
//! Windows la range souvent directement dans le centre de notifications.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsWindow, SetWindowPos,
    GWL_EXSTYLE, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER, WS_EX_TOOLWINDOW,
};

/// Replacements tolérés dans `FENETRE_ESSAIS` avant de lâcher une fenêtre.
const ESSAIS_MAX: usize = 3;
const FENETRE_ESSAIS: Duration = Duration::from_secs(10);

static ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn est_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Active ou coupe la redirection ; coupée, les fenêtres replacées reprennent
/// tout leur écran.
pub fn activer(active: bool) {
    let avant = ACTIVE.swap(active, Ordering::Relaxed);
    if avant && !active {
        restaurer_tout();
    }
}

/// Une fenêtre replacée : son rectangle d'origine, ses replacements récents, et
/// si on l'a lâchée.
struct Suivi {
    origine: RECT,
    zone: RECT,
    essais: Vec<Instant>,
    lachee: bool,
}

thread_local! {
    static SUIVIS: RefCell<HashMap<isize, Suivi>> = RefCell::new(HashMap::new());
    /// Les exécutables déjà prévenus cette session.
    static PREVENUS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// Ce qu'on fait d'une fenêtre plein écran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choix {
    /// La replacer dans la zone de travail.
    Replacer,
    /// La laisser (le dock se masque) — et prévenir, avec cette raison.
    Lacher(Raison),
    /// La laisser sans rien dire : pas de zone réservée sur cet écran, ou déjà
    /// lâchée.
    Laisser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Raison {
    SeRemetEnPleinEcran,
}

/// La décision, PURE. `fenetre` : son rectangle ; `essais_recents` : ses
/// replacements dans `FENETRE_ESSAIS`.
///
/// ⚠️ Une fenêtre qui ne COUVRE pas l'écran est laissée : la détection du dock
/// peut rendre le premier plan sur le seul signal du système (voir
/// `window::fullscreen_window_on_monitor`), et on ne déplace pas une fenêtre
/// ordinaire.
fn choisir(fenetre: RECT, travail: RECT, ecran: RECT, essais_recents: usize, lachee: bool) -> Choix {
    let couvre = crate::win32::window::couvre_l_ecran(fenetre, ecran);
    if lachee || !couvre || travail == ecran || travail.right <= travail.left || travail.bottom <= travail.top {
        Choix::Laisser
    } else if essais_recents >= ESSAIS_MAX {
        Choix::Lacher(Raison::SeRemetEnPleinEcran)
    } else {
        Choix::Replacer
    }
}

/// Le texte de la notification.
fn message(exe: &str, raison: Raison) -> (String, String) {
    let pourquoi = match raison {
        Raison::SeRemetEnPleinEcran => "se remet en plein écran chaque fois qu'il est placé dans la zone centrale : il est sans doute en plein écran exclusif.",
    };
    (
        format!("{exe} : plein écran non redirigé"),
        format!("{exe} {pourquoi} Dans ses options graphiques, choisissez le mode « fenêtré sans bordure » (borderless) pour qu'il s'affiche entre les barres."),
    )
}

/// ⭐ Replace dans la zone centrale la fenêtre plein écran `fenetre` ; `true` si
/// elle l'a été — le dock reste alors affiché. Appelée par
/// `sync_fullscreen_visibility` avant de masquer.
pub fn rediriger(fenetre: HWND) -> bool {
    if !est_active() {
        return false;
    }
    if est_une_fenetre_outil(fenetre) {
        return false;
    }
    let Some((ecran, travail)) = ecran_et_travail(fenetre) else { return false };
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(fenetre, &mut rect) }.is_err() {
        return false;
    }
    let maintenant = Instant::now();
    let cle = fenetre.0 as isize;
    let (essais, lachee) = SUIVIS.with(|s| {
        let mut suivis = s.borrow_mut();
        suivis.retain(|h, _| unsafe { IsWindow(Some(HWND(*h as _))) }.as_bool());
        suivis.get_mut(&cle).map_or((0, false), |suivi| {
            suivi.essais.retain(|t| maintenant.duration_since(*t) < FENETRE_ESSAIS);
            (suivi.essais.len(), suivi.lachee)
        })
    });
    match choisir(rect, travail, ecran, essais, lachee) {
        Choix::Laisser => false,
        Choix::Lacher(raison) => {
            lacher(cle, ecran, travail);
            prevenir(fenetre, raison);
            false
        }
        Choix::Replacer => {
            let (l, h) = (travail.right - travail.left, travail.bottom - travail.top);
            let pose = unsafe { SetWindowPos(fenetre, None, travail.left, travail.top, l, h, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER) };
            if let Err(erreur) = pose {
                // UIPI : une fenêtre plus privilégiée (voir la doc de module).
                crate::diag!("[plein ecran] replacement refusé ({erreur}) : {}", exe_de(fenetre));
                lacher(cle, ecran, travail);
                return false;
            }
            crate::diag!("[plein ecran] {} replacé dans la zone centrale", exe_de(fenetre));
            SUIVIS.with(|s| {
                let mut suivis = s.borrow_mut();
                let suivi = suivis.entry(cle).or_insert(Suivi { origine: ecran, zone: travail, essais: Vec::new(), lachee: false });
                suivi.essais.push(maintenant);
                suivi.zone = travail;
            });
            true
        }
    }
}

fn lacher(cle: isize, ecran: RECT, travail: RECT) {
    SUIVIS.with(|s| {
        s.borrow_mut().entry(cle).or_insert(Suivi { origine: ecran, zone: travail, essais: Vec::new(), lachee: true }).lachee = true;
    });
}

fn prevenir(fenetre: HWND, raison: Raison) {
    let exe = exe_de(fenetre);
    let nouveau = PREVENUS.with(|p| p.borrow_mut().insert(exe.to_lowercase()));
    if nouveau {
        crate::diag!("[plein ecran] non redirigé ({raison:?}) : {exe}");
        let (titre, texte) = message(&exe, raison);
        crate::win32::bulle::notifier(&titre, &texte);
    }
}

/// Les fenêtres encore dans leur zone reprennent tout leur écran.
fn restaurer_tout() {
    let suivis = SUIVIS.with(|s| std::mem::take(&mut *s.borrow_mut()));
    for (cle, suivi) in suivis {
        let hwnd = HWND(cle as _);
        let mut rect = RECT::default();
        let encore_la = unsafe { IsWindow(Some(hwnd)).as_bool() && GetWindowRect(hwnd, &mut rect).is_ok() } && rect == suivi.zone;
        if !suivi.lachee && encore_la {
            let o = suivi.origine;
            let _ = unsafe { SetWindowPos(hwnd, None, o.left, o.top, o.right - o.left, o.bottom - o.top, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER) };
        }
    }
}

fn est_une_fenetre_outil(fenetre: HWND) -> bool {
    let ex = unsafe { GetWindowLongPtrW(fenetre, GWL_EXSTYLE) } as u32;
    ex & WS_EX_TOOLWINDOW.0 != 0
}

/// « exe (classe) » d'une fenêtre, pour le journal ; la fenêtre au premier plan
/// si `fenetre` est nulle.
pub fn decrire(fenetre: HWND) -> String {
    let fenetre = if fenetre.0.is_null() { unsafe { GetForegroundWindow() } } else { fenetre };
    if fenetre.0.is_null() {
        return "aucune fenêtre".to_string();
    }
    let mut classe = [0u16; 128];
    let n = unsafe { GetClassNameW(fenetre, &mut classe) }.max(0) as usize;
    format!("{} ({})", exe_de(fenetre), String::from_utf16_lossy(&classe[..n]))
}

fn ecran_et_travail(fenetre: HWND) -> Option<(RECT, RECT)> {
    let moniteur = unsafe { MonitorFromWindow(fenetre, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { GetMonitorInfoW(moniteur, &mut info) }.as_bool().then_some((info.rcMonitor, info.rcWork))
}

fn exe_de(fenetre: HWND) -> String {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(fenetre, Some(&mut pid)) };
    crate::win32::app_identity::process_image_name(pid).map_or_else(|| "Cette application".to_string(), |(nom, _)| nom)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ECRAN: RECT = RECT { left: 0, top: 0, right: 2560, bottom: 1440 };
    const TRAVAIL: RECT = RECT { left: 83, top: 40, right: 2480, bottom: 1387 };

    /// Une fenêtre qui couvre l'écran est replacée — sans bordure, ou fenêtrée et
    /// débordant de l'écran par sa barre de titre ; celle qui se remet en plein
    /// écran est lâchée.
    #[test]
    fn la_decision_suit_la_sorte_de_plein_ecran() {
        let fenetree = RECT { left: -8, top: -31, right: ECRAN.right + 8, bottom: ECRAN.bottom + 8 };
        assert_eq!(choisir(ECRAN, TRAVAIL, ECRAN, 0, false), Choix::Replacer);
        assert_eq!(choisir(fenetree, TRAVAIL, ECRAN, 0, false), Choix::Replacer);
        assert_eq!(choisir(ECRAN, TRAVAIL, ECRAN, ESSAIS_MAX - 1, false), Choix::Replacer);
        assert_eq!(choisir(ECRAN, TRAVAIL, ECRAN, ESSAIS_MAX, false), Choix::Lacher(Raison::SeRemetEnPleinEcran));
    }

    /// ⛔ Le signal du système seul ne suffit plus (mesuré sur Project Zomboid) :
    /// on ne déplace qu'une fenêtre qui COUVRE l'écran — jamais une fenêtre
    /// ordinaire, ni une fenêtre déjà dans la zone.
    #[test]
    fn une_fenetre_qui_ne_couvre_pas_l_ecran_est_laissee() {
        assert_eq!(choisir(TRAVAIL, TRAVAIL, ECRAN, 0, false), Choix::Laisser);
        assert_eq!(choisir(RECT { left: 100, top: 100, right: 900, bottom: 700 }, TRAVAIL, ECRAN, 0, false), Choix::Laisser);
    }

    /// Rien à faire sans zone réservée (aucune barre sur cet écran), ni pour une
    /// fenêtre déjà lâchée — sinon on la lâcherait et préviendrait à chaque frame.
    #[test]
    fn sans_zone_ou_deja_lachee_on_laisse() {
        assert_eq!(choisir(ECRAN, ECRAN, ECRAN, 0, false), Choix::Laisser);
        assert_eq!(choisir(ECRAN, TRAVAIL, ECRAN, 0, true), Choix::Laisser);
        assert_eq!(choisir(ECRAN, RECT { left: 10, top: 10, right: 10, bottom: 900 }, ECRAN, 0, false), Choix::Laisser);
    }

    /// La notification nomme l'exécutable et propose le mode sans bordure.
    #[test]
    fn la_notification_propose_le_mode_sans_bordure() {
        let (titre, texte) = message("jeu.exe", Raison::SeRemetEnPleinEcran);
        assert!(titre.contains("jeu.exe") && texte.starts_with("jeu.exe"));
        assert!(texte.contains("fenêtré sans bordure") && texte.contains("exclusif"), "{texte}");
    }

    /// ⭐ **SUR UNE VRAIE FENÊTRE** — à lancer à la main (`--ignored`), l'app
    /// ouverte : sans ses barres, la zone de travail vaut l'écran et rien ne bouge.
    /// Une fenêtre sans bordure couvrant le primaire est replacée dans la zone de
    /// travail, puis reprend tout l'écran quand on coupe l'option.
    #[test]
    #[ignore]
    fn essai_reel_une_fenetre_sans_bordure_est_replacee() {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, ShowWindow, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE, WS_POPUP};
        let fenetre = unsafe {
            CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!("essai plein ecran"), WS_POPUP, 0, 0, 100, 100, None, None, None, None)
        }
        .expect("fenêtre d'essai");
        let (ecran, travail) = ecran_et_travail(fenetre).unwrap();
        let o = ecran;
        unsafe {
            SetWindowPos(fenetre, None, o.left, o.top, o.right - o.left, o.bottom - o.top, SWP_NOZORDER | SWP_NOACTIVATE).unwrap();
            let _ = ShowWindow(fenetre, SW_SHOWNOACTIVATE);
        }
        let rect = |h: HWND| {
            let mut r = RECT::default();
            unsafe { GetWindowRect(h, &mut r) }.unwrap();
            (r.left, r.top, r.right, r.bottom)
        };
        activer(true);
        let replacee = rediriger(fenetre);
        let apres = rect(fenetre);
        activer(false);
        let rendue = rect(fenetre);
        unsafe { DestroyWindow(fenetre) }.unwrap();
        println!("écran {:?}  travail {:?}  après {apres:?}  rendue {rendue:?}", ecran, travail);
        assert!(replacee, "travail {travail:?} contre écran {ecran:?} : l'app tourne-t-elle ?");
        assert_eq!(apres, (travail.left, travail.top, travail.right, travail.bottom));
        assert_eq!(rendue, (ecran.left, ecran.top, ecran.right, ecran.bottom));
    }

    /// ⭐ Une fenêtre OUTIL couvrant l'écran (la sélection de Flameshot) n'est pas
    /// déplacée — à lancer à la main (`--ignored`), l'app ouverte : sans ses
    /// barres, rien ne serait déplacé de toute façon. ⚠️ Avec
    /// `--test-threads=1` : les essais réels partagent l'interrupteur `ACTIVE`.
    #[test]
    #[ignore]
    fn essai_reel_une_fenetre_outil_reste_en_place() {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, ShowWindow, SW_SHOWNOACTIVATE, WS_EX_TOPMOST, WS_POPUP};
        let fenetre = unsafe {
            CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST, w!("STATIC"), w!("essai outil"), WS_POPUP, 0, 0, 100, 100, None, None, None, None)
        }
        .expect("fenêtre d'essai");
        let (ecran, travail) = ecran_et_travail(fenetre).unwrap();
        let o = ecran;
        unsafe {
            SetWindowPos(fenetre, None, o.left, o.top, o.right - o.left, o.bottom - o.top, SWP_NOZORDER | SWP_NOACTIVATE).unwrap();
            let _ = ShowWindow(fenetre, SW_SHOWNOACTIVATE);
        }
        activer(true);
        let deplacee = rediriger(fenetre);
        activer(false);
        let mut rect = RECT::default();
        unsafe { GetWindowRect(fenetre, &mut rect) }.unwrap();
        unsafe { DestroyWindow(fenetre) }.unwrap();
        assert_ne!(travail, ecran, "l'app doit tourner : sans barres, l'essai ne prouve rien");
        assert!(!deplacee);
        assert_eq!(rect, ecran);
        assert!(decrire(HWND(std::ptr::null_mut())).len() > 3);
    }

    /// Coupée par défaut : rien ne bouge tant que l'utilisateur ne l'a pas cochée.
    #[test]
    fn coupee_elle_ne_replace_rien() {
        activer(false);
        assert!(!rediriger(HWND(std::ptr::null_mut())));
    }
}
