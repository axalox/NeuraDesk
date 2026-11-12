//! **ENVOYER UNE IMAGE EN LIGNE** — l'outil « imgupload » de Flameshot, mais avec
//! un CHOIX d'hébergeur dans les Réglages (demande du 2026-09-25 : « un choix dans
//! les Réglages »), là où Flameshot n'offre qu'Imgur.
//!
//! ⚠️ **ENVOYER, C'EST PUBLIER.** Une image envoyée est lisible par quiconque a le
//! lien, et un envoi anonyme chez catbox **ne peut plus être supprimé**. D'où deux
//! règles : l'envoi ne part que d'un geste explicite dans l'éditeur (confirmé en
//! deux clics, voir `editeur_image.html`), et **aucun test n'ouvre le réseau** —
//! tout ce qui se vérifie hors ligne (corps multipart, requête par hébergeur,
//! lecture des réponses) est isolé en fonctions pures.
//!
//! Les services et leurs règles ont été relevés dans leur documentation le
//! 2026-09-25 :
//! - **Litterbox** (catbox) — anonyme, EXPIRE après 1, 12, 24 ou 72 h ;
//! - **Catbox** — anonyme, gardé tant qu'il est consulté (supprimé après 2 ans
//!   sans accès), 200 Mo ;
//! - **0x0.st** — anonyme, de 30 jours à 1 an selon la taille, exige un
//!   User-Agent qui nomme honnêtement le programme ;
//! - **ImgBB** et **Freeimage.host** — clé API gratuite (même moteur, Chevereto) ;
//! - **Imgur** — un Client-ID obtenu en déclarant une application.
//!
//! Et un **SERVEUR PERSONNALISÉ** (demande du 2026-09-25 : « la possibilité
//! d'utiliser un serveur personnalisé ») sur le modèle des « custom uploaders »
//! de ShareX : une URL, des en-têtes, des champs de formulaire, et l'endroit où
//! lire le lien dans la réponse — importables depuis un fichier `.sxcu`, le
//! format que Zipline, XBackBone, Chibisafe… exportent. ⚠️ Les VALEURS des
//! en-têtes, champs et paramètres d'URL portent en général le jeton du compte :
//! comme les clés, elles ne repartent jamais vers la page (seulement leurs noms).
//!
//! Le réseau passe par **WinHTTP** (livré avec Windows) : pas de nouvelle
//! dépendance, et le proxy du système est suivi (`WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY`).

use std::collections::BTreeMap;
use std::sync::mpsc::Receiver;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

use crate::config;

/// Le fichier des réglages d'envoi, dans `%APPDATA%\NeuraDesk\`.
const FICHIER_REGLAGES: &str = "envoi_image.toml";
/// Délai de résolution, de connexion, d'envoi et de réception (ms) — une image de
/// quelques Mo sur une ligne lente doit passer, un serveur muet ne doit pas
/// bloquer le fil indéfiniment.
const DELAI_RESEAU_MS: i32 = 60_000;
/// La durée de Litterbox quand aucune n'a été choisie.
const DUREE_PAR_DEFAUT: &str = "24h";
/// Les seules durées que Litterbox accepte (voir sa documentation).
pub const DUREES_LITTERBOX: [&str; 4] = ["1h", "12h", "24h", "72h"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hebergeur {
    Litterbox,
    Catbox,
    ZeroXZero,
    ImgBB,
    Freeimage,
    Imgur,
    Perso,
}

impl Hebergeur {
    pub const ALL: [Hebergeur; 7] = [
        Hebergeur::Litterbox,
        Hebergeur::Catbox,
        Hebergeur::ZeroXZero,
        Hebergeur::ImgBB,
        Hebergeur::Freeimage,
        Hebergeur::Imgur,
        Hebergeur::Perso,
    ];

    /// Nom STABLE, écrit dans `envoi_image.toml` — jamais traduit.
    pub fn as_str(self) -> &'static str {
        match self {
            Hebergeur::Litterbox => "litterbox",
            Hebergeur::Catbox => "catbox",
            Hebergeur::ZeroXZero => "0x0",
            Hebergeur::ImgBB => "imgbb",
            Hebergeur::Freeimage => "freeimage",
            Hebergeur::Imgur => "imgur",
            Hebergeur::Perso => "perso",
        }
    }

    pub fn from_config(nom: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|h| h.as_str() == nom)
    }

    pub fn label(self) -> &'static str {
        match self {
            Hebergeur::Litterbox => "Litterbox",
            Hebergeur::Catbox => "Catbox",
            Hebergeur::ZeroXZero => "0x0.st",
            Hebergeur::ImgBB => "ImgBB",
            Hebergeur::Freeimage => "Freeimage.host",
            Hebergeur::Imgur => "Imgur",
            Hebergeur::Perso => "Personnalisé",
        }
    }

    /// Ce que l'utilisateur doit savoir AVANT de choisir : combien de temps
    /// l'image reste en ligne, et s'il faut une clé.
    pub fn description(self) -> &'static str {
        match self {
            Hebergeur::Litterbox => "Sans compte. L'image est supprimée après la durée choisie.",
            Hebergeur::Catbox => "Sans compte. L'image reste en ligne tant qu'elle est consultée (supprimée après 2 ans sans accès) et ne peut PAS être supprimée sans compte.",
            Hebergeur::ZeroXZero => "Sans compte. L'image reste de 30 jours à 1 an selon sa taille ; le lien est long et difficile à deviner.",
            Hebergeur::ImgBB => "Clé API gratuite (api.imgbb.com, avec un compte). Image permanente, supprimable depuis le compte.",
            Hebergeur::Freeimage => "Clé API (freeimage.host/page/api). Image permanente.",
            Hebergeur::Imgur => "Client-ID à obtenir en déclarant une application (api.imgur.com/oauth2/addclient). Image permanente.",
            Hebergeur::Perso => "Votre propre serveur (Zipline, XBackBone, Chibisafe…) : l'image part en POST multipart. Importez le fichier ShareX (.sxcu) qu'il fournit, ou remplissez les champs.",
        }
    }

    /// Le libellé de la clé que le service exige, ou `None` s'il est anonyme.
    pub fn cle(self) -> Option<&'static str> {
        match self {
            Hebergeur::ImgBB | Hebergeur::Freeimage => Some("Clé API"),
            Hebergeur::Imgur => Some("Client-ID"),
            Hebergeur::Litterbox | Hebergeur::Catbox | Hebergeur::ZeroXZero | Hebergeur::Perso => None,
        }
    }
}

