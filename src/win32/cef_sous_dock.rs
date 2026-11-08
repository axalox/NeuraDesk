//! **LE DASH DOSSIER** — DEUX explorateurs côte à côte, dans la zone libre du dock.
//!
//! Chaque moitié montre les enfants d'un dossier, et **un dossier s'ouvre au
//! CLIC** — comme dans l'Explorateur. Le survol d'une case ne fait plus
//! qu'**éclairer** la case visée (voir `installerSurvol` dans la page) ; il en a
//! fait autre chose, et le retour d'usage a tranché : « je n'aime pas que les
//! dossiers s'ouvrent tout seul au survol ». Les deux moitiés sont INDÉPENDANTES,
//! et celle de **droite** s'ouvre sur le **dossier personnel**
//! (`config::load_sous_dock_personnel`).
//!
//! LOT 1 de `docs/plans/barre-du-bas.md`, inspiré de l'applet `Folders` de
//! Cairo-Dock. Le lot 2 y ajoutera la sélection multiple et les gestes
//! copier/déplacer d'une moitié vers l'autre.
//!
//! ⚠️ **C'EST UN PANNEAU, PAS UN MENU — ET C'EST LA QUESTION À SE POSER AVANT
//! TOUTE INTERACTION DANS UNE PAGE CEF** (règle du dépôt) : un menu charge
//! `menu.js` et reste **MUET** ; un panneau **peut parler**. Ici la page doit
//! demander à ENTRER dans un sous-dossier ou à LANCER un fichier, donc il lui
//! faut la parole.
//!
//! **LE CONTENU EST CONSTRUIT AU CHANGEMENT, JAMAIS PAR FRAME** — lire un dossier
//! rend une **chaîne déjà construite**, que `poll` repousse pendant une courte
//! rafale après un changement (voir `RAFALE_DE_POUSSEE`), sans quoi la première
//! poussée serait perdue si la page n'a pas fini de charger — défaut mesuré trois
//! fois dans ce dépôt.
//!
//! ⭐ **LES ICÔNES, ELLES, SE CALCULENT PAR FRAME — ET C'EST UN CHANGEMENT.** Elles
//! coûtent cher (`SHGetFileInfoW` + `IImageList::GetIcon`, voir
//! `sous_dock::icone_du_shell`) ; les calculer toutes à l'ouverture obligeait à
//! **couper la liste**, donc à ne pas montrer des fichiers. Elles se font
//! maintenant par petits lots, au fur et à mesure que la file se vide (voir
//! `ICONES_A_ENVOYER` et `envoyer_des_icones`) : l'état part tout de suite et
//! complet, les images suivent.
//!
//! Toute la mécanique fenêtre/rendu/relais souris est dans `win32::cef_popup`.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::sous_dock::{Cote, Tri};

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// **LES DEUX MOITIÉS, DANS L'ORDRE DE `Cote::TOUTES`.**
    ///
    /// ⛔ **UNE SEULE DÉCLARATION POUR LES DEUX, ET C'EST DÉLIBÉRÉ.** Deux paires
    /// de `thread_local!` écrites côte à côte auraient divergé — c'est le défaut
    /// que ce dépôt a déjà payé plusieurs fois (« deux moitiés écrites à deux
    /// endroits »). Tout ce qui touche une moitié passe par `avec`.
    static MOITIES: RefCell<[Moitie; 2]> =
        const { RefCell::new([Moitie::neuve(), Moitie::neuve()]) };
    /// Les dossiers déjà lus, pour qu'entrer dans un dossier puis en ressortir ne
    /// relise pas le disque. Voir `contenu`.
    static CACHE: RefCell<Vec<(PathBuf, Tri, Contenu)>> = const { RefCell::new(Vec::new()) };
    /// **LE NUMÉRO D'OUVERTURE — ET SEULEMENT D'OUVERTURE.** Il ne bouge que
    /// dans `ouvrir`, jamais dans un rafraîchissement. La page s'en sert pour
    /// rendre ses DEUX moitiés : c'est ce qui fait survivre la toute première
    /// poussée, qui peut arriver avant que la page n'ait fini de charger.
    ///
    /// ⛔ **IL A D'ABORD ÉTÉ INCRÉMENTÉ À CHAQUE CHANGEMENT, ET C'ÉTAIT UN
    /// DÉFAUT.** La page rendait alors les deux moitiés à chaque survol de
    /// nappe — donc la recherche de la moitié DROITE repartait à zéro pendant
    /// qu'on explorait à gauche, et sa grille était reconstruite EN ENTIER —
    /// icônes comprises — pour rien. C'est exactement ce que la révision par
    /// moitié existe pour éviter ; le compteur global la contournait.
    static OPEN_COUNT: Cell<u64> = const { Cell::new(0) };
    /// **LE NUMÉRO DE CHANGEMENT** — lui, bouge à chaque fois qu'une moitié
    /// change, ouverture comprise. Il ne va PAS dans l'état : il ne sert qu'à la
    /// rafale de poussée et à la sonde, qui doivent réagir à tout changement.
    static CHANGEMENTS: Cell<u64> = const { Cell::new(0) };
    /// **LA RAFALE DE POUSSÉE** — `(numéro de changement déjà poussé, frames
    /// écoulées depuis)`. Voir `poll`.
    static POUSSEE: Cell<(u64, u32)> = const { Cell::new((0, u32::MAX)) };
    /// Le dernier dossier de gauche ÉCRIT SUR LE DISQUE — pour ne pas réécrire le
    /// même chemin à chaque changement de cette moitié (voir `memoriser_gauche`).
    static GAUCHE_MEMORISEE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    /// **LE DOSSIER PERSONNEL, RÉSOLU UNE FOIS** — voir `dossier_personnel`.
    static PERSONNEL: RefCell<Option<DossierPersonnel>> = const { RefCell::new(None) };
    /// **LES CHEMINS DONT L'ICÔNE RESTE À FAIRE**, dans l'ordre où on les a
    /// rencontrés — la file est vidée par lots dans `poll`.
    ///
    /// ⭐ **ELLE PORTE DES CHEMINS, PAS DES ICÔNES** — et c'est tout le correctif.
    /// Les icônes étaient calculées AVANT d'entrer ici (à la lecture du dossier),
    /// ce qui obligeait à couper la liste à 48 pour que l'ouverture reste
    /// supportable : les fichiers au-delà disparaissaient de l'écran. Désormais
    /// le calcul se fait **au moment du vidage**, 24 par frame, exactement comme
    /// `cef_recherche::mettre_en_file` / `cef_recherche::icone`.
    static ICONES_A_ENVOYER: RefCell<std::collections::VecDeque<String>> =
        const { RefCell::new(std::collections::VecDeque::new()) };
/// **LES CHEMINS DONT L'ICÔNE EST DÉJÀ EN FILE OU PARTIE** depuis la dernière
/// ouverture.
///
/// ⚠️ **ON MÉMORISE LE CHEMIN, PAS L'ICÔNE** : quelques dizaines d'octets par
/// entrée, là où garder les images serait le coût qu'on cherche justement à
/// éviter. C'est la PAGE qui garde les images.
///
/// ⛔ **LE CHEMIN EST MARQUÉ À L'EMPILEMENT, PAS À L'ENVOI** (voir
/// `mettre_en_file_les_icones`). C'est ce qui empêche d'empiler deux fois le même
/// chemin quand deux dossiers le montrent. La conséquence est qu'**on ne peut pas
/// vider la file sans démarquer** : les chemins jetés seraient tenus pour partis
/// et la page garderait des carrés en pointillés jusqu'à la prochaine ouverture.
///
/// **IL N'Y A DONC QUE DEUX VIDAGES, ET ILS NE DISENT PAS LA MÊME CHOSE** :
/// `oublier_la_file_d_icones` jette les chemins *en attente* et les démarque —
/// ils pourront repartir ; `ouvrir`, lui, efface **les deux carnets** parce que
/// la page efface son cache d'icônes au même instant (sur le changement de
/// `open`) et qu'il faut donc TOUT renvoyer, y compris ce qui était déjà parti.
/// Prendre l'un pour l'autre perd des icônes, dans un sens comme dans l'autre.
///
/// ⛔ **IL EST VIDÉ À CHAQUE OUVERTURE** (voir `ouvrir`), comme
/// `cef_dash::DashData::pending_icon_loads` : la page efface son cache au même
/// moment (sur le changement de `open`), et si elle a rechargé — un plantage de
/// rendu, par exemple — elle a tout perdu. Le prix est de renvoyer les icônes
/// UNE fois par ouverture, ce qui reste sans commune mesure avec les 44 Mo de la
/// rafale qu'on vient de supprimer.
static ICONES_ENVOYEES: RefCell<std::collections::HashSet<String>> =
    RefCell::new(std::collections::HashSet::new());
/// **LE COMPTE RENDU DU DERNIER GESTE, PAR MOITIÉ** — effacé dès que la moitié
/// change de dossier (un rapport sur le dossier précédent n'a plus de sens).
/// **LES VIGNETTES À REDEMANDER** — chemin, instant où réessayer, essais déjà
/// faits. Voir `RELANCE_VIGNETTE`.
static RELANCES: RefCell<Vec<(String, std::time::Instant, u32)>> = const { RefCell::new(Vec::new()) };
/// Le niveau de zoom (voir `niveau_de_zoom`) — `None` tant qu'il n'est pas lu.
static NIVEAU_ZOOM: Cell<Option<usize>> = const { Cell::new(None) };
static MESSAGE: RefCell<[Option<String>; 2]> = const { RefCell::new([None, None]) };
/// Numéro de la dernière poussée JOURNALISÉE — voir la sonde de
    /// `push_state`. N'existe que pour ne pas écrire 60 lignes par seconde.
    static SONDE_JS: RefCell<u64> = const { RefCell::new(u64::MAX) };
}

/// **LE PANNEAU EST-IL MAINTENU OUVERT ?** — la grâce et le `toggle` d'`ouvrir`
/// s'en servent pour NE PAS refermer.
///
/// ⛔ **LE DRAPEAU VIT DANS `cef_popup`, PAS ICI** (voir `PopupSlot::epingle`), et
/// ce module n'en est qu'un LECTEUR. Il a d'abord eu le sien — `MAINTENU` — avant
/// que la punaise soit étendue aux quinze panneaux : deux drapeaux pour la même
/// idée auraient fini par diverger, et le bouton — dont l'état est posé par
/// `cef_popup` à chaque frame — aurait clignoté entre les deux.
pub fn est_maintenu() -> bool {
    cef_popup::est_epingle(&SLOT)
}

/// **FAUT-IL REFERMER LE PANNEAU ?** — la DÉCISION, séparée de l'effet pour être
/// vérifiable sans fenêtre, sans souris et sans CEF (voir le test qui
/// l'accompagne).
///
/// ⚠️ **ELLE VIT ICI, PAS DANS `app.rs`** : c'est la RÈGLE du panneau, à côté du
/// drapeau qu'elle consulte. Enfouie dans la boucle de rendu, elle ne se
/// vérifierait qu'en lançant l'application et en bougeant la souris — c'est-à-dire
/// jamais, dans ce dépôt où aucune entrée n'est simulée.
///
/// ⚠️ **`maintenu` EST UN PARAMÈTRE, PAS LU ICI** : c'est ce qui rend les huit cas
/// énumérables d'un coup dans un test, au lieu de dépendre d'un état global.
pub fn doit_fermer(maintenu: bool, sur_la_vignette: bool, sur_le_panneau: bool) -> bool {
    // Le curseur est sur la cible : rien à refermer, punaise ou pas.
    if sur_la_vignette || sur_le_panneau {
        return false;
    }
    // ⛔ **LA PUNAISE PASSE APRÈS** — et c'est tout son sens : le curseur est
    // ailleurs, et le panneau reste quand même.
    !maintenu
}

/// **FAUT-IL BASCULER LA FENÊTRE, OU SEULEMENT LA REMPLIR ?** — la seconde décision
/// d'`ouvrir`, extraite pour la même raison que `doit_fermer` : elle se vérifie sans
/// fenêtre, sans souris et sans CEF.
///
/// ⛔ **`cef_popup::toggle` EST UNE VRAIE BASCULE** : appelé sur un panneau DÉJÀ
/// VISIBLE, il le **MASQUE**. Or `app.rs` rappelle `ouvrir` à chaque changement de
/// vignette survolée — sa branche d'ouverture n'a aucune garde `is_visible` (c'est
/// délibéré : c'est ce qui fait suivre le dossier). Basculer à ce moment-là ne fait
/// donc pas « suivre » le survol : **elle le referme.**
///
/// ⚠️ **C'EST LE DÉFAUT SIGNALÉ, MOT POUR MOT** : « le premier s'ouvre dans le dash
/// et le passage sur le second ferme le dash au lieu d'actualiser le dash pour le
/// dossier survolé ». Et il ne se rattrape pas tout seul : `sous_dock_open_for` a
/// bien été mis à jour, donc la frame suivante ne rappelle pas `ouvrir` — le panneau
/// reste FERMÉ jusqu'à la vignette d'après, qui le rouvrira. D'où une alternance
/// ouvre / ferme / ouvre en balayant la barre.
///
/// ⛔ **LA GARDE D'ORIGINE NE COUVRAIT QUE LA PUNAISE** (`est_maintenu()`), alors que
/// son propre commentaire décrivait déjà ce symptôme : elle réparait le panneau
/// MAINTENU et laissait le cas ordinaire se refermer — « sans punaise, rien ne
/// change », disait-elle. Le drapeau n'a rien à faire ici : un panneau simplement
/// VISIBLE ne doit pas plus se basculer qu'un panneau épinglé.
pub fn doit_basculer(visible: bool) -> bool {
    !visible
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefSousDockWindow"),
    window_title: w!("NeuraDesk — Dossier"),
    html_file: "sous_dock.html",
    wndproc: Some(wndproc),
    // ⛔ **PANNEAU, PAS MENU — ET C'EST LA CORRECTION DE LA VOIE CEF.**
    //
    // Ces deux drapeaux sont CE QUI DISTINGUE les deux familles du dépôt, et je
    // les avais mis du côté des MENUS : `above_a_magnified_dock: true` et
    // `fades: true`. Ma page vivait donc dans une fenêtre de la taille d'une
    // vignette (64 px), dont la taille devait être NÉGOCIÉE — et son script ne
    // s'exécutait pas.
    //
    // L'Éditeur d'image, le Calendrier et les Notes sont des PANNEAUX : ils
    // reçoivent le grand rectangle de la zone libre (`dash_rect`), n'ont aucune
    // taille à négocier, et **leurs scripts tournent** (le Terminal fait tourner
    // xterm.js). C'est la comparaison qui a trouvé la différence, pas une
    // hypothèse.
    above_a_magnified_dock: false,
    fades: false,
    fenetre: false,
};

/// Côté des icônes ENVOYÉES à la page, en pixels PHYSIQUES — voir `enfants_prets`,
/// où le pourquoi est écrit.
///
/// **96 = 32 × 3**, et les deux facteurs sont mesurables : la case de
/// `sous_dock.html` fait `width: 32px` en CSS, et le contenu d'un panneau CEF
/// est zoomé par `cef_popup::CONTENT_ZOOM_FACTOR`, qui vaut **3,0** (« multiplie
/// par 3 la taille des menus et des dashs »). En dessous, l'icône est
/// interpolée à l'écran ; au-dessus, on transporte des pixels que personne ne
/// verra.
#[cfg(test)]
const ICONE_PX: u32 = 96;

/// **LES TAILLES DE CASE DU ZOOM, EN PIXELS CSS** — DEMANDE EXPLICITE (2026-10-11) :
/// « pouvoir changer le niveau de zoom ». Le rang 0 est l'ancienne taille fixe
/// (32 px, d'où `ICONE_PX` = 96). ⚠️ **LA PAGE PORTE LA MÊME TABLE**
/// (`TAILLES_CASE` dans `sous_dock.html`) : le test
/// `la_page_porte_la_meme_table_de_zoom` les compare.
const TAILLES_CASE: [u32; 5] = [32, 48, 64, 96, 128];

/// Le niveau de zoom en cours — relu une fois du disque, puis tenu ici.
fn niveau_de_zoom() -> usize {
    NIVEAU_ZOOM.with(|n| match n.get() {
        Some(niveau) => niveau,
        None => {
            let niveau = crate::config::load_dossier_zoom().unwrap_or(0).min(TAILLES_CASE.len() - 1);
            n.set(Some(niveau));
            niveau
        }
    })
}

/// **LE CÔTÉ DES IMAGES ENVOYÉES, EN PIXELS PHYSIQUES** — la case du niveau en
/// cours, fois `CONTENT_ZOOM_FACTOR` (3). C'est ce qui fait qu'un zoom avant
/// redemande des images plus grandes au lieu d'agrandir les petites.
fn icone_px() -> u32 {
    TAILLES_CASE[niveau_de_zoom()] * 3
}

/// Nombre de dossiers gardés en mémoire. Chaque entrée ne pèse plus que ses noms
/// et ses chemins (les icônes en sont sorties, voir `ICONES_A_ENVOYER`) : six
/// suffisent à couvrir l'aller-retour dans une arborescence, qui est le geste
/// qu'on répète.
const CACHE_MAX: usize = 6;

/// **LE PLAFOND D'ICÔNES PAR LOT** — un plafond, plus une consigne.
///
/// ⚠️ **CE N'EST PLUS LUI QUI DÉCIDE DU RYTHME** : c'est
/// `ICONES_MS_PAR_FRAME`, un budget de TEMPS. Garder un nombre fixe reviendrait à
/// choisir le pire des deux mondes — trop lent quand les icônes sont bon marché,
/// et une frame gelée quand elles ne le sont pas. Il ne reste ici que pour borner
/// la TAILLE du JavaScript envoyé à la page en une fois.
const ICONES_PAR_FRAME: usize = 24;

/// **LE BUDGET DE TEMPS D'UN LOT D'ICÔNES, EN MILLISECONDES.**
///
/// ⛔ **MESURÉ, PAS ESTIMÉ.** Sur le harnais, avec un plafond de 24 et l'ancien
/// redimensionnement, les quatre lots d'une ouverture ont pris **100,6 · 86,4 ·
/// 129,6 · 52,2 ms** — soit six à huit frames perdues par lot, sur un budget de
/// 16,6 ms. Une icône coûte entre ~0,8 ms (type déjà vu par le shell) et
/// plusieurs dizaines de ms (premier accès, disque lent, partage réseau) : aucun
/// NOMBRE fixe ne peut convenir aux deux.
///
/// ⚠️ **8 ms LAISSE LA MOITIÉ DE LA FRAME AU RENDU.** Le remplissage est un peu
/// plus long en tout, mais il ne se voit plus.
const ICONES_MS_PAR_FRAME: f64 = 8.0;

/// **COMBIEN DE FRAMES ON REPOUSSE L'ÉTAT APRÈS UN CHANGEMENT.**
///
/// ⚠️ **CE N'EST PAS UNE OPTIMISATION DE CONFORT, C'EST CE QUI REND LE LOT
/// POSSIBLE.** L'état porte maintenant DEUX grilles avec leurs icônes, soit
/// plusieurs dizaines de kilo-octets, et `poll` le repoussait **à chaque frame**
/// tant que le panneau était visible. Le motif d'origine existe pour une raison
/// mesurée — « la poussée faite juste après la création de la fenêtre peut
/// arriver AVANT que `window.setState` n'existe côté JS, et être perdue en
/// silence » — mais cette raison ne couvre que le tout début.
///
/// On repousse donc pendant une seconde (60 frames à 60 Hz) après chaque
/// changement, puis on se TAIT jusqu'au suivant. Le remède d'origine est
/// intégralement conservé — la page est PRÉCHARGÉE (`cef_popup::preload`), donc
/// elle est déjà chargée quand on ouvre, et une seconde est une marge large —
/// et le coût de croisière tombe à zéro. Le remède PROPRE serait un acquittement
/// de la page (`go("pret")`) ; il n'est pas nécessaire ici, et son mode de panne
/// — sans ACK, on repousse toujours — rendrait le gain dépendant de la page.
const RAFALE_DE_POUSSEE: u32 = 60;

