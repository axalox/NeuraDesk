//! **LE RYTHME DES IMAGES, MESURÉ DANS LA BOUCLE** — journal de diagnostic
//! (`NEURADESK_DIAG=1`, inerte sinon) né de la plainte du 2026-11-13 : « ce
//! n'est pas fluide », après le retrait du plafond de fréquence du clic droit.
//!
//! Le survol ne se simule pas (règle de sécurité du dépôt) : c'est
//! l'utilisateur qui bouge la souris, et ce module dit ce que la boucle a fait
//! pendant ce temps. Une ligne par seconde :
//!
//! `[rythme] tours=… images=… trou_max=…ms trous>20ms=… travail_max=…ms
//!  present_max=…ms sommeil=…ms raisons=salve/anime/secours`
//!
//! - `trou_max` : le plus grand écart entre deux images PRÉSENTÉES — c'est la
//!   saccade telle que l'œil la voit ;
//! - `travail_max` : le plus long tour de boucle AVANT le `Present` (dessin,
//!   CEF, fenêtres) — un tour lent fait sauter un vsync ;
//! - `present_max` : le plus long `Present` ;
//! - `raisons` : pourquoi les images ont été présentées.

use std::time::{Duration, Instant};

/// Au-delà, deux images présentées sont un trou visible (un vsync à 60 Hz
/// dure 16,7 ms).
const TROU_VISIBLE: Duration = Duration::from_millis(20);

/// Pourquoi une image a été présentée — la première raison vraie l'emporte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Raison {
    Salve,
    Anime,
    Secours,
    Capture,
}

#[derive(Default)]
pub struct RythmeImages {
    debut_fenetre: Option<Instant>,
    derniere_image: Option<Instant>,
    tours: u32,
    images: u32,
    trou_max: Duration,
    trous_visibles: u32,
    travail_max: Duration,
    present_max: Duration,
    sommeil: Duration,
    travail_total: Duration,
    raisons: [u32; 4],
}

impl RythmeImages {
    /// Un tour de boucle : `travail` = du début du tour jusqu'au `Present`.
    pub fn tour(&mut self, travail: Duration) {
        self.tours += 1;
        self.travail_total += travail;
        self.travail_max = self.travail_max.max(travail);
    }

    pub fn image(&mut self, quand: Instant, duree_present: Duration, raison: Raison) {
        if let Some(avant) = self.derniere_image {
            let trou = quand.saturating_duration_since(avant);
            self.trou_max = self.trou_max.max(trou);
            if trou > TROU_VISIBLE {
                self.trous_visibles += 1;
            }
        }
        self.derniere_image = Some(quand);
        self.images += 1;
        self.present_max = self.present_max.max(duree_present);
        self.raisons[raison as usize] += 1;
    }

    pub fn sommeil(&mut self, duree: Duration) {
        self.sommeil += duree;
    }

    /// Rend la ligne du journal une fois par seconde, et remet les compteurs
    /// à zéro (sauf `derniere_image`, pour que le trou à cheval sur deux
    /// fenêtres soit compté).
    pub fn ligne_si_seconde(&mut self, maintenant: Instant) -> Option<String> {
        let debut = *self.debut_fenetre.get_or_insert(maintenant);
        if maintenant.saturating_duration_since(debut) < Duration::from_secs(1) {
            return None;
        }
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let ligne = format!(
            "[rythme] tours={} images={} trou_max={:.1}ms trous>20ms={} travail_max={:.1}ms travail_moy={:.1}ms present_max={:.1}ms sommeil={:.0}ms raisons={}/{}/{}/{}",
            self.tours,
            self.images,
            ms(self.trou_max),
            self.trous_visibles,
            ms(self.travail_max),
            ms(self.travail_total) / f64::from(self.tours.max(1)),
            ms(self.present_max),
            ms(self.sommeil),
            self.raisons[0],
            self.raisons[1],
            self.raisons[2],
            self.raisons[3],
        );
        *self = RythmeImages { derniere_image: self.derniere_image, debut_fenetre: Some(maintenant), ..Default::default() };
        Some(ligne)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_trou_entre_deux_images_est_compte_et_la_fenetre_se_vide() {
        let t0 = Instant::now();
        let mut r = RythmeImages::default();
        assert!(r.ligne_si_seconde(t0).is_none());
        r.image(t0, Duration::from_millis(2), Raison::Salve);
        r.image(t0 + Duration::from_millis(16), Duration::from_millis(3), Raison::Anime);
        r.image(t0 + Duration::from_millis(60), Duration::from_millis(1), Raison::Secours);
        let ligne = r.ligne_si_seconde(t0 + Duration::from_secs(1)).expect("une seconde écoulée");
        assert!(ligne.contains("images=3"), "{ligne}");
        assert!(ligne.contains("trou_max=44.0ms"), "{ligne}");
        assert!(ligne.contains("trous>20ms=1"), "{ligne}");
        assert!(ligne.contains("raisons=1/1/1/0"), "{ligne}");
        assert_eq!(r.images, 0);
        assert!(r.derniere_image.is_some(), "le trou à cheval sur deux fenêtres doit rester mesurable");
    }
}
