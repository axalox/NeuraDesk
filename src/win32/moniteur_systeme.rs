//! **LE MONITEUR SYSTÈME** — processeur, mémoire, disque, réseau et carte
//! graphique, avec un historique (voir `win32::cef_moniteur`, le panneau, et
//! `assets/popups/moniteur.html`).
//!
//! DEMANDE EXPLICITE (2026-10-20) : « une applet de monitoring du système avec
//! historique […] faite de façon à pouvoir être utilisée sur n'importe quel
//! ordinateur sous Windows 11 ».
//!
//! ## ⛔ CE QUI LE REND PORTABLE
//!
//! - **Uniquement des API présentes sur tout Windows 11**, sans droits
//!   administrateur : `GetSystemTimes` et `GlobalMemoryStatusEx` (voir
//!   `win32::sysinfo`), `GetIfTable2` (voir `win32::debit_reseau`), et les
//!   compteurs de performance (PDH). Ni WMI, ni outil externe, ni bibliothèque
//!   d'un fabricant de carte graphique.
//! - ⛔ **Les compteurs PDH sont désignés par leur nom ANGLAIS**
//!   (`PdhAddEnglishCounterW`) : leur nom affiché est TRADUIT (« Disque
//!   physique » sur un Windows français), et `PdhAddCounterW` ne trouverait
//!   `\PhysicalDisk(...)` que sur un Windows anglais.
//! - **Une mesure absente est `NaN`, jamais une erreur** : une machine
//!   virtuelle sans carte graphique n'a pas de compteurs « GPU Engine », et le
//!   panneau masque simplement ce qu'il n'a jamais reçu.
//! - ⛔ **Pas de températures** : aucune lecture ne marche partout sans droits
//!   administrateur ni pilote (choix confirmé le 2026-10-20).
//!
//! ## L'HISTORIQUE
//!
//! Un relevé par seconde, sur un fil à part, **même panneau fermé** — sinon il
//! n'y aurait d'historique que du temps où il était ouvert. La dernière heure
//! reste en mémoire seconde par seconde ; chaque minute écoulée devient une
//! `Minute` (moyenne et maximum de chaque série), ajoutée au fichier
//! `moniteur/historique.bin` et gardée `CONSERVATION_S` (7 jours, ~770 Ko).
//!
//! **Chaque disque et chaque carte graphique a SON historique** (voir
//! `moniteur_peripheriques`) : mêmes `Seconde` et `Minute`, seules ses séries
//! remplies, dans son propre fichier `moniteur/<sorte>-<empreinte>.bin`.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::core::{w, PCWSTR};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W,
    PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use crate::config;
use crate::win32::debit_reseau;
use crate::win32::moniteur_peripheriques::{self, Peripherique};
use crate::win32::moniteur_taches;
use crate::win32::sysinfo::CpuSampler;

/// L'historique sur disque est gardé 7 jours.
pub const CONSERVATION_S: u32 = 7 * 24 * 3600;
/// La dernière heure, seconde par seconde, en mémoire.
const SECONDES_GARDEES: usize = 3600;
const DOSSIER: &str = "moniteur";
const FICHIER: &str = "historique.bin";

// ---------------------------------------------------------------------------
// LES SÉRIES
// ---------------------------------------------------------------------------

/// Ce qu'on mesure. ⚠️ **L'ORDRE EST CELUI DU FICHIER** : une série ajoutée
/// change la taille d'un enregistrement (voir `TAILLE_MINUTE`) — changer alors
/// aussi le nom du fichier, sinon l'ancien se relirait de travers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Serie {
    /// Processeur, en %.
    Cpu,
    /// Mémoire vive occupée, en %.
    Ram,
    /// Activité du disque, en % (100 − le temps d'inactivité, comme le
    /// Gestionnaire des tâches).
    Disque,
    /// Octets lus par seconde, tous disques.
    Lecture,
    /// Octets écrits par seconde, tous disques.
    Ecriture,
    /// Octets envoyés par seconde, toutes cartes réseau.
    Envoi,
    /// Octets reçus par seconde, toutes cartes réseau.
    Reception,
    /// Carte graphique, en % (le moteur le plus occupé, comme le Gestionnaire
    /// des tâches).
    Gpu,
    /// Mémoire dédiée de la carte graphique occupée, en octets.
    GpuMemoire,
}

pub const NB_SERIES: usize = 9;

impl Serie {
    pub const TOUTES: [Serie; NB_SERIES] = [
        Serie::Cpu,
        Serie::Ram,
        Serie::Disque,
        Serie::Lecture,
        Serie::Ecriture,
        Serie::Envoi,
        Serie::Reception,
        Serie::Gpu,
        Serie::GpuMemoire,
    ];

    /// Le nom qui sert de clé à la page.
    pub fn nom(self) -> &'static str {
        match self {
            Serie::Cpu => "cpu",
            Serie::Ram => "ram",
            Serie::Disque => "disque",
            Serie::Lecture => "lecture",
            Serie::Ecriture => "ecriture",
            Serie::Envoi => "envoi",
            Serie::Reception => "reception",
            Serie::Gpu => "gpu",
            Serie::GpuMemoire => "gpuMemoire",
        }
    }
}

/// Une valeur par série, dans l'ordre de `Serie::TOUTES` ; `NaN` = pas de mesure.
pub type Valeurs = [f32; NB_SERIES];

pub const VIDE: Valeurs = [f32::NAN; NB_SERIES];

/// Un relevé d'une seconde.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seconde {
    pub instant_s: u32,
    pub valeurs: Valeurs,
}

