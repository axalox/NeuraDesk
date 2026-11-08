//! **LA LOUPE** — le grossissement qui suit le curseur pendant qu'on vise un
//! pixel avec la pipette du sélecteur de couleurs (voir `win32::cef_couleurs`).
//!
//! ⛔ **CE N'EST PAS LA PAGE QUI LA DESSINE, ET ÇA NE PEUT PAS L'ÊTRE.** Une
//! page CEF ne peint que dans SA PROPRE fenêtre ; or on vise des pixels
//! appartenant à d'autres applications. Une loupe rendue dans `couleurs.html`
//! ne grossirait que l'applet elle-même.
//!
//! C'est donc une **fenêtre de recouvrement** — `WS_EX_LAYERED | WS_EX_TOPMOST
//! | WS_EX_TOOLWINDOW`, peinte par `UpdateLayeredWindow` depuis un DIB 32 bits,
//! exactement comme `win32::epingle`, dont ce module reprend la peinture et le
//! rééchantillonnage.
//!
//! **AUCUN CODE DE RENDU N'EST NOUVEAU ICI** : la capture vient de
//! `capture_ecran` (`Surface` + `copier_ecran`), la conversion BGRA→RGBA de
//! `recadrer_en_rgba`, l'agrandissement NET de `epingle::reechantillonner`
//! (plus proche voisin), la prémultiplication de `epingle::premultiplier_bgra`.
//! Ce module n'ajoute que la GÉOMÉTRIE et le SUIVI.
//!
//! ⚠️ **LE PIÈGE, ET IL EST TESTÉ.** `copier_ecran` utilise `CAPTUREBLT`, qui
//! capture AUSSI les fenêtres en couches — la loupe en est une. Si son
//! rectangle recouvrait celui qu'elle échantillonne, **elle se filmerait**, et
//! le résultat partirait en bave. La géométrie garantit la non-intersection
//! (voir `DECALAGE`), et le test `la_loupe_ne_recouvre_jamais_son_propre_
//! echantillon` le prouve sur une grille couvrant tous les bords et coins.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, DIB_RGB_COLORS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetSystemMetrics, LoadCursorW, RegisterClassExW, ShowWindow,
    UpdateLayeredWindow, IDC_ARROW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::win32::capture_ecran::{self, Surface};
use crate::win32::epingle::{premultiplier_bgra, reechantillonner};

/// Côté de la zone échantillonnée, en pixels d'écran — `21` = ±10 autour du
/// curseur. Assez pour voir la structure d'un bord, d'un dégradé ou d'une
/// police, sans noyer le pixel visé.
const COTE_ECHANTILLON: i32 = 21;
/// Grossissement. `21 × 7 = 147` px de loupe.
const FACTEUR: i32 = 7;
/// **Écart entre le curseur et le bord PROCHE de la loupe.**
///
/// ⚠️ **IL DOIT DÉPASSER LA DEMI-LARGEUR DE L'ÉCHANTILLON** (`COTE_ECHANTILLON
/// / 2`), sinon la loupe recouvrirait son propre échantillon — et comme
/// `copier_ecran` capture aussi les fenêtres en couches, **elle se filmerait**.
/// La relation est vérifiée par un test, pas par cette phrase.
const DECALAGE: i32 = 26;

/// Le rectangle **échantillonné** autour du curseur.
pub(crate) fn rectangle_echantillon(curseur: POINT, cote: i32) -> RECT {
    let demi = cote / 2;
    RECT {
        left: curseur.x - demi,
        top: curseur.y - demi,
        right: curseur.x - demi + cote,
        bottom: curseur.y - demi + cote,
    }
}

/// Le rectangle **occupé par la loupe** : en bas à droite du curseur, et
/// **basculée de l'autre côté** quand elle sortirait de l'écran.
///
/// ⚠️ **AUCUN RABATTEMENT DANS LES BORNES APRÈS LA BASCULE.** Rabattre la
/// loupe pourrait la ramener SUR le curseur, donc sur son propre échantillon :
/// une loupe partiellement hors écran, sur un écran plus petit qu'elle, est un
/// défaut cosmétique ; une loupe qui se filme est un défaut de fonctionnement.
pub(crate) fn rectangle_loupe(curseur: POINT, cote_loupe: i32, ecran: RECT) -> RECT {
    let mut left = curseur.x + DECALAGE;
    let mut top = curseur.y + DECALAGE;
    if left + cote_loupe > ecran.right {
        left = curseur.x - DECALAGE - cote_loupe;
    }
    if top + cote_loupe > ecran.bottom {
        top = curseur.y - DECALAGE - cote_loupe;
    }
    RECT { left, top, right: left + cote_loupe, bottom: top + cote_loupe }
}

