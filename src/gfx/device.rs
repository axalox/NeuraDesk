//! Création du device Direct3D 11 et de son contexte immédiat.

use windows::core::Result;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION,
};

#[cfg(debug_assertions)]
use windows::Win32::Graphics::Direct3D11::D3D11_CREATE_DEVICE_DEBUG;

pub struct GfxDevice {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
}

impl GfxDevice {
    pub fn new() -> Result<Self> {
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
            if let Ok(dev) = create_device(debug_flags) {
                return Ok(dev);
            }
        }

        create_device(D3D11_CREATE_DEVICE_BGRA_SUPPORT)
    }
}

fn create_device(flags: D3D11_CREATE_DEVICE_FLAG) -> Result<GfxDevice> {
    let feature_levels = [D3D_FEATURE_LEVEL_11_0];
    let mut device = None;
    let mut context = None;

    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
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
