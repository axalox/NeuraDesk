//! **LA VUE D'UNE PAGE SCANNÉE : SON MODÈLE, ET SA FENÊTRE.**
//!
//! ⭐ **CE MODULE PORTE LE MODÈLE, PAS SEULEMENT SA FENÊTRE.** Une page scannée se
//! regarde à DEUX endroits — dans le panneau Scanner (`cef_scanner`, qui bascule
//! depuis la liste) et dans la fenêtre d'ici — et les deux montrent **la même
//! page** : `PAGE`, les deux `TEXTES` d'OCR, la navigation et le message de la
//! ligne d'état vivent donc ici, une seule fois.
//!
//! ⛔ **C'EST LA RÉPONSE À UN VRAI DÉFAUT** : le panneau Scanner se referme dès
//! qu'il perd le focus (`hide_if_focus_elsewhere`), donc ouvrir la fenêtre faisait
//! DISPARAÎTRE le panneau — et il fallait rouvrir l'applet pour rescanner. La vue
//! en grand vit maintenant dans le panneau aussi, et la fenêtre n'est qu'un
//! détachement qu'on demande.
//!
//! Voir `assets/popups/page_scan.html` (la fenêtre) et le mode « page » de
//! `assets/popups/scanner.html`, qui partagent `page_vue.js`.
//!
//! Une clic sur une vignette du panneau Scanner ouvre CETTE fenêtre : la page en
//! grand, redimensionnable, avec ses voisines accessibles.
//!
//! ⭐ **C'EST UNE VRAIE FENÊTRE**, pas un panneau : `fenetre: true` lui donne
//! `WS_EX_APPWINDOW` (barre des tâches et Alt-Tab), ni topmost ni toolwindow. On
//! la garde à côté du dock pendant qu'on travaille — un panneau, lui, se referme
//! dès qu'on clique ailleurs.
//!
//! ⛔ **ET ELLE N'A NI BARRE DE TITRE NI BORDURE.** `create_window` pose
//! **`WS_POPUP`** pour une `fenetre` (le champ `PopupSpec::fenetre` le dit, et
//! avoue avoir affirmé le contraire par le passé) : le système ne fournit donc
//! **plus** le déplacement ni le redimensionnement. Les deux sont réimplémentés
//! **ici** — `WM_NCHITTEST` pour les huit bords, `HTCAPTION` pour le déplacement
//! — exactement comme `cef_couleurs`, `cef_caracteres`, `cef_kaomoji` et
//! `cef_visionneuse`. Le dépôt assume cette duplication.
//!
//! Actions reçues (`"page-scan/<verbe>/…"`, segments ENCODÉS) :
//! - `ready` — la page a chargé
//! - `close` — ferme (cache) la fenêtre
//! - `deplacer` — déplacement natif, comme une barre de titre
//! - `precedente` / `suivante` — la page d'avant / d'après dans la session
//! - `reconnaitre/<moteur>` — lance une reconnaissance (`tesseract` ou `windows`)
//! - `comparer` — lance **les deux** d'un coup, pour les mettre côte à côte
//! - `tourner/<horaire|antihoraire>` — tourne la PAGE d'un quart de tour
//! - `copier/<moteur>` — met le texte reconnu dans le presse-papiers
//! - `enregistrer/<moteur>` — écrit le texte dans un `.txt`
//! - `ameliorer-auto` — étire les niveaux de luminance de la PAGE (voir
//!   `win32::amelioration`)
//! - `ameliorer-regler/<lum>/<con>/<sat>` — les trois curseurs, en −100…+100
//! - `ameliorer-zero` — revient à « inchangé »
//! - `ameliorer-enregistrer` — écrit une **COPIE** améliorée à côté de la page
//!
//! ⭐ **L'AMÉLIORATION EST UN RÉGLAGE POUSSÉ AUX DEUX HÔTES, PAS UN EFFET LOCAL.**
//! L'aperçu est le filtre CSS que RUST émet dans l'état, et le fichier écrit est le
//! même calcul en Rust : les deux hôtes ne peuvent donc pas montrer deux images
//! différentes de la même page, et l'aperçu ressemble à ce qui est écrit.
//!
//! ⚠️ **LA NAVIGATION LIT LA SESSION, ELLE NE LA COPIE PAS** : c'est
//! `cef_scanner` qui possède les pages. Une copie vieillirait — on retirerait une
//! page dans le panneau et la fenêtre continuerait de proposer l'effacée.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetSystemMetrics, GetWindowRect, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCLIENT,
    HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, SM_CXSCREEN, SM_CYSCREEN, WM_CLOSE, WM_NCHITTEST,
};

use crate::gfx::device::GfxDevice;
use crate::win32::amelioration::{self, Reglage};
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::ocr;

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// La clé du dernier `window.setState(...)` émis — vidée au `ready`, même
    /// raison que `cef_scanner::PAYLOAD`.
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// **LA PAGE MONTRÉE.** `None` = la fenêtre n'a jamais rien reçu.
    static PAGE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    /// **LA PAGE DONT L'IMAGE A DÉJÀ ÉTÉ POUSSÉE.**
    ///
    /// ⛔ **L'IMAGE NE PASSE PAS PAR L'ÉTAT, ET CE N'EST PAS UN DÉTAIL DE GOÛT** :
    /// `push_state` sérialise l'état **à chaque frame** pour calculer sa clé de
    /// cache — y mettre une `data:` URI de plusieurs mégaoctets ferait encoder et
    /// comparer ces mégaoctets soixante fois par seconde. L'image part donc par un
    /// `window.setImage(...)` séparé, UNE fois par page.
    ///
    /// ⚠️ **ET CE TÉMOIN SE ROUVRE AU `ready`** : `execute_js` ne signale jamais
    /// son échec, donc une image poussée avant que la page n'écoute serait perdue
    /// sans un mot.
    static ENVOYEE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    /// **LES DEUX RECONNAISSANCES, PAR MOTEUR** — elles peuvent tourner
    /// ENSEMBLE, et c'est justement ce qui rend la comparaison possible.
    static RECONNAISSANCES: RefCell<ParMoteur<ocr::Reconnaissance>> = RefCell::new([None, None]);
    /// **LE DERNIER ÉTAT RENDU, PAR MOTEUR** — ce que la fenêtre affiche et copie.
    static TEXTES: RefCell<ParMoteur<ocr::Etat>> = RefCell::new([None, None]);
    /// La phrase de la ligne d'état de la fenêtre (copie impossible, rien à
    /// enregistrer…). ⚠️ **Séparée des états d'OCR** : elle parle de l'ACTION de
    /// l'utilisateur, pas d'un moteur, et l'écraser dans un état de moteur ferait
    /// disparaître le texte reconnu.
    static MESSAGE: RefCell<Option<String>> = const { RefCell::new(None) };
    /// **LE RÉGLAGE D'AMÉLIORATION, AVEC LA PAGE QUI L'A REÇU.**
    ///
    /// ⛔ **UN SEUL SLOT, PORTEUR DE LA PAGE, ET C'EST LA DESIGN.** Un réglage
    /// attaché à la VIEWER (ou à un rang) suivrait la navigation : la page 3
    /// arriverait éclaircie par les curseurs de la page 2, sans un mot — et l'OCR
    /// lancé dessus lirait une image que personne n'a choisie. La page du slot est
    /// donc COMPAREE à la page courante par `reglage_de`, qui rend `neutre()` dès
    /// que ça ne correspond plus.
    ///
    /// ⚠️ **CE N'EST PAS UN CACHE DE L'IMAGE** : le slot ne porte que trois
    /// entiers. L'image, elle, reste sur le disque — l'aperçu est un filtre CSS, et
    /// le fichier amélioré n'est écrit que quand on le demande.
    static REGLAGE: RefCell<Option<(PathBuf, Reglage)>> = const { RefCell::new(None) };
}

/// Le travail d'un moteur, rangé par moteur — l'index vient de `indice`.
type ParMoteur<T> = [Option<T>; 2];

/// ⚠️ **L'INDEX EST ÉCRIT UNE SEULE FOIS, ICI.** `TOUS` et cet ordre doivent
/// rester d'accord : deux tables parallèles qui divergeraient afficheraient le
/// texte de Tesseract sous le nom de Windows — un défaut qui ne se voit qu'en
/// comparant, c'est-à-dire trop tard.
fn indice(moteur: ocr::Moteur) -> usize {
    match moteur {
        ocr::Moteur::Tesseract => 0,
        ocr::Moteur::Windows => 1,
    }
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefPageScanWindow"),
    window_title: w!("NeuraDesk — Page scannée"),
    html_file: "page_scan.html",
    wndproc: Some(wndproc),
    // Sans objet pour une fenêtre, dont le contrat de pile est celui de Windows.
    above_a_magnified_dock: false,
    // ⛔ `false` : pour une `fenetre`, `create_window` remplace `ex_style`, donc
    // `WS_EX_LAYERED` disparaît — et un fondu qui se croit armé serait un mensonge.
    fades: false,
    fenetre: true,
};

