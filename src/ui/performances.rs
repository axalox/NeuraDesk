//! **LA PRESSION QUE NEURADESK MET SUR LE PROCESSEUR ET LE GPU** — DEMANDE
//! EXPLICITE (2026-11-11) : « serait-il possible d'avoir un réglage pour gérer
//! la pression sur le processeur et le gpu ? ».
//!
//! Deux leviers, et un seul endroit pour les lire :
//!
//! - **LA CADENCE MAXIMALE** (`cadence`, images par seconde). Elle ne concerne
//!   que les frames réellement PRÉSENTÉES. Le `Present` d'une image 4K coûte
//!   ~13 ms de CPU dans le pilote (mesuré, voir la doc de `app::run`) et peint
//!   le GPU derrière : c'est le premier poste, et le seul qui grandit avec la
//!   surface. `CADENCE_ECRAN` (0) veut dire « ne rien plafonner » : c'est la
//!   cadence de l'écran que le vsync impose de toute façon.
//!
//! - **LE MODE ÉCONOMIE** (`economie`) : quand la souris n'est sur aucune barre
//!   ET qu'aucun panneau n'est visible, les animations CONTINUES (dégradé
//!   animé, logo, textures « évoluer », magnification) sont suspendues. Le
//!   reste — une image quand quelque chose change — continue normalement.
//!   Pourquoi la condition sur la souris : c'est le seul moment où l'utilisateur
//!   REGARDE les animations. Un panneau ouvert le retient aussi, même souris
//!   ailleurs : on regarde alors la page, pas les barres.
//!
//! ⚠️ **LES DEUX LEVIERS SONT GLOBAUX** (`thread_local`, même motif que
//! `ui::dock::survol_fond`) : `app::run`, le fil des réglages et la page CEF des
//! Réglages vivent sur le même fil, donc une cellule suffit et aucun câblage
//! n'est à tenir entre trois modules.
//!
//! Les profils (`PRESETS`) ne sont qu'une manière de poser les deux d'un coup ;
//! ils ne sont PAS mémorisés — `preset_courant` les reconnaît en relisant les
//! deux valeurs, ce qui fait retomber le panneau sur « aucun profil » dès qu'un
//! curseur est déplacé à la main. Un profil mémorisé aurait menti après
//! n'importe quel réglage fin.

use std::cell::Cell;
use std::time::Duration;

/// Cadence la plus BASSE réglable au curseur : en dessous, la barre ne répond
/// plus assez vite au survol pour être utilisable (l'animation de magnification
/// se voit).
pub const CADENCE_MIN: u32 = 10;

/// Cadence la plus HAUTE réglable au curseur. Les écrans de cette machine ne
/// dépassent pas 144 Hz ; au-delà, le plafond ne veut plus rien dire.
pub const CADENCE_MAX: u32 = 144;

/// « Suivre la fréquence de l'écran » : aucun plafond. C'est le comportement
/// d'avant le réglage, donc le défaut — un réglage neuf qui change l'affichage
/// existant serait un piège.
pub const CADENCE_ECRAN: u32 = 0;

/// Les trois profils du panneau : `(nom, cadence, économie)`.
///
/// ⚠️ **L'ORDRE EST CELUI DE L'ÉCRAN** (Économie, Équilibré, Qualité) : le
/// panneau les affiche dans cet ordre, et `preset_courant` s'y réfère par nom.
/// **Qualité** porte `CADENCE_ECRAN` : c'est le rendu d'avant, le plus fluide.
pub const PRESETS: [(&str, u32, bool); 3] =
    [("economie", 15, true), ("equilibre", 30, false), ("qualite", CADENCE_ECRAN, false)];

thread_local! {
    static CADENCE: Cell<u32> = const { Cell::new(CADENCE_ECRAN) };
    static ECONOMIE: Cell<bool> = const { Cell::new(false) };
}

/// La cadence maximale en images par seconde, `CADENCE_ECRAN` (0) valant
/// « aucune limite ».
pub fn cadence() -> u32 {
    CADENCE.with(Cell::get)
}

/// Pose la cadence en la BORNANT (voir `CADENCE_MIN`/`CADENCE_MAX`) et rend la
/// valeur d'avant — même contrat que `ui::dock::set_magnification_en_cours`, ce
/// qui rend un test capable de restaurer l'état de la machine. `CADENCE_ECRAN`
/// reste possible : c'est un sens, pas une valeur hors bornes.
pub fn set_cadence(fps: u32) -> u32 {
    let borne = if fps == CADENCE_ECRAN { CADENCE_ECRAN } else { fps.clamp(CADENCE_MIN, CADENCE_MAX) };
    CADENCE.with(|c| c.replace(borne))
}

/// Le temps MINIMUM entre deux images présentées, `None` quand rien n'est
/// plafonné. C'est ce que lit la boucle d'`app::run` : elle présente quand
/// `derniere_presentation.elapsed()` a atteint cette période, et dort le reste.
pub fn periode() -> Option<Duration> {
    match cadence() {
        0 => None,
        fps => Some(Duration::from_secs_f64(1.0 / f64::from(fps))),
    }
}

