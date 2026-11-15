//! **LES CARTES GRAPHIQUES EN DÉTAIL** — pour le moniteur système (voir
//! `win32::moniteur_systeme`, qui relève, et `cef_moniteur`, qui montre).
//!
//! DEMANDE EXPLICITE (2026-11-14), après lecture de gputaskman
//! (github.com/shanselman/gputaskman, MIT) : « ajoute les deux pour intel amd et
//! nvidia » — les tâches qui utilisent le plus chaque carte, et ses capteurs
//! (température, fréquences, puissance, ventilateur).
//!
//! - **Les tâches** : les MÊMES compteurs PDH que les courbes, lus par
//!   processus — `\GPU Engine(*)` (instances `pid_<pid>_luid_…_eng_<n>_engtype_…`)
//!   et `\GPU Process Memory(*)\Dedicated Usage` (`pid_<pid>_luid_…_phys_0`).
//!   L'utilisation d'un processus est celle de son moteur le PLUS occupé (comme
//!   le Gestionnaire des tâches) ; puis les processus d'un même exécutable
//!   s'additionnent, comme la liste du processeur (`moniteur_taches`).
//! - **Les capteurs, en trois sources**, la plus précise l'emportant :
//!   1. `D3DKMTQueryAdapterInfo` (WDDM 2.4+) — ce que lit le Gestionnaire des
//!      tâches, pour TOUTES les marques : température, ventilateur, fréquence
//!      mémoire, puissance EN POURCENT, et la fréquence du moteur 3D (nœud 0).
//!      Une carte intégrée Intel n'en remplit souvent aucun.
//!   2. NVML (`nvml.dll`, livrée par le pilote NVIDIA) : watts, fréquences,
//!      température, ventilateur en %.
//!   3. ADL (`atiadlxx.dll`, livrée par le pilote AMD), `ADL2_New_QueryPMLogData_Get`
//!      — les numéros de capteurs sont ceux de gputaskman (`adl.py`).
//!   Les cartes sont rapprochées par leur ADRESSE PCI (bus, appareil), que
//!   D3DKMT donne pour un LUID : deux cartes identiques ne se confondent pas.
//! - ⛔ **Relevé SEULEMENT panneau ouvert** (voir `moniteur_taches::est_demande`),
//!   rien n'est historisé.
//! - Les DLL des pilotes sont chargées depuis System32 seulement
//!   (`LOAD_LIBRARY_SEARCH_SYSTEM32`) : jamais depuis le dossier courant.

use std::collections::HashMap;
use std::ffi::c_void;

use windows::core::{s, w, PCSTR};
use windows::Wdk::Graphics::Direct3D::{
    D3DKMTCloseAdapter, D3DKMTOpenAdapterFromLuid, D3DKMTQueryAdapterInfo, D3DKMT_ADAPTERADDRESS, D3DKMT_ADAPTER_PERFDATA,
    D3DKMT_ADAPTER_PERFDATACAPS, D3DKMT_CLOSEADAPTER, D3DKMT_OPENADAPTERFROMLUID, D3DKMT_QUERYADAPTERINFO,
    KMTQAITYPE_ADAPTERADDRESS, KMTQAITYPE_ADAPTERPERFDATA, KMTQAITYPE_ADAPTERPERFDATA_CAPS, KMTQAITYPE_NODEPERFDATA,
    KMTQUERYADAPTERINFOTYPE,
};
use windows::Win32::Foundation::{HMODULE, LUID};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};

use crate::win32::moniteur_peripheriques::luid_de_l_instance;

/// Combien de tâches par carte.
pub const NB_TACHES_GPU: usize = 6;

// ---------------------------------------------------------------------------
// LES TÂCHES — calculs purs
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct TacheGpu {
    /// L'exécutable, sans `.exe`.
    pub nom: String,
    pub instances: u32,
    /// En % de la carte (le moteur le plus occupé de chaque processus).
    pub gpu: f32,
    /// Mémoire dédiée, en octets.
    pub memoire: u64,
}

/// PURE : le pid d'une instance `pid_1234_luid_…`.
pub fn pid_de_l_instance(nom: &str) -> Option<u32> {
    nom.strip_prefix("pid_")?.split('_').next()?.parse().ok()
}

/// PURE : le LUID écrit par les compteurs (`luid_0x<haut>_0x<bas>`) en `LUID`.
pub fn luid_numerique(luid: &str) -> Option<LUID> {
    let mut morceaux = luid.strip_prefix("luid_")?.split('_');
    let haut = u32::from_str_radix(morceaux.next()?.strip_prefix("0x")?, 16).ok()?;
    let bas = u32::from_str_radix(morceaux.next()?.strip_prefix("0x")?, 16).ok()?;
    Some(LUID { LowPart: bas, HighPart: haut as i32 })
}

fn sans_exe(exe: &str) -> String {
    let bas = exe.to_ascii_lowercase();
    if bas.ends_with(".exe") {
        exe[..exe.len() - 4].to_string()
    } else {
        exe.to_string()
    }
}

