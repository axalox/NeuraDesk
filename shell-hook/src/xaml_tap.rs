//! **SONDE XAML — ÉTAPE 3 : ENTENDRE L'ARBRE DE COMPOSITION DU SHELL.**
//!
//! La fenêtre du menu volant se déplace mais **ce qui est dessiné ne suit pas**
//! (mesuré à l'étape 2) : le contenu est composé par la couche XAML, pas peint par
//! le `HWND`. Pour l'atteindre, il faut s'abonner à l'arbre XAML.
//!
//! Ce module est la plomberie COM du protocole « XAML Diagnostics », écrit à la
//! main depuis `xamlom.idl` — le crate `windows` n'expose **aucun** type
//! `Windows.UI.Xaml.*` (vérifié : zéro fonctionnalité `UI_Xaml*`), donc les IID et
//! les dispositions de vtable sont recopiés ici, et **une seule erreur de
//! transcription planterait explorer**. C'est la raison pour laquelle chaque
//! interface est écrite avec le nombre EXACT de ses méthodes, dans l'ordre de
//! l'IDL, même celles dont on n'appelle qu'une seule.
//!
//! # Le protocole, et ce qui a failli le faire échouer
//!
//! 1. `InitializeXamlDiagnosticsEx` est appelée **depuis un fil à nous**, avec un
//!    nom de connexion qui n'est pas fixe : les implémentations **essaient des
//!    noms incrémentaux** jusqu'à ce que ça passe (TranslucentTB :
//!    `VisualDiagConnection{N}`). On fait pareil, avec un plafond et un journal,
//!    parce qu'un échec muet ici coûterait une session entière.
//! 2. Le shell charge alors NOTRE DLL et appelle **`DllGetClassObject`** (export
//!    obligatoire, et non `DllGetActivationFactory`) pour le CLSID qu'on lui a
//!    donné. On lui rend une fabrique qui crée un objet **`IObjectWithSite`**.
//! 3. Le shell appelle **`SetSite`** avec un **`IXamlDiagnostics`**. On le garde.
//! 4. On s'abonne par **`IVisualTreeService3::AdviseVisualTreeChange`** — et
//!    **DEPUIS UN FIL NEUF** : appelée sur le fil courant, elle bloque
//!    l'application (c'est écrit dans le mod de Windhawk, et c'est le genre
//!    d'erreur qu'on ne veut pas découvrir en direct sur le shell de quelqu'un).
//! 5. **Aucune attente bloquante** nulle part : `InitializeXamlDiagnosticsEx`
//!    passe par du COM STA, et bloquer un fil d'interface en attendant la poignée
//!    de main **interbloque explorer** (cas documenté).
//!
//! # CE QUE LA RECHERCHE A ÉTABLI, ET OÙ ELLE S'ARRÊTE
//!
//! La question posée était : **comment crocheter explorer pour les icônes de la
//! zone de notification ?** Quatre voies ont été explorées, et chacune a un
//! verdict MESURÉ — à lire avant d'en rouvrir une.
//!
//! 1. **Les crochets de symboles natifs** (`TrayUI::GetAutoHideFlags`,
//!    `TrayUI::GetMinSize`, `CSecondaryTray::CheckSize`…) : c'est ce que font les
//!    mods de Windhawk, via `WindhawkUtils::SYMBOL_HOOK` + `HookSymbols`. Possible,
//!    mais **fragile par construction** — il faut télécharger les PDB, et chaque
//!    mise à jour de Windows déplace les adresses.
//! 2. **L'ancienne voie `CTray::v_WndProc`** (interception de `WM_COPYDATA` avec
//!    la signature `0x34753423` → `TRAY_ICON_DATAW`, qui portait `hWnd` + `uID`,
//!    donc **l'IDENTITÉ de l'icône**) : **MORTE** sur la barre XAML de Windows 11.
//! 3. **L'arbre XAML** — celui-ci FONCTIONNE, et c'est tout ce fichier : poignée
//!    de main, abonnement, événements, et les handles des éléments `SystemTray.*`
//!    qui sont bel et bien relevés.
//! 4. **`RenderTargetBitmap`** — la brique qui devait donner l'IMAGE sans capture
//!    d'écran. **MESURÉ : `E_INVALIDARG` (0x80070057) sur les 64 éléments, À
//!    CHAQUE PASSE, ET DEPUIS LES DEUX FILS** — y compris depuis le fil
//!    d'interface du shell, où le rappel de ce récepteur est ramené. L'hypothèse
//!    « il faut être sur le fil du shell » est donc RÉFUTÉE : ce sont les
//!    ÉLÉMENTS qui ne sont pas rendables par cette API, pas l'appelant.
//!    (Bonne nouvelle au passage : l'appel échoue PROPREMENT — explorer n'a pas
//!    bougé — contrairement à ce que son voisinage laissait craindre.)
//!
//! **LA SEULE VOIE QUI RESTE EN SERVICE est la CAPTURE DU PANNEAU DE
//! DÉBORDEMENT**, avec l'identification par image — côté NeuraDesk, voir
//! `win32::tray_icons::{grab_overflow_panel, match_snapshot_by_image}`, et ses
//! trois pièges (l'ordre, le DPI, le signal de fenêtre) écrits dans le dépôt
//! principal.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use windows::core::{s, w, GUID, PCWSTR};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

