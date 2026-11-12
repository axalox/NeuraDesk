//! Capture du bureau qui EXCLUT nos propres fenêtres — le fond flouté du mode
//! « visible dans les captures d'écran » (voir `gfx::capture_visibility`).
//!
//! **LE PROBLÈME, ET POURQUOI LA DESKTOP DUPLICATION NE PEUT PAS LE RÉSOUDRE.**
//! En mode capturable, nos fenêtres portent `WDA_NONE` (c'est ce qui les rend
//! visibles dans OBS et les captures). Notre propre `IDXGIOutputDuplication`
//! les voit alors AUSSI, et leur surface DWM se relit en BLANC — le fond
//! capturé n'est plus le bureau mais nous-mêmes (bug documenté sur
//! `gfx::backdrop::capture_blurred_batch`). Or il n'existe aucun moyen de
//! s'exclure d'une seule duplication : `WDA_EXCLUDEFROMCAPTURE` est global, et
//! `WCA_EXCLUDED_FROM_DDA` retirerait la fenêtre de TOUTE capture par Desktop
//! Duplication, y compris celle d'OBS.
//!
//! **LA RÉPONSE : L'API MAGNIFICATION, ET SON FILTRE DE FENÊTRES.** C'est la
//! technique de WebRTC (`screen_capturer_win_magnifier.cc`) reprise par
//! RustDesk pour son mode confidentialité : une fenêtre de loupe CACHÉE
//! capture l'écran dans un callback, et `MagSetWindowFilterList`
//! (`MW_FILTERMODE_EXCLUDE`) retire de CETTE capture-là les fenêtres qu'on lui
//! donne. L'exclusion ne vaut que pour notre loupe : les autres applications
//! (OBS, Capture d'écran de Windows...) ne la voient pas — nos fenêtres
//! restent visibles pour elles. Le callback reçoit les pixels en mémoire
//! (`MAGIMAGEHEADER`, 32 bits par pixel), il n'y a qu'à réduire et flouter.
//!
//! **La fenêtre de loupe est créée à la première demande** (première frame du
//! mode capturable), **sur le seul thread de rendu** (`marquer_fil_de_rendu`,
//! appelé par `app::run`) et détruite par `release`, avant le device D3D —
//! même discipline que `gfx::duplication` (aucune fenêtre ne doit survivre au
//! thread qui possède le device). ⚠️ **Les AUTRES threads de rendu n'en créent
//! PAS** : `MagSetWindowFilterList` leur rend `ERROR_NOT_SUPPORTED` (mesuré sur
//! `ouverture-rendu`, 0x80070032) ; ils lisent `LOUPE_OK` — voir `disponible`.
//!
//! ⚠️ **COMPROMIS DE FRÉQUENCE** : la capture passe par le CPU, à la
//! résolution de l'écran. C'est pour cela que `gfx::backdrop` met ses textures
//! en cache quelques dizaines de millisecondes dans ce mode (voir
//! `MAG_CACHE_TTL`) au lieu de recapturer à chaque frame comme le chemin GPU.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::core::{w, BOOL, GUID};
use windows::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::HRGN;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Magnification::{
    MagInitialize, MagSetImageScalingCallback, MagSetWindowFilterList, MagSetWindowSource,
    MagUninitialize, MAGIMAGEHEADER, MW_FILTERMODE_EXCLUDE, WC_MAGNIFIERW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, IsWindowVisible,
    RegisterClassW, ShowWindow, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, WINDOW_EX_STYLE, WNDCLASSW,
    WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
};

