//! **LA CAPTURE D'ÉCRAN** — la moitié « capture » de Flameshot, demandée le
//! 2026-09-25 (« juste la partie capture, le reste devrait déjà être repris par
//! le dash éditeur d'images »).
//!
//! Un clic sur l'applet FIGE l'écran : tout le bureau virtuel est copié d'un
//! coup (`BitBlt`), puis une fenêtre plein écran, au premier plan, montre cette
//! image assombrie. On glisse pour choisir une zone (rendue à sa luminosité
//! d'origine, cadre et dimensions en pixels), on l'AJUSTE — huit poignées, ou
//! un glisser à l'intérieur pour la déplacer — puis on **VALIDE par un bouton**.
//! La zone est alors encodée en PNG et rangée pour `app.rs`, qui l'ouvre dans
//! l'éditeur (`cef_image_editor::ouvrir_capture`) — annoter, copier, épingler,
//! enregistrer, c'est son travail.
//!
//! ⛔ **LE RELÂCHER NE CAPTURE PLUS, ET C'EST LA CONTREPARTIE OBLIGÉE DE
//! L'AJUSTEMENT** : il n'existerait sinon aucun instant où la zone est posée ET
//! encore modifiable. Valider, ce sont les deux boutons dessinés.
//!
//! Un clic SANS glisser prend la FENÊTRE survolée (elle s'éclaire sous le
//! curseur), ou l'écran entier sous le curseur s'il n'y en a pas, ou si Maj est
//! enfoncée — **une seule fois**, tant qu'aucune zone n'est posée : passé ce
//! point, un frémissement de souris remplacerait une zone soigneusement ajustée.
//! Échap, le clic droit ou la perte du premier plan annulent.
//!
//! ⭐ **DEMANDE (2026-10-24), d'après Snip (CleanShot X)** : la capture d'une
//! fenêtre, « la même zone que la dernière fois » (touche Entrée tant qu'aucune
//! zone n'est posée, puis Entrée valide), et le bouton « Texte » qui envoie la
//! zone à la reconnaissance de caractères au lieu de l'éditeur
//! (`Resultat::Texte`, voir `win32::texte_capture`).
//!
//! ⚠️ **LES FENÊTRES SONT RELEVÉES AU GEL, PAS SOUS LE CURSEUR** : notre fenêtre
//! plein écran couvre tout, donc `WindowFromPoint` ne trouverait qu'elle. Et
//! l'image est celle du gel : c'est l'ordre des fenêtres À CET INSTANT qui dit
//! laquelle est visible sous un point (`fenetres_visibles`).
//!
//! ⭐ **RIEN DE CE QU'ON DESSINE N'ENTRE DANS L'IMAGE** : le découpage lit
//! `Etat::clair`, la surface **vierge** de l'écran figé. Cadre, étiquette,
//! poignées, barre et boutons ne vivent que dans `Etat::tampon`, qui n'est que
//! de l'affichage. C'est ce qui permet de dessiner des boutons sans les
//! retrouver dans le PNG.
//!
//! Le clic GAUCHE sur l'applet masque d'abord l'interface (voir
//! `app::PhaseCapture`) ; le clic DROIT capture l'écran tel quel, barres
//! NeuraDesk comprises.
//!
//! **PAS DE CEF ICI**, comme `win32::epingle` : GDI pur, trois DIB en mémoire
//! (l'image claire, l'image sombre, un tampon de composition) — le processus
//! est `PER_MONITOR_AWARE_V2`, donc les coordonnées d'écran sont des pixels
//! PHYSIQUES et l'image figée se pose 1:1 sous la fenêtre.

use std::cell::RefCell;
use std::ffi::c_void;

use windows::core::w;
use windows::core::BOOL;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmFlush, DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DeleteDC, DeleteObject,
    DrawTextW, EndPaint, FillRect, GetDC, GetMonitorInfoW, InvalidateRect, MonitorFromPoint, ReleaseDC, SelectObject,
    SetBkMode, SetTextColor, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, CAPTUREBLT, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_SEMIBOLD,
    HBITMAP, HDC, HFONT, HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SRCCOPY,
    TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, EnumWindows, GetClassNameW, GetCursorPos, GetSystemMetrics,
    GetWindowLongW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, LoadCursorW, RegisterClassExW, SetCursor, GWL_EXSTYLE,
    WS_EX_TRANSPARENT,
    ShowWindow, HCURSOR, IDC_CROSS, IDC_HAND, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE,
    HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOW, WA_INACTIVE, WM_ACTIVATE,
    WM_CAPTURECHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::win32::poignees;

/// Part de luminosité gardée hors de la sélection (Flameshot voile l'écran de
/// la même façon).
const ASSOMBRISSEMENT: u32 = 100;
/// Sous ce déplacement (en pixels, sur chaque axe), un glisser est un CLIC :
/// on prend l'écran entier sous le curseur.
///
/// ⚠️ **ET SEULEMENT TANT QU'AUCUNE ZONE N'EST POSÉE** — voir
/// `Geste::Tracer::remplace`. Passé le premier geste, un frémissement de souris
/// remplacerait une zone soigneusement ajustée par l'écran entier.
const SEUIL_CLIC: i32 = 3;
/// Cadre de la sélection, en pixels logiques (× DPI / 96).
const EPAISSEUR_CADRE: i32 = 2;
/// Couleur du cadre et du fond de l'étiquette (BGR, comme `COLORREF`).
const COULEUR_CADRE: u32 = 0x00FF_A040;
const COULEUR_ETIQUETTE: u32 = 0x0020_2020;
/// Couleur des poignées, et du bouton sous le curseur.
const COULEUR_POIGNEE: u32 = 0x00FF_FFFF;
const COULEUR_BOUTON: u32 = 0x0040_4040;
/// Hauteur de la police de l'étiquette « L × H », en pixels logiques.
const POLICE_ETIQUETTE: i32 = 15;
/// `MK_SHIFT` dans le `wparam` d'un message de souris : Maj+clic prend l'écran.
const MK_SHIFT: usize = 0x0004;
/// L'aide affichée tant qu'aucune zone n'est posée.
const AIDE: &str = "Glisser : une zone  ·  Clic : la fenêtre  ·  Maj+clic : l'écran  ·  Entrée : la dernière zone  ·  Échap : annuler";
/// Sa hauteur et sa marge au haut de l'écran, en pixels logiques.
const AIDE_HAUTEUR: i32 = 30;
const AIDE_MARGE: i32 = 40;

/// **CE QU'UNE CAPTURE VALIDÉE PRODUIT** — l'image pour la carte ou l'éditeur,
/// ou la même image pour la reconnaissance de caractères.
pub enum Resultat {
    /// `zone` : où elle était, en pixels PHYSIQUES d'écran — la carte d'accès
    /// rapide se pose sur cet écran-là.
    Image { png: Vec<u8>, zone: RECT },
    Texte { png: Vec<u8> },
    /// **UNE ZONE À FILMER** (applet « Enregistrer », voir `cef_enregistrer`) —
    /// le rectangle seul, en pixels PHYSIQUES d'écran : rien n'est découpé.
    Zone { zone: RECT },
}

/// Ce que `valider` doit produire.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sortie {
    Image,
    Texte,
}

/// Un DIB 32 bits sélectionné dans son propre DC mémoire — libéré au `Drop`.
///
/// `pub(crate)` : réutilisé tel quel par `win32::loupe`, qui échantillonne une
/// petite zone sous le curseur à chaque frame — même besoin, même API, pas de
/// raison de la dupliquer (même motif que `config_path`, `pub(crate)` pour
/// `win32::apps_cache`).
pub(crate) struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    ancien: HGDIOBJ,
    bits: *mut u8,
}

impl Surface {
    pub(crate) fn nouvelle(largeur: i32, hauteur: i32) -> Option<Surface> {
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: largeur,
            // Négatif : les lignes vont de haut en bas.
            biHeight: -hauteur,
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        unsafe {
            let ecran = GetDC(None);
            let dc = CreateCompatibleDC(Some(ecran));
            ReleaseDC(None, ecran);
            match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(bitmap) if !bits.is_null() => {
                    let ancien = SelectObject(dc, bitmap.into());
                    Some(Surface { dc, bitmap, ancien, bits: bits as *mut u8 })
                }
                _ => {
                    crate::diag!("[capture] CreateDIBSection {largeur}×{hauteur} a échoué");
                    let _ = DeleteDC(dc);
                    None
                }
            }
        }
    }

    pub(crate) fn octets(&mut self, taille: usize) -> &mut [u8] {
        // SAFETY : `bits` pointe sur le DIB de `largeur × hauteur × 4` octets
        // alloué par `nouvelle`, vivant tant que `self` l'est.
        unsafe { std::slice::from_raw_parts_mut(self.bits, taille) }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.ancien);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

