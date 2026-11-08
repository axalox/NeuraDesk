//! **LA SURVEILLANCE DE SERVEURS — ping et HTTP, avec un an d'historique.**
//! DEMANDE EXPLICITE (2026-09-26) : « une applet pour un dash de surveillance de
//! serveurs par ping, permettant de suivre l'évolution d'adresses ip ou
//! d'adresses web avec un historique » — précisée ainsi : une vérification HTTP
//! en plus du ping, une mesure par MINUTE même panneau fermé, UN AN d'historique
//! qui survit au redémarrage, et l'icône de l'applet qui clignote en rouge tant
//! qu'une cible est injoignable (sans son, sans ouvrir le panneau).
//!
//! Ce module porte le MODÈLE, sa persistance et le FIL DE MESURE ; le panneau
//! est `win32::cef_surveillance`.
//!
//! - Les cibles : `%APPDATA%\NeuraDesk\surveillance.toml`.
//! - L'historique : UN FICHIER BINAIRE PAR CIBLE,
//!   `%APPDATA%\NeuraDesk\surveillance\<id>.bin`, en AJOUT SEUL — 12 octets par
//!   mesure (voir `Mesure::encoder`), soit ~6 Mo par cible et par an. Du TOML
//!   pèserait dix fois plus et devrait être réécrit en entier à chaque minute.
//!
//! ⚠️ **LE RÉSEAU NE TOURNE QUE SUR LE FIL DE MESURE** (voir `fil_de_mesure`),
//! lancé par `charger` — jamais depuis le fil de l'interface (un hôte muet
//! bloquerait l'application des secondes entières), et **JAMAIS DANS UN TEST** :
//! tout ce qui décide (adresses, encodage, élagage, agrégation) est PUR.
//!
//! ⚠️ **LE FIL POSSÈDE LES FICHIERS D'HISTORIQUE** : il les relit au démarrage (6 Mo
//! par cible ne se lisent pas sur le fil de l'interface), les élague, y ajoute
//! chaque mesure, et envoie le tout au fil de l'interface par canal — qui n'en
//! garde qu'une COPIE en mémoire pour les graphiques.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV6, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::NetworkManagement::IpHelper::{
    Icmp6CreateFile, Icmp6SendEcho2, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMPV6_ECHO_REPLY_LH, ICMP_ECHO_REPLY,
};
use windows::Win32::Networking::WinSock::{AF_INET6, IN6_ADDR, IN6_ADDR_0, SOCKADDR_IN6, SOCKADDR_IN6_0};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_OPTION_IPV6_FAST_FALLBACK, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

use crate::config;

const FICHIER: &str = "surveillance.toml";
/// Le sous-dossier des historiques, dans `%APPDATA%\NeuraDesk\`.
const DOSSIER_HISTORIQUE: &str = "surveillance";
/// Une mesure par minute (DEMANDE EXPLICITE).
const INTERVALLE: Duration = Duration::from_secs(60);
/// Un an d'historique (DEMANDE EXPLICITE).
pub const CONSERVATION_S: u32 = 365 * 24 * 3600;
const DELAI_PING_MS: u32 = 2_000;
/// Résolution, connexion, envoi, réception : un serveur web muet ne doit pas
/// retenir la mesure au-delà de la minute suivante.
const DELAI_HTTP_MS: i32 = 10_000;
/// Au-delà, une cible de plus ne se surveille plus : une par fil à chaque minute.
pub const CIBLES_MAX: usize = 32;
const LONGUEUR_NOM_MAX: usize = 60;
const LONGUEUR_ADRESSE_MAX: usize = 300;

/// Une latence qui a ÉCHOUÉ (hôte injoignable, délai dépassé, statut d'erreur…).
const ECHEC: u16 = u16::MAX;
/// Une latence qui n'a pas été MESURÉE (HTTP désactivé pour cette cible).
const NON_MESURE: u16 = u16::MAX - 1;
/// La plus grande latence représentable — au-delà, elle est plafonnée.
const LATENCE_MAX: u16 = u16::MAX - 2;

// ---------------------------------------------------------------------------
// LES CIBLES ET LEURS ADRESSES
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cible {
    pub id: u64,
    pub nom: String,
    /// Telle que saisie : une IP, un nom d'hôte, ou une URL `http(s)://`.
    pub adresse: String,
    /// Vérifier aussi en HTTP : la cible n'est EN LIGNE que si le serveur web
    /// répond (statut 2xx ou 3xx), quoi que dise le ping.
    pub http: bool,
    /// L'ALERTE de cette cible — DEMANDE EXPLICITE (2026-10-03) : « couper les
    /// alertes tout en continuant de surveiller ». Faux : la cible est mesurée,
    /// tracée et montrée en panne dans le panneau, mais ne fait plus clignoter
    /// l'icône. Vrai par défaut, et pour un fichier écrit avant ce réglage.
    pub alerte: bool,
}

/// Une adresse saisie, analysée : l'hôte à pinguer, et l'URL de la vérification
/// HTTP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adresse {
    pub hote: String,
    /// L'URL saisie — ou, pour une IP ou un nom d'hôte, `http://<hôte>/` (WinHTTP
    /// suit la redirection vers HTTPS si le serveur la demande).
    pub url: String,
}

fn hote_valide(hote: &str) -> bool {
    !hote.is_empty()
        && hote.len() <= 253
        && hote.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        && !hote.starts_with(['.', '-'])
}

