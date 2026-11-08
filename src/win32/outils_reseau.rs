//! **LES OUTILS RÉSEAU de la Surveillance** — sur une cible ENREGISTRÉE, à la
//! demande. DEMANDE EXPLICITE (2026-09-26) : « des outils réseaux pour les
//! adresses enregistrées » — retenus : traceroute, DNS, test de ports,
//! certificat HTTPS et test IPv4/IPv6 (ce dernier né du défaut de `f17db37` :
//! une AAAA morte qui faisait attendre 21 s).
//!
//! Tout passe par les API de Windows déjà liées (IP Helper, DNS, WinHTTP) et
//! la bibliothèque standard : AUCUNE dépendance nouvelle.
//!
//! ⚠️ **L'ADRESSE VIENT DE `surveillance::cible`, JAMAIS DE LA PAGE** : la page
//! ne désigne qu'un IDENTIFIANT ; seule la liste des ports est saisie, et elle
//! est validée ici (`analyser_ports`, 16 ports au plus — pas de balayage).
//!
//! ⚠️ **UN OUTIL = UN FIL** (`neuradesk-outil-reseau`), qui rapporte ligne à
//! ligne par canal ; `avancer` draine à chaque frame. Un seul rapport à la fois :
//! en lancer un autre LÂCHE le canal du précédent, dont le fil s'arrête au
//! prochain envoi refusé. Rien ne tourne sans un clic — et **JAMAIS EN TEST** :
//! tout ce qui se décide ou se met en forme est PUR.

use std::cell::{Cell, RefCell};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::NetworkManagement::Dns::{
    DnsFree, DnsFreeRecordList, DnsQuery_W, DNS_QUERY_STANDARD, DNS_RECORDA, DNS_RECORDW, DNS_TYPE, DNS_TYPE_A,
    DNS_TYPE_AAAA, DNS_TYPE_CNAME, DNS_TYPE_MX, DNS_TYPE_NS, DNS_TYPE_PTR, DNS_TYPE_TEXT,
};
use windows::Win32::NetworkManagement::IpHelper::{
    IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY, IP_OPTION_INFORMATION, IP_SUCCESS,
    IP_TTL_EXPIRED_TRANSIT,
};
use windows::Win32::Networking::WinHttp::{
    WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryOption, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetOption, WinHttpSetTimeouts, SECURITY_FLAG_IGNORE_CERT_CN_INVALID, SECURITY_FLAG_IGNORE_CERT_DATE_INVALID,
    SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE, SECURITY_FLAG_IGNORE_UNKNOWN_CA, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_CERTIFICATE_INFO, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_IPV6_FAST_FALLBACK,
    WINHTTP_OPTION_SECURITY_CERTIFICATE_STRUCT, WINHTTP_OPTION_SECURITY_FLAGS,
};

use crate::win32::surveillance::{self, Handle};

/// Au-delà, ce serait un balayage — que l'autre bout peut prendre pour une
/// attaque. De quoi couvrir `SERVICES_COURANTS` et quelques ports à soi.
pub const PORTS_MAX: usize = 64;
/// Les services TCP courants — d'un serveur, d'un NAS, d'un réseau local —
/// proposés par défaut au test de ports (DEMANDE du 2026-09-26 : « les ports
/// de tous les services courants »), triés, et le nom affiché de chacun. Rien
/// en UDP (WireGuard, SNMP…) : une connexion TCP ne dirait rien de ces ports.
const SERVICES_COURANTS: [(u16, &str); 46] = [
    (21, "FTP"),
    (22, "SSH"),
    (23, "Telnet"),
    (25, "SMTP"),
    (53, "DNS"),
    (80, "HTTP"),
    (110, "POP3"),
    (111, "RPC"),
    (135, "RPC Windows"),
    (139, "NetBIOS"),
    (143, "IMAP"),
    (389, "LDAP"),
    (443, "HTTPS"),
    (445, "SMB"),
    (465, "SMTPS"),
    (548, "AFP"),
    (587, "SMTP"),
    (631, "Impression IPP"),
    (636, "LDAPS"),
    (873, "rsync"),
    (993, "IMAPS"),
    (995, "POP3S"),
    (1433, "SQL Server"),
    (1883, "MQTT"),
    (2049, "NFS"),
    (3000, "Gitea, Grafana"),
    (3306, "MySQL"),
    (3389, "Bureau à distance"),
    (5000, "Synology"),
    (5001, "Synology HTTPS"),
    (5432, "PostgreSQL"),
    (5900, "VNC"),
    (5985, "WinRM"),
    (6379, "Redis"),
    (8000, "HTTP"),
    (8006, "Proxmox"),
    (8080, "HTTP"),
    (8123, "Home Assistant"),
    (8443, "HTTPS"),
    (8883, "MQTTS"),
    (9000, "Portainer"),
    (9090, "Cockpit"),
    (9100, "Imprimante"),
    (25565, "Minecraft"),
    (27017, "MongoDB"),
    (32400, "Plex"),
];
const SAUTS_MAX: u8 = 30;
/// Un traceroute vers une box qui ne répond pas au ping ne finirait jamais :
/// passé ce nombre de sauts muets D'AFFILÉE, on s'arrête — mais SEULEMENT si
/// la destination ne répond pas au ping (voir `arreter_sur_muets`).
const SAUTS_MUETS_MAX: u8 = 5;
const DELAI_SAUT_MS: u32 = 1_000;
const DELAI_CONNEXION: Duration = Duration::from_secs(5);
const DELAI_HTTPS_MS: i32 = 10_000;
/// Un certificat qui expire dans moins de jours que ceci est signalé.
const JOURS_ALERTE_CERTIFICAT: i64 = 14;

// ---------------------------------------------------------------------------
// LE MODÈLE
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outil {
    Traceroute,
    Dns,
    Ports,
    Certificat,
    Familles,
}

impl Outil {
    pub const TOUS: [Outil; 5] = [Outil::Traceroute, Outil::Dns, Outil::Ports, Outil::Certificat, Outil::Familles];

