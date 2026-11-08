//! Extraction d'une icône d'application réelle depuis un exécutable (via
//! `ExtractIconExW`, la même API que l'Explorateur utilise pour peupler ses
//! listes de fichiers) et upload en texture Direct3D — pour remplacer les
//! carrés de couleur placeholder du dock par les vraies icônes des
//! applications épinglées (voir `win32::appicon`, façon dock macOS de
//! Seelen UI : "vraies icônes + lancement/focus").

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::Graphics::Direct3D11::ID3D11Device;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, HGDIOBJ, BITMAP,
    BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
};
use windows::Win32::UI::Shell::ExtractIconExW;
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

use crate::gfx::texture::Texture;

/// Extrait la première icône "grande" (généralement 32×32, la taille que
/// l'Explorateur utilise pour ses listes) de l'exécutable `path` et
/// l'upload en texture RGBA. `None` en cas d'échec (fichier introuvable,
/// sans icône, etc.) — l'appelant retombe alors sur le carré de couleur
/// placeholder plutôt que de planter (voir `ui::dock::Icon`).
pub fn extract_icon_texture(device: &ID3D11Device, path: &Path) -> Option<Texture> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let mut large_icon = HICON::default();
        let count = ExtractIconExW(PCWSTR(wide.as_ptr()), 0, Some(&mut large_icon), None, 1);
        if count == 0 || large_icon.is_invalid() {
            return None;
        }

        let rgba = icon_to_rgba(large_icon);
        let _ = DestroyIcon(large_icon);
        let (width, height, pixels) = rgba?;
        Texture::from_rgba(device, width, height, &pixels).ok()
    }
}

