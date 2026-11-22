//! La TEMPÉRATURE et la PUISSANCE du processeur — DEMANDE (2026-11-14) :
//! « indique la température du cpu », puis le choix B : une invite UAC par
//! session, RIEN d'installé à demeure.
//!
//! ⛔ **WINDOWS NE LES PUBLIE PAS** (mesuré sur un Ryzen 7 9800X3D : la seule
//! zone thermique ACPI lit 290 K ; la puissance « ASIC » de la Radeon
//! intégrée ne suit pas la charge du processeur — voir
//! `moniteur_systeme::frequences_du_processeur`). Il faut lire le processeur
//! lui-même, ce que seul un pilote noyau sait faire : **PawnIO** (GPL-2, celui
//! de LibreHardwareMonitor, installé à part), avec ses modules signés
//! `AMDFamily17` et `IntelMSR` (`vendor/pawnio/`, LGPL-2.1).
//!
//! ⛔ **PawnIO NE S'OUVRE QU'EN ADMINISTRATEUR** (`pawnio_open` rend
//! `0x80070005` à un processus non élevé, mesuré) et NeuraDesk ne l'est
//! jamais. Donc :
//! 1. `neuradesk-capteurs.exe` (`src/capteurs_cpu_service.rs`), SEUL élevé,
//!    lancé par l'invite UAC (`demander`) à la première ouverture du moniteur
//!    de la session ; il lit le processeur chaque seconde, PUBLIE dans une
//!    section de mémoire partagée (`SECTION`, en lecture seule pour
//!    l'utilisateur) et meurt avec NeuraDesk (il attend son processus) ;
//! 2. le `Lecteur` ci-dessous, côté NeuraDesk, qui ne fait que LIRE.
//!
//! Un refus de l'invite n'est pas reposé dans la session : le panneau
//! propose alors un bouton (`demander(true)`).
//!
//! Ce fichier est inclus par les DEUX exécutables (`#[path]` dans le
//! service) : tout ce qui y est doit compiler sans le reste de NeuraDesk.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, HANDLE};
use windows::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS,
};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

/// La section où le service publie (espace de noms de la SESSION).
pub const SECTION: &str = r"Local\NeuraDesk-capteurs-cpu";
pub const MAGIE: u32 = 0x5043_444E; // « NDCP »
pub const VERSION: u32 = 1;
pub const EXE: &str = "neuradesk-capteurs.exe";
/// Une mesure absente.
pub const ABSENT: i32 = i32::MIN;

/// Ce que le service publie. `compteur` avance à chaque relevé : c'est lui qui
/// dit que le service est VIVANT (une valeur figée = service arrêté).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Publication {
    pub magie: u32,
    pub version: u32,
    pub compteur: u64,
    /// En dixièmes de °C, ou `ABSENT`.
    pub temperature_dixiemes: i32,
    /// En dixièmes de W, ou `ABSENT`.
    pub puissance_dixiemes: i32,
}

// ---------------------------------------------------------------------------
// LE DÉCODAGE — PUR
// ---------------------------------------------------------------------------

/// AMD (familles 17h à 1Ah) : `THM_TCON_CUR_TMP` (SMN `0x59800`) → Tctl en °C.
/// Bits [31:21] par pas de 0,125 °C ; −49 °C quand `RANGE_SEL` (bit 19) ou
/// `TJ_SEL` [17:16] = 3 le signalent (Linux `k10temp`, LibreHardwareMonitor).
pub fn tctl_amd(brut: u32) -> f32 {
    let decale = brut & 0x8_0000 != 0 || brut & 0x3_0000 == 0x3_0000;
    let t = (brut >> 21) as f32 * 0.125;
    if decale {
        t - 49.0
    } else {
        t
    }
}

/// Tctl − Tdie : les seuls modèles qui l'annoncent (table de `k10temp`).
pub fn decalage_tdie_amd(nom: &str) -> f32 {
    if ["1600X", "1700X", "1800X"].iter().any(|m| nom.contains(m)) {
        20.0
    } else if nom.contains("Threadripper 19") || nom.contains("Threadripper 29") {
        27.0
    } else if nom.contains("2700X") {
        10.0
    } else {
        0.0
    }
}

