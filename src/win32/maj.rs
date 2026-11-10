//! **LA MISE À JOUR DE NEURADESK, DEPUIS LA DERNIÈRE RELEASE GITHUB.**
//!
//! DEMANDE (2026-11-10) : « j'aimerais implémenter un système de mise à jour par
//! rapport à la version publiée sur github pour que l'app propose de mettre à
//! jour et fasse sa propre mise à jour ».
//!
//! # Le dépôt et la convention qu'il faut respecter
//!
//! La release est lue sur `https://api.github.com/repos/axalox/NeuraDesk/releases/latest`
//! et l'archive est l'asset **`NeuraDesk-portable.zip`** — le nom FIXE de la
//! convention de publication ([[neuradesk-gitea-remote-and-releases]] : le tag
//! porte la version, jamais le nom de l'asset). Le tag doit commencer par
//! `portable-` : c'est ce qui distingue cette lignée de `master-portable-` ou de
//! `neurodesk-portable-`, et **une release d'une autre lignée ne déclenche
//! jamais** de mise à jour (voir `est_plus_recente`).
//!
//! La version locale est `NEURADESK_PUBLICATION`, que `build.rs` tire de
//! `git describe --tags --abbrev=0` au moment du build. ⚠️ **Elle doit donc
//! être posée AVANT le build final** — c'est déjà la règle de publication.
//!
//! # Ce que fait ce module, et dans quel ordre
//!
//! 1. **Vérification** (fil réseau) : une requête JSON, comparée à la version
//!    locale. Si une version plus récente existe, elle est proposée.
//! 2. **Téléchargement** (fil réseau) : l'archive de ~277 Mo est écrite dans
//!    `%LOCALAPPDATA%\NeuraDesk\maj\`, avec progression. Si la release publie un
//!    asset `.sha256` à côté de l'archive, l'empreinte est vérifiée pendant
//!    l'écriture (l'archive n'est acceptée qu'une fois la comparaison faite).
//! 3. **Extraction** (fil réseau) : le zip est déplié dans un dossier FRÈRE du
//!    dossier de l'application, `<dossier>/.<nom>.maj-<date>`. Cela se fait
//!    pendant que l'application tourne — **aucun fichier en cours d'exécution
//!    n'est touché**, et c'est ce qui rend l'installation finale presque
//!    instantanée.
//! 4. **Installation** : à la demande explicite de l'utilisateur, l'application
//!    se ferme et un **assistant** (une copie de l'exécutable, placée dans le
//!    dossier temporaire) attend sa mort, renomme l'ancien dossier en
//!    `<dossier>.ancien-<date>`, met le nouveau à sa place, relance
//!    l'application, vérifie qu'elle est vivante après dix secondes, puis
//!    supprime l'ancien dossier. Si la nouvelle version ne démarre pas, il
//!    **revient en arrière** et relance l'ancienne.
//!
//! ⛔ **POURQUOI UN ASSISTANT SÉPARÉ** : Windows verrouille l'image d'un
//! exécutable en cours d'exécution — un processus ne peut pas remplacer son
//! propre fichier. L'assistant est une COPIE de l'exécutable, donc son image
//! n'est pas le fichier à remplacer. Il est lancé avec
//! `CREATE_BREAKAWAY_FROM_JOB` : le Job Object de `cef_app::contain_child_processes`
//! tue les enfants à la fermeture du parent, et l'assistant DOIT survivre à
//! cette fermeture.
//!
//! ⚠️ **LE TÉLÉCHARGEMENT ET L'EXTRACTION NE SONT PAS INSTRUMENTÉS PAR LE
//! HARNAIS DE CAPTURE** : `avancer` ne fait rien si `NEURADESK_CAPTURE` ou
//! `NEURADESK_CAPTURE_FRAMES` est positionnée (une requête réseau et une
//! écriture de 600 Mo n'ont rien à faire dans un run de capture), et
//! `NEURADESK_NO_MAJ` coupe tout le module — y compris l'installation.
//! `NEURADESK_MAJ_DOSSIER` remplace le dossier de l'application (seul levier
//! d'essai de l'assistant sans risquer le build courant).
//!
//! # L'anti-rebond et l'état persisté
//!
//! `%APPDATA%\NeuraDesk\maj.toml` retient la date de la dernière vérification
//! (pour ne pas interroger GitHub à chaque lancement) et la dernière version
//! pour laquelle un toast a été montré (pour ne pas le montrer deux fois).
//! Une vérification MANUELLE (bouton de la page, entrée du menu) ignore les deux.
//!
//! ⚠️ **Aucun test de ce module ne fait de réseau.** Les fonctions pures
//! (`est_plus_recente`, `chemin_relatif`, `lire_reponse` sur un JSON figé) sont
//! éprouvées hors ligne ; la sonde réseau est `#[ignore]`.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_LOCATION, WINHTTP_QUERY_STATUS_CODE,
};

use crate::win32::surveillance::{decouper_url, ouvrir};

/// Le dépôt publié — la seule source de mise à jour.
const DEPOT: &str = "axalox/NeuraDesk";
/// L'API des releases. `latest` rend la dernière release NON préliminaire.
const API_DERNIERE: &str = "https://api.github.com/repos/axalox/NeuraDesk/releases/latest";
/// La page que le toast ouvre quand l'installation automatique n'est pas possible.
const PAGE_RELEASES: &str = "https://github.com/axalox/NeuraDesk/releases/latest";
/// Le nom FIXE de l'archive portable (voir la doc de module).
const ASSET: &str = "NeuraDesk-portable.zip";
/// Le préfixe des tags de CETTE lignée.
const PREFIXE_TAG: &str = "portable-";
/// La version compilée dans ce binaire.
const ACTUELLE: &str = env!("NEURADESK_PUBLICATION");

/// Délai de toute opération réseau — un PLAFOND, pas une attente.
const DELAI_RESEAU_MS: i32 = 20_000;
/// Deux vérifications silencieuses ne se suivent pas de plus près.
const ENTRE_DEUX_VERIFICATIONS: Duration = Duration::from_secs(5 * 60);
/// Une vérification automatique n'a lieu que si la dernière date de plus de…
const PERIODE_AUTOMATIQUE: Duration = Duration::from_secs(6 * 3600);
/// Laisse le démarrage se poser avant d'interroger le réseau.
const DELAI_PREMIERE_VERIFICATION: Duration = Duration::from_secs(20);
/// Au-delà, l'archive n'est pas celle d'un portable NeuraDesk (garde anti-zip-bomb).
const TAILLE_MAX_DECOMPRESSEE: u64 = 4 * 1024 * 1024 * 1024;
/// Le dossier de l'application doit contenir ce fichier, sinon l'archive est refusée.
const EXECUTABLE: &str = "neuradesk.exe";
/// Nombre de redirections suivies (GitHub redirige vers `objects.githubusercontent.com`).
const REDIRECTIONS_MAX: usize = 6;
/// Taille du tampon de lecture pendant le téléchargement.
const TAMPON_OCTETS: usize = 256 * 1024;
/// Ne publier la progression que tous les… octets (le JSON n'a pas à changer
/// soixante fois par seconde).
const PAS_DE_PROGRESSION: u64 = 512 * 1024;
/// Le fichier d'état persisté.
const FICHIER_ETAT: &str = "maj.toml";
/// Combien de temps l'assistant attend la fin du processus parent.
const ATTENTE_PARENT_MS: u32 = 120_000;
/// Combien de temps on réessaie de renommer le dossier de l'application (les
/// sous-processus CEF peuvent mettre un instant à rendre leurs handles).
const ATTENTE_VERROU: Duration = Duration::from_secs(60);
/// Combien de temps la nouvelle version doit rester vivante pour être déclarée
/// bonne. En dessous, elle est crue plantée et on revient en arrière.
const ESSAI_DE_VIE: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------
// L'état, et sa génération
// ---------------------------------------------------------------------------

/// **CE QUE FAIT LA MISE À JOUR, EN UN SEUL ENDROIT.** Chaque variante est un
/// état stable que la page « À propos » sait rendre ; `generation` numérote
/// chaque changement pour que `cef_apropos` ne pousse dans la page que ce qui a
/// bougé (voir `refresh_if_visible`).
#[derive(Clone, Debug, PartialEq)]
enum Etat {
    /// Rien n'a encore été demandé.
    Inconnu,
    /// Une requête est en cours.
    Verification,
    /// À jour (ou aucune release utilisable).
    AJour { version: String },
    /// Une version plus récente est proposée.
    Disponible { version: String, page: String, archive: Archive },
    /// L'archive descend.
    Telechargement { version: String, recu: u64, total: Option<u64> },
    /// Le zip est déplié dans le dossier de préparation.
    Extraction { version: String, faits: u64, total: u64 },
    /// Tout est prêt : il ne manque que le redémarrage.
    Pret { version: String },
    /// La dernière opération a échoué (le message est montré tel quel).
    Erreur { message: String },
}

/// L'archive désignée par la release.
#[derive(Clone, Debug, PartialEq)]
struct Archive {
    url: String,
    taille: Option<u64>,
    /// L'asset `.sha256`, quand la release en publie un.
    somme_sha256: Option<String>,
}

/// Ce que l'assistant doit échanger contre le dossier de l'application.
#[derive(Clone, Debug)]
struct Preparation {
    version: String,
    /// Le dossier extrait qui contient `neuradesk.exe`.
    racine: PathBuf,
    /// Le dossier de l'application (la cible du renommage).
    dossier: PathBuf,
    /// Le dossier de préparation (à nettoyer si `racine` n'est pas lui-même).
    temporaire: PathBuf,
}

static ETAT: Mutex<Etat> = Mutex::new(Etat::Inconnu);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static VERIFICATION_EN_COURS: AtomicBool = AtomicBool::new(false);
static TELECHARGEMENT_EN_COURS: AtomicBool = AtomicBool::new(false);
static ANNULER: AtomicBool = AtomicBool::new(false);
/// Le dossier prêt à installer, et le drapeau qui autorise `main` à le faire.
static PRET: Mutex<Option<Preparation>> = Mutex::new(None);
static APPLIQUER: AtomicBool = AtomicBool::new(false);
/// La notification à déposer depuis le fil de l'interface (voir `avancer`).
static NOTIFICATION: Mutex<Option<(String, String)>> = Mutex::new(None);

