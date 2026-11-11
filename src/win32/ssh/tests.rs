//! Banc d'essai : un serveur `russh` EN PROCESSUS sur `127.0.0.1:0` — shell qui
//! renvoie ce qu'il reçoit, sous-système SFTP à contenu fixe. **Aucun essai ne
//! touche un vrai serveur.**
//!
//! Les clés ci-dessous sont des clés JETABLES générées pour ces tests
//! (`ssh-keygen -t ed25519`), sans aucun autre usage.

use super::*;
use russh::server::{Auth, ChannelOpenHandle, Msg, Session, VerifiedPublicKey};
use russh::{Channel, ChannelId};
use russh_sftp::protocol::{Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version};
use std::time::Instant;

const CLE_HOTE: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACBTbTZVs72l8oDnhDfIG+9F5eCAlWrp8BVNmGEEYZvVJAAAAJDpYbll6WG5
ZQAAAAtzc2gtZWQyNTUxOQAAACBTbTZVs72l8oDnhDfIG+9F5eCAlWrp8BVNmGEEYZvVJA
AAAEAc6TJ7RKOtQ4YG6Lr8mrrwf4+uhxaCwpWS+TTKyvYuzlNtNlWzvaXygOeEN8gb70Xl
4ICVaunwFU2YYQRhm9UkAAAACXRlc3QtaG90ZQECAwQ=
-----END OPENSSH PRIVATE KEY-----
";

/// Chiffrée, phrase de passe « secret ».
const CLE_CLIENT: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAACmFlczI1Ni1jdHIAAAAGYmNyeXB0AAAAGAAAABAjyMDVsC
6sWTqo7XPPKW/KAAAAGAAAAAEAAAAzAAAAC3NzaC1lZDI1NTE5AAAAILASZjiDjGPUKP4i
SLHisZC5XUphpKOree62oMihpEv0AAAAkLu6w0TtLCaFOeURJ7lJDewNjizkQH9xFSoMyj
9qx09OwtLTMjwHPMbEB1mpuu04NgBY5zGJSzQxeJPPALkHfv5GQ+GDzli0ROlHmZEqsDKG
1RKXVxsMofs4/Tcnj53XaX3ysoHOrbdyKpxsnHjNgz6IL2OKqHlm9NHiX1dAA8lsy9i4pn
nfecT3oZjkzyBorA==
-----END OPENSSH PRIVATE KEY-----
";
const CLE_CLIENT_PUBLIQUE: &str = "AAAAC3NzaC1lZDI1NTE5AAAAILASZjiDjGPUKP4iSLHisZC5XUphpKOree62oMihpEv0";

const UTILISATEUR: &str = "moi";
const MOT_DE_PASSE: &str = "sésame";

#[derive(Default)]
struct Serveur {
    canaux: HashMap<ChannelId, Channel<Msg>>,
    /// Seuls les canaux qui ont demandé un shell font l'écho — le canal SFTP
    /// renverrait sinon ses propres paquets au client.
    shells: Vec<ChannelId>,
    /// Le SFTP sert ce dossier (`ServeurDisque`) plutôt que le contenu fixe.
    disque: Option<PathBuf>,
}

impl russh::server::Handler for Serveur {
    type Error = russh::Error;

    async fn auth_password(&mut self, utilisateur: &str, mot_de_passe: &str) -> Result<Auth, Self::Error> {
        Ok(if utilisateur == UTILISATEUR && mot_de_passe == MOT_DE_PASSE { Auth::Accept } else { Auth::reject() })
    }

    /// ⚠️ Depuis russh 0.65 la clé arrive **VÉRIFIÉE** (signature contrôlée et
    /// cohérente avec l'algorithme annoncé, cf. l'avis GHSA-j7h2-66j4-ghjf) :
    /// elle est dans `VerifiedPublicKey::public_key`, plus seule.
    async fn auth_publickey(&mut self, utilisateur: &str, cle: VerifiedPublicKey<'_>) -> Result<Auth, Self::Error> {
        let attendue = russh::keys::parse_public_key_base64(CLE_CLIENT_PUBLIQUE).unwrap();
        Ok(if utilisateur == UTILISATEUR && *cle.public_key == attendue { Auth::Accept } else { Auth::reject() })
    }

