//! Dégradé animé « couleur de thème » — DEMANDE EXPLICITE (2026-09-26) : un
//! dégradé animé façon gradient-lab, « commun comme si tous les panneaux ne
//! faisaient qu'un », et « seulement dans le mode capturable pas le mode
//! transparent ».
//!
//! - **Commun** : le shader (`gfx::quad_renderer`, `Degrade`) l'évalue en
//!   coordonnées ÉCRAN — la position du pixel dans sa fenêtre plus l'origine
//!   de la fenêtre (`QuadRenderer::render_at`). Une barre et un panneau
//!   voisins montrent donc les deux morceaux d'une même image, qui continue
//!   d'un écran à l'autre (le repère est l'écran VIRTUEL).
//! - **Mode capturable seulement** (`gfx::capture_visibility`) : le dégradé
//!   REMPLACE alors la teinte ET le fond flouté — c'est son dessein (« pas le
//!   mode transparent ») — donc son rendu est OPAQUE, comme le fond qu'il
//!   remplace, y compris depuis que la loupe (`gfx::mag_capture`) fournit un
//!   fond dans ce mode. Ce n'est que sans fond du tout (loupe indisponible,
//!   repli `sans_fond` de `gfx::quad_renderer`) qu'il garde l'alpha de
//!   transparence réglé par l'utilisateur. Hors de ce mode, la case est sans
//!   effet (`actif`).
//!
//! L'algorithme (« Mesh ») : cinq couleurs portées par cinq points qui
//! orbitent lentement, mélangées par pondération inverse à la distance
//! (Shepard) dans un espace déformé par un bruit de gradient (domain warp) —
//! écrit ici, pas repris d'une bibliothèque.
//!
//! Lot 2 : huit curseurs (`REGLAGES`) et cinq couleurs modifiables. Choisir un
//! préréglage COPIE ses couleurs ; retoucher une couleur fait passer à
//! « Personnalisé » (`PERSONNALISE`).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Un préréglage : cinq couleurs RGB (0..1), dans l'ordre des cinq points.
pub struct Preset {
    pub nom: &'static str,
    pub couleurs: [[f32; 3]; 5],
}

const fn rgb(hex: u32) -> [f32; 3] {
    [
        ((hex >> 16) & 0xFF) as f32 / 255.0,
        ((hex >> 8) & 0xFF) as f32 / 255.0,
        (hex & 0xFF) as f32 / 255.0,
    ]
}

/// Couleurs plutôt sombres : le texte des panneaux reste celui du thème, et
/// un dégradé trop clair le rendrait illisible en mode sombre.
pub const PRESETS: [Preset; 32] = [
    Preset { nom: "Aurore", couleurs: [rgb(0x0B1D3A), rgb(0x1F6F78), rgb(0x2E9E7F), rgb(0x5B3690), rgb(0x10243E)] },
    Preset { nom: "Crépuscule", couleurs: [rgb(0x2B1055), rgb(0x4A5FA8), rgb(0xA52A5A), rgb(0xB0782E), rgb(0x1A0B2E)] },
    Preset { nom: "Océan", couleurs: [rgb(0x03045E), rgb(0x0068A0), rgb(0x0093B5), rgb(0x3A7CA5), rgb(0x023E8A)] },
    Preset { nom: "Braise", couleurs: [rgb(0x1A0505), rgb(0x6E1A1A), rgb(0xB5400D), rgb(0xC07A00), rgb(0x3B0A0A)] },
    Preset { nom: "Lavande", couleurs: [rgb(0x2D1B4E), rgb(0x5D3F9C), rgb(0x8C6BB8), rgb(0xA0588F), rgb(0x3E2A6B)] },
    Preset { nom: "Forêt", couleurs: [rgb(0x0B2418), rgb(0x1E5631), rgb(0x3F7F24), rgb(0x6E8F12), rgb(0x0F3D2E)] },
    // DEMANDE (2026-09-27, lien vers backgrounds.supply/gradient-lab) : plus
    // de préréglages, toujours sombres (même raison que ci-dessus).
    Preset { nom: "Minuit", couleurs: [rgb(0x05070F), rgb(0x141B3C), rgb(0x2A3A7A), rgb(0x4B5BB0), rgb(0x0A0F24)] },
    Preset { nom: "Néon", couleurs: [rgb(0x0A0014), rgb(0x7A0FB0), rgb(0x0F9FB5), rgb(0xC0147A), rgb(0x1A0638)] },
    Preset { nom: "Pêche", couleurs: [rgb(0x2E1414), rgb(0x8C4A3A), rgb(0xB5705A), rgb(0x9C5070), rgb(0x4A2226)] },
    Preset { nom: "Menthe", couleurs: [rgb(0x061F1C), rgb(0x0F5C4E), rgb(0x2E9C80), rgb(0x5AAE9A), rgb(0x0B3A34)] },
    Preset { nom: "Rubis", couleurs: [rgb(0x14030A), rgb(0x5C0A24), rgb(0x9C1238), rgb(0x6E0F4A), rgb(0x2E0614)] },
    Preset { nom: "Nordique", couleurs: [rgb(0x1C2230), rgb(0x3B4A60), rgb(0x5E7A8C), rgb(0x7A6A8C), rgb(0x2A3444)] },
    Preset { nom: "Sable", couleurs: [rgb(0x221A10), rgb(0x6B5230), rgb(0x9C7C46), rgb(0x8C5E3A), rgb(0x3A2C18)] },
    Preset { nom: "Galaxie", couleurs: [rgb(0x05030F), rgb(0x2A0F5C), rgb(0x5C1E8C), rgb(0x1E4A9C), rgb(0x120828)] },
    Preset { nom: "Corail", couleurs: [rgb(0x240A12), rgb(0x9C3A3A), rgb(0xC0604A), rgb(0x2E7A7A), rgb(0x3A1420)] },
    Preset { nom: "Glacier", couleurs: [rgb(0x08141F), rgb(0x1E4A6B), rgb(0x3A8CB0), rgb(0x6AA8C0), rgb(0x0F2A3E)] },
    Preset { nom: "Cyberpunk", couleurs: [rgb(0x0A0A14), rgb(0xB0147A), rgb(0x14B0B0), rgb(0xB0A014), rgb(0x2A0A3A)] },
    Preset { nom: "Automne", couleurs: [rgb(0x1A0C06), rgb(0x7A2E0F), rgb(0xA0561A), rgb(0x8C7A1E), rgb(0x3A1A0A)] },
    Preset { nom: "Mousse", couleurs: [rgb(0x10160A), rgb(0x3A4A1E), rgb(0x5E7030), rgb(0x7A8040), rgb(0x1E2A12)] },
    Preset { nom: "Améthyste", couleurs: [rgb(0x12081C), rgb(0x4A1E6B), rgb(0x7A3A9C), rgb(0x9C5AA0), rgb(0x24103A)] },
    Preset { nom: "Vapeur", couleurs: [rgb(0x1A1030), rgb(0x8C4A9C), rgb(0x3A8CA0), rgb(0xB06A8C), rgb(0x2A1A4A)] },
    Preset { nom: "Coucher de soleil", couleurs: [rgb(0x1E0A1E), rgb(0x8C2A3A), rgb(0xC0602A), rgb(0xB08A30), rgb(0x4A1A3A)] },
    Preset { nom: "Bonbon", couleurs: [rgb(0x2A1030), rgb(0xA04A8C), rgb(0x5A7AC0), rgb(0x3AA08C), rgb(0x4A2050)] },
    Preset { nom: "Encre", couleurs: [rgb(0x06080E), rgb(0x1A2238), rgb(0x2E3A5C), rgb(0x46506E), rgb(0x10141F)] },
    Preset { nom: "Brume", couleurs: [rgb(0x141A22), rgb(0x3A4A5A), rgb(0x5A6E7E), rgb(0x46607A), rgb(0x222C38)] },
    Preset { nom: "Or fondu", couleurs: [rgb(0x1A1004), rgb(0x6B4A0F), rgb(0xB08A2A), rgb(0xC0A04A), rgb(0x3A2608)] },
    Preset { nom: "Carrare", couleurs: [rgb(0x2A2A2E), rgb(0x5A5A60), rgb(0x7A7A80), rgb(0x9A9AA0), rgb(0x3A3A40)] },
    Preset { nom: "Magma", couleurs: [rgb(0x0F0404), rgb(0x5C0F06), rgb(0xA0300A), rgb(0xC06A0F), rgb(0x2A0804)] },
    Preset { nom: "Sauge", couleurs: [rgb(0x141A14), rgb(0x3E5040), rgb(0x5E7460), rgb(0x7A8C6A), rgb(0x243024)] },
    Preset { nom: "Pierre", couleurs: [rgb(0x1A1816), rgb(0x46403A), rgb(0x6A625A), rgb(0x5A5048), rgb(0x2C2824)] },
    Preset { nom: "Espace profond", couleurs: [rgb(0x020208), rgb(0x0F1440), rgb(0x3A1460), rgb(0x0F4A6B), rgb(0x080A1E)] },
    // DEMANDE EXPLICITE (2026-10-03) — les trois couleurs du `ShaderGradient`
    // fourni : `color3` (#2f0000) en fond, puis `color1` (#a10a62) et `color2`
    // (#ffa81c), avec deux intermédiaires pour que la rampe à cinq arrêts passe
    // par les mêmes teintes. À essayer avec le motif « Plan d'eau ».
    Preset { nom: "Plan d'eau", couleurs: [rgb(0x2F0000), rgb(0x6A0638), rgb(0xA10A62), rgb(0xD0593F), rgb(0xFFA81C)] },
];