/// Fenêtre hôte de la loupe : elle ne traite aucun message, tout va au
/// `DefWindowProcW` (montage de WebRTC). Une fonction NOMMÉE est nécessaire —
/// `WNDPROC` attend un `extern "system"`, et la liaison `windows` expose
/// `DefWindowProcW` comme une simple fonction Rust.
unsafe extern "system" fn wndproc_hote(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Classe de la fenêtre HÔTE (cachée) de la loupe. ⛔ **PAS de préfixe
/// `NeuraDesk`** : `capture_visibility::reapply_to_all_windows` traite toutes
/// les fenêtres de ce préfixe (affinité d'affichage, `WS_EX_TOOLWINDOW`), et
/// cette fenêtre n'est pas un élément de l'interface — elle ne doit pas être
/// touchée.
const CLASSE_HOTE: windows::core::PCWSTR = w!("NDMagCaptureHost");

/// Guid WIC des pixels que le callback de la loupe rend. ⚠️ **CELUI DE
/// `GUID_WICPixelFormat32bppRGBA` N'EST PAS DANS LA FAMILLE `6fddc324`** — il a
/// sa propre valeur, `f5c7ad2d-…`, et c'est LUI que la loupe livre (mesuré :
/// sans cette valeur le callback refuse chaque image et la capture est vide).
/// `GUID_WICPixelFormat32bppBGRA` reste accepté : d'autres pilotes rendent
/// l'ordre inverse.
const GUID_32BPP_RGBA: GUID = GUID::from_u128(0xf5c7ad2d_6a8d_43dd_a7a8_a29935261ae9);
const GUID_32BPP_BGRA: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc90c);

/// La dernière image rendue par le callback. `MAGIMAGEHEADER` y est recopié
/// TEEL QUEL : sa largeur, son pas et son ordre de canaux font partie de la
/// réponse, pas de la demande.
struct Image {
    ok: bool,
    header: MAGIMAGEHEADER,
    data: Vec<u8>,
}

/// ⛔ `Mutex` et non `thread_local!` : le callback `extern "system"` n'a aucun
/// contexte, il ne peut pas atteindre un état de thread par une closure.
static DERNIERE_IMAGE: Mutex<Image> = Mutex::new(Image {
    ok: false,
    header: MAGIMAGEHEADER {
        width: 0,
        height: 0,
        format: GUID::from_u128(0),
        stride: 0,
        offset: 0,
        cbSize: 0,
    },
    data: Vec::new(),
});

struct Etat {
    hote: HWND,
    loupe: HWND,
    /// La zone du moniteur PRINCIPAL, en pixels écran — la seule que la
    /// loupe, fille de l'hôte à (0,0), couvre. Tout rectangle qui en sort
    /// (barre d'un écran secondaire) rend `None` et garde son ancien fond.
    ecran: RECT,
    /// Instant de la dernière trame pleine capturée — voir `TRAME_TTL`.
    derniere: Option<Instant>,
    /// Fenêtres déjà posées dans le filtre — on ne rappelle
    /// `MagSetWindowFilterList` que si l'ensemble change (des popups
    /// s'ouvrent et se ferment en permanence).
    exclues: Vec<HWND>,
    /// Le dernier `MagSetWindowFilterList` a-t-il été accepté ? ⛔ À FAUX, on
    /// NE CAPTURE PAS : sans le filtre, la loupe capturerait nos propres
    /// fenêtres (voir `rafraichir_exclusions`).
    filtre_ok: bool,
}

/// Une trame de loupe est la PLEINE ÉCRAN (voir `Etat::ecran`), partagée par
/// tous les rectangles d'un même lot : la recapturer par barre multiplierait
/// la copie plein écran par le nombre de barres. Elle est donc gardée ce
/// délai — juste en dessous du cache par rectangle de `gfx::backdrop`, qui
/// décide lui de la fréquence du flou.
const TRAME_TTL: Duration = Duration::from_millis(30);

thread_local! {
    static ETAT: RefCell<Option<Etat>> = const { RefCell::new(None) };
    /// La loupe a déjà échoué à se créer : ne pas réessayer à chaque frame.
    static ECHEC: Cell<bool> = const { Cell::new(false) };
    /// `release` a été appelé — plus aucune création, même si le rendu
    /// posait encore une question pendant la sortie.
    static LIBERE: Cell<bool> = const { Cell::new(false) };
    /// Échecs de capture consécutifs, pour basculer définitivement vers le
    /// repli (transparence sans flou) si la loupe ne rend jamais rien.
    static ECHECS: Cell<u32> = const { Cell::new(0) };
}