/// Intel : `IA32_PACKAGE_THERM_STATUS` (0x1B1) donne l'écart sous TjMax
/// (bits [22:16]), et `MSR_TEMPERATURE_TARGET` (0x1A2) TjMax (bits [23:16]).
pub fn temperature_intel(paquet: u64, cible: u64) -> f32 {
    let tj_max = match (cible >> 16) & 0xFF {
        0 => 100,
        t => t,
    };
    let sous = (paquet >> 16) & 0x7F;
    tj_max.saturating_sub(sous) as f32
}

/// La puissance moyenne entre deux relevés du compteur d'énergie du boîtier
/// (32 bits, il FAIT LE TOUR) ; l'unité est 1/2^ESU joule, ESU = bits [12:8]
/// de `unite` (`MSR_PWR_UNIT` AMD 0xC0010299, `MSR_RAPL_POWER_UNIT` Intel 0x606).
pub fn puissance_w(avant: u64, apres: u64, unite: u64, secondes: f64) -> Option<f32> {
    if secondes <= 0.0 {
        return None;
    }
    let esu = ((unite >> 8) & 0x1F) as i32;
    let increments = (apres as u32).wrapping_sub(avant as u32) as f64;
    Some((increments / 2f64.powi(esu) / secondes) as f32)
}

/// En dixièmes, ou `ABSENT` (aussi pour une valeur hors de toute réalité).
pub fn en_dixiemes(v: Option<f32>, min: f32, max: f32) -> i32 {
    match v {
        Some(v) if v.is_finite() && (min..=max).contains(&v) => (v * 10.0).round() as i32,
        _ => ABSENT,
    }
}

fn depuis_dixiemes(v: i32) -> Option<f32> {
    (v != ABSENT).then(|| v as f32 / 10.0)
}

/// Le dossier `Program Files` (64 bits) — du système, pas d'une variable
/// d'environnement que l'utilisateur pourrait redéfinir.
pub fn program_files() -> Option<PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_ProgramFiles, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    // SAFETY : chaîne rendue par le système, libérée ci-dessous.
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, None).ok()?;
        let chemin = p.to_string().ok().map(PathBuf::from);
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        chemin
    }
}

/// La bibliothèque de PawnIO, si le pilote est installé.
pub fn pawnio_lib() -> Option<PathBuf> {
    Some(program_files()?.join("PawnIO").join("PawnIOLib.dll")).filter(|p| p.is_file())
}

// ---------------------------------------------------------------------------
// LE LANCEMENT — l'invite UAC, une fois par session
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Lancement {
    Jamais,
    EnCours,
    Lance,
    Refuse,
    Echec,
}

static LANCEMENT: Mutex<Lancement> = Mutex::new(Lancement::Jamais);

fn lancement() -> Lancement {
    *LANCEMENT.lock().unwrap_or_else(|e| e.into_inner())
}

fn poser(l: Lancement) {
    *LANCEMENT.lock().unwrap_or_else(|e| e.into_inner()) = l;
}

/// PURE : faut-il (re)poser l'invite ? Une fois d'office par session ; après
/// un refus ou un échec, seulement sur demande (le bouton du panneau) ; et
/// jamais pendant qu'elle est affichée.
fn faut_il_lancer(etat: Lancement, sur_demande: bool, service_vivant: bool) -> bool {
    match etat {
        Lancement::EnCours => false,
        Lancement::Jamais => true,
        Lancement::Refuse | Lancement::Echec => sur_demande,
        // Lancé mais mort depuis (fermé par le gestionnaire des tâches…).
        Lancement::Lance => sur_demande && !service_vivant,
    }
}

