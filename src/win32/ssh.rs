//! Client SSH / SFTP de l'applet « Serveurs » (voir `docs/plans/ssh-sftp.md`),
//! sur le modèle de Submarine : `russh` + `russh-sftp` sur un runtime `tokio`.
//!
//! **RIEN ICI NE TOURNE SUR LE FIL UI.** Le runtime a ses propres fils ; le fil
//! UI ne fait que déposer des commandes (`connecter`, `ecrire`, …) et vider les
//! évènements (`prendre_evenements`) à chaque frame. Chaque connexion est une
//! tâche qui possède sa session et son canal ; on lui parle par une file.
//!
//! Un ONGLET de la page = une connexion = un numéro choisi par la page.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use russh::client;
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKey};
use russh::{ChannelMsg, Disconnect};
use russh_sftp::client::SftpSession;
use tokio::sync::{mpsc, oneshot};

/// Au-delà, une connexion qui ne répond pas est abandonnée.
const DELAI_CONNEXION: Duration = Duration::from_secs(15);
/// Sans réponse de l'utilisateur à « faire confiance à cette clé ? », on refuse.
const DELAI_CONFIRMATION: Duration = Duration::from_secs(120);
/// Paquet de maintien envoyé après ce silence — les box et pare-feu coupent
/// les connexions TCP muettes.
const MAINTIEN: Duration = Duration::from_secs(30);
/// Taille des lectures d'un transfert ; `russh-sftp` les découpe lui-même en
/// paquets à la taille que le serveur accepte.
const MORCEAU: usize = 256 * 1024;
const CADENCE_PROGRES: Duration = Duration::from_millis(250);

#[derive(Clone)]
pub enum Authentification {
    MotDePasse(String),
    Cle { chemin: PathBuf, phrase: Option<String> },
}