/// Ce que l'utilisateur a choisi dans les Réglages.
#[derive(Clone, Debug, PartialEq)]
pub struct Reglages {
    pub hebergeur: Hebergeur,
    /// Durée Litterbox, toujours une de [`DUREES_LITTERBOX`].
    pub duree: String,
    /// Une clé par hébergeur (indexée par `Hebergeur::as_str`) : changer de service
    /// ne doit pas effacer la clé d'un autre.
    pub cles: BTreeMap<String, String>,
    pub perso: ServeurPerso,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages {
            hebergeur: Hebergeur::Litterbox,
            duree: DUREE_PAR_DEFAUT.to_string(),
            cles: BTreeMap::new(),
            perso: ServeurPerso::default(),
        }
    }
}

/// Le champ qui porte le fichier quand rien d'autre n'est dit — celui de 0x0.st,
/// de Zipline et de la plupart des serveurs à héberger soi-même.
const CHAMP_FICHIER_PAR_DEFAUT: &str = "file";

/// Le serveur personnalisé, sur le modèle d'un « custom uploader » de ShareX.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServeurPerso {
    /// `https://hôte[:port]/chemin`, SANS requête : ses paramètres vivent dans
    /// `parametres`, pour que la page puisse montrer l'URL sans montrer un jeton.
    pub url: String,
    /// Vide = [`CHAMP_FICHIER_PAR_DEFAUT`].
    pub champ_fichier: String,
    /// Où lire le lien dans la réponse — voir [`lecture_perso`].
    pub lien: String,
    /// ⚠️ SECRETS : valeurs jamais renvoyées à la page ni journalisées.
    pub entetes: BTreeMap<String, String>,
    pub champs: BTreeMap<String, String>,
    pub parametres: BTreeMap<String, String>,
}

impl ServeurPerso {
    fn champ_fichier(&self) -> &str {
        if self.champ_fichier.is_empty() { CHAMP_FICHIER_PAR_DEFAUT } else { &self.champ_fichier }
    }

    /// Change l'URL : les paramètres de sa requête REMPLACENT ceux d'avant — c'est
    /// le seul moyen de les modifier à la main, et il est prévisible.
    pub fn changer_url(&mut self, url: &str) -> Result<(), String> {
        let adresse = decouper_url(url.trim())?;
        self.url = adresse.sans_requete();
        self.parametres = adresse.requete.into_iter().collect();
        Ok(())
    }

    /// Lit un fichier `.sxcu` de ShareX et rend le serveur avec son nom. Seul
    /// l'envoi en POST multipart est pris en charge — celui de tous les serveurs
    /// d'images connus — et un fichier qui demande autre chose est REFUSÉ plutôt
    /// qu'envoyé de travers.
    pub fn depuis_sxcu(texte: &str) -> Result<(ServeurPerso, String), String> {
        let json: serde_json::Value = serde_json::from_str(texte.trim().trim_start_matches('\u{feff}'))
            .map_err(|_| "ce n'est pas un fichier .sxcu (JSON illisible)".to_string())?;
        let texte_de = |cle: &str| json.get(cle).and_then(serde_json::Value::as_str).unwrap_or("").trim().to_string();
        let methode = texte_de("RequestMethod");
        if !methode.is_empty() && !methode.eq_ignore_ascii_case("POST") {
            return Err(format!("méthode {methode} non prise en charge (seulement POST)"));
        }
        let corps = texte_de("Body");
        if !corps.is_empty() && corps != "MultipartFormData" {
            return Err(format!("corps « {corps} » non pris en charge (seulement MultipartFormData)"));
        }
        let table = |cle: &str| -> BTreeMap<String, String> {
            json.get(cle)
                .and_then(serde_json::Value::as_object)
                .map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_str().map_or_else(|| v.to_string(), str::to_string))).collect())
                .unwrap_or_default()
        };
        let mut perso = ServeurPerso {
            champ_fichier: texte_de("FileFormName"),
            lien: texte_de("URL"),
            entetes: table("Headers"),
            champs: table("Arguments"),
            ..ServeurPerso::default()
        };
        perso.changer_url(&texte_de("RequestURL"))?;
        // Les paramètres du fichier s'AJOUTENT à ceux écrits dans l'URL.
        perso.parametres.extend(table("Parameters"));
        verifier_noms(&perso)?;
        let nom = texte_de("Name");
        let nom = if nom.is_empty() { decouper_url(&perso.url)?.hote } else { nom };
        Ok((perso, nom))
    }

    fn depuis_toml(table: &toml::Table) -> Self {
        let texte = |cle: &str| table.get(cle).and_then(toml::Value::as_str).unwrap_or("").to_string();
        ServeurPerso {
            url: texte("url"),
            champ_fichier: texte("champ_fichier"),
            lien: texte("lien"),
            entetes: table_de_textes(table.get("entetes")),
            champs: table_de_textes(table.get("champs")),
            parametres: table_de_textes(table.get("parametres")),
        }
    }

    fn vers_toml(&self) -> toml::Table {
        let vers = |m: &BTreeMap<String, String>| -> toml::Value {
            m.iter().map(|(k, v)| (k.clone(), toml::Value::from(v.clone()))).collect::<toml::Table>().into()
        };
        let mut table = toml::Table::new();
        table.insert("url".into(), self.url.clone().into());
        table.insert("champ_fichier".into(), self.champ_fichier.clone().into());
        table.insert("lien".into(), self.lien.clone().into());
        table.insert("entetes".into(), vers(&self.entetes));
        table.insert("champs".into(), vers(&self.champs));
        table.insert("parametres".into(), vers(&self.parametres));
        table
    }
}

