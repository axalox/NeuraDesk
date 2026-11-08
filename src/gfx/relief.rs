//! **LE RELIEF DESSINÉ — les bandes « aluminium brossé » du style Rétro.**
//!
//! C'est la seconde moitié de l'axe relief de `ui::style` : là où `degrade` et
//! `liserel` demandent au SHADER de peindre un biseau, `relief.image` demande de
//! poser des IMAGES. Les deux ne se cumulent pas — un style qui porte une image
//! met ses deux nombres à zéro (voir `assets/styles/retro.toml`).
//!
//! ⚠️ **CE SONT DES SURCOUCHES, PAS DES FONDS.** Le fond d'une barre est le
//! **bureau flouté teinté** (`gfx::backdrop`, une fonctionnalité et un réglage) :
//! une image ne peut donc pas le remplacer, elle se pose DESSUS — et elle est
//! **neutre** (gris + alpha), pour se composer sur n'importe quelle teinte.
//!
//! ⚠️ **DEUX BANDES, PAS UNE IMAGE DE BARRE.** Les quatre barres n'ont pas la même
//! géométrie (72 px de haut sur toute la largeur en haut et en bas, ~100 px de
//! large sur 2000 de haut sur les côtés) : un seul dessin ne peut pas les servir.
//! On ne peint donc que le **haut** et le **bas** de chaque barre, à leur taille
//! NATURELLE, et le milieu reste le fond flouté.
//!
//! ⚠️ **ET C'EST POUR ÇA QU'UNE SEULE ORIENTATION DE GRAIN SUFFIT** : les bandes
//! ne sont jamais étirées verticalement, seulement horizontalement — et un grain
//! constant le long de X y survit, pour les quatre barres. J'ai failli dessiner
//! quatre fichiers pour rien.

use windows::core::Result;
use windows::Win32::Graphics::Direct3D11::ID3D11Device;

use crate::gfx::texture::Texture;
use crate::ui::style::ReliefImage;

/// Les deux bandes, embarquées : un style INTÉGRÉ n'a aucun fichier à charger, et
/// rien à valider — c'est ce qui permet à une image d'entrer dans un système dont
/// la règle est « un style est un TOML de VALEURS ». Le jour où des styles
/// utilisateur porteront des images, ce sera un autre travail (un chemin à
/// valider, un dossier à définir).
const BANDE_HAUT: &[u8] = include_bytes!("../../assets/styles/retro/barre-haut.png");
const BANDE_BAS: &[u8] = include_bytes!("../../assets/styles/retro/barre-bas.png");

/// Hauteur de chaque bande, EN PIXELS — celle du fichier, et elle est VÉRIFIÉE par
/// `les_hauteurs_correspondent_aux_fichiers` : une constante qui dérive de son PNG
/// se verrait (la bande serait rognée ou laisserait un vide), mais seulement à
/// l'œil, et seulement si on regarde cet endroit-là.
const HAUTEUR_BANDE_HAUT: f32 = 34.0;
const HAUTEUR_BANDE_BAS: f32 = 30.0;

/// Les bandes de relief du style actif.
pub struct JeuRelief {
    pub haut: Texture,
    pub bas: Texture,
}

impl JeuRelief {
    /// Les bandes du style ACTIF — `None` quand il n'en demande pas (les deux
    /// autres styles), auquel cas rien n'est dessiné et rien n'est chargé.
    pub fn charger(device: &ID3D11Device) -> Option<Self> {
        let (haut, bas) = match crate::ui::style::relief_image() {
            ReliefImage::Aucune => return None,
            ReliefImage::Aluminium => (BANDE_HAUT, BANDE_BAS),
        };
        Some(Self {
            haut: Texture::from_png_bytes(device, haut).ok()?,
            bas: Texture::from_png_bytes(device, bas).ok()?,
        })
    }

