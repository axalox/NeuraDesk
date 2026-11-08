//! Automatisations « quand… alors… » — **LE MOTEUR ET LE STOCKAGE (lot 1)**.
//!
//! Une automatisation = un DÉCLENCHEUR (« quand… ») + des CONDITIONS
//! facultatives (« seulement si… ») + une ou plusieurs ACTIONS (« alors… »).
//! Exemple : *quand* il est 18 h, *seulement* du lundi au vendredi, *alors*
//! lancer la commande enregistrée « sauvegarde » et afficher une notification.
//!
//! # Ce que ce module fait, et ce qu'il ne fait pas
//!
//! Il porte le MODÈLE, le fichier `%APPDATA%\NeuraDesk\automatisations.toml`,
//! et le MOTEUR qui décide, à chaque frame, ce qui doit se déclencher. Il
//! exécute lui-même les actions qui ne dépendent que de Windows ; celles qui
//! touchent l'état du dock (`Interface`, `Commande`) lui sont RENDUES —
//! `executer` rend `None` pour elles, et `app::run` les applique là où il a la
//! main (même motif que la bascule de thème, voir `theme_toggle_icon_index`).
//!
//! ⛔ **L'ÉVALUATION EST PURE, LA MESURE DU TEMPS NE L'EST PAS.** `Moteur::evaluer`
//! ne lit ni l'horloge ni la liste des processus : il travaille sur l'`Instantane`
//! que le moteur porte. C'est ce qui permet de l'éprouver sur une heure et une
//! liste de programmes CHOISIES, sans attendre 18 h ni lancer quoi que ce soit.
//! `rafraichir` est la seule porte par laquelle le monde extérieur entre.
//!
//! # Trois pièges que ce module désamorce
//!
//! 1. **Un déclencheur horaire sonne UNE fois par minute** — la boucle tourne à
//!    60 Hz, il sonnerait donc soixante fois. La minute du dernier déclenchement
//!    est retenue PAR AUTOMATISATION (son nom est la clé).
//! 2. **Le tout premier instantané n'est qu'une BASE** : rien n'y est « apparu »
//!    ni « disparu ». Sans cela, démarrer NeuraDesk avec Discord déjà ouvert
//!    déclencherait « quand Discord s'ouvre ».
//! 3. **La liste des processus n'est PAS relue à chaque frame** — un instantané
//!    Toolhelp coûte cher (voir `tray_icons::executables_en_cours`). Elle est
//!    rafraîchie au plus une fois par seconde ; l'heure, elle, à chaque frame.
//!
//! ⚠️ **UNE ACTION NE S'EXÉCUTE PAS SOUS L'EMPRUNT DU MOTEUR.** `tick` décide et
//! rend la main ; l'appelant exécute (`executer`), puis écrit au journal
//! (`noter`). Une action écrit au registre ou lance un processus : elle n'a rien
//! à faire sous un `borrow_mut`.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::Path;
use std::time::{Duration, Instant};

// ⚠️ La MACRO `json!` et la FONCTION `json()` de ce module cohabitent : deux
// espaces de noms distincts, mais le nom est le même — à ne pas confondre en
// lisant.
use serde_json::json;

use crate::win32::{appicon, night_light, theme, tray_icons};

/// Le fichier, dans `%APPDATA%\NeuraDesk\` (voir `config::config_path`).
pub const FICHIER: &str = "automatisations.toml";

/// Bornes de chargement : un fichier écrit à la main ne doit pas pouvoir faire
/// travailler la boucle par frame sans fin.
const MAX_AUTOMATISATIONS: usize = 200;
const MAX_ACTIONS: usize = 16;
const MAX_CONDITIONS: usize = 8;
const MAX_TEXTE: usize = 512;
/// Le journal garde les dernières exécutions (le dash du lot 2 les affiche).
const MAX_JOURNAL: usize = 50;
/// Délai minimal entre deux relevés de la liste des processus.
const PERIODE_SCAN: Duration = Duration::from_secs(1);

// ---------------------------------------------------------------------------
// Les jours
// ---------------------------------------------------------------------------

/// **UN ENSEMBLE DE JOURS, EN MASQUE DE BITS** — lundi = bit 0 … dimanche = bit 6.
///
/// Les bits sont dans l'ordre de [`Jours::NOMS`], qui est aussi celui du
/// `jour_semaine` de [`Instantane`] (0 = lundi). C'est l'écriture dans le
/// fichier qui est en NOMS, pas le moteur : un fichier édité à la main doit
/// rester lisible, et « lundi » y vaut mieux que « 1 ».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Jours(u8);

impl Jours {
    pub const TOUS: Jours = Jours(0b0111_1111);
    /// Dans l'ordre des bits — donc aussi l'ordre de la semaine.
    pub const NOMS: [&'static str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];

    #[cfg(test)]
    pub fn depuis_jour(jour: u8) -> Jours {
        Jours(1 << (jour % 7))
    }

    pub fn contient(self, jour: u8) -> bool {
        self.0 & (1 << (jour % 7)) != 0
    }

    pub fn est_vide(self) -> bool {
        self.0 == 0
    }

    /// Les noms lus dans le fichier → le masque.
    ///
    /// ⚠️ **UN NOM INCONNU EST ÉCARTÉ, PAS REFUSÉ** : une faute de frappe sur
    /// « mardi » ne doit pas emporter l'automatisation entière.
    pub fn depuis_noms(noms: &[String]) -> Jours {
        let mut masque = 0u8;
        for nom in noms {
            if let Some(i) = Jours::NOMS.iter().position(|x| x.eq_ignore_ascii_case(nom.trim())) {
                masque |= 1 << i;
            }
        }
        Jours(masque)
    }

    pub fn noms(self) -> Vec<String> {
        Jours::NOMS
            .iter()
            .enumerate()
            .filter(|(i, _)| self.contient(*i as u8))
            .map(|(_, n)| (*n).to_string())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Le modèle
// ---------------------------------------------------------------------------

/// Ce qui met l'automatisation en marche.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Declencheur {
    /// À `heure:minute`, les `jours` donnés.
    Heure { heure: u8, minute: u8, jours: Jours },
    /// Une fois, au démarrage de NeuraDesk.
    Demarrage,
    /// Quand `exe` apparaît (`ouvre`) ou disparaît de la liste des processus.
    /// Le nom est comparé sous la forme `tray_icons::exe_normalise` (minuscules,
    /// sans « .exe ») : « C:\…\Discord.exe » et « discord » désignent le même
    /// programme.
    Programme { exe: String, ouvre: bool },
}

impl Declencheur {
    fn sorte(&self) -> &'static str {
        match self {
            Declencheur::Heure { .. } => "heure",
            Declencheur::Demarrage => "demarrage",
            Declencheur::Programme { .. } => "programme",
        }
    }

    fn est_horaire(&self) -> bool {
        matches!(self, Declencheur::Heure { .. })
    }
}

/// Ce qu'il faut en plus du déclencheur pour que les actions partent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Condition {
    /// Une plage horaire, bornes INCLUSES. `depuis > jusqua` est une plage qui
    /// franchit minuit (22:00 → 06:00) — le cas courant d'un « la nuit », et
    /// surtout pas une faute.
    Plage { depuis: (u8, u8), jusqua: (u8, u8) },
    Jours(Jours),
    ProgrammeEnCours(String),
}

impl Condition {
    fn tenue(&self, i: &Instantane) -> bool {
        match self {
            Condition::Plage { depuis, jusqua } => {
                let maintenant = (i.heure, i.minute);
                if depuis <= jusqua {
                    maintenant >= *depuis && maintenant <= *jusqua
                } else {
                    maintenant >= *depuis || maintenant <= *jusqua
                }
            }
            Condition::Jours(jours) => jours.contient(i.jour_semaine),
            Condition::ProgrammeEnCours(exe) => i.exes.contains(&tray_icons::exe_normalise(exe)),
        }
    }
}

/// Ce qu'une action demande à un réglage à DEUX états.
///
/// ⚠️ **« BASCULER » EXISTE PARCE QUE L'ÉTAT EST RELU, JAMAIS MÉMORISÉ** : le
/// thème et l'éclairage nocturne appartiennent à Windows, et l'utilisateur peut
/// les changer entre-temps. Le moteur ne garde donc aucun état de ces réglages ;
/// `Basculer` interroge Windows au moment d'agir (même doctrine que
/// `theme::toggle_dark_mode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeVoulu {
    Clair,
    Sombre,
    Basculer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LumiereVoulue {
    Allumer,
    Eteindre,
    Basculer,
}

/// Comment une commande enregistrée est exécutée.
///
/// DEMANDE EXPLICITE (2026-10-04) : le choix est fait **par automatisation**, pas
/// globalement — une sauvegarde veut un onglet visible où lire les erreurs, une
/// resynchronisation silencieuse veut l'arrière-plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeCommande {
    /// Dans un onglet du Terminal, où la sortie et les erreurs se lisent.
    Visible,
    /// Sans fenêtre, avec une ligne au journal.
    ArrierePlan,
}

