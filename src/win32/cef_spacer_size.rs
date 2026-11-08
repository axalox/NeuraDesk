//! Fenêtre CEF pour le curseur de taille d'un espace invisible (clic droit
//! sur un séparateur) — Phase 2 de `docs/plans/cef-web-ui-migration.md`.
//! Remplace `PopupKind::SpacerSize` (`win32::popup_menu::
//! render_spacer_size`).
//!
//! Premier popup de cette migration piloté par un `<input type="range">` —
//! voir `assets/popups/spacer_size.html`, doc d'en-tête : l'événement
//! `input` (pas `change`) fait `location.href = "neuradesk://spacer/
//! size/<px>"` à CHAQUE pas du glissement, même mécanisme JS→Rust que les
//! popups précédents (`RequestHandler::on_before_browse`), juste appelé
//! très souvent pendant un glissement — un simple `Vec::push` par pas, sans
//! coût notable.
//!
//! Comme `win32::cef_network`/`win32::cef_sound`, un canal Rust→JS
//! (`push_state`) pousse la taille ACTUELLE à l'ouverture — mais QUEL espace
//! (son `index`, voir `ui::dock::Dock::spacer_size`/`set_spacer_size`)
//! change à CHAQUE ouverture (cette fenêtre, comme toutes les fenêtres
//! popup de ce projet, est réutilisée pour n'importe quel espace, un seul
//! exemplaire à la fois) : retenu CÔTÉ RUST dans `CURRENT_INDEX` (fixé par
//! `toggle`), PAS renvoyé par la page — voir `assets/popups/
//! spacer_size.html`, doc d'en-tête.
//!
//! **L'état est repoussé à CHAQUE frame** (`poll`, voir sa doc) et non poussé
//! une seule fois à l'ouverture : c'est le remède déjà retenu pour
//! `cef_network`/`cef_sound` contre la course avec le chargement de la page,
//! et ce module en avait besoin comme eux — mesuré, pas supposé.
//!
//! **QUATRE GRANDEURS, JAMAIS LES QUATRE ENSEMBLE** : un espace LATÉRAL porte la
//! taille de son blanc (`Icon::spacer_size`) **ou**, s'il est un SÉPARATEUR DE
//! ZONE, trois autres réglages — la cote de la zone qu'il ouvre
//! (`Icon::zone_height`) et le NOMBRE DE COLONNES de sa grille
//! (`Icon::zone_columns`), tous deux DEMANDE EXPLICITE (« que les zones définies
//! par les séparateurs de droite soient découpées en grille dont le nombre de
//! colonnes puisse être défini par l'utilisateur »), puis son VERROU
//! (`Icon::zone_locked`), DEMANDE EXPLICITE : « pour la barre de droite, la
//! possibilité de verrouiller par séparateur ».
//!
//! **Un séparateur de zone n'a PAS de blanc réglable** : son emplacement vaut la
//! taille de son marqueur (voir `ui::dock::bar_layout`), donc un curseur de blanc
//! n'y réglerait rien — exactement le « réglage sans effet » que ce module
//! refuse partout ailleurs. Sa ligne est masquée (voir `CURRENT_STATE`).
//!
//! Les trois réglages de zone ne sont proposés que pour une zone de la barre de
//! DROITE : c'est là que vivent la grille et le verrou par séparateur, donc
//! l'interface ne propose pas un réglage sans effet.
//!
//! **Chaque curseur est masqué par SA propre absence** — `size`, `cote`,
//! `columns` et `locked` sont des `Option`, et c'est le `None` qui cache la
//! ligne ; les lier ferait disparaître l'une quand l'autre manque. `columns`
//! est en outre `None` pour une zone de la barre de GAUCHE : la grille n'existe
//! qu'à droite
//! (voir `ui::dock::Dock::effective_zone_columns`), donc l'interface ne propose
//! pas un réglage sans effet.
//!
//! **LES DEUX PLAFONDS VIENNENT DE LA DISPOSITION, jamais d'une constante
//! recopiée** : `zone_cote_max` (la hauteur réellement disponible dans la
//! barre) et `max_zone_columns` (ce que sa largeur accorde au plancher de 32 px à 150 %
//! par colonne). C'est ce qui garantit qu'un curseur ne peut pas promettre une
//! valeur que le dessin refuserait — et que le geste de redimensionnement
//! s'arrête au même endroit que le curseur.
//!
//! **PIÈGE À NE PAS REDÉCOUVRIR — la borne du curseur d'origine (180 px).**
//! Elle datait du temps où ce popup ne réglait QUE le blanc d'un espace, dont
//! une valeur utile tient en dizaines de pixels. Une cote de zone, elle, se
//! compte en centaines (`DEFAULT_ZONE_HEIGHT` vaut 400) : la même borne
//! rendait donc toute cote supérieure à 180 px INATTEIGNABLE par ce curseur,
//! sans que rien ne le signale. Le second curseur a la sienne, dérivée de la
//! barre.
//!
//! Toute la mécanique fenêtre/rendu/relais souris est dans
//! `win32::cef_popup` — voir `win32::cef_test_window` pour la doc de
//! module complète du pattern suivi ici.

