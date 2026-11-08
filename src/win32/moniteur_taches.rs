//! **LES TÂCHES QUI CONSOMMENT LE PLUS DE PROCESSEUR** — pour le moniteur
//! système (voir `win32::moniteur_systeme`, qui relève, et `cef_moniteur`).
//!
//! DEMANDE EXPLICITE (2026-10-20) : « tu devais ajouter les tâches qui
//! consomment le plus de cpu ».
//!
//! - **Portable, sans droits administrateur** : un instantané `ToolHelp`, puis
//!   `GetProcessTimes` sur un descripteur `PROCESS_QUERY_LIMITED_INFORMATION`
//!   — ce que Windows accorde sur presque tout processus, y compris d'un autre
//!   utilisateur. Les processus PROTÉGÉS le refusent : ils manquent à la liste
//!   (comme ils manquent au Gestionnaire des tâches sans élévation).
//! - **Regroupées par exécutable** : un navigateur est trente processus ; seul,
//!   chacun pèserait peu, et la liste ne dirait pas que le navigateur mange
//!   20 % de la machine.
//! - **Un pourcentage DE LA MACHINE** (tous cœurs confondus), comme la courbe
//!   « Processeur » : la somme des tâches ne dépasse pas 100 %.
//! - ⛔ **Relevé SEULEMENT panneau ouvert** (voir `demander`) : ouvrir quelques
//!   centaines de processus chaque seconde n'a de sens que si quelqu'un regarde,
//!   et rien de tout cela n'est historisé.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use windows::Win32::Foundation::{CloseHandle, FILETIME};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetActiveProcessorCount, GetProcessTimes, OpenProcess, ALL_PROCESSOR_GROUPS, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Combien de tâches le panneau montre.
pub const NB_TACHES: usize = 8;
/// Le panneau redemande à chaque frame ; passé ce délai sans demande, le fil
/// cesse de relever.
const DEMANDE_VALABLE_S: u32 = 3;

/// L'instant (`moniteur_systeme::maintenant_s`) de la dernière demande du panneau.
static DEMANDE_S: AtomicU32 = AtomicU32::new(0);

/// Le panneau est ouvert : le fil relève les tâches (voir `est_demande`).
pub fn demander(maintenant_s: u32) {
    DEMANDE_S.store(maintenant_s, Ordering::Relaxed);
}

pub fn est_demande(maintenant_s: u32) -> bool {
    maintenant_s.saturating_sub(DEMANDE_S.load(Ordering::Relaxed)) <= DEMANDE_VALABLE_S
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tache {
    /// Le nom de l'exécutable, sans `.exe`.
    pub nom: String,
    /// Combien de processus portent ce nom.
    pub instances: u32,
    /// En % de la machine entière.
    pub cpu: f32,
}

/// Regroupe `(exécutable, % de la machine)` par exécutable — sans tenir compte
/// de la casse —, et garde les `n` plus gourmands, du plus au moins.
pub fn classer(mesures: impl IntoIterator<Item = (String, f32)>, n: usize) -> Vec<Tache> {
    let mut groupes: HashMap<String, Tache> = HashMap::new();
    for (exe, cpu) in mesures {
        let nom = exe.strip_suffix(".exe").or_else(|| exe.strip_suffix(".EXE")).unwrap_or(&exe).to_string();
        let tache = groupes.entry(nom.to_lowercase()).or_insert_with(|| Tache { nom, instances: 0, cpu: 0.0 });
        tache.instances += 1;
        tache.cpu += cpu;
    }
    let mut taches: Vec<Tache> = groupes.into_values().collect();
    // À égalité, l'ordre alphabétique : la liste ne papillote pas entre deux relevés.
    taches.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then_with(|| a.nom.cmp(&b.nom)));
    taches.truncate(n);
    taches
}

fn en_u64(f: FILETIME) -> u64 {
    (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
}

/// `(exécutable, création, temps processeur noyau + utilisateur)` de chaque
/// processus lisible — en centaines de nanosecondes.
fn instantane() -> Vec<(u32, String, u64, u64)> {
    let mut liste = Vec::new();
    // SAFETY : instantané et descripteurs fermés ci-dessous ; `dwSize` est posé.
    unsafe {
        let Ok(instantane) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return liste };
        let mut entree = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut suite = Process32FirstW(instantane, &mut entree).is_ok();
        while suite {
            // Le pid 0 est le « processus inactif » : son temps est l'inactivité.
            if entree.th32ProcessID != 0 {
                if let Ok(processus) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, entree.th32ProcessID) {
                    let (mut creation, mut fin, mut noyau, mut utilisateur) = Default::default();
                    if GetProcessTimes(processus, &mut creation, &mut fin, &mut noyau, &mut utilisateur).is_ok() {
                        let long = entree.szExeFile.iter().position(|&c| c == 0).unwrap_or(entree.szExeFile.len());
                        let exe = String::from_utf16_lossy(&entree.szExeFile[..long]);
                        liste.push((entree.th32ProcessID, exe, en_u64(creation), en_u64(noyau) + en_u64(utilisateur)));
                    }
                    let _ = CloseHandle(processus);
                }
            }
            suite = Process32NextW(instantane, &mut entree).is_ok();
        }
        let _ = CloseHandle(instantane);
    }
    liste
}

