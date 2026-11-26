// Noms de méthode COM (PascalCase, ex. `SetDefaultEndpoint`) pour rester
// cohérent avec la convention Win32 utilisée partout ailleurs dans ce
// projet — voir `IPolicyConfig`.
#![allow(non_snake_case)]

//! Contrôle des périphériques audio principaux (sortie/entrée) — façon
//! volet volume/son personnalisé de Seelen UI, à la place du volet natif
//! de Windows. Volume/muet via l'API Core Audio documentée
//! (`IAudioEndpointVolume`) ; bascule du périphérique par défaut via
//! `IPolicyConfig`, une interface COM **non documentée** par Microsoft
//! (aucune API publique n'existe pour changer le périphérique par défaut,
//! contrairement à la lecture qui est entièrement documentée) — même
//! statut que les ordinaux `uxtheme.dll` (voir `win32::theme`) ou la clé de
//! registre des bureaux virtuels (voir `win32::desktops`) : stable depuis
//! Windows 7, largement exploitée par des outils connus (NirSoft
//! SoundVolumeView, EarTrumpet, audioswitch...), mais non garantie par
//! Microsoft. Best-effort partout : une lecture qui échoue retourne `None`
//! plutôt que de planter.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use windows::core::{GUID, HRESULT, Interface, PCWSTR};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, AudioSessionStateActive, AudioSessionStateExpired, IAudioSessionControl2,
    IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, STGM_READ};
use windows::Win32::Foundation::PROPERTYKEY;

/// `PKEY_Device_FriendlyName` (voir `win32::Devices::FunctionDiscovery`
/// dans les bindings Windows — reconstitué ici pour éviter d'activer toute
/// la fonctionnalité `Win32_Devices_FunctionDiscovery` pour une seule
/// constante).
const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 14,
};

/// Un périphérique audio (sortie ou entrée) : son identifiant opaque
/// (nécessaire pour le sélectionner comme périphérique par défaut, voir
/// `set_default_output`/`set_default_input`) et son nom lisible.
#[derive(Clone)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
}

fn enumerator() -> Option<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() }
}

fn device_name(device: &IMMDevice) -> String {
    unsafe {
        let Ok(store) = device.OpenPropertyStore(STGM_READ) else { return String::from("(inconnu)") };
        let Ok(value) = store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME) else { return String::from("(inconnu)") };
        let name = value.to_string();
        if name.is_empty() {
            String::from("(inconnu)")
        } else {
            name
        }
    }
}

fn device_id(device: &IMMDevice) -> Option<String> {
    unsafe {
        let pwstr = device.GetId().ok()?;
        let id = pwstr.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(pwstr.0 as *const _));
        id
    }
}

fn to_audio_device(device: IMMDevice) -> AudioDevice {
    AudioDevice { name: device_name(&device), id: device_id(&device).unwrap_or_default() }
}

/// Périphérique de sortie par défaut (voir `eRender`/`eConsole` — le rôle
/// "console", celui de la lecture audio générale). `None` si aucun
/// périphérique de sortie actif n'est trouvé.
pub fn default_output() -> Option<AudioDevice> {
    let device = unsafe { enumerator()?.GetDefaultAudioEndpoint(eRender, eConsole).ok()? };
    Some(to_audio_device(device))
}

/// Périphérique d'entrée (microphone) par défaut.
pub fn default_input() -> Option<AudioDevice> {
    let device = unsafe { enumerator()?.GetDefaultAudioEndpoint(eCapture, eConsole).ok()? };
    Some(to_audio_device(device))
}

fn list_endpoints(flow: windows::Win32::Media::Audio::EDataFlow) -> Vec<AudioDevice> {
    unsafe {
        let Some(enumerator) = enumerator() else { return Vec::new() };
        let Ok(collection) = enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) else { return Vec::new() };
        let count = collection.GetCount().unwrap_or(0);
        (0..count)
            .filter_map(|i| collection.Item(i).ok())
            .map(to_audio_device)
            .collect()
    }
}

