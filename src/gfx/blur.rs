//! Flou d'arrière-plan multi-passes sur GPU (« dual Kawase blur » —
//! technique décrite par Marius Bjørge, ARM, dans « Bandwidth-Efficient
//! Rendering », largement reprise depuis pour les flous d'interface temps
//! réel type GNOME Shell/Unity) : quelques passes de réduction successive
//! (`PSDown`) puis de remontée (`PSUp`), chacune un simple filtre à 4-8
//! échantillons bilinéaires — beaucoup plus « propre » (pas d'aspect
//! « boîte »/blocs) que le flou boîte CPU de secours de `gfx::backdrop`,
//! pour un coût GPU négligeable même en partant d'un recadrage à pleine
//! résolution écran (voir `gfx::duplication`, qui fournit ce recadrage).
//!
//! Pas de tampon de sommets/index : chaque passe dessine un unique grand
//! triangle couvrant tout le viewport, généré dans le vertex shader à
//! partir de `SV_VertexID` seul (technique standard, voir `VSMain`) — donc
//! aucun `ID3D11InputLayout` n'est nécessaire non plus, le vertex shader
//! ne consomme aucune donnée fournie par l'assembleur d'entrée.
//!
//! Une « pyramide » de textures (`Pyramid`) est mise en cache par taille
//! de rectangle demandée (`PYRAMIDS`, thread_local — tout le rendu tourne
//! sur un seul thread, voir `app::run`) : les fenêtres de ce projet ont
//! des tailles stables d'une frame à l'autre (barres du dock, Dash,
//! Réglages...), pas la peine de réallouer les textures à chaque appel.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::mem::size_of;

use windows::core::Result;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

use crate::gfx::material;
use crate::gfx::shader::{blob_bytes, compile_shader};

/// En dessous de cette taille (dans une dimension), on arrête de
/// descendre la pyramide — un niveau plus petit apporterait plus de bruit
/// d'échantillonnage que de flou utile, et certaines barres du dock sont
/// déjà fines nativement (voir `ui::topbar`/`ui::dock`).
const MIN_LEVEL_SIZE: u32 = 8;
/// Bornes du curseur "Passes de flou" du panneau de réglages (voir
/// `blur_passes`/`set_blur_passes`, `win32::settings_dash`) — au-delà de
/// `MAX_LEVELS_HARD_CAP`, les niveaux suivants n'ajouteraient plus de flou
/// perceptible (voir la doc de module sur la technique du dual Kawase :
/// chaque niveau double le rayon effectif du flou, la contribution
/// marginale décroît vite) tout en coûtant un aller-retour GPU de plus par
/// frame et par barre.
const MIN_LEVELS_HARD_CAP: u32 = 1;
const MAX_LEVELS_HARD_CAP: u32 = 8;

/// Marge (en pixels écran, avant tout flou) ajoutée de CHAQUE côté du
/// rectangle demandé au moment de la capture — BUG RÉEL SIGNALÉ ("oui il y
/// a une coture visible") : chaque rectangle floue une capture RECADRÉE
/// EXACTEMENT à ses propres bords (voir l'ancien `CopySubresourceRegion`
/// dans `render_backdrop`, avant cette marge), donc les passes de descente/
/// remontée Kawase échantillonnent près de CES bords avec l'adressage
/// "clamp" du sampler (voir `texture::create_sampler_state`) — qui répète
/// le pixel de bord du recadrage au lieu de lire le VRAI contenu voisin du
/// bureau, lequel appartient au recadrage indépendant d'un élément adjacent
/// (ex. le panneau Dash contre la barre du haut). Deux flous indépendants,
/// chacun "collé" à son propre bord, ne se raccordent pas — une coupure
/// nette et visible à la jonction. En capturant une zone plus large que le
/// rectangle réellement affiché (voir `padded_*` dans `render_backdrop`) et
/// en ne recadrant qu'À LA FIN (voir `PSCrop`/`Pyramid::final_crop`), le
/// flou dispose de vrai contenu voisin à ses bords internes — la jonction
/// entre deux éléments adjacents s'appuie alors sur le MÊME bureau réel des
/// deux côtés plutôt que sur deux répétitions de bord indépendantes.
const BLEED_MARGIN: u32 = 28;
/// Profondeur par défaut de la pyramide (avant tout réglage utilisateur,
/// voir `config::load_blur_passes`) — valeur d'origine, figée en constante
/// avant le curseur "Intensité du flou" du panneau de réglages.
const DEFAULT_BLUR_PASSES: u32 = 6;
// Ancienne borne haute du curseur "Style de flou", aujourd'hui portée par
// `gfx::material::Material::stop_levels` (voir sa doc) — BUG RÉEL CONSTATÉ
// EN PRATIQUE avec une borne plus haute (6, essayé d'abord — voir
// l'historique, "j'ai l'impression qu'il n'y a que 2 styles de flou") :
// les barres du dock ne font que 72px d'épaisseur PHYSIQUE (voir
// `ui::dock::DockLayout::bar_thickness`), donc leur pyramide ne peut
// descendre que 4 niveaux avant `MIN_LEVEL_SIZE` (72→36→18→9),
// `deepest = 3` — largement en dessous de 6. `stop =
// deepest.saturating_sub(stop_levels)` sature alors à 0 dès que
// `stop_levels >= deepest` : plusieurs préréglages produisaient EXACTEMENT
// le même rendu (le plus net) sur la surface la plus visible de l'appli.
// `3` reste le plafond honnête pour `Material::stop_levels` : chaque
// matériau garde un rendu distinct au moins sur les barres.
thread_local! {
    /// Profondeur de pyramide ACTUELLE (voir `blur_passes`/`set_blur_passes`)
    /// — un seul thread de rendu pour toute l'appli (voir `app::run`), même
    /// principe que `win32::menu::MENU_SCALE`.
    static BLUR_PASSES: Cell<u32> = const { Cell::new(DEFAULT_BLUR_PASSES) };
}

