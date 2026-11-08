//! Réglages partagés des menus contextuels du dock — le rendu proprement dit
//! vit désormais dans les popups CEF individuels (`win32::cef_context`/
//! `cef_network`/`cef_sound`/`cef_voicemeeter`/`cef_calendar`/
//! `cef_spacer_size`/`cef_bar_size`), plus dans l'ancien
//! module `win32::popup_menu` (`egui`/D3D11) retiré à la Phase 7 de
//! `docs/plans/cef-web-ui-migration.md` (retrait complet d'egui).
//!
//! Ce module contenait à l'origine la construction ET le rendu de tous les
//! menus contextuels natifs (`TrackPopupMenu`, items `MF_OWNERDRAW` peints à
//! la main dans `win32::window`) : clic droit sur l'icône NeuraDesk, menu
//! d'alimentation, calendrier, volet son, volet réseau, presse-papiers
//! rapide — migrés vers `egui` (coins arrondis natifs impossibles à obtenir
//! proprement sur une fenêtre `TrackPopupMenu`, qui de toute façon bloque le
//! message loop et ne peut donc jamais être vérifiée par capture d'écran),
//! puis vers CEF/HTML (voir la doc de module de `win32::cef_settings`).
//!
//! `Bar`/`MENU_GAP`/`menu_scale`/`menu_corner_radius`/`menu_tint`/
//! `menu_accent_rgb` restent définis ICI : `Bar` est réutilisé par
//! `win32::cef_settings` (sélecteurs de couleur par barre), et les quatre
//! réglages de menus (échelle, rayon d'arrondi, transparence, couleur de
//! mise en avant) sont globaux, réglables depuis la section "Apparence des
//! menus" du panneau de réglages et persistés (voir `config::
//! load_menu_scale`/`save_menu_scale` et les équivalents pour le rayon/la
//! transparence/la couleur) — un seul endroit à connaître pour l'app
//! entière plutôt que de les dupliquer dans chaque popup CEF.
//!
//! `menu_tint`/`menu_accent_rgb` sont lus par `app::run` pour construire le
//! tint réel (`QuadInstance::new_backdrop`) de TOUS les popups CEF de type
//! "menu" ci-dessus — BUG RÉEL SIGNALÉ ("les menus et les dashs ne suivent
//! pas les thèmes de couleur choisis") : entre la Phase 7 et un premier
//! correctif, plus aucun code ne relisait ces deux getters pour dessiner
//! quoi que ce soit, `app::run` utilisait encore le tint fixe de la fenêtre
//! de test `win32::cef_test_window` sur tous les popups migrés.
//!
//! Ce module a BRIÈVEMENT porté un `DARK_MODE` (copie de
//! `win32::theme::prefers_dark_mode`, lue par `win32::cef_popup::
//! push_menu_palette` pour choisir le texte des popups "menu") — retiré :
//! DEMANDE EXPLICITE ("j'ai un thème clair mais des éléments en noir dans
//! les menus, cela ne me plait pas") après coup. Le thème clair/sombre de
//! Windows N'EST PAS un bon signal pour ces popups : leur fond réel est un
//! flou teinté par `MENU_ACCENT` ci-dessous (une couleur choisie par
//! l'utilisateur, jamais liée au thème système), pas une carte
//! blanche/sombre "à la Windows" — voir `ui::color::menu_fg()`, doc, pour
//! le remplacement (deux pôles FIXES, plus aucune dépendance au thème).
//!
//! `menu_scale`/`menu_corner_radius` avaient le MÊME défaut, signalé
//! séparément ("le curseur Taille/arrondi des menus ne redimensionne
//! toujours aucun popup CEF") : lus seulement pour renvoyer leur valeur
//! courante au panneau de réglages (`win32::cef_settings::push_state`),
//! jamais pour dimensionner/arrondir réellement un popup. Désormais lus par
//! chaque module popup "menu" ci-dessus dans son propre `render_if_visible`
//! (rayon → `QuadInstance::new_backdrop`, voir `win32::cef_popup::render`)
//! et par `app::menu_rect`/`win32::cef_popup::menu_zoom_level` (taille
//! de fenêtre + zoom CEF, appliqués à l'OUVERTURE — voir `menu_scale`, doc
//! de champ, pour pourquoi son point neutre a dû changer de `2.0` à `1.0`
//! au passage).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