/// Au-delà de ce nombre d'échecs consécutifs, on considère la loupe
/// indisponible sur cette machine — `gfx::backdrop` et `gfx::quad_renderer`
/// retombent alors sur l'ancien comportement (teinte seule, alpha de
/// l'utilisateur).
const ECHECS_MAX: u32 = 30;

/// Le thread de RENDU — celui qui possède le device D3D et donc le seul à
/// pouvoir créer la fenêtre de loupe. Posé par `app::run` au démarrage.
static FIL_RENDU: OnceLock<std::thread::ThreadId> = OnceLock::new();

/// La loupe DU THREAD DE RENDU fonctionne-t-elle ? Lu par les autres threads
/// de rendu (voir `disponible`) : sans ça, chacun tentait de créer SA loupe et
/// `MagSetWindowFilterList` la refusait (`ERROR_NOT_SUPPORTED`, 0x80070032) —
/// mesuré sur `ouverture-rendu`, qui n'a pas la boucle de messages qui rend le
/// filtre possible.
static LOUPE_OK: AtomicBool = AtomicBool::new(false);

/// Déclare `fil` (le thread appelant) comme thread de rendu — voir
/// `FIL_RENDU`. Idempotent : la première déclaration gagne.
pub fn marquer_fil_de_rendu() {
    let _ = FIL_RENDU.set(std::thread::current().id());
}

/// La loupe est-elle utilisable ? Crée la fenêtre à la première demande, et
/// seulement quand le mode capturable est actif (hors de ce mode, la Desktop
/// Duplication rapide reste la source).
///
/// ⚠️ **UNE SEULE LOUPE, SUR LE THREAD DE RENDU** (voir `FIL_RENDU`) : les
/// autres threads qui affichent une fenêtre propre (effet d'ouverture,
/// aperçus…) interrogent cette fonction pour savoir si le fond flouté existe ;
/// ils lisent `LOUPE_OK` au lieu d'en créer une, qui échouerait chez eux (voir
/// la doc de `LOUPE_OK`).
pub fn disponible() -> bool {
    if !crate::gfx::capture_visibility::est_capturable() {
        return false;
    }
    if ETAT.with(|e| e.borrow().is_some()) {
        return true;
    }
    if ECHEC.with(Cell::get) || LIBERE.with(Cell::get) {
        return false;
    }
    if let Some(fil) = FIL_RENDU.get() {
        if *fil != std::thread::current().id() {
            return LOUPE_OK.load(Ordering::Relaxed);
        }
    }
    match Etat::creer() {
        Ok(etat) => {
            ETAT.with(|e| *e.borrow_mut() = Some(etat));
            LOUPE_OK.store(true, Ordering::Relaxed);
            true
        }
        Err(err) => {
            eprintln!(
                "[neuradesk] capture par loupe indisponible (fil {:?}, natif {}, nom {:?}), transparence sans flou : {err}",
                std::thread::current().id(),
                unsafe { windows::Win32::System::Threading::GetCurrentThreadId() },
                std::thread::current().name()
            );
            ECHEC.with(|e| e.set(true));
            false
        }
    }
}