// Atomiques et non `thread_local!` : les popups CEF lisent aussi l'état (voir
// la doc de `gfx::capture_visibility`, même contrainte).
static ENABLED: AtomicBool = AtomicBool::new(false);
/// Index dans `PRESETS`, ou `PERSONNALISE`.
static PRESET: AtomicUsize = AtomicUsize::new(0);
static DEPART: OnceLock<Instant> = OnceLock::new();
/// **LA PAUSE** — DEMANDE EXPLICITE (2026-10-12) : « dans l'éditeur de shader il
/// faut un bouton pour mettre en pause le shader ». Le temps passé en pause est
/// RETRANCHÉ du temps du motif : à la reprise, il repart de l'image figée au
/// lieu de sauter. Pour la session seulement : la pause ne survit pas à un
/// redémarrage, un fond figé au lancement passerait pour une panne.
static PAUSE: Mutex<Horloge> = Mutex::new(Horloge::NEUVE);

/// Le temps du motif : celui écoulé depuis `DEPART`, moins les pauses.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Horloge {
    /// Le total des pauses terminées.
    retranche: Duration,
    /// L'écoulé (depuis `DEPART`) au moment de la mise en pause, s'il y en a une.
    fige_a: Option<Duration>,
}

impl Horloge {
    const NEUVE: Horloge = Horloge { retranche: Duration::ZERO, fige_a: None };

    fn temps(&self, ecoule: Duration) -> Duration {
        self.fige_a.unwrap_or(ecoule).saturating_sub(self.retranche)
    }

    fn basculee(self, ecoule: Duration) -> Horloge {
        match self.fige_a {
            None => Horloge { fige_a: Some(ecoule), ..self },
            Some(fige_a) => Horloge { retranche: self.retranche + ecoule.saturating_sub(fige_a), fige_a: None },
        }
    }
}

fn ecoule() -> Duration {
    DEPART.get_or_init(Instant::now).elapsed()
}

/// Met le motif en pause, ou le relance.
pub fn basculer_pause() {
    if let Ok(mut horloge) = PAUSE.lock() {
        *horloge = horloge.basculee(ecoule());
    }
}

pub fn en_pause() -> bool {
    PAUSE.lock().map_or(false, |h| h.fige_a.is_some())
}

fn temps_du_motif() -> Duration {
    let horloge = PAUSE.lock().map_or(Horloge::NEUVE, |h| *h);
    horloge.temps(ecoule())
}

/// Nom affiché (et sauvegardé) quand les couleurs ne sont plus celles d'un
/// préréglage.
pub const PERSONNALISE: &str = "Personnalisé";
const INDEX_PERSONNALISE: usize = usize::MAX;

const fn pack(c: [f32; 3]) -> u32 {
    ((c[0] * 255.0 + 0.5) as u32) << 16 | ((c[1] * 255.0 + 0.5) as u32) << 8 | (c[2] * 255.0 + 0.5) as u32
}

/// Les cinq couleurs COURANTES, en `0xRRGGBB` — au départ celles du premier
/// préréglage.
static COULEURS: [AtomicU32; 5] = {
    let c = PRESETS[0].couleurs;
    [
        AtomicU32::new(pack(c[0])),
        AtomicU32::new(pack(c[1])),
        AtomicU32::new(pack(c[2])),
        AtomicU32::new(pack(c[3])),
        AtomicU32::new(pack(c[4])),
    ]
};

