//! Les COMMANDES ENREGISTRÉES des panneaux Terminal et SSH — DEMANDE
//! EXPLICITE (2026-10-03) : « un panneau rétractable à droite des terminaux
//! classiques et ssh où je puisse enregistrer des commandes pour les envoyer
//! directement dans le terminal actif », puis « un système de dossiers pour
//! gérer les commandes ».
//!
//! Une seule liste, dans `commandes.toml`, partagée par les deux panneaux :
//! chaque commande dit dans quel dossier elle se range, et apparaît dans LES
//! DEUX panneaux — DEMANDE EXPLICITE (2026-10-11) : « que les terminaux et les
//! ssh partagent les mêmes commandes enregistrées ». Une `portee` (tous /
//! terminal / ssh) les cachait à l'autre panneau ; elle est retirée, et le champ
//! des fichiers d'avant est ignoré à la lecture. La page tient la liste et la renvoie ENTIÈRE à chaque changement
//! (POST `terminal/commandes` ou `ssh/commandes`, puis l'action du même nom) ;
//! Rust la valide, l'écrit et la repousse aux deux panneaux. L'envoi dans un
//! terminal ne passe pas par ici : la page écrit le texte comme des frappes.
//!
//! ⛔ RIEN AU JOURNAL sur le contenu : une commande peut porter un mot de passe
//! ou un jeton (les routes `terminal/` et `ssh/` sont déjà muettes).

use std::cell::RefCell;

use serde_json::json;

use crate::config;

const FICHIER: &str = "commandes.toml";
/// Des bornes pour qu'un corps aberrant ne remplisse pas le disque.
const MAX_COMMANDES: usize = 500;
const MAX_DOSSIERS: usize = 100;
const MAX_NOM: usize = 80;
const MAX_TEXTE: usize = 16_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commande {
    pub nom: String,
    pub texte: String,
    /// Envoyer aussi Entrée (la commande s'exécute) ; sinon le texte attend
    /// dans la ligne de commande.
    pub executer: bool,
    /// Le dossier de la commande ; vide : hors dossier. Un seul niveau : un
    /// dossier ne contient que des commandes.
    pub dossier: String,
}

/// Les commandes ET les dossiers. Les dossiers sont une liste À PART : un
/// dossier vide survit, et leur ordre est celui de l'utilisateur.
/// Invariant (voir [`Liste::normaliser`]) : chaque dossier cité par une
/// commande y figure, une seule fois, et aucun nom n'est vide.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Liste {
    pub commandes: Vec<Commande>,
    pub dossiers: Vec<String>,
}

impl Liste {
    fn normaliser(mut self) -> Liste {
        let mut vus: Vec<String> = Vec::new();
        for d in self.dossiers.iter().chain(self.commandes.iter().map(|c| &c.dossier)) {
            if !d.is_empty() && !vus.contains(d) && vus.len() < MAX_DOSSIERS {
                vus.push(d.clone());
            }
        }
        // Au-delà de la borne, une commande retombe hors dossier plutôt que
        // de citer un dossier absent.
        for c in &mut self.commandes {
            if !vus.contains(&c.dossier) {
                c.dossier.clear();
            }
        }
        self.dossiers = vus;
        self
    }
}

fn nom_de_dossier(nom: &str) -> String {
    nom.trim().chars().take(MAX_NOM).collect()
}

/// Une commande lue d'une valeur JSON ou TOML (les deux ont la même forme
/// d'accès) — `None` si elle est inutilisable (texte vide).
fn commande(nom: &str, texte: &str, executer: Option<bool>, dossier: &str) -> Option<Commande> {
    let texte: String = texte.chars().take(MAX_TEXTE).collect();
    if texte.trim().is_empty() {
        return None;
    }
    let nom: String = nom.trim().chars().take(MAX_NOM).collect();
    Some(Commande {
        nom,
        texte,
        executer: executer.unwrap_or(true),
        dossier: nom_de_dossier(dossier),
    })
}

/// La liste envoyée par la page : `{ commandes: [{ nom, texte, executer,
/// dossier }], dossiers: [noms] }`.
pub fn depuis_json(octets: &[u8]) -> Result<Liste, String> {
    let valeur: serde_json::Value = serde_json::from_slice(octets).map_err(|e| format!("JSON illisible : {e}"))?;
    let lignes = valeur.get("commandes").and_then(|v| v.as_array()).ok_or("`commandes` était attendu")?;
    if lignes.len() > MAX_COMMANDES {
        return Err(format!("trop de commandes ({} > {MAX_COMMANDES})", lignes.len()));
    }
    let s = |v: &serde_json::Value, cle: &str| v.get(cle).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let commandes = lignes
        .iter()
        .filter_map(|v| {
            let executer = v.get("executer").and_then(|x| x.as_bool());
            commande(&s(v, "nom"), &s(v, "texte"), executer, &s(v, "dossier"))
        })
        .collect();
    let dossiers = valeur
        .get("dossiers")
        .and_then(|v| v.as_array())
        .map(|l| l.iter().filter_map(|d| d.as_str()).map(nom_de_dossier).collect())
        .unwrap_or_default();
    Ok(Liste { commandes, dossiers }.normaliser())
}