/// Lance le service par l'invite UAC si besoin — sur un fil à part :
/// `ShellExecuteExW` ne rend la main qu'après la réponse à l'invite.
pub fn demander(sur_demande: bool, service_vivant: bool) {
    if pawnio_lib().is_none() {
        return;
    }
    {
        let mut etat = LANCEMENT.lock().unwrap_or_else(|e| e.into_inner());
        if !faut_il_lancer(*etat, sur_demande, service_vivant) {
            return;
        }
        *etat = Lancement::EnCours;
    }
    std::thread::spawn(|| {
        let resultat = lancer_eleve();
        if let Err(e) = &resultat {
            eprintln!("[capteurs-cpu] {e}");
        }
        poser(match resultat {
            Ok(()) => Lancement::Lance,
            Err(Refus) => Lancement::Refuse,
            Err(_) => Lancement::Echec,
        });
    });
}

#[derive(Debug)]
enum ErreurLancement {
    Refus,
    Autre(String),
}
use ErreurLancement::Refus;

impl std::fmt::Display for ErreurLancement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refus => write!(f, "invite administrateur refusée"),
            ErreurLancement::Autre(e) => write!(f, "{e}"),
        }
    }
}

fn lancer_eleve() -> Result<(), ErreurLancement> {
    let exe = std::env::current_exe()
        .map_err(|e| ErreurLancement::Autre(format!("exe : {e}")))?
        .with_file_name(EXE);
    if !exe.is_file() {
        return Err(ErreurLancement::Autre(format!("{} introuvable", exe.display())));
    }
    let fichier = HSTRING::from(exe.as_os_str());
    let parametres = HSTRING::from(format!("--servir {}", std::process::id()));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(fichier.as_ptr()),
        lpParameters: PCWSTR(parametres.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY : structure initialisée, chaînes vivantes jusqu'au retour.
    unsafe { ShellExecuteExW(&mut info) }.map_err(|e| {
        if e.code() == ERROR_CANCELLED.to_hresult() {
            Refus
        } else {
            ErreurLancement::Autre(format!("lancement : {e}"))
        }
    })
}

// ---------------------------------------------------------------------------
// LE LECTEUR — côté NeuraDesk
// ---------------------------------------------------------------------------

/// Où en sont les capteurs du processeur.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Etat {
    /// Le service publie (chaque mesure peut manquer).
    Mesure { temperature_c: Option<f32>, puissance_w: Option<f32> },
    /// L'invite est affichée, ou le service vient d'être lancé.
    Attente,
    /// Invite refusée, échouée, ou service arrêté : le panneau propose un bouton.
    AActiver,
    /// Pas de PawnIO : rien à proposer.
    #[default]
    Indisponible,
}

/// Une publication est « fraîche » si son compteur a bougé depuis moins que ça.
const FRAICHEUR: Duration = Duration::from_secs(5);

struct Vue {
    section: HANDLE,
    adresse: MEMORY_MAPPED_VIEW_ADDRESS,
}

impl Drop for Vue {
    fn drop(&mut self) {
        // SAFETY : vue et poignée ouvertes par `Lecteur::ouvrir`.
        unsafe {
            let _ = UnmapViewOfFile(self.adresse);
            let _ = CloseHandle(self.section);
        }
    }
}

// SAFETY : la vue n'est lue que par le fil qui possède le `Lecteur`.
unsafe impl Send for Vue {}

#[derive(Default)]
pub struct Lecteur {
    vue: Option<Vue>,
    dernier_compteur: u64,
    vu_bouger: Option<Instant>,
}

impl Lecteur {
    fn ouvrir() -> Option<Vue> {
        // SAFETY : nom terminé par zéro ; la vue n'est lue que dans sa taille.
        unsafe {
            let section = OpenFileMappingW(FILE_MAP_READ.0, false, &HSTRING::from(SECTION)).ok()?;
            let adresse = MapViewOfFile(section, FILE_MAP_READ, 0, 0, std::mem::size_of::<Publication>());
            if adresse.Value.is_null() {
                let _ = CloseHandle(section);
                return None;
            }
            Some(Vue { section, adresse })
        }
    }