    async fn channel_open_session(
        &mut self,
        canal: Channel<Msg>,
        reponse: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.canaux.insert(canal.id(), canal);
        reponse.accept().await;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        canal: ChannelId,
        _terminal: &str,
        _colonnes: u32,
        _lignes: u32,
        _px: u32,
        _py: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(canal)
    }

    async fn shell_request(&mut self, canal: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        self.shells.push(canal);
        session.channel_success(canal)
    }

    /// Le « shell » : renvoie tout, et se ferme sur `exit`.
    async fn data(&mut self, canal: ChannelId, octets: &[u8], session: &mut Session) -> Result<(), Self::Error> {
        if !self.shells.contains(&canal) {
            return Ok(());
        }
        if octets == b"exit\r" {
            session.exit_status_request(canal, 0)?;
            return session.close(canal);
        }
        session.data(canal, octets.to_vec())
    }

    async fn window_change_request(
        &mut self,
        canal: ChannelId,
        colonnes: u32,
        lignes: u32,
        _px: u32,
        _py: u32,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.data(canal, format!("taille {colonnes}x{lignes}").into_bytes())
    }

    async fn subsystem_request(&mut self, canal: ChannelId, nom: &str, session: &mut Session) -> Result<(), Self::Error> {
        match (nom, self.canaux.remove(&canal)) {
            ("sftp", Some(ouvert)) => {
                session.channel_success(canal)?;
                match self.disque.clone() {
                    Some(racine) => tokio::spawn(russh_sftp::server::run(ouvert.into_stream(), ServeurDisque::new(racine))),
                    None => tokio::spawn(russh_sftp::server::run(ouvert.into_stream(), ServeurSftp::default())),
                };
                Ok(())
            }
            _ => session.channel_failure(canal),
        }
    }
}

#[derive(Default)]
struct ServeurSftp {
    lu: bool,
}

fn attributs(dossier: bool, taille: u64) -> FileAttributes {
    let mut attributs = FileAttributes::empty();
    attributs.size = Some(taille);
    attributs.permissions = Some(if dossier { 0o040755 } else { 0o100644 });
    attributs.mtime = Some(1_700_000_000);
    attributs
}

impl russh_sftp::server::Handler for ServeurSftp {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(&mut self, _version: u32, _extensions: HashMap<String, String>) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn realpath(&mut self, id: u32, _chemin: String) -> Result<Name, Self::Error> {
        Ok(Name { id, files: vec![File::new("/racine", attributs(true, 0))] })
    }

    async fn opendir(&mut self, id: u32, chemin: String) -> Result<Handle, Self::Error> {
        self.lu = false;
        Ok(Handle { id, handle: chemin })
    }

    async fn readdir(&mut self, id: u32, _poignee: String) -> Result<Name, Self::Error> {
        if std::mem::replace(&mut self.lu, true) {
            return Err(StatusCode::Eof);
        }
        Ok(Name {
            id,
            files: vec![
                File::new(".", attributs(true, 0)),
                File::new("b.txt", attributs(false, 5)),
                File::new("Dossier", attributs(true, 0)),
                File::new("a.txt", attributs(false, 12)),
            ],
        })
    }

    async fn close(&mut self, id: u32, _poignee: String) -> Result<Status, Self::Error> {
        Ok(Status { id, status_code: StatusCode::Ok, error_message: "Ok".into(), language_tag: "fr".into() })
    }
}

/// Un SFTP qui sert un VRAI dossier (`racine` = `/`) — pour éprouver les
/// transferts, la création, le renommage et la suppression de bout en bout.
struct ServeurDisque {
    racine: PathBuf,
    ouverts: HashMap<String, Ouvert>,
    suivant: u32,
    /// Windows ne garde pas les droits Unix : on retient ceux que `setstat` pose.
    droits: HashMap<String, u32>,
}

enum Ouvert {
    Dossier(Option<Vec<File>>),
    Fichier(std::fs::File),
}

impl ServeurDisque {
    fn new(racine: PathBuf) -> Self {
        Self { racine, ouverts: HashMap::new(), suivant: 0, droits: HashMap::new() }
    }

    fn reel(&self, chemin: &str) -> PathBuf {
        self.racine.join(chemin.trim_start_matches('/'))
    }

