//! Capture du bureau sous les barres du dock/fenêtres `egui`, floutée puis
//! teintée (voir `ui::color::soften_tint`), pour approcher l'effet
//! "Mica"/Acrylic de la barre des tâches Windows plutôt qu'un simple aplat
//! de couleur.
//!
//! Limite assumée : contrairement au vrai Mica (géré par le compositeur
//! DWM, qui voit "ce qu'il y a en dessous" indépendamment de la fenêtre
//! courante), un programme classique ne peut capturer que ce qui est
//! RÉELLEMENT affiché à l'écran — y compris son propre rendu s'il est
//! visible au moment de la capture. On évite de se capturer soi-même sans
//! le moindre clignotement grâce à `SetWindowDisplayAffinity(hwnd,
//! WDA_EXCLUDEFROMCAPTURE)` (voir `win32::window::Window::create_dock` et
//! `win32::egui_overlay`) : nos fenêtres restent normalement affichées à
//! l'écran, mais deviennent invisibles pour toute capture (la nôtre y
//! compris — documenté comme s'appliquant aussi à la Desktop Duplication
//! API ci-dessous, pas seulement à `BitBlt`), sans avoir besoin de les
//! masquer/réafficher.
//!
//! **Chemin GPU (par défaut)** : `gfx::duplication` capture le bureau à
//! pleine résolution écran directement en texture D3D11 (`IDXGIOutputDuplication`,
//! coût GPU→GPU négligeable), puis `gfx::blur` le recadre et le floute en
//! plusieurs passes sur GPU (dual Kawase) — voir leurs docs de module.
//! **Repli GDI** (le reste de ce fichier) : utilisé automatiquement, par
//! rectangle, si la Desktop Duplication est indisponible pour cette frame
//! (session verrouillée, bureau distant, GPU hybride...) — capture
//! `BitBlt`/`StretchBlt` sous-échantillonnée dès la source (voir
//! `DOWNSCALE`) puis flou boîte sur CPU. Ne doit JAMAIS faire planter
//! l'appli (voir la mémoire [[neuradesk-live-input-safety]] : elle tourne
//! sur le bureau réel de l'utilisateur), d'où ce repli systématique plutôt
//! qu'une erreur remontée à l'appelant.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use windows::core::Result;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D11::ID3D11Device;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    SetBrushOrgEx, SetStretchBltMode, StretchBlt, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HALFTONE,
    SRCCOPY,
};

use crate::gfx::texture::Texture;
use crate::gfx::{blur, duplication};

/// Rayon (en pixels) du flou boîte CPU appliqué en repli GDI (voir la doc
/// de module) — un flou séparable simple. Appliqué à la capture DÉJÀ
/// SOUS-ÉCHANTILLONNÉE (voir `DOWNSCALE`), donc largement assez fort
/// visuellement malgré un rayon modeste en pixels de la texture
/// réellement produite.
const BLUR_RADIUS: i32 = 6;

/// Facteur de réduction linéaire appliqué À LA CAPTURE elle-même (pas après
/// coup) — BUG DE PERFORMANCE RÉEL CONSTATÉ EN PRATIQUE sans ça : capturer
/// puis flouter en PLEINE résolution (jusqu'à l'écran entier moins les
/// barres pour le Dash, en 4K) coûte assez cher en CPU (`BitBlt` + flou
/// boîte séparable, tous deux O(largeur×hauteur)) pour forcer un
/// rafraîchissement toutes les 750ms plutôt qu'à chaque frame — perceptible
/// comme "pas en temps réel" par l'utilisateur (le flou ne suit le bureau
/// qu'à ~1,3 im/s), alors que l'objectif explicite de cette architecture
/// (voir `win32::egui_overlay`, doc de module) est un flou "en temps réel,
/// au moins 60 FPS", calqué sur la fréquence de l'écran. En réduisant la
/// capture elle-même par CE facteur AVANT de la flouter (voir
/// `capture_screen_region_scaled`, `StretchBlt` — le sous-échantillonnage
/// coûte le prix du blit, pas de la résolution source), le nombre de
/// pixels à traiter chute d'un facteur `DOWNSCALE²` (16× pour la valeur
/// actuelle) : le résultat, plus petit, est ensuite ré-étiré par le GPU en
/// l'affichant (filtrage bilinéaire, voir `gfx::texture::create_sampler_state`)
/// — invisible une fois flouté, exactement comme l'ancien fond du Dash
/// laissait `background-size: cover` faire l'étirement (voir l'historique).
/// Ce gain de coût permet de capturer à CHAQUE FRAME (voir `app::run`,
/// `win32::egui_overlay::EguiOverlay::refresh_backdrop_if_due`) plutôt que
/// de dépendre d'un intervalle minimal. Ne s'applique qu'au repli GDI (voir
/// la doc de module) — le chemin GPU normal (`gfx::duplication` +
/// `gfx::blur`) capture à pleine résolution écran sans avoir besoin de ce
/// compromis, justement l'amélioration recherchée par ce repli.
const DOWNSCALE: u32 = 4;

