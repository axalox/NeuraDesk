//! Matériaux de flou du panneau de réglages ("Mica", "Givré (Kawase pur)",
//! "Acrylique", "Verre liquide", "Frutiger Aero") — remplace l'ancien axe
//! "Style de flou" de `gfx::blur` (`STYLE_PRESETS`, net ↔ diffus sur un seul
//! algorithme). Deux couches indépendantes composent un matériau :
//!
//! - **Couche A — [`BlurAlgo`]**, consommée par `gfx::blur` : la FORME du
//!   graphe de passes GPU qui construit la pyramide de flou. Deux
//!   algorithmes seulement (voir sa doc) — la contrainte des barres du dock
//!   à 72px d'épaisseur physique (voir `gfx::blur::MAX_STOP_LEVELS_HARD_CAP`,
//!   dont la doc reste la référence pour cette limite) ne laisse de la place
//!   que pour DEUX graphes réellement distinguables sur cette surface.
//! - **Couche B — [`PostParams`]**, consommée par `gfx::quad_renderer`
//!   (`PSMain`, `TexMode::Backdrop`) : la post-composition (grain, liseré,
//!   bandeau glossy, saturation, teinte) appliquée à la résolution NATIVE
//!   d'affichage, après ré-étirement du flou — voir la doc de `DitherHash`
//!   dans `quad_renderer` pour pourquoi ce travail ne peut PAS se faire dans
//!   `gfx::blur` (résolution de la pyramide trop petite, coins arrondis/
//!   taille d'affichage inconnus de ce module).
//!
//! L'identité visuelle des 5 matériaux vient à ~85% de la couche B — un seul
//! ([`Material::Mica`]... voir [`MATERIALS`]) justifie un chemin de flou
//! réellement différent (couche A). Ce n'est PAS "5 algorithmes de flou
//! distincts" : c'est 2 algorithmes de flou × 5 réglages de composition.

use std::cell::Cell;

/// Forme du graphe de passes GPU utilisé pour construire la pyramide de
/// flou — voir `gfx::blur::render_backdrop`/`build_pyramid`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlurAlgo {
    /// Descente (`PSDown`) puis remontée Kawase complète (`PSUp`) — voir la
    /// doc de module de `gfx::blur`. L'algorithme historique, utilisé par 4
    /// des 5 matériaux.
    DualKawase,
    /// Descente identique à `DualKawase`, puis UNE SEULE passe de tente
    /// large (`PSTent`, voir `gfx::blur::SHADER_SOURCE`) au lieu de la
    /// remontée Kawase complète — reproduit l'aspect plat et laiteux du
    /// matériau Mica de Windows ("ne floute l'image qu'une fois", contre le
    /// flou "intense" et structuré d'Acrylic — voir la recherche qui a
    /// motivé ce module). Moins cher que `DualKawase` (une passe au lieu de
    /// plusieurs pour la remontée).
    MicaTent,
}

/// Paramètres de POST-COMPOSITION, lus par `gfx::quad_renderer::PSMain`
/// (`TexMode::Backdrop`) — PAS par `gfx::blur` (voir la doc de module).
#[derive(Clone, Copy, Debug)]
pub struct PostParams {
    /// Amplitude du grain de dithering, en unités de `1/255` de canal
    /// couleur — jamais en dessous de [`MIN_NOISE`] (voir sa doc), quel que
    /// soit le matériau : son rôle anti-banding d'origine
    /// (`gfx::quad_renderer::DITHER_AMOUNT`) reste nécessaire même sur les
    /// matériaux qui n'ont pas vocation à "avoir du grain".
    pub noise: f32,
    /// Force du liseré spéculaire sur le bord intérieur (0.0 = aucun).
    pub rim: f32,
    /// Force du bandeau lumineux en haut du quad (0.0 = aucun).
    pub gloss: f32,
    /// Multiplicateur de saturation (1.0 = neutre, >1.0 = plus vif).
    pub saturation: f32,
    /// Multiplicateur BORNÉ (`saturate()` côté shader) de `Color.a`, la
    /// force de mélange avec la teinte de l'utilisateur — voir
    /// `QuadInstance::new_backdrop`. Appliqué UNIQUEMENT dans le shader :
    /// ne doit JAMAIS être écrit dans la config de teinte persistée
    /// (`bar_tint`/`dash_tint`/`menu_tint`), sous peine de détruire
    /// irréversiblement les réglages de transparence choisis par
    /// l'utilisateur au premier changement de matériau.
    pub tint_scale: f32,
    /// Décalage de réfraction de bord, en pixels — pousse l'UV échantillonné
    /// vers l'extérieur près du contour (effet de lentille de bord de
    /// verre). 0.0 = aucune réfraction.
    pub refraction: f32,
    /// Biais de teinte RGB multiplicatif appliqué après la saturation
    /// (1.0, 1.0, 1.0 = neutre) — sert au biais bleu-cyan de Frutiger Aero.
    pub hue_bias: [f32; 3],
}