/// Taille d'ouverture, **EN UNITÉS DE PAGE, PAS EN PIXELS**.
///
/// ⛔ **C'est le piège qui a coûté une manche à `cef_couleurs` et à
/// `cef_caracteres`** : des pixels passés tels quels à `SetWindowPos` alors que la
/// page est rendue à `CONTENT_ZOOM_FACTOR` donnent une fenêtre trois fois trop
/// petite. On passe donc par `cef_couleurs::taille_pour_ecran`, qui est **pure**,
/// **testée**, et qu'on réutilise au lieu de la recopier.
const LARGEUR_UNITES: i32 = 900;
const HAUTEUR_UNITES: i32 = 950;

/// Ouvre la fenêtre sur `page`, ou la **ramène devant** si elle existe déjà en
/// montrant une autre. Rend `true` si elle vient d'être CRÉÉE (voir
/// `cef_popup::montrer_fenetre`, dont la valeur de retour a ce sens-là).
///
/// ⚠️ **`montrer_fenetre`, JAMAIS `toggle`** : une fenêtre ne se ferme pas parce
/// qu'on reclique ce qui l'a ouverte — elle revient devant. C'est la différence
/// avec un panneau, qui bascule.
pub fn montrer(gfx: &GfxDevice, content_scale: f32, page: PathBuf) -> bool {
    CONTENT_SCALE.with(|c| c.set(content_scale));
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    voir(page);
    let creee = cef_popup::montrer_fenetre(&SLOT, &SPEC, rect_defaut(), gfx);
    // ⚠️ **APRÈS la création** : `execute_js` sur un navigateur qui n'existe pas
    // encore ne fait rien, et l'image serait perdue sans un mot.
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    ENVOYEE.with(|e| *e.borrow_mut() = None);
    push_state();
    pousser_l_image();
    creee
}

/// **LA PAGE QUE LE HARNAIS MONTRE** (`NEURADESK_DEBUG_OPEN=page-scan`).
///
/// ⚠️ La première de la session s'il y en a une ; **sinon un chemin vide**, et la
/// fenêtre s'ouvre alors sur son message « cette page n'a pas pu être lue ».
/// C'est voulu : une capture doit pouvoir établir que la fenêtre est **PEINTE**
/// même sans session ouverte — et sans ce bras, le harnais tomberait sur son
/// `eprintln!` d'action inconnue, qui ne va nulle part (l'application est lancée
/// détachée, sans console).
pub fn page_de_debug() -> PathBuf {
    // `NEURADESK_DEBUG_PAGE=<chemin>` — même famille que `NEURADESK_DEBUG_FICHIER`
    // et `NEURADESK_DEBUG_EDIT` : le harnais a besoin de MONTRER un fichier
    // précis, sans qu'une session existe. C'est ce qui permet de capturer la
    // fenêtre avec une vraie page, et donc de vérifier autre chose que son cadre.
    if let Some(chemin) = std::env::var_os("NEURADESK_DEBUG_PAGE") {
        return PathBuf::from(chemin);
    }
    crate::win32::cef_scanner::pages_de_la_session().into_iter().next().unwrap_or_default()
}

/// Cache la fenêtre sans la détruire — la page survit.
pub fn cacher() {
    if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
        cef_popup::cacher_fenetre(hwnd);
    }
}

/// **LA PAGE COURANTE, S'IL Y EN A UNE** — lue par les DEUX hôtes : le panneau
/// Scanner s'en sert pour savoir s'il bascule en mode « page ».
pub(crate) fn page_courante() -> Option<PathBuf> {
    PAGE.with(|p| p.borrow().clone())
}

/// **CE QUE LES DEUX HÔTES MONTRENT.** Poser une page différente suffit à faire
/// repousser son image à la frame suivante, chez l'un comme chez l'autre.
pub(crate) fn voir(page: PathBuf) {
    let change = PAGE.with(|p| {
        let mut courant = p.borrow_mut();
        let change = courant.as_deref() != Some(page.as_path());
        *courant = Some(page);
        change
    });
    if change {
        // L'image de la nouvelle page n'a pas encore été poussée.
        ENVOYEE.with(|e| *e.borrow_mut() = None);
        PAYLOAD.with(|c| *c.borrow_mut() = None);
    }
    // ⚠️ **ON PRÉVIENT LE PANNEAU, MÊME SI RIEN N'A CHANGÉ POUR LA FENÊTRE** :
    // c'est lui qui vient de demander la page, et sa clé de cache est à lui.
    crate::win32::cef_scanner::invalider_payload();
}

/// Le nom du fichier d'une page — ce que la fenêtre affiche dans son titre.
pub(crate) fn nom_de_fichier(page: &Path) -> String {
    page.file_name().map(|nom| nom.to_string_lossy().into_owned()).unwrap_or_default()
}

/// **LA PLACE DE LA PAGE DANS LA SESSION** — `(rang 1-based, total)`.
/// `(0, 0)` si elle n'y est plus : la fenêtre peut survivre à un `Abandonner`.
fn position(page: &Path) -> (usize, usize) {
    let pages = crate::win32::cef_scanner::pages_de_la_session();
    let total = pages.len();
    let rang = pages.iter().position(|connue| connue == page).map_or(0, |i| i + 1);
    (rang, total)
}

/// Une fenêtre centrée sur l'écran PRINCIPAL. **Pas `dash_rect`** : une fenêtre
/// n'occupe pas la zone libre du dock, elle a sa propre place.
fn rect_defaut() -> [i32; 4] {
    let (largeur, hauteur) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let (l, h) = crate::win32::cef_couleurs::taille_pour_ecran(largeur, hauteur, (LARGEUR_UNITES, HAUTEUR_UNITES));
    [((largeur - l) / 2).max(0), ((hauteur - h) / 2).max(0), l, h]
}

/// **CE QUE CHAQUE MOTEUR A RENDU** — la fenêtre dessine les deux colonnes à
/// partir de ça, et c'est aussi ce que « Copier » et « Enregistrer » relisent.
fn etat_ocr() -> Vec<serde_json::Value> {
    let textes = TEXTES.with(|t| t.borrow().clone());
    ocr::Moteur::TOUS
        .iter()
        .map(|(moteur, _nom, libelle)| {
            let etat = textes[indice(*moteur)].clone();
            json!({
                "nom": moteur.nom(),
                "libelle": libelle,
                "etat": etat.as_ref().map(ocr::Etat::nom),
                "texte": etat.as_ref().and_then(ocr::Etat::texte),
                "message": etat.as_ref().and_then(ocr::Etat::message),
            })
        })
        .collect()
}

/// **CE QUE LES DEUX HÔTES AFFICHENT DE LA PAGE** — le nom, la place dans la
/// session, les deux moteurs d'OCR et la phrase d'état.
///
/// ⭐ **UNE SEULE SOURCE POUR DEUX AFFICHAGES** : c'est ce qui garantit que le
/// panneau et la fenêtre ne montrent pas deux choses différentes de la même page.
pub(crate) fn etat_page() -> serde_json::Value {
    let page = PAGE.with(|p| p.borrow().clone());
    let (rang, total) = page.as_deref().map_or((0, 0), position);
    let reglage = REGLAGE.with(|r| reglage_de(&r.borrow(), page.as_deref()));
    json!({
        "nom": page.as_deref().map(nom_de_fichier).unwrap_or_default(),
        "rang": rang,
        "total": total,
        "ocr": etat_ocr(),
        "message": MESSAGE.with(|m| m.borrow().clone()),
        "amelioration": reglage_json(&reglage),
    })
}

fn etat_json() -> serde_json::Value {
    // L'échelle est propre à la FENÊTRE : le panneau a la sienne, et il ajoute la
    // sienne de son côté.
    let mut etat = etat_page();
    if let Some(objet) = etat.as_object_mut() {
        objet.insert("contentScale".to_string(), json!(CONTENT_SCALE.with(Cell::get)));
    }
    etat
}

/// **LE RÉGLAGE D'UNE PAGE, LU DANS L'UNIQUE SLOT** — pur, donc vérifiable sans
/// fenêtre ni thread.
///
/// ⛔ **LA COMPARAISON EST LE GARDE.** Sans elle, le slot serait simplement « le
/// dernier réglage posé » et la navigation emporterait les curseurs d'une page à
/// l'autre. Ce que `cef_page_scan::tests::un_reglage_n_appartient_qu_a_la_page_qui_l_a_recu`
/// rend impossible.
fn reglage_de(slot: &Option<(PathBuf, Reglage)>, page: Option<&Path>) -> Reglage {
    slot.as_ref()
        .and_then(|(pour, reglage)| (Some(pour.as_path()) == page).then_some(*reglage))
        .unwrap_or_else(Reglage::neutre)
}