/// Nombre de passes de flou (profondeur de la pyramide Kawase) actuellement
/// utilisé — voir `BLUR_PASSES`, réglable via le curseur "Intensité du
/// flou" du panneau de réglages (`win32::settings_dash`).
///
/// ⭐ **LE MODE CAPTURABLE NE COUPE PLUS LE FLOU** (DEMANDE EXPLICITE
/// 2026-11-12, voir `gfx::capture_visibility`) : il rend seulement impossible
/// la CAPTURE du fond (nos fenêtres y apparaîtraient). La valeur réglée reste
/// donc celle que rapporte aussi le panneau de réglages
/// (`win32::cef_settings::push_state`) ; sans fond à flouter, elle n'a
/// simplement rien à traiter dans ce mode.
pub fn blur_passes() -> u32 {
    BLUR_PASSES.with(Cell::get)
}

/// Change le nombre de passes de flou, borné à
/// `[MIN_LEVELS_HARD_CAP, MAX_LEVELS_HARD_CAP]` (voir leur doc) — PAS
/// persisté ici (voir `config::save_blur_passes`, appelé par l'appelant),
/// même convention que `win32::menu::set_menu_scale`. Le changement est
/// repris à la prochaine capture (voir `PYRAMIDS`, dont la clé inclut
/// désormais ce nombre de passes — un ancien réglage laissé en cache n'est
/// jamais réutilisé pour un nouveau).
pub fn set_blur_passes(passes: u32) {
    BLUR_PASSES.with(|p| p.set(passes.clamp(MIN_LEVELS_HARD_CAP, MAX_LEVELS_HARD_CAP)));
}

/// Deux `float2` génériques — `half_pixel`/`_pad` pour `PSDown`/`PSUp`/
/// `PSTent`, `uv_scale`/`uv_offset` pour `PSCrop` (voir sa doc dans
/// `SHADER_SOURCE`) : même disposition mémoire (16 octets), un seul type
/// Rust suffit, `write_constants` choisit juste quoi y mettre selon la
/// passe.
#[repr(C)]
struct BlurConstants {
    param0: [f32; 2],
    param1: [f32; 2],
}

const SHADER_SOURCE: &str = r#"
struct VSOut {
    float4 pos : SV_POSITION;
    float2 uv  : TEXCOORD0;
};

// Un seul grand triangle couvrant tout l'écran (uv 0..2), écrêté par le
// rasteriseur au viewport réel — évite tout tampon de sommets pour un
// pipeline aussi simple (technique standard, voir la doc de module).
VSOut VSMain(uint id : SV_VertexID) {
    VSOut o;
    float2 uv = float2((id << 1) & 2, id & 2);
    o.pos = float4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}

cbuffer BlurConstants : register(b0) {
    float2 HalfPixel;
    float2 _Padding;
};

Texture2D    SrcTex     : register(t0);
SamplerState SrcSampler : register(s0);

// Réduction Kawase : 5 échantillons (centre pondéré ×4 + 4 coins en
// diagonale), HalfPixel = un demi-texel de la texture SOURCE.
float4 PSDown(VSOut input) : SV_TARGET {
    float2 uv = input.uv;
    float4 sum = SrcTex.Sample(SrcSampler, uv) * 4.0;
    sum += SrcTex.Sample(SrcSampler, uv - HalfPixel);
    sum += SrcTex.Sample(SrcSampler, uv + HalfPixel);
    sum += SrcTex.Sample(SrcSampler, uv + float2(HalfPixel.x, -HalfPixel.y));
    sum += SrcTex.Sample(SrcSampler, uv - float2(HalfPixel.x, -HalfPixel.y));
    return sum / 8.0;
}

// Remontée Kawase : 8 échantillons en octogone (poids 1/2 alternés),
// HalfPixel = un demi-texel de la texture SOURCE (le niveau plus petit
// qu'on remonte).
float4 PSUp(VSOut input) : SV_TARGET {
    float2 uv = input.uv;
    float4 sum = SrcTex.Sample(SrcSampler, uv + float2(-HalfPixel.x * 2.0, 0.0));
    sum += SrcTex.Sample(SrcSampler, uv + float2(-HalfPixel.x, HalfPixel.y)) * 2.0;
    sum += SrcTex.Sample(SrcSampler, uv + float2(0.0, HalfPixel.y * 2.0));
    sum += SrcTex.Sample(SrcSampler, uv + float2(HalfPixel.x, HalfPixel.y)) * 2.0;
    sum += SrcTex.Sample(SrcSampler, uv + float2(HalfPixel.x * 2.0, 0.0));
    sum += SrcTex.Sample(SrcSampler, uv + float2(HalfPixel.x, -HalfPixel.y)) * 2.0;
    sum += SrcTex.Sample(SrcSampler, uv + float2(0.0, -HalfPixel.y * 2.0));
    sum += SrcTex.Sample(SrcSampler, uv + float2(-HalfPixel.x, -HalfPixel.y)) * 2.0;
    return sum / 12.0;
}