use std::cell::{Cell, RefCell};
use std::path::Path;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::ui::zones::SorteEspace;
use crate::win32::menu;

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// Quel espace la fenêtre actuellement affichée contrôle — voir la doc
    /// de module. Valeur par défaut (`0`) sans conséquence : jamais lue
    /// avant qu'un premier `toggle` ne l'ait fixée.
    static CURRENT_INDEX: Cell<usize> = const { Cell::new(0) };
    /// QUEL élément cette fenêtre règle — même rôle que
    /// `cef_bar_size::CURRENT_TARGET`, voir sa doc.
    ///
    /// **C'est la cible qui décide de la branche du dispatch d'`app::run`**, et
    /// elle est TYPÉE (voir `SpacerTarget`) : un emplacement de la rangée de
    /// widgets n'est ni un espace d'un dock ni un écran, et son
    /// `CURRENT_INDEX` ne veut rien dire. Une chaîne (`"emplacement:<nom>"`) a
    /// été essayée d'abord, et il fallait alors un PRÉFIXE pour l'écarter d'un
    /// nom d'écran — un type le fait par construction, et rend impossible une
    /// cible à moitié posée.
    static CURRENT_TARGET: RefCell<Option<SpacerTarget>> = const { RefCell::new(None) };
    /// Numéro d'OUVERTURE, incrémenté à chaque `toggle` — voir `push_state`,
    /// et `assets/popups/spacer_size.html`, doc de `appliedOpen`. C'est ce qui
    /// permet à la page d'appliquer l'état d'une ouverture UNE SEULE FOIS tout
    /// en recevant la poussée RÉPÉTÉE (`poll`) qui la fait survivre au
    /// chargement de la page.
    static OPEN_COUNT: Cell<u64> = const { Cell::new(0) };
    /// Dernières valeurs poussées — `(taille du blanc, cote, plafond de cote,
    /// colonnes, plafond de colonnes)`. La PREMIÈRE est `None` pour un
    /// SÉPARATEUR DE ZONE, les deux suivantes hors d'une zone de la barre de
    /// droite : c'est cette ABSENCE qui masque leurs lignes (voir
    /// `push_state`).
    ///
    /// **Retenues ici parce que `poll` n'a aucun moyen de les relire** : il est
    /// appelé à chaque frame et ne reçoit rien, ce module n'ayant pas accès au
    /// `Dock` (voir la doc de `ui::dock::Dock::spacer_size` pour ce que
    /// l'appelant, lui, sait à l'ouverture).
    /// ...et, pour une zone, son VERROU (`None` hors zone, comme la cote et les
    /// colonnes : c'est cette absence qui masque la ligne).
    /// **Le blanc est un `Option` comme les trois autres, et pour la même
    /// raison** : sur un SÉPARATEUR DE ZONE il ne règle plus rien (l'emplacement
    /// d'un séparateur vaut la taille de son marqueur, voir
    /// `ui::dock::bar_layout`), donc sa ligne disparaît — offrir un curseur sans
    /// effet ferait croire qu'une zone s'agrandit sous la main.
    static CURRENT_STATE: Cell<(Option<f32>, Option<f32>, f32, Option<u16>, u16, Option<bool>)> =
        const { Cell::new((None, None, 0.0, None, 1, None)) };
    /// Voir `app::content_scale_pour`, doc de fonction — mémorisé ici à
    /// l'ouverture (voir `toggle`) et relu à chaque frame par
    /// `render_if_visible`.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// **LA SORTE DE L'ESPACE OUVERT** (voir `ui::zones::SorteEspace`) — un
    /// `Cell` à part plutôt qu'un septième membre du tuple `CURRENT_STATE`, qui
    /// commençait à être long à lire.
    ///
    /// ⚠️ **`None` POUR UN EMPLACEMENT DE LA BARRE DU HAUT**, et c'est ce `None`
    /// qui MASQUE la ligne : un emplacement n'a pas de case propre à traverser, et
    /// `spacer_hover_line_rect` y rend `(0,0)` — proposer « Trait » là serait un
    /// contrôle qui ment. Même contrat que `size`/`cote`, dont c'est l'absence qui
    /// cache la ligne.
    static SORTE: Cell<Option<SorteEspace>> = const { Cell::new(None) };
    /// **L'ÉLASTIQUE EST-IL SEULEMENT POSSIBLE ICI ?** — vrai pour un espace de la
    /// barre du BAS, et pour elle seule : c'est la seule barre dont la largeur soit
    /// connue, donc la seule où « ce qui reste » existe (voir
    /// `layout_bottom_side`). Ailleurs le bouton n'est pas proposé — **un réglage
    /// sans effet est pire qu'un réglage absent**, et c'est la règle que ce popup
    /// applique déjà pour le blanc d'un séparateur de zone.
    static ELASTIQUE_POSSIBLE: Cell<bool> = const { Cell::new(false) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefSpacerSizeWindow"),
    window_title: w!("NeuraDesk — Taille de l'espace"),
    html_file: "spacer_size.html",
    wndproc: Some(wndproc),
    above_a_magnified_dock: true,
    // MENU — apparaît et disparaît en fondant, voir `PopupSpec::fades`.
    fades: true,
    fenetre: false,
};

