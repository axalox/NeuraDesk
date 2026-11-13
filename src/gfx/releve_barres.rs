//! ⭐ **LA COULEUR RENDUE DES BARRES** — DÉFAUT SIGNALÉ (2026-10-19) : « l'ombre
//! de couleur qui vient de la barre du haut ne suit pas la couleur de la barre du
//! haut automatiquement ». Les ombres colorées (`win32::ombre_barre`) prenaient la
//! TEINTE réglée de chaque barre ; or la barre peinte la mêle au bureau flouté
//! (`bar_tint`), puis aux lueurs, au dégradé et au matériau (voir
//! `gfx::quad_renderer`, `PSMain`). Mesuré à l'écran : barre du haut rosée
//! (145, 90, 80), son ombre ocre (155, 112, 53) — la teinte brute de
//! `bar_colors.txt`, opaque à 40 % seulement.
//!
//! Plutôt que de refaire ce calcul hors du shader, on RELIT ce qu'il a peint :
//! une bande de quelques pixels au bord intérieur de chaque barre, dans le back
//! buffer du dock, réduite en un `Profil` de `SEGMENTS` couleurs le long de la
//! barre — médiane par segment, pour qu'une icône ou un texte qui déborde dans la
//! bande n'y pèse pas.
//!
//! ⛔ **PAS LES ICÔNES** — DÉFAUT SIGNALÉ (2026-10-19, capture `image15.png`) :
//! « un défaut avec l'ombre de couleur du bas à droite ». Le coin arrondi y était
//! vert-gris : au bas de la colonne de droite, les icônes touchent son bord
//! intérieur et couvraient la bande presque entière — la médiane prenait le fond
//! turquoise d'une icône. Un pixel ne compte plus que s'il est PROCHE de la
//! teinte réglée de sa barre (`ECART_MAX`), et parmi eux le tiers le plus proche
//! de leur médiane (`PART_GARDEE`) : le fond de la barre, que le rendu ne fait que
//! nuancer. ⛔ Le tiers le plus proche de la TEINTE donnait le grain le plus sombre
//! de la texture (mesuré le 2026-10-19 : barre (26, 87, 178), profil (22, 82, 166)).
//!
//! ⛔ **HORS DES OMBRES** : le dock peint les ombres des barres sur leurs fonds
//! (voir `win32::ombre_barre::Calque`) — une bande relue sous une ombre en
//! prendrait la couleur, et la suivante l'assombrirait encore. Ces pixels sont
//! écartés (`win32::ombre_barre::ombree`).
//!
//! ⚠️ **ASYNCHRONE** : la copie GPU vers des textures staging part au plus toutes
//! les `PERIODE`, et se relit aux frames suivantes SANS attendre
//! (`D3D11_MAP_FLAG_DO_NOT_WAIT`) — le rendu du dock ne s'arrête jamais pour elle.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::DXGI_ERROR_WAS_STILL_DRAWING;

use crate::ui::dock::DockLayout;

/// Le nombre de couleurs d'un profil, le long de sa barre.
pub const SEGMENTS: usize = 32;

/// Les couleurs RGB d'une barre, de son début (gauche, ou haut) à sa fin.
pub type Profil = [[u8; 3]; SEGMENTS];

/// L'intervalle entre deux relevés : les lueurs « respirent » lentement, et
/// chaque nouveau profil fait repeindre les ombres.
const PERIODE: Duration = Duration::from_millis(250);

/// L'épaisseur de la bande relue, et son retrait depuis le bord intérieur de la
/// barre : le liseré du biseau y assombrit les tout derniers pixels.
const BANDE: u32 = 4;
const RETRAIT: u32 = 4;

/// Un pixel plus transparent ne compte pas : le bord d'un congé, une barre
/// absente du back buffer.
const ALPHA_MIN: u8 = 250;

/// L'écart (somme des trois canaux) au-delà duquel un pixel n'est pas le fond de
/// sa barre mais une icône ou un texte. Le rendu nuance la teinte réglée de 60
/// environ (mesuré le 2026-10-19) ; une icône d'une autre couleur s'en écarte de
/// 200 et plus.
const ECART_MAX: u32 = 120;

/// La part des pixels d'un segment gardée pour sa médiane : les plus proches du
/// centre des pixels retenus.
const PART_GARDEE: usize = 3;

/// Une bande à relire, en pixels de la fenêtre : `[x, y, largeur, hauteur]`.
pub type Bande = [u32; 4];

/// L'étendue de chaque barre `[haut, gauche, droite, bas]` le long d'elle-même,
/// `[début, longueur]` en pixels de la fenêtre : la barre du haut sur toute la
/// largeur, les colonnes SOUS elle (`haut`), la barre du bas ENTRE les colonnes
/// visibles (voir `ui::dock::bar_rects`). Un profil couvre exactement cette
/// étendue — ⛔ courant sur toute la fenêtre, celui d'une colonne prenait la
/// couleur de la barre du haut à son bout, et c'est ce bout qui colore les coins
/// arrondis (défaut signalé le 2026-10-19).
pub fn etendues(largeur: f32, hauteur: f32, haut: f32, gauche: f32, droite: f32) -> [[f32; 2]; 4] {
    let colonne = [haut, (hauteur - haut).max(0.0)];
    [[0.0, largeur], colonne, colonne, [gauche, (largeur - gauche - droite).max(0.0)]]
}