fn table_de_textes(valeur: Option<&toml::Value>) -> BTreeMap<String, String> {
    valeur
        .and_then(toml::Value::as_table)
        .map(|t| t.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
        .unwrap_or_default()
}

/// Lit des lignes « `Nom: valeur` » (en-têtes) ou « `nom=valeur` » (champs) telles
/// que tapées dans les Réglages. Une ligne vide est ignorée ; une ligne sans
/// séparateur est une ERREUR, pas une ligne perdue en silence.
pub fn lire_lignes(texte: &str, separateur: char) -> Result<BTreeMap<String, String>, String> {
    let mut table = BTreeMap::new();
    for ligne in texte.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (nom, valeur) = ligne
            .split_once(separateur)
            .ok_or_else(|| format!("« {} » : il manque le « {separateur} »", ligne.chars().take(40).collect::<String>()))?;
        table.insert(nom.trim().to_string(), valeur.trim().to_string());
    }
    Ok(table)
}

/// Un nom d'en-tête ou de champ ne doit pas pouvoir casser la requête : un saut
/// de ligne y injecterait un en-tête, un guillemet fermerait le `name="…"` du
/// multipart. Les VALEURS d'en-tête sont vérifiées aussi.
pub fn verifier_noms(perso: &ServeurPerso) -> Result<(), String> {
    let interdit = |t: &str| t.contains(['\r', '\n']);
    for (nom, valeur) in &perso.entetes {
        if nom.is_empty() || interdit(nom) || nom.contains(':') || interdit(valeur) {
            return Err(format!("en-tête « {nom} » invalide"));
        }
    }
    for nom in perso.champs.keys().chain(std::iter::once(&perso.champ_fichier)) {
        if interdit(nom) || nom.contains('"') {
            return Err(format!("nom de champ « {nom} » invalide"));
        }
    }
    Ok(())
}

/// Une URL découpée pour WinHTTP, qui veut l'hôte, le port et le chemin À PART.
#[derive(Debug, PartialEq)]
struct Adresse {
    securisee: bool,
    hote: String,
    port: u16,
    chemin: String,
    /// Décodée : ré-encodée à l'envoi par [`encoder_url`].
    requete: Vec<(String, String)>,
}

impl Adresse {
    fn sans_requete(&self) -> String {
        let (schema, defaut) = if self.securisee { ("https", 443) } else { ("http", 80) };
        let port = if self.port == defaut { String::new() } else { format!(":{}", self.port) };
        format!("{schema}://{}{port}{}", self.hote, self.chemin)
    }
}

/// `http://` est ACCEPTÉ — un serveur du réseau local n'a souvent pas de
/// certificat — mais la page prévient que le jeton y circule en clair.
fn decouper_url(url: &str) -> Result<Adresse, String> {
    let (securisee, reste) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err("l'adresse doit commencer par https:// (ou http://)".into());
    };
    let reste = reste.split('#').next().unwrap_or("");
    let fin_hote = reste.find(['/', '?']).unwrap_or(reste.len());
    let (hote_port, chemin_requete) = reste.split_at(fin_hote);
    let (hote, port) = match hote_port.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| format!("port « {p} » invalide"))?),
        None => (hote_port, if securisee { 443 } else { 80 }),
    };
    if hote.is_empty() || !hote.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
        return Err(format!("hôte « {hote} » invalide"));
    }
    let (chemin, requete) = chemin_requete.split_once('?').unwrap_or((chemin_requete, ""));
    if chemin.contains(char::is_whitespace) {
        return Err("l'adresse contient un espace".into());
    }
    let requete = requete
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (crate::win32::cef_app::percent_decode(k), crate::win32::cef_app::percent_decode(v))
        })
        .collect();
    Ok(Adresse {
        securisee,
        hote: hote.to_ascii_lowercase(),
        port,
        chemin: if chemin.is_empty() { "/".into() } else { chemin.into() },
        requete,
    })
}