    /// **LES DEUX RECTANGLES D'UNE BARRE** — le haut et le bas, chacun à sa taille
    /// naturelle, ancrés aux deux bords. Le milieu n'est PAS rendu : c'est là que
    /// le fond flouté passe.
    ///
    /// ⚠️ Une barre plus fine que la somme des deux bandes (moins de 64 px) verrait
    /// les deux se chevaucher. Elles sont donc BORNÉES à la moitié de l'épaisseur
    /// chacune, plutôt que de laisser un recouvrement dépendre de la géométrie.
    pub fn bandes(&self, barre: [f32; 4]) -> [[f32; 4]; 2] {
        let [x, y, w, h] = barre;
        let haut = HAUTEUR_BANDE_HAUT.min(h * 0.5);
        let bas = HAUTEUR_BANDE_BAS.min(h * 0.5);
        [[x, y, w, haut], [x, y + h - bas, w, bas]]
    }
}

/// Largeur et hauteur lues dans l'en-tête d'un PNG — les huit octets de signature,
/// puis le bloc `IHDR`, dont les deux premiers champs sont la largeur et la hauteur
/// en gros-boutiste. Rien à décoder : c'est un contrôle de CONSTANTES.
#[cfg(test)]
fn taille_png(png: &[u8]) -> (u32, u32) {
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "ce n'est pas un PNG");
    assert_eq!(&png[12..16], b"IHDR", "le premier bloc doit être IHDR");
    let lire = |o: usize| u32::from_be_bytes([png[o], png[o + 1], png[o + 2], png[o + 3]]);
    (lire(16), lire(20))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⛔ **LES CONSTANTES DE HAUTEUR DOIVENT DIRE LA VÉRITÉ.** Elles décident de la
    /// place que chaque bande occupe sur la barre : trop grandes, elles se
    /// chevauchent ; trop petites, elles laissent un vide. Les deux se voient à
    /// l'œil — mais seulement si on regarde cet endroit-là, au bon moment.
    ///
    /// Et elles DOIVENT rester d'accord avec `tools/rasteriser_aluminium.py`, qui
    /// découpe les fichiers : c'est le seul lien entre les deux, et rien d'autre ne
    /// le vérifie.
    #[test]
    fn les_hauteurs_correspondent_aux_fichiers() {
        let (lw, lh) = taille_png(BANDE_HAUT);
        let (bw, bh) = taille_png(BANDE_BAS);
        assert_eq!(lh as f32, HAUTEUR_BANDE_HAUT, "la bande du haut : {lw}x{lh}");
        assert_eq!(bh as f32, HAUTEUR_BANDE_BAS, "la bande du bas : {bw}x{bh}");
        // ⚠️ **UNE BANDE EST PLUS LARGE QUE HAUTE**, et ce n'est pas un détail de
        // forme : c'est ce qui dit qu'elle est ÉTIRÉE HORIZONTALEMENT et jamais
        // verticalement — donc qu'un seul sens de grain vaut pour les quatre
        // barres (voir la doc de module). L'assertion inverse avait été écrite
        // d'abord, et c'est ce test qui l'a relevée.
        assert!(lw > lh && bw > bh, "ces bandes sont des TRAITS HORIZONTAUX : {lw}x{lh} et {bw}x{bh}");
    }

    /// ⚠️ **LES DEUX BANDES NE DOIVENT PAS REMPLIR LA BARRE** : le milieu reste le
    /// fond flouté teinté, qui est une FONCTIONNALITÉ et un réglage (voir
    /// `gfx::backdrop`). Deux bandes qui se rejoignent la masqueraient entièrement —
    /// et l'utilisateur perdrait sa teinte sans qu'aucun message ne le dise.
    ///
    /// L'épaisseur par défaut est de 72 px ; c'est celle qu'on vérifie, parce que
    /// c'est celle que tout le monde a.
    #[test]
    fn les_bandes_laissent_le_milieu_au_fond_floute() {
        let epaisseur = 72.0;
        let occupe = HAUTEUR_BANDE_HAUT.min(epaisseur * 0.5) + HAUTEUR_BANDE_BAS.min(epaisseur * 0.5);
        assert!(
            occupe < epaisseur - 4.0,
            "les deux bandes doivent laisser un milieu visible : {occupe} px occupés sur {epaisseur}"
        );
    }
}