/// Les bandes à relire `[haut, gauche, droite, bas]`, au bord intérieur de chaque
/// barre et sur son étendue (`etendues`), aux épaisseurs VISIBLES `visibles` (une
/// barre masquée n'a pas de bande).
pub fn bandes(layout: &DockLayout, [_, gauche, droite, dessous]: [f32; 4]) -> [Option<Bande>; 4] {
    let (largeur, hauteur) = (layout.total_width.round().max(0.0) as u32, layout.total_height.round().max(0.0) as u32);
    let px = |v: f32| v.round().max(0.0) as u32;
    let assez = |e: u32| e > RETRAIT + BANDE;
    let (gauche, droite, dessous) = (gauche.min(layout.left_width), droite.min(layout.right_width), dessous.min(layout.bottom_thickness));
    let [le_haut, colonne, _, le_bas] = etendues(layout.total_width, layout.total_height, layout.haut_colonnes, gauche, droite);
    let haut = px(layout.haut_colonnes);
    let (gauche, droite, dessous) = (px(gauche), px(droite), px(dessous));
    let [(x0, lh), (y0, lc), (xb, lb)] = [le_haut, colonne, le_bas].map(|[debut, longueur]| (px(debut), px(longueur)));
    [
        (assez(haut) && lh > 0).then(|| [x0, haut - RETRAIT - BANDE, lh, BANDE]),
        (assez(gauche) && lc > 0).then(|| [gauche - RETRAIT - BANDE, y0, BANDE, lc]),
        (assez(droite) && largeur >= droite && lc > 0).then(|| [largeur - droite + RETRAIT, y0, BANDE, lc]),
        (assez(dessous) && hauteur >= dessous && lb > 0).then(|| [xb, hauteur - dessous + RETRAIT, lb, BANDE]),
    ]
}

/// Le profil d'une bande relue (`pixels` en BGRA, `pas` octets par ligne) : la
/// médiane de chaque canal par segment de sa plus grande dimension. Un segment
/// sans pixel opaque prend la couleur du plus proche qui en a ; `None` si aucun.
/// `ecarte(x, y)` : un pixel de la bande à ne pas compter (sous une ombre).
/// `reference` : la teinte réglée de la barre, RGB — ne comptent que les pixels
/// proches d'elle (voir la doc de module). Un segment qui n'en a aucun prend son
/// voisin s'il en a un à `VOISINAGE` segments au plus — une icône est étroite ;
/// sinon, son tiers le plus proche : un long pan de barre loin de la teinte (un
/// dégradé, un flou très coloré) est bien la barre.
///
/// ⛔ **UNE TEINTE RÉGLÉE QUI NE DÉCRIT PAS LA BARRE** — DÉFAUT SIGNALÉ
/// (2026-11-13) : « l'ombrage de couleur de la barre de gauche semble être
/// calculé à partir de la couleur de la zone centrale de la barre du haut, celui
/// de la barre de droite réagit à la couleur de mise en avant des icônes ». Une
/// barre texturée ou transparente peut être RENDUE loin de sa teinte (mesuré :
/// réglée orange, rendue rouge sombre) : seuls quelques détails passaient alors
/// le filtre (7 segments sur 32 en haut, 4 sur 32 à droite), et le voisinage les
/// recopiait sur la barre. Quand moins de la moitié des segments ont un pixel
/// proche, la référence devient la MÉDIANE de la bande : c'est elle, la barre.
pub fn profil(pixels: &[u8], pas: usize, largeur: usize, hauteur: usize, reference: [u8; 3], ecarte: impl Fn(usize, usize) -> bool) -> Option<Profil> {
    let compte = |s: &[Option<[u8; 3]>; SEGMENTS]| s.iter().filter(|c| c.is_some()).count();
    let tous = segments_de(pixels, pas, largeur, hauteur, reference, None, &ecarte)?;
    let pres_de_la_teinte = segments_de(pixels, pas, largeur, hauteur, reference, Some(ECART_MAX), &ecarte)?;
    // ⛔ Le CENTRE du filtre est la barre telle que RENDUE (voir le test
    // `une_pastille_proche_de_la_teinte_reglee…`) : la médiane des pixels proches
    // de la teinte réglée, ou de toute la bande s'ils sont trop rares.
    let assez = compte(&pres_de_la_teinte) * 2 >= compte(&tous);
    let centre = mediane_de_la_bande(pixels, pas, largeur, hauteur, assez.then_some(reference), &ecarte).unwrap_or(reference);
    let libres = segments_de(pixels, pas, largeur, hauteur, centre, None, &ecarte)?;
    let proches = segments_de(pixels, pas, largeur, hauteur, centre, Some(ECART_MAX), &ecarte)?;
    let segments: [Option<[u8; 3]>; SEGMENTS] = std::array::from_fn(|k| {
        let voisin = (0..=VOISINAGE).find_map(|d| proches[k.saturating_sub(d)].or(proches[(k + d).min(SEGMENTS - 1)]));
        // Un COURT pan loin de la barre (une pastille, un groupe d'icônes) prend
        // le segment proche le plus près ; un LONG pan (un dégradé) est la barre.
        let pan = sans_proche_autour(&proches, k);
        let plus_pres = || (pan < PAN_DE_BARRE).then(|| (0..SEGMENTS).filter(|&c| proches[c].is_some()).min_by_key(|&c| c.abs_diff(k))).flatten().and_then(|c| proches[c]);
        voisin.or_else(plus_pres).or(libres[k])
    });
    let connus: Vec<usize> = (0..SEGMENTS).filter(|&k| segments[k].is_some()).collect();
    if connus.is_empty() {
        return None;
    }
    Some(std::array::from_fn(|k| {
        let proche = connus.iter().min_by_key(|&&c| c.abs_diff(k)).copied().unwrap_or(k);
        segments[proche].unwrap_or_default()
    }))
}