/// **UN ENFANT PRÊT POUR LA PAGE** — ses métadonnées, et son icône À PART.
///
/// ⛔ **L'ICÔNE NE PART PAS AVEC L'ÉTAT, ET C'EST LA CORRECTION DE FOND DE CE
/// LOT.** Mesuré au journal : **743 Ko par poussée** (gauche 289 Ko, droite
/// 453 Ko), dont ~99 % d'icônes en `data:` base64 — et l'état est repoussé
/// pendant une rafale d'une seconde à chaque changement. La page, elle, JETTE
/// les poussées identiques (garde de révision) : on construisait et transférait
/// 44 Mo pour rien.
///
/// L'icône voyage donc par le canal séparé `window.setIcons`, **une seule fois
/// par chemin**, exactement comme le Dash (`assets/popups/dash.html`,
/// `window.setIcons`, et `cef_dash::load_pending_icons`). L'état retombe à
/// quelques kilo-octets, ce qui rend la rafale gratuite.
///
/// ⚠️ **LA CLÉ EST LE CHEMIN, PAS UN INDEX** — contrairement au Dash, dont les
/// tuiles sont des applications sans identité stable. Ici le chemin est déjà la
/// clé de la page (voir `moitie_json`), donc le cache d'icônes survit à un
/// changement d'ordre ET à une entrée dans un dossier.
///
/// ⭐ **IL NE PORTE PLUS L'ICÔNE DU TOUT** : ni l'image, ni le `data:` URI, ni
/// même la valeur `None` qui dit « ce chemin n'en a pas ». Le calcul a lieu au
/// vidage de la file, et c'est ce qui permet de lister un dossier ENTIER — voir
/// `ICONES_A_ENVOYER`. La valeur `None` reste ENVOYÉE à la page (voir
/// `envoyer_des_icones`) : sans elle, la page redemanderait indéfiniment un
/// chemin qui n'a pas d'icône.
#[derive(Clone)]
struct EnfantPret {
    nom: String,
    /// Sert de CLÉ au cache d'icônes de la page.
    chemin: String,
    dossier: bool,
}

/// **LE CONTENU D'UN DOSSIER, DÉJÀ PRÉPARÉ POUR LA PAGE** — ses enfants (icônes
/// encodées comprises, mais gardées à part) et sa troncature.
#[derive(Clone)]
struct Contenu {
    tronque: bool,
    /// ⚠️ Le cloner recopie les `data:` URI ; ce n'est PAS un coût par poussée,
    /// seulement par changement de dossier (le cache de `contenu` fait le reste).
    enfants: Vec<EnfantPret>,
}

/// **LE DOSSIER PERSONNEL DU DASH DOSSIER, TEL QU'ON LE MONTRE.**
///
/// Les DEUX valeurs sont nécessaires, et elles ne disent pas la même chose :
/// `actif` est ce qui s'affiche, `ecrit` est ce qui a été choisi. Elles
/// divergent dès que le chemin choisi est devenu introuvable — voir
/// `config::sous_dock_personnel_ecrit`, qui porte le pourquoi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DossierPersonnel {
    /// Ce qui s'applique MAINTENANT — toujours un dossier qui existe.
    pub actif: PathBuf,
    /// Ce qui est ÉCRIT dans le réglage, `None` si l'utilisateur n'a jamais
    /// choisi.
    pub ecrit: Option<PathBuf>,
}

/// Le dossier personnel, **RÉSOLU UNE FOIS ET GARDÉ**.
///
/// ⛔ **LE CACHE N'EST PAS UN CONFORT, C'EST CE QUI ÉVITE UN GEL.** Le résoudre
/// demande un `read_to_string` puis un `is_dir()` sur un chemin CHOISI PAR
/// L'UTILISATEUR — donc éventuellement un partage réseau. Or les deux appelants
/// en ont besoin à un rythme qui interdit de le relire :
///
/// - `app::run` l'appelle à CHAQUE OUVERTURE du panneau (un survol de vignette) ;
/// - `cef_settings::push_state` l'appelle **à CHAQUE FRAME** tant que la page
///   Réglages est visible (`cef_settings::poll`, doc de fonction).
///
/// MESURÉ : sur un partage injoignable, `is_dir()` attend le délai SMB — des
/// secondes. Par frame, cela gèle l'application — et le dépôt a déjà payé ce
/// genre de coût par frame (voir `RAFALE_DE_POUSSEE`, qui existe pour la même
/// raison). On résout donc une fois, et `oublier_dossier_personnel` invalide
/// après une écriture du réglage.
pub fn dossier_personnel() -> DossierPersonnel {
    if let Some(garde) = PERSONNEL.with(|p| p.borrow().clone()) {
        return garde;
    }
    let garde = DossierPersonnel {
        actif: crate::config::load_sous_dock_personnel(),
        ecrit: crate::config::sous_dock_personnel_ecrit(),
    };
    PERSONNEL.with(|p| *p.borrow_mut() = Some(garde.clone()));
    garde
}

/// **OUBLIE CE QU'ON SAIT DU RÉGLAGE** — à appeler après l'avoir écrit, sinon la
/// page Réglages montrerait l'ancienne valeur jusqu'au redémarrage.
pub fn oublier_dossier_personnel() {
    PERSONNEL.with(|p| *p.borrow_mut() = None);
}

/// **UNE MOITIÉ DU DASH DOSSIER.**
///
/// ⚠️ **LE `json` EST CONSTRUIT AU CHANGEMENT, JAMAIS PAR FRAME** : c'est le
/// même contrat que l'ancien `ETAT_JSON`, appliqué à chacune des deux moitiés.
struct Moitie {
    /// Le dossier montré. Vide tant que la moitié n'a jamais été ouverte.
    dossier: PathBuf,
    /// SON ordre — les deux moitiés ne trient pas forcément pareil, et le
    /// fichier de réglage en porte deux (voir `config::load_sous_dock_tris`).
    tri: Tri,
    /// **LE COMPTEUR QUE LA PAGE COMPARE.** Il change à chaque fois que CETTE
    /// moitié change, et lui seul : sans lui, changer la gauche ferait
    /// reconstruire la droite — et sa recherche repartirait à zéro, ce qui est
    /// précisément le défaut que la page documente (« la recherche repart à
    /// zéro à chaque changement de dossier »).
    revision: u64,
    /// L'objet JSON de cette moitié, déjà sérialisé.
    json: String,
}

impl Moitie {
    const fn neuve() -> Self {
        Moitie { dossier: PathBuf::new(), tri: Tri::Nom, revision: 0, json: String::new() }
    }
}

/// **LE SEUL ACCÈS À UNE MOITIÉ.** Voir `MOITIES`, où le pourquoi est écrit.
///
/// ⚠️ **ON N'EMPRUNTE JAMAIS À TRAVERS UN APPEL AU SHELL.** `contenu` lit le
/// disque, et `icone_du_chemin` appelle `SHGetFileInfoW` ; l'emprunt est pris et
/// relâché autour de la seule mise à jour des champs. Le dépôt a un précédent de
/// réentrance `RefCell` dans un wndproc — l'emprunt tenu pendant un appel Win32
/// est exactement la forme à ne pas écrire.
fn avec<R>(cote: Cote, f: impl FnOnce(&mut Moitie) -> R) -> R {
    MOITIES.with(|m| f(&mut m.borrow_mut()[cote.index()]))
}

/// **L'ORDRE ACTUEL D'UNE MOITIÉ** — pour ce qui range des fichiers HORS du
/// panneau. La visionneuse et le lecteur de vidéos ont une flèche de voisinage, et
/// elle doit suivre l'ordre que l'œil vient de voir à l'écran : le panneau se
/// referme à l'ouverture, donc c'est ici que l'ordre se lit, pas dans une liste que
/// la fenêtre n'a plus.
pub fn tri_de_la_moitie(cote: Cote) -> Tri {
    avec(cote, |moitie| moitie.tri)
}

/// Ce que la page a demandé — vidé par `dispatch_actions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Demande {
    /// Entrer dans un sous-dossier : la moitié `cote` se ROUVRE sur lui, en
    /// gardant sa place à l'écran. La remontée se fait par `Remonter`.
    ///
    /// ⛔ **IL Y AVAIT ICI UN VERBE `Voir`, QUI FAISAIT DE LA NAPPE SURVOLÉE LE
    /// DOSSIER COURANT. IL A ÉTÉ RETIRÉ — DÉCISION D'USAGE.** La page le
    /// produisait au bout de 150 ms d'arrêt, et le retour a été net : « **je
    /// n'aime pas que les dossiers s'ouvrent tout seul au survol** ». On
    /// s'arrête tout le temps sans le vouloir, et le dossier changeait sous la
    /// souris. Le survol n'éclaire plus que la case (voir `installerSurvol` dans
    /// la page) ; **un dossier s'ouvre au clic**, comme dans l'Explorateur.
    Entrer(Cote, PathBuf),
    /// Lancer un fichier (ou ouvrir un dossier) avec le shell. **Pas de côté** :
    /// le shell ouvre, c'est tout — la moitié d'où vient le clic n'y change rien.
    Ouvrir(PathBuf),
    /// Remonter au dossier PARENT — sans quoi un dash dossier n'est qu'un
    /// entonnoir : on entre et on ne ressort pas.
    Remonter(Cote),
    /// Relire le dossier de cette moitié SANS rien changer d'autre — le geste qui
    /// suit une modification faite ailleurs (un fichier ajouté, une compilation
    /// qui vient de produire un binaire).
    Actualiser(Cote),
    /// Ouvrir le dossier AFFICHÉ de cette moitié dans l'Explorateur Windows.
    ///
    /// ⚠️ **C'est le MÊME geste que `Ouvrir` sur un dossier**, pas un second
    /// chemin : `Demande::Ouvrir` remet déjà un dossier au shell (voir
    /// `app::run`), donc la seule différence est QUE l'on ouvre — le dossier
    /// courant au lieu d'un enfant.
    Explorateur(Cote),
    /// Changer l'ordre des enfants de cette moitié — **et le RETENIR** : les deux
    /// `Tri` sont persistés (voir `config::save_sous_dock_tris`), sans quoi le
    /// choix repartirait à « nom » au lancement suivant sans que personne ne s'en
    /// aperçoive.
    Trier(Cote, Tri),
    /// RETIRER de la barre du bas le dossier de cette moitié.
    ///
    /// ⛔ **IL EXISTE PARCE QUE LE BOUTON EN AVAIT L'AIR.** Épingler était sans
    /// retour : le bouton affichait `✓` et un état « appuyé », donc il se lisait
    /// comme une BASCULE — et un contrôle qui ne fait pas ce qu'il a l'air de faire
    /// est pire qu'un contrôle absent. Signalé par l'utilisateur : « je ne peux pas
    /// désépingler ».
    Desepingler(Cote),
    /// Épingler le dossier de cette moitié à la barre du bas.
    ///
    /// ⚠️ **LE CÔTÉ COMPTE, ET C'EST UNE CORRECTION** : avec deux moitiés, « le
    /// dossier affiché » ne désigne plus rien. Le `+` de la moitié droite doit
    /// épingler LE DOSSIER DE LA MOITIÉ DROITE, même si la gauche montre autre
    /// chose.
    ///
    /// ⚠️ **L'EFFET EST IMMÉDIAT — ET CE COMMENTAIRE A DÛ ÊTRE CORRIGÉ POUR LE
    /// DIRE.** Il affirmait « sans redémarrage, et c'est un choix : le raccourci
    /// n'APPARAÎT sur la barre qu'au prochain lancement ». C'était vrai avant
    /// l'étape 4 du refactor des identités stables, et ça ne l'est plus :
    /// `DossiersVivants` insère l'icône au bon rang, décale les plages qui
    /// suivent, et le rendu de la MÊME frame la peint (voir
    /// `app::basculer_epinglage_du_dossier_affiche`). Le lot 1 a recopié ce
    /// paragraphe en même temps que la page, alors qu'il aurait fallu le RELIRE
    /// — d'où cette réécriture.
    ///
    /// ⚠️ Il citait aussi `dossier_icon_base`, que l'étape 2 a supprimé : une doc
    /// qui nomme un symbole disparu envoie le prochain lecteur chercher ce qu'il
    /// ne trouvera pas.
    Epingler(Cote),
    // ---- CE QUE LE MENU CONTEXTUEL D'UN FICHIER DEMANDE --------------------
    //
    // ⛔ **CES HUIT GESTES SONT NEUFS DANS LE DÉPÔT, ET C'EST POURQUOI ILS SONT
    // ICI PLUTÔT QU'AILLEURS.** Le module sait DÉJÀ quel dossier chaque moitié
    // montre : c'est lui qui peut dire à l'appelant quoi rafraîchir après coup,
    // sans que la page ait à le lui renvoyer (et donc à pouvoir le lui
    // renvoyer FAUX).
    //
    // ⚠️ **LE CÔTÉ EST SUR CHAQUE GESTE**, même quand il ne sert pas à l'action
    // elle-même : c'est lui qui dit QUELLE MOITIÉ relire et où écrire le
    // compte rendu. Sans lui, un renommage à droite rafraîchirait la gauche.
    // ⚠️ **IL N'Y AVAIT PAS D'`Afficher` ICI, ET C'EST LE TEST DE CONTRAT QUI
    // L'AVAIT DÉCIDÉ** : le verbe avait été écrit d'avance pour la visionneuse,
    // mais la page ne l'émettait pas encore — et
    // `les_verbes_de_la_page_sont_exactement_ceux_du_dispatch` a refusé, à juste
    // titre : un verbe que rien n'émet est du code mort. Il revient ici AVEC son
    // émetteur, dans le même changement.
    /// Afficher le fichier dans une visionneuse (texte ou image).
    Afficher(Cote, PathBuf),
    /// ⭐ « Voir l'aide » À LA PLACE D'« AFFICHER » sur une vidéo, VLC absent —
    /// DEMANDE EXPLICITE (2026-10-19) : « ne pas avoir Afficher dans le menu du
    /// clic droit sur une vidéo si VLC n'est pas installé, mais écrire Voir
    /// l'aide et renvoyer à l'aide expliquant comment installer VLC ». Ouvre la
    /// section `vlc` de l'aide (voir `win32::cef_aide::demander`).
    AideVideo,
    /// Confier le fichier au dialogue « Ouvrir avec… » de Windows.
    OuvrirAvec(Cote, PathBuf),
    /// Mettre le chemin dans le presse-papiers, en texte.
    CopierChemin(Cote, PathBuf),
    /// Ouvrir la feuille de propriétés de Windows.
    Proprietes(Cote, PathBuf),
    /// Mettre les fichiers dans le presse-papiers de FICHIERS (`CF_HDROP`) —
    /// PLUSIEURS depuis la sélection multiple (DEMANDE EXPLICITE, 2026-10-11) :
    /// la page les envoie dans UN segment, séparés par des sauts de ligne
    /// (aucun chemin Windows ne peut en contenir).
    CopierFichier(Cote, Vec<PathBuf>),
    /// Comme ci-dessus, mais le destinataire DÉPLACERA au lieu de recopier.
    CouperFichier(Cote, Vec<PathBuf>),
    /// Renommer — **deux arguments** : le fichier et le nouveau nom.
    Renommer(Cote, PathBuf, String),
    /// Supprimer, **vers la corbeille** — jamais définitivement.
    Supprimer(Cote, PathBuf),
    // ---- CE QUE LE MENU CONTEXTUEL DU VIDE DE LA GRILLE DEMANDE -------------
    //
    // ⚠️ **L'ARGUMENT EST UN NOM, PAS UN CHEMIN** : le dossier où créer est
    // celui que la moitié AFFICHE, et Rust le sait déjà (`dossier_affiche`). Le
    // faire revenir par la page, c'est lui permettre de revenir FAUX — même
    // raisonnement qu'`Actualiser`.
    /// Créer un dossier dans le dossier affiché de cette moitié.
    NouveauDossier(Cote, String),
    /// Créer un fichier VIDE dans le dossier affiché de cette moitié.
    NouveauFichier(Cote, String),
    /// **COLLER** les fichiers et dossiers du presse-papiers — DEMANDE EXPLICITE
    /// (2026-10-11) : « il est impossible de copier coller des dossiers dans le
    /// dash dossiers ». Dans le dossier désigné (clic droit sur un dossier), ou
    /// à défaut dans celui que la moitié affiche (clic droit dans le vide).
    Coller(Cote, Option<PathBuf>),
}

/// **LE SIGNAL DE VIE DE LA PAGE — ET IL N'EST PAS UN VERBE.**
///
/// La page l'envoie en DERNIÈRE ligne de son `<script>` (`go("ready")`), à
/// l'instant où `window.setIcons` vient d'exister. C'est le seul moment où Rust
/// peut savoir que ce qu'il envoie sera REÇU.
///
/// ⛔ **SANS LUI, DES ICÔNES SONT PERDUES POUR DE BON.** Rust envoie ses lots dès
/// la première frame, mais une page FROIDE n'a pas encore exécuté son script :
/// l'expression lève, personne ne la rattrape, et `ICONES_ENVOYEES` a déjà marqué
/// ces chemins comme partis — ils ne repartiront JAMAIS. Mesuré sur le harnais,
/// ouverture à la frame 0 : les premières cases de la moitié droite restaient en
/// carré pointillé ; à la frame 60 (page préchargée) tout était là.
///
/// ⚠️ **CE N'EST PAS UN VERBE, ET IL NE DOIT PAS ENTRER DANS `VERBES`** : il ne
/// porte NI côté NI argument, et il n'est pas DÉCODÉ en `Demande` — il ne
/// déclenche aucun geste, il ouvre une porte. `VERBES` sert au contrat
/// page↔dispatch (chaque verbe déclaré doit être décodé ET employé), contrat
/// auquel ce signal ne participe pas.
///
/// ⚠️ **CE QU'IL DÉCLENCHE : UN RENVOI, PAS UNE PORTE** — voir
/// `remettre_tout_en_file`. À la réception, on redemande les icônes des deux
/// moitiés affichées. C'est ce qui répare les lots partis trop tôt, sans jamais
/// pouvoir en supprimer : si ce signal n'arrive pas, tout se comporte comme avant.
///
/// ⚠️ **LA PAGE EST PRÉCHARGÉE (`cef_popup::preload`), DONC ELLE SE SIGNALE
/// SOUVENT AVANT LA PREMIÈRE OUVERTURE.** Le renvoi ne trouve alors aucune moitié
/// ouverte et ne fait rien — et c'est correct : la page était prête, donc rien
/// n'a été perdu.
pub const SIGNAL_PRET: &str = "ready";

/// **LE ZOOM DU PANNEAU** — `sous-dock/zoom/<niveau>`. Comme `SIGNAL_PRET`, il ne
/// porte NI côté NI `Demande` : il vaut pour les deux moitiés, et il se traite ici
/// (voir `changer_le_zoom`).
pub const SIGNAL_ZOOM: &str = "zoom";

/// **LA LISTE DES VERBES QUE LA PAGE A LE DROIT D'ENVOYER — LA SEULE.**
///
/// ⛔ **ELLE EST VÉRIFIÉE DANS LES DEUX SENS PAR UN TEST**, parce que le dépôt a
/// déjà été mordu par un verbe ajouté d'un seul côté (voir
/// `cef_surveillance::la_page_et_le_dispatch_acceptent_les_memes_verbes`) :
/// `les_verbes_de_la_page_sont_exactement_ceux_du_dispatch` exige que celui-ci
/// les décode TOUS, et que la page les emploie TOUS.
pub const VERBES: [&str; 20] = [
    "entrer",
    "ouvrir",
    "remonter",
    "actualiser",
    "explorateur",
    "trier",
    "epingler",
    "desepingler",
    // Les gestes du menu contextuel d'un fichier — voir `Demande`.
    "afficher",
    "aide-video",
    "ouvrir-avec",
    "copier-chemin",
    "proprietes",
    "copier",
    "couper",
    "renommer",
    "supprimer",
    // Les gestes du menu contextuel du VIDE de la grille.
    "nouveau-dossier",
    "nouveau-fichier",
    "coller",
];