    pub fn nom(self) -> &'static str {
        match self {
            Outil::Traceroute => "traceroute",
            Outil::Dns => "dns",
            Outil::Ports => "ports",
            Outil::Certificat => "certificat",
            Outil::Familles => "familles",
        }
    }

    pub fn depuis_nom(nom: &str) -> Option<Outil> {
        Outil::TOUS.into_iter().find(|o| o.nom() == nom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Niveau {
    Info,
    Ok,
    Alerte,
    Erreur,
}

impl Niveau {
    pub fn nom(self) -> &'static str {
        match self {
            Niveau::Info => "info",
            Niveau::Ok => "ok",
            Niveau::Alerte => "alerte",
            Niveau::Erreur => "erreur",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ligne {
    pub niveau: Niveau,
    pub texte: String,
}

fn ligne(niveau: Niveau, texte: impl Into<String>) -> Ligne {
    Ligne { niveau, texte: texte.into() }
}

/// Le rapport affiché : la cible dont la section « Outils » est ouverte, et
/// l'outil lancé dessus (`None` = section ouverte, rien de lancé).
pub struct Rapport {
    pub cible_id: u64,
    pub outil: Option<Outil>,
    pub en_cours: bool,
    pub lignes: Vec<Ligne>,
}

enum Evenement {
    Ligne(Ligne),
    Fin,
}

thread_local! {
    static RAPPORT: RefCell<Option<Rapport>> = const { RefCell::new(None) };
    static CANAL: RefCell<Option<Receiver<Evenement>>> = const { RefCell::new(None) };
    /// Change à chaque nouveauté — la clé de `cef_surveillance::push_state`.
    static VERSION: Cell<u64> = const { Cell::new(0) };
}

fn changer(f: impl FnOnce(&mut Option<Rapport>)) {
    RAPPORT.with(|r| {
        if let Ok(mut r) = r.try_borrow_mut() {
            f(&mut r);
        }
    });
    VERSION.with(|v| v.set(v.get() + 1));
}

pub fn version() -> u64 {
    VERSION.with(Cell::get)
}

pub fn lire<R>(f: impl FnOnce(Option<&Rapport>) -> R) -> R {
    RAPPORT.with(|r| f(r.borrow().as_ref()))
}

/// Ouvre la section « Outils » d'une cible, sans rien lancer.
pub fn ouvrir(cible_id: u64) {
    CANAL.with(|c| *c.borrow_mut() = None);
    changer(|r| *r = Some(Rapport { cible_id, outil: None, en_cours: false, lignes: Vec::new() }));
}

/// Ferme la section — et lâche le canal, ce qui arrête un outil en cours.
pub fn fermer() {
    CANAL.with(|c| *c.borrow_mut() = None);
    changer(|r| *r = None);
}

/// Lance `outil` sur la cible `cible_id`. `ports` : la saisie de la page, pour
/// le seul test de ports (sinon ignorée).
pub fn lancer(cible_id: u64, outil: Outil, ports: Option<&str>) {
    let mut rapport = Rapport { cible_id, outil: Some(outil), en_cours: false, lignes: Vec::new() };
    CANAL.with(|c| *c.borrow_mut() = None);
    match preparer(cible_id, outil, ports) {
        Err(message) => rapport.lignes.push(ligne(Niveau::Erreur, message)),
        Ok(contexte) => {
            let (tx, rx) = std::sync::mpsc::channel();
            let lance = std::thread::Builder::new()
                .name("neuradesk-outil-reseau".into())
                .spawn(move || executer(outil, &contexte, &tx));
            match lance {
                Ok(_) => {
                    rapport.en_cours = true;
                    CANAL.with(|c| *c.borrow_mut() = Some(rx));
                }
                Err(e) => rapport.lignes.push(ligne(Niveau::Erreur, format!("l'outil n'a pas pu démarrer : {e}"))),
            }
        }
    }
    changer(|r| *r = Some(rapport));
}

/// Recueille ce que le fil de l'outil a rapporté — à chaque frame, sans
/// jamais attendre.
pub fn avancer() {
    let evenements: Vec<Option<Evenement>> = CANAL.with(|c| {
        let Ok(mut canal) = c.try_borrow_mut() else { return Vec::new() };
        let Some(rx) = canal.as_ref() else { return Vec::new() };
        let mut recus = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(e) => recus.push(Some(e)),
                Err(TryRecvError::Empty) => break,
                // Le fil est mort sans dire « fin » (panique) : on clôt quand même.
                Err(TryRecvError::Disconnected) => {
                    recus.push(None);
                    *canal = None;
                    break;
                }
            }
        }
        recus
    });
    if evenements.is_empty() {
        return;
    }
    let journal = std::env::var_os("NEURADESK_DEBUG_OUTIL").is_some();
    changer(|r| {
        let Some(r) = r else { return };
        for evenement in evenements {
            match evenement {
                Some(Evenement::Ligne(l)) => {
                    if journal {
                        crate::diag!("[outils] {} {}", l.niveau.nom(), l.texte);
                    }
                    r.lignes.push(l);
                }
                Some(Evenement::Fin) => r.en_cours = false,
                None => {
                    if r.en_cours {
                        r.lignes.push(ligne(Niveau::Erreur, "l'outil s'est arrêté avant la fin"));
                        r.en_cours = false;
                    }
                }
            }
        }
    });
}

/// Levier du harnais : `NEURADESK_DEBUG_OUTIL=<id>/<outil>` lance l'outil à la
/// première ouverture du panneau (voir `cef_surveillance::toggle`) ; le
/// rapport part aussi au journal (`NEURADESK_DIAG=1`).
pub fn lancer_depuis_env() {
    static FAIT: std::sync::Once = std::sync::Once::new();
    let Ok(valeur) = std::env::var("NEURADESK_DEBUG_OUTIL") else { return };
    FAIT.call_once(|| {
        let Some((id, nom)) = valeur.split_once('/') else { return };
        match (id.parse::<u64>(), Outil::depuis_nom(nom)) {
            (Ok(id), Some(outil)) => lancer(id, outil, None),
            _ => crate::diag!("[outils] NEURADESK_DEBUG_OUTIL illisible : {valeur:?}"),
        }
    });
}

// ---------------------------------------------------------------------------
// LES DÉCISIONS ET LES MISES EN FORME (pures)
// ---------------------------------------------------------------------------

/// **LA FRONTIÈRE des ports** : « 22, 80 443;8080 » → `[22, 80, 443, 8080]`.
/// Doublons retirés, ordre gardé ; ni 0, ni plage, ni plus de `PORTS_MAX`.
pub fn analyser_ports(texte: &str) -> Result<Vec<u16>, String> {
    let mut ports = Vec::new();
    for morceau in texte.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|m| !m.is_empty()) {
        let port = morceau
            .parse::<u16>()
            .ok()
            .filter(|&p| p > 0)
            .ok_or_else(|| format!("« {} » n'est pas un port (1 à 65535)", morceau.chars().take(20).collect::<String>()))?;
        if !ports.contains(&port) {
            ports.push(port);
        }
    }
    if ports.is_empty() {
        return Err("aucun port à tester".into());
    }
    if ports.len() > PORTS_MAX {
        return Err(format!("{PORTS_MAX} ports au plus"));
    }
    Ok(ports)
}

/// Les ports proposés pour une cible : les courants, plus celui de son service.
pub fn ports_par_defaut(port_service: u16) -> Vec<u16> {
    let mut ports: Vec<u16> = SERVICES_COURANTS.iter().map(|&(port, _)| port).collect();
    if !ports.contains(&port_service) {
        ports.push(port_service);
    }
    ports.sort_unstable();
    ports
}

/// Les ports proposés dans la page pour une cible, déjà en texte.
pub fn ports_proposes(adresse: &str) -> String {
    let port = surveillance::analyser_adresse(adresse)
        .ok()
        .and_then(|a| surveillance::decouper_url(&a.url))
        .map_or(80, |u| u.port);
    texte_ports(&ports_par_defaut(port))
}

pub fn texte_ports(ports: &[u16]) -> String {
    ports.iter().map(u16::to_string).collect::<Vec<_>>().join(", ")
}

fn nom_du_port(port: u16) -> Option<&'static str> {
    SERVICES_COURANTS.iter().find(|&&(p, _)| p == port).map(|&(_, nom)| nom)
}

