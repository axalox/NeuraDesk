//! Compilation HLSL à la volée (`D3DCompile`), partagée par tous les
//! petits pipelines de rendu du projet (`gfx::quad_renderer`,
//! `gfx::blur`) — un seul endroit pour ce détail Win32 plutôt qu'une copie
//! par pipeline.

use std::ffi::CString;

use windows::core::{Result, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::ID3DBlob;

pub(crate) fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    unsafe {
        let ptr = blob.GetBufferPointer() as *const u8;
        let len = blob.GetBufferSize();
        std::slice::from_raw_parts(ptr, len)
    }
}

thread_local! {
    /// Blobs déjà compilés, par (adresse et longueur de la source, point
    /// d'entrée, cible). Les sources sont des `&'static str` : l'adresse suffit
    /// à les distinguer. Les docks secondaires recréent un `QuadRenderer` à
    /// CHAQUE frame (`DockInstance::render`) — sans ce cache, autant de
    /// `D3DCompile` complets par seconde.
    static CACHE: std::cell::RefCell<std::collections::HashMap<(usize, usize, String, String), ID3DBlob>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

pub(crate) fn compile_shader(source: &str, entry: &str, target: &str) -> Result<ID3DBlob> {
    let key = (source.as_ptr() as usize, source.len(), entry.to_string(), target.to_string());
    if let Some(blob) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(blob);
    }
    let blob = compile_uncached(source, entry, target)?;
    CACHE.with(|c| c.borrow_mut().insert(key, blob.clone()));
    Ok(blob)
}

fn compile_uncached(source: &str, entry: &str, target: &str) -> Result<ID3DBlob> {
    compiler(source, entry, target).map_err(|(erreur, message)| {
        eprintln!("[neuradesk] échec de compilation du shader '{entry}':\n{message}");
        erreur
    })
}

/// **COMPILE SANS CACHE, ET REND LE MESSAGE DU COMPILATEUR** — pour une source
/// écrite par l'utilisateur (voir `gfx::shader_utilisateur`), dont l'erreur doit
/// être MONTRÉE : l'application est lancée détachée, `eprintln!` ne va nulle part.
///
/// Rend les OCTETS du blob : `ID3DBlob` n'est pas `Send`, et la compilation se
/// fait sur un fil à part.
pub(crate) fn compiler_pour_l_utilisateur(source: &str, entry: &str, target: &str) -> std::result::Result<Vec<u8>, String> {
    compiler(source, entry, target)
        .map(|blob| blob_bytes(&blob).to_vec())
        .map_err(|(erreur, message)| if message.is_empty() { erreur.message() } else { message })
}

fn compiler(source: &str, entry: &str, target: &str) -> std::result::Result<ID3DBlob, (windows::core::Error, String)> {
    let entry_c = CString::new(entry).unwrap();
    let target_c = CString::new(target).unwrap();
    let mut blob = None;
    let mut errors = None;

    let result = unsafe {
        D3DCompile(
            source.as_ptr() as *const _,
            source.len(),
            None,
            None,
            None,
            PCSTR(entry_c.as_ptr() as *const u8),
            PCSTR(target_c.as_ptr() as *const u8),
            0,
            0,
            &mut blob,
            Some(&mut errors),
        )
    };

    if let Err(e) = result {
        let msg = errors
            .map(|err_blob| String::from_utf8_lossy(blob_bytes(&err_blob)).trim_end_matches('\0').to_string())
            .unwrap_or_default();
        return Err((e, msg));
    }

    Ok(blob.expect("D3DCompile a réussi sans produire de blob"))
}