/// Un curseur : nom (celui de l'action `degrade-reglage` et de la clé JSON),
/// valeur par défaut, bornes.
pub struct Reglage {
    pub nom: &'static str,
    pub defaut: f32,
    pub min: f32,
    pub max: f32,
}

/// Dans l'ORDRE du fichier `degrade_reglages.txt` et de `DegradeGpu` (échelle
/// … grain) — un réglage ajouté va en FIN, sinon les fichiers déjà écrits se
/// reliraient décalés.
pub const REGLAGES: [Reglage; 23] = [
    Reglage { nom: "echelle", defaut: 1.6, min: 0.3, max: 5.0 },
    Reglage { nom: "douceur", defaut: 0.55, min: 0.0, max: 1.0 },
    Reglage { nom: "warp", defaut: 0.30, min: 0.0, max: 1.0 },
    Reglage { nom: "vitesse", defaut: 0.10, min: 0.0, max: 1.0 },
    Reglage { nom: "luminosite", defaut: 1.0, min: 0.3, max: 2.0 },
    Reglage { nom: "contraste", defaut: 1.0, min: 0.3, max: 2.0 },
    Reglage { nom: "saturation", defaut: 1.0, min: 0.0, max: 2.0 },
    Reglage { nom: "grain", defaut: 0.025, min: 0.0, max: 0.2 },
    // DEMANDE EXPLICITE (2026-09-27) : « ajoute un curseur de flou ».
    Reglage { nom: "flou", defaut: 0.0, min: 0.0, max: 1.0 },
    // Nombre de couleurs actives (2..5), voir `couleurs_actives` — pas un
    // curseur : des boutons dans la page.
    Reglage { nom: "couleurs", defaut: 5.0, min: 2.0, max: 5.0 },
    // DEMANDE EXPLICITE (2026-09-27) : « ajoute des possibilités de
    // randomisation dans le calcul des effets […] avec slider ». L'aléa fait
    // dériver lentement plusieurs curseurs au hasard (voir `derive`) ; la
    // graine change la disposition du motif (le bruit et les tirages du
    // shader, voir `DegGraine`).
    Reglage { nom: "alea", defaut: 0.0, min: 0.0, max: 1.0 },
    Reglage { nom: "graine", defaut: 0.0, min: 0.0, max: 100.0 },
    // DEMANDE EXPLICITE (2026-09-27) : « ajoute des possibilités d'ajouter des
    // réglages aléatoires dans l'origine de chaque effet ». L'origine (0..1
    // de l'écran virtuel) est le point où tombe le centre du motif — sans
    // elle, sur plusieurs écrans, le centre d'un Vortex ou d'une Aura peut
    // tomber entre deux ; l'errance la fait dériver au hasard (voir `derive`).
    Reglage { nom: "origine_x", defaut: 0.5, min: 0.0, max: 1.0 },
    Reglage { nom: "origine_y", defaut: 0.5, min: 0.0, max: 1.0 },
    Reglage { nom: "errance", defaut: 0.0, min: 0.0, max: 1.0 },
    // DEMANDE EXPLICITE (2026-09-27) : « ajoute de la 3d dans les shaders ».
    // **RELIEF** est la force d'un éclairage directionnel appliqué au motif —
    // 0 par défaut, et à zéro la sortie est EXACTEMENT celle d'avant (aucun
    // motif n'a à être revu pour que le curseur existe).
    //
    // ⚠️ L'ORDRE COMPTE : un réglage ajouté va EN FIN, sinon `degrade_reglages
    // .txt` déjà écrit se relirait décalé (voir la doc de `REGLAGES` ci-dessus).
    Reglage { nom: "relief", defaut: 0.0, min: 0.0, max: 1.0 },
    /// Direction de la lumière, en tours (0..1) — un tour complet autour du
    /// motif. Passe par `REGLAGES`, donc les boutons « au hasard » la tirent
    /// comme les autres, sans mécanisme de tirage supplémentaire.
    Reglage { nom: "lumiere", defaut: 0.25, min: 0.0, max: 1.0 },
    // DEMANDE EXPLICITE (2026-09-27) : « oui ajoute » — la palette COSINUS du
    // shader de raymarching de l'utilisateur (`a + b·cos(2π(c·x + d))`), dosée
    // de 0 à 1. À 0, la rampe du thème seule, donc sortie inchangée.
    //
    // Ses paramètres sont TIRÉS DES CINQ COULEURS DU THÈME (voir
    // `DegPaletteCosinus`) : recopier ses constantes aurait figé la palette et
    // cassé ce qui fait qu'un préréglage change le motif.
    Reglage { nom: "cosinus", defaut: 0.0, min: 0.0, max: 1.0 },
    // DEMANDE EXPLICITE (2026-09-27) : « fais le » — la VRAIE 3D : le plan est
    // incliné et on MARCHE le long du rayon de vue dans le champ de hauteur. On
    // obtient la parallaxe, l'auto-occlusion et des silhouettes qui bougent,
    // là où le relief seul ne donne que du modelé.
    //
    // À 0, la direction du regard est DROITE (0, 0, −1) : aucun décalage, donc
    // la sortie est rigoureusement celle d'avant.
    Reglage { nom: "perspective", defaut: 0.0, min: 0.0, max: 1.0 },
    // DEMANDE EXPLICITE (2026-10-03) : « fais en sorte qu'il y ait des remous
    // paramétrables partout ». Des tourbillons semés sur TOUTE la surface, qui
    // tordent le motif AVANT qu'il soit évalué — donc valables pour les 75
    // motifs (voir `DegRemousTorsion`). À force 0 — le défaut —, la torsion
    // n'est pas appliquée : sortie rigoureusement inchangée.
    Reglage { nom: "remous", defaut: 0.0, min: 0.0, max: 1.0 },
    /// Rayon d'un tourbillon, en part de sa cellule.
    Reglage { nom: "remous_taille", defaut: 0.6, min: 0.1, max: 1.0 },
    /// Vitesse de rotation propre des tourbillons (indépendante de « vitesse »).
    Reglage { nom: "remous_vitesse", defaut: 0.3, min: 0.0, max: 1.0 },
    /// Nombre de tourbillons à l'écran (taille de la grille qui les sème).
    Reglage { nom: "remous_densite", defaut: 0.35, min: 0.0, max: 1.0 },
];