/// Le rapport d'un test de ports : chaque port OUVERT sur sa ligne, dans
/// l'ordre saisi ; les autres REGROUPÉS par motif (« fermé », « filtré »…) —
/// une ligne par port noierait les 2 ou 3 ouverts parmi 46.
fn bilan_ports(resultats: &[(u16, Result<u128, (Niveau, &str)>)]) -> Vec<Ligne> {
    let mut lignes = Vec::new();
    let mut groupes: Vec<(Niveau, &str, Vec<u16>)> = Vec::new();
    for (port, resultat) in resultats {
        match resultat {
            Ok(ms) => lignes.push(ligne(Niveau::Ok, format!("{}  ouvert ({} ms)", etiquette_port(*port), ms.max(&1)))),
            Err((niveau, motif)) => match groupes.iter_mut().find(|g| g.1 == *motif) {
                Some(groupe) => groupe.2.push(*port),
                None => groupes.push((*niveau, motif, vec![*port])),
            },
        }
    }
    for (niveau, motif, ports) in groupes {
        lignes.push(ligne(niveau, format!("{motif} ({}) : {}", ports.len(), texte_ports(&ports))));
    }
    let ouverts = resultats.iter().filter(|r| r.1.is_ok()).count();
    lignes.push(ligne(Niveau::Info, format!("{ouverts} ouvert(s) sur {}", resultats.len())));
    lignes
}

fn etiquette_port(port: u16) -> String {
    match nom_du_port(port) {
        Some(nom) => format!("{port} ({nom})"),
        None => port.to_string(),
    }
}

/// S'arrêter après `muets` sauts muets d'affilée ? Seulement quand la
/// destination ne répond pas au ping : si elle répond, des routeurs muets au
/// milieu n'empêchent pas d'arriver, on va donc jusqu'au bout.
fn arreter_sur_muets(muets: u8, destination_repond: bool) -> bool {
    !destination_repond && muets >= SAUTS_MUETS_MAX
}

/// Une ligne de traceroute : « 3  192.168.1.1  4 ms » ou « 3  * ».
fn texte_saut(ttl: u8, reponse: Option<(Ipv4Addr, u32)>) -> String {
    match reponse {
        Some((ip, ms)) => format!("{ttl:>2}  {ip}  {} ms", ms.max(1)),
        None => format!("{ttl:>2}  *  pas de réponse"),
    }
}

/// Le nom inverse d'une IPv4, pour la requête PTR : `82.234.67.39` →
/// `39.67.234.82.in-addr.arpa`.
fn nom_inverse(ip: Ipv4Addr) -> String {
    let [a, b, c, d] = ip.octets();
    format!("{d}.{c}.{b}.{a}.in-addr.arpa")
}

/// Un « nom distinctif » X.500 tel que WinHTTP le rend (une ligne par attribut)
/// ramené à une ligne.
fn nom_distinctif(texte: &str) -> String {
    texte.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(", ")
}

/// Le nom commun (CN) d'un sujet tel que WinHTTP le rend : sa DERNIÈRE ligne.
fn nom_commun(sujet: &str) -> Option<&str> {
    sujet.lines().map(str::trim).filter(|l| !l.is_empty()).last()
}

/// Le nom `cn` d'un certificat couvre-t-il `hote` ? Joker `*.` sur UN niveau.
fn nom_couvert(cn: &str, hote: &str) -> bool {
    let (cn, hote) = (cn.to_ascii_lowercase(), hote.to_ascii_lowercase());
    match cn.strip_prefix("*.") {
        Some(domaine) => hote.split_once('.').is_some_and(|(premier, reste)| !premier.is_empty() && reste == domaine),
        None => cn == hote,
    }
}

/// **Pourquoi Windows a refusé** — WinHTTP ne rend qu'un refus GÉNÉRIQUE
/// (12175) ; le certificat relu dit le reste. MESURÉ sur la box (2026-09-26) :
/// « certificat REFUSÉ : certificat refusé » ne disait rien, alors que le
/// certificat était émis pour `…fbxos.fr` et interrogé par son IP.
fn raison_du_refus(hote: &str, cn: Option<&str>, debut_s: i64, expiration_s: i64, maintenant_s: i64) -> String {
    if maintenant_s < debut_s || maintenant_s > expiration_s {
        return "raison : hors de sa période de validité".into();
    }
    match cn {
        Some(cn) if !nom_couvert(cn, hote) => format!("raison probable : émis pour « {cn} », pas pour « {hote} »"),
        _ => "raison probable : autorité inconnue de Windows (auto-signé ou autorité privée)".into(),
    }
}

/// Un `FILETIME` (centaines de ns depuis 1601) en secondes Unix.
fn filetime_vers_unix(ft: u64) -> i64 {
    (ft as i64 - 116_444_736_000_000_000) / 10_000_000
}

