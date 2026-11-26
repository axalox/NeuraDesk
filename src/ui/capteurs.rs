//! ⭐ **LES DEUX CASES DE CONFIDENTIALITÉ DE LA BARRE DU HAUT** — micro puis
//! caméra, allumés tant qu'une application les utilise (idée n°3 reprise de
//! FluidWin, voir `win32::capteurs_medias` pour la détection et
//! `ui::topbar::capteur_rect` pour la place — à GAUCHE de l'horloge).
//!
//! Les glyphes viennent de **`Segoe MDL2 Assets`**, la police d'icônes de
//! Windows déjà utilisée par les boutons média (`topbar::MEDIA_GLYPH_FONT_NAME`) :
//! aucun PNG à embarquer, aucun thème d'icônes à suivre — un indicateur de
//! confidentialité ne doit pas changer d'apparence avec la couleur des applets.
//!
//! ⚠️ **UNE TEXTURE PAR BARRE**, pas une texture partagée : elle est rendue à la
//! taille d'icône de CETTE barre (voir `dock_instance::CachedText` et la doc de
//! `ui::desktop_labels`, qui porte le bug que ce cache évite).
//!
//! ⚠️ **L'ORDRE DE `GLYPHES` EST CELUI DE `topbar::CAPTEURS` ET DES CHAMPS DE
//! `Usage`** : micro d'abord, caméra ensuite. Le test
//! `chaque_glyphe_a_sa_case` le vérifie côté longueur ; l'ordre, lui, est écrit
//! une seule fois, ici, et les appelants lisent `Usage` par nom de champ — pas
//! par index — pour ne pas dépendre de cette position.

use windows::Win32::Graphics::Direct3D11::ID3D11Device;

use crate::dock_instance::CachedText;
use crate::gfx::QuadInstance;
use crate::gfx::texture::Texture;
use crate::ui::dock::DockLayout;
use crate::ui::topbar;
use crate::win32::capteurs_medias::Usage;

/// Les glyphes, dans l'ordre des cases : micro (`E720`), caméra (`E722`).
///
/// ⚠️ Codés en échappements, pas en caractères littéraux : la zone privée
/// d'Unicode de la police ne doit pas dépendre de l'encodage du fichier source.
pub const GLYPHES: [&str; topbar::CAPTEURS] = ["\u{E720}", "\u{E722}"];

/// Les deux textures d'UNE barre, à SA taille d'icône.
pub struct Textes {
    textes: [CachedText; topbar::CAPTEURS],
}

impl Textes {
    pub fn new() -> Self {
        Self { textes: [CachedText::new(), CachedText::new()] }
    }

    /// À appeler à chaque frame : ne régénère les bitmaps qu'au changement de
    /// taille de barre (voir `CachedText::sync`).
    pub fn sync(&mut self, device: &ID3D11Device, layout: &DockLayout) -> Result<(), Box<dyn std::error::Error>> {
        let px = topbar::capteur_font_px(layout);
        for (cache, glyphe) in self.textes.iter_mut().zip(GLYPHES) {
            cache.sync(device, glyphe, px, topbar::MEDIA_GLYPH_FONT_NAME, false)?;
        }
        Ok(())
    }

    pub fn texture(&self, i: usize) -> Option<&Texture> {
        self.textes.get(i)?.texture().map(|(t, _, _)| t)
    }