static VALEURS: [AtomicU32; 23] = {
    const fn bits(i: usize) -> AtomicU32 {
        AtomicU32::new(REGLAGES[i].defaut.to_bits())
    }
    [bits(0), bits(1), bits(2), bits(3), bits(4), bits(5), bits(6), bits(7), bits(8), bits(9), bits(10), bits(11), bits(12), bits(13), bits(14), bits(15), bits(16), bits(17), bits(18), bits(19), bits(20), bits(21), bits(22)]
};

pub fn reglage(i: usize) -> f32 {
    f32::from_bits(VALEURS[i].load(Ordering::Relaxed))
}

/// Règle le curseur `nom` (borné). Renvoie `false` pour un nom inconnu.
pub fn set_reglage(nom: &str, valeur: f32) -> bool {
    let Some(i) = REGLAGES.iter().position(|r| r.nom == nom) else { return false };
    if !valeur.is_finite() {
        return false;
    }
    let r = &REGLAGES[i];
    VALEURS[i].store(valeur.clamp(r.min, r.max).to_bits(), Ordering::Relaxed);
    true
}

pub fn reglages_par_defaut() {
    for (v, r) in VALEURS.iter().zip(REGLAGES.iter()) {
        v.store(r.defaut.to_bits(), Ordering::Relaxed);
    }
}

pub fn valeurs_reglages() -> [f32; REGLAGES.len()] {
    std::array::from_fn(reglage)
}

pub fn couleur(i: usize) -> [f32; 3] {
    let c = COULEURS[i].load(Ordering::Relaxed);
    [((c >> 16) & 0xFF) as f32 / 255.0, ((c >> 8) & 0xFF) as f32 / 255.0, (c & 0xFF) as f32 / 255.0]
}

pub fn couleurs() -> [[f32; 3]; 5] {
    std::array::from_fn(couleur)
}

/// Index de `REGLAGES` du nombre de couleurs actives.
const REGLAGE_COULEURS: usize = 9;

/// Les cinq couleurs envoyées au GPU : les `n` premières (réglage
/// « couleurs ») RÉÉCHANTILLONNÉES sur les cinq emplacements — les motifs
/// lisent toujours cinq couleurs, et une rampe de `n` couleurs ne montre
/// qu'elles et leurs mélanges.
pub fn couleurs_actives() -> [[f32; 3]; 5] {
    reechantillonner(&couleurs(), reglage(REGLAGE_COULEURS).round() as usize)
}

pub(crate) fn reechantillonner(c: &[[f32; 3]; 5], n: usize) -> [[f32; 3]; 5] {
    let n = n.clamp(2, 5);
    std::array::from_fn(|k| {
        let pos = k as f32 * (n - 1) as f32 / 4.0;
        let i = (pos.floor() as usize).min(n - 2);
        let f = pos - i as f32;
        std::array::from_fn(|ch| c[i][ch] + (c[i + 1][ch] - c[i][ch]) * f)
    })
}

/// Une palette tirée au sort (bouton « Au hasard ») : une teinte de base, un
/// schéma (analogue, complémentaire ou triade), la première couleur sombre
/// (elle fait le fond de plusieurs motifs), les autres moyennement sombres —
/// même contrainte que `PRESETS`.
pub fn palette_au_hasard(graine: u64) -> [[f32; 3]; 5] {
    let mut tirer = hasard(graine);
    let base = tirer();
    let schema = (tirer() * 3.0) as usize;
    std::array::from_fn(|k| {
        let decalage = match schema {
            0 => (k as f32 - 2.0) * 0.07,
            1 => if k % 2 == 1 { 0.5 } else { 0.0 },
            _ => (k % 3) as f32 / 3.0,
        };
        let teinte = (base + decalage + (tirer() - 0.5) * 0.06).rem_euclid(1.0);
        let saturation = 0.5 + 0.4 * tirer();
        let valeur = if k == 0 { 0.06 + 0.12 * tirer() } else { 0.3 + 0.4 * tirer() };
        hsv(teinte, saturation, valeur)
    })
}