pub fn vers_json(liste: &Liste) -> serde_json::Value {
    let commandes: Vec<serde_json::Value> = liste
        .commandes
        .iter()
        .map(|c| {
            json!({
                "nom": c.nom,
                "texte": c.texte,
                "executer": c.executer,
                "dossier": c.dossier,
            })
        })
        .collect();
    json!({ "commandes": commandes, "dossiers": liste.dossiers })
}

fn depuis_toml(texte: &str) -> Liste {
    let Ok(table) = texte.parse::<toml::Table>() else { return Liste::default() };
    let s = |v: &toml::Value, cle: &str| v.get(cle).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let commandes = table
        .get("commandes")
        .and_then(|v| v.as_array())
        .map(|lignes| {
            lignes
                .iter()
                .take(MAX_COMMANDES)
                .filter_map(|v| {
                    let executer = v.get("executer").and_then(|x| x.as_bool());
                    commande(&s(v, "nom"), &s(v, "texte"), executer, &s(v, "dossier"))
                })
                .collect()
        })
        .unwrap_or_default();
    let dossiers = table
        .get("dossiers")
        .and_then(|v| v.as_array())
        .map(|l| l.iter().filter_map(|d| d.as_str()).map(nom_de_dossier).collect())
        .unwrap_or_default();
    Liste { commandes, dossiers }.normaliser()
}

fn vers_toml(liste: &Liste) -> String {
    use toml::Value;
    let lignes = liste
        .commandes
        .iter()
        .map(|c| {
            let mut t = toml::Table::new();
            t.insert("nom".into(), Value::String(c.nom.clone()));
            t.insert("texte".into(), Value::String(c.texte.clone()));
            t.insert("executer".into(), Value::Boolean(c.executer));
            if !c.dossier.is_empty() {
                t.insert("dossier".into(), Value::String(c.dossier.clone()));
            }
            Value::Table(t)
        })
        .collect();
    let mut table = toml::Table::new();
    table.insert("dossiers".into(), Value::Array(liste.dossiers.iter().cloned().map(Value::String).collect()));
    table.insert("commandes".into(), Value::Array(lignes));
    toml::to_string(&table).unwrap_or_default()
}

thread_local! {
    static ETAT: RefCell<Liste> = RefCell::new(Liste::default());
}

/// Relit `commandes.toml` — une fois, au démarrage (voir `app::run`).
pub fn charger() {
    let texte = config::config_path(FICHIER).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    ETAT.with(|e| *e.borrow_mut() = depuis_toml(&texte));
}

pub fn json() -> serde_json::Value {
    ETAT.with(|e| vers_json(&e.borrow()))
}

/// Remplace la liste par celle que la page a envoyée (le corps déposé sur
/// `route`), l'écrit, et repousse l'état aux DEUX panneaux.
pub fn remplacer_depuis(route: &str) {
    let Some(octets) = crate::win32::cef_data::prendre(route) else {
        crate::diag!("[commandes] aucune liste reçue");
        return;
    };
    match depuis_json(&octets) {
        Ok(liste) => {
            if !config::write_config_atomique(FICHIER, vers_toml(&liste)) {
                crate::diag!("[commandes] {FICHIER} n'a pas pu être écrit");
            }
            ETAT.with(|e| *e.borrow_mut() = liste);
        }
        Err(message) => crate::diag!("[commandes] liste refusée : {message}"),
    }
    crate::win32::cef_terminal::refresh_if_visible();
    crate::win32::cef_ssh::refresh_if_visible();
}

/// Ajoute à `actuelle` les commandes de `importee` qu'elle n'a pas déjà (même
/// texte dans le même dossier), et ses dossiers à la suite des siens. Rien n'est
/// retiré ni remplacé. Rend la liste et le nombre de commandes ajoutées.
fn fusionner(actuelle: &Liste, importee: Liste) -> (Liste, usize) {
    let mut commandes = actuelle.commandes.clone();
    let mut ajoutees = 0;
    for c in importee.commandes {
        if commandes.len() >= MAX_COMMANDES {
            break;
        }
        if commandes.iter().any(|x| x.texte == c.texte && x.dossier == c.dossier) {
            continue;
        }
        commandes.push(c);
        ajoutees += 1;
    }
    let dossiers = actuelle.dossiers.iter().chain(&importee.dossiers).cloned().collect();
    (Liste { commandes, dossiers }.normaliser(), ajoutees)
}