/// Capture `rect` ([x, y, largeur, hauteur], coordonnées écran physiques) en
/// excluant nos fenêtres, puis réduit le résultat d'un facteur `facteur`
/// (moyenne des blocs — le flou qui suit efface de toute façon le détail).
///
/// Rend `(largeur, hauteur, RGBA8)` ou `None` si la loupe est indisponible
/// pour cette frame — l'appelant garde alors sa texture précédente.
pub fn capturer(rect: [i32; 4], facteur: u32) -> Option<(u32, u32, Vec<u8>)> {
    if !disponible() {
        return None;
    }

    ETAT.with(|etat| {
        let mut etat = etat.borrow_mut();
        let etat = etat.as_mut()?;
        etat.rafraichir_exclusions();
        // Voir `rafraichir_exclusions` : capturer sans filtre reviendrait à se
        // photographier soi-même.
        if !etat.filtre_ok {
            noter_echec();
            return None;
        }

        // Un rectangle hors du moniteur principal (barre d'un écran
        // secondaire) n'est pas couvert par la loupe : on le dit tout de
        // suite, sans rien capturer.
        let zone = RECT {
            left: rect[0],
            top: rect[1],
            right: rect[0] + rect[2].max(1),
            bottom: rect[1] + rect[3].max(1),
        };
        if zone.left < etat.ecran.left
            || zone.top < etat.ecran.top
            || zone.right > etat.ecran.right
            || zone.bottom > etat.ecran.bottom
        {
            return None;
        }

        // La trame PLEINE ÉCRAN est partagée par tout un lot de rectangles —
        // voir `TRAME_TTL`. Une nouvelle capture repart d'un tampon marqué
        // invalide, le callback le remplit.
        let frais = etat.derniere.map_or(false, |t| t.elapsed() < TRAME_TTL);
        if !frais {
            if let Ok(mut image) = DERNIERE_IMAGE.lock() {
                image.ok = false;
            }
            unsafe {
                // ⚠️ La fenêtre de loupe ne bouge ni ne change de taille après
                // sa création (voir `Etat::creer`) : seule la source est
                // reposée, et elle est TOUJOURS la pleine écran. La
                // redimensionner entre deux rectangles plantait le processus.
                if !MagSetWindowSource(etat.loupe, etat.ecran).as_bool() {
                    return None;
                }
            }
            etat.derniere = Some(Instant::now());
        }

        // ⚠️ Le callback a été appelé PENDANT `MagSetWindowSource` (garanti par
        // sa doc) — le verrou n'est donc jamais tenu pendant l'appel.
        let image = DERNIERE_IMAGE.lock().ok()?;
        if !image.ok {
            return None;
        }
        let source = Source::depuis_header(&image.header)?;
        let reduit = source.reduire_region(&image.data, zone, etat.ecran, facteur);
        drop(image);

        match reduit {
            Some((l, h, pixels)) => {
                ECHECS.with(|e| e.set(0));
                Some((l, h, pixels))
            }
            None => {
                noter_echec();
                None
            }
        }
    })
}

/// Budget d'échecs — voir `ECHECS_MAX`. Hors de `capturer` pour ne pas tenir
/// le verrou de `DERNIERE_IMAGE` pendant le comptage.
fn noter_echec() {
    ECHECS.with(|e| {
        let n = e.get() + 1;
        e.set(n);
        if n >= ECHECS_MAX && !ECHEC.with(Cell::get) {
            eprintln!("[neuradesk] la loupe ne rend pas d'image, transparence sans flou");
            ECHEC.with(|x| x.set(true));
        }
    });
}

