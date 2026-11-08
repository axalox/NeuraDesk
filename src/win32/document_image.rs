//! **LE DOCUMENT À CALQUES** de l'éditeur d'image — ce que le PSD
//! (`win32::format_psd`), le XCF (`win32::format_xcf`) et la page
//! (`assets/popups/editeur_image.html`) ont en commun.
//!
//! DEMANDE EXPLICITE (2026-09-25) : « je voudrais pouvoir gérer les calques
//! comme dans GIMP, et oui il faut pouvoir rouvrir les fichiers ». Le modèle est
//! donc celui de GIMP réduit à ce que la page sait dessiner : une pile de
//! calques RGBA 8 bits, chacun avec son nom, sa position, son opacité, sa
//! visibilité et son mode de fusion. Les groupes, masques et précisions
//! supérieures d'un fichier LU sont aplatis dans ce modèle (voir chaque format).
//!
//! **Les deux sens avec la page** :
//! - page → Rust ([`depuis_page`]) : `[u32 LE : longueur du JSON][JSON][PNG…]`,
//!   déposé sur le canal des grosses données — un PNG par calque, puis celui de
//!   l'image composée (le PSD l'embarque, la page seule sait fusionner) ;
//! - Rust → page ([`vers_page`]) : un JSON dont chaque calque porte son PNG en
//!   `data:` URI, comme une image simple (`cef_image_editor::charger_octets`).

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::{json, Value};

/// Au-delà, Chromium refuse le canevas — la même borne que la page (`COTE_MAX`
/// de `editeur_geometrie.js`).
pub const COTE_MAX: u32 = 16384;
/// Tous calques confondus : un fichier plus gros est REFUSÉ plutôt que de
/// réclamer des gigaoctets sur la foi de son en-tête (1 Gio de RGBA).
pub const PIXELS_MAX: u64 = 256 * 1024 * 1024;