    /// La dernière publication FRAÎCHE, ou rien.
    fn mesure(&mut self) -> Option<Publication> {
        if self.vue.is_none() {
            self.vue = Self::ouvrir();
        }
        let vue = self.vue.as_ref()?;
        // SAFETY : la vue couvre une `Publication` (taille demandée au
        // `MapViewOfFile`) ; lecture volatile, le service écrit en parallèle.
        let p = unsafe { std::ptr::read_volatile(vue.adresse.Value as *const Publication) };
        if p.magie != MAGIE || p.version != VERSION {
            return None;
        }
        let maintenant = Instant::now();
        if p.compteur != self.dernier_compteur {
            self.dernier_compteur = p.compteur;
            self.vu_bouger = Some(maintenant);
        }
        if self.vu_bouger.is_some_and(|t| maintenant.duration_since(t) < FRAICHEUR) {
            Some(p)
        } else {
            // Figée : le service s'est arrêté ; on relâche la vue pour ouvrir
            // celle du prochain.
            self.vue = None;
            None
        }
    }

    /// Un relevé — appelé chaque seconde, panneau ouvert. Le premier pose
    /// l'invite UAC (une fois par session).
    pub fn lire(&mut self) -> Etat {
        if let Some(p) = self.mesure() {
            return Etat::Mesure {
                temperature_c: depuis_dixiemes(p.temperature_dixiemes),
                puissance_w: depuis_dixiemes(p.puissance_dixiemes),
            };
        }
        if pawnio_lib().is_none() {
            return Etat::Indisponible;
        }
        demander(false, false);
        etat_sans_mesure(lancement(), self.vu_bouger)
    }

}

static INSTALLATION_LANCEE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// L'installation de PawnIO a-t-elle été lancée dans la session ?
pub fn installation_lancee() -> bool {
    INSTALLATION_LANCEE.load(std::sync::atomic::Ordering::Relaxed)
}

/// PURE : la commande winget. ⚠️ **SANS `--accept-package-agreements`** :
/// c'est à l'utilisateur d'accepter la licence de PawnIO, dans la fenêtre.
pub const ARGUMENTS_WINGET: &str = "install --id namazso.PawnIO --exact";

/// Le bouton « Installer PawnIO » du panneau : winget dans une fenêtre
/// VISIBLE (l'utilisateur y voit la licence et la progression ; l'installateur
/// pose lui-même son invite administrateur). Une fois le pilote là, la
/// lecture suivante le voit et pose l'invite de `neuradesk-capteurs.exe`.
/// `Err` : winget est absent — le panneau ouvre alors l'aide.
pub fn installer_pawnio() -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let parametres = HSTRING::from(ARGUMENTS_WINGET);
    // SAFETY : chaînes vivantes pendant l'appel.
    let r = unsafe { ShellExecuteW(None, w!("open"), w!("winget.exe"), &parametres, PCWSTR::null(), SW_SHOWNORMAL) };
    // `ShellExecuteW` rend une valeur > 32 en cas de succès.
    if r.0 as usize > 32 {
        INSTALLATION_LANCEE.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    } else {
        Err(format!("winget introuvable ({})", r.0 as usize))
    }
}

/// L'invite UAC du service est-elle à l'écran ? (`ShellExecuteExW` ne rend la
/// main qu'après la réponse : `EnCours` couvre exactement l'invite.) Le
/// Moniteur s'en sert pour ne pas se cacher pendant qu'elle a le focus.
pub fn invite_en_cours() -> bool {
    lancement() == Lancement::EnCours
}

/// Le bouton du panneau — affiché seulement sans mesure fraîche, donc sans
/// service vivant (un service en double sortirait aussitôt : verrou nommé).
pub fn activer() {
    demander(true, false);
}

