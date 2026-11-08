//! Import d'une texture partagée D3D11 exposée par CEF (rendu accéléré
//! hors-écran, voir `win32::cef_app`) — voir
//! `docs/plans/cef-web-ui-migration.md` pour le contexte complet.
//!
//! `cef::osr_texture_import::SharedTextureHandle` (fourni par `cef-rs`)
//! importe DANS `wgpu` — délibérément PAS UTILISÉ ici : ça ajouterait un
//! second backend de rendu GPU parallèle à D3D11 pour rien.
//! `cef::AcceleratedPaintInfo::shared_texture_handle` est un `HANDLE` Win32
//! BRUT (confirmé en lisant `cef-rs/cef/src/bindings/*.rs` — vérifié dans
//! le spike de cette migration), importable directement dans N'IMPORTE QUEL
//! device D3D11 via `ID3D11Device1::OpenSharedResource1`.

use windows::core::{Interface, Result};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11Device1, ID3D11ShaderResourceView, ID3D11Texture2D, D3D11_BIND_SHADER_RESOURCE, D3D11_SUBRESOURCE_DATA,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_IMMUTABLE,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

/// Importe la texture partagée identifiée par `handle` dans `device` — DOIT
/// être le MÊME device D3D11 que celui utilisé pour composer le reste du
/// rendu (`gfx::device::GfxDevice`, partagé avec le dock et le flou), sans
/// quoi la texture importée ne serait liable à aucun contexte de rendu de
/// cette app. `handle` vient de `cef::AcceleratedPaintInfo` (module
/// `windows_sys`, PAS `windows` — voir `to_windows_handle` ci-dessous pour
/// la conversion) et reste valide tant que CEF n'a pas repeint par-dessus
/// (voir `win32::cef_app::on_accelerated_paint`, qui appelle ceci à CHAQUE
/// frame — pas de mise en cache de la texture importée elle-même d'une
/// frame à l'autre, CEF peut fournir un handle DIFFÉRENT si son swap chain
/// interne change de buffer).
pub fn import_shared_texture(device: &ID3D11Device, handle: HANDLE) -> Result<(ID3D11Texture2D, ID3D11ShaderResourceView)> {
    let device1: ID3D11Device1 = device.cast()?;
    let texture: ID3D11Texture2D = unsafe { device1.OpenSharedResource1(handle)? };
    let mut srv = None;
    unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut srv))? };
    Ok((texture, srv.expect("CreateShaderResourceView a réussi sans remplir `srv`")))
}

/// Convertit un `HANDLE` `windows_sys` (le type que `cef-dll-sys`/`cef`
/// utilisent, voir la doc de module) vers le `HANDLE` `windows` (le crate
/// que le reste de ce projet utilise) — même représentation binaire,
/// types Rust distincts, aucune conversion `From` fournie par aucun des
/// deux crates. Prend `isize` (PAS le type `windows_sys::...::HANDLE`
/// directement) pour ne pas avoir à ajouter `windows-sys` comme dépendance
/// DIRECTE de ce projet — `cef`/`cef-dll-sys` l'ont déjà en transitif, mais
/// Cargo n'expose pas les types d'une dépendance transitive à un crate qui
/// ne la liste pas lui-même. L'appelant (`win32::cef_app`) convertit le
/// `HANDLE` `windows_sys` reçu de CEF via `.0 as isize` avant d'appeler
/// ceci — les deux types sont juste une couche fine sur `isize`.
pub fn to_windows_handle(raw: isize) -> HANDLE {
    HANDLE(raw as *mut core::ffi::c_void)
}

/// Copie l'image LOGICIELLE que CEF remet à `on_paint` (BGRA, 4 octets par
/// pixel, lignes jointives, alpha prémultiplié — le même format que la texture
/// partagée) dans une texture D3D11 neuve.
///
/// ⛔ **C'EST LE SEUL RENDU D'UNE MACHINE SANS CARTE GRAPHIQUE** (une machine
/// virtuelle) : Chromium n'y a pas de texture partagée à offrir, il n'appelle
/// jamais `on_accelerated_paint`, et sans ceci chaque panneau restait VIDE —
/// la teinte seule, aucun texte.
pub fn texture_depuis_bgra(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    bgra: &[u8],
) -> Result<(ID3D11Texture2D, ID3D11ShaderResourceView)> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let data = D3D11_SUBRESOURCE_DATA { pSysMem: bgra.as_ptr() as *const _, SysMemPitch: width * 4, SysMemSlicePitch: 0 };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, Some(&data), Some(&mut texture))? };
    let texture = texture.expect("CreateTexture2D a réussi sans remplir `texture`");
    let mut srv = None;
    unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut srv))? };
    Ok((texture, srv.expect("CreateShaderResourceView a réussi sans remplir `srv`")))
}
