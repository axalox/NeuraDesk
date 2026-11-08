//! **L'ÉCRAN D'ACCUEIL** — le logo de NeuraDesk au centre de l'écran pendant
//! le lancement, demandé le 2026-11-08 (« un pop up à l'ouverture de NeuraDesk »).
//!
//! **PAS DE CEF ICI**, comme `win32::epingle` : une fenêtre `WS_EX_LAYERED`
//! peinte par `UpdateLayeredWindow`. C'est ce qui lui permet de paraître AVANT
//! que les pages CEF soient prêtes — c'est tout son propos.
//!
//! **SON PROPRE FIL, SA PROPRE BOUCLE DE MESSAGES** : l'accueil ne touche aucun
//! état de l'application et `app::run` n'a rien à en savoir. Il apparaît en
//! fondu, reste un instant, s'efface, et son fil se termine. Un clic le ferme
//! aussitôt (en fondu).
//!
//! L'image est `assets/popups/logo_neuradesk.png` — la MÊME que celle de l'aide :
//! le logo dont le fond noir a été rendu transparent (alpha = luminosité). Elle
//! est posée ici sur une carte noire aux coins arrondis, ce qui redonne l'aspect
//! d'origine sur n'importe quel fond d'écran (sur un fond clair, le halo seul se
//! délave).
//!
//! Sans effet sous le harnais de capture (`NEURADESK_CAPTURE_FRAMES`) : une
//! fenêtre au premier plan pendant trois secondes polluerait ses photos.

use std::ffi::c_void;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW, MonitorFromPoint, ReleaseDC,
    SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, DIB_RGB_COLORS, HDC,
    MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, KillTimer, LoadCursorW,
    PostQuitMessage, RegisterClassExW, SetTimer, ShowWindow, TranslateMessage, UpdateLayeredWindow, IDC_HAND, MSG,
    SW_SHOWNOACTIVATE, ULW_ALPHA, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

/// Le logo, embarqué : l'accueil doit paraître même si `assets/` manquait.
const LOGO_PNG: &[u8] = include_bytes!("../../assets/popups/logo_neuradesk.png");

/// Largeur de la carte au plus, en part de la largeur de l'écran principal.
const PART_ECRAN: f32 = 0.42;
/// Marge autour du logo, en part de la largeur de la carte.
const MARGE: f32 = 0.05;
/// Rayon des coins de la carte, en part de sa hauteur.
const RAYON: f32 = 0.08;

const FONDU_ENTREE_MS: u32 = 350;
const TENUE_MS: u32 = 2200;
const FONDU_SORTIE_MS: u32 = 550;
const PAS_MS: u32 = 16;
const MINUTERIE: usize = 1;

/// Où en est l'accueil — une seule fenêtre par fil, donc un `thread_local`.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Entree,
    Tenue,
    Sortie,
}

struct Etat {
    hwnd: HWND,
    position: POINT,
    largeur: i32,
    hauteur: i32,
    bgra: Vec<u8>,
    phase: Phase,
    /// Millisecondes écoulées dans la phase courante.
    ecoule: u32,
}

thread_local! {
    static ETAT: std::cell::RefCell<Option<Etat>> = const { std::cell::RefCell::new(None) };
}

/// Lance l'écran d'accueil sur son propre fil et rend la main aussitôt.
pub fn afficher() {
    if std::env::var_os("NEURADESK_CAPTURE_FRAMES").is_some() {
        return;
    }
    let lance = std::thread::Builder::new().name("ecran-accueil".into()).spawn(|| {
        if let Err(err) = executer() {
            crate::diag!("[accueil] écran d'accueil non affiché : {err}");
        }
    });
    if let Err(err) = lance {
        crate::diag!("[accueil] fil non créé : {err}");
    }
}