/// **LA FRONTIÈRE** : tout ce qui vient de la page passe ici avant d'être gardé.
/// Accepte une IPv4, un nom d'hôte, ou une URL `http://` / `https://` (avec
/// port et chemin). Refuse le reste — un espace, un schéma inconnu, une IPv6
/// (le ping ne passe que par `IcmpSendEcho`, IPv4 seulement).
pub fn analyser_adresse(texte: &str) -> Result<Adresse, String> {
    let texte = texte.trim();
    if texte.is_empty() {
        return Err("l'adresse est vide".into());
    }
    if texte.len() > LONGUEUR_ADRESSE_MAX || texte.contains(char::is_whitespace) || texte.contains(char::is_control) {
        return Err("l'adresse contient un espace ou est trop longue".into());
    }
    let schema = ["https://", "http://"].into_iter().find(|s| texte.len() >= s.len() && texte[..s.len()].eq_ignore_ascii_case(s));
    let Some(schema) = schema else {
        if texte.contains(['/', ':', '[']) {
            return Err("une adresse sans http:// ou https:// doit être une IP ou un nom d'hôte, sans port ni chemin".into());
        }
        if !hote_valide(texte) {
            return Err(format!("hôte « {texte} » invalide"));
        }
        let hote = texte.to_ascii_lowercase();
        return Ok(Adresse { url: format!("http://{hote}/"), hote });
    };
    let reste = &texte[schema.len()..];
    let fin_hote = reste.find(['/', '?', '#']).unwrap_or(reste.len());
    let hote_port = &reste[..fin_hote];
    if hote_port.starts_with('[') {
        return Err("les adresses IPv6 ne sont pas gérées".into());
    }
    let hote = match hote_port.rsplit_once(':') {
        Some((h, p)) => {
            p.parse::<u16>().map_err(|_| format!("port « {p} » invalide"))?;
            h
        }
        None => hote_port,
    };
    if !hote_valide(hote) {
        return Err(format!("hôte « {hote} » invalide"));
    }
    Ok(Adresse { hote: hote.to_ascii_lowercase(), url: texte.to_string() })
}

/// Une URL découpée pour WinHTTP, qui veut l'hôte, le port et le chemin À PART.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct UrlDecoupee {
    pub securisee: bool,
    pub hote: String,
    pub port: u16,
    /// Chemin ET requête, tels quels.
    pub chemin: String,
}

/// Ne s'applique qu'à une URL déjà passée par `analyser_adresse`.
pub(crate) fn decouper_url(url: &str) -> Option<UrlDecoupee> {
    let securisee = url.get(..8).is_some_and(|s| s.eq_ignore_ascii_case("https://"));
    let reste = url.get(if securisee { 8 } else { 7 }..)?;
    let reste = reste.split('#').next().unwrap_or("");
    let fin_hote = reste.find(['/', '?']).unwrap_or(reste.len());
    let (hote_port, chemin) = reste.split_at(fin_hote);
    let (hote, port) = match hote_port.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().ok()?),
        None => (hote_port, if securisee { 443 } else { 80 }),
    };
    let chemin = if chemin.is_empty() {
        "/".to_string()
    } else if chemin.starts_with('?') {
        format!("/{chemin}")
    } else {
        chemin.to_string()
    };
    Some(UrlDecoupee { securisee, hote: hote.to_string(), port, chemin })
}

// ---------------------------------------------------------------------------
// LES MESURES ET LEUR FICHIER
// ---------------------------------------------------------------------------

/// Une vérification d'une cible, à un instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mesure {
    /// L'instant Unix, en secondes (tient jusqu'en 2106).
    pub instant_s: u32,
    /// Latence du ping en ms, ou `ECHEC`.
    pub ping_ms: u16,
    /// Durée de la requête HTTP en ms, `ECHEC`, ou `NON_MESURE`.
    pub http_ms: u16,
    /// Le statut HTTP final (après redirections), `0` sans réponse.
    pub statut: u16,
}

/// 4 + 2 + 2 + 2, plus 2 octets réservés : une taille qui reste alignée.
const TAILLE_MESURE: usize = 12;

fn plafonner(ms: u128) -> u16 {
    ms.min(LATENCE_MAX as u128) as u16
}

impl Mesure {
    pub fn ping_ok(&self) -> bool {
        self.ping_ms <= LATENCE_MAX
    }

    pub fn http_mesure(&self) -> bool {
        self.http_ms != NON_MESURE
    }

    pub fn http_ok(&self) -> bool {
        self.http_ms <= LATENCE_MAX && (200..400).contains(&self.statut)
    }

    /// **EN LIGNE** : le serveur web répond quand HTTP est vérifié (un serveur
    /// qui pingue mais dont le site est tombé est EN PANNE), sinon le ping.
    pub fn en_ligne(&self) -> bool {
        if self.http_mesure() {
            self.http_ok()
        } else {
            self.ping_ok()
        }
    }

    /// La latence que trace le graphique : celle du ping, ou à défaut (un
    /// serveur web qui bloque l'ICMP) celle de la requête HTTP.
    pub fn latence(&self) -> Option<u16> {
        if self.ping_ok() {
            Some(self.ping_ms)
        } else if self.http_ok() {
            Some(self.http_ms)
        } else {
            None
        }
    }

    fn encoder(&self) -> [u8; TAILLE_MESURE] {
        let mut o = [0u8; TAILLE_MESURE];
        o[0..4].copy_from_slice(&self.instant_s.to_le_bytes());
        o[4..6].copy_from_slice(&self.ping_ms.to_le_bytes());
        o[6..8].copy_from_slice(&self.http_ms.to_le_bytes());
        o[8..10].copy_from_slice(&self.statut.to_le_bytes());
        o
    }

    fn decoder(o: &[u8]) -> Mesure {
        let u16_a = |i: usize| u16::from_le_bytes([o[i], o[i + 1]]);
        Mesure { instant_s: u32::from_le_bytes([o[0], o[1], o[2], o[3]]), ping_ms: u16_a(4), http_ms: u16_a(6), statut: u16_a(8) }
    }
}

/// Relit un fichier d'historique. Une fin tronquée (application coupée en
/// pleine écriture) est ignorée, et une mesure qui remonterait le temps (horloge
/// système reculée) aussi : la liste reste TRIÉE, ce que `agreger` suppose.
fn decoder_historique(octets: &[u8]) -> Vec<Mesure> {
    let mut mesures: Vec<Mesure> = Vec::with_capacity(octets.len() / TAILLE_MESURE);
    for bloc in octets.chunks_exact(TAILLE_MESURE) {
        let m = Mesure::decoder(bloc);
        if mesures.last().is_none_or(|d| d.instant_s <= m.instant_s) {
            mesures.push(m);
        }
    }
    mesures
}

/// L'index de la première mesure à garder : tout ce qui précède a plus d'un an.
fn premier_a_garder(mesures: &[Mesure], maintenant_s: u32) -> usize {
    let limite = maintenant_s.saturating_sub(CONSERVATION_S);
    mesures.partition_point(|m| m.instant_s < limite)
}

// ---------------------------------------------------------------------------
// L'AGRÉGATION POUR LE GRAPHIQUE
// ---------------------------------------------------------------------------

