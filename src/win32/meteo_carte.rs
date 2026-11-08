//! La carte du pays du panneau Météo — fond OpenStreetMap, radar de pluie,
//! vent, orage et neige.
//!
//! DEMANDE EXPLICITE (2026-10-23) : « avec l'api météo serait-il possible
//! d'ajouter des onglets pour Carte du pays avec radar pluie, vent, orage,
//! neige ? », puis « dessiné par nous, le fond, OpenStreetMap détaillé ».
//!
//! ## Trois sources, vérifiées par de vrais appels (2026-10-23)
//!
//! - **Le FOND** : les tuiles d'OpenStreetMap (`tile.openstreetmap.org/z/x/y.png`).
//!   Leur politique d'usage exige un agent qui NOMME l'application (voir
//!   `meteo::Client::ouvrir`) et un cache : une tuile est gardée `DUREE_FOND`
//!   sur disque, et jamais redemandée avant.
//! - **Le RADAR** : RainViewer, gratuit et sans clé. `weather-maps.json` liste 13
//!   images des deux dernières heures (une toutes les 10 min) ; leurs tuiles
//!   s'arrêtent au zoom 7 — au-delà, MESURÉ, la réponse est une image vide de
//!   1 370 octets. C'est ce qui fixe `ZOOM`.
//! - **VENT, ORAGE, NEIGE** : Open-Meteo, qui ne fait PAS de carte mais accepte
//!   une LISTE de points dans un seul appel (réponse : un tableau d'objets). Une
//!   grille de `GRILLE_COLONNES` × `GRILLE_RANGEES` points couvre la vue, heure
//!   par heure sur `HEURES` heures. Ce sont des PRÉVISIONS de modèle, pas des
//!   observations.
//!
//! ## Ce que ce module fait, et ce qu'il ne fait pas
//!
//! Il télécharge et range sur disque ; **il ne dessine rien**. La page
//! (`meteo.html`) pose les tuiles et peint par-dessus, à partir de la `Vue` (les
//! tuiles couvertes) et des nombres de la grille.
//!
//! ⚠️ **RIEN NE PART TANT QU'ON N'A PAS OUVERT UN ONGLET DE CARTE** (verbe
//! `carte` de la page) : un panneau Météo qu'on ne regarde jamais en carte ne
//! télécharge pas une tuile.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};

use crate::win32::meteo::Client;

/// Le zoom des tuiles — le plus fin que le radar sache servir (voir l'en-tête).
/// À ce zoom une tuile couvre 2,8° de longitude : `COLONNES` × `RANGEES` tuiles
/// tiennent un pays comme la France.
pub const ZOOM: u32 = 7;
/// Combien de tuiles la vue couvre, en largeur et en hauteur.
pub const COLONNES: i32 = 7;
pub const RANGEES: i32 = 5;
/// La grille de points demandée à Open-Meteo — 140 points en un seul appel.
const GRILLE_COLONNES: usize = 14;
const GRILLE_RANGEES: usize = 10;
/// Les heures de prévision de la grille.
const HEURES: usize = 24;
/// Combien d'images de radar on anime : la dernière heure (une toutes les 10 min).
const IMAGES_RADAR: usize = 7;
/// Une tuile de fond n'est jamais redemandée avant ce délai — la politique
/// d'OpenStreetMap demande de respecter le cache ; une carte de pays ne change pas
/// d'une semaine à l'autre.
const DUREE_FOND: Duration = Duration::from_secs(7 * 24 * 3600);
/// Le radar se renouvelle toutes les 10 minutes (la cadence de RainViewer) ; la
/// grille toutes les heures (son pas de temps).
const DUREE_RADAR: Duration = Duration::from_secs(600);
const DUREE_GRILLE: Duration = Duration::from_secs(3600);
/// Les dossiers d'images radar plus vieux que ceci sont effacés.
const GARDE_RADAR: Duration = Duration::from_secs(3 * 3600);

// ---------------------------------------------------------------------------
// La projection — Web Mercator, celle des tuiles
// ---------------------------------------------------------------------------

/// Longitude → abscisse en TUILES au zoom `z` (fractionnaire).
pub fn lon_vers_x(lon: f64, z: u32) -> f64 {
    (lon + 180.0) / 360.0 * f64::from(1u32 << z)
}