/// Précharge ce popup au démarrage — fenêtre et navigateur naissent CACHÉS, pour
/// que la page ait fini de charger avant la première ouverture réelle. Voir
/// `win32::cef_popup::preload`, doc de fonction, qui porte le pourquoi complet.
pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// Hauteur d'une rangée du popup, une fois zoomée — mesurée sur une capture :
/// 96 px pour une rangée de 32 px de CSS au zoom ×3 (voir
/// `cef_popup::CONTENT_ZOOM_FACTOR`). Sert à `popup_height`.
const POPUP_ROW_PX: i32 = 96;
/// Ce que coûte le popup EN PLUS de ses rangées : le titre et les marges, la
/// première rangée comprise.
const POPUP_BASE_PX: i32 = 330 - POPUP_ROW_PX;

/// Hauteur du popup pour `rows` rangées de réglage — un espace ORDINAIRE en a
/// toujours une (le blanc), un séparateur de zone trois autres, et l'appelant
/// compte (voir `app::run`).
///
/// **POURQUOI ELLE EST CALCULÉE ICI, et pas laissée à l'appelant** : une
/// hauteur fixe a été essayée en premier, et la troisième rangée est TOMBÉE
/// HORS DE LA FENÊTRE — invisible, sans erreur, sans rien dans le journal. Il a
/// fallu une capture et un comptage de bandes de texte pour le voir (deux
/// rangées au lieu de trois). Le nombre de rangées et la hauteur qui les
/// contient sont donc décidés au même endroit, par la même fonction, et
/// `app::run` ne peut plus les faire diverger.
///
/// Aucune borne supérieure : c'est le nombre de rangées qui commande, et il ne
/// dépend que de ce que le popup a réellement à montrer.
pub fn popup_height(rows: usize) -> i32 {
    POPUP_BASE_PX + POPUP_ROW_PX * rows.max(1) as i32
}

