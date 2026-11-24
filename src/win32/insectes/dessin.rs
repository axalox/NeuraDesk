//! **LE DESSIN DES INSECTES** — DEMANDE EXPLICITE (2026-11-24) : « les mouches,
//! moustiques et papillons doivent être réalistes ». Des photos vues de dessus
//! (`assets/popups/mascottes/insecte-*.webp`, fabriquées par
//! `tools/insectes_realistes.py`), tête en HAUT, corps au CENTRE, tout le
//! contenu dans le cercle inscrit : elles tournent sans être coupées.
//!
//! À chaque image : rotation selon le cap (échantillonnage bilinéaire), ombre
//! portée tirée de la silhouette ; en vol, les ailes translucides de la mouche
//! et du moustique scintillent, celles du papillon battent (la moitié gauche et
//! la moitié droite écrasées vers le corps, comme vues de dessus).

use std::path::Path;
use std::sync::OnceLock;

use super::Espece;

/// Le moment de l'animation à dessiner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moment {
    /// Le cap, en radians, dans le repère de l'écran (`y` vers le bas).
    pub cap: f32,
    /// En vol (ailes battantes) ou posé.
    pub en_vol: bool,
    /// Phase du battement, de 0 à 1.
    pub battement: f32,
}

/// Paon du jour, vulcain, machaon, citron, azuré.

/// Le côté du carré de la fenêtre, en pixels logiques : la photo et, autour,
/// la place de l'ombre portée en vol.
pub fn cote_logique(espece: Espece) -> f32 {
    cote_photo(espece) + 2.0 * MARGE_OMBRE
}

/// La marge de l'ombre : au moins son décalage en vol.
const MARGE_OMBRE: f32 = 12.0;

/// Le côté de la photo à l'écran, en pixels logiques. Le corps occupe une
/// part fixe de ce côté (sortie de `tools/insectes_realistes.py` : mouche
/// 0,47, moustique 0,51) : mouches ~25 px de long, moustiques ~25 px,
/// papillon ~95 px d'envergure.
fn cote_photo(espece: Espece) -> f32 {
    match espece {
        Espece::Mouche(_) => 52.0,
        Espece::Moustique(_) => 48.0,
        Espece::Papillon(_) => 100.0,
    }
}

/// La demi-largeur du corps du papillon, en fraction du côté : ce qui ne
/// s'écrase pas quand les ailes battent.
const CORPS_PAPILLON: f32 = 0.025;
/// Sous cette opacité, un pixel de mouche ou de moustique est de l'aile.
const ALPHA_AILE: f32 = 0.8;

/// Une photo : RGBA prémultiplié, flottants de 0 à 1, carrée.
pub struct Image {
    cote: u32,
    px: Vec<[f32; 4]>,
}

impl Image {
    fn lire(chemin: &Path) -> Option<Image> {
        let octets = std::fs::read(chemin).map_err(|e| crate::diag!("[insectes] {} : {e}", chemin.display())).ok()?;
        let im = image::load_from_memory(&octets)
            .map_err(|e| crate::diag!("[insectes] {} : {e}", chemin.display()))
            .ok()?
            .to_rgba8();
        let (l, h) = im.dimensions();
        if l != h {
            crate::diag!("[insectes] {} n'est pas carrée ({l}×{h})", chemin.display());
            return None;
        }
        let px = im
            .pixels()
            .map(|p| {
                let a = p.0[3] as f32 / 255.0;
                [p.0[0] as f32 / 255.0 * a, p.0[1] as f32 / 255.0 * a, p.0[2] as f32 / 255.0 * a, a]
            })
            .collect();
        Some(Image { cote: l, px })
    }

