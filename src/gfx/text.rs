//! Rendu de texte à la volée (l'horloge et le glyphe du bouton
//! d'alimentation dans la barre du haut) : GDI dessine le texte en
//! niveaux de gris anti-aliasés sur un DIB noir, puis on réinterprète
//! cette luminance comme canal alpha (texte blanc, fond transparent).
//! Évite toute dépendance à une bibliothèque de police tierce pour de
//! simples libellés.

use windows::core::{Result, PCWSTR};
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Direct3D11::ID3D11Device;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, GetDC,
    GetTextExtentPoint32W, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TextOutW,
    ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    DEFAULT_PITCH, DIB_RGB_COLORS, FF_DONTCARE, FW_BOLD, FW_NORMAL, HDC, OUT_DEFAULT_PRECIS,
    TRANSPARENT,
};

use crate::gfx::texture::Texture;

/// Rend `text` en texte blanc anti-aliasé sur fond transparent, dans la
/// police `font_name` à `font_px` pixels physiques de haut (en gras si
/// `bold`, ex. l'horloge — DEMANDE EXPLICITE "fais-la s'afficher en gras"),
/// et l'upload en texture D3D11. Retourne la texture ainsi que sa taille en
/// pixels (nécessaire pour positionner/dimensionner le quad qui l'affiche).
pub fn render_text(
    device: &ID3D11Device,
    text: &str,
    font_px: i32,
    font_name: &str,
    bold: bool,
) -> Result<(Texture, u32, u32)> {
    let (width, height, rgba) = rendre_rgba(text, font_px, font_name, bold)?;
    let texture = Texture::from_rgba(device, width, height, &rgba)?;
    Ok((texture, width, height))
}

/// **LA MÊME CHOSE, MAIS ROGNÉE À L'ENCRE DU DESSIN PUIS COMPLÉTÉE À UN CARRÉ.**
///
/// POURQUOI UNE VARIANTE, ET NON UN CHANGEMENT DE `render_text` : les autres
/// appelants (horloge, noms de zones, infobulles) dimensionnent leur quad avec la
/// taille RENDUE. Rogner leur texture déplacerait et rétrécirait leur texte. Seul
/// le miroir de notification veut une texture qui REMPLIT sa case — c'est le même
/// traitement que les instantanés, qui passent par
/// `win32::tray_icons::trim_to_content` depuis toujours.
///
/// DÉFAUT SIGNALÉ, ET C'EST CE QUI L'IMPOSE : `render_text` dimensionne sa texture
/// avec `GetTextExtentPoint32W`, soit la **boîte de la ligne et l'avance de la
/// police** — pas l'encre du glyphe. Étirée à la case, elle y mettait donc le
/// glyphe PLUS PETIT, entouré de vide : « certaines icônes sont plus petites que
/// les autres », et les entrées sans nom n'étaient plus qu'un point blanc. Un
/// glyphe de police n'a aucune raison de remplir sa boîte d'avance.
pub fn render_text_trimmed(
    device: &ID3D11Device,
    text: &str,
    font_px: i32,
    font_name: &str,
    bold: bool,
) -> Result<(Texture, u32, u32)> {
    let (width, height, rgba) = rendre_rgba(text, font_px, font_name, bold)?;
    // Pas de couleur de fond à détacher : le rendu sort du blanc à alpha égal à
    // la couverture, donc transparent par construction.
    let (cote, _, carre) =
        crate::win32::tray_icons::trim_to_content(&rgba, width, height, None);
    let texture = Texture::from_rgba(device, cote, cote, &carre)?;
    Ok((texture, cote, cote))
}

/// Alpha minimal compté comme de l'ENCRE par `boite_de_l_encre` — un pixel plus
/// transparent que ça est une frange d'anticrénelage, pas un trait. Sans ce
/// seuil, la boîte de l'encre serait celle de la boîte de la police à un pixel
/// près, et la mesure n'apprendrait rien.
pub(crate) const SEUIL_D_ENCRE: u8 = 16;

