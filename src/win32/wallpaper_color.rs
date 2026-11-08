//! Extraction d'une couleur d'accentuation automatique depuis le fond
//! d'écran actuel — demande explicite : "ajoute la possibilité d'avoir un
//! choix de couleur automatique pour les barres, dashs et menu en fonction
//! du fond d'écran... cherches sur internet comment choisir la couleur en
//! fonction du fond d'écran".
//!
//! Algorithme inspiré de la Palette API d'Android/Material Design — la
//! référence de facto pour "extraire UNE bonne couleur d'accentuation
//! depuis une image" (voir <https://developer.android.com/develop/ui/views/graphics/palette-colors>
//! et la source d'`androidx.palette.graphics.Target`, qui documente les
//! constantes reprises ci-dessous). PAS une simple moyenne des pixels
//! (produit presque toujours un gris/marron terne — des teintes opposées
//! sur le cercle chromatique s'annulent au lieu de se départager), ni le
//! pixel le plus fréquent seul (souvent le ciel/un mur uni, rarement "une
//! bonne couleur d'accent") : un score pondéré qui favorise une couleur à
//! la fois SATURÉE, ni trop sombre ni trop claire, ET suffisamment présente
//! dans l'image (voir `score_candidate`) — exactement le profil "Vibrant"
//! d'Android, le plus proche de ce qu'on attend d'une "couleur
//! d'accentuation".
//!
//! Étapes : (1) lit le chemin du fond d'écran configuré via
//! `SPI_GETDESKWALLPAPER` — l'API Win32 DOCUMENTÉE, PAS le cache interne
//! `TranscodedWallpaper` (non documenté, format/emplacement jamais
//! garantis — voir la mémoire "vérifier les API non documentées avant de
//! deviner") ; (2) décode l'image (`image`, déjà une dépendance de ce
//! projet pour les icônes/miniatures du presse-papiers) et la réduit
//! fortement (voir `THUMBNAIL_MAX_DIMENSION`) — inutile de traiter des
//! millions de pixels pour n'en tirer qu'UNE seule couleur, et ça accélère
//! l'étape suivante d'un facteur énorme ; (3) regroupe les pixels du
//! miniature en un histogramme grossier de couleurs PROCHES en teinte/
//! saturation/luminosité (voir `quantize`) — une approximation simple du
//! "median cut" qu'utilise Android, largement suffisante pour n'extraire
//! qu'UNE seule couleur plutôt qu'une palette complète à 16 entrées ; (4)
//! note chaque couleur candidate contre la cible "Vibrant" (voir
//! `pick_best_candidate`) et retourne la meilleure.

use std::collections::HashMap;
use std::path::PathBuf;

use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETDESKWALLPAPER};

use crate::ui::color;

/// Longueur de tampon large pour `SPI_GETDESKWALLPAPER` — `MAX_PATH` (260)
/// suffit pour un chemin de fond d'écran classique.
const WALLPAPER_PATH_BUFFER_LEN: usize = 260;

/// Chemin du fond d'écran ACTUELLEMENT configuré (voir la doc de module sur
/// pourquoi `SPI_GETDESKWALLPAPER`, pas le cache `TranscodedWallpaper`) —
/// `None` si aucun (bureau à couleur unie, chaîne vide renvoyée par
/// Windows) ou si l'appel échoue.
fn current_wallpaper_path() -> Option<PathBuf> {
    let mut buffer = [0u16; WALLPAPER_PATH_BUFFER_LEN];
    unsafe {
        // Résultat ignoré : le tampon reste à zéro (donc vide une fois
        // converti) si l'appel échoue, ce qui se traite EXACTEMENT comme
        // "aucun fond d'écran" ci-dessous — pas besoin d'un chemin d'erreur
        // séparé.
        let _ = SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            buffer.len() as u32,
            Some(buffer.as_mut_ptr() as *mut _),
            Default::default(),
        );
    }
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    if end == 0 {
        return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(&buffer[..end])))
}

/// **L'EMPREINTE DU FOND D'ÉCRAN** : chemin, date de modification et taille —
/// de quoi voir qu'il a CHANGÉ sans le décoder (la famille « Material You (fond
/// d'écran) » la compare toutes les deux secondes). La date compte : en
/// diaporama, Windows réécrit le MÊME fichier.
pub fn signature() -> Option<(PathBuf, Option<std::time::SystemTime>, u64)> {
    let path = current_wallpaper_path()?;
    let meta = std::fs::metadata(&path).ok()?;
    Some((path, meta.modified().ok(), meta.len()))
}