/// ⛔ **JAMAIS LE SECRET DANS UN `{:?}`** — un `diag!` de la `Cible` l'écrirait
/// en clair dans le journal.
impl std::fmt::Debug for Authentification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MotDePasse(_) => f.write_str("MotDePasse(<masqué>)"),
            Self::Cle { chemin, phrase } => f
                .debug_struct("Cle")
                .field("chemin", chemin)
                .field("phrase", &phrase.as_ref().map(|_| "<masquée>"))
                .finish(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Cible {
    pub hote: String,
    pub port: u16,
    pub utilisateur: String,
    pub authentification: Authentification,
    pub colonnes: u32,
    pub lignes: u32,
    /// Fichier des clés d'hôte acceptées PAR NOUS (écrit à chaque confirmation).
    pub hotes_connus: PathBuf,
    /// Fichier OpenSSH de l'utilisateur, lu SEULEMENT (`~/.ssh/known_hosts`).
    pub hotes_connus_openssh: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EntreeSftp {
    pub nom: String,
    pub dossier: bool,
    pub lien: bool,
    pub taille: u64,
    /// Secondes depuis 1970 (0 si inconnu).
    pub modifie: u32,
    pub droits: u32,
}

/// Les chemins DISTANTS sont absolus (ceux que rend `Lister`) ; un NOM est un
/// seul élément, saisi par l'utilisateur, vérifié par [`nom_distant`].
#[derive(Clone, Debug)]
pub enum RequeteSftp {
    Lister { chemin: String },
    /// Fichiers ET dossiers (récursivement) vers `dossier_local` — ⛔ n'écrase
    /// jamais un fichier local : un nom pris devient « nom (1).ext ».
    Telecharger { distants: Vec<String>, dossier_local: PathBuf },
    /// ⛔ N'écrase jamais un fichier distant (`EXCLUDE`) : refusé s'il existe.
    Televerser { locaux: Vec<PathBuf>, dossier_distant: String },
    CreerDossier { dossier: String, nom: String },
    Renommer { chemin: String, nom: String },
    /// Récursif ; un lien symbolique est supprimé, JAMAIS suivi.
    Supprimer { chemins: Vec<String> },
    Droits { chemin: String, mode: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ReponseSftp {
    Liste { chemin: String, entrees: Vec<EntreeSftp> },
    /// Une opération est terminée — la page relit le dossier affiché, même
    /// quand une partie a échoué (`erreurs`, une ligne par élément).
    Fait { message: String, erreurs: Vec<String> },
    /// Un transfert avance — au plus une fois par [`CADENCE_PROGRES`].
    Progres { nom: String, fait: u64, total: u64 },
    Erreur { message: String },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Etat {
    Connexion,
    Authentification,
    Ouvert,
    Ferme,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Evenement {
    Etat { onglet: u32, etat: Etat },
    /// La connexion a échoué ou s'est terminée sur une erreur (message en français).
    Erreur { onglet: u32, message: String },
    Sortie { onglet: u32, octets: Vec<u8> },
    /// Clé d'hôte jamais vue : la page doit répondre par `confirmer_cle`.
    CleInconnue { onglet: u32, algorithme: String, empreinte: String },
    Sftp { onglet: u32, reponse: ReponseSftp },
    /// Le volet « ce PC » de la vue Fichiers (voir [`local`]).
    Local { onglet: u32, reponse: Result<local::ListeLocale, String> },
}

enum Commande {
    Ecrire(Vec<u8>),
    Redimensionner(u32, u32),
    Sftp(RequeteSftp),
    Fermer,
}

static EVENEMENTS: Mutex<Vec<Evenement>> = Mutex::new(Vec::new());
static SESSIONS: Mutex<Option<HashMap<u32, mpsc::UnboundedSender<Commande>>>> = Mutex::new(None);
static CONFIRMATIONS: Mutex<Option<HashMap<u32, oneshot::Sender<bool>>>> = Mutex::new(None);

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("ssh")
            .enable_all()
            .build()
            .expect("runtime tokio de l'applet SSH")
    })
}

fn emettre(evenement: Evenement) {
    if let Ok(mut file) = EVENEMENTS.lock() {
        file.push(evenement);
    }
}

/// Vide la file des évènements (fil UI, chaque frame).
pub fn prendre_evenements() -> Vec<Evenement> {
    EVENEMENTS.lock().map(|mut file| std::mem::take(&mut *file)).unwrap_or_default()
}

fn envoyer(onglet: u32, commande: Commande) -> bool {
    let Ok(sessions) = SESSIONS.lock() else { return false };
    sessions
        .as_ref()
        .and_then(|table| table.get(&onglet))
        .is_some_and(|file| file.send(commande).is_ok())
}

/// Ouvre la connexion `onglet`. Une connexion déjà ouverte sous ce numéro est
/// fermée d'abord (reconnexion).
pub fn connecter(onglet: u32, cible: Cible) {
    fermer(onglet);
    let (envoi, reception) = mpsc::unbounded_channel();
    if let Ok(mut sessions) = SESSIONS.lock() {
        sessions.get_or_insert_with(HashMap::new).insert(onglet, envoi.clone());
    }
    runtime().spawn(async move {
        let resultat = mener(onglet, cible, reception).await;
        // Ne retirer QUE notre propre file : une reconnexion a pu la remplacer.
        if let Ok(mut sessions) = SESSIONS.lock() {
            if let Some(table) = sessions.as_mut() {
                if table.get(&onglet).is_some_and(|file| file.same_channel(&envoi)) {
                    table.remove(&onglet);
                }
            }
        }
        if let Err(message) = resultat {
            emettre(Evenement::Erreur { onglet, message });
        }
        emettre(Evenement::Etat { onglet, etat: Etat::Ferme });
    });
}

pub fn ecrire(onglet: u32, octets: Vec<u8>) -> bool {
    envoyer(onglet, Commande::Ecrire(octets))
}

pub fn redimensionner(onglet: u32, colonnes: u32, lignes: u32) -> bool {
    envoyer(onglet, Commande::Redimensionner(colonnes.max(1), lignes.max(1)))
}

pub fn sftp(onglet: u32, requete: RequeteSftp) -> bool {
    envoyer(onglet, Commande::Sftp(requete))
}

/// Liste un dossier de CE PC hors du fil UI (un lecteur réseau peut être lent).
/// Pas besoin d'être connecté : l'onglet ne sert qu'à router la réponse.
pub fn lister_local(onglet: u32, chemin: String) {
    runtime().spawn_blocking(move || emettre(Evenement::Local { onglet, reponse: local::lister(&chemin) }));
}

pub fn fermer(onglet: u32) {
    envoyer(onglet, Commande::Fermer);
    if let Ok(mut sessions) = SESSIONS.lock() {
        if let Some(table) = sessions.as_mut() {
            table.remove(&onglet);
        }
    }
    // Une confirmation en attente ne sera plus jamais donnée.
    confirmer_cle(onglet, false);
}

/// Réponse de l'utilisateur à `Evenement::CleInconnue`.
pub fn confirmer_cle(onglet: u32, accepter: bool) {
    let attente = CONFIRMATIONS.lock().ok().and_then(|mut table| table.as_mut()?.remove(&onglet));
    if let Some(attente) = attente {
        let _ = attente.send(accepter);
    }
}

pub fn empreinte(cle: &PublicKey) -> String {
    cle.fingerprint(HashAlg::Sha256).to_string()
}

/// Verdict des fichiers de clés d'hôte.
#[derive(Debug, PartialEq)]
enum Verdict {
    Connue,
    Inconnue,
    /// ⛔ Une clé DIFFÉRENTE est enregistrée pour cet hôte : peut-être une
    /// attaque de l'homme du milieu. On refuse, sans proposer d'accepter.
    Changee,
}

fn verdict_fichier(hote: &str, port: u16, cle: &PublicKey, chemin: &std::path::Path) -> Verdict {
    match russh::keys::check_known_hosts_path(hote, port, cle, chemin) {
        Ok(true) => Verdict::Connue,
        Err(russh::keys::Error::KeyChanged { .. }) => Verdict::Changee,
        // Fichier absent, ou ligne d'un type de clé que `russh` ne sait pas
        // lire (le fichier d'OpenSSH peut en contenir) : rien de CONNU ici.
        _ => Verdict::Inconnue,
    }
}

struct Client {
    onglet: u32,
    hote: String,
    port: u16,
    hotes_connus: PathBuf,
    hotes_connus_openssh: Option<PathBuf>,
}

impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        cle: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let cle = cle.public_key();
        let mut fichiers = vec![self.hotes_connus.clone()];
        fichiers.extend(self.hotes_connus_openssh.clone());
        let mut connue = false;
        for fichier in &fichiers {
            match verdict_fichier(&self.hote, self.port, &cle, fichier) {
                Verdict::Connue => connue = true,
                Verdict::Changee => {
                    emettre(Evenement::Erreur {
                        onglet: self.onglet,
                        message: format!(
                            "La clé de {} a CHANGÉ depuis la dernière connexion (nouvelle empreinte {}). \
                             Connexion refusée : ce peut être une attaque. Si le serveur a été réinstallé, \
                             retirez l'ancienne clé de {}.",
                            self.hote,
                            empreinte(&cle),
                            fichier.display()
                        ),
                    });
                    return Ok(false);
                }
                Verdict::Inconnue => {}
            }
        }
        if connue {
            return Ok(true);
        }

        let (envoi, reception) = oneshot::channel();
        if let Ok(mut table) = CONFIRMATIONS.lock() {
            table.get_or_insert_with(HashMap::new).insert(self.onglet, envoi);
        }
        emettre(Evenement::CleInconnue {
            onglet: self.onglet,
            algorithme: cle.algorithm().to_string(),
            empreinte: empreinte(&cle),
        });
        let accepte = matches!(tokio::time::timeout(DELAI_CONFIRMATION, reception).await, Ok(Ok(true)));
        if !accepte {
            emettre(Evenement::Erreur {
                onglet: self.onglet,
                message: "Clé du serveur refusée.".to_string(),
            });
            return Ok(false);
        }
        if let Err(erreur) = russh::keys::known_hosts::learn_known_hosts_path(&self.hote, self.port, &cle, &self.hotes_connus) {
            // La connexion continue ; seule la mémorisation a échoué.
            emettre(Evenement::Erreur {
                onglet: self.onglet,
                message: format!("Clé acceptée mais non mémorisée : {erreur}"),
            });
        }
        Ok(true)
    }
}

fn texte_erreur(erreur: russh::Error) -> String {
    match erreur {
        russh::Error::UnknownKey => "Clé du serveur refusée.".to_string(),
        russh::Error::ConnectionTimeout | russh::Error::InactivityTimeout => "Le serveur ne répond plus.".to_string(),
        russh::Error::IO(e) => format!("Connexion impossible : {e}"),
        russh::Error::Disconnect => "Le serveur a fermé la connexion.".to_string(),
        autre => format!("Erreur SSH : {autre}"),
    }
}

async fn authentifier(session: &mut client::Handle<Client>, cible: &Cible) -> Result<(), String> {
    let utilisateur = cible.utilisateur.clone();
    let reussi = match &cible.authentification {
        Authentification::MotDePasse(mot_de_passe) => {
            let reponse = session
                .authenticate_password(utilisateur.clone(), mot_de_passe.clone())
                .await
                .map_err(texte_erreur)?;
            if reponse.success() {
                true
            } else {
                // Beaucoup de serveurs refusent « password » mais acceptent le
                // même secret par « keyboard-interactive » (PAM).
                clavier_interactif(session, &utilisateur, mot_de_passe).await?
            }
        }
        Authentification::Cle { chemin, phrase } => {
            let cle = russh::keys::load_secret_key(chemin, phrase.as_deref()).map_err(|erreur| match erreur {
                russh::keys::Error::KeyIsEncrypted => "La clé est protégée : indiquez sa phrase de passe.".to_string(),
                autre => format!("Clé illisible ({}) : {autre}", chemin.display()),
            })?;
            let hachage = session.best_supported_rsa_hash().await.map_err(texte_erreur)?.flatten();
            session
                .authenticate_publickey(utilisateur, PrivateKeyWithHashAlg::new(Arc::new(cle), hachage))
                .await
                .map_err(texte_erreur)?
                .success()
        }
    };
    if reussi {
        Ok(())
    } else {
        Err("Authentification refusée.".to_string())
    }
}

async fn clavier_interactif(
    session: &mut client::Handle<Client>,
    utilisateur: &str,
    mot_de_passe: &str,
) -> Result<bool, String> {
    use russh::client::KeyboardInteractiveAuthResponse as Reponse;
    let mut reponse = session
        .authenticate_keyboard_interactive_start(utilisateur.to_string(), None::<String>)
        .await
        .map_err(texte_erreur)?;
    // Borné : un serveur qui redemande sans fin n'accepte pas ce secret.
    for _ in 0..4 {
        match reponse {
            Reponse::Success => return Ok(true),
            Reponse::Failure { .. } => return Ok(false),
            Reponse::InfoRequest { prompts, .. } => {
                let reponses = prompts.iter().map(|_| mot_de_passe.to_string()).collect();
                reponse = session
                    .authenticate_keyboard_interactive_respond(reponses)
                    .await
                    .map_err(texte_erreur)?;
            }
        }
    }
    Ok(false)
}

async fn mener(onglet: u32, cible: Cible, mut commandes: mpsc::UnboundedReceiver<Commande>) -> Result<(), String> {
    emettre(Evenement::Etat { onglet, etat: Etat::Connexion });
    let config = Arc::new(client::Config {
        keepalive_interval: Some(MAINTIEN),
        keepalive_max: 3,
        ..Default::default()
    });
    let client = Client {
        onglet,
        hote: cible.hote.clone(),
        port: cible.port,
        hotes_connus: cible.hotes_connus.clone(),
        hotes_connus_openssh: cible.hotes_connus_openssh.clone(),
    };
    // Le délai ne couvre PAS l'attente d'une confirmation de clé : elle a le sien.
    let connexion = client::connect(config, (cible.hote.as_str(), cible.port), client);
    let mut session = match tokio::time::timeout(DELAI_CONNEXION + DELAI_CONFIRMATION, connexion).await {
        Ok(resultat) => resultat.map_err(texte_erreur)?,
        Err(_) => return Err(format!("{}:{} ne répond pas.", cible.hote, cible.port)),
    };

    emettre(Evenement::Etat { onglet, etat: Etat::Authentification });
    authentifier(&mut session, &cible).await?;

    let mut canal = session.channel_open_session().await.map_err(texte_erreur)?;
    canal
        .request_pty(false, "xterm-256color", cible.colonnes, cible.lignes, 0, 0, &[])
        .await
        .map_err(texte_erreur)?;
    canal.request_shell(false).await.map_err(texte_erreur)?;
    emettre(Evenement::Etat { onglet, etat: Etat::Ouvert });

    let mut sftp: Option<Arc<SftpSession>> = None;
    loop {
        tokio::select! {
            commande = commandes.recv() => match commande {
                None | Some(Commande::Fermer) => break,
                Some(Commande::Ecrire(octets)) => canal.data(&octets[..]).await.map_err(texte_erreur)?,
                Some(Commande::Redimensionner(colonnes, lignes)) => {
                    canal.window_change(colonnes, lignes, 0, 0).await.map_err(texte_erreur)?
                }
                Some(Commande::Sftp(requete)) => {
                    if sftp.is_none() {
                        match ouvrir_sftp(&session).await {
                            Ok(ouverte) => sftp = Some(Arc::new(ouverte)),
                            Err(message) => {
                                emettre(Evenement::Sftp { onglet, reponse: ReponseSftp::Erreur { message } });
                                continue;
                            }
                        }
                    }
                    if let Some(sftp) = sftp.clone() {
                        // Une tâche par requête : un gros listing ne gèle pas le terminal.
                        tokio::spawn(async move {
                            let reponse = traiter_sftp(&sftp, onglet, requete).await;
                            emettre(Evenement::Sftp { onglet, reponse });
                        });
                    }
                }
            },
            message = canal.wait() => match message {
                Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                    emettre(Evenement::Sortie { onglet, octets: data.to_vec() });
                }
                Some(ChannelMsg::ExitStatus { .. }) | Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                Some(_) => {}
            },
        }
    }
    if let Some(sftp) = sftp {
        let _ = sftp.close().await;
    }
    let _ = session.disconnect(Disconnect::ByApplication, "", "fr").await;
    Ok(())
}

async fn ouvrir_sftp(session: &client::Handle<Client>) -> Result<SftpSession, String> {
    let canal = session.channel_open_session().await.map_err(texte_erreur)?;
    canal.request_subsystem(true, "sftp").await.map_err(texte_erreur)?;
    SftpSession::new(canal.into_stream())
        .await
        .map_err(|erreur| format!("SFTP indisponible sur ce serveur : {erreur}"))
}

mod fichiers;
use fichiers::traiter_sftp;
pub mod local;

#[cfg(test)]
mod tests;