impl Etat {
    fn creer() -> windows::core::Result<Self> {
        unsafe {
            if !MagInitialize().as_bool() {
                return Err(windows::core::Error::from_thread());
            }

            let instance = GetModuleHandleW(None)?;
            let classe = WNDCLASSW {
                lpfnWndProc: Some(wndproc_hote),
                hInstance: instance.into(),
                lpszClassName: CLASSE_HOTE,
                ..Default::default()
            };
            // `RegisterClassW` échoue si la classe existe déjà (une seconde
            // tentative après un `release` partiel) : ce n'est pas une erreur.
            if RegisterClassW(&classe) == 0
                && GetLastError() != windows::Win32::Foundation::ERROR_CLASS_ALREADY_EXISTS
            {
                let err = windows::core::Error::from_thread();
                let _ = MagUninitialize();
                return Err(err);
            }

            let hote = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                CLASSE_HOTE,
                w!("NeuraDesk mag capture"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )?;

            // ⚠️ TAILLE FIXE, CELLE DE L'ÉCRAN — ET C'EST UN CORRECTIF MESURÉ.
            // Redimensionner la fenêtre de loupe entre deux rectangles (barre
            // du haut 3840×50, puis barre de gauche 126×2110) fait planter le
            // processus : violation d'accès dans `msvcrt` PENDANT le
            // `SetWindowPos` qui la redimensionne (0xC0000005, relevé dans
            // `neuradesk-plantages.log`). Or c'est la TAILLE DE CETTE FENÊTRE
            // qui décide de l'image rendue — mesuré : quelle que soit la source
            // demandée, le callback rend l'écran ENTIER. Elle reste donc à la
            // taille du moniteur, et c'est `capturer` qui découpe les barres.
            let (ecran_l, ecran_h) =
                (GetSystemMetrics(SM_CXSCREEN).max(1), GetSystemMetrics(SM_CYSCREEN).max(1));
            let loupe = match CreateWindowExW(
                WINDOW_EX_STYLE(0),
                WC_MAGNIFIERW,
                w!(""),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                ecran_l,
                ecran_h,
                Some(hote),
                None,
                Some(instance.into()),
                None,
            ) {
                Ok(loupe) => loupe,
                Err(err) => {
                    let _ = DestroyWindow(hote);
                    let _ = MagUninitialize();
                    return Err(err);
                }
            };

            // L'hôte est caché : la loupe rend quand même, c'est son callback
            // qui livre l'image (montage de WebRTC, voir la doc de module).
            let _ = ShowWindow(hote, SW_HIDE);

            if !MagSetImageScalingCallback(loupe, Some(recueillir_image)).as_bool() {
                let err = windows::core::Error::from_thread();
                let _ = DestroyWindow(loupe);
                let _ = DestroyWindow(hote);
                let _ = MagUninitialize();
                return Err(err);
            }

            // La source ne quittera plus la pleine écran — voir `capturer`.
            let ecran = RECT {
                left: 0,
                top: 0,
                right: ecran_l,
                bottom: ecran_h,
            };
            let mut etat = Self {
                hote,
                loupe,
                ecran,
                derniere: None,
                exclues: Vec::new(),
                filtre_ok: true,
            };
            etat.rafraichir_exclusions();
            let _ = MagSetWindowSource(loupe, ecran);
            etat.derniere = Some(Instant::now());
            Ok(etat)
        }
    }

    /// Repose la liste des fenêtres à exclure si elle a changé. L'ensemble
    /// change à chaque popup ouvert/fermé, et `EnumWindows` rend l'ordre
    /// Z : on trie par poignée pour ne pas rappeler l'API pour rien.
    ///
    /// ⚠️ **FENÊTRES VISIBLES SEULEMENT — MESURÉ.** La liste brute de
    /// `own_ui_windows` compte une soixantaine de fenêtres (les popups CEF
    /// sont TOUS préchargés et cachés) et `MagSetWindowFilterList` la refuse
    /// alors par `ERROR_NOT_SUPPORTED` (0x80070032). Or une fenêtre cachée
    /// n'est pas composée : elle n'apparaît dans aucune capture, l'exclure
    /// n'apporterait rien. Les ~dix fenêtres réellement visibles passent, elles.
    ///
    /// ⛔ **ET SI LE FILTRE ÉCHOUE QUAND MÊME, ON NE CAPTURE PAS** : sans lui
    /// la loupe verrait nos fenêtres, exactement le défaut que ce module
    /// existe pour éviter. `capturer` teste `filtre_ok` après cet appel.
    fn rafraichir_exclusions(&mut self) {
        let mut fenetres: Vec<HWND> = crate::gfx::capture_visibility::own_ui_windows()
            .into_iter()
            .filter(|hwnd| unsafe { IsWindowVisible(*hwnd).as_bool() })
            .collect();
        fenetres.sort_unstable_by_key(|hwnd| hwnd.0 as usize);
        if fenetres == self.exclues && self.filtre_ok {
            return;
        }
        let reussi = unsafe {
            MagSetWindowFilterList(
                self.loupe,
                MW_FILTERMODE_EXCLUDE,
                fenetres.len() as i32,
                if fenetres.is_empty() { std::ptr::null_mut() } else { fenetres.as_mut_ptr() },
            )
        };
        self.filtre_ok = reussi.as_bool();
        if self.filtre_ok {
            self.exclues = fenetres;
        } else {
            use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
            let classes: Vec<String> = fenetres
                .iter()
                .map(|hwnd| {
                    let mut buf = [0u16; 64];
                    let len = unsafe { GetClassNameW(*hwnd, &mut buf) };
                    String::from_utf16_lossy(&buf[..len.max(0) as usize])
                })
                .collect();
            eprintln!(
                "[neuradesk] MagSetWindowFilterList a echoue ({} fenetres {:?}) : {}",
                fenetres.len(),
                classes,
                windows::core::Error::from_thread()
            );
        }
    }
}

