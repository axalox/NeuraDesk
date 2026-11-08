//! **LES COINS ARRONDIS NOIRS DES ÉCRANS** — DEMANDE EXPLICITE (2026-10-14) : « des
//! coins arrondis noirs permanents sur la partie extérieure de l'application, dans
//! les coins d'écrans principal et secondaires avec une option pour les désactiver ».
//!
//! Quatre petites fenêtres par écran, une par coin, peintes UNE fois par
//! `UpdateLayeredWindow` (comme `win32::epingle`) : du noir hors d'un quart de
//! disque, transparent dedans. Elles sont **transparentes aux clics**
//! (`WS_EX_TRANSPARENT`) et ne prennent jamais le focus.
//!
//! **Des fenêtres à part, et pas des quads du dock** : sur un écran secondaire, la
//! fenêtre du dock ne couvre que la barre du haut — les coins du bas n'ont aucune
//! surface où se dessiner. Et une barre masquée ne doit pas emporter ses coins.
//!
//! ⛔ **POSSÉDÉES PAR LA FENÊTRE DU DOCK** (`set_proprietaire`) — sans
//! propriétaire, un coin était la PREMIÈRE fenêtre visible du processus, donc sa
//! « fenêtre principale » pour Windows : `Process.CloseMainWindow()` (le script de
//! fermeture, le gestionnaire de tâches) lui envoyait `WM_CLOSE`, le coin se
//! détruisait, et l'application restait ouverte. Mesuré le 2026-10-14. Un coin
//! ignore aussi `WM_CLOSE`.
//!
//! ⚠️ **Classe hors du préfixe `NeuraDesk`** : `gfx::capture_visibility::
//! reapply_to_all_windows` retire `WS_EX_TOOLWINDOW` aux fenêtres `NeuraDesk*`,
//! ce qui ferait apparaître quatre coins par écran dans Alt-Tab. L'affinité de
//! capture est posée ici à la création, et la bascule du réglage RECRÉE les coins
//! (elle fait partie de la signature, voir `synchroniser`).

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER,
    BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, DIB_RGB_COLORS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, SetWindowDisplayAffinity, SetWindowPos, ShowWindow,
    UpdateLayeredWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SW_SHOWNOACTIVATE, WM_CLOSE, ULW_ALPHA, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

/// Rayon par défaut, en pixels LOGIQUES (multiplié par le DPI de chaque écran).
pub const RAYON_PAR_DEFAUT: f32 = 12.0;
pub const RAYON_MIN: f32 = 2.0;
pub const RAYON_MAX: f32 = 60.0;

static ACTIFS: AtomicBool = AtomicBool::new(true);
/// La fenêtre du dock, propriétaire de tous les coins (voir la doc de module).
static PROPRIETAIRE: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// À poser AVANT le premier `synchroniser` : la fenêtre du dock primaire.
pub fn set_proprietaire(hwnd: HWND) {
    PROPRIETAIRE.store(hwnd.0 as isize, Ordering::Relaxed);
}
/// La fenêtre du dock, pour les autres fenêtres décoratives qu'elle possède
/// (`win32::ombre_barre`). `None` : pas encore posée.
pub fn proprietaire() -> Option<HWND> {
    let hwnd = HWND(PROPRIETAIRE.load(Ordering::Relaxed) as *mut std::ffi::c_void);
    (!hwnd.0.is_null()).then_some(hwnd)
}
static RAYON_BITS: AtomicU32 = AtomicU32::new(0x4140_0000); // 12.0

pub fn actifs() -> bool {
    ACTIFS.load(Ordering::Relaxed)
}

pub fn activer(actifs: bool) {
    ACTIFS.store(actifs, Ordering::Relaxed);
}

pub fn rayon() -> f32 {
    f32::from_bits(RAYON_BITS.load(Ordering::Relaxed))
}

/// Pose le rayon, borné par `RAYON_MIN`/`RAYON_MAX`.
pub fn set_rayon(px: f32) {
    RAYON_BITS.store(borner_rayon(px).to_bits(), Ordering::Relaxed);
}