/// La période que montre le graphique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plage {
    Heure,
    Jour,
    Semaine,
    Mois,
    An,
}

impl Plage {
    pub const TOUTES: [Plage; 5] = [Plage::Heure, Plage::Jour, Plage::Semaine, Plage::Mois, Plage::An];

    pub fn nom(self) -> &'static str {
        match self {
            Plage::Heure => "1h",
            Plage::Jour => "24h",
            Plage::Semaine => "7j",
            Plage::Mois => "30j",
            Plage::An => "1an",
        }
    }

    pub fn depuis_nom(nom: &str) -> Option<Plage> {
        Plage::TOUTES.into_iter().find(|p| p.nom() == nom)
    }

    /// `(durée totale en s, nombre de barres)` — une barre par minute sur une
    /// heure, par quart d'heure sur un jour, par heure sur une semaine, par
    /// 6 h sur un mois, par jour sur un an.
    pub fn decoupage(self) -> (u32, usize) {
        match self {
            Plage::Heure => (3600, 60),
            Plage::Jour => (24 * 3600, 96),
            Plage::Semaine => (7 * 24 * 3600, 168),
            Plage::Mois => (30 * 24 * 3600, 120),
            Plage::An => (CONSERVATION_S, 365),
        }
    }
}

/// Une barre du graphique : ce qui a été mesuré dans sa tranche de temps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Seau {
    pub mesures: u32,
    pub en_ligne: u32,
    pub latence_somme: u64,
    pub latence_n: u32,
    pub latence_max: u16,
}

impl Seau {
    pub fn latence_moyenne(&self) -> Option<u32> {
        (self.latence_n > 0).then(|| (self.latence_somme / self.latence_n as u64) as u32)
    }
}

/// Range `mesures` (TRIÉES) dans `nb` barres couvrant `]fin_s - duree_s, fin_s]`.
pub fn agreger(mesures: &[Mesure], fin_s: u32, duree_s: u32, nb: usize) -> Vec<Seau> {
    let mut seaux = vec![Seau::default(); nb];
    if nb == 0 || duree_s == 0 {
        return seaux;
    }
    let debut = fin_s.saturating_sub(duree_s);
    let premier = mesures.partition_point(|m| m.instant_s <= debut);
    for m in mesures[premier..].iter().take_while(|m| m.instant_s <= fin_s) {
        let rang = (((m.instant_s - debut - 1) as u64 * nb as u64) / duree_s as u64) as usize;
        let seau = &mut seaux[rang.min(nb - 1)];
        seau.mesures += 1;
        if m.en_ligne() {
            seau.en_ligne += 1;
        }
        if let Some(l) = m.latence() {
            seau.latence_somme += l as u64;
            seau.latence_n += 1;
            seau.latence_max = seau.latence_max.max(l);
        }
    }
    seaux
}

/// La disponibilité en %, sur les barres données — `None` sans aucune mesure.
pub fn disponibilite(seaux: &[Seau]) -> Option<f64> {
    let (n, ok) = seaux.iter().fold((0u64, 0u64), |(n, ok), s| (n + s.mesures as u64, ok + s.en_ligne as u64));
    (n > 0).then(|| ok as f64 * 100.0 / n as f64)
}

// ---------------------------------------------------------------------------
// LE FICHIER DES CIBLES
// ---------------------------------------------------------------------------

pub fn depuis_toml(texte: &str) -> Vec<Cible> {
    let Ok(table) = texte.parse::<toml::Table>() else { return Vec::new() };
    let Some(liste) = table.get("cible").and_then(|v| v.as_array()) else { return Vec::new() };
    let mut cibles: Vec<Cible> = Vec::new();
    for v in liste {
        // Une ligne abîmée ne coûte qu'elle-même — et une adresse que
        // `analyser_adresse` refuse aujourd'hui n'est pas surveillée.
        let (Some(id), Some(adresse)) = (v.get("id").and_then(|x| x.as_integer()), v.get("adresse").and_then(|x| x.as_str())) else {
            continue;
        };
        if id <= 0 || analyser_adresse(adresse).is_err() || cibles.iter().any(|c| c.id == id as u64) {
            continue;
        }
        cibles.push(Cible {
            id: id as u64,
            nom: v.get("nom").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            adresse: adresse.to_string(),
            http: v.get("http").and_then(|x| x.as_bool()).unwrap_or(false),
            alerte: v.get("alerte").and_then(|x| x.as_bool()).unwrap_or(true),
        });
    }
    cibles.truncate(CIBLES_MAX);
    cibles
}

pub fn vers_toml(cibles: &[Cible]) -> String {
    use toml::Value;
    let liste: Vec<Value> = cibles
        .iter()
        .map(|c| {
            let mut t = toml::Table::new();
            t.insert("id".into(), Value::Integer(c.id as i64));
            t.insert("nom".into(), Value::String(c.nom.clone()));
            t.insert("adresse".into(), Value::String(c.adresse.clone()));
            t.insert("http".into(), Value::Boolean(c.http));
            t.insert("alerte".into(), Value::Boolean(c.alerte));
            Value::Table(t)
        })
        .collect();
    let mut table = toml::Table::new();
    table.insert("cible".into(), Value::Array(liste));
    toml::to_string(&table).unwrap_or_default()
}

fn nettoyer_nom(nom: &str) -> String {
    nom.trim().chars().filter(|c| !c.is_control()).take(LONGUEUR_NOM_MAX).collect()
}

// ---------------------------------------------------------------------------
// L'ÉTAT DE L'APPLICATION (fil de l'interface)
// ---------------------------------------------------------------------------

/// Ce que le fil de l'interface demande au fil de mesure.
enum Ordre {
    /// La liste des cibles a changé ; les identifiants donnés sont à mesurer
    /// TOUT DE SUITE (une cible ajoutée ou modifiée n'attend pas une minute).
    Cibles(Vec<Cible>, Vec<u64>),
    /// « Vérifier maintenant » : toutes les cibles.
    ToutMesurer,
    /// Une cible supprimée : son historique part avec elle.
    Oublier(u64),
}

/// Ce que le fil de mesure rapporte.
enum Nouvelle {
    Historique(u64, Vec<Mesure>),
    Mesure(u64, Mesure),
}