/// Tous les périphériques de sortie actuellement actifs (branchés/activés),
/// dans l'ordre rapporté par Windows.
pub fn list_outputs() -> Vec<AudioDevice> {
    list_endpoints(eRender)
}

/// Tous les périphériques d'entrée actuellement actifs.
pub fn list_inputs() -> Vec<AudioDevice> {
    list_endpoints(eCapture)
}

// ---------------------------------------------------------------------------
// QUI TIENT UNE SESSION DE CAPTURE OUVERTE ? — ESSAI, ET REFUSÉ COMME SIGNAL
// ---------------------------------------------------------------------------
//
// ⛔ **L'INDICATEUR « MICRO » NE SE SERT PAS DE CE RELEVÉ, ET C'EST MESURÉ.**
// L'idée paraissait la plus directe : les sessions WASAPI des périphériques
// d'ENTRÉE disent si une application tire du son du micro maintenant, sans
// croire une déclaration d'application sur parole.
//
// Relevé le 2026-11-26 sur cette machine (`essai_reel_qui_capture_le_micro`) :
// **cinq** sessions de capture à l'état `Active` alors que personne n'enregistre —
// QUATRE pour « VB-AUDIO Virtual Audio Device Mixing Console Application »
// (Voicemeeter, qui garde ses entrées ouvertes pour router le son) et une pour
// Voicemod. Les moteurs audio virtuels écoutent en permanence : un voyant bâti
// sur cet état resterait ALLUMÉ sans arrêt, et ne signalerait donc plus rien.
//
// Le registre `CapabilityAccessManager` (voir `win32::capteurs_medias`) dit, lui,
// « cette application utilise le micro depuis tel instant » — la même source que
// l'indicateur de Windows. Voicemeeter y figure avec `LastUsedTimeStop` POSTÉRIEUR
// à son début, donc comme une utilisation TERMINÉE.
//
// Gardé comme essai seulement : c'est lui qui permet de dire QUI tient une
// capture ouverte, et la mesure ci-dessus ne se refait pas de mémoire.
#[cfg(test)]
fn pids_de_capture_actifs() -> Vec<u32> {
    let mut pids = Vec::new();
    unsafe {
        let Some(enumerateur) = enumerator() else { return pids };
        let Ok(collection) = enumerateur.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) else { return pids };
        for i in 0..collection.GetCount().unwrap_or(0) {
            let Ok(device) = collection.Item(i) else { continue };
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
            let Ok(liste) = manager.GetSessionEnumerator() else { continue };
            for j in 0..liste.GetCount().unwrap_or(0) {
                let Ok(controle) = liste.GetSession(j) else { continue };
                if controle.GetState().map(|etat| etat == AudioSessionStateActive).unwrap_or(false) {
                    // Pas de repli sur 0 : PID 0 est « sons du système », jamais
                    // une session de capture — une lecture qui échoue est écartée.
                    if let Ok(pid) = controle.cast::<IAudioSessionControl2>().and_then(|c| c.GetProcessId()) {
                        pids.push(pid);
                    }
                }
            }
        }
    }
    pids
}

// ---------------------------------------------------------------------------
// Mélangeur PAR APPLICATION (WASAPI, API toutes documentées)
// ---------------------------------------------------------------------------
//
// Une « session audio » appartient à un processus et possède son propre
// volume et son propre muet (`ISimpleAudioVolume`), indépendants du volume du
// périphérique. C'est ce que montre le mélangeur de Windows (et EarTrumpet) :
// baisser Spotify sans baisser Chrome.
//
// ⭐ **UNE LIGNE PAR PROCESSUS** : Chrome ouvre une session par onglet qui
// joue ; les afficher séparément donnerait « Chrome » cinq fois. On regroupe
// donc par PID, le volume affiché est celui de la PREMIÈRE session du groupe,
// et toute action s'applique à TOUTES (voir `avec_les_sessions`).
//
// PID 0 = « Sons du système » (le bip de Windows, les alertes) : une ligne à
// part, contrôlable comme les autres.

