//! Client minimal pour l'API distante de Voicemeeter (VB-Audio), voir
//! `VoicemeeterRemoteAPI.pdf` (fourni par VB-Audio avec son SDK, non redistribué ici) — demande explicite : "une nouvelle icone
//! son qui s'appellera voicemeeter... ouvrir un popup de contrôle du volume
//! via l'API Remote". `VoicemeeterRemote.dll` n'est PAS liée statiquement
//! (elle n'est installée qu'avec Voicemeeter lui-même, pas dans le SDK
//! Windows) : chargée dynamiquement à l'exécution, comme `win32::theme`
//! (`uxtheme.dll`, ordinaux non documentés) le fait déjà pour une autre DLL —
//! même best-effort, mêmes garanties (`None`/`Result` plutôt qu'un panic si
//! une étape échoue).
//!
//! Connexion établie UNE SEULE FOIS, mémorisée pour toute la durée de vie du
//! process (voir `with_client`, `VoicemeeterRemoteAPI.pdf` p.7 :
//! "Login function must be called once by the user application, for all its
//! duration life") — le login réussit MÊME SI Voicemeeter n'est pas encore
//! lancé (même doc, p.7) : `snapshot()` distingue "pas installé" (`None`,
//! rien à faire) de "installé mais pas lancé" (`Snapshot::running == false`,
//! l'utilisateur peut le lancer depuis le popup, voir `launch`).

use std::ffi::CString;
use std::path::PathBuf;
use std::sync::OnceLock;