/// Capture puis floute chacun de `rects` (coordonnées écran physiques
/// absolues, voir `win32::window::Window::screen_origin`), et upload le
/// résultat en textures D3D11, dans le même ordre — voir la doc de module
/// pour le choix entre chemin GPU (Desktop Duplication + flou multi-passes,
/// le cas normal) et repli GDI (par rectangle, en cas d'échec du premier).
pub fn capture_blurred_batch(device: &ID3D11Device, rects: &[[i32; 4]]) -> Vec<Result<Texture>> {
    // Mode « visible dans les captures d'écran » (voir `gfx::capture_visibility`,
    // doc de module) : nos fenêtres sont en `WDA_NONE`, donc notre Desktop
    // Duplication LES VOIT et leur surface DWM se relit en BLANC (bug réel
    // documenté plus bas, dans l'historique). La duplication est donc
    // inutilisable ici — mais le flou, lui, reste demandé.
    //
    // ⭐ **LE REMÈDE EST LA LOUPE, PAS LE RENONCEMENT** (DEMANDE EXPLICITE
    // 2026-11-12 : « quand Rendre l'interface visible sur les captures
    // d'écran est activé, le flou doit aussi être activé »). `gfx::mag_capture`
    // capture le bureau et EXCLUT nos fenêtres par `MagSetWindowFilterList` :
    // l'exclusion ne vaut que pour notre loupe, donc les captureurs externes
    // (OBS, Capture d'écran) continuent de voir l'interface. C'est la
    // technique de WebRTC/RustDesk (voir sa doc de module).
    //
    // Elle passe par le CPU et rend l'écran à pleine résolution : ses
    // textures sont donc gardées `MAG_CACHE_TTL` avant d'être recapturées, au
    // lieu d'être refaites à chaque frame comme le chemin GPU ci-dessous.
    //
    // Si la loupe elle-même est indisponible (échec de `MagInitialize`, WDDM
    // absent, RDP...), on retombe sur l'ancien comportement : un fond NEUTRE
    // construit ici, que le shader IGNORE (drapeau `sans_fond`,
    // `Ombres.y`) — la teinte seule, avec l'alpha de l'utilisateur composé par
    // DWM. Surtout PAS une liste vide ni des `Err` : une liste vide fait
    // échouer `app::capture_bar_backdrops` ("capture d'arrière-plan manquante
    // (haut)"), et un `Err` remonte en erreur fatale au démarrage
    // ("L'opération a réussi. (0x00000000)").
    if crate::gfx::capture_visibility::est_capturable() {
        if crate::gfx::mag_capture::disponible() {
            return rects.iter().map(|&rect| capture_blurred_mag(device, rect)).collect();
        }
        const NEUTRAL_RGBA: [u8; 4] = [128, 128, 128, 255];
        return rects
            .iter()
            .map(|_| crate::gfx::texture::Texture::from_rgba(device, 1, 1, &NEUTRAL_RGBA))
            .collect();
    }

    let ctx = match unsafe { device.GetImmediateContext() } {
        Ok(ctx) => ctx,
        // Ne devrait jamais arriver en pratique (le device existe déjà,
        // donc son contexte immédiat aussi) — repli GDI par sécurité plutôt
        // que de propager une erreur qui ferait disparaître tout arrière-plan.
        Err(_) => return rects.iter().map(|&rect| capture_blurred_rect_gdi(device, rect)).collect(),
    };

    // Une seule capture de bureau partagée pour tout le lot (les trois
    // barres du dock arrivent ensemble ici, voir `app::run`) — inutile
    // d'appeler `AcquireNextFrame` une fois par rectangle.
    let gpu_frame = duplication::ensure_frame(device, &ctx);

    rects
        .iter()
        .map(|&rect| {
            // BUG RÉEL CORRIGÉ ("le fond capturé pour la transparence ne
            // correspond pas à ce qu'il y a derrière les barres" — écrans
            // secondaires) : `gfx::duplication` ne duplique QUE le moniteur
            // PRINCIPAL (voir sa doc de module, "Portée volontairement
            // limitée"), donc `desktop_rect` ne couvre jamais un écran
            // secondaire. `blur::render_backdrop` RECADRAIT malgré tout le
            // rect demandé DANS cette zone (`clamp`, voir sa doc) plutôt
            // que d'échouer — pour une barre secondaire, ça capturait
            // silencieusement un bout du moniteur PRINCIPAL au lieu du
            // bureau réellement affiché sous cette barre. On ne tente donc
            // le chemin GPU que si `rect` est ENTIÈREMENT contenu dans la
            // zone effectivement dupliquée ; sinon repli GDI direct, qui
            // capture toujours les VRAIES coordonnées écran demandées quel
            // que soit le moniteur.
            if let Some((full_frame, desktop_rect)) = &gpu_frame {
                if rect_within(rect, *desktop_rect) {
                    match blur::render_backdrop(device, &ctx, full_frame, *desktop_rect, rect) {
                        Ok(srv) => return Ok(Texture { srv }),
                        Err(err) => {
                            eprintln!("[neuradesk] flou GPU indisponible pour ce rectangle, repli GDI ({err})");
                        }
                    }
                }
            }
            capture_blurred_rect_gdi(device, rect)
        })
        .collect()
}