/// Les modes de fusion que la page sait dessiner — ceux de
/// `globalCompositeOperation`, dont le nom sert d'identifiant partout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl Mode {
    pub const TOUS: [Mode; 16] = [
        Mode::Normal,
        Mode::Multiply,
        Mode::Screen,
        Mode::Overlay,
        Mode::Darken,
        Mode::Lighten,
        Mode::ColorDodge,
        Mode::ColorBurn,
        Mode::HardLight,
        Mode::SoftLight,
        Mode::Difference,
        Mode::Exclusion,
        Mode::Hue,
        Mode::Saturation,
        Mode::Color,
        Mode::Luminosity,
    ];

    /// Le nom de `globalCompositeOperation` — sauf `normal`, qui s'y appelle
    /// `source-over` (la page fait la traduction).
    pub fn nom(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::Multiply => "multiply",
            Mode::Screen => "screen",
            Mode::Overlay => "overlay",
            Mode::Darken => "darken",
            Mode::Lighten => "lighten",
            Mode::ColorDodge => "color-dodge",
            Mode::ColorBurn => "color-burn",
            Mode::HardLight => "hard-light",
            Mode::SoftLight => "soft-light",
            Mode::Difference => "difference",
            Mode::Exclusion => "exclusion",
            Mode::Hue => "hue",
            Mode::Saturation => "saturation",
            Mode::Color => "color",
            Mode::Luminosity => "luminosity",
        }
    }

    pub fn depuis_nom(nom: &str) -> Option<Mode> {
        Mode::TOUS.into_iter().find(|m| m.nom() == nom)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Calque {
    pub nom: String,
    /// Le coin haut-gauche dans l'image — négatif ou au-delà : un calque peut
    /// déborder du canevas, comme dans GIMP.
    pub x: i32,
    pub y: i32,
    pub largeur: u32,
    pub hauteur: u32,
    /// RGBA 8 bits NON prémultiplié, `largeur × hauteur × 4` octets.
    pub rgba: Vec<u8>,
    /// De 0 à 1.
    pub opacite: f32,
    pub visible: bool,
    pub mode: Mode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub largeur: u32,
    pub hauteur: u32,
    /// Du BAS vers le HAUT — l'ordre de dessin, celui du PSD et de la page.
    /// Le XCF range à l'envers (voir `format_xcf`).
    pub calques: Vec<Calque>,
}

/// Une taille de calque ou d'image acceptable.
pub fn cote_valide(largeur: u32, hauteur: u32) -> bool {
    (1..=COTE_MAX).contains(&largeur) && (1..=COTE_MAX).contains(&hauteur)
}

/// Tient la comptabilité des pixels d'un fichier en cours de lecture.
pub struct Budget(u64);

impl Budget {
    pub fn nouveau() -> Self {
        Budget(0)
    }

    pub fn prendre(&mut self, largeur: u32, hauteur: u32) -> Result<(), String> {
        self.0 += u64::from(largeur) * u64::from(hauteur);
        if self.0 > PIXELS_MAX {
            return Err(format!("document trop grand (plus de {PIXELS_MAX} pixels)"));
        }
        Ok(())
    }
}

/// Ce que la page envoie pour un enregistrement : le document, et son image
/// composée (RGBA, à la taille de l'image).
pub struct Envoi {
    pub document: Document,
    pub composite: Vec<u8>,
}

/// Lit ce que la page a déposé sur le canal (voir la doc du module).
pub fn depuis_page(octets: &[u8]) -> Result<Envoi, String> {
    let longueur = octets.get(..4).ok_or("envoi tronqué")?;
    let longueur = u32::from_le_bytes(longueur.try_into().unwrap()) as usize;
    let json = octets.get(4..4 + longueur).ok_or("JSON tronqué")?;
    let entete: Value = serde_json::from_slice(json).map_err(|e| format!("JSON illisible : {e}"))?;
    let mut reste = &octets[4 + longueur..];
    let mut png_suivant = |taille: usize| -> Result<(u32, u32, Vec<u8>), String> {
        let png = reste.get(..taille).ok_or("PNG tronqué")?;
        reste = &reste[taille..];
        crate::win32::clipboard::decode_png(png).ok_or_else(|| "PNG illisible".to_string())
    };
    let entier = |v: &Value, cle: &str| v[cle].as_i64().ok_or_else(|| format!("champ {cle} manquant"));
    let largeur = u32::try_from(entier(&entete, "largeur")?).map_err(|_| "largeur invalide")?;
    let hauteur = u32::try_from(entier(&entete, "hauteur")?).map_err(|_| "hauteur invalide")?;
    if !cote_valide(largeur, hauteur) {
        return Err(format!("taille d'image invalide : {largeur}×{hauteur}"));
    }
    let mut calques = Vec::new();
    for c in entete["calques"].as_array().ok_or("calques manquants")? {
        let (l, h, rgba) = png_suivant(entier(c, "taille")? as usize)?;
        let position = |cle| i32::try_from(entier(c, cle)?).map_err(|_| format!("{cle} invalide"));
        calques.push(Calque {
            nom: c["nom"].as_str().unwrap_or("").to_string(),
            x: position("x")?,
            y: position("y")?,
            largeur: l,
            hauteur: h,
            rgba,
            opacite: c["opacite"].as_f64().unwrap_or(1.0).clamp(0.0, 1.0) as f32,
            visible: c["visible"].as_bool().unwrap_or(true),
            mode: c["mode"].as_str().and_then(Mode::depuis_nom).unwrap_or(Mode::Normal),
        });
    }
    if calques.is_empty() {
        return Err("aucun calque".into());
    }
    let (l, h, composite) = png_suivant(entier(&entete, "composite")? as usize)?;
    if (l, h) != (largeur, hauteur) {
        return Err(format!("image composée {l}×{h} pour une image {largeur}×{hauteur}"));
    }
    Ok(Envoi { document: Document { largeur, hauteur, calques }, composite })
}

/// Le JSON que la page reçoit (`window.editeurChargerDocument`).
pub fn vers_page(document: &Document) -> Result<Value, String> {
    let calques = document
        .calques
        .iter()
        .map(|c| {
            let png = crate::win32::clipboard::encode_png(c.largeur, c.hauteur, &c.rgba)
                .ok_or_else(|| format!("calque « {} » : encodage PNG échoué", c.nom))?;
            Ok(json!({
                "nom": c.nom,
                "x": c.x,
                "y": c.y,
                "opacite": c.opacite,
                "visible": c.visible,
                "mode": c.mode.nom(),
                "png": format!("data:image/png;base64,{}", BASE64.encode(png)),
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(json!({ "largeur": document.largeur, "hauteur": document.hauteur, "calques": calques }))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Un document d'essai : un fond opaque, un calque décalé qui déborde, un
    /// calque semi-transparent en « multiply », un calque caché.
    pub(crate) fn exemple() -> Document {
        let uni = |l: u32, h: u32, p: [u8; 4]| p.iter().copied().cycle().take((l * h * 4) as usize).collect::<Vec<u8>>();
        let mut degrade = Vec::new();
        for y in 0..37u32 {
            for x in 0..53u32 {
                degrade.extend_from_slice(&[(x * 4) as u8, (y * 6) as u8, ((x + y) * 2) as u8, 255 - (x as u8)]);
            }
        }
        Document {
            largeur: 120,
            hauteur: 80,
            calques: vec![
                Calque { nom: "Fond".into(), x: 0, y: 0, largeur: 120, hauteur: 80, rgba: uni(120, 80, [250, 250, 250, 255]), opacite: 1.0, visible: true, mode: Mode::Normal },
                Calque { nom: "Dégradé décalé".into(), x: -10, y: 50, largeur: 53, hauteur: 37, rgba: degrade, opacite: 1.0, visible: true, mode: Mode::Normal },
                Calque { nom: "Bleu".into(), x: 30, y: 20, largeur: 70, hauteur: 40, rgba: uni(70, 40, [0, 0, 255, 128]), opacite: 0.5, visible: true, mode: Mode::Multiply },
                Calque { nom: "Caché".into(), x: 5, y: 5, largeur: 64, hauteur: 65, rgba: uni(64, 65, [0, 200, 0, 255]), opacite: 1.0, visible: false, mode: Mode::Screen },
            ],
        }
    }

    #[test]
    fn chaque_mode_a_un_nom_unique() {
        for m in Mode::TOUS {
            assert_eq!(Mode::depuis_nom(m.nom()), Some(m));
        }
        assert_eq!(Mode::depuis_nom("source-over"), None);
    }

    /// Ce que la page enverrait, reconstruit ici : la lecture rend le document.
    #[test]
    fn l_envoi_de_la_page_se_relit() {
        let doc = exemple();
        let mut pngs = Vec::new();
        let mut calques = Vec::new();
        for c in &doc.calques {
            let png = crate::win32::clipboard::encode_png(c.largeur, c.hauteur, &c.rgba).unwrap();
            calques.push(json!({ "nom": c.nom, "x": c.x, "y": c.y, "opacite": c.opacite, "visible": c.visible, "mode": c.mode.nom(), "taille": png.len() }));
            pngs.extend(png);
        }
        let composite = vec![7u8; (doc.largeur * doc.hauteur * 4) as usize];
        let png = crate::win32::clipboard::encode_png(doc.largeur, doc.hauteur, &composite).unwrap();
        let entete = json!({ "largeur": doc.largeur, "hauteur": doc.hauteur, "calques": calques, "composite": png.len() }).to_string();
        let mut octets = (entete.len() as u32).to_le_bytes().to_vec();
        octets.extend(entete.as_bytes());
        octets.extend(pngs);
        octets.extend(png);
        let envoi = depuis_page(&octets).unwrap();
        assert_eq!(envoi.document, doc);
        assert_eq!(envoi.composite, composite);
        assert!(depuis_page(&octets[..octets.len() - 1]).is_err());
    }
}