/// PURE : les tâches de chaque carte (clé : son `luid_…`), regroupées par
/// exécutable, les `n` plus actives d'abord (puis les plus gourmandes en
/// mémoire). Un pid dont le nom est inconnu (processus disparu, ou protégé)
/// s'appelle `pid <n>`.
///
/// ⚠️ **`plafonds` (luid → mémoire dédiée de la carte) ÉCARTE LES VALEURS
/// IMPOSSIBLES** : Windows rapporte parfois pour un processus plus que la
/// carte entière (mesuré : `dwm` à 55 Go sur une carte de 15,6 Go, un autre
/// à 2,6 To). Écartées AVANT la somme par programme, pour ne pas en gâter le
/// total. Une carte sans plafond connu laisse tout passer.
pub fn taches_par_carte(
    moteurs: &[(String, f64)],
    memoires: &[(String, f64)],
    noms: &HashMap<u32, String>,
    plafonds: &HashMap<String, u64>,
    n: usize,
) -> HashMap<String, Vec<TacheGpu>> {
    // (luid, pid) → (moteur le plus occupé, mémoire)
    let mut par_processus: HashMap<(String, u32), (f64, f64)> = HashMap::new();
    for (instance, v) in moteurs {
        let (Some(pid), Some(luid)) = (pid_de_l_instance(instance), luid_de_l_instance(instance)) else { continue };
        let e = par_processus.entry((carte_de(&luid), pid)).or_default();
        e.0 = e.0.max(*v);
    }
    for (instance, v) in memoires {
        let (Some(pid), Some(luid)) = (pid_de_l_instance(instance), luid_de_l_instance(instance)) else { continue };
        let carte = carte_de(&luid);
        let impossible = plafonds.get(&carte).is_some_and(|&p| p > 0 && *v > p as f64);
        if impossible {
            continue;
        }
        par_processus.entry((carte, pid)).or_default().1 += *v;
    }
    let mut par_carte: HashMap<String, HashMap<String, TacheGpu>> = HashMap::new();
    for ((luid, pid), (gpu, memoire)) in par_processus {
        if gpu < 0.05 && memoire < 1.0 {
            continue;
        }
        let nom = noms.get(&pid).map_or_else(|| format!("pid {pid}"), |exe| sans_exe(exe));
        let tache = par_carte
            .entry(luid)
            .or_default()
            .entry(nom.to_lowercase())
            .or_insert_with(|| TacheGpu { nom: nom.clone(), instances: 0, gpu: 0.0, memoire: 0 });
        // Le nom montré ne dépend pas de l'ordre du `HashMap` : le plus petit.
        if nom < tache.nom {
            tache.nom = nom;
        }
        tache.instances += 1;
        tache.gpu = (tache.gpu + gpu as f32).min(100.0);
        tache.memoire += memoire as u64;
    }
    par_carte
        .into_iter()
        .map(|(luid, taches)| {
            let mut liste: Vec<TacheGpu> = taches.into_values().collect();
            liste.sort_by(|a, b| {
                b.gpu.total_cmp(&a.gpu).then_with(|| b.memoire.cmp(&a.memoire)).then_with(|| a.nom.cmp(&b.nom))
            });
            liste.truncate(n);
            (luid, liste)
        })
        .collect()
}

/// `luid_0x…_0x…_phys_0` → `luid_0x…_0x…` : la carte, sans son moteur physique.
fn carte_de(luid_phys: &str) -> String {
    luid_phys.split("_phys").next().unwrap_or(luid_phys).to_string()
}

// ---------------------------------------------------------------------------
// LES CAPTEURS
// ---------------------------------------------------------------------------

/// Ce qu'on sait d'une carte à l'instant ; chaque mesure peut manquer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Capteurs {
    pub temperature_c: Option<f32>,
    pub frequence_mhz: Option<f32>,
    pub frequence_max_mhz: Option<f32>,
    pub frequence_memoire_mhz: Option<f32>,
    pub puissance_w: Option<f32>,
    /// La puissance est celle de la PUCE entière (ADL sans capteur de carte) :
    /// sur une carte intégrée, processeur compris.
    pub puissance_puce: bool,
    /// Du maximum de la carte, en %.
    pub puissance_pct: Option<f32>,
    pub ventilateur_tr_min: Option<u32>,
    pub ventilateur_pct: Option<u32>,
    /// Les sources qui ont répondu (« NVML », « ADL », « Windows »).
    pub sources: Vec<&'static str>,
}

impl Capteurs {
    /// PURE : garde nos valeurs, complète par celles de `autre`.
    pub fn completer(mut self, autre: Capteurs) -> Capteurs {
        self.temperature_c = self.temperature_c.or(autre.temperature_c);
        self.frequence_mhz = self.frequence_mhz.or(autre.frequence_mhz);
        self.frequence_max_mhz = self.frequence_max_mhz.or(autre.frequence_max_mhz);
        self.frequence_memoire_mhz = self.frequence_memoire_mhz.or(autre.frequence_memoire_mhz);
        if self.puissance_w.is_none() {
            self.puissance_w = autre.puissance_w;
            self.puissance_puce = autre.puissance_puce;
        }
        self.puissance_pct = self.puissance_pct.or(autre.puissance_pct);
        self.ventilateur_tr_min = self.ventilateur_tr_min.or(autre.ventilateur_tr_min);
        self.ventilateur_pct = self.ventilateur_pct.or(autre.ventilateur_pct);
        for s in autre.sources {
            if !self.sources.contains(&s) {
                self.sources.push(s);
            }
        }
        self
    }