/// La dernière vérification de la session — pour l'anti-rebond de `verifier`.
static DERNIERE_VERIFICATION: Mutex<Option<Instant>> = Mutex::new(None);
/// L'horloge utilisée par `avancer` pour la première vérification.
static DEMARRE: Mutex<Option<Instant>> = Mutex::new(None);
/// Quand la prochaine vérification AUTOMATIQUE est due (voir `avancer`). Elle
/// part de la date persistée : un lancement qui suit une vérification récente
/// n'en refait pas une.
static PROCHAINE_AUTOMATIQUE: Mutex<Option<Instant>> = Mutex::new(None);
static NETTOYAGE_FAIT: AtomicBool = AtomicBool::new(false);

fn poser_etat(etat: Etat) {
    if let Ok(mut courant) = ETAT.lock() {
        *courant = etat;
    }
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

fn etat_courant() -> Etat {
    ETAT.lock().map(|e| e.clone()).unwrap_or(Etat::Inconnu)
}

/// **LE NUMÉRO D'ÉTAT** — lu par `cef_apropos::refresh_if_visible` pour ne
/// pousser dans la page que lorsque quelque chose a changé.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// L'état persisté (`maj.toml`)
// ---------------------------------------------------------------------------

/// L'horodatage de la dernière vérification, et la version déjà notifiée.
fn lire_persiste() -> (u64, Option<String>) {
    let Some(chemin) = crate::config::config_path(FICHIER_ETAT) else { return (0, None) };
    let Ok(texte) = std::fs::read_to_string(chemin) else { return (0, None) };
    let Ok(table) = texte.parse::<toml::Table>() else { return (0, None) };
    let derniere = table.get("derniere_verification").and_then(|v| v.as_integer()).unwrap_or(0).max(0) as u64;
    let notifiee = table.get("version_notifiee").and_then(|v| v.as_str()).map(str::to_string);
    (derniere, notifiee)
}

fn ecrire_persiste(derniere: u64, notifiee: Option<&str>) {
    let Some(chemin) = crate::config::config_path(FICHIER_ETAT) else { return };
    let mut texte = format!("derniere_verification = {derniere}\n");
    if let Some(version) = notifiee {
        texte.push_str(&format!("version_notifiee = {version:?}\n"));
    }
    let _ = std::fs::write(chemin, texte);
}

fn maintenant_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Les versions — pures, donc testables
// ---------------------------------------------------------------------------

/// **LA CLÉ DE TRI D'UN TAG `portable-AAAA-MM-JJ<suffixe>`.**
///
/// Rend `None` pour tout ce qui n'est pas de cette lignée : `master-portable-…`
/// et `neurodesk-portable-…` sont d'AUTRES applications, et la comparaison ne
/// doit jamais les rapprocher. Le suffixe (`""`, `b`, `c`…) départage deux
/// publications du même jour — c'est exactement la convention en vigueur
/// (« un tag déjà publié ne se déplace pas, on suffixe `b` »).
fn cle_de_version(tag: &str) -> Option<(i32, i32, i32, String)> {
    let reste = tag.strip_prefix(PREFIXE_TAG)?;
    let date = reste.get(..10)?;
    let suffixe = reste.get(10..)?;
    let mut morceaux = date.split('-');
    let annee = morceaux.next()?.parse().ok()?;
    let mois = morceaux.next()?.parse().ok()?;
    let jour = morceaux.next()?.parse().ok()?;
    if morceaux.next().is_some() {
        return None;
    }
    if !(1..=12).contains(&mois) || !(1..=31).contains(&jour) {
        return None;
    }
    if !suffixe.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some((annee, mois, jour, suffixe.to_string()))
}

/// **LA RÈGLE DE DÉCISION.** `true` ⇒ la version distante doit être proposée.
///
/// * deux tags `portable-…` se comparent par date puis par suffixe ;
/// * une version locale qui n'est PAS de cette lignée (build de développement,
///   `inconnue`, autre lignée) est considérée plus ancienne : la release est
///   proposée ;
/// * un tag distant hors de la lignée (`neurodesk-portable-…`) ne déclenche
///   JAMAIS rien — ce n'est pas notre version.
pub fn est_plus_recente(distant: &str, locale: &str) -> bool {
    match (cle_de_version(distant), cle_de_version(locale)) {
        (Some(d), Some(l)) => d > l,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

/// La taille en unités lisibles (le même format que la page des Réglages).
pub fn taille_lisible(octets: u64) -> String {
    const UNITES: [&str; 4] = ["o", "Ko", "Mo", "Go"];
    let mut valeur = octets as f64;
    let mut rang = 0;
    while valeur >= 1024.0 && rang + 1 < UNITES.len() {
        valeur /= 1024.0;
        rang += 1;
    }
    if rang == 0 {
        format!("{octets} o")
    } else {
        format!("{valeur:.1} {}", UNITES[rang])
    }
}

// ---------------------------------------------------------------------------
// La réponse de GitHub — pure elle aussi
// ---------------------------------------------------------------------------

/// Le résultat exploitable d'une release.
#[derive(Clone, Debug, PartialEq)]
struct SortieGithub {
    tag: String,
    page: String,
    archive: Archive,
}

/// **LIT LA RÉPONSE DE L'API.** Rend une erreur quand la release n'a pas
/// l'archive portable, ou quand l'URL de cette archive ne pointe pas vers
/// `github.com/axalox/NeuraDesk/releases/download/` — une réponse détournée ne
/// doit pas pouvoir désigner un autre hôte.
fn lire_reponse(valeur: &Value) -> Result<SortieGithub, String> {
    let tag = valeur.get("tag_name").and_then(Value::as_str).ok_or("la release n'a pas de tag")?;
    let prefixe_depot = format!("https://github.com/{DEPOT}/");
    let prefixe_archive = format!("https://github.com/{DEPOT}/releases/download/");
    let page = valeur.get("html_url").and_then(Value::as_str).unwrap_or(PAGE_RELEASES).to_string();
    let page = if page.starts_with(&prefixe_depot) { page } else { PAGE_RELEASES.to_string() };
    let assets = valeur.get("assets").and_then(Value::as_array).ok_or("la release n'a pas d'assets")?;

    let mut archive = None;
    let mut somme = None;
    for asset in assets {
        let Some(nom) = asset.get("name").and_then(Value::as_str) else { continue };
        let Some(url) = asset.get("browser_download_url").and_then(Value::as_str) else { continue };
        if nom.eq_ignore_ascii_case(ASSET) {
            if !url.starts_with(&prefixe_archive) {
                return Err(format!("l'archive de la release pointe vers un hôte inattendu : {url}"));
            }
            archive = Some(Archive {
                url: url.to_string(),
                taille: asset.get("size").and_then(Value::as_u64),
                somme_sha256: None,
            });
        } else if nom.eq_ignore_ascii_case(&format!("{ASSET}.sha256")) || nom.eq_ignore_ascii_case("SHA256SUMS") {
            if url.starts_with(&prefixe_archive) {
                somme = Some(url.to_string());
            }
        }
    }
    let mut archive = archive.ok_or_else(|| format!("la release ne contient pas {ASSET}"))?;
    archive.somme_sha256 = somme;
    Ok(SortieGithub { tag: tag.to_string(), page, archive })
}

// ---------------------------------------------------------------------------
// Le réseau
// ---------------------------------------------------------------------------

/// Une session WinHTTP réglée comme les autres appels du dépôt.
fn session(usage: &str) -> Result<crate::win32::surveillance::Handle, String> {
    let agent = HSTRING::from(format!("NeuraDesk/{} ({usage})", env!("CARGO_PKG_VERSION")));
    unsafe {
        let session = ouvrir(
            WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0),
            "session",
        )?;
        let _ = WinHttpSetTimeouts(session.0, DELAI_RESEAU_MS, DELAI_RESEAU_MS, DELAI_RESEAU_MS, DELAI_RESEAU_MS);
        Ok(session)
    }
}

/// **UNE REQUÊTE OUVERTE, ET LA CONNEXION QUI LA PORTE.**
///
/// ⛔ **LA CONNEXION DOIT VIVRE AUSSI LONGTEMPS QUE LA REQUÊTE** : les handles
/// WinHTTP sont hiérarchiques (session → connexion → requête), et fermer le
/// parent ferme l'enfant. Les rendre séparément laissait la connexion mourir en
/// fin de fonction, c'est-à-dire avant la lecture du corps. `demande` est déclaré
/// EN PREMIER : les champs tombent dans l'ordre de déclaration, donc la requête
/// est fermée avant sa connexion.
struct Requete {
    demande: crate::win32::surveillance::Handle,
    #[allow(dead_code)]
    connexion: crate::win32::surveillance::Handle,
    statut: u32,
    emplacement: Option<String>,
}

/// **UNE REQUÊTE OUVERTE**, sans lecture du corps : les deux appelants n'en
/// veulent pas la même chose (JSON en mémoire, archive en flux).
fn envoyer(session: &crate::win32::surveillance::Handle, url: &str, accept: &str) -> Result<Requete, String> {
    let url = decouper_url(url).ok_or("URL illisible")?;
    unsafe {
        let connexion = ouvrir(
            WinHttpConnect(session.0, &HSTRING::from(url.hote.as_str()), url.port, 0),
            "connexion",
        )?;
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

        let mut tampon = [0u16; 4096];
        let mut taille = (tampon.len() * 2) as u32;
        let emplacement = if WinHttpQueryHeaders(
            demande.0,
            WINHTTP_QUERY_LOCATION,
            PCWSTR::null(),
            Some(tampon.as_mut_ptr().cast()),
            &mut taille,
            std::ptr::null_mut(),
        )
        .is_ok()
        {
            let longueur = (taille as usize / 2).min(tampon.len());
            let texte = String::from_utf16_lossy(&tampon[..longueur]);
            let texte = texte.trim_end_matches('\0').to_string();
            (!texte.is_empty()).then_some(texte)
        } else {
            None
        };
        Ok(Requete { demande, connexion, statut, emplacement })
    }
}

/// Le corps d'une réponse, en entier (JSON ou fichier de somme : petits).
fn lire_corps(demande: &crate::win32::surveillance::Handle) -> Result<Vec<u8>, String> {
    let mut corps = Vec::new();
    unsafe {
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
    }
    Ok(corps)
}

/// **UN `GET` QUI SUIT LES REDIRECTIONS.** GitHub répond 302 vers
/// `objects.githubusercontent.com` pour les assets ; `meteo::get_json` ne suit
/// pas les redirections (aucun de ses appels n'en produit), d'où ce chemin.
fn get_octets(url: &str, accept: &str) -> Result<Vec<u8>, String> {
    let session = session("maj")?;
    let mut adresse = url.to_string();
    for _ in 0..REDIRECTIONS_MAX {
        let requete = envoyer(&session, &adresse, accept)?;
        if (300..400).contains(&requete.statut) {
            let Some(suivante) = requete.emplacement.clone() else {
                return Err(format!("redirection sans destination ({})", requete.statut));
            };
            adresse = if suivante.starts_with("http") {
                suivante
            } else {
                let base = decouper_url(&adresse).ok_or("URL illisible")?;
                format!("{}://{}{suivante}", if base.securisee { "https" } else { "http" }, base.hote)
            };
            continue;
        }
        if requete.statut != 200 {
            return Err(format!("le serveur a répondu {}", requete.statut));
        }
        return lire_corps(&requete.demande);
    }
    Err("trop de redirections".to_string())
}

fn get_json(url: &str) -> Result<Value, String> {
    let octets = get_octets(url, "application/vnd.github+json")?;
    serde_json::from_slice(&octets).map_err(|e| format!("réponse illisible : {e}"))
}

/// La longueur annoncée, si le serveur la donne (le corps d'une redirection n'en a pas).
fn longueur_annoncee(demande: &crate::win32::surveillance::Handle) -> Option<u64> {
    let mut valeur = 0u32;
    let mut taille = std::mem::size_of::<u32>() as u32;
    unsafe {
        WinHttpQueryHeaders(
            demande.0,
            WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut valeur as *mut u32).cast()),
            &mut taille,
            std::ptr::null_mut(),
        )
        .ok()?;
    }
    Some(valeur as u64)
}