pub fn preload(gfx: &GfxDevice) {
    // **LES DEUX TRIS SONT RELUS ICI, ET NULLE PART AILLEURS.** Le préchargement
    // est le seul moment garanti avant toute ouverture (voir `app::run`, qui
    // précharge les treize popups au démarrage) : les lire à la première
    // ouverture marcherait aussi, mais laisserait le module dans un état « pas
    // encore chargé » que chaque appelant devrait connaître.
    let tris = crate::config::load_sous_dock_tris();
    for cote in Cote::TOUTES {
        avec(cote, |m| m.tri = tris[cote.index()]);
    }
    // Le souvenir du dernier dossier de gauche, relu ici pour la même raison —
    // et gardé, afin de ne pas réécrire le même chemin à chaque changement.
    GAUCHE_MEMORISEE.with(|g| *g.borrow_mut() = crate::config::load_sous_dock_gauche());
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// **APPLIQUE UN TRI DÉJÀ VALIDÉ PAR LA PAGE** : il devient le tri de cette
/// moitié, il est ÉCRIT SUR LE DISQUE, et son dossier est relu dans le nouvel
/// ordre.
///
/// ⚠️ **LA SAUVEGARDE A LIEU ICI, PAS À LA FERMETURE** : l'application peut être
/// terminée par un arrêt de session, qui ne passe par aucun chemin de fermeture.
/// C'est la même règle que `config::save_dossiers`, écrit AVANT le redémarrage.
///
/// ⚠️ **ON ÉCRIT LES DEUX MOITIÉS À LA FOIS** (voir `save_sous_dock_tris`) : un
/// écrivain par côté écraserait la ligne de l'autre.
pub fn set_tri(cote: Cote, tri: Tri) {
    avec(cote, |m| m.tri = tri);
    let mut tris = [Tri::default(); 2];
    for c in Cote::TOUTES {
        tris[c.index()] = avec(c, |m| m.tri);
    }
    crate::config::save_sous_dock_tris(tris);
    if let Some(dossier) = dossier_affiche(cote) {
        // ⚠️ **`rafraichir` ET NON `changer`** : `changer` met à jour la moitié
        // mais NE POUSSE PAS (c'est `rafraichir` qui appelle
        // `pousser_tout_de_suite`). Le confondre ferait changer l'ordre côté
        // Rust sans que la page le montre — un bouton de tri qui ne trie rien à
        // l'écran, et qui se remettrait d'aplomb au changement de dossier
        // suivant, ce qui rendrait le défaut très difficile à croire.
        rafraichir(cote, &dossier);
    }
}

/// **OUVRE LE DASH DOSSIER SUR SES DEUX DOSSIERS.**
///
/// ⚠️ **`rect` EST LA ZONE LIBRE DU DOCK (`dash_rect`), PAS LA CASE DE LA
/// VIGNETTE** — et c'est la correction qui a débloqué ce panneau : ouvert sur sa
/// vignette (64 px), il vivait dans une fenêtre dont la taille devait être
/// négociée, et sa page n'exécutait jamais son script. Voir `SPEC` et
/// `render_if_visible`, qui portent la mesure.
///
/// `gauche` est le dossier survolé dans le dock, s'il y en a un. **`None` n'est
/// pas un cas d'erreur** : la gauche reprend alors le DERNIER dossier qu'elle a
/// montré (voir `config::load_sous_dock_gauche`), et à défaut le dossier
/// personnel — c'est la demande de l'utilisateur. ⚠️ **Ce cas n'a aujourd'hui
/// aucun appelant** : `app::run` ouvre toujours le panneau au survol d'un
/// dossier, donc toujours avec un chemin. La branche existe pour l'entrée qui
/// viendra, elle est TESTÉE, mais elle n'est pas exercée en service.
///
/// `droite` est le dossier personnel — **le DÉFAUT de la moitié droite, pas sa
/// destination**. Voir `dossier_de_droite` : elle ne le prend qu'à sa toute
/// première ouverture, puis elle garde le dossier où l'utilisateur l'a laissée.
///
/// ⭐ **SANS ICÔNE, UN DASH DOSSIER N'EST QU'UNE LISTE DE NOMS** — et c'est
/// l'icône qui fait qu'on reconnaît ce qu'on cherche. Toutes les entrées finissent
/// donc par en recevoir une ; elles arrivent simplement APRÈS l'affichage, par
/// lots, au lieu d'être calculées toutes ensemble avant lui. Voir
/// `ICONES_A_ENVOYER` : c'est ce délai qui permet de montrer un dossier entier.
pub fn ouvrir(rect: [i32; 4], gfx: &GfxDevice, gauche: Option<&Path>, droite: &Path) {
    let gauche = gauche
        .map(Path::to_path_buf)
        .or_else(crate::config::load_sous_dock_gauche)
        .unwrap_or_else(|| droite.to_path_buf());
    // ⛔ **LE NUMÉRO D'OUVERTURE NE BOUGE QU'ICI** — voir `OPEN_COUNT`.
    OPEN_COUNT.with(|c| c.set(c.get() + 1));
    // ⛔ **ET LE CARNET D'ICÔNES SE VIDE ICI, PAS AILLEURS.** La page efface son
    // cache d'icônes au même instant (elle le fait sur le changement de `open`,
    // voir `window.setState`) : les deux côtés repartent donc ensemble, ce qui
    // rattrape une page qui aurait rechargé. Les vidage et remplissage sont dans
    // cet ordre — vider, PUIS `changer` qui remet en file — sans quoi les icônes
    // des deux moitiés seraient perdues pour toute l'ouverture.
    ICONES_A_ENVOYER.with(|file| file.borrow_mut().clear());
    ICONES_ENVOYEES.with(|envoyees| envoyees.borrow_mut().clear());
    changer(Cote::Gauche, &gauche);
    memoriser_gauche(&gauche);
    // ⚠️ **LA MOITIÉ DROITE EST RECONSTRUITE À CHAQUE OUVERTURE, MÊME QUAND ELLE
    // GARDE SON DOSSIER** — parce que son état porte aussi `epingle`, relu du
    // fichier, et que la barre du bas a pu changer pendant que le panneau était
    // fermé. Le dossier, lui, ne bouge que la première fois.
    let droite = dossier_de_droite(&avec(Cote::Droite, |m| m.dossier.clone()), droite);
    changer(Cote::Droite, &droite);
    // ⛔ **UN PANNEAU DÉJÀ OUVERT SE REMPLIT, IL NE SE BASCULE PAS** — voir
    // `doit_basculer`, qui porte le défaut mesuré au long. C'est le SEUL `toggle` de
    // cette fonction, donc le seul endroit d'où le panneau peut se refermer ici.
    //
    // ⚠️ **ON NE SAUTE QUE LE `toggle`, PAS LE RESTE** : les deux moitiés ont déjà
    // été rechargées plus haut (gauche sur le dossier survolé, droite sur le sien).
    // Le panneau reste donc ouvert ET suit le survol — ce qu'on veut dans tous les
    // cas, épinglé ou non. `pousser_tout_de_suite` juste en dessous est ce qui fait
    // voir le nouveau dossier.
    //
    // ⚠️ **LE RECTANGLE EST LE MÊME POUR TOUS LES DOSSIERS** (`dash_rect`, calculé
    // sur la zone libre du dock, jamais sur la case survolée) : sauter le `toggle`
    // ne prive donc d'aucun repositionnement.
    // ⛔ **SON ZOOM DOIT SUIVRE LE DPI**, et c'est le cas le plus exposé des trois :
    // ce panneau s'ouvre près du dock CLIQUÉ, donc sur un écran secondaire dès
    // qu'on s'en sert là. ⚠️ Sa page porte pourtant `--text-scale` : la raison
    // n'est pas qu'il en manque, c'est que **rien ne l'alimente** — ce module ne
    // pousse aucun `contentScale`. Voir `cef_popup::regler_zoom_dpi`.
    // ⚠️ L'appel est HORS du `if` : le drapeau doit
    // être posé même quand la bascule ne fait que REFERMER, sinon un sous-dock
    // rouvert après coup garderait la valeur de la fois précédente.
    cef_popup::regler_zoom_dpi(&SLOT, true);
    if doit_basculer(cef_popup::is_visible(&SLOT)) {
        cef_popup::toggle(&SLOT, &SPEC, rect, gfx);
    }
    pousser_tout_de_suite();
}

/// **LE DOSSIER QUE PREND LA MOITIÉ DROITE À UNE OUVERTURE — pur, et c'est
/// pour ça qu'il est extrait.**
///
/// `ouvrir` crée une vraie fenêtre CEF et ne se teste pas ; la seule DÉCISION
/// qu'il prend pour la droite tient dans ces trois lignes, et c'est celle que
/// l'utilisateur a tranchée : **la droite garde sa position**.
///
/// ⚠️ **LE DOSSIER PERSONNEL EST UN DÉFAUT, PAS UNE DESTINATION.** La remettre
/// dessus à chaque ouverture ferait qu'un survol de passage dans la barre du bas
/// déciderait de ce qu'elle montre la fois suivante — et l'utilisateur perdrait
/// la navigation qu'il venait de faire à droite en revenant voir à gauche.
///
/// ⚠️ **CONSÉQUENCE À DIRE : un changement du réglage ne se voit donc qu'au
/// LANCEMENT SUIVANT** — c'est le seul moment où la droite n'a pas encore de
/// position. La page Réglages le dit, elle ; un réglage dont l'effet n'apparaît
/// pas se lit comme un réglage qui n'a pas marché.
fn dossier_de_droite(actuel: &Path, defaut: &Path) -> PathBuf {
    if actuel.as_os_str().is_empty() {
        defaut.to_path_buf()
    } else {
        actuel.to_path_buf()
    }
}

/// Recharge le dossier d'UNE moitié SANS déplacer la fenêtre — pour « remonter »,
/// pour « entrer », ou après un changement de contenu. `cef_popup::toggle` sur le
/// même rectangle ferait clignoter le panneau ; ici on ne fait que remplacer
/// l'état.
pub fn rafraichir(cote: Cote, dossier: &Path) {
    changer(cote, dossier);
    // ⚠️ **LE SOUVENIR S'ÉCRIT ICI, PAS DANS `changer`.** `changer` est appelé
    // par un test, et un test qui écrit dans `%APPDATA%` abîmerait le vrai
    // réglage de l'utilisateur — c'est la même discipline que les « cœurs purs »
    // de `config.rs`, extraits exprès pour ne pas toucher au fichier réel.
    if cote == Cote::Gauche {
        memoriser_gauche(dossier);
    }
    pousser_tout_de_suite();
}

/// Le corps commun d'`ouvrir`, de `rafraichir` et de `set_tri` : la moitié prend
/// ce dossier, sa révision avance, et son JSON est reconstruit.
///
/// ⚠️ **IL N'ÉCRIT RIEN SUR LE DISQUE** — voir `rafraichir`.
fn changer(cote: Cote, dossier: &Path) {
    // ⚠️ **CHANGER DE DOSSIER EFFACE LE COMPTE RENDU** : « « x.txt » existe déjà »
    // n'a plus rien à dire une fois qu'on est ailleurs. C'est la SEULE différence
    // avec `changer_sans_effacer`, que `signaler` emploie.
    MESSAGE.with(|m| m.borrow_mut()[cote.index()] = None);
    changer_sans_effacer(cote, dossier);
}

/// Le corps commun d'`ouvrir`, de `rafraichir` et de `set_tri` : la moitié prend
/// ce dossier, sa révision avance, et son JSON est reconstruit.
///
/// ⚠️ **IL N'ÉCRIT RIEN SUR LE DISQUE** — voir `rafraichir`.
fn changer_sans_effacer(cote: Cote, dossier: &Path) {
    let elements = crate::config::load_barre_du_bas();
    // ⚠️ **À LIRE AVANT LA MISE À JOUR** : c'est ce qui distingue « on entre
    // ailleurs » de « ce dossier a changé sur le disque » (`signaler`, `Actualiser`).
    // Les deux passent ici, et seul le premier doit rebâtir la file d'icônes.
    let (ancien, tri) = avec(cote, |m| (m.dossier.clone(), m.tri));
    let change_de_dossier = ancien.as_path() != dossier;
    // ⛔ **ON QUITTE UN DOSSIER : ON LÈVE LES MARQUES DE SES ICÔNES.** Sans ça, y
    // revenir le montrerait SANS AUCUNE ICÔNE — la page les a élaguées en même
    // temps que l'état cessait de les afficher, et le carnet les croyait en place.
    // Voir `oublier_les_marques`, où le défaut signalé est décrit en entier.
    //
    // ⚠️ **LE `Contenu` DE L'ANCIEN DOSSIER SE RELIT PAR `contenu`** — il est en
    // cache (on vient de l'y mettre), ce n'est pas une relecture du disque.
    if change_de_dossier && !ancien.as_os_str().is_empty() {
        oublier_les_marques(&contenu(&ancien, tri));
    }
    let contenu = contenu(dossier, tri);
    let message = MESSAGE.with(|m| m.borrow()[cote.index()].clone());
    avec(cote, |m| {
        m.dossier = dossier.to_path_buf();
        m.revision = m.revision.wrapping_add(1);
        m.json = moitie_json(m, &contenu, &elements, message.as_deref());
    });
    // ⭐ **LA FILE EST (RE)BÂTIE APRÈS LA MISE À JOUR**, donc quand `m.dossier` est
    // bien le nouveau. Changer de dossier **jette** les icônes encore en attente et
    // remet celles des DEUX moitiés affichées ; relire le même dossier ne fait
    // qu'ajouter ce qui manque, sans jeter ce qui est déjà en route.
    if change_de_dossier {
        remettre_les_icones_en_file(cote);
    } else {
        mettre_en_file_les_icones(&contenu);
    }
}

/// **REBÂTIT LA FILE D'ICÔNES POUR LES DEUX MOITIÉS** — appelée quand une moitié
/// change de dossier.
///
/// ⚠️ **POURQUOI LES DEUX, ET PAS SEULEMENT CELLE QUI CHANGE** : la file est
/// COMMUNE aux deux moitiés. La vider pour jeter les icônes du dossier quitté
/// jetterait du même coup celles que l'AUTRE moitié attend encore — et comme
/// `oublier_la_file_d_icones` les démarque, plus rien ne les remettrait en file :
/// cette moitié resterait en carrés pointillés jusqu'à ce qu'elle change à son
/// tour. On remet donc les deux, systématiquement.
///
/// `prioritaire` passe EN PREMIER : c'est la moitié qu'on vient de manipuler,
/// donc celle qu'on regarde. L'autre suit.
///
/// ⚠️ **LE `Contenu` DE L'AUTRE MOITIÉ SE RELIT PAR `contenu`**, mais il est en
/// cache (`CACHE_MAX`) : ce n'est pas une relecture du disque.
fn remettre_les_icones_en_file(prioritaire: Cote) {
    oublier_la_file_d_icones();
    for cote in [
        prioritaire,
        Cote::TOUTES.into_iter().find(|c| *c != prioritaire).unwrap_or(prioritaire),
    ] {
        // ⛔ **L'EMPRUNT EST RELÂCHÉ AVANT `contenu`** — il appelle le shell, et
        // c'est la discipline que `avec` documente.
        let (dossier, tri) = avec(cote, |m| (m.dossier.clone(), m.tri));
        if dossier.as_os_str().is_empty() {
            continue; // Une moitié jamais ouverte n'a rien à iconifier.
        }
        mettre_en_file_les_icones(&contenu(&dossier, tri));
    }
}

/// **DIT CE QU'UN GESTE A DONNÉ** — un renommage refusé, une suppression qui n'a
/// pas eu lieu, un presse-papiers qu'une autre application tenait.
///
/// ⛔ **SANS ÇA, UN ÉCHEC EST UN NON-ÉVÉNEMENT.** Le dépôt refuse les échecs
/// silencieux, et c'est ici que ça coûterait le plus cher : un renommage qui n'a
/// pas eu lieu se lirait comme un renommage qui a mal pris, et l'utilisateur
/// recommencerait — ou pire, croirait que le fichier a été supprimé.
///
/// ⚠️ **LE MESSAGE EST POUSSÉ IMMÉDIATEMENT**, par une reconstruction de la
/// moitié : c'est le seul chemin qui fait partir le JSON. Le contenu, lui, est
/// déjà en cache — ce n'est pas une relecture du disque.
pub fn signaler(cote: Cote, texte: impl Into<String>) {
    let texte = texte.into();
    // Le journal d'abord : c'est là qu'on lit ce qui s'est passé APRÈS coup.
    crate::diag!("[sous-dock] geste {} : {texte}", cote.cle());
    MESSAGE.with(|m| m.borrow_mut()[cote.index()] = Some(texte));
    if let Some(dossier) = dossier_affiche(cote) {
        changer_sans_effacer(cote, &dossier);
        pousser_tout_de_suite();
    }
}

/// **MET EN FILE LES CHEMINS DE CE DOSSIER DONT L'ICÔNE RESTE À FAIRE.**
///
/// ⚠️ **LE CARNET EST TENU ICI, PAS DANS LA PAGE** : c'est le seul endroit d'où
/// l'on sait ce qui est DÉJÀ en file ou parti. Demander à la page ce qu'elle a
/// serait un aller-retour par frame pour une information qu'on a sous la main.
///
/// ⛔ **LE CHEMIN EST MARQUÉ ICI, À L'EMPILEMENT — PAS À L'ENVOI.** C'est ce qui
/// empêche d'empiler deux fois le même chemin quand les deux moitiés montrent le
/// même dossier. La contrepartie est écrite dans `ICONES_ENVOYEES` : **vider la
/// file oblige à démarquer**, et c'est le seul travail d'`oublier_la_file_d_icones`.
///
/// ⚠️ **AUCUN CALCUL ICI** : on n'empile que des chemins. L'extraction du shell,
/// le redimensionnement et l'encodage ont lieu au vidage (voir
/// `envoyer_des_icones`), et c'est ce qui rend une liste de 5 000 entrées
/// supportable à l'ouverture.
fn mettre_en_file_les_icones(contenu: &Contenu) {
    ICONES_A_ENVOYER.with(|file| {
        let mut file = file.borrow_mut();
        ICONES_ENVOYEES.with(|envoyees| {
            let mut envoyees = envoyees.borrow_mut();
            for enfant in &contenu.enfants {
                if envoyees.insert(enfant.chemin.clone()) {
                    file.push_back(enfant.chemin.clone());
                }
            }
        });
    });
}

/// **OUBLIE LA FILE D'ICÔNES — LA VIDE *ET* DÉMARQUE SES CHEMINS.**
///
/// ⛔ **LES DEUX ENSEMBLE, JAMAIS L'UN SANS L'AUTRE.** `ICONES_ENVOYEES` marque un
/// chemin à l'empilement, pas à l'envoi (voir `mettre_en_file_les_icones`) : vider
/// la file en laissant les marques ferait tenir pour « déjà partie » une icône qui
/// n'est jamais partie, et **la page garderait un carré en pointillés jusqu'à la
/// prochaine ouverture, sans un mot**. C'est précisément le genre de défaut qu'on
/// ne sait plus attribuer à sa cause une fois qu'il est là.
///
/// ⚠️ **POURQUOI VIDER, ALORS QU'ON NE LE FAISAIT QU'À L'OUVERTURE** : depuis que
/// l'icône se calcule au vidage, la file d'un dossier quitté coûterait du CALCUL,
/// plus seulement des octets. Sortir d'un dossier de 5 000 entrées laisserait une
/// seconde et demie d'extractions inutiles devant les icônes du dossier où l'on
/// vient d'entrer — visible à l'écran, et incompréhensible.
fn oublier_la_file_d_icones() {
    ICONES_A_ENVOYER.with(|file| {
        let mut file = file.borrow_mut();
        ICONES_ENVOYEES.with(|envoyees| {
            let mut envoyees = envoyees.borrow_mut();
            for chemin in file.drain(..) {
                envoyees.remove(&chemin);
            }
        });
    });
}

/// **RENVOIE LES ICÔNES DES DEUX MOITIÉS AFFICHÉES** — appelée quand la page se
/// signale vivante (voir `SIGNAL_PRET`).
///
/// ⛔ **C'EST UNE RÉPARATION, PAS UNE OPTIMISATION.** Tout lot envoyé avant que la
/// page n'existe est perdu au niveau JS : l'expression lève, `execute_js` ne rend
/// aucun compte, et `ICONES_ENVOYEES` a déjà marqué ces chemins comme partis —
/// donc ils ne repartiraient JAMAIS. Mesuré sur le harnais, ouverture à la frame 0
/// (page froide) : les premières cases de la moitié droite restaient en carré
/// pointillé. Ici on lève la marque des chemins affichés et on les remet en file :
/// ils repartent, maintenant que `window.setIcons` existe pour les recevoir.
///
/// ⚠️ **POURQUOI UN RENVOI PLUTÔT QU'UNE PORTE QUI BLOQUE.** Une porte — ne rien
/// envoyer tant que la page ne s'est pas signalée — a été écrite d'abord, puis
/// RETIRÉE. Elle rendait le défaut pire quand le signal manque : plus AUCUNE icône
/// ne part, et rien ne le dit. Un renvoi ne peut rien casser — sans signal, le
/// comportement reste exactement celui d'avant.
///
/// ⚠️ **LE RENVOI EST SANS EFFET VISIBLE QUAND TOUT VA BIEN** : les icônes
/// redéposées sont identiques, et la page ne retouche pas une image dont la source
/// n'a pas changé (voir `poserLeVisuel`).
fn remettre_tout_en_file() {
    for cote in Cote::TOUTES {
        // ⛔ **L'EMPRUNT EST RELÂCHÉ AVANT `contenu`** — il appelle le disque.
        let (dossier, tri) = avec(cote, |m| (m.dossier.clone(), m.tri));
        if dossier.as_os_str().is_empty() {
            continue; // Une moitié jamais ouverte n'a rien à renvoyer.
        }
        let contenu = contenu(&dossier, tri);
        // ⛔ **ON DÉMARQUE D'ABORD, SINON LE RENVOI NE RENVOIE RIEN.**
        // `mettre_en_file_les_icones` saute tout chemin déjà marqué — c'est ce qui
        // l'empêche d'empiler deux fois le même, et ici c'est exactement la marque
        // qu'il faut lever.
        oublier_les_marques(&contenu);
        mettre_en_file_les_icones(&contenu);
    }
}

/// **LÈVE LA MARQUE DES ICÔNES DE CE CONTENU** — elles pourront repartir.
///
/// ⛔ **C'EST LE PENDANT EXACT DE L'ÉLAGAGE DE LA PAGE, ET IL A FALLU UN DÉFAUT
/// SIGNALÉ POUR LE VOIR.** La page ne garde que les icônes des chemins que l'état
/// AFFICHE (`elaguerLesIcones`, dans `sous_dock.html`) ; ce carnet, lui, n'oubliait
/// jamais rien. Les deux disaient donc des choses différentes dès qu'on quittait un
/// dossier : la page jetait ses icônes, et Rust, les croyant toujours en place,
/// refusait de les renvoyer (`mettre_en_file_les_icones` saute tout chemin marqué).
/// **Revenir dans un dossier déjà visité le montrait donc SANS AUCUNE ICÔNE** —
/// « comme si elles étaient transparentes ». Reproduction : ouvrir un dossier,
/// entrer dans un sous-dossier, remonter.
///
/// ⚠️ **ON NE LÈVE QUE LES MARQUES DU DOSSIER QU'ON QUITTE**, jamais celles des
/// deux moitiés : la page garde tout ce que l'état affiche encore, donc renvoyer
/// l'autre moitié serait du travail perdu à chaque pas dans l'arborescence.
fn oublier_les_marques(contenu: &Contenu) {
    ICONES_ENVOYEES.with(|envoyees| {
        let mut envoyees = envoyees.borrow_mut();
        for enfant in &contenu.enfants {
            envoyees.remove(&enfant.chemin);
        }
    });
}

/// **FAUT-IL RÉÉCRIRE LE SOUVENIR DE GAUCHE ?** — la DÉCISION, séparée de
/// l'écriture pour être testable **sans toucher au vrai fichier**.
///
/// ⚠️ **CE FICHIER EST LE PLUS ÉCRIT DU DÉPÔT** : la moitié gauche change à
/// chaque dossier qu'on y ouvre. Le dépôt a une plaie ouverte sur une
/// écriture trop confiante — celle qui a transformé un défaut de lecture
/// transitoire en perte DÉFINITIVE de deux dossiers épinglés. On ne réécrit donc
/// jamais le chemin déjà retenu.
fn souvenir_a_ecrire(dossier: &Path) -> bool {
    GAUCHE_MEMORISEE.with(|g| g.borrow().as_deref() != Some(dossier))
}

/// Retient le dernier dossier de gauche — **seulement s'il a changé**.
fn memoriser_gauche(dossier: &Path) {
    if !souvenir_a_ecrire(dossier) {
        return;
    }
    GAUCHE_MEMORISEE.with(|g| *g.borrow_mut() = Some(dossier.to_path_buf()));
    crate::config::save_sous_dock_gauche(dossier);
}

/// **LE CONTENU D'UN DOSSIER — DU CACHE S'IL Y EST, SINON DU DISQUE.**
///
/// ⚠️ **LE CACHE N'EST PLUS CELUI DES ICÔNES, ET SON PRIX A CHANGÉ DE NATURE.**
/// Il économisait jusqu'à 48 `SHGetFileInfoW` ; il n'économise plus qu'un
/// `read_dir` et un tri — mais sur un dossier de 5 000 entrées, ce n'est pas
/// rien, et le geste le plus fréquent d'un explorateur (entrer quelque part,
/// puis ressortir) le paierait deux fois. Les icônes, elles, ne sont plus
/// calculées ici du tout : elles se demandent une fois, à l'envoi.
///
/// ⚠️ **LA CLÉ EST `(chemin, tri)`** : le même dossier rangé autrement est un
/// autre contenu, et le confondre afficherait un ordre qui ne correspond à aucun
/// bouton allumé. **L'invalidation est le fait d'`Actualiser`** (voir
/// `invalider`) : sans elle, le bouton « relire ce dossier » ne relirait rien.
fn contenu(chemin: &Path, tri: Tri) -> Contenu {
    let trouve = CACHE.with(|c| {
        c.borrow()
            .iter()
            .position(|(p, t, _)| p == chemin && *t == tri)
    });
    if let Some(position) = trouve {
        // Le plus récent repasse DEVANT : c'est une liste LRU, et l'entrée qu'on
        // vient de relire est celle qu'on relira le plus probablement.
        return CACHE.with(|c| {
            let mut c = c.borrow_mut();
            let entree = c.remove(position);
            let contenu = entree.2.clone();
            c.insert(0, entree);
            contenu
        });
    }

    let liste = crate::win32::sous_dock::enfants(chemin, tri);
    let contenu = Contenu { tronque: liste.tronque, enfants: enfants_prets(&liste) };
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.insert(0, (chemin.to_path_buf(), tri, contenu.clone()));
        c.truncate(CACHE_MAX);
    });
    contenu
}

