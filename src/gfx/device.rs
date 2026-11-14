//! Création du device Direct3D 11 et de son contexte immédiat.

use windows::core::Result;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::IDXGIAdapter;

#[cfg(debug_assertions)]
use windows::Win32::Graphics::Direct3D11::D3D11_CREATE_DEVICE_DEBUG;

pub struct GfxDevice {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
}

impl GfxDevice {
    /// Sur la carte choisie dans les Réglages (voir `gfx::carte_graphique`),
    /// sinon sur celle que Windows désigne. Si la carte choisie refuse le
    /// device, on retombe sur le choix de Windows plutôt que de ne pas
    /// démarrer — ⚠️ Chromium, lui, garde la carte choisie : ses panneaux
    /// peuvent alors rester vides, d'où la trace.
    pub fn new() -> Result<Self> {
        if let Some(adaptateur) = super::carte_graphique::adaptateur_choisi() {
            let adaptateur: IDXGIAdapter = windows::core::Interface::cast(&adaptateur)?;
            match Self::sur(Some(&adaptateur)) {
                Ok(dev) => return Ok(dev),
                Err(e) => crate::diag!("[gpu] la carte choisie refuse le device ({e}) : Windows choisit"),
            }
        }
        Self::sur(None)
    }

    fn sur(adaptateur: Option<&IDXGIAdapter>) -> Result<Self> {
        // BGRA_SUPPORT est requis pour un swap chain DXGI_FORMAT_B8G8R8A8_UNORM
        // (le format qu'attend la composition DWM pour un canal alpha).
        #[cfg(debug_assertions)]
        {
            // Tente d'abord avec la couche de debug D3D11 (utile pour les
            // diagnostics en dev). Elle nécessite le composant optionnel
            // Windows "Graphics Tools" ; s'il est absent, D3D11CreateDevice
            // renvoie DXGI_ERROR_SDK_COMPONENT_MISSING - dans ce cas on
            // retombe silencieusement sur le device sans debug layer.
            let debug_flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_DEBUG;
            if let Ok(dev) = create_device(adaptateur, debug_flags) {
                return Ok(dev);
            }
        }

        create_device(adaptateur, D3D11_CREATE_DEVICE_BGRA_SUPPORT)
    }
}

fn create_device(adaptateur: Option<&IDXGIAdapter>, flags: D3D11_CREATE_DEVICE_FLAG) -> Result<GfxDevice> {
    let feature_levels = [D3D_FEATURE_LEVEL_11_0];
    let mut device = None;
    let mut context = None;
    // Un adaptateur explicite EXIGE `D3D_DRIVER_TYPE_UNKNOWN` (`E_INVALIDARG`
    // sinon) ; sans adaptateur, `HARDWARE` laisse Windows choisir.
    let pilote: D3D_DRIVER_TYPE = if adaptateur.is_some() { D3D_DRIVER_TYPE_UNKNOWN } else { D3D_DRIVER_TYPE_HARDWARE };

    unsafe {
        D3D11CreateDevice(
            adaptateur,
            pilote,
            HMODULE::default(),
            flags,
            Some(&feature_levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }

    Ok(GfxDevice {
        device: device.expect("D3D11CreateDevice a réussi sans remplir `device`"),
        context: context.expect("D3D11CreateDevice a réussi sans remplir `context`"),
    })
}