impl Reglages {
    /// Le nom à montrer dans l'éditeur : pour un serveur personnalisé, son hôte.
    pub fn nom_affiche(&self) -> String {
        match self.hebergeur {
            Hebergeur::Perso => decouper_url(&self.perso.url).map_or_else(|_| "serveur personnalisé".into(), |a| a.hote),
            h => h.label().to_string(),
        }
    }

    pub fn cle_de(&self, hebergeur: Hebergeur) -> Option<&str> {
        self.cles.get(hebergeur.as_str()).map(String::as_str).filter(|c| !c.is_empty())
    }

    fn depuis_toml(texte: &str) -> Self {
        let Ok(table) = texte.parse::<toml::Table>() else { return Reglages::default() };
        let hebergeur = table
            .get("service")
            .and_then(toml::Value::as_str)
            .and_then(Hebergeur::from_config)
            .unwrap_or(Hebergeur::Litterbox);
        let duree = table
            .get("duree")
            .and_then(toml::Value::as_str)
            .filter(|d| DUREES_LITTERBOX.contains(d))
            .unwrap_or(DUREE_PAR_DEFAUT)
            .to_string();
        let cles = table_de_textes(table.get("cles"));
        let perso = table.get("perso").and_then(toml::Value::as_table).map(ServeurPerso::depuis_toml).unwrap_or_default();
        Reglages { hebergeur, duree, cles, perso }
    }

    fn vers_toml(&self) -> String {
        let mut table = toml::Table::new();
        table.insert("service".into(), self.hebergeur.as_str().into());
        table.insert("duree".into(), self.duree.clone().into());
        let cles: toml::Table = self.cles.iter().map(|(k, v)| (k.clone(), toml::Value::from(v.clone()))).collect();
        table.insert("cles".into(), cles.into());
        table.insert("perso".into(), self.perso.vers_toml().into());
        toml::to_string(&table).unwrap_or_default()
    }
}

pub fn charger_reglages() -> Reglages {
    config::config_path(FICHIER_REGLAGES)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| Reglages::depuis_toml(&t))
        .unwrap_or_default()
}

pub fn enregistrer_reglages(reglages: &Reglages) {
    if !config::write_config_atomique(FICHIER_REGLAGES, reglages.vers_toml()) {
        crate::diag!("[envoi] réglages NON enregistrés ({FICHIER_REGLAGES})");
    }
}

/// Une requête prête à partir : tout sauf le fichier lui-même.
#[derive(Debug, PartialEq)]
struct Requete {
    hote: String,
    port: u16,
    /// `false` seulement pour un serveur personnalisé en `http://`.
    securisee: bool,
    /// Chemin ET requête (`/1/upload?key=…`). ⚠️ Peut porter une clé : jamais
    /// dans le journal.
    chemin: String,
    /// En-têtes supplémentaires (sans `Content-Type`, ajouté à l'envoi).
    entetes: Vec<String>,
    /// Champs texte du formulaire.
    champs: Vec<(String, String)>,
    /// Le nom du champ qui porte le fichier.
    champ_fichier: String,
    lecture: Lecture,
}

/// Où se trouve le lien dans la réponse.
#[derive(Debug, PartialEq)]
enum Lecture {
    /// Le corps entier est le lien.
    Texte,
    /// Un pointeur JSON (RFC 6901) vers le lien.
    Json(String),
    /// Un modèle à la ShareX : du texte avec des `{json:…}`/`$json:…$` et
    /// `{response}`/`$response$` à remplacer.
    Modele(String),
}

/// Une requête vers un des services connus, en HTTPS sur le port 443.
fn requete_connue(hote: &str, chemin: String, entetes: Vec<String>, champs: &[(&str, String)], champ_fichier: &str, lecture: Lecture) -> Requete {
    Requete {
        hote: hote.into(),
        port: INTERNET_DEFAULT_HTTPS_PORT,
        securisee: true,
        chemin,
        entetes,
        champs: champs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
        champ_fichier: champ_fichier.into(),
        lecture,
    }
}

fn requete(reglages: &Reglages) -> Result<Requete, String> {
    let hebergeur = reglages.hebergeur;
    let cle = || {
        reglages.cle_de(hebergeur).map(str::trim).ok_or_else(|| {
            format!("{} demande une {} : renseignez-la dans Réglages → Captures d'écran et éditeur.", hebergeur.label(), hebergeur.cle().unwrap_or("clé"))
        })
    };
    let json = |pointeur: &str| Lecture::Json(pointeur.into());
    Ok(match hebergeur {
        Hebergeur::Litterbox => requete_connue(
            "litterbox.catbox.moe",
            "/resources/internals/api.php".into(),
            vec![],
            &[("reqtype", "fileupload".into()), ("time", reglages.duree.clone())],
            "fileToUpload",
            Lecture::Texte,
        ),
        Hebergeur::Catbox => requete_connue("catbox.moe", "/user/api.php".into(), vec![], &[("reqtype", "fileupload".into())], "fileToUpload", Lecture::Texte),
        // `secret` (valeur ignorée) : « a longer, hard-to-guess URL will be generated ».
        Hebergeur::ZeroXZero => requete_connue("0x0.st", "/".into(), vec![], &[("secret", String::new())], "file", Lecture::Texte),
        Hebergeur::ImgBB => requete_connue("api.imgbb.com", format!("/1/upload?key={}", encoder_url(cle()?)), vec![], &[], "image", json("/data/url")),
        Hebergeur::Freeimage => requete_connue(
            "freeimage.host",
            format!("/api/1/upload?key={}&action=upload&format=json", encoder_url(cle()?)),
            vec![],
            &[],
            "source",
            json("/image/url"),
        ),
        // Le Client-ID va dans un EN-TÊTE : il ne doit pas être encodé en URL.
        Hebergeur::Imgur => requete_connue(
            "api.imgur.com",
            "/3/image".into(),
            vec![format!("Authorization: Client-ID {}", cle()?)],
            &[("type", "file".into())],
            "image",
            json("/data/link"),
        ),
        Hebergeur::Perso => requete_perso(&reglages.perso)?,
    })
}