/// Réduit l'image à cette dimension MAXIMALE (sur le plus grand côté) avant
/// analyse — Android limite lui aussi la bitmap analysée à 192px (voir
/// `androidx.palette.graphics.Palette.DEFAULT_RESIZE_BITMAP_MAX_DIMENSION`),
/// même ordre de grandeur repris ici : largement assez de pixels pour un
/// histogramme de couleurs fiable, sans décoder/parcourir une image en
/// pleine résolution pour n'en tirer qu'UNE seule couleur.
const THUMBNAIL_MAX_DIMENSION: u32 = 192;

/// Poids relatifs saturation/luminosité/population dans le score d'une
/// couleur candidate — repris tels quels d'`androidx.palette.graphics.
/// Target` (`WEIGHT_SATURATION`/`WEIGHT_LUMA`/`WEIGHT_POPULATION`), voir la
/// doc de module.
const WEIGHT_SATURATION: f32 = 0.24;
const WEIGHT_LUMA: f32 = 0.52;
const WEIGHT_POPULATION: f32 = 0.24;

/// Cible de saturation "Vibrant" d'Android (`androidx.palette.graphics.
/// Target.VIBRANT`) — commune aux trois bandes de luminosité ci-dessous
/// (`LUMA_BANDS`) : Android distingue Vibrant/DarkVibrant/LightVibrant
/// UNIQUEMENT par la bande de luminosité visée, la cible de saturation
/// reste la même dans les trois cas.
const VIBRANT_TARGET_SATURATION: f32 = 1.0;
const VIBRANT_MIN_SATURATION: f32 = 0.35;

/// Bornes MIN/MAX/CIBLE de luminosité des trois profils "Vibrant" d'Android
/// (`Target.VIBRANT`/`Target.DARK_VIBRANT`/`Target.LIGHT_VIBRANT`) —
/// ESSAYÉS DANS CET ORDRE par `pick_best_candidate`. BUG RÉEL SIGNALÉ EN
/// PRATIQUE ("avec un fond d'écran noir la fonction automatique ne
/// fonctionne pas") : la version précédente ne connaissait QUE la bande
/// "normale" (0.3..0.7) — un fond d'écran sombre (luminosité MOYENNE sous
/// 0.3) ne laissait alors AUCUNE couleur passer le filtre, quelle que soit
/// sa saturation, puisque `pick_best_candidate` n'assouplissait QUE la
/// saturation, jamais la luminosité. Une couleur candidate en dehors de
/// `min`/`max` est écartée avant même d'être notée (voir
/// `best_in_luma_band`), pas seulement pénalisée — comme dans la source
/// originale.
struct LumaBand {
    min: f32,
    max: f32,
    target: f32,
}
const LUMA_BANDS: [LumaBand; 3] = [
    LumaBand { min: 0.3, max: 0.7, target: 0.5 },   // Vibrant
    LumaBand { min: 0.0, max: 0.45, target: 0.26 }, // Dark Vibrant — fond d'écran sombre
    LumaBand { min: 0.55, max: 1.0, target: 0.74 }, // Light Vibrant — fond d'écran clair
];

/// Une couleur "représentative" d'un groupe de pixels proches (voir
/// `quantize`) — `rgb` est la MOYENNE réelle des pixels du groupe (pas
/// juste le centre du panier arrondi utilisé pour les regrouper), pour un
/// résultat final plus fidèle à l'image source. `saturation`/`luma`
/// recalculées sur cette moyenne, `population` : nombre de pixels
/// représentés.
struct Candidate {
    rgb: [f32; 3],
    saturation: f32,
    luma: f32,
    population: u32,
}