/// Tirages uniformes dans [0, 1) — splitmix64 : assez pour tirer quelques
/// nombres, sans dépendance.
fn hasard(graine: u64) -> impl FnMut() -> f32 {
    let mut etat = graine;
    move || {
        etat = etat.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = etat;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn graine_horloge() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

/// Curseurs tirés au sort (bouton « Curseurs au hasard ») — dans des
/// fourchettes qui restent regardables, pas sur toute leur course : un
/// contraste à 0,3 ou une luminosité à 2 ne donnent rien d'utilisable. Le
/// flou et l'aléa ne sont pas tirés (ce sont des choix, pas des variations).
pub fn reglages_au_hasard(graine: u64) -> Vec<(&'static str, f32)> {
    let mut tirer = hasard(graine);
    let mut entre = |a: f32, b: f32| a + (b - a) * tirer();
    vec![
        ("echelle", entre(0.6, 3.5)),
        ("douceur", entre(0.1, 1.0)),
        ("warp", entre(0.0, 1.0)),
        ("vitesse", entre(0.04, 0.4)),
        ("luminosite", entre(0.85, 1.25)),
        ("contraste", entre(0.9, 1.3)),
        ("saturation", entre(0.8, 1.4)),
        ("grain", entre(0.0, 0.05)),
        ("couleurs", entre(3.0, 6.0).floor()),
        ("graine", entre(0.0, 101.0).floor()),
        ("origine_x", entre(0.15, 0.85)),
        ("origine_y", entre(0.15, 0.85)),
    ]
}

/// Origine tirée au sort (bouton « Origine au hasard ») — pas trop près des
/// bords, où le centre du motif sortirait presque de l'écran.
pub fn tirer_origine() {
    let mut tirer = hasard(graine_horloge());
    set_reglage("origine_x", 0.1 + 0.8 * tirer());
    set_reglage("origine_y", 0.1 + 0.8 * tirer());
}

/// Amplitude de l'errance au maximum, en fraction de l'écran virtuel.
const ERRANCE_MAX: f32 = 0.35;

/// Décalage du motif, en unités de HAUTEUR de l'écran virtuel (celles de `p`
/// dans le shader) : l'origine réglée, plus son errance à l'instant `t`,
/// moins le centre de l'écran — à l'origine par défaut, aucun décalage.
fn decalage_origine(v: &[f32; REGLAGES.len()], t: f32, aspect: f32) -> [f32; 2] {
    let (graine, errance) = (v[11], v[14]);
    let x = v[12] + errance * ERRANCE_MAX * derive(t, graine, 6);
    let y = v[13] + errance * ERRANCE_MAX * derive(t, graine, 7);
    [(x - 0.5) * aspect, y - 0.5]
}

/// Tire les curseurs au sort ; `tout` tire aussi le motif et la palette.
pub fn tirer_reglages(tout: bool) {
    let graine = graine_horloge();
    for (nom, v) in reglages_au_hasard(graine) {
        set_reglage(nom, v);
    }
    if tout {
        let mut tirer = hasard(graine ^ 0xA5A5_A5A5);
        MODE.store(((tirer() * MODES.len() as f32) as usize).min(MODES.len() - 1), Ordering::Relaxed);
        for (i, c) in palette_au_hasard(graine.rotate_left(17)).into_iter().enumerate() {
            set_couleur(i, c);
        }
    }
}

/// Dérive lente dans [-1, 1] du curseur `i` à l'instant `t` (secondes) :
/// deux sinus de périodes sans rapport simple (≈ 25 à 70 s), déphasés par la
/// graine — le mouvement ne se répète pas à l'œil et reste doux.
fn derive(t: f32, graine: f32, i: usize) -> f32 {
    let k = i as f32 + 1.0;
    let a = (t * (0.09 + 0.013 * k) + graine * 1.7 + k * 2.3).sin();
    let b = (t * (0.041 + 0.007 * k) + graine * 0.9 + k * 5.1).sin();
    (a + b) * 0.5
}

/// Les curseurs de rendu après la dérive de l'aléa : `(échelle, douceur,
/// ondulation, luminosité, contraste, saturation)`. La VITESSE ne dérive pas :
/// le shader lit `temps × vitesse`, la faire varier ferait sauter le motif.
fn avec_alea(v: &[f32; REGLAGES.len()], t: f32) -> [f32; 6] {
    let [echelle, douceur, warp, _, luminosite, contraste, saturation, ..] = *v;
    let (alea, graine) = (v[10], v[11]);
    if alea <= 0.0 {
        return [echelle, douceur, warp, luminosite, contraste, saturation];
    }
    let d = |i| alea * derive(t, graine, i);
    [
        (echelle * (1.0 + 0.45 * d(0))).max(REGLAGES[0].min),
        (douceur + 0.4 * d(1)).clamp(0.0, 1.0),
        (warp + 0.5 * d(2)).clamp(0.0, 1.0),
        luminosite * (1.0 + 0.12 * d(3)),
        contraste * (1.0 + 0.15 * d(4)),
        (saturation * (1.0 + 0.35 * d(5))).max(0.0),
    ]
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let f = |n: f32| {
        let k = (n + h * 6.0).rem_euclid(6.0);
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [f(5.0), f(3.0), f(1.0)]
}

/// Tire une palette au sort et la prend comme couleurs personnalisées.
pub fn tirer_palette() {
    for (i, c) in palette_au_hasard(graine_horloge()).into_iter().enumerate() {
        set_couleur(i, c);
    }
}

/// Remplace la couleur `i` — les couleurs deviennent « Personnalisé ».
/// Renvoie `false` pour un index hors de 0..5.
pub fn set_couleur(i: usize, rgb: [f32; 3]) -> bool {
    if i >= COULEURS.len() {
        return false;
    }
    COULEURS[i].store(pack(rgb.map(|v| v.clamp(0.0, 1.0))), Ordering::Relaxed);
    PRESET.store(INDEX_PERSONNALISE, Ordering::Relaxed);
    true
}

/// Les modes du shader (`Degrade` dans `gfx::quad_renderer`, qui aiguille
/// sur l'INDEX) — DEMANDE EXPLICITE (2026-09-27) : « vas y ajoute ces
/// shaders ». Le nom est celui du bouton et du fichier `degrade_mode.txt` ;
/// un mode ajouté va en FIN (le shader lit l'index).
pub const MODES: [&str; 75] = [
    "Mesh", "Aurore", "Plasma", "Bandes", "Soie", "Voronoï", "Caustiques", "Synthwave", "Tramé",
    // DEMANDE (2026-09-27, lien vers backgrounds.supply/gradient-lab) : écrits
    // d'après les techniques publiques, pas repris du site.
    "Doux", "Cannelé", "Concentrique", "Fumée", "Rayons", "Prisme", "Vortex", "Marbre", "Vagues", "Ruban",
    "Tunnel", "Kaléidoscope", "Lave", "Nébuleuse", "Conique", "Topographie", "Pixels", "Moiré", "Lueurs", "Damier",
    "Ondes", "Fibres", "Irisé", "Holographique", "Braises", "Pluie", "Aura", "Éclosion", "Drapé", "Duo", "Satin",
    // DEMANDE EXPLICITE (2026-09-27) : « créer 20 nouveaux motifs pour la
    // visibilité dans les captures » — la section des Réglages qui porte le
    // dégradé s'appelle « Visible dans les captures d'écran ». Écrits ici,
    // aucun n'est repris d'une bibliothèque.
    "Toile", "Écailles", "Zèbre", "Cristal", "Brume", "Sillons", "Chevrón", "Étoiles", "Réseau", "Marée",
    "Confettis", "Spirale", "Écho", "Prairie", "Amande", "Voile", "Tisons", "Ondulation", "Vitrail", "Neige",
    // DEMANDE EXPLICITE (2026-09-27) : « cherches sur internet de la doc pour
    // faire de nouveaux shaders jamais vus ». Chacun vient d'une famille
    // DOCUMENTÉE absente des soixante précédents, et la source est citée sur la
    // fonction (`gfx::quad_renderer`) — aucune n'est devinée.
    "Truchet", "Labyrinthe", "Chladni", "Rosace", "Quasicristal", "Guilloché",
    "Phyllotaxie", "Ridé", "Newton", "Nid d'abeille", "Résonance",
    // DEMANDE EXPLICITE (2026-09-27) : « peux tu faire un shader avec ça ? », à
    // partir du motif `facetgrad` de Tabbied (voir `DegFacetgrad`).
    "Facetgrad",
    // DEMANDE EXPLICITE (2026-10-03) : « ajouter un motif en t'inspirant de ce
    // code », un `<ShaderGradient type="waterPlane">` (voir `DegPlanDeau`).
    "Plan d'eau",
    // DEMANDE EXPLICITE (2026-10-03) : sa variante, « des rotations de surfaces
    // et une impression de mélanges en spirale » (voir `DegRemous`).
    "Remous",
    // DEMANDE EXPLICITE (2026-10-11) : une bulle CSS « avec de nombreuses bulles
    // et des turbulences de vent », intégrée (voir `DegBulles`).
    "Bulles",
];

static MODE: AtomicUsize = AtomicUsize::new(0);

pub fn mode_index() -> usize {
    MODE.load(Ordering::Relaxed).min(MODES.len() - 1)
}

pub fn mode_nom() -> &'static str {
    MODES[mode_index()]
}

/// Choisit le mode `nom`. Renvoie `false` pour un nom inconnu.
///
/// Un motif INTÉGRÉ choisi retire celui de l'utilisateur (voir
/// `gfx::shader_utilisateur`) : c'est le geste qui le quitte.
pub fn set_mode(nom: &str) -> bool {
    let Some(i) = MODES.iter().position(|m| *m == nom) else { return false };
    MODE.store(i, Ordering::Relaxed);
    crate::gfx::shader_utilisateur::desactiver();
    true
}

/// La case des Réglages — PAS l'état effectif, voir `actif`.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Le dégradé est dessiné : case cochée ET mode capturable.
pub fn actif() -> bool {
    is_enabled() && crate::gfx::capture_visibility::is_enabled()
}

/// Case « Mise en avant tirée du dégradé » — DEMANDE EXPLICITE (2026-09-27,
/// « oui pour tout ») : les panneaux prennent leur accent dans les couleurs du
/// dégradé plutôt que dans leur couleur propre. Cochée par défaut.
static ACCENT: AtomicBool = AtomicBool::new(true);

pub fn accent_enabled() -> bool {
    ACCENT.load(Ordering::Relaxed)
}

pub fn set_accent_enabled(enabled: bool) {
    ACCENT.store(enabled, Ordering::Relaxed);
}

/// Luminosité (max des canaux) minimale de l'accent : les préréglages sont
/// SOMBRES (ils font un fond), un accent pris tel quel se confondrait avec
/// le fond qu'il doit mettre en avant.
const ACCENT_LUMINOSITE_MIN: f32 = 0.8;

/// Accent des panneaux, commun à tous, quand le dégradé est dessiné et que
/// la case de l'accent est cochée ; sinon `None` (chaque panneau garde sa
/// couleur). Voir `accent_de`.
pub fn accent() -> Option<[f32; 3]> {
    (actif() && accent_enabled()).then(|| accent_de(&couleurs_actives()))
}

/// La couleur la plus SATURÉE des cinq (écart max − min des canaux, départagé
/// par la luminosité), éclaircie à teinte et saturation constantes jusqu'à
/// `ACCENT_LUMINOSITE_MIN`.
fn accent_de(couleurs: &[[f32; 3]; 5]) -> [f32; 3] {
    let chroma = |c: &[f32; 3]| c.iter().copied().fold(0.0f32, f32::max) - c.iter().copied().fold(1.0f32, f32::min);
    let score = |c: &[f32; 3]| chroma(c) * 4.0 + c.iter().copied().fold(0.0f32, f32::max);
    let c = couleurs.iter().copied().max_by(|a, b| score(a).total_cmp(&score(b))).unwrap_or([0.5; 3]);
    let max = c.iter().copied().fold(0.0f32, f32::max);
    if max >= ACCENT_LUMINOSITE_MIN {
        c
    } else if max <= 0.0 {
        [ACCENT_LUMINOSITE_MIN; 3]
    } else {
        c.map(|v| v * ACCENT_LUMINOSITE_MIN / max)
    }
}

/// Nom du préréglage courant, ou `PERSONNALISE`.
pub fn preset_nom() -> &'static str {
    PRESETS.get(PRESET.load(Ordering::Relaxed)).map_or(PERSONNALISE, |p| p.nom)
}

/// Choisit le préréglage `nom` et copie ses couleurs. Renvoie `false` pour un
/// nom inconnu.
pub fn set_preset(nom: &str) -> bool {
    let Some(i) = PRESETS.iter().position(|p| p.nom == nom) else { return false };
    for (dst, c) in COULEURS.iter().zip(PRESETS[i].couleurs) {
        dst.store(pack(c), Ordering::Relaxed);
    }
    PRESET.store(i, Ordering::Relaxed);
    true
}

/// Recharge la case, le préréglage (ou les couleurs personnalisées) et les
/// curseurs — une fois au démarrage (`app::run`).
pub fn charger() {
    set_enabled(crate::config::load_bool_setting("degrade_anime"));
    set_accent_enabled(crate::config::load_bool_setting_or("degrade_accent", true));
    if let Some(mode) = crate::config::load_degrade_mode() {
        match mode.strip_prefix(crate::gfx::shader_utilisateur::PREFIXE) {
            Some(nom) => crate::gfx::shader_utilisateur::charger(nom),
            None => {
                set_mode(&mode);
            }
        }
    }
    let nom = crate::config::load_degrade_preset();
    if nom.as_deref() == Some(PERSONNALISE) {
        if let Some(v) = crate::config::load_degrade_couleurs() {
            for (i, c) in v.chunks_exact(3).take(COULEURS.len()).enumerate() {
                set_couleur(i, [c[0], c[1], c[2]]);
            }
        }
    } else if let Some(nom) = nom {
        set_preset(&nom);
    }
    if let Some(v) = crate::config::load_degrade_reglages() {
        for (r, valeur) in REGLAGES.iter().zip(v) {
            set_reglage(r.nom, valeur);
        }
    }
    crate::gfx::degrade_sauvegardes::charger();
}

/// Écrit tout l'état (motif, couleurs, curseurs) — après un changement qui
/// touche à tout (tirage complet, sauvegarde rechargée).
pub fn sauver_tout() {
    let mode = match crate::gfx::shader_utilisateur::nom_actif() {
        Some(nom) => format!("{}{nom}", crate::gfx::shader_utilisateur::PREFIXE),
        None => mode_nom().to_string(),
    };
    crate::config::save_degrade_mode(&mode);
    sauver_couleurs();
    crate::config::save_degrade_reglages(&valeurs_reglages());
}

/// Sauvegarde le préréglage (et les couleurs si « Personnalisé »).
pub fn sauver_couleurs() {
    crate::config::save_degrade_preset(preset_nom());
    if preset_nom() == PERSONNALISE {
        crate::config::save_degrade_couleurs(&couleurs().concat());
    }
}

/// Période de repli du temps envoyé au GPU : un `f32` garde une précision
/// d'un centième de seconde jusque vers 10⁵ s, et le saut au repli (une fois
/// par jour) passe inaperçu dans un mouvement aussi lent.
const PERIODE_S: f32 = 86_400.0;

/// Bloc de fin de `cbuffer Viewport` (voir `gfx::quad_renderer::
/// ViewportConstants`) — même règle de packing : des groupes de quatre `f32`,
/// taille multiple de 16 (voir le test en fin de fichier).
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct DegradeGpu {
    /// Coin supérieur gauche de la fenêtre dessinée, en pixels écran.
    origin: [f32; 2],
    time: f32,
    /// 1.0 si `actif`, sinon 0.0 — le shader garde alors la teinte.
    actif: f32,
    /// Écran virtuel : x, y, largeur, hauteur (pixels).
    ecran: [f32; 4],
    echelle: f32,
    douceur: f32,
    warp: f32,
    vitesse: f32,
    luminosite: f32,
    contraste: f32,
    saturation: f32,
    grain: f32,
    couleurs: [[f32; 4]; 5],
    /// Graine du motif (réglage « graine », 0..100) — le mode n'est plus
    /// envoyé : le shader est compilé par motif (voir `gfx::quad_renderer::
    /// pixel_source`).
    graine: f32,
    /// Rayon du flou, 0..1 de `REGLAGES` (0 : aucun échantillon de plus).
    flou: f32,
    /// Force du relief (0 : sortie identique à avant le relief).
    relief: f32,
    /// Direction de la lumière, en tours (0..1).
    lumiere: f32,
    /// Décalage du motif (voir `decalage_origine`).
    origine: [f32; 2],
    /// Dosage de la palette cosinus (voir `REGLAGES`) — **placé DANS le
    /// remplissage du deuxième registre**, donc sans décaler `origine` : elle
    /// occupe les 4 octets qui étaient déjà perdus.
    cosinus: f32,
    /// Force de la perspective (voir `REGLAGES`). ⚠️ **Le deuxième registre est
    /// maintenant PLEIN** — `origine` (8) + `cosinus` (4) + `perspective` (4) =
    /// 16, exactement. Il n'y a donc plus de remplissage à tenir, et le prochain
    /// champ devra ouvrir un TROISIÈME registre (voir le test de `packing`).
    perspective: f32,
    /// Les remous (voir `REGLAGES`) — un TROISIÈME registre, rempli exactement.
    remous: f32,
    remous_taille: f32,
    remous_vitesse: f32,
    remous_densite: f32,
}

/// Constantes pour une fenêtre dont le coin supérieur gauche est `origin`.
pub(crate) fn constantes(origin: (i32, i32)) -> DegradeGpu {
    let couleurs = couleurs_actives().map(|[r, g, b]| [r, g, b, 1.0]);
    let v = valeurs_reglages();
    let (vitesse, grain, flou, graine) = (v[3], v[7], v[8], v[11]);
    // Relief et lumière : les deux derniers, ajoutés en fin (voir `REGLAGES`).
    let (relief, lumiere) = (v[15], v[16]);
    let temps = temps_du_motif().as_secs_f32() % PERIODE_S;
    let [echelle, douceur, warp, luminosite, contraste, saturation] = avec_alea(&v, temps);
    let ecran = ecran_virtuel();
    DegradeGpu {
        origin: [origin.0 as f32, origin.1 as f32],
        time: temps,
        actif: if actif() { 1.0 } else { 0.0 },
        ecran,
        echelle,
        douceur,
        warp,
        vitesse,
        luminosite,
        contraste,
        saturation,
        grain,
        couleurs,
        graine,
        flou,
        relief,
        lumiere,
        origine: decalage_origine(&v, temps, ecran[2] / ecran[3]),
        cosinus: v[17],
        perspective: v[18],
        remous: v[19],
        remous_taille: v[20],
        remous_vitesse: v[21],
        remous_densite: v[22],
    }
}

fn ecran_virtuel() -> [f32; 4] {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };
    let [x, y, w, h] = unsafe {
        [SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN].map(|m| GetSystemMetrics(m))
    };
    [x as f32, y as f32, w.max(1) as f32, h.max(1) as f32]
}

