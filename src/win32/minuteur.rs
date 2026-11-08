//! **LE MINUTEUR — comptes à rebours et alarmes.** DEMANDE EXPLICITE
//! (2026-09-25) : « une applet qui permette de mettre des comptes à rebours et
//! des alarmes », avec répétition, sonnerie en boucle jusqu'à l'arrêt (ou
//! « Répéter dans 5 min »), et des comptes qui survivent à un redémarrage.
//!
//! Ce module porte le MODÈLE et sa persistance (`%APPDATA%\NeuraDesk\alarmes.toml`) ;
//! le panneau est `win32::cef_minuteur`, et la sonnerie vit dans le bloc de
//! sonnerie de `app::run`, avec celle des rappels.
//!
//! ⚠️ **UN COMPTE EN COURS EST UN INSTANT DE FIN, PAS UN RESTE** : c'est ce qui le
//! fait survivre à un redémarrage — l'instant Unix reste vrai pendant que
//! l'application est fermée, un reste décompté frame par frame, non.
//!
//! ⚠️ **TOUT CE QUI DÉCIDE EST PUR** (`Minuteur::avancer`, `depuis_toml`,
//! `vers_toml`) : on lui passe l'instant, le jour et l'heure. Aucun test ne touche
//! le vrai fichier de l'utilisateur.

use std::cell::RefCell;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::config;
use crate::win32::notes::{jour_de_semaine, JOURS_SEMAINE};

const FICHIER: &str = "alarmes.toml";

/// Une alarme qui n'a pas pu sonner à son heure (application fermée) sonne
/// encore si on la rattrape dans ce délai — au-delà, un réveil de 7 h qui
/// sonnerait à 23 h ne voudrait plus rien dire.
const RATTRAPAGE_MINUTES: u32 = 30;

/// « Répéter dans 5 min ».
pub const REPORT_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compte {
    pub id: u64,
    pub nom: String,
    pub duree_ms: i64,
    /// Ce qu'il reste quand le compte est ARRÊTÉ (en pause ou pas encore lancé).
    pub reste_ms: i64,
    /// L'instant Unix (ms) de la fin quand il TOURNE — voir la doc de module.
    pub fin_ms: Option<i64>,
    pub sonne: bool,
    /// Un compte né d'un « Répéter dans 5 min » : il disparaît quand on l'arrête.
    pub ephemere: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alarme {
    pub id: u64,
    pub nom: String,
    pub heure: (u32, u32),
    /// Même masque que `notes::AlarmeNote::jours` : `0` = une fois.
    pub jours: u8,
    pub active: bool,
    /// Le NOM d'un fichier de `son::dossier()` — jamais un chemin, pour qu'aucune
    /// valeur venue de la page ne sorte de ce dossier (voir `chemin_du_son`).
    pub son: Option<String>,
    /// Le dernier jour où elle a sonné (ou a été armée après son heure) : elle
    /// ne sonne qu'une fois par jour.
    pub derniere: Option<(i32, u32, u32)>,
    pub sonne: bool,
}

/// ⭐ **LE POMODORO** — DEMANDE EXPLICITE (2026-10-24) : « un Pomodoro relié
/// au Minuteur ». Des séances de travail séparées de pauses, une longue pause
/// toutes les `SEANCES_AVANT_LONGUE` séances.
///
/// ⚠️ **UNE PHASE QUI FINIT SONNE COMME UN COMPTE** (même sonnerie, même
/// bandeau) ; « Arrêter » l'acquitte et **ENCHAÎNE** la phase suivante, lancée à
/// la frame d'après par `avancer` (voir `enchainer`) — `arreter` n'a pas
/// l'instant, et l'ajouter changerait tous ses appels. « Répéter dans 5 min »
/// PROLONGE la phase en cours au lieu de créer un compte à part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PhasePomodoro {
    #[default]
    Travail,
    Pause,
    LonguePause,
}

impl PhasePomodoro {
    pub fn cle(self) -> &'static str {
        match self {
            PhasePomodoro::Travail => "travail",
            PhasePomodoro::Pause => "pause",
            PhasePomodoro::LonguePause => "longue",
        }
    }
    pub fn nom(self) -> &'static str {
        match self {
            PhasePomodoro::Travail => "Pomodoro — travail",
            PhasePomodoro::Pause => "Pomodoro — pause",
            PhasePomodoro::LonguePause => "Pomodoro — longue pause",
        }
    }
    pub fn depuis_cle(cle: &str) -> Option<Self> {
        [PhasePomodoro::Travail, PhasePomodoro::Pause, PhasePomodoro::LonguePause].into_iter().find(|p| p.cle() == cle)
    }
}

/// Une longue pause après ce nombre de séances de travail.
pub const SEANCES_AVANT_LONGUE: u32 = 4;
/// Les durées réglables, en minutes, et leurs bornes.
pub const POMODORO_MIN_MINUTES: u32 = 1;
pub const POMODORO_MAX_MINUTES: u32 = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pomodoro {
    pub phase: PhasePomodoro,
    /// Séances de travail TERMINÉES depuis la dernière remise à zéro.
    pub seances: u32,
    pub reste_ms: i64,
    pub fin_ms: Option<i64>,
    pub sonne: bool,
    /// La phase suivante attend d'être lancée par `avancer` (voir la doc du type).
    pub enchainer: bool,
    pub travail_min: u32,
    pub pause_min: u32,
    pub longue_min: u32,
}

