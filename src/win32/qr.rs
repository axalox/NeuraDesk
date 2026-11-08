//! ⭐ LES QR CODES DU PRESSE-PAPIERS — DEMANDE EXPLICITE (2026-10-24) :
//! « générateur de mots de passe et de QR codes ». Un texte de l'historique
//! s'affiche en QR code pour être lu par un téléphone (un lien, une adresse…).
//!
//! Le codage est celui de la bibliothèque `qrcode` (MIT/Apache-2.0), en SVG :
//! la page l'insère tel quel, il reste net à toute taille.
//!
//! ⚠️ **NOIR SUR BLANC, EN DUR, ET C'EST VOULU** — comme le chrome du sélecteur
//! de couleur des Réglages : ce sont les couleurs que les lecteurs attendent,
//! pas un rôle d'interface. Un QR teinté par le thème (clair sur sombre) n'est
//! pas lu par tous les téléphones.

use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};

/// Au-delà, le QR code devient trop dense pour une caméra de téléphone à
/// l'écran — et la bibliothèque refuserait de toute façon vers 2 900 octets.
pub const MAX_OCTETS: usize = 1200;

/// Le QR code de `texte`, en SVG — ou l'erreur à montrer.
pub fn svg(texte: &str) -> Result<String, String> {
    if texte.is_empty() {
        return Err("rien à coder".into());
    }
    if texte.len() > MAX_OCTETS {
        return Err(format!("texte trop long pour un QR code lisible ({} octets, {MAX_OCTETS} au plus)", texte.len()));
    }
    let code = QrCode::with_error_correction_level(texte.as_bytes(), EcLevel::M).map_err(|e| format!("QR code impossible : {e}"))?;
    Ok(code
        .render::<svg::Color<'_>>()
        .min_dimensions(256, 256)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .quiet_zone(true)
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_lien_donne_un_svg_et_les_extremes_sont_refuses() {
        let image = svg("https://example.org/").expect("QR code");
        assert!(image.contains("<svg") && image.contains("#000000"));
        assert!(svg("").is_err());
        assert!(svg(&"a".repeat(MAX_OCTETS + 1)).is_err());
    }
}