/// Une minute écoulée : la moyenne et le maximum de chaque série.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Minute {
    /// Le début de la minute.
    pub instant_s: u32,
    pub moyennes: Valeurs,
    pub max: Valeurs,
}

/// Ce que le panneau montre de l'instant, sans l'historiser.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Detail {
    /// L'utilisation de chaque cœur, dans l'ordre des processeurs logiques.
    pub coeurs: Vec<f32>,
    pub ram_utilisee: u64,
    pub ram_totale: u64,
    /// Les plus gourmandes en processeur — relevées panneau ouvert seulement
    /// (voir `moniteur_taches`).
    pub taches: Vec<moniteur_taches::Tache>,
}

// ---------------------------------------------------------------------------
// LE FICHIER — 4 + 2 × 9 × 4 = 76 octets par minute, petit-boutiste
// ---------------------------------------------------------------------------

pub const TAILLE_MINUTE: usize = 4 + 2 * NB_SERIES * 4;

impl Minute {
    pub fn encoder(&self) -> [u8; TAILLE_MINUTE] {
        let mut octets = [0u8; TAILLE_MINUTE];
        octets[..4].copy_from_slice(&self.instant_s.to_le_bytes());
        for (i, v) in self.moyennes.iter().chain(self.max.iter()).enumerate() {
            octets[4 + i * 4..8 + i * 4].copy_from_slice(&v.to_le_bytes());
        }
        octets
    }

    pub fn decoder(octets: &[u8]) -> Option<Minute> {
        let octets: &[u8; TAILLE_MINUTE] = octets.try_into().ok()?;
        let lire = |i: usize| f32::from_le_bytes([octets[i], octets[i + 1], octets[i + 2], octets[i + 3]]);
        let mut moyennes = VIDE;
        let mut max = VIDE;
        for s in 0..NB_SERIES {
            moyennes[s] = lire(4 + s * 4);
            max[s] = lire(4 + (NB_SERIES + s) * 4);
        }
        Some(Minute { instant_s: u32::from_le_bytes([octets[0], octets[1], octets[2], octets[3]]), moyennes, max })
    }
}

/// Les minutes d'un fichier. Un enregistrement final TRONQUÉ (écriture
/// interrompue) est ignoré, et le désordre (horloge reculée) est trié.
pub fn decoder_historique(octets: &[u8]) -> Vec<Minute> {
    let mut minutes: Vec<Minute> = octets.chunks_exact(TAILLE_MINUTE).filter_map(Minute::decoder).collect();
    minutes.sort_by_key(|m| m.instant_s);
    minutes
}

/// L'indice de la première minute à garder à l'instant `maintenant_s`.
pub fn premier_a_garder(minutes: &[Minute], maintenant_s: u32) -> usize {
    let limite = maintenant_s.saturating_sub(CONSERVATION_S);
    minutes.partition_point(|m| m.instant_s < limite)
}

// ---------------------------------------------------------------------------
// LA MINUTE EN COURS
// ---------------------------------------------------------------------------

/// Cumule les secondes d'une minute ; rend la minute précédente quand une
/// seconde d'une AUTRE minute arrive.
#[derive(Clone, Debug, Default)]
pub struct Cumul {
    minute_s: Option<u32>,
    sommes: [f64; NB_SERIES],
    comptes: [u32; NB_SERIES],
    max: [f32; NB_SERIES],
}

impl Cumul {
    pub fn ajouter(&mut self, seconde: &Seconde) -> Option<Minute> {
        let minute_s = seconde.instant_s - seconde.instant_s % 60;
        let finie = match self.minute_s {
            Some(courante) if courante != minute_s => self.minute(courante),
            _ => None,
        };
        if self.minute_s != Some(minute_s) {
            *self = Cumul { minute_s: Some(minute_s), sommes: [0.0; NB_SERIES], comptes: [0; NB_SERIES], max: VIDE };
        }
        for (s, v) in seconde.valeurs.iter().enumerate() {
            if v.is_finite() {
                self.sommes[s] += f64::from(*v);
                self.comptes[s] += 1;
                self.max[s] = if self.max[s].is_finite() { self.max[s].max(*v) } else { *v };
            }
        }
        finie
    }

    /// La minute cumulée, ou `None` si aucune série n'y a de mesure.
    fn minute(&self, instant_s: u32) -> Option<Minute> {
        let mut moyennes = VIDE;
        for s in 0..NB_SERIES {
            if self.comptes[s] > 0 {
                moyennes[s] = (self.sommes[s] / f64::from(self.comptes[s])) as f32;
            }
        }
        self.comptes.iter().any(|&n| n > 0).then_some(Minute { instant_s, moyennes, max: self.max })
    }
}

// ---------------------------------------------------------------------------
// LES PLAGES ET LE REGROUPEMENT
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Plage {
    #[default]
    CinqMinutes,
    Heure,
    Jour,
    Semaine,
}

impl Plage {
    pub const TOUTES: [Plage; 4] = [Plage::CinqMinutes, Plage::Heure, Plage::Jour, Plage::Semaine];

    pub fn nom(self) -> &'static str {
        match self {
            Plage::CinqMinutes => "5min",
            Plage::Heure => "1h",
            Plage::Jour => "24h",
            Plage::Semaine => "7j",
        }
    }

    pub fn depuis_nom(nom: &str) -> Option<Plage> {
        Plage::TOUTES.into_iter().find(|p| p.nom() == nom)
    }

    /// `(durée en secondes, nombre de points)`. Les deux premières lisent les
    /// SECONDES en mémoire, les deux autres les MINUTES.
    pub fn decoupage(self) -> (u32, usize) {
        match self {
            Plage::CinqMinutes => (300, 300),
            Plage::Heure => (3600, 360),
            Plage::Jour => (86_400, 288),
            Plage::Semaine => (7 * 86_400, 336),
        }
    }

    pub fn lit_les_minutes(self) -> bool {
        matches!(self, Plage::Jour | Plage::Semaine)
    }
}