/// Regroupe les pixels de `img` en un histogramme grossier — voir la doc de
/// module ("approximation simple du median cut"). Paniers de 15° en teinte
/// (24 paniers sur tout le cercle chromatique), 5 crans en saturation et en
/// luminosité chacun : assez de résolution pour distinguer des couleurs
/// visiblement différentes, assez grossier pour qu'une même couleur
/// dominante légèrement bruitée (compression JPEG, dégradé de ciel...)
/// tombe dans le MÊME panier plutôt que de se disperser entre des dizaines
/// de candidates presque identiques qui se voleraient mutuellement de la
/// population.
fn quantize(img: &image::RgbImage) -> Vec<Candidate> {
    const HUE_BUCKETS: f32 = 24.0;
    const LEVEL_BUCKETS: f32 = 5.0;

    let mut buckets: HashMap<(u8, u8, u8), (u32, u64, u64, u64)> = HashMap::new();
    for pixel in img.pixels() {
        let [r, g, b] = pixel.0;
        let rgb = [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0];
        let (h, s, l) = color::rgb_to_hsl(rgb);
        let hue_bucket = ((h / 360.0 * HUE_BUCKETS) as u8).min(HUE_BUCKETS as u8 - 1);
        let sat_bucket = (s * (LEVEL_BUCKETS - 1.0)).round() as u8;
        let light_bucket = (l * (LEVEL_BUCKETS - 1.0)).round() as u8;
        let entry = buckets.entry((hue_bucket, sat_bucket, light_bucket)).or_insert((0, 0, 0, 0));
        entry.0 += 1;
        entry.1 += r as u64;
        entry.2 += g as u64;
        entry.3 += b as u64;
    }

    buckets
        .into_values()
        .map(|(population, sum_r, sum_g, sum_b)| {
            let rgb = [
                (sum_r as f32 / population as f32) / 255.0,
                (sum_g as f32 / population as f32) / 255.0,
                (sum_b as f32 / population as f32) / 255.0,
            ];
            let (_, saturation, luma) = color::rgb_to_hsl(rgb);
            Candidate { rgb, saturation, luma, population }
        })
        .collect()
}

/// Score pondéré d'une couleur candidate contre `band` (une des `LUMA_BANDS`,
/// saturation cible toujours `VIBRANT_TARGET_SATURATION`) — reprend
/// `invertDiff`/`weightedMean` d'`androidx.palette.graphics.Target` : `1 -
/// |valeur - cible|` pour chacun des deux axes continus (saturation,
/// luminosité — déjà dans 0.0..1.0, directement comparables à leur cible
/// sans normalisation supplémentaire), la population normalisée par la plus
/// peuplée des candidates, combinées par une moyenne pondérée (`WEIGHT_*`,
/// dont la somme vaut déjà 1.0 — la division reste explicite pour rester
/// correcte si ces poids changent un jour).
fn score_candidate(candidate: &Candidate, band: &LumaBand, max_population: u32) -> f32 {
    let saturation_score = 1.0 - (candidate.saturation - VIBRANT_TARGET_SATURATION).abs();
    let luma_score = 1.0 - (candidate.luma - band.target).abs();
    let population_score = candidate.population as f32 / max_population as f32;
    (saturation_score * WEIGHT_SATURATION + luma_score * WEIGHT_LUMA + population_score * WEIGHT_POPULATION)
        / (WEIGHT_SATURATION + WEIGHT_LUMA + WEIGHT_POPULATION)
}

/// Meilleure couleur candidate dans `band` (une des `LUMA_BANDS`) au-dessus
/// de `min_saturation` — voir `score_candidate`. `min_saturation` : borne
/// basse ASSOUPLIE au besoin par `pick_best_candidate` (voir sa doc — un
/// fond d'écran presque monochrome ne laisserait sinon AUCUNE candidate
/// passer le filtre "Vibrant" plein, ex. 0.35, et ne produirait jamais de
/// résultat).
fn best_in_luma_band(candidates: &[Candidate], band: &LumaBand, min_saturation: f32) -> Option<[f32; 3]> {
    let filtered: Vec<&Candidate> =
        candidates.iter().filter(|c| c.saturation >= min_saturation && c.luma >= band.min && c.luma <= band.max).collect();
    let max_population = filtered.iter().map(|c| c.population).max()?;
    filtered
        .iter()
        .map(|c| (score_candidate(c, band, max_population), c.rgb))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, rgb)| rgb)
}