/// `JJ/MM/AAAA` (UTC) d'un instant Unix — l'algorithme « days from civil »
/// de H. Hinnant, à l'envers.
fn date_utc(unix_s: i64) -> String {
    let jours = unix_s.div_euclid(86_400) + 719_468;
    let ere = jours.div_euclid(146_097);
    let jour_ere = jours - ere * 146_097;
    let annee_ere = (jour_ere - jour_ere / 1460 + jour_ere / 36_524 - jour_ere / 146_096) / 365;
    let jour_annee = jour_ere - (365 * annee_ere + annee_ere / 4 - annee_ere / 100);
    let mp = (5 * jour_annee + 2) / 153;
    let jour = jour_annee - (153 * mp + 2) / 5 + 1;
    let mois = if mp < 10 { mp + 3 } else { mp - 9 };
    let annee = annee_ere + ere * 400 + i64::from(mois <= 2);
    format!("{jour:02}/{mois:02}/{annee}")
}

fn jours_restants(expiration_s: i64, maintenant_s: i64) -> i64 {
    (expiration_s - maintenant_s).div_euclid(86_400)
}

fn ligne_expiration(expiration_s: i64, maintenant_s: i64) -> Ligne {
    let jours = jours_restants(expiration_s, maintenant_s);
    let date = date_utc(expiration_s);
    if jours < 0 {
        ligne(Niveau::Erreur, format!("EXPIRÉ depuis le {date} ({} j)", -jours))
    } else if jours < JOURS_ALERTE_CERTIFICAT {
        ligne(Niveau::Alerte, format!("expire le {date} — dans {jours} j seulement"))
    } else {
        ligne(Niveau::Ok, format!("expire le {date} — dans {jours} j"))
    }
}

/// Le motif d'un refus TLS de WinHTTP, en clair.
fn motif_tls(code: u32) -> Option<&'static str> {
    Some(match code {
        12037 => "certificat expiré ou pas encore valide",
        12038 => "certificat émis pour un autre nom",
        12045 => "autorité de certification inconnue (auto-signé ?)",
        12169 => "certificat invalide",
        12170 => "certificat révoqué",
        12179 => "certificat non prévu pour un serveur web",
        12175 => "certificat refusé",
        12157 => "échec de la négociation TLS",
        _ => return None,
    })
}

fn motif_winhttp(code: u32) -> String {
    match code {
        12002 => "délai dépassé".into(),
        12007 => "nom introuvable".into(),
        12029 => "connexion impossible".into(),
        12030 => "connexion coupée".into(),
        12152 => "réponse illisible (le port parle-t-il HTTPS ?)".into(),
        autre => motif_tls(autre).map(str::to_string).unwrap_or_else(|| format!("erreur {autre}")),
    }
}

/// Le code Win32 porté par une erreur WinHTTP (`HRESULT_FROM_WIN32`).
fn code_win32(e: &windows::core::Error) -> u32 {
    e.code().0 as u32 & 0xFFFF
}

/// Le verdict du test IPv4/IPv6. `resultats` : (est IPv6, a répondu).
fn conclusion_familles(resultats: &[(bool, bool)]) -> Ligne {
    let v4: Vec<bool> = resultats.iter().filter(|r| !r.0).map(|r| r.1).collect();
    let v6: Vec<bool> = resultats.iter().filter(|r| r.0).map(|r| r.1).collect();
    let v4_ok = v4.iter().any(|&ok| ok);
    let v6_ok = v6.iter().any(|&ok| ok);
    match (v4.is_empty(), v6.is_empty()) {
        (true, true) => ligne(Niveau::Erreur, "aucune adresse"),
        _ if !v4_ok && !v6_ok => ligne(Niveau::Erreur, "aucune adresse ne répond"),
        (false, false) if v4_ok && !v6_ok => ligne(
            Niveau::Alerte,
            "l'IPv6 ne répond pas : un client sans repli rapide attend son délai (souvent ~21 s) avant de passer à l'IPv4",
        ),
        (false, false) if !v4_ok => ligne(Niveau::Alerte, "l'IPv4 ne répond pas, seul l'IPv6 passe"),
        (false, true) => ligne(Niveau::Info, "pas d'adresse IPv6 (AAAA) : IPv4 seulement, qui répond"),
        (true, false) => ligne(Niveau::Info, "pas d'adresse IPv4 (A) : IPv6 seulement, qui répond"),
        _ => ligne(Niveau::Ok, "IPv4 et IPv6 répondent tous les deux"),
    }
}

// ---------------------------------------------------------------------------
// L'EXÉCUTION (fil de l'outil — réseau)
// ---------------------------------------------------------------------------

struct Contexte {
    hote: String,
    /// Le port du service surveillé : celui de l'URL, sinon 80 (443 en HTTPS).
    port_service: u16,
    /// Le port HTTPS à interroger pour le certificat.
    port_https: u16,
    ports: Vec<u16>,
    /// La cible est vérifiée en HTTP : sinon elle ne l'est QUE par ping, et
    /// l'outil IPv4 / IPv6 doit pinguer — son port web peut être fermé.
    http: bool,
}

fn preparer(cible_id: u64, outil: Outil, ports: Option<&str>) -> Result<Contexte, String> {
    let cible = surveillance::cible(cible_id).ok_or("cette cible n'existe plus")?;
    let adresse = surveillance::analyser_adresse(&cible.adresse)?;
    let url = surveillance::decouper_url(&adresse.url).ok_or("URL illisible")?;
    let ports = match (outil, ports) {
        (Outil::Ports, Some(texte)) => analyser_ports(texte)?,
        _ => ports_par_defaut(url.port),
    };
    Ok(Contexte { hote: adresse.hote, port_service: url.port, port_https: if url.securisee { url.port } else { 443 }, ports, http: cible.http })
}

/// Rapporte une ligne ; `false` = plus personne n'écoute (rapport remplacé ou
/// fermé), l'outil doit s'arrêter.
fn dire(tx: &Sender<Evenement>, niveau: Niveau, texte: impl Into<String>) -> bool {
    tx.send(Evenement::Ligne(ligne(niveau, texte))).is_ok()
}

fn executer(outil: Outil, c: &Contexte, tx: &Sender<Evenement>) {
    match outil {
        Outil::Traceroute => traceroute(c, tx),
        Outil::Dns => dns(c, tx),
        Outil::Ports => ports(c, tx),
        Outil::Certificat => certificat(c, tx),
        Outil::Familles => familles(c, tx),
    }
    let _ = tx.send(Evenement::Fin);
}