/// **Ce que la souris est en train de faire.** Un seul geste à la fois — et
/// c'est lui qui distingue « un relâcher qui conclut quelque chose » d'« un
/// relâcher orphelin », celui du clic sur l'applet qui tombe sur cette fenêtre
/// (voir `WM_LBUTTONUP`).
enum Geste {
    /// Premier glisser : la zone se dessine de `ancre` jusqu'au curseur.
    Tracer {
        ancre: POINT,
        /// Une zone était DÉJÀ posée quand ce tracé a commencé — voir
        /// `SEUIL_CLIC`. Un tracé trop court qui REMPLACE une zone existante est
        /// un frémissement, pas une intention, et il ne doit rien remplacer.
        remplace: bool,
    },
    /// Une poignée est tirée : `poignee` est un code `HT*` de bord.
    Redimensionner { poignee: u32, origine: RECT },
    /// Glisser à l'intérieur de la zone : on la translate.
    Deplacer { origine: RECT, depart: POINT },
    /// **Un bouton de la barre a été ENFONCÉ ICI.**
    ///
    /// ⛔ **`action` N'EST PAS UNE COMMODITÉ** : sans elle, on déciderait au
    /// RELÂCHER, en regardant seulement *où* le curseur se trouve. Déplacer la
    /// zone en relâchant par-dessus la barre vaudrait alors « Valider » — et on
    /// validerait **l'ancienne** zone, celle d'avant le déplacement. C'est
    /// exactement le besoin que `Etat::appui_droit` couvre déjà pour le bouton
    /// droit : un relâcher ne compte que si l'appui a eu lieu ICI.
    Bouton { action: Action },
}

/// Les trois boutons de la barre.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Valider,
    Texte,
    Annuler,
}

/// **Le cœur PUR de `zone_affichee`** — tout ce qui décide du rectangle, à
/// partir du seul geste, du curseur, de la zone posée et des dimensions.
///
/// ⚠️ **PUR VOLONTAIREMENT** : `Etat` porte des `Surface`, donc il n'est pas
/// constructible dans un test. Extraire la décision est ce qui la rend
/// éprouvable — et c'est la décision qui compte.
fn zone_du_geste(
    geste: Option<&Geste>,
    curseur: POINT,
    posee: Option<RECT>,
    ecran: (i32, i32),
    minimum: i32,
) -> Option<RECT> {
    match geste {
        Some(Geste::Tracer { ancre, .. }) => borner(rectangle_entre(*ancre, curseur), ecran.0, ecran.1),
        Some(Geste::Redimensionner { poignee, origine }) => {
            borner(poignees::redimensionner(*origine, *poignee, curseur, ecran, minimum), ecran.0, ecran.1)
        }
        Some(Geste::Deplacer { origine, depart }) => {
            let d = (curseur.x - depart.x, curseur.y - depart.y);
            borner(poignees::deplacer(*origine, d, ecran), ecran.0, ecran.1)
        }
        _ => posee,
    }
}

/// **Le rectangle de la zone, tel qu'il doit être DESSINÉ *ET* VALIDÉ.**
///
/// ⛔ **UNE SEULE FONCTION POUR LES DEUX, ET C'EST TOUT SON OBJET.** `peindre`
/// l'appelle pour dessiner, `valider` pour découper : la divergence entre ce que
/// l'utilisateur voit et ce qu'il obtient devient impossible. Deux calculs
/// séparés — même écrits identiques — finiraient par dériver d'un pixel, et
/// c'est le genre d'écart qui ne se voit qu'à l'usage.
fn zone_affichee(e: &Etat) -> Option<RECT> {
    zone_du_geste(e.geste.as_ref(), e.curseur, e.zone, (e.largeur, e.hauteur), e.minimum())
}

/// **Ce qu'un relâcher POSE** — le cœur pur de `WM_LBUTTONUP`. `None` signifie
/// « ne rien changer à la zone déjà posée », pas « rectangle dégénéré » : les
/// deux se confondent volontairement, parce qu'un rectangle dégénéré ne doit de
/// toute façon pas remplacer une zone valide.
///
/// `ecran_sous` est le moniteur sous le point d'appui, calculé par l'appelant :
/// `MonitorFromPoint` est le seul morceau Win32 de cette décision.
/// `fenetre_sous` : la fenêtre sous ce point (`None` avec Maj) — un clic la
/// prend en priorité sur l'écran.
///
/// ⚠️ **LE RELÂCHER PASSE PAR `zone_du_geste`, LA MÊME QUE LE DESSIN** pour les
/// trois gestes d'ajustement. C'est ce qui garantit que ce qui est posé est
/// exactement ce qui était dessiné au dernier instant — recalculer ici, même
/// avec les mêmes formules, rouvrirait la divergence.
fn zone_posee_par_le_relacher(
    geste: &Geste,
    curseur: POINT,
    posee: Option<RECT>,
    ecran_sous: Option<RECT>,
    fenetre_sous: Option<RECT>,
    ecran: (i32, i32),
    minimum: i32,
) -> Option<RECT> {
    match geste {
        // Un bouton ne pose rien : c'est `WM_LBUTTONUP` qui le traduit en action.
        Geste::Bouton { .. } => None,
        Geste::Tracer { ancre, remplace } if est_un_clic(*ancre, curseur) => {
            // ⚠️ **LE RACCOURCI « ÉCRAN ENTIER » NE VAUT QUE POUR LE PREMIER
            // GESTE** : une fois une zone posée et ajustée, un frémissement de
            // deux pixels ne doit pas la remplacer par l'écran entier.
            if *remplace {
                return None;
            }
            fenetre_sous.or(ecran_sous).and_then(|z| borner(z, ecran.0, ecran.1))
        }
        _ => zone_du_geste(Some(geste), curseur, posee, ecran, minimum),
    }
}

/// **LA FENÊTRE SOUS `point`** — la première de `fenetres` (rangées du premier
/// plan vers l'arrière, voir `fenetres_visibles`) qui le contient.
fn fenetre_sous(fenetres: &[RECT], point: POINT) -> Option<RECT> {
    fenetres.iter().copied().find(|r| poignees::dans(point, *r))
}

/// **Les poignées et la barre ne s'affichent que sur une zone AJUSTABLE** —
/// c'est-à-dire relâchée, ou en cours de redimensionnement/déplacement, auquel
/// cas elles suivent l'aperçu. Pendant le tracé initial il n'y a rien à ajuster,
/// et l'étiquette de dimensions suffit.
fn zone_ajustable(e: &Etat) -> bool {
    e.zone.is_some() && !matches!(e.geste, Some(Geste::Tracer { .. }))
}

/// La capture en cours : l'écran figé et la sélection.
struct Etat {
    /// La fenêtre de sélection elle-même — voir `est_en_cours`, qui s'en sert
    /// pour reconnaître un état ORPHELIN (fenêtre détruite sans que
    /// `WM_NCDESTROY` ait remis `ETAT` à `None`).
    hwnd: HWND,
    /// Coin haut-gauche du bureau virtuel, en coordonnées d'écran.
    origine: POINT,
    largeur: i32,
    hauteur: i32,
    /// Résolution de la fenêtre, résolue UNE fois : les seuils logiques
    /// (poignées, barre, taille minimale) en dérivent à chaque usage.
    dpi: i32,
    clair: Surface,
    sombre: Surface,
    tampon: Surface,
    police: HFONT,
    /// Le curseur courant, en coordonnées CLIENT — le tracé EN COURS s'en sert,
    /// et le survol des poignées aussi.
    curseur: POINT,
    /// **La zone POSÉE** : c'est elle que validera le bouton. `None` tant
    /// qu'aucun geste n'a été conclu.
    zone: Option<RECT>,
    /// Le geste en cours, s'il y en a un.
    geste: Option<Geste>,
    /// Vrai dès la première activation — voir `WM_ACTIVATE`.
    activee: bool,
    /// Un clic droit a été ENFONCÉ dans cette fenêtre — voir `WM_RBUTTONUP`.
    appui_droit: bool,
    /// Les fenêtres visibles au gel, en coordonnées CLIENT, du premier plan vers
    /// l'arrière — voir `fenetres_visibles`.
    fenetres: Vec<RECT>,
    /// La fenêtre survolée tant qu'aucune zone n'est posée : celle qu'un clic prendrait.
    survol: Option<RECT>,
    /// Maj était enfoncée à l'appui : le clic prend l'écran, pas la fenêtre.
    maj: bool,
    /// La sélection a été ouverte pour FILMER une zone (voir
    /// `demander_une_zone_a_filmer`) : valider rend `Resultat::Zone`.
    pour_filmer: bool,
    /// L'écran sous le curseur au gel (client) : l'aide s'y affiche.
    ecran_aide: RECT,
}

impl Etat {
    /// Taille minimale d'une zone ajustée, en pixels **physiques**.
    fn minimum(&self) -> i32 {
        poignees::COTE_MINIMAL * self.dpi / 96
    }

    /// Largeur de la bande de prise d'une poignée, en pixels **physiques**.
    fn bande(&self) -> i32 {
        poignees::BANDE_PRISE * self.dpi / 96
    }

    /// Où poser la barre pour la zone donnée.
    fn barre(&self, zone: RECT) -> poignees::Barre {
        poignees::barre(zone, (self.largeur, self.hauteur), self.dpi)
    }
}

impl Drop for Etat {
    fn drop(&mut self) {
        let _ = unsafe { DeleteObject(self.police.into()) };
    }
}

