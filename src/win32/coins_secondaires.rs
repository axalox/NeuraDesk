//! **LES COINS ARRONDIS SOUS LA BARRE DU HAUT DES ÉCRANS SECONDAIRES** — DEMANDE
//! (2026-10-24) : « ajouter des coins arrondis sous la barre du haut des écrans
//! secondaires, reprenant la couleur de la barre supérieure de chaque écran ».
//! L'équivalent des congés de la zone principale (`ui::dock::conges`), mais
//! OPTIONNEL et réglé ÉCRAN PAR ÉCRAN, dans l'onglet de chaque écran
//! (`config::load_coins_secondaires`).
//!
//! **Des fenêtres à part, comme `win32::coins_ecran`** : la fenêtre d'un dock
//! secondaire EST la bande du haut (voir `window::resolve_secondary_geometry`),
//! rien ne s'y dessine en dessous. Deux petites fenêtres par écran, posées sous
//! les deux bouts de la bande, transparentes aux clics, de la classe des coins
//! d'écran (même `wndproc`, même propriétaire).
//!
//! ⭐ **LA COULEUR EST CELLE RENDUE** : la barre est floutée, teintée, éclairée —
//! sa teinte réglée ne lui ressemble pas (voir `gfx::releve_barres`). Chaque coin
//! prend le profil relevé au bout de la bande qu'il prolonge ; avant le premier
//! relevé, la teinte adoucie de la barre.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SetWindowDisplayAffinity, ShowWindow, SW_SHOWNOACTIVATE, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::win32::coins_ecran;

thread_local! {
    /// Les écrans (`\\.\DISPLAY2`…) où les coins sont demandés — lus au premier
    /// usage (voir `config::load_coins_secondaires`).
    static ECRANS: RefCell<Option<BTreeSet<String>>> = const { RefCell::new(None) };
    /// Le rayon réglé par écran, en pixels logiques (voir
    /// `config::load_coins_secondaires_rayons`) — absent : il suit le style.
    static RAYONS: RefCell<Option<BTreeMap<String, u32>>> = const { RefCell::new(None) };
}

fn rayons<R>(f: impl FnOnce(&mut BTreeMap<String, u32>) -> R) -> R {
    RAYONS.with(|r| f(r.borrow_mut().get_or_insert_with(crate::config::load_coins_secondaires_rayons)))
}

/// Le plus grand rayon réglable, en pixels logiques (le curseur de l'onglet de
/// l'écran va de 0, « comme le style », à cette valeur).
pub const RAYON_MAX: u32 = 96;

/// Le rayon réglé pour `ecran`, en pixels logiques — 0 s'il suit le style.
pub fn rayon_regle(ecran: &str) -> u32 {
    rayons(|r| r.get(ecran).copied().unwrap_or(0))
}

/// Règle le rayon de `ecran` (0 = suivre le style), et l'enregistre. DEMANDE du
/// 2026-11-01 : « pouvoir régler le rayon des coins arrondis sous les barres
/// secondaires ». Les coins se reposent à la frame suivante (`synchroniser` voit
/// leur côté changer).
pub fn regler_rayon(ecran: &str, texte: &str) {
    let Some(px) = texte.trim().parse::<f64>().ok().filter(|v| v.is_finite()) else {
        crate::diag!("[coins_secondaires] rayon illisible : {texte:?}");
        return;
    };
    let px = px.round().clamp(0.0, f64::from(RAYON_MAX)) as u32;
    rayons(|r| {
        if px == 0 {
            r.remove(ecran);
        } else {
            r.insert(ecran.to_string(), px);
        }
        crate::config::save_coins_secondaires_rayons(r);
    });
}

fn ecrans<R>(f: impl FnOnce(&mut BTreeSet<String>) -> R) -> R {
    ECRANS.with(|e| f(e.borrow_mut().get_or_insert_with(crate::config::load_coins_secondaires)))
}

/// Les coins sont-ils demandés sur l'écran `ecran` ?
pub fn actif_sur(ecran: &str) -> bool {
    ecrans(|e| e.contains(ecran))
}

/// Sur au moins un écran (le relevé de couleur ne sert qu'à eux).
pub fn un_ecran_actif() -> bool {
    ecrans(|e| !e.is_empty())
}