/// Vide le cache des textures du mode capturable — à appeler AVANT que le
/// device D3D ne soit détruit (voir `app::GpuCacheGuard`). ⛔ **Oublié, il fait
/// sortir l'application en 0x87A** : ce sont des textures vivantes d'un device
/// qu'on détruit, l'exception « objets DXGI encore vivants » que le garde
/// existe pour éteindre (même défaut que `gfx::duplication::release`).
pub fn release_cache() {
    MAG_CACHE.with(|cache| cache.borrow_mut().clear());
}

/// Âge maximal d'une texture de la loupe avant nouvelle capture. La loupe
/// travaille sur le CPU, à la résolution de l'écran (voir
/// `gfx::mag_capture`, doc de module) : la recapturer à chaque frame coûterait
/// bien plus que le flou lui-même. 60 ms donnent ~16 images/s au fond flouté —
/// il ne porte que du détail flou, un décalage de quelques dizaines de
/// millisecondes ne se voit pas, alors que le coût, lui, se mesurerait.
const MAG_CACHE_TTL: Duration = Duration::from_millis(60);

/// Une texture au-delà de cet âge est oubliée même si son rectangle n'a pas
/// été redemandé (un popup fermé laisse sinon sa texture en mémoire à vie).
const MAG_CACHE_OUBLI: Duration = Duration::from_secs(5);

/// Nombre maximal d'entrées du cache — chaque barre, congé et bout est un
/// rectangle distinct, et plusieurs popups peuvent être ouverts ; au-delà, on
/// jette la plus ancienne.
const MAG_CACHE_MAX: usize = 48;

struct EntreeMag {
    rect: [i32; 4],
    quand: Instant,
    texture: Texture,
}

thread_local! {
    /// Les textures du mode capturable, partagées par TOUTES les fenêtres :
    /// le dock, les popups CEF, l'infobulle et les aperçus demandent leur fond
    /// au même endroit (voir `capture_blurred_mag`), et un même rectangle n'a
    /// aucune raison d'être capturé deux fois.
    static MAG_CACHE: RefCell<Vec<EntreeMag>> = const { RefCell::new(Vec::new()) };
}