/// Le mode économie est-il demandé ? `false` par défaut : sans ce drapeau, rien
/// ne change par rapport à avant le réglage.
pub fn economie() -> bool {
    ECONOMIE.with(Cell::get)
}

/// Pose le mode économie et rend l'état d'avant (même contrat que `set_cadence`).
pub fn set_economie(actif: bool) -> bool {
    ECONOMIE.with(|c| c.replace(actif))
}

/// Applique un profil de `PRESETS` par son nom (`"economie"`, `"equilibre"`,
/// `"qualite"`). Rend `false` — et ne touche à RIEN — pour un nom inconnu : une
/// page CEF peut envoyer n'importe quelle chaîne, et un `unwrap` sur elle
/// transformerait une faute de frappe en plantage du panneau.
pub fn appliquer_preset(nom: &str) -> bool {
    let Some(&(_, cadence, economie)) = PRESETS.iter().find(|(n, _, _)| *n == nom) else {
        return false;
    };
    set_cadence(cadence);
    set_economie(economie);
    true
}

/// Le nom du profil que les deux valeurs courantes RÉALISENT, ou `""` si elles
/// n'en forment aucun (curseur déplacé à la main). C'est la valeur poussée au
/// panneau : aucun bouton n'est allumé dans le second cas, ce qui dit
/// exactement la vérité.
pub fn preset_courant() -> &'static str {
    let (cadence, economie) = (cadence(), economie());
    PRESETS
        .iter()
        .find(|&&(_, c, e)| c == cadence && e == economie)
        .map(|&(nom, _, _)| nom)
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les deux cellules sont GLOBALES au fil : chaque test restaure l'état
    /// qu'il a trouvé, sinon un test qui passe seul échouerait dans la suite
    /// complète (le harnais fait tourner les tests sur plusieurs fils, mais deux
    /// tests de CE module peuvent tomber sur le même).
    fn avec_etat(cadence: u32, economie: bool) -> (u32, bool) {
        (set_cadence(cadence), set_economie(economie))
    }

    fn restaurer((cadence, economie): (u32, bool)) {
        set_cadence(cadence);
        set_economie(economie);
    }

    #[test]
    fn la_cadence_est_bornee_et_zero_suit_l_ecran() {
        let avant = avec_etat(CADENCE_ECRAN, false);
        // 5 images/s est sous le plancher : le survol deviendrait illisible.
        assert_eq!(set_cadence(5), CADENCE_ECRAN);
        assert_eq!(cadence(), CADENCE_MIN, "une cadence sous le plancher doit être remontée, pas acceptée");
        // 400 dépasse le plafond de tous les écrans de cette machine.
        set_cadence(400);
        assert_eq!(cadence(), CADENCE_MAX);
        // Zéro n'est pas « sous le plancher » : c'est le sens « suivre l'écran ».
        set_cadence(CADENCE_ECRAN);
        assert_eq!(cadence(), CADENCE_ECRAN);
        assert_eq!(periode(), None, "aucun plafond ne doit être imposé quand on suit l'écran");
        restaurer(avant);
    }

    #[test]
    fn la_periode_se_deduit_de_la_cadence() {
        let avant = avec_etat(CADENCE_ECRAN, false);
        set_cadence(30);
        let periode = periode().expect("30 images/s doit imposer une période");
        // 1/30 s = 33,3 ms — la borne haute tolère l'arrondi flottant.
        assert!(periode >= Duration::from_millis(33) && periode <= Duration::from_millis(34), "{periode:?}");
        restaurer(avant);
    }

    #[test]
    fn les_presets_posent_les_deux_leviers_et_se_relisent() {
        let avant = avec_etat(CADENCE_ECRAN, false);
        assert!(appliquer_preset("economie"));
        assert_eq!((cadence(), economie()), (15, true));
        assert_eq!(preset_courant(), "economie");
        assert!(appliquer_preset("equilibre"));
        assert_eq!((cadence(), economie()), (30, false));
        assert_eq!(preset_courant(), "equilibre");
        assert!(appliquer_preset("qualite"));
        assert_eq!((cadence(), economie()), (CADENCE_ECRAN, false));
        assert_eq!(preset_courant(), "qualite");
        restaurer(avant);
    }

    /// Un profil inconnu — faute de frappe dans la page CEF — ne touche à rien
    /// et ne panique pas.
    #[test]
    fn un_preset_inconnu_ne_change_rien() {
        let avant = avec_etat(30, false);
        assert!(!appliquer_preset("turbo"));
        assert_eq!((cadence(), economie()), (30, false));
        // Une cadence hors des profils : plus aucun bouton n'est allumé.
        set_cadence(45);
        assert_eq!(preset_courant(), "", "un réglage fin doit faire tomber au profil « aucun »");
        restaurer(avant);
    }

    #[test]
    fn le_mode_economie_rend_son_ancien_etat() {
        let avant = set_economie(false);
        assert!(!economie(), "coupé par défaut : le réglage neuf ne change rien");
        assert!(!set_economie(true), "l'état d'avant était bien `false`");
        assert!(economie());
        assert!(set_economie(false), "et la retombée rend `true`");
        set_economie(avant);
    }
}
