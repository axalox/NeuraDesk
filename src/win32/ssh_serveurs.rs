//! Les SERVEURS ENREGISTRÉS de l'applet SSH (voir `win32::ssh`, qui porte le
//! client, et `docs/plans/ssh-sftp.md`) — `ssh.toml`.
//!
//! ⛔ **LE SECRET NE QUITTE JAMAIS RUST EN CLAIR.** Mot de passe ou phrase de
//! passe de la clé : chiffré par **DPAPI** (`CryptProtectData`, lié à la session
//! Windows de l'utilisateur) avant d'être écrit, jamais renvoyé à la page (elle
//! ne reçoit que `secretDefini`), jamais journalisé.

use std::cell::RefCell;
use std::path::PathBuf;

use base64::Engine;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

use crate::config;
use crate::win32::ssh::{Authentification, Cible};

const FICHIER: &str = "ssh.toml";
/// Clés d'hôte acceptées depuis l'applet — un fichier À NOUS : on n'écrit
/// jamais dans `~/.ssh/known_hosts`, qu'on lit seulement.
const HOTES_CONNUS: &str = "ssh_hotes_connus";
pub const PORT_DEFAUT: u16 = 22;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Methode {
    #[default]
    MotDePasse,
    Cle,
}

impl Methode {
    pub fn nom(self) -> &'static str {
        match self {
            Methode::MotDePasse => "motdepasse",
            Methode::Cle => "cle",
        }
    }

    pub fn depuis_nom(nom: &str) -> Option<Methode> {
        match nom {
            "motdepasse" => Some(Methode::MotDePasse),
            "cle" => Some(Methode::Cle),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Serveur {
    pub id: u32,
    pub nom: String,
    pub hote: String,
    pub port: u16,
    pub utilisateur: String,
    pub methode: Methode,
    /// Chemin de la clé privée (méthode `Cle`).
    pub cle: String,
    /// Mot de passe (méthode `MotDePasse`) ou phrase de passe de la clé,
    /// CHIFFRÉ DPAPI puis base64 — tel qu'écrit dans `ssh.toml`.
    secret: Option<String>,
}

impl Serveur {
    pub fn secret_defini(&self) -> bool {
        self.secret.is_some()
    }

    /// Le libellé d'un onglet : le nom donné, à défaut `utilisateur@hôte`.
    pub fn libelle(&self) -> String {
        if self.nom.trim().is_empty() {
            format!("{}@{}", self.utilisateur, self.hote)
        } else {
            self.nom.clone()
        }
    }

    /// La cible de connexion. `secret_saisi` (tapé à la connexion, jamais
    /// enregistré) passe avant le secret enregistré. `Err` : il manque le mot
    /// de passe — la page doit le demander.
    pub fn cible(&self, secret_saisi: Option<String>, colonnes: u32, lignes: u32) -> Result<Cible, String> {
        let secret = secret_saisi.or_else(|| self.secret.as_deref().and_then(dechiffrer));
        let authentification = match self.methode {
            Methode::MotDePasse => Authentification::MotDePasse(secret.ok_or("Mot de passe requis.")?),
            Methode::Cle => {
                if self.cle.trim().is_empty() {
                    return Err("Aucune clé privée choisie pour ce serveur.".into());
                }
                Authentification::Cle { chemin: PathBuf::from(self.cle.trim()), phrase: secret.filter(|p| !p.is_empty()) }
            }
        };
        Ok(Cible {
            hote: self.hote.trim().to_string(),
            port: self.port,
            utilisateur: self.utilisateur.trim().to_string(),
            authentification,
            colonnes,
            lignes,
            hotes_connus: config::config_path(HOTES_CONNUS).unwrap_or_else(|| PathBuf::from(HOTES_CONNUS)),
            hotes_connus_openssh: std::env::var_os("USERPROFILE")
                .map(|profil| PathBuf::from(profil).join(".ssh").join("known_hosts")),
        })
    }
}

/// Ce que le formulaire de la page envoie (JSON, par le canal POST — jamais
/// dans une URL, qui passerait par le journal).
#[derive(Debug, Default, PartialEq)]
pub struct Fiche {
    /// 0 = nouveau serveur.
    pub id: u32,
    pub nom: String,
    pub hote: String,
    pub port: Option<u16>,
    pub utilisateur: String,
    pub methode: String,
    pub cle: String,
    /// `None` : garder le secret enregistré ; `Some("")` : l'effacer.
    pub secret: Option<String>,
}

impl Fiche {
    pub fn depuis_json(octets: &[u8]) -> Result<Fiche, String> {
        let v: serde_json::Value = serde_json::from_slice(octets).map_err(|_| "Formulaire illisible.".to_string())?;
        let texte = |cle: &str| v.get(cle).and_then(|x| x.as_str()).unwrap_or("").to_string();
        Ok(Fiche {
            id: v.get("id").and_then(|x| x.as_u64()).and_then(|x| u32::try_from(x).ok()).unwrap_or(0),
            nom: texte("nom"),
            hote: texte("hote"),
            port: match v.get("port") {
                None | Some(serde_json::Value::Null) => None,
                Some(p) => Some(
                    p.as_u64()
                        .or_else(|| p.as_str().and_then(|s| s.trim().parse().ok()))
                        .and_then(|p| u16::try_from(p).ok())
                        .ok_or("Port invalide (1 à 65535).")?,
                ),
            },
            utilisateur: texte("utilisateur"),
            methode: texte("methode"),
            cle: texte("cle"),
            secret: v.get("secret").and_then(|x| x.as_str()).map(str::to_string),
        })
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Serveurs {
    pub liste: Vec<Serveur>,
}

impl Serveurs {
    pub fn trouver(&self, id: u32) -> Option<&Serveur> {
        self.liste.iter().find(|s| s.id == id)
    }

    /// Ajoute (`id == 0`) ou modifie un serveur ; rend son numéro.
    pub fn enregistrer(&mut self, fiche: Fiche) -> Result<u32, String> {
        let hote = fiche.hote.trim().to_string();
        let utilisateur = fiche.utilisateur.trim().to_string();
        if hote.is_empty() {
            return Err("Indiquez l'adresse du serveur.".into());
        }
        if hote.contains(char::is_whitespace) {
            return Err("L'adresse du serveur ne peut pas contenir d'espace.".into());
        }
        if utilisateur.is_empty() {
            return Err("Indiquez l'identifiant.".into());
        }
        let methode = Methode::depuis_nom(&fiche.methode).ok_or("Méthode d'authentification inconnue.")?;
        let port = fiche.port.filter(|&p| p != 0).unwrap_or(PORT_DEFAUT);
        let secret = match fiche.secret {
            None => None,
            Some(s) if s.is_empty() => Some(None),
            Some(s) => Some(Some(chiffrer(&s).ok_or("Le secret n'a pas pu être chiffré.")?)),
        };
        let id = if fiche.id == 0 { self.liste.iter().map(|s| s.id).max().unwrap_or(0) + 1 } else { fiche.id };
        let existant = self.liste.iter().position(|s| s.id == id);
        if fiche.id != 0 && existant.is_none() {
            return Err("Ce serveur n'existe plus.".into());
        }
        let ancien_secret = existant.and_then(|i| self.liste[i].secret.clone());
        let serveur = Serveur {
            id,
            nom: fiche.nom.trim().to_string(),
            hote,
            port,
            utilisateur,
            methode,
            cle: fiche.cle.trim().to_string(),
            secret: secret.unwrap_or(ancien_secret),
        };
        match existant {
            Some(i) => self.liste[i] = serveur,
            None => self.liste.push(serveur),
        }
        Ok(id)
    }

    pub fn supprimer(&mut self, id: u32) {
        self.liste.retain(|s| s.id != id);
    }
}

// ---------------------------------------------------------------------------
// DPAPI
// ---------------------------------------------------------------------------

fn blob(octets: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB { cbData: octets.len() as u32, pbData: octets.as_ptr() as *mut u8 }
}

/// Recopie puis libère un blob rendu par DPAPI (alloué par `LocalAlloc`).
fn prendre_blob(sortie: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if sortie.pbData.is_null() {
        return Vec::new();
    }
    // SAFETY : DPAPI rend `cbData` octets valides à `pbData`.
    let octets = unsafe { std::slice::from_raw_parts(sortie.pbData, sortie.cbData as usize) }.to_vec();
    unsafe { LocalFree(Some(HLOCAL(sortie.pbData.cast()))) };
    octets
}

fn chiffrer(clair: &str) -> Option<String> {
    let entree = blob(clair.as_bytes());
    let mut sortie = CRYPT_INTEGER_BLOB::default();
    unsafe { CryptProtectData(&entree, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut sortie) }.ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(prendre_blob(sortie)))
}

fn dechiffrer(chiffre: &str) -> Option<String> {
    let octets = base64::engine::general_purpose::STANDARD.decode(chiffre).ok()?;
    let entree = blob(&octets);
    let mut sortie = CRYPT_INTEGER_BLOB::default();
    if let Err(err) = unsafe { CryptUnprotectData(&entree, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut sortie) } {
        // Le message ne contient rien du secret.
        crate::diag!("[ssh] secret enregistré indéchiffrable ({err}) — autre session Windows ?");
        return None;
    }
    String::from_utf8(prendre_blob(sortie)).ok()
}

// ---------------------------------------------------------------------------
// ssh.toml
// ---------------------------------------------------------------------------

fn depuis_toml(texte: &str) -> Serveurs {
    let Ok(table) = texte.parse::<toml::Table>() else { return Serveurs::default() };
    let s = |v: &toml::Value, cle: &str| v.get(cle).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let liste = table
        .get("serveurs")
        .and_then(|v| v.as_array())
        .map(|lignes| {
            lignes
                .iter()
                .filter_map(|v| {
                    let id = u32::try_from(v.get("id")?.as_integer()?).ok().filter(|&id| id != 0)?;
                    Some(Serveur {
                        id,
                        nom: s(v, "nom"),
                        hote: s(v, "hote"),
                        port: v
                            .get("port")
                            .and_then(|x| x.as_integer())
                            .and_then(|p| u16::try_from(p).ok())
                            .unwrap_or(PORT_DEFAUT),
                        utilisateur: s(v, "utilisateur"),
                        methode: Methode::depuis_nom(&s(v, "methode")).unwrap_or_default(),
                        cle: s(v, "cle"),
                        secret: Some(s(v, "secret")).filter(|x| !x.is_empty()),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Serveurs { liste }
}

fn vers_toml(serveurs: &Serveurs) -> String {
    use toml::Value;
    let lignes = serveurs
        .liste
        .iter()
        .map(|s| {
            let mut t = toml::Table::new();
            t.insert("id".into(), Value::Integer(s.id as i64));
            t.insert("nom".into(), Value::String(s.nom.clone()));
            t.insert("hote".into(), Value::String(s.hote.clone()));
            t.insert("port".into(), Value::Integer(s.port as i64));
            t.insert("utilisateur".into(), Value::String(s.utilisateur.clone()));
            t.insert("methode".into(), Value::String(s.methode.nom().into()));
            t.insert("cle".into(), Value::String(s.cle.clone()));
            if let Some(secret) = &s.secret {
                t.insert("secret".into(), Value::String(secret.clone()));
            }
            Value::Table(t)
        })
        .collect();
    let mut table = toml::Table::new();
    table.insert("serveurs".into(), Value::Array(lignes));
    toml::to_string(&table).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// L'ÉTAT DE L'APPLICATION
// ---------------------------------------------------------------------------

thread_local! {
    static ETAT: RefCell<Serveurs> = RefCell::new(Serveurs::default());
}

/// Relit `ssh.toml` — une fois, au démarrage (voir `app::run`).
pub fn charger() {
    let texte = config::config_path(FICHIER).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    ETAT.with(|e| *e.borrow_mut() = depuis_toml(&texte));
}

pub fn lire<R>(f: impl FnOnce(&Serveurs) -> R) -> R {
    ETAT.with(|e| f(&e.borrow()))
}

/// Modifie l'état PUIS l'enregistre — le seul chemin d'écriture.
pub fn modifier<R>(f: impl FnOnce(&mut Serveurs) -> R) -> R {
    ETAT.with(|e| {
        let mut serveurs = e.borrow_mut();
        let r = f(&mut serveurs);
        if !config::write_config_atomique(FICHIER, vers_toml(&serveurs)) {
            crate::diag!("[ssh] ssh.toml n'a pas pu être écrit");
        }
        r
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fiche(id: u32, secret: Option<&str>) -> Fiche {
        Fiche {
            id,
            nom: "Maison".into(),
            hote: " nas.local ".into(),
            port: None,
            utilisateur: "moi".into(),
            methode: "motdepasse".into(),
            cle: String::new(),
            secret: secret.map(str::to_string),
        }
    }

    #[test]
    fn dpapi_rend_le_secret_et_ne_l_ecrit_pas_en_clair() {
        let chiffre = chiffrer("sésame ouvre-toi").expect("DPAPI doit chiffrer");
        assert!(!chiffre.contains("sésame"));
        assert_eq!(dechiffrer(&chiffre).as_deref(), Some("sésame ouvre-toi"));
        assert_eq!(dechiffrer("pas du base64 !"), None);
    }

    #[test]
    fn ajouter_modifier_et_garder_le_secret() {
        let mut serveurs = Serveurs::default();
        let id = serveurs.enregistrer(fiche(0, Some("sésame"))).unwrap();
        assert_eq!(id, 1);
        let s = serveurs.trouver(1).unwrap();
        assert_eq!((s.hote.as_str(), s.port), ("nas.local", PORT_DEFAUT), "adresse rognée, port par défaut");
        assert!(s.secret_defini());

        // Modifier sans renvoyer le secret le GARDE.
        let mut modif = fiche(1, None);
        modif.port = Some(2222);
        serveurs.enregistrer(modif).unwrap();
        let s = serveurs.trouver(1).unwrap();
        assert_eq!(s.port, 2222);
        match s.cible(None, 80, 24).unwrap().authentification {
            Authentification::MotDePasse(m) => assert_eq!(m, "sésame"),
            autre => panic!("{autre:?}"),
        }

        // Un secret vide l'EFFACE ; la connexion le demande alors.
        serveurs.enregistrer(fiche(1, Some(""))).unwrap();
        assert!(!serveurs.trouver(1).unwrap().secret_defini());
        assert!(serveurs.trouver(1).unwrap().cible(None, 80, 24).is_err());
        assert!(serveurs.trouver(1).unwrap().cible(Some("tapé".into()), 80, 24).is_ok());

        assert_eq!(serveurs.enregistrer(fiche(0, None)).unwrap(), 2, "numéro suivant");
        serveurs.supprimer(1);
        assert_eq!(serveurs.liste.iter().map(|s| s.id).collect::<Vec<_>>(), vec![2]);
        assert!(serveurs.enregistrer(fiche(1, None)).is_err(), "modifier un serveur supprimé");
    }

    #[test]
    fn la_fiche_se_lit_depuis_le_json_de_la_page() {
        let f = Fiche::depuis_json(
            br#"{"id":3,"nom":"NAS","hote":"nas","port":"2222","utilisateur":"moi","methode":"cle","cle":"C:\\k","secret":null}"#,
        )
        .unwrap();
        assert_eq!((f.id, f.port, f.secret), (3, Some(2222), None));
        assert_eq!(Fiche::depuis_json(br#"{"port":""}"#).unwrap_err(), "Port invalide (1 à 65535).");
        assert_eq!(Fiche::depuis_json(br#"{"port":70000}"#).unwrap_err(), "Port invalide (1 à 65535).");
        assert_eq!(Fiche::depuis_json(br#"{"secret":""}"#).unwrap().secret.as_deref(), Some(""));
        assert!(Fiche::depuis_json(b"pas du json").is_err());
    }

    #[test]
    fn une_fiche_incomplete_est_refusee() {
        let mut serveurs = Serveurs::default();
        let mut f = fiche(0, None);
        f.hote = "  ".into();
        assert!(serveurs.enregistrer(f).is_err());
        let mut f = fiche(0, None);
        f.utilisateur.clear();
        assert!(serveurs.enregistrer(f).is_err());
        let mut f = fiche(0, None);
        f.methode = "pigeon".into();
        assert!(serveurs.enregistrer(f).is_err());
        assert!(serveurs.liste.is_empty());
    }

    #[test]
    fn ssh_toml_fait_l_aller_retour_sans_le_secret_en_clair() {
        let mut serveurs = Serveurs::default();
        serveurs.enregistrer(fiche(0, Some("sésame"))).unwrap();
        let mut cle = fiche(0, Some("phrase"));
        cle.methode = "cle".into();
        cle.cle = r"C:\Users\moi\.ssh\id_ed25519".into();
        serveurs.enregistrer(cle).unwrap();
        let texte = vers_toml(&serveurs);
        assert!(!texte.contains("sésame") && !texte.contains("phrase"));
        assert_eq!(depuis_toml(&texte), serveurs);
        match depuis_toml(&texte).trouver(2).unwrap().cible(None, 80, 24).unwrap().authentification {
            Authentification::Cle { chemin, phrase } => {
                assert_eq!(chemin, PathBuf::from(r"C:\Users\moi\.ssh\id_ed25519"));
                assert_eq!(phrase.as_deref(), Some("phrase"));
            }
            autre => panic!("{autre:?}"),
        }
        assert_eq!(depuis_toml("n'importe quoi"), Serveurs::default());
    }

    #[test]
    fn le_debug_de_la_cible_masque_le_secret() {
        let mut serveurs = Serveurs::default();
        serveurs.enregistrer(fiche(0, Some("sésame"))).unwrap();
        let cible = serveurs.trouver(1).unwrap().cible(None, 80, 24).unwrap();
        assert!(!format!("{cible:?}").contains("sésame"));
    }
}