/// Fond flouté du mode capturable : capture par la loupe (avec nos fenêtres
/// exclues), réduction, flou boîte — puis mémoire pendant `MAG_CACHE_TTL`.
/// En cas d'échec ponctuel, la dernière texture connue de ce rectangle est
/// resservie ; s'il n'y en a aucune, un gris neutre (voir l'appelant).
fn capture_blurred_mag(device: &ID3D11Device, rect: [i32; 4]) -> Result<Texture> {
    let maintenant = Instant::now();
    let frais = MAG_CACHE.with(|cache| {
        cache
            .borrow()
            .iter()
            .find(|entree| entree.rect == rect && maintenant.duration_since(entree.quand) < MAG_CACHE_TTL)
            .map(|entree| entree.texture.clone())
    });
    if let Some(texture) = frais {
        return Ok(texture);
    }

    match crate::gfx::mag_capture::capturer(rect, DOWNSCALE) {
        Some((largeur, hauteur, pixels)) => {
            let flou = box_blur(&pixels, largeur, hauteur, BLUR_RADIUS);
            let texture = Texture::from_rgba(device, largeur, hauteur, &flou)?;
            MAG_CACHE.with(|cache| {
                let mut cache = cache.borrow_mut();
                cache.retain(|entree| {
                    entree.rect != rect && maintenant.duration_since(entree.quand) < MAG_CACHE_OUBLI
                });
                cache.push(EntreeMag { rect, quand: maintenant, texture: texture.clone() });
                if cache.len() > MAG_CACHE_MAX {
                    let plus_ancienne = cache
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, entree)| entree.quand)
                        .map(|(rang, _)| rang);
                    if let Some(rang) = plus_ancienne {
                        cache.remove(rang);
                    }
                }
            });
            Ok(texture)
        }
        None => {
            // Échec ponctuel : mieux vaut une image un peu datée qu'un trou.
            let ancienne = MAG_CACHE.with(|cache| {
                cache
                    .borrow()
                    .iter()
                    .find(|entree| entree.rect == rect)
                    .map(|entree| entree.texture.clone())
            });
            match ancienne {
                Some(texture) => Ok(texture),
                None => {
                    const NEUTRAL_RGBA: [u8; 4] = [128, 128, 128, 255];
                    Texture::from_rgba(device, 1, 1, &NEUTRAL_RGBA)
                }
            }
        }
    }
}

/// `rect` (coordonnées écran physiques absolues, `[x, y, w, h]`) est-il
/// ENTIÈREMENT à l'intérieur de `desktop_rect` (la zone réellement dupliquée
/// par `gfx::duplication`, limitée au moniteur principal — voir sa doc de
/// module) ? Voir `capture_blurred_batch`, seul appelant : un rect qui
/// déborde (même partiellement, ex. un moniteur secondaire) ne doit jamais
/// passer par le chemin GPU, qui le RECADRAIT silencieusement dans la
/// mauvaise zone plutôt que d'échouer (voir `blur::render_backdrop`).
fn rect_within(rect: [i32; 4], desktop_rect: RECT) -> bool {
    let [x, y, w, h] = rect;
    x >= desktop_rect.left
        && y >= desktop_rect.top
        && x.saturating_add(w) <= desktop_rect.right
        && y.saturating_add(h) <= desktop_rect.bottom
}

/// Repli GDI (voir la doc de module) : capture `BitBlt`/`StretchBlt` déjà
/// sous-échantillonnée (voir `DOWNSCALE`) puis flou boîte CPU (voir
/// `box_blur`) — l'ancien chemin, avant l'ajout de `gfx::duplication`/
/// `gfx::blur`, gardé comme filet de sécurité.
fn capture_blurred_rect_gdi(device: &ID3D11Device, [x, y, w, h]: [i32; 4]) -> Result<Texture> {
    let width = (w.max(1) as u32 / DOWNSCALE).max(1);
    let height = (h.max(1) as u32 / DOWNSCALE).max(1);
    let pixels = unsafe { capture_screen_region_scaled(x, y, w.max(1) as u32, h.max(1) as u32, width, height) };
    let blurred = box_blur(&pixels, width, height, BLUR_RADIUS);
    Texture::from_rgba(device, width, height, &blurred)
}