/// Un point regroupé : moyenne et maximum de chaque série (`NaN` sans mesure).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub moyennes: Valeurs,
    pub max: Valeurs,
}

/// PURE : range `(instant, moyennes, max)` dans `nb` points de `pas_s`
/// secondes à partir de `debut_s`. Chaque série se regroupe à part : un `NaN`
/// n'efface pas les mesures des autres.
pub fn regrouper<'a>(
    entrees: impl Iterator<Item = (u32, &'a Valeurs, &'a Valeurs)>,
    debut_s: u32,
    pas_s: u32,
    nb: usize,
) -> Vec<Point> {
    let mut sommes = vec![[0.0f64; NB_SERIES]; nb];
    let mut comptes = vec![[0u32; NB_SERIES]; nb];
    let mut max = vec![VIDE; nb];
    for (instant_s, moyennes, maxima) in entrees {
        if instant_s < debut_s || pas_s == 0 {
            continue;
        }
        let i = ((instant_s - debut_s) / pas_s) as usize;
        if i >= nb {
            continue;
        }
        for s in 0..NB_SERIES {
            if moyennes[s].is_finite() {
                sommes[i][s] += f64::from(moyennes[s]);
                comptes[i][s] += 1;
            }
            if maxima[s].is_finite() {
                max[i][s] = if max[i][s].is_finite() { max[i][s].max(maxima[s]) } else { maxima[s] };
            }
        }
    }
    (0..nb)
        .map(|i| {
            let mut moyennes = VIDE;
            for s in 0..NB_SERIES {
                if comptes[i][s] > 0 {
                    moyennes[s] = (sommes[i][s] / f64::from(comptes[i][s])) as f32;
                }
            }
            Point { moyennes, max: max[i] }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// LES CALCULS PURS SUR LES COMPTEURS
// ---------------------------------------------------------------------------

/// PURE : l'utilisation de la carte graphique d'après les instances de
/// `\GPU Engine(*)\Utilization Percentage`, nommées
/// `pid_1234_luid_0x…_0x…_phys_0_eng_3_engtype_3D`.
///
/// Comme le Gestionnaire des tâches : on SOMME les processus d'un même moteur
/// (tout ce qui suit `luid_`), puis on garde le moteur le PLUS occupé. Sans
/// instance (pas de carte, ou une machine virtuelle) : `None`.
pub fn utilisation_gpu(instances: &[(String, f64)]) -> Option<f32> {
    let mut moteurs: Vec<(&str, f64)> = Vec::new();
    for (nom, valeur) in instances {
        let Some(debut) = nom.find("luid_") else { continue };
        let moteur = &nom[debut..];
        match moteurs.iter_mut().find(|(m, _)| *m == moteur) {
            Some((_, somme)) => *somme += valeur,
            None => moteurs.push((moteur, *valeur)),
        }
    }
    moteurs.iter().map(|(_, somme)| *somme).reduce(f64::max).map(|v| v.clamp(0.0, 100.0) as f32)
}

/// PURE : l'utilisation de chaque cœur d'après `\Processor(*)\% Processor Time`,
/// dans l'ordre NUMÉRIQUE des processeurs (« 10 » après « 9 ») et sans
/// l'instance `_Total`.
pub fn utilisation_des_coeurs(instances: &[(String, f64)]) -> Vec<f32> {
    let mut coeurs: Vec<(Vec<u32>, f32)> = instances
        .iter()
        .filter(|(nom, _)| !nom.contains("_Total"))
        .filter_map(|(nom, v)| {
            let rang: Option<Vec<u32>> = nom.split(',').map(|n| n.trim().parse().ok()).collect();
            Some((rang?, v.clamp(0.0, 100.0) as f32))
        })
        .collect();
    coeurs.sort_by(|a, b| a.0.cmp(&b.0));
    coeurs.into_iter().map(|(_, v)| v).collect()
}

// ---------------------------------------------------------------------------
// LES CAPTEURS — sur le fil de mesure seulement
// ---------------------------------------------------------------------------

/// Une requête PDH et ses compteurs ; chacun peut manquer.
struct Pdh {
    requete: PDH_HQUERY,
    /// Par disque PLUS l'instance `_Total` (voir `total`).
    disques_inactif: Option<PDH_HCOUNTER>,
    disques_lecture: Option<PDH_HCOUNTER>,
    disques_ecriture: Option<PDH_HCOUNTER>,
    gpu_moteurs: Option<PDH_HCOUNTER>,
    gpu_memoire: Option<PDH_HCOUNTER>,
    coeurs: Option<PDH_HCOUNTER>,
}

impl Pdh {
    fn ouvrir() -> Option<Pdh> {
        let mut requete = PDH_HQUERY::default();
        // SAFETY : `requete` est un pointeur valide ; source nulle = données en direct.
        let statut = unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut requete) };
        if statut != 0 {
            crate::diag!("[moniteur] PdhOpenQueryW : {statut:#x}");
            return None;
        }
        let ajouter = |chemin: PCWSTR| {
            let mut compteur = PDH_HCOUNTER::default();
            // SAFETY : chemin littéral terminé par zéro, requête ouverte ci-dessus.
            let statut = unsafe { PdhAddEnglishCounterW(requete, chemin, 0, &mut compteur) };
            if statut != 0 {
                crate::diag!("[moniteur] compteur absent {:?} : {statut:#x}", unsafe { chemin.to_string() });
                return None;
            }
            Some(compteur)
        };
        let pdh = Pdh {
            requete,
            disques_inactif: ajouter(w!("\\PhysicalDisk(*)\\% Idle Time")),
            disques_lecture: ajouter(w!("\\PhysicalDisk(*)\\Disk Read Bytes/sec")),
            disques_ecriture: ajouter(w!("\\PhysicalDisk(*)\\Disk Write Bytes/sec")),
            gpu_moteurs: ajouter(w!("\\GPU Engine(*)\\Utilization Percentage")),
            gpu_memoire: ajouter(w!("\\GPU Adapter Memory(*)\\Dedicated Usage")),
            coeurs: ajouter(w!("\\Processor(*)\\% Processor Time")),
        };
        // Les compteurs de TAUX ont besoin de deux relevés : le premier sert de référence.
        // SAFETY : requête ouverte.
        unsafe { PdhCollectQueryData(pdh.requete) };
        Some(pdh)
    }

    fn relever(&self) -> bool {
        // SAFETY : requête ouverte.
        unsafe { PdhCollectQueryData(self.requete) == 0 }
    }

    /// L'instance `_Total` d'un compteur à joker.
    fn total(instances: &[(String, f64)]) -> Option<f64> {
        instances.iter().find(|(nom, _)| nom == "_Total").map(|(_, v)| *v)
    }

    /// Les instances d'un compteur à joker, `(nom, valeur)`.
    fn tableau(compteur: Option<PDH_HCOUNTER>) -> Vec<(String, f64)> {
        let Some(compteur) = compteur else { return Vec::new() };
        let (mut taille, mut nombre) = (0u32, 0u32);
        // SAFETY : premier appel sans tampon — il rend la taille nécessaire.
        let statut = unsafe { PdhGetFormattedCounterArrayW(compteur, PDH_FMT_DOUBLE, &mut taille, &mut nombre, None) };
        if statut != PDH_MORE_DATA || taille == 0 {
            return Vec::new();
        }
        // Le tampon porte les éléments PUIS les noms : on l'alloue en éléments
        // (alignement) assez nombreux pour couvrir `taille` octets.
        let element = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        let mut tampon = vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); (taille as usize).div_ceil(element)];
        // SAFETY : `tampon` couvre au moins `taille` octets.
        let statut =
            unsafe { PdhGetFormattedCounterArrayW(compteur, PDH_FMT_DOUBLE, &mut taille, &mut nombre, Some(tampon.as_mut_ptr())) };
        if statut != 0 {
            return Vec::new();
        }
        tampon
            .iter()
            .take(nombre as usize)
            .filter(|e| e.FmtValue.CStatus == PDH_CSTATUS_VALID_DATA || e.FmtValue.CStatus == PDH_CSTATUS_NEW_DATA)
            .filter_map(|e| {
                // SAFETY : `szName` pointe dans `tampon`, encore vivant ; `PDH_FMT_DOUBLE` demandé.
                let nom = unsafe { e.szName.to_string() }.ok()?;
                let valeur = unsafe { e.FmtValue.Anonymous.doubleValue };
                valeur.is_finite().then_some((nom, valeur))
            })
            .collect()
    }
}