    pub fn est_vide(&self) -> bool {
        self.temperature_c.is_none()
            && self.frequence_mhz.is_none()
            && self.frequence_memoire_mhz.is_none()
            && self.puissance_w.is_none()
            && self.puissance_pct.is_none()
            && self.ventilateur_tr_min.is_none()
            && self.ventilateur_pct.is_none()
    }
}

/// PURE : ce que D3DKMT rend, en unités lisibles. `caps` dit ce que la carte
/// sait mesurer : un ventilateur à 0 tr/min est réel (arrêt au repos) si la
/// carte en a un, absent sinon.
pub fn depuis_d3dkmt(
    perf: Option<&D3DKMT_ADAPTER_PERFDATA>,
    caps: Option<&D3DKMT_ADAPTER_PERFDATACAPS>,
    noeud: Option<&NoeudPerf>,
) -> Capteurs {
    let mut c = Capteurs::default();
    if let Some(p) = perf {
        let temperature_sue = caps.is_none_or(|k| k.TemperatureMax > 0);
        if temperature_sue && p.Temperature > 0 {
            c.temperature_c = Some(p.Temperature as f32 / 10.0);
        }
        if caps.is_some_and(|k| k.MaxFanRPM > 0) {
            c.ventilateur_tr_min = Some(p.FanRPM);
        }
        if p.MemoryFrequency > 0 {
            c.frequence_memoire_mhz = Some((p.MemoryFrequency as f64 / 1e6) as f32);
        }
        if p.Power > 0 {
            c.puissance_pct = Some(p.Power as f32 / 10.0);
        }
    }
    if let Some(n) = noeud {
        if n.frequency > 0 {
            c.frequence_mhz = Some((n.frequency as f64 / 1e6) as f32);
        }
        if n.max_frequency > 0 {
            c.frequence_max_mhz = Some((n.max_frequency as f64 / 1e6) as f32);
        }
    }
    if !c.est_vide() {
        c.sources.push("Windows");
    }
    c
}

/// `D3DKMT_NODE_PERFDATA` TEL QUE DOCUMENTÉ — avec le `Reserved` final que la
/// déclaration du crate `windows` omet : la taille passée au pilote doit être
/// celle de l'en-tête `d3dkmthk.h`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NoeudPerf {
    pub node_ordinal: u32,
    pub physical_adapter_index: u32,
    pub frequency: u64,
    pub max_frequency: u64,
    pub max_frequency_oc: u64,
    pub voltage: u32,
    pub voltage_max: u32,
    pub voltage_max_oc: u32,
    pub max_transition_latency: u64,
    pub reserved: u64,
}

/// SAFETY de l'appelant : `T` est la structure que `genre` attend.
unsafe fn interroger<T>(adaptateur: u32, genre: KMTQUERYADAPTERINFOTYPE, donnees: &mut T) -> bool {
    let mut requete = D3DKMT_QUERYADAPTERINFO {
        hAdapter: adaptateur,
        Type: genre,
        pPrivateDriverData: std::ptr::from_mut(donnees).cast(),
        PrivateDriverDataSize: std::mem::size_of::<T>() as u32,
    };
    // SAFETY : tampon de la taille annoncée, vivant pendant l'appel.
    unsafe { D3DKMTQueryAdapterInfo(&mut requete) }.is_ok()
}

/// L'adresse PCI `(bus, appareil)` et les mesures de Windows pour une carte.
fn lire_d3dkmt(luid: LUID) -> (Option<(u32, u32)>, Capteurs) {
    let mut ouvrir = D3DKMT_OPENADAPTERFROMLUID { AdapterLuid: luid, hAdapter: 0 };
    // SAFETY : structure valide ; l'adaptateur ouvert est fermé plus bas.
    if unsafe { D3DKMTOpenAdapterFromLuid(&mut ouvrir) }.is_err() {
        return (None, Capteurs::default());
    }
    let h = ouvrir.hAdapter;
    let mut adresse = D3DKMT_ADAPTERADDRESS::default();
    let mut perf = D3DKMT_ADAPTER_PERFDATA::default();
    let mut caps = D3DKMT_ADAPTER_PERFDATACAPS::default();
    let mut noeud = NoeudPerf::default();
    // SAFETY : chaque structure est celle de son genre de requête.
    let (a, p, k, n) = unsafe {
        (
            interroger(h, KMTQAITYPE_ADAPTERADDRESS, &mut adresse),
            interroger(h, KMTQAITYPE_ADAPTERPERFDATA, &mut perf),
            interroger(h, KMTQAITYPE_ADAPTERPERFDATA_CAPS, &mut caps),
            interroger(h, KMTQAITYPE_NODEPERFDATA, &mut noeud),
        )
    };
    // SAFETY : ouvert ci-dessus.
    let _ = unsafe { D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: h }) };
    let capteurs = depuis_d3dkmt(p.then_some(&perf), k.then_some(&caps), n.then_some(&noeud));
    (a.then_some((adresse.BusNumber, adresse.DeviceNumber)), capteurs)
}