/// Garde le relevé précédent : un taux est une DIFFÉRENCE entre deux relevés.
#[derive(Default)]
pub struct Releveur {
    /// pid → (création, temps processeur). La création distingue un pid réutilisé.
    precedents: HashMap<u32, (u64, u64)>,
    quand: Option<Instant>,
}

impl Releveur {
    /// Les tâches les plus gourmandes depuis le relevé précédent — vide au
    /// premier relevé, ou après une pause (le taux couvrirait toute la pause).
    pub fn relever(&mut self) -> Vec<Tache> {
        let maintenant = Instant::now();
        let ecoule = self.quand.map(|q| maintenant.duration_since(q));
        let liste = instantane();
        let precedents = std::mem::take(&mut self.precedents);
        self.precedents = liste.iter().map(|(pid, _, creation, temps)| (*pid, (*creation, *temps))).collect();
        self.quand = Some(maintenant);
        let Some(ecoule) = ecoule.filter(|e| e.as_secs() <= u64::from(DEMANDE_VALABLE_S)) else { return Vec::new() };
        // SAFETY : sans argument pointeur.
        let coeurs = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) }.max(1);
        let capacite = ecoule.as_nanos() as f64 / 100.0 * f64::from(coeurs);
        if capacite <= 0.0 {
            return Vec::new();
        }
        let mesures = liste.into_iter().filter_map(|(pid, exe, creation, temps)| {
            let &(_, temps_avant) = precedents.get(&pid).filter(|(c, _)| *c == creation)?;
            Some((exe, (temps.saturating_sub(temps_avant) as f64 * 100.0 / capacite).min(100.0) as f32))
        });
        classer(mesures, NB_TACHES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(exe: &str, cpu: f32) -> (String, f32) {
        (exe.to_string(), cpu)
    }

    #[test]
    fn les_processus_d_un_meme_executable_s_additionnent() {
        let taches = classer([m("chrome.exe", 2.0), m("Code.exe", 5.0), m("chrome.exe", 4.5), m("CHROME.EXE", 0.5)], 8);
        assert_eq!(taches[0], Tache { nom: "chrome".into(), instances: 3, cpu: 7.0 }, "trois chrome, casse ignorée : {taches:?}");
        assert_eq!(taches[1], Tache { nom: "Code".into(), instances: 1, cpu: 5.0 });
    }

    #[test]
    fn seules_les_n_plus_gourmandes_restent_dans_l_ordre() {
        let taches = classer((0..20).map(|i| m(&format!("p{i:02}.exe"), i as f32)), 3);
        let noms: Vec<_> = taches.iter().map(|t| t.nom.as_str()).collect();
        assert_eq!(noms, ["p19", "p18", "p17"]);
    }

    #[test]
    fn a_egalite_l_ordre_est_alphabetique() {
        let taches = classer([m("b.exe", 0.0), m("a.exe", 0.0), m("c.exe", 0.0)], 8);
        let noms: Vec<_> = taches.iter().map(|t| t.nom.as_str()).collect();
        assert_eq!(noms, ["a", "b", "c"], "sinon l'ordre du `HashMap` ferait papilloter la liste");
    }

    #[test]
    fn la_demande_expire() {
        demander(1000);
        assert!(est_demande(1000) && est_demande(1000 + DEMANDE_VALABLE_S));
        assert!(!est_demande(1001 + DEMANDE_VALABLE_S), "panneau fermé : le fil cesse de relever");
    }

    /// Sur la machine : deux relevés à une seconde, et la liste ne doit pas être vide.
    #[test]
    #[ignore = "sonde : lit les processus de la machine"]
    fn sonde_les_taches_de_la_machine() {
        let mut releveur = Releveur::default();
        assert!(releveur.relever().is_empty(), "le premier relevé n'a pas de référence");
        std::thread::sleep(std::time::Duration::from_secs(1));
        let debut = Instant::now();
        let taches = releveur.relever();
        println!("relevé en {:?}", debut.elapsed());
        for t in &taches {
            println!("{:>6.1} %  {} ×{}", t.cpu, t.nom, t.instances);
        }
        assert_eq!(taches.len(), NB_TACHES);
    }
}