fn requete_perso(perso: &ServeurPerso) -> Result<Requete, String> {
    if perso.url.is_empty() {
        return Err("Aucun serveur personnalisé : renseignez son adresse dans Réglages → Captures d'écran et éditeur.".into());
    }
    let adresse = decouper_url(&perso.url).map_err(|e| format!("Serveur personnalisé : {e}"))?;
    verifier_noms(perso).map_err(|e| format!("Serveur personnalisé : {e}"))?;
    let mut chemin = adresse.chemin;
    // `adresse.requete` est vide (l'URL est rangée sans elle, voir
    // `ServeurPerso::url`) : les paramètres viennent tous de `parametres`.
    let parametres: Vec<String> = perso.parametres.iter().map(|(k, v)| format!("{}={}", encoder_url(k), encoder_url(v))).collect();
    if !parametres.is_empty() {
        chemin.push('?');
        chemin.push_str(&parametres.join("&"));
    }
    Ok(Requete {
        hote: adresse.hote,
        port: adresse.port,
        securisee: adresse.securisee,
        chemin,
        // `Content-Type` est celui du multipart, posé à l'envoi : un second
        // en-tête du même nom le contredirait.
        entetes: perso
            .entetes
            .iter()
            .filter(|(nom, _)| !nom.eq_ignore_ascii_case("content-type"))
            .map(|(nom, valeur)| format!("{nom}: {valeur}"))
            .collect(),
        champs: perso.champs.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        champ_fichier: perso.champ_fichier().to_string(),
        lecture: lecture_perso(&perso.lien),
    })
}

/// Le champ « lien » d'un serveur personnalisé : vide = le corps entier ; avec
/// un `{…}` ou `$…$` = un modèle ShareX (`{json:files[0].url}`,
/// `https://hote/{json:name}`) ; sinon un chemin JSON (`data.link`,
/// `files[0].url` ou un pointeur `/data/link`).
fn lecture_perso(lien: &str) -> Lecture {
    let lien = lien.trim();
    if lien.is_empty() {
        Lecture::Texte
    } else if lien.contains(['{', '$']) {
        Lecture::Modele(lien.into())
    } else {
        Lecture::Json(pointeur_json(lien))
    }
}

/// `files[0].url` → `/files/0/url` ; un pointeur déjà écrit (`/…`) est rendu tel quel.
fn pointeur_json(chemin: &str) -> String {
    if chemin.starts_with('/') {
        return chemin.into();
    }
    let mut pointeur = String::from("/");
    for c in chemin.chars() {
        match c {
            '.' | '[' => pointeur.push('/'),
            ']' => {}
            _ => pointeur.push(c),
        }
    }
    pointeur.replace("//", "/")
}

/// Remplit un modèle ShareX avec la réponse. Un `{json:…}` qui ne désigne rien
/// est une ERREUR : un lien à moitié rempli ne mène nulle part.
fn remplir_modele(modele: &str, corps: &str) -> Option<String> {
    let json: Option<serde_json::Value> = serde_json::from_str(corps).ok();
    let mut sortie = String::new();
    let mut reste = modele;
    while let Some(i) = reste.find(['{', '$']) {
        sortie.push_str(&reste[..i]);
        let ouvrant = reste.as_bytes()[i] as char;
        let fermant = if ouvrant == '{' { '}' } else { '$' };
        let apres = &reste[i + 1..];
        let Some(j) = apres.find(fermant) else {
            sortie.push_str(&reste[i..]);
            return Some(sortie);
        };
        let jeton = &apres[..j];
        if jeton == "response" {
            sortie.push_str(corps);
        } else if let Some(chemin) = jeton.strip_prefix("json:") {
            let valeur = json.as_ref()?.pointer(&pointeur_json(chemin))?;
            sortie.push_str(&valeur.as_str().map_or_else(|| valeur.to_string(), str::to_string));
        } else {
            // Inconnu (`{filename}`…) : laissé tel quel, comme ShareX.
            sortie.push(ouvrant);
            sortie.push_str(jeton);
            sortie.push(fermant);
        }
        reste = &apres[j + 1..];
    }
    sortie.push_str(reste);
    Some(sortie)
}