impl ModeCommande {
    fn nom(self) -> &'static str {
        match self {
            ModeCommande::Visible => "visible",
            ModeCommande::ArrierePlan => "arriere-plan",
        }
    }

    fn depuis_nom(nom: &str) -> ModeCommande {
        match nom.trim().to_ascii_lowercase().as_str() {
            "arriere-plan" | "arrière-plan" | "arriereplan" => ModeCommande::ArrierePlan,
            _ => ModeCommande::Visible,
        }
    }
}

/// Ce que l'automatisation FAIT.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Un programme, un fichier ou une **URL** (`ShellExecuteW` accepte les trois).
    Lancer { chemin: String, arguments: String },
    /// Une commande enregistrée (le volet des terminaux, voir `win32::commandes`).
    Commande { nom: String, mode: ModeCommande },
    Theme(ThemeVoulu),
    EclairageNocturne(LumiereVoulue),
    Notifier { titre: String, message: String, son: bool },
    /// Masquer (`true`) ou réafficher (`false`) l'interface.
    Interface { masquer: bool },
}

impl Action {
    fn sorte(&self) -> &'static str {
        match self {
            Action::Lancer { .. } => "lancer",
            Action::Commande { .. } => "commande",
            Action::Theme(_) => "theme",
            Action::EclairageNocturne(_) => "eclairage",
            Action::Notifier { .. } => "notifier",
            Action::Interface { .. } => "interface",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Automatisation {
    /// **LE NOM EST LA CLÉ** : c'est lui qui retient la minute du dernier
    /// déclenchement horaire, et c'est lui que le journal nomme. Deux
    /// automatisations du même nom seraient indiscernables — `depuis_toml` écarte
    /// donc la seconde.
    pub nom: String,
    pub actif: bool,
    pub declencheur: Declencheur,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Action>,
}

/// Ce que le moteur a décidé, pour une automatisation donnée.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declenchement {
    pub automatisation: String,
    pub action: Action,
}

/// Une ligne du journal des exécutions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entree {
    /// `HH:MM` au moment de l'exécution.
    pub quand: String,
    pub automatisation: String,
    pub resume: String,
    pub reussi: bool,
}

// ---------------------------------------------------------------------------
// L'instant présent
// ---------------------------------------------------------------------------

/// **TOUT CE QUE LE MOTEUR A BESOIN DE SAVOIR DU MONDE, EN UNE VALEUR.** Aucune
/// de ces données n'est lue par `evaluer` : c'est ce qui rend l'évaluation
/// éprouvable sur une heure choisie.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Instantane {
    pub annee: i32,
    pub mois: u32,
    pub jour: u32,
    /// 0 = lundi … 6 = dimanche (l'ordre de [`Jours::NOMS`]).
    pub jour_semaine: u8,
    pub heure: u8,
    pub minute: u8,
    /// Les exécutables en cours, sous la forme `tray_icons::exe_normalise`.
    pub exes: BTreeSet<String>,
}

impl Instantane {
    fn cle_minute(&self) -> (i32, u32, u32, u8, u8) {
        (self.annee, self.mois, self.jour, self.heure, self.minute)
    }

    fn horloge(&self) -> String {
        format!("{:02}:{:02}", self.heure, self.minute)
    }