/// Capture le rectangle écran `(x, y, src_width, src_height)` via GDI, DÉJÀ
/// réduit à `(dst_width, dst_height)` (voir `DOWNSCALE`, doc de module) —
/// `StretchBlt` fait la réduction directement pendant le blit (mode
/// `HALFTONE`, le seul des modes `StretchBlt` qui moyenne proprement
/// plusieurs pixels sources par pixel destination plutôt que d'en
/// sous-échantillonner un sur N ; `SetBrushOrgEx` juste après est requis
/// par ce mode précis, voir la documentation Win32 de `SetStretchBltMode`)
/// — bien moins cher qu'un `BitBlt` pleine résolution suivi d'une réduction
/// séparée. Ne touche pas à la visibilité de quelque fenêtre que ce soit —
/// voir `capture_blurred_batch` pour l'occultation de la fenêtre du dock.
unsafe fn capture_screen_region_scaled(
    x: i32,
    y: i32,
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> Vec<u8> {
    let pixel_count = (dst_width * dst_height) as usize;
    let mut rgba = vec![0u8; pixel_count * 4];

    let screen_dc = GetDC(None);
    let mem_dc = CreateCompatibleDC(Some(screen_dc));

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: dst_width as i32,
            biHeight: -(dst_height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0, // BI_RGB
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    if let Ok(bitmap) = CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
        let old_bitmap = SelectObject(mem_dc, bitmap.into());
        SetStretchBltMode(mem_dc, HALFTONE);
        // Requis par le mode HALFTONE (voir sa doc de fonction) : sans ça,
        // l'origine du motif de tramage utilisé en interne dérive d'un
        // appel à l'autre, visible comme un léger défilement du flou.
        let _ = SetBrushOrgEx(mem_dc, 0, 0, None);
        let _ = StretchBlt(
            mem_dc,
            0,
            0,
            dst_width as i32,
            dst_height as i32,
            Some(screen_dc),
            x,
            y,
            src_width as i32,
            src_height as i32,
            SRCCOPY,
        );

        // DIB GDI = octets (B, G, R, réservé) par pixel ; on convertit en
        // RGBA pour `Texture::from_rgba`, alpha forcé opaque (StretchBlt ne
        // touche pas le 4e octet — une capture d'écran normale est opaque).
        let src = std::slice::from_raw_parts(bits as *const u8, pixel_count * 4);
        for (dst, src_px) in rgba.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            dst[0] = src_px[2];
            dst[1] = src_px[1];
            dst[2] = src_px[0];
            dst[3] = 255;
        }

        SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(bitmap.into());
    }

    let _ = DeleteDC(mem_dc);
    ReleaseDC(None, screen_dc);
    rgba
}

/// Flou boîte séparable (passe horizontale puis verticale) sur un buffer
/// RGBA8 de `width`×`height` — repli GDI uniquement, voir la doc de module
/// (le chemin GPU normal utilise `gfx::blur`, un flou multi-passes bien
/// plus propre).
fn box_blur(pixels: &[u8], width: u32, height: u32, radius: i32) -> Vec<u8> {
    let w = width as i32;
    let h = height as i32;
    let mut horizontal = vec![0u8; pixels.len()];
    box_blur_pass(pixels, &mut horizontal, w, h, radius, true);
    let mut result = vec![0u8; pixels.len()];
    box_blur_pass(&horizontal, &mut result, w, h, radius, false);
    result
}

fn box_blur_pass(src: &[u8], dst: &mut [u8], w: i32, h: i32, radius: i32, horizontal: bool) {
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            for offset in -radius..=radius {
                let (sx, sy) = if horizontal { (x + offset, y) } else { (x, y + offset) };
                if sx < 0 || sx >= w || sy < 0 || sy >= h {
                    continue;
                }
                let idx = ((sy * w + sx) * 4) as usize;
                for c in 0..4 {
                    sum[c] += src[idx + c] as u32;
                }
                count += 1;
            }
            let idx = ((y * w + x) * 4) as usize;
            for c in 0..4 {
                dst[idx + c] = (sum[c] / count.max(1)) as u8;
            }
        }
    }
}