/// Jusqu'où un segment sans pixel proche de la teinte cherche un voisin qui en a.
const VOISINAGE: usize = 2;

/// La longueur (en segments) à partir de laquelle une suite de segments sans
/// pixel proche de la barre est un pan de la barre (un dégradé), et non ce qui la
/// couvre. MESURÉ (2026-11-13) : les pastilles du pied de la colonne gauche
/// couvraient 3 segments ; un dégradé couvre des quarts de barre.
const PAN_DE_BARRE: usize = SEGMENTS / 4;

/// Le nombre de segments consécutifs sans `proches` autour de `k` (`k` compris).
fn sans_proche_autour(proches: &[Option<[u8; 3]>; SEGMENTS], k: usize) -> usize {
    if proches[k].is_some() {
        return 0;
    }
    let avant = (0..k).rev().take_while(|&c| proches[c].is_none()).count();
    let apres = (k + 1..SEGMENTS).take_while(|&c| proches[c].is_none()).count();
    avant + 1 + apres
}

/// La médiane de chaque segment (voir `profil`), parmi les pixels à `ecart_max`
/// au plus de `reference` (`None` : tous) — `None` par segment sans pixel ; `None`
/// tout court pour une bande vide.
fn segments_de(
    pixels: &[u8],
    pas: usize,
    largeur: usize,
    hauteur: usize,
    reference: [u8; 3],
    ecart_max: Option<u32>,
    ecarte: &impl Fn(usize, usize) -> bool,
) -> Option<[Option<[u8; 3]>; SEGMENTS]> {
    let ecart = |c: &[u8; 3], r: [u8; 3]| c.iter().zip(r).map(|(a, b)| a.abs_diff(b) as u32).sum::<u32>();
    let horizontale = largeur >= hauteur;
    let longueur = if horizontale { largeur } else { hauteur };
    if longueur == 0 {
        return None;
    }
    let mut segments: [Option<[u8; 3]>; SEGMENTS] = [None; SEGMENTS];
    let mut couleurs: Vec<[u8; 3]> = Vec::new();
    let mut canaux: [Vec<u8>; 3] = Default::default();
    for (k, segment) in segments.iter_mut().enumerate() {
        couleurs.clear();
        for le_long in k * longueur / SEGMENTS..((k + 1) * longueur / SEGMENTS).max(k * longueur / SEGMENTS + 1).min(longueur) {
            for travers in 0..if horizontale { hauteur } else { largeur } {
                let (x, y) = if horizontale { (le_long, travers) } else { (travers, le_long) };
                let Some(p) = pixels.get(y * pas + x * 4..y * pas + x * 4 + 4) else { continue };
                // BGRA vers RGB.
                let c = [p[2], p[1], p[0]];
                if p[3] >= ALPHA_MIN && !ecarte(x, y) && ecart_max.is_none_or(|m| ecart(&c, reference) <= m) {
                    couleurs.push(c);
                }
            }
        }
        if couleurs.is_empty() {
            continue;
        }
        // ⛔ Le tiers le plus proche du CENTRE des pixels retenus, pas de la
        // teinte : plus sombre que la barre texturée, elle choisissait le grain
        // le plus sombre (voir le test `le_profil_est_le_centre_du_grain…`).
        let centre = if ecart_max.is_some() { mediane(&couleurs, &mut canaux) } else { reference };
        couleurs.sort_by_key(|c| ecart(c, centre));
        *segment = Some(mediane(&couleurs[..couleurs.len().div_ceil(PART_GARDEE)], &mut canaux));
    }
    Some(segments)
}

/// La médiane de chaque canal de `couleurs` (non vide) ; `canaux` : des tampons.
fn mediane(couleurs: &[[u8; 3]], canaux: &mut [Vec<u8>; 3]) -> [u8; 3] {
    for (c, canal) in canaux.iter_mut().enumerate() {
        canal.clear();
        canal.extend(couleurs.iter().map(|p| p[c]));
    }
    std::array::from_fn(|c| {
        let milieu = canaux[c].len() / 2;
        *canaux[c].select_nth_unstable(milieu).1
    })
}