/// Boîte de l'ENCRE d'un rendu de `rendre_rgba` — `(x0, y0, x1, y1)`, coins
/// INCLUS, `None` si rien n'a été dessiné.
///
/// **POURQUOI LA MESURER PLUTÔT QUE LA DÉDUIRE** : `rendre_rgba` dimensionne sa
/// texture avec `GetTextExtentPoint32W`, soit la BOÎTE DE LA LIGNE de la police,
/// pas l'encre du glyphe (voir `render_text_trimmed`, qui existe pour cette
/// raison exacte). Or un chiffre n'a pas de jambage : dans sa boîte de ligne, son
/// encre est collée en HAUT, avec tout l'espace du jambage laissé vide en dessous.
/// Centrer une boîte de ligne n'est donc pas centrer un chiffre — et l'écart ne
/// se calcule pas sans les métriques de la police, alors qu'il se MESURE.
///
/// Sert aux sondes (voir `ui::desktop_labels::mesure_les_numeros_des_pastilles`),
/// aux tests qui vérifient qu'un texte tient dans sa case, et au thème d'icônes
/// « Fluent » (`ui::icones`), qui centre ainsi un glyphe de police dans le carré
/// d'une icône.
pub(crate) fn boite_de_l_encre(rgba: &[u8], width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let mut boite: Option<(u32, u32, u32, u32)> = None;
    for y in 0..height {
        for x in 0..width {
            let alpha = rgba[((y * width + x) * 4 + 3) as usize];
            if alpha < SEUIL_D_ENCRE {
                continue;
            }
            boite = Some(match boite {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        }
    }
    boite
}

/// Le rendu GDI lui-même — partagé par les deux formes publiques ci-dessus.
///
/// `pub(crate)` pour les SONDES qui doivent lire l'encre et pas seulement la
/// boîte (voir `boite_de_l_encre` et `ui::desktop_labels`, dont les ratios de
/// police se calibrent sur ces mesures).
pub(crate) fn rendre_rgba(text: &str, font_px: i32, font_name: &str, bold: bool) -> Result<(u32, u32, Vec<u8>)> {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let font_name_wide: Vec<u16> = font_name.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(screen_dc));

        // `charset`/`output_precision`/`clip_precision`/`quality` prennent
        // désormais directement leur propre type fort (`FONT_CHARSET`/
        // `FONT_OUTPUT_PRECISION`/`FONT_CLIP_PRECISION`/`FONT_QUALITY`) —
        // PAS `.0 as u32` (jusqu'ici nécessaire, ces quatre paramètres
        // n'étaient que des `u32` bruts).
        let font = create_font(font_px, &font_name_wide, bold);
        let old_font = SelectObject(mem_dc, font.into());

        let mut extent = Default::default();
        let _ = GetTextExtentPoint32W(mem_dc, &wide, &mut extent);
        let width = extent.cx.max(1) as u32;
        let height = extent.cy.max(1) as u32;

        let (bitmap, bits) = create_dib(mem_dc, width, height)?;
        let old_bitmap = SelectObject(mem_dc, bitmap.into());

        // Fond noir (luminance 0 => alpha 0 après conversion), texte blanc
        // (luminance 255 => alpha pleine).
        let pixel_count = (width * height) as usize;
        std::ptr::write_bytes(bits, 0u8, pixel_count * 4);

        SetBkMode(mem_dc, TRANSPARENT);
        SetTextColor(mem_dc, COLORREF(0x00FFFFFF));
        let _ = TextOutW(mem_dc, 0, 0, &wide);

        // DIB 32bpp GDI = octets (B, G, R, réservé) par pixel ; texte blanc
        // sur fond noir => B = G = R = couverture. On relit cette couverture
        // comme alpha et on sort du blanc opaque à cette alpha (format RGBA
        // attendu par `Texture::from_rgba`).
        let src = std::slice::from_raw_parts(bits as *const u8, pixel_count * 4);
        let mut rgba = vec![0u8; pixel_count * 4];
        for (dst, src_px) in rgba.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            let coverage = src_px[0];
            dst[0] = 255;
            dst[1] = 255;
            dst[2] = 255;
            dst[3] = coverage;
        }

        SelectObject(mem_dc, old_font);
        SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(font.into());
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem_dc);
        ReleaseDC(None, screen_dc);

        Ok((width, height, rgba))
    }
}

/// **LA définition unique de la police** de tout texte rendu ici — extraite de
/// `render_text` pour que `measure_text` mesure avec EXACTEMENT la même. Deux
/// créations divergentes (l'une en gras, l'autre non, ou un charset différent)
/// donneraient une mesure qui ne décrit pas ce qui est dessiné : on ajusterait
/// une taille pour faire tenir un texte, et le texte rendu déborderait quand
/// même.
///
/// Pas de ClearType : il faudrait un fond opaque connu pour compenser le
/// lissage sous-pixel RVB, ce qu'on n'a pas ici (fond transparent composé par
/// DWM). Le lissage niveaux de gris donne R=G=B, ce dont la conversion
/// luminance → alpha de `render_text` a besoin.
///
/// `font_name_wide` doit être terminé par un zéro.
unsafe fn create_font(font_px: i32, font_name_wide: &[u16], bold: bool) -> windows::Win32::Graphics::Gdi::HFONT {
    unsafe {
        CreateFontW(
            font_px,
            0,
            0,
            0,
            if bold { FW_BOLD.0 as i32 } else { FW_NORMAL.0 as i32 },
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            (DEFAULT_PITCH.0 as u32) | (FF_DONTCARE.0 as u32),
            PCWSTR(font_name_wide.as_ptr()),
        )
    }
}