    fn attributs(&self, chemin: &str) -> Result<FileAttributes, StatusCode> {
        let meta = std::fs::symlink_metadata(self.reel(chemin)).map_err(statut)?;
        let (type_, defaut) = if meta.is_dir() { (0o040000, 0o755) } else { (0o100000, 0o644) };
        let mut attributs = FileAttributes::empty();
        attributs.size = Some(meta.len());
        attributs.permissions = Some(type_ | self.droits.get(chemin).copied().unwrap_or(defaut));
        attributs.mtime = Some(1_700_000_000);
        Ok(attributs)
    }

    fn poignee(&mut self, ouvert: Ouvert) -> String {
        self.suivant += 1;
        let poignee = self.suivant.to_string();
        self.ouverts.insert(poignee.clone(), ouvert);
        poignee
    }
}

fn statut(erreur: std::io::Error) -> StatusCode {
    match erreur.kind() {
        std::io::ErrorKind::NotFound => StatusCode::NoSuchFile,
        std::io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    }
}

fn ok(id: u32) -> Status {
    Status { id, status_code: StatusCode::Ok, error_message: "Ok".into(), language_tag: "fr".into() }
}

impl russh_sftp::server::Handler for ServeurDisque {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(&mut self, _version: u32, _extensions: HashMap<String, String>) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn realpath(&mut self, id: u32, chemin: String) -> Result<Name, Self::Error> {
        let chemin = if chemin.is_empty() || chemin == "." { "/".to_string() } else { chemin };
        Ok(Name { id, files: vec![File::new(chemin, FileAttributes::empty())] })
    }

    async fn stat(&mut self, id: u32, chemin: String) -> Result<Attrs, Self::Error> {
        Ok(Attrs { id, attrs: self.attributs(&chemin)? })
    }

    async fn lstat(&mut self, id: u32, chemin: String) -> Result<Attrs, Self::Error> {
        Ok(Attrs { id, attrs: self.attributs(&chemin)? })
    }

    async fn opendir(&mut self, id: u32, chemin: String) -> Result<Handle, Self::Error> {
        let mut entrees = Vec::new();
        for entree in std::fs::read_dir(self.reel(&chemin)).map_err(statut)? {
            let nom = entree.map_err(statut)?.file_name().to_string_lossy().into_owned();
            let attributs = self.attributs(&format!("{}/{nom}", chemin.trim_end_matches('/')))?;
            entrees.push(File::new(nom, attributs));
        }
        Ok(Handle { id, handle: self.poignee(Ouvert::Dossier(Some(entrees))) })
    }

    async fn readdir(&mut self, id: u32, poignee: String) -> Result<Name, Self::Error> {
        match self.ouverts.get_mut(&poignee) {
            Some(Ouvert::Dossier(entrees)) => entrees.take().map(|files| Name { id, files }).ok_or(StatusCode::Eof),
            _ => Err(StatusCode::Failure),
        }
    }

    async fn open(&mut self, id: u32, nom: String, drapeaux: OpenFlags, _attrs: FileAttributes) -> Result<Handle, Self::Error> {
        let fichier = std::fs::OpenOptions::from(drapeaux).open(self.reel(&nom)).map_err(statut)?;
        Ok(Handle { id, handle: self.poignee(Ouvert::Fichier(fichier)) })
    }

    async fn read(&mut self, id: u32, poignee: String, decalage: u64, longueur: u32) -> Result<Data, Self::Error> {
        use std::io::{Read, Seek, SeekFrom};
        let Some(Ouvert::Fichier(fichier)) = self.ouverts.get_mut(&poignee) else { return Err(StatusCode::Failure) };
        fichier.seek(SeekFrom::Start(decalage)).map_err(statut)?;
        let mut data = vec![0; longueur as usize];
        let lu = fichier.read(&mut data).map_err(statut)?;
        if lu == 0 {
            return Err(StatusCode::Eof);
        }
        data.truncate(lu);
        Ok(Data { id, data })
    }

    async fn write(&mut self, id: u32, poignee: String, decalage: u64, data: Vec<u8>) -> Result<Status, Self::Error> {
        use std::io::{Seek, SeekFrom, Write};
        let Some(Ouvert::Fichier(fichier)) = self.ouverts.get_mut(&poignee) else { return Err(StatusCode::Failure) };
        fichier.seek(SeekFrom::Start(decalage)).map_err(statut)?;
        fichier.write_all(&data).map_err(statut)?;
        Ok(ok(id))
    }