thread_local! {
    static ETAT: RefCell<Option<Etat>> = const { RefCell::new(None) };
    static RESULTAT: RefCell<Option<Resultat>> = const { RefCell::new(None) };
    /// Posé par `demander_une_zone_a_filmer`, consommé par `lancer`.
    static POUR_FILMER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Posé par `demander_un_ecran`, consommé par `lancer` : la prochaine capture
    /// prend CET écran (`[x, y, largeur, hauteur]` en pixels physiques), sans
    /// ouvrir de sélection.
    ///
    /// ⚠️ **LE RECTANGLE, PAS UN DRAPEAU** (corrigé le 2026-11-04 : « si je
    /// capture un écran je ne peux pas choisir quel écran et ils sont tous
    /// capturés ») — c'était le bureau virtuel ENTIER qui partait. La source
    /// « Écran » de l'applet Capture choisit désormais son moniteur.
    static ECRAN_A_CAPTURER: std::cell::Cell<Option<[i32; 4]>> = const { std::cell::Cell::new(None) };
}

/// La PROCHAINE capture prendra CET écran, **sans sélection** — choix « Écran »
/// de l'applet Capture d'écran (voir `win32::cef_capture`). `app::run` la lance
/// comme une capture ordinaire : les barres s'effacent d'abord, puis `lancer`
/// voit la demande et range l'image de l'écran.
pub fn demander_un_ecran(zone: [i32; 4]) {
    ECRAN_A_CAPTURER.with(|e| e.set(Some(zone)));
}

/// La PROCHAINE sélection servira à choisir une zone à filmer : valider rendra
/// `Resultat::Zone` au lieu d'une image. C'est `app::run` qui la lance, comme
/// une capture ordinaire (les barres s'effacent d'abord).
pub fn demander_une_zone_a_filmer() {
    POUR_FILMER.with(|p| p.set(true));
}

/// Fige l'écran et ouvre la sélection. Sans effet si une capture est déjà en cours.
pub fn lancer() {
    if est_en_cours() {
        return;
    }
    // ⭐ **UN ÉCRAN CHOISI, SANS SÉLECTION** (applet Capture d'écran, source
    // « Écran ») : même gel, même encodage, mais aucune fenêtre à ouvrir — et
    // la demande est consommée MÊME si la copie échoue, sinon la capture
    // suivante hériterait d'un écran qu'on n'a pas demandé.
    if let Some(zone) = ECRAN_A_CAPTURER.with(|e| e.take()) {
        lancer_plein_ecran(zone);
        return;
    }
    // La fenêtre des actions de la capture précédente (voir `cef_capture`) : elle
    // est déjà cachée quand la capture est demandée, mais la cacher ici aussi
    // garantit qu'elle ne figure dans aucune image.
    crate::win32::cef_capture::hide_if_visible();
    let (x, y, largeur, hauteur) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    let Some(mut clair) = Surface::nouvelle(largeur, hauteur) else { return };
    if !copier_ecran(&clair, x, y, largeur, hauteur) {
        return;
    }
    // AVANT de créer notre fenêtre : elle couvrirait tout.
    let fenetres = fenetres_visibles(POINT { x, y }, largeur, hauteur);
    let taille = (largeur * hauteur * 4) as usize;
    let Some(mut sombre) = Surface::nouvelle(largeur, hauteur) else { return };
    let Some(tampon) = Surface::nouvelle(largeur, hauteur) else { return };
    assombrir(clair.octets(taille), sombre.octets(taille));
    let Some(hwnd) = creer_fenetre(x, y, largeur, hauteur) else { return };
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
    let police = unsafe {
        CreateFontW(
            -(POLICE_ETIQUETTE * dpi / 96),
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            (DEFAULT_PITCH.0 as u32) | (FF_DONTCARE.0 as u32),
            w!("Segoe UI"),
        )
    };
    ETAT.with(|e| {
        *e.borrow_mut() = Some(Etat {
            hwnd,
            origine: POINT { x, y },
            largeur,
            hauteur,
            dpi,
            clair,
            sombre,
            tampon,
            police,
            curseur: POINT::default(),
            zone: None,
            geste: None,
            activee: false,
            appui_droit: false,
            fenetres,
            survol: None,
            maj: false,
            pour_filmer: POUR_FILMER.with(|p| p.replace(false)),
            ecran_aide: {
                let mut curseur = POINT::default();
                let _ = unsafe { GetCursorPos(&mut curseur) };
                ecran_sous(curseur, POINT { x, y })
            },
        })
    });
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        crate::win32::cef_popup::force_foreground(hwnd);
        let _ = SetFocus(Some(hwnd));
    }
    crate::diag!("[capture] écran figé : {largeur}×{hauteur} à {x},{y}");
}

/// Vrai tant que la fenêtre de sélection est ouverte — `app.rs` attend sa
/// fermeture (validée OU annulée) pour réafficher les barres.
///
/// ⛔ **L'ÉTAT SE VÉRIFIE CONTRE LA FENÊTRE, ET C'EST UN CORRECTIF.** `ETAT`
/// n'était remis à `None` que par `WM_NCDESTROY` ; une fenêtre détruite sans
/// que ce message ait pu aboutir (emprunt impossible pendant le `wndproc`)
/// laissait un état ORPHELIN. Or `lancer` sort immédiatement quand cette
/// fonction dit `true`, et `app::run` n'ouvre une capture que si
/// `phase_capture` vaut `None` : l'applet restait alors MUETTE jusqu'au
/// redémarrage — symptôme du 2026-11-04 (« la capture ne fait rien »), qui a
/// disparu à la relance.
///
/// Un état orphelin est donc RECONNU et JETÉ ici : la fenêtre n'existe plus,
/// il n'y a rien à attendre. Le journal le dit, pour que la réparation ne
/// soit pas elle-même silencieuse.
pub fn est_en_cours() -> bool {
    ETAT.with(|e| {
        // Un emprunt impossible reste CONSERVATEUR : on ne jette pas un état
        // qu'on n'a pas pu lire.
        let Ok(mut garde) = e.try_borrow_mut() else { return true };
        let Some(etat) = garde.as_ref() else { return false };
        let vivante = unsafe { IsWindow(Some(etat.hwnd)) }.as_bool();
        if la_capture_continue(true, vivante) {
            return true;
        }
        crate::diag!("[capture] état orphelin : la fenêtre de sélection n'existe plus — réarmé");
        *garde = None;
        false
    })
}

/// Décision PURE de `est_en_cours` : une capture continue-t-elle ? — extraite
/// pour être testable sans fenêtre (idiome du dépôt : la règle est pure,
/// l'appelant porte la valeur). Un état présent dont la fenêtre a disparu
/// n'est PAS une capture en cours.
fn la_capture_continue(etat_present: bool, fenetre_vivante: bool) -> bool {
    etat_present && fenetre_vivante
}

/// La dernière capture validée, une seule fois.
pub fn prendre_resultat() -> Option<Resultat> {
    RESULTAT.with(|r| r.try_borrow_mut().ok()?.take())
}

/// **LES FENÊTRES VISIBLES**, en coordonnées client (`origine` = coin du bureau
/// virtuel), du premier plan vers l'arrière — l'ordre d'`EnumWindows`.
///
/// Écartées : les fenêtres cachées, réduites ou MASQUÉES PAR DWM (celles des
/// autres bureaux virtuels, les applications suspendues — `DWMWA_CLOAKED`),
/// celles qui laissent passer la souris, et le bureau lui-même (`Progman`,
/// `WorkerW`) : un clic sur le fond d'écran prend l'écran, pas « la fenêtre du
/// bureau ». Le cadre est celui que DWM DESSINE (`DWMWA_EXTENDED_FRAME_BOUNDS`),
/// sans la bordure invisible de redimensionnement que `GetWindowRect` compte.
fn fenetres_visibles(origine: POINT, largeur: i32, hauteur: i32) -> Vec<RECT> {
    unsafe extern "system" fn une(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY : `lparam` est l'adresse du `Vec` de `fenetres_visibles`, vivant
        // pendant tout l'`EnumWindows`.
        let liste = unsafe { &mut *(lparam.0 as *mut Vec<RECT>) };
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                return BOOL(1);
            }
            if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TRANSPARENT.0 != 0 {
                return BOOL(1);
            }
            // ⛔ **NOS PROPRES FENÊTRES** : le dock couvre tout l'écran (même
            // masqué pendant la capture, il reste « visible ») — premier de la
            // liste, il aurait été « la fenêtre » sous chaque point.
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == GetCurrentProcessId() {
                return BOOL(1);
            }
            let mut masquee = 0u32;
            let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut masquee as *mut u32 as *mut c_void, 4);
            if masquee != 0 {
                return BOOL(1);
            }
            let mut classe = [0u16; 32];
            let n = GetClassNameW(hwnd, &mut classe) as usize;
            let classe = String::from_utf16_lossy(&classe[..n]);
            if classe == "Progman" || classe == "WorkerW" {
                return BOOL(1);
            }
            let mut r = RECT::default();
            let taille = std::mem::size_of::<RECT>() as u32;
            if DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut c_void, taille).is_err()
            {
                return BOOL(1);
            }
            if r.right > r.left && r.bottom > r.top {
                if liste.len() < 12 {
                    crate::diag!("[capture] fenêtre {} : {},{} → {},{}", classe, r.left, r.top, r.right, r.bottom);
                }
                liste.push(r);
            }
        }
        BOOL(1)
    }
    let mut liste: Vec<RECT> = Vec::new();
    let _ = unsafe { EnumWindows(Some(une), LPARAM(&mut liste as *mut Vec<RECT> as isize)) };
    liste
        .into_iter()
        .filter_map(|r| {
            let client = RECT {
                left: r.left - origine.x,
                top: r.top - origine.y,
                right: r.right - origine.x,
                bottom: r.bottom - origine.y,
            };
            borner(client, largeur, hauteur)
        })
        .collect()
}