/// **L'ARCHIVE, EN FLUX.** Elle ne passe JAMAIS par la mémoire du processus
/// (277 Mo) : le corps est écrit par blocs, l'empreinte est calculée au fil de
/// l'eau, et la fonction rend `(octets écrits, sha256)`.
fn telecharger(url: &str, fichier: &Path, version: &str) -> Result<(u64, String), String> {
    let mut hash = Hash256::ouvrir()?;
    let mut sortie = std::io::BufWriter::new(
        std::fs::File::create(fichier).map_err(|e| format!("création de l'archive : {e}"))?,
    );
    let session = session("maj")?;
    let mut adresse = url.to_string();
    let mut recu = 0u64;
    let mut publie = 0u64;

    for _ in 0..REDIRECTIONS_MAX {
        let requete = envoyer(&session, &adresse, "application/octet-stream")?;
        if (300..400).contains(&requete.statut) {
            let Some(suivante) = requete.emplacement.clone() else {
                return Err(format!("redirection sans destination ({})", requete.statut));
            };
            adresse = if suivante.starts_with("http") {
                suivante
            } else {
                let base = decouper_url(&adresse).ok_or("URL illisible")?;
                format!("{}://{}{suivante}", if base.securisee { "https" } else { "http" }, base.hote)
            };
            continue;
        }
        if requete.statut != 200 {
            return Err(format!("le serveur a répondu {}", requete.statut));
        }
        let demande = &requete.demande;
        let total = longueur_annoncee(demande);
        poser_etat(Etat::Telechargement { version: version.to_string(), recu: 0, total });
        let mut tampon = vec![0u8; TAMPON_OCTETS];
        unsafe {
            loop {
                if ANNULER.load(Ordering::Relaxed) {
                    return Err("téléchargement annulé".to_string());
                }
                let mut disponible = 0u32;
                WinHttpQueryDataAvailable(demande.0, &mut disponible).map_err(|e| format!("lecture : {e}"))?;
                if disponible == 0 {
                    break;
                }
                let mut lus = 0u32;
                // ⛔ **JAMAIS PLUS QUE LA TAILLE DU TAMPON** : `disponible` est ce
                // que WinHTTP a en réserve, pas ce que le tampon peut recevoir —
                // lui passer la valeur brute écrirait au-delà du `Vec`.
                let a_lire = disponible.min(TAMPON_OCTETS as u32);
                WinHttpReadData(demande.0, tampon.as_mut_ptr().cast(), a_lire, &mut lus)
                    .map_err(|e| format!("lecture : {e}"))?;
                if lus == 0 {
                    break;
                }
                let bloc = &tampon[..lus as usize];
                sortie.write_all(bloc).map_err(|e| format!("écriture de l'archive : {e}"))?;
                hash.ajouter(bloc)?;
                recu += lus as u64;
                if recu - publie >= PAS_DE_PROGRESSION {
                    publie = recu;
                    poser_etat(Etat::Telechargement { version: version.to_string(), recu, total });
                }
            }
        }
        sortie.flush().map_err(|e| format!("écriture de l'archive : {e}"))?;
        poser_etat(Etat::Telechargement { version: version.to_string(), recu, total });
        return Ok((recu, hash.terminer()?));
    }
    Err("trop de redirections".to_string())
}

// ---------------------------------------------------------------------------
// SHA-256 (BCrypt — livré avec Windows, comme le reste)
// ---------------------------------------------------------------------------

/// Un SHA-256 incrémental, pour ne pas relire 277 Mo après le téléchargement.
struct Hash256 {
    algorithme: windows::Win32::Security::Cryptography::BCRYPT_ALG_HANDLE,
    hash: windows::Win32::Security::Cryptography::BCRYPT_HASH_HANDLE,
}

impl Hash256 {
    fn ouvrir() -> Result<Self, String> {
        use windows::Win32::Security::Cryptography::{
            BCryptCreateHash, BCryptOpenAlgorithmProvider, BCRYPT_ALG_HANDLE, BCRYPT_HASH_HANDLE,
            BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS, BCRYPT_SHA256_ALGORITHM,
        };
        unsafe {
            let mut algorithme = BCRYPT_ALG_HANDLE(std::ptr::null_mut());
            if BCryptOpenAlgorithmProvider(
                &mut algorithme,
                BCRYPT_SHA256_ALGORITHM,
                PCWSTR::null(),
                BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
            )
            .0
                < 0
            {
                return Err("SHA-256 indisponible".to_string());
            }
            let mut hash = BCRYPT_HASH_HANDLE(std::ptr::null_mut());
            if BCryptCreateHash(algorithme, &mut hash, None, None, 0).0 < 0 {
                let _ = windows::Win32::Security::Cryptography::BCryptCloseAlgorithmProvider(algorithme, 0);
                return Err("SHA-256 indisponible".to_string());
            }
            Ok(Self { algorithme, hash })
        }
    }

    fn ajouter(&mut self, octets: &[u8]) -> Result<(), String> {
        use windows::Win32::Security::Cryptography::BCryptHashData;
        if unsafe { BCryptHashData(self.hash, octets, 0) }.0 < 0 {
            return Err("calcul de l'empreinte impossible".to_string());
        }
        Ok(())
    }

    fn terminer(&self) -> Result<String, String> {
        use windows::Win32::Security::Cryptography::BCryptFinishHash;
        let mut somme = [0u8; 32];
        if unsafe { BCryptFinishHash(self.hash, &mut somme, 0) }.0 < 0 {
            return Err("calcul de l'empreinte impossible".to_string());
        }
        Ok(somme.iter().map(|o| format!("{o:02x}")).collect())
    }
}

impl Drop for Hash256 {
    fn drop(&mut self) {
        use windows::Win32::Security::Cryptography::{BCryptCloseAlgorithmProvider, BCryptDestroyHash};
        unsafe {
            if !self.hash.0.is_null() {
                let _ = BCryptDestroyHash(self.hash);
            }
            if !self.algorithme.0.is_null() {
                let _ = BCryptCloseAlgorithmProvider(self.algorithme, 0);
            }
        }
    }
}

