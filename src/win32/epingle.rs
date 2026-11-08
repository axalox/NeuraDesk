//! **L'ÉPINGLE** — l'outil « pin » de Flameshot, demandé le 2026-09-25 (« ajoutes
//! tous les outils de flameshot ») : l'image de l'éditeur posée à l'écran dans
//! une petite fenêtre SANS cadre, toujours au premier plan.
//!
//! Comme chez Flameshot : glisser la déplace, la molette l'agrandit ou la
//! réduit, Ctrl+molette règle son opacité, un double-clic (ou Échap, ou le
//! bouton du milieu) la ferme. Plusieurs épingles peuvent coexister.
//!
//! **PAS DE CEF ICI** : une fenêtre `WS_EX_LAYERED` peinte par
//! `UpdateLayeredWindow` depuis un DIB 32 bits — l'alpha de l'image est donc
//! respecté tel quel, et une épingle ne coûte ni navigateur ni swapchain.

use std::cell::RefCell;
use std::ffi::c_void;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW, MonitorFromPoint, ReleaseDC,
    SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, DIB_RGB_COLORS, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetWindowRect, LoadCursorW, RegisterClassExW,
    ShowWindow, UpdateLayeredWindow, HTCAPTION, IDC_SIZEALL, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_KEYDOWN, WM_MOUSEWHEEL,
    WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCMBUTTONUP, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

/// Une épingle vivante : l'image en BGRA PRÉMULTIPLIÉ (le format que demande
/// `UpdateLayeredWindow`), à sa taille d'origine.
struct Epingle {
    hwnd: HWND,
    largeur: u32,
    hauteur: u32,
    bgra: Vec<u8>,
    echelle: f32,
    opacite: f32,
}

thread_local! {
    static EPINGLES: RefCell<Vec<Epingle>> = const { RefCell::new(Vec::new()) };
}

/// Part de l'écran (aire de travail) qu'une épingle occupe au plus à sa naissance.
const PART_ECRAN_MAX: f32 = 0.8;
/// Un cran de molette multiplie (ou divise) la taille par ce facteur.
const PAS_ECHELLE: f32 = 1.1;
const ECHELLE_MIN: f32 = 0.05;
const ECHELLE_MAX: f32 = 8.0;
/// Un cran de Ctrl+molette ajoute (ou retire) ce dixième d'opacité — comme Flameshot.
const PAS_OPACITE: f32 = 0.1;
const OPACITE_MIN: f32 = 0.1;
/// `MK_CONTROL` dans le mot bas du `wparam` de `WM_MOUSEWHEEL`.
const MK_CONTROL: usize = 0x0008;

/// Épingle l'image `rgba` (non prémultipliée, comme `clipboard::decode_png`)
/// centrée sous le curseur.
pub fn epingler(largeur: u32, hauteur: u32, rgba: &[u8]) {
    if largeur == 0 || hauteur == 0 || rgba.len() != (largeur * hauteur * 4) as usize {
        crate::diag!("[epingle] image invalide : {largeur}×{hauteur}, {} octets", rgba.len());
        return;
    }
    let mut curseur = POINT::default();
    let _ = unsafe { GetCursorPos(&mut curseur) };
    let travail = aire_de_travail(curseur);
    let echelle = echelle_initiale(largeur, hauteur, travail);
    let Some(hwnd) = creer_fenetre() else { return };
    EPINGLES.with(|e| {
        e.borrow_mut().push(Epingle { hwnd, largeur, hauteur, bgra: premultiplier_bgra(rgba), echelle, opacite: 1.0 })
    });
    let (l, h) = taille_affichee(largeur, hauteur, echelle);
    let x = (curseur.x - l / 2).clamp(travail.left, (travail.right - l).max(travail.left));
    let y = (curseur.y - h / 2).clamp(travail.top, (travail.bottom - h).max(travail.top));
    peindre(hwnd, POINT { x, y });
    // SANS activer : l'éditeur garde le premier plan, donc reste ouvert (il se
    // masque quand il le perd) — on peut épingler plusieurs fois de suite.
    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    crate::diag!("[epingle] {largeur}×{hauteur} épinglée à {x},{y} (échelle {echelle:.2})");
}

fn aire_de_travail(point: POINT) -> RECT {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let ecran = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let _ = GetMonitorInfoW(ecran, &mut info);
    }
    info.rcWork
}

/// 1:1 si l'image tient dans [`PART_ECRAN_MAX`] de l'aire de travail, sinon
/// réduite pour y tenir.
fn echelle_initiale(largeur: u32, hauteur: u32, travail: RECT) -> f32 {
    let l = (travail.right - travail.left) as f32 * PART_ECRAN_MAX;
    let h = (travail.bottom - travail.top) as f32 * PART_ECRAN_MAX;
    (l / largeur as f32).min(h / hauteur as f32).min(1.0).max(ECHELLE_MIN)
}

