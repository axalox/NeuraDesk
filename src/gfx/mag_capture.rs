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
//! **La fenêtre de loupe vit sur SON PROPRE FIL** (`loupe`), lancé à la
//! première demande du thread de rendu (`marquer_fil_de_rendu`, appelé par
//! `app::run`) et arrêté par `release`, avant le device D3D. Ce fil pompe ses
//! messages (le filtre de fenêtres l'exige : sans boucle de messages,
//! `MagSetWindowFilterList` rend `ERROR_NOT_SUPPORTED`, mesuré sur
//! `ouverture-rendu`, 0x80070032) et capture une trame plein écran toutes les
//! `TRAME_PERIODE` tant que le rendu en demande. ⚠️ **Une seule loupe** : les
//! autres threads de rendu lisent `LOUPE_OK` — voir `disponible`.
//!
//! ⚠️ **COMPROMIS DE FRÉQUENCE** : la capture passe par le CPU, à la
//! résolution de l'écran (~18 ms en 4K). Elle tournait sur le fil de rendu et
//! faisait sauter des images au survol (mesuré le 2026-11-13) ; le rendu ne
//! fait plus que découper la dernière trame publiée.

use std::cell::RefCell;
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

/// L'image publiée : la dernière trame COMPLÈTE de la loupe, échangée sous
/// verrou en O(1) (voir `fil_de_la_loupe`) — le fil de rendu ne la lit que le
/// temps d'en découper une barre.
struct Image {
    ok: bool,
    header: MAGIMAGEHEADER,
    data: Vec<u8>,
}

const IMAGE_VIDE: Image = Image {
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
};

/// ⛔ `Mutex` et non `thread_local!` : le fil de la loupe l'écrit, le fil de
/// rendu la lit.
static DERNIERE_IMAGE: Mutex<Image> = Mutex::new(IMAGE_VIDE);

thread_local! {
    /// Le tampon que le CALLBACK remplit — il tourne sur le fil de la loupe,
    /// pendant `MagSetWindowSource` (garanti par sa doc). Il n'est publié dans
    /// `DERNIERE_IMAGE` qu'une fois complet.
    static TAMPON: RefCell<Image> = const { RefCell::new(IMAGE_VIDE) };
}

/// La zone du moniteur PRINCIPAL couverte par la loupe (`[gauche, haut,
/// droite, bas]`, pixels écran) — posée par le fil de la loupe à sa création.
/// Tout rectangle qui en sort (barre d'un écran secondaire) rend `None`.
static ECRAN: Mutex<Option<RECT>> = Mutex::new(None);

struct Etat {
    hote: HWND,
    loupe: HWND,
    ecran: RECT,
    /// Fenêtres déjà posées dans le filtre — on ne rappelle
    /// `MagSetWindowFilterList` que si l'ensemble change (des popups
    /// s'ouvrent et se ferment en permanence).
    exclues: Vec<HWND>,
    /// Le dernier `MagSetWindowFilterList` a-t-il été accepté ? ⛔ À FAUX, on
    /// NE CAPTURE PAS : sans le filtre, la loupe capturerait nos propres
    /// fenêtres (voir `rafraichir_exclusions`).
    filtre_ok: bool,
}

/// L'intervalle entre deux trames de la loupe. Le fond flouté ne porte que du
/// détail flou : ~16 images/s ne se distinguent pas d'un rafraîchissement à
/// chaque frame, et chaque trame coûte ~18 ms de CPU en 4K — sur LE FIL DE LA
/// LOUPE, plus sur celui du rendu.
const TRAME_PERIODE: Duration = Duration::from_millis(60);

/// Sans demande du rendu depuis ce délai, la loupe ne capture plus : aucune
/// image n'est dessinée (barres au repos), personne ne regarde son fond.
const DEMANDE_RECENTE: Duration = Duration::from_millis(500);

/// Au-delà de ce nombre d'échecs consécutifs, on considère la loupe
/// indisponible sur cette machine — `gfx::backdrop` et `gfx::quad_renderer`
/// retombent alors sur l'ancien comportement (teinte seule, alpha de
/// l'utilisateur).
const ECHECS_MAX: u32 = 30;