#[derive(Default)]
pub struct Surveillance {
    pub cibles: Vec<Cible>,
    pub historiques: HashMap<u64, Vec<Mesure>>,
    pub plage: Option<Plage>,
    /// Le dernier refus d'une saisie, à montrer dans le panneau.
    pub erreur: Option<String>,
    /// Change à CHAQUE nouvelle — la clé de `cef_surveillance::push_state`.
    pub version: u64,
    /// TOUTES les alertes coupées d'un coup (voir `Cible::alerte`), sans rien
    /// toucher aux cibles : rallumer rend à chacune son propre réglage.
    pub alertes_coupees: bool,
    ordres: Option<Sender<Ordre>>,
    nouvelles: Option<Receiver<Nouvelle>>,
}

impl Surveillance {
    pub fn plage(&self) -> Plage {
        self.plage.unwrap_or(Plage::Jour)
    }

    pub fn derniere(&self, id: u64) -> Option<Mesure> {
        self.historiques.get(&id).and_then(|h| h.last()).copied()
    }

    /// Une cible au moins est injoignable À SA DERNIÈRE MESURE.
    #[cfg(test)]
    pub fn en_panne(&self) -> bool {
        self.cibles.iter().any(|c| self.derniere(c.id).is_some_and(|m| !m.en_ligne()))
    }

    /// **CE QUI FAIT CLIGNOTER L'ICÔNE** : une cible injoignable dont l'alerte
    /// est allumée, et les alertes ne sont pas toutes coupées. `en_panne`, lui,
    /// reste le FAIT — l'infobulle et le panneau le disent quoi qu'il arrive.
    pub fn alerte_active(&self) -> bool {
        !self.alertes_coupees
            && self.cibles.iter().any(|c| c.alerte && self.derniere(c.id).is_some_and(|m| !m.en_ligne()))
    }

    fn prochain_id(&self) -> u64 {
        self.cibles.iter().map(|c| c.id).max().unwrap_or(0) + 1
    }

    fn envoyer(&self, ordre: Ordre) {
        if let Some(ordres) = &self.ordres {
            let _ = ordres.send(ordre);
        }
    }
}

thread_local! {
    static ETAT: RefCell<Surveillance> = RefCell::new(Surveillance::default());
}

fn enregistrer(cibles: &[Cible]) {
    if !config::write_config_atomique(FICHIER, vers_toml(cibles)) {
        crate::diag!("[surveillance] surveillance.toml n'a pas pu être écrit");
    }
}

/// Relit `surveillance.toml` et lance le fil de mesure — une fois, au démarrage
/// (voir `app::run`).
pub fn charger() {
    let texte = config::config_path(FICHIER).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    let cibles = depuis_toml(&texte);
    let (tx_ordres, rx_ordres) = std::sync::mpsc::channel();
    let (tx_nouvelles, rx_nouvelles) = std::sync::mpsc::channel();
    let pour_le_fil = cibles.clone();
    let lance = std::thread::Builder::new()
        .name("neuradesk-surveillance".into())
        .spawn(move || fil_de_mesure(pour_le_fil, rx_ordres, tx_nouvelles));
    if let Err(e) = &lance {
        crate::diag!("[surveillance] le fil de mesure n'a pas pu démarrer : {e}");
    }
    ETAT.with(|e| {
        let mut s = e.borrow_mut();
        s.cibles = cibles;
        s.alertes_coupees = config::load_bool_setting("surveillance_alertes_coupees");
        if lance.is_ok() {
            s.ordres = Some(tx_ordres);
            s.nouvelles = Some(rx_nouvelles);
        }
    });
}

/// Lit l'état.
pub fn lire<R>(f: impl FnOnce(&Surveillance) -> R) -> R {
    ETAT.with(|e| f(&e.borrow()))
}

/// Recueille ce que le fil de mesure a rapporté — à chaque frame (voir
/// `app::run`), sans jamais attendre.
pub fn avancer() {
    ETAT.with(|e| {
        let Ok(mut s) = e.try_borrow_mut() else { return };
        let Some(nouvelles) = s.nouvelles.take() else { return };
        for nouvelle in nouvelles.try_iter() {
            match nouvelle {
                Nouvelle::Historique(id, mesures) => {
                    s.historiques.insert(id, mesures);
                }
                // Une mesure partie avant la suppression de sa cible arrive
                // après : elle ne doit rien laisser derrière elle.
                Nouvelle::Mesure(id, mesure) => {
                    if s.cibles.iter().any(|c| c.id == id) {
                        s.historiques.entry(id).or_default().push(mesure);
                    }
                }
            }
            s.version += 1;
        }
        s.nouvelles = Some(nouvelles);
    });
}

/// Une copie de la cible `id` — la SEULE source des adresses sur lesquelles
/// les outils réseau agissent (voir `win32::outils_reseau`) : jamais une
/// adresse venue de la page.
pub fn cible(id: u64) -> Option<Cible> {
    lire(|s| s.cibles.iter().find(|c| c.id == id).cloned())
}

/// La couleur de l'icône de l'applet : la couleur normale, ou — tant qu'une
/// cible est injoignable — un ROUGE qui clignote (DEMANDE EXPLICITE).
pub fn couleur_icone(normale: [f32; 3]) -> [f32; 3] {
    static DEPART: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let temps = DEPART.get_or_init(Instant::now).elapsed();
    couleur_icone_a(lire(Surveillance::alerte_active), normale, temps)
}

const ROUGE_PANNE: [f32; 3] = [1.0, 0.25, 0.25];
const PERIODE_CLIGNOTEMENT_MS: u128 = 500;

fn couleur_icone_a(panne: bool, normale: [f32; 3], temps: Duration) -> [f32; 3] {
    if panne && (temps.as_millis() / PERIODE_CLIGNOTEMENT_MS) % 2 == 0 {
        ROUGE_PANNE
    } else {
        normale
    }
}