    /// `[micro, caméra]` — `None` pour une source AU REPOS : ne rien pousser est
    /// le seul moyen de la cacher (le nuanceur de glyphes ignore l'alpha de la
    /// couleur, voir `app::run`).
    ///
    /// `clock` : la taille de la texture d'horloge, dont toute la rangée de
    /// droite dépend (voir `topbar::bell_rect`) — l'appelant est donc gardé par
    /// « la texture d'horloge existe ».
    pub fn instances(
        &self,
        layout: &DockLayout,
        clock: (f32, f32),
        usage: Usage,
        couleur: [f32; 3],
    ) -> [Option<QuadInstance>; topbar::CAPTEURS] {
        let actifs = [usage.micro, usage.camera];
        std::array::from_fn(|i| {
            actifs[i].then(|| {
                topbar::capteur_instance(
                    // Échelle 1.0 EN DUR : ces cases ne magnifient pas, rien
                    // n'est cliquable (voir `topbar::capteur_rect`).
                    topbar::presented_capteur_rect(topbar::capteur_rect(layout, clock.0, clock.1, i), 1.0, layout),
                    couleur,
                )
            })
        })
    }
}

impl Default for Textes {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> DockLayout {
        DockLayout {
            total_width: 1920.0,
            total_height: 1080.0,
            top_thickness: 48.0,
            haut_colonnes: 48.0,
            left_width: 60.0,
            right_width: 70.0,
            bottom_thickness: 50.0,
        }
    }

    /// ⛔ **UNE CASE PAR GLYPHE** : un glyphe ajouté sans case (ou l'inverse)
    /// enverrait un indicateur dans le vide — ou laisserait une case toujours
    /// noire, sans qu'aucun test ne le voie.
    #[test]
    fn chaque_glyphe_a_sa_case() {
        assert_eq!(GLYPHES.len(), topbar::CAPTEURS);
        assert_eq!(GLYPHES.len(), 2, "micro puis caméra");
        assert_ne!(GLYPHES[0], GLYPHES[1], "deux sources, deux glyphes distincts");
    }

    /// Le glyphe du micro EST bien celui attendu de `Segoe MDL2 Assets` — une
    /// faute de frappe dans l'échappement donnerait une case vide, qui se
    /// lirait comme « micro libre » alors que le micro est occupé.
    #[test]
    fn les_glyphes_sont_ceux_de_mdl2() {
        assert_eq!(GLYPHES[0], "\u{E720}", "Microphone (Segoe MDL2 Assets)");
        assert_eq!(GLYPHES[1], "\u{E722}", "Camera (Segoe MDL2 Assets)");
    }

    /// **LES DEUX CASES SONT RÉSERVÉES** : chaque source a la sienne, quelle que
    /// soit l'autre — et elles ne se chevauchent jamais.
    ///
    /// ⛔ **À GAUCHE DE L'HORLOGE** : c'est le seul côté qui reste libre sur un
    /// écran secondaire. À droite, la capture du 2026-11-26 a montré la caméra
    /// PAR-DESSUS la pastille « NUM » de cette machine.
    #[test]
    fn les_deux_cases_sont_reservees_et_distinctes() {
        let l = layout();
        let (w, h) = (80.0, 30.0);
        let micro = topbar::capteur_rect(&l, w, h, 0);
        let camera = topbar::capteur_rect(&l, w, h, 1);
        let note = topbar::note_rect(&l, w, h);
        let horloge_gauche = (l.total_width - w) * 0.5;
        assert_eq!(micro[0] + micro[2], note[0] - topbar::CAPTEUR_MARGIN_LEFT, "le micro précède l'icône de note");
        assert!(camera[0] + camera[2] <= micro[0], "la caméra ne recouvre jamais le micro");
        assert!(camera[0] > 0.0 && camera[0] + camera[2] < horloge_gauche, "la rangée reste à gauche de l'horloge");
        assert_eq!(micro[3], camera[3], "même hauteur");
        assert_eq!(micro[1], camera[1], "même centrage vertical");
    }

    /// La case ne suit pas `magnification` : ces indicateurs ne se cliquent pas,
    /// donc leur rectangle dessiné est le rectangle au repos.
    #[test]
    fn le_rectangle_dessine_est_celui_au_repos() {
        let l = layout();
        let repos = topbar::capteur_rect(&l, 80.0, 30.0, 0);
        assert_eq!(topbar::presented_capteur_rect(repos, 1.0, &l), repos);
    }
}