/// Levier du harnais (`NEURADESK_DEBUG_CAPTURE_ECRAN=x,y,l,h`) : capture la
/// zone donnée en coordonnées d'écran SANS fenêtre de sélection — le même
/// chemin de recadrage et d'encodage que `valider`, sans aucune entrée.
///
/// ⚠️ **CE N'EST PLUS « QUE LE RELÂCHER »** : le relâcher ne capture plus, il
/// POSE la zone. Ce qui découpe, c'est le bouton Valider, donc `valider()`.
pub fn debug_capturer_zone(zone: RECT) {
    let (largeur, hauteur) = (zone.right - zone.left, zone.bottom - zone.top);
    let Some(mut surface) = Surface::nouvelle(largeur, hauteur) else { return };
    if !copier_ecran(&surface, zone.left, zone.top, largeur, hauteur) {
        return;
    }
    let bgra = surface.octets((largeur * hauteur * 4) as usize).to_vec();
    let entiere = RECT { left: 0, top: 0, right: largeur, bottom: hauteur };
    ranger(&bgra, largeur, entiere, POINT { x: zone.left, y: zone.top }, Sortie::Image);
}

/// Copie l'écran `(x, y, largeur, hauteur)` dans `surface`. `CAPTUREBLT` pour
/// inclure les fenêtres en couches (menus, info-bulles, épingles).
///
/// ⚠️ **C'EST CE `CAPTUREBLT` QUI OBLIGE LA LOUPE À NE PAS SE RECOUVRIR
/// ELLE-MÊME** : il capture AUSSI la fenêtre de la loupe, qui est une fenêtre en
/// couches. Si son rectangle recouvrait celui qu'elle échantillonne, elle se
/// filmerait et le résultat partirait en bave. `win32::loupe` garantit la
/// non-intersection par sa géométrie, et le prouve par un test — voir
/// `la_loupe_ne_recouvre_jamais_son_propre_echantillon`.
///
/// `pub(crate)` : réutilisée par `win32::loupe` (voir `Surface`).
pub(crate) fn copier_ecran(surface: &Surface, x: i32, y: i32, largeur: i32, hauteur: i32) -> bool {
    // ⭐ **ATTENDRE QUE LE BUREAU SOIT RECOMPOSÉ AVANT DE LIRE** — DEMANDE
    // (2026-11-04) : « la capture est trop tôt et je vois l'image fantôme de la
    // boîte de dialogue ». La fenêtre de l'applet venait d'être cachée, mais
    // `BitBlt` lisait la DERNIÈRE image composée par DWM, qui la contenait
    // encore : le gel pouvait suivre le masquage de quelques microsecondes (le
    // journal portait « gel apres 1.4µs »). `DwmFlush` bloque jusqu'à ce que la
    // composition en cours soit terminée — donc jusqu'à ce que le masquage soit
    // dans l'image lue.
    unsafe {
        let _ = DwmFlush();
    }
    // ⚠️ **SONDE — LA SEULE PREUVE QUE L'IMAGE NE PEUT PAS LA CONTENIR** : si la
    // fenêtre de l'applet est encore visible ici, c'est le masquage qu'il faut
    // corriger, pas la composition.
    crate::diag!("[capture] fenetre de l'applet au gel : {:?}", crate::win32::cef_capture::etat_de_la_fenetre());
    unsafe {
        let ecran = GetDC(None);
        let copie = BitBlt(surface.dc, 0, 0, largeur, hauteur, Some(ecran), x, y, SRCCOPY | CAPTUREBLT);
        ReleaseDC(None, ecran);
        if let Err(err) = &copie {
            crate::diag!("[capture] BitBlt {largeur}×{hauteur} a échoué : {err}");
        }
        copie.is_ok()
    }
}

fn assombrir(clair: &[u8], sombre: &mut [u8]) {
    for (s, c) in sombre.iter_mut().zip(clair) {
        *s = ((*c as u32 * ASSOMBRISSEMENT + 127) / 255) as u8;
    }
}

/// Le rectangle (bornes exclusives) entre deux points, quel que soit le sens du glisser.
fn rectangle_entre(a: POINT, b: POINT) -> RECT {
    RECT { left: a.x.min(b.x), top: a.y.min(b.y), right: a.x.max(b.x) + 1, bottom: a.y.max(b.y) + 1 }
}

fn est_un_clic(a: POINT, b: POINT) -> bool {
    (a.x - b.x).abs() < SEUIL_CLIC && (a.y - b.y).abs() < SEUIL_CLIC
}

/// `zone` ramenée dans `[0, largeur) × [0, hauteur)` ; `None` si elle est vide.
fn borner(zone: RECT, largeur: i32, hauteur: i32) -> Option<RECT> {
    let r = RECT {
        left: zone.left.clamp(0, largeur),
        top: zone.top.clamp(0, hauteur),
        right: zone.right.clamp(0, largeur),
        bottom: zone.bottom.clamp(0, hauteur),
    };
    (r.right > r.left && r.bottom > r.top).then_some(r)
}

/// Découpe `zone` d'une image BGRA de `largeur` pixels de large, en RGBA opaque
/// (une capture d'écran n'a pas d'alpha : `BitBlt` y laisse 0).
///
/// `pub(crate)` : réutilisée par `win32::loupe` (voir `Surface`).
pub(crate) fn recadrer_en_rgba(bgra: &[u8], largeur: i32, zone: RECT) -> Vec<u8> {
    let l = (zone.right - zone.left) as usize;
    let mut rgba = Vec::with_capacity(l * (zone.bottom - zone.top) as usize * 4);
    for y in zone.top..zone.bottom {
        let debut = (y * largeur + zone.left) as usize * 4;
        for p in bgra[debut..debut + l * 4].chunks_exact(4) {
            rgba.extend_from_slice(&[p[2], p[1], p[0], 255]);
        }
    }
    rgba
}

/// Encode `zone` de l'image BGRA en PNG et la range pour `prendre_resultat`.
/// `origine` : le coin de l'image en coordonnées d'écran.
fn ranger(bgra: &[u8], largeur: i32, zone: RECT, origine: POINT, sortie: Sortie) {
    let rgba = recadrer_en_rgba(bgra, largeur, zone);
    let (l, h) = ((zone.right - zone.left) as u32, (zone.bottom - zone.top) as u32);
    match crate::win32::clipboard::encode_png(l, h, &rgba) {
        Some(png) => {
            crate::diag!("[capture] zone {l}×{h} capturée ({} octets de PNG)", png.len());
            let ecran = RECT {
                left: zone.left + origine.x,
                top: zone.top + origine.y,
                right: zone.right + origine.x,
                bottom: zone.bottom + origine.y,
            };
            let resultat = match sortie {
                Sortie::Image => Resultat::Image { png, zone: ecran },
                Sortie::Texte => Resultat::Texte { png },
            };
            RESULTAT.with(|r| {
                if let Ok(mut r) = r.try_borrow_mut() {
                    *r = Some(resultat);
                }
            });
        }
        None => crate::diag!("[capture] l'encodage PNG de {l}×{h} a échoué"),
    }
}

/// Fige UN ÉCRAN et le range pour `prendre_resultat` — la source « Écran » de
/// l'applet Capture d'écran, sans sélection ni fenêtre.
///
/// ⚠️ **LE MÊME GEL QUE `lancer`** : `copier_ecran` sur le rectangle demandé,
/// puis `ranger` — donc le même PNG, au même endroit, par le même chemin que
/// toute autre capture (fenêtre des actions ou éditeur).
///
/// ⚠️ **L'ORIGINE VOYAGE AVEC L'IMAGE** : `ranger` reçoit `POINT { x, y }`, la
/// position de l'écran dans le bureau virtuel — c'est elle qui fait apparaître
/// la fenêtre des actions sur l'écran d'où l'on vient de capturer.
fn lancer_plein_ecran(ecran: [i32; 4]) {
    // La fenêtre des actions de la capture précédente reste hors de l'image
    // (même règle que `lancer`).
    crate::win32::cef_capture::hide_if_visible();
    let [x, y, largeur, hauteur] = ecran;
    let Some(mut clair) = Surface::nouvelle(largeur, hauteur) else { return };
    if !copier_ecran(&clair, x, y, largeur, hauteur) {
        return;
    }
    let taille = (largeur * hauteur * 4) as usize;
    let zone = RECT { left: 0, top: 0, right: largeur, bottom: hauteur };
    ranger(clair.octets(taille), largeur, zone, POINT { x, y }, Sortie::Image);
}

fn creer_fenetre(x: i32, y: i32, largeur: i32, hauteur: i32) -> Option<HWND> {
    static CLASSE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).ok()?.into();
        CLASSE.get_or_init(|| {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
                lpszClassName: w!("NeuraDeskCaptureEcran"),
                ..Default::default()
            };
            RegisterClassExW(&wc);
        });
        match CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("NeuraDeskCaptureEcran"),
            w!("NeuraDesk — Capture d'écran"),
            WS_POPUP,
            x,
            y,
            largeur,
            hauteur,
            None,
            None,
            Some(instance),
            None,
        ) {
            Ok(hwnd) => Some(hwnd),
            Err(err) => {
                crate::diag!("[capture] CreateWindowExW a échoué : {err}");
                None
            }
        }
    }
}

