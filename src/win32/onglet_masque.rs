//! **L'ONGLET DE L'INTERFACE MASQUÉE** — DEMANDE EXPLICITE (2026-10-02) : « quand
//! l'interface est masquée, il faudrait que cette applet reste visible dans un
//! tout petit onglet à l'écran, toujours visible, qui fasse que l'applet soit
//! toujours visible au même emplacement sur l'écran ».
//!
//! Une fenêtre minuscule, posée EXACTEMENT sur le rectangle où l'applet de
//! masquage était dessinée (voir `ui::topbar::masque_rect`), au premier plan et
//! sans jamais prendre le focus. Un clic gauche la signale (`a_ete_cliquee`) ;
//! c'est `app::run` qui réaffiche l'interface et la cache.
//!
//! **GDI, pas D3D** : elle n'affiche qu'un glyphe sur un fond uni, et elle doit
//! vivre pendant que tout le reste est caché — pas de chaîne d'échange, pas de
//! dépendance au rendu du dock. Modèle de fenêtre : `win32::fullscreen_clock`
//! (au premier plan, sans activation), mais CLIQUABLE — sans
//! `WS_EX_TRANSPARENT`.
//!
//! Le glyphe est un œil OUVERT (`E890`, « View ») : il dit ce que le clic fait,
//! réafficher. L'applet de la barre montre l'œil barré.
//!
//! ⭐ **SOUS UN THÈME D'ICÔNES EN COULEUR**, c'est l'icône de l'applet de ce
//! thème (le singe qui se cache les yeux) — DÉFAUT SIGNALÉ (2026-10-12) :
//! « c'est toujours l'ancienne icône quand l'app est masquée ». Posée par
//! `AlphaBlend` : `DrawTextW` ne sait dessiner qu'un glyphe d'une seule teinte.

use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::{w, Result};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AlphaBlend, BeginPaint, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateRoundRectRgn, CreateSolidBrush,
    DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, InvalidateRect, SelectObject, SetBkMode, SetTextColor,
    SetWindowRgn, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, DIB_RGB_COLORS, DT_CENTER,
    DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, HDC, HGDIOBJ, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, RegisterClassExW, SetWindowPos, ShowWindow, CS_HREDRAW,
    CS_VREDRAW, HWND_TOPMOST, IDC_HAND, LoadCursorW, MA_NOACTIVATE, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE,
    WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_PAINT, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

/// Le glyphe de l'onglet — voir la doc de module.
const GLYPHE: &str = "\u{E890}";

/// Part du côté de l'onglet occupée par le glyphe : la même respiration qu'une
/// icône de la barre dans sa case.
const PART_DU_GLYPHE: f32 = 0.7;

/// Rayon des coins, en fraction du côté.
const PART_DU_RAYON: f32 = 0.3;

/// Posé par le `wndproc`, relevé (et remis à zéro) par `a_ete_cliquee`.
static CLIQUEE: AtomicBool = AtomicBool::new(false);

/// Vrai une fois depuis le dernier appel si l'onglet a été cliqué.
pub fn a_ete_cliquee() -> bool {
    CLIQUEE.swap(false, Ordering::Relaxed)
}

/// La fenêtre de l'onglet. Créée une fois, cachée tant que l'interface est
/// visible.
pub struct OngletMasque {
    hwnd: HWND,
    visible: bool,
}

impl OngletMasque {
    pub fn new() -> Result<Self> {
        let instance = unsafe { GetModuleHandleW(None)? };
        let classe = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: unsafe { LoadCursorW(None, IDC_HAND)? },
            lpszClassName: w!("NeuraDeskOngletMasque"),
            ..Default::default()
        };
        unsafe { RegisterClassExW(&classe) };
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                w!("NeuraDeskOngletMasque"),
                w!("NeuraDesk"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance.into()),
                None,
            )?
        };
        Ok(Self { hwnd, visible: false })
    }

    /// Montre l'onglet sur `rect` (coordonnées ÉCRAN, pixels physiques).
    pub fn montrer(&mut self, rect: RECT) {
        let (largeur, hauteur) = ((rect.right - rect.left).max(8), (rect.bottom - rect.top).max(8));
        let rayon = ((largeur.min(hauteur) as f32) * PART_DU_RAYON).round() as i32;
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), rect.left, rect.top, largeur, hauteur, SWP_NOACTIVATE);
            // La région est ADOPTÉE par la fenêtre : on ne la libère pas.
            let region = CreateRoundRectRgn(0, 0, largeur + 1, hauteur + 1, rayon, rayon);
            SetWindowRgn(self.hwnd, Some(region), true);
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            let _ = InvalidateRect(Some(self.hwnd), None, true);
        }
        self.visible = true;
    }

    pub fn cacher(&mut self) {
        if self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
        }
        CLIQUEE.store(false, Ordering::Relaxed);
    }

    /// Réaffirme le premier plan — une fenêtre `TOPMOST` ouverte après nous
    /// passerait devant. Appelée par `app::run` tant que l'onglet est montré.
    pub fn garder_au_premier_plan(&self) {
        if self.visible {
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE,
                );
            }
        }
    }
}

/// `[0.0, 1.0]` → `COLORREF` (`0x00BBGGRR`).
fn couleur(rgb: [f32; 3]) -> COLORREF {
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    COLORREF(c(rgb[0]) | (c(rgb[1]) << 8) | (c(rgb[2]) << 16))
}