/// Une DLL de pilote, depuis System32 seulement.
fn charger(nom: windows::core::PCWSTR) -> Option<HMODULE> {
    // SAFETY : nom littéral terminé par zéro.
    unsafe { LoadLibraryExW(nom, None, LOAD_LIBRARY_SEARCH_SYSTEM32) }.ok()
}

/// SAFETY de l'appelant : `F` est le type exact de la fonction exportée.
unsafe fn fonction<F: Copy>(module: HMODULE, nom: PCSTR) -> Option<F> {
    // SAFETY : module chargé, nom terminé par zéro.
    let p = unsafe { GetProcAddress(module, nom) }?;
    debug_assert_eq!(std::mem::size_of::<F>(), std::mem::size_of_val(&p));
    // SAFETY : l'appelant garantit la signature.
    Some(unsafe { std::mem::transmute_copy::<_, F>(&p) })
}

// ---- NVML ------------------------------------------------------------------

type NvmlDevice = *mut c_void;

#[repr(C)]
struct NvmlPciInfo {
    bus_id_legacy: [u8; 16],
    domain: u32,
    bus: u32,
    device: u32,
    pci_device_id: u32,
    pci_sub_system_id: u32,
    bus_id: [u8; 32],
}

struct Nvml {
    temperature: unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32,
    puissance: unsafe extern "C" fn(NvmlDevice, *mut u32) -> i32,
    limite: Option<unsafe extern "C" fn(NvmlDevice, *mut u32) -> i32>,
    horloge: unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32,
    horloge_max: Option<unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32>,
    ventilateur: Option<unsafe extern "C" fn(NvmlDevice, *mut u32) -> i32>,
    /// `(bus, appareil)` → la carte.
    cartes: HashMap<(u32, u32), NvmlDevice>,
}

const NVML_TEMPERATURE_GPU: u32 = 0;
const NVML_CLOCK_GRAPHICS: u32 = 0;
const NVML_CLOCK_MEM: u32 = 2;

impl Nvml {
    fn ouvrir() -> Option<Nvml> {
        let m = charger(w!("nvml.dll"))?;
        // SAFETY : signatures de `nvml.h`.
        unsafe {
            let init: unsafe extern "C" fn() -> i32 = fonction(m, s!("nvmlInit_v2"))?;
            let compte: unsafe extern "C" fn(*mut u32) -> i32 = fonction(m, s!("nvmlDeviceGetCount_v2"))?;
            let par_index: unsafe extern "C" fn(u32, *mut NvmlDevice) -> i32 = fonction(m, s!("nvmlDeviceGetHandleByIndex_v2"))?;
            let pci: unsafe extern "C" fn(NvmlDevice, *mut NvmlPciInfo) -> i32 = fonction(m, s!("nvmlDeviceGetPciInfo_v3"))?;
            if init() != 0 {
                crate::diag!("[moniteur] nvmlInit_v2 a échoué");
                return None;
            }
            let mut n = 0u32;
            if compte(&mut n) != 0 {
                return None;
            }
            let mut cartes = HashMap::new();
            for i in 0..n.min(16) {
                let mut d: NvmlDevice = std::ptr::null_mut();
                let mut info: NvmlPciInfo = std::mem::zeroed();
                if par_index(i, &mut d) == 0 && pci(d, &mut info) == 0 {
                    cartes.insert((info.bus, info.device), d);
                }
            }
            crate::diag!("[moniteur] NVML : {} carte(s)", cartes.len());
            Some(Nvml {
                temperature: fonction(m, s!("nvmlDeviceGetTemperature"))?,
                puissance: fonction(m, s!("nvmlDeviceGetPowerUsage"))?,
                limite: fonction(m, s!("nvmlDeviceGetEnforcedPowerLimit")),
                horloge: fonction(m, s!("nvmlDeviceGetClockInfo"))?,
                horloge_max: fonction(m, s!("nvmlDeviceGetMaxClockInfo")),
                ventilateur: fonction(m, s!("nvmlDeviceGetFanSpeed")),
                cartes,
            })
        }
    }