/// Notre CLSID — {C85D8CC7-5463-40E8-A432-F5916B6427E5} est celui de Windhawk.
/// On prend le NÔTRE, distinct : rien ne justifie d'entrer en collision avec un
/// outil tiers qui pourrait tourner sur la même machine.
pub const CLSID_TAP: GUID = GUID::from_u128(0x4e55_7261_4465_736b_5f54_4150_0000_0001);

/// IID de `IVisualTreeService3` (`xamlom.idl`).
const IID_VISUAL_TREE_SERVICE3: GUID = GUID::from_u128(0x0e79c6e0_85a0_4be8_b41a_655cf1fd19bd);
/// IID de `IVisualTreeServiceCallback2` (`xamlom.idl`).
const IID_CALLBACK2: GUID = GUID::from_u128(0xbad9eb88_ae77_4397_b948_5fa2db0a19ea);
/// IID de `IXamlDiagnostics` (`xamlom.idl`) — c'est CE que `SetSite` nous livre,
/// et ce que `GetSite` doit rendre.
const IID_XAML_DIAGNOSTICS: GUID = GUID::from_u128(0x18c9e2b6_3f43_4116_9f2b_ff935d7770d2);
/// L'`IXamlDiagnostics` du shell, obtenu par `QueryInterface` **sur le fil du
/// shell** (dans `SetSite`) — jamais depuis un autre fil. On garde la référence
/// que le `QI` nous a donnée : elle ne doit pas être rendue.
static XAML_DIAGNOSTICS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// **`QueryInterface` PAR RANG SUR UN POINTEUR BRUT.** `QueryInterface` est
/// toujours la case 0 de n'importe quelle vtable COM : on peut donc la demander
/// sans connaître l'interface — ce qui est exactement ce dont on a besoin quand
/// le crate `windows` n'expose pas le type.
unsafe fn qi_depuis(objet: usize, iid: *const GUID, sortie: *mut *mut c_void) -> i32 {
    if objet == 0 {
        return E_NOINTERFACE;
    }
    let vtable = *(objet as *const *const usize);
    let query: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32 =
        std::mem::transmute(*vtable);
    query(objet as *mut c_void, iid, sortie)
}

/// IID de `IObjectWithSite` (Win32 standard).
const IID_OBJECT_WITH_SITE: GUID = GUID::from_u128(0xfc4801a3_2ba9_11cf_a229_00aa003d7352);
/// IID de `IClassFactory` (Win32 standard).
const IID_CLASS_FACTORY: GUID = GUID::from_u128(0x00000001_0000_0000_c000_000000000046);

// ---------------------------------------------------------------------------
// Les types échangés, recopiés de `xamlom.idl` dans leur ordre exact.
// ---------------------------------------------------------------------------

