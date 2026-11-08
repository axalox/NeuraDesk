//! La météo — les prévisions d'Open-Meteo, pour le panneau de l'applet.
//!
//! DEMANDE EXPLICITE (2026-10-04) : « une nouvelle applet dans la barre de droite
//! dans la zone Intégrés, pour afficher la météo en utilisant l'api gratuite de
//! <https://api.open-meteo.com>, dans un dash ».
//!
//! ## Ce que ce module fait, et ce qu'il ne fait pas
//!
//! Il tient la VILLE choisie, interroge l'API, et met le résultat à disposition de
//! `win32::cef_meteo`. **Il ne dessine rien** : la table des codes WMO et leurs
//! libellés vivent dans `assets/popups/meteo.html`, comme les catégories de fêtes
//! vivent dans `calendar.html`. Rust passe des NOMBRES, la page fait les mots.
//!
//! ## Le réseau ne touche jamais le fil de l'interface
//!
//! Même discipline que `win32::surveillance`, dont ce module reprend le fil dédié,
//! les canaux, et jusqu'au découpage d'URL (`decouper_url`) et au ramasse-miettes
//! de handle (`Handle`) — ils sont déjà `pub(crate)` là-bas, et une TROISIÈME
//! copie n'aurait rien apporté. La seule chose qui manquait là-bas, c'est la
//! lecture du CORPS de la réponse : `surveillance` ne lit que le code de statut, et
//! `envoi_image` fait un POST. D'où `get_json`, qui est un GET **avec corps**.
//!
//! ⚠️ **Aucun test de ce module ne fait de réseau.** Les fonctions pures
//! (`bulletin_depuis`, `jour_dimanche_index`, …) sont éprouvées sur des réponses
//! FIGÉES, et la sonde réseau est `#[ignore]` (voir `mod tests`).

use std::cell::RefCell;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use serde_json::{json, Value};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_OPTION_IPV6_FAST_FALLBACK, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE,
};

use crate::win32::surveillance::{decouper_url, ouvrir};

/// Où la ville choisie est retenue.
pub const FICHIER: &str = "meteo.toml";

/// Délai de toute opération réseau. L'API répond en ~200 ms ; 12 s est large, et
/// c'est un PLAFOND, pas une attente.
const DELAI_RESEAU_MS: i32 = 12_000;

/// **15 MINUTES, ET C'EST LA CADENCE DE L'API ELLE-MÊME** : chaque réponse porte
/// `"interval": 900`, c'est-à-dire que la valeur « actuelle » ne bouge pas plus
/// vite. Interroger plus souvent ne donnerait rien de plus récent, seulement plus
/// de requêtes.
const INTERVALLE: Duration = Duration::from_secs(900);

/// Combien de jours et d'heures on demande — la page affiche exactement ça.
const JOURS: usize = 7;
const HEURES: usize = 24;

/// Combien de villes on propose quand le géocodage en rend plusieurs.
const CANDIDATS_MAX: usize = 6;

/// Longueur maximale d'un nom de ville saisi — au-delà, c'est une faute de frappe
/// ou une tentative, et l'API n'a rien à en faire.
const VILLE_MAX: usize = 80;

/// Combien de lieux favoris on garde — DEMANDE EXPLICITE (2026-10-19) : « un
/// maximum de 50 villes » (c'était 12, une rangée). Au-delà de quelques rangées,
/// la zone des favoris défile (voir `#favoris`, `meteo.html`).
const FAVORIS_MAX: usize = 50;

// ---------------------------------------------------------------------------
// Le modèle
// ---------------------------------------------------------------------------

/// La ville retenue, et ses coordonnées.
#[derive(Clone, Debug, PartialEq)]
pub struct Lieu {
    /// Ce que l'utilisateur a tapé, nettoyé — c'est lui qu'on remet dans le champ.
    pub ville: String,
    /// Ce qu'on AFFICHE — « Lyon, France ». Rempli par le géocodage : deux villes
    /// homonymes ne se distinguent que par là.
    pub libelle: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// Une ville proposée par le géocodage, avant que l'utilisateur tranche.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidat {
    pub nom: String,
    pub libelle: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// Les conditions du moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Conditions {
    pub temperature: f64,
    pub ressenti: f64,
    pub humidite: f64,
    pub precipitations: f64,
    pub vent: f64,
    /// Les RAFALES, distinctes du vent moyen : c'est elles qui font tomber une
    /// branche, et l'API les donne.
    pub rafales: f64,
    pub code: i64,
    /// Jour ou nuit — l'API le dit (`is_day`), et c'est ce qui choisit entre un
    /// soleil et une lune.
    pub jour: bool,
}

/// Une heure à venir.
#[derive(Clone, Debug, PartialEq)]
pub struct HeureCiel {
    /// « 17:00 » — déjà mis en forme.
    pub heure: String,
    pub temperature: f64,
    pub code: i64,
    pub probabilite: f64,
    /// **JOUR OU NUIT, POUR CETTE HEURE-LÀ.** L'API le donne aussi au pas horaire
    /// (`is_day`), et il le FAUT : sans lui, la bande des 24 h montre un soleil à
    /// 02:00 — c'est-à-dire une erreur visible, pas un détail.
    pub jour: bool,
}

/// Un jour à venir.
#[derive(Clone, Debug, PartialEq)]
pub struct JourCiel {
    /// « lun. 5 » — même raison.
    pub jour: String,
    pub code: i64,
    pub min: f64,
    pub max: f64,
    /// « 07:38 » et « 19:08 ».
    pub lever: String,
    pub coucher: String,
    /// **DÉJÀ MIS EN FORME** — « 11 h 31 ». ⚠️ L'API rend des SECONDES
    /// (`daylight_duration: 41449.53`), et l'afficher tel quel ne dit rien.
    pub jour_duree: String,
    /// « 16:28 » / « 01:04 » — **VIDES QUAND LA LUNE NE SE LÈVE PAS** de la
    /// journée : l'API rend alors `null`. Ce n'est pas une panne, c'est une
    /// journée sur deux.
    pub lever_lune: String,
    pub coucher_lune: String,
    /// « Dernier croissant » — voir `libelle_phase_lune`.
    pub phase_lune: String,
    /// La fraction BRUTE (0..1), pour l'icône de la page.
    pub phase_fraction: f64,
    pub uv_max: f64,
    /// Le cumul de pluie du jour, en mm.
    pub pluie: f64,
    pub rafales_max: f64,
}

/// **LA QUALITÉ DE L'AIR, QUI VIENT D'UN AUTRE SERVICE.**
///
/// ⚠️ Elle est demandée par une SECONDE requête, sur un autre hôte
/// (`air-quality-api.open-meteo.com`) — même maison, même gratuité, mais un autre
/// appel. C'est `Option` partout : son échec ne doit JAMAIS emporter les
/// prévisions, qui sont la raison d'être du panneau.
#[derive(Clone, Debug, PartialEq)]
pub struct Air {
    /// L'indice européen (EAQI), 0..100+ — voir les bandes dans la page.
    pub aqi: i64,
    pub pm2_5: f64,
    pub pm10: f64,
    /// Les pollens, **dans l'ordre d'affichage** — seulement ceux qui existent en
    /// Europe et en saison. Les valeurs nulles sont gardées ici : c'est la PAGE
    /// qui décide de n'en montrer que les non nuls (une ligne de six zéros
    /// n'apprend rien).
    pub pollens: Vec<Pollenne>,
    pub unites: UnitesAir,
}

/// Un pollen et sa concentration.
#[derive(Clone, Debug, PartialEq)]
pub struct Pollenne {
    /// « Aulne », « Bouleau »… — le libellé est FRANÇAIS et vit ici, comme les
    /// codes WMO.
    pub nom: &'static str,
    /// En grains/m³.
    pub grains: f64,
}

/// Les unités de l'air, **telles que l'API les donne** — même règle que pour la
/// météo : on ne les suppose pas, on les lit dans `current_units`.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitesAir {
    /// « μg/m³ ».
    pub particules: String,
    /// « grains/m³ ».
    pub pollens: String,
}

impl Default for UnitesAir {
    fn default() -> Self {
        Self { particules: "μg/m³".into(), pollens: "grains/m³".into() }
    }
}

/// Les unités, telles que l'API les donne — on ne les suppose pas.
#[derive(Clone, Debug, PartialEq)]
pub struct Unites {
    pub temperature: String,
    pub vent: String,
    pub precipitations: String,
}

impl Default for Unites {
    fn default() -> Self {
        Self { temperature: "°C".into(), vent: "km/h".into(), precipitations: "mm".into() }
    }
}

/// **TOUT CE QU'UNE RÉPONSE CONTIENT, DÉJÀ DÉMÊLÉ.**
///
/// ⚠️ L'API rend des **tableaux parallèles** (`hourly.time`, `hourly.temperature_2m`,
/// `hourly.weather_code`, …) qu'il faut faire correspondre par INDEX. Ce
/// rapprochement est fait ICI, une fois, sous test — jamais dans la page, où une
/// longueur qui divergerait décalerait silencieusement tous les jours.
#[derive(Clone, Debug, PartialEq)]
pub struct Bulletin {
    pub actuel: Conditions,
    pub heures: Vec<HeureCiel>,
    pub jours: Vec<JourCiel>,
    pub unites: Unites,
    /// **FACULTATIVE, ET ELLE DOIT LE RESTER** : elle vient d'un second appel, sur
    /// un autre hôte. Son absence laisse le panneau complet sur tout le reste.
    pub air: Option<Air>,
}

#[derive(Default)]
pub struct Meteo {
    pub lieu: Option<Lieu>,
    /// **LES LIEUX FAVORIS — DEMANDE EXPLICITE : « ajouter des lieux préférés et
    /// les choisir en un clic ».**
    ///
    /// ⚠️ **LE LIEU ACTIF N'EST PAS DANS CETTE LISTE** : il garde ses propres
    /// champs à la racine du fichier (voir `enregistrer`). Un favori se désigne
    /// donc par son RANG dans `favoris`, et « lequel est actif » se répond en
    /// comparant les COORDONNÉES — jamais les noms, qui ne sont pas uniques (trois
    /// villes s'appellent Lyon).
    pub favoris: Vec<Lieu>,
    pub bulletin: Option<Bulletin>,
    /// Les villes proposées, en attente d'un choix de l'utilisateur.
    pub candidats: Vec<Candidat>,
    /// La dernière panne, à montrer **sans effacer `bulletin`** : des prévisions
    /// d'il y a un quart d'heure valent mieux qu'un panneau vide.
    pub erreur: Option<String>,
    /// Une interrogation est en vol.
    pub chargement: bool,
    /// Change à CHAQUE nouvelle — la clé de `cef_meteo::push_state`.
    pub version: u64,
    ordres: Option<Sender<Ordre>>,
    nouvelles: Option<Receiver<Nouvelle>>,
}