/// Callback `MagSetImageScalingCallback` : recopie l'image source. Rend la
/// fenêtre de loupe inutilisable si le format n'est pas celui qu'on sait lire
/// — jamais un plantage (c'est un callback C appelé par le système).
unsafe extern "system" fn recueillir_image(
    _hwnd: HWND,
    srcdata: *mut core::ffi::c_void,
    srcheader: MAGIMAGEHEADER,
    _destdata: *mut core::ffi::c_void,
    _destheader: MAGIMAGEHEADER,
    _unclipped: RECT,
    _clipped: RECT,
    _dirty: HRGN,
) -> BOOL {
    let Ok(mut image) = DERNIERE_IMAGE.lock() else {
        return BOOL(0);
    };
    image.header = srcheader;
    image.ok = false;
    if srcdata.is_null() || srcheader.cbSize == 0 {
        return BOOL(0);
    }
    if Source::depuis_header(&srcheader).is_none() {
        return BOOL(0);
    }
    // Le tampon est réutilisé d'une frame à l'autre : pas d'allocation par
    // capture au-delà de sa croissance initiale.
    image.data.resize(srcheader.cbSize, 0);
    std::ptr::copy_nonoverlapping(srcdata as *const u8, image.data.as_mut_ptr(), srcheader.cbSize);
    image.ok = true;
    BOOL(1)
}

/// La géométrie d'une image de loupe, validée une fois.
#[derive(Clone, Copy)]
struct Source {
    largeur: u32,
    hauteur: u32,
    pas: usize,
    decalage: usize,
    bgra: bool,
}

impl Source {
    fn depuis_header(header: &MAGIMAGEHEADER) -> Option<Self> {
        let bgra = if header.format == GUID_32BPP_RGBA {
            false
        } else if header.format == GUID_32BPP_BGRA {
            true
        } else {
            return None;
        };
        if header.width == 0 || header.height == 0 {
            return None;
        }
        let pas = header.stride as usize;
        if pas < header.width as usize * 4 {
            return None;
        }
        Some(Self {
            largeur: header.width,
            hauteur: header.height,
            pas,
            decalage: header.offset as usize,
            bgra,
        })
    }

    /// Découpe `zone` (coordonnées écran, `ecran` étant le rectangle que la
    /// trame COUVRE) dans l'image plein écran, la réduit par moyenne de blocs
    /// `facteur × facteur` et la rend en RGBA (alpha opaque : une capture
    /// d'écran l'est).
    ///
    /// ⭐ **LA TRAME EST TOUJOURS PLEINE ÉCRAN** : la loupe rend l'aire de sa
    /// fenêtre, pas la zone demandée à `MagSetWindowSource` (mesuré — la
    /// fenêtre bouge ou change de taille, pas l'image). C'est donc nous qui
    /// découpons chaque barre / panneau.
    fn reduire_region(
        &self,
        data: &[u8],
        zone: RECT,
        ecran: RECT,
        facteur: u32,
    ) -> Option<(u32, u32, Vec<u8>)> {
        let besoin = self
            .decalage
            .checked_add(self.pas.checked_mul(self.hauteur as usize)?)?;
        if data.len() < besoin {
            return None;
        }
        // La trame est en pixels PHYSIQUES du contexte du processus ; `ecran`
        // est en pixels écran du même contexte. Les deux coïncident pour une
        // application DPI-aware, mais le rapport est calculé quand même — une
        // loupe virtualisée ne doit pas décaler le découpage.
        let ecran_largeur = (ecran.right - ecran.left) as f32;
        let ecran_hauteur = (ecran.bottom - ecran.top) as f32;
        if ecran_largeur <= 0.0 || ecran_hauteur <= 0.0 {
            return None;
        }
        let echelle_x = self.largeur as f32 / ecran_largeur;
        let echelle_y = self.hauteur as f32 / ecran_hauteur;
        let x0 = (((zone.left - ecran.left) as f32) * echelle_x).max(0.0);
        let y0 = (((zone.top - ecran.top) as f32) * echelle_y).max(0.0);
        let x1 = (((zone.right - ecran.left) as f32) * echelle_x).min(self.largeur as f32);
        let y1 = (((zone.bottom - ecran.top) as f32) * echelle_y).min(self.hauteur as f32);
        if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
            return None;
        }

