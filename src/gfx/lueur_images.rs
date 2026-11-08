//! ⭐ **LES IMAGES DES LUEURS** — DEMANDE EXPLICITE (2026-10-14) : « pouvoir
//! importer des images à utiliser comme lueurs ». Une lueur de figure
//! `Figure::Image` dessine une image importée à la place d'une forme : posée dans
//! son rectangle (demi-largeur `longueur`, demi-hauteur `rayon`), tournée de son
//! angle, floutée par son `flou`, et mêlée aux fonds selon son ALPHA et son
//! intensité.
//!
//! **LA BIBLIOTHÈQUE** : `%APPDATA%\NeuraDesk\lueurs_images\<id> <nom>.png`, où
//! `<id>` est une empreinte (8 chiffres hexadécimaux) du fichier d'origine — une
//! même image importée deux fois ne se duplique pas — et `<nom>` le nom du
//! fichier d'origine. Chaque image y est redimensionnée à `COTE`×`COTE` : le
//! rectangle de la lueur donne les proportions (voir `importer`, qui les rend).
//!
//! **SUR LE GPU** : UN `Texture2DArray` (registre `t1`), une tranche par image
//! UTILISÉE par la scène (voir `gfx::lueur::images_utilisees`), avec toute sa
//! chaîne de mips — c'est elle qui fait le FLOU, pour rien : le shader lit un
//! niveau d'autant plus petit que le flou est grand. L'alpha est PRÉMULTIPLIÉ
//! avant de réduire, sans quoi les bords transparents (RGB noir) assombriraient
//! les niveaux flous.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView, D3D11_BIND_SHADER_RESOURCE, D3D11_SUBRESOURCE_DATA,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_IMMUTABLE,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC};

/// Le côté d'une image dans la bibliothèque, et de chaque tranche du tableau.
pub const COTE: u32 = 512;
/// Le nombre de niveaux de mips de `COTE` (512 → 1).
pub const NIVEAUX: u32 = 10;
const DOSSIER: &str = "lueurs_images";

fn dossier() -> Option<PathBuf> {
    let mut path = crate::config::config_dir()?;
    path.push(DOSSIER);
    Some(path)
}

/// L'empreinte FNV-1a d'un fichier — jamais 0 (0 veut dire « pas d'image »).
pub fn empreinte(octets: &[u8]) -> u32 {
    let h = octets.iter().fold(0x811c_9dc5u32, |h, &o| (h ^ u32::from(o)).wrapping_mul(0x0100_0193));
    h.max(1)
}

/// `(id, nom)` d'un fichier de la bibliothèque — `None` s'il n'en est pas un.
fn identifier(nom_de_fichier: &str) -> Option<(u32, String)> {
    let base = nom_de_fichier.strip_suffix(".png")?;
    let (id, nom) = base.split_once(' ').unwrap_or((base, ""));
    (id.len() == 8).then_some(())?;
    let id = u32::from_str_radix(id, 16).ok().filter(|&v| v != 0)?;
    Some((id, nom.to_string()))
}

/// La bibliothèque, triée par nom : `(id, nom)`.
pub fn bibliotheque() -> Vec<(u32, String)> {
    let Some(d) = dossier() else { return Vec::new() };
    let Ok(lecture) = std::fs::read_dir(d) else { return Vec::new() };
    let mut images: Vec<(u32, String)> =
        lecture.flatten().filter_map(|e| e.file_name().to_str().and_then(identifier)).collect();
    images.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    images
}

fn fichier(id: u32) -> Option<PathBuf> {
    let d = dossier()?;
    std::fs::read_dir(&d)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_str().and_then(identifier).is_some_and(|(i, _)| i == id))
        .map(|e| e.path())
}