impl Drop for Pdh {
    fn drop(&mut self) {
        // SAFETY : requête ouverte par `ouvrir`, fermée une seule fois.
        unsafe { PdhCloseQuery(self.requete) };
    }
}

struct Capteurs {
    cpu: CpuSampler,
    reseau: debit_reseau::Echantillonneur,
    pdh: Option<Pdh>,
    taches: moniteur_taches::Releveur,
    peripheriques: moniteur_peripheriques::Suivi,
}

impl Capteurs {
    fn new() -> Self {
        let mut capteurs = Capteurs { cpu: CpuSampler::new(), reseau: debit_reseau::Echantillonneur::new(), pdh: Pdh::ouvrir(), taches: Default::default(), peripheriques: Default::default() };
        // Les références des taux (processeur, réseau).
        capteurs.cpu.sample();
        capteurs.reseau.echantillonner();
        capteurs
    }

    fn mesurer(&mut self, instant_s: u32) -> (Seconde, Detail, moniteur_peripheriques::Releve) {
        let mut valeurs = VIDE;
        let mut detail = Detail::default();
        let mut poser = |serie: Serie, v: Option<f64>| {
            if let Some(v) = v {
                valeurs[serie as usize] = v as f32;
            }
        };
        poser(Serie::Cpu, self.cpu.sample().map(f64::from));
        let mut memoire = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        // SAFETY : `dwLength` est posé.
        if unsafe { GlobalMemoryStatusEx(&mut memoire) }.is_ok() && memoire.ullTotalPhys > 0 {
            detail.ram_totale = memoire.ullTotalPhys;
            detail.ram_utilisee = memoire.ullTotalPhys.saturating_sub(memoire.ullAvailPhys);
            poser(Serie::Ram, Some(detail.ram_utilisee as f64 * 100.0 / detail.ram_totale as f64));
        }
        if let Some((envoi, reception)) = self.reseau.echantillonner() {
            poser(Serie::Envoi, Some(envoi));
            poser(Serie::Reception, Some(reception));
        }
        let mut releve = moniteur_peripheriques::Releve::default();
        if let Some(pdh) = self.pdh.as_ref().filter(|p| p.relever()) {
            let disques_inactif = Pdh::tableau(pdh.disques_inactif);
            let disques_lecture = Pdh::tableau(pdh.disques_lecture);
            let disques_ecriture = Pdh::tableau(pdh.disques_ecriture);
            let gpu_moteurs = Pdh::tableau(pdh.gpu_moteurs);
            let gpu_memoire = Pdh::tableau(pdh.gpu_memoire);
            poser(Serie::Disque, Pdh::total(&disques_inactif).map(|inactif| (100.0 - inactif).clamp(0.0, 100.0)));
            poser(Serie::Lecture, Pdh::total(&disques_lecture));
            poser(Serie::Ecriture, Pdh::total(&disques_ecriture));
            poser(Serie::Gpu, utilisation_gpu(&gpu_moteurs).map(f64::from));
            poser(Serie::GpuMemoire, (!gpu_memoire.is_empty()).then(|| gpu_memoire.iter().map(|(_, v)| v).sum()));
            detail.coeurs = utilisation_des_coeurs(&Pdh::tableau(pdh.coeurs));
            releve = self.peripheriques.relever(
                instant_s,
                &moniteur_peripheriques::Instances {
                    disques_inactif: &disques_inactif,
                    disques_lecture: &disques_lecture,
                    disques_ecriture: &disques_ecriture,
                    gpu_moteurs: &gpu_moteurs,
                    gpu_memoire: &gpu_memoire,
                },
            );
        }
        if moniteur_taches::est_demande(instant_s) {
            detail.taches = self.taches.relever();
        }
        (Seconde { instant_s, valeurs }, detail, releve)
    }
}