/// Ce qu'un pas de curseur règle — les deux grandeurs qu'un espace LATÉRAL
/// porte, et que ce popup présente ensemble (voir la doc de module).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SpacerEdit {
    /// La taille du blanc (`ui::dock::Dock::set_spacer_size`) — le seul
    /// réglage d'un espace ordinaire. Sa ligne n'est PAS proposée pour un
    /// séparateur de zone, dont l'emplacement vaut la taille de son marqueur
    /// (voir `ui::dock::bar_layout`) : l'action reste acceptée — la valeur est
    /// conservée et redevient agissante si la zone est défaite — mais rien ne
    /// l'émet tant que la ligne est masquée.
    Size(f32),
    /// La cote de la zone qu'il ouvre (`ui::dock::Dock::set_zone_height`) —
    /// le seul réglage qui distingue un SÉPARATEUR DE ZONE.
    ///
    /// `0.0` est permis et DÉFAIT la zone (voir `set_zone_height`) : c'est
    /// exactement ce que fait le geste de redimensionnement à la même valeur,
    /// et deux commandes de la même grandeur ne peuvent pas s'arrêter à des
    /// endroits différents.
    Cote(f32),
    /// Le NOMBRE DE COLONNES de la grille de la zone qu'il ouvre (`ui::dock::
    /// Dock::set_zone_columns`) — DEMANDE EXPLICITE : « que les zones définies
    /// par les séparateurs de droite soient découpées en grille dont le nombre
    /// de colonnes puisse être défini par l'utilisateur ».
    ///
    /// **Un `u16` et non un `f32`** : un nombre de colonnes est entier, et le
    /// curseur de la page est en `step="1"` — le passer en flottant ferait
    /// entrer des 2.9999 dans une valeur qu'on arrondit ensuite, pour rien.
    Columns(u16),
    /// Le VERROU de la zone qu'il ouvre (`ui::dock::Dock::set_zone_locked`) —
    /// DEMANDE EXPLICITE : « pour la barre de droite, la possibilité de
    /// verrouiller par séparateur ». Une case à cocher, donc un `bool` franc :
    /// contrairement aux trois autres, il n'y a pas de « valeur absente » qui
    /// aurait un sens.
    Locked(bool),
    /// **CE QUE L'ESPACE DESSINE** (voir `ui::zones::SorteEspace`) — un choix
    /// parmi trois, donc un bouton par sorte plutôt qu'une case.
    ///
    /// ⚠️ **`Elastique` N'EST PROPOSÉ QUE SUR LA BARRE DU BAS** (voir
    /// `ELASTIQUE_POSSIBLE`) : c'est la seule dont la largeur soit connue, donc la
    /// seule où « ce qui reste » existe. L'action reste ACCEPTÉE ailleurs — la
    /// valeur est conservée et redeviendra agissante si l'espace change de barre —
    /// mais rien ne l'émet tant que le bouton est masqué. Même contrat que `Size`
    /// pour un séparateur de zone.
    Sorte(SorteEspace),
}

/// QUEL élément ce popup règle — il est PARTAGÉ par tous (une seule fenêtre,
/// réutilisée), donc sa cible doit dire à la fois **de quelle FAMILLE** il
/// s'agit et **de quel ÉCRAN**.
///
/// **Deux variantes, et non un `(device, slot)` en deux champs** : le second
/// laisserait `Some(slot)` à côté d'un `device` signifiant « primaire »
/// indistinguable d'un oubli, alors qu'ici chaque cas est un constructeur
/// distinct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpacerTarget {
    /// Un ESPACE d'un dock — `None` = barre PRIMAIRE. C'est la seule famille
    /// qui utilise `CURRENT_INDEX`.
    DockSpacer(Option<String>),
    /// Un SÉPARATEUR de la rangée de widgets de la barre du HAUT (voir
    /// `ui::topbar::TopSlot`) — `device: None` = barre PRIMAIRE.
    ///
    /// **`rank` est devenu nécessaire le 2026-09-19** : un intervalle porte
    /// désormais PLUSIEURS séparateurs (DEMANDE EXPLICITE : « je n'arrive pas à
    /// ajouter plus d'un séparateur entre le widget pour afficher le bureau et la
    /// bascule de bureau »), donc le NOM de l'intervalle ne suffit plus à dire
    /// lequel on règle. `index` reste inutilisé pour cette famille : ce sont les
    /// rangs, pas les index d'icônes, qui désignent un séparateur.
    TopSlot { device: Option<String>, slot: crate::ui::topbar::TopSlot, rank: usize },
}