/// Encodage « pourcent » de ce qui n'est pas non réservé (RFC 3986) — une clé
/// collée avec un espace ou un `&` ne doit pas casser la requête.
fn encoder_url(texte: &str) -> String {
    texte
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Le corps `multipart/form-data` : les champs texte, puis le fichier PNG.
fn corps_multipart(frontiere: &str, champs: &[(String, String)], champ_fichier: &str, nom_fichier: &str, png: &[u8]) -> Vec<u8> {
    let mut corps = Vec::with_capacity(png.len() + 512);
    for (nom, valeur) in champs {
        corps.extend_from_slice(format!("--{frontiere}\r\nContent-Disposition: form-data; name=\"{nom}\"\r\n\r\n{valeur}\r\n").as_bytes());
    }
    corps.extend_from_slice(
        format!("--{frontiere}\r\nContent-Disposition: form-data; name=\"{champ_fichier}\"; filename=\"{nom_fichier}\"\r\nContent-Type: image/png\r\n\r\n").as_bytes(),
    );
    corps.extend_from_slice(png);
    corps.extend_from_slice(format!("\r\n--{frontiere}--\r\n").as_bytes());
    corps
}

/// Le lien de l'image, lu dans la réponse du service — ou le message d'erreur à
/// montrer. `nom` ne sert qu'au message.
fn lire_reponse(nom: &str, lecture: &Lecture, statut: u32, corps: &str) -> Result<String, String> {
    let texte = corps.trim();
    let lien = match lecture {
        Lecture::Texte => Some(texte.to_string()),
        Lecture::Json(pointeur) => {
            let json: serde_json::Value = serde_json::from_str(texte).unwrap_or_default();
            json.pointer(pointeur).and_then(serde_json::Value::as_str).map(str::to_string)
        }
        Lecture::Modele(modele) => remplir_modele(modele, texte),
    };
    let est_un_lien = |l: &str| (l.starts_with("https://") || l.starts_with("http://")) && !l.contains(char::is_whitespace);
    match lien {
        Some(lien) if (200..300).contains(&statut) && est_un_lien(&lien) => Ok(lien),
        _ => {
            // Un extrait court : une page d'erreur HTML entière n'a rien à faire
            // dans un message.
            let extrait: String = texte.chars().take(160).collect();
            Err(format!("{nom} a refusé l'envoi (HTTP {statut}) : {extrait}"))
        }
    }
}

/// Un handle WinHTTP fermé à la sortie de portée, quel que soit le chemin.
struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn ouvrir(handle: *mut core::ffi::c_void, etape: &str) -> Result<Handle, String> {
    if handle.is_null() {
        Err(format!("{etape} : {}", windows::core::Error::from_thread()))
    } else {
        Ok(Handle(handle))
    }
}

/// Envoie `corps` en POST (HTTPS, ou HTTP pour un serveur personnalisé qui le
/// demande) et rend `(statut, réponse)`. BLOQUANT : appelé
/// depuis le fil de [`lancer`], jamais depuis le fil de l'interface.
fn poster(requete: &Requete, type_contenu: &str, corps: &[u8]) -> Result<(u32, String), String> {
    let agent = HSTRING::from(format!("NeuraDesk/{}", env!("CARGO_PKG_VERSION")));
    unsafe {
        let session = ouvrir(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0), "session")?;
        let _ = WinHttpSetTimeouts(session.0, DELAI_RESEAU_MS, DELAI_RESEAU_MS, DELAI_RESEAU_MS, DELAI_RESEAU_MS);
        let connexion = ouvrir(WinHttpConnect(session.0, &HSTRING::from(requete.hote.as_str()), requete.port, 0), "connexion")?;
        let drapeaux = if requete.securisee { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        let demande = ouvrir(
            WinHttpOpenRequest(connexion.0, &HSTRING::from("POST"), &HSTRING::from(requete.chemin.as_str()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), drapeaux),
            "requête",
        )?;
        let entetes: Vec<u16> = requete
            .entetes
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(format!("Content-Type: {type_contenu}").as_str()))
            .collect::<Vec<_>>()
            .join("\r\n")
            .encode_utf16()
            .collect();
        let longueur = u32::try_from(corps.len()).map_err(|_| "image trop lourde".to_string())?;
        WinHttpSendRequest(demande.0, Some(&entetes), Some(corps.as_ptr().cast()), longueur, longueur, 0).map_err(|e| format!("envoi : {e}"))?;
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

        let mut reponse = Vec::new();
        loop {
            let mut disponible = 0u32;
            WinHttpQueryDataAvailable(demande.0, &mut disponible).map_err(|e| format!("lecture : {e}"))?;
            if disponible == 0 {
                break;
            }
            let debut = reponse.len();
            reponse.resize(debut + disponible as usize, 0);
            let mut lus = 0u32;
            WinHttpReadData(demande.0, reponse[debut..].as_mut_ptr().cast(), disponible, &mut lus).map_err(|e| format!("lecture : {e}"))?;
            reponse.truncate(debut + lus as usize);
        }
        Ok((statut, String::from_utf8_lossy(&reponse).into_owned()))
    }
}