/// **OUBLIE CE QU'ON SAIT D'UN DOSSIER** — appelé par `Actualiser`, et le jour où
/// un geste modifiera le disque (lot 2 : copier/déplacer).
///
/// ⚠️ **ON OUBLIE TOUS LES TRIS DE CE CHEMIN**, pas seulement celui en cours : le
/// contenu a changé, donc les quatre ordres sont périmés.
pub fn invalider(chemin: &Path) {
    CACHE.with(|c| c.borrow_mut().retain(|(p, _, _)| p != chemin));
}

/// Le tableau des enfants d'un dossier — **SANS LEURS ICÔNES**, qui partent par
/// `window.setIcons` (voir `EnfantPret` et `envoyer_des_icones`).
///
/// C'est une simple recopie : plus aucune image n'est décodée ni encodée ici.
fn enfants_prets(liste: &crate::win32::sous_dock::Liste) -> Vec<EnfantPret> {
    liste
        .enfants
        .iter()
        .map(|e| EnfantPret {
            nom: e.nom.clone(),
            chemin: e.chemin.to_string_lossy().into_owned(),
            dossier: e.est_dossier,
        })
        .collect()
}

/// **RAMÈNE UNE ICÔNE DU SHELL À LA TAILLE DE LA CASE** — pur, donc vérifiable
/// sans shell ni GPU.
///
/// ⛔ **LE ROGNAGE DE LA MARGE TRANSPARENTE EST LE CŒUR DE CETTE FONCTION, ET SON
/// ABSENCE ÉTAIT UN DÉFAUT SIGNALÉ** : « les fichiers java, leur icône apparaît en
/// taille minuscule ». Mesuré sur un vrai `.jar` :
///
/// | type | canvas du shell | dessin | à l'écran |
/// |---|---|---|---|
/// | **`.jar`** | 256×256 | **48×48, dans un coin** | **18×18 sur 96** |
/// | `.txt` | 256×256 | 167×222 | 63×83 |
/// | `.exe` | 256×256 | 246×225 | 92×84 |
/// | dossier | 256×256 | 224×176 | 84×66 |
///
/// Le shell ne rend donc pas une icône plus PETITE pour un `.jar` — il rend un
/// grand canvas dont le **dessin n'occupe qu'un coin**, et la mise à l'échelle
/// rétrécit le dessin avec la marge. `gfx::icon::trim_transparent_padding`, que le
/// Dash, la Recherche et les applis UWP utilisent déjà, décrit ce cas noir sur
/// blanc ; **le dash dossier était le seul à l'avoir oublié**.
///
/// ⚠️ **IL REPOSE 6 % DE MARGE APRÈS ROGNAGE** (`PADDING_FRACTION`) : une icône
/// déjà bien cadrée retrouve donc presque son cadrage d'origine — d'où l'absence
/// d'effet de bord sur les autres types, alors qu'un `.jar` passe de 18 px à toute
/// la case.
///
/// ⚠️ **LE PLUS GRAND CÔTÉ VAUT `ICONE_PX`, L'AUTRE SUIT** — on ne déforme pas. La
/// page s'en accommode (`object-fit: contain`, voir `sous_dock.html`) : elle
/// centre une image non carrée au lieu de l'étirer, ce que forcer un carré ici
/// aurait interdit.
///
/// ⚠️ **UNE ICÔNE DÉJÀ ASSEZ PETITE EST RENDUE TELLE QUELLE**, sans agrandissement
/// — comme `cef_dash::reduire_icone`. La page l'agrandit de toute façon pour
/// remplir sa case, et le faire ici coûterait un redimensionnement pour rien.
///
/// ⛔ **`thumbnail` ET NON `resize(Lanczos3)`, ET C'EST UNE MESURE, PAS UN GOÛT.**
/// `Lanczos3` coûtait **4,36 ms** par icône — les deux tiers du prix total d'une
/// case. On RÉDUIT ici, on n'agrandit pas : le filtre boîte de `thumbnail` est
/// l'outil juste, et il a été chronométré à **0,71 ms** sur les mêmes images.
/// Comparaison complète, via `la_sonde_du_cout_d_une_icone` :
/// `Lanczos3 4,36 · CatmullRom 3,04 · Triangle 1,72 · thumbnail 0,71` (ms/icône).
/// ⚠️ `cef_dash::reduire_icone` fait le même travail avec `Lanczos3` — elle sert le
/// Dash, dont les icônes sont en cache et bien moins nombreuses.
#[cfg(test)]
fn normaliser_icone(largeur: u32, hauteur: u32, rgba: Vec<u8>) -> Option<(u32, u32, Vec<u8>)> {
    normaliser_icone_a(ICONE_PX, largeur, hauteur, rgba)
}

/// `normaliser_icone` pour un côté donné — celui du niveau de zoom en cours.
fn normaliser_icone_a(cote: u32, largeur: u32, hauteur: u32, rgba: Vec<u8>) -> Option<(u32, u32, Vec<u8>)> {
    let (largeur, hauteur, rgba) =
        crate::gfx::icon::trim_transparent_padding(largeur, hauteur, &rgba);
    if largeur == 0 || hauteur == 0 {
        return None;
    }
    if largeur <= cote && hauteur <= cote {
        return Some((largeur, hauteur, rgba));
    }
    let image = image::RgbaImage::from_raw(largeur, hauteur, rgba)?;
    let echelle = cote as f32 / largeur.max(hauteur) as f32;
    let l = ((largeur as f32 * echelle).round() as u32).max(1);
    let h = ((hauteur as f32 * echelle).round() as u32).max(1);
    let reduite = image::imageops::thumbnail(&image, l, h);
    Some((l, h, reduite.into_raw()))
}

/// **L'ICÔNE D'UN CHEMIN, EN `data:` URI, CALCULÉE MAINTENANT.**
///
/// ⭐ **C'EST ICI QUE SE JOUE LA DIFFÉRENCE.** Avant, cette extraction avait lieu à
/// la LECTURE du dossier, pour toutes les entrées à la fois : c'est ce coût qui
/// obligeait à couper la liste à 48, et donc à ne pas montrer des fichiers. Elle a
/// lieu maintenant dans le vidage de la file, `ICONES_MS_PAR_FRAME` à la fois.
///
/// ⚠️ **`data:` URI et non un chemin de fichier** : la page n'a AUCUN accès au
/// système de fichiers, c'est la règle de toutes les pages de ce dépôt (voir
/// `win32::file_dialog`, doc de module). **`None` quand le shell n'a pas donné
/// d'icône** — c'est une valeur qu'on ENVOIE (voir `envoyer_des_icones`), la page
/// dessine alors un carré vide et ne redemandera plus ce chemin.
///
/// ⛔ **LA TAILLE ENCODÉE N'EST PAS CELLE DU SHELL : voir `normaliser_icone`.** Le
/// shell rend du JUMBO (256 px, voir `sous_dock::icone_du_shell`) parce qu'une
/// vignette de dossier peut être grande — mais ici la case fait 32 px CSS, donc
/// 96 px physiques. Transporter du 256 px serait sept fois les pixels pour rien, et
/// c'est cette charge-là qui faisait les 743 Ko mesurés.
///
/// ⭐ **UNE IMAGE REÇOIT SA VIGNETTE** (DEMANDE EXPLICITE, 2026-10-11) — celle du
/// shell (`sous_dock::vignette_du_shell`), à la taille du zoom. Sans vignette (un
/// format que le shell ne sait pas lire), elle retombe sur son icône.
///
/// ⚠️ **LE BOOLÉEN DIT « UNE IMAGE SANS SA VIGNETTE »** : l'appelant montre alors
/// l'icône, mais redemandera la vignette (voir `RELANCE_VIGNETTE`).
///
/// ⭐ **UNE VIDÉO AUSSI** (DEMANDE EXPLICITE, 2026-10-12), mais ⛔ **JAMAIS EXTRAITE
/// ICI** : seul le cache du shell est lu (voir `sous_dock::vignette_video_du_cache`,
/// 1,07 s mesurée pour une vidéo neuve). Une absence part à l'extracteur, et c'est
/// SA fin qui redemande la vidéo (voir `relances_echues`) — pas les relances
/// minutées, qui s'épuiseraient avant lui sur un dossier de vidéos neuves.
fn icone_du_chemin(chemin: &str) -> (Option<String>, bool) {
    let cote = icone_px();
    let chemin = Path::new(chemin);
    let video = crate::win32::sous_dock::est_une_video(chemin);
    let image = crate::win32::sous_dock::est_une_image(chemin);
    let vignette = if video {
        let lue = crate::win32::sous_dock::vignette_video_du_cache(chemin, cote);
        if lue.is_none() {
            crate::win32::sous_dock::demander_extraction(chemin, cote);
        }
        lue
    } else if image {
        crate::win32::sous_dock::vignette_du_shell(chemin, cote)
    } else {
        None
    };
    if image || video {
        if let Some((largeur, hauteur, rgba)) = vignette {
            if let Some((l, h, octets)) = reduire_vignette(cote, largeur, hauteur, rgba) {
                return (encoder_vignette(l, h, &octets), false);
            }
        }
    }
    let icone = crate::win32::sous_dock::icone_du_shell(chemin)
        .and_then(|(largeur, hauteur, rgba)| normaliser_icone_a(cote, largeur, hauteur, rgba))
        .and_then(|(l, h, octets)| crate::win32::cef_dash::encode_rgba_to_data_uri(l, h, &octets));
    (icone, image)
}

/// **LE DÉLAI AVANT DE REDEMANDER UNE VIGNETTE, ET LE NOMBRE D'ESSAIS.**
///
/// ⛔ **DÉFAUT SIGNALÉ (2026-10-11)** : « je suis obligé d'ouvrir 2 fois le même
/// dossier pour que les vignettes des images apparaissent ». Le journal le
/// montrait : à la PREMIÈRE entrée dans un dossier OneDrive, des dizaines d'images
/// partaient à **10 694 octets pile** — l'icône générique « image », la même pour
/// toutes. Le shell n'avait pas encore leur vignette, on retombait sur l'icône, et
/// rien ne la redemandait ; à la seconde entrée, son cache était rempli. On montre
/// donc l'icône, puis on redemande la vignette plus tard, quelques fois.
const RELANCE_VIGNETTE: std::time::Duration = std::time::Duration::from_millis(700);
const RELANCES_MAX: u32 = 5;

/// Programme une nouvelle demande de vignette pour `chemin` — sauf s'il a déjà
/// épuisé ses essais.
fn programmer_relance(chemin: String, essais: u32) {
    if essais >= RELANCES_MAX {
        crate::diag!("[sous-dock] vignette abandonnee apres {essais} essais : {chemin}");
        return;
    }
    RELANCES.with(|r| r.borrow_mut().push((chemin, std::time::Instant::now() + RELANCE_VIGNETTE, essais + 1)));
}

/// **REMET EN FILE LES RELANCES ÉCHUES** — et rend le nombre d'essais de chacune,
/// pour que l'envoi le reporte. ⚠️ Un chemin qui n'est plus MARQUÉ n'est plus
/// affiché (on a quitté son dossier, voir `oublier_les_marques`) : sa relance est
/// jetée, sinon une vignette d'un autre dossier arriverait pour rien.
fn relances_echues() -> std::collections::HashMap<String, u32> {
    let maintenant = std::time::Instant::now();
    let echues: Vec<(String, u32)> = RELANCES.with(|r| {
        let mut r = r.borrow_mut();
        let (pretes, attente): (Vec<_>, Vec<_>) = r.drain(..).partition(|(_, quand, _)| *quand <= maintenant);
        *r = attente;
        pretes.into_iter().map(|(chemin, _, essais)| (chemin, essais)).collect()
    });
    let mut essais = std::collections::HashMap::new();
    for (chemin, n) in echues {
        let affiche = ICONES_ENVOYEES.with(|e| e.borrow().contains(&chemin));
        if affiche {
            ICONES_A_ENVOYER.with(|file| file.borrow_mut().push_back(chemin.clone()));
            essais.insert(chemin, n);
        }
    }
    // Les VIDÉOS dont l'extracteur vient de remplir le cache (voir
    // `icone_du_chemin`) — redemandées si elles sont encore affichées.
    for chemin in crate::win32::sous_dock::extraites() {
        let chemin = chemin.to_string_lossy().into_owned();
        if ICONES_ENVOYEES.with(|e| e.borrow().contains(&chemin)) {
            ICONES_A_ENVOYER.with(|file| file.borrow_mut().push_back(chemin));
        }
    }
    essais
}

/// Une vignette ramenée à `cote` — ⚠️ **SANS ROGNAGE** : la marge transparente
/// d'une icône est un défaut du shell (voir `normaliser_icone`), celle d'une image
/// est son CONTENU, et la rogner recadrerait le dessin de l'utilisateur.
fn reduire_vignette(cote: u32, largeur: u32, hauteur: u32, rgba: Vec<u8>) -> Option<(u32, u32, Vec<u8>)> {
    if largeur <= cote && hauteur <= cote {
        return Some((largeur, hauteur, rgba));
    }
    let image = image::RgbaImage::from_raw(largeur, hauteur, rgba)?;
    let echelle = cote as f32 / largeur.max(hauteur) as f32;
    let l = ((largeur as f32 * echelle).round() as u32).max(1);
    let h = ((hauteur as f32 * echelle).round() as u32).max(1);
    Some((l, h, image::imageops::thumbnail(&image, l, h).into_raw()))
}