/// Meilleure couleur candidate de `candidates` — essaie d'abord les trois
/// bandes de luminosité (`LUMA_BANDS`, dans l'ordre Vibrant/Dark/Light) à la
/// saturation "Vibrant" pleine (`VIBRANT_MIN_SATURATION`), puis L'ASSOUPLIT
/// PROGRESSIVEMENT (voir `best_in_luma_band`) sur les trois bandes à nouveau
/// si rien ne passe, jusqu'à n'exiger AUCUNE saturation minimale — un fond
/// d'écran presque monochrome (photo en noir et blanc, dégradé neutre...)
/// doit tout de même produire UNE couleur plutôt qu'aucune, quitte à ce
/// qu'elle soit peu saturée. Les trois bandes COUVRENT ENSEMBLE toute la
/// plage de luminosité 0.0..1.0 (`0.0..0.45` ∪ `0.3..0.7` ∪ `0.55..1.0`) :
/// au dernier niveau de saturation (0.0), UNE candidate est donc TOUJOURS
/// trouvée tant que `candidates` n'est pas vide — y compris pour un fond
/// d'écran QUASI ENTIÈREMENT noir ou blanc, qui produira alors cette
/// couleur presque noire/blanche elle-même (`ui::color::
/// dark_theme_from_base_color`/`compute_palette` gèrent déjà correctement
/// un accent noir pur, voir leurs tests). `None` UNIQUEMENT si
/// `candidates` est vide (échec de décodage/réduction de l'image, déjà
/// signalé séparément par `extract_accent_color`).
fn pick_best_candidate(candidates: &[Candidate]) -> Option<[f32; 3]> {
    for min_saturation in [VIBRANT_MIN_SATURATION, 0.15, 0.0] {
        for band in &LUMA_BANDS {
            if let Some(rgb) = best_in_luma_band(candidates, band, min_saturation) {
                return Some(rgb);
            }
        }
    }
    None
}

/// Extrait une couleur d'accentuation depuis l'image de fond d'écran
/// ACTUELLE — voir la doc de module pour l'algorithme complet. `None`
/// UNIQUEMENT si aucun fond d'écran n'est configuré (bureau à couleur
/// unie), ou si le fichier est illisible/dans un format non pris en charge
/// — voir `pick_best_candidate` : une image décodée avec succès produit
/// TOUJOURS une couleur, même quasi noire/blanche pour un fond d'écran
/// monochrome (voir sa doc — BUG RÉEL SIGNALÉ, "avec un fond d'écran noir
/// la fonction automatique ne fonctionne pas").
pub fn extract_accent_color() -> Option<[f32; 3]> {
    let path = current_wallpaper_path()?;
    let bytes = std::fs::read(&path).ok()?;
    let decoded = image::load_from_memory(&bytes).ok()?;
    let thumbnail = decoded.thumbnail(THUMBNAIL_MAX_DIMENSION, THUMBNAIL_MAX_DIMENSION).to_rgb8();
    let candidates = quantize(&thumbnail);
    pick_best_candidate(&candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_image(w: u32, h: u32, rgb: [u8; 3]) -> image::RgbImage {
        image::RgbImage::from_fn(w, h, |_, _| image::Rgb(rgb))
    }

    /// Régression : "avec un fond d'écran noir la fonction automatique ne
    /// fonctionne pas" — avant le correctif (une seule bande de luminosité,
    /// 0.3..0.7), AUCUNE couleur d'un fond d'écran uniformément noir ne
    /// passait jamais ce filtre, quel que soit le niveau de saturation
    /// essayé (voir `LUMA_BANDS`, doc détaillée).
    #[test]
    fn pure_black_wallpaper_returns_black_not_none() {
        let img = solid_image(64, 64, [0, 0, 0]);
        let candidates = quantize(&img);
        let result = pick_best_candidate(&candidates);
        assert!(result.is_some(), "should not be None for a decodable (if monochrome) image");
    }

    /// Même régression, mais pour le cas RÉEL le plus probable derrière le
    /// signalement : pas un noir parfait, mais un fond d'écran globalement
    /// SOMBRE avec une vraie couleur dedans (luminosité moyenne bien sous
    /// 0.3) — doit être détecté via la bande "Dark Vibrant", pas rejeté.
    #[test]
    fn dark_but_colorful_wallpaper_finds_dark_vibrant() {
        // Bleu marine dominant, mélangé à un peu de bruit quasi noir.
        let img = image::RgbImage::from_fn(64, 64, |x, _| {
            if x % 5 == 0 { image::Rgb([2, 2, 4]) } else { image::Rgb([10, 20, 90]) }
        });
        let candidates = quantize(&img);
        let result = pick_best_candidate(&candidates);
        let rgb = result.expect("should find a dark vibrant candidate");
        let (_, s, l) = color::rgb_to_hsl(rgb);
        assert!(s > 0.3, "extracted color should retain the navy saturation, got {s}");
        assert!(l < 0.3, "extracted color should stay dark, got {l}");
    }
}
