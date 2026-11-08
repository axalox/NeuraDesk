//! **GSTREAMER, CHARGÉ À LA DEMANDE, DANS UN SOUS-PROCESSUS.**
//!
//! Le moteur de l'enregistreur (voir `win32::enregistrement` pour la chaîne,
//! `win32::enregistreur` pour l'applet). GStreamer n'est PAS lié à
//! l'exécutable : ses DLL sont chargées par `LoadLibraryExW` depuis un dossier
//! trouvé à l'exécution (voir `racine`), comme libVLC (`lecteur_video`).
//!
//! ⭐ **DANS UN SOUS-PROCESSUS, ET C'EST VOULU** : un pilote de caméra ou
//! d'encodeur qui plante ne doit faire tomber que l'enregistrement, pas les
//! barres. Mesuré avec libVLC le 2026-10-31 : une caméra VIRTUELLE choisie par
//! défaut a tué le processus. Le même `neuradesk.exe`, lancé avec `DRAPEAU`,
//! fait le travail et SORT (le modèle est `decodeur_document`).
//!
//! **PROTOCOLE** : la chaîne arrive dans un fichier (pas sur la ligne de
//! commande : les chemins de périphériques sont pleins de `\` et de `#`).
//! L'enfant écrit sur sa sortie standard `en-cours`, puis `fini` ou
//! `erreur: <message>`. Il s'arrête PROPREMENT — un fin de flux (EOS) qui
//! referme le fichier — quand il lit `stop` sur son entrée standard, ou quand
//! elle se ferme (le parent est parti : on garde ce qui a été enregistré).

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windows::core::{s, HSTRING};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, SetDllDirectoryW, LOAD_WITH_ALTERED_SEARCH_PATH,
};

/// Le premier argument qui fait de l'exécutable un enregistreur.
pub const DRAPEAU: &str = "--enregistrer";
/// … ou un énumérateur de périphériques.
pub const DRAPEAU_LISTE: &str = "--lister-peripheriques";

/// Où chercher GStreamer : à côté de l'exécutable (le portable), puis là où
/// l'installeur officiel le pose (pour toute la machine, ou pour l'utilisateur
/// — c'est le cas de winget).
fn candidats() -> Vec<PathBuf> {
    let mut dossiers = Vec::new();
    if let Some(exe) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        dossiers.push(exe.join("gstreamer"));
    }
    if let Some(d) = std::env::var_os("GSTREAMER_1_0_ROOT_MSVC_X86_64") {
        dossiers.push(PathBuf::from(d));
    }
    if let Some(d) = std::env::var_os("LOCALAPPDATA") {
        dossiers.push(PathBuf::from(d).join(r"Programs\gstreamer\1.0\msvc_x86_64"));
    }
    for variable in ["ProgramW6432", "ProgramFiles"] {
        if let Some(d) = std::env::var_os(variable) {
            dossiers.push(PathBuf::from(d).join(r"gstreamer\1.0\msvc_x86_64"));
        }
    }
    dossiers
}

/// Le dossier GStreamer utilisable : il porte `bin\gstreamer-1.0-0.dll` et ses
/// plugins.
pub fn racine() -> Option<PathBuf> {
    candidats().into_iter().find(|d| d.join(r"bin\gstreamer-1.0-0.dll").is_file() && d.join(r"lib\gstreamer-1.0").is_dir())
}

/// GStreamer est-il là (sans le charger) ?
pub fn installe() -> bool {
    racine().is_some()
}

// ─── L'API, RÉSOLUE À LA MAIN ───────────────────────────────────────────────

/// `GError` (glib) : `domain`, `code`, `message`.
#[repr(C)]
struct GError {
    domaine: u32,
    code: c_int,
    message: *const c_char,
}

/// `GList` (glib).
#[repr(C)]
struct GList {
    donnee: *mut c_void,
    suivant: *mut GList,
    precedent: *mut GList,
}

/// Décalage du champ `type` d'un `GstMessage` : il suit un `GstMiniObject`,
/// 64 octets en 64 bits (`GType`, 3 entiers, 3 pointeurs de fonction, un
/// entier et un pointeur privés). Stable depuis GStreamer 1.0.
const DECALAGE_TYPE_MESSAGE: usize = 64;
const MESSAGE_EOS: u32 = 1;
const MESSAGE_ERREUR: u32 = 2;
const ETAT_NUL: c_int = 1;
const ETAT_LECTURE: c_int = 4;
const CHANGEMENT_ECHOUE: c_int = 0;

