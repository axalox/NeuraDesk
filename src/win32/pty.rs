//! Shells locaux sous pseudo-console (ConPTY) — le moteur du panneau Terminal
//! (voir `win32::cef_terminal`). DEMANDE EXPLICITE (2026-09-27) : « que ce
//! soit un dash avec le terminal à l'intérieur ». Le terminal déroulant
//! manipulait une fenêtre de console de l'extérieur ; ici la console n'a pas
//! de fenêtre, ses octets (VT, UTF-8) passent par deux tubes et xterm.js les
//! dessine dans la page. La pseudo-console elle-même vit dans
//! `win32::conpty`, partagée avec le relais administrateur.
//!
//! Une session par onglet de la page, numérotée par la page. Par session :
//! - un fil LECTEUR qui verse la sortie dans la file d'évènements, puis
//!   `Evenement::Fin` quand le tube casse ;
//! - un fil GUETTEUR qui attend la fin du shell et referme alors la
//!   pseudo-console — ⚠️ sans lui le tube de sortie ne casse JAMAIS : conhost
//!   le garde ouvert après la mort du shell, et un `exit` laissait l'onglet
//!   muet au lieu de le fermer.
//!
//! **ONGLETS ADMINISTRATEUR** (`ouvrir_admin`) — DEMANDE EXPLICITE
//! (2026-09-27) : dans le panneau, « sans lancer neuradesk en administrateur ».
//! Le shell tourne sous `neuradesk-admin.exe` (voir `src/relais_admin.rs`),
//! seul élevé par l'UAC, qui le relaie par deux tubes nommés créés ICI.
//! ⚠️ Risque accepté par l'utilisateur : un programme qui s'injecterait dans
//! NeuraDesk pourrait taper dans un onglet administrateur ouvert.
//!
//! Le shell est choisi par un IDENTIFIANT (`Shell`), jamais par une ligne de
//! commande venue de la page.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_CANCELLED, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_SHARE_NONE, OPEN_EXISTING,
    PIPE_ACCESS_INBOUND, PIPE_ACCESS_OUTBOUND,
};
use windows::Win32::System::Console::ResizePseudoConsole;
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_WAIT,
};
use windows::Win32::System::Threading::{GetProcessId, WaitForSingleObject, INFINITE};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

pub use super::conpty::Shell;
use super::conpty::{self, Poignee, Pseudo};

/// `pwsh.exe` trouvé dans le `PATH` ?
pub fn pwsh_installe() -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dossier| dossier.join("pwsh.exe").is_file()))
}

/// Les shells disponibles sur cette machine, le premier étant celui du
/// premier onglet.
pub fn shells() -> Vec<Shell> {
    let mut liste = vec![Shell::PowerShell, Shell::Cmd];
    if pwsh_installe() {
        liste.insert(0, Shell::Pwsh);
    }
    liste
}

pub enum Evenement {
    Sortie { onglet: u32, octets: Vec<u8> },
    /// Le shell est terminé (et toute sa sortie a été versée avant).
    Fin { onglet: u32 },
    /// Un onglet administrateur est relié : la page renvoie sa taille, qui a
    /// pu changer pendant l'invite UAC.
    Pret { onglet: u32 },
    /// L'ouverture d'un onglet administrateur a échoué (UAC refusée…).
    Erreur { onglet: u32, message: String },
}

static EVENEMENTS: Mutex<Vec<Evenement>> = Mutex::new(Vec::new());

pub fn prendre_evenements() -> Vec<Evenement> {
    std::mem::take(&mut *EVENEMENTS.lock().unwrap_or_else(|e| e.into_inner()))
}

fn verser(evenement: Evenement) {
    EVENEMENTS.lock().unwrap_or_else(|e| e.into_inner()).push(evenement);
}

enum Canal {
    Local { pseudo: Arc<Pseudo>, entree: Arc<Poignee> },
    /// Invite UAC en cours : rien à écrire encore. `fermer` retire l'entrée,
    /// et le relais arrivé ensuite est congédié (voir `relier_admin`).
    Attente,
    /// Les trames vers le relais — sous VERROU : frappes (fil du canal POST)
    /// et tailles (fil UI) ne doivent pas s'entrelacer.
    Admin { entree: Arc<Mutex<Poignee>> },
}