type InstanceHandle = u64;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ParentChildRelation {
    parent: InstanceHandle,
    child: InstanceHandle,
    child_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SourceInfo {
    file_name: *mut u16,
    line: u32,
    column: u32,
    char_position: u32,
    hash: *mut u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VisualElement {
    handle: InstanceHandle,
    src_info: SourceInfo,
    ty: *mut u16,
    name: *mut u16,
    num_children: u32,
}

/// Gardée pour l'étape suivante : lire un `BSTR` n'a sa place que **hors du fil
/// du shell**, dans un récepteur qui met en file au lieu d'écrire.
#[allow(dead_code)]
fn depuis_bstr(pointeur: *const u16) -> String {
    if pointeur.is_null() {
        return String::new();
    }
    let mut longueur = 0usize;
    unsafe {
        while *pointeur.add(longueur) != 0 && longueur < 512 {
            longueur += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(pointeur, longueur))
    }
}

// ---------------------------------------------------------------------------
// L'OBJET DE RÉCEPTION — `IObjectWithSite`
// ---------------------------------------------------------------------------

#[repr(C)]
struct SiteVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    set_site: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
    get_site: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
}

#[repr(C)]
struct ObjetSite {
    vtbl: *const SiteVtbl,
    refs: AtomicU32,
    /// Le `IXamlDiagnostics` que le shell nous donne — gardé brut, on ne fait que
    /// le lire. Zéro tant que `SetSite` n'est pas passé.
    pointeur_site: std::sync::atomic::AtomicUsize,
}

impl ObjetSite {
    fn nouveau() -> *mut ObjetSite {
        let objet = Box::new(ObjetSite {
            vtbl: &SITE_VTBL,
            refs: AtomicU32::new(1),
            pointeur_site: std::sync::atomic::AtomicUsize::new(0),
        });
        Box::into_raw(objet)
    }
}

/// L'objet de site vivant, pour que le fil d'abonnement puisse le retrouver.
/// On garde l'ADRESSE et non la référence : l'objet appartient au shell dès que
/// la fabrique l'a rendu, et il nous survit.
static OBJET_SITE_POUR_ABONNEMENT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

static SITE_VTBL: SiteVtbl = SiteVtbl {
    query_interface: site_query_interface,
    add_ref: site_add_ref,
    release: site_release,
    set_site: site_set_site,
    get_site: site_get_site,
};

unsafe extern "system" fn site_query_interface(
    _this: *mut c_void,
    iid: *const GUID,
    sortie: *mut *mut c_void,
) -> i32 {
    if iid.is_null() || sortie.is_null() {
        return E_POINTER;
    }
    let demande = *iid;
    if demande == IID_OBJECT_WITH_SITE || demande == IID_UNKNOWN {
        *sortie = _this;
        site_add_ref(_this);
        return S_OK;
    }
    *sortie = std::ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "system" fn site_add_ref(this: *mut c_void) -> u32 {
    let objet = &*(this as *const ObjetSite);
    objet.refs.fetch_add(1, Ordering::SeqCst) + 1
}

unsafe extern "system" fn site_release(this: *mut c_void) -> u32 {
    let objet = &*(this as *const ObjetSite);
    let restants = objet.refs.fetch_sub(1, Ordering::SeqCst) - 1;
    if restants == 0 {
        drop(Box::from_raw(this as *mut ObjetSite));
    }
    restants
}

/// **LE POINT D'ENTRÉE UTILE** : le shell nous remet son `IXamlDiagnostics`.
unsafe extern "system" fn site_set_site(this: *mut c_void, site: *mut c_void) -> i32 {
    crate::journaliser(&format!("XAML : SetSite reçu, site={site:?}"));
    let objet = &*(this as *const ObjetSite);
    objet.pointeur_site.store(site as usize, Ordering::SeqCst);
    if !site.is_null() {
        // **LE `QueryInterface` SE FAIT ICI, SUR LE FIL DU SHELL** — c'est le seul
        // endroit où le pointeur soit valide sans marshaling (TranslucentTB fait
        // exactement cela, dans le constructeur de son watcher, donc sur ce fil).
        // La référence rendue est GARDÉE : on ne la libère jamais.
        let mut diagnostic: *mut c_void = std::ptr::null_mut();
        let resultat = qi_depuis(site as usize, &IID_XAML_DIAGNOSTICS, &mut diagnostic);
        if resultat < 0 || diagnostic.is_null() {
            crate::journaliser(&format!(
                "XAML : IXamlDiagnostics NON obtenu (hr={resultat:#x}) — abandon"
            ));
            return S_OK;
        }
        XAML_DIAGNOSTICS.store(diagnostic as usize, Ordering::SeqCst);
        crate::journaliser(&format!("XAML : IXamlDiagnostics obtenu à {diagnostic:?}"));
        // L'ABONNEMENT SE FAIT AILLEURS — et c'est vital : `AdviseVisualTreeChange`
        // sur le fil de SETSITE bloque l'application. On rend donc la main TOUT DE
        // SUITE et on s'abonne depuis un fil à nous.
        std::thread::spawn(|| s_abonner());
    }
    S_OK
}

/// **`GetSite` DOIT RENDRE LE SITE DU SHELL, ET C'EST CE QUI A TUÉ EXPLORER.**
///
/// Ma première version rendait NOTRE objet. Le shell lui demandait alors
/// `IXamlDiagnostics` et recevait un pointeur dont la vtable n'a que cinq cases,
/// celles d'`IObjectWithSite` : il appelait les méthodes de `IXamlDiagnostics`
/// dans le vide, **dans le processus qui fait tourner le bureau**. D'où deux
/// redémarrages d'explorer, tous deux juste après `SetSite`.
///
/// TranslucentTB écrit, en une ligne : `return site.as(riid, ppvSite);` — on
/// relaie donc le `QueryInterface` **au site stocké**, jamais à nous.
unsafe extern "system" fn site_get_site(
    this: *mut c_void,
    iid: *const GUID,
    sortie: *mut *mut c_void,
) -> i32 {
    let objet = &*(this as *const ObjetSite);
    qi_depuis(objet.pointeur_site.load(Ordering::SeqCst), iid, sortie)
}

// ---------------------------------------------------------------------------
// LA FABRIQUE — `IClassFactory`, rendue par `DllGetClassObject`
// ---------------------------------------------------------------------------

#[repr(C)]
struct FabriqueVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    create_instance:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *const GUID, *mut *mut c_void) -> i32,
    lock_server: unsafe extern "system" fn(*mut c_void, i32) -> i32,
}

#[repr(C)]
struct Fabrique {
    vtbl: *const FabriqueVtbl,
    refs: AtomicU32,
}

static FABRIQUE_VTBL: FabriqueVtbl = FabriqueVtbl {
    query_interface: fabrique_query_interface,
    add_ref: fabrique_add_ref,
    release: fabrique_release,
    create_instance: fabrique_create_instance,
    lock_server: fabrique_lock_server,
};

unsafe extern "system" fn fabrique_lock_server(_this: *mut c_void, _verrou: i32) -> i32 {
    S_OK
}

static FABRIQUE: Fabrique = Fabrique { vtbl: &FABRIQUE_VTBL, refs: AtomicU32::new(1) };

// La fabrique ne contient qu'une vtable STATIQUE et un compteur atomique : elle
// est partagée entre les fils du shell par construction, et cela ne donne accès à
// aucune donnée mutable non synchronisée.
unsafe impl Sync for Fabrique {}