fn executer() -> Result<(), String> {
    let logo = image::load_from_memory(LOGO_PNG).map_err(|e| format!("logo non décodé : {e}"))?.to_rgba8();
    let ecran = ecran_principal();
    let (largeur, hauteur) = taille_carte(logo.width(), logo.height(), ecran.right - ecran.left);
    let logo_l = (largeur as f32 * (1.0 - 2.0 * MARGE)).round() as u32;
    let logo_h = ((logo_l as f32) * logo.height() as f32 / logo.width() as f32).round() as u32;
    let logo = image::imageops::resize(&logo, logo_l, logo_h, image::imageops::FilterType::Lanczos3);
    let bgra = composer_carte(largeur as u32, hauteur as u32, logo.as_raw(), logo_l, logo_h);
    let position = POINT {
        x: ecran.left + (ecran.right - ecran.left - largeur) / 2,
        y: ecran.top + (ecran.bottom - ecran.top - hauteur) / 2,
    };
    let hwnd = creer_fenetre()?;
    ETAT.with(|e| {
        *e.borrow_mut() = Some(Etat { hwnd, position, largeur, hauteur, bgra, phase: Phase::Entree, ecoule: 0 })
    });
    peindre(0.0);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(Some(hwnd), MINUTERIE, PAS_MS, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    ETAT.with(|e| e.borrow_mut().take());
    Ok(())
}

fn ecran_principal() -> windows::Win32::Foundation::RECT {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let moniteur = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
        let _ = GetMonitorInfoW(moniteur, &mut info);
    }
    info.rcWork
}

/// Taille de la carte : le logo plus sa marge, au plus `PART_ECRAN` de l'écran
/// et jamais plus grand que l'image elle-même (un agrandissement la flouterait).
fn taille_carte(logo_l: u32, logo_h: u32, ecran_l: i32) -> (i32, i32) {
    let naturelle = logo_l as f32 / (1.0 - 2.0 * MARGE);
    let largeur = naturelle.min(ecran_l.max(1) as f32 * PART_ECRAN).max(1.0);
    let logo_affiche_l = largeur * (1.0 - 2.0 * MARGE);
    let logo_affiche_h = logo_affiche_l * logo_h as f32 / logo_l.max(1) as f32;
    let hauteur = logo_affiche_h + 2.0 * MARGE * largeur;
    (largeur.round() as i32, hauteur.round() as i32)
}

/// La carte en BGRA PRÉMULTIPLIÉ (le format de `UpdateLayeredWindow`) : un
/// rectangle noir aux coins arrondis et lissés, le logo `rgba` (droit) centré
/// dessus. Le logo étant posé sur du noir, sa couleur prémultipliée EST la
/// couleur finale ; seule la forme de la carte porte l'alpha.
fn composer_carte(l: u32, h: u32, rgba: &[u8], logo_l: u32, logo_h: u32) -> Vec<u8> {
    let mut sortie = vec![0u8; (l * h * 4) as usize];
    let rayon = (h as f32 * RAYON).max(1.0);
    let ox = (l.saturating_sub(logo_l) / 2) as i64;
    let oy = (h.saturating_sub(logo_h) / 2) as i64;
    for y in 0..h {
        for x in 0..l {
            let couverture = couverture_coin_arrondi(x as f32 + 0.5, y as f32 + 0.5, l as f32, h as f32, rayon);
            if couverture <= 0.0 {
                continue;
            }
            let (lx, ly) = (x as i64 - ox, y as i64 - oy);
            let mut couleur = [0f32; 3];
            if lx >= 0 && ly >= 0 && (lx as u32) < logo_l && (ly as u32) < logo_h {
                let i = ((ly as u32 * logo_l + lx as u32) * 4) as usize;
                let a = rgba[i + 3] as f32 / 255.0;
                couleur = [rgba[i] as f32 * a, rgba[i + 1] as f32 * a, rgba[i + 2] as f32 * a];
            }
            let o = ((y * l + x) * 4) as usize;
            sortie[o] = (couleur[2] * couverture).round() as u8;
            sortie[o + 1] = (couleur[1] * couverture).round() as u8;
            sortie[o + 2] = (couleur[0] * couverture).round() as u8;
            sortie[o + 3] = (255.0 * couverture).round() as u8;
        }
    }
    sortie
}