impl Meteo {
    fn envoyer(&self, ordre: Ordre) {
        if let Some(ordres) = &self.ordres {
            let _ = ordres.send(ordre);
        }
    }
}

/// Ce que l'interface demande au fil de mesure.
enum Ordre {
    /// Géocode ce nom, puis propose les villes trouvées.
    Chercher(String),
    /// Une ville a été choisie : on la retient et on demande ses prévisions.
    Choisir(Candidat),
    /// Redemander les prévisions de la ville retenue.
    Rafraichir,
}

/// Ce que le fil rapporte.
enum Nouvelle {
    Candidats(Vec<Candidat>),
    Bulletin(Box<Bulletin>),
    Echec(String),
}

thread_local! {
    static ETAT: RefCell<Meteo> = RefCell::new(Meteo::default());
}

// ---------------------------------------------------------------------------
// Le cycle de vie
// ---------------------------------------------------------------------------

/// Relit `meteo.toml` et lance le fil de mesure — une fois, au démarrage (voir
/// `app::run`).
pub fn charger() {
    let texte = crate::config::config_path(FICHIER).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    let (lieu, favoris) = lieu_depuis_toml(&texte);
    let (tx_ordres, rx_ordres) = std::sync::mpsc::channel();
    let (tx_nouvelles, rx_nouvelles) = std::sync::mpsc::channel();
    let pour_le_fil = lieu.clone();
    let lance = std::thread::Builder::new()
        .name("neuradesk-meteo".into())
        .spawn(move || fil_de_meteo(pour_le_fil, rx_ordres, tx_nouvelles));
    if let Err(e) = &lance {
        crate::diag!("[meteo] le fil d'interrogation n'a pas pu démarrer : {e}");
    }
    ETAT.with(|e| {
        let mut m = e.borrow_mut();
        m.lieu = lieu.clone();
        m.favoris = favoris;
        m.chargement = lieu.is_some();
        if lance.is_ok() {
            m.ordres = Some(tx_ordres);
            m.nouvelles = Some(rx_nouvelles);
        }
    });
}

/// Lit l'état.
pub fn lire<R>(f: impl FnOnce(&Meteo) -> R) -> R {
    ETAT.with(|e| f(&e.borrow()))
}

/// Recueille ce que le fil a rapporté — à chaque frame (voir `app::run`), sans
/// jamais attendre.
pub fn avancer() {
    ETAT.with(|e| {
        let Ok(mut m) = e.try_borrow_mut() else { return };
        let Some(nouvelles) = m.nouvelles.take() else { return };
        for nouvelle in nouvelles.try_iter() {
            match nouvelle {
                Nouvelle::Candidats(villes) => {
                    m.chargement = false;
                    m.erreur = None;
                    m.candidats = villes;
                }
                Nouvelle::Bulletin(bulletin) => {
                    m.chargement = false;
                    m.erreur = None;
                    m.candidats.clear();
                    m.bulletin = Some(*bulletin);
                }
                Nouvelle::Echec(message) => {
                    m.chargement = false;
                    m.erreur = Some(message);
                }
            }
            m.version += 1;
        }
        m.nouvelles = Some(nouvelles);
    });
}

/// Demande les villes qui portent ce nom (voir `Ordre::Chercher`).
pub fn chercher_ville(nom: &str) {
    let nom = nettoyer_ville(nom);
    if nom.is_empty() {
        ETAT.with(|e| {
            let mut m = e.borrow_mut();
            m.erreur = Some("entrez un nom de ville".into());
            m.version += 1;
        });
        return;
    }
    ETAT.with(|e| {
        let mut m = e.borrow_mut();
        m.chargement = true;
        m.erreur = None;
        m.candidats.clear();
        m.version += 1;
        m.envoyer(Ordre::Chercher(nom));
    });
}

/// Retient une ville et demande ses prévisions.
pub fn choisir_ville(candidat: Candidat) {
    ETAT.with(|e| {
        let mut m = e.borrow_mut();
        m.chargement = true;
        m.erreur = None;
        m.candidats.clear();
        m.version += 1;
        m.lieu = Some(Lieu {
            ville: candidat.nom.clone(),
            libelle: candidat.libelle.clone(),
            latitude: candidat.latitude,
            longitude: candidat.longitude,
        });
        // C'est un GESTE : le fichier suit tout de suite, favoris compris.
        enregistrer(&m);
        m.envoyer(Ordre::Choisir(candidat));
    });
}

/// **BASCULE SUR UN FAVORI — UN SEUL GESTE, ET C'EST TOUT L'INTÉRÊT.**
///
/// ⚠️ **LE RANG VIENT DE LA PAGE, DONC IL PEUT ÊTRE HORS BORNES** : un état plus
/// vieux que la liste affichée, et `favoris[9]` ferait tomber le fil d'interface.
/// On relit la liste au moment du clic.
pub fn choisir_favori(rang: usize) {
    let choisi = lire(|m| m.favoris.get(rang).cloned());
    let Some(lieu) = choisi else {
        crate::diag!("[meteo] favori {rang} hors de la liste");
        return;
    };
    // Passer par `choisir_ville` fait tout : le lieu actif, l'écriture, ET la
    // nouvelle interrogation — un favori n'a pas de chemin à lui.
    choisir_ville(Candidat {
        nom: lieu.ville,
        libelle: lieu.libelle,
        latitude: lieu.latitude,
        longitude: lieu.longitude,
    });
}

/// Ajoute le lieu COURANT aux favoris — s'il n'y est pas déjà.
pub fn ajouter_favori() {
    ETAT.with(|e| {
        let mut m = e.borrow_mut();
        let Some(lieu) = m.lieu.clone() else {
            m.erreur = Some("aucun lieu à retenir".into());
            m.version += 1;
            return;
        };
        // ⚠️ **DÉJÀ LÀ : ON NE DIT RIEN.** Ce n'est pas une erreur — le bouton
        // n'est d'ailleurs montré que quand il manque (voir `meteo.html`), et un
        // message pour un geste sans effet serait du bruit.
        if m.favoris.iter().any(|f| meme_endroit(f, &lieu)) {
            return;
        }
        if m.favoris.len() >= FAVORIS_MAX {
            m.erreur = Some(format!("au plus {FAVORIS_MAX} lieux favoris"));
            m.version += 1;
            return;
        }
        m.favoris.push(lieu);
        m.version += 1;
        enregistrer(&m);
    });
}

/// Retire un favori. **NE TOUCHE PAS AU LIEU ACTIF** : retirer le favori qu'on est
/// en train de regarder le fait disparaître de la rangée, pas de l'écran — c'est
/// ce qu'on attend d'un « retirer des favoris », et l'inverse surprendrait.
pub fn retirer_favori(rang: usize) {
    ETAT.with(|e| {
        let mut m = e.borrow_mut();
        if rang >= m.favoris.len() {
            return;
        }
        m.favoris.remove(rang);
        m.version += 1;
        enregistrer(&m);
    });
}

/// Redemande les prévisions tout de suite — le geste « rafraîchir » du panneau.
pub fn rafraichir() {
    ETAT.with(|e| {
        let mut m = e.borrow_mut();
        if m.lieu.is_none() {
            return;
        }
        m.chargement = true;
        m.version += 1;
        m.envoyer(Ordre::Rafraichir);
    });
}

/// Ce que la page affiche — voir `cef_meteo::push_state`.
pub fn json() -> Value {
    lire(|m| {
        json!({
            "lieu": m.lieu.as_ref().map(|l| json!({ "ville": l.ville, "libelle": l.libelle })),
            "chargement": m.chargement,
            "erreur": m.erreur,
            "candidats": m.candidats.iter().map(|c| json!({
                "nom": c.nom, "libelle": c.libelle,
                "latitude": c.latitude, "longitude": c.longitude,
            })).collect::<Vec<_>>(),
            "favoris": m.favoris.iter().map(|f| json!({
                "ville": f.ville, "libelle": f.libelle,
                // « est-ce le lieu AFFICHÉ ? » — par les coordonnées, jamais par
                // le nom : trois villes s'appellent Lyon.
                "actif": m.lieu.as_ref().is_some_and(|l| meme_endroit(l, f)),
            })).collect::<Vec<_>>(),
            // Le lieu courant est-il DÉJÀ en favori ? C'est ce qui montre ou cache
            // le bouton « Ajouter » — le calculer ici évite à la page de comparer
            // des coordonnées en JavaScript.
            "lieuFavori": m.lieu.as_ref().is_some_and(|l| m.favoris.iter().any(|f| meme_endroit(f, l))),
            "bulletin": m.bulletin.as_ref().map(|b| json!({
                "actuel": {
                    "temperature": b.actuel.temperature,
                    "ressenti": b.actuel.ressenti,
                    "humidite": b.actuel.humidite,
                    "precipitations": b.actuel.precipitations,
                    "vent": b.actuel.vent,
                    "rafales": b.actuel.rafales,
                    "code": b.actuel.code,
                    // Le libellé part avec le code : la page n'a pas à porter la
                    // table des 28 codes WMO (voir `libelle_wmo`, et pourquoi).
                    "libelle": libelle_wmo(b.actuel.code),
                    "jour": b.actuel.jour,
                },
                "heures": b.heures.iter().map(|h| json!({
                    "heure": h.heure, "temperature": h.temperature,
                    "code": h.code, "libelle": libelle_wmo(h.code),
                    "probabilite": h.probabilite, "jour": h.jour,
                })).collect::<Vec<_>>(),
                "jours": b.jours.iter().map(|j| json!({
                    "jour": j.jour, "code": j.code, "libelle": libelle_wmo(j.code),
                    "min": j.min, "max": j.max,
                    "lever": j.lever, "coucher": j.coucher, "jourDuree": j.jour_duree,
                    "leverLune": j.lever_lune, "coucherLune": j.coucher_lune,
                    "phaseLune": j.phase_lune, "phaseFraction": j.phase_fraction,
                    "uvMax": j.uv_max, "pluie": j.pluie, "rafalesMax": j.rafales_max,
                })).collect::<Vec<_>>(),
                "unites": {
                    "temperature": b.unites.temperature,
                    "vent": b.unites.vent,
                    "precipitations": b.unites.precipitations,
                },
            })),
            // `null` quand la qualité de l'air n'a pas pu être obtenue — la page
            // n'affiche alors simplement pas le bloc.
            //
            // ⚠️ **`m.bulletin`, PAS `b`** : `b` est le paramètre de la fermeture
            // du `bulletin`, au-dessus, et il n'existe pas ici.
            "air": m.bulletin.as_ref().and_then(|b| b.air.as_ref()).map(|a| json!({
                "aqi": a.aqi,
                "pm2_5": a.pm2_5,
                "pm10": a.pm10,
                "pollens": a.pollens.iter().map(|p| json!({
                    "nom": p.nom, "grains": p.grains,
                })).collect::<Vec<_>>(),
                "unites": { "particules": a.unites.particules, "pollens": a.unites.pollens },
            })),
        })
    })
}