/// Lance l'envoi de `png` vers le service choisi, sur un fil à part (WinHTTP est
/// bloquant) : le résultat — le lien ou l'erreur — arrive par le `Receiver`.
/// `Err` immédiat si la requête ne peut même pas être formée (clé manquante,
/// serveur personnalisé sans adresse). Le nom rendu est celui à montrer : pour
/// un serveur personnalisé, son hôte.
pub fn lancer(png: Vec<u8>) -> Result<(String, Receiver<Result<String, String>>), String> {
    let reglages = charger_reglages();
    let requete = requete(&reglages)?;
    let nom = reglages.nom_affiche();
    let nom_fil = nom.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let frontiere = format!("----NeuraDesk{millis:x}");
        let corps = corps_multipart(&frontiere, &requete.champs, &requete.champ_fichier, &format!("image-{millis}.png"), &png);
        let resultat = poster(&requete, &format!("multipart/form-data; boundary={frontiere}"), &corps)
            .and_then(|(statut, reponse)| lire_reponse(&nom_fil, &requete.lecture, statut, &reponse));
        let _ = tx.send(resultat);
    });
    Ok((nom, rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reglages(hebergeur: Hebergeur, cle: Option<&str>) -> Reglages {
        let mut r = Reglages { hebergeur, ..Reglages::default() };
        if let Some(cle) = cle {
            r.cles.insert(hebergeur.as_str().into(), cle.into());
        }
        r
    }

    #[test]
    fn chaque_hebergeur_relit_son_propre_nom() {
        for h in Hebergeur::ALL {
            assert_eq!(Hebergeur::from_config(h.as_str()), Some(h));
        }
        assert_eq!(Hebergeur::from_config("inconnu"), None);
    }

    #[test]
    fn les_reglages_survivent_a_un_aller_retour_toml() {
        let mut r = reglages(Hebergeur::ImgBB, Some("abc"));
        r.duree = "72h".into();
        r.cles.insert("imgur".into(), "xyz".into());
        assert_eq!(Reglages::depuis_toml(&r.vers_toml()), r);
    }

    #[test]
    fn un_fichier_absent_ou_faux_donne_litterbox_24h() {
        assert_eq!(Reglages::depuis_toml(""), Reglages::default());
        let r = Reglages::depuis_toml("service = \"nimporte\"\nduree = \"5h\"");
        assert_eq!((r.hebergeur, r.duree.as_str()), (Hebergeur::Litterbox, "24h"));
    }

    #[test]
    fn un_service_a_cle_refuse_de_partir_sans_elle() {
        // `Perso` n'a pas de clé mais une ADRESSE (voir `les_adresses_invalides_sont_refusees`).
        for h in Hebergeur::ALL.into_iter().filter(|h| *h != Hebergeur::Perso) {
            let sans = requete(&reglages(h, None));
            assert_eq!(sans.is_err(), h.cle().is_some(), "{h:?}");
            let vide = requete(&reglages(h, Some("")));
            assert_eq!(vide.is_err(), h.cle().is_some(), "{h:?} (clé vide)");
        }
    }

    #[test]
    fn la_cle_est_encodee_dans_l_url_mais_pas_dans_l_en_tete() {
        let bb = requete(&reglages(Hebergeur::ImgBB, Some("a b&c"))).unwrap();
        assert_eq!(bb.chemin, "/1/upload?key=a%20b%26c");
        let imgur = requete(&reglages(Hebergeur::Imgur, Some("id123"))).unwrap();
        assert_eq!(imgur.entetes, vec!["Authorization: Client-ID id123".to_string()]);
    }

    #[test]
    fn litterbox_porte_la_duree_choisie() {
        let mut r = reglages(Hebergeur::Litterbox, None);
        r.duree = "1h".into();
        let q = requete(&r).unwrap();
        assert!(q.champs.contains(&("time".to_string(), "1h".to_string())));
        assert_eq!(q.champ_fichier, "fileToUpload");
    }

    #[test]
    fn le_corps_multipart_encadre_champs_et_fichier() {
        let corps = corps_multipart("F", &[("reqtype".into(), "fileupload".into())], "fileToUpload", "a.png", &[1, 2, 3]);
        let attendu = b"--F\r\nContent-Disposition: form-data; name=\"reqtype\"\r\n\r\nfileupload\r\n\
--F\r\nContent-Disposition: form-data; name=\"fileToUpload\"; filename=\"a.png\"\r\nContent-Type: image/png\r\n\r\n\x01\x02\x03\r\n--F--\r\n";
        assert_eq!(corps, attendu.to_vec());
    }

    fn lire(h: Hebergeur, statut: u32, corps: &str) -> Result<String, String> {
        let r = requete(&reglages(h, Some("k"))).unwrap();
        lire_reponse(h.label(), &r.lecture, statut, corps)
    }

    #[test]
    fn les_reponses_texte_et_json_donnent_le_lien() {
        assert_eq!(lire(Hebergeur::Catbox, 200, "https://files.catbox.moe/abc.png"), Ok("https://files.catbox.moe/abc.png".into()));
        assert_eq!(lire(Hebergeur::ZeroXZero, 200, "https://0x0.st/s/x.png\n"), Ok("https://0x0.st/s/x.png".into()));
        assert_eq!(lire(Hebergeur::ImgBB, 200, r#"{"data":{"url":"https://i.ibb.co/a/b.png"},"success":true}"#), Ok("https://i.ibb.co/a/b.png".into()));
        assert_eq!(lire(Hebergeur::Freeimage, 200, r#"{"status_code":200,"image":{"url":"https://iili.io/x.png"}}"#), Ok("https://iili.io/x.png".into()));
        assert_eq!(lire(Hebergeur::Imgur, 200, r#"{"data":{"link":"https://i.imgur.com/x.png"},"success":true}"#), Ok("https://i.imgur.com/x.png".into()));
    }

    #[test]
    fn une_erreur_du_service_n_est_jamais_prise_pour_un_lien() {
        assert!(lire(Hebergeur::Catbox, 412, "https://x").is_err());
        assert!(lire(Hebergeur::Litterbox, 200, "No file uploaded").is_err());
        assert!(lire(Hebergeur::ImgBB, 400, r#"{"error":{"message":"Invalid API v1 key."}}"#).is_err());
        let longue = "x".repeat(1000);
        assert!(lire(Hebergeur::Catbox, 500, &longue).unwrap_err().len() < 300);
    }

    /// Le `.sxcu` que Zipline exporte (forme relevée dans sa documentation).
    const SXCU_ZIPLINE: &str = r#"{
  "Version": "17.0.0",
  "Name": "Zipline - img.exemple.fr",
  "DestinationType": "ImageUploader, TextUploader, FileUploader",
  "RequestMethod": "POST",
  "RequestURL": "https://img.exemple.fr/api/upload",
  "Headers": { "authorization": "jeton-secret", "x-zipline-format": "random" },
  "URL": "{json:files[0].url}",
  "Body": "MultipartFormData",
  "FileFormName": "file"
}"#;

    #[test]
    fn un_sxcu_de_zipline_donne_un_serveur_complet() {
        let (perso, nom) = ServeurPerso::depuis_sxcu(SXCU_ZIPLINE).unwrap();
        assert_eq!(nom, "Zipline - img.exemple.fr");
        let r = Reglages { hebergeur: Hebergeur::Perso, perso, ..Reglages::default() };
        let q = requete(&r).unwrap();
        assert_eq!((q.hote.as_str(), q.port, q.securisee, q.chemin.as_str()), ("img.exemple.fr", 443, true, "/api/upload"));
        assert_eq!(q.entetes, vec!["authorization: jeton-secret".to_string(), "x-zipline-format: random".to_string()]);
        assert_eq!(q.champ_fichier, "file");
        let reponse = r#"{"files":[{"id":"1","url":"https://img.exemple.fr/u/abc.png"}]}"#;
        assert_eq!(lire_reponse(&q.hote, &q.lecture, 200, reponse), Ok("https://img.exemple.fr/u/abc.png".into()));
        // Et il survit à l'enregistrement.
        assert_eq!(Reglages::depuis_toml(&r.vers_toml()), r);
    }

    #[test]
    fn un_sxcu_qui_n_est_pas_un_post_multipart_est_refuse() {
        assert!(ServeurPerso::depuis_sxcu(r#"{"RequestMethod":"PUT","RequestURL":"https://a.b/"}"#).is_err());
        assert!(ServeurPerso::depuis_sxcu(r#"{"Body":"Binary","RequestURL":"https://a.b/"}"#).is_err());
        assert!(ServeurPerso::depuis_sxcu(r#"{"RequestURL":"ftp://a.b/"}"#).is_err());
        assert!(ServeurPerso::depuis_sxcu("pas du json").is_err());
        assert!(ServeurPerso::depuis_sxcu(r#"{"RequestURL":"https://a.b/","Headers":{"X":"a\r\nHost: pirate"}}"#).is_err());
    }

    #[test]
    fn les_parametres_de_l_url_sont_ranges_a_part_puis_reencodes() {
        let mut perso = ServeurPerso::default();
        perso.changer_url("http://192.168.1.10:8080/upload.php?key=a%20b&x=1").unwrap();
        assert_eq!(perso.url, "http://192.168.1.10:8080/upload.php");
        assert_eq!(perso.parametres.get("key").map(String::as_str), Some("a b"));
        let q = requete_perso(&perso).unwrap();
        assert_eq!((q.port, q.securisee, q.chemin.as_str()), (8080, false, "/upload.php?key=a%20b&x=1"));
    }

    #[test]
    fn les_adresses_invalides_sont_refusees() {
        for url in ["", "exemple.fr/upload", "https://", "https://ex mple.fr/", "https://a.b:99999/", "https://a.b/x y"] {
            assert!(decouper_url(url).is_err(), "{url}");
        }
        assert!(requete_perso(&ServeurPerso::default()).is_err());
    }

    #[test]
    fn le_lien_se_lit_en_texte_en_chemin_json_ou_en_modele() {
        let corps = r#"{"data":{"link":"https://h/x.png","id":"x"},"files":[{"url":"https://h/f.png"}]}"#;
        let lit = |lien: &str, corps: &str| lire_reponse("h", &lecture_perso(lien), 200, corps);
        assert_eq!(lit("", "https://h/brut.png\n"), Ok("https://h/brut.png".into()));
        assert_eq!(lit("data.link", corps), Ok("https://h/x.png".into()));
        assert_eq!(lit("/data/link", corps), Ok("https://h/x.png".into()));
        assert_eq!(lit("files[0].url", corps), Ok("https://h/f.png".into()));
        assert_eq!(lit("$json:data.link$", corps), Ok("https://h/x.png".into()));
        assert_eq!(lit("https://h/i/{json:data.id}.png", corps), Ok("https://h/i/x.png".into()));
        assert_eq!(lit("{response}", "https://h/r.png"), Ok("https://h/r.png".into()));
        assert!(lit("{json:data.absent}", corps).is_err());
    }

    #[test]
    fn les_lignes_tapees_dans_les_reglages_se_lisent_ou_se_refusent() {
        let t = lire_lignes("Authorization: Bearer x:y\n\n  X-A :  b ", ':').unwrap();
        assert_eq!(t.get("Authorization").map(String::as_str), Some("Bearer x:y"));
        assert_eq!(t.get("X-A").map(String::as_str), Some("b"));
        assert!(lire_lignes("sans separateur", '=').is_err());
        assert_eq!(lire_lignes("", '=').unwrap().len(), 0);
    }
}