fn taille_affichee(largeur: u32, hauteur: u32, echelle: f32) -> (i32, i32) {
    (((largeur as f32 * echelle).round() as i32).max(1), ((hauteur as f32 * echelle).round() as i32).max(1))
}

fn creer_fenetre() -> Option<HWND> {
    static CLASSE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).ok()?.into();
        CLASSE.get_or_init(|| {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_SIZEALL).unwrap_or_default(),
                lpszClassName: w!("NeuraDeskEpingle"),
                ..Default::default()
            };
            RegisterClassExW(&wc);
        });
        match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("NeuraDeskEpingle"),
            w!("NeuraDesk — Épingle"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance),
            None,
        ) {
            Ok(hwnd) => Some(hwnd),
            Err(err) => {
                crate::diag!("[epingle] CreateWindowExW a échoué : {err}");
                None
            }
        }
    }
}

/// Repeint l'épingle `hwnd` à sa taille et son opacité courantes, coin
/// haut-gauche en `position`. Ne tient `EPINGLES` que le temps du calcul.
fn peindre(hwnd: HWND, position: POINT) {
    let Some((l, h, pixels, opacite)) = EPINGLES.with(|e| {
        let e = e.try_borrow().ok()?;
        let p = e.iter().find(|p| p.hwnd == hwnd)?;
        let (l, h) = taille_affichee(p.largeur, p.hauteur, p.echelle);
        Some((l, h, reechantillonner(&p.bgra, p.largeur, p.hauteur, l as u32, h as u32), p.opacite))
    }) else {
        return;
    };
    unsafe {
        let ecran = GetDC(None);
        let memoire = CreateCompatibleDC(Some(ecran));
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: l,
            // Négatif : les lignes vont de haut en bas, comme `pixels`.
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        match CreateDIBSection(Some(memoire), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bitmap) if !bits.is_null() => {
                std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
                let ancien = SelectObject(memoire, bitmap.into());
                let fusion = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: (opacite * 255.0).round() as u8,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                let taille = SIZE { cx: l, cy: h };
                let origine = POINT::default();
                if let Err(err) = UpdateLayeredWindow(
                    hwnd,
                    Some(ecran),
                    Some(&position),
                    Some(&taille),
                    Some(memoire),
                    Some(&origine),
                    COLORREF(0),
                    Some(&fusion),
                    ULW_ALPHA,
                ) {
                    crate::diag!("[epingle] UpdateLayeredWindow a échoué : {err}");
                }
                SelectObject(memoire, ancien);
                let _ = DeleteObject(bitmap.into());
            }
            _ => crate::diag!("[epingle] CreateDIBSection {l}×{h} a échoué"),
        }
        let _ = DeleteDC(memoire);
        ReleaseDC(None, ecran);
    }
}

/// RGBA droit → BGRA prémultiplié.
///
/// `pub(crate)` : réutilisée par `win32::loupe`, qui peint elle aussi une
/// fenêtre en couches par `UpdateLayeredWindow` — même besoin, même format.
pub(crate) fn premultiplier_bgra(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|p| {
            let a = p[3] as u32;
            let m = |c: u8| ((c as u32 * a + 127) / 255) as u8;
            [m(p[2]), m(p[1]), m(p[0]), p[3]]
        })
        .collect()
}

/// Rééchantillonne une image 4 octets/pixel de `l`×`h` vers `nl`×`nh` : la
/// MOYENNE de la surface couverte en réduction (sinon une capture réduite
/// scintille de moirés), le plus proche voisin en agrandissement (les pixels
/// d'une capture restent nets, comme la loupe de l'éditeur).
///
/// `pub(crate)` : c'est CE plus-proche-voisin qui sert à `win32::loupe` — un
/// `StretchBlt` aurait pu convenir, mais le dépôt sait déjà que `HALFTONE`
/// floute (`gfx::backdrop`), et se tromper de mode se voit à peine. Ici le
/// comportement est écrit, et testé.
pub(crate) fn reechantillonner(src: &[u8], l: u32, h: u32, nl: u32, nh: u32) -> Vec<u8> {
    if (l, h) == (nl, nh) {
        return src.to_vec();
    }
    let mut dst = vec![0u8; (nl * nh * 4) as usize];
    // Les bornes [debut, fin) de la surface source couverte par le pixel `i`.
    let bornes = |i: u32, n: u32, taille: u32| {
        let debut = (i as u64 * taille as u64 / n as u64) as u32;
        let fin = (((i + 1) as u64 * taille as u64).div_ceil(n as u64) as u32).clamp(debut + 1, taille);
        (debut, fin)
    };
    for y in 0..nh {
        let (y0, y1) = bornes(y, nh, h);
        for x in 0..nl {
            let (x0, x1) = bornes(x, nl, l);
            let mut somme = [0u32; 4];
            for sy in y0..y1 {
                let ligne = (sy * l) as usize * 4;
                for sx in x0..x1 {
                    let i = ligne + sx as usize * 4;
                    for c in 0..4 {
                        somme[c] += src[i + c] as u32;
                    }
                }
            }
            let n = (y1 - y0) * (x1 - x0);
            let o = ((y * nl + x) * 4) as usize;
            for c in 0..4 {
                dst[o + c] = ((somme[c] + n / 2) / n) as u8;
            }
        }
    }
    dst
}