// Tente large 13 échantillons — DÉLIBÉRÉMENT différente de la remontée
// Kawase (`PSUp`) : rayon 4x plus large, poids quasi uniformes, aucune
// structure conservée. C'est ce qui donne au matériau Mica
// (`gfx::material::BlurAlgo::MicaTent`) son aspect plat/laiteux, là où
// Kawase conserve un dégradé structuré. Une seule passe suffit (voir
// `render_backdrop`) — pas de remontée progressive comme `PSUp`.
float4 PSTent(VSOut input) : SV_TARGET {
    float2 uv = input.uv;
    float2 o = HalfPixel * 4.0;
    float4 sum = 0.0;
    [unroll] for (int y = -1; y <= 1; ++y) {
        [unroll] for (int x = -1; x <= 1; ++x) {
            sum += SrcTex.Sample(SrcSampler, uv + float2(x, y) * o);
        }
    }
    // 4 diagonales à mi-distance : casse l'aspect "croix" d'une grille 3x3
    // pure sur un dégradé lisse.
    sum += SrcTex.Sample(SrcSampler, uv + float2( o.x,  o.y) * 0.5);
    sum += SrcTex.Sample(SrcSampler, uv + float2( o.x, -o.y) * 0.5);
    sum += SrcTex.Sample(SrcSampler, uv + float2(-o.x,  o.y) * 0.5);
    sum += SrcTex.Sample(SrcSampler, uv + float2(-o.x, -o.y) * 0.5);
    return sum / 13.0;
}

// Dernière passe de `render_backdrop` (voir `BLEED_MARGIN`, doc de
// constante) : réutilise le MÊME cbuffer `BlurConstants` que `PSDown`/`PSUp`
// (même disposition mémoire, 2 paires de `float2` — seule la SIGNIFICATION
// change : `UvOffset`/`UvScale` au lieu de `HalfPixel`/`_Padding`, voir
// `write_constants`) pour recadrer le dernier niveau de la pyramide
// (agrandi par la marge de bavure) vers le rectangle réellement affiché,
// par un simple ré-échantillonnage bilinéaire sans flou additionnel — le
// flou a déjà eu lieu dans les passes précédentes.
float4 PSCrop(VSOut input) : SV_TARGET {
    float2 uv = input.uv * HalfPixel + _Padding; // UvScale, UvOffset
    return SrcTex.Sample(SrcSampler, uv);
}
"#;

/// Un niveau de la pyramide — voir `Pyramid`. Le niveau 0 sert à la fois
/// de destination de recadrage (`ID3D11DeviceContext::CopySubresourceRegion`
/// depuis `gfx::duplication`) et de source pour la première passe de
/// réduction ; il porte donc les mêmes bind flags que les autres niveaux
/// par simplicité (le RTV du niveau 0 n'est en pratique jamais utilisé).
struct Level {
    tex: ID3D11Texture2D,
    rtv: ID3D11RenderTargetView,
    srv: ID3D11ShaderResourceView,
    width: u32,
    height: u32,
}

/// Pyramide de textures pour UNE taille de rectangle donnée — mise en
/// cache dans `PYRAMIDS`, voir la doc de module.
struct Pyramid {
    /// `levels[0]` = résolution native du rectangle demandé, chaque niveau
    /// suivant fait moitié moins large/haut, jusqu'à `MIN_LEVEL_SIZE`/
    /// `blur_passes()`.
    levels: Vec<Level>,
    /// Index (dans `levels`) où s'arrête la remontée — voir `stop_levels`.
    /// `levels[stop]` alimente la passe finale de recadrage (voir
    /// `draw_crop_pass`), PAS le résultat renvoyé directement (voir
    /// `final_crop`, dont c'est le rôle depuis `BLEED_MARGIN`).
    stop: usize,
    /// Résultat final, à la taille D'ORIGINE (non agrandie par
    /// `BLEED_MARGIN`) — voir `build_pyramid`/`draw_crop_pass`. C'est ce
    /// niveau, pas `levels[stop]`, que `render_backdrop` renvoie.
    final_crop: Level,
}

struct BlurPipeline {
    vs: ID3D11VertexShader,
    ps_down: ID3D11PixelShader,
    ps_up: ID3D11PixelShader,
    /// Voir `gfx::material::BlurAlgo::MicaTent` — passe alternative à
    /// `ps_up`, utilisée seulement pour ce matériau.
    ps_tent: ID3D11PixelShader,
    /// Voir `draw_crop_pass`/`BLEED_MARGIN` — dernière passe de
    /// `render_backdrop`, recadre le résultat agrandi par la marge de
    /// bavure vers la taille d'origine.
    ps_crop: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    constants: ID3D11Buffer,
    blend_state: ID3D11BlendState,
    rasterizer_state: ID3D11RasterizerState,
}