/// Une application qui a une session audio sur la sortie par défaut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioApp {
    /// PID du processus ; **0** pour les « Sons du système ».
    pub pid: u32,
    /// Nom lisible de l'application (voir `nom_de_l_app`).
    pub name: String,
    /// Volume de la première session du groupe, 0..100.
    pub volume: u8,
    /// `true` seulement si TOUTES les sessions du groupe sont muettes — une
    /// seule non muette suffit à entendre l'app.
    pub muted: bool,
    /// Au moins une session joue en ce moment.
    pub active: bool,
}

/// Une session audio brute, avant regroupement par processus.
#[derive(Clone, Copy, Debug)]
struct SessionBrute {
    pid: u32,
    volume: f32,
    muted: bool,
    active: bool,
}

thread_local! {
    /// Nom lisible par CHEMIN d'exécutable : lire le bloc `VERSIONINFO` est un
    /// accès disque, inutile de le refaire à chaque rafraîchissement (le popup
    /// Son relit les sessions toutes les 500 ms tant qu'il est ouvert).
    static NOMS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Le gestionnaire de sessions de la sortie par défaut.
fn session_manager() -> Option<IAudioSessionManager2> {
    unsafe {
        let device = enumerator()?.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None).ok()
    }
}

/// Toutes les sessions de la sortie par défaut, une par session (pas encore
/// regroupées). Les sessions EXPIRÉES sont écartées : leur processus n'existe
/// plus, elles ne servent à rien.
fn sessions_brutes() -> Vec<SessionBrute> {
    unsafe {
        let Some(manager) = session_manager() else { return Vec::new() };
        let Ok(liste) = manager.GetSessionEnumerator() else { return Vec::new() };
        (0..liste.GetCount().unwrap_or(0))
            .filter_map(|i| {
                let controle = liste.GetSession(i).ok()?;
                let etat = controle.GetState().ok()?;
                if etat == AudioSessionStateExpired {
                    return None;
                }
                let volume = controle.cast::<ISimpleAudioVolume>().ok()?;
                let niveau = volume.GetMasterVolume().ok()?;
                let muet = volume.GetMute().map(|m| m.as_bool()).unwrap_or(false);
                // ⛔ Pas de repli sur 0 : PID 0 est une VRAIE valeur (sons du
                // système), donc une lecture qui échoue ne doit pas se
                // déguiser en sons du système — on écarte la session.
                let pid = controle.cast::<IAudioSessionControl2>().ok().and_then(|c| c.GetProcessId().ok())?;
                Some(SessionBrute { pid, volume: niveau, muted: muet, active: etat == AudioSessionStateActive })
            })
            .collect()
    }
}

/// Regroupe les sessions par processus. **PURE** — `nom` est passé en
/// paramètre pour que les essais n'aient aucun appel système à faire.
fn regrouper(sessions: &[SessionBrute], nom: impl Fn(u32) -> String) -> Vec<AudioApp> {
    let mut apps: Vec<AudioApp> = Vec::new();
    for session in sessions {
        match apps.iter_mut().find(|a| a.pid == session.pid) {
            Some(app) => {
                app.muted &= session.muted;
                app.active |= session.active;
            }
            None => apps.push(AudioApp {
                pid: session.pid,
                name: nom(session.pid),
                volume: (session.volume.clamp(0.0, 1.0) * 100.0).round() as u8,
                muted: session.muted,
                active: session.active,
            }),
        }
    }
    apps
}

/// Les apps qui jouent EN CE MOMENT d'abord, puis par nom. **PURE.**
fn trier(apps: &mut [AudioApp]) {
    apps.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.pid.cmp(&b.pid))
    });
}

