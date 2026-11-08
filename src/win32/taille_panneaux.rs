//! Taille du contenu des panneaux Calendrier, Minuteur, Surveillance,
//! Calculatrice et Serveurs SSH — DEMANDE EXPLICITE (2026-09-26) : « on a
//! ajouté des choses mais les possibilités de réglages n'ont pas été mises à
//! jour ». La section « Taille du contenu des dashs » s'arrêtait au Dash, au
//! Presse-papiers, aux Notes et aux Réglages.
//!
//! Un facteur par panneau, MULTIPLIÉ à l'échelle d'écran que chaque module
//! reçoit dans son `toggle` (voir `app::content_scale_pour`) : le réglage
//! s'ajoute à la mise à l'échelle automatique d'un écran secondaire au lieu de
//! la remplacer. Il prend effet à la prochaine ouverture — ces panneaux
//! occupent la même zone libre que les Réglages, jamais ouverts en même temps.

use std::cell::Cell;

/// Les panneaux réglables, dans l'ordre des curseurs de `settings.html`. Le
/// nom est aussi celui du fichier (`<nom>_content_scale.txt`).
// ⛔ **L'ÉDITEUR D'IMAGE ET LE SOUS-DOCK NE SONT PAS (ENCORE) ICI, ET C'EST
// DÉLIBÉRÉ.** La vérification a montré que le plan se trompait DEUX FOIS : les
// Notes ont déjà leurs deux curseurs (« Texte »/« Images », jusqu'à 400 %), et
// c'est CETTE liste — globale, un curseur par panneau — qui est le foyer des
// autres, pas un tableau par écran.
//
// ⚠️ Ajouter les deux noms ici SANS câbler leurs modules donnerait des curseurs
// qui ne font RIEN : le miroir exact du défaut que ce dépôt connaît déjà
// (« poussée, présente dans l'état, et simplement jamais dessinée », voir le test
// de `cef_settings` sur les curseurs du dégradé). Le sous-dock est à une ligne de
// l'être — sa page lit déjà `state.echelle` (`sous_dock.html`) et `composer` porte
// déjà le champ ; l'éditeur d'image, lui, n'a AUCUN `--text-scale` : son facteur
// passe par le zoom CEF (`cef_popup::regler_zoom_textes`).
pub const PANNEAUX: [&str; 19] =
    // ⭐ **L'AIDE** (2026-10-07) : son module CONSOMME ce facteur (voir
    // `cef_aide::toggle`), donc son curseur n'est pas un no-op. C'est la règle —
    // n'ajouter un nom ici qu'avec le module qui le lit.
    // ⭐ **LE SCANNER** (2026-10-09) : même règle, et elle est tenue —
    // `cef_scanner::toggle` lit `facteur("scanner")`.
    // ⭐ **LES SEPT SANS CURSEUR** (2026-10-18) — DEMANDE EXPLICITE : recherche,
    // automatisations, météo, éditeurs de shaders, de lueurs et de textures,
    // éditeur d'image. Chacun lit son facteur à l'ouverture (`toggle`).
    [
        "calendar", "minuteur", "surveillance", "calculatrice", "ssh", "terminal", "dossier", "visionneuse", "couleurs",
        "aide", "scanner", "recherche", "automatisations", "meteo", "shaders", "lueurs", "textures", "editeur_image",
        // ⭐ LE MONITEUR SYSTÈME (2026-10-20) : `cef_moniteur::toggle` lit `facteur("moniteur")`.
        "moniteur",
    ];

const MIN_SCALE: f32 = 0.25;
const MAX_SCALE: f32 = 2.0;

thread_local! {
    static FACTEURS: [Cell<f32>; PANNEAUX.len()] = const { [const { Cell::new(1.0) }; PANNEAUX.len()] };
}

fn index(panneau: &str) -> Option<usize> {
    PANNEAUX.iter().position(|p| *p == panneau)
}