/// L'infobulle de l'applet : « Surveillance — 3/4 en ligne », ou le nom des
/// cibles en panne.
pub fn infobulle() -> String {
    lire(|s| {
        let en_panne: Vec<&str> = s
            .cibles
            .iter()
            .filter(|c| s.derniere(c.id).is_some_and(|m| !m.en_ligne()))
            .map(|c| if c.nom.is_empty() { c.adresse.as_str() } else { c.nom.as_str() })
            .collect();
        if !en_panne.is_empty() {
            // La panne se DIT toujours ; seule l'alerte (le clignotement) se coupe.
            let coupee = if s.alerte_active() { "" } else { " (alerte coupée)" };
            return format!("Surveillance — injoignable : {}{coupee}", en_panne.join(", "));
        }
        let mesurees = s.cibles.iter().filter(|c| s.derniere(c.id).is_some()).count();
        if mesurees == 0 {
            "Surveillance".to_string()
        } else {
            format!("Surveillance — {mesurees}/{} en ligne", s.cibles.len())
        }
    })
}

fn modifier_cibles(f: impl FnOnce(&mut Surveillance) -> Result<Vec<u64>, String>) {
    ETAT.with(|e| {
        let mut s = e.borrow_mut();
        match f(&mut s) {
            Ok(a_mesurer) => {
                s.erreur = None;
                enregistrer(&s.cibles);
                let cibles = s.cibles.clone();
                s.envoyer(Ordre::Cibles(cibles, a_mesurer));
            }
            Err(message) => {
                crate::diag!("[surveillance] saisie refusée : {message}");
                s.erreur = Some(message);
            }
        }
        s.version += 1;
    });
}

/// Ajoute une cible (`id = None`) ou la remplace. L'adresse passe par
/// `analyser_adresse` : c'est ICI que la saisie de la page est validée.
pub fn enregistrer_cible(id: Option<u64>, adresse: &str, http: bool, nom: &str) {
    let adresse = adresse.trim().to_string();
    let nom = nettoyer_nom(nom);
    modifier_cibles(|s| {
        analyser_adresse(&adresse)?;
        match id {
            None => {
                if s.cibles.len() >= CIBLES_MAX {
                    return Err(format!("{CIBLES_MAX} cibles au plus"));
                }
                let id = s.prochain_id();
                s.cibles.push(Cible { id, nom, adresse, http, alerte: true });
                Ok(vec![id])
            }
            Some(id) => {
                let cible = s.cibles.iter_mut().find(|c| c.id == id).ok_or("cette cible n'existe plus")?;
                // Modifier l'adresse ou le nom ne rallume pas une alerte coupée.
                *cible = Cible { id, nom, adresse, http, alerte: cible.alerte };
                Ok(vec![id])
            }
        }
    });
}

pub fn supprimer_cible(id: u64) {
    ETAT.with(|e| e.borrow().envoyer(Ordre::Oublier(id)));
    modifier_cibles(|s| {
        s.cibles.retain(|c| c.id != id);
        s.historiques.remove(&id);
        Ok(Vec::new())
    });
}

/// Allume ou coupe l'alerte d'UNE cible — la mesure continue.
pub fn regler_alerte(id: u64, alerte: bool) {
    modifier_cibles(|s| {
        let cible = s.cibles.iter_mut().find(|c| c.id == id).ok_or("cette cible n'existe plus")?;
        cible.alerte = alerte;
        // Rien à re-mesurer : l'alerte ne change pas ce qu'on mesure.
        Ok(Vec::new())
    });
}

/// Coupe ou rallume TOUTES les alertes, retenu d'un lancement à l'autre.
pub fn couper_les_alertes(coupees: bool) {
    config::save_bool_setting("surveillance_alertes_coupees", coupees);
    ETAT.with(|e| {
        let mut s = e.borrow_mut();
        s.alertes_coupees = coupees;
        s.version += 1;
    });
}

pub fn choisir_plage(plage: Plage) {
    ETAT.with(|e| {
        let mut s = e.borrow_mut();
        s.plage = Some(plage);
        s.version += 1;
    });
}

pub fn tout_mesurer() {
    lire(|s| s.envoyer(Ordre::ToutMesurer));
}

pub fn maintenant_s() -> u32 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// LE FIL DE MESURE — tout le réseau et toute l'écriture d'historique
// ---------------------------------------------------------------------------

fn chemin_historique(id: u64) -> Option<PathBuf> {
    config::config_path(DOSSIER_HISTORIQUE).map(|d| d.join(format!("{id}.bin")))
}

/// Relit l'historique d'une cible et l'élague à un an — en RÉÉCRIVANT le
/// fichier (temporaire puis renommage) seulement s'il y avait à élaguer.
fn relire_historique(id: u64) -> Vec<Mesure> {
    let Some(chemin) = chemin_historique(id) else { return Vec::new() };
    let Ok(octets) = std::fs::read(&chemin) else { return Vec::new() };
    let mesures = decoder_historique(&octets);
    let premier = premier_a_garder(&mesures, maintenant_s());
    let gardees = mesures[premier..].to_vec();
    if premier > 0 || octets.len() != mesures.len() * TAILLE_MESURE {
        let contenu: Vec<u8> = gardees.iter().flat_map(Mesure::encoder).collect();
        let temporaire = chemin.with_extension("bin.tmp");
        let reecrit = std::fs::write(&temporaire, contenu).and_then(|()| std::fs::rename(&temporaire, &chemin));
        if let Err(e) = reecrit {
            let _ = std::fs::remove_file(&temporaire);
            crate::diag!("[surveillance] élagage de {id}.bin impossible : {e}");
        }
    }
    gardees
}

fn ajouter_a_l_historique(id: u64, mesure: &Mesure) {
    let Some(chemin) = chemin_historique(id) else { return };
    if let Some(dossier) = chemin.parent() {
        let _ = std::fs::create_dir_all(dossier);
    }
    let ecrit = std::fs::OpenOptions::new().create(true).append(true).open(&chemin).and_then(|mut f| f.write_all(&mesure.encoder()));
    if let Err(e) = ecrit {
        crate::diag!("[surveillance] écriture de {id}.bin impossible : {e}");
    }
}

/// Mesure les cibles données EN PARALLÈLE (un fil chacune : un hôte muet coûte
/// ses délais à lui seul), les écrit, et les rapporte.
fn mesurer(cibles: &[Cible], nouvelles: &Sender<Nouvelle>) {
    std::thread::scope(|portee| {
        for cible in cibles {
            portee.spawn(move || {
                let Ok(adresse) = analyser_adresse(&cible.adresse) else { return };
                let mesure = mesurer_une(&adresse, cible.http);
                ajouter_a_l_historique(cible.id, &mesure);
                let _ = nouvelles.send(Nouvelle::Mesure(cible.id, mesure));
            });
        }
    });
}