/// Plafond de la rangée « Taille » pour une cible — `None` laisse celui de la
/// page.
///
/// **PUR, donc testable, et c'est la seule définition de cette règle** : un
/// EMPLACEMENT de la rangée a le sien (`ui::topbar::MAX_SLOT_WIDTH`), plus large
/// que celui d'un espace du dock — lequel reste le littéral de la page, que
/// Rust ne connaît pas (`Dock::set_spacer_size` n'a PAS de plafond, la valeur
/// n'y est bornée que par le curseur lui-même).
pub fn max_size_for(target: Option<SpacerTarget>) -> Option<f32> {
    match target {
        Some(SpacerTarget::TopSlot { .. }) => Some(crate::ui::topbar::MAX_SLOT_WIDTH),
        Some(SpacerTarget::DockSpacer(_)) | None => None,
    }
}

/// Voir `win32::cef_popup::toggle`. `index` : QUEL espace — retenu et poussé à
/// la page (voir `push_state`, `CURRENT_INDEX`), même si la fenêtre existe déjà
/// et n'est que repositionnée (voir la doc de module : un seul exemplaire
/// réutilisé pour n'importe quel espace). `current_size` : sa taille de blanc
/// actuelle (voir `ui::dock::Dock::spacer_size`), `None` quand ce n'est pas elle
/// qui compte (voir plus bas).
///
/// `current_size` : `None` pour un SÉPARATEUR DE ZONE (voir
/// `ui::dock::Dock::is_zone_separator`) — c'est ce `None` qui masque la
/// PREMIÈRE ligne, un séparateur n'ayant plus de blanc réglable (voir
/// `CURRENT_STATE`).
///
/// `cote` : `Some` seulement pour un SÉPARATEUR DE ZONE (voir
/// `ui::dock::Dock::is_zone_separator`), sa valeur actuelle — c'est ce `None`
/// qui masque le second curseur côté page. `max_cote` vient de
/// `ui::dock::Dock::zone_cote_max`.
#[allow(clippy::too_many_arguments)] // un paramètre par grandeur affichée, voir la doc de module
pub fn toggle(
    rect: [i32; 4],
    gfx: &GfxDevice,
    content_scale: f32,
    index: usize,
    current_size: Option<f32>,
    cote: Option<f32>,
    max_cote: f32,
    columns: Option<u16>,
    max_columns: u16,
    locked: Option<bool>,
    // La sorte ACTUELLE de l'espace ouvert (voir `SorteEspace`) — `None` quand il
    // n'y a pas d'espace de dock à régler (un emplacement de la barre du haut).
    sorte: Option<SorteEspace>,
    // L'élastique est-il proposé ici ? Vrai pour un espace de la barre du BAS
    // seulement (voir `ELASTIQUE_POSSIBLE`).
    elastique_possible: bool,
    target: Option<SpacerTarget>,
) {
    CURRENT_INDEX.with(|c| c.set(index));
    CURRENT_TARGET.with(|t| *t.borrow_mut() = target);
    CONTENT_SCALE.with(|c| c.set(content_scale));
    SORTE.with(|c| c.set(sorte));
    ELASTIQUE_POSSIBLE.with(|c| c.set(elastique_possible));
    // Les COLONNES ne sont annoncées que pour une zone de la barre de DROITE :
    // c'est la seule où la grille existe (voir `effective_zone_columns`), donc
    // la seule où la ligne du curseur doit apparaître.
    CURRENT_STATE.with(|c| c.set((current_size, cote, max_cote, columns, max_columns, locked)));
    // Une ouverture de plus : la page appliquera cet état, MÊME si elle a déjà
    // appliqué le précédent (voir `OPEN_COUNT`).
    OPEN_COUNT.with(|c| c.set(c.get() + 1));
    cef_popup::toggle(&SLOT, &SPEC, rect, gfx);
    // Tentative immédiate — perdue en silence si la page n'a pas fini de
    // charger, c'est `poll` qui la rattrape (voir sa doc).
    push_state();
}