thread_local! {
    /// Facteur d'échelle GLOBAL de tous les popups CEF de type "menu" (voir
    /// `menu_scale`/`set_menu_scale`, et `app::menu_rect`/
    /// `win32::cef_popup::menu_zoom_level` pour la consommation réelle,
    /// ajoutée après coup — voir BUG RÉEL ci-dessous).
    ///
    /// Point neutre à `1.0` — PAS `2.0` comme à l'ère `egui` d'avant la
    /// Phase 7 (où `1.0` voulait dire "taille native", et `2.0` reprenait un
    /// doublement déjà appliqué manuellement). Depuis la migration CEF,
    /// TOUS les popups (menus compris) sont déjà zoomés ×3 en dur par
    /// `win32::cef_popup::CONTENT_ZOOM_FACTOR` — un nombre qui n'a plus le
    /// même sens que l'ancien `MENU_SCALE`. `1.0` ICI signifie désormais
    /// "cette taille ×3 déjà en place, telle quelle" : brancher ce curseur
    /// sur le zoom réel en gardant l'ancien défaut `2.0` aurait DOUBLÉ (×6
    /// au lieu de ×3) la taille de tous les menus dès le premier lancement
    /// suivant ce correctif, pour quiconque n'avait jamais touché ce
    /// réglage — BUG RÉEL SIGNALÉ ("le curseur Taille/arrondi des menus ne
    /// redimensionne toujours aucun popup CEF") : entre la Phase 7 et ce
    /// correctif, `menu_scale`/`menu_corner_radius` n'étaient lus QUE pour
    /// renvoyer leur valeur courante au panneau de réglages
    /// (`win32::cef_settings::push_state`), jamais pour dimensionner/
    /// arrondir réellement un popup — un curseur sans le moindre effet
    /// observable.
    static MENU_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// Rayon (px, à l'échelle `1.0`) des coins arrondis des menus/items —
    /// voir `win32::popup_menu`. Réglable indépendamment de `MENU_SCALE`
    /// (demande explicite distincte : "des bords arrondis dans un style
    /// web", pas seulement "plus grand") — `0.0` = angles droits.
    ///
    /// `None` tant que l'utilisateur n'a pas touché au curseur : le rayon suit
    /// alors `rayon_grand` du style actif (`ui::style`, 10 px pour
    /// « NeuraDesk », l'ancienne valeur fixe). Le curseur, lui, l'emporte sur
    /// le style.
    static MENU_CORNER_RADIUS: Cell<Option<f32>> = const { Cell::new(None) };
    /// Force du mélange entre le fond flouté brut et sa teinte (voir
    /// `menu_tint`/`set_menu_tint`) — même unité/même curseur que `config::
    /// load_dash_tint` (0.0 = flou pur, 1.0 = couleur pleine), mais une
    /// valeur INDÉPENDANTE dédiée aux menus contextuels (`win32::popup_menu`)
    /// : demande explicite ("je voudrais pouvoir régler la transparence de
    /// CES MENUS", distincts du Dash/des réglages/du presse-papiers, qui ont
    /// déjà leur propre curseur partagé). `0.55`, même valeur de repli que
    /// `dash_tint` avant personnalisation.
    static MENU_TINT: Cell<f32> = const { Cell::new(0.55) };
    /// Couleur de mise en avant des menus contextuels (fond teinté, voir
    /// `menu_tint`) — voir `menu_accent_rgb`/`set_menu_accent_rgb`. Valeur de
    /// repli purement technique (jamais montrée telle quelle en pratique,
    /// voir sa doc) : `app::run` l'écrase TOUJOURS au démarrage, soit avec la
    /// valeur persistée (voir `config::load_window_accent("popup")`), soit
    /// avec la couleur d'accentuation Windows actuelle si jamais personnalisée
    /// — pour ne rien changer au comportement d'origine (qui suivait déjà
    /// cette couleur système) tant que l'utilisateur n'a pas lui-même choisi
    /// une couleur dans le panneau de réglages.
    static MENU_ACCENT: Cell<[f32; 3]> = const { Cell::new([51.0 / 255.0, 153.0 / 255.0, 51.0 / 255.0]) };
    /// Couleur de fond INDIVIDUELLE par menu, quand elle a été personnalisée —
    /// demande explicite : « personnaliser les menus individuellement au niveau
    /// de la couleur ». Un menu ABSENT de cette table suit la couleur PARTAGÉE
    /// `MENU_ACCENT` (comportement d'avant ce réglage). Clés : voir
    /// `MENU_COLOR_KEYS`. Chargée une fois au démarrage (`load_menu_colors`),
    /// relue par `app::run` à chaque frame (en mémoire, aucun accès disque).
    static MENU_COLORS: RefCell<HashMap<&'static str, [f32; 3]>> = RefCell::new(HashMap::new());
}