/// Compose la zone `rc` (client) dans le tampon puis la pose sur `hdc` :
/// l'écran sombre, la sélection claire, son cadre et son étiquette.
fn peindre(hdc: HDC, rc: RECT) {
    ETAT.with(|e| {
        let Ok(e) = e.try_borrow() else { return };
        let Some(e) = e.as_ref() else { return };
        let (l, h) = (rc.right - rc.left, rc.bottom - rc.top);
        unsafe {
            let _ = BitBlt(e.tampon.dc, rc.left, rc.top, l, h, Some(e.sombre.dc), rc.left, rc.top, SRCCOPY);
            if let Some(sel) = zone_affichee(e) {
                let (sl, sh) = (sel.right - sel.left, sel.bottom - sel.top);
                let _ = BitBlt(e.tampon.dc, sel.left, sel.top, sl, sh, Some(e.clair.dc), sel.left, sel.top, SRCCOPY);
                let dpi = e.dpi;
                dessiner_cadre(e.tampon.dc, sel, EPAISSEUR_CADRE * dpi / 96);
                if zone_ajustable(e) {
                    // ⛔ **L'ÉTIQUETTE ET LA BARRE S'EXCLUENT** — et pas par goût :
                    // elles occupent le MÊME créneau autour de la zone (au-dessus
                    // si ça tient, sinon dedans), donc les dessiner toutes les deux
                    // les ferait se recouvrir. La barre porte les dimensions, ce
                    // qui est exactement ce que l'étiquette apportait.
                    let barre = e.barre(sel);
                    dessiner_poignees(e.tampon.dc, sel, dpi);
                    dessiner_barre(e.tampon.dc, e.police, &barre, sel, dpi, e.curseur);
                } else {
                    dessiner_etiquette(e.tampon.dc, e.police, sel, dpi, e.largeur);
                }
            } else if let Some(fenetre) = e.survol {
                // La fenêtre qu'un clic prendrait : claire et encadrée, comme une zone.
                let (fl, fh) = (fenetre.right - fenetre.left, fenetre.bottom - fenetre.top);
                let _ = BitBlt(e.tampon.dc, fenetre.left, fenetre.top, fl, fh, Some(e.clair.dc), fenetre.left, fenetre.top, SRCCOPY);
                dessiner_cadre(e.tampon.dc, fenetre, EPAISSEUR_CADRE * e.dpi / 96);
                dessiner_etiquette(e.tampon.dc, e.police, fenetre, e.dpi, e.largeur);
            }
            if e.zone.is_none() && e.geste.is_none() {
                dessiner_aide(e.tampon.dc, e.police, e.ecran_aide, e.dpi);
            }
            let _ = BitBlt(hdc, rc.left, rc.top, l, h, Some(e.tampon.dc), rc.left, rc.top, SRCCOPY);
        }
    });
}

/// Les huit carrés de préhension.
///
/// ⚠️ **ILS SONT UNE INDICATION, PAS LA CIBLE** : la prise réelle couvre toute la
/// bande le long d'un bord (voir `poignees::BANDE_PRISE`), comme les bordures de
/// Windows. Les dessiner aux huit positions est ce qui rend la bande
/// compréhensible — sans eux, on ne saurait pas que la zone est ajustable.
unsafe fn dessiner_poignees(dc: HDC, zone: RECT, dpi: i32) {
    unsafe {
        let pinceau = CreateSolidBrush(COLORREF(COULEUR_POIGNEE));
        for (_, carre) in poignees::rects_poignees(zone, poignees::COTE_POIGNEE * dpi / 96) {
            FillRect(dc, &carre, pinceau);
        }
        let _ = DeleteObject(pinceau.into());
    }
}

/// La barre `[ L × H ] [ Valider ] [ Texte ] [ Annuler ]`, avec le bouton survolé éclairci.
///
/// ⚠️ **LE SURVOL NE REPEINT PAS TOUTE LA FENÊTRE** : il est peint ICI, dans le
/// tampon, à l'occasion d'un repeint qui a déjà lieu pour une autre raison. Voir
/// `WM_MOUSEMOVE`, qui n'invalide que pendant un geste.
unsafe fn dessiner_barre(dc: HDC, police: HFONT, barre: &poignees::Barre, zone: RECT, dpi: i32, curseur: POINT) {
    unsafe {
        let fond = CreateSolidBrush(COLORREF(COULEUR_ETIQUETTE));
        FillRect(dc, &barre.fond, fond);
        let _ = DeleteObject(fond.into());
        let survol = CreateSolidBrush(COLORREF(COULEUR_BOUTON));
        for r in [barre.valider, barre.texte, barre.annuler] {
            if poignees::dans(curseur, r) {
                FillRect(dc, &r, survol);
            }
        }
        let _ = DeleteObject(survol.into());

        let ancienne = SelectObject(dc, police.into());
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x00FF_FFFF));
        // ⚠️ `dpi` n'entre pas dans le dessin du texte : la police a été créée à
        // la bonne taille dans `lancer`, et les cases sont déjà mises à
        // l'échelle par `poignees::barre`.
        let _ = dpi;
        let centrer = DT_CENTER | DT_VCENTER | DT_SINGLELINE;
        let mut dimensions: Vec<u16> =
            format!("{} × {}", zone.right - zone.left, zone.bottom - zone.top).encode_utf16().collect();
        let mut boite = barre.etiquette;
        DrawTextW(dc, &mut dimensions, &mut boite, centrer);
        for (libelle, r) in [("Valider", barre.valider), ("Texte", barre.texte), ("Annuler", barre.annuler)] {
            let mut texte: Vec<u16> = libelle.encode_utf16().collect();
            let mut boite = r;
            DrawTextW(dc, &mut texte, &mut boite, centrer);
        }
        SelectObject(dc, ancienne);
    }
}

/// Le cadre, À L'EXTÉRIEUR de la sélection : il ne masque aucun pixel capturé.
unsafe fn dessiner_cadre(dc: HDC, sel: RECT, e: i32) {
    unsafe {
        let pinceau = CreateSolidBrush(COLORREF(COULEUR_CADRE));
        let (g, h, d, b) = (sel.left - e, sel.top - e, sel.right + e, sel.bottom + e);
        for bande in [
            RECT { left: g, top: h, right: d, bottom: sel.top },
            RECT { left: g, top: sel.bottom, right: d, bottom: b },
            RECT { left: g, top: sel.top, right: sel.left, bottom: sel.bottom },
            RECT { left: sel.right, top: sel.top, right: d, bottom: sel.bottom },
        ] {
            FillRect(dc, &bande, pinceau);
        }
        let _ = DeleteObject(pinceau.into());
    }
}

/// L'aide des gestes, centrée en haut de l'écran `ecran` (client).
unsafe fn dessiner_aide(dc: HDC, police: HFONT, ecran: RECT, dpi: i32) {
    let mut texte: Vec<u16> = AIDE.encode_utf16().collect();
    let (h, marge) = (AIDE_HAUTEUR * dpi / 96, AIDE_MARGE * dpi / 96);
    let l = (760 * dpi / 96).min(ecran.right - ecran.left);
    let x = ecran.left + (ecran.right - ecran.left - l) / 2;
    let mut boite = RECT { left: x, top: ecran.top + marge, right: x + l, bottom: ecran.top + marge + h };
    unsafe {
        let fond = CreateSolidBrush(COLORREF(COULEUR_ETIQUETTE));
        FillRect(dc, &boite, fond);
        let _ = DeleteObject(fond.into());
        let ancienne = SelectObject(dc, police.into());
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x00FF_FFFF));
        DrawTextW(dc, &mut texte, &mut boite, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(dc, ancienne);
    }
}

/// « L × H » au-dessus de la sélection — dedans si elle touche le haut de l'écran.
unsafe fn dessiner_etiquette(dc: HDC, police: HFONT, sel: RECT, dpi: i32, largeur_ecran: i32) {
    let texte: Vec<u16> =
        format!("{} × {}", sel.right - sel.left, sel.bottom - sel.top).encode_utf16().collect();
    let (l, h, marge) = (110 * dpi / 96, 24 * dpi / 96, 6 * dpi / 96);
    let x = sel.left.min(largeur_ecran - l).max(0);
    let y = if sel.top - marge - h >= 0 { sel.top - marge - h } else { sel.top + marge };
    let mut boite = RECT { left: x, top: y, right: x + l, bottom: y + h };
    unsafe {
        let fond = CreateSolidBrush(COLORREF(COULEUR_ETIQUETTE));
        FillRect(dc, &boite, fond);
        let _ = DeleteObject(fond.into());
        let ancienne = SelectObject(dc, police.into());
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x00FF_FFFF));
        let mut texte = texte;
        DrawTextW(dc, &mut texte, &mut boite, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(dc, ancienne);
    }
}

