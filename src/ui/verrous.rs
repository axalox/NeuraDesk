//! ⭐ **VERR. NUM ET VERR. MAJ DANS LA BARRE DU HAUT** — DEMANDE EXPLICITE
//! (2026-10-14) : « une applet dans la barre du haut qui affiche l'état des
//! touches Ver num et caps lock pour savoir quand elles sont pressées, un
//! arrière plan de couleur pour dire qu'elles sont activées, une couleur par
//! touche, paramétrable dans les réglages ».
//!
//! Deux pastilles, « NUM » puis « MAJ », posées à GAUCHE de la fréquence
//! d'affichage (ou de ce qui la remplace quand elle est masquée — même
//! chaînage par `next_left_edge` que le reste de la moitié droite, voir
//! `ui::topbar::refresh_rate_rect`). Une touche ACTIVE a un fond plein de SA
//! couleur ; inactive, la pastille n'est qu'un fond léger de la couleur du
//! texte.
//!
//! L'état est relu à CHAQUE frame par `GetKeyState` (bit de bascule) : deux
//! appels, pas de crochet clavier. Rien à cliquer, rien qui magnifie — le widget
//! reste DANS la barre, donc ni l'union des boîtes magnifiées ni le tableau
//! d'échelles de survol ne le concernent.
//!
//! Les réglages sont GLOBAUX (toutes les barres du haut de tous les écrans),
//! persistés dans `verrous.txt` (voir `config::load_verrous`).

use std::sync::Mutex;

use windows::Win32::Graphics::Direct3D11::ID3D11Device;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CAPITAL, VK_NUMLOCK};

use crate::dock_instance::CachedText;
use crate::gfx::QuadInstance;
use crate::gfx::texture::Texture;
use crate::ui::dock::DockLayout;
use crate::ui::topbar;

/// Les libellés, dans l'ordre de la barre (de gauche à droite).
pub const LIBELLES: [&str; 2] = ["NUM", "MAJ"];

/// Écart entre la pastille MAJ et l'élément qui la suit — celui de la
/// fréquence d'affichage envers le moniteur système.
const MARGE_DROITE: f32 = 16.0;
/// Écart entre les deux pastilles.
const ECART: f32 = 6.0;
/// Marge horizontale du texte dans sa pastille, en fraction de sa hauteur.
const MARGE_TEXTE: f32 = 0.45;
/// Hauteur de la pastille, en fraction de la hauteur du texte — plafonnée par
/// la barre (voir `rectangles`).
const HAUTEUR_PASTILLE: f32 = 1.2;
/// Opacité du fond d'une touche INACTIVE (couleur du texte de la barre).
const FOND_INACTIF_ALPHA: f32 = 0.12;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reglages {
    pub affiche: bool,
    /// `[Verr. num, Verr. maj]`.
    pub couleurs: [[f32; 3]; 2],
}

pub const REGLAGES_PAR_DEFAUT: Reglages = Reglages {
    affiche: true,
    couleurs: [[0.20, 0.70, 0.35], [0.95, 0.55, 0.15]],
};

static REGLAGES: Mutex<Reglages> = Mutex::new(REGLAGES_PAR_DEFAUT);

pub fn reglages() -> Reglages {
    REGLAGES.lock().map_or(REGLAGES_PAR_DEFAUT, |r| *r)
}

pub fn set_reglages(r: Reglages) {
    if let Ok(mut g) = REGLAGES.lock() {
        *g = r;
    }
}

/// `[Verr. num, Verr. maj]` — `true` si la touche est ACTIVÉE (bit de bascule de
/// `GetKeyState`, celui qui suit le voyant du clavier).
pub fn etat() -> [bool; 2] {
    let bascule = |vk: u16| unsafe { GetKeyState(vk as i32) as u16 & 1 != 0 };
    [bascule(VK_NUMLOCK.0), bascule(VK_CAPITAL.0)]
}

/// Les rectangles des deux pastilles, `[NUM, MAJ]` — PURE. `tailles` : celles des
/// textes rendus. MAJ finit à `MARGE_DROITE` de `next_left_edge`, NUM la précède.
pub fn rectangles(next_left_edge: f32, layout: &DockLayout, tailles: [(f32, f32); 2]) -> [[f32; 4]; 2] {
    let pastille = |(w, h): (f32, f32)| {
        let ph = (h * HAUTEUR_PASTILLE).min((layout.top_thickness - 4.0).max(h));
        (w + 2.0 * (h * MARGE_TEXTE).round(), ph)
    };
    let (w_maj, h_maj) = pastille(tailles[1]);
    let (w_num, h_num) = pastille(tailles[0]);
    let x_maj = next_left_edge - MARGE_DROITE - w_maj;
    let x_num = x_maj - ECART - w_num;
    let y = |h: f32| ((layout.top_thickness - h) * 0.5).round();
    [[x_num, y(h_num), w_num, h_num], [x_maj, y(h_maj), w_maj, h_maj]]
}