// ---------------------------------------------------------------------------
// Le fil d'interrogation
// ---------------------------------------------------------------------------

/// **LE SEUL ENDROIT OÙ LE RÉSEAU EXISTE.** Boucle sur un délai, et se termine
/// quand le canal d'ordres se ferme — c'est-à-dire quand le `Sender` retenu par
/// l'état du fil d'interface est détruit, à l'arrêt du processus (même mécanisme
/// que `surveillance::fil_de_mesure`, qui n'a pas non plus de `JoinHandle`).
fn fil_de_meteo(mut lieu: Option<Lieu>, ordres: Receiver<Ordre>, nouvelles: Sender<Nouvelle>) {
    // Une ville déjà configurée se rafraîchit tout de suite : au lancement, on ne
    // veut pas attendre le premier quart d'heure pour voir quelque chose.
    if lieu.is_some() {
        interroger(&mut lieu, &nouvelles, false);
    }
    loop {
        match ordres.recv_timeout(INTERVALLE) {
            Ok(Ordre::Chercher(nom)) => chercher(&nom, &nouvelles),
            Ok(Ordre::Choisir(candidat)) => {
                // ⚠️ **LE FIL DE MESURE N'ÉCRIT PLUS LE FICHIER** (voir
                // `vers_toml`) : il ne connaît pas les favoris, et écrire ici les
                // EFFACERAIT à chaque changement de ville. L'écriture appartient
                // au geste, côté fil d'interface (`choisir_ville`, `enregistrer`).
                lieu = Some(Lieu {
                    ville: candidat.nom.clone(),
                    libelle: candidat.libelle.clone(),
                    latitude: candidat.latitude,
                    longitude: candidat.longitude,
                });
                interroger(&mut lieu, &nouvelles, true);
            }
            Ok(Ordre::Rafraichir) => interroger(&mut lieu, &nouvelles, false),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => interroger(&mut lieu, &nouvelles, false),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// La PREMIÈRE ville qui porte ce nom — le géocodage sans choix, pour
/// `win32::meteo_horloge`. `Ok(None)` : aucune ville (l'API répond 200 sans
/// résultat, voir `candidats_depuis`).
pub(crate) fn geocoder(nom: &str) -> Result<Option<Candidat>, String> {
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=fr&format=json",
        encoder_query(nom)
    );
    get_json(&url).and_then(|v| candidats_depuis(&v)).map(|villes| villes.into_iter().next())
}

/// Géocode un nom et propose les villes trouvées.
fn chercher(nom: &str, nouvelles: &Sender<Nouvelle>) {
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count={CANDIDATS_MAX}&language=fr&format=json",
        encoder_query(nom)
    );
    match get_json(&url).and_then(|v| candidats_depuis(&v)) {
        Ok(villes) if villes.is_empty() => {
            let _ = nouvelles.send(Nouvelle::Echec(format!("aucune ville ne s'appelle « {nom} »")));
        }
        Ok(villes) => {
            let _ = nouvelles.send(Nouvelle::Candidats(villes));
        }
        Err(e) => {
            let _ = nouvelles.send(Nouvelle::Echec(e));
        }
    }
}

/// Interroge les prévisions de `lieu`.
///
/// `retentir` : après un CHOIX de ville, une panne laisserait l'utilisateur devant
/// un panneau vide et sans explication — on refait donc une tentative avant
/// d'abandonner. Au rafraîchissement périodique, non : les prévisions précédentes
/// sont encore à l'écran, et insister toutes les 15 minutes ne sert à rien.
fn interroger(lieu: &mut Option<Lieu>, nouvelles: &Sender<Nouvelle>, retentir: bool) {
    let Some(l) = lieu.as_ref() else { return };
    let url = url_previsions(l);
    let essais = if retentir { 2 } else { 1 };
    let mut dernier = String::new();
    for _ in 0..essais {
        match get_json(&url).and_then(|v| bulletin_depuis(&v)) {
            Ok(mut bulletin) => {
                // ⚠️ **LA QUALITÉ DE L'AIR EST UN SECOND APPEL, SUR UN AUTRE HÔTE,
                // ET SON ÉCHEC NE DOIT RIEN EMPORTER.** `Option` de bout en bout :
                // si elle tombe, le panneau reste complet sur tout le reste, avec
                // seulement ce bloc en moins. La faire remonter ferait perdre des
                // prévisions valides pour un bonus.
                bulletin.air = get_json(&url_air(l)).and_then(|v| air_depuis(&v)).ok();
                // ⚠️ **ON N'ÉCRIT PAS LE FICHIER ICI.** Un rafraîchissement
                // périodique réécrirait `meteo.toml` toutes les quinze minutes
                // pour rien : la ville n'a pas changé. L'écriture appartient au
                // CHOIX (voir `Ordre::Choisir`).
                let _ = nouvelles.send(Nouvelle::Bulletin(Box::new(bulletin)));
                return;
            }
            Err(e) => dernier = e,
        }
    }
    let _ = nouvelles.send(Nouvelle::Echec(dernier));
}

/// L'URL des prévisions — **UNE SEULE DÉFINITION**, pour que la sonde réseau et
/// le code de production interrogent exactement la même chose.
fn url_previsions(lieu: &Lieu) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}\
         &current=temperature_2m,relative_humidity_2m,apparent_temperature,is_day,precipitation,weather_code,wind_speed_10m,wind_gusts_10m\
         &hourly=temperature_2m,weather_code,precipitation_probability,is_day\
         &daily=weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset,daylight_duration,\
moonrise,moonset,moon_phase,uv_index_max,precipitation_sum,wind_gusts_10m_max\
         &timezone=auto&forecast_days={JOURS}&forecast_hours={HEURES}",
        lieu.latitude, lieu.longitude
    )
}

/// **L'URL DE LA QUALITÉ DE L'AIR — UN AUTRE HÔTE, LE SEUL DU MODULE.**
///
/// Même maison, même gratuité, même forme de réponse, mais un **second appel** :
/// c'est pour ça que `Bulletin::air` est facultatif, et que son échec n'emporte
/// jamais les prévisions.
fn url_air(lieu: &Lieu) -> String {
    format!(
        "https://air-quality-api.open-meteo.com/v1/air-quality?latitude={}&longitude={}\
         &current=european_aqi,pm10,pm2_5,alder_pollen,birch_pollen,grass_pollen,mugwort_pollen,olive_pollen,ragweed_pollen\
         &timezone=auto",
        lieu.latitude, lieu.longitude
    )
}

/// Les pollens suivis, **dans l'ordre d'affichage** — le libellé français vit ici,
/// comme les codes WMO. L'API n'en propose pas d'autres, et ils n'existent qu'en
/// Europe et en saison.
const POLLENS: [(&str, &str); 6] = [
    ("alder_pollen", "Aulne"),
    ("birch_pollen", "Bouleau"),
    ("grass_pollen", "Graminées"),
    ("mugwort_pollen", "Armoise"),
    ("olive_pollen", "Olivier"),
    ("ragweed_pollen", "Ambroisie"),
];

/// Lit la réponse de la qualité de l'air.
///
/// ⚠️ **TOUT Y EST FACULTATIF, MÊME L'INDICE.** Les pollens ne sont rendus qu'en
/// Europe et en saison : hors saison, l'API peut les omettre. Une absence ne doit
/// pas faire échouer ce bloc — elle le rend seulement plus pauvre.
fn air_depuis(valeur: &Value) -> Result<Air, String> {
    let actuel = valeur.get("current").ok_or("la réponse d'air n'a pas de bloc `current`")?;
    Ok(Air {
        // -1 : « l'API n'a rien dit ». La page ne montre alors pas d'indice, plutôt
        // que d'en inventer un.
        aqi: entier(actuel, "european_aqi").unwrap_or(-1),
        pm2_5: nombre(actuel, "pm2_5").unwrap_or(0.0),
        pm10: nombre(actuel, "pm10").unwrap_or(0.0),
        pollens: POLLENS.iter().map(|(cle, nom)| Pollenne { nom, grains: nombre(actuel, cle).unwrap_or(0.0) }).collect(),
        unites: valeur.get("current_units").map(unites_air_depuis).unwrap_or_default(),
    })
}

fn unites_air_depuis(valeur: &Value) -> UnitesAir {
    let defaut = UnitesAir::default();
    UnitesAir {
        particules: texte(valeur, "pm2_5").unwrap_or(defaut.particules),
        pollens: texte(valeur, "grass_pollen").unwrap_or(defaut.pollens),
    }
}

// ---------------------------------------------------------------------------
// Le réseau — un GET AVEC CORPS
// ---------------------------------------------------------------------------

/// **UN `GET` QUI LIT LE CORPS.** Bloquant : appelé depuis le fil d'interrogation
/// seulement.
///
/// ⚠️ **POURQUOI IL Y EN A UN TROISIÈME.** `surveillance::verifier_http` ne lit
/// que le code de statut — c'est tout ce qu'une sonde de disponibilité veut
/// savoir. `envoi_image::poster` lit le corps, mais fait un POST. Aucune des deux
/// ne fait un GET dont la RÉPONSE est la donnée ; c'est le cas ici, et c'est la
/// seule raison de cette fonction.
///
/// Le découpage d'URL et le ramasse-miettes de handle, eux, sont PARTAGÉS avec
/// `surveillance` (ils y sont `pub(crate)`) : réécrire l'un ou l'autre aurait été
/// une copie gratuite.
pub(crate) fn get_json(url: &str) -> Result<Value, String> {
    let (statut, corps) = Client::ouvrir("meteo")?.get(url, "application/json")?;
    if statut != 200 {
        return Err(format!("l'API a répondu {statut}"));
    }
    serde_json::from_str(&String::from_utf8_lossy(&corps)).map_err(|e| format!("réponse illisible : {e}"))
}