/// Change l'échelle de l'épingle en gardant son CENTRE immobile.
fn changer_echelle(hwnd: HWND, facteur: f32) {
    let Some((avant, apres)) = EPINGLES.with(|e| {
        let mut e = e.try_borrow_mut().ok()?;
        let p = e.iter_mut().find(|p| p.hwnd == hwnd)?;
        let avant = taille_affichee(p.largeur, p.hauteur, p.echelle);
        p.echelle = (p.echelle * facteur).clamp(ECHELLE_MIN, ECHELLE_MAX);
        Some((avant, taille_affichee(p.largeur, p.hauteur, p.echelle)))
    }) else {
        return;
    };
    let mut r = RECT::default();
    let _ = unsafe { GetWindowRect(hwnd, &mut r) };
    let position = POINT { x: r.left + (avant.0 - apres.0) / 2, y: r.top + (avant.1 - apres.1) / 2 };
    peindre(hwnd, position);
}

fn changer_opacite(hwnd: HWND, delta: f32) {
    let change = EPINGLES.with(|e| {
        let Ok(mut e) = e.try_borrow_mut() else { return false };
        let Some(p) = e.iter_mut().find(|p| p.hwnd == hwnd) else { return false };
        p.opacite = (p.opacite + delta).clamp(OPACITE_MIN, 1.0);
        true
    });
    if change {
        let mut r = RECT::default();
        let _ = unsafe { GetWindowRect(hwnd, &mut r) };
        peindre(hwnd, POINT { x: r.left, y: r.top });
    }
}

fn fermer(hwnd: HWND) {
    let _ = unsafe { DestroyWindow(hwnd) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // Toute l'épingle est une « barre de titre » : glisser la déplace, sans
        // une ligne de code pour le glisser.
        WM_NCHITTEST => return LRESULT(HTCAPTION as isize),
        WM_MOUSEWHEEL => {
            let vers_le_haut = ((wparam.0 >> 16) as u16 as i16) > 0;
            if wparam.0 & MK_CONTROL != 0 {
                changer_opacite(hwnd, if vers_le_haut { PAS_OPACITE } else { -PAS_OPACITE });
            } else {
                changer_echelle(hwnd, if vers_le_haut { PAS_ECHELLE } else { 1.0 / PAS_ECHELLE });
            }
            return LRESULT(0);
        }
        WM_NCLBUTTONDBLCLK | WM_NCMBUTTONUP => {
            fermer(hwnd);
            return LRESULT(0);
        }
        WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
            fermer(hwnd);
            return LRESULT(0);
        }
        WM_NCDESTROY => {
            // `try_borrow_mut` : jamais `borrow` dans un wndproc (voir la mémoire
            // « RefCell reentrancy »).
            let retiree = EPINGLES.with(|e| e.try_borrow_mut().map(|mut e| e.retain(|p| p.hwnd != hwnd)).is_ok());
            if !retiree {
                crate::diag!("[epingle] {:#x} détruite pendant un emprunt : son image reste en mémoire", hwnd.0 as usize);
            }
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_bgra_est_premultiplie_et_permute() {
        assert_eq!(premultiplier_bgra(&[255, 128, 0, 255]), vec![0, 128, 255, 255]);
        assert_eq!(premultiplier_bgra(&[255, 255, 255, 0]), vec![0, 0, 0, 0]);
        assert_eq!(premultiplier_bgra(&[200, 100, 50, 128]), vec![25, 50, 100, 128]);
    }

    #[test]
    fn reduire_de_moitie_fait_la_moyenne_de_chaque_carre() {
        // 2×2 → 1×1 : la moyenne des quatre.
        let src = [0, 0, 0, 255, 100, 0, 0, 255, 0, 200, 0, 255, 0, 0, 40, 255];
        assert_eq!(reechantillonner(&src, 2, 2, 1, 1), vec![25, 50, 10, 255]);
    }

    #[test]
    fn agrandir_garde_les_pixels_nets() {
        let src = [10, 20, 30, 255, 40, 50, 60, 255];
        let dst = reechantillonner(&src, 2, 1, 4, 1);
        assert_eq!(dst, vec![10, 20, 30, 255, 10, 20, 30, 255, 40, 50, 60, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn une_taille_inchangee_rend_l_image_intacte() {
        let src: Vec<u8> = (0..48).collect();
        assert_eq!(reechantillonner(&src, 4, 3, 4, 3), src);
    }

    #[test]
    fn une_grande_image_nait_reduite_une_petite_a_un_pour_un() {
        let travail = RECT { left: 0, top: 0, right: 1000, bottom: 1000 };
        assert_eq!(echelle_initiale(400, 300, travail), 1.0);
        assert_eq!(echelle_initiale(1600, 400, travail), 0.5);
    }
}