/// Nom lisible d'un processus : `FileDescription` de son exécutable, sinon
/// son nom de fichier sans `.exe`, sinon « Application ».
fn nom_de_l_app(pid: u32) -> String {
    if pid == 0 {
        return "Sons du système".to_string();
    }
    let Some((fichier, chemin)) = crate::win32::app_identity::process_image_name(pid) else {
        return "Application".to_string();
    };
    if let Some(nom) = NOMS.with(|n| n.borrow().get(&chemin).cloned()) {
        return nom;
    }
    let nom = crate::win32::exe_metadata::description(Path::new(&chemin))
        .unwrap_or_else(|| fichier.trim_end_matches(".exe").to_string());
    NOMS.with(|n| {
        n.borrow_mut().insert(chemin, nom.clone());
    });
    nom
}

/// **Les applications qui ont une session sur la sortie par défaut**, triées.
/// La sortie par défaut seulement : c'est celle que l'utilisateur entend, et
/// c'est déjà celle que contrôle le reste de ce popup.
pub fn apps_on_default_output() -> Vec<AudioApp> {
    let mut apps = regrouper(&sessions_brutes(), nom_de_l_app);
    trier(&mut apps);
    apps
}

/// Applique `action` à toutes les sessions de `pid` sur la sortie par défaut.
fn avec_les_sessions(pid: u32, mut action: impl FnMut(&ISimpleAudioVolume)) {
    unsafe {
        let Some(manager) = session_manager() else { return };
        let Ok(liste) = manager.GetSessionEnumerator() else { return };
        for i in 0..liste.GetCount().unwrap_or(0) {
            let Ok(controle) = liste.GetSession(i) else { continue };
            let Ok(etat) = controle.GetState() else { continue };
            if etat == AudioSessionStateExpired {
                continue;
            }
            // Un PID ILLISIBLE n'est pas le PID 0 : on n'y touche pas (sinon
            // « Sons du système » contrôlerait une session inconnue).
            let proprietaire = controle.cast::<IAudioSessionControl2>().ok().and_then(|c| c.GetProcessId().ok());
            if proprietaire != Some(pid) {
                continue;
            }
            if let Ok(volume) = controle.cast::<ISimpleAudioVolume>() {
                action(&volume);
            }
        }
    }
}

/// Fixe le volume de TOUTES les sessions de `pid` (0..100).
pub fn set_app_volume_percent(pid: u32, percent: u8) {
    let niveau = percent.min(100) as f32 / 100.0;
    avec_les_sessions(pid, |volume| unsafe {
        let _ = volume.SetMasterVolume(niveau, std::ptr::null());
    });
}

/// L'état « muet » d'un GROUPE de sessions : vrai seulement si TOUTES le sont
/// (et faux s'il n'y en a aucune). **PURE** — c'est la règle que `regrouper`
/// applique à l'affichage ET celle que `toggle_app_mute` applique à sa cible.
fn groupe_muet(muets: &[bool]) -> bool {
    !muets.is_empty() && muets.iter().all(|muet| *muet)
}

/// Bascule le muet de TOUTES les sessions de `pid`, d'après ce que le groupe
/// AFFICHE — le ET de `regrouper`, pas la première session.
///
/// ⛔ **LA PREMIÈRE SESSION NE DONNE PAS L'ÉTAT AFFICHÉ.** WASAPI crée les
/// nouvelles sessions NON muettes : une app muette qui ouvre un second flux
/// affiche « Couper » (le groupe n'est plus muet) alors que sa première
/// session l'est encore. Lire celle-là ferait « réactiver » ce qui est déjà
/// audible, et il faudrait un second clic — exactement le symptôme « mon clic
/// ne fait rien » déjà documenté sur ce popup.
pub fn toggle_app_mute(pid: u32) {
    let mut muets = Vec::new();
    avec_les_sessions(pid, |volume| {
        if let Ok(muet) = unsafe { volume.GetMute() } {
            muets.push(muet.as_bool());
        }
    });
    let cible = !groupe_muet(&muets);
    avec_les_sessions(pid, |volume| unsafe {
        let _ = volume.SetMute(cible, std::ptr::null());
    });
}

fn endpoint_volume(flow: windows::Win32::Media::Audio::EDataFlow) -> Option<IAudioEndpointVolume> {
    unsafe {
        let device = enumerator()?.GetDefaultAudioEndpoint(flow, eConsole).ok()?;
        device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
    }
}