unsafe extern "system" fn fabrique_query_interface(
    this: *mut c_void,
    iid: *const GUID,
    sortie: *mut *mut c_void,
) -> i32 {
    if iid.is_null() || sortie.is_null() {
        return E_POINTER;
    }
    let demande = *iid;
    if demande == IID_CLASS_FACTORY || demande == IID_UNKNOWN {
        *sortie = this;
        fabrique_add_ref(this);
        return S_OK;
    }
    *sortie = std::ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "system" fn fabrique_add_ref(_this: *mut c_void) -> u32 {
    2 // Fabrique statique : jamais détruite, donc jamais libérée.
}

unsafe extern "system" fn fabrique_release(_this: *mut c_void) -> u32 {
    1
}

unsafe extern "system" fn fabrique_create_instance(
    _this: *mut c_void,
    externe: *mut c_void,
    iid: *const GUID,
    sortie: *mut *mut c_void,
) -> i32 {
    if !externe.is_null() {
        return 0x8004_0102u32 as i32; // CLASS_E_NOAGGREGATION
    }
    let objet = ObjetSite::nouveau();
    // On le retient AVANT de le rendre : c'est par lui que `SetSite` arrivera, et
    // c'est donc lui que le fil d'abonnement devra interroger.
    let _ = OBJET_SITE_POUR_ABONNEMENT.set(objet as usize);
    let resultat = site_query_interface(objet as *mut c_void, iid, sortie);
    site_release(objet as *mut c_void);
    resultat
}

/// **L'EXPORT QUE LE SHELL CHERCHE** — `xamlom.idl` : « CLSID of an object
/// implementing `IObjectWithSite` created from the injected DLL ». `DllGetClassObject`,
/// et non `DllGetActivationFactory` : les deux existent dans la nature, et seul le
/// premier est demandé ici.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    sortie: *mut *mut c_void,
) -> i32 {
    if rclsid.is_null() || riid.is_null() || sortie.is_null() {
        return E_POINTER;
    }
    if *rclsid != CLSID_TAP {
        return 0x8004_0154u32 as i32; // CLASS_E_CLASSNOTAVAILABLE
    }
    fabrique_query_interface(&FABRIQUE as *const Fabrique as *mut c_void, riid, sortie)
}

// ---------------------------------------------------------------------------
// LA RÉCEPTION — `IVisualTreeServiceCallback2`
// ---------------------------------------------------------------------------

#[repr(C)]
struct ReceptionVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    on_visual_tree_change:
        unsafe extern "system" fn(*mut c_void, ParentChildRelation, VisualElement, i32) -> i32,
    on_element_state_changed:
        unsafe extern "system" fn(*mut c_void, InstanceHandle, i32, *const u16) -> i32,
}

#[repr(C)]
struct Reception {
    vtbl: *const ReceptionVtbl,
    refs: AtomicU32,
    vus: AtomicU32,
}

static RECEPTION_VTBL: ReceptionVtbl = ReceptionVtbl {
    query_interface: reception_query_interface,
    add_ref: reception_add_ref,
    release: reception_release,
    on_visual_tree_change: reception_on_visual_tree_change,
    on_element_state_changed: reception_on_element_state_changed,
};

static RECEPTION: Reception =
    Reception { vtbl: &RECEPTION_VTBL, refs: AtomicU32::new(1), vus: AtomicU32::new(0) };

// Même raison que pour la fabrique : vtable statique et compteur atomique.
unsafe impl Sync for Reception {}