/// Le thread de RENDU — le seul qui LANCE le fil de la loupe. Posé par
/// `app::run` au démarrage.
static FIL_RENDU: OnceLock<std::thread::ThreadId> = OnceLock::new();

/// La loupe fonctionne-t-elle ? Posé par le fil de la loupe, lu par tous les
/// fils de rendu (voir `disponible`).
static LOUPE_OK: AtomicBool = AtomicBool::new(false);
/// La loupe a échoué (création, filtre, ou `ECHECS_MAX` trames vides) : plus
/// aucune tentative.
static ECHEC: AtomicBool = AtomicBool::new(false);
/// `release` a été appelé — plus aucun lancement, même si le rendu posait
/// encore une question pendant la sortie.
static LIBERE: AtomicBool = AtomicBool::new(false);
/// Le fil de la loupe, pour l'arrêter (`release`) : sa poignée et son
/// identifiant natif (pour `PostThreadMessageW`).
static FIL_LOUPE: Mutex<Option<(std::thread::JoinHandle<()>, u32)>> = Mutex::new(None);
/// Dernière demande d'image du rendu — voir `DEMANDE_RECENTE`.
static DEMANDE: Mutex<Option<Instant>> = Mutex::new(None);

/// Déclare le thread appelant comme thread de rendu — voir `FIL_RENDU`.
/// Idempotent : la première déclaration gagne.
pub fn marquer_fil_de_rendu() {
    let _ = FIL_RENDU.set(std::thread::current().id());
}

/// La loupe est-elle utilisable ? Lance son fil à la première demande du fil
/// de RENDU, et seulement quand le mode capturable est actif (hors de ce mode,
/// la Desktop Duplication rapide reste la source).
///
/// ⭐ **LA LOUPE A SON PROPRE FIL** — DÉFAUT RÉEL MESURÉ (2026-11-13, « ce
/// n'est pas fluide ») : la trame plein écran coûtait ~18 ms SUR LE FIL DE
/// RENDU, une frame sur quelques-unes — à 143 Hz, chacune de ces frames en
/// faisait sauter deux ou trois, et le survol saccadait. Le fil `loupe`
/// possède maintenant la fenêtre et sa boucle de messages (le filtre de
/// fenêtres en a besoin : sans elle, `MagSetWindowFilterList` rend
/// `ERROR_NOT_SUPPORTED`, mesuré sur `ouverture-rendu`) ; le rendu ne fait que
/// découper la dernière trame publiée.
///
/// ⚠️ **UNE SEULE LOUPE** : seul le fil de rendu lance le fil de la loupe ; les
/// autres (effet d'ouverture, aperçus…) lisent `LOUPE_OK`.
pub fn disponible() -> bool {
    if !crate::gfx::capture_visibility::est_capturable() {
        return false;
    }
    if LOUPE_OK.load(Ordering::Relaxed) {
        return true;
    }
    if ECHEC.load(Ordering::Relaxed) || LIBERE.load(Ordering::Relaxed) {
        return false;
    }
    if FIL_RENDU.get().is_some_and(|fil| *fil != std::thread::current().id()) {
        return false;
    }
    lancer_le_fil();
    // Le fil vient peut-être de naître : la loupe sera prête dans un instant,
    // et les premières frames se passent de flou.
    LOUPE_OK.load(Ordering::Relaxed)
}

/// Lance le fil de la loupe s'il ne tourne pas déjà.
fn lancer_le_fil() {
    let Ok(mut fil) = FIL_LOUPE.lock() else { return };
    if fil.is_some() {
        return;
    }
    let (envoi, reception) = std::sync::mpsc::channel::<u32>();
    match std::thread::Builder::new().name("loupe".into()).spawn(move || fil_de_la_loupe(envoi)) {
        Ok(poignee) => {
            // L'identifiant natif, pour `PostThreadMessageW` : le fil l'envoie
            // dès sa naissance.
            let natif = reception.recv_timeout(Duration::from_secs(2)).unwrap_or(0);
            *fil = Some((poignee, natif));
        }
        Err(err) => {
            eprintln!("[neuradesk] fil de la loupe non lancé, transparence sans flou : {err}");
            ECHEC.store(true, Ordering::Relaxed);
        }
    }
}