fn traceroute(c: &Contexte, tx: &Sender<Evenement>) {
    let Some(destination) = surveillance::resoudre(&c.hote) else {
        dire(tx, Niveau::Erreur, format!("{} : aucune adresse IPv4", c.hote));
        return;
    };
    if !dire(tx, Niveau::Info, format!("vers {destination}, {SAUTS_MAX} sauts au plus")) {
        return;
    }
    let Ok(icmp) = (unsafe { IcmpCreateFile() }) else {
        dire(tx, Niveau::Erreur, "ICMP indisponible");
        return;
    };
    const DONNEES: &[u8; 32] = b"NeuraDesk surveillance -- trace!";
    let mut reponse = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + DONNEES.len() + 64];
    // Un écho par saut ; `None` pour le TTL = celui du système (ping ordinaire).
    let mut echo = |ttl: Option<u8>| -> Option<(Ipv4Addr, u32, u32)> {
        let options = ttl.map(|ttl| IP_OPTION_INFORMATION { Ttl: ttl, ..Default::default() });
        let n = unsafe {
            IcmpSendEcho(
                icmp,
                u32::from_ne_bytes(destination.octets()),
                DONNEES.as_ptr().cast(),
                DONNEES.len() as u16,
                options.as_ref().map(|o| o as *const _),
                reponse.as_mut_ptr().cast(),
                reponse.len() as u32,
                DELAI_SAUT_MS,
            )
        };
        let r: Option<ICMP_ECHO_REPLY> = (n > 0).then(|| unsafe { std::ptr::read_unaligned(reponse.as_ptr().cast()) });
        r.map(|r| (Ipv4Addr::from(r.Address.to_ne_bytes()), r.RoundTripTime, r.Status))
    };
    // DEMANDE (2026-09-26) : dire D'ABORD si la destination répond au ping.
    // MESURÉ sur prdom.freeboxos.fr : `tracert` de Windows reste muet du saut
    // 3 au saut 30 — une box qui refuse le ping n'est JAMAIS « atteinte ».
    let destination_repond = matches!(echo(None), Some((_, _, IP_SUCCESS)));
    if !destination_repond
        && !dire(tx, Niveau::Alerte, "la destination ne répond pas au ping : le traceroute ne pourra pas aboutir (seuls les premiers sauts seront visibles)")
    {
        return;
    }
    let mut muets = 0u8;
    for ttl in 1..=SAUTS_MAX {
        let saut = echo(Some(ttl));
        let (niveau, texte, fini) = match saut {
            Some((ip, ms, IP_SUCCESS)) => (Niveau::Ok, format!("{} — arrivé", texte_saut(ttl, Some((ip, ms)))), true),
            Some((ip, ms, IP_TTL_EXPIRED_TRANSIT)) => (Niveau::Info, texte_saut(ttl, Some((ip, ms))), false),
            Some((ip, ms, statut)) => (Niveau::Erreur, format!("{} — injoignable (statut {statut})", texte_saut(ttl, Some((ip, ms)))), true),
            None => (Niveau::Info, texte_saut(ttl, None), false),
        };
        muets = if saut.is_some() { 0 } else { muets + 1 };
        if !dire(tx, niveau, texte) || fini {
            break;
        }
        if arreter_sur_muets(muets, destination_repond) {
            dire(tx, Niveau::Alerte, format!("{SAUTS_MUETS_MAX} sauts muets de suite : arrêt"));
            break;
        }
        if ttl == SAUTS_MAX {
            dire(tx, Niveau::Alerte, "destination non atteinte");
        }
    }
    unsafe {
        let _ = IcmpCloseHandle(icmp);
    }
}

/// Les enregistrements d'un type, en lignes — vide si aucun.
fn requete_dns(nom: &str, type_: DNS_TYPE) -> Result<Vec<String>, u32> {
    let mut resultats: *mut DNS_RECORDW = std::ptr::null_mut();
    let statut = unsafe {
        DnsQuery_W(
            &HSTRING::from(nom),
            type_,
            DNS_QUERY_STANDARD,
            None,
            (&mut resultats as *mut *mut DNS_RECORDW).cast::<*mut DNS_RECORDA>(),
            None,
        )
    };
    // 9501 = DNS_INFO_NO_RECORDS : le nom existe, pas ce type.
    if statut.0 == 9501 {
        return Ok(Vec::new());
    }
    if statut.0 != 0 {
        return Err(statut.0);
    }
    let mut lignes = Vec::new();
    let mut courant = resultats;
    unsafe {
        while let Some(r) = courant.as_ref() {
            // Une requête A rend AUSSI la chaîne CNAME : on ne garde que le type demandé.
            if r.wType == type_.0 {
                let texte = match type_ {
                    DNS_TYPE_A => Some(Ipv4Addr::from(r.Data.A.IpAddress.to_ne_bytes()).to_string()),
                    DNS_TYPE_AAAA => Some(std::net::Ipv6Addr::from(r.Data.AAAA.Ip6Address.IP6Byte).to_string()),
                    DNS_TYPE_CNAME | DNS_TYPE_NS | DNS_TYPE_PTR => r.Data.PTR.pNameHost.to_string().ok(),
                    DNS_TYPE_MX => r.Data.MX.pNameExchange.to_string().ok().map(|h| format!("{} (priorité {})", h, r.Data.MX.wPreference)),
                    DNS_TYPE_TEXT => {
                        let txt = &r.Data.TXT;
                        let chaines = std::slice::from_raw_parts(txt.pStringArray.as_ptr(), txt.dwStringCount as usize);
                        Some(chaines.iter().filter_map(|s| s.to_string().ok()).collect::<String>())
                    }
                    _ => None,
                };
                if let Some(texte) = texte {
                    lignes.push(format!("{texte}  (TTL {} s)", r.dwTtl));
                }
            }
            courant = r.pNext;
        }
        if !resultats.is_null() {
            DnsFree(Some(resultats as *const _), DnsFreeRecordList);
        }
    }
    lignes.dedup();
    Ok(lignes)
}