fn borner_rayon(px: f32) -> f32 {
    if px.is_finite() { px.clamp(RAYON_MIN, RAYON_MAX) } else { RAYON_PAR_DEFAUT }
}

/// Un coin à poser : position écran du coin haut-gauche de sa fenêtre, côté, et
/// de quel côté de l'écran il est (`droite`, `bas`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Coin {
    x: i32,
    y: i32,
    cote: i32,
    droite: bool,
    bas: bool,
}

thread_local! {
    /// Les fenêtres posées, et ce qui les a fait poser — rien n'est refait tant
    /// que la signature ne change pas.
    static POSES: RefCell<(Vec<Coin>, bool, Vec<HWND>)> = const { RefCell::new((Vec::new(), false, Vec::new())) };
}

/// Les quatre coins de chaque écran `[x, y, largeur, hauteur]`, au rayon `rayon`
/// (déjà en pixels physiques, un par écran).
fn coins_des_ecrans(ecrans: &[([i32; 4], i32)]) -> Vec<Coin> {
    ecrans
        .iter()
        .flat_map(|&([x, y, l, h], rayon)| {
            let cote = rayon.min(l / 2).min(h / 2).max(1);
            [(false, false), (true, false), (false, true), (true, true)].map(|(droite, bas)| Coin {
                x: if droite { x + l - cote } else { x },
                y: if bas { y + h - cote } else { y },
                cote,
                droite,
                bas,
            })
        })
        .collect()
}

/// Met les fenêtres en accord avec l'option, le rayon et les écrans branchés. À
/// appeler à chaque frame : sans changement, il ne fait qu'énumérer les écrans.
pub fn synchroniser() {
    let voulus = if actifs() {
        let ecrans: Vec<([i32; 4], i32)> = crate::win32::monitors::enumerate()
            .iter()
            .map(|m| {
                let r = m.rect;
                let dpi = crate::win32::dpi::get_dpi_for_monitor(m.handle) as f32;
                let rayon = (rayon() * dpi / crate::win32::dpi::STANDARD_DPI as f32).round() as i32;
                ([r.left, r.top, r.right - r.left, r.bottom - r.top], rayon)
            })
            .collect();
        coins_des_ecrans(&ecrans)
    } else {
        Vec::new()
    };
    let capturables = crate::gfx::capture_visibility::is_enabled();
    POSES.with(|p| {
        let Ok(mut poses) = p.try_borrow_mut() else { return };
        if poses.0 == voulus && poses.1 == capturables {
            drop(poses);
            remonter();
            return;
        }
        for hwnd in poses.2.drain(..) {
            let _ = unsafe { DestroyWindow(hwnd) };
        }
        let fenetres = voulus.iter().filter_map(|c| poser(c)).collect();
        *poses = (voulus, capturables, fenetres);
    });
}

/// Repasse les coins au-dessus de toute autre fenêtre topmost — appelé après chaque
/// remontée du dock (`Window::raise_to_top`) et à chaque `synchroniser`.
pub fn remonter() {
    POSES.with(|p| {
        let Ok(poses) = p.try_borrow() else { return };
        for &hwnd in &poses.2 {
            remonter_coin(hwnd);
        }
    });
}