/// **UNE SESSION WinHTTP RÉUTILISABLE** — pour `win32::meteo_carte`, qui
/// télécharge des CENTAINES de tuiles d'affilée.
///
/// ⚠️ **C'EST LA SESSION QUI GARDE LES CONNEXIONS OUVERTES** : WinHTTP regroupe
/// ses sockets par session, et une session neuve par requête refaisait la
/// négociation TLS à chaque tuile. `get_json`, qui ne fait qu'un appel, en ouvre
/// une jetable.
pub(crate) struct Client {
    session: crate::win32::surveillance::Handle,
}

impl Client {
    /// `usage` complète l'agent (« NeuraDesk/x.y (meteo) ») : c'est ce qui permet
    /// à un service de reconnaître l'application dans ses journaux — et
    /// OpenStreetMap l'EXIGE (politique d'usage des tuiles).
    pub(crate) fn ouvrir(usage: &str) -> Result<Client, String> {
        let agent = HSTRING::from(format!("NeuraDesk/{} ({usage})", env!("CARGO_PKG_VERSION")));
        unsafe {
            let session = ouvrir(
                WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0),
                "session",
            )?;
            let _ = WinHttpSetTimeouts(session.0, DELAI_RESEAU_MS, DELAI_RESEAU_MS, DELAI_RESEAU_MS, DELAI_RESEAU_MS);
            // Même raison que dans `surveillance` : un hôte dont l'IPv6 ne répond pas
            // ferait attendre WinHTTP ~21 s avant de retomber sur l'IPv4.
            let _ = WinHttpSetOption(Some(session.0 as *const _), WINHTTP_OPTION_IPV6_FAST_FALLBACK, Some(&1u32.to_ne_bytes()));
            Ok(Client { session })
        }
    }

    /// **UN `GET` QUI LIT LE CORPS** — le statut et les octets, tels quels.
    /// Bloquant : appelé depuis un fil réseau seulement.
    pub(crate) fn get(&self, url: &str, accept: &str) -> Result<(u32, Vec<u8>), String> {
        let url = decouper_url(url).ok_or("URL illisible")?;
        unsafe {
            let connexion = ouvrir(WinHttpConnect(self.session.0, &HSTRING::from(url.hote.as_str()), url.port, 0), "connexion")?;
            let drapeaux = if url.securisee { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
            let demande = ouvrir(
                WinHttpOpenRequest(
                    connexion.0,
                    &HSTRING::from("GET"),
                    &HSTRING::from(url.chemin.as_str()),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    std::ptr::null(),
                    drapeaux,
                ),
                "requête",
            )?;
            // ⚠️ **L'EN-TÊTE `Accept` N'EST PAS DÉCORATIF** : avec l'agent, c'est ce
            // qui permet de reconnaître NeuraDesk dans les journaux du service si un
            // jour il fallait lui écrire.
            let entetes: Vec<u16> = format!("Accept: {accept}").encode_utf16().collect();
            WinHttpSendRequest(demande.0, Some(&entetes), None, 0, 0, 0).map_err(|e| format!("envoi : {e}"))?;
            WinHttpReceiveResponse(demande.0, std::ptr::null_mut()).map_err(|e| format!("réponse : {e}"))?;

            let mut statut = 0u32;
            let mut taille = std::mem::size_of::<u32>() as u32;
            WinHttpQueryHeaders(
                demande.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some((&mut statut as *mut u32).cast()),
                &mut taille,
                std::ptr::null_mut(),
            )
            .map_err(|e| format!("statut : {e}"))?;

            let mut corps = Vec::new();
            loop {
                let mut disponible = 0u32;
                WinHttpQueryDataAvailable(demande.0, &mut disponible).map_err(|e| format!("lecture : {e}"))?;
                if disponible == 0 {
                    break;
                }
                let debut = corps.len();
                corps.resize(debut + disponible as usize, 0);
                let mut lus = 0u32;
                WinHttpReadData(demande.0, corps[debut..].as_mut_ptr().cast(), disponible, &mut lus)
                    .map_err(|e| format!("lecture : {e}"))?;
                corps.truncate(debut + lus as usize);
            }
            Ok((statut, corps))
        }
    }
}

// ---------------------------------------------------------------------------
// La lecture des réponses
// ---------------------------------------------------------------------------

/// Les villes proposées par le géocodage.
///
/// ⚠️ **UNE VILLE INCONNUE N'EST PAS UNE ERREUR HTTP** : l'API répond 200 et
/// **omet simplement la clé `results`** (vérifié le 2026-10-04). La traiter comme
/// une panne ferait dire « l'API a échoué » là où il faut dire « aucune ville ne
/// s'appelle ainsi ».
fn candidats_depuis(valeur: &Value) -> Result<Vec<Candidat>, String> {
    let Some(liste) = valeur.get("results").and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    Ok(liste
        .iter()
        .take(CANDIDATS_MAX)
        .filter_map(|r| {
            let nom = r.get("name")?.as_str()?.to_string();
            let latitude = r.get("latitude")?.as_f64()?;
            let longitude = r.get("longitude")?.as_f64()?;
            Some(Candidat { libelle: libelle_du_lieu(r, &nom), nom, latitude, longitude })
        })
        .collect())
}

/// « Lyon, France » — ou « Lyon, Rhône-Alpes, France » quand la région existe et
/// diffère du pays. Deux homonymes ne se distinguent que par là : l'API en rend
/// trois « Lyon » (France, Mississippi, Missouri).
fn libelle_du_lieu(valeur: &Value, nom: &str) -> String {
    let pays = valeur.get("country").and_then(|v| v.as_str()).unwrap_or("");
    let region = valeur.get("admin1").and_then(|v| v.as_str()).unwrap_or("");
    match (region.is_empty() || region == nom, pays.is_empty()) {
        (_, true) => nom.to_string(),
        (true, false) => format!("{nom}, {pays}"),
        (false, false) => format!("{nom}, {region}, {pays}"),
    }
}

/// **LE CŒUR DE LA LECTURE.** Démêle les tableaux parallèles de l'API.
fn bulletin_depuis(valeur: &Value) -> Result<Bulletin, String> {
    let actuel = valeur.get("current").ok_or("la réponse n'a pas de bloc `current`")?;
    let conditions = Conditions {
        temperature: nombre(actuel, "temperature_2m")?,
        ressenti: nombre(actuel, "apparent_temperature")?,
        humidite: nombre(actuel, "relative_humidity_2m")?,
        precipitations: nombre(actuel, "precipitation")?,
        vent: nombre(actuel, "wind_speed_10m")?,
        rafales: nombre(actuel, "wind_gusts_10m").unwrap_or(0.0),
        code: entier(actuel, "weather_code")?,
        jour: entier(actuel, "is_day").unwrap_or(1) != 0,
    };

    let heures = bloc_parallele(valeur, "hourly", HEURES, |horodatage, i, bloc| {
        Ok(HeureCiel {
            heure: heure_de(horodatage).ok_or("horaire illisible")?,
            temperature: nombre_serie(bloc, "temperature_2m", i)?,
            code: entier_serie(bloc, "weather_code", i)?,
            // La probabilité est FACULTATIVE : l'API l'omet sur certaines
            // périodes, et une absence ne doit pas jeter tout le bulletin.
            probabilite: nombre_serie(bloc, "precipitation_probability", i).unwrap_or(0.0),
            // Une heure sans `is_day` est traitée comme le JOUR : c'est le cas par
            // défaut le moins surprenant, et l'API le rend toujours aujourd'hui.
            jour: entier_serie(bloc, "is_day", i).unwrap_or(1) != 0,
        })
    })?;

    let jours = bloc_parallele(valeur, "daily", JOURS, |horodatage, i, bloc| {
        let phase_fraction = nombre_serie(bloc, "moon_phase", i).unwrap_or(0.0);
        Ok(JourCiel {
            jour: jour_de(horodatage).ok_or("date illisible")?,
            code: entier_serie(bloc, "weather_code", i)?,
            min: nombre_serie(bloc, "temperature_2m_min", i)?,
            max: nombre_serie(bloc, "temperature_2m_max", i)?,
            lever: heure_serie(bloc, "sunrise", i),
            coucher: heure_serie(bloc, "sunset", i),
            jour_duree: duree_en_clair(nombre_serie(bloc, "daylight_duration", i).unwrap_or(0.0)),
            // ⚠️ **`moonrise` VAUT `null` CERTAINS JOURS** — la lune ne se lève pas
            // toutes les 24 h. `heure_serie` rend alors une chaîne vide, et la page
            // n'affiche simplement rien : ce n'est pas une panne, c'est une journée.
            lever_lune: heure_serie(bloc, "moonrise", i),
            coucher_lune: heure_serie(bloc, "moonset", i),
            phase_lune: libelle_phase_lune(phase_fraction).to_string(),
            phase_fraction,
            uv_max: nombre_serie(bloc, "uv_index_max", i).unwrap_or(0.0),
            pluie: nombre_serie(bloc, "precipitation_sum", i).unwrap_or(0.0),
            rafales_max: nombre_serie(bloc, "wind_gusts_10m_max", i).unwrap_or(0.0),
        })
    })?;

    let unites = valeur.get("current_units").map(unites_depuis).unwrap_or_default();
    // `air` est rempli par `interroger`, qui fait le SECOND appel — jamais ici :
    // cette fonction ne lit qu'une réponse, et celle-ci n'en contient pas.
    Ok(Bulletin { actuel: conditions, heures, jours, unites, air: None })
}

/// **DURÉE EN CLAIR — « 11 h 31 ».**
///
/// ⚠️ **L'API REND DES SECONDES.** `daylight_duration` vaut `41449.53`, et
/// l'afficher tel quel ne dit rien à personne. Mesuré sur un appel réel.
fn duree_en_clair(secondes: f64) -> String {
    if !secondes.is_finite() || secondes <= 0.0 {
        return String::new();
    }
    // ⚠️ **ARRONDI À LA MINUTE, PAS TRONQUÉ.** `41449.53` vaut 11 h 30 min 49 s :
    // tronquer afficherait « 11 h 30 », qui est faux à une minute près — et c'est
    // le test qui l'a montré, pas une relecture. Le `+ 30` arrondit au plus
    // proche avant de diviser.
    let minutes = (secondes.round() as i64 + 30) / 60;
    format!("{} h {:02}", minutes / 60, minutes % 60)
}