fn dns(c: &Contexte, tx: &Sender<Evenement>) {
    let debut = Instant::now();
    let (nom, types): (String, &[(DNS_TYPE, &str)]) = match c.hote.parse::<Ipv4Addr>() {
        Ok(ip) => (nom_inverse(ip), &[(DNS_TYPE_PTR, "PTR")]),
        Err(_) => (
            c.hote.clone(),
            &[(DNS_TYPE_CNAME, "CNAME"), (DNS_TYPE_A, "A"), (DNS_TYPE_AAAA, "AAAA"), (DNS_TYPE_MX, "MX"), (DNS_TYPE_NS, "NS"), (DNS_TYPE_TEXT, "TXT")],
        ),
    };
    let mut trouves = 0;
    for &(type_, etiquette) in types {
        match requete_dns(&nom, type_) {
            Ok(lignes) => {
                for l in lignes {
                    trouves += 1;
                    if !dire(tx, Niveau::Info, format!("{etiquette:<5} {l}")) {
                        return;
                    }
                }
            }
            // 9003 = DNS_ERROR_RCODE_NAME_ERROR : le nom n'existe pas.
            Err(9003) => {
                dire(tx, Niveau::Erreur, format!("{nom} : ce nom n'existe pas"));
                return;
            }
            Err(code) => {
                if !dire(tx, Niveau::Alerte, format!("{etiquette:<5} échec de la requête (erreur {code})")) {
                    return;
                }
            }
        }
    }
    let ms = debut.elapsed().as_millis();
    if trouves == 0 {
        dire(tx, Niveau::Alerte, format!("aucun enregistrement ({ms} ms)"));
    } else {
        dire(tx, Niveau::Ok, format!("{trouves} enregistrement(s) en {ms} ms"));
    }
}

/// Une tentative de connexion TCP : `Ok(ms)`, ou le motif de l'échec.
fn connecter(adresse: SocketAddr) -> Result<u128, (Niveau, &'static str)> {
    let debut = Instant::now();
    match TcpStream::connect_timeout(&adresse, DELAI_CONNEXION) {
        Ok(_) => Ok(debut.elapsed().as_millis()),
        Err(e) => Err(match e.kind() {
            std::io::ErrorKind::ConnectionRefused => (Niveau::Info, "fermé"),
            std::io::ErrorKind::TimedOut => (Niveau::Alerte, "filtré (pas de réponse)"),
            _ => (Niveau::Erreur, "injoignable"),
        }),
    }
}

fn adresses(hote: &str, port: u16) -> Vec<SocketAddr> {
    let mut toutes: Vec<SocketAddr> = (hote, port).to_socket_addrs().map(|a| a.collect()).unwrap_or_default();
    toutes.dedup();
    toutes
}

fn ports(c: &Contexte, tx: &Sender<Evenement>) {
    let toutes = adresses(&c.hote, 0);
    let Some(ip) = toutes.iter().find(|a| a.is_ipv4()).or(toutes.first()).map(SocketAddr::ip) else {
        dire(tx, Niveau::Erreur, format!("{} : aucune adresse", c.hote));
        return;
    };
    if !dire(tx, Niveau::Info, format!("sur {ip} — délai {} s par port", DELAI_CONNEXION.as_secs())) {
        return;
    }
    // Tous les ports EN MÊME TEMPS (`PORTS_MAX` au plus) : le test dure un
    // délai de connexion, pas un par port.
    let resultats: Vec<(u16, Result<u128, (Niveau, &str)>)> = std::thread::scope(|s| {
        let fils: Vec<_> = c.ports.iter().map(|&p| (p, s.spawn(move || connecter(SocketAddr::new(ip, p))))).collect();
        fils.into_iter().map(|(p, f)| (p, f.join().unwrap_or(Err((Niveau::Erreur, "échec du test"))))).collect()
    });
    for l in bilan_ports(&resultats) {
        if !dire(tx, l.niveau, l.texte) {
            return;
        }
    }
}

/// Chaque adresse est testée COMME LA SURVEILLANCE TESTE LA CIBLE : par une
/// connexion TCP au port du service si elle est vérifiée en HTTP, par un ping
/// sinon. Toujours en TCP, une cible surveillée par ping seul dont le port 80
/// est fermé (un serveur sans site web) sortait « aucune adresse ne répond »
/// alors que son ping passait.
fn familles(c: &Contexte, tx: &Sender<Evenement>) {
    let toutes = adresses(&c.hote, c.port_service);
    let annonce = if c.http {
        format!("connexion TCP au port {} de chaque adresse de {}", c.port_service, c.hote)
    } else {
        format!("ping de chaque adresse de {}", c.hote)
    };
    if !dire(tx, Niveau::Info, annonce) {
        return;
    }
    let http = c.http;
    let resultats: Vec<Result<u128, (Niveau, &str)>> = std::thread::scope(|s| {
        let fils: Vec<_> = toutes
            .iter()
            .map(|&a| {
                s.spawn(move || {
                    if http {
                        connecter(a)
                    } else {
                        surveillance::ping_adresse(a).map(u128::from).ok_or((Niveau::Erreur, "pas de réponse au ping"))
                    }
                })
            })
            .collect();
        fils.into_iter().map(|f| f.join().unwrap_or(Err((Niveau::Erreur, "échec du test")))).collect()
    });
    let mut bilan = Vec::new();
    for (adresse, resultat) in toutes.iter().zip(&resultats) {
        let famille = if adresse.is_ipv6() { "IPv6" } else { "IPv4" };
        let (niveau, texte) = match resultat {
            Ok(ms) => (Niveau::Ok, format!("{famille}  {}  répond en {} ms", adresse.ip(), ms.max(&1))),
            Err((_, motif)) => (Niveau::Erreur, format!("{famille}  {}  {motif}", adresse.ip())),
        };
        bilan.push((adresse.is_ipv6(), resultat.is_ok()));
        if !dire(tx, niveau, texte) {
            return;
        }
    }
    let conclusion = conclusion_familles(&bilan);
    dire(tx, conclusion.niveau, conclusion.texte);
}

/// Une chaîne allouée par WinHTTP (`LocalFree` à la charge de l'appelant).
unsafe fn chaine_locale(p: windows::core::PWSTR) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let texte = unsafe { p.to_string().ok() };
    unsafe {
        let _ = LocalFree(Some(HLOCAL(p.0.cast())));
    }
    texte.filter(|t| !t.trim().is_empty())
}

