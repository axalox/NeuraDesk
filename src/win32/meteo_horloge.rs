//! La météo ACTUELLE d'une ville, sous le calendrier du menu de l'horloge.
//!
//! DEMANDE EXPLICITE (2026-10-14) : « dans le menu en bas à gauche sous le
//! calendrier je voudrais avoir la météo actuelle pour une ville paramétrable dans
//! le dash réglages, le nom de la ville, la température et l'icône météo ».
//!
//! ⚠️ **UNE VILLE À ELLE, PAS CELLE DE L'APPLET MÉTÉO.** La ville se règle dans les
//! Réglages (un nom), celle de l'applet dans son panneau : changer l'une ne touche
//! pas l'autre. Le réseau, lui, est PARTAGÉ avec `win32::meteo` (`get_json`, le
//! géocodage, la table des codes WMO) — seule la requête diffère : on ne demande
//! que l'actuel.
//!
//! Même discipline que `win32::meteo` : un fil dédié, jamais de réseau sur le fil
//! de l'interface ; `avancer` draine ce qu'il rapporte à chaque frame.

use std::cell::RefCell;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use serde_json::{json, Value};

use crate::win32::meteo;

/// Le fichier de la ville — un nom, rien d'autre.
const FICHIER: &str = "meteo_horloge.txt";

/// La cadence de l'API (voir `meteo::INTERVALLE`, même raison).
const INTERVALLE: Duration = Duration::from_secs(900);

/// Les conditions du moment, telles que le menu les affiche.
#[derive(Clone, Debug, PartialEq)]
pub struct Actuel {
    /// « Lyon » — le nom rendu par le géocodage, pas forcément celui tapé.
    pub ville: String,
    pub temperature: f64,
    /// « °C ».
    pub unite: String,
    pub code: i64,
    pub jour: bool,
}

#[derive(Default)]
struct Etat {
    /// La ville saisie dans les Réglages — vide : rien d'affiché.
    ville: String,
    actuel: Option<Actuel>,
    erreur: Option<String>,
    /// Change à CHAQUE nouvelle — c'est la clé du cache de `cef_calendar`.
    version: u64,
    ordres: Option<Sender<String>>,
    nouvelles: Option<Receiver<Result<Actuel, String>>>,
}

thread_local! {
    static ETAT: RefCell<Etat> = RefCell::new(Etat::default());
}

/// Relit la ville et lance le fil — une fois, au démarrage (voir `app::run`).
pub fn charger() {
    let ville = crate::config::config_path(FICHIER)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| meteo::nettoyer_ville(&t))
        .unwrap_or_default();
    let (tx_ordres, rx_ordres) = std::sync::mpsc::channel();
    let (tx_nouvelles, rx_nouvelles) = std::sync::mpsc::channel();
    let pour_le_fil = ville.clone();
    let lance = std::thread::Builder::new()
        .name("neuradesk-meteo-horloge".into())
        .spawn(move || fil(pour_le_fil, rx_ordres, tx_nouvelles));
    if let Err(e) = &lance {
        crate::diag!("[meteo-horloge] le fil d'interrogation n'a pas pu démarrer : {e}");
    }
    ETAT.with(|e| {
        let mut m = e.borrow_mut();
        m.ville = ville;
        if lance.is_ok() {
            m.ordres = Some(tx_ordres);
            m.nouvelles = Some(rx_nouvelles);
        }
    });
}

/// Recueille ce que le fil a rapporté — à chaque frame, sans jamais attendre.
pub fn avancer() {
    ETAT.with(|e| {
        let Ok(mut m) = e.try_borrow_mut() else { return };
        let Some(nouvelles) = m.nouvelles.take() else { return };
        for nouvelle in nouvelles.try_iter() {
            match nouvelle {
                Ok(actuel) => {
                    m.actuel = Some(actuel);
                    m.erreur = None;
                }
                // Une panne n'efface PAS la dernière mesure : une température
                // d'il y a un quart d'heure vaut mieux qu'un trou.
                Err(message) => m.erreur = Some(message),
            }
            m.version += 1;
        }
        m.nouvelles = Some(nouvelles);
    });
}

/// La ville réglée (pour le champ des Réglages).
pub fn ville() -> String {
    ETAT.with(|e| e.borrow().ville.clone())
}

/// Change la ville — l'enregistre et la fait interroger tout de suite. Un nom
/// vide RETIRE la météo du menu.
pub fn changer_ville(nom: &str) {
    let nom = meteo::nettoyer_ville(nom);
    crate::config::write_config(FICHIER, nom.clone());
    ETAT.with(|e| {
        let mut m = e.borrow_mut();
        m.ville = nom.clone();
        // L'ancienne mesure est celle d'une AUTRE ville : la garder afficherait
        // le nom de la nouvelle à côté de la température de l'ancienne.
        m.actuel = None;
        m.erreur = None;
        m.version += 1;
        if let Some(ordres) = &m.ordres {
            let _ = ordres.send(nom);
        }
    });
}