/// Latitude → ordonnée en TUILES au zoom `z` (fractionnaire, 0 au nord).
pub fn lat_vers_y(lat: f64, z: u32) -> f64 {
    let phi = lat.clamp(-85.0511, 85.0511).to_radians();
    (1.0 - (phi.tan() + 1.0 / phi.cos()).ln() / std::f64::consts::PI) / 2.0 * f64::from(1u32 << z)
}

pub fn x_vers_lon(x: f64, z: u32) -> f64 {
    x / f64::from(1u32 << z) * 360.0 - 180.0
}

pub fn y_vers_lat(y: f64, z: u32) -> f64 {
    let n = std::f64::consts::PI * (1.0 - 2.0 * y / f64::from(1u32 << z));
    n.sinh().atan().to_degrees()
}

/// **LES TUILES COUVERTES** — la première (en haut à gauche) et le nombre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vue {
    pub zoom: u32,
    pub x0: i32,
    pub y0: i32,
    pub colonnes: i32,
    pub rangees: i32,
    /// Le lieu du panneau, en tuiles — la page y pose un repère.
    pub centre: (f64, f64),
}

/// La vue CENTRÉE sur le lieu. ⚠️ Bornée verticalement : près des pôles, des
/// tuiles au-delà du monde n'existent pas.
pub fn vue_autour(lat: f64, lon: f64) -> Vue {
    let (cx, cy) = (lon_vers_x(lon, ZOOM), lat_vers_y(lat, ZOOM));
    let monde = 1i32 << ZOOM;
    let x0 = (cx - f64::from(COLONNES) / 2.0).floor() as i32;
    let y0 = ((cy - f64::from(RANGEES) / 2.0).floor() as i32).clamp(0, monde - RANGEES);
    Vue { zoom: ZOOM, x0, y0, colonnes: COLONNES, rangees: RANGEES, centre: (cx, cy) }
}

impl Vue {
    /// Les tuiles à télécharger, `(x, y)` — `x` ramené dans le monde (une vue à
    /// cheval sur l'antiméridien boucle).
    pub fn tuiles(&self) -> Vec<(i32, i32)> {
        let monde = 1i32 << self.zoom;
        let mut v = Vec::new();
        for y in self.y0..self.y0 + self.rangees {
            for x in self.x0..self.x0 + self.colonnes {
                v.push((x.rem_euclid(monde), y));
            }
        }
        v
    }

    /// Les points de la grille, `(latitude, longitude)`, au CENTRE de chaque case —
    /// rangée par rangée, du nord au sud.
    pub fn points_de_grille(&self) -> Vec<(f64, f64)> {
        let mut v = Vec::with_capacity(GRILLE_COLONNES * GRILLE_RANGEES);
        for r in 0..GRILLE_RANGEES {
            let y = f64::from(self.y0) + (r as f64 + 0.5) * f64::from(self.rangees) / GRILLE_RANGEES as f64;
            for c in 0..GRILLE_COLONNES {
                let x = f64::from(self.x0) + (c as f64 + 0.5) * f64::from(self.colonnes) / GRILLE_COLONNES as f64;
                v.push((y_vers_lat(y, self.zoom), x_vers_lon(x, self.zoom)));
            }
        }
        v
    }
}

// ---------------------------------------------------------------------------
// Le modèle
// ---------------------------------------------------------------------------

/// Un point de la grille et ses `HEURES` valeurs horaires.
#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    pub lat: f64,
    pub lon: f64,
    pub pluie: Vec<f64>,
    pub neige: Vec<f64>,
    pub vent: Vec<f64>,
    pub direction: Vec<f64>,
    pub cape: Vec<f64>,
    /// ⚠️ **−1 QUAND L'API NE SAIT PAS** : `lightning_potential` n'existe que dans
    /// le domaine des modèles qui le calculent (ICON, Europe). Ailleurs elle rend
    /// `null`, et 0 voudrait dire « aucun risque » — un mensonge. La page retombe
    /// alors sur l'énergie orageuse (`cape`).
    pub foudre: Vec<f64>,
    pub code: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Grille {
    /// Les heures, « 14:00 », dans le fuseau du premier point.
    pub heures: Vec<String>,
    pub points: Vec<Point>,
}

/// Une image du radar : son heure (secondes Unix) et le dossier de ses tuiles.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageRadar {
    pub temps: i64,
    pub dossier: PathBuf,
}