/// Une requête `HEAD /` en HTTPS. `ignorer` : accepter un certificat invalide
/// — pour pouvoir le LIRE après qu'une première tentative l'a refusé.
/// Rend la requête, que le certificat se lise dessus.
fn poignee_tls(connexion: &Handle, ignorer: bool) -> Result<Handle, u32> {
    unsafe {
        let demande = surveillance::ouvrir(
            WinHttpOpenRequest(connexion.0, &HSTRING::from("HEAD"), &HSTRING::from("/"), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE),
            "requête",
        )
        .map_err(|_| 0u32)?;
        if ignorer {
            let drapeaux = SECURITY_FLAG_IGNORE_UNKNOWN_CA
                | SECURITY_FLAG_IGNORE_CERT_DATE_INVALID
                | SECURITY_FLAG_IGNORE_CERT_CN_INVALID
                | SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE;
            let _ = WinHttpSetOption(Some(demande.0 as *const _), WINHTTP_OPTION_SECURITY_FLAGS, Some(&drapeaux.to_ne_bytes()));
        }
        WinHttpSendRequest(demande.0, None, None, 0, 0, 0).map_err(|e| code_win32(&e))?;
        WinHttpReceiveResponse(demande.0, std::ptr::null_mut()).map_err(|e| code_win32(&e))?;
        Ok(demande)
    }
}