/// **LES HUIT PHASES DE LA LUNE.** `moon_phase` est une FRACTION du cycle :
/// 0 = nouvelle lune, 0.5 = pleine, 0.25 et 0.75 les quartiers.
///
/// On arrondit au huitième le plus proche — c'est la granularité des huit noms
/// ci-dessous, et elle suffit : personne ne lit une phase à 3 % près.
fn libelle_phase_lune(fraction: f64) -> &'static str {
    const PHASES: [&str; 8] = [
        "Nouvelle lune",
        "Premier croissant",
        "Premier quartier",
        "Gibbeuse croissante",
        "Pleine lune",
        "Gibbeuse décroissante",
        "Dernier quartier",
        "Dernier croissant",
    ];
    if !fraction.is_finite() {
        return "";
    }
    PHASES[((fraction.rem_euclid(1.0)) * 8.0).round() as usize % 8]
}

/// « 2026-10-04T07:38 » → « 07:38 », ou **la chaîne VIDE** quand la série n'a pas
/// cette entrée — c'est le cas de `moonrise`, qui vaut `null` certains jours.
fn heure_serie(racine: &Value, cle: &str, i: usize) -> String {
    serie(racine, cle, i).and_then(heure_de).unwrap_or_default()
}

/// Recoud un bloc à tableaux parallèles (`hourly`, `daily`) en une liste.
///
/// Le constructeur reçoit `(l'horodatage, son index, le bloc entier)` : les séries
/// se lisent par `nombre_serie(bloc, "temperature_2m", i)`.
///
/// ⚠️ **LA BORNE EST LE PLUS COURT DES TABLEAUX, ET ELLE EST AUTOMATIQUE.** Si
/// l'API rendait un jour `time` de 7 et `weather_code` de 6, l'index 6 lirait
/// `None` sur `weather_code` ; la boucle s'ARRÊTE là au lieu de paniquer. Mieux
/// vaut six jours justes que sept dont un faux.
fn bloc_parallele<T>(
    valeur: &Value,
    bloc: &str,
    max: usize,
    construire: impl Fn(&Value, usize, &Value) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    let Some(racine) = valeur.get(bloc) else { return Ok(Vec::new()) };
    let temps = racine.get("time").and_then(|v| v.as_array()).ok_or("le bloc n'a pas de `time`")?;
    let mut sortie = Vec::new();
    for (i, horodatage) in temps.iter().take(max).enumerate() {
        let Ok(item) = construire(horodatage, i, racine) else { break };
        sortie.push(item);
    }
    Ok(sortie)
}

/// Un élément d'une série parallèle — `None` au-delà de sa longueur.
fn serie<'a>(racine: &'a Value, cle: &str, i: usize) -> Option<&'a Value> {
    racine.get(cle)?.as_array()?.get(i)
}

fn nombre_serie(racine: &Value, cle: &str, i: usize) -> Result<f64, String> {
    serie(racine, cle, i).and_then(|v| v.as_f64()).ok_or_else(|| format!("`{cle}[{i}]` manque"))
}

fn entier_serie(racine: &Value, cle: &str, i: usize) -> Result<i64, String> {
    serie(racine, cle, i).and_then(|v| v.as_i64()).ok_or_else(|| format!("`{cle}[{i}]` manque"))
}

fn unites_depuis(valeur: &Value) -> Unites {
    let defaut = Unites::default();
    Unites {
        temperature: texte(valeur, "temperature_2m").unwrap_or(defaut.temperature),
        vent: texte(valeur, "wind_speed_10m").unwrap_or(defaut.vent),
        precipitations: texte(valeur, "precipitation").unwrap_or(defaut.precipitations),
    }
}

fn nombre(valeur: &Value, cle: &str) -> Result<f64, String> {
    valeur.get(cle).and_then(|v| v.as_f64()).ok_or_else(|| format!("`{cle}` manque ou n'est pas un nombre"))
}

fn entier(valeur: &Value, cle: &str) -> Result<i64, String> {
    valeur.get(cle).and_then(|v| v.as_i64()).ok_or_else(|| format!("`{cle}` manque ou n'est pas un entier"))
}

fn texte(valeur: &Value, cle: &str) -> Option<String> {
    valeur.get(cle).and_then(|v| v.as_str()).map(str::to_string)
}

fn heure_de(temps: &Value) -> Option<String> {
    // « 2026-10-04T17:00 » → « 17:00 ». On ne lit QUE ce qu'on affiche : pas de
    // fuseau à recalculer, l'API l'a déjà appliqué (`timezone=auto`).
    let texte = temps.as_str()?;
    texte.split_once('T').map(|(_, h)| h.to_string())
}

fn jour_de(temps: &Value) -> Option<String> {
    // « 2026-10-05 » → « lun. 5 ».
    let texte = temps.as_str()?;
    let (date, _) = texte.split_once('T').unwrap_or((texte, ""));
    let mut morceaux = date.split('-');
    let annee: i64 = morceaux.next()?.parse().ok()?;
    let mois: u32 = morceaux.next()?.parse().ok()?;
    let jour: u32 = morceaux.next()?.parse().ok()?;
    Some(format!("{} {}", NOMS_COURTS[jour_semaine(annee, mois, jour)], jour))
}

/// « lun. », « mar. »… dans l'ordre de `Jours::NOMS` (lundi premier).
const NOMS_COURTS: [&str; 7] = ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."];

/// **LES 28 CODES WMO QUE L'API PEUT RENDRE, ET LEUR LIBELLÉ.**
///
/// ⚠️ **CETTE TABLE EST ICI, ET PAS DANS LA PAGE, PARCE QU'ELLE DOIT ÊTRE
/// COMPLÈTE.** L'API ne rend qu'un NOMBRE : un code absent de la table laisserait
/// un libellé vide, et rien ne le signalerait — c'est exactement le genre de trou
/// silencieux que ce dépôt se fait prendre à répétition. Ici, `libelle_wmo` a un
/// test qui passe les 28 codes et refuse le repli.
///
/// L'ICÔNE, elle, reste dans la page : un code inconnu y retombe sur un nuage, ce
/// qui se voit sans être faux.
pub fn libelle_wmo(code: i64) -> &'static str {
    match code {
        0 => "Ciel dégagé",
        1 => "Plutôt dégagé",
        2 => "Partiellement nuageux",
        3 => "Couvert",
        45 => "Brouillard",
        48 => "Brouillard givrant",
        51 => "Bruine légère",
        53 => "Bruine modérée",
        55 => "Bruine dense",
        56 => "Bruine verglaçante légère",
        57 => "Bruine verglaçante dense",
        61 => "Pluie faible",
        63 => "Pluie modérée",
        65 => "Pluie forte",
        66 => "Pluie verglaçante faible",
        67 => "Pluie verglaçante forte",
        71 => "Neige faible",
        73 => "Neige modérée",
        75 => "Neige forte",
        77 => "Grains de neige",
        80 => "Averses faibles",
        81 => "Averses modérées",
        82 => "Averses violentes",
        85 => "Averses de neige faibles",
        86 => "Averses de neige fortes",
        95 => "Orage",
        96 => "Orage avec grêle légère",
        97 => "Orage fort",
        99 => "Orage avec grêle forte",
        _ => "Conditions inconnues",
    }
}

/// Index du jour de la semaine, **lundi = 0**, pour une date grégorienne.
///
/// Algorithme « days from civil » de Howard Hinnant : il ramène la date à un
/// nombre de jours depuis l'époque, dont le reste modulo 7 donne le jour. Le
/// `+ 3` place le lundi à zéro — le 1970-01-01 était un JEUDI, donc à 3.
///
/// ⚠️ **C'est du calcul de date, et ce dépôt s'est déjà fait prendre une fois**
/// (voir `fetes`, dont la table était décalée) : d'où le test qui épingle des
/// dates CONNUES plutôt qu'un aller-retour avec la même formule.
fn jour_semaine(annee: i64, mois: u32, jour: u32) -> usize {
    let annee_ajustee = if mois <= 2 { annee - 1 } else { annee };
    let ere = if annee_ajustee >= 0 { annee_ajustee } else { annee_ajustee - 399 } / 400;
    let annee_ere = annee_ajustee - ere * 400;
    let mois_ajuste = ((mois + 9) % 12) as i64;
    let jour_annee = (153 * mois_ajuste + 2) / 5 + jour as i64 - 1;
    let jour_ere = annee_ere * 365 + annee_ere / 4 - annee_ere / 100 + jour_annee;
    let jours = ere * 146_097 + jour_ere - 719_468;
    ((jours + 3).rem_euclid(7)) as usize
}

// ---------------------------------------------------------------------------
// Le fichier de la ville
// ---------------------------------------------------------------------------

/// Nettoie un nom saisi : espaces resserrés, longueur bornée.
pub(crate) fn nettoyer_ville(nom: &str) -> String {
    let resserre = nom.split_whitespace().collect::<Vec<_>>().join(" ");
    resserre.chars().take(VILLE_MAX).collect()
}

/// Encodage pour la chaîne de requête — seuls les caractères qui la casseraient.
/// Une ville s'écrit rarement avec autre chose que des lettres, mais « Aix-en-
/// Provence » a des tirets, et « Saint-Étienne » des accents.
fn encoder_query(texte: &str) -> String {
    let mut sortie = String::with_capacity(texte.len());
    for octet in texte.bytes() {
        match octet {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => sortie.push(octet as char),
            b' ' => sortie.push('+'),
            autre => sortie.push_str(&format!("%{autre:02X}")),
        }
    }
    sortie
}

/// **LIT LE FICHIER : le lieu ACTIF, puis les favoris.**
///
/// ⚠️ **LE LIEU ACTIF GARDE SES CHAMPS À LA RACINE** (`ville`, `latitude`…) : un
/// `meteo.toml` écrit AVANT les favoris reste donc lisible tel quel, et se relit
/// comme « un lieu actif, zéro favori ». Un favori est un tableau `[[favori]]` en
/// plus — pas une liste dont il faudrait désigner l'élu par un index, qui
/// désignerait un autre lieu dès qu'on en retire un.
fn lieu_depuis_toml(texte: &str) -> (Option<Lieu>, Vec<Lieu>) {
    let Ok(table) = texte.parse::<toml::Table>() else { return (None, Vec::new()) };
    let favoris = table
        .get("favori")
        .and_then(|v| v.as_array())
        .map(|liste| liste.iter().filter_map(|v| v.as_table().and_then(lieu_du_table)).take(FAVORIS_MAX).collect())
        .unwrap_or_default();
    (lieu_du_table(&table), favoris)
}