type Ptr = *mut c_void;

#[allow(clippy::type_complexity)]
struct Api {
    init_check: unsafe extern "C" fn(*mut c_int, *mut *mut *mut c_char, *mut *mut GError) -> c_int,
    parse_launch: unsafe extern "C" fn(*const c_char, *mut *mut GError) -> Ptr,
    set_state: unsafe extern "C" fn(Ptr, c_int) -> c_int,
    get_bus: unsafe extern "C" fn(Ptr) -> Ptr,
    pop_filtre: unsafe extern "C" fn(Ptr, u64, u32) -> Ptr,
    parse_error: unsafe extern "C" fn(Ptr, *mut *mut GError, *mut *mut c_char),
    send_event: unsafe extern "C" fn(Ptr, Ptr) -> c_int,
    event_eos: unsafe extern "C" fn() -> Ptr,
    object_unref: unsafe extern "C" fn(Ptr),
    mini_unref: unsafe extern "C" fn(Ptr),
    error_free: unsafe extern "C" fn(*mut GError),
    g_free: unsafe extern "C" fn(Ptr),
    moniteur_new: unsafe extern "C" fn() -> Ptr,
    moniteur_filtre: unsafe extern "C" fn(Ptr, *const c_char, Ptr) -> u32,
    moniteur_start: unsafe extern "C" fn(Ptr) -> c_int,
    moniteur_stop: unsafe extern "C" fn(Ptr),
    moniteur_liste: unsafe extern "C" fn(Ptr) -> *mut GList,
    device_nom: unsafe extern "C" fn(Ptr) -> *mut c_char,
    device_proprietes: unsafe extern "C" fn(Ptr) -> Ptr,
    structure_chaine: unsafe extern "C" fn(Ptr, *const c_char) -> *const c_char,
    structure_booleen: unsafe extern "C" fn(Ptr, *const c_char, *mut c_int) -> c_int,
    structure_free: unsafe extern "C" fn(Ptr),
    liste_free: unsafe extern "C" fn(*mut GList),
}

/// Prépare l'environnement de GStreamer (DLL, plugins, registre) puis résout
/// les fonctions. À n'appeler que dans le SOUS-PROCESSUS : il change le dossier
/// de recherche des DLL et des variables d'environnement.
fn charger() -> Result<Api, String> {
    let racine = racine().ok_or("GStreamer n'est pas installé")?;
    let bin = racine.join("bin");
    // Les dépendances des plugins (glib, gstvideo…) se trouvent dans `bin`.
    unsafe { SetDllDirectoryW(&HSTRING::from(bin.as_os_str())) }.map_err(|e| format!("SetDllDirectory : {e}"))?;
    // SAFETY : sous-processus à un seul fil à ce stade, avant tout appel GStreamer.
    unsafe {
        std::env::set_var("GST_PLUGIN_SYSTEM_PATH_1_0", racine.join(r"lib\gstreamer-1.0"));
        std::env::remove_var("GST_PLUGIN_PATH_1_0");
        std::env::remove_var("GST_PLUGIN_PATH");
        // Le registre des plugins, reconstruit au premier lancement : dans
        // le cache de l'utilisateur, pas à côté de l'exe (dossier en lecture
        // seule possible).
        if let Some(cache) = std::env::var_os("LOCALAPPDATA") {
            let dossier = PathBuf::from(cache).join("NeuraDesk");
            let _ = std::fs::create_dir_all(&dossier);
            std::env::set_var("GST_REGISTRY_1_0", dossier.join("gstreamer-registre.bin"));
        }
        // Pas de `gst-plugin-scanner.exe` dans le portable : l'analyse se fait ici.
        std::env::set_var("GST_REGISTRY_FORK", "no");
    }
    let charger_dll = |nom: &str| -> Result<HMODULE, String> {
        let chemin = bin.join(nom);
        unsafe { LoadLibraryExW(&HSTRING::from(chemin.as_os_str()), None, LOAD_WITH_ALTERED_SEARCH_PATH) }
            .map_err(|e| format!("{nom} ne se charge pas : {e}"))
    };
    let gst = charger_dll("gstreamer-1.0-0.dll")?;
    let glib = charger_dll("glib-2.0-0.dll")?;
    macro_rules! fonction {
        ($module:expr, $nom:literal) => {
            match unsafe { GetProcAddress($module, s!($nom)) } {
                // SAFETY : prototypes de `gst.h` / `glib.h` (GStreamer 1.x).
                Some(f) => unsafe { std::mem::transmute_copy(&f) },
                None => return Err(format!("GStreamer incomplet : {} manque", $nom)),
            }
        };
    }
    Ok(Api {
        init_check: fonction!(gst, "gst_init_check"),
        parse_launch: fonction!(gst, "gst_parse_launch"),
        set_state: fonction!(gst, "gst_element_set_state"),
        get_bus: fonction!(gst, "gst_element_get_bus"),
        pop_filtre: fonction!(gst, "gst_bus_timed_pop_filtered"),
        parse_error: fonction!(gst, "gst_message_parse_error"),
        send_event: fonction!(gst, "gst_element_send_event"),
        event_eos: fonction!(gst, "gst_event_new_eos"),
        object_unref: fonction!(gst, "gst_object_unref"),
        mini_unref: fonction!(gst, "gst_mini_object_unref"),
        error_free: fonction!(glib, "g_error_free"),
        g_free: fonction!(glib, "g_free"),
        moniteur_new: fonction!(gst, "gst_device_monitor_new"),
        moniteur_filtre: fonction!(gst, "gst_device_monitor_add_filter"),
        moniteur_start: fonction!(gst, "gst_device_monitor_start"),
        moniteur_stop: fonction!(gst, "gst_device_monitor_stop"),
        moniteur_liste: fonction!(gst, "gst_device_monitor_get_devices"),
        device_nom: fonction!(gst, "gst_device_get_display_name"),
        device_proprietes: fonction!(gst, "gst_device_get_properties"),
        structure_chaine: fonction!(gst, "gst_structure_get_string"),
        structure_booleen: fonction!(gst, "gst_structure_get_boolean"),
        structure_free: fonction!(gst, "gst_structure_free"),
        liste_free: fonction!(glib, "g_list_free"),
    })
}

