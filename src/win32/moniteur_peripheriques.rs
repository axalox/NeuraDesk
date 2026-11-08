//! **UN GRAPHIQUE PAR DISQUE ET PAR CARTE GRAPHIQUE** — pour le moniteur
//! système (voir `win32::moniteur_systeme`, qui relève et historise).
//!
//! DEMANDE EXPLICITE (2026-10-20) : « je voudrais un graphique par disque dur et
//! un graphique par gpu ».
//!
//! - **Les mesures** : les MÊMES compteurs PDH que les totaux, lus par instance
//!   — `\PhysicalDisk(*)` (instances `0 C:`, `1 D: E:`, `_Total`) et
//!   `\GPU Engine(*)` / `\GPU Adapter Memory(*)`, dont les instances portent le
//!   `luid_0x…_0x…` de leur carte.
//! - **Les noms** : le modèle d'un disque par `IOCTL_STORAGE_QUERY_PROPERTY`
//!   sur `\\.\PhysicalDriveN` ouvert SANS DROIT D'ACCÈS (ce qui ne demande pas
//!   l'administrateur), et une carte par DXGI, qui donne le même LUID.
//! - ⛔ **UN LUID CHANGE À CHAQUE DÉMARRAGE**, un numéro de disque quand on en
//!   branche un autre : l'historique d'un appareil est rangé sous une `cle`
//!   tirée de son MODÈLE (et du numéro de série d'un disque), pas du LUID.
//! - Les cartes LOGICIELLES (« Microsoft Basic Render Driver ») et les LUID que
//!   DXGI ne connaît pas sont ignorés.

use std::collections::{HashMap, HashSet};

use windows::core::HSTRING;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows::Win32::System::Ioctl::{
    PropertyStandardQuery, StorageDeviceProperty, IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY,
};
use windows::Win32::System::IO::DeviceIoControl;

use crate::win32::moniteur_systeme::{utilisation_gpu, Seconde, Serie, VIDE};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sorte {
    Disque,
    Gpu,
}