#[cfg(test)]
mod tests {
    /// Figé pendant la pause, et repris SANS SAUT : le temps passé en pause ne
    /// compte pas.
    #[test]
    fn la_pause_fige_le_temps_et_la_reprise_repart_sans_saut() {
        let s = Duration::from_secs;
        let h = super::Horloge::NEUVE;
        assert_eq!(h.temps(s(10)), s(10));
        let h = h.basculee(s(10));
        assert_eq!(h.temps(s(25)), s(10), "en pause, le temps ne bouge plus");
        let h = h.basculee(s(25));
        assert_eq!(h.temps(s(25)), s(10), "pas de saut à la reprise");
        assert_eq!(h.temps(s(30)), s(15));
        let h = h.basculee(s(30)).basculee(s(40));
        assert_eq!(h.temps(s(41)), s(16), "les pauses s'additionnent");
    }

    use super::*;

    #[test]
    fn accent_prend_la_plus_saturee_et_l_eclaircit() {
        let gris = [0.2, 0.2, 0.2];
        let bleu = [0.1, 0.2, 0.5];
        let c = accent_de(&[gris, gris, bleu, gris, gris]);
        assert!((c[2] - ACCENT_LUMINOSITE_MIN).abs() < 1e-6);
        assert!((c[0] / c[2] - 0.2).abs() < 1e-6, "teinte conservée : {c:?}");
        // Déjà assez claire : rendue telle quelle.
        let vif = [0.9, 0.3, 0.1];
        assert_eq!(accent_de(&[gris, vif, gris, gris, gris]), vif);
        // Tout noir : pas de division par zéro.
        assert_eq!(accent_de(&[[0.0; 3]; 5]), [ACCENT_LUMINOSITE_MIN; 3]);
    }