#[derive(Default)]
pub struct Carte {
    pub vue: Option<Vue>,
    pub radar: Vec<ImageRadar>,
    pub grille: Option<Grille>,
    pub chargement: bool,
    pub erreur: Option<String>,
    /// Change à CHAQUE nouvelle — une part de la clé de `cef_meteo::push_state`.
    pub version: u64,
    ordres: Option<Sender<Ordre>>,
    nouvelles: Option<Receiver<Nouvelle>>,
}

enum Ordre {
    /// Charge (ou complète) la carte autour de ce lieu.
    Charger { lat: f64, lon: f64 },
}

enum Nouvelle {
    Fond(Vue),
    Radar(Vec<ImageRadar>),
    Grille(Box<Grille>),
    Fin,
    Echec(String),
}

thread_local! {
    static ETAT: RefCell<Carte> = RefCell::new(Carte::default());
}

// ---------------------------------------------------------------------------
// Le cycle de vie
// ---------------------------------------------------------------------------

/// **DEMANDÉE PAR LA PAGE** (verbe `carte`) à l'ouverture d'un onglet de carte.
/// Le fil n'est lancé qu'au premier appel : rien ne tourne pour un panneau qu'on
/// ne regarde jamais en carte.
pub fn demander(lat: f64, lon: f64) {
    ETAT.with(|e| {
        let mut c = e.borrow_mut();
        if c.ordres.is_none() {
            let (tx_ordres, rx_ordres) = std::sync::mpsc::channel();
            let (tx_nouvelles, rx_nouvelles) = std::sync::mpsc::channel();
            match std::thread::Builder::new()
                .name("neuradesk-meteo-carte".into())
                .spawn(move || fil_de_carte(rx_ordres, tx_nouvelles))
            {
                Ok(_) => {
                    c.ordres = Some(tx_ordres);
                    c.nouvelles = Some(rx_nouvelles);
                }
                Err(e) => {
                    c.erreur = Some(format!("le fil de la carte n'a pas pu démarrer : {e}"));
                    c.version += 1;
                    return;
                }
            }
        }
        c.chargement = true;
        c.version += 1;
        if let Some(ordres) = &c.ordres {
            let _ = ordres.send(Ordre::Charger { lat, lon });
        }
    });
}

/// Recueille ce que le fil a rapporté — à chaque frame, sans jamais attendre.
pub fn avancer() {
    ETAT.with(|e| {
        let Ok(mut c) = e.try_borrow_mut() else { return };
        let Some(nouvelles) = c.nouvelles.take() else { return };
        for nouvelle in nouvelles.try_iter() {
            match nouvelle {
                Nouvelle::Fond(vue) => {
                    // Un AUTRE lieu : ce qui a été relevé pour l'ancien n'a plus de sens.
                    if c.vue != Some(vue) {
                        c.radar.clear();
                        c.grille = None;
                    }
                    c.vue = Some(vue);
                }
                Nouvelle::Radar(images) => c.radar = images,
                Nouvelle::Grille(grille) => c.grille = Some(*grille),
                Nouvelle::Fin => {
                    c.chargement = false;
                    c.erreur = None;
                }
                Nouvelle::Echec(message) => {
                    c.chargement = false;
                    c.erreur = Some(message);
                }
            }
            c.version += 1;
        }
        c.nouvelles = Some(nouvelles);
    });
}

pub fn version() -> u64 {
    ETAT.with(|e| e.try_borrow().map(|c| c.version).unwrap_or(0))
}

/// `file:///C:/…` — l'adresse qu'une page CEF chargée en `file://` sait lire.
fn url_fichier(chemin: &Path) -> String {
    format!("file:///{}", chemin.display().to_string().replace('\\', "/"))
}

/// Ce que la page reçoit, sous la clé `carte` de l'état (voir `cef_meteo`).
pub fn json() -> Value {
    ETAT.with(|e| {
        let Ok(c) = e.try_borrow() else { return Value::Null };
        let racine = racine_du_cache();
        json!({
            "chargement": c.chargement,
            "erreur": c.erreur,
            "vue": c.vue.map(|v| json!({
                "zoom": v.zoom, "x0": v.x0, "y0": v.y0,
                "colonnes": v.colonnes, "rangees": v.rangees,
                "monde": 1i32 << v.zoom,
                "centre": [v.centre.0, v.centre.1],
            })),
            "fond": racine.as_ref().map(|r| url_fichier(&r.join("osm"))),
            "radar": c.radar.iter().map(|i| json!({ "temps": i.temps, "dossier": url_fichier(&i.dossier) })).collect::<Vec<_>>(),
            "grille": c.grille.as_ref().map(|g| json!({
                "heures": g.heures,
                "points": g.points.iter().map(|p| json!({
                    "lat": p.lat, "lon": p.lon,
                    "pluie": p.pluie, "neige": p.neige, "vent": p.vent,
                    "direction": p.direction, "cape": p.cape, "foudre": p.foudre, "code": p.code,
                })).collect::<Vec<_>>(),
            })),
        })
    })
}