/// **SAUVEGARDE LES COMMANDES DANS UN FICHIER** — DEMANDE EXPLICITE
/// (2026-10-11) : « il faudrait que je puisse sauvegarder mes commandes ssh et
/// de terminaux, personnalisées ». Le fichier a le format de `commandes.toml` :
/// « Importer » le reprend, ici ou sur un autre poste.
pub fn exporter(proprietaire: windows::Win32::Foundation::HWND) {
    let date = crate::sauvegarde_config::maintenant(false);
    let Some(chemin) = crate::win32::file_dialog::enregistrer_sous(
        proprietaire,
        "Exporter les commandes",
        &format!("commandes-{}", &date[..10]),
        "toml",
        "Commandes NeuraDesk (.toml)",
    ) else {
        return; // Annuler n'est pas une erreur.
    };
    let (texte, nombre) = ETAT.with(|e| (vers_toml(&e.borrow()), e.borrow().commandes.len()));
    match std::fs::write(&chemin, texte) {
        Ok(()) => crate::diag!("[commandes] export : {nombre} commandes dans « {} »", chemin.display()),
        Err(err) => crate::diag!("[commandes] export : « {} » N'A PAS PU ÊTRE ÉCRIT ({err})", chemin.display()),
    }
}

/// **REPREND UN FICHIER EXPORTÉ** et l'AJOUTE à la liste (voir [`fusionner`]) :
/// importer ne fait jamais perdre une commande.
pub fn importer(proprietaire: windows::Win32::Foundation::HWND) {
    let Some(chemin) =
        crate::win32::file_dialog::ouvrir(proprietaire, "Importer des commandes", "toml", "Commandes NeuraDesk (.toml)")
    else {
        return;
    };
    let texte = match std::fs::read_to_string(&chemin) {
        Ok(t) => t,
        Err(err) => {
            crate::diag!("[commandes] import : « {} » illisible ({err})", chemin.display());
            return;
        }
    };
    let (liste, ajoutees) = ETAT.with(|e| fusionner(&e.borrow(), depuis_toml(&texte)));
    crate::diag!("[commandes] import : {ajoutees} commandes ajoutées depuis « {} »", chemin.display());
    if !config::write_config_atomique(FICHIER, vers_toml(&liste)) {
        crate::diag!("[commandes] {FICHIER} n'a pas pu être écrit");
    }
    ETAT.with(|e| *e.borrow_mut() = liste);
    crate::win32::cef_terminal::refresh_if_visible();
    crate::win32::cef_ssh::refresh_if_visible();
}

/// Le script du volet, pour les tests des deux panneaux qui le chargent.
#[cfg(test)]
pub const VOLET_JS: &str = include_str!("../../assets/popups/volet_commandes.js");

#[cfg(test)]
mod tests {
    use super::*;

    fn c(nom: &str, texte: &str, executer: bool, dossier: &str) -> Commande {
        Commande { nom: nom.into(), texte: texte.into(), executer, dossier: dossier.into() }
    }

    fn liste(commandes: Vec<Commande>, dossiers: &[&str]) -> Liste {
        Liste { commandes, dossiers: dossiers.iter().map(|d| d.to_string()).collect() }
    }

    #[test]
    fn toml_fait_l_aller_retour() {
        let l = liste(
            vec![
                c("État git", "git status", true, ""),
                c("Multi", "cd /var/log\ntail -f \"syslog\" # ' \\", false, "Serveurs"),
                c("", "dir", true, "Windows"),
            ],
            &["Windows", "Serveurs", "Vide"],
        );
        assert_eq!(depuis_toml(&vers_toml(&l)), l);
    }

    #[test]
    fn json_fait_l_aller_retour() {
        let l = liste(vec![c("a", "ls -la", false, "D")], &["D", "E"]);
        let octets = vers_json(&l).to_string().into_bytes();
        assert_eq!(depuis_json(&octets).unwrap(), l);
    }

    #[test]
    fn une_commande_sans_texte_est_ecartee_et_les_champs_absents_ont_un_defaut() {
        let octets = br#"{"commandes":[{"nom":"vide","texte":"   "},{"texte":"whoami"}]}"#;
        assert_eq!(depuis_json(octets).unwrap(), liste(vec![c("", "whoami", true, "")], &[]));
    }