/// Le corps du fil de la loupe : crée la fenêtre, puis capture une trame toutes
/// les `TRAME_PERIODE` tant que le rendu en demande, en pompant ses messages.
fn fil_de_la_loupe(annoncer: std::sync::mpsc::Sender<u32>) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MsgWaitForMultipleObjectsEx, PeekMessageW, TranslateMessage, MSG,
        MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, WM_QUIT,
    };
    let _ = annoncer.send(unsafe { windows::Win32::System::Threading::GetCurrentThreadId() });
    let mut etat = match Etat::creer() {
        Ok(etat) => etat,
        Err(err) => {
            eprintln!("[neuradesk] capture par loupe indisponible, transparence sans flou : {err}");
            ECHEC.store(true, Ordering::Relaxed);
            return;
        }
    };
    if let Ok(mut ecran) = ECRAN.lock() {
        *ecran = Some(etat.ecran);
    }
    let mut echecs = 0u32;
    let mut derniere: Option<Instant> = None;
    'fil: loop {
        // La boucle de messages : le filtre de fenêtres en a besoin, et une
        // fenêtre dont le fil ne pompe pas bloque les diffusions du système.
        let mut msg = MSG::default();
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_QUIT {
                break 'fil;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if LIBERE.load(Ordering::Relaxed) {
            break;
        }
        let demandee = DEMANDE.lock().ok().and_then(|d| *d).is_some_and(|t| t.elapsed() < DEMANDE_RECENTE);
        let due = derniere.is_none_or(|t| t.elapsed() >= TRAME_PERIODE);
        // La toute première trame part sans attendre de demande : `LOUPE_OK`
        // n'est posé qu'après une image réussie, et c'est lui qui fait venir
        // les demandes.
        if (demandee || derniere.is_none()) && due {
            derniere = Some(Instant::now());
            if etat.capturer_une_trame() {
                echecs = 0;
                LOUPE_OK.store(true, Ordering::Relaxed);
            } else {
                echecs += 1;
                if echecs >= ECHECS_MAX {
                    eprintln!("[neuradesk] la loupe ne rend pas d'image, transparence sans flou");
                    ECHEC.store(true, Ordering::Relaxed);
                    LOUPE_OK.store(false, Ordering::Relaxed);
                    break;
                }
            }
        }
        let attente = derniere.map_or(TRAME_PERIODE, |t| TRAME_PERIODE.saturating_sub(t.elapsed())).max(Duration::from_millis(1));
        unsafe {
            MsgWaitForMultipleObjectsEx(None, attente.as_millis() as u32, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }
    etat.detruire();
    LOUPE_OK.store(false, Ordering::Relaxed);
}