thread_local! {
    static PIPELINE: RefCell<Option<BlurPipeline>> = const { RefCell::new(None) };
    /// Clé = (x, y locaux, largeur, hauteur, passes, INDEX DE MATÉRIAU) —
    /// `blur_passes()` ET `gfx::material::current_index()` font PARTIE de la
    /// clé plutôt que de forcer une invalidation manuelle : un changement du
    /// curseur "Intensité" ou du sélecteur "Matériau" en cours d'exécution
    /// construit simplement une nouvelle entrée pour la taille déjà vue
    /// (les anciennes entrées à d'autres réglages restent en cache,
    /// inutilisées mais inoffensives — quelques Mo de VRAM pour les
    /// combinaisons réellement essayées). L'INDEX de matériau, pas
    /// `(algo, stop_levels)` séparés : l'index détermine à lui seul l'algo
    /// ET les niveaux, donc il est impossible de désynchroniser la clé du
    /// contenu réellement construit par `build_pyramid` — deux champs
    /// séparés rouvriraient la classe de bug ci-dessous.
    ///
    /// BUG RÉEL CONSTATÉ EN PRATIQUE (signalé par l'utilisateur : "en
    /// déplaçant une fenêtre sous la barre de droite, je la vois passer sous
    /// la barre de gauche en même temps dans sa transparence") : la clé
    /// n'incluait AUPARAVANT que (largeur, hauteur, passes, niveaux) — sans
    /// position. Les barres latérales gauche et droite du dock ont
    /// exactement la même largeur (`left_width`/`right_width`, souvent
    /// égales, voir `ui::dock::bar_rects`) et la même hauteur
    /// (`total_height - top_thickness` pour les deux) : elles retombaient
    /// donc sur LA MÊME entrée de cache, donc LA MÊME texture GPU physique.
    /// `capture_bar_backdrops` traite haut/gauche/droite dans cet ordre à
    /// chaque frame (voir `app::capture_bar_backdrops`) : la capture de la
    /// barre droite écrasait `levels[0]` (et tous les niveaux dérivés) juste
    /// après que la barre gauche les ait remplis, avant que celle-ci soit
    /// composée à l'écran — la barre gauche affichait donc le flou de LA
    /// BARRE DROITE. `x`/`y` (coordonnées locales DÉJÀ recadrées dans
    /// `full_frame`, voir `local_x`/`local_y` ci-dessous) suffisent à lever
    /// l'ambiguïté : deux rectangles de même taille mais de position
    /// différente n'aliasent plus la même entrée, tout en restant mis en
    /// cache normalement d'une frame à l'autre pour UN MÊME rectangle
    /// (position ET taille stables tant que l'utilisateur ne redimensionne
    /// pas les barres/fenêtres, voir `win32::settings_dash`).
    static PYRAMIDS: RefCell<HashMap<(i32, i32, u32, u32, u32, usize), Pyramid>> = RefCell::new(HashMap::new());
}

/// Libère les pyramides de flou et le pipeline — mêmes raison et urgence que
/// `gfx::duplication::release`, à appeler au même moment (voir `app::run`).
///
/// Ce sont des `thread_local!`, donc détruits à la fin du THREAD, après
/// `app::run` et donc après le device D3D11 : les textures de rendu des
/// pyramides et les états du pipeline étaient encore vivants quand le device
/// mourait, ce que DXGI signale par une exception `0x87A` non rattrapée — le
/// processus mourait à chaque fermeture.
pub fn release_gpu_resources() {
    PYRAMIDS.with(|pyramids| pyramids.borrow_mut().clear());
    PIPELINE.with(|pipeline| *pipeline.borrow_mut() = None);
}

