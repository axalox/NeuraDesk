//! **LE FORMAT DE PHOTOSHOP** (PSD), par la crate `ag-psd` (portage Rust de la
//! bibliothèque JavaScript du même nom).
//!
//! **LIRE** aplatit les groupes comme `format_xcf` : leurs calques gardent leur
//! place, et héritent de la visibilité et de l'opacité du groupe. Un PSD sans
//! calques (enregistré « à plat ») rend un seul calque, son image composée.
//! Les pixels arrivent en RGBA 8 bits quelle que soit la profondeur du fichier
//! (la crate réduit elle-même 16 et 32 bits).
//!
//! ⛔ `ag-psd` **PEUT PANIQUER** sur un fichier corrompu, et le dock est compilé
//! en `panic = "abort"` : la lecture d'un fichier ouvert par l'utilisateur ne
//! se fait JAMAIS dans le processus du dock — voir `decodeur_document`.
//! L'écriture, elle, ne part que de ce que l'éditeur a produit.

use ag_psd::psd::{BlendMode, Layer, PixelData, Psd, ReadOptions, WriteOptions};

use super::document_image::{cote_valide, Budget, Calque, Document, Envoi, Mode};

fn mode_depuis_psd(mode: Option<BlendMode>) -> Mode {
    match mode {
        Some(BlendMode::Multiply) => Mode::Multiply,
        Some(BlendMode::Screen) => Mode::Screen,
        Some(BlendMode::Overlay) => Mode::Overlay,
        Some(BlendMode::Darken | BlendMode::DarkerColor) => Mode::Darken,
        Some(BlendMode::Lighten | BlendMode::LighterColor) => Mode::Lighten,
        Some(BlendMode::ColorDodge | BlendMode::LinearDodge) => Mode::ColorDodge,
        Some(BlendMode::ColorBurn | BlendMode::LinearBurn) => Mode::ColorBurn,
        Some(BlendMode::HardLight) => Mode::HardLight,
        Some(BlendMode::SoftLight) => Mode::SoftLight,
        Some(BlendMode::Difference) => Mode::Difference,
        Some(BlendMode::Exclusion) => Mode::Exclusion,
        Some(BlendMode::Hue) => Mode::Hue,
        Some(BlendMode::Saturation) => Mode::Saturation,
        Some(BlendMode::Color) => Mode::Color,
        Some(BlendMode::Luminosity) => Mode::Luminosity,
        _ => Mode::Normal,
    }
}

fn mode_vers_psd(mode: Mode) -> BlendMode {
    match mode {
        Mode::Normal => BlendMode::Normal,
        Mode::Multiply => BlendMode::Multiply,
        Mode::Screen => BlendMode::Screen,
        Mode::Overlay => BlendMode::Overlay,
        Mode::Darken => BlendMode::Darken,
        Mode::Lighten => BlendMode::Lighten,
        Mode::ColorDodge => BlendMode::ColorDodge,
        Mode::ColorBurn => BlendMode::ColorBurn,
        Mode::HardLight => BlendMode::HardLight,
        Mode::SoftLight => BlendMode::SoftLight,
        Mode::Difference => BlendMode::Difference,
        Mode::Exclusion => BlendMode::Exclusion,
        Mode::Hue => BlendMode::Hue,
        Mode::Saturation => BlendMode::Saturation,
        Mode::Color => BlendMode::Color,
        Mode::Luminosity => BlendMode::Luminosity,
    }
}

/// Des pixels de la crate, s'ils sont bien en RGBA 8 bits à la taille dite.
fn pixels(p: Option<&PixelData>) -> Option<&PixelData> {
    p.filter(|p| cote_valide(p.width, p.height) && p.data.len() == (p.width as usize) * (p.height as usize) * 4)
}

/// Lit un fichier PSD (voir la doc du module).
pub fn lire(octets: &[u8]) -> Result<Document, String> {
    let options = ReadOptions { skip_thumbnail: Some(true), skip_linked_files_data: Some(true), ..ReadOptions::default() };
    let psd = ag_psd::read_psd(octets, &options).map_err(|e| format!("PSD illisible : {e:?}"))?;
    if !(psd.width >= 1.0 && psd.height >= 1.0) {
        return Err("PSD sans taille".into());
    }
    let (largeur, hauteur) = (psd.width as u32, psd.height as u32);
    if !cote_valide(largeur, hauteur) {
        return Err(format!("taille d'image PSD refusée : {largeur}×{hauteur}"));
    }
    let mut budget = Budget::nouveau();
    let mut calques = Vec::new();
    aplatir(psd.children.as_deref().unwrap_or(&[]), true, 1.0, &mut budget, &mut calques)?;
    if calques.is_empty() {
        let p = pixels(psd.canvas.as_ref().or(psd.image_data.as_ref())).ok_or("PSD sans calque ni image composée")?;
        calques.push(Calque { nom: "Fond".into(), x: 0, y: 0, largeur: p.width, hauteur: p.height, rgba: p.data.clone(), opacite: 1.0, visible: true, mode: Mode::Normal });
    }
    Ok(Document { largeur, hauteur, calques })
}

