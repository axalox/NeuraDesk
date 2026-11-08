//! Rendu d'une image SVG en RGBA, par le moteur SVG de **Direct2D**
//! (`ID2D1DeviceContext5::CreateSvgDocument`, Windows 10 1703+).
//!
//! Pourquoi Direct2D plutôt qu'une bibliothèque (`resvg`) : il est DANS Windows,
//! donc aucune dépendance à ajouter pour quelques icônes. Il sait ce dont les
//! avatars de profils Firefox ont besoin (voir `win32::firefox_profiles`) —
//! chemins, `fill-rule="evenodd"`, `clip-path` — et ignore ce qu'il ne connaît
//! pas (`context-fill`, remplacé AVANT par l'appelant).
//!
//! Le rendu se fait hors écran, dans un bitmap WIC : pas de fenêtre, pas de
//! périphérique Direct3D.

use windows::core::{Interface, GUID};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DeviceContext5, ID2D1Factory1, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_RENDER_TARGET_PROPERTIES,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapCacheOnLoad,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::SHCreateMemStream;

/// Rend `svg` dans un carré de `size` pixels, en RGBA8 NON prémultiplié (la
/// forme de `gfx::icon::icon_to_rgba`, donc de toutes les icônes du dock).
///
/// Le document est posé dans une fenêtre de `size`×`size` : un SVG qui déclare
/// `width`/`height` en dur (les avatars : `48`) doit être réécrit par
/// l'appelant pour occuper le carré — sa `viewBox` fait alors la mise à
/// l'échelle.
///
/// Suppose COM initialisé sur ce fil (c'est le cas du fil d'interface).
pub fn svg_rgba(svg: &str, size: u32) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let bitmap = wic
            .CreateBitmap(size, size, &GUID_WICPixelFormat32bppPBGRA as *const GUID, WICBitmapCacheOnLoad)
            .ok()?;
        let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
        let properties = D2D1_RENDER_TARGET_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            ..Default::default()
        };
        let target = factory.CreateWicBitmapRenderTarget(&bitmap, &properties).ok()?;
        // Le moteur SVG n'existe que sur `ID2D1DeviceContext5` : une cible WIC
        // créée par une fabrique `ID2D1Factory1` en EST un.
        let context: ID2D1DeviceContext5 = target.cast().ok()?;
        let stream = SHCreateMemStream(Some(svg.as_bytes()))?;
        let document = context
            .CreateSvgDocument(&stream, D2D_SIZE_F { width: size as f32, height: size as f32 })
            .ok()?;
        context.BeginDraw();
        context.Clear(Some(&D2D1_COLOR_F::default()));
        context.DrawSvgDocument(&document);
        context.EndDraw(None, None).ok()?;

        let stride = size * 4;
        let mut pixels = vec![0u8; (stride * size) as usize];
        bitmap.CopyPixels(std::ptr::null(), stride, &mut pixels).ok()?;
        Some((size, size, unpremultiply_bgra(pixels)))
    }
}

/// **FONCTION PURE, ET TESTÉE** : BGRA prémultiplié (la sortie de Direct2D) →
/// RGBA droit.
fn unpremultiply_bgra(mut pixels: Vec<u8>) -> Vec<u8> {
    for px in pixels.chunks_exact_mut(4) {
        let (b, g, r, a) = (px[0], px[1], px[2], px[3]);
        let straight = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
        px[0] = straight(r);
        px[1] = straight(g);
        px[2] = straight(b);
        px[3] = a;
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

    #[test]
    fn unpremultiply_bgra_restores_straight_rgba() {
        assert_eq!(unpremultiply_bgra(vec![0, 0, 128, 128]), vec![255, 0, 0, 128]);
        assert_eq!(unpremultiply_bgra(vec![9, 9, 9, 0]), vec![0, 0, 0, 0]);
        assert_eq!(unpremultiply_bgra(vec![10, 20, 30, 255]), vec![30, 20, 10, 255]);
    }

    /// Un disque rouge dans une `viewBox` de 48, rendu à 96 : le centre est
    /// rouge et opaque, le coin transparent — le moteur tourne et met à
    /// l'échelle.
    #[test]
    fn svg_rgba_renders_and_scales_a_disc() {
        unsafe { let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); }
        let svg = r##"<svg width="96" height="96" viewBox="0 0 48 48" xmlns="http://www.w3.org/2000/svg"><circle cx="24" cy="24" r="24" fill="#ff0000"/></svg>"##;
        let (w, h, px) = svg_rgba(svg, 96).expect("rendu Direct2D");
        assert_eq!((w, h), (96, 96));
        let at = |x: u32, y: u32| &px[((y * w + x) * 4) as usize..][..4];
        assert_eq!(at(48, 48), &[255, 0, 0, 255]);
        assert_eq!(at(0, 0)[3], 0);
    }
}