/// Recadre `rect` (coordonnées écran physiques absolues, comme
/// `desktop_rect`) depuis `full_frame` (voir `gfx::duplication::
/// ensure_frame`), puis le floute sur GPU (voir la doc de module) et
/// renvoie la texture résultat, prête à être échantillonnée par
/// `gfx::quad_renderer` (`QuadInstance::new_backdrop`).
///
/// `rect` est recadré défensivement dans les bornes de `full_frame` —
/// un rectangle demandé partiellement hors écran (bord d'un moniteur en
/// cours de reconfiguration, fenêtre en cours de déplacement...) ne doit
/// jamais produire une `D3D11_BOX` invalide.
pub(crate) fn render_backdrop(
    device: &ID3D11Device,
    ctx: &ID3D11DeviceContext,
    full_frame: &ID3D11Texture2D,
    desktop_rect: RECT,
    rect: [i32; 4],
) -> Result<ID3D11ShaderResourceView> {
    // `full_w`/`full_h` valent toujours >= 1 en pratique (un moniteur réel
    // n'a jamais de résolution nulle) — le `.max(1)` ci-dessous n'est qu'une
    // garde défensive pour ne jamais produire une `D3D11_BOX` invalide même
    // sur un descripteur de sortie DXGI dégénéré.
    let full_w = (desktop_rect.right - desktop_rect.left).max(1);
    let full_h = (desktop_rect.bottom - desktop_rect.top).max(1);

    // Recadrage en deux temps : d'abord `local_x`/`local_y` DANS [0, full-1]
    // (jamais exactement `full_w`/`full_h`, qui laisserait 0 pixel de large
    // restant), PUIS `w`/`h` bornée par l'espace qui reste à partir de là —
    // garantit `local_x + w <= full_w` (et pareil en y) dans tous les cas,
    // y compris un rect collé ou dépassant le bord du moniteur (ex. une
    // barre latérale du dock flush contre le bord droit de l'écran).
    let local_x = (rect[0] - desktop_rect.left).clamp(0, full_w - 1);
    let local_y = (rect[1] - desktop_rect.top).clamp(0, full_h - 1);
    let w = rect[2].max(1).min(full_w - local_x) as u32;
    let h = rect[3].max(1).min(full_h - local_y) as u32;

    // Marge de bavure (voir `BLEED_MARGIN`, doc de constante) : bornée par
    // l'espace RÉELLEMENT disponible de chaque côté sur le bureau, jamais
    // symétrisée de force — un rectangle collé au bord de l'écran (ex. une
    // barre latérale flush contre le bord droit) dégrade proprement vers 0
    // de marge de CE côté-là plutôt que de produire une `D3D11_BOX` hors
    // bornes.
    let pad_left = BLEED_MARGIN.min(local_x as u32);
    let pad_top = BLEED_MARGIN.min(local_y as u32);
    let pad_right = BLEED_MARGIN.min((full_w - local_x - w as i32).max(0) as u32);
    let pad_bottom = BLEED_MARGIN.min((full_h - local_y - h as i32).max(0) as u32);
    let padded_x = local_x - pad_left as i32;
    let padded_y = local_y - pad_top as i32;
    let padded_w = w + pad_left + pad_right;
    let padded_h = h + pad_top + pad_bottom;

    PIPELINE.with(|pipeline_cell| -> Result<ID3D11ShaderResourceView> {
        {
            let mut slot = pipeline_cell.borrow_mut();
            if slot.is_none() {
                *slot = Some(BlurPipeline::new(device)?);
            }
        }
        let pipeline_ref = pipeline_cell.borrow();
        let pipeline = pipeline_ref.as_ref().unwrap();

        PYRAMIDS.with(|pyramids_cell| -> Result<ID3D11ShaderResourceView> {
            let passes = blur_passes();
            let material_index = material::current_index();
            let material = material::current();
            // `local_x`/`local_y` FONT PARTIE de la clé — voir la doc de
            // `PYRAMIDS` : deux rectangles de même taille mais de position
            // différente (ex. barres gauche/droite du dock) ne doivent
            // jamais partager la même texture GPU.
            let key = (local_x, local_y, w, h, passes, material_index);
            let mut pyramids = pyramids_cell.borrow_mut();
            if !pyramids.contains_key(&key) {
                pyramids.insert(key, build_pyramid(device, padded_w, padded_h, w, h, passes, material)?);
            }
            let pyramid = pyramids.get(&key).unwrap();

            // Capture la zone AGRANDIE (voir `BLEED_MARGIN`) — pas juste
            // `rect` — dans `levels[0]`, désormais lui-même à la taille
            // agrandie (voir `build_pyramid`) : les passes de descente/
            // remontée qui suivent disposent ainsi de vrai contenu voisin à
            // leurs bords, pas d'une répétition du bord de `rect` lui-même.
            let box_ = D3D11_BOX {
                left: padded_x as u32,
                top: padded_y as u32,
                front: 0,
                right: padded_x as u32 + padded_w,
                bottom: padded_y as u32 + padded_h,
                back: 1,
            };
            unsafe {
                ctx.CopySubresourceRegion(&pyramid.levels[0].tex, 0, 0, 0, 0, full_frame, 0, Some(&box_));
            }

            let deepest = pyramid.levels.len() - 1;
            for i in 0..deepest {
                let halfpixel = [0.5 / pyramid.levels[i].width as f32, 0.5 / pyramid.levels[i].height as f32];
                draw_pass(ctx, pipeline, &pipeline.ps_down, &pyramid.levels[i].srv, &pyramid.levels[i + 1], halfpixel)?;
            }
            match material.algo {
                material::BlurAlgo::DualKawase => {
                    // Remontée Kawase — inchangée par rapport à l'algorithme
                    // historique, voir la doc de module.
                    for i in (pyramid.stop..deepest).rev() {
                        let halfpixel = [
                            0.5 / pyramid.levels[i + 1].width as f32,
                            0.5 / pyramid.levels[i + 1].height as f32,
                        ];
                        draw_pass(ctx, pipeline, &pipeline.ps_up, &pyramid.levels[i + 1].srv, &pyramid.levels[i], halfpixel)?;
                    }
                }
                material::BlurAlgo::MicaTent => {
                    // Une seule passe de tente large, du niveau le plus
                    // profond vers son parent immédiat — `build_pyramid` a
                    // déjà posé `stop = deepest - 1` pour ce cas.
                    if deepest > 0 {
                        let halfpixel = [
                            0.5 / pyramid.levels[deepest].width as f32,
                            0.5 / pyramid.levels[deepest].height as f32,
                        ];
                        draw_pass(
                            ctx,
                            pipeline,
                            &pipeline.ps_tent,
                            &pyramid.levels[deepest].srv,
                            &pyramid.levels[deepest - 1],
                            halfpixel,
                        )?;
                    }
                }
            }

            // Recadrage final (voir `BLEED_MARGIN`/`draw_crop_pass`) :
            // `levels[stop]` est encore à la taille AGRANDIE (proportionnellement
            // à `padded_w`/`padded_h`, chaque niveau ayant hérité du même
            // ratio de marge par halving successif) — ces fractions restent
            // valables quelle que soit la résolution réelle du niveau, donc
            // calculables une seule fois ici à partir de la géométrie de
            // `rect`, pas de `levels[stop].width`/`height`.
            let uv_scale = [w as f32 / padded_w as f32, h as f32 / padded_h as f32];
            let uv_offset = [pad_left as f32 / padded_w as f32, pad_top as f32 / padded_h as f32];
            draw_crop_pass(ctx, pipeline, &pyramid.levels[pyramid.stop].srv, &pyramid.final_crop, uv_scale, uv_offset)?;

            // Le dernier niveau écrit reste sinon lié comme render target —
            // conflit RTV/SRV garanti au prochain rendu qui l'échantillonne
            // (le composite de `gfx::quad_renderer` plus tard cette même
            // frame). Voir aussi le début de `draw_pass`, même précaution
            // entre deux passes consécutives de CETTE pyramide.
            unsafe { ctx.OMSetRenderTargets(None, None) };

            Ok(pyramid.final_crop.srv.clone())
        })
    })
}