struct Session {
    /// Distingue deux sessions d'un même numéro d'onglet (voir `guetter`).
    generation: u64,
    canal: Canal,
}

static SESSIONS: Mutex<Option<HashMap<u32, Session>>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn avec_sessions<R>(f: impl FnOnce(&mut HashMap<u32, Session>) -> R) -> R {
    let mut garde = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    f(garde.get_or_insert_with(HashMap::new))
}

/// Lance `shell` dans une pseudo-console de `colonnes × lignes`, dans le
/// dossier personnel. Un onglet déjà ouvert est d'abord refermé.
pub fn ouvrir(onglet: u32, shell: Shell, colonnes: u32, lignes: u32) -> Result<(), String> {
    ouvrir_commande(onglet, &shell.ligne_de_commande(), colonnes, lignes)
}

fn ouvrir_commande(onglet: u32, commande: &str, colonnes: u32, lignes: u32) -> Result<(), String> {
    fermer(onglet);
    let (pseudo, entree, sortie, processus) = conpty::lancer(commande, conpty::taille(colonnes, lignes))?;
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    let canal = Canal::Local { pseudo: Arc::new(pseudo), entree: Arc::new(entree) };
    avec_sessions(|s| s.insert(onglet, Session { generation, canal }));
    suivre(onglet, generation, sortie, Arc::new(processus))
}

/// Les fils LECTEUR et GUETTEUR d'une session enregistrée.
fn suivre(onglet: u32, generation: u64, sortie: Poignee, processus: Arc<Poignee>) -> Result<(), String> {
    std::thread::Builder::new()
        .name(format!("pty-lecteur-{onglet}"))
        .spawn(move || lire(onglet, sortie))
        .map_err(|e| format!("Fil de lecture impossible : {e}"))?;
    std::thread::Builder::new()
        .name(format!("pty-guetteur-{onglet}"))
        .spawn(move || guetter(onglet, generation, processus))
        .map_err(|e| format!("Fil de surveillance impossible : {e}"))?;
    Ok(())
}

/// Ouvre un onglet ADMINISTRATEUR : l'invite UAC, puis le relais. Sur un FIL
/// — `ShellExecuteExW` ne rend la main qu'après l'invite. Le résultat arrive
/// par `Evenement::Pret` ou `Evenement::Erreur`.
pub fn ouvrir_admin(onglet: u32, shell: Shell, colonnes: u32, lignes: u32) {
    fermer(onglet);
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    avec_sessions(|s| s.insert(onglet, Session { generation, canal: Canal::Attente }));
    let lance = std::thread::Builder::new().name(format!("pty-admin-{onglet}")).spawn(move || {
        if let Err(message) = relier_admin(onglet, generation, shell, colonnes, lignes) {
            avec_sessions(|s| {
                if s.get(&onglet).is_some_and(|x| x.generation == generation) {
                    s.remove(&onglet);
                }
            });
            verser(Evenement::Erreur { onglet, message });
        }
    });
    if lance.is_err() {
        avec_sessions(|s| s.remove(&onglet));
        verser(Evenement::Erreur { onglet, message: "Fil d'ouverture impossible".into() });
    }
}

/// Le nom des tubes d'un relais : imprévisible, pour qu'aucun autre
/// processus ne puisse le préparer d'avance.
fn nom_de_tubes() -> Result<String, String> {
    // SAFETY : aucun argument.
    let guid = unsafe { windows::Win32::System::Com::CoCreateGuid() }.map_err(|e| format!("Nom de tube impossible : {e}"))?;
    Ok(format!("neuradesk-admin-{}-{:032x}", std::process::id(), guid.to_u128()))
}