/// Le profil d'une bande (voir `profil`), sans les pixels `ombree` ni ceux des
/// cases d'`icone` — SAUF si ces cases ne laissent pas au moins la moitié des
/// segments qui ont un pixel hors de l'ombre : leurs trous ne sont alors pas la
/// barre, mais ce qui luit entre deux icônes. MESURÉ (2026-11-13, colonne de
/// droite) : deux segments sur 32 restaient, et donnaient sa couleur à toute la
/// colonne. Sans les cases, la médiane de la bande (voir `profil`) fait le tri.
pub fn profil_de_la_bande(
    pixels: &[u8],
    pas: usize,
    largeur: usize,
    hauteur: usize,
    reference: [u8; 3],
    ombree: impl Fn(usize, usize) -> bool,
    icone: impl Fn(usize, usize) -> bool,
) -> Option<Profil> {
    let avec_les_cases = |x, y| ombree(x, y) || icone(x, y);
    let couverts = |ecarte: &dyn Fn(usize, usize) -> bool| {
        segments_de(pixels, pas, largeur, hauteur, reference, None, &ecarte).map_or(0, |s| s.iter().filter(|c| c.is_some()).count())
    };
    if couverts(&avec_les_cases) * 2 >= couverts(&ombree) {
        profil(pixels, pas, largeur, hauteur, reference, avec_les_cases)
    } else {
        profil(pixels, pas, largeur, hauteur, reference, ombree)
    }
}

/// La médiane de chaque canal sur toute la bande, parmi les pixels opaques et non
/// écartés — et, avec `pres_de`, à `ECART_MAX` au plus de cette couleur. `None`
/// sans pixel (voir `profil`).
fn mediane_de_la_bande(pixels: &[u8], pas: usize, largeur: usize, hauteur: usize, pres_de: Option<[u8; 3]>, ecarte: &impl Fn(usize, usize) -> bool) -> Option<[u8; 3]> {
    let ecart = |c: [u8; 3], r: [u8; 3]| c.iter().zip(r).map(|(a, b)| a.abs_diff(b) as u32).sum::<u32>();
    let couleurs: Vec<[u8; 3]> = (0..hauteur)
        .flat_map(|y| (0..largeur).map(move |x| (x, y)))
        .filter_map(|(x, y)| {
            let p = pixels.get(y * pas + x * 4..y * pas + x * 4 + 4)?;
            let c = [p[2], p[1], p[0]];
            (p[3] >= ALPHA_MIN && !ecarte(x, y) && pres_de.is_none_or(|r| ecart(c, r) <= ECART_MAX)).then_some(c)
        })
        .collect();
    (!couleurs.is_empty()).then(|| mediane(&couleurs, &mut Default::default()))
}

/// Une bande en cours de copie, et la texture staging qui la reçoit.
struct Copie {
    staging: ID3D11Texture2D,
    /// Le coin de la bande dans la fenêtre, et sa taille.
    coin: (u32, u32),
    taille: (u32, u32),
    /// Les cases des icônes au moment de la copie (voir `relever`).
    icones: Vec<[f32; 4]>,
}

#[derive(Default)]
struct Releve {
    copies: [Option<Copie>; 4],
    /// Des copies attendent d'être relues.
    en_vol: bool,
    depuis: Option<Instant>,
    profils: [Option<Profil>; 4],
    /// Les teintes réglées des barres, RGB (voir `profil`).
    references: [[u8; 3]; 4],
}

thread_local! {
    /// Un relevé par fenêtre de dock (la clé : son `HWND`).
    static RELEVES: RefCell<HashMap<isize, Releve>> = RefCell::new(HashMap::new());
}

/// Rend au device les textures de relevé que ce cache détient — **avant la destruction du
/// device** (voir `app::GpuCacheGuard`) : sinon elle ne mourait qu'au démontage
/// du processus, et DXGI y faisait sortir NeuraDesk en code 122.
pub fn liberer_gpu() {
    RELEVES.with(|r| {
        if let Ok(mut r) = r.try_borrow_mut() {
            r.clear();
        }
    });
}

/// Les derniers profils relevés pour la fenêtre `cle` — `None` par barre tant
/// qu'aucun relevé n'a abouti.
pub fn profils(cle: isize) -> [Option<Profil>; 4] {
    RELEVES.with(|r| r.try_borrow().ok().and_then(|r| r.get(&cle).map(|releve| releve.profils)).unwrap_or([None; 4]))
}