impl Sorte {
    pub fn nom(self) -> &'static str {
        match self {
            Sorte::Disque => "disque",
            Sorte::Gpu => "gpu",
        }
    }

    /// Les séries qu'un appareil de cette sorte remplit.
    pub fn series(self) -> &'static [Serie] {
        match self {
            Sorte::Disque => &[Serie::Disque, Serie::Lecture, Serie::Ecriture],
            Sorte::Gpu => &[Serie::Gpu, Serie::GpuMemoire],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Peripherique {
    /// Stable d'un démarrage à l'autre : nomme le fichier d'historique.
    pub cle: String,
    pub sorte: Sorte,
    /// L'ordre d'affichage : le numéro du disque, l'ordre DXGI d'une carte.
    pub rang: u32,
    pub nom: String,
    /// Les lettres d'un disque (`C: D:`) ; vide pour une carte.
    pub precision: String,
    /// La mémoire dédiée d'une carte, en octets ; 0 si inconnue.
    pub memoire_totale: u64,
}

// ---------------------------------------------------------------------------
// LES CALCULS PURS
// ---------------------------------------------------------------------------

/// PURE : `0 C:` → `(0, "C:")`, `1 D: E:` → `(1, "D: E:")`, `_Total` → `None`.
pub fn disque_de_l_instance(instance: &str) -> Option<(u32, String)> {
    let mut morceaux = instance.trim().splitn(2, ' ');
    let numero = morceaux.next()?.parse().ok()?;
    Some((numero, morceaux.next().unwrap_or("").trim().to_string()))
}

/// PURE : le `luid_0x…_0x…` d'une instance GPU, en minuscules —
/// `pid_12_luid_0x00000000_0x0000D1B6_phys_0_eng_3_engtype_3D` comme
/// `luid_0x00000000_0x0000D1B6_phys_0`.
pub fn luid_de_l_instance(nom: &str) -> Option<String> {
    let reste = &nom[nom.find("luid_")?..];
    let fin = reste.match_indices('_').nth(2).map_or(reste.len(), |(i, _)| i);
    Some(reste[..fin].to_lowercase())
}

/// PURE : le LUID de DXGI écrit comme celui des compteurs.
pub fn luid_dxgi(bas: u32, haut: i32) -> String {
    format!("luid_0x{:08x}_0x{:08x}", haut as u32, bas)
}

/// PURE : les instances GPU rangées par carte.
pub fn par_carte(instances: &[(String, f64)]) -> HashMap<String, Vec<(String, f64)>> {
    let mut cartes: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    for (nom, valeur) in instances {
        if let Some(luid) = luid_de_l_instance(nom) {
            cartes.entry(luid).or_default().push((nom.clone(), *valeur));
        }
    }
    cartes
}

/// PURE : la chaîne ASCII terminée par zéro à `decalage` dans `octets` — vide
/// si le décalage est nul ou hors du tampon.
pub fn chaine_a(octets: &[u8], decalage: u32) -> String {
    let debut = decalage as usize;
    if debut == 0 || debut >= octets.len() {
        return String::new();
    }
    let fin = octets[debut..].iter().position(|&o| o == 0).map_or(octets.len(), |n| debut + n);
    String::from_utf8_lossy(&octets[debut..fin]).trim().to_string()
}

/// PURE : le nom du fichier d'historique d'un appareil — FNV-1a de sa clé : un
/// modèle de disque peut porter `/` ou `:`.
pub fn fichier_de(cle: &str) -> String {
    let sorte = cle.split(':').next().unwrap_or("appareil");
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for o in cle.bytes() {
        h = (h ^ u64::from(o)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{sorte}-{h:016x}.bin")
}

/// PURE : `cle`, ou `cle#2`, `cle#3`… si elle est déjà prise (deux cartes ou
/// deux disques identiques).
fn cle_libre(cle: String, prises: &HashSet<String>) -> String {
    if !prises.contains(&cle) {
        return cle;
    }
    (2..).map(|n| format!("{cle}#{n}")).find(|c| !prises.contains(c)).unwrap_or(cle)
}

// ---------------------------------------------------------------------------
// L'IDENTIFICATION — sur le fil de mesure
// ---------------------------------------------------------------------------

/// `(modèle, numéro de série)` du disque physique `numero`.
fn modele_du_disque(numero: u32) -> Option<(String, String)> {
    let chemin = HSTRING::from(format!(r"\\.\PhysicalDrive{numero}"));
    // SAFETY : chemin terminé par zéro ; accès 0 = interroger sans lire ni écrire.
    let disque = unsafe {
        CreateFileW(&chemin, 0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None)
    }
    .ok()?;
    let requete =
        STORAGE_PROPERTY_QUERY { PropertyId: StorageDeviceProperty, QueryType: PropertyStandardQuery, ..Default::default() };
    // En `u32` : aligné pour `STORAGE_DEVICE_DESCRIPTOR`.
    let mut tampon = vec![0u32; 256];
    let taille = (tampon.len() * 4) as u32;
    let mut rendus = 0u32;
    // SAFETY : tampons valides de la taille annoncée ; descripteur fermé juste après.
    let lu = unsafe {
        DeviceIoControl(
            disque,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(std::ptr::from_ref(&requete).cast()),
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(tampon.as_mut_ptr().cast()),
            taille,
            Some(&mut rendus),
            None,
        )
    }
    .is_ok();
    // SAFETY : ouvert ci-dessus.
    unsafe {
        let _ = CloseHandle(disque);
    }
    if !lu || (rendus as usize) < std::mem::size_of::<STORAGE_DEVICE_DESCRIPTOR>() {
        return None;
    }
    // SAFETY : le tampon est aligné et contient au moins un descripteur.
    let descripteur = unsafe { &*tampon.as_ptr().cast::<STORAGE_DEVICE_DESCRIPTOR>() };
    // SAFETY : `rendus` ≤ la taille du tampon.
    let octets = unsafe { std::slice::from_raw_parts(tampon.as_ptr().cast::<u8>(), rendus as usize) };
    let produit = chaine_a(octets, descripteur.ProductIdOffset);
    let modele = if produit.is_empty() { chaine_a(octets, descripteur.VendorIdOffset) } else { produit };
    Some((modele, chaine_a(octets, descripteur.SerialNumberOffset))).filter(|(m, _)| !m.is_empty())
}

/// Les cartes matérielles : `(luid, rang, nom, mémoire dédiée)`.
fn cartes_dxgi() -> Vec<(String, u32, String, u64)> {
    let mut cartes = Vec::new();
    // SAFETY : sans précondition ; aucune initialisation COM n'est requise par DXGI.
    let Ok(fabrique) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return cartes };
    for rang in 0.. {
        // SAFETY : la fabrique est vivante ; `Err` marque la fin de la liste.
        let Ok(carte) = (unsafe { fabrique.EnumAdapters1(rang) }) else { break };
        // SAFETY : interface valide.
        let Ok(d) = (unsafe { carte.GetDesc1() }) else { continue };
        if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let long = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
        let nom = String::from_utf16_lossy(&d.Description[..long]).trim().to_string();
        cartes.push((luid_dxgi(d.AdapterLuid.LowPart, d.AdapterLuid.HighPart), rang, nom, d.DedicatedVideoMemory as u64));
    }
    cartes
}

// ---------------------------------------------------------------------------
// LE SUIVI — sur le fil de mesure
// ---------------------------------------------------------------------------

/// Ce qu'un relevé apprend des appareils.
#[derive(Default)]
pub struct Releve {
    /// Les appareils vus pour la PREMIÈRE fois.
    pub nouveaux: Vec<Peripherique>,
    /// Une seconde par appareil présent, sous sa clé.
    pub secondes: Vec<(String, Seconde)>,
}

/// Les compteurs PDH par instance, pour `Suivi::relever`.
pub struct Instances<'a> {
    pub disques_inactif: &'a [(String, f64)],
    pub disques_lecture: &'a [(String, f64)],
    pub disques_ecriture: &'a [(String, f64)],
    pub gpu_moteurs: &'a [(String, f64)],
    pub gpu_memoire: &'a [(String, f64)],
}

#[derive(Default)]
pub struct Suivi {
    /// L'instance PDH d'un disque (`0 C:`) → l'appareil. Une autre instance au
    /// même numéro (disque rebranché, lettre changée) le fait réidentifier.
    disques: HashMap<String, Peripherique>,
    /// LUID → la carte.
    cartes: HashMap<String, Peripherique>,
    /// Les LUID que DXGI ne connaît pas : on ne le réinterroge pas à chaque seconde.
    ignores: HashSet<String>,
    cles: HashSet<String>,
}

impl Suivi {
    pub fn relever(&mut self, instant_s: u32, pdh: &Instances) -> Releve {
        let mut releve = Releve::default();
        let valeur = |liste: &[(String, f64)], instance: &str| {
            liste.iter().find(|(nom, _)| nom == instance).map_or(f32::NAN, |(_, v)| *v as f32)
        };
        for (instance, inactif) in pdh.disques_inactif {
            let Some((numero, lettres)) = disque_de_l_instance(instance) else { continue };
            if !self.disques.contains_key(instance) {
                let (modele, serie) = modele_du_disque(numero).unwrap_or_default();
                let cle = match (modele.is_empty(), serie.is_empty()) {
                    (true, _) => format!("disque:{numero}"),
                    (false, true) => format!("disque:{modele}"),
                    (false, false) => format!("disque:{modele}:{serie}"),
                };
                let nom = if modele.is_empty() { format!("Disque {numero}") } else { modele };
                let p = self.nouveau(Peripherique { cle, sorte: Sorte::Disque, rang: numero, nom, precision: lettres, memoire_totale: 0 });
                releve.nouveaux.push(p.clone());
                self.disques.insert(instance.clone(), p);
            }
            let mut valeurs = VIDE;
            valeurs[Serie::Disque as usize] = (100.0 - *inactif as f32).clamp(0.0, 100.0);
            valeurs[Serie::Lecture as usize] = valeur(pdh.disques_lecture, instance);
            valeurs[Serie::Ecriture as usize] = valeur(pdh.disques_ecriture, instance);
            releve.secondes.push((self.disques[instance].cle.clone(), Seconde { instant_s, valeurs }));
        }

        let moteurs = par_carte(pdh.gpu_moteurs);
        let memoires = par_carte(pdh.gpu_memoire);
        let luids: HashSet<&String> = moteurs.keys().chain(memoires.keys()).collect();
        if luids.iter().any(|l| !self.cartes.contains_key(*l) && !self.ignores.contains(*l)) {
            let dxgi = cartes_dxgi();
            for luid in &luids {
                if self.cartes.contains_key(*luid) {
                    continue;
                }
                match dxgi.iter().find(|(l, ..)| l == *luid) {
                    Some((_, rang, nom, memoire)) => {
                        let p = Peripherique {
                            cle: format!("gpu:{nom}"),
                            sorte: Sorte::Gpu,
                            rang: *rang,
                            nom: nom.clone(),
                            precision: String::new(),
                            memoire_totale: *memoire,
                        };
                        let p = self.nouveau(p);
                        releve.nouveaux.push(p.clone());
                        self.cartes.insert((*luid).clone(), p);
                    }
                    None => {
                        self.ignores.insert((*luid).clone());
                    }
                }
            }
        }
        for (luid, carte) in &self.cartes {
            if !luids.contains(luid) {
                continue;
            }
            let mut valeurs = VIDE;
            if let Some(v) = moteurs.get(luid).and_then(|m| utilisation_gpu(m)) {
                valeurs[Serie::Gpu as usize] = v;
            }
            if let Some(m) = memoires.get(luid) {
                valeurs[Serie::GpuMemoire as usize] = m.iter().map(|(_, v)| v).sum::<f64>() as f32;
            }
            releve.secondes.push((carte.cle.clone(), Seconde { instant_s, valeurs }));
        }
        releve
    }

    /// Réserve la clé de `p` (rendue unique) et le rend.
    fn nouveau(&mut self, mut p: Peripherique) -> Peripherique {
        p.cle = cle_libre(std::mem::take(&mut p.cle), &self.cles);
        self.cles.insert(p.cle.clone());
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i(nom: &str, v: f64) -> (String, f64) {
        (nom.to_string(), v)
    }

    #[test]
    fn une_instance_de_disque_donne_son_numero_et_ses_lettres() {
        assert_eq!(disque_de_l_instance("0 C:"), Some((0, "C:".into())));
        assert_eq!(disque_de_l_instance("1 D: E:"), Some((1, "D: E:".into())));
        assert_eq!(disque_de_l_instance("2"), Some((2, String::new())), "un disque sans lettre");
        assert_eq!(disque_de_l_instance("_Total"), None);
    }

    #[test]
    fn le_luid_d_un_moteur_et_d_une_memoire_est_le_meme() {
        let moteur = luid_de_l_instance("pid_12_luid_0x00000000_0x0000D1B6_phys_0_eng_3_engtype_3D");
        let memoire = luid_de_l_instance("luid_0x00000000_0x0000D1B6_phys_0");
        assert_eq!(moteur.as_deref(), Some("luid_0x00000000_0x0000d1b6"));
        assert_eq!(moteur, memoire);
        assert_eq!(luid_dxgi(0xD1B6, 0), "luid_0x00000000_0x0000d1b6", "DXGI : partie HAUTE d'abord, comme PDH");
    }

    #[test]
    fn deux_cartes_se_separent() {
        let cartes = par_carte(&[
            i("pid_1_luid_0x00000000_0x0000AAAA_phys_0_eng_0_engtype_3D", 30.0),
            i("pid_2_luid_0x00000000_0x0000AAAA_phys_0_eng_0_engtype_3D", 20.0),
            i("pid_1_luid_0x00000000_0x0000BBBB_phys_0_eng_0_engtype_3D", 5.0),
        ]);
        assert_eq!(utilisation_gpu(&cartes["luid_0x00000000_0x0000aaaa"]), Some(50.0));
        assert_eq!(utilisation_gpu(&cartes["luid_0x00000000_0x0000bbbb"]), Some(5.0));
    }

    #[test]
    fn une_chaine_du_descripteur_se_lit_jusqu_au_zero() {
        let octets = b"\0\0\0\0Samsung SSD 980   \0S123\0";
        assert_eq!(chaine_a(octets, 4), "Samsung SSD 980");
        assert_eq!(chaine_a(octets, 23), "S123");
        assert_eq!(chaine_a(octets, 0), "", "décalage nul : champ absent");
        assert_eq!(chaine_a(octets, 999), "", "hors du tampon");
    }

    #[test]
    fn le_fichier_d_un_appareil_est_stable_et_sans_caractere_interdit() {
        let f = fichier_de("disque:WDC/WD10:AB:CD");
        assert_eq!(f, fichier_de("disque:WDC/WD10:AB:CD"));
        assert_ne!(f, fichier_de("disque:WDC/WD10:AB:CE"));
        assert!(f.starts_with("disque-") && f.ends_with(".bin") && !f[7..].contains([':', '/']), "{f}");
    }

    #[test]
    fn deux_appareils_identiques_ont_deux_cles() {
        let mut prises = HashSet::new();
        let a = cle_libre("gpu:RTX".into(), &prises);
        prises.insert(a.clone());
        let b = cle_libre("gpu:RTX".into(), &prises);
        assert_eq!((a.as_str(), b.as_str()), ("gpu:RTX", "gpu:RTX#2"));
    }

    /// Sur la machine : les disques et les cartes, nommés.
    #[test]
    #[ignore = "sonde : lit les disques et les cartes de la machine"]
    fn sonde_les_appareils_de_la_machine() {
        for n in 0..4 {
            println!("PhysicalDrive{n} : {:?}", modele_du_disque(n));
        }
        for carte in cartes_dxgi() {
            println!("carte : {carte:?}");
        }
    }
}