/// Volume actuel de la sortie par défaut, en pourcentage (0..100).
pub fn output_volume_percent() -> Option<u8> {
    let level = unsafe { endpoint_volume(eRender)?.GetMasterVolumeLevelScalar().ok()? };
    Some((level.clamp(0.0, 1.0) * 100.0).round() as u8)
}

/// Fixe le volume de la sortie par défaut. Best-effort, sans effet
/// silencieux en cas d'échec (périphérique déconnecté entre-temps, etc.).
pub fn set_output_volume_percent(percent: u8) {
    unsafe {
        if let Some(volume) = endpoint_volume(eRender) {
            let _ = volume.SetMasterVolumeLevelScalar(percent.min(100) as f32 / 100.0, std::ptr::null());
        }
    }
}

/// `true` si la sortie par défaut est actuellement coupée.
pub fn output_muted() -> Option<bool> {
    let muted = unsafe { endpoint_volume(eRender)?.GetMute().ok()? };
    Some(muted.as_bool())
}

/// Bascule l'état muet de la sortie par défaut.
pub fn toggle_output_mute() {
    unsafe {
        if let Some(volume) = endpoint_volume(eRender) {
            if let Ok(muted) = volume.GetMute() {
                let _ = volume.SetMute(!muted.as_bool(), std::ptr::null());
            }
        }
    }
}

/// `true` si l'entrée par défaut (microphone) est actuellement coupée.
pub fn input_muted() -> Option<bool> {
    let muted = unsafe { endpoint_volume(eCapture)?.GetMute().ok()? };
    Some(muted.as_bool())
}

/// Bascule l'état muet de l'entrée par défaut.
pub fn toggle_input_mute() {
    unsafe {
        if let Some(volume) = endpoint_volume(eCapture) {
            if let Ok(muted) = volume.GetMute() {
                let _ = volume.SetMute(!muted.as_bool(), std::ptr::null());
            }
        }
    }
}

/// Change le périphérique de sortie par défaut (voir doc de module :
/// `IPolicyConfig`, non documenté). `device_id` vient de `list_outputs`.
/// Best-effort silencieux en cas d'échec.
pub fn set_default_output(device_id: &str) {
    set_default_endpoint(device_id);
}

/// Change le périphérique d'entrée par défaut. Mêmes garanties que
/// `set_default_output`.
pub fn set_default_input(device_id: &str) {
    set_default_endpoint(device_id);
}

/// La page « Son » des Paramètres — URI `ms-settings:` documenté par Microsoft
/// (« Launch the Windows Settings app »).
const PARAMETRES_URI: &str = "ms-settings:sound";

/// Ouvre les paramètres de son de Windows — DEMANDE EXPLICITE (2026-09-26) :
/// « qu'un clic droit sur l'applet de son ouvre les paramètres de son de
/// windows ». Même voie que `network::ouvrir_parametres`.
pub fn ouvrir_parametres() {
    crate::diag!("son : ouverture de {PARAMETRES_URI}");
    if let Err(err) = std::process::Command::new("explorer.exe").arg(PARAMETRES_URI).spawn() {
        crate::diag!("son : lancement impossible — {err}");
    }
}

fn set_default_endpoint(device_id: &str) {
    unsafe {
        let Ok(policy) = CoCreateInstance::<_, IPolicyConfig>(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) else {
            eprintln!("[neuradesk] changement de périphérique par défaut indisponible (IPolicyConfig introuvable)");
            return;
        };
        let wide: Vec<u16> = device_id.encode_utf16().chain(std::iter::once(0)).collect();
        let id = PCWSTR(wide.as_ptr());
        // Les trois rôles sont mis à jour pour que le périphérique soit
        // réellement "par défaut" partout (Windows distingue console,
        // multimédia et communications, mais ne les expose pas
        // séparément dans son propre réglage rapide — on les aligne tous).
        let _ = policy.SetDefaultEndpoint(id, eConsole);
        let _ = policy.SetDefaultEndpoint(id, windows::Win32::Media::Audio::eMultimedia);
        let _ = policy.SetDefaultEndpoint(id, windows::Win32::Media::Audio::eCommunications);
    }
}