/// À appeler APRÈS le rendu de la fenêtre `cle` dans `source` (son back buffer,
/// BGRA), AVANT de le présenter : relit les copies arrivées, et en lance de
/// nouvelles toutes les `PERIODE`. `teintes` : la teinte réglée de chaque barre
/// (RGB 0..=1), dont la couleur relue ne s'écarte que peu (voir `profil`).
///
/// ⛔ **`icones` : LES CASES DES ICÔNES, QUI NE SONT PAS LA BARRE** — DEMANDE
/// EXPLICITE (2026-10-23) : « les ombres de couleur ne doivent pas prendre en
/// compte les icônes dans le calcul de la couleur ». Le filtre par la teinte
/// (`ECART_MAX`) ne suffit pas : sur une barre en DÉGRADÉ la teinte réglée est loin
/// du rendu, et les pastilles colorées des icônes de la colonne gauche couvrent
/// toute sa largeur, jusqu'à la bande. Un pixel dans une case (élargie de
/// `MARGE_ICONE`, pour la pastille) ne compte plus ; un segment sans autre pixel
/// prend son voisin (voir `profil`).
pub fn relever(
    cle: isize,
    device: &ID3D11Device,
    ctx: &ID3D11DeviceContext,
    source: &ID3D11Texture2D,
    bandes: [Option<Bande>; 4],
    teintes: [[f32; 3]; 4],
    icones: Vec<[f32; 4]>,
) {
    RELEVES.with(|r| {
        let Ok(mut releves) = r.try_borrow_mut() else { return };
        let releve = releves.entry(cle).or_default();
        releve.references = teintes.map(|t| t.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
        if releve.en_vol {
            if !lire(cle, ctx, releve) {
                return;
            }
            releve.en_vol = false;
        }
        if releve.depuis.is_some_and(|t| t.elapsed() < PERIODE) {
            return;
        }
        releve.depuis = Some(Instant::now());
        copier(device, ctx, source, bandes, releve);
        for copie in releve.copies.iter_mut().flatten() {
            copie.icones.clone_from(&icones);
        }
    });
}

/// L'élargissement des cases d'icône, en part de leur côté : la pastille d'une
/// icône ouverte et son liseré débordent de la case.
const MARGE_ICONE: f32 = 0.25;

/// Le pixel `(x, y)` (fenêtre) est-il dans la case d'une icône, élargie ?
fn dans_une_icone(icones: &[[f32; 4]], x: f32, y: f32) -> bool {
    icones.iter().any(|&[ix, iy, l, h]| {
        let (mx, my) = (l * MARGE_ICONE, h * MARGE_ICONE);
        x >= ix - mx && x < ix + l + mx && y >= iy - my && y < iy + h + my
    })
}

/// Relit les copies en vol ; `false` si le GPU ne les a pas encore faites.
fn lire(cle: isize, ctx: &ID3D11DeviceContext, releve: &mut Releve) -> bool {
    for ((copie, profil_lu), reference) in releve.copies.iter().zip(releve.profils.iter_mut()).zip(releve.references) {
        let Some(copie) = copie else {
            *profil_lu = None;
            continue;
        };
        let mut lu = D3D11_MAPPED_SUBRESOURCE::default();
        match unsafe { ctx.Map(&copie.staging, 0, D3D11_MAP_READ, D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32, Some(&mut lu)) } {
            Ok(()) => {
                let (largeur, hauteur) = (copie.taille.0 as usize, copie.taille.1 as usize);
                let pas = lu.RowPitch as usize;
                let octets = unsafe { std::slice::from_raw_parts(lu.pData as *const u8, pas * (hauteur - 1) + largeur * 4) };
                let (x0, y0) = copie.coin;
                let neuf = profil_de_la_bande(
                    octets,
                    pas,
                    largeur,
                    hauteur,
                    reference,
                    |x, y| crate::win32::ombre_barre::ombree(cle, x0 + x as u32, y0 + y as u32),
                    |x, y| dans_une_icone(&copie.icones, (x0 + x as u32) as f32 + 0.5, (y0 + y as u32) as f32 + 0.5),
                );
                *profil_lu = garder_si_proche(*profil_lu, neuf);
                unsafe { ctx.Unmap(&copie.staging, 0) };
            }
            Err(e) if e.code() == DXGI_ERROR_WAS_STILL_DRAWING => return false,
            Err(_) => *profil_lu = None,
        }
    }
    true
}

/// L'écart (par canal, sur 255) en dessous duquel un nouveau profil ne remplace
/// pas l'ancien. Une variation de cet ordre ne se voit pas dans une ombre, et
/// chaque nouveau profil fait recomposer les ombres colorées (plusieurs
/// dizaines de ms en 4K).
const ECART_VISIBLE: u8 = 12;

/// ⭐ **UN PROFIL PRESQUE IDENTIQUE N'EST PAS UN NOUVEAU PROFIL** — DÉFAUT RÉEL
/// MESURÉ (2026-11-13, « ce n'est pas fluide ») : le relevé rendait à chaque
/// `PERIODE` un profil qui différait de quelques unités sur quelques segments,
/// et chaque différence faisait recomposer toutes les ombres colorées (~400 ms
/// sur 4K à l'époque) : la boucle ne tournait plus que 3 fois par seconde.
/// L'ancien profil est gardé tant qu'aucun segment ne s'en écarte de plus de
/// `ECART_VISIBLE` ; la comparaison se fait contre le profil GARDÉ, donc une
/// dérive lente finit bien par passer.
fn garder_si_proche(ancien: Option<Profil>, neuf: Option<Profil>) -> Option<Profil> {
    match (ancien, neuf) {
        (Some(a), Some(n)) if a.iter().zip(n.iter()).all(|(a, n)| a.iter().zip(n.iter()).all(|(a, n)| a.abs_diff(*n) <= ECART_VISIBLE)) => Some(a),
        _ => neuf,
    }
}

/// Lance la copie de chaque bande (bornée au back buffer) vers sa texture
/// staging, recréée quand sa taille change.
fn copier(device: &ID3D11Device, ctx: &ID3D11DeviceContext, source: &ID3D11Texture2D, bandes: [Option<Bande>; 4], releve: &mut Releve) {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { source.GetDesc(&mut desc) };
    for (bande, copie) in bandes.iter().zip(releve.copies.iter_mut()) {
        let Some([x, y, l, h]) = *bande else {
            *copie = None;
            continue;
        };
        let (droite, bas) = ((x + l).min(desc.Width), (y + h).min(desc.Height));
        if droite <= x || bas <= y {
            *copie = None;
            continue;
        }
        let taille = (droite - x, bas - y);
        if copie.as_ref().is_none_or(|c| c.taille != taille) {
            *copie = staging(device, &desc, taille).map(|staging| Copie { staging, coin: (x, y), taille, icones: Vec::new() });
        }
        let Some(c) = copie else { continue };
        c.coin = (x, y);
        let zone = D3D11_BOX { left: x, top: y, front: 0, right: droite, bottom: bas, back: 1 };
        unsafe { ctx.CopySubresourceRegion(&c.staging, 0, 0, 0, 0, source, 0, Some(&zone)) };
    }
    releve.en_vol = releve.copies.iter().any(Option::is_some);
    if !releve.en_vol {
        releve.profils = [None; 4];
    }
}

fn staging(device: &ID3D11Device, source: &D3D11_TEXTURE2D_DESC, (largeur, hauteur): (u32, u32)) -> Option<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: largeur,
        Height: hauteur,
        MipLevels: 1,
        ArraySize: 1,
        Format: source.Format,
        SampleDesc: source.SampleDesc,
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }.ok()?;
    texture
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une teinte réglée loin de toutes les couleurs des tests : sans pixel
    /// proche, chaque segment garde son tiers le plus proche (voir `profil`).
    const GRIS: [u8; 3] = [0, 0, 0];

    /// Une bande BGRA de `largeur`×`hauteur`, chaque pixel donné par `couleur(x, y)` (RGBA).
    fn bande(largeur: usize, hauteur: usize, couleur: impl Fn(usize, usize) -> [u8; 4]) -> Vec<u8> {
        (0..hauteur).flat_map(|y| (0..largeur).map(move |x| (x, y))).flat_map(|(x, y)| {
            let [r, g, b, a] = couleur(x, y);
            [b, g, r, a]
        }).collect()
    }

    /// ⭐ Le profil suit la couleur LE LONG de la barre : la moitié gauche rose,
    /// la droite violette.
    #[test]
    fn le_profil_suit_la_couleur_le_long_de_la_barre() {
        let px = bande(640, 4, |x, _| if x < 320 { [145, 90, 80, 255] } else { [90, 40, 140, 255] });
        let p = profil(&px, 640 * 4, 640, 4, GRIS, |_, _| false).unwrap();
        assert_eq!(p[0], [145, 90, 80]);
        assert_eq!(p[SEGMENTS / 2 - 1], [145, 90, 80]);
        assert_eq!(p[SEGMENTS / 2], [90, 40, 140]);
        assert_eq!(p[SEGMENTS - 1], [90, 40, 140]);
    }

    /// Une bande verticale (une colonne) se lit de haut en bas.
    #[test]
    fn une_colonne_se_lit_de_haut_en_bas() {
        let px = bande(4, 320, |_, y| if y < 160 { [10, 20, 30, 255] } else { [200, 100, 50, 255] });
        let p = profil(&px, 4 * 4, 4, 320, GRIS, |_, _| false).unwrap();
        assert_eq!((p[0], p[SEGMENTS - 1]), ([10, 20, 30], [200, 100, 50]));
    }

    /// ⭐ Une icône qui déborde dans la bande ne change pas la couleur : la
    /// médiane l'ignore tant qu'elle couvre moins de la moitié du segment.
    #[test]
    fn une_icone_dans_la_bande_ne_pese_pas() {
        let px = bande(320, 4, |x, _| if x % 10 < 4 { [255, 255, 255, 255] } else { [145, 90, 80, 255] });
        let p = profil(&px, 320 * 4, 320, 4, GRIS, |_, _| false).unwrap();
        assert!(p.iter().all(|&c| c == [145, 90, 80]), "{p:?}");
    }

    /// Les pixels transparents ne comptent pas ; un segment qui n'en a pas
    /// d'autres prend son voisin ; une bande entièrement vide ne donne rien.
    #[test]
    fn les_pixels_transparents_ne_comptent_pas() {
        let px = bande(320, 4, |x, _| if x < 40 { [0, 0, 0, 0] } else { [145, 90, 80, 255] });
        let p = profil(&px, 320 * 4, 320, 4, GRIS, |_, _| false).unwrap();
        assert_eq!(p[0], [145, 90, 80], "le segment vide prend le plus proche");
        assert_eq!(profil(&bande(320, 4, |_, _| [9, 9, 9, 0]), 320 * 4, 320, 4, GRIS, |_, _| false), None);
    }

    /// ⛔ Un pixel sous une ombre que le dock peint ne compte pas : sinon chaque
    /// relevé assombrirait la couleur que l'ombre reprend au suivant.
    #[test]
    fn un_pixel_sous_une_ombre_ne_compte_pas() {
        let px = bande(320, 4, |x, _| if x < 40 { [10, 10, 10, 255] } else { [145, 90, 80, 255] });
        let p = profil(&px, 320 * 4, 320, 4, GRIS, |x, _| x < 40).unwrap();
        assert!(p.iter().all(|&c| c == [145, 90, 80]), "le bout ombré prend son voisin : {p:?}");
        assert_eq!(profil(&px, 320 * 4, 320, 4, GRIS, |_, _| true), None, "tout ombré : rien");
    }

    /// ⛔ **UNE ICÔNE QUI COUVRE PRESQUE TOUT LE SEGMENT** (capture `image15.png`) :
    /// le fond turquoise d'une icône y était majoritaire, la médiane le prenait.
    /// Seuls les pixels proches de la teinte réglée comptent.
    #[test]
    fn une_icone_majoritaire_ne_teinte_pas_la_barre() {
        let (violet, turquoise) = ([110, 55, 100, 255], [2, 218, 150, 255]);
        let px = bande(4, 320, |_, y| if y % 10 < 2 { violet } else { turquoise });
        let p = profil(&px, 4 * 4, 4, 320, [120, 60, 110], |_, _| false).unwrap();
        assert!(p.iter().all(|&c| c == [110, 55, 100]), "le fond, pas l'icône : {p:?}");
        // Un segment tout entier sous l'icône prend son voisin.
        let px = bande(4, 320, |_, y| if y >= 300 { turquoise } else { violet });
        let p = profil(&px, 4 * 4, 4, 320, [120, 60, 110], |_, _| false).unwrap();
        assert_eq!(p[SEGMENTS - 1], [110, 55, 100]);
    }

    /// ⭐ Demande du 2026-10-23 : une pastille d'icône ne pèse pas, MÊME quand le
    /// filtre par la teinte la laisse passer — barre en dégradé (violet rendu,
    /// teinte réglée orange : loin des deux), pastille olive sur 60 % de la bande.
    /// Sans les cases (l'ancien code), le profil prenait l'olive.
    #[test]
    fn la_case_d_une_icone_ne_compte_pas_meme_si_la_teinte_la_laisse_passer() {
        let (violet, olive) = ([120, 35, 135, 255], [130, 140, 40, 255]);
        let pastille = |y: usize| (40..232).contains(&y);
        let px = bande(4, 320, |_, y| if pastille(y) { olive } else { violet });
        let orange = [230, 130, 20];
        let icones = [[0.0, 48.0, 4.0, 154.0]];
        let p = profil(&px, 4 * 4, 4, 320, orange, |x, y| dans_une_icone(&icones, x as f32 + 0.5, y as f32 + 0.5)).unwrap();
        assert!(p.iter().all(|&c| c == [120, 35, 135]), "le fond violet partout : {p:?}");
        let sans = profil(&px, 4 * 4, 4, 320, orange, |_, _| false).unwrap();
        assert!(sans.iter().any(|&c| c == [130, 140, 40]), "sans les cases, l'olive passait");
    }

    /// ⛔ **UNE TEINTE RÉGLÉE QUI NE DÉCRIT PAS LA BARRE** — DÉFAUT SIGNALÉ
    /// (2026-11-13) : « l'ombrage de couleur de la barre de gauche semble être
    /// calculé à partir de la couleur de la zone centrale de la barre du haut ».
    /// MESURÉ (capture 4K) : barre du haut réglée orange (189, 54, 5), RENDUE
    /// rouge sombre (~75, 35, 40) ; seul un petit élément orange (192, 96, 47) vers
    /// x = 240 passait le filtre, et le voisinage le recopiait sur tout le bout
    /// gauche — que l'ombre colorée du haut pose sur la colonne de gauche. Ici :
    /// une barre rouge sombre, un élément orange sur le quart d'UN segment.
    #[test]
    fn une_teinte_reglee_loin_de_toute_la_barre_ne_choisit_pas_un_detail() {
        let (sombre, orange) = ([75, 35, 40, 255], [192, 96, 47, 255]);
        let segment = 3840 / SEGMENTS;
        let px = bande(3840, 4, |x, _| if (2 * segment..2 * segment + segment / 4).contains(&x) { orange } else { sombre });
        let p = profil(&px, 3840 * 4, 3840, 4, [189, 54, 5], |_, _| false).unwrap();
        assert!(p.iter().all(|&c| c == [75, 35, 40]), "la barre, pas l'élément orange : {:?}", &p[..5]);
    }

    /// ⛔ **UNE PASTILLE PLUS PROCHE DE LA TEINTE RÉGLÉE QUE LA BARRE ELLE-MÊME** —
    /// DÉFAUT SIGNALÉ (2026-11-13), ombres de couleur fausses en bas des colonnes.
    /// MESURÉ : colonne gauche réglée (94, 93, 186), rendue (89, 64, 122) ; le bas
    /// de la bande traverse des pastilles bleues (71, 143, 208), à 87 de la teinte
    /// quand la barre en est à 98 : elles passaient le filtre et coloraient le pied
    /// de la colonne. Le centre du filtre est la barre telle que RENDUE.
    #[test]
    fn une_pastille_proche_de_la_teinte_reglee_ne_colore_pas_le_pied_de_la_colonne() {
        let (barre, pastille) = ([89, 64, 122, 255], [71, 143, 208, 255]);
        let segment = 2110 / SEGMENTS;
        let px = bande(4, 2110, |_, y| if y >= 29 * segment || (y / 8) % 4 == 0 { pastille } else { barre });
        let p = profil(&px, 4 * 4, 4, 2110, [94, 93, 186], |_, _| false).unwrap();
        assert!(p.iter().all(|&c| c == [89, 64, 122]), "le pied : {:?}", &p[SEGMENTS - 3..]);
    }

    /// ⛔ **DES CASES D'ICÔNES QUI MANGENT TOUTE LA BANDE** — MESURÉ (2026-11-13,
    /// colonne de droite, 4×2110) : les cases élargies couvraient 5288 pixels sur
    /// 8440 ; il ne restait que deux segments, entre deux icônes, et leur couleur
    /// (une mise en avant) devenait celle de toute la colonne. Ici : barre bleu
    /// nuit, icônes partout sauf deux trous où luit un liseré clair.
    #[test]
    fn des_cases_qui_couvrent_presque_toute_la_bande_ne_la_relisent_pas() {
        let (nuit, lisere) = ([0, 11, 40, 255], [120, 200, 255, 255]);
        let segment = 2110 / SEGMENTS;
        let trou = |y: usize| (14 * segment..15 * segment).contains(&y) || (28 * segment..29 * segment).contains(&y);
        let px = bande(4, 2110, |_, y| if trou(y) { lisere } else { nuit });
        let p = profil_de_la_bande(&px, 4 * 4, 4, 2110, [3, 88, 154], |_, _| false, |_, y| !trou(y)).unwrap();
        assert!(p.iter().all(|&c| c == [0, 11, 40]), "la colonne, pas le liseré : {:?}", &p[..3]);
        // Des icônes qui laissent la plupart des segments : elles restent écartées.
        let px = bande(4, 2110, |_, y| if y % segment < segment / 2 { lisere } else { nuit });
        let p = profil_de_la_bande(&px, 4 * 4, 4, 2110, [0, 11, 40], |_, _| false, |_, y| y % segment < segment / 2).unwrap();
        assert!(p.iter().all(|&c| c == [0, 11, 40]), "{:?}", &p[..3]);
    }

    /// Le pas d'une ligne staging dépasse souvent sa largeur : les octets de
    /// bourrage ne sont pas des pixels.
    #[test]
    fn le_pas_de_ligne_est_respecte() {
        let (largeur, pas) = (64usize, 64 * 4 + 32);
        let mut px = vec![0u8; pas * 2];
        for y in 0..2 {
            for x in 0..largeur {
                px[y * pas + x * 4..y * pas + x * 4 + 4].copy_from_slice(&[30, 20, 10, 255]);
            }
        }
        let p = profil(&px, pas, largeur, 2, GRIS, |_, _| false).unwrap();
        assert!(p.iter().all(|&c| c == [10, 20, 30]));
    }

    /// ⛔ **LE GRAIN LE PLUS SOMBRE** (mesuré le 2026-10-19) : barre rendue
    /// (26, 87, 178) au milieu de sa bande, profil (22, 82, 166). La teinte réglée,
    /// adoucie, est plus sombre que la barre texturée : le tiers le plus proche
    /// d'elle était le grain le plus sombre. Le profil est le CENTRE du grain.
    #[test]
    fn le_profil_est_le_centre_du_grain_pas_son_cote_le_plus_proche() {
        let grain = [[18, 80, 178], [30, 92, 190], [42, 104, 202]];
        let px = bande(320, 4, |x, y| {
            let [r, g, b] = grain[(x + y) % 3];
            [r, g, b, 255]
        });
        let p = profil(&px, 320 * 4, 320, 4, [10, 75, 154], |_, _| false).unwrap();
        assert!(p.iter().all(|&c| c == [30, 92, 190]), "le centre du grain : {:?}", p[0]);
    }

    /// Les bandes au bord INTÉRIEUR de chaque barre visible, hors du liseré.
    #[test]
    fn les_bandes_sont_au_bord_interieur_des_barres() {
        let layout = DockLayout { total_width: 1920.0, total_height: 1080.0, top_thickness: 60.0, haut_colonnes: 60.0, left_width: 124.0, right_width: 120.0, bottom_thickness: 80.0 };
        let [haut, gauche, droite, bas] = bandes(&layout, [60.0, 124.0, 120.0, 80.0]);
        assert_eq!(haut, Some([0, 60 - 8, 1920, 4]));
        // ⛔ Les colonnes SOUS la barre du haut, le bas ENTRE les colonnes : une
        // bande qui débordait sur la barre voisine en prenait la couleur.
        assert_eq!(gauche, Some([124 - 8, 60, 4, 1080 - 60]));
        assert_eq!(droite, Some([1920 - 120 + 4, 60, 4, 1080 - 60]));
        assert_eq!(bas, Some([124, 1080 - 80 + 4, 1920 - 124 - 120, 4]));
        let [_, gauche, _, bas] = bandes(&layout, [60.0, 0.0, 120.0, 0.0]);
        assert_eq!((gauche, bas), (None, None), "barres masquées");
        let [_, _, _, bas] = bandes(&layout, [60.0, 0.0, 120.0, 80.0]);
        assert_eq!(bas, Some([0, 1080 - 80 + 4, 1920 - 120, 4]), "colonne masquée : le bas va jusqu'au bord");
    }
}


#[cfg(test)]
mod tests_garder_si_proche {
    use super::*;

    #[test]
    fn un_profil_presque_identique_garde_l_ancien_et_un_vrai_changement_passe() {
        let ancien: Profil = [[100, 100, 100]; SEGMENTS];
        let mut proche = ancien;
        proche[5] = [100 + ECART_VISIBLE, 100, 100 - ECART_VISIBLE];
        assert_eq!(garder_si_proche(Some(ancien), Some(proche)), Some(ancien));
        let mut loin = ancien;
        loin[31] = [100, 100 + ECART_VISIBLE + 1, 100];
        assert_eq!(garder_si_proche(Some(ancien), Some(loin)), Some(loin));
        assert_eq!(garder_si_proche(None, Some(proche)), Some(proche));
        assert_eq!(garder_si_proche(Some(ancien), None), None);
    }
}