/// **LE BOUTON « Valider »** : la zone affichée part au PNG.
///
/// ⚠️ **AUCUN PARAMÈTRE, ET C'EST VOULU** : elle passe par `zone_affichee`, la
/// MÊME fonction que le dessin. Elle valide donc exactement ce que l'utilisateur
/// voit — un rectangle passé en argument rouvrirait la porte à l'écart d'un pixel.
///
/// ⛔ **À N'APPELER QUE HORS DE TOUT EMPRUNT DE `ETAT`.** `try_borrow_mut` rend
/// `Err` au lieu de bloquer, donc un `?` sur un emprunt déjà pris sort **sans
/// PNG et sans un mot** ; c'est pourquoi la décision est calculée dans un `with`
/// et l'action exécutée APRÈS (voir `WM_LBUTTONUP`).
fn valider(sortie: Sortie) {
    let paquet = ETAT.with(|e| {
        let mut garde = e.try_borrow_mut().ok()?;
        let etat = garde.as_mut()?;
        let zone = zone_affichee(etat)?;
        let taille = (etat.largeur * etat.hauteur * 4) as usize;
        let largeur = etat.largeur;
        if etat.pour_filmer {
            return Some((Vec::new(), largeur, zone, etat.origine, true));
        }
        Some((etat.clair.octets(taille).to_vec(), largeur, zone, etat.origine, false))
    });
    match paquet {
        Some((_, _, zone, origine, true)) => {
            let ecran = RECT {
                left: zone.left + origine.x,
                top: zone.top + origine.y,
                right: zone.right + origine.x,
                bottom: zone.bottom + origine.y,
            };
            crate::diag!("[capture] zone a filmer : {},{} -> {},{}", ecran.left, ecran.top, ecran.right, ecran.bottom);
            RESULTAT.with(|r| {
                if let Ok(mut r) = r.try_borrow_mut() {
                    *r = Some(Resultat::Zone { zone: ecran });
                }
            });
        }
        Some((bgra, largeur, zone, origine, false)) => {
            // « La même zone que la dernière fois » : retenue en coordonnées
            // d'ÉCRAN, puisque le bureau virtuel peut changer d'ici là.
            crate::config::save_derniere_zone_capture([
                zone.left + origine.x,
                zone.top + origine.y,
                zone.right + origine.x,
                zone.bottom + origine.y,
            ]);
            ranger(&bgra, largeur, zone, origine, sortie)
        }
        // ⚠️ **LE DIRE.** Sans cette ligne, un emprunt impossible serait
        // indiscernable d'un clic sur « Annuler » : la fenêtre se ferme, et
        // l'utilisateur croit avoir capturé.
        None => crate::diag!("[capture] validation sans zone exploitable — rien n'a ete produit"),
    }
}

/// L'écran qui contient `point` (écran), en coordonnées client (`origine` = coin du bureau virtuel).
fn ecran_sous(point: POINT, origine: POINT) -> RECT {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let _ = GetMonitorInfoW(MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST), &mut info);
    }
    let r = info.rcMonitor;
    RECT { left: r.left - origine.x, top: r.top - origine.y, right: r.right - origine.x, bottom: r.bottom - origine.y }
}

fn fermer(hwnd: HWND) {
    let _ = unsafe { ReleaseCapture() };
    let _ = unsafe { DestroyWindow(hwnd) };
}

fn point_client(lparam: LPARAM) -> POINT {
    POINT { x: (lparam.0 & 0xFFFF) as u16 as i16 as i32, y: ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32 }
}