/// Pousse l'état de l'ouverture courante dans la page via `window.setState`
/// (voir `assets/popups/spacer_size.html`).
fn push_state() {
    let (size, cote, max_cote, columns, max_columns, locked) = CURRENT_STATE.with(Cell::get);
    let open = OPEN_COUNT.with(Cell::get);
    // `null` — et non `0` — pour un espace ordinaire : la page cache les deux
    // curseurs de zone sur une valeur absente, alors qu'un zéro les afficherait
    // et laisserait croire à une zone de hauteur nulle et sans colonne (voir la
    // page, doc d'en-tête).
    //
    // Même règle pour le BLANC, mais dans l'autre sens : c'est un SÉPARATEUR DE
    // ZONE qui n'en a plus (voir `CURRENT_STATE`).
    let size = size.map_or_else(|| "null".to_string(), |size| size.to_string());
    let cote = cote.map_or_else(|| "null".to_string(), |cote| cote.to_string());
    let columns = columns.map_or_else(|| "null".to_string(), |columns| columns.to_string());
    // `null` hors zone, comme les deux précédentes : la case disparaît alors,
    // au lieu de s'afficher décochée — ce qui laisserait croire qu'un espace
    // ordinaire PEUT être verrouillé.
    let locked = locked.map_or_else(|| "null".to_string(), |locked| locked.to_string());
    // LE PLAFOND DE LA RANGÉE « TAILLE » SUIT LA CIBLE — voir `max_size_for`.
    let max_size = max_size_for(CURRENT_TARGET.with(|t| t.borrow().clone()));
    let max_size = max_size.map_or_else(|| "null".to_string(), |max_size| max_size.to_string());
    // LA SORTE — sa clé, et si l'élastique est seulement PROPOSABLE ici. Les deux
    // partent ensemble : la page a besoin des deux pour décider ce qu'elle montre
    // ET ce qu'elle surligne, et les séparer la ferait décider sur un état à
    // moitié lu.
    // `null` hors d'un espace de dock — et c'est cette ABSENCE qui masque la ligne,
    // exactement comme pour le blanc et la cote (voir `SORTE`).
    let sorte = SORTE.with(Cell::get).map_or_else(|| "null".to_string(), |s| format!("\"{}\"", s.cle()));
    let elastique_possible = ELASTIQUE_POSSIBLE.with(Cell::get);
    cef_popup::execute_js(
        &SLOT,
        &format!(
            // ⚠️ **`sorte.cle()` EST LA SEULE CHAÎNE QUE CETTE FONCTION ENVOIE, ET
            // ELLE N'A PAS BESOIN D'ÊTRE ÉCHAPPÉE** : elle sort d'un `match` à
            // trois branches littérales (`"rond"`/`"trait"`/`"elastique"`), donc
            // elle ne peut contenir ni guillemet ni antislash. C'est une
            // CIRCONSTANCE, pas une règle — le jour où une valeur venue de
            // l'utilisateur prendra ce chemin, il faudra l'échapper (voir
            // `cef_dossier_menu::etat_json`, qui passe par `serde_json` pour ça).
            "window.setState({{open: {open}, size: {size}, maxSize: {max_size}, cote: {cote}, maxCote: {max_cote}, columns: {columns}, maxColumns: {max_columns}, locked: {locked}, sorte: {sorte}, elastiquePossible: {elastique_possible}}})"
        ),
    );
}