/// Largeur et hauteur qu'occuperait `text` à `font_px` — LA MÊME mesure que
/// celle qui dimensionne la texture de `render_text` (même police, même appel
/// `GetTextExtentPoint32W`), mais SANS rien dessiner ni téléverser : c'est ce qui
/// permet de chercher une taille qui « rentre » sans rendre une texture par
/// essai (voir `ui::dock::zone_label_fit_px`, DEMANDE EXPLICITE : « les titres
/// des zones de droite réduisent automatiquement leur taille si le texte ne
/// rentre pas dans la largeur de la barre »).
///
/// La mesure est celle de la police GDI, pas une estimation par nombre de
/// caractères : un libellé en majuscules larges (« OUTILS ») et un autre en
/// minuscules étroites n'occupent pas la même place à taille égale.
pub fn measure_text(text: &str, font_px: i32, font_name: &str, bold: bool) -> (u32, u32) {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let font_name_wide: Vec<u16> = font_name.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        let font = create_font(font_px, &font_name_wide, bold);
        let old_font = SelectObject(mem_dc, font.into());

        let mut extent = Default::default();
        let _ = GetTextExtentPoint32W(mem_dc, &wide, &mut extent);

        SelectObject(mem_dc, old_font);
        let _ = DeleteObject(font.into());
        let _ = DeleteDC(mem_dc);
        ReleaseDC(None, screen_dc);

        (extent.cx.max(1) as u32, extent.cy.max(1) as u32)
    }
}

/// Crée un DIB section 32bpp top-down (origine en haut à gauche, comme les
/// coordonnées d'écran) de `width`×`height`, sélectionné dans `dc`.
/// Retourne le bitmap et un pointeur direct sur ses pixels.
unsafe fn create_dib(
    dc: HDC,
    width: u32,
    height: u32,
) -> Result<(windows::Win32::Graphics::Gdi::HBITMAP, *mut u8)> {
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0, // BI_RGB
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let bitmap = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
    Ok((bitmap, bits as *mut u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SONDE — **OÙ EST L'ENCRE D'UN GLYPHE DE `Segoe Fluent Icons` ?**
    ///
    /// Mesure : `cargo test --release --bins -- --ignored --nocapture
    /// prints_the_ink_box_of_a_shell_glyph`
    ///
    /// Elle existe parce que deux mesures se contredisaient : le rognage rend une
    /// texture de 64×64 (donc l'encre semble remplir la boîte), et le dock dessine
    /// pourtant une encre de 22 px dans une case de 44. C'est ici, sans GPU et sans
    /// dock, qu'on voit OÙ est l'encre dans le glyphe rendu.
    #[test]
    #[ignore]
    fn prints_the_ink_box_of_a_shell_glyph() {
        // Les glyphes RÉELLEMENT utilisés par le miroir, relevés dans le journal
        // de provenances de l'application.
        for (nom, glyphe) in [
            ("réseau", '\u{E701}'),
            ("volume", '\u{E767}'),
            ("clavier", '\u{E765}'),
            ("micro", '\u{E720}'),
            ("emojis", '\u{E76E}'),
            ("entrée", '\u{E774}'),
            ("avertiss", '\u{E7BA}'),
            ("protégé", '\u{EA18}'),
            ("synchro", '\u{E895}'),
            ("générique", '\u{E915}'),
        ] {
            let Ok((largeur, hauteur, rgba)) =
                rendre_rgba(&glyphe.to_string(), 64, "Segoe Fluent Icons", false)
            else {
                continue;
            };
            for seuil in [1u8, 16, 64, 128] {
                let (mut x0, mut y0, mut x1, mut y1) = (largeur, hauteur, 0u32, 0u32);
                let mut encre = 0u32;
                for y in 0..hauteur {
                    for x in 0..largeur {
                        let offset = ((y * largeur + x) * 4) as usize;
                        if rgba[offset + 3] >= seuil {
                            encre += 1;
                            x0 = x0.min(x);
                            y0 = y0.min(y);
                            x1 = x1.max(x);
                            y1 = y1.max(y);
                        }
                    }
                }
                println!(
                    "{nom:>10} U+{:04X} seuil {seuil:>3} : boîte {}x{} ({} px d'encre) — texture {largeur}x{hauteur}",
                    glyphe as u32,
                    x1.saturating_sub(x0) + 1,
                    y1.saturating_sub(y0) + 1,
                    encre,
                );
            }
        }
    }
}