impl Default for Pomodoro {
    fn default() -> Self {
        let mut p = Pomodoro {
            phase: PhasePomodoro::Travail,
            seances: 0,
            reste_ms: 0,
            fin_ms: None,
            sonne: false,
            enchainer: false,
            travail_min: 25,
            pause_min: 5,
            longue_min: 15,
        };
        p.reste_ms = p.duree_ms(PhasePomodoro::Travail);
        p
    }
}

impl Pomodoro {
    pub fn duree_ms(&self, phase: PhasePomodoro) -> i64 {
        let minutes = match phase {
            PhasePomodoro::Travail => self.travail_min,
            PhasePomodoro::Pause => self.pause_min,
            PhasePomodoro::LonguePause => self.longue_min,
        };
        i64::from(minutes) * 60_000
    }

    /// Passe à la phase suivante, ARRÊTÉE, pleine durée.
    fn suivante(&mut self) {
        self.phase = match self.phase {
            PhasePomodoro::Travail => {
                self.seances += 1;
                if self.seances % SEANCES_AVANT_LONGUE == 0 { PhasePomodoro::LonguePause } else { PhasePomodoro::Pause }
            }
            _ => PhasePomodoro::Travail,
        };
        self.fin_ms = None;
        self.sonne = false;
        self.reste_ms = self.duree_ms(self.phase);
    }

    pub fn demarrer(&mut self, maintenant_ms: i64) {
        if self.fin_ms.is_some() {
            return;
        }
        if self.reste_ms <= 0 {
            self.reste_ms = self.duree_ms(self.phase);
        }
        self.sonne = false;
        self.enchainer = false;
        self.fin_ms = Some(maintenant_ms + self.reste_ms);
    }

    pub fn pause(&mut self, maintenant_ms: i64) {
        if let Some(fin) = self.fin_ms.take() {
            self.reste_ms = (fin - maintenant_ms).max(0);
        }
        self.enchainer = false;
    }

    /// Saute la phase en cours et LANCE la suivante.
    pub fn passer(&mut self, maintenant_ms: i64) {
        self.suivante();
        self.demarrer(maintenant_ms);
    }

    /// Retour au début : première séance de travail, arrêtée, compteur à zéro.
    pub fn reinitialiser(&mut self) {
        self.phase = PhasePomodoro::Travail;
        self.seances = 0;
        self.fin_ms = None;
        self.sonne = false;
        self.enchainer = false;
        self.reste_ms = self.duree_ms(PhasePomodoro::Travail);
    }

    /// Change une durée ; une phase ARRÊTÉE à pleine durée prend la nouvelle.
    pub fn regler(&mut self, phase: PhasePomodoro, minutes: u32) {
        let minutes = minutes.clamp(POMODORO_MIN_MINUTES, POMODORO_MAX_MINUTES);
        let pleine = self.fin_ms.is_none() && self.reste_ms == self.duree_ms(self.phase);
        match phase {
            PhasePomodoro::Travail => self.travail_min = minutes,
            PhasePomodoro::Pause => self.pause_min = minutes,
            PhasePomodoro::LonguePause => self.longue_min = minutes,
        }
        if pleine {
            self.reste_ms = self.duree_ms(self.phase);
        }
    }
}

/// ⭐ **LE CHRONOMÈTRE** — DEMANDE EXPLICITE (2026-11-06) : « un chronomètre au
/// minuteur ». Il COMPTE VERS LE HAUT (contrairement aux comptes à rebours) ; il
/// ne sonne jamais. Comme un compte en cours, l'état qui TOURNE est un instant
/// Unix (`marche`), pas un écoulé décompté frame par frame : c'est ce qui le fait
/// survivre à un redémarrage sans dériver pendant que l'application est fermée.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chronometre {
    /// Quand il TOURNE : l'instant Unix (ms) tel que `écoulé = maintenant - marche`
    /// (l'origine recule déjà de l'écoulé accumulé). `None` = arrêté.
    pub marche: Option<i64>,
    /// L'écoulé FIGÉ quand il est arrêté.
    pub accumule_ms: i64,
    /// Les tours : l'écoulé au moment de chaque « Tour ».
    pub tours: Vec<i64>,
}

impl Chronometre {
    /// Le nombre de tours qu'on garde — un garde-fou, pas une limite attendue.
    const MAX_TOURS: usize = 1000;

    pub fn ecoule(&self, maintenant_ms: i64) -> i64 {
        self.marche.map_or(self.accumule_ms, |m| (maintenant_ms - m).max(0))
    }

    pub fn en_marche(&self) -> bool {
        self.marche.is_some()
    }

    pub fn demarrer(&mut self, maintenant_ms: i64) {
        if self.marche.is_none() {
            // L'origine recule de l'écoulé déjà accumulé : la reprise continue.
            self.marche = Some(maintenant_ms - self.accumule_ms);
        }
    }

