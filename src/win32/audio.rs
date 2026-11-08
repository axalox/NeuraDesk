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

use windows::core::{GUID, HRESULT, PCWSTR};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
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