pub(crate) fn remonter_coin(hwnd: HWND) {
    // ⛔ `SWP_NOOWNERZORDER` — BUG RÉEL SIGNALÉ (2026-10-23) : « l'ombre
    // apparaît périodiquement au-dessus des menus ». Un coin est POSSÉDÉ
    // par le dock : sans ce drapeau, Windows remonte AUSSI le
    // propriétaire et tout son groupe (dock, ombres, coins). MESURÉ :
    // huit `WM_WINDOWPOSCHANGING` sur le dock à chaque `synchroniser`
    // (une fois par seconde), et le dock et ses ombres repassaient
    // devant le menu de l'horloge jusqu'au rangement suivant.
    let _ = unsafe { SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Un coin ne se ferme pas seul : c'est `synchroniser` qui les détruit.
    if msg == WM_CLOSE {
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

pub(crate) fn classe() -> Option<HINSTANCE> {
    static CLASSE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }.ok()?.into();
    CLASSE.get_or_init(|| {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: w!("NdCoinEcran"),
            ..Default::default()
        };
        unsafe { RegisterClassExW(&wc) };
    });
    Some(instance)
}

fn poser(coin: &Coin) -> Option<HWND> {
    let instance = classe()?;
    let proprietaire = HWND(PROPRIETAIRE.load(Ordering::Relaxed) as *mut std::ffi::c_void);
    let proprietaire = (!proprietaire.0.is_null()).then_some(proprietaire);
    let hwnd = match unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("NdCoinEcran"),
            w!("NeuraDesk — coin d'écran"),
            WS_POPUP,
            coin.x,
            coin.y,
            coin.cote,
            coin.cote,
            proprietaire,
            None,
            Some(instance),
            None,
        )
    } {
        Ok(hwnd) => hwnd,
        Err(err) => {
            crate::diag!("[coins_ecran] CreateWindowExW échoué : {err}");
            return None;
        }
    };
    let _ = unsafe { SetWindowDisplayAffinity(hwnd, crate::gfx::capture_visibility::window_display_affinity()) };
    peindre(hwnd, coin);
    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    Some(hwnd)
}

/// BGRA prémultiplié d'un coin de `cote` px : noir opaque hors du quart de disque
/// centré sur le coin INTÉRIEUR, transparent dedans, bord lissé sur un pixel.
fn pixels_du_coin(cote: i32, droite: bool, bas: bool) -> Vec<u8> {
    pixels_du_coin_en_couleur(cote, droite, bas, [0, 0, 0])
}