/// Les calques de `enfants` (du bas vers le haut, comme dans le PSD), groupes
/// dépliés.
fn aplatir(enfants: &[Layer], visible: bool, opacite: f32, budget: &mut Budget, sortie: &mut Vec<Calque>) -> Result<(), String> {
    for l in enfants {
        let visible = visible && !l.hidden.unwrap_or(false);
        let opacite = opacite * l.opacity.unwrap_or(1.0).clamp(0.0, 1.0) as f32;
        if let Some(sous) = &l.children {
            aplatir(sous, visible, opacite, budget, sortie)?;
            continue;
        }
        // Un calque vide (texte non rendu, calque de réglage…) n'a pas de pixels.
        let Some(p) = pixels(l.canvas.as_ref().or(l.image_data.as_ref())) else { continue };
        budget.prendre(p.width, p.height)?;
        let position = |v: Option<f64>| v.unwrap_or(0.0).clamp(-f64::from(i32::MAX), f64::from(i32::MAX)) as i32;
        sortie.push(Calque {
            nom: l.additional_info.name.clone().unwrap_or_default(),
            x: position(l.left),
            y: position(l.top),
            largeur: p.width,
            hauteur: p.height,
            rgba: p.data.clone(),
            opacite,
            visible,
            mode: mode_depuis_psd(l.blend_mode),
        });
    }
    Ok(())
}

/// Écrit l'envoi de la page en PSD : ses calques, et son image composée (celle
/// qu'affichent les logiciels qui ne lisent pas les calques).
pub fn ecrire(envoi: &Envoi) -> Result<Vec<u8>, String> {
    let d = &envoi.document;
    if !cote_valide(d.largeur, d.hauteur) || envoi.composite.len() != (d.largeur * d.hauteur * 4) as usize {
        return Err("image composée incohérente".into());
    }
    let mut enfants = Vec::new();
    for c in &d.calques {
        // `write_psd` panique sur un calque de taille nulle.
        if !cote_valide(c.largeur, c.hauteur) || c.rgba.len() != (c.largeur * c.hauteur * 4) as usize {
            return Err(format!("calque « {} » incohérent", c.nom));
        }
        let mut l = Layer::default();
        l.additional_info.name = Some(c.nom.clone());
        l.left = Some(f64::from(c.x));
        l.top = Some(f64::from(c.y));
        l.right = Some(f64::from(c.x) + f64::from(c.largeur));
        l.bottom = Some(f64::from(c.y) + f64::from(c.hauteur));
        l.blend_mode = Some(mode_vers_psd(c.mode));
        l.opacity = Some(f64::from(c.opacite.clamp(0.0, 1.0)));
        l.hidden = Some(!c.visible);
        l.canvas = Some(PixelData { width: c.largeur, height: c.hauteur, data: c.rgba.clone() });
        enfants.push(l);
    }
    let psd = Psd {
        width: f64::from(d.largeur),
        height: f64::from(d.hauteur),
        children: Some(enfants),
        canvas: Some(PixelData { width: d.largeur, height: d.hauteur, data: envoi.composite.clone() }),
        ..Psd::default()
    };
    // `no_background` : le calque du bas reste un calque, avec son alpha.
    let options = WriteOptions { no_background: Some(true), compress: Some(true), ..WriteOptions::default() };
    Ok(ag_psd::write_psd(&psd, &options))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::win32::document_image::tests::exemple;

    #[test]
    fn ecrire_puis_lire_rend_le_document() {
        let doc = exemple();
        let composite = vec![9u8; (doc.largeur * doc.hauteur * 4) as usize];
        let octets = ecrire(&Envoi { document: doc.clone(), composite }).unwrap();
        let relu = lire(&octets).unwrap();
        assert_eq!((relu.largeur, relu.hauteur), (doc.largeur, doc.hauteur));
        assert_eq!(relu.calques.len(), doc.calques.len());
        for (a, b) in relu.calques.iter().zip(&doc.calques) {
            assert_eq!((&a.nom, a.x, a.y, a.largeur, a.hauteur, a.visible, a.mode), (&b.nom, b.x, b.y, b.largeur, b.hauteur, b.visible, b.mode));
            // Le PSD range l'opacité sur un octet.
            assert!((a.opacite - b.opacite).abs() < 1.0 / 255.0, "{} : {} ≠ {}", a.nom, a.opacite, b.opacite);
            assert_eq!(a.rgba, b.rgba, "pixels de « {} »", a.nom);
        }
    }

    #[test]
    fn un_fichier_quelconque_est_une_erreur() {
        assert!(lire(b"pas un PSD").is_err());
    }

    #[test]
    fn chaque_mode_revient_tel_quel() {
        for m in Mode::TOUS {
            assert_eq!(mode_depuis_psd(Some(mode_vers_psd(m))), m);
        }
    }
}