/// Part (0..1) du pixel centré en (`px`, `py`) couverte par un rectangle
/// `l`×`h` aux coins de rayon `r` — lissage d'un pixel sur le bord des coins.
fn couverture_coin_arrondi(px: f32, py: f32, l: f32, h: f32, r: f32) -> f32 {
    let cx = px.clamp(r, l - r);
    let cy = py.clamp(r, h - r);
    let distance = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
    (r - distance + 0.5).clamp(0.0, 1.0)
}

/// L'opacité de la fenêtre pour une phase et un temps écoulé, lissée.
fn opacite(phase: Phase, ecoule: u32) -> f32 {
    let t = |duree: u32| (ecoule as f32 / duree as f32).clamp(0.0, 1.0);
    let lisse = |t: f32| t * t * (3.0 - 2.0 * t);
    match phase {
        Phase::Entree => lisse(t(FONDU_ENTREE_MS)),
        Phase::Tenue => 1.0,
        Phase::Sortie => 1.0 - lisse(t(FONDU_SORTIE_MS)),
    }
}

fn creer_fenetre() -> Result<HWND, String> {
    unsafe {
        let instance = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW : {e}"))?;
        let classe = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_HAND).unwrap_or_default(),
            lpszClassName: w!("NeuraDeskEcranAccueil"),
            ..Default::default()
        };
        // Un second enregistrement (classe déjà connue) échoue sans gravité.
        RegisterClassExW(&classe);
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("NeuraDeskEcranAccueil"),
            w!("NeuraDesk"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .map_err(|e| format!("CreateWindowExW : {e}"))
    }
}

/// Repeint la carte à l'opacité `alpha` (0..1).
fn peindre(alpha: f32) {
    ETAT.with(|e| {
        let Ok(e) = e.try_borrow() else { return };
        let Some(e) = e.as_ref() else { return };
        unsafe {
            let ecran = GetDC(None);
            poser_image(e, ecran, alpha);
            ReleaseDC(None, ecran);
        }
    });
}

unsafe fn poser_image(e: &Etat, ecran: HDC, alpha: f32) {
    let memoire = CreateCompatibleDC(Some(ecran));
    let mut info = BITMAPINFO::default();
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: e.largeur,
        // Négatif : lignes de haut en bas, comme `bgra`.
        biHeight: -e.hauteur,
        biPlanes: 1,
        biBitCount: 32,
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    match CreateDIBSection(Some(memoire), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
        Ok(bitmap) if !bits.is_null() => {
            std::ptr::copy_nonoverlapping(e.bgra.as_ptr(), bits as *mut u8, e.bgra.len());
            let ancien = SelectObject(memoire, bitmap.into());
            let fusion = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let taille = SIZE { cx: e.largeur, cy: e.hauteur };
            let origine = POINT::default();
            if let Err(err) = UpdateLayeredWindow(
                e.hwnd,
                Some(ecran),
                Some(&e.position),
                Some(&taille),
                Some(memoire),
                Some(&origine),
                COLORREF(0),
                Some(&fusion),
                ULW_ALPHA,
            ) {
                crate::diag!("[accueil] UpdateLayeredWindow a échoué : {err}");
            }
            SelectObject(memoire, ancien);
            let _ = DeleteObject(bitmap.into());
        }
        _ => crate::diag!("[accueil] CreateDIBSection {}×{} a échoué", e.largeur, e.hauteur),
    }
    let _ = DeleteDC(memoire);
}