// ---------------------------------------------------------------------------
// L'ÉTAT, sur le fil principal
// ---------------------------------------------------------------------------

/// D'où vient une mesure : le système entier (`None`), ou un appareil (sa clé).
type Source = Option<String>;

enum Nouvelle {
    /// Les minutes relues du fichier de la source.
    Historique(Source, Vec<Minute>),
    /// Un appareil vu pour la première fois, avec son historique relu.
    Appareil(Peripherique, Vec<Minute>),
    /// Le relevé du système, et celui de chaque appareil présent.
    Seconde(Seconde, Detail, Vec<(String, Seconde)>),
    Minute(Source, Minute),
}

/// La dernière heure seconde par seconde, et `CONSERVATION_S` de minutes, d'une source.
#[derive(Default)]
pub struct Historique {
    pub secondes: VecDeque<Seconde>,
    pub minutes: Vec<Minute>,
}

impl Historique {
    /// Le dernier relevé.
    pub fn actuel(&self) -> Valeurs {
        self.secondes.back().map_or(VIDE, |s| s.valeurs)
    }

    pub fn dernier_s(&self) -> Option<u32> {
        self.secondes.back().map(|s| s.instant_s)
    }

    /// Les points de `plage`, qui finit à `fin_s` (exclue).
    pub fn points(&self, plage: Plage, fin_s: u32) -> Vec<Point> {
        let (duree_s, nb) = plage.decoupage();
        let pas_s = duree_s / nb as u32;
        let debut_s = fin_s.saturating_sub(duree_s);
        if plage.lit_les_minutes() {
            regrouper(self.minutes.iter().map(|m| (m.instant_s, &m.moyennes, &m.max)), debut_s, pas_s, nb)
        } else {
            regrouper(self.secondes.iter().map(|s| (s.instant_s, &s.valeurs, &s.valeurs)), debut_s, pas_s, nb)
        }
    }

    fn relu(&mut self, minutes: Vec<Minute>) {
        // Les minutes déjà reçues du fil sont plus récentes que le fichier.
        let premiere = self.minutes.first().map(|m| m.instant_s);
        let mut tout: Vec<Minute> = minutes.into_iter().filter(|m| premiere.is_none_or(|p| m.instant_s < p)).collect();
        tout.append(&mut self.minutes);
        self.minutes = tout;
    }

    fn seconde(&mut self, seconde: Seconde) {
        if self.secondes.len() >= SECONDES_GARDEES {
            self.secondes.pop_front();
        }
        self.secondes.push_back(seconde);
    }

    fn minute(&mut self, minute: Minute) {
        self.minutes.push(minute);
        let premier = premier_a_garder(&self.minutes, minute.instant_s);
        self.minutes.drain(..premier);
    }
}

/// Un disque ou une carte graphique, et son historique.
pub struct Appareil {
    pub info: Peripherique,
    pub historique: Historique,
}

/// Un appareil sans relevé depuis ce délai est débranché : sa carte disparaît.
const ABSENT_APRES_S: u32 = 5;

#[derive(Default)]
pub struct Moniteur {
    pub systeme: Historique,
    /// Dans l'ordre de leur découverte — voir `appareils_presents`.
    pub appareils: Vec<Appareil>,
    pub detail: Detail,
    plage: Plage,
    /// Change à chaque nouvelle donnée : la clé de `cef_moniteur::push_state`.
    pub version: u64,
    nouvelles: Option<Receiver<Nouvelle>>,
}

impl Moniteur {
    pub fn plage(&self) -> Plage {
        self.plage
    }

    /// Le dernier relevé du système.
    pub fn actuel(&self) -> Valeurs {
        self.systeme.actuel()
    }

    /// Les points du système sur la plage courante, qui finit à `fin_s` (exclue).
    pub fn points(&self, fin_s: u32) -> Vec<Point> {
        self.systeme.points(self.plage, fin_s)
    }

    /// Les appareils relevés depuis moins de `ABSENT_APRES_S` : les disques
    /// puis les cartes, chacun dans son ordre.
    pub fn appareils_presents(&self, maintenant_s: u32) -> Vec<&Appareil> {
        let mut presents: Vec<&Appareil> = self
            .appareils
            .iter()
            .filter(|a| a.historique.dernier_s().is_some_and(|d| maintenant_s.saturating_sub(d) <= ABSENT_APRES_S))
            .collect();
        presents.sort_by_key(|a| (a.info.sorte, a.info.rang));
        presents
    }