/// **UNE VIGNETTE OPAQUE PART EN JPEG, UNE AUTRE EN PNG.** Une photo de 384 px
/// pèse environ dix fois moins en JPEG — et ces octets traversent `execute_js`
/// par lots. ⚠️ Un PNG transparent (un logo, une capture détourée) garde son
/// alpha : le JPEG n'en a pas, et le fond deviendrait noir.
fn encoder_vignette(largeur: u32, hauteur: u32, rgba: &[u8]) -> Option<String> {
    if rgba.chunks_exact(4).any(|p| p[3] < 255) {
        return crate::win32::cef_dash::encode_rgba_to_data_uri(largeur, hauteur, rgba);
    }
    use base64::Engine as _;
    let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let image = image::RgbImage::from_raw(largeur, hauteur, rgb)?;
    let mut octets = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut octets, 85).encode_image(&image).ok()?;
    Some(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(octets)))
}

/// **CHANGE LE NIVEAU DE ZOOM** — le retient, et RENVOIE les images des deux
/// moitiés à la nouvelle taille : sans ce renvoi, un zoom avant agrandirait des
/// icônes de 96 px jusqu'à 384, floues.
///
/// ⚠️ **LA FILE EN COURS EST OUBLIÉE D'ABORD** : elle porte des chemins promis à
/// l'ANCIENNE taille, et `remettre_tout_en_file` les remet en tête à la nouvelle.
fn changer_le_zoom(niveau: usize) {
    let niveau = niveau.min(TAILLES_CASE.len() - 1);
    if niveau == niveau_de_zoom() {
        return;
    }
    NIVEAU_ZOOM.with(|n| n.set(Some(niveau)));
    crate::config::save_dossier_zoom(niveau);
    crate::diag!("[sous-dock] zoom : niveau {niveau} ({} px)", TAILLES_CASE[niveau]);
    oublier_la_file_d_icones();
    remettre_tout_en_file();
    pousser_tout_de_suite();
}

/// L'objet JSON d'UNE moitié — **SANS LES ICÔNES** (voir `EnfantPret`).
fn moitie_json(
    moitie: &Moitie,
    contenu: &Contenu,
    elements: &[crate::config::ElementBarre],
    message: Option<&str>,
) -> String {
    // Le PARENT, pour le bouton de remontée — absent à la racine d'un lecteur.
    let parent = moitie.dossier.parent().map(|p| p.to_string_lossy().into_owned());
    let enfants: Vec<serde_json::Value> = contenu
        .enfants
        .iter()
        .map(|e| {
            serde_json::json!({
                "nom": e.nom,
                // Le chemin sert de CLÉ à la page (jamais un index, qui change
                // dès qu'on entre quelque part) — même règle que les raccourcis
                // de la section Réglages. C'est AUSSI la clé du cache d'icônes.
                "chemin": e.chemin,
                "dossier": e.dossier,
                // Une vidéo se lit avec VLC (voir `vlc` plus bas) : la page ne
                // tient pas de liste d'extensions, c'est la nôtre qui décide.
                "video": !e.dossier && crate::win32::sous_dock::est_une_video(Path::new(&e.chemin)),
            })
        })
        .collect();
    serde_json::json!({
        "dossier": moitie.dossier.to_string_lossy(),
        "nom": moitie.dossier.file_name().map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| moitie.dossier.to_string_lossy().into_owned()),
        "parent": parent,
        "tronque": contenu.tronque,
        // LE TRI EN COURS, pour que la page puisse montrer lequel est actif —
        // sans lui, elle afficherait quatre boutons dont aucun ne dirait l'ordre
        // dans lequel on lit réellement la liste.
        "tri": moitie.tri.cle(),
        // **LE COMPTEUR QUE LA PAGE COMPARE POUR NE RECONSTRUIRE QUE CETTE
        // MOITIÉ** — voir `Moitie::revision`.
        "revision": moitie.revision,
        // LE DOSSIER EST-IL DÉJÀ ÉPINGLÉ ? — relu DU FICHIER, et non d'un
        // drapeau tenu ici : le panneau Réglages écrit la même liste, et un
        // drapeau local mentirait dès qu'on y aurait touché. Ce n'est PAS un coût
        // par frame : `moitie_json` n'est appelé qu'au changement d'une moitié.
        // ⚠️ **ON FILTRE LES DOSSIERS** : la liste porte aussi des SEPARATEURS,
        // qui n'ont pas de chemin (voir `config::ElementBarre`).
        "epingle": elements.iter().any(|element| {
            matches!(element, crate::config::ElementBarre::Dossier(d) if d.chemin == moitie.dossier)
        }),
        // Un TABLEAU, pas une chaîne : la page itère dessus.
        // ⛔ **IL NE PORTE PAS LES ICÔNES** — voir `EnfantPret`, où la mesure des
        // 743 Ko est écrite. C'est ce qui fait tomber l'état de ~370 Ko à
        // quelques kilo-octets par moitié.
        "enfants": enfants,
        // **LE COMPTE RENDU DU DERNIER GESTE**, ou `null` — voir `signaler`.
        // ⚠️ Il voyage DANS l'état de la moitié (et non à part) pour une raison :
        // la page ne reconstruit une moitié que sur changement de RÉVISION, et
        // c'est `signaler` qui la fait avancer. Un message poussé par un autre
        // canal n'aurait provoqué aucun rendu.
        "message": message,
        // VLC est-il installé ? Sans lui, le menu d'une vidéo offre « Voir
        // l'aide » au lieu d'« Afficher » (voir `Demande::AideVideo`). Relu à
        // chaque changement de la moitié : installé entre-temps, il est vu à la
        // prochaine ouverture d'un dossier.
        "vlc": crate::win32::lecteur_video::vlc_installe(),
    })
    .to_string()
}

/// **L'ÉTAT COMPLET DU PANNEAU, ASSEMBLÉ À PARTIR DES DEUX MOITIÉS.**
///
/// Extrait pour être TESTABLE : c'est du collage de chaînes, et une virgule ou
/// une accolade perdue y produirait le même `SyntaxError` muet que celui qui a
/// coûté une journée (voir `expression_js`).
///
/// ⚠️ **LA PUNAISE N'EST PLUS DANS CET ÉTAT.** Elle l'a été une heure, quand ce
/// panneau avait sa mécanique propre ; depuis qu'elle est commune aux quinze
/// panneaux, c'est `cef_popup` qui la reflète à chaque frame (`js_epingle`), par le
/// même canal que le zoom. La laisser ici aurait fait DEUX écrivains du même
/// attribut, donc un bouton qui clignote.
fn composer(gauche: &str, droite: &str, open: u64, echelle: f32, zoom: usize) -> String {
    format!(
        r#"{{"gauche":{gauche},"droite":{droite},"open":{open},"echelle":{echelle},"zoom":{zoom}}}"#
    )
}

/// **L'EXPRESSION JS ENVOYÉE À LA PAGE** — extraite pour être TESTABLE sans
/// disque, sans fenêtre et sans CEF (voir le test qui l'accompagne : c'est le
/// seul garde-fou d'un défaut entièrement silencieux).
///
/// ⚠️ **LE NUMÉRO D'OUVERTURE EST MAINTENANT DANS L'ÉTAT LUI-MÊME**, et non
/// greffé par un second argument d'`Object.assign`. C'est ce qui a supprimé le
/// défaut d'origine : l'ancienne forme devait OUVRIR l'objet JSON pour y glisser
/// `{open: N}`, et le `}` retiré pour ce faire n'était jamais remis. Ici on
/// assemble l'objet entier en une seule fois (`composer`) — il n'y a plus
/// d'ouverture à faire, donc plus d'accolade à oublier.
fn expression_js(json: &str) -> String {
    format!("window.setState({json})")
}

fn pousser_tout_de_suite() {
    // ⚠️ **`CHANGEMENTS` ET NON `OPEN_COUNT`** : on pousse parce que QUELQUE
    // CHOSE a changé, pas parce que le panneau vient de naître. Les confondre
    // ferait rendre les deux moitiés à chaque changement (voir `OPEN_COUNT`).
    CHANGEMENTS.with(|c| c.set(c.get() + 1));
    // Le compteur de rafale repart de zéro : `poll` va repousser pendant
    // `RAFALE_DE_POUSSEE` frames, puis se taire.
    let changement = CHANGEMENTS.with(Cell::get);
    POUSSEE.with(|p| p.set((changement, 0)));
    push_state();
}

fn push_state() {
    let gauche = avec(Cote::Gauche, |m| m.json.clone());
    let droite = avec(Cote::Droite, |m| m.json.clone());
    if gauche.is_empty() || droite.is_empty() {
        return;
    }
    let open = OPEN_COUNT.with(Cell::get);
    let changement = CHANGEMENTS.with(Cell::get);
    // La TAILLE DU CONTENU (Réglages, « Taille du contenu des dashs ») — lue à
    // chaque poussée, donc prise à la prochaine ouverture ou au prochain
    // changement du panneau : comme les autres panneaux, pas en direct.
    let json = composer(
        &gauche,
        &droite,
        open,
        // La taille des textes GLOBALE s'y ajoute (`taille_panneaux::TEXTES`).
        crate::win32::taille_panneaux::facteur("dossier") * crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES),
        niveau_de_zoom(),
    );
    let js = expression_js(&json);
    // SONDE — **CE QUI PART RÉELLEMENT VERS LA PAGE**, et sous forme de NOMBRE.
    // `execute_js` ne dit rien de son échec (voir `cef_popup::execute_js`), donc
    // la taille envoyée est la seule chose qui se relise après coup.
    //
    // ⛔ **LE DÉBUT DE LA CHAÎNE N'EST PAS JOURNALISÉ, DÉLIBÉRÉMENT** : il a
    // coûté une journée en ayant l'air valide (l'erreur était dans les vingt
    // derniers caractères). Un compte d'octets, lui, se COMPARE d'un run à
    // l'autre — c'est ainsi que les 4607 octets d'avant et les 4608 d'après ont
    // désigné l'accolade manquante, à elle seule.
    //
    // ⚠️ **LA SONDE PORTE MAINTENANT LA TAILLE DE CHAQUE MOITIÉ.** Avec deux
    // grilles, « 90 Ko » ne dit plus laquelle a grossi ; les deux chiffres
    // séparés, si. Une fois par OUVERTURE et non par frame.
    SONDE_JS.with(|dernier| {
        if *dernier.borrow() != changement {
            *dernier.borrow_mut() = changement;
            crate::diag!(
                "[sous-dock] envoi n°{open}.{changement} : {} octets (gauche {}, droite {})",
                js.len(),
                gauche.len(),
                droite.len()
            );
        }
    });
    cef_popup::execute_js(&SLOT, &js);
}

/// **L'EXPRESSION QUI PORTE UN LOT D'ICÔNES.**
///
/// Extraite pour la même raison qu'`expression_js`, et ce n'est pas une
/// coquetterie : elle transporte des CHEMINS D'UTILISATEUR — donc des accolades,
/// des guillemets et des barres obliques inversées. Une chaîne mal échappée y
/// produirait le même `SyntaxError` muet que celui qui a coûté une journée, et
/// `execute_js` ne rend toujours aucun compte de son échec.
fn expression_icones(lot: &[(String, Option<String>)]) -> String {
    let paires = serde_json::Value::Array(
        lot.iter()
            .map(|(chemin, icone)| serde_json::json!([chemin, icone]))
            .collect(),
    );
    format!("window.setIcons({paires})")
}

/// **ENVOIE UN LOT D'ICÔNES À LA PAGE** — appelé à chaque frame par `poll`, et
/// pour son propre compte : la fin de la rafale d'état ne doit PAS arrêter les
/// icônes, qui ont leur propre file.
///
/// ⭐ **C'EST ICI QUE LES ICÔNES SE CALCULENT.** La file ne portait que des
/// chemins ; l'extraction du shell, le redimensionnement et l'encodage ont lieu
/// dans la boucle ci-dessous, pour les seuls `ICONES_PAR_FRAME` chemins qui
/// sortent de la file à cette frame. C'est ce déplacement qui permet de lister un
/// dossier ENTIER sans payer son coût à l'ouverture.
///
/// ⚠️ **PAR LOTS** — même valeur et même raison que
/// `cef_dash::ICON_BATCH_PER_FRAME` : le coût total est le même, c'est sa
/// RÉPARTITION qui change, et c'est elle qui se voit à l'écran.
fn envoyer_des_icones() {
    let essais_en_cours = relances_echues();
    let depart = std::time::Instant::now();
    let mut lot: Vec<(String, Option<String>)> = Vec::new();
    // ⚠️ **ON POP UN CHEMIN À LA FOIS, ET ON S'ARRÊTE SUR LE TEMPS.** Le plafond
    // `ICONES_PAR_FRAME` borne le lot, mais c'est `ICONES_MS_PAR_FRAME` qui décide
    // quand s'arrêter : un nombre fixe gelait la frame sur les dossiers chargés.
    while lot.len() < ICONES_PAR_FRAME {
        // ⛔ **L'EMPRUNT EST RELÂCHÉ AVANT DE CALCULER.** `icone_du_chemin` appelle
        // le shell ; tenir un `RefCell` pendant un appel Win32 est exactement la
        // forme de réentrance que `avec` documente comme à ne pas écrire.
        let Some(chemin) = ICONES_A_ENVOYER.with(|file| file.borrow_mut().pop_front()) else {
            break;
        };
        let (icone, sans_vignette) = icone_du_chemin(&chemin);
        if sans_vignette {
            programmer_relance(chemin.clone(), essais_en_cours.get(&chemin).copied().unwrap_or(0));
        }
        lot.push((chemin, icone));
        // ⚠️ **LE TEST EST APRÈS LE CALCUL, ET C'EST VOULU** : il garantit **au
        // moins une icône par frame**. Testé avant, une icône plus chère que le
        // budget à elle seule ferait tourner la boucle sans jamais avancer — la
        // file ne se viderait plus, et le panneau resterait à moitié rempli.
        if depart.elapsed().as_secs_f64() * 1000.0 >= ICONES_MS_PAR_FRAME {
            break;
        }
    }
    if lot.is_empty() {
        return;
    }
    // ⚠️ **`None` EST UNE VALEUR QU'ON ENVOIE**, pas une absence : c'est ce qui dit
    // à la page « ce chemin n'aura jamais d'icône », et l'empêche de le redemander
    // indéfiniment (voir `EnfantPret`).
    let js = expression_icones(&lot);
    // SONDE — **CE QUI PART PAR LE CANAL DES ICÔNES**, et **CE QU'IL A COÛTÉ**.
    // Le nombre d'octets dit ce que la page reçoit ; la DURÉE dit ce que la frame
    // a payé sur le fil de l'interface. ⚠️ **Une durée au-dessus de
    // `ICONES_MS_PAR_FRAME` n'est pas une anomalie** : c'est une icône qui a coûté
    // plus cher que le budget à elle seule (le minimum d'une par frame).
    crate::diag!(
        "[sous-dock] icones : lot de {} ({} octets, {:.1} ms)",
        lot.len(),
        js.len(),
        depart.elapsed().as_secs_f64() * 1000.0
    );
    cef_popup::execute_js(&SLOT, &js);
}

/// **À APPELER À CHAQUE FRAME tant que le panneau est visible — mais elle NE
/// REPOUSSE PAS l'état à chaque frame.** Voir `RAFALE_DE_POUSSEE`, qui porte la
/// mesure et le pourquoi : la poussée sert à survivre au chargement de la page,
/// elle n'a aucune raison de durer une fois la page en place.
///
/// ⚠️ **LES ICÔNES SONT SERVIES À PART, ET HORS DE LA RAFALE** : leur file est
/// vidée à chaque frame jusqu'à épuisement, quelle que soit la rafale. Les faire
/// dépendre de la condition de poussée les ferait cesser au bout d'une seconde —
/// or une icône demandée plus tard (un dossier traversé) doit partir tout de
/// suite, sans rouvrir de rafale.
pub fn poll() {
    if !cef_popup::is_visible(&SLOT) {
        return;
    }
    let (poussee, frames) = POUSSEE.with(Cell::get);
    let changement = CHANGEMENTS.with(Cell::get);
    if poussee != changement || frames < RAFALE_DE_POUSSEE {
        POUSSEE.with(|p| p.set((changement, frames.saturating_add(1))));
        push_state();
    }
    envoyer_des_icones();
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

/// Les chemins d'une sélection, envoyés en un segment séparé par des sauts de
/// ligne (voir `Demande::CopierFichier`).
fn plusieurs_chemins(segment: &str) -> Vec<PathBuf> {
    segment.split('\n').map(str::trim).filter(|c| !c.is_empty()).map(PathBuf::from).collect()
}

/// La fenêtre du panneau — propriétaire des dialogues de Windows qu'un geste
/// ouvre, pour qu'ils passent DEVANT lui (il est au premier plan).
pub fn hwnd() -> Option<HWND> {
    cef_popup::hwnd(&SLOT)
}

/// Les collages terminés sur leur fil (voir `app::coller_dans_la_moitie`) :
/// le côté, et le message à montrer.
static COLLAGES_TERMINES: std::sync::Mutex<Vec<(Cote, String)>> = std::sync::Mutex::new(Vec::new());

pub fn collage_termine(cote: Cote, message: String) {
    if let Ok(mut l) = COLLAGES_TERMINES.lock() {
        l.push((cote, message));
    }
}

/// À relever à chaque frame, sur le fil de l'interface.
pub fn collages_termines() -> Vec<(Cote, String)> {
    COLLAGES_TERMINES.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default()
}

/// Le panneau est-il affiché ? — `app::run` s'en sert pour ne repousser l'état
/// que quand il y a une page à qui parler.
pub fn is_visible() -> bool {
    cef_popup::is_visible(&SLOT)
}

/// **LE CURSEUR EST-IL SUR LE PANNEAU ?** — l'équivalent, pour un popup CEF, de
/// `window_previews::PreviewPanel::contains_screen_point`.
///
/// ⛔ **C'EST LA PIÈCE QUI FAIT QUE LE GESTE DE RETOUR MARCHE**, et ce n'est PAS
/// `cef_popup::hide_if_focus_elsewhere` : cette dernière est **délibérément
/// absente** des popups dont l'utilisateur doit garder la main dessus (les deux
/// curseurs, voicemeeter), parce qu'elle masque la fenêtre **150 ms** après que
/// le premier plan lui a été repris — défaut mesuré, « le slider s'affiche un
/// bref instant puis disparaît ». Pour un panneau ouvert AU SURVOL, elle
/// refermerait le panneau à l'instant où l'on tend la main vers lui.
///
/// ⚠️ **DEPUIS LE LOT 1, CE TEST COUVRE PRESQUE TOUTE LA ZONE LIBRE** — la page
/// remplit désormais sa fenêtre au lieu d'y flotter en `fit-content`. La
/// conséquence à surveiller est à l'autre bout : `app::run` ne referme plus par
/// contenance dès qu'on est dans la zone, donc **la grâce de
/// `PREVIEW_HIDE_GRACE` et le verrou `sous_dock_open_for` sont les seuls
/// garde-fous**.
///
/// ⚠️ **`GetWindowRect` ET NON LE RECTANGLE PASSÉ À `ouvrir`** : `cef_popup`
/// redimensionne la fenêtre quand la page annonce sa taille
/// (`resize_to_content`), donc le rectangle d'ouverture est PÉRIMÉ dès la
/// première frame. La fenêtre sait où elle est ; on le lui demande.
pub fn contient_le_point(x: i32, y: i32) -> bool {
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return false };
    let mut rect = windows::Win32::Foundation::RECT::default();
    if unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect) }.is_err() {
        return false;
    }
    x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom
}

/// Le dossier montré par une moitié (`None` si elle n'a jamais été ouverte).
pub fn dossier_affiche(cote: Cote) -> Option<PathBuf> {
    avec(cote, |m| (!m.dossier.as_os_str().is_empty()).then(|| m.dossier.clone()))
}

