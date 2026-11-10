//! Le dock : une forme en "⊓" (barre supérieure + barre latérale gauche +
//! barre latérale droite, chacune réglable INDÉPENDAMMENT — voir
//! `DockLayout` — depuis le panneau de réglages) façon launcher Unity, avec
//! un effet de magnification douce au survol (comme le
//! launcher Unity/Compiz sous Ubuntu). Les icônes sont alignées en haut de
//! chacune des deux barres latérales, juste sous la barre supérieure — leur
//! répartition entre les deux et leur ordre sont réglables par
//! glisser-déposer (voir `Side`, `Dock::left_order`/`right_order`,
//! `begin_potential_drag`/`update_drag`/`end_drag`), Ctrl maintenu pour
//! passer d'une barre à l'autre plutôt que simplement réordonner dans la
//! même barre. Ce module gère les icônes ; le fond des barres elles-mêmes
//! (arrière-plan flouté teinté, voir `gfx::backdrop`) est composé dans
//! `app::run`.

use super::zones::{index_of_key, key_of_index, BarLetter, BarSpacers, Orders, SorteEspace, ZoneKey, Zones};
use crate::gfx::QuadInstance;
use crate::ui::color;
use std::cell::{Cell, RefCell};
use windows::Win32::Foundation::HWND;
/// Bornes de la taille d'icône dérivée de l'épaisseur de barre (voir
/// `icon_size_for`) — évite des icônes illisibles au minimum du curseur
/// (`BAR_SIZE_MIN`, voir `win32::settings_dash`) ou disproportionnées à son
/// maximum.
const ICON_SIZE_MIN: f32 = 32.0;

/// Le plancher d'icône, pour `config_initiale` : une barre plus étroite que
/// `ICON_SIZE_MIN + 2 × marge` ne peut pas poser une icône sans qu'elle touche
/// ses bords (voir `config_initiale::elargir_les_barres`).
pub fn taille_minimale_d_icone() -> f32 {
    icone_minimale()
}
const ICON_SIZE_MAX: f32 = 96.0;
/// Marge conservée de chaque côté de l'icône dans l'épaisseur de sa barre,
/// avant mise à l'échelle (voir `icon_size_for`) — au réglage d'épaisseur de
/// barre par défaut (72px, calqué sur la barre des tâches Windows au premier
/// lancement), ceci reproduit exactement l'ancienne taille fixe (48px).
///
/// ⭐ **RÉGLABLE** (`set_icon_margin`) — DEMANDE EXPLICITE (2026-10-14) : « régler
/// la marge autour des icônes du dock ». Une valeur GLOBALE, comme l'épaisseur
/// qu'elle découpe : `icon_size_for` est une fonction libre, appelée par le dock,
/// la barre du haut et les écrans secondaires.
pub const DEFAULT_ICON_MARGIN: f32 = 12.0;
pub const MIN_ICON_MARGIN: f32 = 0.0;
pub const MAX_ICON_MARGIN: f32 = 40.0;

/// La marge courante, en bits d'`f32` (voir `icon_margin`).
static ICON_MARGIN_BITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x4140_0000); // 12.0

/// Marge conservée de chaque côté d'une icône dans l'épaisseur de sa barre.
/// Insère `identite` dans la liste de zones (ou de barre) qui cite `voisine` — juste après
/// elle si `decalage` vaut 1, juste avant s'il vaut 0. Rend `false` si aucune liste ne la cite.
fn inserer_pres_de(zones: &mut Zones, voisine: &str, identite: &str, decalage: usize) -> bool {
    let listes = zones.zones.values_mut().map(|record| &mut record.icones).chain(
        zones
            .bars
            .values_mut()
            .flat_map(|barre| [&mut barre.icones, &mut barre.icones_milieu, &mut barre.icones_droite]),
    );
    for liste in listes {
        if let Some(position) = liste.iter().position(|cite| cite == voisine) {
            liste.insert(position + decalage, identite.to_string());
            return true;
        }
    }
    false
}

pub fn icon_margin() -> f32 {
    f32::from_bits(ICON_MARGIN_BITS.load(std::sync::atomic::Ordering::Relaxed))
}

/// Pose la marge, bornée par `MIN_ICON_MARGIN`/`MAX_ICON_MARGIN`.
pub fn set_icon_margin(px: f32) {
    ICON_MARGIN_BITS.store(borner_marge(px).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

fn borner_marge(px: f32) -> f32 {
    if px.is_finite() { px.clamp(MIN_ICON_MARGIN, MAX_ICON_MARGIN) } else { DEFAULT_ICON_MARGIN }
}

/// ⭐ **LE FONDU EN HAUT DE LA BARRE DE GAUCHE** — DEMANDE EXPLICITE (2026-10-14) :
/// « un effet de flou entre la barre du haut et la barre de gauche sur une hauteur
/// réglable qui permette de faire disparaître les icônes du dock ». Sur les
/// `fondu_haut_gauche()` premiers pixels de la colonne de gauche, son CONTENU
/// (icônes, glyphes, indicateurs) se floute et s'efface jusqu'à disparaître contre
/// la barre du haut — le FOND des barres n'est pas touché (voir
/// `gfx::quad_renderer`, `FonduVisibilite`). 0 : aucun fondu.
///
/// ⭐ **LA BARRE DE DROITE A LE SIEN** — DEMANDE EXPLICITE (2026-10-14) : « fais
/// la même chose pour la barre de droite ». Une hauteur À ELLE
/// (`fondu_haut_droite`), même borne.
pub const MAX_FONDU_HAUT_GAUCHE: f32 = 800.0;

/// La hauteur du fondu, en bits d'`f32` (0.0 par défaut : désactivé).
static FONDU_HAUT_GAUCHE_BITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn fondu_haut_gauche() -> f32 {
    f32::from_bits(FONDU_HAUT_GAUCHE_BITS.load(std::sync::atomic::Ordering::Relaxed))
}

/// Pose la hauteur du fondu, bornée à `0..=MAX_FONDU_HAUT_GAUCHE`.
pub fn set_fondu_haut_gauche(px: f32) {
    FONDU_HAUT_GAUCHE_BITS.store(borner_fondu(px).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// La hauteur du fondu de la barre de DROITE, en bits d'`f32` (voir
/// `MAX_FONDU_HAUT_GAUCHE`).
static FONDU_HAUT_DROITE_BITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn fondu_haut_droite() -> f32 {
    f32::from_bits(FONDU_HAUT_DROITE_BITS.load(std::sync::atomic::Ordering::Relaxed))
}

pub fn set_fondu_haut_droite(px: f32) {
    FONDU_HAUT_DROITE_BITS.store(borner_fondu(px).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

fn borner_fondu(px: f32) -> f32 {
    if px.is_finite() { px.clamp(0.0, MAX_FONDU_HAUT_GAUCHE) } else { 0.0 }
}

/// ⭐ **L'OMBRE SOUS LA BARRE DU HAUT** — DEMANDE EXPLICITE (2026-10-15) : « un
/// petit dégradé noir en haut de la barre de gauche, de la barre de droite et des
/// 2 angles arrondis du haut de la zone centrale pour donner l'illusion que la
/// barre du haut est plus haute ». Sur `ombre_barre_haut()` pixels sous la barre du
/// haut, les fonds des barres s'assombrissent, de `ombre_barre_haut_force()`
/// contre la barre à rien en bas (voir `win32::ombre_barre`).
/// La zone centrale étant transparente, l'ombre épouse les deux colonnes et les
/// congés. C'est une OMBRE PORTÉE, pas un relief des barres (refusé trois fois).
pub const MAX_OMBRE_BARRE_HAUT: f32 = 200.0;
pub const DEFAULT_OMBRE_BARRE_HAUT: f32 = 24.0;
pub const DEFAULT_OMBRE_BARRE_HAUT_FORCE: f32 = 0.35;

static OMBRE_BARRE_HAUT_BITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(DEFAULT_OMBRE_BARRE_HAUT.to_bits());
static OMBRE_BARRE_HAUT_FORCE_BITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(DEFAULT_OMBRE_BARRE_HAUT_FORCE.to_bits());

/// La hauteur de l'ombre, en pixels — 0 : aucune ombre.
pub fn ombre_barre_haut() -> f32 {
    f32::from_bits(OMBRE_BARRE_HAUT_BITS.load(std::sync::atomic::Ordering::Relaxed))
}

pub fn set_ombre_barre_haut(px: f32) {
    OMBRE_BARRE_HAUT_BITS.store(borner_ombre(px).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// La force de l'ombre contre la barre, `0..=1` — 1 : noir.
pub fn ombre_barre_haut_force() -> f32 {
    f32::from_bits(OMBRE_BARRE_HAUT_FORCE_BITS.load(std::sync::atomic::Ordering::Relaxed))
}

pub fn set_ombre_barre_haut_force(force: f32) {
    OMBRE_BARRE_HAUT_FORCE_BITS.store(borner_force_ombre(force).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

fn borner_ombre(px: f32) -> f32 {
    if px.is_finite() { px.clamp(0.0, MAX_OMBRE_BARRE_HAUT) } else { DEFAULT_OMBRE_BARRE_HAUT }
}

fn borner_force_ombre(force: f32) -> f32 {
    if force.is_finite() { force.clamp(0.0, 1.0) } else { DEFAULT_OMBRE_BARRE_HAUT_FORCE }
}


/// La bande du fondu (`x, y, largeur, hauteur`, coordonnées fenêtre) : le HAUT de
/// la colonne de gauche, à partir de son bord supérieur — sous la barre du haut,
/// ou au bord de l'écran si elle est masquée. `None` : pas de fondu, ou pas de
/// colonne de gauche.
///
/// `[gauche, droite]` : une bande par colonne latérale.
pub fn bandes_de_fondu(layout: &DockLayout) -> [Option<[f32; 4]>; 2] {
    bandes_de_fondu_de(layout, [fondu_haut_gauche(), fondu_haut_droite()])
}

fn bandes_de_fondu_de(layout: &DockLayout, [gauche, droite]: [f32; 2]) -> [Option<[f32; 4]>; 2] {
    let bande = |x: f32, largeur: f32, hauteur: f32| {
        let hauteur = hauteur.min(layout.total_height - layout.haut_colonnes);
        (hauteur > 0.0 && largeur > 0.0).then_some([x, layout.haut_colonnes, largeur, hauteur])
    };
    [bande(0.0, layout.left_width, gauche), bande(layout.total_width - layout.right_width, layout.right_width, droite)]
}
/// Rayon des coins d'une icône — `rayon_icone` du style actif (`ui::style`,
/// 14 px pour « NeuraDesk »).
fn icon_radius() -> f32 {
    crate::ui::style::rayon_icone()
}

/// Taille d'une icône carrée du dock, dérivée de l'épaisseur de la barre qui
/// la contient (haut, gauche ou droite — chacune a sa propre épaisseur,
/// voir `DockLayout`) : suit désormais le réglage utilisateur (demande
/// explicite : "la taille des icones des barres ne s'adapte pas") plutôt
/// qu'une constante fixe. Partagée avec `ui::topbar`, dont toutes les
/// icônes vivent dans la barre du haut (`layout.top_thickness`).
pub fn icon_size_for(bar_thickness: f32) -> f32 {
    (bar_thickness - icon_margin() * 2.0).clamp(icone_minimale(), ICON_SIZE_MAX)
}

/// ⛔ **POUR LES DEUX BARRES LATÉRALES : LE PLANCHER NE DÉPASSE PAS LA PLACE QUE
/// LES MARGES LAISSENT** (mesure du 2026-10-29). Une barre de gauche convertie à
/// 29 px avec une marge de 9 laisse 11 px ; `ICON_SIZE_MIN = 32` les relevait à
/// 32, l'échelle ×1,5 portait à 48, et le plafond de la barre ramenait à 29 :
/// l'icône occupait TOUTE la barre, sans marges, là où l'écran d'origine en
/// laissait 61 %.
///
/// ⚠️ **NE S'APPLIQUE PAS AUX BARRES DU HAUT ET DU BAS**, qui ont leurs règles :
/// la vignette du bas rapetisse avec une marge SERRÉE (2 px, voir
/// `bottom_icon_size_for` — une barre de 18 px n'a aucune place pour les marges
/// ordinaires, et la lui imposer donnerait des icônes d'un pixel).
fn base_laterale(bar_width: f32, marge: f32) -> f32 {
    let place = (bar_width - marge * 2.0).max(1.0);
    let plancher = icone_minimale().min(place);
    place.clamp(plancher, ICON_SIZE_MAX)
}
/// Échelle des icônes de la barre du HAUT, appliquée à `icon_size_for` — DEMANDE
/// EXPLICITE (2026-10-14) : « régler la taille des icônes de la barre du haut ».
/// 100 % = la taille d'avant. Agrandie, une icône ne dépasse jamais l'épaisseur de
/// la barre (voir `taille_a_l_echelle`).
pub const DEFAULT_TOP_ICON_SCALE: f32 = 1.0;
pub const MIN_TOP_ICON_SCALE: f32 = 0.5;
pub const MAX_TOP_ICON_SCALE: f32 = 1.5;

/// L'échelle courante, en bits d'`f32` (voir `top_icon_scale`).
static TOP_ICON_SCALE_BITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3F80_0000); // 1.0

pub fn top_icon_scale() -> f32 {
    f32::from_bits(TOP_ICON_SCALE_BITS.load(std::sync::atomic::Ordering::Relaxed))
}

/// Pose l'échelle, bornée par `MIN_TOP_ICON_SCALE`/`MAX_TOP_ICON_SCALE`.
pub fn set_top_icon_scale(scale: f32) {
    TOP_ICON_SCALE_BITS.store(borner_echelle(scale).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Taille d'une icône de la barre du HAUT (applets, widgets, icônes épinglées dans
/// ses deux moitiés) : `icon_size_for` à l'échelle réglée.
pub fn top_icon_size_for(bar_thickness: f32) -> f32 {
    taille_a_l_echelle(icon_size_for(bar_thickness), bar_thickness, top_icon_scale())
}

/// Échelle des icônes de la barre du BAS — DEMANDE EXPLICITE (2026-10-14) : « changer
/// la taille des icônes de la barre du bas ». Même règle que celle du haut
/// (`taille_a_l_echelle`), réglage distinct.
pub const DEFAULT_BOTTOM_ICON_SCALE: f32 = 1.0;
/// ⚠️ **SA BORNE HAUTE EST LE TRIPLE DE CELLE DU HAUT** — DEMANDE EXPLICITE
/// (2026-10-23) : « tripler taille max ». Agrandie, une vignette s'arrête toujours
/// à l'épaisseur de la barre : au-delà de 150 %, c'est sur une barre ÉPAISSE que
/// le gain se voit.
pub const MAX_BOTTOM_ICON_SCALE: f32 = MAX_TOP_ICON_SCALE * 3.0;

static BOTTOM_ICON_SCALE_BITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3F80_0000); // 1.0

pub fn bottom_icon_scale() -> f32 {
    f32::from_bits(BOTTOM_ICON_SCALE_BITS.load(std::sync::atomic::Ordering::Relaxed))
}

/// Pose l'échelle, bornée par `MIN_TOP_ICON_SCALE`/`MAX_BOTTOM_ICON_SCALE`.
pub fn set_bottom_icon_scale(scale: f32) {
    BOTTOM_ICON_SCALE_BITS.store(borner_echelle_bas(scale).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

fn borner_echelle_bas(scale: f32) -> f32 {
    if scale.is_finite() { scale.clamp(MIN_TOP_ICON_SCALE, MAX_BOTTOM_ICON_SCALE) } else { DEFAULT_BOTTOM_ICON_SCALE }
}

/// Échelle des icônes de la barre de GAUCHE — DEMANDE EXPLICITE (2026-10-18) : « un
/// réglage pour régler la taille des icônes de la barre de gauche ». Mêmes bornes et
/// même règle que celles du haut et du bas : agrandie, une icône s'arrête à la
/// largeur de la barre.
pub const DEFAULT_LEFT_ICON_SCALE: f32 = 1.0;

static LEFT_ICON_SCALE_BITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3F80_0000); // 1.0

pub fn left_icon_scale() -> f32 {
    f32::from_bits(LEFT_ICON_SCALE_BITS.load(std::sync::atomic::Ordering::Relaxed))
}

/// Pose l'échelle, bornée par `MIN_TOP_ICON_SCALE`/`MAX_TOP_ICON_SCALE`.
pub fn set_left_icon_scale(scale: f32) {
    LEFT_ICON_SCALE_BITS.store(borner_echelle(scale).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Taille d'une icône d'une barre LATÉRALE de largeur `bar_width` : celle de gauche
/// suit son échelle (`left_icon_scale`), celle de droite garde `icon_size_for`.
pub fn lateral_icon_size_for(side: Side, bar_width: f32) -> f32 {
    taille_laterale(side, bar_width, left_icon_scale())
}

/// Pure, comme `taille_a_l_echelle` : testable sans toucher à l'échelle globale,
/// que les autres tests lisent en parallèle.
fn taille_laterale(side: Side, bar_width: f32, echelle_gauche: f32) -> f32 {
    let base = base_laterale(bar_width, icon_margin());
    match side {
        Side::Left => taille_a_l_echelle(base, bar_width, echelle_gauche),
        _ => base,
    }
}

fn borner_echelle(scale: f32) -> f32 {
    if scale.is_finite() { scale.clamp(MIN_TOP_ICON_SCALE, MAX_TOP_ICON_SCALE) } else { DEFAULT_TOP_ICON_SCALE }
}

/// Pure, donc testable sans toucher à l'échelle globale. Agrandie, l'icône s'arrête
/// à l'épaisseur de la barre — mais jamais sous la taille de base, qui peut déjà la
/// dépasser (`ICON_SIZE_MIN` sur une barre fine).
fn taille_a_l_echelle(base: f32, bar_thickness: f32, scale: f32) -> f32 {
    (base * scale).min(base.max(bar_thickness))
}

/// Épaisseur/largeur MINIMALE d'une barre **SUR UN ÉCRAN 3840×2160**, par barre :
/// `[haut, gauche, droite, bas]`. DEMANDE EXPLICITE (2026-11-02) : « les tailles
/// minimales pour un écran en 3840 x 2160, haut 28, bas 28, gauche 40, droite 40,
/// tailles minimales à bloquer dans le code ». Elles remplacent 16 (haut) et 18
/// (bas). Sur un autre écran, elles suivent sa hauteur — voir
/// `bar_thickness_min_pour`. ⚠️ Sous 72 px, une VIGNETTE de dossier reste à sa
/// borne basse (`ICON_SIZE_MIN`) et rapetisse avec la barre du bas.
pub const BAR_THICKNESS_MIN_4K: [f32; 4] = [28.0, 40.0, 40.0, 28.0];

/// La hauteur d'écran, en pixels PHYSIQUES, pour laquelle `BAR_THICKNESS_MIN_4K`
/// est donné — 3840×2160.
pub const HAUTEUR_DE_REFERENCE: f32 = 2160.0;

/// **LE MINIMUM D'UNE BARRE SUR UN ÉCRAN DE `hauteur_ecran` PX** — celui du 4K,
/// rapporté à la hauteur (même règle que la conversion du portable,
/// `config_initiale::Facteurs::physique`) : 28 px sur un 4K, 14 sur un 1080p.
/// Arrondi, et jamais sous 1. Une hauteur absurde (≤ 0, non finie) rend le
/// minimum du 4K. Hors bornes : celui de la DROITE.
pub fn bar_thickness_min_pour(index: usize, hauteur_ecran: f32) -> f32 {
    let base = BAR_THICKNESS_MIN_4K.get(index).copied().unwrap_or(BAR_THICKNESS_MIN_4K[2]);
    if !(hauteur_ecran.is_finite() && hauteur_ecran > 0.0) {
        return base;
    }
    (base * hauteur_ecran / HAUTEUR_DE_REFERENCE).round().max(1.0)
}

/// La hauteur PHYSIQUE de l'écran principal, celui des barres — la référence si
/// Windows ne la dit pas.
fn hauteur_ecran_principal() -> f32 {
    crate::config_initiale::Ecran::principal().map_or(HAUTEUR_DE_REFERENCE, |e| e.hauteur as f32)
}

/// Les minimums des quatre barres sur l'écran principal, `[haut, gauche,
/// droite, bas]` — ce que les curseurs reçoivent.
pub fn bar_thickness_mins() -> [f32; 4] {
    let hauteur = hauteur_ecran_principal();
    std::array::from_fn(|i| bar_thickness_min_pour(i, hauteur))
}

/// **LE VERROU** : toute épaisseur qui entre dans l'app (fichier relu, curseur
/// des Réglages, curseur du clic droit) passe par ici. Non finie : le minimum.
pub fn borner_epaisseur(index: usize, valeur: f32) -> f32 {
    let (min, max) = bar_thickness_bounds(index);
    if valeur.is_finite() { valeur.clamp(min, max) } else { min }
}

/// Épaisseur/largeur MAXIMALE d'une barre, la même pour les quatre — DEMANDE
/// EXPLICITE : « il faudrait que l'on puisse aller à 250 dans les 2 cas ».
pub const BAR_THICKNESS_MAX: f32 = 250.0;

/// Taille de la VIGNETTE d'un emplacement de la barre du BAS.
///
/// ⚠️ **SOUS `ICON_SIZE_MIN` + 2 × `BOTTOM_TIGHT_MARGIN`, ELLE RAPETISSE AVEC LA
/// BARRE** : la barre du bas descend à 28 px sur un 4K (`BAR_THICKNESS_MIN_4K`), et une vignette
/// gardée à 32 px, centrée, débordait alors de 7 px en haut ET sous le bord de
/// l'écran — mesuré à la capture, le 2026-10-12. Au-dessus, rien ne change.
///
/// Puis l'échelle réglée (`bottom_icon_scale`), avec la même règle que la barre du
/// haut : agrandie, une vignette s'arrête à l'épaisseur de la barre.
pub fn bottom_icon_size_for(bar_thickness: f32) -> f32 {
    let base = icon_size_for(bar_thickness).min((bar_thickness - BOTTOM_TIGHT_MARGIN * 2.0).max(1.0));
    taille_a_l_echelle(base, bar_thickness, bottom_icon_scale())
}

/// Marge minimale au-dessus et au-dessous d'une vignette de la barre du bas, quand
/// la barre est trop fine pour les marges ordinaires (`icon_margin`).
const BOTTOM_TIGHT_MARGIN: f32 = 2.0;

/// Épaisseur par DÉFAUT de la barre du bas, pour un `bar_thicknesses.txt` écrit
/// avant qu'elle n'existe (voir `config::load_bar_thicknesses`) : une vignette de
/// 64 px et les deux marges, soit `64 + 24`. **La bande de nom (16 px) a été
/// retirée** avec le texte sous les vignettes — c'est ce qui reste.
pub const DEFAULT_BOTTOM_THICKNESS: f32 = 104.0;

/// **LA SEULE AUTORITÉ sur ces deux bornes**, dans l'ordre `[haut, gauche,
/// droite]` — le curseur du clic droit sur une barre (`win32::cef_bar_size`) et
/// celui des Réglages (`win32::cef_settings`) la reçoivent tous les deux par
/// leur ÉTAT POUSSÉ, jamais d'un littéral recopié dans leur page.
///
/// **C'est exactement ainsi que l'écart est né** : les deux pages portaient
/// chacune son littéral, 160 d'un côté et 500 de l'autre, pour la même
/// grandeur, sans que rien ne le signale — il a fallu que l'utilisateur bute
/// dessus. Un commentaire dans chaque page ne pouvait pas l'empêcher ; une
/// seule définition le peut.
///
/// Hors bornes : rend les bornes de la DROITE, comme le repli de `Side` — un
/// appelant qui se trompe d'index obtient une barre réglable plutôt qu'un
/// panic.
pub fn bar_thickness_bounds(index: usize) -> (f32, f32) {
    (bar_thickness_min_pour(index, hauteur_ecran_principal()), BAR_THICKNESS_MAX)
}

/// Écartement PAR DÉFAUT entre deux icônes consécutives (voir `Dock::
/// icon_gap`, réglable dans le panneau de réglages depuis ce point de
/// départ) — figé en constante avant ce réglage.
pub const DEFAULT_ICON_GAP: f32 = 12.0;
const TOP_MARGIN: f32 = 16.0;
/// Marge entre le coin de l'ÉCRAN (PAS le coin de la barre de gauche/
/// droite : voir `layout_zone`, `drag_insert_index`, les deux anchorent
/// SYMÉTRIQUEMENT à `layout.total_width - TOP_ZONE_CORNER_MARGIN`/
/// `TOP_ZONE_CORNER_MARGIN` tout court, jamais `layout.left_width`/
/// `right_width` en plus) et la première icône du dock déposée dans la
/// zone `Side::TopLeft`/`TopRight` de la barre du haut — MÊME valeur que
/// `ui::topbar::DESKTOP_DOTS_MARGIN_LEFT` (demande explicite : "pouvoir
/// déplacer les icônes des barres de gauche et de droite dans la barre
/// supérieure... [selon] le côté... où elles sont déposées"). BUG RÉEL
/// SIGNALÉ ("je n'arrive pas à placer d'icone sur le bord de l'écran à
/// gauche... il y a un espace invisible") : une première version ajoutait
/// `layout.left_width` à l'ancre de `TopLeft` (~96px de marge morte avec
/// une barre de gauche à 80px) alors que `TopRight` restait déjà flush au
/// bord (juste cette marge, sans `right_width`) — la barre gauche/droite ne
/// s'étend de toute façon JAMAIS au-dessus de `layout.top_thickness` (voir
/// `layout_side`, `start_y`), rien à éviter dans ce coin. Ces icônes
/// deviennent le nouvel élément le plus proche de CHAQUE coin, les
/// pastilles de bureaux virtuels/contrôles média (à gauche) et le groupe
/// alimentation/son/réseau (à droite, désormais lui aussi composé d'icônes
/// du dock, voir `app::run`) se décalant d'autant pour leur laisser la
/// place (voir `Dock::top_zone_widths`).
const TOP_ZONE_CORNER_MARGIN: f32 = 16.0;

/// Facteur de magnification par DÉFAUT d'une icône survolée — l'ancienne
/// constante `HOVER_SCALE`, désormais réglable par curseur (voir
/// `hover_scale`). DEMANDE EXPLICITE : « magnification au survol réglable par
/// curseur ».
pub const DEFAULT_HOVER_SCALE: f32 = 1.30;
/// **LE TIRAGE VERS LE CENTRE DES ICÔNES AGRANDIES** — 0.0 : l'icône grossie reste
/// CENTRÉE sur sa case, quitte à être coupée par le bord ; 1.0 : elle est ramenée
/// entièrement dans la fenêtre, donc décalée vers le centre ; entre les deux, elle
/// l'est à proportion.
///
/// DEMANDE EXPLICITE du 2026-09-24, **en trois temps** : « les icones ... qui sont au
/// bord de l'écran se décalent à gauche, peux-tu enlever ce déplacement » — puis,
/// après l'avoir retiré — « je veux que le décalage des icônes du dock se décalent et
/// que ce soit réglable ».
///
/// ⚠️ **LE DÉFAUT EST 1.0, LE COMPORTEMENT HISTORIQUE** : c'est ce que l'application
/// faisait avant ce jour, parce que le décalage était une DEMANDE (« il faudrait que
/// les icones magnifiées se déplacent un peu vers le centre de l'écran pour ne pas
/// être tronquées »). Le régler à 0 rend la troncature — c'est un choix, pas un oubli.
pub const DEFAULT_TIRAGE: f32 = 1.0;

/// **LA BARRE À LAQUELLE UN TIRAGE S'APPLIQUE** — quatre clés, et non les cinq
/// positions de `Side` : DEMANDE EXPLICITE du 2026-09-24, « le décalage classique
/// soit réglable pour le dock et les autres barres individuellement » — donc par
/// TYPE de barre, pas par coin.
///
/// ⚠️ **`Bas` est en FIN, et son rang vaut 3** : `config` range ses valeurs dans
/// l'ordre de `TOUTES`, et les fichiers déjà écrits n'en portent que trois. Une
/// valeur ajoutée en fin se relit donc sans réécrire les anciens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BarreTirage {
    Haut,
    Gauche,
    Droite,
    Bas,
}

impl BarreTirage {
    pub const TOUTES: [BarreTirage; 4] =
        [BarreTirage::Haut, BarreTirage::Gauche, BarreTirage::Droite, BarreTirage::Bas];

    /// Le RANG dans le tableau des tirages — public parce que `config` range
    /// ses valeurs dans cet ordre.
    pub fn rang(self) -> usize {
        match self {
            BarreTirage::Haut => 0,
            BarreTirage::Gauche => 1,
            BarreTirage::Droite => 2,
            BarreTirage::Bas => 3,
        }
    }

    /// **DE QUELLE BARRE RELÈVE UNE ICÔNE.** ⚠️ **ON NE LA DÉDUIT PAS DE SON
    /// RECTANGLE** (voir `icon_side`, sa doc) : tout ce qui n'est ni à gauche ni à
    /// droite ni en bas est « en haut » — la barre du haut, et ses coins.
    pub fn de_la_side(side: Side) -> Self {
        match side {
            Side::Left => BarreTirage::Gauche,
            Side::Right => BarreTirage::Droite,
            Side::Bottom => BarreTirage::Bas,
            _ => BarreTirage::Haut,
        }
    }
}
/// Borne BASSE du curseur « Magnification au survol » des Réglages : `1.0`
/// (aucun effet) et non une valeur inférieure, parce que sous `1.0` le geste
/// s'INVERSE — l'icône survolée RÉTRÉCIT — ce qui se lit comme un défaut sur
/// un réglage qui annonce une magnification.
pub const MIN_HOVER_SCALE: f32 = 1.00;
/// Borne HAUTE du même curseur : au-delà, une icône à `ICON_SIZE_MAX` (96px)
/// atteindrait 240px dans une barre de 72px, débordant à la fois des barres
/// latérales et de la barre du haut.
pub const MAX_HOVER_SCALE: f32 = 2.50;
/// Vitesse d'approche par DÉFAUT de l'animation (plus haut = plus réactif) —
/// l'ancienne constante `EASE_SPEED`, désormais réglable (voir
/// `hover_ease_speed`).
pub const DEFAULT_HOVER_EASE_SPEED: f32 = 14.0;
/// Borne BASSE du curseur « Vitesse d'approche » : garde l'animation
/// perceptible (à `0` elle n'aurait plus lieu du tout, l'échelle ne
/// bougerait jamais).
pub const MIN_HOVER_EASE_SPEED: f32 = 2.0;
/// Borne HAUTE du même curseur : rend l'approche quasi instantanée sans
/// supprimer l'animation (un saut sec d'un coup).
pub const MAX_HOVER_EASE_SPEED: f32 = 60.0;

// --- Magnification des HORLOGES ------------------------------------------
// Réglage DISTINCT de celui des icônes (voir `CLOCK_HOVER_SCALE`) — DEMANDE
// EXPLICITE : « pouvoir régler sa taille agrandie ». Le défaut est celui des
// icônes, donc le premier lancement ne change rien à ce qui existait avant que
// ce curseur n'existe.

/// Facteur de magnification par DÉFAUT du texte des deux horloges (barre du
/// haut et mini-barre plein écran) — voir `clock_hover_scale`.
pub const DEFAULT_CLOCK_HOVER_SCALE: f32 = 1.30;
/// Borne BASSE du curseur « Magnification des horloges » : même argument
/// d'inversion que `MIN_HOVER_SCALE` (sous `1.0`, le texte RÉTRÉCIT au survol).
pub const MIN_CLOCK_HOVER_SCALE: f32 = 1.00;
/// Borne HAUTE du même curseur, plus large que `MAX_HOVER_SCALE` : la limite
/// des icônes (`ICON_SIZE_MAX` dans une barre de 72px) ne concerne pas un texte,
/// dont la place est réservée à part (voir `max_clock_magnified_growth`).
pub const MAX_CLOCK_HOVER_SCALE: f32 = 3.50;

thread_local! {
    /// Facteur de magnification GLOBAL des ICÔNES, partagé par les instances de
    /// `HoverAnimator` qui les animent — celle de `Dock` et celle des trois
    /// boutons média de `app::run` (voir `HoverAnimator`, doc de type). Global
    /// plutôt que porté par le `Dock` : la magnification était identique partout
    /// du temps où c'était une constante, et c'est le seul choix qui garde les
    /// deux instances synchrones SANS synchronisation par frame — `app::run` ne
    /// peut pas les atteindre toutes les deux (`SettingsRefs` ne porte que
    /// `&mut Dock`, voir `win32::cef_settings`). Même motif que
    /// `win32::menu::MENU_SCALE`, l'autre réglage global d'apparence.
    ///
    /// Le texte des horloges a la SIENNE (`CLOCK_HOVER_SCALE` ci-dessous), et
    /// son propre animateur : c'est ce qui permet à `update_scaled` de servir
    /// une cible par appel plutôt que de figer la cible dans la structure.
    static HOVER_SCALE: Cell<f32> = const { Cell::new(DEFAULT_HOVER_SCALE) };
    /// Le TIRAGE des icônes agrandies — même portée et même raison que
    /// `HOVER_SCALE` ci-dessus : c'est un réglage d'apparence GLOBAL, lu par le
    /// dessin, le clic, l'ancrage et la région, et `app::run` ne peut pas atteindre
    /// toutes les instances de dock pour le leur transmettre.
    /// Les QUATRE tirages — un par TYPE de barre (voir `BarreTirage`), indexés par
    /// `BarreTirage::rang()`. Même portée et même raison que `HOVER_SCALE` : un
    /// réglage d'apparence GLOBAL, lu par le dessin, le clic, l'ancrage et la région.
    ///
    /// ⚠️ **L'ARITÉ DOIT SUIVRE `BarreTirage::TOUTES`** : `rang()` y indexe
    /// directement, donc un cinquième type de barre sans une cinquième case fait
    /// paniquer à la PREMIÈRE lecture — pas au premier réglage. C'est ce que le
    /// test `les_tirages_des_trois_barres_sont_independants` a attrapé.
    static TIRAGES: Cell<[f32; 4]> = const { Cell::new([DEFAULT_TIRAGE; 4]) };
    /// Vitesse d'approche GLOBALE — même portée, même raison que `HOVER_SCALE`
    /// ci-dessus.
    static HOVER_EASE_SPEED: Cell<f32> = const { Cell::new(DEFAULT_HOVER_EASE_SPEED) };
    /// Facteur de magnification GLOBAL du TEXTE des horloges — un réglage à
    /// part entière, distinct de `HOVER_SCALE` ci-dessus, mais GLOBAL pour
    /// exactement la même raison : les deux horloges vivent dans des fenêtres
    /// différentes (`win32::fullscreen_clock` en a même une par écran) et
    /// `app::run` ne peut atteindre ni l'une ni l'autre directement. Une
    /// cellule lue à chaque `update` les fait toutes suivre à chaud, sans
    /// synchronisation.
    ///
    /// La VITESSE d'approche, elle, reste partagée avec les icônes
    /// (`HOVER_EASE_SPEED`) : la demande ne portait que sur la taille.
    static CLOCK_HOVER_SCALE: Cell<f32> = const { Cell::new(DEFAULT_CLOCK_HOVER_SCALE) };
}

/// Facteur de magnification au survol actuel — voir `HOVER_SCALE`.
/// Bornes du réglage de TAILLE DU NOM DES ZONES (voir `zone_label_font_px`) —
/// DEMANDE EXPLICITE : « des curseurs pour régler la taille du nom des zones ».
///
/// Le défaut d'origine (14 px) est le PLANCHER : il s'est révélé illisible sur
/// l'écran 4K de cette machine, où 14 px physiques ne font presque rien.
pub const MIN_ZONE_LABEL_FONT_PX: u16 = 10;
pub const MAX_ZONE_LABEL_FONT_PX: u16 = 48;
pub const DEFAULT_ZONE_LABEL_FONT_PX: u16 = 22;

thread_local! {
    /// Taille du texte des étiquettes de zone, en pixels — **GLOBALE**, comme
    /// `hover_scale`/`clock_hover_scale` et pour la même raison : elle est lue
    /// au DESSIN (`app::run`) et par le panneau Réglages, qui ne partagent
    /// aucun état. Une taille PAR ZONE demanderait de la ranger dans un champ
    /// d'`Icon`, qui doit rester `Copy` (voir `Dock::zone_names`, doc).
    static ZONE_LABEL_FONT_PX: Cell<u16> = const { Cell::new(DEFAULT_ZONE_LABEL_FONT_PX) };
}

/// Taille actuelle du texte des étiquettes de zone — voir `ZONE_LABEL_FONT_PX`.
pub fn zone_label_font_px() -> u16 {
    ZONE_LABEL_FONT_PX.with(Cell::get)
}

/// Fixe la taille du texte des étiquettes, bornée — valeur illisible (NaN ✗) ou
/// hors bornes ramenée dans la plage plutôt que refusée : l'appelant est un
/// curseur, dont la position EST la valeur.
pub fn set_zone_label_font_px(px: u16) {
    ZONE_LABEL_FONT_PX.with(|c| c.set(px.clamp(MIN_ZONE_LABEL_FONT_PX, MAX_ZONE_LABEL_FONT_PX)));
}

/// **Marge de case des icônes de zone, RÉGLABLE** — voir
/// `Dock::DEFAULT_ZONE_CELL_MARGIN` pour la raison d'être et le défaut.
///
/// Rangée dans une cellule globale, exactement comme `ZONE_LABEL_FONT_PX`
/// ci-dessus et pour la même raison : elle est lue au DESSIN (`app::run`) et
/// écrite par le panneau Réglages, qui ne partagent aucun état — et une marge
/// PAR ZONE demanderait de la ranger dans un champ d'`Icon`, qui doit rester
/// `Copy` (voir `Dock::zone_names`, doc).
thread_local! {
    static ZONE_CELL_MARGIN: Cell<f32> = const { Cell::new(Dock::DEFAULT_ZONE_CELL_MARGIN) };
}

/// Marge actuelle — voir `ZONE_CELL_MARGIN`.
pub fn zone_cell_margin() -> f32 {
    ZONE_CELL_MARGIN.with(Cell::get)
}

/// Fixe la marge, bornée — une valeur négative ferait DÉBORDER l'icône de sa
/// case, et au-delà de `MAX_ZONE_CELL_MARGIN` elle la ferait disparaître (le
/// plancher de 1 px de `layout_side` la garde dessinée, mais illisible).
pub fn set_zone_cell_margin(px: f32) {
    let clamped = if px.is_finite() { px.clamp(MIN_ZONE_CELL_MARGIN, MAX_ZONE_CELL_MARGIN) } else { Dock::DEFAULT_ZONE_CELL_MARGIN };
    ZONE_CELL_MARGIN.with(|c| c.set(clamped));
}

/// Bornes de la marge de case — `0` doit rester permis : c'est le retour au
/// comportement d'origine (des icônes qui remplissent leur case).
pub const MIN_ZONE_CELL_MARGIN: f32 = 0.0;
pub const MAX_ZONE_CELL_MARGIN: f32 = 24.0;

/// Ce que le popup de réglage d'un espace doit PRÉSENTER — `(blanc, cote)`,
/// chacun `None` quand sa ligne ne doit pas être proposée (voir
/// `win32::cef_spacer_size`, qui les reçoit en `Option`).
///
/// **SEULE définition de cette règle, et PURE** — c'est ce qui la rend testable,
/// et elle méritait de l'être : les deux grandeurs sont OPPOSÉES (un espace
/// ordinaire a un blanc, un séparateur de zone a une cote), et les écrire l'une
/// sur le modèle de l'autre les inverse en silence — les deux sont des
/// `Option<f32>`, donc le compilateur ne dit rien. DÉFAUT RÉEL SIGNALÉ, causé par
/// exactement cette inversion : « le menu qui s'ouvre par le clic droit sur un
/// séparateur affiche le texte séparateur mais pas de curseur de réglage » — le
/// popup s'ouvrait avec sa seule rangée de blanc ANNULÉE, donc vide.
pub fn spacer_popup_values(is_zone_separator: bool, spacer_size: f32, zone_height: f32) -> (Option<f32>, Option<f32>) {
    if is_zone_separator {
        (None, Some(zone_height))
    } else {
        (Some(spacer_size), None)
    }
}

/// Taille de police EFFECTIVE d'un libellé de zone pour qu'il TIENNE dans
/// `available_px` — DEMANDE EXPLICITE : « j'aimerais que les titres des zones de
/// droite réduisent automatiquement leur taille si le texte ne rentre pas dans
/// la largeur de la barre ».
///
/// **La taille demandée (le curseur) est un MAXIMUM**, jamais un minimum : on
/// redescend d'un pixel à la fois jusqu'à ce que la largeur mesurée tienne, et
/// jamais sous `MIN_ZONE_LABEL_FONT_PX` — le plancher de lisibilité que le
/// curseur lui-même respecte. Un libellé qui ne tient toujours pas à cette taille
/// est rendu tel quel et DÉCOUPÉ à la barre, comme avant : mieux vaut un texte
/// tronqué qu'un texte illisible.
///
/// `width_at` rend la largeur mesurée à une taille donnée (voir
/// `gfx::text::measure_text`) — passée en paramètre pour que cette décision soit
/// PURE : elle se teste sans écran ni GDI, et la mesure employée reste celle du
/// dessin.
pub fn zone_label_fit_px(requested: u16, available_px: f32, width_at: impl Fn(u16) -> f32) -> u16 {
    let requested = requested.clamp(MIN_ZONE_LABEL_FONT_PX, MAX_ZONE_LABEL_FONT_PX);
    // Une barre sans largeur (fenêtre pas encore dimensionnée, ou boîte
    // dégénérée) ne doit pas faire disparaître le texte : on rend la demande.
    if !(available_px.is_finite() && available_px > 0.0) {
        return requested;
    }
    let mut px = requested;
    while px > MIN_ZONE_LABEL_FONT_PX {
        if width_at(px) <= available_px {
            return px;
        }
        px -= 1;
    }
    MIN_ZONE_LABEL_FONT_PX
}

/// **Position du TEXTE d'une zone : centré sur son SÉPARATEUR.**
///
/// DEMANDE EXPLICITE : « les textes des zones de droite apparaissent centrés sur
/// les séparateurs de zone ». Le texte était auparavant posé sous le séparateur,
/// à distance fixe (`ZONE_LABEL_MARGIN`, retirée depuis) — un en-tête de zone ;
/// il est maintenant centré sur lui, donc à cheval.
///
/// **`marker` EST L'EMPLACEMENT DU SÉPARATEUR, PAS LA BOÎTE DE SA ZONE** —
/// `Dock::icon_rect(sep, layout)`, la même source que le dessin et le clic (voir
/// `icon_rects_map`, « la seule source de vérité de ce qui est à l'écran »).
///
/// ⚠️ **CONFONDRE LES DEUX EST L'ERREUR QUI A ÉTÉ COMMISE ICI, ET ELLE EST
/// SILENCIEUSE.** `zone_boxes` rend la boîte de la ZONE, dont le `top` se trouve
/// déjà `SPACER_HEIGHT + SPACER_GAP` (24 px) sous le sommet de l'emplacement du
/// séparateur — c'est écrit noir sur blanc dans `bar_layout`, et c'est ce que
/// vérifie `a_zones_box_starts_just_under_its_separators_circle`. Le premier jet
/// de cette fonction a centré le texte sur `zone_boxes` : le texte restait donc
/// SOUS la pastille, à 24 px près, c'est-à-dire exactement le défaut que la
/// demande visait à corriger. D'où le test qui confronte les DEUX sources au lieu
/// de refaire le calcul à la main.
///
/// ⚠️ **LE TEXTE RECOUVRE LE PETIT CERCLE DU SÉPARATEUR, ET C'EST CHOISI.**
/// Le marqueur est un cercle de `separator_size` (14 px par défaut, jusqu'à 18)
/// centré dans ce même emplacement : centrer l'étiquette dessus la centre donc
/// exactement sur le cercle, et un texte de 22 px le masque entièrement. Trois
/// placements ont été proposés — recouvrir le cercle, remplacer le cercle, ou
/// rester dessous sans marge — et c'est celui-ci qui a été explicitement retenu.
/// Le cercle perd donc son rôle de repère là où une zone porte une étiquette
/// visible ; ce n'est pas un oubli à « corriger ».
///
/// Fonction PURE et testable, comme `zone_label_fit_px` : c'est de la géométrie,
/// et rien ne la rattrape si elle se trompe — un texte décalé ne se plaint pas.
pub(crate) fn zone_label_origin(marker: [f32; 4], text_w: f32, text_h: f32) -> [f32; 2] {
    [
        // Centré dans la LARGEUR de l'emplacement, et non sur le cercle : le
        // cercle est lui-même centré dans son emplacement (voir
        // `spacer_marker_instance`), donc les deux coïncident — et centrer sur
        // l'emplacement reste juste même si le marqueur change de côté.
        marker[0] + (marker[2] - text_w) * 0.5,
        marker[1] + (marker[3] - text_h) * 0.5,
    ]
}

/// Voir `DEFAULT_TIRAGE`, doc de la constante — 0.0 centrée, 1.0 ramenée.
pub fn tirage_vers_le_centre(barre: BarreTirage) -> f32 {
    TIRAGES.with(|t| t.get()[barre.rang()])
}

/// Le BORNAGE est fait ici, comme pour `set_hover_scale` : l'appelant écrit ce que
/// le panneau lui donne, et c'est ce module qui décide de ce qui est dessinable.
pub fn set_tirage_vers_le_centre(barre: BarreTirage, tirage: f32) {
    TIRAGES.with(|t| {
        let mut valeurs = t.get();
        valeurs[barre.rang()] = tirage.clamp(0.0, 1.0);
        t.set(valeurs);
    });
}

pub fn hover_scale() -> f32 {
    HOVER_SCALE.with(Cell::get)
}

/// Change le facteur de magnification au survol, borné à
/// [`MIN_HOVER_SCALE`, `MAX_HOVER_SCALE`] — le BORNAGE est fait ici, comme
/// `win32::menu::set_menu_scale`, pour qu'un fichier de configuration corrompu
/// ne puisse jamais atteindre l'animation. Pris en compte dès la PROCHAINE
/// frame : `HoverAnimator::update` relit la valeur à chaque appel.
pub fn set_hover_scale(scale: f32) {
    // `f32::clamp` PROPAGE un NaN au lieu de le borner (`clamp` rend NaN dès
    // que `self` en est un) : un `hover_scale.txt` contenant « nan » — que
    // `config::read_config_floats` accepte, puisque `"nan".parse::<f32>()`
    // réussit — passerait donc intact et figerait l'échelle de l'icône
    // survolée à NaN DÉFINITIVEMENT (l'animation ne s'en remet pas : `NaN`
    // plus quoi que ce soit reste `NaN`, même après que le pointeur a quitté
    // l'icône). On ignore donc l'entrée, en gardant la valeur courante — la
    // valeur par défaut au démarrage, la dernière valeur valide ensuite.
    //
    // Le test porte sur le SEUL NaN, pas sur `is_finite` : l'infini, lui, est
    // parfaitement borné par le `clamp` ci-dessous (il sature à la borne), et
    // le rejeter aussi ferait perdre cette saturation sans rien gagner.
    if scale.is_nan() {
        return;
    }
    HOVER_SCALE.with(|s| s.set(scale.clamp(MIN_HOVER_SCALE, MAX_HOVER_SCALE)));
}

/// Vitesse d'approche actuelle de l'animation — voir `HOVER_EASE_SPEED`.
pub fn hover_ease_speed() -> f32 {
    HOVER_EASE_SPEED.with(Cell::get)
}

/// Change la vitesse d'approche, bornée à [`MIN_HOVER_EASE_SPEED`,
/// `MAX_HOVER_EASE_SPEED`] — voir `set_hover_scale` pour le pourquoi du
/// bornage ici plutôt que dans l'accesseur, et pour le cas du NaN.
pub fn set_hover_ease_speed(speed: f32) {
    if speed.is_nan() {
        return;
    }
    HOVER_EASE_SPEED.with(|s| s.set(speed.clamp(MIN_HOVER_EASE_SPEED, MAX_HOVER_EASE_SPEED)));
}

/// Facteur de magnification au survol du TEXTE des horloges — voir
/// `CLOCK_HOVER_SCALE`. Distinct de [`hover_scale`], qui régit les icônes.
pub fn clock_hover_scale() -> f32 {
    CLOCK_HOVER_SCALE.with(Cell::get)
}

/// Change le facteur de magnification au survol des horloges, borné à
/// [`MIN_CLOCK_HOVER_SCALE`, `MAX_CLOCK_HOVER_SCALE`] — voir `set_hover_scale`
/// pour le pourquoi du bornage ici plutôt que dans l'accesseur, et pour le cas
/// du NaN (le même `clock_hover_scale.txt` corrompu figerait le texte).
pub fn set_clock_hover_scale(scale: f32) {
    if scale.is_nan() {
        return;
    }
    CLOCK_HOVER_SCALE
        .with(|s| s.set(scale.clamp(MIN_CLOCK_HOVER_SCALE, MAX_CLOCK_HOVER_SCALE)));
}

/// Écart d'échelle sous lequel l'animation RECOLLE à sa cible au lieu de
/// continuer à l'approcher (voir `HoverAnimator::update`) — sans quoi elle ne
/// l'atteint jamais en `f32`. Au-delà de la précision visuelle : pour une icône
/// de 96px, `1e-4` d'échelle vaut moins d'un centième de pixel.
const SCALE_SNAP_EPSILON: f32 = 1e-4;

/// Anime un facteur d'échelle de magnification au survol pour une liste
/// arbitraire de rectangles — extrait de `Dock` (qui l'utilise pour ses
/// propres icônes, voir `Dock::update`) pour être réutilisable par les
/// icônes qui vivent EN DEHORS du dock (barre du haut, icône réglages de la
/// barre droite) mais méritent le même effet : BUG RÉEL SIGNALÉ ("toutes
/// les icônes n'ont pas l'effet de magnification") — seules les icônes du
/// dock (barre de gauche) en bénéficiaient jusqu'ici, voir `app::run` pour
/// l'instance dédiée aux icônes des barres du haut/droite.
///
/// **La cible et la vitesse ne sont pas des champs de cette structure** : elles
/// sont relues à chaque `update` dans `HOVER_SCALE`/`HOVER_EASE_SPEED` (voir ces
/// statiques), ce qui fait suivre un réglage changé à chaud à toutes les
/// instances sans que ni `Dock` ni `app::run` n'aient quoi que ce soit à
/// synchroniser.
///
/// **Une instance, une cible** — c'est toute la raison d'être de
/// `update_scaled` : le texte des horloges a son propre facteur
/// (`CLOCK_HOVER_SCALE`), donc son propre animateur, plutôt qu'une cible par
/// index qui devrait alors être tenue dans la structure et maintenue en phase
/// avec `rects` (dont un simple changement de longueur réinitialise tout,
/// voir `update`).
pub struct HoverAnimator {
    scales: Vec<f32>,
}

impl HoverAnimator {
    pub fn new() -> Self {
        Self { scales: Vec::new() }
    }

    /// Avance l'animation d'un pas `dt` (secondes) vers l'état survolé/non
    /// survolé de chaque rectangle de `rects` (coordonnées fenêtre),
    /// `mouse_pos` étant la position courante de la souris — redimensionne
    /// le tampon interne si `rects.len()` change (ex. nombre d'applications
    /// épinglées, ou de boutons médias affichés, qui varie).
    ///
    /// La cible est celle des ICÔNES ; pour toute autre cible, voir
    /// `update_scaled`.
    pub fn update(&mut self, rects: &[[f32; 4]], mouse_pos: Option<(f32, f32)>, dt: f32) {
        self.update_scaled(rects, mouse_pos, dt, hover_scale());
    }

    /// Comme `update`, mais avec un facteur de magnification cible EXPLICITE —
    /// c'est ce qui donne au texte des horloges son propre réglage (voir
    /// `clock_hover_scale`) : un animateur par cible, plutôt qu'une cible par
    /// index qu'il faudrait tenir dans la structure et remettre en phase à
    /// chaque changement de longueur de `rects`.
    ///
    /// **`target_scale` n'est PAS borné ici** : l'appelant passe la cellule
    /// `CLOCK_HOVER_SCALE`, dont le mutateur borne déjà (voir
    /// `set_clock_hover_scale`). Une valeur NaN empoisonnerait `scales`
    /// exactement comme le décrit `set_hover_scale` — la garantie est donc dans
    /// la cellule, pas ici.
    pub fn update_scaled(
        &mut self,
        rects: &[[f32; 4]],
        mouse_pos: Option<(f32, f32)>,
        dt: f32,
        target_scale: f32,
    ) {
        if self.scales.len() != rects.len() {
            self.scales = vec![1.0; rects.len()];
        }
        let hovered = mouse_pos.and_then(|(mx, my)| rects.iter().position(|r| hit(r, mx, my)));

        // La vitesse est résolue en LOCALE une fois par appel (voir le doc de
        // type) : un réglage modifié en cours de frame ne peut donc pas produire
        // une frame mélangeant une ancienne vitesse et une nouvelle cible. La
        // cible, elle, arrive en paramètre — déjà résolue par l'appelant.
        let ease = (dt * hover_ease_speed()).clamp(0.0, 1.0);
        for (i, scale) in self.scales.iter_mut().enumerate() {
            let target = if Some(i) == hovered { target_scale } else { 1.0 };
            *scale += (target - *scale) * ease;
            // RECOLLAGE à la cible dès que l'écart devient imperceptible — DÉFAUT
            // RÉEL TROUVÉ PAR UN TEST : l'approche exponentielle ci-dessus
            // n'atteint JAMAIS sa cible en `f32`, elle se bloque un ou deux ULP
            // au-dessus (mesuré : `1.0000002` pour une cible de `1.0`, parce que
            // `1.0 + 0.7667ε` s'arrondit de nouveau sur `1.0 + 2 ULP`, un point
            // fixe de la récurrence). Ce n'est pas qu'une imprécision de rendu :
            // tout test « cette icône est-elle magnifiée ? » — c'est-à-dire
            // `scale > 1.0`, voir `magnified_index` — resterait VRAI pour
            // toujours après le premier survol, et l'icône garderait à jamais son
            // lot tardif et son rectangle présenté. Au seuil retenu, l'écart vaut
            // moins d'un centième de pixel pour une icône de 96px.
            if (target - *scale).abs() < SCALE_SNAP_EPSILON {
                *scale = target;
            }
        }
    }

    /// Échelle courante de l'élément `index` (`1.0` : ni animé, ni encore
    /// dimensionné par `update` — sûr d'appeler avant le premier `update`).
    pub fn scale(&self, index: usize) -> f32 {
        self.scales.get(index).copied().unwrap_or(1.0)
    }
}

impl Default for HoverAnimator {
    fn default() -> Self {
        Self::new()
    }
}

/// Barre/zone contenant une icône (voir `Dock::left_order`/`right_order`/
/// `top_left_order`/`top_right_order`/`bottom_order`) — demande explicite :
/// "déplacer les icônes des barres de droite et gauche dans leur barre
/// respective... changer les icônes de barre en utilisant la touche ctrl",
/// ÉTENDUE ENSUITE : "pouvoir déplacer les icônes des barres de gauche et de
/// droite dans la barre supérieure... [selon] le côté de la barre supérieure
/// où elles sont déposées" — `TopLeft`/`TopRight` : moitié gauche/droite de
/// la barre du HAUT (voir `Dock::zone_at`, `layout_top_side`), PAS ses
/// éléments FIXES (horloge, bureaux virtuels, contrôles média, alimentation/
/// son/réseau...), qui restent une liste à part (voir `ui::topbar`) — seules
/// les icônes du dock qu'un utilisateur y dépose explicitement vivent ici.
///
/// ⚠️ **`Bottom` (la barre du BAS) est en FIN de l'énumération**, pour la même
/// raison que partout ailleurs dans ce fichier : rien ne doit glisser devant
/// les variantes déjà écrites dans les fichiers de configuration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
    TopLeft,
    TopRight,
    /// **LE MILIEU DE LA BARRE DU HAUT** — la troisième zone demandée le
    /// 2026-10-29 : « la barre du haut a 3 zones, celle de gauche le milieu et la
    /// zone de droite ».
    ///
    /// ⚠️ **EN FIN, APRÈS `Bottom`** — rien ne doit glisser devant les variantes
    /// déjà écrites dans les fichiers de configuration.
    TopMiddle,
    /// La barre du BAS — **écran principal seulement** (voir
    /// `BarThicknesses::derive_for_screen`, qui lui donne une épaisseur nulle
    /// sur un écran secondaire : une barre ne se désactive pas par un drapeau,
    /// elle devient invisible par la géométrie).
    Bottom,
}

/// Les barres dont l'écartement se règle à part, dans l'ordre de
/// `Dock::icon_gaps_barres` et du fichier `icon_gap_barres.txt`.
pub const BARRES_ECARTEMENT: [&str; 4] = ["haut", "gauche", "droite", "bas"];

/// Le rang, dans `BARRES_ECARTEMENT`, de la barre qui porte `side`.
pub fn barre_ecartement(side: Side) -> usize {
    match side {
        Side::TopLeft | Side::TopRight | Side::TopMiddle => 0,
        Side::Left => 1,
        Side::Right => 2,
        Side::Bottom => 3,
    }
}

impl Side {
    /// Vrai pour les barres **LATÉRALES** (gauche/droite) — celles dont le
    /// contenu s'empile verticalement et **défile** (`Dock::scroll_bar`), donc
    /// les seules dont le contenu peut sortir de sa barre et empiéter sur la
    /// barre du haut. C'est ce que `Dock::lateral_clip` borne.
    pub fn is_lateral(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }
}

/// Distance (px, coordonnées fenêtre) au-delà de laquelle un clic-maintenu
/// sur une icône devient un GLISSER (voir `IconDrag::dragging`) — assez
/// petit pour rester réactif, assez grand pour ne jamais confondre un
/// simple clic (la main tremble un peu même sans intention de glisser) avec
/// un début de glisser.
const DRAG_THRESHOLD: f32 = 6.0;

/// Glisser-déposer d'icône EN COURS (voir `Dock::begin_potential_drag`) —
/// `None` la plupart du temps. Couvre à la fois le réordonnancement DANS une
/// même barre (glisser simple) et le déplacement VERS l'autre barre (Ctrl
/// maintenu, voir `Dock::drag_target_side`) : demande explicite, les deux
/// gestes partagent le même mécanisme, seule la barre CIBLE diffère.
struct IconDrag {
    /// Identité de l'icône déplacée — un index dans `Dock::icons`, PAS une
    /// position dans une barre (celle-ci change justement pendant le
    /// glisser).
    icon_index: usize,
    /// Barre de départ — la barre CIBLE tant que Ctrl n'est pas maintenu
    /// (voir `Dock::drag_target_side`).
    from_side: Side,
    /// Position souris (coordonnées fenêtre) au `MouseDown` initial — sert
    /// uniquement à mesurer le déplacement total pour `dragging` (voir sa
    /// doc), jamais réutilisée ensuite.
    start_pos: (f32, f32),
    /// Position souris COURANTE (coordonnées fenêtre), mise à jour à chaque
    /// `Event::MouseMove` (voir `Dock::update_drag`) — pilote à la fois
    /// l'icône flottante (voir `Dock::drag_preview`) et l'index d'insertion
    /// prévisualisé (voir `Dock::drag_insert_index`).
    current_pos: (f32, f32),
    /// Touche Ctrl actuellement maintenue (relue à chaque `MouseMove`, voir
    /// `Dock::update_drag`) — demande explicite : "changer les icônes de
    /// barre en utilisant la touche ctrl EN PLUS du clic gauche" — sans
    /// Ctrl, un glisser reste toujours confiné à `from_side` (voir
    /// `Dock::drag_target_side`), quelle que soit la position du curseur.
    ctrl_held: bool,
    /// `false` tant que le déplacement total depuis `start_pos` n'a pas
    /// dépassé `DRAG_THRESHOLD` — distingue un simple CLIC (relâché sans
    /// avoir assez bougé, voir `Dock::end_drag`, `DragOutcome::Clicked`)
    /// d'un VRAI glisser : sans ce seuil, le moindre tremblement de main au
    /// clic aurait réordonné les icônes au lieu de lancer l'application
    /// visée.
    dragging: bool,
    /// `true` quand ce glisser REDIMENSIONNE un espace au lieu de le déplacer
    /// (voir `Dock::update_drag`) — DEMANDE EXPLICITE : « avoir une barre de
    /// séparateur coulissante », **sans toucher au geste de déplacement**.
    ///
    /// Le geste est **Maj + glisser**, décision explicite : le glisser simple
    /// est déjà pris par le déplacement, et un espace de `SPACER_HEIGHT`
    /// (18 px, la taille d'un espace neuf) n'a **pas la place** de distinguer
    /// une zone de saisie du reste de l'élément. Maj marche pour toutes les
    /// tailles, et s'insère dans l'idiome du projet (Ctrl + clic droit gère les
    /// espaces, Ctrl + glisser les déplace entre barres).
    ///
    /// Toujours `None` pour un espace NON latéral : un espace de la barre du
    /// haut reçoit un emplacement de largeur `icon_size` et n'y a **aucune**
    /// hauteur propre (voir `layout_top_side`), le redimensionner n'y aurait
    /// aucun effet visible.
    ///
    /// **Figé à la QUALIFICATION du geste**, et surtout pas relu à chaque
    /// image : une cote amenée à `0.0` en cours de geste (permis, c'est le
    /// retour à l'état d'espace ordinaire) rendrait sinon l'élément ordinaire,
    /// et la suite du geste se mettrait à régler autre chose — le curseur
    /// revenant vers le bas ne pourrait plus remonter ce qu'il vient d'annuler.
    resizing: Option<DragAdjust>,
    /// Valeur de la grandeur réglée (voir `resizing`) et ordonnée de la souris
    /// **au moment où le geste a commencé** — la valeur courante vaut
    /// `resize_start_size + (y - resize_start_y)`, donc l'élément SUIT le
    /// curseur au lieu de dériver d'une image à l'autre (voir `update_drag`).
    resize_start_size: f32,
    resize_start_y: f32,
}

/// Ce qu'un glisser RÈGLE au lieu de réordonner — les trois grandeurs qu'un
/// espace porte, et le geste qui va avec chacune (voir `Dock::update_drag`).
#[derive(Clone, Copy, PartialEq)]
enum DragAdjust {
    /// **Maj + glisser** sur un espace ORDINAIRE : la taille de son blanc
    /// (`Icon::spacer_size`).
    SpacerSize,
    /// **Maj + glisser** sur un SÉPARATEUR DE ZONE : sa cote
    /// (`Icon::zone_height`), c'est-à-dire la hauteur de la zone qu'il ouvre.
    ZoneCote,
    /// **Glisser simple** sur un séparateur de zone qui n'a AUCUNE zone
    /// au-dessus de lui : le vide qui le précède (`Icon::zone_lead`), donc sa
    /// propre position. DEMANDE EXPLICITE : « je voudrais que ces séparateurs
    /// de zone soient déplaçables librement ».
    ///
    /// Sans zone au-dessus, sa position n'était dictée par RIEN de réglable —
    /// il restait collé au haut de sa barre, et le glisser ne le bougeait pas.
    ZoneLead,
    /// **Glisser simple** sur un séparateur de zone QUI EN A une au-dessus :
    /// la cote de CETTE zone-là (`Icon::zone_height` du séparateur précédent).
    ///
    /// Le marqueur est le bord inférieur de la zone du dessus : le tirer vers
    /// le bas, c'est l'agrandir. Règle ce champ plutôt que le sien, sinon un
    /// vide s'intercalerait ENTRE deux zones, ce qui ne voudrait rien dire —
    /// la frontière doit bouger, pas se détacher.
    ///
    /// L'index visé est celui du séparateur PRÉCÉDENT, résolu à la
    /// qualification du geste (voir `Dock::update_drag`).
    PreviousZoneCote(usize),
}

/// Ce que le panneau Réglages sait d'une zone de la barre de droite (voir
/// `Dock::zone_summaries`) — de quoi la NOMMER et la verrouiller.
#[derive(Clone, PartialEq, Debug)]
pub struct ZoneSummary {
    /// Index (dans `Dock::icons`) du séparateur qui l'ouvre — ce par quoi le
    /// verrou se pose (`set_zone_locked`).
    pub sep_index: usize,
    /// Sa cote, en pixels.
    pub cote: f32,
    /// Son nombre de colonnes DEMANDÉ (voir `Icon::zone_columns`).
    pub columns: u16,
    /// Son verrou (voir `Icon::zone_locked`).
    pub locked: bool,
    /// **CACHÉE : toute sa plage disparaît de la barre** (voir `ZoneRecord::hidden`) —
    /// la case du panneau, et ce qui la désigne comme telle dans la liste.
    pub hidden: bool,
    /// **NON SUPPRIMABLE** (voir `ZoneRecord::permanent`) — le panneau n'offre plus que
    /// de la cacher, et l'action de retrait REFUSE (voir `cef_settings`).
    pub permanent: bool,
    /// Combien d'ICÔNES elle contient — ce qui la distingue d'une voisine de
    /// même cote.
    pub content: usize,
    /// Affiche-t-elle son numéro/nom en haut d'elle-même ? (voir
    /// `Icon::zone_show_name`) — la case de la section Verrous.
    pub shows_label: bool,
    /// Le NOM que l'utilisateur lui a donné — `None` tant qu'elle n'en a pas.
    /// DEMANDE EXPLICITE : « permettre de renommer une zone à l'utilisateur ».
    pub name: Option<String>,
    /// Son NUMÉRO automatique, à partir de 1 EN PARTANT DU HAUT de la barre —
    /// DEMANDE EXPLICITE : « toujours numérotées en partant du haut de la barre
    /// même si on change leur ordre ». Il suit donc la POSITION, jamais
    /// l'identité : déplacer une zone la renumérote, et c'est le but.
    ///
    /// **`0` pour une zone RENOMMÉE** : DEMANDE EXPLICITE — « ces zones ne
    /// soient plus comptabilisées pour numéroter les zones automatiquement ».
    /// Une zone nommée ne consomme donc pas de numéro, et les suivantes se
    /// renumérotent sans trou (voir le test).
    pub number: usize,
}

/// **UNE LIGNE DU CONTENU D'UNE ZONE** — voir `Dock::zone_contents`, qui rend la
/// LISTE là où `ZoneSummary::content` ne rend qu'un compte.
///
/// Le dock ne dit QUE ce qu'il sait : la structure (un élément de l'ordre, ou le
/// miroir) et deux faits d'`Icon` (masqué, et sa taille s'il est un espace). **Il
/// ne dit NI le nom NI la nature** — une épingle est un lanceur ou non, une icône
/// est une applet ou non : cela se lit dans `config::load_pinned_apps` et dans les
/// index d'applets, qui vivent dans `app::run`. C'est l'appelant qui nomme et qui
/// décide de l'action (voir `cef_settings::ZoneRow`), et c'est ce découpage qui
/// évite de faire porter un drapeau de plus à `Icon` — il a dix-sept sites de
/// construction.
#[derive(Clone, PartialEq, Debug)]
pub enum ZoneEntry {
    /// Un élément de l'ordre de sa barre.
    Ordered {
        /// Son index dans `Dock::icons` — ce par quoi l'appelant le NOMME et le
        /// classe. Il ne quitte pas le processus : la page ne reçoit jamais d'index
        /// (voir `cef_settings::push_state`, qui n'envoie que des noms).
        index: usize,
        /// Masqué (une applet décochée dans les Réglages).
        hidden: bool,
        /// **Sa TAILLE si c'est un ESPACE**, `None` pour une icône. Un espace est
        /// un blanc réglable, et c'est la seule chose qui le décrive.
        spacer: Option<f32>,
    },
    /// **Le miroir de notification, en UNE ligne pour toute sa plage.** Ses cases
    /// SONT dans `right_order` (`set_tray_count` les y ajoute) — mais elles ne sont
    /// pas des icônes comme les autres : elles sont une quarantaine, souvent
    /// masquées, et **le fichier ne les exporte pas**. Les détailler noierait le
    /// panneau ; une ligne le résume, et explique pourquoi la zone qui les porte
    /// paraît pleine pendant que son compte annonce « 0 icône ».
    Mirror { count: usize },
}

/// La grandeur qu'un espace donne à LIRE au survol ou pendant son
/// redimensionnement (voir `Dock::spacer_readout`) — deux familles, et un
/// seul chiffre affiché à la fois.
///
/// L'index désigne TOUJOURS l'espace dont on lit la grandeur, qui n'est pas
/// forcément celui sous la main : un glisser simple sur un séparateur de zone
/// règle la cote de la zone du DESSUS (voir `DragAdjust::PreviousZoneCote`),
/// et c'est cette cote-là qu'il faut annoncer.
#[derive(Clone, Copy, PartialEq, Debug)]


pub struct SpacerReadout {
    /// Index (dans `Dock::icons`) de l'espace mesuré.
    pub index: usize,
    /// `true` : une cote de zone (`Icon::zone_height`). `false` : la taille
    /// du blanc d'un espace ordinaire (`Icon::spacer_size`).
    pub is_cote: bool,
    /// La valeur, en pixels — jamais négative.
    pub value: f32,
}

/// Ce que `Dock::end_drag` a résolu pour l'appelant (voir `app::run`,
/// `Event::MouseUp`) — remplace l'ancienne exécution IMMÉDIATE au
/// `MouseDown` (nécessaire pour pouvoir d'abord distinguer un clic d'un
/// glisser, voir `IconDrag::dragging`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragOutcome {
    /// Aucun glisser n'était en cours (le `MouseUp` ne concernait pas une
    /// icône du dock) — rien à faire.
    None,
    /// Relâché sans avoir dépassé `DRAG_THRESHOLD` : un simple clic sur
    /// l'icône `usize`, à router normalement (voir `app::run`, exactement
    /// le même `match` qu'avant ce système, juste déplacé de `MouseDown` à
    /// `MouseUp`).
    Clicked(usize),
    /// Glisser terminé, `left_order`/`right_order` déjà mis à jour — reste
    /// à persister (voir `config::save_icon_order`).
    Moved,
    /// Maj + glisser sur un espace LATÉRAL : sa hauteur a été appliquée en
    /// direct pendant le geste (voir `Dock::update_drag`), et **aucun ordre n'a
    /// bougé** — c'est tout l'enjeu, le glisser simple continuant de réordonner.
    /// Reste à persister (voir `config::save_spacer_sizes`).
    Resized,
    /// Le lâcher **n'a PAS eu lieu** : l'ordre est intact, il n'y a rien à
    /// persister ni à router.
    ///
    /// Le SEUL vocabulaire de ce refus, et c'est le point : les deux causes qui
    /// le produisent — déposer dans une zone VERROUILLÉE, et déposer une icône
    /// HORS de toute zone dans une barre qui en a (voir
    /// `Dock::drag_insert_index`) — se lisaient autrement, la première rendant
    /// `Moved`, un nom qui affirme que l'ordre a bougé alors qu'il n'a pas
    /// bougé. Les distinguer un jour (pour dire POURQUOI) se fera en enrichissant
    /// ce variant, jamais en rendant `Moved` à nouveau.
    Refused,
}

/// Épaisseur du trait dessiné au SURVOL d'un espace latéral — assez fin pour ne
/// pas se lire comme une icône, assez épais pour se voir.
///
/// **`pub(crate)` parce que la barre du HAUT dessine le même trait** (voir
/// `ui::topbar::slot_hover_line_rect`) : les deux familles de séparateurs
/// doivent se lire comme la même chose, donc elles partagent l'épaisseur et la
/// marge plutôt que d'en avoir chacune une — c'est exactement la divergence que
/// ce dépôt a déjà payée sur la largeur d'une barre (voir
/// `ui::dock::bar_thickness_bounds`).
pub(crate) const SPACER_HOVER_LINE_PX: f32 = 3.0;
/// **LA LARGEUR D'UN ESPACE DE SORTE `Trait`** — 8 px, contre `SPACER_HEIGHT`
/// (18) pour un espace ordinaire.
///
/// DEMANDE EXPLICITE (2026-09-30) : « les nouveaux séparateurs trait n'ajoutent
/// qu'un espace de 8 pixels ». Un espace ORDINAIRE doit son effet à son BLANC —
/// c'est lui qui sépare, d'autant plus quand le marqueur est rendu transparent ;
/// un TRAIT, lui, EST le séparateur, donc il n'a besoin que de la place de son
/// trait et d'un peu d'air.
pub(crate) const SPACER_TRAIT_PX: f32 = 8.0;

/// Blanc laissé entre ce trait et les bords de la barre (voir
/// `SPACER_HOVER_LINE_PX` pour la raison du `pub(crate)`).
pub(crate) const SPACER_HOVER_LINE_MARGIN: f32 = 10.0;

/// Rectangle du trait pleine largeur d'un espace latéral — PURE, donc testable :
/// il traverse la barre dans son ÉPAISSEUR, et se centre sur la hauteur de
/// l'espace QUELLE QUE SOIT sa taille (18 px comme 300).
///
/// Le « 300 » ne vaut que pour un espace ORDINAIRE : un séparateur de zone
/// occupe `SPACER_HEIGHT` quel que soit son blanc (voir `bar_layout`), donc son
/// trait se centre toujours dans la même case.
fn spacer_hover_line_rect(rect: [f32; 4], layout: &DockLayout, side: Side) -> [f32; 4] {
    let (thickness, x) = match side {
        Side::Left => (layout.left_width, SPACER_HOVER_LINE_MARGIN),
        Side::Right => (
            layout.right_width,
            layout.total_width - layout.right_width + SPACER_HOVER_LINE_MARGIN,
        ),
        // **LA BARRE DU BAS A MAINTENANT SON TRAIT, ET IL EST VERTICAL.** Cette
        // fonction rendait `(0,0)` ici, et le disait : « cette fonction ne sait
        // rendre qu'un trait HORIZONTAL, or traverser l'épaisseur d'une barre
        // horizontale demande un trait VERTICAL ». Le voici — la transposée
        // exacte du cas latéral.
        //
        // ⚠️ **IL SERT DONC AUSSI AU SURVOL**, pas seulement à la sorte `Trait` :
        // un espace de la barre du bas reçoit désormais le même retour visuel que
        // les autres. C'est un CHANGEMENT DE COMPORTEMENT assumé, pas un effet de
        // bord — et il va dans le sens de la cohérence.
        Side::Bottom => return spacer_vertical_line_rect(rect, layout),
        // La barre du HAUT reste sans trait : ses emplacements n'ont pas de case
        // propre à traverser, et rien n'en a jamais demandé.
        Side::TopLeft | Side::TopRight | Side::TopMiddle => (0.0, 0.0),
    };
    let width = (thickness - SPACER_HOVER_LINE_MARGIN * 2.0).max(0.0);
    let y = rect[1] + rect[3] * 0.5 - SPACER_HOVER_LINE_PX * 0.5;
    [x, y, width, SPACER_HOVER_LINE_PX]
}

/// **LE RECTANGLE D'UN TRAIT VERTICAL** — la transposée exacte de
/// `spacer_hover_line_rect` : il traverse l'ÉPAISSEUR de la barre du bas, centré
/// sur la case de l'espace, quelle que soit la largeur de celle-ci (18 px comme
/// 300 — et un ÉLASTIQUE en fait des centaines).
///
/// ⚠️ **Il se pose sur la BANDE, pas sur la case** : `rect` porte la hauteur d'une
/// icône, alors que le trait doit traverser toute l'épaisseur de la barre — c'est
/// la bande qui la donne (`total_height - bottom_thickness`, le même calcul que
/// `bar_rects`, et il doit le rester).
fn spacer_vertical_line_rect(rect: [f32; 4], layout: &DockLayout) -> [f32; 4] {
    let hauteur = (layout.bottom_thickness - SPACER_HOVER_LINE_MARGIN * 2.0).max(0.0);
    let x = rect[0] + rect[2] * 0.5 - SPACER_HOVER_LINE_PX * 0.5;
    let y = layout.total_height - layout.bottom_thickness + SPACER_HOVER_LINE_MARGIN;
    [x, y, SPACER_HOVER_LINE_PX, hauteur]
}

/// Instance du trait de survol — arrondi à la moitié de son épaisseur, donc une
/// extrémité en demi-cercle quelle que soit la longueur.
fn spacer_line_instance(rect: [f32; 4], layout: &DockLayout, side: Side, color: [f32; 4]) -> QuadInstance {
    QuadInstance::new(spacer_hover_line_rect(rect, layout, side), color, SPACER_HOVER_LINE_PX * 0.5, 1.0)
}

/// Géométrie de la fenêtre en "⊓" (voir `win32::window`) : une barre
/// supérieure pleine largeur et deux barres latérales (gauche, droite),
/// chacune de sa PROPRE épaisseur — INDÉPENDANTES depuis le panneau de
/// réglages (demande explicite : "redimensionner les barres gauche et
/// droite en largeur individuellement et l'épaisseur de la barre
/// supérieure"), toutes calquées par défaut sur l'épaisseur de la barre des
/// tâches Windows (voir `win32::window::Window::create_dock`) avant tout
/// réglage utilisateur.
#[derive(Clone, Copy)]
pub struct DockLayout {
    pub total_width: f32,
    pub total_height: f32,
    /// Épaisseur de la barre du HAUT (pleine largeur).
    pub top_thickness: f32,
    /// Largeur de la barre de GAUCHE.
    pub left_width: f32,
    /// Largeur de la barre de DROITE.
    pub right_width: f32,
    /// Épaisseur de la barre du BAS (pleine largeur) — **écran principal
    /// seulement** : vaut `0.0` sur un écran secondaire (voir
    /// `BarThicknesses::derive_for_screen`), et à `0.0` toutes les géométries
    /// ci-dessous redonnent EXACTEMENT celles d'avant cette barre.
    pub bottom_thickness: f32,
    /// **OÙ COMMENCENT LES COLONNES LATÉRALES** — `top_thickness` d'ordinaire,
    /// `0.0` tant que la barre du haut est MASQUÉE (masquage automatique, non
    /// révélée). DEMANDE EXPLICITE (2026-10-03) : « que leur contenu s'adapte à
    /// l'espace disponible gagné en hauteur sans afficher le contenu de la barre
    /// du haut ». Tout ce qui place, découpe ou fait défiler les colonnes lit ce
    /// champ ; tout ce qui concerne la barre du haut elle-même garde
    /// `top_thickness`.
    pub haut_colonnes: f32,
}

/// **LES PLAGES D'ICÔNES DU DOCK, ET LEUR ORDRE DANS LA BARRE.**
///
/// Le dock alloue ses icônes par PLAGES : chacune a une base stable et un compte
/// variable. Elles s'empilent dans cet ordre dans `icons` :
///
///     … épingles Windows … | dossiers + « + » | notification | applications ouvertes
///
/// ⛔ **POURQUOI CETTE ÉNUMÉRATION EXISTE.** Jusqu'au 2026-09-29, chaque plage
/// était connue par un `base` que `app.rs` mettait en cache de son côté
/// (`winpin_icon_base`, `dossier_icon_base`, `dynamic_icon_base`…). Un indice
/// mémorisé est une POSITION : insérer une icône décale tout ce qui suit, et les
/// caches pointent alors à côté — pendant la seconde qui suit, un clic tombe sur
/// la mauvaise icône. C'est la famille de défauts que le refactor supprime, et
/// `docs/plans/dock-identites-stables.md` porte les quatre étapes.
///
/// ⚠️ **L'ORDRE DE CETTE ÉNUMÉRATION EST CELUI DE LA BARRE, ET IL EST PORTEUR** :
/// `set_dynamic_apps` se termine par un `truncate` sur SA base, donc la plage de
/// notification doit vivre SOUS la sienne (voir `tray_base`, doc de champ). On ne
/// réordonne pas cette liste sans relire ces contraintes.
// ⚠️ **`dead_code` RESTE, ET IL NE RESTE PLUS POUR LA MÊME RAISON.** Depuis l'étape 4,
// `Plage::Dossiers` EST construit par du code de production (`app::DossiersVivants`, et
// les boucles de rendu de `run`) — mais les TROIS AUTRES variantes ne le sont pas :
// `WinPins` et `Notification` n'ont pas encore d'appelant, et `Dynamiques` n'en aura
// jamais, puisque `plage_fin` la refuse (elle n'a pas de compte, donc rien ne borne une
// insertion ni un retrait). L'`allow` couvre ces trois-là, pas l'énumération entière.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plage {
    WinPins,
    Dossiers,
    Notification,
    Dynamiques,
}

#[allow(dead_code)] // `label` sera utilisé pour le rendu de texte (voir roadmap README)
pub struct Icon {
    pub label: char,
    pub color: [f32; 4],
    /// `true` pour la première icône (le logo NeuraDesk) : dessinée à
    /// partir de la texture chargée dans `gfx::Texture::load_logo` au lieu
    /// d'un carré de couleur unie.
    pub has_texture: bool,
    /// `true` si `app::run` a réussi à extraire une vraie icône
    /// d'application pour cette icône (voir `win32::appicon`,
    /// `gfx::icon::extract_icon_texture`) — dans ce cas `render_instances`
    /// ne produit PAS d'instance pour elle : `app::run` la dessine
    /// lui-même dans un lot séparé, avec sa propre texture (le pipeline ne
    /// lie qu'une texture par lot, voir `gfx::quad_renderer`, donc chaque
    /// icône avec une image qui lui est propre a besoin de son propre
    /// batch). `false` (valeur par défaut) : rendue ici en carré de
    /// couleur uni, comme avant.
    pub external_texture: bool,
    /// `true` pour un espace INVISIBLE ajouté par l'utilisateur comme
    /// séparateur (voir `Dock::add_spacer_after`/`add_spacer_at_end`,
    /// demande explicite : "ajouter la possibilité d'ajouter ou retirer des
    /// espaces invisibles de la taille d'une icône comme séparateur").
    /// Occupe un emplacement de la taille d'une icône normale dans l'ordre
    /// de sa barre (voir `layout_side`, agnostique du CONTENU d'une icône)
    /// mais ne dessine RIEN (voir `render_instances`/`drag_preview_instance`,
    /// qui l'excluent explicitement) et ne lance rien au clic gauche (voir
    /// `app::run`, `DragOutcome::Clicked`) — reste néanmoins déplaçable par
    /// glisser comme n'importe quelle icône (voir `begin_potential_drag`,
    /// agnostique lui aussi du contenu).
    pub is_spacer: bool,
    /// Hauteur de CET espace précis (voir `Icon::is_spacer`), en pixels —
    /// ignoré si `is_spacer` est faux. `SPACER_HEIGHT` par défaut à la
    /// création (voir `Dock::add_spacer_after`/`add_spacer_at_end`),
    /// réglable ENSUITE individuellement par espace (voir
    /// `Dock::set_spacer_size`, demande explicite : "ajouter une option au
    /// clic droit sur un séparateur pour choisir sa taille depuis un
    /// curseur") — chaque espace garde SA PROPRE taille, pas un réglage
    /// unique partagé par tous.
    ///
    /// **Sans effet tant que l'élément DÉCLARE une zone** (voir `zone_height`) :
    /// un séparateur de zone occupe la taille de son marqueur, jamais son blanc
    /// (voir `bar_layout`).
    pub spacer_size: f32,
    /// Hauteur de la ZONE que cet espace DÉLIMITE, en pixels — `0.0` pour un
    /// espace ORDINAIRE (le cas de tous ceux d'aujourd'hui : rien ne change pour
    /// eux), `> 0.0` pour un **séparateur de zone**. DEMANDE EXPLICITE : « les
    /// séparateurs de zone de la barre de droite semblent être liés, je ne peux
    /// pas définir des tailles de zones librement ».
    ///
    /// Un séparateur de zone déclare que **les éléments qui le SUIVENT** occupent
    /// une boîte de cette hauteur : l'élément d'après (le prochain séparateur de
    /// zone, ou la fin de l'ordre) commence exactement au bas de cette boîte,
    /// quel que soit le contenu. Contenu plus court → du vide ; plus long → rogné
    /// au bord de la zone et défilable DANS la zone.
    ///
    /// **Reste `is_spacer: true`, et c'est porteur** : ce drapeau est ce qui
    /// empêche `set_icon_order` de réintégrer l'élément de force dans la barre de
    /// gauche au lancement suivant (voir sa doc). Un type d'élément distinct, sans
    /// `is_spacer`, serait silencieusement ressuscité.
    ///
    /// **Sa taille d'espace n'a AUCUN EFFET tant qu'il déclare une zone** : son
    /// emplacement vaut la taille de son marqueur (voir `bar_layout`), la zone
    /// se règle par sa COTE et l'espace avant elle par son VIDE (`zone_lead`).
    /// La valeur est CONSERVÉE pour autant — elle redevient agissante si la zone
    /// est défaite (`set_zone_height` remet alors le vide à zéro au même
    /// moment), et la supprimer ferait perdre un réglage sans rien gagner.
    ///
    /// Deux conséquences à connaître : `spacer_sizes()` exporte TOUS les
    /// `is_spacer` — un séparateur de zone y figure donc aussi, avec sa taille de
    /// MARQUEUR (voir `spacer_size`), qui n'est pas sa cote ; et la cote voyage
    /// dans son PROPRE fichier (`zone_heights.txt`), parce que
    /// `config::load_spacer_sizes` écarte une ligne mal formée EN ENTIER — ajouter
    /// un troisième champ à ce format perdrait toutes les tailles d'espace.
    pub zone_height: f32,
    /// Vide inséré AVANT cet élément, en pixels — `0.0` partout sauf sur un
    /// SÉPARATEUR DE ZONE qu'on a tiré vers le bas. DEMANDE EXPLICITE : « je
    /// voudrais que ces séparateurs de zone soient déplaçables librement ».
    ///
    /// C'est ce que règle un **glisser simple** sur un séparateur qui n'a
    /// AUCUNE zone au-dessus de lui (le premier de sa barre — le cas d'une
    /// barre vide, celui où on l'essaie en premier). Sa position était sinon
    /// entièrement dictée par la cote de la zone précédente, et le geste n'y
    /// changeait rien à l'écran : c'est ce qui a été signalé.
    ///
    /// Quand une zone le précède, le glisser règle la COTE de celle-ci à la
    /// place (voir `IconDrag::resizing`) : la frontière monte ou descend, et
    /// c'est bien la zone du dessus qui s'agrandit — pas de vide intercalé
    /// entre deux zones, qui ne voudrait rien dire.
    ///
    /// Reste `is_spacer: true` (voir `zone_height`) : c'est ce drapeau qui
    /// empêche `set_icon_order` de le réintégrer de force dans la barre de
    /// gauche au lancement suivant.
    pub zone_lead: f32,
    /// Nombre de COLONNES du contenu de la zone ouverte par ce séparateur —
    /// DEMANDE EXPLICITE : « que les zones définies par les séparateurs de
    /// droite soient découpées en grille dont le nombre de colonnes puisse être
    /// défini par l'utilisateur par rapport à la largeur de la barre ».
    ///
    /// **Vaut `DEFAULT_ZONE_COLUMNS` (1) partout ailleurs**, donc RIEN ne change
    /// pour l'existant : à une colonne, le contenu de la zone s'empile
    /// exactement comme avant les grilles. C'est le même choix que
    /// `zone_height` à `0.0` — la valeur par défaut EST le comportement
    /// d'origine, et il n'y a pas de chemin de code séparé pour « pas de
    /// grille ».
    ///
    /// **La valeur STOCKÉE n'est pas bornée** : c'est la disposition qui la
    /// borne à ce que la largeur de la barre permet (voir
    /// `Dock::effective_zone_columns`), pour qu'élargir la barre rende
    /// réellement les colonnes qu'on avait demandées — les borner à la saisie
    /// les perdrait définitivement au premier rétrécissement.
    pub zone_columns: u16,
    /// `true` pour une zone FIGÉE (DEMANDE EXPLICITE : « pour la barre de
    /// droite, la possibilité de verrouiller par séparateur ») — son contenu ne
    /// peut plus ni entrer ni sortir, et sa cote ne bouge plus. Porté par le
    /// SÉPARATEUR qui l'ouvre, comme sa cote et ses colonnes.
    ///
    /// **`false` partout par défaut**, donc rien ne change pour l'existant —
    /// même choix que `zone_height = 0.0` et `zone_columns = 1`.
    ///
    /// Voir `Dock::is_locked_at`, le prédicat que TOUS les gestes consultent.
    pub zone_locked: bool,
    /// `true` pour que la zone affiche son NUMÉRO ou son NOM en haut d'elle-même
    /// (DEMANDE EXPLICITE) — le choix se fait dans la section Verrous.
    ///
    /// **Un `bool` et non le texte** : `Icon` doit rester `Copy` (voir
    /// `Dock::zone_names`, doc du champ), et un `bool` ne coûte rien — c'est le
    /// TEXTE qui vit dans la table creuse, et il est composé à l'affichage (voir
    /// `Dock::zone_labels`).
    pub zone_show_name: bool,
    /// **CETTE ZONE NE PEUT PAS ÊTRE SUPPRIMÉE** — le panneau n'offre plus que de la
    /// cacher. DEMANDE EXPLICITE (2026-09-23), sur trois zones que l'utilisateur
    /// nomme (Accès rapide, Réglages, Notifications).
    ///
    /// **Un drapeau PAR SÉPARATEUR, comme le verrou et l'affichage du nom** — et non
    /// une liste de noms lue dans le code : une règle qui lirait les noms perdrait sa
    /// protection au premier renommage, en silence (voir `ZoneRecord::permanent`).
    ///
    /// ⚠️ **LA VISIBILITÉ, ELLE, N'A PAS DE CHAMP ICI** : c'est `hidden` qui la porte,
    /// parce que la disposition SAUTE DÉJÀ une icône masquée — `bar_layout` n'a donc
    /// qu'à sauter la PLAGE en plus, alors qu'un champ de plus aurait demandé de lui
    /// apprendre à le lire.
    pub zone_permanent: bool,
    /// `true` pour une icône "plugin" DÉSACTIVÉE (voir `app::Features`,
    /// demande explicite : "gérer les icones de la barre supérieure et les
    /// dashs comme des plugins activables ou désactivables") — n'occupe
    /// PLUS aucun emplacement dans sa barre (voir `layout_side`/
    /// `bar_layout`, qui l'excluent explicitement de l'ITÉRATION, PAS
    /// seulement du rendu, contrairement à `is_spacer`) : les autres icônes
    /// de la même barre se resserrent pour combler l'espace. BUG RÉEL
    /// SIGNALÉ EN PRATIQUE avec une première version qui RETIRAIT l'icône
    /// de `left_order`/`right_order` à la place (comme `remove_spacer`) :
    /// `Dock::set_icon_order` réintègre AUTOMATIQUEMENT toute icône
    /// EXISTANTE absente des deux listes à la fin de la barre de gauche
    /// (protection contre un ordre sauvegardé partiellement obsolète, voir
    /// sa doc) — annulant IMMÉDIATEMENT ce retrait au moindre appel
    /// ultérieur de `set_icon_order` ("l'icone du dash... ne disparaissent
    /// pas quand elles sont désélectionnées"). Rester DANS l'ordre mais en
    /// être exclu à la lecture évite ce conflit : la position d'origine est
    /// aussi naturellement conservée pour la réactivation, sans "réapparaît
    /// à la fin" comme avec l'ancienne approche.
    pub hidden: bool,
    /// **CE QUE CET ESPACE DESSINE** (voir `ui::zones::SorteEspace`) — sans effet
    /// sur une icône ordinaire, comme `spacer_size` et les champs `zone_*`.
    ///
    /// ⚠️ **`Rond` par défaut, donc le champ est INERTE pour les quelque quinze
    /// littéraux `Icon` du dépôt** : seul `insert_spacer` (et la pose d'un espace
    /// depuis le panneau) a une raison d'y mettre autre chose.
    ///
    /// ⚠️ **`Elastique` N'A DE SENS QUE SUR LA BARRE DU BAS** : c'est la seule dont
    /// la largeur soit connue et où « ce qui reste » existe (voir
    /// `layout_bottom_side`). Sur une barre latérale la valeur est CONSERVÉE (le
    /// fichier la garde) mais ne change rien au dessin — et l'interface ne la
    /// propose pas là, parce qu'un réglage sans effet est pire qu'un réglage absent.
    pub sorte: SorteEspace,
}

pub struct Dock {
    /// Le NOM de chaque icône du NOYAU — posé UNE fois par
    /// `app::apply_topbar_features` (voir `poser_noms`/`identite`, étape 1 de
    /// `docs/plans/disposition-barres-zones-icones.md`).
    ///
    /// ⚠️ Parallèle à `icons` : un changement STRUCTUREL (insertion ou retrait)
    /// la décale, et il faut la reposer. Le démarrage la pose ; rien d'autre ne
    /// la lit encore.
    noms: Vec<Option<&'static str>>,
    /// **LA LARGEUR DE L'HORLOGE, EN PIXELS** — posée par `app::run` à chaque frame (le
    /// texte est rendu en texture boutique par boutique, voir `clock_texture`), et lue par
    /// `layout_zone(Side::TopMiddle)`.
    ///
    /// ⛔ **SANS ELLE, LA RANGÉE DU MILIEU SE POSE SUR L'HORLOGE** : elle était ancrée au
    /// centre de l'écran, qui est exactement le centre de l'horloge. L'utilisateur considère
    /// (2026-10-30) que « le milieu est l'horloge et les applets qui sont autour d'elle » —
    /// donc les icônes de cette rangée vont À GAUCHE de l'horloge, comme l'icône de note
    /// (voir `topbar::note_icon_rect`). `0.0` = horloge inconnue (première frame, ou
    /// fonctionnalité désactivée) : on retombe alors sur le centrage d'avant.
    clock_width: f32,
    /// **LA CIBLE DE CHAQUE ÉPINGLE DU DOCK** (`config::PinnedApp`), dans l'ordre des
    /// places — `exe:<nom>`, `uri:<uri>`, ou le nom à défaut. Posée par `app::run` à la
    /// construction (voir `poser_cles_des_pins`), lue par `identite`.
    ///
    /// ⛔ **SANS ELLE, LES ÉPINGLES DU DOCK N'AVAIENT AUCUNE IDENTITÉ** — mesuré le
    /// 2026-10-30 en lançant la migration sur la vraie configuration : 33 applications de
    /// « Logiciels » n'étaient pas nommables, leur zone sortait VIDE de `zones.toml`, et
    /// elles seraient devenues des orphelines (reversées à la gauche) au lancement suivant.
    /// Le filet est dans `repartir_les_icones` (`sans_identite`), qui refuse d'écrire.
    pin_cles: Vec<String>,
    pub icons: Vec<Icon>,
    /// Ordre d'affichage (haut vers bas) des icônes de la barre de GAUCHE —
    /// des INDEX dans `icons`, pas les icônes elles-mêmes : leur IDENTITÉ
    /// (via les constantes `*_ICON_INDEX` d'`app::run`, ex. `APP_ICON_INDEX`)
    /// reste stable même quand leur POSITION/BARRE change (voir
    /// `set_icon_order`, `begin_potential_drag`/`end_drag`) — demande
    /// explicite : "déplacer les icônes des barres de droite et gauche dans
    /// leur barre respective". Un index n'apparaît JAMAIS dans les deux
    /// listes à la fois (voir `set_icon_order`, `end_drag`).
    left_order: Vec<usize>,
    /// Même principe que `left_order`, pour la barre de DROITE — contient
    /// SEULEMENT l'icône "Réglages" par défaut (voir `new`), comme le
    /// comportement d'origine avant ce système.
    right_order: Vec<usize>,
    /// Même principe que `left_order`/`right_order`, pour la MOITIÉ GAUCHE
    /// de la barre du HAUT (voir `Side::TopLeft`) — VIDE par défaut (aucune
    /// icône du dock n'y vit tant que l'utilisateur n'en dépose pas une).
    top_left_order: Vec<usize>,
    /// Même principe, pour la moitié DROITE de la barre du haut (voir
    /// `Side::TopRight`).
    top_right_order: Vec<usize>,
    /// **LE MILIEU DE LA BARRE DU HAUT** — voir `Side::TopMiddle`.
    top_middle_order: Vec<usize>,
    /// L'index de l'icône RÉSEAU — le débit réseau s'ancre sur elle (voir
    /// `bord_gauche_reseau`).
    index_reseau: usize,
    /// Même principe, pour la barre du BAS (voir `Side::Bottom`) — **VIDE par
    /// défaut**, et elle le RESTE : la barre du bas ne contient que des
    /// raccourcis de dossiers, qui vivent dans une table latérale et **ne
    /// passent jamais par `icon_order.txt`** (voir `Dock::dossiers`).
    ///
    /// ⚠️ **C'est délibéré, et c'est ce qui évite trois pièges mesurés** :
    /// `set_icon_order` ressuscite dans la barre de GAUCHE tout élément
    /// non-`is_spacer` absent des listes persistées, et tout ce qui dépasse
    /// `base_non_persistable` n'est de toute façon jamais écrit. Une barre du
    /// bas qui n'a rien à persister n'a rien à ressusciter.
    bottom_order: Vec<usize>,
    /// Décalage de DÉFILEMENT vertical de chaque barre LATÉRALE, en pixels —
    /// index 0 : gauche, 1 : droite (même convention que `Side::Left`/
    /// `Side::Right` traduits par `side_slot`). Toujours ≥ 0 : c'est le nombre
    /// de pixels dont on a fait remonter le contenu (voir `scroll_bar`, qui
    /// le borne). La barre du haut ne défile pas (elle s'étend en largeur) et
    /// n'a donc pas d'entrée.
    scroll: [f32; 2],
    /// ⭐ LE REPLIEMENT façon Unity (voir `ui::empilement`, `pliage_de_la_barre`),
    /// une case par barre latérale : progression LINÉAIRE du dépliement au
    /// survol (`hover_animation_` de Unity)…
    pli_survol: [f32; 2],
    /// … et défilement de la barre, en pixels, toujours ≤ 0
    /// (`launcher_drag_delta_`) — il survit au repli : ce qui a passé le haut
    /// s'y replie.
    pli_delta: [f32; 2],
    /// Réglage IMPOSÉ à ce dock — `None` : le réglage global
    /// (`ui::empilement::reglages`). Les tests s'en servent pour ne pas
    /// dépendre d'un état global partagé.
    empilement_force: Option<crate::ui::empilement::Empilement>,
    /// Défilement PROPRE à chaque zone (voir `Icon::zone_height`), en pixels —
    /// même convention que `scroll` (toujours ≥ 0), mais une entrée PAR ZONE
    /// QUI A DÉFILÉ, et CLÉE PAR L'INDEX DE SON SÉPARATEUR.
    ///
    /// Pas par le rang de la zone dans `BarLayout::zones` : ce rang se décale
    /// dès qu'un séparateur est ajouté ou retiré ailleurs dans la barre, et le
    /// défilement d'une zone sauterait alors sur sa voisine. L'index d'un
    /// espace, lui, ne change jamais (un espace retiré garde son entrée dans
    /// `icons`, voir `remove_spacer`) — même choix de clé que
    /// `zone_heights.txt`, pour la même raison.
    ///
    /// Table CREUSE : une zone jamais défilée n'y figure pas (`0.0` est la
    /// valeur par défaut, voir `zone_scroll_offset`).
    zone_scroll: Vec<(usize, f32)>,
    /// NOM donné par l'utilisateur à une zone, par l'index de son séparateur —
    /// table CREUSE comme `zone_scroll` : la plupart des zones n'ont pas de nom.
    ///
    /// **PAS un champ d'`Icon`**, et c'est délibéré : `Icon` est `Copy`, et lui
    /// ajouter une `String` le ferait cesser de l'être — des copies d'`Icon`
    /// existent un peu partout, à commencer par les quatorze littéraux de
    /// `Dock::new`. Une table à part coûte une recherche, et ne casse rien.
    ///
    /// Voir `Dock::zone_name` et `zone_summaries`, qui en tire la NUMÉROTATION
    /// automatique.
    zone_names: Vec<(usize, String)>,
    /// Glisser-déposer d'icône en cours (voir `IconDrag`, `begin_potential_drag`).
    drag: Option<IconDrag>,
    hover: HoverAnimator,
    mouse_pos: Option<(f32, f32)>,
    /// Fond PARTAGÉ par défaut des trois barres : couleur d'accentuation
    /// Windows détectée, ou `color::CUSTOM_BACKGROUND` si l'utilisateur en a
    /// fixé une (voir `win32::theme::detect_accent_color`, appelé depuis
    /// `app::run`). Reste la source d'à peu près tout ce qui a besoin d'une
    /// couleur "d'accentuation" générale (pastille "en cours d'exécution",
    /// pastille de bureau actif, teinte des panneaux WebView2...) — seul le
    /// RENDU des trois barres elles-mêmes suit désormais `bar_colors` en
    /// priorité (voir `bar_color`).
    background: [f32; 4],
    /// Couleur de fond spécifique à chaque barre — haut, gauche, droite,
    /// même indexation que `app::bar_tint`/`config::save_bar_tint` — `None` :
    /// suit `background` (comportement d'origine, une seule couleur pour les
    /// trois). `Some(color)` : cette barre a sa propre couleur, indépendante
    /// des deux autres (voir `win32::settings_dash`, un réglage par barre
    /// plutôt qu'une seule couleur partagée pour les trois).
    bar_colors: [Option<[f32; 4]>; 4],
    /// Couleur des séparateurs (voir `Icon::is_spacer`/`spacer_marker_instance`) —
    /// `None` : suit `effective_topbar_text_color` (couleur principale du
    /// texte de LA BARRE À LAQUELLE l'espace est rattaché, voir `app::run`)
    /// — DEMANDE EXPLICITE ("les séparateurs d'icones soit de la couleur
    /// principale du texte de la barre à laquelle ils sont ratachés").
    /// `Some(color)` : couleur choisie par l'utilisateur, alpha à 0 pour les
    /// rendre transparents ("donnes la possibilité de choisir leur couleur
    /// ou de les rendre transparent").
    separator_color: Option<[f32; 4]>,
    /// Diamètre (px) du petit cercle indicateur des espaces invisibles (voir
    /// `spacer_marker_instance`/`Icon::is_spacer`) — DEMANDE EXPLICITE
    /// ("ajoute la possibilité de modifier la taille des séparateurs par
    /// interface") : contrairement à `separator_color`, pas d'`Option` — une
    /// valeur PLEINE comme `icon_gap`, bornée à `MIN_SEPARATOR_SIZE..=
    /// MAX_SEPARATOR_SIZE` par `set_separator_size`. `SPACER_MARKER_DIAMETER`
    /// par défaut, comme avant que ce réglage n'existe.
    separator_size: f32,
    /// Largeur des SÉPARATEURS DE LA RANGÉE DE WIDGETS de la barre du haut
    /// (`ui::topbar::TopSlot`) — aucun séparateur par défaut, donc
    /// **rien ne bouge**. Même forme que `separator_size` : une valeur PLEINE,
    /// pas d'`Option`, bornée par `set_slot_widths`.
    ///
    /// **PAR ÉCRAN, comme tout le reste ici, et c'est le fond du correctif** :
    /// ces largeurs vivaient dans un `thread_local` global, donc un séparateur
    /// posé sur la barre PRIMAIRE apparaissait sur toutes les barres
    /// secondaires. DÉFAUT RÉEL SIGNALÉ : « ça ne devrait pas être le cas,
    /// chaque interface doit avoir ses propres réglages sur chaque écran ».
    ///
    /// **AUCUN HÉRITAGE de la primaire**, contrairement à `icon_gap`/
    /// `separator_color`/`separator_size` : un séparateur est un élément de
    /// MISE EN PAGE (comme l'ordre des icônes, `icon_orders`, qui n'hérite pas
    /// non plus), pas un STYLE. Hériter serait reproduire exactement le défaut
    /// signalé. Voir `app::resolve_slot_widths`.
    slot_widths: crate::ui::topbar::SlotWidths,
    /// Verrou de CHAQUE barre, `[haut, gauche, droite]` — DEMANDE EXPLICITE :
    /// « une option dans les réglages pour verrouiller les barres contre tout
    /// changement pour éviter les erreurs ». Voir `Dock::is_locked_at`.
    ///
    /// **Par barre, comme tout le reste des Réglages** (taille, teinte,
    /// masquage) : on peut n'en verrouiller qu'une.
    ///
    /// Attention : `bar_rank` (haut/gauche/droite, les DEUX zones du haut
    /// confondues) n'est PAS `side_slot` (gauche/droite, pour `scroll`) — deux
    /// tableaux différents, deux indexations différentes.
    bar_locked: [bool; 4],
    /// Écartement entre deux icônes consécutives (voir `icon_rects`),
    /// réglable dans le panneau de réglages (voir `config::load_icon_gap`,
    /// `set_icon_gap`) — figé en constante (`DEFAULT_ICON_GAP`) avant ce
    /// réglage.
    icon_gap: f32,
    /// L'écartement PROPRE à chaque barre — haut, gauche, droite, bas (voir
    /// `BARRES_ECARTEMENT`) — DEMANDE EXPLICITE (2026-10-18) : « que
    /// l'écartement des icônes puisse être réglé individuellement pour chaque
    /// barre ». `None` : la barre suit `icon_gap`.
    icon_gaps_barres: [Option<f32>; 4],
    /// Taille (côté du carré) des pastilles de bureaux virtuels DE CET
    /// ÉCRAN (voir `ui::topbar::desktop_dot_rect`) — DEMANDE EXPLICITE ("je
    /// voudrais que les pastilles des bureaux virtuels soient
    /// redimensionnables dans le dash réglage, en ayant chacune leur
    /// propre réglage dans la barre correspondante") : un `Dock` par écran
    /// (voir `dock_instance::DockInstance`), donc CE champ suffit à rendre
    /// le réglage indépendant par écran, même mécanisme que `icon_gap`
    /// ci-dessus mais réglé PAR INSTANCE plutôt que globalement partagé —
    /// voir `app::rebuild_secondary_docks`, qui résout la valeur de chaque
    /// écran secondaire (réglage propre ou repli sur la valeur PRIMAIRE).
    desktop_dot_size: f32,
    /// Taille de police RÉGLÉE À LA MAIN des textes de la barre du haut de CET
    /// écran — `None` = réglage AUTOMATIQUE, c'est-à-dire celle que déduit
    /// l'épaisseur de la barre (voir `ui::topbar::topbar_text_font_px_auto`).
    ///
    /// DEMANDE EXPLICITE (2026-09-23) : « le texte des applets de la barre du
    /// haut soit configurable aussi en laissant le choix du réglage automatique
    /// ou de la taille personnalisée ». **Réglé PAR INSTANCE, pas globalement**
    /// (DÉCISION UTILISATEUR), exactement comme `desktop_dot_size` juste
    /// au-dessus et pour la même raison : un `Dock` par écran suffit à rendre le
    /// réglage indépendant — voir `app::rebuild_secondary_docks`, qui résout la
    /// valeur de chaque écran secondaire (réglage propre ou repli sur la valeur
    /// PRIMAIRE).
    ///
    /// ⚠️ **Une taille, pas quatre** : c'est la BASE que ce champ remplace. Les
    /// écarts entre l'horloge et les textes secondaires restent ceux des
    /// constantes de `ui::topbar` — voir `topbar_text_font_px`, doc.
    ///
    /// Le PLAFOND n'est pas stocké ici mais déduit à chaque calcul de
    /// l'épaisseur de la barre (`topbar::topbar_text_font_px_max`) : il doit
    /// suivre le curseur d'épaisseur, pas être figé au moment du réglage.
    topbar_text_size: Option<f32>,
    /// Taille, forme et couleur de l'indicateur « en cours d'exécution » (voir
    /// `Dock::running_dot_instance`) — DEMANDE EXPLICITE : « donnes moi la
    /// possibilité d'agrandir la taille de l'indicateur, de changer sa forme et
    /// sa couleur dans le dash de réglages ». `running_dot_color` à `None` =
    /// suit la couleur d'accentuation du dock, le comportement d'origine.
    running_dot_size: f32,
    /// Décalage HORIZONTAL de l'indicateur, en pixels, positif vers la DROITE de
    /// l'écran — DEMANDE EXPLICITE (2026-10-14). `0` = sa place d'origine.
    running_dot_offset: f32,
    running_dot_shape: RunningDotShape,
    running_dot_color: Option<[f32; 4]>,
    /// Base et noms des icônes DYNAMIQUES (applications ouvertes non
    /// épinglées, voir `set_dynamic_apps`) — jamais persistées (voir
    /// `icon_order`).
    dynamic_base: Option<usize>,
    /// Les identités des ÉPINGLES Windows (`epingle:<cible>`), posées par
    /// `app::run` après `ensure_winpin_capacity` — voir `identite`.
    epingle_cles: Vec<String>,
    /// Les identités des DOSSIERS de la barre du bas (`dossier:<chemin>`),
    /// dans l'ordre de `set_dossiers` — voir `identite`.
    dossiers_cles: Vec<String>,
    dynamic_exes: Vec<String>,
    /// Base de la plage des ÉPINGLES WINDOWS (voir `ensure_winpin_capacity`).
    ///
    /// ⚠️ **CE CHAMP A ÉTÉ AJOUTÉ LE 2026-09-29 POUR `Plage::WinPins`** : la base
    /// était seulement RENDUE par `ensure_winpin_capacity`, et c'est `app.rs` qui la
    /// gardait (`winpin_icon_base`) — c'est-à-dire exactement ce que le refactor des
    /// identités stables supprime. La mémoriser ici ne change rien pour l'instant,
    /// et rend la notion de plage complète.
    winpin_base: Option<usize>,
    /// Le NOMBRE d'épingles Windows allouées — le pendant de `dossiers_count` et
    /// `tray_count`, sans lequel `plage_fin(Plage::WinPins)` ne peut pas répondre.
    /// Posé par `ensure_winpin_capacity`, qui le recevait déjà en paramètre.
    winpin_count: usize,
    /// Base et compte des emplacements de la barre du BAS : un par raccourci de
    /// dossier, PLUS UN pour le bouton « + » (voir `set_dossiers`).
    ///
    /// ⚠️ **Jamais persistés, comme les deux plages ci-dessus** — et c'est ce
    /// qui les met à l'abri des trois pièges des plages variables :
    /// `set_icon_order` ressuscite dans la barre de GAUCHE tout élément
    /// non-`is_spacer` absent des listes persistées, et tout ce qui dépasse
    /// `base_non_persistable` n'est jamais écrit. L'ORDRE des dossiers, lui,
    /// vit dans `dossiers.toml` (voir `config::DossierEpingle`).
    dossiers_base: Option<usize>,
    /// Nombre de raccourcis de DOSSIERS (le « + » n'en fait pas partie : il est
    /// à `dossiers_base + dossiers_count`).
    dossiers_count: usize,
    /// Base de la plage d'icônes de la ZONE DE NOTIFICATION (voir
    /// `set_tray_count`) — jamais persistée non plus, et pour la même raison.
    ///
    /// **Distincte de `dynamic_base`, et créée AVANT elle** : la plage
    /// dynamique se termine par un `truncate` sur sa propre base, donc tout ce
    /// qui vivrait après elle serait emporté au premier changement du nombre
    /// d'applications ouvertes. Voir `set_tray_count`, qui porte la contrainte.
    tray_base: Option<usize>,
    /// Nombre de cases RÉELLEMENT occupées de cette plage — le reste de la
    /// capacité est caché (`Icon::hidden`), ce qui évite tout décalage d'index.
    tray_count: usize,
    /// Nom affiché de chaque case occupée (l'infobulle de l'entrée, réduite à sa
    /// première ligne) — **c'est LUI qui rend l'icône reconnaissable**, et c'est
    /// la raison d'être de ce champ : à 3 colonnes dans une barre de 80 px, une
    /// case fait 27 px, où aucun logo n'est identifiable (voir `set_tray_names`).
    tray_names: Vec<String>,
}

thread_local! {
    /// **SONDE — dernier index signalé comme « glyphe dessiné dans la case d'un
    /// ESPACE »** (voir `external_glyph_instance`, qui porte le défaut et sa
    /// mesure). `None` = l'état est propre, donc le prochain défaut se signalera.
    ///
    /// La garde n'est pas un confort : une sonde qui journalise à CHAQUE frame
    /// noie le journal (même leçon que `journaliser_les_espaces`), et un journal
    /// noyé ne se lit pas.
    static GLYPHE_SUR_UN_ESPACE: Cell<Option<usize>> = const { Cell::new(None) };

    /// **SONDE — la DERNIÈRE rangée du bas journalisée** (voir `layout_bottom_side`).
    ///
    /// La garde est indispensable ici plus qu'ailleurs : `layout_bottom_side` est
    /// appelée à CHAQUE frame, et une trace qui dure écrirait soixante lignes par
    /// seconde — un journal noyé ne se lit pas (même leçon que `GLYPHE_SUR_UN_ESPACE`).
    ///
    /// ⚠️ **ELLE PORTE TOUT LE CALCUL, ET PLUS SEULEMENT LE DÉBORDEMENT.** Le
    /// garde-fou booléen d'avant ne parlait QUE si le code CROYAIT déborder — et il
    /// ne croyait rien, pendant que l'écran montrait le contraire (« le + dépasse de
    /// la barre », « le point du premier élastique est invisible », qui est la
    /// signature de `part == 0`). **Quand l'instrument et l'écran se contredisent,
    /// c'est l'instrument qu'il faut ouvrir** : ces nombres diront lequel se trompe.
    ///
    /// ⛔ **UNE MÉMOIRE PAR BANDE, PAS UNE SEULE** : `app::run` calcule DEUX
    /// dispositions (primaire 3640 px, secondaire 1920 px — voir les « deux
    /// `DockLayout` jumeaux »). Avec une seule case, elles l'écrasaient à tour de
    /// rôle, chaque frame passait pour un changement, et la trace remplissait le
    /// journal à 15 lignes par seconde (61 001 lignes mesurées).
    static DERNIERE_RANGEE_DU_BAS: RefCell<Vec<(i32, (i32, i32, i32, i32, usize))>> = const { RefCell::new(Vec::new()) };
}

/// **LE DÉCALAGE DU BLOC DES ÉPINGLES, SUR LES QUATRE LISTES — fonction PURE, donc
/// partagée par ses DEUX appelants** : `Dock::decaler_le_bloc_des_epingles` (le
/// retrait d'un raccourci, depuis les Réglages) et `cef_dash::handle_add_to_zone`
/// (l'ajout, qui tient déjà ses quatre listes en locales avant d'écrire le
/// fichier). Une règle recopiée d'un côté aurait fini par diverger de l'autre —
/// et ce dépôt a déjà payé un « frère oublié » exactement sur cette mécanique.
///
/// **POURQUOI CE DÉCALAGE EXISTE, la démonstration complète étant portée par
/// `Dock::decaler_le_bloc_des_epingles`** : les index des applets valent
/// `PINNED_ICON_BASE + pinned_apps.len()`, donc une épingle de plus ou de moins
/// les décale d'un cran — tandis que les espaces de zone et les épingles de la
/// barre des tâches, eux, ne bougent pas (les remplisseurs absorbent, et
/// `winpin_base` est figé). Comme les quatre ordres citent des index ABSOLUS,
/// tout ce qui appartient à `[index, limite[` doit suivre, et RIEN au-delà.
pub(crate) fn decaler_bloc_epingles_dans(
    ordres: [&mut Vec<usize>; 4],
    index: usize,
    limite: usize,
    retrait: bool,
) {
    for order in ordres {
        if retrait {
            // La citation de l'épingle retirée part AVANT le décalage : sinon elle
            // serait recopiée d'un cran vers le bas, et désignerait l'icône suivante.
            order.retain(|&entry| entry != index);
            for entry in order.iter_mut().filter(|entry| **entry > index && **entry < limite) {
                *entry -= 1;
            }
        } else {
            for entry in order.iter_mut().filter(|entry| **entry >= index && **entry < limite) {
                *entry += 1;
            }
        }
    }
}

impl Dock {
    /// `pinned_count` : nombre d'icônes supplémentaires à réserver à la
    /// suite des icônes intégrées, pour les applications épinglées par
    /// l'utilisateur (voir `config::load_pinned_apps`) — chacune démarre
    /// en simple carré de couleur de repli, en attendant que `app::run`
    /// y attache une vraie icône extraite (voir `Icon::external_texture`).
    ///
    /// Couleur de repli — DEMANDE EXPLICITE ("les éléments des barres
    /// [doivent utiliser] le même système de thème que les menus") : toutes
    /// les icônes de repli partagent maintenant `color::menu_fg()`, le
    /// MÊME pôle neutre fixe que les popups CEF "menu" (voir
    /// `win32::cef_popup::push_menu_palette`), au lieu de `color::
    /// icon_palette` (retiré d'ici, resté défini mais inutilisé) — qui
    /// dérivait un dégradé de six teintes de l'accent Windows, une palette
    /// franchement différente de celle des menus. `dark_mode` n'a donc plus
    /// aucun rôle ICI (il servait uniquement à cette dérivation) : paramètre
    /// retiré de cette fonction (voir son unique appelant, `app::run`).
    ///
    pub fn new(background: [f32; 4], pinned_count: usize) -> Self {
        let bar_fg = { let [r, g, b] = color::menu_fg(); [r, g, b, 1.0] };
        let mut icons = vec![
            Icon { label: 'N', color: bar_fg, has_texture: true, external_texture: false, is_spacer: false, spacer_size: 0.0, zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false, hidden: false, sorte: SorteEspace::Rond }, // Logo NeuraDesk
        ];
        // Presse-papiers (voir win32::clipboard_viewer) : glyphe posé
        // dessus au lancement (voir `app::run`, `Icon::external_texture`),
        // couleur de repli sans grande importance (jamais visible en
        // pratique, contrairement aux icônes d'application dont
        // l'extraction peut échouer).
        icons.push(Icon { label: 'P', color: bar_fg, has_texture: false, external_texture: false, is_spacer: false, spacer_size: 0.0, zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false, hidden: false, sorte: SorteEspace::Rond });
        // Réglages (voir `win32::settings_dash::toggle`) — un icône du dock
        // COMME LES AUTRES depuis ce système (voir `Side`, doc de module),
        // plus une fenêtre à part gérée par `dock::settings_icon_rect`/
        // `settings_icon_hit` (retirées) : demande explicite, "déplacer les
        // icônes des barres de droite et gauche" doit aussi couvrir celle-ci.
        // `external_texture: true` : glyphe Iconsax "setting-2" (voir
        // `external_glyph_instance`), même mécanisme que Presse-papiers,
        // PAS une texture d'application extraite.
        let settings_index = icons.len();
        icons.push(Icon {
            label: 'S',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        for _ in 0..pinned_count {
            icons.push(Icon { label: '?', color: bar_fg, has_texture: false, external_texture: false, is_spacer: false, spacer_size: 0.0, zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false, hidden: false, sorte: SorteEspace::Rond });
        }
        // Quatre icônes autrefois câblées EN DUR dans `ui::topbar`
        // (alimentation/terminal déroulant/son/réseau, voir
        // `RightGroupRects` avant ce système) — désormais des
        // icônes du dock COMME LES AUTRES, même mécanisme que Réglages
        // ci-dessus (`external_texture: true`, glyphes déjà pré-rendus,
        // jamais un carré de couleur de repli visible en pratique). Demande
        // explicite : "les icônes fixes... il faut les intégrer au système
        // du dock... pour qu'elles deviennent déplaçables/réordonnables
        // comme les autres". Toujours APRÈS les icônes épinglées (jamais
        // AVANT, ni entrelacées) : `app::run` calcule leur index par
        // arithmétique sur `PINNED_ICON_BASE + pinned_count` (même
        // convention que les espaces invisibles ajoutés dynamiquement,
        // voir `add_spacer_after`) plutôt que via une valeur retournée
        // ici — seul un ordre de poussée STABLE le permet.
        let power_index = icons.len();
        icons.push(Icon {
            label: 'p',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        let topbar_terminal_index = icons.len();
        icons.push(Icon {
            label: 't',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        let sound_index = icons.len();
        icons.push(Icon {
            label: 's',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        let network_index = icons.len();
        icons.push(Icon {
            label: 'n',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Voicemeeter (voir `app::run`, `win32::voicemeeter`) — demande
        // explicite : "une nouvelle icone son qui s'appellera voicemeeter
        // avec la même icône que le réglage son". `external_texture: true` :
        // réutilise le PNG déjà chargé pour l'icône son ci-dessus
        // (`icones.sound` dans `app::run`), pas une texture séparée. DANS
        // `left_order` PAR DÉFAUT (voir plus bas, PAS exclue de son filtre) —
        // une icône de lancement/contrôle ordinaire comme les applications
        // épinglées, PAS un cinquième membre du groupe alimentation/
        // terminal/son/réseau ci-dessus (jamais demandé).
        icons.push(Icon {
            label: 'v',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Notes (voir `app::run`, `win32::cef_notes`) — dash de prise de
        // notes inspiré de usememos/memos. AJOUTÉE APRÈS Voicemeeter
        // ci-dessus plutôt qu'à côté de Réglages/Presse-papiers (ce qui
        // semblerait plus "logique" pour une icône intégrée) : même
        // raisonnement que Voicemeeter — `app::run` calcule son index par
        // arithmétique sur `voicemeeter_icon_index + 1`, un ordre de
        // poussée STABLE (toujours la DERNIÈRE icône intégrée) est donc
        // requis, et insérer ailleurs décalerait les index déjà persistés
        // des icônes/épingles suivantes (voir `config::load_icon_order`).
        // `external_texture: true` : glyphe Iconsax "note-text" dédié (voir
        // `icones.notes` dans `app::run`), pas une texture réutilisée.
        // DANS `left_order` PAR DÉFAUT (voir plus bas) — une icône de
        // lancement/contrôle ordinaire, comme Voicemeeter.
        icons.push(Icon {
            label: 'o',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Bascule du thème clair/sombre de Windows (voir `app::run`,
        // `win32::theme`). **AJOUTÉE APRÈS Notes** — toujours la DERNIÈRE icône
        // intégrée, même raisonnement que Notes et Voicemeeter ci-dessus :
        // `app::run` calcule son index par arithmétique sur
        // `notes_icon_index + 1`, donc l'ordre de poussée doit être STABLE, et
        // `PINNED_ICON_BASE` est un `const` gravé dans `icon_order.txt`.
        // `external_texture: true` : glyphe dédié (voir `icones.theme` dans
        // `app::run`), le disque coupé dessiné pour cette fonctionnalité.
        icons.push(Icon {
            label: 'c',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        let theme_toggle_index = icons.len() - 1;
        // Éclairage nocturne de Windows (voir `app::run`, `win32::night_light`) —
        // **AJOUTÉE APRÈS LA BASCULE DE THÈME**, c'est-à-dire EN DERNIER, et c'est
        // délibéré : l'arithmétique des applets est `PINNED_ICON_BASE + épingles + k`,
        // donc une place ajoutée EN FIN DE BLOC ne décale AUCUN index existant — les
        // remplisseurs absorbent la croissance — et aucun ordre n'a à être translaté.
        // Intercaler cette icône, elle, aurait déplacé tous les index suivants, et
        // `icon_order.txt` les cite en absolu (voir `decaler_bloc_epingles_dans`).
        // `external_texture: true` : glyphe dédié, la lune dessinée pour cette
        // fonctionnalité (voir `icones.night_light` dans `app::run`).
        icons.push(Icon {
            label: 'n',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        let night_light_index = icons.len() - 1;
        // File d'impression de Windows (voir `app::run`, `win32::printer`) —
        // **AJOUTÉE EN DERNIER, APRÈS l'éclairage nocturne**, et pour la raison exacte
        // qui vient d'être écrite juste au-dessus : une place ajoutée en fin de bloc ne
        // décale AUCUN index existant, et `icon_order.txt` les cite en absolu.
        // `external_texture: true` : glyphe dédié, l'imprimante dessinée pour cette
        // fonctionnalité (voir `icones.printer` dans `app::run`). Elle reste dans
        // `left_order` PAR DÉFAUT — un lanceur ordinaire, comme Voicemeeter/Notes — donc
        // visible d'emblée sur la barre primaire, et déplaçable ensuite à la souris.
        icons.push(Icon {
            label: 'i',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        let printer_index = icons.len() - 1;
        // Calendrier (voir `app::run`, `win32::fetes`) — **AJOUTÉE EN DERNIER, APRÈS
        // la file d'impression**, et c'est la règle invariable de ce bloc : une place
        // ajoutée en fin de liste ne décale AUCUN index existant, or `icon_order.txt`
        // les cite en absolu (voir la doc de `printer_index` juste au-dessus, qui
        // porte déjà la démonstration).
        //
        // `external_texture: true` : glyphe dédié, le calendrier dessiné pour cette
        // fonctionnalité (voir `TopbarIcons/SOURCE.md` et `icones.calendar` dans
        // `app::run`).
        //
        // ⚠️ SA PLACE PAR DÉFAUT EST DANS LA ZONE **INTÉGRÉS** DE LA BARRE DE DROITE,
        // et elle ne se décide PAS ici : `right_order` ci-dessous ne cite que les trois
        // icônes historiques, et c'est `app::run` qui pose les applets ajoutées après
        // coup, en écrivant l'ordre (voir son bloc « LA FILE D'IMPRESSION VA DANS LA
        // ZONE RÉGLAGES », dont celui du calendrier reprend la mécanique pour la zone
        // Intégrés).
        icons.push(Icon {
            label: 'j',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        let calendar_index = icons.len() - 1;
        // Éditeur d'image (voir `win32::cef_image_editor`) — **ENCORE EN DERNIER**,
        // même règle invariable. Sa place par défaut, juste après le calendrier dans
        // la zone Intégrés, est posée par `app::run` comme celle du calendrier.
        icons.push(Icon {
            label: 'k',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Capture d'écran (voir `win32::capture_ecran`) — **ENCORE EN DERNIER**,
        // même règle ; sa place par défaut, juste après l'éditeur d'image, est
        // posée par `app::run`.
        icons.push(Icon {
            label: 'k',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Minuteur (voir `win32::minuteur`) — **ENCORE EN DERNIER**, même règle ;
        // sa place par défaut, juste après la capture d'écran, est posée par
        // `app::run`.
        icons.push(Icon {
            label: 'm',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Surveillance (voir `win32::surveillance`) — **ENCORE EN DERNIER**, même
        // règle ; sa place par défaut, juste à gauche des Notes, est posée par
        // `app::run`.
        icons.push(Icon {
            label: 's',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Windows Update (voir `win32::windows_update`) — **ENCORE EN DERNIER**,
        // même règle ; sa place par défaut, dans la zone Réglages juste après la
        // file d'impression, est posée par `app::run`.
        icons.push(Icon {
            label: 'u',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Gestionnaire de périphériques (voir `win32::gestionnaire_peripheriques`)
        // — **ENCORE EN DERNIER**, même règle ; sa place par défaut, dans la zone
        // Réglages juste après Windows Update, est posée par `app::run`.
        icons.push(Icon {
            label: 'g',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Réglages de l'écran (voir `win32::reglages_ecran`) — **ENCORE EN
        // DERNIER**, même règle ; sa place par défaut, dans la zone Réglages
        // juste après le gestionnaire de périphériques, est posée par `app::run`.
        icons.push(Icon {
            label: 'e',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Calculatrice (voir `win32::calculatrice`) — **ENCORE EN DERNIER**, même
        // règle ; sa place par défaut, dans la zone Intégrés après la
        // surveillance, est posée par `app::run`.
        icons.push(Icon {
            label: 'c',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Serveurs SSH (voir `win32::cef_ssh`) — **ENCORE EN DERNIER**, même
        // règle ; posés par défaut après la calculatrice par `app::run`.
        icons.push(Icon {
            label: 's',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Sélecteur de couleurs (voir `win32::cef_couleurs`) — **ENCORE EN
        // DERNIER**, même règle invariable : une place ajoutée en fin de liste ne
        // décale AUCUN index existant, or `icon_order.txt` les cite en absolu.
        // Sa place par défaut, en fin de zone Intégrés, est posée par `app::run`.
        icons.push(Icon {
            label: 'p',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Fonctionnalités facultatives (voir `win32::fonctionnalites_facultatives`)
        // — **ENCORE EN DERNIER**, même règle ; sa place par défaut, dans la zone
        // Réglages juste après les réglages de l'écran, est posée par `app::run`.
        icons.push(Icon {
            label: 'f',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Notifications (voir `win32::centre_notifications`) — **ENCORE EN
        // DERNIER**, même règle ; sa place par défaut, dans la zone Réglages juste
        // après les fonctionnalités facultatives, est posée par `app::run`.
        icons.push(Icon {
            label: 'n',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Recherche (voir `win32::cef_recherche`) — **ENCORE EN DERNIER**, même
        // règle ; sa place par défaut, dans la zone Intégrés juste après les
        // notifications, est posée par `app::run`.
        icons.push(Icon {
            label: 'r',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Automatisations « quand… alors… » (voir `win32::cef_automatisations`)
        // — **ENCORE EN DERNIER**, même règle ; sa place par défaut, dans la zone
        // Réglages, est posée par `app::run`.
        icons.push(Icon {
            label: 'a',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // La météo (voir `win32::cef_meteo`) — **ENCORE EN DERNIER**, même règle ;
        // sa place par défaut, dans la zone Intégrés de la barre de droite, est
        // posée par `app::run` (DEMANDE EXPLICITE).
        icons.push(Icon {
            label: 'm',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Verrouiller la session (voir `win32::power::verrouiller_avec_confirmation`)
        // — **ENCORE EN DERNIER**, même règle : ajouter en FIN de bloc ne décale
        // aucun index, et `icon_order.txt` cite les index en ABSOLU. Sa place par
        // défaut, dans la zone Réglages de la barre de droite, est posée par
        // `app::run` (DEMANDE EXPLICITE, 2026-10-05).
        // ⚠️ `'l'` : les minuscules déjà prises sont a,c,d,e,f,g,i,j,k,m,n,o,p,r,s,t,u,v,w.
        icons.push(Icon {
            label: 'l',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Caractères accentués (voir `win32::caracteres`) — **ENCORE EN DERNIER**,
        // même règle. Sa place par défaut, dans la zone Intégrés de la barre de
        // droite, est posée par `app::run` (DEMANDE EXPLICITE, 2026-10-06).
        // ⚠️ `'b'` : les minuscules déjà prises sont a,c,d,e,f,g,i,j,k,l,m,n,o,p,r,s,t,u,v,w.
        icons.push(Icon {
            label: 'b',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Kaomojis (voir `win32::kaomoji`) — **ENCORE EN DERNIER**, même règle, et
        // comme les caractères : une FENÊTRE CEF, posée dans la zone Intégrés de la
        // barre de droite par `app::run` (DEMANDE EXPLICITE, 2026-10-06).
        // ⚠️ `'f'` : `b` (caractères) et `a,c,d,e` sont pris — la liste complète est
        // sur le bloc des caractères juste au-dessus.
        icons.push(Icon {
            label: 'f',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **L'AIDE** (voir `win32::cef_aide`) — EN DERNIER, et c'est OBLIGATOIRE :
        // l'ordre de cette liste EST celui des index, et l'aide est en fin de chaîne
        // (voir `app::run`). L'insérer au milieu aurait décalé les applets suivantes
        // et rendu faux l'`icon_order.txt` de l'utilisateur.
        // ⚠️ `'g'` : `b` (caractères) et `a,c,d,e,f` sont pris — voir le bloc des
        // caractères, qui porte la liste complète.
        icons.push(Icon {
            label: 'g',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **LES PROPRIÉTÉS SYSTÈME** (voir `win32::proprietes_systeme`) — EN
        // DERNIER, même règle que l'aide juste au-dessus et pour la même raison :
        // l'ordre de cette liste EST celui des index, et l'applet est en fin de
        // chaîne (voir `app::run`). Sa place par défaut, dans la zone **Réglages**
        // de la barre de droite, est posée par `app::run` — DEMANDE EXPLICITE
        // (2026-10-08) : « une nouvelle applet dans Intégrés pour ouvrir sysdm.cpl »,
        // la zone ayant été corrigée le même jour.
        // ⚠️ `'h'` — par la convention du fichier (voir le bloc des caractères,
        // plus haut). ⛔ **CES LABELS NE SONT PAS UNIQUES, ET ILS NE SONT PAS UNE
        // IDENTITÉ** : `'f'` et `'g'` sont chacun utilisés DEUX FOIS dans
        // `Dock::new` (mesuré : lignes 1882/1986 et 1812/2003), et le champ
        // `label` est mort de toute façon (`#[allow(dead_code)]` sur `Icon`).
        // C'est écrit ici parce qu'un test a été bâti sur l'hypothèse inverse et
        // a dû être retiré.
        icons.push(Icon {
            label: 'h',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **L'APPLET SCANNER** (voir `win32::scanner`) — EN DERNIER, même règle
        // que les deux juste au-dessus : **l'ordre de cette liste EST celui des
        // index**, et une applet neuve se pose en FIN de chaîne. L'insérer au
        // milieu décalerait tout ce qui suit, et `icon_order.txt` — qui référence
        // des index ABSOLUS — désignerait alors les mauvaises icônes chez
        // l'utilisateur. Sa place par défaut, dans la zone **Intégrés** de la
        // barre de droite, est posée par `app::run`.
        // ⚠️ `'i'` — par la convention du fichier. ⛔ Ces labels ne sont PAS une
        // identité (voir le commentaire au-dessus).
        icons.push(Icon {
            label: 'i',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **L'ÉDITEUR DE SHADERS** (voir `win32::cef_shaders`) — EN DERNIER,
        // même règle que le scanner juste au-dessus. Sa place par défaut (Intégrés,
        // après le scanner) est posée par `app::run`.
        icons.push(Icon {
            label: 'j',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **LES SYMBOLES PHONÉTIQUES** (voir `win32::cef_phonetique`) — EN
        // DERNIER, même règle. Sa place par défaut (Intégrés, après l'éditeur de
        // shaders) est posée par `app::run`.
        icons.push(Icon {
            label: 'k',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **LES SYMBOLES MATHÉMATIQUES** (voir `win32::cef_maths`) — EN DERNIER,
        // même règle. Sa place par défaut (Intégrés, après les symboles
        // phonétiques) est posée par `app::run`.
        icons.push(Icon {
            label: 'k',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **L'ÉDITEUR DES LUEURS** (voir `win32::cef_lueurs`) — EN DERNIER, même
        // règle. Sa place par défaut (Intégrés, après les symboles mathématiques)
        // est posée par `app::run`.
        icons.push(Icon {
            label: 'k',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **L'ÉDITEUR DES TEXTURES** (voir `win32::cef_textures`) — EN DERNIER,
        // même règle. Sa place par défaut (Intégrés, après l'éditeur des lueurs)
        // est posée par `app::run`.
        icons.push(Icon {
            label: 'k',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **LE NARRATEUR** (voir `win32::narrateur`) — EN DERNIER, même règle.
        // Sa place par défaut (Réglages, après les propriétés système) est posée
        // par `app::run`.
        icons.push(Icon {
            label: 'k',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **LA LOUPE** (voir `win32::loupe_windows`) — EN DERNIER, même règle. Sa
        // place par défaut (Réglages, après le narrateur) est posée par
        // `app::run`.
        icons.push(Icon {
            label: 'k',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **LE MONITEUR SYSTÈME** (voir `win32::cef_moniteur`) — EN DERNIER, même
        // règle. Sa place par défaut (Intégrés, après l'éditeur des textures) est
        // posée par `app::run`.
        icons.push(Icon {
            label: 'k',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **L'OBSERVATEUR D'ÉVÉNEMENTS** (voir `win32::observateur_evenements`) — EN
        // DERNIER, même règle : une applet ajoutée en fin de bloc ne décale AUCUN index
        // existant, les remplisseurs absorbant la croissance. Sa place par défaut (zone
        // Réglages, après les réglages de l'écran) est posée par `app::run`.
        icons.push(Icon {
            label: 'o',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **ENREGISTRER** (voir `win32::cef_enregistrer`) — EN DERNIER, même règle.
        // Sa place par défaut (après la capture d'écran) est posée par `app::run`.
        icons.push(Icon {
            label: 'e',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **LE CLAVIER VISUEL** (voir `win32::clavier_visuel`) — EN DERNIER, même règle.
        // Sa place par défaut (dans Réglages, après la loupe) est posée par `app::run`.
        icons.push(Icon {
            label: 'k',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **2048** (voir `win32::cef_jeu2048`) — EN DERNIER, même règle.
        // Sa place par défaut (dans Intégrés, après le moniteur) est posée par `app::run`.
        icons.push(Icon {
            label: 'g',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **CINQ OU PLUS** (voir `win32::cef_cinq_ou_plus`) — EN DERNIER, même règle.
        // Sa place par défaut (dans Intégrés, après 2048) est posée par `app::run`.
        icons.push(Icon {
            label: 'f',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **QUADRAPASSEL** (voir `win32::cef_quadrapassel`) — EN DERNIER, même règle.
        // Sa place par défaut (dans Intégrés, après « Cinq ou plus ») est posée par `app::run`.
        icons.push(Icon {
            label: 'e',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **LIGHTSOFF** (voir `win32::cef_lightsoff`) — EN DERNIER, même règle.
        // Sa place par défaut (dans Intégrés, après « Quadrapassel ») est posée par `app::run`.
        icons.push(Icon {
            label: 'l',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **REVERSI** (voir `win32::cef_reversi`) — EN DERNIER, même règle.
        // Sa place par défaut (dans Intégrés, après « Lights Off ») est posée par `app::run`.
        icons.push(Icon {
            label: 'r',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **BOVO** (voir `win32::cef_bovo`) — EN DERNIER, même règle.
        // Sa place par défaut (dans Intégrés, après « Reversi ») est posée par `app::run`.
        icons.push(Icon {
            label: 'b',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // ⭐ **PATIENCE** (voir `win32::cef_patience`) — EN DERNIER, même règle.
        // Sa place par défaut (dans Intégrés, après « Bovo ») est posée par `app::run`.
        icons.push(Icon {
            label: 'p',
            color: bar_fg,
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        });
        // Les épingles de la VRAIE barre des tâches Windows (voir
        // `win32::taskbar_pins`) ne sont PAS créées ici — voir
        // `ensure_winpin_capacity`, appelée PLUS TARD par `app::run`, après
        // les espaces invisibles. Les créer ici les intercalerait dans la
        // plage d'index que `icon_order.txt` référence pour ses espaces.
        // Répartition PAR DÉFAUT entre les QUATRE zones (voir
        // `set_icon_order` pour la restaurer depuis la configuration
        // persistée à la place) : tout à gauche SAUF Réglages et la bascule de
        // thème (les DEUX à droite, Réglages en tête) et les quatre icônes
        // ci-dessus (dans la moitié DROITE de la barre du haut, dans CET ORDRE
        // — reproduit exactement leur empilage d'origine, alimentation la plus
        // proche du bord, voir `layout_top_side`) — comportement d'origine
        // avant que cette répartition ne devienne réglable.
        let left_order: Vec<usize> = (0..icons.len())
            .filter(|&i| {
                i != settings_index
                    && i != theme_toggle_index
                    && i != night_light_index
                    && i != power_index
                    && i != topbar_terminal_index
                    && i != sound_index
                    && i != network_index
            })
            .collect();
        // Réglages en PREMIER : la bascule de thème vient s'ajouter SOUS lui,
        // sans déplacer l'icône que l'utilisateur a l'habitude de trouver là.
        // DEMANDE EXPLICITE : « une icône dans la zone réglage de la barre de
        // droite ». Le glisser-déposer permet ensuite de la mettre ailleurs.
        let right_order: Vec<usize> = vec![settings_index, theme_toggle_index, night_light_index];
        let top_right_order: Vec<usize> = vec![power_index, topbar_terminal_index, sound_index, network_index];
        Self {
            noms: vec![None; icons.len()],
            clock_width: 0.0,
            icons,
            left_order,
            right_order,
            top_left_order: Vec::new(),
            top_right_order,
            top_middle_order: Vec::new(),
            index_reseau: network_index,
            bottom_order: Vec::new(),
            scroll: [0.0; 2],
            pli_survol: [0.0; 2],
            pli_delta: [0.0; 2],
            empilement_force: None,
            zone_scroll: Vec::new(),
            zone_names: Vec::new(),
            drag: None,
            hover: HoverAnimator::new(),
            mouse_pos: None,
            background,
            bar_colors: [None; 4],
            separator_color: None,
            separator_size: SPACER_MARKER_DIAMETER,
            // Aucun séparateur de rangée, sur AUCUNE barre : c'est le défaut
            // « rien ne bouge » (voir `slot_widths`, champ).
            slot_widths: crate::ui::topbar::NO_SLOT_WIDTHS,
            bar_locked: [false; 4],
            icon_gap: DEFAULT_ICON_GAP,
            icon_gaps_barres: [None; 4],
            desktop_dot_size: crate::ui::topbar::DEFAULT_DESKTOP_DOT_SIZE,
            // AUTOMATIQUE par défaut — on ne change pas l'apparence de quelqu'un
            // sans qu'il le demande, même principe que `logo_is_circle`.
            topbar_text_size: None,
            running_dot_size: DEFAULT_RUNNING_DOT_SIZE,
            running_dot_offset: 0.0,
            running_dot_shape: RunningDotShape::Circle,
            running_dot_color: None,
            dynamic_base: None,
            winpin_base: None,
            winpin_count: 0,
            dossiers_base: None,
            dossiers_count: 0,            epingle_cles: Vec::new(),
            dossiers_cles: Vec::new(),
            pin_cles: Vec::new(),
            dynamic_exes: Vec::new(),
            tray_base: None,
            tray_count: 0,
            tray_names: Vec::new(),
        }
    }

    /// Restaure la répartition/l'ordre des icônes entre les QUATRE zones
    /// persistées (voir `config::load_icon_order`) — à appeler juste après
    /// `new`, avant le premier rendu. Validé DÉFENSIVEMENT, comme
    /// `set_bar_colors` : tout index qui n'existe plus (ex. moins
    /// d'applications épinglées qu'à la dernière sauvegarde) est ignoré, tout
    /// index déjà affecté à une zone PRÉCÉDENTE dans l'ordre des paramètres
    /// (gauche puis droite puis haut-gauche puis haut-droite) est ignoré
    /// dans les zones suivantes, et toute icône ACTUELLE absente des QUATRE
    /// listes (ex. nouvelle application épinglée depuis) rejoint la fin de
    /// la barre de gauche — jamais d'icône perdue/invisible faute d'un ordre
    /// sauvegardé devenu partiellement obsolète.
    /// **LES QUATRE LISTES VIVANTES**, plages d'exécution comprises (miroir de
    /// notification, applications ouvertes) — l'inverse d'`icon_order`, qui les écarte.
    pub fn icon_order_vivant(&self) -> [&[usize]; 4] {
        [&self.left_order, &self.right_order, &self.top_left_order, &self.top_right_order]
    }

    /// ⛔ **POSÉ À CHAUD, L'ORDRE GARDE SES PLAGES D'EXÉCUTION** — DÉFAUT RÉEL
    /// (2026-10-10) : « la zone de notification est vide » après une épingle posée à
    /// chaud. Les appelants à chaud (`cef_dash`) passent l'ordre PERSISTABLE
    /// (`icon_order`), qui ne cite ni le miroir de notification ni les applications
    /// ouvertes : sans ce qui suit, le miroir quittait la barre de droite et partait
    /// dans la GAUCHE avec les orphelines — et les dossiers de la barre du bas aussi.
    ///
    /// - les entrées NON persistables de chaque liste vivante y sont remises, à la
    ///   fin (là où `set_tray_count` / `set_dynamic_apps` les posent) ;
    /// - une orpheline n'est reversée à gauche que si elle est persistable ET
    ///   absente de la barre du bas.
    ///
    /// Au LANCEMENT, rien de tout cela n'existe encore : le comportement est inchangé.
    pub fn set_icon_order(&mut self, left: Vec<usize>, right: Vec<usize>, top_left: Vec<usize>, top_right: Vec<usize>) {
        let vivantes: Vec<Vec<usize>> = self
            .icon_order_vivant()
            .iter()
            .map(|liste| liste.iter().copied().filter(|&i| !self.is_persistable(i)).collect())
            .collect();
        self.poser_l_ordre(left, right, top_left, top_right);
        for (liste, vivantes) in [&mut self.left_order, &mut self.right_order, &mut self.top_left_order, &mut self.top_right_order]
            .into_iter()
            .zip(vivantes)
        {
            liste.retain(|i| !vivantes.contains(i));
            liste.extend(vivantes);
        }
    }

    fn poser_l_ordre(&mut self, left: Vec<usize>, right: Vec<usize>, top_left: Vec<usize>, top_right: Vec<usize>) {
        let count = self.icons.len();
        let mut new_left: Vec<usize> = left.into_iter().filter(|&i| i < count).collect();
        let new_right: Vec<usize> = right.into_iter().filter(|&i| i < count && !new_left.contains(&i)).collect();
        let new_top_left: Vec<usize> = top_left
            .into_iter()
            .filter(|&i| i < count && !new_left.contains(&i) && !new_right.contains(&i))
            .collect();
        let new_top_right: Vec<usize> = top_right
            .into_iter()
            .filter(|&i| i < count && !new_left.contains(&i) && !new_right.contains(&i) && !new_top_left.contains(&i))
            .collect();
        let mut seen = vec![false; count];
        for &i in new_left.iter().chain(new_right.iter()).chain(new_top_left.iter()).chain(new_top_right.iter()) {
            seen[i] = true;
        }
        // ⛔ **LES ICÔNES DU MILIEU NE SONT PAS DES ORPHELINES.** Elles ne figurent
        // dans AUCUNE des quatre listes reçues (le milieu se pose à part, voir
        // `top_middle_order`) : sans cette ligne, la règle « toute icône non citée
        // rejoint la GAUCHE » les y ferait toutes tomber au premier
        // `set_icon_order` — une barre du haut qui se vide dans la barre de gauche.
        for &i in &self.top_middle_order {
            if i < count {
                seen[i] = true;
            }
        }
        // BUG RÉEL SIGNALÉ ("ceux qui se retirent dans les autres barres
        // réapparaissent à l'ouverture de l'app") : un espace invisible
        // RETIRÉ (voir `remove_spacer`, doc détaillée — laisse VOLONTAIREMENT
        // une entrée orpheline dans `icons`, jamais réutilisée) est
        // indiscernable ici d'une icône FEATURE nouvellement intégrée que
        // cette réintégration existe pour rattraper (voir la doc de
        // fonction, "nouvelle application épinglée depuis") — sans cette
        // exclusion, `ensure_spacer_capacity` (appelée AVANT, côté
        // `app::run`) recréait la même entrée d'espace au même index avant
        // même d'arriver ici, et cette boucle la reversait alors
        // systématiquement dans la barre de gauche au lancement suivant. Un
        // espace n'existe QUE par sa présence dans l'une des quatre listes
        // (jamais "nouvellement apparu" comme une vraie icône) : jamais
        // besoin de le réintégrer d'office.
        for (i, &already_seen) in seen.iter().enumerate() {
            let ailleurs = !self.is_persistable(i) || self.bottom_order.contains(&i);
            if !already_seen && !self.icons[i].is_spacer && !ailleurs {
                new_left.push(i);
            }
        }
        self.left_order = new_left;
        self.right_order = new_right;
        self.top_left_order = new_top_left;
        self.top_right_order = new_top_right;
    }

    /// Longueur minimale que doit atteindre `Dock::icons` pour que **tous les
    /// espaces enregistrés existent** — `None` si aucun ne l'est.
    ///
    /// **Les sources sont celles qu'un geste écrit SANS FILTRE** :
    /// - `orders` : les quatre listes d'`icon_order.txt`, dont on ne garde que
    ///   les index **≥ `winpin_end`** — un index au-dessus de la dernière
    ///   épingle ne PEUT pas être une épingle, et `add_spacer_after` (le geste
    ///   de création) ANNEXE justement à la fin d'`icons` ;
    /// - `zone_floor` : **le plus grand index cité par une cote de zone**, ou
    ///   `None` s'il n'y en a aucune — `persist_lateral_sizes` les écrit sans
    ///   filtrer, contrairement à l'ordre (voir le défaut ci-dessous).
    ///
    /// ⚠️ **`zone_floor` REMPLACE L'ANCIEN PARAMÈTRE `zones: &[(usize, f32,
    /// f32)]`, ET C'EST UN DÉCOUPLAGE DÉLIBÉRÉ** : cette fonction n'a jamais eu
    /// besoin des cotes, seulement du plus grand index qu'elles citent. Prendre
    /// un `Option<usize>` pré-calculé par l'appelant retire du dock toute
    /// connaissance de la FORME des fichiers de zone — ce qui est ce qui
    /// permettra de changer leur clé sans retoucher ce fichier.
    ///
    /// **`spacer_sizes.txt` est délibérément ABSENT** : il contient TOUS les
    /// espaces, y compris les invisibles de remplissage créés par
    /// `ensure_spacer_capacity`, et le tri par `winpin_end` ne suffit plus à
    /// les distinguer dès qu'un remplissage a été créé AU-DESSUS des épingles
    /// — ce que cette fonction fait elle-même. S'en servir ne recréerait que
    /// des icônes INERTES (voir `ensure_saved_spacers_exist`).
    ///
    /// **COMPLÉMENT INDISPENSABLE D'`insert_spacer`** (voir sa doc), qui est ce
    /// qui fait qu'un espace créé est encore LÀ au redémarrage suivant. Les deux
    /// moitiés du défaut se tenaient par les deux bouts : un espace créé au bout
    /// d'`icons` n'était ni exporté par `icon_order` (donc jamais persisté), ni
    /// à l'abri de la troncature de `set_dynamic_apps` ; et même recréé, son
    /// index devait EXISTER — ce que cette fonction garantit.
    ///
    /// **À utiliser APRÈS `ensure_winpin_capacity`** (voir `app::run`) : les
    /// espaces ainsi recréés se posent AU-DESSUS des épingles, c'est-à-dire
    /// exactement là où le geste les a créés.
    pub fn saved_spacer_capacity(orders: [&[usize]; 4], zone_floor: Option<usize>, winpin_end: usize) -> Option<usize> {
        orders
            .iter()
            .flat_map(|order| order.iter().copied())
            .chain(zone_floor)
            .filter(|&index| index >= winpin_end)
            .max()
            .map(|highest| highest + 1)
    }

    /// Recrée les espaces ENREGISTRÉS jusqu'au dernier index que leurs fichiers
    /// citent (voir `saved_spacer_capacity`) — à appeler **APRÈS
    /// `ensure_winpin_capacity` et AVANT `set_icon_order`**, dans les DEUX
    /// branches : les espaces créés à la main sont annexés AU-DESSUS des
    /// épingles (voir `add_spacer_after`), donc c'est là qu'il faut les
    /// recréer, et `winpin_icon_base` reste celui des épingles.
    ///
    /// No-op si aucun espace n'est enregistré.
    ///
    /// **Sans cet appel, un espace créé pendant une session ne survit JAMAIS au
    /// redémarrage** : son index n'existe plus, `set_icon_order`/
    /// `set_spacer_sizes`/`set_zone_heights` l'écartent EN SILENCE, et le
    /// premier enregistrement réécrit le fichier sans lui — la perte est alors
    /// définitive. Défaut réel, mesuré sur la configuration de cette machine :
    /// cotes enregistrées aux index 124 et 125, barre longue de 115.
    /// Les icônes ainsi recréées mais citées par AUCUNE des quatre listes sont
    /// **INERTES** : `set_icon_order` ne réintègre jamais un espace (voir la
    /// boucle d'orphelines, qui écarte `is_spacer`), donc rien ne les place et
    /// rien ne les dessine. C'est voulu, et c'est ce qui rend cette étape sans
    /// effet visible sur une disposition existante : elle rend les fichiers
    /// COHÉRENTS, elle ne remet pas en place ce dont l'entrée d'ordre a déjà
    /// été perdue.
    pub fn ensure_saved_spacers_exist(&mut self, orders: [&[usize]; 4], zone_floor: Option<usize>, winpin_end: usize) {
        // Le contrat « APRÈS `ensure_winpin_capacity` » est VÉRIFIÉ, pas
        // seulement écrit : `saved_spacer_capacity` compare les index à
        // `winpin_end`, et si cette valeur n'était pas la longueur courante du
        // vecteur, le `limit` rendu pourrait être INFÉRIEUR à `icons.len()` —
        // `ensure_spacer_capacity` serait alors un no-op silencieux, et les
        // espaces seraient perdus exactement comme avant ce correctif.
        debug_assert_eq!(winpin_end, self.icons.len(), "à appeler juste après `ensure_winpin_capacity`");
        if let Some(limit) = Self::saved_spacer_capacity(orders, zone_floor, winpin_end) {
            self.ensure_spacer_capacity(limit);
        }
    }

    /// Complète `icons` avec des espaces invisibles jusqu'à atteindre
    /// `min_len` icônes — à appeler par `app::run` AVANT `set_icon_order`,
    /// avec le plus grand index référencé par la configuration persistée
    /// (voir `config::load_icon_order`) + 1 : sans ça, un espace créé lors
    /// d'une session précédente ne survivrait JAMAIS à un redémarrage —
    /// `new` ne recrée que les icônes intégrées/épinglées, et
    /// `set_icon_order` ignore silencieusement tout index qui n'existe pas
    /// encore dans `icons` (voir sa doc). Suppose que tout index manquant
    /// EST un espace — vrai tant que le nombre d'applications épinglées
    /// (voir `config::load_pinned_apps`) n'a pas changé depuis la dernière
    /// sauvegarde (seuls des espaces sont annexés APRÈS la création
    /// initiale, voir la doc de `remove_spacer`) ; si ce nombre a changé,
    /// `set_icon_order`/`set_spacer_sizes` continuent de dégrader proprement
    /// (jamais de panique, au pire un espace mal placé ou une taille
    /// perdue) plutôt que de garantir une reprise parfaite dans ce cas
    /// limite.
    ///
    /// **CAS SYMÉTRIQUE, À CONNAÎTRE** : le nombre d'ÉPINGLES de la barre des
    /// tâches peut lui aussi changer (il a déjà valu 20 puis 23 sur cette
    /// machine). S'il DIMINUE, les index qu'elles occupaient restent écrits
    /// dans `icon_order.txt` et passent au-dessus de la nouvelle
    /// `winpin_end` — `saved_spacer_capacity` les prend alors pour des espaces
    /// et les matérialise, ce qui laisse des blancs à leur place. Cosmétique et
    /// réversible (Ctrl + clic droit), mais c'est un changement de
    /// comportement : auparavant ces index, hors bornes, étaient simplement
    /// écartés.
    pub fn ensure_spacer_capacity(&mut self, min_len: usize) {
        while self.icons.len() < min_len {
            self.icons.push(Icon {
                label: ' ',
                color: [0.0, 0.0, 0.0, 0.0],
                has_texture: false,
                external_texture: false,
                is_spacer: true,
                spacer_size: SPACER_HEIGHT,
                zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
                hidden: false,
                sorte: SorteEspace::Rond,
            });
        }
    }

    /// Crée les icônes des épingles de la VRAIE barre des tâches Windows
    /// (voir `win32::taskbar_pins`) — à appeler par `app::run` **APRÈS**
    /// `ensure_spacer_capacity` et **AVANT** `set_icon_order`, dans cet ordre
    /// précis.
    ///
    /// POURQUOI PAS DANS `new`, ET POURQUOI CET ORDRE — BUG RÉEL CONSTATÉ :
    /// six des seize épingles se sont retrouvées dans la barre du HAUT, aux
    /// places que l'utilisateur avait réservées à ses espaces invisibles. Les
    /// index élevés de `icon_order.txt` (jusqu'à 28 sur cette machine)
    /// désignent des ESPACES, que `ensure_spacer_capacity` crée APRÈS `new`
    /// pour honorer ces index — et non des icônes de `new`. Créées dans
    /// `new`, les épingles Windows s'intercalaient donc DANS cette plage :
    /// les index 23 à 28 ne pointaient plus vers des espaces mais vers des
    /// épingles, que `set_icon_order` plaçait alors dans la barre du haut.
    ///
    /// C'est le piège que la doc de `app::run` décrit pour `PINNED_ICON_BASE`
    /// (« décaler d'un cran re-pointerait silencieusement chaque index
    /// sauvegardé ») : ajouter « à la fin de `new` » ne suffit pas, la vraie
    /// fin de la liste est atteinte une fois les espaces créés.
    ///
    /// Les épingles ainsi créées sont des ORPHELINES au sens de
    /// `set_icon_order` (absentes des quatre listes persistées) : elle les
    /// reverse à la fin de la barre de GAUCHE, exactement l'emplacement
    /// demandé.
    pub fn ensure_winpin_capacity(&mut self, count: usize) -> usize {
        let base = self.icons.len();
        // La base cesse d'être seulement RENDUE : le dock la garde, pour que
        // `Plage::WinPins` se résolve comme les trois autres (voir `emplacement`).
        self.winpin_base = Some(base);
        // Le COMPTE aussi : c'est lui qui rend `plage_fin(Plage::WinPins)` possible,
        // et donc les deux sites d'`app.rs` qui calculaient `base + winpins.len()`.
        self.winpin_count = count;
        for i in 0..count {
            self.icons.push(Icon {
                label: 'w',
                // **REPLI À 25 % D'OPACITÉ, PAS UN BLANC PLEIN** — DÉFAUT RÉEL
                // SIGNALÉ : « certaines icônes ne sont pas visibles et affichent
                // un carré blanc ». Quand l'extraction de l'icône échoue,
                // `app::run` ne marque PAS l'icône `external_texture` (il ne le
                // fait que si la texture existe) : `render_instances` dessine
                // alors ce repli, en couleur pleine — et cette couleur est le
                // blanc du texte de la barre. Un carré blanc plein, donc, qui se
                // lit comme une icône cassée sans dire ce qu'il remplace. À 25 %,
                // la même case se lit comme un emplacement VIDE, ce qu'elle est.
                color: { let [r, g, b] = color::menu_fg(); [r, g, b, 0.25] },
                has_texture: false,
                external_texture: false,
                is_spacer: false,
                spacer_size: 0.0,
                zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
                hidden: false,
                sorte: SorteEspace::Rond,
            });
            // Placées dès maintenant dans la barre de gauche : indispensable
            // quand AUCUN ordre persisté n'existe (premier lancement,
            // `icon_order.txt` supprimé) — `left_order` a été figé par `new`
            // AVANT leur création, elles n'y seraient donc pas. Quand un ordre
            // persisté existe, `set_icon_order` reconstruit `left_order` et
            // les y reverse de toute façon, comme orphelines : cette ligne est
            // alors sans effet, jamais contradictoire.
            self.left_order.push(base + i);
        }
        base
    }

    /// Ordre courant des icônes des QUATRE zones (voir `left_order`/
    /// `right_order`/`top_left_order`/`top_right_order`) — pour persister
    /// l'état complet (voir `config::save_icon_order`), même convention que
    /// `bar_colors`.
    /// **Les icônes DYNAMIQUES en sont EXCLUES** (voir `set_dynamic_apps`) :
    /// elles apparaissent et disparaissent au fil des applications ouvertes,
    /// donc les persister ferait d'un index enregistré une référence mouvante.
    /// C'est LE point qui empêche cette fonctionnalité de corrompre la
    /// disposition de l'utilisateur — celle qui a déjà été cassée deux fois.
    pub fn icon_order(&self) -> (Vec<usize>, Vec<usize>, Vec<usize>, Vec<usize>) {
        let keep = |order: &Vec<usize>| {
            order.iter().copied().filter(|i| self.is_persistable(*i)).collect::<Vec<usize>>()
        };
        (keep(&self.left_order), keep(&self.right_order), keep(&self.top_left_order), keep(&self.top_right_order))
    }

    /// Vrai si l'index `i` fait partie de la disposition PERSISTÉE — faux pour
    /// les DEUX plages d'exécution : les applications ouvertes
    /// (`set_dynamic_apps`) et les icônes de notification (`set_tray_count`).
    ///
    /// **On prend la PLUS PETITE des deux bases**, ce qui rend la règle
    /// indépendante de l'ordre de création : tout ce qui vit au-delà de la
    /// première plage d'exécution est, par construction, une icône qui va et
    /// vient — donc jamais persistable.
    /// **LA PREMIÈRE BASE NON PERSISTABLE** — la plus petite des deux plages d'exécution, `None`
    /// s'il n'y en a aucune.
    ///
    /// C'est **LA** définition de la frontière : `is_persistable` et `insert_spacer` s'y réfèrent
    /// tous les deux, et c'est ce qui les empêche de diverger. La règle n'existait qu'à un seul
    /// endroit, et `insert_spacer` s'était fié à `dynamic_base` seul — d'où un espace créé
    /// au-delà de la base du MIROIR, jamais écrit (voir sa doc, mesure à l'appui).
    fn base_non_persistable(&self) -> Option<usize> {
        match (self.tray_base, self.dynamic_base) {
            (Some(tray), Some(dynamic)) => Some(tray.min(dynamic)),
            (tray, dynamic) => tray.or(dynamic),
        }
    }

    fn is_persistable(&self, i: usize) -> bool {
        self.base_non_persistable().is_none_or(|base| i < base)
    }

    /// **POSE LA TABLE DES NOMS DU NOYAU** — appelée UNE fois par
    /// `app::apply_topbar_features`, au moment où les index sont calculés.
    ///
    /// Rend le nombre d'icônes PERSISTABLES restées SANS NOM : c'est le plancher
    /// de l'étape 1 du plan (`docs/plans/disposition-barres-zones-icones.md`) —
    /// zéro attendu, et une icône oubliée se DIT au lieu de disparaître.
    ///
    /// L'index 0 n'est jamais compté : c'est le logo, nommé par `identite`.
    pub fn poser_noms(&mut self, paires: &[(usize, &'static str)]) -> usize {
        self.noms = vec![None; self.icons.len()];
        for &(index, nom) in paires {
            if let Some(case) = self.noms.get_mut(index) {
                *case = Some(nom);
            }
        }
        let fin = self.base_non_persistable().unwrap_or(self.icons.len());
        (1..fin).filter(|&i| self.identite(i).is_none()).count()
    }

    /// **LE NOM D'UNE ICÔNE, PAR SON IDENTITÉ** — `noyau:<nom>` pour le noyau,
    /// `noyau:logo` pour l'index 0 (qui n'a jamais besoin d'être enregistré).
    ///
    /// `None` pour tout ce qui n'est PAS du noyau : épingles Windows, dossiers,
    /// applications ouvertes, miroir de notification. Ces sortes-là reçoivent
    /// leur identité de leur PLAGE, pas d'un index (voir `Plage`).
    pub fn identite(&self, index: usize) -> Option<String> {
        if index == 0 {
            return Some(String::from("noyau:logo"));
        }
        if let Some(nom) = self.noms.get(index).copied().flatten() {
            return Some(format!("noyau:{nom}"));
        }
        // ── LES ÉPINGLES DU DOCK (`config::PinnedApp`) ────────────────────────
        // Leur place est `PINNED_ICON_BASE + rang` (voir `Dock::new`), et leur identité
        // vient de la CIBLE posée au démarrage (voir `poser_cles_des_pins`).
        if let Some(rang) = index.checked_sub(crate::app::PINNED_ICON_BASE) {
            if let Some(cle) = self.pin_cles.get(rang) {
                return Some(format!("epingle_dock:{cle}"));
            }
        }
        // ── LES PLAGES ────────────────────────────────────────────────────────
        // Chacune répond par SA base et SA longueur (voir `Plage`) : l'ordre des
        // tests n'a pas d'importance, les plages ne se recouvrent pas.
        if let Some(base) = self.winpin_base {
            if index >= base && index < base + self.winpin_count {
                let i = index - base;
                // Sans cible connue, l'identité reste DÉSIGNABLE par son rang —
                // elle changera si l'ordre des épingles de Windows change, ce qui
                // est le pire cas assumé, et il est DIT par le préfixe `#`.
                return Some(match self.epingle_cles.get(i) {
                    Some(cle) if !cle.is_empty() => format!("epingle:{cle}"),
                    _ => format!("epingle:#{i}"),
                });
            }
        }
        if let Some(base) = self.dossiers_base {
            if index >= base && index < base + self.dossiers_count {
                let i = index - base;
                return self.dossiers_cles.get(i).map(|cle| format!("dossier:{cle}"));
            }
        }
        if let Some(base) = self.tray_base {
            if index >= base && index < base + self.tray_count {
                return Some(format!("notification:miroir:{}", index - base));
            }
        }
        if let Some(base) = self.dynamic_base {
            if let Some(cle) = index.checked_sub(base).and_then(|i| self.dynamic_exes.get(i)) {
                return Some(format!("ouvert:{cle}"));
            }
        }
        None
    }

    /// **TOUTES LES IDENTITÉS QUE CE DOCK SAIT RÉSOUDRE** — l'inverse de la résolution :
    /// sert à reconnaître une identité qui vient d'une AUTRE machine (une épinglette absente
    /// ici) pour la GARDER au lieu de l'effacer à la première sauvegarde (voir
    /// `config::zones_a_ecrire`).
    pub fn identites_connues(&self) -> std::collections::BTreeSet<String> {
        (0..self.icons.len()).filter_map(|index| self.identite(index)).collect()
    }

    /// **POSE LES IDENTITÉS DES ÉPINGLES DU DOCK** — appelée par `app::run` juste après
    /// `Dock::new`, avec la cible de chaque `config::PinnedApp` (`exe:<nom>`, `uri:<uri>`,
    /// ou son nom à défaut). Voir `identite`.
    ///
    /// ⚠️ **LE PRÉFIXE EST `epingle_dock:`, PAS `epingle:`** : une même application peut
    /// être épinglée À LA FOIS dans le dock et dans la barre des tâches Windows. Avec un
    /// préfixe commun, les deux porteraient la MÊME identité, et la résolution
    /// (`ordres_des_zones`, qui garde le premier index) en perdrait une — en silence.
    pub fn poser_cles_des_pins(&mut self, cles: Vec<String>) {
        self.pin_cles = cles;
    }

    /// **POSE LES IDENTITÉS DES ÉPINGLES WINDOWS** — appelée par `app::run` juste
    /// après `ensure_winpin_capacity`, avec la cible de chaque épingle (`exe:…`,
    /// `aumid:…`, ou son nom à défaut). Voir `identite`.
    pub fn poser_cles_d_epingles(&mut self, cles: Vec<String>) {
        self.epingle_cles = cles;
    }

    /// **RÉSOUT UNE LISTE D'IDENTITÉS EN INDEX** — les marqueurs d'espaces (`espace:G2`) par
    /// leur rang, le reste par `par_identite`. Une entrée qui ne se résout pas est COMPTÉE
    /// (`oubliees`) : l'appelant la journalise, jamais ne l'avale.
    fn resoudre_une_liste(
        &self,
        liste: &[String],
        par_identite: &std::collections::HashMap<&str, usize>,
        oubliees: &mut usize,
    ) -> Vec<usize> {
        let mut ordre: Vec<usize> = Vec::new();
        for entree in liste {
            let resolu = match crate::ui::zones::cle_du_marqueur(entree) {
                Some(cle) => self.spacer_index_at_rank(self.orders(), cle),
                None => par_identite.get(entree.as_str()).copied(),
            };
            match resolu {
                Some(index) if !ordre.contains(&index) => ordre.push(index),
                Some(_) => {}
                None => *oubliees += 1,
            }
        }
        ordre
    }

    /// **LES ORDRES D'UNE BARRE, BÂTIS DEPUIS LES LISTES DES ZONES** — étapes 3 et 4 du
    /// plan `docs/plans/disposition-barres-zones-icones.md`.
    ///
    /// Rend `None` quand AUCUNE zone ne porte de liste : c'est le cas de tous les
    /// `zones.toml` d'avant l'étape 2, et l'appelant ne touche alors à RIEN (le
    /// modèle par `icon_order.txt` continue).
    ///
    /// Les rangs d'une barre sont parcourus **par RANG croissant** — le rang est déjà
    /// l'ordre, il n'y a pas de liste d'ordre à ajouter au fichier. **Chaque rang émet son
    /// ESPACE, déclaré ou non** : une zone n'est que du contenu, et une barre reconstruite
    /// sans ses séparateurs perdrait toutes ses zones (voir le commentaire de `ordre_de`).
    ///
    /// ⚠️ **UNE BARRE QUI NE DÉCLARE AUCUNE ZONE REND UNE LISTE VIDE** — c'est ce qui
    /// permet à `appliquer_les_zones` de ne pas la remplacer.
    ///
    /// ⚠️ **LE BAS N'EST PAS TRAITÉ** : il garde son ordre à part (`bottom_order`, que
    /// `set_icon_order` ne reçoit pas). Le HAUT, lui, est rendu mais reversé au MILIEU par
    /// `appliquer_les_zones` tant que la découpe à trois zones n'est pas faite.
    pub fn ordres_des_zones(
        &self,
        zones: &crate::ui::zones::Zones,
    ) -> Option<(Vec<usize>, Vec<usize>, Vec<usize>, Vec<usize>, Vec<usize>)> {
        use crate::ui::zones::BarLetter;

        if zones.zones.values().all(|record| record.icones.is_empty())
            && zones.bars.values().all(|barre| {
                barre.icones.is_empty() && barre.icones_milieu.is_empty() && barre.icones_droite.is_empty()
            })
        {
            return None;
        }

        // L'identité de CHAQUE index, une seule fois : c'est la résolution.
        let mut par_identite: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        let identites: Vec<Option<String>> = (0..self.icons.len()).map(|i| self.identite(i)).collect();
        for (index, identite) in identites.iter().enumerate() {
            if let Some(identite) = identite {
                par_identite.entry(identite.as_str()).or_insert(index);
            }
        }

        let mut oubliees = 0usize;
        let mut ordre_de = |barre: BarLetter| -> Vec<usize> {
            let a_des_zones = zones.zones.keys().any(|cle| cle.bar == barre);
            // ⭐ **UNE BARRE QUI NE DÉCLARE AUCUNE ZONE PORTE SA LISTE À ELLE** (étape 5,
            // mesuré le 2026-10-30 : sans ça, la gauche retombait sur la disposition par
            // défaut dès qu'`icon_order.txt` manquait). Ses ESPACES y sont cités par leur
            // rang (`espace:G2`) — un espace n'a pas d'autre identité, et sans eux la barre
            // perdrait ses points et son compte d'espaces.
            if !a_des_zones {
                let liste = zones.bars.get(&barre).map(|barre| barre.icones.as_slice()).unwrap_or(&[]);
                return self.resoudre_une_liste(liste, &par_identite, &mut oubliees);
            }
            // ⛔ **CHAQUE RANG ÉMET SON ESPACE, DÉCLARÉ OU NON.** Une zone n'est que du
            // CONTENU : c'est le RANG qui porte le séparateur, et une barre reconstruite
            // sans eux perdrait tous ses espaces — donc toutes ses zones, qui ne se
            // résolvent plus (`zone_key_of`) et que la sauvegarde suivante effacerait.
            // C'est exactement la classe de perte que ce chantier existe pour supprimer.
            let mut ordre: Vec<usize> = Vec::new();
            for rang in 1..=zones.spacer_count(barre) {
                let Some(cle) = crate::ui::zones::ZoneKey::new(barre, rang) else { continue };
                if let Some(index) = self.spacer_index_at_rank(self.orders(), cle) {
                    if !ordre.contains(&index) {
                        ordre.push(index);
                    }
                }
                let Some(record) = zones.zones.get(&cle) else { continue };
                for identite in &record.icones {
                    match par_identite.get(identite.as_str()) {
                        Some(&index) if !ordre.contains(&index) => ordre.push(index),
                        Some(_) => {}
                        None => oubliees += 1,
                    }
                }
            }
            ordre
        };

        let gauche = ordre_de(BarLetter::Gauche);
        let droite = ordre_de(BarLetter::Droite);
        // ⭐ **LE HAUT : TROIS MOITIÉS, TROIS LISTES** (capture de l'utilisateur du
        // 2026-10-30). Ses zones ne sont pas gérées — c'est la moitié qui range — donc on
        // lit les trois listes de barre, jamais le chemin par rangs.
        let haut = zones.bars.get(&BarLetter::Haut);
        let haut_gauche = self.resoudre_une_liste(
            haut.map(|entree| entree.icones.as_slice()).unwrap_or(&[]),
            &par_identite,
            &mut oubliees,
        );
        let haut_milieu = self.resoudre_une_liste(
            haut.map(|entree| entree.icones_milieu.as_slice()).unwrap_or(&[]),
            &par_identite,
            &mut oubliees,
        );
        let haut_droite = self.resoudre_une_liste(
            haut.map(|entree| entree.icones_droite.as_slice()).unwrap_or(&[]),
            &par_identite,
            &mut oubliees,
        );
        if oubliees > 0 {
            crate::diag!("[zones] {oubliees} identite(s) citee(s) par une zone n'existent PAS sur cette machine");
        }
        Some((gauche, droite, haut_gauche, haut_milieu, haut_droite))
    }

    /// **AJOUTE AUX LISTES DES ZONES LES APPLETS DU NOYAU QU'AUCUNE NE CITE** — à la place que
    /// leur donne l'ordre courant : juste après la voisine citée qui les précède (sinon juste
    /// avant celle qui les suit).
    ///
    /// ⛔ **SANS ELLE, UNE APPLET NOUVELLE N'APPARAÎT JAMAIS CHEZ QUI A DÉJÀ UN `zones.toml`.**
    /// Mesuré le 2026-10-31 sur la VM Windows 10 : le fichier avait été écrit par une version
    /// d'avant l'Observateur d'événements ; la pose par défaut ne touche qu'`icon_order.txt`,
    /// et les listes des zones gouvernent la barre (voir `appliquer_les_zones`) — l'applet
    /// restait orpheline à chaque lancement, et le fichier était réécrit sans elle.
    ///
    /// **SEULES LES IDENTITÉS `noyau:` SONT AJOUTÉES** : une épingle ou un dossier absent des
    /// listes l'est parce que l'utilisateur l'a retiré ; une applet du noyau, elle, n'en sort
    /// jamais — même désactivée, elle y reste, cachée. Le BAS n'est pas parcouru : il a son
    /// propre arrangement (voir `bar_repartie`).
    ///
    /// ⛔ **LA VOISINE DE L'ORDRE COURANT N'EST QU'UN REPLI : `places` PASSE D'ABORD.** Mesuré
    /// le 2026-10-31 sur la même VM : l'ordre courant vient d'`icon_order.txt`, qui range des
    /// INDEX — et l'index de l'applet y figurait déjà, écrit par une version où il désignait
    /// une autre icône. La pose par défaut l'a cru « déjà posé », et l'applet est sortie
    /// après le presse-papiers. Une place donnée par IDENTITÉ (`(applet, voisine)`, l'applet
    /// juste après la voisine) ne dépend d'aucun index.
    ///
    /// Rend les identités ajoutées, dans l'ordre — vide si `zones.toml` ne porte aucune liste.
    pub fn ajouter_les_applets_absentes(&self, zones: &mut Zones, places: &[(&str, &str)]) -> Vec<String> {
        let listes = |zones: &Zones| -> Vec<String> {
            zones
                .zones
                .values()
                .flat_map(|record| record.icones.iter())
                .chain(zones.bars.values().flat_map(|barre| {
                    barre.icones.iter().chain(barre.icones_milieu.iter()).chain(barre.icones_droite.iter())
                }))
                .cloned()
                .collect()
        };
        let mut citees: std::collections::HashSet<String> = listes(zones).into_iter().collect();
        if citees.is_empty() {
            return Vec::new();
        }
        let mut ajoutees = Vec::new();
        for ordre in [&self.left_order, &self.right_order, &self.top_left_order, &self.top_middle_order, &self.top_right_order] {
            let identites: Vec<Option<String>> = ordre.iter().map(|&index| self.identite(index)).collect();
            for (position, identite) in identites.iter().enumerate() {
                let Some(identite) = identite else { continue };
                if !identite.starts_with("noyau:") || citees.contains(identite) {
                    continue;
                }
                let donnee = places
                    .iter()
                    .find(|(applet, _)| *applet == identite.as_str())
                    .map(|(_, voisine)| voisine.to_string())
                    .filter(|voisine| citees.contains(voisine));
                let avant = identites[..position].iter().rev().flatten().find(|voisine| citees.contains(*voisine));
                let apres = identites[position + 1..].iter().flatten().find(|voisine| citees.contains(*voisine));
                let placee = match (donnee.as_ref().or(avant), apres) {
                    (Some(voisine), _) => inserer_pres_de(zones, voisine, identite, 1),
                    (None, Some(voisine)) => inserer_pres_de(zones, voisine, identite, 0),
                    (None, None) => false,
                };
                if placee {
                    citees.insert(identite.clone());
                    ajoutees.push(identite.clone());
                }
            }
        }
        ajoutees
    }

    /// **APPLIQUE LES LISTES DES ZONES, SI ELLES EXISTENT** — une barre dont les zones
    /// portent des listes est remplacée ; **une barre SANS aucune zone garde son ordre**,
    /// exactement comme aujourd'hui.
    ///
    /// ⛔ **C'EST UNE CORRECTION, PAS UN CONFORT** : `ordres_des_zones` rend une liste vide
    /// pour une barre sans zone, et la passer telle quelle à `set_icon_order` faisait de
    /// TOUTES ses icônes des orphelines — reversées en vrac dans la barre de gauche. Mesuré
    /// sur la configuration réelle : la barre de GAUCHE n'a aucune zone, la droite en a
    /// quatre ; dès que les listes de la droite existent, la gauche se vidait.
    ///
    /// Rend `true` si quelque chose a été appliqué.
    pub fn appliquer_les_zones(&mut self, zones: &crate::ui::zones::Zones) -> bool {
        let Some((gauche, droite, haut_gauche, haut_milieu, haut_droite)) = self.ordres_des_zones(zones) else {
            return false;
        };
        if gauche.is_empty()
            && droite.is_empty()
            && haut_gauche.is_empty()
            && haut_milieu.is_empty()
            && haut_droite.is_empty()
        {
            return false;
        }
        // La barre sans zone garde SA liste, plages d'exécution comprises : elles sont
        // réécrites au même endroit par `set_icon_order` (voir sa doc), donc la copie est
        // sans effet sur elles.
        //
        // ⛔ **MAIS UNE ICÔNE CITÉE PAR UNE ZONE NE PEUT PAS RESTER DANS UNE AUTRE BARRE.**
        // `poser_l_ordre` donne la priorité à la GAUCHE : une icône que la zone de droite
        // revendique et que la liste (gardée, ou par défaut) tient ailleurs en serait
        // écartée EN SILENCE. C'est exactement le cas du PORTABLE (étape 5), où
        // `icon_order.txt` n'existe pas et où la disposition par défaut a placé ces icônes
        // autre part. Les listes des zones sont donc retirées des barres qu'elles ne
        // gouvernent pas.
        let mut gauche = gauche;
        let mut droite = droite;
        if gauche.is_empty() {
            gauche = self.left_order.iter().copied().filter(|index| !droite.contains(index)).collect();
        }
        if droite.is_empty() {
            droite = self.right_order.iter().copied().filter(|index| !gauche.contains(index)).collect();
        }
        let revendique: Vec<usize> = gauche.iter().chain(droite.iter()).copied().collect();
        // ⭐ **LE HAUT GARDE SES TROIS MOITIÉS DISTINCTES** (capture de l'utilisateur du
        // 2026-10-30). Une moitié qui ne porte pas de liste garde son ordre courant — et
        // les icônes que la gauche ou la droite revendiquent en sont retirées, comme
        // partout (voir le commentaire ci-dessus).
        let mut haut_gauche: Vec<usize> = if haut_gauche.is_empty() {
            self.top_left_order.iter().copied().filter(|index| !revendique.contains(index)).collect()
        } else {
            haut_gauche.into_iter().filter(|index| !revendique.contains(index)).collect()
        };
        let mut haut_droite: Vec<usize> = if haut_droite.is_empty() {
            self.top_right_order.iter().copied().filter(|index| !revendique.contains(index)).collect()
        } else {
            haut_droite.into_iter().filter(|index| !revendique.contains(index)).collect()
        };
        // ⚠️ LE MILIEU EST POSE AVANT `set_icon_order` : `poser_l_ordre` doit le
        // connaître pour ne pas reverser ses icônes dans la barre de gauche.
        if !haut_milieu.is_empty() {
            // ⛔ **ET SES ICÔNES SONT RETIRÉES DES AUTRES BARRES** : `poser_l_ordre` marque
            // le milieu « vu », mais ne retire PAS ses icônes d'une liste qui les citerait
            // déjà — or au démarrage celles d'`icon_order.txt` sont orphelines, donc
            // reversées à la gauche : elles seraient dessinées DEUX fois. Mesuré sur le
            // chemin : `gauche` est remplacée par la liste de `zones.toml` quand elle
            // existe ; ce filtre couvre le cas contraire.
            let milieu: Vec<usize> = haut_milieu.clone();
            gauche.retain(|index| !milieu.contains(index));
            droite.retain(|index| !milieu.contains(index));
            haut_gauche.retain(|index| !milieu.contains(index));
            haut_droite.retain(|index| !milieu.contains(index));
            self.top_middle_order = haut_milieu;
        }
        self.set_icon_order(gauche, droite, haut_gauche, haut_droite);
        true
    }

    /// **JOURNALISE LES ESPACES DES QUATRE BARRES** — index, clé, cote, persistable, caché.
    ///
    /// Écrite pour UNE mesure précise, parce qu'elle ne se lit nulle part ailleurs : « ma zone est
    /// écrasée et ne réagit pas aux réglages ». Le panneau n'affiche qu'une LISTE (index, cote,
    /// colonnes, contenu) et ne dit **jamais** si l'espace qu'une clé désigne est **écrit** dans
    /// `icon_order.txt` — or c'est là que tout se joue : une zone se désigne par un RANG, et le
    /// rang se compte sur l'ordre VIVANT, qui peut contenir un espace que `icon_order()` écarte
    /// parce qu'il n'est pas persistable (`is_persistable`) ou parce que rien ne le cite.
    ///
    /// **Le désaccord à voir** : `[bars.D] spacers = N` (l'état vivant, écrit par `save_zones`) et
    /// le nombre d'espaces réellement présents dans la ligne de `icon_order.txt` doivent être
    /// ÉGAUX. S'ils diffèrent, les clés de `zones.toml` désigneront d'autres espaces au prochain
    /// lancement — c'est exactement ce que « les réglages ne réagissent pas » veut dire.
    /// ⚠️ **L'ÉNUMÉRATION VIENT DE `Self::ZONES`, ET C'EST UNE CORRECTION.** Elle
    /// était écrite à la main sur **quatre** barres : la barre du **BAS** n'y a
    /// jamais figuré, donc cette sonde — écrite précisément pour dire ce qu'aucun
    /// autre chemin ne dit — était **muette sur la barre même où le défaut se
    /// produisait**. Troisième occurrence de cette famille (voir `orders_mut`).
    ///
    /// **TROIS CHAMPS DE PLUS, ET ILS SONT LA MESURE** : `vu` (ce que `icon_side`
    /// répond), `verrou` (le verdict d'`is_locked_at`, que le clic droit consulte)
    /// et `aussi` (les **autres** barres qui citent le même index). Une case citée
    /// par deux barres n'est pas une curiosité : `icon_side` répond par la
    /// **première** de sa chaîne, tandis que `icon_rects_map` est écrit par la
    /// **dernière** qui dispose la barre (`Self::ZONES` met `Bottom` en fin) — les
    /// deux désignent alors des RECTANGLES DIFFÉRENTS, et un clic atterrit sur l'un
    /// en se faisant juger par l'autre.
    pub fn journaliser_les_espaces(&self, quoi: &str) {
        for side in Self::ZONES {
            let espaces: Vec<String> = self
                .order(side)
                .iter()
                .filter(|&&index| self.is_spacer(index))
                .map(|&index| {
                    let aussi: Vec<&str> = self
                        .barres_qui_citent(index)
                        .iter()
                        .filter(|&&autre| autre != side)
                        .map(|autre| match autre {
                            Side::Left => "G",
                            Side::Right => "D",
                            Side::TopLeft => "HG",
                            Side::TopRight => "HD",
                            Side::TopMiddle => "HM",
                            Side::Bottom => "B",
                        })
                        .collect();
                    format!(
                        "{index}[cle={} cote={:?} col={:?} pers={} cache={} vu={:?} verrou={} aussi={}]",
                        self.zone_key_of(index).map_or("-".to_string(), |key| key.to_string()),
                        self.zone_height(index),
                        self.zone_columns(index),
                        self.is_persistable(index),
                        self.icons.get(index).is_some_and(|icon| icon.hidden),
                        self.icon_side(index),
                        self.is_locked_at(index),
                        if aussi.is_empty() { "-".to_string() } else { aussi.join(",") },
                    )
                })
                .collect();
            crate::diag!("espaces {quoi} — {side:?} : {}", espaces.join(" "));
        }
    }

    /// **LA RANGÉE DU BAS, TELLE QU'ELLE EST CITÉE** — un jeton par emplacement, dans
    /// l'ordre : `T<index>` un séparateur, `+` le bouton, `d<index>` un dossier.
    ///
    /// Écrite pour UNE question, et rien d'autre ne pouvait y répondre : **où le « + »
    /// se trouve-t-il réellement ?** Le geste d'insertion calcule sa `position` dans
    /// `elements` (la liste de CONFIGURATION) puis s'en sert comme index dans
    /// `bottom_order` (la liste du DOCK) — et `citer_dans_la_barre_du_bas` BORNE
    /// (`position.min(self.bottom_order.len())`), si bien qu'une position trop grande
    /// ne rate pas : elle atterrit **à la fin**, donc APRÈS le « + ». Or
    /// `set_dossiers` force le « + » en dernier à chaque lancement — un séparateur
    /// cité après lui ne peut donc pas survivre au redémarrage.
    pub fn journaliser_la_rangee_du_bas(&self, quoi: &str) {
        let plus = self.dossiers_base.map(|base| base + self.dossiers_count);
        let jeton = |index: usize| {
            if Some(index) == plus {
                "+".to_string()
            } else if self.is_spacer(index) {
                format!("T{index}")
            } else {
                format!("d{index}")
            }
        };
        let rangee: Vec<String> = self.bottom_order.iter().map(|&index| jeton(index)).collect();
        crate::diag!("rangee du bas ({quoi}) : {}", rangee.join(" "));
    }

    /// **LES BARRES QUI CITENT `index`**, dans l'ordre de `Self::ZONES`.
    ///
    /// Existe pour UNE mesure, et elle était impossible à faire autrement :
    /// `icon_side` répond par la **première** barre de sa chaîne (`G`, `D`, `HG`,
    /// `HD`, `B`), tandis que `icon_rects_map` écrit `map[index]` à chaque barre et
    /// ne garde que le **dernier** écrivain — `Bottom` étant en fin de `ZONES`. Tant
    /// qu'un index n'est cité que par UNE barre, les deux s'accordent et rien ne
    /// paraît. Dès que deux barres le citent, elles désignent **deux rectangles
    /// différents** : le clic atterrit sur l'un (celui du bas, disposé en dernier)
    /// et se fait juger par l'autre (`icon_side` → le verrou de la barre latérale).
    ///
    /// Rendre plus d'une barre ici est donc le signe d'un défaut, jamais d'un cas
    /// prévu — c'est ce que la sonde ci-dessus affiche en `aussi=`.
    pub fn barres_qui_citent(&self, index: usize) -> Vec<Side> {
        Self::ZONES.iter().copied().filter(|&side| self.order(side).contains(&index)).collect()
    }

    /// **Capacité de la plage d'icônes de la ZONE DE NOTIFICATION** — voir
    /// `set_tray_count`.
    ///
    /// **FIXE, et allouée une fois pour toutes**, comme celle des épingles de la
    /// barre des tâches (`ensure_winpin_capacity`) et pour une raison voisine :
    /// une plage dont le nombre d'icônes VARIE ne peut pas se terminer par un
    /// `truncate` si elle n'est pas la DERNIÈRE — or la plage des applications
    /// ouvertes vient après elle. On alloue donc large une fois, et on CACHE les
    /// cases inoccupées : le nombre d'icônes change sans qu'aucun index ne bouge.
    ///
    /// 64 est large : la zone de notification en compte une dizaine en pratique,
    /// et une case inoccupée ne coûte qu'un `Icon` (type `Copy`, quelques
    /// dizaines d'octets) qui n'est ni dessiné ni cliquable.
    const TRAY_ICON_CAPACITY: usize = 64;

    /// Affiche `count` icônes de la ZONE DE NOTIFICATION dans la barre de
    /// DROITE, et rend leur base — l'index de la première case.
    ///
    /// **DEMANDE EXPLICITE** : « je voudrais que dans la grille de la zone
    /// systèmes s'affichent les icônes de la barre systèmes ». Ce que cette
    /// fonction affiche vient de `win32::tray_icons` — **la seule source**, la
    /// zone de notification n'ayant aucune API publique d'énumération sur
    /// Windows 11 (voir la doc de ce module pour ce qui est MESURÉ, et pour ce
    /// qu'un miroir ne peut pas être).
    ///
    /// ⚠️ **LA BASE NE BOUGE JAMAIS APRÈS LE PREMIER APPEL**, comme celle des
    /// applications ouvertes : la recalculer décalerait tout ce qui suit.
    ///
    /// ⚠️ **À APPELER AVANT `set_dynamic_apps`** — voir `TRAY_ICON_CAPACITY`.
    /// C'est la SEULE contrainte d'ordre entre les deux plages.
    ///
    /// **Rien de tout ceci n'entre dans `icon_order()`** (voir
    /// `is_persistable`) : persister un index qui va et vient corromprait la
    /// disposition de l'utilisateur — celle qui a déjà été cassée deux fois.
    pub fn set_tray_count(&mut self, count: usize) -> usize {
        let count = count.min(Self::TRAY_ICON_CAPACITY);
        let premiere_fois = self.tray_base.is_none();
        let base = *self.tray_base.get_or_insert(self.icons.len());
        // SONDE — les DEUX bases (celle-ci et celle des applications ouvertes) décident de ce qui
        // se persiste : `is_persistable` prend la PLUS PETITE, donc tout espace créé au-delà n'est
        // JAMAIS écrit dans `icon_order.txt`. Journalisées une seule fois, à leur création : c'est
        // le seul moment où on peut les lire sans toucher à la configuration.
        if premiere_fois {
            crate::diag!("base du MIROIR de notification = {base} (icons.len() = {})", self.icons.len());
        }
        while self.icons.len() < base + Self::TRAY_ICON_CAPACITY {
            self.icons.push(Icon {
                // Même repli que partout ailleurs : une case sans texture doit se
                // lire comme VIDE, pas comme un carré blanc (voir
                // `ensure_winpin_capacity`, qui porte la mesure).
                label: 't',
                color: { let [r, g, b] = color::menu_fg(); [r, g, b, 0.25] },
                has_texture: false,
                external_texture: false,
                is_spacer: false,
                spacer_size: 0.0,
                zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
                hidden: true,
                sorte: SorteEspace::Rond,
            });
        }
        // Les `count` premières cases sont occupées, les suivantes CACHÉES.
        // `hidden` suffit : `render_instances` les saute.
        //
        // ⚠️ **ELLES SONT BIEN DANS `right_order`, EN REVANCHE** (voir le
        // `retain`/`extend` plus bas) — c'est **le FICHIER** qui ne les exporte
        // pas, parce qu'elles vivent au-delà de `base_non_persistable` (voir
        // `icon_order`). Cette phrase disait « l'ordre ne les contient pas non
        // plus », et elle était FAUSSE : elle a fait croire au contraire pendant
        // l'écriture de `Dock::zone_contents`, qui a dû les écarter explicitement
        // pour ne pas lister quarante lignes de miroir.
        for offset in 0..Self::TRAY_ICON_CAPACITY {
            self.icons[base + offset].hidden = offset >= count;
        }
        self.tray_count = count;
        // Réassertion à CHAQUE appel, comme `set_dynamic_apps` : une icône
        // retirée doit disparaître de la barre, pas seulement de `icons`.
        // **`retain` puis `extend`, JAMAIS `truncate`** : la plage vit au MILIEU
        // de `right_order`, où le reste de la barre (Réglages, bascule de thème,
        // séparateurs de zone) doit survivre.
        self.right_order.retain(|i| !(base..base + Self::TRAY_ICON_CAPACITY).contains(i));
        self.right_order.extend(base..base + count);
        base
    }

    /// Base de la plage de notification, ou `None` tant qu'elle n'a pas été
    /// créée (voir `set_tray_count`) — pour que l'appelant sache quels index
    /// lui appartiennent.
    pub fn tray_icon_base(&self) -> Option<usize> {
        self.tray_base
    }

    /// Fixe le nom affiché de chaque case occupée — à appeler juste après
    /// `set_tray_count`, avec le MÊME compte (même contrat que
    /// `set_dynamic_exes`).
    ///
    /// **C'EST LA RÉPONSE AU DÉFAUT « JE NE RECONNAIS PAS CETTE ICÔNE »**, et il
    /// n'est pas dans l'image : à 3 colonnes dans une barre de 80 px, une case
    /// fait **27 px** (`zone_cell_size`), où aucun logo n'est identifiable.
    /// L'infobulle, elle, dit le nom de l'entrée — et un nom se lit à n'importe
    /// quelle taille.
    pub fn set_tray_names(&mut self, names: Vec<String>) {
        self.tray_names = names;
    }

    /// **Remplacer le nom d'UNE entrée de la zone de notification.**
    ///
    /// Utilisé quand l'IDENTIFICATION PAR IMAGE reconnaît une entrée qui n'avait
    /// aucun nom (voir `app::run`) : plutôt que de reconstruire toute la liste —
    /// qui vient d'UI Automation, et qu'on n'a plus sous la main à ce moment-là —
    /// on corrige la seule entrée concernée.
    pub fn set_tray_name(&mut self, offset: usize, name: String) {
        if let Some(slot) = self.tray_names.get_mut(offset) {
            *slot = name;
        }
    }

    /// Nom de l'entrée de notification à cet index — `None` hors de la plage
    /// occupée (voir `set_tray_names`).
    pub fn tray_name(&self, index: usize) -> Option<&str> {
        let base = self.tray_base?;
        self.tray_names.get(index.checked_sub(base)?).map(String::as_str)
    }

    /// Crée — ou RECRÉE — les icônes des applications ouvertes non épinglées,
    /// à la suite de tout le reste, et rend leur base (l'index de la première).
    ///
    /// **`count` peut monter comme descendre** : c'est cette fonction qui fait
    /// apparaître une icône quand une application s'ouvre et la retire quand
    /// elle se ferme. La base, elle, ne bouge JAMAIS après le premier appel —
    /// la recalculer décalerait les icônes déjà en place.
    ///
    /// Les icônes créées sont placées dans la barre de GAUCHE, après les
    /// épingles, et **`icon_order` ne les exporte pas** (voir sa doc).
    ///
    /// Pas de re-bornage du défilement ici : il est fait à chaque frame par
    /// `Dock::update` (voir `reclamp_scroll`), ce qui couvre ce cas comme tous
    /// les autres changements de hauteur de contenu.
    pub fn set_dynamic_apps(&mut self, count: usize) -> usize {
        let premiere_fois = self.dynamic_base.is_none();
        let base = self.dynamic_base.unwrap_or(self.icons.len());
        self.dynamic_base = Some(base);
        // SONDE — voir `set_tray_count` : c'est la PLUS PETITE des deux bases qui borne ce qui se
        // persiste, et un espace créé au-delà n'est jamais écrit.
        if premiere_fois {
            crate::diag!("base des APPLICATIONS OUVERTES = {base} (icons.len() = {})", self.icons.len());
        }

        self.icons.truncate(base + count);
        while self.icons.len() < base + count {
            self.icons.push(Icon {
                // Même repli que les épingles, et pour la même raison : si
                // l'icône de l'application ne se résout pas, la case doit se
                // lire comme VIDE, pas comme un carré blanc (voir
                // `ensure_winpin_capacity`).
                label: 'a',
                color: { let [r, g, b] = color::menu_fg(); [r, g, b, 0.25] },
                has_texture: false,
                external_texture: false,
                is_spacer: false,
                spacer_size: 0.0,
                zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false,
                hidden: false,
                sorte: SorteEspace::Rond,
            });
        }

        // Ordre reconstruit à CHAQUE appel : un retrait doit disparaître de la
        // barre, pas seulement de `icons` — sinon l'emplacement resterait vide.
        self.left_order.retain(|i| *i < base);
        self.left_order.extend(base..base + count);

        // ⚠️⚠️ **ET LES TROIS AUTRES ORDRES DOIVENT ÊTRE PURGÉS AUSSI.** Le
        // `truncate` ci-dessus peut **RÉTRÉCIR** la liste — une application se
        // ferme — et un index de la plage dynamique qui traîne dans une AUTRE
        // barre devient alors **HORS BORNES**. C'est possible : le
        // glisser-déposer accepte n'importe quelle barre, et rien n'y interdit
        // une icône dynamique.
        //
        // MESURÉ le 2026-09-22 : `index out of bounds: the len is 173 but the
        // index is 174` — **plantage entier** du dock, `right_order` portant 174.
        // **`left_order` était le SEUL ordre nettoyé**, et c'est ce qui rendait
        // le défaut invisible tant qu'on ne glissait pas une application ouverte
        // dans la barre de droite.
        //
        // ⚠️ **`set_tray_count` fait DÉJÀ cette purge pour sa propre plage**
        // (voir son `right_order.retain`) : c'était une **asymétrie**, pas un
        // choix. On retire tout index qui n'existe plus — la condition exacte du
        // plantage, et la seule qui soit sûre sans connaître l'ancien compte.
        let existantes = self.icons.len();
        for ordre in
            [&mut self.right_order, &mut self.top_left_order, &mut self.top_right_order]
        {
            ordre.retain(|i| *i < existantes);
        }
        base
    }

    /// Nom d'exécutable de l'application dynamique `index` — `None` si `index`
    /// n'est pas dans la plage dynamique. C'est lui qui sert à la rejoindre
    /// (`win32::appicon::focus_or`).
    pub fn dynamic_exe(&self, index: usize) -> Option<&str> {
        let base = self.dynamic_base?;
        self.dynamic_exes.get(index.checked_sub(base)?).map(String::as_str)
    }

    /// **LES EMPLACEMENTS DE LA BARRE DU BAS** — `count` raccourcis de dossiers,
    /// plus UNE case pour le bouton « + ». Rend la BASE des index.
    ///
    /// Patron `set_dynamic_apps` : la plage est allouée en fin de `icons`,
    /// placée dans `bottom_order`, et **ses cases ne sont pas persistées**. Elle
    /// est recréée à CHAQUE appel (`truncate` puis remplissage), parce qu'un
    /// retrait de dossier doit disparaître de la barre et pas seulement de
    /// `icons` — sinon l'emplacement resterait vide.
    ///
    /// `external_texture = true` sur TOUTES les cases : c'est `app::run` qui les
    /// dessine (la vignette de chaque dossier, un glyphe pour le « + »), le
    /// `Dock` ne sachant construire ni l'une ni l'autre.
    pub fn set_dossiers(&mut self, elements: &[crate::config::ElementBarre]) -> usize {
        use crate::config::ElementBarre;
        let premiere_fois = self.dossiers_base.is_none();
        let base = self.dossiers_base.unwrap_or(self.icons.len());
        self.dossiers_base = Some(base);
        // ⛔ **`dossiers_count` COMPTE LES DOSSIERS, PAS LES EMPLACEMENTS.** C'est
        // ce qui garde la plage « dossiers seulement » : tout ce qui lit un rang —
        // `dossier_slot`, `rang_dans`, les quatre sites de clic — continue de
        // désigner un DOSSIER, et un séparateur n'a aucun rang (il vit au-dessus de
        // `plage_fin`). Voir `app::dossier_au_rang`, qui porte la raison.
        let count = elements.iter().filter(|e| matches!(e, ElementBarre::Dossier(_))).count();
        self.dossiers_count = count;
        // ⭐ L'IDENTITÉ DE CHAQUE DOSSIER (`dossier:<chemin>`), dans l'ordre de la
        // rangée — un séparateur n'en a pas (c'est une zone, pas une icône).
        self.dossiers_cles = elements
            .iter()
            .filter_map(|e| match e {
                ElementBarre::Dossier(d) => Some(d.chemin.to_string_lossy().into_owned()),
                ElementBarre::Separateur(_) => None,
            })
            .collect();
        let total = count + 1; // les dossiers, puis le « »
        if premiere_fois {
            crate::diag!("base des DOSSIERS (barre du bas) = {base} (icons.len() = {})", self.icons.len());
        }

        self.icons.truncate(base + total);
        while self.icons.len() < base + total {
            self.icons.push(Self::icone_de_dossier());
        }

        // **LES SÉPARATEURS VIVENT À LA SUITE DE LA PLAGE, ET LA RANGÉE EST LEUR
        // ENTRRELACEMENT — C'EST CE QUI PORTE ENFIN LEUR POSITION.**
        //
        // ⛔ **CE QU'IL Y AVAIT ICI, ET POURQUOI ÇA A CHANGÉ.** La liste était
        // reconstruite avec la seule plage, et les séparateurs déjà cités étaient
        // REPRIS mais **repoussés juste avant le « + »** : leur ordre relatif
        // survivait, leur PLACE non. Constaté par l'utilisateur — « le changement
        // disparaît au redémarrage » : un séparateur posé en tête revenait en fin
        // de rangée.
        //
        // ⇒ L'ORDRE VIENT MAINTENANT DE `elements`, qui le décrit en entier (voir
        // `config::ElementBarre`). La fonction ne devine plus rien.
        //
        // ⚠️ **LA PLAGE RESTE « DOSSIERS SEULEMENT », LES SÉPARATEURS SONT
        // AU-DESSUS** (`sep_base = base + total`) : c'est ce qui fait qu'un rang de
        // `dossier_slot` désigne toujours un DOSSIER (voir `app::dossier_au_rang`).
        let sep_base = base + total;
        let n_separateurs = elements.len() - count;
        // On ne fait que REDIMENSIONNER le bloc : ses icônes existantes gardent
        // leurs propriétés (sorte, taille), qui viennent de `zones.toml` et des
        // réglages de l'utilisateur — les recréer les perdrait à chaque appel.
        if self.icons.len() > sep_base {
            self.icons.truncate(sep_base.max(sep_base.min(self.icons.len())));
        }
        while self.icons.len() < sep_base + n_separateurs {
            self.icons.push(Self::icone_de_separateur());
        }
        if self.icons.len() > sep_base + n_separateurs {
            self.icons.truncate(sep_base + n_separateurs);
        }

        self.bottom_order.clear();
        let (mut dossier_suivant, mut separateur_suivant) = (base, sep_base);
        for element in elements {
            let index = match element {
                ElementBarre::Dossier(_) => {
                    let index = dossier_suivant;
                    dossier_suivant += 1;
                    index
                }
                ElementBarre::Separateur(sorte) => {
                    let index = separateur_suivant;
                    separateur_suivant += 1;
                    // ⛔ **LA SORTE VIENT DE L'ÉLÉMENT, PAS DU RANG** — voir
                    // `config::ElementBarre::Separateur`, qui porte la mesure. C'est ce
                    // qui fait qu'insérer un séparateur ne déguise plus les autres : la
                    // sorte ne dépend plus d'une position qui bouge.
                    //
                    // La case EXISTE déjà (le bloc a été rempli juste au-dessus) —
                    // `set_spacer_sorte` est un no-op sur une case absente, donc l'ordre
                    // de ces deux étapes compte.
                    self.set_spacer_sorte(index, *sorte);
                    index
                }
            };
            self.bottom_order.push(index);
        }
        // LE « + » EN DERNIER, APRÈS TOUT — séparateurs compris. Il est la dernière
        // case de la PLAGE (`base + count`), et `plus_dossier_index` le calcule
        // ainsi : le poser ailleurs dans la liste ferait diverger les deux.
        self.bottom_order.push(base + count);

        // ⚠️ **MÊME PURGE QUE `set_dynamic_apps`, ET POUR LA MÊME RAISON** : le
        // `truncate` ci-dessus peut RÉTRÉCIR la liste (on retire un dossier), et
        // un index de cette plage qui traînerait dans une AUTRE barre
        // deviendrait HORS BORNES — c'est le plantage mesuré du 2026-09-22
        // (`index out of bounds: the len is 173 but the index is 174`).
        // `bottom_order` est reconstruit juste au-dessus, donc il n'a pas besoin
        // d'être purgé ; les trois autres, si.
        let existantes = self.icons.len();
        for ordre in [&mut self.left_order, &mut self.right_order, &mut self.top_left_order, &mut self.top_right_order] {
            ordre.retain(|i| *i < existantes);
        }
        base
    }

    /// **L'ICÔNE D'UN RACCOURCI DE LA BARRE DU BAS** — la case que `set_dossiers`
    /// remplit, et que l'appelant d'`inserer` doit construire lui-même.
    ///
    /// ⚠️ **ELLE EST PARTAGÉE, ET C'EST TOUT L'INTÉRÊT** : `inserer` insère un `Icon`
    /// que L'APPELANT fabrique (voir sa doc), donc sans cette fonction l'épinglage à
    /// chaud recopierait les quatorze champs de la case — et un dossier neuf serait
    /// dessiné autrement que ses voisins dès le premier ajustement de l'un des deux.
    ///
    /// **Repli muet si la vignette ne se résout pas** : la case doit se lire comme
    /// VIDE, pas comme un carré blanc (même choix que
    /// `set_dynamic_apps`/`ensure_winpin_capacity`).
    ///
    /// ⚠️ **`external_texture: true` EST POSÉ ICI, ET L'APPELANT D'`inserer` DOIT LE
    /// RÉAFFIRMER** (il le remet à `vignette.is_some() || repli.is_some()`) : un dossier
    /// sans rien à dessiner doit retomber sur l'aplat « emplacement vide » plutôt que
    /// sur une case blanche opaque.
    pub fn icone_de_dossier() -> Icon {
        Icon {
            label: 'd',
            color: { let [r, g, b] = color::menu_fg(); [r, g, b, 0.25] },
            has_texture: false,
            external_texture: true,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0,
            zone_lead: 0.0,
            zone_columns: DEFAULT_ZONE_COLUMNS,
            zone_locked: false,
            zone_show_name: false,
            zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        }
    }

    /// **LE NOMBRE DE SÉPARATEURS CITÉS PAR LA BARRE DU BAS** — ceux qui vivent
    /// AU-DESSUS de sa plage (voir `set_dossiers`).
    fn separateurs_cites(&self) -> usize {
        let Some(fin) = self.plage_fin(Plage::Dossiers) else { return 0 };
        self.bottom_order.iter().filter(|&&index| index > fin).count()
    }

    /// **AJOUTE UNE CASE DE SÉPARATEUR À LA FIN DE SON BLOC** — pour le geste qui en
    /// pose un À CHAUD, sans recomposer toute la rangée.
    ///
    /// Rend l'index de la case neuve, que l'appelant cite ensuite dans
    /// `bottom_order` **À LA PLACE VOULUE** : c'est l'ORDRE qui décide de la
    /// position, jamais l'index — la case neuve peut donc se poser ici, tout au
    /// bout, et apparaître n'importe où dans la rangée.
    ///
    /// ⚠️ **CE QUI VIT AU-DESSUS GLISSE D'UN CRAN** : le miroir de notification et
    /// les applications ouvertes sont alloués APRÈS, donc leurs index changent, et
    /// `app::run` relit leurs bases à chaque frame. Une base laissée en arrière
    /// routerait leurs clics sur la MAUVAISE icône — même règle qu'`inserer`.
    pub fn ajouter_une_case_de_separateur(&mut self) -> usize {
        let at = match self.plage_fin(Plage::Dossiers) {
            Some(fin) => fin + 1 + self.separateurs_cites(),
            None => self.icons.len(),
        };
        self.icons.insert(at, Self::icone_de_separateur());
        for base in [&mut self.tray_base, &mut self.dynamic_base] {
            if let Some(borne) = base.as_mut() {
                if *borne >= at {
                    *borne += 1;
                }
            }
        }
        // ⛔ **LES CINQ LISTES D'ORDRE AUSSI — ET C'EST LE CŒUR DU DÉFAUT, MESURÉ.**
        //
        // `icons.insert` décale d'un cran TOUT ce qui vit à `at` ou au-delà ; les deux
        // BASES ci-dessus en tiennent compte, les listes d'ordre NON. Une barre
        // latérale qui citait un index ≥ `at` se mettait donc à citer **la case neuve**.
        //
        // ⇒ **L'ESPACE DEVENAIT CITÉ PAR DEUX BARRES.** `icon_side` répond par la
        // PREMIÈRE de sa chaîne (la latérale), donc `is_locked_at` consulte le verrou de
        // CETTE barre — et refuse un clic fait dans la barre du BAS. Mesuré le
        // 2026-10-01 : `cite_par=[Right, Bottom]`, `barre=Some(Right)`, `verrou=true`,
        // « clic droit REFUSE sur l'espace index 237 ». Défaut signalé : « je ne peux pas
        // TOUT LE TEMPS cliquer sur les points de séparateur de gauche » — « pas tout le
        // temps », parce que seuls les espaces doublement cités étaient refusés.
        //
        // ⚠️ `orders_mut()` COUVRE LES CINQ BARRES, la barre du bas comprise : c'est le
        // remède déjà retenu pour les opérations « sur toutes les barres », et il évite
        // d'en oublier une — l'erreur que ce fichier a déjà payée deux fois.
        for ordre in self.orders_mut() {
            for index in ordre.iter_mut() {
                if *index >= at {
                    *index += 1;
                }
            }
        }
        at
    }

    /// **CITE `index` À LA POSITION `position` DE LA RANGÉE** — l'autre moitié du
    /// geste ci-dessus, et le seul chemin par lequel une case neuve apparaît.
    ///
    /// `position` est une position **d'emplacement** : `bottom_order` a un
    /// représentant par élément, dans le même ordre (voir `set_dossiers`), le « + »
    /// fermant la liste.
    pub fn citer_dans_la_barre_du_bas(&mut self, position: usize, index: usize) {
        let position = position.min(self.bottom_order.len());
        self.bottom_order.insert(position, index);
    }

    /// **L'ICÔNE D'UN SÉPARATEUR** — la case que `set_dossiers` alloue à la suite de
    /// la plage des dossiers, et que `insert_spacer` pose à la frontière persistable.
    ///
    /// Partagée comme `icone_de_dossier` l'est pour les dossiers : deux littéraux
    /// recopiés divergeraient au premier ajustement de l'un.
    pub fn icone_de_separateur() -> Icon {
        Icon {
            label: ' ',
            color: [0.0, 0.0, 0.0, 0.0],
            has_texture: false,
            external_texture: false,
            is_spacer: true,
            spacer_size: SPACER_HEIGHT,
            zone_height: 0.0,
            zone_lead: 0.0,
            zone_columns: DEFAULT_ZONE_COLUMNS,
            zone_locked: false,
            zone_show_name: false,
            zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        }
    }

    /// Base des emplacements de la barre du bas (voir `set_dossiers`).
    pub fn dossiers_base(&self) -> Option<usize> {
        self.dossiers_base
    }

    /// **LE NOMBRE DE RACCOURCIS DE DOSSIERS QUE LE DOCK PORTE** — `None` tant que
    /// `set_dossiers` n'a pas alloué la plage, c'est-à-dire exactement quand
    /// `plage_base(Plage::Dossiers)` rend `None`.
    ///
    /// ⛔ **C'EST CE NOMBRE QUI GOUVERNE LES TABLEAUX DE TEXTURES, ET JAMAIS
    /// `dossiers.len()`.** Les deux ont divergé dès le premier épinglage à chaud — la
    /// liste en mémoire grandissait, le dock non — et boucler sur le mauvais a coûté un
    /// `0xc0000409` sans une ligne au journal (voir `app.rs`, l'ancien `dossier_slots`,
    /// qui était le contournement de cette divergence, et qui disparaît avec l'étape 4).
    ///
    /// Le dock est la seule autorité sur sa plage : c'est la doctrine de l'étape 2
    /// (*« c'est lui qui sait où finit sa plage »*), appliquée au COMPTE.
    ///
    /// ⚠️ **Le pendant existe déjà pour l'autre sens** : `plage_fin(Plage::Dossiers)` rend
    /// `base + compte`, et le compte seul s'obtient en soustrayant `dossiers_base()`.
    /// Cette fonction le dit d'un coup, sans faire porter l'addition à l'appelant.
    pub fn dossiers_count(&self) -> Option<usize> {
        self.dossiers_base.map(|_| self.dossiers_count)
    }

    /// Le raccourci de dossier porté par `index` — `None` pour le bouton « + »
    /// (qui est à `base + count`), pour un index hors plage, ou si la barre du
    /// bas n'a pas été garnie.
    /// **L'EMPLACEMENT D'UNE ICÔNE DANS SA PLAGE** — voir `Plage`.
    ///
    /// ⛔ **C'EST LA SEULE FAÇON DONT UN APPELANT DOIT OBTENIR UN INDEX.** Tant
    /// qu'`app.rs` garde de son côté un `*_icon_base` et calcule `base + i`, il
    /// détient une POSITION, et une insertion la rend fausse en silence. Le but du
    /// refactor (voir `docs/plans/dock-identites-stables.md`) est que plus personne
    /// n'en détienne.
    ///
    /// ⚠️ **ÉTAPE 1, ET ELLE NE CHANGE RIEN** : cette fonction rend exactement ce
    /// que `base + i` rendait, à ceci près qu'elle REFUSE le débordement
    /// (`checked_add`) là où `base + i` débordait en silence. Aucun appelant ne s'en
    /// sert encore — voir l'étape 2 du plan.
    #[allow(dead_code)] // étape 1 : les appelants viennent à l'étape 2 (voir le plan)
    pub fn emplacement(&self, plage: Plage, i: usize) -> Option<usize> {
        self.plage_base(plage)?.checked_add(i)
    }

    /// **LA BORNE DE FIN D'UNE PLAGE** — l'index juste APRÈS son dernier élément.
    ///
    /// ⚠️ **EMMÉNAGÉE ICI LE 2026-09-29, ET C'EST LE POINT** : `app.rs` calculait
    /// `winpin_icon_base + winpins.len()` à DEUX endroits, pour dire au dock
    /// lui-même jusqu'où il pouvait recréer des espaces. Le dock est le seul à
    /// savoir où finit sa plage ; le lui faire recalculer par son appelant, c'est
    /// exactement la position mémorisée que ce refactor supprime.
    ///
    /// ⚠️ **`Plage::Dynamiques` REND `None`** : cette plage n'a pas de compte — elle
    /// se termine par un `truncate` sur sa propre base, donc sa fin est celle de la
    /// liste des icônes, qui n'est pas une propriété de la plage. Le dire `None` vaut
    /// mieux qu'un nombre inventé.
    pub fn plage_fin(&self, plage: Plage) -> Option<usize> {
        let count = match plage {
            Plage::WinPins => self.winpin_count,
            Plage::Dossiers => self.dossiers_count,
            Plage::Notification => self.tray_count,
            Plage::Dynamiques => return None,
        };
        self.plage_base(plage)?.checked_add(count)
    }

    /// **LE RANG D'UN INDEX DANS SA PLAGE** — l'inverse d'`emplacement`, et le seul
    /// chemin par lequel un appelant doit retrouver « la n-ième épingle » à partir
    /// d'un index de clic.
    ///
    /// ⚠️ **C'est la GÉNÉRALISATION DE `dossier_slot`, qui existait déjà** et faisait
    /// exactement cela pour les dossiers : `index - base`, borné par le compte. Les
    /// huit sites d'`app.rs` qui écrivent `index - winpin_icon_base` à la main font
    /// la même chose **sans contrôle de bornes** — un index hors plage y rendait un
    /// rang absurde au lieu de `None`.
    pub fn rang_dans(&self, plage: Plage, index: usize) -> Option<usize> {
        let fin = self.plage_fin(plage)?;
        let rang = index.checked_sub(self.plage_base(plage)?)?;
        (index < fin).then_some(rang)
    }

    /// La base d'une plage — `None` tant qu'elle n'a pas été allouée.
    ///
    /// ⚠️ **`WinPins` A ÉTÉ AJOUTÉE AU DOCK POUR CETTE FONCTION** : sa base était
    /// seulement RENDUE par `ensure_winpin_capacity`, et mise en cache par
    /// l'appelant — c'est-à-dire exactement le défaut que ce refactor supprime.
    ///
    /// ⚠️ **`pub` DEPUIS LE 2026-09-29 POUR `cef_settings`** : le panneau Réglages
    /// lisait cette base dans un FICHIER (`winpin_base.txt`) — il la demande
    /// maintenant au dock, qui est la source, donc une valeur qui ne peut plus être
    /// périmée. **Le dock primaire est bien celui que `cef_settings` reçoit** (voir
    /// `app.rs`, `SettingsRefs { dock: &mut dock, … }`) : la substitution est
    /// équivalente — vérifié, parce qu'un dock SECONDAIRE, lui, a ses propres bases
    /// et aurait rendu ce remplacement faux en silence.
    ///
    /// ⚠️ **LE FICHIER, LUI, RESTE ÉCRIT ET ENCORE LU** : `app.rs` s'en sert au
    /// démarrage, AVANT que le dock n'existe, pour ne recréer les espaces que
    /// jusqu'à cette base (voir `save_winpin_base`, appelé juste à côté). Ce n'est
    /// donc PAS la même valeur « qui transitait par le disque pour rien » — j'ai
    /// failli le croire et retirer l'écriture, ce qui aurait laissé des espaces
    /// fantômes au redémarrage suivant.
    pub fn plage_base(&self, plage: Plage) -> Option<usize> {
        match plage {
            Plage::WinPins => self.winpin_base,
            Plage::Dossiers => self.dossiers_base,
            Plage::Notification => self.tray_base,
            Plage::Dynamiques => self.dynamic_base,
        }
    }

    pub fn dossier_slot(&self, index: usize) -> Option<usize> {
        // ⚠️ **CE CORPS EST DEVENU `rang_dans`** (2026-09-29) : c'était le motif juste,
        // écrit une seule fois, pour une seule plage. Les huit sites d'`app.rs` qui
        // font `index - winpin_icon_base` à la main le refaisaient sans contrôle de
        // bornes — ils ont désormais la même fonction, et la même correction.
        self.rang_dans(Plage::Dossiers, index)
    }

    /// L'index du bouton « + » — `None` si la barre du bas n'a pas été garnie.
    pub fn plus_dossier_index(&self) -> Option<usize> {
        Some(self.dossiers_base? + self.dossiers_count)
    }

    /// Vrai si `index` est le bouton « + » de la barre du bas.
    pub fn is_plus_dossier(&self, index: usize) -> bool {
        self.plus_dossier_index() == Some(index)
    }

    /// Déplace l'icône `index` juste après `parent` dans la barre de gauche —
    /// un profil de navigateur à la suite de son navigateur (voir
    /// `app::profile_parents`). Sans effet si l'une des deux n'y est pas : une
    /// icône glissée dans une autre barre y reste.
    pub fn place_after(&mut self, index: usize, parent: usize) {
        if index == parent || !self.left_order.contains(&parent) {
            return;
        }
        let Some(from) = self.left_order.iter().position(|&i| i == index) else { return };
        self.left_order.remove(from);
        let to = self.left_order.iter().position(|&i| i == parent).map_or(self.left_order.len(), |p| p + 1);
        self.left_order.insert(to, index);
    }

    /// Fixe le nom d'exécutable de chaque application dynamique — à appeler
    /// juste après `set_dynamic_apps`, avec le MÊME compte.
    pub fn set_dynamic_exes(&mut self, exes: Vec<String>) {
        self.dynamic_exes = exes;
    }

    /// Hauteur de TOUS les espaces invisibles actuellement connus (voir
    /// `Icon::is_spacer`/`spacer_size`), y compris ceux retirés d'une barre
    /// (voir `remove_spacer` — reste dans `icons`, juste plus dans aucun
    /// ordre) : exporté tel quel, sans filtrage, pour persister l'état
    /// complet (voir `config::save_spacer_sizes`) — même convention que
    /// `icon_order`.
    ///
    /// **Un SÉPARATEUR DE ZONE y figure comme les autres**, avec une valeur
    /// dont la disposition ne fait rien tant qu'il déclare une zone (voir
    /// `Icon::spacer_size`) : elle est conservée pour le jour où la zone serait
    /// défaite, pas parce qu'elle décrit ce qui est dessiné.
    pub fn spacer_sizes(&self) -> Vec<(usize, f32)> {
        self.icons.iter().enumerate().filter(|(_, icon)| icon.is_spacer).map(|(i, icon)| (i, icon.spacer_size)).collect()
    }

    /// Restaure les hauteurs d'espaces persistées (voir
    /// `config::load_spacer_sizes`) — à appeler juste après `set_icon_order`
    /// (l'espace référencé doit déjà exister CETTE session, voir la doc de
    /// `set_spacer_size` : no-op silencieux sinon, même dégradation
    /// gracieuse que `set_icon_order` pour un index devenu invalide).
    pub fn set_spacer_sizes(&mut self, sizes: &[(usize, f32)]) {
        for &(index, size) in sizes {
            self.set_spacer_size(index, size);
        }
    }

    /// Cote ET vide des séparateurs de zone, pour la persistance — jumeau de
    /// `spacer_sizes`, même convention (index brut, orphelins compris : un
    /// séparateur retiré garde son entrée dans `icons`). Seuls ceux dont la
    /// cote est `> 0` sortent : un espace ordinaire n'a rien à y faire, et le
    /// vide d'un élément qui ne déclare pas de zone n'en est pas un (voir
    /// `Icon::zone_lead`).
    ///
    /// Un TRIPLET `(index, cote, vide)` : le vide voyage avec la cote parce
    /// qu'ils ne servent à rien l'un sans l'autre, mais il reste le DERNIER
    /// champ, pour qu'un fichier écrit avant lui se relise sans le connaître
    /// (voir `config::load_zone_heights`).
    pub fn zone_heights(&self) -> Vec<(usize, f32, f32)> {
        self.icons
            .iter()
            .enumerate()
            .filter(|(_, icon)| icon.is_spacer && icon.zone_height > 0.0)
            .map(|(i, icon)| (i, icon.zone_height, icon.zone_lead))
            .collect()
    }

    /// Restaure les hauteurs de zone persistées (voir `zone_heights`) — jumeau de
    /// `set_spacer_sizes`, appelé juste après lui au démarrage.
    pub fn set_zone_heights(&mut self, heights: &[(usize, f32, f32)]) {
        for &(index, height, lead) in heights {
            self.set_zone_height(index, height);
            self.set_zone_lead(index, lead);
        }
    }

    /// Écartement actuel entre deux icônes consécutives (voir `icon_rects`,
    /// doc du champ `icon_gap`) — utilisé aussi par `app::run` pour calculer
    /// la "zone" pleine largeur de barre associée à une icône (voir
    /// `icon_zone_instance`), qui doit tiler exactement avec ce même
    /// espacement pour ne laisser ni chevauchement ni interstice.
    pub fn icon_gap(&self) -> f32 {
        self.icon_gap
    }

    /// Change l'écartement entre les icônes, repris au prochain rendu (voir
    /// `win32::settings_dash`, réglage utilisateur).
    pub fn set_icon_gap(&mut self, gap: f32) {
        self.icon_gap = gap;
    }

    /// L'écartement de la barre `barre` (rang dans `BARRES_ECARTEMENT`) : le
    /// sien s'il est réglé, sinon le général.
    pub fn icon_gap_barre(&self, barre: usize) -> f32 {
        self.icon_gaps_barres.get(barre).copied().flatten().unwrap_or(self.icon_gap)
    }

    /// Les écartements propres aux barres, `None` pour celles qui suivent le
    /// général (voir le champ `icon_gaps_barres`).
    pub fn icon_gaps_barres(&self) -> [Option<f32>; 4] {
        self.icon_gaps_barres
    }

    pub fn set_icon_gaps_barres(&mut self, gaps: [Option<f32>; 4]) {
        self.icon_gaps_barres = gaps;
    }

    /// L'écartement de la barre qui porte `side` — les deux moitiés du haut
    /// n'en font qu'une.
    pub fn icon_gap_side(&self, side: Side) -> f32 {
        self.icon_gap_barre(barre_ecartement(side))
    }

    /// L'écartement de la barre où vit l'icône `index` (le général si elle
    /// n'est dans aucune).
    pub fn icon_gap_de(&self, index: usize) -> f32 {
        self.icon_side(index).map_or(self.icon_gap, |side| self.icon_gap_side(side))
    }

    /// Taille actuelle des pastilles de bureaux virtuels DE CET ÉCRAN (voir
    /// la doc du champ `desktop_dot_size`).
    pub fn desktop_dot_size(&self) -> f32 {
        self.desktop_dot_size
    }

    /// Change la taille des pastilles de bureaux virtuels, repris au
    /// prochain rendu (voir `win32::cef_settings`, réglage utilisateur) —
    /// borné à 1px minimum, même garde que `Dock::set_spacer_size` (jamais
    /// une taille négative ou nulle qui rendrait la pastille invisible/
    /// inversée).
    pub fn set_desktop_dot_size(&mut self, size: f32) {
        self.desktop_dot_size = size.max(1.0);
    }

    /// Taille de police personnalisée des textes de la barre du haut DE CET
    /// ÉCRAN, `None` = automatique — voir la doc du champ `topbar_text_size`,
    /// que `app::run` et `dock_instance::DockInstance::sync_text` passent aux
    /// fonctions `ui::topbar::*_font_px`.
    pub fn topbar_text_size(&self) -> Option<f32> {
        self.topbar_text_size
    }

    /// Change la taille de police personnalisée des textes de la barre du haut,
    /// reprise au prochain rendu (voir `win32::cef_settings`, réglage
    /// utilisateur) — `None` remet le réglage AUTOMATIQUE.
    ///
    /// **Aucune borne ici, et c'est voulu** : le maximum dépend de l'épaisseur
    /// de la barre, qui peut changer APRÈS ce réglage (curseur d'épaisseur). Le
    /// borner maintenant le figerait sur une épaisseur périmée — c'est
    /// `topbar_text_font_px` qui borne à CHAQUE calcul, et
    /// `topbar_text_font_px_max` qui donne le maximum courant au curseur.
    pub fn set_topbar_text_size(&mut self, size: Option<f32>) {
        self.topbar_text_size = size;
    }

    /// Taille de l'indicateur « en cours d'exécution » (voir
    /// `Dock::running_dot_instance`) — DEMANDE EXPLICITE : « agrandir la taille
    /// de l'indicateur ».
    pub fn running_dot_size(&self) -> f32 {
        self.running_dot_size
    }

    /// Borné par `MIN`/`MAX_RUNNING_DOT_SIZE` — le minimum le garde visible, le
    /// maximum l'empêche de recouvrir entièrement l'icône qu'il marque.
    pub fn set_running_dot_size(&mut self, size: f32) {
        self.running_dot_size = size.clamp(MIN_RUNNING_DOT_SIZE, MAX_RUNNING_DOT_SIZE);
    }

    /// Décalage horizontal de l'indicateur (voir `running_dot_offset`).
    pub fn running_dot_offset(&self) -> f32 {
        self.running_dot_offset
    }

    /// Borné par `MAX_RUNNING_DOT_OFFSET` dans les deux sens.
    pub fn set_running_dot_offset(&mut self, px: f32) {
        self.running_dot_offset = if px.is_finite() { px.clamp(-MAX_RUNNING_DOT_OFFSET, MAX_RUNNING_DOT_OFFSET) } else { 0.0 };
    }

    /// Forme de l'indicateur — DEMANDE EXPLICITE : « changer sa forme ».
    pub fn running_dot_shape(&self) -> RunningDotShape {
        self.running_dot_shape
    }

    pub fn set_running_dot_shape(&mut self, shape: RunningDotShape) {
        self.running_dot_shape = shape;
    }

    /// Couleur de l'indicateur, `None` = suit l'accentuation du dock (voir la
    /// doc du champ) — DEMANDE EXPLICITE : « changer sa couleur ».
    pub fn running_dot_color(&self) -> Option<[f32; 4]> {
        self.running_dot_color
    }

    pub fn set_running_dot_color(&mut self, color: Option<[f32; 4]>) {
        self.running_dot_color = color;
    }

    /// Couleur de fond PARTAGÉE — source de tout ce qui a besoin d'une
    /// couleur "d'accentuation" générale ailleurs que sur le rendu des
    /// barres elles-mêmes (voir `bar_color` pour ce dernier, doc du champ
    /// `background`) : figée au démarrage (couleur d'accentuation Windows
    /// détectée, ou `color::CUSTOM_BACKGROUND`), plus aucun réglage ne la
    /// change ensuite (chaque barre a désormais son propre bouton "Couleur
    /// d'accentuation Windows", voir `win32::settings_dash`) — pas de
    /// mutateur correspondant, volontairement.
    pub fn background(&self) -> [f32; 4] {
        self.background
    }

    /// Couleur de fond EFFECTIVE de la barre `index` (haut=0, gauche=1,
    /// droite=2, bas=3, voir `bar_colors`) — sa propre couleur si elle en a une,
    /// sinon la couleur partagée (voir `background`).
    pub fn bar_color(&self, index: usize) -> [f32; 4] {
        self.bar_colors.get(index).copied().flatten().unwrap_or(self.background)
    }

    /// Donne à la barre `index` sa propre couleur, indépendante des autres
    /// (voir `win32::settings_dash`) — `index` hors bornes (ne
    /// devrait pas arriver en pratique, seuls 0/1/2/3 sont jamais produits)
    /// ignoré silencieusement plutôt que de paniquer pour un simple réglage
    /// visuel.
    pub fn set_bar_color(&mut self, index: usize, color: [f32; 4]) {
        if let Some(slot) = self.bar_colors.get_mut(index) {
            *slot = Some(color);
        }
    }

    /// Couleurs actuelles des quatre barres (voir `bar_color`), dans l'ordre
    /// haut/gauche/droite/bas — pour persister l'état complet (voir
    /// `config::save_bar_colors`) ou l'envoyer au panneau de réglages (voir
    /// `win32::settings_dash::SettingsSnapshot`).
    pub fn bar_colors(&self) -> [[f32; 4]; 4] {
        std::array::from_fn(|i| self.bar_color(i))
    }

    /// Restaure les couleurs par barre persistées (voir
    /// `config::load_bar_colors`) — à appeler juste après `Dock::new`, avant
    /// le premier rendu.
    pub fn set_bar_colors(&mut self, bar_colors: [Option<[f32; 4]>; 4]) {
        self.bar_colors = bar_colors;
    }

    /// Couleur des séparateurs choisie par l'utilisateur (voir
    /// `separator_color`) — `None` : suit la couleur principale du texte de
    /// la barre (résolu par l'appelant, voir `app::run`).
    pub fn separator_color(&self) -> Option<[f32; 4]> {
        self.separator_color
    }

    /// Fixe/efface la couleur des séparateurs (voir `separator_color`) —
    /// `None` pour revenir au suivi automatique de la couleur du texte.
    pub fn set_separator_color(&mut self, color: Option<[f32; 4]>) {
        self.separator_color = color;
    }

    /// Diamètre courant du petit cercle indicateur des espaces invisibles
    /// (voir `separator_size`, champ de struct).
    pub fn separator_size(&self) -> f32 {
        self.separator_size
    }

    /// Fixe le diamètre du petit cercle indicateur des espaces invisibles
    /// (voir `separator_size`) — DEMANDE EXPLICITE ("ajoute la possibilité
    /// de modifier la taille des séparateurs par interface"). Borné comme
    /// `set_desktop_dot_size`/`set_icon_gap` : une valeur hors bornes reçue
    /// (curseur JS, fichier de config manuel) ne peut ni faire disparaître
    /// le cercle (`0` ou négatif) ni déborder de l'emplacement de l'espace
    /// (voir `MIN_SEPARATOR_SIZE`/`MAX_SEPARATOR_SIZE`).
    pub fn set_separator_size(&mut self, size: f32) {
        self.separator_size = size.clamp(MIN_SEPARATOR_SIZE, MAX_SEPARATOR_SIZE);
    }

    /// Largeurs des trois emplacements de la rangée de widgets (voir
    /// `slot_widths`, champ de struct) — lues par la chaîne de disposition ET
    /// par le dessin, qui les reçoivent en ARGUMENT plutôt que d'aller les
    /// chercher (`app::dots_left_offset`, `app::media_left_offset`).
    pub fn slot_widths(&self) -> crate::ui::topbar::SlotWidths {
        self.slot_widths
    }

    /// Fixe les séparateurs des trois intervalles d'un coup (voir `slot_widths`)
    /// — les TROIS, parce qu'ils forment une seule valeur (`SlotWidths`) et que
    /// les fichiers de configuration les écrivent et les relisent ensemble.
    ///
    /// **Passe par `ui::topbar::canonical`**, qui borne chaque largeur ET repose
    /// l'invariant « les rangs occupés sont `0..n`, sans trou ». C'est LE point
    /// d'entrée unique — un curseur JS, un fichier écrit à la main et le
    /// chargement du démarrage y passent tous —, donc aucun chemin ne peut
    /// laisser un intervalle non canonique dans un `Dock`. Même règle que
    /// `set_separator_size` ci-dessus.
    pub fn set_slot_widths(&mut self, widths: crate::ui::topbar::SlotWidths) {
        self.slot_widths = crate::ui::topbar::canonical(widths);
    }

    pub fn on_mouse_move(&mut self, x: f32, y: f32, _hwnd: HWND) {
        self.mouse_pos = Some((x, y));
    }

    pub fn on_mouse_leave(&mut self, _hwnd: HWND) {
        self.mouse_pos = None;
    }

    /// Position courante de la souris (coordonnées fenêtre), `None` si hors
    /// fenêtre — exposée pour que `app::run` puisse animer la magnification
    /// des icônes qui vivent EN DEHORS du dock (voir `HoverAnimator`) avec la
    /// même position, sans dupliquer son propre suivi de `Event::MouseMove`.
    pub fn mouse_pos(&self) -> Option<(f32, f32)> {
        self.mouse_pos
    }

    /// Index de l'icône sous `(x, y)`, quel que soit le bouton de la
    /// souris à l'origine de l'appel (clic gauche pour lancer, clic droit
    /// pour le menu contextuel — voir `app::run`). `(x, y)` sont DÉJÀ des
    /// coordonnées FENÊTRE (celles fournies par `WM_LBUTTONDOWN`/`WM_
    /// LBUTTONUP`/`WM_RBUTTONDOWN` — voir `win32::window::wndproc`/
    /// `secondary_wndproc`, `loword(lparam)`/`hiword(lparam)`), exactement
    /// le système utilisé par `icon_rects_map` — BUG RÉEL SIGNALÉ ("les
    /// icones... de l'interface secondaire ne sont pas cliquable") :
    /// l'ancien `ScreenToClient(_hwnd, ...)` ici traitait `(x, y)` comme
    /// des coordonnées ÉCRAN à reconvertir en coordonnées fenêtre, alors
    /// qu'elles l'étaient DÉJÀ — une double conversion. Invisible sur la
    /// barre PRIMAIRE (dont l'origine écran vaut (0, 0) ou en est très
    /// proche, voir `hwnd_screen_origin`, doc de fonction, pour ce même
    /// piège déjà rencontré ailleurs), mais brisait le test sur tout écran
    /// SECONDAIRE, dont l'origine écran est arbitraire. `_hwnd` n'a donc
    /// plus aucun usage ici, gardé dans la signature pour ne pas changer
    /// tous les appelants.
    pub fn icon_at(&self, x: f32, y: f32, layout: &DockLayout, _hwnd: HWND) -> Option<usize> {
        // L'icône MAGNIFIÉE est testée EN PREMIER, jamais en repli — DEMANDE
        // EXPLICITE (« qu'elles passent au premier plan par rapport aux autres
        // icônes ») : elle est dessinée PAR-DESSUS ses voisines (voir `app::run`,
        // son lot y est poussé en dernier), donc c'est elle qui doit gagner le
        // clic là où sa boîte grossie recouvre la case d'une voisine. L'ordre
        // inverse produirait exactement le contraire : à 2.5x la boîte fait
        // 240px et CONTIENT ENTIÈREMENT les cases des voisines, si bien qu'un
        // clic sur le HAUT de l'icône grossie lancerait l'icône du dessus.
        //
        // Aucun risque d'oscillation : le survol est animé par
        // `HoverAnimator::update` contre les cases NON grossies (voir
        // `Dock::update`), un chemin de code entièrement distinct de celui-ci —
        // `icon_at` n'alimente jamais l'animateur.
        //
        // Les deux tests portent sur ce qui est DESSINÉ : la partie cachée d'une
        // icône défilée n'est pas cliquable (voir `cases_de_la_souris`).
        if let Some(index) = self.magnified_index() {
            let zones = self.zone_clips(layout);
            if let Some(rect) = self.presented_rect(index, layout).and_then(|r| self.rogner_a_ce_qui_est_dessine(index, r, &zones, layout)) {
                if hit(&rect, x, y) {
                    return Some(index);
                }
            }
        }
        let rects = self.cases_de_la_souris(layout);
        rects.iter().position(|r| r.is_some_and(|r| hit(&r, x, y)))
    }

    /// Rectangle (coordonnées fenêtre) de l'icône `index`, si elle existe —
    /// utilisé pour décaler le menu contextuel à côté de l'icône cliquée
    /// plutôt qu'au point de clic exact (voir `win32::menu`). `None` aussi
    /// pour l'icône ACTUELLEMENT glissée (voir `icon_rects_map`) : son
    /// rectangle "flottant" vient de `drag_preview`, pas de celui-ci.
    ///
    /// **C'est la case AU REPOS**, pas ce qui est dessiné : voir
    /// `presented_rect` pour le rectangle réellement à l'écran.
    pub fn icon_rect(&self, index: usize, layout: &DockLayout) -> Option<[f32; 4]> {
        self.icon_rects_map(layout).get(index).copied().flatten()
    }

    /// Les cases (coordonnées fenêtre) de toutes les icônes visibles, espaces
    /// exceptés — ce que le relevé de la couleur des barres ne doit pas lire
    /// (voir `gfx::releve_barres::relever`).
    pub fn cases_des_icones(&self, layout: &DockLayout) -> Vec<[f32; 4]> {
        self.icon_rects_map(layout)
            .into_iter()
            .enumerate()
            .filter(|&(i, _)| !self.icons[i].is_spacer)
            .filter_map(|(_, r)| r)
            .collect()
    }

    /// Rectangle RÉELLEMENT dessiné à l'écran pour l'icône `index` : sa case
    /// grossie par la magnification courante, puis TRANSLATÉE pour tenir
    /// entièrement dans la fenêtre — DEMANDE EXPLICITE : « il faudrait que les
    /// icones magnifiées se déplacent un peu vers le centre de l'écran pour ne
    /// pas être tronquées ».
    ///
    /// C'est la SEULE source de vérité de ce qui est à l'écran : le dessin (voir
    /// `external_icon_instance`/`render_instances`, qui la reçoivent avec
    /// `scale = 1.0`), le clic (voir `icon_at`), l'ancrage des popups et le
    /// débordement de région (voir `win32::window::Window::set_overflow`). Un
    /// seul nombre porte toute la transformation, donc ces consommateurs ne
    /// peuvent pas diverger — c'est ce qui rend le décalage sûr.
    pub fn presented_rect(&self, index: usize, layout: &DockLayout) -> Option<[f32; 4]> {
        let barre = BarreTirage::de_la_side(self.icon_side(index)?);
        Some(presented_rect_pour_la_barre(self.icon_rect(index, layout)?, self.hover.scale(index), layout, barre))
    }

    /// Index de l'icône actuellement MAGNIFIÉE, s'il y en a une — la plus
    /// grossie s'il y en a plusieurs : il y en a au plus une en régime établi,
    /// deux le temps que l'animation croise entre l'icône quittée et l'icône
    /// survolée, et c'est alors la plus grossie qui est visuellement au-dessus.
    ///
    /// Se déduit de l'échelle ANIMÉE, jamais de la position de la souris : au
    /// moment d'un `WM_LBUTTONDOWN`, `mouse_pos` peut être en retard d'une
    /// frame alors que l'échelle est déjà celle de l'icône sous le curseur.
    ///
    /// **Les espaces invisibles sont exclus** : leur marqueur est un petit
    /// cercle centré (voir `spacer_marker_instance`) qui ne quitte jamais sa
    /// case, donc rien à promouvoir ni à faire déborder — et `icon_at`/
    /// `hovered_icon` les excluent déjà, si bien que les laisser ici ferait
    /// signifier deux choses différentes à « magnifié ».
    pub fn magnified_index(&self) -> Option<usize> {
        (0..self.icons.len())
            .filter(|&i| !self.icons[i].is_spacer && self.hover.scale(i) > 1.0)
            .max_by(|&a, &b| self.hover.scale(a).total_cmp(&self.hover.scale(b)))
    }

    /// Rectangles (coordonnées fenêtre) à faire DÉBORDER de la région pour
    /// toutes les icônes actuellement magnifiées, à l'échelle **VISÉE**, avec la
    /// BARRE qui porte chacune (pour savoir quel fond lui donner — voir
    /// `app::run`).
    ///
    /// L'appelant en fait l'UNION pour la région de la fenêtre : un croisement
    /// entre deux icônes en magnifie deux à la fois, et les deux doivent
    /// déborder — n'en couvrir qu'une rognerait l'autre.
    ///
    /// L'échelle visée, et non l'échelle animée, est ce qui permet à la région
    /// de ne PAS être reposée à chaque frame d'animation : la boîte est un
    /// majorant constant de ce qui est dessiné pendant toute la croissance, et
    /// elle n'est retirée qu'une fois l'animation RETOMBÉE — c'est-à-dire quand
    /// `HoverAnimator` recolle à `1.0` (voir `SCALE_SNAP_EPSILON`), jamais dès
    /// que le pointeur s'en va. La retirer plus tôt rognerait l'icône pendant
    /// toute sa décroissance, au moment précis où le débordement sert.
    ///
    /// **C'est aussi la boîte du FOND à poser dessous** (voir `app::run`) : les
    /// deux DOIVENT être calculées de la même façon, sinon l'écart entre elles
    /// laisse apparaître les pixels noirs de la fenêtre (un swap chain en modèle
    /// flip n'a pas de transparence par pixel, voir `app::run`).
    pub fn magnified_boxes(&self, layout: &DockLayout) -> Vec<([f32; 4], Side)> {
        let target = hover_scale();
        (0..self.icons.len())
            .filter(|&i| !self.icons[i].is_spacer && self.hover.scale(i) > 1.0)
            .filter_map(|i| {
                let barre = BarreTirage::de_la_side(self.icon_side(i)?);
                let rect = presented_rect_pour_la_barre(self.icon_rect(i, layout)?, target, layout, barre);
                // La boîte OCCUPÉE, et non celle de l'icône : la région doit
                // couvrir les COINS de l'icône dans toutes ses positions de
                // balancement, sinon ils seraient rognés en tournant (voir
                // `occupied_rect_for`).
                Some((occupied_rect_for(rect), self.icon_side(i)?))
            })
            .collect()
    }

    /// Barre CONTENANT l'icône `index`, `None` si `index` est hors bornes (ne
    /// devrait pas arriver en pratique).
    ///
    /// **Publique** parce que l'appelant en a besoin pour découper : une icône
    /// latérale peut sortir de sa barre en défilant, une icône de la barre du
    /// haut non (voir `lateral_clip`, et `gfx::quad_renderer::QuadBatch::clipped`).
    /// Ne pas déduire la barre du RECTANGLE de l'icône : l'icône peut y être
    /// n'importe où (une pastille « ouverte » est dessinée SOUS son icône), et
    /// se tromper couperait une icône de la barre du haut.
    pub fn icon_side(&self, index: usize) -> Option<Side> {
        if self.left_order.contains(&index) {
            Some(Side::Left)
        } else if self.right_order.contains(&index) {
            Some(Side::Right)
        } else if self.top_left_order.contains(&index) {
            Some(Side::TopLeft)
        } else if self.top_right_order.contains(&index) {
            Some(Side::TopRight)
        } else if self.top_middle_order.contains(&index) {
            // ⛔ **CETTE BRANCHE MANQUAIT, ET C'EST LA MÊME LEÇON QUE `Bottom`
            // JUSTE EN DESSOUS** — mesurée le 2026-10-30 par l'utilisateur : « j'ai essayé
            // de déplacer l'applet réseau à côté de l'horloge et elle a disparu ».
            // `render_instances` fait `let Some(side) = self.icon_side(i) else { continue }` :
            // sans côté, l'icône est SAUTÉE EN SILENCE. Elle avait son rectangle
            // (`Self::ZONES`), sa liste (`icones_milieu`) et son fichier — et aucun côté,
            // donc aucun dessin. Une fonction qui rend `Option` ne réveille le compilateur
            // sur RIEN : c'est l'usage qui l'a trouvée, comme pour `Bottom`.
            Some(Side::TopMiddle)
        } else if self.bottom_order.contains(&index) {
            // ⛔ **CETTE BRANCHE MANQUAIT, ET RIEN NE POUVAIT LE DIRE.** Comme
            // `side_at`, cette fonction rend un `Option<Side>` : ce n'est PAS un
            // `match` exhaustif, donc l'ajout du membre `Side::Bottom` n'a fait
            // lever aucun avertissement — là où les dix-huit `match` du fichier
            // ont tous été signalés.
            //
            // Conséquence MESURÉE : les raccourcis de dossiers étaient chargés,
            // leur place allouée (`base du MIROIR = 182` = 180 + un dossier + le
            // « + »), le fichier écrit — et **rien ne se dessinait**.
            // `render_instances` fait `let Some(side) = self.icon_side(i) else {
            // continue }` : sans côté, l'icône est SAUTÉE, en silence.
            //
            // C'est la leçon de ce chantier : **une fonction qui rend
            // `Option<Side>` est un `match` exhaustif déguisé, et l'énumération
            // ne le sait pas**. Les deux (`icon_side`, `side_at`) ont été
            // trouvées par l'usage, pas par le compilateur.
            Some(Side::Bottom)
        } else {
            None
        }
    }

    fn order(&self, side: Side) -> &Vec<usize> {
        match side {
            Side::Left => &self.left_order,
            Side::Right => &self.right_order,
            Side::TopLeft => &self.top_left_order,
            Side::TopRight => &self.top_right_order,
            Side::TopMiddle => &self.top_middle_order,
            Side::Bottom => &self.bottom_order,
        }
    }

    fn order_mut(&mut self, side: Side) -> &mut Vec<usize> {
        match side {
            Side::Left => &mut self.left_order,
            Side::Right => &mut self.right_order,
            Side::TopLeft => &mut self.top_left_order,
            Side::TopRight => &mut self.top_right_order,
            Side::TopMiddle => &mut self.top_middle_order,
            Side::Bottom => &mut self.bottom_order,
        }
    }

    /// **LES CINQ LISTES D'ORDRE, EN MUTABLE** — le pendant d'`orders()` (qui les
    /// rend en lecture) pour ce qui doit toucher **TOUTES** les barres.
    ///
    /// ⛔ **ELLE EXISTE PARCE QU'UNE ÉNUMÉRATION ÉCRITE À LA MAIN A ÉTÉ OUBLIÉE
    /// DEUX FOIS.** Les opérations « sur toutes les barres » listaient les champs
    /// un par un, et la barre ajoutée en dernier n'y figurait jamais :
    /// - `remove_spacer` retirait l'index de `left_order`/`right_order` — un espace
    ///   posé dans une barre du **HAUT** ne partait plus ;
    /// - corrigé en quatre listes… et la barre du **BAS**, arrivée ensuite, n'a
    ///   jamais rejoint la liste : un espace posé là ne se retirait plus non plus.
    ///
    /// **Ni le compilateur ni un test ne le signalaient** — et c'est pire qu'un
    /// `match` exhaustif déguisé : il n'y a même pas d'énumération à mettre à jour,
    /// juste un champ qu'on peut oublier d'écrire.
    ///
    /// ⇒ **Toute opération qui doit toucher toutes les barres passe par ici.** La
    /// question « ai-je pensé à toutes les barres ? » disparaît : il n'y a plus de
    /// liste à rallonger.
    fn orders_mut(&mut self) -> [&mut Vec<usize>; 5] {
        [
            &mut self.left_order,
            &mut self.right_order,
            &mut self.top_left_order,
            &mut self.top_right_order,
            &mut self.bottom_order,
        ]
    }

    /// `true` si `index` est un espace invisible (voir `Icon::is_spacer`) —
    /// `false` aussi pour un index hors bornes plutôt que de paniquer.
    pub fn is_spacer(&self, index: usize) -> bool {
        self.icons.get(index).is_some_and(|i| i.is_spacer)
    }

    /// **L'ICÔNE EST-ELLE MASQUÉE ?** (hors bornes : faux). Un PLACEHOLDER masqué — les
    /// places d'épingle réservées, voir `masquer_les_places_libres` — n'a pas d'identité et
    /// ne se dessine jamais : la répartition ne doit donc pas le compter comme un trou
    /// (voir `repartir_les_icones`, paramètre `visible`).
    /// **POSE LA LARGEUR DE L'HORLOGE** (voir le champ) — appelée par `app::run` quand la
    /// texture de l'heure est régénérée, et à `0.0` quand il n'y a pas d'horloge.
    pub fn set_clock_width(&mut self, largeur: f32) {
        self.clock_width = largeur.max(0.0);
    }

    pub fn is_hidden(&self, index: usize) -> bool {
        self.icons.get(index).is_some_and(|icon| icon.hidden)
    }

    /// Active/désactive l'icône `index` (voir `Icon::hidden`, doc détaillée
    /// — demande explicite : "gérer les icones de la barre supérieure et
    /// les dashs comme des plugins activables ou désactivables") — NO-OP si
    /// `index` est hors bornes. Ne touche PAS `left_order`/`right_order` :
    /// l'icône reste à sa place dans l'ordre, juste exclue de la mise en
    /// page tant qu'elle est masquée (voir `layout_side`) — contrairement à
    /// une première version qui la retirait de l'ordre (voir la doc du
    /// champ, "BUG RÉEL SIGNALÉ").
    pub fn set_icon_hidden(&mut self, index: usize, hidden: bool) {
        if let Some(icon) = self.icons.get_mut(index) {
            icon.hidden = hidden;
        }
    }

    /// **MASQUE LES PLACES D'ÉPINGLE RÉSERVÉES** (`epingles..capacite`) — voir
    /// `ui::reserve_epingles`. Une place masquée ne prend aucune place dans la barre ;
    /// épingler à chaud la démasque, sans qu'aucun index ne bouge.
    pub fn masquer_les_places_libres(&mut self, epingles: usize, capacite: usize) {
        for rang in epingles..capacite {
            self.set_icon_hidden(crate::app::PINNED_ICON_BASE + rang, true);
        }
    }

    /// **RETIRE L'ÉPINGLE `rang` À CHAUD** — les suivantes reculent d'un cran, et son
    /// icône part au bout du bloc, masquée : elle redevient une place libre. Les
    /// applets (`PINNED_ICON_BASE + capacite..`) ne bougent pas.
    ///
    /// ⚠️ **LES ORDRES NE SONT PAS TOUCHÉS ICI** : c'est
    /// `decaler_le_bloc_des_epingles(…, retrait = true)` qui les fait suivre, borné à
    /// la capacité. Rend `false` sans rien toucher si `rang` n'est pas une épingle.
    pub fn retirer_une_epingle(&mut self, rang: usize, epingles: usize, capacite: usize) -> bool {
        let base = crate::app::PINNED_ICON_BASE;
        if rang >= epingles || epingles > capacite || base + capacite > self.icons.len() {
            return false;
        }
        self.icons[base + rang..base + capacite].rotate_left(1);
        let liberee = &mut self.icons[base + capacite - 1];
        liberee.hidden = true;
        liberee.external_texture = false;
        true
    }

    /// Hauteur ACTUELLE de l'espace `index` (voir `Icon::spacer_size`) —
    /// `None` si `index` n'est pas un espace (ou hors bornes), pour que
    /// l'appelant (voir `win32::popup_menu`, le curseur du menu ouvert sur
    /// un espace) sache distinguer "pas un espace" de "espace à 0px".
    pub fn spacer_size(&self, index: usize) -> Option<f32> {
        self.icons.get(index).filter(|i| i.is_spacer).map(|i| i.spacer_size)
    }

    /// Hauteur de zone de `index` (voir `Icon::zone_height`) — `None` si ce n'est
    /// pas un espace, `Some(0.0)` pour un espace ORDINAIRE. Jumeau de
    /// `spacer_size`.
    pub fn zone_height(&self, index: usize) -> Option<f32> {
        self.icons.get(index).filter(|i| i.is_spacer).map(|i| i.zone_height)
    }

    /// Fixe la hauteur de zone de `index` — no-op si ce n'est pas un espace,
    /// jamais négative. Jumeau de `set_spacer_size`. Une cote de `0.0` en
    /// refait un espace ORDINAIRE (voir `Icon::zone_height`), ce qui rend le
    /// geste de défaire réversible jusqu'au bout.
    ///
    /// ⛔ **MAIS AUCUN CURSEUR NE DOIT Y MENER — BUG RÉEL DU 2026-10-08.** Ils y
    /// menaient, et l'utilisateur a perdu sa zone de notification : « j'ai réduit
    /// la hauteur à zéro et ses réglages ont disparu ainsi que la zone ». Un `0`
    /// de HAUTEUR se lit « pas de hauteur », donc « cachée » — pas « détruite ».
    /// Les deux curseurs de cote sont donc PLANCHERÉS à 1 px, et faire disparaître
    /// une zone se dit par la case « Cacher cette zone », qui la laisse être une
    /// zone.
    /// ⚠️ **LES GESTES, EUX, GARDENT LE 0, DÉLIBÉRÉMENT** : le glisser d'un bord
    /// (voir `DragAdjust::ZoneCote`) et Maj + clic droit
    /// (`toggle_zone_separator`). On y voit la zone se défaire sous la main ; un
    /// contrôle discret qui détruit, lui, ne pardonne pas l'aller-retour.
    pub fn set_zone_height(&mut self, index: usize, height: f32) {
        if let Some(icon) = self.icons.get_mut(index) {
            if icon.is_spacer {
                icon.zone_height = height.max(0.0);
            }
        }
        // Une cote ramenée à 0 DÉFAIT la zone : oublier du même coup où elle
        // avait défilé. Sans cela, la refaire ramènerait son contenu à cette
        // position-là — une mémoire que rien ne justifie, et qui se lirait
        // comme un défaut (« la zone se rouvre déjà défilée »).
        if self.zone_height(index).unwrap_or(0.0) <= 0.0 {
            self.zone_scroll.retain(|&(sep, _)| sep != index);
            // Le vide AUSSI (voir `Icon::zone_lead`) : il ne s'applique qu'à un
            // séparateur de zone, donc le laisser en place ferait rouvrir la
            // zone plus bas qu'avant, une mémoire que rien ne justifie.
            self.set_zone_lead(index, 0.0);
        }
    }

    /// Défilement propre de la zone ouverte par le séparateur `sep` — `0.0`
    /// pour une zone qui n'a jamais défilé (voir `zone_scroll`, table creuse).
    fn zone_scroll_offset(&self, sep: usize) -> f32 {
        self.zone_scroll.iter().find(|&&(index, _)| index == sep).map_or(0.0, |&(_, offset)| offset)
    }

    /// Fixe le défilement propre de la zone ouverte par `sep`, en CRÉANT son
    /// entrée au besoin, et jamais négatif (voir `zone_scroll`).
    fn set_zone_scroll_offset(&mut self, sep: usize, offset: f32) {
        match self.zone_scroll.iter_mut().find(|(index, _)| *index == sep) {
            Some(entry) => entry.1 = offset.max(0.0),
            None => self.zone_scroll.push((sep, offset.max(0.0))),
        }
    }

    /// Vide inséré AVANT le séparateur `index` (voir `Icon::zone_lead`) —
    /// `None` si ce n'est pas un espace, jumeau de `zone_height`.
    pub fn zone_lead(&self, index: usize) -> Option<f32> {
        self.icons.get(index).filter(|i| i.is_spacer).map(|i| i.zone_lead)
    }

    /// Fixe le vide précédant le séparateur `index` — no-op si ce n'est pas un
    /// espace, jamais négatif. Jumeau de `set_zone_height`.
    pub fn set_zone_lead(&mut self, index: usize, lead: f32) {
        if let Some(icon) = self.icons.get_mut(index) {
            if icon.is_spacer {
                icon.zone_lead = lead.max(0.0);
            }
        }
    }

    /// Séparateur de la zone qui PRÉCÈDE `index` dans sa barre — celui dont la
    /// cote fixe le bas juste au-dessus de lui, donc celui qu'un glisser simple
    /// sur `index` doit régler (voir `DragAdjust::PreviousZoneCote`).
    ///
    /// `None` s'il n'y en a aucun — `index` est alors le PREMIER séparateur de
    /// sa barre (ou de la barre, s'il n'y en a qu'un) : rien au-dessus de lui
    /// n'est réglable, et c'est son propre `zone_lead` que le geste déplace.
    fn previous_zone_separator(&self, index: usize) -> Option<usize> {
        let side = self.icon_side(index).filter(|side| side.is_lateral())?;
        let order = self.order(side);
        let position = order.iter().position(|&i| i == index)?;
        order[..position].iter().rev().find(|&&i| self.is_zone_separator(i)).copied()
    }

    /// **CE QUE DESSINE CET ESPACE** — `None` si `index` n'est pas un espace, comme
    /// `spacer_size` et `zone_height` (voir leurs docs). Un espace ordinaire rend
    /// donc `Some(Rond)`, jamais `None` : « quel espace » et « quelle sorte » sont
    /// deux questions distinctes, et les confondre ferait passer un espace normal
    /// pour une icône.
    pub fn spacer_sorte(&self, index: usize) -> Option<SorteEspace> {
        self.icons.get(index).filter(|icon| icon.is_spacer).map(|icon| icon.sorte)
    }

    /// Change la sorte d'un espace — no-op sur ce qui n'est PAS un espace, comme
    /// `set_spacer_size` : régler la sorte d'une icône ordinaire ne voudrait rien
    /// dire, et la valeur ne serait jamais lue.
    ///
    /// ⚠️ **DEVENIR UN TRAIT DONNE AUSSI SA LARGEUR** — DEMANDE EXPLICITE
    /// (2026-09-30) : « les nouveaux séparateurs trait n'ajoutent qu'un espace de
    /// 8 pixels ». Un trait EST le séparateur : il n'a pas besoin du blanc de
    /// `SPACER_HEIGHT` (18 px) qui, lui, sépare à lui seul quand le marqueur est
    /// invisible. Huit pixels, c'est la place du trait et un peu d'air.
    ///
    /// ⚠️ **SEULEMENT SUR UN CHANGEMENT DE SORTE** : repasser par « Trait » alors
    /// qu'on l'est déjà ne doit pas écraser une largeur que l'utilisateur vient de
    /// régler au curseur — deux commandes de la même grandeur ne peuvent pas se
    /// contredire en silence.
    pub fn set_spacer_sorte(&mut self, index: usize, sorte: SorteEspace) {
        if let Some(icon) = self.icons.get_mut(index).filter(|icon| icon.is_spacer) {
            let change = icon.sorte != sorte;
            icon.sorte = sorte;
            if change && sorte == SorteEspace::Trait {
                icon.spacer_size = SPACER_TRAIT_PX;
            }
        }
    }

    /// `true` si `index` est un espace qui DÉCLARE une zone (voir
    /// `Icon::zone_height`) — `false` pour une icône ordinaire comme pour un
    /// espace ordinaire, la seule question que posent le geste de
    /// redimensionnement (« quelle grandeur règle-t-il ? ») et la persistance
    /// (« quelles cotes exporter ? », voir `zone_heights`).
    pub fn is_zone_separator(&self, index: usize) -> bool {
        self.zone_height(index).is_some_and(|height| height > 0.0)
    }

    /// Nombre de colonnes DEMANDÉ pour la zone ouverte par `index` — `None` si
    /// ce n'est pas un espace, jumeau de `zone_height`. C'est la valeur
    /// STOCKÉE, telle que l'utilisateur l'a réglée : ce que la barre en accorde
    /// réellement se lit par `effective_zone_columns`.
    pub fn zone_columns(&self, index: usize) -> Option<u16> {
        self.icons.get(index).filter(|i| i.is_spacer).map(|i| i.zone_columns)
    }

    /// Fixe le nombre de colonnes de la zone ouverte par `index` — no-op si ce
    /// n'est pas un espace. Jamais 0 : une zone sans colonne n'aurait pas de
    /// contenu du tout, et le plancher rend le geste réversible sans jamais
    /// faire disparaître ce qu'il règle.
    pub fn set_zone_columns(&mut self, index: usize, columns: u16) {
        if let Some(icon) = self.icons.get_mut(index) {
            if icon.is_spacer {
                icon.zone_columns = columns.max(1);
            }
        }
    }

    /// Nombre de colonnes qu'une barre LARGE DE `bar_width` peut réellement
    /// accueillir — le plancher de `MIN_ZONE_COLUMN_PX` par colonne, jamais
    /// moins d'une. **Fonction PURE**, et c'est elle qui porte la règle : le
    /// plafond du curseur (voir `win32::cef_spacer_size`) et la disposition
    /// (voir `layout_side`) l'appellent tous les deux, donc l'interface ne peut
    /// pas promettre une colonne que la disposition refuserait.
    pub fn max_zone_columns(bar_width: f32) -> u16 {
        (bar_width / plancher_de_colonne()).floor().max(1.0) as u16
    }

    /// Colonnes RÉELLEMENT appliquées à la zone ouverte par `index` dans une
    /// barre large de `bar_width` : ce que l'utilisateur a demandé, ramené à ce
    /// que la barre permet (voir `max_zone_columns`). `1` pour tout ce qui n'est
    /// pas un séparateur de zone — donc l'empilement d'origine.
    ///
    /// **Le plafond est appliqué ICI, à la lecture, et jamais au stockage** :
    /// une barre rétrécie doit pouvoir reprendre ses colonnes quand on
    /// l'élargit de nouveau, sans que l'utilisateur ait à les redemander.
    pub fn effective_zone_columns(&self, index: usize, side: Side, bar_width: f32) -> u16 {
        // LA GRILLE NE CONCERNE QUE LA BARRE DE DROITE — DEMANDE EXPLICITE :
        // « les zones définies par les séparateurs de droite ». Une zone de la
        // barre de GAUCHE s'empile, quel que soit le nombre de colonnes qu'elle
        // déclarerait : le réglage ne lui est pas offert (voir
        // `win32::cef_spacer_size`, qui ne le propose que d'un côté), et il ne
        // doit pas y produire d'effet par accident — un séparateur déplacé
        // d'une barre à l'autre emporte son réglage.
        if side != Side::Right || !self.is_zone_separator(index) {
            return DEFAULT_ZONE_COLUMNS;
        }
        self.zone_columns(index).unwrap_or(DEFAULT_ZONE_COLUMNS).min(Self::max_zone_columns(bar_width))
    }

    /// La grille d'une barre, sous la forme que consomme `bar_layout` : pour un
    /// séparateur, ses colonnes EFFECTIVES et la taille de case qui en découle.
    ///
    /// **Une seule définition, et c'est le point** : la disposition dessinée
    /// (`layout_side`), le découpage des zones (`zone_clips`), le bornage du
    /// défilement (`reclamp_scroll`) et le test de clic (`zone_separator_at`)
    /// mesurent tous la MÊME grille — sans quoi une zone serait cliquable
    /// ailleurs que là où elle se voit.
    fn zone_grid(&self, side: Side, icon_size: f32, bar_width: f32) -> impl Fn(usize) -> (u16, f32) + '_ {
        move |sep| {
            let columns = self.effective_zone_columns(sep, side, bar_width);
            (columns, Self::zone_cell_size(icon_size, bar_width, columns))
        }
    }

    /// Bord GAUCHE et largeur d'une barre LATÉRALE — **une seule définition**,
    /// pour la même raison que `zone_grid` : une grille se compte depuis ce bord
    /// et se répartit sur cette largeur, donc ce qui DESSINE (`layout_side`) et
    /// ce qui décide d'un LÂCHER (`drag_insert_index`) doivent lire les mêmes
    /// deux nombres. Deux `match` séparés seraient deux endroits à corriger le
    /// jour où une barre change de géométrie.
    fn lateral_bar_span(side: Side, layout: &DockLayout) -> (f32, f32) {
        match side {
            Side::Left => (0.0, layout.left_width),
            Side::Right => (layout.total_width - layout.right_width, layout.right_width),
            Side::TopLeft | Side::TopRight | Side::TopMiddle | Side::Bottom => {
                unreachable!("lateral_bar_span ne gère que les barres LATÉRALES, voir Side")
            }
        }
    }

    /// Colonnes EFFECTIVES de l'emplacement `slot` — `DEFAULT_ZONE_COLUMNS` hors
    /// zone, sinon celles que la barre accorde à la zone qui le contient (voir
    /// `effective_zone_columns`).
    ///
    /// Le rang de zone vient de `slot.zone` : c'est `bar_layout` qui sait à
    /// quelle zone appartient un emplacement, et le redemander ailleurs ferait
    /// un second endroit qui croit le savoir.
    fn slot_columns(&self, bar: &BarLayout, side: Side, bar_width: f32, slot: &Slot) -> u16 {
        slot.zone
            .and_then(|rank| bar.zones.get(rank))
            .map_or(DEFAULT_ZONE_COLUMNS, |zone| self.effective_zone_columns(zone.sep_index, side, bar_width))
    }

    /// Abscisse et largeur d'un emplacement DANS SA BARRE — **la seule
    /// définition**, partagée par ce qui dessine et par ce qui décide où tombe
    /// un lâcher.
    ///
    /// C'est la propriété que ce fichier énonce déjà pour `zone_grid` : sans
    /// elle, une case serait visée ailleurs que là où elle se voit. Elle a bel
    /// et bien été violée — `drag_insert_index` ne comparait que l'ORDONNÉE,
    /// donc deux cases d'une même rangée se confondaient, et une grille à N
    /// colonnes ne changeait rien au geste.
    ///
    /// Un ESPACE, et tout ce qui n'est pas dans une grille, garde l'abscisse
    /// ordinaire `centered_x` à la taille `icon_size` : il occupe une rangée
    /// entière et son marqueur se centre dans la barre, comme hors grille.
    fn slot_geometry(
        side: Side,
        layout: &DockLayout,
        icon_size: f32,
        centered_x: f32,
        columns: u16,
        slot: &Slot,
        is_spacer: bool,
    ) -> (f32, f32) {
        if columns > 1 && !is_spacer {
            let (column_left, column_width) = Self::column_band(side, layout, columns, slot.column);
            (column_left + (column_width - slot.height).max(0.0) * 0.5, slot.height)
        } else {
            (centered_x, icon_size)
        }
    }

    /// Bande HORIZONTALE de la colonne `column` d'une grille à `columns`
    /// colonnes — `(bord gauche, largeur)`.
    ///
    /// **Les colonnes PAVENT la barre sans laisser de trou** (`largeur / colonnes`,
    /// bout à bout) : c'est ce qui rend cette bande utilisable comme test
    /// d'appartenance — « la case sous le curseur » — là où la case DESSINÉE,
    /// plus étroite et centrée dans sa colonne (voir `slot_geometry`), laisserait
    /// de chaque côté un jeu qu'aucune colonne ne revendiquerait.
    fn column_band(side: Side, layout: &DockLayout, columns: u16, column: usize) -> (f32, f32) {
        let (bar_left, bar_width) = Self::lateral_bar_span(side, layout);
        let column_width = bar_width / columns.max(1) as f32;
        (bar_left + column_width * column as f32, column_width)
    }

    /// Colonne dont la BANDE contient `x` — la PREMIÈRE si `x` est en deçà de la
    /// barre, la DERNIÈRE s'il est au-delà. C'est l'inverse de `column_band`, et
    /// elle s'appuie dessus : les deux ne peuvent donc pas diverger.
    ///
    /// Sert au lâcher : « la case sous le curseur » se dit
    /// `colonne_sous(x) == slot.column`, puisque les bandes PAVENT la barre.
    fn colonne_sous(side: Side, layout: &DockLayout, columns: u16, x: f32) -> usize {
        let derniere = columns.max(1) as usize - 1;
        (0..=derniere)
            .find(|&col| {
                let (gauche, largeur) = Self::column_band(side, layout, columns, col);
                x >= gauche && x < gauche + largeur
            })
            .unwrap_or_else(|| {
                let (gauche, _) = Self::column_band(side, layout, columns, 0);
                if x < gauche {
                    0
                } else {
                    derniere
                }
            })
    }

    /// Abscisse CENTRÉE d'un élément hors grille — l'unique colonne d'une barre
    /// qui n'en découpe pas, et tout ce qui précède la première zone (voir
    /// `layout_side`).
    ///
    /// **Une seule formule pour les deux barres** : `lateral_bar_span` rend déjà
    /// le bord gauche et la largeur, et `bord + (largeur - icône) / 2` est
    /// exactement ce que chaque bras du `match` écrivait à la main.
    fn centered_icon_x(side: Side, layout: &DockLayout, icon_size: f32) -> f32 {
        let (bar_left, bar_width) = Self::lateral_bar_span(side, layout);
        bar_left + (bar_width - icon_size) * 0.5
    }

    /// Rang d'une barre dans les tableaux PAR BARRE (`bar_locked`,
    /// `BAR_THICKNESS_MIN_4K`, `bar_thicknesses.txt`) : `[haut, gauche, droite]`.
    ///
    /// Les DEUX zones de la barre du haut n'en font qu'UNE — c'est une seule
    /// barre vue de l'utilisateur, et c'est ainsi que les Réglages la
    /// présentent. **À ne pas confondre avec `side_slot`**, qui indexe `scroll`
    /// sur les seules barres latérales : deux tableaux, deux indexations.
    fn bar_rank(side: Side) -> usize {
        match side {
            Side::TopLeft | Side::TopRight | Side::TopMiddle => 0,
            Side::Left => 1,
            Side::Right => 2,
            // ⚠️ **EN FIN, rang 3** : `bar_rank` indexe les tableaux par barre
            // (`bar_color`, `bar_locked`, `controller()`), et `BarreTirage::rang`
            // comme `bar_rects` doivent donner le MÊME numéro.
            Side::Bottom => 3,
        }
    }

    /// Vraie si la barre `side` est VERROUILLÉE — DEMANDE EXPLICITE : « une
    /// option dans les réglages pour verrouiller les barres contre tout
    /// changement pour éviter les erreurs ». Voir `is_locked_at`, le prédicat
    /// que les gestes consultent réellement.
    pub fn is_bar_locked(&self, side: Side) -> bool {
        // ⚠️ **`.get()` ET NON `[ ... ]`** : `bar_rank` rend un rang par variante
        // de `Side`, et ce tableau porte une case par barre. Les deux comptent
        // aujourd'hui le même nombre (5 variantes → rangs 0..3, tableau de 4),
        // donc ceci résout TOUJOURS. La garde reste pour la fois suivante : une
        // barre ajoutée sans élargir le tableau ferait paniquer le dock au
        // premier clic droit, ce qui est un prix absurde pour un réglage
        // d'apparence.
        self.bar_locked.get(Self::bar_rank(side)).copied().unwrap_or(false)
    }

    /// Vraie si la barre du HAUT est verrouillée — **le verrou que consultent les
    /// SÉPARATEURS DE LA RANGÉE DE WIDGETS** (voir `ui::topbar::TopSlot`).
    ///
    /// **POURQUOI UN PRÉDICAT NOMMÉ, et pas `is_bar_locked(Side::TopLeft)` écrit
    /// dans le geste** : la doc d'`is_locked_at` dit elle-même qu'une règle
    /// recopiée dans quelques chemins « laisserait des trous que personne ne
    /// verrait — le verrou aurait l'air de marcher, sauf par un chemin précis ».
    /// Ce trou EXISTAIT : un séparateur de rangée n'a pas d'index, donc
    /// `is_locked_at` ne pouvait pas le couvrir, et il était le SEUL élément de
    /// mise en page de la barre du haut à ignorer le verrou — dans la même barre
    /// où créer un espace était refusé. Ici la règle n'est pas recopiée : elle
    /// DÉLÈGUE.
    ///
    /// **Une seule barre pour les deux moitiés** : `bar_rank` réunit `TopLeft` et
    /// `TopRight` (c'est une seule barre vue de l'utilisateur, et les Réglages la
    /// présentent ainsi) — les emplacements de la rangée, eux, se répartissent sur
    /// toute sa largeur.
    ///
    /// Le verrou ne cache RIEN : les marqueurs continuent d'être dessinés et
    /// survolés. Il refuse les GESTES, comme partout ailleurs.
    pub fn is_top_row_locked(&self) -> bool {
        self.is_bar_locked(Side::TopLeft)
    }

    /// Pose ou lève le verrou d'UNE barre.
    pub fn set_bar_locked(&mut self, side: Side, locked: bool) {
        // Même garde que `is_bar_locked`, et pour la même raison — voir sa doc.
        if let Some(slot) = self.bar_locked.get_mut(Self::bar_rank(side)) {
            *slot = locked;
        }
    }

    /// Les quatre verrous, dans l'ordre `[haut, gauche, droite, bas]` — pour la
    /// persistance (voir `config::save_bar_locks`).
    pub fn bar_locks(&self) -> [bool; 4] {
        self.bar_locked
    }

    /// Restaure les trois verrous persistés — jumeau de `set_bar_colors`,
    /// appelé au démarrage.
    pub fn set_bar_locks(&mut self, locks: [bool; 4]) {
        self.bar_locked = locks;
    }

    /// Vraie si `index` est un SÉPARATEUR DE ZONE FIGÉ — jumeau de
    /// `is_zone_separator`, et comme lui porté par le séparateur lui-même.
    pub fn is_zone_locked(&self, index: usize) -> bool {
        self.icons.get(index).is_some_and(|icon| icon.is_spacer && icon.zone_locked)
    }

    /// Fige ou libère la zone ouverte par le séparateur `index` — no-op si ce
    /// n'est pas un espace, comme tous les réglages d'espace.
    pub fn set_zone_locked(&mut self, index: usize, locked: bool) {
        if let Some(icon) = self.icons.get_mut(index) {
            if icon.is_spacer {
                icon.zone_locked = locked;
            }
        }
    }

    /// Index des séparateurs FIGÉS — pour la persistance. Une LISTE d'index, et
    /// non des paires « index,valeur » comme `zone_heights` : le verrou n'a
    /// qu'un état, et écrire « index,false » n'aurait aucun sens. Le fichier ne
    /// porte donc que ce qui est verrouillé, comme `spacer_sizes.txt` ne porte
    /// que ce qui a une taille.
    #[cfg(test)]
    pub fn zone_locks(&self) -> Vec<usize> {
        self.icons.iter().enumerate().filter(|(_, icon)| icon.is_spacer && icon.zone_locked).map(|(i, _)| i).collect()
    }

    /// La zone ouverte par `index` affiche-t-elle son numéro ou son nom ?
    /// Jumeau de `is_zone_locked`.
    pub fn is_zone_label_shown(&self, index: usize) -> bool {
        self.is_zone_separator(index) && self.icons.get(index).is_some_and(|icon| icon.zone_show_name)
    }

    /// Affiche ou masque le numéro/nom de la zone ouverte par `index` — no-op si
    /// ce n'est pas un SÉPARATEUR DE ZONE : une icône n'a pas de zone à étiqueter.
    pub fn set_zone_label_shown(&mut self, index: usize, shown: bool) {
        if !self.is_zone_separator(index) {
            return;
        }
        if let Some(icon) = self.icons.get_mut(index) {
            icon.zone_show_name = shown;
        }
    }

    /// Boîte de CHAQUE zone de la barre de DROITE, avec l'index de son
    /// séparateur — pour y DESSINER son numéro ou son nom (voir `zone_labels`).
    ///
    /// **Barre de droite uniquement**, comme tout ce qui touche aux zones : c'est
    /// la seule où elles se nomment, se verrouillent et se découpent en grille.
    ///
    /// Coordonnées FENÊTRE, mêmes que celles de `layout_side` et par le même
    /// calcul — c'est ce qui garantit que le texte tombe dans la boîte que le
    /// contenu respecte, et pas à côté.
    pub fn zone_boxes(&self, layout: &DockLayout) -> Vec<(usize, [f32; 4])> {
        let order: Vec<usize> = self.order(Side::Right).iter().copied().filter(|&i| !self.icons[i].hidden).collect();
        let bar_width = layout.right_width;
        let icon_size = lateral_icon_size_for(Side::Right, bar_width);
        let start_y = layout.haut_colonnes + TOP_MARGIN - self.decalage_barre(Side::Right, layout);
        let bar = self.bar_layout(Side::Right, 
            &order,
            icon_size,
            start_y,
            &|sep| self.zone_scroll_offset(sep),
            &self.zone_grid(Side::Right, icon_size, bar_width),
        );
        let bar_left = layout.total_width - bar_width;
        bar.zones
            .iter()
            .map(|zone| (zone.sep_index, [bar_left, zone.top, bar_width, (zone.bottom - zone.top).max(0.0)]))
            .collect()
    }

    /// **LE TEXTE d'une zone étiquetée** — `(index du séparateur, texte)`. Le nom
    /// s'il y en a un, sinon « Zone N ».
    ///
    /// **Rend l'INDEX DU SÉPARATEUR, PAS UNE POSITION** : c'est lui qui désigne
    /// la zone, et l'appelant en tire le rectangle par `zone_boxes`, puis la
    /// position du texte par `zone_label_origin` — laquelle est désormais
    /// **CENTRÉE SUR LE SÉPARATEUR** (demande explicite), et non plus posée
    /// dessous en en-tête de zone.
    ///
    /// **Le numéro vient de `zone_summaries`**, jamais d'un compteur local : il
    /// suit la POSITION (voir `ZoneSummary::number`), et deux compteurs
    /// finiraient par ne plus dire la même chose — le panneau Réglages
    /// afficherait « Zone 2 » là où la barre écrirait « Zone 3 ».
    pub fn zone_labels(&self) -> Vec<(usize, String)> {
        self.zone_summaries()
            .iter()
            .filter(|zone| self.is_zone_label_shown(zone.sep_index))
            .map(|zone| (zone.sep_index, zone.name.clone().unwrap_or_else(|| format!("Zone {}", zone.number))))
            .collect()
    }

    /// Le NOM donné à la zone ouverte par le séparateur `index` — `None` tant
    /// qu'elle n'en a pas. Voir `zone_names`, doc du champ.
    pub fn zone_name(&self, index: usize) -> Option<&str> {
        self.zone_names.iter().find(|(sep, _)| *sep == index).map(|(_, name)| name.as_str())
    }

    /// Nomme la zone ouverte par `index`, ou lui rend son numéro automatique si
    /// `name` est vide — DEMANDE EXPLICITE : « permettre de renommer une zone ».
    ///
    /// **Un nom vide EFFACE l'entrée** au lieu d'enregistrer une chaîne vide :
    /// les deux se liraient pareil à l'écran, mais seule la première rend la
    /// zone à la numérotation automatique (voir `ZoneSummary::number`).
    ///
    /// No-op sur ce qui n'est pas un SÉPARATEUR DE ZONE : nommer une icône ou un
    /// espace ordinaire ne voudrait rien dire, et le nom ne serait jamais montré.
    pub fn set_zone_name(&mut self, index: usize, name: Option<&str>) {
        if !self.is_zone_separator(index) {
            return;
        }
        let nomme = name.map(str::trim).filter(|name| !name.is_empty()).map(str::to_owned);
        self.zone_names.retain(|(sep, _)| *sep != index);
        if let Some(nomme) = nomme {
            self.zone_names.push((index, nomme));
        }
    }

    /// Les noms persistés — couples `(index, nom)`, pour la sauvegarde (voir
    /// `config::save_zone_names`).
    #[cfg(test)]
    pub fn zone_names(&self) -> Vec<(usize, String)> {
        self.zone_names.clone()
    }

    // --- Les zones, par CLÉ STABLE (voir `ui::zones`, doc de module) ------------------
    //
    // Une zone était désignée par l'INDEX de son séparateur, un nombre qui bouge à chaque
    // épingle ajoutée. Elle est maintenant désignée par une lettre de barre et un RANG parmi
    // les espaces de cette barre — ce qui ne bouge plus, parce qu'une épingle s'insère SOUS
    // les espaces.

    /// Les quatre listes d'ordre **VIVANTES** — pour la résolution et la sauvegarde.
    pub fn orders(&self) -> Orders<'_> {
        Orders::new(
            &self.left_order,
            &self.right_order,
            &self.top_left_order,
            &self.top_right_order,
            &self.bottom_order,
        )
        .avec_milieu(&self.top_middle_order)
    }

    /// **L'INDEX DE L'ESPACE DE RANG `key.rank` DANS LA BARRE `key.bar`**, lu dans la
    /// séquence passée.
    ///
    /// ⚠️ **LA SÉQUENCE EST UN PARAMÈTRE, ET C'EST TOUT LE MÉCANISME.** Au démarrage,
    /// `app::run` doit résoudre les clés AVANT `set_icon_order` — parce que c'est ce même
    /// appel qui rend les ordres persistés vivants. `Dock::order(side)` porte encore les
    /// listes PAR DÉFAUT de `Dock::new` à ce moment-là : il est FAUX. L'appelant passe donc
    /// les quatre listes locales qu'il vient de lire (`config::load_icon_order`), qui sont
    /// exactement celles que `set_icon_order` rendra vivantes une ligne plus bas.
    ///
    /// **Pure** : ne lit que `icons` (pour savoir ce qui est un espace) et la séquence
    /// donnée. Elle ne consulte JAMAIS la cote — voir `is_zone_separator`, qui est faux pour
    /// tous à ce moment-là, et c'est le piège que la doc d'`app::run` décrit.
    pub fn spacer_index_at_rank(&self, orders: Orders<'_>, key: ZoneKey) -> Option<usize> {
        index_of_key(orders, |index| self.is_spacer(index), key)
    }

    /// L'inverse de `spacer_index_at_rank`, dans les listes **vivantes** — la clé d'un
    /// espace. `None` pour un index qui n'est pas un espace, ou qu'aucune barre ne cite (une
    /// ORPHELINE, voir `zones`).
    pub fn zone_key_of(&self, index: usize) -> Option<ZoneKey> {
        key_of_index(self.orders(), |candidate| self.is_spacer(candidate), index)
    }

    /// Le nombre d'ESPACES d'une barre, dans ces listes — c'est-à-dire son nombre de rangs.
    pub fn bar_spacer_count(&self, orders: Orders<'_>, bar: BarLetter) -> usize {
        orders.of_bar(bar).filter(|&index| self.is_spacer(index)).count()
    }

    /// **APPLIQUE TOUT `zones.toml` EN UNE PASSE**, chaque réglage retrouvant son espace par
    /// sa clé — remplace les six `set_*` échelonnés du démarrage (`set_spacer_sizes` puis les
    /// cinq `set_zone_*`).
    ///
    /// ⚠️ **L'ORDRE DES APPELS EST PORTEUR, ET IL EST CELUI D'`app::run`.** La COTE vient
    /// avant les autres attributs de zone parce que c'est elle qui **déclare** la zone
    /// (`is_zone_separator` EST `zone_height > 0`) : `set_zone_name` et `set_zone_label_shown`
    /// ont cette garde, et les appeler avant la cote en ferait des **no-op silencieux**.
    ///
    /// **Les TAILLES D'ESPACE PASSENT EN PREMIER, et ce n'est pas un ordre porteur** : une
    /// taille et une cote sont deux champs DISTINCTS du même `Icon`, aucun des deux ne crée ni
    /// ne retire d'espace (c'était déjà dit du temps où les deux venaient de fichiers
    /// séparés). Elles ont simplement suivi `spacer_sizes.txt` dans `zones.toml` — voir
    /// `ui::zones`, décision 4.
    ///
    /// Un réglage dont la clé ne se résout sur aucun espace est IGNORÉ : c'est le cas d'une
    /// barre qui n'a pas encore été complétée (voir `compléter_les_barres`, à appeler AVANT),
    /// et la zone reviendra au prochain lancement — jamais perdue, puisque `zones.toml` n'est
    /// pas réécrit à partir de ce que le dock a pu appliquer.
    pub fn apply_zones(&mut self, orders: Orders<'_>, zones: &Zones) {
        // Les tailles, par rang. Une barre dont `sizes` ne fait pas la longueur du compte rend
        // `None` pour TOUS ses rangs (voir `Zones::size_at`) : elle est ignorée en entier, et
        // les espaces gardent la taille par défaut de `Dock::new`. C'est voulu — une liste de
        // tailles est tout ou rien, un compte faux déplacerait tous les rangs.
        for bar in BarLetter::ALL {
            // ⛔ **LA BARRE DU BAS EST EXCLUE — L'ARRANGEMENT PORTE SES SORTES** (voir
            // `config::ElementBarre::Separateur`, qui porte la mesure).
            //
            // ⚠️ **C'ÉTAIT LE TROU DU PREMIER CORRECTIF, ET IL S'EST VU À L'ÉCRAN** :
            // `Dock::zones()` avait cessé d'ÉCRIRE les sortes du bas, mais cette boucle
            // continuait de les RELIRE de `zones.toml` et de les réappliquer **PAR RANG**
            // — défaisant, après `set_dossiers`, ce qu'il venait de poser élément par
            // élément. Mesuré : `elastiques=12` et **aucun point dessiné**.
            //
            // Une propriété se retire des DEUX côtés à la fois : celui qui l'écrit et
            // celui qui la lit. N'en traiter qu'un laisse le défaut en place, simplement
            // plus difficile à voir.
            if bar == BarLetter::Bas {
                continue;
            }
            for rank in 1..=zones.spacer_count(bar) {
                let Some(key) = ZoneKey::new(bar, rank) else { continue };
                let Some(index) = self.spacer_index_at_rank(orders, key) else { continue };
                // **LA SORTE D'ABORD, LA TAILLE ENSUITE** — sans ordre imposé, mais
                // les deux se demandent à la MÊME clé : les séparer en deux boucles
                // ferait résoudre le rang deux fois, et un rang qui se résout
                // différemment entre les deux serait un défaut invisible.
                if let Some(sorte) = zones.sorte_at(key) {
                    self.set_spacer_sorte(index, sorte);
                }
                if let Some(size) = zones.size_at(key) {
                    self.set_spacer_size(index, size);
                }
            }
        }
        for (key, record) in &zones.zones {
            let Some(index) = self.spacer_index_at_rank(orders, *key) else { continue };
            self.set_zone_height(index, record.cote);
            self.set_zone_lead(index, record.lead);
            self.set_zone_columns(index, record.columns);
            self.set_zone_locked(index, record.locked);
            self.set_zone_name(index, record.name.as_deref());
            self.set_zone_label_shown(index, record.label);
            // **LE MASQUAGE PASSE PAR `Icon::hidden`** — c'est ce qui fait que la
            // disposition saute DÉJÀ le séparateur, et `bar_layout` n'a plus qu'à sauter
            // la PLAGE (voir `ZoneRecord::hidden`). La permanence a son propre drapeau.
            self.set_icon_hidden(index, record.hidden);
            if let Some(icon) = self.icons.get_mut(index) {
                icon.zone_permanent = record.permanent;
            }
            // ⚠️ **LA SORTE N'EST PAS ICI, ET C'EST MESURÉ** : cette boucle ne voit
            // que les espaces qui DÉCLARENT une zone, donc un séparateur ordinaire
            // n'y passe jamais. Elle voyage par `[bars.X] sortes`, appliquée par la
            // boucle des tailles juste au-dessus (voir `BarSpacers::sortes`).
        }
    }

    /// **L'ÉTAT COMPLET DES ZONES, par clé** — ce que `config::save_zones` écrit.
    ///
    /// ⚠️ **UN SÉPARATEUR QU'AUCUNE BARRE NE CITE N'EST PAS EXPORTÉ**, faute de clé. C'est
    /// délibéré, et c'est un changement par rapport à l'ancien `zone_heights()`, qui exportait
    /// les orphelins et les maintenait ainsi en vie indéfiniment. Un espace orphelin n'est
    /// **jamais dessiné** (`set_icon_order` ne réintègre jamais un espace), donc ses réglages
    /// n'ont aucun effet : les garder ne servait qu'à faire grossir les fichiers. Le jour de la
    /// migration, leurs réglages ont été RECRÉÉS comme zones réelles — rien n'est perdu.
    pub fn zones(&self) -> Zones {
        let orders = self.orders();
        let mut zones = Zones::default();

        for bar in BarLetter::ALL {
            let count = self.bar_spacer_count(orders, bar);
            // ⚠️ **UNE BARRE SANS ESPACE MAIS AVEC DES ICÔNES AU MILIEU EXISTE QUAND MÊME** :
            // c'est une liste à écrire (voir `BarSpacers::icones_milieu`). Sans cette
            // exception, une applet déposée au milieu d'une barre du haut SANS séparateur ne
            // serait jamais persistée.
            let milieu_non_vide = bar == BarLetter::Haut && !self.top_middle_order.is_empty();
            if count == 0 && !milieu_non_vide {
                continue;
            }
            // Une taille par rang. Un rang qui ne se résout pas (barre incomplète) prend le
            // défaut plutôt que de décaler la liste : `sizes` DOIT faire la longueur du
            // compte, sinon la relecture l'ignore EN ENTIER (voir `Zones::size_at`).
            let sizes = (1..=count)
                .map(|rank| {
                    ZoneKey::new(bar, rank)
                        .and_then(|key| self.spacer_index_at_rank(orders, key))
                        .and_then(|index| self.icons.get(index))
                        .map_or(SPACER_HEIGHT, |icon| icon.spacer_size)
                })
                .collect();
            // **UNE SORTE PAR RANG, PARALLÈLE AUX TAILLES** — et c'est ICI que se
            // règle le défaut mesuré : `Dock::zones()` ne construit un `ZoneRecord`
            // que pour un espace qui DÉCLARE une zone (voir la boucle suivante),
            // donc un séparateur ordinaire n'avait nulle part où porter sa sorte.
            // `[bars.X]` est écrit pour TOUTE barre qui a des espaces, zone ou pas.
            let sortes = (1..=count)
                .map(|rank| {
                    ZoneKey::new(bar, rank)
                        .and_then(|key| self.spacer_index_at_rank(orders, key))
                        .and_then(|index| self.icons.get(index))
                        .map_or(SorteEspace::Rond, |icon| icon.sorte)
                })
                .collect();
            // ⛔ **POUR LA BARRE DU BAS, `sizes` ET `sortes` RESTENT VIDES — C'EST
            // L'ARRANGEMENT QUI LES PORTE** (voir `config::ElementBarre::Separateur`, et
            // la mesure qui l'a décidé : une liste PAR RANG ne survit pas à une
            // insertion). On n'écrit plus que le COMPTE, dont la reprise a besoin.
            //
            // ⚠️ **VIDE PLUTÔT QUE RETIRÉ** : `size_at` et `sorte_at` rendent `None` dès
            // que la longueur ne vaut pas le compte, donc **rien ne s'applique** — et
            // `apply_zones` cesse de défaire, par rang, ce que `set_dossiers` vient de
            // poser élément par élément.
            //
            // Le jour où la TAILLE d'un espace du bas devra être réglable, elle suivra
            // le même chemin que la sorte : dans l'arrangement, pas ici.
            let (sizes, sortes) = if bar == BarLetter::Bas {
                (Vec::new(), Vec::new())
            } else {
                (sizes, sortes)
            };
            zones.bars.insert(
                bar,
                BarSpacers { spacers: count, sizes, sortes, icones: Vec::new(), icones_milieu: Vec::new(), icones_droite: Vec::new() },
            );
        }

        for (index, icon) in self.icons.iter().enumerate() {
            if !(icon.is_spacer && icon.zone_height > 0.0) {
                continue;
            }
            let Some(key) = self.zone_key_of(index) else { continue };
            zones.zones.insert(
                key,
                crate::ui::zones::ZoneRecord {
                    cote: icon.zone_height,
                    lead: icon.zone_lead,
                    name: self.zone_name(index).map(str::to_owned),
                    columns: icon.zone_columns,
                    locked: icon.zone_locked,
                    label: icon.zone_show_name,
                    // La VISIBILITÉ vit dans `Icon::hidden` (c'est elle qui fait sauter
                    // la plage, voir `bar_layout`), la permanence dans son propre
                    // drapeau — voir `ZoneRecord::hidden`/`permanent`.
                    hidden: icon.hidden,
                    permanent: icon.zone_permanent,
                    // ⚠️ LE DOCK NE CONNAIT PAS ENCORE LES IDENTITES RANGEES DANS
                    // LES ZONES (etape 3 du plan). Vide ici, et `config::
                    // zones_a_ecrire` REPORTE la liste lue du fichier : sauvegarder
                    // ne doit pas effacer ce que la migration a ecrit.
                    icones: Vec::new(),
                },
            );
        }

        // **`dash_add_zone` N'EST PAS REMPLI ICI, ET C'EST DÉLIBÉRÉ** : cette destination vit
        // dans la CONFIGURATION, pas dans le dock, et ce module ne fait aucune E/S. C'est
        // `config::zones_a_ecrire` qui compose l'état complet — le dock fournit ce qu'il
        // connaît, la configuration ce qu'elle connaît.
        zones
    }

    /// **COMPLÈTE CHAQUE BARRE jusqu'au nombre d'espaces que `zones.toml` déclare.**
    ///
    /// C'est le remplacement de l'ancienne moitié « cotes » de `Dock::saved_spacer_capacity`,
    /// et il a un vrai rôle : sans lui, un `zones.toml` présent alors qu'`icon_order.txt`
    /// manque ne résoudrait plus aucun rang, et la première sauvegarde **écraserait** le
    /// fichier — de l'érosion, pire que ce que faisait le code d'avant.
    ///
    /// ⚠️ **À APPELER APRÈS `set_icon_order`** : c'est le seul moment où les listes vivantes
    /// sont celles de la configuration (avant, ce sont les défauts de `Dock::new`).
    ///
    /// ⚠️ **PAR `add_spacer_at_end`, LE GESTE RÉEL**, qui passe par `insert_spacer` : il
    /// décale proprement tout ce qui suit. Recalculer un index à la main ici recopierait une
    /// connaissance qui n'appartient qu'à `insert_spacer`.
    ///
    /// ⚠️ **JAMAIS DE RETRAIT.** Une barre qui déclare MOINS d'espaces que sa liste n'en cite
    /// est laissée telle quelle : le compte peut être en retard d'une sauvegarde, et **créer
    /// est réparable, supprimer ne l'est pas**.
    pub fn compléter_les_barres(&mut self, zones: &Zones) {
        for bar in BarLetter::ALL {
            // ⛔ **LA BARRE DU BAS EST EXCLUE, ET C'EST ELLE QUI A CHANGÉ DE
            // SOURCE.** Son nombre de séparateurs ne vient plus de `[bars.B]` mais
            // de l'ARRANGEMENT (`dossiers.toml`, voir `config::ElementBarre`), et
            // c'est `set_dossiers` qui les matérialise. La laisser ici ferait DEUX
            // sources pour le même compte — et c'est exactement ce qui produisait
            // un séparateur fantôme à chaque lancement (voir `set_dossiers`).
            if bar == BarLetter::Bas {
                continue;
            }
            let declared = zones.spacer_count(bar);
            let mut present = self.bar_spacer_count(self.orders(), bar);
            while present < declared {
                self.add_spacer_at_end(bar.append_side());
                present += 1;
            }
        }
    }

    /// Position, dans l'ordre de la barre de **DROITE**, où insérer une icône
    /// pour qu'elle se range **à la fin de la zone ouverte par `sep`** — voir
    /// `cef_dash::handle_add_to_zone`, seul appelant.
    ///
    /// **POURQUOI UNE POSITION, ET NON UN COMPTE.** L'appartenance à une zone est
    /// *implicite dans l'ordre* : une zone est la suite d'icônes qui commence
    /// APRÈS son séparateur et s'arrête au séparateur suivant — même frontière
    /// que `zone_summaries` et `bar_layout`. `zone_summaries` rend un NOMBRE
    /// d'icônes, utile pour reconnaître une zone, mais qui ne dit pas OÙ
    /// insérer.
    ///
    /// Rend la position juste AVANT le séparateur suivant, ou la fin de l'ordre
    /// s'il n'y en a pas : c'est la lecture la plus littérale de « fin de la
    /// zone », et elle tient même quand la zone est vide — l'icône se pose alors
    /// juste après son séparateur.
    ///
    /// `None` si `sep` n'est pas (ou plus) dans la barre de droite : l'appelant
    /// doit le constater et refuser, jamais ranger au hasard.
    pub fn zone_end_order_position(&self, sep: usize) -> Option<usize> {
        // Un index qui n'ouvre PAS de zone n'a pas de « fin de zone » : mieux
        // vaut ne rien rendre que rendre une position PLAUSIBLE ET FAUSSE — le
        // cas se produirait, par exemple, sur un réglage devenu périmé parce que
        // la barre a été réordonnée.
        if !self.is_zone_separator(sep) {
            return None;
        }
        let order = self.order(Side::Right);
        let debut = order.iter().position(|&index| index == sep)?;
        Some(
            order[debut + 1..]
                .iter()
                .position(|&index| self.is_zone_separator(index))
                .map_or(order.len(), |offset| debut + 1 + offset),
        )
    }

    /// Ce que le panneau Réglages montre d'UNE zone de la barre de droite, pour
    /// la laisser verrouiller sans passer par le clic droit — DEMANDE EXPLICITE :
    /// « les deux, réunis au même endroit ».
    ///
    /// **Tout ce qu'il faut pour la RECONNAÎTRE**, et pas seulement son index :
    /// un index ne dit rien à personne, alors qu'une cote, un nombre d'icônes et
    /// un nombre de colonnes désignent la zone sans ambiguïté — deux zones
    /// peuvent partager une cote, rarement les trois.
    pub fn zone_summaries(&self) -> Vec<ZoneSummary> {
        let order = self.order(Side::Right);
        let mut out = Vec::new();
        // Compteur des zones SANS NOM, dans l'ordre du haut vers le bas : c'est
        // lui qui numérote, et il ne s'incrémente que pour elles.
        let mut unnamed = 0usize;
        for (rank, &index) in order.iter().enumerate() {
            if !self.is_zone_separator(index) {
                continue;
            }
            // Le contenu : ce qui SUIT, jusqu'au prochain séparateur de zone —
            // exactement la même frontière que `bar_layout`.
            //
            // **CE QU'ON VOIT, et rien d'autre** : ni les icônes masquées, ni
            // les ESPACES. DÉFAUT RÉEL SIGNALÉ — « dans les verrous pour la
            // première zone de droite il est marqué qu'il y a une icône, hors je
            // n'en vois pas » : le compte incluait un espace invisible, et le
            // libellé annonçait donc une icône là où l'écran n'en montre aucune.
            // Ce nombre sert à RECONNAÎTRE la zone ; compter ce qui ne se voit
            // pas va exactement contre son but.
            let content = order[rank + 1..]
                .iter()
                .take_while(|&&next| !self.is_zone_separator(next))
                .filter(|&&next| !self.icons[next].hidden && !self.icons[next].is_spacer)
                .count();
            // LE NOM, s'il y en a un — et le NUMÉRO, qui ne compte QUE les
            // zones sans nom : c'est ce qui fait qu'une zone renommée ne
            // consomme pas de numéro et que les suivantes se renumérotent sans
            // trou (voir `ZoneSummary::number`, doc).
            let name = self.zone_name(index).map(str::to_owned);
            let number = if name.is_some() {
                0
            } else {
                unnamed += 1;
                unnamed
            };
            out.push(ZoneSummary {
                sep_index: index,
                cote: self.zone_height(index).unwrap_or(0.0),
                columns: self.zone_columns(index).unwrap_or(DEFAULT_ZONE_COLUMNS),
                locked: self.is_zone_locked(index),
                // La visibilité vit dans `Icon::hidden` (c'est elle qui fait sauter la
                // plage, voir `ordre_visible`), la permanence dans son propre drapeau.
                hidden: self.icons[index].hidden,
                permanent: self.icons[index].zone_permanent,
                shows_label: self.is_zone_label_shown(index),
                content,
                name,
                number,
            });
        }
        out
    }

    /// **CE QU'UNE ZONE CONTIENT, DANS L'ORDRE DE LA BARRE — la LISTE, là où
    /// `zone_summaries` ne rend qu'un COMPTE.**
    ///
    /// DEMANDE EXPLICITE (2026-09-23) : « dans les réglages de zones je voudrais
    /// voir ce qui se trouve dedans, et avoir la possibilité de la cacher quand
    /// c'est une applet et de le retirer quand c'est un raccourci ».
    ///
    /// Même frontière que `bar_layout` et que le `content` ci-dessus — le
    /// séparateur suivant ferme la zone : une seule règle, trois consommateurs.
    ///
    /// ⚠️ **CE QUI EST LISTÉ N'EST PAS CE QUI EST COMPTÉ, ET C'EST DÉLIBÉRÉ.** Le
    /// compte écarte les espaces ET les icônes masquées, parce qu'il sert à
    /// RECONNAÎTRE la zone à l'œil (voir `ZoneSummary::content`, et le défaut réel
    /// dont il vient : « il est marqué qu'il y a une icône, or je n'en vois
    /// pas »). La LISTE, elle, les montre — sans quoi on ne pourrait ni voir un
    /// espace, ni réafficher une applet cachée depuis ce panneau.
    ///
    /// ⚠️ **LES CASES DU MIROIR SONT DANS L'ORDRE, ET N'Y SONT PAS DES ICÔNES.**
    /// `set_tray_count` les ajoute bel et bien à `right_order` (`retain` puis
    /// `extend`) : c'est **le FICHIER** qui ne les exporte pas (non persistables).
    /// Les lister une par une noierait le panneau sous quarante lignes
    /// « Avertissement », « MSG Afterburner »… — elles donnent donc **UNE ligne
    /// `Mirror`**, portée par la zone qui les contient réellement (celle dont le
    /// contenu cite un index de la plage, et non « la dernière » par convention).
    ///
    /// Rend une liste VIDE pour un index qui n'ouvre aucune zone : l'appelant n'a
    /// rien à afficher, et deviner serait pire que se taire.
    pub fn zone_contents(&self, sep: usize) -> Vec<ZoneEntry> {
        if !self.is_zone_separator(sep) {
            return Vec::new();
        }
        let order = self.order(Side::Right);
        let Some(rank) = order.iter().position(|&index| index == sep) else { return Vec::new() };
        let brut: Vec<usize> = order[rank + 1..]
            .iter()
            .copied()
            .take_while(|&next| !self.is_zone_separator(next))
            .collect();
        let dans_le_miroir = |index: usize| {
            self.tray_base.is_some_and(|base| (base..base + Self::TRAY_ICON_CAPACITY).contains(&index))
        };
        let mut out: Vec<ZoneEntry> = brut
            .iter()
            .copied()
            .filter(|&index| !dans_le_miroir(index))
            .map(|index| ZoneEntry::Ordered {
                index,
                hidden: self.icons.get(index).is_some_and(|icon| icon.hidden),
                // `filter(...).map(...)` : un espace donne SA taille (réglable, voir
                // `Icon::spacer_size`), une icône donne `None`. Un index hors bornes
                // aussi — il n'y a rien à en dire.
                spacer: self.icons.get(index).filter(|icon| icon.is_spacer).map(|icon| icon.spacer_size),
            })
            .collect();
        if brut.iter().any(|&index| dans_le_miroir(index)) {
            if let Some(count) = self.tray_count() {
                out.push(ZoneEntry::Mirror { count });
            }
        }
        out
    }

    /// **Nombre d'entrées OCCUPÉES du miroir de notification** — les cases de la
    /// plage créée par `set_tray_count` qui ne sont PAS masquées, puisque c'est
    /// `hidden` qui y marque les places libres (voir sa boucle). `None` tant que
    /// le miroir n'existe pas.
    ///
    /// Existe pour le panneau : il annonce « Miroir de notification (N) » sur la
    /// dernière zone, seule façon de comprendre pourquoi elle paraît pleine alors
    /// que son compte d'icônes vaut zéro.
    pub fn tray_count(&self) -> Option<usize> {
        let base = self.tray_base?;
        let fin = (base + Self::TRAY_ICON_CAPACITY).min(self.icons.len());
        Some(self.icons[base..fin].iter().filter(|icon| !icon.hidden).count())
    }

    /// **LE PRÉDICAT QUE TOUS LES GESTES CONSULTENT** : `index` est-il
    /// intouchable ? Vrai si sa barre est verrouillée, ou s'il appartient à une
    /// zone FIGÉE (voir `Icon::zone_locked`) — y compris s'il EST le séparateur
    /// de cette zone.
    ///
    /// **Une seule définition, et c'est tout l'intérêt** : le glisser, la
    /// création et le retrait d'un espace, la bascule d'un séparateur de zone
    /// et les curseurs du clic droit refusent donc EXACTEMENT la même chose.
    /// Recopier la règle dans chacun de ces chemins laisserait des trous que
    /// personne ne verrait — le verrou aurait l'air de marcher, sauf par un
    /// chemin précis, et c'est le genre de défaut qu'on ne trouve qu'en le
    /// rencontrant.
    pub fn is_locked_at(&self, index: usize) -> bool {
        // Un index hors de toute barre est REFUSÉ : aucun geste ne l'atteint,
        // donc refuser ne coûte rien et ne peut pas laisser passer un cas
        // oublié.
        let Some(side) = self.icon_side(index) else { return true };
        if self.is_bar_locked(side) {
            return true;
        }
        // **UN SÉPARATEUR N'EST PAS LE CONTENU D'UNE ZONE — c'est LUI qui
        // l'ouvre**, et c'est pourquoi il ne regarde que la SIENNE.
        //
        // Le faire regarder la zone d'AVANT (ce que fait le cas général)
        // figeait, en plus, le séparateur qui la SUIT — or c'est justement
        // celui qu'on attrape pour réordonner, puisque le glisser simple d'un
        // séparateur règle la cote de la zone du dessus. Un utilisateur qui
        // verrouille une zone se retrouvait donc avec DEUX marqueurs
        // indéplaçables, sans un mot (mesuré :
        // `a_separator_after_a_locked_zone_can_still_be_dragged`).
        //
        // Sa PROPRE zone verrouillée le fige toujours : déplacer le séparateur,
        // c'est déplacer la zone, et c'est ce qu'un verrou doit empêcher.
        if self.is_zone_separator(index) {
            return self.is_zone_locked(index);
        }
        // La zone qui le CONTIENT — `previous_zone_separator` remonte l'ordre de
        // sa barre jusqu'au séparateur qui l'ouvre.
        self.is_zone_locked(index) || self.previous_zone_separator(index).is_some_and(|sep| self.is_zone_locked(sep))
    }

    /// Plafond d'une cote de zone : LA hauteur disponible dans une barre
    /// latérale — **même formule que `reclamp_scroll`**, pour que le geste de
    /// redimensionnement (voir `update_drag`) et le curseur du clic droit
    /// (`win32::cef_spacer_size`, qui la reçoit pour borner son second
    /// curseur) s'arrêtent au même endroit : au-delà, la barre se met de toute
    /// façon à défiler.
    ///
    /// **Une seule définition, et non deux formules jumelles** : les deux
    /// commandes règlent la MÊME grandeur, et un écart entre leurs plafonds se
    /// lirait comme un défaut (« le curseur va plus loin que la souris »).
    pub fn zone_cote_max(&self, layout: &DockLayout) -> f32 {
        (layout.total_height - layout.haut_colonnes - TOP_MARGIN * 2.0).max(0.0)
    }

    /// Bascule l'espace `index` entre espace ORDINAIRE et SÉPARATEUR DE ZONE
    /// — DEMANDE EXPLICITE, geste **Maj + clic droit** : c'est le seul moyen de
    /// CRÉER un séparateur de zone, et le refaire l'annule.
    ///
    /// No-op silencieux si `index` n'est pas un espace : comme
    /// `set_spacer_size`, pas question de transformer une icône normale par
    /// erreur d'appel.
    ///
    /// La cote posée est `DEFAULT_ZONE_HEIGHT`, jamais `0.0` — qui serait un
    /// espace ordinaire, donc une bascule invisible. La suite du réglage se
    /// fait à la souris (Maj + glisser, voir `update_drag`).
    /// Crée un SÉPARATEUR DE ZONE juste APRÈS l'icône `after_index` — la
    /// version « zone » de `add_spacer_after`, pour le geste Maj + clic droit.
    ///
    /// DEMANDE EXPLICITE : le geste doit CRÉER le séparateur quand il n'y a pas
    /// d'espace sous le curseur, sans quoi il ne fait RIEN dans une barre VIDE —
    /// le cas de la barre de droite par défaut, et précisément celui où
    /// l'intention est la plus claire (l'utilisateur a essayé, et rien ne s'est
    /// produit). Sur un espace déjà présent, `toggle_zone_separator` suffit.
    ///
    /// `None` si l'icône n'appartient à aucune barre — aucun espace n'a alors été créé, et c'est à
    /// l'appelant de persister ou non (aucun fichier n'est touché ici).
    ///
    /// **Rend l'index du nouvel espace**, comme `add_zone_separator_at_end` et pour la même raison :
    /// l'appelant en a besoin pour le RÉGLER aussitôt (voir `win32::cef_settings`, qui lui donne des
    /// colonnes).
    pub fn add_zone_separator_after(&mut self, after_index: usize, cote: f32) -> Option<usize> {
        // L'index RENDU par `add_spacer_after`, et non la longueur d'avant :
        // l'espace s'insère SOUS les icônes d'applications ouvertes, donc son
        // index n'est plus celui du bout de `icons` (voir `insert_spacer`, doc).
        // C'est aussi ce `None` qui porte le « aucun espace créé ».
        let new_index = self.add_spacer_after(after_index)?;
        self.set_zone_height(new_index, cote);
        Some(new_index)
    }

    /// Crée un SÉPARATEUR DE ZONE en fin de barre `side` — la version « zone »
    /// de `add_spacer_at_end`, pour le cas où le curseur est dans le VIDE d'une
    /// barre (voir `add_zone_separator_after`, même raison).
    ///
    /// **Rend l'index du nouvel espace**, et ce n'est pas un détail de confort :
    /// l'appelant en a besoin pour le RÉGLER aussitôt. `win32::cef_settings` lui
    /// donne des colonnes dans la foulée, et sans l'index il devrait le
    /// redemander par sa clé — donc supposer qu'il est le DERNIER espace, ce qui
    /// est faux dès qu'un autre s'insère entre-temps.
    pub fn add_zone_separator_at_end(&mut self, side: Side, cote: f32) -> usize {
        let new_index = self.add_spacer_at_end(side);
        self.set_zone_height(new_index, cote);
        new_index
    }

    /// **AJOUTE UNE ZONE APRÈS CELLE-CI — c'est-à-dire après TOUT SON CONTENU.**
    ///
    /// `add_zone_separator_after` insère JUSTE APRÈS l'index qu'on lui donne (voir
    /// `add_spacer_after`) : lui passer le séparateur créerait donc une zone **VIDE**, et
    /// verserait le contenu de la zone visée dans la nouvelle — le contraire de ce que
    /// « ajouter une zone après celle-ci » veut dire, et l'utilisateur verrait sa zone se vider.
    ///
    /// On lui passe donc le **dernier élément de la zone**, qui est le séparateur lui-même
    /// lorsqu'elle est vide : `zone_end_order_position` rend en effet `position(sep) + 1` dans ce
    /// cas, donc `fin - 1` y retombe sur `sep`. C'est ce qui rend le cas dégénéré juste SANS le
    /// traiter à part.
    ///
    /// `None` si `sep` n'ouvre aucune zone (`zone_end_order_position` le dit), ou si la barre de
    /// droite ne porte rien. Rend l'index du nouvel espace, pour que l'appelant puisse le régler.
    pub fn add_zone_after_zone(&mut self, sep: usize, cote: f32) -> Option<usize> {
        let fin = self.zone_end_order_position(sep)?;
        let dernier = self.order(Side::Right).get(fin.saturating_sub(1)).copied()?;
        self.add_zone_separator_after(dernier, cote)
    }

    pub fn toggle_zone_separator(&mut self, index: usize) {
        // Par `zone_height` — donc no-op si ce n'est pas un espace — et par
        // `set_zone_height`, jamais en écrivant le champ directement : c'est ce
        // qui fait tomber aussi le défilement propre de la zone défaite (voir
        // `set_zone_height`). Écrire le champ laisserait cette position en
        // place pour la bascule suivante.
        let Some(current) = self.zone_height(index) else { return };
        self.set_zone_height(index, if current > 0.0 { 0.0 } else { DEFAULT_ZONE_HEIGHT });
    }

    /// Change la hauteur de l'espace `index` (voir `Icon::spacer_size`,
    /// demande explicite : "ajouter une option au clic droit sur un
    /// séparateur pour choisir sa taille depuis un curseur") — no-op si
    /// `index` n'est pas un espace (ou hors bornes) : pas question de
    /// changer la TAILLE D'ICÔNE d'une icône normale par erreur d'appel.
    pub fn set_spacer_size(&mut self, index: usize, size: f32) {
        if let Some(icon) = self.icons.get_mut(index) {
            if icon.is_spacer {
                icon.spacer_size = size.max(0.0);
            }
        }
    }

    /// Harnais de VÉRIFICATION VISUELLE uniquement (voir
    /// `NEURADESK_DEBUG_ZONE`, doc de `win32::debug_capture`) : insère un
    /// espace APRÈS l'élément de rang `after_position` de la barre `side`, et
    /// en fait un séparateur de zone de cote `cote`.
    ///
    /// Le geste réel est Maj + clic droit (`toggle_zone_separator`), mais
    /// aucune entrée n'est jamais simulée dans ce projet : sans cette sonde,
    /// une zone ne serait tout simplement pas observable, et son rendu — la
    /// seule chose qu'un test ne peut pas juger — resterait invérifié.
    ///
    /// **Ne persiste RIEN et ne touche à aucun fichier** : l'espace et sa cote
    /// disparaissent avec le processus. Sans effet si la barre n'a pas
    /// d'élément à ce rang, ou si elle est la barre du haut (un espace n'y a
    /// pas de hauteur propre, voir `layout_top_side`).
    pub fn debug_add_zone_separator(&mut self, side: Side, after_position: usize, cote: f32) {
        if !side.is_lateral() {
            return;
        }
        let Some(&after) = self.order(side).get(after_position) else { return };
        let before = self.icons.len();
        self.add_zone_separator_after(after, cote);
        // L'index CRÉÉ est annoncé : c'est ce que `NEURADESK_DEBUG_HOVER`
        // attend pour survoler le séparateur, et il n'est pas devinable depuis
        // l'extérieur — le nombre d'espaces déjà présents dépend de la
        // configuration de l'utilisateur (épingles, épinglettes de la barre des
        // tâches...). Sans cette ligne, la sonde pose une zone que rien ne peut
        // ensuite survoler, donc dont la cote au survol reste invérifiable.
        if self.icons.len() > before {
            let index = self.icons.len() - 1;
            // L'ÉTIQUETTE EST ACTIVÉE ICI, et c'est délibéré : c'est le seul
            // moyen de VOIR le numéro/le nom rendu dans la barre (voir
            // `Dock::zone_labels` et `app::run`, qui le dessine) — sans quoi une
            // capture ne montrerait jamais ce chemin de rendu. Même famille
            // d'appel de fonction direct que le reste de cette sonde.
            self.set_zone_label_shown(index, true);
            eprintln!("[neuradesk][capture] séparateur de zone créé : index {index} (étiquette affichée)");
        }
    }

    /// Barre géométrique sous `(x, y)` — contrairement à `icon_at`, répond
    /// aussi dans l'espace VIDE d'une barre (sous sa dernière icône) : voir
    /// `add_spacer_at_end`, demande explicite : "ajouter... des espaces
    /// invisibles" y compris là où il n'y a pas encore d'icône pour servir
    /// de point d'ancrage à `add_spacer_after`.
    /// La barre sous `(x, y)`, LES CINQ comprises : les trois zones de la barre
    /// du haut (voir `zone_at`) en plus de ce que rend `side_at`, qui répond
    /// `None` pour toute la barre du haut. ⛔ C'est ce `None` qui rendait le clic
    /// molette muet sur la barre du haut (constat de l'utilisateur, 2026-11-01).
    pub fn barre_sous(&self, x: f32, y: f32, layout: &DockLayout) -> Option<Side> {
        if y < layout.haut_colonnes {
            self.zone_at(x, y, layout)
        } else {
            self.side_at(x, y, layout)
        }
    }

    pub fn side_at(&self, x: f32, y: f32, layout: &DockLayout) -> Option<Side> {
        // `haut_colonnes` : barre du haut masquée, son espace est aux colonnes.
        if y < layout.haut_colonnes {
            None
        } else if layout.bottom_thickness > 0.0 && y >= layout.total_height - layout.bottom_thickness {
            // ⚠️ **LA BANDE DU BAS SE TESTE AVANT LES LATÉRALES**, et c'est la même
            // règle que `bar_rects` : la barre du bas prend les COINS, donc les
            // bandes latérales s'arrêtent au-dessus d'elle ; aux angles bas,
            // c'est donc ELLE qui est sous le curseur.
            //
            // ⛔ **CE CAS A ÉTÉ OUBLIÉ, ET RIEN NE L'A SIGNALÉ.** Cette fonction
            // rend un `Option<Side>` : elle n'est pas un `match` exhaustif sur
            // `Side`, donc contrairement aux dix-huit autres, le compilateur ne
            // pouvait pas réclamer le membre ajouté. Conséquence MESURÉE : le
            // clic droit sur la barre du bas ne faisait RIEN — `side_at` rendait
            // `None`, et tout ce qui décide de la barre sous le curseur (le
            // curseur d'épaisseur, les menus) était sauté sans un mot.
            //
            // À `bottom_thickness == 0.0` (écran secondaire), la condition est
            // inatteignable pour un clic DANS la fenêtre : la géométrie d'avant
            // est reproduite à l'identique.
            Some(Side::Bottom)
        } else if x < layout.left_width {
            Some(Side::Left)
        } else if x > layout.total_width - layout.right_width {
            Some(Side::Right)
        } else {
            None
        }
    }

    /// Ajoute un nouvel espace invisible (voir `Icon::is_spacer`) JUSTE
    /// APRÈS l'icône `after_index`, dans SA barre — demande explicite :
    /// "ajouter la possibilité d'ajouter... des espaces invisibles de la
    /// taille d'une icône comme séparateur". No-op si `after_index`
    /// n'appartient à aucune des deux barres. TOUJOURS annexé à la fin de
    /// `icons` (voir la doc de `remove_spacer` sur pourquoi jamais
    /// ailleurs) ; sa position VISUELLE ne vient que de son insertion dans
    /// l'ordre de la barre.
    pub fn add_spacer_after(&mut self, after_index: usize) -> Option<usize> {
        let side = self.icon_side(after_index)?;
        // ⚠️ **LA POSITION SE PREND AVANT `insert_spacer`, ET C'EST TOUT LE
        // CORRECTIF.** `insert_spacer` DÉCALE de +1 tout index ≥ `at` dans les
        // quatre listes (voir sa doc) : chercher `after_index` APRÈS coup ne le
        // trouve donc plus (il est devenu `after_index + 1`), `position` rend
        // `None`, et le repli `order.len()` posait l'espace **EN FIN DE BARRE**.
        // Conséquence mesurée : le bouton « ＋ Ajouter une zone après celle-ci »
        // des Réglages — qui passe le DERNIER élément de la zone, donc un index
        // ≥ `at` — voyait son séparateur atterrir sous le miroir de
        // notification, où `icon_order` ne l'exporte PAS : la zone était perdue
        // au redémarrage SUIVANT, en silence. Le commentaire d'`insert_spacer`
        // dit déjà que « l'ordre des deux opérations est important ».
        let pos = self.order(side).iter().position(|&i| i == after_index).map_or_else(
            || self.order(side).len(),
            |p| p + 1,
        );
        let new_index = self.insert_spacer();
        self.order_mut(side).insert(pos, new_index);
        Some(new_index)
    }

    /// **AJOUTE UN ESPACE JUSTE AVANT `before_index`, DANS SA BARRE** — le pendant
    /// exact d'`add_spacer_after`, et **la seule façon de poser un espace EN TÊTE
    /// DE BARRE** : `add_spacer_at_end` n'ajoute qu'à la fin, et
    /// `add_spacer_after` qu'après une icône — donc la PREMIÈRE icône d'une barre
    /// n'avait aucun moyen d'être précédée.
    ///
    /// DEMANDE EXPLICITE (2026-09-30) : « je ne peux pas mettre de séparateur avant
    /// le premier dossier ».
    ///
    /// ⚠️ **LA POSITION SE PREND AVANT `insert_spacer`**, pour la raison exacte que
    /// porte `add_spacer_after` (voir sa doc, et le défaut mesuré qu'elle décrit) :
    /// après l'insertion, tout index ≥ `at` a glissé d'un cran, et chercher
    /// `before_index` ne le retrouve plus.
    ///
    /// Le repli `unwrap_or(0)` est DÉFENSIF : `icon_side` vient de prouver que
    /// l'index est cité par cette barre, donc `position` le trouve. S'il ne le
    /// trouvait pas, poser en TÊTE est le choix le moins surprenant pour une
    /// fonction qui s'appelle « avant » — là où `add_spacer_after` retombe sur la
    /// fin, qui est son pendant.
    pub fn add_spacer_avant(&mut self, before_index: usize) -> Option<usize> {
        let side = self.icon_side(before_index)?;
        let pos = self.order(side).iter().position(|&i| i == before_index).unwrap_or(0);
        let new_index = self.insert_spacer();
        self.order_mut(side).insert(pos, new_index);
        Some(new_index)
    }

    /// **DÉCALE LE BLOC DES ÉPINGLES ET DES APPLETS — la SEULE chose qui bouge
    /// quand le NOMBRE d'épingles change.**
    ///
    /// **POURQUOI CE BLOC BOUGE, ET RIEN AU-DESSUS.** `Dock::new` place les
    /// épingles de l'utilisateur juste après les trois icônes intégrées, puis les
    /// **sept applets**, puis les REMPLISSEURS jusqu'à `winpin_base`. Une épingle
    /// de plus décale donc les applets d'un cran — tandis que les remplisseurs
    /// rétrécissent d'autant, si bien que **les espaces de zone et les épingles de
    /// la barre des tâches ne bougent PAS** : c'est la propriété mesurée par
    /// `ui::zones::tests::adding_a_dock_pin_shifts_no_spacer`, et la raison d'être
    /// de `winpin_base`, FIGÉ d'une session à l'autre.
    ///
    /// **ET C'EST POURQUOI LES QUATRE ORDRES DOIVENT SUIVRE** : ils citent des
    /// index ABSOLUS, et l'arithmétique des applets est
    /// `PINNED_ICON_BASE + pinned_apps.len()` (voir `app::run`, et le journal de
    /// la machine : « applets : alimentation 8, … theme 14 (pins 3..7, 5
    /// epingle(s)) » — 3 + 5 = 8). Sans ce décalage, l'entrée qui voulait dire
    /// « alimentation » désigne « terminal » au lancement suivant : la classe de
    /// défaut que `insert_spacer` documente déjà (« elles désignent l'icône
    /// VOISINE »), et que ce dépôt a payée trois fois.
    ///
    /// `index` : la place libérée (retrait) ou occupée (ajout) dans `icons`.
    /// `limite` : `winpin_base` — **au-delà, plus rien ne bouge** (les espaces et
    /// les épingles de la barre des tâches gardent leurs index). Passé par
    /// l'appelant, comme `ensure_saved_spacers_exist` le fait de `winpin_end`.
    /// `retrait` : `true` ⇒ l'entrée de `index` **DISPARAÎT** et celles de
    /// `]index, limite[` reculent d'un cran ; `false` ⇒ celles de
    /// `[index, limite[` avancent — la place est alors libre pour la nouvelle
    /// épingle, que l'appelant insère **ENSUITE** (l'ordre des deux opérations
    /// compte, exactement comme dans `insert_spacer`).
    pub fn decaler_le_bloc_des_epingles(&mut self, index: usize, limite: usize, retrait: bool) {
        decaler_bloc_epingles_dans(
            [&mut self.left_order, &mut self.right_order, &mut self.top_left_order, &mut self.top_right_order],
            index,
            limite,
            retrait,
        );
    }

    /// Ajoute un espace invisible à la FIN de la barre `side` (voir
    /// `add_spacer_after`, `side_at`) — pour un Ctrl+clic droit dans la zone
    /// vide sous la dernière icône d'une barre. Rend son index.
    pub fn add_spacer_at_end(&mut self, side: Side) -> usize {
        let new_index = self.insert_spacer();
        self.order_mut(side).push(new_index);
        new_index
    }

    /// **LA LISTE QUI PORTE UNE PLAGE** — déduite des setters qui l'allouent, jamais
    /// inventée : `ensure_winpin_capacity` et `set_dynamic_apps` placent leurs icônes
    /// dans la barre de **GAUCHE**, `set_tray_count` dans celle de **DROITE**, et
    /// `set_dossiers` remplit `bottom_order`.
    #[allow(dead_code)] // étape 3 : le premier appelant de production arrive à l'étape 4
    fn liste_de_la_plage(&mut self, plage: Plage) -> &mut Vec<usize> {
        match plage {
            Plage::Dossiers => &mut self.bottom_order,
            Plage::Notification => &mut self.right_order,
            Plage::WinPins | Plage::Dynamiques => &mut self.left_order,
        }
    }

    /// **INSÈRE UNE ICÔNE AU RANG `rang` D'UNE PLAGE, ET DÉCALE TOUT CE QUI SUIT.**
    ///
    /// ⛔ **C'EST L'ÉTAPE 3 DU REFACTOR** (voir `docs/plans/dock-identites-stables.md`),
    /// et **aucun des deux mécanismes existants ne suffisait** — leurs deux lectures
    /// sont dans le plan, et c'est ce qui a décidé cette forme :
    /// - `insert_spacer` **INSÈRE**, mais son point d'insertion n'est pas un paramètre :
    ///   il vaut `base_non_persistable()`, donc **au-dessus** de la plage des dossiers ;
    /// - `decaler_bloc_epingles_dans` **DÉCALE**, mais **borné**, **sans toucher
    ///   `icons`**, et **sans toucher `bottom_order`**.
    ///
    /// **CE QU'INSÉRER DÉPLACE, ET POURQUOI CHACUN** :
    /// 1. `icons` — l'icône elle-même, au rang demandé ;
    /// 2. les **quatre ordres persistés** — **non bornés**, exactement comme
    ///    `insert_spacer` : tout index ≥ `at` désigne une icône qui a réellement glissé
    ///    d'un cran. Les borner laisserait des citations pointant sur l'icône VOISINE,
    ///    la famille de défauts que la doc d'`insert_spacer` mesure et que ce dépôt a
    ///    payée trois fois ;
    /// 3. **`bottom_order`** — la **cinquième** liste, celle qu'`insert_spacer` ne touche
    ///    pas : c'est elle qui porte la barre du bas, donc la plage qui sert d'abord ;
    /// 4. la **citation de l'icône neuve**, à son rang, dans la liste qui porte sa plage ;
    /// 5. les **bases** des plages situées AU-DESSUS (`base > at`) — `app::run` les relit
    ///    à chaque frame, donc une base laissée en arrière routerait les clics sur la
    ///    **mauvaise** icône ;
    /// 6. le **compte** de la plage, sans quoi `plage_fin` mentirait dès l'insertion
    ///    suivante.
    ///
    /// ⚠️ **`>` ET NON `>=` POUR LES BASES, ET C'EST LA DIFFÉRENCE AVEC `insert_spacer`** :
    /// chez lui, `at` **EST** la plus petite base — l'espace se pose à la frontière, donc
    /// la plage qui commence là recule. Ici, au rang 0, l'icône neuve prend la place de
    /// l'ancien rang 0 : **la plage commence toujours au même index**, et décaler sa base
    /// la ferait sauter d'un cran à chaque insertion en tête.
    ///
    /// Rend l'index de la nouvelle icône — `None` si la plage n'est pas allouée, si elle
    /// n'a pas de compte (`Dynamiques`), ou si `rang` dépasse sa fin.
    ///
    /// ⚠️ **`Plage::Notification` EST ACCEPTÉE MAIS N'A PAS ÉTÉ ÉPROUVÉE** : sa capacité
    /// est FIXE (`TRAY_ICON_CAPACITY`, la plage est remplie une fois pour toutes par
    /// `set_tray_count`), donc un appelant qui y insérerait au-delà de la capacité
    /// rendrait `tray_count` menteur. Aucun appelant ne le fait aujourd'hui — c'est DIT
    /// ici plutôt que gardé par une branche que rien ne mesurerait.
    /// ⚠️ **PLUS D'`#[allow(dead_code)]` DEPUIS L'ÉTAPE 4** : il a un appelant de
    /// production (`app::DossiersVivants::ajouter`), donc **le compilateur PROUVE qu'il
    /// sert**. C'est l'assertion la plus forte de tout ce lot, et elle est gratuite —
    /// garder l'`allow` serait se priver d'un contrôle.
    pub fn inserer(&mut self, plage: Plage, rang: usize, icone: Icon) -> Option<usize> {
        // --- LA VALIDATION D'ABORD, ET ENTIÈREMENT : rien n'est modifié tant que tout
        // n'est pas accepté, ce qui rend le refus sans effet de bord.
        let base = self.plage_base(plage)?;
        // `plage_fin` refuse `Dynamiques` (pas de compte) — donc l'insertion aussi.
        let fin = self.plage_fin(plage)?;
        if rang > fin - base {
            return None;
        }
        let at = base.checked_add(rang)?;

        // ⚠️ **LA POSITION SE PREND AVANT `icons.insert`** : après, tout index ≥ `at` a
        // glissé d'un cran et la chercher ne la retrouve plus — exactement le défaut
        // qu'`add_spacer_after` documente (« l'ordre des deux opérations est
        // important »).
        let position = {
            let liste = self.liste_de_la_plage(plage);
            liste.iter().position(|&i| i >= at).unwrap_or(liste.len())
        };

        // LE COMPTE SUIT LA PLAGE : `base + count` **EST** la fin, donc l'oublier ferait
        // mentir `plage_fin` — et avec lui `emplacement`, `rang_dans` et l'insertion
        // suivante.
        match plage {
            Plage::WinPins => self.winpin_count += 1,
            Plage::Dossiers => self.dossiers_count += 1,
            Plage::Notification => self.tray_count += 1,
            // DÉJÀ refusée par `plage_fin` ci-dessus : on le redit plutôt que de le
            // supposer, parce que ce `match` est le seul endroit qui décide du compte.
            Plage::Dynamiques => return None,
        }

        self.icons.insert(at, icone);

        // LES CINQ LISTES, PAR `orders_mut` — jamais une énumération écrite à la
        // main : c'est elle qui a laissé la barre du bas en arrière deux fois
        // (voir `orders_mut`, doc).
        for order in self.orders_mut() {
            for entry in order.iter_mut().filter(|entry| **entry >= at) {
                *entry += 1;
            }
        }
        self.liste_de_la_plage(plage).insert(position, at);

        for base in [&mut self.winpin_base, &mut self.dossiers_base, &mut self.tray_base, &mut self.dynamic_base] {
            if let Some(borne) = base.as_mut() {
                if *borne > at {
                    *borne += 1;
                }
            }
        }

        Some(at)
    }

    /// **RETIRE L'ICÔNE DU RANG `rang` D'UNE PLAGE, ET FAIT RECULER TOUT CE QUI SUIT.**
    ///
    /// C'est **l'inverse exact d'`inserer`** — mêmes cinq listes, mêmes quatre bases, mêmes
    /// comptes, dans l'autre sens — et il est indispensable à l'étape 4 : sans lui, un
    /// dossier désépinglé disparaîtrait du fichier mais **resterait sur la barre** jusqu'au
    /// redémarrage suivant. **Un contrôle qui a l'air d'une bascule doit en être une** ;
    /// sinon il ment, et un contrôle qui ment est pire qu'un contrôle absent.
    ///
    /// ⚠️ **LA BORNE N'EST PAS CELLE D'`INSERER`.** Ici `rang` doit être **strictement**
    /// inférieur à `fin - base`, là où `inserer` accepte l'égalité. Ce n'est pas une
    /// incohérence : pour la plage des dossiers, `fin` est l'index du bouton **« + »**, qui
    /// n'est pas un dossier — on peut insérer À SA PLACE (le nouveau passe avant lui), mais
    /// pas le retirer : la barre du bas sans son « + » n'aurait plus aucun moyen d'ajouter
    /// un dossier.
    ///
    /// ⚠️ **LA CITATION DE L'INDEX PART D'ABORD, ET SUR LES CINQ LISTES.** C'est la règle
    /// que `decaler_bloc_epingles_dans` applique déjà (`retain` **puis** décrément) : après
    /// `icons.remove(at)`, toute citation de `at` désignerait l'icône qui a glissé à sa
    /// place — la famille de défauts que ce dépôt a payée trois fois. Les retirer des cinq
    /// listes plutôt que de la seule plage visée évite de **supposer** qu'aucune autre ne le
    /// cite : un glisser-déposer peut citer n'importe quelle icône dans n'importe quelle
    /// barre (voir `set_dynamic_apps`, qui purge les trois autres pour cette raison exacte).
    /// ⚠️ **PLUS D'`#[allow(dead_code)]` DEPUIS L'ÉTAPE 4** : son appelant de production
    /// est `app::DossiersVivants::retirer`, donc le compilateur prouve qu'il sert.
    pub fn retirer(&mut self, plage: Plage, rang: usize) -> Option<usize> {
        // --- LA VALIDATION D'ABORD, ET ENTIÈREMENT : rien n'est modifié tant que tout n'est
        // pas accepté, ce qui rend le refus sans effet de bord.
        let base = self.plage_base(plage)?;
        // `plage_fin` refuse `Dynamiques` (pas de compte) — donc le retrait aussi.
        let fin = self.plage_fin(plage)?;
        // STRICTEMENT inférieur : `fin` n'est pas un élément de la plage (voir la doc).
        if rang >= fin - base {
            return None;
        }
        let at = base.checked_add(rang)?;

        match plage {
            Plage::WinPins => self.winpin_count -= 1,
            Plage::Dossiers => self.dossiers_count -= 1,
            Plage::Notification => self.tray_count -= 1,
            // DÉJÀ refusée par `plage_fin` ci-dessus : on le redit plutôt que de le
            // supposer, parce que ce `match` est le seul endroit qui décide du compte.
            Plage::Dynamiques => return None,
        }

        // ⚠️ **LE `retain` AVANT LE DÉCALAGE** — l'ordre est porteur, et c'est celui que
        // `decaler_bloc_epingles_dans` documente : décrémenter d'abord ferait glisser la
        // citation de `at` sur un index qu'elle ne désigne plus, puis le `retain` ne la
        // retrouverait pas.
        for order in self.orders_mut() {
            order.retain(|&entry| entry != at);
            for entry in order.iter_mut().filter(|entry| **entry > at) {
                *entry -= 1;
            }
        }

        self.icons.remove(at);

        // `>` et non `>=`, comme dans `inserer` : la base de la plage visée ne bouge pas.
        for base in [&mut self.winpin_base, &mut self.dossiers_base, &mut self.tray_base, &mut self.dynamic_base] {
            if let Some(borne) = base.as_mut() {
                if *borne > at {
                    *borne -= 1;
                }
            }
        }

        Some(at)
    }

    /// Insère un espace invisible **à la fin de la partie PERSISTÉE d'`icons`**
    /// — donc SOUS les DEUX plages d'exécution — et rend son index.
    ///
    /// **C'EST LE CŒUR DE LA SURVIE D'UN ESPACE**, et pas un détail de
    /// rangement. Annexer au bout (`icons.push`) plaçait l'espace DANS une plage
    /// d'exécution, où deux mécanismes le condamnaient :
    /// - `icon_order` filtre ses listes par `is_persistable`, donc son entrée
    ///   d'ordre n'était **jamais écrite** — sa cote l'était
    ///   (`persist_lateral_sizes` ne filtre pas), d'où des fichiers citant une
    ///   cote sans que l'ordre cite l'espace ;
    /// - `set_dynamic_apps` fait `icons.truncate(base + count)`, donc l'espace
    ///   était **DÉTRUIT** dès que le nombre d'applications ouvertes diminuait.
    ///
    /// Défaut réel, mesuré, et reproduit par la sonde
    /// `a_created_spacer_is_lost_once_dynamic_apps_exist` avant ce correctif :
    /// base dynamique 115, espace créé à l'index 116, jamais exporté.
    ///
    /// ⚠️⚠️ **LA FRONTIÈRE EST CELLE D'`is_persistable`, DONC LA PLUS PETITE DES DEUX PLAGES** —
    /// pas `dynamic_base`. S'y fier seule laissait le défaut ENTIER dès que le miroir de
    /// notification est créé AVANT les applications ouvertes, sa base étant alors la plus basse :
    /// **MESURÉ sur la machine de l'utilisateur le 2026-09-22 — base du miroir 178, base dynamique
    /// 242, espace neuf posé en 242**, or `242 < 178` est faux, donc **jamais écrit**. Sa zone
    /// recevait une clé et `zones.toml` l'enregistrait, mais aucun ordre écrit ne citait l'espace :
    /// au lancement suivant la clé ne se résolvait sur RIEN (curseur de cote sans effet, zone non
    /// dessinée), et `completer_les_barres` recréait l'espace manquant **au même endroit** — non
    /// persisté de nouveau. La boucle ne se réparait jamais.
    ///
    /// **Les icônes d'exécution montent d'un cran, et c'est gratuit** : leur
    /// ordre n'est jamais persisté (c'est tout l'objet d'`is_persistable`), et
    /// leurs drapeaux — `external_texture` compris — voyagent DANS les `Icon`,
    /// donc ils se décalent avec elles. **Ce sont les DEUX BASES qui doivent suivre**, chacune si
    /// le point d'insertion l'a atteinte : `app::run` relit `dynamic_icon_base`/`tray_icon_base`
    /// après chaque création, et une base laissée en arrière lui ferait chercher une icône **un
    /// cran trop bas** — donc router les clics sur la MAUVAISE. Une seule était décalée, et
    /// INCONDITIONNELLEMENT (`at + 1`), ce qui la **rétrécissait** quand l'espace s'insérait plus
    /// bas qu'elle, faisant passer des icônes d'exécution du mauvais côté de la frontière.
    fn insert_spacer(&mut self) -> usize {
        let at = self.base_non_persistable().unwrap_or(self.icons.len());
        let last = self.icons.len().saturating_sub(1);
        self.icons.insert(at, Icon { label: ' ', color: [0.0, 0.0, 0.0, 0.0], has_texture: false, external_texture: false, is_spacer: true, spacer_size: SPACER_HEIGHT, zone_height: 0.0, zone_lead: 0.0, zone_columns: DEFAULT_ZONE_COLUMNS, zone_locked: false, zone_show_name: false, zone_permanent: false, hidden: false, sorte: SorteEspace::Rond });
        // **TOUT INDEX ≥ `at` A GLISSÉ D'UN CRAN, ET LES QUATRE LISTES DOIVENT
        // SUIVRE** — elles désignent des index, donc sans ce décalage elles
        // pointent sur l'icône VOISINE. Défaut réel de la première version de
        // cette fonction, constaté sur la configuration de la machine :
        // `gauche=[…, 114, 126, 126]` — l'icône de l'application ouverte restait
        // citée à 126 (elle était passée à 127), l'espace créé s'y ajoutait, et
        // l'index apparaissait DEUX fois : deux emplacements pour un seul
        // élément, un trou à la place de l'autre, et l'icône de l'application
        // plus citée par aucune liste — donc disparue de la barre.
        //
        // Le nouvel espace n'est pas encore dans une liste à ce stade (c'est
        // l'appelant qui l'y pousse, APRÈS cet appel) : il ne peut donc pas être
        // décalé par erreur, et c'est ce qui rend l'ordre des deux opérations
        // important.
        if at <= last {
            // LES CINQ, PAR `orders_mut` (voir sa doc) — et non les quatre
            // persistées. `at` vaut `base_non_persistable()`, donc la barre du bas
            // est TOUJOURS en dessous et la boucle ne la touche pas aujourd'hui :
            // c'est justement le genre de raisonnement qui a laissé un trou les
            // deux fois précédentes. On ne l'écrit plus.
            for order in self.orders_mut() {
                for entry in order.iter_mut().filter(|entry| **entry >= at) {
                    *entry += 1;
                }
            }
        }
        // LES DEUX BASES SUIVENT LE DÉCALAGE, chacune si le point d'insertion l'a atteinte (voir la
        // doc : une base laissée en arrière ferait router les clics sur la mauvaise icône).
        if let Some(tray) = self.tray_base.as_mut() {
            if *tray >= at {
                *tray += 1;
            }
        }
        if let Some(dynamic) = self.dynamic_base.as_mut() {
            if *dynamic >= at {
                *dynamic += 1;
            }
        }
        at
    }

    /// Index de la PREMIÈRE icône d'application ouverte non épinglée, `None`
    /// s'il n'y en a aucune (voir `set_dynamic_apps`) — exposé pour qu'`app::run`
    /// relise sa propre copie après une CRÉATION D'ESPACE, qui décale la plage
    /// dynamique d'un cran (voir `insert_spacer`, doc).
    pub fn dynamic_icon_base(&self) -> Option<usize> {
        self.dynamic_base
    }

    /// Retire l'espace invisible `index` (voir `Icon::is_spacer`) — demande
    /// explicite : "la possibilité de les supprimer en utilisant... ctrl +
    /// clic droit". Retire SEULEMENT sa présence des ORDRES d'affichage (les
    /// CINQ — voir `orders_mut`), JAMAIS son entrée de `icons` : la
    /// retirer de ce tableau aurait décalé l'INDEX de toutes les icônes
    /// suivantes — hors de question tant que d'autres constantes
    /// (`APP_ICON_INDEX` etc. dans `app::run`) restent des index FIXES dans
    /// ce même tableau. L'entrée orpheline ne coûte rien (jamais dessinée,
    /// jamais référencée par aucun ordre une fois retirée d'ici) — bien
    /// plus sûr qu'un remaniement d'index à chaque suppression. No-op si
    /// `index` n'est pas un espace.
    pub fn remove_spacer(&mut self, index: usize) {
        if !self.is_spacer(index) {
            return;
        }
        // ⛔ **CE DÉFAUT A ÉTÉ PAYÉ DEUX FOIS — VOIR `orders_mut`, QUI EXISTE POUR
        // ÇA.** La version d'origine retirait l'index de `left_order`/
        // `right_order`, écrits à la main : un espace posé dans une barre du HAUT
        // n'en partait donc jamais (« je n'arrive plus à retirer des séparateurs
        // que j'ai ajouté dans les barres du haut »). On a ajouté les deux listes
        // du haut… et la barre du BAS, arrivée plus tard, n'a jamais rejoint la
        // liste : un espace posé là ne se retirait plus. `add_spacer_after` et
        // `add_spacer_at_end` insèrent dans N'IMPORTE LAQUELLE des cinq (voir
        // `order_mut`).
        //
        // ⇒ La boucle remplace l'énumération. Il n'y a plus rien à rallonger
        // quand une barre apparaît, donc plus rien à oublier.
        for order in self.orders_mut() {
            order.retain(|&i| i != index);
        }
    }

    /// Démarre un glisser POTENTIEL sur l'icône `index` (voir `IconDrag`,
    /// `DRAG_THRESHOLD`) — à appeler sur `Event::MouseDown` dès que
    /// `icon_at` désigne une icône, AVANT toute exécution de son action
    /// (voir `app::run`, `DragOutcome::Clicked` : l'action n'est routée
    /// qu'au relâchement, une fois confirmé qu'il ne s'agissait PAS d'un
    /// glisser). No-op si `index` n'appartient à aucune des deux barres.
    pub fn begin_potential_drag(&mut self, index: usize, x: f32, y: f32, _hwnd: HWND) {
        // ⛔ **ICI, ON N'REFUSE RIEN — PAS MÊME LE « + », QUI N'EST POURTANT PAS
        // DÉPLAÇABLE.** Le refus vit dans `update_drag` (voir `geste_reste_un_clic`), et
        // la place n'est pas un détail : refuser ici n'empêche pas le déplacement, ça
        // **supprime le CLIC**. `self.drag` n'est alors jamais posé, donc `end_drag`
        // rend `DragOutcome::None` au lieu de `Clicked(index)` — le « + » cesse
        // d'ouvrir le sélecteur de dossier, sans journal ni erreur. Le défaut a été
        // livré une fois sous cette forme exacte, avec un commentaire qui affirmait le
        // contraire. Même leçon que `is_locked_at` : une icône FIGÉE garde son clic.
        let Some(from_side) = self.icon_side(index) else { return };
        self.drag = Some(IconDrag {
            icon_index: index,
            from_side,
            start_pos: (x, y),
            current_pos: (x, y),
            ctrl_held: false,
            dragging: false,
            // Voir `IconDrag::resizing` — décidé au FRANCHISSEMENT du seuil, pas
            // ici : tant qu'on n'a pas bougé, c'est peut-être un simple clic.
            resizing: None,
            resize_start_size: 0.0,
            resize_start_y: 0.0,
        });
    }

    /// `true` si un glisser a RÉELLEMENT commencé (dépassé `DRAG_THRESHOLD`,
    /// voir `IconDrag::dragging`) — PAS seulement "bouton enfoncé sur une
    /// icône" (voir `begin_potential_drag`, qui peut encore n'aboutir qu'à
    /// un simple clic).
    pub fn is_dragging(&self) -> bool {
        self.drag.as_ref().is_some_and(|d| d.dragging)
    }

    /// **CE GESTE NE DEVIENDRA JAMAIS UN DÉPLACEMENT — il doit rester un CLIC.**
    ///
    /// Deux raisons très différentes, réunies ici parce que la CONSÉQUENCE l'est —
    /// `drag.dragging` n'est jamais posé, donc `end_drag` rend `Clicked(index)` :
    /// - **figé** (voir `is_locked_at`) : le contenu d'une zone verrouillée ne bouge
    ///   pas, mais son clic continue de lancer l'application ;
    /// - **le « + »** : c'est un BOUTON, pas un emplacement de la rangée. Un glisser le
    ///   sortirait de la plage où `plus_dossier_index` le calcule (`base + count`),
    ///   et l'utilisateur verrait le bouton d'ajout au MILIEU de ses dossiers.
    ///
    /// ⛔ **LE REFUS DOIT ÊTRE ICI, ET JAMAIS DANS `begin_potential_drag`** — vérifié
    /// par `le_bouton_plus_de_la_barre_du_bas_ne_se_glisse_pas_mais_reste_cliquable`.
    /// Là-bas, refuser le geste ne l'empêche pas : ça **supprime le clic**, et le
    /// bouton devient inerte sans un mot au journal. La leçon était déjà écrite dans
    /// `update_drag`, une quinzaine de lignes plus bas ; elle a été reperdue une fois.
    fn geste_reste_un_clic(&self, index: usize) -> bool {
        self.is_locked_at(index) || self.is_plus_dossier(index)
    }

    /// À appeler à CHAQUE `Event::MouseMove`, glisser en cours ou non
    /// (no-op bon marché sinon) — met à jour la position courante et l'état
    /// de la touche Ctrl (voir `IconDrag::ctrl_held`), et fait basculer
    /// `dragging` à `true` dès que le déplacement total dépasse
    /// `DRAG_THRESHOLD`.
    pub fn update_drag(&mut self, x: f32, y: f32, ctrl_held: bool, shift_held: bool, layout: &DockLayout) {
        // `take` plutôt qu'un emprunt de `self.drag` : la branche de
        // redimensionnement ci-dessous a besoin de `&mut self` (`set_spacer_size`),
        // et `IconDrag` est `Copy`-able en pratique (trois scalaires + un index).
        let Some(mut drag) = self.drag.take() else { return };
        drag.current_pos = (x, y);
        drag.ctrl_held = ctrl_held;
        if !drag.dragging {
            let (sx, sy) = drag.start_pos;
            if (x - sx).hypot(y - sy) >= DRAG_THRESHOLD {
                // VERROU — voir `is_locked_at`, doc. Un élément FIGÉ ne devient
                // jamais un glisser : le geste reste donc un simple CLIC, qui
                // lance l'application comme avant. DEMANDE EXPLICITE : « cliquer
                // une icône continue de lancer l'application — c'est un usage,
                // pas une modification ». C'est pour ça que le refus est ICI et
                // non dans `begin_potential_drag` : là-bas, refuser le geste
                // aurait aussi supprimé le clic, et l'icône serait devenue
                // inerte.
                if self.geste_reste_un_clic(drag.icon_index) {
                    self.drag = Some(drag);
                    return;
                }
                drag.dragging = true;
                // LE seul endroit où le geste est qualifié — voir `DragAdjust`
                // pour ce que règle chacun des cas.
                let lateral = drag.from_side.is_lateral();
                let separator = self.is_zone_separator(drag.icon_index);
                drag.resizing = if shift_held && lateral && self.is_spacer(drag.icon_index) {
                    // Maj sur un espace LATÉRAL : on règle une grandeur. Celle
                    // d'un séparateur de zone est sa COTE (sa raison d'être —
                    // sa taille de marqueur n'est plus qu'une fioriture, voir
                    // `Icon::zone_height`), celle d'un espace ordinaire, la
                    // taille de son blanc.
                    Some(if separator { DragAdjust::ZoneCote } else { DragAdjust::SpacerSize })
                } else if !ctrl_held && lateral && separator {
                    // Glisser SIMPLE sur un séparateur de zone : il se déplace
                    // (DEMANDE EXPLICITE : « je voudrais que ces séparateurs de
                    // zone soient déplaçables librement »). Sa position était
                    // sinon dictée par la cote de la zone du dessus, et le geste
                    // n'y changeait rien à l'écran. Ctrl + glisser, lui, garde
                    // son rôle : réordonner, et changer de barre.
                    Some(match self.previous_zone_separator(drag.icon_index) {
                        Some(previous) => DragAdjust::PreviousZoneCote(previous),
                        None => DragAdjust::ZoneLead,
                    })
                } else {
                    // Tout le reste garde le comportement d'avant (déplacement),
                    // y compris Maj sur une icône normale et le glisser simple
                    // d'un espace ordinaire.
                    None
                };
                drag.resize_start_size = match drag.resizing {
                    Some(DragAdjust::SpacerSize) => self.spacer_size(drag.icon_index).unwrap_or(SPACER_HEIGHT),
                    Some(DragAdjust::ZoneCote) => self.zone_height(drag.icon_index).unwrap_or(DEFAULT_ZONE_HEIGHT),
                    Some(DragAdjust::ZoneLead) => self.zone_lead(drag.icon_index).unwrap_or(0.0),
                    Some(DragAdjust::PreviousZoneCote(previous)) => {
                        self.zone_height(previous).unwrap_or(DEFAULT_ZONE_HEIGHT)
                    }
                    None => 0.0,
                };
                // Origine = le CLIC, pas le franchissement du seuil : l'élément
                // suit ainsi le curseur au pixel près, au lieu de retarder de
                // `DRAG_THRESHOLD` (6 px) sur toute la durée du geste.
                drag.resize_start_y = drag.start_pos.1;
            }
        }
        if let Some(adjust) = drag.resizing {
            let available = self.zone_cote_max(layout);
            // Delta par rapport au DÉBUT du geste, jamais cumulé : la valeur
            // suit la position absolue du curseur, donc elle ne dérive pas si
            // une image est sautée.
            let dragged = (drag.resize_start_size + (y - drag.resize_start_y)).clamp(0.0, available);
            match adjust {
                DragAdjust::SpacerSize => self.set_spacer_size(drag.icon_index, dragged),
                // Descendre la cote jusqu'à 0 est permis, et c'est un retour à
                // l'état d'espace ordinaire — la bascule de Maj + clic droit,
                // atteinte à la souris.
                DragAdjust::ZoneCote => self.set_zone_height(drag.icon_index, dragged),
                DragAdjust::ZoneLead => self.set_zone_lead(drag.icon_index, dragged),
                // PLANCHER ici, contrairement au cas ci-dessus : amener la cote
                // d'une AUTRE zone à 0 en annulerait le séparateur, et la
                // structure se déferait sous la main qui est en train de la
                // régler. Assez petit pour ne pas gêner, assez grand pour que
                // la zone demeure.
                DragAdjust::PreviousZoneCote(previous) => {
                    self.set_zone_height(previous, dragged.max(MIN_ZONE_HEIGHT))
                }
            }
        }
        self.drag = Some(drag);
    }

    /// **CE QU'UN GLISSER DÉPLACE — et, pour un SÉPARATEUR DE ZONE, tout son CONTENU.**
    ///
    /// Une zone n'est pas un contenant mais une FRONTIÈRE : son contenu, c'est « ce qui suit son
    /// séparateur jusqu'au séparateur d'après » (voir `bar_layout`). Déplacer le séparateur SEUL
    /// déplace donc la frontière et pas les icônes — la zone se retrouve vide, et ses icônes
    /// appartiennent à sa voisine. Défaut RÉEL SIGNALÉ : « j'ai déplacé la zone Notifications
    /// mais son contenu n'a pas suivi ». Le bloc les emporte ensemble.
    ///
    /// **Deux bornes, et chacune a sa raison :**
    /// * **le prochain séparateur de zone** — au-delà, c'est le contenu d'une AUTRE zone ;
    /// * **la première icône NON PERSISTABLE** (`is_persistable`) — le miroir de la zone de
    ///   notification et les applications ouvertes vivent à la fin de l'ordre et ne sont pas
    ///   écrits (voir `set_tray_count`, `set_dynamic_apps`). Les franchir mettrait des éléments
    ///   persistés APRÈS des icônes qui ne le sont pas, et l'ordre relu au lancement suivant
    ///   n'aurait plus la même forme.
    ///
    /// Les icônes DÉSACTIVÉES sont emmenées comme les autres : elles appartiennent à l'ordre, et
    /// les laisser derrière ferait changer la structure de la zone sous prétexte qu'elle est
    /// invisible.
    ///
    /// Un élément ordinaire — icône, espace — est son propre bloc : **son geste est inchangé**.
    fn bloc_deplace(&self, drag: &IconDrag) -> Vec<usize> {
        if !self.is_zone_separator(drag.icon_index) {
            return vec![drag.icon_index];
        }
        let Some(side) = self.icon_side(drag.icon_index) else { return vec![drag.icon_index] };
        let order = self.order(side);
        let Some(position) = order.iter().position(|&index| index == drag.icon_index) else {
            return vec![drag.icon_index];
        };
        let mut bloc = vec![drag.icon_index];
        for &icon in &order[position + 1..] {
            if self.is_zone_separator(icon) || !self.is_persistable(icon) {
                break;
            }
            bloc.push(icon);
        }
        bloc
    }

    /// À appeler sur `Event::MouseUp` — termine un glisser éventuellement en
    /// cours (voir `DragOutcome`). Applique et persiste le nouvel ordre
    /// lui-même (voir `order_mut`) ; c'est à l'appelant de sauvegarder la
    /// configuration résultante (voir `config::save_icon_order`,
    /// `icon_order`) et de router `DragOutcome::Clicked` normalement.
    pub fn end_drag(&mut self, layout: &DockLayout, _hwnd: HWND) -> DragOutcome {
        let Some(drag) = self.drag.take() else { return DragOutcome::None };
        if !drag.dragging {
            return DragOutcome::Clicked(drag.icon_index);
        }
        // Redimensionnement : la taille est DÉJÀ appliquée (voir `update_drag`,
        // appelé une dernière fois juste avant), et surtout **aucun ordre ne doit
        // bouger** — c'est ce qui distingue ce geste du déplacement.
        if drag.resizing.is_some() {
            return DragOutcome::Resized;
        }
        // ⛔ **LA BARRE DU BAS EXIGE Ctrl** — voir `deplacement_autorise`, où le
        // pourquoi est écrit. Refusé AVANT tout calcul : rien n'a bougé, donc rien
        // à persister, et aucun ordre à remettre.
        if !self.deplacement_autorise(&drag) {
            return DragOutcome::Refused;
        }
        let target_side = self.drag_target_side(&drag, layout);
        // CE QUI SE DÉPLACE — le bloc, pas seulement l'élément saisi (voir `bloc_deplace`) : pour un
        // séparateur de zone, il emporte son contenu, sans quoi déplacer une zone la viderait sur
        // ses voisines.
        let bloc = self.bloc_deplace(&drag);
        // `others` = la barre cible MOINS TOUT LE BLOC : c'est la disposition que `drag_insert_index`
        // mesure, donc la géométrie que voit l'utilisateur pendant le geste, et l'index rendu est un
        // rang dans cette liste-là.
        let others: Vec<usize> = self
            .order(target_side)
            .iter()
            .copied()
            .filter(|index| !bloc.contains(index))
            .collect();
        // REFUS par la GÉOMÉTRIE — le curseur n'est dans aucune zone d'une barre
        // qui en a (voir `drag_insert_index`). Rien n'a bougé, donc rien à
        // persister : c'est le MÊME variant que le refus par verrou ci-dessous,
        // et c'est délibéré — « le lâcher n'a pas eu lieu » se dit d'une seule
        // façon.
        let Some(insert_at) = self.drag_insert_index(&others, &drag, target_side, layout) else {
            return DragOutcome::Refused;
        };
        let insert_at = insert_at.min(others.len());
        // …ET LA POSITION AUSSI EST BORNÉE, pour la raison SYMÉTRIQUE de celle qui arrête le bloc
        // devant `is_persistable` : lâcher un bloc SOUS le miroir de notification (ou sous les
        // applications ouvertes) le placerait après des icônes qui ne s'écrivent pas. Rien ne le
        // dirait sur l'écran, et le déplacement serait PERDU au redémarrage — l'ordre relu ne
        // contient que ce qui se persiste. On l'arrête donc devant la première d'entre elles.
        //
        // Inoffensif là où il n'y a ni miroir ni applications ouvertes (le cas ordinaire) : la
        // recherche ne trouve alors rien et la position reste celle du geste.
        let insert_at = match others.iter().position(|&index| !self.is_persistable(index)) {
            Some(plafond) => insert_at.min(plafond),
            None => insert_at,
        };
        // Retire D'ABORD l'icône de sa barre d'origine — un no-op si
        // `target_side == drag.from_side` (le cas le plus courant, un
        // simple réordonnancement SANS Ctrl) puisqu'elle y est justement
        // encore présente à ce moment.
        // VERROU — la zone où l'on DÉPOSE est-elle figée ? Le glisser ne devient
        // alors RIEN : l'icône reste où elle est, et l'ordre persisté est
        // inchangé. Sans ce contrôle, une zone figée pourrait encore ACCUEILLIR
        // une icône venue d'ailleurs — son contenu ENTRERAIT, ce que le verrou
        // doit précisément empêcher.
        //
        // **Contrôle par l'icône VOISINE du point d'insertion**, et non par la
        // boîte géométrique de la zone : le déposer dans une zone figée VIDE
        // n'est donc pas refusé (aucune icône à interroger). Limite connue, et
        // assumée : elle ne se voit que dans ce cas-là. Le contrôle géométrique,
        // lui, est désormais À PORTÉE (`drag_insert_index` résout la zone
        // survolée depuis le 2026-09-22) mais il ne la franchit pas encore :
        // l'index rendu n'est pas la zone, et le faire remonter jusqu'ici est un
        // changement à part, non demandé.
        let destination = others.get(insert_at.min(others.len().saturating_sub(1))).copied();
        if destination.is_some_and(|neighbour| self.is_locked_at(neighbour)) {
            return DragOutcome::Refused;
        }
        // LE BLOC ENTIER, D'UN SEUL TENANT — et retiré de sa barre d'origine AVANT d'être réinséré,
        // pour que `insert_at` (un rang dans `others`, qui l'exclut déjà) reste valable dans les
        // deux cas : une réorganisation sur place (le cas courant) comme un passage à l'autre barre.
        self.order_mut(drag.from_side).retain(|index| !bloc.contains(index));
        let destination = self.order_mut(target_side);
        for (offset, &icon) in bloc.iter().enumerate() {
            destination.insert(insert_at + offset, icon);
        }
        DragOutcome::Moved
    }

    /// Zone géométrique sous `(x, y)`, LES QUATRE confondues (voir `Side`,
    /// `layout_zone`) — `None` si `(x, y)` ne tombe dans AUCUNE des quatre
    /// (le reste de l'écran, sous la barre du haut et hors des deux barres
    /// latérales). La barre du haut se partage en deux MOITIÉS (gauche/
    /// droite de son milieu horizontal) — demande explicite : "pouvoir
    /// déplacer les icônes des barres de gauche et de droite dans la barre
    /// supérieure... [selon] le côté de la barre supérieure où elles sont
    /// déposées" : une zone de dépôt TOUJOURS disponible sur toute sa
    /// moitié, même quand elle ne contient encore AUCUNE icône du dock (pas
    /// seulement la petite emprise de ses icônes actuelles, qui pourrait
    /// être vide).
    fn zone_at(&self, x: f32, y: f32, layout: &DockLayout) -> Option<Side> {
        if y < layout.haut_colonnes {
            // ⭐ **LE MILIEU EXISTE COMME CIBLE** (2026-10-30) : c'est l'horloge et les
            // applets qui l'entourent. Sans lui, un glisser au centre de la barre du haut
            // tombait dans la moitié GAUCHE ou DROITE, et rien ne pouvait jamais atterrir
            // au milieu — la liste `icones_milieu` restait vide par construction.
            //
            // La bande est celle de l'horloge ET de sa garniture (note à gauche, cloche et
            // minuteur à droite), élargie d'une icône : c'est la zone qu'on voit comme « le
            // milieu ». Sans horloge connue (`0.0`), le partage à la moitié reprend, intact.
            if self.clock_width > 0.0 {
                let centre = layout.total_width * 0.5;
                let garniture =
                    top_icon_size_for(layout.top_thickness) + crate::ui::topbar::NOTE_MARGIN_RIGHT;
                let demi = self.clock_width * 0.5 + garniture;
                if (x - centre).abs() <= demi {
                    return Some(Side::TopMiddle);
                }
            }
            return Some(if x < layout.total_width * 0.5 { Side::TopLeft } else { Side::TopRight });
        }
        if x < layout.left_width {
            return Some(Side::Left);
        }
        if x > layout.total_width - layout.right_width {
            return Some(Side::Right);
        }
        None
    }

    /// Barre/zone CIBLE d'un glisser en cours — `from_side` tant que Ctrl
    /// n'est pas maintenu (voir `IconDrag::ctrl_held`, demande explicite :
    /// "changer les icônes de barre en utilisant la touche ctrl EN PLUS du
    /// clic gauche"), sinon la zone RÉELLE sous le curseur (voir `zone_at`) —
    /// `from_side` par défaut si le curseur ne survole AUCUNE des quatre
    /// zones (dead zone au milieu de l'écran) : un glisser qui reste
    /// clairement dans sa zone d'origine, ou passe momentanément par une
    /// zone morte, ne bascule jamais par erreur.
    /// **CE GESTE A-T-IL LE DROIT DE DÉPLACER CETTE ICÔNE ?** — la RÈGLE, en un
    /// seul endroit parce que DEUX fonctions la consultent.
    ///
    /// ⛔ **LA BARRE DU BAS EXIGE Ctrl** — DEMANDE EXPLICITE : « les dossiers ne
    /// devraient pas être organisables avec un simple clic gauche ». Ailleurs, le
    /// glisser simple réordonne ; ici, il **ouvrirait** le dossier s'il était un
    /// clic, et le réordonnerait s'il est un glisser : deux gestes voisins, deux
    /// effets sans aucun rapport. Ctrl les sépare.
    ///
    /// ⚠️ **ELLE SERT AUSSI À `drag_preview`**, et ce n'est pas un détail : sans
    /// ça, le geste REFUSÉ montrerait quand même l'icône flottante qui suit le
    /// curseur, et ne ferait rien au lâcher. C'est exactement le défaut qu'on vient
    /// de corriger sur cette même barre — **un geste qui promet**.
    fn deplacement_autorise(&self, drag: &IconDrag) -> bool {
        drag.from_side != Side::Bottom || drag.ctrl_held
    }

    fn drag_target_side(&self, drag: &IconDrag, layout: &DockLayout) -> Side {
        // ⛔ **LA BARRE DU BAS RESTE CHEZ ELLE, MÊME AVEC Ctrl** — DEMANDE
        // EXPLICITE : « glisser l'icône avec Ctrl + clic gauche ». Pour les quatre
        // autres barres, Ctrl veut dire « viser une AUTRE barre » ; pour celle-ci,
        // ça n'a jamais rien voulu dire : `zone_at` ne rend jamais `Bottom`, donc
        // un Ctrl+glisser depuis le bas visait une barre latérale — un
        // comportement que rien ne définissait, et qu'aucune garde n'arrêtait.
        // Ici, Ctrl réordonne SUR PLACE, comme le glisser simple.
        if drag.from_side == Side::Bottom {
            return Side::Bottom;
        }
        if !drag.ctrl_held {
            return drag.from_side;
        }
        let (x, y) = drag.current_pos;
        self.zone_at(x, y, layout).unwrap_or(drag.from_side)
    }

    /// Index d'insertion prévisualisé DANS `others` (la zone cible, icône
    /// glissée déjà exclue — voir `end_drag`/`icon_rects_map`) — `None` quand le
    /// lâcher **n'a pas lieu** (voir `DragOutcome::Refused`).
    ///
    /// **Trois cas, dans cet ordre** :
    ///
    /// 1. **Barre sans aucune zone**, ou **séparateur de zone** glissé —
    ///    insertion LINÉAIRE, exactement comme avant les grilles : le premier
    ///    emplacement dont le CENTRE tombe sous le curseur, la fin s'il est sous
    ///    tous. Un séparateur n'est jamais contraint parce qu'il DÉFINIT les
    ///    zones : l'enfermer dans l'une d'elles rendrait le réordonnancement
    ///    impossible (décision explicite : « les séparateurs restent libres »).
    /// 2. **Le curseur est DANS une zone** — l'unité visée est la **CASE** de sa
    ///    grille, en DEUX dimensions (demande explicite : « se ranger dans une
    ///    CASE »). C'est ce que la règle d'origine ne savait pas faire : elle ne
    ///    comparait que l'ORDONNÉE, donc deux cases d'une même rangée rendaient
    ///    le même index, et le nombre de colonnes n'avait **aucun** effet sur le
    ///    geste. À **une** colonne, la règle d'origine s'applique au pixel près
    ///    — garantie qu'aucune zone créée avant les grilles ne change.
    /// 3. **Le curseur est HORS de toute zone, dans une barre qui en a** —
    ///    insertion linéaire comme au cas 1, SAUF dans le **VIDE** : sous tout le
    ///    contenu, ou au-dessus de la première icône, il n'y a rien à quoi se
    ///    lier, et le lâcher est REFUSÉ (décision explicite : « refuser
    ///    seulement le vide »). Refuser plus large a un coût MESURÉ : le geste
    ///    qui CRÉE un séparateur (Maj + clic droit sur une icône) le pose juste
    ///    APRÈS elle, donc l'icône se retrouve au-dessus de la première zone —
    ///    refuser là aurait rendu cette icône indéplaçable et silencieusement
    ///    inerte.
    ///
    /// Pour `TopLeft`/`TopRight` (empilage HORIZONTAL, voir `layout_top_side`),
    /// l'équivalent horizontal d'origine — façon glisser-déposer classique d'une
    /// liste (insère AVANT cet emplacement-là), **sans aucune notion de zone** :
    /// ce bras ne construit pas de `BarLayout`, donc rien n'y est jamais refusé.
    ///
    /// Les positions viennent de `slot_geometry` / `column_band` / `colonne_sous`
    /// — **les mêmes que `layout_side` emploie pour dessiner** — sur la MÊME
    /// liste visible (`ordre_visible`), avec le MÊME `start_y` et le MÊME
    /// défilement de zone : un lâcher doit viser la case telle qu'elle se voit.
    /// Cette fonction mettait auparavant les défilements à zéro et mesurait sur
    /// la liste COMPLÈTE, ce qui décalait la grille d'une case par icône
    /// désactivée.
    fn drag_insert_index(&self, others: &[usize], drag: &IconDrag, side: Side, layout: &DockLayout) -> Option<usize> {
        match side {
            Side::Left | Side::Right => {
                let (_, bar_width) = Self::lateral_bar_span(side, layout);
                let icon_size = lateral_icon_size_for(side, bar_width);
                // MÊME `start_y` ET MÊME défilement de zone que `layout_side`
                // (voir sa doc) : ce qui suit mesure la disposition DESSINÉE.
                let start_y = layout.haut_colonnes + TOP_MARGIN - self.decalage_barre(side, layout);
                // **LA MÊME LISTE QUE CE QUI EST DESSINÉ.** `layout_side` passe
                // par `ordre_visible`, qui écarte les icônes DÉSACTIVÉES et les
                // index hors bornes ; mesurer sur la liste complète décalait la
                // grille d'une case par icône désactivée — le défaut même que ce
                // calcul répare, resté vivant sous cette forme.
                let visibles = self.ordre_visible(others, "lacher");
                let bar = self.bar_layout(side, 
                    &visibles,
                    icon_size,
                    start_y,
                    &|sep| self.zone_scroll_offset(sep),
                    &self.zone_grid(side, icon_size, bar_width),
                );
                let (x, y) = drag.current_pos;
                // Ramène un rang de `visibles` à un rang de `others` : c'est
                // `others` qui reçoit l'insertion (les icônes désactivées y
                // gardent leur place). **Identité quand rien n'est désactivé**,
                // c'est-à-dire dans le cas ordinaire — aucun geste existant ne
                // change.
                let dans_others = |rang: usize| -> usize {
                    visibles.get(rang).and_then(|&icone| others.iter().position(|&i| i == icone)).unwrap_or(others.len())
                };
                // CAS 1 — la règle d'origine, intacte. `is_zone_separator` suffit
                // à écarter le séparateur : un `zone_height` non nul n'existe que
                // sur un espace (voir `Icon::zone_height`).
                if bar.zones.is_empty() || self.is_zone_separator(drag.icon_index) {
                    let rang = bar
                        .slots
                        .iter()
                        .position(|slot| y < slot.top + slot.height * 0.5)
                        .unwrap_or(visibles.len());
                    return Some(dans_others(rang));
                }
                // CAS 2 — LA ZONE SURVOLÉE. Même test que `zone_separator_at`,
                // qui ne prend lui aussi que `y` : une zone est une étendue
                // VERTICALE, sa largeur n'entre pas dans le choix (elle occupe la
                // barre).
                if let Some(rank) = bar.zones.iter().position(|zone| y >= zone.top && y < zone.bottom) {
                    let zone = bar.zones[rank];
                    // LE CONTENU DE CETTE ZONE : de justesse après son séparateur,
                    // jusqu'à ce que `slot.zone` cesse de la désigner. C'est
                    // l'intervalle où un lâcher a le droit d'atterrir, et il est
                    // BORNÉ — insérer au-delà serait sortir de la zone.
                    let debut = visibles.iter().position(|&i| i == zone.sep_index).map_or(visibles.len(), |p| p + 1);
                    let fin = (debut..bar.slots.len())
                        .find(|&i| bar.slots[i].zone != Some(rank))
                        .unwrap_or(bar.slots.len());
                    // Les colonnes sont celles du SÉPARATEUR, résolues une fois par
                    // `bar_layout` pour tout son contenu (voir `zone_grid`) : elles
                    // sont donc uniformes dans cet intervalle.
                    let columns = self.effective_zone_columns(zone.sep_index, side, bar_width);
                    let rang = if columns <= 1 {
                        // LA RÈGLE D'ORIGINE, au pixel près — et c'est le point :
                        // une zone SANS grille continue de se comporter exactement
                        // comme avant qu'elles existent, la même garantie que
                        // `bar_layout` se donne pour `columns == 1`. Le CENTRE de
                        // l'emplacement tranche : au-dessus → avant lui, en dessous
                        // → avant le suivant.
                        (debut..fin)
                            .find(|&i| y < bar.slots[i].top + bar.slots[i].height * 0.5)
                            .unwrap_or(fin)
                    } else {
                        // UNE GRILLE : l'unité visée est la CASE. On cherche
                        // celle qui CONTIENT le curseur, dans les DEUX dimensions.
                        //
                        // « Contient » se dit `colonne_sous(x) == slot.column` :
                        // les bandes de `column_band` PAVENT la barre, donc
                        // « la case sous le curseur » est définie partout, sans jeu
                        // entre deux colonnes — et `column_band` est le MÊME calcul
                        // que `slot_geometry` emploie pour dessiner la case.
                        //
                        // `is_spacer` et NON `visibles[i].is_spacer` : c'est
                        // l'accès INDEXÉ qui a déjà fait tomber tout le dock (voir
                        // `ordre_visible`). Un ordre peut garder l'index d'une
                        // icône disparue, et `bar_layout` y survit par
                        // construction (`is_spacer`/`zone_height` passent par
                        // `get`) — cette ligne était le premier accès direct du
                        // chemin de lâcher.
                        (debut..fin)
                            .find(|&i| {
                                let slot = bar.slots[i];
                                if y < slot.top || y >= slot.top + slot.height {
                                    return false;
                                }
                                // Un espace occupe la rangée ENTIÈRE (voir
                                // `bar_layout`) : rien à départager en abscisse.
                                if self.is_spacer(visibles[i]) {
                                    return true;
                                }
                                Self::colonne_sous(side, layout, columns, x) == slot.column
                            })
                            // AUCUNE case sous le curseur : la rangée qui suit
                            // commence plus bas — le curseur est donc dans la
                            // GOUTTIÈRE entre deux rangées, ou au-dessus du
                            // contenu (il tombe alors sur la rangée du haut).
                            // On y CONSERVE LA COLONNE visée : la perdre renvoyait
                            // en colonne 1 un curseur qui visait la dernière.
                            .or_else(|| {
                                let premier = (debut..fin).find(|&i| y < bar.slots[i].top)?;
                                let haut = bar.slots[premier].top;
                                let visee = Self::colonne_sous(side, layout, columns, x);
                                Some(
                                    premier
                                        + (premier..fin)
                                            .take_while(|&i| bar.slots[i].top == haut && bar.slots[i].column < visee)
                                            .count(),
                                )
                            })
                            // Toujours rien : le curseur est SOUS le contenu de la
                            // zone, où la place existe encore — il va donc en fin
                            // de zone, jamais au début d'une autre.
                            .unwrap_or(fin)
                    };
                    return Some(dans_others(rang));
                }
                // CAS 3 — hors de toute zone. Le VIDE est refusé : sous tout le
                // contenu, ou au-dessus de la première icône, il n'y a rien à
                // quoi se lier (voir la doc, cas 3). Partout ailleurs, la règle
                // linéaire d'avant.
                let Some(premier) = bar.slots.first() else { return Some(dans_others(0)) };
                let dernier = bar.slots.last().expect("non vide : `premier` l'a prouvé");
                if y < premier.top || y >= dernier.top + dernier.height {
                    return None;
                }
                let rang = bar
                    .slots
                    .iter()
                    .position(|slot| y < slot.top + slot.height * 0.5)
                    .unwrap_or(visibles.len());
                Some(dans_others(rang))
            }
            Side::TopLeft | Side::TopRight | Side::TopMiddle => {
                let icon_size = top_icon_size_for(layout.top_thickness);
                let step = icon_size + self.icon_gap_side(side);
                let (x, _) = drag.current_pos;
                let anchor_x = match side {
                    Side::TopLeft => TOP_ZONE_CORNER_MARGIN,
                    Side::TopRight => layout.total_width - TOP_ZONE_CORNER_MARGIN,
                    // ⚠️ **LE MILIEU POUSSE VERS LA GAUCHE**, depuis le bord droit de sa
                    // rangée — c'est-à-dire depuis ce qui précède l'horloge (voir
                    // `layout_zone`). L'ancre du geste doit être la MÊME que celle du
                    // dessin, sinon on dépose une icône à un endroit et elle apparaît à un
                    // autre.
                    Side::TopMiddle => {
                        if self.clock_width > 0.0 {
                            (layout.total_width - self.clock_width) * 0.5
                                - crate::ui::topbar::NOTE_MARGIN_RIGHT
                                - icon_size
                                - self.icon_gap_side(Side::TopMiddle)
                        } else {
                            layout.total_width * 0.5
                        }
                    }
                    Side::Left | Side::Right | Side::Bottom => unreachable!(),
                };
                // ⚠️ Seule la moitié GAUCHE du haut pousse vers la droite ; la moitié
                // droite et le MILIEU poussent vers la gauche (le milieu, depuis le bord
                // gauche de l'horloge — voir `layout_zone`).
                let grow_right = side == Side::TopLeft;
                Some(
                    (0..others.len())
                        .position(|i| {
                            let center = if grow_right {
                                anchor_x + i as f32 * step + icon_size * 0.5
                            } else {
                                anchor_x - i as f32 * step - icon_size * 0.5
                            };
                            if grow_right { x < center } else { x > center }
                        })
                        .unwrap_or(others.len()),
                )
            }
            // ⛔ **CE BRAS RENDAIT `None`, ET POUR UNE RAISON PÉRIMÉE** : il
            // affirmait que `bottom_order` est toujours vide. **C'est faux** depuis
            // `set_dossiers`, qui la remplit à chaque fois — donc le geste
            // COMMENÇAIT, l'icône flottante suivait le curseur, et le lâcher ne
            // faisait RIEN. C'est le pire des états : il promet.
            //
            // ⚠️ **ON MESURE LA DISPOSITION DESSINÉE**, exactement comme
            // `layout_bottom_side` : même ancre (`left_width +
            // TOP_ZONE_CORNER_MARGIN` — la barre du bas occupe le MILIEU, elle ne
            // part pas du bord de l'écran), même taille
            // (`bottom_icon_size_for`, qui n'est PAS `icon_size_for`), même écart,
            // et la même liste filtrée par `ordre_visible`.
            Side::Bottom => {
                let visibles = self.ordre_visible(others, "lacher (barre du bas)");
                // ⛔ **ON MESURE, ON NE MODÉLISE PAS — ET C'EST LA CORRECTION.**
                // La première version calculait une grille RÉGULIÈRE
                // (`ancre + i × (taille + écart)`). **FAUX** : la barre du bas a
                // des espaces ÉLASTIQUES qui absorbent la largeur restante (voir
                // `layout_bottom_side`), donc les icônes qui les suivent sont
                // DÉCALÉES — et six séparateurs élastiques en tête suffisent à
                // rendre le dépôt faux partout. Constaté par l'utilisateur : « ça
                // bouge, mais pas au bon endroit ».
                //
                // ⇒ On appelle donc **la fonction qui dessine la barre**, sur la
                // liste cible, et on lit les places qu'elle rend. Ce qui est mesuré
                // est exactement ce qui est affiché, élastiques compris — la même
                // règle que le bras latéral, qui passe par `bar_layout`.
                let mut map = vec![None; self.icons.len()];
                self.layout_bottom_side(
                    others,
                    layout,
                    layout.left_width + TOP_ZONE_CORNER_MARGIN,
                    bottom_band_width(layout),
                    &mut map,
                );
                let centre_x = |icone: usize| -> Option<f32> {
                    map.get(icone)
                        .and_then(|place| *place)
                        .map(|place| place[0] + place[2] * 0.5)
                };
                let (x, _) = drag.current_pos;
                // Ramène un rang de `visibles` à un rang de `others` — même
                // mécanisme que le bras latéral, et pour la même raison : une icône
                // désactivée garde sa place dans `others`.
                let dans_others = |rang: usize| -> usize {
                    visibles
                        .get(rang)
                        .and_then(|&icone| others.iter().position(|&i| i == icone))
                        .unwrap_or(others.len())
                };
                // Le premier emplacement dont le CENTRE dépasse le curseur — la
                // règle linéaire des barres horizontales (voir le bras `TopLeft`).
                // ⚠️ Une icône SANS place (hors du champ, cas rare) arrête la
                // recherche : on insère devant elle plutôt que de la sauter.
                let rang = visibles
                    .iter()
                    .position(|&icone| centre_x(icone).is_some_and(|centre| x < centre))
                    .unwrap_or(visibles.len());
                // ⛔ **ET RIEN NE PASSE APRÈS LE « + ».** Il est la DERNIÈRE case de
                // la plage (`base + count`, voir `set_dossiers`) et
                // `plus_dossier_index` le calcule ainsi : insérer derrière lui
                // ferait diverger les deux, et l'utilisateur verrait le bouton
                // d'ajout au MILIEU de ses dossiers.
                let plafond = visibles
                    .iter()
                    .position(|&index| self.is_plus_dossier(index))
                    .unwrap_or(visibles.len());
                Some(dans_others(rang.min(plafond)))
            }
        }
    }

    /// Icône et rectangle FLOTTANT à dessiner par-dessus tout le reste
    /// pendant un glisser (voir `IconDrag::dragging`, demande explicite :
    /// "l'icône suit le curseur, les autres se décalent en direct") — centré
    /// sur la position courante de la souris, taille de la barre CIBLE (voir
    /// `drag_target_side` : peut différer de la barre d'origine si Ctrl
    /// fait passer le curseur dans l'autre barre, dont l'épaisseur peut ne
    /// pas être la même). `None` tant que `dragging` n'est pas encore vrai
    /// (voir `DRAG_THRESHOLD`) : un simple clic n'affiche rien de spécial.
    pub fn drag_preview(&self, layout: &DockLayout) -> Option<(usize, [f32; 4])> {
        // `!d.resizing` : pendant un REDIMENSIONNEMENT il n'y a pas d'icône
        // flottante — l'élément ne se déplace pas, il grandit sur place (voir
        // `IconDrag::resizing`). Sans ce filtre, l'espace apparaîtrait deux
        // fois : une fois à sa place (qui s'allonge) et une fois sous le
        // curseur.
        let drag = self
            .drag
            .as_ref()
            .filter(|d| d.dragging && d.resizing.is_none() && self.deplacement_autorise(d))?;
        let side = self.drag_target_side(drag, layout);
        let icon_size = match side {
            Side::Left => lateral_icon_size_for(Side::Left, layout.left_width),
            Side::Right => lateral_icon_size_for(Side::Right, layout.right_width),
            Side::TopLeft | Side::TopRight | Side::TopMiddle => top_icon_size_for(layout.top_thickness),
            Side::Bottom => bottom_icon_size_for(layout.bottom_thickness),
        };
        let (x, y) = drag.current_pos;
        Some((drag.icon_index, [x - icon_size * 0.5, y - icon_size * 0.5, icon_size, icon_size]))
    }

    /// Instance de quad pour l'aperçu flottant (voir `drag_preview`) — TOUJOURS
    /// en aplat de sa couleur de repli (`Icon::color`), même pour une icône
    /// normalement texturée (logo, icône d'application extraite, glyphe...) :
    /// éviter de dupliquer ici la correspondance icône → texture externe que
    /// `app::run` gère déjà pour le rendu normal (`external_icon_instance`/
    /// `external_glyph_instance`, une texture différente par icône) — la
    /// couleur de repli suffit à identifier l'icône déplacée le temps du
    /// glisser, sa VRAIE texture réapparaît dès le dépôt (voir
    /// `render_instances`, qui l'exclut le temps du glisser). `None` hors
    /// glisser (voir `drag_preview`) — à dessiner PAR-DESSUS tout le reste
    /// (voir `app::run`, dernier lot envoyé au pipeline). Pour un espace
    /// invisible (voir `Icon::is_spacer`), le petit cercle indicateur (voir
    /// `spacer_marker_instance`) plutôt qu'un aplat de couleur — demande
    /// explicite : "un petit cercle dans les espaces invisibles pour qu'il
    /// soit plus évident de les visualiser", y compris PENDANT qu'on le
    /// déplace, pas seulement au repos.
    pub fn drag_preview_instance(&self, layout: &DockLayout, separator_color: [f32; 4], separator_size: f32) -> Option<QuadInstance> {
        let (index, rect) = self.drag_preview(layout)?;
        let icon = self.icons.get(index)?;
        Some(if icon.is_spacer {
            spacer_marker_instance(rect, 1.0, separator_color, separator_size)
        } else {
            QuadInstance::new(rect, icon.color, icon_radius(), 1.0)
        })
    }

    /// Instance de quad pour une icône à texture externe (voir
    /// `Icon::external_texture`) — même géométrie/rayon d'arrondi/échelle
    /// de survol que celles rendues par `render_instances`, laissée à
    /// l'appelant pour l'associer à sa propre texture (le pipeline ne lie
    /// qu'une texture par lot, voir `gfx::quad_renderer`).
    pub fn external_icon_instance(&self, index: usize, layout: &DockLayout) -> Option<QuadInstance> {
        // ⭐ Une icône REPLIÉE : entière, basculée, sans magnification (voir
        // `bascule_de`).
        if let Some((rect, angle)) = self.bascule_de(index, layout) {
            let mut instance = QuadInstance::new_textured(rect, icon_radius(), 1.0);
            instance.bascule = angle;
            return Some(instance);
        }
        // Rectangle PRÉSENTÉ (grossi, puis ramené dans la fenêtre) avec
        // `scale = 1.0` : la boîte porte DÉJÀ la magnification, l'échelle ne
        // doit donc pas être appliquée une seconde fois. Les deux formes sont
        // exactement équivalentes — le calcul du shader est linéaire
        // (`center + LocalPos * rect.zw * scale`) — mais celle-ci fait porter
        // toute la transformation par un seul rectangle, celui-là même qui sert
        // au clic, à l'ancrage des popups et au débordement de région.
        let rect = self.presented_rect(index, layout)?;
        let mut instance = QuadInstance::new_textured(rect, icon_radius(), 1.0);
        // BALANCEMENT au survol (voir `hover_wobble_angle`) — appliqué à la
        // source, donc à TOUTES les familles d'icônes d'un coup, y compris la
        // copie que `app::run` repousse en dernier.
        instance.rotation = hover_wobble_angle(self.hover.scale(index));
        Some(instance)
    }

    /// Comme `external_icon_instance`, mais pour une icône dont la texture
    /// est un glyphe vectoriel MONOCHROME (ex. Lanceur/Presse-papiers, voir
    /// `TopbarIcons/SOURCE.md` — même style Iconsax que les glyphes de la
    /// barre du haut/l'icône réglages) plutôt qu'une vraie icône
    /// d'application multicolore : teintée par `color` pour rester cohérente
    /// avec ces glyphes-là (teinté par `effective_topbar_text_color`, voir
    /// `app::run` — Réglages/alimentation/terminal/presse-papiers rapide/
    /// son/réseau utilisent tous CETTE fonction, `Icon::external_texture`
    /// étant vrai pour eux, voir `Dock::new`) — BUG RÉEL SIGNALÉ ("certaines
    /// icônes de la barre de gauche ne s'affichent pas de la même couleur
    /// que les icônes des autres barres") : ces deux icônes passaient
    /// jusqu'ici par `external_icon_instance` (texture BRUTE, non teintée,
    /// pensée pour une vraie icône d'application dont la couleur ne doit
    /// justement PAS être touchée), et s'affichaient donc dans la couleur
    /// native du PNG plutôt que la teinte réglée par l'utilisateur.
    pub fn external_glyph_instance(&self, index: usize, layout: &DockLayout, color: [f32; 3]) -> Option<QuadInstance> {
        // Rectangle présenté + `scale = 1.0` — voir `external_icon_instance`.
        let rect = self.presented_rect(index, layout)?;
        // **UN GLYPHE N'A RIEN À FAIRE DANS LA CASE D'UN ESPACE — ET C'EST LA SONDE.**
        //
        // Défaut signalé le 2026-09-22, NON ÉLUCIDÉ : une forme grise, CREUSE,
        // large de ~92 pour 18 de haut, apparaît sur un séparateur de zone d'une
        // instance qui a VÉCU (un redémarrage l'efface ; le processus n'avait
        // pourtant pas été relancé quand elle a disparu). `bar_layout` ne donne
        // 18 px de haut qu'à un ESPACE (`if is_spacer { SPACER_HEIGHT } else
        // { cell }`) : ce qui est dessiné là est donc un glyphe carré ÉCRASÉ dans
        // une case d'espace. Cette fonction ne regarde jamais `is_spacer` — elle
        // rend la case qu'on lui donne, quelle qu'elle soit.
        //
        // **La sonde NE CHANGE PAS LE COMPORTEMENT : elle NOMME l'index**, et
        // c'est le seul moyen de le savoir — l'état est PROPRE au lancement
        // suivant, donc invisible pour le harnais (qui ne simule aucune entrée,
        // voir `NEURADESK_DEBUG_ZONE`) comme pour une capture d'un processus neuf.
        // L'index se résout en clair avec les sept applets journalisés au
        // démarrage par `app::run` (« applets : … »).
        if self.is_spacer(index) {
            if GLYPHE_SUR_UN_ESPACE.with(Cell::get) != Some(index) {
                GLYPHE_SUR_UN_ESPACE.with(|c| c.set(Some(index)));
                crate::diag!(
                    "sonde: glyphe dessine dans la case d'un ESPACE — index {index}, case {rect:?}"
                );
            }
        } else {
            GLYPHE_SUR_UN_ESPACE.with(|c| c.set(None));
        }
        // ⭐ Une icône REPLIÉE : entière, basculée (voir `bascule_de`).
        if let Some((entiere, angle)) = self.bascule_de(index, layout) {
            let mut instance = Self::instance_icone(entiere, color, 1.0);
            instance.rotation = 0.0;
            instance.bascule = angle;
            return Some(instance);
        }
        Some(Self::instance_icone(rect, color, self.hover.scale(index)))
    }

    /// **L'INSTANCE D'UNE ICÔNE D'APPLET — LE POINT UNIQUE.**
    ///
    /// ⛔ **IL Y A DEUX FAMILLES D'ICÔNES, ET LA DIFFÉRENCE EST ICI.** Les thèmes
    /// monochromes (« NeuraDesk », « Fluent ») livrent un PNG ou un glyphe dont
    /// SEUL L'ALPHA compte : `new_glyph_scaled` le repeint dans la teinte `color`.
    /// Le thème « Symboles » livre au contraire des emoji DÉJÀ COLORÉS, et les
    /// teinter les rendrait BLANCS — d'où `new_textured`, le mode des VRAIES
    /// icônes d'application (`TexMode::Texture`, qui dessine le RGB tel quel :
    /// voir sa doc). `color` devient inutilisé sur ce chemin, et c'est la
    /// définition même de ce mode.
    ///
    /// ⛔ **ET C'EST POURQUOI ELLE EST UNE FONCTION, PAS UN `if` RECOPIÉ.** Un
    /// premier jet l'avait mise en un seul endroit — `external_glyph_instance` —
    /// en croyant que TOUTES les applets y passaient. **C'ÉTAIT FAUX** : quatre
    /// widgets de la barre du haut en construisent une eux-mêmes
    /// (`topbar::{show_desktop_instance, bell_instance, minuteur_instance,
    /// note_instance}`), et « Afficher le bureau » s'est donc affiché en **carré
    /// blanc** — l'alpha de l'emoji fenêtre, peint dans la teinte. Signalé par
    /// l'utilisateur.
    ///
    /// Les autres fonctions de `topbar` dessinent du TEXTE (horloge, débit
    /// d'images, titre du média) : elles n'appellent pas ceci, et n'ont pas à le
    /// faire.
    pub fn instance_icone(rect: [f32; 4], color: [f32; 3], hover_scale: f32) -> QuadInstance {
        let mut instance = if crate::ui::icones::est_couleur() {
            QuadInstance::new_textured(rect, icon_radius(), 1.0)
        } else {
            QuadInstance::new_glyph_scaled(rect, color, 1.0)
        };
        // Voir `external_icon_instance` : le balancement s'applique à la source,
        // donc à toutes les familles d'icônes.
        instance.rotation = hover_wobble_angle(hover_scale);
        instance
    }

    /// Avance les animations d'un pas `dt` (secondes). L'icône en cours de
    /// glisser (voir `is_dragging`) n'a pas de rectangle "de grille" (voir
    /// `icon_rects_map`) — sa case dans `scales` garde simplement sa
    /// dernière valeur, sans incidence : `drag_preview`, pas `render_instances`,
    /// dessine cette icône-là pendant un glisser.
    pub fn update(&mut self, dt: f32, layout: &DockLayout) {
        // Le défilement de chaque barre latérale est re-borné ICI, une fois par
        // frame, et pas seulement au coup de molette : le contenu change aussi
        // de hauteur SANS elle — une application qui se ferme, une icône
        // désactivée dans les Réglages, un espace redimensionné, une épaisseur
        // de barre modifiée. Un décalage resté au-delà de la nouvelle fin
        // laisserait un trou sous la dernière icône jusqu'au prochain cran.
        // C'est le seul point d'entrée par frame qui dispose du `layout` — pour
        // CE dock. **Il n'est appelé que par `app::run`, donc pour le dock
        // PRIMAIRE** (voir `magnified_index`, doc de fonction, qui documente ce
        // même fait) : un dock d'écran SECONDAIRE garde donc son défaut — son
        // contenu peut changer de hauteur (icône désactivée POUR CET ÉCRAN, voir
        // `dock_instance`) sans que son décalage soit re-borné avant le prochain
        // cran de molette. Le combler demanderait de rejouer ce re-bornage depuis
        // le chemin par frame de `DockInstance`, qui n'appelle jamais `update`.
        //
        // AVANT de construire `rects` : le survol doit être animé contre les
        // positions corrigées, pas contre celles d'un décalage périmé.
        self.reclamp_scroll(Side::Left, layout);
        self.reclamp_scroll(Side::Right, layout);
        self.animer_les_pliages(dt, layout);

        // Les cases VISIBLES (voir `cases_de_la_souris`), pas les cases entières :
        // la moitié cachée d'une icône défilée ne doit pas la faire grossir.
        let rects: Vec<[f32; 4]> =
            self.cases_de_la_souris(layout).into_iter().map(|r| r.unwrap_or([0.0, 0.0, 0.0, 0.0])).collect();
        let mouse_pos = self.mouse_pos;
        self.hover.update(&rects, mouse_pos, dt);
    }

    /// ⛔ **LES CASES DE LA SOURIS** (survol et clic) : celles d'`icon_rects_map`,
    /// ROGNÉES pour une barre latérale à ce que le dessin en montre — sous la
    /// barre du haut (`lateral_clip`), et dans la boîte de sa zone
    /// (`zone_clips`). `None` pour une case entièrement rognée.
    ///
    /// Défaut SIGNALÉ (2026-10-15) : l'icône à CHEVAL sur le haut de la colonne
    /// gardait sa case entière ; sa moitié passée sous la barre du haut
    /// recouvrait « Afficher le bureau », y grossissait au survol et
    /// réapparaissait, et un clic l'aurait lancée (`icon_at` passe avant la
    /// barre du haut, voir `app::run`). `icon_rects_map` reste la case ENTIÈRE :
    /// le dessin en a besoin, la découpe s'en charge.
    ///
    /// Verticalement seulement, comme les découpes elles-mêmes : la
    /// magnification déborde librement sur les côtés.
    fn cases_de_la_souris(&self, layout: &DockLayout) -> Vec<Option<[f32; 4]>> {
        let zones = self.zone_clips(layout);
        self.icon_rects_map(layout)
            .into_iter()
            .enumerate()
            .map(|(i, rect)| self.rogner_a_ce_qui_est_dessine(i, rect?, &zones, layout))
            .collect()
    }

    /// `rect` de l'icône `index` rogné à ce que sa découpe dessine (voir
    /// `cases_de_la_souris`) — tel quel hors des barres latérales.
    fn rogner_a_ce_qui_est_dessine(&self, index: usize, rect: [f32; 4], zones: &[Option<[f32; 4]>], layout: &DockLayout) -> Option<[f32; 4]> {
        if !matches!(self.icon_side(index), Some(Side::Left | Side::Right)) {
            return Some(rect);
        }
        let clip = zones.get(index).copied().flatten().unwrap_or_else(|| Self::lateral_clip(layout));
        let haut = rect[1].max(clip[1]);
        let bas = (rect[1] + rect[3]).min(clip[1] + clip[3]);
        (bas > haut).then_some([rect[0], haut, rect[2], bas - haut])
    }

    /// Construit la liste des icônes à envoyer au GPU. Les fonds des trois
    /// barres ne sont plus produits ici : ils sont composés dans
    /// `app::run` à partir d'un arrière-plan flouté teinté (voir
    /// `gfx::backdrop`, `bar_rects`) plutôt que d'un simple aplat de
    /// couleur. N'inclut PAS l'icône en cours de glisser (voir
    /// `icon_rects_map`) : dessinée à part par l'appelant via `drag_preview`,
    /// par-dessus tout le reste. Les espaces invisibles (voir
    /// `Icon::is_spacer`) occupent bien un emplacement dans la disposition,
    /// mais ne dessinent qu'un petit cercle indicateur discret (voir
    /// `spacer_marker_instance`) — demande explicite : "un petit cercle dans
    /// les espaces invisibles pour qu'il soit plus évident de les
    /// visualiser", sans quoi rien ne permettait de repérer où ils sont
    /// pour les glisser/les retirer (voir `app::run`, Ctrl + clic droit).
    /// **Chaque instance est accompagnée de sa BARRE** (`Side`) : le pipeline
    /// ne lie qu'une texture et qu'un découpage à la fois, donc l'appelant doit
    /// regrouper les instances par barre pour pouvoir découper les barres
    /// latérales (voir `lateral_clip`) sans toucher à celle du haut. Déduire la
    /// barre du rectangle de l'instance serait faux — voir `icon_side`.
    ///
    /// **Et de son INDEX d'icône** : l'appelant doit pouvoir RETIRER une icône
    /// précise de ce lot pour la redessiner à part, par-dessus tout le reste —
    /// c'est ainsi que l'icône magnifiée passe devant ses voisines (voir
    /// `app::run`). Sans l'index, il faudrait la reconnaître à son rectangle,
    /// comparaison de flottants fragile, ou recalculer l'ordre du filtre
    /// ci-dessous.
    /// `separator_line_color` : **la couleur du TRAIT, distincte de celle du
    /// marqueur**, et cette distinction est une demande explicite (2026-09-30) :
    /// « qu'ils soient toujours visibles ». Le réglage des séparateurs permet de
    /// les rendre TRANSPARENTS, ce qui est un choix légitime pour un marqueur — un
    /// rond invisible laisse un blanc qui sépare déjà. Mais un TRAIT *est* le
    /// séparateur : transparent, il ne dessine RIEN, et la sorte qu'on vient de
    /// choisir perd tout son sens. L'appelant résout donc la couleur du trait
    /// **visible**, et passe les deux.
    pub fn render_instances(
        &self,
        layout: &DockLayout,
        separator_color: [f32; 4],
        separator_line_color: [f32; 4],
        separator_size: f32,
    ) -> Vec<(usize, Side, QuadInstance)> {
        let rects = self.icon_rects_map(layout);
        // Boucle plutôt que `filter_map` : un espace SURVOLÉ rend DEUX instances
        // (son marqueur, puis le trait pleine largeur — voir plus bas), donc la
        // sortie n'est plus une par icône. L'index est répété sur le trait, ce
        // que l'appelant tolère : il s'en sert pour RETIRER l'icône survolée de
        // son lot et la redessiner par-dessus, ce qui emmène le trait avec elle.
        let mut out = Vec::new();
        for (i, icon) in self.icons.iter().enumerate() {
            if icon.external_texture {
                continue;
            }
            let Some(rect) = rects.get(i).copied().flatten() else { continue };
            let Some(side) = self.icon_side(i) else { continue };
            let scale = self.hover.scale(i);
            let instance = if icon.is_spacer {
                    // **CE QU'UN ESPACE DESSINE DÉPEND DE SA SORTE** (voir
                    // `SorteEspace`) :
                    // - `Rond` : le marqueur des quatre autres barres, inchangé ;
                    // - `Trait` : le trait PERMANENT, perpendiculaire à sa barre —
                    //   la même géométrie que le trait de survol, mais toujours là ;
                    // - `Elastique` : RIEN. Un rond au milieu d'un grand blanc se
                    //   lirait mal, et l'espace qu'il occupe se voit déjà.
                    match icon.sorte {
                        SorteEspace::Rond => {
                            // Les espaces invisibles GARDENT leur géométrie au repos :
                            // leur marqueur est un petit cercle centré dans la case
                            // (voir `spacer_marker_instance`), qui ne déborde jamais de
                            // sa barre — lui faire suivre le rectangle présenté lui
                            // ferait perdre l'agrandissement au survol sans rien
                            // corriger. La demande ne porte que sur les vraies icônes.
                            Some(spacer_marker_instance(rect, scale, separator_color, separator_size))
                        }
                        // ⚠️ **LA COULEUR DU TRAIT, PAS CELLE DU MARQUEUR** — voir
                        // la doc de la fonction : un trait transparent ne
                        // dessinerait rien.
                        SorteEspace::Trait => Some(spacer_line_instance(rect, layout, side, separator_line_color)),
                        SorteEspace::Elastique => None,
                    }
                } else {
                    // Rectangle présenté + `scale = 1.0` — voir
                    // `external_icon_instance`.
                    // ⚠️ **REPLI SUR LA BARRE DU HAUT** quand le côté est inconnu : une
                    // icône hors bornes ne devrait pas arriver (voir `icon_side`), et
                    // `render_instances` ne rend pas d'`Option` — s'y arrêter ferait
                    // disparaître toutes les icônes suivantes pour une seule.
                    let barre = self.icon_side(i).map(BarreTirage::de_la_side).unwrap_or(BarreTirage::Haut);
                    // ⭐ Une icône REPLIÉE est dessinée entière et basculée, sans
                    // magnification (voir `bascule_de`).
                    let bascule = self.bascule_de(i, layout);
                    let rect = match bascule {
                        Some((entiere, _)) => entiere,
                        None => presented_rect_pour_la_barre(rect, scale, layout, barre),
                    };
                    let mut instance = if icon.has_texture {
                        QuadInstance::new_textured(rect, icon_radius(), 1.0)
                    } else {
                        QuadInstance::new(rect, icon.color, icon_radius(), 1.0)
                    };
                    // Voir `external_icon_instance` : le balancement s'applique à
                    // la source, donc à toutes les familles d'icônes.
                    match bascule {
                        Some((_, angle)) => instance.bascule = angle,
                        None => instance.rotation = hover_wobble_angle(scale),
                    }
                    Some(instance)
                };
            // ⚠️ **`if let` ET NON UN `push` DIRECT** : un espace ÉLASTIQUE ne
            // dessine RIEN, donc la boucle n'est plus « une instance par icône ».
            // C'est le prix — minime — d'une sorte muette.
            if let Some(instance) = instance {
                out.push((i, side, instance));
            }
            // Trait pleine largeur au SURVOL d'un espace (demande explicite) — le
            // marqueur reste au repos, le trait s'ajoute par-dessus. Un espace de
            // la barre du haut n'en reçoit jamais (voir `spacer_hover_line`).
            if icon.is_spacer {
                if let Some(line) = self.spacer_hover_line(rect, layout, side, separator_color) {
                    out.push((i, side, line));
                }
            }
        }
        out
    }

    /// Trait pleine largeur à dessiner EN PLUS du marqueur quand un espace
    /// LATÉRAL est survolé — `None` sinon (voir `spacer_hover_line_rect` pour la
    /// géométrie, et `IconDrag::resizing` pour le geste qui le redimensionne).
    ///
    /// **Partagé par les DEUX chemins de dessin des espaces** — `render_instances`
    /// (dock PRIMAIRE, `app::run`) et `spacer_instances` (écrans SECONDAIRES,
    /// `dock_instance::DockInstance::render`). Ce sont deux fonctions distinctes
    /// qui dessinent le même élément ; une règle recopiée dans l'une finirait
    /// par diverger de l'autre, et le trait n'apparaîtrait que sur un écran.
    fn spacer_hover_line(
        &self,
        rect: [f32; 4],
        layout: &DockLayout,
        side: Side,
        color: [f32; 4],
    ) -> Option<QuadInstance> {
        // `mouse_pos` est la position COURANTE du dock, remise à `None` quand la
        // souris quitte la fenêtre (voir `on_mouse_leave`) : aucun état à
        // maintenir ici.
        (side.is_lateral() && self.mouse_pos.is_some_and(|(mx, my)| hit(&rect, mx, my)))
            .then(|| spacer_line_instance(rect, layout, side, color))
    }

    /// La grandeur à ÉCRIRE EN CLAIR pour un espace — sa cote s'il déclare une
    /// zone, la taille de son blanc sinon (voir `SpacerReadout`) : l'étape
    /// « la cote au survol » du chantier des zones, DEMANDE EXPLICITE.
    ///
    /// **C'est le GESTE EN COURS qui prime sur le survol**, et c'est le point
    /// qui décide de tout : pendant un Maj + glisser, le marqueur d'un
    /// séparateur de zone reste immobile (sa position est imposée par la zone
    /// qu'il clôt, voir `Slot::zone`) alors que la souris s'en écarte — ne
    /// mesurer que le survol ne montrerait donc plus rien, précisément pendant
    /// le geste où ce chiffre est le SEUL retour disponible.
    ///
    /// Ne dit rien d'un simple DÉPLACEMENT (`DragAdjust::ZoneLead`, ou glisser
    /// sans Maj) : déplacer une icône n'a jamais annoncé sa position, et le
    /// vide qui précède un séparateur est une position, pas une grandeur.
    ///
    /// **Barres LATÉRALES seulement**, comme `spacer_hover_line` et pour la
    /// même raison : un espace de la barre du haut reçoit un emplacement de
    /// largeur `icon_size` et n'a aucune hauteur propre (voir
    /// `layout_top_side`) — sa cote n'y produit rien, l'annoncer serait mentir.
    pub fn spacer_readout(&self, layout: &DockLayout, hwnd: HWND) -> Option<SpacerReadout> {
        // 1. Un redimensionnement en cours (voir `IconDrag::resizing`), quelle
        //    que soit la position du curseur à cet instant.
        if let Some(drag) = self.drag.as_ref().filter(|drag| drag.dragging) {
            match drag.resizing {
                Some(DragAdjust::SpacerSize) => return self.readout_of_size(drag.icon_index),
                Some(DragAdjust::ZoneCote) => return self.readout_of_cote(drag.icon_index),
                // La cote de la zone du DESSUS, pas celle de l'élément tiré :
                // c'est elle qui bouge (voir `DragAdjust::PreviousZoneCote`).
                Some(DragAdjust::PreviousZoneCote(previous)) => return self.readout_of_cote(previous),
                Some(DragAdjust::ZoneLead) | None => {}
            }
        }
        // 2. Sinon, l'espace SOUS LE CURSEUR — `icon_at` les rend comme
        //    n'importe quelle icône (c'est `hovered_icon`, côté `app::run`, qui
        //    les écarte des infobulles d'icônes).
        let (mx, my) = self.mouse_pos?;
        let index = self.icon_at(mx, my, layout, hwnd)?;
        if !self.icon_side(index).is_some_and(Side::is_lateral) {
            return None;
        }
        if self.is_zone_separator(index) {
            self.readout_of_cote(index)
        } else {
            self.readout_of_size(index)
        }
    }

    /// La cote de `index`, ou `None` si ce n'est pas un espace — voir
    /// `spacer_readout`. **Ne demande PAS que la cote soit `> 0`** : un Maj +
    /// glisser peut l'amener à `0.0` (retour à l'espace ordinaire, permis et
    /// documenté dans `update_drag`), et le chiffre doit alors suivre jusqu'à
    /// zéro au lieu de disparaître en cours de geste.
    fn readout_of_cote(&self, index: usize) -> Option<SpacerReadout> {
        Some(SpacerReadout { index, is_cote: true, value: self.zone_height(index)? })
    }

    /// La taille de blanc de `index`, ou `None` si ce n'est pas un espace.
    fn readout_of_size(&self, index: usize) -> Option<SpacerReadout> {
        Some(SpacerReadout { index, is_cote: false, value: self.spacer_size(index)? })
    }

    /// Comme `render_instances`, mais RÉDUIT aux seuls espaces invisibles
    /// (voir `Icon::is_spacer`) — pour un appelant (voir
    /// `dock_instance::DockInstance::render`, barres SECONDAIRES) qui
    /// construit déjà lui-même chaque icône nommée individuellement plutôt
    /// que par un unique passage générique comme `app::run` : réutiliser
    /// `render_instances` tel quel y dessinerait aussi le logo/les icônes
    /// épinglées/le presse-papiers une SECONDE fois, sans leur texture (ces
    /// icônes-là n'ont `external_texture: false` que parce qu'elles portent
    /// une texture chargée à part, pas parce qu'elles n'en ont pas). BUG
    /// RÉEL CONSTATÉ : les barres secondaires ne dessinaient JUSQU'ICI aucun
    /// indicateur pour leurs espaces invisibles.
    /// Comme `render_instances`, **chaque instance est accompagnée de sa
    /// BARRE** — même raison : l'appelant doit pouvoir découper les barres
    /// latérales (voir `lateral_clip`) et pas celle du haut — **et de son
    /// INDEX**, même forme et même raison que `render_instances` depuis les
    /// zones : un marqueur qui appartient au contenu d'une zone doit être
    /// rogné à sa boîte (voir `zone_clips`), ce qui se résout par l'index.
    pub fn spacer_instances(
        &self,
        layout: &DockLayout,
        separator_color: [f32; 4],
        // La couleur du TRAIT, distincte de celle du marqueur — voir
        // `render_instances`, qui porte la raison.
        separator_line_color: [f32; 4],
        separator_size: f32,
    ) -> Vec<(usize, Side, QuadInstance)> {
        let rects = self.icon_rects_map(layout);
        let mut out = Vec::new();
        for (i, icon) in self.icons.iter().enumerate() {
            if !icon.is_spacer {
                continue;
            }
            let Some(rect) = rects.get(i).copied().flatten() else { continue };
            let Some(side) = self.icon_side(i) else { continue };
            // **LA MÊME RÈGLE QUE `render_instances`, ET C'EST LE POINT** : deux
            // fonctions dessinent les espaces — le dock PRIMAIRE et les écrans
            // SECONDAIRES — et une règle recopiée dans l'une finirait par diverger
            // de l'autre. Une sorte qui ne s'appliquerait que sur un écran serait
            // exactement ce défaut.
            match icon.sorte {
                SorteEspace::Rond => {
                    out.push((i, side, spacer_marker_instance(rect, self.hover.scale(i), separator_color, separator_size)))
                }
                SorteEspace::Trait => out.push((i, side, spacer_line_instance(rect, layout, side, separator_line_color))),
                // Muet, comme sur le dock primaire.
                SorteEspace::Elastique => {}
            }
            // Même trait que le dock primaire, par la MÊME méthode (voir
            // `spacer_hover_line`) : sur un écran secondaire il ne se déclenche
            // jamais, ces docks n'ayant pas de barres latérales (voir
            // `SecondaryWindow`) — mais la règle reste écrite à un seul endroit.
            if let Some(line) = self.spacer_hover_line(rect, layout, side, separator_color) {
                out.push((i, side, line));
            }
        }
        out
    }

    /// Rectangles COURANTS de toutes les icônes, indexés comme `icons` (voir
    /// `icon_rect`/`icon_at`/`render_instances`) — `None` seulement pour
    /// l'icône ACTUELLEMENT glissée (voir `drag_preview`, qui la positionne
    /// séparément). Sans glisser en cours, chaque barre est simplement
    /// empilée dans son propre ordre (voir `layout_side`) ; PENDANT un
    /// glisser (voir `IconDrag::dragging`), la barre CIBLE (voir
    /// `drag_target_side`) est recalculée avec l'icône glissée retirée puis
    /// réinsérée à l'index prévisualisé (voir `drag_insert_index`) — les
    /// AUTRES icônes de cette barre se décalent ainsi en direct pour ouvrir/
    /// refermer sa place, demande explicite : "les autres se décalent en
    /// direct".
    /// Les QUATRE zones (voir `Side`), dans l'ordre où `icon_rects_map`/
    /// `zone_at` les considèrent — un tableau plutôt que répété à chaque
    /// appel.
    /// **LES BARRES QUI PORTENT DES ICÔNES, ET L'ORDRE DANS LEQUEL ELLES SE DISPOSENT.**
    ///
    /// ⛔ **`TopMiddle` Y FIGURE, ET SON ABSENCE ÉTAIT UN DÉFAUT MUET** (mesuré le
    /// 2026-10-30 par l'utilisateur : « en essayant de déplacer l'applet réseau pour tester
    /// elle a disparu »). La variante existait depuis le 2026-10-29, `layout_zone` savait la
    /// disposer, `icones_milieu` savait l'écrire — mais **cette liste ne la contenait pas**,
    /// donc aucun de ses emplacements n'était jamais calculé : une icône déposée au milieu
    /// était PERSISTÉE et JAMAIS DESSINÉE. Elle existait dans le fichier et pas à l'écran,
    /// ce qui est la pire façon de perdre quelque chose.
    ///
    /// ⚠️ **`Bottom` RESTE EN FIN** : c'est elle qui dispose la barre du bas sur ses
    /// rectangles, et l'ordre de cette liste décide qui gagne quand deux barres citent le
    /// même index (voir `journaliser_les_espaces`, champ `aussi`).
    const ZONES: [Side; 6] = [Side::Left, Side::Right, Side::TopLeft, Side::TopRight, Side::TopMiddle, Side::Bottom];

    /// Dispatch vers `layout_side` (Left/Right, empilage VERTICAL) ou
    /// `layout_top_side` (TopLeft/TopRight, empilage HORIZONTAL depuis
    /// `TOP_ZONE_CORNER_MARGIN`) selon `side` — voir la doc de `Side`.
    fn layout_zone(&self, side: Side, order: &[usize], layout: &DockLayout, map: &mut [Option<[f32; 4]>]) {
        match side {
            Side::Left | Side::Right => self.layout_side(side, order, layout, map),
            Side::TopLeft => {
                self.layout_top_side(order, layout, TOP_ZONE_CORNER_MARGIN, true, map);
            }
            Side::TopRight => {
                self.layout_top_side(order, layout, layout.total_width - TOP_ZONE_CORNER_MARGIN, false, map);
            }
            // ⭐ LE MILIEU — ancré au CENTRE : on mesure d'abord la largeur de la
            // rangée (chaque case vaut `top_icon_size_for`, plus l'écart de la
            // barre), puis on part de `(total - largeur) / 2`. ⚠️ Ce n'est PAS
            // l'ancre d'un bord : une rangée vide reste au centre, pas collée au
            // début.
            Side::TopMiddle => {
                let taille = top_icon_size_for(layout.top_thickness);
                let gap = self.icon_gap_side(Side::TopMiddle);
                let n = order.len() as f32;
                let largeur = if n > 0.0 { n * taille + (n - 1.0) * gap } else { 0.0 };
                if self.clock_width > 0.0 {
                    // ⭐ **À GAUCHE DE L'HORLOGE** — le milieu EST l'horloge et les applets
                    // qui l'entourent (décision de l'utilisateur, 2026-10-30). La rangée est
                    // donc posée par son bord DROIT (`grow_right = false`), qui bute sur ce
                    // qui précède déjà l'horloge : l'icône de NOTE (voir
                    // `topbar::note_rect`, qui la pose à `cx - NOTE_MARGIN_RIGHT - taille`).
                    //
                    // ⚠️ **LA PLACE DE LA NOTE EST RÉSERVÉE MÊME QUAND ELLE EST CACHÉE** :
                    // le dock ne connaît pas les fonctionnalités, et un bouton désactivé qui
                    // laisse un petit blanc vaut mieux que deux icônes l'une sur l'autre.
                    let cx = (layout.total_width - self.clock_width) * 0.5;
                    let bord_droit = cx - crate::ui::topbar::NOTE_MARGIN_RIGHT - taille - gap;
                    self.layout_top_side(order, layout, bord_droit, false, map);
                } else {
                    self.layout_top_side(order, layout, (layout.total_width - largeur) * 0.5, true, map);
                }
            }
            Side::Bottom => {
                // ⚠️ **L'ANCRE PART DE LA BARRE DE GAUCHE, PAS DU BORD DE
                // L'ÉCRAN** : la barre du bas occupe le MILIEU (voir `bar_rects`),
                // donc son premier emplacement se pose après la colonne latérale.
                // L'ancrer à `TOP_ZONE_CORNER_MARGIN` — ce que je faisais — posait
                // le raccourci DANS la barre de gauche, ce que l'utilisateur a vu
                // tout de suite : « le dossier est décalé à gauche sur la barre de
                // gauche ».
                self.layout_bottom_side(
                    order,
                    layout,
                    layout.left_width + TOP_ZONE_CORNER_MARGIN,
                    bottom_band_width(layout),
                    map,
                );
            }
        }
    }

    fn icon_rects_map(&self, layout: &DockLayout) -> Vec<Option<[f32; 4]>> {
        let mut map = vec![None; self.icons.len()];
        match &self.drag {
            // `!drag.resizing` : pendant un redimensionnement, AUCUNE place n'est
            // ouverte — l'ordre est celui du dessin normal, l'espace grandit sur
            // place et les icônes suivantes descendent d'elles-mêmes (c'est le
            // calcul de `bar_layout`, pas un aperçu).
            // ⛔ **ET UN GESTE REFUSÉ N'OUVRE AUCUNE PLACE — `deplacement_autorise` EST
            // CONSULTÉE ICI AUSSI.** `drag_preview` ne cache que l'icône FLOTTANTE ;
            // or c'est CE tableau qui décide où chaque icône se dessine (voir
            // `presented_rect`, « la SEULE source de vérité de ce qui est à l'écran »,
            // et `icon_at`). Sans cette garde, un glisser SIMPLE dans la barre du bas
            // — que `end_drag` refuse — écartait quand même toutes les icônes pour
            // laisser la place au dépôt : le geste PROMETTAIT encore, une ligne plus
            // loin que là où la garde avait été posée. En la refusant, on retombe sur
            // le `_` ci-dessous, qui dispose chaque barre à sa place normale.
            Some(drag) if drag.dragging && drag.resizing.is_none() && self.deplacement_autorise(drag) => {
                let target_side = self.drag_target_side(drag, layout);
                for side in Self::ZONES {
                    if side == target_side {
                        // LE BLOC, ici aussi (voir `bloc_deplace`) : la barre se dispose SANS lui, et
                        // l'aperçu le réinsère d'un seul tenant. C'est ce qui MONTRE la zone voyager
                        // pendant le geste — ses icônes se dessinent à leur nouvelle place, et seul
                        // le marqueur flotte (voir `map[drag.icon_index] = None` juste après).
                        let bloc = self.bloc_deplace(drag);
                        let others: Vec<usize> =
                            self.order(side).iter().copied().filter(|index| !bloc.contains(index)).collect();
                        match self.drag_insert_index(&others, drag, side, layout) {
                            Some(insert_at) => {
                                let at = insert_at.min(others.len());
                                let mut preview = others;
                                for (offset, &icon) in bloc.iter().enumerate() {
                                    preview.insert(at + offset, icon);
                                }
                                self.layout_zone(side, &preview, layout, &mut map);
                            }
                            // LÂCHER REFUSÉ (voir `DragOutcome::Refused`) :
                            // AUCUNE place n'est ouverte, la barre se dispose
                            // telle quelle. C'est le seul signe du refus PENDANT
                            // le geste — les autres icônes ne s'écartent pas,
                            // là où elles le font dès que le lâcher est accepté.
                            None => self.layout_zone(side, self.order(side), layout, &mut map),
                        }
                        map[drag.icon_index] = None;
                    } else {
                        self.layout_zone(side, self.order(side), layout, &mut map);
                    }
                }
            }
            _ => {
                for side in Self::ZONES {
                    self.layout_zone(side, self.order(side), layout, &mut map);
                }
            }
        }
        map
    }

    /// Disposition COMPLÈTE d'une barre latérale : l'emplacement de chaque
    /// élément de `order`, empilés de haut en bas depuis `start_y`, ET les
    /// boîtes des zones déclarées par les séparateurs de zone (voir
    /// `Icon::zone_height`). PURE — ni GPU ni fenêtre — donc vérifiable par
    /// test, et PARTAGÉE par ses trois consommateurs : `layout_side`
    /// (positionnement réel), `reclamp_scroll` (hauteur de contenu à faire
    /// défiler) et `drag_insert_index` (aperçu de l'index d'insertion
    /// pendant un glisser) — sans ce partage, les trois calculs auraient pu
    /// diverger dès qu'un espace se mêle à des icônes normales dans la même
    /// barre.
    ///
    /// Un espace invisible (voir `Icon::is_spacer`) occupe une hauteur FIXE
    /// (`SPACER_HEIGHT`, voir sa doc), PAS `icon_size` entier comme une icône
    /// normale, ET l'espacement de chaque côté d'un espace utilise
    /// `SPACER_GAP` (voir sa doc) plutôt que `self.icon_gap` — BUG RÉEL
    /// SIGNALÉ EN PRATIQUE ("le séparateur ne fait toujours pas un tiers de
    /// la hauteur d'une icône") avec un `icon_gap` réglé par l'utilisateur à
    /// 40px (bar_thicknesses 80px, icon_size 56px) : réduire la hauteur
    /// PROPRE de l'espace ne servait à rien tant que l'espacement CONSTANT
    /// autour de lui (40px des deux côtés, identique à celui d'une icône
    /// normale) écrasait entièrement cette réduction dans l'empreinte
    /// visuelle TOTALE.
    ///
    /// **Le contrat des zones** : à la rencontre d'un séparateur de zone, la
    /// fin de la zone en cours est IMPOSÉE à `top + cote` (voir
    /// `Icon::zone_height`) — le séparateur lui-même se pose là, et tout ce
    /// qui suit repart de là. Zone plus courte que sa cote : du vide sous son
    /// contenu. Plus longue : son contenu déborde de la boîte, quitte à y
    /// être rogné et à y défiler (`zone_scroll`). C'est ce qui rend les
    /// tailles de zones INDÉPENDANTES de leur contenu — DEMANDE EXPLICITE :
    /// « les séparateurs de zone de la barre de droite semblent être liés, je
    /// ne peux pas définir des tailles de zones librement ».
    ///
    /// **Tant qu'aucun séparateur de zone n'existe** — tous les `zone_height`
    /// à `0.0`, le cas de tous ceux d'aujourd'hui — `zones` est VIDE et les
    /// emplacements sont exactement ceux de l'ancien `slot_positions` : toute
    /// la non-régression du reste tient à cette seule propriété.
    ///
    /// `zone_scroll` : défilement PROPRE à une zone, reçu par l'index de son
    /// SÉPARATEUR — pas par le rang de la zone dans `zones`, qui se décale dès
    /// qu'un séparateur est ajouté ou retiré ailleurs dans la barre, alors que
    /// l'index d'un espace, lui, ne change jamais (même choix de clé que
    /// `zone_heights.txt` et `Dock::zone_scroll`).
    ///
    /// Il ne déplace que le CONTENU d'une zone — jamais sa boîte, ni le
    /// séparateur qui la clôt, dont la position est précisément ce que la zone
    /// garantit. Le défilement de BARRE, lui, est déjà retranché dans `start_y`
    /// par l'appelant, et déplace tout, boîtes comprises.
    /// `bar_width` : largeur de la barre. Elle ne sert QU'À la grille des zones
    /// (voir `Icon::zone_columns`) — c'est elle qui décide combien de colonnes
    /// une zone reçoit réellement, donc la taille d'une case.
    fn bar_layout(
        &self,
        side: Side,
        order: &[usize],
        icon_size: f32,
        start_y: f32,
        zone_scroll: &dyn Fn(usize) -> f32,
        zone_grid: &dyn Fn(usize) -> (u16, f32),
    ) -> BarLayout {
        let mut slots = Vec::with_capacity(order.len());
        let mut zones: Vec<ZoneBox> = Vec::new();
        // Zone en cours : `None` tant qu'aucun séparateur de zone n'a été
        // rencontré — ce qui la précède n'appartient à aucune zone et
        // s'empile depuis `start_y`, exactement comme avant les zones.
        let mut current: Option<usize> = None;
        // Bas du dernier élément reçu par la zone en cours — avec son `top`
        // (voir `ZoneBox`), de quoi mesurer son `content_height` quand elle
        // se clôt.
        let mut zone_used = start_y;
        // Curseur d'empilement, en coordonnées NON défilées par zone : le
        // défilement d'une zone n'est retranché qu'en posant l'emplacement,
        // pour que la mesure de son contenu reste stable (sinon la borne du
        // défilement se déplacerait avec le défilement lui-même).
        let mut y = start_y;

        // --- GRILLE DE LA ZONE EN COURS (voir `Icon::zone_columns`) --------
        //
        // `columns = 1` HORS ZONE COMME DANS UNE ZONE QUI N'EN DÉCLARE QU'UNE,
        // et c'est la propriété qui compte : à une colonne, chaque élément
        // termine sa propre rangée, `row_top` vaut `y` à chaque tour, et tout
        // le calcul ci-dessous se réduit EXACTEMENT à l'empilement d'origine.
        // Aucune zone existante ne peut donc changer d'aspect — c'est la même
        // garantie que `zone_height = 0.0` donne aux espaces ordinaires.
        let mut columns: u16 = DEFAULT_ZONE_COLUMNS;
        // Taille d'une CASE : l'icône rétrécit pour ne jamais dépasser sa
        // colonne (voir `zone_cell_size`) — à une colonne, c'est `icon_size`.
        let mut cell = icon_size;
        let mut col: usize = 0;
        let mut row_top = start_y;

        for (i, &icon_index) in order.iter().enumerate() {
            let is_spacer = self.is_spacer(icon_index);
            // Cote DÉCLARÉE par cet élément (voir `Icon::zone_height`) :
            // toujours `0.0` pour une icône, un espace ordinaire, ou un
            // séparateur de zone qui n'en déclare pas encore.
            //
            // LUE AVANT LA HAUTEUR, parce qu'elle la décide : c'est elle qui
            // fait d'un espace un SÉPARATEUR DE ZONE, et un séparateur n'a pas
            // la hauteur d'un espace ordinaire (voir `height` ci-dessous).
            let declared = if is_spacer { self.zone_height(icon_index).unwrap_or(0.0) } else { 0.0 };
            // `spacer_size(icon_index)` (voir sa doc, demande explicite :
            // "choisir sa taille depuis un curseur") — PAS `SPACER_HEIGHT`
            // en dur : chaque espace ORDINAIRE garde SA PROPRE hauteur, réglée
            // individuellement. Dans une grille, un espace occupe une RANGÉE
            // ENTIÈRE (voir plus bas) : sa hauteur est donc toujours la sienne,
            // jamais celle d'une case.
            //
            // **UN SÉPARATEUR DE ZONE FAIT EXCEPTION, et c'est le fond du
            // correctif** : son emplacement vaut la taille de son MARQUEUR
            // (`SPACER_HEIGHT`, le plafond de `MAX_SEPARATOR_SIZE`, dans lequel
            // le cercle tient donc toujours), jamais son blanc. Sans cela le
            // blanc — un réglage qui, sur un séparateur, ne veut rien dire —
            // repoussait la ZONE, et avec elle le nom qu'elle porte, à l'insu
            // de qui venait de régler tout autre chose (DÉFAUT RÉEL SIGNALÉ :
            // « les textes ne sont pas placés en haut des zones », mesuré à
            // 227 px sous le marqueur pour un blanc de 221). L'espacement
            // AVANT une zone garde son propre réglage, le VIDE
            // (`Icon::zone_lead`, glisser simple sur le séparateur), qui ne
            // touche que lui.
            let height = if is_spacer {
                if declared > 0.0 {
                    SPACER_HEIGHT
                } else {
                    self.spacer_size(icon_index).unwrap_or(SPACER_HEIGHT)
                }
            } else {
                cell
            };

            if declared > 0.0 {
                // Ce séparateur OUVRE une zone : il clôt d'abord la
                // précédente, dont le bas IMPOSÉ remplace le curseur — c'est
                // tout l'objet des zones.
                if let Some(open) = current {
                    zones[open].content_height = (zone_used - zones[open].top).max(0.0);
                    y = zones[open].bottom;
                }
                // ...précédé du VIDE que le glisser lui a donné, s'il y en a un
                // (voir `Icon::zone_lead`) : c'est ce qui fait suivre le
                // marqueur au curseur au lieu de le laisser collé au bas de la
                // zone du dessus.
                y += self.zone_lead(icon_index).unwrap_or(0.0);
                // Le séparateur occupe ensuite un emplacement ordinaire, à sa
                // taille de MARQUEUR (voir `height`, résolue plus haut) — qui
                // n'est PAS sa cote : un séparateur de zone reste fin, c'est sa
                // zone qui est grande. Les deux espaces qui l'entourent gardent
                // `SPACER_GAP` (`is_spacer` est vrai pour lui, comme dans
                // l'ancien calcul).
                slots.push(Slot { top: y, height, column: 0, zone: None });
                y += height + SPACER_GAP;
                // ...et la zone s'ouvre juste sous lui, vide pour l'instant —
                // donc à `SPACER_GAP` du bas de son cercle : c'est ce qui pose
                // le CONTENU de la zone (sa première icône) juste sous le
                // marqueur, quel que soit le blanc que celui-ci a gardé.
                //
                // ⚠️ **CE N'EST PLUS CE QUI POSE LE NOM DE LA ZONE**, et la
                // distinction est exactement l'erreur commise en écrivant ce
                // placement : l'étiquette est désormais centrée SUR l'emplacement
                // du séparateur (voir `zone_label_origin`), donc AU-DESSUS de
                // cette boîte-ci. Prendre `zone.top` pour la position du
                // séparateur laisse le texte 24 px trop bas, sous la pastille.
                zones.push(ZoneBox { sep_index: icon_index, top: y, bottom: y + declared, content_height: 0.0 });
                // LA GRILLE DE CETTE ZONE, résolue UNE SEULE FOIS ICI : la
                // demande de l'utilisateur (voir `Icon::zone_columns`), ramenée
                // à ce que la largeur de la barre accorde (voir
                // `effective_zone_columns`). La résoudre par élément ferait
                // relire la même chose à chaque icône.
                (columns, cell) = zone_grid(icon_index);
                col = 0;
                row_top = y;
                zone_used = y;
                current = Some(zones.len() - 1);
                continue;
            }

            // UN ESPACE ORDINAIRE DANS UNE GRILLE INTERROMPT LA RANGÉE : sa
            // raison d'être est un VIDE vertical de SA taille (voir
            // `spacer_size`, réglable au curseur), qu'une case de grille ne
            // saurait pas exprimer — il prend donc toute la largeur, et la
            // rangée en cours se ferme avant lui.
            if columns > 1 && is_spacer && col > 0 {
                col = 0;
                row_top += cell + self.icon_gap_side(side);
                y = row_top;
            }

            slots.push(Slot {
                top: match current {
                    // L'index du SÉPARATEUR, pas le rang de la zone : voir
                    // `zone_scroll` (le rang se décale, l'index non). Le
                    // défilement retranche la RANGÉE entière, pas l'élément :
                    // une grille qui défilerait de travers ne se lirait pas.
                    Some(zone) => row_top - zone_scroll(zones[zone].sep_index),
                    None => y,
                },
                height,
                // La colonne de l'élément DANS SA RANGÉE — `0` hors grille, où
                // la barre n'a qu'une colonne (voir `layout_side`, qui la tourne
                // en abscisse).
                column: if columns > 1 { col } else { 0 },
                zone: current,
            });
            zone_used = row_top + height;
            let next_is_spacer = order.get(i + 1).is_some_and(|&next| self.is_spacer(next));
            let gap = if is_spacer || next_is_spacer { SPACER_GAP } else { self.icon_gap_side(side) };
            // LA RANGÉE N'AVANCE QUE LORSQU'ELLE EST PLEINE — et un espace la
            // remplit à lui seul. À une colonne, `col` atteint `columns` à
            // chaque tour : l'avance est celle de l'empilement d'origine.
            col += 1;
            if col as u16 >= columns || is_spacer {
                col = 0;
                row_top += height + gap;
                y = row_top;
            }
        }
        // La dernière zone n'a pas de clôture (rien ne la suit) : sa mesure
        // se prend ici.
        if let Some(open) = current {
            zones[open].content_height = (zone_used - zones[open].top).max(0.0);
        }
        BarLayout { slots, zones }
    }

    /// Taille d'une CASE dans une zone à `columns` colonnes, pour une barre
    /// large de `bar_width` : l'icône y **rétrécit pour tenir** (DEMANDE
    /// EXPLICITE), sans jamais dépasser la colonne — à 4 colonnes dans 140 px,
    /// elles font ~35 px au lieu de ~56.
    ///
    /// **Fonction PURE, et la seule définition de cette règle** : la disposition
    /// (voir `bar_layout`) et l'interface (voir `win32::cef_spacer_size`, qui
    /// annonce la taille obtenue) l'appellent toutes les deux, donc le réglage
    /// ne peut pas promettre une taille que le dessin refuserait.
    fn zone_cell_size(icon_size: f32, bar_width: f32, columns: u16) -> f32 {
        icon_size.min(bar_width / columns.max(1) as f32)
    }

    /// **Marge laissée DANS LA CASE d'une icône de zone**, en pixels — DEMANDE
    /// EXPLICITE : « les icônes dans les grilles des zones de droite aient une
    /// marge de 4 pixels dans leur case », puis « on pourrait essayer 8 pixels ou
    /// mieux que ce soit paramétrable ». C'est **`zone_cell_margin()`** qui fait
    /// foi (réglable, voir `set_zone_cell_margin`) ; la valeur ci-dessus n'est
    /// que son défaut.
    ///
    /// Sans marge, deux icônes voisines d'une grille se TOUCHENT : à 3 colonnes
    /// dans une barre de 80 px, les cases font 27 px et l'aplat de l'une vient
    /// border celui de la suivante, si bien que la grille se lit comme une seule
    /// masse au lieu d'une rangée d'icônes.
    ///
    /// **ELLE S'APPLIQUE AUSSI AU CLIC, ET C'EST VOULU** : le rectangle d'une
    /// icône est ici ce qui est DESSINÉ **et** ce qui est cliquable (voir
    /// `icon_at`, `presented_rect`) — laisser une zone cliquable là où rien n'est
    /// peint serait exactement le défaut que ce dépôt corrige partout ailleurs.
    pub const DEFAULT_ZONE_CELL_MARGIN: f32 = 8.0;

    /// Remplit `map[icon_index] = Some(rect)` pour chaque icône de `order`,
    /// empilées de haut en bas dans la barre `side` (voir `bar_layout`
    /// pour la hauteur RÉELLE de chaque emplacement) — factorise ce
    /// qu'étaient `icon_rects` (barre de gauche) ET `settings_icon_rect`
    /// (barre de droite, un seul élément) avant ce système : les deux
    /// n'étaient qu'un cas particulier ("une seule barre", "un seul
    /// élément") de la même disposition.
    /// **LES ICÔNES VISIBLES D'UN ORDRE — en écartant celles qui n'existent plus.**
    ///
    /// ⚠️ **UN INDEX HORS BORNES DANS UN ORDRE EST UN DÉFAUT AILLEURS, MAIS IL NE
    /// DOIT PAS FAIRE TOMBER L'APPLICATION.** MESURÉ le 2026-09-22 : `index out of
    /// bounds: the len is 173 but the index is 174` — **plantage entier** du dock,
    /// depuis trois fonctions différentes, parce qu'un ordre gardait l'index d'une
    /// icône disparue. La cause est corrigée à la source (`set_dynamic_apps` purge
    /// désormais les QUATRE ordres, comme `set_tray_count` le faisait déjà pour sa
    /// propre plage) ; cette garde-ci est la ceinture **et** les bretelles — et
    /// **elle DIT ce qu'elle écarte**, ce qui est exactement ce qui a permis de
    /// nommer la barre et la valeur.
    ///
    /// ⚠️ **UNE SEULE FOIS** (comme `tray_icons::warn_once`) : ces fonctions
    /// tournent à CHAQUE FRAME, un défaut persistant noierait le journal.
    fn ordre_visible(&self, order: &[usize], quoi: &str) -> Vec<usize> {
        static HORS_BORNES: std::sync::Once = std::sync::Once::new();
        let mut out = Vec::with_capacity(order.len());
        // **UNE ZONE CACHÉE EMPORTE TOUTE SA PLAGE — SON SÉPARATEUR ET SON CONTENU.**
        //
        // DEMANDE EXPLICITE (2026-09-23) : ces zones « ne puissent pas être supprimées
        // mais seulement cachées ». Un séparateur masqué est déjà écarté par le filtre
        // ci-dessous (`Icon::hidden`) ; ce qui manquait, c'est d'emporter AUSSI ce qui
        // suit, jusqu'au prochain séparateur de zone.
        //
        // ⚠️ **C'EST ICI, ET PAS DANS `bar_layout`, QUE ÇA SE JOUE** : cette fonction
        // est ce qui RETIRE les icônes masquées de l'itération, donc `bar_layout` ne
        // voit JAMAIS le séparateur caché — un saut écrit là-bas serait mort. C'est
        // exactement l'erreur commise en premier, et le test l'a montrée.
        //
        // Le curseur d'empilement, lui, n'a rien à corriger : la zone suivante reprend
        // là où la précédente s'est arrêtée, donc sans trou.
        let mut saut = false;
        for &i in order {
            if i >= self.icons.len() {
                HORS_BORNES.call_once(|| {
                    crate::diag!(
                        "ordre {quoi} : index {i} HORS BORNES (icons.len() = {}) — ecarte",
                        self.icons.len()
                    )
                });
                continue;
            }
            // Un SÉPARATEUR DE ZONE finit toujours le saut (c'est lui qui reprend le
            // fil) ; le reste du contenu d'une zone cachée est sauté.
            let est_separateur = self.icons[i].is_spacer && self.icons[i].zone_height > 0.0;
            if saut && !est_separateur {
                continue;
            }
            saut = false;
            if self.icons[i].hidden {
                // Il ouvre le saut SEULEMENT s'il ouvre une zone : un espace ordinaire
                // masqué ne cache que lui-même, comme avant.
                if est_separateur {
                    saut = true;
                }
                continue;
            }
            out.push(i);
        }
        out
    }

    fn layout_side(&self, side: Side, order: &[usize], layout: &DockLayout, map: &mut [Option<[f32; 4]>]) {
        // Uniquement Left/Right (voir la doc de `Side` — barres LATÉRALES,
        // empilage VERTICAL) : les deux zones de la barre du haut ont leur
        // propre disposition HORIZONTALE, voir `layout_top_side`, jamais
        // appelée d'ici.
        // La largeur de la barre vient de `lateral_bar_span`, **la même source
        // que le bord gauche** qu'emploie `slot_geometry` : c'est ce qui garantit
        // que le dessin et le lâcher mesurent la même grille. Le bord gauche
        // lui-même n'est pas lu ici — `slot_geometry` s'en charge, à un seul
        // endroit.
        let (_, bar_width) = Self::lateral_bar_span(side, layout);
        let icon_size = lateral_icon_size_for(side, bar_width);
        // Le défilement (voir `scroll_bar`) remonte le contenu en décalant ce
        // `start_y`, COMMUN à toutes les icônes de la barre : le hit-test et
        // le survol (`icon_at` → `icon_rects_map` → ici) suivent donc le
        // défilement sans une ligne de code supplémentaire.
        let start_y = layout.haut_colonnes + TOP_MARGIN - self.scroll[side_slot(side)];
        // Abscisse ordinaire : celle d'une icône CENTRÉE dans sa barre. C'est
        // aussi l'origine de toute case qui ne relève pas d'une grille (voir
        // `slot_geometry`).
        let x = Self::centered_icon_x(side, layout, icon_size);
        // Exclut les icônes DÉSACTIVÉES (voir `Icon::hidden`, doc détaillée)
        // de L'ITÉRATION elle-même, pas seulement du rendu — `map[i]` reste
        // `None` pour elles (comme une icône hors de `order`), et les
        // suivantes se resserrent naturellement pour combler l'espace,
        // exactement comme si elles n'avaient jamais fait partie de `order`.
        //
        // ⚠️ **UN INDEX HORS BORNES EST UN DÉFAUT AILLEURS — MAIS IL NE DOIT PAS
        // FAIRE TOMBER L'APPLICATION.** MESURÉ le 2026-09-22 : `index out of
        // bounds: the len is 174 but the index is 174`, plantage ENTIER depuis
        // cette ligne, et la valeur fautive ne figurait dans AUCUN fichier
        // enregistré (l'ordre persisté s'arrêtait à 138) — elle était donc
        // construite à l'exécution. `set_dynamic_apps` fait
        // `icons.truncate(base + count)`, et un ordre qu'il ne reconstruit PAS
        // (droite, haut) garde alors une valeur d'un cran trop loin.
        //
        // On **écarte l'index ET on le dit** : le journal nomme la barre et la
        // valeur — exactement ce qui manquait pour remonter à la cause — et
        // l'utilisateur perd une icône au lieu de son dock.
        //
        // ⚠️ **UNE SEULE FOIS** (comme `tray_icons::warn_once`) : cette fonction
        // tourne à CHAQUE FRAME, un défaut persistant noierait le journal.
        let visible_order = self.ordre_visible(order, &format!("{side:?}"));
        // Le défilement PROPRE à chaque zone, par l'index de son séparateur
        // (voir `zone_scroll`) — distinct de celui de la barre, déjà retranché
        // dans `start_y` ci-dessus : la barre déplace tout, boîtes comprises,
        // une zone ne déplace que son propre contenu.
        let bar = self.bar_layout(side, &visible_order, icon_size, start_y, &|sep| self.zone_scroll_offset(sep), &self.zone_grid(side, icon_size, bar_width));
        // ⭐ LE REPLIEMENT (voir `pliage_de_la_barre`) : la position verticale
        // vient de lui, l'abscisse reste celle de la case.
        let pliage = self.pliage_de_la_barre(side, layout);
        for (rang, (&icon_index, slot)) in visible_order.iter().zip(bar.slots.iter()).enumerate() {
            if let Some(pliage) = pliage.as_ref() {
                let place = pliage.places[rang];
                let espace = self.icons[icon_index].is_spacer;
                let columns = self.slot_columns(&bar, side, bar_width, slot);
                let (slot_x, slot_width) = Self::slot_geometry(side, layout, icon_size, x, columns, slot, espace);
                // Une icône BASCULÉE se voit raccourcie (`cos`) : c'est sa case
                // de clic et de survol. Un espace ne bascule pas, il fond.
                let hauteur = if espace { 2.0 * place.demi } else { slot.height * place.bascule.cos() };
                let top = layout.haut_colonnes + place.centre - hauteur / 2.0;
                if top >= layout.total_height || top + hauteur <= layout.haut_colonnes {
                    continue;
                }
                if let Some(cell) = map.get_mut(icon_index) {
                    *cell = Some([slot_x, top, slot_width, hauteur]);
                }
                continue;
            }
            // ÉCARTÉE si le défilement l'a ENTIÈREMENT sortie de la fenêtre.
            //
            // Le DESSIN n'a plus besoin de cette exclusion : au repos, le
            // rectangle est rendu tel quel (l'identité est le contrat de
            // `presented_rect_in` à l'échelle 1.0) et une icône sortie par le bas
            // est simplement coupée par le bord. Ce qui reste à écarter, c'est
            // tout le RESTE, qui passe par `icon_rect` : le clic, le survol,
            // l'ancrage des popups, les pastilles. Sans elle, un emplacement
            // invisible restait CLIQUABLE dans la bande qu'il occupe hors de la
            // fenêtre — et une icône sortie par le HAUT, atteignable par la
            // souris sous la barre du haut, s'y faisait magnifier, sa boîte
            // grossie ramenée à `y = 0` laissant dépasser une bande visible
            // par-dessus la barre du haut.
            //
            // `map[i]` reste `None`, exactement comme pour une icône DÉSACTIVÉE
            // ci-dessus — à une exception près, sans effet visible : `Dock::update`
            // substitue un rectangle NUL au `None` avant de nourrir
            // `HoverAnimator`, qui ignore donc l'exclusion (il faudrait que la
            // souris soit à exactement (0,0), et l'index ainsi élu n'a de toute
            // façon aucun rectangle : `icon_at` et `magnified_boxes` l'écartent).
            //
            // Seul l'emplacement ENTIÈREMENT hors champ est écarté : une icône à
            // moitié sortie reste dessinée, coupée par le bord — c'est le
            // principe même du défilement.
            if slot.top >= layout.total_height || slot.top + slot.height <= layout.haut_colonnes {
                continue;
            }
            // MÊME PRINCIPE pour la ZONE, et pour la même raison : ce que sa
            // découpe ne DESSINE pas ne doit pas être cliquable non plus (voir
            // `zone_clips`). Sans cette exclusion, un emplacement entièrement
            // sorti de sa boîte — le cas courant d'une zone plus courte que son
            // contenu — resterait atteignable par la souris dans la bande qu'il
            // occupe, alors que rien n'y est visible. Seul l'emplacement
            // ENTIÈREMENT dehors est écarté : un élément à moitié rogné reste
            // cliquable, exactement comme une icône à moitié sortie de la
            // fenêtre reste cliquable.
            if let Some(zone) = slot.zone.and_then(|rank| bar.zones.get(rank)) {
                if slot.top >= zone.bottom || slot.top + slot.height <= zone.top {
                    continue;
                }
            }
            // ABSCISSE — c'est ICI, et seulement ici, que la colonne d'une
            // grille devient une position (`bar_layout` ne connaît que
            // l'écoulement vertical, voir `Slot::column`). Le calcul lui-même
            // vit dans `slot_geometry`, **partagé avec `drag_insert_index`** :
            // un lâcher doit viser la case telle qu'elle se voit.
            let columns = self.slot_columns(&bar, side, bar_width, slot);
            let (slot_x, slot_width) =
                Self::slot_geometry(side, layout, icon_size, x, columns, slot, self.icons[icon_index].is_spacer);
            // LA MARGE DE CASE (voir `ZONE_CELL_MARGIN`) — appliquée à tout ce
            // qui VIT DANS UNE ZONE, et à rien d'autre : une icône de la barre
            // ordinaire (hors zone) garde sa taille pleine, c'est le
            // comportement d'origine de toutes les barres.
            //
            // Un ESPACE en est exclu, comme partout ailleurs dans cette
            // fonction : il occupe une rangée entière et son marqueur se centre
            // dans la barre, une marge n'y aurait aucun sens.
            let in_zone = slot.zone.is_some() && !self.icons[icon_index].is_spacer;
            let (slot_x, slot_top, slot_width, slot_height) = if in_zone {
                let margin = zone_cell_margin();
                (
                    slot_x + margin,
                    slot.top + margin,
                    // `.max(1.0)` : une case plus petite que la marge reste
                    // dessinée et cliquable plutôt que de disparaître — la
                    // géométrie ne doit jamais rendre un rectangle NÉGATIF.
                    (slot_width - margin * 2.0).max(1.0),
                    (slot.height - margin * 2.0).max(1.0),
                )
            } else {
                (slot_x, slot.top, slot_width, slot.height)
            };
            if let Some(cell) = map.get_mut(icon_index) {
                *cell = Some([slot_x, slot_top, slot_width, slot_height]);
            }
        }
    }

    /// Bande de découpe des barres LATÉRALES (`x, y, largeur, hauteur`, pixels,
    /// coordonnées fenêtre) — à passer à `QuadBatch::clipped` pour tout ce qui
    /// appartient à une barre latérale.
    ///
    /// **Pleine largeur, et c'est délibéré** : seule la VERTICALE est bornée,
    /// pour que la magnification au survol et l'aperçu de glisser continuent de
    /// déborder librement sur les côtés. Ce qui doit disparaître est uniquement
    /// ce qui REMONTE au-dessus des barres latérales — c'est-à-dire ce qui
    /// empiétait sur la barre du haut.
    ///
    /// ⛔ **LE PLAFOND N'A PAS DE JUMEAU EN BAS, ET C'EST DÉLIBÉRÉ.** J'ai cru
    /// l'inverse et j'ai borné le bas : c'était faux, parce que la barre du bas
    /// **n'occupe pas les coins** (voir `bar_rects`). Les colonnes latérales
    /// descendent donc bien jusqu'au bas de l'écran, et rogner leur contenu
    /// au-dessus de la bande ne ferait que retirer des icônes visibles.
    pub fn lateral_clip(layout: &DockLayout) -> [f32; 4] {
        [0.0, layout.haut_colonnes, layout.total_width, layout.total_height - layout.haut_colonnes]
    }

    /// Découpage PROPRE À LA ZONE de chaque icône, indexé comme `icons` : la
    /// boîte de la zone qui la contient quand elle en déclare une (voir
    /// `Icon::zone_height`), `None` partout ailleurs — là, le découpage de la
    /// seule barre (`lateral_clip`) suffit.
    ///
    /// C'est ce qui ROGNE le contenu d'une zone : un contenu plus long que sa
    /// cote déborde de la boîte, il y est coupé, et c'est ce qui rend son
    /// défilement propre (`Dock::scroll_bar`) lisible — sans cette découpe, le
    /// contenu recouvre la zone voisine, et le faire défiler le fait sortir par
    /// le haut de la sienne.
    ///
    /// **Pleine largeur, comme `lateral_clip`, et pour la même raison** : seule
    /// la VERTICALE est bornée, pour que la magnification au survol continue de
    /// déborder librement sur les côtés. Le plancher de `lateral_clip` (le bas
    /// de la barre du haut) est conservé : une zone ne fait pas remonter son
    /// contenu par-dessus la barre du haut.
    ///
    /// Calculé D'UN COUP pour toutes les icônes, et non icône par icône :
    /// `bar_layout` est déjà O(n) par barre, l'appeler pour chacune ferait un
    /// O(n²) à chaque image.
    pub fn zone_clips(&self, layout: &DockLayout) -> Vec<Option<[f32; 4]>> {
        let mut clips = vec![None; self.icons.len()];
        for side in [Side::Left, Side::Right] {
            let (bar_width, order) = match side {
                Side::Left => (layout.left_width, &self.left_order),
                Side::Right => (layout.right_width, &self.right_order),
                Side::TopLeft | Side::TopRight | Side::TopMiddle => continue,
                // La barre du BAS est horizontale : elle ne défile pas plus que
                // celle du haut, et ses découpes de zone ne se calculent pas ici.
                Side::Bottom => continue,
            };
            let visible = self.ordre_visible(order, "zone_clips");
            let icon_size = lateral_icon_size_for(side, bar_width);
            // Même disposition que celle qui sera DESSINÉE, défilements
            // compris — la découpe doit tomber sur ce que l'œil voit.
            let start_y = layout.haut_colonnes + TOP_MARGIN - self.decalage_barre(side, layout);
            let bar = self.bar_layout(side, &visible, icon_size, start_y, &|sep| self.zone_scroll_offset(sep), &self.zone_grid(side, icon_size, bar_width));
            for (&icon_index, slot) in visible.iter().zip(bar.slots.iter()) {
                let Some(zone) = slot.zone.and_then(|rank| bar.zones.get(rank)) else { continue };
                let top = zone.top.max(layout.haut_colonnes);
                if let Some(cell) = clips.get_mut(icon_index) {
                    *cell = Some([0.0, top, layout.total_width, zone.bottom.max(top) - top]);
                }
            }
        }
        clips
    }

    /// Fait défiler les icônes d'une barre LATÉRALE — DEMANDE EXPLICITE :
    /// « permettre de faire défiler les icônes avec la molette de la souris »,
    /// pour le cas où une barre contient plus d'icônes que sa hauteur ne
    /// permet d'en afficher (16 épingles de la barre des tâches Windows plus
    /// six icônes intégrées, dans une barre qui en tient une vingtaine).
    ///
    /// `delta` : valeur brute de `Event::MouseWheel`, en multiples de
    /// `WHEEL_DELTA`, positive vers le haut. Le décalage résultant est
    /// **borné des deux côtés** : jamais négatif (le premier élément ne
    /// disparaît pas vers le bas), jamais au-delà de la fin du contenu (on ne
    /// défile pas dans le vide). Sans ces bornes, quelques coups de molette
    /// suffiraient à vider la barre, et il faudrait la remettre à l'endroit
    /// à l'aveugle.
    ///
    /// **LE DÉCOUPAGE N'EST PAS GRATUIT** — c'est l'hypothèse fausse qui a
    /// produit un défaut visible (« quand je fais défiler les programmes, les
    /// icônes recouvrent la barre supérieure »). Ce qu'on croyait : la fenêtre
    /// du dock étant restreinte à l'union des trois barres
    /// (`Window::set_dock_region`), le système couperait de lui-même ce qui
    /// dépasse. Or c'est une **union** : la zone de la barre du haut en fait
    /// partie, donc une icône remontée au-dessus du bord de sa barre latérale
    /// reste DANS la région — et se dessine par-dessus la barre du haut.
    ///
    /// Le découpage est donc explicite, et inévitable : avec plus de contenu
    /// que de place, il y a toujours des icônes à moitié sorties — c'est le
    /// principe même du défilement. Voir `lateral_clip`, appliqué par
    /// l'appelant (`app::run`, `dock_instance::DockInstance::render`).
    /// `y` : ordonnée du curseur (coordonnées fenêtre) — c'est elle qui décide
    /// de ce qui défile. Une ZONE sous le curseur (voir `Icon::zone_height`)
    /// fait défiler SON contenu dans sa boîte, sans que la barre ni les zones
    /// voisines bougent ; partout ailleurs, la barre ENTIÈRE, comme avant.
    ///
    /// **LA ZONE PREND LA MOLETTE TANT QU'ELLE PEUT BOUGER, PUIS LA BARRE LA
    /// REPREND** — DEMANDE EXPLICITE : « lorsqu'une zone est masquée,
    /// l'utilisateur doit pouvoir faire défiler le contenu de la barre de droite
    /// avec la molette ». Sans ce relais, une zone qui n'a plus rien à donner
    /// (contenu arrivé à son début ou à sa fin, contenu qui tient dans sa cote,
    /// ou zone sans contenu) AVALAIT le cran de molette : la barre ne bougeait
    /// plus du tout, et les zones situées plus bas — hors de la fenêtre, donc
    /// jamais atteintes par le curseur — devenaient INACCESSIBLES. C'est le
    /// chaînage usuel des conteneurs imbriqués, et il ne coûte rien à qui
    /// défile normalement : la zone garde la molette exactement tant qu'elle
    /// répond.
    ///
    /// Le cran passe à la barre sur la zone **immobile**, pas « à la dernière
    /// zone » : c'est `scroll_one_zone` qui répond, avec la MÊME borne que
    /// `reclamp_scroll` (voir `scroll_allowances`).
    pub fn scroll_bar(&mut self, side: Side, delta: f32, y: f32, layout: &DockLayout) {
        // Barre du haut : elle s'étend en LARGEUR, son contenu ne déborde
        // jamais vers le bas — rien à faire défiler.
        if matches!(side, Side::TopLeft | Side::TopRight) {
            return;
        }
        // La vitesse est celle de CETTE barre (voir `scroll_step_px`, réglable
        // par l'utilisateur) — et le défilement D'UNE ZONE la suit, puisque
        // c'est ce même pas qui lui est appliqué juste en dessous.
        let step = delta / WHEEL_DELTA * scroll_step_px(side);
        // ⭐ REPLIÉE, la barre défile en `pli_delta` — repliée comme dépliée :
        // ce qui passe au-dessus du haut s'y replie (voir `ui::empilement::plier`).
        if let Some(pliage) = self.pliage_de_la_barre(side, layout) {
            let slot = side_slot(side);
            let min = crate::ui::empilement::delta_min(&pliage.elements, TOP_MARGIN, pliage.longueur);
            self.pli_delta[slot] = (self.pli_delta[slot] + step).clamp(min, 0.0);
            return;
        }
        let zone_took_it = match self.zone_separator_at(side, y, layout) {
            Some(sep) => self.scroll_one_zone(sep, step, side, layout),
            None => false,
        };
        if !zone_took_it {
            let slot = side_slot(side);
            self.scroll[slot] -= step;
        }
        // Les bornes sont appliquées par `reclamp_scroll`, PARTAGÉE avec
        // `set_dynamic_apps` — voir sa doc. Il borne AUSSI le défilement de
        // chaque zone de la barre, sans quoi une zone dont le contenu a
        // raccourci resterait défilée au-delà de sa nouvelle fin.
        self.reclamp_scroll(side, layout);
    }

    /// Fait défiler la zone `sep` d'un pas (`step` positif = vers le haut, comme
    /// dans `scroll_bar`) et dit si elle a RÉELLEMENT bougé — `false` quand elle
    /// est déjà à sa borne, au début du contenu comme à sa fin, ou quand son
    /// contenu tient dans sa cote. C'est ce `false` qui fait passer le cran de
    /// molette à la barre (voir `scroll_bar`).
    ///
    /// La borne vient de `scroll_allowances` : la même que celle que
    /// `reclamp_scroll` applique juste après, donc la zone ne peut pas être
    /// déclarée immobile ici et ramenée ailleurs un cran plus loin.
    fn scroll_one_zone(&mut self, sep: usize, step: f32, side: Side, layout: &DockLayout) -> bool {
        let (_, allowances) = self.scroll_allowances(side, layout);
        let max = allowances.iter().find(|&&(index, _)| index == sep).map_or(0.0, |&(_, max)| max);
        let before = self.zone_scroll_offset(sep);
        let after = (before - step).clamp(0.0, max);
        if after == before {
            return false;
        }
        self.set_zone_scroll_offset(sep, after);
        true
    }

    /// Séparateur de la zone qui CONTIENT l'ordonnée `y` dans la barre `side`
    /// — `None` hors de toute zone : au-dessus du premier séparateur, ou SUR
    /// un séparateur lui-même (il est le bord de la boîte, pas son contenu).
    ///
    /// La position testée est celle de l'ÉCRAN, défilement de barre compris
    /// (voir `layout_side`) : c'est ce que l'utilisateur voit sous son
    /// curseur. Le défilement PROPRE aux zones, lui, entre aussi dans le
    /// calcul — sans quoi une zone déjà défilée verrait ses bornes
    /// s'appliquer à un contenu qui n'y est plus.
    fn zone_separator_at(&self, side: Side, y: f32, layout: &DockLayout) -> Option<usize> {
        let (bar_width, order) = match side {
            Side::Left => (layout.left_width, &self.left_order),
            Side::Right => (layout.right_width, &self.right_order),
            Side::TopLeft | Side::TopRight | Side::TopMiddle => return None,
            Side::Bottom => return None,
        };
        let visible: Vec<usize> = order.iter().copied().filter(|&i| !self.icons[i].hidden).collect();
        let icon_size = lateral_icon_size_for(side, bar_width);
        let start_y = layout.haut_colonnes + TOP_MARGIN - self.decalage_barre(side, layout);
        let bar = self.bar_layout(side, &visible, icon_size, start_y, &|sep| self.zone_scroll_offset(sep), &self.zone_grid(side, icon_size, bar_width));
        bar.zones.iter().find(|zone| y >= zone.top && y < zone.bottom).map(|zone| zone.sep_index)
    }

    /// Borne le décalage de défilement d'une barre latérale à la hauteur RÉELLE
    /// de son contenu — le calcul décrit par `scroll_bar`, extrait ici parce
    /// qu'il doit AUSSI être rejoué quand le contenu change de hauteur sans
    /// intervention de la molette : une application qui se ferme, une icône
    /// désactivée dans les Réglages, un espace redimensionné ou une épaisseur de
    /// barre modifiée raccourcissent la barre, et un décalage resté au-delà de
    /// la nouvelle fin laissait un trou sous la dernière icône jusqu'au coup de
    /// molette suivant. Rejoué à chaque frame par `Dock::update` — donc pour le
    /// dock **PRIMAIRE** seulement (voir sa doc) : un dock d'écran secondaire
    /// garde ce défaut, son contenu pouvant changer de hauteur sans que rien ne
    /// re-borne son décalage avant le prochain cran de molette.
    ///
    /// `start_y = 0.0` : seule la HAUTEUR TOTALE du contenu nous intéresse ici,
    /// pas sa position — le décalage réel est appliqué par `layout_side`.
    fn reclamp_scroll(&mut self, side: Side, layout: &DockLayout) {
        // Barre du haut : rien à borner (son contenu ne déborde pas vers le
        // bas), et surtout `side_slot` n'a pas de case pour elle.
        if matches!(side, Side::TopLeft | Side::TopRight) {
            return;
        }
        let slot = side_slot(side);
        // ⭐ REPLIÉE, la barre ne défile pas en pixels (voir `pli_delta`) ;
        // une barre repliée n'a pas de zone.
        match self.pliage_de_la_barre(side, layout) {
            Some(pliage) => {
                self.scroll[slot] = 0.0;
                let min = crate::ui::empilement::delta_min(&pliage.elements, TOP_MARGIN, pliage.longueur);
                self.pli_delta[slot] = self.pli_delta[slot].clamp(min, 0.0);
                return;
            }
            None => {
                self.pli_delta[slot] = 0.0;
            }
        }
        let (content_height, allowances) = self.scroll_allowances(side, layout);
        let available = (layout.total_height - layout.haut_colonnes - TOP_MARGIN * 2.0).max(0.0);
        let max_offset = (content_height - available).max(0.0);
        self.scroll[slot] = self.scroll[slot].clamp(0.0, max_offset);
        // Et le défilement PROPRE à chaque zone de la barre, borné à ce que son
        // contenu a réellement en trop (voir `scroll_allowances`).
        //
        // `iter_mut` et non `set_zone_scroll_offset` : une zone qui n'a jamais
        // défilé n'a pas à recevoir d'entrée pour la seule raison qu'on la
        // borne (voir `zone_scroll`, table creuse).
        for (sep, max) in allowances {
            if let Some(entry) = self.zone_scroll.iter_mut().find(|(index, _)| *index == sep) {
                entry.1 = entry.1.clamp(0.0, max);
            }
        }
    }

    /// Réglage de repliement en vigueur pour ce dock (voir `empilement_force`).
    fn empilement(&self) -> crate::ui::empilement::Empilement {
        self.empilement_force.unwrap_or_else(crate::ui::empilement::reglages)
    }

    /// ⭐ LE REPLIEMENT de la barre latérale `side` (voir `ui::empilement`) —
    /// `None` quand il est éteint, pour une barre qui a des ZONES (elles ont
    /// leur propre défilement), ou quand TOUT tient : la somme compte chaque
    /// élément affiché de la barre — épinglées, applications ouvertes, applets,
    /// espaces.
    fn pliage_de_la_barre(&self, side: Side, layout: &DockLayout) -> Option<PliageDeBarre> {
        if !self.empilement().actif {
            return None;
        }
        let (order, slot) = match side {
            Side::Left => (&self.left_order, 0),
            Side::Right => (&self.right_order, 1),
            _ => return None,
        };
        let ordre = self.ordre_visible(order, "pli");
        let (_, bw) = Self::lateral_bar_span(side, layout);
        let cote = lateral_icon_size_for(side, bw);
        let bar = self.bar_layout(side, &ordre, cote, 0.0, &|_| 0.0, &self.zone_grid(side, cote, bw));
        if !bar.zones.is_empty() || bar.slots.len() < 2 {
            return None;
        }
        let ecart = self.icon_gap_side(side);
        let elements: Vec<crate::ui::empilement::Element> = bar
            .slots
            .iter()
            .enumerate()
            .map(|(k, slot)| {
                let place = bar.slots.get(k + 1).map_or(slot.height + ecart, |suivant| suivant.top - slot.top);
                crate::ui::empilement::Element { etendue: slot.height, place }
            })
            .collect();
        // La barre s'arrête à `TOP_MARGIN` du bas, comme elle commence à
        // `TOP_MARGIN` du haut : une icône repliée se dessine plus haute que la
        // place qu'elle occupe (sa bascule n'écrase pas jusqu'au quart), la
        // dernière déborderait sinon de l'écran.
        let longueur = layout.total_height - layout.haut_colonnes - TOP_MARGIN;
        let places = crate::ui::empilement::plier(&elements, TOP_MARGIN, longueur, longueur, self.pli_survol[slot], -self.pli_delta[slot])?;
        Some(PliageDeBarre { ordre, places, elements, longueur, haut: layout.haut_colonnes })
    }

    /// Décalage à retrancher à `haut_colonnes + TOP_MARGIN` pour obtenir le
    /// `start_y` de la barre `side` : `scroll` sans repliement ; repliée, celui
    /// de la barre DÉPLIÉE (le défilement au survol) — c'est elle que voient le
    /// glisser et les zones.
    fn decalage_barre(&self, side: Side, layout: &DockLayout) -> f32 {
        let slot = side_slot(side);
        match self.pliage_de_la_barre(side, layout) {
            Some(_) => -self.pli_delta[slot] * self.pli_survol[slot],
            None => self.scroll[slot],
        }
    }

    /// L'icône `index` BASCULÉE par le repliement : son rectangle ENTIER (avant
    /// bascule, à passer à `QuadInstance` avec `bascule`) et l'angle — `None`
    /// pour une icône droite, un espace, ou hors d'une barre repliée.
    fn bascule_de(&self, index: usize, layout: &DockLayout) -> Option<([f32; 4], f32)> {
        if !self.empilement().actif || self.icons.get(index)?.is_spacer {
            return None;
        }
        let side = self.icon_side(index).filter(|side| matches!(side, Side::Left | Side::Right))?;
        let pliage = self.pliage_de_la_barre(side, layout)?;
        let rang = pliage.ordre.iter().position(|&i| i == index)?;
        let place = pliage.places[rang];
        if place.bascule.abs() < 0.001 {
            return None;
        }
        let case = self.icon_rect(index, layout)?;
        let cote = pliage.elements[rang].etendue;
        Some(([case[0], pliage.haut + place.centre - cote / 2.0, case[2], cote], place.bascule))
    }

    /// ⭐ ORDRE DE DESSIN des icônes des barres repliées : `(index, clé)`, à
    /// dessiner par clé CROISSANTE — DÉFAUT SIGNALÉ : « en bas les icônes
    /// s'empilent à l'envers, celles qui devraient être sous les autres sont
    /// complètement visibles ». Comme `Launcher::DrawContent` de Unity, qui
    /// dessine à REBOURS : en bas, chaque icône passe sous celle du dessus. Le
    /// groupe replié du HAUT est le miroir : chaque icône sous celle du
    /// dessous, et tout ce groupe avant le reste.
    pub fn ordre_de_dessin_plie(&self, layout: &DockLayout) -> Vec<(usize, i64)> {
        let mut out = Vec::new();
        for side in [Side::Left, Side::Right] {
            let Some(pliage) = self.pliage_de_la_barre(side, layout) else { continue };
            let n = pliage.ordre.len() as i64;
            for (rang, (&index, place)) in pliage.ordre.iter().zip(&pliage.places).enumerate() {
                let rang = rang as i64;
                out.push((index, if place.en_haut { rang - 2 * n } else { -rang }));
            }
        }
        out
    }

    /// ⭐ Déplie la barre tant que la souris la survole, la replie sinon (le
    /// `hover_animation_` de Unity, linéaire) ; en entrant par sa partie
    /// repliée, la fait défiler pour garder sous la souris l'icône visée
    /// (`SetDndDelta`).
    fn animer_les_pliages(&mut self, dt: f32, layout: &DockLayout) {
        let duree = self.empilement().duree_ms;
        for side in [Side::Left, Side::Right] {
            let slot = side_slot(side);
            let Some(pliage) = self.pliage_de_la_barre(side, layout) else {
                self.pli_survol[slot] = 0.0;
                self.pli_delta[slot] = 0.0;
                continue;
            };
            let (bx, bw) = Self::lateral_bar_span(side, layout);
            let souris = self.mouse_pos.filter(|&(mx, my)| mx >= bx && mx < bx + bw && my >= layout.haut_colonnes);
            if let (Some((_, my)), true) = (souris, self.pli_survol[slot] == 0.0) {
                let y = my - layout.haut_colonnes;
                let visee = pliage.places.iter().position(|place| (y - place.centre).abs() <= place.demi.max(1.0));
                if let Some(rang) = visee.filter(|&rang| pliage.places[rang].pli > 0.0) {
                    self.pli_delta[slot] = crate::ui::empilement::delta_d_entree(&pliage.elements, TOP_MARGIN, pliage.longueur, rang, y);
                }
            }
            // Le défilement SURVIT au repli : la barre reste défilée, son haut
            // replié (voir `ui::empilement::plier`).
            self.pli_survol[slot] = crate::ui::empilement::avancer(self.pli_survol[slot], souris.is_some(), dt, duree);
        }
    }

    /// Ce que la barre `side` a en trop, et ce que CHAQUE zone de cette barre a
    /// en trop — `(fin du contenu de la barre, [(index du séparateur,
    /// dépassement)])`, en pixels.
    ///
    /// **SEULE mesure de ces deux grandeurs**, et c'est tout son intérêt : elle
    /// borne le défilement (`reclamp_scroll`) ET décide si la molette peut
    /// encore bouger une zone avant de passer à la barre (`scroll_one_zone`).
    /// Deux calculs de la même borne finiraient par s'autoriser l'un ce que
    /// l'autre interdit — le dépôt refuse déjà ce genre de doublon pour la cote
    /// d'une zone (voir `zone_cote_max`).
    ///
    /// **`zone_scroll` à ZÉRO et `start_y` à ZÉRO, et c'est indispensable** :
    /// une borne se mesure sur la disposition AU REPOS. Mesurée sur la
    /// disposition défilée, elle se déplacerait avec le défilement qu'elle
    /// borne, et le contenu ne se reposerait jamais (voir `reclamp_scroll`).
    ///
    /// `content_height` (voir `BarLayout`) plutôt que le bas du dernier
    /// emplacement : une zone au contenu plus court que sa cote laisse du VIDE
    /// sous lui, qui doit lui aussi pouvoir être amené à l'écran.
    fn scroll_allowances(&self, side: Side, layout: &DockLayout) -> (f32, Vec<(usize, f32)>) {
        let (bar_width, order) = match side {
            Side::Left => (layout.left_width, &self.left_order),
            Side::Right => (layout.right_width, &self.right_order),
            Side::TopLeft | Side::TopRight | Side::TopMiddle => return (0.0, Vec::new()),
            Side::Bottom => return (0.0, Vec::new()),
        };
        let visible: Vec<usize> = order.iter().copied().filter(|&i| !self.icons[i].hidden).collect();
        let icon_size = lateral_icon_size_for(side, bar_width);
        let bar = self.bar_layout(side, &visible, icon_size, 0.0, &|_zone| 0.0, &self.zone_grid(side, icon_size, bar_width));
        // `content_height` moins la cote (voir `ZoneBox`) : ce qu'il reste à
        // parcourir dans la zone.
        let allowances = bar
            .zones
            .iter()
            .map(|zone| (zone.sep_index, (zone.content_height - (zone.bottom - zone.top)).max(0.0)))
            .collect();
        (bar.content_height(), allowances)
    }

    /// Empile HORIZONTALEMENT les icônes de `order` à partir de `anchor_x`
    /// (comme `layout_side` les empile VERTICALEMENT dans les barres
    /// latérales) — pour les icônes du dock déposées DANS la barre du haut
    /// (voir `Side::TopLeft`/`TopRight`, demande explicite : "pouvoir
    /// déplacer les icônes des barres de gauche et de droite dans la barre
    /// supérieure comme les autres icônes"). `grow_right` : `true` pour
    /// `TopLeft` (empile vers la droite depuis `anchor_x`, le bord GAUCHE de
    /// la première icône), `false` pour `TopRight` (empile vers la gauche
    /// depuis `anchor_x`, le bord DROIT de la première icône) — les deux
    /// zones s'éloignent chacune de LEUR coin, comme les icônes "Réglages"/
    /// "Alimentation" partent chacune du bord de leur propre côté.
    ///
    /// Retourne la largeur TOTALE occupée (icônes + écarts, `0.0` si
    /// `order` ne contient, une fois les icônes désactivées exclues (voir
    /// `Icon::hidden`), aucune icône) — à l'appelant (`app::run`) de décaler
    /// d'autant les éléments FIXES de la barre du haut qui suivraient sinon
    /// cette zone (pastilles de bureaux virtuels/contrôles média à droite de
    /// `TopLeft`, groupe alimentation/son/réseau à gauche de `TopRight`) :
    /// un décalage EXACT plutôt qu'une marge fixe devinée (voir `ui::
    /// topbar::MEDIA_GROUP_MARGIN_LEFT`, qui accepte elle un chevauchement
    /// possible faute de mieux — ici la largeur réellement occupée est
    /// connue avec certitude, pas de raison de deviner).
    /// **LA LARGEUR D'UN ESPACE EST SA PROPRE TAILLE** (`Icon::spacer_size`),
    /// jamais celle d'une icône — DEMANDE EXPLICITE : « je ne peux plus régler
    /// la taille des séparateurs par le clic droit [...] dans la bande du haut ».
    /// Avant, tout élément recevait `icon_size` : le curseur de taille d'un
    /// espace de la barre du haut réglait donc une grandeur que RIEN ne lisait, et
    /// l'espace prenait la place d'une icône quoi qu'on fasse.
    ///
    /// C'est aussi l'UNIFICATION avec les barres latérales : là-bas, la taille
    /// d'un espace est sa HAUTEUR, donc son étendue le long de l'axe
    /// d'empilement ; ici, la même grandeur devient sa LARGEUR — un blanc
    /// horizontal réglable. Une seule définition de ce qu'« un espace » occupe.
    ///
    /// Conséquence sur l'empilement : les positions ne peuvent plus se déduire du
    /// RANG (`i × pas`), puisque les largeurs diffèrent — elles s'accumulent.
    ///
    /// La largeur rendue compte l'écart qui SUIT le dernier élément, comme
    /// avant (`len × pas`) : ce n'est pas un oubli, c'est ce que l'appelant
    /// attend pour décaler les éléments fixes qui viennent après.
    fn layout_top_side(&self, order: &[usize], layout: &DockLayout, anchor_x: f32, grow_right: bool, map: &mut [Option<[f32; 4]>]) -> f32 {
        let icon_size = top_icon_size_for(layout.top_thickness);
        let y = (layout.top_thickness - icon_size) * 0.5;
        let gap = self.icon_gap_side(Side::TopLeft);
        // ⚠️ **Même garde que `layout_side`** — voir sa doc : un index d'ordre
        // hors bornes ne doit pas faire tomber l'application, il doit se DIRE.
        // (Cette fonction ne reçoit pas de `Side` : la barre du haut en a deux
        // moitiés, donc le message les nomme ensemble.)
        let visible_order = self.ordre_visible(order, "barre du HAUT");
        let mut cursor = anchor_x;
        let mut occupied = 0.0;
        for &icon_index in &visible_order {
            let width = if self.icons[icon_index].is_spacer {
                self.spacer_size(icon_index).unwrap_or(SPACER_HEIGHT)
            } else {
                icon_size
            };
            // Un élément qui pousse vers la GAUCHE (`TopRight`) est posé par son
            // bord DROIT, comme avant : le curseur est alors la borne droite.
            let x = if grow_right { cursor } else { cursor - width };
            if let Some(cell) = map.get_mut(icon_index) {
                *cell = Some([x, y, width, icon_size]);
            }
            cursor = if grow_right { cursor + width + gap } else { cursor - width - gap };
            occupied += width + gap;
        }
        occupied
    }

    /// Disposition de la barre du BAS (voir `Side::Bottom`) — **horizontale comme
    /// celle du haut**, mais dans la bande du bas, et toujours posée par son bord
    /// GAUCHE puis étendue vers la droite (les raccourcis de dossiers n'ont pas
    /// d'équivalent des deux moitiés de la barre du haut : c'est UNE barre, vue
    /// d'un seul tenant, et son bouton « + » vient en fin de liste).
    ///
    /// `anchor_x` vient de `TOP_ZONE_CORNER_MARGIN`, la même marge de coin que la
    /// barre du haut : c'est le même concept, un dégagement depuis l'angle de
    /// l'écran, et lui donner une constante jumelle ne ferait que créer deux
    /// nombres à garder égaux.
    ///
    /// ⚠️ **`icon_size_for(layout.bottom_thickness)`** : la vignette d'un dossier
    /// et le nom qui l'accompagne doivent tenir dans l'épaisseur RÉGLÉE de la
    /// barre. Agrandir la barre agrandit les vignettes, comme partout ailleurs.
    fn layout_bottom_side(
        &self,
        order: &[usize],
        layout: &DockLayout,
        anchor_x: f32,
        // LA LARGEUR DE LA BANDE (voir `bottom_band_width`) — c'est ELLE que les
        // élastiques absorbent. Elle n'entrait pas dans cette fonction, alors
        // qu'elle existe déjà pour `bar_rects` : « ce qui reste » n'avait donc
        // aucune source.
        band_width: f32,
        map: &mut [Option<[f32; 4]>],
    ) -> f32 {
        let icon_size = bottom_icon_size_for(layout.bottom_thickness);
        // La bande du bas commence à `total_height - bottom_thickness` : c'est le
        // même calcul que `bar_rects`, et il doit le rester. La vignette est
        // CENTRÉE dans l'épaisseur — sans compensation : il n'y a plus de ligne de
        // nom sous elle, et la réserver la remontait de 8 px (défaut signalé :
        // « les icônes ne sont pas centrées verticalement »).
        let y = layout.total_height - layout.bottom_thickness
            + (layout.bottom_thickness - icon_size) * 0.5;
        let gap = self.icon_gap_side(Side::Bottom);
        let visible_order = self.ordre_visible(order, "barre du BAS");

        // ⚠️ **LA RANGÉE COMMENCE À `anchor_x`, PAS AU BORD DE LA BANDE** — et c'est
        // exactement `TOP_ZONE_CORNER_MARGIN` d'écart (voir `layout_zone`, qui ancre à
        // `layout.left_width + TOP_ZONE_CORNER_MARGIN`). Les ÉLASTIQUES absorbaient
        // `band_width` ENTIER : leur part valait donc `band_width - fixe`, et la rangée
        // finissait `margin` pixels PLUS LOIN que le bord droit de la bande —
        // c'est-à-dire **sur la barre de DROITE**. Défaut signalé : « les éléments vont
        // plus loin que l'extrémité de droite dans la barre du bas ».
        //
        // ⚠️ **LE CONTRÔLE DE DÉBORDEMENT NE POUVAIT PAS LE VOIR** : il comparait
        // `fixe` — qui compte les élastiques pour ZÉRO (voir la passe 1) — à
        // `band_width`, si bien qu'une rangée gonflée par eux passait toujours. Il
        // porte maintenant sur la rangée ENTIÈRE. C'est pour la même raison que le
        // premier séparateur, ÉLASTIQUE, ne montrait plus son point : sa largeur
        // n'était plus 18 px mais la moitié de la bande.
        //
        // ⚠️ **LA MÊME MARGE DES DEUX CÔTÉS — DEMANDE EXPLICITE (2026-10-01).** La
        // rangée n'avait de dégagement qu'à GAUCHE (`TOP_ZONE_CORNER_MARGIN`, par son
        // ancre) et finissait **exactement** sur le bord droit (`fin = 3740 =
        // bord_droit`, mesuré) : son seul souffle était le `gap` après le dernier
        // élément, si bien que le « + » se lisait **plaqué contre la barre**, voire
        // au-delà. « Le + va trop loin à droite, il dépasse de la barre du bas » était
        // cette asymétrie-là, pas un débordement — le contrôle ci-dessous ne la voyait
        // pas, puisqu'il n'y avait rien à déborder.
        let marge = (anchor_x - layout.left_width).max(0.0);
        let disponible = (band_width - marge * 2.0).max(0.0);

        // **PASSE 1 — CE QUE LE FIXE COÛTE.** Les ÉLASTIQUES comptent pour ZÉRO :
        // leur largeur est précisément ce qui reste, la compter serait circulaire.
        let mut fixe = 0.0;
        let mut elastiques = 0usize;
        for &icon_index in &visible_order {
            let icon = &self.icons[icon_index];
            if icon.sorte == SorteEspace::Elastique {
                elastiques += 1;
                // ⛔ **LA SÉPARATION COMPTE, ELLE, POUR TOUT LE MONDE** — et c'est
                // exactement ce qui manquait. L'élastique était sauté EN ENTIER ici,
                // donc son `gap` n'entrait pas dans `fixe`, alors que la boucle de
                // placement l'ajoute pour CHAQUE élément, élastiques compris. `part` se
                // croyait donc plus large qu'il ne pouvait l'être, et la rangée
                // dépassait `disponible` de `elastiques × gap` : **le dernier élément,
                // le « + », sortait de la barre.**
                //
                // MESURÉ le 2026-10-01 (5 élastiques, écart ≈ 9 px) : le « + » peint à
                // `x=3726` pour un bord de barre à 3740 — **20 px dehors** — et sa CASE,
                // dont il est centré, allait jusqu'à 3771.
                //
                // ⚠️ **ET LE CONTRÔLE DE DÉBORDEMENT NE POUVAIT PAS LE VOIR** : il
                // comparait `fixe + part × elastiques` à `disponible`, or `part` EST
                // `(disponible − fixe) / elastiques` — cette expression vaut donc
                // `disponible` par construction. Une tautologie ne détecte rien ; c'est
                // la sonde des quads peints qui a trouvé, pas le calcul.
                fixe += gap;
                continue;
            }
            fixe += if icon.is_spacer { self.spacer_size(icon_index).unwrap_or(SPACER_HEIGHT) } else { icon_size } + gap;
        }

        // **PASSE 2 — LE RESTE, À PARTS ÉGALES.** `.max(0.0)` : quand le fixe
        // déborde DÉJÀ, un élastique vaut ZÉRO plutôt qu'une largeur négative — un
        // rectangle négatif se dessinerait à l'envers.
        let part = if elastiques == 0 { 0.0 } else { ((disponible - fixe) / elastiques as f32).max(0.0) };

        // ⚠️ **QUAND LE FIXE DÉBORDE, RIEN NE BORNE LA RANGÉE.** La barre du bas
        // n'a AUCUN découpage (`zone_clips` et `scroll_allowances` la sautent
        // explicitement), et elle est ancrée à GAUCHE : le débordement se répand
        // donc sur la barre de DROITE, vignettes dessinées par-dessus. C'est
        // signalé UNE SEULE FOIS — un journal noyé ne se lit pas (même garde que
        // `GLYPHE_SUR_UN_ESPACE`).
        // ⚠️ **LE CONTRÔLE PORTE SUR LA RANGÉE ENTIÈRE, ÉLASTIQUES COMPRIS** (voir
        // `disponible` plus haut) : ne comparer que `fixe` laissait passer toute
        // rangée gonflée par un élastique, puisque ceux-ci y comptent pour ZÉRO.
        let occupe = fixe + part * elastiques as f32;
        // **TRACE BORNÉE : elle ne parle QUE quand le calcul change** — une fois par
        // état de la rangée, jamais par frame (voir `DERNIERE_RANGEE_DU_BAS`).
        let signature = (
            anchor_x.round() as i32,
            disponible.round() as i32,
            fixe.round() as i32,
            occupe.round() as i32,
            elastiques,
        );
        let bande = band_width.round() as i32;
        let nouvelle = DERNIERE_RANGEE_DU_BAS.with(|memoire| {
            let mut memoire = memoire.borrow_mut();
            match memoire.iter_mut().find(|(b, _)| *b == bande) {
                Some((_, derniere)) if *derniere == signature => false,
                Some((_, derniere)) => {
                    *derniere = signature;
                    true
                }
                None => {
                    memoire.push((bande, signature));
                    true
                }
            }
        });
        if nouvelle {
            let bord_droit = layout.total_width - layout.right_width;
            let fin = anchor_x + occupe;
            crate::diag!(
                "rangee du BAS : ancre={anchor_x:.0} bande={band_width:.0} disponible={disponible:.0} fixe={fixe:.0} elastiques={elastiques} part={part:.0} rangee={occupe:.0} fin={fin:.0} bord_droit={bord_droit:.0}{}",
                if fin > bord_droit + 0.5 { "   <<< DEBORDE" } else { "" }
            );
        }

        let mut cursor = anchor_x;
        let mut occupied = 0.0;
        for &icon_index in &visible_order {
            let icon = &self.icons[icon_index];
            let width = if icon.sorte == SorteEspace::Elastique {
                part
            } else if icon.is_spacer {
                self.spacer_size(icon_index).unwrap_or(SPACER_HEIGHT)
            } else {
                icon_size
            };
            if let Some(cell) = map.get_mut(icon_index) {
                *cell = Some([cursor, y, width, icon_size]);
            }
            cursor += width + gap;
            occupied += width + gap;
        }
        occupied
    }

    /// Largeur occupée par les zones `TopLeft`/`TopRight` de la barre du
    /// haut (voir `layout_top_side`) — à appeler par `app::run` AVANT de
    /// construire les rectangles des éléments FIXES de la barre du haut
    /// (pastilles de bureaux virtuels, contrôles média, groupe alimentation/
    /// son/réseau...), pour les décaler d'autant.
    pub fn top_zone_widths(&self, layout: &DockLayout) -> (f32, f32) {
        let mut map = vec![None; self.icons.len()];
        let left_width = self.layout_top_side(&self.top_left_order, layout, 0.0, true, &mut map);
        let right_width = self.layout_top_side(&self.top_right_order, layout, 0.0, false, &mut map);
        (left_width, right_width)
    }

    /// Bord GAUCHE de la zone occupée par les icônes de la moitié DROITE de
    /// la barre du haut (voir `top_zone_widths`) — que la zone soit VIDE
    /// (juste le coin, voir `TOP_ZONE_CORNER_MARGIN`) ou qu'elle contienne
    /// des icônes (alimentation/terminal/presse-papiers/son/réseau par
    /// défaut, voir `new`, plus tout ce que l'utilisateur y aura déposé) :
    /// à utiliser par `app::run`/`ui::topbar::sys_monitor_instance` pour
    /// ancrer le moniteur système juste avant cette zone, sans jamais la
    /// chevaucher — remplace l'ancien `RightGroupRects::next_left_edge`
    /// (retiré avec lui, ces cinq icônes vivant désormais dans `Dock`
    /// COMME LES AUTRES plutôt que dans une structure séparée).
    pub fn top_right_zone_left_edge(&self, layout: &DockLayout) -> f32 {
        let (_, right_width) = self.top_zone_widths(layout);
        layout.total_width - TOP_ZONE_CORNER_MARGIN - right_width
    }

    /// ⭐ **LE BORD GAUCHE DE L'ICÔNE RÉSEAU** — le débit réseau se pose contre
    /// elle (DEMANDE EXPLICITE, 2026-10-14 : « à la gauche de l'icône réseau »).
    ///
    /// ⚠️ **PAS `top_right_zone_left_edge`** : la zone peut contenir, à gauche de
    /// l'icône réseau, des cases qui occupent de la place SANS RIEN DESSINER —
    /// MESURÉ sur la configuration réelle : neuf entrées, ~450 px de vide. Ancré
    /// au bord de la zone, le débit poussait toute la moitié droite vers la
    /// gauche, jusque SOUS l'horloge et ses applets.
    ///
    /// Le texte, posé PAR-DESSUS ce vide, en rend la place réelle. Repli sur le
    /// bord de la zone quand l'icône n'est pas dans la moitié droite de la barre
    /// du haut (déplacée ailleurs, ou masquée).
    pub fn bord_gauche_reseau(&self, layout: &DockLayout) -> f32 {
        let dans_la_zone = self.top_right_order.contains(&self.index_reseau)
            && self.icons.get(self.index_reseau).is_some_and(|i| !i.hidden);
        dans_la_zone
            .then(|| self.icon_rect(self.index_reseau, layout))
            .flatten()
            .map_or_else(|| self.top_right_zone_left_edge(layout), |r| r[0])
    }
}

/// Instance de quad (couleur unie, coins droits) couvrant TOUTE la largeur
/// de la barre contenant l'icône d'un des trois "dash" `egui` (Dash,
/// réglages, visionneuse de presse-papiers — voir `config::
/// load_window_accent`, `win32::settings_dash` pour le réglage "fond
/// d'icône" par fenêtre), pas seulement un fond posé juste derrière elle
/// (essayé d'abord, une simple pastille/pilule — remplacé à la demande
/// explicite : "toute la zone correspondant à un dash dans les barres").
/// `bar_left_x`/`bar_thickness` : bord gauche et épaisseur/largeur DE LA
/// BARRE contenant cette icône (`0.0`/`layout.left_width` pour la barre de
/// gauche, `layout.total_width - layout.right_width`/`layout.right_width`
/// pour la barre de droite — voir les appelants dans `app::run`, DEPUIS que
/// les trois barres ont chacune leur propre épaisseur, ce ne sont plus la
/// même valeur). `gap` : `Dock::icon_gap()`, DOIT être la même valeur que
/// celle utilisée par `Dock::layout_side` pour que la zone de
/// chaque icône tile exactement avec ses voisines (ni chevauchement, ni
/// interstice) — à dessiner dans le même lot que les icônes pleine couleur
/// (`tex_mode` "uni"), AVANT l'icône elle-même (texturée ou glyphe) pour
/// que celle-ci reste visible par-dessus.
/// ⚠️ **`color` PORTE MAINTENANT UN ALPHA** (il était codé à `1.0` en dur) :
/// c'est ce qui permet au fond du logo de s'effacer sans que l'icône s'efface,
/// l'alpha se composant sur la barre DÉJÀ PEINTE — le fond part dans le lot
/// `plain`, dessiné avant les lots texturés (voir `app::run`). Les trois autres
/// fonds (Réglages, Presse-papiers, Notes) passent `1.0` et ne changent pas.
pub fn icon_zone_instance(icon_rect: [f32; 4], bar_left_x: f32, bar_thickness: f32, gap: f32, color: [f32; 4]) -> QuadInstance {
    let [_, y, _, h] = icon_rect;
    let zone = [bar_left_x, y - gap * 0.5, bar_thickness, h + gap];
    QuadInstance::new(zone, color, 0.0, 1.0)
}

/// Comme `icon_zone_instance`, mais pour une icône de la barre du HAUT
/// (`Side::TopLeft`/`Side::TopRight`, voir `layout_top_side` — empilage
/// HORIZONTAL, contrairement aux barres latérales que couvre
/// `icon_zone_instance`) — DEMANDE EXPLICITE ("ajoutes des possibilités de
/// réglage pour le dash notes", `notes_accent.icon_bg` dans `app::run`) :
/// l'icône "Notes" (`notes_icon_index`) est un glyphe de cette barre-là, pas
/// une icône de barre latérale. Les rôles des axes sont inversés par rapport
/// à `icon_zone_instance` : ici c'est la LARGEUR de la zone qui suit
/// `icon_rect` (son `x`/`w`, plus `gap` de chaque côté) et sa HAUTEUR qui
/// est fixe, celle de toute la barre (`bar_thickness` = `layout.
/// top_thickness`, depuis `y = 0.0`).
pub fn icon_zone_instance_top(icon_rect: [f32; 4], bar_thickness: f32, gap: f32, color: [f32; 4]) -> QuadInstance {
    let [x, _, w, _] = icon_rect;
    let zone = [x - gap * 0.5, 0.0, w + gap, bar_thickness];
    QuadInstance::new(zone, color, 0.0, 1.0)
}

/// Comme `icon_zone_instance_top`, pour une icône de la barre du BAS (voir
/// `Side::Bottom` et `layout_bottom_side` — même empilage HORIZONTAL, mais dans
/// la bande du bas).
///
/// ⚠️ **`icon_zone_instance_top` ne peut pas servir** : elle pose la bande à
/// `y = 0.0`, qui est le HAUT de la fenêtre. Les rôles des axes sont les mêmes
/// qu'elle (largeur suivant `icon_rect`, hauteur = toute l'épaisseur de la
/// barre) ; seul le bord de référence change, d'où `bar_y` en paramètre plutôt
/// qu'une constante.
pub fn icon_zone_instance_bottom(
    icon_rect: [f32; 4],
    bar_y: f32,
    bar_thickness: f32,
    gap: f32,
    color: [f32; 4],
) -> QuadInstance {
    let [x, _, w, _] = icon_rect;
    let zone = [x - gap * 0.5, bar_y, w + gap, bar_thickness];
    QuadInstance::new(zone, color, 0.0, 1.0)
}

/// Le fond d'icône d'un « dash », avec la géométrie de la barre où SON ICÔNE se
/// trouve **RÉELLEMENT** — le choix entre les deux constructeurs ci-dessus.
///
/// **C'EST LE CORRECTIF DU 2026-09-20.** Les quatre fonds étaient dessinés avec
/// une barre **supposée par fenêtre** : Dash et Presse-papiers en barre de
/// GAUCHE, Réglages en barre de DROITE, Notes en barre du HAUT. Une icône
/// DÉPLACÉE dans une autre barre gardait donc la géométrie de son ancienne — le
/// fond se posait au bon `y` mais dans la mauvaise colonne, c'est-à-dire
/// **derrière d'autres icônes**. DÉFAUT RÉEL SIGNALÉ : « les fonds d'icône des
/// dashs de la barre supérieure ne s'affichent pas derrière leurs icônes, ils
/// s'affichent derrière d'autres icônes » — exact : chez cet utilisateur, le
/// Presse-papiers et les Réglages sont dans la barre du HAUT.
///
/// La barre se lit donc sur `Dock::icon_side` : elle suit l'icône où qu'elle
/// soit. `None` (icône sans emplacement) retombe sur la barre de GAUCHE, qui est
/// ce que faisaient les deux appelants latéraux avant ce correctif.
///
/// **ELLE VIT ICI, ET PAS DANS `app::run`** : c'est de la géométrie pure, à côté
/// de ses deux jumelles — et c'est ce qui la rend testable, ce qu'un correctif
/// non testé de cette famille ne reste pas longtemps.
pub fn icon_zone_for(dock: &Dock, layout: &DockLayout, index: usize, color: [f32; 4]) -> Option<QuadInstance> {
    let rect = dock.icon_rect(index, layout)?;
    // L'écartement de SA barre (voir `Dock::icon_gap_de`) : la zone doit tuiler
    // avec ses voisines, qui sont espacées ainsi.
    let gap = dock.icon_gap_de(index);
    Some(match dock.icon_side(index) {
        Some(Side::TopLeft | Side::TopRight | Side::TopMiddle) => icon_zone_instance_top(rect, layout.top_thickness, gap, color),
        Some(Side::Bottom) => icon_zone_instance_bottom(
            rect,
            layout.total_height - layout.bottom_thickness,
            layout.bottom_thickness,
            gap,
            color,
        ),
        Some(Side::Right) => {
            icon_zone_instance(rect, layout.total_width - layout.right_width, layout.right_width, gap, color)
        }
        Some(Side::Left) | None => icon_zone_instance(rect, 0.0, layout.left_width, gap, color),
    })
}

/// Le fond ANIMÉ du logo — DEMANDE EXPLICITE (2026-09-26) : « que ça ne soit
/// que sur la zone de l'icône, pas toute la largeur de la barre ».
///
/// **Un carré un peu PLUS GRAND que l'icône, pas le carré de l'icône** : le logo
/// est opaque sur 90 % de sa surface, un fond de sa taille exacte serait caché
/// derrière. La marge vaut `gap / 2` de chaque côté — l'étendue que le fond
/// pleine largeur avait déjà dans l'axe de la barre — et le rayon grandit
/// d'autant, pour que le fond reste parallèle aux coins de l'icône (même règle
/// que `active_ring`).
///
/// Il suit le rectangle PRÉSENTÉ (magnification, balancement) : un fond qui
/// resterait au repos pendant que l'icône grossit se lirait comme un décalage.
///
/// `rond` : un DISQUE, pour l'icône circulaire — DEMANDE EXPLICITE : « activer
/// animer le fond du logo quand c'est le logo circulaire qui est choisi ». Le
/// disque est invariant par rotation, donc le logo peut tourner dessus sans
/// que le fond ait à le suivre.
pub fn icon_halo_for(dock: &Dock, layout: &DockLayout, index: usize, rond: bool, color: [f32; 4]) -> Option<QuadInstance> {
    let [x, y, w, h] = dock.presented_rect(index, layout)?;
    // L'écartement de SA barre, comme `icon_zone_for`.
    let gap = dock.icon_gap_de(index);
    let margin = gap * 0.5;
    let radius = if rond { (w.min(h) + gap) * 0.5 } else { icon_radius() + margin };
    let mut instance = QuadInstance::new([x - margin, y - margin, w + gap, h + gap], color, radius, 1.0);
    instance.rotation = hover_wobble_angle(dock.hover.scale(index));
    Some(instance)
}

/// Hauteur PAR DÉFAUT (px, PAS proportionnelle à `icon_size_for`) d'un
/// NOUVEL espace invisible à sa création (voir `Icon::spacer_size`,
/// `Dock::add_spacer_after`/`add_spacer_at_end`) — deux tentatives en
/// FRACTION de la taille d'icône (1/3 puis 1/6, voir l'historique) jugées
/// encore trop grandes à chaque fois ("la hauteur du séparateur n'a pas
/// changé" au second essai, alors que le calcul restait exact — sur un
/// écran/une taille d'icône donnés, diviser une grande valeur par 6 au lieu
/// de 3 ne fait qu'une différence de quelques pixels, difficile à
/// percevoir). Une valeur FIXE, indépendante de la taille d'icône (qui peut
/// monter jusqu'à `ICON_SIZE_MAX` = 96px), donne un point de départ
/// constamment fin, quel que soit le réglage de taille des barres — chaque
/// espace reste ENSUITE réglable individuellement (voir `Dock::
/// set_spacer_size`, demande explicite : "choisir sa taille depuis un
/// curseur"), cette constante ne sert plus qu'à la création.
pub(crate) const SPACER_HEIGHT: f32 = 18.0;

/// Cote (px) donnée à un espace quand Maj + clic droit en fait un
/// **séparateur de zone** (voir `Dock::toggle_zone_separator`) — l'équivalent
/// de `SPACER_HEIGHT` pour ce geste-là : un point de départ remaniable à la
/// souris ensuite (Maj + glisser), pas une valeur définitive.
///
/// Une valeur FIXE plutôt qu'une fraction de la barre, pour la même raison
/// que `SPACER_HEIGHT` : ce qui compte à la création est d'obtenir une zone
/// franchement distincte d'un espace ordinaire, et de la voir apparaître.
/// C'est ce qui la distingue d'un « espace de 400 px » : elle déclare une
/// BOÎTE, dont le contenu ne peut plus déborder (voir `Icon::zone_height`).
pub const DEFAULT_ZONE_HEIGHT: f32 = 400.0;

/// Cote PLANCHER qu'un glisser peut donner à la zone qu'il rétrécit (voir
/// `DragAdjust::PreviousZoneCote`) — jamais 0, qui déferait le séparateur
/// concerné (voir `Icon::zone_height`) : une structure ne doit pas s'annuler
/// sous la main qui la règle. Assez petit pour ne pas gêner un rétrécissement
/// volontaire.
const MIN_ZONE_HEIGHT: f32 = 8.0;

/// Nombre de colonnes d'une zone qui n'en déclare pas — **1, c'est-à-dire
/// l'empilement d'origine** (voir `Icon::zone_columns`). Comme pour
/// `Icon::zone_height` à `0.0`, la valeur par défaut EST le comportement
/// d'avant la fonctionnalité : aucune zone existante ne change d'aspect, et il
/// n'y a pas de chemin de code « sans grille ».
pub const DEFAULT_ZONE_COLUMNS: u16 = 1;

/// Largeur MINIMALE d'une colonne d'une zone en grille — DEMANDE EXPLICITE :
/// « de façon à ne jamais avoir moins de 32 pixels de large par colonne ».
///
/// C'est ce plancher qui PLAFONNE le nombre de colonnes qu'une barre peut
/// recevoir (voir `Dock::max_zone_columns`) : le réglage de l'utilisateur est
/// ce qu'il DEMANDE, la largeur de la barre décide de ce qu'elle en accorde.
/// Le plafond suit donc la barre — élargir celle-ci rend les colonnes qu'on
/// avait demandées, sans avoir à les redemander.
pub const MIN_ZONE_COLUMN_PX: f32 = 32.0;

/// ⭐ **UNE ÉPINGLE DISPARUE N'EST PAS UN ESPACE** — DÉFAUT SIGNALÉ (2026-10-24) :
/// la configuration initiale du portable, lancée sur une machine qui a MOINS
/// d'épingles dans sa barre des tâches, montrait « plein d'espaces de 18 px » à
/// leur place. C'est le « cas symétrique » que `ensure_spacer_capacity` documente :
/// les index `[base + nouveau, base + ancien[` restent écrits dans les ordres,
/// passent au-dessus de la fin des épingles, et `saved_spacer_capacity` les
/// matérialise en espaces. Ils sont RETIRÉS des ordres ici ; les espaces que
/// `ensure_spacer_capacity` recrée à ces index ne sont cités par aucun ordre,
/// donc inertes (voir `ensure_saved_spacers_exist`). `ancien` vient de
/// `config::load_winpin_count`.
pub fn retirer_les_epingles_disparues(ordre: &mut Vec<usize>, base: usize, ancien: usize, nouveau: usize) {
    if nouveau < ancien {
        ordre.retain(|&index| !(base + nouveau..base + ancien).contains(&index));
    }
}

/// Le DPI de l'écran où les « 32 pixels » de `MIN_ZONE_COLUMN_PX` ont été demandés
/// (3840×2160 à 150 %).
pub const DPI_DU_PLANCHER_DE_COLONNE: u32 = 144;

thread_local! {
    static PLANCHER_DE_COLONNE: Cell<f32> = const { Cell::new(MIN_ZONE_COLUMN_PX) };
}

/// ⛔ **LES PLANCHERS SONT CALIBRÉS SUR UN ÉCRAN DE 2160 DE HAUT, ET ILS NE SUIVAIENT
/// PAS LA CONVERSION.** Mesuré sur la VM (1280×648) : la barre du haut convertie à
/// 18 px d'épaisseur portait des icônes de **32** (`ICON_SIZE_MIN`, absolu) — plus
/// grandes que leur barre —, et la barre de droite convertie à 37 était ÉLARGIE à
/// **64** par le plancher de colonne. L'utilisateur l'a vu avant moi : « les barres
/// de gauche et de droite sont trop larges, les icônes de la barre du haut sont trop
/// grandes ».
///
/// ⇒ **Les deux planchers suivent désormais la HAUTEUR de l'écran**, comme toutes les
/// autres longueurs : à 2160, rien ne change (facteur 1) ; à 648, ils valent 30 %.
pub const HAUTEUR_DES_PLANCHERS: f32 = 2160.0;

thread_local! {
    static ECHELLE_DES_PLANCHERS: Cell<f32> = const { Cell::new(1.0) };
}

/// Pose l'échelle des planchers d'après la HAUTEUR de l'écran principal — voir
/// `HAUTEUR_DES_PLANCHERS`. Appelée au démarrage, et par la configuration initiale du
/// portable (qui, elle, s'exécute avant `app::run`).
pub fn poser_hauteur_des_planchers(hauteur: f32) {
    let echelle = if hauteur > 0.0 { (hauteur / HAUTEUR_DES_PLANCHERS).clamp(0.1, 4.0) } else { 1.0 };
    ECHELLE_DES_PLANCHERS.with(|c| c.set(echelle));
}

/// L'échelle courante des planchers (voir `poser_hauteur_des_planchers`).
pub fn echelle_des_planchers() -> f32 {
    ECHELLE_DES_PLANCHERS.with(Cell::get)
}

/// **LE PLANCHER D'ICÔNE, À L'ÉCHELLE DE L'ÉCRAN** — jamais moins de 8 px, seuil en
/// deçà duquel une icône n'est plus lisible du tout.
pub fn icone_minimale() -> f32 {
    (ICON_SIZE_MIN * echelle_des_planchers()).max(8.0)
}

/// Le plancher RÉEL d'une colonne, en pixels physiques — voir
/// `poser_dpi_du_plancher_de_colonne`.
pub fn plancher_de_colonne() -> f32 {
    // ⚠️ LE DPI **ET** LA HAUTEUR : le premier dit la finesse de l'écran, le second sa
    // taille. À 2160 de haut et 144 DPI, le facteur vaut 1 — rien ne change.
    PLANCHER_DE_COLONNE.with(Cell::get) * echelle_des_planchers()
}

/// ⭐ **LE PLANCHER SUIT LE DPI DE L'ÉCRAN PRINCIPAL** — mesuré le 2026-10-24 avec
/// la configuration initiale du portable (`config_initiale`) : adaptée à un écran
/// deux fois moins haut, la barre de droite passe de 124 à 62 px, et les « 32 px »
/// pris à la lettre n'y laissaient plus qu'UNE colonne là où il y en avait deux.
/// 32 px à 150 % font 21⅓ px logiques : c'est cette taille-là qui reste lisible
/// d'un écran à l'autre. À 144 DPI, rien ne change.
pub fn poser_dpi_du_plancher_de_colonne(dpi: u32) {
    let dpi = if dpi == 0 { DPI_DU_PLANCHER_DE_COLONNE } else { dpi };
    PLANCHER_DE_COLONNE.with(|c| c.set(MIN_ZONE_COLUMN_PX * dpi as f32 / DPI_DU_PLANCHER_DE_COLONNE as f32));
}

/// Espacement FIXE (px, PAS `Dock::icon_gap`) utilisé de chaque côté d'un
/// espace invisible dans l'empilement de sa barre (voir `Dock::
/// bar_layout`) — demande explicite : "le séparateur ne fait toujours
/// pas un tiers de la hauteur d'une icône", alors que sa hauteur PROPRE
/// (`SPACER_HEIGHT`) était bien correcte : `icon_gap`, réglable par
/// l'utilisateur (voir `config::load_icon_gap`, jusqu'à 40px constaté en
/// pratique), s'ajoute des DEUX côtés de CHAQUE emplacement, spacer compris
/// — un grand `icon_gap` écrasait donc entièrement l'effet de
/// `SPACER_HEIGHT` sur l'empreinte TOTALE (hauteur + espacement) d'un
/// espace, qui restait alors comparable à celle d'une icône normale malgré
/// une hauteur propre réduite. Un espacement propre à l'espace, INDÉPENDANT
/// du réglage utilisateur, garantit une empreinte totale toujours petite.
const SPACER_GAP: f32 = 6.0;

/// Combien de pixels un cran de molette déplace les icônes d'une barre (voir
/// `Dock::scroll_bar`) — une FRACTION d'icône plutôt qu'une icône entière :
/// les icônes font 32 à 96 px selon l'épaisseur de la barre (voir
/// `icon_size_for`), un déplacement d'une icône pleine paraîtrait sauter, et
/// un cran de molette vaut de toute façon 120 unités arbitraires (voir
/// `Event::MouseWheel`), pas une distance.
///
/// C'est le DÉFAUT du réglage par barre (voir `scroll_step_px`) : la valeur
/// appliquée est celle de la cellule, pas cette constante.
const SCROLL_STEP_PX: f32 = 48.0;

/// Bornes du réglage de VITESSE DE DÉFILEMENT À LA MOLETTE, en pixels par cran —
/// DEMANDE EXPLICITE : « pouvoir régler les vitesses de défilement à la molette
/// dans les barres », amplitude choisie par l'utilisateur (16 à 160 px). Le
/// défaut reste la valeur d'origine, 48 px.
///
/// Le PLANCHER à 16 px correspond à peu près à un cinquième d'icône : en dessous,
/// un cran ne déplacerait presque rien et le défilement paraîtrait bloqué. Le
/// PLAFOND à 160 px vaut deux icônes larges : au-delà, une barre se parcourt en
/// deux crans et le réglage perd son intérêt.
pub const MIN_SCROLL_STEP_PX: f32 = 16.0;
pub const MAX_SCROLL_STEP_PX: f32 = 160.0;
/// Défaut du réglage — la valeur d'origine, voir `SCROLL_STEP_PX`.
pub const DEFAULT_SCROLL_STEP_PX: f32 = SCROLL_STEP_PX;

thread_local! {
    /// Vitesse de défilement à la molette, PAR BARRE (voir `side_slot` : index 0
    /// = gauche, 1 = droite) — DEMANDE EXPLICITE : « une par barre », et l'index
    /// est le MÊME que celui de `Dock::scroll` (même convention, même fonction,
    /// donc aucun risque de confondre les deux barres).
    ///
    /// GLOBALE et non portée par le `Dock`, comme `hover_scale`/`icon_gap` : le
    /// panneau Réglages règle un curseur qui doit valoir pour la barre primaire
    /// ET pour celle de chaque écran secondaire — ces docks-là vivent dans des
    /// `DockInstance` que le panneau n'atteint pas (voir `SettingsRefs`, qui ne
    /// porte qu'un `&mut Dock`).
    ///
    /// Un cran DANS une zone suit la vitesse de SA barre (même chemin,
    /// `Dock::scroll_bar`) : deux vitesses pour un même geste se liraient comme
    /// un défaut.
    /// (Quatre cellules identiques écrites une par une : `[Cell::new(x); 4]`
    /// exigerait `Cell: Copy`, ce qu'elle n'est pas — un bloc `const` par
    /// élément dit la même chose sans la copie.)
    static SCROLL_STEP: [Cell<f32>; 4] = [
        const { Cell::new(DEFAULT_SCROLL_STEP_PX) },
        const { Cell::new(DEFAULT_SCROLL_STEP_PX) },
        const { Cell::new(DEFAULT_SCROLL_STEP_PX) },
        const { Cell::new(DEFAULT_SCROLL_STEP_PX) },
    ];
}

/// Vitesse de défilement actuelle de la barre `side`, en pixels par cran — voir
/// `SCROLL_STEP`. Les deux entrées de la barre du HAUT existent (même tableau
/// que `Dock::scroll`) mais ne sont jamais lues : elle ne défile pas.
pub fn scroll_step_px(side: Side) -> f32 {
    SCROLL_STEP.with(|steps| steps[side_slot(side)].get())
}

/// Change la vitesse de défilement d'une barre, bornée à
/// [`MIN_SCROLL_STEP_PX`, `MAX_SCROLL_STEP_PX`] — le BORNAGE est fait ici, comme
/// pour `set_hover_scale` : l'appelant est un curseur, dont la position EST la
/// valeur.
pub fn set_scroll_step_px(side: Side, px: f32) {
    SCROLL_STEP.with(|steps| steps[side_slot(side)].set(px.clamp(MIN_SCROLL_STEP_PX, MAX_SCROLL_STEP_PX)));
}

/// Un cran de molette, en unités de `WM_MOUSEWHEEL` — valeur Win32 fixe
/// (`WHEEL_DELTA`) : un cran vaut toujours ±120.
const WHEEL_DELTA: f32 = 120.0;

/// Index du décalage de défilement (voir `Dock::scroll`) d'une barre
/// latérale.
fn side_slot(side: Side) -> usize {
    match side {
        Side::Left => 0,
        Side::Right => 1,
        // La barre du haut ne défile pas (elle s'étend en largeur) : ses deux
        // moitiés retombent sur l'entrée de gauche, qui ne les concerne pas —
        // `scroll_bar` refuse explicitement ces deux zones, et
        // `layout_top_side` n'applique aucun décalage. La barre du BAS est dans
        // le même cas : horizontale, donc sans défilement vertical.
        Side::TopLeft | Side::TopRight | Side::TopMiddle | Side::Bottom => 0,
    }
}

/// Un emplacement de barre LATÉRALE — la place qu'occupe une icône, un espace
/// ou un séparateur de zone dans l'empilement vertical de sa barre (voir
/// `Dock::bar_layout`, qui les produit tous d'un coup).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Slot {
    /// Bord SUPÉRIEUR, en coordonnées fenêtre — défilement de barre déjà
    /// appliqué (via `start_y`), et défilement de la zone aussi s'il y en a
    /// un (voir `zone`).
    top: f32,
    /// Largeur/hauteur de la case — `height` ET largeur, puisqu'une case est
    /// CARRÉE : l'icône d'une zone en grille rétrécit pour tenir dans sa
    /// colonne (voir `Dock::zone_cell_size`), et les deux dimensions suivent
    /// ensemble. Hors grille, c'est la taille d'icône de la barre.
    height: f32,
    /// Rang de la COLONNE dans la rangée — `0` hors grille, où la barre n'a
    /// qu'une colonne (voir `Dock::bar_layout`, `layout_side`). C'est
    /// `layout_side` qui la tourne en abscisse : `bar_layout` ne connaît que
    /// l'écoulement vertical, et lui donner la géométrie de la barre en plus
    /// ferait deux endroits qui savent où sont les colonnes.
    column: usize,
    /// Zone dont cet emplacement est le CONTENU (index dans
    /// `BarLayout::zones`), donc celle dont le défilement propre le déplace —
    /// `None` pour tout ce qui PRÉCÈDE le premier séparateur de zone, ET pour
    /// un séparateur de zone lui-même : sa position est IMPOSÉE par la zone
    /// qu'il clôt, aucun défilement ne doit l'en écarter.
    zone: Option<usize>,
}

/// Boîte déclarée par un séparateur de zone (voir `Icon::zone_height`) et
/// déduite par `Dock::bar_layout` : une étendue VERTICALE FIXE que le contenu
/// qui suit ne peut ni dépasser (il est rogné, et défile DANS la boîte) ni
/// rétrécir (le vide qu'il laisse reste du vide).
#[derive(Debug, Clone, Copy, PartialEq)]
struct ZoneBox {
    /// Index (dans `Dock::icons`) du séparateur qui OUVRE la zone — la cote
    /// appartient à LUI, et c'est par lui qu'on la retrouve.
    #[allow(dead_code)] // consommé par l'étape « la cote au survol » (et le réglage de la cote)
    sep_index: usize,
    /// Haut du CONTENU de la zone : juste sous son séparateur, `SPACER_GAP`
    /// compris.
    top: f32,
    /// Bas de la boîte, toujours `top + cote` (voir `Icon::zone_height`) —
    /// l'élément suivant, quel qu'il soit, commence exactement là.
    bottom: f32,
    /// Hauteur RÉELLEMENT occupée par le contenu (bas du dernier emplacement
    /// de la zone moins `top`, `0.0` si elle est vide) — c'est
    /// `content_height > bottom - top` qui rend la zone défilable.
    content_height: f32,
}

/// ⭐ La barre latérale REPLIÉE (voir `Dock::pliage_de_la_barre`,
/// `ui::empilement::plier`) : pour chaque élément de `ordre`, sa `Place`,
/// comptée depuis le haut de la colonne (`haut`).
#[derive(Debug, Clone, PartialEq)]
struct PliageDeBarre {
    ordre: Vec<usize>,
    places: Vec<crate::ui::empilement::Place>,
    elements: Vec<crate::ui::empilement::Element>,
    longueur: f32,
    haut: f32,
}

/// Résultat de `Dock::bar_layout` : les emplacements ET les zones d'une barre
/// latérale, calculés ENSEMBLE parce que les seconds ne se déduisent pas des
/// premiers — une zone au contenu plus court que sa cote n'occupe aucun
/// emplacement là où elle laisse du vide, et c'est ce vide qui fixe le début
/// de tout ce qui suit.
#[derive(Debug, Clone, Default, PartialEq)]
struct BarLayout {
    slots: Vec<Slot>,
    zones: Vec<ZoneBox>,
}

impl BarLayout {
    /// Bas du contenu d'une barre — le plus bas de son dernier emplacement ET
    /// de sa dernière boîte de zone : ce que le défilement de barre
    /// (`Dock::reclamp_scroll`) doit pouvoir amener à l'écran. Sans zone,
    /// c'est exactement le bas du dernier emplacement, comme avant elles.
    fn content_height(&self) -> f32 {
        let slots = self.slots.iter().map(|slot| slot.top + slot.height).fold(0.0, f32::max);
        let zones = self.zones.iter().map(|zone| zone.bottom).fold(0.0, f32::max);
        slots.max(zones)
    }
}

/// Diamètre PAR DÉFAUT (px) du petit cercle indicateur d'un espace invisible
/// (voir `spacer_marker_instance`) — INDÉPENDANT de `SPACER_HEIGHT` (demande
/// explicite : "agrandis le point dans le séparateur") : les deux tailles
/// n'ont plus besoin de se contraindre l'une l'autre comme quand le cercle
/// dépendait d'une fraction de la taille d'icône (voir l'historique, 0.22
/// puis 0.12) — nettement plus grand qu'avant pour ne plus risquer qu'on le
/// manque (voir l'historique : plusieurs espaces créés par erreur faute
/// d'un retour visuel assez net que le premier avait bien été ajouté).
/// Reste sous `SPACER_HEIGHT` pour ne pas déborder de son emplacement.
/// DEMANDE EXPLICITE ("ajoute la possibilité de modifier la taille des
/// séparateurs par interface") : n'est plus qu'une valeur de repli — voir
/// `Dock::separator_size`, réglable par écran comme `icon_gap`.
pub const SPACER_MARKER_DIAMETER: f32 = 14.0;

/// Diamètre minimum/maximum acceptés pour `Dock::set_separator_size` — bornes
/// du curseur du panneau de réglages (voir `win32::cef_settings`). Le
/// minimum garde le cercle repérable au clic ; le maximum reste sous
/// `SPACER_HEIGHT` pour ne pas déborder de l'emplacement de l'espace (voir
/// la doc de `SPACER_MARKER_DIAMETER`).
pub const MIN_SEPARATOR_SIZE: f32 = 4.0;
pub const MAX_SEPARATOR_SIZE: f32 = SPACER_HEIGHT;

/// Instance de quad (petit cercle plein, voir `Dock::separator_size`) centré
/// dans `icon_rect` — rend un espace invisible (voir `Icon::is_spacer`)
/// repérable sans pour autant le faire ressembler à une vraie icône. `scale`
/// : même facteur de magnification au survol que les icônes normales (voir
/// `HoverAnimator`), pour un repère cohérent "cette case réagit aussi au
/// survol/au glisser". `color` : résolue par l'appelant (voir `Dock::
/// separator_color`) — par défaut la couleur principale du texte de la
/// barre à laquelle l'espace appartient (DEMANDE EXPLICITE : "les
/// séparateurs d'icones soit de la couleur principale du texte de la barre
/// à laquelle ils sont ratachés"), personnalisable ou rendue transparente
/// (alpha à 0) par l'utilisateur. `diameter` : résolu par l'appelant (voir
/// `Dock::separator_size`) — même principe que `color`, DEMANDE EXPLICITE
/// ("ajoute la possibilité de modifier la taille des séparateurs par
/// interface").
fn spacer_marker_instance(icon_rect: [f32; 4], scale: f32, color: [f32; 4], diameter: f32) -> QuadInstance {
    let [x, y, w, h] = icon_rect;
    let cx = x + w * 0.5;
    let cy = y + h * 0.5;
    let rect = [cx - diameter * 0.5, cy - diameter * 0.5, diameter, diameter];
    QuadInstance::new(rect, color, diameter * 0.5, scale)
}

/// Taille par défaut de la pastille "en cours d'exécution" (voir
/// `win32::appicon::is_running`) superposée à une icône du dock.
pub const DEFAULT_RUNNING_DOT_SIZE: f32 = 12.0;

/// Décalage maximal de l'indicateur, dans chaque sens (voir `running_dot_offset`).
pub const MAX_RUNNING_DOT_OFFSET: f32 = 60.0;
/// Bornes du curseur « Taille de l'indicateur » des Réglages — DEMANDE
/// EXPLICITE : « donnes moi la possibilité d'agrandir la taille de
/// l'indicateur ». Le minimum le garde visible, le maximum reste sous la taille
/// d'une icône pour ne pas la couvrir entièrement.
pub const MIN_RUNNING_DOT_SIZE: f32 = 4.0;
/// DEMANDE EXPLICITE : « je voudrais que la taille maximale de l'indicateur soit
/// équivalent à 3 fois la valeur actuelle » (28 -> 84), PUIS « le maximum est
/// trop bas » — relevé à 200, soit le double de la plus grande icône possible
/// (`ICON_SIZE_MAX`). La borne n'existe que pour éviter une valeur absurde
/// (négative, ou nulle au point de rendre l'indicateur invisible).
///
/// **La pastille PEUT être rognée au bord de sa barre** au-delà d'environ 73px
/// (rond, barre de 140px, icône de 96px) : son centre est posé à 20 % de sa
/// taille à l'intérieur du coin de l'icône, donc son bord extérieur dépasse
/// d'autant plus qu'elle grandit, et il ne reste que ~22px de marge avant le
/// bord de la barre — davantage encore pour la forme « Trait », large de 2,4
/// fois la taille. C'est un CHOIX ASSUMÉ de l'utilisateur : il lui a été proposé
/// de recentrer la pastille pour qu'elle grandisse vers l'intérieur et reste
/// dans sa barre, et il a préféré garder sa position actuelle avec un plafond
/// plus haut. Ne pas « corriger » la coupe sans le lui redemander.
pub const MAX_RUNNING_DOT_SIZE: f32 = 200.0;

/// De combien le bas de l'indicateur « en cours d'exécution » pend SOUS le bas
/// de l'icône qu'il marque — voir `Dock::running_dot_instance`. DEMANDE
/// EXPLICITE : « il faudrait que l'indicateur ait sa partie basse 3 pixels plus
/// bas que l'icone » (il était auparavant 3px AU-DESSUS, donc mordu sur
/// l'icône). Fixe, donc indépendante du réglage de taille : c'est ce qui fait
/// grandir l'indicateur vers le HAUT uniquement, au lieu qu'il s'enfonce vers
/// les icônes du dessous quand on l'agrandit.
const DOT_BOTTOM_OVERHANG: f32 = 3.0;

/// Forme de l'indicateur « en cours d'exécution » — DEMANDE EXPLICITE :
/// « de changer sa forme ». Sept formes, chacune avec sa raison d'être (le rond
/// est discret, le carré s'aligne sur les icônes, le trait et le souligné se
/// lisent comme des traits, le losange se remarque, l'anneau entoure l'icône).
///
/// **Toutes géométriquement distinctes** : une forme de plus qui n'aurait pas
/// d'effet visible serait le pire des réglages, et le test
/// `running_dot_shapes_are_all_distinguishable` le vérifie sur `ALL` — donc
/// toute variante ajoutée y passe d'office.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunningDotShape {
    Circle,
    Square,
    /// Trait horizontal, façon soulignement — la forme la plus lisible quand la
    /// pastille est petite.
    Bar,
    /// DEMANDE EXPLICITE : « j'aimerais aussi avoir un rectangle de la moitié
    /// de la largeur de l'icône comme forme disponible ». Sa largeur suit donc
    /// l'ICÔNE, pas le curseur de taille — celui-ci donne sa hauteur.
    Rectangle,
    /// Ligne FINE qui suit presque toute la largeur de l'icône — DEMANDE
    /// EXPLICITE : le soulignement de la barre des tâches Windows 11. Se
    /// distingue du `Rectangle` par sa hauteur (une fraction de la taille, au
    /// lieu de la taille entière) et du `Bar` par sa largeur (celle de l'icône,
    /// au lieu d'un multiple du curseur).
    Underline,
    /// Carré pivoté d'un quart de tour — DEMANDE EXPLICITE. **C'est le seul cas
    /// où la boîte du quad ne dit pas ce qu'on voit** : voir `footprint` et
    /// `rotation`, sans quoi la forme déborderait du côté écran.
    Diamond,
    /// CADRE CREUX autour de l'icône — DEMANDE EXPLICITE. C'est la SEULE forme
    /// qui n'est pas suspendue sous l'icône : elle l'entoure (voir `placement`),
    /// et c'est aussi la seule que le pipeline ne sait pas rendre en aplat, d'où
    /// le mode « anneau » des quads (voir `QuadInstance::new_ring`). Sa taille
    /// règle son ÉPAISSEUR, pas une dimension (voir `ring_box`).
    Ring,
}

/// Où une forme d'indicateur se pose par rapport à l'icône qu'elle marque (voir
/// `RunningDotShape::placement`) — c'est ce que `running_dot_instance` branche.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DotPlacement {
    /// SUSPENDUE sous l'icône, accrochée à son coin extérieur (voir
    /// `hung_dot_box`).
    Hung,
    /// AUTOUR de l'icône, en cadre creux (voir `ring_box`).
    Around,
}

impl RunningDotShape {
    pub const ALL: [RunningDotShape; 7] =
        [Self::Circle, Self::Square, Self::Bar, Self::Rectangle, Self::Underline, Self::Diamond, Self::Ring];

    /// Nom persisté (voir `config::load_running_dot_shape`) — stable, donc en
    /// anglais et jamais traduit : il vit dans un fichier de configuration.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Circle => "circle",
            Self::Square => "square",
            Self::Bar => "bar",
            Self::Rectangle => "rectangle",
            Self::Underline => "underline",
            Self::Diamond => "diamond",
            Self::Ring => "ring",
        }
    }

    pub fn from_config(value: &str) -> Self {
        match value.trim() {
            "square" => Self::Square,
            "bar" => Self::Bar,
            "rectangle" => Self::Rectangle,
            "underline" => Self::Underline,
            "diamond" => Self::Diamond,
            "ring" => Self::Ring,
            _ => Self::Circle,
        }
    }

    /// Libellé affiché dans les Réglages.
    pub fn label(self) -> &'static str {
        match self {
            Self::Circle => "Rond",
            Self::Square => "Carré",
            Self::Bar => "Trait",
            Self::Rectangle => "Rectangle",
            Self::Underline => "Souligné",
            Self::Diamond => "Losange",
            Self::Ring => "Anneau",
        }
    }

    /// `(largeur, hauteur, rayon des coins)` pour une taille demandée.
    ///
    /// La « taille » du réglage est le diamètre pour le rond et le carré ; pour
    /// le trait, elle donne la HAUTEUR et la largeur en découle — sans quoi le
    /// même curseur produirait un trait minuscule à côté d'un rond correct.
    ///
    /// `icon_w` : largeur de l'icône marquée, dont dépendent le `Rectangle` et
    /// le `Underline` (voir leurs doc) — les autres formes l'ignorent.
    ///
    /// **`None` pour l'anneau**, qui n'a pas de boîte suspendue : il est posé
    /// SUR la case de l'icône, et la sienne dépend de sa HAUTEUR comme de sa
    /// largeur, que cette fonction ne reçoit pas (voir `ring_box`). Un `Option`
    /// explicite, et non une valeur de repli inventée : les tests de distinction
    /// parcourent `ALL`, et une boîte de repli ferait passer l'anneau pour une
    /// forme suspendue plausible — exactement ce qu'il n'est pas.
    fn geometry(self, size: f32, icon_w: f32) -> Option<(f32, f32, f32)> {
        Some(match self {
            Self::Circle => (size, size, size * 0.5),
            Self::Square => (size, size, size * 0.18),
            Self::Bar => {
                let height = (size * 0.45).max(3.0);
                (size * 2.4, height, height * 0.5)
            }
            // Moitié de la largeur de l'ICÔNE, hauteur donnée par le curseur —
            // un rectangle large et bas, qui occupe la moitié du bas de l'icône.
            Self::Rectangle => (icon_w * 0.5, size, size * 0.18),
            // Presque toute la largeur de l'icône, et une hauteur qui reste une
            // FRACTION de la taille : c'est ce qui en fait un soulignement et
            // non un rectangle. Le plancher de 2 px l'empêche de disparaître aux
            // petites tailles (même garde-fou que le `Bar`).
            Self::Underline => {
                let height = (size * 0.18).max(2.0);
                (icon_w * 0.8, height, height * 0.5)
            }
            // La DIAGONALE vaut la taille demandée, donc le côté vaut
            // `taille/√2` : une boîte pivotée d'un quart de tour occupe `√2`
            // fois son côté, et c'est cet encombrement-là que l'ancrage
            // considère (voir `footprint`). Sans cette conversion, le losange
            // déborderait du côté écran de 41 px à la taille maximale.
            Self::Diamond => {
                let side = size * std::f32::consts::FRAC_1_SQRT_2;
                (side, side, side * 0.18)
            }
            // L'anneau n'a pas de boîte suspendue (voir la doc ci-dessus).
            Self::Ring => return None,
        })
    }

    /// Où la forme se pose par rapport à l'icône : SUSPENDUE sous elle, ou
    /// AUTOUR d'elle. Seul l'anneau entoure.
    fn placement(self) -> DotPlacement {
        match self {
            Self::Ring => DotPlacement::Around,
            _ => DotPlacement::Hung,
        }
    }

    /// Comme `geometry`, mais pour une forme dont on SAIT qu'elle est suspendue :
    /// les tests qui éprouvent une forme précise n'ont que faire du `None` de
    /// l'anneau (voir `geometry`, dont l'`Option` est justifié).
    #[cfg(test)]
    fn hung_geometry(self, size: f32, icon_w: f32) -> (f32, f32, f32) {
        self.geometry(size, icon_w).expect("cette forme est suspendue")
    }

    /// Facteur d'ENCOMBREMENT de la forme : ce qu'elle occupe une fois
    /// DESSINÉE, rapporté à sa largeur de boîte.
    ///
    /// Vaut `1.0` partout sauf pour le `Diamond`, dont la boîte pivotée d'un
    /// quart de tour occupe `√2` fois son côté. **C'est ce facteur, et non la
    /// boîte du quad, qui est posé bord à bord avec le côté écran** (voir
    /// `hung_dot_box`) : l'ignorer ferait sortir les coins du losange de la
    /// colonne de l'icône — exactement le défaut que l'ancrage corrige.
    fn footprint(self) -> f32 {
        match self {
            Self::Diamond => std::f32::consts::SQRT_2,
            // Explicite, et non un `_` : une future forme `Around` doit se
            // demander si elle a, comme le losange, une boîte qui ment sur son
            // encombrement.
            Self::Ring => 1.0,
            Self::Circle | Self::Square | Self::Bar | Self::Rectangle | Self::Underline => 1.0,
        }
    }

    /// Rotation à poser sur l'instance (radians, autour du CENTRE de sa boîte).
    /// Seule la forme qui n'est pas ce qu'elle paraît en a une : le losange EST
    /// le carré, pivoté. L'anneau, lui, reçoit celle de l'ICÔNE (voir
    /// `running_dot_instance`), pas la sienne.
    fn rotation(self) -> f32 {
        match self {
            Self::Diamond => std::f32::consts::FRAC_PI_4,
            Self::Ring => 0.0,
            Self::Circle | Self::Square | Self::Bar | Self::Rectangle | Self::Underline => 0.0,
        }
    }
}

/// Indicateur de l'application lancée — l'overlay posé sur le coin EXTÉRIEUR de
/// son icône, façon indicateur « actif » des docks macOS/Unity (voir `win32::
/// appicon::focus_or`).
///
/// **Le côté est déduit de la BARRE qui porte l'icône** (`Dock::icon_side`), et
/// c'est un DÉFAUT RÉEL SIGNALÉ qui l'a imposé : « j'ai déplacé une icône de
/// programme dans la barre de droite et je vois que son indicateur est à droite,
/// il faudrait qu'il passe à gauche ». La pastille était posée en dur au coin
/// BAS-DROITE, ce qui tombe bien pour la barre de gauche (le côté tourné vers la
/// zone libre) mais pas pour celle de droite, où c'est le côté collé au bord de
/// l'écran.
///
/// Le côté EXTÉRIEUR est donc celui qui regarde la zone libre : à droite pour la
/// barre de gauche et les deux moitiés de la barre du haut (où l'indicateur pend
/// de toute façon SOUS l'icône), à GAUCHE pour la barre de droite. **La
/// géométrie elle-même vit dans `hung_dot_box`**, fonction pure, testable sans
/// `Dock` — c'est elle qui porte la règle d'ancrage.
///
/// **Méthode de `Dock` et non fonction libre** : la pastille a une TAILLE, une
/// FORME et une COULEUR réglables (voir `config::load_running_dot_size` et
/// consorts), qui vivent dans le dock — les passer en paramètres à chaque appel
/// ne ferait que déplacer la verbosité.
impl Dock {
    /// Vrai si la forme choisie **ENTOURE** l'icône au lieu de pendre sous elle
    /// (voir `DotPlacement`) — aujourd'hui l'anneau, et lui seul.
    ///
    /// Existe pour `app::run` : une forme qui entoure doit être poussée dans le
    /// même lot TARDIF et NON DÉCOUPÉ que l'icône magnifiée, sinon sa bande
    /// supérieure — qui monte au-dessus de la barre quand l'icône grandit —
    /// serait coupée par `Dock::lateral_clip`, alors que l'icône, elle, déborde
    /// librement. Un anneau ouvert en haut se lit comme un défaut.
    pub fn running_dot_wraps_the_icon(&self) -> bool {
        self.running_dot_shape.placement() == DotPlacement::Around
    }

    /// Instance de l'indicateur de l'icône `index`, `None` si elle n'est pas
    /// dessinable (hors bornes, en cours de glisser, désactivée, ou emplacement
    /// entièrement sorti par le défilement).
    ///
    /// `color_override` : couleur FORCÉE de la pastille, qui l'emporte sur le
    /// réglage du dock — sert au clignotement de notification (voir
    /// `notification_blink_color`), qui est une couleur PAR ICÔNE et non un
    /// réglage.
    ///
    /// **Prend l'INDEX et non un rectangle** : la pastille suit ainsi exactement
    /// ce qui est DESSINÉ (`presented_rect` — magnification, décalage de survol
    /// et défilement compris). C'est ce qui la fait survivre à l'agrandissement
    /// de son icône — DEMANDE EXPLICITE : « j'aimerais que l'indicateur ne
    /// disparaisse pas quand l'icône est agrandie ». Les trois appelants y
    /// gagnent aussi : ils calculaient chacun `icon_rect` ET `icon_side`, deux
    /// choses qui pouvaient diverger.
    pub fn running_dot_instance(&self, index: usize, layout: &DockLayout, color_override: Option<[f32; 4]>) -> Option<QuadInstance> {
        // ⭐ Une icône REPLIÉE : la pastille se pose sous l'icône ENTIÈRE et
        // bascule avec elle (voir `ui::empilement::projeter_attache`).
        let bascule = self.bascule_de(index, layout);
        let icon = match bascule {
            Some((entiere, _)) => entiere,
            None => self.presented_rect(index, layout)?,
        };
        let side = self.icon_side(index)?;
        let shape = self.running_dot_shape;
        let size = self.running_dot_size;
        // Couleur : le clignotement de notification s'il y en a un, sinon celle
        // choisie dans les Réglages, sinon l'accent de la famille de thèmes (voir
        // `color::accent_du_theme`), sinon la couleur d'accentuation du dock —
        // le comportement d'origine.
        // ⭐ Le DÉGRADÉ remplace la couleur choisie ; saturation, luminosité et
        // transparence s'appliquent ENSUITE à toutes les couleurs — sauf au
        // clignotement de notification, qui est un SIGNAL et reste tel quel.
        let color = match color_override {
            Some(clignotement) => clignotement,
            None => {
                let base = if indicateur_degrade() {
                    // La palette ne donne que la TEINTE : en mode sombre ses tons sont
                    // des tons de FOND, presque noirs sous l'icône (mesuré à la
                    // capture). Saturation et luminosité sont celles de l'indicateur
                    // automatique, si bien que le dégradé brille autant que lui.
                    let fond = self.background();
                    let accent = color::accent_du_theme().unwrap_or([fond[0], fond[1], fond[2]]);
                    let palette = color::icon_palette(accent, !color::mode_clair());
                    let teinte = couleur_degradee_at(logo_animation_elapsed().as_secs_f32(), index, &palette);
                    couleur_du_degrade(teinte, color::accent_du_theme())
                } else {
                    couleur_indicateur(None, self.running_dot_color, color::accent_du_theme(), self.background())
                };
                ajuster_indicateur(base, indicateur_reglages())
            }
        };
        // Rotation de l'ICÔNE (nulle au repos) : une forme qui ENTOURE doit la
        // suivre, sinon l'icône magnifiée se balance (voir `hover_wobble_angle`)
        // et ses coins traversent la bande de l'anneau deux fois par période.
        let icon_rotation = hover_wobble_angle(self.hover.scale(index));
        Some(match shape.placement() {
            DotPlacement::Around => {
                let (mut rect, radius, thickness) = ring_box(icon, size, self.icon_gap_de(index));
                rect[0] += self.running_dot_offset;
                let mut ring = QuadInstance::new_ring(rect, color, radius, thickness);
                ring.rotation = icon_rotation;
                // Centré sur l'icône : il bascule exactement comme elle.
                if let Some((_, angle)) = bascule {
                    ring.rotation = 0.0;
                    ring.bascule = angle;
                }
                // La texture de l'interface — DEMANDE EXPLICITE (2026-10-16).
                ring.avec_texture()
            }
            DotPlacement::Hung => {
                let (mut rect, radius) = hung_dot_box(icon, side, shape, size)?;
                rect[0] += self.running_dot_offset;
                if let Some((entiere, angle)) = bascule {
                    rect = crate::ui::empilement::projeter_attache(rect, entiere, angle, layout.total_height);
                }
                let mut dot = QuadInstance::new(rect, color, radius, 1.0);
                // Rotation PROPRE À LA FORME (le losange) : elle se pose à part,
                // comme pour l'icône magnifiée — même champ, même pivot central.
                dot.rotation = shape.rotation();
                if let Some((_, angle)) = bascule {
                    dot.bascule = angle;
                }
                dot.avec_texture()
            }
        })
    }
}

/// Épaisseur MINIMALE d'un anneau, en pixels.
///
/// Elle ne mord JAMAIS en borne basse — `size × 0.25` vaut au moins 1 pour la
/// plus petite taille réglable (4) — mais elle sert de PLANCHER AU PLAFOND :
/// sans elle, une icône de moins de 4 px donnerait un plafond inférieur au
/// minimum, et le `clamp` paniquerait (`min > max`).
const MIN_RING_THICKNESS: f32 = 1.0;

/// Épaisseur de l'anneau pour la taille demandée.
///
/// **Le curseur « Taille de l'indicateur » y donne une ÉPAISSEUR, pas un
/// diamètre** : un anneau de 200 px de diamètre autour d'une icône de 96 serait
/// un carré plein. Le quart de la taille est l'échelle qui reste lisible à tous
/// les réglages.
///
/// **Deux plafonds, et pas un de plus** :
/// - `TOP_MARGIN` — la bande SUPÉRIEURE de l'anneau de la première icône d'une
///   barre latérale sortirait sinon au-dessus de la barre, coupée par
///   `Dock::lateral_clip` : un anneau ouvert en haut se lit comme un défaut ;
/// - l'écartement des icônes (`gap`) — la bande s'étend de `t` de chaque côté,
///   donc au-delà de l'écartement elle mordrait sur la case VOISINE, où elle
///   serait recouverte par l'icône texturée d'à côté.
///
/// **Aucun plafond proportionnel à l'icône, délibérément** : la bande est
/// ENTIÈREMENT hors de la boîte de l'icône (propriété du SDF, voir
/// `QuadInstance::new_ring`), donc elle ne peut jamais la recouvrir. Un tel
/// plafond, en plus d'être inutile, ferait VARIER l'épaisseur pendant
/// l'animation de magnification — il dépendrait d'une taille qui bouge.
///
/// `gap` : `Dock::icon_gap()`, pris en paramètre plutôt que lu ici pour que
/// cette fonction reste PURE, donc testable sans `Dock`.
fn ring_thickness(size: f32, icon: [f32; 4], gap: f32) -> f32 {
    let ceiling = TOP_MARGIN.min(gap).max(MIN_RING_THICKNESS);
    (size * 0.25).clamp(MIN_RING_THICKNESS, ceiling)
}

/// Boîte, rayon des coins et épaisseur de l'anneau — fonction PURE, comme
/// `hung_dot_box`, et pour la même raison : c'est la géométrie qui se vérifie
/// sans `Dock`.
///
/// `icon` : la case de l'icône — **sa case PRÉSENTÉE**, comme pour les formes
/// suspendues, donc l'anneau suit la magnification de ce qu'il entoure. Le
/// BALANCEMENT, lui, ne se joue pas ici : c'est `running_dot_instance` qui pose
/// la rotation de l'icône sur l'instance.
///
/// Le rayon des coins vaut `icon_radius() + épaisseur` : le contour INTÉRIEUR de
/// l'anneau épouse donc exactement les coins de l'icône, et la bande garde une
/// épaisseur CONSTANTE sur tout le pourtour. Un rayon plus grand laisserait les
/// coins de l'icône à nu ; plus petit, l'anneau serait plus épais aux coins.
///
/// Il n'est pas question ici de « coin extérieur » ni de croissance : l'anneau
/// entoure, il ne pend pas (voir `DotPlacement`).
fn ring_box(icon: [f32; 4], size: f32, gap: f32) -> ([f32; 4], f32, f32) {
    let [x, y, w, h] = icon;
    let thickness = ring_thickness(size, icon, gap);
    ([x - thickness, y - thickness, w + 2.0 * thickness, h + 2.0 * thickness], icon_radius() + thickness, thickness)
}

/// Boîte et rayon des coins d'une forme SUSPENDUE (voir `RunningDotShape`) —
/// fonction PURE, donc vérifiable sans `Dock` : c'est elle qui porte la règle
/// d'ancrage, donc celle que le défaut signalé fait verrouiller par un test.
///
/// `icon` : la case de l'icône — **sa case PRÉSENTÉE** si elle est magnifiée,
/// voir `Dock::presented_rect`, seule source de vérité de ce qui est dessiné.
///
/// **L'ANCRAGE SE FAIT PAR L'ENCOMBREMENT, PAS PAR UN CENTRE DÉCALÉ** — DEMANDE
/// EXPLICITE : « dans le coin inférieur droit […] qu'ils grandissent vers le
/// haut et le centre, et je ne veux pas que leur partie droite déborde de la
/// zone visible, ça crée un souci visuel quand les icônes proches sont
/// agrandies ». Le bord EXTÉRIEUR de la forme coïncide donc EXACTEMENT avec le
/// bord de l'icône, quelle que soit la taille : rien ne peut plus sortir de la
/// colonne de l'icône, donc rien ne peut mordre sur la zone libre à côté d'une
/// icône agrandie. Et comme ce bord ne bouge pas avec la taille, la forme croît
/// vers le HAUT (son bas reste ancré) et vers le CENTRE de l'icône.
///
/// Remplace un décalage proportionnel à la taille (`taille × 0.2`), dont la
/// moitié extérieure sortait de la colonne — et d'autant plus que la forme
/// était grande.
///
/// `side` : la barre qui porte l'icône ; le côté EXTÉRIEUR est celui qui regarde
/// la zone libre (à droite pour les barres de gauche et les deux moitiés de la
/// barre du haut, à gauche pour la barre de droite — même convention que
/// `running_dot_instance` avant ce changement, voir `Side`).
fn hung_dot_box(icon: [f32; 4], side: Side, shape: RunningDotShape, size: f32) -> Option<([f32; 4], f32)> {
    let [x, y, iw, ih] = icon;
    let (w, h, radius) = shape.geometry(size, iw)?;
    let occupied = w * shape.footprint();
    let cx = match side {
        // Barre de DROITE : le côté extérieur est à GAUCHE.
        Side::Right => x + occupied * 0.5,
        Side::Left | Side::TopLeft | Side::TopRight | Side::TopMiddle | Side::Bottom => x + iw - occupied * 0.5,
    };
    // Le bas est ancré à distance FIXE sous l'icône (voir `DOT_BOTTOM_OVERHANG`),
    // donc indépendant de la taille : la forme grandit vers le haut, jamais vers
    // le bas — DEMANDE EXPLICITE antérieure, verrouillée par
    // `running_dot_grows_upward_only`.
    let bottom = y + ih + DOT_BOTTOM_OVERHANG;
    Some(([cx - w * 0.5, bottom - h, w, h], radius))
}

/// Rectangles (coordonnées fenêtre) des fonds des trois barres — haut,
/// gauche, droite, dans cet ordre — qui se touchent exactement (aucun
/// chevauchement, aucune ligne de jonction visible : voir
/// `gfx::quad_renderer`, qui dessine les rectangles à coins droits sans
/// anti-aliasing). Utilisé par `app::run` pour positionner et capturer
/// l'arrière-plan flouté sous chacune (voir `gfx::backdrop`).
pub fn bar_rects(layout: &DockLayout) -> [[f32; 4]; 4] {
    // ⚠️ **LA BARRE DU BAS OCCUPE LE MILIEU, PAS LES COINS** — décision corrigée
    // par l'usage : l'utilisateur a vu le premier raccourci « décalé à gauche sur
    // la barre de gauche ». Les barres LATÉRALES gardent donc leur colonne sur
    // toute la hauteur (le dessin ET le découpage : voir `lateral_clip`), et le
    // bas vit entre elles, comme les dashs.
    //
    // ⛔ **CE N'ÉTAIT PAS MON PREMIER CHOIX** : j'avais fait prendre les coins au
    // bas, par symétrie avec le haut. C'était faux — la barre du haut prend les
    // coins parce que RIEN d'autre n'y vit ; les colonnes latérales, elles,
    // descendent jusqu'en bas à l'écran, et un raccourci posé dessous se lit
    // comme un défaut d'alignement.
    //
    // À `bottom_thickness == 0.0` (écran secondaire) la quatrième bande est
    // dégénérée et les trois autres valent exactement la géométrie d'avant.
    [
        [0.0, 0.0, layout.total_width, layout.top_thickness],
        [0.0, layout.haut_colonnes, layout.left_width, layout.total_height - layout.haut_colonnes],
        [
            layout.total_width - layout.right_width,
            layout.haut_colonnes,
            layout.right_width,
            layout.total_height - layout.haut_colonnes,
        ],
        [
            layout.left_width,
            layout.total_height - layout.bottom_thickness,
            bottom_band_width(layout),
            layout.bottom_thickness,
        ],
    ]
}

/// ⭐ **LES COINS ARRONDIS DE LA ZONE PRINCIPALE** — DEMANDE EXPLICITE
/// (2026-10-12) : « une option pour ajouter des coins arrondis qui suivent le
/// style sélectionné dans la zone principale de l'app ». La zone principale est
/// l'espace LIBRE que les barres encadrent ; ses coins deviennent des CONGÉS.
///
/// Un congé est un carré de côté `rayon` posé dans le coin de la zone libre,
/// MOINS le quart de disque de rayon `rayon` centré sur son coin intérieur
/// (`centre`). Il prolonge la barre `barre` (0 haut, 3 bas — l'index de
/// `bar_rects`) : il en prend la teinte et le flou.
///
/// ⚠️ **UN COIN N'EST ARRONDI QUE SI SES DEUX BARRES SONT LÀ** : un congé entre
/// une barre et le bord de l'écran ne raccorderait rien. Le même calcul sert au
/// DESSIN (épaisseurs du `DockLayout`) et à la RÉGION de la fenêtre (épaisseurs
/// VISIBLES, voir `win32::window::set_dock_region`) : une barre masquée retire
/// donc ses congés de l'écran, comme sa bande.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Conge {
    /// x, y, largeur, hauteur — un carré de côté `rayon`.
    pub rect: [f32; 4],
    /// Le coin du carré tourné vers la zone libre : (-1 | 1, -1 | 1), x puis y,
    /// `1` à droite / en bas.
    pub vers: [f32; 2],
    /// La barre que le congé prolonge (index de `bar_rects`).
    pub barre: usize,
}

/// Les congés de la zone libre d'un écran de `largeur` × `hauteur`, encadrée
/// par des barres d'épaisseurs `haut`, `gauche`, `droite`, `bas`. Le rayon est
/// borné à la demi-taille de la zone libre ; sous un pixel, aucun congé.
///
/// ⭐ **UN COIN SUIT SES BARRES VISIBLES** — DEMANDE EXPLICITE (2026-10-14) : « que
/// les coins arrondis de la zone d'affichage libre se déplacent ou disparaissent en
/// fonction du réglage de masquage des barres ». Il suffit d'UNE de ses deux
/// barres : la barre latérale masquée, le coin glisse au bord de l'écran sous la
/// barre du haut ; ses DEUX barres masquées, il disparaît. Les appelants passent
/// donc les épaisseurs VISIBLES.
///
/// ⭐ **UN COIN DU HAUT PROLONGE SA BARRE LATÉRALE** — DEMANDE EXPLICITE
/// (2026-10-16) : « lorsque le panneau du haut a une couleur différente des barres
/// latérales, la couleur des arrondis du haut qui longent les barres latérales
/// [doit être] celle de la barre latérale vers laquelle ils sont ». Le congé du
/// haut descend le long de la colonne : il en prend la teinte, et celle de la
/// barre du haut seulement sous une colonne masquée. Ceux du bas prolongent
/// toujours la barre du bas d'abord.
pub fn conges(largeur: f32, hauteur: f32, [haut, gauche, droite, bas]: [f32; 4], rayon: f32) -> Vec<Conge> {
    let rayon = rayon.min((largeur - gauche - droite) / 2.0).min((hauteur - haut - bas) / 2.0);
    if rayon < 1.0 {
        return Vec::new();
    }
    let (x_gauche, x_droite) = (gauche, largeur - droite - rayon);
    let (y_haut, y_bas) = (haut, hauteur - bas - rayon);
    // La barre que le coin prolonge : l'horizontale si elle est là, sinon la
    // latérale ; aucune des deux, pas de coin.
    let barre = |horizontale: f32, h: usize, laterale: f32, l: usize| {
        if horizontale > 0.0 {
            Some(h)
        } else if laterale > 0.0 {
            Some(l)
        } else {
            None
        }
    };
    // En haut, la latérale d'abord (voir la doc).
    let barre_du_haut = |laterale: f32, l: usize| if laterale > 0.0 { Some(l) } else { barre(haut, 0, 0.0, l) };
    [
        (barre_du_haut(gauche, 1), x_gauche, y_haut, [1.0, 1.0]),
        (barre_du_haut(droite, 2), x_droite, y_haut, [-1.0, 1.0]),
        (barre(bas, 3, gauche, 1), x_gauche, y_bas, [1.0, -1.0]),
        (barre(bas, 3, droite, 2), x_droite, y_bas, [-1.0, -1.0]),
    ]
    .into_iter()
    .filter_map(|(barre, x, y, vers)| Some(Conge { rect: [x, y, rayon, rayon], vers, barre: barre? }))
    .collect()
}

/// **LES BOUTS DE BARRE DU BAS** sous une colonne latérale MASQUÉE — DEMANDE
/// EXPLICITE (2026-10-14) : « quand une barre est masquée à droite ou à gauche de
/// la barre du bas, j'aimerais que l'espace créé soit remplacé par un morceau de
/// barre et que l'arrondi en tienne compte ». `visibles` : épaisseurs VISIBLES
/// `[haut, gauche, droite, bas]` ; rien si la barre du bas ne l'est pas.
///
/// ⚠️ **PEINTS PAR-DESSUS LE PIED DE LA COLONNE** : une colonne masquée reste
/// dessinée (seule la région la retire), icônes comprises — `app::run` pousse donc
/// ces bouts APRÈS tout le reste. L'arrondi, lui, vient de `conges`, qui pose déjà
/// le coin du bas au bord de l'écran quand la colonne est masquée.
pub fn bouts_de_barre_du_bas(layout: &DockLayout, [_, gauche, droite, bas]: [f32; 4]) -> Vec<[f32; 4]> {
    if bas <= 0.0 || layout.bottom_thickness <= 0.0 {
        return Vec::new();
    }
    let y = layout.total_height - layout.bottom_thickness;
    [
        (gauche <= 0.0 && layout.left_width > 0.0).then(|| [0.0, y, layout.left_width, layout.bottom_thickness]),
        (droite <= 0.0 && layout.right_width > 0.0)
            .then(|| [layout.total_width - layout.right_width, y, layout.right_width, layout.bottom_thickness]),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Le rectangle `rect` tombe-t-il dans une colonne latérale MASQUÉE (`visibles`
/// comme `bouts_de_barre_du_bas`) ? Un congé qui y tombe est peint, comme les
/// bouts, APRÈS le contenu de la colonne qu'il recouvre.
pub fn dans_une_colonne_masquee(rect: [f32; 4], layout: &DockLayout, [_, gauche, droite, _]: [f32; 4]) -> bool {
    (gauche <= 0.0 && rect[0] < layout.left_width) || (droite <= 0.0 && rect[0] + rect[2] > layout.total_width - layout.right_width)
}

/// L'option elle-même, relue par le dessin et la région à chaque frame — un
/// atomique, pas un fichier (voir `config::load_coins_zone_principale`, lu UNE
/// fois au démarrage).
static COINS_ZONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn coins_zone_actifs() -> bool {
    COINS_ZONE.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn activer_coins_zone(actifs: bool) {
    COINS_ZONE.store(actifs, std::sync::atomic::Ordering::Relaxed);
}

/// ⚠️ **LE DOUBLE DU RAYON DES FENÊTRES DU STYLE** : `rayon_grand` est pensé pour
/// une fenêtre (8 à 28 px) ; au coin d'un écran entier, le même rayon se lirait
/// à peine. Un style à coins droits (`rayon_grand = 0`) n'arrondit rien.
const FACTEUR_CONGE: f32 = 2.0;

/// Le rayon RÉGLÉ des congés, en pixels physiques, lu UNE fois au démarrage
/// (`app::run`, `config::load_coins_zone_rayon`). `SANS_REGLAGE` : personne ne
/// l'a réglé, le style décide — et il suit alors ses changements.
///
/// ⭐ Sans ce réglage, le rayon restait une constante de style que la conversion
/// du portable ne touchait pas : 56 px sur une VM dont la barre fait 18 px.
static RAYON_CONGE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(SANS_REGLAGE);
const SANS_REGLAGE: u32 = u32::MAX;

pub fn regler_rayon_conge(rayon: Option<f32>) {
    RAYON_CONGE.store(rayon.map(f32::to_bits).unwrap_or(SANS_REGLAGE), std::sync::atomic::Ordering::Relaxed);
}

/// Ce que le STYLE impose, sans aucun réglage — sert aussi à l'export du
/// portable (`config_initiale::exporter`), qui doit ÉCRIRE une valeur explicite
/// pour qu'elle soit convertie.
pub fn rayon_conge_du_style(style: &crate::ui::style::Style) -> f32 {
    f32::from(style.rayon_grand) * FACTEUR_CONGE
}

/// Le rayon des congés, en pixels physiques — zéro si l'option est éteinte.
pub fn rayon_conge() -> f32 {
    if !coins_zone_actifs() {
        return 0.0;
    }
    let regle = RAYON_CONGE.load(std::sync::atomic::Ordering::Relaxed);
    if regle == SANS_REGLAGE {
        crate::ui::style::rayon_grand() * FACTEUR_CONGE
    } else {
        f32::from_bits(regle)
    }
}

/// Le code du congé pour le shader des quads (voir `QuadInstance::new_conge`) :
/// `-1 - (x à droite) - 2 × (y en bas)`, soit -1 à -4.
pub fn code_conge(vers: [f32; 2]) -> f32 {
    -1.0 - f32::from(u8::from(vers[0] > 0.0)) - 2.0 * f32::from(u8::from(vers[1] > 0.0))
}

/// **LA LARGEUR DE LA BANDE DU BAS** — l'écran moins les deux colonnes latérales.
///
/// ⚠️ **DÉFINIE UNE SEULE FOIS, ET `bar_rects` S'EN SERT** : deux calculs de la
/// même bande finiraient par diverger, et cet écart-là ne se verrait qu'à l'écran.
/// Le dépôt a déjà cette règle pour les barres latérales (voir `lateral_bar_span`).
///
/// **C'est aussi ce que l'ÉLASTIQUE absorbe** (voir `layout_bottom_side`) : la
/// rangée du bas est la seule dont la largeur soit ainsi connue, et donc la seule
/// où « ce qui reste » ait un sens.
pub fn bottom_band_width(layout: &DockLayout) -> f32 {
    (layout.total_width - layout.left_width - layout.right_width).max(0.0)
}

/// Boîte d'un rectangle `rect` grossi par `scale`, AVANT tout recadrage.
///
/// Reproduit exactement le calcul du vertex shader (`gfx::quad_renderer::
/// VSMain` : `center ± rect.zw * scale / 2`, l'agrandissement se faisant
/// TOUJOURS autour du CENTRE du rectangle), mais côté CPU — parce que cette
/// géométrie doit désormais être connue de l'appelant (clic, ancrage des
/// popups, débordement de région), et plus seulement du GPU.
pub(crate) fn magnified_box(rect: [f32; 4], scale: f32) -> [f32; 4] {
    let (cx, cy) = (rect[0] + rect[2] * 0.5, rect[1] + rect[3] * 0.5);
    let (w, h) = (rect[2] * scale, rect[3] * scale);
    [cx - w * 0.5, cy - h * 0.5, w, h]
}

/// TRANSLATE `rect` du minimum nécessaire pour le faire tenir entièrement dans
/// `bounds` — **ne le redimensionne jamais**, et ne le RECENTRE pas : une boîte
/// plus grande que les bornes reste collée au bord haut/gauche, faute de mieux.
///
/// Même contrat que `app::menu_rect` (« ne recentre PAS le popup, le
/// replace juste entièrement à l'écran »), dont ceci est la version `f32` :
/// `ui::dock` ne peut pas dépendre d'`app`, la dépendance va dans l'autre sens.
pub(crate) fn clamp_by_translation(rect: [f32; 4], bounds: [f32; 4]) -> [f32; 4] {
    let [x, y, w, h] = rect;
    let [bx, by, bw, bh] = bounds;
    [x.min(bx + bw - w).max(bx), y.min(by + bh - h).max(by), w, h]
}

/// Rectangle à DESSINER pour une icône du DOCK. Point d'entrée de tous les appelants
/// qui ont un `DockLayout` sous la main (`Dock::presented_rect`, `render_instances`,
/// `magnified_boxes`, `topbar::presented_media_rect`).
///
/// ⛔ **AUCUN DÉCALAGE HORIZONTAL, DANS AUCUNE BARRE** — DEMANDE EXPLICITE du
/// 2026-09-24, **en deux temps** : « dans la partie droite de l'app les icones et les
/// applets qui sont au bord de l'écran se décalent à gauche, peux-tu enlever ce
/// déplacement », puis « dans la barre supérieure la dernière applet à droite qui
/// doit d'ailleurs rester à cet emplacement se décale aussi vers la gauche ».
///
/// La règle est donc UNIFORME, et elle est plus simple qu'une règle par barre : une
/// icône grossie reste **CENTRÉE sur sa case**, quitte à être coupée par le bord.
///
/// ⚠️ **LE DÉCALAGE VERTICAL RESTE, ET IL N'EST PAS COSMÉTIQUE** : c'est lui qui
/// empêche une icône de la barre du haut de disparaître par le haut — sa fenêtre part
/// de `y = 0`, donc rien ne peut la rattraper. Il ramène aussi une icône latérale
/// sortie par le bas.
///
/// ⚠️ **C'ÉTAIT UNE DEMANDE ANTÉRIEURE, À L'INVERSE** : « il faudrait que les icones
/// magnifiées se déplacent un peu vers le centre de l'écran pour ne pas être
/// tronquées ». L'utilisateur a tranché depuis, **en connaissant la conséquence** :
/// au bord, l'icône grossie sera TRONQUÉE par le bord de la fenêtre — `set_overflow`
/// ne peut pas étendre une région au-delà de sa fenêtre.
pub(crate) fn presented_rect_for(rect: [f32; 4], scale: f32, layout: &DockLayout) -> [f32; 4] {
    presented_rect_pour_la_barre(rect, scale, layout, BarreTirage::Haut)
}

/// **LA FORMULE, POUR UNE BARRE DONNÉE** — voir `presented_rect_for`, qui n'en est
/// que le cas de la barre du haut (son unique appelant hors du dock : les widgets de
/// la barre du haut, qui n'en connaissent pas d'autre).
pub(crate) fn presented_rect_pour_la_barre(rect: [f32; 4], scale: f32, layout: &DockLayout, barre: BarreTirage) -> [f32; 4] {
    if scale <= 1.0 {
        return rect;
    }
    let occupee = magnified_box(rect, scale * WOBBLE_HEADROOM);
    // La même boîte, RAMENÉE ENTIÈREMENT dans la fenêtre — c'est la position du
    // tirage à 1.0, et celle qu'aucun réglage ne dépasse.
    let bornee = clamp_by_translation(occupee, [0.0, 0.0, layout.total_width, layout.total_height]);
    // ⚠️ **ON INTERPOLE LA POSITION, JAMAIS LA TAILLE** : la boîte garde ses
    // dimensions, donc l'icône grandit pareil quel que soit le réglage — seule sa
    // place change. Interpoler la taille ferait qu'un tirage plus faible
    // rapetisserait l'icône, ce que personne n'a demandé.
    let t = tirage_vers_le_centre(barre);
    let melange = [occupee[0] + (bornee[0] - occupee[0]) * t, occupee[1] + (bornee[1] - occupee[1]) * t, occupee[2], occupee[3]];
    magnified_box(melange, 1.0 / WOBBLE_HEADROOM)
}

/// Rectangle à DESSINER pour un élément magnifié : sa boîte grossie, ramenée
/// dans `bounds` — c'est le « décalage vers le centre de l'écran » demandé,
/// obtenu SANS traitement par barre : ce qui dépasse est ramené du minimum, et
/// rien d'autre ne bouge. Sur les barres latérales le décalage est horizontal,
/// sur la barre du haut il est vertical (vers le bas) — même formule pour les
/// trois, parce que la contrainte est la même : tenir dans les bornes.
///
/// **Bornes EXPLICITES plutôt qu'un `DockLayout`** : c'est ce qui permet de
/// réutiliser la formule pour un élément magnifié qui ne vit PAS dans la
/// fenêtre du dock et n'a donc aucun `DockLayout` à fournir — aujourd'hui le
/// texte de l'horloge plein écran, qui a sa propre fenêtre (voir
/// `win32::fullscreen_clock`). Sans cette séparation, il devrait fabriquer un
/// faux `DockLayout` (un mensonge sur ses propres bornes) ou recopier la
/// formule — exactement la divergence que `topbar::presented_media_rect`
/// documente déjà comme à éviter.
#[cfg(test)]
pub(crate) fn presented_rect_in(rect: [f32; 4], scale: f32, bounds: [f32; 4]) -> [f32; 4] {
    // AU REPOS, LE RECTANGLE EST RENDU TEL QUEL — et c'est la seule formulation
    // cohérente avec une barre qui DÉFILE : un emplacement sorti de la fenêtre
    // doit le rester (c'est la fenêtre, puis `lateral_clip`, qui le coupent) au
    // lieu d'être TRANSLATÉ dedans.
    //
    // Le rappel à l'échelle 1.0 était sans effet tant que rien ne sortait de la
    // fenêtre : il datait d'avant le défilement, où cette fonction n'était
    // appelée que pour l'icône MAGNIFIÉE — cas pour lequel « ramener la boîte
    // grossie dans la fenêtre » est précisément la demande (voir plus bas), et
    // où il reste en vigueur. Depuis que la barre défile, il COLLAIT AU BORD BAS
    // toute icône sortie par le bas, par-dessus celle qui occupe légitimement
    // cette place — DÉFAUT RÉEL SIGNALÉ : « la dernière icône ne se cache pas
    // au défilement, elle reste affichée en bas de barre par-dessus le reste
    // alors que la barre est remplie d'icônes ». Les icônes d'applications
    // ouvertes, ajoutées en FIN de barre, sont exactement celles qui sortent par
    // le bas.
    //
    // Le clic ne change pas : `icon_at` teste la case BRUTE pour tout ce qui
    // n'est pas magnifié (voir sa doc), il ne consultait déjà pas ce rappel.
    if scale <= 1.0 {
        return rect;
    }
    // Une icône MAGNIFIÉE peut en plus PIVOTER (voir `hover_wobble_angle`) : on
    // fait donc tenir à l'écran sa boîte OCCUPÉE — élargie de `WOBBLE_HEADROOM`
    // pour contenir ses coins dans toutes les positions — puis on rend la boîte
    // de l'ICÔNE elle-même, qui en occupe le centre. Conséquence assumée : une
    // icône magnifiée ne colle plus tout à fait au bord de l'écran, elle laisse
    // la place que ses coins demanderont en tournant.
    let occupied = clamp_by_translation(magnified_box(rect, scale * WOBBLE_HEADROOM), bounds);
    magnified_box(occupied, 1.0 / WOBBLE_HEADROOM)
}

/// Amplitude du BALANCEMENT d'une icône magnifiée — DEMANDE EXPLICITE :
/// « faire tourner les icones grandifiées de 45° vers la droite et les gauches
/// de façon fluide tant que la souris survole une icone ». Un seul nombre à
/// changer si l'amplitude déplaît.
const HOVER_WOBBLE_DEGREES: f32 = 45.0;
/// Durée d'un aller-retour complet du balancement.
const HOVER_WOBBLE_PERIOD: std::time::Duration = std::time::Duration::from_millis(900);

/// Encombrement d'une icône qui PIVOTE : un carré tourné de 45° occupe
/// théoriquement jusqu'à `√2` fois sa place (le rapport exact, `cos θ + sin θ`
/// valant `√2` à 45°). Sert à la fois à faire tenir l'icône à l'écran dans
/// toutes ses positions (voir `presented_rect_for`) et à dire quelle surface la
/// région de la fenêtre doit couvrir (voir `occupied_rect_for`).
///
/// **Volontairement PLUS GRAND que le `√2` théorique** — DEMANDE EXPLICITE :
/// « peux tu agrandir la marge au bord de l'écran, certaines icones sont rognées
/// en tournant ». Le calcul exact dit qu'à 45° les coins touchent pile la boîte
/// décrite par `√2` ; l'utilisateur voyait pourtant des icônes rognées, et la
/// cause n'a pas été élucidée (ni arrondi entier, ni scission, ni région : tout
/// a été vérifié). Plutôt que de laisser un défaut visible pour un demi-pixel de
/// théorie, on prend ~40 % de marge en plus : `1.75` au lieu de `1.414`. Le coût
/// est une icône magnifiée un peu plus éloignée du bord de l'écran ; le
/// bénéfice est qu'aucun coin ne peut être coupé, quel que soit l'arrondi.
const WOBBLE_HEADROOM: f32 = 1.75;

/// Angle de balancement d'une icône magnifiée, en radians.
///
/// ⭐ **LA COULEUR DE L'INDICATEUR : DÉGRADÉ, SATURATION, LUMINOSITÉ,
/// TRANSPARENCE** — DEMANDE EXPLICITE (2026-10-14) : « indicateur application
/// ouverte du dock paramétrage couleur auto ou dégradé ou couleur perso,
/// saturation, luminosité, transparence ». Des valeurs GLOBALES, comme la marge
/// des icônes : un seul indicateur pour tout le dock.
///
/// Le DÉGRADÉ fait passer chaque indicateur par les couleurs de la palette
/// d'accent (`color::icon_palette`), en fondu, un tour en
/// `INDICATEUR_DEGRADE_MS` — décalé d'une icône à l'autre, si bien que la barre
/// montre un dégradé qui coule.
pub const INDICATEUR_DEGRADE_MS: f32 = 12_000.0;
/// Le décalage du dégradé d'une icône à la suivante, en fraction de tour.
const INDICATEUR_DEGRADE_DECALAGE: f32 = 0.08;

static INDICATEUR_DEGRADE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Saturation, luminosité (des FORCES : 1.0 = telle quelle, de 0 à 2) et
/// transparence (0 = opaque, 1 = invisible), en bits d'`f32`.
static INDICATEUR_REGLAGES: [std::sync::atomic::AtomicU32; 3] = [
    std::sync::atomic::AtomicU32::new(0x3F80_0000),
    std::sync::atomic::AtomicU32::new(0x3F80_0000),
    std::sync::atomic::AtomicU32::new(0),
];
/// Les bornes de `INDICATEUR_REGLAGES`, dans le même ordre.
pub const INDICATEUR_BORNES: [(f32, f32); 3] = [(0.0, 2.0), (0.0, 2.0), (0.0, 1.0)];

pub fn indicateur_degrade() -> bool {
    INDICATEUR_DEGRADE.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_indicateur_degrade(actif: bool) {
    INDICATEUR_DEGRADE.store(actif, std::sync::atomic::Ordering::Relaxed);
}

/// `[saturation, luminosité, transparence]`.
pub fn indicateur_reglages() -> [f32; 3] {
    INDICATEUR_REGLAGES.each_ref().map(|r| f32::from_bits(r.load(std::sync::atomic::Ordering::Relaxed)))
}

/// Pose le réglage `quel` (0 saturation, 1 luminosité, 2 transparence), borné.
pub fn set_indicateur_reglage(quel: usize, valeur: f32) {
    let (Some(case), Some(&(min, max))) = (INDICATEUR_REGLAGES.get(quel), INDICATEUR_BORNES.get(quel)) else { return };
    let defaut = if quel == 2 { 0.0 } else { 1.0 };
    let v = if valeur.is_finite() { valeur.clamp(min, max) } else { defaut };
    case.store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Saturation et luminosité du dégradé quand le thème n'a PAS d'accent (un thème
/// hors famille) : celles d'un accent vif typique.
const DEGRADE_SANS_ACCENT: (f32, f32) = (0.75, 0.6);

/// **LA COULEUR DU DÉGRADÉ** : la TEINTE de `teinte`, la saturation et la
/// luminosité de l'accent du thème — pour briller autant que l'indicateur
/// automatique. ⛔ Sans accent, elles NE viennent PAS du fond : un fond sombre
/// (luminosité ~0,09) donnait des pastilles presque noires, « plus en couleurs »
/// (défaut signalé le 2026-10-15, après le choix d'un thème hors famille).
pub fn couleur_du_degrade(teinte: [f32; 3], accent_du_theme: Option<[f32; 3]>) -> [f32; 4] {
    let (h, _, _) = color::rgb_to_hsl(teinte);
    let (s, l) = match accent_du_theme {
        Some(accent) => {
            let (_, s, l) = color::rgb_to_hsl(accent);
            (s, l)
        }
        None => DEGRADE_SANS_ACCENT,
    };
    let [r, g, b] = color::hsl_to_rgb(h, s, l);
    [r, g, b, 1.0]
}

/// La couleur du dégradé à `secondes` pour l'icône `index` — PURE, testable.
/// Fondu ADOUCI d'une couleur de la palette à la suivante.
pub fn couleur_degradee_at(secondes: f32, index: usize, palette: &[[f32; 4]]) -> [f32; 3] {
    let Some(premiere) = palette.first() else { return [1.0, 1.0, 1.0] };
    if palette.len() == 1 {
        return [premiere[0], premiere[1], premiere[2]];
    }
    let n = palette.len();
    let tour = (secondes * 1000.0 / INDICATEUR_DEGRADE_MS + index as f32 * INDICATEUR_DEGRADE_DECALAGE).rem_euclid(1.0);
    let u = tour * n as f32;
    let k = (u as usize).min(n - 1);
    let f = u - k as f32;
    let f = f * f * (3.0 - 2.0 * f);
    let (a, b) = (palette[k], palette[(k + 1) % n]);
    [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f]
}

/// Applique saturation, luminosité et transparence — PURE. `[1, 1, 0]` rend la
/// couleur telle quelle. La luminosité est une FORCE : sous 1 elle assombrit
/// vers le noir, au-dessus elle éclaircit vers le blanc.
pub fn ajuster_indicateur(couleur: [f32; 4], [saturation, luminosite, transparence]: [f32; 3]) -> [f32; 4] {
    let (h, s, l) = color::rgb_to_hsl([couleur[0], couleur[1], couleur[2]]);
    let s = (s * saturation).clamp(0.0, 1.0);
    let l = if luminosite <= 1.0 { l * luminosite } else { l + (1.0 - l) * (luminosite - 1.0) }.clamp(0.0, 1.0);
    let [r, g, b] = color::hsl_to_rgb(h, s, l);
    [r, g, b, couleur[3] * (1.0 - transparence).clamp(0.0, 1.0)]
}

/// Couleur de l'indicateur « en cours d'exécution » : le clignotement de
/// notification, sinon la couleur choisie dans les Réglages, sinon l'accent de
/// la famille de thèmes (voir `color::accent_du_theme`), sinon le fond du dock
/// — le comportement d'origine, qui se confond avec la barre sous une famille.
pub fn couleur_indicateur(
    clignotement: Option<[f32; 4]>,
    choisie: Option<[f32; 4]>,
    accent_du_theme: Option<[f32; 3]>,
    fond: [f32; 4],
) -> [f32; 4] {
    clignotement.or(choisie).or_else(|| accent_du_theme.map(|[r, g, b]| [r, g, b, 1.0])).unwrap_or(fond)
}

/// **Fonction PURE du temps** — même choix, et même raison, que le clignotement
/// de notification (voir `notification_blink_color_at`) : deux icônes qui se
/// balanceraient avec leur propre phase se liraient comme un défaut.
///
/// **Pondérée par l'avancement du survol** (`scale`, voir `HoverAnimator`) :
/// nulle au repos, maximale à pleine magnification. L'icône se met donc à
/// balancer en grandissant et s'arrête en rétrécissant, au lieu de démarrer et
/// de stopper d'un coup.
pub fn hover_wobble_angle(scale: f32) -> f32 {
    hover_wobble_angle_at(hover_wobble_elapsed(), scale, HOVER_WOBBLE_DEGREES, hover_scale())
}

/// Instant de départ PARTAGÉ par tous les balancements — icônes comme textes.
/// Une seule horloge veut dire une seule PHASE : deux éléments qui se
/// balanceraient chacun dans leur coin se liraient comme un défaut (même
/// raison, et même choix, que le clignotement de notification ci-dessous).
fn hover_wobble_elapsed() -> std::time::Duration {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed()
}

/// Angle de balancement à un instant DONNÉ — la forme testable de
/// `hover_wobble_angle`, découpée exactement comme la paire
/// `notification_blink_color_at`/`notification_blink_color` et pour la même
/// raison : une fonction qui lit l'horloge en interne ne se teste qu'en
/// bornage, jamais sur sa forme.
///
/// `degrees` porte l'amplitude ; `full` est la magnification RÉGLÉE **de
/// l'élément considéré**, c'est elle qui donne `progress`, donc l'amplitude
/// relative en cours de croissance.
///
/// **`full` est un PARAMÈTRE, et ce n'est pas cosmétique** : icônes et horloges
/// ont désormais deux réglages distincts (`hover_scale` / `clock_hover_scale`).
/// Relire le global des icônes ici ferait SATURER `progress` à `1.0` dès ~40 %
/// de la croissance d'une horloge réglée plus grande qu'eux — le balancement
/// prendrait toute son amplitude d'un coup, puis n'évoluerait plus — et,
/// inversement, une horloge réglée plus petite n'atteindrait jamais la sienne.
pub fn hover_wobble_angle_at(
    elapsed: std::time::Duration,
    scale: f32,
    degrees: f32,
    full: f32,
) -> f32 {
    // `!(full > 1.0)` plutôt que `full <= 1.0` : écarte aussi le NaN, qui
    // rendrait `progress` NaN et l'angle avec. L'infini, lui, passe et donne
    // `progress = 0` — inoffensif.
    if !(full > 1.0) || scale <= 1.0 {
        return 0.0;
    }
    let turns = elapsed.as_secs_f32() / HOVER_WOBBLE_PERIOD.as_secs_f32();
    let progress = ((scale - 1.0) / (full - 1.0)).clamp(0.0, 1.0);
    degrees.to_radians() * progress * (turns * std::f32::consts::TAU).sin()
}

/// Amplitude du balancement d'un **TEXTE** magnifié (les horloges) — distincte
/// de `HOVER_WOBBLE_DEGREES`, qui est calibrée pour des icônes carrées.
///
/// **Volontairement BEAUCOUP plus faible, et c'est un choix, pas un oubli** :
/// à 45°, des chiffres qui pivotent sont illisibles et se lisent comme un
/// défaut, alors que l'analogie « le texte est l'icône » porte sur le
/// GROSSISSEMENT, pas sur l'amplitude. La période reste commune, donc la phase
/// reste partagée avec les icônes (voir `hover_wobble_elapsed`).
const TEXT_WOBBLE_DEGREES: f32 = 6.0;

/// Marge de rotation d'un **TEXTE** magnifié — le pendant de `WOBBLE_HEADROOM`
/// pour les horloges, calibré sur `TEXT_WOBBLE_DEGREES` et non sur les ±45° des
/// icônes.
///
/// Un carré de côté `s` penché de `θ` occupe `s · (cos θ + sin θ)` : à 6° cela
/// vaut `1,099`, d'où `1.20` — arrondi au-dessus, jamais en dessous, parce que
/// cette constante est un MAJORANT (une boîte trop petite rognerait le texte).
///
/// Elle remplace `WOBBLE_HEADROOM` (1,75) pour tout ce qui touche les horloges,
/// et c'est elle qui rend le plafond de 350 % gratuit : la fenêtre de l'horloge
/// plein écran est pré-dimensionnée sur
/// `MAX_CLOCK_HOVER_SCALE · TEXT_WOBBLE_HEADROOM` (voir
/// `max_clock_magnified_growth`) et reste donc PLUS PETITE qu'avant, alors que
/// le texte peut désormais grossir bien davantage.
const TEXT_WOBBLE_HEADROOM: f32 = 1.20;

/// Angle de balancement d'un texte magnifié — voir `TEXT_WOBBLE_DEGREES`.
///
/// Le dénominateur de `progress` est le réglage des HORLOGES
/// (`clock_hover_scale`), pas celui des icônes : c'est ce qui fait que le texte
/// atteint sa pleine amplitude exactement quand il a fini de grandir, quel que
/// soit le facteur choisi pour lui.
pub fn hover_text_wobble_angle(scale: f32) -> f32 {
    hover_wobble_angle_at(
        hover_wobble_elapsed(),
        scale,
        TEXT_WOBBLE_DEGREES,
        clock_hover_scale(),
    )
}

/// Boîte OCCUPÉE par une icône magnifiée : la sienne, élargie de ce qu'il faut
/// pour qu'elle puisse PIVOTER sans être rognée.
///
/// C'est CETTE boîte que la région de la fenêtre doit couvrir (voir
/// `magnified_boxes`), et que `presented_rect_for` fait tenir à l'écran — sinon
/// les coins d'une icône penchée sortiraient de la région et seraient coupés,
/// exactement le défaut que le débordement a été écrit pour corriger.
///
/// **`WOBBLE_HEADROOM` est calibré pour les ICÔNES (±45°)** — c'est la seule
/// famille d'éléments qui l'utilise : le TEXTE des horloges a la sienne
/// (`TEXT_WOBBLE_HEADROOM`, voir `occupied_text_rect_for`), calibrée sur les ±6°
/// auxquels il balance. Un seul `headroom` pour les deux aurait été un majorant
/// deux fois trop large pour le texte, et il se paie deux fois (fenêtre plus
/// grande pour l'horloge plein écran, zone de débordement du dock plus large,
/// donc qui avale des clics au-delà du nécessaire pendant le survol).
pub(crate) fn occupied_rect_for(presented: [f32; 4]) -> [f32; 4] {
    magnified_box(presented, WOBBLE_HEADROOM)
}

/// Boîte OCCUPÉE par un TEXTE magnifié (les deux horloges) — le pendant de
/// `occupied_rect_for` pour elles, élargi de `TEXT_WOBBLE_HEADROOM` et non de
/// `WOBBLE_HEADROOM`.
///
/// **La boîte dessinée et la boîte de région doivent dériver de la MÊME
/// constante** : c'est un majorant, et une région plus petite que le dessin
/// rogne le texte (voir la note « pixels noirs » de `magnified_boxes`).
pub(crate) fn occupied_text_rect_for(presented: [f32; 4]) -> [f32; 4] {
    magnified_box(presented, TEXT_WOBBLE_HEADROOM)
}

/// Le même majorant, pour le TEXTE des horloges : le plafond de LEUR réglage
/// (`MAX_CLOCK_HOVER_SCALE`, 350 %) multiplié par ce que ce texte occupe en
/// balançant à ±6° (`TEXT_WOBBLE_HEADROOM`).
///
/// **C'est le majorant dont dépend la fenêtre de l'horloge plein écran** — elle
/// est pré-dimensionnée une fois pour toutes avec (voir `win32::fullscreen_clock::
/// overscan_padding`), ce qui lui évite tout `SetWindowPos`/`ResizeBuffers`
/// pendant l'animation. Corollaire à ne pas perdre : le plafond du réglage ne
/// peut PAS être relevé sans que cette fenêtre suive, puisque `show` est à la
/// demande et que ce module ne redimensionne jamais une fenêtre vivante.
///
/// À 350 % × 1,20 = 4,20, contre 250 % × 1,75 = 4,375 pour les icônes : le
/// texte peut donc grossir nettement plus qu'avant dans une fenêtre légèrement
/// PLUS PETITE qu'avant.
pub fn max_clock_magnified_growth() -> f32 {
    MAX_CLOCK_HOVER_SCALE * TEXT_WOBBLE_HEADROOM
}

/// Boîte d'un élément magnifié qui **ne doit pas se déplacer** : grossissement
/// pur autour du centre, sans aucun rappel dans des bornes — DEMANDE EXPLICITE :
/// « que l'effet de survol des horloges ne fasse pas se déplacer le texte, il
/// faut juste qu'il s'agrandisse et oscille ».
///
/// `presented_rect_in` fait en plus TENIR la boîte dans des bornes, ce qui la
/// **translate** quand elle en dépasse — et c'est ce déplacement que le texte
/// d'une horloge ne doit pas subir. Ce rappel garde un intérêt pour une ICÔNE
/// (elle ne doit pas sortir de l'écran), aucun pour un texte centré dont on veut
/// un grossissement symétrique.
///
/// Contrepartie assumée : au bord de l'écran, un COIN peut sortir de la fenêtre
/// pendant le balancement, donc être coupé — quelques pixels, le temps d'un
/// extrême de l'oscillation. C'est le prix du « ne pas se déplacer ».
pub(crate) fn magnified_box_centred(rect: [f32; 4], scale: f32) -> [f32; 4] {
    if scale <= 1.0 {
        rect
    } else {
        magnified_box(rect, scale)
    }
}

/// `true` si `(x, y)` tombe dans `rect` (`[x, y, largeur, hauteur]`).
/// Partagé avec `ui::topbar` pour le bouton d'alimentation.
pub(crate) fn hit(rect: &[f32; 4], x: f32, y: f32) -> bool {
    x >= rect[0] && x <= rect[0] + rect[2] && y >= rect[1] && y <= rect[1] + rect[3]
}

/// Couleurs du clignotement de l'indicateur d'une application qui a envoyé une
/// notification — DEMANDE EXPLICITE : « il faudrait aussi que l'indicateur
/// clignote de plusieurs couleurs si l'app envoit une notification ».
///
/// Quatre teintes très contrastées entre elles, plutôt qu'un dégradé de
/// l'accentuation : le but est d'être remarqué du coin de l'œil, et elles
/// doivent rester distinctes sur n'importe quelle couleur de barre.
const NOTIFICATION_BLINK_COLORS: [[f32; 4]; 4] = [
    [1.00, 0.80, 0.10, 1.0],
    [1.00, 0.25, 0.25, 1.0],
    [0.25, 0.80, 1.00, 1.0],
    [0.45, 1.00, 0.45, 1.0],
];

/// Durée pendant laquelle chaque couleur reste affichée avant la suivante.
const NOTIFICATION_BLINK_PERIOD: std::time::Duration = std::time::Duration::from_millis(450);

/// Couleur du clignotement à un instant donné de la vie du processus.
///
/// **Fonction PURE du temps écoulé**, et non un état par notification : deux
/// notifications arrivées à trois cents millisecondes d'écart doivent clignoter
/// ENSEMBLE — chacune avec sa propre phase, le décalage se lirait comme un
/// défaut. C'est aussi ce qui la rend testable sans rien dessiner.
pub fn notification_blink_color_at(elapsed: std::time::Duration) -> [f32; 4] {
    let period = NOTIFICATION_BLINK_PERIOD.as_millis();
    let step = (elapsed.as_millis() / period) as usize;
    NOTIFICATION_BLINK_COLORS[step % NOTIFICATION_BLINK_COLORS.len()]
}

/// Couleur du clignotement MAINTENANT — voir `notification_blink_color_at`.
pub fn notification_blink_color() -> [f32; 4] {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    notification_blink_color_at(START.get_or_init(std::time::Instant::now).elapsed())
}

// ─────────────────────────────────────────────────────────────────────────────
// FOND ANIMÉ DU LOGO — DEMANDE EXPLICITE : « un fond changeant de couleur,
// réglable dans le dash réglage, et passant de 100 % d'opacité à 100 % de
// transparence sur un temps réglable ».
//
// **RIEN DE NOUVEAU N'EST DESSINÉ** : c'est le fond que le logo porte DÉJÀ quand
// `dash_accent.icon_bg` est actif (voir `icon_zone_instance` et son appel dans
// `app::run`) dont la couleur et l'alpha varient. Deux choses n'y avaient jamais
// été exercées — une couleur qui change, et un alpha < 1.
//
// **LES COULEURS VIENNENT DE LA PALETTE DE L'UTILISATEUR**, pas d'un tour de la
// roue chromatique : `color::icon_palette` dérive déjà six teintes de l'accent,
// à 60° d'écart, et elles suivent donc son réglage d'accent.
// ─────────────────────────────────────────────────────────────────────────────

/// Durée d'un aller de l'opacité (100 % → 0 %), en millisecondes.
pub const DEFAULT_LOGO_FADE_MS: f32 = 1500.0;
/// Bornes du fondu. **La borne basse n'est PAS zéro** : à zéro l'onde n'aurait
/// plus de forme, et « animation active au fondu instantané » n'est pas une
/// demande — c'est la case d'activation qui éteint l'animation, pas un curseur
/// à zéro (contraste assumé avec `fade::MIN_FADE_MS`, où zéro veut dire « pas de
/// fondu » parce qu'il y a un état statique à retrouver).
pub const MIN_LOGO_FADE_MS: f32 = 200.0;
pub const MAX_LOGO_FADE_MS: f32 = 10_000.0;
/// Durée pendant laquelle chaque couleur reste affichée avant la suivante.
pub const DEFAULT_LOGO_COLOR_MS: f32 = 2000.0;
pub const MIN_LOGO_COLOR_MS: f32 = 200.0;
pub const MAX_LOGO_COLOR_MS: f32 = 20_000.0;

// Valeurs courantes — même mécanisme que `HOVER_SCALE` : la cellule EST le
// stockage, relue à chaque frame, donc le curseur des Réglages agit à chaud sans
// que `SettingsRefs` ait à porter quoi que ce soit.
thread_local! {
    static LOGO_ANIMATION_ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static LOGO_FADE_MS: std::cell::Cell<f32> = const { std::cell::Cell::new(DEFAULT_LOGO_FADE_MS) };
    static LOGO_COLOR_MS: std::cell::Cell<f32> = const { std::cell::Cell::new(DEFAULT_LOGO_COLOR_MS) };
}

/// Vrai si le fond du logo doit être animé — **DÉSACTIVÉ PAR DÉFAUT**.
///
/// Même choix que l'animation des notifications, et pour la même raison : une
/// animation permanente sur le dock est une décision de l'utilisateur, pas un
/// effet qu'on lui impose au premier lancement.
pub fn logo_animation_enabled() -> bool {
    LOGO_ANIMATION_ENABLED.with(std::cell::Cell::get)
}

pub fn set_logo_animation_enabled(enabled: bool) {
    LOGO_ANIMATION_ENABLED.with(|cell| cell.set(enabled));
}

pub fn logo_fade_ms() -> f32 {
    LOGO_FADE_MS.with(std::cell::Cell::get)
}

/// Change la durée du fondu, bornée — et **rejette `NaN`**, comme
/// `set_hover_scale` : `f32::clamp` PROPAGE `NaN` au lieu de le borner, et un
/// alpha `NaN` ferait disparaître le fond sans rien dire.
pub fn set_logo_fade_ms(ms: f32) {
    if !ms.is_nan() {
        LOGO_FADE_MS.with(|cell| cell.set(ms.clamp(MIN_LOGO_FADE_MS, MAX_LOGO_FADE_MS)));
    }
}

pub fn logo_color_ms() -> f32 {
    LOGO_COLOR_MS.with(std::cell::Cell::get)
}

/// Change la durée par couleur, bornée et rejetant `NaN` — voir `set_logo_fade_ms`.
pub fn set_logo_color_ms(ms: f32) {
    if !ms.is_nan() {
        LOGO_COLOR_MS.with(|cell| cell.set(ms.clamp(MIN_LOGO_COLOR_MS, MAX_LOGO_COLOR_MS)));
    }
}

/// Bornes des deux durées — **SEULE AUTORITÉ**, poussée aux Réglages : la page
/// n'en recopie aucune (même règle que `ui::dock::bar_thickness_bounds`, et même
/// forme de retour groupé).
pub fn logo_animation_bounds() -> ((f32, f32), (f32, f32)) {
    ((MIN_LOGO_FADE_MS, MAX_LOGO_FADE_MS), (MIN_LOGO_COLOR_MS, MAX_LOGO_COLOR_MS))
}

/// L'instant de départ de l'animation du fond du logo — **PARTAGÉ par la couleur
/// et l'opacité**, qui sont deux facettes du même effet : deux horloges
/// distinctes donneraient deux phases qui se décalent, et le décalage se lit
/// comme un défaut (même raisonnement qu'au balancement et au clignotement).
fn logo_animation_elapsed() -> std::time::Duration {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed()
}

/// L'**opacité** du fond du logo à un instant donné — une **onde triangulaire
/// continue** de période `2 × fade_ms` : elle part de 1.0 (pleinement opaque),
/// descend à 0.0, et remonte. **Aucun palier aux extrémités** — DEMANDE
/// EXPLICITE : « mouvement continu ».
///
/// **Fonction PURE du temps écoulé**, comme `notification_blink_color_at` : même
/// instant, même opacité — et c'est aussi ce qui la rend testable sans rien
/// dessiner.
pub fn logo_background_alpha_at(elapsed: std::time::Duration, fade_ms: f32) -> f32 {
    if !(fade_ms > 0.0) {
        // `!(x > 0.0)` plutôt que `x <= 0.0` — rejette AUSSI `NaN`, exactement
        // comme `hover_wobble_angle_at` (voir sa doc). Sans forme, on rend l'état
        // le plus neutre : pleinement opaque.
        return 1.0;
    }
    let phase = (elapsed.as_secs_f32() * 1000.0 / fade_ms).rem_euclid(2.0);
    if phase <= 1.0 {
        1.0 - phase
    } else {
        phase - 1.0
    }
}

/// La **couleur** du fond du logo à un instant donné, prise dans `palette` — un
/// pas DISCRET par couleur, sans interpolation : « quelques couleurs de la
/// palette », pas un dégradé (DEMANDE EXPLICITE), et c'est la forme du
/// clignotement de notification.
///
/// Une `palette` vide rend le blanc, faute de mieux : l'appelant ne devrait
/// jamais en passer une, mais une couleur unie vaut mieux qu'un `panic` dans la
/// boucle de rendu.
/// La palette est prise telle que `color::icon_palette` la rend — des `[f32;4]`
/// — et seul son RVB est utilisé : l'ALPHA du fond vient de l'onde d'opacité,
/// jamais de la palette, sans quoi les deux animations se disputeraient le même
/// canal.
pub fn logo_background_color_at(elapsed: std::time::Duration, color_ms: f32, palette: &[[f32; 4]]) -> [f32; 3] {
    let Some(first) = palette.first() else { return [1.0, 1.0, 1.0] };
    let rgb = |c: &[f32; 4]| [c[0], c[1], c[2]];
    if !(color_ms > 0.0) {
        return rgb(first);
    }
    let step = (elapsed.as_secs_f32() * 1000.0 / color_ms) as usize;
    rgb(&palette[step % palette.len()])
}

/// L'opacité du fond MAINTENANT — voir `logo_background_alpha_at`.
pub fn logo_background_alpha() -> f32 {
    logo_background_alpha_at(logo_animation_elapsed(), logo_fade_ms())
}

/// La couleur du fond MAINTENANT — voir `logo_background_color_at`.
pub fn logo_background_color(palette: &[[f32; 4]]) -> [f32; 3] {
    logo_background_color_at(logo_animation_elapsed(), logo_color_ms(), palette)
}

// ─────────────────────────────────────────────────────────────────────────────
// ICÔNE CIRCULAIRE QUI TOURNE — DEMANDE EXPLICITE : « avoir le choix entre
// l'icône carré avec un fond respirant et l'icône cercle qui tourne », avec le
// SENS et la VITESSE réglables.
//
// **LE FOND ANIMÉ VAUT POUR LES DEUX** (demande explicite du 2026-09-26, qui
// lève l'exclusivité d'avant) : un disque sous le cercle qui tourne, voir
// `icon_halo_for`. Seul le fond FIXE (`icon_bg`) reste réservé au carré.
// ─────────────────────────────────────────────────────────────────────────────

/// Durée d'un tour complet, en millisecondes.
pub const DEFAULT_LOGO_TURN_MS: f32 = 8000.0;
/// Bornes. Le plancher d'une seconde garde la rotation lisible plutôt que
/// stroboscopique ; le plafond d'une minute la rend presque immobile.
pub const MIN_LOGO_TURN_MS: f32 = 1000.0;
pub const MAX_LOGO_TURN_MS: f32 = 60_000.0;

thread_local! {
    static LOGO_IS_CIRCLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static LOGO_SPIN_CLOCKWISE: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    static LOGO_TURN_MS: std::cell::Cell<f32> = const { std::cell::Cell::new(DEFAULT_LOGO_TURN_MS) };
}

/// Vrai si le dock doit montrer l'icône **circulaire** (qui tourne) plutôt que
/// la carrée (qui porte le fond respirant). **FAUX PAR DÉFAUT** : le carré est
/// l'icône historique, et on ne change pas l'apparence de quelqu'un sans qu'il
/// le demande.
pub fn logo_is_circle() -> bool {
    LOGO_IS_CIRCLE.with(std::cell::Cell::get)
}

pub fn set_logo_is_circle(circle: bool) {
    LOGO_IS_CIRCLE.with(|cell| cell.set(circle));
}

/// Sens de rotation — `true` = horaire. Sans objet pour l'icône carrée.
pub fn logo_spin_clockwise() -> bool {
    LOGO_SPIN_CLOCKWISE.with(std::cell::Cell::get)
}

pub fn set_logo_spin_clockwise(clockwise: bool) {
    LOGO_SPIN_CLOCKWISE.with(|cell| cell.set(clockwise));
}

pub fn logo_turn_ms() -> f32 {
    LOGO_TURN_MS.with(std::cell::Cell::get)
}

/// Change la durée d'un tour, bornée et rejetant `NaN` — même garde que les deux
/// durées du fond (voir `set_logo_fade_ms`).
pub fn set_logo_turn_ms(ms: f32) {
    if !ms.is_nan() {
        LOGO_TURN_MS.with(|cell| cell.set(ms.clamp(MIN_LOGO_TURN_MS, MAX_LOGO_TURN_MS)));
    }
}

/// Bornes de la durée d'un tour — **SEULE AUTORITÉ**, poussée aux Réglages.
pub fn logo_turn_bounds() -> (f32, f32) {
    (MIN_LOGO_TURN_MS, MAX_LOGO_TURN_MS)
}

/// L'**ANGLE** de rotation de l'icône circulaire, en RADIANS, à un instant donné.
///
/// **Fonction PURE du temps écoulé**, comme l'opacité et la couleur du fond : la
/// rotation est linéaire en temps, donc « tourner indéfiniment » ne coûte AUCUN
/// état — rien à accumuler par frame, donc rien qui puisse dériver.
///
/// ⚠️ **`rem_euclid` N'EST PAS COSMÉTIQUE** : `QuadInstance::rotation` part au
/// shader, qui en prend le `sin`/`cos`. Un angle qui croît sans borne perd sa
/// précision au bout de quelques heures (`f32`), et la rotation se mettrait à
/// saccader — un défaut qu'on ne verrait qu'après une longue session, donc
/// jamais pendant la mise au point.
///
/// ⚠️ **CHANGER LE SENS OU LA VITESSE FAIT SAUTER L'ANGLE**, et c'est inhérent à
/// une rotation définie comme fonction du temps écoulé : la position dépend de
/// l'instant et du réglage, donc changer le réglage déplace l'icône d'un coup.
/// C'est le prix de n'avoir aucun état ; un accumulateur l'éviterait au prix
/// d'une dérive et d'un champ à traîner.
pub fn logo_rotation_at(elapsed: std::time::Duration, turn_ms: f32, clockwise: bool) -> f32 {
    if !(turn_ms > 0.0) {
        // `!(x > 0.0)` rejette aussi `NaN` — même idiome que
        // `logo_background_alpha_at`.
        return 0.0;
    }
    let sign = if clockwise { 1.0 } else { -1.0 };
    let turns = elapsed.as_secs_f32() * 1000.0 / turn_ms;
    (sign * turns * std::f32::consts::TAU).rem_euclid(std::f32::consts::TAU)
}

/// L'angle de rotation MAINTENANT — voir `logo_rotation_at`.
pub fn logo_rotation() -> f32 {
    logo_rotation_at(logo_animation_elapsed(), logo_turn_ms(), logo_spin_clockwise())
}

// ─────────────────────────────────────────────────────────────────────────────
// TAILLE ET ZOOM DU LOGO — ⭐ DEMANDE EXPLICITE (2026-10-15) : « la taille de
// l'icône du logo modifiable » et « un effet de zoom centré temporisé
// paramétrable ».
//
// **SEUL LE DESSIN CHANGE** : la case du logo (survol, clic, place dans la
// barre) reste celle de `icon_rect`. Le rectangle DESSINÉ est mis à l'échelle
// autour de son centre (`scale_rect_centered`), dans les DEUX chemins de dessin
// (`app::run` et `dock_instance`), comme la rotation. Au-delà de 100 %, le logo
// déborde de sa case — c'est le choix de l'utilisateur.
// ─────────────────────────────────────────────────────────────────────────────

/// Taille du logo, en fraction de sa case (1.0 = 100 %).
pub const MIN_LOGO_SIZE: f32 = 0.4;
pub const MAX_LOGO_SIZE: f32 = 1.5;
/// Grossissement au sommet du zoom, en fraction de la taille (0.25 = +25 %).
pub const DEFAULT_LOGO_ZOOM_AMPLITUDE: f32 = 0.25;
pub const MIN_LOGO_ZOOM_AMPLITUDE: f32 = 0.05;
pub const MAX_LOGO_ZOOM_AMPLITUDE: f32 = 1.0;
/// Durée d'un zoom, aller ET retour.
pub const DEFAULT_LOGO_ZOOM_MS: f32 = 800.0;
pub const MIN_LOGO_ZOOM_MS: f32 = 200.0;
pub const MAX_LOGO_ZOOM_MS: f32 = 5000.0;
/// Attente à taille normale entre deux zooms.
pub const DEFAULT_LOGO_ZOOM_INTERVAL_MS: f32 = 10_000.0;
pub const MIN_LOGO_ZOOM_INTERVAL_MS: f32 = 500.0;
pub const MAX_LOGO_ZOOM_INTERVAL_MS: f32 = 300_000.0;

thread_local! {
    static LOGO_SIZE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
    static LOGO_ZOOM_ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static LOGO_ZOOM_AMPLITUDE: std::cell::Cell<f32> = const { std::cell::Cell::new(DEFAULT_LOGO_ZOOM_AMPLITUDE) };
    static LOGO_ZOOM_MS: std::cell::Cell<f32> = const { std::cell::Cell::new(DEFAULT_LOGO_ZOOM_MS) };
    static LOGO_ZOOM_INTERVAL_MS: std::cell::Cell<f32> = const { std::cell::Cell::new(DEFAULT_LOGO_ZOOM_INTERVAL_MS) };
}

/// Borne `value` et rejette `NaN` (garde `current`) — même garde que
/// `set_logo_fade_ms` : `f32::clamp` PROPAGE `NaN`.
fn borne_ou_garde(value: f32, current: f32, min: f32, max: f32) -> f32 {
    if value.is_nan() { current } else { value.clamp(min, max) }
}

pub fn logo_size() -> f32 {
    LOGO_SIZE.with(std::cell::Cell::get)
}

pub fn set_logo_size(size: f32) {
    LOGO_SIZE.with(|cell| cell.set(borne_ou_garde(size, cell.get(), MIN_LOGO_SIZE, MAX_LOGO_SIZE)));
}

/// Vrai si le zoom périodique est actif — **DÉSACTIVÉ PAR DÉFAUT**, même
/// raison que `logo_animation_enabled`.
pub fn logo_zoom_enabled() -> bool {
    LOGO_ZOOM_ENABLED.with(std::cell::Cell::get)
}

pub fn set_logo_zoom_enabled(enabled: bool) {
    LOGO_ZOOM_ENABLED.with(|cell| cell.set(enabled));
}

pub fn logo_zoom_amplitude() -> f32 {
    LOGO_ZOOM_AMPLITUDE.with(std::cell::Cell::get)
}

pub fn set_logo_zoom_amplitude(amplitude: f32) {
    LOGO_ZOOM_AMPLITUDE.with(|cell| {
        cell.set(borne_ou_garde(amplitude, cell.get(), MIN_LOGO_ZOOM_AMPLITUDE, MAX_LOGO_ZOOM_AMPLITUDE))
    });
}

pub fn logo_zoom_ms() -> f32 {
    LOGO_ZOOM_MS.with(std::cell::Cell::get)
}

pub fn set_logo_zoom_ms(ms: f32) {
    LOGO_ZOOM_MS.with(|cell| cell.set(borne_ou_garde(ms, cell.get(), MIN_LOGO_ZOOM_MS, MAX_LOGO_ZOOM_MS)));
}

pub fn logo_zoom_interval_ms() -> f32 {
    LOGO_ZOOM_INTERVAL_MS.with(std::cell::Cell::get)
}

pub fn set_logo_zoom_interval_ms(ms: f32) {
    LOGO_ZOOM_INTERVAL_MS.with(|cell| {
        cell.set(borne_ou_garde(ms, cell.get(), MIN_LOGO_ZOOM_INTERVAL_MS, MAX_LOGO_ZOOM_INTERVAL_MS))
    });
}

/// Bornes de la taille, de l'amplitude, de la durée et de l'attente du zoom —
/// **SEULE AUTORITÉ**, poussée aux Réglages (même règle que `logo_turn_bounds`).
pub fn logo_size_zoom_bounds() -> [(f32, f32); 4] {
    [
        (MIN_LOGO_SIZE, MAX_LOGO_SIZE),
        (MIN_LOGO_ZOOM_AMPLITUDE, MAX_LOGO_ZOOM_AMPLITUDE),
        (MIN_LOGO_ZOOM_MS, MAX_LOGO_ZOOM_MS),
        (MIN_LOGO_ZOOM_INTERVAL_MS, MAX_LOGO_ZOOM_INTERVAL_MS),
    ]
}

/// Le facteur du zoom à un instant donné : `1.0` pendant `interval_ms`, puis
/// une bosse de `zoom_ms` qui monte à `1 + amplitude` et redescend.
///
/// La bosse est `(1 − cos 2πu) / 2` : sa pente est NULLE aux deux bouts, donc le
/// zoom démarre et finit sans à-coup. **Fonction PURE du temps écoulé**, comme
/// `logo_rotation_at` : aucun état, rien qui dérive.
pub fn logo_zoom_factor_at(elapsed: std::time::Duration, amplitude: f32, zoom_ms: f32, interval_ms: f32) -> f32 {
    // `!(x > 0.0)` rejette aussi `NaN` — même idiome que `logo_rotation_at`.
    if !(zoom_ms > 0.0) || !(amplitude > 0.0) {
        return 1.0;
    }
    let interval_ms = if interval_ms > 0.0 { interval_ms } else { 0.0 };
    let phase = (elapsed.as_secs_f32() * 1000.0).rem_euclid(interval_ms + zoom_ms);
    if phase < interval_ms {
        return 1.0;
    }
    let u = (phase - interval_ms) / zoom_ms;
    1.0 + amplitude * (1.0 - (std::f32::consts::TAU * u).cos()) * 0.5
}

/// L'échelle de DESSIN du logo MAINTENANT : la taille réglée, multipliée par le
/// zoom s'il est actif. Partage l'horloge du fond et de la rotation.
pub fn logo_draw_scale() -> f32 {
    let zoom = if logo_zoom_enabled() {
        logo_zoom_factor_at(logo_animation_elapsed(), logo_zoom_amplitude(), logo_zoom_ms(), logo_zoom_interval_ms())
    } else {
        1.0
    };
    logo_size() * zoom
}

/// `rect` (`[x, y, largeur, hauteur]`) mis à l'échelle `scale` autour de son
/// CENTRE.
pub fn scale_rect_centered(rect: [f32; 4], scale: f32) -> [f32; 4] {
    let [x, y, w, h] = rect;
    let (nw, nh) = (w * scale, h * scale);
    [x + (w - nw) * 0.5, y + (h - nh) * 0.5, nw, nh]
}

// L'icône "réglages" est désormais une icône du dock COMME LES AUTRES (voir
// `Side`, `Dock::new`) — les fonctions `settings_icon_rect`/`settings_icon_hit`/
// `settings_icon_instance` qui vivaient ici (ancrées en dur en haut de la
// barre de DROITE, en dehors du système de `Dock::icons`) ont été retirées :
// `dock.icon_rect(SETTINGS_ICON_INDEX, ...)`/`dock.icon_at(...)`/
// `dock.external_glyph_instance(SETTINGS_ICON_INDEX, ...)` couvrent
// maintenant exactement le même besoin, voir `app::run`.

#[cfg(test)]
mod tests {
    use super::*;

    /// Quatre barres : quatre congés, chacun dans SON coin de la zone libre et
    /// tourné vers elle ; en haut ils prolongent leur barre LATÉRALE (demande du
    /// 2026-10-16 : la teinte de la colonne qu'ils longent), en bas celle du bas.
    #[test]
    fn quatre_barres_donnent_quatre_conges_tournes_vers_la_zone_libre() {
        let c = conges(1000.0, 800.0, [40.0, 60.0, 70.0, 50.0], 20.0);
        assert_eq!(c.len(), 4);
        assert_eq!(c[0], Conge { rect: [60.0, 40.0, 20.0, 20.0], vers: [1.0, 1.0], barre: 1 });
        assert_eq!(c[1], Conge { rect: [910.0, 40.0, 20.0, 20.0], vers: [-1.0, 1.0], barre: 2 });
        assert_eq!(c[2], Conge { rect: [60.0, 730.0, 20.0, 20.0], vers: [1.0, -1.0], barre: 3 });
        assert_eq!(c[3], Conge { rect: [910.0, 730.0, 20.0, 20.0], vers: [-1.0, -1.0], barre: 3 });
        assert_eq!(c.iter().map(|c| code_conge(c.vers)).collect::<Vec<_>>(), [-4.0, -3.0, -2.0, -1.0]);
    }

    /// Une barre masquée DÉPLACE ses coins au bord de l'écran, qui prolongent
    /// alors l'autre barre ; sans aucune de leurs deux barres, ils disparaissent.
    /// (Avant le 2026-10-14, un seul côté masqué suffisait à les effacer.)
    #[test]
    fn un_coin_suit_ses_barres_visibles() {
        // Barre du bas masquée : les coins du bas descendent au bord et
        // prolongent les latérales.
        let c = conges(1000.0, 800.0, [40.0, 60.0, 70.0, 0.0], 20.0);
        assert_eq!(c.iter().map(|c| c.barre).collect::<Vec<_>>(), [1, 2, 1, 2]);
        // Une colonne masquée : son coin du haut reprend la barre du haut.
        let masquee = conges(1000.0, 800.0, [40.0, 0.0, 70.0, 50.0], 20.0);
        assert_eq!(masquee.iter().map(|c| c.barre).collect::<Vec<_>>(), [0, 2, 3, 3]);
        assert_eq!(c[2].rect, [60.0, 780.0, 20.0, 20.0]);
        // Latérales masquées : les coins du haut glissent au bord de l'écran.
        let c = conges(1000.0, 800.0, [40.0, 0.0, 0.0, 0.0], 20.0);
        assert_eq!(c.iter().map(|c| (c.rect[0], c.barre)).collect::<Vec<_>>(), [(0.0, 0), (980.0, 0)]);
        // Rien de visible : aucun coin.
        assert!(conges(1000.0, 800.0, [0.0; 4], 20.0).is_empty());
        assert!(conges(1000.0, 800.0, [40.0, 60.0, 70.0, 50.0], 0.5).is_empty(), "sous un pixel, rien");
    }

    /// Un bout de barre sous chaque colonne masquée, à la hauteur de la barre du
    /// bas ; rien sans barre du bas visible.
    #[test]
    fn un_bout_de_barre_comble_le_pied_d_une_colonne_masquee() {
        let layout = DockLayout { total_width: 1000.0, total_height: 800.0, top_thickness: 40.0, haut_colonnes: 40.0, left_width: 60.0, right_width: 70.0, bottom_thickness: 50.0 };
        assert!(bouts_de_barre_du_bas(&layout, [40.0, 60.0, 70.0, 50.0]).is_empty(), "tout visible : rien");
        assert_eq!(bouts_de_barre_du_bas(&layout, [40.0, 0.0, 70.0, 50.0]), [[0.0, 750.0, 60.0, 50.0]]);
        assert_eq!(bouts_de_barre_du_bas(&layout, [40.0, 60.0, 0.0, 50.0]), [[930.0, 750.0, 70.0, 50.0]]);
        assert!(bouts_de_barre_du_bas(&layout, [40.0, 0.0, 0.0, 0.0]).is_empty(), "barre du bas masquée : rien");
        // Le coin du bas glisse au bord et tombe dans la colonne masquée.
        let c = conges(1000.0, 800.0, [40.0, 0.0, 70.0, 50.0], 20.0);
        assert_eq!(c[2].rect, [0.0, 730.0, 20.0, 20.0]);
        assert!(dans_une_colonne_masquee(c[2].rect, &layout, [40.0, 0.0, 70.0, 50.0]));
        assert!(!dans_une_colonne_masquee(c[3].rect, &layout, [40.0, 0.0, 70.0, 50.0]));
    }

    /// Le rayon est borné à la demi-zone libre : deux congés ne se chevauchent
    /// jamais.
    #[test]
    fn le_rayon_est_borne_a_la_demi_zone_libre() {
        let c = conges(200.0, 800.0, [40.0, 60.0, 70.0, 50.0], 100.0);
        assert_eq!(c[0].rect[2], 35.0);
    }

/// **LE CONTRAT DU `debug_assert_eq!` D'`ensure_saved_spacers_exist`** — un `limit`
/// INFÉRIEUR à `icons.len()` doit être **impossible**, pas seulement improbable :
/// `ensure_spacer_capacity` serait alors un no-op silencieux, et les espaces seraient
/// perdus exactement comme avant le correctif que cet assert protège.
#[test]
fn saved_spacer_capacity_never_goes_below_winpin_end() {
    const WINPIN_END: usize = 116;
    let vide: &[usize] = &[];

    // Rien à recréer : aucun index n'atteint la frontière des épingles.
    assert_eq!(Dock::saved_spacer_capacity([vide; 4], None, WINPIN_END), None);
    assert_eq!(Dock::saved_spacer_capacity([vide; 4], Some(40), WINPIN_END), None);
    assert_eq!(Dock::saved_spacer_capacity([&[3, 7], &[5], vide, vide], Some(80), WINPIN_END), None);

    // Le plus grand index cité, ORDRES ET PLANCHER CONFONDUS, plus un.
    assert_eq!(Dock::saved_spacer_capacity([&[3], &[130], vide, vide], None, WINPIN_END), Some(131));
    assert_eq!(Dock::saved_spacer_capacity([&[3], &[130], vide, vide], Some(150), WINPIN_END), Some(151));

    // ⚠️ La frontière elle-même compte comme un espace : `>=`, pas `>`. Avec `>`, un espace
    // pile à `winpin_end` serait ignoré et ne serait jamais recréé.
    assert_eq!(Dock::saved_spacer_capacity([&[WINPIN_END], vide, vide, vide], None, WINPIN_END), Some(117));

    // Et le `limit` rendu dépasse TOUJOURS la frontière — c'est ce que le
    // `debug_assert_eq!` d'`ensure_saved_spacers_exist` protège.
    for floor in [None, Some(150)] {
        let limit = Dock::saved_spacer_capacity([&[3], &[130], vide, vide], floor, WINPIN_END).unwrap();
        assert!(limit > WINPIN_END, "un limit doit depasser winpin_end, vu {limit}");
    }
}


    /// **Le plus grand index cité par une cote** — la seule chose que
    /// `ensure_saved_spacers_exist` a jamais eue besoin de savoir des cotes.
    ///
    /// Ces tests tiennent leurs cotes sous la forme `(index, cote, vide)` parce que c'est ce
    /// que `Dock::zone_heights()` rend ; le dock, lui, ne connaît plus cette forme depuis que
    /// le paramètre a été réduit à un plancher d'index (voir sa doc). Ce petit adaptateur
    /// garde les fixtures lisibles.
    fn floor(zones: &[(usize, f32, f32)]) -> Option<usize> {
        zones.iter().map(|&(index, _, _)| index).max()
    }

    /// Les trois formes doivent se DISTINGUER à la taille où elles servent :
    /// une forme dont la géométrie serait identique à celle du rond donnerait
    /// un réglage sans effet visible — le pire des défauts, puisqu'il ne se
    /// voit qu'en comparant deux captures.
    #[test]
    fn running_dot_shapes_are_geometrically_distinct() {
        let size = DEFAULT_RUNNING_DOT_SIZE;
        let icon_w = 64.0;
        let circle = RunningDotShape::Circle.hung_geometry(size, icon_w);
        let square = RunningDotShape::Square.hung_geometry(size, icon_w);
        let bar = RunningDotShape::Bar.hung_geometry(size, icon_w);

        // Le rond et le carré partagent leur empreinte, et ne diffèrent que par
        // le rayon des coins.
        assert_eq!((circle.0, circle.1), (square.0, square.1));
        assert!(circle.2 > square.2, "le rond est plus arrondi que le carré");
        // Le trait est plus LARGE que haut — c'est ce qui le fait lire comme un
        // soulignement — et sa taille de réglage donne sa HAUTEUR.
        assert!(bar.0 > bar.1, "le trait est plus large que haut");
        assert!(bar.1 < circle.1, "le trait est plus fin que le rond");
    }

    /// La taille demandée est respectée comme diamètre pour le rond : c'est le
    /// comportement d'origine, celui que l'utilisateur voit déjà.
    #[test]
    fn running_dot_size_is_the_diameter() {
        for size in [MIN_RUNNING_DOT_SIZE, 12.0, MAX_RUNNING_DOT_SIZE] {
            let (w, h, radius) = RunningDotShape::Circle.hung_geometry(size, 64.0);
            assert_eq!((w, h), (size, size));
            assert_eq!(radius, size * 0.5);
        }
    }

    /// Les noms persistés font l'aller-retour — un nom qui ne se relirait pas
    /// ferait silencieusement retomber la forme sur le rond au redémarrage.
    #[test]
    fn running_dot_shape_names_round_trip() {
        for shape in RunningDotShape::ALL {
            assert_eq!(RunningDotShape::from_config(shape.as_str()), shape);
        }
        // Une valeur inconnue ou vide retombe sur le rond plutôt que de paniquer.
        assert_eq!(RunningDotShape::from_config(""), RunningDotShape::Circle);
        assert_eq!(RunningDotShape::from_config("ovale"), RunningDotShape::Circle);
        // Noms LITTÉRAUX des formes ajoutées : ce fichier est PERSISTÉ, donc un
        // renommage silencieux ferait retomber la forme sur « Rond » au
        // redémarrage suivant sans que rien ne le signale — le test d'aller-retour
        // ci-dessus ne le verrait pas, `as_str` et `from_config` restant d'accord
        // entre eux.
        assert_eq!(RunningDotShape::Underline.as_str(), "underline");
        assert_eq!(RunningDotShape::Diamond.as_str(), "diamond");
        assert_eq!(RunningDotShape::Ring.as_str(), "ring");
    }

    /// Pose les réglages de survol, exécute `body`, puis les REMET dans leur
    /// état d'avant : les cellules sont globales au fil d'exécution (voir
    /// `HOVER_SCALE`), donc un test qui les laisserait modifiées contaminerait
    /// les suivants dès que libtest les enchaîne sur un même fil
    /// (`--test-threads=1`). Ce n'est pas une précaution de principe : c'est
    /// aussi ce qui rend chaque test lisible sans répéter six lignes.
    ///
    /// **L'horloge est restaurée ELLE AUSSI**, sans qu'aucun appelant n'ait à le
    /// demander : c'est le seul moyen d'être certain qu'aucun des tests écrits
    /// avant que ce réglage n'existe — il y en a sept — ne laisse derrière lui
    /// une valeur qui ferait échouer un test sans rapport apparent.
    /// Le pendant de `with_hover_settings` pour le SEUL tirage : on le pose, on
    /// rend la main, et on le remet comme il etait — un test ne doit pas laisser un
    /// reglage d'apparence derriere lui.
    fn with_tirage(tirage: f32, body: impl FnOnce()) {
        for barre in BarreTirage::TOUTES {
            set_tirage_vers_le_centre(barre, tirage);
        }
        body();
        for barre in BarreTirage::TOUTES {
            set_tirage_vers_le_centre(barre, DEFAULT_TIRAGE);
        }
    }

    fn with_hover_settings(scale: f32, speed: f32, body: impl FnOnce()) {
        let previous_clock = clock_hover_scale();
        let (previous_scale, previous_speed) = (hover_scale(), hover_ease_speed());
        set_hover_scale(scale);
        set_hover_ease_speed(speed);
        body();
        set_hover_scale(previous_scale);
        set_hover_ease_speed(previous_speed);
        set_clock_hover_scale(previous_clock);
    }

    /// Le pendant de `with_hover_settings` pour le SEUL réglage des horloges,
    /// qui n'a pas de vitesse à lui — elle est partagée avec les icônes (voir
    /// `CLOCK_HOVER_SCALE`, doc de statique).
    fn with_clock_hover_scale(scale: f32, body: impl FnOnce()) {
        let previous = clock_hover_scale();
        set_clock_hover_scale(scale);
        body();
        set_clock_hover_scale(previous);
    }

    /// Le bornage est ce qui empêche un fichier de configuration corrompu
    /// d'atteindre l'animation — une échelle à `1000` ferait disparaître les
    /// icônes hors de leur barre. Valeurs EXACTES (le bornage affecte la
    /// constante elle-même), pas des approximations.
    #[test]
    fn hover_settings_clamp_to_their_bounds() {
        with_hover_settings(DEFAULT_HOVER_SCALE, DEFAULT_HOVER_EASE_SPEED, || {
            set_hover_scale(1000.0);
            assert_eq!(hover_scale(), MAX_HOVER_SCALE);
            set_hover_scale(0.0);
            assert_eq!(hover_scale(), MIN_HOVER_SCALE);

            set_hover_ease_speed(1000.0);
            assert_eq!(hover_ease_speed(), MAX_HOVER_EASE_SPEED);
            set_hover_ease_speed(0.0);
            assert_eq!(hover_ease_speed(), MIN_HOVER_EASE_SPEED);
        });
    }

    /// Le SEUL trou que le bornage laissait : `f32::clamp` propage un NaN au
    /// lieu de le borner, et `config::read_config_floats` accepte « nan »
    /// puisque `"nan".parse::<f32>()` réussit (voir `set_hover_scale`, doc).
    /// Sans ce test, une telle valeur figerait l'échelle de l'icône survolée
    /// pour de bon.
    #[test]
    fn hover_settings_ignore_a_nan_value() {
        with_hover_settings(DEFAULT_HOVER_SCALE, DEFAULT_HOVER_EASE_SPEED, || {
            // Une valeur valide posée D'ABORD, pour vérifier qu'elle survit au
            // NaN : c'est ce qui distingue « ignorer » de « retomber à zéro ».
            set_hover_scale(2.0);
            set_hover_scale(f32::NAN);
            assert_eq!(hover_scale(), 2.0);

            set_hover_ease_speed(30.0);
            set_hover_ease_speed(f32::NAN);
            assert_eq!(hover_ease_speed(), 30.0);

            // L'infini, lui, reste BORNÉ — il n'est pas NaN, et le garde-fou ne
            // doit pas casser cette saturation en même temps.
            set_hover_scale(f32::INFINITY);
            assert_eq!(hover_scale(), MAX_HOVER_SCALE);
            set_hover_ease_speed(f32::INFINITY);
            assert_eq!(hover_ease_speed(), MAX_HOVER_EASE_SPEED);
        });
    }

    /// Le bornage du réglage des HORLOGES — même rôle que
    /// `hover_settings_clamp_to_their_bounds`, mais avec un plafond qui n'est
    /// PAS celui des icônes (350 % contre 250 %) : c'est ce qui rend ce test
    /// utile plutôt que redondant.
    #[test]
    fn clock_hover_settings_clamp_to_their_bounds() {
        with_clock_hover_scale(DEFAULT_CLOCK_HOVER_SCALE, || {
            set_clock_hover_scale(1000.0);
            assert_eq!(clock_hover_scale(), MAX_CLOCK_HOVER_SCALE);
            set_clock_hover_scale(0.0);
            assert_eq!(clock_hover_scale(), MIN_CLOCK_HOVER_SCALE);
        });
    }

    /// Même trou, même garde-fou que `hover_settings_ignore_a_nan_value` : un
    /// `clock_hover_scale.txt` contenant « nan » figerait le texte des DEUX
    /// horloges pour de bon.
    #[test]
    fn clock_hover_settings_ignore_a_nan_value() {
        with_clock_hover_scale(DEFAULT_CLOCK_HOVER_SCALE, || {
            // Une valeur valide posée D'ABORD : c'est ce qui distingue
            // « ignorer » de « retomber au défaut ».
            set_clock_hover_scale(2.0);
            set_clock_hover_scale(f32::NAN);
            assert_eq!(clock_hover_scale(), 2.0);

            set_clock_hover_scale(f32::INFINITY);
            assert_eq!(clock_hover_scale(), MAX_CLOCK_HOVER_SCALE);
        });
    }

    /// **Les deux réglages sont INDÉPENDANTS** — et sans ce test, un accesseur
    /// d'horloge pointé par erreur sur `HOVER_SCALE` passerait tout le reste :
    /// même mécanisme, mêmes bornes basses, même valeur par défaut.
    #[test]
    fn clock_hover_scale_is_independent_of_the_icon_scale() {
        with_hover_settings(1.4, DEFAULT_HOVER_EASE_SPEED, || {
            with_clock_hover_scale(2.8, || {
                assert_eq!(hover_scale(), 1.4);
                assert_eq!(clock_hover_scale(), 2.8);

                set_hover_scale(1.9);
                assert_eq!(clock_hover_scale(), 2.8, "le réglage des icônes a bougé l'horloge");
                set_clock_hover_scale(1.1);
                assert_eq!(hover_scale(), 1.9, "le réglage de l'horloge a bougé les icônes");
            });
        });
    }

    /// **La règle de clic de toute la fonctionnalité** : l'icône magnifiée est
    /// testée EN PREMIER, jamais en repli — puisqu'elle est dessinée PAR-DESSUS
    /// les autres (voir `app::run`, son lot est poussé en dernier).
    ///
    /// Le test ne se contente pas de vérifier qu'on la trouve : il cherche un
    /// point qui tombe à la fois dans sa boîte grossie ET dans la case au repos
    /// d'une AUTRE icône, et exige que ce soit elle qui gagne. C'est exactement
    /// le cas que l'ordre inverse casserait — un clic sur le haut de l'icône
    /// grossie lancerait alors sa voisine. À 1.3x, la boîte d'une icône de 96px
    /// déborde déjà de 14px de chaque côté, donc ce recouvrement existe pour de
    /// bon (et la dernière assertion refuse un test qui n'aurait rien éprouvé).
    #[test]
    fn icon_at_gives_the_click_to_the_magnified_icon() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let mut overlaps_checked = 0;
        for hovered in 0..dock.icons.len() {
            if dock.icons[hovered].is_spacer {
                continue;
            }
            let Some(rect) = dock.icon_rect(hovered, &layout) else { continue };
            let (cx, cy) = (rect[0] + rect[2] * 0.5, rect[1] + rect[3] * 0.5);
            dock.on_mouse_move(cx, cy, HWND(std::ptr::null_mut()));
            for _ in 0..120 {
                dock.update(1.0 / 60.0, &layout);
            }
            assert_eq!(dock.magnified_index(), Some(hovered), "icône {hovered} survolée");

            // Le rectangle présenté est celui de CETTE icône, grossi — un
            // `presented_rect` qui prendrait l'échelle d'une autre (ou une
            // échelle constante) laisserait celui-ci à sa taille au repos.
            let presented = dock.presented_rect(hovered, &layout).expect("présentée");
            assert!(presented[2] > rect[2], "l'icône {hovered} doit être grossie");

            for other in 0..dock.icons.len() {
                if other == hovered || dock.icons[other].is_spacer {
                    continue;
                }
                let Some(o) = dock.icon_rect(other, &layout) else { continue };
                let (x0, y0) = (presented[0].max(o[0]), presented[1].max(o[1]));
                let (x1, y1) = (
                    (presented[0] + presented[2]).min(o[0] + o[2]),
                    (presented[1] + presented[3]).min(o[1] + o[3]),
                );
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                let at = dock.icon_at((x0 + x1) * 0.5, (y0 + y1) * 0.5, &layout, HWND(std::ptr::null_mut()));
                assert_eq!(
                    at,
                    Some(hovered),
                    "à cheval entre l'icône {hovered} grossie et la case de {other}, \
                     c'est l'icône grossie qui doit gagner le clic"
                );
                overlaps_checked += 1;
            }

            dock.on_mouse_leave(HWND(std::ptr::null_mut()));
            for _ in 0..600 {
                dock.update(1.0 / 60.0, &layout);
            }
        }
        assert!(overlaps_checked > 0, "aucun chevauchement trouvé : le test n'éprouverait rien");
    }

    /// Le clignotement de notification est une fonction PURE du temps : il doit
    /// être PÉRIODIQUE (sans quoi deux notifications arrivées à quelques
    /// centaines de millisecondes d'écart clignoteraient en décalé, ce qui se
    /// lit comme un défaut) et ne jamais rendre une couleur transparente — une
    /// pastille invisible ne signalerait rien.
    #[test]
    fn notification_blink_cycles_through_opaque_colors() {
        let period = NOTIFICATION_BLINK_PERIOD;
        assert_eq!(
            notification_blink_color_at(std::time::Duration::ZERO),
            notification_blink_color_at(period * NOTIFICATION_BLINK_COLORS.len() as u32),
            "la couleur revient après un tour complet"
        );
        assert_ne!(
            notification_blink_color_at(std::time::Duration::ZERO),
            notification_blink_color_at(period),
            "et elle change d'un pas au suivant"
        );
        for step in 0..(NOTIFICATION_BLINK_COLORS.len() as u32 * 2) {
            let color = notification_blink_color_at(period * step);
            assert_eq!(color[3], 1.0, "couleur opaque au pas {step}");
        }
    }

    /// Palette d'essai pour les tests du fond animé — trois couleurs
    /// RECONNAISSABLES, pour qu'un décalage d'index se voie au lieu de tomber
    /// sur une teinte voisine.
    const LOGO_TEST_PALETTE: [[f32; 4]; 3] = [
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
    ];

    /// Remet les réglages du fond animé à leur défaut — les cellules sont des
    /// `thread_local`, donc elles survivent d'un test à l'autre SUR LE MÊME
    /// THREAD (même raison que `with_hover_settings`).
    fn reset_logo_animation_settings() {
        set_logo_animation_enabled(false);
        set_logo_fade_ms(DEFAULT_LOGO_FADE_MS);
        set_logo_color_ms(DEFAULT_LOGO_COLOR_MS);
        set_logo_is_circle(false);
        set_logo_spin_clockwise(true);
        set_logo_turn_ms(DEFAULT_LOGO_TURN_MS);
        set_logo_size(1.0);
        set_logo_zoom_enabled(false);
        set_logo_zoom_amplitude(DEFAULT_LOGO_ZOOM_AMPLITUDE);
        set_logo_zoom_ms(DEFAULT_LOGO_ZOOM_MS);
        set_logo_zoom_interval_ms(DEFAULT_LOGO_ZOOM_INTERVAL_MS);
    }

    #[test]
    fn an_icon_background_follows_the_bar_its_icon_is_in() {
        // LE CORRECTIF DU 2026-09-20, FIGÉ. Le fond d'un « dash » doit prendre la
        // FORME de la barre où son icône se trouve — étroit et haut dans une
        // barre latérale, large et court dans la barre du haut. C'est ce que le
        // défaut confondait : la barre était supposée PAR FENÊTRE, donc une icône
        // déplacée gardait la géométrie de son ancienne barre et le fond
        // atterrissait derrière d'AUTRES icônes.
        let layout = geometry_layout();
        let dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let (mut laterales, mut haut, mut bas) = (0, 0, 0);
        for index in 0..dock.icons.len() {
            let Some(side) = dock.icon_side(index) else { continue };
            let Some(zone) = icon_zone_for(&dock, &layout, index, [1.0, 0.0, 0.0, 1.0]) else { continue };
            match side {
                Side::TopLeft | Side::TopRight | Side::TopMiddle => {
                    // `icon_zone_instance_top` : toute la HAUTEUR de la barre, et
                    // posée à `y = 0`.
                    assert_eq!(zone.rect[3], layout.top_thickness, "icône {index} : hauteur de la barre du haut");
                    assert_eq!(zone.rect[1], 0.0, "icône {index} : posée au bord haut");
                    haut += 1;
                }
                Side::Left => {
                    // `icon_zone_instance` : toute la LARGEUR de la barre, et
                    // ancrée à son bord gauche.
                    assert_eq!(zone.rect[2], layout.left_width, "icône {index} : largeur de la barre de gauche");
                    assert_eq!(zone.rect[0], 0.0, "icône {index} : ancrée au bord gauche");
                    laterales += 1;
                }
                Side::Right => {
                    // Les deux barres latérales ont la MÊME épaisseur ici : c'est
                    // donc l'ANCRE qui les distingue, et non la taille.
                    assert_eq!(zone.rect[2], layout.right_width, "icône {index} : largeur de la barre de droite");
                    assert_eq!(
                        zone.rect[0],
                        layout.total_width - layout.right_width,
                        "icône {index} : ancrée au bord droit"
                    );
                    laterales += 1;
                }
                Side::Bottom => {
                    // `icon_zone_instance_bottom` : toute la HAUTEUR de la barre
                    // du bas, ancrée à `total_height - bottom_thickness` — le
                    // jumeau de la barre du haut, mais par le bord BAS.
                    // Aucune icône n'y vit par défaut (`bottom_order` est vide,
                    // les raccourcis de dossiers passant par une table latérale),
                    // donc cette branche n'est PAS atteinte aujourd'hui : elle est
                    // écrite pour que l'invariant existe le jour où une icône y
                    // arrivera, plutôt que d'être découvert à ce moment-là.
                    assert_eq!(zone.rect[3], layout.bottom_thickness, "icône {index} : hauteur de la barre du bas");
                    assert_eq!(
                        zone.rect[1],
                        layout.total_height - layout.bottom_thickness,
                        "icône {index} : ancrée au bord bas"
                    );
                    bas += 1;
                }
            }
        }
        // GARDE CONTRE UN TEST VACUOUS : il faut avoir rencontré LES DEUX formes.
        // Un dock qui n'aurait que des icônes latérales passerait les assertions
        // ci-dessus sans jamais éprouver le choix, c'est-à-dire sans rien prouver
        // sur le défaut.
        assert!(
            haut > 0 && laterales > 0,
            "les deux formes doivent être éprouvées (haut={haut}, latérales={laterales}, bas={bas})"
        );
    }

    /// ⭐ **CHAQUE BARRE SON ÉCARTEMENT** — DEMANDE EXPLICITE (2026-10-18) : régler
    /// celui d'une barre n'écarte que ses icônes ; les autres gardent le général.
    #[test]
    fn l_ecartement_d_une_barre_ne_touche_qu_elle() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        // Les deux premières icônes d'une barre : dans le dock d'origine, ni
        // espace ni icône masquée entre elles.
        let paire = |dock: &Dock, side: Side| (dock.order(side)[0], dock.order(side)[1]);
        let ecart_vertical = |dock: &Dock, (a, b): (usize, usize)| {
            let (ra, rb) = (dock.icon_rect(a, &layout).unwrap(), dock.icon_rect(b, &layout).unwrap());
            rb[1] - (ra[1] + ra[3])
        };
        let ecart_horizontal = |dock: &Dock, (a, b): (usize, usize)| {
            let (ra, rb) = (dock.icon_rect(a, &layout).unwrap(), dock.icon_rect(b, &layout).unwrap());
            (rb[0] - (ra[0] + ra[2])).abs().min((ra[0] - (rb[0] + rb[2])).abs())
        };
        let (droite, haut) = (paire(&dock, Side::Right), paire(&dock, Side::TopRight));
        let (avant_droite, avant_haut) = (ecart_vertical(&dock, droite), ecart_horizontal(&dock, haut));
        assert_eq!(avant_droite, dock.icon_gap(), "(prémisse : la barre de droite suit le général)");
        dock.set_icon_gaps_barres([None, None, Some(dock.icon_gap() + 15.0), None]);
        assert_eq!(ecart_vertical(&dock, droite), avant_droite + 15.0, "la barre de droite s'écarte");
        assert_eq!(ecart_horizontal(&dock, haut), avant_haut, "celle du haut ne bouge pas");
        assert_eq!(dock.icon_gap_de(droite.0), dock.icon_gap() + 15.0);
        dock.set_icon_gaps_barres([Some(dock.icon_gap() + 5.0), None, None, None]);
        assert_eq!(ecart_horizontal(&dock, haut), avant_haut + 5.0, "et celle du haut se règle à part");
        assert_eq!(ecart_vertical(&dock, droite), avant_droite, "sans toucher la droite");
    }

    #[test]
    fn le_halo_du_logo_entoure_l_icone_sans_prendre_la_barre() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.set_icon_gap(8.0);
        let gap = dock.icon_gap_de(0);
        let icone = dock.presented_rect(0, &layout).expect("le logo a un emplacement");
        let halo = icon_halo_for(&dock, &layout, 0, false, [1.0, 0.0, 0.0, 1.0]).unwrap();
        // Centré sur l'icône, débordant de `gap / 2` de chaque côté.
        assert_eq!(halo.rect, [icone[0] - 4.0, icone[1] - 4.0, icone[2] + gap, icone[3] + gap]);
        // Et PAS la largeur de la barre : c'était la demande.
        let pleine = icon_zone_for(&dock, &layout, 0, [1.0, 0.0, 0.0, 1.0]).unwrap();
        assert!(halo.rect[2] * halo.rect[3] < pleine.rect[2] * pleine.rect[3]);
        assert!((halo.radius - icon_radius() - 4.0).abs() < 1e-3);
        // Pour l'icône circulaire, le même carré devient un DISQUE.
        let disque = icon_halo_for(&dock, &layout, 0, true, [1.0, 0.0, 0.0, 1.0]).unwrap();
        assert_eq!(disque.rect, halo.rect);
        assert!((disque.radius - disque.rect[2] * 0.5).abs() < 1e-3);
    }

    #[test]
    fn logo_rotation_turns_forever_in_the_chosen_direction() {
        let turn: u64 = 4000;
        let turn_ms = turn as f32;
        let full = std::f32::consts::TAU;

        // Départ à l'angle nul, quart de tour à un quart, retour à zéro après un
        // tour complet : la rotation est LINÉAIRE en temps.
        let at0 = logo_rotation_at(std::time::Duration::ZERO, turn_ms, true);
        assert!(at0.abs() < 1e-5, "on part de zéro : {at0}");
        let quarter = logo_rotation_at(std::time::Duration::from_millis(turn / 4), turn_ms, true);
        assert!((quarter - full / 4.0).abs() < 1e-4, "quart de tour : {quarter}");
        let after = logo_rotation_at(std::time::Duration::from_millis(turn), turn_ms, true);
        assert!(after.abs() < 1e-4, "un tour complet ramène à zéro : {after}");

        // LE SENS INVERSE — comparé par SINUS et COSINUS, et non par les angles :
        // l'angle opposé tombe de l'autre côté du tour complet (2π−x contre x),
        // donc une comparaison directe échouerait à tort.
        let cw = logo_rotation_at(std::time::Duration::from_millis(turn / 4), turn_ms, true);
        let ccw = logo_rotation_at(std::time::Duration::from_millis(turn / 4), turn_ms, false);
        assert!((cw.sin() + ccw.sin()).abs() < 1e-4, "sinus opposés : {cw} / {ccw}");
        assert!((cw.cos() - ccw.cos()).abs() < 1e-4, "mêmes cosinus : {cw} / {ccw}");

        // **L'ANGLE RESTE BORNÉ, MÊME APRÈS UN MILLION DE TOURS** — c'est tout
        // l'objet du `rem_euclid`, et un test qui ne regarderait que le premier
        // tour ne prouverait rien sur ce qu'il protège (la perte de précision
        // d'un `f32` sur un angle qui croît sans fin).
        for tours in [100u64, 10_000, 1_000_000] {
            let angle = logo_rotation_at(std::time::Duration::from_millis(turn * tours), turn_ms, true);
            assert!((0.0..full).contains(&angle), "angle non borné après {tours} tours : {angle}");
        }

        // Une durée absurde ne rend PAS `NaN` : l'angle part au shader, où il
        // ferait disparaître l'icône sans rien dire.
        for bad in [0.0, -1.0, f32::NAN] {
            let angle = logo_rotation_at(std::time::Duration::from_millis(500), bad, true);
            assert!(angle.is_finite(), "durée {bad} : {angle}");
        }
    }

    #[test]
    fn logo_spin_settings_clamp_and_reject_nan() {
        reset_logo_animation_settings();
        set_logo_turn_ms(1.0e9);
        assert_eq!(logo_turn_ms(), MAX_LOGO_TURN_MS);
        set_logo_turn_ms(-1.0);
        assert_eq!(logo_turn_ms(), MIN_LOGO_TURN_MS);
        set_logo_turn_ms(3000.0);
        set_logo_turn_ms(f32::NAN);
        assert_eq!(logo_turn_ms(), 3000.0, "NaN doit être ignoré");
        assert_eq!(logo_turn_bounds(), (MIN_LOGO_TURN_MS, MAX_LOGO_TURN_MS));
        // Le sens et le choix d'icône sont de simples drapeaux, sans borne.
        set_logo_is_circle(true);
        assert!(logo_is_circle());
        set_logo_spin_clockwise(false);
        assert!(!logo_spin_clockwise());
        reset_logo_animation_settings();
        assert!(!logo_is_circle() && logo_spin_clockwise());
    }

    /// ⭐ DEMANDE EXPLICITE (2026-10-15) : « la taille de l'icône du logo
    /// modifiable » et « un effet de zoom centré temporisé paramétrable ».
    /// Le zoom attend `interval_ms` à taille normale, puis grossit et revient
    /// en `zoom_ms`, sans à-coup aux deux bouts.
    #[test]
    fn le_zoom_du_logo_attend_puis_grossit_et_revient() {
        let at = |ms: u64| logo_zoom_factor_at(std::time::Duration::from_millis(ms), 0.5, 1000.0, 4000.0);
        // Pendant l'attente : taille normale.
        assert_eq!(at(0), 1.0);
        assert_eq!(at(3999), 1.0);
        // Au milieu du zoom : le sommet, 1 + amplitude.
        assert!((at(4500) - 1.5).abs() < 1e-4, "{}", at(4500));
        // Les deux bouts du zoom sont doux : presque 1.
        assert!(at(4010) < 1.001 && at(4990) < 1.001, "{} {}", at(4010), at(4990));
        // Et ça recommence : période = attente + zoom.
        assert_eq!(at(5000), 1.0);
        assert!((at(9500) - 1.5).abs() < 1e-4);
        // Jamais plus petit que la taille réglée.
        assert!((0..20_000).step_by(37).all(|ms| at(ms) >= 1.0));
        // Des durées absurdes ne produisent jamais de NaN.
        for bad in [0.0, -1.0, f32::NAN] {
            let f = logo_zoom_factor_at(std::time::Duration::from_millis(700), 0.5, bad, bad);
            assert!(f.is_finite() && f >= 1.0, "durée {bad} : {f}");
        }
    }

    #[test]
    fn la_taille_et_le_zoom_du_logo_sont_bornes() {
        reset_logo_animation_settings();
        assert_eq!(logo_size(), 1.0, "100 % par défaut");
        assert!(!logo_zoom_enabled(), "zoom désactivé par défaut");
        set_logo_size(9.0);
        assert_eq!(logo_size(), MAX_LOGO_SIZE);
        set_logo_size(0.0);
        assert_eq!(logo_size(), MIN_LOGO_SIZE);
        set_logo_size(f32::NAN);
        assert_eq!(logo_size(), MIN_LOGO_SIZE, "NaN doit être ignoré");
        set_logo_zoom_amplitude(5.0);
        assert_eq!(logo_zoom_amplitude(), MAX_LOGO_ZOOM_AMPLITUDE);
        set_logo_zoom_ms(1.0);
        assert_eq!(logo_zoom_ms(), MIN_LOGO_ZOOM_MS);
        set_logo_zoom_interval_ms(1.0e9);
        assert_eq!(logo_zoom_interval_ms(), MAX_LOGO_ZOOM_INTERVAL_MS);
        // Zoom éteint : seule la taille compte.
        set_logo_size(0.8);
        assert_eq!(logo_draw_scale(), 0.8);
        reset_logo_animation_settings();
        assert_eq!(logo_size(), 1.0);
    }

    /// L'agrandissement est CENTRÉ : le centre du rectangle ne bouge pas.
    #[test]
    fn le_logo_grossit_autour_de_son_centre() {
        let r = scale_rect_centered([10.0, 20.0, 40.0, 40.0], 1.5);
        assert_eq!(r, [0.0, 10.0, 60.0, 60.0]);
        let r = scale_rect_centered([10.0, 20.0, 40.0, 40.0], 0.5);
        assert_eq!(r, [20.0, 30.0, 20.0, 20.0]);
    }

    #[test]
    fn logo_background_color_cycles_through_the_palette() {
        let ms: u64 = 1000;
        let ms_f = ms as f32;
        // Un pas par couleur, dans l'ordre de la palette.
        for (step, expected) in LOGO_TEST_PALETTE.iter().enumerate() {
            let at = logo_background_color_at(std::time::Duration::from_millis(ms * step as u64), ms_f, &LOGO_TEST_PALETTE);
            assert_eq!(at, [expected[0], expected[1], expected[2]], "pas {step}");
        }
        // La couleur REVIENT après un tour complet — la boucle est le sujet.
        let tour = std::time::Duration::from_millis(ms * LOGO_TEST_PALETTE.len() as u64);
        assert_eq!(
            logo_background_color_at(std::time::Duration::ZERO, ms_f, &LOGO_TEST_PALETTE),
            logo_background_color_at(tour, ms_f, &LOGO_TEST_PALETTE),
            "un tour complet ramène la première couleur"
        );
        // Une palette vide rend le blanc au lieu de paniquer : le fond est dans
        // la boucle de rendu, un `panic` y tuerait l'application entière.
        assert_eq!(logo_background_color_at(std::time::Duration::ZERO, ms_f, &[]), [1.0, 1.0, 1.0]);
    }

    #[test]
    fn logo_background_alpha_breathes_between_opaque_and_invisible() {
        let fade = 1000u64;
        let fade_ms = fade as f32;
        // Départ PLEINEMENT OPAQUE, invisible à un aller, de retour à deux :
        // c'est la forme que la demande décrit (« de 100 % d'opacité à 100 % de
        // transparence »), et elle est CONTINUE — aucun palier.
        assert!((logo_background_alpha_at(std::time::Duration::ZERO, fade_ms) - 1.0).abs() < 1e-6);
        assert!(logo_background_alpha_at(std::time::Duration::from_millis(fade), fade_ms).abs() < 1e-6);
        assert!((logo_background_alpha_at(std::time::Duration::from_millis(fade * 2), fade_ms) - 1.0).abs() < 1e-6);

        // Bornée partout, périodique, et jamais NaN.
        let mut descentes = 0;
        for step in 0..40u64 {
            let elapsed = std::time::Duration::from_millis(fade * step / 40);
            let alpha = logo_background_alpha_at(elapsed, fade_ms);
            assert!((0.0..=1.0).contains(&alpha), "hors bornes : {alpha}");
            let later = logo_background_alpha_at(elapsed + std::time::Duration::from_millis(fade * 2), fade_ms);
            assert!((alpha - later).abs() < 1e-5, "période : {alpha} puis {later}");
            if alpha < 0.9 {
                descentes += 1;
            }
        }
        // GARDE CONTRE UN TEST VACUOUS : la boucle doit avoir réellement
        // rencontré des instants où le fond s'efface (même réflexe que le test du
        // balancement, qui compte ses couples opposés).
        assert!(descentes > 0, "la boucle n'a éprouvé aucun instant d'effacement");

        // Une durée absurde rend l'état le plus neutre au lieu de `NaN` — et
        // `!(x > 0.0)` doit aussi rejeter `NaN`, comme `hover_wobble_angle_at`.
        for dur in [0.0, -1.0, f32::NAN] {
            let alpha = logo_background_alpha_at(std::time::Duration::from_millis(500), dur);
            assert!((alpha - 1.0).abs() < 1e-6, "durée {dur} : {alpha}");
        }
    }

    #[test]
    fn logo_animation_settings_clamp_to_their_bounds() {
        reset_logo_animation_settings();
        set_logo_fade_ms(1.0e9);
        assert_eq!(logo_fade_ms(), MAX_LOGO_FADE_MS);
        set_logo_fade_ms(-1.0);
        assert_eq!(logo_fade_ms(), MIN_LOGO_FADE_MS);
        set_logo_color_ms(1.0e9);
        assert_eq!(logo_color_ms(), MAX_LOGO_COLOR_MS);
        set_logo_color_ms(-1.0);
        assert_eq!(logo_color_ms(), MIN_LOGO_COLOR_MS);

        // `NaN` est IGNORÉ — pas appliqué, et surtout pas remis à zéro : un
        // `NaN` venu d'un fichier de configuration ferait disparaître le fond
        // sans rien dire (`f32::clamp` le propage).
        set_logo_fade_ms(1234.0);
        set_logo_fade_ms(f32::NAN);
        assert_eq!(logo_fade_ms(), 1234.0, "NaN doit être ignoré");
        set_logo_color_ms(4321.0);
        set_logo_color_ms(f32::NAN);
        assert_eq!(logo_color_ms(), 4321.0, "NaN doit être ignoré");
        // L'infini, lui, SATURE : le garde ne doit pas avoir cassé le bornage.
        set_logo_fade_ms(f32::INFINITY);
        assert_eq!(logo_fade_ms(), MAX_LOGO_FADE_MS);

        // Et les bornes publiées sont celles des constantes — la page des
        // Réglages n'en recopie aucune, donc c'est le seul endroit qui les fixe.
        assert_eq!(logo_animation_bounds(), ((MIN_LOGO_FADE_MS, MAX_LOGO_FADE_MS), (MIN_LOGO_COLOR_MS, MAX_LOGO_COLOR_MS)));
        reset_logo_animation_settings();
    }

    #[test]
    fn icon_zone_instance_carries_the_alpha_it_is_given() {
        // Le fond du logo s'efface SANS que l'icône s'efface : c'est cet alpha
        // qui le permet, et ce chemin ne l'avait jamais exercé (il était codé à
        // 1.0 en dur).
        let icon_rect = [0.0, 100.0, 48.0, 48.0];
        let opaque = icon_zone_instance(icon_rect, 0.0, 60.0, 8.0, [0.2, 0.4, 0.6, 1.0]);
        assert_eq!(opaque.color, [0.2, 0.4, 0.6, 1.0]);
        let half = icon_zone_instance(icon_rect, 0.0, 60.0, 8.0, [0.2, 0.4, 0.6, 0.5]);
        assert_eq!(half.color, [0.2, 0.4, 0.6, 0.5], "l'alpha traverse tel quel");
        // La zone couvre TOUTE la barre, pas seulement l'icône (rappel : c'était
        // une demande explicite, et c'est ce que le fond animé recouvre).
        assert_eq!(half.rect, [0.0, 100.0 - 4.0, 60.0, 48.0 + 8.0]);
        assert_eq!(half.tex_mode, 0.0, "le fond reste un aplat, pas une texture");
    }

    /// La boîte OCCUPÉE doit contenir l'icône dans TOUTES ses positions de
    /// balancement : c'est elle qui décide de ce que la région doit couvrir, donc
    /// de ce qui serait rogné sans elle.
    #[test]
    fn the_occupied_box_contains_a_rotated_icon() {
        let icon = [100.0, 100.0, 200.0, 200.0];
        let occupied = occupied_rect_for(icon);
        // Un carré tourné de 45° tient dans le cercle circonscrit, de rayon
        // `côté × √2 / 2` — la boîte occupée doit au moins le couvrir.
        let circumscribed = 200.0 * WOBBLE_HEADROOM;
        assert!(occupied[2] >= circumscribed - 1e-3, "largeur {}", occupied[2]);
        assert!(occupied[3] >= circumscribed - 1e-3, "hauteur {}", occupied[3]);
        // Et elle s'élargit AUTOUR de l'icône : le centre ne bouge pas.
        assert_eq!(occupied[0] + occupied[2] * 0.5, icon[0] + icon[2] * 0.5);
        assert_eq!(occupied[1] + occupied[3] * 0.5, icon[1] + icon[3] * 0.5);
    }

    /// Le balancement est fonction du temps et de l'avancement du survol : NUL au
    /// repos — sinon toutes les icônes du dock seraient penchées en permanence —
    /// et borné par l'amplitude demandée.
    #[test]
    fn hover_wobble_is_bounded_and_idle_at_rest() {
        assert_eq!(hover_wobble_angle(1.0), 0.0, "aucune rotation au repos");
        let amplitude = HOVER_WOBBLE_DEGREES.to_radians();
        for step in 0..200 {
            let scale = 1.0 + step as f32 * 0.01;
            let angle = hover_wobble_angle(scale);
            assert!(angle.abs() <= amplitude + 1e-4, "angle {angle} hors de l'amplitude");
        }
    }

    /// Disposition de référence des tests de géométrie — celle de la machine de
    /// développement (3840 de large), avec les épaisseurs par défaut. Noter
    /// `left_width: 140` : `icon_size_for(140)` sature à `ICON_SIZE_MAX`, donc
    /// les icônes LATÉRALES y font 96px, pas 48.
    /// ⛔ **LES DEUX PLANCHERS SUIVENT LA HAUTEUR DE L'ÉCRAN** — défaut SIGNALÉ sur la VM
    /// le 2026-10-30 : « les barres de gauche et de droite sont trop larges, les icônes de
    /// la barre du haut sont trop grandes ». Les valeurs sont celles de la VM (1280×648 à
    /// 96 DPI) et de la configuration réelle convertie : barre du haut 18 px, de droite 37.
    #[test]
    fn the_floors_follow_the_screen_height() {
        // Sur l'écran de référence, RIEN ne change (facteur 1).
        poser_hauteur_des_planchers(HAUTEUR_DES_PLANCHERS);
        assert_eq!(icone_minimale(), ICON_SIZE_MIN, "a 2160, le plancher d'icone est inchange");
        assert_eq!(plancher_de_colonne(), MIN_ZONE_COLUMN_PX, "a 144 DPI et 2160, la colonne aussi");

        // Sur la VM : 648 de haut, 96 DPI.
        poser_hauteur_des_planchers(648.0);
        poser_dpi_du_plancher_de_colonne(96);
        let colonne = plancher_de_colonne();
        assert!((colonne - 6.4).abs() < 0.01, "une colonne doit valoir 32*96/144*0.3 = 6,4, pas {colonne}");
        // ⭐ La barre de droite CONVERTIE (37) n'est plus élargie par le plancher : trois
        // colonnes tiennent dans 19,2 px, donc `max(37, 19.2) = 37`.
        assert!(3.0 * colonne < 37.0, "trois colonnes tiennent dans 37 px");
        // ⭐ Et une icône TIENT dans sa barre de 18 px, au lieu d'être forcée à 32.
        let icone = icon_size_for(18.0);
        assert!(icone <= 18.0, "l'icone ({icone}) ne doit pas depasser sa barre de 18 px");
        assert!((icone - 9.6).abs() < 0.01, "elle vaut 32*0.3 = 9,6, pas {icone}");

        // Remise en place pour les autres tests de ce fil.
        poser_hauteur_des_planchers(HAUTEUR_DES_PLANCHERS);
        poser_dpi_du_plancher_de_colonne(DPI_DU_PLANCHER_DE_COLONNE);
    }

    /// ⛔ **UNE ICÔNE AU MILIEU DOIT AVOIR UN RECTANGLE** — le test de la disparition.
    ///
    /// Mesuré le 2026-10-30 : l'utilisateur déplace l'applet réseau vers le milieu, elle est
    /// écrite dans `zones.toml` (`icones_milieu = ["noyau:reseau"]`) et **n'apparaît nulle
    /// part**. `Side::TopMiddle` manquait dans `Self::ZONES`, la liste des barres dont on
    /// calcule les emplacements : la variante existait, `layout_zone` savait la disposer,
    /// mais personne ne lui demandait jamais de le faire.
    #[test]
    fn an_icon_in_the_middle_gets_a_rectangle() {
        let layout = DockLayout {
            total_width: 1920.0,
            total_height: 1080.0,
            top_thickness: 40.0,
            haut_colonnes: 40.0,
            left_width: 0.0,
            right_width: 0.0,
            bottom_thickness: 0.0,
        };
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.poser_noms(&[(1, "presse-papiers")]);
        dock.set_clock_width(120.0);
        let mut zones = Zones::default();
        zones.bars.entry(BarLetter::Haut).or_default().icones_milieu = vec!["noyau:presse-papiers".to_string()];
        assert!(dock.appliquer_les_zones(&zones), "le milieu doit s'appliquer");

        // Act
        let rect = dock.icon_rect(1, &layout);

        // Assert — elle a un CÔTÉ (sinon `render_instances` la saute en silence), une
        // place, et elle est À GAUCHE de l'horloge.
        assert_eq!(dock.icon_side(1), Some(Side::TopMiddle), "sans cote, l'icone n'est PAS dessinee");
        let [x, _, w, _] = rect.expect("une icone du MILIEU doit avoir un rectangle");
        let bord_horloge = (1920.0 - 120.0) * 0.5;
        assert!(x + w <= bord_horloge, "l'icone ({x}+{w}) doit finir avant l'horloge ({bord_horloge})");
    }

    /// ⭐ **LE CENTRE DE LA BARRE DU HAUT EST UNE CIBLE** — sans quoi un glisser au milieu
    /// tombait dans la moitié gauche ou droite, et `icones_milieu` restait vide par
    /// construction (constat de l'utilisateur, 2026-10-30 : « la zone milieu doit être
    /// utilisée »).
    #[test]
    fn the_centre_of_the_top_bar_is_the_middle_zone() {
        let layout = DockLayout {
            total_width: 1920.0,
            total_height: 1080.0,
            top_thickness: 40.0,
            haut_colonnes: 40.0,
            left_width: 0.0,
            right_width: 0.0,
            bottom_thickness: 0.0,
        };
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);

        // Sans horloge connue : le partage d'avant, à la moitié (rien ne change).
        assert_eq!(dock.zone_at(900.0, 10.0, &layout), Some(Side::TopLeft));
        assert_eq!(dock.zone_at(1000.0, 10.0, &layout), Some(Side::TopRight));

        // Avec l'horloge : le centre est le MILIEU, et les bords restent les moitiés.
        dock.set_clock_width(120.0);
        assert_eq!(dock.zone_at(960.0, 10.0, &layout), Some(Side::TopMiddle));
        assert_eq!(dock.zone_at(200.0, 10.0, &layout), Some(Side::TopLeft));
        assert_eq!(dock.zone_at(1800.0, 10.0, &layout), Some(Side::TopRight));
        // Et la bande du milieu ne mange pas les moitiés : à 400 px du centre, on est
        // déjà dans la moitié gauche.
        assert_eq!(dock.zone_at(560.0, 10.0, &layout), Some(Side::TopLeft));
    }

    /// `barre_sous` voit les CINQ barres — la barre du haut comprise, que
    /// `side_at` ignore (le clic molette y était muet, 2026-11-01).
    #[test]
    fn barre_sous_voit_la_barre_du_haut_et_celle_du_bas() {
        let layout = DockLayout {
            total_width: 1920.0,
            total_height: 1080.0,
            top_thickness: 40.0,
            haut_colonnes: 40.0,
            left_width: 60.0,
            right_width: 60.0,
            bottom_thickness: 50.0,
        };
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.set_clock_width(120.0);
        assert_eq!(dock.side_at(200.0, 10.0, &layout), None, "le constat : side_at ignore le haut");
        assert_eq!(dock.barre_sous(200.0, 10.0, &layout), Some(Side::TopLeft));
        assert_eq!(dock.barre_sous(960.0, 10.0, &layout), Some(Side::TopMiddle));
        assert_eq!(dock.barre_sous(1800.0, 10.0, &layout), Some(Side::TopRight));
        assert_eq!(dock.barre_sous(960.0, 1060.0, &layout), Some(Side::Bottom));
        assert_eq!(dock.barre_sous(20.0, 500.0, &layout), Some(Side::Left));
        assert_eq!(dock.barre_sous(960.0, 500.0, &layout), None);
    }

    /// ⭐ **LE MILIEU SE POSE À GAUCHE DE L'HORLOGE** — décision de l'utilisateur du
    /// 2026-10-30 : « le milieu est l'horloge et les applets qui sont autour de l'horloge ».
    /// Sans la largeur de l'horloge, la rangée se posait SUR elle (le centre de l'écran est
    /// le centre de l'horloge).
    #[test]
    fn the_middle_row_sits_left_of_the_clock() {
        // Arrange — deux icônes au milieu, une horloge de 120 px de large.
        let layout = DockLayout {
            total_width: 1920.0,
            total_height: 1080.0,
            top_thickness: 40.0,
            haut_colonnes: 40.0,
            left_width: 0.0,
            right_width: 0.0,
            bottom_thickness: 0.0,
        };
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let a = dock.add_spacer_at_end(Side::TopMiddle);
        let b = dock.add_spacer_at_end(Side::TopMiddle);
        let _ = (a, b);

        // Act — SANS largeur d'horloge (première frame) : centrage d'avant.
        let ordre = [a, b];
        let mut sans = vec![None; dock.icons.len()];
        dock.layout_zone(Side::TopMiddle, &ordre, &layout, &mut sans);
        // Puis AVEC.
        dock.set_clock_width(120.0);
        let mut avec = vec![None; dock.icons.len()];
        dock.layout_zone(Side::TopMiddle, &ordre, &layout, &mut avec);

        // Assert — la rangée finit AVANT l'horloge, à la marge de la note et à l'écart près.
        let taille = top_icon_size_for(layout.top_thickness);
        let gap = dock.icon_gap_side(Side::TopMiddle);
        let bord_attendu = (1920.0 - 120.0) * 0.5 - crate::ui::topbar::NOTE_MARGIN_RIGHT - taille - gap;
        let droite_rangee = avec[a].map(|r| r[0] + r[2]).expect("la premiere icone doit etre posee");
        assert!(
            (droite_rangee - bord_attendu).abs() < 0.01,
            "la rangee doit finir a {bord_attendu}, elle finit a {droite_rangee}"
        );
        // Et sans largeur connue, le comportement d'avant est INTACT : la rangée part du
        // centre de l'écran, donc elle finit bien plus à droite.
        let droite_sans = sans[a].map(|r| r[0] + r[2]).expect("posee");
        assert!(
            droite_sans > droite_rangee + 40.0,
            "sans horloge connue, la rangee reste au centre ({droite_sans}) — elle ne doit pas bouger"
        );
    }

    fn geometry_layout() -> DockLayout {
        DockLayout { total_width: 3840.0, total_height: 2088.0, top_thickness: 72.0, haut_colonnes: 72.0, left_width: 140.0, right_width: 140.0, bottom_thickness: 0.0 }
    }

    /// Le fondu part du HAUT de la colonne de gauche, sur sa largeur ; rien sans
    /// hauteur ni colonne, et jamais plus haut que la colonne elle-même.
    #[test]
    fn la_bande_de_fondu_coiffe_la_colonne_de_gauche() {
        let layout = geometry_layout();
        let gauche = |l: &DockLayout, h: f32| bandes_de_fondu_de(l, [h, 0.0])[0];
        assert_eq!(gauche(&layout, 200.0), Some([0.0, 72.0, 140.0, 200.0]));
        assert_eq!(gauche(&layout, 0.0), None, "0 : désactivé");
        assert_eq!(gauche(&DockLayout { left_width: 0.0, ..layout }, 200.0), None, "pas de colonne");
        assert_eq!(gauche(&layout, 9000.0).map(|b| b[3]), Some(2088.0 - 72.0));
        let sans_haut = DockLayout { haut_colonnes: 0.0, ..layout };
        assert_eq!(gauche(&sans_haut, 100.0), Some([0.0, 0.0, 140.0, 100.0]), "barre du haut masquée : depuis le bord");
    }

    /// La barre de droite a SA bande, à son bord, avec SA hauteur.
    #[test]
    fn la_bande_de_droite_coiffe_la_colonne_de_droite() {
        let layout = geometry_layout();
        assert_eq!(bandes_de_fondu_de(&layout, [0.0, 300.0]), [None, Some([3840.0 - 140.0, 72.0, 140.0, 300.0])]);
        assert_eq!(bandes_de_fondu_de(&layout, [100.0, 300.0])[0].map(|b| b[3]), Some(100.0), "chacune sa hauteur");
        assert_eq!(bandes_de_fondu_de(&DockLayout { right_width: 0.0, ..layout }, [0.0, 300.0])[1], None, "pas de colonne");
    }

    /// ⭐ DEMANDE EXPLICITE (2026-10-16) : la texture de l'interface sur les
    /// indicateurs d'application — l'aplat ET l'anneau.
    #[test]
    fn l_indicateur_porte_la_texture_de_l_interface() {
        for forme in [RunningDotShape::Ring, RunningDotShape::Circle] {
            let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
            dock.set_running_dot_shape(forme);
            let layout = DockLayout { total_width: 1920.0, total_height: 1080.0, top_thickness: 72.0, haut_colonnes: 72.0, left_width: 72.0, right_width: 72.0, bottom_thickness: 0.0 };
            let instance = dock.running_dot_instance(2, &layout, None).expect("un indicateur");
            assert_eq!(instance.tex_mode, 5.0, "{forme:?}");
        }
    }

    /// Les réglages de l'ombre sont BORNÉS — une valeur absurde ne noircit pas tout.
    #[test]
    fn l_ombre_est_bornee() {
        assert_eq!(borner_ombre(-5.0), 0.0);
        assert_eq!(borner_ombre(9000.0), MAX_OMBRE_BARRE_HAUT);
        assert_eq!(borner_ombre(f32::NAN), DEFAULT_OMBRE_BARRE_HAUT);
        assert_eq!(borner_force_ombre(3.0), 1.0);
        assert_eq!(borner_force_ombre(-1.0), 0.0);
        assert_eq!(borner_force_ombre(f32::NAN), DEFAULT_OMBRE_BARRE_HAUT_FORCE);
    }

    /// Le dégradé passe d'une couleur de la palette à la suivante en fondu, et deux
    /// icônes voisines n'en sont pas au même point.
    #[test]
    fn le_degrade_de_l_indicateur_coule_d_une_icone_a_l_autre() {
        let palette = [[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]];
        assert_eq!(couleur_degradee_at(0.0, 0, &palette), [1.0, 0.0, 0.0], "au départ, la première");
        let mi = couleur_degradee_at(INDICATEUR_DEGRADE_MS / 1000.0 / 4.0, 0, &palette);
        assert!((mi[0] - 0.5).abs() < 1e-4 && (mi[2] - 0.5).abs() < 1e-4, "à mi-chemin, le mélange : {mi:?}");
        assert_eq!(couleur_degradee_at(INDICATEUR_DEGRADE_MS / 1000.0 / 2.0, 0, &palette), [0.0, 0.0, 1.0]);
        assert_ne!(couleur_degradee_at(0.0, 0, &palette), couleur_degradee_at(0.0, 1, &palette), "décalée d'une icône à l'autre");
        assert_eq!(couleur_degradee_at(3.0, 0, &[]), [1.0, 1.0, 1.0], "palette vide : du blanc, pas un panic");
    }

    /// ⛔ Sans accent de thème, le dégradé reste VIF : il ne prend pas la
    /// luminosité du fond (défaut du 2026-10-15 : pastilles presque noires).
    #[test]
    fn le_degrade_sans_accent_reste_en_couleurs() {
        let rouge = [1.0, 0.0, 0.0];
        let c = couleur_du_degrade(rouge, None);
        let (h, s, l) = color::rgb_to_hsl([c[0], c[1], c[2]]);
        assert!(h.abs() < 1.0 || (h - 360.0).abs() < 1.0, "la teinte du dégradé : {h}");
        assert!(s > 0.5 && l > 0.4 && l < 0.8, "vive et lisible : s {s}, l {l}");
        // Avec un accent, c'est le sien qui donne saturation et luminosité.
        let c = couleur_du_degrade(rouge, Some([0.2, 0.3, 0.2]));
        let (_, s2, l2) = color::rgb_to_hsl([c[0], c[1], c[2]]);
        let (_, sa, la) = color::rgb_to_hsl([0.2, 0.3, 0.2]);
        assert!((s2 - sa).abs() < 1e-3 && (l2 - la).abs() < 1e-3);
    }

    /// `[1, 1, 0]` ne touche à rien ; chaque curseur fait ce qu'il dit.
    #[test]
    fn les_curseurs_de_l_indicateur_font_ce_qu_ils_disent() {
        let rouge = [0.8, 0.2, 0.2, 1.0];
        let telle = ajuster_indicateur(rouge, [1.0, 1.0, 0.0]);
        assert!(telle.iter().zip(rouge).all(|(a, b)| (a - b).abs() < 1e-4), "{telle:?}");
        let gris = ajuster_indicateur(rouge, [0.0, 1.0, 0.0]);
        assert!((gris[0] - gris[1]).abs() < 1e-4 && (gris[1] - gris[2]).abs() < 1e-4, "saturation 0 : un gris");
        assert_eq!(ajuster_indicateur(rouge, [1.0, 0.0, 0.0])[..3], [0.0, 0.0, 0.0], "luminosité 0 : noir");
        assert_eq!(ajuster_indicateur(rouge, [1.0, 2.0, 0.0])[..3], [1.0, 1.0, 1.0], "luminosité 200 % : blanc");
        assert_eq!(ajuster_indicateur(rouge, [1.0, 1.0, 0.75])[3], 0.25, "transparence 75 %");
    }

    /// Les réglages sont BORNÉS, et un `NaN` revient au neutre.
    #[test]
    fn les_reglages_de_l_indicateur_sont_bornes() {
        set_indicateur_reglage(0, 9.0);
        set_indicateur_reglage(2, f32::NAN);
        assert_eq!(indicateur_reglages(), [2.0, 1.0, 0.0]);
        set_indicateur_reglage(0, 1.0);
        set_indicateur_reglage(7, 0.5);
        assert_eq!(indicateur_reglages(), [1.0, 1.0, 0.0], "un rang inconnu ne touche à rien");
    }

    /// Le contrat du décalage est de TRANSLATER, jamais de redimensionner :
    /// « décaler pour ne pas être tronquée » ne doit pas se traduire par
    /// « rétrécir l'icône ».
    #[test]
    fn clamp_by_translation_does_not_resize() {
        let bounds = [0.0, 0.0, 1000.0, 500.0];
        for rect in [[-50.0, 10.0, 120.0, 120.0], [950.0, 460.0, 120.0, 120.0], [10.0, 10.0, 120.0, 120.0]] {
            let clamped = clamp_by_translation(rect, bounds);
            assert_eq!((clamped[2], clamped[3]), (rect[2], rect[3]), "taille inchangée pour {rect:?}");
        }
    }

    /// Boîte plus grande que la fenêtre : elle reste collée en haut/gauche, faute
    /// de mieux — c'est le contrat que `app::menu_rect` documente aussi.
    #[test]
    fn clamp_by_translation_sticks_to_the_top_left_when_bigger_than_bounds() {
        let clamped = clamp_by_translation([10.0, 20.0, 5000.0, 5000.0], [0.0, 0.0, 100.0, 100.0]);
        assert_eq!(clamped, [0.0, 0.0, 5000.0, 5000.0]);
    }

    // Tolérance des tests de rectangle présenté : la boîte occupée se déduit de la
    // boîte présentée par une multiplication, et celle-ci de la première par la
    // multiplication inverse — l'aller-retour laisse une erreur d'arrondi d'un
    // micro-pixel, sans aucune conséquence (la région, elle, est arrondie à
    // l'entier).
    //
    // ⚠️ **DÉCLARÉE ICI, ET PLUS AU MILIEU DU MODULE** : elle vivait entre deux
    // tests, et remplacer ce voisinage l'a emportée avec lui — trois tests ne
    // compilaient plus. Une constante partagée par plusieurs tests se pose AVANT
    // eux.
    const TOLERANCE: f32 = 0.01;

    /// À l'échelle du minimum (1.0 = aucun effet), le rectangle présenté EST la
    /// case : pas de magnification, pas de déplacement. Même esprit que
    /// `hover_scale_minimum_is_a_no_op`.
    #[test]
    fn presented_rect_at_scale_one_is_the_grid_rect() {
        let layout = geometry_layout();
        for rect in [[22.0, 88.0, 96.0, 96.0], [16.0, 12.0, 48.0, 48.0], [3776.0, 12.0, 48.0, 48.0]] {
            assert_eq!(presented_rect_for(rect, 1.0, &layout), rect);
        }
    }

    /// Barre du HAUT : l'icône grossie est ramenée vers le BAS. Le haut de sa
    /// boîte sortirait de l'écran, et celui-ci n'est récupérable par RIEN — le
    /// rectangle de la barre du haut part de `y = 0` (voir `set_dock_region`).
    /// Le test vérifie donc les deux moitiés de la demande : plus de coupe au
    /// bord, et un débordement SOUS la barre.
    #[test]
    fn presented_rect_shifts_the_top_bar_down() {
        let layout = geometry_layout();
        let rect = [16.0, 12.0, 48.0, 48.0];
        let presented = presented_rect_for(rect, MAX_HOVER_SCALE, &layout);
        // Voir `presented_rect_shifts_the_left_bar_right` : c'est la boîte
        // OCCUPÉE qui est ramenée au bord, l'icône gardant de quoi tourner.
        let occupied = occupied_rect_for(presented);
        assert!(occupied[1].abs() < TOLERANCE, "boîte occupée au bord supérieur : {}", occupied[1]);
        assert!(
            occupied[1] + occupied[3] > layout.top_thickness,
            "et elle déborde bien sous la barre du haut"
        );
    }

    /// **UN RÉGLAGE PAR BARRE, ET ILS SONT INDÉPENDANTS.** DEMANDE EXPLICITE :
    /// « le décalage classique soit réglable pour le dock et les autres barres
    /// individuellement ». C'est LA propriété qui justifie trois cellules plutôt
    /// qu'une — et elle ne se voit que si on la teste, parce qu'un réglage qui
    /// déborde sur ses voisins se comporte exactement comme un réglage global.
    ///
    /// ⚠️ **LES BARRES N'ONT PAS LES MÊMES CONTRAINTES**, et c'est ce qui justifie
    /// de pouvoir les régler séparément : celle du haut part de `y = 0`, où rien ne
    /// rattrape une icône sortie par le haut, alors qu'une barre latérale a de la
    /// place au-dessus et en dessous.
    #[test]
    fn les_tirages_des_trois_barres_sont_independants() {
        let layout = geometry_layout();
        let icon = 96.0;
        let avant = BarreTirage::TOUTES.map(tirage_vers_le_centre);

        // On règle la barre du HAUT, et rien d'autre.
        set_tirage_vers_le_centre(BarreTirage::Haut, 0.0);
        assert_eq!(tirage_vers_le_centre(BarreTirage::Haut), 0.0);
        assert_eq!(tirage_vers_le_centre(BarreTirage::Gauche), avant[1], "la barre de gauche a bougé");
        assert_eq!(tirage_vers_le_centre(BarreTirage::Droite), avant[2], "la barre de droite a bougé");

        // ⚠️ **ET L'ÉCART SE VOIT SUR LE RECTANGLE**, pas seulement sur la valeur :
        // un réglage lu mais jamais appliqué passerait le test ci-dessus.
        let centre = |r: [f32; 4]| r[0] + r[2] * 0.5;
        let haut = [layout.total_width - TOP_ZONE_CORNER_MARGIN - icon, 12.0, icon, icon];
        let gauche = [(layout.left_width - icon) * 0.5, 88.0, icon, icon];
        let p_haut = presented_rect_pour_la_barre(haut, MAX_HOVER_SCALE, &layout, BarreTirage::Haut);
        let p_gauche = presented_rect_pour_la_barre(gauche, MAX_HOVER_SCALE, &layout, BarreTirage::Gauche);
        assert!((centre(p_haut) - centre(haut)).abs() < TOLERANCE, "à 0 %, la barre du haut ne décale plus");
        assert!(centre(p_gauche) > centre(gauche) + 1.0, "à 100 %, celle de gauche décale toujours");

        for barre in BarreTirage::TOUTES {
            set_tirage_vers_le_centre(barre, DEFAULT_TIRAGE);
        }
    }

    /// **LE TIRAGE DES ICÔNES AGRANDIES — LES TROIS CAS DU RÉGLAGE.**
    ///
    /// DEMANDE EXPLICITE du 2026-09-24, **EN TROIS TEMPS** : « les icones ... qui sont
    /// au bord de l'écran se décalent à gauche, peux-tu enlever ce déplacement » ;
    /// puis, une fois retiré, « je veux que le décalage des icônes du dock se
    /// décalent et que ce soit réglable ». Le comportement est donc devenu un
    /// RÉGLAGE (`ui::dock::DEFAULT_TIRAGE` en porte l'histoire), et ces tests en
    /// décrivent les deux bouts et le milieu.
    ///
    /// ⚠️ **`with_tirage` EST OBLIGATOIRE DANS CHAQUE TEST** : la cellule est
    /// GLOBALE, donc un test qui ne la pose pas dépendrait de l'ordre d'exécution —
    /// et le défaut (1.0) n'est pas forcément celui qu'il veut éprouver.
    #[test]
    fn le_tirage_a_un_ramene_l_icone_entièrement_dans_la_fenetre() {
        let layout = geometry_layout();
        let icon = 96.0;
        with_tirage(1.0, || {
            for (rect, ou) in [
                ([(layout.left_width - icon) * 0.5, 88.0, icon, icon], "gauche"),
                ([layout.total_width - layout.right_width + (layout.right_width - icon) * 0.5, 88.0, icon, icon], "droite"),
                ([TOP_ZONE_CORNER_MARGIN, 12.0, icon, icon], "haut-gauche"),
                ([layout.total_width - TOP_ZONE_CORNER_MARGIN - icon, 12.0, icon, icon], "haut-droite"),
            ] {
                let p = presented_rect_for(rect, MAX_HOVER_SCALE, &layout);
                let occupee = occupied_rect_for(p);
                assert!(occupee[0] >= -TOLERANCE, "{ou} : rien ne sort à gauche ({occupee:?})");
                assert!(occupee[1] >= -TOLERANCE, "{ou} : rien ne sort en haut ({occupee:?})");
                assert!(occupee[0] + occupee[2] <= layout.total_width + TOLERANCE, "{ou} : ni à droite ({occupee:?})");
                assert!(occupee[1] + occupee[3] <= layout.total_height + TOLERANCE, "{ou} : ni en bas ({occupee:?})");
            }
        });
    }

    #[test]
    fn le_tirage_a_zero_laisse_l_icone_centree_meme_si_elle_depasse() {
        let layout = geometry_layout();
        let icon = 96.0;
        with_tirage(0.0, || {
            let rect = [layout.total_width - TOP_ZONE_CORNER_MARGIN - icon, 12.0, icon, icon];
            let p = presented_rect_for(rect, MAX_HOVER_SCALE, &layout);
            let centre_avant = rect[0] + rect[2] * 0.5;
            let centre_apres = p[0] + p[2] * 0.5;
            assert!(
                (centre_apres - centre_avant).abs() < TOLERANCE,
                "centrée sur sa case : {rect:?} -> {p:?}"
            );
            // La troncature est la CONSÉQUENCE du choix, pas un défaut à corriger
            // discrètement plus tard.
            assert!(p[0] + p[2] > layout.total_width, "elle sort à droite : {p:?}");
        });
    }

    /// **ET LE MILIEU INTERPOLE LA POSITION, JAMAIS LA TAILLE** — l'icône grandit
    /// pareil à 0 %, à 50 % et à 100 % ; seule sa place change. Interpoler la taille
    /// ferait qu'un tirage plus faible RAPETISSERAIT l'icône, ce que personne n'a
    /// demandé.
    #[test]
    fn le_tirage_interpole_la_position_sans_toucher_la_taille() {
        let layout = geometry_layout();
        let icon = 96.0;
        let rect = [layout.total_width - TOP_ZONE_CORNER_MARGIN - icon, 12.0, icon, icon];
        let mut a_zero = [0.0; 4];
        let mut a_un = [0.0; 4];
        let mut a_moitie = [0.0; 4];
        with_tirage(0.0, || a_zero = presented_rect_for(rect, MAX_HOVER_SCALE, &layout));
        with_tirage(1.0, || a_un = presented_rect_for(rect, MAX_HOVER_SCALE, &layout));
        with_tirage(0.5, || a_moitie = presented_rect_for(rect, MAX_HOVER_SCALE, &layout));

        // Même taille dans les trois cas.
        assert!((a_zero[2] - a_un[2]).abs() < TOLERANCE && (a_moitie[2] - a_un[2]).abs() < TOLERANCE, "la taille ne dépend pas du tirage");
        // Et la position tombe au MILIEU des deux autres.
        assert!(
            (a_moitie[0] - (a_zero[0] + a_un[0]) * 0.5).abs() < TOLERANCE,
            "50 % tombe entre les deux : {} attendu, {} obtenu",
            (a_zero[0] + a_un[0]) * 0.5,
            a_moitie[0]
        );
    }

    /// **À 100 %, `presented_rect_for` REDEVIENT EXACTEMENT `presented_rect_in`** —
    /// c'est ce qui rend le comportement historique retrouvable, et c'est ce que le
    /// test de délégation d'origine affirmait avant que je ne le remplace à tort.
    /// ⚠️ L'ÉGALITÉ DÉPEND DU RÉGLAGE : à 0 %, les deux divergent (voir le test
    /// ci-dessus) — c'est bien pourquoi la façade a dû être réécrite.
    #[test]
    fn au_tirage_maximal_la_facade_egale_presented_rect_in() {
        let layout = geometry_layout();
        with_tirage(1.0, || {
            for rect in [
                [22.0, 88.0, 96.0, 96.0],
                [layout.total_width - 70.0, 12.0, 96.0, 96.0],
                [16.0, 12.0, 48.0, 48.0],
            ] {
                for scale in [1.0, 1.3, MAX_HOVER_SCALE] {
                    assert_eq!(
                        presented_rect_for(rect, scale, &layout),
                        presented_rect_in(rect, scale, [0.0, 0.0, layout.total_width, layout.total_height]),
                        "rect {rect:?} à {scale}"
                    );
                }
            }
        });
    }

    /// **UNE ICÔNE AU REPOS N'EST JAMAIS DÉPLACÉE** (échelle 1.0, ou moins) — c'est
    /// la propriété qui rend le DÉFILEMENT possible : un emplacement sorti de la
    /// fenêtre doit y rester, sinon la barre recollerait au bord toutes les icônes
    /// qu'elle fait sortir.
    ///
    /// ⚠️ **SON ATTRIBUT `#[test]` A ÉTÉ EMPORTÉ** le 2026-09-24, par un remplacement
    /// qui visait la fonction VOISINE — et le symptôme était exactement celui que ce
    /// dépôt documente : **le test compilait sans être enregistré** (`running 0
    /// tests`), donc il ne protégeait plus rien. Le compilateur le DISAIT (« function
    /// is never used ») et cet avertissement a failli passer pour du bruit. C'est le
    /// piège de `dock.rs`, et il ne se voit qu'en comptant les tests.
    #[test]
    fn presented_rect_in_leaves_a_resting_rect_alone() {
        let bounds = [0.0, 0.0, 3840.0, 2088.0];
        for rect in [[100.0, 200.0, 96.0, 96.0], [10.0, 2400.0, 96.0, 96.0], [-50.0, -50.0, 96.0, 96.0]] {
            assert_eq!(presented_rect_in(rect, 1.0, bounds), rect, "à l'échelle 1.0");
            assert_eq!(presented_rect_in(rect, 0.5, bounds), rect, "sous l'échelle 1.0");
        }
    }

    /// Le contrat du décalage est de TRANSLATER, jamais de redimensionner — y
    /// compris à travers le double passage par `WOBBLE_HEADROOM` (élargissement
    /// puis réduction). Miroir de `clamp_by_translation_does_not_resize`, un cran
    /// plus haut dans la chaîne.
    #[test]
    fn presented_rect_in_never_resizes() {
        let bounds = [0.0, 0.0, 3840.0, 2088.0];
        for rect in [[100.0, 200.0, 96.0, 96.0], [3700.0, 2000.0, 96.0, 96.0]] {
            for scale in [1.3_f32, 2.0, MAX_HOVER_SCALE] {
                let presented = presented_rect_in(rect, scale, bounds);
                let expected = [rect[2] * scale, rect[3] * scale];
                assert!((presented[2] - expected[0]).abs() < 0.01, "largeur {presented:?} vs {expected:?}");
                assert!((presented[3] - expected[1]).abs() < 0.01, "hauteur {presented:?} vs {expected:?}");
            }
        }
    }

    /// Le balancement est une fonction PURE du temps : même instant, même angle ;
    /// un tour complet plus tard, le même angle encore ; un demi-tour plus tard,
    /// l'angle opposé. C'est ce que `hover_wobble_angle` ne pouvait pas éprouver,
    /// faute de pouvoir injecter le temps — même découpage, et même raison, que
    /// la paire `notification_blink_color_at`/`notification_blink_color`.
    #[test]
    fn hover_wobble_angle_at_is_periodic_and_bounded() {
        // Le dénominateur de `progress` est passé EXPLICITEMENT (c'est le sens
        // du quatrième paramètre) : ce test ne dépend donc plus d'aucune cellule
        // globale, et n'a plus besoin d'en poser une.
        const FULL: f32 = 2.0;
        let mut opposites = 0;
        for step in 0..40u32 {
            let elapsed = HOVER_WOBBLE_PERIOD * step / 40;
            let angle = hover_wobble_angle_at(elapsed, 2.0, HOVER_WOBBLE_DEGREES, FULL);
            assert!(angle.abs() <= HOVER_WOBBLE_DEGREES.to_radians() + 1e-4, "hors bornes : {angle}");

            let later = hover_wobble_angle_at(elapsed + HOVER_WOBBLE_PERIOD, 2.0, HOVER_WOBBLE_DEGREES, FULL);
            assert!((angle - later).abs() < 1e-4, "période : {angle} puis {later}");

            // Un demi-tour plus tard il est de l'autre côté — sauf aux deux
            // instants où il repasse par zéro, qu'on ne compte pas.
            if angle.abs() > 0.05 {
                let opposite =
                    hover_wobble_angle_at(elapsed + HOVER_WOBBLE_PERIOD / 2, 2.0, HOVER_WOBBLE_DEGREES, FULL);
                assert!(angle * opposite < 0.0, "{angle} et {opposite} devraient être opposés");
                opposites += 1;
            }
        }
        assert!(opposites > 0, "le test doit avoir éprouvé au moins un couple opposé");
    }

    /// Le CÂBLAGE de `hover_text_wobble_angle` — la fonction que les DEUX
    /// horloges appellent réellement — et pas seulement la constante qu'elle est
    /// censée utiliser : sans ce test, remplacer `TEXT_WOBBLE_DEGREES` par
    /// `HOVER_WOBBLE_DEGREES` dans son corps ne ferait tomber aucun test, et des
    /// chiffres se mettraient à pivoter à 45°.
    ///
    /// La borne est vérifiée sur la fonction RÉELLE, donc sans pouvoir injecter
    /// le temps (`hover_wobble_angle_at` existe exactement pour ça) : elle ne
    /// peut jamais échouer à tort, et elle attrape quand même la mauvaise
    /// constante, dont l'amplitude dépasse la borne dès que le sinus n'est pas
    /// proche de zéro.
    #[test]
    fn hover_text_wobble_angle_is_wired_to_the_text_amplitude() {
        // Le réglage posé est celui des HORLOGES, et c'est important : depuis que
        // `hover_text_wobble_angle` lit `clock_hover_scale()` comme dénominateur,
        // ce test tournerait sinon avec la valeur par défaut de l'horloge. Il
        // passerait encore — `progress` étant borné à 1.0, une assertion « angle
        // ≤ 6° » ne peut pas échouer — mais il ne testerait plus rien.
        with_clock_hover_scale(2.0, || {
            assert_eq!(hover_text_wobble_angle(1.0), 0.0, "nul au repos");
            for scale in [1.2_f32, 1.5, 2.0] {
                let angle = hover_text_wobble_angle(scale).abs();
                assert!(angle <= TEXT_WOBBLE_DEGREES.to_radians() + 1e-4, "amplitude {angle} à l'échelle {scale}");
            }
        });
    }

    /// **Le test qui verrouille le dénominateur** — celui sans lequel relire le
    /// global des icônes dans `hover_text_wobble_angle` passerait inaperçu.
    ///
    /// L'assertion doit être prise À MI-COURSE et avec une horloge réglée PLUS
    /// GRANDE que les icônes, parce que c'est le seul régime où les deux
    /// dénominateurs diffèrent visiblement : `progress` est borné à 1.0, donc
    /// une horloge plus PETITE que les icônes plafonne de toute façon et un
    /// mauvais dénominateur y reste invisible. Ici, à l'échelle 1.9 sur une cible
    /// de 2.5, le bon `progress` vaut 0.6 — l'amplitude ne doit donc PAS être
    /// pleine. Avec le global des icônes laissé à 1.3, `progress` saturerait à
    /// 1.0 et l'angle atteindrait son maximum.
    #[test]
    fn text_wobble_progress_uses_the_clock_denominator() {
        let quarter_turn = HOVER_WOBBLE_PERIOD / 4; // le sinus y vaut 1
        let full = TEXT_WOBBLE_DEGREES.to_radians();

        let partway = hover_wobble_angle_at(quarter_turn, 1.9, TEXT_WOBBLE_DEGREES, 2.5).abs();
        assert!(
            partway < full * 0.99,
            "amplitude {partway} : `progress` a saturé au lieu de valoir 0,6 (plafond {full})"
        );
        assert!(partway > full * 0.5, "amplitude {partway} : `progress` a été écrasé");

        // Au bout de la croissance, l'amplitude est bien PLEINE — c'est l'autre
        // moitié du contrat, et elle interdit de « corriger » le problème en
        // divisant par autre chose.
        let complete = hover_wobble_angle_at(quarter_turn, 2.5, TEXT_WOBBLE_DEGREES, 2.5).abs();
        assert!((complete - full).abs() < 1e-4, "amplitude {complete} au lieu de {full}");
    }

    /// La marge de rotation du TEXTE doit couvrir l'amplitude à laquelle il
    /// balance réellement — c'est elle qui dimensionne la fenêtre de l'horloge
    /// plein écran (`overscan_padding`) ET la boîte de sa région
    /// (`occupied_text_rect_for`). Une marge trop juste rognerait les coins du
    /// texte au sommet de son balancement.
    #[test]
    fn text_wobble_headroom_covers_the_text_amplitude() {
        let extent = 200.0_f32;
        let rotated = extent * (TEXT_WOBBLE_DEGREES.to_radians().cos() + TEXT_WOBBLE_DEGREES.to_radians().sin());
        let headroom = extent * TEXT_WOBBLE_HEADROOM;

        assert!(
            headroom >= rotated,
            "la marge ({headroom}) ne couvre pas un carré penché à {TEXT_WOBBLE_DEGREES}° ({rotated})"
        );
        // Et elle doit rester PLUS SERRÉE que celle des icônes : c'est tout
        // l'objet de sa raison d'être.
        assert!(TEXT_WOBBLE_HEADROOM < WOBBLE_HEADROOM, "aucun intérêt à une marge dédiée si elle ne resserre rien");
    }

    /// Le balancement d'un TEXTE est volontairement bien plus faible que celui
    /// d'une icône (voir `TEXT_WOBBLE_DEGREES`) : des chiffres qui pivotent à 45°
    /// sont illisibles. Et les deux sont NULS au repos — sans quoi une icône ou
    /// une horloge non survolée tournerait sur elle-même.
    #[test]
    fn text_wobble_is_much_weaker_than_icon_wobble() {
        // Dénominateur explicite (voir `hover_wobble_angle_at`) : plus aucune
        // cellule globale n'entre ici.
        const FULL: f32 = 2.0;
        let quarter_turn = HOVER_WOBBLE_PERIOD / 4;
        let icon = hover_wobble_angle_at(quarter_turn, 2.0, HOVER_WOBBLE_DEGREES, FULL).abs();
        let text = hover_wobble_angle_at(quarter_turn, 2.0, TEXT_WOBBLE_DEGREES, FULL).abs();
        assert!(text > 0.0, "le texte doit tout de même balancer");
        assert!(text < icon / 4.0, "le texte ({text}) doit rester discret devant l'icône ({icon})");

        for scale in [1.0_f32, 0.5] {
            assert_eq!(hover_wobble_angle_at(quarter_turn, scale, HOVER_WOBBLE_DEGREES, FULL), 0.0);
            assert_eq!(hover_wobble_angle_at(quarter_turn, scale, TEXT_WOBBLE_DEGREES, FULL), 0.0);
        }
    }

    /// Une icône ENTIÈREMENT sortie de la fenêtre par le défilement ne doit
    /// plus avoir de rectangle du tout — et non se faire ramener dans la
    /// fenêtre, collée au bord bas, par-dessus l'icône qui occupe
    /// légitimement cette place. DÉFAUT RÉEL SIGNALÉ : « la dernière icône ne
    /// se cache pas au défilement, elle reste affichée en bas de barre
    /// par-dessus le reste alors que la barre est remplie d'icônes » — les
    /// icônes d'applications ouvertes, ajoutées en FIN de barre, sont
    /// exactement celles qui sortent par le bas.
    #[test]
    fn scrolled_out_icons_are_not_dragged_back_into_the_window() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        // Bien plus d'icônes que la barre n'en tient : le défilement devient
        // nécessaire, et les dernières sortent par le bas.
        dock.set_dynamic_apps(40);
        let first = dock.left_order[0];
        let last = *dock.left_order.last().expect("la barre n'est pas vide");

        assert!(dock.icon_rect(first, &layout).is_some(), "la première icône est en haut de la barre");
        assert!(dock.icon_rect(last, &layout).is_none(), "la dernière doit être HORS CHAMP, pas ramenée dedans");

        // L'INVARIANT QUI MANQUAIT : sans survol, ce qui est DESSINÉ est la case
        // de la grille, au pixel près. C'est ce que le rappel-à-l'écran de
        // `presented_rect_for` violait pour l'icône à CHEVAL sur le bord bas —
        // écartée par personne (elle est partiellement visible), elle était
        // translatée d'une hauteur d'icône vers le haut et recouvrait sa voisine.
        // C'est le symptôme signalé, et il survit à la seule exclusion des
        // emplacements entièrement hors champ.
        for i in 0..dock.icons.len() {
            assert_eq!(dock.presented_rect(i, &layout), dock.icon_rect(i, &layout), "icône {i} : décalée sans survol");
        }

        // Jusqu'au bout : c'est le défilement qui la fait apparaître, et elle
        // doit alors tenir ENTIÈREMENT dans la fenêtre.
        // `y` : le curseur est censé être DANS la barre. Peu importe où ici —
        // ce dock n'a aucun séparateur de zone, donc aucune zone ne peut le
        // retenir (voir `Dock::scroll_bar`, `zone_separator_at`), et c'est bien
        // la BARRE ENTIÈRE que ce test fait défiler.
        let y = layout.top_thickness + 100.0;
        for _ in 0..20 {
            dock.scroll_bar(Side::Left, -1000.0, y, &layout);
        }
        let rect = dock.icon_rect(last, &layout).expect("en défilant jusqu'au bout, la dernière réapparaît");
        assert!(rect[1] + rect[3] <= layout.total_height + 0.001, "elle doit tenir dans la fenêtre : {rect:?}");
        for i in 0..dock.icons.len() {
            assert_eq!(dock.presented_rect(i, &layout), dock.icon_rect(i, &layout), "icône {i} : décalée après défilement");
        }

        // Même règle à l'autre bout : la première est sortie par le HAUT, donc
        // écartée elle aussi. Le DESSIN ne change pas — elle était déjà coupée
        // par `lateral_clip` — mais le CLIC si : sans cette exclusion, elle
        // restait atteignable dans la bande qu'elle occupe hors de la fenêtre,
        // sous la barre du haut, où la souris peut se trouver, et s'y faisait
        // magnifier — une bande de sa boîte grossie dépassait alors par-dessus
        // la barre du haut.
        assert!(dock.icon_rect(first, &layout).is_none(), "la première est sortie par le haut");
    }

    /// La hauteur du contenu change SANS la molette — une application qui se
    /// ferme, une icône désactivée dans les Réglages, un espace redimensionné,
    /// une épaisseur de barre modifiée : le décalage doit redescendre tout seul,
    /// sinon un trou s'installe sous la dernière icône jusqu'au prochain cran.
    /// C'est `Dock::update`, une fois par frame, qui le garantit.
    /// ⛔ **LA PARTIE CACHÉE D'UNE ICÔNE N'EST NI SURVOLABLE NI CLIQUABLE.**
    /// Défaut SIGNALÉ (2026-10-15) : « quand les icônes de la barre de gauche ont
    /// défilé et que je survole l'icône pour afficher le bureau, je vois dessous
    /// l'icône cachée ». L'icône à cheval sur le haut de la colonne gardait sa
    /// case ENTIÈRE pour le survol : sa moitié passée sous la barre du haut
    /// recouvrait « Afficher le bureau », s'y agrandissait, et réapparaissait. Le
    /// clic, testé avant celui de la barre du haut, l'aurait LANCÉE.
    #[test]
    fn la_partie_cachee_d_une_icone_defilee_n_est_ni_survolee_ni_cliquee() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.set_dynamic_apps(40);
        let haut = layout.haut_colonnes;
        // Un décalage qui laisse une icône À CHEVAL sur le haut de la colonne.
        let (index, rect) = (1..400)
            .find_map(|d| {
                dock.scroll[0] = d as f32;
                let rects = dock.icon_rects_map(&layout);
                rects.iter().enumerate().find_map(|(i, r)| {
                    r.filter(|r| dock.icon_side(i) == Some(Side::Left) && r[1] + 4.0 < haut && r[1] + r[3] > haut + 4.0).map(|r| (i, r))
                })
            })
            .expect("prémisse : une icône à cheval sur le haut de la colonne");
        let x = rect[0] + rect[2] * 0.5;
        let cache = (x, (rect[1] + haut) * 0.5);
        let visible = (x, (haut + rect[1] + rect[3]) * 0.5);
        let hwnd = HWND(std::ptr::null_mut());
        assert_ne!(dock.icon_at(cache.0, cache.1, &layout, hwnd), Some(index), "la moitié cachée n'est pas cliquable");
        assert_eq!(dock.icon_at(visible.0, visible.1, &layout, hwnd), Some(index), "la moitié visible l'est");
        dock.on_mouse_move(cache.0, cache.1, hwnd);
        dock.update(0.5, &layout);
        assert_eq!(dock.hover.scale(index), 1.0, "survolée par sa moitié cachée, elle ne grossit pas");
        dock.on_mouse_move(visible.0, visible.1, hwnd);
        dock.update(0.5, &layout);
        assert!(dock.hover.scale(index) > 1.0, "par sa moitié visible, si");
    }

    // ---- ⭐ Le repliement façon Unity des barres latérales (voir `ui::empilement`) ----

    const REPLIEMENT_ACTIF: crate::ui::empilement::Empilement =
        crate::ui::empilement::Empilement { actif: true, duree_ms: 200 };

    /// Dock dont la barre de GAUCHE déborde, repliement imposé.
    fn dock_qui_deborde(reglage: crate::ui::empilement::Empilement) -> Dock {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        // Un débordement MODÉRÉ : au-delà, même replié au quart, tout ne tient
        // plus et Unity replie jusqu'au haut de la barre. ⚠️ **LE NOMBRE SUIT LE
        // NOMBRE D'APPLETS** : chaque applet ajoutée au dock allonge la barre d'une
        // case, et c'est ici qu'on rend la place — sinon le repliement maximal ne
        // suffit plus à tout faire tenir (mesuré à l'ajout de la Patience,
        // 2026-11-10 : 8 épingles dynamiques débordaient encore d'une case).
        dock.set_dynamic_apps(7);
        dock.empilement_force = Some(reglage);
        dock
    }

    /// Les icônes (pas les espaces) de la barre de gauche, dans l'ordre, et
    /// leur rectangle.
    fn rects_de_gauche(dock: &Dock, layout: &DockLayout) -> Vec<(usize, Option<[f32; 4]>)> {
        let rects = dock.icon_rects_map(layout);
        dock.left_order.iter().filter(|&&i| !dock.icons[i].is_spacer && !dock.icons[i].hidden).map(|&i| (i, rects[i])).collect()
    }

    fn souris_et_temps(dock: &mut Dock, layout: &DockLayout, x: f32, y: f32) {
        dock.on_mouse_move(x, y, HWND(std::ptr::null_mut()));
        for _ in 0..20 {
            dock.update(0.05, layout);
        }
    }

    #[test]
    fn au_repos_le_haut_reste_en_place_et_le_bas_se_replie_dans_l_ecran() {
        let layout = geometry_layout();
        let droite = dock_qui_deborde(crate::ui::empilement::Empilement::DEFAUT);
        assert!(droite.pliage_de_la_barre(Side::Left, &layout).is_none());
        assert!(rects_de_gauche(&droite, &layout).iter().any(|(_, r)| r.is_none()), "prémisse : la barre déborde");

        let dock = dock_qui_deborde(REPLIEMENT_ACTIF);
        assert!(dock.pliage_de_la_barre(Side::Left, &layout).is_some());
        let rects = rects_de_gauche(&dock, &layout);
        let (premiere, r0) = rects[0];
        assert_eq!(r0, droite.icon_rect(premiere, &layout), "la première ne bouge pas");
        assert!(dock.bascule_de(premiere, &layout).is_none());
        // TOUTES les icônes sont à l'écran, la dernière repliée au maximum.
        for (i, r) in &rects {
            let r = r.unwrap_or_else(|| panic!("l'icône {i} est à l'écran"));
            assert!(r[1] + r[3] <= layout.total_height + 1.0, "{i} : {r:?}");
        }
        let (derniere, rd) = *rects.last().unwrap();
        let (entiere, angle) = dock.bascule_de(derniere, &layout).expect("basculée");
        assert!((angle - crate::ui::empilement::BASCULE_MAX).abs() < 1e-4);
        assert_eq!(entiere[2], entiere[3], "dessinée entière, carrée");
        assert!(rd.unwrap()[3] < entiere[3] * 0.6, "sa case est raccourcie");
        // Et c'est ce que reçoit le dessin.
        let instance = dock.external_icon_instance(derniere, &layout).unwrap();
        assert_eq!((instance.rect, instance.bascule), (entiere, angle));
        assert_eq!(dock.external_icon_instance(premiere, &layout).unwrap().bascule, 0.0);
    }

    #[test]
    fn au_survol_tout_se_deplie_a_sa_place_et_la_molette_fait_defiler() {
        let layout = geometry_layout();
        let droite = dock_qui_deborde(crate::ui::empilement::Empilement::DEFAUT);
        let mut dock = dock_qui_deborde(REPLIEMENT_ACTIF);
        // Entrée par le HAUT de la barre : pas de défilement d'entrée.
        souris_et_temps(&mut dock, &layout, 20.0, layout.haut_colonnes + 30.0);
        assert_eq!(dock.pli_survol[0], 1.0);
        for (i, r) in rects_de_gauche(&dock, &layout) {
            assert!(dock.bascule_de(i, &layout).is_none(), "{i} dépliée");
            assert_eq!(r, droite.icon_rect(i, &layout), "{i} à sa place de barre droite");
        }
        // La molette vers le bas remonte la barre dépliée, jusqu'à poser la
        // dernière en bas.
        let premiere = rects_de_gauche(&dock, &layout)[0];
        dock.scroll_bar(Side::Left, -WHEEL_DELTA, 500.0, &layout);
        let apres = dock.icon_rect(premiere.0, &layout);
        assert!(apres.is_none_or(|r| r[1] < premiere.1.unwrap()[1]), "elle a remonté");
        for _ in 0..500 {
            dock.scroll_bar(Side::Left, -WHEEL_DELTA, 500.0, &layout);
        }
        let derniere = rects_de_gauche(&dock, &layout).last().unwrap().1.expect("la dernière se voit");
        assert!((derniere[1] + derniere[3] - layout.total_height).abs() < 40.0, "posée en bas : {derniere:?}");
        // La souris partie : repliée, mais TOUJOURS défilée — le haut est
        // replié en miroir (bascule négative), et tout reste à l'écran.
        let defile = dock.pli_delta[0];
        souris_et_temps(&mut dock, &layout, layout.total_width * 0.5, 500.0);
        assert_eq!((dock.pli_survol[0], dock.pli_delta[0]), (0.0, defile));
        let rects = rects_de_gauche(&dock, &layout);
        let (_, angle) = dock.bascule_de(rects[0].0, &layout).expect("la première est repliée en haut");
        assert!(angle < 0.0, "{angle}");
        let haut = rects[0].1.expect("à l'écran");
        assert!(haut[1] >= layout.haut_colonnes, "{haut:?}");
        assert!(rects.iter().all(|(_, r)| r.is_some_and(|r| r[1] + r[3] <= layout.total_height + 1.0)));
        // Et la molette la ramène en haut, droite.
        for _ in 0..500 {
            dock.scroll_bar(Side::Left, WHEEL_DELTA, 500.0, &layout);
        }
        assert_eq!(dock.pli_delta[0], 0.0);
        assert!(dock.bascule_de(rects_de_gauche(&dock, &layout)[0].0, &layout).is_none());
    }

    #[test]
    fn en_entrant_par_la_partie_repliee_l_icone_visee_reste_sous_la_souris() {
        let layout = geometry_layout();
        let mut dock = dock_qui_deborde(REPLIEMENT_ACTIF);
        let rects = rects_de_gauche(&dock, &layout);
        let (visee, r) = rects.iter().rev().find(|(i, _)| dock.bascule_de(*i, &layout).is_some()).copied().unwrap();
        let r = r.unwrap();
        let y = r[1] + r[3] * 0.5;
        souris_et_temps(&mut dock, &layout, r[0] + r[2] * 0.5, y);
        let apres = dock.icon_rect(visee, &layout).expect("visible");
        let centre = apres[1] + apres[3] * 0.5;
        assert!(dock.bascule_de(visee, &layout).is_none(), "dépliée");
        assert!((centre - y).abs() < apres[3] * 0.5 + 1.0, "sous la souris : {centre} contre {y}");
    }

    #[test]
    fn en_bas_chaque_icone_repliee_passe_sous_celle_du_dessus_en_haut_l_inverse() {
        let layout = geometry_layout();
        let mut dock = dock_qui_deborde(REPLIEMENT_ACTIF);
        let cle = |dock: &Dock, i: usize| dock.ordre_de_dessin_plie(&layout).into_iter().find(|&(j, _)| j == i).map(|(_, k)| k).unwrap();
        let icones: Vec<usize> = rects_de_gauche(&dock, &layout).into_iter().map(|(i, _)| i).collect();
        // En bas : dessinées de la dernière à la première.
        assert!(icones.windows(2).all(|p| cle(&dock, p[0]) > cle(&dock, p[1])));
        // Défilée : le groupe du haut passe AVANT tout le reste, chaque icône
        // sous celle qui la suit.
        dock.pli_delta[0] = -400.0;
        let en_haut: Vec<usize> = icones.iter().copied().filter(|&i| dock.bascule_de(i, &layout).is_some_and(|(_, a)| a < 0.0)).collect();
        assert!(en_haut.len() >= 2, "prémisse : {en_haut:?}");
        assert!(en_haut.windows(2).all(|p| cle(&dock, p[0]) < cle(&dock, p[1])));
        let max_haut = en_haut.iter().map(|&i| cle(&dock, i)).max().unwrap();
        assert!(icones.iter().filter(|i| !en_haut.contains(i)).all(|&i| cle(&dock, i) > max_haut));
        assert!(Dock::new([0.0; 4], 0).ordre_de_dessin_plie(&layout).is_empty(), "rien sans repliement");
    }

    #[test]
    fn une_barre_qui_tient_ne_se_replie_pas() {
        let haute = DockLayout { total_height: 40_000.0, ..geometry_layout() };
        let dock = dock_qui_deborde(REPLIEMENT_ACTIF);
        assert!(dock.pliage_de_la_barre(Side::Left, &geometry_layout()).is_some(), "prémisse");
        assert!(dock.pliage_de_la_barre(Side::Left, &haute).is_none());
        assert!(rects_de_gauche(&dock, &haute).iter().all(|(i, r)| r.is_some() && dock.bascule_de(*i, &haute).is_none()));
    }

    #[test]
    fn a_shorter_bar_pulls_the_scroll_offset_back() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.set_dynamic_apps(40);
        // Voir le test précédent : aucune zone ici, donc `y` est sans effet.
        let y = layout.top_thickness + 100.0;
        for _ in 0..20 {
            dock.scroll_bar(Side::Left, -1000.0, y, &layout);
        }
        let scrolled = dock.scroll[0];
        assert!(scrolled > 0.0, "le test doit partir d'une barre réellement défilée, pas de {scrolled}");

        // Le contenu rétrécit, puis une frame passe.
        dock.set_dynamic_apps(8);
        dock.update(0.0, &layout);

        assert!(dock.scroll[0] < scrolled, "le décalage devait redescendre : {scrolled} -> {}", dock.scroll[0]);
        // Et la barre reste défilable JUSQU'AU BOUT : la dernière icône est de
        // nouveau atteignable, aucune n'est restée coincée sous le bord.
        let last = *dock.left_order.last().expect("la barre n'est pas vide");
        assert!(dock.icon_rect(last, &layout).is_some(), "la dernière icône doit être atteignable après le re-bornage");
    }

    /// `magnified_index` est ce qui rend tout le mécanisme INERTE là où rien ne
    /// magnifie — les docks d'écrans SECONDAIRES, dont `Dock::update` n'est
    /// jamais appelé, donc dont les échelles restent à 1.0 pour toujours : sans
    /// index magnifié, pas de lot tardif, pas de boîte présentée, pas de
    /// débordement de région.
    #[test]
    fn magnified_index_is_none_until_something_is_hovered() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        assert_eq!(dock.magnified_index(), None, "rien de survolé au départ");

        // Le logo — index 0, celui de `app::APP_ICON_INDEX`, écrit en clair ici
        // pour que ce module (feuille de rendu) ne dépende pas de `app` — est la
        // seule icône du dock par défaut qui vit dans la barre de gauche (voir
        // `Dock::new`).
        const LOGO: usize = 0;
        let rect = dock.icon_rect(LOGO, &layout).expect("le logo est placé");
        let (cx, cy) = (rect[0] + rect[2] * 0.5, rect[1] + rect[3] * 0.5);
        dock.on_mouse_move(cx, cy, HWND(std::ptr::null_mut()));
        for _ in 0..120 {
            dock.update(1.0 / 60.0, &layout);
        }
        assert_eq!(dock.magnified_index(), Some(LOGO));

        // Et le rectangle présenté de cette icône est bien celui de sa case,
        // agrandi puis ramené dans la fenêtre.
        let presented = dock.presented_rect(LOGO, &layout).expect("présentée");
        assert!(presented[2] > rect[2], "elle est bien grossie");
        assert!(presented[0] >= 0.0 && presented[0] + presented[2] <= layout.total_width + 0.001);

        // Curseur sorti : plus rien de magnifié.
        dock.on_mouse_leave(HWND(std::ptr::null_mut()));
        for _ in 0..600 {
            dock.update(1.0 / 60.0, &layout);
        }
        assert_eq!(dock.magnified_index(), None);
    }

    /// L'échelle revient EXACTEMENT au repos, et pas « presque » — DÉFAUT RÉEL
    /// TROUVÉ PAR LE TEST CI-DESSUS, qui échouait sur `1.0000002` : l'approche
    /// exponentielle se bloquait deux ULP au-dessus de sa cible, un point fixe
    /// de la récurrence en `f32`. Comme tout le mécanisme se déclenche sur
    /// `scale > 1.0` (voir `magnified_index`), l'icône serait restée
    /// « magnifiée » POUR TOUJOURS après son premier survol.
    #[test]
    fn hover_scale_returns_exactly_to_rest() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let rect = dock.icon_rect(0, &layout).expect("le logo est placé");
        let (cx, cy) = (rect[0] + rect[2] * 0.5, rect[1] + rect[3] * 0.5);
        dock.on_mouse_move(cx, cy, HWND(std::ptr::null_mut()));
        for _ in 0..120 {
            dock.update(1.0 / 60.0, &layout);
        }
        dock.on_mouse_leave(HWND(std::ptr::null_mut()));
        for _ in 0..600 {
            dock.update(1.0 / 60.0, &layout);
        }
        for i in 0..dock.icons.len() {
            assert_eq!(dock.hover.scale(i), 1.0, "icône {i} : échelle non revenue au repos");
        }
    }

    /// **Le test qui verrouille la non-régression du réglage.** Il vise une
    /// cible DIFFÉRENTE du défaut (2.0 contre 1.30) : à la valeur par défaut, il
    /// passerait encore si `update` lisait l'ancienne constante au lieu de la
    /// cellule, et ne prouverait donc rien.
    #[test]
    fn hover_animator_reaches_the_configured_scale() {
        with_hover_settings(2.0, MAX_HOVER_EASE_SPEED, || {
            let rects = [[0.0, 0.0, 64.0, 64.0]];
            let mut animator = HoverAnimator::new();
            for _ in 0..600 {
                animator.update(&rects, Some((32.0, 32.0)), 1.0 / 60.0);
            }
            assert!(
                (animator.scale(0) - 2.0).abs() < 1e-3,
                "l'icône survolée doit atteindre la cible réglée, pas l'ancienne constante : {}",
                animator.scale(0)
            );
        });
    }

    /// **Le test qui verrouille `update_scaled`** : la cible passée en paramètre
    /// doit être suivie TELLE QUELLE, et non le global des icônes — c'est toute
    /// la mécanique qui donne aux horloges leur propre facteur. La cible visée
    /// est délibérément éloignée de celle que pose `with_hover_settings` : si
    /// `update_scaled` retombait sur `hover_scale()`, l'assertion tomberait.
    #[test]
    fn update_scaled_uses_the_passed_target_not_the_global() {
        with_hover_settings(1.30, MAX_HOVER_EASE_SPEED, || {
            let rects = [[0.0, 0.0, 64.0, 64.0], [200.0, 200.0, 64.0, 64.0]];
            let mut animator = HoverAnimator::new();
            for _ in 0..600 {
                animator.update_scaled(&rects, Some((32.0, 32.0)), 1.0 / 60.0, 3.20);
            }
            assert!(
                (animator.scale(0) - 3.20).abs() < 1e-3,
                "la cible passée doit être respectée : {}",
                animator.scale(0)
            );
            assert!(
                (animator.scale(0) - hover_scale()).abs() > 1e-3,
                "l'animateur a suivi le global des icônes au lieu de la cible passée"
            );
            // Le rectangle NON survolé retombe à 1.0 quelle que soit la cible :
            // le contrat de `update` ne dépend pas d'elle.
            assert_eq!(animator.scale(1), 1.0, "un rectangle non survolé doit rester au repos");
        });
    }

    /// La vitesse règle bien la VITESSE, pas seulement la cible : à cible, `dt`
    /// et nombre de pas identiques, le réglage rapide doit avoir davantage
    /// approché que le lent. C'est ce qui distingue un vrai second réglage d'un
    /// curseur décoratif.
    #[test]
    fn hover_ease_speed_controls_the_approach_rate() {
        let rects = [[0.0, 0.0, 64.0, 64.0]];
        let step = 1.0 / 60.0;
        let steps = 6;

        let mut slow = HoverAnimator::new();
        with_hover_settings(MAX_HOVER_SCALE, MIN_HOVER_EASE_SPEED, || {
            for _ in 0..steps {
                slow.update(&rects, Some((32.0, 32.0)), step);
            }
        });

        let mut fast = HoverAnimator::new();
        with_hover_settings(MAX_HOVER_SCALE, MAX_HOVER_EASE_SPEED, || {
            for _ in 0..steps {
                fast.update(&rects, Some((32.0, 32.0)), step);
            }
        });

        assert!(
            fast.scale(0) > slow.scale(0),
            "à pas égal, le réglage rapide ({}) doit dépasser le lent ({})",
            fast.scale(0),
            slow.scale(0)
        );
        // Et aucun des deux ne dépasse la cible : la vitesse ne fait
        // qu'approcher plus ou moins vite.
        assert!(fast.scale(0) <= MAX_HOVER_SCALE);
    }

    /// `MIN_HOVER_SCALE` doit être un VRAI repos : survoler une icône ne la
    /// change plus du tout. C'est ce qui justifie le plancher à 1.0 plutôt
    /// qu'une valeur qui ferait rétrécir l'icône.
    #[test]
    fn hover_scale_minimum_is_a_no_op() {
        with_hover_settings(MIN_HOVER_SCALE, MAX_HOVER_EASE_SPEED, || {
            // Deux rectangles : un survolé, un non — les deux doivent rester à
            // 1.0, donc l'écart nul est la preuve que le survol ne fait plus
            // rien.
            let rects = [[0.0, 0.0, 64.0, 64.0], [0.0, 100.0, 64.0, 64.0]];
            let mut animator = HoverAnimator::new();
            for _ in 0..600 {
                animator.update(&rects, Some((32.0, 32.0)), 1.0 / 60.0);
            }
            assert_eq!(animator.scale(0), 1.0, "l'icône survolée reste inchangée");
            assert_eq!(animator.scale(1), 1.0);
        });
    }

    /// **Maj + glisser un espace LATÉRAL change sa HAUTEUR, et ne touche à AUCUN
    /// ordre** — la propriété qui protège le geste de déplacement, lequel doit
    /// rester identique (demande explicite : « garde le déplacement »).
    #[test]
    fn shift_drag_on_a_lateral_spacer_resizes_without_reordering() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let index = dock.icons.len() - 1;
        let order_before = dock.order(Side::Right).to_vec();
        let size_before = dock.spacer_size(index).expect("prémisse : l'espace existe");
        // Prémisse : sans elle, l'assertion finale ne prouverait rien.
        assert!(order_before.contains(&index), "l'espace est bien dans la barre de droite");

        let (x, y) = (900.0, 400.0);
        dock.begin_potential_drag(index, x, y, HWND::default());
        // Maj DÈS le franchissement du seuil : c'est ce seul instant qui qualifie
        // le geste (voir `IconDrag::resizing`).
        dock.update_drag(x, y + 10.0, false, true, &layout);
        assert_eq!(dock.spacer_size(index), Some(size_before + 10.0), "la taille suit le curseur");
        // Delta depuis le DÉBUT du geste, pas un cumul : repasser ailleurs donne
        // la taille correspondante même si une image a été sautée entre les deux.
        dock.update_drag(x, y + 40.0, false, true, &layout);
        assert_eq!(dock.spacer_size(index), Some(size_before + 40.0), "position absolue du curseur");

        assert!(matches!(dock.end_drag(&layout, HWND::default()), DragOutcome::Resized));
        assert_eq!(dock.order(Side::Right).to_vec(), order_before, "AUCUN ordre ne doit avoir bougé");
    }

    /// **Régler un espace ne doit PAS toucher à la taille d'un AUTRE** — défaut
    /// RÉEL SIGNALÉ : « les séparateurs de zone de la barre de droite semblent
    /// être liés, je ne peux pas définir des tailles de zones librement »,
    /// reproduit ici de la façon la plus directe : deux espaces dans la MÊME
    /// barre, de tailles DIFFÉRENTES, on en redimensionne un à la souris, et
    /// l'autre doit garder la sienne.
    #[test]
    fn resizing_one_spacer_leaves_its_neighbour_alone() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let first = dock.icons.len() - 1;
        dock.set_spacer_size(first, 40.0);
        dock.add_spacer_at_end(Side::Right);
        let second = dock.icons.len() - 1;
        dock.set_spacer_size(second, 120.0);
        // Prémisses : deux espaces DISTINCTS, de tailles différentes, tous deux
        // dans la barre de droite.
        assert_ne!(first, second, "deux espaces distincts");
        assert_eq!(dock.spacer_size(first), Some(40.0));
        assert_eq!(dock.spacer_size(second), Some(120.0));
        // Elle disait « 3 » (Réglages + ces deux espaces) : un COMPTE, donc une
        // prémisse qui casse à chaque icône ajoutée dans cette barre — ce qui
        // est arrivé avec la bascule de thème (`right_order` dans `Dock::new`).
        // Ce que le test veut dire est que les deux espaces sont BIEN DANS
        // cette barre, pas qu'ils y sont seuls : on l'écrit comme tel.
        let right = dock.order(Side::Right).to_vec();
        assert!(
            right.contains(&first) && right.contains(&second),
            "prémisse : les deux espaces sont dans la barre de droite, or elle contient {right:?}"
        );

        let (x, y) = (900.0, 400.0);
        dock.begin_potential_drag(first, x, y, HWND::default());
        dock.update_drag(x, y + 10.0, false, true, &layout); // franchit le seuil, Maj
        dock.update_drag(x, y + 60.0, false, true, &layout); // 60 px plus bas que le clic

        assert_eq!(dock.spacer_size(first), Some(100.0), "le premier a bien grandi (40 + 60)");
        assert_eq!(dock.spacer_size(second), Some(120.0), "le SECOND ne doit pas avoir bougé");
    }

    /// **Glisser SANS Maj sur le même espace le DÉPLACE toujours, et ne change
    /// pas sa taille** — la réciproque du test précédent : sans elle, on ne
    /// saurait pas si le redimensionnement a simplement REMPLACÉ le déplacement.
    #[test]
    fn plain_drag_on_a_spacer_still_moves_it_without_resizing() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        // La barre de droite n'est PAS vide au départ (`Dock::new` y place
        // Réglages) : on mesure donc le compte AVANT plutôt que de le supposer.
        let count_before = dock.order(Side::Right).len();
        dock.add_spacer_at_end(Side::Right);
        let index = dock.icons.len() - 1;
        let size_before = dock.spacer_size(index).expect("prémisse : l'espace existe");
        dock.add_spacer_at_end(Side::Right);
        assert_eq!(
            dock.order(Side::Right).len(),
            count_before + 2,
            "prémisse : les deux espaces sont dans la barre de droite"
        );

        let (x, y) = (900.0, 400.0);
        dock.begin_potential_drag(index, x, y, HWND::default());
        dock.update_drag(x, y + 40.0, false, false, &layout); // sans Maj
        assert_eq!(dock.spacer_size(index), Some(size_before), "la taille ne bouge pas");
        assert!(matches!(dock.end_drag(&layout, HWND::default()), DragOutcome::Moved));
        assert_eq!(dock.order(Side::Right).last().copied(), Some(index), "l'espace a bien été déplacé");
    }

    /// Maj ne change RIEN au geste sur une icône NORMALE : elle se déplace
    /// toujours. Sans ce test, on ignorerait si Maj est bien limité aux espaces
    /// ou s'il perturbe tous les glissers.
    #[test]
    fn shift_drag_on_a_normal_icon_still_moves_it() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        // L'index 1 est une icône ordinaire de la barre de gauche (voir `Dock::new`).
        assert!(!dock.is_spacer(1), "prémisse : l'index 1 n'est pas un espace");
        let order_before = dock.order(Side::Left).to_vec();
        assert!(order_before.contains(&1), "prémisse : elle est dans la barre de gauche");

        let (x, y) = (60.0, 200.0);
        dock.begin_potential_drag(1, x, y, HWND::default());
        dock.update_drag(x, y + 400.0, false, true, &layout); // Maj maintenu
        assert!(matches!(dock.end_drag(&layout, HWND::default()), DragOutcome::Moved));
        assert_ne!(dock.order(Side::Left).to_vec(), order_before, "l'ordre a bien changé (déplacement)");
    }

    /// Bornes du redimensionnement, ET le cas NON latéral : un espace de la barre
    /// du HAUT n'a pas de hauteur propre (voir `layout_top_side`), Maj n'y
    /// redimensionne donc rien.
    #[test]
    fn spacer_resize_is_clamped_and_never_applies_to_the_top_bar() {
        let layout = geometry_layout();
        // Même formule que `reclamp_scroll` : la hauteur disponible d'une barre
        // latérale.
        let available = layout.total_height - layout.top_thickness - TOP_MARGIN * 2.0;

        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let index = dock.icons.len() - 1;
        let (x, y) = (900.0, 400.0);
        dock.begin_potential_drag(index, x, y, HWND::default());
        dock.update_drag(x, y + 10.0, false, true, &layout); // franchit le seuil
        dock.update_drag(x, 100_000.0, false, true, &layout); // tire très bas
        assert_eq!(dock.spacer_size(index), Some(available), "plafonné à la hauteur de la barre");
        dock.update_drag(x, -100_000.0, false, true, &layout); // tire très haut
        assert_eq!(dock.spacer_size(index), Some(0.0), "jamais négatif");
        let _ = dock.end_drag(&layout, HWND::default());

        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::TopRight);
        let index = dock.icons.len() - 1;
        let size_before = dock.spacer_size(index).expect("prémisse : l'espace existe");
        dock.begin_potential_drag(index, 3000.0, 40.0, HWND::default());
        dock.update_drag(3000.0, 140.0, false, true, &layout);
        assert_eq!(dock.spacer_size(index), Some(size_before), "taille inchangée dans la barre du haut");
    }

    /// Le trait de survol traverse la BARRE dans son épaisseur et se centre sur la
    /// hauteur de l'espace, quelle que soit sa taille — c'est ce qui en fait un
    /// repère, pas un ornement.
    #[test]
    fn spacer_hover_line_spans_the_bar_and_centres_on_the_spacer() {
        let layout = geometry_layout();
        for (side, height) in [(Side::Right, 18.0), (Side::Right, 300.0), (Side::Left, 18.0)] {
            let rect = [0.0, 500.0, 56.0, height];
            let line = spacer_hover_line_rect(rect, &layout, side);

            assert_eq!(line[1] + line[3] * 0.5, rect[1] + rect[3] * 0.5, "centré sur l'espace (hauteur {height})");
            match side {
                Side::Right => {
                    assert!(line[0] >= layout.total_width - layout.right_width, "dans la barre de droite");
                    assert!(line[0] + line[2] <= layout.total_width, "et pas au-delà");
                }
                _ => {
                    assert!(line[0] >= 0.0, "dans la barre de gauche");
                    assert!(line[0] + line[2] <= layout.left_width, "et pas au-delà");
                }
            }
        }
    }

    // ---- Les séparateurs de zone (voir `Dock::bar_layout`) ----------------
    //
    // Aucun séparateur de zone n'est encore CRÉABLE depuis l'interface (c'est
    // l'étape suivante) : ces tests posent donc la cote à la main, par
    // `set_zone_height`. Ils vérifient le CONTRAT du calcul, pas un geste.

    /// Ordre VISIBLE de la barre de droite et sa disposition, exactement comme
    /// `layout_side` les calcule (`start_y` sans défilement) — les tests de
    /// zones ont besoin des deux, et refaire ce calcul dans chacun d'eux
    /// laisserait passer une divergence entre le test et le code testé.
    fn right_bar(dock: &Dock, layout: &DockLayout) -> (Vec<usize>, BarLayout) {
        let order: Vec<usize> = dock.order(Side::Right).iter().copied().filter(|&i| !dock.icons[i].hidden).collect();
        // Le défilement PROPRE aux zones est passé comme `layout_side` le fait
        // (voir `Dock::zone_scroll_offset`) : un zéro en dur ici rendrait le
        // test aveugle au défilement par zone, qui est justement ce qu'il
        // vérifie.
        let bar = dock.bar_layout(Side::Right, 
            &order,
            icon_size_for(layout.right_width),
            layout.top_thickness + TOP_MARGIN,
            &|sep| dock.zone_scroll_offset(sep),
            &dock.zone_grid(Side::Right, icon_size_for(layout.right_width), layout.right_width),
        );
        (order, bar)
    }

    /// Emplacement de l'icône `index` — `bar.slots` suit l'ordre de `order`,
    /// donc son rang y est le même.
    fn slot_of(order: &[usize], bar: &BarLayout, index: usize) -> Slot {
        let position = order.iter().position(|&i| i == index).expect("l'icône est dans la barre");
        bar.slots[position]
    }

    /// **LE TEST DE NON-RÉGRESSION DE CETTE ÉTAPE.** Un espace ORDINAIRE
    /// (aucune cote) doit s'empiler EXACTEMENT comme avant l'existence des
    /// zones. La référence n'est pas `bar_layout` lui-même mais
    /// l'arithmétique d'origine réécrite ici à la main : sinon le test ne
    /// comparerait le code qu'à lui-même et passerait quoi qu'il arrive.
    #[test]
    fn a_plain_spacer_still_stacks_exactly_as_before_zones() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let spacer = dock.icons.len() - 1;
        // Une taille DIFFÉRENTE de `SPACER_HEIGHT` : sinon l'assertion finale
        // passerait encore si le réglage de l'utilisateur était ignoré.
        dock.set_spacer_size(spacer, 40.0);

        let (order, bar) = right_bar(&dock, &layout);
        assert!(bar.zones.is_empty(), "aucune cote déclarée : aucune zone ne doit exister");

        let icon_size = icon_size_for(layout.right_width);
        let mut expected = Vec::new();
        let mut y = layout.top_thickness + TOP_MARGIN;
        for (i, &index) in order.iter().enumerate() {
            let is_spacer = dock.is_spacer(index);
            let height = if is_spacer { dock.spacer_size(index).unwrap_or(SPACER_HEIGHT) } else { icon_size };
            expected.push(Slot { top: y, height, column: 0, zone: None });
            let next_is_spacer = order.get(i + 1).is_some_and(|&next| dock.is_spacer(next));
            let gap = if is_spacer || next_is_spacer { SPACER_GAP } else { dock.icon_gap() };
            y += height + gap;
        }

        assert_eq!(bar.slots.len(), order.len(), "un emplacement par élément");
        assert_eq!(bar.slots, expected, "un espace ordinaire doit s'empiler exactement comme avant les zones");
        // Prémisses sans lesquelles l'égalité ci-dessus ne prouverait rien.
        assert!(order.contains(&spacer), "l'espace est bien dans la barre de droite");
        assert!(bar.slots.iter().any(|slot| slot.height == 40.0), "sa taille réglée est bien prise en compte");
    }

    /// La cote d'un séparateur n'est PAS sa taille : le séparateur reste FIN
    /// (la taille de son MARQUEUR), c'est sa ZONE qui est grande. Les confondre
    /// ferait un pavé de la hauteur de la zone là où l'utilisateur attend un
    /// trait.
    ///
    /// **Le blanc est donné ÉNORME exprès** : c'est ce qui distingue « le
    /// séparateur occupe `SPACER_HEIGHT` » de « le séparateur occupe son blanc,
    /// qui se trouve valoir `SPACER_HEIGHT` par défaut ». DÉFAUT RÉEL SIGNALÉ
    /// avant ce test : un blanc de 221 px repoussait la zone — et le nom qu'elle
    /// porte — de 227 px vers le bas (« les textes ne sont pas placés en haut des
    /// zones », mesuré au journal de diagnostic).
    #[test]
    fn a_zone_separator_keeps_its_marker_size() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let sep = dock.icons.len() - 1;
        dock.set_spacer_size(sep, 221.0);
        dock.set_zone_height(sep, 400.0);

        let (order, bar) = right_bar(&dock, &layout);
        let zone = bar.zones.first().expect("prémisse : ce séparateur déclare bien une zone");
        assert_eq!(zone.bottom, zone.top + 400.0, "le bas de la zone est sa cote");
        assert_eq!(slot_of(&order, &bar, sep).height, SPACER_HEIGHT, "le séparateur, lui, garde sa taille de marqueur");
        // La VALEUR est conservée — elle redevient agissante si la zone est
        // défaite (voir `Icon::spacer_size`) : c'est la disposition qui l'ignore,
        // pas la mémoire du réglage.
        assert_eq!(dock.spacer_size(sep), Some(221.0), "son blanc est conservé, simplement sans effet");
    }

    /// **Un espace peut être créé dans la barre du HAUT, et sa TAILLE y est sa
    /// LARGEUR** — prémisse du correctif de « je n'arrive pas à ajouter de
    /// séparateur dans la partie gauche de la barre supérieure » (le geste
    /// échouait faute de NOMMER la barre : `side_at` ne rend que les deux barres
    /// latérales, voir `app::run`), et de « je ne peux plus régler la taille des
    /// séparateurs par le clic droit [...] dans la bande du haut » (la taille y
    /// était lue par personne).
    #[test]
    fn a_spacer_can_be_created_in_the_top_bar() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let icon_size = top_icon_size_for(layout.top_thickness);

        let spacer = dock.add_spacer_at_end(Side::TopLeft);
        assert!(dock.is_spacer(spacer), "c'est bien un espace");
        assert!(dock.order(Side::TopLeft).contains(&spacer), "et il est dans la barre du HAUT (moitié gauche)");

        // Il occupe une place DANS la bande du haut : c'est ce qui le dessine
        // (le marqueur de `render_instances`) et ce qui décale ce qui le suit.
        let rect = dock.icon_rect(spacer, &layout).expect("un espace de la barre du haut a un rectangle");
        assert!(rect[1] < layout.top_thickness, "posé DANS la bande du haut : {rect:?}");
        assert!(rect[2] > 0.0 && rect[3] > 0.0, "et il a une surface : {rect:?}");

        // SA TAILLE EST SA LARGEUR — et elle décale bien ce qui suit, d'autant.
        dock.set_spacer_size(spacer, 40.0);
        let etroit = dock.icon_rect(spacer, &layout).expect("toujours là");
        assert_eq!(etroit[2], 40.0, "la largeur EST la taille de l'espace : {etroit:?}");
        assert_ne!(etroit[2], icon_size, "(prémisse : une icône ferait une autre largeur)");

        // Un second espace, réglé autrement, se pose APRÈS le premier — l'écart
        // entre les deux suit la largeur du premier, pas un pas uniforme.
        let second = dock.add_spacer_at_end(Side::TopLeft);
        dock.set_spacer_size(second, 80.0);
        let (a, b) = (
            dock.icon_rect(spacer, &layout).expect("le premier"),
            dock.icon_rect(second, &layout).expect("le second"),
        );
        assert!(b[0] > a[0], "le second est à droite du premier : {a:?} puis {b:?}");
        assert_eq!(b[0] - (a[0] + a[2]), dock.icon_gap(), "séparés par l'écartement, et rien d'autre");
    }

    /// **Le popup d'un espace ordinaire propose son BLANC, celui d'un séparateur
    /// de zone propose sa COTE — jamais l'inverse** — DÉFAUT RÉEL SIGNALÉ, causé
    /// par cette inversion : le popup s'ouvrait avec son titre et RIEN dedans.
    ///
    /// Le test tient les DEUX sens, parce que c'est leur opposITION qui se
    /// trompe : écrire l'une des deux lignes sur le modèle de l'autre les
    /// inverse, et rien ne le signale — les deux sont des `Option<f32>`.
    #[test]
    fn the_spacer_popup_proposes_the_blank_or_the_cote_never_the_wrong_one() {
        // Un espace ORDINAIRE : son blanc, pas de cote.
        assert_eq!(spacer_popup_values(false, 18.0, 0.0), (Some(18.0), None));
        // Même avec une cote résiduelle non nulle dans le champ (un espace dont
        // la zone vient d'être défaite), c'est le BLANC qui est proposé.
        assert_eq!(spacer_popup_values(false, 18.0, 400.0), (Some(18.0), None));
        // Un SÉPARATEUR DE ZONE : sa cote, pas de blanc.
        assert_eq!(spacer_popup_values(true, 221.0, 400.0), (None, Some(400.0)));
        // Les deux ne sont jamais proposés ENSEMBLE : le popup n'a qu'une
        // première rangée, et deux curseurs y seraient deux réglages pour une
        // seule ligne.
        for is_zone in [false, true] {
            let (blanc, cote) = spacer_popup_values(is_zone, 18.0, 400.0);
            assert!(blanc.is_some() != cote.is_some(), "exactement une des deux, pour is_zone={is_zone}");
        }
    }

    /// **Le libellé d'une zone RÉDUIT sa taille pour tenir dans la barre** —
    /// DEMANDE EXPLICITE : « j'aimerais que les titres des zones de droite
    /// réduisent automatiquement leur taille si le texte ne rentre pas dans la
    /// largeur de la barre ». La taille du curseur est un MAXIMUM, jamais un
    /// minimum.
    ///
    /// La MESURE est passée en paramètre — c'est ce qui rend cette décision
    /// testable sans GDI ni écran, et la mesure employée en production reste
    /// celle du dessin (`gfx::text::measure_text`, même police que
    /// `render_text`).
    #[test]
    fn a_zone_label_shrinks_to_fit_the_bar() {
        // Un libellé de 15 caractères, à 0,46875 × la taille par caractère —
        // donc une largeur PROPORTIONNELLE à la taille, comme du vrai texte.
        let width_at = |px: u16| 15.0 * (px as f32 * 0.46875);

        // Toute la place du monde, ou exactement la place : la demande est
        // rendue TELLE QUELLE — sans quoi un libellé court rétrécirait sans
        // raison.
        assert_eq!(zone_label_fit_px(22, 1000.0, width_at), 22);
        assert_eq!(zone_label_fit_px(22, width_at(22), width_at), 22);

        // Un pixel de moins que nécessaire, et il faut descendre d'une taille.
        assert_eq!(zone_label_fit_px(22, width_at(22) - 1.0, width_at), 21);

        // Beaucoup trop étroit : on s'arrête au PLANCHER de lisibilité (celui du
        // curseur), jamais en dessous — un texte tronqué à la barre vaut mieux
        // qu'un texte illisible.
        assert_eq!(zone_label_fit_px(48, 10.0, width_at), MIN_ZONE_LABEL_FONT_PX);

        // La demande elle-même est bornée : le curseur ne peut pas la dépasser,
        // et une valeur aberrante ne fait pas rendre plus grand qu'annoncé.
        assert_eq!(zone_label_fit_px(200, 1000.0, width_at), MAX_ZONE_LABEL_FONT_PX);

        // Une barre pas encore dimensionnée ne doit faire DISPARAÎTRE aucun
        // texte : on rend la demande plutôt que de réduire à rien.
        assert_eq!(zone_label_fit_px(30, 0.0, width_at), 30);
    }

    /// **Le CONTENU d'une zone commence JUSTE SOUS LE CERCLE de son séparateur.**
    ///
    /// ⚠️ **CE N'EST PLUS LA POSITION DU NOM DE LA ZONE** — il l'a été, et le
    /// commentaire d'origine le disait (« le texte est dessiné au haut de la
    /// boîte de la zone »). Depuis la demande explicite « les textes des zones
    /// apparaissent centrés sur les séparateurs », l'étiquette est centrée SUR
    /// l'emplacement du séparateur (voir `zone_label_origin`) et ne vit donc plus
    /// ici : ce que cette marge positionne désormais, c'est la PREMIÈRE ICÔNE de
    /// la zone. Elle garde donc tout son sens — un contenu qui mordrait sur le
    /// cercle, ou que le blanc d'un espace repousserait, reste un défaut.
    ///
    /// Le marqueur est un cercle de `separator_size` centré dans son emplacement
    /// (voir `spacer_marker_instance`) : son bas vaut
    /// `marqueur.top + marqueur.height / 2 + separator_size / 2`, et la boîte
    /// doit commencer à `SPACER_GAP` de là, jamais à la hauteur d'un blanc.
    #[test]
    fn a_zones_box_starts_just_under_its_separators_circle() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let sep = dock.icons.len() - 1;
        // Le blanc ÉNORME qui a produit le défaut signalé : c'est lui que ce test
        // met sous le nez de la disposition.
        dock.set_spacer_size(sep, 221.0);
        dock.set_zone_height(sep, 400.0);

        let (order, bar) = right_bar(&dock, &layout);
        let slot = slot_of(&order, &bar, sep);
        let zone = bar.zones.first().expect("prémisse : ce séparateur déclare bien une zone");
        let circle_bottom = slot.top + slot.height * 0.5 + dock.separator_size() * 0.5;
        let marge = zone.top - circle_bottom;

        // Les deux bornes ont un sens : jamais NÉGATIVE (le CONTENU de la zone ne
        // doit pas mordre sur le cercle — c'est la position de l'étiquette, et
        // non la sienne, qui a changé, voir la doc de ce test), jamais plus
        // grande que ce que coûte l'emplacement du marqueur lui-même — un blanc
        // de 221 px donnait 109,5 px.
        assert!(
            (SPACER_GAP..=SPACER_HEIGHT).contains(&marge),
            "le haut de la zone tombe à {marge} px sous le bas du cercle, pas juste sous lui"
        );
        // Prémisses sans lesquelles l'assertion ci-dessus serait vraie d'un dock
        // vide, ou d'un séparateur qui n'en serait pas un.
        assert_eq!(slot.height, SPACER_HEIGHT, "l'emplacement d'un séparateur est la taille de son marqueur");
        assert!(dock.is_zone_separator(sep), "ce séparateur déclare bien une zone");
    }

    /// Le texte d'une zone est **centré sur l'EMPLACEMENT de son séparateur** —
    /// et non sur la boîte de sa zone, qui commence 24 px plus bas.
    ///
    /// **Ce test confronte DEUX SOURCES au lieu de refaire un calcul**, et c'est
    /// tout son intérêt : le premier jet de `zone_label_origin` centrait le texte
    /// sur `zone_boxes` (la boîte de la ZONE) au lieu d'`icon_rect` (l'emplacement
    /// du SÉPARATEUR), et laissait donc le texte SOUS la pastille — exactement le
    /// défaut que la demande visait à corriger. Un test qui aurait refait le
    /// calcul à la main aurait confirmé l'erreur ; confronter les deux sources la
    /// voit.
    #[test]
    fn a_zone_label_is_centred_on_its_separator() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let sep = dock.icons.len() - 1;
        dock.set_zone_height(sep, 400.0);

        let (text_w, text_h) = (60.0, 20.0);
        let marker = dock.icon_rect(sep, &layout).expect("le séparateur a un emplacement");
        let [x, y] = zone_label_origin(marker, text_w, text_h);

        // La propriété qui compte : le MILIEU du texte tombe sur le milieu de
        // l'emplacement du séparateur — donc sur le centre de son cercle, qu'il
        // recouvre (choix explicite de l'utilisateur).
        assert_eq!(marker[3], SPACER_HEIGHT, "l'emplacement d'un séparateur est la taille de son marqueur");
        assert_eq!(x + text_w * 0.5, marker[0] + marker[2] * 0.5);
        assert_eq!(y + text_h * 0.5, marker[1] + marker[3] * 0.5);

        // La MÊME vérité, vue par la disposition : c'est bien le milieu de
        // l'emplacement que `bar_layout` place, et non un rectangle inventé.
        let (order, bar) = right_bar(&dock, &layout);
        let slot = slot_of(&order, &bar, sep);
        assert_eq!(marker[1] + marker[3] * 0.5, slot.top + slot.height * 0.5);

        // LE PIÈGE, MESURÉ — la boîte de la ZONE commence SOUS le séparateur, et
        // c'est bien elle qui avait été prise : le texte serait 24 px trop bas,
        // c'est-à-dire sous la pastille.
        let (_, zone) = dock
            .zone_boxes(&layout)
            .into_iter()
            .find(|(index, _)| *index == sep)
            .expect("prémisse : ce séparateur déclare bien une zone");
        assert!(zone[1] > marker[1] + marker[3], "la zone doit commencer SOUS l'emplacement du séparateur");
        assert_ne!(y + text_h * 0.5, zone[1] + (zone[3] - text_h) * 0.5, "centrer sur la ZONE serait l'erreur");
    }

    /// **Une cote IMPOSE le bas de sa zone** : un contenu plus court qu'elle
    /// laisse du VIDE, et le séparateur suivant n'est pas collé au contenu —
    /// il commence exactement au bas déclaré. C'est toute la différence entre
    /// « une taille de zone » et « un séparateur qui pousse un peu ».
    ///
    /// C'est bien le séparateur SUIVANT qu'une cote déplace, pas le premier
    /// élément venu : tout ce qui suit un séparateur de zone est le CONTENU de
    /// sa zone, et seule la rencontre d'un nouveau séparateur clôt la
    /// précédente (voir le cas inverse, `a_zone_never_grows_to_fit_a_longer_
    /// content`).
    #[test]
    fn a_zone_pushes_the_next_separator_down_to_its_declared_bottom() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let first_sep = dock.icons.len() - 1;
        dock.set_zone_height(first_sep, 400.0);
        dock.add_spacer_at_end(Side::Right);
        let inner = dock.icons.len() - 1;
        dock.set_spacer_size(inner, 40.0);
        dock.add_spacer_at_end(Side::Right);
        let second_sep = dock.icons.len() - 1;
        dock.set_zone_height(second_sep, 300.0);

        let (order, bar) = right_bar(&dock, &layout);
        let zone = bar.zones[0];
        let inner_slot = slot_of(&order, &bar, inner);
        let second_slot = slot_of(&order, &bar, second_sep);

        assert_eq!(inner_slot.top, zone.top, "le contenu de la zone commence à son haut");
        // Prémisse : le contenu est BIEN plus court que la cote, sans quoi il
        // n'y aurait aucun vide à combler.
        assert!(zone.content_height < 400.0, "contenu ({} px) plus court que la cote (400)", zone.content_height);
        assert_eq!(second_slot.top, zone.bottom, "le séparateur suivant commence au bas déclaré");
        assert!(
            second_slot.top > inner_slot.top + inner_slot.height,
            "il est plus bas que la fin du contenu : le vide est bien entre les deux"
        );
    }

    /// **Le cas inverse : un contenu plus LONG que sa cote.** La zone ne
    /// s'agrandit pas pour autant — c'est le séparateur suivant qui est RAMENÉ
    /// au bas déclaré, quitte à recouvrir le débordement (que l'étape
    /// « découpe par zone » rognera). C'est ce test qui prouve que les tailles
    /// de zones sont indépendantes de leur contenu, la demande explicite.
    #[test]
    fn a_zone_never_grows_to_fit_a_longer_content() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let first_sep = dock.icons.len() - 1;
        dock.set_zone_height(first_sep, 60.0);
        dock.add_spacer_at_end(Side::Right);
        let long = dock.icons.len() - 1;
        dock.set_spacer_size(long, 400.0); // bien plus que la cote de sa zone
        dock.add_spacer_at_end(Side::Right);
        let second_sep = dock.icons.len() - 1;
        dock.set_zone_height(second_sep, 200.0);
        dock.add_spacer_at_end(Side::Right);
        let tail = dock.icons.len() - 1;

        let (order, bar) = right_bar(&dock, &layout);
        assert_eq!(bar.zones.len(), 2, "deux séparateurs de zone, deux zones");
        let (first_zone, second_zone) = (bar.zones[0], bar.zones[1]);
        assert_eq!(bar.zones[0].sep_index, first_sep, "la zone nomme le séparateur qui l'ouvre");
        assert_eq!(bar.zones[1].sep_index, second_sep);

        // Prémisses : la zone déborde vraiment, et le séparateur suivant est
        // vraiment SOUS le bas de la zone.
        assert!(first_zone.content_height > 60.0, "contenu plus long que la cote");
        let second_slot = slot_of(&order, &bar, second_sep);
        assert_eq!(second_slot.top, first_zone.bottom, "le séparateur suivant est ramené au bas déclaré");
        assert!(
            second_slot.top < slot_of(&order, &bar, long).top + 400.0,
            "il est remonté AU-DESSUS de la fin du contenu qui déborde"
        );

        // Et la seconde zone repart de LUI, pas du contenu débordant : son
        // contenu à elle commence juste sous son séparateur.
        assert_eq!(second_zone.top, second_slot.top + second_slot.height + SPACER_GAP);
        assert_eq!(slot_of(&order, &bar, tail).top, second_zone.top, "le contenu de la 2e zone suit son séparateur");
        // Aucun séparateur ne la clôt : sa mesure se prend en fin d'ordre.
        assert!(second_zone.content_height > 0.0, "une zone sans clôture est mesurée en fin d'ordre");
    }

    /// La hauteur de contenu d'une barre tient compte du VIDE qu'une zone
    /// laisse sous son contenu : sans cela, la fin d'une grande zone resterait
    /// inatteignable au défilement, alors qu'elle occupe l'écran.
    #[test]
    fn content_height_covers_the_empty_part_of_a_zone() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let sep = dock.icons.len() - 1;
        dock.set_zone_height(sep, 900.0);

        let (order, bar) = right_bar(&dock, &layout);
        let zone = bar.zones[0];
        let last_slot_bottom = bar.slots.last().map(|slot| slot.top + slot.height).unwrap_or(0.0);
        // Prémisses : le vide va bien AU-DELÀ du dernier emplacement réel.
        assert!(last_slot_bottom < zone.bottom, "la zone laisse du vide sous son contenu");
        assert_eq!(bar.content_height(), zone.bottom, "la hauteur de contenu va jusqu'au bas de la zone");
    }

    /// `zone_scroll` ne déplace que le CONTENU de sa zone : ni sa boîte, ni le
    /// séparateur qui la clôt, qui doivent rester exactement là où la cote les
    /// a mis — c'est ce qui permettra de faire défiler une zone sans que ses
    /// voisines bougent d'un pixel.
    #[test]
    fn a_zone_scroll_moves_only_the_content_of_its_own_zone() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let first_sep = dock.icons.len() - 1;
        dock.set_zone_height(first_sep, 400.0);
        dock.add_spacer_at_end(Side::Right);
        let inner = dock.icons.len() - 1;
        dock.set_spacer_size(inner, 100.0);
        dock.add_spacer_at_end(Side::Right);
        let second_sep = dock.icons.len() - 1;
        dock.set_zone_height(second_sep, 300.0);
        dock.add_spacer_at_end(Side::Right);
        let outer = dock.icons.len() - 1;

        let order: Vec<usize> = dock.order(Side::Right).iter().copied().filter(|&i| !dock.icons[i].hidden).collect();
        let icon_size = icon_size_for(layout.right_width);
        let start_y = layout.top_thickness + TOP_MARGIN;
        let still = dock.bar_layout(Side::Right, &order, icon_size, start_y, &|_zone| 0.0, &dock.zone_grid(Side::Right, icon_size, layout.right_width));
        // La PREMIÈRE zone défile de 120 px, la seconde pas du tout — la clé
        // est l'index du SÉPARATEUR (`first_sep`), pas le rang de la zone : un
        // rang se décale dès qu'un séparateur est ajouté ou retiré ailleurs
        // dans la barre, l'index d'un espace jamais (voir `Dock::zone_scroll`).
        let scrolled = dock.bar_layout(Side::Right, &order, icon_size, start_y, &|sep| if sep == first_sep { 120.0 } else { 0.0 }, &dock.zone_grid(Side::Right, icon_size, layout.right_width));

        assert_eq!(scrolled.zones, still.zones, "les boîtes sont intangibles : seul le contenu défile");
        assert_eq!(
            slot_of(&order, &scrolled, inner).top,
            slot_of(&order, &still, inner).top - 120.0,
            "le contenu de la zone défile, lui"
        );
        assert_eq!(
            slot_of(&order, &scrolled, second_sep).top,
            slot_of(&order, &still, second_sep).top,
            "le séparateur qui CLÔT la zone ne bouge pas"
        );
        assert_eq!(
            slot_of(&order, &scrolled, outer).top,
            slot_of(&order, &still, outer).top,
            "la zone SUIVANTE ne bouge pas non plus"
        );
    }

    /// Maj + clic droit BASCULE un espace en séparateur de zone, et le refaire
    /// l'annule (voir `Dock::toggle_zone_separator`) — le seul moyen d'en créer
    /// un, donc la prémisse de tout le chantier.
    #[test]
    fn toggling_a_spacer_makes_it_declare_a_zone_and_undoes_it() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let spacer = dock.icons.len() - 1;

        assert_eq!(dock.zone_height(spacer), Some(0.0), "prémisse : un espace neuf ne déclare aucune zone");
        dock.toggle_zone_separator(spacer);
        assert_eq!(dock.zone_height(spacer), Some(DEFAULT_ZONE_HEIGHT), "la bascule pose une cote, jamais 0");
        assert!(dock.is_zone_separator(spacer));
        assert_eq!(
            dock.zone_heights(),
            vec![(spacer, DEFAULT_ZONE_HEIGHT, 0.0)],
            "et c'est ce que la persistance exporte — cote ET vide devant"
        );

        dock.toggle_zone_separator(spacer);
        assert_eq!(dock.zone_height(spacer), Some(0.0), "la refaire l'annule");
        assert!(!dock.is_zone_separator(spacer));
        assert!(dock.zone_heights().is_empty(), "un espace ordinaire ne s'exporte plus");
        // La taille de MARQUEUR, elle, n'a jamais bougé : les deux grandeurs
        // sont bien distinctes.
        assert_eq!(dock.spacer_size(spacer), Some(SPACER_HEIGHT));
    }

    /// **Le geste doit CRÉER le séparateur là où il n'y a aucun espace** —
    /// c'est le cas d'une barre VIDE, et c'est là qu'il a été essayé pour la
    /// première fois : il n'y faisait rien du tout. Même logique que le
    /// Ctrl + clic droit, qui crée un espace ordinaire aux mêmes endroits.
    #[test]
    fn a_zone_separator_can_be_created_where_there_is_no_spacer_at_all() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        assert!(dock.order(Side::Right).iter().all(|&i| !dock.is_spacer(i)), "prémisse : aucun espace à droite");

        // Dans le VIDE d'une barre : le séparateur va en fin de barre.
        dock.add_zone_separator_at_end(Side::Right, DEFAULT_ZONE_HEIGHT);
        let appended = dock.icons.len() - 1;
        assert!(dock.is_zone_separator(appended), "il déclare bien une zone");
        assert_eq!(dock.order(Side::Right).last().copied(), Some(appended), "et il est dans la barre visée");

        // SUR une icône normale : le séparateur se crée juste APRÈS elle.
        let icon = dock.order(Side::Right)[0];
        assert!(!dock.is_spacer(icon), "prémisse : c'est une icône ordinaire");
        // L'INDEX RENDU, et non `icons.len() - 1` : l'espace s'insère au plus tard à la base des
        // plages d'exécution, donc « le dernier de `icons` » n'est PAS le nouvel espace dès qu'une
        // telle plage existe (voir `insert_spacer`). Ici il n'y en a pas, mais s'en remettre à la
        // longueur ferait passer ce test à côté du jour où il y en aurait une.
        let inserted = dock.add_zone_separator_after(icon, DEFAULT_ZONE_HEIGHT).expect("création signalée");
        assert!(dock.is_zone_separator(inserted));
        assert_eq!(dock.order(Side::Right)[1], inserted, "inséré juste après l'icône cliquée");

        // Une icône qui n'appartient à aucune barre n'en crée AUCUN — et le dit,
        // pour que l'appelant ne persiste pas un ordre inchangé.
        let before = dock.icons.len();
        assert!(dock.add_zone_separator_after(usize::MAX, DEFAULT_ZONE_HEIGHT).is_none(), "échec signalé");
        assert_eq!(dock.icons.len(), before, "aucun espace fantôme n'a été ajouté");
    }

    /// **Le séparateur se DÉPLACE sous un glisser simple** — DEMANDE EXPLICITE :
    /// « je voudrais que ces séparateurs de zone soient déplaçables librement ».
    /// Sa position était dictée par la cote de la zone au-dessus de lui, et le
    /// geste n'y changeait donc rien à l'écran.
    #[test]
    fn a_zone_separator_moves_freely_under_a_plain_drag() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let sep = dock.icons.len() - 1;
        dock.toggle_zone_separator(sep);
        let (order, before) = right_bar(&dock, &layout);
        let top_before = slot_of(&order, &before, sep).top;

        let (x, y) = (3800.0, top_before + 5.0);
        dock.begin_potential_drag(sep, x, y, HWND::default());
        dock.update_drag(x, y + 60.0, false, false, &layout); // sans touche : le glisser simple

        // Le geste RÈGLE une grandeur au lieu de réordonner — c'est tout l'objet
        // du choix, et l'ordre doit donc rester intact.
        assert!(matches!(dock.end_drag(&layout, HWND::default()), DragOutcome::Resized));
        assert_eq!(dock.order(Side::Right).to_vec(), order, "aucun ordre n'a bougé");
        assert_eq!(dock.zone_lead(sep), Some(60.0), "le vide devant lui suit le curseur");
        assert_eq!(dock.zone_height(sep), Some(DEFAULT_ZONE_HEIGHT), "sa cote, elle, n'a pas bougé");

        let (_, after) = right_bar(&dock, &layout);
        assert_eq!(slot_of(&order, &after, sep).top, top_before + 60.0, "il est bien descendu de 60 px");
    }

    /// Un séparateur qui a une **zone au-dessus** de lui règle la COTE de
    /// celle-ci en se déplaçant : le marqueur EST le bord inférieur de cette
    /// zone, donc le tirer vers le bas l'agrandit. Et rien ne vient s'intercaler
    /// entre les deux zones — une frontière doit bouger, pas se détacher.
    #[test]
    fn dragging_a_separator_resizes_the_zone_above_it() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let first = dock.icons.len() - 1;
        dock.toggle_zone_separator(first);
        dock.add_spacer_at_end(Side::Right);
        let second = dock.icons.len() - 1;
        dock.toggle_zone_separator(second);
        assert_eq!(dock.previous_zone_separator(second), Some(first), "prémisse : une zone le précède");

        let (order, before) = right_bar(&dock, &layout);
        let top_before = slot_of(&order, &before, second).top;
        let (x, y) = (3800.0, top_before + 5.0);
        dock.begin_potential_drag(second, x, y, HWND::default());
        dock.update_drag(x, y + 120.0, false, false, &layout);

        assert!(matches!(dock.end_drag(&layout, HWND::default()), DragOutcome::Resized));
        assert_eq!(dock.zone_height(first), Some(DEFAULT_ZONE_HEIGHT + 120.0), "la zone du DESSUS a grandi");
        assert_eq!(dock.zone_height(second), Some(DEFAULT_ZONE_HEIGHT), "la sienne n'a pas bougé");
        assert_eq!(dock.zone_lead(second), Some(0.0), "aucun vide ne s'est glissé entre les deux zones");

        let (_, after) = right_bar(&dock, &layout);
        assert_eq!(slot_of(&order, &after, second).top, top_before + 120.0, "le marqueur a suivi le curseur");
        assert_eq!(after.zones[0].bottom, slot_of(&order, &after, second).top, "la zone du dessus va jusqu'à lui");
    }

    /// Le glisser d'un séparateur ne peut pas DESCENDRE la zone qu'il rétrécit
    /// jusqu'à l'annuler : une cote nulle en ferait un espace ordinaire (voir
    /// `Icon::zone_height`), et la structure se déferait sous la main qui est en
    /// train de la régler.
    #[test]
    fn dragging_a_separator_up_never_undoes_the_zone_above_it() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let first = dock.icons.len() - 1;
        dock.toggle_zone_separator(first);
        dock.add_spacer_at_end(Side::Right);
        let second = dock.icons.len() - 1;
        dock.toggle_zone_separator(second);

        let (order, bar) = right_bar(&dock, &layout);
        let (x, y) = (3800.0, slot_of(&order, &bar, second).top + 5.0);
        dock.begin_potential_drag(second, x, y, HWND::default());
        dock.update_drag(x, y - 100_000.0, false, false, &layout); // tiré très haut
        let _ = dock.end_drag(&layout, HWND::default());

        assert_eq!(dock.zone_height(first), Some(MIN_ZONE_HEIGHT), "plancher, jamais zéro");
        assert!(dock.is_zone_separator(first), "le séparateur du dessus déclare TOUJOURS sa zone");
        assert_eq!(bar.zones[0].sep_index, first, "prémisse : le premier séparateur ouvre bien une zone");
    }

    /// La bascule ne touche PAS une icône normale — c'est un espace qu'elle
    /// transforme, pas n'importe quel élément : même garde que
    /// `set_spacer_size`/`set_zone_height`, et un index hors bornes doit être
    /// un no-op silencieux, jamais un panic.
    #[test]
    fn toggling_a_zone_separator_ignores_anything_that_is_not_a_spacer() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        assert!(!dock.is_spacer(1), "prémisse : l'index 1 est une icône ordinaire");

        dock.toggle_zone_separator(1);
        assert_eq!(dock.zone_height(1), None, "une icône normale n'a pas de cote");
        assert!(!dock.is_zone_separator(1));
        dock.toggle_zone_separator(usize::MAX);
    }

    // ---- La cote au survol (voir `Dock::spacer_readout`) ----------------
    //
    // Le chiffre que l'interface écrit en clair à côté d'un espace : sa cote
    // s'il déclare une zone, la taille de son blanc sinon. Le survol le dit au
    // repos, et le GESTE le dit pendant un redimensionnement — c'est là qu'il
    // compte le plus, le marqueur ne suivant pas la souris.

    /// Le survol d'un espace annonce SA grandeur — chaque famille la sienne, et
    /// jamais celle de l'autre.
    #[test]
    fn hovering_a_spacer_announces_its_own_kind_of_size() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let plain = dock.icons.len() - 1;
        // Une valeur DIFFÉRENTE de `SPACER_HEIGHT` : sinon l'assertion passerait
        // encore si le code lisait la constante au lieu du réglage.
        dock.set_spacer_size(plain, 42.0);
        dock.add_spacer_at_end(Side::Right);
        let sep = dock.icons.len() - 1;
        // Idem, différente de `DEFAULT_ZONE_HEIGHT`.
        dock.set_zone_height(sep, 321.0);

        let hover = |dock: &mut Dock, index: usize| {
            let [x, y, w, h] = dock.icon_rect(index, &layout).expect("l'espace est dans une barre");
            dock.on_mouse_move(x + w * 0.5, y + h * 0.5, HWND::default());
        };

        hover(&mut dock, plain);
        assert_eq!(
            dock.spacer_readout(&layout, HWND::default()),
            Some(SpacerReadout { index: plain, is_cote: false, value: 42.0 }),
            "un espace ordinaire annonce la taille de son blanc"
        );

        hover(&mut dock, sep);
        assert_eq!(
            dock.spacer_readout(&layout, HWND::default()),
            Some(SpacerReadout { index: sep, is_cote: true, value: 321.0 }),
            "un séparateur de zone annonce sa cote, jamais sa taille de marqueur"
        );
    }

    /// Rien à annoncer pour une icône ordinaire, pour un curseur hors de la
    /// fenêtre — ni pour un espace de la barre du **HAUT**, dont la cote ne
    /// produit rien (voir `layout_top_side`) : l'annoncer serait mentir.
    #[test]
    fn only_a_lateral_spacer_announces_a_size() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::TopLeft);
        let top = dock.icons.len() - 1;
        dock.set_zone_height(top, 400.0);
        assert!(dock.is_zone_separator(top), "prémisse : il déclare bien une cote");
        assert!(dock.icon_side(top).is_some_and(|side| !side.is_lateral()), "prémisse : il est dans la barre du haut");

        let [x, y, w, h] = dock.icon_rect(top, &layout).expect("l'espace est dans la barre du haut");
        dock.on_mouse_move(x + w * 0.5, y + h * 0.5, HWND::default());
        assert_eq!(dock.spacer_readout(&layout, HWND::default()), None, "barre du haut : rien à annoncer");

        // Une icône ordinaire d'une barre LATÉRALE, survolée pour de bon.
        assert!(dock.icon_side(1).is_some_and(Side::is_lateral), "prémisse : l'index 1 est dans une barre latérale");
        let [x, y, w, h] = dock.icon_rect(1, &layout).expect("l'icône est dans une barre");
        dock.on_mouse_move(x + w * 0.5, y + h * 0.5, HWND::default());
        assert_eq!(dock.spacer_readout(&layout, HWND::default()), None, "une icône ordinaire : rien");

        dock.on_mouse_leave(HWND::default());
        assert_eq!(dock.spacer_readout(&layout, HWND::default()), None, "hors fenêtre : rien");
    }

    /// **Pendant un Maj + glisser, le chiffre suit la valeur même si la souris a
    /// quitté le marqueur** — le marqueur d'un séparateur de zone ne bouge pas
    /// (sa position est imposée par la zone qu'il clôt), donc ne mesurer que le
    /// survol ne montrerait plus rien au moment précis où ce chiffre est le seul
    /// retour disponible.
    #[test]
    fn a_resize_gesture_announces_its_value_away_from_the_marker() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let sep = dock.icons.len() - 1;
        dock.toggle_zone_separator(sep);

        let (order, bar) = right_bar(&dock, &layout);
        let (x, y) = (3800.0, slot_of(&order, &bar, sep).top + 5.0);
        dock.begin_potential_drag(sep, x, y, HWND::default());
        // Maj : le geste RÈGLE la cote au lieu de déplacer le marqueur.
        dock.update_drag(x, y + 60.0, false, true, &layout);
        // La souris est LOIN du marqueur — et c'est pourtant lui, avec sa
        // nouvelle cote, que le chiffre doit annoncer.
        dock.on_mouse_move(100.0, 100.0, HWND::default());
        assert_eq!(dock.zone_height(sep), Some(DEFAULT_ZONE_HEIGHT + 60.0), "prémisse : le geste a bien réglé la cote");

        assert_eq!(
            dock.spacer_readout(&layout, HWND::default()),
            Some(SpacerReadout { index: sep, is_cote: true, value: DEFAULT_ZONE_HEIGHT + 60.0 }),
            "le geste en cours prime sur le survol"
        );

        let _ = dock.end_drag(&layout, HWND::default());
        assert_eq!(dock.spacer_readout(&layout, HWND::default()), None, "geste terminé : plus rien à annoncer");
    }

    /// Un glisser SIMPLE sur un séparateur qui a une zone au-dessus règle la
    /// cote de CETTE zone : le chiffre annonce celle-là, et non la sienne (voir
    /// `DragAdjust::PreviousZoneCote`). L'index est le point du test — annoncer
    /// l'élément sous la main afficherait un chiffre qui ne bouge pas.
    #[test]
    fn a_plain_drag_on_a_separator_announces_the_zone_above_it() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let first = dock.icons.len() - 1;
        dock.toggle_zone_separator(first);
        dock.add_spacer_at_end(Side::Right);
        let second = dock.icons.len() - 1;
        dock.toggle_zone_separator(second);

        let (order, bar) = right_bar(&dock, &layout);
        let (x, y) = (3800.0, slot_of(&order, &bar, second).top + 5.0);
        dock.begin_potential_drag(second, x, y, HWND::default());
        dock.update_drag(x, y + 120.0, false, false, &layout); // sans touche : le glisser simple

        assert_eq!(
            dock.spacer_readout(&layout, HWND::default()),
            Some(SpacerReadout { index: first, is_cote: true, value: DEFAULT_ZONE_HEIGHT + 120.0 }),
            "c'est la cote de la zone du DESSUS qui a bougé"
        );
        let _ = dock.end_drag(&layout, HWND::default());
    }

    /// **LA SEULE AUTORITÉ sur les bornes d'épaisseur d'une barre** (voir
    /// `bar_thickness_bounds`, doc) : elle est poussée aux DEUX curseurs — celui
    /// du clic droit sur une barre et celui des Réglages.
    ///
    /// Ce test fige les valeurs ET L'ORDRE des trois barres : un décalage
    /// d'ordre donnerait à la barre du haut la borne de la gauche (40 au lieu
    /// de 16), en silence — c'est le même genre d'écart que celui qui a fait
    /// diverger les deux pages, et rien d'autre ne le verrait.
    #[test]
    fn bar_thickness_bounds_are_the_single_authority() {
        // DEMANDE EXPLICITE (2026-11-02) : « pour un écran en 3840 x 2160, haut
        // 28, bas 28, gauche 40, droite 40 ».
        let quatre_k: [f32; 4] = std::array::from_fn(|i| bar_thickness_min_pour(i, 2160.0));
        assert_eq!(quatre_k, [28.0, 40.0, 40.0, 28.0], "[haut, gauche, droite, bas] sur un 4K");
        let hd: [f32; 4] = std::array::from_fn(|i| bar_thickness_min_pour(i, 1080.0));
        assert_eq!(hd, [14.0, 20.0, 20.0, 14.0], "rapportés à la hauteur sur un 1080p");
        let (min, max) = bar_thickness_bounds(0);
        assert_eq!(max, 250.0);
        assert!(min >= 1.0);
    }

    /// Une hauteur absurde rend le minimum du 4K ; un index hors bornes, celui
    /// de la droite.
    #[test]
    fn le_minimum_d_une_barre_resiste_aux_valeurs_absurdes() {
        assert_eq!(bar_thickness_min_pour(0, 0.0), 28.0);
        assert_eq!(bar_thickness_min_pour(0, f32::NAN), 28.0);
        assert_eq!(bar_thickness_min_pour(9, 2160.0), 40.0);
        assert_eq!(bar_thickness_min_pour(0, 10.0), 1.0, "jamais sous 1");
    }

    /// **LE VERROU** : une épaisseur sous le minimum remonte au minimum, au-dessus
    /// du maximum redescend, et un NaN vaut le minimum.
    #[test]
    fn une_epaisseur_hors_bornes_est_ramenee_dans_les_bornes() {
        for i in 0..4 {
            let (min, max) = bar_thickness_bounds(i);
            assert_eq!(borner_epaisseur(i, 0.0), min);
            assert_eq!(borner_epaisseur(i, 10_000.0), max);
            assert_eq!(borner_epaisseur(i, f32::NAN), min);
            assert_eq!(borner_epaisseur(i, min + 1.0), min + 1.0);
        }
    }

    /// La marge est bornée, et une valeur absurde retombe sur le défaut.
    #[test]
    fn la_marge_des_icones_est_bornee() {
        assert_eq!(borner_marge(-5.0), MIN_ICON_MARGIN);
        assert_eq!(borner_marge(500.0), MAX_ICON_MARGIN);
        assert_eq!(borner_marge(f32::NAN), DEFAULT_ICON_MARGIN);
        assert_eq!(f32::from_bits(0x4140_0000), DEFAULT_ICON_MARGIN, "la valeur initiale de ICON_MARGIN_BITS");
    }

    /// L'échelle des icônes du haut : 100 % ne change rien, réduite elle suit,
    /// agrandie elle s'arrête à l'épaisseur — sans jamais passer sous la base.
    #[test]
    /// ⛔ **UNE BARRE FINE DONNE UNE ICÔNE FINE** (mesure du 2026-10-29 sur une
    /// VM 1280×648 : la barre de gauche convertie fait 29 px, sa marge 9, et
    /// l'icône occupait TOUTE la barre parce que le plancher de 32 la relevait).
    /// ÉTAPE 1 du plan `docs/plans/disposition-barres-zones-icones.md` : le
    /// noyau reçoit un NOM, et l'index 0 le sien sans enregistrement.
    #[test]
    fn le_noyau_recoit_un_nom_et_le_logo_le_sien() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let total = dock.icons.len();
        // Le logo, sans qu'on ait rien enregistré.
        assert_eq!(dock.identite(0).as_deref(), Some("noyau:logo"));
        // Rien n'est nommé tant que personne ne l'a posé.
        assert_eq!(dock.identite(1), None);
        // Le PLANCHER : tout ce qui est persistable et non nommé se compte.
        let manquants = dock.poser_noms(&[(1, "presse_papiers"), (2, "reglages")]);
        assert!(manquants > 0, "les 3..{total} ne sont pas nommés");
        assert_eq!(dock.identite(1).as_deref(), Some("noyau:presse_papiers"));
        assert_eq!(dock.identite(2).as_deref(), Some("noyau:reglages"));
        // Une seconde pose REMPLACE la première, elle ne s'y ajoute pas.
        dock.poser_noms(&[(1, "autre")]);
        assert_eq!(dock.identite(1).as_deref(), Some("noyau:autre"));
        assert_eq!(dock.identite(2), None);
        // Hors bornes : ni panique, ni nom.
        assert_eq!(dock.identite(total + 50), None);
        dock.poser_noms(&[(total + 50, "fantome")]);
        assert_eq!(dock.identite(total + 50), None);
    }

    /// Les PLAGES répondent aussi par leur identité — c'est ce qui permettra à
    /// une zone de citer `epingle:` ou `ouvert:` et de retrouver SON icône sur
    /// une autre machine (étape 3 du plan).
    #[test]
    fn les_plages_donnent_aussi_leur_identite() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);

        // Épingles de la barre des tâches Windows : la CIBLE, jamais la place.
        let epingles = dock.ensure_winpin_capacity(2);
        dock.poser_cles_d_epingles(vec![String::from("exe:firefox.exe"), String::new()]);
        assert_eq!(dock.identite(epingles).as_deref(), Some("epingle:exe:firefox.exe"));
        // Sans cible : un rang DIT (`#1`), pas un silence.
        assert_eq!(dock.identite(epingles + 1).as_deref(), Some("epingle:#1"));

        // Applications ouvertes : la clé dynamique (exe, ou `profile:<id>`).
        let ouvertes = dock.set_dynamic_apps(1);
        dock.set_dynamic_exes(vec![String::from("profile:abc")]);
        assert_eq!(dock.identite(ouvertes).as_deref(), Some("ouvert:profile:abc"));

        // Hors de toute plage et sans nom : rien.
        assert_eq!(dock.identite(dock.icons.len() + 10), None);
    }

    /// ÉTAPE 3 : les listes de zones pilotent les barres de GAUCHE et de DROITE,
    /// **par rang croissant** — le rang EST l'ordre, il n'y a pas de liste d'ordre
    /// à ajouter au fichier.
    #[test]
    fn les_zones_pilotent_les_ordres_de_gauche_et_de_droite() {
        use crate::ui::zones::{BarLetter, ZoneKey, ZoneRecord, Zones};

        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        // Deux applets nommés, pour avoir des identités à citer.
        dock.poser_noms(&[(1, "presse_papiers"), (2, "reglages")]);
        assert!(dock.identite(1).is_some() && dock.identite(2).is_some());

        // AUCUNE liste : rien à appliquer (c'est le cas de tous les zones.toml d'avant).
        let vides = Zones::default();
        assert_eq!(dock.ordres_des_zones(&vides), None);
        assert!(!dock.appliquer_les_zones(&vides));

        // Deux zones de DROITE : le rang 1 avant le rang 2, quoi que dise l'ordre courant.
        let mut zones = Zones::default();
        let (un, deux) = (
            ZoneKey::new(BarLetter::Droite, 1).expect("rang >= 1"),
            ZoneKey::new(BarLetter::Droite, 2).expect("rang >= 1"),
        );
        zones.zones.insert(un, ZoneRecord { cote: 100.0, icones: vec![String::from("noyau:reglages")], ..ZoneRecord::default() });
        zones.zones.insert(deux, ZoneRecord { cote: 100.0, icones: vec![String::from("noyau:presse_papiers")], ..ZoneRecord::default() });

        let (gauche, droite, haut, _, _) = dock.ordres_des_zones(&zones).expect("des listes existent");
        assert!(gauche.is_empty(), "aucune zone de gauche");
        assert!(haut.is_empty(), "aucune zone du haut");
        assert_eq!(droite, vec![2, 1], "le rang 1 (reglages=2) passe avant le rang 2 (presse-papiers=1)");

        // Une identité INCONNUE est ignorée, pas fatale.
        let mut avec_inconnue = zones.clone();
        avec_inconnue.zones.get_mut(&un).unwrap().icones.push(String::from("noyau:nexiste_pas"));
        let (_, droite, _, _, _) = dock.ordres_des_zones(&avec_inconnue).expect("les listes existent toujours");
        assert_eq!(droite, vec![2, 1], "l'inconnue ne s'ajoute pas");
    }

    #[test]
    fn une_barre_fine_donne_une_icone_fine() {
        // L'écran d'origine : 98 px de barre, 29 px de marge → 40 px d'icône,
        // soit 61 % de la barre. C'est la proportion à retrouver ailleurs.
        assert_eq!(base_laterale(98.0, 29.0), 40.0);
        // La VM : 29 px de barre, 9 px de marge → 11 px, et NON le plancher de 32.
        assert_eq!(base_laterale(29.0, 9.0), 11.0);
        // Le plancher garde son rôle quand la place le permet.
        assert_eq!(base_laterale(60.0, 12.0), 36.0);
        // Jamais au-delà du plafond.
        assert_eq!(base_laterale(400.0, 12.0), ICON_SIZE_MAX);
        // Une barre plus fine que ses propres marges ne rend jamais du négatif.
        assert!(base_laterale(16.0, 12.0) >= 1.0);
    }

    #[test]
    fn la_taille_des_icones_du_haut_suit_son_echelle() {
        assert_eq!(f32::from_bits(0x3F80_0000), DEFAULT_TOP_ICON_SCALE, "la valeur initiale de TOP_ICON_SCALE_BITS");
        assert_eq!(taille_a_l_echelle(64.0, 88.0, 1.0), 64.0);
        assert_eq!(taille_a_l_echelle(64.0, 88.0, 0.5), 32.0);
        assert_eq!(taille_a_l_echelle(64.0, 88.0, 1.25), 80.0);
        assert_eq!(taille_a_l_echelle(64.0, 88.0, 1.5), 88.0, "bornée par l'épaisseur");
        assert_eq!(taille_a_l_echelle(32.0, 16.0, 1.5), 32.0, "barre fine : la base, pas moins");
        assert_eq!(f32::from_bits(0x3F80_0000), DEFAULT_BOTTOM_ICON_SCALE, "la valeur initiale de BOTTOM_ICON_SCALE_BITS");
        assert_eq!(borner_echelle(0.1), MIN_TOP_ICON_SCALE);
        assert_eq!(borner_echelle(9.0), MAX_TOP_ICON_SCALE);
        assert_eq!(borner_echelle(f32::NAN), DEFAULT_TOP_ICON_SCALE);
    }

    /// La barre du BAS monte au TRIPLE de la borne du haut (450 %), et sa vignette
    /// s'arrête toujours à l'épaisseur de la barre.
    #[test]
    fn l_echelle_du_bas_monte_au_triple() {
        assert_eq!(MAX_BOTTOM_ICON_SCALE, 4.5);
        assert_eq!(borner_echelle_bas(4.0), 4.0, "la borne du haut ne s'applique plus au bas");
        assert_eq!(borner_echelle_bas(9.0), MAX_BOTTOM_ICON_SCALE);
        assert_eq!(borner_echelle_bas(0.1), MIN_TOP_ICON_SCALE);
        assert_eq!(borner_echelle_bas(f32::NAN), DEFAULT_BOTTOM_ICON_SCALE);
        assert_eq!(taille_a_l_echelle(80.0, 250.0, 3.0), 240.0);
        assert_eq!(taille_a_l_echelle(80.0, 250.0, 4.5), 250.0, "bornée par l'épaisseur");
    }

    /// L'échelle de la barre de gauche ne touche qu'elle : la droite garde sa taille.
    #[test]
    fn la_taille_des_icones_de_gauche_ne_touche_qu_elle() {
        assert_eq!(f32::from_bits(0x3F80_0000), DEFAULT_LEFT_ICON_SCALE, "la valeur initiale de LEFT_ICON_SCALE_BITS");
        let avant = (taille_laterale(Side::Left, 140.0, 1.0), taille_laterale(Side::Right, 140.0, 1.0));
        assert_eq!(avant.0, icon_size_for(140.0), "100 % : la taille d'avant");
        let apres = (taille_laterale(Side::Left, 140.0, 0.5), taille_laterale(Side::Right, 140.0, 0.5));
        assert_eq!(apres.0, avant.0 * 0.5);
        assert_eq!(apres.1, avant.1, "la droite ne bouge pas");
    }

    /// Le décalage déplace l'indicateur, et seulement horizontalement.
    #[test]
    fn le_decalage_deplace_l_indicateur_vers_la_droite() {
        let layout = DockLayout { total_width: 1920.0, total_height: 1080.0, top_thickness: 72.0, haut_colonnes: 72.0, left_width: 72.0, right_width: 72.0, bottom_thickness: 0.0 };
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let index = 2; // Réglages, à droite dans `Dock::new`
        let avant = dock.running_dot_instance(index, &layout, None).expect("un indicateur");
        dock.set_running_dot_offset(10.0);
        let apres = dock.running_dot_instance(index, &layout, None).expect("un indicateur");
        assert_eq!(apres.rect[0], avant.rect[0] + 10.0);
        assert_eq!(apres.rect[1..], avant.rect[1..]);
        dock.set_running_dot_offset(1000.0);
        assert_eq!(dock.running_dot_offset(), MAX_RUNNING_DOT_OFFSET);
    }

    /// À 18 px, la vignette du bas tient DANS la barre ; à l'épaisseur par défaut, elle
    /// garde sa taille d'avant.
    #[test]
    fn la_vignette_du_bas_tient_dans_une_barre_fine() {
        assert_eq!(bottom_icon_size_for(18.0), 14.0);
        assert!(bottom_icon_size_for(14.0) <= 14.0, "le minimum du bas sur un 1080p");
        assert_eq!(bottom_icon_size_for(DEFAULT_BOTTOM_THICKNESS), icon_size_for(DEFAULT_BOTTOM_THICKNESS));
        assert_eq!(bottom_icon_size_for(56.0), icon_size_for(56.0));
        // Hors bornes : les bornes de la DROITE, jamais un panic (voir la doc).
        assert_eq!(bar_thickness_bounds(9), bar_thickness_bounds(2));
    }

    // ---- LE VERROU (voir `Dock::is_locked_at`) ----------------------------

    /// **RIEN N'EST VERROUILLÉ PAR DÉFAUT** — la propriété qui garantit que
    /// cette fonctionnalité ne change rien pour l'existant, comme
    /// `zone_height = 0.0` et `zone_columns = 1` avant elle.
    /// **UN CÔTÉ SE RÉPOND POUR TOUTE BARRE QUI EXISTE** — et une zone de clic
    /// aussi.
    ///
    /// ⛔ **CE TEST EXISTE PARCE QUE DEUX DÉFAUTS JUMEAUX SONT PASSÉS**, tous deux
    /// invisibles au compilateur : `icon_side` et `side_at` rendent un
    /// `Option<Side>`, donc ce ne sont **pas** des `match` exhaustifs — l'ajout
    /// du membre `Side::Bottom` n'a fait lever **aucun** avertissement, là où les
    /// dix-huit `match` du fichier ont tous été signalés.
    ///
    /// Mesuré en service : les raccourcis de dossiers étaient chargés, leur place
    /// allouée, le fichier écrit — et **rien ne se dessinait**, parce que
    /// `render_instances` fait `let Some(side) = self.icon_side(i) else {
    /// continue }`. Et le clic droit sur la barre du bas ne faisait rien, parce
    /// que `side_at` rendait `None`.
    ///
    /// Le commentaire est long parce que la leçon l'est : **une fonction qui rend
    /// `Option<Side>` est un `match` exhaustif déguisé, et l'énumération ne le
    /// sait pas.**
    #[test]
    fn chaque_barre_a_un_cote_et_une_zone_de_clic() {
        let mut layout = geometry_layout();
        layout.bottom_thickness = 104.0;
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let base = dock.set_dossiers(&crate::config::ElementBarre::dossiers_de_test(1));

        // Chaque emplacement de la plage a un côté — c'est ce que
        // `render_instances` exige pour ne pas sauter l'icône.
        for index in base..base + 2 {
            assert_eq!(
                dock.icon_side(index),
                Some(Side::Bottom),
                "l'emplacement {index} de la barre du bas doit avoir un côté"
            );
        }

        // Le clic DANS la bande du bas désigne la barre du bas...
        let y_bande = layout.total_height - layout.bottom_thickness + 8.0;
        assert_eq!(
            dock.side_at(layout.total_width * 0.5, y_bande, &layout),
            Some(Side::Bottom)
        );
        // ...Y COMPRIS ses coins, puisqu'elle prend les coins (voir `bar_rects`,
        // qui arrête les bandes latérales au-dessus d'elle).
        assert_eq!(dock.side_at(4.0, y_bande, &layout), Some(Side::Bottom));
        assert_eq!(
            dock.side_at(layout.total_width - 4.0, y_bande, &layout),
            Some(Side::Bottom)
        );

        // Et une bande de hauteur NULLE ne réclame RIEN : la géométrie d'avant la
        // barre du bas est reproduite à l'identique (écran secondaire).
        layout.bottom_thickness = 0.0;
        assert_eq!(
            dock.side_at(layout.total_width * 0.5, layout.total_height - 4.0, &layout),
            None
        );
    }

    #[test]
    fn nothing_is_locked_until_it_is_asked_for() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        assert_eq!(dock.bar_locks(), [false; 4]);
        assert!(dock.zone_locks().is_empty());
        for index in 0..dock.icons.len() {
            assert!(!dock.is_locked_at(index), "l'icône {index} ne doit pas être verrouillée au départ");
        }
        // ...même avec une zone en place, tant qu'elle n'est pas figée.
        let sep = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(sep, 400.0);
        dock.set_icon_order(Vec::new(), vec![sep, 1, 2], Vec::new(), Vec::new());
        assert!(!dock.is_locked_at(sep));
        assert!(!dock.is_locked_at(1));
        assert!(dock.icon_side(sep).is_some(), "prémisse : le séparateur est bien dans une barre");
    }

    /// **LE VERROU D'UNE BARRE COUVRE TOUT CE QU'ELLE CONTIENT** — les icônes,
    /// les espaces, et les zones qu'elle porte.
    #[test]
    fn a_locked_bar_refuses_everything_it_contains() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let sep = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(sep, 400.0);
        dock.set_icon_order(Vec::new(), vec![sep, 1, 2], Vec::new(), Vec::new());
        let inside = [sep, 1, 2];

        assert!(inside.iter().all(|&i| !dock.is_locked_at(i)), "prémisse : rien n'est verrouillé");

        dock.set_bar_locked(Side::Right, true);
        for index in inside {
            assert!(dock.is_locked_at(index), "l'icône {index} est dans la barre verrouillée");
        }
        // ...et la barre de GAUCHE, elle, reste libre — le verrou est PAR BARRE.
        let left_icon = dock.order(Side::Left).first().copied().expect("la barre de gauche n'est pas vide");
        assert!(!dock.is_locked_at(left_icon), "verrouiller la droite ne doit pas toucher la gauche");

        dock.set_bar_locked(Side::Right, false);
        assert!(inside.iter().all(|&i| !dock.is_locked_at(i)), "et le lever rend tout");
    }

    /// **LE VERROU DE LA RANGÉE EST CELUI DE LA BARRE DU HAUT**, et d'elle seule :
    /// verrouiller la barre de GAUCHE ou de DROITE ne doit pas figer les
    /// séparateurs de la rangée — ce serait refuser un geste dans une barre que
    /// l'utilisateur n'a PAS verrouillée.
    ///
    /// Le risque est réel : `bar_rank` indexe `[haut, gauche, droite]`, donc se
    /// tromper d'un cran ferait obéir la rangée au verrou d'une barre latérale, et
    /// le défaut ne se verrait qu'en verrouillant une barre latérale puis en
    /// essayant de régler la rangée.
    ///
    /// **Le trou que ce prédicat referme** : un séparateur de rangée n'a pas
    /// d'index, donc `is_locked_at` ne pouvait pas le couvrir, et il était le SEUL
    /// élément de mise en page de la barre du haut à ignorer son verrou — dans la
    /// même barre où créer un espace était refusé.
    #[test]
    fn the_top_row_lock_follows_the_top_bar_and_no_other() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        assert!(!dock.is_top_row_locked(), "rien n'est verrouillé au départ");

        dock.set_bar_locked(Side::Left, true);
        assert!(!dock.is_top_row_locked(), "la barre de GAUCHE n'est pas celle du haut");
        dock.set_bar_locked(Side::Right, true);
        assert!(!dock.is_top_row_locked(), "la barre de DROITE non plus");

        dock.set_bar_locked(Side::TopLeft, true);
        assert!(dock.is_top_row_locked(), "la barre du haut, oui");
        // Les deux moitiés de la barre du haut n'en font qu'une seule barre.
        dock.set_bar_locked(Side::TopLeft, false);
        dock.set_bar_locked(Side::TopRight, true);
        assert!(dock.is_top_row_locked(), "les deux moitiés sont la MÊME barre");
    }

    /// **LE VERROU D'UN SÉPARATEUR FIGE SA ZONE, ET SEULEMENT ELLE** : son
    /// contenu, lui-même, et pas celui d'à côté.
    #[test]
    fn a_locked_zone_freezes_its_content_and_nothing_else() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let sep = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(sep, 400.0);
        // L'ordre met DEUX icônes dans la zone, puis un second séparateur qui
        // en ouvre une autre — pour vérifier que la seconde reste libre.
        let sep2 = dock.icons.len();
        dock.set_icon_order(Vec::new(), vec![sep, 1, 2], Vec::new(), Vec::new());
        dock.add_spacer_after(2);
        dock.set_zone_height(sep2, 200.0);

        assert_eq!(dock.previous_zone_separator(1), Some(sep), "prémisse : 1 et 2 sont dans la zone de `sep`");

        dock.set_zone_locked(sep, true);
        assert!(dock.is_locked_at(sep), "le séparateur lui-même est figé");
        assert!(dock.is_locked_at(1) && dock.is_locked_at(2), "et tout ce que sa zone contient");
        assert_eq!(dock.zone_locks(), vec![sep], "un seul séparateur figé");

        dock.set_zone_locked(sep, false);
        assert!(!dock.is_locked_at(1), "le libérer rend son contenu");
    }

    /// **CE QUE LES RÉGLAGES MONTRENT D'UNE ZONE** — de quoi la RECONNAÎTRE, et
    /// pas seulement son index : un index ne dit rien à personne, alors que sa
    /// cote, son nombre d'icônes et ses colonnes la désignent sans ambiguïté.
    #[test]
    fn a_zone_summary_describes_the_zone_well_enough_to_recognise_it() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let sep = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(sep, 300.0);
        dock.set_zone_columns(sep, 2);
        dock.set_icon_order(Vec::new(), vec![sep, 1, 2, 3], Vec::new(), Vec::new());
        dock.set_zone_locked(sep, true);

        let zones = dock.zone_summaries();
        assert_eq!(zones.len(), 1, "une seule zone dans la barre de droite");
        let zone = &zones[0];
        assert_eq!(zone.sep_index, sep, "par quoi le verrou se pose");
        assert_eq!(zone.cote, 300.0);
        assert_eq!(zone.columns, 2);
        assert_eq!(zone.content, 3, "les trois icônes qui la suivent");
        assert!(zone.locked, "et son verrou, que la case des Réglages doit refléter");

        // **UN ESPACE DANS LA ZONE N'EST PAS UNE ICÔNE** — DÉFAUT RÉEL
        // SIGNALÉ : la zone annonçait une icône là où l'écran n'en montre
        // aucune, parce que le compte incluait un espace invisible. Le nombre
        // sert à RECONNAÎTRE la zone ; il ne doit compter que ce qui se voit.
        let espace = dock.add_spacer_after(3).expect("l'icône 3 est dans la barre");
        assert!(dock.icon_side(espace) == Some(Side::Right), "prémisse : l'espace est bien dans la zone");
        assert_eq!(dock.zone_summaries()[0].content, 3, "l'espace invisible ne s'ajoute pas aux trois icônes");

        // Une zone de la barre de GAUCHE n'y figure PAS : la liste est celle de
        // la barre de droite, la seule où le verrou par séparateur est proposé.
        let gauche = dock.add_spacer_at_end(Side::Left);
        dock.set_zone_height(gauche, 200.0);
        assert_eq!(dock.zone_summaries().len(), 1, "la barre de gauche n'y entre pas");
    }

    /// **UNE ICÔNE AJOUTÉE SE RANGE APRÈS LA DERNIÈRE DE SA ZONE** — et la
    /// frontière est le séparateur SUIVANT, pas la fin de la barre.
    #[test]
    fn a_new_icon_lands_after_the_last_one_of_its_zone() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let premiere = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(premiere, 300.0);
        let seconde = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(seconde, 200.0);
        // `[première, 1, 2, seconde, 3]` : la première zone contient 1 et 2, la
        // seconde contient 3.
        dock.set_icon_order(Vec::new(), vec![premiere, 1, 2, seconde, 3], Vec::new(), Vec::new());

        assert_eq!(
            dock.zone_end_order_position(premiere),
            Some(3),
            "la première zone s'arrête au séparateur SUIVANT, pas à la fin de la barre"
        );
        assert_eq!(
            dock.zone_end_order_position(seconde),
            Some(5),
            "la dernière zone, elle, va jusqu'au bout de l'ordre"
        );
        // **UN INDEX QUI N'OUVRE PAS DE ZONE REND `None`** — et c'est le cas qui
        // protège d'un réglage périmé : mieux vaut refuser que ranger à un
        // endroit plausible et faux. L'index 1 est bien DANS la barre ici, il
        // n'est simplement pas un séparateur.
        assert_eq!(dock.zone_end_order_position(1), None, "1 est une icône, pas un séparateur");

        // **ZONE VIDE** : l'icône se pose juste après son séparateur, sans rien
        // décaler — le cas d'une zone qu'on vient de nommer et de remplir.
        let vide = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(vide, 100.0);
        dock.set_icon_order(Vec::new(), vec![vide], Vec::new(), Vec::new());
        assert_eq!(dock.zone_end_order_position(vide), Some(1), "juste après le séparateur");
    }

    /// **LA NUMÉROTATION SUIT LA POSITION, ET SAUTE LES ZONES NOMMÉES.**
    /// DEMANDE EXPLICITE : « toujours numérotées en partant du haut de la barre
    /// même si on change leur ordre », et « ces zones ne soient plus
    /// comptabilisées pour numéroter les zones automatiquement ».
    #[test]
    fn zones_are_numbered_from_the_top_and_named_ones_skip_the_count() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let mut seps = Vec::new();
        for _ in 0..3 {
            let sep = dock.add_spacer_at_end(Side::Right);
            dock.set_zone_height(sep, 200.0);
            seps.push(sep);
        }
        let par_position = |dock: &Dock| dock.zone_summaries().iter().map(|z| (z.sep_index, z.number)).collect::<Vec<_>>();
        assert_eq!(par_position(&dock), vec![(seps[0], 1), (seps[1], 2), (seps[2], 3)], "numérotées de haut en bas");

        // RENOMMER la deuxième : elle affiche son nom, et NE CONSOMME PLUS de
        // numéro — la troisième devient donc la 2ᵉ, sans trou.
        dock.set_zone_name(seps[1], Some("Musique"));
        let z = dock.zone_summaries();
        assert_eq!(z[1].name.as_deref(), Some("Musique"));
        assert_eq!(z[1].number, 0, "une zone nommée n'a pas de numéro");
        assert_eq!(par_position(&dock), vec![(seps[0], 1), (seps[1], 0), (seps[2], 2)], "et la suivante se renumérote");

        // Effacer le nom rend le numéro — la zone revient dans le compte.
        dock.set_zone_name(seps[1], None);
        assert_eq!(par_position(&dock), vec![(seps[0], 1), (seps[1], 2), (seps[2], 3)], "effacer le nom rend le numéro");
        // ...et un nom fait d'espaces ne compte pas comme un nom.
        dock.set_zone_name(seps[1], Some("   "));
        assert_eq!(dock.zone_summaries()[1].name, None, "des blancs ne sont pas un nom");

        // **LE NUMÉRO SUIT LA POSITION** : on remonte la DERNIÈRE en tête. C'est
        // elle qui doit porter le 1 — si le numéro suivait l'identité, elle
        // garderait le 3, et ce test ne le verrait pas s'il ne comparait que la
        // liste des numéros (elle serait [1,2,3] dans les deux cas).
        let mut order = dock.order(Side::Right).to_vec();
        let dernier = order.pop().expect("prémisse : la barre de droite n'est pas vide");
        order.insert(0, dernier);
        dock.set_icon_order(Vec::new(), order, Vec::new(), Vec::new());
        assert_eq!(
            par_position(&dock),
            vec![(dernier, 1), (seps[0], 2), (seps[1], 3)],
            "le numéro a suivi la POSITION, pas l'identité"
        );
    }

    /// **LE TEXTE ÉCRIT EN HAUT D'UNE ZONE** : son nom s'il en a un, sinon
    /// « Zone N » — et RIEN pour une zone qui n'a pas demandé son étiquette.
    ///
    /// Le test DISCRIMINE la renumérotation : en nommant la PREMIÈRE zone, la
    /// deuxième étiquetée doit passer de « Zone 2 » à « Zone 1 ». Un compteur
    /// local à l'affichage aurait continué de dire « Zone 2 », et une assertion
    /// qui n'aurait regardé que « y a-t-il un texte ? » aurait été verte.
    #[test]
    fn a_zone_label_is_the_name_or_the_number_and_only_when_asked() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let mut seps = Vec::new();
        for _ in 0..3 {
            let sep = dock.add_spacer_at_end(Side::Right);
            dock.set_zone_height(sep, 200.0);
            seps.push(sep);
        }
        assert!(dock.zone_labels().is_empty(), "aucune étiquette tant qu'on n'en demande pas");

        // La TROISIÈME zone demande son étiquette : elle porte le numéro 3,
        // celui du panneau Réglages — le même calcul, pas un compteur parallèle.
        dock.set_zone_label_shown(seps[2], true);
        assert_eq!(dock.zone_labels(), vec![(seps[2], "Zone 3".to_string())], "le numéro du panneau");

        // **NOMMER LES DEUX PREMIÈRES LA FAIT PASSER À « ZONE 1 »** : une zone
        // nommée ne consomme plus de numéro, donc les précédentes sortent du
        // compte. Un compteur local à l'affichage aurait continué de dire
        // « Zone 3 » — et une assertion qui n'aurait vérifié que la PRÉSENCE
        // d'un texte aurait été verte dans les deux cas.
        dock.set_zone_name(seps[0], Some("Jeux"));
        dock.set_zone_name(seps[1], Some("Musique"));
        assert_eq!(dock.zone_labels(), vec![(seps[2], "Zone 1".to_string())], "renumérotée sans trou");

        // ...et le NOM d'une zone étiquetée remplace son numéro — c'est la
        // demande : « son nom ne soit plus un nombre mais le nom donné ».
        dock.set_zone_label_shown(seps[1], true);
        assert_eq!(
            dock.zone_labels(),
            vec![(seps[1], "Musique".to_string()), (seps[2], "Zone 1".to_string())],
            "deux étiquettes, dans l'ordre de la barre"
        );

        // Une icône ne s'étiquette pas — c'est le jumeau de `set_zone_name`.
        dock.set_zone_label_shown(1, true);
        assert!(!dock.is_zone_label_shown(1), "une icône n'a pas de zone à étiqueter");
    }

    /// Un nom ne se pose QUE sur un séparateur de zone — sur une icône ou un
    /// espace ordinaire, il ne serait jamais montré.
    #[test]
    fn only_a_zone_can_be_named() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let espace = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_name(espace, Some("pas une zone"));
        assert_eq!(dock.zone_name(espace), None, "un espace ordinaire ne se nomme pas");
        dock.set_zone_name(1, Some("une icône"));
        assert_eq!(dock.zone_name(1), None, "ni une icône");
        assert!(dock.zone_names().is_empty());

        // ...mais dès qu'il devient un séparateur de zone, il accepte son nom.
        dock.set_zone_height(espace, 200.0);
        dock.set_zone_name(espace, Some("Musique"));
        assert_eq!(dock.zone_name(espace), Some("Musique"));
        assert_eq!(dock.zone_names(), vec![(espace, "Musique".to_string())]);
    }

    /// Les DEUX moitiés de la barre du haut ne font qu'UNE barre pour le verrou
    /// — c'est une seule barre vue de l'utilisateur, et c'est ainsi que les
    /// Réglages la présentent. Un décalage ici donnerait un verrou qui ne
    /// s'applique qu'à la moitié des icônes du haut.
    #[test]
    fn the_two_halves_of_the_top_bar_share_one_lock() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.set_bar_locked(Side::TopLeft, true);
        assert!(dock.is_bar_locked(Side::TopRight), "verrouiller une moitié verrouille l'autre");
        assert!(!dock.is_bar_locked(Side::Left), "et seulement le haut");
        assert_eq!(dock.bar_locks(), [true, false, false, false], "l'ordre est [haut, gauche, droite, bas]");
        // ⚠️ **LA BARRE DU BAS A SA PROPRE CASE, EN FIN.** Elle est la dernière
        // arrivée : `bar_rank` lui donne le rang 3, et ce test est ce qui
        // garantit que ce rang ne se confond ni avec la droite (2) ni avec rien.
        dock.set_bar_locked(Side::Bottom, true);
        assert_eq!(
            dock.bar_locks(),
            [true, false, false, true],
            "verrouiller le bas ne touche AUCUNE des trois autres"
        );
    }

    // ---- La GRILLE des zones (voir `Icon::zone_columns`) ------------------

    /// **LE CŒUR DE LA GRILLE** : à deux colonnes, les deux premières icônes
    /// d'une zone partagent leur rangée, la troisième en ouvre une autre, et
    /// chacune occupe SA colonne — à sa largeur, jamais au-delà.
    #[test]
    fn a_zone_with_several_columns_flows_its_content_into_rows() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let sep = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(sep, 400.0);
        dock.set_zone_columns(sep, 2);
        // Quatre icônes DANS la zone : elles suivent le séparateur dans l'ordre
        // de la barre. Les indices 1 à 4 sont des icônes ordinaires du dock.
        dock.set_icon_order(Vec::new(), vec![sep, 1, 2, 3, 4], Vec::new(), Vec::new());

        let rect = |icon: usize| dock.icon_rect(icon, &layout).expect("l'icône est dans la barre");
        let (r1, r2, r3, r4) = (rect(1), rect(2), rect(3), rect(4));

        assert_eq!(r1[1], r2[1], "les deux premières partagent leur rangée : {r1:?} {r2:?}");
        assert!(r3[1] > r1[1], "la troisième ouvre la rangée suivante : {r3:?}");
        assert_eq!(r1[0], r3[0], "et elle reprend la PREMIÈRE colonne");
        assert_eq!(r2[0], r4[0], "la quatrième reprend la seconde");
        assert!(r2[0] > r1[0], "la seconde colonne est à droite de la première");

        // L'icône RÉTRÉCIT pour tenir dans sa colonne (demande explicite), la
        // case est carrée, **et elle laisse une MARGE de `ZONE_CELL_MARGIN` de
        // chaque côté** — DEMANDE EXPLICITE : « les icônes dans les grilles des
        // zones de droite aient une marge de 4 pixels dans leur case ». C'est ce
        // que ce test fige désormais : la taille de la CASE ne suffit plus à
        // décrire le rectangle, la marge s'y soustrait.
        let width = layout.right_width;
        let bar_left = layout.total_width - width;
        let cell = Dock::zone_cell_size(icon_size_for(width), width, 2);
        assert!(cell < icon_size_for(width), "prémisse : la case est plus petite que l'icône d'origine");
        let inner = cell - zone_cell_margin() * 2.0;
        for r in [r1, r2, r3, r4] {
            assert_eq!((r[2], r[3]), (inner, inner), "icône carrée, à la case MOINS sa marge : {r:?}");
        }
        // La marge se mesure là où elle se voit : le bord GAUCHE de la première
        // icône est à exactement `ZONE_CELL_MARGIN` du bord gauche de la barre —
        // sans elle, il serait à 0 quand la case remplit la colonne.
        assert_eq!(r1[0], bar_left + zone_cell_margin(), "la marge se voit à gauche");
        // Et aucune ne mord sur sa voisine.
        assert!(r1[0] + r1[2] <= r2[0], "la colonne 0 ne déborde pas sur la 1 : {r1:?} {r2:?}");
        assert!(r2[0] + r2[2] <= layout.total_width, "ni la dernière sur le bord de l'écran");
    }

    /// Les épingles sont `[92, 92 + n[` : de 5 à 2, les index 94..97 sortent des
    /// ordres ; les épingles qui restent et les espaces au-delà (97, 120) aussi.
    #[test]
    fn une_epingle_disparue_ne_devient_pas_un_espace() {
        let mut ordre = vec![3, 92, 95, 4, 93, 94, 96, 97, 120];
        retirer_les_epingles_disparues(&mut ordre, 92, 5, 2);
        assert_eq!(ordre, vec![3, 92, 4, 93, 97, 120]);
        // Autant ou plus d'épingles : rien ne bouge.
        let mut ordre = vec![92, 95, 120];
        retirer_les_epingles_disparues(&mut ordre, 92, 4, 4);
        retirer_les_epingles_disparues(&mut ordre, 92, 4, 6);
        assert_eq!(ordre, vec![92, 95, 120]);
    }

    /// Le plancher de `MIN_ZONE_COLUMN_PX` PLAFONNE ce que la barre accorde, et
    /// le plafond SUIT sa largeur : élargir la barre rend les colonnes qu'on
    /// avait demandées, sans avoir à les redemander.
    #[test]
    fn the_column_count_is_capped_by_the_bar_width() {
        assert_eq!(Dock::max_zone_columns(140.0), 4, "140 / 32 = 4 colonnes entières");
        assert_eq!(Dock::max_zone_columns(64.0), 2);
        assert_eq!(Dock::max_zone_columns(31.0), 1, "jamais zéro colonne, même plus étroit que le plancher");
        // ⭐ Sur un écran à 100 % (96 DPI), le plancher fait 21⅓ px : une barre de
        // 62 px (124 à 150 %, adaptée) garde ses DEUX colonnes.
        assert_eq!(Dock::max_zone_columns(62.0), 1, "à 144 DPI, 62 / 32 = 1");
        poser_dpi_du_plancher_de_colonne(96);
        assert_eq!(Dock::max_zone_columns(62.0), 2, "à 96 DPI, 62 / 21,3 = 2");
        poser_dpi_du_plancher_de_colonne(DPI_DU_PLANCHER_DE_COLONNE);
        assert_eq!(plancher_de_colonne(), MIN_ZONE_COLUMN_PX);

        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let sep = dock.icons.len() - 1;
        dock.set_zone_height(sep, 400.0);
        dock.set_zone_columns(sep, 6);

        assert_eq!(dock.zone_columns(sep), Some(6), "la DEMANDE est conservée telle quelle");
        assert_eq!(dock.effective_zone_columns(sep, Side::Right, 140.0), 4, "mais la barre n'en accorde que 4");
        assert_eq!(dock.effective_zone_columns(sep, Side::Right, 240.0), 6, "élargie à 240, elle les rend toutes");
        // DEMANDE EXPLICITE : la grille est celle de la barre de DROITE. Une
        // zone de la barre de gauche s'empile, même si elle déclare des
        // colonnes — et le réglage lui survit si on la déplace.
        assert_eq!(dock.effective_zone_columns(sep, Side::Left, 240.0), 1, "la barre de gauche s'empile");
    }

    /// Une icône ne dépasse JAMAIS sa colonne, et ne grandit jamais non plus :
    /// à une colonne elle garde exactement sa taille d'origine — ce qui est ce
    /// qui rend le défaut (une colonne) identique à l'avant-grilles.
    #[test]
    fn a_cell_never_exceeds_its_column_and_never_grows() {
        assert_eq!(Dock::zone_cell_size(56.0, 140.0, 4), 35.0, "elle rétrécit à la largeur de colonne");
        assert_eq!(Dock::zone_cell_size(56.0, 140.0, 1), 56.0, "à une colonne, elle garde sa taille");
        assert_eq!(Dock::zone_cell_size(30.0, 140.0, 2), 30.0, "et elle ne GRANDIT pas pour remplir la colonne");
    }

    // ---- Le LÂCHER dans une grille (voir `Dock::drag_insert_index`) --------

    /// Le dock des tests de lâcher : une zone à `columns` colonnes dans la barre
    /// de DROITE, contenant les icônes `1..=contenu`, et l'icône **9** dans la
    /// barre de GAUCHE — c'est elle qu'on glisse, donc `from_side` est la gauche
    /// et le Ctrl fait réellement changer de barre (voir `drag_target_side`).
    ///
    /// ⚠️ **Indices 0..=9 SEULEMENT** : `Dock::new(background, 0)` crée dix
    /// icônes (les `pinned_count` épingles s'y ajoutent, voir son `for`), et
    /// `set_icon_order` ÉCARTE en silence un index hors bornes — un 14 ici
    /// laisserait la barre de gauche vide, `begin_potential_drag` sans effet, et
    /// le test conclurait « aucun glisser » au lieu de conclure juste.
    ///
    /// ⚠️ **La barre de gauche n'est PAS réduite à l'icône 9** :
    /// `set_icon_order` place les icônes LISTÉES en tête et **rend les autres à
    /// leur barre** (mesuré : la gauche garde `0, 5, 6, 7, 8`). Aucun test
    /// n'asserte donc sa liste entière, seulement ce que le geste y change.
    ///
    /// Rend aussi la disposition et le séparateur, pour que chaque test n'ait
    /// pas à recalculer ce que `bar_layout` sait déjà.
    fn dock_pour_lacher(columns: u16, contenu: usize) -> (Dock, DockLayout, usize) {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let sep = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(sep, 400.0);
        dock.set_zone_columns(sep, columns);
        let mut droite = vec![sep];
        droite.extend(1..=contenu);
        dock.set_icon_order(vec![9], droite, Vec::new(), Vec::new());
        (dock, layout, sep)
    }

    /// Lâche l'icône 9 (voir `dock_pour_lacher`) sur `(x, y)`, **Ctrl maintenu**
    /// pour que la barre de droite soit la cible. Rend ce que `end_drag` a
    /// résolu, et l'ordre de la barre de droite APRÈS le geste.
    fn lacher_sur(dock: &mut Dock, layout: &DockLayout, x: f32, y: f32) -> (DragOutcome, Vec<usize>) {
        dock.begin_potential_drag(9, 40.0, 300.0, HWND::default());
        dock.update_drag(x, y, true, false, layout);
        let issue = dock.end_drag(layout, HWND::default());
        (issue, dock.order(Side::Right).to_vec())
    }

    /// **LE DÉFAUT QUE CE CORRECTIF RÉPARE, ET LE TEST QUI LE FIXE.** Une icône
    /// lâchée sur la SECONDE COLONNE d'une rangée doit se ranger **dans cette
    /// case** — ni dans la première, ni dans la rangée suivante.
    ///
    /// Avant, `drag_insert_index` ne comparait que l'ORDONNÉE : les deux cases
    /// d'une même rangée partagent leur `top`, donc elles rendaient le MÊME
    /// index, et le nombre de colonnes n'avait **aucun** effet sur le geste.
    #[test]
    fn a_drop_lands_in_the_column_under_the_cursor() {
        let (mut dock, layout, _) = dock_pour_lacher(2, 4);
        let case = dock.icon_rect(2, &layout).expect("l'icône 2 est dans la barre");
        let (x, y) = (case[0] + case[2] * 0.5, case[1] + case[3] * 0.5);

        let (issue, droite) = lacher_sur(&mut dock, &layout, x, y);

        assert!(matches!(issue, DragOutcome::Moved), "le lâcher est accepté, obtenu {issue:?}");
        let sep = droite[0];
        assert_eq!(
            droite,
            vec![sep, 1, 9, 2, 3, 4],
            "l'icône déposée prend la case de l'icône 2 — SECONDE colonne, pas la première"
        );
    }

    /// **L'INVARIANT QUE LE DÉFAUT VIOLAIT** : après le lâcher, le point du
    /// curseur tombe DANS le rectangle DESSINÉ de l'icône déposée. C'est la
    /// propriété que ce fichier énonce pour `zone_grid` — « sans quoi une zone
    /// serait cliquable ailleurs que là où elle se voit » — et la seule qui
    /// compte pour l'œil : viser une case, c'est la voir s'occuper.
    ///
    /// Les deux points visés sont dans la MÊME rangée et ne diffèrent que par
    /// l'abscisse : c'est précisément ce que l'ancien calcul ne distinguait pas.
    #[test]
    fn the_dropped_icon_covers_the_cell_that_was_aimed_at() {
        for (vise, laquelle) in [(1usize, "première"), (2, "seconde")] {
            let (mut dock, layout, _) = dock_pour_lacher(2, 4);
            let case = dock.icon_rect(vise, &layout).expect("l'icône visée est dans la barre");
            let (x, y) = (case[0] + case[2] * 0.5, case[1] + case[3] * 0.5);

            let (issue, _) = lacher_sur(&mut dock, &layout, x, y);
            assert!(matches!(issue, DragOutcome::Moved), "le lâcher est accepté");

            let deposee = dock.icon_rect(9, &layout).expect("l'icône déposée est dans la barre");
            assert!(
                x >= deposee[0] && x < deposee[0] + deposee[2] && y >= deposee[1] && y < deposee[1] + deposee[3],
                "le curseur ({x}, {y}) doit tomber dans la case DESSINÉE de l'icône déposée \
                 ({deposee:?}) — visé : {laquelle} colonne"
            );
        }
    }

    /// **LE CAS DÉGÉNÉRÉ, ET C'EST LA GARANTIE QUI COMPTE** : une zone à UNE
    /// colonne — l'état de toute zone créée avant les grilles — s'insère comme
    /// avant elles. La règle d'origine est écrite ici À LA MAIN : c'est le CENTRE
    /// de l'emplacement qui tranche, et le curseur placé SOUS ce centre doit
    /// donc passer avant l'emplacement SUIVANT.
    #[test]
    fn a_single_column_zone_inserts_exactly_as_before_grids() {
        let (mut dock, layout, _) = dock_pour_lacher(DEFAULT_ZONE_COLUMNS, 4);
        let (order, bar) = right_bar(&dock, &layout);
        let deux = slot_of(&order, &bar, 2);
        let centre = deux.top + deux.height * 0.5;
        let y = deux.top + deux.height * 0.75;
        assert!(y >= centre, "prémisse : le curseur est SOUS le centre de l'emplacement");
        let x = Dock::centered_icon_x(Side::Right, &layout, icon_size_for(layout.right_width));

        let (issue, droite) = lacher_sur(&mut dock, &layout, x, y);

        assert!(matches!(issue, DragOutcome::Moved), "le lâcher est accepté, obtenu {issue:?}");
        let sep = droite[0];
        assert_eq!(droite, vec![sep, 1, 2, 9, 3, 4], "sous le centre de l'icône 2 → AVANT l'icône 3");
    }

    /// Une barre QUI A des zones refuse un lâcher qui n'en vise aucune — ici
    /// dans la marge haute, au-dessus du premier séparateur. L'ordre reste
    /// **intact**, et l'icône n'a même pas quitté sa barre : c'est ce que
    /// `DragOutcome::Refused` annonce à l'appelant (rien à persister).
    #[test]
    fn a_drop_outside_every_zone_is_refused_in_a_bar_that_has_zones() {
        let (mut dock, layout, _) = dock_pour_lacher(2, 4);
        let avant = dock.order(Side::Right).to_vec();
        let x = Dock::centered_icon_x(Side::Right, &layout, icon_size_for(layout.right_width));
        let y = layout.top_thickness + 8.0;

        let (issue, droite) = lacher_sur(&mut dock, &layout, x, y);

        assert!(matches!(issue, DragOutcome::Refused), "aucune zone sous le curseur");
        assert_eq!(droite, avant, "et l'ordre n'a pas bougé d'un cran");
        assert!(dock.order(Side::Left).contains(&9), "l'icône n'a même pas quitté sa barre");
    }

    /// **NON-RÉGRESSION, PAR L'AUTRE BOUT** : une barre SANS aucune zone garde
    /// l'insertion LINÉAIRE d'avant les zones. Rien n'y est jamais refusé — la
    /// barre de gauche n'a aucun séparateur, et c'est elle qui porte le miroir
    /// des épingles de Windows.
    #[test]
    fn a_drop_stays_free_in_a_bar_without_zones() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.set_icon_order(vec![9, 8], Vec::new(), Vec::new(), Vec::new());
        assert!(dock.order(Side::Right).is_empty(), "prémisse : la barre de droite est vide, donc sans zone");
        // `set_icon_order` place les icônes LISTÉES en tête et **rend les autres
        // à leur barre** (mesuré : la gauche garde 0..=8) : on n'asserte donc pas
        // la liste entière, seulement ce que le geste doit changer.
        let avant = dock.order(Side::Left).to_vec();
        assert_ne!(avant.last().copied(), Some(9), "prémisse : il n'était pas déjà en fin — {avant:?}");

        // Bien plus bas que toutes les icônes : l'insertion LINÉAIRE le met en fin.
        dock.begin_potential_drag(9, 40.0, 90.0, HWND::default());
        dock.update_drag(40.0, 6000.0, false, false, &layout);
        let issue = dock.end_drag(&layout, HWND::default());

        assert!(matches!(issue, DragOutcome::Moved), "aucun refus dans une barre sans zone, obtenu {issue:?}");
        assert_eq!(
            dock.order(Side::Left).last().copied(),
            Some(9),
            "l'insertion linéaire l'a descendu en fin de barre — avant : {avant:?}"
        );
    }

    /// **LE SÉPARATEUR RESTE LIBRE** — décision explicite : c'est avec lui qu'on
    /// réordonne les zones, donc l'enfermer dans l'une d'elles rendrait ce
    /// réordonnancement impossible. Glissé à un endroit où AUCUNE icône ne
    /// serait acceptée (au-dessus de la première zone), il est quand même pris.
    ///
    /// ⚠️ **Ctrl est nécessaire pour le DÉPLACER** : le glisser simple sur un
    /// séparateur règle une grandeur, jamais l'ordre (voir `Dock::update_drag`,
    /// décision inchangée).
    #[test]
    fn a_zone_separator_is_never_constrained_by_a_zone() {
        let (mut dock, layout, sep) = dock_pour_lacher(2, 4);
        let second = dock.add_spacer_at_end(Side::Right);
        dock.toggle_zone_separator(second);
        assert_eq!(dock.order(Side::Right).to_vec(), vec![sep, 1, 2, 3, 4, second], "prémisse : il clôt la barre");

        let x = Dock::centered_icon_x(Side::Right, &layout, icon_size_for(layout.right_width));
        let y = layout.top_thickness + 8.0;
        dock.begin_potential_drag(second, x, 4000.0, HWND::default());
        dock.update_drag(x, y, true, false, &layout);
        let issue = dock.end_drag(&layout, HWND::default());

        assert!(matches!(issue, DragOutcome::Moved), "un séparateur n'est jamais refusé");
        assert_eq!(dock.order(Side::Right).first().copied(), Some(second), "il est remonté en tête");
    }

    /// **LE GESTE QUI CRÉE UNE ZONE LAISSE UNE ICÔNE AU-DESSUS D'ELLE.** Maj +
    /// clic droit sur une icône pose le séparateur **juste APRÈS elle** (voir
    /// `app::run`, `add_zone_separator_after`) : la disposition ordinaire est donc
    /// `[icône, séparateur, …]`. Lâcher sur cette icône doit continuer de marcher
    /// — c'est exactement le cas qu'un refus « hors de toute zone » aurait rendu
    /// silencieusement inerte, et le coût qui a fait restreindre le refus au VIDE.
    #[test]
    fn a_drop_on_content_above_the_first_zone_still_moves_it() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let sep = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(sep, 400.0);
        dock.set_zone_columns(sep, 2);
        // L'icône 1 AVANT le séparateur : la disposition que le geste produit.
        dock.set_icon_order(vec![9], vec![1, sep, 2, 3], Vec::new(), Vec::new());
        let (order, bar) = right_bar(&dock, &layout);
        assert!(slot_of(&order, &bar, 1).top < bar.zones[0].top, "prémisse : l'icône 1 est AU-DESSUS de la zone");

        let case = dock.icon_rect(1, &layout).expect("l'icône 1 est dans la barre");
        let (x, y) = (case[0] + case[2] * 0.5, case[1] + case[3] * 0.5);
        let (issue, droite) = lacher_sur(&mut dock, &layout, x, y);

        assert!(matches!(issue, DragOutcome::Moved), "lâcher sur une icône hors zone : accepté, obtenu {issue:?}");
        assert_eq!(droite, vec![1, 9, sep, 2, 3], "l'icône s'est posée juste sous celle du haut");
    }

    /// **LE VIDE EST LE SEUL REFUS.** Sous la cote de la dernière zone, il n'y a
    /// plus rien à quoi se lier. C'est le pendant exact du test de la barre SANS
    /// zone, où le même geste descend l'icône en fin de barre : la différence
    /// tient à ce qu'il y ait, ou non, des zones dans la barre.
    #[test]
    fn a_drop_below_the_last_zone_is_refused() {
        let (mut dock, layout, _) = dock_pour_lacher(2, 4);
        let avant = dock.order(Side::Right).to_vec();
        let (_, bar) = right_bar(&dock, &layout);
        let bas = bar.zones.last().expect("prémisse : le séparateur ouvre une zone").bottom;
        let x = Dock::centered_icon_x(Side::Right, &layout, icon_size_for(layout.right_width));

        let (issue, droite) = lacher_sur(&mut dock, &layout, x, bas + 40.0);

        assert!(matches!(issue, DragOutcome::Refused), "rien à lier sous la dernière zone, obtenu {issue:?}");
        assert_eq!(droite, avant, "et l'ordre est intact");
    }

    /// **UNE ICÔNE DÉSACTIVÉE NE COMPTE PAS DANS LA GRILLE MESURÉE**, parce
    /// qu'elle n'est pas dessinée : `layout_side` l'écarte, le lâcher doit
    /// l'écarter aussi. Sinon la case visée glisse d'une case par icône
    /// désactivée — le défaut même que ce calcul répare, resté vivant sous cette
    /// forme.
    ///
    /// Les deux assertions comptent : la case VISÉE est bien occupée, **et**
    /// l'icône désactivée garde sa place dans l'ordre (c'est ce que la
    /// traduction du rang assure, et `others` est ce qui est persisté).
    #[test]
    fn a_hidden_icon_does_not_shift_the_cell_a_drop_targets() {
        let (mut dock, layout, sep) = dock_pour_lacher(2, 4);
        dock.set_icon_hidden(2, true);
        let (order, _) = right_bar(&dock, &layout);
        assert_eq!(order, vec![sep, 1, 3, 4], "prémisse : l'icône 2 n'est plus dessinée");

        // Désactivée, l'icône 4 occupe la case (rangée 1, colonne 0) — celle
        // qu'un calcul sur la liste COMPLÈTE aurait donnée à l'icône 3.
        let case = dock.icon_rect(4, &layout).expect("l'icône 4 est dans la barre");
        let (x, y) = (case[0] + case[2] * 0.5, case[1] + case[3] * 0.5);
        let (issue, droite) = lacher_sur(&mut dock, &layout, x, y);

        assert!(matches!(issue, DragOutcome::Moved), "obtenu {issue:?}");
        assert_eq!(droite, vec![sep, 1, 2, 3, 9, 4], "l'icône désactivée garde SA place dans l'ordre");
        let dessine: Vec<usize> = droite.iter().copied().filter(|&i| i != 2).collect();
        assert_eq!(dessine, vec![sep, 1, 3, 9, 4], "et l'icône déposée occupe la case VISIBLE visée");
        let deposee = dock.icon_rect(9, &layout).expect("l'icône déposée est dans la barre");
        assert!(
            x >= deposee[0] && x < deposee[0] + deposee[2] && y >= deposee[1] && y < deposee[1] + deposee[3],
            "le curseur ({x}, {y}) doit tomber dans la case dessinée de l'icône déposée ({deposee:?})"
        );
    }

    /// **LA GOUTTIÈRE ENTRE DEUX RANGÉES GARDE LA COLONNE VISÉE.** Entre deux
    /// rangées (l'`icon_gap`), aucune case ne contient le curseur ; la rangée qui
    /// suit commence plus bas, et y retomber en colonne 1 renverrait tout à gauche
    /// une icône que le curseur visait à droite. C'est le défaut réparé, à
    /// l'échelle d'une gouttière.
    ///
    /// Le contraste avec la barre SANS zone est voulu : là, la règle linéaire
    /// n'a pas de colonne à conserver.
    #[test]
    fn a_drop_in_the_row_gutter_keeps_the_column_that_was_aimed_at() {
        let (mut dock, layout, sep) = dock_pour_lacher(2, 4);
        let (order, bar) = right_bar(&dock, &layout);
        let haut = slot_of(&order, &bar, 1);
        let suivante = slot_of(&order, &bar, 3);
        assert!(suivante.top > haut.top + haut.height, "prémisse : un intervalle sépare les deux rangées");

        // Dans l'intervalle, et sous la SECONDE colonne.
        let (colonne1, largeur) = Dock::column_band(Side::Right, &layout, 2, 1);
        let x = colonne1 + largeur * 0.5;
        let (issue, droite) = lacher_sur(&mut dock, &layout, x, haut.top + haut.height + 1.0);

        assert!(matches!(issue, DragOutcome::Moved), "obtenu {issue:?}");
        assert_eq!(
            droite,
            vec![sep, 1, 2, 3, 9, 4],
            "la colonne 1 est conservée : l'icône prend la case de l'icône 4, pas celle de l'icône 3"
        );
    }

    /// **UN SÉPARATEUR FIGÉ NE FIGE QUE LUI-MÊME.** `is_locked_at` le fait
    /// regarder SA propre zone, et non celle d'avant (voir sa doc) — sans quoi
    /// le séparateur qui SUIT une zone verrouillée devenait indéplaçable, lui
    /// aussi. Et c'est MUET : `update_drag` refuse alors de seulement commencer
    /// le glisser, donc le geste reste un CLIC et rien ne se passe.
    ///
    /// C'est la trappe dans laquelle on tombe en réordonnant : le marqueur qu'on
    /// attrape est celui qui suit la zone qu'on a verrouillée.
    #[test]
    fn a_separator_after_a_locked_zone_can_still_be_dragged() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let mut sep = Vec::new();
        for _ in 0..3 {
            let index = dock.add_spacer_at_end(Side::Right);
            dock.toggle_zone_separator(index);
            sep.push(index);
        }
        let (a, b, c) = (sep[0], sep[1], sep[2]);
        dock.set_zone_locked(a, true);
        // La barre de droite réduite à ses trois marqueurs : l'assertion d'ordre
        // reste lisible.
        dock.set_icon_order(Vec::new(), vec![a, b, c], Vec::new(), Vec::new());

        assert!(dock.is_locked_at(a), "la zone est figée : son PROPRE marqueur l'est aussi");
        assert!(!dock.is_locked_at(b), "mais le marqueur SUIVANT ne l'est pas : sa zone n'est pas la même");
        assert!(!dock.is_locked_at(c), "ni celui d'après");

        // `b` suit la zone verrouillée : c'est LUI qui doit pouvoir se déplacer.
        // On le descend après `c`, dont la zone est libre — le verrou se contrôle
        // sur l'icône VOISINE du point d'insertion (voir `end_drag`), donc viser
        // un voisin libre est ce qui isole la règle testée ici.
        let (_, bar) = right_bar(&dock, &layout);
        let ordre = dock.order(Side::Right).to_vec();
        let sous_c = slot_of(&ordre, &bar, c);
        let x = Dock::centered_icon_x(Side::Right, &layout, icon_size_for(layout.right_width));
        dock.begin_potential_drag(b, x, sous_c.top, HWND::default());
        dock.update_drag(x, sous_c.top + 600.0, true, false, &layout);

        assert!(
            matches!(dock.end_drag(&layout, HWND::default()), DragOutcome::Moved),
            "il se déplace malgré le verrou de la zone d'avant"
        );
        assert_eq!(dock.order(Side::Right).to_vec(), vec![a, c, b], "il est descendu après c");
    }

    // ---- Le BLOC qu'un glisser déplace, et les commandes de zone ----------

    /// Le dock des tests de bloc : la barre de DROITE porte `[sepA, 1, 2, sepB, 3]` — deux zones,
    /// la première avec deux icônes, la seconde avec une — et l'icône **9** est laissée à la barre
    /// de GAUCHE, pour les gestes qui déplacent une icône ordinaire.
    ///
    /// Rend `(dock, disposition, sepA, sepB)`.
    fn dock_deux_zones() -> (Dock, DockLayout, usize, usize) {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let sep_a = dock.add_spacer_at_end(Side::Right);
        let sep_b = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(sep_a, 400.0);
        dock.set_zone_height(sep_b, 300.0);
        dock.set_icon_order(vec![9], vec![sep_a, 1, 2, sep_b, 3], Vec::new(), Vec::new());
        (dock, layout, sep_a, sep_b)
    }

    /// Ctrl + lâcher de `glisse` sur `(x, y)` — LE geste de réordonnancement (le glisser simple
    /// règle une cote, voir `update_drag`). Rend l'issue et l'ordre de la barre de droite.
    ///
    /// ⚠️ **LE DÉPART EST DÉLIBÉRÉMENT AILLEURS QUE LA CIBLE** (`y = 200`) : un glisser partant du
    /// point d'arrivée ne dépasse pas `DRAG_THRESHOLD`, reste un CLIC, et le test conclurait
    /// « aucun glisser » au lieu de conclure juste.
    fn lacher_element(dock: &mut Dock, layout: &DockLayout, glisse: usize, x: f32, y: f32) -> (DragOutcome, Vec<usize>) {
        dock.begin_potential_drag(glisse, x, 200.0, HWND::default());
        dock.update_drag(x, y, true, false, layout);
        let issue = dock.end_drag(layout, HWND::default());
        (issue, dock.order(Side::Right).to_vec())
    }

    /// ⭐ **UN GLISSER DANS LA BARRE DU BAS RÉORDONNE — CE QUE RIEN NE VÉRIFIAIT.**
    ///
    /// Le lâcher y était **refusé** : `drag_insert_index` rendait `None`, avec pour
    /// raison écrite « `bottom_order` est toujours vide » — faux depuis
    /// `set_dossiers`, qui la remplit à chaque appel. Le geste commençait donc,
    /// l'icône flottante suivait le curseur, et **RIEN ne se passait** : le pire
    /// des états, il promet.
    ///
    /// ⚠️ **TOUS LES TESTS DE CETTE BARRE PASSAIENT PAR `set_dossiers`**, insertion
    /// ou retrait — jamais par le glisser. C'est ce trou qui a laissé le défaut
    /// vivre, et ce test le ferme.
    ///
    /// ⚠️ **AVEC Ctrl, ET C'EST LE GESTE DEMANDÉ** (« glisser l'icône avec Ctrl +
    /// clic gauche ») : il vérifie donc aussi que Ctrl, qui veut dire « viser une
    /// AUTRE barre » partout ailleurs, réordonne SUR PLACE dans celle-ci.
    #[test]
    fn un_glisser_dans_la_barre_du_bas_reordonne() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let base = dock.set_dossiers(&crate::config::ElementBarre::dossiers_de_test(3));
        // ⚠️ **`bottom_thickness` NON NUL, ET C'EST LE POINT** : la maquette des
        // autres tests la laisse à `0.0` — une barre du bas de hauteur nulle, où
        // aucune place n'existe, donc où aucun lâcher n'est mesurable.
        let layout = DockLayout {
            total_width: 3840.0,
            total_height: 2088.0,
            top_thickness: 72.0,
            haut_colonnes: 72.0,
            left_width: 140.0,
            right_width: 140.0,
            bottom_thickness: DEFAULT_BOTTOM_THICKNESS,
        };
        let avant = dock.order(Side::Bottom).to_vec();
        assert_eq!(avant.len(), 4, "trois dossiers, puis le « + »");

        // On attrape le PREMIER dossier et on le lâche sur la case du TROISIÈME —
        // assez loin pour franchir `DRAG_THRESHOLD`, sinon ce serait un clic.
        let cible = dock.icon_rect(base + 2, &layout).expect("le troisieme dossier a une place");
        let (x, y) = (cible[0] + cible[2] * 0.5, cible[1] + cible[3] * 0.5);

        dock.begin_potential_drag(base, x - 200.0, y, HWND::default());
        dock.update_drag(x, y, true, false, &layout);
        let issue = dock.end_drag(&layout, HWND::default());

        assert!(matches!(issue, DragOutcome::Moved), "le lacher doit etre ACCEPTE, obtenu {issue:?}");
        let apres = dock.order(Side::Bottom).to_vec();
        assert_ne!(apres, avant, "l'ordre doit avoir change");
        assert_eq!(apres.len(), avant.len(), "sans perdre ni gagner de case");
        assert_eq!(*apres.last().expect("non vide"), base + 3, "le « + » reste le DERNIER");
        assert_ne!(apres[0], base, "le dossier glisse ne doit plus etre en tete");
    }

    /// ⛔ **UN ESPACE ÉLASTIQUE NE DOIT PAS FAUSSER LE DÉPÔT — C'EST LE DÉFAUT
    /// SIGNALÉ.** « ça bouge, mais pas au bon endroit » : la première version
    /// calculait une grille RÉGULIÈRE (`ancre + i × pas`), alors que les espaces
    /// élastiques absorbent la largeur restante et **décalent tout ce qui les
    /// suit**. Le cas réel de l'utilisateur — six élastiques en tête — rendait le
    /// dépôt faux partout.
    ///
    /// Le remède est de MESURER : `drag_insert_index` appelle maintenant la
    /// fonction qui DESSINE la barre. Ce test est ce qui distingue les deux — une
    /// grille régulière y trouve un autre rang.
    #[test]
    fn un_espace_elastique_ne_fausse_pas_le_depot() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.ensure_spacer_capacity(10);
        let mut elements = vec![crate::config::ElementBarre::Separateur(
            crate::ui::zones::SorteEspace::Elastique,
        )];
        elements.extend(crate::config::ElementBarre::dossiers_de_test(3));
        let base = dock.set_dossiers(&elements);
        let layout = DockLayout {
            total_width: 3840.0,
            total_height: 2088.0,
            top_thickness: 72.0,
            haut_colonnes: 72.0,
            left_width: 140.0,
            right_width: 140.0,
            bottom_thickness: DEFAULT_BOTTOM_THICKNESS,
        };

        // ⚠️ **ON VISE LA PREMIÈRE CASE, PAS LA DERNIÈRE** — et ce n'est pas un
        // détail : viser la dernière faisait retomber les deux géométries sur le
        // MÊME rang (toutes deux « au-delà de tout »), donc le test passait avec
        // la grille régulière et ne prouvait rien. Ici, l'élastique a décalé les
        // icônes vers la droite, et le modèle régulier se trompe franchement.
        let cible = dock.icon_rect(base, &layout).expect("le premier dossier a une place");
        let (x, y) = (cible[0] + cible[2] * 0.5, cible[1] + cible[3] * 0.5);

        dock.begin_potential_drag(base + 2, x - 300.0, y, HWND::default());
        dock.update_drag(x, y, true, false, &layout);
        let issue = dock.end_drag(&layout, HWND::default());
        assert!(matches!(issue, DragOutcome::Moved), "obtenu {issue:?}");

        let apres = dock.order(Side::Bottom).to_vec();
        let elastique = dock.plus_dossier_index().expect("la plage") + 1;
        println!("ordre apres depot : {apres:?} (base={base}, elastique={elastique})");
        assert_eq!(apres.len(), 5, "un espace, trois dossiers, le « + »");
        assert_eq!(apres[0], elastique, "l'elastique reste en tete");
        assert_eq!(*apres.last().unwrap(), base + 3, "le « + » reste le dernier");
        // ⭐ **LE POINT DU TEST** : le dossier glissé se pose **À CÔTÉ de la case
        // visée** — pas à l'autre bout de la rangée.
        //
        // ⚠️ **« À CÔTÉ », PAS « JUSTE APRÈS »** : la rangée se RÉORGANISE pendant
        // le geste (l'icône glissée quitte la liste, et l'élastique reprend sa
        // largeur), donc la case visée s'est décalée d'un cran à l'instant du
        // lâcher. Exiger « juste après » dépendrait de ce décalage — c'est le même
        // piège que documente `moving_an_ordinary_icon_moves_only_it`, qui vise le
        // QUART supérieur et non le centre pour cette raison. Le défaut signalé
        // (« ça bouge, mais pas au bon endroit ») envoyait l'icône EN FIN DE
        // RANGÉE : c'est cet écart-là que ce test refuse.
        let rang_du_glisse =
            apres.iter().position(|&i| i == base + 2).expect("il est dans la rangee");
        let rang_vise = apres.iter().position(|&i| i == base).expect("la cible aussi");
        let ecart = rang_du_glisse.abs_diff(rang_vise);
        assert_eq!(
            ecart, 1,
            "le dossier doit se poser A COTE de la case visee — obtenu {apres:?}"
        );
    }

    /// ⛔ **UN GLISSER SIMPLE NE RÉORDONNE PAS LA BARRE DU BAS — Ctrl est exigé.**
    ///
    /// DEMANDE EXPLICITE : « les dossiers ne devraient pas être organisables avec
    /// un simple clic gauche ». Ailleurs, le glisser simple réordonne ; ici, un
    /// clic simple **ouvre** le dossier — deux gestes voisins, deux effets sans
    /// aucun rapport, que Ctrl sépare.
    ///
    /// ⚠️ **ET LE GESTE NE DOIT PAS PROMETTRE** : `drag_preview` consulte la même
    /// règle. Sans ça, l'icône flottante suivrait le curseur et le lâcher ne ferait
    /// rien — le défaut qu'on vient de corriger sur cette même barre.
    #[test]
    fn un_glisser_simple_ne_reordonne_pas_la_barre_du_bas() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let base = dock.set_dossiers(&crate::config::ElementBarre::dossiers_de_test(3));
        let layout = DockLayout {
            total_width: 3840.0,
            total_height: 2088.0,
            top_thickness: 72.0,
            haut_colonnes: 72.0,
            left_width: 140.0,
            right_width: 140.0,
            bottom_thickness: DEFAULT_BOTTOM_THICKNESS,
        };
        let avant = dock.order(Side::Bottom).to_vec();
        let places_avant: Vec<Option<[f32; 4]>> =
            (0..4).map(|i| dock.icon_rect(base + i, &layout)).collect();
        let cible = dock.icon_rect(base + 2, &layout).expect("le troisieme dossier a une place");
        let (x, y) = (cible[0] + cible[2] * 0.5, cible[1] + cible[3] * 0.5);

        dock.begin_potential_drag(base, x - 300.0, y, HWND::default());
        // ⚠️ **SANS Ctrl** — c'est tout le sujet.
        dock.update_drag(x, y, false, false, &layout);
        // Le geste est bien ENGAGÉ (le seuil est franchi) : c'est ce qui rend le
        // refus significatif, plutôt qu'un simple clic qui n'aurait rien tenté.
        assert!(
            dock.drag.as_ref().is_some_and(|d| d.dragging),
            "le seuil doit etre franchi, sinon ce test ne mesurerait rien"
        );
        assert!(dock.drag_preview(&layout).is_none(), "et RIEN ne doit suivre le curseur");
        // ⛔ **ET AUCUNE PLACE NE DOIT S'OUVRIR.** `drag_preview` ne cache que l'icône
        // FLOTTANTE ; la place, elle, vient d'`icon_rects_map` — donc d'`icon_rect`,
        // qui est ce que le DESSIN lit (voir `presented_rect`). Une garde posée sur
        // le seul aperçu laissait donc les icônes s'écarter pour un lâcher qui
        // n'aurait pas lieu : le geste promettait encore.
        let places_pendant: Vec<Option<[f32; 4]>> =
            (0..4).map(|i| dock.icon_rect(base + i, &layout)).collect();
        assert_eq!(places_pendant, places_avant, "aucune icone ne doit s'ecarter");

        let issue = dock.end_drag(&layout, HWND::default());
        assert!(matches!(issue, DragOutcome::Refused), "obtenu {issue:?}");
        assert_eq!(dock.order(Side::Bottom).to_vec(), avant, "l'ordre ne doit PAS bouger");
    }

    /// ⛔ **LE « + » NE SE GLISSE PAS — ET SURTOUT, IL RESTE CLIQUABLE.**
    ///
    /// C'est un BOUTON, pas un emplacement de la rangée : le glisser le sortirait de la
    /// plage où `plus_dossier_index` le calcule (`base + count`).
    ///
    /// ⛔ **LE REFUS VIT DANS `update_drag`, ET C'EST TOUT LE SUJET DE CE TEST.** Le
    /// défaut a été livré une fois en le mettant dans `begin_potential_drag` : le
    /// glisser n'était pas empêché, le CLIC était supprimé — `self.drag` n'étant jamais
    /// posé, `end_drag` rendait `DragOutcome::None` au lieu de `Clicked(index)`, et le
    /// « + » n'ouvrait plus rien du tout, sans journal ni erreur. Le test d'alors
    /// affirmait `dock.drag.is_none()` : **il encodait le défaut au lieu de le
    /// garder.** Celui-ci le tient des deux côtés.
    #[test]
    fn le_bouton_plus_de_la_barre_du_bas_ne_se_glisse_pas_mais_reste_cliquable() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let base = dock.set_dossiers(&crate::config::ElementBarre::dossiers_de_test(2));
        let plus = base + 2;
        assert!(dock.is_plus_dossier(plus), "premisse : l'index du « + »");
        let layout = DockLayout {
            total_width: 3840.0,
            total_height: 2088.0,
            top_thickness: 72.0,
            haut_colonnes: 72.0,
            left_width: 140.0,
            right_width: 140.0,
            bottom_thickness: DEFAULT_BOTTOM_THICKNESS,
        };

        dock.begin_potential_drag(plus, 500.0, 500.0, HWND::default());
        // Un geste ENGAGÉ — assez loin pour franchir `DRAG_THRESHOLD`, sinon ce serait
        // un simple appui et le test ne mesurerait rien.
        dock.update_drag(900.0, 500.0, true, false, &layout);
        assert!(!dock.is_dragging(), "le « + » ne doit JAMAIS devenir un glisser");
        assert!(dock.drag_preview(&layout).is_none(), "et RIEN ne doit suivre le curseur");

        let issue = dock.end_drag(&layout, HWND::default());
        assert!(
            matches!(issue, DragOutcome::Clicked(index) if index == plus),
            "le lacher doit rendre le CLIC sur le « + » — c'est lui qui ouvre le \
             selecteur de dossier — et non `None` : obtenu {issue:?}"
        );
    }

    /// ⭐ **LE DÉFAUT SIGNALÉ : « LES CHEMINS SONT MÉLANGÉS ENTRE LES ICÔNES ».**
    ///
    /// Un glisser dans la barre du bas changeait l'ordre des ÉLÉMENTS — et RIEN
    /// d'autre. Or **le rang est la seule identité d'un dossier**, et il est lu à DEUX
    /// endroits qui doivent rester d'accord :
    ///
    /// - la **liste** (`elements`), par `dossier_au_rang(elements, rang)` — c'est ce
    ///   qu'un clic OUVRE ;
    /// - les **vignettes**, qui sont indexées par le RANG — `app::run` peint
    ///   `vignettes[rang]` dans `emplacement(Plage::Dossiers, rang)` (voir le bloc
    ///   `dossier_render_instances`), et c'est ce que la rangée MONTRE.
    ///
    /// Permuter la première sans la seconde fait donc porter à chaque icône le chemin
    /// d'un autre : le clic ouvre le mauvais dossier, **sans plantage et sans
    /// journal**, jusqu'au redémarrage suivant (qui, lui, rebâtit les deux depuis la
    /// même liste — c'est pourquoi le défaut disparaissait tout seul).
    ///
    /// ⚠️ **LES VIGNETTES SONT REPRÉSENTÉES PAR UN TABLEAU DE CHEMINS** : c'est ce
    /// qu'elles portent, à l'index près, et cela évite de fabriquer des textures (donc
    /// un device GPU) pour mesurer une **indexation**. La permutation appliquée est
    /// celle du dépôt, `config::reordonne_par` — le même appel que
    /// `app::EmplacementsDossiers::permuter`.
    ///
    /// ⚠️ **ET LE TEST MONTRE QUE L'ANCIEN COMPORTEMENT VIOLE L'INVARIANT** : un test
    /// qu'on n'a pas vu échouer sur le code d'avant ne garde rien.
    #[test]
    fn un_reordonnancement_ne_melange_pas_les_chemins_des_dossiers() {
        use crate::app::{dossier_au_rang, nombre_de_dossiers};
        use crate::config::{elements_selon_l_ordre_du_dock, permutation_des_dossiers, reordonne_par, ElementBarre};

        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let elements = ElementBarre::dossiers_de_test(3);
        let base = dock.set_dossiers(&elements);
        let count = nombre_de_dossiers(&elements);
        let layout = DockLayout {
            total_width: 3840.0,
            total_height: 2088.0,
            top_thickness: 72.0,
            haut_colonnes: 72.0,
            left_width: 140.0,
            right_width: 140.0,
            bottom_thickness: DEFAULT_BOTTOM_THICKNESS,
        };
        // L'ÉTAT DE DÉPART — `vignettes[rang]` est le chemin du dossier de rang `rang`,
        // exactement comme `EmplacementsDossiers` le porte au lancement.
        let mut vignettes: Vec<String> = elements
            .iter()
            .filter_map(|e| match e {
                ElementBarre::Dossier(d) => Some(d.chemin.display().to_string()),
                ElementBarre::Separateur(_) => None,
            })
            .collect();
        /// **CE QUE LA RANGÉE MONTRE À CHAQUE PLACE DE DOSSIER, ET CE QU'UN CLIC Y OUVRE.**
        let rangee = |dock: &Dock, vignettes: &[String], elements: &[ElementBarre]| -> Vec<(String, String)> {
            dock.order(Side::Bottom)
                .iter()
                .filter_map(|&slot| dock.dossier_slot(slot))
                .map(|rang| {
                    let montre = vignettes.get(rang).cloned().unwrap_or_default();
                    let ouvre = dossier_au_rang(elements, rang)
                        .map(|d| d.chemin.display().to_string())
                        .unwrap_or_default();
                    (montre, ouvre)
                })
                .collect()
        };
        for (montre, ouvre) in rangee(&dock, &vignettes, &elements) {
            assert_eq!(montre, ouvre, "premisse : au depart, chaque place doit deja s'accorder");
        }

        // LE GESTE — le premier dossier, lâché sur la case du troisième (assez loin
        // pour franchir `DRAG_THRESHOLD`), avec Ctrl.
        let cible = dock.icon_rect(base + 2, &layout).expect("le troisieme dossier a une place");
        let (x, y) = (cible[0] + cible[2] * 0.5, cible[1] + cible[3] * 0.5);
        dock.begin_potential_drag(base, x - 300.0, y, HWND::default());
        dock.update_drag(x, y, true, false, &layout);
        let issue = dock.end_drag(&layout, HWND::default());
        assert!(matches!(issue, DragOutcome::Moved), "obtenu {issue:?}");

        // ⭐ **CE QUE L'UTILISATEUR VIENT DE VOIR** — la rangée telle que le geste
        // l'a composée, AVANT que quoi que ce soit ne soit permuté. C'est la
        // référence : le correctif ne doit pas la changer, seulement la rendre vraie
        // pour de bon. ⚠️ Elle se capture ICI, avec les vignettes encore dans
        // l'ancien ordre — c'est ce qui la distingue de la vérification finale.
        let vu_pendant_le_geste: Vec<String> = dock
            .order(Side::Bottom)
            .iter()
            .filter_map(|&slot| dock.dossier_slot(slot))
            .map(|rang| vignettes[rang].clone())
            .collect();

        // CE QUE `app::run` EN FAIT (voir son bras `DragOutcome::Moved`) : la rangée
        // se traduit en éléments, puis en permutation des rangs.
        let nouvel = elements_selon_l_ordre_du_dock(&elements, base, count, dock.order(Side::Bottom))
            .expect("la rangee doit se traduire");
        assert_ne!(nouvel, elements, "premisse : le geste doit avoir change l'ordre");
        let permutation = permutation_des_dossiers(&elements, &nouvel).expect("une permutation des dossiers");

        // ❌ **L'ANCIEN COMPORTEMENT** — la liste permutée, les vignettes NON.
        let melange = rangee(&dock, &vignettes, &nouvel);
        assert!(
            melange.iter().any(|(montre, ouvre)| montre != ouvre),
            "l'ancien comportement DOIT melanger les chemins — sinon ce test ne prouve rien : {melange:?}"
        );

        // ✅ **LE CORRECTIF — LES DEUX MOITIÉS, ET CHACUNE EST NÉCESSAIRE.**
        //
        // 1. Les vignettes suivent la permutation → le RANG ne change plus de sens ;
        // 2. le dock recompose sa rangée depuis la nouvelle liste → la rangée reste
        //    celle que le geste a montrée (c'est `set_dossiers` qui POSE les rangs, et
        //    lui seul sait où passe le « + »).
        //
        // ⚠️ **SANS LA SECONDE, LA PREMIÈRE NE SE VOIT PAS** — la permutation laisserait
        // la rangée dans l'ordre des ANCIENNES cases, donc elle afficherait un ordre que
        // l'utilisateur n'a jamais demandé. C'est ce que la comparaison ci-dessous
        // interdit, et c'est pour ça qu'elle existe.
        vignettes = reordonne_par(&vignettes, &permutation).expect("une permutation");
        dock.set_dossiers(&nouvel);

        let montre_apres: Vec<String> = dock
            .order(Side::Bottom)
            .iter()
            .filter_map(|&slot| dock.dossier_slot(slot))
            .map(|rang| vignettes[rang].clone())
            .collect();
        assert_eq!(
            montre_apres, vu_pendant_le_geste,
            "la rangee doit continuer de montrer ce que le geste avait montre"
        );
        for (montre, ouvre) in rangee(&dock, &vignettes, &nouvel) {
            assert_eq!(montre, ouvre, "apres le geste, chaque place doit montrer ce qu'elle ouvre");
        }
    }

    /// **LE CONTENU SUIT LA ZONE** — la demande même : « j'ai déplacé la zone Notifications mais son
    /// contenu n'a pas suivi ». Une zone n'est pas un contenant mais une FRONTIÈRE ; le bloc
    /// `[séparateur + son contenu]` part donc d'un seul tenant.
    ///
    /// Sans le bloc, le séparateur descendrait SEUL et les icônes 1 et 2 resteraient en tête,
    /// rattachées à la zone suivante — précisément ce que ce test refuse.
    #[test]
    fn a_moved_zone_carries_its_content() {
        let (mut dock, layout, sep_a, sep_b) = dock_deux_zones();
        let x = Dock::centered_icon_x(Side::Right, &layout, icon_size_for(layout.right_width));

        let (issue, droite) = lacher_element(&mut dock, &layout, sep_a, x, 4000.0);

        assert!(matches!(issue, DragOutcome::Moved), "obtenu {issue:?}");
        assert_eq!(
            droite,
            vec![sep_b, 3, sep_a, 1, 2],
            "la zone est descendue AVEC ses deux icônes, et la zone du dessous n'a pas bougé"
        );
    }

    /// Une zone déplacée n'emporte QUE son propre contenu : le séparateur suivant et ses icônes ne
    /// bougent pas. C'est la borne « jusqu'au prochain séparateur » (voir `bloc_deplace`).
    #[test]
    fn a_moved_zone_leaves_the_next_zone_alone() {
        let (mut dock, layout, sep_a, sep_b) = dock_deux_zones();
        let x = Dock::centered_icon_x(Side::Right, &layout, icon_size_for(layout.right_width));

        let (issue, droite) = lacher_element(&mut dock, &layout, sep_b, x, layout.top_thickness + 1.0);

        assert!(matches!(issue, DragOutcome::Moved), "obtenu {issue:?}");
        assert_eq!(
            droite,
            vec![sep_b, 3, sep_a, 1, 2],
            "il remonte avec SA seule icône, et laisse 1 et 2 à l'autre zone"
        );
    }

    /// **LE BLOC, ET LA POSITION, S'ARRÊTENT DEVANT CE QUI NE SE PERSISTE PAS.** Le miroir de la
    /// zone de notification vit APRÈS tout ce qui s'écrit (voir `set_tray_count`) : le franchir
    /// mettrait des éléments persistés après des icônes qui ne le sont pas, et l'ordre relu au
    /// lancement suivant n'aurait plus la même forme — le déplacement serait perdu **en silence**.
    #[test]
    fn a_moved_zone_stops_before_the_icons_that_are_not_persisted() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let sep = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(sep, 400.0);
        dock.set_icon_order(Vec::new(), vec![sep, 1, 2], Vec::new(), Vec::new());
        let base = dock.set_tray_count(3);
        assert!(dock.order(Side::Right).contains(&base), "prémisse : le miroir est dans la barre");

        let layout = geometry_layout();
        let x = Dock::centered_icon_x(Side::Right, &layout, icon_size_for(layout.right_width));
        // On pousse la zone tout en bas : sans borne, elle passerait SOUS le miroir.
        lacher_element(&mut dock, &layout, sep, x, 4000.0);

        let ordre = dock.order(Side::Right).to_vec();
        let premier = ordre
            .iter()
            .position(|&i| !dock.is_persistable(i))
            .unwrap_or_else(|| panic!("prémisse : le miroir doit être dans l'ordre — {ordre:?}"));
        assert!(
            ordre[premier..].iter().all(|&i| !dock.is_persistable(i)),
            "aucun élément persisté ne doit se retrouver APRÈS un élément d'exécution : {ordre:?}"
        );
    }

    /// **L'ESPACE CRÉÉ DOIT ÊTRE PERSISTABLE MÊME QUAND LE MIROIR EST CRÉÉ EN PREMIER.** C'est le
    /// cas de la machine de l'utilisateur — base du miroir **178**, base des applications ouvertes
    /// **242** — et c'est ce qui faisait qu'une zone neuve ne réagissait à AUCUN réglage : son
    /// espace n'était cité par aucun ordre ÉCRIT, donc sa clé ne se résolvait plus au lancement
    /// suivant, et `completer_les_barres` recréait l'espace manquant au même endroit — non
    /// persisté de nouveau.
    ///
    /// **L'ORDRE DES DEUX CRÉATIONS EST LE SUJET DU TEST** : le miroir d'abord, la plage
    /// dynamique ensuite, exactement comme au démarrage réel. L'ancienne règle (`at =
    /// dynamic_base`) passait ce test dans l'AUTRE ordre, et c'est pourquoi le défaut a survécu.
    #[test]
    fn a_spacer_created_after_the_notification_mirror_is_still_persisted() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let miroir = dock.set_tray_count(2);
        let dynamique = dock.set_dynamic_apps(3);
        assert!(miroir < dynamique, "prémisse : le miroir est la PLUS BASSE des deux plages ({miroir} < {dynamique})");

        let espace = dock.add_spacer_at_end(Side::Right);

        assert!(dock.is_persistable(espace), "l'espace créé ({espace}) doit être persistable — la frontière est {miroir}");
        let (_, droite, _, _) = dock.icon_order();
        assert!(droite.contains(&espace), "et figurer dans l'ordre ÉCRIT, seul relu au lancement : {droite:?}");
        // LES DEUX BASES SUIVENT LE DÉCALAGE — sinon `app::run` les relit un cran trop bas et
        // route les clics sur la mauvaise icône (voir `insert_spacer`, doc).
        assert_eq!(dock.tray_icon_base(), Some(miroir + 1), "la base du miroir suit");
        assert_eq!(dock.dynamic_icon_base(), Some(dynamique + 1), "celle des applications ouvertes aussi");
    }

    /// **NON-RÉGRESSION** : une icône ordinaire est son propre bloc — la déplacer ne déplace
    /// qu'elle, et non les icônes de la zone où elle se trouvait.
    #[test]
    fn moving_an_ordinary_icon_moves_only_it() {
        let (mut dock, layout, sep_a, sep_b) = dock_deux_zones();
        // La case de l'icône 3, dans la zone de `sep_b` : c'est là qu'on dépose l'icône 1.
        //
        // ⚠️ **LE QUART SUPÉRIEUR, PAS LE CENTRE** : à UNE colonne la règle d'origine tranche au
        // CENTRE de l'emplacement (voir `drag_insert_index`), donc pile au centre l'icône passe
        // APRÈS — ce qui est son comportement voulu, et non ce que ce test-ci mesure.
        let case = dock.icon_rect(3, &layout).expect("l'icône 3 est dans la barre");
        let (x, y) = (case[0] + case[2] * 0.5, case[1] + case[3] * 0.25);

        let (issue, droite) = lacher_element(&mut dock, &layout, 1, x, y);

        assert!(matches!(issue, DragOutcome::Moved), "obtenu {issue:?}");
        assert_eq!(droite, vec![sep_a, 2, sep_b, 1, 3], "SEULE l'icône 1 a bougé");
    }

    /// **SUPPRIMER UNE ZONE** — l'espace DISPARAÎT (décision explicite : le séparateur est retiré,
    /// et non ramené à une cote nulle, qui laisserait un blanc), ses icônes rejoignent la zone
    /// précédente, et **le RANG des zones suivantes se décale** : c'est ce que `zones.toml` doit
    /// réécrire, sans quoi il désignerait encore l'ancien.
    #[test]
    fn removing_a_zone_takes_its_separator_away_and_shifts_the_ranks() {
        let (mut dock, _, sep_a, sep_b) = dock_deux_zones();
        let cle_avant = dock.zone_key_of(sep_b).expect("prémisse : la seconde zone a une clé");
        assert_eq!(cle_avant, crate::ui::zones::ZoneKey::new(crate::ui::zones::BarLetter::Droite, 2).unwrap());

        dock.remove_spacer(sep_a);

        assert_eq!(
            dock.order(Side::Right).to_vec(),
            vec![1, 2, sep_b, 3],
            "le séparateur part, les icônes RESTENT et rejoignent la zone précédente"
        );
        let cle_apres = dock.zone_key_of(sep_b).expect("la zone suivante existe toujours");
        assert_ne!(cle_apres, cle_avant, "son RANG a changé — la persistance doit le suivre");
        assert_eq!(cle_apres, crate::ui::zones::ZoneKey::new(crate::ui::zones::BarLetter::Droite, 1).unwrap());
        // Le séparateur retiré n'est cité par aucune liste : sa cote survit dans `icons`, mais
        // `zones_a_ecrire` ne l'exporte pas (voir `a_separator_that_no_bar_cites_is_not_exported`).
        assert_eq!(dock.zone_key_of(sep_a), None, "un orphelin n'a pas de clé, donc pas d'export");
    }

    /// **AJOUTER UNE ZONE APRÈS UNE AUTRE** — elle naît APRÈS LE CONTENU, jamais collée au
    /// séparateur : sinon la zone visée se viderait dans la nouvelle, ce qui est exactement le
    /// défaut que ce chantier répare (voir `add_zone_after_zone`).
    #[test]
    fn adding_a_zone_after_a_zone_lands_after_its_content() {
        let (mut dock, _, sep_a, sep_b) = dock_deux_zones();

        assert!(dock.add_zone_after_zone(sep_a, DEFAULT_ZONE_HEIGHT).is_some(), "la zone A ouvre bien une zone");

        let ordre = dock.order(Side::Right).to_vec();
        assert_eq!(ordre.len(), 6, "un espace de plus : {ordre:?}");
        let nouvelle = ordre[3];
        assert!(dock.is_zone_separator(nouvelle), "et c'est bien un séparateur de zone");
        assert_eq!(
            ordre,
            vec![sep_a, 1, 2, nouvelle, sep_b, 3],
            "elle se place APRÈS le contenu (1, 2), et non juste après le séparateur"
        );
    }

    /// **LE VERROU D'UNE ZONE REFUSE LE LÂCHER** — et il le dit désormais avec le
    /// MÊME variant que le vide (`Refused`), là où il rendait `Moved` : un nom qui
    /// affirmait que l'ordre avait bougé alors qu'il n'avait pas bougé.
    #[test]
    fn a_drop_into_a_locked_zone_is_refused() {
        let (mut dock, layout, sep) = dock_pour_lacher(2, 4);
        dock.set_zone_locked(sep, true);
        let avant = dock.order(Side::Right).to_vec();
        let case = dock.icon_rect(2, &layout).expect("l'icône 2 est dans la barre");
        let (x, y) = (case[0] + case[2] * 0.5, case[1] + case[3] * 0.5);

        let (issue, droite) = lacher_sur(&mut dock, &layout, x, y);

        assert!(matches!(issue, DragOutcome::Refused), "zone figée : le lâcher n'a pas lieu, obtenu {issue:?}");
        assert_eq!(droite, avant, "et l'ordre est intact");
    }

    /// **LE DÉFILEMENT COMPTE.** Une zone défilée dessine ses rangées plus haut ;
    /// le lâcher doit viser ce qui est DESSINÉ, pas la position au repos. C'est
    /// pour cela que `start_y` (défilement de barre) et `zone_scroll_offset`
    /// (défilement de zone) ont été câblés ici — cette fonction les mettait
    /// auparavant à zéro.
    ///
    /// Discriminant : sans le défilement, le même point tomberait sur l'icône 2 au
    /// lieu de l'icône 3 (les deux décalages sont écrits dans le test).
    #[test]
    fn a_drop_in_a_scrolled_zone_targets_the_row_that_is_drawn() {
        let (mut dock, layout, sep) = dock_pour_lacher(1, 4);
        dock.set_zone_scroll_offset(sep, 120.0);
        // La rangée DESSINÉE de l'icône 3 : c'est elle qu'on vise, et non sa
        // position au repos (voir `set_zone_scroll_offset`).
        let case = dock.icon_rect(3, &layout).expect("l'icône 3 est dans la barre");
        let (x, y) = (case[0] + case[2] * 0.5, case[1] + case[3] * 0.25);

        let (issue, droite) = lacher_sur(&mut dock, &layout, x, y);

        assert!(matches!(issue, DragOutcome::Moved), "obtenu {issue:?}");
        assert_eq!(droite, vec![sep, 1, 2, 9, 3, 4], "l'icône prend la case DESSINÉE de l'icône 3");
    }

    /// **UN LÂCHER REFUSÉ N'OUVRE AUCUNE PLACE**, et c'est le SEUL signe du refus
    /// pendant le geste (voir `icon_rects_map`) : les autres icônes ne s'écartent
    /// pas, là où elles le font dès que le lâcher est accepté.
    #[test]
    fn a_refused_drag_opens_no_place() {
        let (mut dock, layout, _) = dock_pour_lacher(2, 4);
        let avant: Vec<Option<[f32; 4]>> = (0..dock.icons.len()).map(|i| dock.icon_rect(i, &layout)).collect();

        // Le curseur dans le VIDE, sous la dernière zone : ce lâcher sera refusé.
        let (_, bar) = right_bar(&dock, &layout);
        let bas = bar.zones.last().expect("prémisse : le séparateur ouvre une zone").bottom;
        let x = Dock::centered_icon_x(Side::Right, &layout, icon_size_for(layout.right_width));
        dock.begin_potential_drag(9, 40.0, 300.0, HWND::default());
        dock.update_drag(x, bas + 40.0, true, false, &layout);
        assert!(dock.is_dragging(), "prémisse : le geste est bien un GLISSER, pas un clic");

        let pendant: Vec<Option<[f32; 4]>> = (0..dock.icons.len()).map(|i| dock.icon_rect(i, &layout)).collect();

        // ⚠️ **L'ICÔNE GLISSÉE est mise à `None` PARTOUT pendant le geste** — elle
        // flotte sous le curseur et ne doit pas être dessinée deux fois, même dans
        // sa barre d'origine (voir `icon_rects_map`). On compare donc tout le
        // RESTE, et on constate à part ce retrait, qui est le contrat de l'icône
        // flottante.
        let sans_la_glissee = |rects: &[Option<[f32; 4]>]| -> Vec<Option<[f32; 4]>> {
            rects.iter().enumerate().filter(|&(i, _)| i != 9).map(|(_, r)| *r).collect()
        };
        assert_eq!(
            sans_la_glissee(&pendant),
            sans_la_glissee(&avant),
            "aucune place ouverte : les autres icônes ne bougent pas pendant un refus"
        );
        assert_eq!(pendant[9], None, "et l'icône glissée est retirée de sa place — elle flotte sous le curseur");
        assert!(matches!(dock.end_drag(&layout, HWND::default()), DragOutcome::Refused));
    }

    /// **NON-RÉGRESSION, par l'autre bout** : à une colonne — le défaut, donc
    /// l'état de TOUTE zone existante — le contenu s'empile exactement comme
    /// avant les grilles, y compris la cote qui impose le bas de la zone et le
    /// défilement propre qui déplace son contenu.
    #[test]
    fn a_single_column_zone_stacks_exactly_as_before_grids() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let sep = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(sep, 400.0);
        assert_eq!(dock.zone_columns(sep), Some(DEFAULT_ZONE_COLUMNS), "prémisse : une zone neuve n'a qu'une colonne");
        dock.set_icon_order(Vec::new(), vec![sep, 1, 2, 3], Vec::new(), Vec::new());

        let (order, bar) = right_bar(&dock, &layout);
        let icon_size = icon_size_for(layout.right_width);
        let zone = bar.zones.first().expect("prémisse : le séparateur déclare une zone");

        // L'arithmétique d'avant, écrite ici à la main : un emplacement par
        // icône, chacun sous le précédent, à la taille d'icône de la barre.
        let mut y = zone.top;
        for &icon in [1usize, 2, 3].iter() {
            let slot = slot_of(&order, &bar, icon);
            assert_eq!(slot.top, y, "l'icône {icon} s'empile sous la précédente");
            assert_eq!(slot.height, icon_size, "à sa taille d'icône pleine, pas rétrécie");
            assert_eq!(slot.column, 0, "et dans l'unique colonne");
            y += icon_size + dock.icon_gap();
        }
        assert_eq!(y - dock.icon_gap(), zone.top + 3.0 * icon_size + 2.0 * dock.icon_gap(), "trois rangées d'une icône");
    }

    // ---- La survie d'un espace ENREGISTRÉ d'une session à l'autre --------

    /// **LE TEST QUI FIGE LA PROPRIÉTÉ** : un espace créé pendant une session
    /// doit retrouver son emplacement à la session suivante.
    ///
    /// La session 2 rejoue la reconstruction d'`app::run` **dans son ordre**,
    /// parce que c'est l'ordre qui fait la propriété et pas les appels pris
    /// isolément : base des épingles → épingles → l'étape qui recrée les
    /// espaces enregistrés → ordre → tailles et cotes.
    ///
    /// Les nombres sont ceux de la configuration de cette machine, mesurés :
    /// les épingles commencent à 92 et sont 23, donc `icons` fait 115.
    #[test]
    fn a_spacer_created_after_the_taskbar_pins_survives_a_restart() {
        const WINPIN_BASE: usize = 92;
        const WINPIN_COUNT: usize = 23;

        let (orders, sizes, zones, sep);
        {
            // --- Session 1, jusqu'à la création d'un séparateur de zone.
            let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
            dock.ensure_spacer_capacity(WINPIN_BASE);
            dock.ensure_winpin_capacity(WINPIN_COUNT);
            assert_eq!(dock.icons.len(), WINPIN_BASE + WINPIN_COUNT, "prémisse : la barre fait la taille mesurée");

            // Le geste (Maj + clic droit, ou Ctrl + clic droit pour un espace
            // ordinaire) : `add_spacer_after`/`add_spacer_at_end` ANNEXENT à la
            // fin de `icons`, donc AU-DESSUS des épingles.
            dock.add_spacer_at_end(Side::Right);
            sep = dock.icons.len() - 1;
            dock.set_zone_height(sep, 400.0);
            assert!(
                sep >= WINPIN_BASE + WINPIN_COUNT,
                "prémisse : l'espace créé se pose au-dessus des épingles (index {sep})"
            );
            // Ce que les DEUX gestes écrivent : l'ordre (le contrôle de
            // l'endroit) et la cote. C'est ce qui serait relu au redémarrage.
            orders = dock.icon_order();
            sizes = dock.spacer_sizes();
            zones = dock.zone_heights();
            assert!(zones.iter().any(|&(index, _, _)| index == sep), "prémisse : sa cote est bien enregistrée");
        }

        // --- Session 2 : la reconstruction, dans l'ordre d'`app::run`.
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.ensure_spacer_capacity(WINPIN_BASE);
        dock.ensure_winpin_capacity(WINPIN_COUNT);
        // Les espaces enregistrés doivent retrouver un emplacement AVANT que
        // l'ordre et les cotes ne soient appliqués — sans quoi tout ce qui les
        // vise est écarté en silence (voir `set_icon_order`). C'est l'appel
        // EXACT d'`app::run`, à la même place.
        let (left, right, top_left, top_right) = orders;
        dock.ensure_saved_spacers_exist(
            [&left, &right, &top_left, &top_right],
            floor(&zones),
            WINPIN_BASE + WINPIN_COUNT,
        );
        dock.set_icon_order(left, right, top_left, top_right);
        dock.set_spacer_sizes(&sizes);
        dock.set_zone_heights(&zones);

        assert!(
            dock.is_zone_separator(sep),
            "le séparateur créé la session précédente (index {sep}) doit retrouver son emplacement"
        );
        assert_eq!(dock.zone_height(sep), Some(400.0), "et sa cote avec lui");
    }

    /// **LE GESTE DE CRÉATION POSE L'ESPACE SOUS LA PLAGE DYNAMIQUE** — c'est ce
    /// qui le rend persistable, indestructible par `set_dynamic_apps`, et
    /// recrÉABLE au redémarrage. Les quatre assertions ci-dessous sont les quatre
    /// maillons de cette chaîne, et c'est le défaut mesuré AVANT `insert_spacer`
    /// (base dynamique 115, espace créé à l'index 116, « exporté : false »).
    #[test]
    fn a_created_spacer_stays_below_the_dynamic_apps() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.ensure_spacer_capacity(92);
        dock.ensure_winpin_capacity(23);

        // Une application non épinglée est ouverte — ce qu'`app::run` détecte dès
        // la première seconde sur un bureau normal (voir `set_dynamic_apps`).
        dock.set_dynamic_apps(1);
        let dynamic_base = dock.dynamic_icon_base().expect("prémisse : une application ouverte occupe la plage dynamique");

        // Le geste de création (Ctrl + clic droit pour un espace, Maj + clic
        // droit pour un séparateur de zone — même entonnoir).
        let created = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(created, 400.0);

        // 1. L'espace prend la place juste SOUS la plage dynamique, qui monte
        //    d'un cran — les icônes d'applications suivent avec leurs drapeaux.
        assert_eq!(created, dynamic_base, "l'espace créé se pose SOUS la plage dynamique");
        assert_eq!(dock.dynamic_icon_base(), Some(dynamic_base + 1), "et la base monte d'un cran");
        assert!(!dock.is_spacer(dynamic_base + 1), "l'icône d'application est toujours là, un cran plus haut");

        // 2. Son index est donc EXPORTÉ par `icon_order` — `is_persistable`
        //    (`i < dynamic_base`) ne l'écarte plus, et il sera persisté.
        let (left, right, top_left, top_right) = dock.icon_order();
        let listed = [&left, &right, &top_left, &top_right].iter().any(|order| order.contains(&created));
        assert!(listed, "sans quoi l'espace n'est JAMAIS écrit, et rien ne peut le faire revenir");

        // 3. Il survit à la fermeture de l'application : `set_dynamic_apps`
        //    tronque à `base + count`, et il est sous la base.
        dock.set_dynamic_apps(0);
        assert!(created < dock.icons.len(), "l'espace survit à la disparition de l'application");
        assert!(dock.is_zone_separator(created), "avec sa cote");

        // 4. Et il retrouve son emplacement au redémarrage — reconstruction
        //    d'`app::run`, dans son ordre.
        let zones = dock.zone_heights();
        let mut fresh = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        fresh.ensure_spacer_capacity(92);
        fresh.ensure_winpin_capacity(23);
        fresh.ensure_saved_spacers_exist([&left, &right, &top_left, &top_right], floor(&zones), 92 + 23);
        fresh.set_icon_order(left, right, top_left, top_right);
        fresh.set_zone_heights(&zones);
        assert!(fresh.is_zone_separator(created), "le séparateur doit retrouver son emplacement");
        assert_eq!(fresh.zone_height(created), Some(400.0), "et sa cote");
    }

    /// Un espace créé dans la barre du HAUT doit être DESSINÉ comme les autres.
    ///
    /// Ctrl + clic droit sur une icône de la barre du haut crée l'espace par
    /// `add_spacer_after` (`side_at` rend `None` dans toute la bande du haut,
    /// donc le geste dans le VIDE n'y crée rien) : cet espace a un emplacement
    /// (`layout_top_side` en donne un de largeur `icon_size`), et rien ne
    /// justifie qu'il soit le seul à ne pas recevoir son marqueur.
    #[test]
    fn a_top_bar_spacer_is_drawn_like_any_other() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let anchor = dock.order(Side::TopRight)[0];
        let spacer = dock.add_spacer_after(anchor).expect("l'icône est dans une barre");

        assert_eq!(dock.icon_side(spacer), Some(Side::TopRight), "prémisse : l'espace est dans la barre du haut");
        let rect = dock.icon_rect(spacer, &layout).expect("il a un emplacement");
        assert!(rect[2] > 0.0 && rect[3] > 0.0, "et il n'est pas dégénéré : {rect:?}");

        let drawn: Vec<usize> = dock.render_instances(&layout, [1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0], 10.0).into_iter().map(|(i, _, _)| i).collect();
        assert!(drawn.contains(&spacer), "l'espace de la barre du haut doit recevoir son marqueur (dessinés : {drawn:?})");
    }

    /// **INSÉRER DÉCALE TOUS LES INDEX SUPÉRIEURS**, donc les quatre listes
    /// d'ordre doivent suivre — sans quoi elles désignent l'icône VOISINE.
    ///
    /// Défaut réel constaté sur la configuration de la machine, après le
    /// premier correctif : `gauche=[…, 114, 126, 126]`. L'icône de l'application
    /// ouverte restait citée à 126 alors qu'elle était passée à 127, l'espace
    /// créé s'y ajoutait, et l'index apparaissait DEUX fois — deux emplacements
    /// pour un seul élément, un trou à la place de l'autre, et l'icône de
    /// l'application plus citée par aucune liste, donc disparue de la barre.
    #[test]
    fn creating_a_spacer_shifts_the_orders_with_the_icons() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.ensure_spacer_capacity(92);
        dock.ensure_winpin_capacity(23);
        dock.set_dynamic_apps(1);
        let dynamic_icon = dock.dynamic_icon_base().expect("une application ouverte occupe la plage dynamique");
        assert!(dock.order(Side::Left).contains(&dynamic_icon), "prémisse : son icône est dans la barre de gauche");

        let created = dock.add_spacer_at_end(Side::Left);

        let left = dock.order(Side::Left).to_vec();
        assert_eq!(left.iter().filter(|&&i| i == created).count(), 1, "l'espace n'est cité qu'UNE fois : {left:?}");
        assert!(left.contains(&(dynamic_icon + 1)), "l'icône décalée d'un cran est toujours dans la barre : {left:?}");
        assert!(!dock.is_spacer(dynamic_icon + 1), "et c'est bien elle, pas l'espace");
        assert_eq!(dock.dynamic_icon_base(), Some(dynamic_icon + 1), "la base suit le décalage");
    }

    /// **LES DEUX BASES SUIVENT TOUTE INSERTION — C'EST LE CONTRAT SUR LEQUEL `app::run`
    /// S'APPUIE DEPUIS QU'IL LES RELIT À CHAQUE FRAME.**
    ///
    /// Défaut réel du 2026-09-22 : le panneau Réglages créait un séparateur, le dock ajustait
    /// bien ses deux bases (`insert_spacer` les décale, voir sa doc), mais `app::run` en gardait
    /// des **COPIES** — et le panneau ne pouvait pas les corriger, ce sont des locales d'ici. Le
    /// miroir de notification se croyait donc encore à son ancienne base et dessinait sa PREMIÈRE
    /// icône (« Avertissement », un glyphe du shell) dans la **case du séparateur neuf** : 18 px de
    /// haut au lieu d'une case d'icône, d'où la forme plate, grise et creuse signalée sur la zone
    /// qu'on venait de créer — et qui disparaissait au redémarrage, où les bases sont recalculées.
    ///
    /// `app::run` lit désormais les getters à chaque frame, ce qui rend toute copie impossible.
    /// Ce test verrouille l'AUTRE moitié : que les getters disent bien la vérité après une
    /// insertion. Sans lui, une future voie de décalage qui oublierait d'ajuster une base
    /// passerait inaperçue — et casserait maintenant TOUS ses lecteurs d'un coup.
    #[test]
    fn both_bases_follow_every_insertion() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.ensure_spacer_capacity(92);
        dock.ensure_winpin_capacity(23);
        // Le miroir AVANT les applications ouvertes : c'est l'ordre réel (le journal de la
        // machine montre la base du miroir à 178 et celle des applications bien plus haut),
        // et c'est lui qui fait de la base du miroir la PLUS PETITE des deux — donc celle
        // que `insert_spacer` prend pour point d'insertion.
        let tray = dock.set_tray_count(3);
        dock.set_dynamic_apps(2);
        let dynamic_avant = dock.dynamic_icon_base().expect("deux applications ouvertes ont une base");
        assert!(tray < dynamic_avant, "prémisse : le miroir est SOUS les applications ouvertes");

        let created = dock.add_zone_separator_at_end(Side::Right, 400.0);

        assert_eq!(created, tray, "prémisse : l'espace neuf prend l'index de la plus petite base");
        assert_eq!(dock.tray_icon_base(), Some(tray + 1), "la base du MIROIR suit le décalage — c'est elle que son dessin lit");
        assert_eq!(dock.dynamic_icon_base(), Some(dynamic_avant + 1), "et celle des applications ouvertes aussi");
    }

    /// **LE GESTE « AJOUTER UNE ZONE APRÈS CELLE-CI » DOIT POSER LE SÉPARATEUR
    /// APRÈS LA ZONE — PAS EN FIN DE BARRE.**
    ///
    /// `add_spacer_after` cherche la POSITION de `after_index` **après** avoir
    /// appelé `insert_spacer`, lequel vient de décaler de +1 tout index ≥ `at`
    /// (voir le test ci-dessus). L'index visé n'existe donc plus sous ce numéro,
    /// `position` rend `None`, et le `map_or(order.len(), ..)` du chemin
    /// d'échec pose le nouvel espace **en fin de barre**. Le commentaire
    /// d'`insert_spacer` dit pourtant noir sur blanc que « l'ordre des deux
    /// opérations est important » — c'est exactement ce que ce test verrouille.
    ///
    /// **Ce n'est pas un cas tordu** : `add_zone_after_zone`, le bouton
    /// « ＋ Ajouter une zone après celle-ci » des Réglages, lui passe `dernier`,
    /// le DERNIER élément de la zone — un index de la barre, donc presque
    /// toujours ≥ `at`.
    #[test]
    fn a_new_spacer_lands_right_after_the_element_it_follows() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.ensure_spacer_capacity(92);
        dock.ensure_winpin_capacity(23);
        dock.set_dynamic_apps(1);
        let side = Side::Left;
        // ⚠️ **UN ÉLÉMENT APRÈS LA CIBLE, ET IL EST INDISPENSABLE.** Sans lui,
        // « juste après la cible » et « en fin de barre » désignent la MÊME
        // position : le test passerait avec le défaut en place, et ne mesurerait
        // donc rien (même piège que le lâcher dont le départ est l'arrivée).
        dock.add_spacer_at_end(side);

        let before = dock.order(side).to_vec();
        // L'AVANT-DERNIER élément : c'est la seule position où « juste après »
        // se distingue de « à la fin ». `at` (la base non persistable) n'est pas
        // exposé — le test ne le DEVINE donc pas, il le DÉTECTE : la cible a
        // glissé si et seulement si son index a disparu de l'ordre.
        let target = before[before.len() - 2];
        let trailing = before[before.len() - 1];

        let created = dock.add_spacer_after(target).expect("la cible est dans une barre");

        let after = dock.order(side).to_vec();
        assert_eq!(after.iter().filter(|&&i| i == created).count(), 1, "l'espace n'est cité qu'UNE fois : {after:?}");
        // ⚠️ **L'INVARIANT SE MESURE PAR LA POSITION, PAS PAR L'IDENTITÉ DE
        // L'INDEX** : `insert_spacer` donne au nouvel espace l'index `at`, qui
        // est précisément celui que la cible vient de libérer en glissant d'un
        // cran. Chercher « l'index de la cible » après coup retombe donc sur le
        // nouvel espace lui-même — et le test se tromperait d'un cran en
        // croyant mesurer juste. Ce qui est vrai, et qui se vérifie : l'espace
        // neuf se pose JUSTE AVANT l'élément qui suivait la cible.
        let trailing_now = if after.contains(&trailing) { trailing } else { trailing + 1 };
        assert_eq!(
            (after[after.len() - 2], after[after.len() - 1]),
            (created, trailing_now),
            "l'espace doit se poser JUSTE APRÈS la cible, donc JUSTE AVANT la queue {trailing} (avant : {before:?}, après : {after:?})"
        );
    }

    /// **LE BLOC DES ÉPINGLES SUIT LE NOMBRE D'ÉPINGLES — ET RIEN AU-DESSUS.**
    ///
    /// C'est l'invariant qui rend une épingle ajoutée ou retirée inoffensive pour
    /// le reste de la barre : les remplisseurs absorbent la croissance et
    /// `winpin_base` est figé (voir `decaler_le_bloc_des_epingles`, qui porte la
    /// démonstration). Ce test verrouille les DEUX moitiés, et la seconde compte
    /// autant que la première : un décalage trop LARGE déplacerait les zones de
    /// l'utilisateur, un décalage ABSENT ferait désigner l'icône voisine.
    #[test]
    fn the_pin_block_shifts_and_nothing_above_it() {
        let limite = 92;
        let barre = |dock: &Dock| dock.order(Side::Left).to_vec();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        // ⚠️ **LES DEUX CAPACITÉS D'ABORD, ET PAS PAR CONFORT** : `set_icon_order`
        // écarte tout index au-delà de `icons.len()`, donc les index de la limite
        // et de la barre des tâches n'existeraient pas encore — et le test
        // comparerait des listes amputées sans le voir.
        dock.ensure_spacer_capacity(92);
        // DEUX épingles de barre des tâches SEULEMENT, et toutes deux citées par
        // l'ordre : `set_icon_order` RÉINTÈGRE toute icône non citée (voir sa doc,
        // « BUG RÉEL SIGNALÉ »), donc une capacité plus large ferait apparaître
        // vingt épingles non citées dans la liste comparée.
        dock.ensure_winpin_capacity(2);
        // L'ordre cite TOUTES les icônes de la zone des épingles (les index 3..9,
        // applets comprises), puis DEUX INDEX AU-DESSUS DE LA LIMITE. Les
        // remplisseurs (10..91) en sont absents, comme dans la vraie configuration :
        // ce sont des espaces, et eux ne sont jamais réintégrés.
        // ⚠️ **LA FIXTURE DOIT CITER TOUTE ICÔNE NON-ESPACE** : `set_icon_order`
        // RÉINTÈGRE les non-citées (voir sa doc, « BUG RÉEL SIGNALÉ »), et une icône
        // ajoutée au dock — comme l'éclairage nocturne — se retrouverait alors en fin
        // de liste et ferait échouer la comparaison pour une raison qui n'a rien à voir
        // avec le décalage mesuré.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 29** (les caractères accentués, dernière applet
        // ajoutée) — même raison, encore.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 28** (verrouiller la session, dernière applet
        // ajoutée) — même raison, encore.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 27** (la météo, dernière applet ajoutée)
        // — même raison, encore.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 26** (les automatisations, dernière applet
        // ajoutée) — même raison, encore.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 25** (la recherche, dernière applet
        // ajoutée) — même raison, encore.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 24** (les notifications, dernière applet
        // ajoutée) — même raison, encore.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 23** (les fonctionnalités facultatives,
        // dernière applet ajoutée) — même raison, encore.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 22** (le sélecteur de couleurs, dernière
        // applet ajoutée) — même raison, encore.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 21** (les serveurs SSH, dernière applet
        // ajoutée) — même raison, encore.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 20** (la calculatrice, dernière applet
        // ajoutée) — même raison, encore.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 19** (les réglages de l'écran, dernière
        // applet ajoutée) — même raison, encore.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 18** (le gestionnaire de périphériques,
        // dernière applet ajoutée) — même raison, encore.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 17** (Windows Update, dernière applet
        // ajoutée) — même raison, encore.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 16** (la surveillance, dernière applet
        // ajoutée) — même raison, encore.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 15** (le minuteur, dernière applet ajoutée)
        // — même raison, une fois de plus.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 14** (la capture d'écran, dernière applet
        // ajoutée) — même raison que ce qui suit, arrivée une fois de plus.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 12, PAS JUSQU'À 11** : le calendrier a été
        // ajouté au dock (voir `Dock::new`), et le commentaire ci-dessus dit
        // exactement ce qui arrive si on l'oublie ici — il se fait réintégrer en
        // fin de liste et la comparaison échoue pour une raison qui n'a rien à
        // voir avec le décalage mesuré. C'est ce qui est arrivé, et c'est le test
        // qui a parlé.
        // ⚠️ **LA FIXTURE CITE JUSQU'À 32** — les propriétés système, dernière
        // applet ajoutée (2026-10-08), après l'aide (31). ⚠️ **LA PILE
        // D'AVERTISSEMENTS CI-DESSOUS S'ARRÊTE À 29 ET C'EST NORMAL** : chaque
        // applet en ajoutait un, mais le compte ne suit plus le dernier index.
        // **C'est la LISTE qui fait foi, pas la pile de commentaires** — un compte
        // tenu à la main dérive, c'est écrit plus haut dans ce fichier.
        // ⚠️ **JUSQU'À 33** — l'applet Scanner (2026-10-09), même raison encore.
        // ⚠️ **JUSQU'À 34** — l'éditeur de shaders (2026-10-10), même raison.
        // ⚠️ **JUSQU'À 35** — les symboles phonétiques (2026-10-11), même raison.
        // ⚠️ **JUSQU'À 36** — les symboles mathématiques (2026-10-12), même raison.
        // ⚠️ **JUSQU'À 37** — l'éditeur des lueurs (2026-10-14), même raison.
        // ⚠️ **JUSQU'À 38** — l'éditeur des textures (2026-10-15), même raison.
        // ⚠️ **JUSQU'À 39** — le narrateur (2026-10-15), même raison.
        // ⚠️ **JUSQU'À 40** — la loupe (2026-10-19), même raison.
        // ⚠️ **JUSQU'À 41** — le moniteur système (2026-10-20), même raison.
        // ⚠️ **JUSQU'À 43** — l'observateur d'événements (2026-10-30), puis Enregistrer
        // (2026-10-31), même raison.
        // ⚠️ **JUSQU'À 44** — le clavier visuel (2026-11-07), même raison.
        // ⚠️ **JUSQU'À 45** — 2048 (2026-11-07), même raison.
        // ⚠️ **JUSQU'À 46** — Cinq ou plus (2026-11-07), même raison.
        // ⚠️ **JUSQU'À 47** — Quadrapassel (2026-11-07), même raison.
        // ⚠️ **JUSQU'À 48** — Lights Off (2026-11-08), même raison.
        // ⚠️ **JUSQU'À 49** — Reversi (2026-11-10), même raison.
        // ⚠️ **JUSQU'À 50** — Bovo (2026-11-10), même raison.
        // ⚠️ **JUSQU'À 51** — la Patience (2026-11-10), même raison.
        let avant = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 92, 93];
        dock.set_icon_order(avant.clone(), vec![], vec![], vec![]);

        // --- RETRAIT de l'épingle 5 ---
        dock.decaler_le_bloc_des_epingles(5, limite, true);
        assert_eq!(
            barre(&dock),
            vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 92, 93],
            "sa citation part, celles de ]5, 92[ reculent d'un cran, et RIEN au-dessus ne bouge"
        );

        // --- AJOUT à la même place : le décalage s'annule exactement ---
        dock.decaler_le_bloc_des_epingles(5, limite, false);
        assert_eq!(
            barre(&dock),
            vec![0, 1, 2, 3, 4, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 92, 93],
            "l'ajout est l'inverse du retrait — la citation de 5, elle, est reposée par l'APPELANT"
        );
    }

    /// **CACHER UNE ZONE EMPORTE TOUTE SA PLAGE — SON SÉPARATEUR *ET* SON CONTENU —
    /// ET RIEN D'AUTRE.**
    ///
    /// DEMANDE EXPLICITE (2026-09-23) : ces zones « ne puissent pas être supprimées
    /// mais seulement cachées ». Ce test verrouille les DEUX moitiés, et la seconde
    /// compte autant : ce qui disparaît, et ce qui SURVIT — la zone suivante, dont le
    /// séparateur doit garder son emplacement.
    ///
    /// **Le « sans trou » est garanti par la même mécanique** : le saut ne touche pas au
    /// curseur `y`, donc la zone suivante reprend exactement là où la précédente s'est
    /// arrêtée (voir `bar_layout`, qui porte le commentaire).
    #[test]
    fn a_hidden_zone_takes_its_whole_span_away() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.ensure_spacer_capacity(92);
        let sep_cache = dock.add_zone_separator_at_end(Side::Right, 400.0);
        let contenu = dock.add_spacer_at_end(Side::Right);
        let sep_visible = dock.add_zone_separator_at_end(Side::Right, 400.0);
        dock.set_icon_order(vec![], vec![sep_cache, contenu, sep_visible], vec![], vec![]);
        // Un masquage posé à la main : c'est `apply_zones` qui le fait depuis
        // `ZoneRecord::hidden`, mais le TEST veut l'état, pas la lecture du fichier.
        dock.set_icon_hidden(sep_cache, true);

        assert!(dock.icon_rect(sep_cache, &layout).is_none(), "le séparateur caché n'a plus d'emplacement");
        assert!(dock.icon_rect(contenu, &layout).is_none(), "et le contenu de sa zone non plus");
        assert!(dock.icon_rect(sep_visible, &layout).is_some(), "la zone SUIVANTE survit, elle");
        assert!(
            dock.zone_boxes(&layout).iter().all(|(sep, _)| *sep != sep_cache),
            "une zone cachée n'a plus de boîte du tout"
        );
    }

    /// **LA LISTE DU CONTENU D'UNE ZONE — ce que le panneau affichera.**
    ///
    /// Elle se distingue du COMPTE (`ZoneSummary::content`) sur deux points, et ce
    /// test verrouille les deux : elle montre les **espaces** et les icônes
    /// **masquées**, que le compte écarte parce qu'il sert à reconnaître la zone à
    /// l'œil. Sans eux dans la liste, on ne pourrait ni voir un espace, ni
    /// réafficher une applet cachée depuis ce panneau — les deux demandes de
    /// l'utilisateur.
    ///
    /// Et le **miroir** n'apparaît que sur la DERNIÈRE zone : ses cases ne sont
    /// dans aucun ordre, donc sans cette ligne la zone qui le porte afficherait
    /// « 0 icône » tout en paraissant pleine à l'écran.
    #[test]
    fn zone_contents_lists_what_the_count_hides() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.ensure_spacer_capacity(92);
        dock.ensure_winpin_capacity(23);

        // DEUX zones, séparées par deux séparateurs, et un contenu ÉCRIT À LA MAIN
        // entre les deux : une icône visible, un espace de 30 px, une icône masquée.
        let sep_a = dock.add_zone_separator_at_end(Side::Right, 400.0);
        let sep_b = dock.add_zone_separator_at_end(Side::Right, 400.0);
        let espace = dock.add_spacer_at_end(Side::Right);
        dock.set_spacer_size(espace, 30.0);
        // Un index d'ICÔNE, pas d'espace : avec zéro épingle, les applets
        // commencent à `PINNED_ICON_BASE` (voir `Dock::new`).
        let visible = crate::app::PINNED_ICON_BASE;
        let masquee = crate::app::PINNED_ICON_BASE + 1;
        assert!(!dock.is_spacer(visible) && !dock.is_spacer(masquee), "prémisse : ce sont des icônes");
        dock.set_icon_order(vec![], vec![sep_a, visible, espace, masquee, sep_b], vec![], vec![]);
        dock.set_icon_hidden(masquee, true);
        // Le miroir APRÈS tout le reste, comme au lancement.
        dock.set_tray_count(3);

        assert_eq!(
            dock.zone_contents(sep_a),
            vec![
                ZoneEntry::Ordered { index: visible, hidden: false, spacer: None },
                ZoneEntry::Ordered { index: espace, hidden: false, spacer: Some(30.0) },
                ZoneEntry::Ordered { index: masquee, hidden: true, spacer: None },
            ],
            "la première zone liste son contenu, espaces et icônes masquées compris"
        );
        // Le COMPTE, lui, n'en voit qu'UNE — et c'est voulu : il sert à reconnaître
        // la zone à l'œil, pas à la décrire.
        assert_eq!(
            dock.zone_summaries().iter().find(|zone| zone.sep_index == sep_a).map(|zone| zone.content),
            Some(1),
            "le compte écarte l'espace et l'icône masquée"
        );
        assert_eq!(
            dock.zone_contents(sep_b),
            vec![ZoneEntry::Mirror { count: 3 }],
            "la DERNIÈRE zone porte le miroir, en une seule ligne"
        );
        // Un index qui n'ouvre aucune zone : rien, plutôt qu'une supposition.
        assert!(dock.zone_contents(visible).is_empty(), "une icône n'ouvre pas de zone");
    }

    /// **SONDE DE MESURE, PAS UN TEST** — rejoue la séquence de création d'un
    /// espace sur la configuration RÉELLE de la machine, avec la plage
    /// dynamique telle qu'elle est posée au lancement, et dit si l'espace créé
    /// est EXPORTÉ par `icon_order` (donc s'il sera persisté et replacé).
    ///
    /// C'est la reproduction, hors application, du défaut « les nouveaux
    /// séparateurs sont transparents » : un espace absent de `icon_order` n'est
    /// rattaché à aucune barre, donc jamais dessiné.
    ///
    /// `cargo test --bins -- --ignored --nocapture a_created_spacer`
    #[test]
    #[ignore]
    fn a_created_spacer_is_exported_by_icon_order_on_the_real_config() {
        let sizes = crate::config::load_spacer_sizes();
        let zones = crate::config::load_zone_heights();
        // ⚠️⚠️ **CETTE SONDE MESURE L'ANCIEN FORMAT, ET ELLE LE DIT.** Depuis la migration vers
        // `zones.toml`, `spacer_sizes.txt` et `zone_heights.txt` sont RENOMMÉS : les deux listes
        // ci-dessus seraient VIDES, et la sonde conclurait « rien de perdu » **sans avoir rien
        // mesuré**. C'est le piège que la mémoire du projet appelle « une absence de trace n'est
        // pas une preuve », et il est déjà arrivé trois fois ici.
        //
        // La sonde qui fait autorité est désormais
        // `ui::zones::tests::the_real_legacy_config_migrates_without_loss` : elle lit les anciens
        // fichiers ET rend le `zones.toml` produit, et sa sortie est déterministe.
        if sizes.is_empty() && zones.is_empty() && crate::config::zones_path().is_some_and(|path| path.exists()) {
            println!("SANS OBJET : les anciens fichiers de zone sont MIGRES (zones.toml existe).");
            println!("Voir `ui::zones::tests::the_real_legacy_config_migrates_without_loss`.");
            return;
        }
        let pins = crate::win32::taskbar_pins::read_pins();
        let base = crate::config::load_winpin_base();
        let order = crate::config::load_icon_order();
        // La CAPACITÉ, pas le nombre : voir `ui::reserve_epingles`.
        let epingles = crate::config::load_pinned_apps().len();
        let pinned = crate::config::load_pin_capacity().unwrap_or(epingles).max(epingles);

        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], pinned);
        dock.ensure_spacer_capacity(base.unwrap_or(0));
        let winpin_base = dock.ensure_winpin_capacity(pins.len());
        let winpin_end = winpin_base + pins.len();
        if let Some((left, right, top_left, top_right)) = order {
            dock.ensure_saved_spacers_exist([&left, &right, &top_left, &top_right], floor(&zones), winpin_end);
            dock.set_icon_order(left, right, top_left, top_right);
        }
        dock.set_spacer_sizes(&sizes);
        dock.set_zone_heights(&zones);
        println!("au demarrage : icons.len() = {}", dock.icons.len());

        // La plage dynamique est posée dès qu'une application non épinglée est
        // détectée — `set_dynamic_apps` fige `dynamic_base` à la longueur
        // courante du vecteur.
        let dynamic_base = dock.set_dynamic_apps(1);
        println!("dynamic_base ................... {dynamic_base}");

        // Le geste : Ctrl + clic droit dans le vide d'une barre latérale.
        let created = dock.add_spacer_at_end(Side::Left);
        let (l, r, tl, tr) = dock.icon_order();
        let exported = [&l, &r, &tl, &tr].iter().any(|o| o.contains(&created));
        println!("espace cree a l'index .......... {created}");
        println!("dynamic_base apres creation .... {:?}", dock.dynamic_icon_base());
        println!("EXPORTE par icon_order ......... {exported}   (attendu : true)");
        println!("listes ecrites : gauche={l:?}");
        assert!(exported, "l'espace cree a l'index {created} n'est pas exporte : il ne sera jamais persisté");
    }

    /// **CE QUI SE PASSE (ET NE SE PASSE PAS) DANS LA BARRE DU HAUT.** Le geste
    /// de création d'un espace passe par `side_at` quand le curseur n'est sur
    /// aucune icône — et `side_at` rend `None` dans TOUTE la bande du haut
    /// (`y < top_thickness`), délibérément : son doc ne promet que « l'espace
    /// VIDE d'une barre, sous sa dernière icône », une notion latérale.
    ///
    /// Conséquence à connaître : **Ctrl + clic droit dans le vide de la barre du
    /// haut ne crée RIEN, en silence**. Dans la barre du haut, un espace ne
    /// naît qu'en cliquant SUR une icône (`add_spacer_after`, qui insère le
    /// nouveau venu juste après elle). Rien n'indique à l'utilisateur laquelle
    /// des deux choses vient de se produire — c'est le genre d'écart entre ce
    /// que le geste promet et ce qu'il fait qui se lit comme « ça ne marche
    /// pas ».
    #[test]
    fn the_top_bar_has_no_empty_space_to_create_a_spacer_in() {
        let layout = geometry_layout();
        let dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);

        // Un point de la bande du haut, au milieu (donc loin des icônes).
        assert_eq!(dock.side_at(layout.total_width * 0.5, layout.top_thickness * 0.5, &layout), None);
        // Et le même point un peu plus bas, dans une barre latérale, répond.
        assert_eq!(dock.side_at(layout.left_width * 0.5, layout.top_thickness + 10.0, &layout), Some(Side::Left));
    }

    /// **`apply_zones` NE TOUCHE PLUS À LA BARRE DU BAS — L'ARRANGEMENT PORTE SES SORTES.**
    ///
    /// ⚠️ **CE TEST EN REMPLACE UN AUTRE, ET SON PRÉDÉCESSEUR AVAIT RAISON À L'ÉPOQUE.**
    /// Il figeait le correctif du 2026-09-30 : `apply_zones` devait passer **après**
    /// `set_dossiers`, sans quoi les clés `B*` ne se résolvaient sur rien et les cases
    /// naissaient en `Rond` — un espace ÉLASTIQUE redevenait ordinaire à chaque
    /// ouverture.
    ///
    /// Depuis le 2026-10-01, **la sorte voyage AVEC L'ÉLÉMENT** (voir
    /// `config::ElementBarre::Separateur`) : une liste **par rang** ne survit pas à une
    /// insertion, or insérer un séparateur est précisément ce qu'on fait dans cette
    /// barre. `[bars.B] sortes` n'est donc plus ni écrit ni appliqué, et le rôle
    /// d'`apply_zones` pour cette barre **a disparu**.
    ///
    /// Ce test fige la règle NOUVELLE, dans les deux sens : la clé `B*` ne pose plus
    /// rien, et ce que `set_dossiers` a posé **reste**. Si la garde est retirée, cette
    /// assertion tombe — l'élastique redeviendrait rond.
    #[test]
    fn apply_zones_ne_touche_plus_a_la_barre_du_bas() {
        use crate::ui::zones::{BarLetter, BarSpacers, Orders, SorteEspace, Zones};

        // La mécanique d'`app::run` : copier les orders (ils empruntent le dock), puis
        // appliquer. Écrite ici pour que le test emprunte le MÊME chemin.
        let appliquer = |dock: &mut Dock, zones: &Zones| {
            let (gauche, droite, haut_gauche, haut_droite, bas) = {
                let ordres = dock.orders();
                (
                    ordres.of(Side::Left).to_vec(),
                    ordres.of(Side::Right).to_vec(),
                    ordres.of(Side::TopLeft).to_vec(),
                    ordres.of(Side::TopRight).to_vec(),
                    ordres.of(Side::Bottom).to_vec(),
                )
            };
            dock.apply_zones(Orders::new(&gauche, &droite, &haut_gauche, &haut_droite, &bas), zones);
        };
        let seul_separateur = |dock: &Dock| {
            dock.order(Side::Bottom)
                .iter()
                .copied()
                .find(|&index| dock.is_spacer(index))
                .expect("la barre du bas cite un separateur")
        };

        // `[bars.B]` DÉCLARE « ROND » — s'il était encore appliqué, il DÉFERAIT
        // l'élastique que l'arrangement porte. C'est exactement ce qu'on mesure.
        let mut zones = Zones::default();
        zones.bars.insert(
            BarLetter::Bas,
            BarSpacers {
                spacers: 1,
                sizes: vec![super::SPACER_HEIGHT],
                sortes: vec![SorteEspace::Rond],
                ..BarSpacers::default()
            },
        );
        let elements = [crate::config::ElementBarre::Separateur(SorteEspace::Elastique)];

        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.set_dossiers(&elements);
        assert_eq!(
            dock.spacer_sorte(seul_separateur(&dock)),
            Some(SorteEspace::Elastique),
            "prémisse : la sorte vient de l'ÉLÉMENT (voir `ElementBarre::Separateur`)"
        );

        // LA PASSE QUE `app::run` REJOUE APRÈS `set_dossiers` — celle qui, avant ce soir,
        // réappliquait les sortes du bas PAR RANG et les défaisait.
        appliquer(&mut dock, &zones);
        assert_eq!(
            dock.spacer_sorte(seul_separateur(&dock)),
            Some(SorteEspace::Elastique),
            "`apply_zones` ne doit PAS defaire ce que `set_dossiers` a pose (il dirait Rond)"
        );
    }

    /// **SONDE DE MESURE, PAS UN TEST** — rejoue la reconstruction d'`app::run`
    /// sur la configuration RÉELLE de cette machine et dit combien d'espaces
    /// enregistrés retrouvent un emplacement. Même motif que
    /// `win32::taskbar_pins::tests::read_pins_prints_the_real_taskbar_order` :
    /// mesurer sur les vraies données, sans lancer l'application.
    ///
    /// `cargo test --bins -- --ignored --nocapture saved_spacers`
    #[test]
    #[ignore]
    fn saved_spacers_resolve_against_the_real_config() {
        let sizes = crate::config::load_spacer_sizes();
        let zones = crate::config::load_zone_heights();
        // ⚠️⚠️ **SANS OBJET APRÈS LA MIGRATION — et elle le dit.** Cette sonde compare deux
        // reconstructions sur les fichiers INDEXÉS PAR LE SÉPARATEUR, que `zones.toml` remplace
        // et que la migration RENOMME. Les deux listes seraient vides, les deux rapports
        // identiques et nuls, et la comparaison ne mesurerait plus rien.
        //
        // La question qu'elle posait — « chaque espace enregistré retrouve-t-il un emplacement ? »
        // — se pose désormais **par clé**, et c'est
        // `ui::zones::tests::the_real_legacy_config_migrates_without_loss` qui y répond.
        if sizes.is_empty() && zones.is_empty() && crate::config::zones_path().is_some_and(|path| path.exists()) {
            println!("SANS OBJET : les anciens fichiers de zone sont MIGRES (zones.toml existe).");
            println!("Voir `ui::zones::tests::the_real_legacy_config_migrates_without_loss`.");
            return;
        }
        let pins = crate::win32::taskbar_pins::read_pins();
        let base = crate::config::load_winpin_base();
        let order = crate::config::load_icon_order();
        // La CAPACITÉ, pas le nombre : voir `ui::reserve_epingles`.
        let epingles = crate::config::load_pinned_apps().len();
        let pinned = crate::config::load_pin_capacity().unwrap_or(epingles).max(epingles);

        // DEUX reconstructions COMPLÈTES, la seule différence étant l'étape
        // ajoutée — donc tout ce qui diffère entre les deux rapports vient
        // d'elle, et de rien d'autre.
        for (label, recreate) in [("AVANT (aujourd'hui)", false), ("APRES (correctif)", true)] {
            let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], pinned);
            dock.ensure_spacer_capacity(base.unwrap_or(0));
            let winpin_base = dock.ensure_winpin_capacity(pins.len());
            let winpin_end = winpin_base + pins.len();
            match order.clone() {
                Some((left, right, top_left, top_right)) => {
                    if recreate {
                        dock.ensure_saved_spacers_exist([&left, &right, &top_left, &top_right], floor(&zones), winpin_end);
                    }
                    dock.set_icon_order(left, right, top_left, top_right);
                }
                None if recreate => dock.ensure_saved_spacers_exist([&[], &[], &[], &[]], floor(&zones), winpin_end),
                None => {}
            }
            dock.set_spacer_sizes(&sizes);
            dock.set_zone_heights(&zones);

            let out_of_range = |i: usize| i >= dock.icons.len();
            let missing_spacers = sizes.iter().filter(|&&(i, _)| out_of_range(i)).count();
            let missing_zones = zones.iter().filter(|&&(i, _, _)| out_of_range(i)).count();
            let lost_zones = zones.iter().filter(|&&(i, _, _)| !dock.is_zone_separator(i)).count();
            let left = dock.order(Side::Left).to_vec();
            let left_spacers = left.iter().filter(|&&i| dock.is_spacer(i)).count();
            let left_zones = left.iter().filter(|&&i| dock.is_zone_separator(i)).count();

            println!("=== {label} ===");
            println!("  base des epingles ................ {winpin_base}   (epingles lues : {})", pins.len());
            println!("  longueur d'icons ................. {}", dock.icons.len());
            println!("  espaces enregistres hors bornes .. {missing_spacers}");
            println!("  cotes enregistrees hors bornes ... {missing_zones}");
            println!("  cotes qui n'ont PAS pris ......... {lost_zones}");
            println!(
                "  barre de GAUCHE .................. {} elements, dont {left_spacers} espaces et {left_zones} zones",
                left.len()
            );
        }
    }

    // ---- Le défilement PROPRE à une zone --------------------------------

    /// Monte une barre de droite DÉBORDANTE portant UNE zone dont le contenu
    /// déborde aussi (400 px dans une cote de 100). Retourne le dock, le
    /// séparateur, le contenu de la zone et la barre déjà calculée.
    ///
    /// Le grand blanc de TÊTE n'est pas décoratif : sans lui la barre de
    /// droite, qui ne contient que « Réglages » et deux espaces, tiendrait
    /// dans la fenêtre, et un test de défilement de BARRE n'y mesurerait rien.
    fn dock_with_an_overflowing_zone(layout: &DockLayout) -> (Dock, usize, usize, BarLayout) {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let filler = dock.icons.len() - 1;
        dock.set_spacer_size(filler, 4000.0);
        dock.add_spacer_at_end(Side::Right);
        let sep = dock.icons.len() - 1;
        dock.set_zone_height(sep, 100.0);
        dock.add_spacer_at_end(Side::Right);
        let inner = dock.icons.len() - 1;
        dock.set_spacer_size(inner, 400.0);
        let (_, bar) = right_bar(&dock, layout);
        assert!(bar.zones[0].content_height > 100.0, "prémisse : le contenu de la zone déborde de sa cote");
        (dock, sep, inner, bar)
    }

    /// **La molette dans une zone fait défiler SON CONTENU, pas la barre** —
    /// c'est tout l'objet du défilement par zone : deux zones d'une même barre
    /// se parcourent indépendamment. Sans ce test, on ne saurait pas si la
    /// molette continue simplement de faire glisser toute la barre.
    #[test]
    fn the_wheel_scrolls_the_zone_under_the_cursor_not_the_bar() {
        let layout = geometry_layout();
        let (mut dock, sep, inner, bar) = dock_with_an_overflowing_zone(&layout);
        let (order, _) = right_bar(&dock, &layout);
        let before = slot_of(&order, &bar, inner).top;

        // `y` DANS la boîte de la zone, et non sur son séparateur (qui en est
        // le bord, pas le contenu).
        let y = bar.zones[0].top + 10.0;
        dock.scroll_bar(Side::Right, -WHEEL_DELTA, y, &layout);

        assert_eq!(dock.zone_scroll_offset(sep), SCROLL_STEP_PX, "un cran, en pixels de défilement");
        assert_eq!(dock.scroll[1], 0.0, "la BARRE, elle, n'a pas bougé d'un pixel");
        let (_, after) = right_bar(&dock, &layout);
        assert_eq!(
            slot_of(&order, &after, inner).top,
            before - SCROLL_STEP_PX,
            "le contenu de la zone est bien remonté d'un cran"
        );
    }

    /// Hors de toute zone (ici sous la barre du haut, avant le premier
    /// séparateur) la molette fait défiler la BARRE ENTIÈRE, comme avant les
    /// zones — le comportement d'origine ne doit pas se perdre en chemin.
    #[test]
    fn the_wheel_outside_any_zone_still_scrolls_the_whole_bar() {
        let layout = geometry_layout();
        let (mut dock, sep, _, bar) = dock_with_an_overflowing_zone(&layout);
        // Précis : le contenu qui PRÉCÈDE la zone (l'icône Réglages) est bien
        // hors de ses bornes.
        let y = bar.zones[0].top - 1.0;
        assert!(y > layout.top_thickness, "prémisse : ce point est dans la barre, pas dans la barre du haut");
        dock.scroll_bar(Side::Right, -WHEEL_DELTA, y, &layout);

        assert!(dock.scroll[1] > 0.0, "la barre a bien défilé : {}", dock.scroll[1]);
        assert_eq!(dock.zone_scroll_offset(sep), 0.0, "et AUCUNE zone n'a bougé");
    }

    /// **Arrivée au bout de son contenu, la zone CÈDE la molette à la barre** —
    /// DEMANDE EXPLICITE : « lorsqu'une zone est masquée, l'utilisateur doit
    /// pouvoir faire défiler le contenu de la barre de droite avec la molette ».
    /// Sans ce relais, une zone à sa butée avalait le cran : la barre ne bougeait
    /// plus jamais, et les zones situées plus bas — hors de la fenêtre, donc
    /// jamais atteintes par le curseur — étaient INACCESSIBLES.
    ///
    /// Les deux moitiés de la règle sont vérifiées ici, dans cet ordre : la zone
    /// d'abord (la barre ne bouge pas d'un pixel tant que la zone répond), puis
    /// la barre.
    #[test]
    fn a_zone_that_reached_its_end_hands_the_wheel_to_the_bar() {
        let layout = geometry_layout();
        let (mut dock, sep, _, bar) = dock_with_an_overflowing_zone(&layout);
        let y = bar.zones[0].top + 10.0;
        let allowance = bar.zones[0].content_height - (bar.zones[0].bottom - bar.zones[0].top);
        assert!(allowance > SCROLL_STEP_PX, "prémisse : la zone a de quoi tenir plusieurs crans");

        // Tant qu'il lui reste du contenu, la zone garde la molette pour elle.
        let mut notches = 0;
        loop {
            let (zone_before, bar_before) = (dock.zone_scroll_offset(sep), dock.scroll[1]);
            dock.scroll_bar(Side::Right, -WHEEL_DELTA, y, &layout);
            if dock.zone_scroll_offset(sep) == zone_before {
                break;
            }
            assert!(dock.zone_scroll_offset(sep) > zone_before, "elle descend bien vers la fin de son contenu");
            assert_eq!(dock.scroll[1], bar_before, "et la BARRE attend son tour");
            notches += 1;
            assert!(notches < 100, "la zone n'atteint jamais sa fin : borne fausse");
        }
        assert_eq!(dock.zone_scroll_offset(sep), allowance, "elle s'arrête EXACTEMENT à sa borne");

        // Le cran suivant, lui, appartient à la barre.
        let bar_before = dock.scroll[1];
        dock.scroll_bar(Side::Right, -WHEEL_DELTA, y, &layout);
        assert!(dock.scroll[1] > bar_before, "le cran passe à la barre : {bar_before} → {}", dock.scroll[1]);
        assert_eq!(dock.zone_scroll_offset(sep), allowance, "sans que la zone reparte en arrière");
    }

    /// Une zone qui n'a **rien à faire défiler** — son contenu tient dans sa
    /// cote, ou elle n'en a aucun (toutes ses icônes désactivées) — cède le cran
    /// comme une zone arrivée au bout : c'est le cas « zone masquée » sous lequel
    /// l'utilisateur a décrit le défaut. La zone n'est pas bornée pour autant, et
    /// elle le montre ici : elle ne bouge pas d'un pixel, c'est la barre qui
    /// travaille.
    #[test]
    fn a_zone_with_nothing_to_scroll_hands_the_wheel_to_the_bar() {
        let layout = geometry_layout();
        let (mut dock, sep, inner, _) = dock_with_an_overflowing_zone(&layout);
        // Le contenu de la zone rétrécit jusqu'à tenir dans sa cote.
        dock.set_spacer_size(inner, 40.0);
        let (_, bar) = right_bar(&dock, &layout);
        assert!(bar.zones[0].content_height <= bar.zones[0].bottom - bar.zones[0].top, "prémisse : plus rien à défiler");
        let y = bar.zones[0].top + 10.0;
        assert!(y < bar.zones[0].bottom, "prémisse : ce point est bien DANS la boîte de la zone");

        let bar_before = dock.scroll[1];
        dock.scroll_bar(Side::Right, -WHEEL_DELTA, y, &layout);

        assert!(dock.scroll[1] > bar_before, "la barre a bien défilé : {bar_before} → {}", dock.scroll[1]);
        assert_eq!(dock.zone_scroll_offset(sep), 0.0, "et la zone, qui n'avait rien à donner, n'a pas bougé");
    }

    /// **Une vitesse PAR BARRE, et un cran vaut exactement cette vitesse** —
    /// DEMANDE EXPLICITE : « pouvoir régler les vitesses de défilement à la
    /// molette dans les barres », une par barre. Le défaut reste la valeur
    /// d'origine (48 px, voir `SCROLL_STEP_PX`) : les tests de défilement qui
    /// l'assertent mesurent donc encore ce qu'ils croient.
    ///
    /// Le défilement À L'INTÉRIEUR d'une zone suit la vitesse de SA barre : même
    /// chemin (`scroll_bar`), donc rien à vérifier de plus — sauf que le pas est
    /// bien celui de la barre, ce que ce test tient par les deux bouts.
    #[test]
    fn the_wheel_step_is_settable_per_bar() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        // De quoi faire déborder la barre de GAUCHE (les applications ouvertes y
        // vont), sinon rien ne peut y défiler.
        dock.set_dynamic_apps(40);
        // ...et de quoi faire déborder celle de DROITE, qui n'a sinon que
        // « Réglages » : sans ce grand espace, son décalage serait borné à zéro
        // et le test mesurerait la borne, pas la vitesse.
        //
        // L'index RENDU, jamais `icons.len() - 1` : un espace créé s'INSÈRE
        // au-dessus de la plage des applications ouvertes (voir
        // `Dock::insert_spacer`), donc pas au bout du vecteur — et c'est
        // précisément le cas ici, après `set_dynamic_apps`.
        let filler = dock.add_spacer_at_end(Side::Right);
        dock.set_spacer_size(filler, 4000.0);

        assert_eq!(scroll_step_px(Side::Left), DEFAULT_SCROLL_STEP_PX, "défaut : la valeur d'origine");
        assert_eq!(scroll_step_px(Side::Right), DEFAULT_SCROLL_STEP_PX, "défaut : la valeur d'origine");

        set_scroll_step_px(Side::Left, 24.0);
        set_scroll_step_px(Side::Right, 120.0);
        assert_eq!(scroll_step_px(Side::Left), 24.0);
        assert_eq!(scroll_step_px(Side::Right), 120.0, "régler une barre ne touche pas l'autre");

        let y = layout.top_thickness + 100.0;
        dock.scroll_bar(Side::Left, -WHEEL_DELTA, y, &layout);
        assert_eq!(dock.scroll[side_slot(Side::Left)], 24.0, "un cran = la vitesse de la barre de gauche");
        dock.scroll_bar(Side::Right, -WHEEL_DELTA, y, &layout);
        assert_eq!(dock.scroll[side_slot(Side::Right)], 120.0, "la barre de droite a la SIENNE");

        // Bornage : un curseur ne peut pas sortir de la plage que l'interface
        // annonce (voir `MIN_SCROLL_STEP_PX`/`MAX_SCROLL_STEP_PX`).
        set_scroll_step_px(Side::Left, 1.0);
        assert_eq!(scroll_step_px(Side::Left), MIN_SCROLL_STEP_PX);
        set_scroll_step_px(Side::Left, 10_000.0);
        assert_eq!(scroll_step_px(Side::Left), MAX_SCROLL_STEP_PX);

        // Remise au défaut : ces cellules sont globales au THREAD (chaque test a
        // le sien, voir `SCROLL_STEP`), mais un test laisse l'état comme il l'a
        // trouvé — c'est le seul moyen qu'un futur test qui lit le défaut ne
        // dépende pas de l'ORDRE d'exécution.
        set_scroll_step_px(Side::Left, DEFAULT_SCROLL_STEP_PX);
        set_scroll_step_px(Side::Right, DEFAULT_SCROLL_STEP_PX);
    }

    /// Une zone dont le contenu TIENT dans sa cote ne défile pas : rien à
    /// parcourir. C'est la borne `content_height - cote`, et sans elle le
    /// contenu remonterait hors de sa boîte en laissant un vide dessous.
    #[test]
    fn a_zone_whose_content_fits_does_not_scroll() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let sep = dock.icons.len() - 1;
        dock.set_zone_height(sep, 400.0);
        dock.add_spacer_at_end(Side::Right);
        let inner = dock.icons.len() - 1;
        dock.set_spacer_size(inner, 40.0); // bien moins que la cote
        let (_, bar) = right_bar(&dock, &layout);
        assert!(bar.zones[0].content_height < 400.0, "prémisse : le contenu tient dans la zone");

        let y = bar.zones[0].top + 10.0;
        for _ in 0..5 {
            dock.scroll_bar(Side::Right, -WHEEL_DELTA, y, &layout);
        }
        assert_eq!(dock.zone_scroll_offset(sep), 0.0, "cinq crans dans le vide : la zone reste au repos");
    }

    // ---- La découpe par zone (voir `Dock::zone_clips`) -------------------

    /// Le découpage d'une icône de zone tombe sur LA BOÎTE de sa zone, en
    /// PLEINE LARGEUR (seule la verticale est bornée, comme `lateral_clip` —
    /// sinon la magnification au survol serait rognée sur les côtés), et rien
    /// d'autre n'est découpé par zone. C'est ce qui rogne un contenu qui
    /// déborde de sa cote.
    #[test]
    fn zone_clips_cut_to_the_zone_box_and_leave_everything_else_alone() {
        let layout = geometry_layout();
        let (dock, _, inner, bar) = dock_with_an_overflowing_zone(&layout);
        let zone = bar.zones[0];
        let clips = dock.zone_clips(&layout);

        assert_eq!(
            clips[inner],
            Some([0.0, zone.top, layout.total_width, zone.bottom - zone.top]),
            "le contenu de la zone est rogné à sa boîte, pleine largeur"
        );
        // Le contenu qui PRÉCÈDE le premier séparateur n'appartient à aucune
        // zone : découper à la boîte d'une zone y serait arbitraire, et le
        // découpage de la barre suffit. Le SÉPARATEUR lui-même non plus : il
        // est le BORD de la boîte, le rogner effacerait la limite même qu'il
        // dessine.
        let first_of_bar = dock.order(Side::Right)[0];
        assert_eq!(clips[first_of_bar], None, "hors zone, aucune découpe supplémentaire");
        assert_eq!(clips[bar.zones[0].sep_index], None, "le séparateur est le bord, pas le contenu");
        for (index, _) in dock.icons.iter().enumerate() {
            if index != inner {
                assert_eq!(clips[index], None, "icône {index} : hors zone, rien à découper");
            }
        }
    }

    /// **Le découpage d'une zone ne suit PAS son contenu.** Il reste sur la
    /// boîte pendant que le contenu défile — sinon il ne rognerait rien du
    /// tout : il suivrait exactement ce qu'il est censé retenir.
    #[test]
    fn a_zone_clip_stays_on_the_box_while_its_content_scrolls() {
        let layout = geometry_layout();
        let (mut dock, _, inner, bar) = dock_with_an_overflowing_zone(&layout);
        let before = dock.zone_clips(&layout)[inner];

        dock.scroll_bar(Side::Right, -WHEEL_DELTA, bar.zones[0].top + 10.0, &layout);

        assert_eq!(dock.zone_scroll_offset(bar.zones[0].sep_index), SCROLL_STEP_PX, "prémisse : la zone a défilé");
        assert_eq!(dock.zone_clips(&layout)[inner], before, "la boîte n'a pas bougé, sa découpe non plus");
    }

    /// Le découpage d'une zone ne remonte jamais par-dessus la barre du HAUT —
    /// le plancher de `lateral_clip` est conservé : une zone ne fait pas
    /// dessiner son contenu sur la barre du haut.
    #[test]
    fn a_zone_clip_never_reaches_above_the_top_bar() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let sep = dock.icons.len() - 1;
        dock.set_zone_height(sep, 400.0);
        dock.add_spacer_at_end(Side::Right);
        let inner = dock.icons.len() - 1;

        // Une zone qui commence AU-DESSUS de la barre du haut : le défilement
        // de BARRE l'y a poussée, la zone ne s'en aperçoit pas.
        let (_, bar) = right_bar(&dock, &layout);
        let excess = bar.zones[0].top - layout.top_thickness + 50.0;
        assert!(excess > 0.0, "prémisse : la zone commence bien sous la barre du haut");
        dock.scroll[1] = excess;

        let clip = dock.zone_clips(&layout)[inner].expect("prémisse : l'icône est dans une zone");
        assert_eq!(clip[1], layout.top_thickness, "le haut de la découpe est celui de la barre du haut");
        assert!(clip[3] > 0.0, "et la zone garde de la hauteur visible : {}", clip[3]);
    }

    /// **Ce que la zone ne dessine pas ne doit pas être cliquable** — le
    /// pendant exact, pour la zone, de l'exclusion des emplacements sortis de
    /// la FENÊTRE (`layout_side`) : un élément entièrement hors de sa boîte
    /// n'a plus de rectangle du tout, donc plus de clic ni de survol, alors
    /// qu'il restait atteignable dans la bande qu'il occupe.
    #[test]
    fn content_clipped_out_of_its_zone_is_not_clickable() {
        let layout = geometry_layout();
        // Une zone qui vaut EXACTEMENT le premier élément : celui-ci remplit la
        // boîte, et tout ce qui suit tombe entièrement dessous. La cote est
        // choisie ainsi (et non « plus petite ») pour que l'exclusion porte sur
        // un élément vraiment invisible, pas sur un chevauchement de quelques
        // pixels — un élément à moitié rogné, lui, DOIT rester cliquable.
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let sep = dock.icons.len() - 1;
        dock.set_zone_height(sep, 30.0);
        dock.add_spacer_at_end(Side::Right);
        let first = dock.icons.len() - 1;
        dock.set_spacer_size(first, 30.0);
        dock.add_spacer_at_end(Side::Right);
        let second = dock.icons.len() - 1;
        dock.set_spacer_size(second, 400.0);

        let (_, bar) = right_bar(&dock, &layout);
        let zone = bar.zones[0];
        let first_slot = slot_of(&dock.order(Side::Right), &bar, first);
        let second_slot = slot_of(&dock.order(Side::Right), &bar, second);
        // Prémisses : le premier tient dans la boîte, le second est ENTIÈREMENT
        // dessous — sans quoi le test ne prouverait rien.
        assert!(first_slot.top + first_slot.height <= zone.bottom, "le premier est dans la boîte");
        assert!(second_slot.top >= zone.bottom, "le second est entièrement dehors");

        assert!(dock.icon_rect(first, &layout).is_some(), "celui qu'on voit reste cliquable");
        assert!(dock.icon_rect(second, &layout).is_none(), "celui qu'on ne voit plus ne l'est plus");
    }

    /// Le défilement d'une zone s'OUBLIE quand la zone est défaite (cote
    /// ramenée à 0) — la refaire doit la rouvrir AU REPOS, pas à la position
    /// où elle avait été laissée. Sans cet oubli, une zone rouverte
    /// apparaîtrait déjà défilée, ce qui se lirait comme un défaut.
    #[test]
    fn making_a_spacer_ordinary_again_forgets_where_its_zone_had_scrolled() {
        let layout = geometry_layout();
        let (mut dock, sep, _, bar) = dock_with_an_overflowing_zone(&layout);

        dock.scroll_bar(Side::Right, -WHEEL_DELTA, bar.zones[0].top + 10.0, &layout);
        assert_eq!(dock.zone_scroll_offset(sep), SCROLL_STEP_PX, "prémisse : la zone a bien défilé");

        dock.toggle_zone_separator(sep); // cote à 0 : la zone est défaite
        assert_eq!(dock.zone_height(sep), Some(0.0));
        dock.toggle_zone_separator(sep); // et la refaire
        assert_eq!(dock.zone_scroll_offset(sep), 0.0, "la zone rouverte repart au repos");
    }

    /// **Le geste partagé, et c'est tout l'enjeu** : Maj + glisser règle la
    /// COTE sur un séparateur de zone, la taille du blanc sur un espace
    /// ordinaire. Sans ce test, rien ne dirait si le réglage de la cote a
    /// simplement REMPLACÉ celui de la taille d'espace.
    ///
    /// La taille surveillée ici est celle que le geste RÈGLE (le blanc d'un
    /// espace, tel qu'il est persisté) — sur un séparateur de zone elle ne
    /// décrit plus ce qui est dessiné (voir `a_zone_separator_keeps_its_marker_
    /// size`), mais elle doit rester intacte sous le doigt : c'est elle qui
    /// reprendrait effet si la zone était défaite.
    #[test]
    fn shift_drag_resizes_the_cote_of_a_zone_separator_and_leaves_the_marker_alone() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let sep = dock.icons.len() - 1;
        dock.set_spacer_size(sep, 24.0);
        dock.toggle_zone_separator(sep);
        let marker_before = dock.spacer_size(sep);

        let (x, y) = (900.0, 400.0);
        dock.begin_potential_drag(sep, x, y, HWND::default());
        dock.update_drag(x, y + 10.0, false, true, &layout); // franchit le seuil, Maj
        dock.update_drag(x, y + 160.0, false, true, &layout); // 160 px plus bas que le CLIC

        assert_eq!(dock.zone_height(sep), Some(DEFAULT_ZONE_HEIGHT + 160.0), "la cote suit le curseur");
        assert_eq!(dock.spacer_size(sep), marker_before, "la taille de MARQUEUR ne bouge pas");
        assert!(matches!(dock.end_drag(&layout, HWND::default()), DragOutcome::Resized));
    }

    /// Descendre la cote jusqu'à zéro ramène l'espace à l'état ordinaire — la
    /// bascule de Maj + clic droit, atteinte à la souris, et la raison pour
    /// laquelle `set_zone_height` accepte 0.
    ///
    /// **Le retour ne doit pas être un aller sans retour** : le curseur remonté
    /// dans le MÊME geste doit pouvoir rétablir la cote. C'est ce que fige
    /// `IconDrag::resize_zone`, et ce test le tient.
    #[test]
    fn dragging_the_cote_to_zero_makes_the_spacer_ordinary_again() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let sep = dock.icons.len() - 1;
        dock.toggle_zone_separator(sep);
        dock.add_spacer_at_end(Side::Right);
        let (order, with_zone) = right_bar(&dock, &layout);
        assert_eq!(with_zone.zones.len(), 1, "prémisse : une zone est bien déclarée");

        let (x, y) = (900.0, 400.0);
        dock.begin_potential_drag(sep, x, y, HWND::default());
        dock.update_drag(x, y + 10.0, false, true, &layout); // franchit le seuil, Maj
        dock.update_drag(x, y - 100_000.0, false, true, &layout); // tiré très haut : cote à 0
        assert_eq!(dock.zone_height(sep), Some(0.0), "la cote est ramenée à zéro");
        // Le MÊME geste redescend : la cote doit revenir, et non la taille de
        // marqueur se mettre à bouger à sa place.
        dock.update_drag(x, y + 60.0, false, true, &layout);
        // 60 px SOUS LE CLIC, donc `DEFAULT_ZONE_HEIGHT + 60` — la cote suit la
        // position ABSOLUE du curseur, jamais un cumul depuis l'image d'avant.
        assert_eq!(dock.zone_height(sep), Some(DEFAULT_ZONE_HEIGHT + 60.0), "la cote se rétablit dans le même geste");
        assert_eq!(dock.spacer_size(sep), Some(SPACER_HEIGHT), "et la taille de marqueur reste intacte");
        let _ = dock.end_drag(&layout, HWND::default());

        // Retour à zéro pour de bon : plus aucune zone, et rien n'a bougé.
        dock.toggle_zone_separator(sep);
        assert!(!dock.is_zone_separator(sep));
        let (order_after, without_zone) = right_bar(&dock, &layout);
        assert_eq!(order, order_after, "aucun ordre n'a bougé");
        assert!(without_zone.zones.is_empty(), "plus aucune zone déclarée");
    }

    /// La cote atteint le RENDU, pas seulement le calcul pur : les
    /// emplacements que `icon_rect` renvoie (donc ce que le dock dessine, ce
    /// que le clic et le survol utilisent) doivent être ceux des zones. Sans ce
    /// test, un `layout_side` resté sur l'ancien calcul passerait inaperçu.
    #[test]
    fn the_rendered_rects_follow_the_declared_zone_height() {
        let layout = geometry_layout();
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.add_spacer_at_end(Side::Right);
        let first_sep = dock.icons.len() - 1;
        // Zone au contenu VIDE : ses 400 px sont entièrement du vide, donc
        // l'écart mesuré ne peut venir que de la cote.
        dock.set_zone_height(first_sep, 400.0);
        dock.add_spacer_at_end(Side::Right);
        let second_sep = dock.icons.len() - 1;
        dock.set_zone_height(second_sep, 300.0);

        let (order, bar) = right_bar(&dock, &layout);
        let rendered = |index: usize| dock.icon_rect(index, &layout).expect("prémisse : l'élément a un emplacement")[1];

        assert_eq!(rendered(second_sep), bar.zones[0].bottom, "le 2e séparateur est dessiné au bas de la 1re zone");
        assert_eq!(
            rendered(second_sep),
            slot_of(&order, &bar, second_sep).top,
            "le rendu suit `bar_layout`, il ne le recalcule pas"
        );
        assert!(
            rendered(second_sep) > rendered(first_sep) + 400.0,
            "l'écart vient bien de la cote, pas de la simple pile des éléments"
        );
    }
}

/// Le `Rectangle` tient sa largeur de l'ICÔNE, pas du curseur de taille — c'est
/// ce qui le distingue du `Trait` (DEMANDE EXPLICITE : « un rectangle de la
/// moitié de la largeur de l'icône »).
#[test]
fn running_dot_rectangle_follows_the_icon_width() {
    let size = DEFAULT_RUNNING_DOT_SIZE;
    for icon_w in [32.0, 64.0, 96.0] {
        let (w, h, _) = RunningDotShape::Rectangle.hung_geometry(size, icon_w);
        assert_eq!(w, icon_w * 0.5, "largeur = moitié de l'icône");
        assert_eq!(h, size, "hauteur = réglage de taille");
    }
    // Le trait, lui, ne dépend PAS de l'icône : c'est ce qui justifie deux
    // formes distinctes.
    let (bar_a, ..) = RunningDotShape::Bar.hung_geometry(size, 32.0);
    let (bar_b, ..) = RunningDotShape::Bar.hung_geometry(size, 96.0);
    assert_eq!(bar_a, bar_b);
}

/// L'indicateur grandit VERS LE HAUT : son bord inférieur reste à la même place
/// quelle que soit la taille (DÉFAUT RÉEL SIGNALÉ — « quand j'agrandis les
/// indicateurs ils s'agrandissent aussi vers le bas et vont trop sur les icônes
/// inférieures »).
/// Les formes SUSPENDUES — tout sauf l'anneau, qui n'a pas de boîte suspendue
/// (voir `RunningDotShape::geometry`). Les tests de géométrie suspendue itèrent
/// sur celles-ci plutôt que sur `ALL` ; `only_the_ring_goes_around` vérifie
/// l'autre moitié du contrat.
#[cfg(test)]
fn hung_shapes() -> impl Iterator<Item = RunningDotShape> {
    RunningDotShape::ALL.into_iter().filter(|shape| shape.placement() == DotPlacement::Hung)
}

#[test]
fn running_dot_grows_upward_only() {
    let icon = [10.0, 100.0, 64.0, 64.0];
    let icon_bottom = icon[1] + icon[3];

    for shape in hung_shapes() {
        let mut previous_top: Option<f32> = None;
        for size in [MIN_RUNNING_DOT_SIZE, 12.0, 40.0, MAX_RUNNING_DOT_SIZE] {
            let (rect, _) = hung_dot_box(icon, Side::Left, shape, size).expect("forme suspendue");
            let (top, height) = (rect[1], rect[3]);

            // Le bas ne bouge JAMAIS, et il pend SOUS le bas de l'icône —
            // DEMANDE EXPLICITE : « que l'indicateur ait sa partie basse 3
            // pixels plus bas que l'icone ».
            assert_eq!(top + height, icon_bottom + DOT_BOTTOM_OVERHANG, "{shape:?} : bas ancré, taille {size}");
            assert!(top + height > icon_bottom, "{shape:?} : l'indicateur pend bien SOUS l'icône");
            // Le haut, lui, remonte à chaque taille plus grande.
            if let Some(previous) = previous_top {
                assert!(top < previous, "{shape:?} : le haut remonte quand la taille augmente");
            }
            previous_top = Some(top);
        }
    }
}

/// **LE TEST DE LA DEMANDE** : « je ne veux pas que leur partie droite déborde
/// de la zone visible, ça crée un souci visuel quand les icônes proches sont
/// agrandies ». Pour chaque forme suspendue, chaque barre, chaque largeur
/// d'icône et toutes les tailles jusqu'au MAXIMUM, le bord EXTÉRIEUR de la
/// forme — **encombrement réel, rotation comprise** — ne dépasse jamais celui de
/// l'icône. C'est ce que l'ancrage par l'encombrement garantit, et ce que
/// l'ancien décalage proportionnel à la taille (`taille × 0.2`) ne garantissait
/// pas : sa moitié extérieure sortait de la colonne, d'autant plus que la forme
/// était grande.
#[test]
fn running_dot_never_leaves_its_icon_column() {
    for icon_w in [32.0_f32, 48.0, 96.0] {
        let icon = [100.0, 100.0, icon_w, icon_w];
        for shape in hung_shapes() {
            for size in [MIN_RUNNING_DOT_SIZE, 12.0, 40.0, 96.0, MAX_RUNNING_DOT_SIZE] {
                for side in [Side::Left, Side::Right] {
                    let (rect, _) = hung_dot_box(icon, side, shape, size).expect("forme suspendue");
                    let occupied = rect[2] * shape.footprint();
                    let centre = rect[0] + rect[2] * 0.5;
                    let (outer, icon_edge) = match side {
                        Side::Right => (centre - occupied * 0.5, icon[0]),
                        _ => (centre + occupied * 0.5, icon[0] + icon[2]),
                    };
                    assert!(
                        (outer - icon_edge).abs() < 1e-3,
                        "{shape:?} {side:?}, taille {size}, icône {icon_w} : bord extérieur {outer} au lieu de {icon_edge}"
                    );
                }
            }
        }
    }
}

/// Le côté extérieur est celui qui regarde la ZONE LIBRE : les deux barres se
/// répondent donc en miroir. Invariant fort et bon marché, qui interdit une
/// régression d'un seul côté — c'est exactement le défaut d'origine : « j'ai
/// déplacé une icône de programme dans la barre de droite et je vois que son
/// indicateur est à droite, il faudrait qu'il passe à gauche ».
#[test]
fn running_dot_mirrors_between_the_two_bars() {
    let total_width = 3840.0;
    let icon = [100.0, 100.0, 96.0, 96.0];
    let mirrored = [total_width - icon[0] - icon[2], icon[1], icon[2], icon[3]];

    for shape in hung_shapes() {
        for size in [MIN_RUNNING_DOT_SIZE, 24.0, MAX_RUNNING_DOT_SIZE] {
            let (left, left_radius) = hung_dot_box(icon, Side::Left, shape, size).expect("forme suspendue");
            let (right, right_radius) = hung_dot_box(mirrored, Side::Right, shape, size).expect("forme suspendue");
            let mirror_of_left = total_width - (left[0] + left[2]);
            assert!(
                (right[0] - mirror_of_left).abs() < 1e-3,
                "{shape:?}, taille {size} : {right:?} n'est pas le miroir de {left:?}"
            );
            assert_eq!(right[1], left[1], "{shape:?} : même hauteur");
            assert_eq!(right[3], left[3], "{shape:?} : même hauteur");
            assert_eq!(right_radius, left_radius, "{shape:?} : même rayon");
        }
    }
}

/// Le losange vaut la taille demandée par sa **diagonale**, pas par son côté :
/// c'est ce qui le fait tenir dans la colonne de l'icône malgré sa rotation,
/// puisqu'une boîte pivotée d'un quart de tour occupe `√2` fois son côté. Et
/// c'est la SEULE forme dans ce cas — les deux propriétés vont ensemble.
#[test]
fn diamond_reaches_the_requested_size_on_its_diagonal() {
    for size in [MIN_RUNNING_DOT_SIZE, 12.0, 40.0, MAX_RUNNING_DOT_SIZE] {
        let (w, h, _) = RunningDotShape::Diamond.hung_geometry(size, 96.0);
        assert!((w * RunningDotShape::Diamond.footprint() - size).abs() < 1e-3, "diagonale {w} pour {size}");
        assert!((h * RunningDotShape::Diamond.footprint() - size).abs() < 1e-3, "diagonale {h} pour {size}");
    }

    for shape in RunningDotShape::ALL {
        let is_diamond = shape == RunningDotShape::Diamond;
        assert_eq!(shape.rotation() != 0.0, is_diamond, "{shape:?} : rotation");
        assert_eq!((shape.footprint() - 1.0).abs() > 1e-6, is_diamond, "{shape:?} : encombrement");
    }
}

/// **Une seule forme ENTOURE l'icône** — garde-fou : une seconde forme `Around`
/// ajoutée sans traitement passerait par `hung_dot_box`, qui n'a pas de boîte
/// pour elle, et le `?` la ferait DISPARAÎTRE en silence au lieu de se voir.
#[test]
fn only_the_ring_goes_around() {
    let around: Vec<RunningDotShape> =
        RunningDotShape::ALL.into_iter().filter(|shape| shape.placement() == DotPlacement::Around).collect();
    assert_eq!(around, vec![RunningDotShape::Ring]);

    // Et l'anneau est bien la SEULE forme sans boîte suspendue : les deux
    // moitiés du contrat disent la même chose.
    for shape in RunningDotShape::ALL {
        assert_eq!(shape.geometry(12.0, 96.0).is_none(), shape == RunningDotShape::Ring, "{shape:?}");
    }
}

/// L'anneau est CONCENTRIQUE à l'icône — il l'entoure, il ne se pose pas à côté
/// — et son contour INTÉRIEUR épouse ses coins : c'est ce que garantit le rayon
/// `icon_radius() + épaisseur`, sans quoi les coins de l'icône apparaîtraient à nu
/// dans le cadre.
#[test]
fn ring_is_concentric_with_the_icon() {
    const GAP: f32 = 12.0;
    for icon in [[100.0_f32, 200.0, 96.0, 96.0], [0.0, 0.0, 48.0, 48.0], [0.0, 0.0, 64.0, 40.0]] {
        for size in [MIN_RUNNING_DOT_SIZE, 12.0, 40.0, MAX_RUNNING_DOT_SIZE] {
            let (rect, radius, thickness) = ring_box(icon, size, GAP);
            assert!((rect[0] + rect[2] * 0.5 - (icon[0] + icon[2] * 0.5)).abs() < 1e-3, "centre en X : {rect:?}");
            assert!((rect[1] + rect[3] * 0.5 - (icon[1] + icon[3] * 0.5)).abs() < 1e-3, "centre en Y : {rect:?}");
            assert!((rect[0] - (icon[0] - thickness)).abs() < 1e-3, "débord à gauche : {rect:?}");
            assert!((rect[2] - (icon[2] + 2.0 * thickness)).abs() < 1e-3, "largeur : {rect:?}");
            assert!((radius - thickness - icon_radius()).abs() < 1e-3, "rayon {radius}, épaisseur {thickness}");
        }
    }
}

/// L'épaisseur reste une ÉPAISSEUR : jamais nulle, jamais au-delà de la marge
/// haute de barre (sinon la bande supérieure de l'anneau de la première icône
/// d'une barre latérale serait coupée par `lateral_clip`), ni au-delà de
/// l'écartement des icônes (sinon elle mordrait sur la case voisine, où l'icône
/// texturée d'à côté la recouvrirait).
///
/// **Elle ne dépend ni de la taille NI de la forme de l'icône** : un plafond
/// proportionnel à l'icône ferait varier l'épaisseur pendant l'animation de
/// magnification, qui change la case présentée image par image. C'est pour cela
/// qu'il n'y en a pas — et c'est ce que la seconde boucle vérifie.
#[test]
fn ring_thickness_stays_a_ring() {
    const GAP: f32 = 12.0;
    for icon in [[0.0_f32, 0.0, 32.0, 32.0], [0.0, 0.0, 96.0, 96.0]] {
        for size in [MIN_RUNNING_DOT_SIZE, 12.0, 40.0, MAX_RUNNING_DOT_SIZE] {
            let thickness = ring_thickness(size, icon, GAP);
            assert!(thickness >= MIN_RING_THICKNESS, "épaisseur {thickness} à la taille {size}");
            assert!(thickness <= TOP_MARGIN, "la bande sortirait de la barre : {thickness}");
            assert!(thickness <= GAP, "elle mordrait sur l'icône voisine : {thickness}");
        }
    }

    // Insensible à la taille de l'icône : c'est ce qui la garde CONSTANTE
    // pendant que l'icône grossit (voir la doc).
    let small = [0.0, 0.0, 32.0, 32.0];
    let magnified = [0.0, 0.0, 80.0, 80.0];
    for size in [MIN_RUNNING_DOT_SIZE, 40.0, MAX_RUNNING_DOT_SIZE] {
        assert_eq!(ring_thickness(size, small, GAP), ring_thickness(size, magnified, GAP), "taille {size}");
    }
}

#[test]
fn l_indicateur_prend_l_accent_du_theme_avant_le_fond() {
    let fond = [0.1, 0.1, 0.1, 1.0];
    let accent = [0.9, 0.2, 0.5];
    let choisie = [0.0, 1.0, 0.0, 1.0];
    let clignote = [1.0, 0.0, 0.0, 1.0];
    assert_eq!(couleur_indicateur(None, None, None, fond), fond);
    assert_eq!(couleur_indicateur(None, None, Some(accent), fond), [0.9, 0.2, 0.5, 1.0]);
    assert_eq!(couleur_indicateur(None, Some(choisie), Some(accent), fond), choisie);
    assert_eq!(couleur_indicateur(Some(clignote), Some(choisie), Some(accent), fond), clignote);
}

/// Le CÂBLAGE de l'anneau — la seule partie que la géométrie pure ne couvre pas :
/// `running_dot_instance` doit emprunter `new_ring` (et non la boîte suspendue),
/// poser l'anneau sur la case PRÉSENTÉE de l'icône, et lui donner l'épaisseur
/// calculée.
#[test]
fn the_ring_wraps_the_icon_through_running_dot_instance() {
    // Disposition littérale : `geometry_layout` vit dans l'autre module de tests
    // de ce fichier, celui des géométries de la barre du haut.
    let layout =
        DockLayout { total_width: 3840.0, total_height: 2088.0, top_thickness: 72.0, haut_colonnes: 72.0, left_width: 140.0, right_width: 140.0, bottom_thickness: 0.0 };
    let mut dock = Dock::new([0.1, 0.2, 0.3, 1.0], 0);
    dock.set_running_dot_shape(RunningDotShape::Ring);
    dock.set_running_dot_size(20.0);

    let icon = dock.icon_rect(0, &layout).expect("l'icône du logo est dans la barre de gauche");
    let (expected, radius, thickness) = ring_box(icon, 20.0, dock.icon_gap());
    let instance = dock.running_dot_instance(0, &layout, None).expect("l'anneau doit être dessinable");

    assert_eq!(instance.ring, thickness, "l'épaisseur calculée est bien posée");
    assert!(instance.ring > 0.0, "un anneau a une épaisseur");
    assert_eq!(instance.radius, radius, "le rayon des coins");
    assert_eq!(instance.rect, expected, "la boîte entoure la case de l'icône");
    assert_eq!(instance.rotation, 0.0, "aucune rotation au repos (l'icône ne balance pas)");
    // Et c'est bien la forme qui ENTOURE : les formes suspendues, elles, ont
    // leur boîte sous l'icône (voir `hung_dot_box`).
    assert!(dock.running_dot_wraps_the_icon());
}

/// Les sept formes doivent être DISTINGUABLES — une forme dont la géométrie
/// serait identique à celle d'une autre donnerait un réglage sans effet visible,
/// « le pire des défauts ». Test GÉNÉRATIF sur `ALL`, et sur une signature POSÉE
/// plutôt que sur la seule géométrie : le carré et le losange partagent leur
/// boîte et leur rayon, et ne se distinguent que par leur rotation et leur
/// encombrement — la géométrie seule les aurait crus identiques.
#[test]
fn running_dot_shapes_are_all_distinguishable() {
    let signatures: Vec<(RunningDotShape, String)> = RunningDotShape::ALL
        .into_iter()
        .map(|shape| {
            let box_part = match shape.geometry(12.0, 96.0) {
                Some((w, h, radius)) => format!("boîte {w:.3}x{h:.3} rayon {radius:.3}"),
                None => String::from("sur la case de l'icône"),
            };
            let signature = format!(
                "{box_part} encombrement {:.3} rotation {:.3} {:?}",
                shape.footprint(),
                shape.rotation(),
                shape.placement()
            );
            (shape, signature)
        })
        .collect();

    for (i, (shape, signature)) in signatures.iter().enumerate() {
        for (other, other_signature) in signatures.iter().skip(i + 1) {
            assert_ne!(signature, other_signature, "{shape:?} et {other:?} sont indiscernables");
        }
    }
}

#[cfg(test)]
mod sonde_placement_applets_tests {
    use super::*;

    /// **SONDE — OÙ LE DOCK CROIT-IL QUE CHAQUE APPLET SE TROUVE ?**
    ///
    /// Écrite pour un défaut qui résiste au raisonnement : l'utilisateur voit
    /// l'applet d'ALIMENTATION **tout à gauche de la barre du haut**, et le fichier
    /// d'ordre la déclare sur la 4ᵉ ligne — qui est la moitié DROITE de cette barre
    /// (voir `config::load_icon_order`). L'un des deux ment, et cette sonde demande
    /// au dock lui-même, **avec l'ordre réel lu du disque**.
    ///
    /// ⚠️ **ELLE NE MODIFIE RIEN** : `load_icon_order` LIT le fichier de
    /// l'utilisateur, elle ne l'écrit pas. Aucune saisie, aucun clic.
    ///
    ///     cargo test --bin neuradesk sonde_ou_sont -- --ignored --nocapture
    #[test]
    #[ignore]
    fn sonde_ou_sont_les_applets() {
        let Some((left, right, top_left, top_right)) = crate::config::load_icon_order() else {
            panic!("aucun icon_order.txt : la sonde n'a rien à lire");
        };
        println!("ordre lu : left={} right={} top_left={} top_right={}", left.len(), right.len(), top_left.len(), top_right.len());
        // La disposition RÉELLE de la machine (3840x2088, voir la note DPI du
        // dépôt) : `geometry_layout()` est privée au module de tests voisin, et une
        // sonde qui mesurerait une autre géométrie ne dirait rien de ce qui est à
        // l'écran.
        let layout = DockLayout { total_width: 3840.0, total_height: 2088.0, top_thickness: 72.0, haut_colonnes: 72.0, left_width: 140.0, right_width: 140.0, bottom_thickness: 0.0 };
        let mut dock = Dock::new([0.0, 0.0, 1080.0, 1.0], 0);
        dock.set_icon_order(left, right, top_left, top_right);
        for index in 3..=18 {
            let cote = dock.icon_side(index);
            let rect = dock.icon_rect(index, &layout);
            let barre = cote.map(BarreTirage::de_la_side);
            println!("  icone {index:>2} : cote={cote:?} barre_de_tirage={barre:?} rect={rect:?}");
        }
    }

    // ---- LES PLAGES D'ICÔNES (étape 1 du refactor des identités stables) ----

    /// **`emplacement` DOIT RENDRE EXACTEMENT CE QUE `base + i` RENDAIT.** C'est
    /// tout l'enjeu de l'étape 1 : le refactor ne doit RIEN changer au
    /// comportement, sinon on ne saurait plus à quoi attribuer une régression.
    /// Voir `docs/plans/dock-identites-stables.md`.
    #[test]
    fn un_emplacement_est_la_base_plus_le_rang() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        // Rien n'est alloué au départ : aucune plage ne se résout, et c'est ce qui
        // distingue « pas encore garnie » de « garnie à la base 0 ».
        for plage in [Plage::WinPins, Plage::Dossiers, Plage::Notification, Plage::Dynamiques] {
            assert_eq!(dock.emplacement(plage, 0), None, "{plage:?} ne devait pas etre allouee");
        }

        let winpins = dock.ensure_winpin_capacity(2);
        for i in 0..2 {
            assert_eq!(dock.emplacement(Plage::WinPins, i), Some(winpins + i));
        }
        // ⚠️ **ET ELLE NE SE CONFOND PAS AVEC SA VOISINE** : la plage des dossiers
        // n'est toujours pas allouée, alors que `WinPins` l'est. Sans ce contrôle,
        // un `base_de` qui rendrait la mauvaise plage passerait inaperçu.
        assert_eq!(dock.emplacement(Plage::Dossiers, 0), None);

        let dossiers = dock.set_dossiers(&crate::config::ElementBarre::dossiers_de_test(3));
        assert_eq!(dock.emplacement(Plage::Dossiers, 0), Some(dossiers));
        assert_eq!(dock.emplacement(Plage::Dossiers, 3), Some(dossiers + 3), "le bouton « + » est le 4e");
    }

    /// Le débordement est REFUSÉ, là où `base + i` débordait en silence — c'est la
    /// seule différence assumée de l'étape 1, et elle va dans le bon sens.
    #[test]
    fn un_emplacement_hors_bornes_ne_deborde_pas() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.ensure_winpin_capacity(1);
        assert_eq!(dock.emplacement(Plage::WinPins, usize::MAX), None);
    }

    /// **LA BORNE DE FIN, ET SON CAS IMPOSSIBLE.** `plage_fin` remplace
    /// `base + compte` écrit à la main par l'appelant — dont le dock LUI-MÊME se
    /// servait pour savoir jusqu'où recréer des espaces.
    #[test]
    fn la_fin_d_une_plage_est_la_base_plus_le_compte() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let base = dock.ensure_winpin_capacity(3);
        assert_eq!(dock.plage_fin(Plage::WinPins), Some(base + 3));
        // ⚠️ **`Dynamiques` N'A PAS DE COMPTE** — elle se termine par un `truncate`
        // sur sa propre base, donc sa fin n'est pas une propriété de la plage. On le
        // DIT (`None`) plutôt que d'inventer un nombre : un appelant qui reçoit un
        // chiffre faux ne peut pas s'en apercevoir.
        assert_eq!(dock.plage_fin(Plage::Dynamiques), None);
    }

    /// **`rang_dans` EST LA GÉNÉRALISATION DE `dossier_slot`, ET ELLE APPORTE UNE
    /// CORRECTION** : les huit sites d'`app.rs` écrivaient `index - base` sans
    /// borner, donc un index HORS plage rendait un rang absurde — un clic sur une
    /// icône voisine aurait été lu comme « la n-ième épingle ». Ici, c'est `None`.
    #[test]
    fn un_rang_hors_plage_n_est_pas_un_rang() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let base = dock.ensure_winpin_capacity(3);
        assert_eq!(dock.rang_dans(Plage::WinPins, base), Some(0));
        assert_eq!(dock.rang_dans(Plage::WinPins, base + 2), Some(2), "le dernier est dans la plage");
        // AU-DELÀ de la plage : aujourd'hui `index - base` rendait `3`, un rang qui
        // n'existe pas — et `winpins.get(3)` rendait `None` par chance, ce qui
        // masquait le défaut sans le corriger.
        assert_eq!(dock.rang_dans(Plage::WinPins, base + 3), None);
        // ET AVANT la plage : soustraction négative, donc `None` — c'est ce que le
        // `checked_sub` protégeait déjà, mais seulement dans `dossier_slot`.
        assert_eq!(dock.rang_dans(Plage::WinPins, base.saturating_sub(1)), None);
    }

    // ---- L'INSERTION DANS UNE PLAGE (étape 3 du refactor) -----------------

    /// Un `Icon` banal, pour les tests d'insertion — seule la `label` distingue
    /// celui qu'on insère de ses voisins.
    fn icone_de_test(label: char) -> Icon {
        Icon {
            label,
            color: [0.0, 0.0, 0.0, 1.0],
            has_texture: false,
            external_texture: false,
            is_spacer: false,
            spacer_size: 0.0,
            zone_height: 0.0,
            zone_lead: 0.0,
            zone_columns: DEFAULT_ZONE_COLUMNS,
            zone_locked: false,
            zone_show_name: false,
            zone_permanent: false,
            hidden: false,
            sorte: SorteEspace::Rond,
        }
    }

    /// **LE DOCK DE L'ÉTAPE 3, DANS L'ORDRE D'ALLOCATION RÉEL** — et cet ordre EST la
    /// disposition des index (voir `docs/plans/dock-identites-stables.md`) : les
    /// épingles de la barre des tâches, puis les dossiers, puis le miroir de
    /// notification, puis les applications ouvertes. Chacune s'alloue à `icons.len()`
    /// la PREMIÈRE fois — c'est pourquoi l'ordre des appels décide de tout, et
    /// pourquoi une insertion doit décaler ce qui suit.
    fn dock_avec_les_quatre_plages() -> Dock {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.ensure_spacer_capacity(92);
        dock.ensure_winpin_capacity(3);
        dock.set_dossiers(&crate::config::ElementBarre::dossiers_de_test(2));
        dock.set_tray_count(3);
        dock.set_dynamic_apps(2);
        dock
    }

    /// **INSÉRER UNE ICÔNE AU RANG `rang` D'UNE PLAGE — LE CONTRAT COMPLET.**
    ///
    /// ⛔ **ÉCRIT AVANT `Dock::inserer`, ET C'EST DÉLIBÉRÉ** : c'est ce test qui décide
    /// la forme de la fonction. La lecture des deux mécanismes existants a montré
    /// qu'AUCUN DES DEUX ne suffit (voir l'étape 3 du plan) :
    /// - `insert_spacer` **INSÈRE**, mais son point d'insertion n'est pas un paramètre
    ///   (`base_non_persistable()`, donc AU-DESSUS de la plage des dossiers) ;
    /// - `decaler_bloc_epingles_dans` **DÉCALE**, mais borné, sans toucher `icons`, et
    ///   sans toucher `bottom_order`.
    ///
    /// **CE QUE CE TEST VERROUILLE, ET C'EST LA LISTE DES OUBLIS POSSIBLES** : la plage
    /// visée grandit d'un cran ; `bottom_order` — la **cinquième** liste, celle
    /// qu'`insert_spacer` ignore — cite la nouvelle venue à son rang ; les DEUX plages
    /// d'exécution du dessus voient leur base avancer ; les quatre ordres persistés
    /// glissent d'**exactement** un cran, sans doublon ni trou ; et la plage située
    /// SOUS l'insertion ne bouge pas.
    #[test]
    fn inserer_dans_une_plage_decale_tout_ce_qui_suit() {
        let mut dock = dock_avec_les_quatre_plages();
        let winpins = dock.plage_base(Plage::WinPins).expect("les epingles sont allouees");
        let dossiers = dock.dossiers_base().expect("les dossiers sont alloues");
        let tray = dock.tray_icon_base().expect("le miroir est alloue");
        let dynamic = dock.dynamic_icon_base().expect("les applications ouvertes sont allouees");

        let ordres_avant = [
            dock.order(Side::Left).to_vec(),
            dock.order(Side::Right).to_vec(),
            dock.order(Side::TopLeft).to_vec(),
            dock.order(Side::TopRight).to_vec(),
        ];
        let bas_avant = dock.order(Side::Bottom).to_vec();

        // ⚠️ **AU RANG 1, NI EN TÊTE NI EN QUEUE** : c'est la seule position où « tout
        // ce qui suit a glissé » se distingue de « rien n'a bougé » ET de « tout a
        // glissé ». Aux deux extrémités, un décalage oublié passerait pour un décalage
        // juste (même piège que le lâcher dont le départ est l'arrivée).
        let rang = 1;
        let index = dock.inserer(Plage::Dossiers, rang, icone_de_test('n')).expect("le rang 1 est dans les bornes");

        assert_eq!(index, dossiers + rang, "le rang {rang} de la plage tombe a la base + {rang}");
        assert_eq!(dock.icons[index].label, 'n', "et c'est bien l'icone inseree qui s'y trouve");

        // --- LA PLAGE VISÉE GRANDIT, SA PROPRE BASE NE BOUGE PAS --------------
        assert_eq!(dock.dossiers_base(), Some(dossiers), "inserer DANS une plage ne deplace pas sa propre base");
        // ⚠️ **`plage_fin` EST L'INDEX DU « + », PAS LE NOMBRE D'ICÔNES** : la plage
        // valait `base + 2` (deux dossiers, puis le bouton) et vaut maintenant
        // `base + 3` — le bouton a reculé d'un cran avec tout le reste. Compter les
        // ICÔNES (quatre) donnerait un cran de trop, et l'assertion passerait pour la
        // mauvaise raison si `inserer` en oubliait une.
        assert_eq!(
            dock.plage_fin(Plage::Dossiers),
            Some(dossiers + 3),
            "la fin de plage est l'index du « + », qui a recule d'un cran"
        );

        // --- LES DEUX PLAGES D'EXÉCUTION DU DESSUS SUIVENT --------------------
        assert_eq!(dock.tray_icon_base(), Some(tray + 1), "le miroir de notification est AU-DESSUS : il recule d'un cran");
        assert_eq!(dock.dynamic_icon_base(), Some(dynamic + 1), "les applications ouvertes aussi");

        // --- RIEN SOUS L'INSERTION NE BOUGE ----------------------------------
        assert_eq!(
            dock.plage_base(Plage::WinPins),
            Some(winpins),
            "les epingles de la barre des taches sont SOUS : elles ne bougent pas"
        );

        // --- LA CINQUIÈME LISTE : `bottom_order`, QUE PERSONNE NE DÉCALAIT -----
        let mut bas_attendu: Vec<usize> = bas_avant.iter().map(|&i| if i >= index { i + 1 } else { i }).collect();
        bas_attendu.insert(rang, index);
        assert_eq!(
            dock.order(Side::Bottom).to_vec(),
            bas_attendu,
            "la barre du bas doit citer la nouvelle icone a son rang et avoir decale le « + » (avant : {bas_avant:?})"
        );

        // --- LES QUATRE ORDRES PERSISTÉS, EXACTEMENT D'UN CRAN ----------------
        let noms = ["gauche", "droite", "haut-gauche", "haut-droite"];
        let ordres_apres = [
            dock.order(Side::Left).to_vec(),
            dock.order(Side::Right).to_vec(),
            dock.order(Side::TopLeft).to_vec(),
            dock.order(Side::TopRight).to_vec(),
        ];
        for ((nom, avant), apres) in noms.iter().zip(&ordres_avant).zip(&ordres_apres) {
            let attendu: Vec<usize> = avant.iter().map(|&i| if i >= index { i + 1 } else { i }).collect();
            assert_eq!(apres, &attendu, "{nom} : tout index >= {index} glisse d'UN cran, et RIEN d'autre");
        }
    }

    /// **INSÉRER AU PREMIER RANG NE DÉPLACE PAS LA BASE DE SA PROPRE PLAGE** — et c'est
    /// la seule raison pour laquelle la règle des bases est `base > at` et non
    /// `base >= at`.
    ///
    /// ⚠️ **`insert_spacer` FAIT L'INVERSE, ET C'EST JUSTE CHEZ LUI** : il décale les
    /// deux bases avec `>= at` **parce que chez lui `at` EST la plus petite base** —
    /// l'espace se pose à la frontière, donc la plage qui commence là recule. Ici, au
    /// rang 0, la nouvelle icône **prend la place de l'ancien rang 0** : la plage
    /// commence toujours au même index. Décaler sa base la ferait sauter d'un cran à
    /// chaque insertion en tête — exactement la classe de défaut que ce refactor
    /// existe pour supprimer.
    #[test]
    fn inserer_au_premier_rang_ne_deplace_pas_sa_propre_base() {
        let mut dock = dock_avec_les_quatre_plages();
        let dossiers = dock.dossiers_base().expect("les dossiers sont alloues");
        let tray = dock.tray_icon_base().expect("le miroir est alloue");

        let index = dock.inserer(Plage::Dossiers, 0, icone_de_test('n')).expect("le rang 0 est dans les bornes");

        assert_eq!(index, dossiers, "au rang 0, l'icone neuve prend l'index de la base");
        assert_eq!(dock.dossiers_base(), Some(dossiers), "et la base NE BOUGE PAS");
        assert_eq!(dock.tray_icon_base(), Some(tray + 1), "alors que le miroir, au-dessus, suit quand meme");
    }

    /// **LE RANG EST REFUSÉ HORS DES BORNES**, comme `emplacement` refuse le
    /// débordement — et une plage **sans compte** n'en accepte aucun.
    ///
    /// ⚠️ **L'ORDRE DES DEUX ASSERTIONS COMPTE** : le premier `inserer` qui RÉUSSIT fait
    /// grandir la plage d'un cran, donc le rang refusé juste après ne serait plus le
    /// même. On refuse d'abord, on accepte ensuite.
    #[test]
    fn inserer_hors_bornes_est_refuse() {
        let mut dock = dock_avec_les_quatre_plages();
        let dossiers = dock.dossiers_base().expect("les dossiers sont alloues");
        let dernier = dock.plage_fin(Plage::Dossiers).expect("la plage des dossiers a un compte") - dossiers;

        assert!(
            dock.inserer(Plage::Dossiers, dernier + 1, icone_de_test('n')).is_none(),
            "un cran de trop est refuse"
        );
        assert!(
            dock.inserer(Plage::Dossiers, dernier, icone_de_test('n')).is_some(),
            "le rang {dernier} est le dernier valide — insere un dossier en fin de liste, juste avant le « + »"
        );
        // ⚠️ **`Dynamiques` N'A PAS DE COMPTE** (`plage_fin` rend `None`) : sa fin n'est
        // pas une propriété de la plage, donc aucune insertion ne peut y être bornée.
        // On le REFUSE plutôt que d'inventer un nombre.
        assert!(
            dock.inserer(Plage::Dynamiques, 0, icone_de_test('n')).is_none(),
            "une plage sans compte n'accepte pas d'insertion"
        );
    }

    /// **L'ÉTAT COMPLET D'UN DOCK, POUR COMPARER AVANT ET APRÈS** — les cinq listes, les
    /// quatre bases et les quatre fins (donc les comptes).
    ///
    /// ⚠️ **COMPLET, ET PAS SEULEMENT « ce qui devrait bouger »** : c'est la comparaison
    /// partielle qui laisse passer le défaut. Un décalage oublié dans la liste qu'on n'a
    /// pas pensée à regarder est exactement ce que ce test doit attraper — et ce dépôt a
    /// déjà payé un « frère oublié » sur cette mécanique précise.
    fn etat_du_dock(dock: &Dock) -> (Vec<Vec<usize>>, [Option<usize>; 4], [Option<usize>; 4]) {
        let listes = vec![
            dock.order(Side::Left).to_vec(),
            dock.order(Side::Right).to_vec(),
            dock.order(Side::TopLeft).to_vec(),
            dock.order(Side::TopRight).to_vec(),
            dock.order(Side::Bottom).to_vec(),
        ];
        let plages = [Plage::WinPins, Plage::Dossiers, Plage::Notification, Plage::Dynamiques];
        let bases = plages.map(|p| dock.plage_base(p));
        let fins = plages.map(|p| dock.plage_fin(p));
        (listes, bases, fins)
    }

    /// **`retirer` DOIT ÊTRE L'INVERSE EXACT D'`inserer`.**
    ///
    /// ⛔ **POURQUOI CETTE PROPRIÉTÉ EST LE TEST, ET PAS UN DÉTAIL** : le désépinglage
    /// déplace, au rang près, **tout ce qui suit** — les cinq listes, les deux plages
    /// d'exécution, les comptes. Un décalage oublié d'un seul côté fait glisser une
    /// citation sur l'icône **voisine**, et ne se verrait qu'à l'usage suivant.
    /// L'aller-retour est la seule mesure qui couvre les six mécanismes d'un coup, et il
    /// n'a pas besoin de savoir lesquels existent.
    #[test]
    fn retirer_est_l_inverse_d_inserer() {
        let mut dock = dock_avec_les_quatre_plages();
        let avant = etat_du_dock(&dock);
        let icones_avant = dock.icons.len();

        // Au rang 1 : DANS la plage, ni en tête ni en queue — aux extrémités, un décalage
        // oublié passerait pour un décalage juste.
        let rang = 1;
        let index = dock.inserer(Plage::Dossiers, rang, icone_de_test('n')).expect("le rang 1 est dans les bornes");

        // PRÉMISSE : l'insertion a bien changé quelque chose, sinon le test se contenterait
        // de comparer deux états identiques.
        assert_ne!(etat_du_dock(&dock), avant, "prémisse : inserer a changé l'état");
        assert_eq!(dock.icons.len(), icones_avant + 1, "prémisse : une icône de plus");

        assert_eq!(dock.retirer(Plage::Dossiers, rang), Some(index), "retirer rend l'index libéré");

        assert_eq!(etat_du_dock(&dock), avant, "retirer doit rendre l'état EXACT d'avant l'insertion");
        assert_eq!(dock.icons.len(), icones_avant, "et l'icône a bien quitté `icons`");
    }

    /// **RETIRER AU PREMIER RANG NE DÉPLACE PAS LA BASE DE SA PROPRE PLAGE** — le miroir de
    /// `inserer_au_premier_rang_ne_deplace_pas_sa_propre_base`, et la même règle `> at`.
    #[test]
    fn retirer_au_premier_rang_ne_deplace_pas_sa_propre_base() {
        let mut dock = dock_avec_les_quatre_plages();
        let dossiers = dock.dossiers_base().expect("les dossiers sont alloues");
        let tray = dock.tray_icon_base().expect("le miroir est alloue");

        let index = dock.retirer(Plage::Dossiers, 0).expect("le rang 0 est retirable");

        assert_eq!(index, dossiers, "au rang 0, l'icône retirée est celle de la base");
        assert_eq!(dock.dossiers_base(), Some(dossiers), "et la base NE BOUGE PAS");
        assert_eq!(dock.tray_icon_base(), Some(tray - 1), "alors que le miroir, au-dessus, recule d'un cran");
    }

    /// **CE QUI N'EST PAS RETIRABLE, ET POURQUOI** — et la borne n'est PAS celle d'`inserer`.
    ///
    /// ⚠️ **`inserer` ACCEPTE `rang == fin - base`, `retirer` LE REFUSE** — et ce n'est pas
    /// une incohérence : pour la plage des dossiers, `fin` est l'index du bouton **« + »**,
    /// qui n'est pas un dossier. On peut donc insérer À SA PLACE (le nouveau passe avant
    /// lui), mais on ne peut pas le retirer — la barre du bas sans son « + » n'aurait plus
    /// de moyen d'ajouter un dossier.
    #[test]
    fn retirer_hors_bornes_est_refuse() {
        let mut dock = dock_avec_les_quatre_plages();
        let dossiers = dock.dossiers_base().expect("les dossiers sont alloues");
        // Deux dossiers : les rangs 0 et 1 sont retirables, le rang 2 est le « + ».
        assert_eq!(dock.plus_dossier_index(), Some(dossiers + 2), "prémisse : le « + » est le 3e");

        assert!(dock.retirer(Plage::Dossiers, 2).is_none(), "le bouton « + » ne se retire pas");

        assert!(dock.retirer(Plage::Dossiers, 1).is_some(), "le dernier DOSSIER, lui, se retire");
        assert_eq!(dock.plus_dossier_index(), Some(dossiers + 1), "et le « + » est revenu d'un cran");

        // `Dynamiques` n'a pas de compte (`plage_fin` rend `None`) : rien ne borne un
        // retrait, donc aucun n'est accepté.
        assert!(dock.retirer(Plage::Dynamiques, 0).is_none(), "une plage sans compte n'accepte pas de retrait");
    }

    /// **UN TRAIT PREND 8 PIXELS — ET NE LES REPREND PAS À CHAQUE CLIC.**
    ///
    /// DEMANDE EXPLICITE (2026-09-30) : « les nouveaux séparateurs trait n'ajoutent
    /// qu'un espace de 8 pixels ». Un espace ORDINAIRE doit son effet à son blanc ;
    /// un TRAIT *est* le séparateur, donc il n'a besoin que de la place de son trait.
    ///
    /// ⚠️ **La seconde assertion est celle qui compte** : re-cliquer « Trait » sur un
    /// espace DÉJÀ trait ne doit pas écraser une taille réglée au curseur. Deux
    /// commandes de la même grandeur ne peuvent pas se contredire en silence.
    #[test]
    fn un_trait_prend_huit_pixels_sans_ecraser_une_taille_reglee() {
        let mut dock = dock_avec_les_quatre_plages();
        let espace = dock.add_spacer_at_end(Side::Bottom);
        assert_eq!(dock.spacer_size(espace), Some(SPACER_HEIGHT), "prémisse : un espace neuf est ordinaire");

        dock.set_spacer_sorte(espace, SorteEspace::Trait);
        assert_eq!(dock.spacer_size(espace), Some(SPACER_TRAIT_PX), "devenir un trait donne la largeur du trait");

        // L'utilisateur règle la largeur au curseur, puis revient sur « Trait » —
        // c'est-à-dire reclique la sorte qui est DÉJÀ la sienne.
        dock.set_spacer_size(espace, 30.0);
        dock.set_spacer_sorte(espace, SorteEspace::Trait);
        assert_eq!(
            dock.spacer_size(espace),
            Some(30.0),
            "re-cliquer la MEME sorte ne doit pas ecraser une taille reglee a la main"
        );
    }

    /// **UN ESPACE PEUT SE POSER EN TÊTE DE BARRE** — la seule façon de précéder le
    /// premier dossier.
    ///
    /// DEMANDE EXPLICITE (2026-09-30) : « je ne peux pas mettre de séparateur avant
    /// le premier dossier ». `add_spacer_after` pose APRÈS l'icône visée et
    /// `add_spacer_at_end` n'ajoute qu'à la fin : la première icône d'une barre
    /// n'avait donc aucun moyen d'être précédée.
    #[test]
    fn un_espace_peut_se_poser_en_tete_de_barre() {
        let mut dock = dock_avec_les_quatre_plages();
        let premier = dock.order(Side::Bottom).first().copied().expect("la barre du bas a des emplacements");

        let espace = dock.add_spacer_avant(premier).expect("le premier emplacement est cite par une barre");

        assert_eq!(
            dock.order(Side::Bottom).first().copied(),
            Some(espace),
            "l'espace doit etre EN TETE : c'est ce que « avant le premier dossier » veut dire"
        );
        assert_eq!(
            dock.order(Side::Bottom).get(1).copied(),
            Some(premier),
            "et le premier emplacement recule d'un rang, sans disparaitre"
        );
    }

    /// **LA CASE NEUVE SE CITE À LA POSITION DEMANDÉE, PAS À LA FIN** — c'est tout
    /// l'intérêt de séparer la CASE de sa CITATION : la case se pose au bout du bloc
    /// (le moins coûteux), et c'est l'ordre qui décide où elle apparaît.
    #[test]
    fn une_case_de_separateur_neuve_se_cite_ou_on_veut() {
        let mut dock = dock_avec_les_quatre_plages();
        let avant = dock.order(Side::Bottom).len();

        let case = dock.ajouter_une_case_de_separateur();
        dock.citer_dans_la_barre_du_bas(0, case);

        let ordre = dock.order(Side::Bottom).to_vec();
        assert_eq!(
            ordre.first().copied(),
            Some(case),
            "la case neuve doit se citer EN TETE — c'est ce que « avant le premier dossier » veut dire"
        );
        assert_eq!(ordre.len(), avant + 1, "et la rangee compte un emplacement de plus");
        assert!(dock.is_spacer(case), "prémisse : la case est bien un espace");
        assert_ne!(dock.order(Side::Bottom).last().copied(), Some(case), "elle n'est PAS en fin de rangee");

        dock.remove_spacer(case);
        assert_eq!(dock.order(Side::Bottom).len(), avant, "et le retrait la fait partir");
    }

    /// **UNE DISPOSITION AVEC UNE VRAIE BARRE DU BAS** — `bottom_thickness` non
    /// nul, sans quoi il n'y a pas de rangée à mesurer. Les autres modules de
    /// tests ont la leur, qui la laisse à `0.0` : elle convient à ce qu'ils
    /// mesurent, pas à ceci.
    fn layout_avec_barre_du_bas() -> DockLayout {
        DockLayout {
            total_width: 3840.0,
            total_height: 2088.0,
            top_thickness: 72.0, haut_colonnes: 72.0,
            left_width: 140.0,
            right_width: 140.0,
            bottom_thickness: DEFAULT_BOTTOM_THICKNESS,
        }
    }

    /// **BARRE DU HAUT MASQUÉE : LES COLONNES MONTENT** (DEMANDE EXPLICITE,
    /// 2026-10-03). Leur fond, leur découpe et leur zone de clic commencent au bord
    /// de l'écran ; la barre du haut, elle, garde son épaisseur.
    #[test]
    fn la_barre_du_haut_masquee_rend_son_espace_aux_colonnes() {
        let normal = layout_avec_barre_du_bas();
        let masquee = DockLayout { haut_colonnes: 0.0, ..normal };
        let dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        assert_eq!(bar_rects(&normal)[1][1], 72.0, "d'ordinaire, la colonne gauche commence sous la barre du haut");
        assert_eq!(bar_rects(&masquee)[1][1], 0.0, "masquée, elle part du bord");
        assert_eq!(bar_rects(&masquee)[2][3], masquee.total_height, "et la droite gagne toute la hauteur");
        assert_eq!(bar_rects(&masquee)[0][3], 72.0, "la barre du haut garde son épaisseur");
        assert_eq!(Dock::lateral_clip(&masquee)[1], 0.0, "la découpe des colonnes suit");
        assert_eq!(dock.side_at(20.0, 30.0, &normal), None, "le coin est à la barre du haut");
        assert_eq!(dock.side_at(20.0, 30.0, &masquee), Some(Side::Left), "masquée, il est à la colonne");
    }

    /// **UN ESPACE ÉLASTIQUE ABSORBE CE QUI RESTE — ET LES AUTRES GARDENT LEUR
    /// TAILLE.**
    ///
    /// Les deux moitiés comptent, et la seconde autant que la première : un
    /// élastique qui prendrait la place **sans** que les autres conservent la leur
    /// signifierait que le calcul a débordé sur le fixe. On mesure donc les deux.
    ///
    /// ⚠️ **C'est la première fois que la largeur de la bande entre dans
    /// `layout_bottom_side`** : elle existait déjà pour `bar_rects`, mais la
    /// rangée du bas l'ignorait et laissait le reste inutilisé (voir sa doc).
    #[test]
    fn un_espace_elastique_absorbe_la_largeur_restante() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.ensure_spacer_capacity(10);
        // ⚠️ **UN SEPARATEUR EST UN EMPLACEMENT, IL SE DECLARE DANS LA LISTE** :
        // `add_spacer_at_end` ne fait que citer un index dans `bottom_order`, et
        // `set_dossiers` reconstruit cette liste DEPUIS les elements — un espace
        // ajoute avant serait donc simplement efface.
        let mut elements = crate::config::ElementBarre::dossiers_de_test(1);
        elements.push(crate::config::ElementBarre::Separateur(crate::ui::zones::SorteEspace::Rond));
        elements.push(crate::config::ElementBarre::Separateur(crate::ui::zones::SorteEspace::Rond));
        dock.set_dossiers(&elements);
        // Les separateurs vivent A LA SUITE de la plage (voir `set_dossiers`).
        // ⚠️ **LE PREMIER SEPARATEUR EST A LA SUITE DE LA PLAGE** : `plage_fin(Dossiers)`
        // est l'index du bouton « + », PAS celui du premier separateur — ils vivent
        // a `base + total`, un cran plus loin.
        let rond = dock.plus_dossier_index().expect("la plage est allouee") + 1;
        let elastique = rond + 1;
        dock.set_spacer_sorte(elastique, SorteEspace::Elastique);

        // ⚠️ **`bottom_thickness` NON NUL, ET C'EST LE POINT** : `geometry_layout`
        // des autres modules de tests la laisse à `0.0` — une barre du bas de
        // hauteur nulle, où rien de ce test ne serait mesurable.
        let layout = layout_avec_barre_du_bas();
        let largeur_rond = dock.icon_rect(rond, &layout).expect("le rond a un emplacement")[2];
        let largeur_elastique = dock.icon_rect(elastique, &layout).expect("l'elastique a un emplacement")[2];

        assert_eq!(
            largeur_rond,
            dock.spacer_size(rond).expect("un espace a une taille"),
            "un espace ROND doit garder sa taille — sinon l'elastique a mange le fixe"
        );
        assert!(
            largeur_elastique > largeur_rond * 10.0,
            "l'elastique doit absorber ce qui reste : {largeur_elastique} px contre {largeur_rond} px \
             pour un espace ordinaire, dans une bande de {} px",
            bottom_band_width(&layout)
        );
    }

    /// **INSÉRER UN SÉPARATEUR NE CHANGE PLUS LA SORTE DES AUTRES — MESURÉ.**
    ///
    /// La sorte vivait dans `[bars.B] sortes`, une liste **PAR RANG** : insérer un
    /// séparateur décalait tous les rangs, donc **chaque séparateur héritait de la sorte
    /// de son voisin** au redémarrage suivant.
    ///
    /// Défaut signalé : « j'ai ajouté des séparateurs mais je ne vois pas les points leur
    /// correspondant » — ils étaient devenus **ÉLASTIQUES**, une sorte qui ne dessine
    /// RIEN (voir `spacer_instances`), en héritant du rang d'à côté.
    ///
    /// La sorte voyage désormais **avec l'élément** (voir
    /// `config::ElementBarre::Separateur`). Sans ce correctif, `set_dossiers` ne pose
    /// aucune sorte et la prémisse tombe (`Rond`, `Rond`).
    #[test]
    fn inserer_un_separateur_ne_change_pas_la_sorte_des_autres() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.ensure_spacer_capacity(10);
        let sortes = |dock: &Dock| -> Vec<Option<SorteEspace>> {
            dock.order(Side::Bottom)
                .iter()
                .copied()
                .filter(|&index| dock.is_spacer(index))
                .map(|index| dock.spacer_sorte(index))
                .collect()
        };

        // DEUX séparateurs : le premier ÉLASTIQUE, le second ROND.
        let mut elements = crate::config::ElementBarre::dossiers_de_test(1);
        elements.push(crate::config::ElementBarre::Separateur(SorteEspace::Elastique));
        elements.push(crate::config::ElementBarre::Separateur(SorteEspace::Rond));
        dock.set_dossiers(&elements);
        assert_eq!(
            sortes(&dock),
            vec![Some(SorteEspace::Elastique), Some(SorteEspace::Rond)],
            "prémisse : chaque séparateur porte SA sorte"
        );

        // ON EN INSÈRE UN TROISIÈME EN TÊTE — le geste décale tous les rangs.
        elements.insert(0, crate::config::ElementBarre::Separateur(SorteEspace::Rond));
        dock.set_dossiers(&elements);

        assert_eq!(
            sortes(&dock),
            vec![Some(SorteEspace::Rond), Some(SorteEspace::Elastique), Some(SorteEspace::Rond)],
            "l'ÉLASTIQUE doit rester élastique malgré l'insertion en tête"
        );
    }

    /// **INSÉRER UNE CASE NE DOIT PAS LA FAIRE CITER PAR UNE AUTRE BARRE — MESURÉ.**
    ///
    /// `icons.insert` décale d'un cran tout ce qui vit à `at` ou au-delà ; les DEUX BASES
    /// en tenaient compte, les listes d'ORDRE non. Une barre latérale qui citait un index
    /// ≥ `at` se mettait donc à citer **la case neuve** : l'espace devenait cité par deux
    /// barres, `icon_side` répondait par la première de sa chaîne, et `is_locked_at`
    /// consultait le verrou de CETTE barre.
    ///
    /// Défaut signalé : « je ne peux pas **tout le temps** cliquer sur les points de
    /// séparateur de gauche ». Le journal portait `cite_par=[Right, Bottom]`,
    /// `barre=Some(Right)`, `verrou=true` — un clic fait dans la barre du BAS, refusé par
    /// le verrou de celle de DROITE. « Pas tout le temps », parce que seuls les espaces
    /// doublement cités l'étaient.
    ///
    /// ⚠️ **LE BLOC APRÈS LA PLAGE EST INDISPENSABLE AU TEST** : sans lui, le point
    /// d'insertion tombe à la FIN de `icons`, rien ne glisse, et le défaut ne se voit pas
    /// — c'est ce qui rendait le geste apparemment inoffensif.
    #[test]
    fn une_case_de_separateur_neuve_n_est_citee_par_aucune_autre_barre() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.ensure_spacer_capacity(10);
        let mut elements = crate::config::ElementBarre::dossiers_de_test(2);
        elements.push(crate::config::ElementBarre::Separateur(crate::ui::zones::SorteEspace::Rond));
        dock.set_dossiers(&elements);
        // ⚠️ **UN BLOC APRÈS LA PLAGE DU BAS EST INDISPENSABLE** : sans lui, l'insertion
        // tombe à la FIN de `icons`, rien ne glisse, et le défaut ne peut pas se
        // produire — c'est précisément pourquoi le geste paraissait inoffensif. Le
        // miroir de notification est, dans l'application réelle, ce qui vit là.
        dock.set_tray_count(3);
        let cite = dock.icons.len() - 1;
        dock.set_icon_order(Vec::new(), vec![cite], Vec::new(), Vec::new());

        let avant = dock.icons.len();
        let case = dock.ajouter_une_case_de_separateur();
        assert!(
            cite >= case,
            "prémisse : l'index cite {cite} (dernier avant = {avant}) doit etre au-dessus du point d'insertion {case}"
        );
        assert!(
            dock.barres_qui_citent(case).is_empty(),
            "la case neuve {case} est citee par {:?} — l'insertion a fait glisser un index sur elle",
            dock.barres_qui_citent(case)
        );
        assert!(
            dock.order(Side::Right).contains(&(cite + 1)),
            "l'index cite par la DROITE doit avoir glisse d'un cran : {:?}",
            dock.order(Side::Right)
        );

        dock.citer_dans_la_barre_du_bas(0, case);
        assert_eq!(
            dock.barres_qui_citent(case),
            vec![Side::Bottom],
            "une fois citee, la case n'appartient QU'AU BAS"
        );
    }

    /// **LA RANGÉE FINIT AU BORD DE LA BANDE, PAS `TOP_ZONE_CORNER_MARGIN` PLUS LOIN.**
    ///
    /// ⚠️ **MESURÉ, ET C'EST CE QUI A ÉTÉ SIGNALÉ** : « les éléments vont plus loin que
    /// l'extrémité de droite dans la barre du bas ». La rangée est ANCRÉE à
    /// `layout.left_width + TOP_ZONE_CORNER_MARGIN` (voir `layout_zone`) alors que les
    /// élastiques absorbaient `bottom_band_width` ENTIER : leur part valait donc
    /// `band_width - fixe`, et la rangée finissait `margin` pixels **sur la barre de
    /// DROITE**. Le contrôle de débordement ne le voyait pas — il comparait `fixe`, qui
    /// compte les élastiques pour ZÉRO (voir la passe 1 de `layout_bottom_side`).
    ///
    /// Sans le correctif, le bord droit du « + » tombe à `bord + marge - gap`, donc
    /// au-delà du bord dès que `gap < marge` : ce test tombe.
    #[test]
    fn la_rangee_du_bas_finit_au_bord_de_la_bande_meme_avec_un_elastique() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.ensure_spacer_capacity(10);
        let mut elements = crate::config::ElementBarre::dossiers_de_test(1);
        elements.push(crate::config::ElementBarre::Separateur(crate::ui::zones::SorteEspace::Rond));
        dock.set_dossiers(&elements);
        let elastique = dock.plus_dossier_index().expect("la plage est allouee") + 1;
        dock.set_spacer_sorte(elastique, SorteEspace::Elastique);

        let layout = layout_avec_barre_du_bas();
        // Le « + » FERME la rangée (voir `set_dossiers`) : c'est donc son bord droit
        // qui dit où la rangée s'arrête.
        let plus = dock.plus_dossier_index().expect("la plage est allouee");
        let rect = dock.icon_rect(plus, &layout).expect("le + a un emplacement");
        let bord_droit_bande = layout.total_width - layout.right_width;
        // ⚠️ **LA MÊME MARGE DES DEUX CÔTÉS** — DEMANDE EXPLICITE (2026-10-01) : la
        // rangée finissait EXACTEMENT sur le bord droit, son seul souffle étant le
        // `gap` après le dernier élément, et le « + » s'y lisait plaqué (« il dépasse
        // de la barre du bas »). Elle doit donc garder à droite ce que l'ancre lui
        // donne à gauche.
        assert!(
            rect[0] + rect[2] <= bord_droit_bande - TOP_ZONE_CORNER_MARGIN + 0.5,
            "la rangee ne garde pas sa marge a DROITE : elle finit a {} pour un bord a {bord_droit_bande} \
             moins la marge {TOP_ZONE_CORNER_MARGIN} (ancre {}, bande {} px)",
            rect[0] + rect[2],
            layout.left_width + TOP_ZONE_CORNER_MARGIN,
            bottom_band_width(&layout),
        );

        // ⛔ **ET LA RANGÉE VAUT EXACTEMENT `disponible`** — c'est ce que la passe 1
        // rate quand elle saute le `gap` des élastiques : la rangée vaut alors
        // `disponible + elastiques × gap`, et le dernier élément sort de la barre.
        // MESURÉ le 2026-10-01 : le « + » peint à 3726 pour un bord à 3740.
        //
        // Le bord droit PLUS la séparation doit donc tomber PILE sur la fin de la zone
        // utile — l'assertion précédente seule ne suffisait pas : avec un seul élastique
        // et un `gap` modeste, le débordement restait sous sa tolérance.
        let fin_utile = layout.left_width
            + TOP_ZONE_CORNER_MARGIN
            + (bottom_band_width(&layout) - TOP_ZONE_CORNER_MARGIN * 2.0);
        assert!(
            (rect[0] + rect[2] + dock.icon_gap - fin_utile).abs() < 0.5,
            "la rangee ne vaut pas `disponible` : bord droit {} (+ gap = {}) pour une fin utile a {fin_utile}",
            rect[0] + rect[2],
            rect[0] + rect[2] + dock.icon_gap,
        );
    }

    /// **UN ÉLASTIQUE SANS PLACE VAUT ZÉRO, PAS UNE LARGEUR NÉGATIVE.** Une bande
    /// plus étroite que son contenu fixe est un cas réel (barre rétrécie, écran
    /// étroit) : un rectangle négatif se dessinerait **à l'envers**, et rien ne le
    /// signalerait.
    #[test]
    fn un_espace_elastique_sans_place_vaut_zero() {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        dock.ensure_spacer_capacity(10);
        let mut elements = crate::config::ElementBarre::dossiers_de_test(3);
        elements.push(crate::config::ElementBarre::Separateur(crate::ui::zones::SorteEspace::Rond));
        dock.set_dossiers(&elements);
        let elastique = dock.plus_dossier_index().expect("la plage est allouee") + 1;
        dock.set_spacer_sorte(elastique, SorteEspace::Elastique);

        // Une bande MINUSCULE : le fixe la dépasse largement.
        let mut layout = layout_avec_barre_du_bas();
        layout.total_width = layout.left_width + layout.right_width + 1.0;

        let rect = dock.icon_rect(elastique, &layout).expect("l'elastique a un emplacement");
        assert!(rect[2] >= 0.0, "la largeur ne peut pas etre negative : {}", rect[2]);
    }

    /// **UN SÉPARATEUR DE LA BARRE DU BAS SURVIT À `set_dossiers`** — et c'est le
    /// défaut MESURÉ sur la configuration de la machine, pas une hypothèse.
    ///
    /// `zones.toml` y déclare `[bars.B] spacers = 1, sizes = [180.0]`, et la barre
    /// n'en montre AUCUN. La chaîne : `compléter_les_barres` crée le séparateur au
    /// démarrage (`app.rs:3707`), `apply_zones` lui donne sa taille (`3734`), puis
    /// `set_dossiers` (`3852`) **effaçait sa citation**. Son icône restait dans
    /// `icons`, citée par aucun ordre — jamais disposée, jamais dessinée — et le
    /// lancement suivant en créait un AUTRE.
    ///
    /// ⚠️ **Le séparateur ne vit pas dans la plage** : `insert_spacer` le pose à la
    /// frontière persistable, donc sous elle dans `icons`. Sa seule appartenance à
    /// la barre est sa citation dans `bottom_order`, et c'est elle que la
    /// reconstruction doit reprendre.
    #[test]
    fn la_rangee_suit_l_arrangement_et_le_plus_reste_le_dernier() {
        use crate::config::ElementBarre;

        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        // ⛔ **LA POSITION D'UN SÉPARATEUR SE DÉCLARE, ELLE NE SE DEVINE PAS.** Le
        // défaut mesuré était qu'un séparateur posé EN TÊTE revenait EN FIN de
        // rangée au redémarrage : il n'existait nulle part où écrire sa place
        // (voir `config::ElementBarre`). Ici il est entre les deux dossiers, et il
        // doit y rester.
        let mut elements = ElementBarre::dossiers_de_test(1);
        elements.push(ElementBarre::Separateur(SorteEspace::Rond));
        elements.extend(ElementBarre::dossiers_de_test(1));
        dock.set_dossiers(&elements);

        let ordre = dock.order(Side::Bottom).to_vec();
        let base = dock.dossiers_base().expect("la plage est allouee");
        let plus = dock.plus_dossier_index().expect("le « + »");
        let separateur = plus + 1;
        assert_eq!(
            ordre,
            vec![base, separateur, base + 1, plus],
            "la rangee doit suivre l'ARRANGEMENT — dossier, separateur, dossier, puis le « + » en \
             DERNIER (le poser ailleurs ferait diverger `plus_dossier_index`, qui est arithmetique)"
        );
        // ... et le SEPARATEUR n'est pas range a la fin : c'est TOUTE la difference
        // entre « une place ecrite » et « un ajout a la suite ».
        assert_ne!(ordre.get(1).copied(), Some(plus), "le separateur ne doit pas etre repousse avant le « + »");
    }

    /// **UN ESPACE SE RETIRE DE CHACUNE DES CINQ BARRES** — une boucle sur TOUTES,
    /// jamais sur celles qu'on a pensé à écrire.
    ///
    /// ⛔ **DÉFAUT SIGNALÉ, ET C'ÉTAIT LA DEUXIÈME FOIS** : « j'ai ajouté un
    /// séparateur dans la barre du bas mais la fonction de suppression ne le retire
    /// pas ». `remove_spacer` retirait l'index de quatre listes **écrites à la
    /// main** ; `bottom_order` n'y était pas. La première fois, c'étaient les barres
    /// du **haut** qui manquaient (« je n'arrive plus à retirer des séparateurs que
    /// j'ai ajouté dans les barres du haut ») — on avait alors rallongé la liste, ce
    /// qui n'a fait que reculer le problème d'une barre.
    ///
    /// ⇒ Le remède n'est pas la ligne ajoutée mais **`orders_mut`** (voir sa doc) :
    /// il n'y a plus de liste à rallonger. **ET CE TEST EST LA GARDE QUI MANQUAIT** —
    /// il boucle sur les cinq barres, donc une sixième oubliée dans `orders_mut`
    /// échouerait ici.
    ///
    /// ⚠️ Le contrôle final porte sur les **cinq**, et pas seulement sur celle où
    /// l'espace a été posé : c'est ce qui attrape l'oubli quelle que soit la barre.
    #[test]
    fn un_espace_se_retire_de_chacune_des_cinq_barres() {
        const BARRES: [Side; 5] = [Side::Left, Side::Right, Side::TopLeft, Side::TopRight, Side::Bottom];
        for side in BARRES {
            let mut dock = dock_avec_les_quatre_plages();
            let espace = dock.add_spacer_at_end(side);
            assert!(dock.is_spacer(espace), "prémisse : {side:?} — l'espace posé est bien un espace");
            assert!(dock.order(side).contains(&espace), "prémisse : {side:?} — il est cité par cette barre");

            dock.remove_spacer(espace);

            for barre in BARRES {
                assert!(
                    !dock.order(barre).contains(&espace),
                    "un espace posé dans {side:?} est encore cité par {barre:?} après `remove_spacer` — \
                     il reste DESSINÉ, et le geste de retrait ne fait rien (défaut signalé deux fois)"
                );
            }
        }
    }
}