/// Le premier jeton hexadécimal de 64 caractères d'un fichier `.sha256`.
fn attendue_depuis_texte(texte: &str) -> Option<String> {
    for jeton in texte.split_whitespace() {
        let jeton = jeton.trim_start_matches('*');
        if jeton.len() == 64 && jeton.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(jeton.to_ascii_lowercase());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Les dossiers
// ---------------------------------------------------------------------------

/// Le dossier de l'application — `NEURADESK_MAJ_DOSSIER` le remplace (essais).
fn dossier_de_l_application() -> Option<PathBuf> {
    if let Some(force) = std::env::var_os("NEURADESK_MAJ_DOSSIER") {
        return Some(PathBuf::from(force));
    }
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

/// **UNE VERSION CONSTRUITE DEPUIS LES SOURCES NE SE REMPLACE PAS.** Le dossier
/// d'un build de développement (`target/<profil>`, à côté de son `Cargo.toml`)
/// n'est pas un portable : l'écraser avec l'archive publiée détruirait le build
/// de la personne qui développe. `NEURADESK_MAJ_DOSSIER` lève le garde, parce
/// que c'est le levier d'essai de l'assistant.
fn dossier_de_developpement(dossier: &Path) -> bool {
    if std::env::var_os("NEURADESK_MAJ_DOSSIER").is_some() {
        return false;
    }
    dossier.join("Cargo.toml").is_file()
        || dossier.components().any(|c| c.as_os_str().eq_ignore_ascii_case("target"))
}

/// `%LOCALAPPDATA%\NeuraDesk\maj` : ce qui se régénère va dans les caches.
fn dossier_des_travaux() -> Option<PathBuf> {
    let mut chemin = crate::config::cache_dir()?;
    chemin.push("maj");
    Some(chemin)
}

/// **LE JOURNAL DE L'ASSISTANT** — `diag!` n'écrit que si `NEURADESK_DIAG` est
/// posée et que `diag::init` a tourné ; l'assistant démarre avant tout cela, et
/// c'est justement son échec qu'il faut pouvoir lire après coup. Ce journal-là
/// est TOUJOURS écrit, petit, et ne tourne pas sur le fil de rendu.
fn journaliser(message: &str) {
    let chemin = dossier_des_travaux()
        .unwrap_or_else(std::env::temp_dir)
        .join("maj.log");
    if let Some(parent) = chemin.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut fichier) = std::fs::OpenOptions::new().create(true).append(true).open(&chemin) {
        let _ = writeln!(fichier, "[{}] {message}", maintenant_unix());
    }
}

/// **UN CHEMIN D'ENTRÉE DE ZIP, ASSAINI.** Refuse tout ce qui sortirait du
/// dossier d'extraction : chemin absolu, `..`, lettre de lecteur, entrée vide.
fn chemin_relatif(nom: &str) -> Option<PathBuf> {
    if nom.contains('\0') || nom.starts_with('/') || nom.starts_with('\\') {
        return None;
    }
    let mut chemin = PathBuf::new();
    for morceau in nom.split(['/', '\\']) {
        match morceau {
            "" | "." => continue,
            ".." => return None,
            morceau if morceau.contains(':') => return None,
            morceau => chemin.push(morceau),
        }
    }
    (!chemin.as_os_str().is_empty()).then_some(chemin)
}

/// Le dossier qui porte `neuradesk.exe`, après extraction. Le zip portable a un
/// dossier racine (`NeuraDesk-portable/`), mais on ne le suppose pas.
fn racine_de_l_application(extraite: &Path) -> Option<PathBuf> {
    if extraite.join(EXECUTABLE).is_file() {
        return Some(extraite.to_path_buf());
    }
    let mut dossiers = std::fs::read_dir(extraite)
        .ok()?
        .filter_map(|entree| entree.ok())
        .filter(|entree| entree.file_type().map(|t| t.is_dir()).unwrap_or(false));
    let premier = dossiers.next()?.path();
    if dossiers.next().is_some() {
        return None;
    }
    premier.join(EXECUTABLE).is_file().then_some(premier)
}

/// **LE ZIP EST DÉPLIÉ ICI, ET NULLE PART AILLEURS.** Rend le nombre d'entrées.
fn extraire(archive: &Path, cible: &Path, version: &str) -> Result<u64, String> {
    let fichier = std::fs::File::open(archive).map_err(|e| format!("archive illisible : {e}"))?;
    let mut zip = zip::ZipArchive::new(fichier).map_err(|e| format!("archive .zip illisible : {e}"))?;
    let total = zip.len() as u64;
    std::fs::create_dir_all(cible).map_err(|e| format!("dossier de préparation : {e}"))?;
    let mut decompresse = 0u64;
    let mut faits = 0u64;
    for i in 0..zip.len() {
        let mut entree = zip.by_index(i).map_err(|e| format!("archive abîmée : {e}"))?;
        faits += 1;
        if entree.is_dir() {
            continue;
        }
        // Un lien symbolique d'archive pourrait écrire ailleurs : on refuse.
        if entree.is_symlink() {
            return Err("l'archive contient un lien symbolique".to_string());
        }
        let nom = entree.name().map_err(|e| format!("archive abîmée : {e}"))?.into_owned();
        let Some(relatif) = chemin_relatif(&nom) else {
            return Err(format!("chemin refusé dans l'archive : {nom}"));
        };
        decompresse += entree.size();
        if decompresse > TAILLE_MAX_DECOMPRESSEE {
            return Err("l'archive décompressée dépasse la taille admise".to_string());
        }
        let destination = cible.join(&relatif);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("dossier de préparation : {e}"))?;
        }
        let mut sortie = std::fs::File::create(&destination)
            .map_err(|e| format!("écriture de {} : {e}", relatif.display()))?;
        std::io::copy(&mut entree, &mut sortie)
            .map_err(|e| format!("écriture de {} : {e}", relatif.display()))?;
        if faits % 25 == 0 || faits == total {
            poser_etat(Etat::Extraction { version: version.to_string(), faits, total });
        }
    }
    poser_etat(Etat::Extraction { version: version.to_string(), faits: total, total });
    if racine_de_l_application(cible).is_none() {
        return Err(format!("l'archive ne contient pas {EXECUTABLE} à sa racine"));
    }
    Ok(faits)
}

/// **LE DOSSIER DE PRÉPARATION EST UN FRÈRE DU DOSSIER DE L'APPLICATION** : le
/// renommage final doit rester sur le MÊME volume, sinon `fs::rename` échoue
/// (franchir un volume est une copie, pas un renommage).
fn dossier_de_preparation(dossier: &Path) -> Result<PathBuf, String> {
    let nom = dossier.file_name().and_then(OsStr::to_str).ok_or("le dossier de l'application n'a pas de nom")?;
    let parent = dossier.parent().ok_or("le dossier de l'application n'a pas de parent")?;
    let racine = format!(".{nom}.maj-{}", maintenant_unix());
    let chemin = parent.join(racine);
    // Une préparation précédente du même nom (même seconde) est un résidu.
    let _ = std::fs::remove_dir_all(&chemin);
    Ok(chemin)
}

// ---------------------------------------------------------------------------
// L'API publique : vérifier, installer, redémarrer
// ---------------------------------------------------------------------------

/// `true` quand le module doit se taire (harnais de capture, essai explicite).
fn desactive() -> bool {
    std::env::var_os("NEURADESK_NO_MAJ").is_some()
        || std::env::var_os("NEURADESK_CAPTURE").is_some()
        || std::env::var_os("NEURADESK_CAPTURE_FRAMES").is_some()
}

/// **INTERROGE GITHUB.** `force` ignore l'anti-rebond de cinq minutes (bouton et
/// entrée de menu) ; la vérification automatique, elle, l'applique.
pub fn verifier(force: bool) {
    if desactive() {
        return;
    }
    // Une vérification pendant un téléchargement écraserait l'état de la
    // progression : elle n'apporterait rien et brouillerait la page.
    if TELECHARGEMENT_EN_COURS.load(Ordering::SeqCst) {
        return;
    }
    if !force {
        let recente = DERNIERE_VERIFICATION
            .lock()
            .ok()
            .and_then(|d| *d)
            .is_some_and(|t| t.elapsed() < ENTRE_DEUX_VERIFICATIONS);
        if recente {
            return;
        }
    }
    if VERIFICATION_EN_COURS.swap(true, Ordering::SeqCst) {
        return;
    }
    poser_etat(Etat::Verification);
    let _ = std::thread::Builder::new().name("maj-verification".to_string()).spawn(|| {
        let resultat = (|| -> Result<Option<SortieGithub>, String> {
            let valeur = get_json(API_DERNIERE)?;
            let sortie = lire_reponse(&valeur)?;
            if est_plus_recente(&sortie.tag, ACTUELLE) {
                Ok(Some(sortie))
            } else {
                Ok(None)
            }
        })();
        VERIFICATION_EN_COURS.store(false, Ordering::SeqCst);
        if let Ok(mut derniere) = DERNIERE_VERIFICATION.lock() {
            *derniere = Some(Instant::now());
        }
        match resultat {
            Ok(Some(sortie)) => {
                let version = sortie.tag.clone();
                poser_etat(Etat::Disponible {
                    version,
                    page: sortie.page.clone(),
                    archive: sortie.archive,
                });
                // ⚠️ LE TOAST EST DÉPOSÉ DEPUIS LE FIL DE L'INTERFACE (voir `avancer`) :
                // WinRT demande un appartement COM initialisé, que seul le fil
                // principal a. Le fil réseau ne fait que déposer la demande.
                let (_, notifiee) = lire_persiste();
                if notifiee.as_deref() != Some(sortie.tag.as_str()) {
                    let texte = format!(
                        "La version {} est disponible. Ouvrez « À propos de NeuraDesk » (clic droit sur l'icône de l'application) pour l'installer.",
                        sortie.tag
                    );
                    if let Ok(mut notification) = NOTIFICATION.lock() {
                        *notification = Some((sortie.tag, texte));
                    }
                }
                ecrire_persiste(maintenant_unix(), notifiee.as_deref());
                crate::diag!("[maj] version {} disponible (locale : {ACTUELLE})", sortie.page);
            }
            Ok(None) => {
                let (_, notifiee) = lire_persiste();
                poser_etat(Etat::AJour { version: ACTUELLE.to_string() });
                ecrire_persiste(maintenant_unix(), notifiee.as_deref());
                crate::diag!("[maj] à jour ({ACTUELLE})");
            }
            Err(message) => {
                poser_etat(Etat::Erreur { message: message.clone() });
                crate::diag!("[maj] vérification impossible : {message}");
            }
        }
    });
}