    /// Un dossier cité par une commande mais absent de la liste y est ajouté ;
    /// les doublons et les noms vides disparaissent.
    #[test]
    fn les_dossiers_sont_normalises() {
        let octets =
            br#"{"commandes":[{"texte":"a","dossier":" B "},{"texte":"b","dossier":"C"}],"dossiers":["A","","A","B"]}"#;
        let l = depuis_json(octets).unwrap();
        assert_eq!(l.dossiers, ["A", "B", "C"]);
        assert_eq!(l.commandes[0].dossier, "B");
    }

    #[test]
    fn les_bornes_tiennent() {
        let long = "x".repeat(MAX_TEXTE + 50);
        let nom = "n".repeat(MAX_NOM + 50);
        let octets = json!({ "commandes": [{ "nom": nom, "texte": long }] }).to_string().into_bytes();
        let l = depuis_json(&octets).unwrap();
        assert_eq!(l.commandes[0].texte.chars().count(), MAX_TEXTE);
        assert_eq!(l.commandes[0].nom.chars().count(), MAX_NOM);
        let trop: Vec<_> = (0..=MAX_COMMANDES).map(|_| json!({ "texte": "a" })).collect();
        assert!(depuis_json(json!({ "commandes": trop }).to_string().as_bytes()).is_err());
        let dossiers: Vec<String> = (0..MAX_DOSSIERS + 10).map(|i| format!("d{i}")).collect();
        let l = depuis_json(json!({ "commandes": [], "dossiers": dossiers }).to_string().as_bytes()).unwrap();
        assert_eq!(l.dossiers.len(), MAX_DOSSIERS);
    }

    /// Le format d'avant les dossiers se relit : les commandes sans `dossier`
    /// restent hors dossier.
    #[test]
    fn l_ancien_commandes_toml_se_relit() {
        let ancien = "[[commandes]]\nnom = \"g\"\ntexte = \"git status\"\nexecuter = true\nportee = \"tous\"\n";
        assert_eq!(depuis_toml(ancien), liste(vec![c("g", "git status", true, "")], &[]));
    }

    /// Une commande réservée à un panneau (`portee = "ssh"`, d'avant le partage)
    /// se relit comme les autres, et la réécriture ne garde plus la portée.
    #[test]
    fn une_ancienne_portee_est_oubliee() {
        let ancien = "[[commandes]]\ntexte = \"uptime\"\nexecuter = true\nportee = \"ssh\"\n";
        let l = depuis_toml(ancien);
        assert_eq!(l, liste(vec![c("", "uptime", true, "")], &[]));
        assert!(!vers_toml(&l).contains("portee"));
        assert!(vers_json(&l)["commandes"][0].get("portee").is_none());
    }

    /// Importer AJOUTE : les commandes déjà là (même texte, même dossier) ne
    /// sont pas doublées, les dossiers nouveaux suivent les anciens.
    #[test]
    fn importer_ajoute_sans_doubler() {
        let actuelle = liste(vec![c("a", "ls", true, ""), c("b", "df -h", true, "Srv")], &["Srv"]);
        let fichier = vers_toml(&liste(
            vec![c("a bis", "ls", false, ""), c("", "df -h", true, "Autre"), c("u", "uptime", true, "Srv")],
            &["Autre", "Srv"],
        ));
        let (l, ajoutees) = fusionner(&actuelle, depuis_toml(&fichier));
        assert_eq!(ajoutees, 2);
        assert_eq!(l.dossiers, ["Srv", "Autre"]);
        assert_eq!(
            l.commandes,
            [c("a", "ls", true, ""), c("b", "df -h", true, "Srv"), c("", "df -h", true, "Autre"), c("u", "uptime", true, "Srv")]
        );
    }

    #[test]
    fn importer_respecte_la_borne() {
        let pleine = liste((0..MAX_COMMANDES).map(|i| c("", &format!("echo {i}"), true, "")).collect(), &[]);
        let (l, ajoutees) = fusionner(&pleine, liste(vec![c("", "nouvelle", true, "")], &[]));
        assert_eq!((l.commandes.len(), ajoutees), (MAX_COMMANDES, 0));
    }

    #[test]
    fn un_corps_illisible_est_refuse() {
        assert!(depuis_json(b"pas du json").is_err());
        assert!(depuis_json(br#"[{"texte":"a"}]"#).is_err());
        assert_eq!(depuis_toml("[[[ cassé"), Liste::default());
    }
}