fn certificat(c: &Contexte, tx: &Sender<Evenement>) {
    if !dire(tx, Niveau::Info, format!("https://{}:{}/", c.hote, c.port_https)) {
        return;
    }
    let agent = HSTRING::from(format!("NeuraDesk/{} (surveillance)", env!("CARGO_PKG_VERSION")));
    let ouvertes = unsafe {
        surveillance::ouvrir(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0), "session").and_then(|session| {
            let _ = WinHttpSetTimeouts(session.0, DELAI_HTTPS_MS, DELAI_HTTPS_MS, DELAI_HTTPS_MS, DELAI_HTTPS_MS);
            let _ = WinHttpSetOption(Some(session.0 as *const _), WINHTTP_OPTION_IPV6_FAST_FALLBACK, Some(&1u32.to_ne_bytes()));
            let connexion = surveillance::ouvrir(WinHttpConnect(session.0, &HSTRING::from(c.hote.as_str()), c.port_https, 0), "connexion")?;
            Ok((session, connexion))
        })
    };
    // `session` vit jusqu'au bout : WinHTTP ferme ses enfants avec elle.
    let (_session, connexion) = match ouvertes {
        Ok(h) => h,
        Err(e) => {
            dire(tx, Niveau::Erreur, e);
            return;
        }
    };
    // Le refus GÉNÉRIQUE (12175) n'est expliqué qu'une fois le certificat relu.
    let mut refus_generique = false;
    let demande = match poignee_tls(&connexion, false) {
        Ok(d) => {
            dire(tx, Niveau::Ok, "certificat accepté par Windows");
            d
        }
        Err(code) if motif_tls(code).is_some() => {
            refus_generique = code == 12175;
            let texte = if refus_generique { "certificat REFUSÉ par Windows".to_string() } else { format!("certificat REFUSÉ : {}", motif_winhttp(code)) };
            dire(tx, Niveau::Erreur, texte);
            match poignee_tls(&connexion, true) {
                Ok(d) => d,
                Err(code) => {
                    dire(tx, Niveau::Erreur, format!("illisible : {}", motif_winhttp(code)));
                    return;
                }
            }
        }
        Err(code) => {
            dire(tx, Niveau::Erreur, motif_winhttp(code));
            return;
        }
    };
    let mut info = WINHTTP_CERTIFICATE_INFO::default();
    let mut taille = std::mem::size_of::<WINHTTP_CERTIFICATE_INFO>() as u32;
    let lu = unsafe {
        WinHttpQueryOption(demande.0, WINHTTP_OPTION_SECURITY_CERTIFICATE_STRUCT, Some((&mut info as *mut WINHTTP_CERTIFICATE_INFO).cast()), &mut taille)
    };
    if lu.is_err() {
        dire(tx, Niveau::Erreur, "le certificat n'a pas pu être lu");
        return;
    }
    let (sujet, emetteur) = unsafe {
        // Toutes les chaînes sont à LIBÉRER, même celles qu'on n'affiche pas.
        let _ = chaine_locale(info.lpszProtocolName);
        let _ = chaine_locale(info.lpszSignatureAlgName);
        let _ = chaine_locale(info.lpszEncryptionAlgName);
        (chaine_locale(info.lpszSubjectInfo), chaine_locale(info.lpszIssuerInfo))
    };
    let ft = |f: windows::Win32::Foundation::FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    let maintenant = surveillance::maintenant_s() as i64;
    let (debut_s, expiration_s) = (filetime_vers_unix(ft(info.ftStart)), filetime_vers_unix(ft(info.ftExpiry)));
    let mut lignes = vec![
        ligne(Niveau::Info, format!("pour : {}", sujet.as_deref().map(nom_distinctif).unwrap_or_else(|| "?".into()))),
        ligne(Niveau::Info, format!("émis par : {}", emetteur.as_deref().map(nom_distinctif).unwrap_or_else(|| "?".into()))),
        ligne(Niveau::Info, format!("valide depuis le {}", date_utc(debut_s))),
        ligne_expiration(expiration_s, maintenant),
    ];
    if refus_generique {
        let raison = raison_du_refus(&c.hote, sujet.as_deref().and_then(nom_commun), debut_s, expiration_s, maintenant);
        lignes.insert(0, ligne(Niveau::Erreur, raison));
    }
    for l in lignes {
        if tx.send(Evenement::Ligne(l)).is_err() {
            return;
        }
    }
    if info.dwKeySize > 0 {
        dire(tx, Niveau::Info, format!("chiffrement : clé de {} bits", info.dwKeySize));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_ports_se_saisissent_librement_mais_bornes() {
        assert_eq!(analyser_ports(" 22, 80 443;8080,80 "), Ok(vec![22, 80, 443, 8080]));
        assert!(analyser_ports("").is_err());
        assert!(analyser_ports("0").is_err());
        assert!(analyser_ports("70000").is_err());
        assert!(analyser_ports("22-80").is_err());
        assert!(analyser_ports("ssh").is_err());
        let trop: String = (1..=PORTS_MAX as u16 + 1).map(|p| format!("{p},")).collect();
        assert!(analyser_ports(&trop).is_err());
        let juste: String = (1..=PORTS_MAX as u16).map(|p| format!("{p} ")).collect();
        assert_eq!(analyser_ports(&juste).map(|p| p.len()), Ok(PORTS_MAX));
    }

    #[test]
    fn les_ports_par_defaut_incluent_celui_du_service() {
        let courants = ports_par_defaut(80);
        assert_eq!(courants.len(), SERVICES_COURANTS.len());
        for port in [22, 80, 443, 445, 3389, 3000, 8080, 25565] {
            assert!(courants.contains(&port), "{port} manque");
        }
        let avec_service = ports_par_defaut(38080);
        assert_eq!(avec_service.len(), SERVICES_COURANTS.len() + 1);
        assert_eq!(avec_service.last(), Some(&38080));
        assert!(ports_par_defaut(443).windows(2).all(|w| w[0] < w[1]), "trié, sans doublon");
        assert_eq!(etiquette_port(22), "22 (SSH)");
        assert_eq!(etiquette_port(38080), "38080");
    }

    #[test]
    fn les_services_courants_tiennent_dans_la_limite_et_passent_la_frontiere() {
        // Service ajouté en plus : la liste proposée doit rester acceptée.
        assert!(SERVICES_COURANTS.len() < PORTS_MAX);
        assert!(SERVICES_COURANTS.windows(2).all(|w| w[0].0 < w[1].0), "triés, sans doublon");
        assert_eq!(analyser_ports(&texte_ports(&ports_par_defaut(38080))), Ok(ports_par_defaut(38080)));
    }

    #[test]
    fn le_bilan_des_ports_detaille_les_ouverts_et_regroupe_le_reste() {
        let resultats = [
            (21, Err((Niveau::Info, "fermé"))),
            (22, Ok(3)),
            (23, Err((Niveau::Alerte, "filtré (pas de réponse)"))),
            (80, Err((Niveau::Info, "fermé"))),
            (443, Ok(0)),
        ];
        let textes: Vec<String> = bilan_ports(&resultats).into_iter().map(|l| l.texte).collect();
        assert_eq!(
            textes,
            [
                "22 (SSH)  ouvert (3 ms)",
                "443 (HTTPS)  ouvert (1 ms)",
                "fermé (2) : 21, 80",
                "filtré (pas de réponse) (1) : 23",
                "2 ouvert(s) sur 5",
            ]
        );
    }

    #[test]
    fn un_outil_se_nomme_dans_les_deux_sens() {
        for outil in Outil::TOUS {
            assert_eq!(Outil::depuis_nom(outil.nom()), Some(outil));
        }
        assert_eq!(Outil::depuis_nom("nmap"), None);
    }

    #[test]
    fn le_nom_inverse_renverse_les_octets() {
        assert_eq!(nom_inverse(Ipv4Addr::new(82, 234, 67, 39)), "39.67.234.82.in-addr.arpa");
    }

    #[test]
    fn les_sauts_muets_n_arretent_que_si_la_destination_ne_repond_pas() {
        assert!(arreter_sur_muets(SAUTS_MUETS_MAX, false));
        assert!(!arreter_sur_muets(SAUTS_MUETS_MAX - 1, false));
        assert!(!arreter_sur_muets(SAUTS_MUETS_MAX + 3, true));
    }

    #[test]
    fn un_saut_se_lit() {
        assert_eq!(texte_saut(3, Some((Ipv4Addr::new(192, 168, 1, 1), 0))), " 3  192.168.1.1  1 ms");
        assert_eq!(texte_saut(12, None), "12  *  pas de réponse");
    }

    #[test]
    fn les_dates_du_certificat() {
        // 1601 → 1970 : le décalage connu des FILETIME.
        assert_eq!(filetime_vers_unix(116_444_736_000_000_000), 0);
        assert_eq!(date_utc(0), "01/01/1970");
        assert_eq!(date_utc(951_782_400), "29/02/2000");
        assert_eq!(date_utc(1_790_380_800), "26/09/2026");
        let jour = 86_400;
        assert_eq!(jours_restants(10 * jour, 0), 10);
        assert_eq!(jours_restants(-1, 0), -1);
        assert_eq!(ligne_expiration(100 * jour, 0).niveau, Niveau::Ok);
        assert_eq!(ligne_expiration(3 * jour, 0).niveau, Niveau::Alerte);
        assert_eq!(ligne_expiration(-3 * jour, 0).niveau, Niveau::Erreur);
    }

    #[test]
    fn un_nom_distinctif_tient_sur_une_ligne() {
        assert_eq!(nom_distinctif("FR\r\nParis\r\n\r\nexemple.fr\r\n"), "FR, Paris, exemple.fr");
    }

    #[test]
    fn le_refus_generique_s_explique_par_le_certificat() {
        assert_eq!(nom_commun("FR\r\nbox.exemple.fr\r\n"), Some("box.exemple.fr"));
        assert!(nom_couvert("Exemple.fr", "exemple.fr"));
        assert!(nom_couvert("*.exemple.fr", "www.exemple.fr"));
        assert!(!nom_couvert("*.exemple.fr", "exemple.fr"));
        assert!(!nom_couvert("*.exemple.fr", "a.b.exemple.fr"));
        // Le cas mesuré : la box interrogée par son IP.
        let r = raison_du_refus("192.168.1.254", Some("box.exemple.fr"), 0, 100, 50);
        assert!(r.contains("box.exemple.fr") && r.contains("192.168.1.254"), "{r}");
        assert!(raison_du_refus("exemple.fr", Some("exemple.fr"), 0, 100, 150).contains("validité"));
        assert!(raison_du_refus("exemple.fr", Some("exemple.fr"), 0, 100, 50).contains("autorité"));
    }

    #[test]
    fn les_refus_tls_sont_reconnus() {
        assert!(motif_tls(12045).is_some());
        assert!(motif_tls(12002).is_none());
        assert_eq!(motif_winhttp(12002), "délai dépassé");
        assert_eq!(motif_winhttp(99), "erreur 99");
    }

    #[test]
    fn le_verdict_ipv4_ipv6() {
        // Le cas de `f17db37` : une AAAA morte, l'IPv4 vivante.
        assert_eq!(conclusion_familles(&[(false, true), (true, false)]).niveau, Niveau::Alerte);
        assert_eq!(conclusion_familles(&[(false, true), (true, true)]).niveau, Niveau::Ok);
        assert_eq!(conclusion_familles(&[(false, true)]).niveau, Niveau::Info);
        assert_eq!(conclusion_familles(&[(false, false), (true, false)]).niveau, Niveau::Erreur);
        assert_eq!(conclusion_familles(&[]).niveau, Niveau::Erreur);
        assert_eq!(conclusion_familles(&[(false, false), (true, true)]).niveau, Niveau::Alerte);
    }
}