/// Le réglage, dans l'état poussé aux DEUX hôtes.
///
/// ⭐ **RUST ÉMET LE FILTRE CSS, LA PAGE LE POSE TEL QUEL.** Écrire la formule
/// dans la page en ferait deux : l'aperçu d'un côté, le fichier écrit de l'autre,
/// et rien ne les forcrait à se ressembler. `amelioration` porte les formules une
/// fois, et c'est ce qui fait que ce qu'on regarde est ce qui sera écrit.
fn reglage_json(reglage: &Reglage) -> serde_json::Value {
    json!({
        "luminosite": reglage.luminosite,
        "contraste": reglage.contraste,
        "saturation": reglage.saturation,
        "filtre": reglage.filtre_css(),
    })
}

/// **LES NIVEAUX AUTOMATIQUES SUR LA PAGE COURANTE.**
///
/// ⚠️ **UNE DÉCODAGE D'IMAGE, UNE FOIS PAR CLIC** — pas à la frame. `etat_page` ne
/// relit que le réglage déjà calculé ; recalculer l'histogramme soixante fois par
/// seconde sur une A4 à 300 dpi gelerait le dock, exactement le défaut des
/// mégaoctets sérialisés à chaque frame (voir `ENVOYEE`).
pub(crate) fn ameliorer_auto() {
    let Some(page) = PAGE.with(|p| p.borrow().clone()) else {
        return dire("Aucune page n'est ouverte.".to_string());
    };
    let image = match image::open(&page) {
        Ok(image) => image.to_rgba8(),
        Err(erreur) => return dire(format!("{} n'a pas pu être relue : {erreur}", page.display())),
    };
    let reglage = amelioration::auto_niveaux(&amelioration::histogramme(&image));
    if reglage.est_neutre() {
        // ⭐ **RIEN À ÉTIRER SE DIT, ET LE RÉGLAGE MANUEL RESTE EN PLACE.** Remettre
        // les curseurs à zéro parce que l'automatique n'a rien trouvé jetterait le
        // travail de la personne pour un résultat qui dit « rien changé ».
        return dire(
            "Les niveaux automatiques n'ont rien trouvé à étirer : la page est déjà plate, ou déjà étendue."
                .to_string(),
        );
    }
    REGLAGE.with(|r| *r.borrow_mut() = Some((page, reglage)));
    invalider_les_deux();
    crate::diag!("[page-scan] niveaux auto L{} C{} S{}", reglage.luminosite, reglage.contraste, reglage.saturation);
    dire(format!(
        "Niveaux automatiques : luminosité {:+}, contraste {:+}, saturation {:+}.",
        reglage.luminosite, reglage.contraste, reglage.saturation
    ));
}

/// **UN CURSEUR MOUVé, REMIS AUX TROIS D'UN COUP** — la page envoie toujours le
/// jeu complet, parce qu'un réglage moitié (deux valeurs sur trois) serait deux
/// demi-états qui ne se correspondent plus.
///
/// ⚠️ **UN RÉGLAGE ILLISIBLE EST REFUSÉ EN LE DISANT** (règle du dépôt, la même
/// que dans `cef_scanner`) : il vient d'une page, donc d'un code qui a pu diverger,
/// pas d'un geste.
pub(crate) fn ameliorer_regler(luminosite: &str, contraste: &str, saturation: &str) {
    let Some(page) = PAGE.with(|p| p.borrow().clone()) else {
        return dire("Aucune page n'est ouverte.".to_string());
    };
    let Ok(l) = luminosite.parse::<i32>() else {
        return dire(format!("Luminosité illisible : {luminosite:?}"));
    };
    let Ok(c) = contraste.parse::<i32>() else {
        return dire(format!("Contraste illisible : {contraste:?}"));
    };
    let Ok(s) = saturation.parse::<i32>() else {
        return dire(format!("Saturation illisible : {saturation:?}"));
    };
    let reglage = Reglage { luminosite: l, contraste: c, saturation: s }.borne();
    if reglage.est_neutre() {
        REGLAGE.with(|r| *r.borrow_mut() = None);
    } else {
        REGLAGE.with(|r| *r.borrow_mut() = Some((page, reglage)));
    }
    // ⛔ **PAS DE MESSAGE POUR UN CURSEUR** : l'aperçu du filtre EST la réponse, et
    // remplir la ligne d'état à chaque crantement d'ascenseur écraserait les
    // messages d'OCR (« rien trouvé », « le presse-papiers est tenu… »).
    invalider_les_deux();
}

/// **REVENIR À « INCHANGÉ ».** Cela se dit à l'écran, contrairement au curseur :
/// le retour visuel d'une page déjà claire est faible, et on doit savoir que le
/// clic a porté.
pub(crate) fn ameliorer_zero() {
    REGLAGE.with(|r| *r.borrow_mut() = None);
    invalider_les_deux();
    dire("Réglages remis à zéro : la page est montrée telle qu'elle est numérisée.".to_string());
}

/// **ÉCRIRE LA COPIE AMÉLIORÉE, À CÔTÉ DE LA PAGE.**
///
/// ⛔ **LA NUMÉRISATION N'EST JAMAIS ÉCRASÉE** (voir `amelioration::ecrire_amelioration`,
/// qui porte la leçon du 2026-09-25 : le codec JPEG tronque la destination avant
/// d'échouer). Améliorer n'est pas corriger la numérisation — c'est en produire une
/// autre image, que la personne garde ou jette.
pub(crate) fn ameliorer_enregistrer() {
    let Some(page) = PAGE.with(|p| p.borrow().clone()) else {
        return dire("Aucune page n'est ouverte.".to_string());
    };
    let reglage = REGLAGE.with(|r| reglage_de(&r.borrow(), Some(page.as_path())));
    if reglage.est_neutre() {
        // ⭐ **ÉCRIRE SANS RÉGLAGE NE SERAIT QU'UN DOUBLE** : un ré-encodage JPEG de
        // plus, un peu dégradé, pour aucun gain. On le refuse, et on dit quoi faire.
        return dire(
            "Aucun réglage à écrire : bougez un curseur (ou « Auto ») avant d'enregistrer la copie.".to_string(),
        );
    }
    let cible = amelioration::nom_ameliore(&page);
    match amelioration::ecrire_amelioration(&page, &reglage, &cible) {
        Ok(ecrit) => {
            crate::diag!("[page-scan] copie amelioree ecrite : {}", ecrit.display());
            dire(format!("Copie améliorée écrite à côté de la page : {}.", nom_de_fichier(&ecrit)));
        }
        Err(message) => dire(message),
    }
}

/// **LIT LE LEVIER ET LE POSE** — appelé par les DEUX hôtes de la vue agrandie.
///
/// ⚠️ **LES DEUX, PARCE QUE LA FENÊTRE EST L'HÔTE QUI SE PHOTOGRAPHIE LE PLUS SÛR** :
/// le panneau Scanner se referme dès que le focus réel le quitte
/// (`hide_if_focus_elsewhere`), et une capture de panneau peut donc montrer le
/// bandeau d'un run et pas du suivant. Poser le réglage depuis un seul bras
/// laisserait l'autre hote vérifiable à moitié.
pub fn preset_depuis_levier(page: &Path) {
    // ⚠️ **UNE PAGE D'ABORD** : `page_de_debug()` rend un chemin VIDE quand ni le
    // levier `NEURADESK_DEBUG_PAGE` ni une session ne fournissent de page. Poser un
    // réglage sur cette page-là occuperait l'unique emplacement avec une page qui
    // n'existe pas, et le vrai réglage de la page affichée serait refusé ensuite.
    if page.as_os_str().is_empty() {
        return;
    }
    if let Some(valeurs) = std::env::var_os("NEURADESK_DEBUG_AMELIORATION") {
        preset_reglage_de_debug(page, &valeurs.to_string_lossy());
    }
}