unsafe extern "system" fn reception_query_interface(
    this: *mut c_void,
    iid: *const GUID,
    sortie: *mut *mut c_void,
) -> i32 {
    if iid.is_null() || sortie.is_null() {
        return E_POINTER;
    }
    let demande = *iid;
    if demande == IID_CALLBACK2 || demande == IID_CALLBACK || demande == IID_UNKNOWN {
        *sortie = this;
        return S_OK;
    }
    *sortie = std::ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "system" fn reception_add_ref(_this: *mut c_void) -> u32 {
    2
}

unsafe extern "system" fn reception_release(_this: *mut c_void) -> u32 {
    1
}

/// **CHAQUE ÉLÉMENT DE L'ARBRE PASSE ICI.** On journalise, mais **borné** : sans
/// plafond, un arbre XAML noie le journal en une seconde et le disque avec.
/// **LE RÉCEPTEUR DOIT ÊTRE GRATUIT — et c'est ce qui a fait tomber explorer.**
///
/// Ma première version écrivait une ligne de journal, **avec `flush`**, à CHAQUE
/// élément de l'arbre. Abonné, le shell en envoie des milliers, **sur son fil
/// d'interface** : je le bloquais donc à chaque élément, et Windows a fini par
/// redémarrer le processus. Le récepteur ne fait désormais **qu'incrémenter un
/// compteur atomique** — pas de verrou, pas d'allocation, pas d'entrée-sortie.
/// C'est le fil de veille, qui n'appartient pas au shell, qui journalise.
///
/// Conséquence assumée : on ne voit plus QUEL élément passe, seulement COMBIEN.
/// C'est suffisant pour répondre à la question de cette étape — l'arbre est-il
/// vivant et nous parle-t-il ? — et pour tout le reste, il faudra une file
/// d'attente bornée, traitée hors du fil du shell.
unsafe extern "system" fn reception_on_visual_tree_change(
    _this: *mut c_void,
    _relation: ParentChildRelation,
    element: VisualElement,
    _mutation: i32,
) -> i32 {
    RECEPTION.vus.fetch_add(1, Ordering::Relaxed);
    // **ON NE GARDE QUE `SystemTray.*`, ET SANS RIEN ALLOUER.** Le relevé est un
    // `u64` écrit dans un tableau FIXE : pas de verrou, pas d'allocation, pas
    // d'entrée-sortie. C'est ce qui a fait tomber explorer une fois, et le fil
    // qui TRAITE ces handles est celui de la veille, pas celui du shell.
    if type_est_systeme(element.ty) {
        let rang = HANDLES_ECRITS.fetch_add(1, Ordering::Relaxed);
        if rang < HANDLES.len() {
            HANDLES[rang].store(element.handle, Ordering::Relaxed);
        }
    }
    // **L'APPEL SE FAIT ICI, SUR LE FIL DU SHELL — ET UNE SEULE FOIS.**
    //
    // C'est l'hypothèse à éprouver : depuis NOTRE fil, `RenderTargetBitmap` rend
    // E_INVALIDARG sur tous les éléments. Or le rappel du `VisualTreeWatcher` est
    // **ramené sur le fil d'interface par le shell** — c'est écrit noir sur blanc
    // dans le code de Windhawk — donc c'est ici, et nulle part ailleurs, qu'un
    // appel de RENDU a une chance d'aboutir.
    //
    // ⚠️ **UNE SEULE TENTATIVE, ET RIEN D'AUTRE SUR CE FIL** : pas d'écriture de
    // fichier, pas de journal — le résultat part dans une case, et c'est le fil
    // de veille qui l'écrit. C'est exactement ce qui a fait tomber explorer une
    // fois (un journal avec `flush` à chaque événement), et le rappel en reçoit
    // plus de mille par seconde.
    if type_est_systeme(element.ty) && !TENTATIVE.swap(true, Ordering::Relaxed) {
        let diagnostic = XAML_DIAGNOSTICS.load(Ordering::SeqCst);
        let issue = rendre_en_image(diagnostic, element.handle);
        if let Ok(mut garde) = RESULTAT.lock() {
            *garde = Some(issue);
        }
    }
    S_OK
}

/// La tentative depuis le fil du shell a-t-elle eu lieu ? Une seule fois.
static TENTATIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Ce que le rendu depuis le fil du shell a produit, en attendant que le fil de
/// veille vienne l'écrire. **Aucune entrée-sortie sur le fil du shell.**
static RESULTAT: Mutex<Option<Result<(u32, u32, Vec<u8>, i32), String>>> = Mutex::new(None);

/// **L'IMAGE D'UN ÉLÉMENT XAML, LUE DANS L'ARBRE — sans aucune capture d'écran.**
///
/// `IVisualTreeService2::RenderTargetBitmap` rend l'élément en bitmap. C'est ce
/// qui remplace la capture du panneau de débordement, **avec ses trois pièges en
/// moins** : plus d'ordre de capture à respecter, plus de DPI à faire concorder,
/// plus besoin que le chevron soit ouvert au bon moment — l'arbre sait dessiner
/// ce qu'il contient.
fn rendre_en_image(diagnostic: usize, handle: u64) -> Result<(u32, u32, Vec<u8>, i32), String> {
    unsafe {
        let mut service: *mut c_void = std::ptr::null_mut();
        let qi = qi_depuis(diagnostic, &IID_VISUAL_TREE_SERVICE2, &mut service);
        if qi < 0 || service.is_null() {
            return Err(format!("IVisualTreeService2 non obtenu (hr={qi:#x})"));
        }
        let vtable = *(service as *const *const usize);
        let rendre: unsafe extern "system" fn(
            *mut c_void,
            u64,
            i32,
            u32,
            u32,
            *mut *mut c_void,
        ) -> i32 = std::mem::transmute(*vtable.add(RANG_RENDER_TARGET_BITMAP));
        let mut bitmap: *mut c_void = std::ptr::null_mut();
        // `RenderTarget` (0) et non `RenderTargetAndChildren` (1) : on veut
        // l'ICÔNE, pas la barre entière.
        let appel = rendre(service, handle, 0, 256, 256, &mut bitmap);
        if appel < 0 || bitmap.is_null() {
            return Err(format!(
                "RenderTargetBitmap a rendu hr={appel:#x}, bitmap={:?}",
                bitmap
            ));
        }
        let vtable_bitmap = *(bitmap as *const *const usize);
        let decrire: unsafe extern "system" fn(*mut c_void, *mut u8) -> i32 =
            std::mem::transmute(*vtable_bitmap.add(RANG_DESCRIPTION_BITMAP));
        let mut description = [0u8; 16];
        let decrit = decrire(bitmap, description.as_mut_ptr());
        if decrit < 0 {
            return Err(format!("GetBitmapDescription a rendu hr={decrit:#x}"));
        }
        let largeur = u32::from_le_bytes(description[0..4].try_into().unwrap_or([0; 4]));
        let hauteur = u32::from_le_bytes(description[4..8].try_into().unwrap_or([0; 4]));
        let format = i32::from_le_bytes(description[8..12].try_into().unwrap_or([0; 4]));
        if largeur == 0 || hauteur == 0 || largeur > 4096 || hauteur > 4096 {
            return Err(format!("dimensions invraisemblables : {largeur}x{hauteur}"));
        }
        let copier: unsafe extern "system" fn(*mut c_void, u32, u32, *mut u8, *mut u32) -> i32 =
            std::mem::transmute(*vtable_bitmap.add(RANG_COPY_BYTES_TO));
        let mut octets = vec![0u8; (largeur * hauteur * 4) as usize];
        let mut copies = 0u32;
        let copie = copier(bitmap, 0, octets.len() as u32, octets.as_mut_ptr(), &mut copies);
        if copie < 0 {
            return Err(format!("CopyBytesTo a rendu hr={copie:#x}"));
        }
        octets.truncate(copies as usize);
        // `DXGI_FORMAT_B8G8R8A8_UNORM` vaut 87 : les octets sortent en BGRA, et
        // une texture les attend en RGBA. On échange les canaux plutôt que de
        // laisser une icône aux couleurs inversées.
        if format == 87 {
            for pixel in octets.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        Ok((largeur, hauteur, octets, format))
    }
}

/// **LE FIL QUI TRAITE LES HANDLES — hors du fil du shell.**
fn traiter_les_handles() {
    let diagnostic = XAML_DIAGNOSTICS.load(Ordering::SeqCst);
    if diagnostic == 0 {
        return;
    }
    let ecrits = HANDLES_ECRITS.load(Ordering::Relaxed).min(HANDLES.len());
    let mut rendus = 0usize;
    for rang in 0..ecrits {
        let handle = HANDLES[rang].load(Ordering::Relaxed);
        if handle == 0 {
            continue;
        }
        match rendre_en_image(diagnostic, handle) {
            Ok((largeur, hauteur, octets, format)) => {
                rendus += 1;
                let chemin = format!(r"C:\ndtarget\icone-xaml-{rang:02}.png");
                ecrire_bmp(&chemin, largeur, hauteur, &octets);
                crate::journaliser(&format!(
                    "XAML : élément {rang} rendu en {largeur}x{hauteur} (format {format}) -> {chemin}"
                ));
            }
            Err(raison) => {
                // **LA RAISON EST ÉCRITE, PAS SEULEMENT L'ÉCHEC.** Trois échecs
                // muets indiscernables ont déjà coûté une manche aujourd'hui.
                if rang < 6 {
                    crate::journaliser(&format!(
                        "XAML : élément {rang} (handle {handle:#x}) NON rendu — {raison}"
                    ));
                }
            }
        }
    }
    crate::journaliser(&format!("XAML : {rendus} élément(s) rendu(s) sur {ecrits} relevé(s)"));
    // **CE QUE LE FIL DU SHELL A RENDU — écrit ici, jamais là-bas.**
    match RESULTAT.lock().ok().and_then(|mut garde| garde.take()) {
        Some(Ok((largeur, hauteur, octets, format))) => {
            let chemin = r"C:\ndtarget\icone-depuis-fil-shell.png";
            ecrire_bmp(chemin, largeur, hauteur, &octets);
            crate::journaliser(&format!(
                "XAML : **LE FIL DU SHELL A RENDU** — {largeur}x{hauteur} (format {format}) -> {chemin}"
            ));
        }
        Some(Err(raison)) => crate::journaliser(&format!(
            "XAML : **depuis le fil du shell, le rendu échoue AUSSI** — {raison}"
        )),
        None => {
            if TENTATIVE.load(Ordering::Relaxed) {
                crate::journaliser("XAML : tentative depuis le fil du shell en cours ou sans suite");
            }
        }
    }
}

/// Écrit un BMP 32 bits — même choix que la capture d'écran de `lib.rs` : aucun
/// encodeur à écrire, et l'outil de lecture saura le convertir.
fn ecrire_bmp(chemin: &str, largeur: u32, hauteur: u32, rgba: &[u8]) {
    let mut sortie: Vec<u8> = Vec::with_capacity(rgba.len() + 54);
    sortie.extend_from_slice(b"BM");
    sortie.extend_from_slice(&(54u32 + rgba.len() as u32).to_le_bytes());
    sortie.extend_from_slice(&[0u8; 4]);
    sortie.extend_from_slice(&54u32.to_le_bytes());
    sortie.extend_from_slice(&40u32.to_le_bytes());
    sortie.extend_from_slice(&largeur.to_le_bytes());
    sortie.extend_from_slice(&(-(hauteur as i32)).to_le_bytes());
    sortie.extend_from_slice(&1u16.to_le_bytes());
    sortie.extend_from_slice(&32u16.to_le_bytes());
    sortie.extend_from_slice(&0u32.to_le_bytes());
    sortie.extend_from_slice(&(rgba.len() as u32).to_le_bytes());
    sortie.extend_from_slice(&[0u8; 16]);
    // Le BMP attend BGRA ; nos octets sont en RGBA.
    for pixel in rgba.chunks_exact(4) {
        sortie.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    match std::fs::write(chemin, &sortie) {
        Ok(()) => {}
        Err(erreur) => crate::journaliser(&format!("XAML : écriture impossible : {erreur}")),
    }
}

unsafe extern "system" fn reception_on_element_state_changed(
    _this: *mut c_void,
    _element: InstanceHandle,
    _etat: i32,
    _contexte: *const u16,
) -> i32 {
    S_OK
}

// ---------------------------------------------------------------------------
// L'ABONNEMENT
// ---------------------------------------------------------------------------

/// **ON S'ABONNE DEPUIS UN FIL À NOUS**, et c'est la précaution qui évite de
/// bloquer le shell : `AdviseVisualTreeChange` sur le fil de `SetSite` gèle
/// l'application (documenté par le mod de Windhawk).
fn s_abonner() {
    // **COM DOIT ÊTRE INITIALISÉ SUR CE FIL.** C'est un fil neuf : sans cet appel,
    // toucher la vtable d'un objet COM est une violation du contrat — la première
    // version l'a payé d'un `explorer.exe` redémarré.
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }
    // Le `IXamlDiagnostics` a été obtenu par `QueryInterface` **sur le fil du
    // shell** (dans `SetSite`) : c'est le seul endroit où il n'y a pas de
    // marshaling à faire. Ici, on ne fait plus que le LIRE.
    let diagnostic = XAML_DIAGNOSTICS.load(Ordering::SeqCst);
    if diagnostic == 0 {
        crate::journaliser("XAML : abonnement impossible — pas d'IXamlDiagnostics");
        return;
    }
    unsafe {
        let mut service: *mut c_void = std::ptr::null_mut();
        let resultat = qi_depuis(diagnostic, &IID_VISUAL_TREE_SERVICE3, &mut service);
        if resultat < 0 || service.is_null() {
            crate::journaliser(&format!(
                "XAML : IVisualTreeService3 NON exposé (hr={resultat:#x}) — l'arbre reste hors d'atteinte"
            ));
            return;
        }
        // **CASE 3 DE LA VTABLE** : `AdviseVisualTreeChange` est la PREMIÈRE
        // méthode de `IVisualTreeService`, donc toujours la case 3 après les trois
        // d'`IUnknown` — dans les trois générations de l'interface. On l'appelle
        // par rang, ce qui évite de transcrire vingt signatures.
        let vtable = *(service as *const *const usize);
        let advise: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32 =
            std::mem::transmute(*vtable.add(3));
        let resultat = advise(service, &RECEPTION as *const Reception as *mut c_void);
        crate::journaliser(&format!(
            "XAML : AdviseVisualTreeChange rendu {resultat:#x} — l'arbre nous parle-t-il, on le saura au compteur"
        ));
    }
    armer_evenement();
    // **LE RENDU SE FAIT AILLEURS, ET PLUS TARD** : l'arbre se peuple au fil des
    // créations d'éléments (les icônes de la zone arrivent quand le shell les
    // construit, et le chevron ouvert en ajoute). On repasse trois fois — tôt,
    // puis tard — plutôt qu'une seule, pour ne pas conclure d'un arbre encore vide.
    std::thread::spawn(|| {
        for delai in [5u64, 15, 30] {
            std::thread::sleep(std::time::Duration::from_secs(delai));
            traiter_les_handles();
        }
    });
}

/// Le nom de l'événement de poignée de main, celui que TranslucentTB crée avant
/// de demander la connexion et arme une fois l'abonnement fait.
fn nom_evenement() -> windows::core::PCWSTR {
    windows::core::w!("NeuraDeskXamlTapReady")
}

/// **L'ÉVÉNEMENT NOMMÉ, créé AVANT la connexion.** TranslucentTB en crée un et
/// l'arme après l'abonnement : c'est la poignée de main avec l'infrastructure de
/// diagnostic, et elle manquait à ma première version.
fn creer_evenement() {
    use windows::Win32::System::Threading::CreateEventW;
    unsafe {
        match CreateEventW(None, true, false, nom_evenement()) {
            Ok(evenement) => {
                // Volontairement gardé ouvert pour toute la vie du processus.
                std::mem::forget(evenement);
                crate::journaliser("XAML : événement de poignée de main créé");
            }
            Err(erreur) => crate::journaliser(&format!("XAML : événement non créé : {erreur:?}")),
        }
    }
}

fn armer_evenement() {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};
    unsafe {
        if let Ok(evenement) = OpenEventW(EVENT_MODIFY_STATE, false, nom_evenement()) {
            let _ = SetEvent(evenement);
            let _ = CloseHandle(evenement);
            crate::journaliser("XAML : événement armé — la poignée de main est complète");
        }
    }
}

/// **ON DEMANDE AU SHELL DE NOUS PARLER.** Appelée une fois, depuis un fil à nous.
pub fn demarrer() {
    std::thread::spawn(|| unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
        let xaml = match GetModuleHandleW(w!("Windows.UI.Xaml.dll")) {
            Ok(module) if !module.0.is_null() => module,
            _ => {
                crate::journaliser("XAML : Windows.UI.Xaml.dll absent — rien à faire");
                return;
            }
        };
        let Some(adresse) = GetProcAddress(xaml, s!("InitializeXamlDiagnosticsEx")) else {
            crate::journaliser("XAML : InitializeXamlDiagnosticsEx introuvable");
            return;
        };
        let init: unsafe extern "system" fn(
            *const u16,
            u32,
            *const u16,
            *const u16,
            *const GUID,
            *const u16,
        ) -> i32 = std::mem::transmute(adresse);

        let notre_dll = match crate::chemin_de_la_dll() {
            Some(chemin) => chemin,
            None => {
                crate::journaliser("XAML : chemin de la DLL introuvable");
                return;
            }
        };
        let notre_dll: Vec<u16> =
            notre_dll.encode_utf16().chain(std::iter::once(0)).collect();

        // LE NOM DE CONNEXION N'EST PAS FIXE : on essaie des noms incrémentaux
        // jusqu'à ce que ça passe (même méthode que TranslucentTB). Un échec muet
        // ici coûterait une session entière — donc chaque essai est journalisé.
        creer_evenement();
        let pid = std::process::id();
        for essai in 1..=40u32 {
            let nom: Vec<u16> = format!("VisualDiagConnection{essai}")
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let chemin_essai = notre_dll.clone();
            // **UN FIL NEUF PAR TENTATIVE, ET JOINT AUSSITÔT.** TranslucentTB le
            // dit dans un commentaire : « XAML Diagnostics ne peut être initialisé
            // qu'une fois par fil ; les appels suivants rendent S_OK sans rien
            // faire ». Une boucle sur un même fil ferait donc semblant de réussir
            // à partir du deuxième essai — un faux succès, pire qu'un échec.
            let resultat = std::thread::spawn(move || {
                init(
                    nom.as_ptr(),
                    pid,
                    std::ptr::null(),
                    chemin_essai.as_ptr(),
                    &CLSID_TAP,
                    std::ptr::null(),
                )
            })
            .join()
            .unwrap_or(-1);
            if resultat >= 0 {
                crate::journaliser(&format!(
                    "XAML : InitializeXamlDiagnosticsEx OK à l'essai {essai} (hr={resultat:#x})"
                ));
                return;
            }
            if essai == 1 || essai % 10 == 0 {
                crate::journaliser(&format!("XAML : essai {essai} → hr={resultat:#x}"));
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        crate::journaliser("XAML : 40 essais de connexion, tous refusés");
    });
}

/// **LES RANGS DANS LES VTABLES — recopiés de `xamlom.idl`, jamais devinés.**
///
/// `IUnknown` occupe les cases 0 à 2, puis chaque interface AJOUTE les siennes,
/// les interfaces héritées gardant leur préfixe :
///
/// - `IVisualTreeService` : 12 méthodes → cases 3 à 14.
/// - `IVisualTreeService2` : hérite, puis `GetPropertyIndex` (15), `GetProperty`
///   (16), `ReplaceResource` (17), **`RenderTargetBitmap` (18)**.
/// - `IXamlDiagnostics` : `GetDispatcher` (3), `GetUiLayer` (4),
///   `GetApplication` (5), **`GetIInspectableFromHandle` (6)**, …
/// - `IBitmapData` : **`CopyBytesTo` (3)**, `GetStride` (4),
///   `GetBitmapDescription` (5), `GetSourceBitmapDescription` (6).
const RANG_RENDER_TARGET_BITMAP: usize = 18;
const RANG_INSPECTABLE_DEPUIS_HANDLE: usize = 6;
const RANG_COPY_BYTES_TO: usize = 3;
const RANG_DESCRIPTION_BITMAP: usize = 5;

/// IID de `IVisualTreeService2` (`xamlom.idl`).
const IID_VISUAL_TREE_SERVICE2: GUID = GUID::from_u128(0x130f5136_ec43_4f61_89c7_9801a36d2e95);

/// **LES HANDLES DES ÉLÉMENTS DE LA ZONE DE NOTIFICATION**, relevés par le
/// récepteur et traités AILLEURS.
///
/// Le récepteur tourne sur le **fil d'interface du shell** : il ne doit ni
/// allouer, ni verrouiller, ni journaliser (c'est ce qui a fait tomber explorer
/// une fois). Il se contente donc d'écrire un `u64` dans un tableau FIXE, et le
/// fil de veille vient les chercher.
static HANDLES: [std::sync::atomic::AtomicU64; 64] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 64];
static HANDLES_ECRITS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Le type d'un élément commence-t-il par `SystemTray.` ?
///
/// **SANS ALLOUER** : on lit les `u16` du `BSTR` directement et on les compare.
/// Construire une `String` ici — sur le fil du shell — est exactement ce qu'il
/// faut éviter.
unsafe fn type_est_systeme(pointeur: *const u16) -> bool {
    const PREFIXE: &[u16] = &[83, 121, 115, 116, 101, 109, 84, 114, 97, 121]; // "SystemTray"
    if pointeur.is_null() {
        return false;
    }
    for (index, attendu) in PREFIXE.iter().enumerate() {
        if *pointeur.add(index) != *attendu {
            return false;
        }
    }
    true
}

/// Combien d'événements d'arbre le shell nous a envoyés. Lu par le fil de veille
/// — c'est LUI qui journalise, jamais le récepteur, qui tourne sur le fil du shell.
pub fn evenements_recus() -> u32 {
    RECEPTION.vus.load(Ordering::Relaxed)
}

const S_OK: i32 = 0;
const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
const E_POINTER: i32 = 0x8000_4003u32 as i32;
const IID_UNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
const IID_CALLBACK: GUID = GUID::from_u128(0xaa7a8931_80e4_4fec_8f3b_553f87b4966e);