/// Une passe de réduction ou de remontée : lit `src`, écrit dans tout
/// `dst` (`dst.width`×`dst.height`, le viewport couvre exactement ce
/// niveau).
fn draw_pass(
    ctx: &ID3D11DeviceContext,
    pipeline: &BlurPipeline,
    ps: &ID3D11PixelShader,
    src: &ID3D11ShaderResourceView,
    dst: &Level,
    half_pixel: [f32; 2],
) -> Result<()> {
    unsafe {
        // Voir la doc de `render_backdrop` : `src` peut encore être lié
        // comme render target par la passe précédente (la remontée relit
        // le niveau que la descente venait d'écrire) — le délier d'abord
        // évite l'avertissement/le résultat indéfini "ressource liée à la
        // fois en lecture et en écriture".
        ctx.OMSetRenderTargets(None, None);

        write_constants(ctx, &pipeline.constants, half_pixel, [0.0, 0.0])?;

        ctx.PSSetShaderResources(0, Some(&[Some(src.clone())]));

        let viewport = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: dst.width as f32,
            Height: dst.height as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        ctx.RSSetViewports(Some(&[viewport]));
        ctx.RSSetState(&pipeline.rasterizer_state);

        ctx.OMSetRenderTargets(Some(&[Some(dst.rtv.clone())]), None);
        ctx.OMSetBlendState(&pipeline.blend_state, None, 0xFFFFFFFF);

        ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        ctx.VSSetShader(&pipeline.vs, None);
        ctx.PSSetShader(ps, None);
        ctx.PSSetConstantBuffers(0, Some(&[Some(pipeline.constants.clone())]));
        ctx.PSSetSamplers(0, Some(&[Some(pipeline.sampler.clone())]));

        // Pas de tampon de sommets/index (voir la doc de module) : le
        // vertex shader ne lit que `SV_VertexID`.
        ctx.Draw(3, 0);
    }
    Ok(())
}

/// Dernière passe de `render_backdrop` (voir `BLEED_MARGIN`, doc de
/// constante et `PSCrop` dans `SHADER_SOURCE`) : ré-échantillonne `src`
/// (le dernier niveau de la pyramide, agrandi par la marge de bavure) vers
/// `dst` (`Pyramid::final_crop`, exactement à la taille du rectangle
/// demandé), sans flou additionnel — juste un recadrage par décalage/
/// mise à l'échelle d'UV.
fn draw_crop_pass(
    ctx: &ID3D11DeviceContext,
    pipeline: &BlurPipeline,
    src: &ID3D11ShaderResourceView,
    dst: &Level,
    uv_scale: [f32; 2],
    uv_offset: [f32; 2],
) -> Result<()> {
    unsafe {
        ctx.OMSetRenderTargets(None, None);

        write_constants(ctx, &pipeline.constants, uv_scale, uv_offset)?;

        ctx.PSSetShaderResources(0, Some(&[Some(src.clone())]));

        let viewport = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: dst.width as f32,
            Height: dst.height as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        ctx.RSSetViewports(Some(&[viewport]));
        ctx.RSSetState(&pipeline.rasterizer_state);

        ctx.OMSetRenderTargets(Some(&[Some(dst.rtv.clone())]), None);
        ctx.OMSetBlendState(&pipeline.blend_state, None, 0xFFFFFFFF);

        ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        ctx.VSSetShader(&pipeline.vs, None);
        ctx.PSSetShader(&pipeline.ps_crop, None);
        ctx.PSSetConstantBuffers(0, Some(&[Some(pipeline.constants.clone())]));
        ctx.PSSetSamplers(0, Some(&[Some(pipeline.sampler.clone())]));

        ctx.Draw(3, 0);
    }
    Ok(())
}