/// **TÉLÉCHARGE ET PRÉPARE.** Ne fait rien si ce n'est pas l'état `Disponible`.
pub fn installer() {
    if desactive() {
        return;
    }
    let Etat::Disponible { version, archive, .. } = etat_courant() else { return };
    if TELECHARGEMENT_EN_COURS.swap(true, Ordering::SeqCst) {
        return;
    }
    ANNULER.store(false, Ordering::SeqCst);
    let Some(dossier) = dossier_de_l_application() else {
        poser_etat(Etat::Erreur { message: "dossier de l'application introuvable".to_string() });
        TELECHARGEMENT_EN_COURS.store(false, Ordering::SeqCst);
        return;
    };
    if dossier_de_developpement(&dossier) {
        poser_etat(Etat::Erreur {
            message: "version de développement : la mise à jour automatique est réservée au portable".to_string(),
        });
        TELECHARGEMENT_EN_COURS.store(false, Ordering::SeqCst);
        return;
    }
    let Some(travaux) = dossier_des_travaux() else {
        poser_etat(Etat::Erreur { message: "dossier temporaire introuvable".to_string() });
        TELECHARGEMENT_EN_COURS.store(false, Ordering::SeqCst);
        return;
    };
    let _ = std::thread::Builder::new().name("maj-telechargement".to_string()).spawn(move || {
        let resultat = (|| -> Result<Preparation, String> {
            std::fs::create_dir_all(&travaux).map_err(|e| format!("dossier temporaire : {e}"))?;
            let archive_locale = travaux.join(ASSET);
            let partiel = travaux.join(format!("{ASSET}.partiel"));
            let _ = std::fs::remove_file(&partiel);
            let (_, somme) = telecharger(&archive.url, &partiel, &version)?;
            if let Some(url) = &archive.somme_sha256 {
                let texte = String::from_utf8_lossy(&get_octets(url, "text/plain")?).into_owned();
                let attendue = attendue_depuis_texte(&texte).ok_or("le fichier de somme est illisible")?;
                if attendue != somme {
                    let _ = std::fs::remove_file(&partiel);
                    return Err("l'archive ne correspond pas à l'empreinte publiée".to_string());
                }
                crate::diag!("[maj] empreinte SHA-256 vérifiée : {somme}");
            } else {
                crate::diag!("[maj] aucune empreinte publiée pour {ASSET} — archive acceptée telle quelle");
            }
            std::fs::rename(&partiel, &archive_locale).map_err(|e| format!("archive : {e}"))?;

            let temporaire = dossier_de_preparation(&dossier)?;
            let _ = std::fs::remove_dir_all(&temporaire);
            extraire(&archive_locale, &temporaire, &version)?;
            let racine = racine_de_l_application(&temporaire)
                .ok_or_else(|| format!("l'archive ne contient pas {EXECUTABLE}"))?;
            if let Some(taille) = archive.taille {
                let obtenue = std::fs::metadata(&archive_locale).map(|m| m.len()).unwrap_or(0);
                if obtenue != taille {
                    return Err(format!("archive incomplète : {obtenue} octets au lieu de {taille}"));
                }
            }
            let _ = std::fs::remove_file(&archive_locale);
            Ok(Preparation { version: version.clone(), racine, dossier: dossier.clone(), temporaire })
        })();
        TELECHARGEMENT_EN_COURS.store(false, Ordering::SeqCst);
        match resultat {
            Ok(preparation) => {
                if let Ok(mut pret) = PRET.lock() {
                    *pret = Some(preparation.clone());
                }
                poser_etat(Etat::Pret { version: preparation.version.clone() });
                crate::diag!("[maj] {} prête, dossier {:?}", preparation.version, preparation.racine);
            }
            Err(message) => {
                // Rien de partiel ne doit rester : la prochaine tentative
                // repart d'une archive entière (le dossier de préparation, lui,
                // est nettoyé au démarrage suivant).
                let _ = std::fs::remove_file(travaux.join(format!("{ASSET}.partiel")));
                let _ = std::fs::remove_file(travaux.join(ASSET));
                if message.contains("annulé") {
                    poser_etat(Etat::Disponible { version, page: String::new(), archive });
                } else {
                    poser_etat(Etat::Erreur { message: message.clone() });
                    crate::diag!("[maj] installation impossible : {message}");
                }
            }
        }
    });
}

/// **ANNULE LE TÉLÉCHARGEMENT EN COURS.** Sans effet sur une préparation finie.
pub fn annuler() {
    ANNULER.store(true, Ordering::SeqCst);
}

/// **L'UTILISATEUR DEMANDE LE REDÉMARRAGE** — `main` fera le reste.
pub fn redemarrer_pour_installer() {
    if matches!(etat_courant(), Etat::Pret { .. }) {
        APPLIQUER.store(true, Ordering::SeqCst);
        crate::app::request_reload();
    }
}

/// **APPELÉ PAR `main` APRÈS `cef_shutdown`, SUR LE CHEMIN DU RECHARGEMENT.**
/// Rend `true` quand l'assistant a été lancé — `main` ne relance alors PAS
/// l'application lui-même (deux instances CEF se tuent mutuellement).
pub fn lancer_l_installation_si_demandee() -> bool {
    if !APPLIQUER.swap(false, Ordering::SeqCst) {
        return false;
    }
    let Some(preparation) = PRET.lock().ok().and_then(|p| p.clone()) else { return false };
    match demarrer_l_assistant(&preparation) {
        Ok(()) => {
            crate::diag!("[maj] assistant lancé pour {}", preparation.version);
            true
        }
        Err(erreur) => {
            crate::diag!("[maj] assistant impossible : {erreur}");
            false
        }
    }
}