/// Un lieu, lu dans une table TOML — la racine ou une entrée `[[favori]]`.
fn lieu_du_table(table: &toml::Table) -> Option<Lieu> {
    let ville = table.get("ville")?.as_str()?;
    if ville.is_empty() {
        return None;
    }
    Some(Lieu {
        ville: ville.to_string(),
        libelle: table.get("libelle").and_then(|v| v.as_str()).unwrap_or(ville).to_string(),
        latitude: flottant(table, "latitude")?,
        longitude: flottant(table, "longitude")?,
    })
}

/// Un flottant TOML, **ÉCRIT `47.0` OU `47`.**
///
/// ⛔ **TOML DISTINGUE L'ENTIER DU FLOTTANT, ET `as_float()` NE CONVERTIT PAS.**
/// `latitude = 47` se relit comme un ENTIER, `as_float()` rend `None`, et le lieu
/// est alors **écarté en silence** — un favori, ou le lieu actif, qui disparaît
/// sans un mot. Une coordonnée ronde suffit (l'équateur est à 0).
///
/// D'où les deux à la lecture, et l'écriture TOUJOURS en flottant (`{:?}` garde
/// le « .0 », là où `{}` l'enlève) — voir `vers_toml`.
///
/// ⚠️ **LA DÉFINITION VIT DANS `config::flottant_toml`**, pas ici : le motif
/// existait déjà dans `gfx::degrade_sauvegardes` et MANQUAIT pour la cote d'une
/// zone. Une seule définition, trois appelants.
fn flottant(table: &toml::Table, cle: &str) -> Option<f64> {
    crate::config::flottant_toml(table.get(cle)?)
}

/// **LA SEULE ÉCRITURE DU FICHIER.**
///
/// ⚠️ **ELLE APPARTIENT AU FIL D'INTERFACE, PAS AU FIL D'INTERROGATION.** Le fil
/// de mesure ne connaît que le lieu ACTIF ; lui laisser l'écriture **effacerait
/// les favoris** à chaque rafraîchissement réussi. C'est `enregistrer` qui écrit,
/// appelée par les GESTES — choisir, ajouter, retirer.
fn vers_toml(lieu: &Option<Lieu>, favoris: &[Lieu]) -> String {
    let mut texte = String::new();
    // ⚠️ **`{:?}` POUR LES COORDONNÉES, PAS `{}`.** `{}` écrit `47` pour 47.0, que
    // TOML relit comme un ENTIER — et `as_float()` ne convertit pas, donc le lieu
    // serait écarté en silence au rechargement. `{:?}` garde le « .0 ». La lecture
    // tolère les deux de toute façon (voir `flottant`), pour les fichiers déjà
    // écrits par l'ancienne version.
    if let Some(l) = lieu {
        texte.push_str(&format!(
            "ville = {:?}\nlibelle = {:?}\nlatitude = {:?}\nlongitude = {:?}\n",
            l.ville, l.libelle, l.latitude, l.longitude
        ));
    }
    for f in favoris {
        texte.push_str(&format!(
            "\n[[favori]]\nville = {:?}\nlibelle = {:?}\nlatitude = {:?}\nlongitude = {:?}\n",
            f.ville, f.libelle, f.latitude, f.longitude
        ));
    }
    texte
}

/// Le même endroit ? — **PAR LES COORDONNÉES, JAMAIS PAR LE NOM** : l'API rend
/// trois villes qui s'appellent « Lyon » (France, Mississippi, Missouri), et le
/// nom ne les distingue pas.
fn meme_endroit(a: &Lieu, b: &Lieu) -> bool {
    (a.latitude - b.latitude).abs() < 1e-6 && (a.longitude - b.longitude).abs() < 1e-6
}