fn mesurer_une(adresse: &Adresse, http: bool) -> Mesure {
    let instant_s = maintenant_s();
    let ping_ms = resoudre(&adresse.hote).and_then(ping).unwrap_or(ECHEC);
    let (http_ms, statut) = if http {
        match verifier_http(&adresse.url) {
            Ok((statut, ms)) if (200..400).contains(&statut) => (ms, statut),
            Ok((statut, _)) => (ECHEC, statut),
            // Pas de journal ici : une cible en panne l'écrirait à chaque minute.
            Err(_) => (ECHEC, 0),
        }
    } else {
        (NON_MESURE, 0)
    };
    Mesure { instant_s, ping_ms, http_ms, statut }
}

fn fil_de_mesure(mut cibles: Vec<Cible>, ordres: Receiver<Ordre>, nouvelles: Sender<Nouvelle>) {
    for cible in &cibles {
        let _ = nouvelles.send(Nouvelle::Historique(cible.id, relire_historique(cible.id)));
    }
    let mut prochaine = Instant::now();
    loop {
        let attente = prochaine.saturating_duration_since(Instant::now());
        match ordres.recv_timeout(attente) {
            Ok(Ordre::Cibles(nouvelles_cibles, a_mesurer)) => {
                for cible in nouvelles_cibles.iter().filter(|n| !cibles.iter().any(|c| c.id == n.id)) {
                    let _ = nouvelles.send(Nouvelle::Historique(cible.id, relire_historique(cible.id)));
                }
                cibles = nouvelles_cibles;
                let tout_de_suite: Vec<Cible> = cibles.iter().filter(|c| a_mesurer.contains(&c.id)).cloned().collect();
                mesurer(&tout_de_suite, &nouvelles);
            }
            Ok(Ordre::ToutMesurer) => mesurer(&cibles, &nouvelles),
            Ok(Ordre::Oublier(id)) => {
                if let Some(chemin) = chemin_historique(id) {
                    let _ = std::fs::remove_file(chemin);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                mesurer(&cibles, &nouvelles);
                // Une mesure lente (délais HTTP) ne décale pas la cadence ;
                // une mise en veille, si — on repart d'une minute pleine.
                prochaine += INTERVALLE;
                if prochaine < Instant::now() {
                    prochaine = Instant::now() + INTERVALLE;
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// L'IPv4 d'un hôte : littérale, ou résolue par le système.
pub(crate) fn resoudre(hote: &str) -> Option<Ipv4Addr> {
    if let Ok(ip) = hote.parse::<Ipv4Addr>() {
        return Some(ip);
    }
    (hote, 0).to_socket_addrs().ok()?.find_map(|a| match a.ip() {
        std::net::IpAddr::V4(ip) => Some(ip),
        std::net::IpAddr::V6(_) => None,
    })
}

/// Un écho ICMP par `IcmpSendEcho` — SANS droits d'administrateur, à la
/// différence d'une socket brute. `None` = pas de réponse dans le délai.
fn ping(ip: Ipv4Addr) -> Option<u16> {
    const DONNEES: &[u8; 32] = b"NeuraDesk surveillance -- ping!!";
    let mut reponse = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + DONNEES.len() + 64];
    unsafe {
        let icmp = IcmpCreateFile().ok()?;
        let n = IcmpSendEcho(
            icmp,
            u32::from_ne_bytes(ip.octets()),
            DONNEES.as_ptr().cast(),
            DONNEES.len() as u16,
            None,
            reponse.as_mut_ptr().cast(),
            reponse.len() as u32,
            DELAI_PING_MS,
        );
        let _ = IcmpCloseHandle(icmp);
        if n == 0 {
            return None;
        }
        let r: ICMP_ECHO_REPLY = std::ptr::read_unaligned(reponse.as_ptr().cast());
        // `IP_SUCCESS` = 0 : tout autre statut (hôte injoignable…) est un échec.
        (r.Status == 0).then(|| plafonner(r.RoundTripTime as u128))
    }
}

/// Un écho ICMP vers UNE adresse, IPv4 ou IPv6 — le ping de la surveillance
/// adresse par adresse (l'outil IPv4 / IPv6). `None` = pas de réponse.
pub(crate) fn ping_adresse(adresse: SocketAddr) -> Option<u16> {
    match adresse {
        SocketAddr::V4(a) => ping(*a.ip()),
        SocketAddr::V6(a) => ping6(&a),
    }
}

/// `IcmpSendEcho` ne connaît que l'IPv4 : l'IPv6 passe par `Icmp6SendEcho2`,
/// sans droits d'administrateur lui aussi. L'identifiant de portée est gardé :
/// sans lui, une adresse de lien local (`fe80::`) est injoignable.
fn ping6(adresse: &SocketAddrV6) -> Option<u16> {
    const DONNEES: &[u8; 32] = b"NeuraDesk surveillance -- ping!!";
    let source = SOCKADDR_IN6 { sin6_family: AF_INET6, ..Default::default() };
    let destination = SOCKADDR_IN6 {
        sin6_family: AF_INET6,
        sin6_addr: IN6_ADDR { u: IN6_ADDR_0 { Byte: adresse.ip().octets() } },
        Anonymous: SOCKADDR_IN6_0 { sin6_scope_id: adresse.scope_id() },
        ..Default::default()
    };
    let mut reponse = vec![0u8; std::mem::size_of::<ICMPV6_ECHO_REPLY_LH>() + DONNEES.len() + 64];
    unsafe {
        let icmp = Icmp6CreateFile().ok()?;
        let n = Icmp6SendEcho2(
            icmp,
            None,
            None,
            None,
            &source,
            &destination,
            DONNEES.as_ptr().cast(),
            DONNEES.len() as u16,
            None,
            reponse.as_mut_ptr().cast(),
            reponse.len() as u32,
            DELAI_PING_MS,
        );
        let _ = IcmpCloseHandle(icmp);
        if n == 0 {
            return None;
        }
        let r: ICMPV6_ECHO_REPLY_LH = std::ptr::read_unaligned(reponse.as_ptr().cast());
        (r.Status == 0).then(|| plafonner(r.RoundTripTime as u128))
    }
}

/// Un handle WinHTTP fermé à la sortie de portée (même motif que
/// `envoi_image::Handle`).
pub(crate) struct Handle(pub *mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

pub(crate) fn ouvrir(handle: *mut core::ffi::c_void, etape: &str) -> Result<Handle, String> {
    if handle.is_null() {
        Err(format!("{etape} : {}", windows::core::Error::from_thread()))
    } else {
        Ok(Handle(handle))
    }
}

/// Un `GET` qui rend `(statut final, durée en ms)` sans lire le corps. BLOQUANT :
/// appelé depuis le fil de mesure seulement.
fn verifier_http(url: &str) -> Result<(u16, u16), String> {
    let url = decouper_url(url).ok_or("URL illisible")?;
    let agent = HSTRING::from(format!("NeuraDesk/{} (surveillance)", env!("CARGO_PKG_VERSION")));
    let debut = Instant::now();
    unsafe {
        let session = ouvrir(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0), "session")?;
        let _ = WinHttpSetTimeouts(session.0, DELAI_HTTP_MS, DELAI_HTTP_MS, DELAI_HTTP_MS, DELAI_HTTP_MS);
        // **« HAPPY EYEBALLS »** — MESURÉ : un hôte dont l'adresse IPv6 (AAAA)
        // ne répond pas faisait attendre WinHTTP ~21 s sur elle avant de
        // retomber sur l'IPv4 (qui répondait en 107 ms) : au-delà du délai,
        // donc « pas de réponse » pour un site que le navigateur — qui essaie
        // les deux en parallèle — joignait sans peine. Cette option fait
        // pareil. Windows 10 2004 et après ; un refus ne coûte que l'ancien
        // comportement.
        let _ = WinHttpSetOption(Some(session.0 as *const _), WINHTTP_OPTION_IPV6_FAST_FALLBACK, Some(&1u32.to_ne_bytes()));
        let connexion = ouvrir(WinHttpConnect(session.0, &HSTRING::from(url.hote.as_str()), url.port, 0), "connexion")?;
        let drapeaux = if url.securisee { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        let demande = ouvrir(
            WinHttpOpenRequest(connexion.0, &HSTRING::from("GET"), &HSTRING::from(url.chemin.as_str()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), drapeaux),
            "requête",
        )?;
        WinHttpSendRequest(demande.0, None, None, 0, 0, 0).map_err(|e| format!("envoi : {e}"))?;
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
        Ok((statut.min(u16::MAX as u32) as u16, plafonner(debut.elapsed().as_millis())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SONDE (ignorée : elle émet de vrais échos ICMP) — le ping par adresse,
    /// sur la BOUCLE LOCALE seulement : `127.0.0.1` et `::1` doivent répondre.
    /// `cargo test --release --bin neuradesk -- --ignored --nocapture ping_de_la_boucle`
    #[test]
    #[ignore]
    fn ping_de_la_boucle_locale_en_ipv4_et_ipv6() {
        let v4 = ping_adresse("127.0.0.1:0".parse().unwrap());
        let v6 = ping_adresse("[::1]:0".parse().unwrap());
        println!("127.0.0.1 : {v4:?}  ::1 : {v6:?}");
        assert!(v4.is_some() && v6.is_some());
    }

    fn m(instant_s: u32, ping_ms: u16, http_ms: u16, statut: u16) -> Mesure {
        Mesure { instant_s, ping_ms, http_ms, statut }
    }

    #[test]
    fn une_adresse_ip_un_hote_ou_une_url_sont_acceptes() {
        assert_eq!(analyser_adresse(" 192.168.1.205 "), Ok(Adresse { hote: "192.168.1.205".into(), url: "http://192.168.1.205/".into() }));
        assert_eq!(analyser_adresse("NAS.local"), Ok(Adresse { hote: "nas.local".into(), url: "http://nas.local/".into() }));
        let url = analyser_adresse("https://Exemple.fr:8443/etat?x=1").unwrap();
        assert_eq!(url.hote, "exemple.fr");
        assert_eq!(url.url, "https://Exemple.fr:8443/etat?x=1");
    }

    #[test]
    fn une_adresse_douteuse_est_refusee() {
        for mauvaise in ["", "   ", "exemple.fr/chemin", "exemple.fr:80", "ftp://exemple.fr", "https://", "https://[::1]/", "https://a b.fr", "http://exemple.fr:99999/", "-x.fr", "a;b"] {
            assert!(analyser_adresse(mauvaise).is_err(), "{mauvaise:?}");
        }
    }

    #[test]
    fn une_url_se_decoupe_pour_winhttp() {
        assert_eq!(
            decouper_url("https://exemple.fr:8443/etat?x=1#ancre"),
            Some(UrlDecoupee { securisee: true, hote: "exemple.fr".into(), port: 8443, chemin: "/etat?x=1".into() })
        );
        assert_eq!(decouper_url("http://nas/"), Some(UrlDecoupee { securisee: false, hote: "nas".into(), port: 80, chemin: "/".into() }));
        assert_eq!(decouper_url("HTTPS://exemple.fr?q"), Some(UrlDecoupee { securisee: true, hote: "exemple.fr".into(), port: 443, chemin: "/?q".into() }));
    }

    #[test]
    fn une_mesure_fait_l_aller_retour_en_douze_octets() {
        let mesure = m(1_790_000_000, 12, NON_MESURE, 0);
        let octets = mesure.encoder();
        assert_eq!(octets.len(), 12);
        assert_eq!(Mesure::decoder(&octets), mesure);
    }

    #[test]
    fn l_historique_ignore_une_fin_tronquee_et_un_retour_dans_le_temps() {
        let mut octets: Vec<u8> = [m(100, 1, NON_MESURE, 0), m(160, 2, NON_MESURE, 0), m(120, 3, NON_MESURE, 0), m(220, 4, NON_MESURE, 0)]
            .iter()
            .flat_map(Mesure::encoder)
            .collect();
        octets.extend_from_slice(&[1, 2, 3]);
        let relues = decoder_historique(&octets);
        assert_eq!(relues.iter().map(|m| m.ping_ms).collect::<Vec<_>>(), vec![1, 2, 4]);
    }

    #[test]
    fn l_elagage_ne_garde_qu_un_an() {
        let maintenant = 400 * 24 * 3600;
        let mesures = [m(10, 1, NON_MESURE, 0), m(maintenant - CONSERVATION_S - 1, 1, NON_MESURE, 0), m(maintenant - CONSERVATION_S, 1, NON_MESURE, 0), m(maintenant, 1, NON_MESURE, 0)];
        assert_eq!(premier_a_garder(&mesures, maintenant), 2);
        assert_eq!(premier_a_garder(&mesures, 1000), 0);
    }

    #[test]
    fn en_ligne_suit_le_http_quand_il_est_verifie() {
        assert!(m(0, 5, NON_MESURE, 0).en_ligne());
        assert!(!m(0, ECHEC, NON_MESURE, 0).en_ligne());
        // Le site est tombé, le serveur pingue encore : EN PANNE.
        assert!(!m(0, 5, ECHEC, 503).en_ligne());
        // Le serveur bloque l'ICMP mais son site répond : EN LIGNE, et la
        // latence tracée est celle du HTTP.
        let web = m(0, ECHEC, 80, 200);
        assert!(web.en_ligne());
        assert_eq!(web.latence(), Some(80));
        assert!(m(0, 5, 40, 301).en_ligne());
    }

    #[test]
    fn l_agregation_range_chaque_mesure_dans_sa_barre() {
        // Une heure en 60 barres, finissant à 3600 : la barre k couvre ]60k, 60(k+1)].
        let mesures = [m(0, 9, NON_MESURE, 0), m(30, 10, NON_MESURE, 0), m(60, 20, NON_MESURE, 0), m(61, ECHEC, NON_MESURE, 0), m(3600, 30, NON_MESURE, 0), m(3601, 1, NON_MESURE, 0)];
        let seaux = agreger(&mesures, 3600, 3600, 60);
        assert_eq!(seaux.len(), 60);
        assert_eq!(seaux[0].mesures, 2, "30 et 60");
        assert_eq!(seaux[0].latence_moyenne(), Some(15));
        assert_eq!(seaux[1], Seau { mesures: 1, en_ligne: 0, latence_somme: 0, latence_n: 0, latence_max: 0 });
        assert_eq!(seaux[59].latence_max, 30);
        assert_eq!(seaux.iter().map(|s| s.mesures).sum::<u32>(), 4, "0 et 3601 hors de la plage");
        assert_eq!(disponibilite(&seaux), Some(75.0));
        assert_eq!(disponibilite(&agreger(&[], 3600, 3600, 60)), None);
    }

    #[test]
    fn chaque_plage_a_son_nom_et_son_decoupage() {
        for plage in Plage::TOUTES {
            assert_eq!(Plage::depuis_nom(plage.nom()), Some(plage));
            let (duree, nb) = plage.decoupage();
            assert_eq!(duree % nb as u32, 0, "{:?} : des barres de durée entière", plage);
        }
        assert_eq!(Plage::depuis_nom("2h"), None);
    }

    #[test]
    fn le_fichier_des_cibles_fait_l_aller_retour_et_ecarte_les_lignes_abimees() {
        let cibles = vec![
            Cible { id: 1, nom: "NAS « maison »".into(), adresse: "192.168.1.205".into(), http: false, alerte: false },
            Cible { id: 4, nom: String::new(), adresse: "https://exemple.fr/".into(), http: true, alerte: true },
        ];
        assert_eq!(depuis_toml(&vers_toml(&cibles)), cibles);
        let abime = "[[cible]]\nid = 1\nadresse = \"a b\"\n[[cible]]\nnom = \"sans id\"\nadresse = \"x.fr\"\n[[cible]]\nid = 2\nadresse = \"x.fr\"\n[[cible]]\nid = 2\nadresse = \"y.fr\"\n";
        assert_eq!(depuis_toml(abime).iter().map(|c| c.adresse.as_str()).collect::<Vec<_>>(), vec!["x.fr"]);
        assert!(depuis_toml("n'importe quoi").is_empty());
    }

    #[test]
    fn l_icone_clignote_en_rouge_seulement_en_panne() {
        let normale = [0.9, 0.9, 0.9];
        assert_eq!(couleur_icone_a(false, normale, Duration::from_millis(0)), normale);
        assert_eq!(couleur_icone_a(true, normale, Duration::from_millis(100)), ROUGE_PANNE);
        assert_eq!(couleur_icone_a(true, normale, Duration::from_millis(600)), normale);
    }

    #[test]
    fn en_panne_regarde_la_derniere_mesure_de_chaque_cible() {
        let mut s = Surveillance { cibles: vec![Cible { id: 1, nom: String::new(), adresse: "x.fr".into(), http: false, alerte: true }], ..Default::default() };
        assert!(!s.en_panne(), "jamais mesurée : pas en panne");
        s.historiques.insert(1, vec![m(1, ECHEC, NON_MESURE, 0), m(2, 3, NON_MESURE, 0)]);
        assert!(!s.en_panne());
        s.historiques.get_mut(&1).unwrap().push(m(3, ECHEC, NON_MESURE, 0));
        assert!(s.en_panne());
    }

    /// Couper l'alerte n'efface pas la panne : elle éteint le clignotement, par
    /// cible ou pour toutes, et rallumer rend à chaque cible son réglage.
    #[test]
    fn couper_l_alerte_garde_la_panne_et_eteint_le_clignotement() {
        let cible = |id, alerte| Cible { id, nom: String::new(), adresse: "x.fr".into(), http: false, alerte };
        let mut s = Surveillance { cibles: vec![cible(1, true), cible(2, false)], ..Default::default() };
        s.historiques.insert(1, vec![m(1, 3, NON_MESURE, 0)]);
        s.historiques.insert(2, vec![m(1, ECHEC, NON_MESURE, 0)]);
        assert!(s.en_panne(), "la cible 2 est en panne");
        assert!(!s.alerte_active(), "mais son alerte est coupée");
        s.historiques.get_mut(&1).unwrap().push(m(2, ECHEC, NON_MESURE, 0));
        assert!(s.alerte_active(), "la cible 1, en panne et alerte allumée");
        s.alertes_coupees = true;
        assert!(s.en_panne() && !s.alerte_active(), "tout coupé : la panne reste un fait");
    }

    #[test]
    fn un_nom_est_nettoye_et_borne() {
        assert_eq!(nettoyer_nom("  NAS\n "), "NAS");
        assert_eq!(nettoyer_nom(&"x".repeat(200)).chars().count(), LONGUEUR_NOM_MAX);
    }
}