/// `content_w`/`content_h` : taille RÉELLE (agrandie par `BLEED_MARGIN`,
/// voir sa doc) des textures effectivement allouées à chaque niveau —
/// détermine le coût GPU/mémoire. `calib_w`/`calib_h` : taille D'ORIGINE,
/// non agrandie, du rectangle demandé — sert UNIQUEMENT à décider combien
/// de niveaux construire (`MIN_LEVEL_SIZE`), pour que `BLEED_MARGIN`
/// n'altère jamais la profondeur de pyramide calibrée pour chaque élément
/// (voir la doc de `MIN_LEVELS_HARD_CAP`/`MAX_LEVELS_HARD_CAP` : agrandir
/// `content_w`/`content_h` directement pousserait par exemple les barres
/// latérales, plus étroites que hautes, d'une profondeur `3` à `4` — une
/// régression non désirée par rapport à l'étalonnage documenté).
fn build_pyramid(
    device: &ID3D11Device,
    content_w: u32,
    content_h: u32,
    calib_w: u32,
    calib_h: u32,
    passes: u32,
    material: &material::Material,
) -> Result<Pyramid> {
    let mut levels = vec![create_level(device, content_w.max(1), content_h.max(1))?];
    let (mut w, mut h) = (content_w.max(1), content_h.max(1));
    let (mut cw, mut ch) = (calib_w.max(1), calib_h.max(1));
    // BUG RÉEL SIGNALÉ : "la valeur de flou ne semble pas s'appliquer au
    // dessus d'une certaine valeur [...] différence visuelle entre les
    // dashs et les barres". Un premier correctif (amplifier le rayon
    // d'échantillonnage à profondeur figée, voir l'historique de cette
    // fonction) restait visiblement différent du Dash — la largeur ET la
    // hauteur s'arrêtaient de descendre ENSEMBLE dès qu'UNE seule touchait
    // `MIN_LEVEL_SIZE`, alors qu'une barre du dock n'est étroite que dans
    // UNE dimension (sa hauteur pour la barre du haut, sa largeur pour les
    // barres latérales — voir `ui::topbar`/`ui::dock`) : sa largeur/hauteur
    // dans l'AUTRE sens reste comparable à celle du Dash. Chaque dimension
    // arrête maintenant de descendre INDÉPENDAMMENT dès qu'elle atteint
    // `MIN_LEVEL_SIZE`, l'autre continuant seule — la pyramide d'une barre
    // atteint ainsi une profondeur RÉELLE bien plus proche de celle du
    // Dash pour le même `passes`, au lieu d'un rayon amplifié artificiel
    // sur une poignée de niveaux figés.
    while (levels.len() as u32) < passes {
        // On ne retient une moitié que si elle reste À ou AU-DESSUS de
        // `MIN_LEVEL_SIZE` — jamais en dessous (voir sa doc), exactement
        // comme l'ancien garde-fou commun aux deux dimensions, mais
        // évalué ici indépendamment pour chacune.
        let next_cw_candidate = (cw / 2).max(1);
        let next_ch_candidate = (ch / 2).max(1);
        let shrink_w = next_cw_candidate >= MIN_LEVEL_SIZE;
        let shrink_h = next_ch_candidate >= MIN_LEVEL_SIZE;
        if !shrink_w && !shrink_h {
            break;
        }
        let next_w = if shrink_w { (w / 2).max(1) } else { w };
        let next_h = if shrink_h { (h / 2).max(1) } else { h };
        let next_cw = if shrink_w { next_cw_candidate } else { cw };
        let next_ch = if shrink_h { next_ch_candidate } else { ch };
        levels.push(create_level(device, next_w, next_h)?);
        w = next_w;
        h = next_h;
        cw = next_cw;
        ch = next_ch;
    }
    let deepest = levels.len() - 1;
    let stop = match material.algo {
        material::BlurAlgo::DualKawase => deepest.saturating_sub(material.stop_levels),
        // Une seule passe de tente : le résultat vit un cran au-dessus du
        // niveau le plus profond, quelle que soit l'"Intensité" choisie.
        // `saturating_sub(1)` couvre la pyramide dégénérée à un seul niveau
        // (rectangle plus petit que `MIN_LEVEL_SIZE`) — `render_backdrop`
        // saute alors la passe de tente (`if deepest > 0`) et renvoie
        // directement `levels[0]`.
        material::BlurAlgo::MicaTent => deepest.saturating_sub(1),
    };
    debug_assert!(stop < levels.len());
    // Destination du recadrage final (voir `draw_crop_pass`) — TOUJOURS à
    // la taille d'origine `calib_w`×`calib_h`, jamais agrandie par la marge
    // de bavure : c'est ce qui garde `Texture` (le type renvoyé à
    // l'appelant, voir `render_backdrop`) à une taille identique à celle
    // d'avant `BLEED_MARGIN`, sans toucher `gfx::quad_renderer` ni aucun
    // site d'appel.
    let final_crop = create_level(device, calib_w.max(1), calib_h.max(1))?;
    Ok(Pyramid { levels, stop, final_crop })
}