/// Allume ou éteint les coins de l'écran `ecran`, et l'enregistre.
pub fn activer(ecran: &str, actifs: bool) {
    ecrans(|e| {
        if actifs {
            e.insert(ecran.to_string());
        } else {
            e.remove(ecran);
        }
        crate::config::save_coins_secondaires(e);
    });
}

/// Le rayon d'un style à coins droits (`rayon_grand = 0`), en pixels LOGIQUES :
/// l'option demandée doit se voir quel que soit le style.
const RAYON_DE_REPLI: f32 = 16.0;

/// Le rayon des coins de l'écran `ecran` (de `dpi`), en pixels physiques : celui
/// réglé dans son onglet, sinon celui des congés de la zone principale
/// (`ui::dock::rayon_conge`, le double du rayon des fenêtres du style) — mis à
/// l'échelle de l'écran.
pub fn rayon_pour(ecran: &str, dpi: u32) -> i32 {
    rayon_physique(rayon_regle(ecran), crate::ui::style::rayon_grand(), dpi)
}

/// La règle, sans état : `regle` (0 = suivre le style), `rayon_style` = le rayon
/// des fenêtres du style.
fn rayon_physique(regle: u32, rayon_style: f32, dpi: u32) -> i32 {
    let logique = if regle > 0 {
        regle as f32
    } else if rayon_style * 2.0 >= 1.0 {
        rayon_style * 2.0
    } else {
        RAYON_DE_REPLI
    };
    (logique * dpi as f32 / crate::win32::dpi::STANDARD_DPI as f32).round().max(1.0) as i32
}

/// La bande du haut d'un écran secondaire, telle que l'appelant la voit.
#[derive(Debug, Clone, Copy)]
pub struct Bande {
    /// x, y (écran), largeur, épaisseur — en pixels physiques.
    pub rect: [i32; 4],
    pub rayon: i32,
    /// La couleur rendue à chaque bout, gauche puis droite (RGB).
    pub couleurs: [[u8; 3]; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Coin {
    x: i32,
    y: i32,
    cote: i32,
    droite: bool,
}

/// Les deux coins sous `bande`, collés à ses bouts et à son bord inférieur. Une
/// bande trop étroite pour deux coins n'en a pas.
fn coins_de(bande: &Bande) -> Option<[Coin; 2]> {
    let [x, y, largeur, epaisseur] = bande.rect;
    let cote = bande.rayon.min(largeur / 2);
    (cote >= 1 && epaisseur > 0).then(|| {
        let bas = y + epaisseur;
        [Coin { x, y: bas, cote, droite: false }, Coin { x: x + largeur - cote, y: bas, cote, droite: true }]
    })
}

thread_local! {
    /// Les coins posés, leurs couleurs, leurs fenêtres — et le réglage de capture
    /// sous lequel ils l'ont été (voir `coins_ecran`, même raison).
    static POSES: RefCell<Poses> = RefCell::new(Poses::default());
}

#[derive(Default)]
struct Poses {
    coins: Vec<Coin>,
    couleurs: Vec<[u8; 3]>,
    capturables: bool,
    fenetres: Vec<HWND>,
}

/// Met les coins en accord avec les bandes VISIBLES `bandes` des écrans où ils sont
/// demandés (voir `app::bandes_secondaires`) — à appeler à chaque
/// frame : sans changement, il ne fait rien ; une couleur qui change repeint
/// seulement.
pub fn synchroniser(bandes: &[Bande]) {
    let (coins, couleurs): (Vec<Coin>, Vec<[u8; 3]>) = bandes
        .iter()
        .filter_map(|b| coins_de(b).map(|[g, d]| [(g, b.couleurs[0]), (d, b.couleurs[1])]))
        .flatten()
        .unzip();
    let capturables = crate::gfx::capture_visibility::is_enabled();
    POSES.with(|p| {
        let Ok(mut poses) = p.try_borrow_mut() else { return };
        if poses.coins == coins && poses.capturables == capturables {
            if poses.couleurs != couleurs {
                for ((coin, &couleur), &hwnd) in coins.iter().zip(&couleurs).zip(&poses.fenetres) {
                    peindre(hwnd, coin, couleur);
                }
                poses.couleurs = couleurs;
            }
            return;
        }
        for hwnd in poses.fenetres.drain(..) {
            let _ = unsafe { DestroyWindow(hwnd) };
        }
        let fenetres = coins.iter().zip(&couleurs).filter_map(|(c, &couleur)| poser(c, couleur)).collect();
        *poses = Poses { coins, couleurs, capturables, fenetres };
    });
}

/// Repasse les coins au-dessus des autres fenêtres topmost (voir
/// `coins_ecran::remonter`).
pub fn remonter() {
    POSES.with(|p| {
        let Ok(poses) = p.try_borrow() else { return };
        for &hwnd in &poses.fenetres {
            coins_ecran::remonter_coin(hwnd);
        }
    });
}

fn poser(coin: &Coin, couleur: [u8; 3]) -> Option<HWND> {
    let instance = coins_ecran::classe()?;
    let hwnd = match unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("NdCoinEcran"),
            w!("NeuraDesk — coin sous la barre"),
            WS_POPUP,
            coin.x,
            coin.y,
            coin.cote,
            coin.cote,
            coins_ecran::proprietaire(),
            None,
            Some(instance),
            None,
        )
    } {
        Ok(hwnd) => hwnd,
        Err(err) => {
            crate::diag!("[coins_secondaires] CreateWindowExW échoué : {err}");
            return None;
        }
    };
    let _ = unsafe { SetWindowDisplayAffinity(hwnd, crate::gfx::capture_visibility::window_display_affinity()) };
    peindre(hwnd, coin, couleur);
    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    Some(hwnd)
}