        let facteur = facteur.max(1);
        let largeur = (((zone.right - zone.left).max(1) as u32) / facteur).max(1);
        let hauteur = (((zone.bottom - zone.top).max(1) as u32) / facteur).max(1);
        let mut sortie = vec![0u8; (largeur as usize) * (hauteur as usize) * 4];

        for dy in 0..hauteur {
            let y_debut = y0 + (y1 - y0) * (dy as f32) / (hauteur as f32);
            let y_fin = y0 + (y1 - y0) * ((dy + 1) as f32) / (hauteur as f32);
            let sy0 = (y_debut.floor() as usize).min(self.hauteur as usize - 1);
            let sy1 = (y_fin.ceil().max(y_debut + 1.0) as usize).min(self.hauteur as usize);
            for dx in 0..largeur {
                let x_debut = x0 + (x1 - x0) * (dx as f32) / (largeur as f32);
                let x_fin = x0 + (x1 - x0) * ((dx + 1) as f32) / (largeur as f32);
                let sx0 = (x_debut.floor() as usize).min(self.largeur as usize - 1);
                let sx1 = (x_fin.ceil().max(x_debut + 1.0) as usize).min(self.largeur as usize);
                let mut somme = [0u32; 3];
                let mut n = 0u32;
                for sy in sy0..sy1.max(sy0 + 1) {
                    let ligne = self.decalage + sy * self.pas;
                    for sx in sx0..sx1.max(sx0 + 1) {
                        let i = ligne + sx * 4;
                        if i + 3 >= data.len() {
                            continue;
                        }
                        let (r, b) = if self.bgra {
                            (data[i + 2], data[i])
                        } else {
                            (data[i], data[i + 2])
                        };
                        somme[0] += r as u32;
                        somme[1] += data[i + 1] as u32;
                        somme[2] += b as u32;
                        n += 1;
                    }
                }
                let n = n.max(1);
                let o = ((dy as usize) * largeur as usize + dx as usize) * 4;
                sortie[o] = (somme[0] / n) as u8;
                sortie[o + 1] = (somme[1] / n) as u8;
                sortie[o + 2] = (somme[2] / n) as u8;
                sortie[o + 3] = 255;
            }
        }

        Some((largeur, hauteur, sortie))
    }
}