/// Un matériau de flou proposé dans le panneau de réglages (sélecteur
/// "Matériau", voir `assets/popups/settings.html`).
pub struct Material {
    pub label: &'static str,
    pub algo: BlurAlgo,
    /// Nombre de passes de remontée Kawase réellement effectuées (voir
    /// `gfx::blur::build_pyramid`, `stop = deepest - stop_levels`). Sens
    /// CONFIRMÉ À L'ŒIL par l'utilisateur (un premier calcul, avant
    /// vérification, avait conclu l'inverse) : PLUS `stop_levels` est GRAND,
    /// PLUS de remontée Kawase tourne — chaque passe de remontée (`PSUp`)
    /// est ELLE-MÊME un filtre de flou, donc plus de remontée accumule PLUS
    /// de flou réel = un rendu plus DIFFUS. `0` = aucune remontée, le
    /// résultat renvoyé reste la texture la plus petite de la pyramide,
    /// simplement agrandie par le filtrage bilinéaire du compositeur = un
    /// rendu plus NET. Ignoré si `algo == BlurAlgo::MicaTent`
    /// (`gfx::blur::build_pyramid` calcule alors son propre point d'arrêt).
    /// Ne dépasse jamais 3 : au-delà, les barres du dock (72px d'épaisseur
    /// physique, pyramide limitée à 4 niveaux) ne peuvent plus distinguer un
    /// palier de plus — voir l'ancienne doc de
    /// `gfx::blur::MAX_STOP_LEVELS_HARD_CAP` pour le bug réel déjà rencontré
    /// en assouplissant cette borne.
    pub stop_levels: usize,
    pub post: PostParams,
}

/// Plancher de grain, quel que soit le matériau choisi — voir
/// `PostParams::noise`. Reprend le rôle anti-banding d'origine de l'ancienne
/// constante `gfx::quad_renderer::DITHER_AMOUNT` — PAS réglable par
/// l'utilisateur (essayé un temps comme curseur "Style de flou", voir
/// l'historique : un simple bruit ajouté "ressemblait à une couche posée
/// dessus", pas à un changement de l'aspect du flou lui-même, d'où ce
/// module).
///
/// BUG RÉEL CONSTATÉ EN PRATIQUE (voir l'historique, "le flou semble
/// bruité... de moins bonne qualité") : le constant buffer qui porte cette
/// valeur n'était lié qu'au vertex shader jusqu'à récemment
/// (`PSSetConstantBuffers` manquant côté PS, voir `gfx::quad_renderer::
/// QuadRenderer::render`) — `6.0/255.0` avait été choisi À L'AVEUGLE, sans
/// jamais voir son effet réel une fois `PSMain` effectivement alimenté. Une
/// fois le bug corrigé, cette amplitude s'est révélée bien trop forte : un
/// grain nettement visible plutôt qu'un simple correctif de banding
/// discret. Ramené à cette valeur, réellement subtile pour son rôle
/// d'origine (casser le banding sur un dégradé lisse, même technique que le
/// `dash_noise.png` d'Unity ou le bruit du matériau "Acrylic" de Windows,
/// voir `gfx::quad_renderer::DitherHash`) sans se voir comme du "bruit"
/// ajouté.
pub const MIN_NOISE: f32 = 1.0 / 255.0;

/// Les cinq matériaux, dans l'ordre d'affichage du sélecteur. Défaut =
/// [`DEFAULT_MATERIAL_INDEX`] ("Acrylique") : le rendu le plus proche de
/// l'ancien défaut "Équilibré" + le dither déjà présent (`gfx::
/// quad_renderer` le décrit littéralement comme "le bruit du matériau
/// Acrylic de Windows") — aucun saut visuel pour un utilisateur existant au
/// premier lancement après cette mise à jour. Son `stop_levels` a depuis
/// été écarté de celui de "Givré" (voir sa doc) pour rester distinguable.
pub const MATERIALS: [Material; 5] = [
    Material {
        label: "Mica",
        algo: BlurAlgo::MicaTent,
        stop_levels: 0, // ignoré par MicaTent, voir la doc du champ
        post: PostParams {
            noise: MIN_NOISE,
            rim: 0.0,
            gloss: 0.0,
            saturation: 0.92,
            tint_scale: 1.12,
            refraction: 0.0,
            hue_bias: [1.0, 1.0, 1.0],
        },
    },
    Material {
        label: "Givré (Kawase pur)",
        algo: BlurAlgo::DualKawase,
        stop_levels: 3,
        post: PostParams {
            noise: MIN_NOISE,
            rim: 0.0,
            gloss: 0.0,
            saturation: 1.0,
            tint_scale: 1.0,
            refraction: 0.0,
            hue_bias: [1.0, 1.0, 1.0],
        },
    },
    Material {
        label: "Acrylique",
        algo: BlurAlgo::DualKawase,
        // BUG RÉEL SIGNALÉ : "je ne vois pas de différence entre le flou
        // givré et le flou acrylique". `stop_levels` n'était qu'à 1 palier
        // de "Givré" (2 contre 3, voir la doc du champ) : un seul filtre
        // `PSUp` de moins reste peu perceptible tant que le reste des
        // réglages (couche B ci-dessous) se contente lui aussi d'un écart
        // discret (noise/rim/tint_scale à quelques centièmes de "Givré").
        // Écarté au minimum plausible (0, voir la doc du champ) pour
        // maximiser le contraste avec "Givré" (déjà au plafond, 3) : le
        // rendu "Acrylique" attendu (voir la doc de module) est plus NET/
        // texturé que "Givré", pas juste "un peu moins diffus" — même
        // logique que le vrai matériau Acrylic de Windows face à un simple
        // flou givré : plus de grain/liseré, moins de remontée Kawase.
        stop_levels: 0,
        post: PostParams {
            noise: 6.0 / 255.0,
            rim: 0.12,
            gloss: 0.0,
            saturation: 1.0,
            tint_scale: 1.1,
            refraction: 0.0,
            hue_bias: [1.0, 1.0, 1.0],
        },
    },
    Material {
        label: "Verre liquide",
        algo: BlurAlgo::DualKawase,
        stop_levels: 3,
        post: PostParams {
            noise: MIN_NOISE,
            rim: 0.55,
            gloss: 0.10,
            saturation: 1.05,
            tint_scale: 0.82,
            refraction: 2.5,
            hue_bias: [1.0, 1.0, 1.0],
        },
    },
    Material {
        label: "Frutiger Aero",
        algo: BlurAlgo::DualKawase,
        stop_levels: 2,
        post: PostParams {
            noise: MIN_NOISE,
            rim: 0.25,
            gloss: 0.45,
            saturation: 1.35,
            tint_scale: 0.95,
            refraction: 0.0,
            hue_bias: [0.96, 1.02, 1.08],
        },
    },
];