/// Capture `rect` ([x, y, largeur, hauteur], coordonnées écran physiques) en
/// excluant nos fenêtres, puis réduit le résultat d'un facteur `facteur`
/// (moyenne des blocs — le flou qui suit efface de toute façon le détail).
///
/// Rend `(largeur, hauteur, RGBA8)` ou `None` si aucune trame n'est encore
/// prête ou si `rect` sort de l'écran principal — l'appelant garde alors sa
/// texture précédente. Ne capture RIEN lui-même : il découpe la dernière trame
/// du fil de la loupe, et lui signale qu'on regarde (`DEMANDE`).
pub fn capturer(rect: [i32; 4], facteur: u32) -> Option<(u32, u32, Vec<u8>)> {
    if !disponible() {
        return None;
    }
    if let Ok(mut demande) = DEMANDE.lock() {
        *demande = Some(Instant::now());
    }
    let ecran = (*ECRAN.lock().ok()?)?;
    let zone = RECT {
        left: rect[0],
        top: rect[1],
        right: rect[0] + rect[2].max(1),
        bottom: rect[1] + rect[3].max(1),
    };
    if zone.left < ecran.left || zone.top < ecran.top || zone.right > ecran.right || zone.bottom > ecran.bottom {
        return None;
    }
    let image = DERNIERE_IMAGE.lock().ok()?;
    if !image.ok {
        return None;
    }
    let source = Source::depuis_header(&image.header)?;
    source.reduire_region(&image.data, zone, ecran, facteur)
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

            // La source ne quittera plus la pleine écran — voir `capturer_une_trame`.
            let ecran = RECT {
                left: 0,
                top: 0,
                right: ecran_l,
                bottom: ecran_h,
            };
            Ok(Self {
                hote,
                loupe,
                ecran,
                exclues: Vec::new(),
                filtre_ok: false,
            })
        }
    }

    /// Une trame plein écran, publiée dans `DERNIERE_IMAGE` si elle est
    /// complète. `false` : filtre refusé ou image vide (voir `ECHECS_MAX`).
    fn capturer_une_trame(&mut self) -> bool {
        self.rafraichir_exclusions();
        // Voir `rafraichir_exclusions` : capturer sans filtre reviendrait à se
        // photographier soi-même.
        if !self.filtre_ok {
            return false;
        }
        TAMPON.with(|t| t.borrow_mut().ok = false);
        // ⚠️ La fenêtre de loupe ne bouge ni ne change de taille après sa
        // création (voir `creer`) : seule la source est reposée, et elle est
        // TOUJOURS la pleine écran. La redimensionner plantait le processus.
        if !unsafe { MagSetWindowSource(self.loupe, self.ecran) }.as_bool() {
            return false;
        }
        // Le callback a rempli `TAMPON` PENDANT `MagSetWindowSource`. On
        // l'ÉCHANGE avec l'image publiée : le verrou n'est tenu que le temps
        // d'un échange de pointeurs, jamais d'une copie de 33 Mo.
        TAMPON.with(|t| {
            let mut tampon = t.borrow_mut();
            if !tampon.ok {
                return false;
            }
            let Ok(mut publiee) = DERNIERE_IMAGE.lock() else { return false };
            std::mem::swap(&mut *publiee, &mut *tampon);
            true
        })
    }

    fn detruire(self) {
        unsafe {
            if !self.loupe.is_invalid() {
                let _ = DestroyWindow(self.loupe);
            }
            if !self.hote.is_invalid() {
                let _ = DestroyWindow(self.hote);
            }
            let _ = MagUninitialize();
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
    // Sur le fil de la loupe, pendant `MagSetWindowSource` : voir `TAMPON`.
    TAMPON.with(|t| {
        let Ok(mut image) = t.try_borrow_mut() else {
            return BOOL(0);
        };
        recopier(&mut image, srcdata, srcheader)
    })
}

/// Recopie l'image du callback dans `image` ; `BOOL(0)` si son format n'est
/// pas lisible.
unsafe fn recopier(image: &mut Image, srcdata: *mut core::ffi::c_void, srcheader: MAGIMAGEHEADER) -> BOOL {
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
    unsafe { std::ptr::copy_nonoverlapping(srcdata as *const u8, image.data.as_mut_ptr(), srcheader.cbSize) };
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

/// Arrête le fil de la loupe (qui détruit sa fenêtre et rend la main à
/// `Magnification.dll`) — à appeler avant que le device D3D ne meure, comme
/// `gfx::duplication::release` (voir `app::run`).
pub fn release() {
    LIBERE.store(true, Ordering::Relaxed);
    let fil = FIL_LOUPE.lock().ok().and_then(|mut f| f.take());
    if let Some((poignee, natif)) = fil {
        if natif != 0 {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                    natif,
                    windows::Win32::UI::WindowsAndMessaging::WM_QUIT,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
        // Le fil se réveille au plus toutes les `TRAME_PERIODE` et lit
        // `LIBERE` : l'attente est courte.
        let _ = poignee.join();
    }
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
