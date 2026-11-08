//! ⭐ **LA FAMILLE « MATERIAL YOU » QUI SUIT LE FOND D'ÉCRAN** — DEMANDE
//! EXPLICITE (2026-10-12) : « ajoute le mode Material You qui suit le fond
//! d'écran ».
//!
//! Ses couleurs viennent de `color::famille_material_you`, appliquée à la
//! couleur source du fond d'écran (`wallpaper_color::extract_accent_color`).
//! Ce module dit QUAND les recalculer :
//!
//! - **l'empreinte** du fond d'écran (`wallpaper_color::signature` : chemin,
//!   date, taille) est relue au plus toutes les deux secondes — un `stat`, rien
//!   de plus ;
//! - quand elle change, l'image est décodée **sur un fil à part** : un fond
//!   d'écran 4K se décode en une fraction de seconde, assez pour faire sauter
//!   des images d'une interface à 144 Hz ;
//! - chaque nouveau résultat avance une **génération**, que `cef_settings::poll`
//!   compare à celle qu'il a appliquée pour réappliquer la famille si elle est
//!   choisie.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::ui::color::{self, FamilleTheme};
use crate::win32::wallpaper_color;

/// Au plus une relecture de l'empreinte par intervalle.
const INTERVALLE: Duration = Duration::from_secs(2);

type Empreinte = (std::path::PathBuf, Option<std::time::SystemTime>, u64);

struct Etat {
    /// L'empreinte dont `famille` est tirée (ou en cours d'analyse).
    empreinte: Option<Empreinte>,
    famille: Option<FamilleTheme>,
    derniere_lecture: Option<Instant>,
}

static ETAT: Mutex<Etat> = Mutex::new(Etat { empreinte: None, famille: None, derniere_lecture: None });
static EN_COURS: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// La famille tirée du fond d'écran actuel — `None` tant qu'il n'a pas été
/// analysé, ou s'il n'y en a pas (bureau de couleur unie).
pub fn famille() -> Option<FamilleTheme> {
    ETAT.lock().ok()?.famille
}

/// Avance à chaque nouvelle analyse du fond d'écran.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// Relit l'empreinte si l'intervalle est écoulé, et lance l'analyse si le fond
/// d'écran a changé. À appeler aussi souvent qu'on veut : il ne fait rien la
/// plupart du temps.
pub fn rafraichir() {
    let Ok(mut etat) = ETAT.lock() else { return };
    if etat.derniere_lecture.is_some_and(|t| t.elapsed() < INTERVALLE) || EN_COURS.load(Ordering::Relaxed) {
        return;
    }
    etat.derniere_lecture = Some(Instant::now());
    let empreinte = wallpaper_color::signature();
    if empreinte == etat.empreinte {
        return;
    }
    etat.empreinte = empreinte.clone();
    drop(etat);
    EN_COURS.store(true, Ordering::Relaxed);
    let lance = std::thread::Builder::new().name("material-you".into()).spawn(move || {
        let famille = empreinte.and(wallpaper_color::extract_accent_color()).map(color::famille_material_you);
        if let Ok(mut etat) = ETAT.lock() {
            etat.famille = famille;
        }
        GENERATION.fetch_add(1, Ordering::Relaxed);
        EN_COURS.store(false, Ordering::Relaxed);
    });
    if lance.is_err() {
        EN_COURS.store(false, Ordering::Relaxed);
    }
}