    async fn close(&mut self, id: u32, poignee: String) -> Result<Status, Self::Error> {
        self.ouverts.remove(&poignee);
        Ok(ok(id))
    }

    async fn mkdir(&mut self, id: u32, chemin: String, _attrs: FileAttributes) -> Result<Status, Self::Error> {
        std::fs::create_dir(self.reel(&chemin)).map_err(statut)?;
        Ok(ok(id))
    }

    async fn rmdir(&mut self, id: u32, chemin: String) -> Result<Status, Self::Error> {
        std::fs::remove_dir(self.reel(&chemin)).map_err(statut)?;
        Ok(ok(id))
    }

    async fn remove(&mut self, id: u32, chemin: String) -> Result<Status, Self::Error> {
        std::fs::remove_file(self.reel(&chemin)).map_err(statut)?;
        Ok(ok(id))
    }

    async fn rename(&mut self, id: u32, ancien: String, nouveau: String) -> Result<Status, Self::Error> {
        std::fs::rename(self.reel(&ancien), self.reel(&nouveau)).map_err(statut)?;
        Ok(ok(id))
    }

    async fn setstat(&mut self, id: u32, chemin: String, attributs: FileAttributes) -> Result<Status, Self::Error> {
        self.attributs(&chemin)?;
        if let Some(mode) = attributs.permissions {
            self.droits.insert(chemin, mode & 0o7777);
        }
        Ok(ok(id))
    }
}

/// Démarre un serveur d'essai et rend son port.
fn serveur() -> u16 {
    serveur_sur(None)
}

/// Démarre un serveur d'essai dont le SFTP sert `disque`.
fn serveur_sur(disque: Option<PathBuf>) -> u16 {
    let ecoute = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = ecoute.local_addr().unwrap().port();
    ecoute.set_nonblocking(true).unwrap();
    let config = Arc::new(russh::server::Config {
        keys: vec![russh::keys::decode_secret_key(CLE_HOTE, None).unwrap()],
        auth_rejection_time: Duration::from_millis(10),
        auth_rejection_time_initial: Some(Duration::from_millis(0)),
        ..Default::default()
    });
    runtime().spawn(async move {
        let ecoute = tokio::net::TcpListener::from_std(ecoute).unwrap();
        while let Ok((flux, _)) = ecoute.accept().await {
            let config = config.clone();
            let disque = disque.clone();
            tokio::spawn(async move {
                let serveur = Serveur { disque, ..Default::default() };
                if let Ok(session) = russh::server::run_stream(config, flux, serveur).await {
                    let _ = session.await;
                }
            });
        }
    });
    port
}