/// **DÉCODE UNE ACTION DE LA PAGE — PUR, POUR ÊTRE TESTABLE SANS CEF.**
///
/// ⚠️ **TROIS SEGMENTS : LE CÔTÉ, LE VERBE, L'ARGUMENT.** Le côté est décodé
/// comme le reste (`Cote::depuis_cle`) et **REFUSÉ s'il est inconnu** : replier
/// sur la gauche enverrait à la mauvaise moitié l'ordre destiné à la droite, et
/// l'utilisateur verrait bouger un panneau qu'il n'a pas visé.
///
/// ⚠️ **`splitn(4, …)` ET NON `splitn(3, …)`** : on ne scinde QUE les trois
/// premiers `/`, donc le chemin reste ENTIER même s'il en contenait un (il n'en
/// contient pas — `encodeURIComponent` encode `/` en `%2F` — mais la découpe ne
/// doit pas en dépendre). ⛔ **Le quatrième segment existe pour `renommer`**, le
/// seul geste qui porte DEUX valeurs : le fichier ET son nouveau nom. Le joindre
/// au chemin par un séparateur inventé aurait demandé de choisir un caractère
/// interdit dans un nom de fichier — il n'y en a pas d'assez sûr.
pub fn demande_depuis_action(action: &str) -> Option<Demande> {
    let reste = action.strip_prefix("sous-dock/")?;
    // ⭐ **LE SIGNAL DE VIE SE TRAITE AVANT TOUT PARSING DE CÔTÉ** : il n'en a
    // aucun, et `Cote::depuis_cle("ready")` le refuserait — à raison, c'est ce qui
    // empêche la page de viser une moitié qui n'existe pas. Il ne rend AUCUNE
    // `Demande` : il ouvre une porte, il ne déclenche pas de geste.
    if reste == SIGNAL_PRET {
        crate::diag!("[sous-dock] la page se dit prete : on renvoie les icones des deux moities");
        remettre_tout_en_file();
        return None;
    }
    if let Some(niveau) = reste.strip_prefix(SIGNAL_ZOOM).and_then(|r| r.strip_prefix('/')) {
        match niveau.parse::<usize>() {
            Ok(niveau) if niveau < TAILLES_CASE.len() => changer_le_zoom(niveau),
            _ => eprintln!("[neuradesk][cef] zoom de sous-dock illisible : {action:?}"),
        }
        return None;
    }
    // ⚠️ **LA PUNAISE N'EST PLUS ICI** : elle est devenue commune aux quinze
    // panneaux, donc elle est traitée dans `cef_popup::drain_actions`, qui retire
    // l'action avant que ce module ne la voie (voir `est_maintenu`).
    let mut parts = reste.splitn(4, '/');
    let cote = parts.next().unwrap_or("");
    let verbe = parts.next().unwrap_or("");
    // Le CHEMIN est décodé ici : la page encode chaque segment
    // (`encodeURIComponent`), donc chaque bras doit décoder le sien — règle du
    // dépôt, et elle a déjà été oubliée deux fois.
    let argument = parts.next().map(crate::win32::cef_app::percent_decode);
    let complement = parts.next().map(crate::win32::cef_app::percent_decode);
    let Some(cote) = Cote::depuis_cle(cote) else {
        eprintln!("[neuradesk][cef] demande sous-dock sans cote valide : {action:?}");
        return None;
    };
    // **LE GARDE PAR LA LISTE, AVANT LE `match`.** Elle n'est donc pas
    // décorative : un verbe ajouté au `match` sans l'être ici est REFUSÉ, et un
    // verbe ajouté ici sans bras dans le `match` tombe dans le `_` et est
    // refusé aussi. Le test ne peut plus passer en ne vérifiant qu'un seul côté.
    if !VERBES.contains(&verbe) {
        eprintln!("[neuradesk][cef] verbe de sous-dock inconnu : {action:?}");
        return None;
    }
    // ⛔ **UN ARGUMENT BLANC EST REFUSÉ** — vide, ou réduit à des espaces. Il
    // n'arrive pas depuis notre page (elle n'envoie que des chemins reçus de
    // Rust), mais `read_dir("")` comme `read_dir(" ")` échouent : la moitié
    // afficherait une grille VIDE sans un mot, alors qu'elle montrait quelque
    // chose — l'inverse de la discipline du reste du module (`Tri::depuis_cle`
    // refuse une clé inconnue, `config::chemin_retenu` refuse un chemin relatif,
    // et pour la raison écrite là-bas).
    //
    // ⚠️ **ON TESTE `trim()`, ON NE ROGNE PAS LA VALEUR** : un chemin qui
    // commencerait ou finirait par une espace est légal sur Windows, et le
    // tronquer viserait un autre dossier que celui qu'on a reçu. On refuse le
    // blanc, on ne réécrit pas les chemins.
    if argument.as_deref().is_some_and(|a| a.trim().is_empty()) {
        eprintln!("[neuradesk][cef] demande sous-dock a argument blanc : {action:?}");
        return None;
    }
    match (verbe, argument) {
        ("entrer", Some(chemin)) => Some(Demande::Entrer(cote, PathBuf::from(chemin))),
        ("ouvrir", Some(chemin)) => Some(Demande::Ouvrir(PathBuf::from(chemin))),
        ("remonter", _) => Some(Demande::Remonter(cote)),
        ("actualiser", _) => Some(Demande::Actualiser(cote)),
        ("explorateur", _) => Some(Demande::Explorateur(cote)),
        ("epingler", _) => Some(Demande::Epingler(cote)),
        ("desepingler", _) => Some(Demande::Desepingler(cote)),
        ("trier", Some(cle)) => match Tri::depuis_cle(&cle) {
            Some(tri) => Some(Demande::Trier(cote, tri)),
            // ⛔ **UNE CLÉ INCONNUE NE RANGE RIEN.** Se rabattre sur le tri
            // par défaut ferait changer l'ordre de la liste à cause d'une
            // faute de frappe dans la page — et un changement d'ordre
            // silencieux est le genre de défaut qu'on n'attribue pas à sa
            // cause. On le dit, et l'état en cours ne bouge pas.
            None => {
                eprintln!("[neuradesk][cef] tri inconnu : {cle:?}");
                None
            }
        },
        // ---- LES GESTES DU MENU CONTEXTUEL D'UN FICHIER --------------------
        //
        // ⚠️ **LE CHEMIN EST DÉJÀ DÉCODÉ, ET ON NE LE VALIDE PAS ICI.** Il vient
        // d'une case que Rust a lui-même remplie ; le valider une seconde fois
        // ferait diverger deux règles le jour où l'une bougerait. Ce qui compte
        // est ailleurs : chaque action REND UN COMPTE, et un refus se dit
        // (voir `win32::actions_fichiers`, dont toutes les fonctions rendent un
        // `Result` dont le message est montré).
        ("afficher", Some(chemin)) => Some(Demande::Afficher(cote, PathBuf::from(chemin))),
        ("aide-video", _) => Some(Demande::AideVideo),
        ("ouvrir-avec", Some(chemin)) => Some(Demande::OuvrirAvec(cote, PathBuf::from(chemin))),
        ("copier-chemin", Some(chemin)) => Some(Demande::CopierChemin(cote, PathBuf::from(chemin))),
        ("proprietes", Some(chemin)) => Some(Demande::Proprietes(cote, PathBuf::from(chemin))),
        ("copier", Some(chemins)) => Some(Demande::CopierFichier(cote, plusieurs_chemins(&chemins))),
        ("couper", Some(chemins)) => Some(Demande::CouperFichier(cote, plusieurs_chemins(&chemins))),
        ("supprimer", Some(chemin)) => Some(Demande::Supprimer(cote, PathBuf::from(chemin))),
        ("nouveau-dossier", Some(nom)) => Some(Demande::NouveauDossier(cote, nom)),
        ("nouveau-fichier", Some(nom)) => Some(Demande::NouveauFichier(cote, nom)),
        ("coller", dossier) => Some(Demande::Coller(cote, dossier.map(PathBuf::from))),
        // ⛔ **LE NOUVEAU NOM SE LIT ICI, ET PAS DANS LE `match`.** Le quatrième
        // segment n'appartient qu'à ce verbe : faire porter trois éléments à
        // TOUS les bras obligerait à écrire `_` quinze fois, pour une valeur que
        // quinze d'entre eux n'ont pas.
        //
        // ⚠️ **UN NOUVEAU NOM ABSENT EST REFUSÉ** : renommer « vers rien » n'a
        // pas de sens, et deviner (garder le nom ? l'effacer ?) serait pire que
        // de refuser.
        ("renommer", Some(chemin)) => match complement {
            Some(nom) => Some(Demande::Renommer(cote, PathBuf::from(chemin), nom)),
            None => {
                eprintln!("[neuradesk][cef] renommer sans nouveau nom : {action:?}");
                None
            }
        },
        _ => {
            eprintln!("[neuradesk][cef] demande sous-dock illisible : {action:?}");
            None
        }
    }
}

/// Vide les demandes de la page. **UNE SEULE EST RENDUE** — la dernière lue : la
/// page peut en émettre plusieurs entre deux frames (un clic puis un autre), et
/// c'est la plus récente qui décrit l'état voulu.
///
/// ⚠️ **MAIS ON DIT QUAND ON EN JETTE.** Une demande valide écrasée par une plus
/// récente disparaissait **sans une ligne** : le jour où le geste perdu serait
/// celui que l'utilisateur attendait, il n'y aurait aucun moyen de le savoir —
/// et c'est le seul diagnostic possible ici. Le journal est donc le seul endroit
/// où cette perte peut devenir croyable.
pub fn dispatch_actions() -> Option<Demande> {
    let mut resultat = None;
    let mut vues = 0usize;
    for action in cef_popup::drain_actions(&SLOT) {
        // La croix : globale au panneau, sans côté — elle ne passe donc pas par
        // `demande_depuis_action`, qui en exige un.
        if action == "sous-dock/close" {
            cef_popup::fermer_par_geste(&SLOT);
            continue;
        }
        if let Some(demande) = demande_depuis_action(&action) {
            if resultat.is_some() {
                vues += 1;
            }
            resultat = Some(demande);
        } else if !action.starts_with("sous-dock/") {
            eprintln!("[neuradesk][cef] action sous-dock inconnue : {action:?}");
        }
    }
    if vues > 0 {
        crate::diag!(
            "[sous-dock] {vues} demande(s) valide(s) jetee(s) : une seule frame en portait plusieurs, \
             et c'est la DERNIERE qui s'applique"
        );
    }
    resultat
}

pub fn render_if_visible(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::render_if_visible(
        &SLOT,
        tint_rgba,
        // ⚠️ **LE RAYON DE MENU EST GARDÉ, LA PALETTE NON** — les deux
        // paramètres sont INDÉPENDANTS (voir `cef_popup::render_if_visible`), et
        // c'est ici qu'ils divergent : ce panneau FLOTTE au-dessus du bureau, à
        // côté de sa vignette, donc il garde les coins arrondis du fond flouté
        // comme un menu. La page dessine elle-même une boîte arrondie
        // (`border-radius`), et un coin carré derrière elle laisserait voir
        // l'écart.
        crate::win32::menu::menu_corner_radius(),
        // ⛔ **`Window` ET NON `Menu` — C'EST LA MOITIÉ QUI MANQUAIT, ET ELLE A
        // ÉTÉ MESURÉE.**
        //
        // Les drapeaux du `SPEC` ci-dessus (« PANNEAU, PAS MENU ») ne suffisent
        // PAS : `content_factor` n'est posé QUE par le bras `Menu` de
        // `cef_popup::render`, et la doc de ce champ dit « `0.0` = hauteur
        // pilotée par la page INTERDITE » — c'est-à-dire qu'un panneau ignore la
        // taille que sa page annonce. Avec `Menu`, le facteur valait
        // `1.2^zoom = 2,7`, lu au journal :
        //
        //   `[menu/size] recu w=Some(253.0) h=Some(39.0) | facteur=2.6999… -> applique=true`
        //
        // soit exactement la taille d'une boîte VIDE (`min-width: 240px` de la
        // page + son seul en-tête) : la page RÉTRÉCISSAIT le panneau au lieu de
        // le laisser occuper la zone libre, et l'ancien rectangle de 64 px
        // n'était donc qu'un cas particulier du même défaut.
        cef_popup::PopupPalette::Window { accent_rgb, dark_mode },
    );
}