/// Les menus contextuels réglables INDIVIDUELLEMENT en couleur. Chaque clé est
/// à la fois le nom de fichier de config (`{clé}_menu_color.txt`, voir
/// `config::load_menu_color`) et la cible côté page de réglages (`MENU_TARGETS`
/// dans `settings.html`). ⛔ `test_window` est EXCLU : fenêtre de débogage, pas
/// un élément de l'app.
pub const MENU_COLOR_KEYS: [&str; 9] = [
    "context", "calendar", "dossier_menu", "network", "sound", "voicemeeter", "spacer_size", "bar_size", "confirm",
];

/// Couleur de fond individuelle d'un menu si elle a été personnalisée, sinon
/// `None` — l'appelant (`app::run`) retombe alors sur `menu_accent_rgb`.
pub fn menu_color(key: &str) -> Option<[f32; 3]> {
    MENU_COLORS.with(|m| m.borrow().get(key).copied())
}

/// Pose la couleur individuelle d'un menu (cache mémoire + persistance), prise
/// en compte dès la frame suivante. `key` DOIT appartenir à `MENU_COLOR_KEYS`,
/// sinon l'appel est ignoré — pour ne jamais créer de fichier orphelin.
pub fn set_menu_color(key: &str, rgb: [f32; 3]) {
    let Some(canon) = MENU_COLOR_KEYS.iter().copied().find(|k| *k == key) else { return };
    MENU_COLORS.with(|m| {
        m.borrow_mut().insert(canon, rgb);
    });
    crate::config::save_menu_color(canon, rgb);
}

/// Retire la couleur individuelle d'un menu — il revient à la couleur partagée
/// des menus (`menu_accent_rgb`). Efface le cache ET le fichier.
pub fn clear_menu_color(key: &str) {
    let Some(canon) = MENU_COLOR_KEYS.iter().copied().find(|k| *k == key) else { return };
    MENU_COLORS.with(|m| {
        m.borrow_mut().remove(canon);
    });
    crate::config::delete_menu_color(canon);
}

/// Charge en mémoire toutes les couleurs de menu persistées — appelé une fois
/// par `app::run` au démarrage.
pub fn load_menu_colors() {
    for key in MENU_COLOR_KEYS {
        if let Some(rgb) = crate::config::load_menu_color(key) {
            MENU_COLORS.with(|m| {
                m.borrow_mut().insert(key, rgb);
            });
        }
    }
}

/// Force de teinte actuelle des menus contextuels — voir `MENU_TINT`.
///
/// "Visible dans les captures d'écran" (voir `gfx::capture_visibility`,
/// DEMANDE EXPLICITE) force une opacité de 100% tant qu'il est actif, SANS
/// modifier `MENU_TINT` lui-même — même principe que `gfx::blur::
/// blur_passes`, voir sa doc.
pub fn menu_tint() -> f32 {
    crate::gfx::capture_visibility::tint_alpha(MENU_TINT.with(Cell::get))
}

/// Change la force de teinte des menus contextuels, prise en compte dès la
/// prochaine frame (voir `win32::popup_menu::render_if_visible`) — appelé
/// par `win32::settings_dash` (curseur "Transparence" de la section "Menus
/// contextuels") et par `app::run` au démarrage (voir `config::
/// load_menu_tint`). Borné à [0.0, 1.0], même plage que `dash_tint`.
pub fn set_menu_tint(tint: f32) {
    MENU_TINT.with(|t| t.set(tint.clamp(0.0, 1.0)));
}

/// Couleur de mise en avant actuelle des menus contextuels — voir
/// `MENU_ACCENT`.
pub fn menu_accent_rgb() -> [f32; 3] {
    MENU_ACCENT.with(Cell::get)
}

/// Change la couleur de mise en avant des menus contextuels, prise en compte
/// dès la prochaine frame — appelé par `win32::settings_dash` (sélecteur de
/// couleur de la section "Menus contextuels") et par `app::run` au démarrage
/// (voir `config::load_window_accent("popup")`, ou la couleur d'accentuation
/// Windows actuelle si jamais personnalisée — voir `MENU_ACCENT`, doc du
/// champ).
pub fn set_menu_accent_rgb(rgb: [f32; 3]) {
    MENU_ACCENT.with(|c| c.set(rgb));
}