    fn lire(&self, pci: (u32, u32)) -> Capteurs {
        let Some(&d) = self.cartes.get(&pci) else { return Capteurs::default() };
        let mut c = Capteurs::default();
        let mut v = 0u32;
        // SAFETY : carte rendue par NVML, pointeurs de sortie valides.
        unsafe {
            if (self.temperature)(d, NVML_TEMPERATURE_GPU, &mut v) == 0 {
                c.temperature_c = Some(v as f32);
            }
            if (self.horloge)(d, NVML_CLOCK_GRAPHICS, &mut v) == 0 && v > 0 {
                c.frequence_mhz = Some(v as f32);
            }
            if (self.horloge)(d, NVML_CLOCK_MEM, &mut v) == 0 && v > 0 {
                c.frequence_memoire_mhz = Some(v as f32);
            }
            if let Some(f) = self.horloge_max {
                if f(d, NVML_CLOCK_GRAPHICS, &mut v) == 0 && v > 0 {
                    c.frequence_max_mhz = Some(v as f32);
                }
            }
            if (self.puissance)(d, &mut v) == 0 {
                let watts = v as f32 / 1000.0;
                c.puissance_w = Some(watts);
                let mut limite = 0u32;
                if let Some(f) = self.limite {
                    if f(d, &mut limite) == 0 && limite > 0 {
                        c.puissance_pct = Some(watts * 100_000.0 / limite as f32);
                    }
                }
            }
            if let Some(f) = self.ventilateur {
                if f(d, &mut v) == 0 {
                    c.ventilateur_pct = Some(v);
                }
            }
        }
        if !c.est_vide() {
            c.sources.push("NVML");
        }
        c
    }
}

// ---- ADL -------------------------------------------------------------------

#[repr(C)]
struct AdlAdapterInfo {
    size: i32,
    index: i32,
    udid: [u8; 256],
    bus: i32,
    device: i32,
    function: i32,
    vendor: i32,
    name: [u8; 256],
    display: [u8; 256],
    present: i32,
    exists: i32,
    driver: [u8; 256],
    driver_ext: [u8; 256],
    pnp: [u8; 256],
    os_index: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AdlCapteur {
    supporte: i32,
    valeur: i32,
}

#[repr(C)]
struct AdlPmLog {
    size: i32,
    capteurs: [AdlCapteur; 256],
}

/// Les numéros `ADLSensorType` (display-library, repris de gputaskman `adl.py`).
const ADL_HORLOGE_GFX: usize = 1;
const ADL_HORLOGE_MEMOIRE: usize = 2;
const ADL_TEMPERATURE: usize = 8;
const ADL_VENTILATEUR_PCT: usize = 15;
const ADL_PUISSANCE_PUCE: usize = 23;
const ADL_PUISSANCE_CARTE: usize = 73;

/// ADL alloue par ce rappel ; ce qu'il rend ici n'est jamais libéré (il ne
/// sert qu'aux listes qu'ADL construit lui-même, pas aux nôtres).
extern "system" fn adl_malloc(taille: i32) -> *mut c_void {
    let taille = taille.max(1) as usize;
    let mut v = vec![0u8; taille].into_boxed_slice();
    let p = v.as_mut_ptr().cast();
    std::mem::forget(v);
    p
}

struct Adl {
    contexte: *mut c_void,
    pmlog: unsafe extern "C" fn(*mut c_void, i32, *mut AdlPmLog) -> i32,
    /// `(bus, appareil)` → l'index ADL.
    cartes: HashMap<(u32, u32), i32>,
}

impl Adl {
    fn ouvrir() -> Option<Adl> {
        let m = charger(w!("atiadlxx.dll"))?;
        // SAFETY : signatures d'`adl_sdk.h`.
        unsafe {
            let creer: unsafe extern "C" fn(extern "system" fn(i32) -> *mut c_void, i32, *mut *mut c_void) -> i32 =
                fonction(m, s!("ADL2_Main_Control_Create"))?;
            let nombre: unsafe extern "C" fn(*mut c_void, *mut i32) -> i32 = fonction(m, s!("ADL2_Adapter_NumberOfAdapters_Get"))?;
            let infos: unsafe extern "C" fn(*mut c_void, *mut AdlAdapterInfo, i32) -> i32 =
                fonction(m, s!("ADL2_Adapter_AdapterInfo_Get"))?;
            let pmlog = fonction(m, s!("ADL2_New_QueryPMLogData_Get"))?;
            let mut contexte: *mut c_void = std::ptr::null_mut();
            if creer(adl_malloc, 1, &mut contexte) != 0 {
                crate::diag!("[moniteur] ADL2_Main_Control_Create a échoué");
                return None;
            }
            let mut n = 0i32;
            if nombre(contexte, &mut n) != 0 || n <= 0 || n > 256 {
                return None;
            }
            let mut liste: Vec<AdlAdapterInfo> = (0..n).map(|_| std::mem::zeroed()).collect();
            for e in &mut liste {
                e.size = std::mem::size_of::<AdlAdapterInfo>() as i32;
            }
            let octets = (std::mem::size_of::<AdlAdapterInfo>() * liste.len()) as i32;
            if infos(contexte, liste.as_mut_ptr(), octets) != 0 {
                return None;
            }
            let mut cartes = HashMap::new();
            for e in &liste {
                if e.bus >= 0 && e.device >= 0 {
                    cartes.entry((e.bus as u32, e.device as u32)).or_insert(e.index);
                }
            }
            crate::diag!("[moniteur] ADL : {} carte(s)", cartes.len());
            Some(Adl { contexte, pmlog, cartes })
        }
    }