    /// L'heure locale seule — les processus sont relevés à part, et moins souvent.
    fn heure_locale() -> Instantane {
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        Instantane {
            annee: t.wYear as i32,
            mois: t.wMonth as u32,
            jour: t.wDay as u32,
            // `SYSTEMTIME::wDayOfWeek` compte dimanche = 0 ; `Jours` compte lundi = 0.
            jour_semaine: ((t.wDayOfWeek + 6) % 7) as u8,
            heure: t.wHour as u8,
            minute: t.wMinute as u8,
            exes: BTreeSet::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Le moteur
// ---------------------------------------------------------------------------

/// **LE MOTEUR — il porte l'instant présent ET ce qu'il a déjà fait.**
#[derive(Debug, Default)]
pub struct Moteur {
    instantane: Instantane,
    /// **LES ARÊTES DU DERNIER RELEVÉ, PAS ENCORE CONSOMMÉES.** C'est la
    /// différence entre deux relevés qui fait une ouverture et une fermeture, et
    /// elle n'est calculée qu'au relevé — une fois par seconde, pas par frame.
    /// `evaluer` les PREND : une même ouverture ne déclenche donc qu'une fois,
    /// même si soixante évaluations suivent le même relevé.
    apparus: BTreeSet<String>,
    disparus: BTreeSet<String>,
    /// Le dernier relevé des processus, et quand il a été fait.
    dernier_scan: Option<Instant>,
    /// La minute du dernier déclenchement HORAIRE, par nom d'automatisation.
    dernieres: HashMap<String, (i32, u32, u32, u8, u8)>,
    /// Faux tant qu'aucun instantané n'a été pris : le premier ne sert que de base.
    amorce: bool,
    demarrage_fait: bool,
    journal: VecDeque<Entree>,
}

impl Moteur {
    /// **LE SEUL ENDROIT PAR OÙ LE MONDE EXTÉRIEUR ENTRE** : l'heure à CHAQUE
    /// appel, la liste des processus au plus une fois par seconde.
    pub fn rafraichir(&mut self) {
        let heure = Instantane::heure_locale();
        let perime = self.dernier_scan.map(|t| t.elapsed() >= PERIODE_SCAN).unwrap_or(true);
        let releve = perime.then(|| {
            self.dernier_scan = Some(Instant::now());
            tray_icons::executables_en_cours().into_iter().collect::<BTreeSet<String>>()
        });

        // L'heure est recopiée à CHAQUE frame ; les processus, seulement au relevé.
        self.instantane.annee = heure.annee;
        self.instantane.mois = heure.mois;
        self.instantane.jour = heure.jour;
        self.instantane.jour_semaine = heure.jour_semaine;
        self.instantane.heure = heure.heure;
        self.instantane.minute = heure.minute;
        if let Some(exes) = releve {
            self.mesurer(exes);
        }
    }

    /// Pour les tests, et pour tout appelant qui voudrait une heure choisie.
    /// L'instantané posé vaut un RELEVÉ : ses processus remplacent les précédents.
    ///
    /// ⛔ **LA LISTE PRÉCÉDENTE EST MISE DE CÔTÉ AVANT L'AFFECTATION**, et c'est
    /// tout l'objet des deux `take` : `self.instantane = instantane` écraserait
    /// les processus du relevé d'avant, que `mesurer` doit encore comparer. Sans
    /// cela il n'y a JAMAIS d'arête — et comme plus rien ne distingue un
    /// programme déjà lancé d'un programme qui vient de s'ouvrir, le défaut se
    /// cache derrière des tests qui passent pour la mauvaise raison.
    #[cfg(test)]
    pub fn poser_instantane(&mut self, mut instantane: Instantane) {
        let exes = std::mem::take(&mut instantane.exes);
        let anciens = std::mem::take(&mut self.instantane.exes);
        self.instantane = instantane;
        self.instantane.exes = anciens;
        self.mesurer(exes);
    }

    /// **UN RELEVÉ DU MONDE** : les processus en cours remplacent les précédents,
    /// et la DIFFÉRENCE des deux devient les arêtes à consommer.
    ///
    /// ⛔ **LE TOUT PREMIER RELEVÉ N'EST QU'UNE BASE** : rien n'y est apparu ni
    /// disparu. Sans cela, démarrer NeuraDesk avec Discord déjà ouvert
    /// déclencherait « quand Discord s'ouvre ».
    ///
    /// ⚠️ **LES ARÊTES S'ACCUMULENT** si deux relevés se suivent sans qu'aucune
    /// évaluation ne les prenne : c'est voulu — mieux vaut une ouverture en
    /// retard qu'une ouverture perdue.
    fn mesurer(&mut self, exes: BTreeSet<String>) {
        let anciens = std::mem::replace(&mut self.instantane.exes, exes);
        if self.amorce {
            self.apparus.extend(self.instantane.exes.difference(&anciens).cloned());
            self.disparus.extend(anciens.difference(&self.instantane.exes).cloned());
        }
        self.amorce = true;
    }

    pub fn journal(&self) -> Vec<Entree> {
        self.journal.iter().cloned().collect()
    }

    /// Ajoute une ligne au journal, en ne gardant que les plus récentes.
    pub fn noter(&mut self, automatisation: &str, resume: Result<String, String>) {
        let (resume, reussi) = match resume {
            Ok(s) => (s, true),
            Err(e) => (e, false),
        };
        if self.journal.len() >= MAX_JOURNAL {
            self.journal.pop_front();
        }
        self.journal.push_back(Entree {
            quand: self.instantane.horloge(),
            automatisation: automatisation.to_string(),
            resume: resume.chars().take(MAX_TEXTE).collect(),
            reussi,
        });
    }

    /// **CE QUI DOIT SE DÉCLENCHER MAINTENANT — fonction de l'instantané seul.**
    ///
    /// Elle met à jour ce qu'elle a déjà fait (la minute retenue, la base des
    /// processus), donc elle prend `&mut self` — mais elle ne lit rien du monde.
    ///
    /// ⚠️ **DEUX TEMPS, ET C'EST NÉCESSAIRE** : la décision se prend en LISANT
    /// (`&self.instantane`), puis la minute est retenue en ÉCRIVANT. Tout faire
    /// d'un tenant demanderait d'emprunter `self` deux fois.
    pub fn evaluer(&mut self, autos: &[Automatisation]) -> Vec<Declenchement> {
        let demarrage = !self.demarrage_fait;
        self.demarrage_fait = true;
        let cle = self.instantane.cle_minute();
        // Les arêtes sont PRISES : le relevé qui les a produites ne les produira
        // pas une seconde fois, même si l'évaluation est rappelée soixante fois.
        let apparus = std::mem::take(&mut self.apparus);
        let disparus = std::mem::take(&mut self.disparus);

        // Les automatisations qui partent, et si leur déclencheur est horaire.
        let mut partants: Vec<(String, bool)> = Vec::new();
        {
            let instantane = &self.instantane;

            for auto in autos {
                if !auto.actif || auto.actions.is_empty() {
                    continue;
                }
                let declenche = match &auto.declencheur {
                    Declencheur::Heure { heure, minute, jours } => {
                        instantane.heure == *heure
                            && instantane.minute == *minute
                            && jours.contient(instantane.jour_semaine)
                            && self.dernieres.get(&auto.nom).copied() != Some(cle)
                    }
                    Declencheur::Demarrage => demarrage,
                    Declencheur::Programme { exe, ouvre } => {
                        let exe = tray_icons::exe_normalise(exe);
                        if *ouvre {
                            apparus.contains(&exe)
                        } else {
                            disparus.contains(&exe)
                        }
                    }
                };
                if declenche && auto.conditions.iter().all(|c| c.tenue(instantane)) {
                    partants.push((auto.nom.clone(), auto.declencheur.est_horaire()));
                }
            }
        }

        let mut ordres = Vec::new();
        for (nom, horaire) in &partants {
            // La minute n'est retenue QUE si le déclenchement a bien eu lieu : une
            // condition qui se dément à 18:00 pile peut encore devenir vraie dans la
            // même minute, et l'automatisation doit alors partir.
            if *horaire {
                self.dernieres.insert(nom.clone(), cle);
            }
            if let Some(auto) = autos.iter().find(|a| &a.nom == nom) {
                for action in auto.actions.iter().take(MAX_ACTIONS) {
                    ordres.push(Declenchement { automatisation: nom.clone(), action: action.clone() });
                }
            }
        }

        // Une automatisation renommée ou retirée ne doit pas laisser sa minute
        // retenue pour toujours.
        self.dernieres.retain(|nom, _| autos.iter().any(|a| &a.nom == nom));
        ordres
    }
}

// ---------------------------------------------------------------------------
// L'état du module
// ---------------------------------------------------------------------------

thread_local! {
    static ETAT: RefCell<Vec<Automatisation>> = const { RefCell::new(Vec::new()) };
    static MOTEUR: RefCell<Moteur> = RefCell::new(Moteur::default());
    /// **CE QUI RESTE À EXÉCUTER, DÉCIDÉ MAIS PAS ENCORE APPLIQUÉ.** Le moteur et
    /// le bouton « Tester » y déposent ; `app::run` draine une fois par frame.
    ///
    /// ⛔ **UNE FILE, PAS UN RETOUR DE FONCTION, ET C'EST DÉLIBÉRÉ** : `tester`
    /// est appelé depuis le panneau (`cef_automatisations`), qui n'a AUCUN accès à
    /// l'état du dock. Passer par la même file que le moteur est ce qui garantit
    /// qu'un test fait exactement ce que le déclenchement fera — un chemin
    /// d'exécution différent ne prouverait rien.
    static A_EXECUTER: RefCell<Vec<Declenchement>> = const { RefCell::new(Vec::new()) };
}

/// Met des ordres en file, sans jamais en perdre : deux appels dans la même frame
/// s'ajoutent (le moteur et un « Tester » peuvent tomber ensemble).
fn mettre_en_file(ordres: Vec<Declenchement>) {
    if ordres.is_empty() {
        return;
    }
    A_EXECUTER.with(|f| f.borrow_mut().extend(ordres));
}

/// **CE QUE L'APPLICATION DOIT APPLIQUER MAINTENANT** — vide la file.
pub fn prendre_a_executer() -> Vec<Declenchement> {
    A_EXECUTER.with(|f| std::mem::take(&mut *f.borrow_mut()))
}

/// Relit `automatisations.toml` — une fois, au démarrage (voir `app::run`).
pub fn charger() {
    let texte = crate::config::config_path(FICHIER)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let autos = depuis_toml(&texte);
    crate::diag!("[automatisations] {} chargée(s)", autos.len());
    ETAT.with(|e| *e.borrow_mut() = autos);
}

/// **LE POINT D'ENTRÉE DE LA BOUCLE PAR FRAME** : rafraîchit l'instant, évalue,
/// et MET EN FILE ce qu'il y a à faire — l'appelant draine ensuite par
/// [`prendre_a_executer`]. La décision et l'exécution sont séparées (voir la doc
/// de module).
pub fn tick() {
    ETAT.with(|e| {
        let autos = e.borrow();
        let ordres = MOTEUR.with(|m| {
            let mut m = m.borrow_mut();
            m.rafraichir();
            m.evaluer(&autos)
        });
        mettre_en_file(ordres);
    })
}

/// **LE BOUTON « TESTER »** : met en file les actions d'une automatisation sans
/// attendre son déclencheur.
///
/// ⚠️ **ELLES PASSENT PAR LA MÊME FILE QUE LE MOTEUR** : le test emprunte donc le
/// chemin d'exécution réel, y compris pour les actions que l'application doit
/// appliquer elle-même. Un « Tester » qui exécuterait les actions par un autre
/// chemin ne prouverait rien de ce qui se passera vraiment.
///
/// ⚠️ **L'ACTIVATION N'EST PAS CONSULTÉE** : on teste une automatisation
/// DÉSACTIVÉE, c'est même le cas d'usage — on l'essaie avant de l'allumer.
pub fn tester(nom: &str) -> Result<(), String> {
    ETAT.with(|e| {
        let autos = e.borrow();
        let Some(auto) = autos.iter().find(|a| a.nom == nom) else {
            return Err(format!("aucune automatisation nommée « {nom} »"));
        };
        if auto.actions.is_empty() {
            return Err("cette automatisation n'a aucune action".into());
        }
        mettre_en_file(
            auto.actions
                .iter()
                .map(|action| Declenchement { automatisation: auto.nom.clone(), action: action.clone() })
                .collect(),
        );
        Ok(())
    })
}

/// Inscrit au journal le sort d'un déclenchement.
pub fn noter(automatisation: &str, resume: Result<String, String>) {
    MOTEUR.with(|m| m.borrow_mut().noter(automatisation, resume));
}

/// **EXÉCUTE UNE ACTION — `None` quand elle appartient à l'application.**
///
/// `Interface` et `Commande` touchent l'état du dock (le masquage, les onglets du
/// Terminal) : le moteur ne les exécute pas lui-même, il les rend à l'appelant.
/// Rendre `None` plutôt qu'une erreur est délibéré : ce n'est pas un échec, c'est
/// un ordre qui n'est pas le nôtre.
///
/// ⚠️ **`Interface` EST MAINTENANT APPLIQUÉ**, par `app::run` (voir son drainage
/// des actions) : masquer l'interface demande le rectangle de l'applet de
/// masquage et un local de la boucle de rendu, réafficher demande d'emprunter le
/// chemin du clic sur l'onglet de retour. Rien de tout cela n'est atteignable
/// d'ici. `Commande` reste, elle, **non câblée**.
pub fn executer(action: &Action) -> Option<Result<String, String>> {
    match action {
        Action::Interface { .. } | Action::Commande { .. } => None,
        Action::Lancer { chemin, arguments } => {
            let chemin = chemin.trim();
            if chemin.is_empty() {
                return Some(Err("aucun chemin à lancer".into()));
            }
            if arguments.trim().is_empty() {
                appicon::launch(Path::new(chemin));
            } else {
                appicon::launch_with_args(Path::new(chemin), arguments);
            }
            Some(Ok(format!("lancé : {chemin}")))
        }
        Action::Theme(voulu) => Some(match regler_le_theme(*voulu) {
            Some(true) => Ok("thème : sombre".into()),
            Some(false) => Ok("thème : clair".into()),
            None => Err("le thème n'a pas pu être réglé".into()),
        }),
        Action::EclairageNocturne(voulu) => Some(match regler_l_eclairage(*voulu) {
            Some(true) => Ok("éclairage nocturne : allumé".into()),
            Some(false) => Ok("éclairage nocturne : éteint".into()),
            None => Err("l'éclairage nocturne n'a pas pu être réglé".into()),
        }),
        Action::Notifier { titre, message, son } => {
            if *son {
                jouer_la_sonnerie_du_systeme();
            }
            let titre = if titre.trim().is_empty() { "NeuraDesk" } else { titre.trim() };
            // **LE BANDEAU, TOUT DE SUITE** (voir `win32::bandeau`) : il s'efface
            // au bout de quelques secondes, et laisse le message dans
            // l'historique du clic droit de l'horloge. `montrer` n'ouvre RIEN —
            // il enregistre, et `app::run` ouvre (lui seul a la géométrie).
            crate::win32::bandeau::montrer(titre, message);

            // **LA TRACE DANS LE CENTRE DE NOTIFICATIONS** (voir `win32::bulle`).
            //
            // ⚠️ **LE RÉSUMÉ DIT « REMISE », PAS « AFFICHÉE », ET CE N'EST PAS UNE
            // PRÉCAUTION DE STYLE** : mesuré le 2026-10-04, `Shell_NotifyIcon`
            // rend `true` sur cette machine et **rien n'apparaît nulle part**
            // (voir la doc de module de `bulle`). Annoncer « notifiée » au journal
            // serait donc un mensonge que le code ne peut pas détecter.
            let remise = crate::win32::bulle::notifier(titre, message);
            let resume = if remise {
                format!("{titre} — {message} (remise à Windows)")
            } else {
                format!("{titre} — {message}")
            };
            Some(Ok(resume))
        }
    }
}

/// **LA SONNERIE DU SYSTÈME, SANS FICHIER À CHOISIR.**
///
/// ⛔ **POURQUOI `PlaySoundW` ICI, ALORS QUE `win32::son` PASSE PAR MCI.**
/// `son::jouer` lit un `.mp3` CHOISI PAR L'UTILISATEUR, ce que `PlaySoundW` ne
/// sait pas faire. Ici, le son n'est qu'un SIGNAL : on demande à Windows SA
/// sonnerie de notification, par son alias — aucun fichier, aucune hypothèse sur
/// ce qui est installé.
///
/// `SND_NODEFAULT` : un alias absent fait un SILENCE, jamais un « ding » de
/// remplacement. `SND_ASYNC` : la boucle de rendu n'attend pas la fin du son.
fn jouer_la_sonnerie_du_systeme() {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
    // SAFETY : l'alias est une chaîne statique, terminée par un zéro.
    let _ = unsafe { PlaySoundW(windows::core::w!("SystemNotification"), None, SND_ALIAS | SND_ASYNC | SND_NODEFAULT) };
}

/// ⚠️ Les trois réglages rendent l'état ATTEINT, pas la demande : `Basculer` peut
/// très bien avoir abouti à l'état déjà en place.
fn regler_le_theme(voulu: ThemeVoulu) -> Option<bool> {
    match voulu {
        ThemeVoulu::Sombre => theme::set_dark_mode(true).then_some(true),
        ThemeVoulu::Clair => theme::set_dark_mode(false).then_some(false),
        ThemeVoulu::Basculer => theme::toggle_dark_mode(),
    }
}

fn regler_l_eclairage(voulu: LumiereVoulue) -> Option<bool> {
    match voulu {
        LumiereVoulue::Basculer => night_light::toggle(),
        LumiereVoulue::Allumer => night_light::set_allume(true),
        LumiereVoulue::Eteindre => night_light::set_allume(false),
    }
}

// ---------------------------------------------------------------------------
// Le fichier
// ---------------------------------------------------------------------------

fn chaine(v: &toml::Value, cle: &str) -> String {
    v.get(cle).and_then(|x| x.as_str()).unwrap_or("").trim().chars().take(MAX_TEXTE).collect()
}

fn entier(v: &toml::Value, cle: &str, defaut: i64) -> i64 {
    v.get(cle).and_then(|x| x.as_integer()).unwrap_or(defaut)
}

fn booleen(v: &toml::Value, cle: &str, defaut: bool) -> bool {
    v.get(cle).and_then(|x| x.as_bool()).unwrap_or(defaut)
}

fn noms(v: &toml::Value, cle: &str) -> Vec<String> {
    v.get(cle)
        .and_then(|x| x.as_array())
        .map(|l| l.iter().filter_map(|x| x.as_str()).map(str::to_string).collect())
        .unwrap_or_default()
}

/// `"18:05"` → `(18, 5)`. `None` si l'écriture n'est pas une heure.
fn heure_en_clair(texte: &str) -> Option<(u8, u8)> {
    let (h, m) = texte.trim().split_once(':')?;
    let h: u8 = h.trim().parse().ok()?;
    let m: u8 = m.trim().parse().ok()?;
    (h < 24 && m < 60).then_some((h, m))
}

fn declencheur(v: &toml::Value) -> Option<Declencheur> {
    match chaine(v, "quand").as_str() {
        "demarrage" => Some(Declencheur::Demarrage),
        "programme" => {
            let exe = chaine(v, "exe");
            (!exe.is_empty()).then(|| Declencheur::Programme { exe, ouvre: chaine(v, "sens") != "ferme" })
        }
        "heure" => {
            let heure = entier(v, "heure", -1);
            let minute = entier(v, "minute", 0);
            if !(0..24).contains(&heure) || !(0..60).contains(&minute) {
                return None;
            }
            let jours = Jours::depuis_noms(&noms(v, "jours"));
            // ⚠️ Une liste VIDE vaut « tous les jours » : c'est ce que veut dire un
            // fichier qui ne dit rien des jours, et « aucun jour » n'est l'intention
            // de personne.
            let jours = if jours.est_vide() { Jours::TOUS } else { jours };
            Some(Declencheur::Heure { heure: heure as u8, minute: minute as u8, jours })
        }
        _ => None,
    }
}

fn condition(v: &toml::Value) -> Option<Condition> {
    match chaine(v, "sorte").as_str() {
        "plage" => {
            let depuis = heure_en_clair(&chaine(v, "depuis"))?;
            let jusqua = heure_en_clair(&chaine(v, "jusqua"))?;
            Some(Condition::Plage { depuis, jusqua })
        }
        "jours" => {
            let jours = Jours::depuis_noms(&noms(v, "jours"));
            (!jours.est_vide()).then_some(Condition::Jours(jours))
        }
        "programme" => {
            let exe = chaine(v, "exe");
            (!exe.is_empty()).then_some(Condition::ProgrammeEnCours(exe))
        }
        _ => None,
    }
}

fn action(v: &toml::Value) -> Option<Action> {
    match chaine(v, "sorte").as_str() {
        "lancer" => {
            let chemin = chaine(v, "chemin");
            (!chemin.is_empty()).then(|| Action::Lancer { chemin, arguments: chaine(v, "arguments") })
        }
        "commande" => {
            let nom = chaine(v, "nom");
            (!nom.is_empty()).then(|| Action::Commande { nom, mode: ModeCommande::depuis_nom(&chaine(v, "mode")) })
        }
        "theme" => Some(Action::Theme(match chaine(v, "valeur").as_str() {
            "clair" => ThemeVoulu::Clair,
            "sombre" => ThemeVoulu::Sombre,
            _ => ThemeVoulu::Basculer,
        })),
        "eclairage" => Some(Action::EclairageNocturne(match chaine(v, "valeur").as_str() {
            "allumer" => LumiereVoulue::Allumer,
            "eteindre" => LumiereVoulue::Eteindre,
            _ => LumiereVoulue::Basculer,
        })),
        "notifier" => Some(Action::Notifier {
            titre: chaine(v, "titre"),
            message: chaine(v, "message"),
            son: booleen(v, "son", true),
        }),
        "interface" => Some(Action::Interface { masquer: booleen(v, "masquer", true) }),
        _ => None,
    }
}

/// **LE FICHIER, RELU — et jamais refusé en bloc.**
///
/// Une automatisation dont le déclencheur est illisible est ÉCARTÉE, les autres
/// survivent : perdre toute la liste pour une faute de frappe dans la dernière
/// entrée serait le pire des comportements. Deux automatisations de même nom ne
/// peuvent pas coexister (le nom est la clé du moteur) : la SECONDE est écartée.
///
/// ⚠️ **LES CONDITIONS ET LES ACTIONS SONT À PART, RELIÉES PAR L'INDEX `auto`**
/// (voir `vers_toml`). Un index qui ne désigne aucune automatisation — parce
/// qu'elle a été écartée — est simplement IGNORÉ : c'est le sort normal d'une
/// entrée orpheline, pas une erreur.
pub fn depuis_toml(texte: &str) -> Vec<Automatisation> {
    match texte.parse::<toml::Table>() {
        Ok(table) => depuis_table(&table),
        Err(_) => Vec::new(),
    }
}

/// **LE CŒUR DE LA LECTURE, PARTAGÉ PAR LE FICHIER ET LA PAGE.**
///
/// ⛔ **CE QUI DIFFÈRE ENTRE LES DEUX FORMATS N'EST PAS LA VALIDATION, C'EST LE
/// RANGEMENT.** Le fichier met les conditions et les actions À PART et les relie
/// par l'index `auto` (voir [`vers_toml`] — `toml::to_string` ne sait pas
/// imbriquer sans rattacher les sous-tables à la mauvaise entrée) ; la page les
/// IMBRIQUE sous leur automatisation, ce qui est la forme naturelle en JSON.
/// Chaque appelant fait donc son propre rassemblement, puis vient ici : les deux
/// formats sont validés par le MÊME code, et la page n'est pas une source plus
/// fiable que le fichier qu'elle écrit.
///
/// Rend `None` si la ligne est inutilisable (nom vide, déclencheur illisible) —
/// l'appelant l'écarte, elle seule.
fn lire_ligne(ligne: &toml::Value, conditions: &[&toml::Value], actions: &[&toml::Value]) -> Option<Automatisation> {
    let nom = chaine(ligne, "nom");
    if nom.is_empty() {
        return None;
    }
    Some(Automatisation {
        nom,
        actif: booleen(ligne, "actif", true),
        declencheur: declencheur(ligne)?,
        conditions: conditions.iter().take(MAX_CONDITIONS).filter_map(|c| condition(c)).collect(),
        actions: actions.iter().take(MAX_ACTIONS).filter_map(|a| action(a)).collect(),
    })
}

/// Deux automatisations de même nom ne peuvent pas coexister — le nom est la clé
/// du moteur. La SECONDE est écartée, dans les deux formats.
fn ajouter(autos: &mut Vec<Automatisation>, auto: Option<Automatisation>) {
    let Some(auto) = auto else { return };
    if autos.iter().any(|a| a.nom == auto.nom) {
        return;
    }
    autos.push(auto);
}

/// Les lignes qui VISENT l'automatisation `i` — la forme plate du fichier.
fn visant(l: &[toml::Value], i: usize) -> Vec<&toml::Value> {
    l.iter().filter(|v| v.get("auto").and_then(|x| x.as_integer()) == Some(i as i64)).collect()
}

/// Toutes les lignes, empruntées — la forme imbriquée de la page.
fn emprunts(v: &[toml::Value]) -> Vec<&toml::Value> {
    v.iter().collect()
}

/// **LA FORME PLATE DU FICHIER** : trois tableaux reliés par l'index `auto`.
fn depuis_table(table: &toml::Table) -> Vec<Automatisation> {
    let Some(lignes) = table.get("automatisation").and_then(|v| v.as_array()) else { return Vec::new() };
    let vides: Vec<toml::Value> = Vec::new();
    let conditions = table.get("condition").and_then(|v| v.as_array()).unwrap_or(&vides);
    let actions = table.get("action").and_then(|v| v.as_array()).unwrap_or(&vides);

    let mut autos = Vec::new();
    // ⚠️ **L'INDEX EST CELUI DU FICHIER, DONC CELUI DU COMPTEUR, PAS DU RÉSULTAT** :
    // une entrée écartée laisse un trou, et les conditions qui la visaient
    // deviennent orphelines — c'est leur sort normal, pas une erreur.
    for (i, ligne) in lignes.iter().take(MAX_AUTOMATISATIONS).enumerate() {
        ajouter(&mut autos, lire_ligne(ligne, &visant(conditions, i), &visant(actions, i)));
    }
    autos
}

/// **L'ÉCRITURE — un tableau par famille, TOUS PLATS.**
///
/// ⛔ **POURQUOI RIEN N'EST IMBRIQUÉ.** `vers_toml` passe par `toml::to_string`,
/// qui range les clés d'une table dans l'ordre alphabétique et écrit
/// `[parent.enfant]` APRÈS `[[parent]]`. Une action imbriquée sous son
/// automatisation se retrouverait donc rattachée à la MAUVAISE entrée dès qu'il y
/// en a deux. Trois tableaux plats (`automatisation`, `condition`, `action`),
/// reliés par l'INDEX `auto`, n'ont pas ce défaut — et restent lisibles à la main.
pub fn vers_toml(autos: &[Automatisation]) -> String {
    use toml::Value;

    let lignes: Vec<Value> = autos
        .iter()
        .map(|a| {
            let mut t = toml::Table::new();
            t.insert("nom".into(), Value::String(a.nom.clone()));
            t.insert("actif".into(), Value::Boolean(a.actif));
            t.insert("quand".into(), Value::String(a.declencheur.sorte().into()));
            match &a.declencheur {
                Declencheur::Heure { heure, minute, jours } => {
                    t.insert("heure".into(), Value::Integer(*heure as i64));
                    t.insert("minute".into(), Value::Integer(*minute as i64));
                    t.insert("jours".into(), Value::Array(jours.noms().into_iter().map(Value::String).collect()));
                }
                Declencheur::Programme { exe, ouvre } => {
                    t.insert("exe".into(), Value::String(exe.clone()));
                    t.insert("sens".into(), Value::String(if *ouvre { "ouvre".into() } else { "ferme".into() }));
                }
                Declencheur::Demarrage => {}
            }
            Value::Table(t)
        })
        .collect();

    let mut conditions = Vec::new();
    let mut actions = Vec::new();
    for (i, auto) in autos.iter().enumerate() {
        for c in auto.conditions.iter().take(MAX_CONDITIONS) {
            let mut t = toml::Table::new();
            t.insert("auto".into(), Value::Integer(i as i64));
            match c {
                Condition::Plage { depuis, jusqua } => {
                    t.insert("sorte".into(), Value::String("plage".into()));
                    t.insert("depuis".into(), Value::String(format!("{:02}:{:02}", depuis.0, depuis.1)));
                    t.insert("jusqua".into(), Value::String(format!("{:02}:{:02}", jusqua.0, jusqua.1)));
                }
                Condition::Jours(jours) => {
                    t.insert("sorte".into(), Value::String("jours".into()));
                    t.insert("jours".into(), Value::Array(jours.noms().into_iter().map(Value::String).collect()));
                }
                Condition::ProgrammeEnCours(exe) => {
                    t.insert("sorte".into(), Value::String("programme".into()));
                    t.insert("exe".into(), Value::String(exe.clone()));
                }
            }
            conditions.push(Value::Table(t));
        }

        for a in auto.actions.iter().take(MAX_ACTIONS) {
            let mut t = toml::Table::new();
            t.insert("auto".into(), Value::Integer(i as i64));
            t.insert("sorte".into(), Value::String(a.sorte().into()));
            match a {
                Action::Lancer { chemin, arguments } => {
                    t.insert("chemin".into(), Value::String(chemin.clone()));
                    if !arguments.is_empty() {
                        t.insert("arguments".into(), Value::String(arguments.clone()));
                    }
                }
                Action::Commande { nom, mode } => {
                    t.insert("nom".into(), Value::String(nom.clone()));
                    t.insert("mode".into(), Value::String(mode.nom().into()));
                }
                Action::Theme(voulu) => {
                    let mot = match voulu {
                        ThemeVoulu::Clair => "clair",
                        ThemeVoulu::Sombre => "sombre",
                        ThemeVoulu::Basculer => "basculer",
                    };
                    t.insert("valeur".into(), Value::String(mot.into()));
                }
                Action::EclairageNocturne(voulu) => {
                    let mot = match voulu {
                        LumiereVoulue::Allumer => "allumer",
                        LumiereVoulue::Eteindre => "eteindre",
                        LumiereVoulue::Basculer => "basculer",
                    };
                    t.insert("valeur".into(), Value::String(mot.into()));
                }
                Action::Notifier { titre, message, son } => {
                    t.insert("titre".into(), Value::String(titre.clone()));
                    t.insert("message".into(), Value::String(message.clone()));
                    t.insert("son".into(), Value::Boolean(*son));
                }
                Action::Interface { masquer } => {
                    t.insert("masquer".into(), Value::Boolean(*masquer));
                }
            }
            actions.push(Value::Table(t));
        }
    }

    let mut table = toml::Table::new();
    table.insert("automatisation".into(), Value::Array(lignes));
    if !conditions.is_empty() {
        table.insert("condition".into(), Value::Array(conditions));
    }
    if !actions.is_empty() {
        table.insert("action".into(), Value::Array(actions));
    }
    toml::to_string(&table).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// L'échange avec la page
// ---------------------------------------------------------------------------

/// **UN ARBRE JSON → UN ARBRE TOML**, pour n'avoir qu'UNE lecture des deux
/// formats (voir [`depuis_table`]).
///
/// Les formes qui nous concernent — chaînes, entiers, booléens, tableaux, tables —
/// se traduisent sans perte. **Tout le reste est REFUSÉ** (`None`) : un flottant
/// là où l'on attend un entier, ou un `null`, écarte la ligne par la lecture
/// ordinaire plutôt que d'inventer une valeur.
fn arbre_toml(v: &serde_json::Value) -> Option<toml::Value> {
    use serde_json::Value as J;
    Some(match v {
        J::Bool(b) => toml::Value::Boolean(*b),
        J::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => toml::Value::Integer(i),
            (None, Some(f)) => toml::Value::Float(f),
            _ => return None,
        },
        J::String(s) => toml::Value::String(s.clone()),
        J::Array(l) => toml::Value::Array(l.iter().map(arbre_toml).collect::<Option<Vec<_>>>()?),
        J::Object(o) => {
            toml::Value::Table(o.iter().map(|(k, v)| Some((k.clone(), arbre_toml(v)?))).collect::<Option<toml::Table>>()?)
        }
        J::Null => return None,
    })
}

/// **LA LISTE RENVOYÉE PAR LA PAGE** — même forme que le fichier.
///
/// ⚠️ **LE TROP-PLEIN EST UNE ERREUR ICI, PAS UNE TRONCATURE.** `depuis_toml`
/// coupe au-delà de `MAX_AUTOMATISATIONS` (un fichier trop gros ne doit pas
/// empêcher le dock de démarrer) ; mais une page qui en envoie trop a un défaut,
/// et l'écrire en silence **effacerait** celles qui dépassent.
pub fn depuis_json(octets: &[u8]) -> Result<Vec<Automatisation>, String> {
    let valeur: serde_json::Value = serde_json::from_slice(octets).map_err(|e| format!("JSON illisible : {e}"))?;
    let lignes = valeur
        .get("automatisation")
        .and_then(|v| v.as_array())
        .ok_or("une liste `automatisation` était attendue")?;
    if lignes.len() > MAX_AUTOMATISATIONS {
        return Err(format!("trop d'automatisations ({} > {MAX_AUTOMATISATIONS})", lignes.len()));
    }

    // La page IMBRIQUE les conditions et les actions : on les traduit en arbre
    // TOML pour les confier à la lecture commune (voir `lire_ligne`).
    let mut autos = Vec::new();
    for ligne in lignes {
        let Some(row) = arbre_toml(ligne) else { continue };
        let sous = |cle: &str| -> Vec<toml::Value> {
            ligne
                .get(cle)
                .and_then(|v| v.as_array())
                .map(|l| l.iter().filter_map(arbre_toml).collect())
                .unwrap_or_default()
        };
        let (c, a) = (sous("conditions"), sous("actions"));
        ajouter(&mut autos, lire_ligne(&row, &emprunts(&c), &emprunts(&a)));
    }
    Ok(autos)
}

fn condition_json(c: &Condition) -> serde_json::Value {
    match c {
        Condition::Plage { depuis, jusqua } => json!({
            "sorte": "plage",
            "depuis": format!("{:02}:{:02}", depuis.0, depuis.1),
            "jusqua": format!("{:02}:{:02}", jusqua.0, jusqua.1),
        }),
        Condition::Jours(jours) => json!({ "sorte": "jours", "jours": jours.noms() }),
        Condition::ProgrammeEnCours(exe) => json!({ "sorte": "programme", "exe": exe }),
    }
}

fn action_json(a: &Action) -> serde_json::Value {
    match a {
        Action::Lancer { chemin, arguments } => json!({ "sorte": "lancer", "chemin": chemin, "arguments": arguments }),
        Action::Commande { nom, mode } => json!({ "sorte": "commande", "nom": nom, "mode": mode.nom() }),
        Action::Theme(voulu) => json!({
            "sorte": "theme",
            "valeur": match voulu {
                ThemeVoulu::Clair => "clair",
                ThemeVoulu::Sombre => "sombre",
                ThemeVoulu::Basculer => "basculer",
            },
        }),
        Action::EclairageNocturne(voulu) => json!({
            "sorte": "eclairage",
            "valeur": match voulu {
                LumiereVoulue::Allumer => "allumer",
                LumiereVoulue::Eteindre => "eteindre",
                LumiereVoulue::Basculer => "basculer",
            },
        }),
        Action::Notifier { titre, message, son } => {
            json!({ "sorte": "notifier", "titre": titre, "message": message, "son": son })
        }
        Action::Interface { masquer } => json!({ "sorte": "interface", "masquer": masquer }),
    }
}

/// **L'ÉTAT ENVOYÉ À LA PAGE** — la MÊME forme que le fichier, clé pour clé,
/// pour que la page n'ait qu'une seule représentation à connaître.
pub fn vers_json(autos: &[Automatisation]) -> serde_json::Value {
    let lignes: Vec<serde_json::Value> = autos
        .iter()
        .map(|a| {
            let mut ligne = json!({
                "nom": a.nom,
                "actif": a.actif,
                "quand": a.declencheur.sorte(),
                "conditions": a.conditions.iter().map(condition_json).collect::<Vec<_>>(),
                "actions": a.actions.iter().map(action_json).collect::<Vec<_>>(),
            });
            match &a.declencheur {
                Declencheur::Heure { heure, minute, jours } => {
                    ligne["heure"] = json!(heure);
                    ligne["minute"] = json!(minute);
                    ligne["jours"] = json!(jours.noms());
                }
                Declencheur::Programme { exe, ouvre } => {
                    ligne["exe"] = json!(exe);
                    ligne["sens"] = json!(if *ouvre { "ouvre" } else { "ferme" });
                }
                Declencheur::Demarrage => {}
            }
            ligne
        })
        .collect();
    json!({ "automatisation": lignes })
}

pub fn journal_json() -> serde_json::Value {
    MOTEUR.with(|m| {
        json!(m
            .borrow()
            .journal()
            .iter()
            .map(|e| json!({
                "quand": e.quand,
                "automatisation": e.automatisation,
                "resume": e.resume,
                "reussi": e.reussi,
            }))
            .collect::<Vec<_>>())
    })
}

/// **L'ÉTAT COMPLET POUR LA PAGE** — la liste ET le journal des exécutions.
pub fn json() -> serde_json::Value {
    ETAT.with(|e| {
        let mut valeur = vers_json(&e.borrow());
        valeur["journal"] = journal_json();
        valeur
    })
}

/// **LA PAGE A RENVOYÉ SA LISTE** — le corps est arrivé par `cef_data` (voir
/// `win32::cef_data`), sur `route`.
///
/// ⚠️ **LA LISTE EST REMPLACÉE ENTIÈREMENT, JAMAIS MUTÉE EN PLACE** : le moteur
/// évalue `ETAT` à chaque frame, sur le fil de rendu ; une liste à moitié écrite
/// lui ferait voir un état qui n'existe pas.
pub fn remplacer_depuis(route: &str) {
    let Some(octets) = crate::win32::cef_data::prendre(route) else {
        crate::diag!("[automatisations] aucune liste reçue sur {route}");
        return;
    };
    match depuis_json(&octets) {
        Ok(autos) => {
            if !crate::config::write_config_atomique(FICHIER, vers_toml(&autos)) {
                crate::diag!("[automatisations] {FICHIER} n'a pas pu être écrit");
            }
            crate::diag!("[automatisations] liste remplacée : {} entrée(s)", autos.len());
            ETAT.with(|e| *e.borrow_mut() = autos);
            // La page est repoussée par le `refresh_if_visible` de la frame
            // suivante : la charge utile ayant changé, elle repart.
        }
        Err(message) => crate::diag!("[automatisations] liste refusée : {message}"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠️ **LA DATE SUIT LE JOUR DE LA SEMAINE**, sans quoi deux « jours »
    /// différents porteraient la même clé de minute — et le test « le lendemain,
    /// même heure » passerait pour la mauvaise raison (ou échouerait).
    fn instantane(heure: u8, minute: u8, jour_semaine: u8, exes: &[&str]) -> Instantane {
        Instantane {
            annee: 2026,
            mois: 10,
            jour: 5 + jour_semaine as u32,
            jour_semaine,
            heure,
            minute,
            exes: exes.iter().map(|e| e.to_string()).collect(),
        }
    }

    /// Un moteur déjà amorcé (l'instant précédent est posé), pour éprouver les
    /// déclencheurs de programme sans buter sur la base.
    fn moteur_amorce(exes: &[&str]) -> Moteur {
        let mut moteur = Moteur::default();
        moteur.poser_instantane(instantane(9, 0, 0, exes));
        moteur.evaluer(&[]);
        moteur
    }

    fn auto_heure(nom: &str, heure: u8, minute: u8) -> Automatisation {
        Automatisation {
            nom: nom.into(),
            actif: true,
            declencheur: Declencheur::Heure { heure, minute, jours: Jours::TOUS },
            conditions: Vec::new(),
            actions: vec![Action::Notifier { titre: "t".into(), message: "m".into(), son: false }],
        }
    }

    fn auto_programme(nom: &str, exe: &str, ouvre: bool) -> Automatisation {
        Automatisation {
            nom: nom.into(),
            actif: true,
            declencheur: Declencheur::Programme { exe: exe.into(), ouvre },
            conditions: Vec::new(),
            actions: vec![Action::Theme(ThemeVoulu::Basculer)],
        }
    }

    /// **UNE FOIS PAR MINUTE, PAS SOIXANTE.** La boucle tourne à 60 Hz : sans la
    /// minute retenue, l'automatisation de 18 h partirait soixante fois.
    #[test]
    fn l_heure_sonne_une_fois_par_minute() {
        let mut moteur = Moteur::default();
        let autos = [auto_heure("soir", 18, 0)];

        moteur.poser_instantane(instantane(18, 0, 0, &[]));
        assert_eq!(moteur.evaluer(&autos).len(), 1, "premier passage");
        for _ in 0..59 {
            assert!(moteur.evaluer(&autos).is_empty(), "la même minute ne sonne qu'une fois");
        }

        moteur.poser_instantane(instantane(18, 1, 0, &[]));
        assert!(moteur.evaluer(&autos).is_empty(), "l'heure est passée");

        // Le LENDEMAIN, à la même heure, elle repart.
        moteur.poser_instantane(instantane(18, 0, 1, &[]));
        assert_eq!(moteur.evaluer(&autos).len(), 1, "un autre jour, même heure");
    }

    #[test]
    fn une_automatisation_inactive_ne_part_pas() {
        let mut moteur = Moteur::default();
        let mut auto = auto_heure("soir", 18, 0);
        auto.actif = false;
        moteur.poser_instantane(instantane(18, 0, 0, &[]));
        assert!(moteur.evaluer(&[auto]).is_empty());
    }

    /// Une automatisation sans action ne fait rien — et ne consomme pas sa minute
    /// pour autant : elle est simplement ignorée.
    #[test]
    fn une_automatisation_sans_action_ne_part_pas() {
        let mut moteur = Moteur::default();
        let mut auto = auto_heure("vide", 18, 0);
        auto.actions.clear();
        moteur.poser_instantane(instantane(18, 0, 0, &[]));
        assert!(moteur.evaluer(&[auto]).is_empty());
    }

    #[test]
    fn le_jour_choisi_est_respecte() {
        let mut moteur = Moteur::default();
        let mut auto = auto_heure("matin", 8, 0);
        auto.declencheur = Declencheur::Heure { heure: 8, minute: 0, jours: Jours::depuis_jour(0) };

        moteur.poser_instantane(instantane(8, 0, 2, &[])); // mercredi
        assert!(moteur.evaluer(&[auto.clone()]).is_empty(), "mercredi : rien");
        moteur.poser_instantane(instantane(8, 0, 0, &[])); // lundi
        assert_eq!(moteur.evaluer(&[auto]).len(), 1, "lundi : oui");
    }

    /// ⛔ **LE PIÈGE DU DÉMARRAGE** : NeuraDesk se lance avec Discord déjà ouvert.
    /// Sans la base, « quand Discord s'ouvre » partirait à chaque démarrage.
    #[test]
    fn le_premier_instantane_n_est_qu_une_base() {
        let mut moteur = Moteur::default();
        let auto = auto_programme("discord", "discord", true);

        moteur.poser_instantane(instantane(9, 0, 0, &["discord", "explorer"]));
        assert!(moteur.evaluer(&[auto.clone()]).is_empty(), "déjà ouvert au démarrage");

        // Il se ferme, puis se réouvre : là, oui.
        moteur.poser_instantane(instantane(9, 1, 0, &["explorer"]));
        assert!(moteur.evaluer(&[auto.clone()]).is_empty(), "la fermeture ne compte pas");
        moteur.poser_instantane(instantane(9, 2, 0, &["explorer", "discord"]));
        assert_eq!(moteur.evaluer(&[auto]).len(), 1, "il vient de s'ouvrir");
    }

    #[test]
    fn un_programme_qui_se_ferme_declenche_la_fermeture() {
        let mut moteur = moteur_amorce(&["obs64"]);
        let auto = auto_programme("fin obs", "obs64", false);
        assert!(moteur.evaluer(&[auto.clone()]).is_empty(), "il tourne encore");
        moteur.poser_instantane(instantane(9, 1, 0, &[]));
        assert_eq!(moteur.evaluer(&[auto]).len(), 1);
    }

    /// Le nom est comparé sous forme NORMALISÉE : le fichier peut écrire un
    /// chemin complet, la liste des processus ne porte que le nom.
    #[test]
    fn le_nom_du_programme_est_normalise() {
        let mut moteur = moteur_amorce(&[]);
        let auto = auto_programme("obs", r"C:\Program Files\obs-studio\bin\obs64.EXE", true);
        moteur.poser_instantane(instantane(9, 1, 0, &["obs64"]));
        assert_eq!(moteur.evaluer(&[auto]).len(), 1);
    }

    #[test]
    fn le_demarrage_ne_sonne_qu_une_fois() {
        let mut moteur = Moteur::default();
        let auto = Automatisation {
            nom: "accueil".into(),
            actif: true,
            declencheur: Declencheur::Demarrage,
            conditions: Vec::new(),
            actions: vec![Action::Notifier { titre: "t".into(), message: "m".into(), son: false }],
        };
        moteur.poser_instantane(instantane(9, 0, 0, &[]));
        assert_eq!(moteur.evaluer(&[auto.clone()]).len(), 1);
        for _ in 0..10 {
            assert!(moteur.evaluer(&[auto.clone()]).is_empty());
        }
    }

    #[test]
    fn une_condition_fausse_bloque_le_declenchement() {
        let mut moteur = Moteur::default();
        let mut auto = auto_heure("soir", 18, 0);
        auto.conditions = vec![Condition::ProgrammeEnCours("obs64".into())];

        moteur.poser_instantane(instantane(18, 0, 0, &["explorer"]));
        assert!(moteur.evaluer(&[auto.clone()]).is_empty(), "OBS ne tourne pas");
        // Mais dans la MÊME minute, la condition devient vraie : elle part.
        moteur.poser_instantane(instantane(18, 0, 0, &["explorer", "obs64"]));
        assert_eq!(moteur.evaluer(&[auto]).len(), 1, "la condition devient vraie dans la minute");
    }

    /// ⛔ **LA PLAGE QUI FRANCHIT MINUIT** — 22:00 → 06:00 est le cas courant d'un
    /// « la nuit », et une comparaison naïve le rendrait toujours FAUX.
    #[test]
    fn une_plage_qui_franchit_minuit_tient_les_deux_bouts() {
        let nuit = Condition::Plage { depuis: (22, 0), jusqua: (6, 0) };
        for (heure, attendu) in [(22, true), (23, true), (0, true), (3, true), (6, true), (7, false), (12, false), (21, false)] {
            assert_eq!(nuit.tenue(&instantane(heure, 0, 0, &[])), attendu, "{heure} h");
        }
        let journee = Condition::Plage { depuis: (8, 0), jusqua: (18, 0) };
        for (heure, attendu) in [(8, true), (12, true), (18, true), (19, false), (7, false)] {
            assert_eq!(journee.tenue(&instantane(heure, 0, 0, &[])), attendu, "{heure} h");
        }
    }

    #[test]
    fn les_jours_font_l_aller_retour_par_leurs_noms() {
        let jours = Jours::depuis_noms(&["lundi".into(), "vendredi".into(), "dimanche".into()]);
        assert!(jours.contient(0) && jours.contient(4) && jours.contient(6));
        assert!(!jours.contient(1) && !jours.contient(5));
        assert_eq!(jours.noms(), vec!["lundi", "vendredi", "dimanche"]);

        // Une faute de frappe est ÉCARTÉE, pas fatale.
        assert_eq!(Jours::depuis_noms(&["mardi".into(), "mardii".into()]).noms(), vec!["mardi"]);
        assert!(Jours::depuis_noms(&[]).est_vide());

        // La casse et les espaces n'ont pas d'importance.
        assert_eq!(Jours::depuis_noms(&[" LUNDI ".into()]), Jours::depuis_jour(0));
    }

    #[test]
    fn une_heure_illisible_ecarte_cette_automatisation_seulement() {
        let texte = r#"
            [[automatisation]]
            nom = "cassée"
            quand = "heure"
            heure = 99

            [[automatisation]]
            nom = "bonne"
            quand = "heure"
            heure = 7
            minute = 30

            [[action]]
            auto = 1
            sorte = "theme"
            valeur = "sombre"
        "#;
        let autos = depuis_toml(texte);
        assert_eq!(autos.len(), 1, "la mauvaise est écartée, l'autre survit");
        assert_eq!(autos[0].nom, "bonne");
        assert_eq!(autos[0].actions, vec![Action::Theme(ThemeVoulu::Sombre)]);
    }

    /// Une entrée orpheline (index `auto` qui ne désigne rien) est IGNORÉE, sans
    /// emporter le reste : c'est ce qui arrive dès qu'une automatisation est
    /// écartée par `depuis_toml` alors que ses actions, elles, restent lisibles.
    #[test]
    fn une_entree_orpheline_est_ignoree() {
        let texte = r#"
            [[automatisation]]
            nom = "seule"
            quand = "demarrage"

            [[condition]]
            auto = 7
            sorte = "jours"
            jours = ["lundi"]

            [[action]]
            auto = 0
            sorte = "notifier"
            message = "bonjour"
        "#;
        let autos = depuis_toml(texte);
        assert_eq!(autos.len(), 1);
        assert!(autos[0].conditions.is_empty(), "l'orpheline ne s'est pas accrochée");
        assert_eq!(autos[0].actions.len(), 1);
    }

    #[test]
    fn deux_automatisations_du_meme_nom_ne_coexistent_pas() {
        let texte = r#"
            [[automatisation]]
            nom = "doublon"
            quand = "demarrage"

            [[automatisation]]
            nom = "doublon"
            quand = "demarrage"
        "#;
        assert_eq!(depuis_toml(texte).len(), 1, "le nom est la clé du moteur");
    }

    #[test]
    fn une_liste_de_jours_vide_vaut_tous_les_jours() {
        let autos = depuis_toml("[[automatisation]]\nnom = \"a\"\nquand = \"heure\"\nheure = 6\n");
        match &autos[0].declencheur {
            Declencheur::Heure { jours, .. } => assert_eq!(*jours, Jours::TOUS),
            autre => panic!("déclencheur inattendu : {autre:?}"),
        }
    }

    /// **LE MÊME JEU POUR LES DEUX FORMATS** — c'est ce qui permet de dire que le
    /// fichier et la page portent la même information, et pas seulement que chacun
    /// se relit lui-même.
    fn jeu_d_essai() -> Vec<Automatisation> {
        vec![
            Automatisation {
                nom: "Sauvegarde du soir".into(),
                actif: true,
                declencheur: Declencheur::Heure {
                    heure: 18,
                    minute: 30,
                    jours: Jours::depuis_noms(&["lundi".into(), "vendredi".into()]),
                },
                conditions: vec![
                    Condition::Plage { depuis: (8, 0), jusqua: (18, 30) },
                    Condition::Jours(Jours::depuis_jour(0)),
                    Condition::ProgrammeEnCours("obs64".into()),
                ],
                actions: vec![
                    Action::Lancer { chemin: r"C:\Outils\sauve.bat".into(), arguments: "--tout".into() },
                    Action::Commande { nom: "sauvegarde".into(), mode: ModeCommande::ArrierePlan },
                    Action::Theme(ThemeVoulu::Sombre),
                    Action::EclairageNocturne(LumiereVoulue::Allumer),
                    Action::Notifier { titre: "Sauvegarde".into(), message: "terminée".into(), son: true },
                    Action::Interface { masquer: true },
                ],
            },
            Automatisation {
                nom: "OBS fermé".into(),
                actif: false,
                declencheur: Declencheur::Programme { exe: "obs64".into(), ouvre: false },
                conditions: Vec::new(),
                actions: vec![Action::Lancer { chemin: "https://exemple.test".into(), arguments: String::new() }],
            },
            Automatisation {
                nom: "Démarrage".into(),
                actif: true,
                declencheur: Declencheur::Demarrage,
                conditions: Vec::new(),
                actions: vec![Action::Theme(ThemeVoulu::Basculer)],
            },
        ]
    }

    /// Le fichier fait un aller-retour COMPLET : chaque déclencheur, chaque
    /// condition et chaque action doivent revenir à l'identique — c'est ce qui
    /// prouve que l'index `auto` relie bien les trois tableaux.
    #[test]
    fn le_fichier_fait_l_aller_retour() {
        let autos = jeu_d_essai();
        let ecrit = vers_toml(&autos);
        assert_eq!(depuis_toml(&ecrit), autos, "fichier écrit :\n{ecrit}");
    }

    /// Le chemin de la PAGE fait le même aller-retour que le fichier.
    #[test]
    fn le_json_fait_l_aller_retour() {
        let autos = jeu_d_essai();
        let octets = serde_json::to_vec(&vers_json(&autos)).unwrap();
        assert_eq!(depuis_json(&octets).unwrap(), autos);
    }

    /// ⛔ **LES DEUX FORMATS DOIVENT DIRE LA MÊME CHOSE.** Ce test est le seul qui
    /// relie les deux écritures : sans lui, une clé renommée d'un côté seulement
    /// passerait inaperçue, chaque format se relisant très bien tout seul.
    #[test]
    fn le_json_et_le_toml_disent_la_meme_chose() {
        let autos = jeu_d_essai();
        let par_le_fichier = depuis_toml(&vers_toml(&autos));
        let par_la_page = depuis_json(&serde_json::to_vec(&vers_json(&autos)).unwrap()).unwrap();
        assert_eq!(par_le_fichier, par_la_page);
    }

    #[test]
    fn un_json_illisible_est_refuse() {
        assert!(depuis_json(b"pas du json").is_err());
        assert!(depuis_json(b"[1, 2, 3]").is_err(), "une liste n'est pas une table");
    }

    /// ⚠️ **LE TROP-PLEIN EST UNE ERREUR ICI, PAS UNE TRONCATURE** : une page qui
    /// en envoie trop a un défaut, et écrire en silence effacerait les dernières.
    #[test]
    fn un_json_trop_long_est_refuse_sans_tronquer() {
        let lignes: Vec<serde_json::Value> = (0..(MAX_AUTOMATISATIONS + 1))
            .map(|i| json!({ "nom": format!("a{i}"), "quand": "demarrage" }))
            .collect();
        let octets = serde_json::to_vec(&json!({ "automatisation": lignes })).unwrap();
        assert!(depuis_json(&octets).is_err());
    }

    /// Le bouton « Tester » met les actions DANS LA MÊME FILE que le moteur — et
    /// il ne consulte pas l'activation : on essaie une automatisation avant de
    /// l'allumer.
    #[test]
    fn tester_met_les_actions_en_file_sans_regarder_l_activation() {
        let mut auto = auto_heure("essai", 3, 0);
        auto.actif = false;
        ETAT.with(|e| *e.borrow_mut() = vec![auto]);
        let _ = prendre_a_executer();

        assert!(tester("essai").is_ok());
        let ordres = prendre_a_executer();
        assert_eq!(ordres.len(), 1);
        assert_eq!(ordres[0].automatisation, "essai");
        assert!(prendre_a_executer().is_empty(), "la file est vidée en la lisant");

        assert!(tester("inconnue").is_err(), "un nom inconnu doit le DIRE");
    }

    #[test]
    fn une_automatisation_sans_action_ne_se_test_pas() {
        let mut auto = auto_heure("vide", 3, 0);
        auto.actions.clear();
        ETAT.with(|e| *e.borrow_mut() = vec![auto]);
        assert!(tester("vide").is_err());
    }

    /// Le journal garde les DERNIÈRES, et rien de plus.
    #[test]
    fn le_journal_est_borne() {
        let mut moteur = Moteur::default();
        moteur.poser_instantane(instantane(9, 0, 0, &[]));
        for i in 0..(MAX_JOURNAL + 10) {
            moteur.noter("a", Ok(format!("ligne {i}")));
        }
        let journal = moteur.journal();
        assert_eq!(journal.len(), MAX_JOURNAL);
        assert_eq!(journal.last().unwrap().resume, format!("ligne {}", MAX_JOURNAL + 9));
        assert!(journal[0].reussi);
        assert_eq!(journal[0].quand, "09:00");
    }

    #[test]
    fn un_echec_est_journalise_comme_tel() {
        let mut moteur = Moteur::default();
        moteur.poser_instantane(instantane(9, 0, 0, &[]));
        moteur.noter("a", Err("rien n'a pu être fait".into()));
        assert!(!moteur.journal()[0].reussi);
    }

    /// Le moteur ne retient la minute que des automatisations qui EXISTENT encore :
    /// un renommage ne doit pas laisser une clé orpheline pour toujours.
    #[test]
    fn les_minutes_retenues_sont_nettoyees() {
        let mut moteur = Moteur::default();
        moteur.poser_instantane(instantane(18, 0, 0, &[]));
        assert_eq!(moteur.evaluer(&[auto_heure("soir", 18, 0)]).len(), 1);
        moteur.evaluer(&[auto_heure("matin", 7, 0)]);
        assert!(moteur.dernieres.is_empty(), "la minute de « soir » a été oubliée");
    }

    /// ⛔ **L'APARTÉ DE L'ÉCRITURE EST DANS LE MÊME ORDRE QUE LA LECTURE.** Ce test
    /// garde la forme du fichier : trois tableaux plats, reliés par `auto`.
    #[test]
    fn l_ecriture_ne_met_rien_en_imbrique() {
        let autos = vec![auto_heure("a", 1, 0), auto_heure("b", 2, 0)];
        let texte = vers_toml(&autos);
        assert!(!texte.contains("[automatisation."), "aucune sous-table :\n{texte}");
        assert_eq!(texte.matches("[[automatisation]]").count(), 2);
        assert_eq!(texte.matches("[[action]]").count(), 2);
    }
}