/// **PRÉSETTER LE RÉGLAGE POUR LE HARNAIS** — `NEURADESK_DEBUG_AMELIORATION=<lum>,<con>,<sat>`.
///
/// ⚠️ **SANS LUI, LA BARRE SE PHOTOGRAPHIE À ZÉRO** : les trois ascenseurs au
/// milieu, `filtre` valant `none`, et la page intacte. On verrait les commandes,
/// JAMAIS le filtre qu'elles produisent — donc la moitié importante de la
/// fonctionnalité (état → Rust → filtre sur l'image) ne serait pas vérifiable. Le
/// harnais n'a pas de clic fiable (voir la mémoire du clic aveugle), et un levier
/// d'environnement est ce que ce dépôt utilise pour ça, comme
/// `NEURADESK_DEBUG_COULEURS_HISTORIQUE`.
///
/// ⚠️ **IL NE TOUCHE PAS LA PAGE** : il pose trois entiers en mémoire. Le fichier
/// numérisé reste ce qu'il est, y compris pendant une capture.
pub fn preset_reglage_de_debug(page: &Path, valeurs: &str) -> bool {
    let morceaux: Vec<&str> = valeurs.split(',').map(str::trim).collect();
    let [l, c, s] = morceaux.as_slice() else {
        crate::diag!("[page-scan] NEURADESK_DEBUG_AMELIORATION : {valeurs:?} n'est pas trois valeurs séparées par des virgules");
        return false;
    };
    let (Ok(l), Ok(c), Ok(s)) = (l.parse::<i32>(), c.parse::<i32>(), s.parse::<i32>()) else {
        crate::diag!("[page-scan] NEURADESK_DEBUG_AMELIORATION : {valeurs:?} n'est pas trois entiers");
        return false;
    };
    REGLAGE.with(|r| *r.borrow_mut() = Some((page.to_path_buf(), Reglage { luminosite: l, contraste: c, saturation: s }.borne())));
    invalider_les_deux();
    crate::diag!("[page-scan] reglage de debug L{l} C{c} S{s}");
    true
}

/// **DIRE QUELQUE CHOSE À L'ÉCRAN.** Un bouton qui refuse en silence est le défaut
/// que ce dépôt refuse : tout refus passe par la ligne d'état.
fn dire(message: String) {
    crate::diag!("[page-scan] {message}");
    MESSAGE.with(|m| *m.borrow_mut() = Some(message));
    invalider_les_deux();
}

/// **PRÉVENIR LES DEUX HÔTES QU'IL Y A DU NEUF.** Chacun a SA clé de cache, donc
/// n'invalider que la sienne laisserait l'autre afficher un état périmé — le
/// défaut « il faut rouvrir le panneau pour voir le résultat ».
fn invalider_les_deux() {
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    crate::win32::cef_scanner::invalider_payload();
}

fn push_state() {
    let payload = etat_json();
    let cle = payload.to_string();
    if PAYLOAD.with(|c| c.borrow().as_deref() == Some(cle.as_str())) {
        return;
    }
    PAYLOAD.with(|c| *c.borrow_mut() = Some(cle));
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
}

/// Le type MIME d'une page.
///
/// ⚠️ **PAS CÂBLÉ EN DUR, MÊME SI UNE PAGE EST TOUJOURS UN JPEG AUJOURD'HUI**
/// (`scanner::EXTENSION_PAGE`) : le jour où le format des pages changera, le type
/// doit suivre sans que personne y pense — et un `image/jpeg` sur un PNG donne une
/// image cassée, pas une erreur.
fn mime_pour(page: &Path) -> &'static str {
    match page.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("bmp") => "image/bmp",
        _ => "image/jpeg",
    }
}

/// **POUSSE L'IMAGE DE LA PAGE COURANTE**, si elle n'est pas déjà partie.
///
/// ⚠️ **LA LECTURE PEUT ÉCHOUER, ET ÇA SE DIT** : la page a pu être retirée ou
/// effacée depuis le panneau. On envoie alors `null`, et la fenêtre affiche
/// pourquoi — au lieu de garder l'image précédente, qui serait un mensonge.
fn pousser_l_image() {
    let Some(page) = PAGE.with(|p| p.borrow().clone()) else { return };
    if ENVOYEE.with(|e| e.borrow().as_deref() == Some(page.as_path())) {
        return;
    }
    // ⚠️ **LE TÉMOIN EST POSÉ MÊME SI LA LECTURE ÉCHOUE** : sans ça, une page
    // disparue serait relue — et son échec redit — soixante fois par seconde.
    ENVOYEE.with(|e| *e.borrow_mut() = Some(page.clone()));
    let uri = image_data_uri(&page);
    cef_popup::execute_js(&SLOT, &format!("window.setImage({})", json!(uri)));
}

/// L'image d'une page, en `data:` URI — ou `None` si elle est illisible.
///
/// ⛔ **AUCUN `unwrap`** : le binaire est compilé avec `panic = "abort"`, donc un
/// fichier disparu entre deux frames tuerait le dock ENTIER.
pub(crate) fn image_data_uri(page: &Path) -> Option<String> {
    let octets = match std::fs::read(page) {
        Ok(octets) => octets,
        Err(erreur) => {
            crate::diag!("[page-scan] {} illisible : {erreur}", page.display());
            return None;
        }
    };
    Some(format!("data:{};base64,{}", mime_pour(page), BASE64.encode(octets)))
}

/// **LA PAGE VOISINE** — `delta` vaut -1 ou +1.
///
/// ⚠️ **SANS BOUCLAGE** : depuis la première page, « précédente » ne fait rien
/// plutôt que de sauter à la dernière. Une navigation qui tourne en rond fait
/// perdre le compte de ce qu'on regarde.
fn voisine(delta: i32) -> Option<PathBuf> {
    let page = PAGE.with(|p| p.borrow().clone())?;
    let pages = crate::win32::cef_scanner::pages_de_la_session();
    let rang = pages.iter().position(|connue| *connue == page)? as i32;
    let cible = rang + delta;
    if cible < 0 || cible as usize >= pages.len() {
        return None;
    }
    Some(pages[cible as usize].clone())
}

/// ⚠️ **ON NE POUSSE RIEN ICI.** `voir` change le modèle et invalide les deux
/// clés ; chaque hôte repousse à sa frame (voir `refresh_if_visible` et
/// `cef_scanner::refresh_if_visible`). Pousser depuis ici voudrait dire choisir
/// un hôte — et l'autre afficherait une page périmée.
/// **LE SENS D'UN QUART DE TOUR.** Nommé, et relu depuis la page — jamais un
/// booléen, qui ne dirait pas lequel des deux.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Sens {
    Horaire,
    Antihoraire,
}