/// Le coin est la couleur de la barre HORS du quart de disque centré sur son
/// coin bas-intérieur : le dessin d'un coin d'écran du HAUT (`bas = false`).
fn peindre(hwnd: HWND, coin: &Coin, couleur: [u8; 3]) {
    let pixels = coins_ecran::pixels_du_coin_en_couleur(coin.cote, coin.droite, false, couleur);
    coins_ecran::peindre_carre(hwnd, (coin.x, coin.y), coin.cote, &pixels);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sous la bande, collés à ses bouts — y compris sur un écran à gauche du
    /// principal (coordonnées négatives).
    #[test]
    fn deux_coins_sous_la_bande_colles_a_ses_bouts() {
        let bande = Bande { rect: [-1920, 0, 1920, 40], rayon: 24, couleurs: [[1, 2, 3], [4, 5, 6]] };
        let [g, d] = coins_de(&bande).expect("deux coins");
        assert_eq!(g, Coin { x: -1920, y: 40, cote: 24, droite: false });
        assert_eq!(d, Coin { x: -24, y: 40, cote: 24, droite: true });
    }

    #[test]
    fn une_bande_masquee_ou_vide_n_a_pas_de_coin() {
        let bande = |rect| Bande { rect, rayon: 24, couleurs: [[0; 3]; 2] };
        assert!(coins_de(&bande([0, 0, 1920, 0])).is_none(), "épaisseur nulle : bande masquée");
        assert!(coins_de(&bande([0, 0, 1, 40])).is_none());
    }

    /// La couleur de la barre au coin extérieur (en haut, côté bord de l'écran),
    /// transparente vers la zone libre.
    #[test]
    fn le_coin_prend_la_couleur_dehors_et_s_efface_dedans() {
        let px = coins_ecran::pixels_du_coin_en_couleur(10, false, false, [200, 100, 50]);
        assert_eq!(&px[0..4], &[50, 100, 200, 255], "haut-gauche : la barre, opaque (BGRA)");
        let dernier = px.len() - 4;
        assert_eq!(px[dernier + 3], 0, "bas-droite : la zone libre");
    }

    #[test]
    fn le_rayon_suit_le_dpi() {
        assert!(rayon_physique(0, 8.0, 192) >= 2 * rayon_physique(0, 8.0, 96) - 1);
        assert_eq!(rayon_physique(20, 8.0, 192), 40);
    }

    /// Un rayon réglé l'emporte sur le style ; 0 le suit ; un style à coins droits
    /// prend le repli.
    #[test]
    fn le_rayon_regle_l_emporte_sur_le_style() {
        assert_eq!(rayon_physique(30, 8.0, 96), 30);
        assert_eq!(rayon_physique(0, 8.0, 96), 16);
        assert_eq!(rayon_physique(0, 0.0, 96), RAYON_DE_REPLI as i32);
    }
}