pub fn debug_dump_backbuffer(path: &Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(result) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// **GARDE-FOU D'UN DÉFAUT ENTIÈREMENT SILENCIEUX — NE PAS LE RETIRER.**
///
/// L'expression envoyée à la page doit être du **JavaScript valide**, et rien
/// dans la chaîne d'exécution ne le dira jamais : `cef_popup::execute_js` ne rend
/// aucun compte de son échec (voir sa doc), et un script **rejeté à la
/// compilation** n'appelle aucune fonction, n'écrit aucune trace, ne touche aucun
/// compteur. C'est exactement ce qui s'est produit le 2026-09-29 : `push_state`
/// retirait l'accolade fermante du JSON (`json.trim_end_matches('}')`) pour y
/// greffer `open` — **sans jamais la remettre** — et produisait
///
///   `window.setState(Object.assign({"dossier":…,"tronque":false, {open: 1}))`
///
/// soit un `SyntaxError`. Une journée entière a été passée à mettre en cause le
/// canal, la page, la taille du message et un témoin à l'écran, alors que le
/// défaut tenait dans les **VINGT DERNIERS caractères** — et que la sonde du
/// journal n'en montrait que les 220 premiers.
///
/// Ce test marche donc l'expression comme un analyseur : profondeur des accolades
/// et des parenthèses, littéraux de chaîne refermés. C'est précisément ce qu'un
/// `SyntaxError` de cette famille casse, et il n'y a aucun autre moyen de le voir.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_selection_arrive_en_plusieurs_chemins() {
        assert_eq!(
            plusieurs_chemins("C:\\a b\\x.txt\nC:\\Dossier\n\n"),
            [PathBuf::from(r"C:\a b\x.txt"), PathBuf::from(r"C:\Dossier")]
        );
        assert_eq!(plusieurs_chemins(r"C:\seul"), [PathBuf::from(r"C:\seul")]);
    }

    /// `(accolades, parentheses, chaine_restee_ouverte)` à la fin de `js`.
    fn structure(js: &str) -> (i32, i32, bool) {
        let chars: Vec<char> = js.chars().collect();
        let (mut acc, mut par) = (0i32, 0i32);
        let mut dans = false;
        let mut i = 0usize;
        while i < chars.len() {
            let c = chars[i];
            if dans {
                // `\` saute le caractère SUIVANT : c'est ce qui rend `\"` et
                // `\\` inoffensifs, et sans quoi tout comptage serait faux dès
                // le premier chemin Windows.
                if c == '\\' {
                    i += 2;
                    continue;
                }
                if c == '"' {
                    dans = false;
                }
            } else {
                match c {
                    '"' => dans = true,
                    '{' => acc += 1,
                    '}' => acc -= 1,
                    '(' => par += 1,
                    ')' => par -= 1,
                    _ => {}
                }
            }
            i += 1;
        }
        (acc, par, dans)
    }

    /// **LE TEST QUI AURAIT ÉVITÉ LA JOURNÉE PERDUE.**
    #[test]
    fn l_expression_envoyee_a_la_page_est_du_javascript_valide() {
        for json in [
            r#"{"dossier":"C:\\Users\\utilisateur","tronque":false,"enfants":[{"nom":"a b","dossier":true}]}"#,
            // Les valeurs qui doivent rester SANS EFFET sur le comptage : une
            // accolade et des guillemets DANS une chaîne, une barre oblique
            // inversée en fin de chemin, une grille vide.
            r#"{"dossier":"C:\\a{curly}\"quote\"","tronque":true,"enfants":[]}"#,
            r#"{"dossier":"C:\\fin\\","tronque":false,"enfants":[]}"#,
        ] {
            let js = expression_js(json);
            assert!(js.contains(json), "le JSON doit être passé COMPLET : {js}");
            assert_eq!(
                structure(&js),
                (0, 0, false),
                "expression JS invalide (accolades, parenthèses, littéral ouvert) pour {json} : {js}"
            );
        }

        // **ET LE DÉFAUT QUI A COÛTÉ LA JOURNÉE EST BIEN ATTRAPÉ.** Un test dont
        // on n'a jamais vu l'échec ne prouve rien : on refait ici la version
        // fautive — l'accolade fermante retirée par `trim_end_matches` — et on
        // exige que le comptage la REFUSE. Sans cette assertion, rien ne
        // garantirait que `structure` mesure ce qu'on croit.
        let fautif = format!(
            "window.setState(Object.assign({}, {{open: 7}}))",
            r#"{"tronque":false}"#.trim_end_matches('}')
        );
        assert_ne!(
            structure(&fautif),
            (0, 0, false),
            "le defaut d'origine doit etre ATTRAPE : {fautif}"
        );
    }

    /// **L'ÉTAT À DEUX MOITIÉS EST DU JAVASCRIPT VALIDE** — et c'est une
    /// PROLONGATION du test précédent, pas un doublon : le JSON est maintenant
    /// plus profond d'un cran, il porte DEUX grilles, et surtout **le nom d'un
    /// fichier de l'utilisateur peut contenir une accolade ou un guillemet**.
    /// C'est cette valeur-là qui casserait l'analyseur, et elle vient du disque,
    /// pas de nous.
    #[test]
    fn l_expression_a_deux_moities_est_du_javascript_valide() {
        let gauche = r#"{"dossier":"C:\\a","nom":"a","revision":3,"enfants":[{"nom":"total {1}","chemin":"C:\\a\\x"}]}"#;
        let droite = r#"{"dossier":"C:\\b\"fin","nom":"b","revision":0,"enfants":[]}"#;
        let js = expression_js(&composer(gauche, droite, 12, 1.25, 2));
        assert_eq!(
            structure(&js),
            (0, 0, false),
            "l'etat a deux moities doit etre du JS valide : {js}"
        );
        // Et il porte bien les DEUX moitiés, chacune intacte.
        assert!(js.contains(gauche), "la moitie gauche doit passer entiere : {js}");
        assert!(js.contains(droite), "la moitie droite doit passer entiere : {js}");
        assert!(js.contains(r#""open":12"#), "le numero d'ouverture voyage dans l'objet : {js}");
    }

    /// **FAUT-IL REFERMER LE PANNEAU ?** — les QUATRE cas, et pas seulement celui
    /// qui a motivé la punaise.
    ///
    /// ⚠️ **C'EST LE SEUL TEST DE CE LOT QUI PORTE SUR LA RÈGLE ELLE-MÊME** : la
    /// fermeture vit dans `app.rs`, dans la boucle de rendu, et aucune entrée n'est
    /// simulée dans ce dépôt — donc elle ne s'éprouve QUE par cette fonction pure.
    /// C'est exactement pourquoi elle a été extraite.
    #[test]
    fn la_punaise_empeche_de_refermer_le_panneau() {
        // Le curseur sur la cible : on ne referme jamais, punaise ou pas.
        assert!(!doit_fermer(false, true, false));
        assert!(!doit_fermer(false, false, true));
        assert!(!doit_fermer(true, true, false));
        assert!(!doit_fermer(true, false, true));

        // Le cas qui compte : le curseur est AILLEURS…
        assert!(doit_fermer(false, false, false), "sans punaise, on referme");
        assert!(
            !doit_fermer(true, false, false),
            "AVEC la punaise, le panneau reste ouvert — c'est tout son sens"
        );
    }

    /// ⛔ **UN PANNEAU DÉJÀ OUVERT NE SE BASCULE PAS — IL SE REMPLIT.**
    ///
    /// C'est le défaut signalé : « le premier s'ouvre dans le dash et le passage sur
    /// le second ferme le dash au lieu d'actualiser le dash pour le dossier
    /// survolé ». `ouvrir` est rappelé à CHAQUE changement de vignette survolée, et
    /// `cef_popup::toggle` masque ce qui est visible.
    ///
    /// ⚠️ **ET LE TEST MONTRE OÙ LA RÈGLE D'AVANT DIVERGEAIT** : elle ne différait
    /// que par la PUNAISE — `!(maintenu && visible)` —, si bien qu'un panneau ouvert
    /// mais NON épinglé était quand même basculé, donc refermé. C'était le cas
    /// ordinaire, c'est-à-dire celui de tout le monde.
    #[test]
    fn un_panneau_deja_ouvert_ne_se_bascule_pas() {
        assert!(doit_basculer(false), "panneau ferme : on l'ouvre");
        assert!(!doit_basculer(true), "panneau DEJA OUVERT : on le remplit, on ne le bascule pas");

        // **LA RÈGLE D'AVANT, RÉÉCRITE POUR VOIR LA DIVERGENCE.**
        let ancienne = |maintenu: bool, visible: bool| !(maintenu && visible);
        assert!(
            ancienne(false, true),
            "l'ancienne regle basculait un panneau OUVERT et NON EPINGLE — donc le fermait"
        );
        assert_ne!(
            doit_basculer(true),
            ancienne(false, true),
            "les deux regles DOIVENT diverger sur ce cas : c'est tout le correctif"
        );
        // Et sur les deux autres cas, elles s'accordent — le correctif ne change donc
        // que ce qu'il devait changer.
        assert_eq!(doit_basculer(false), ancienne(false, false));
        assert_eq!(doit_basculer(false), ancienne(true, false));
    }

    /// ⛔ **LE CÔTÉ EST DÉCODÉ, ET UN CÔTÉ INCONNU EST REFUSÉ.** Replier sur la
    /// gauche ferait bouger la mauvaise moitié, en silence — c'est le genre de
    /// défaut qu'on met une heure à croire quand on le voit à l'écran.
    #[test]
    fn le_cote_est_decode_et_non_devine() {
        for cote in Cote::TOUTES {
            let action = format!("sous-dock/{}/remonter", cote.cle());
            assert_eq!(demande_depuis_action(&action), Some(Demande::Remonter(cote)));
        }
        assert_eq!(demande_depuis_action("sous-dock/gauche/remonter"), None);
        assert_eq!(demande_depuis_action("sous-dock/remonter"), None, "l'ancienne forme a DEUX segments : elle doit etre refusee, pas devinee");
        assert_eq!(demande_depuis_action("sous-dock//remonter"), None);
    }

    /// **LE CHEMIN TRAVERSE ENTIER** — avec ses espaces et ses accents, et il
    /// ressort DÉCODÉ. Trois segments découpés au mauvais endroit rogneraient le
    /// chemin sans rien dire.
    ///
    /// ⚠️ **L'ENCODAGE EST ÉCRIT EN CLAIR, ET C'EST VOULU** : il vient
    /// d'`encodeURIComponent`, côté page, qui n'a pas d'équivalent Rust dans ce
    /// dépôt — un `percent_encode` de test ne prouverait que sa propre
    /// existence. Ce que ce test doit attraper, c'est un `splitn` qui coupe au
    /// mauvais `/`, et des `%XX` multi-octets (le « É » est `%C3%89`) laissés
    /// intacts.
    #[test]
    fn le_chemin_traverse_entier_et_decode() {
        let chemin = r"C:\Users\utilisateur\Mes documents\Été";
        let encode = "C%3A%5CUsers%5Cutilisateur%5CMes%20documents%5C%C3%89t%C3%A9";
        assert_eq!(
            demande_depuis_action(&format!("sous-dock/g/entrer/{encode}")),
            Some(Demande::Entrer(Cote::Gauche, PathBuf::from(chemin)))
        );
        assert_eq!(
            demande_depuis_action(&format!("sous-dock/d/entrer/{encode}")),
            Some(Demande::Entrer(Cote::Droite, PathBuf::from(chemin)))
        );
    }

    /// **LES LITTÉRAUX QUE LA PAGE ENVOIE VRAIMENT** — ceux qui sont passés à
    /// `go(…)`, et non ceux qu'elle se contente de citer.
    ///
    /// ⛔ **C'EST LA CORRECTION DU TEST, ET ELLE COMPTE.** La version précédente
    /// cherchait `"verbe"` n'importe où dans le fichier : un verbe qui ne
    /// survivrait que dans un COMMENTAIRE entre guillemets — ou dans une phrase
    /// d'infobulle — satisfaisait le garde alors que la page ne l'envoyait plus.
    /// Le test passait donc au vert sur précisément ce qu'il prétendait
    /// surveiller. Ici on lit les arguments réels des appels.
    ///
    /// ⚠️ Les appels de `go` de cette page n'imbriquent aucune parenthèse (on y
    /// passe des chemins ou des clés, jamais des expressions parenthésées) : la
    /// découpe au premier `)` est donc exacte pour ce fichier-ci, et le test le
    /// vérifie de toute façon dans les deux sens — un littéral non reconnu le
    /// fait échouer.
    fn litteraux_envoyes(page: &str) -> Vec<String> {
        let mut trouves = Vec::new();
        let mut reste = page;
        while let Some(position) = reste.find("go(") {
            let apres = &reste[position + 3..];
            let Some(fin) = apres.find(')') else { break };
            // Les morceaux d'indice IMPAIR sont ceux qui sont ENTRE guillemets.
            for morceau in apres[..fin].split('"').skip(1).step_by(2) {
                trouves.push(morceau.to_string());
            }
            reste = &apres[fin + 1..];
        }
        trouves
    }

    /// ⛔ **LE CONTRAT PAGE ↔ DISPATCH, VÉRIFIÉ DANS LES DEUX SENS.** Le dépôt a
    /// déjà été mordu par un verbe ajouté d'un seul côté (voir
    /// `cef_surveillance`). Ici on exige que `VERBES` soit exactement ce que le
    /// décodeur accepte, ET que la page envoie chacun d'eux — ET rien d'autre.
    /// La croix du panneau envoie l'action que `dispatch_actions` intercepte.
    #[test]
    fn la_croix_envoie_l_action_de_fermeture() {
        let page = include_str!("../../assets/popups/sous_dock.html");
        assert!(page.contains("\"neuradesk://sous-dock/close\""), "la croix de sous_dock.html n'envoie pas `sous-dock/close`");
    }

    #[test]
    fn les_verbes_de_la_page_sont_exactement_ceux_du_dispatch() {
        // Un argument VALIDE par verbe, et **`renommer` en a DEUX** — c'est le
        // seul. Lui en donner un seul ferait passer un verbe correct pour un
        // verbe absent du `match`, et le test accuserait le mauvais coupable.
        let action = |verbe: &str| match verbe {
            "trier" => "sous-dock/g/trier/nom".to_string(),
            "renommer" => "sous-dock/g/renommer/C%3A%5Cessai/nouveau.txt".to_string(),
            _ => format!("sous-dock/g/{verbe}/C%3A%5Cessai"),
        };
        // Sens 1 : tout verbe déclaré est DÉCODÉ par le dispatch.
        for verbe in VERBES {
            let action = action(verbe);
            assert!(
                demande_depuis_action(&action).is_some(),
                "le verbe {verbe:?} est declare mais le dispatch ne le decode pas ({action})"
            );
        }

        // Sens 2 : la page ENVOIE chacun d'eux.
        let page = include_str!("../../assets/popups/sous_dock.html");
        let envoyes = litteraux_envoyes(page);
        for verbe in VERBES {
            assert!(
                envoyes.iter().any(|envoye| envoye == verbe),
                "le verbe {verbe:?} est declare cote Rust mais la page ne l'envoie jamais \
                 (elle peut le citer en commentaire — ce n'est pas un envoi). Envoyes : {envoyes:?}"
            );
        }

        // Sens 3 : **LA PAGE N'ENVOIE RIEN QUE LE DISPATCH NE CONNAÎT.** Sans ce
        // sens, un verbe inventé côté page passerait inaperçu : le décodeur le
        // refuserait, et le geste serait muet à l'écran sans que rien ne le dise.
        for envoye in &envoyes {
            let connu = VERBES.contains(&envoye.as_str())
                || Cote::TOUTES.into_iter().any(|cote| cote.cle() == envoye)
                // ⚠️ **LE SIGNAL DE VIE EST UNE TROISIÈME SORTE DE LITTÉRAL** : ni
                // un verbe (il ne porte ni côté ni argument, et ne rend aucune
                // `Demande`), ni un côté. Sans cette ligne, ajouter `go("ready")` à
                // la page ferait échouer ce test — et c'est exactement ce qu'on
                // veut de lui : il force à décider dans quelle catégorie tombe ce
                // qu'on vient d'ajouter.
                || envoye == SIGNAL_PRET
                // Le zoom, même catégorie : il vaut pour tout le panneau.
                || envoye == SIGNAL_ZOOM;
            assert!(connu, "la page envoie {envoye:?} a `go`, qui n'est ni un verbe connu ni un cote");
        }

        // Sens 4 : un verbe INVENTÉ est refusé — sinon les sens ci-dessus
        // passeraient encore si le `match` acceptait tout par un joker.
        assert_eq!(demande_depuis_action("sous-dock/g/invente"), None);
        // Et un argument VIDE est refusé : `read_dir("")` échouerait, et la
        // moitié afficherait une grille vide sans un mot.
        assert_eq!(demande_depuis_action("sous-dock/g/voir/"), None);
        assert_eq!(demande_depuis_action("sous-dock/g/entrer/%20"), None);
    }

    /// ⛔ **LES ICÔNES NE PARTENT PAS AVEC L'ÉTAT.**
    ///
    /// C'est LA correction de fond d'un lot précédent : au journal, **743 Ko par
    /// poussée** (gauche 289 Ko, droite 453 Ko), dont ~99 % d'icônes, dans un état
    /// repoussé soixante fois par ouverture. Ce test refuse la régression — et
    /// c'est le geste qu'on ferait en croyant simplifier : remettre les images
    /// dans `moitie_json` pour n'avoir « qu'un seul canal ».
    ///
    /// ⚠️ Depuis que `EnfantPret` ne porte plus d'icône DU TOUT, ce test ne peut
    /// plus en placer une pour prouver qu'elle est écartée : il vérifie donc la
    /// FORME de l'état — aucun `icone`, aucun `data:`, mais le chemin bien là.
    #[test]
    fn les_icones_ne_partent_pas_avec_l_etat() {
        let contenu = Contenu {
            tronque: false,
            enfants: vec![EnfantPret {
                nom: "une nappe".to_string(),
                chemin: r"C:\essai\une nappe".to_string(),
                dossier: true,
            }],
        };
        let moitie = Moitie {
            dossier: PathBuf::from(r"C:\essai"),
            tri: Tri::Nom,
            revision: 1,
            json: String::new(),
        };
        let json = moitie_json(&moitie, &contenu, &[], None);
        assert!(!json.contains("icone"), "l'etat ne doit porter AUCUNE icone : {json}");
        assert!(!json.contains("data:image"), "ni le moindre data: URI : {json}");
        assert!(
            json.contains(r"C:\\essai\\une nappe"),
            "mais bien le CHEMIN, qui est la cle du cache d'icones de la page : {json}"
        );
        // Et l'état reste du JavaScript valide — c'est le garde-fou d'origine.
        assert_eq!(structure(&expression_js(&json)), (0, 0, false));
    }

    /// ⭐ « Voir l'aide » sur une vidéo sans VLC (demande du 2026-10-19) : l'état
    /// dit quels fichiers sont des vidéos — un dossier jamais — et si VLC est
    /// installé ; la page offre l'aide à la place d'« Afficher » ; le verbe
    /// ouvre l'aide.
    #[test]
    fn une_video_sans_vlc_renvoie_a_l_aide() {
        let enfant = |nom: &str, dossier: bool| EnfantPret { nom: nom.to_string(), chemin: format!(r"C:\essai\{nom}"), dossier };
        let contenu = Contenu { tronque: false, enfants: vec![enfant("film.MP4", false), enfant("notes.txt", false), enfant("rushes.mkv", true)] };
        let moitie = Moitie { dossier: PathBuf::from(r"C:\essai"), tri: Tri::Nom, revision: 1, json: String::new() };
        let json: serde_json::Value = serde_json::from_str(&moitie_json(&moitie, &contenu, &[], None)).unwrap();
        let videos: Vec<bool> = json["enfants"].as_array().unwrap().iter().map(|e| e["video"].as_bool().unwrap()).collect();
        assert_eq!(videos, [true, false, false], "un dossier nommé comme une vidéo n'en est pas une");
        assert_eq!(json["vlc"].as_bool(), Some(crate::win32::lecteur_video::vlc_installe()));
        assert!(matches!(demande_depuis_action("sous-dock/g/aide-video"), Some(Demande::AideVideo)));
        let page = include_str!("../../assets/popups/sous_dock.html");
        assert!(page.contains(r#"boite.dataset.video === "1" && !(moitie && moitie.vlc)"#), "la page lit les deux drapeaux");
        let aide = include_str!("../../assets/popups/aide.html");
        assert!(aide.contains(r#"<h2 id="vlc">"#), "la section que le verbe ouvre existe");
    }

    /// **L'EXPRESSION D'UN LOT D'ICÔNES EST DU JAVASCRIPT VALIDE** — même
    /// garde-fou qu'`expression_js`, et il compte davantage ici : elle transporte
    /// des CHEMINS D'UTILISATEUR, donc des accolades, des guillemets et des
    /// barres obliques inversées.
    #[test]
    fn l_expression_des_icones_est_du_javascript_valide() {
        let lot = vec![
            (r"C:\a{curly}\quote".to_string(), Some("data:image/png;base64,AA==".to_string())),
            (r"C:\fin\".to_string(), None),
        ];
        let js = expression_icones(&lot);
        assert!(js.starts_with("window.setIcons("), "{js}");
        assert_eq!(structure(&js), (0, 0, false), "expression invalide : {js}");
        // ⛔ **`null` DOIT VOYAGER.** Une entrée sans icône se transmet QUAND
        // MÊME : c'est ce qui dit à la page de ne plus la redemander.
        assert!(js.contains("null"), "une entree sans icone doit voyager quand meme : {js}");
    }

    /// ⛔ **UN CHEMIN N'EST MIS EN FILE QU'UNE FOIS.**
    ///
    /// Deux moitiés qui montrent le même dossier, ou un dossier relu après un
    /// geste : sans ce carnet, le même chemin repartirait autant de fois, et
    /// chaque passage paierait une extraction de shell pour rien.
    #[test]
    fn un_chemin_n_est_mis_en_file_qu_une_fois() {
        ICONES_A_ENVOYER.with(|f| f.borrow_mut().clear());
        ICONES_ENVOYEES.with(|e| e.borrow_mut().clear());
        let contenu = Contenu {
            tronque: false,
            enfants: vec![
                EnfantPret { nom: "a".into(), chemin: r"C:\a".into(), dossier: false },
                EnfantPret { nom: "b".into(), chemin: r"C:\b".into(), dossier: false },
            ],
        };

        mettre_en_file_les_icones(&contenu);
        assert_eq!(ICONES_A_ENVOYER.with(|f| f.borrow().len()), 2, "les DEUX enfants");
        // ⭐ **C'EST LE CHEMIN QUI EST EN FILE, PAS L'ICÔNE** — c'est ce qui permet
        // de lister un dossier entier sans rien calculer à l'ouverture.
        assert_eq!(
            ICONES_A_ENVOYER.with(|f| f.borrow().front().cloned()),
            Some(r"C:\a".to_string())
        );
        // Deuxième passage — le même dossier relu, ou l'autre moitié qui le montre.
        mettre_en_file_les_icones(&contenu);
        assert_eq!(
            ICONES_A_ENVOYER.with(|f| f.borrow().len()),
            2,
            "un chemin deja en file ne se remet pas"
        );
        // Un enfant NEUF, lui, part.
        let mut avec_un_neuf = contenu.clone();
        avec_un_neuf
            .enfants
            .push(EnfantPret { nom: "c".into(), chemin: r"C:\c".into(), dossier: false });
        mettre_en_file_les_icones(&avec_un_neuf);
        assert_eq!(ICONES_A_ENVOYER.with(|f| f.borrow().len()), 3);

        ICONES_A_ENVOYER.with(|f| f.borrow_mut().clear());
        ICONES_ENVOYEES.with(|e| e.borrow_mut().clear());
    }

    /// ⛔ **OUBLIER LA FILE DÉMARQUE AUSSI — SINON ON PERD DES ICÔNES.**
    ///
    /// Un chemin est marqué « parti » **au moment de l'empiler**, pas à l'envoi
    /// (voir `mettre_en_file_les_icones`). Vider la file sans le démarquer ferait
    /// tenir pour partie une icône qui n'est JAMAIS partie : la page garderait un
    /// carré pointillé jusqu'à la prochaine ouverture, et rien ne le dirait. C'est
    /// le piège que ce test tient fermé — le seul endroit où il se verrait.
    #[test]
    fn oublier_la_file_permet_de_remettre_les_icones_en_file() {
        ICONES_A_ENVOYER.with(|f| f.borrow_mut().clear());
        ICONES_ENVOYEES.with(|e| e.borrow_mut().clear());
        let contenu = Contenu {
            tronque: false,
            enfants: vec![EnfantPret { nom: "a".into(), chemin: r"C:\a".into(), dossier: false }],
        };

        mettre_en_file_les_icones(&contenu);
        assert_eq!(ICONES_A_ENVOYER.with(|f| f.borrow().len()), 1);

        oublier_la_file_d_icones();
        assert_eq!(ICONES_A_ENVOYER.with(|f| f.borrow().len()), 0, "la file est videe");

        // ⭐ **LE POINT DU TEST** : le chemin est REDEMANDABLE.
        mettre_en_file_les_icones(&contenu);
        assert_eq!(
            ICONES_A_ENVOYER.with(|f| f.borrow().len()),
            1,
            "un chemin oublie doit pouvoir repartir — sinon la case reste vide a jamais"
        );

        ICONES_A_ENVOYER.with(|f| f.borrow_mut().clear());
        ICONES_ENVOYEES.with(|e| e.borrow_mut().clear());
    }

    /// ⭐ **REVENIR DANS UN DOSSIER DÉJÀ VISITÉ DOIT RENVOYER SES ICÔNES.**
    ///
    /// C'est le défaut signalé, mot pour mot : « lorsqu'un dossier a déjà été
    /// visité et que j'y retourne, il n'y a plus les icônes de dossiers et de
    /// fichiers, comme si elles étaient transparentes ».
    ///
    /// La page élague ses icônes en même temps que l'état cesse de les afficher
    /// (voir `elaguerLesIcones`, dans `sous_dock.html`). Le carnet de Rust, lui, ne
    /// les oubliait pas — et comme `mettre_en_file_les_icones` saute tout chemin
    /// déjà marqué, il refusait de les renvoyer. Ce test tient les deux bouts
    /// ensemble : sur disque, parce que c'est `contenu` qui décide et qu'il lit
    /// vraiment.
    #[test]
    fn revenir_dans_un_dossier_deja_visite_renvoie_ses_icones() {
        let racine = std::env::temp_dir().join(format!("nd-va-et-vient-{}", std::process::id()));
        let (a, b) = (racine.join("a"), racine.join("b"));
        for (dossier, nom) in [(&a, "dans-a.txt"), (&b, "dans-b.txt")] {
            let _ = std::fs::create_dir_all(dossier);
            std::fs::write(dossier.join(nom), b"x").expect("ecriture");
        }
        let chemin_a = a.join("dans-a.txt").to_string_lossy().into_owned();

        let nettoyer = || {
            ICONES_A_ENVOYER.with(|f| f.borrow_mut().clear());
            ICONES_ENVOYEES.with(|e| e.borrow_mut().clear());
            CACHE.with(|c| c.borrow_mut().clear());
        };
        nettoyer();

        // On ouvre A, puis on « envoie » tout — c'est ce qui marque les chemins.
        changer_sans_effacer(Cote::Gauche, &a);
        ICONES_A_ENVOYER.with(|f| f.borrow_mut().clear());

        // On part dans B : c'est là que la page élague les icônes de A.
        changer_sans_effacer(Cote::Gauche, &b);
        ICONES_A_ENVOYER.with(|f| f.borrow_mut().clear());

        // ⭐ **ET ON REVIENT.** L'icône de A doit être REDEMANDÉE.
        changer_sans_effacer(Cote::Gauche, &a);
        let remis: Vec<String> = ICONES_A_ENVOYER.with(|f| f.borrow().iter().cloned().collect());
        assert!(
            remis.iter().any(|c| *c == chemin_a),
            "revenir dans A doit remettre son icone en file — sinon la case reste vide, \
             ce qui est exactement le defaut signale. Remis : {remis:?}"
        );

        avec(Cote::Gauche, |m| *m = Moitie::neuve());
        nettoyer();
        let _ = std::fs::remove_dir_all(&racine);
    }

    /// **LE CHEMIN QUI N'EXISTE PAS NE FAIT PAS PANIQUER L'EXTRACTION.** Un
    /// dossier épinglé peut avoir été supprimé, un lecteur retiré : la case
    /// s'affichera en carré pointillé, et c'est tout.
    #[test]
    fn l_icone_d_un_chemin_inexistant_est_absente_sans_paniquer() {
        // ⚠️ Un `.png` : c'est une IMAGE sans vignette, donc elle est à redemander.
        assert_eq!(icone_du_chemin(r"C:\ce\chemin\n'existe\pas\du\tout.png"), (None, true));
    }

    /// ⭐ **UNE IMAGE SANS VIGNETTE EST REDEMANDÉE — TANT QU'ELLE EST AFFICHÉE, ET
    /// PAS INDÉFINIMENT.** C'est la réparation de « je suis obligé d'ouvrir 2 fois
    /// le même dossier » : sans relance, l'icône générique restait pour de bon.
    #[test]
    fn une_vignette_manquante_est_redemandee_tant_qu_elle_est_affichee() {
        let (affichee, quittee) = ("C:\\relance\\affichee.jpg".to_string(), "C:\\relance\\quittee.jpg".to_string());
        ICONES_ENVOYEES.with(|e| e.borrow_mut().insert(affichee.clone()));
        let passe = std::time::Instant::now() - std::time::Duration::from_secs(1);
        RELANCES.with(|r| {
            let mut r = r.borrow_mut();
            r.push((affichee.clone(), passe, 2));
            r.push((quittee.clone(), passe, 1));
        });
        let echues = relances_echues();
        assert_eq!(echues.get(&affichee), Some(&2), "l'affichée repart, avec son compte d'essais");
        assert!(!echues.contains_key(&quittee), "celle d'un dossier quitté est jetée");
        let en_file: Vec<String> = ICONES_A_ENVOYER.with(|f| f.borrow().iter().cloned().collect());
        assert!(en_file.contains(&affichee) && !en_file.contains(&quittee));
        // Le dernier essai n'en programme plus.
        programmer_relance(affichee.clone(), RELANCES_MAX);
        assert!(RELANCES.with(|r| r.borrow().is_empty()), "pas de relance au-delà de {RELANCES_MAX}");
        programmer_relance(affichee.clone(), 0);
        assert_eq!(RELANCES.with(|r| r.borrow().len()), 1);
        // Nettoyage : ce fil de test ne doit rien laisser aux suivants.
        RELANCES.with(|r| r.borrow_mut().clear());
        ICONES_A_ENVOYER.with(|f| f.borrow_mut().clear());
        ICONES_ENVOYEES.with(|e| e.borrow_mut().remove(&affichee));
    }

    /// ⭐ **LE CAS DU `.jar`, REJOUÉ SANS SHELL NI DISQUE.**
    ///
    /// C'est la réduction de la mesure faite sur un vrai fichier Java : un canvas de
    /// 256×256 dont le dessin n'occupe que 48×48 **dans un coin**. Sans rognage, la
    /// case de 96 px n'en montrait que 18 — le défaut signalé, « l'icône apparaît en
    /// taille minuscule ».
    ///
    /// ⚠️ **CE QUI COMPTE EST LE REMPLISSAGE, PAS LA TAILLE ENCODÉE** : une icône
    /// assez petite est rendue telle quelle, et c'est la page qui l'agrandit. Ce qui
    /// doit être vrai, c'est que le DESSIN occupe presque toute l'image envoyée.
    #[test]
    fn une_icone_entouree_de_marge_transparente_est_rognee() {
        let (canvas, dessin) = (256u32, 48u32);
        let mut rgba = vec![0u8; (canvas * canvas * 4) as usize];
        for y in 0..dessin {
            for x in 0..dessin {
                let i = ((y * canvas + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&[200, 30, 30, 255]);
            }
        }

        let (l, h, octets) = normaliser_icone(canvas, canvas, rgba).expect("une icone");

        let (mut x0, mut y0, mut x1, mut y1) = (l, h, 0u32, 0u32);
        for y in 0..h {
            for x in 0..l {
                if octets[((y * l + x) * 4 + 3) as usize] > 8 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        let (gw, gh) = (x1 - x0 + 1, y1 - y0 + 1);
        assert!(
            gw * 100 >= l * 80 && gh * 100 >= h * 80,
            "le dessin doit remplir l'image envoyee — sinon la case le montre minuscule \
             (mesure sur un vrai .jar : 18 px sur 96). Obtenu : dessin {gw}x{gh} dans {l}x{h}"
        );
    }

    /// ⚠️ **LE RAPPORT LARGEUR/HAUTEUR EST CONSERVÉ** : la page centre une image non
    /// carrée (`object-fit: contain`), elle ne l'étire pas. Forcer un carré ici
    /// aurait déformé toute icône qui ne l'est pas — un dossier de Windows, par
    /// exemple (224×176 mesuré sur un vrai).
    #[test]
    fn une_icone_non_carree_garde_ses_proportions() {
        let (l0, h0) = (384u32, 192u32);
        let rgba = vec![255u8; (l0 * h0 * 4) as usize];
        let (l, h, _) = normaliser_icone(l0, h0, rgba).expect("une icone");
        assert_eq!((l, h), (ICONE_PX, ICONE_PX / 2), "le rapport 2:1 doit survivre");
    }

    /// ⭐ **LE SIGNAL DE VIE NE DÉCLENCHE AUCUN GESTE, ET IL RÉPARE SANS RISQUE.**
    ///
    /// S'il rendait une `Demande`, la page ferait agir le dock en se contentant de
    /// dire bonjour. Et s'il n'était pas reconnu du tout, la page enverrait une
    /// action que personne ne comprendrait — silencieusement, puisqu'elle commence
    /// par `sous-dock/`.
    ///
    /// ⚠️ **LE RENVOI QU'IL DÉCLENCHE EST SANS EFFET ICI** : aucune moitié n'est
    /// ouverte dans un test unitaire, donc il ne fait rien. C'est ce qu'on vérifie —
    /// qu'il ne panique pas et ne rende rien.
    /// ⭐ **LA PAGE ET RUST ONT LA MÊME TABLE DE ZOOM** — un écart ferait envoyer
    /// des images d'une taille pour des cases d'une autre (floues ou trop lourdes).
    #[test]
    fn la_page_porte_la_meme_table_de_zoom() {
        let page = include_str!("../../assets/popups/sous_dock.html");
        let attendu = format!(
            "const TAILLES_CASE = [{}];",
            TAILLES_CASE.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")
        );
        assert!(page.contains(&attendu), "la page ne porte pas « {attendu} »");
        assert_eq!(TAILLES_CASE[0] * 3, ICONE_PX, "le rang 0 est l'ancienne taille fixe");
    }

    /// Le zoom ne rend aucune `Demande` ; un niveau hors table ou illisible est
    /// refusé sans rien changer.
    #[test]
    fn le_zoom_ne_rend_aucune_demande_et_refuse_l_illisible() {
        assert_eq!(demande_depuis_action("sous-dock/zoom/99"), None);
        assert_eq!(demande_depuis_action("sous-dock/zoom/abc"), None);
        assert_eq!(niveau_de_zoom_apres("sous-dock/zoom/99"), niveau_de_zoom_apres("sous-dock/zoom/abc"));
    }

    fn niveau_de_zoom_apres(action: &str) -> usize {
        let _ = demande_depuis_action(action);
        niveau_de_zoom()
    }

    /// ⭐ **UNE PHOTO PART EN JPEG, UNE IMAGE TRANSPARENTE EN PNG** — et une
    /// vignette n'est jamais rognée : sa marge est son contenu.
    #[test]
    fn une_vignette_garde_sa_transparence_et_sa_marge() {
        // Un gris OPAQUE : alpha à 255 sur chaque pixel.
        let opaque: Vec<u8> = [200u8, 200, 200, 255].repeat(4 * 4);
        assert!(encoder_vignette(4, 4, &opaque).unwrap().starts_with("data:image/jpeg;base64,"));
        let mut transparente = opaque.clone();
        transparente[3] = 0;
        assert!(encoder_vignette(4, 4, &transparente).unwrap().starts_with("data:image/png;base64,"));
        // 400×200 ramenée à 96 : le rapport reste 2:1, rien n'est rogné.
        let grande = vec![0u8; 400 * 200 * 4];
        let (l, h, _) = reduire_vignette(96, 400, 200, grande).unwrap();
        assert_eq!((l, h), (96, 48));
    }

    #[test]
    fn le_signal_de_vie_ne_rend_aucune_demande_et_ne_panique_pas() {
        assert_eq!(demande_depuis_action("sous-dock/ready"), None, "aucun geste");

        // ⚠️ **UN CÔTÉ NOMMÉ « ready » NE DOIT PAS ÊTRE PRIS POUR LE SIGNAL** :
        // sinon une action forgée déclencherait un renvoi en visant une moitié qui
        // n'existe pas. Le signal est comparé au reste ENTIER, pas à son préfixe.
        assert_eq!(demande_depuis_action("sous-dock/ready/entrer/C%3A%5Cessai"), None);
    }

    /// **SONDE — POURQUOI L'ICÔNE D'UN FICHIER JAVA SORT MINUSCULE.** À lancer à
    /// la main :
    /// `cargo test --bins -- --ignored --nocapture la_sonde_d_une_icone_minuscule`.
    ///
    /// Elle distingue les deux causes possibles, qui se ressemblent à l'écran : le
    /// shell rend une icône PLUS PETITE que les autres (le repli 32 px), ou il en
    /// rend une grande dont le DESSIN n'occupe qu'un coin — auquel cas la réduction
    /// vers `ICONE_PX` rétrécit le dessin avec la marge.
    #[test]
    #[ignore]
    fn la_sonde_d_une_icone_minuscule() {
        // ⚠️ **AUCUN CHEMIN PERSONNEL EN DUR DANS LE DÉPÔT.** Le `.jar` se désigne
        // par `NEURADESK_SONDE_JAR` ; à défaut, on prend le premier trouvé dans les
        // Téléchargements de qui lance la sonde. S'il n'y en a aucun, **le cas est
        // SAUTÉ ET DIT** — une sonde qui échoue sur une machine sans Java ne
        // mesurerait rien du tout.
        fn premier_jar_des_telechargements() -> Option<String> {
            let dossier =
                std::path::PathBuf::from(std::env::var_os("USERPROFILE")?).join("Downloads");
            let mut trouves: Vec<String> = std::fs::read_dir(dossier)
                .ok()?
                .flatten()
                .map(|entree| entree.path())
                .filter(|chemin| {
                    chemin.extension().is_some_and(|e| e.eq_ignore_ascii_case("jar"))
                })
                .map(|chemin| chemin.display().to_string())
                .collect();
            trouves.sort();
            trouves.into_iter().next()
        }

        let jar = std::env::var("NEURADESK_SONDE_JAR").ok().or_else(premier_jar_des_telechargements);
        let mut cas: Vec<(&str, &str)> = Vec::new();
        match jar.as_deref() {
            Some(chemin) => cas.push(("jar", chemin)),
            None => println!(
                "jar      : aucun .jar trouve — cas saute (voir NEURADESK_SONDE_JAR)"
            ),
        }
        cas.push(("txt", r"C:\Windows\System32\drivers\etc\hosts"));
        cas.push(("exe", r"C:\Windows\System32\notepad.exe"));
        cas.push(("dossier", r"C:\Windows\System32\drivers"));
        for (nom, chemin) in cas {
            let Some((l, h, rgba)) = crate::win32::sous_dock::icone_du_shell(Path::new(chemin))
            else {
                println!("{nom:8} : le shell ne donne AUCUNE icone");
                continue;
            };
            // Boîte englobante des pixels non transparents — ce que l'œil VOIT.
            let (mut x0, mut y0, mut x1, mut y1) = (l, h, 0u32, 0u32);
            for y in 0..h {
                for x in 0..l {
                    if rgba[((y * l + x) * 4 + 3) as usize] > 8 {
                        x0 = x0.min(x);
                        y0 = y0.min(y);
                        x1 = x1.max(x);
                        y1 = y1.max(y);
                    }
                }
            }
            if x1 < x0 || y1 < y0 {
                println!("{nom:8} : {l}x{h}, ENTIEREMENT transparent");
                continue;
            }
            let echelle = ICONE_PX as f64 / l as f64;
            let (nl, nh, noctets) =
                normaliser_icone(l, h, rgba.clone()).expect("une icone normalisee");
            let mut nx1 = 0u32;
            let mut ny1 = 0u32;
            for y in 0..nh {
                for x in 0..nl {
                    if noctets[((y * nl + x) * 4 + 3) as usize] > 8 {
                        nx1 = nx1.max(x);
                        ny1 = ny1.max(y);
                    }
                }
            }
            println!(
                "{nom:8} : shell {l}x{h} | dessin {}x{} en ({x0},{y0}) | a l'ecran {:.0}x{:.0} sur {ICONE_PX} \
                 -> APRES normalisation {nl}x{nh}, dessin rempli a {}%x{}%",
                x1 - x0 + 1,
                y1 - y0 + 1,
                (x1 - x0 + 1) as f64 * echelle,
                (y1 - y0 + 1) as f64 * echelle,
                nx1 * 100 / nl.max(1),
                ny1 * 100 / nh.max(1)
            );
        }
    }

    /// **SONDE — OÙ PASSENT LES 4,5 ms D'UNE ICÔNE ?** À lancer à la main :
    /// `cargo test --bins -- --ignored --nocapture la_sonde_du_cout_d_une_icone`.
    ///
    /// Elle est née d'une mesure, pas d'une intuition : un lot de 24 icônes a été
    /// chronométré à **100 ms** dans `envoyer_des_icones`, soit dix fois le budget
    /// d'une frame. Trois étapes se partagent ce coût — l'extraction par le shell,
    /// le redimensionnement, l'encodage — et il fallait savoir LAQUELLE avant de
    /// toucher à quoi que ce soit.
    #[test]
    #[ignore]
    fn la_sonde_du_cout_d_une_icone() {
        let liste = crate::win32::sous_dock::enfants(Path::new(r"C:\Windows\System32"), Tri::Nom);
        let chemins: Vec<std::path::PathBuf> =
            liste.enfants.iter().take(40).map(|e| e.chemin.clone()).collect();

        let depart = std::time::Instant::now();
        let extraites: Vec<_> =
            chemins.iter().map(|c| crate::win32::sous_dock::icone_du_shell(c)).collect();
        let shell = depart.elapsed().as_secs_f64() * 1000.0 / chemins.len() as f64;

        let sources: Vec<(u32, u32, Vec<u8>)> = extraites.iter().flatten().cloned().collect();
        let cotes: Vec<(u32, u32)> = sources.iter().map(|(l, h, _)| (*l, *h)).take(3).collect();
        println!(
            "extraction shell (jumbo) : {shell:.2} ms/icone  (n={}/{} extraites, cotes {cotes:?})",
            sources.len(),
            chemins.len()
        );

        // ⚠️ **ON COMPARE, ON NE CHOISIT PAS D'AVANCE.** Lanczos3 est le filtre le
        // plus coûteux de la famille ; les autres sont plus doux. Le seul juge est
        // le chronomètre, et le seul risque est la netteté — d'où la comparaison.
        for (nom, filtre) in [
            ("Lanczos3", image::imageops::FilterType::Lanczos3),
            ("CatmullRom", image::imageops::FilterType::CatmullRom),
            ("Triangle", image::imageops::FilterType::Triangle),
        ] {
            let depart = std::time::Instant::now();
            for (l, h, rgba) in &sources {
                let image = image::RgbaImage::from_raw(*l, *h, rgba.clone()).expect("image");
                let _ = image::imageops::resize(&image, ICONE_PX, ICONE_PX, filtre);
            }
            let ms = depart.elapsed().as_secs_f64() * 1000.0 / sources.len().max(1) as f64;
            println!("  {nom:11} {ms:.2} ms/icone");
        }
        let depart = std::time::Instant::now();
        for (l, h, rgba) in &sources {
            let image = image::RgbaImage::from_raw(*l, *h, rgba.clone()).expect("image");
            let _ = image::imageops::thumbnail(&image, ICONE_PX, ICONE_PX);
        }
        let ms = depart.elapsed().as_secs_f64() * 1000.0 / sources.len().max(1) as f64;
        println!("  {:11} {ms:.2} ms/icone", "thumbnail");
    }

    /// ⛔ **LA MOITIÉ DROITE GARDE SA POSITION D'UNE OUVERTURE À L'AUTRE.**
    ///
    /// C'est une DÉCISION DE L'UTILISATEUR (2026-10-01), et elle a une
    /// conséquence qu'il faut connaître : le dossier personnel n'est qu'un
    /// **défaut**, pris à la première ouverture. Un changement du réglage ne se
    /// verra donc qu'au lancement suivant.
    ///
    /// ⚠️ Le défaut refusé est concret : remettre la droite sur le dossier
    /// personnel à chaque survol de vignette ferait perdre la navigation faite à
    /// droite dès qu'on revient voir à gauche — c'est-à-dire à chaque fois.
    #[test]
    fn la_moitie_droite_garde_sa_position_d_une_ouverture_a_l_autre() {
        let profil = PathBuf::from(r"C:\Users\essai");
        let ailleurs = PathBuf::from(r"C:\essai\projets");

        // Première ouverture : elle n'a pas encore de position, elle prend le défaut.
        assert_eq!(dossier_de_droite(Path::new(""), &profil), profil);
        // Les suivantes : elle garde la sienne, même si le défaut a changé.
        assert_eq!(dossier_de_droite(&ailleurs, &profil), ailleurs);
        assert_eq!(
            dossier_de_droite(&ailleurs, Path::new(r"C:\autre\profil")),
            ailleurs,
            "un reglage modifie ne doit PAS deplacer une moitie qui a une position"
        );
    }

    /// ⛔ **LES DEUX MOITIÉS SONT INDÉPENDANTES.** C'est LA régression la plus
    /// probable de ce lot : un seul `PathBuf` partagé, et changer la gauche
    /// ferait bouger la droite. On le refuse explicitement.
    #[test]
    fn les_deux_moities_sont_independantes() {
        let gauche = PathBuf::from(r"C:\essai\gauche");
        let droite = PathBuf::from(r"C:\essai\droite");
        changer(Cote::Gauche, &gauche);
        changer(Cote::Droite, &droite);
        assert_eq!(dossier_affiche(Cote::Gauche), Some(gauche.clone()));
        assert_eq!(dossier_affiche(Cote::Droite), Some(droite.clone()));

        // Changer la gauche ne touche pas la droite, et ne fait pas avancer SA
        // révision — sans quoi la page reconstruirait la droite et sa recherche
        // repartirait à zéro.
        let revision_droite = avec(Cote::Droite, |m| m.revision);
        changer(Cote::Gauche, &PathBuf::from(r"C:\essai\ailleurs"));
        assert_eq!(dossier_affiche(Cote::Droite), Some(droite));
        assert_eq!(
            avec(Cote::Droite, |m| m.revision),
            revision_droite,
            "la revision de la droite ne doit pas bouger quand la gauche change"
        );
    }

    /// ⛔ **LE SOUVENIR DE GAUCHE NE S'ÉCRIT PAS POUR RIEN.** C'est le fichier le
    /// plus écrit du dépôt (`config::save_sous_dock_gauche`), et une écriture à
    /// chaque pas dans l'arborescence serait exactement le genre de zèle que le
    /// dépôt a déjà payé. On éprouve **la DÉCISION**, pas l'écriture :
    /// `souvenir_a_ecrire` est séparée de `memoriser_gauche` justement pour qu'un
    /// test n'aille pas abîmer le vrai réglage de l'utilisateur dans `%APPDATA%`.
    #[test]
    fn le_souvenir_de_gauche_ne_se_reécrit_pas_s_il_est_deja_le_bon() {
        let chemin = PathBuf::from(r"C:\essai\un");
        let autre = PathBuf::from(r"C:\essai\deux");

        GAUCHE_MEMORISEE.with(|g| *g.borrow_mut() = None);
        assert!(souvenir_a_ecrire(&chemin), "rien de retenu : il faut ecrire");

        GAUCHE_MEMORISEE.with(|g| *g.borrow_mut() = Some(chemin.clone()));
        assert!(!souvenir_a_ecrire(&chemin), "le meme chemin ne doit RIEN reecrire");
        assert!(souvenir_a_ecrire(&autre), "un autre chemin doit etre retenu");
        assert!(souvenir_a_ecrire(Path::new(r"C:\essai\UN")), "la casse n'est pas repliee : ce n'est pas le meme chemin Windows");
    }
}