impl Api {
    /// Le message d'un `GError`, libéré au passage.
    fn message(&self, erreur: *mut GError) -> String {
        if erreur.is_null() {
            return "erreur inconnue".into();
        }
        // SAFETY : `GError` rendu par GStreamer, libéré une seule fois ici.
        unsafe {
            let texte = if (*erreur).message.is_null() {
                "erreur sans message".into()
            } else {
                CStr::from_ptr((*erreur).message).to_string_lossy().into_owned()
            };
            (self.error_free)(erreur);
            texte
        }
    }

    fn initialiser(&self) -> Result<(), String> {
        let mut erreur: *mut GError = std::ptr::null_mut();
        // SAFETY : pas d'arguments de ligne de commande transmis.
        if unsafe { (self.init_check)(std::ptr::null_mut(), std::ptr::null_mut(), &mut erreur) } == 0 {
            return Err(format!("GStreamer ne démarre pas : {}", self.message(erreur)));
        }
        Ok(())
    }
}

// ─── LE SOUS-PROCESSUS ──────────────────────────────────────────────────────

/// À appeler EN TÊTE de `main()` : si le processus a été lancé en enregistreur
/// ou en énumérateur, fait le travail et SORT, sans jamais revenir.
pub fn executer_si_demande() {
    let arguments: Vec<_> = std::env::args_os().collect();
    let Some(drapeau) = arguments.get(1).and_then(|a| a.to_str()) else { return };
    let code = match drapeau {
        DRAPEAU => match arguments.get(2) {
            Some(fichier) => enregistrer(Path::new(fichier)),
            None => 2,
        },
        DRAPEAU_LISTE => lister(),
        _ => return,
    };
    std::process::exit(code);
}

fn dire(ligne: &str) {
    let mut sortie = std::io::stdout().lock();
    let _ = writeln!(sortie, "{ligne}");
    let _ = sortie.flush();
}