    /// Bilinéaire, transparent hors de l'image.
    fn lire_en(&self, x: f32, y: f32) -> [f32; 4] {
        let (x, y) = (x - 0.5, y - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let c = self.cote as i32;
        let au = |xi: i32, yi: i32| -> [f32; 4] {
            if xi < 0 || yi < 0 || xi >= c || yi >= c {
                [0.0; 4]
            } else {
                self.px[(yi * c + xi) as usize]
            }
        };
        let (xi, yi) = (x0 as i32, y0 as i32);
        let (a, b, cc, d) = (au(xi, yi), au(xi + 1, yi), au(xi, yi + 1), au(xi + 1, yi + 1));
        let mut r = [0.0; 4];
        for k in 0..4 {
            r[k] = (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (cc[k] * (1.0 - fx) + d[k] * fx) * fy;
        }
        r
    }
}

/// Toutes les photos d'insectes.
pub struct Photos {
    /// Par espèce, dans l'ordre de `ESPECES` : posée puis en vol (le
    /// papillon : une seule photo, ses ailes battent par le dessin).
    images: Vec<[Image; 2]>,
}

impl Photos {
    pub fn charger(dossier: &Path) -> Option<Photos> {
        let lire = |nom: &str| Image::lire(&dossier.join(format!("insecte-{nom}.webp")));
        let images: Option<Vec<[Image; 2]>> = super::ESPECES
            .iter()
            .map(|e| {
                if e.est_papillon() {
                    Some([lire(e.nom())?, lire(e.nom())?])
                } else {
                    Some([lire(&format!("{}-pose", e.nom()))?, lire(&format!("{}-vol", e.nom()))?])
                }
            })
            .collect();
        Some(Photos { images: images? })
    }

    fn image(&self, espece: Espece, m: &Moment) -> &Image {
        &self.images[espece.rang()][usize::from(m.en_vol && !espece.est_papillon())]
    }
}

/// Les photos livrées à côté de l'exe, chargées une fois (`None` : absentes).
pub fn photos() -> Option<&'static Photos> {
    static PHOTOS: OnceLock<Option<Photos>> = OnceLock::new();
    PHOTOS
        .get_or_init(|| {
            let dossier = std::env::current_exe().ok()?.parent()?.join("assets/popups/mascottes");
            Photos::charger(&dossier)
        })
        .as_ref()
}

/// Le décalage de l'ombre portée (pixels logiques) et son opacité : petite et
/// nette posée, plus large et plus pâle en vol.
fn ombre(en_vol: bool) -> ((f32, f32), f32) {
    if en_vol {
        ((6.0, 9.0), 0.18)
    } else {
        ((1.2, 1.8), 0.35)
    }
}

/// L'ouverture des ailes du papillon vues de dessus : 1 grandes ouvertes,
/// proche de 0 de profil.
fn ouverture(m: &Moment) -> f32 {
    let onde = 0.5 + 0.5 * (m.battement * std::f32::consts::TAU).cos();
    if m.en_vol {
        0.1 + 0.9 * onde
    } else {
        0.55 + 0.45 * onde
    }
}

/// Dessine l'insecte au centre d'un carré de `cote` pixels, `echelle` pixels
/// par pixel logique : BGRA prémultiplié. Le carré se cale sur `cote_logique`.
pub fn dessiner(photos: &Photos, espece: Espece, m: &Moment, echelle: f32, cote: u32) -> Vec<u8> {
    let img = photos.image(espece, m);
    let s = img.cote as f32;
    // Pixels de la photo par pixel d'écran.
    let k = s / (cote_photo(espece) * echelle);
    let centre = cote as f32 / 2.0;
    let (sin, cos) = m.cap.sin_cos();
    let papillon = espece.est_papillon();
    let ouvre = ouverture(m).max(0.05);
    let corps = CORPS_PAPILLON * s;
    // Les ailes translucides scintillent en vol.
    let scintille = if m.en_vol && !papillon {
        0.45 + 0.55 * (m.battement * std::f32::consts::TAU).sin().abs()
    } else {
        1.0
    };
    let ((ox, oy), alpha_ombre) = ombre(m.en_vol);
    let (ox, oy) = (ox * echelle, oy * echelle);
    // Le pixel de la photo vu au point d'écran (dx, dy), relatif au centre.
    let lire = |dx: f32, dy: f32| -> [f32; 4] {
        let avant = dx * cos + dy * sin;
        let mut droite = (-dx * sin + dy * cos) * k;
        if papillon && droite.abs() > corps {
            droite = droite.signum() * (corps + (droite.abs() - corps) / ouvre);
        }
        let mut p = img.lire_en(s / 2.0 + droite, s / 2.0 - avant * k);
        if scintille < 1.0 && p[3] < ALPHA_AILE {
            for c in &mut p {
                *c *= scintille;
            }
        }
        p
    };
    let mut sortie = Vec::with_capacity((cote * cote * 4) as usize);
    for y in 0..cote {
        for x in 0..cote {
            let (dx, dy) = (x as f32 + 0.5 - centre, y as f32 + 0.5 - centre);
            let p = lire(dx, dy);
            let o = lire(dx - ox, dy - oy)[3] * alpha_ombre;
            // L'insecte par-dessus son ombre (noire : rien à ajouter aux couleurs).
            let a = p[3] + o * (1.0 - p[3]);
            let octet = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            let a8 = octet(a);
            sortie.extend_from_slice(&[octet(p[2]).min(a8), octet(p[1]).min(a8), octet(p[0]).min(a8), a8]);
        }
    }
    sortie
}

#[cfg(test)]
mod tests {
    use super::*;