/// Facteur courant de `panneau` — 1.0 pour un nom inconnu.
pub fn facteur(panneau: &str) -> f32 {
    index(panneau).map_or(1.0, |i| FACTEURS.with(|f| f[i].get()))
}

/// Règle le facteur de `panneau` (borné), sans le sauvegarder. Renvoie
/// `false` pour un nom inconnu.
pub fn regler(panneau: &str, scale: f32) -> bool {
    let Some(i) = index(panneau) else { return false };
    FACTEURS.with(|f| f[i].set(scale.clamp(MIN_SCALE, MAX_SCALE)));
    true
}

/// ⭐ **LA TAILLE DES TEXTES, GLOBALE — ET CELLE DES NOTIFICATIONS** — DEMANDE
/// EXPLICITE (2026-10-18) : « pouvoir régler la taille des textes des
/// notifications qui s'affichent à l'écran ainsi que la taille des textes des
/// autres menus et fenêtres d'applis ». Deux facteurs qui zooment TOUTE la
/// fenêtre (texte, icônes, marges), comme les curseurs « Contenu » :
///
/// - `textes` multiplie l'échelle que `app::content_scale_pour` donne à CHAQUE
///   menu et fenêtre d'applet à son ouverture — il s'ajoute aux curseurs par
///   panneau et à « Taille des menus » au lieu de les remplacer ;
/// - `notifications` agrandit la carte du bandeau (`win32::bandeau`) et sa page.
///
/// Mêmes bornes et même fichier que les panneaux (`<nom>_content_scale.txt`),
/// mais HORS de `PANNEAUX` : ce ne sont pas des curseurs « Contenu ».
pub const TEXTES: &str = "textes";
pub const NOTIFICATIONS: &str = "notifications";

thread_local! {
    static GLOBAUX: [Cell<f32>; 2] = const { [const { Cell::new(1.0) }; 2] };
}

fn index_global(nom: &str) -> Option<usize> {
    [TEXTES, NOTIFICATIONS].iter().position(|n| *n == nom)
}

/// Facteur global `nom` (`TEXTES` ou `NOTIFICATIONS`) — 1.0 pour un nom inconnu.
pub fn global(nom: &str) -> f32 {
    index_global(nom).map_or(1.0, |i| GLOBAUX.with(|g| g[i].get()))
}

/// Règle le facteur global `nom` (borné), sans le sauvegarder.
pub fn regler_global(nom: &str, scale: f32) -> bool {
    let Some(i) = index_global(nom) else { return false };
    GLOBAUX.with(|g| g[i].set(scale.clamp(MIN_SCALE, MAX_SCALE)));
    true
}

/// Recharge les facteurs sauvegardés — une fois au démarrage (`app::run`).
pub fn charger() {
    for panneau in PANNEAUX {
        if let Some(scale) = crate::config::load_panel_content_scale(panneau) {
            regler(panneau, scale);
        }
    }
    for nom in [TEXTES, NOTIFICATIONS] {
        if let Some(scale) = crate::config::load_panel_content_scale(nom) {
            regler_global(nom, scale);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facteur_borne_et_nom_inconnu_ignore() {
        assert!(regler("minuteur", 9.0));
        assert_eq!(facteur("minuteur"), MAX_SCALE);
        assert!(regler("minuteur", 0.01));
        assert_eq!(facteur("minuteur"), MIN_SCALE);
        assert!(!regler("dash", 1.5));
        assert_eq!(facteur("dash"), 1.0);
        assert_eq!(facteur("ssh"), 1.0);
    }

    #[test]
    fn les_facteurs_globaux_sont_bornes_et_a_part() {
        assert_eq!(global(TEXTES), 1.0);
        assert!(regler_global(NOTIFICATIONS, 1.5));
        assert_eq!(global(NOTIFICATIONS), 1.5);
        assert!(regler_global(TEXTES, 9.0));
        assert_eq!(global(TEXTES), MAX_SCALE);
        assert!(!regler_global("minuteur", 1.5), "un panneau n'est pas un facteur global");
        assert!(!regler("textes", 1.5), "ni l'inverse");
    }
}