/// Facteur d'échelle actuel — voir `MENU_SCALE`.
pub fn menu_scale() -> f32 {
    MENU_SCALE.with(Cell::get)
}

/// Change le facteur d'échelle de tous les menus, pris en compte à la
/// PROCHAINE OUVERTURE d'un menu (voir `app::menu_rect`/
/// `win32::cef_popup::menu_zoom_level` — taille de fenêtre ET zoom CEF
/// figés à l'ouverture, contrairement à la teinte/couleur qui se relit à
/// chaque frame) — appelé par `win32::cef_settings` (curseur "Taille des
/// menus") et par `app::run` au démarrage pour reprendre la valeur
/// persistée (voir `config::load_menu_scale`). Borné à [0.5, 2.0] :
/// au-dessus, le plus grand des menus (le calendrier, déjà 960×1440 au
/// point neutre `1.0`, voir sa doc de champ) exploserait au-delà de ce
/// qu'un écran 1080p peut afficher — borne resserrée par rapport à l'ancien
/// [0.5, 3.0] pré-CEF (voir `MENU_SCALE`, doc de champ) précisément parce
/// que ce curseur multiplie désormais une taille déjà zoomée ×3, pas des
/// pixels natifs.
pub fn set_menu_scale(scale: f32) {
    MENU_SCALE.with(|s| s.set(scale.clamp(0.5, 2.0)));
}

/// Rayon de coin actuel (px, DÉJÀ à l'échelle — voir `menu_scale`) —
/// consommé chaque frame par le `render_if_visible` de chaque popup
/// "menu" (voir `MENU_ACCENT`, même relecture continue que la couleur/
/// teinte, pas seulement à l'ouverture comme `menu_scale` lui-même) pour le
/// `radius` de `QuadInstance::new_backdrop` (voir `win32::cef_popup::
/// render`). Voir `MENU_CORNER_RADIUS`.
pub fn menu_corner_radius() -> f32 {
    MENU_CORNER_RADIUS.with(Cell::get).unwrap_or_else(crate::ui::style::rayon_grand) * menu_scale()
}

/// Change le rayon de coin de base (px, avant mise à l'échelle) — appelé par
/// `win32::cef_settings` (curseur "Arrondi des menus") et `app::run` au
/// démarrage (voir `config::load_menu_corner_radius`). Borné à [0.0, 40.0] —
/// DEMANDE EXPLICITE ("le curseur de l'arrondi des menus ne va que jusqu'à
/// 24") : plafond relevé de 24 à 40 (voir `assets/popups/settings.html`,
/// même valeur `max` sur le curseur — les deux doivent rester synchronisés).
/// L'ancien plafond de 24 datait d'avant `win32::cef_popup::render` (voir sa
/// doc, section coins arrondis) qui détoure désormais VRAIMENT la fenêtre du
/// popup via `SetWindowRgn` plutôt que de compter sur le mélange alpha DWM :
/// un rayon plus élevé n'expose plus le bug de coins noirs qui motivait
/// implicitement une valeur prudente.
pub fn set_menu_corner_radius(radius: f32) {
    MENU_CORNER_RADIUS.with(|s| s.set(Some(radius.clamp(0.0, 40.0))));
}

/// Rend le rayon au style actif (`rayon_grand`) — appelé quand un style est
/// choisi : le curseur revient à la valeur du style (demande explicite).
pub fn reset_menu_corner_radius() {
    MENU_CORNER_RADIUS.with(|s| s.set(None));
}

/// Marge entre le bord de l'icône cliquée et le menu qui s'affiche à côté
/// (voir `win32::popup_menu::toggle`, appelé par `app::run`) — PAS mise à
/// l'échelle (voir `menu_scale`) : un simple décalage de positionnement, pas
/// une taille visuelle du menu lui-même.
pub const MENU_GAP: f32 = 4.0;

/// Une des QUATRE barres du dock (voir `ui::dock::bar_rects`) — sélecteurs de
/// couleur par barre dans `win32::settings_dash`.
///
/// ⚠️ **`Bottom` est en FIN**, et c'est le même rang que `ui::dock::BarreTirage`
/// et `ui::dock::bar_rank` : les trois indexent les mêmes tableaux par barre.
#[derive(Debug, Clone, Copy)]
pub enum Bar {
    Top,
    Left,
    Right,
    Bottom,
}