/// Le texte posé au CENTRE de sa pastille — PURE.
pub fn rectangle_du_texte(pastille: [f32; 4], (w, h): (f32, f32)) -> [f32; 4] {
    let [x, y, pw, ph] = pastille;
    [(x + (pw - w) * 0.5).round(), (y + (ph - h) * 0.5).round(), w, h]
}

/// Un texte LISIBLE sur `fond` : presque noir sur un fond clair, blanc sinon.
pub fn texte_sur(fond: [f32; 3]) -> [f32; 3] {
    let luminance = 0.2126 * fond[0] + 0.7152 * fond[1] + 0.0722 * fond[2];
    if luminance > 0.55 { [0.08, 0.08, 0.08] } else { [1.0, 1.0, 1.0] }
}

/// Les deux textes d'UNE barre, à SA taille de police (voir
/// `dock_instance::CachedText`, et la doc de `ui::desktop_labels` pour la
/// raison d'un cache par barre).
pub struct Textes {
    textes: [CachedText; 2],
}

impl Textes {
    pub fn new() -> Self {
        Self { textes: [CachedText::new(), CachedText::new()] }
    }

    /// À appeler à chaque frame : ne régénère qu'au changement de police.
    pub fn sync(&mut self, device: &ID3D11Device, layout: &DockLayout, taille_texte: Option<f32>) -> Result<(), Box<dyn std::error::Error>> {
        let px = topbar::refresh_rate_font_px(layout, taille_texte);
        for (cache, libelle) in self.textes.iter_mut().zip(LIBELLES) {
            cache.sync(device, libelle, px, topbar::REFRESH_RATE_FONT_NAME, true)?;
        }
        Ok(())
    }

    pub fn texture(&self, i: usize) -> Option<&Texture> {
        self.textes.get(i)?.texture().map(|(t, _, _)| t)
    }

    /// `[(fond, texte); 2]`, ou `None` si le widget est masqué ou que ses textes
    /// n'existent pas encore.
    pub fn instances(&self, next_left_edge: f32, layout: &DockLayout, couleur_texte: [f32; 3]) -> Option<[(QuadInstance, QuadInstance); 2]> {
        let r = reglages();
        if !r.affiche {
            return None;
        }
        let tailles = [self.textes[0].texture()?, self.textes[1].texture()?].map(|(_, w, h)| (w, h));
        let rects = rectangles(next_left_edge, layout, tailles);
        let actives = etat();
        Some(std::array::from_fn(|i| {
            let [_, _, _, ph] = rects[i];
            let rayon = ph * 0.3;
            let (fond, texte) = if actives[i] {
                let c = r.couleurs[i];
                ([c[0], c[1], c[2], 1.0], texte_sur(c))
            } else {
                let c = couleur_texte;
                ([c[0], c[1], c[2], FOND_INACTIF_ALPHA], couleur_texte)
            };
            (
                QuadInstance::new(rects[i], fond, rayon, 1.0),
                QuadInstance::new_glyph(rectangle_du_texte(rects[i], tailles[i]), texte),
            )
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(top: f32) -> DockLayout {
        DockLayout { total_width: 1920.0, total_height: 1080.0, top_thickness: top, haut_colonnes: top, left_width: 60.0, right_width: 70.0, bottom_thickness: 50.0 }
    }

    /// MAJ finit à la marge du bord donné, NUM la précède, sans chevauchement,
    /// et les deux sont centrées dans la barre.
    #[test]
    fn les_pastilles_se_rangent_avant_le_bord_donne() {
        let [num, maj] = rectangles(1000.0, &layout(48.0), [(40.0, 20.0), (42.0, 20.0)]);
        assert_eq!(maj[0] + maj[2], 1000.0 - MARGE_DROITE);
        assert_eq!(num[0] + num[2], maj[0] - ECART);
        assert_eq!(maj[3], 24.0);
        assert_eq!(maj[1], 12.0, "centrée : (48 - 24) / 2");
        assert_eq!(maj[2], 42.0 + 2.0 * 9.0, "le texte et ses deux marges");
    }

    /// Une barre très fine ne fait pas déborder la pastille — sans la rendre plus
    /// petite que son texte.
    #[test]
    fn une_barre_fine_plafonne_la_pastille() {
        let [_, maj] = rectangles(500.0, &layout(22.0), [(30.0, 20.0), (30.0, 20.0)]);
        assert_eq!(maj[3], 20.0);
    }

    #[test]
    fn le_texte_est_au_centre_de_sa_pastille() {
        assert_eq!(rectangle_du_texte([100.0, 10.0, 60.0, 24.0], (40.0, 20.0)), [110.0, 12.0, 40.0, 20.0]);
    }

    #[test]
    fn le_texte_reste_lisible_sur_son_fond() {
        assert_eq!(texte_sur([1.0, 0.9, 0.2]), [0.08, 0.08, 0.08], "jaune : texte sombre");
        assert_eq!(texte_sur([0.1, 0.2, 0.6]), [1.0, 1.0, 1.0], "bleu foncé : texte blanc");
    }
}