/// Copie l'exécutable courant dans les travaux et lance l'assistant.
fn demarrer_l_assistant(preparation: &Preparation) -> Result<(), String> {
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    use std::os::windows::process::CommandExt;

    let exe = std::env::current_exe().map_err(|e| format!("exécutable courant : {e}"))?;
    let travaux = dossier_des_travaux().ok_or("dossier temporaire introuvable")?;
    std::fs::create_dir_all(&travaux).map_err(|e| format!("dossier temporaire : {e}"))?;
    let assistant = travaux.join(format!("neuradesk-maj-{}.exe", std::process::id()));
    std::fs::copy(&exe, &assistant).map_err(|e| format!("copie de l'assistant : {e}"))?;

    Command::new(&assistant)
        .arg("--maj-appliquer")
        .arg(std::process::id().to_string())
        .arg(&preparation.dossier)
        .arg(&preparation.racine)
        .arg(&preparation.temporaire)
        .creation_flags(CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("lancement de l'assistant : {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// L'ASSISTANT — la partie qui tourne APRÈS la fermeture de l'application
// ---------------------------------------------------------------------------

/// **LE POINT D'ENTRÉE DE L'ASSISTANT**, appelé par `main` AVANT tout CEF.
/// `--maj-appliquer <pid> <dossier> <racine> <temporaire>`.
pub fn executer_si_demande() {
    let args: Vec<OsString> = std::env::args_os().collect();
    if args.get(1) != Some(&OsString::from("--maj-appliquer")) {
        return;
    }
    if args.len() < 6 {
        erreur_visible("Les arguments de la mise à jour sont incomplets.");
        std::process::exit(1);
    }
    let pid = args[2].to_string_lossy().parse::<u32>().unwrap_or(0);
    let dossier = PathBuf::from(&args[3]);
    let racine = PathBuf::from(&args[4]);
    let temporaire = PathBuf::from(&args[5]);
    // Le dossier courant ne doit PAS être le dossier à renommer : Windows
    // verrouille le répertoire de travail d'un processus. À défaut des travaux
    // (l'assistant peut être lancé à la main), le TEMPORAIRE du système — et
    // surtout PAS `temporaire`, que le nettoyage final supprime.
    let ailleurs = dossier_des_travaux().unwrap_or_else(std::env::temp_dir);
    let _ = std::env::set_current_dir(&ailleurs);
    if std::env::current_dir().ok().as_deref() == Some(dossier.as_path()) {
        if let Some(parent) = dossier.parent() {
            let _ = std::env::set_current_dir(parent);
        }
    }

    match appliquer(pid, &dossier, &racine, &temporaire) {
        Ok(()) => std::process::exit(0),
        Err(erreur) => {
            journaliser(&format!("installation échouée : {erreur}"));
            // ⚠️ **LA RELANCE EST GARANTIE PAR `appliquer` LUI-MÊME** (voir
            // `RelanceGarantie`) : relancer aussi ici créerait une SECONDE
            // instance, et deux instances CEF se tuent mutuellement.
            erreur_visible(&format!("La mise à jour n'a pas pu être installée.\n\n{erreur}"));
            std::process::exit(1);
        }
    }
}

/// **L'APPLICATION EST RELANCÉE MÊME QUAND L'INSTALLATION ÉCHOUE.**
///
/// L'ancienne version a été FERMÉE avant que l'assistant ne commence : chaque
/// chemin d'erreur doit donc rendre une application à l'utilisateur. Ce garde
/// s'arme dès que le processus parent est mort, et son `Drop` relance l'ancienne
/// version si aucun lancement n'a déjà eu lieu. Il est DÉSARMÉ dès qu'un
/// processus (nouveau ou ancien) a été lancé avec succès — relancer deux fois
/// créerait deux instances, qui se tuent mutuellement (deux instances CEF).
struct RelanceGarantie<'a> {
    dossier: &'a Path,
    arme: std::cell::Cell<bool>,
}

impl<'a> RelanceGarantie<'a> {
    fn new(dossier: &'a Path) -> Self {
        Self { dossier, arme: std::cell::Cell::new(false) }
    }
    fn armer(&self) {
        self.arme.set(true);
    }
    fn desarmer(&self) {
        self.arme.set(false);
    }
}

impl Drop for RelanceGarantie<'_> {
    fn drop(&mut self) {
        if self.arme.get() && self.dossier.join(EXECUTABLE).is_file() {
            journaliser("échec : relance de l'ancienne version");
            let _ = relancer(self.dossier);
        }
    }
}

/// Le cœur de l'assistant. Tout est réversible jusqu'au renommage du dossier.
fn appliquer(pid: u32, dossier: &Path, racine: &Path, temporaire: &Path) -> Result<(), String> {
    appliquer_avec(pid, dossier, racine, temporaire, ATTENTE_VERROU)
}

/// **COMMENT LE NOUVEAU DOSSIER A ÉTÉ POSÉ — donc aussi comment on revient.**
enum Placement {
    /// Le renommage a marché : l'ancien dossier est intact sous `ancien`, et le
    /// retour en arrière remet tout en place.
    ParRenommage,
    /// Le support a REFUSÉ de renommer le dossier : les fichiers ont été écrasés
    /// sur place, et le seul retour possible est l'exécutable d'avant, gardé
    /// sous `secours`.
    SurPlace { secours: PathBuf },
}

/// Le même, avec le budget de renommage en paramètre : un test ne peut pas
/// attendre les soixante secondes du terrain (voir `ATTENTE_VERROU`).
fn appliquer_avec(
    pid: u32,
    dossier: &Path,
    racine: &Path,
    temporaire: &Path,
    attente_renommage: Duration,
) -> Result<(), String> {
    let journal = |texte: &str| journaliser(texte);

    if !racine.join(EXECUTABLE).is_file() {
        return Err(format!("la préparation ne contient pas {EXECUTABLE}"));
    }
    // ⛔ **PAS DE RELANCE SI LE PARENT EST ENCORE VIVANT** : le garde n'est
    // armé qu'après sa mort. Une seconde instance pendant que la première vit
    // tuerait les deux.
    if !attendre_le_parent(pid) {
        return Err("l'application ne s'est pas arrêtée : mise à jour annulée".to_string());
    }
    let garantie = RelanceGarantie::new(dossier);
    garantie.armer();
    // Les sous-processus CEF meurent avec le Job Object du parent ; on leur
    // laisse un instant pour rendre leurs handles avant le renommage.
    std::thread::sleep(Duration::from_millis(800));

    let nom = dossier.file_name().and_then(OsStr::to_str).ok_or("dossier de l'application sans nom")?;
    let horodatage = maintenant_unix();
    let ancien = dossier.with_file_name(format!("{nom}.ancien-{horodatage}"));
    let rate = dossier.with_file_name(format!("{nom}.rate-{horodatage}"));

    journal(&format!("renommage de {dossier:?} en {ancien:?}"));
    let placement = match renommer_avec_attente(dossier, &ancien, attente_renommage) {
        Ok(()) => {
            if let Err(erreur) = renommer_avec_attente(racine, dossier, attente_renommage) {
                // Le dossier de l'application n'existe plus : on remet l'ancien en place.
                let _ = std::fs::rename(&ancien, dossier);
                if relancer(dossier).is_ok() {
                    garantie.desarmer();
                }
                return Err(format!("mise en place impossible : {erreur}"));
            }
            journal("nouveau dossier en place, lancement de l'application");
            Placement::ParRenommage
        }
        Err(refus) => {
            // ⭐ **LE SUPPORT REFUSE DE RENOMMER LE DOSSIER DE L'APPLICATION.**
            // Constaté le 2026-11-10 sur `Z:\NeuraDesk-portable` : « Accès
            // refusé. (os error 5) », tenace pendant les soixante secondes de
            // réessai — ce n'est donc pas un handle qui traîne, c'est le support
            // qui dit non (montage cloud, dossier en cours d'utilisation par un
            // autre programme, droits du volume). Renommer n'est qu'un MOYEN :
            // ce qui compte est que la nouvelle version soit à la place de
            // l'ancienne, alors on recopie les FICHIERS par-dessus.
            journal(&format!("{refus} — remplacement des fichiers sur place"));
            let secours = dossier.with_file_name(format!("{nom}.exe-prec-{horodatage}"));
            let secours = remplacer_les_fichiers(dossier, racine, &secours)?;
            journal("fichiers remplacés sur place, lancement de l'application");
            Placement::SurPlace { secours }
        }
    };

    let mut enfant = match relancer(dossier) {
        Ok(enfant) => {
            garantie.desarmer();
            enfant
        }
        Err(erreur) => {
            revenir(dossier, &placement, &ancien, &rate, &garantie)?;
            return Err(format!("la nouvelle version ne démarre pas : {erreur}"));
        }
    };

    // ⭐ **LE CONTRÔLE QUI ÉVITE DE LAISSER L'UTILISATEUR SANS APPLICATION** :
    // une version qui meurt dans les dix secondes est réputée plantée, et on
    // remet l'ancienne en place.
    std::thread::sleep(ESSAI_DE_VIE);
    if let Ok(Some(_)) = enfant.try_wait() {
        revenir(dossier, &placement, &ancien, &rate, &garantie)?;
        return Err("la nouvelle version s'est arrêtée au démarrage".to_string());
    }
    journal("la nouvelle version est vivante, nettoyage");
    match &placement {
        Placement::ParRenommage => {
            let _ = std::fs::remove_dir_all(&ancien);
        }
        Placement::SurPlace { secours } => {
            let _ = std::fs::remove_file(secours);
        }
    }
    if temporaire != racine {
        let _ = std::fs::remove_dir_all(temporaire);
    }
    Ok(())
}

/// Le retour en arrière de chaque façon de poser le dossier.
fn revenir(
    dossier: &Path,
    placement: &Placement,
    ancien: &Path,
    rate: &Path,
    garantie: &RelanceGarantie,
) -> Result<(), String> {
    match placement {
        Placement::ParRenommage => revenir_en_arriere(dossier, ancien, rate, garantie),
        Placement::SurPlace { secours } => revenir_sur_place(dossier, secours, garantie),
    }
}

/// **RECOPIE L'ARBRE PRÉPARÉ PAR-DESSUS L'ANCIEN**, l'exécutable EN DERNIER.
///
/// C'est le chemin de secours quand le dossier ne peut pas être renommé. Ce
/// qu'il ne peut pas faire, et qu'il faut savoir : un fichier de l'ancienne
/// version ABSENT de la nouvelle reste en place, et un échec au milieu laisse
/// un arbre mélangé (l'ancien exécutable, lui, est restaurable — voir
/// `revenir_sur_place`).
fn remplacer_les_fichiers(dossier: &Path, racine: &Path, secours: &Path) -> Result<PathBuf, String> {
    std::fs::copy(dossier.join(EXECUTABLE), secours)
        .map_err(|e| format!("sauvegarde de l'ancien exécutable impossible : {e}"))?;
    copier_l_arbre(racine, dossier)?;
    Ok(secours.to_path_buf())
}

/// Copie récursive, SANS l'exécutable pendant la promenade : il est écrit à la
/// fin, pour que l'ancien reste lançable tant que le reste n'est pas en place.
fn copier_l_arbre(source: &Path, cible: &Path) -> Result<(), String> {
    let mut a_parcourir = vec![source.to_path_buf()];
    while let Some(repertoire) = a_parcourir.pop() {
        let entrees = std::fs::read_dir(&repertoire).map_err(|e| format!("lecture de {repertoire:?} : {e}"))?;
        for entree in entrees {
            let entree = entree.map_err(|e| format!("lecture de {repertoire:?} : {e}"))?;
            let chemin = entree.path();
            let relatif = chemin.strip_prefix(source).map_err(|_| format!("chemin hors de {source:?}"))?.to_path_buf();
            let vers = cible.join(&relatif);
            if chemin.is_dir() {
                std::fs::create_dir_all(&vers).map_err(|e| format!("création de {vers:?} : {e}"))?;
                a_parcourir.push(chemin);
                continue;
            }
            if relatif == Path::new(EXECUTABLE) {
                continue;
            }
            if let Some(parent) = vers.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("création de {parent:?} : {e}"))?;
            }
            // ⚠️ Un fichier extrait d'une archive peut porter l'attribut
            // « lecture seule », que `copy` refuse d'écraser : on le retire
            // d'abord (sans se plaindre, le cas est rare).
            if let Ok(meta) = std::fs::metadata(&vers) {
                let mut droits = meta.permissions();
                if droits.readonly() {
                    droits.set_readonly(false);
                    let _ = std::fs::set_permissions(&vers, droits);
                }
            }
            std::fs::copy(&chemin, &vers).map_err(|e| format!("copie de {chemin:?} : {e}"))?;
        }
    }
    std::fs::copy(source.join(EXECUTABLE), cible.join(EXECUTABLE))
        .map_err(|e| format!("copie de l'exécutable : {e}"))?;
    Ok(())
}

/// Remet l'exécutable d'avant et relance : c'est tout ce que le remplacement
/// sur place permet de défaire.
fn revenir_sur_place(dossier: &Path, secours: &Path, garantie: &RelanceGarantie) -> Result<(), String> {
    journaliser("retour en arrière (sur place)");
    garantie.armer();
    std::fs::copy(secours, dossier.join(EXECUTABLE))
        .map_err(|e| format!("remise de l'ancien exécutable impossible : {e}"))?;
    // La copie a réussi : la sauvegarde ne sert plus à rien, même si la
    // relance échoue (l'ancien exécutable est déjà en place).
    let _ = std::fs::remove_file(secours);
    relancer(dossier).map_err(|e| format!("l'ancienne version n'a pas pu être relancée : {e}"))?;
    garantie.desarmer();
    Ok(())
}

/// Renvoie les deux dossiers dans leur état d'avant et relance l'ancienne version.
fn revenir_en_arriere(
    dossier: &Path,
    ancien: &Path,
    rate: &Path,
    garantie: &RelanceGarantie,
) -> Result<(), String> {
    journaliser("retour en arrière");
    garantie.armer();
    let _ = std::fs::rename(dossier, rate);
    std::fs::rename(ancien, dossier).map_err(|e| format!("retour en arrière impossible : {e}"))?;
    relancer(dossier).map_err(|e| format!("l'ancienne version n'a pas pu être relancée : {e}"))?;
    garantie.desarmer();
    let _ = std::fs::remove_dir_all(rate);
    Ok(())
}

fn relancer(dossier: &Path) -> Result<std::process::Child, String> {
    let exe = dossier.join(EXECUTABLE);
    Command::new(&exe)
        .current_dir(dossier)
        .spawn()
        .map_err(|e| format!("lancement de {exe:?} : {e}"))
}