impl Sens {
    const TOUS: [(Sens, &'static str); 2] = [(Sens::Horaire, "horaire"), (Sens::Antihoraire, "antihoraire")];

    pub(crate) fn depuis_nom(texte: &str) -> Option<Self> {
        Self::TOUS.iter().find(|(_, nom)| nom.eq_ignore_ascii_case(texte)).map(|(sens, _)| *sens)
    }
}

/// **L'IMAGE TOURNÉE D'UN QUART DE TOUR — PURE.** Séparée de la lecture et de
/// l'écriture pour être **vérifiable** : c'est le seul endroit où l'on peut se
/// tromper de sens, et une rotation à l'envers ne se voit qu'à l'usage.
fn tourner_image(image: image::RgbaImage, sens: Sens) -> image::RgbaImage {
    match sens {
        Sens::Horaire => image::imageops::rotate90(&image),
        Sens::Antihoraire => image::imageops::rotate270(&image),
    }
}

/// **TOURNE LA PAGE D'UN QUART DE TOUR — LE FICHIER, PAS L'AFFICHAGE.**
///
/// ⭐ **ET C'EST LA DÉCISION QUI COMPTE.** On tourne une page parce qu'elle est
/// **de travers** : la correction doit survivre jusqu'au DOCUMENT. Un
/// `transform: rotate()` n'aurait l'air de rien — sauf qu'à « Terminer », le PDF
/// assemblé porterait la page de travers, et l'utilisateur découvrirait que son
/// geste n'avait rien corrigé.
///
/// ⚠️ **LE FICHIER EST RÉÉCRIT, DONC RÉENCODÉ.** Quatre quarts de tour ramènent à
/// l'orientation d'origine avec quatre générations JPEG — c'est le prix, et il est
/// faible pour une numérisation de texte. On ne garde pas l'original : la page
/// temporaire EST la numérisation, et la tourner est une correction.
pub(crate) fn tourner(sens: Sens) {
    let Some(page) = PAGE.with(|p| p.borrow().clone()) else {
        return dire("Aucune page n'est ouverte.".to_string());
    };
    if let Err(message) = tourner_fichier(&page, sens) {
        return dire(message);
    }
    // ⚠️ **LE FICHIER A CHANGÉ SOUS LA VUE** : l'image poussée et la vignette
    // décrivent maintenant une autre image, et RIEN ne les rafraîchirait tout
    // seul — la clé de cache de la fenêtre ne regarde que `PAGE`, et celle du
    // panneau que la SESSION, qui n'a pas bougé.
    ENVOYEE.with(|e| *e.borrow_mut() = None);
    crate::win32::cef_scanner::rafraichir_la_vignette(&page);
    crate::diag!("[page-scan] page tournee ({sens:?})");
    dire(format!(
        "Page tournée ({}).",
        match sens {
            Sens::Horaire => "quart de tour à droite",
            Sens::Antihoraire => "quart de tour à gauche",
        }
    ));
}

/// **LA ROTATION DU FICHIER — la partie qui touche au disque**, séparée de la vue
/// pour être **vérifiable sur un vrai fichier**.
///
/// ⛔ **DEUX DÉFAUTS RÉELS, SIGNALÉS LE 2026-10-09 PAR « Format error decoding
/// Jpeg: I/O errors Not enough bytes, expected 2 but found 0 » :**
///
/// 1. **`RgbaImage::save()` vers un `.jpg` ÉCHOUE** — le codec JPEG n'accepte pas
///    le RGBA (le format n'a pas d'alpha). Il faut convertir en RGB d'abord.
/// 2. ⛔ **ET IL TRONQUE LE FICHIER AVANT D'ÉCHOUER.** Écrire dans la page elle-
///    même coûtait donc **la page** : la rotation suivante ne pouvait plus la
///    relire, et la numérisation était perdue. **Un échec d'écriture ne doit
///    jamais coûter la donnée** — on écrit À CÔTÉ puis on renomme, comme
///    `config::write_atomique`. L'original ne bouge que si la nouvelle version
///    existe.
fn tourner_fichier(page: &Path, sens: Sens) -> Result<(), String> {
    let image = image::open(page)
        .map_err(|erreur| format!("{} n'a pas pu être relue : {erreur}", page.display()))?
        .to_rgba8();
    let tournee = tourner_image(image, sens);
    // ⚠️ **EN RGB, PAS EN RGBA** : c'est ce que le codec JPEG accepte.
    // ⚠️ **EN RGB, PAS EN RGBA** : c'est ce que le codec JPEG accepte. Un
    // `RgbaImage::save` vers un `.jpg` échoue — le format n'a pas d'alpha.
    let rgb = image::DynamicImage::ImageRgba8(tournee).to_rgb8();

    // ⛔ **ON N'ÉCRIT PAS DANS LA PAGE AVANT D'ÊTRE SÛR DE POUVOIR LE FAIRE.**
    // Défaut réel du 2026-10-09 : `save()` sur le chemin de la page la TRONQUE
    // avant d'échouer, et la numérisation est PERDUE — la rotation suivante ne
    // peut même plus la relire (« Not enough bytes, expected 2 but found 0 »).
    // On écrit donc À CÔTÉ puis on renomme, comme `config::write_atomique` :
    // l'original ne bouge que si la nouvelle version existe.
    //
    // ⚠️ **LE TEMPORAIRE DOIT PORTER `.jpg`** : `save` choisit le format par
    // l'extension, et écrire un JPEG sous un autre nom le ferait échouer.
    let temporaire = page.with_extension("tmp.jpg");
    if let Err(erreur) = rgb.save(&temporaire) {
        let _ = std::fs::remove_file(&temporaire);
        return Err(format!("{} n'a pas pu être tournée : {erreur}", page.display()));
    }
    if let Err(erreur) = std::fs::rename(&temporaire, page) {
        let _ = std::fs::remove_file(&temporaire);
        return Err(format!("{} n'a pas pu être remplacée : {erreur}", page.display()));
    }
    Ok(())
}

pub(crate) fn naviguer(delta: i32) {
    match voisine(delta) {
        Some(page) => voir(page),
        // ⚠️ **LE BOUTON EST DÉJÀ GRISÉ PAR LA PAGE** — s'il arrive quand même,
        // c'est que l'affichage a divergé : on le journalise sans le lui reprocher.
        None => crate::diag!("[page-scan] pas de page voisine a {delta:+}"),
    }
}

/// **LANCE UNE RECONNAISSANCE SUR LA PAGE COURANTE.**
///
/// ⚠️ **UN MOTEUR DÉJÀ OCCUPÉ EST REFUSÉ, PAS RELANCÉ** : deux `tesseract.exe`
/// écriraient dans le même `.txt`, et le second résultat serait celui du premier
/// — un défaut qui ne se voit qu'en comparant les deux textes.
pub(crate) fn lancer_ocr(nom: &str) {
    let Some(moteur) = ocr::Moteur::depuis_nom(nom) else {
        return dire(format!("Moteur de reconnaissance inconnu : {nom:?}"));
    };
    let i = indice(moteur);
    if RECONNAISSANCES.with(|r| r.borrow()[i].is_some()) {
        return dire(format!("{} travaille déjà sur cette page.", moteur.libelle()));
    }
    let Some(page) = PAGE.with(|p| p.borrow().clone()) else {
        return dire("Aucune page n'est ouverte.".to_string());
    };
    MESSAGE.with(|m| *m.borrow_mut() = None);
    // ⚠️ **LES DEUX HÔTES, PAS SEULEMENT LE NÔTRE** : effacer un message ne se
    // voit que si celui qui l'affichait repousse son état.
    invalider_les_deux();
    // ⭐ **CE JOURNAL EST L'INSTRUMENT QUI MANQUAIT.** Sans lui, « le bouton ne
    // fait rien » ne se distinguait pas de « la reconnaissance tourne mais rien
    // ne remonte » : les deux laissent l'écran identique, et le journal restait
    // muet dans le premier cas comme dans le second. Il sépare les deux moitiés.
    crate::diag!("[page-scan] lancement {} sur {}", moteur.nom(), page.display());
    match ocr::Reconnaissance::lancer(&page, moteur) {
        Ok(reconnaissance) => {
            RECONNAISSANCES.with(|r| r.borrow_mut()[i] = Some(reconnaissance));
            // ⭐ **L'ÉTAT PASSE À « EN COURS » TOUT DE SUITE** — et il faut que ça
            // se VOIE, dans l'hôte qui a demandé.
            //
            // ⛔ **C'ÉTAIT LE DÉFAUT RÉEL : « QUAND JE CLIQUE SUR RECONNAÎTRE, RIEN
            // NE SE PASSE »** (signalé le 2026-10-09). Ce `PAYLOAD`-ci est celui de
            // la FENÊTRE. Un clic dans le PANNEAU mettait bien `TEXTES` à « en
            // cours » côté Rust, mais la clé de cache du panneau restait la même :
            // son `push_state` n'envoyait rien, le bouton ne bougeait pas, et
            // l'écran restait identique pendant toute la reconnaissance. Le
            // commentaire d'origine promettait déjà « l'état passe à en cours tout
            // de suite » — il décrivait ce que le code aurait dû faire.
            TEXTES.with(|t| t.borrow_mut()[i] = Some(ocr::Etat::EnCours));
            invalider_les_deux();
        }
        // L'échec du LANCEMENT est un état comme un autre — voir `ocr`, qui met
        // la phrase dans l'erreur plutôt que de la garder pour lui.
        Err(message) => {
            TEXTES.with(|t| t.borrow_mut()[i] = Some(ocr::Etat::Echec(message.clone())));
            dire(message);
        }
    }
}

/// **RELÈVE LES DEUX MOTEURS** — appelée à chaque frame, panneau fermé compris :
/// une reconnaissance terminée pendant que la fenêtre est cachée doit être
/// ramassée quand même, sinon son résultat serait perdu.
pub(crate) fn relever_les_ocr() {
    let mut change = false;
    for i in 0..2 {
        // ⚠️ **ON SORT LE TRAVAIL DU CACHE AVANT DE RELEVER** : `relever` lit un
        // fichier, et le traitement touche `TEXTES`. Garder l'emprunt ouvert
        // ferait un `BorrowMutError` — qui, dans ce dépôt, échoue EN SILENCE.
        let Some(mut reconnaissance) = RECONNAISSANCES.with(|r| r.borrow_mut()[i].take()) else { continue };
        match reconnaissance.relever() {
            Some(etat) => {
                match &etat {
                    ocr::Etat::Echec(message) => crate::diag!("[ocr] {message}"),
                    ocr::Etat::Pret(texte) => crate::diag!("[ocr] {} caracteres reconnus", texte.chars().count()),
                    ocr::Etat::EnCours => {}
                }
                TEXTES.with(|t| t.borrow_mut()[i] = Some(etat));
                change = true;
            }
            // Toujours en train de tourner : on le remet tel quel.
            None => RECONNAISSANCES.with(|r| r.borrow_mut()[i] = Some(reconnaissance)),
        }
    }
    if change {
        invalider_les_deux();
    }
}

/// Le texte reconnu par un moteur, s'il y en a un.
fn texte_de(moteur: ocr::Moteur) -> Option<String> {
    TEXTES.with(|t| t.borrow()[indice(moteur)].as_ref().and_then(ocr::Etat::texte).map(str::to_string))
}

/// **COPIE LE TEXTE RECONNU.** ⚠️ **UN PRESSE-PAPIERS TENU SE DIT** — c'est
/// exactement pour ça que `clipboard::write_text` rend un `bool` au lieu de ne
/// rien rendre.
pub(crate) fn copier_le_texte(nom: &str) {
    let Some(moteur) = ocr::Moteur::depuis_nom(nom) else {
        return dire(format!("Moteur de reconnaissance inconnu : {nom:?}"));
    };
    match texte_de(moteur) {
        Some(texte) if !texte.is_empty() => {
            if ocr::copier_le_texte(&texte) {
                dire(format!("Texte de {} copié ({} caractères).", moteur.libelle(), texte.chars().count()));
            } else {
                dire("Le presse-papiers est tenu par une autre application — réessayez.".to_string());
            }
        }
        _ => dire(format!("Rien à copier : lancez d'abord « {0} ».", moteur.libelle())),
    }
}

/// **ENREGISTRE LE TEXTE DANS UN `.txt`.** ⚠️ La boîte est MODALE et gèle le
/// redessin du dock pendant qu'elle est ouverte — assumé, documenté dans
/// `win32::file_dialog`, et déjà vrai pour `cef_scanner::choisir_destination`.
pub(crate) fn enregistrer_le_texte(proprietaire: HWND, nom: &str) {
    let Some(moteur) = ocr::Moteur::depuis_nom(nom) else {
        return dire(format!("Moteur de reconnaissance inconnu : {nom:?}"));
    };
    let Some(texte) = texte_de(moteur) else {
        return dire(format!("Rien à enregistrer : lancez d'abord « {0} ».", moteur.libelle()));
    };
    let Some(page) = PAGE.with(|p| p.borrow().clone()) else {
        return dire("Aucune page n'est ouverte.".to_string());
    };
    // ⚠️ **SANS PROPRIÉTAIRE, ON REFUSE** : une boîte sans fenêtre propriétaire
    // peut passer DERRIÈRE, et l'application paraîtrait figée alors qu'elle attend
    // un choix invisible (voir `cef_scanner::dialogue_possible`).
    // ⚠️ **LE PROPRIÉTAIRE EST CELUI DE L'HÔTE QUI DEMANDE** — le panneau ou la
    // fenêtre. Une boîte sans propriétaire peut passer DERRIÈRE, et l'application
    // paraîtrait figée alors qu'elle attend un choix invisible.
    let propose = ocr::nom_de_fichier_texte(&page, moteur);
    let Some(chemin) = crate::win32::file_dialog::enregistrer_sous(
        proprietaire,
        "Enregistrer le texte reconnu",
        &propose,
        "txt",
        "Fichier texte (*.txt)",
    ) else {
        // Annuler n'est pas une erreur : rien à journaliser, rien à dire.
        return;
    };
    match std::fs::write(&chemin, &texte) {
        Ok(()) => dire(format!("Texte enregistré : {}", chemin.display())),
        Err(erreur) => dire(format!("{} n'a pas pu être écrit : {erreur}", chemin.display())),
    }
}

/// Repousse l'état, et l'image si elle n'est pas encore partie.
pub fn refresh_if_visible() {
    relever_les_ocr();
    if cef_popup::is_visible(&SLOT) {
        pousser_l_image();
        push_state();
    }
}

/// ⛔ **PAS de `resize_if_needed` ici**, contrairement à tous les panneaux : pour
/// une `fenetre`, la taille est celle de la ZONE CLIENTE, relue à chaque frame par
/// `render`. L'appeler ferait entrer la géométrie du dock en lutte avec le
/// redimensionnement de l'utilisateur.
pub fn render_if_visible(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

/// Voir `win32::cef_calculatrice::debug_dump_backbuffer` — le harnais a DEUX
/// listes à tenir (`NEURADESK_DEBUG_OPEN=page-scan`).
pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// Démarre un déplacement NATIF de la fenêtre, comme le ferait sa barre de titre
/// si elle en avait une (voir `cef_popup::deplacer_par_le_systeme`).
fn deplacer() {
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return };
    cef_popup::deplacer_par_le_systeme(hwnd, "page-scan");
}

pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("page-scan/") else {
            crate::diag!("[page-scan] action inconnue : {action:?}");
            continue;
        };
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["ready"] => {
                PAYLOAD.with(|c| *c.borrow_mut() = None);
                // ⛔ **L'IMAGE REPART.** `execute_js` ne signale jamais son échec :
                // une image poussée avant ce `ready` est perdue sans un mot, et
                // rouvrir le témoin est le seul moyen de la repousser.
                ENVOYEE.with(|e| *e.borrow_mut() = None);
            }
            ["close"] => cacher(),
            ["deplacer"] => deplacer(),
            ["precedente"] => naviguer(-1),
            ["suivante"] => naviguer(1),
            ["reconnaitre", moteur] => lancer_ocr(moteur),
            ["tourner", sens] => match Sens::depuis_nom(sens) {
                Some(sens) => tourner(sens),
                None => crate::diag!("[page-scan] sens de rotation inconnu : {sens:?}"),
            },
            // ⭐ **« COMPARER » LANCE LES DEUX.** Un échec de l'un n'empêche pas
            // l'autre : c'est justement quand un moteur se tait que l'autre est
            // intéressant.
            ["comparer"] => {
                for (_moteur, nom, _libelle) in ocr::Moteur::TOUS {
                    lancer_ocr(nom);
                }
            }
            ["copier", moteur] => copier_le_texte(moteur),
            // ⭐ **L'AMÉLIORATION D'IMAGE** — le réglage vit dans ce module, donc le
            // panneau et la fenêtre ne peuvent pas montrer deux filtres différents
            // de la même page (les mêmes fonctions sont appelées des deux côtés).
            ["ameliorer-auto"] => ameliorer_auto(),
            ["ameliorer-regler", luminosite, contraste, saturation] => ameliorer_regler(luminosite, contraste, saturation),
            ["ameliorer-zero"] => ameliorer_zero(),
            ["ameliorer-enregistrer"] => ameliorer_enregistrer(),
            // ⚠️ **LE PROPRIÉTAIRE DE LA BOÎTE EST CETTE FENÊTRE** — c'est
            // `cef_popup::hwnd`, qui rend `None` tant qu'elle n'existe pas : on
            // refuse alors plutôt que d'ouvrir une boîte orpheline.
            ["enregistrer", moteur] => match cef_popup::hwnd(&SLOT) {
                Some(hwnd) => enregistrer_le_texte(hwnd, moteur),
                None => dire("La fenêtre n'existe pas encore : rouvrez la page.".to_string()),
            },
            _ => crate::diag!("[page-scan] action inconnue : {action:?}"),
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // ⛔ **LA FENÊTRE N'A NI BORDURE NI BARRE DE TITRE** (voir la doc de module) :
    // le système ne fournit donc plus le redimensionnement par les bords, et c'est
    // ici qu'on le rend. Sans ce message, la fenêtre serait FIGÉE à sa taille de
    // création.
    if msg == WM_NCHITTEST {
        return code_de_redimensionnement(hwnd, point_du_message(lparam));
    }
    if msg == WM_CLOSE {
        cef_popup::cacher_fenetre(hwnd);
        return LRESULT(0);
    }
    if let Some(result) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Épaisseur, en pixels, de la bande de redimensionnement sur les bords — la
/// fenêtre n'ayant plus de bordure système, c'est à nous de la définir.
const BORDURE_REDIMENSIONNEMENT: i32 = 6;

/// Les coordonnées ÉCRAN d'un message souris. `GET_X_LPARAM`/`GET_Y_LPARAM` :
/// deux mots de 16 bits SIGNÉS — un simple masquage donnerait un x négatif (écran
/// à gauche du principal) lu comme 65500.
fn point_du_message(lparam: LPARAM) -> POINT {
    let v = lparam.0 as i32;
    POINT { x: (v << 16) >> 16, y: v >> 16 }
}

/// Quel curseur de redimensionnement pour ce point ? **Fonction PURE**, donc
/// testable : la comparer à `GetWindowRect` est la seule partie qui touche Win32.
fn code_pour_point(cadre: [i32; 4], point: POINT) -> u32 {
    let [g, h, l, b] = [cadre[0], cadre[1], cadre[0] + cadre[2], cadre[1] + cadre[3]];
    let gauche = point.x - g < BORDURE_REDIMENSIONNEMENT;
    let droite = l - point.x < BORDURE_REDIMENSIONNEMENT;
    let haut = point.y - h < BORDURE_REDIMENSIONNEMENT;
    let bas = b - point.y < BORDURE_REDIMENSIONNEMENT;
    match (gauche, droite, haut, bas) {
        (true, _, true, _) => HTTOPLEFT,
        (_, true, true, _) => HTTOPRIGHT,
        (true, _, _, true) => HTBOTTOMLEFT,
        (_, true, _, true) => HTBOTTOMRIGHT,
        (true, _, _, _) => HTLEFT,
        (_, true, _, _) => HTRIGHT,
        (_, _, true, _) => HTTOP,
        (_, _, _, true) => HTBOTTOM,
        _ => HTCLIENT,
    }
}

fn code_de_redimensionnement(hwnd: HWND, point: POINT) -> LRESULT {
    let mut cadre = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut cadre) }.is_err() {
        return LRESULT(HTCLIENT as isize);
    }
    let [g, h] = [cadre.left, cadre.top];
    LRESULT(code_pour_point([g, h, cadre.right - g, cadre.bottom - h], point) as isize)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les verbes que `dispatch_actions` accepte — même lecture que
    /// `cef_scanner::tests::verbes_du_dispatch`.
    fn verbes_du_dispatch() -> Vec<String> {
        let source = include_str!("cef_page_scan.rs");
        source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(source)
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//"))
            .filter_map(|l| {
                let reste = l.strip_prefix("[\"")?;
                let (verbe, fin) = reste.split_once('"')?;
                matches!(fin.chars().next(), Some(']') | Some(','))
                    .then(|| verbe.to_string())
            })
            .collect()
    }