fn large(texte: &str) -> Vec<u16> {
    texte.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `FILE_FLAG_FIRST_PIPE_INSTANCE` : échoue si le nom existe déjà — personne
/// ne peut s'être installé à sa place.
fn creer_tube(nom: &str, sens: FILE_FLAGS_AND_ATTRIBUTES) -> Result<Poignee, String> {
    let chemin = large(&format!(r"\\.\pipe\{nom}"));
    // SAFETY : chemin terminé par un zéro ; sécurité par défaut (l'utilisateur
    // courant, que le relais élevé est aussi).
    let poignee = unsafe {
        CreateNamedPipeW(
            PCWSTR(chemin.as_ptr()),
            sens | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            64 * 1024,
            64 * 1024,
            0,
            None,
        )
    };
    if poignee.is_invalid() {
        return Err(format!("Tube impossible : {}", windows::core::Error::from_thread()));
    }
    Ok(Poignee(poignee))
}

/// Attend la connexion du relais et vérifie que c'est bien LUI (`pid`).
fn attendre_le_relais(tube: &Poignee, pid: u32) -> Result<(), String> {
    // SAFETY : poignée de tube serveur valide ; appel synchrone.
    if let Err(e) = unsafe { ConnectNamedPipe(tube.0, None) } {
        if e.code() != ERROR_PIPE_CONNECTED.to_hresult() {
            return Err(format!("Relais non connecté : {e}"));
        }
    }
    let mut client = 0u32;
    // SAFETY : poignée de tube connectée.
    unsafe { GetNamedPipeClientProcessId(tube.0, &mut client) }.map_err(|e| format!("Client inconnu : {e}"))?;
    if client != pid {
        return Err("Un autre processus s'est connecté au relais : onglet refusé".into());
    }
    Ok(())
}

/// Si le relais meurt avant de s'être connecté, `ConnectNamedPipe` attendrait
/// pour toujours : on s'y connecte alors soi-même, ce qui la débloque — et la
/// vérification du `pid` refuse ce faux client.
fn debloquer_si_le_relais_meurt(nom: String, processus: Arc<Poignee>, connecte: Arc<AtomicBool>) {
    let _ = std::thread::Builder::new().name("pty-admin-garde".into()).spawn(move || {
        while !connecte.load(Ordering::Relaxed) {
            // SAFETY : poignée de processus tenue vivante par l'`Arc`.
            if unsafe { WaitForSingleObject(processus.0, 200) }.0 == 0 {
                for (sens, acces) in [("entree", GENERIC_READ.0), ("sortie", GENERIC_WRITE.0)] {
                    let chemin = large(&format!(r"\\.\pipe\{nom}-{sens}"));
                    // SAFETY : chemin terminé par un zéro ; poignée refermée aussitôt.
                    if let Ok(h) = unsafe {
                        CreateFileW(PCWSTR(chemin.as_ptr()), acces, FILE_SHARE_NONE, None, OPEN_EXISTING, Default::default(), None)
                    } {
                        drop(Poignee(h));
                    }
                }
                return;
            }
        }
    });
}

fn relier_admin(onglet: u32, generation: u64, shell: Shell, colonnes: u32, lignes: u32) -> Result<(), String> {
    let relais = std::env::current_exe()
        .map_err(|e| format!("Exécutable introuvable : {e}"))?
        .with_file_name("neuradesk-admin.exe");
    relier(onglet, generation, shell, colonnes, lignes, &relais, lancer_eleve)
}

/// Relie un onglet au relais `relais`, lancé par `lancer` (élevé en vrai ;
/// sans élévation dans le test, qui éprouve ainsi TOUT le reste du trajet).
fn relier(
    onglet: u32,
    generation: u64,
    shell: Shell,
    colonnes: u32,
    lignes: u32,
    relais: &std::path::Path,
    lancer: impl FnOnce(&std::path::Path, &str) -> Result<Poignee, String>,
) -> Result<(), String> {
    if !relais.is_file() {
        return Err(format!("{} introuvable", relais.display()));
    }
    let nom = nom_de_tubes()?;
    let entree = creer_tube(&format!("{nom}-entree"), PIPE_ACCESS_OUTBOUND)?;
    let sortie = creer_tube(&format!("{nom}-sortie"), PIPE_ACCESS_INBOUND)?;
    let parametres =
        format!("{} {} {} {nom} {}", shell.id(), colonnes.clamp(2, 1000), lignes.clamp(1, 1000), std::process::id());
    let processus = Arc::new(lancer(relais, &parametres)?);
    // SAFETY : poignée de processus valide.
    let pid = unsafe { GetProcessId(processus.0) };

    let connecte = Arc::new(AtomicBool::new(false));
    debloquer_si_le_relais_meurt(nom, Arc::clone(&processus), Arc::clone(&connecte));
    let reliee = attendre_le_relais(&entree, pid).and_then(|()| attendre_le_relais(&sortie, pid));
    connecte.store(true, Ordering::Relaxed);
    reliee?;

    // L'onglet a pu être fermé pendant l'invite : le relais est alors congédié
    // (tubes refermés en sortant d'ici — il sort, et son shell avec).
    let entree = Arc::new(Mutex::new(entree));
    let enregistre = avec_sessions(|s| match s.get_mut(&onglet) {
        Some(session) if session.generation == generation => {
            session.canal = Canal::Admin { entree };
            true
        }
        _ => false,
    });
    if !enregistre {
        return Ok(());
    }
    verser(Evenement::Pret { onglet });
    suivre(onglet, generation, sortie, processus)
}

/// L'invite UAC, puis le relais élevé.
fn lancer_eleve(relais: &std::path::Path, parametres: &str) -> Result<Poignee, String> {
    let fichier = large(&relais.to_string_lossy());
    let parametres = large(parametres);
    let dossier = large(&std::env::var("USERPROFILE").unwrap_or_default());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(fichier.as_ptr()),
        lpParameters: PCWSTR(parametres.as_ptr()),
        lpDirectory: PCWSTR(dossier.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY : structure initialisée, chaînes vivantes jusqu'au retour.
    if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
        return Err(if e.code() == ERROR_CANCELLED.to_hresult() {
            "Élévation refusée.".to_string()
        } else {
            format!("Lancement administrateur impossible : {e}")
        });
    }
    if info.hProcess.is_invalid() {
        return Err("Relais lancé sans poignée de processus".into());
    }
    Ok(Poignee(info.hProcess))
}

fn lire(onglet: u32, sortie: Poignee) {
    let mut tampon = vec![0u8; 16 * 1024];
    while let Some(lus) = conpty::lire_une_fois(sortie.0, &mut tampon) {
        verser(Evenement::Sortie { onglet, octets: tampon[..lus].to_vec() });
    }
    verser(Evenement::Fin { onglet });
}

fn guetter(onglet: u32, generation: u64, processus: Arc<Poignee>) {
    // SAFETY : poignée de processus tenue vivante par l'`Arc`.
    unsafe { WaitForSingleObject(processus.0, INFINITE) };
    // Seulement la MÊME session : l'onglet a pu être refermé et rouvert.
    let session = avec_sessions(|s| match s.get(&onglet) {
        Some(x) if x.generation == generation => s.remove(&onglet),
        _ => None,
    });
    drop(session);
}

/// Envoie des frappes au shell — sûr hors du fil UI (le canal POST, voir
/// `win32::cef_data`). Un onglet inconnu ou en attente les ignore.
pub fn ecrire(onglet: u32, octets: Vec<u8>) {
    enum Cible {
        Local(Arc<Poignee>),
        Admin(Arc<Mutex<Poignee>>),
    }
    let cible = avec_sessions(|s| match s.get(&onglet).map(|x| &x.canal) {
        Some(Canal::Local { entree, .. }) => Some(Cible::Local(Arc::clone(entree))),
        Some(Canal::Admin { entree }) => Some(Cible::Admin(Arc::clone(entree))),
        _ => None,
    });
    match cible {
        Some(Cible::Local(entree)) => {
            conpty::ecrire_tout(entree.0, &octets);
        }
        Some(Cible::Admin(entree)) => {
            let entree = entree.lock().unwrap_or_else(|e| e.into_inner());
            conpty::ecrire_tout(entree.0, &conpty::trame(conpty::TRAME_FRAPPES, &octets));
        }
        None => {}
    }
}

pub fn redimensionner(onglet: u32, colonnes: u32, lignes: u32) {
    enum Cible {
        Local(Arc<Pseudo>),
        Admin(Arc<Mutex<Poignee>>),
    }
    let cible = avec_sessions(|s| match s.get(&onglet).map(|x| &x.canal) {
        Some(Canal::Local { pseudo, .. }) => Some(Cible::Local(Arc::clone(pseudo))),
        Some(Canal::Admin { entree }) => Some(Cible::Admin(Arc::clone(entree))),
        _ => None,
    });
    match cible {
        // SAFETY : pseudo-console tenue vivante par l'`Arc`.
        Some(Cible::Local(pseudo)) => {
            let _ = unsafe { ResizePseudoConsole(pseudo.0, conpty::taille(colonnes, lignes)) };
        }
        Some(Cible::Admin(entree)) => {
            let entree = entree.lock().unwrap_or_else(|e| e.into_inner());
            conpty::ecrire_tout(entree.0, &conpty::trame_taille(colonnes, lignes));
        }
        None => {}
    }
}

/// Referme l'onglet : met fin au shell. La fermeture de la pseudo-console
/// part sur un fil — elle peut attendre que conhost ait fini d'écrire. Pour
/// un onglet administrateur, refermer le tube suffit : le relais sort.
pub fn fermer(onglet: u32) {
    if let Some(session) = avec_sessions(|s| s.remove(&onglet)) {
        let _ = std::thread::Builder::new().name("pty-fermeture".into()).spawn(move || drop(session));
    }
}

/// Referme tous les onglets — à la sortie de l'application, pour ne pas
/// laisser de shell orphelin.
pub fn fermer_tout() {
    let sessions: Vec<Session> = avec_sessions(|s| s.drain().map(|(_, x)| x).collect());
    drop(sessions);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::HANDLE;

    /// `prendre_evenements` VIDE une file commune : deux tests qui la lisent en
    /// parallèle se volent leurs évènements. Chacun la tient sous ce verrou.
    static FILE_COMMUNE: Mutex<()> = Mutex::new(());

    /// Un vrai shell sous pseudo-console, sans fenêtre et sans aucune frappe :
    /// sa sortie arrive, puis `Fin` quand il se termine (le guetteur referme
    /// la pseudo-console — sans lui, ce test attendrait indéfiniment).
    #[test]
    fn un_shell_ecrit_puis_se_termine() {
        const ONGLET: u32 = 4_000_000_001;
        let _file = FILE_COMMUNE.lock().unwrap_or_else(|e| e.into_inner());
        ouvrir_commande(ONGLET, "cmd.exe /d /c echo neuradesk-pty-ok", 80, 25).expect("lancement");
        let debut = Instant::now();
        let (mut texte, mut fini) = (Vec::new(), false);
        while !fini && debut.elapsed() < Duration::from_secs(15) {
            for evenement in prendre_evenements() {
                match evenement {
                    Evenement::Sortie { onglet, octets } if onglet == ONGLET => texte.extend(octets),
                    Evenement::Fin { onglet } if onglet == ONGLET => fini = true,
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let texte = String::from_utf8_lossy(&texte);
        assert!(texte.contains("neuradesk-pty-ok"), "{texte:?}");
        assert!(fini, "pas de fin après 15 s");
        assert!(avec_sessions(|s| !s.contains_key(&ONGLET)), "session restée dans la table");
    }

    #[test]
    fn les_identifiants_de_shell_font_l_aller_retour() {
        for shell in [Shell::Cmd, Shell::PowerShell, Shell::Pwsh] {
            assert_eq!(Shell::depuis_id(shell.id()), Some(shell));
        }
        assert_eq!(Shell::depuis_id("cmd.exe /c del"), None);
        assert!(shells().contains(&Shell::Cmd));
        assert_eq!(Shell::Pwsh.ligne_de_commande(), "pwsh.exe -NoLogo");
    }

    /// Le nom que l'application donne à ses tubes est celui que le relais
    /// accepte (sa règle est recopiée ici : le relais est un autre binaire).
    #[test]
    fn le_nom_des_tubes_est_imprevisible_et_accepte_par_le_relais() {
        let (a, b) = (nom_de_tubes().expect("nom"), nom_de_tubes().expect("nom"));
        assert_ne!(a, b);
        for nom in [&a, &b] {
            let reste = nom.strip_prefix("neuradesk-admin-").expect("préfixe");
            assert!(nom.len() < 100 && reste.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'), "{nom}");
        }
        let relais = include_str!("../relais_admin.rs");
        assert!(relais.contains("strip_prefix(\"neuradesk-admin-\")"), "la règle du relais a changé");
    }

    /// TOUT le trajet d'un onglet administrateur — sauf l'UAC : le vrai
    /// `neuradesk-admin.exe`, lancé SANS élévation, relie un cmd sans fenêtre ;
    /// une commande tapée par les tubes revient par les tubes, puis `exit`
    /// ferme l'onglet. ⚠️ Éprouve le binaire tel que compilé : `cargo build
    /// --release` d'abord s'il a changé.
    #[test]
    fn un_onglet_administrateur_passe_par_le_relais() {
        use std::os::windows::io::IntoRawHandle;
        const ONGLET: u32 = 4_000_000_002;
        let _file = FILE_COMMUNE.lock().unwrap_or_else(|e| e.into_inner());
        // ⛔ Le binaire de test n'est pas toujours à `release/deps/` : Cargo 1.100
        // le range sous `release/build/neuradesk/<hash>/`. On remonte jusqu'au
        // dossier qui contient le relais.
        let exe = std::env::current_exe().expect("exe");
        let relais = exe
            .ancestors()
            .skip(1)
            .map(|d| d.join("neuradesk-admin.exe"))
            .find(|r| r.is_file())
            .unwrap_or_else(|| panic!("neuradesk-admin.exe introuvable au-dessus de {} : cargo build --release d'abord", exe.display()));
        let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
        avec_sessions(|s| s.insert(ONGLET, Session { generation, canal: Canal::Attente }));
        let lancer = |relais: &std::path::Path, parametres: &str| {
            let enfant = std::process::Command::new(relais)
                .args(parametres.split(' '))
                .spawn()
                .map_err(|e| e.to_string())?;
            Ok(Poignee(HANDLE(enfant.into_raw_handle())))
        };
        relier(ONGLET, generation, Shell::Cmd, 80, 25, &relais, lancer).expect("relais relié");
        redimensionner(ONGLET, 100, 30);
        ecrire(ONGLET, b"echo neuradesk-relais-ok\r".to_vec());
        ecrire(ONGLET, b"exit\r".to_vec());
        let debut = Instant::now();
        let (mut texte, mut pret, mut fini) = (Vec::new(), false, false);
        while !fini && debut.elapsed() < Duration::from_secs(15) {
            for evenement in prendre_evenements() {
                match evenement {
                    Evenement::Pret { onglet } if onglet == ONGLET => pret = true,
                    Evenement::Sortie { onglet, octets } if onglet == ONGLET => texte.extend(octets),
                    Evenement::Fin { onglet } if onglet == ONGLET => fini = true,
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let texte = String::from_utf8_lossy(&texte);
        assert!(pret, "pas de Pret");
        assert!(texte.contains("neuradesk-relais-ok"), "{texte:?}");
        assert!(fini, "pas de fin après 15 s");
        assert!(avec_sessions(|s| !s.contains_key(&ONGLET)), "session restée dans la table");
    }

    /// Deux tubes du même nom : le second est refusé — personne ne peut
    /// s'installer sur le nom d'un relais.
    #[test]
    fn un_nom_de_tube_deja_pris_est_refuse() {
        let nom = nom_de_tubes().expect("nom");
        let premier = creer_tube(&nom, PIPE_ACCESS_OUTBOUND).expect("premier tube");
        assert!(creer_tube(&nom, PIPE_ACCESS_OUTBOUND).is_err());
        drop(premier);
    }
}