fn peindre(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    // Le fond : l'accent des menus, assombri — la même famille que les menus
    // et l'horloge de plein écran, qui sont teintés par lui.
    let accent = crate::win32::menu::menu_accent_rgb();
    let fond = unsafe { CreateSolidBrush(couleur([accent[0] * 0.55, accent[1] * 0.55, accent[2] * 0.55])) };
    unsafe {
        FillRect(hdc, &client, fond);
        let _ = DeleteObject(HGDIOBJ(fond.0));
    }
    let cote = (client.right - client.left).min(client.bottom - client.top);
    if let Some(png) = crate::ui::icones::masque_en_couleur() {
        if poser_le_png(hdc, &client, png, (cote as f32 * PART_DU_GLYPHE).round() as i32) {
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            return;
        }
    }
    let police = unsafe {
        CreateFontW(
            -((cote as f32 * PART_DU_GLYPHE).round() as i32),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            0,
            w!("Segoe Fluent Icons"),
        )
    };
    unsafe {
        let ancienne = SelectObject(hdc, HGDIOBJ(police.0));
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, couleur(crate::ui::color::menu_fg()));
        let mut texte: Vec<u16> = GLYPHE.encode_utf16().collect();
        DrawTextW(hdc, &mut texte, &mut client, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        SelectObject(hdc, ancienne);
        let _ = DeleteObject(HGDIOBJ(police.0));
        let _ = EndPaint(hwnd, &ps);
    }
}

/// Pose `png` au centre de `client`, réduit à `taille` px de côté, par
/// `AlphaBlend` (alpha PRÉMULTIPLIÉ, comme il l'exige). `false` si le PNG ne se
/// décode pas ou si GDI refuse : l'appelant retombe sur le glyphe.
fn poser_le_png(hdc: HDC, client: &RECT, png: &[u8], taille: i32) -> bool {
    let Ok(image) = image::load_from_memory(png) else { return false };
    let taille = taille.max(1);
    let image = image.resize_exact(taille as u32, taille as u32, image::imageops::FilterType::Lanczos3).to_rgba8();
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: taille,
            // Négatif : lignes de HAUT en bas, comme l'image.
            biHeight: -taille,
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        let mut bits = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(hdc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else { return false };
        let pixels = std::slice::from_raw_parts_mut(bits as *mut u8, (taille * taille * 4) as usize);
        for (dst, src) in pixels.chunks_exact_mut(4).zip(image.pixels()) {
            let [r, g, b, a] = src.0;
            let pre = |c: u8| ((c as u32 * a as u32 + 127) / 255) as u8;
            dst.copy_from_slice(&[pre(b), pre(g), pre(r), a]);
        }
        let memoire = CreateCompatibleDC(Some(hdc));
        let ancien = SelectObject(memoire, HGDIOBJ(bitmap.0));
        let x = client.left + (client.right - client.left - taille) / 2;
        let y = client.top + (client.bottom - client.top - taille) / 2;
        let melange = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let pose = AlphaBlend(hdc, x, y, taille, taille, memoire, 0, 0, taille, taille, melange).as_bool();
        SelectObject(memoire, ancien);
        let _ = DeleteDC(memoire);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        pose
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            peindre(hwnd);
            LRESULT(0)
        }
        // Cliquer ne doit JAMAIS voler le focus à la fenêtre de l'utilisateur.
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONUP => {
            CLIQUEE.store(true, Ordering::Relaxed);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::{GetDIBits, BI_RGB};

    /// ⭐ **L'ICÔNE EN COULEUR ARRIVE EN COULEUR DANS L'ONGLET** : le singe posé
    /// par `poser_le_png` sur un fond gris laisse des pixels COLORÉS, centrés,
    /// et les coins restent au fond — l'alpha est respecté.
    #[test]
    fn le_png_en_couleur_est_pose_en_couleur_au_centre() {
        // Un PNG EN COULEUR, quel qu'il soit : c'est `poser_le_png` qu'on éprouve,
        // pas le thème. `symboles/` a été retiré le 2026-10-28 (voir `ui::icones`).
        let png = include_bytes!("../../TopbarIcons/fluent_couleur/masque.png");
        let cote = 64;
        unsafe {
            let dc = CreateCompatibleDC(None);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: cote,
                    biHeight: -cote,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let toile = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
            let ancien = SelectObject(dc, HGDIOBJ(toile.0));
            let gris = CreateSolidBrush(COLORREF(0x00404040));
            let client = RECT { left: 0, top: 0, right: cote, bottom: cote };
            FillRect(dc, &client, gris);
            assert!(poser_le_png(dc, &client, png, 44), "AlphaBlend a refusé");
            let mut pixels = vec![0u8; (cote * cote * 4) as usize];
            let mut lecture = info;
            GetDIBits(dc, toile, 0, cote as u32, Some(pixels.as_mut_ptr() as *mut _), &mut lecture, DIB_RGB_COLORS);
            SelectObject(dc, ancien);
            let _ = DeleteObject(HGDIOBJ(gris.0));
            let _ = DeleteObject(HGDIOBJ(toile.0));
            let _ = DeleteDC(dc);
            let px = |x: i32, y: i32| {
                let i = ((y * cote + x) * 4) as usize;
                (pixels[i + 2], pixels[i + 1], pixels[i])
            };
            assert_eq!(px(1, 1), (0x40, 0x40, 0x40), "coin : le fond, pas l'icône");
            let colores = (0..cote)
                .flat_map(|y| (0..cote).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    let (r, g, b) = px(x, y);
                    r.max(g).max(b) - r.min(g).min(b) > 40
                })
                .count();
            assert!(colores > 300, "{colores} pixels colorés seulement");
        }
    }
}