    fn lire(&self, pci: (u32, u32)) -> Capteurs {
        let Some(&index) = self.cartes.get(&pci) else { return Capteurs::default() };
        // SAFETY : zéro est valide pour cette structure d'entiers.
        let mut journal: AdlPmLog = unsafe { std::mem::zeroed() };
        journal.size = std::mem::size_of::<AdlPmLog>() as i32;
        // SAFETY : contexte ouvert, structure de la taille annoncée.
        if unsafe { (self.pmlog)(self.contexte, index, &mut journal) } != 0 {
            return Capteurs::default();
        }
        let capteur = |i: usize| {
            let c = journal.capteurs[i];
            (c.supporte != 0).then_some(c.valeur)
        };
        let mut c = Capteurs {
            temperature_c: capteur(ADL_TEMPERATURE).map(|v| v as f32),
            frequence_mhz: capteur(ADL_HORLOGE_GFX).filter(|&v| v > 0).map(|v| v as f32),
            frequence_memoire_mhz: capteur(ADL_HORLOGE_MEMOIRE).filter(|&v| v > 0).map(|v| v as f32),
            puissance_w: capteur(ADL_PUISSANCE_CARTE).or(capteur(ADL_PUISSANCE_PUCE)).map(|v| v as f32),
            puissance_puce: capteur(ADL_PUISSANCE_CARTE).is_none() && capteur(ADL_PUISSANCE_PUCE).is_some(),
            ventilateur_pct: capteur(ADL_VENTILATEUR_PCT).map(|v| v.max(0) as u32),
            ..Capteurs::default()
        };
        if !c.est_vide() {
            c.sources.push("ADL");
        }
        c
    }
}

// ---------------------------------------------------------------------------
// LES SONDES — sur le fil de mesure
// ---------------------------------------------------------------------------

/// Les bibliothèques des pilotes, ouvertes au premier besoin (et une seule fois,
/// qu'elles soient là ou non).
#[derive(Default)]
pub struct Sondes {
    ouvertes: bool,
    nvml: Option<Nvml>,
    adl: Option<Adl>,
}

impl Sondes {
    /// Les capteurs de la carte `luid` (écrit comme les compteurs).
    pub fn lire(&mut self, luid: &str) -> Capteurs {
        if !self.ouvertes {
            self.ouvertes = true;
            self.nvml = Nvml::ouvrir();
            self.adl = Adl::ouvrir();
        }
        let Some(luid) = luid_numerique(luid) else { return Capteurs::default() };
        let (pci, windows) = lire_d3dkmt(luid);
        let mut c = Capteurs::default();
        if let Some(pci) = pci {
            if let Some(nvml) = &self.nvml {
                c = c.completer(nvml.lire(pci));
            }
            if let Some(adl) = &self.adl {
                c = c.completer(adl.lire(pci));
            }
        }
        c.completer(windows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i(nom: &str, v: f64) -> (String, f64) {
        (nom.to_string(), v)
    }

    const A: &str = "luid_0x00000000_0x0000d1b6";

    #[test]
    fn le_pid_et_le_luid_se_lisent_dans_le_nom_de_l_instance() {
        assert_eq!(pid_de_l_instance("pid_1234_luid_0x00000000_0x0000D1B6_phys_0_eng_3_engtype_3D"), Some(1234));
        assert_eq!(pid_de_l_instance("luid_0x00000000_0x0000D1B6_phys_0"), None);
        let l = luid_numerique(A).unwrap();
        assert_eq!((l.LowPart, l.HighPart), (0xd1b6, 0));
        let l = luid_numerique("luid_0x00000001_0x80000002").unwrap();
        assert_eq!((l.LowPart, l.HighPart), (0x8000_0002, 1), "le bas est le SECOND nombre");
    }

    #[test]
    fn un_processus_vaut_son_moteur_le_plus_occupe_et_un_programme_la_somme_de_ses_processus() {
        let moteurs = [
            i("pid_10_luid_0x00000000_0x0000D1B6_phys_0_eng_0_engtype_3D", 30.0),
            i("pid_10_luid_0x00000000_0x0000D1B6_phys_0_eng_5_engtype_VideoDecode", 50.0),
            i("pid_11_luid_0x00000000_0x0000D1B6_phys_0_eng_0_engtype_3D", 5.0),
            i("pid_12_luid_0x00000000_0x0000D1B6_phys_0_eng_0_engtype_3D", 2.0),
        ];
        let memoires = [
            i("pid_10_luid_0x00000000_0x0000D1B6_phys_0", 1000.0),
            i("pid_11_luid_0x00000000_0x0000D1B6_phys_0", 500.0),
            i("pid_12_luid_0x00000000_0x0000D1B6_phys_0", 9000.0),
        ];
        let noms = HashMap::from([(10, "chrome.exe".to_string()), (11, "Chrome.EXE".to_string()), (12, "dwm.exe".to_string())]);
        let t = &taches_par_carte(&moteurs, &memoires, &noms, &HashMap::new(), 8)[A];
        assert_eq!(t[0], TacheGpu { nom: "Chrome".into(), instances: 2, gpu: 55.0, memoire: 1500 }, "{t:?}");
        assert_eq!(t[1].nom, "dwm");
    }

    /// Mesuré le 2026-11-14 sur la RTX 5080 (15,6 Go) : `dwm` à 59 059 675 136
    /// octets, un autre processus à 2 837 966 999 552 — Windows le rapporte
    /// tel quel. Une valeur plus grande que la carte n'est pas une mesure.
    #[test]
    fn une_memoire_plus_grande_que_la_carte_est_ecartee_avant_la_somme() {
        let moteurs = [
            i("pid_1_luid_0x00000000_0x0000D1B6_phys_0_eng_0_engtype_3D", 2.0),
            i("pid_2_luid_0x00000000_0x0000D1B6_phys_0_eng_0_engtype_3D", 1.0),
        ];
        let memoires = [
            i("pid_1_luid_0x00000000_0x0000D1B6_phys_0", 59_059_675_136.0),
            i("pid_2_luid_0x00000000_0x0000D1B6_phys_0", 300.0),
        ];
        let noms = HashMap::from([(1, "dwm.exe".to_string()), (2, "dwm.exe".to_string())]);
        let plafonds = HashMap::from([(A.to_string(), 16_000_000_000u64)]);
        let t = &taches_par_carte(&moteurs, &memoires, &noms, &plafonds, 8)[A];
        assert_eq!(t[0], TacheGpu { nom: "dwm".into(), instances: 2, gpu: 3.0, memoire: 300 }, "{t:?}");
        // Sans plafond connu, la valeur passe (rien pour en juger).
        let t = &taches_par_carte(&moteurs, &memoires, &noms, &HashMap::new(), 8)[A];
        assert_eq!(t[0].memoire, 59_059_675_436);
    }

    #[test]
    fn deux_cartes_ne_se_melangent_pas_et_un_pid_inconnu_garde_son_numero() {
        let moteurs = [
            i("pid_7_luid_0x00000000_0x0000D1B6_phys_0_eng_0_engtype_3D", 10.0),
            i("pid_7_luid_0x00000000_0x0000AAAA_phys_0_eng_0_engtype_3D", 20.0),
        ];
        let t = taches_par_carte(&moteurs, &[], &HashMap::new(), &HashMap::new(), 8);
        assert_eq!(t.len(), 2);
        assert_eq!(t[A][0].gpu, 10.0);
        assert_eq!(t["luid_0x00000000_0x0000aaaa"][0], TacheGpu { nom: "pid 7".into(), instances: 1, gpu: 20.0, memoire: 0 });
    }

    #[test]
    fn les_taches_sans_rien_sont_ecartees_et_la_liste_est_bornee() {
        let mut moteurs = vec![i("pid_1_luid_0x00000000_0x0000D1B6_phys_0_eng_0_engtype_3D", 0.0)];
        moteurs.extend((2..20).map(|p| i(&format!("pid_{p}_luid_0x00000000_0x0000D1B6_phys_0_eng_0_engtype_3D"), p as f64)));
        let noms: HashMap<u32, String> = (1..20).map(|p| (p, format!("p{p:02}.exe"))).collect();
        let t = &taches_par_carte(&moteurs, &[], &noms, &HashMap::new(), 3)[A];
        let n: Vec<_> = t.iter().map(|t| t.nom.as_str()).collect();
        assert_eq!(n, ["p19", "p18", "p17"]);
    }

    #[test]
    fn la_source_precise_l_emporte_et_windows_complete() {
        let nvml = Capteurs { temperature_c: Some(61.0), puissance_w: Some(87.5), sources: vec!["NVML"], ..Default::default() };
        let windows = Capteurs {
            temperature_c: Some(60.5),
            ventilateur_tr_min: Some(1200),
            puissance_pct: Some(25.0),
            sources: vec!["Windows"],
            ..Default::default()
        };
        let c = Capteurs::default().completer(nvml).completer(windows);
        assert_eq!(c.temperature_c, Some(61.0));
        assert_eq!(c.puissance_w, Some(87.5));
        assert_eq!(c.ventilateur_tr_min, Some(1200));
        assert_eq!(c.sources, ["NVML", "Windows"]);
    }

    #[test]
    fn windows_en_unites_lisibles_et_ce_que_la_carte_ne_sait_pas_mesurer_manque() {
        let perf = D3DKMT_ADAPTER_PERFDATA { Temperature: 615, FanRPM: 0, Power: 253, MemoryFrequency: 14_000_000_000, ..Default::default() };
        let avec_ventilateur = D3DKMT_ADAPTER_PERFDATACAPS { MaxFanRPM: 3000, TemperatureMax: 900, ..Default::default() };
        let noeud = NoeudPerf { frequency: 2_610_000_000, max_frequency: 2_900_000_000, ..Default::default() };
        let c = depuis_d3dkmt(Some(&perf), Some(&avec_ventilateur), Some(&noeud));
        assert_eq!(c.temperature_c, Some(61.5));
        assert_eq!(c.ventilateur_tr_min, Some(0), "un ventilateur arrêté au repos est une mesure");
        assert_eq!(c.puissance_pct, Some(25.3));
        assert_eq!(c.frequence_memoire_mhz, Some(14_000.0));
        assert_eq!(c.frequence_mhz, Some(2610.0));
        let sans = D3DKMT_ADAPTER_PERFDATACAPS::default();
        let c = depuis_d3dkmt(Some(&perf), Some(&sans), None);
        assert_eq!((c.temperature_c, c.ventilateur_tr_min), (None, None), "intégrée sans capteurs : rien, pas des zéros");
        assert!(depuis_d3dkmt(None, None, None).est_vide());
    }

    #[test]
    fn la_structure_du_noeud_a_la_taille_de_l_en_tete() {
        // 2×4 + 3×8 + 3×4 (+4 de bourrage) + 8 + 8 = 64 octets, comme `d3dkmthk.h`.
        assert_eq!(std::mem::size_of::<NoeudPerf>(), 64);
        assert_eq!(std::mem::size_of::<NvmlPciInfo>(), 68);
    }

    /// Sur la machine : TOUS les capteurs ADL pris en charge, par carte.
    #[test]
    #[ignore = "sonde : vide le journal ADL de chaque carte AMD"]
    fn sonde_tous_les_capteurs_adl() {
        let Some(adl) = Adl::ouvrir() else { return println!("pas d'ADL") };
        for (pci, &index) in &adl.cartes {
            let mut journal: AdlPmLog = unsafe { std::mem::zeroed() };
            journal.size = std::mem::size_of::<AdlPmLog>() as i32;
            let r = unsafe { (adl.pmlog)(adl.contexte, index, &mut journal) };
            let pris: Vec<String> = journal
                .capteurs
                .iter()
                .enumerate()
                .filter(|(_, c)| c.supporte != 0)
                .map(|(i, c)| format!("{i}={}", c.valeur))
                .collect();
            println!("{pci:?} index {index} (code {r}) : {}", pris.join(" "));
        }
    }

    /// Sur la machine : la puissance ADL de la Radeon intégrée suit-elle la
    /// charge du PROCESSEUR ? (tous les cœurs occupés 6 s, la carte au repos)
    #[test]
    #[ignore = "sonde : charge le processeur 6 s"]
    fn sonde_la_puissance_adl_suit_le_processeur() {
        let Some(adl) = Adl::ouvrir() else { return println!("pas d'ADL") };
        let lire = || {
            adl.cartes
                .iter()
                .map(|(pci, &index)| {
                    let mut j: AdlPmLog = unsafe { std::mem::zeroed() };
                    j.size = std::mem::size_of::<AdlPmLog>() as i32;
                    unsafe { (adl.pmlog)(adl.contexte, index, &mut j) };
                    let v: Vec<String> = [17usize, 23, 28, 29, 30, 31, 73]
                        .iter()
                        .filter(|&&i| j.capteurs[i].supporte != 0)
                        .map(|&i| format!("{i}={}", j.capteurs[i].valeur))
                        .collect();
                    format!("{pci:?} {}", v.join(" "))
                })
                .collect::<Vec<_>>()
                .join(" | ")
        };
        println!("repos   : {}", lire());
        let fin = std::time::Instant::now() + std::time::Duration::from_secs(6);
        let n = std::thread::available_parallelism().map_or(8, |n| n.get());
        let fils: Vec<_> = (0..n)
            .map(|_| {
                std::thread::spawn(move || {
                    let mut x = 1u64;
                    while std::time::Instant::now() < fin {
                        for _ in 0..100_000 {
                            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                        }
                    }
                    x
                })
            })
            .collect();
        std::thread::sleep(std::time::Duration::from_secs(4));
        println!("charge  : {}", lire());
        for f in fils {
            let _ = f.join();
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
        println!("après   : {}", lire());
    }

    /// Sur la machine : les capteurs de chaque carte, et leurs tâches.
    #[test]
    #[ignore = "sonde : lit les cartes graphiques de la machine"]
    fn sonde_les_cartes_de_la_machine() {
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
        let mut sondes = Sondes::default();
        // SAFETY : sans précondition.
        let fabrique: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.unwrap();
        for rang in 0.. {
            // SAFETY : fabrique vivante.
            let Ok(carte) = (unsafe { fabrique.EnumAdapters1(rang) }) else { break };
            // SAFETY : interface valide.
            let d = unsafe { carte.GetDesc1() }.unwrap();
            let luid = crate::win32::moniteur_peripheriques::luid_dxgi(d.AdapterLuid.LowPart, d.AdapterLuid.HighPart);
            let nom = String::from_utf16_lossy(&d.Description).trim_end_matches('\0').to_string();
            println!("{nom} ({luid}) : {:?}", sondes.lire(&luid));
        }
    }
}