/// Détruit la fenêtre de loupe et rend la main à `Magnification.dll` — à
/// appeler AVANT que le device D3D ne meure, comme `gfx::duplication::release`
/// (voir `app::run`).
pub fn release() {
    let etat = ETAT.with(|e| e.borrow_mut().take());
    if let Some(etat) = etat {
        unsafe {
            if !etat.loupe.is_invalid() {
                let _ = DestroyWindow(etat.loupe);
            }
            if !etat.hote.is_invalid() {
                let _ = DestroyWindow(etat.hote);
            }
            let _ = MagUninitialize();
        }
    }
    LIBERE.with(|l| l.set(true));
    LOUPE_OK.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(largeur: u32, hauteur: u32, pas: usize, bgra: bool) -> Source {
        Source {
            largeur,
            hauteur,
            pas,
            decalage: 0,
            bgra,
        }
    }

    /// La réduction moyenne bien les blocs et rend un RGBA opaque — c'est la
    /// seule partie du module testable sans écran (la loupe demande un bureau
    /// réel).
    #[test]
    fn la_reduction_moyenne_les_blocs_et_rend_du_rgba() {
        // 4 × 2 en BGRA : colonne gauche = bleu pur, colonne droite = rouge.
        let mut data = Vec::new();
        for y in 0..2 {
            for x in 0..4 {
                let (b, g, r) = if x < 2 { (255, 0, 0) } else { (0, 0, 255) };
                data.extend_from_slice(&[b, g, r, 255]);
                let _ = y;
            }
        }
        let ecran = RECT { left: 0, top: 0, right: 4, bottom: 2 };
        let (w, h, rgba) = source(4, 2, 16, true).reduire_region(&data, ecran, ecran, 2).unwrap();
        assert_eq!((w, h), (2, 1));
        // Pixel (0) : deux pixels bleus sur deux → bleu pur en RGBA.
        assert_eq!(&rgba[0..4], &[0, 0, 255, 255]);
        assert_eq!(&rgba[4..8], &[255, 0, 0, 255]);
    }

    /// Un pas de ligne plus grand que la largeur (le cas réel : DWM aligne
    /// parfois les lignes) est respecté, et le décalage compte aussi.
    #[test]
    fn le_pas_et_le_decalage_sont_respectes() {
        let mut data = vec![0u8; 3 * 8 + 8];
        // Ligne 0 à l'offset 8, pas 8 octets : un pixel rouge, du remplissage.
        data[8..12].copy_from_slice(&[0, 0, 255, 255]);
        data[16..20].copy_from_slice(&[0, 0, 255, 255]);
        let ecran = RECT { left: 0, top: 0, right: 1, bottom: 2 };
        let (w, h, rgba) = Source {
            largeur: 1,
            hauteur: 2,
            pas: 8,
            decalage: 8,
            bgra: true,
        }
        .reduire_region(&data, ecran, ecran, 1)
        .unwrap();
        assert_eq!((w, h), (1, 2));
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
    }

    /// **SONDE — la loupe sur le bureau réel.** Elle crée la fenêtre de loupe
    /// et capture un rectangle ; à lancer à la main (`--ignored --nocapture`)
    /// quand on soupçonne un défaut de l'API sur cette machine. Elle change le
    /// réglage global le temps du test, d'où `#[ignore]` : elle n'a rien à
    /// faire dans une suite parallèle.
    #[test]
    #[ignore = "sonde : cree une fenetre de loupe sur le bureau reel"]
    fn la_loupe_capture_un_rectangle() {
        crate::gfx::capture_visibility::set_enabled(true);
        eprintln!("loupe disponible : {}", disponible());

        // Des tailles DIFFÉRENTES, comme le dock réel (haut, gauche, bas) : la
        // fenêtre de loupe ne doit plus être redimensionnée entre deux prises.
        for rect in [[0, 0, 640, 200], [0, 50, 126, 800], [0, 0, 2000, 50], [2100, 50, 126, 600]] {
            let capture = capturer(rect, 1);
            eprintln!("capture {rect:?} : {:?}", capture.as_ref().map(|(w, h, p)| (*w, *h, p.len())));
        }
        release();
        crate::gfx::capture_visibility::set_enabled(false);
    }

    /// Un en-tête inconnu ne panique pas : il rend `None` et la capture est
    /// simplement considérée comme indisponible pour cette frame.
    #[test]
    fn un_format_inconnu_est_refuse_sans_paniquer() {
        let header = MAGIMAGEHEADER {
            width: 4,
            height: 4,
            format: GUID::from_u128(1),
            stride: 16,
            offset: 0,
            cbSize: 64,
        };
        assert!(Source::depuis_header(&header).is_none());
    }
}