/// `pixels_du_coin` dans la couleur `rgb` (BGRA prémultiplié).
pub(crate) fn pixels_du_coin_en_couleur(cote: i32, droite: bool, bas: bool, [r8, g8, b8]: [u8; 3]) -> Vec<u8> {
    let premultiplie = |c: u8, a: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
    let r = cote as f32;
    let mut px = Vec::with_capacity((cote * cote * 4) as usize);
    for y in 0..cote {
        for x in 0..cote {
            // Distance au centre du disque, mesurée depuis le coin EXTÉRIEUR.
            let dx = if droite { x as f32 + 0.5 } else { r - (x as f32 + 0.5) };
            let dy = if bas { y as f32 + 0.5 } else { r - (y as f32 + 0.5) };
            let a = ((dx.hypot(dy) - r + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
            px.extend_from_slice(&[premultiplie(b8, a), premultiplie(g8, a), premultiplie(r8, a), a]);
        }
    }
    px
}

fn peindre(hwnd: HWND, coin: &Coin) {
    peindre_carre(hwnd, (coin.x, coin.y), coin.cote, &pixels_du_coin(coin.cote, coin.droite, coin.bas));
}

/// Peint la fenêtre `hwnd` (`WS_EX_LAYERED`), carré de `cote` px posé en `position`
/// (écran), avec `pixels` en BGRA prémultiplié — partagé avec
/// `win32::coins_secondaires`.
pub(crate) fn peindre_carre(hwnd: HWND, position: (i32, i32), cote: i32, pixels: &[u8]) {
    unsafe {
        let ecran = GetDC(None);
        let memoire = CreateCompatibleDC(Some(ecran));
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: cote,
            biHeight: -cote,
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        if let Ok(bitmap) = CreateDIBSection(Some(memoire), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            if !bits.is_null() {
                std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
            }
            let ancien = SelectObject(memoire, bitmap.into());
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let taille = SIZE { cx: cote, cy: cote };
            let source = POINT::default();
            let position = POINT { x: position.0, y: position.1 };
            if let Err(err) = UpdateLayeredWindow(
                hwnd,
                Some(ecran),
                Some(&position),
                Some(&taille),
                Some(memoire),
                Some(&source),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            ) {
                crate::diag!("[coins_ecran] UpdateLayeredWindow échoué : {err}");
            }
            SelectObject(memoire, ancien);
            let _ = DeleteObject(bitmap.into());
        }
        let _ = DeleteDC(memoire);
        ReleaseDC(None, ecran);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Quatre coins par écran, collés à ses bords — y compris un écran placé à
    /// gauche de la primaire (coordonnées négatives).
    #[test]
    fn quatre_coins_par_ecran_colles_aux_bords() {
        let coins = coins_des_ecrans(&[([0, 0, 3840, 2160], 24), ([-1920, 500, 1920, 1080], 12)]);
        assert_eq!(coins.len(), 8);
        assert_eq!(coins[0], Coin { x: 0, y: 0, cote: 24, droite: false, bas: false });
        assert_eq!(coins[3], Coin { x: 3816, y: 2136, cote: 24, droite: true, bas: true });
        assert_eq!(coins[5], Coin { x: -12, y: 500, cote: 12, droite: true, bas: false });
        assert_eq!(coins[6], Coin { x: -1920, y: 1568, cote: 12, droite: false, bas: true });
    }

    /// Le pixel du coin EXTÉRIEUR est noir opaque, celui du coin intérieur
    /// transparent — pour les quatre orientations.
    #[test]
    fn noir_dehors_transparent_dedans() {
        let cote = 16;
        let alpha = |px: &[u8], x: i32, y: i32| px[((y * cote + x) * 4 + 3) as usize];
        for (droite, bas) in [(false, false), (true, false), (false, true), (true, true)] {
            let px = pixels_du_coin(cote, droite, bas);
            let (ex, ey) = (if droite { cote - 1 } else { 0 }, if bas { cote - 1 } else { 0 });
            let (ix, iy) = (cote - 1 - ex, cote - 1 - ey);
            assert_eq!(alpha(&px, ex, ey), 255, "extérieur {droite} {bas}");
            assert_eq!(alpha(&px, ix, iy), 0, "intérieur {droite} {bas}");
            assert!(px.chunks_exact(4).all(|p| p[..3] == [0, 0, 0]), "du noir, prémultiplié");
        }
    }

    #[test]
    fn le_rayon_est_borne() {
        assert_eq!(borner_rayon(0.0), RAYON_MIN);
        assert_eq!(borner_rayon(500.0), RAYON_MAX);
        assert_eq!(borner_rayon(f32::NAN), RAYON_PAR_DEFAUT);
        assert_eq!(f32::from_bits(0x4140_0000), RAYON_PAR_DEFAUT, "la valeur initiale de RAYON_BITS");
    }
}

#[cfg(test)]
mod tests_rang {
    use super::*;

    fn fenetre(proprietaire: Option<HWND>) -> HWND {
        let instance = classe().unwrap();
        unsafe {
            CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, w!("NdCoinEcran"), w!("essai"), WS_POPUP, 0, 0, 1, 1, proprietaire, None, Some(instance), None)
        }
        .unwrap()
    }

    /// ⛔ Remonter un coin ne remonte PAS le dock qui le possède — sinon le dock
    /// et ses ombres repassaient devant les menus une fois par seconde.
    #[test]
    fn remonter_un_coin_laisse_le_dock_sous_le_menu() {
        let dock = fenetre(None);
        let coin = fenetre(Some(dock));
        let menu = fenetre(None);
        let haut = |h| unsafe { SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
        let _ = (haut(dock), haut(menu));

        remonter_coin(coin);

        let rang = |cible| {
            let mut c = unsafe { windows::Win32::UI::WindowsAndMessaging::GetTopWindow(None) }.ok();
            let mut i = 0;
            while let Some(h) = c {
                if h == cible {
                    return i;
                }
                i += 1;
                c = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindow(h, windows::Win32::UI::WindowsAndMessaging::GW_HWNDNEXT) }.ok();
            }
            usize::MAX
        };
        assert!(rang(menu) < rang(dock), "le dock reste sous le menu");
        for h in [coin, menu, dock] {
            let _ = unsafe { DestroyWindow(h) };
        }
    }
}