use windows::core::{s, PCWSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;

use crate::win32::registry_util::{enumerate_subkeys, read_string_value};

/// Nom du fichier DLL selon l'architecture du process — voir la doc PDF
/// (p.6) : "use right DLL according O/S type", `VoicemeeterRemote64.dll` en
/// 64 bits (ce projet compile toujours en 64 bits, voir `Cargo.toml`, mais
/// l'alternative reste correcte si ça changeait un jour).
#[cfg(target_pointer_width = "64")]
const DLL_FILE_NAME: &str = "VoicemeeterRemote64.dll";
#[cfg(not(target_pointer_width = "64"))]
const DLL_FILE_NAME: &str = "VoicemeeterRemote.dll";

type LoginFn = unsafe extern "system" fn() -> i32;
type RunVoicemeeterFn = unsafe extern "system" fn(i32) -> i32;
type GetVoicemeeterTypeFn = unsafe extern "system" fn(*mut i32) -> i32;
type IsParametersDirtyFn = unsafe extern "system" fn() -> i32;
type GetParameterFloatFn = unsafe extern "system" fn(*const i8, *mut f32) -> i32;
type SetParameterFloatFn = unsafe extern "system" fn(*const i8, f32) -> i32;
type GetParameterStringAFn = unsafe extern "system" fn(*const i8, *mut i8) -> i32;

/// Taille (en octets) du tampon que `VBVMR_GetParameterStringA` s'attend à
/// pouvoir remplir — voir `VoicemeeterRemote.h` : "Pointer on String (512
/// char or wchar) receiving the wanted value." PAS une limite devinée : un
/// tampon plus petit exposerait la DLL à écrire hors de ses bornes.
const PARAMETER_STRING_BUFFER_LEN: usize = 512;

/// Table de fonctions résolues dans la DLL, PLUS le type Voicemeeter détecté
/// à l'INSTALLATION (1 Standard/2 Banana/3 Potato, voir `find_install`) —
/// juste des pointeurs de fonction et un entier, donc automatiquement `Send`
/// + `Sync` (aucun pointeur de DONNÉES ni `HMODULE` conservé : voir la doc
/// de `connect`, le module reste chargé pour toute la durée du process sans
/// qu'il soit besoin de garder son handle).
struct Api {
    login: LoginFn,
    run: RunVoicemeeterFn,
    get_type: GetVoicemeeterTypeFn,
    is_dirty: IsParametersDirtyFn,
    get_float: GetParameterFloatFn,
    set_float: SetParameterFloatFn,
    get_string: GetParameterStringAFn,
    /// Repli pour `launch()` tant que `VBVMR_GetVoicemeeterType` échoue
    /// encore (Voicemeeter pas lancé) — voir `find_install`.
    install_vtype: i32,
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Cherche le dossier d'installation de Voicemeeter (Standard/Banana/Potato,
/// n'importe laquelle des trois) via les entrées "Désinstaller un programme"
/// du registre — pas de clé dédiée documentée par VB-Audio pour ça. Une
/// seule entrée est confirmée par recherche externe (`SOFTWARE\...\
/// Uninstall\VB:Voicemeeter {17359A74-1236-5467}`, vérifiée présente sur
/// cette machine) mais le GUID diffère très probablement pour Banana/Potato
/// (chaque édition a sa propre entrée) : plutôt que deviner d'autres GUID,
/// énumère TOUTES les entrées et filtre par `DisplayName` commençant par
/// "Voicemeeter" — fonctionne pour les trois éditions sans les connaître
/// à l'avance. Essaie la vue registre NATIVE puis `WOW6432Node` (Voicemeeter
/// est un installeur 32 bits ; sur cette machine [64 bits] son entrée
/// n'existe QUE dans la vue 32 bits, confirmé par une requête directe) :
/// aucune des deux n'est présumée, les deux sont tentées.
/// Voicemeeter est-il installé ? — la même reconnaissance que celle qui
/// charge sa DLL (voir `find_install`).
pub fn installe() -> bool {
    find_install().is_some()
}

fn find_install() -> Option<(PathBuf, i32)> {
    const UNINSTALL_KEYS: [&str; 2] = [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];
    for uninstall_key in UNINSTALL_KEYS {
        for subkey_name in enumerate_subkeys(HKEY_LOCAL_MACHINE, uninstall_key) {
            let subkey = format!("{uninstall_key}\\{subkey_name}");
            let Some(display_name) = read_string_value(HKEY_LOCAL_MACHINE, &subkey, "DisplayName") else { continue };
            if !display_name.starts_with("Voicemeeter") {
                continue;
            }
            let Some(uninstall_string) = read_string_value(HKEY_LOCAL_MACHINE, &subkey, "UninstallString") else {
                continue;
            };
            let Some(folder) = PathBuf::from(&uninstall_string).parent().map(|p| p.to_path_buf()) else { continue };
            // Ordre de test important : "Potato" avant "Banana" avant le
            // repli Standard — les noms affichés observés sont "Voicemeeter"
            // (Standard), "Voicemeeter Banana" et "Voicemeeter Potato".
            let vtype = if display_name.contains("Potato") {
                3
            } else if display_name.contains("Banana") {
                2
            } else {
                1
            };
            return Some((folder, vtype));
        }
    }
    None
}

/// Résout `name` dans `module` et le transmute vers `F` — même technique que
/// `win32::theme::resolve_ordinal` (un pointeur de fonction n'est qu'une
/// adresse, valide à réinterpréter tant que la signature `F` correspond
/// réellement à ce que documente `VoicemeeterRemote.h`).
unsafe fn resolve<F: Copy>(module: HMODULE, name: windows::core::PCSTR) -> Option<F> {
    let proc = GetProcAddress(module, name)?;
    Some(std::mem::transmute_copy(&proc))
}

/// Charge la DLL et se connecte — appelée UNE SEULE FOIS par `with_client`
/// (voir sa doc). Le module chargé n'est JAMAIS libéré (pas de
/// `FreeLibrary`) : ni son handle ni son nom ne sont conservés au-delà de
/// cette fonction, Windows le garde chargé pour toute la durée du process,
/// exactement la durée de vie voulue pour cette connexion (voir la doc de
/// module).
fn connect() -> Result<Api, &'static str> {
    let Some((folder, install_vtype)) = find_install() else { return Err("Voicemeeter n'est pas installé") };
    let dll_path = folder.join(DLL_FILE_NAME);
    let dll_wide = to_wide(&dll_path.to_string_lossy());
    let module =
        unsafe { LoadLibraryW(PCWSTR(dll_wide.as_ptr())) }.map_err(|_| "échec du chargement de VoicemeeterRemote.dll")?;
    let api = unsafe {
        Api {
            login: resolve(module, s!("VBVMR_Login")).ok_or("VBVMR_Login introuvable")?,
            run: resolve(module, s!("VBVMR_RunVoicemeeter")).ok_or("VBVMR_RunVoicemeeter introuvable")?,
            get_type: resolve(module, s!("VBVMR_GetVoicemeeterType")).ok_or("VBVMR_GetVoicemeeterType introuvable")?,
            is_dirty: resolve(module, s!("VBVMR_IsParametersDirty")).ok_or("VBVMR_IsParametersDirty introuvable")?,
            get_float: resolve(module, s!("VBVMR_GetParameterFloat")).ok_or("VBVMR_GetParameterFloat introuvable")?,
            set_float: resolve(module, s!("VBVMR_SetParameterFloat")).ok_or("VBVMR_SetParameterFloat introuvable")?,
            get_string: resolve(module, s!("VBVMR_GetParameterStringA")).ok_or("VBVMR_GetParameterStringA introuvable")?,
            install_vtype,
        }
    };
    if unsafe { (api.login)() } < 0 {
        return Err("échec de connexion à Voicemeeter (Login)");
    }
    Ok(api)
}

/// Connexion mémorisée pour toute la durée du process (voir la doc de
/// module) — `None` renvoyé par l'appelant si la connexion a échoué
/// (Voicemeeter pas installé, DLL/symboles introuvables...), jamais
/// retentée : ces échecs sont permanents pour la durée de vie du process
/// (rien ne changera sans redémarrer NeuraDesk).
fn with_client<T>(f: impl FnOnce(&Api) -> T) -> Option<T> {
    static CLIENT: OnceLock<Result<Api, &'static str>> = OnceLock::new();
    CLIENT.get_or_init(connect).as_ref().ok().map(f)
}

/// Nombre de strips (entrées) et de bus (sorties) pour chaque type
/// Voicemeeter (1 Standard/2 Banana/3 Potato) — voir
/// `VoicemeeterRemoteAPI.pdf`, "Input Strip Parameters"/"BUS Parameters".
fn channel_counts(vtype: i32) -> (usize, usize) {
    match vtype {
        2 => (5, 5),
        3 => (8, 8),
        _ => (3, 2),
    }
}

/// Nombre de bus PHYSIQUES ("A", reliés à un vrai périphérique de sortie —
/// voir `Bus[i].device.name`) et VIRTUELS ("B", pilotables par d'autres
/// applications, jamais reliés à un périphérique) pour chaque type
/// Voicemeeter — voir `VoicemeeterRemoteAPI.pdf` et l'interface
/// Voicemeeter elle-même : Standard (A1 + B1), Banana (A1-A3 + B1-B2),
/// Potato (A1-A5 + B1-B3). La somme des deux vaut toujours `channel_counts
/// (vtype).1` (le compte TOTAL de bus) — utilisé par `bus_label` pour
/// nommer chaque bus "à la Voicemeeter" plutôt qu'un générique "Sortie N"
/// qui ne correspond à rien dans l'interface Voicemeeter — demande
/// explicite : "ajoute les bus de A1 à B3 au menu".
fn bus_ab_split(vtype: i32) -> (usize, usize) {
    match vtype {
        2 => (3, 2),
        3 => (5, 3),
        _ => (1, 1),
    }
}

/// Nom Voicemeeter natif du bus d'index `index` (0-based, dans l'ordre
/// `Bus[0]`/`Bus[1]`/...) pour l'édition `vtype` — "A1".."A5" pour les bus
/// physiques puis "B1".."B3" pour les virtuels (voir `bus_ab_split`) :
/// TOUJOURS défini pour un index valide (`< channel_counts(vtype).1`),
/// contrairement au libellé PERSONNALISÉ de l'utilisateur ou au nom du
/// périphérique physique (voir `win32::popup_menu::
/// voicemeeter_channel_label`, qui restent prioritaires sur celui-ci —
/// simple repli générique, comme "Entrée N" pour les strips).
pub fn bus_label(vtype: i32, index: usize) -> String {
    let (physical_count, _virtual_count) = bus_ab_split(vtype);
    if index < physical_count {
        format!("A{}", index + 1)
    } else {
        format!("B{}", index - physical_count + 1)
    }
}

/// État courant de Voicemeeter, à relire À CHAQUE FRAME pendant que le popup
/// est ouvert (voir `win32::popup_menu::render_voicemeeter`) — aussi bon
/// marché qu'`win32::audio::default_output` (voir sa doc de module) : pas de
/// mise en cache au-delà de la connexion elle-même.
pub struct Snapshot {
    /// `false` si Voicemeeter est installé mais pas lancé (voir `launch`) —
    /// jamais `true` si la connexion elle-même a échoué (voir `snapshot`,
    /// qui renvoie `None` dans ce cas).
    pub running: bool,
    pub strip_count: usize,
    pub bus_count: usize,
    /// 1 Standard/2 Banana/3 Potato — voir `bus_label`, qui en a besoin pour
    /// nommer chaque bus "à la Voicemeeter" (A1.."A5"/B1.."B3", le split
    /// exact dépendant de l'édition). `0` si `running` est `false` (jamais
    /// interrogé dans ce cas, voir `snapshot`) : sans signification, pas
    /// utilisé par l'appelant tant que Voicemeeter n'est pas lancé.
    pub vtype: i32,
}

/// `None` si Voicemeeter n'est pas installé ou si la connexion à son API a
/// échoué (DLL/symboles introuvables, `Login` refusé) — best-effort comme le
/// reste des intégrations `win32::*` de ce projet, jamais un panic.
pub fn snapshot() -> Option<Snapshot> {
    with_client(|api| {
        // `VBVMR_IsParametersDirty` renvoie un code négatif tant que
        // Voicemeeter n'est pas lancé (voir docs, "the returned code can be
        // used to detect a reconnection/disconnection") — la façon
        // documentée de distinguer "installé mais éteint" de "en cours
        // d'exécution", sans dépendre d'une détection de processus à part.
        if unsafe { (api.is_dirty)() } < 0 {
            return Snapshot { running: false, strip_count: 0, bus_count: 0, vtype: 0 };
        }
        let mut vtype: i32 = 0;
        let resolved_type = if unsafe { (api.get_type)(&mut vtype) } == 0 { vtype } else { api.install_vtype };
        let (strip_count, bus_count) = channel_counts(resolved_type);
        Snapshot { running: true, strip_count, bus_count, vtype: resolved_type }
    })
}

/// Lance l'édition de Voicemeeter détectée à l'installation (voir
/// `find_install`) — no-op silencieux si la connexion elle-même est
/// indisponible (voir `with_client`).
pub fn launch() {
    with_client(|api| unsafe {
        (api.run)(api.install_vtype);
    });
}

/// Lit un paramètre flottant (voir `VoicemeeterRemoteAPI.pdf`,
/// "Parameters List" pour les noms disponibles, ex. `"Bus[0].Gain"`) —
/// `None` si la connexion est indisponible OU si l'appel échoue (code
/// négatif, voir la doc de l'API : "0 means OK... error code if below
/// zero").
pub fn get_float(param: &str) -> Option<f32> {
    with_client(|api| {
        let name = CString::new(param).ok()?;
        let mut value: f32 = 0.0;
        let rep = unsafe { (api.get_float)(name.as_ptr(), &mut value) };
        (rep >= 0).then_some(value)
    })
    .flatten()
}

/// Lit un paramètre CHAÎNE (voir `VoicemeeterRemoteAPI.pdf`, ex.
/// `"Strip[0].Label"`/`"Bus[0].Label"` — le nom personnalisé qu'un
/// utilisateur donne à un canal depuis l'interface Voicemeeter, demande
/// explicite : "récupérer les noms personnalisés des canaux") — `None` dans
/// les mêmes cas que `get_float`, OU si la chaîne renvoyée est VIDE (un
/// canal jamais renommé n'a pas de label : `Some(String::new())` serait
/// indiscernable d'un vrai échec pour l'appelant, qui veut de toute façon
/// retomber sur un nom générique dans les deux cas — voir
/// `win32::popup_menu::render_voicemeeter`).
pub fn get_string(param: &str) -> Option<String> {
    with_client(|api| {
        let name = CString::new(param).ok()?;
        let mut buffer = [0i8; PARAMETER_STRING_BUFFER_LEN];
        let rep = unsafe { (api.get_string)(name.as_ptr(), buffer.as_mut_ptr()) };
        if rep < 0 {
            return None;
        }
        // ANSI (pas garanti UTF-8, voir la doc de l'API : "char *, pointer
        // on ANSI String") — `from_utf8_lossy` reste une approximation
        // raisonnable pour de simples noms de canaux tapés par l'utilisateur
        // (ASCII en pratique), pas une vraie conversion de page de code.
        let bytes: Vec<u8> = buffer.iter().take_while(|&&b| b != 0).map(|&b| b as u8).collect();
        let label = String::from_utf8_lossy(&bytes).trim().to_string();
        (!label.is_empty()).then_some(label)
    })
    .flatten()
}

/// Écrit un paramètre flottant — best-effort silencieux (voir `get_float`,
/// même raison qu'`win32::audio::set_output_volume_percent` : une écriture
/// individuelle qui échoue ne mérite pas d'interrompre l'utilisateur, le
/// prochain `snapshot()`/`get_float` reflétera simplement l'état réel).
pub fn set_float(param: &str, value: f32) {
    with_client(|api| {
        if let Ok(name) = CString::new(param) {
            unsafe {
                (api.set_float)(name.as_ptr(), value);
            }
        }
    });
}