/// Lance la chaîne lue dans `fichier`, jusqu'à `stop` ou la fermeture de
/// l'entrée standard. Rend le code de sortie.
fn enregistrer(fichier: &Path) -> i32 {
    let resultat = (|| -> Result<(), String> {
        let description = std::fs::read_to_string(fichier).map_err(|e| format!("chaîne illisible : {e}"))?;
        let api = charger()?;
        api.initialiser()?;
        let description = CString::new(description.trim()).map_err(|_| "chaîne invalide")?;
        let mut erreur: *mut GError = std::ptr::null_mut();
        // SAFETY : chaîne terminée par NUL, `erreur` reçoit un GError ou reste nul.
        let chaine = unsafe { (api.parse_launch)(description.as_ptr(), &mut erreur) };
        if !erreur.is_null() {
            if !chaine.is_null() {
                unsafe { (api.object_unref)(chaine) };
            }
            return Err(format!("chaîne refusée : {}", api.message(erreur)));
        }
        if chaine.is_null() {
            return Err("chaîne refusée".into());
        }
        let resultat = faire_tourner(&api, chaine);
        unsafe {
            (api.set_state)(chaine, ETAT_NUL);
            (api.object_unref)(chaine);
        }
        resultat
    })();
    match resultat {
        Ok(()) => {
            dire("fini");
            0
        }
        Err(e) => {
            dire(&format!("erreur: {e}"));
            1
        }
    }
}