/// Les deux rectangles se recouvrent-ils ? Bornes EXCLUSIVES à droite et en
/// bas, comme `RECT`.
pub(crate) fn se_recouvrent(a: RECT, b: RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// Le bureau VIRTUEL entier — tous les écrans, pas seulement le principal : on
/// peut viser sur n'importe lequel.
fn bureau() -> RECT {
    let (x, y, l, h) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    RECT { left: x, top: y, right: x + l, bottom: y + h }
}

thread_local! {
    static ARMEE: Cell<bool> = const { Cell::new(false) };
    /// Le HWND de la loupe, en `isize` : `0` = pas encore créée. En `isize` et
    /// non en `HWND`, comme `cef_couleurs::NOTRE_HWND` — les dérivations de
    /// `HWND` ont bougé d'une version de `windows-rs` à l'autre, un entier est
    /// sans ambiguïté.
    static FENETRE: Cell<isize> = const { Cell::new(0) };
    /// Le dernier point suivi. **C'est lui qui rend l'appel par frame
    /// gratuit** : tant que le curseur n'a pas bougé, rien n'est recapturé.
    static DERNIER: Cell<Option<POINT>> = const { Cell::new(None) };
    /// Le DIB de l'échantillon, **RÉUTILISÉ** d'une frame à l'autre — le
    /// réallouer serait une allocation par frame, à la fréquence de l'écran.
    static SURFACE: RefCell<Option<Surface>> = const { RefCell::new(None) };
}

pub fn est_armee() -> bool {
    ARMEE.with(Cell::get)
}

/// Arme la loupe : elle apparaîtra au prochain `suivre`.
pub fn armer() {
    ARMEE.with(|a| a.set(true));
    DERNIER.with(|d| d.set(None));
}

pub fn desarmer() {
    ARMEE.with(|a| a.set(false));
    DERNIER.with(|d| d.set(None));
    cacher();
}

/// Cache la loupe sans la détruire — c'est ce qui rend l'armement suivant
/// instantané : la fenêtre et son DIB existent déjà.
pub fn cacher() {
    let hwnd = FENETRE.with(Cell::get);
    if hwnd != 0 {
        unsafe {
            let _ = ShowWindow(HWND(hwnd as *mut c_void), SW_HIDE);
        }
    }
}

/// **Suivre le curseur : capturer, grossir, peindre.** Appelé à chaque frame
/// par `cef_couleurs::poll`.
pub fn suivre(curseur: POINT) {
    if !est_armee() {
        return;
    }
    // Rien à faire tant que le curseur n'a pas bougé : c'est ce qui rend
    // l'appel par frame gratuit, et ce qui évite une capture d'écran au rythme
    // de l'écran sur un curseur immobile.
    if DERNIER.with(|d| d.get()) == Some(curseur) {
        return;
    }
    DERNIER.with(|d| d.set(Some(curseur)));

    let cote_loupe = COTE_ECHANTILLON * FACTEUR;
    let Some(rgba) = capturer(rectangle_echantillon(curseur, COTE_ECHANTILLON)) else { return };
    let grossi = reechantillonner(
        &rgba,
        COTE_ECHANTILLON as u32,
        COTE_ECHANTILLON as u32,
        cote_loupe as u32,
        cote_loupe as u32,
    );
    let bgra = premultiplier_bgra(&grossi);

    let Some(hwnd) = fenetre_ou_creer() else { return };
    let zone = rectangle_loupe(curseur, cote_loupe, bureau());
    // ⛔ **GARDE-FOU D'EXÉCUTION, EN PLUS DU TEST.** Le test prouve l'invariant
    // sur une grille de positions ; celui-ci le vérifie sur la position
    // RÉELLE, au cas où quelqu'un aurait modifié `DECALAGE` sans relire le
    // test. Ne rien peindre est laid ; peindre une bave est pire, parce que
    // c'est SILENCIEUX — on croirait la couleur prélevée juste.
    if se_recouvrent(rectangle_echantillon(curseur, COTE_ECHANTILLON), zone) {
        crate::diag!("[loupe] la loupe recouvrirait son propre echantillon — NON PEINTE (voir DECALAGE)");
        return;
    }
    peindre(hwnd, POINT { x: zone.left, y: zone.top }, &bgra, cote_loupe);
    unsafe {
        // `SW_SHOWNOACTIVATE` : la loupe ne doit JAMAIS prendre le focus — une
        // pipette qui volerait le premier plan interromprait le geste qu'elle
        // sert.
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
}

/// Capture `cadre` depuis l'écran, en RGBA opaque.
fn capturer(cadre: RECT) -> Option<Vec<u8>> {
    SURFACE.with(|s| {
        let mut garde = s.try_borrow_mut().ok()?;
        if garde.is_none() {
            *garde = Some(Surface::nouvelle(COTE_ECHANTILLON, COTE_ECHANTILLON)?);
        }
        let surface = garde.as_mut()?;
        if !capture_ecran::copier_ecran(surface, cadre.left, cadre.top, COTE_ECHANTILLON, COTE_ECHANTILLON) {
            return None;
        }
        let octets = surface.octets((COTE_ECHANTILLON * COTE_ECHANTILLON * 4) as usize);
        // La `Surface` fait EXACTEMENT la taille de l'échantillon : la zone à
        // découper est donc tout son contenu.
        let tout = RECT { left: 0, top: 0, right: COTE_ECHANTILLON, bottom: COTE_ECHANTILLON };
        Some(capture_ecran::recadrer_en_rgba(octets, COTE_ECHANTILLON, tout))
    })
}

fn fenetre_ou_creer() -> Option<HWND> {
    let existante = FENETRE.with(Cell::get);
    if existante != 0 {
        return Some(HWND(existante as *mut c_void));
    }
    let hwnd = creer_fenetre()?;
    FENETRE.with(|f| f.set(hwnd.0 as isize));
    Some(hwnd)
}

fn creer_fenetre() -> Option<HWND> {
    static CLASSE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).ok()?.into();
        CLASSE.get_or_init(|| {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: w!("NeuraDeskLoupe"),
                ..Default::default()
            };
            RegisterClassExW(&wc);
        });
        match CreateWindowExW(
            // `WS_EX_TOOLWINDOW` : ni barre des tâches ni Alt-Tab — la loupe
            // n'est pas une fenêtre de l'application, c'est un viseur.
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("NeuraDeskLoupe"),
            w!("NeuraDesk — Loupe"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance),
            None,
        ) {
            Ok(hwnd) => Some(hwnd),
            Err(err) => {
                crate::diag!("[loupe] CreateWindowExW a échoué : {err}");
                None
            }
        }
    }
}