// ---------------------------------------------------------------------------
// Le fil de téléchargement
// ---------------------------------------------------------------------------

fn racine_du_cache() -> Option<PathBuf> {
    crate::config::cache_path("carte_meteo")
}

/// **LE SEUL ENDROIT OÙ CE MODULE TOUCHE AU RÉSEAU.** Charge à la demande, puis
/// renouvelle le radar toutes les `DUREE_RADAR` et la grille toutes les
/// `DUREE_GRILLE`, pour le dernier lieu demandé. Se termine quand le canal se
/// ferme (même mécanisme que `meteo::fil_de_meteo`).
fn fil_de_carte(ordres: Receiver<Ordre>, nouvelles: Sender<Nouvelle>) {
    let mut lieu: Option<(f64, f64)> = None;
    let mut grille_recue: Option<(Vue, Instant)> = None;
    let mut radar_recu: Option<Instant> = None;
    loop {
        let ordre = ordres.recv_timeout(DUREE_RADAR);
        match ordre {
            Ok(Ordre::Charger { lat, lon }) => lieu = Some((lat, lon)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
        let Some((lat, lon)) = lieu else { continue };
        let Some(racine) = racine_du_cache() else {
            let _ = nouvelles.send(Nouvelle::Echec("aucun dossier de cache".into()));
            continue;
        };
        let client = match Client::ouvrir("meteo-carte") {
            Ok(c) => c,
            Err(e) => {
                let _ = nouvelles.send(Nouvelle::Echec(e));
                continue;
            }
        };
        let vue = vue_autour(lat, lon);
        let mut erreurs = Vec::new();

        // 1. LE FOND — d'abord, c'est ce qui donne une carte à regarder.
        if let Err(e) = telecharger_fond(&client, &racine, &vue) {
            erreurs.push(e);
        }
        let _ = nouvelles.send(Nouvelle::Fond(vue));

        // 2. LE RADAR — de la plus récente à la plus ancienne, et chaque image
        // annoncée dès qu'elle est là : la dernière se voit sans attendre l'heure.
        if radar_recu.is_none_or(|t| t.elapsed() >= DUREE_RADAR) || ordre.is_ok() {
            match telecharger_radar(&client, &racine, &vue, &nouvelles) {
                Ok(()) => radar_recu = Some(Instant::now()),
                Err(e) => erreurs.push(e),
            }
        }

        // 3. LA GRILLE — une heure de validité, ou un autre lieu.
        if grille_recue.is_none_or(|(v, t)| v != vue || t.elapsed() >= DUREE_GRILLE) {
            match client_get_json(&client, &url_grille(&vue)).and_then(|v| grille_depuis(&v)) {
                Ok(grille) => {
                    grille_recue = Some((vue, Instant::now()));
                    let _ = nouvelles.send(Nouvelle::Grille(Box::new(grille)));
                }
                Err(e) => erreurs.push(format!("prévisions de la carte : {e}")),
            }
        }

        let _ = nouvelles.send(if erreurs.is_empty() { Nouvelle::Fin } else { Nouvelle::Echec(erreurs.join(" · ")) });
    }
}

fn client_get_json(client: &Client, url: &str) -> Result<Value, String> {
    let (statut, corps) = client.get(url, "application/json")?;
    if statut != 200 {
        return Err(format!("l'API a répondu {statut}"));
    }
    serde_json::from_str(&String::from_utf8_lossy(&corps)).map_err(|e| format!("réponse illisible : {e}"))
}

/// Une tuile : téléchargée et écrite seulement si elle manque (ou si elle a plus
/// de `duree`). ⚠️ Écrite À CÔTÉ puis RENOMMÉE : la page peut lire le fichier au
/// même instant, et une tuile à moitié écrite s'afficherait coupée.
fn tuile(client: &Client, url: &str, chemin: &Path, duree: Option<Duration>) -> Result<(), String> {
    let fraiche = std::fs::metadata(chemin)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| duree.is_none_or(|d| age < d));
    if fraiche {
        return Ok(());
    }
    let (statut, octets) = client.get(url, "image/png")?;
    if statut != 200 {
        return Err(format!("{url} : {statut}"));
    }
    if let Some(dossier) = chemin.parent() {
        std::fs::create_dir_all(dossier).map_err(|e| format!("{} : {e}", dossier.display()))?;
    }
    let provisoire = chemin.with_extension("part");
    std::fs::write(&provisoire, &octets).map_err(|e| format!("{} : {e}", provisoire.display()))?;
    std::fs::rename(&provisoire, chemin).map_err(|e| format!("{} : {e}", chemin.display()))
}

fn telecharger_fond(client: &Client, racine: &Path, vue: &Vue) -> Result<(), String> {
    let mut echecs = 0;
    for (x, y) in vue.tuiles() {
        let url = format!("https://tile.openstreetmap.org/{}/{x}/{y}.png", vue.zoom);
        let chemin = racine.join("osm").join(vue.zoom.to_string()).join(x.to_string()).join(format!("{y}.png"));
        if tuile(client, &url, &chemin, Some(DUREE_FOND)).is_err() {
            echecs += 1;
        }
    }
    if echecs > 0 {
        return Err(format!("{echecs} tuiles de fond manquent"));
    }
    Ok(())
}

/// Les images du radar annoncées par RainViewer : `(temps, url de base)`, de la
/// plus ANCIENNE à la plus récente — l'ordre de l'API.
fn images_radar_depuis(valeur: &Value) -> Result<Vec<(i64, String)>, String> {
    let hote = valeur.get("host").and_then(Value::as_str).ok_or("pas d'hôte radar")?;
    let passees = valeur.pointer("/radar/past").and_then(Value::as_array).ok_or("pas d'images radar")?;
    Ok(passees
        .iter()
        .filter_map(|i| Some((i.get("time")?.as_i64()?, format!("{hote}{}", i.get("path")?.as_str()?))))
        .collect())
}

fn telecharger_radar(client: &Client, racine: &Path, vue: &Vue, nouvelles: &Sender<Nouvelle>) -> Result<(), String> {
    let index = client_get_json(client, "https://api.rainviewer.com/public/weather-maps.json")?;
    let mut annoncees = images_radar_depuis(&index)?;
    let garder = annoncees.len().saturating_sub(IMAGES_RADAR);
    annoncees.drain(..garder);
    let dossier_radar = racine.join("radar");
    purger_radar(&dossier_radar);

    let mut pretes: Vec<ImageRadar> = Vec::new();
    for (temps, base) in annoncees.iter().rev() {
        let dossier = dossier_radar.join(temps.to_string());
        for (x, y) in vue.tuiles() {
            // `256` : la taille ; `2` : la palette « universelle » ; `1_1` : lissée,
            // et la NEIGE dans sa propre teinte.
            let url = format!("{base}/256/{}/{x}/{y}/2/1_1.png", vue.zoom);
            let chemin = dossier.join(vue.zoom.to_string()).join(x.to_string()).join(format!("{y}.png"));
            tuile(client, &url, &chemin, None)?;
        }
        pretes.insert(0, ImageRadar { temps: *temps, dossier });
        let _ = nouvelles.send(Nouvelle::Radar(pretes.clone()));
    }
    Ok(())
}

/// Efface les images de radar plus vieilles que `GARDE_RADAR` : un dossier par
/// image et toutes les 10 minutes, sans purge le cache grossirait sans fin.
fn purger_radar(dossier: &Path) {
    let Ok(entrees) = std::fs::read_dir(dossier) else { return };
    let maintenant = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    for entree in entrees.flatten() {
        let temps = entree.file_name().to_string_lossy().parse::<i64>().ok();
        if temps.is_some_and(|t| maintenant - t > GARDE_RADAR.as_secs() as i64) {
            let _ = std::fs::remove_dir_all(entree.path());
        }
    }
}

/// L'URL de la grille — **UNE SEULE DÉFINITION**, comme `meteo::url_previsions`.
fn url_grille(vue: &Vue) -> String {
    let points = vue.points_de_grille();
    let lats: Vec<String> = points.iter().map(|(la, _)| format!("{la:.3}")).collect();
    let lons: Vec<String> = points.iter().map(|(_, lo)| format!("{lo:.3}")).collect();
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}\
         &hourly=precipitation,snowfall,wind_speed_10m,wind_direction_10m,cape,lightning_potential,weather_code\
         &timezone=auto&forecast_hours={HEURES}",
        lats.join(","),
        lons.join(",")
    )
}

