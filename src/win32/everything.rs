//! **CHERCHER DES FICHIERS AVEC EVERYTHING** (voidtools) — DEMANDE EXPLICITE
//! (2026-10-04) : « une applet de recherche qui n'apparaît que si Everything est
//! installé, dans la zone Intégrés », avec les résultats « dans un dash », et
//! Everything démarré s'il ne tourne pas.
//!
//! **LE CANAL EST L'IPC OFFICIEL D'EVERYTHING**, celui que son SDK décrit dans
//! `everything_ipc.h` (téléchargé de voidtools.com, `Everything-SDK.zip`) — sans
//! la DLL du SDK : un `WM_COPYDATA` `EVERYTHING_IPC_COPYDATA_QUERY2W` (18) vers la
//! fenêtre de classe `EVERYTHING_TASKBAR_NOTIFICATION`, et la réponse revient en
//! `WM_COPYDATA` vers NOTRE fenêtre, avec le `dwData` que la requête a choisi.
//!
//! **SUR UN FIL À LUI** : la réponse arrive par la file de messages d'une fenêtre,
//! et l'attendre bloquerait la boucle de rendu. Le fil crée une fenêtre
//! `HWND_MESSAGE`, prend la DERNIÈRE requête en attente (une frappe rapide en
//! empile plusieurs : seule la dernière compte), l'envoie, et pompe sa file
//! jusqu'à la réponse ou au délai.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, MsgWaitForMultipleObjects, PeekMessageW,
    RegisterClassW, SendMessageTimeoutW, TranslateMessage, HWND_MESSAGE, MSG, PM_REMOVE, QS_ALLINPUT,
    SMTO_ABORTIFHUNG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_COPYDATA, WNDCLASSW,
};

/// La classe de la fenêtre d'Everything qui reçoit l'IPC.
const CLASSE_EVERYTHING: PCWSTR = w!("EVERYTHING_TASKBAR_NOTIFICATION");
/// `EVERYTHING_IPC_COPYDATA_QUERY2W` — une requête en UTF-16.
const COPYDATA_QUERY2W: usize = 18;
/// Le `dwData` que nous demandons pour la RÉPONSE — une valeur à nous, qui la
/// distingue de tout autre `WM_COPYDATA` qu'on recevrait.
const COPYDATA_REPONSE: u32 = 0x4E44_5243;

/// `EVERYTHING_IPC_QUERY2_REQUEST_*` — ce qu'on demande pour chaque résultat.
const DEMANDE_NOM: u32 = 0x0000_0001;
const DEMANDE_CHEMIN: u32 = 0x0000_0002;
const DEMANDE_TAILLE: u32 = 0x0000_0010;
const DEMANDE_MODIFIE: u32 = 0x0000_0040;
const DEMANDES: u32 = DEMANDE_NOM | DEMANDE_CHEMIN | DEMANDE_TAILLE | DEMANDE_MODIFIE;

/// `EVERYTHING_IPC_FOLDER` — le drapeau d'élément « dossier ».
const ELEMENT_DOSSIER: u32 = 0x0000_0001;

/// Le plus de résultats rendus à la page — assez pour parcourir, pas assez pour
/// la ralentir (Everything rend le TOTAL à part).
pub const MAX_RESULTATS: u32 = 200;
/// Délai laissé à Everything pour répondre.
const DELAI_REPONSE: Duration = Duration::from_secs(5);
/// Délai laissé à Everything pour apparaître après l'avoir démarré.
const DELAI_DEMARRAGE: Duration = Duration::from_secs(6);

/// Les tris proposés — `EVERYTHING_IPC_SORT_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tri {
    Nom,
    Chemin,
    Taille,
    Date,
}

impl Tri {
    pub fn depuis_nom(nom: &str) -> Option<Tri> {
        match nom {
            "nom" => Some(Tri::Nom),
            "chemin" => Some(Tri::Chemin),
            "taille" => Some(Tri::Taille),
            "date" => Some(Tri::Date),
            _ => None,
        }
    }