fn dossier_essai(nom: &str) -> PathBuf {
    let dossier = std::env::temp_dir().join(format!("nd-ssh-{nom}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dossier);
    std::fs::create_dir_all(&dossier).unwrap();
    dossier
}

fn cible(port: u16, dossier: &std::path::Path, authentification: Authentification) -> Cible {
    Cible {
        hote: "127.0.0.1".into(),
        port,
        utilisateur: UTILISATEUR.into(),
        authentification,
        colonnes: 80,
        lignes: 24,
        hotes_connus: dossier.join("hotes_connus"),
        hotes_connus_openssh: None,
    }
}

fn mot_de_passe() -> Authentification {
    Authentification::MotDePasse(MOT_DE_PASSE.into())
}

/// Retire de la file les évènements de `onglet` jusqu'à ce que `fini` en
/// accepte un ; rend tout ce qui a été retiré. Les autres onglets (tests
/// parallèles) restent dans la file.
fn attendre(onglet: u32, fini: impl Fn(&Evenement) -> bool) -> Vec<Evenement> {
    let debut = Instant::now();
    let mut vus = Vec::new();
    while debut.elapsed() < Duration::from_secs(10) {
        {
            let mut file = EVENEMENTS.lock().unwrap();
            let mut garde = Vec::new();
            for evenement in file.drain(..) {
                if onglet_de(&evenement) == onglet {
                    vus.push(evenement);
                } else {
                    garde.push(evenement);
                }
            }
            *file = garde;
        }
        if vus.iter().any(&fini) {
            return vus;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("onglet {onglet} : délai dépassé ; vus : {vus:?}");
}

fn onglet_de(evenement: &Evenement) -> u32 {
    match evenement {
        Evenement::Etat { onglet, .. }
        | Evenement::Erreur { onglet, .. }
        | Evenement::Sortie { onglet, .. }
        | Evenement::CleInconnue { onglet, .. }
        | Evenement::Sftp { onglet, .. }
        | Evenement::Local { onglet, .. } => *onglet,
    }
}

fn sortie(evenements: &[Evenement]) -> String {
    evenements
        .iter()
        .filter_map(|e| match e {
            Evenement::Sortie { octets, .. } => Some(String::from_utf8_lossy(octets).into_owned()),
            _ => None,
        })
        .collect()
}

fn ferme(e: &Evenement) -> bool {
    matches!(e, Evenement::Etat { etat: Etat::Ferme, .. })
}

fn erreurs(evenements: &[Evenement]) -> Vec<String> {
    evenements
        .iter()
        .filter_map(|e| match e {
            Evenement::Erreur { message, .. } => Some(message.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn cle_inconnue_confirmee_puis_terminal_redimensionnement_et_sftp() {
    let port = serveur();
    let dossier = dossier_essai("parcours");
    let onglet = 101;
    connecter(onglet, cible(port, &dossier, mot_de_passe()));

    // 1. Clé jamais vue : on nous la montre, avec son empreinte.
    let vus = attendre(onglet, |e| matches!(e, Evenement::CleInconnue { .. }));
    let Some(Evenement::CleInconnue { algorithme, empreinte: montree, .. }) = vus.last().cloned() else { unreachable!() };
    assert_eq!(algorithme, "ssh-ed25519");
    let attendue = empreinte(russh::keys::decode_secret_key(CLE_HOTE, None).unwrap().public_key());
    assert_eq!(montree, attendue);
    assert!(montree.starts_with("SHA256:"), "{montree}");
    confirmer_cle(onglet, true);

    // 2. Ouvert, et la clé est mémorisée pour la fois suivante.
    attendre(onglet, |e| matches!(e, Evenement::Etat { etat: Etat::Ouvert, .. }));
    let memorise = std::fs::read_to_string(dossier.join("hotes_connus")).unwrap();
    assert!(memorise.trim_start().starts_with(&format!("[127.0.0.1]:{port} ssh-ed25519 ")), "{memorise}");

    // 3. Le terminal : ce qu'on écrit revient (écho du serveur d'essai).
    assert!(ecrire(onglet, "bonjour é".as_bytes().to_vec()));
    let vus = attendre(onglet, |e| matches!(e, Evenement::Sortie { .. }));
    assert_eq!(sortie(&vus), "bonjour é");

    // 4. Redimensionnement : le serveur reçoit la nouvelle taille.
    assert!(redimensionner(onglet, 120, 40));
    let vus = attendre(onglet, |e| matches!(e, Evenement::Sortie { .. }));
    assert_eq!(sortie(&vus), "taille 120x40");

    // 5. SFTP : chemin absolu, « . » écarté, dossiers d'abord puis par nom.
    assert!(sftp(onglet, RequeteSftp::Lister { chemin: String::new() }));
    let vus = attendre(onglet, |e| matches!(e, Evenement::Sftp { .. }));
    let Some(Evenement::Sftp { reponse: ReponseSftp::Liste { chemin, entrees }, .. }) = vus.last().cloned() else {
        panic!("{vus:?}")
    };
    assert_eq!(chemin, "/racine");
    let noms: Vec<_> = entrees.iter().map(|e| e.nom.as_str()).collect();
    assert_eq!(noms, ["Dossier", "a.txt", "b.txt"]);
    assert!(entrees[0].dossier && !entrees[1].dossier);
    assert_eq!((entrees[1].taille, entrees[1].droits, entrees[1].modifie), (12, 0o644, 1_700_000_000));

    // 6. `exit` : le serveur ferme, l'onglet le sait.
    ecrire(onglet, b"exit\r".to_vec());
    attendre(onglet, ferme);
    assert!(!ecrire(onglet, b"x".to_vec()), "la file de l'onglet fermé doit avoir disparu");
}

#[test]
fn cle_deja_connue_et_authentification_par_cle_chiffree() {
    let port = serveur();
    let dossier = dossier_essai("cle");
    let publique = russh::keys::decode_secret_key(CLE_HOTE, None).unwrap().public_key().clone();
    russh::keys::known_hosts::learn_known_hosts_path("127.0.0.1", port, &publique, dossier.join("hotes_connus")).unwrap();
    std::fs::write(dossier.join("id"), CLE_CLIENT).unwrap();
    let onglet = 102;
    connecter(
        onglet,
        cible(port, &dossier, Authentification::Cle { chemin: dossier.join("id"), phrase: Some("secret".into()) }),
    );
    let vus = attendre(onglet, |e| matches!(e, Evenement::Etat { etat: Etat::Ouvert, .. }) || ferme(e));
    assert!(!vus.iter().any(|e| matches!(e, Evenement::CleInconnue { .. })), "clé connue : aucune question ; {vus:?}");
    assert!(vus.iter().any(|e| matches!(e, Evenement::Etat { etat: Etat::Ouvert, .. })), "{vus:?}");
    fermer(onglet);
    attendre(onglet, ferme);
}

#[test]
fn cle_connue_dans_le_fichier_openssh_suffit() {
    let port = serveur();
    let dossier = dossier_essai("openssh");
    let publique = russh::keys::decode_secret_key(CLE_HOTE, None).unwrap().public_key().clone();
    russh::keys::known_hosts::learn_known_hosts_path("127.0.0.1", port, &publique, dossier.join("known_hosts")).unwrap();
    let mut cible = cible(port, &dossier, mot_de_passe());
    cible.hotes_connus_openssh = Some(dossier.join("known_hosts"));
    let onglet = 103;
    connecter(onglet, cible);
    let vus = attendre(onglet, |e| matches!(e, Evenement::Etat { etat: Etat::Ouvert, .. }) || ferme(e));
    assert!(!vus.iter().any(|e| matches!(e, Evenement::CleInconnue { .. })), "{vus:?}");
    fermer(onglet);
    attendre(onglet, ferme);
}

#[test]
fn cle_changee_refusee_sans_question() {
    let port = serveur();
    let dossier = dossier_essai("changee");
    // Une AUTRE clé ed25519 enregistrée pour cet hôte.
    let autre = russh::keys::parse_public_key_base64(CLE_CLIENT_PUBLIQUE).unwrap();
    russh::keys::known_hosts::learn_known_hosts_path("127.0.0.1", port, &autre, dossier.join("hotes_connus")).unwrap();
    let onglet = 104;
    connecter(onglet, cible(port, &dossier, mot_de_passe()));
    let vus = attendre(onglet, ferme);
    assert!(!vus.iter().any(|e| matches!(e, Evenement::CleInconnue { .. })), "{vus:?}");
    assert!(erreurs(&vus).iter().any(|m| m.contains("CHANGÉ")), "{vus:?}");
    assert!(!vus.iter().any(|e| matches!(e, Evenement::Etat { etat: Etat::Ouvert, .. })));
}

#[test]
fn cle_refusee_par_l_utilisateur() {
    let port = serveur();
    let dossier = dossier_essai("refus");
    let onglet = 105;
    connecter(onglet, cible(port, &dossier, mot_de_passe()));
    attendre(onglet, |e| matches!(e, Evenement::CleInconnue { .. }));
    confirmer_cle(onglet, false);
    let vus = attendre(onglet, ferme);
    assert!(erreurs(&vus).iter().any(|m| m.contains("refusée")), "{vus:?}");
    assert!(!dossier.join("hotes_connus").exists(), "une clé refusée ne doit pas être mémorisée");
}

#[test]
fn mauvais_mot_de_passe() {
    let port = serveur();
    let dossier = dossier_essai("mdp");
    let onglet = 106;
    connecter(onglet, cible(port, &dossier, Authentification::MotDePasse("faux".into())));
    attendre(onglet, |e| matches!(e, Evenement::CleInconnue { .. }));
    confirmer_cle(onglet, true);
    let vus = attendre(onglet, ferme);
    assert_eq!(erreurs(&vus), ["Authentification refusée."], "{vus:?}");
}

#[test]
fn cle_chiffree_sans_phrase() {
    let port = serveur();
    let dossier = dossier_essai("phrase");
    std::fs::write(dossier.join("id"), CLE_CLIENT).unwrap();
    let publique = russh::keys::decode_secret_key(CLE_HOTE, None).unwrap().public_key().clone();
    russh::keys::known_hosts::learn_known_hosts_path("127.0.0.1", port, &publique, dossier.join("hotes_connus")).unwrap();
    let onglet = 107;
    connecter(onglet, cible(port, &dossier, Authentification::Cle { chemin: dossier.join("id"), phrase: None }));
    let vus = attendre(onglet, ferme);
    assert!(erreurs(&vus).iter().any(|m| m.contains("phrase de passe")), "{vus:?}");
}

#[test]
fn port_ferme() {
    // Un port lié puis relâché : plus personne n'écoute.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let dossier = dossier_essai("ferme");
    let onglet = 108;
    connecter(onglet, cible(port, &dossier, mot_de_passe()));
    let vus = attendre(onglet, ferme);
    assert!(erreurs(&vus).iter().any(|m| m.starts_with("Connexion impossible")), "{vus:?}");
}

/// Envoie `requete` et attend sa réponse (les `Progres` sont passés).
fn demander(onglet: u32, requete: RequeteSftp) -> ReponseSftp {
    assert!(sftp(onglet, requete));
    let fin = |e: &Evenement| matches!(e, Evenement::Sftp { reponse, .. } if !matches!(reponse, ReponseSftp::Progres { .. }));
    let vus = attendre(onglet, fin);
    match vus.into_iter().rev().find(fin) {
        Some(Evenement::Sftp { reponse, .. }) => reponse,
        _ => unreachable!(),
    }
}

fn sans_erreur(reponse: ReponseSftp) {
    assert!(matches!(&reponse, ReponseSftp::Fait { erreurs, .. } if erreurs.is_empty()), "{reponse:?}");
}

#[test]
fn fichiers_transferts_creation_renommage_droits_et_suppression() {
    let dossier = dossier_essai("fichiers");
    let (racine, local) = (dossier.join("serveur"), dossier.join("local"));
    std::fs::create_dir_all(racine.join("docs/sous")).unwrap();
    std::fs::create_dir_all(&local).unwrap();
    std::fs::write(racine.join("docs/a.txt"), b"alpha").unwrap();
    // Plus grand qu'un `MORCEAU` : la copie doit boucler.
    let gros: Vec<u8> = (0..600_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(racine.join("docs/sous/b.bin"), &gros).unwrap();
    let port = serveur_sur(Some(racine.clone()));
    let publique = russh::keys::decode_secret_key(CLE_HOTE, None).unwrap().public_key().clone();
    russh::keys::known_hosts::learn_known_hosts_path("127.0.0.1", port, &publique, dossier.join("hotes_connus")).unwrap();
    let onglet = 109;
    connecter(onglet, cible(port, &dossier, mot_de_passe()));
    attendre(onglet, |e| matches!(e, Evenement::Etat { etat: Etat::Ouvert, .. }));

    // Créer un dossier ; un nom à « / » est refusé avant d'atteindre le serveur.
    sans_erreur(demander(onglet, RequeteSftp::CreerDossier { dossier: "/".into(), nom: "neuf".into() }));
    assert!(racine.join("neuf").is_dir());
    let refus = demander(onglet, RequeteSftp::CreerDossier { dossier: "/".into(), nom: "a/b".into() });
    assert!(matches!(refus, ReponseSftp::Erreur { .. }), "{refus:?}");

    // Téléverser ; la seconde fois, le fichier distant n'est PAS écrasé.
    std::fs::write(local.join("envoi.txt"), b"contenu").unwrap();
    let envoi = RequeteSftp::Televerser { locaux: vec![local.join("envoi.txt")], dossier_distant: "/neuf".into() };
    sans_erreur(demander(onglet, envoi.clone()));
    assert_eq!(std::fs::read(racine.join("neuf/envoi.txt")).unwrap(), b"contenu");
    std::fs::write(local.join("envoi.txt"), b"autre").unwrap();
    let ReponseSftp::Fait { erreurs, .. } = demander(onglet, envoi) else { panic!() };
    assert!(erreurs.len() == 1 && erreurs[0].contains("existe déjà"), "{erreurs:?}");
    assert_eq!(std::fs::read(racine.join("neuf/envoi.txt")).unwrap(), b"contenu");

    // Téléverser un DOSSIER, récursivement (le volet « ce PC » en envoie).
    std::fs::create_dir_all(local.join("paquet/interne")).unwrap();
    std::fs::write(local.join("paquet/interne/c.txt"), b"gamma").unwrap();
    let paquet = RequeteSftp::Televerser { locaux: vec![local.join("paquet")], dossier_distant: "/neuf".into() };
    sans_erreur(demander(onglet, paquet.clone()));
    assert_eq!(std::fs::read(racine.join("neuf/paquet/interne/c.txt")).unwrap(), b"gamma");
    let ReponseSftp::Fait { erreurs, .. } = demander(onglet, paquet) else { panic!() };
    assert!(erreurs.len() == 1 && erreurs[0].contains("existe déjà"), "{erreurs:?}");
    std::fs::remove_dir_all(local.join("paquet")).unwrap();

    // Télécharger un dossier, récursivement ; la seconde fois, un nom libre.
    let recevoir = RequeteSftp::Telecharger { distants: vec!["/docs".into()], dossier_local: local.clone() };
    sans_erreur(demander(onglet, recevoir.clone()));
    assert_eq!(std::fs::read(local.join("docs/a.txt")).unwrap(), b"alpha");
    assert_eq!(std::fs::read(local.join("docs/sous/b.bin")).unwrap(), gros);
    sans_erreur(demander(onglet, recevoir));
    assert_eq!(std::fs::read(local.join("docs (1)/a.txt")).unwrap(), b"alpha");

    // Renommer ; vers un nom pris, refusé.
    sans_erreur(demander(onglet, RequeteSftp::Renommer { chemin: "/neuf/envoi.txt".into(), nom: "r.txt".into() }));
    assert!(racine.join("neuf/r.txt").is_file() && !racine.join("neuf/envoi.txt").exists());
    let pris = demander(onglet, RequeteSftp::Renommer { chemin: "/docs/a.txt".into(), nom: "sous".into() });
    assert!(matches!(pris, ReponseSftp::Erreur { .. }), "{pris:?}");

    // Droits : relus par le listage.
    sans_erreur(demander(onglet, RequeteSftp::Droits { chemin: "/docs/a.txt".into(), mode: 0o600 }));
    let ReponseSftp::Liste { entrees, .. } = demander(onglet, RequeteSftp::Lister { chemin: "/docs".into() }) else { panic!() };
    assert_eq!(entrees.iter().find(|e| e.nom == "a.txt").map(|e| e.droits), Some(0o600));

    // Supprimer, récursivement.
    sans_erreur(demander(onglet, RequeteSftp::Supprimer { chemins: vec!["/docs".into(), "/neuf".into()] }));
    assert_eq!(std::fs::read_dir(&racine).unwrap().count(), 0);

    fermer(onglet);
    attendre(onglet, ferme);
    let _ = std::fs::remove_dir_all(&dossier);
}

/// ⭐ **LA VERSION MINIMALE DE `russh` EST UNE EXIGENCE DE SÉCURITÉ, PAS UN GOÛT** —
/// six avis Dependabot sur 0.63, tous corrigés en 0.65 (2026-11-11). Deux d'entre
/// eux vivent dans le code **CLIENT**, celui que nous exécutons : allocations non
/// bornées via `EXT_INFO` (GHSA-4p9f-7p9m-5vfg) et écriture de canal qui pend
/// pour toujours sur une taille de paquet maximale nulle (GHSA-fw5c-8223-6h42).
/// Les quatre autres sont côté serveur / serveur d'agent, que nous ne lançons pas
/// (le serveur ci-dessus est un banc d'essai).
///
/// **MUTATION** : remettre `version = "0.63"` dans `Cargo.toml` et ce test tombe.
#[test]
fn russh_est_au_moins_en_0_65() {
    let manifeste = include_str!("../../../Cargo.toml");
    let ligne = manifeste
        .lines()
        .find(|l| l.trim_start().starts_with("russh ="))
        .expect("la dépendance `russh` a disparu de Cargo.toml");
    let version = ligne
        .split("version = \"")
        .nth(1)
        .and_then(|reste| reste.split('"').next())
        .expect("la ligne `russh` de Cargo.toml ne porte pas de version");
    // « 0.65 » se lit majeur, puis MINEUR — le premier nombre est le majeur.
    let mut nombres = version.split('.').map(|n| n.parse::<u32>());
    let majeur = nombres.next().expect("version `russh` illisible").expect("version `russh` illisible");
    let mineur = nombres.next().expect("version `russh` illisible").expect("version `russh` illisible");
    assert!(
        (majeur, mineur) >= (0, 65),
        "russh {version} est sous le correctif des six avis : monter à 0.65 au moins"
    );
}