/// Un nom de fichier SÛR : ni séparateur, ni caractère interdit par Windows.
fn nom_sur(texte: &str) -> String {
    let propre: String = texte
        .chars()
        .map(|c| if c.is_control() || r#"<>:"/\|?*;,"#.contains(c) { '_' } else { c })
        .take(60)
        .collect();
    let propre = propre.trim().trim_end_matches('.').to_string();
    if propre.is_empty() { "image".to_string() } else { propre }
}

/// **IMPORTE UNE IMAGE** dans la bibliothèque : `(id, largeur / hauteur)` de
/// l'image d'origine. `None` si elle ne se décode pas (le journal dit pourquoi).
pub fn importer(source: &Path) -> Option<(u32, f32)> {
    let octets = std::fs::read(source).map_err(|e| crate::diag!("[lueurs] image illisible : {e}")).ok()?;
    let image = image::load_from_memory(&octets).map_err(|e| crate::diag!("[lueurs] image non décodée : {e}")).ok()?;
    let (l, h) = (image.width().max(1), image.height().max(1));
    let id = empreinte(&octets);
    if fichier(id).is_none() {
        let d = dossier()?;
        std::fs::create_dir_all(&d).ok()?;
        let nom = nom_sur(source.file_stem().and_then(|s| s.to_str()).unwrap_or("image"));
        let carre = image::imageops::resize(&image.to_rgba8(), COTE, COTE, image::imageops::FilterType::Lanczos3);
        let cible = d.join(format!("{id:08x} {nom}.png"));
        carre.save(&cible).map_err(|e| crate::diag!("[lueurs] image non enregistrée : {e}")).ok()?;
        oublier(id);
    }
    Some((id, l as f32 / h as f32))
}

/// L'image `id` en RGBA PRÉMULTIPLIÉ, `COTE`×`COTE` — transparente si elle manque
/// (un effet importé d'une autre machine, une image retirée du dossier).
fn pixels(id: u32) -> Vec<u8> {
    let image = fichier(id).and_then(|p| image::open(p).ok()).map(|i| {
        let i = i.to_rgba8();
        if i.dimensions() == (COTE, COTE) { i } else { image::imageops::resize(&i, COTE, COTE, image::imageops::FilterType::Triangle) }
    });
    let Some(image) = image else { return vec![0; (COTE * COTE * 4) as usize] };
    let mut brut = image.into_raw();
    premultiplier(&mut brut);
    brut
}

/// RGB × alpha, sur place — PURE.
pub fn premultiplier(rgba: &mut [u8]) {
    for p in rgba.chunks_exact_mut(4) {
        let a = u16::from(p[3]);
        for c in &mut p[..3] {
            *c = ((u16::from(*c) * a + 127) / 255) as u8;
        }
    }
}

/// Le niveau suivant d'une chaîne de mips : chaque pixel est la moyenne de quatre
/// — PURE. `cote` : le côté du niveau donné (pair, ou 1).
pub fn reduire(rgba: &[u8], cote: u32) -> Vec<u8> {
    let n = (cote / 2).max(1);
    let lire = |x: u32, y: u32, c: usize| u32::from(rgba[((y.min(cote - 1) * cote + x.min(cote - 1)) * 4) as usize + c]);
    let mut sortie = vec![0u8; (n * n * 4) as usize];
    for y in 0..n {
        for x in 0..n {
            for c in 0..4 {
                let s = lire(2 * x, 2 * y, c) + lire(2 * x + 1, 2 * y, c) + lire(2 * x, 2 * y + 1, c) + lire(2 * x + 1, 2 * y + 1, c);
                sortie[((y * n + x) * 4) as usize + c] = ((s + 2) / 4) as u8;
            }
        }
    }
    sortie
}

struct Cache {
    /// Le périphérique de la vue, et les images de ses tranches, dans l'ordre.
    device: usize,
    ids: Vec<u32>,
    vue: Option<ID3D11ShaderResourceView>,
    /// Les chaînes de mips déjà calculées, par image.
    mips: HashMap<u32, Vec<Vec<u8>>>,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache { device: 0, ids: Vec::new(), vue: None, mips: HashMap::new() });
}

/// Rend au device la texture que ce cache détient — **avant la destruction du
/// device** (voir `app::GpuCacheGuard`) : sinon elle ne mourait qu'au démontage
/// du processus, et DXGI y faisait sortir NeuraDesk en code 122.
pub fn liberer_gpu() {
    CACHE.with(|c| {
        if let Ok(mut c) = c.try_borrow_mut() {
            c.vue = None;
            c.device = 0;
        }
    });
}

/// Oublie la chaîne de mips d'une image (elle vient d'être réimportée).
fn oublier(id: u32) {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.mips.remove(&id);
        c.ids.clear();
    });
}