/// À appeler à CHAQUE frame tant que ce popup est visible (voir `app::run`,
/// même endroit que `win32::cef_network::poll`/`win32::cef_sound::poll`) —
/// même remède au même bug : le `push_state()` de `toggle()`, appelé juste
/// après la création de la fenêtre, peut arriver AVANT que `window.setState`
/// n'existe côté JS et être perdu en silence. BUG RÉEL CONSTATÉ ICI PAR
/// CAPTURE, et non supposé : la page s'affichait avec sa valeur par défaut
/// (poignée au MILIEU du rail, alors que l'espace ouvert valait 20 px) et
/// SANS sa seconde ligne, celle de la cote — c'est-à-dire sans rien de ce que
/// Rust lui avait envoyé.
///
/// `cef_bar_size` a exactement la même forme et le même défaut, non corrigé
/// ici : hors du périmètre de cette étape.
///
/// **La page n'applique pourtant cet état QU'UNE FOIS par ouverture** (voir
/// son `appliedOpen`) : repousser la même ouverture à chaque frame ne doit
/// pas réécrire une poignée que l'utilisateur est en train de glisser — règle
/// du projet, et une poussée en retard d'une frame ferait carrément RECULER
/// le curseur sous son doigt.
pub fn poll() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

/// Vide les actions demandées par `assets/popups/spacer_size.html` (formats
/// `"spacer/size/<px>"`, `"spacer/cote/<px>"` et `"spacer/columns/<n>"`) et
/// retourne `(index, quoi,
/// cible)` pour L'ESPACE COURAMMENT OUVERT (voir `CURRENT_INDEX`) si au moins
/// une action a été reçue depuis le dernier appel — `None` sinon (aucune
/// action, ou valeur illisible). Ne pousse PAS d'état en retour : contrairement
/// à `win32::cef_sound`/`win32::cef_network`, rien côté Rust ne peut changer
/// ces grandeurs AUTREMENT que par ce curseur pendant qu'il est ouvert
/// (comparer à Wi-Fi/Bluetooth, basculables par ailleurs), donc pas besoin de
/// rafraîchir la page après coup — elle reflète déjà ce qu'elle vient
/// d'envoyer.
///
/// **La dernière action reçue gagne**, toutes familles confondues : la page
/// n'en émet qu'une par pas de curseur, et un glissement très rapide peut en
/// empiler plusieurs — c'est bien le dernier état qui compte, pas l'ordre
/// d'arrivée entre les deux curseurs (ils ne bougent pas ensemble).
pub fn dispatch_actions() -> Option<(usize, SpacerEdit, Option<SpacerTarget>)> {
    let mut result = None;
    for action in cef_popup::drain_actions(&SLOT) {
        // `Option` et non `Result` : les trois grandeurs ne se parsent pas vers
        // le MÊME type d'erreur (`ParseFloatError` contre `ParseIntError`),
        // donc un `Result` commun aux trois branches ne compilerait pas — et
        // l'erreur précise n'a de toute façon aucun intérêt ici, seule compte
        // la valeur.
        let parsed: Option<SpacerEdit> = if let Some(px) = action.strip_prefix("spacer/size/") {
            px.parse::<f32>().ok().map(SpacerEdit::Size)
        } else if let Some(px) = action.strip_prefix("spacer/cote/") {
            px.parse::<f32>().ok().map(SpacerEdit::Cote)
        } else if let Some(flag) = action.strip_prefix("spacer/locked/") {
            // Une case à cocher n'a que deux états, et la page envoie "1"/"0" —
            // pas de valeur illisible à prévoir, mais on ne le suppose pas.
            match flag {
                "1" => Some(SpacerEdit::Locked(true)),
                "0" => Some(SpacerEdit::Locked(false)),
                _ => None,
            }
        } else if let Some(cle) = action.strip_prefix("spacer/sorte/") {
            // La clé d'une sorte : un ensemble FERMÉ, et une clé inconnue est
            // refusée plutôt que ramenée au défaut (voir `SorteEspace::depuis_cle`).
            SorteEspace::depuis_cle(cle).map(SpacerEdit::Sorte)
        } else if let Some(count) = action.strip_prefix("spacer/columns/") {
            // Le curseur de la page est en `step="1"` : un entier, et c'est ce
            // que la valeur est (voir `SpacerEdit::Columns`).
            count.parse::<u16>().ok().map(SpacerEdit::Columns)
        } else {
            eprintln!("[neuradesk][cef] action espace inconnue : {action:?}");
            continue;
        };
        match parsed {
            Some(edit) => {
                // **LA SORTE CHOISIE EST REPOSÉE ICI — ET C'EST CE QUI FAIT BOUGER
                // LA LENTILLE.** `push_state` relit `SORTE`, qui n'était fixée
                // qu'à l'OUVERTURE : sans cette ligne, le bouton cliqué restait
                // sans surbrillance alors que le dessin, lui, changeait — le
                // panneau montrait l'état d'AVANT le clic, et l'utilisateur en
                // concluait que rien ne marchait.
                //
                // ⚠️ **Les autres grandeurs n'ont pas ce besoin, et il faut savoir
                // pourquoi** : rien d'autre qu'elles-mêmes ne les change pendant
                // que le popup est ouvert (voir `dispatch_actions`, doc), donc
                // l'état retenu à l'ouverture reste vrai. La sorte est la première
                // qui se repose par une AUTRE voie — la même, du reste, mais il
                // faut le dire.
                if let SpacerEdit::Sorte(sorte) = &edit {
                    SORTE.with(|c| c.set(Some(*sorte)));
                }
                result = Some((
                    CURRENT_INDEX.with(|c| c.get()),
                    edit,
                    CURRENT_TARGET.with(|t| t.borrow().clone()),
                ))
            }
            None => eprintln!("[neuradesk][cef] valeur d'espace invalide : {action:?}"),
        }
    }
    result
}