    fn historique_de(&mut self, source: Option<&str>) -> Option<&mut Historique> {
        match source {
            None => Some(&mut self.systeme),
            Some(cle) => self.appareils.iter_mut().find(|a| a.info.cle == cle).map(|a| &mut a.historique),
        }
    }

    fn recevoir(&mut self, nouvelle: Nouvelle) {
        match nouvelle {
            Nouvelle::Historique(source, minutes) => {
                if let Some(h) = self.historique_de(source.as_deref()) {
                    h.relu(minutes);
                }
            }
            Nouvelle::Appareil(info, minutes) => {
                let mut historique = Historique::default();
                historique.relu(minutes);
                self.appareils.push(Appareil { info, historique });
            }
            Nouvelle::Seconde(seconde, detail, appareils) => {
                self.systeme.seconde(seconde);
                self.detail = detail;
                for (cle, seconde) in appareils {
                    if let Some(h) = self.historique_de(Some(&cle)) {
                        h.seconde(seconde);
                    }
                }
            }
            Nouvelle::Minute(source, minute) => {
                if let Some(h) = self.historique_de(source.as_deref()) {
                    h.minute(minute);
                }
            }
        }
        self.version += 1;
    }
}

thread_local! {
    static ETAT: RefCell<Moniteur> = RefCell::new(Moniteur::default());
}

/// Lance le fil de mesure — une fois, au démarrage (voir `app::run`).
pub fn demarrer() {
    let (tx, rx) = std::sync::mpsc::channel();
    match std::thread::Builder::new().name("neuradesk-moniteur".into()).spawn(move || fil_de_mesure(tx)) {
        Ok(_) => ETAT.with(|e| e.borrow_mut().nouvelles = Some(rx)),
        Err(e) => crate::diag!("[moniteur] le fil de mesure n'a pas pu démarrer : {e}"),
    }
}

pub fn lire<R>(f: impl FnOnce(&Moniteur) -> R) -> R {
    ETAT.with(|e| f(&e.borrow()))
}

/// Recueille ce que le fil a mesuré — à chaque frame, sans jamais attendre.
pub fn avancer() {
    ETAT.with(|e| {
        let Ok(mut m) = e.try_borrow_mut() else { return };
        let Some(nouvelles) = m.nouvelles.take() else { return };
        for nouvelle in nouvelles.try_iter() {
            m.recevoir(nouvelle);
        }
        m.nouvelles = Some(nouvelles);
    });
}

pub fn choisir_plage(plage: Plage) {
    ETAT.with(|e| {
        let mut m = e.borrow_mut();
        m.plage = plage;
        m.version += 1;
    });
}

pub fn maintenant_s() -> u32 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// LE FIL DE MESURE
// ---------------------------------------------------------------------------

/// `historique.bin` pour le système, `<sorte>-<empreinte>.bin` pour un appareil.
fn chemin_historique(source: Option<&str>) -> Option<PathBuf> {
    let dossier = config::config_path(DOSSIER)?;
    Some(match source {
        None => dossier.join(FICHIER),
        Some(cle) => dossier.join(moniteur_peripheriques::fichier_de(cle)),
    })
}

/// Relit l'historique et l'élague à `CONSERVATION_S` — en RÉÉCRIVANT le fichier
/// (temporaire puis renommage) seulement s'il y avait à élaguer.
fn relire_historique(source: Option<&str>) -> Vec<Minute> {
    let Some(chemin) = chemin_historique(source) else { return Vec::new() };
    let Ok(octets) = std::fs::read(&chemin) else { return Vec::new() };
    let minutes = decoder_historique(&octets);
    let premier = premier_a_garder(&minutes, maintenant_s());
    let gardees = minutes[premier..].to_vec();
    if premier > 0 || octets.len() != minutes.len() * TAILLE_MINUTE {
        let contenu: Vec<u8> = gardees.iter().flat_map(Minute::encoder).collect();
        let temporaire = chemin.with_extension("bin.tmp");
        let reecrit = std::fs::write(&temporaire, contenu).and_then(|()| std::fs::rename(&temporaire, &chemin));
        if let Err(e) = reecrit {
            let _ = std::fs::remove_file(&temporaire);
            crate::diag!("[moniteur] élagage de l'historique impossible : {e}");
        }
    }
    gardees
}

fn ajouter_a_l_historique(source: Option<&str>, minute: &Minute) {
    let Some(chemin) = chemin_historique(source) else { return };
    if let Some(dossier) = chemin.parent() {
        let _ = std::fs::create_dir_all(dossier);
    }
    let ecrit = std::fs::OpenOptions::new().create(true).append(true).open(&chemin).and_then(|mut f| f.write_all(&minute.encoder()));
    if let Err(e) = ecrit {
        crate::diag!("[moniteur] écriture de l'historique impossible : {e}");
    }
}

/// Efface le fichier d'un appareil qui n'a rien reçu depuis `CONSERVATION_S`
/// — un disque retiré pour de bon : tout ce qu'il contient a expiré.
fn effacer_les_appareils_oublies() {
    let Some(dossier) = config::config_path(DOSSIER) else { return };
    let Ok(entrees) = std::fs::read_dir(&dossier) else { return };
    let limite = SystemTime::now() - Duration::from_secs(u64::from(CONSERVATION_S));
    for entree in entrees.flatten() {
        let nom = entree.file_name();
        let appareil = nom.to_str().is_some_and(|n| n.ends_with(".bin") && n != FICHIER);
        let oublie = entree.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < limite);
        if appareil && oublie {
            let _ = std::fs::remove_file(entree.path());
        }
    }
}