    fn verbes_de_la_page() -> Vec<String> {
        const SOURCE: &str = include_str!("../../assets/popups/page_scan.html");
        SOURCE
            .match_indices("go(")
            .filter_map(|(i, _)| {
                let reste = &SOURCE[i + 3..];
                let guillemet = reste.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                reste[1..].split(guillemet).next().map(str::to_string)
            })
            .collect()
    }

    /// ⭐ **LE MÊME CONTRAT QUE PARTOUT** : une page qui émet un verbe que Rust ne
    /// connaît pas ne produit aucune erreur — juste un clic sans effet.
    #[test]
    fn la_page_et_le_dispatch_acceptent_les_memes_verbes() {
        let mut page = verbes_de_la_page();
        let mut rust = verbes_du_dispatch();
        page.sort();
        page.dedup();
        rust.sort();
        rust.dedup();
        assert_eq!(page, rust);
    }

    /// ⛔ **SANS CE MESSAGE, LA FENÊTRE SERAIT FIGÉE.** `fenetre: true` pose
    /// `WS_POPUP`, donc le système ne fournit plus le redimensionnement : ce test
    /// est ce qui empêche de « simplifier » la fonction un jour.
    #[test]
    fn les_bords_rendent_le_bon_code_de_redimensionnement() {
        let cadre = [100, 200, 600, 500]; // g, h, largeur, hauteur
        let au = |x: i32, y: i32| code_pour_point(cadre, POINT { x, y });
        assert_eq!(au(100, 200), HTTOPLEFT, "le coin haut-gauche");
        assert_eq!(au(700, 200), HTTOPRIGHT, "le coin haut-droit");
        assert_eq!(au(100, 700), HTBOTTOMLEFT, "le coin bas-gauche");
        assert_eq!(au(700, 700), HTBOTTOMRIGHT, "le coin bas-droit");
        assert_eq!(au(100, 450), HTLEFT, "le bord gauche");
        assert_eq!(au(700, 450), HTRIGHT, "le bord droit");
        assert_eq!(au(400, 200), HTTOP, "le bord haut");
        assert_eq!(au(400, 700), HTBOTTOM, "le bord bas");
        assert_eq!(au(400, 450), HTCLIENT, "et le milieu appartient a la page");
    }

