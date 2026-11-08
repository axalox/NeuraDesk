//! ⭐ **LE DÉBIT RÉSEAU DANS LA BARRE DU HAUT** — l'affichage ; le relevé est
//! dans `win32::debit_reseau`. DEMANDE EXPLICITE (2026-10-14) : « à la gauche de
//! l'icône réseau de la barre du haut ».
//!
//! L'icône réseau est la PREMIÈRE de la zone d'icônes de droite : le texte se
//! pose donc juste avant `Dock::top_right_zone_left_edge`, et TOUS les textes de
//! la moitié droite (moniteur système, fréquence, Verr. num / maj) s'ancrent
//! désormais sur SON bord gauche (`bord_gauche`) au lieu du bord de la zone.
//!
//! ⚠️ **UNE LARGEUR RÉSERVÉE, PAS LA LARGEUR DU TEXTE.** Le texte change chaque
//! seconde ; ancrer les voisins sur sa largeur réelle ferait trembler toute la
//! moitié droite de la barre à chaque relevé. La place est celle d'un GABARIT
//! (le texte le plus large qu'on puisse afficher, jamais dessiné), et le texte
//! réel s'aligne à DROITE dedans — collé à l'icône réseau.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use windows::Win32::Graphics::Direct3D11::ID3D11Device;

use crate::dock_instance::CachedText;
use crate::gfx::texture::Texture;
use crate::gfx::QuadInstance;
use crate::ui::dock::DockLayout;
use crate::ui::topbar;

/// Le texte le plus large possible (voir `win32::debit_reseau::formater` :
/// trois chiffres et une virgule, « Mo » étant la plus large des unités).
const GABARIT: &str = "↑ 88,8 Mo/s  ↓ 88,8 Mo/s";
/// Écart entre le texte et l'icône réseau — celui du moniteur système.
const MARGE_DROITE: f32 = 16.0;

static AFFICHE: AtomicBool = AtomicBool::new(true);
/// Le dernier texte relevé, partagé par toutes les barres (un débit est GLOBAL,
/// pas propre à un écran). Vide tant qu'aucun débit n'a été calculé.
static TEXTE: Mutex<String> = Mutex::new(String::new());

pub fn affiche() -> bool {
    AFFICHE.load(Ordering::Relaxed)
}

pub fn set_affiche(v: bool) {
    AFFICHE.store(v, Ordering::Relaxed);
}

pub fn publier(texte: String) {
    if let Ok(mut t) = TEXTE.lock() {
        *t = texte;
    }
}

fn texte_courant() -> String {
    TEXTE.lock().map(|t| t.clone()).unwrap_or_default()
}

/// Le rectangle RÉSERVÉ — PURE : il finit à `MARGE_DROITE` de l'icône réseau.
pub fn rect_reserve(bord_icones: f32, layout: &DockLayout, (w, h): (f32, f32)) -> [f32; 4] {
    [bord_icones - MARGE_DROITE - w, ((layout.top_thickness - h) * 0.5).round(), w, h]
}

/// Le texte réel, aligné à DROITE dans la place réservée — PURE.
pub fn rect_texte(reserve: [f32; 4], (w, h): (f32, f32)) -> [f32; 4] {
    let [x, y, rw, rh] = reserve;
    [(x + rw - w).round(), (y + (rh - h) * 0.5).round(), w, h]
}

/// Les textes d'UNE barre (à SA police), plus le gabarit qui réserve la place.
pub struct Textes {
    texte: CachedText,
    gabarit: CachedText,
}

impl Textes {
    pub fn new() -> Self {
        Self { texte: CachedText::new(), gabarit: CachedText::new() }
    }

    pub fn sync(&mut self, device: &ID3D11Device, layout: &DockLayout, taille_texte: Option<f32>) -> Result<(), Box<dyn std::error::Error>> {
        let px = topbar::sys_monitor_font_px(layout, taille_texte);
        self.texte.sync(device, &texte_courant(), px, topbar::SYS_MONITOR_FONT_NAME, false)?;
        self.gabarit.sync(device, GABARIT, px, topbar::SYS_MONITOR_FONT_NAME, false)?;
        Ok(())
    }

    fn reserve(&self, bord_icones: f32, layout: &DockLayout) -> Option<[f32; 4]> {
        if !affiche() {
            return None;
        }
        let (_, w, h) = self.gabarit.texture()?;
        Some(rect_reserve(bord_icones, layout, (w, h)))
    }

    /// Le bord auquel s'ancrent les autres textes de la moitié droite. `bord_zone` :
    /// celui de la zone d'icônes, où ils s'ancraient avant le débit ;
    /// `bord_reseau` : celui de l'icône réseau, contre laquelle le débit se pose.
    /// Le PLUS À GAUCHE des deux bords : tant que le débit tient dans le vide de
    /// la zone (voir `Dock::bord_gauche_reseau`), les autres textes ne bougent pas.
    pub fn bord_gauche(&self, bord_zone: f32, bord_reseau: f32, layout: &DockLayout) -> f32 {
        self.reserve(bord_reseau, layout).map_or(bord_zone, |r| r[0].min(bord_zone))
    }

    pub fn instance(&self, bord_icones: f32, layout: &DockLayout, couleur: [f32; 3]) -> Option<(&Texture, QuadInstance)> {
        let reserve = self.reserve(bord_icones, layout)?;
        let (texture, w, h) = self.texte.texture()?;
        Some((texture, QuadInstance::new_glyph(rect_texte(reserve, (w, h)), couleur)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(top: f32) -> DockLayout {
        DockLayout { total_width: 1920.0, total_height: 1080.0, top_thickness: top, haut_colonnes: top, left_width: 60.0, right_width: 70.0, bottom_thickness: 50.0 }
    }

    /// Tant que le débit tient dans le vide de la zone, les autres textes ne
    /// bougent pas ; dès qu'il en sort, ils s'écartent devant lui.
    #[test]
    fn le_bord_des_autres_textes_est_le_plus_a_gauche() {
        let reserve = |bord: f32| rect_reserve(bord, &layout(48.0), (180.0, 20.0))[0];
        assert_eq!(reserve(1500.0).min(1000.0), 1000.0, "le débit tient dans le vide : la zone");
        assert_eq!(reserve(1100.0).min(1000.0), 1100.0 - MARGE_DROITE - 180.0, "il en sort : son bord");
    }

    #[test]
    fn la_place_finit_avant_l_icone_reseau() {
        let r = rect_reserve(1500.0, &layout(48.0), (180.0, 20.0));
        assert_eq!(r, [1500.0 - MARGE_DROITE - 180.0, 14.0, 180.0, 20.0]);
    }

    /// Le texte réel colle au bord DROIT de la place : c'est ce qui le garde
    /// contre l'icône réseau quand il est plus court que le gabarit.
    #[test]
    fn le_texte_s_aligne_a_droite_dans_sa_place() {
        let t = rect_texte([1000.0, 14.0, 180.0, 20.0], (120.0, 20.0));
        assert_eq!(t, [1060.0, 14.0, 120.0, 20.0]);
    }
}