/// Lit la réponse de la grille — un TABLEAU d'objets, un par point, dans l'ordre
/// demandé.
fn grille_depuis(valeur: &Value) -> Result<Grille, String> {
    let liste = valeur.as_array().ok_or("la grille n'est pas un tableau")?;
    let serie = |h: &Value, cle: &str, absent: f64| -> Vec<f64> {
        h.get(cle)
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(absent)).collect())
            .unwrap_or_else(|| vec![absent; HEURES])
    };
    let mut heures = Vec::new();
    let mut points = Vec::with_capacity(liste.len());
    for (i, objet) in liste.iter().enumerate() {
        let horaire = objet.get("hourly").ok_or(format!("le point {i} n'a pas de bloc `hourly`"))?;
        if i == 0 {
            heures = horaire
                .get("time")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(|t| t.get(11..16).unwrap_or(t).to_string()).collect())
                .unwrap_or_default();
        }
        points.push(Point {
            lat: objet.get("latitude").and_then(Value::as_f64).ok_or(format!("le point {i} n'a pas de latitude"))?,
            lon: objet.get("longitude").and_then(Value::as_f64).ok_or(format!("le point {i} n'a pas de longitude"))?,
            pluie: serie(horaire, "precipitation", 0.0),
            neige: serie(horaire, "snowfall", 0.0),
            vent: serie(horaire, "wind_speed_10m", 0.0),
            direction: serie(horaire, "wind_direction_10m", 0.0),
            cape: serie(horaire, "cape", 0.0),
            foudre: serie(horaire, "lightning_potential", -1.0),
            code: serie(horaire, "weather_code", 0.0).into_iter().map(|c| c as i64).collect(),
        });
    }
    if points.is_empty() {
        return Err("la grille est vide".into());
    }
    Ok(Grille { heures, points })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La projection aller-retour : une tuile, puis le point qu'elle désigne.
    #[test]
    fn la_projection_fait_l_aller_retour() {
        for (lat, lon) in [(45.76, 4.83), (48.85, 2.35), (-33.9, 151.2), (0.0, 0.0)] {
            let (x, y) = (lon_vers_x(lon, ZOOM), lat_vers_y(lat, ZOOM));
            assert!((x_vers_lon(x, ZOOM) - lon).abs() < 1e-9, "longitude {lon}");
            assert!((y_vers_lat(y, ZOOM) - lat).abs() < 1e-9, "latitude {lat}");
        }
        // Repères connus : au zoom 7, Lyon est dans la tuile (65, 45).
        assert_eq!((lon_vers_x(4.83, 7) as i32, lat_vers_y(45.76, 7) as i32), (65, 45));
    }

    /// ⭐ La vue est centrée sur le lieu et couvre la France depuis Lyon.
    #[test]
    fn la_vue_autour_de_lyon_couvre_la_france() {
        let vue = vue_autour(45.76, 4.83);
        assert_eq!(vue.tuiles().len(), (COLONNES * RANGEES) as usize);
        let ouest = x_vers_lon(f64::from(vue.x0), ZOOM);
        let est = x_vers_lon(f64::from(vue.x0 + vue.colonnes), ZOOM);
        let nord = y_vers_lat(f64::from(vue.y0), ZOOM);
        let sud = y_vers_lat(f64::from(vue.y0 + vue.rangees), ZOOM);
        // Brest (-4,5°), Strasbourg (7,8°), Lille (50,6°), Perpignan (42,7°).
        assert!(ouest < -4.5 && est > 7.8, "de {ouest} à {est}");
        assert!(nord > 50.6 && sud < 42.7, "de {nord} à {sud}");
        let (cx, cy) = vue.centre;
        assert!(cx > f64::from(vue.x0) && cx < f64::from(vue.x0 + vue.colonnes));
        assert!(cy > f64::from(vue.y0) && cy < f64::from(vue.y0 + vue.rangees));
    }

    /// Près du pôle, la vue ne sort pas du monde ; à l'antiméridien, elle boucle.
    #[test]
    fn la_vue_reste_dans_le_monde() {
        let monde = 1i32 << ZOOM;
        let nord = vue_autour(84.0, 10.0);
        assert!(nord.y0 >= 0 && nord.y0 + nord.rangees <= monde);
        let fidji = vue_autour(-17.7, 179.9);
        assert!(fidji.tuiles().iter().all(|&(x, _)| (0..monde).contains(&x)), "x ramené dans le monde");
    }

    /// La grille : le bon nombre de points, tous DANS la vue.
    #[test]
    fn la_grille_couvre_la_vue() {
        let vue = vue_autour(45.76, 4.83);
        let points = vue.points_de_grille();
        assert_eq!(points.len(), GRILLE_COLONNES * GRILLE_RANGEES);
        for (lat, lon) in points {
            let (x, y) = (lon_vers_x(lon, ZOOM), lat_vers_y(lat, ZOOM));
            assert!(x > f64::from(vue.x0) && x < f64::from(vue.x0 + vue.colonnes));
            assert!(y > f64::from(vue.y0) && y < f64::from(vue.y0 + vue.rangees));
        }
        assert!(url_grille(&vue).contains("&forecast_hours=24"));
    }

    /// ⚠️ La foudre ABSENTE vaut −1, pas 0 ; un `null` ailleurs vaut 0.
    #[test]
    fn la_grille_se_lit_et_la_foudre_absente_n_est_pas_zero() {
        let reponse: Value = serde_json::from_str(
            r#"[{"latitude":45.75,"longitude":4.83,"hourly":{"time":["2026-10-23T14:00","2026-10-23T15:00"],
                "precipitation":[0.4,null],"snowfall":[0,0],"wind_speed_10m":[12,20],"wind_direction_10m":[270,280],
                "cape":[150,300],"lightning_potential":[null,null],"weather_code":[61,95]}},
               {"latitude":48.85,"longitude":2.35,"hourly":{"time":["2026-10-23T14:00","2026-10-23T15:00"],
                "precipitation":[0,0],"snowfall":[1.2,0],"wind_speed_10m":[5,6],"wind_direction_10m":[90,95],
                "cape":[0,0],"lightning_potential":[0,40],"weather_code":[71,3]}}]"#,
        )
        .unwrap();
        let g = grille_depuis(&reponse).unwrap();
        assert_eq!(g.heures, ["14:00", "15:00"]);
        assert_eq!(g.points.len(), 2);
        assert_eq!(g.points[0].pluie, [0.4, 0.0]);
        assert_eq!(g.points[0].foudre, [-1.0, -1.0], "absente : −1");
        assert_eq!(g.points[1].foudre, [0.0, 40.0]);
        assert_eq!(g.points[0].code, [61, 95]);
        assert!(grille_depuis(&json!([])).is_err(), "une grille vide se dit");
    }

    /// Les images du radar, dans l'ordre de l'API, avec leur URL de base.
    #[test]
    fn l_index_du_radar_se_lit() {
        let index = json!({
            "host": "https://tilecache.rainviewer.com",
            "radar": { "past": [ {"time": 100, "path": "/v2/radar/a"}, {"time": 700, "path": "/v2/radar/b"} ] }
        });
        assert_eq!(
            images_radar_depuis(&index).unwrap(),
            [(100, "https://tilecache.rainviewer.com/v2/radar/a".to_string()), (700, "https://tilecache.rainviewer.com/v2/radar/b".to_string())]
        );
        assert!(images_radar_depuis(&json!({})).is_err());
    }

    #[test]
    fn une_url_de_fichier_a_des_barres_obliques() {
        assert_eq!(url_fichier(Path::new(r"C:\a\b")), "file:///C:/a/b");
    }

    /// La sonde réseau — `cargo test -- --ignored sonde_carte`. Elle ne tourne pas
    /// d'office : aucun test de ce dépôt ne dépend du réseau.
    #[test]
    #[ignore]
    fn sonde_carte() {
        let client = Client::ouvrir("meteo-carte").unwrap();
        let g = client_get_json(&client, &url_grille(&vue_autour(45.76, 4.83))).and_then(|v| grille_depuis(&v)).unwrap();
        assert_eq!(g.points.len(), GRILLE_COLONNES * GRILLE_RANGEES);
        assert_eq!(g.heures.len(), HEURES);
    }
}