    /// ⚠️ **LES COORDONNÉES NÉGATIVES SONT RELUES CORRECTEMENT** — et c'est le
    /// DÉCODAGE qui est en cause, pas le calcul.
    ///
    /// Un écran à gauche du principal donne des x NÉGATIFS dans le `lparam`, sous
    /// forme de complément à deux sur 16 bits. Un simple masquage les lirait comme
    /// 65500 : le bord gauche ne se saisirait plus jamais, et le défaut serait
    /// « la fenêtre ne se redimensionne pas par ce côté-là, parfois ».
    #[test]
    fn les_coordonnees_negatives_sont_relues_correctement() {
        // (-1920, 300) : mot bas = 0xF880, mot haut = 0x012C.
        let lparam = LPARAM((((300u32) << 16) | 0xF880) as isize);
        assert_eq!(point_du_message(lparam), POINT { x: -1920, y: 300 });

        // Et le calcul, sur un cadre lui aussi négatif (écran secondaire à gauche).
        let cadre = [-1920, 0, 800, 600]; // x de -1920 à -1120
        assert_eq!(code_pour_point(cadre, POINT { x: -1920, y: 300 }), HTLEFT, "le bord gauche");
        assert_eq!(code_pour_point(cadre, POINT { x: -1120, y: 300 }), HTRIGHT, "le bord droit");
        assert_eq!(code_pour_point(cadre, POINT { x: -1520, y: 300 }), HTCLIENT, "et le milieu");
    }

    /// ⛔ **TOURNER DEUX FOIS NE DOIT PAS DÉTRUIRE LA PAGE.**
    ///
    /// C'est le défaut signalé le 2026-10-09, mot pour mot : « page-1.jpg n'a pas
    /// pu être relue : Format error decoding Jpeg: I/O errors Not enough bytes,
    /// expected 2 but found 0 ». La première rotation avait **tronqué la page**
    /// avant d'échouer sur un codec qui refuse le RGBA — et la numérisation était
    /// perdue.
    ///
    /// **Ce test ÉCHOUE sur l'ancien code** : la seconde relecture ne trouve plus
    /// un JPEG valide. Il écrit un vrai fichier, parce que c'est le DISQUE qui
    /// était en cause — un test en mémoire n'aurait rien vu.
    #[test]
    fn tourner_deux_fois_ne_detruit_pas_la_page() {
        let chemin = std::env::temp_dir().join("neuradesk-test-rotation.jpg");
        let _ = std::fs::remove_file(&chemin);
        let mut temoin = image::RgbImage::new(2, 1);
        temoin.put_pixel(0, 0, image::Rgb([255, 0, 0]));
        temoin.put_pixel(1, 0, image::Rgb([0, 0, 255]));
        temoin.save(&chemin).expect("le temoin doit s'ecrire");

        // Un quart de tour échange les côtés : 2x1 -> 1x2 -> 2x1.
        for (etape, attendu) in [(1, (1u32, 2u32)), (2, (2, 1))] {
            tourner_fichier(&chemin, Sens::Horaire).unwrap_or_else(|e| panic!("etape {etape} : {e}"));
            let relue = image::open(&chemin).unwrap_or_else(|e| panic!("etape {etape} : LA PAGE DOIT RESTER LISIBLE : {e}"));
            assert_eq!((relue.width(), relue.height()), attendu, "etape {etape}");
        }
        // Et aucun temporaire ne doit rester derriere.
        assert!(!chemin.with_extension("tmp.jpg").exists(), "le temporaire doit avoir disparu");
        let _ = std::fs::remove_file(&chemin);
    }

    /// ⭐ **LE SENS DE LA ROTATION, ANCRÉ SUR UN PIXEL.** C'est le seul endroit
    /// où l'on peut se tromper, et **une rotation à l'envers ne se voit qu'à
    /// l'usage** — l'utilisateur conclurait que le bouton ne fait rien, ou pire,
    /// que la page « se tourne toute seule dans le mauvais sens ».
    ///
    /// Le repère : une image de 2×1 dont le pixel GAUCHE est rouge. Tournée d'un
    /// quart de tour **horaire**, la gauche passe **en haut**.
    #[test]
    fn un_quart_de_tour_horaire_va_bien_vers_la_droite() {
        let mut image = image::RgbaImage::new(2, 1);
        image.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        image.put_pixel(1, 0, image::Rgba([0, 0, 255, 255]));

        let horaire = tourner_image(image.clone(), Sens::Horaire);
        assert_eq!((horaire.width(), horaire.height()), (1, 2), "les dimensions tournent aussi");
        assert_eq!(horaire.get_pixel(0, 0).0, [255, 0, 0, 255], "a DROITE, la gauche passe EN HAUT");
        assert_eq!(horaire.get_pixel(0, 1).0, [0, 0, 255, 255]);

        let antihoraire = tourner_image(image.clone(), Sens::Antihoraire);
        assert_eq!(antihoraire.get_pixel(0, 0).0, [0, 0, 255, 255], "a GAUCHE, la gauche passe EN BAS");

        // Les deux sens sont OPPOSÉS, et se compensent — sans quoi ils seraient
        // le même, et un bouton sur deux ne ferait rien.
        assert_ne!(horaire, antihoraire);
        assert_eq!(tourner_image(horaire, Sens::Antihoraire), image);
    }