/// Avance l'animation d'un pas ; rend l'opacité à peindre, ou `None` quand
/// l'accueil est fini.
fn avancer(pas: u32) -> Option<f32> {
    ETAT.with(|e| {
        let mut e = e.try_borrow_mut().ok()?;
        let e = e.as_mut()?;
        e.ecoule += pas;
        let (phase, duree) = match e.phase {
            Phase::Entree => (Phase::Tenue, FONDU_ENTREE_MS),
            Phase::Tenue => (Phase::Sortie, TENUE_MS),
            Phase::Sortie => {
                return (e.ecoule < FONDU_SORTIE_MS).then(|| opacite(Phase::Sortie, e.ecoule));
            }
        };
        if e.ecoule >= duree {
            e.phase = phase;
            e.ecoule = 0;
        }
        Some(opacite(e.phase, e.ecoule))
    })
}

/// Un clic écourte la tenue : on passe au fondu de sortie, depuis l'opacité
/// courante (une entrée interrompue ne saute pas au plein).
fn ecourter() {
    ETAT.with(|e| {
        let Ok(mut e) = e.try_borrow_mut() else { return };
        let Some(e) = e.as_mut() else { return };
        if e.phase == Phase::Sortie {
            return;
        }
        let courante = opacite(e.phase, e.ecoule);
        e.phase = Phase::Sortie;
        // Le temps de sortie qui donne l'opacité courante.
        e.ecoule = (0..=FONDU_SORTIE_MS).find(|&t| opacite(Phase::Sortie, t) <= courante).unwrap_or(0);
    });
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER if wparam.0 == MINUTERIE => {
            match avancer(PAS_MS) {
                Some(alpha) => peindre(alpha),
                None => {
                    let _ = KillTimer(Some(hwnd), MINUTERIE);
                    let _ = DestroyWindow(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP | WM_RBUTTONUP => {
            ecourter();
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_carte_ne_depasse_ni_l_ecran_ni_l_image() {
        let (l, h) = taille_carte(960, 496, 1920);
        assert!(l as f32 <= 1920.0 * PART_ECRAN + 1.0, "{l}");
        let (l_grand, _) = taille_carte(960, 496, 7680);
        assert!(l_grand as f32 <= 960.0 / (1.0 - 2.0 * MARGE) + 1.0, "{l_grand}");
        assert!(h > 0 && h < l);
    }

    #[test]
    fn les_coins_sont_transparents_et_le_centre_opaque() {
        let (l, h) = (200u32, 100u32);
        let logo = vec![255u8; 4];
        let carte = composer_carte(l, h, &logo, 1, 1);
        let alpha = |x: u32, y: u32| carte[((y * l + x) * 4 + 3) as usize];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(l - 1, h - 1), 0);
        assert_eq!(alpha(l / 2, 0), 255);
        assert_eq!(alpha(l / 2, h / 2), 255);
        // Le pixel du logo (blanc opaque) est posé au centre ; autour, du noir.
        let px = |x: u32, y: u32| {
            let c = ((y * l + x) * 4) as usize;
            carte[c..c + 4].to_vec()
        };
        assert_eq!(px((l - 1) / 2, (h - 1) / 2), vec![255, 255, 255, 255]);
        assert_eq!(px(l / 2 + 5, h / 2), vec![0, 0, 0, 255]);
    }

    #[test]
    fn l_opacite_monte_tient_puis_descend() {
        assert_eq!(opacite(Phase::Entree, 0), 0.0);
        assert_eq!(opacite(Phase::Entree, FONDU_ENTREE_MS), 1.0);
        assert_eq!(opacite(Phase::Tenue, 500), 1.0);
        assert_eq!(opacite(Phase::Sortie, FONDU_SORTIE_MS), 0.0);
    }

    #[test]
    fn le_logo_embarque_se_decode() {
        let logo = image::load_from_memory(LOGO_PNG).expect("logo décodable");
        assert!(logo.width() > 0 && logo.height() > 0);
    }
}