/// La boucle : démarre, surveille le bus, envoie la fin de flux à l'arrêt
/// demandé, attend qu'elle ait traversé la chaîne (le fichier est alors fermé).
fn faire_tourner(api: &Api, chaine: Ptr) -> Result<(), String> {
    const PAS_NS: u64 = 100_000_000;
    // Au-delà, la fin de flux n'arrive pas : on sort quand même.
    const ATTENTE_EOS: std::time::Duration = std::time::Duration::from_secs(15);
    let arret = Arc::new(AtomicBool::new(false));
    {
        let arret = arret.clone();
        let _ = std::thread::Builder::new().name("ordres".into()).spawn(move || {
            let entree = std::io::stdin();
            for ligne in entree.lock().lines() {
                match ligne {
                    Ok(l) if l.trim() == "stop" => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            // `stop`, ou l'entrée fermée : on termine proprement.
            arret.store(true, Ordering::SeqCst);
        });
    }
    // SAFETY : `chaine` est un GstElement vivant jusqu'au retour.
    if unsafe { (api.set_state)(chaine, ETAT_LECTURE) } == CHANGEMENT_ECHOUE {
        return Err(premiere_erreur(api, chaine).unwrap_or_else(|| "la chaîne ne démarre pas".into()));
    }
    dire("en-cours");
    let bus = unsafe { (api.get_bus)(chaine) };
    let mut eos_envoye: Option<std::time::Instant> = None;
    let resultat = loop {
        if arret.load(Ordering::SeqCst) && eos_envoye.is_none() {
            unsafe { (api.send_event)(chaine, (api.event_eos)()) };
            eos_envoye = Some(std::time::Instant::now());
        }
        if eos_envoye.is_some_and(|t| t.elapsed() > ATTENTE_EOS) {
            break Err("la fin d'enregistrement n'est pas arrivée : fichier peut-être incomplet".into());
        }
        let message = unsafe { (api.pop_filtre)(bus, PAS_NS, MESSAGE_EOS | MESSAGE_ERREUR) };
        if message.is_null() {
            continue;
        }
        // SAFETY : voir `DECALAGE_TYPE_MESSAGE`.
        let sorte = unsafe { *(message as *const u8).add(DECALAGE_TYPE_MESSAGE).cast::<u32>() };
        let issue = if sorte == MESSAGE_ERREUR { Some(Err(texte_erreur(api, message))) } else { Some(Ok(())) };
        unsafe { (api.mini_unref)(message) };
        if let Some(issue) = issue {
            break issue;
        }
    };
    unsafe { (api.object_unref)(bus) };
    resultat
}

/// Le texte d'un message d'erreur du bus (avec le détail de débogage, qui dit
/// QUEL élément a échoué).
fn texte_erreur(api: &Api, message: Ptr) -> String {
    let mut erreur: *mut GError = std::ptr::null_mut();
    let mut detail: *mut c_char = std::ptr::null_mut();
    unsafe { (api.parse_error)(message, &mut erreur, &mut detail) };
    let mut texte = api.message(erreur);
    if !detail.is_null() {
        let d = unsafe { CStr::from_ptr(detail) }.to_string_lossy().into_owned();
        unsafe { (api.g_free)(detail.cast()) };
        texte = format!("{texte} ({d})");
    }
    texte
}

/// Une erreur déjà postée sur le bus (au démarrage refusé).
fn premiere_erreur(api: &Api, chaine: Ptr) -> Option<String> {
    let bus = unsafe { (api.get_bus)(chaine) };
    let message = unsafe { (api.pop_filtre)(bus, 0, MESSAGE_ERREUR) };
    let texte = (!message.is_null()).then(|| {
        let t = texte_erreur(api, message);
        unsafe { (api.mini_unref)(message) };
        t
    });
    unsafe { (api.object_unref)(bus) };
    texte
}

// ─── LES PÉRIPHÉRIQUES ──────────────────────────────────────────────────────

/// Un périphérique proposé dans le panneau.
#[derive(Clone, Debug, PartialEq)]
pub struct Peripherique {
    pub nom: String,
    /// Le NOM d'une webcam (voir `enregistrement::branche_webcam`), le
    /// `device` (id WASAPI) d'un micro.
    pub id: String,
}

/// Les webcams et les micros, en JSON sur la sortie standard.
fn lister() -> i32 {
    let resultat = (|| -> Result<serde_json::Value, String> {
        let api = charger()?;
        api.initialiser()?;
        Ok(serde_json::json!({
            "webcams": en_json(&peripheriques(&api, "Video/Source", "mediafoundation", None)),
            "micros": en_json(&peripheriques(&api, "Audio/Source", "wasapi2", Some(c"device.id"))),
        }))
    })();
    match resultat {
        Ok(v) => {
            dire(&v.to_string());
            0
        }
        Err(e) => {
            dire(&serde_json::json!({ "erreur": e }).to_string());
            1
        }
    }
}

fn en_json(liste: &[Peripherique]) -> serde_json::Value {
    liste.iter().map(|p| serde_json::json!({ "nom": p.nom, "id": p.id })).collect()
}

/// Les périphériques de la classe `classe` servis par `api_voulue`, identifiés
/// par la propriété `cle` — par leur nom sans elle. Les sorties en bouclage et les entrées « par
/// défaut » sont écartées (le panneau propose « par défaut » lui-même).
fn peripheriques(api: &Api, classe: &str, api_voulue: &str, cle: Option<&CStr>) -> Vec<Peripherique> {
    let mut sortie = Vec::new();
    let classe = CString::new(classe).unwrap_or_default();
    unsafe {
        let moniteur = (api.moniteur_new)();
        (api.moniteur_filtre)(moniteur, classe.as_ptr(), std::ptr::null_mut());
        if (api.moniteur_start)(moniteur) == 0 {
            (api.object_unref)(moniteur);
            return sortie;
        }
        let liste = (api.moniteur_liste)(moniteur);
        let mut noeud = liste;
        while !noeud.is_null() {
            let device = (*noeud).donnee;
            let proprietes = (api.device_proprietes)(device);
            let chaine = |nom: &CStr| -> Option<String> {
                if proprietes.is_null() {
                    return None;
                }
                let p = (api.structure_chaine)(proprietes, nom.as_ptr());
                (!p.is_null()).then(|| CStr::from_ptr(p).to_string_lossy().into_owned())
            };
            let booleen = |nom: &CStr| -> bool {
                let mut v: c_int = 0;
                !proprietes.is_null() && (api.structure_booleen)(proprietes, nom.as_ptr(), &mut v) != 0 && v != 0
            };
            let bonne_api = chaine(c"device.api").as_deref() == Some(api_voulue);
            let ecarte = booleen(c"wasapi2.device.loopback") || booleen(c"device.default");
            if bonne_api && !ecarte {
                let nom_c = (api.device_nom)(device);
                let nom = (!nom_c.is_null()).then(|| CStr::from_ptr(nom_c).to_string_lossy().into_owned());
                if !nom_c.is_null() {
                    (api.g_free)(nom_c.cast());
                }
                let id = match cle {
                    Some(cle) => chaine(cle),
                    None => nom.clone(),
                };
                if let (Some(nom), Some(id)) = (nom, id) {
                    sortie.push(Peripherique { nom, id });
                }
            }
            if !proprietes.is_null() {
                (api.structure_free)(proprietes);
            }
            (api.object_unref)(device);
            noeud = (*noeud).suivant;
        }
        (api.liste_free)(liste);
        (api.moniteur_stop)(moniteur);
        (api.object_unref)(moniteur);
    }
    sortie
}