/// Convertit un `HICON` en pixels RGBA8 (largeur, hauteur, données) via
/// `GetIconInfo` + `GetDIBits` sur son bitmap couleur — les icônes 32bpp
/// modernes (le cas normal depuis Windows Vista pour toute icône
/// d'exécutable actuel) stockent déjà un canal alpha exploitable
/// directement, sans avoir besoin de composer avec le bitmap de masque.
///
/// ⛔ **SAUF LES ICÔNES À L'ANCIENNE, ET IL EN RESTE** — mesuré le 2026-10-02 sur
/// `shotcut.exe` : 32×32, 870 pixels colorés, **alpha à 0 partout**. Sa forme est
/// dans le MASQUE (1 bit), et lire l'alpha seul donnait une icône entièrement
/// transparente : la place était prise dans le dock, l'icône invisible. Quand
/// aucun pixel n'a d'alpha, l'alpha est donc tiré du masque (`alpha_depuis_masque`).
/// `pub(crate)` : réutilisée par `win32::dash` pour charger les icônes
/// d'application en texture `egui` (voir sa doc).
pub(crate) unsafe fn icon_to_rgba(icon: HICON) -> Option<(u32, u32, Vec<u8>)> {
    let mut info = ICONINFO::default();
    GetIconInfo(icon, &mut info).ok()?;
    // `hbmMask` ne sert qu'aux icônes sans alpha (voir doc de fonction), mais doit
    // être libéré comme `hbmColor` dans tous les cas pour ne pas fuir de
    // ressource GDI.
    let _mask_guard = GdiObjectGuard(info.hbmMask.0);
    let color_guard = GdiObjectGuard(info.hbmColor.0);

    let mut bitmap = BITMAP::default();
    let bitmap_size = std::mem::size_of::<BITMAP>() as i32;
    if GetObjectW(info.hbmColor.into(), bitmap_size, Some(&mut bitmap as *mut _ as *mut _)) == 0 {
        return None;
    }
    let width = bitmap.bmWidth as u32;
    let height = bitmap.bmHeight as u32;
    if width == 0 || height == 0 {
        return None;
    }

    let screen_dc = GetDC(None);
    let mem_dc = CreateCompatibleDC(Some(screen_dc));

    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32), // top-down, comme `gfx::text`
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0, // BI_RGB
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bgra = vec![0u8; (width * height * 4) as usize];
    let lines = GetDIBits(
        mem_dc,
        info.hbmColor,
        0,
        height,
        Some(bgra.as_mut_ptr() as *mut _),
        &mut bmi,
        DIB_RGB_COLORS,
    );

    // Le masque, lu dans le MÊME format 32 bits : GDI convertit le monochrome, noir
    // (0 = opaque) et blanc (1 = transparent). Lu seulement s'il servira.
    let sans_alpha = lines != 0 && bgra.chunks_exact(4).all(|px| px[3] == 0);
    let masque = if sans_alpha && !info.hbmMask.is_invalid() {
        let mut masque = vec![0u8; bgra.len()];
        let lues = GetDIBits(mem_dc, info.hbmMask, 0, height, Some(masque.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
        (lues != 0).then_some(masque)
    } else {
        None
    };

    let _ = DeleteDC(mem_dc);
    ReleaseDC(None, screen_dc);
    drop(color_guard);

    if lines == 0 {
        return None;
    }
    if let Some(masque) = masque {
        alpha_depuis_masque(&mut bgra, &masque);
    }

    // GDI donne du BGRA ; notre pipeline (voir `gfx::texture::Texture::from_rgba`)
    // attend du RGBA.
    let mut rgba = vec![0u8; bgra.len()];
    for (dst, src) in rgba.chunks_exact_mut(4).zip(bgra.chunks_exact(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }

    Some((width, height, rgba))
}

/// **L'ALPHA D'UNE ICÔNE À L'ANCIENNE, TIRÉ DE SON MASQUE** — fonction PURE,
/// testable. `masque` est le masque ET relu en 32 bits : un pixel NOIR y est
/// opaque, un pixel BLANC transparent. Les deux tampons sont en BGRA, de même
/// taille.
fn alpha_depuis_masque(bgra: &mut [u8], masque: &[u8]) {
    for (px, m) in bgra.chunks_exact_mut(4).zip(masque.chunks_exact(4)) {
        px[3] = if m[0] == 0 && m[1] == 0 && m[2] == 0 { 255 } else { 0 };
    }
}

/// Rogne les marges transparentes autour du contenu visible d'une icône
/// RGBA8 — certaines icônes (notamment les logos d'applications UWP/Store,
/// voir `win32::uwp_apps::logo_rgba`, conçues pour un masque de forme et un
/// effet d'ombre appliqués par Windows lui-même dans le menu Démarrer/Start
/// tile, PAS pour un affichage brut) embarquent une bonne marge
/// transparente tout autour du glyphe visible. Affichée telle quelle dans
/// une tuile de taille fixe (voir `win32::dash`, `.tile img`, `object-fit:
/// contain`), cette marge fait paraître l'icône "toute petite" au milieu
/// d'un grand espace vide — même une fois la tuile correctement
/// dimensionnée (voir historique : `SHGFI_LARGEICON` remplacée par l'icône
/// "jumbo" pour les raccourcis classiques, qui n'a pas résolu ce cas précis
/// puisque le problème n'est pas la résolution mais le cadrage de l'image
/// elle-même). Sans effet notable sur une icône déjà bien cadrée (la
/// quasi-totalité des icônes d'exécutables Win32 classiques) : la marge
/// ajoutée après rognage (voir `PADDING_FRACTION`) reproduit alors presque
/// le cadrage d'origine.
///
/// Seuil de transparence à 10/255 (pas 0) : ignore la frange
/// semi-transparente d'anti-crénelage sur le contour du glyphe, qui
/// élargirait sinon la marge "détectée" de quelques pixels sans rien
/// apporter visuellement.
pub(crate) fn trim_transparent_padding(width: u32, height: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
    const ALPHA_THRESHOLD: u8 = 10;
    const PADDING_FRACTION: f32 = 0.06;

    let is_opaque = |x: u32, y: u32| -> bool {
        let idx = ((y * width + x) * 4 + 3) as usize;
        rgba.get(idx).is_some_and(|&a| a > ALPHA_THRESHOLD)
    };

    let (mut min_x, mut max_x, mut min_y, mut max_y) = (width, 0u32, height, 0u32);
    for y in 0..height {
        for x in 0..width {
            if is_opaque(x, y) {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }

    // Entièrement transparente, ou déjà cadrée pile sur les bords (rien à
    // rogner) : renvoie l'image telle quelle plutôt que de risquer une
    // dimension nulle ou une copie inutile.
    let fully_framed = min_x == 0 && min_y == 0 && max_x == width.saturating_sub(1) && max_y == height.saturating_sub(1);
    if min_x > max_x || min_y > max_y || fully_framed {
        return (width, height, rgba.to_vec());
    }

    let content_w = max_x - min_x + 1;
    let content_h = max_y - min_y + 1;
    let pad = ((content_w.max(content_h) as f32) * PADDING_FRACTION).round() as u32;
    let crop_x0 = min_x.saturating_sub(pad);
    let crop_y0 = min_y.saturating_sub(pad);
    let crop_x1 = (max_x + pad).min(width - 1);
    let crop_y1 = (max_y + pad).min(height - 1);
    let crop_w = crop_x1 - crop_x0 + 1;
    let crop_h = crop_y1 - crop_y0 + 1;

    let mut cropped = vec![0u8; (crop_w * crop_h * 4) as usize];
    for y in 0..crop_h {
        let src_start = (((crop_y0 + y) * width + crop_x0) * 4) as usize;
        let src_row = &rgba[src_start..src_start + (crop_w * 4) as usize];
        let dst_start = (y * crop_w * 4) as usize;
        cropped[dst_start..dst_start + (crop_w * 4) as usize].copy_from_slice(src_row);
    }

    (crop_w, crop_h, cropped)
}

/// Libère un `HBITMAP` GDI à la destruction — évite d'oublier un
/// `DeleteObject` sur un chemin d'erreur (`?`/retour anticipé) dans
/// `icon_to_rgba`.
struct GdiObjectGuard(*mut core::ffi::c_void);

impl Drop for GdiObjectGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(self.0));
            }
        }
    }
}

#[cfg(test)]
mod tests_masque {
    use super::alpha_depuis_masque;

    /// Un pixel NOIR du masque rend le pixel opaque, un BLANC le rend
    /// transparent — et les couleurs ne bougent pas.
    #[test]
    fn le_masque_donne_l_alpha_d_une_icone_sans_alpha() {
        let mut bgra = vec![10, 20, 30, 0, 40, 50, 60, 0];
        let masque = vec![0, 0, 0, 0, 255, 255, 255, 0];
        alpha_depuis_masque(&mut bgra, &masque);
        assert_eq!(bgra, vec![10, 20, 30, 255, 40, 50, 60, 0]);
    }
}