/// Un relevé par seconde, calé sur le début de chaque seconde. S'arrête quand
/// plus personne n'écoute.
fn fil_de_mesure(nouvelles: Sender<Nouvelle>) {
    effacer_les_appareils_oublies();
    if nouvelles.send(Nouvelle::Historique(None, relire_historique(None))).is_err() {
        return;
    }
    let mut capteurs = Capteurs::new();
    let mut cumuls: HashMap<Source, Cumul> = HashMap::new();
    loop {
        let maintenant = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        std::thread::sleep(Duration::from_secs(1) - Duration::from_nanos(u64::from(maintenant.subsec_nanos())));
        let (seconde, detail, releve) = capteurs.mesurer(maintenant_s());
        for appareil in releve.nouveaux {
            let minutes = relire_historique(Some(&appareil.cle));
            if nouvelles.send(Nouvelle::Appareil(appareil, minutes)).is_err() {
                return;
            }
        }
        let sources = std::iter::once((None, &seconde)).chain(releve.secondes.iter().map(|(cle, s)| (Some(cle.clone()), s)));
        for (source, s) in sources {
            if let Some(minute) = cumuls.entry(source.clone()).or_default().ajouter(s) {
                ajouter_a_l_historique(source.as_deref(), &minute);
                if nouvelles.send(Nouvelle::Minute(source, minute)).is_err() {
                    return;
                }
            }
        }
        if nouvelles.send(Nouvelle::Seconde(seconde, detail, releve.secondes)).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valeurs(v: f32) -> Valeurs {
        [v; NB_SERIES]
    }

    #[test]
    fn une_minute_fait_l_aller_retour_par_le_fichier() {
        let mut moyennes = valeurs(12.5);
        moyennes[Serie::Gpu as usize] = f32::NAN;
        let minute = Minute { instant_s: 1_800_000_000, moyennes, max: valeurs(99.0) };
        let relue = Minute::decoder(&minute.encoder()).expect("relue");
        assert_eq!(relue.instant_s, minute.instant_s);
        assert_eq!(relue.max, minute.max);
        assert!(relue.moyennes[Serie::Gpu as usize].is_nan(), "une série absente le reste");
        assert_eq!(relue.moyennes[Serie::Cpu as usize], 12.5);
    }

    #[test]
    fn un_enregistrement_tronque_est_ignore_et_le_desordre_trie() {
        let a = Minute { instant_s: 120, moyennes: valeurs(1.0), max: valeurs(1.0) };
        let b = Minute { instant_s: 60, moyennes: valeurs(2.0), max: valeurs(2.0) };
        let mut octets: Vec<u8> = a.encoder().into_iter().chain(b.encoder()).collect();
        octets.extend_from_slice(&[1, 2, 3]);
        let relues = decoder_historique(&octets);
        assert_eq!(relues.iter().map(|m| m.instant_s).collect::<Vec<_>>(), vec![60, 120]);
    }

    #[test]
    fn l_historique_garde_sept_jours() {
        let maintenant = 10 * 86_400;
        let minutes: Vec<Minute> =
            [0, 3 * 86_400 - 1, 3 * 86_400, 9 * 86_400].iter().map(|&t| Minute { instant_s: t, moyennes: VIDE, max: VIDE }).collect();
        assert_eq!(premier_a_garder(&minutes, maintenant), 2, "la limite elle-même est gardée");
    }

    #[test]
    fn une_minute_se_rend_quand_la_suivante_commence() {
        let mut cumul = Cumul::default();
        let mut cpu = |instant_s: u32, v: f32| {
            let mut valeurs = VIDE;
            valeurs[Serie::Cpu as usize] = v;
            cumul.ajouter(&Seconde { instant_s, valeurs })
        };
        assert_eq!(cpu(120, 10.0), None);
        assert_eq!(cpu(150, 30.0), None);
        let minute = cpu(180, 50.0).expect("la minute 120 est finie");
        assert_eq!(minute.instant_s, 120);
        assert_eq!(minute.moyennes[Serie::Cpu as usize], 20.0);
        assert_eq!(minute.max[Serie::Cpu as usize], 30.0);
        assert!(minute.moyennes[Serie::Ram as usize].is_nan(), "aucune mesure de mémoire : pas de zéro inventé");
    }

    #[test]
    fn une_minute_sans_aucune_mesure_n_est_pas_rendue() {
        let mut cumul = Cumul::default();
        assert_eq!(cumul.ajouter(&Seconde { instant_s: 0, valeurs: VIDE }), None);
        assert_eq!(cumul.ajouter(&Seconde { instant_s: 60, valeurs: VIDE }), None);
    }

    #[test]
    fn le_regroupement_range_chaque_serie_a_part() {
        let mut a = VIDE;
        a[Serie::Cpu as usize] = 10.0;
        let mut b = VIDE;
        b[Serie::Cpu as usize] = 30.0;
        b[Serie::Ram as usize] = 50.0;
        let entrees = [(100, a), (105, b), (115, a), (99, b), (130, b)];
        let points = regrouper(entrees.iter().map(|(t, v)| (*t, v, v)), 100, 10, 3);
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].moyennes[Serie::Cpu as usize], 20.0);
        assert_eq!(points[0].max[Serie::Cpu as usize], 30.0);
        assert_eq!(points[0].moyennes[Serie::Ram as usize], 50.0, "le NaN de `a` n'efface pas la mesure de `b`");
        assert_eq!(points[1].moyennes[Serie::Cpu as usize], 10.0);
        assert!(points[1].moyennes[Serie::Ram as usize].is_nan());
        assert!(points[2].moyennes[Serie::Cpu as usize].is_nan(), "130 est hors de la plage [100, 130[");
    }

    #[test]
    fn chaque_plage_tombe_juste_en_points() {
        for plage in Plage::TOUTES {
            let (duree, nb) = plage.decoupage();
            assert_eq!(duree % nb as u32, 0, "{}", plage.nom());
            assert_eq!(Plage::depuis_nom(plage.nom()), Some(plage));
        }
        assert!(Plage::Semaine.decoupage().0 <= CONSERVATION_S, "la plus longue plage tient dans l'historique");
    }

    #[test]
    fn le_gpu_somme_les_processus_d_un_moteur_et_garde_le_plus_occupe() {
        let instances = vec![
            ("pid_10_luid_0x0_0x1_phys_0_eng_0_engtype_3D".to_string(), 20.0),
            ("pid_11_luid_0x0_0x1_phys_0_eng_0_engtype_3D".to_string(), 25.0),
            ("pid_10_luid_0x0_0x1_phys_0_eng_3_engtype_VideoDecode".to_string(), 30.0),
            ("sans_moteur".to_string(), 99.0),
        ];
        assert_eq!(utilisation_gpu(&instances), Some(45.0));
        assert_eq!(utilisation_gpu(&[]), None, "pas de carte : pas de mesure, pas un zéro");
        assert_eq!(utilisation_gpu(&[("pid_1_luid_a".to_string(), 180.0)]), Some(100.0), "borné");
    }

    #[test]
    fn les_coeurs_sont_dans_l_ordre_numerique_sans_le_total() {
        let instances: Vec<(String, f64)> =
            [("10", 5.0), ("_Total", 50.0), ("2", 2.0), ("0", 0.0), ("0,_Total", 1.0)].iter().map(|(n, v)| (n.to_string(), *v)).collect();
        assert_eq!(utilisation_des_coeurs(&instances), vec![0.0, 2.0, 5.0]);
    }

    /// ⭐ **SUR LA MACHINE** : chaque compteur PDH s'ouvre par son nom ANGLAIS,
    /// quelle que soit la langue de Windows — c'est ce qui rend l'applet
    /// portable. `--ignored` : il lit le système réel.
    #[test]
    #[ignore]
    fn sonde_les_capteurs_de_la_machine() {
        let mut capteurs = Capteurs::new();
        std::thread::sleep(Duration::from_millis(1100));
        let (seconde, detail, releve) = capteurs.mesurer(maintenant_s());
        for serie in Serie::TOUTES {
            println!("{:>10} : {}", serie.nom(), seconde.valeurs[serie as usize]);
        }
        println!("cœurs : {:?}", detail.coeurs);
        for p in &releve.nouveaux {
            println!("appareil : {p:?}");
        }
        for (cle, s) in &releve.secondes {
            println!("{cle} : {:?}", s.valeurs);
        }
        assert!(seconde.valeurs[Serie::Cpu as usize].is_finite());
        assert!(seconde.valeurs[Serie::Disque as usize].is_finite(), "un compteur PDH anglais doit s'ouvrir sur un Windows français");
        assert!(!detail.coeurs.is_empty());
        assert!(releve.nouveaux.iter().any(|p| p.sorte == moniteur_peripheriques::Sorte::Disque), "au moins un disque");
    }

    fn appareil(cle: &str, sorte: moniteur_peripheriques::Sorte, rang: u32) -> Peripherique {
        Peripherique { cle: cle.into(), sorte, rang, nom: cle.into(), precision: String::new(), memoire_totale: 0 }
    }

    #[test]
    fn un_appareil_recoit_ses_secondes_et_disparait_debranche() {
        use moniteur_peripheriques::Sorte;
        let mut m = Moniteur::default();
        m.recevoir(Nouvelle::Appareil(appareil("gpu:A", Sorte::Gpu, 0), Vec::new()));
        m.recevoir(Nouvelle::Appareil(appareil("disque:B", Sorte::Disque, 1), Vec::new()));
        m.recevoir(Nouvelle::Appareil(appareil("disque:C", Sorte::Disque, 0), Vec::new()));
        let seconde = |v: f32| Seconde { instant_s: 1000, valeurs: valeurs(v) };
        m.recevoir(Nouvelle::Seconde(
            seconde(1.0),
            Detail::default(),
            vec![("gpu:A".into(), seconde(2.0)), ("disque:B".into(), seconde(3.0)), ("disque:C".into(), seconde(4.0))],
        ));
        let cles: Vec<_> = m.appareils_presents(1000).iter().map(|a| a.info.cle.as_str()).collect();
        assert_eq!(cles, ["disque:C", "disque:B", "gpu:A"], "les disques par numéro, puis les cartes");
        assert_eq!(m.appareils[1].historique.actuel()[0], 3.0, "chaque appareil reçoit SA seconde");
        assert_eq!(m.actuel()[0], 1.0, "le système garde la sienne");
        assert!(m.appareils_presents(1000 + ABSENT_APRES_S + 1).is_empty(), "sans relevé, un appareil est débranché");
    }

    #[test]
    fn une_minute_d_appareil_va_dans_son_historique() {
        let mut m = Moniteur::default();
        m.recevoir(Nouvelle::Appareil(appareil("disque:B", moniteur_peripheriques::Sorte::Disque, 0), Vec::new()));
        let minute = Minute { instant_s: 60, moyennes: valeurs(5.0), max: valeurs(9.0) };
        m.recevoir(Nouvelle::Minute(Some("disque:B".into()), minute));
        assert_eq!(m.appareils[0].historique.minutes, [minute]);
        assert!(m.systeme.minutes.is_empty(), "rien dans l'historique du système");
    }
}