    #[test]
    fn reglages_au_hasard_restent_dans_les_bornes() {
        for graine in 0..200u64 {
            for (nom, v) in reglages_au_hasard(graine) {
                let r = REGLAGES.iter().find(|r| r.nom == nom).expect("curseur connu");
                assert!((r.min..=r.max).contains(&v), "{nom} = {v}");
            }
        }
        let couleurs: Vec<f32> = (0..200).map(|g| reglages_au_hasard(g)[8].1).collect();
        assert!(couleurs.contains(&3.0) && couleurs.contains(&5.0));
    }

    #[test]
    fn l_alea_nul_ne_touche_a_rien_et_reste_borne() {
        let mut v: [f32; REGLAGES.len()] = std::array::from_fn(|i| REGLAGES[i].defaut);
        let sans = avec_alea(&v, 123.0);
        assert_eq!(sans, [v[0], v[1], v[2], v[4], v[5], v[6]]);
        v[10] = 1.0;
        v[1] = 1.0;
        let mut a_bouge = false;
        for s in 0..600 {
            let [echelle, douceur, warp, _, _, saturation] = avec_alea(&v, s as f32 * 0.5);
            assert!(echelle >= REGLAGES[0].min && (0.0..=1.0).contains(&douceur) && (0.0..=1.0).contains(&warp));
            assert!(saturation >= 0.0);
            a_bouge |= (echelle - v[0]).abs() > 0.2;
        }
        assert!(a_bouge, "l'aléa au maximum doit faire dériver l'échelle");
    }