/// La version de l'état — voir `Etat::version`.
pub fn version() -> u64 {
    ETAT.with(|e| e.borrow().version)
}

/// Ce que le menu affiche : `null` sans ville réglée.
pub fn json() -> Value {
    ETAT.with(|e| {
        let m = e.borrow();
        if m.ville.is_empty() {
            return Value::Null;
        }
        json!({
            "ville": m.actuel.as_ref().map_or(m.ville.clone(), |a| a.ville.clone()),
            "temperature": m.actuel.as_ref().map(|a| a.temperature),
            "unite": m.actuel.as_ref().map(|a| a.unite.clone()),
            "code": m.actuel.as_ref().map(|a| a.code),
            "jour": m.actuel.as_ref().map_or(true, |a| a.jour),
            "libelle": m.actuel.as_ref().map(|a| meteo::libelle_wmo(a.code)),
            "erreur": m.erreur,
        })
    })
}

/// **LE SEUL ENDROIT OÙ LE RÉSEAU EXISTE.** Se termine quand le canal d'ordres
/// se ferme, à l'arrêt du processus (même mécanisme que `meteo::fil_de_meteo`).
fn fil(mut ville: String, ordres: Receiver<String>, nouvelles: Sender<Result<Actuel, String>>) {
    // Les coordonnées de `ville`, géocodées une fois par changement de nom.
    let mut lieu: Option<meteo::Candidat> = None;
    let interroger = |ville: &str, lieu: &mut Option<meteo::Candidat>| {
        if ville.is_empty() {
            return;
        }
        if lieu.is_none() {
            match meteo::geocoder(ville) {
                Ok(Some(trouve)) => *lieu = Some(trouve),
                Ok(None) => {
                    let _ = nouvelles.send(Err(format!("aucune ville ne s'appelle « {ville} »")));
                    return;
                }
                Err(e) => {
                    let _ = nouvelles.send(Err(e));
                    return;
                }
            }
        }
        let Some(l) = lieu.as_ref() else { return };
        let _ = nouvelles.send(meteo::get_json(&url_actuel(l)).and_then(|v| actuel_depuis(&v, &l.nom)));
    };
    interroger(&ville, &mut lieu);
    loop {
        match ordres.recv_timeout(INTERVALLE) {
            Ok(nom) => {
                ville = nom;
                lieu = None;
                interroger(&ville, &mut lieu);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => interroger(&ville, &mut lieu),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// L'URL de l'actuel SEUL — trois valeurs, rien de plus.
fn url_actuel(lieu: &meteo::Candidat) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,weather_code,is_day&timezone=auto",
        lieu.latitude, lieu.longitude
    )
}

/// Lit le bloc `current` d'une réponse.
fn actuel_depuis(valeur: &Value, ville: &str) -> Result<Actuel, String> {
    let actuel = valeur.get("current").ok_or("la réponse n'a pas de bloc `current`")?;
    let temperature = actuel.get("temperature_2m").and_then(Value::as_f64).ok_or("température absente")?;
    let code = actuel.get("weather_code").and_then(Value::as_i64).ok_or("code météo absent")?;
    let jour = actuel.get("is_day").and_then(Value::as_i64).unwrap_or(1) != 0;
    let unite = valeur
        .get("current_units")
        .and_then(|u| u.get("temperature_2m"))
        .and_then(Value::as_str)
        .unwrap_or("°C")
        .to_string();
    Ok(Actuel { ville: ville.to_string(), temperature, unite, code, jour })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La forme RÉELLE de la réponse (relevée sur l'API, 2026-10-14).
    #[test]
    fn l_actuel_se_lit_d_une_reponse_de_l_api() {
        let reponse = json!({
            "current_units": { "temperature_2m": "°C", "weather_code": "wmo code", "is_day": "" },
            "current": { "time": "2026-10-14T19:15", "interval": 900, "temperature_2m": 12.4, "weather_code": 3, "is_day": 0 },
        });
        let a = actuel_depuis(&reponse, "Lyon").unwrap();
        assert_eq!(a, Actuel { ville: "Lyon".into(), temperature: 12.4, unite: "°C".into(), code: 3, jour: false });
    }

    #[test]
    fn une_reponse_sans_temperature_est_une_erreur() {
        assert!(actuel_depuis(&json!({ "current": { "weather_code": 0 } }), "Lyon").is_err());
        assert!(actuel_depuis(&json!({}), "Lyon").is_err());
    }

    /// Sans ville réglée, le menu n'affiche RIEN (pas un bloc vide).
    #[test]
    fn sans_ville_le_menu_ne_montre_rien() {
        ETAT.with(|e| e.borrow_mut().ville.clear());
        assert_eq!(json(), Value::Null);
    }
}