    pub fn pause(&mut self, maintenant_ms: i64) {
        if let Some(m) = self.marche.take() {
            self.accumule_ms = (maintenant_ms - m).max(0);
        }
    }

    /// Enregistre un tour — seulement s'il tourne (un tour à l'arrêt n'a pas de
    /// sens), et jamais au-delà du garde-fou.
    pub fn tour(&mut self, maintenant_ms: i64) {
        if self.marche.is_some() && self.tours.len() < Self::MAX_TOURS {
            self.tours.push(self.ecoule(maintenant_ms));
        }
    }

    pub fn reinitialiser(&mut self) {
        self.marche = None;
        self.accumule_ms = 0;
        self.tours.clear();
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Minuteur {
    pub comptes: Vec<Compte>,
    pub alarmes: Vec<Alarme>,
    /// Le son des comptes à rebours (nom de fichier, voir `Alarme::son`).
    pub son_comptes: Option<String>,
    pub pomodoro: Pomodoro,
    pub chrono: Chronometre,
}

fn minutes(heure: (u32, u32)) -> u32 {
    heure.0 * 60 + heure.1
}

impl Minuteur {
    fn prochain_id(&self) -> u64 {
        self.comptes.iter().map(|c| c.id).chain(self.alarmes.iter().map(|a| a.id)).max().unwrap_or(0) + 1
    }

    /// **LE PAS DE TEMPS** — fait sonner ce qui doit sonner. Rend `true` si
    /// quelque chose a changé (donc à enregistrer et à repousser à la page).
    pub fn avancer(&mut self, maintenant_ms: i64, jour: (i32, u32, u32), heure: (u32, u32)) -> bool {
        let mut change = false;
        let p = &mut self.pomodoro;
        if p.enchainer {
            p.demarrer(maintenant_ms);
            change = true;
        }
        if p.fin_ms.is_some_and(|fin| fin <= maintenant_ms) {
            p.fin_ms = None;
            p.reste_ms = 0;
            p.sonne = true;
            change = true;
        }
        for c in &mut self.comptes {
            if c.fin_ms.is_some_and(|fin| fin <= maintenant_ms) {
                c.fin_ms = None;
                c.reste_ms = 0;
                c.sonne = true;
                change = true;
            }
        }
        for a in &mut self.alarmes {
            if !a.active || a.sonne || a.derniere == Some(jour) {
                continue;
            }
            let retard = minutes(heure).checked_sub(minutes(a.heure));
            let jour_choisi = a.jours == 0 || a.jours & (1 << jour_de_semaine(jour)) != 0;
            if jour_choisi && retard.is_some_and(|r| r < RATTRAPAGE_MINUTES) {
                a.sonne = true;
                a.derniere = Some(jour);
                // Une alarme « une fois » se range après avoir sonné, comme un
                // réveil de téléphone.
                if a.jours == 0 {
                    a.active = false;
                }
                change = true;
            }
        }
        change
    }

    pub fn sonne(&self) -> bool {
        self.comptes.iter().any(|c| c.sonne) || self.alarmes.iter().any(|a| a.sonne) || self.pomodoro.sonne
    }

    /// Le son à jouer : celui de la première alarme qui sonne, sinon celui des
    /// comptes. `None` = le premier du dossier (voir `app::run`).
    pub fn son_a_jouer(&self) -> Option<String> {
        self.alarmes
            .iter()
            .find(|a| a.sonne)
            .and_then(|a| a.son.clone())
            .or_else(|| self.son_comptes.clone())
    }

    /// Les noms de ce qui sonne, pour le bandeau du panneau.
    pub fn noms_qui_sonnent(&self) -> Vec<String> {
        let comptes = self.comptes.iter().filter(|c| c.sonne).map(|c| nom_ou(&c.nom, "Compte à rebours"));
        let alarmes = self.alarmes.iter().filter(|a| a.sonne).map(|a| nom_ou(&a.nom, "Alarme"));
        let pomodoro = self.pomodoro.sonne.then(|| self.pomodoro.phase.nom().to_string());
        pomodoro.into_iter().chain(comptes).chain(alarmes).collect()
    }

    /// **« ARRÊTER »** — tout ce qui sonne se tait. Un compte revient à sa durée
    /// (prêt à relancer), un compte éphémère disparaît.
    pub fn arreter(&mut self) {
        self.comptes.retain(|c| !(c.sonne && c.ephemere));
        for c in &mut self.comptes {
            if c.sonne {
                c.sonne = false;
                c.reste_ms = c.duree_ms;
            }
        }
        for a in &mut self.alarmes {
            a.sonne = false;
        }
        if self.pomodoro.sonne {
            self.pomodoro.suivante();
            self.pomodoro.enchainer = true;
        }
    }

    /// **« RÉPÉTER DANS 5 MIN »** — un compte éphémère par chose qui sonnait,
    /// lancé tout de suite, puis tout se tait.
    pub fn repeter(&mut self, maintenant_ms: i64) {
        // Le Pomodoro PROLONGE sa phase (voir `Pomodoro`), sans compte à part.
        if self.pomodoro.sonne {
            self.pomodoro.sonne = false;
            self.pomodoro.fin_ms = Some(maintenant_ms + REPORT_MS);
        }
        let noms = self.noms_qui_sonnent();
        self.arreter();
        for nom in noms {
            let id = self.prochain_id();
            self.comptes.push(Compte {
                id,
                nom,
                duree_ms: REPORT_MS,
                reste_ms: REPORT_MS,
                fin_ms: Some(maintenant_ms + REPORT_MS),
                sonne: false,
                ephemere: true,
            });
        }
    }

    /// Un compte neuf, LANCÉ tout de suite — c'est ce qu'on attend d'un bouton
    /// « 5 min ».
    pub fn ajouter_compte(&mut self, nom: &str, duree_ms: i64, maintenant_ms: i64) {
        if duree_ms <= 0 {
            return;
        }
        let id = self.prochain_id();
        self.comptes.push(Compte {
            id,
            nom: nom.trim().to_string(),
            duree_ms,
            reste_ms: duree_ms,
            fin_ms: Some(maintenant_ms + duree_ms),
            sonne: false,
            ephemere: false,
        });
    }

    fn compte(&mut self, id: u64) -> Option<&mut Compte> {
        self.comptes.iter_mut().find(|c| c.id == id)
    }

    pub fn demarrer(&mut self, id: u64, maintenant_ms: i64) {
        let Some(c) = self.compte(id) else { return };
        if c.fin_ms.is_some() {
            return;
        }
        if c.reste_ms <= 0 {
            c.reste_ms = c.duree_ms;
        }
        c.sonne = false;
        c.fin_ms = Some(maintenant_ms + c.reste_ms);
    }

    pub fn pause(&mut self, id: u64, maintenant_ms: i64) {
        let Some(c) = self.compte(id) else { return };
        if let Some(fin) = c.fin_ms.take() {
            c.reste_ms = (fin - maintenant_ms).max(0);
        }
    }

    pub fn reinitialiser(&mut self, id: u64) {
        let Some(c) = self.compte(id) else { return };
        c.fin_ms = None;
        c.reste_ms = c.duree_ms;
        c.sonne = false;
    }

    pub fn supprimer_compte(&mut self, id: u64) {
        self.comptes.retain(|c| c.id != id);
    }

    /// Crée (`id == None`) ou remplace une alarme, et l'ARME.
    ///
    /// ⚠️ **ARMER APRÈS L'HEURE NE LA FAIT PAS SONNER AUJOURD'HUI** : une alarme
    /// de 7 h posée à 7 h 10 est pour DEMAIN, pas pour tout de suite — d'où
    /// `derniere` posé au jour même.
    pub fn enregistrer_alarme(
        &mut self,
        id: Option<u64>,
        nom: &str,
        heure_alarme: (u32, u32),
        jours: u8,
        son: Option<String>,
        jour: (i32, u32, u32),
        heure: (u32, u32),
    ) {
        let derniere = (minutes(heure_alarme) <= minutes(heure)).then_some(jour);
        let alarme = Alarme {
            id: id.unwrap_or_else(|| self.prochain_id()),
            nom: nom.trim().to_string(),
            heure: heure_alarme,
            jours: jours & crate::win32::notes::TOUS_LES_JOURS,
            active: true,
            son,
            derniere,
            sonne: false,
        };
        match self.alarmes.iter_mut().find(|a| Some(a.id) == id) {
            Some(existante) => *existante = alarme,
            None => self.alarmes.push(alarme),
        }
        self.alarmes.sort_by_key(|a| (a.heure, a.id));
    }

    /// Allume ou éteint une alarme — l'allumer l'ARME (même règle que
    /// `enregistrer_alarme`).
    pub fn basculer_alarme(&mut self, id: u64, jour: (i32, u32, u32), heure: (u32, u32)) {
        let Some(a) = self.alarmes.iter_mut().find(|a| a.id == id) else { return };
        a.active = !a.active;
        a.sonne = false;
        if a.active {
            a.derniere = (minutes(a.heure) <= minutes(heure)).then_some(jour);
        }
    }

    pub fn supprimer_alarme(&mut self, id: u64) {
        self.alarmes.retain(|a| a.id != id);
    }

    /// Le compte en cours qui finit le plus tôt : `(nom, reste_ms)` — pour
    /// l'infobulle de l'applet.
    pub fn prochaine_fin(&self, maintenant_ms: i64) -> Option<(String, i64)> {
        self.comptes
            .iter()
            .filter_map(|c| c.fin_ms.map(|fin| (nom_ou(&c.nom, "Compte à rebours"), (fin - maintenant_ms).max(0))))
            .chain(self.pomodoro.fin_ms.map(|fin| (self.pomodoro.phase.nom().to_string(), (fin - maintenant_ms).max(0))))
            .min_by_key(|(_, reste)| *reste)
    }
}

fn nom_ou(nom: &str, defaut: &str) -> String {
    if nom.is_empty() { defaut.to_string() } else { nom.to_string() }
}

/// `3 725 000` ms → `1:02:05` ; `65 000` → `1:05`. Arrondi à la seconde
/// SUPÉRIEURE : un compte qui affiche `0:00` doit avoir fini.
pub fn format_duree(ms: i64) -> String {
    let s = (ms.max(0) + 999) / 1000;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

// ---------------------------------------------------------------------------
// FICHIER
// ---------------------------------------------------------------------------

fn lire_jour(texte: &str) -> Option<(i32, u32, u32)> {
    let mut p = texte.split('-');
    let a = p.next()?.parse().ok()?;
    let m = p.next()?.parse().ok()?;
    let j = p.next()?.parse().ok()?;
    (p.next().is_none() && (1..=12).contains(&m) && (1..=31).contains(&j)).then_some((a, m, j))
}

fn lire_heure(texte: &str) -> Option<(u32, u32)> {
    let (h, m) = texte.split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

/// Relit `alarmes.toml`. Une ligne illisible est OMISE plutôt que de faire
/// perdre tout le fichier — même discipline que `config::load_notes`.
pub fn depuis_toml(texte: &str) -> Minuteur {
    let Ok(table) = texte.parse::<toml::Table>() else { return Minuteur::default() };
    let s = |v: &toml::Value, cle: &str| v.get(cle).and_then(|x| x.as_str()).map(str::to_string);
    let i = |v: &toml::Value, cle: &str| v.get(cle).and_then(|x| x.as_integer());
    let b = |v: &toml::Value, cle: &str| v.get(cle).and_then(|x| x.as_bool()).unwrap_or(false);
    let lignes = |cle: &str| table.get(cle).and_then(|v| v.as_array()).cloned().unwrap_or_default();

    let comptes = lignes("comptes")
        .iter()
        .filter_map(|v| {
            let duree_ms = i(v, "duree_ms").filter(|d| *d > 0)?;
            Some(Compte {
                id: i(v, "id")? as u64,
                nom: s(v, "nom").unwrap_or_default(),
                duree_ms,
                reste_ms: i(v, "reste_ms").unwrap_or(duree_ms).clamp(0, duree_ms),
                fin_ms: i(v, "fin_ms"),
                sonne: b(v, "sonne"),
                ephemere: b(v, "ephemere"),
            })
        })
        .collect();
    let alarmes = lignes("alarmes")
        .iter()
        .filter_map(|v| {
            let jours = v
                .get("jours")
                .and_then(|x| x.as_array())
                .map(|noms| {
                    noms.iter()
                        .filter_map(|n| n.as_str())
                        .filter_map(|n| JOURS_SEMAINE.iter().position(|j| *j == n))
                        .fold(0u8, |m, i| m | (1 << i))
                })
                .unwrap_or(0);
            Some(Alarme {
                id: i(v, "id")? as u64,
                nom: s(v, "nom").unwrap_or_default(),
                heure: lire_heure(&s(v, "heure")?)?,
                jours,
                active: b(v, "active"),
                son: s(v, "son"),
                derniere: s(v, "derniere").as_deref().and_then(lire_jour),
                // Une alarme qui sonnait à la fermeture ne resonne pas au
                // relancement : elle a déjà eu son jour (`derniere`).
                sonne: false,
            })
        })
        .collect();
    let pomodoro = table.get("pomodoro").map_or_else(Pomodoro::default, |v| {
        let mut p = Pomodoro::default();
        let minutes = |cle: &str, defaut: u32| {
            i(v, cle).map_or(defaut, |m| m.clamp(i64::from(POMODORO_MIN_MINUTES), i64::from(POMODORO_MAX_MINUTES)) as u32)
        };
        p.travail_min = minutes("travail_min", p.travail_min);
        p.pause_min = minutes("pause_min", p.pause_min);
        p.longue_min = minutes("longue_min", p.longue_min);
        p.phase = s(v, "phase").as_deref().and_then(PhasePomodoro::depuis_cle).unwrap_or_default();
        p.seances = i(v, "seances").map_or(0, |n| n.clamp(0, 10_000) as u32);
        let pleine = p.duree_ms(p.phase);
        p.reste_ms = i(v, "reste_ms").unwrap_or(pleine).clamp(0, pleine);
        p.fin_ms = i(v, "fin_ms");
        p.sonne = b(v, "sonne");
        p.enchainer = b(v, "enchainer");
        p
    });
    let chrono = table.get("chronometre").map_or_else(Chronometre::default, |v| Chronometre {
        marche: i(v, "marche"),
        accumule_ms: i(v, "accumule_ms").unwrap_or(0).max(0),
        tours: v
            .get("tours")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(toml::Value::as_integer).filter(|t| *t >= 0).take(Chronometre::MAX_TOURS).collect())
            .unwrap_or_default(),
    });
    Minuteur {
        comptes,
        alarmes,
        son_comptes: table.get("son_comptes").and_then(|v| v.as_str()).map(str::to_string),
        pomodoro,
        chrono,
    }
}

pub fn vers_toml(m: &Minuteur) -> String {
    use toml::Value;
    let mut table = toml::Table::new();
    if let Some(son) = &m.son_comptes {
        table.insert("son_comptes".into(), Value::String(son.clone()));
    }
    let comptes = m
        .comptes
        .iter()
        .map(|c| {
            let mut t = toml::Table::new();
            t.insert("id".into(), Value::Integer(c.id as i64));
            t.insert("nom".into(), Value::String(c.nom.clone()));
            t.insert("duree_ms".into(), Value::Integer(c.duree_ms));
            t.insert("reste_ms".into(), Value::Integer(c.reste_ms));
            if let Some(fin) = c.fin_ms {
                t.insert("fin_ms".into(), Value::Integer(fin));
            }
            t.insert("sonne".into(), Value::Boolean(c.sonne));
            t.insert("ephemere".into(), Value::Boolean(c.ephemere));
            Value::Table(t)
        })
        .collect();
    let alarmes = m
        .alarmes
        .iter()
        .map(|a| {
            let mut t = toml::Table::new();
            t.insert("id".into(), Value::Integer(a.id as i64));
            t.insert("nom".into(), Value::String(a.nom.clone()));
            t.insert("heure".into(), Value::String(format!("{:02}:{:02}", a.heure.0, a.heure.1)));
            let jours = (0..7).filter(|i| a.jours & (1 << i) != 0).map(|i| Value::String(JOURS_SEMAINE[i].into())).collect();
            t.insert("jours".into(), Value::Array(jours));
            t.insert("active".into(), Value::Boolean(a.active));
            if let Some(son) = &a.son {
                t.insert("son".into(), Value::String(son.clone()));
            }
            if let Some((an, mo, jo)) = a.derniere {
                t.insert("derniere".into(), Value::String(format!("{an:04}-{mo:02}-{jo:02}")));
            }
            Value::Table(t)
        })
        .collect();
    table.insert("comptes".into(), Value::Array(comptes));
    table.insert("alarmes".into(), Value::Array(alarmes));
    let p = &m.pomodoro;
    let mut pomodoro = toml::Table::new();
    pomodoro.insert("phase".into(), Value::String(p.phase.cle().into()));
    pomodoro.insert("seances".into(), Value::Integer(i64::from(p.seances)));
    pomodoro.insert("reste_ms".into(), Value::Integer(p.reste_ms));
    if let Some(fin) = p.fin_ms {
        pomodoro.insert("fin_ms".into(), Value::Integer(fin));
    }
    pomodoro.insert("sonne".into(), Value::Boolean(p.sonne));
    pomodoro.insert("enchainer".into(), Value::Boolean(p.enchainer));
    pomodoro.insert("travail_min".into(), Value::Integer(i64::from(p.travail_min)));
    pomodoro.insert("pause_min".into(), Value::Integer(i64::from(p.pause_min)));
    pomodoro.insert("longue_min".into(), Value::Integer(i64::from(p.longue_min)));
    table.insert("pomodoro".into(), Value::Table(pomodoro));
    let c = &m.chrono;
    let mut chrono = toml::Table::new();
    if let Some(marche) = c.marche {
        chrono.insert("marche".into(), Value::Integer(marche));
    }
    chrono.insert("accumule_ms".into(), Value::Integer(c.accumule_ms));
    chrono.insert("tours".into(), Value::Array(c.tours.iter().map(|t| Value::Integer(*t)).collect()));
    table.insert("chronometre".into(), Value::Table(chrono));
    toml::to_string(&table).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// L'ÉTAT DE L'APPLICATION
// ---------------------------------------------------------------------------

thread_local! {
    static ETAT: RefCell<Minuteur> = RefCell::new(Minuteur::default());
}

/// Relit `alarmes.toml` — une fois, au démarrage (voir `app::run`).
pub fn charger() {
    let texte = config::config_path(FICHIER).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    ETAT.with(|e| *e.borrow_mut() = depuis_toml(&texte));
}

fn enregistrer(m: &Minuteur) {
    if !config::write_config_atomique(FICHIER, vers_toml(m)) {
        crate::diag!("[minuteur] alarmes.toml n'a pas pu être écrit");
    }
}

/// Lit l'état.
pub fn lire<R>(f: impl FnOnce(&Minuteur) -> R) -> R {
    ETAT.with(|e| f(&e.borrow()))
}

/// Modifie l'état PUIS l'enregistre — le seul chemin d'écriture.
pub fn modifier<R>(f: impl FnOnce(&mut Minuteur) -> R) -> R {
    ETAT.with(|e| {
        let mut m = e.borrow_mut();
        let r = f(&mut m);
        enregistrer(&m);
        r
    })
}

/// L'instant Unix en millisecondes.
pub fn maintenant_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Le jour et l'heure LOCAUX, ce qu'attend `Minuteur::avancer`.
pub fn jour_et_heure() -> ((i32, u32, u32), (u32, u32)) {
    let t = unsafe { GetLocalTime() };
    ((t.wYear as i32, t.wMonth as u32, t.wDay as u32), (t.wHour as u32, t.wMinute as u32))
}

/// Le pas de temps de chaque frame (voir `app::run`). N'écrit le fichier que
/// si quelque chose vient de sonner.
pub fn avancer() {
    let (jour, heure) = jour_et_heure();
    let maintenant = maintenant_ms();
    ETAT.with(|e| {
        let mut m = e.borrow_mut();
        if m.avancer(maintenant, jour, heure) {
            enregistrer(&m);
        }
    });
}

pub fn sonne() -> bool {
    lire(Minuteur::sonne)
}

/// L'infobulle de l'applet : « Minuteur », ou le compte qui finit le plus tôt
/// (« Minuteur — thé 4:32 »). Le temps restant ne s'affiche que LÀ : le
/// dessiner dans la barre demanderait une texture de texte par seconde.
pub fn infobulle() -> String {
    match lire(|m| m.prochaine_fin(maintenant_ms())) {
        Some((nom, reste)) => format!("Minuteur — {nom} {}", format_duree(reste)),
        None => "Minuteur".to_string(),
    }
}

/// Le chemin d'un son choisi, SEULEMENT s'il est bien un fichier du dossier
/// `son/` (voir `son::fichiers`) — un nom venu de la page ne peut donc
/// désigner rien d'autre.
pub fn chemin_du_son(nom: &str) -> Option<std::path::PathBuf> {
    crate::win32::son::fichiers().into_iter().find(|p| crate::win32::son::nom(p) == nom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_pomodoro_enchaine_travail_pause_et_longue_pause() {
        let mut m = Minuteur::default();
        let (jour, heure) = ((2026, 10, 24), (9, 0));
        m.pomodoro.demarrer(0);
        let travail = m.pomodoro.duree_ms(PhasePomodoro::Travail);
        let mut t = travail;
        for seance in 1..=SEANCES_AVANT_LONGUE {
            assert!(m.avancer(t, jour, heure), "la phase de travail finit");
            assert!(m.sonne() && m.noms_qui_sonnent() == ["Pomodoro — travail"]);
            m.arreter();
            assert!(!m.sonne());
            let attendue = if seance == SEANCES_AVANT_LONGUE { PhasePomodoro::LonguePause } else { PhasePomodoro::Pause };
            assert_eq!(m.pomodoro.phase, attendue, "séance {seance}");
            assert_eq!(m.pomodoro.fin_ms, None, "lancée à la frame SUIVANTE");
            assert!(m.avancer(t, jour, heure));
            let pause = m.pomodoro.duree_ms(attendue);
            assert_eq!(m.pomodoro.fin_ms, Some(t + pause));
            t += pause;
            assert!(m.avancer(t, jour, heure));
            m.arreter();
            assert_eq!(m.pomodoro.phase, PhasePomodoro::Travail);
            assert!(m.avancer(t, jour, heure));
            t += travail;
        }
        assert_eq!(m.pomodoro.seances, SEANCES_AVANT_LONGUE);
    }

    #[test]
    fn repeter_prolonge_la_phase_du_pomodoro_sans_creer_de_compte() {
        let mut m = Minuteur::default();
        m.pomodoro.demarrer(0);
        let fin = m.pomodoro.duree_ms(PhasePomodoro::Travail);
        m.avancer(fin, (2026, 10, 24), (9, 0));
        m.repeter(fin);
        assert!(!m.sonne());
        assert!(m.comptes.is_empty());
        assert_eq!(m.pomodoro.phase, PhasePomodoro::Travail);
        assert_eq!(m.pomodoro.fin_ms, Some(fin + REPORT_MS));
    }

    #[test]
    fn le_pomodoro_survit_au_fichier_et_borne_ses_durees() {
        let mut m = Minuteur::default();
        m.pomodoro.regler(PhasePomodoro::Travail, 50);
        m.pomodoro.regler(PhasePomodoro::Pause, 0);
        m.pomodoro.passer(1000);
        let relu = depuis_toml(&vers_toml(&m));
        assert_eq!(relu.pomodoro, m.pomodoro);
        assert_eq!(relu.pomodoro.pause_min, POMODORO_MIN_MINUTES);
        assert_eq!(relu.pomodoro.phase, PhasePomodoro::Pause);
        assert_eq!(depuis_toml("").pomodoro, Pomodoro::default(), "un ancien fichier sans pomodoro");
    }

    #[test]
    fn le_chronometre_compte_vers_le_haut_se_met_en_pause_et_reprend() {
        let mut c = Chronometre::default();
        assert_eq!(c.ecoule(1_000), 0, "arrêté à zéro");
        c.demarrer(1_000);
        assert!(c.en_marche());
        assert_eq!(c.ecoule(4_000), 3_000, "3 s écoulées");
        c.tour(4_000);
        c.pause(4_000);
        assert!(!c.en_marche());
        assert_eq!(c.ecoule(10_000), 3_000, "en pause, le temps ne passe pas");
        // La reprise CONTINUE à partir de l'accumulé, pas de zéro.
        c.demarrer(10_000);
        assert_eq!(c.ecoule(12_000), 5_000, "3 s + 2 s");
        assert_eq!(c.tours, vec![3_000]);
        c.reinitialiser();
        assert_eq!(c, Chronometre::default());
    }

    #[test]
    fn un_tour_a_l_arret_est_ignore() {
        let mut c = Chronometre::default();
        c.tour(1_000);
        assert!(c.tours.is_empty(), "pas de tour sans marche");
    }

    const VENDREDI: (i32, u32, u32) = (2026, 9, 25);
    const SAMEDI: (i32, u32, u32) = (2026, 9, 26);

    #[test]
    fn un_compte_sonne_a_son_instant_de_fin_et_se_met_en_pause() {
        let mut m = Minuteur::default();
        m.ajouter_compte(" thé ", 180_000, 1_000);
        assert_eq!(m.comptes[0].nom, "thé");
        assert!(!m.avancer(100_000, VENDREDI, (12, 0)));
        m.pause(1, 100_000);
        assert_eq!(m.comptes[0].reste_ms, 81_000);
        assert!(!m.avancer(10_000_000, VENDREDI, (12, 0)), "en pause, le temps ne passe pas");
        m.demarrer(1, 10_000_000);
        assert!(m.avancer(10_081_000, VENDREDI, (12, 0)));
        assert!(m.sonne());
        m.arreter();
        assert!(!m.sonne());
        assert_eq!(m.comptes[0].reste_ms, 180_000, "arrêté, il revient à sa durée");
    }

    #[test]
    fn repeter_cree_un_compte_ephemere_de_cinq_minutes() {
        let mut m = Minuteur::default();
        m.ajouter_compte("pâtes", 1_000, 0);
        m.avancer(1_000, VENDREDI, (12, 0));
        m.repeter(2_000);
        assert!(!m.sonne());
        let rappel = m.comptes.iter().find(|c| c.ephemere).expect("le report existe");
        assert_eq!(rappel.nom, "pâtes");
        assert_eq!(rappel.fin_ms, Some(2_000 + REPORT_MS));
        m.avancer(2_000 + REPORT_MS, VENDREDI, (12, 5));
        m.arreter();
        assert!(m.comptes.iter().all(|c| !c.ephemere), "un report arrêté disparaît");
    }

    #[test]
    fn une_alarme_sonne_une_fois_par_jour_choisi() {
        let mut m = Minuteur::default();
        // Du lundi au vendredi à 7 h, posée la veille au soir.
        m.enregistrer_alarme(None, "réveil", (7, 0), 0b0001_1111, None, (2026, 9, 24), (22, 0));
        assert!(!m.avancer(0, VENDREDI, (6, 59)));
        assert!(m.avancer(0, VENDREDI, (7, 0)));
        m.arreter();
        assert!(!m.avancer(0, VENDREDI, (7, 1)), "une seule fois ce jour-là");
        assert!(!m.avancer(0, SAMEDI, (7, 0)), "pas le samedi");
        assert!(m.alarmes[0].active, "une alarme répétée reste active");
    }

    #[test]
    fn une_alarme_posee_apres_son_heure_attend_demain() {
        let mut m = Minuteur::default();
        m.enregistrer_alarme(None, "", (7, 0), 0, None, VENDREDI, (7, 10));
        assert!(!m.avancer(0, VENDREDI, (7, 10)));
        assert!(m.avancer(0, SAMEDI, (7, 5)), "le lendemain, dans le rattrapage");
        assert!(!m.alarmes[0].active, "une alarme « une fois » se range après avoir sonné");
    }

    #[test]
    fn une_alarme_trop_en_retard_ne_sonne_plus() {
        let mut m = Minuteur::default();
        m.enregistrer_alarme(None, "", (7, 0), notes_tous(), None, (2026, 9, 24), (22, 0));
        assert!(!m.avancer(0, VENDREDI, (7, RATTRAPAGE_MINUTES)));
    }

    fn notes_tous() -> u8 {
        crate::win32::notes::TOUS_LES_JOURS
    }

    #[test]
    fn le_fichier_fait_l_aller_retour() {
        let mut m = Minuteur { son_comptes: Some("gong.mp3".into()), ..Default::default() };
        m.ajouter_compte("four", 600_000, 5_000);
        m.ajouter_compte("", 60_000, 5_000);
        m.pause(2, 20_000);
        m.enregistrer_alarme(None, "réveil", (6, 45), 0b0100_0001, Some("cloche.mp3".into()), VENDREDI, (8, 0));
        m.chrono.demarrer(2_000);
        m.chrono.tour(5_000);
        let relu = depuis_toml(&vers_toml(&m));
        assert_eq!(relu, m);
    }

    #[test]
    fn un_fichier_abime_ne_fait_perdre_que_ses_lignes_abimees() {
        let texte = r#"
[[comptes]]
id = 1
duree_ms = 0

[[comptes]]
id = 2
nom = "ok"
duree_ms = 1000

[[alarmes]]
id = 3
heure = "25:00"
"#;
        let m = depuis_toml(texte);
        assert_eq!(m.comptes.len(), 1);
        assert!(m.alarmes.is_empty());
        assert_eq!(depuis_toml("pas du toml ["), Minuteur::default());
    }

    #[test]
    fn la_duree_s_affiche_arrondie_a_la_seconde_superieure() {
        assert_eq!(format_duree(65_000), "1:05");
        assert_eq!(format_duree(64_001), "1:05");
        assert_eq!(format_duree(3_725_000), "1:02:05");
        assert_eq!(format_duree(0), "0:00");
    }
}