    #[test]
    fn l_origine_par_defaut_ne_decale_rien_et_l_errance_bouge() {
        let mut v: [f32; REGLAGES.len()] = std::array::from_fn(|i| REGLAGES[i].defaut);
        assert_eq!(decalage_origine(&v, 50.0, 2.0), [0.0, 0.0]);
        v[12] = 1.0;
        v[13] = 0.0;
        assert_eq!(decalage_origine(&v, 50.0, 2.0), [1.0, -0.5], "en unités de hauteur : x × aspect");
        v[14] = 1.0;
        let bouge = (0..200).any(|s| decalage_origine(&v, s as f32, 2.0) != [1.0, -0.5]);
        let borne = (0..200).all(|s| {
            let [x, y] = decalage_origine(&v, s as f32, 2.0);
            (x - 1.0).abs() <= ERRANCE_MAX * 2.0 + 1e-5 && (y + 0.5).abs() <= ERRANCE_MAX + 1e-5
        });
        assert!(bouge && borne);
    }

    #[test]
    fn reechantillonner_garde_les_extremites() {
        let c = [[0.0; 3], [1.0; 3], [0.5; 3], [0.2; 3], [0.9; 3]];
        assert_eq!(reechantillonner(&c, 5), c);
        let deux = reechantillonner(&c, 2);
        assert_eq!(deux[0], [0.0; 3]);
        assert_eq!(deux[2], [0.5; 3]);
        assert_eq!(deux[4], [1.0; 3]);
        let trois = reechantillonner(&c, 3);
        assert_eq!(trois[2], [1.0; 3]);
        assert_eq!(trois[4], [0.5; 3]);
        // Hors bornes : ramené à 2..5.
        assert_eq!(reechantillonner(&c, 0), deux);
        assert_eq!(reechantillonner(&c, 9), c);
    }

    #[test]
    fn palette_au_hasard_reste_sombre_et_varie() {
        for graine in 0..200u64 {
            let p = palette_au_hasard(graine);
            assert!(p.iter().flatten().all(|v| (0.0..=1.0).contains(v)), "{p:?}");
            assert!(p[0].iter().all(|v| *v <= 0.18 + 1e-6), "fond sombre : {p:?}");
            assert!(p.iter().flatten().all(|v| *v <= 0.7 + 1e-6), "{p:?}");
        }
        assert_eq!(palette_au_hasard(7), palette_au_hasard(7));
        assert_ne!(palette_au_hasard(7), palette_au_hasard(8));
    }

    #[test]
    fn hsv_couleurs_primaires() {
        assert_eq!(hsv(0.0, 1.0, 1.0), [1.0, 0.0, 0.0]);
        assert_eq!(hsv(1.0 / 3.0, 1.0, 1.0).map(|v| v.round()), [0.0, 1.0, 0.0]);
        assert_eq!(hsv(0.5, 0.0, 0.4), [0.4; 3]);
    }

    #[test]
    fn degrade_gpu_packing() {
        assert_eq!(size_of::<DegradeGpu>() % 16, 0);
        assert_eq!(std::mem::offset_of!(DegradeGpu, ecran), 16);
        assert_eq!(std::mem::offset_of!(DegradeGpu, couleurs), 64);
        assert_eq!(std::mem::offset_of!(DegradeGpu, graine), 144);
        // `origine` tombe sur la frontière de registre SUIVANTE, pas juste
        // après `lumiere` — voir `_remplissage`, qui existe pour ça.
        assert_eq!(std::mem::offset_of!(DegradeGpu, origine), 160);
        // `cosinus` se loge dans le REMPLISSAGE du registre d'`origine`, donc
        // l'ajouter ne décale rien — c'est le test qui le garantit.
        assert_eq!(std::mem::offset_of!(DegradeGpu, cosinus), 168);
        // Le deuxième registre est PLEIN : `origine` (8) + `cosinus` (4) +
        // `perspective` (4) = 16. Le prochain champ devra en ouvrir un
        // troisième, et ce test le dira.
        assert_eq!(std::mem::offset_of!(DegradeGpu, perspective), 172);
        // Le TROISIÈME registre : les quatre réglages des remous, pile 16 octets.
        assert_eq!(std::mem::offset_of!(DegradeGpu, remous), 176);
        assert_eq!(std::mem::offset_of!(DegradeGpu, remous_densite), 188);
        assert_eq!(size_of::<DegradeGpu>(), 192);
    }

    // Un seul test pour l'état global : deux tests en parallèle se
    // marcheraient dessus.
    #[test]
    fn preset_couleurs_et_reglages() {
        assert!(set_preset("Océan"));
        assert_eq!(preset_nom(), "Océan");
        assert!(!set_preset("Inexistant"));
        assert_eq!(preset_nom(), "Océan");
        // Le préréglage est COPIÉ (arrondi au 1/255 près).
        assert!((couleur(1)[1] - PRESETS[2].couleurs[1][1]).abs() < 0.003);

        assert!(set_couleur(4, [1.0, 0.0, 2.0]));
        assert_eq!(preset_nom(), PERSONNALISE);
        assert_eq!(couleur(4), [1.0, 0.0, 1.0]);
        assert!(!set_couleur(5, [0.0; 3]));

        assert!(set_reglage("grain", 9.0));
        assert_eq!(reglage(7), 0.2);
        assert!(!set_reglage("inconnu", 1.0));
        assert!(!set_reglage("warp", f32::NAN));
        reglages_par_defaut();
        assert_eq!(reglage(7), REGLAGES[7].defaut);
        assert!(set_preset("Aurore"));

        assert!(set_mode("Voronoï"));
        assert_eq!(mode_nom(), "Voronoï");
        assert!(!set_mode("Inconnu"));
        assert_eq!(mode_nom(), "Voronoï");
        assert!(set_mode("Mesh"));
    }
}