/// Index par défaut dans [`MATERIALS`] — "Acrylique", voir la doc de la
/// table.
pub const DEFAULT_MATERIAL_INDEX: usize = 2;

thread_local! {
    /// Index du matériau actuellement sélectionné — un seul thread de rendu
    /// pour toute l'appli (voir `app::run`), même principe que
    /// `gfx::blur::BLUR_PASSES`.
    static CURRENT: Cell<usize> = const { Cell::new(DEFAULT_MATERIAL_INDEX) };
}

/// Change le matériau actif, borné défensivement à la dernière entrée
/// valide plutôt que de paniquer (même raison que
/// `gfx::blur::set_blur_passes` pour un `blur_material.txt` corrompu ou
/// d'une version antérieure). PAS persisté ici (voir
/// `config::save_blur_material`, appelé par l'appelant).
pub fn set_current(index: usize) {
    CURRENT.with(|c| c.set(index.min(MATERIALS.len() - 1)));
}

/// Index du matériau actuellement sélectionné — voir `CURRENT`.
pub fn current_index() -> usize {
    CURRENT.with(Cell::get)
}

/// Le matériau actuellement sélectionné — voir `CURRENT`.
pub fn current() -> &'static Material {
    &MATERIALS[current_index()]
}

/// Étiquettes des matériaux dans l'ordre d'affichage, pour le sélecteur du
/// panneau de réglages (voir `win32::cef_settings::push_state`,
/// `"blurMaterialLabels"`).
pub fn labels() -> impl Iterator<Item = &'static str> {
    MATERIALS.iter().map(|m| m.label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_current_clamps_out_of_range_index_instead_of_panicking() {
        set_current(99);
        assert_eq!(current_index(), MATERIALS.len() - 1);
        // Remet l'état par défaut : thread_local partagé par tous les tests
        // de ce module dans le même thread de test.
        set_current(DEFAULT_MATERIAL_INDEX);
    }

    #[test]
    fn every_material_respects_the_72px_bar_stop_levels_cap() {
        // Voir gfx::blur::MAX_STOP_LEVELS_HARD_CAP — au-delà de 3, deux
        // matériaux produiraient un rendu identique sur les barres du dock.
        for material in &MATERIALS {
            assert!(
                material.stop_levels <= 3,
                "{} a stop_levels={}, au-delà du plafond des barres 72px",
                material.label,
                material.stop_levels
            );
        }
    }

    #[test]
    fn every_material_has_sane_post_params() {
        for material in &MATERIALS {
            let post = &material.post;
            assert!(post.noise >= MIN_NOISE, "{}: noise sous le plancher anti-banding", material.label);
            assert!(post.saturation > 0.0, "{}: saturation nulle ou négative", material.label);
            assert!(
                (0.5..=1.5).contains(&post.tint_scale),
                "{}: tint_scale={} hors de la plage plausible",
                material.label,
                post.tint_scale
            );
        }
    }

    #[test]
    fn materials_table_has_five_entries_and_a_valid_default() {
        assert_eq!(MATERIALS.len(), 5);
        assert!(DEFAULT_MATERIAL_INDEX < MATERIALS.len());
    }

    #[test]
    fn labels_are_in_display_order() {
        let collected: Vec<_> = labels().collect();
        assert_eq!(collected, MATERIALS.iter().map(|m| m.label).collect::<Vec<_>>());
    }
}