    fn livrees() -> Photos {
        let dossier = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/popups/mascottes");
        Photos::charger(&dossier).expect("photos d'insectes livrées")
    }

    #[test]
    fn chaque_insecte_tient_dans_son_carre_sous_tous_les_caps() {
        let photos = livrees();
        for espece in super::super::ESPECES {
            let echelle = 1.5;
            let cote = (cote_logique(espece) * echelle).ceil() as u32;
            for en_vol in [false, true] {
                for n in 0..12 {
                    let m = Moment { cap: n as f32 * 0.55, en_vol, battement: 0.0 };
                    let px = dessiner(&photos, espece, &m, echelle, cote);
                    assert_eq!(px.len(), (cote * cote * 4) as usize);
                    let centre = ((cote / 2 * cote + cote / 2) * 4 + 3) as usize;
                    assert!(px[centre] > 200, "{espece:?} : le corps est au centre ({})", px[centre]);
                    // Le bord du carré reste presque vide : rien n'est coupé.
                    let mut au_bord = 0u32;
                    for i in 0..cote {
                        for (x, y) in [(i, 0), (i, cote - 1), (0, i), (cote - 1, i)] {
                            au_bord = au_bord.max(px[((y * cote + x) * 4 + 3) as usize] as u32);
                        }
                    }
                    assert!(au_bord < 40, "{espece:?} coupé au bord ({au_bord}), cap {}, vol {en_vol}", m.cap);
                }
            }
        }
    }

    /// Agrandi (taille réglée × échelle d'écran), en vol, à chaque battement :
    /// toujours rien de coupé au bord de la fenêtre.
    #[test]
    fn un_papillon_agrandi_n_est_jamais_coupe() {
        let photos = livrees();
        for echelle in [1.0f32, 3.0, 6.0] {
            for n in 0..super::super::PAR_FAMILLE as u8 {
                let espece = Espece::Papillon(n);
                let cote = (cote_logique(espece) * echelle).ceil() as u32;
                for b in 0..8 {
                    for cap in [0.0f32, 0.8, 1.6, 2.4] {
                        let m = Moment { cap, en_vol: true, battement: b as f32 / 8.0 };
                        let px = dessiner(&photos, espece, &m, echelle, cote);
                        let mut au_bord = 0u32;
                        for i in 0..cote {
                            for (x, y) in [(i, 0), (i, cote - 1), (0, i), (cote - 1, i)] {
                                au_bord = au_bord.max(px[((y * cote + x) * 4 + 3) as usize] as u32);
                            }
                        }
                        assert!(au_bord < 40, "papillon {n} ×{echelle} battement {b} cap {cap} : {au_bord}");
                    }
                }
            }
        }
    }

    #[test]
    fn les_pixels_sont_premultiplies() {
        let photos = livrees();
        let m = Moment { cap: 0.7, en_vol: true, battement: 0.3 };
        let px = dessiner(&photos, Espece::Papillon(1), &m, 1.0, 124);
        assert!(px.chunks_exact(4).all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]));
        assert!(px.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255), "un bord lissé");
    }

    #[test]
    fn le_papillon_ferme_ses_ailes_en_battant() {
        let photos = livrees();
        let couvert = |battement: f32| {
            let m = Moment { cap: 0.0, en_vol: true, battement };
            dessiner(&photos, Espece::Papillon(0), &m, 1.0, 124).chunks_exact(4).filter(|p| p[3] > 160).count()
        };
        assert!(couvert(0.0) > couvert(0.5) * 3, "grandes ouvertes ≫ de profil");
    }

    #[test]
    fn la_photo_tourne_avec_le_cap() {
        // Cap 0 = vers la droite de l'écran : la tête (le haut de la photo) à droite.
        let photos = livrees();
        let cote = 76;
        let masse_x = |cap: f32| {
            let m = Moment { cap, en_vol: false, battement: 0.0 };
            // La mouche bleue : ses yeux rouges sont le seul rouge franc (le
            // thorax brun de la domestique passerait le seuil).
            let px = dessiner(&photos, Espece::Mouche(2), &m, 1.0, cote);
            let (mut s, mut n) = (0.0f32, 0.0f32);
            for (i, p) in px.chunks_exact(4).enumerate() {
                if p[3] > 200 && p[2] as i32 > p[1] as i32 + 60 && p[2] as i32 > p[0] as i32 + 60 {
                    s += (i as u32 % cote) as f32;
                    n += 1.0;
                }
            }
            s / n.max(1.0)
        };
        assert!(masse_x(0.0) > cote as f32 / 2.0 + 4.0, "tête à droite : {}", masse_x(0.0));
        assert!(masse_x(std::f32::consts::PI) < cote as f32 / 2.0 - 4.0, "tête à gauche");
    }
}