/// PURE : ce que montre le panneau sans mesure fraîche.
fn etat_sans_mesure(l: Lancement, deja_vu: Option<Instant>) -> Etat {
    match l {
        Lancement::Jamais | Lancement::EnCours => Etat::Attente,
        // Lancé et jamais publié : il démarre (quelques centaines de ms).
        // Lancé ET déjà vu publier : il est mort depuis.
        Lancement::Lance if deja_vu.is_none() => Etat::Attente,
        Lancement::Lance | Lancement::Refuse | Lancement::Echec => Etat::AActiver,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tctl_amd_lit_les_bits_hauts_et_retire_49_quand_le_registre_le_dit() {
        // 54,125 °C = 433 pas de 0,125.
        assert_eq!(tctl_amd(433 << 21), 54.125);
        // RANGE_SEL : la même lecture vaut 49 °C de moins.
        assert_eq!(tctl_amd((433 << 21) | 0x8_0000), 5.125);
        // TJ_SEL = 3 aussi ; TJ_SEL = 1 non.
        assert_eq!(tctl_amd((433 << 21) | 0x3_0000), 5.125);
        assert_eq!(tctl_amd((433 << 21) | 0x1_0000), 54.125);
    }

    #[test]
    fn seuls_les_vieux_ryzen_ont_un_decalage_tdie() {
        assert_eq!(decalage_tdie_amd("AMD Ryzen 7 1700X Eight-Core Processor"), 20.0);
        assert_eq!(decalage_tdie_amd("AMD Ryzen 7 2700X Eight-Core Processor"), 10.0);
        assert_eq!(decalage_tdie_amd("AMD Ryzen Threadripper 1950X 16-Core Processor"), 27.0);
        assert_eq!(decalage_tdie_amd("AMD Ryzen 7 9800X3D 8-Core Processor"), 0.0);
    }

    #[test]
    fn intel_est_tjmax_moins_l_ecart_lu() {
        assert_eq!(temperature_intel(38 << 16, 100 << 16), 62.0);
        assert_eq!(temperature_intel(30 << 16, 0), 70.0, "TjMax illisible : 100");
    }

    #[test]
    fn la_puissance_suit_le_compteur_meme_quand_il_fait_le_tour() {
        // ESU 14 : 1/16384 J par pas ; 16384 × 50 pas en 1 s = 50 W.
        let unite = 14 << 8;
        assert_eq!(puissance_w(1_000, 1_000 + 16_384 * 50, unite, 1.0), Some(50.0));
        assert_eq!(puissance_w(u32::MAX as u64 - 16_383, 16_384 * 50 - 16_384, unite, 1.0), Some(50.0));
        assert_eq!(puissance_w(0, 10, unite, 0.0), None);
    }

    #[test]
    fn une_valeur_hors_de_la_realite_n_est_pas_publiee() {
        assert_eq!(en_dixiemes(Some(54.13), -20.0, 150.0), 541);
        assert_eq!(en_dixiemes(Some(400.0), -20.0, 150.0), ABSENT);
        assert_eq!(en_dixiemes(None, -20.0, 150.0), ABSENT);
        assert_eq!(depuis_dixiemes(541), Some(54.1));
        assert_eq!(depuis_dixiemes(ABSENT), None);
    }

    #[test]
    fn l_invite_n_est_posee_d_office_qu_une_fois_par_session() {
        use Lancement::*;
        assert!(faut_il_lancer(Jamais, false, false));
        assert!(!faut_il_lancer(EnCours, true, false), "jamais deux invites à la fois");
        assert!(!faut_il_lancer(Refuse, false, false), "un refus n'est pas reposé d'office");
        assert!(faut_il_lancer(Refuse, true, false), "le bouton la repose");
        assert!(faut_il_lancer(Echec, true, false));
        assert!(!faut_il_lancer(Lance, false, false));
        assert!(!faut_il_lancer(Lance, true, true), "service vivant : rien à relancer");
        assert!(faut_il_lancer(Lance, true, false), "service mort : le bouton le relance");
    }

    #[test]
    fn sans_mesure_le_panneau_attend_ou_propose_le_bouton() {
        use Lancement::*;
        assert_eq!(etat_sans_mesure(Jamais, None), Etat::Attente);
        assert_eq!(etat_sans_mesure(EnCours, None), Etat::Attente);
        assert_eq!(etat_sans_mesure(Lance, None), Etat::Attente, "il démarre");
        assert_eq!(etat_sans_mesure(Lance, Some(Instant::now())), Etat::AActiver, "il est mort");
        assert_eq!(etat_sans_mesure(Refuse, None), Etat::AActiver);
        assert_eq!(etat_sans_mesure(Echec, None), Etat::AActiver);
    }

    #[test]
    fn la_publication_a_une_taille_fixe() {
        assert_eq!(std::mem::size_of::<Publication>(), 24);
    }
}