    /// Les noms des sens font l'aller-retour — c'est ce que la page envoie.
    #[test]
    fn les_noms_de_sens_font_l_aller_retour() {
        for (sens, nom) in Sens::TOUS {
            assert_eq!(Sens::depuis_nom(nom), Some(sens));
            assert_eq!(Sens::depuis_nom(&nom.to_uppercase()), Some(sens));
        }
        assert_eq!(Sens::depuis_nom("quart"), None);
    }

    /// ⭐ **L'ÉTAT DE PAGE PORTE TOUT CE QUE LES DEUX HÔTES DESSINENT.** Un champ
    /// oublié ici ne casse rien : la page lirait `undefined` et dessinerait une
    /// colonne VIDE — le défaut que ce test rend impossible. Même discipline que
    /// `cef_scanner::tests::l_etat_porte_chaque_reglage_et_ses_choix`.
    ///
    /// ⚠️ **`ocr` EST LE CHAMP QUI COMPTE** : c'est lui qui fait apparaître les
    /// colonnes. Vide, le volet s'ouvre sur RIEN — ce qui ressemble à « le volet
    /// est masqué », et envoie chercher le défaut au mauvais endroit.
    #[test]
    fn l_etat_de_page_porte_les_deux_moteurs() {
        let etat = etat_page();
        for cle in ["nom", "rang", "total", "ocr", "message", "amelioration"] {
            assert!(etat.get(cle).is_some(), "l'etat de page ne porte pas {cle}");
        }
        let ocr = etat["ocr"].as_array().expect("ocr doit etre une LISTE");
        assert_eq!(ocr.len(), ocr::Moteur::TOUS.len(), "un moteur par colonne");
        assert_eq!(ocr[0]["nom"], ocr::Moteur::Tesseract.nom());
        assert_eq!(ocr[1]["nom"], ocr::Moteur::Windows.nom());
        // L'ordre compte : c'est `indice` qui fait le lien entre la table Rust et
        // l'ordre des colonnes.
        for (valeur, (moteur, _nom, libelle)) in ocr.iter().zip(ocr::Moteur::TOUS) {
            assert_eq!(valeur["libelle"], libelle, "{moteur:?} n'est pas a sa place");
        }
    }

    /// ⭐ **LA FENÊTRE NE DOIT PAS DÉBORDER DE L'ÉCRAN**, et elle ne doit jamais
    /// être nulle — même pour une taille absurde.
    #[test]
    fn la_fenetre_d_ouverture_tient_dans_l_ecran() {
        let (largeur, hauteur) = crate::win32::cef_couleurs::taille_pour_ecran(
            unsafe { GetSystemMetrics(SM_CXSCREEN) },
            unsafe { GetSystemMetrics(SM_CYSCREEN) },
            (LARGEUR_UNITES, HAUTEUR_UNITES),
        );
        let (ecran_l, ecran_h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        assert!(largeur > 0 && hauteur > 0);
        assert!(largeur <= ecran_l && hauteur <= ecran_h);
        let [g, h, l, b] = rect_defaut();
        assert!(g >= 0 && h >= 0, "la fenetre ne part pas hors de l'ecran");
        assert!(g + l <= ecran_l && h + b <= ecran_h);
    }

    /// Le type MIME suit l'extension : un `image/jpeg` sur un PNG donnerait une
    /// image cassée, sans erreur.
    #[test]
    fn le_type_mime_suit_l_extension() {
        assert_eq!(mime_pour(Path::new(r"C:\T\page-1.jpg")), "image/jpeg");
        assert_eq!(mime_pour(Path::new(r"C:\T\page-1.JPEG")), "image/jpeg");
        assert_eq!(mime_pour(Path::new(r"C:\T\page-1.png")), "image/png");
        assert_eq!(mime_pour(Path::new(r"C:\T\page-1.bmp")), "image/bmp");
    }

    /// Le nom affiché est le dernier segment, jamais le chemin entier.
    #[test]
    fn le_nom_affiche_est_le_fichier() {
        assert_eq!(nom_de_fichier(Path::new(r"C:\Temp\neuradesk-scanner\page-7.jpg")), "page-7.jpg");
    }

    /// ⛔ **UN RÉGLAGE N'APPARTIENT QU'À LA PAGE QUI L'A REÇU.** Sans la
    /// comparaison, le slot serait « le dernier réglage posé » et la navigation
    /// emporterait les curseurs de la page 2 sur la page 3 : une page grise
    /// soudain éclaircie, sans un mot, et l'OCR lancé dessus dans cet état.
    ///
    /// **Ce test ROUGIT si on retire la comparaison** (slot lu tel quel) — vérifié.
    #[test]
    fn un_reglage_n_appartient_qu_a_la_page_qui_l_a_recu() {
        let reglage = Reglage { luminosite: 20, contraste: 40, saturation: 0 };
        let slot = Some((PathBuf::from(r"C:\T\page-3.jpg"), reglage));
        assert_eq!(reglage_de(&slot, Some(Path::new(r"C:\T\page-3.jpg"))), reglage, "la page qui l'a reçu le retrouve");
        assert_eq!(
            reglage_de(&slot, Some(Path::new(r"C:\T\page-4.jpg"))),
            Reglage::neutre(),
            "une autre page ne recoit PAS le réglage de la précédente"
        );
        assert_eq!(reglage_de(&slot, None), Reglage::neutre(), "aucune page ouverte ne recoit rien");
        assert_eq!(reglage_de(&None, Some(Path::new(r"C:\T\page-3.jpg"))), Reglage::neutre(), "slot vide = rien");
    }

    /// ⭐ **L'ÉTAT PORTE LES TROIS CURSEURS ET LE FILTRE CALCULÉ PAR RUST** — c'est
    /// la seule raison pour laquelle les deux hôtes dessinent la même image, et
    /// pour laquelle l'aperçu ressemble au fichier écrit.
    #[test]
    fn l_etat_porte_les_trois_curseurs_et_le_filtre_de_rust() {
        let reglage = Reglage { luminosite: 20, contraste: 18, saturation: -12 };
        let etat = reglage_json(&reglage);
        assert_eq!(etat["luminosite"], 20);
        assert_eq!(etat["contraste"], 18);
        assert_eq!(etat["saturation"], -12);
        assert_eq!(etat["filtre"], "brightness(120%) contrast(118%) saturate(88%)");
        assert_eq!(reglage_json(&Reglage::neutre())["filtre"], "none", "une page intacte recoit aucun filtre");
    }

    /// ⚠️ **UNE VALEUR DE PAGE HORS BORNE EST PLANCHÉE À L'ENTRÉE**, avant d'être
    /// stockée : sinon l'aperçu (planché par `facteur`) et l'état poussé (qui
    /// reprendrait 900) se contrediraient — le curseur à 200 % et le fichier écrit
    /// à 200 %, mais la page disant « 900 ».
    /// ⚠️ **UN LEVIER DE HARNAIS MAL FORMÉ NE POSE RIEN EN DEMI** : une valeur
    /// moitiéd (deux sur trois) photographierait une page réglée de travers en
    /// croyant montrer un réglage complet — et la conclusion de la capture irait
    /// au mauvais endroit.
    #[test]
    fn le_levier_de_debug_ne_pose_rien_s_il_est_mal_forme() {
        let page = Path::new(r"C:\T\page-9.jpg");
        assert!(preset_reglage_de_debug(page, "30,40,-10"), "trois entiers portent");
        assert_eq!(
            reglage_de(&REGLAGE.with(|r| r.borrow().clone()), Some(page)),
            Reglage { luminosite: 30, contraste: 40, saturation: -10 }
        );
        assert!(!preset_reglage_de_debug(page, "30,40"), "deux valeurs ne portent pas");
        assert!(!preset_reglage_de_debug(page, "clair,fonce,0"), "du texte ne porte pas");
        assert_eq!(
            reglage_de(&REGLAGE.with(|r| r.borrow().clone()), Some(page)),
            Reglage { luminosite: 30, contraste: 40, saturation: -10 },
            "un refus laisse le réglage déjà posé en place"
        );
        // La borne s'applique AUSSI au levier : un harnais qui demanderait 900
        // photographierait un filtre à 200 % pendant que l'état dirait 900.
        assert!(preset_reglage_de_debug(page, "900,-900,0"));
        assert_eq!(reglage_de(&REGLAGE.with(|r| r.borrow().clone()), Some(page)), Reglage { luminosite: 100, contraste: -100, saturation: 0 });
    }

    #[test]
    fn un_reglage_de_page_hors_borne_est_planche_avant_d_etre_stocke() {
        assert_eq!(Reglage { luminosite: 900, contraste: -900, saturation: 12 }.borne(), Reglage { luminosite: 100, contraste: -100, saturation: 12 });
    }
}