/// Peint la loupe — même mécanique que `win32::epingle::peindre`, dont ceci
/// reprend la forme : un DIB 32 bits prémultiplié, présenté d'un bloc par
/// `UpdateLayeredWindow`.
fn peindre(hwnd: HWND, position: POINT, pixels: &[u8], cote: i32) {
    unsafe {
        let ecran = GetDC(None);
        let memoire = CreateCompatibleDC(Some(ecran));
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: cote,
            // Négatif : les lignes vont de haut en bas, comme `pixels`.
            biHeight: -cote,
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        match CreateDIBSection(Some(memoire), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bitmap) if !bits.is_null() => {
                std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
                let ancien = SelectObject(memoire, bitmap.into());
                let fusion = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    // **OPAQUE, ET C'EST LE POINT DE LA LOUPE** : elle doit
                    // montrer la couleur RÉELLE, pas une version fondue avec ce
                    // qu'il y a derrière elle — c'est déjà la couleur exacte
                    // qu'on veut prélever.
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                let taille = SIZE { cx: cote, cy: cote };
                let origine = POINT::default();
                if let Err(err) = UpdateLayeredWindow(
                    hwnd,
                    Some(ecran),
                    Some(&position),
                    Some(&taille),
                    Some(memoire),
                    Some(&origine),
                    COLORREF(0),
                    Some(&fusion),
                    ULW_ALPHA,
                ) {
                    crate::diag!("[loupe] UpdateLayeredWindow a échoué : {err}");
                }
                SelectObject(memoire, ancien);
                let _ = DeleteObject(bitmap.into());
            }
            _ => crate::diag!("[loupe] CreateDIBSection {cote}×{cote} a échoué"),
        }
        let _ = DeleteDC(memoire);
        ReleaseDC(None, ecran);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **L'INVARIANT QUI EMPÊCHE LA LOUPE DE SE FILMER.**
    ///
    /// `copier_ecran` utilise `CAPTUREBLT`, donc il capture aussi les fenêtres
    /// en couches — la loupe en est une. Si son rectangle recouvrait celui
    /// qu'elle échantillonne, elle se capturerait elle-même et le résultat
    /// partirait en bave.
    ///
    /// Vérifié sur une grille qui couvre les quatre bords, les quatre coins et
    /// l'intérieur : ce test vaut mieux que n'importe quel examen à l'œil, qui
    /// ne verrait la bave que sur UNE position.
    #[test]
    fn la_loupe_ne_recouvre_jamais_son_propre_echantillon() {
        let ecran = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        let cote_loupe = COTE_ECHANTILLON * FACTEUR;
        for x in [0, 1, 5, 10, 11, 26, 100, 960, 1800, 1910, 1919, 1920] {
            for y in [0, 1, 5, 10, 11, 26, 100, 540, 1000, 1070, 1079, 1080] {
                let curseur = POINT { x, y };
                let echantillon = rectangle_echantillon(curseur, COTE_ECHANTILLON);
                let loupe = rectangle_loupe(curseur, cote_loupe, ecran);
                assert!(
                    !se_recouvrent(echantillon, loupe),
                    "le curseur {curseur:?} fait recouvrir la loupe {loupe:?} sur son échantillon {echantillon:?}"
                );
            }
        }
    }

    /// La relation dont dépend l'invariant ci-dessus, dite explicitement : si
    /// quelqu'un rapproche la loupe du curseur pour l'esthétique, ce test tombe
    /// AVANT que la bave n'apparaisse.
    #[test]
    fn le_decalage_depasse_la_demi_largeur_de_l_echantillon() {
        assert!(
            DECALAGE > COTE_ECHANTILLON / 2,
            "DECALAGE ({DECALAGE}) doit depasser la demi-largeur de l'echantillon ({})",
            COTE_ECHANTILLON / 2
        );
    }

    /// La loupe reste dans l'écran sur un écran ORDINAIRE — la bascule de bord
    /// suffit, et c'est ce qui justifie de ne pas rabattre ensuite (voir
    /// `rectangle_loupe`).
    #[test]
    fn la_loupe_bascule_et_reste_dans_l_ecran() {
        let ecran = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        let cote_loupe = COTE_ECHANTILLON * FACTEUR;
        for curseur in [
            POINT { x: 1919, y: 1079 },
            POINT { x: 0, y: 0 },
            POINT { x: 1919, y: 0 },
            POINT { x: 0, y: 1079 },
        ] {
            let loupe = rectangle_loupe(curseur, cote_loupe, ecran);
            assert!(loupe.left >= ecran.left && loupe.top >= ecran.top, "{curseur:?} -> {loupe:?}");
            assert!(loupe.right <= ecran.right && loupe.bottom <= ecran.bottom, "{curseur:?} -> {loupe:?}");
        }
    }

    /// **L'AGRANDISSEMENT EST DU PLUS PROCHE VOISIN** — c'est ce qui garde les
    /// pixels NETS, et ce qui fait qu'on peut viser un pixel précis.
    ///
    /// Le test ne se contente pas de vérifier les blocs : il vérifie qu'aucune
    /// couleur ABSENTE de la source n'apparaît. Un rééchantillonnage lissé
    /// (bilinéaire, `StretchBlt` en `HALFTONE`) inventerait des intermédiaires.
    #[test]
    fn l_agrandissement_est_du_plus_proche_voisin() {
        let src: Vec<u8> = vec![
            255, 0, 0, 255, 0, 255, 0, 255, // ligne 0 : rouge, vert
            0, 0, 255, 255, 255, 255, 0, 255, // ligne 1 : bleu, jaune
        ];
        let dst = reechantillonner(&src, 2, 2, 6, 6);
        assert_eq!(dst.len(), 6 * 6 * 4);
        let pixel = |x: usize, y: usize| &dst[(y * 6 + x) * 4..(y * 6 + x) * 4 + 4];
        // Chaque pixel source occupe un bloc 3×3 PLEIN.
        for y in 0..3 {
            for x in 0..3 {
                assert_eq!(pixel(x, y), [255, 0, 0, 255], "bloc rouge en ({x},{y})");
                assert_eq!(pixel(x + 3, y), [0, 255, 0, 255], "bloc vert en ({x},{y})");
                assert_eq!(pixel(x, y + 3), [0, 0, 255, 255], "bloc bleu en ({x},{y})");
                assert_eq!(pixel(x + 3, y + 3), [255, 255, 0, 255], "bloc jaune en ({x},{y})");
            }
        }
        // Aucune couleur inventée.
        for p in dst.chunks_exact(4) {
            assert!(
                matches!(p, [255, 0, 0, 255] | [0, 255, 0, 255] | [0, 0, 255, 255] | [255, 255, 0, 255]),
                "couleur absente de la source : {p:?} — l'agrandissement lisse"
            );
        }
    }

    /// Le décalage par défaut place bien la loupe EN BAS À DROITE quand il y a
    /// la place — c'est le côté que l'œil attend, et la bascule ne doit être
    /// qu'une exception de bord.
    #[test]
    fn au_centre_la_loupe_est_en_bas_a_droite_du_curseur() {
        let ecran = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        let curseur = POINT { x: 500, y: 400 };
        let loupe = rectangle_loupe(curseur, COTE_ECHANTILLON * FACTEUR, ecran);
        assert_eq!((loupe.left, loupe.top), (curseur.x + DECALAGE, curseur.y + DECALAGE));
    }
}
