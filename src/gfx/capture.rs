//! Relecture du back buffer D3D11 vers un PNG sur disque — le SEUL moyen de
//! vérifier visuellement le rendu de cette application depuis l'extérieur du
//! processus : ses fenêtres (dock + les trois overlays `egui`) sont exclues
//! de toute capture d'écran externe (`SetWindowDisplayAffinity(...,
//! WDA_EXCLUDEFROMCAPTURE)`, voir `win32::window::Window::create_dock` et
//! `win32::egui_overlay::make_transparent_and_uncapturable`) — Outil
//! Capture, PrintScreen et Desktop Duplication API n'en voient rien, quel
//! que soit l'outil utilisé. Réservé au harnais de vérification déclenché
//! par les variables d'environnement `NEURADESK_CAPTURE*`/`NEURADESK_DEBUG_OPEN`
//! (voir `app::run`) — jamais appelé en usage normal.

use std::path::Path;

use windows::core::Result;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};

/// Copie `source` (le back buffer d'un swap chain, typiquement) vers une
/// texture "staging" lisible par le CPU puis l'écrit en PNG à `path` (crée
/// les dossiers manquants). `source` doit être au format
/// `DXGI_FORMAT_B8G8R8A8_UNORM` (le seul utilisé par les swap chains et
/// render targets de ce projet, voir `gfx::swapchain`/`gfx::blur`) — aucune
/// conversion de format n'est faite, seul l'ordre des canaux BGRA→RGBA est
/// corrigé.
///
/// L'alpha écrit dans le PNG est BRUT (prémultiplié, voir la doc de
/// `gfx::quad_renderer` et d'`egui-directx11`) — utile pour juger de
/// l'opacité elle-même (ex. Phase 4, transitions), mais se lit à tort comme
/// "sombre" là où le vrai rendu composé par DWM serait clair. Utiliser
/// [`dump_backbuffer_composited`] pour un rendu tel qu'un humain le verrait
/// sur le vrai bureau.
#[allow(dead_code)] // Phase 4 (transitions) : jugera l'opacité brute avant composition — voir docs/plans
pub fn dump_backbuffer(
    device: &ID3D11Device,
    ctx: &ID3D11DeviceContext,
    source: &ID3D11Texture2D,
    path: &Path,
) -> Result<()> {
    let (width, height, rgba) = capture_rgba(device, ctx, source)?;
    save_png(&rgba, width, height, path);
    Ok(())
}

/// Comme [`dump_backbuffer`], mais compose l'alpha prémultiplié sur un
/// damier gris (16px, la convention "transparence" habituelle) avant
/// d'écrire le PNG — pour juger du contraste réel tel qu'affiché sur le
/// bureau, sans se laisser tromper par une image où la transparence se lit
/// comme du noir.
pub fn dump_backbuffer_composited(
    device: &ID3D11Device,
    ctx: &ID3D11DeviceContext,
    source: &ID3D11Texture2D,
    path: &Path,
) -> Result<()> {
    let (width, height, mut rgba) = capture_rgba(device, ctx, source)?;
    composite_on_checkerboard(&mut rgba, width, height);
    save_png(&rgba, width, height, path);
    Ok(())
}

/// Copie `source` vers une texture staging et relit son contenu en RGBA8
/// (converti depuis le BGRA8 natif de la swap chain) — cœur commun de
/// [`dump_backbuffer`] et [`dump_backbuffer_composited`].
///
/// **`pub(crate)`** : sert aussi à relire une frame venue d'AILLEURS — une
/// capture `Windows.Graphics.Capture` de la barre des tâches (voir
/// `win32::tray_probe`, qui a mesuré que WGC accepte `Shell_TrayWnd` même
/// masquée). Le chemin GPU → processeur est le même, et il n'y a aucune raison
/// d'en écrire un second.
pub(crate) fn capture_rgba(
    device: &ID3D11Device,
    ctx: &ID3D11DeviceContext,
    source: &ID3D11Texture2D,
) -> Result<(u32, u32, Vec<u8>)> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { source.GetDesc(&mut desc) };

    let staging_desc = D3D11_TEXTURE2D_DESC {
        Width: desc.Width,
        Height: desc.Height,
        MipLevels: 1,
        ArraySize: 1,
        Format: desc.Format,
        SampleDesc: desc.SampleDesc,
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut staging = None;
    unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut staging))? };
    let staging = staging.unwrap();
    unsafe { ctx.CopyResource(&staging, source) };

    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
    let (width, height) = (desc.Width, desc.Height);
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    unsafe {
        let src = mapped.pData as *const u8;
        for y in 0..height {
            let row = std::slice::from_raw_parts(src.add((y * mapped.RowPitch) as usize), (width * 4) as usize);
            let dst = &mut rgba[(y * width * 4) as usize..((y + 1) * width * 4) as usize];
            for x in 0..width as usize {
                // Le back buffer est BGRA (`DXGI_FORMAT_B8G8R8A8_UNORM`,
                // voir `gfx::swapchain`) — `image` attend du RGBA.
                dst[x * 4] = row[x * 4 + 2];
                dst[x * 4 + 1] = row[x * 4 + 1];
                dst[x * 4 + 2] = row[x * 4];
                dst[x * 4 + 3] = row[x * 4 + 3];
            }
        }
        ctx.Unmap(&staging, 0);
    }
    Ok((width, height, rgba))
}

fn composite_on_checkerboard(rgba: &mut [u8], width: u32, height: u32) {
    const TILE: u32 = 16;
    const LIGHT: [u8; 3] = [200, 200, 200];
    const DARK: [u8; 3] = [150, 150, 150];
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            // Alpha prémultiplié : `rgba[i+c]` vaut déjà `couleur * a`, donc
            // le compositing "over" s'écrit sans division par `a`.
            let a = rgba[i + 3] as f32 / 255.0;
            let checker = if ((x / TILE) + (y / TILE)) % 2 == 0 { LIGHT } else { DARK };
            for c in 0..3 {
                let fg = rgba[i + c] as f32;
                let bg = checker[c] as f32;
                rgba[i + c] = (fg + bg * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
            }
            rgba[i + 3] = 255;
        }
    }
}

fn save_png(rgba: &[u8], width: u32, height: u32, path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(err) = image::save_buffer(path, rgba, width, height, image::ColorType::Rgba8) {
        eprintln!("[neuradesk][capture] échec écriture {}: {err}", path.display());
    } else {
        eprintln!("[neuradesk][capture] écrit {}", path.display());
    }
}
