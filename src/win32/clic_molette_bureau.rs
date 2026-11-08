//! **LE CLIC MOLETTE SUR LE BUREAU** — il ouvre la sphère des fenêtres (voir
//! `win32::cef_fenetres`). DEMANDE du 2026-11-01 : « la même fonctionnalité pour
//! choisir vers quel programme ouvert basculer », ouverte par « 1 b » : un clic
//! molette sur le bureau ou sur la barre du bas. La barre du bas est à nous (son
//! `wndproc` le reçoit) ; le BUREAU est à l'Explorateur, d'où ce crochet.
//!
//! Un crochet `WH_MOUSE_LL` **qui n'avale rien** : il observe, c'est tout — le
//! clic continue vers sa fenêtre, comme si NeuraDesk n'existait pas (voir
//! [[neuradesk-live-input-safety]] : aucune entrée n'est simulée ni retenue).
//!
//! ⛔ **SUR SON PROPRE FIL, JAMAIS SUR LE FIL PRINCIPAL.** Un crochet bas niveau
//! s'exécute dans la boucle de messages du fil qui l'a posé ; la boucle de rendu
//! a des arrêts mesurés de 500 à 860 ms (voir `win32::ouverture_fenetres`), au-delà
//! de `LowLevelHooksTimeout` : la souris de TOUT le système ramerait, puis Windows
//! retirerait le crochet sans prévenir.
//!
//! ⛔ **LE CROCHET NE FAIT QUE NOTER LE POINT.** `WindowFromPoint` peut envoyer
//! `WM_NCHITTEST` à la fenêtre d'un autre processus et attendre sa réponse : il
//! se fait APRÈS, hors du rappel, sur le même fil (un message posé à soi-même).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetAncestor, GetClassNameW, GetMessageW, PostThreadMessageW, SetWindowsHookExW, WindowFromPoint,
    GA_ROOT, HC_ACTION, MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_APP, WM_MBUTTONDOWN,
};

/// Le message que le crochet se poste : `wParam`/`lParam` = x/y écran.
const WM_CLIC_MOLETTE: u32 = WM_APP + 0x31;

/// Le fil du crochet (0 : pas encore lancé).
static FIL: AtomicU32 = AtomicU32::new(0);
/// Le dernier clic molette sur le bureau, pas encore pris par la boucle.
static CLIC: Mutex<Option<(i32, i32)>> = Mutex::new(None);

/// Les classes de la fenêtre RACINE du bureau : `Progman` (le bureau et ses
/// icônes) et `WorkerW` (l'hôte du fond d'écran, où `SHELLDLL_DefView` passe
/// selon les versions de Windows).
pub fn est_le_bureau(classe_racine: &str) -> bool {
    matches!(classe_racine, "Progman" | "WorkerW")
}

/// Lance le fil du crochet — une seule fois.
pub fn installer() {
    if FIL.load(Ordering::Relaxed) != 0 {
        return;
    }
    let lance = std::thread::Builder::new().name("clic-molette-bureau".into()).spawn(|| unsafe {
        FIL.store(GetCurrentThreadId(), Ordering::Relaxed);
        let crochet = match SetWindowsHookExW(WH_MOUSE_LL, Some(souris), None, 0) {
            Ok(c) => c,
            Err(erreur) => {
                crate::diag!("[clic-molette-bureau] crochet impossible : {erreur:?}");
                return;
            }
        };
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_CLIC_MOLETTE {
                let point = POINT { x: msg.wParam.0 as i32, y: msg.lParam.0 as i32 };
                if classe_racine_sous(point).is_some_and(|c| est_le_bureau(&c)) {
                    *CLIC.lock().unwrap_or_else(|e| e.into_inner()) = Some((point.x, point.y));
                }
            }
        }
        let _ = windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(crochet);
    });
    if let Err(erreur) = lance {
        crate::diag!("[clic-molette-bureau] fil impossible : {erreur}");
    }
}

/// Le clic molette sur le bureau en attente, s'il y en a un — à prendre une
/// fois par frame.
pub fn prendre() -> Option<(i32, i32)> {
    CLIC.lock().unwrap_or_else(|e| e.into_inner()).take()
}

fn classe_racine_sous(point: POINT) -> Option<String> {
    unsafe {
        let sous = WindowFromPoint(point);
        if sous.0.is_null() {
            return None;
        }
        let racine: HWND = GetAncestor(sous, GA_ROOT);
        let mut tampon = [0u16; 64];
        let n = GetClassNameW(racine, &mut tampon);
        (n > 0).then(|| String::from_utf16_lossy(&tampon[..n as usize]))
    }
}

/// Le crochet : il note le point d'un appui molette et le rend aussitôt.
unsafe extern "system" fn souris(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wparam.0 as u32 == WM_MBUTTONDOWN {
        let data = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let _ = unsafe {
            PostThreadMessageW(FIL.load(Ordering::Relaxed), WM_CLIC_MOLETTE, WPARAM(data.pt.x as isize as usize), LPARAM(data.pt.y as isize))
        };
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seul_le_bureau_ouvre_la_sphere() {
        assert!(est_le_bureau("Progman"));
        assert!(est_le_bureau("WorkerW"));
        for autre in ["Shell_TrayWnd", "CabinetWClass", "NeuraDeskDockWindow", "Chrome_WidgetWin_1", ""] {
            assert!(!est_le_bureau(autre), "{autre}");
        }
    }

    /// Le crochet ne doit rien avaler : il rend TOUJOURS la main au suivant.
    #[test]
    fn le_crochet_n_avale_rien() {
        let source = include_str!("clic_molette_bureau.rs");
        let crochet = &source[source.find("unsafe extern \"system\" fn souris").unwrap()..];
        let crochet = &crochet[..crochet.find("#[cfg(test)]").unwrap()];
        assert!(!crochet.contains("LRESULT(1)"), "le crochet avale un clic");
        assert!(crochet.trim_end().trim_end_matches('}').trim_end().ends_with("unsafe { CallNextHookEx(None, code, wparam, lparam) }"));
        assert!(!crochet.contains("WindowFromPoint("), "WindowFromPoint dans le rappel peut attendre un autre processus");
    }
}
