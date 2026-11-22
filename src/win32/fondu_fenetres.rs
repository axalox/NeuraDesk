//! ⭐ **LE FONDU DES FENÊTRES DE NEURADESK** — DEMANDE EXPLICITE (2026-10-25) :
//! « un effet de fondu paramétrable à l'ouverture et à la fermeture des
//! fenêtres neuradesk ». Un panneau ou une fenêtre d'applet apparaît et
//! disparaît EN PLACE, en fondant.
//!
//! ⛔ **LES EFFETS DES FENÊTRES DES AUTRES APPLICATIONS ONT ÉTÉ RETIRÉS**
//! (DEMANDE, 2026-11-22 : « retirer les effets de fenêtres et conseiller
//! d'utiliser windhawk dans l'aide »). Ce fondu y survit, sur décision de
//! l'utilisateur, mais n'emprunte plus leur machinerie (image capturée posée
//! dans une surcouche) : c'est l'opacité de la fenêtre elle-même
//! (`WS_EX_LAYERED` + `LWA_ALPHA`), le mécanisme des menus — voir
//! `win32::fade::WindowFade` et `cef_popup::engager_fondu_panneau`.
//!
//! Ce module ne tient que les RÉGLAGES : l'interrupteur global et les deux
//! durées. Une fenêtre peut avoir son fondu PROPRE (la sphère des programmes,
//! voir `cef_popup::regler_fondu_propre`).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

static FONDU_ACTIF: AtomicBool = AtomicBool::new(false);
pub const DUREE_FONDU_PAR_DEFAUT_MS: u32 = 200;
/// Bornes réglables des durées du fondu, en millisecondes.
pub const DUREE_FONDU_MIN_MS: u32 = 50;
pub const DUREE_FONDU_MAX_MS: u32 = 1200;
static DUREE_FONDU_OUVERTURE_MS: AtomicU32 = AtomicU32::new(DUREE_FONDU_PAR_DEFAUT_MS);
static DUREE_FONDU_FERMETURE_MS: AtomicU32 = AtomicU32::new(DUREE_FONDU_PAR_DEFAUT_MS);

pub fn activer_fondu(actif: bool) {
    FONDU_ACTIF.store(actif, Ordering::Relaxed);
}

pub fn fondu_actif() -> bool {
    FONDU_ACTIF.load(Ordering::Relaxed)
}

pub fn regler_duree_fondu_ouverture(ms: u32) {
    DUREE_FONDU_OUVERTURE_MS.store(ms.clamp(DUREE_FONDU_MIN_MS, DUREE_FONDU_MAX_MS), Ordering::Relaxed);
}

pub fn duree_fondu_ouverture_ms() -> u32 {
    DUREE_FONDU_OUVERTURE_MS.load(Ordering::Relaxed)
}

pub fn regler_duree_fondu_fermeture(ms: u32) {
    DUREE_FONDU_FERMETURE_MS.store(ms.clamp(DUREE_FONDU_MIN_MS, DUREE_FONDU_MAX_MS), Ordering::Relaxed);
}

pub fn duree_fondu_fermeture_ms() -> u32 {
    DUREE_FONDU_FERMETURE_MS.load(Ordering::Relaxed)
}

/// La durée du pas de fondu en cours : celle de la disparition si la fenêtre
/// s'éteint, celle de l'apparition sinon. `propre` : le fondu propre de la
/// fenêtre (apparition, disparition), prioritaire sur le réglage global.
pub fn duree_du_pas(propre: Option<(u32, u32)>, disparait: bool) -> f32 {
    let (apparition, disparition) = propre.unwrap_or_else(|| (duree_fondu_ouverture_ms(), duree_fondu_fermeture_ms()));
    (if disparait { disparition } else { apparition }) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_fondu_propre_passe_avant_le_reglage_global() {
        assert_eq!(duree_du_pas(Some((80, 900)), false), 80.0);
        assert_eq!(duree_du_pas(Some((80, 900)), true), 900.0);
    }

    #[test]
    fn les_durees_sont_bornees() {
        regler_duree_fondu_ouverture(1);
        assert_eq!(duree_fondu_ouverture_ms(), DUREE_FONDU_MIN_MS);
        regler_duree_fondu_fermeture(99_999);
        assert_eq!(duree_fondu_fermeture_ms(), DUREE_FONDU_MAX_MS);
        regler_duree_fondu_ouverture(DUREE_FONDU_PAR_DEFAUT_MS);
        regler_duree_fondu_fermeture(DUREE_FONDU_PAR_DEFAUT_MS);
    }
}