pub fn render_if_visible(tint_rgba: [f32; 4]) {
    let scale = CONTENT_SCALE.with(|c| c.get());
    cef_popup::render_if_visible(&SLOT, tint_rgba, menu::menu_corner_radius(), cef_popup::PopupPalette::Menu { zoom_level: cef_popup::menu_zoom_level(scale) });
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

#[cfg(test)]
mod les_curseurs_de_cote_ne_detruisent_pas_la_zone {
    /// ⛔ **LE CURSEUR DE COTE NE DOIT PAS DESCENDRE À 0 — BUG RÉEL DU 2026-10-08.**
    ///
    /// Un `0` de HAUTEUR ne veut pas dire « pas de hauteur » : il **DÉFAIT** la
    /// zone (`Dock::set_zone_height`, et `is_zone_separator` EST
    /// `zone_height > 0`). Un curseur qui peut l'atteindre DÉTRUIT donc une zone
    /// en silence — c'est ainsi que l'utilisateur a perdu la sienne : « ses
    /// réglages ont disparu ainsi que la zone dans la barre de droite ». Le
    /// libellé ne le disait qu'une fois ARRIVÉ à zéro : trop tard.
    ///
    /// ⚠️ **LES DEUX PAGES SONT CONTRÔLÉES ICI, ET C'EST NÉCESSAIRE** : le popup de
    /// taille et la page des Réglages règlent la MÊME grandeur. En planchériser
    /// une seule laisserait le piège entier par l'autre — la première version du
    /// correctif n'en avait vu qu'une.
    ///
    /// ⚠️ **LES GESTES, EUX, GARDENT LE 0 DÉLIBÉRÉMENT** (le glisser d'un bord,
    /// et Maj + clic droit) : on y voit la zone se défaire sous la main. Ce test
    /// ne porte donc que sur les CURSEURS — ne pas l'étendre au glisser.
    #[test]
    fn les_deux_curseurs_de_cote_sont_plancheres() {
        let popup = include_str!("../../assets/popups/spacer_size.html");
        let ligne = popup.lines().find(|l| l.contains(r#"id="cote""#)).expect("le curseur de cote du popup");
        assert!(
            !ligne.contains(r#"min="0""#),
            "le curseur de cote du popup descend a 0 : il DEFAIT la zone. Ligne : {ligne}"
        );

        let reglages = include_str!("../../assets/popups/settings.html");
        let depart = reglages.find("Hauteur déclarée (cote)").expect("le curseur de cote des Réglages");
        let bloc = &reglages[depart..(depart + 220).min(reglages.len())];
        assert!(
            !bloc.contains("min: 0"),
            "le curseur de cote des Réglages descend a 0 : il DEFAIT la zone. Bloc : {bloc}"
        );
    }
}