/// CLSID de `CPolicyConfigClient`, stable depuis Windows 7 (voir doc de
/// module).
const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

/// Interface `IPolicyConfig` reconstituée manuellement (non fournie par les
/// bindings `windows-rs`, qui ne couvrent que les API documentées) — voir
/// doc de module. Toutes les méthodes précédant `SetDefaultEndpoint`
/// doivent être déclarées dans le bon ordre même si on ne s'en sert pas :
/// l'emplacement dans la vtable dépend de l'ordre de déclaration.
#[windows::core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: windows::core::IUnknown {
    fn GetMixFormat(&self, device_id: PCWSTR, format: *mut *mut core::ffi::c_void) -> HRESULT;
    fn GetDeviceFormat(&self, device_id: PCWSTR, default: i32, format: *mut *mut core::ffi::c_void) -> HRESULT;
    fn ResetDeviceFormat(&self, device_id: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(
        &self,
        device_id: PCWSTR,
        endpoint_format: *mut core::ffi::c_void,
        mix_format: *mut core::ffi::c_void,
    ) -> HRESULT;
    fn GetProcessingPeriod(
        &self,
        device_id: PCWSTR,
        default: i32,
        default_period: *mut i64,
        min_period: *mut i64,
    ) -> HRESULT;
    fn SetProcessingPeriod(&self, device_id: PCWSTR, period: *mut i64) -> HRESULT;
    fn GetShareMode(&self, device_id: PCWSTR, mode: *mut core::ffi::c_void) -> HRESULT;
    fn SetShareMode(&self, device_id: PCWSTR, mode: *mut core::ffi::c_void) -> HRESULT;
    fn GetPropertyValue(
        &self,
        device_id: PCWSTR,
        key: *const PROPERTYKEY,
        value: *mut core::ffi::c_void,
    ) -> HRESULT;
    fn SetPropertyValue(
        &self,
        device_id: PCWSTR,
        key: *const PROPERTYKEY,
        value: *const core::ffi::c_void,
    ) -> HRESULT;
    fn SetDefaultEndpoint(&self, device_id: PCWSTR, role: windows::Win32::Media::Audio::ERole) -> HRESULT;
    fn SetEndpointVisibility(&self, device_id: PCWSTR, visible: i32) -> HRESULT;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(pid: u32, volume: f32, muted: bool, active: bool) -> SessionBrute {
        SessionBrute { pid, volume, muted, active }
    }

    /// ⭐ Le fait qui justifie tout le module : Chrome ouvre une session par
    /// onglet qui joue, et le mélangeur doit afficher UNE ligne.
    #[test]
    fn deux_sessions_du_meme_processus_font_une_seule_ligne() {
        let apps = regrouper(
            &[session(42, 0.8, false, true), session(42, 0.3, false, false)],
            |_| "Google Chrome".to_string(),
        );
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].pid, 42);
        assert_eq!(apps[0].volume, 80);
        assert!(apps[0].active, "une seule session qui joue suffit");
    }

    /// Une app n'est muette que si TOUTES ses sessions le sont — sinon on
    /// afficherait « muet » sur une app qu'on entend.
    #[test]
    fn le_groupe_est_muet_seulement_si_toutes_les_sessions_le_sont() {
        let melange = regrouper(&[session(42, 0.5, true, false), session(42, 0.5, false, false)], |_| "Chrome".into());
        assert!(!melange[0].muted);
        let toutes = regrouper(&[session(42, 0.5, true, false), session(42, 0.5, true, false)], |_| "Chrome".into());
        assert!(toutes[0].muted);
    }

    /// Les sons du système (PID 0) forment une ligne, pas une par session.
    #[test]
    fn le_son_du_systeme_est_une_seule_ligne() {
        let apps = regrouper(&[session(0, 1.0, false, true), session(0, 1.0, false, true)], |_| {
            "Sons du système".to_string()
        });
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].pid, 0);
    }

    /// ⛔ **L'ÉTAT AFFICHÉ ET LA CIBLE DU BOUTON SONT LA MÊME RÈGLE.** Une app
    /// muette qui ouvre un second flux non muet affiche « Couper » : le clic
    /// doit alors COUPER, pas « réactiver » la première session — sans quoi il
    /// ne fait rien de perceptible et il faut cliquer deux fois.
    #[test]
    fn la_cible_du_muet_suit_l_etat_affiche() {
        assert!(groupe_muet(&[true, true]));
        assert!(!groupe_muet(&[true, false]), "une session audible rend le groupe audible");
        assert!(!groupe_muet(&[]), "aucune session lisible n'est pas « muet »");
    }

    /// Un volume hors 0..1 (pilote exotique, session en cours de changement)
    /// est borné au lieu d'être rendu tel quel au popup.
    #[test]
    fn un_volume_hors_bornes_est_borne() {
        assert_eq!(regrouper(&[session(1, 1.5, false, false)], |_| "A".into())[0].volume, 100);
        assert_eq!(regrouper(&[session(1, -0.2, false, false)], |_| "A".into())[0].volume, 0);
    }

    /// ⭐ **SUR LA VRAIE SORTIE** — à lancer à la main (`--ignored`), avec de
    /// l'audio qui joue : affiche ce que le mélangeur lirait. COM doit être
    /// initialisé sur le fil de l'essai (l'application le fait au démarrage,
    /// pas le binaire de test).
    #[test]
    #[ignore]
    fn essai_reel_les_applications_de_la_sortie_par_defaut() {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
        let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let apps = apps_on_default_output();
        for app in &apps {
            println!("pid {:>6}  {:>3}%  {}  {}{}", app.pid, app.volume, if app.muted { "MUET" } else { "    " }, if app.active { "▶ " } else { "  " }, app.name);
        }
        if init.is_ok() {
            unsafe { CoUninitialize() };
        }
        assert!(apps.iter().all(|a| a.volume <= 100 && !a.name.is_empty()), "{apps:?}");
    }

    /// ⚠️ **QUI CAPTURE LE MICRO ?** — à lancer à la main (`--ignored`). Un
    /// indicateur allumé en permanence ne se diagnostique qu'en NOMMANT la
    /// source : cet essai dit quels processus tiennent une session de capture
    /// active, et combien de temps la lecture prend.
    ///
    /// Relevé le 2026-11-26 sur cette machine : `micro=true` sans application
    /// visible en train d'enregistrer — c'est le relevé qui l'a signalé.
    #[test]
    #[ignore]
    fn essai_reel_qui_capture_le_micro() {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
        let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let debut = std::time::Instant::now();
        let pids = pids_de_capture_actifs();
        let duree = debut.elapsed();
        for pid in &pids {
            println!("[capture] pid {pid:>6}  {}", nom_de_l_app(*pid));
        }
        println!("[capture] {} session(s) active(s), relevé en {:.2} ms", pids.len(), duree.as_secs_f64() * 1000.0);
        if init.is_ok() {
            unsafe { CoUninitialize() };
        }
    }

    /// Les apps qui jouent passent devant ; le reste est trié par nom, et le
    /// PID départage deux noms identiques (deux `java.exe` par exemple).
    #[test]
    fn les_apps_actives_passent_devant_puis_le_nom() {
        let mut apps = regrouper(
            &[session(3, 0.5, false, false), session(2, 0.5, false, true), session(1, 0.5, false, false)],
            |pid| format!("app{pid}"),
        );
        trier(&mut apps);
        assert_eq!(apps.iter().map(|a| a.pid).collect::<Vec<_>>(), vec![2, 1, 3]);

        let mut meme_nom = regrouper(&[session(9, 0.5, false, false), session(7, 0.5, false, false)], |_| "Zoom".into());
        trier(&mut meme_nom);
        assert_eq!(meme_nom.iter().map(|a| a.pid).collect::<Vec<_>>(), vec![7, 9]);
    }
}