fn chaine(id: u32) -> Vec<Vec<u8>> {
    let mut niveaux = vec![pixels(id)];
    let mut cote = COTE;
    while cote > 1 {
        let suivant = reduire(niveaux.last().map_or(&[][..], Vec::as_slice), cote);
        niveaux.push(suivant);
        cote /= 2;
    }
    niveaux
}

fn creer(device: &ID3D11Device, ids: &[u32], mips: &mut HashMap<u32, Vec<Vec<u8>>>) -> Option<ID3D11ShaderResourceView> {
    for &id in ids {
        mips.entry(id).or_insert_with(|| chaine(id));
    }
    let mut donnees = Vec::with_capacity(ids.len() * NIVEAUX as usize);
    for id in ids {
        for (k, niveau) in mips.get(id)?.iter().enumerate() {
            donnees.push(D3D11_SUBRESOURCE_DATA { pSysMem: niveau.as_ptr().cast(), SysMemPitch: (COTE >> k).max(1) * 4, SysMemSlicePitch: 0 });
        }
    }
    let desc = D3D11_TEXTURE2D_DESC {
        Width: COTE,
        Height: COTE,
        MipLevels: NIVEAUX,
        ArraySize: ids.len() as u32,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, Some(donnees.as_ptr()), Some(&mut texture)) }
        .map_err(|e| crate::diag!("[lueurs] tableau d'images non créé : {e}"))
        .ok()?;
    let mut vue = None;
    unsafe { device.CreateShaderResourceView(&texture?, None, Some(&mut vue)) }.ok()?;
    vue
}

/// **LIE LES IMAGES DE LA SCÈNE AU REGISTRE `t1`** — à chaque rendu (voir
/// `QuadRenderer::render`). Le tableau n'est recréé que quand la LISTE des images
/// utilisées change, ou le périphérique.
pub fn lier(ctx: &ID3D11DeviceContext) {
    let ids = crate::gfx::lueur::images_utilisees();
    let vue = CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if ids.is_empty() {
            return None;
        }
        let device = unsafe { ctx.GetDevice() }.ok()?;
        let cle = device.as_raw() as usize;
        if c.ids != ids || c.device != cle || c.vue.is_none() {
            let Cache { mips, .. } = &mut *c;
            let vue = creer(&device, &ids, mips);
            c.vue = vue;
            c.ids = ids;
            c.device = cle;
        }
        c.vue.clone()
    });
    unsafe { ctx.PSSetShaderResources(1, Some(&[vue])) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_nom_de_la_bibliotheque_se_relit() {
        assert_eq!(identifier("0a1b2c3d Coucher de soleil.png"), Some((0x0a1b2c3d, "Coucher de soleil".to_string())));
        assert_eq!(identifier("0a1b2c3d.png"), Some((0x0a1b2c3d, String::new())));
        assert_eq!(identifier("00000000 rien.png"), None, "0 veut dire « pas d'image »");
        assert_eq!(identifier("pas une image.png"), None);
        assert_eq!(identifier("0a1b2c3d photo.jpg"), None);
    }

    #[test]
    fn le_nom_d_un_fichier_est_sur() {
        assert_eq!(nom_sur("a/b\\c:d;e,f"), "a_b_c_d_e_f");
        assert_eq!(nom_sur("   "), "image");
    }

    #[test]
    fn l_empreinte_n_est_jamais_zero_et_distingue_deux_fichiers() {
        assert_ne!(empreinte(b"a"), empreinte(b"b"));
        assert_ne!(empreinte(b""), 0);
    }

    /// Un pixel transparent devient NOIR transparent : c'est ce qui empêche le
    /// flou d'assombrir les bords.
    #[test]
    fn l_alpha_est_premultiplie() {
        let mut p = [255, 128, 0, 128, 200, 200, 200, 0];
        premultiplier(&mut p);
        assert_eq!(p, [128, 64, 0, 128, 0, 0, 0, 0]);
    }

    #[test]
    fn un_niveau_de_mips_est_la_moyenne_de_quatre_pixels() {
        let rgba = [0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0];
        assert_eq!(reduire(&rgba, 2), vec![128, 128, 128, 128]);
        assert_eq!(reduire(&[9, 9, 9, 9], 1), vec![9, 9, 9, 9], "le dernier niveau reste 1×1");
    }
}