    /// Nom croissant, chemin croissant ; taille et date DÉCROISSANTES (le plus
    /// gros, le plus récent d'abord — ce qu'on cherche en triant par là).
    fn code(self) -> u32 {
        match self {
            Tri::Nom => 1,
            Tri::Chemin => 3,
            Tri::Taille => 6,
            Tri::Date => 14,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resultat {
    pub nom: String,
    pub dossier_parent: String,
    pub est_dossier: bool,
    /// En octets ; `None` quand Everything ne la connaît pas (dossiers).
    pub taille: Option<u64>,
    /// Millisecondes depuis 1970 ; `None` si inconnue.
    pub modifie_ms: Option<i64>,
}

impl Resultat {
    pub fn chemin(&self) -> PathBuf {
        Path::new(&self.dossier_parent).join(&self.nom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reponse {
    pub numero: u32,
    pub total: u32,
    pub resultats: Vec<Resultat>,
}

// ---------------------------------------------------------------------------
// INSTALLÉ ?
// ---------------------------------------------------------------------------

/// Les clés de désinstallation d'Everything — 64 bits puis 32 bits (les DEUX
/// existent sur la machine de l'utilisateur : 1.4.1.1030 x64 et 1.4.1.1032 x86).
const CLES: [&str; 2] = [
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Everything",
    r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Everything",
];

/// L'exécutable d'Everything installé, s'il y en a un — relu au plus toutes les
/// `RELECTURE` : les Réglages le demandent À CHAQUE FRAME (voir
/// `cef_settings::push_state`, 60 Hz), et l'applet aussi.
pub fn installe() -> Option<PathBuf> {
    const RELECTURE: Duration = Duration::from_secs(15);
    static CACHE: Mutex<Option<(Instant, Option<PathBuf>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((quand, chemin)) = cache.as_ref() {
        if quand.elapsed() < RELECTURE {
            return chemin.clone();
        }
    }
    let chemin = chercher_l_installation();
    *cache = Some((Instant::now(), chemin.clone()));
    chemin
}

fn chercher_l_installation() -> Option<PathBuf> {
    use crate::win32::registry_util::read_string_value;
    CLES.iter()
        .filter_map(|cle| {
            read_string_value(HKEY_LOCAL_MACHINE, cle, "InstallLocation")
                .map(|dossier| PathBuf::from(dossier).join("Everything.exe"))
                .or_else(|| read_string_value(HKEY_LOCAL_MACHINE, cle, "DisplayIcon").map(PathBuf::from))
        })
        .chain([
            PathBuf::from(r"C:\Program Files\Everything\Everything.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Everything\Everything.exe"),
        ])
        .find(|exe| exe.is_file())
}

// ---------------------------------------------------------------------------
// LA RÉPONSE : `EVERYTHING_IPC_LIST2`
// ---------------------------------------------------------------------------

fn lire_u32(octets: &[u8], position: usize) -> Option<u32> {
    octets.get(position..position + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn lire_u64(octets: &[u8], position: usize) -> Option<u64> {
    octets.get(position..position + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8])))
}

/// Un texte de la réponse : `DWORD longueur` (en caractères, sans le zéro), puis
/// le texte UTF-16 ET son zéro. Rend le texte et la position qui le suit.
fn lire_texte(octets: &[u8], position: usize) -> Option<(String, usize)> {
    let longueur = lire_u32(octets, position)? as usize;
    let debut = position + 4;
    let fin = debut.checked_add(longueur.checked_mul(2)?)?;
    let unites: Vec<u16> = octets.get(debut..fin)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Some((String::from_utf16_lossy(&unites), fin + 2))
}

/// Un `FILETIME` (centaines de nanosecondes depuis 1601) en millisecondes depuis
/// 1970 ; `None` pour 0 et pour la valeur « inconnue » (tous les bits à 1).
fn filetime_en_ms(filetime: u64) -> Option<i64> {
    const ECART_1601_1970_MS: i64 = 11_644_473_600_000;
    if filetime == 0 || filetime == u64::MAX {
        return None;
    }
    i64::try_from(filetime / 10_000).ok().map(|ms| ms - ECART_1601_1970_MS)
}

/// Décode une `EVERYTHING_IPC_LIST2` : l'en-tête (`totitems`, `numitems`,
/// `offset`, `request_flags`, `sort_type`), puis `numitems` éléments (`flags`,
/// `data_offset`), et pour chacun ses données À `data_offset`, depuis le DÉBUT de
/// la liste, dans l'ordre du SDK : nom, chemin, taille, date de modification.
///
/// ⚠️ **LES DONNÉES SE LISENT D'APRÈS LES `request_flags` DE LA RÉPONSE**, pas
/// d'après ceux qu'on a demandés : le SDK dit qu'Everything peut en rendre
/// d'autres. Un drapeau inconnu de cette fonction rend la réponse illisible
/// (on ne saurait pas sauter sa donnée) — elle est REFUSÉE plutôt que mal lue.
pub fn analyser(octets: &[u8], numero: u32) -> Result<Reponse, String> {
    let total = lire_u32(octets, 0).ok_or("réponse trop courte")?;
    let nombre = lire_u32(octets, 4).ok_or("réponse trop courte")? as usize;
    let drapeaux = lire_u32(octets, 12).ok_or("réponse trop courte")?;
    if drapeaux & !DEMANDES != 0 {
        return Err(format!("données inattendues dans la réponse (drapeaux {drapeaux:#x})"));
    }
    let mut resultats = Vec::with_capacity(nombre.min(MAX_RESULTATS as usize));
    for i in 0..nombre {
        let element = 20 + i * 8;
        let illisible = || format!("élément {i} illisible");
        let drapeaux_element = lire_u32(octets, element).ok_or_else(illisible)?;
        let mut position = lire_u32(octets, element + 4).ok_or_else(illisible)? as usize;
        let mut nom = String::new();
        let mut dossier_parent = String::new();
        let mut taille = None;
        let mut modifie_ms = None;
        if drapeaux & DEMANDE_NOM != 0 {
            (nom, position) = lire_texte(octets, position).ok_or_else(illisible)?;
        }
        if drapeaux & DEMANDE_CHEMIN != 0 {
            (dossier_parent, position) = lire_texte(octets, position).ok_or_else(illisible)?;
        }
        if drapeaux & DEMANDE_TAILLE != 0 {
            let brute = lire_u64(octets, position).ok_or_else(illisible)?;
            taille = (brute != u64::MAX).then_some(brute);
            position += 8;
        }
        if drapeaux & DEMANDE_MODIFIE != 0 {
            modifie_ms = filetime_en_ms(lire_u64(octets, position).ok_or_else(illisible)?);
        }
        let est_dossier = drapeaux_element & ELEMENT_DOSSIER != 0;
        resultats.push(Resultat {
            nom,
            dossier_parent,
            est_dossier,
            taille: taille.filter(|_| !est_dossier),
            modifie_ms,
        });
    }
    Ok(Reponse { numero, total, resultats })
}

/// La requête `EVERYTHING_IPC_QUERY2` + la recherche en UTF-16 terminée par un zéro.
fn requete(fenetre_reponse: HWND, recherche: &str, tri: Tri) -> Vec<u8> {
    let mut octets = Vec::new();
    for champ in [
        // Le SDK le dit : 32 bits suffisent pour une fenêtre, même en 64 bits.
        fenetre_reponse.0 as usize as u32,
        COPYDATA_REPONSE,
        0, // drapeaux de recherche : ni casse, ni mot entier, ni chemin
        0, // à partir du premier résultat
        MAX_RESULTATS,
        DEMANDES,
        tri.code(),
    ] {
        octets.extend_from_slice(&champ.to_le_bytes());
    }
    for unite in recherche.encode_utf16().chain(std::iter::once(0)) {
        octets.extend_from_slice(&unite.to_le_bytes());
    }
    octets
}

// ---------------------------------------------------------------------------
// LE FIL
// ---------------------------------------------------------------------------

struct Demande {
    numero: u32,
    recherche: String,
    tri: Tri,
}

struct Canal {
    demandes: Sender<Demande>,
    reponses: Receiver<Result<Reponse, (u32, String)>>,
}

thread_local! {
    /// Le canal du fil UI vers le fil Everything — créé à la première recherche.
    static CANAL: RefCell<Option<Canal>> = const { RefCell::new(None) };
    /// La réponse reçue par la fenêtre du fil Everything (son `wndproc` la dépose).
    static RECUE: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// Envoie une recherche ; la réponse viendra par [`prendre_reponses`].
pub fn chercher(numero: u32, recherche: String, tri: Tri) {
    CANAL.with(|c| {
        let mut canal = c.borrow_mut();
        let canal = canal.get_or_insert_with(demarrer_le_fil);
        if canal.demandes.send(Demande { numero, recherche, tri }).is_err() {
            crate::diag!("[recherche] le fil Everything est arrêté");
        }
    });
}

/// Les réponses arrivées depuis le dernier appel — `Err((numéro, message))` pour
/// une recherche qui a échoué.
pub fn prendre_reponses() -> Vec<Result<Reponse, (u32, String)>> {
    CANAL.with(|c| c.borrow().as_ref().map(|canal| canal.reponses.try_iter().collect()).unwrap_or_default())
}

fn demarrer_le_fil() -> Canal {
    let (tx_demandes, rx_demandes) = mpsc::channel::<Demande>();
    let (tx_reponses, rx_reponses) = mpsc::channel();
    std::thread::spawn(move || {
        let Some(fenetre) = creer_fenetre() else {
            crate::diag!("[recherche] fenêtre de réponse impossible à créer");
            return;
        };
        while let Ok(premiere) = rx_demandes.recv() {
            // Une frappe rapide empile des recherches : seule la DERNIÈRE compte.
            let demande = rx_demandes.try_iter().last().unwrap_or(premiere);
            let reponse = executer(fenetre, &demande).map_err(|message| (demande.numero, message));
            if tx_reponses.send(reponse).is_err() {
                return;
            }
        }
    });
    Canal { demandes: tx_demandes, reponses: rx_reponses }
}

fn creer_fenetre() -> Option<HWND> {
    static CLASSE: OnceLock<bool> = OnceLock::new();
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let classe = w!("NeuraDeskEverythingReponse");
        let enregistree = *CLASSE.get_or_init(|| {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                lpszClassName: classe,
                ..Default::default()
            };
            RegisterClassW(&wc) != 0
        });
        if !enregistree {
            return None;
        }
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            classe,
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance.into()),
            None,
        )
        .ok()
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_COPYDATA {
        // SAFETY : `lparam` d'un `WM_COPYDATA` pointe sur un `COPYDATASTRUCT`
        // valide le temps du traitement ; on COPIE les octets avant de rendre.
        if let Some(copie) = unsafe { (lparam.0 as *const COPYDATASTRUCT).as_ref() } {
            if copie.dwData == COPYDATA_REPONSE as usize && !copie.lpData.is_null() {
                let octets = unsafe { std::slice::from_raw_parts(copie.lpData as *const u8, copie.cbData as usize) };
                RECUE.with(|r| *r.borrow_mut() = Some(octets.to_vec()));
                return LRESULT(1);
            }
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// La fenêtre d'Everything — en le démarrant (`-startup`, en arrière-plan) s'il
/// est installé mais ne tourne pas, puis en l'attendant.
fn fenetre_everything() -> Result<HWND, String> {
    let trouver = || unsafe { FindWindowW(CLASSE_EVERYTHING, PCWSTR::null()).ok() };
    if let Some(hwnd) = trouver() {
        return Ok(hwnd);
    }
    let exe = installe().ok_or("Everything n'est pas installé.")?;
    crate::diag!("[recherche] Everything ne tourne pas : démarrage de {}", exe.display());
    // `ShellExecuteW` et non `Command::spawn` : un `Everything.exe` qui demande
    // l'élévation fait échouer `CreateProcess` en 740 (voir la mémoire « lancer un
    // exe qui demande l'élévation ») ; `ShellExecuteW` présente l'invite UAC.
    {
        use std::os::windows::ffi::OsStrExt;
        let exe_large: Vec<u16> = exe.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let resultat = unsafe {
            windows::Win32::UI::Shell::ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(exe_large.as_ptr()),
                w!("-startup"),
                PCWSTR::null(),
                windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE,
            )
        };
        // Un code ≤ 32 est un échec (documentation de `ShellExecuteW`).
        if resultat.0 as isize <= 32 {
            return Err(format!("Everything n'a pas pu être démarré (code {}).", resultat.0 as isize));
        }
    }
    let debut = Instant::now();
    while debut.elapsed() < DELAI_DEMARRAGE {
        std::thread::sleep(Duration::from_millis(150));
        if let Some(hwnd) = trouver() {
            return Ok(hwnd);
        }
    }
    Err("Everything ne répond pas.".to_string())
}

fn executer(fenetre: HWND, demande: &Demande) -> Result<Reponse, String> {
    let everything = fenetre_everything()?;
    let octets = requete(fenetre, &demande.recherche, demande.tri);
    let copie = COPYDATASTRUCT {
        dwData: COPYDATA_QUERY2W,
        cbData: octets.len() as u32,
        lpData: octets.as_ptr() as *mut _,
    };
    RECUE.with(|r| r.borrow_mut().take());
    let mut accepte = 0usize;
    // SAFETY : `copie` et `octets` vivent jusqu'au retour de l'envoi SYNCHRONE.
    let envoye = unsafe {
        SendMessageTimeoutW(
            everything,
            WM_COPYDATA,
            WPARAM(fenetre.0 as usize),
            LPARAM(&copie as *const COPYDATASTRUCT as isize),
            SMTO_ABORTIFHUNG,
            DELAI_REPONSE.as_millis() as u32,
            Some(&mut accepte),
        )
    };
    if envoye.0 == 0 || accepte == 0 {
        return Err("Everything a refusé la recherche (index en cours de chargement ?).".to_string());
    }
    // La réponse arrive par la file de notre fenêtre : on la pompe jusqu'à elle.
    let debut = Instant::now();
    loop {
        if let Some(reponse) = RECUE.with(|r| r.borrow_mut().take()) {
            return analyser(&reponse, demande.numero);
        }
        let reste = DELAI_REPONSE.saturating_sub(debut.elapsed());
        if reste.is_zero() {
            return Err("Everything n'a pas répondu à temps.".to_string());
        }
        unsafe {
            MsgWaitForMultipleObjects(None, false, reste.as_millis() as u32, QS_ALLINPUT);
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// OUVRIR
// ---------------------------------------------------------------------------

/// Ouvre l'emplacement de `chemin` dans l'Explorateur, l'élément SÉLECTIONNÉ.
pub fn ouvrir_l_emplacement(chemin: &Path) {
    use std::os::windows::process::CommandExt;
    // `raw_arg` : `/select,"…"` doit arriver tel quel — l'échappement de
    // `arg` mettrait des guillemets autour de TOUT, virgule comprise, et
    // l'Explorateur ouvrirait « Documents » au lieu du dossier visé.
    let argument = format!("/select,\"{}\"", chemin.display());
    if let Err(err) = std::process::Command::new("explorer.exe").raw_arg(argument).spawn() {
        crate::diag!("[recherche] emplacement impossible à ouvrir — {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texte(octets: &mut Vec<u8>, s: &str) {
        let unites: Vec<u16> = s.encode_utf16().collect();
        octets.extend_from_slice(&(unites.len() as u32).to_le_bytes());
        for u in unites.iter().chain(std::iter::once(&0u16)) {
            octets.extend_from_slice(&u.to_le_bytes());
        }
    }

    /// Une `EVERYTHING_IPC_LIST2` fabriquée selon `everything_ipc.h` : un fichier
    /// et un dossier, données placées APRÈS la table des éléments.
    fn liste_fabriquee() -> Vec<u8> {
        let entete = 20;
        let table = 2 * 8;
        let mut donnees = Vec::new();
        let premier = (entete + table) as u32;
        texte(&mut donnees, "rapport.pdf");
        texte(&mut donnees, r"C:\Users\moi\Documents");
        donnees.extend_from_slice(&1_234_567u64.to_le_bytes());
        // 2026-01-01T00:00:00Z en FILETIME.
        donnees.extend_from_slice(&134_116_992_000_000_000u64.to_le_bytes());
        let second = premier + donnees.len() as u32;
        texte(&mut donnees, "Projets");
        texte(&mut donnees, r"D:\");
        donnees.extend_from_slice(&u64::MAX.to_le_bytes());
        donnees.extend_from_slice(&0u64.to_le_bytes());

        let mut octets = Vec::new();
        for v in [57u32, 2, 0, DEMANDES, 1] {
            octets.extend_from_slice(&v.to_le_bytes());
        }
        for (drapeaux, offset) in [(0u32, premier), (ELEMENT_DOSSIER, second)] {
            octets.extend_from_slice(&drapeaux.to_le_bytes());
            octets.extend_from_slice(&offset.to_le_bytes());
        }
        octets.extend_from_slice(&donnees);
        octets
    }

    #[test]
    fn une_liste_se_decode_selon_le_sdk() {
        let reponse = analyser(&liste_fabriquee(), 7).expect("liste lisible");
        assert_eq!(reponse.numero, 7);
        assert_eq!(reponse.total, 57, "le TOTAL d'Everything, pas le nombre rendu");
        assert_eq!(reponse.resultats.len(), 2);
        let fichier = &reponse.resultats[0];
        assert_eq!(fichier.nom, "rapport.pdf");
        assert_eq!(fichier.chemin(), PathBuf::from(r"C:\Users\moi\Documents\rapport.pdf"));
        assert_eq!(fichier.taille, Some(1_234_567));
        assert_eq!(fichier.modifie_ms, Some(1_767_225_600_000));
        assert!(!fichier.est_dossier);
        let dossier = &reponse.resultats[1];
        assert!(dossier.est_dossier);
        assert_eq!(dossier.taille, None, "un dossier n'a pas de taille affichée");
        assert_eq!(dossier.modifie_ms, None, "FILETIME nul = date inconnue");
    }

    #[test]
    fn une_reponse_tronquee_ou_inattendue_est_refusee_sans_paniquer() {
        let complete = liste_fabriquee();
        for longueur in [0, 3, 19, 30, complete.len() - 3] {
            assert!(analyser(&complete[..longueur], 1).is_err(), "tronquée à {longueur}");
        }
        let mut drapeau_inconnu = complete.clone();
        drapeau_inconnu[12..16].copy_from_slice(&(DEMANDES | 0x20).to_le_bytes());
        assert!(analyser(&drapeau_inconnu, 1).is_err());
    }

    #[test]
    fn la_requete_suit_la_disposition_query2() {
        let octets = requete(HWND(0x1234 as *mut _), "ab", Tri::Date);
        let champ = |i: usize| u32::from_le_bytes(octets[i * 4..i * 4 + 4].try_into().unwrap());
        assert_eq!(champ(0), 0x1234);
        assert_eq!(champ(1), COPYDATA_REPONSE);
        assert_eq!(champ(4), MAX_RESULTATS);
        assert_eq!(champ(5), DEMANDES);
        assert_eq!(champ(6), 14, "date de modification décroissante");
        assert_eq!(&octets[28..], &[b'a', 0, b'b', 0, 0, 0], "UTF-16 terminé par un zéro");
    }

    /// **SONDE SUR L'EVERYTHING RÉEL** (lecture seule : une recherche) — à lancer
    /// à la main : `cargo test --release --bin neuradesk sonde_everything_reel --
    /// --ignored --nocapture`. Elle prouve le canal de bout en bout, ce que les
    /// tests sur une liste fabriquée ne peuvent pas.
    #[test]
    #[ignore]
    fn sonde_everything_reel() {
        println!("installé : {:?}", installe());
        chercher(1, "Everything.exe".to_string(), Tri::Nom);
        let debut = Instant::now();
        loop {
            let reponses = prendre_reponses();
            if let Some(r) = reponses.into_iter().next() {
                match r {
                    Ok(r) => {
                        println!("total {} ; rendus {}", r.total, r.resultats.len());
                        for x in r.resultats.iter().take(5) {
                            println!("  {} | taille {:?} | modifié {:?} | dossier {}", x.chemin().display(), x.taille, x.modifie_ms, x.est_dossier);
                        }
                        assert!(r.total > 0);
                    }
                    Err((n, m)) => panic!("recherche {n} : {m}"),
                }
                break;
            }
            assert!(debut.elapsed() < Duration::from_secs(15), "aucune réponse");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