/// Écrit le fichier depuis l'état COURANT — appelée par les gestes seulement.
fn enregistrer(m: &Meteo) {
    if !crate::config::write_config_atomique(FICHIER, vers_toml(&m.lieu, &m.favoris)) {
        crate::diag!("[meteo] meteo.toml n'a pas pu être écrit");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une réponse FIGÉE, de la forme exacte rendue par l'API (relevée le
    /// 2026-10-04). Deux heures et deux jours suffisent à éprouver le recousage
    /// des tableaux parallèles.
    const REPONSE: &str = r#"{
        "latitude": 45.75, "longitude": 4.85,
        "current_units": { "temperature_2m": "°C", "wind_speed_10m": "km/h", "precipitation": "mm" },
        "current": { "time": "2026-10-04T17:00", "interval": 900,
                     "temperature_2m": 24.8, "relative_humidity_2m": 47,
                     "apparent_temperature": 24.4, "is_day": 1, "precipitation": 0.0,
                     "weather_code": 0, "wind_speed_10m": 8.4, "wind_gusts_10m": 11.2 },
        "hourly_units": { "temperature_2m": "°C" },
        "hourly": { "time": ["2026-10-04T17:00", "2026-10-04T18:00"],
                    "temperature_2m": [24.8, 24.0], "weather_code": [0, 3],
                    "precipitation_probability": [0, 15], "is_day": [1, 0] },
        "daily_units": { "temperature_2m_max": "°C", "daylight_duration": "s" },
        "daily": { "time": ["2026-10-04", "2026-10-05"], "weather_code": [0, 61],
                   "temperature_2m_max": [25.4, 18.2], "temperature_2m_min": [12.1, 9.8],
                   "sunrise": ["2026-10-04T07:38", "2026-10-05T07:39"],
                   "sunset": ["2026-10-04T19:08", "2026-10-05T19:06"],
                   "daylight_duration": [41449.53, 41245.66],
                   "moonrise": [null, "2026-10-05T01:04"],
                   "moonset": ["2026-10-04T16:28", "2026-10-05T16:58"],
                   "moon_phase": [0.781, 0.817],
                   "uv_index_max": [3.8, 3.85],
                   "precipitation_sum": [4.8, 0.0],
                   "wind_gusts_10m_max": [15.5, 13.0] }
    }"#;

    /// Une réponse FIGÉE de la qualité de l'air — **HORS SAISON, DONC SANS
    /// POLLENS** : c'est le cas réel qui doit être éprouvé, pas le cas garni.
    const REPONSE_AIR: &str = r#"{
        "current_units": { "european_aqi": "EAQI" },
        "current": { "time": "2026-10-04T17:00", "interval": 3600,
                     "european_aqi": 33, "pm10": 9.6, "pm2_5": 6.4 }
    }"#;

    // ---- Les fonctions pures ------------------------------------------------

    /// ⚠️ **LES 28 CODES, ET AUCUN NE DOIT TOMBER DANS LE REPLI.** L'API ne rend
    /// qu'un nombre : un code absent laisserait un libellé vide, sans un mot.
    #[test]
    fn la_table_wmo_couvre_les_vingt_huit_codes() {
        for code in [
            0, 1, 2, 3, 45, 48, 51, 53, 55, 56, 57, 61, 63, 65, 66, 67, 71, 73, 75, 77, 80, 81, 82, 85, 86, 95, 96, 97,
            99,
        ] {
            assert_ne!(libelle_wmo(code), "Conditions inconnues", "le code WMO {code} n'a pas de libellé");
        }
        // Et le repli existe bien pour ce que l'API n'a jamais rendu.
        assert_eq!(libelle_wmo(1234), "Conditions inconnues");
        assert_eq!(libelle_wmo(-1), "Conditions inconnues");
    }

    /// **DES DATES CONNUES, PAS UN ALLER-RETOUR AVEC LA MÊME FORMULE.** Un
    /// aller-retour validerait l'arithmétique contre elle-même ; ce dépôt s'est
    /// déjà fait prendre par une table décalée (voir `fetes`).
    #[test]
    fn le_jour_de_la_semaine_est_juste() {
        // (annee, mois, jour, attendu) — lundi = 0, comme `Jours::NOMS`.
        for (a, m, j, attendu) in [
            (1970, 1, 1, 3),  // un jeudi, l'époque Unix
            (2000, 1, 1, 5),  // un samedi
            (2024, 2, 29, 3), // un jeudi — année bissextile
            (2026, 10, 4, 6), // un dimanche (le jour où ceci est écrit)
            (2026, 10, 5, 0), // le lendemain, un lundi
            (2026, 12, 31, 3), // un jeudi
        ] {
            assert_eq!(jour_semaine(a, m, j), attendu, "{a}-{m:02}-{j:02} mal placé");
        }
    }

    /// ⚠️ **L'API REND DES SECONDES.** `daylight_duration` vaut `41449.53` — soit
    /// 11 h 31. L'afficher tel quel aurait donné un nombre à sept chiffres.
    #[test]
    fn la_duree_du_jour_est_dite_en_heures_pas_en_secondes() {
        assert_eq!(duree_en_clair(41449.53), "11 h 31");
        assert_eq!(duree_en_clair(3600.0), "1 h 00");
        assert_eq!(duree_en_clair(0.0), "", "une absence ne s'écrit pas « 0 h 00 »");
        assert_eq!(duree_en_clair(f64::NAN), "");
        assert_eq!(duree_en_clair(-5.0), "");
    }

    /// **LES HUIT PHASES, ET LEURS BORDS.** `moon_phase` est une fraction du
    /// cycle : on arrondit au huitième le plus proche.
    #[test]
    fn les_huit_phases_de_la_lune() {
        assert_eq!(libelle_phase_lune(0.0), "Nouvelle lune");
        assert_eq!(libelle_phase_lune(0.25), "Premier quartier");
        assert_eq!(libelle_phase_lune(0.5), "Pleine lune");
        assert_eq!(libelle_phase_lune(0.75), "Dernier quartier");
        // La valeur RÉELLE relevée sur l'API le 2026-10-04 : 0.781 est plus près
        // du dernier quartier (0.75) que du dernier croissant (0.875).
        assert_eq!(libelle_phase_lune(0.781), "Dernier quartier");
        // La fraction peut valoir 1.0 (cycle bouclé) — elle doit retomber sur 0.
        assert_eq!(libelle_phase_lune(1.0), "Nouvelle lune");
        assert_eq!(libelle_phase_lune(0.999), "Nouvelle lune");
        assert_eq!(libelle_phase_lune(f64::NAN), "");
    }

    #[test]
    fn les_horodatages_sont_mis_en_forme_pour_l_affichage() {
        assert_eq!(heure_de(&json!("2026-10-04T17:00")), Some("17:00".to_string()));
        assert_eq!(jour_de(&json!("2026-10-05")), Some("lun. 5".to_string()));
        assert_eq!(jour_de(&json!("2026-10-04")), Some("dim. 4".to_string()));
        // Une écriture inattendue ne doit pas jeter le bulletin, seulement cet
        // élément — d'où `Option` et non une panique.
        assert_eq!(heure_de(&json!("n'importe quoi")), None);
        assert_eq!(jour_de(&json!("2026-10")), None);
    }

    // ---- La lecture des réponses -------------------------------------------

    #[test]
    fn une_reponse_complete_est_demelee() {
        let valeur: Value = serde_json::from_str(REPONSE).expect("JSON de test");
        let b = bulletin_depuis(&valeur).expect("bulletin");
        assert_eq!(b.actuel.temperature, 24.8);
        assert_eq!(b.actuel.code, 0);
        assert!(b.actuel.jour);
        assert_eq!(b.unites.temperature, "°C");
        assert_eq!(b.unites.vent, "km/h");
        assert_eq!(b.heures.len(), 2);
        assert_eq!(b.heures[0].heure, "17:00");
        assert_eq!(b.heures[1].probabilite, 15.0);
        // ⚠️ **LE JOUR ET LA NUIT VIENNENT DE L'API, PAS DE L'HEURE.** Une version
        // antérieure les supposait toujours « jour », et la bande des 24 h montrait
        // donc un SOLEIL à 02:00 — relevé sur une capture, pas déduit.
        assert!(b.heures[0].jour, "17:00 doit être le jour");
        assert!(!b.heures[1].jour, "18:00 est donné de nuit par la réponse figée");
        assert_eq!(b.actuel.rafales, 11.2);
        // Le soleil et la lune du premier jour.
        assert_eq!(b.jours[0].lever, "07:38");
        assert_eq!(b.jours[0].coucher, "19:08");
        assert_eq!(b.jours[0].jour_duree, "11 h 31");
        assert_eq!(b.jours[0].phase_lune, "Dernier quartier");
        assert_eq!(b.jours[0].uv_max, 3.8);
        assert_eq!(b.jours[0].pluie, 4.8);
        assert_eq!(b.jours[0].rafales_max, 15.5);
        // ⚠️ **LE PREMIER JOUR N'A PAS DE LEVÉE DE LUNE** — la réponse dit `null`,
        // et c'est une journée ordinaire, pas une panne.
        assert_eq!(b.jours[0].lever_lune, "", "moonrise null doit donner une chaîne VIDE");
        assert_eq!(b.jours[0].coucher_lune, "16:28");
        assert_eq!(b.jours[1].lever_lune, "01:04", "le lendemain, la lune se lève");
        assert_eq!(b.jours.len(), 2);
        assert_eq!(b.jours[1].jour, "lun. 5");
        assert_eq!(b.jours[1].min, 9.8);
    }

    /// ⚠️ **UNE PROBABILITÉ ABSENTE N'EST PAS UNE PANNE.** L'API l'omet sur
    /// certaines périodes ; exiger sa présence jetterait tout le bulletin.
    #[test]
    fn une_serie_facultative_absente_ne_jette_pas_le_bulletin() {
        let sans = REPONSE.replace(r#""precipitation_probability": [0, 15]"#, r#""temperature_2m_extra": [1, 2]"#);
        let valeur: Value = serde_json::from_str(&sans).expect("JSON de test");
        let b = bulletin_depuis(&valeur).expect("le bulletin doit survivre");
        assert_eq!(b.heures.len(), 2);
        assert_eq!(b.heures[0].probabilite, 0.0);
    }

    /// **LA BORNE EST LE PLUS COURT DES TABLEAUX.** Trois horodatages, deux
    /// températures : on rend DEUX heures justes, jamais trois dont une inventée.
    #[test]
    fn un_tableau_plus_court_arrete_la_liste_au_lieu_de_paniquer() {
        let tronque = REPONSE.replace(
            r#""time": ["2026-10-04T17:00", "2026-10-04T18:00"]"#,
            r#""time": ["2026-10-04T17:00", "2026-10-04T18:00", "2026-10-04T19:00"]"#,
        );
        let valeur: Value = serde_json::from_str(&tronque).expect("JSON de test");
        let b = bulletin_depuis(&valeur).expect("bulletin");
        assert_eq!(b.heures.len(), 2, "le troisième horodatage n'a pas de température");
    }

    /// ⚠️ **UNE VILLE INCONNUE REND 200 SANS LA CLÉ `results`** — vérifié sur
    /// l'API. Ce n'est PAS une panne, et le confondre ferait dire « l'API a
    /// échoué » là où il faut dire « aucune ville ne s'appelle ainsi ».
    #[test]
    fn une_recherche_sans_resultat_n_est_pas_une_erreur() {
        let vide: Value = serde_json::from_str(r#"{"generationtime_ms": 0.5}"#).expect("JSON");
        assert_eq!(candidats_depuis(&vide).expect("pas une erreur"), Vec::new());
    }

    #[test]
    fn les_villes_proposees_portent_de_quoi_les_distinguer() {
        // L'API rend TROIS « Lyon » : France, Mississippi, Missouri. Sans le pays
        // et la région, la liste serait trois fois le même mot.
        let valeur: Value = serde_json::from_str(
            r#"{"results": [
                {"name": "Lyon", "latitude": 45.74, "longitude": 4.84, "country": "France", "admin1": "Rhône-Alpes"},
                {"name": "Lyon", "latitude": 34.21, "longitude": -90.54, "country": "États-Unis", "admin1": "Mississippi"}
            ]}"#,
        )
        .expect("JSON");
        let villes = candidats_depuis(&valeur).expect("candidats");
        assert_eq!(villes.len(), 2);
        assert_eq!(villes[0].libelle, "Lyon, Rhône-Alpes, France");
        assert_eq!(villes[1].libelle, "Lyon, Mississippi, États-Unis");
        assert_eq!(villes[0].nom, "Lyon");
    }

    /// **LA QUALITÉ DE L'AIR SE LIT MÊME QUAND LES POLLENS MANQUENT** — hors
    /// saison, l'API les omet. Ce n'est pas une panne : l'indice et les particules
    /// restent, les pollens tombent à zéro.
    #[test]
    fn la_qualite_de_l_air_se_lit_meme_sans_pollens() {
        let valeur: Value = serde_json::from_str(REPONSE_AIR).expect("JSON de test");
        let air = air_depuis(&valeur).expect("l'air doit se lire");
        assert_eq!(air.aqi, 33);
        assert_eq!(air.pm2_5, 6.4);
        assert_eq!(air.pm10, 9.6);
        assert_eq!(air.pollens.len(), 6, "les six pollens sont TOUJOURS là, à zéro s'il le faut");
        assert!(air.pollens.iter().all(|p| p.grains == 0.0));
        assert_eq!(air.pollens[0].nom, "Aulne");
    }

    /// Un indice ABSENT ne devient pas 0 : 0 est une bonne qualité d'air, et
    /// l'inventer serait un mensonge. -1 dit « l'API n'en a pas parlé ».
    #[test]
    fn un_indice_absent_ne_devient_pas_une_bonne_qualite_d_air() {
        let sans: Value = serde_json::from_str(r#"{"current": {"pm2_5": 6.4}}"#).expect("JSON");
        let air = air_depuis(&sans).expect("l'air doit se lire");
        assert_eq!(air.aqi, -1, "l'absence ne doit pas se lire comme « 0, excellent »");
    }

    #[test]
    fn le_libelle_retombe_sur_le_nom_quand_l_api_ne_dit_rien() {
        let sans_pays: Value = serde_json::from_str(r#"{"name": "Ailleurs"}"#).expect("JSON");
        assert_eq!(libelle_du_lieu(&sans_pays, "Ailleurs"), "Ailleurs");
    }

    // ---- La saisie et le fichier -------------------------------------------

    #[test]
    fn un_nom_de_ville_est_resserre_et_borne() {
        assert_eq!(nettoyer_ville("  Aix   en  Provence "), "Aix en Provence");
        assert_eq!(nettoyer_ville("   "), "");
        assert_eq!(nettoyer_ville(&"x".repeat(200)).chars().count(), VILLE_MAX);
    }

    #[test]
    fn l_encodage_de_requete_laisse_passer_les_accents_et_les_tirets() {
        assert_eq!(encoder_query("Saint-Étienne"), "Saint-%C3%89tienne");
        assert_eq!(encoder_query("Aix en Provence"), "Aix+en+Provence");
        assert_eq!(encoder_query("New York"), "New+York");
    }

    /// Un lieu de test — le libellé n'a pas d'importance ici, les coordonnées si.
    fn lieu(nom: &str, latitude: f64, longitude: f64) -> Lieu {
        Lieu { ville: nom.into(), libelle: format!("{nom}, quelque part"), latitude, longitude }
    }

    #[test]
    fn le_fichier_de_la_ville_fait_l_aller_retour() {
        let actif = lieu("Lyon", 45.74906, 4.84789);
        let texte = vers_toml(&Some(actif.clone()), &[]);
        assert_eq!(lieu_depuis_toml(&texte), (Some(actif), Vec::new()));
        // Un fichier vide ou abîmé ne doit pas fabriquer un lieu à zéro, zéro.
        assert_eq!(lieu_depuis_toml(""), (None, Vec::new()));
        assert_eq!(lieu_depuis_toml("latitude = 1.0\nlongitude = 2.0\n"), (None, Vec::new()), "sans ville");
    }

    /// **LE FICHIER D'AVANT LES FAVORIS RESTE LISIBLE.** C'est ce qui évite une
    /// migration — et un `meteo.toml` existant ne se perd pas à la mise à jour.
    #[test]
    fn un_fichier_sans_favori_se_lit_comme_avant() {
        let ancien = "ville = \"Vesoul\"\nlibelle = \"Vesoul, Bourgogne, France\"\nlatitude = 47.62604\nlongitude = 6.14251\n";
        let (actif, favoris) = lieu_depuis_toml(ancien);
        assert_eq!(actif.expect("le lieu actif").ville, "Vesoul");
        assert!(favoris.is_empty(), "zéro favori, et ce n'est pas une erreur");
    }

    /// ⭐ **CINQUANTE LIEUX** (demande du 2026-10-19) : les cinquante se relisent,
    /// le cinquante et unième non.
    #[test]
    fn cinquante_favoris_se_relisent() {
        let favoris: Vec<Lieu> = (0..51).map(|i| lieu(&format!("Ville {i}"), 40.0 + i as f64 / 10.0, 2.0)).collect();
        let (_, relus) = lieu_depuis_toml(&vers_toml(&None, &favoris));
        assert_eq!(relus.len(), 50);
        assert_eq!(relus[..], favoris[..50]);
    }

    /// Les favoris font l'aller-retour **EN PLUS** du lieu actif, et leur ORDRE
    /// est conservé — c'est lui qui donne le rang des boutons.
    #[test]
    fn les_favoris_font_l_aller_retour_dans_l_ordre() {
        let actif = lieu("Vesoul", 47.62604, 6.14251);
        let favoris = vec![lieu("Lyon", 45.74906, 4.84789), lieu("Paris", 48.8566, 2.3522)];
        let texte = vers_toml(&Some(actif.clone()), &favoris);
        let (relu, relus) = lieu_depuis_toml(&texte);
        assert_eq!(relu, Some(actif));
        assert_eq!(relus, favoris, "l'ordre des favoris est celui des boutons");
    }

    /// ⛔ **LA RÉGRESSION QUI COMPTAIT.** Le fil de mesure écrivait le SEUL lieu
    /// actif : lui laisser l'écriture aurait **effacé les favoris** à chaque
    /// changement de ville. C'est pourquoi `vers_toml` prend les deux.
    #[test]
    fn ecrire_ne_perd_pas_les_favoris() {
        let favoris = vec![lieu("Lyon", 45.75, 4.85)];
        let texte = vers_toml(&Some(lieu("Vesoul", 47.63, 6.14)), &favoris);
        assert!(texte.contains("[[favori]]"), "les favoris doivent être écrits :\n{texte}");
        assert_eq!(lieu_depuis_toml(&texte).1, favoris);
    }

    /// ⛔ **UNE COORDONNÉE RONDE NE DOIT PAS FAIRE DISPARAÎTRE LE LIEU.**
    ///
    /// TOML distingue l'ENTIER du FLOTTANT : `latitude = 47` se relit comme un
    /// entier, et `as_float()` **ne convertit pas** — il rend `None`. Sans
    /// `flottant`, le lieu était donc **écarté en silence**. L'équateur est à 0,
    /// et bien des villes sont à des degrés ronds.
    ///
    /// **Trouvé par un test qui utilisait 1.0, 2.0 et 3.0 comme coordonnées** — un
    /// jeu « trop propre » pour être réaliste, et qui a justement révélé le cas.
    #[test]
    fn une_coordonnee_ronde_ne_perd_pas_le_lieu() {
        // Tel que l'écrivait la version d'avant, qui utilisait `{}`.
        let ancien = "ville = \"Quito\"\nlibelle = \"Quito\"\nlatitude = 0\nlongitude = -78\n";
        let (lu, _) = lieu_depuis_toml(ancien);
        let lu = lu.expect("un entier doit se lire comme un flottant");
        assert_eq!(lu.latitude, 0.0);
        assert_eq!(lu.longitude, -78.0);

        // Et ce qu'on écrit AUJOURD'HUI reste un flottant, donc relisible.
        let texte = vers_toml(&Some(lieu("Quito", 0.0, -78.0)), &[lieu("Libreville", 0.0, 9.0)]);
        assert!(texte.contains("latitude = 0.0"), "un flottant doit rester un flottant :\n{texte}");
        let (actif, favoris) = lieu_depuis_toml(&texte);
        assert_eq!(actif.expect("actif").ville, "Quito");
        assert_eq!(favoris.len(), 1, "le favori à longitude ronde doit survivre");
    }

    /// ⭐ **LE MÊME ENDROIT SE RECONNAÎT AUX COORDONNÉES, PAS AU NOM.** L'API rend
    /// trois villes qui s'appellent « Lyon », dont deux aux États-Unis : comparer
    /// les noms en ferait un seul favori, et « ajouter » refuserait la seconde.
    #[test]
    fn deux_villes_homonymes_ne_sont_pas_le_meme_endroit() {
        let lyon_fr = lieu("Lyon", 45.74906, 4.84789);
        let lyon_us = lieu("Lyon", 34.21789, -90.54204);
        assert!(meme_endroit(&lyon_fr, &lyon_fr.clone()), "le même lieu est le même");
        assert!(!meme_endroit(&lyon_fr, &lyon_us), "deux Lyon différents ne se confondent pas");
    }

    /// Un favori se désigne par son RANG — celui que la page renvoie. Le fichier
    /// doit donc rendre les favoris dans l'ordre où les boutons les montrent.
    #[test]
    fn le_rang_d_un_favori_est_celui_du_fichier() {
        let favoris = vec![lieu("A", 1.0, 1.0), lieu("B", 2.0, 2.0), lieu("C", 3.0, 3.0)];
        let texte = vers_toml(&None, &favoris);
        let (_, relus) = lieu_depuis_toml(&texte);
        assert_eq!(relus[0].ville, "A");
        assert_eq!(relus[2].ville, "C");
    }

    /// L'URL demandée est CELLE QUI A ÉTÉ VÉRIFIÉE À LA MAIN — variables,
    /// `timezone=auto`, et les deux bornes ensemble (elles se combinent : éprouvé).
    #[test]
    fn l_url_des_previsions_demande_tout_ce_que_la_page_affiche() {
        let lieu = Lieu { ville: "Lyon".into(), libelle: "Lyon".into(), latitude: 45.75, longitude: 4.85 };
        let url = url_previsions(&lieu);
        for morceau in [
            "https://api.open-meteo.com/v1/forecast?",
            "latitude=45.75",
            "longitude=4.85",
            "current=temperature_2m",
            "relative_humidity_2m",
            "apparent_temperature",
            "is_day",
            "weather_code",
            "wind_speed_10m",
            "hourly=temperature_2m",
            "precipitation_probability",
            "is_day",
            "daily=weather_code",
            "temperature_2m_max",
            "temperature_2m_min",
            "sunrise",
            "sunset",
            "daylight_duration",
            "moonrise",
            "moonset",
            "moon_phase",
            "uv_index_max",
            "precipitation_sum",
            "wind_gusts_10m_max",
            "wind_gusts_10m",
            "timezone=auto",
            "forecast_days=7",
            "forecast_hours=24",
        ] {
            assert!(url.contains(morceau), "l'URL ne demande pas {morceau}");
        }
    }

    /// L'air est un AUTRE HÔTE — c'est ce qui justifie un second appel, et ce qui
    /// oblige à le rendre facultatif.
    #[test]
    fn l_url_de_l_air_est_un_autre_hote() {
        let lieu = Lieu { ville: "Lyon".into(), libelle: "Lyon".into(), latitude: 45.75, longitude: 4.85 };
        let url = url_air(&lieu);
        assert!(url.starts_with("https://air-quality-api.open-meteo.com/v1/air-quality?"), "{url}");
        for morceau in ["latitude=45.75", "longitude=4.85", "european_aqi", "pm2_5", "pm10"] {
            assert!(url.contains(morceau), "l'URL d'air ne demande pas {morceau}");
        }
        // Les six pollens doivent tous être demandés.
        for (cle, _) in POLLENS {
            assert!(url.contains(cle), "l'URL d'air ne demande pas {cle}");
        }
    }

    /// Les libellés de pollens sont FRANÇAIS et dans l'ordre d'affichage — comme
    /// les codes WMO, ils vivent ici et pas dans la page.
    #[test]
    fn les_pollens_ont_leur_libelle_francais() {
        assert_eq!(POLLENS.len(), 6);
        assert_eq!(POLLENS[0], ("alder_pollen", "Aulne"));
        assert_eq!(POLLENS[2], ("grass_pollen", "Graminées"));
        assert_eq!(POLLENS[5], ("ragweed_pollen", "Ambroisie"));
    }

    // ---- La sonde réseau ----------------------------------------------------

    /// **SONDE RÉSEAU — `#[ignore]`, JAMAIS DANS LA SUITE.**
    ///
    /// `cargo test --release --bins -- --ignored --nocapture la_sonde_reseau`
    ///
    /// Elle existe pour séparer deux pannes qui se ressemblent : « l'API a changé »
    /// et « la page ne dessine pas ». Sans elle, un panneau vide ferait hésiter.
    #[test]
    #[ignore]
    fn la_sonde_reseau_interroge_vraiment_open_meteo() {
        // Lyon.
        let lieu = Lieu { ville: "Lyon".into(), libelle: "Lyon, France".into(), latitude: 45.74906, longitude: 4.84789 };

        let villes = get_json("https://geocoding-api.open-meteo.com/v1/search?name=Lyon&count=3&language=fr&format=json")
            .and_then(|v| candidats_depuis(&v))
            .expect("géocodage");
        println!("[sonde] {} ville(s) pour « Lyon » : {:?}", villes.len(), villes.first().map(|c| &c.libelle));
        assert!(!villes.is_empty());

        let brut = get_json(&url_previsions(&lieu)).expect("prévisions");
        let b = bulletin_depuis(&brut).expect("bulletin");
        println!(
            "[sonde] actuel : {:.1}{} — {} (code {}) ; {} heure(s) ; {} jour(s)",
            b.actuel.temperature,
            b.unites.temperature,
            libelle_wmo(b.actuel.code),
            b.actuel.code,
            b.heures.len(),
            b.jours.len()
        );
        let j = b.jours.first().expect("au moins un jour");
        println!(
            "[sonde] soleil : {} → {} ({}) ; lune : {} → {} ({}) ; UV max {} ; pluie {} mm ; rafales max {}",
            j.lever, j.coucher, j.jour_duree, j.lever_lune, j.coucher_lune, j.phase_lune, j.uv_max, j.pluie, j.rafales_max
        );
        assert_eq!(b.heures.len(), HEURES, "l'API ne rend plus 24 heures");
        assert_eq!(b.jours.len(), JOURS, "l'API ne rend plus 7 jours");
        assert_ne!(libelle_wmo(b.actuel.code), "Conditions inconnues");
        // ⚠️ **LA SONDE DOIT VÉRIFIER LE FORMAT, PAS SEULEMENT LA PRÉSENCE.**
        // `daylight_duration` est en SECONDES : si l'API changeait d'unité, ce
        // serait ici qu'on le verrait, et pas dans six mois.
        assert!(j.jour_duree.contains(" h "), "durée du jour illisible : {:?}", j.jour_duree);

        let air = get_json(&url_air(&lieu)).and_then(|v| air_depuis(&v)).expect("l'air");
        println!(
            "[sonde] air : indice {} ; PM2,5 {} ; PM10 {} ; pollens non nuls : {:?}",
            air.aqi,
            air.pm2_5,
            air.pm10,
            air.pollens.iter().filter(|p| p.grains > 0.0).map(|p| (p.nom, p.grains)).collect::<Vec<_>>()
        );
    }
}