/// Le curseur à montrer pour ce point.
///
/// ⚠️ **C'EST CE QUI REND LA BANDE DE PRISE VISIBLE.** Les huit carrés disent
/// « ici on ajuste », mais ils ne disent pas que toute la longueur d'un bord se
/// saisit — sans retour de curseur, cette bande serait une affordance qu'on ne
/// découvre qu'en la heurtant.
fn curseur_pour(e: &Etat, point: POINT) -> HCURSOR {
    let Some(zone) = zone_affichee(e).filter(|_| zone_ajustable(e)) else {
        return unsafe { LoadCursorW(None, IDC_CROSS) }.unwrap_or_default();
    };
    let barre = e.barre(zone);
    let id = if [barre.valider, barre.texte, barre.annuler].iter().any(|r| poignees::dans(point, *r)) {
        IDC_HAND
    } else {
        match poignees::poignee_sous(zone, point, e.bande()) {
            Some(HTLEFT) | Some(HTRIGHT) => IDC_SIZEWE,
            Some(HTTOP) | Some(HTBOTTOM) => IDC_SIZENS,
            Some(HTTOPLEFT) | Some(HTBOTTOMRIGHT) => IDC_SIZENWSE,
            Some(HTTOPRIGHT) | Some(HTBOTTOMLEFT) => IDC_SIZENESW,
            // ⛔ **DANS la zone sans poignée ⇒ on la DÉPLACE**, et c'est la
            // seule des affordances qui ne se devine pas au dessin.
            Some(_) => IDC_SIZEALL,
            None if poignees::dans_la_zone(zone, point) => IDC_SIZEALL,
            None => IDC_CROSS,
        }
    };
    unsafe { LoadCursorW(None, id) }.unwrap_or_default()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            peindre(hdc, ps.rcPaint);
            let _ = unsafe { EndPaint(hwnd, &ps) };
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            let p = point_client(lparam);
            ETAT.with(|e| {
                let Ok(mut garde) = e.try_borrow_mut() else { return };
                let Some(e) = garde.as_mut() else { return };
                e.curseur = p;
                e.maj = wparam.0 & MK_SHIFT != 0;
                let libre = e.geste.is_none();
                // ⛔ **LES BOUTONS SONT TESTÉS EN PREMIER.** Quand la barre tombe
                // DEDANS la zone — écran entier, ou pas la place dehors — un clic
                // sur « Valider » doit valider, pas commencer un déplacement.
                if let Some(zone) = zone_affichee(e).filter(|_| libre && zone_ajustable(e)) {
                    let barre = e.barre(zone);
                    if poignees::dans(p, barre.valider) {
                        e.geste = Some(Geste::Bouton { action: Action::Valider });
                        return;
                    }
                    if poignees::dans(p, barre.texte) {
                        e.geste = Some(Geste::Bouton { action: Action::Texte });
                        return;
                    }
                    if poignees::dans(p, barre.annuler) {
                        e.geste = Some(Geste::Bouton { action: Action::Annuler });
                        return;
                    }
                }
                let prise = e.zone.filter(|_| libre);
                e.geste = Some(match prise {
                    Some(z) => match poignees::poignee_sous(z, p, e.bande()) {
                        Some(poignee) => Geste::Redimensionner { poignee, origine: z },
                        None if poignees::dans_la_zone(z, p) => Geste::Deplacer { origine: z, depart: p },
                        // ⚠️ Hors de la zone posée, on en dessine une AUTRE — mais
                        // `remplace: true` retiendra qu'un frémissement ne doit
                        // pas effacer la précédente (voir `WM_LBUTTONUP`).
                        None => Geste::Tracer { ancre: p, remplace: true },
                    },
                    None => Geste::Tracer { ancre: p, remplace: false },
                });
            });
            unsafe { SetCapture(hwnd) };
            let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            let p = point_client(lparam);
            let (repeindre, curseur) = ETAT.with(|e| {
                let Ok(mut garde) = e.try_borrow_mut() else { return (false, None) };
                let Some(e) = garde.as_mut() else { return (false, None) };
                let bouge = e.curseur != p;
                e.curseur = p;
                // Le survol d'une fenêtre ne repeint que quand il CHANGE de fenêtre.
                let survol = if e.zone.is_none() && e.geste.is_none() { fenetre_sous(&e.fenetres, p) } else { None };
                let survol_change = survol != e.survol;
                e.survol = survol;
                // ⛔ **ON NE REPEINT QUE PENDANT UN GESTE.** `peindre` recopie
                // l'écran ENTIER depuis `sombre` : sur un bureau multi-écrans,
                // c'est des dizaines de Mo par repeint, et repeindre à chaque
                // mouvement de souris pour un simple survol serait un bégaiement
                // permanent. Le curseur, lui, se change SANS repeindre.
                ((bouge && e.geste.is_some()) || survol_change, Some(curseur_pour(e, p)))
            });
            if let Some(curseur) = curseur {
                unsafe { SetCursor(Some(curseur)) };
            }
            if repeindre {
                let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
            }
            return LRESULT(0);
        }
        // ⛔ **UN RELÂCHER SANS GESTE EST IGNORÉ** — c'est celui du clic sur
        // l'applet, qui arrive sur cette fenêtre quand elle vient de s'ouvrir
        // sous le curseur.
        //
        // ⛔ **ET LA DÉCISION SE PREND SOUS L'EMPRUNT, L'ACTION APRÈS** :
        // `valider()` et `fermer()` prennent TOUTES DEUX l'emprunt de `ETAT` (la
        // seconde par `WM_NCDESTROY`). Or `try_borrow_mut` rend `Err` au lieu de
        // patienter : appelées sous l'emprunt, elles échoueraient EN SILENCE — la
        // fenêtre se fermerait sans produire d'image, et l'utilisateur croirait
        // avoir capturé.
        WM_LBUTTONUP => {
            let p = point_client(lparam);
            let decision = ETAT.with(|e| {
                let Ok(mut garde) = e.try_borrow_mut() else { return None };
                let Some(e) = garde.as_mut() else { return None };
                e.curseur = p;
                // ⛔ **`take()`** : un relâcher ne vaut que s'il CONCLUT un geste.
                let geste = e.geste.take()?;
                if let Geste::Bouton { action } = geste {
                    return Some(action);
                }
                // ⚠️ **`MonitorFromPoint` EST LE SEUL APPEL WIN32 DE CETTE
                // DÉCISION** : on le fait ici, et la règle elle-même part pure
                // dans `zone_posee_par_le_relacher`, où elle est testée.
                // (Nommée autrement que la fonction, qui porte le même nom.)
                let sous_l_ancre = match &geste {
                    Geste::Tracer { ancre, .. } => Some(ecran_sous(
                        POINT { x: ancre.x + e.origine.x, y: ancre.y + e.origine.y },
                        e.origine,
                    )),
                    _ => None,
                };
                let fenetre = match &geste {
                    Geste::Tracer { ancre, .. } if !e.maj => fenetre_sous(&e.fenetres, *ancre),
                    _ => None,
                };
                e.survol = None;
                let ecran = (e.largeur, e.hauteur);
                if let Some(zone) =
                    zone_posee_par_le_relacher(&geste, p, e.zone, sous_l_ancre, fenetre, ecran, e.minimum())
                {
                    // ⚠️ On ne remplace la zone que si le relâcher en a produit
                    // une : un rectangle dégénéré ne doit pas effacer une zone
                    // valide (voir la doc de `zone_posee_par_le_relacher`).
                    e.zone = Some(zone);
                }
                None
            });
            let _ = unsafe { ReleaseCapture() };
            match decision {
                Some(Action::Valider) => {
                    valider(Sortie::Image);
                    fermer(hwnd);
                }
                Some(Action::Texte) => {
                    valider(Sortie::Texte);
                    fermer(hwnd);
                }
                Some(Action::Annuler) => fermer(hwnd),
                None => {
                    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
                }
            }
            return LRESULT(0);
        }
        // ⛔ **LA CAPTURE SOURIS PEUT ÊTRE PERDUE SANS QU'ON REÇOIVE DE
        // RELÂCHER** : un Alt+Tab, un popup du shell, ou un autre processus qui
        // appelle `SetCapture`. Sans ce bras, le geste resterait ARMÉ — et comme
        // notre fenêtre couvre tout l'écran, `WM_MOUSEMOVE` continuerait
        // d'arriver : **la zone suivrait le curseur SANS BOUTON ENFONCÉ**.
        //
        // ⛔ **NE JAMAIS APPELER `fermer` ICI** : elle commence par
        // `ReleaseCapture`, qui renvoie CE message, et le gestionnaire se
        // rappellerait lui-même.
        WM_CAPTURECHANGED => {
            ETAT.with(|e| {
                if let Ok(mut garde) = e.try_borrow_mut() {
                    if let Some(e) = garde.as_mut() {
                        e.geste = None;
                    }
                }
            });
            return LRESULT(0);
        }
        // ⛔ **`SetCursor` DOIT AUSSI ÊTRE APPELÉ DANS `WM_MOUSEMOVE`, ET CE
        // N'EST PAS UNE REDONDANCE** : ce message-ci n'est PAS renvoyé à chaque
        // déplacement du curseur, seulement quand il entre dans la fenêtre ou que
        // son rang change. Ici on couvre l'arrivée ; là-bas, le suivi.
        //
        // `LRESULT(1)` empêche `DefWindowProcW` de reposer le curseur de CLASSE
        // (`IDC_CROSS`), qui écraserait le nôtre.
        WM_SETCURSOR => {
            let curseur = ETAT.with(|e| {
                let Ok(garde) = e.try_borrow() else { return None };
                let e = garde.as_ref()?;
                Some(curseur_pour(e, e.curseur))
            });
            if let Some(curseur) = curseur {
                unsafe { SetCursor(Some(curseur)) };
            }
            return LRESULT(1);
        }
        WM_RBUTTONDOWN => {
            ETAT.with(|e| {
                if let Ok(mut e) = e.try_borrow_mut() {
                    if let Some(e) = e.as_mut() {
                        e.appui_droit = true;
                    }
                }
            });
            return LRESULT(0);
        }
        // Même règle que `WM_LBUTTONUP` : le clic droit sur l'applet (capture
        // SANS masquer l'interface) est émis à l'APPUI, son relâcher tombe donc
        // sur cette fenêtre et l'annulerait aussitôt.
        WM_RBUTTONUP => {
            let appuye = ETAT.with(|e| e.try_borrow().is_ok_and(|e| e.as_ref().is_some_and(|e| e.appui_droit)));
            if appuye {
                crate::diag!("[capture] annulée (clic droit)");
                fermer(hwnd);
            }
            return LRESULT(0);
        }
        // ENTRÉE : valide la zone posée ; sans zone, pose LA DERNIÈRE (encore
        // ajustable). Décidé sous l'emprunt, exécuté après — voir `WM_LBUTTONUP`.
        WM_KEYDOWN if wparam.0 == VK_RETURN.0 as usize => {
            let valider_maintenant = ETAT.with(|e| {
                let Ok(mut garde) = e.try_borrow_mut() else { return false };
                let Some(e) = garde.as_mut() else { return false };
                if e.geste.is_some() {
                    return false;
                }
                if e.zone.is_some() {
                    return true;
                }
                let derniere = crate::config::load_derniere_zone_capture().and_then(|[g, h, d, b]| {
                    let client = RECT { left: g - e.origine.x, top: h - e.origine.y, right: d - e.origine.x, bottom: b - e.origine.y };
                    borner(client, e.largeur, e.hauteur)
                });
                if derniere.is_none() {
                    crate::diag!("[capture] Entrée : aucune dernière zone à reprendre");
                }
                e.zone = derniere;
                e.survol = None;
                false
            });
            if valider_maintenant {
                valider(Sortie::Image);
                fermer(hwnd);
            } else {
                let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
            }
            return LRESULT(0);
        }
        WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
            crate::diag!("[capture] annulée (Échap)");
            fermer(hwnd);
            return LRESULT(0);
        }
        // Une fenêtre plein écran au premier plan qui resterait après un
        // Alt+Tab masquerait tout le bureau : perdre l'activation annule. Pas
        // AVANT la première activation — `force_foreground` peut échouer, et la
        // fenêtre doit alors rester utilisable au premier clic.
        WM_ACTIVATE => {
            let inactive = (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE;
            let annuler = ETAT.with(|e| {
                let Ok(mut e) = e.try_borrow_mut() else { return false };
                let Some(e) = e.as_mut() else { return false };
                if !inactive {
                    e.activee = true;
                }
                inactive && e.activee
            });
            if annuler {
                // QUI a pris le premier plan — DÉFAUT SIGNALÉ (2026-10-16) : « je ne
                // peux faire la capture qu'une fois » ; le journal montrait huit
                // annulations de suite, 70 ms après le gel, sans nommer la fenêtre.
                // `lParam` : la fenêtre activée à notre place (peut être nulle).
                let autre = HWND(lparam.0 as _);
                crate::diag!("[capture] annulée (premier plan perdu) au profit de {}", crate::win32::plein_ecran_zone::decrire(autre));
                fermer(hwnd);
                return LRESULT(0);
            }
        }
        WM_NCDESTROY => {
            // `try_borrow_mut` : jamais `borrow` dans un wndproc (voir la mémoire
            // « RefCell reentrancy »).
            let libere = ETAT.with(|e| e.try_borrow_mut().map(|mut e| *e = None).is_ok());
            if !libere {
                crate::diag!("[capture] fenêtre détruite pendant un emprunt : l'écran figé reste en mémoire");
            }
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⛔ **UN ÉTAT SANS FENÊTRE N'EST PAS UNE CAPTURE EN COURS** — correctif du
    /// « l'applet ne fait plus rien » (2026-11-04). `ETAT` n'était remis à
    /// `None` que par `WM_NCDESTROY` ; un état orphelin faisait sortir
    /// `lancer` sans rien faire, et `app::run` n'ouvrait même pas sa phase
    /// (elle est gardée par `phase_capture.is_none()`), jusqu'au redémarrage.
    #[test]
    fn un_etat_sans_fenetre_n_est_pas_une_capture_en_cours() {
        assert!(!la_capture_continue(false, false));
        assert!(!la_capture_continue(false, true));
        assert!(la_capture_continue(true, true), "fenêtre vivante : la capture continue");
        assert!(
            !la_capture_continue(true, false),
            "fenêtre détruite : l'état est orphelin, il n'y a plus rien à attendre"
        );
    }

    #[test]
    fn le_rectangle_ne_depend_pas_du_sens_du_glisser() {
        let a = POINT { x: 50, y: 10 };
        let b = POINT { x: 20, y: 40 };
        let attendu = RECT { left: 20, top: 10, right: 51, bottom: 41 };
        assert_eq!(rectangle_entre(a, b), attendu);
        assert_eq!(rectangle_entre(b, a), attendu);
    }

    #[test]
    fn un_petit_glisser_est_un_clic() {
        let a = POINT { x: 100, y: 100 };
        assert!(est_un_clic(a, POINT { x: 102, y: 98 }));
        assert!(!est_un_clic(a, POINT { x: 103, y: 100 }));
        assert!(!est_un_clic(a, POINT { x: 100, y: 90 }));
    }

    #[test]
    fn la_zone_est_bornee_a_l_ecran() {
        let zone = RECT { left: -10, top: 5, right: 50, bottom: 500 };
        assert_eq!(borner(zone, 40, 100), Some(RECT { left: 0, top: 5, right: 40, bottom: 100 }));
        assert_eq!(borner(RECT { left: 60, top: 0, right: 80, bottom: 10 }, 40, 100), None);
    }

    #[test]
    fn le_recadrage_permute_en_rgba_opaque() {
        // Image 3×2 en BGRA, alpha 0 comme après `BitBlt` ; chaque pixel porte
        // son index dans le bleu.
        let bgra: Vec<u8> = (0..6u8).flat_map(|i| [i, 100, 200, 0]).collect();
        let zone = RECT { left: 1, top: 0, right: 3, bottom: 2 };
        assert_eq!(
            recadrer_en_rgba(&bgra, 3, zone),
            vec![200, 100, 1, 255, 200, 100, 2, 255, 200, 100, 4, 255, 200, 100, 5, 255]
        );
    }

    #[test]
    fn l_assombrissement_garde_une_part_de_la_lumiere() {
        let mut sombre = [0u8; 3];
        assombrir(&[255, 0, 128], &mut sombre);
        assert_eq!(sombre, [100, 0, 50]);
    }

    /// Lit réellement l'écran (sans rien afficher ni simuler) : le chemin
    /// `BitBlt` → recadrage → PNG complet, comme au relâcher.
    #[test]
    fn une_zone_de_l_ecran_devient_un_png() {
        debug_capturer_zone(RECT { left: 0, top: 0, right: 32, bottom: 16 });
        let Some(Resultat::Image { png, zone }) = prendre_resultat() else { panic!("une image") };
        assert_eq!(zone, RECT { left: 0, top: 0, right: 32, bottom: 16 }, "la zone est rendue en coordonnées d'écran");
        let (l, h, rgba) = crate::win32::clipboard::decode_png(&png).expect("un PNG lisible");
        assert_eq!((l, h), (32, 16));
        assert!(rgba.chunks_exact(4).all(|p| p[3] == 255));
        assert!(prendre_resultat().is_none(), "un résultat ne se prend qu'une fois");
    }
}

#[cfg(test)]
mod zone_ajustable_tests {
    use super::*;

    const ECRAN: (i32, i32) = (1920, 1080);
    const POSEE: RECT = RECT { left: 400, top: 300, right: 800, bottom: 600 };
    const MINI: i32 = 8;

    fn p(x: i32, y: i32) -> POINT {
        POINT { x, y }
    }

    /// ⭐ **CE QUE LE RELÂCHER POSE EST CE QUI ÉTAIT DESSINÉ**, parce que les deux
    /// passent par `zone_du_geste` — la seule façon de les faire diverger serait
    /// d'écrire un second calcul. Ce test fige les valeurs des trois gestes.
    #[test]
    fn chaque_geste_rend_le_rectangle_attendu() {
        let redim = Geste::Redimensionner { poignee: HTLEFT, origine: POSEE };
        assert_eq!(
            zone_du_geste(Some(&redim), p(250, 900), Some(POSEE), ECRAN, MINI),
            Some(RECT { left: 250, top: 300, right: 800, bottom: 600 }),
            "seul le bord gauche bouge — ni le haut, ni le bas, ni la droite"
        );
        let deplace = Geste::Deplacer { origine: POSEE, depart: p(600, 450) };
        assert_eq!(
            zone_du_geste(Some(&deplace), p(650, 500), Some(POSEE), ECRAN, MINI),
            Some(RECT { left: 450, top: 350, right: 850, bottom: 650 }),
            "translation de (50, 50), taille préservée"
        );
        let trace = Geste::Tracer { ancre: p(100, 100), remplace: false };
        assert_eq!(
            zone_du_geste(Some(&trace), p(200, 250), Some(POSEE), ECRAN, MINI),
            Some(RECT { left: 100, top: 100, right: 201, bottom: 251 }),
            "le rectangle entre l'ancre et le curseur"
        );
        assert_eq!(
            zone_du_geste(None, p(0, 0), Some(POSEE), ECRAN, MINI),
            Some(POSEE),
            "sans geste, c'est la zone POSÉE — c'est elle que le bouton validera"
        );
    }

    /// ⛔ **LE RACCOURCI « ÉCRAN ENTIER » NE VAUT QUE POUR LE PREMIER GESTE.**
    /// Passé le premier, un frémissement de deux pixels remplacerait une zone
    /// soigneusement ajustée par le moniteur entier — et il n'y aurait aucun
    /// moyen de la récupérer.
    #[test]
    fn un_fremissement_ne_remplace_pas_une_zone_posee() {
        let ecran_entier = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        let premier = Geste::Tracer { ancre: p(500, 500), remplace: false };
        assert_eq!(
            zone_posee_par_le_relacher(&premier, p(501, 501), None, Some(ecran_entier), None, ECRAN, MINI),
            Some(ecran_entier),
            "le TOUT PREMIER clic prend bien l'écran entier"
        );
        let suivant = Geste::Tracer { ancre: p(100, 100), remplace: true };
        assert_eq!(
            zone_posee_par_le_relacher(&suivant, p(101, 101), Some(POSEE), Some(ecran_entier), Some(POSEE), ECRAN, MINI),
            None,
            "mais un frémissement PLUS TARD ne remplace RIEN"
        );
    }

    /// Un VRAI glisser, lui, remplace la zone — même quand il y en avait déjà une.
    #[test]
    fn un_vrai_glisser_remplace_la_zone() {
        let g = Geste::Tracer { ancre: p(100, 100), remplace: true };
        assert_eq!(
            zone_posee_par_le_relacher(&g, p(300, 400), Some(POSEE), None, None, ECRAN, MINI),
            Some(RECT { left: 100, top: 100, right: 301, bottom: 401 })
        );
    }

    /// Le relâcher d'un geste d'ajustement produit EXACTEMENT le rectangle que le
    /// dessin montrait au même instant.
    #[test]
    fn le_relacher_d_un_ajustement_suit_le_dessin() {
        for geste in [
            Geste::Redimensionner { poignee: HTBOTTOMRIGHT, origine: POSEE },
            Geste::Deplacer { origine: POSEE, depart: p(600, 450) },
        ] {
            let curseur = p(900, 700);
            let dessine = zone_du_geste(Some(&geste), curseur, Some(POSEE), ECRAN, MINI);
            let pose = zone_posee_par_le_relacher(&geste, curseur, Some(POSEE), None, None, ECRAN, MINI);
            assert_eq!(dessine, pose, "dessiné et posé doivent être le MÊME rectangle");
            assert!(pose.is_some());
        }
    }

    /// Un bouton ne POSE aucune zone : c'est `WM_LBUTTONUP` qui le traduit en
    /// action. Sans quoi un clic sur « Valider » déplacerait la zone.
    #[test]
    fn un_bouton_ne_pose_aucune_zone() {
        let g = Geste::Bouton { action: Action::Valider };
        assert_eq!(zone_posee_par_le_relacher(&g, p(700, 640), Some(POSEE), None, None, ECRAN, MINI), None);
    }

    /// ⭐ **UN CLIC PREND LA FENÊTRE SOUS LE CURSEUR**, l'écran seulement à défaut.
    #[test]
    fn un_clic_prend_la_fenetre_survolee() {
        let ecran_entier = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        let fenetre = RECT { left: 200, top: 150, right: 900, bottom: 700 };
        let clic = Geste::Tracer { ancre: p(500, 500), remplace: false };
        assert_eq!(
            zone_posee_par_le_relacher(&clic, p(500, 500), None, Some(ecran_entier), Some(fenetre), ECRAN, MINI),
            Some(fenetre)
        );
        assert_eq!(
            zone_posee_par_le_relacher(&clic, p(500, 500), None, Some(ecran_entier), None, ECRAN, MINI),
            Some(ecran_entier),
            "sans fenêtre (le bureau, ou Maj enfoncée), l'écran"
        );
    }

    /// La fenêtre sous un point est la PREMIÈRE de la liste qui le contient : la
    /// liste va du premier plan vers l'arrière.
    #[test]
    fn la_fenetre_du_dessus_l_emporte() {
        let dessus = RECT { left: 300, top: 300, right: 600, bottom: 600 };
        let dessous = RECT { left: 0, top: 0, right: 1000, bottom: 1000 };
        assert_eq!(fenetre_sous(&[dessus, dessous], p(400, 400)), Some(dessus));
        assert_eq!(fenetre_sous(&[dessus, dessous], p(100, 100)), Some(dessous));
        assert_eq!(fenetre_sous(&[dessus], p(100, 100)), None);
    }

    /// ⚠️ **LES SEUILS SONT EN PIXELS LOGIQUES**, donc sur un écran à 200 % la
    /// bande de prise double en pixels physiques — et une zone qui resterait
    /// déplaçable à 96 dpi doit le rester à 192.
    #[test]
    fn une_petite_zone_reste_deplacable_aux_forts_dpi() {
        let zone = RECT { left: 100, top: 100, right: 140, bottom: 140 };
        assert_eq!(poignees::poignee_sous(zone, p(120, 120), 16), None, "le centre reste libre");
        // Et à 96 dpi, la même bande ne prend pas non plus tout l'intérieur.
        assert_eq!(poignees::poignee_sous(zone, p(120, 120), 8), None);
    }
}