/// **RENOMME EN RÉESSAYANT** — un fichier encore tenu par un sous-processus CEF
/// fait échouer `fs::rename` (`ERROR_ACCESS_DENIED`/`SHARING_VIOLATION`) pendant
/// quelques centaines de millisecondes.
fn renommer_avec_attente(de: &Path, vers: &Path, attente: Duration) -> Result<(), String> {
    let limite = Instant::now() + attente;
    loop {
        match std::fs::rename(de, vers) {
            Ok(()) => return Ok(()),
            Err(erreur) => {
                if Instant::now() >= limite {
                    return Err(format!("renommage de {de:?} impossible : {erreur}"));
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
}

/// **ATTEND LA MORT DU PROCESSUS PARENT** — par son handle, jamais par son PID
/// relu (un PID peut être réutilisé). Rend `true` s'il est mort (ou si `pid`
/// vaut 0, c'est-à-dire « ne pas attendre »).
fn attendre_le_parent(pid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    if pid == 0 {
        return true;
    }
    unsafe {
        match OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            // Le handle introuvable veut dire que le processus n'existe plus.
            Err(_) => true,
            Ok(handle) => {
                let resultat = WaitForSingleObject(handle, ATTENTE_PARENT_MS);
                let _ = CloseHandle(handle);
                resultat == WAIT_OBJECT_0
            }
        }
    }
}

fn erreur_visible(message: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MB_TOPMOST};
    let texte = HSTRING::from(message);
    let titre = HSTRING::from("Mise à jour de NeuraDesk");
    unsafe {
        MessageBoxW(None, &texte, &titre, MB_OK | MB_ICONERROR | MB_TOPMOST | MB_SETFOREGROUND);
    }
}

// ---------------------------------------------------------------------------
// La boucle de l'application
// ---------------------------------------------------------------------------

/// **APPELÉ À CHAQUE FRAME.** Ne fait rien tant que le délai de démarrage n'est
/// pas écoulé ; ensuite, une vérification automatique au plus toutes les six
/// heures, et le dépôt de la notification sur le fil de l'interface.
pub fn avancer() {
    if desactive() {
        return;
    }
    nettoyer_les_residus_une_fois();
    livrer_la_notification();

    let demarre = {
        let mut garde = match DEMARRE.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        *garde.get_or_insert_with(Instant::now)
    };
    if demarre.elapsed() < DELAI_PREMIERE_VERIFICATION {
        return;
    }
    // La prochaine échéance part de la date PERSISTÉE : un lancement qui suit
    // une vérification récente n'en refait pas une, et une session qui dure
    // plus de six heures en refait une.
    let due = {
        let Ok(mut garde) = PROCHAINE_AUTOMATIQUE.lock() else { return };
        match *garde {
            None => {
                let (derniere, _) = lire_persiste();
                let attente = if derniere == 0 {
                    Duration::ZERO
                } else {
                    PERIODE_AUTOMATIQUE.saturating_sub(Duration::from_secs(maintenant_unix().saturating_sub(derniere)))
                };
                *garde = Some(Instant::now() + attente);
                false
            }
            Some(echeance) if Instant::now() >= echeance => {
                *garde = Some(Instant::now() + PERIODE_AUTOMATIQUE);
                true
            }
            Some(_) => false,
        }
    };
    if due && !VERIFICATION_EN_COURS.load(Ordering::SeqCst) && !TELECHARGEMENT_EN_COURS.load(Ordering::SeqCst) {
        verifier(false);
    }
}

/// Le toast est déposé ICI, sur le fil principal : WinRT demande un appartement
/// COM, que seul ce fil a initialisé (voir `main`, `CoInitializeEx`).
fn livrer_la_notification() {
    let demande = NOTIFICATION.lock().ok().and_then(|mut n| n.take());
    let Some((version, texte)) = demande else { return };
    let page = PAGE_RELEASES;
    let pris = crate::win32::bulle::notifier_avec_liens(
        "Mise à jour de NeuraDesk",
        &texte,
        None,
        &[crate::win32::bulle::Lien { libelle: "Voir la version", url: page }],
    );
    crate::diag!("[maj] toast proposé pour {version} (pris : {pris})");
    let (derniere, _) = lire_persiste();
    ecrire_persiste(derniere.max(maintenant_unix()), Some(&version));
}

/// **LES RÉSIDUS D'UNE INSTALLATION INTERROMPUE** — uniquement des dossiers
/// FRÈRES de l'application (`.<nom>.maj-*`, `<nom>.ancien-*`, `<nom>.rate-*`) et
/// d'anciennes copies de l'assistant. Best-effort : un dossier encore tenu est
/// simplement laissé pour la prochaine fois.
fn nettoyer_les_residus_une_fois() {
    if NETTOYAGE_FAIT.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(dossier) = dossier_de_l_application() else { return };
    let (Some(nom), Some(parent)) = (dossier.file_name().and_then(OsStr::to_str), dossier.parent()) else { return };
    let Ok(entrees) = std::fs::read_dir(parent) else { return };
    for entree in entrees.flatten() {
        let Some(nom_entree) = entree.file_name().to_str().map(str::to_string) else { continue };
        let residu = nom_entree.starts_with(&format!(".{nom}.maj-"))
            || nom_entree.starts_with(&format!("{nom}.ancien-"))
            || nom_entree.starts_with(&format!("{nom}.rate-"));
        if residu {
            if let Err(erreur) = std::fs::remove_dir_all(entree.path()) {
                crate::diag!("[maj] résidu non supprimé {:?} : {erreur}", entree.path());
            }
        }
    }
    if let Some(travaux) = dossier_des_travaux() {
        if let Ok(entrees) = std::fs::read_dir(&travaux) {
            for entree in entrees.flatten() {
                let nom_fichier = entree.file_name();
                let nom_fichier = nom_fichier.to_string_lossy();
                if nom_fichier.starts_with("neuradesk-maj-") && nom_fichier.ends_with(".exe") {
                    let _ = std::fs::remove_file(entree.path());
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// La page « À propos »
// ---------------------------------------------------------------------------

/// **L'ÉTAT DE LA MISE À JOUR, POUR `cef_apropos`.** Une forme stable et plate,
/// que `apropos.html` rend sans rien deviner.
pub fn etat_json() -> Value {
    let actuelle = ACTUELLE.to_string();
    match etat_courant() {
        Etat::Inconnu => json!({ "etat": "inconnu", "actuelle": actuelle }),
        Etat::Verification => json!({ "etat": "verification", "actuelle": actuelle }),
        Etat::AJour { version } => json!({ "etat": "a_jour", "actuelle": actuelle, "version": version }),
        Etat::Disponible { version, page, archive } => json!({
            "etat": "disponible",
            "actuelle": actuelle,
            "version": version,
            "page": page,
            "taille": archive.taille,
            "taille_lisible": archive.taille.map(taille_lisible),
        }),
        Etat::Telechargement { version, recu, total } => json!({
            "etat": "telechargement",
            "actuelle": actuelle,
            "version": version,
            "recu": recu,
            "total": total,
        }),
        Etat::Extraction { version, faits, total } => json!({
            "etat": "extraction",
            "actuelle": actuelle,
            "version": version,
            "recu": faits,
            "total": total,
        }),
        Etat::Pret { version } => json!({ "etat": "pret", "actuelle": actuelle, "version": version }),
        Etat::Erreur { message } => json!({ "etat": "erreur", "actuelle": actuelle, "message": message }),
    }
}

// ---------------------------------------------------------------------------
// Tests — hors ligne uniquement
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_tags_portable_se_comparent_par_date_puis_par_suffixe() {
        assert!(est_plus_recente("portable-2026-11-10", "portable-2026-11-09b"));
        assert!(est_plus_recente("portable-2026-11-09b", "portable-2026-11-09"));
        assert!(est_plus_recente("portable-2026-12-01", "portable-2026-11-30"));
        assert!(!est_plus_recente("portable-2026-11-09", "portable-2026-11-09"));
        assert!(!est_plus_recente("portable-2026-11-09b", "portable-2026-11-10"));
        // Deux suffixes du même jour : `b` passe APRÈS `a`, mais un suffixe
        // vide est le premier.
        assert!(est_plus_recente("portable-2026-11-09a", "portable-2026-11-09"));
    }

    #[test]
    fn une_autre_lignee_ne_declenche_jamais_de_mise_a_jour() {
        assert!(!est_plus_recente("neurodesk-portable-2026-12-01", "portable-2026-11-09"));
        assert!(!est_plus_recente("master-portable-2026-12-01", "portable-2026-11-09"));
        assert!(!est_plus_recente("v1.2.3", "portable-2026-11-09"));
    }

    #[test]
    fn une_version_locale_inconnue_accepte_la_release() {
        assert!(est_plus_recente("portable-2026-11-10", "inconnue"));
        assert!(est_plus_recente("portable-2026-11-10", ""));
        assert!(est_plus_recente("portable-2026-11-10", "neurodesk-portable-2026-11-09"));
    }

    #[test]
    fn un_chemin_absolu_ou_parent_est_refuse() {
        assert_eq!(chemin_relatif("NeuraDesk-portable/assets/popups/aide.html"), Some(PathBuf::from("NeuraDesk-portable/assets/popups/aide.html")));
        assert_eq!(chemin_relatif("a/./b"), Some(PathBuf::from("a/b")));
        assert_eq!(chemin_relatif("../evade.txt"), None);
        assert_eq!(chemin_relatif("a/../../evade.txt"), None);
        assert_eq!(chemin_relatif("/absolu.txt"), None);
        assert_eq!(chemin_relatif("C:/Windows/evade.txt"), None);
        assert_eq!(chemin_relatif(""), None);
    }

    #[test]
    fn la_reponse_de_github_est_lue_et_l_url_est_verifiee() {
        let valeur: Value = serde_json::from_str(
            r#"{
              "tag_name": "portable-2026-11-10",
              "html_url": "https://github.com/axalox/NeuraDesk/releases/tag/portable-2026-11-10",
              "assets": [
                {"name": "NeuraDesk-portable.zip", "size": 276959808,
                 "browser_download_url": "https://github.com/axalox/NeuraDesk/releases/download/portable-2026-11-10/NeuraDesk-portable.zip"},
                {"name": "NeuraDesk-portable.zip.sha256", "size": 80,
                 "browser_download_url": "https://github.com/axalox/NeuraDesk/releases/download/portable-2026-11-10/NeuraDesk-portable.zip.sha256"}
              ]
            }"#,
        )
        .unwrap();
        let sortie = lire_reponse(&valeur).expect("réponse complète");
        assert_eq!(sortie.tag, "portable-2026-11-10");
        assert_eq!(sortie.archive.taille, Some(276959808));
        assert!(sortie.archive.somme_sha256.is_some());

        let detournee: Value = serde_json::from_str(
            r#"{"tag_name":"portable-2026-11-10","assets":[{"name":"NeuraDesk-portable.zip","size":1,
                "browser_download_url":"https://exemple.invalide/NeuraDesk-portable.zip"}]}"#,
        )
        .unwrap();
        assert!(lire_reponse(&detournee).is_err());
    }

    #[test]
    fn une_release_sans_l_archive_portable_est_refusee() {
        let valeur: Value = serde_json::from_str(
            r#"{"tag_name":"portable-2026-11-10","assets":[{"name":"autre.zip","browser_download_url":"https://github.com/axalox/NeuraDesk/releases/download/x/autre.zip"}]}"#,
        )
        .unwrap();
        assert!(lire_reponse(&valeur).is_err());
    }

    #[test]
    fn l_empreinte_publiee_se_lit_dans_les_deux_formats() {
        let hash = "a".repeat(64);
        assert_eq!(attendue_depuis_texte(&format!("{hash}  NeuraDesk-portable.zip")), Some(hash.clone()));
        assert_eq!(attendue_depuis_texte(&format!("{hash}\n")), Some(hash));
        assert_eq!(attendue_depuis_texte("pas de somme ici"), None);
    }

    #[test]
    fn le_sha256_est_celui_attendu() {
        // Vecteur de la FIPS 180-4 : SHA-256("abc").
        let mut hash = Hash256::ouvrir().expect("BCrypt SHA-256");
        hash.ajouter(b"abc").expect("ajout");
        assert_eq!(hash.terminer().unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn la_taille_est_dite_en_unites_lisibles() {
        assert_eq!(taille_lisible(512), "512 o");
        assert_eq!(taille_lisible(276_959_808), "264.1 Mo");
        assert_eq!(taille_lisible(2 * 1024 * 1024 * 1024), "2.0 Go");
    }

    #[test]
    fn l_etat_est_relisible_et_porte_la_version_locale() {
        let valeur = etat_json();
        assert!(valeur.get("etat").and_then(Value::as_str).is_some());
        assert_eq!(valeur.get("actuelle").and_then(Value::as_str), Some(ACTUELLE));
    }

    /// Un dossier de test unique, nettoyé à la fin.
    fn terrain(nom: &str) -> PathBuf {
        let dossier = std::env::temp_dir().join(format!("nd-maj-{nom}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dossier);
        std::fs::create_dir_all(&dossier).expect("dossier de test");
        dossier
    }

    fn zip_avec(chemin: &Path, entrees: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(chemin).unwrap());
        for (nom, contenu) in entrees {
            zip.start_file(*nom, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(contenu).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn une_archive_portable_est_extraite_et_sa_racine_est_trouvee() {
        let terrain = terrain("extraction");
        let archive = terrain.join("portable.zip");
        zip_avec(
            &archive,
            &[
                ("NeuraDesk-portable/neuradesk.exe", b"exe"),
                ("NeuraDesk-portable/assets/popups/aide.html", b"<html>"),
            ],
        );
        let cible = terrain.join("prepare");
        extraire(&archive, &cible, "portable-2026-11-10").expect("extraction");
        assert!(cible.join("NeuraDesk-portable/neuradesk.exe").is_file());
        assert!(cible.join("NeuraDesk-portable/assets/popups/aide.html").is_file());
        assert_eq!(racine_de_l_application(&cible), Some(cible.join("NeuraDesk-portable")));
        let _ = std::fs::remove_dir_all(&terrain);
    }

    #[test]
    fn une_entree_avec_parent_refuse_l_archive() {
        let terrain = terrain("evasion");
        let archive = terrain.join("mechante.zip");
        zip_avec(
            &archive,
            &[("NeuraDesk-portable/neuradesk.exe", b"exe"), ("../evade.txt", b"dehors")],
        );
        let cible = terrain.join("prepare");
        let resultat = extraire(&archive, &cible, "portable-2026-11-10");
        assert!(resultat.is_err(), "une entrée `../` doit être refusée");
        assert!(!terrain.join("evade.txt").exists(), "rien ne doit être écrit hors du dossier");
        let _ = std::fs::remove_dir_all(&terrain);
    }

    #[test]
    fn un_echec_de_lancement_revient_a_l_ancienne_version() {
        let terrain = terrain("retour");
        let app = terrain.join("app");
        let nouveau = terrain.join("new/NeuraDesk-portable");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::create_dir_all(&nouveau).unwrap();
        // Un « exe » qui n'en est pas un : `Command::spawn` échoue, ce qui
        // éprouve le retour en arrière SANS lancer de processus.
        std::fs::write(app.join("neuradesk.exe"), b"ancienne").unwrap();
        std::fs::write(app.join("marqueur-ancien.txt"), b"1").unwrap();
        std::fs::write(nouveau.join("neuradesk.exe"), "pas un exécutable".as_bytes()).unwrap();
        std::fs::write(nouveau.join("marqueur-nouveau.txt"), b"1").unwrap();

        let resultat = appliquer(0, &app, &nouveau, &terrain.join("new"));
        assert!(resultat.is_err(), "l'assistant doit signaler l'échec");
        assert!(app.join("marqueur-ancien.txt").is_file(), "l'ancienne version doit être revenue");
        assert!(!app.join("marqueur-nouveau.txt").exists());
        assert!(!terrain.join("new/NeuraDesk-portable").exists(), "le nouveau dossier a été déplacé");
        let _ = std::fs::remove_dir_all(&terrain);
    }

    /// ⭐ **UN DOSSIER IMPOSSIBLE À RENOMMER SE REMPLACE SUR PLACE.**
    ///
    /// C'est le défaut signalé le 2026-11-10 : l'assistant échouait sur
    /// « renommage de "Z:\\NeuraDesk-portable" impossible : Accès refusé. (os
    /// error 5) ». Le répertoire de travail d'un processus ne peut PAS être
    /// renommé par Windows : c'est une façon déterministe de reproduire le
    /// refus, sans toucher aux droits d'un disque ni attendre les soixante
    /// secondes de réessai.
    #[test]
    fn un_dossier_impossible_a_renommer_est_remplace_sur_place() {
        let terrain = terrain("sur-place");
        let app = terrain.join("app");
        let nouveau = terrain.join("new/NeuraDesk-portable");
        std::fs::create_dir_all(app.join("assets/popups")).unwrap();
        std::fs::create_dir_all(nouveau.join("assets/popups")).unwrap();
        std::fs::write(app.join("neuradesk.exe"), b"ancienne").unwrap();
        std::fs::write(app.join("marqueur-ancien.txt"), b"1").unwrap();
        std::fs::write(app.join("assets/popups/vieille.html"), b"<html>").unwrap();
        std::fs::write(nouveau.join("neuradesk.exe"), b"pas un executable").unwrap();
        std::fs::write(nouveau.join("marqueur-nouveau.txt"), b"1").unwrap();
        std::fs::write(nouveau.join("assets/popups/neuve.html"), b"<html>").unwrap();

        let avant = std::env::current_dir().expect("dossier courant");
        std::env::set_current_dir(&app).expect("dossier courant = l'application");
        let resultat = appliquer_avec(0, &app, &nouveau, &terrain.join("new"), Duration::ZERO);
        std::env::set_current_dir(&avant).expect("dossier courant rendu");

        // La fausse version ne se lance pas, donc l'assistant signale un échec —
        // mais les FICHIERS ont été posés, et l'ancien exécutable est revenu.
        assert!(resultat.is_err(), "lancer un faux exécutable doit échouer");
        assert!(app.join("marqueur-nouveau.txt").is_file(), "les fichiers doivent être recopiés : {resultat:?}");
        assert!(app.join("assets/popups/neuve.html").is_file(), "les sous-dossiers aussi");
        assert_eq!(std::fs::read(app.join("neuradesk.exe")).unwrap(), b"ancienne", "l'ancien exécutable doit revenir");
        let restes: Vec<_> = std::fs::read_dir(&terrain).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(restes.len(), 2, "ni .ancien ni .rate ni sauvegarde ne doivent rester : {restes:?}");
        let _ = std::fs::remove_dir_all(&terrain);
    }

    #[test]
    fn une_archive_sans_executable_est_refusee() {
        let terrain = terrain("sans-exe");
        let archive = terrain.join("incomplete.zip");
        zip_avec(&archive, &[("NeuraDesk-portable/assets/aide.html", b"<html>")]);
        let cible = terrain.join("prepare");
        assert!(extraire(&archive, &cible, "portable-2026-11-10").is_err());
        let _ = std::fs::remove_dir_all(&terrain);
    }

    /// **SONDE RÉSEAU, `#[ignore]`** — éprouve la redirection ET le
    /// téléchargement en flux sur `releases/latest`, que GitHub redirige vers la
    /// page du tag. Le corps est du HTML (petit), donc cet essai ne coûte pas
    /// 277 Mo.
    #[test]
    #[ignore]
    fn telecharge_un_petit_corps_a_travers_une_redirection() {
        let chemin = std::env::temp_dir().join(format!("nd-maj-redirection-{}.bin", std::process::id()));
        let (octets, somme) = telecharger(PAGE_RELEASES, &chemin, "essai").expect("téléchargement");
        assert!(octets > 1_000, "corps trop court : {octets} octets");
        assert_eq!(somme.len(), 64);
        let debut = std::fs::read(&chemin).expect("relecture");
        let entete = &debut[..debut.len().min(2048)];
        assert!(
            entete.windows(9).any(|f| f == b"<!DOCTYPE") || entete.windows(5).any(|f| f == b"<html"),
            "ce n'est pas la page attendue"
        );
        let _ = std::fs::remove_file(&chemin);
        println!("redirection suivie : {octets} octets, sha256 {somme}");
    }

    /// **SONDE RÉSEAU, `#[ignore]`** — mesure la vraie release publiée, comme
    /// `meteo::tests` le fait pour Open-Meteo. À lancer à la main :
    /// `cargo test --bin neuradesk -- --ignored --nocapture interroge_la_release`.
    #[test]
    #[ignore]
    fn interroge_la_release_publiee() {
        let valeur = get_json(API_DERNIERE).expect("API GitHub");
        let sortie = lire_reponse(&valeur).expect("release exploitable");
        println!(
            "tag distant : {} — locale : {ACTUELLE} — mise à jour : {}",
            sortie.tag,
            est_plus_recente(&sortie.tag, ACTUELLE)
        );
        println!("archive : {} ({} octets)", sortie.archive.url, sortie.archive.taille.unwrap_or(0));
    }
}