fn create_level(device: &ID3D11Device, width: u32, height: u32) -> Result<Level> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut tex = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut tex))? };
    let tex = tex.unwrap();

    let mut rtv = None;
    unsafe { device.CreateRenderTargetView(&tex, None, Some(&mut rtv))? };
    let mut srv = None;
    unsafe { device.CreateShaderResourceView(&tex, None, Some(&mut srv))? };

    Ok(Level {
        tex,
        rtv: rtv.unwrap(),
        srv: srv.unwrap(),
        width,
        height,
    })
}

/// Écrit les deux `float2` génériques du cbuffer partagé — voir la doc de
/// `BlurConstants` pour ce que chaque appelant y met réellement
/// (`half_pixel`/zéro pour `PSDown`/`PSUp`/`PSTent` via `draw_pass`,
/// `uv_scale`/`uv_offset` pour `PSCrop` via `draw_crop_pass`).
unsafe fn write_constants(ctx: &ID3D11DeviceContext, buffer: &ID3D11Buffer, param0: [f32; 2], param1: [f32; 2]) -> Result<()> {
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    ctx.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
    let constants = BlurConstants { param0, param1 };
    std::ptr::copy_nonoverlapping(&constants, mapped.pData as *mut BlurConstants, 1);
    ctx.Unmap(buffer, 0);
    Ok(())
}

impl BlurPipeline {
    fn new(device: &ID3D11Device) -> Result<Self> {
        let vs_blob = compile_shader(SHADER_SOURCE, "VSMain", "vs_5_0")?;
        let ps_down_blob = compile_shader(SHADER_SOURCE, "PSDown", "ps_5_0")?;
        let ps_up_blob = compile_shader(SHADER_SOURCE, "PSUp", "ps_5_0")?;
        let ps_tent_blob = compile_shader(SHADER_SOURCE, "PSTent", "ps_5_0")?;

        let vs = unsafe {
            let mut out = None;
            device.CreateVertexShader(blob_bytes(&vs_blob), None, Some(&mut out))?;
            out.unwrap()
        };
        let ps_down = unsafe {
            let mut out = None;
            device.CreatePixelShader(blob_bytes(&ps_down_blob), None, Some(&mut out))?;
            out.unwrap()
        };
        let ps_up = unsafe {
            let mut out = None;
            device.CreatePixelShader(blob_bytes(&ps_up_blob), None, Some(&mut out))?;
            out.unwrap()
        };
        let ps_tent = unsafe {
            let mut out = None;
            device.CreatePixelShader(blob_bytes(&ps_tent_blob), None, Some(&mut out))?;
            out.unwrap()
        };
        let ps_crop_blob = compile_shader(SHADER_SOURCE, "PSCrop", "ps_5_0")?;
        let ps_crop = unsafe {
            let mut out = None;
            device.CreatePixelShader(blob_bytes(&ps_crop_blob), None, Some(&mut out))?;
            out.unwrap()
        };

        let sampler = crate::gfx::texture::create_sampler_state(device)?;
        let constants = create_constant_buffer(device)?;
        let blend_state = create_opaque_blend_state(device)?;
        let rasterizer_state = create_rasterizer_state(device)?;

        Ok(Self {
            vs,
            ps_down,
            ps_up,
            ps_tent,
            ps_crop,
            sampler,
            constants,
            blend_state,
            rasterizer_state,
        })
    }
}

fn create_constant_buffer(device: &ID3D11Device) -> Result<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: size_of::<BlurConstants>() as u32,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer))? };
    Ok(buffer.unwrap())
}

/// Chaque passe réécrit intégralement son render target — pas de mélange
/// nécessaire (contrairement à `gfx::quad_renderer`, dont le blend state
/// alpha prémultiplié n'a pas de sens ici).
///
/// BUG RÉEL CONSTATÉ EN PRATIQUE (fond des barres totalement noir, voir
/// l'historique) : `D3D11_BLEND_DESC::default()` met TOUS les champs à
/// zéro, y compris `RenderTarget[0].RenderTargetWriteMask` — qui vaut donc
/// 0 (AUCUN canal couleur écrit) plutôt que "tous les canaux" comme on
/// pourrait le supposer de `BlendEnable: FALSE` seul. Chaque passe
/// `draw_pass` s'exécutait sans jamais rien écrire dans son render target,
/// qui restait à son contenu initial (zéro) — `level0` (rempli par
/// `CopySubresourceRegion`, pas par un draw call) contenait bien la bonne
/// image, mais tout niveau produit par une passe Down/Up restait
/// (0,0,0,0), d'où un résultat final toujours transparent-noir composé en
/// façade opaque par `gfx::quad_renderer` (`TexMode::Backdrop` force
/// alpha=1.0). Il faut mettre `RenderTargetWriteMask` explicitement à
/// `D3D11_COLOR_WRITE_ENABLE_ALL`.
fn create_opaque_blend_state(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    let mut state = None;
    unsafe { device.CreateBlendState(&desc, Some(&mut state))? };
    Ok(state.unwrap())
}

fn create_rasterizer_state(device: &ID3D11Device) -> Result<ID3D11RasterizerState> {
    let desc = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_NONE,
        DepthClipEnable: true.into(),
        ScissorEnable: false.into(),
        ..Default::default()
    };
    let mut state = None;
    unsafe { device.CreateRasterizerState(&desc, Some(&mut state))? };
    Ok(state.unwrap())
}
