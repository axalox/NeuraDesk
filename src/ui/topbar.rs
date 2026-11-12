//! Éléments de la barre supérieure autres que les icônes du dock (voir
//! `ui::dock::Side::TopLeft`/`TopRight` pour celles-ci, y compris désormais
//! alimentation/terminal déroulant/presse-papiers rapide/son/réseau — voir
//! `ui::dock::Dock::new` — devenues des icônes du dock COMME LES AUTRES) :
//! les pastilles de bureaux virtuels et les contrôles multimédias (voir
//! `win32::media`) côté gauche, l'horloge centrée (cliquable pour un mini
//! calendrier) et le moniteur système — façon barre supérieure GNOME/Seelen
//! UI.

use crate::gfx::QuadInstance;
use crate::ui::dock::{hit, top_icon_size_for, DockLayout};

/// Marge laissée au-dessus et en dessous de TOUS les textes de la barre du
/// haut (horloge, moniteur système, fréquence d'affichage, titre média), en
/// FRACTION de l'épaisseur de la barre plutôt qu'en pixels fixes — BUG RÉEL
/// SIGNALÉ ("la taille des textes se réduit trop vite") : avec une marge en
/// pixels fixes, `font_px = thickness - 2*marge` est une fonction AFFINE de
/// l'épaisseur, pas proportionnelle — la marge (constante) mange une part
/// de plus en plus grande de la barre à mesure qu'elle rétrécit, donc le
/// texte rapetisse plus vite que la barre elle-même (ex. barre ÷4.5 →
/// police ÷7.75). Une marge exprimée en fraction de `layout.top_thickness`
/// donne `font_px = thickness * (1 - 2*fraction)`, une fonction linéaire
/// PROPORTIONNELLE : le texte rétrécit exactement au même rythme que la
/// barre. La fraction ci-dessous est calibrée pour retrouver la marge de
/// l'horloge (5px) à l'épaisseur de référence
/// `screen_dock::DEFAULT_BAR_THICKNESS_PX` (48px).
///
/// DEMANDE EXPLICITE ("que la taille des textes dans les barres
/// supérieures s'adaptent comme la taille du texte de l'horloge") :
/// moniteur système/fréquence d'affichage/titre média utilisaient chacun
/// leur propre marge (plus large, façon "indicateur secondaire"), donc une
/// police relativement plus petite ET qui rétrécissait plus vite que
/// l'horloge à mesure que la barre s'affine. Une seule constante partagée,
/// utilisée par les quatre — voir `topbar_text_font_px` ci-dessous.
const TOPBAR_TEXT_VERTICAL_MARGIN_FRACTION: f32 = 5.0 / 48.0;
/// Hauteur de police minimale des textes de la barre du haut, au cas où la
/// barre serait trop fine pour la marge ci-dessus (ne devrait pas arriver
/// en pratique).
const TOPBAR_TEXT_FONT_PX_MIN: i32 = 8;

/// Fraction de l'épaisseur de la barre du haut que la taille PERSONNALISÉE ne
/// peut pas dépasser — DÉCISION UTILISATEUR (2026-09-23) : « plafonné par la
/// barre », choisie contre une plage fixe, pour qu'un texte réglé à la main ne
/// puisse JAMAIS déborder de sa barre. Le plafond suit donc l'épaisseur, il ne
/// la précède pas — voir `topbar_text_font_px_max`.
///
/// 0,9 plutôt que 1,0 : à l'épaisseur exacte, le texte toucherait les deux
/// bords. C'est au-dessus de ce que donne l'automatique (voir
/// `TOPBAR_TEXT_VERTICAL_MARGIN_FRACTION`, ≈ 0,79), donc le curseur a toujours
/// de la marge au-dessus du réglage automatique.
const TOPBAR_TEXT_CUSTOM_MAX_FRACTION: f32 = 0.9;

/// Hauteur de police AUTOMATIQUE des textes de la barre du haut — celle qui
/// s'applique tant que l'utilisateur n'a pas fixé de taille (voir
/// `topbar_text_font_px`). Exposée pour que le panneau de réglages puisse
/// MONTRER la valeur dont on s'écarte : régler à l'aveugle une taille dont on
/// ne connaît pas la valeur automatique n'a pas de sens.
/// `top_thickness` plutôt qu'un `&DockLayout` : c'est la SEULE chose dont cette
/// taille dépend, et le panneau de réglages a l'épaisseur sous la main sans
/// avoir de disposition à construire (voir `win32::cef_settings`, qui la pousse
/// à la page pour qu'elle puisse AFFICHER la valeur automatique).
pub fn topbar_text_font_px_auto(top_thickness: f32) -> i32 {
    let px = top_thickness * (1.0 - 2.0 * TOPBAR_TEXT_VERTICAL_MARGIN_FRACTION);
    (px.round() as i32).max(TOPBAR_TEXT_FONT_PX_MIN)
}

/// Hauteur de police MAXIMALE réglable pour un texte de la barre du haut, à
/// cette épaisseur-ci — voir `TOPBAR_TEXT_CUSTOM_MAX_FRACTION`.
///
/// **C'est le MAXIMUM DU CURSEUR**, pas seulement une borne de sécurité : la
/// page de réglages le reçoit et dimensionne son curseur avec (voir
/// `win32::cef_settings`). Amincir la barre du haut RÉTRÉCIT donc la plage
/// réglable, et une taille devenue trop grande pour la nouvelle épaisseur est
/// ramenée par `topbar_text_font_px` — jamais laissée à déborder.
///
/// Jamais sous `TOPBAR_TEXT_FONT_PX_MIN` : un curseur dont le maximum serait
/// inférieur au minimum n'aurait aucune valeur valide.
pub fn topbar_text_font_px_max(top_thickness: f32) -> i32 {
    let px = top_thickness * TOPBAR_TEXT_CUSTOM_MAX_FRACTION;
    (px.round() as i32).max(TOPBAR_TEXT_FONT_PX_MIN)
}

/// Hauteur de police commune à l'horloge/au moniteur système/à la
/// fréquence d'affichage/au titre média — voir
/// `TOPBAR_TEXT_VERTICAL_MARGIN_FRACTION`.
///
/// `custom` : la taille RÉGLÉE À LA MAIN pour la barre de cet écran
/// (DÉCISION UTILISATEUR, 2026-09-23 : « en laissant le choix du réglage
/// automatique ou de la taille personnalisée »), `None` = automatique.
///
/// **C'EST LA BASE QUI EST REMPLACÉE, PAS LES QUATRE TEXTES** (DÉCISION
/// UTILISATEUR) : l'horloge reste `CLOCK_FONT_SIZE_BOOST_PX` au-dessus et les
/// textes secondaires `TOPBAR_SECONDARY_TEXT_SIZE_STEP_PX` en dessous, comme
/// les constantes ci-dessous le veulent depuis toujours. Une taille réglée à
/// 40 donne donc une horloge à 48 et un moniteur système à 36 — les deux
/// demandes explicites qui avaient créé ces écarts tiennent à toute taille.
fn topbar_text_font_px(top_thickness: f32, custom: Option<f32>) -> i32 {
    match custom {
        Some(px) => (px.round() as i32).clamp(TOPBAR_TEXT_FONT_PX_MIN, topbar_text_font_px_max(top_thickness)),
        None => topbar_text_font_px_auto(top_thickness),
    }
}

/// Un "cran" de taille typographique de la barre du haut, en pixels de
/// police — unité commune au renfort de l'horloge (`CLOCK_FONT_SIZE_BOOST_PX`)
/// et au retrait des textes secondaires (`TOPBAR_SECONDARY_TEXT_SIZE_STEP_PX`),
/// façon paliers Segoe UI (12→10→8…) plutôt que des pixels arbitraires.
///
/// BUG RÉEL MESURÉ ("le texte de l'horloge n'est toujours pas plus grand") :
/// avec un cran de 2px, l'écart de POLICE demandée (6px, voir
/// `CLOCK_FONT_SIZE_BOOST_PX` + `TOPBAR_SECONDARY_TEXT_SIZE_STEP_PX`) ne se
/// traduisait qu'en ~3px d'écart de hauteur de glyphe RÉELLEMENT RENDUE
/// (mesuré par capture d'écran + analyse de pixels : hauteur d'encre des
/// chiffres de l'horloge 31px vs 28px pour le moniteur système, alors que
/// les tailles de police demandées étaient 54px vs 44px) — `CreateFontW`
/// dimensionne sur la hauteur de cellule (ascent+descent+interligne
/// interne), pas sur la hauteur d'encre des chiffres, donc un même écart de
/// points se tasse fortement à l'affichage. Cran doublé (2px -> 4px) pour
/// que l'écart reste visuellement net une fois rendu.
const TOPBAR_FONT_SIZE_STEP_PX: i32 = 4;

/// Renfort de taille de l'horloge par rapport aux autres textes de la barre
/// du haut — DEMANDE EXPLICITE ("boost le texte de l'horloge de 2 tailles") :
/// 2 crans (`TOPBAR_FONT_SIZE_STEP_PX`) ajoutés APRÈS le calcul
/// proportionnel commun (`topbar_text_font_px`), donc l'horloge reste plus
/// grande que les autres textes à n'importe quelle épaisseur de barre, pas
/// seulement à l'épaisseur de référence.
const CLOCK_FONT_SIZE_BOOST_PX: i32 = 2 * TOPBAR_FONT_SIZE_STEP_PX;

/// Écart, en pixels de police, entre l'horloge et les textes "secondaires"
/// (moniteur système/fréquence d'affichage/titre média) — DEMANDE EXPLICITE
/// ("réduis la taille des textes dans les barres supérieures de une taille
/// par rapport au texte de l'horloge") : 1 cran (`TOPBAR_FONT_SIZE_STEP_PX`),
/// PAS une nouvelle marge propre en fraction de l'épaisseur — ça referait
/// retomber dans le bug "la taille des textes se réduit trop vite" (voir
/// `TOPBAR_TEXT_VERTICAL_MARGIN_FRACTION`, doc).
///
/// Soustrait du calcul commun `topbar_text_font_px`, PAS de `clock_font_px`
/// — BUG RÉEL CORRIGÉ ("c'est le texte de l'horloge qui doit être plus
/// grand que les autres") : une première version soustrayait de
/// `clock_font_px` (donc `topbar_text_font_px + CLOCK_FONT_SIZE_BOOST_PX -
/// TOPBAR_SECONDARY_TEXT_SIZE_STEP_PX`), ce qui faisait suivre aux textes
/// secondaires le renfort de taille de l'horloge — l'écart entre les deux
/// restait constant, mais l'horloge ne se détachait donc PAS davantage des
/// autres textes qu'avant l'agrandissement (les deux grossissaient
/// ensemble). En repartant du calcul commun (non boosté), seule l'horloge
/// profite de `CLOCK_FONT_SIZE_BOOST_PX` : l'écart avec les textes
/// secondaires est maintenant `CLOCK_FONT_SIZE_BOOST_PX +
/// TOPBAR_SECONDARY_TEXT_SIZE_STEP_PX`, l'horloge se démarque réellement.
const TOPBAR_SECONDARY_TEXT_SIZE_STEP_PX: i32 = TOPBAR_FONT_SIZE_STEP_PX;

/// Hauteur de police des textes "secondaires" de la barre du haut (moniteur
/// système/fréquence d'affichage/titre média) — sous l'horloge, voir
/// `TOPBAR_SECONDARY_TEXT_SIZE_STEP_PX`.
fn topbar_secondary_text_font_px(layout: &DockLayout, custom: Option<f32>) -> i32 {
    (topbar_text_font_px(layout.top_thickness, custom) - TOPBAR_SECONDARY_TEXT_SIZE_STEP_PX).max(TOPBAR_TEXT_FONT_PX_MIN)
}

pub const CLOCK_FONT_NAME: &str = "Segoe UI";

/// Hauteur de police à demander pour l'horloge — voir `topbar_text_font_px`.
/// `custom` : voir `topbar_text_font_px`, c'est la MÊME taille de référence —
/// le renfort de l'horloge s'ajoute par-dessus, réglage manuel ou pas.
pub fn clock_font_px(layout: &DockLayout, custom: Option<f32>) -> i32 {
    topbar_text_font_px(layout.top_thickness, custom) + CLOCK_FONT_SIZE_BOOST_PX
}

/// Rectangle de l'horloge (texte déjà rendu, `text_w`×`text_h` en pixels),
/// centré horizontalement et verticalement dans la barre du haut.
pub(crate) fn clock_rect(layout: &DockLayout, text_w: f32, text_h: f32) -> [f32; 4] {
    let x = (layout.total_width - text_w) * 0.5;
    let y = (layout.top_thickness - text_h) * 0.5;
    [x, y, text_w, text_h]
}

/// Rectangle à DESSINER de l'horloge quand elle est MAGNIFIÉE — sa case grossie
/// par `scale`, **autour de son centre et sans aucun rappel dans la fenêtre**
/// (voir `dock::magnified_box_centred`) : DEMANDE EXPLICITE, le texte ne doit pas
/// se DÉPLACER, seulement s'agrandir et osciller — or le rappel-à-l'écran le
/// translatait vers le bas dès que la boîte dépassait le haut de la fenêtre, ce
/// qui arrive toujours ici, l'horloge étant dans la barre du haut.
///
/// `layout` n'est plus lu : la signature est conservée pour ne pas changer ses
/// appelants (dessin, clic, débordement de région), qui passent tous la même
/// disposition.
///
/// À `scale = 1.0`, rend `clock_rect` **tel quel** : au repos, rien ne bouge.
pub fn presented_clock_rect(layout: &DockLayout, text_w: f32, text_h: f32, scale: f32) -> [f32; 4] {
    crate::ui::dock::magnified_box_centred(clock_rect(layout, text_w, text_h), scale)
}

/// Index de l'horloge dans le tableau d'ÉCHELLES rendu par `app::topbar_hit_scales`.
///
/// **APRÈS les trois boutons média, dont les indices 0/1/2 sont POSITIONNELS** :
/// ne jamais insérer d'élément au milieu de cette liste — tout ce qui lit une
/// échelle par index (dessin, clic, zone libre) se décalerait en silence. Cet
/// avertissement vaut pour les deux tableaux de rectangles correspondants
/// (`app::run`), média et horloge.
///
/// Ce n'est en revanche **plus** un index dans un tableau de rectangles unique :
/// depuis que l'horloge a son propre facteur de magnification, elle a aussi son
/// propre `HoverAnimator` (`app::clock_hover`), donc l'index 3 ne désigne plus
/// rien dans `topbar_hover` — qui ne contient plus que les trois média.
pub const HOVER_CLOCK_INDEX: usize = 3;

/// Indice de l'icône « afficher le bureau » dans le tableau d'échelles de survol
/// (voir `app::topbar_hit_scales`).
///
/// **AJOUTÉE EN FIN DE TABLEAU, ET C'EST LA RÈGLE À SUIVRE POUR TOUT NOUVEL
/// ÉLÉMENT** : `HOVER_CLOCK_INDEX` vaut `3` et le reste. Un élément inséré AVANT
/// lui décalerait silencieusement toutes les échelles — l'horloge lirait celle
/// d'un bouton média, et l'erreur ne se verrait qu'à l'œil. C'est le piège que la
/// doc de `HOVER_CLOCK_INDEX` décrit ; ajouter à la fin est ce qui le neutralise.
///
/// **Elle magnifie depuis le 2026-09-19, révisant une décision antérieure** dont
/// `show_desktop_instance` portait le commentaire (« pas de magnification au
/// survol, décision explicite »). Ses deux motifs sont traités, pas contournés :
/// l'index, par cet ajout en fin de tableau ; les pastilles — son unique voisin —
/// par la même demande, widget par widget. DEMANDE EXPLICITE : « l'applet afficher
/// le bureau ne se grandifie pas », puis « tous les widgets de la barre du haut ».
pub const HOVER_SHOW_DESKTOP_INDEX: usize = 4;

/// Instance de quad pour l'horloge (texte déjà rendu par `gfx::text`,
/// régénéré uniquement quand l'affichage change — voir `app::run`), teinté
/// par `color`, grossi par `scale` et penché de `rotation` radians.
///
/// **C'est la BOÎTE qui porte la magnification** (`1.0` passé au constructeur,
/// voir `presented_clock_rect`) : même convention que
/// `Dock::external_glyph_instance`, et c'est ce qui fait que le rectangle
/// réellement dessiné est aussi celui du clic et de la région.
pub fn clock_instance(
    layout: &DockLayout,
    text_w: f32,
    text_h: f32,
    color: [f32; 3],
    scale: f32,
    rotation: f32,
) -> QuadInstance {
    let mut instance = QuadInstance::new_glyph_scaled(presented_clock_rect(layout, text_w, text_h, scale), color, 1.0);
    instance.rotation = rotation;
    instance
}

/// `true` si `(x, y)` tombe sur l'horloge — clic qui ouvre le mini
/// calendrier (voir `win32::menu::show_calendar_popup`, façon menu date de
/// GNOME).
///
/// Teste le rectangle PRÉSENTÉ, comme `media_*_hit`, et non la case au repos :
/// une horloge magnifiée doit rester cliquable là où elle EST, pas là où elle
/// était.
pub fn clock_hit(x: f32, y: f32, layout: &DockLayout, text_w: f32, text_h: f32, scale: f32) -> bool {
    hit(&presented_clock_rect(layout, text_w, text_h, scale), x, y)
}

/// Écart entre le moniteur système (CPU/RAM) et la zone occupée par les
/// icônes de la moitié droite de la barre du haut qui le précède désormais
/// (voir `ui::dock::Dock::top_right_zone_left_edge`).
const SYS_MONITOR_MARGIN_RIGHT: f32 = 16.0;
pub const SYS_MONITOR_FONT_NAME: &str = "Segoe UI";

/// Hauteur de police du moniteur système — même rythme que `clock_font_px`
/// (voir `TOPBAR_TEXT_VERTICAL_MARGIN_FRACTION`), un cran plus petit (voir
/// `TOPBAR_SECONDARY_TEXT_SIZE_STEP_PX`).
pub fn sys_monitor_font_px(layout: &DockLayout, custom: Option<f32>) -> i32 {
    topbar_secondary_text_font_px(layout, custom)
}

/// Rectangle du moniteur système (texte déjà rendu, `text_w`×`text_h`),
/// aligné à droite juste avant la zone occupée par les icônes de la moitié
/// droite de la barre du haut (voir `ui::dock::Dock::
/// top_right_zone_left_edge`, passé ici comme `next_left_edge` — PAS
/// forcément l'icône réseau : BUG RÉEL SIGNALÉ PAR LE PASSÉ, "les icones de
/// la barre supérieure ne s'adaptent pas... elles restent sur place"),
/// centré verticalement dans la barre du haut.
/// **Publique depuis le 2026-09-18** : l'infobulle de survol d'un widget de la
/// barre du haut (`app::topbar_widget_under`) a besoin du rectangle DESSINÉ, et
/// le recalculer de son côté aurait été une seconde formule de la même
/// géométrie — celle que le dessin et le clic partagent déjà.
pub fn sys_monitor_rect(next_left_edge: f32, layout: &DockLayout, text_w: f32, text_h: f32) -> [f32; 4] {
    let x = next_left_edge - SYS_MONITOR_MARGIN_RIGHT - text_w;
    let y = (layout.top_thickness - text_h) * 0.5;
    [x, y, text_w, text_h]
}

/// Instance de quad pour le moniteur système (texte déjà rendu par
/// `gfx::text` — voir `win32::sysinfo`, régénéré périodiquement plutôt
/// qu'à chaque frame, voir `app::run`), teinté par `color`. `next_left_edge` :
/// voir `sys_monitor_rect`.
pub fn sys_monitor_instance(next_left_edge: f32, layout: &DockLayout, text_w: f32, text_h: f32, color: [f32; 3]) -> QuadInstance {
    QuadInstance::new_glyph(sys_monitor_rect(next_left_edge, layout, text_w, text_h), color)
}

/// Bord gauche du moniteur système (CPU/RAM) — exposé pour que
/// `refresh_rate_rect` (l'indicateur de fréquence d'affichage, voir sa doc)
/// puisse s'y accrocher et se positionner juste avant lui, même mécanisme de
/// chaînage par `next_left_edge` que partout ailleurs dans ce fichier.
pub fn sys_monitor_left_edge(next_left_edge: f32, layout: &DockLayout, text_w: f32, text_h: f32) -> f32 {
    sys_monitor_rect(next_left_edge, layout, text_w, text_h)[0]
}

/// Écart entre l'indicateur de fréquence d'affichage et l'élément qui le
/// suit (le moniteur système CPU/RAM, voir `sys_monitor_left_edge`, ou
/// directement la zone d'icônes de droite si celui-ci est absent) — même
/// esprit que `SYS_MONITOR_MARGIN_RIGHT`.
const REFRESH_RATE_MARGIN_RIGHT: f32 = 16.0;
pub const REFRESH_RATE_FONT_NAME: &str = "Segoe UI";

/// Hauteur de police de l'indicateur de fréquence d'affichage — même
/// logique que `sys_monitor_font_px` (un cran sous l'horloge).
pub fn refresh_rate_font_px(layout: &DockLayout, custom: Option<f32>) -> i32 {
    topbar_secondary_text_font_px(layout, custom)
}

/// Rectangle de l'indicateur de fréquence d'affichage (texte déjà rendu,
/// `text_w`×`text_h`), aligné à droite juste avant `next_left_edge` (voir
/// `sys_monitor_left_edge`), centré verticalement dans la barre du haut.
///
/// **Publique depuis le 2026-09-18**, même raison que `sys_monitor_rect` : le
/// rectangle de l'infobulle doit être celui du dessin, pas un second calcul.
pub fn refresh_rate_rect(next_left_edge: f32, layout: &DockLayout, text_w: f32, text_h: f32) -> [f32; 4] {
    let x = next_left_edge - REFRESH_RATE_MARGIN_RIGHT - text_w;
    let y = (layout.top_thickness - text_h) * 0.5;
    [x, y, text_w, text_h]
}

/// Instance de quad pour l'indicateur de fréquence d'affichage (texte déjà
/// rendu par `gfx::text` — voir `win32::display::refresh_rate_hz`, régénéré
/// périodiquement plutôt qu'à chaque frame, voir `app::run`), teinté par
/// `color`. `next_left_edge` : voir `refresh_rate_rect`.
pub fn refresh_rate_instance(next_left_edge: f32, layout: &DockLayout, text_w: f32, text_h: f32, color: [f32; 3]) -> QuadInstance {
    QuadInstance::new_glyph(refresh_rate_rect(next_left_edge, layout, text_w, text_h), color)
}

/// Côté PAR DÉFAUT d'une pastille de bureau virtuel (voir
/// `win32::desktops`) — DEMANDE EXPLICITE : "au lieu de point de petite
/// taille [...] je voudrais des carrés assez grands pour être bien
/// visibles" (remplace l'ancien diamètre de 10px, trop discret, voir aussi
/// `desktop_dot_instance` pour le passage cercle → carré à coins légèrement
/// arrondis). Valeur de repli de `ui::dock::Dock::desktop_dot_size` —
/// DEMANDE EXPLICITE ULTÉRIEURE ("je voudrais que les pastilles des
/// bureaux virtuels soient redimensionnables dans le dash réglage, en
/// ayant chacune leur propre réglage dans la barre correspondante") : la
/// taille RÉELLEMENT utilisée est désormais un paramètre (voir
/// `desktop_dot_rect`/`desktop_dot_instance` ci-dessous), réglable par
/// écran (`app::SecondaryScreenOverrides::desktop_dot_sizes`) — cette
/// constante ne sert plus qu'à amorcer la valeur PRIMAIRE tant qu'elle n'a
/// jamais été personnalisée (voir `config::load_desktop_dot_size`).
pub(crate) const DEFAULT_DESKTOP_DOT_SIZE: f32 = 18.0;
/// Écart entre deux pastilles adjacentes, en FRACTION de leur taille — même
/// proportion que la constante d'origine (8px pour une pastille de 18px)
/// pour que l'espacement suive la taille réglable ci-dessus au lieu de
/// rester figé en pixels.
const DESKTOP_DOT_GAP_RATIO: f32 = 8.0 / DEFAULT_DESKTOP_DOT_SIZE;
/// Rayon des coins d'une pastille, en FRACTION de sa taille (même
/// raisonnement que `DESKTOP_DOT_GAP_RATIO`) — modeste (contrairement à
/// l'ancien `rect[2]` qui produisait un cercle plein) pour obtenir un carré
/// aux angles légèrement adoucis plutôt qu'un rectangle à angles vifs.
/// Le numérateur est `rayon_petit` du style actif (`ui::style`, 3 px pour
/// « NeuraDesk »).
fn desktop_dot_corner_radius_ratio() -> f32 {
    crate::ui::style::rayon_petit() / DEFAULT_DESKTOP_DOT_SIZE
}
/// Marge entre le coin de l'ÉCRAN (PAS le coin de la barre de gauche — voir
/// `ui::dock::TOP_ZONE_CORNER_MARGIN`, même valeur, même raison : la barre
/// de gauche ne s'étend jamais au-dessus de `layout.top_thickness`, rien à
/// éviter dans ce coin) et la première pastille.
const DESKTOP_DOTS_MARGIN_LEFT: f32 = 16.0;
/// Opacité d'une pastille inactive (le bureau actif est affiché à pleine
/// couleur d'accentuation, voir `desktop_dot_instance`).
const DESKTOP_DOT_INACTIVE_ALPHA: f32 = 0.35;

/// Rectangle de la pastille `index` (sur des bureaux virtuels affichés en
/// ligne), alignée à gauche de la barre du haut, juste après son coin.
/// `left_offset` : largeur occupée par d'éventuelles icônes du dock
/// déposées dans la moitié GAUCHE de la barre du haut (voir `ui::dock::
/// Dock::top_zone_widths`, même mécanisme côté droit avec `ui::dock::Dock::
/// top_right_zone_left_edge`) — décale les pastilles d'autant vers la
/// droite pour ne jamais les chevaucher ; MÊME ancre que cette zone
/// (`DESKTOP_DOTS_MARGIN_LEFT` == `ui::dock::TOP_ZONE_CORNER_MARGIN`, PAS
/// `layout.left_width` en plus, BUG RÉEL SIGNALÉ sur la zone elle-même —
/// voir sa doc) pour rester CONTIGUËS avec elle, jamais un blanc entre les
/// deux. `dot_size` : voir `ui::dock::Dock::desktop_dot_size`, réglable
/// PAR ÉCRAN.
pub fn desktop_dot_rect(layout: &DockLayout, index: usize, left_offset: f32, dot_size: f32) -> [f32; 4] {
    let gap = dot_size * DESKTOP_DOT_GAP_RATIO;
    let x = DESKTOP_DOTS_MARGIN_LEFT + left_offset + index as f32 * (dot_size + gap);
    let y = (layout.top_thickness - dot_size) * 0.5;
    [x, y, dot_size, dot_size]
}

/// Rectangle à DESSINER d'une pastille : sa boîte grossie par `scale`, puis
/// ramenée dans la fenêtre — même formule que `presented_media_rect` et
/// `presented_show_desktop_rect` (`ui::dock::presented_rect_for`), jamais une
/// seconde formule qui finirait par diverger.
pub fn presented_desktop_dot_rect(rect: [f32; 4], scale: f32, layout: &DockLayout) -> [f32; 4] {
    crate::ui::dock::presented_rect_for(rect, scale, layout)
}

/// Index (0-based) de la pastille sous `(x, y)`, si `count` bureaux
/// virtuels sont affichés — `None` si le clic tombe à côté.
///
/// `scales` : l'échelle de survol de CHAQUE pastille, dans l'ordre (voir
/// `app::run`, `dots_hover`). Le test porte sur le rectangle PRÉSENTÉ — celui
/// qu'on voit — jamais sur la case au repos, comme les boutons média et
/// l'icône « afficher le bureau ». Un tableau plus court que `count` retombe
/// sur `1.0` : une pastille qu'on n'a pas encore animée se teste à sa place.
pub fn desktop_dot_at(
    x: f32,
    y: f32,
    layout: &DockLayout,
    count: usize,
    left_offset: f32,
    dot_size: f32,
    scales: &[f32],
) -> Option<usize> {
    (0..count).find(|&i| {
        let scale = scales.get(i).copied().unwrap_or(1.0);
        let presented = presented_desktop_dot_rect(desktop_dot_rect(layout, i, left_offset, dot_size), scale, layout);
        hit(&presented, x, y)
    })
}

/// Couleur d'une pastille de bureaux virtuels : couleur d'accentuation à pleine
/// opacité si c'est le bureau actif, neutre discret et translucide sinon
/// (`dark_mode` choisit blanc ou noir pour rester visible sur les deux thèmes).
///
/// **EXTRAITE DE `desktop_dot_instance` POUR ÊTRE PARTAGÉE** : le numéro posé
/// sur la pastille (voir `ui::desktop_labels::color_for`) a besoin de savoir sur
/// QUELLE couleur il est écrit pour choisir la sienne. Deux calculs séparés
/// finiraient par diverger, et le numéro se retrouverait à contraster contre une
/// couleur qui n'est plus celle qu'on dessine.
///
/// ⚠️ L'alpha de la pastille inactive n'est PAS décoratif : c'est lui qui oblige
/// `color_for` à juger la couleur COMPOSÉE sur le fond de la barre
/// (`ui::color::composite_over`) plutôt que la couleur brute. Ne pas la rendre
/// opaque ici sans relire cette doc.
pub fn desktop_dot_color(active: bool, accent: [f32; 4], dark_mode: bool) -> [f32; 4] {
    if active {
        [accent[0], accent[1], accent[2], 1.0]
    } else if dark_mode {
        [1.0, 1.0, 1.0, DESKTOP_DOT_INACTIVE_ALPHA]
    } else {
        [0.0, 0.0, 0.0, DESKTOP_DOT_INACTIVE_ALPHA]
    }
}

/// Instance de quad (carré aux coins légèrement arrondis — voir
/// `desktop_dot_corner_radius_ratio`) pour la pastille `index` — voir
/// `desktop_dot_color` pour sa couleur.
pub fn desktop_dot_instance(
    layout: &DockLayout,
    index: usize,
    active: bool,
    accent: [f32; 4],
    dark_mode: bool,
    left_offset: f32,
    dot_size: f32,
    hover_scale: f32,
    hover: f32,
) -> QuadInstance {
    let rect = presented_desktop_dot_rect(desktop_dot_rect(layout, index, left_offset, dot_size), hover_scale, layout);
    let color = desktop_dot_color(active, accent, dark_mode);
    // Le RAYON suit l'échelle : la boîte grossit, donc un rayon constant rendrait
    // la pastille de moins en moins ronde à mesure qu'elle grandit.
    let mut instance = QuadInstance::new(rect, color, dot_size * desktop_dot_corner_radius_ratio() * hover_scale, 1.0);
    // Elle BALAIE comme les icônes et l'applet « afficher le bureau » (voir
    // `show_desktop_instance`) : une pastille est un élément de la barre, pas un
    // texte — c'est donc `hover_wobble_angle` (±45°), jamais la variante texte.
    //
    // ⚠️ **`hover` ET NON `hover_scale`** : la rotation suit l'AVANCEMENT DU
    // SURVOL (voir `dock::HoverAnimator::hover`), l'échelle ne servant qu'à la
    // géométrie. À magnification 100 %, seule la première dit que la pastille
    // est survolée — et c'est ce qui la fait balancer sans grossir.
    instance.rotation = crate::ui::dock::hover_wobble_angle(hover);
    instance
}

/// Icône « afficher le bureau » (voir `win32::show_desktop`) — le tout PREMIER
/// élément de la barre du haut, donc À GAUCHE des pastilles qui précèdent dans
/// ce fichier (l'ordre du fichier suit celui de l'écran). Un clic bascule
/// l'affichage du bureau, comme le bouton du bord droit de la barre des tâches.
///
/// Widget PROPRE À CE MODULE, pas une `ui::dock::Icon` comme les icônes de
/// fonctionnalités de la barre : sa place doit être fixe. Une `Dock::Icon`
/// s'ajoute en FIN de rangée (les index sont persistés dans `icon_order.txt`,
/// on n'insère jamais — voir `Features::notes`, doc) et ne pourrait donc pas
/// être garantie à l'extrême gauche.
///
/// Même ancre que les pastilles, et surtout même `left_offset` qu'elles : la
/// largeur des icônes du dock déposées dans la moitié GAUCHE de la barre (voir
/// `ui::dock::Dock::top_zone_widths`). C'est ce qui évite tout chevauchement
/// sur les écrans SECONDAIRES, où l'icône Dash occupe déjà ce coin (voir
/// `app::rebuild_secondary_docks`) : les deux se chaînent, icône puis
/// pastilles.
const SHOW_DESKTOP_MARGIN_LEFT: f32 = DESKTOP_DOTS_MARGIN_LEFT;
/// Blanc laissé entre cette icône et la première pastille qu'elle décale —
/// l'écartement par DÉFAUT des icônes du dock (`ui::dock::DEFAULT_ICON_GAP`)
/// plutôt qu'une valeur inventée ici : cette icône en est une de plus dans la
/// barre, elle doit respirer comme les autres.
///
/// À savoir : c'est la valeur par défaut, pas le réglage de l'utilisateur —
/// `ui::topbar` n'a aucun `Dock` sous la main, et les pastilles comme les
/// boutons média prennent déjà leur espacement dans des constantes. Si
/// l'utilisateur règle l'écartement des icônes du dock, celle-ci ne suivra pas.
const SHOW_DESKTOP_GAP: f32 = crate::ui::dock::DEFAULT_ICON_GAP;

/// INTERVALLES de la rangée de widgets — les blancs où des SÉPARATEURS peuvent
/// être posés. DEMANDE EXPLICITE, réitérée : « je veux pouvoir insérer des
/// séparateurs entre tous les widgets de la barre du haut », puis « je n'arrive
/// pas à ajouter PLUS D'UN séparateur entre le widget pour afficher le bureau et
/// la bascule de bureau ».
///
/// **VOCABULAIRE, et il compte** : un *intervalle* (`TopSlot`) est un BLANC entre
/// deux widgets, et il porte une LISTE de séparateurs (voir `SlotWidths`). Trois
/// intervalles, donc, mais autant de séparateurs qu'on veut dans chacun — « un
/// par intervalle » était la règle jusqu'au 2026-09-19, et c'est ce qui a changé.
///
/// **Identifié par la BORNE qui le précède**, jamais par un rang : masquer un
/// widget ne doit pas renuméroter les autres — même raison que `zone_scroll`,
/// que ce dépôt repère par l'index du SÉPARATEUR et non par le rang de la zone.
///
/// Ce sont les intervalles que la disposition rend ATTEIGNABLES. Les deux bouts
/// de la rangée n'y sont pas : un espace en fin de liste d'icônes les couvre
/// déjà (`top_zone_widths`/`top_right_zone_left_edge`). Les abords de l'horloge
/// non plus : elle est CENTRÉE, hors de toute chaîne, et les décaler demande une
/// règle à part.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TopSlot {
    /// Entre « afficher le bureau » et la première pastille de bureaux virtuels.
    AfterShowDesktop,
    /// Entre la dernière pastille et le premier bouton média.
    AfterDots,
    /// Après le bloc média — son TITRE compris, qui en fait partie.
    AfterMedia,
}

/// Tous les emplacements, dans l'ordre de l'écran — pour les boucles (dessin,
/// persistance, gestes) qui ne doivent pas recopier la liste.
pub const TOP_SLOTS: [TopSlot; 3] = [TopSlot::AfterShowDesktop, TopSlot::AfterDots, TopSlot::AfterMedia];

/// Rang d'un INTERVALLE dans `TOP_SLOTS` — la seule correspondance entre l'enum
/// et le tableau, donc l'endroit à changer si un intervalle s'ajoute.
///
/// **Ne pas le confondre avec le `rank` d'un SÉPARATEUR** (`marker_offset`,
/// `remove_slot_width`…), qui numérote les séparateurs DANS un intervalle : deux
/// numérotations différentes, sur deux axes différents.
fn slot_rank(slot: TopSlot) -> usize {
    match slot {
        TopSlot::AfterShowDesktop => 0,
        TopSlot::AfterDots => 1,
        TopSlot::AfterMedia => 2,
    }
}

/// Nom PERSISTÉ d'un intervalle — c'est lui qui va dans le fichier, JAMAIS le
/// rang : ajouter un intervalle au milieu de `TOP_SLOTS` déplacerait sinon les
/// séparateurs de tout le monde (même raison que la borne au lieu du rang).
///
/// C'est aussi lui qui désigne l'intervalle dans `SpacerTarget::TopSlot`, aux
/// côtés du `rank` du séparateur — le nom dit OÙ, le rang dit LEQUEL.
pub fn slot_name(slot: TopSlot) -> &'static str {
    match slot {
        TopSlot::AfterShowDesktop => "apres-afficher-le-bureau",
        TopSlot::AfterDots => "apres-les-pastilles",
        TopSlot::AfterMedia => "apres-les-medias",
    }
}

/// Emplacement portant ce nom persisté — `None` pour un nom inconnu, qui est
/// alors IGNORÉ (une ligne d'un fichier écrit par une version future ne doit ni
/// planter ni être confondue avec un autre emplacement).
pub fn slot_from_name(name: &str) -> Option<TopSlot> {
    TOP_SLOTS.into_iter().find(|&slot| slot_name(slot) == name)
}

/// Largeur qu'un séparateur prend quand on le POSE — assez large pour se voir et
/// pour être visé à la souris, assez étroite pour ne pas bousculer la rangée.
/// Elle se règle ensuite par le curseur de taille, comme n'importe quel espace.
pub const DEFAULT_SLOT_WIDTH: f32 = 40.0;
/// Plafond d'UN séparateur : au-delà, la rangée serait déplacée par un seul
/// d'entre eux — ce sont des blancs entre deux widgets, pas une mise en page.
///
/// **Le plafond d'un INTERVALLE est tout autre** : c'est
/// `MAX_SLOTS_PER_INTERVAL × MAX_SLOT_WIDTH`, puisque rien n'interdit d'en
/// empiler plusieurs (voir `SlotWidths`).
pub const MAX_SLOT_WIDTH: f32 = 400.0;

/// Nombre MAXIMUM de séparateurs dans un même intervalle.
///
/// **Un tableau FIXE, et c'est un choix, pas une limite subie** : `[f32; 8]` est
/// `Copy` comme `[f32; 3]` l'était, donc toutes les signatures qui passent
/// `SlotWidths` PAR VALEUR restent valables et le style de ce module — des
/// fonctions PURES dont l'appelant porte la valeur — tient sans un seul emprunt
/// supplémentaire. Un `Vec<f32>` aurait forcé `&SlotWidths` partout, et
/// `Dock::set_slot_widths` se serait retrouvé à emprunter le dock en MUTABLE
/// pendant qu'on le lit. Précédent : `bar_locks: [bool; 3]`,
/// `bar_colors: [Option<[f32; 4]>; 3]`.
///
/// 8 × `MAX_SLOT_WIDTH` (400) = 3200 px, plus large que n'importe quel écran :
/// le plafond n'est donc pas atteignable en pratique. **L'atteindre n'est
/// pourtant PAS un silence** — `insert_slot_width` rend `None` et l'appelant le
/// dit (règle du dépôt : un geste qui rate doit rater, jamais faire autre chose).
pub const MAX_SLOTS_PER_INTERVAL: usize = 8;

/// Les séparateurs d'UN intervalle, **tassés au début** : les rangs
/// `0..slot_count` sont occupés, tous les suivants valent `0.0`.
pub type SlotInterval = [f32; MAX_SLOTS_PER_INTERVAL];

/// L'ENSEMBLE des séparateurs d'une barre du haut, un intervalle (`TopSlot`) par
/// entrée.
///
/// **La forme canonique est « tassé au début, sans trou »**, et c'est un
/// INVARIANT, pas une convention : `canonical_interval` est la seule fonction qui
/// le pose, et tous les chemins d'écriture passent par elle — le curseur comme le
/// fichier. Deux définitions de cet invariant est exactement ce que ce dépôt paie
/// à répétition.
///
/// **PAR ÉCRAN** : ces largeurs vivent sur le `Dock` (`ui::dock::Dock::slot_widths`),
/// comme `icon_gap` ou `separator_color`, donc un séparateur posé sur la barre
/// primaire n'apparaît plus sur les secondaires — DÉFAUT RÉEL SIGNALÉ : « ça ne
/// devrait pas être le cas, chaque interface doit avoir ses propres réglages sur
/// chaque écran ». Ce module ne les STOCKE pas : il ne sait que quel rang
/// correspond à quel intervalle (`slot_rank`).
pub type SlotWidths = [SlotInterval; 3];

/// **Aucun séparateur nulle part** — le défaut d'une barre jamais réglée, et ce
/// par quoi le chargement de la configuration commence pour CHAQUE écran.
pub const NO_SLOT_WIDTHS: SlotWidths = [[0.0; MAX_SLOTS_PER_INTERVAL]; 3];

/// Le BORNAGE d'une largeur de séparateur, **ici et nulle part ailleurs** : la
/// valeur vient d'un curseur ou d'un fichier, dont la valeur EST la valeur (même
/// règle que `Dock::set_separator_size`). `canonical_interval` passe par ici,
/// donc ni le curseur ni le chargement ne peuvent contourner la borne.
pub fn clamp_slot_width(px: f32) -> f32 {
    px.clamp(0.0, MAX_SLOT_WIDTH)
}

/// Borne PUIS tasse un intervalle — **la SEULE définition de l'invariant** « les
/// rangs occupés sont `0..n`, sans trou ».
///
/// Un `0.0` n'est PAS un séparateur de largeur nulle : c'est un rang LIBRE. Le
/// curseur descend à 4 px et `clamp_slot_width` ne laisse rien passer en dessous
/// de 0, donc aucun chemin ne peut produire un séparateur invisible — un zéro ne
/// peut venir que d'une case non écrite, et il disparaît en faisant remonter les
/// suivants.
pub fn canonical_interval(row: SlotInterval) -> SlotInterval {
    let mut out = [0.0; MAX_SLOTS_PER_INTERVAL];
    let mut next = 0;
    for px in row {
        let px = clamp_slot_width(px);
        if px > 0.0 && next < MAX_SLOTS_PER_INTERVAL {
            out[next] = px;
            next += 1;
        }
    }
    out
}

/// Les trois intervalles canonicalisés d'un coup (voir `canonical_interval`).
pub fn canonical(widths: SlotWidths) -> SlotWidths {
    widths.map(canonical_interval)
}

/// Nombre de séparateurs d'un intervalle.
pub fn slot_count(slots: SlotWidths, slot: TopSlot) -> usize {
    slots[slot_rank(slot)].iter().take_while(|&&px| px > 0.0).count()
}

/// Largeur TOTALE d'un intervalle — **la SOMME de ses séparateurs**, et c'est
/// ELLE que consomment les chaînages (`app::dots_left_offset`,
/// `app::media_left_offset`) : un blanc de trois séparateurs décale d'autant que
/// les trois ensemble.
///
/// **À NE PAS CONFONDRE avec `slot_width_at`** (la largeur d'UN rang), et c'est
/// pourquoi l'ancien `slot_width` a été SUPPRIMÉ au lieu d'être renommé : chaque
/// site d'appel est devenu une erreur de compilation, et l'ambiguïté « la somme
/// ou le rang ? » a été tranchée une fois pour toutes plutôt que relue.
pub fn interval_width(slots: SlotWidths, slot: TopSlot) -> f32 {
    occupied_widths(slots, slot).sum()
}

/// Largeur du `rank`-ième séparateur — `None` s'il n'existe pas.
pub fn slot_width_at(slots: SlotWidths, slot: TopSlot, rank: usize) -> Option<f32> {
    slots[slot_rank(slot)].get(rank).copied().filter(|&px| px > 0.0)
}

/// Les largeurs occupées d'un intervalle, dans l'ordre des rangs.
///
/// **Itérateur PROPRIÉTAIRE et non `&[f32]`** : `SlotWidths` est reçu PAR VALEUR
/// partout, donc une tranche emprunterait une valeur locale et ne pourrait pas
/// sortir de la fonction. Un `impl Iterator` n'a aucune durée de vie à porter et
/// n'alloue rien.
pub fn occupied_widths(slots: SlotWidths, slot: TopSlot) -> impl Iterator<Item = f32> {
    slots[slot_rank(slot)].into_iter().take(slot_count(slots, slot))
}

/// Distance de l'ANCRE au MILIEU du `rank`-ième marqueur, **dans le sens du
/// rang** (toujours positive : la direction de l'intervalle est appliquée par
/// l'appelant, voir `app::slot_anchor_dir`).
pub fn marker_offset(slots: SlotWidths, slot: TopSlot, rank: usize) -> Option<f32> {
    if rank >= slot_count(slots, slot) {
        return None;
    }
    let row = slots[slot_rank(slot)];
    Some(row.iter().take(rank).sum::<f32>() + row[rank] * 0.5)
}

/// Rang auquel INSÉRER un clic situé à `offset` de l'ancre, dans le sens du rang
/// — le nombre de marqueurs dont le milieu PRÉCÈDE ce point.
///
/// **Le clic ne choisit qu'un RANG, jamais une position**, et il faut le dire
/// clairement : dans un blanc jointif il n'y a PAS d'espace libre entre deux
/// séparateurs, donc le marqueur neuf se pose à la FRONTIÈRE de ce rang et non
/// sous le curseur. Ce qui est vrai est plus faible, et suffit : il apparaît **du
/// côté où l'on a cliqué**. Et la propriété qui rend les clics répétés utilisables
/// — celle qui justifie toute cette conception — est que **la bande après un
/// ajout contient toujours la bande d'avant** : on ne perd jamais la zone qu'on
/// vient de viser.
pub fn insert_rank(slots: SlotWidths, slot: TopSlot, offset: f32) -> usize {
    let row = slots[slot_rank(slot)];
    let mut before = 0.0;
    for rank in 0..slot_count(slots, slot) {
        if offset < before + row[rank] * 0.5 {
            return rank;
        }
        before += row[rank];
    }
    slot_count(slots, slot)
}

/// Insère un séparateur au rang `rank` — `None` si l'intervalle est **PLEIN** ou
/// si la largeur ne donne pas un séparateur (voir `MAX_SLOTS_PER_INTERVAL` :
/// `None` est ce qui empêche le plafond d'être un silence).
pub fn insert_slot_width(slots: SlotWidths, slot: TopSlot, rank: usize, px: f32) -> Option<SlotWidths> {
    let px = clamp_slot_width(px);
    let count = slot_count(slots, slot);
    if px <= 0.0 || count >= MAX_SLOTS_PER_INTERVAL {
        return None;
    }
    let mut row = slots[slot_rank(slot)];
    // Décale la queue d'un cran vers la droite — `rank.min(count)` borne le rang,
    // et `count < MAX` garantit que la destination tient (le rang `count` est
    // libre par définition).
    row.copy_within(rank.min(count)..count, rank.min(count) + 1);
    row[rank.min(count)] = px;
    Some(set_interval_row(slots, slot, row))
}

/// Retire le séparateur `rank` — `None` s'il n'existe pas. Les suivants
/// REMONTENT : l'invariant « sans trou » est reposé par `set_interval_row`.
pub fn remove_slot_width(slots: SlotWidths, slot: TopSlot, rank: usize) -> Option<SlotWidths> {
    let count = slot_count(slots, slot);
    if rank >= count {
        return None;
    }
    let mut row = slots[slot_rank(slot)];
    row.copy_within(rank + 1..count, rank);
    row[count - 1] = 0.0;
    Some(set_interval_row(slots, slot, row))
}

/// Fixe la largeur du `rank`-ième séparateur — `px <= 0` le RETIRE (même chose
/// que « absent », voir `remove_slot_width`), et `None` pour un rang qui
/// n'existe pas.
pub fn set_slot_width_at(slots: SlotWidths, slot: TopSlot, rank: usize, px: f32) -> Option<SlotWidths> {
    let px = clamp_slot_width(px);
    if px <= 0.0 {
        return remove_slot_width(slots, slot, rank);
    }
    let mut row = slots[slot_rank(slot)];
    if rank >= slot_count(slots, slot) {
        return None;
    }
    row[rank] = px;
    Some(set_interval_row(slots, slot, row))
}

/// Écrit un intervalle depuis un FICHIER : borne, écarte les zéros, tasse (par
/// `canonical_interval`) et **TRONQUE au plafond**.
///
/// **La troncature n'est pas un détail de confort** : `panic = "abort"` est actif
/// dans ce binaire (voir `src/diag.rs`), donc un `copy_from_slice` d'une ligne de
/// douze valeurs dans un `[f32; 8]` TUERAIT le processus sans un mot de journal.
/// Le `zip` s'arrête au plus court, et rien ici ne peut indexer hors bornes —
/// une ligne écrite à la main ne doit jamais pouvoir faire ça.
pub fn set_interval_widths(slots: SlotWidths, slot: TopSlot, widths: &[f32]) -> SlotWidths {
    let mut row = [0.0; MAX_SLOTS_PER_INTERVAL];
    for (cell, px) in row.iter_mut().zip(widths) {
        *cell = *px;
    }
    set_interval_row(slots, slot, row)
}

/// Le même jeu, avec l'intervalle `slot` remplacé — **le point d'écriture UNIQUE
/// de ce module**, donc aucun chemin ne peut poser un intervalle non canonique.
/// `canonical_interval` est appelée ICI et pas chez les appelants : c'est ce qui
/// fait de l'invariant une propriété du type plutôt qu'une discipline.
fn set_interval_row(slots: SlotWidths, slot: TopSlot, row: SlotInterval) -> SlotWidths {
    let mut next = slots;
    next[slot_rank(slot)] = canonical_interval(row);
    next
}

/// Diamètre MINIMUM de la cible de RETRAIT d'un séparateur — le disque dessiné,
/// avec un plancher pour qu'un marqueur réglé très petit reste visable.
pub const MARKER_HIT_MIN_DIAMETER: f32 = 16.0;

/// Rayon de la cible de RETRAIT d'un séparateur — **le disque DESSINÉ, jamais
/// moins**.
///
/// ⚠️ **CE RAYON NE PEUT PAS ÊTRE AGRANDI PAR L'UTILISATEUR**, et il faut le
/// savoir avant de s'appuyer dessus : `separator_size` est borné par
/// `MAX_SEPARATOR_SIZE = SPACER_HEIGHT` (18 px), donc cette cible plafonne à
/// 9 px de rayon sur une barre 4K. On ne peut pas non plus relever le plancher :
/// sur un séparateur réglé au minimum, un disque large mordrait sur ses voisins
/// et un clic d'AJOUT retirerait le mauvais. D'où la règle qui accompagne ce
/// rayon — **le marqueur le plus proche gagne** (voir `app::top_slot_at`).
pub fn marker_hit_radius(separator_size: f32) -> f32 {
    separator_size.max(MARKER_HIT_MIN_DIAMETER) * 0.5
}

/// Vrai si le point `(x, y)` — coordonnées FENÊTRE — tombe dans le DISQUE de
/// retrait d'un marqueur, sœur de `slot_contains` qui teste la CASE : **on retire
/// ce qu'on voit** (le cercle), alors qu'on survole la case entière.
///
/// **Seule l'ABSCISSE est testée**, comme `slot_contains` : le marqueur est centré
/// verticalement dans la barre, et exiger en plus la distance verticale rendrait
/// la cible deux fois plus difficile à atteindre sans rien apporter — la barre
/// fait 72 px de haut pour un disque de 16.
pub fn marker_contains(mid: f32, radius: f32, layout: &DockLayout, x: f32, y: f32) -> bool {
    (0.0..layout.top_thickness).contains(&y) && (x - mid).abs() <= radius
}

/// Vrai si le point `(x, y)` — coordonnées FENÊTRE — tombe dans la CASE d'un
/// séparateur dont le marqueur est à `mid` (sa largeur).
///
/// **La case, pas le seul cercle, et c'est le contraire de `marker_contains`** :
/// le SURVOL s'adresse à toute la case (comme un espace du dock, qui teste son
/// emplacement entier — voir `ui::dock::Dock::spacer_hover_line`), alors que le
/// RETRAIT s'adresse au cercle. Un trait de survol ne coûte rien à déclencher ;
/// un retrait, si.
pub fn slot_contains(mid: f32, width: f32, layout: &DockLayout, x: f32, y: f32) -> bool {
    (0.0..layout.top_thickness).contains(&y) && (x - mid).abs() <= width * 0.5
}

/// Rectangle du trait de SURVOL d'un séparateur de la rangée — PURE, donc
/// testable, et sœur de `ui::dock::spacer_hover_line_rect`.
///
/// **La barre du haut se traverse dans l'AUTRE SENS**, et c'est tout ce qui
/// change : un espace latéral est un blanc VERTICAL, donc son trait est
/// horizontal et traverse la largeur de la barre ; un séparateur de la rangée
/// est un blanc HORIZONTAL, donc son trait est vertical et traverse l'ÉPAISSEUR
/// de la barre. L'épaisseur du trait et la marge aux bords sont celles des
/// espaces du dock (voir `ui::dock::SPACER_HOVER_LINE_PX`) : les deux familles
/// doivent se lire comme la même chose.
pub fn slot_hover_line_rect(mid: f32, layout: &DockLayout) -> [f32; 4] {
    let px = crate::ui::dock::SPACER_HOVER_LINE_PX;
    let margin = crate::ui::dock::SPACER_HOVER_LINE_MARGIN;
    [mid - px * 0.5, margin, px, (layout.top_thickness - margin * 2.0).max(0.0)]
}

/// Instance du trait de survol d'un emplacement — arrondi à la moitié de son
/// épaisseur, donc deux extrémités en demi-cercle, exactement comme
/// `ui::dock::spacer_line_instance` (voir `slot_hover_line_rect`).
pub fn slot_hover_line_instance(mid: f32, layout: &DockLayout, color: [f32; 4]) -> QuadInstance {
    let px = crate::ui::dock::SPACER_HOVER_LINE_PX;
    QuadInstance::new(slot_hover_line_rect(mid, layout), color, px * 0.5, 1.0)
}

/// De combien les pastilles (et tout ce qui est ancré à gauche après elles)
/// reculent quand cette icône est affichée : sa largeur plus le blanc qui la
/// sépare de la suivante.
///
/// **Seule définition de ce décalage** — pure, donc testable, et c'est ce qui
/// garantit que le dessin des pastilles et leur test de clic restent d'accord
/// (les deux la consomment, voir `app::run`).
pub fn show_desktop_shift(layout: &DockLayout) -> f32 {
    top_icon_size_for(layout.top_thickness) + SHOW_DESKTOP_GAP
}

/// Rectangle de l'icône — même forme que `media_button_rect` : taille
/// `top_icon_size_for`, centrée verticalement dans la barre.
pub fn show_desktop_rect(layout: &DockLayout, left_offset: f32) -> [f32; 4] {
    let icon_size = top_icon_size_for(layout.top_thickness);
    [SHOW_DESKTOP_MARGIN_LEFT + left_offset, (layout.top_thickness - icon_size) * 0.5, icon_size, icon_size]
}

/// Rectangle à DESSINER de l'icône : sa boîte grossie par `scale`, puis RAMENÉE
/// DANS LA FENÊTRE — voir `ui::dock::presented_rect_for`, la même formule que
/// `presented_media_rect` (une seconde formule finirait par diverger).
///
/// **C'est aussi ce qui règle son cas de bord, sans code particulier** : l'icône
/// occupe l'extrémité GAUCHE de la barre, donc une magnification centrée
/// sortirait de la fenêtre par la gauche — `presented_rect_for` l'y ramène.
pub fn presented_show_desktop_rect(rect: [f32; 4], scale: f32, layout: &DockLayout) -> [f32; 4] {
    crate::ui::dock::presented_rect_for(rect, scale, layout)
}

/// Test de clic — `scale` : l'échelle de survol courante (voir
/// `HOVER_SHOW_DESKTOP_INDEX` et `app::topbar_hit_scales`). Le test porte sur le
/// rectangle PRÉSENTÉ — celui qu'on voit — jamais sur la case au repos, comme les
/// boutons média : une icône grossie doit rester cliquable là où elle est
/// DESSINÉE, et pas seulement dans sa case d'origine.
pub fn show_desktop_hit(x: f32, y: f32, layout: &DockLayout, left_offset: f32, scale: f32) -> bool {
    hit(&presented_show_desktop_rect(show_desktop_rect(layout, left_offset), scale, layout), x, y)
}

/// Instance de quad de l'icône (glyphe rastérisé embarqué, voir
/// `gfx::texture::SHOW_DESKTOP_ICON_PNG`), teintée par `color` — `TexMode::
/// Glyph`, donc seul le canal alpha du PNG compte, la couleur vient d'ici.
///
/// **`hover` ne sert QU'À la rotation de balancement** (voir
/// `dock::hover_wobble_angle`) : c'est l'AVANCEMENT DU SURVOL, `0.0` au repos donc
/// aucune rotation — et `0.0` aussi sur un écran secondaire, qui ne reçoit jamais
/// la souris primaire et ne balance donc jamais.
///
/// **L'ÉCHELLE D'INSTANCE RESTE `1.0`, ET CE N'EST PAS UN OUBLI** : `rect` est le
/// rectangle PRÉSENTÉ, qui porte DÉJÀ la magnification — c'est la convention des
/// boutons média (voir le commentaire à leur appel) comme de `clock_instance`.
/// La passer aussi ici la double, et l'icône s'affiche alors PLUS PETITE que les
/// autres au lieu de pareil — DÉFAUT RÉEL SIGNALÉ : « l'applet afficher le bureau
/// ne s'affiche pas avec la même grandification que les autres applets ».
///
/// **Elle magnifie depuis le 2026-09-19** — DEMANDE EXPLICITE, après une première
/// décision inverse écrite ici même (« pas de magnification au survol, décision
/// explicite ») : voir `HOVER_SHOW_DESKTOP_INDEX`, qui raconte ce qui est advenu
/// de ses deux motifs.
pub fn show_desktop_instance(rect: [f32; 4], color: [f32; 3], hover: f32) -> QuadInstance {
    // ⚠️ **PAS `new_glyph_scaled` EN DUR : c'est une APPLET.** Un thème d'icônes
    // en COULEUR doit passer par `instance_icone`, sinon l'emoji est réduit à son
    // alpha et peint dans la teinte — « Afficher le bureau » s'affichait en CARRÉ
    // BLANC (l'alpha de l'emoji fenêtre EST un carré). Signalé par l'utilisateur.
    crate::ui::dock::Dock::instance_icone(rect, color, hover)
}

// ---------------------------------------------------------------------------
// LA CLOCHE DE RAPPEL — à DROITE de l'horloge
// ---------------------------------------------------------------------------

/// Marge entre le bord DROIT de l'horloge et la cloche — voir `bell_rect`.
pub const BELL_MARGIN_LEFT: f32 = 10.0;

/// Index de la CLOCHE dans le tableau d'échelles de survol (voir
/// `app::topbar_hit_scales`).
///
/// **AJOUTÉE EN FIN DE TABLEAU, APRÈS `HOVER_SHOW_DESKTOP_INDEX`** — c'est la
/// règle que la doc de ce dernier énonce, et elle vaut pour tout nouvel élément :
/// un indice inséré avant `HOVER_CLOCK_INDEX` (3) décalerait silencieusement
/// toutes les échelles.
pub const HOVER_BELL_INDEX: usize = 5;

/// Rectangle de la cloche — **à DROITE de l'horloge**, centrée verticalement, de
/// la taille d'une icône de barre (`top_icon_size_for`), comme « afficher le bureau ».
///
/// ⚠️ **LA GÉOMÉTRIE DE L'HORLOGE ARRIVE EN PARAMÈTRE, ET C'EST OBLIGATOIRE** :
/// `clock_rect` avec une largeur NULLE rend un rectangle au **CENTRE** de la barre
/// (sa formule est `(total - w) * 0.5`). Une cloche calculée sur une horloge de
/// largeur nulle s'afficherait donc **au milieu de l'écran**, et poserait une
/// boîte parasite dans l'union qui alimente la RÉGION du dock — à la toute
/// première frame, quand `app::run` n'a pas encore régénéré la texture du texte.
/// **Les appelants sont donc tous gardés par « la texture d'horloge existe »**,
/// et c'est la seule garde dont cette fonction a besoin.
///
/// ⚠️ **L'horloge ABSENTE n'est PAS un problème, contrairement à ce qu'on
/// pourrait croire** : la texture du texte est régénérée SANS condition de
/// fonctionnalité (voir `app::run`), donc `clock_size` est renseigné même quand
/// l'horloge est éteinte — la cloche garde alors sa place à droite d'une horloge
/// invisible. Éteindre l'horloge n'emporte pas la cloche.
pub fn bell_rect(layout: &DockLayout, clock_w: f32, clock_h: f32) -> [f32; 4] {
    let taille = top_icon_size_for(layout.top_thickness);
    let [cx, _, cw, _] = clock_rect(layout, clock_w, clock_h);
    [cx + cw + BELL_MARGIN_LEFT, (layout.top_thickness - taille) * 0.5, taille, taille]
}

/// Rectangle à DESSINER de la cloche : sa boîte grossie par `scale`, puis RAMENÉE
/// DANS LA FENÊTRE — `dock::presented_rect_for`, la même formule que
/// `presented_show_desktop_rect` et `presented_media_rect`.
///
/// Le rappel dans la fenêtre n'est pas décoratif ici : l'horloge est au CENTRE,
/// donc une cloche magnifiée près du bord droit d'une barre étroite pourrait en
/// sortir, et c'est cette fonction qui l'y ramène.
pub fn presented_bell_rect(rect: [f32; 4], scale: f32, layout: &DockLayout) -> [f32; 4] {
    crate::ui::dock::presented_rect_for(rect, scale, layout)
}

/// Test de clic — porte sur le rectangle PRÉSENTÉ, celui qu'on voit, jamais sur
/// la case au repos : une cloche grossie doit rester cliquable là où elle est
/// DESSINÉE (même règle que les boutons média et « afficher le bureau »).
pub fn bell_hit(x: f32, y: f32, layout: &DockLayout, clock_w: f32, clock_h: f32, scale: f32) -> bool {
    hit(&presented_bell_rect(bell_rect(layout, clock_w, clock_h), scale, layout), x, y)
}

/// Instance de quad de la cloche (glyphe rastérisé embarqué, voir
/// `gfx::texture::BELL_ICON_PNG`), teintée par `color` — `TexMode::Glyph`, donc
/// seul le canal alpha du PNG compte et **la couleur vient d'ici** : c'est ce qui
/// permet au clignotement de la faire changer de couleur sans changer d'icône.
///
/// **`hover` ne sert QU'À la rotation de balancement**, comme
/// `show_desktop_instance` : c'est l'AVANCEMENT DU SURVOL, `0.0` au repos donc
/// aucune rotation — et `0.0` sur un écran secondaire, qui ne balance jamais.
///
/// **L'ÉCHELLE D'INSTANCE RESTE `1.0`** : `rect` est le rectangle PRÉSENTÉ, qui
/// porte DÉJÀ la magnification. La passer aussi ici la doublerait.
pub fn bell_instance(rect: [f32; 4], color: [f32; 3], hover: f32) -> QuadInstance {
    // ⛔ **CELLE-CI NE SUIT PAS LE THÈME, ET C'EST DÉLIBÉRÉ** — voir sa doc
    // ci-dessus : **sa teinte EST sa fonction.** Le clignotement la fait changer
    // de couleur pour signaler un rappel dû, et c'est ce que `TexMode::Glyph`
    // permet (la couleur vient de l'appelant). `Dock::instance_icone` la figerait
    // sur l'emoji, et **l'alerte disparaîtrait sans un mot**.
    //
    // ⚠️ CONSÉQUENCE ASSUMÉE sous le thème « Symboles » : une cloche BLANCHE
    // alors que ses voisines sont colorées. C'est le SEUL widget dans ce cas, et
    // l'échange — une alerte contre une couleur — se fait dans ce sens-là.
    let mut instance = QuadInstance::new_glyph_scaled(rect, color, 1.0);
    instance.rotation = crate::ui::dock::hover_wobble_angle(hover);
    instance
}

// ---------------------------------------------------------------------------
// L'ICÔNE DU MINUTEUR — à DROITE de la cloche
// ---------------------------------------------------------------------------
//
// DEMANDE EXPLICITE (2026-09-26) : « quand une alarme se termine, l'icône de
// l'applet s'affiche à côté des horloges, à côté de l'emplacement pour la
// cloche ». Même taille, même centrage, même magnification que la cloche ; elle
// n'est DESSINÉE que tant que quelque chose sonne (voir `win32::minuteur::sonne`).
//
// ⚠️ **ELLE S'ANCRE SUR L'EMPLACEMENT DE LA CLOCHE, PAS SUR LA CLOCHE DESSINÉE** :
// la cloche n'apparaît que les jours de rappel, et une icône qui glisserait d'une
// case selon qu'elle est là ou non changerait de place sous le curseur.

/// Marge entre la cloche et l'icône du minuteur — la même que celle entre
/// l'horloge et la cloche.
pub const MINUTEUR_MARGIN_LEFT: f32 = BELL_MARGIN_LEFT;

/// Index de l'ICÔNE DU MINUTEUR dans le tableau d'échelles de survol (voir
/// `app::topbar_hit_scales`) — **EN FIN DE TABLEAU, APRÈS `HOVER_NOTE_INDEX`**,
/// même règle.
pub const HOVER_MINUTEUR_INDEX: usize = 7;

/// Rectangle de l'icône du minuteur — **à DROITE de l'emplacement de la
/// cloche**. ⚠️ Même piège de largeur nulle que `bell_rect`, dont elle dérive :
/// les appelants sont gardés par « la texture d'horloge existe ».
pub fn minuteur_rect(layout: &DockLayout, clock_w: f32, clock_h: f32) -> [f32; 4] {
    let [bx, by, bw, bh] = bell_rect(layout, clock_w, clock_h);
    [bx + bw + MINUTEUR_MARGIN_LEFT, by, bw, bh]
}

/// Rectangle à DESSINER — même formule que la cloche.
pub fn presented_minuteur_rect(rect: [f32; 4], scale: f32, layout: &DockLayout) -> [f32; 4] {
    crate::ui::dock::presented_rect_for(rect, scale, layout)
}

/// Test de clic — sur le rectangle PRÉSENTÉ, comme la cloche.
pub fn minuteur_hit(x: f32, y: f32, layout: &DockLayout, clock_w: f32, clock_h: f32, scale: f32) -> bool {
    hit(&presented_minuteur_rect(minuteur_rect(layout, clock_w, clock_h), scale, layout), x, y)
}

/// Instance de quad (le MÊME PNG que l'applet de la barre, voir
/// `gfx::texture::MINUTEUR_ICON_PNG`) — même convention que `bell_instance` :
/// la couleur vient d'ici, l'échelle d'instance reste `1.0`.
pub fn minuteur_instance(rect: [f32; 4], color: [f32; 3], hover: f32) -> QuadInstance {
    // Même règle que `show_desktop_instance` : c'est une APPLET, donc la source
    // de l'icône suit le thème. `hover` est l'avancement du survol, pas une
    // échelle — voir `Dock::instance_icone`.
    crate::ui::dock::Dock::instance_icone(rect, color, hover)
}

// ---------------------------------------------------------------------------
// L'APPLET « MASQUER L'INTERFACE » — à DROITE de l'emplacement du minuteur
// ---------------------------------------------------------------------------
//
// DEMANDE EXPLICITE (2026-10-02) : « une applet dans la barre du haut pour
// effectuer un masquage de l'interface ; quand l'interface est masquée, cette
// applet reste visible dans un tout petit onglet, toujours au même emplacement
// sur l'écran ». Même taille, même centrage, même magnification que la cloche.
//
// ⚠️ **ELLE S'ANCRE SUR L'EMPLACEMENT DU MINUTEUR, PAS SUR LE MINUTEUR DESSINÉ**
// — même règle que lui envers la cloche : le minuteur n'apparaît que quand une
// alarme sonne, et l'applet ne doit pas changer de place selon qu'il est là.
// C'est aussi ce qui permet à l'onglet (voir `win32::onglet_masque`) de reprendre
// EXACTEMENT sa place.

/// Index de l'APPLET DE MASQUAGE dans le tableau d'échelles de survol (voir
/// `app::topbar_hit_scales`) — **EN FIN DE TABLEAU, APRÈS `HOVER_MINUTEUR_INDEX`**.
pub const HOVER_MASQUE_INDEX: usize = 8;

/// Rectangle de l'applet — **à DROITE de l'emplacement du minuteur**. Même piège
/// de largeur nulle que `bell_rect` : les appelants sont gardés par « la texture
/// d'horloge existe ».
pub fn masque_rect(layout: &DockLayout, clock_w: f32, clock_h: f32) -> [f32; 4] {
    let [mx, my, mw, mh] = minuteur_rect(layout, clock_w, clock_h);
    [mx + mw + MINUTEUR_MARGIN_LEFT, my, mw, mh]
}

/// Rectangle à DESSINER — même formule que la cloche.
pub fn presented_masque_rect(rect: [f32; 4], scale: f32, layout: &DockLayout) -> [f32; 4] {
    crate::ui::dock::presented_rect_for(rect, scale, layout)
}

// ---------------------------------------------------------------------------
// L'ICÔNE RÉGLAGES DE SECOURS — à DROITE de l'applet de masquage
// ---------------------------------------------------------------------------
//
// DEMANDE EXPLICITE (2026-10-03) : « j'ai masqué la barre de droite et n'ai donc
// plus accès aux réglages ; fais en sorte qu'une icône réglage apparaisse dans
// la barre du haut quand la barre de droite est masquée ». Barre PRIMAIRE
// seulement — c'est la sienne qui porte l'icône Réglages —, et seulement tant
// que la barre de droite est masquée.

/// Index de l'ICÔNE RÉGLAGES DE SECOURS dans le tableau d'échelles de survol —
/// **EN FIN DE TABLEAU, APRÈS `HOVER_MASQUE_INDEX`**.
pub const HOVER_REGLAGES_INDEX: usize = 9;

/// Rectangle de l'icône — à DROITE de l'emplacement de l'applet de masquage,
/// même taille, même hauteur.
pub fn reglages_rect(layout: &DockLayout, clock_w: f32, clock_h: f32) -> [f32; 4] {
    let [mx, my, mw, mh] = masque_rect(layout, clock_w, clock_h);
    [mx + mw + MINUTEUR_MARGIN_LEFT, my, mw, mh]
}

/// Rectangle à DESSINER — même formule que la cloche.
pub fn presented_reglages_rect(rect: [f32; 4], scale: f32, layout: &DockLayout) -> [f32; 4] {
    crate::ui::dock::presented_rect_for(rect, scale, layout)
}

/// Test de clic — sur le rectangle PRÉSENTÉ, comme la cloche.
pub fn reglages_hit(x: f32, y: f32, layout: &DockLayout, clock_w: f32, clock_h: f32, scale: f32) -> bool {
    hit(&presented_reglages_rect(reglages_rect(layout, clock_w, clock_h), scale, layout), x, y)
}

/// Test de clic — sur le rectangle PRÉSENTÉ, comme la cloche.
pub fn masque_hit(x: f32, y: f32, layout: &DockLayout, clock_w: f32, clock_h: f32, scale: f32) -> bool {
    hit(&presented_masque_rect(masque_rect(layout, clock_w, clock_h), scale, layout), x, y)
}

// ---------------------------------------------------------------------------
// LE VOYANT D'ENREGISTREMENT — à DROITE de l'emplacement Réglages de secours
// ---------------------------------------------------------------------------
//
// DEMANDE (2026-10-31, « oui » au voyant REC) : tant que l'applet Enregistrer
// tourne, un point rouge clignote dans la barre du haut, et un clic dessus
// ARRÊTE l'enregistrement. Il n'existe QUE pendant l'enregistrement (voir
// `win32::enregistreur::en_cours`), comme l'icône du minuteur pendant sa
// sonnerie.
//
// ⚠️ **IL S'ANCRE SUR L'EMPLACEMENT RÉGLAGES, PAS SUR L'ICÔNE DESSINÉE** — même
// règle que toute la rangée : Réglages de secours n'est là que si la barre de
// droite est masquée, et le voyant ne doit pas changer de place selon qu'elle
// l'est. Et EN DERNIER : il ne décale aucune case existante.
//
// **UN DISQUE PLEIN, PAS UNE TEXTURE** : le point rouge EST le symbole
// d'enregistrement, et un quad uni n'a besoin d'aucun PNG ni d'aucun thème.

/// Index du VOYANT D'ENREGISTREMENT dans le tableau d'échelles de survol —
/// **EN FIN DE TABLEAU, APRÈS `HOVER_REGLAGES_INDEX`**.
pub const HOVER_REC_INDEX: usize = 10;

/// Diamètre du disque, en fraction du côté de la case : une icône pleine de la
/// taille de la case écraserait ses voisines, qui ont leurs marges internes.
pub const REC_DOT_RATIO: f32 = 0.5;

/// Demi-période du clignotement : allumé, puis atténué, autant de temps chacun.
pub const REC_BLINK_HALF_PERIOD_MS: u128 = 600;

/// Rouge du voyant, et son alpha atténué (le disque ne DISPARAÎT jamais : une
/// case vide une frame sur deux serait un clic dans le vide une fois sur deux).
pub const REC_RED: [f32; 3] = [0.92, 0.16, 0.16];
pub const REC_DIM_ALPHA: f32 = 0.35;

/// Rectangle de la CASE du voyant — à DROITE de l'emplacement Réglages. Même
/// piège de largeur nulle que `bell_rect` : les appelants sont gardés par « la
/// texture d'horloge existe ».
pub fn rec_rect(layout: &DockLayout, clock_w: f32, clock_h: f32) -> [f32; 4] {
    let [rx, ry, rw, rh] = reglages_rect(layout, clock_w, clock_h);
    [rx + rw + MINUTEUR_MARGIN_LEFT, ry, rw, rh]
}

/// Rectangle à DESSINER (la case grossie) — même formule que la cloche.
pub fn presented_rec_rect(rect: [f32; 4], scale: f32, layout: &DockLayout) -> [f32; 4] {
    crate::ui::dock::presented_rect_for(rect, scale, layout)
}

/// Test de clic — sur la CASE présentée, pas sur le seul disque : viser un
/// point de quelques pixels serait une corvée.
pub fn rec_hit(x: f32, y: f32, layout: &DockLayout, clock_w: f32, clock_h: f32, scale: f32) -> bool {
    hit(&presented_rec_rect(rec_rect(layout, clock_w, clock_h), scale, layout), x, y)
}

/// Couleur du voyant après `elapsed` : rouge plein, puis rouge atténué. Un
/// enregistrement qui se FERME (`clignote == false`) reste fixe et atténué —
/// le clic n'y fait plus rien, le voyant ne doit plus inviter à cliquer.
pub fn rec_color_at(elapsed: std::time::Duration, clignote: bool) -> [f32; 4] {
    let allume = clignote && (elapsed.as_millis() / REC_BLINK_HALF_PERIOD_MS) % 2 == 0;
    let alpha = if allume { 1.0 } else { REC_DIM_ALPHA };
    [REC_RED[0], REC_RED[1], REC_RED[2], alpha]
}

/// Couleur du voyant MAINTENANT — voir `rec_color_at`.
pub fn rec_color(clignote: bool) -> [f32; 4] {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    rec_color_at(START.get_or_init(std::time::Instant::now).elapsed(), clignote)
}

/// Diamètre de l'anneau, en multiple de celui du disque, et son épaisseur en
/// fraction du disque.
pub const REC_RING_RATIO: f32 = 1.5;
pub const REC_RING_THICKNESS: f32 = 0.14;

/// L'anneau PUIS le disque, CENTRÉS dans la case présentée.
///
/// ⛔ **UN DISQUE ROUGE SEUL DISPARAÎT SUR UNE BARRE ROUGE** — mesuré au premier
/// essai : le dégradé de l'utilisateur est rouge au centre, et le voyant ne se
/// devinait plus. L'anneau prend la couleur des TEXTES de la barre (`ring_rgb`),
/// celle que le thème choisit justement pour rester lisible sur ce fond ; il ne
/// clignote pas, lui : c'est lui qui garantit qu'on VOIT la case.
pub fn rec_instances(rect: [f32; 4], color: [f32; 4], ring_rgb: [f32; 3]) -> [QuadInstance; 2] {
    let d = rect[2].min(rect[3]) * REC_DOT_RATIO;
    let (cx, cy) = (rect[0] + rect[2] * 0.5, rect[1] + rect[3] * 0.5);
    let r = d * REC_RING_RATIO;
    let anneau = QuadInstance::new_ring(
        [cx - r * 0.5, cy - r * 0.5, r, r],
        [ring_rgb[0], ring_rgb[1], ring_rgb[2], 1.0],
        r * 0.5,
        d * REC_RING_THICKNESS,
    );
    let disque = QuadInstance::new([cx - d * 0.5, cy - d * 0.5, d, d], color, d * 0.5, 1.0);
    [anneau, disque]
}

// ---------------------------------------------------------------------------
// L'ICÔNE DE NOTE — à GAUCHE de l'horloge
// ---------------------------------------------------------------------------
//
// DEMANDE EXPLICITE (2026-09-24) : « je voudrais le même mécanisme avec une icône
// de note à gauche de l'horloge ». Miroir strict de la cloche ci-dessus — même
// taille, même centrage, même magnification, même clignotement — à trois
// différences près, toutes voulues :
//
//   * elle est à GAUCHE, la cloche est à droite ;
//   * elle signale les rappels de NOTES, et la cloche ne les couvre plus : une
//     fête vaut pour un JOUR, une alarme de note pour un INSTANT, et les mêler
//     faisait acquitter la seconde en même temps que la première ;
//   * son clic **OUVRE le panneau Notes ET ACQUITTE** ses rappels — une note ne
//     se lit qu'en l'ouvrant, et un rappel qui sonne doit se taire au geste le
//     plus évident. ⚠️ Le projet avait d'abord dit « ouvrir seulement » ; c'est
//     l'USAGE qui a tranché l'inverse, le 2026-09-24 (voir `app::run`, au clic).
//     Le bouton « Acquitter » du panneau reste, lui, PAR NOTE.

/// Marge entre le bord GAUCHE de l'horloge et l'icône de note — voir `note_rect`.
pub const NOTE_MARGIN_RIGHT: f32 = 10.0;

/// Index de l'ICÔNE DE NOTE dans le tableau d'échelles de survol (voir
/// `app::topbar_hit_scales`).
///
/// **AJOUTÉE EN FIN DE TABLEAU, APRÈS `HOVER_BELL_INDEX`** — c'est la règle que la
/// doc de ce dernier énonce, et elle vaut pour tout nouvel élément : un indice
/// inséré avant `HOVER_CLOCK_INDEX` (3) décalerait silencieusement toutes les
/// échelles.
pub const HOVER_NOTE_INDEX: usize = 6;

/// Rectangle de l'icône de note — **à GAUCHE de l'horloge**, centrée
/// verticalement, de la taille d'une icône de barre (`top_icon_size_for`).
///
/// ⚠️ **MÊME PIÈGE QUE LA CLOCHE, ET C'EST EN MIROIR** : `clock_rect` avec une
/// largeur NULLE rend un rectangle au **CENTRE** de la barre (sa formule est
/// `(total - w) * 0.5`). Une icône calculée sur une horloge de largeur nulle se
/// poserait donc au milieu, à une marge de la cloche — **les deux icônes
/// superposées**, à la toute première frame, quand la texture du texte de
/// l'horloge n'existe pas encore. Les appelants sont donc gardés par « la texture
/// d'horloge existe », exactement comme pour la cloche.
pub fn note_rect(layout: &DockLayout, clock_w: f32, clock_h: f32) -> [f32; 4] {
    let taille = top_icon_size_for(layout.top_thickness);
    let [cx, _, _, _] = clock_rect(layout, clock_w, clock_h);
    [cx - NOTE_MARGIN_RIGHT - taille, (layout.top_thickness - taille) * 0.5, taille, taille]
}

/// Rectangle à DESSINER de l'icône : sa boîte grossie par `scale`, puis RAMENÉE
/// DANS LA FENÊTRE — `dock::presented_rect_for`, la même formule que la cloche.
pub fn presented_note_rect(rect: [f32; 4], scale: f32, layout: &DockLayout) -> [f32; 4] {
    crate::ui::dock::presented_rect_for(rect, scale, layout)
}

/// Test de clic — porte sur le rectangle PRÉSENTÉ, celui qu'on voit : une icône
/// grossie doit rester cliquable là où elle est DESSINÉE.
pub fn note_hit(x: f32, y: f32, layout: &DockLayout, clock_w: f32, clock_h: f32, scale: f32) -> bool {
    hit(&presented_note_rect(note_rect(layout, clock_w, clock_h), scale, layout), x, y)
}

/// Instance de quad de l'icône de note (glyphe rastérisé embarqué, voir
/// `gfx::texture::NOTES_ICON_PNG` — le MÊME PNG que l'applet Notes de la barre),
/// teintée par `color` : `TexMode::Glyph`, donc seul le canal alpha du PNG
/// compte, et c'est ce qui permet au clignotement de la faire changer de couleur
/// sans changer d'icône.
///
/// **L'ÉCHELLE D'INSTANCE RESTE `1.0`** : `rect` est le rectangle PRÉSENTÉ, qui
/// porte DÉJÀ la magnification. La passer aussi ici la doublerait.
pub fn note_instance(rect: [f32; 4], color: [f32; 3], hover: f32) -> QuadInstance {
    // Même règle que `show_desktop_instance` : c'est une APPLET, donc la source
    // de l'icône suit le thème (voir `Dock::instance_icone`). `hover` est
    // l'avancement du survol, pas une échelle.
    crate::ui::dock::Dock::instance_icone(rect, color, hover)
}

/// Contrôles multimédias (voir `win32::media`) : précédent/lecture-pause/
/// suivant + titre en cours, alignés à gauche de la barre du haut — façon
/// module "Media" de Seelen UI. Ancrés à une marge FIXE depuis le coin de
/// l'ÉCRAN (PAS `layout.left_width` en plus, voir `ui::dock::
/// TOP_ZONE_CORNER_MARGIN`) plutôt qu'après les pastilles de bureaux
/// virtuels (dont le nombre varie) : marge assez large pour ne pas les
/// chevaucher dans le cas courant (quelques bureaux), au prix d'un
/// chevauchement possible dans le cas extrême de très nombreux bureaux —
/// compromis simple plutôt qu'une détection dynamique de collision.
const MEDIA_GROUP_MARGIN_LEFT: f32 = 220.0;
/// Écart entre les trois boutons (précédent/lecture-pause/suivant).
const MEDIA_BUTTON_GAP: f32 = 4.0;
/// Écart entre le bouton "suivant" et le titre en cours.
const MEDIA_TITLE_MARGIN_LEFT: f32 = 12.0;
pub const MEDIA_TITLE_FONT_NAME: &str = "Segoe UI";
pub const MEDIA_GLYPH_FONT_NAME: &str = "Segoe MDL2 Assets";
pub const MEDIA_GLYPH_FONT_PX: i32 = 32;
pub const MEDIA_PREVIOUS_GLYPH: &str = "\u{E892}";
pub const MEDIA_NEXT_GLYPH: &str = "\u{E893}";
pub const MEDIA_PLAY_GLYPH: &str = "\u{E768}";
pub const MEDIA_PAUSE_GLYPH: &str = "\u{E769}";

// `left_offset` : voir `desktop_dot_rect` — mêmes icônes déposables de
// dock à décaler.
/// Bord GAUCHE du bloc des contrôles média, en coordonnées fenêtre —
/// **SEULE définition de cette position**, et c'est ce qui a manqué : l'ancrage
/// absolu (`MEDIA_GROUP_MARGIN_LEFT`) était ajouté À L'INTÉRIEUR de
/// `media_button_rect`, donc un appelant qui recevait `left_offset` croyait tenir
/// le bord du bloc — il tenait celui du groupe d'icônes. DÉFAUT RÉEL SIGNALÉ :
/// « je n'y arrive toujours pas », et le journal de diagnostic a montré une cible
/// 220 px trop à gauche, c'est-à-dire au milieu des pastilles.
pub fn media_group_left(left_offset: f32) -> f32 {
    MEDIA_GROUP_MARGIN_LEFT + left_offset
}

fn media_button_rect(layout: &DockLayout, index: usize, left_offset: f32) -> [f32; 4] {
    let icon_size = top_icon_size_for(layout.top_thickness);
    let x = media_group_left(left_offset) + index as f32 * (icon_size + MEDIA_BUTTON_GAP);
    let y = (layout.top_thickness - icon_size) * 0.5;
    [x, y, icon_size, icon_size]
}

/// Rectangle à DESSINER d'un bouton média : sa boîte grossie par `scale`, puis
/// ramenée dans la fenêtre — voir `ui::dock::presented_rect_for`, qui porte la
/// formule.
///
/// Passer par `dock` plutôt que de recopier le calcul est délibéré : ces trois
/// boutons sont les seuls éléments magnifiés de la barre du haut qui ne vivent
/// pas DANS le dock (voir `topbar_hover`, `app::run`), et ils doivent pourtant
/// se décaler **exactement** comme les icônes de cette barre-là. Une seconde
/// formule finirait par diverger.
pub fn presented_media_rect(rect: [f32; 4], scale: f32, layout: &DockLayout) -> [f32; 4] {
    crate::ui::dock::presented_rect_for(rect, scale, layout)
}

pub fn media_previous_rect(layout: &DockLayout, left_offset: f32) -> [f32; 4] {
    media_button_rect(layout, 0, left_offset)
}
pub fn media_play_pause_rect(layout: &DockLayout, left_offset: f32) -> [f32; 4] {
    media_button_rect(layout, 1, left_offset)
}
pub fn media_next_rect(layout: &DockLayout, left_offset: f32) -> [f32; 4] {
    media_button_rect(layout, 2, left_offset)
}

/// `scale` : l'échelle de survol courante du bouton (voir `topbar_hover` dans
/// `app::run`). Le test porte sur le rectangle PRÉSENTÉ — celui qu'on voit —
/// jamais sur la case au repos : un bouton grossi et décalé vers le bas doit
/// rester cliquable là où il est dessiné, et pas seulement dans sa case
/// d'origine.
pub fn media_previous_hit(x: f32, y: f32, layout: &DockLayout, left_offset: f32, scale: f32) -> bool {
    hit(&presented_media_rect(media_previous_rect(layout, left_offset), scale, layout), x, y)
}
pub fn media_play_pause_hit(x: f32, y: f32, layout: &DockLayout, left_offset: f32, scale: f32) -> bool {
    hit(&presented_media_rect(media_play_pause_rect(layout, left_offset), scale, layout), x, y)
}
pub fn media_next_hit(x: f32, y: f32, layout: &DockLayout, left_offset: f32, scale: f32) -> bool {
    hit(&presented_media_rect(media_next_rect(layout, left_offset), scale, layout), x, y)
}

/// Instance de quad pour un bouton média (glyphe déjà rendu par
/// `gfx::text` — précédent/suivant sont statiques, lecture-pause change de
/// texture selon l'état de lecture, voir `app::run`), teinté par `color`,
/// magnifié par `scale` (voir `power_button_instance`).
pub fn media_button_instance(rect: [f32; 4], color: [f32; 3], scale: f32) -> QuadInstance {
    QuadInstance::new_glyph_scaled(rect, color, scale)
}

/// Hauteur de police du titre en cours — même logique que
/// `sys_monitor_font_px` (un cran sous l'horloge).
pub fn media_title_font_px(layout: &DockLayout, custom: Option<f32>) -> i32 {
    topbar_secondary_text_font_px(layout, custom)
}

/// Rectangle du titre en cours (texte déjà rendu, `text_w`×`text_h`),
/// juste à droite du bouton "suivant", centré verticalement.
pub fn media_title_rect(layout: &DockLayout, text_w: f32, text_h: f32, left_offset: f32) -> [f32; 4] {
    let next_rect = media_next_rect(layout, left_offset);
    let x = next_rect[0] + next_rect[2] + MEDIA_TITLE_MARGIN_LEFT;
    let y = (layout.top_thickness - text_h) * 0.5;
    [x, y, text_w, text_h]
}

/// Instance de quad pour le titre en cours, teinté par `color`.
pub fn media_title_instance(layout: &DockLayout, text_w: f32, text_h: f32, color: [f32; 3], left_offset: f32) -> QuadInstance {
    QuadInstance::new_glyph(media_title_rect(layout, text_w, text_h, left_offset), color)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Disposition de référence — mêmes valeurs que `ui::dock::geometry_layout` :
    /// 3840×2088, barre du haut de 72 px, barres latérales de 140.
    fn test_layout() -> DockLayout {
        DockLayout { total_width: 3840.0, total_height: 2088.0, top_thickness: 72.0, haut_colonnes: 72.0, left_width: 140.0, right_width: 140.0, bottom_thickness: 0.0 }
    }

    /// **Au repos, la géométrie de l'horloge ne bouge pas d'un pixel** — c'est ce
    /// qui garantit que l'effet de survol n'a rien changé tant que la souris n'est
    /// pas dessus.
    #[test]
    fn presented_clock_rect_at_rest_is_the_clock_rect() {
        let layout = test_layout();
        for (w, h) in [(120.0, 28.0), (80.0, 20.0), (240.0, 40.0)] {
            assert_eq!(presented_clock_rect(&layout, w, h, 1.0), clock_rect(&layout, w, h), "{w}×{h}");
        }
    }

    /// **LA CLOCHE EST À DROITE DE L'HORLOGE, ET À LA MÊME HAUTEUR** — c'est la
    /// demande (« une icône de cloche [...] placée à droite de l'horloge »), et
    /// c'est la seule chose que `bell_rect` promet.
    #[test]
    fn la_cloche_est_a_droite_de_l_horloge_et_centree_verticalement() {
        let layout = test_layout();
        for (w, h) in [(120.0, 28.0), (80.0, 20.0), (240.0, 40.0)] {
            let horloge = clock_rect(&layout, w, h);
            let cloche = bell_rect(&layout, w, h);
            assert_eq!(
                cloche[0],
                horloge[0] + horloge[2] + BELL_MARGIN_LEFT,
                "{w}×{h} : la cloche commence après la fin de l'horloge, à la marge près"
            );
            // Centrée verticalement dans la barre, comme « afficher le bureau ».
            assert_eq!(cloche[1] + cloche[3] * 0.5, layout.top_thickness * 0.5, "{w}×{h} : centrée");
            // Et elle ne recouvre JAMAIS l'horloge, quelle que soit la largeur du
            // texte de celle-ci.
            assert!(cloche[0] >= horloge[0] + horloge[2], "{w}×{h} : aucun chevauchement");
        }
    }

    /// ⚠️ **CE TEST FIGE UN PIÈGE, IL NE LE RECOMMANDE PAS.** Une horloge de
    /// largeur NULLE n'a pas d'ancre : `clock_rect` rend alors un rectangle au
    /// **CENTRE** de la barre (sa formule est `(total - w) * 0.5`), donc une cloche
    /// calculée dessus s'afficherait au MILIEU DE L'ÉCRAN — et poserait une boîte
    /// parasite dans l'union qui alimente la RÉGION du dock.
    ///
    /// C'est pourquoi **tous** les appelants sont gardés par « la texture
    /// d'horloge existe » : `app::run` (dessin, union `boxes`, test de clic,
    /// infobulle) et `dock_instance::DockInstance::render`. Ce test existe pour que
    /// ce garde ne soit jamais retiré par mégarde — il rend le comportement
    /// indésirable VISIBLE, au lieu de le laisser enfoui dans un commentaire.
    #[test]
    fn une_horloge_de_largeur_nulle_mettrait_la_cloche_au_centre() {
        let layout = test_layout();
        let cloche = bell_rect(&layout, 0.0, 0.0);
        let centre = layout.total_width * 0.5;
        assert!(
            (cloche[0] - (centre + BELL_MARGIN_LEFT)).abs() < 0.5,
            "largeur nulle ⇒ cloche au centre ({}) : les appelants DOIVENT être gardés",
            cloche[0]
        );
    }

    /// **L'APPLET DE MASQUAGE SE POSE À DROITE DE L'EMPLACEMENT DU MINUTEUR**,
    /// même hauteur, même taille — et son indice d'échelle est le DERNIER.
    #[test]
    fn l_applet_de_masquage_est_a_droite_du_minuteur() {
        let layout = test_layout();
        for (w, h) in [(120.0, 28.0), (80.0, 20.0), (240.0, 40.0)] {
            let minuteur = minuteur_rect(&layout, w, h);
            let masque = masque_rect(&layout, w, h);
            assert_eq!(masque[0], minuteur[0] + minuteur[2] + MINUTEUR_MARGIN_LEFT, "{w}×{h}");
            assert_eq!(masque[1], minuteur[1], "{w}×{h} : même hauteur");
            assert_eq!(masque[2], minuteur[2], "{w}×{h} : même taille");
        }
        assert_eq!(HOVER_MASQUE_INDEX, HOVER_MINUTEUR_INDEX + 1, "ajoutée EN FIN de tableau");
    }

    /// L'icône Réglages de secours se pose à droite de l'applet de masquage.
    #[test]
    fn l_icone_reglages_de_secours_est_a_droite_du_masquage() {
        let layout = test_layout();
        let (masque, reglages) = (masque_rect(&layout, 120.0, 28.0), reglages_rect(&layout, 120.0, 28.0));
        assert_eq!(reglages[0], masque[0] + masque[2] + MINUTEUR_MARGIN_LEFT);
        assert_eq!((reglages[1], reglages[2]), (masque[1], masque[2]));
        assert_eq!(HOVER_REGLAGES_INDEX, HOVER_MASQUE_INDEX + 1, "ajoutée EN FIN de tableau");
    }

    /// **LE VOYANT REC SE POSE À DROITE DE L'EMPLACEMENT RÉGLAGES**, même
    /// hauteur, même taille — et son indice d'échelle est le DERNIER.
    #[test]
    fn le_voyant_rec_est_a_droite_de_reglages_et_en_fin_de_tableau() {
        let layout = test_layout();
        for (w, h) in [(120.0, 28.0), (80.0, 20.0), (240.0, 40.0)] {
            let reglages = reglages_rect(&layout, w, h);
            let rec = rec_rect(&layout, w, h);
            assert_eq!(rec[0], reglages[0] + reglages[2] + MINUTEUR_MARGIN_LEFT, "{w}×{h}");
            assert_eq!((rec[1], rec[2], rec[3]), (reglages[1], reglages[2], reglages[3]), "{w}×{h}");
        }
        assert_eq!(HOVER_REC_INDEX, HOVER_REGLAGES_INDEX + 1, "ajouté EN FIN de tableau");
    }

    /// Le disque est centré dans sa case, rond, et plus petit qu'elle.
    #[test]
    fn le_disque_rec_est_centre_et_rond() {
        let case = [100.0, 10.0, 40.0, 30.0];
        let [anneau, disque] = rec_instances(case, [1.0, 0.0, 0.0, 0.35], [0.2, 0.3, 0.4]);
        let [x, y, w, h] = disque.rect;
        assert_eq!(w, h, "un disque");
        assert_eq!(w, 30.0 * REC_DOT_RATIO, "le plus petit côté fait foi");
        assert_eq!(x + w * 0.5, 120.0, "centré en X");
        assert_eq!(y + h * 0.5, 25.0, "centré en Y");
        let [ax, ay, aw, ah] = anneau.rect;
        assert_eq!((ax + aw * 0.5, ay + ah * 0.5), (120.0, 25.0), "anneau centré");
        assert!(aw > w, "l'anneau entoure le disque");
        assert!(anneau.ring > 0.0, "un anneau CREUX, pas un aplat");
        assert_eq!(anneau.color, [0.2, 0.3, 0.4, 1.0], "couleur des textes, OPAQUE même quand le disque s'atténue");
    }

    /// Le voyant clignote pendant l'enregistrement, reste fixe et atténué
    /// pendant la fermeture — et n'est jamais TRANSPARENT.
    #[test]
    fn le_voyant_rec_clignote_puis_se_fige() {
        use std::time::Duration;
        let demi = Duration::from_millis(REC_BLINK_HALF_PERIOD_MS as u64);
        assert_eq!(rec_color_at(Duration::ZERO, true)[3], 1.0);
        assert_eq!(rec_color_at(demi, true)[3], REC_DIM_ALPHA);
        assert_eq!(rec_color_at(demi * 2, true)[3], 1.0);
        for t in [Duration::ZERO, demi, demi * 2] {
            assert_eq!(rec_color_at(t, false)[3], REC_DIM_ALPHA, "fermeture : fixe");
        }
        assert!(REC_DIM_ALPHA > 0.0);
    }

    /// **LE MINUTEUR SE POSE À DROITE DE L'EMPLACEMENT DE LA CLOCHE**, à la même
    /// hauteur et de la même taille, sans jamais la toucher.
    #[test]
    fn le_minuteur_est_a_droite_de_la_cloche_et_centre_verticalement() {
        let layout = test_layout();
        for (w, h) in [(120.0, 28.0), (80.0, 20.0), (240.0, 40.0)] {
            let cloche = bell_rect(&layout, w, h);
            let minuteur = minuteur_rect(&layout, w, h);
            assert_eq!(minuteur[0], cloche[0] + cloche[2] + MINUTEUR_MARGIN_LEFT, "{w}×{h}");
            assert_eq!(minuteur[1] + minuteur[3] * 0.5, layout.top_thickness * 0.5, "{w}×{h} : centré");
            assert_eq!(minuteur[2], cloche[2], "{w}×{h} : même taille que la cloche");
        }
    }

    /// **L'ICÔNE DE NOTE EST LE MIROIR DE LA CLOCHE** — même marge, même taille,
    /// même centrage, et de l'AUTRE côté de l'horloge. Trois propriétés, celles du
    /// test de la cloche, inversées en X.
    #[test]
    fn l_icone_de_note_est_a_gauche_de_l_horloge_et_centree_verticalement() {
        let layout = test_layout();
        for (w, h) in [(120.0, 28.0), (80.0, 20.0), (240.0, 40.0)] {
            let horloge = clock_rect(&layout, w, h);
            let icone = note_rect(&layout, w, h);
            assert_eq!(
                icone[0] + icone[2],
                horloge[0] - NOTE_MARGIN_RIGHT,
                "{w}×{h} : l'icône finit avant le début de l'horloge, à la marge près"
            );
            assert_eq!(icone[1] + icone[3] * 0.5, layout.top_thickness * 0.5, "{w}×{h} : centrée");
            assert!(icone[0] + icone[2] <= horloge[0], "{w}×{h} : aucun chevauchement");
            // Et les deux icônes ne se rencontrent jamais, quelle que soit la
            // largeur du texte de l'horloge : elles sont séparées par la largeur
            // de l'horloge PLUS les deux marges.
            let cloche = bell_rect(&layout, w, h);
            assert!(icone[0] + icone[2] <= cloche[0], "{w}×{h} : l'icône de note ne touche pas la cloche");
        }
    }

    /// ⚠️ **LE MÊME PIÈGE QUE LA CLOCHE, ET C'EST POURQUOI IL FAUT LE REDIRE** :
    /// une horloge de largeur NULLE n'a pas d'ancre — `clock_rect` rend un
    /// rectangle au CENTRE de la barre. L'icône de note se poserait alors à une
    /// marge DE LA CLOCHE, elle aussi au centre : **les deux icônes superposées**,
    /// et deux boîtes parasites dans l'union qui alimente la RÉGION du dock.
    ///
    /// C'est pourquoi les appelants sont gardés par « la texture d'horloge
    /// existe », exactement comme pour la cloche.
    #[test]
    fn une_horloge_de_largeur_nulle_mettrait_l_icone_de_note_au_centre() {
        let layout = test_layout();
        let icone = note_rect(&layout, 0.0, 0.0);
        let cloche = bell_rect(&layout, 0.0, 0.0);
        let centre = layout.total_width * 0.5;
        assert!(
            (icone[0] + icone[2] - (centre - NOTE_MARGIN_RIGHT)).abs() < 0.5,
            "largeur nulle ⇒ l'icône vient buter sur le centre ({}) : les appelants DOIVENT être gardés",
            icone[0]
        );
        assert!(
            icone[0] + icone[2] > centre - 2.0 * NOTE_MARGIN_RIGHT,
            "…et elle est alors TOUT PRÈS de la cloche, qui est à {}",
            cloche[0]
        );
    }

    /// **Le clic suit ce qui est DESSINÉ, pas la case au repos** — pendant exact de
    /// `clock_hit_follows_the_magnified_text`, et c'est ce qui discrimine un `_hit`
    /// qui aurait oublié de lire l'échelle.
    #[test]
    fn le_clic_de_l_icone_de_note_suit_le_rectangle_magnifie() {
        let layout = test_layout();
        let (w, h) = (120.0, 28.0);
        let rest = note_rect(&layout, w, h);
        let magnified = presented_note_rect(rest, 2.5, &layout);

        // ⚠️ **ON N'ASSERTE RIEN SUR L'AXE VERTICAL, ET C'EST DÉLIBÉRÉ** : une
        // icône de barre du haut grossie DÉBORDE par le haut de la fenêtre, donc
        // `presented_rect_for` la RAMÈNE dedans — vers le BAS (`clamp_by_translation`).
        // `magnified[1]` est donc PLUS GRAND que `rest[1]`, pas plus petit.
        // Le test de l'horloge, lui, assert `magnified[1] < rest[1]` : c'est que
        // `presented_clock_rect` ne ramène RIEN (l'horloge grandit autour de son
        // centre, sans se déplacer — DEMANDE EXPLICITE). Deux formules voisines,
        // deux comportements, et c'est ici que ça se voit.
        assert!(magnified[0] < rest[0], "la boîte grossie déborde à GAUCHE, elle : {rest:?} -> {magnified:?}");

        // Le point témoin est pris au CENTRE DE LA BOÎTE GROSSIE, et il est donc
        // AILLEURS que celui du repos : mesuré ici, la boîte grossie vaut
        // `[1766, 45, 120, 120]` quand la case au repos vaut `[1802, 12, 48, 48]`
        // — le rappel dans la fenêtre l'a POUSSÉE VERS LE BAS, si bien que son
        // centre (y = 105) tombe SOUS la case au repos (qui finit à 60).
        // C'est ce qui rend l'assertion suivante probante : le même point, au
        // repos, n'est nulle part.
        let milieu = magnified[1] + magnified[3] * 0.5;
        let gained = [magnified[0] + 2.0, milieu];
        assert!(
            note_hit(gained[0], gained[1], &layout, w, h, 2.5),
            "la marge gagnée est cliquable — rest {rest:?}, magnified {magnified:?}, point {gained:?}"
        );
        assert!(!note_hit(gained[0], gained[1], &layout, w, h, 1.0), "…et ne l'est pas au repos");

        let outside = [magnified[0] - 5.0, milieu];
        assert!(!note_hit(outside[0], outside[1], &layout, w, h, 2.5), "hors de la boîte grossie");
        // Et au centre de la case au repos, le clic tombe bien.
        assert!(
            note_hit(rest[0] + rest[2] * 0.5, rest[1] + rest[3] * 0.5, &layout, w, h, 1.0),
            "le centre au repos"
        );
    }

    /// Magnifiée, l'horloge GRANDIT **autour de son centre**, sans se déplacer —
    /// DEMANDE EXPLICITE : « que l'effet de survol des horloges ne fasse pas se
    /// déplacer le texte, il faut juste qu'il s'agrandisse et oscille ». C'est
    /// précisément ce que le rappel-à-l'écran empêchait : il translatait la
    /// boîte vers le bas dès qu'elle dépassait le haut de la fenêtre — ce qui
    /// arrive toujours ici, l'horloge étant dans la barre du haut.
    #[test]
    fn presented_clock_rect_grows_around_its_centre() {
        let layout = test_layout();
        let (w, h) = (120.0, 28.0);
        let rest = clock_rect(&layout, w, h);
        let magnified = presented_clock_rect(&layout, w, h, 2.5);

        assert!(magnified[2] > rest[2] && magnified[3] > rest[3], "{rest:?} -> {magnified:?}");

        let rest_centre = [rest[0] + rest[2] * 0.5, rest[1] + rest[3] * 0.5];
        let magnified_centre = [magnified[0] + magnified[2] * 0.5, magnified[1] + magnified[3] * 0.5];
        assert!((magnified_centre[0] - rest_centre[0]).abs() < 1e-3, "centre en X : {rest:?} -> {magnified:?}");
        assert!((magnified_centre[1] - rest_centre[1]).abs() < 1e-3, "centre en Y : {rest:?} -> {magnified:?}");

        // Conséquence directe du grossissement centré : la boîte grossie
        // CONTIENT la case au repos. C'était faux avec le rappel, qui la
        // décalait au lieu de l'agrandir.
        assert!(magnified[0] <= rest[0] && magnified[1] <= rest[1], "{rest:?} -> {magnified:?}");
    }

    /// Au repos, le clic tombe exactement sur la case dessinée — et surtout PAS
    /// ailleurs : un test de clic vrai partout ne prouverait rien.
    #[test]
    fn clock_hit_at_rest_matches_the_drawn_rect() {
        let layout = test_layout();
        let (w, h) = (120.0, 28.0);
        let rest = clock_rect(&layout, w, h);
        let (cx, cy) = (rest[0] + rest[2] * 0.5, rest[1] + rest[3] * 0.5);

        assert!(clock_hit(cx, cy, &layout, w, h, 1.0), "le centre");
        assert!(clock_hit(rest[0], rest[1], &layout, w, h, 1.0), "le coin haut-gauche est inclus");
        assert!(!clock_hit(rest[0] - 1.0, cy, &layout, w, h, 1.0), "juste à gauche");
        assert!(!clock_hit(cx, rest[1] + rest[3] + 1.0, &layout, w, h, 1.0), "juste en dessous");
    }

    /// **Le clic suit ce qui est DESSINÉ, pas la case au repos** : la marge gagnée
    /// par la magnification est cliquable, et la bande abandonnée ne l'est plus.
    /// Pendant de `icon_at_gives_the_click_to_the_magnified_icon`.
    #[test]
    fn clock_hit_follows_the_magnified_text() {
        let layout = test_layout();
        let (w, h) = (120.0, 28.0);
        let rest = clock_rect(&layout, w, h);
        let magnified = presented_clock_rect(&layout, w, h, 2.5);

        // Prémisses : sans elles, l'assertion ci-dessous ne prouverait rien — et
        // si la géométrie change un jour, l'échec tombe ici, lisiblement.
        assert!(magnified[0] < rest[0], "la boîte grossie déborde à gauche : {rest:?} -> {magnified:?}");
        assert!(magnified[1] < rest[1], "et vers le haut : {rest:?} -> {magnified:?}");

        // GAGNÉ : un point de la marge, touché seulement une fois grossie.
        let gained = [magnified[0] + 2.0, magnified[1] + magnified[3] * 0.5];
        assert!(clock_hit(gained[0], gained[1], &layout, w, h, 2.5), "la marge gagnée est cliquable");
        assert!(!clock_hit(gained[0], gained[1], &layout, w, h, 1.0), "…et ne l'est pas au repos");

        // Et HORS de la boîte grossie, rien n'est cliquable — le clic suit ce qui
        // est DESSINÉ, à toutes les échelles.
        let outside = [magnified[0] - 5.0, magnified[1] + magnified[3] * 0.5];
        assert!(!clock_hit(outside[0], outside[1], &layout, w, h, 2.5), "hors de la boîte grossie");
    }

    /// L'icône « afficher le bureau » occupe le PREMIER rang de la barre, et
    /// elle CÈDE la place aux icônes du dock déposées dans la moitié gauche au
    /// lieu de les recouvrir — même `left_offset` que les pastilles qu'elle
    /// précède, et pour la même raison.
    #[test]
    fn show_desktop_is_leftmost_and_yields_to_top_left_dock_icons() {
        let layout = test_layout();

        let bare = show_desktop_rect(&layout, 0.0);
        assert_eq!(bare[0], DESKTOP_DOTS_MARGIN_LEFT, "sans rien devant, l'icône occupe l'ancre du coin");

        let shifted = show_desktop_rect(&layout, 220.0);
        assert_eq!(shifted[0], DESKTOP_DOTS_MARGIN_LEFT + 220.0, "sinon elle se décale d'autant");
        assert_eq!(shifted[2], bare[2], "seule la position change, jamais la taille");
    }

    /// **Jamais de recouvrement avec la première pastille**, quelles que soient
    /// l'épaisseur de barre et la taille de pastille réglée. C'est la seule
    /// propriété qui compte ici : le dessin des pastilles et leur test de clic
    /// consomment le décalage séparément (voir `show_desktop_shift`), donc un
    /// écart faux d'un côté se lit comme « la pastille ne répond plus ».
    #[test]
    fn show_desktop_never_overlaps_the_first_desktop_dot() {
        for thickness in [48.0, 72.0, 96.0] {
            for dot_size in [18.0, 24.0, 32.0] {
                let layout = DockLayout { top_thickness: thickness, ..test_layout() };
                let icon = show_desktop_rect(&layout, 0.0);
                let first_dot = desktop_dot_rect(&layout, 0, show_desktop_shift(&layout), dot_size);

                assert!(
                    first_dot[0] >= icon[0] + icon[2] + SHOW_DESKTOP_GAP - 0.01,
                    "épaisseur {thickness}, pastille {dot_size} : {icon:?} puis {first_dot:?}"
                );
            }
        }
    }

    /// Le décalage annoncé vaut exactement la largeur de l'icône plus le blanc,
    /// et la première pastille commence EXACTEMENT là — ni trou, ni recouvrement.
    #[test]
    fn show_desktop_shift_is_the_icon_width_plus_the_gap() {
        let layout = test_layout();
        let icon = show_desktop_rect(&layout, 0.0);

        assert_eq!(show_desktop_shift(&layout), icon[2] + SHOW_DESKTOP_GAP);
        assert_eq!(desktop_dot_rect(&layout, 0, show_desktop_shift(&layout), 18.0)[0], icon[0] + show_desktop_shift(&layout));
    }


    /// Le clic correspond à la boîte DESSINÉE, avec ses VRAIS négatifs : sans
    /// magnification il n'y a aucune marge à rattraper, donc un point juste à
    /// côté ne doit rien déclencher — et le clic suit le décalage.
    ///
    /// **ET DEPUIS QU'ELLE MAGNIFIE (2026-09-19), IL SUIT AUSSI L'ÉCHELLE** :
    /// c'est la seconde moitié du test, et c'est elle qui DISCRIMINE — un
    /// `show_desktop_hit` qui ignorerait son paramètre `scale` passerait les cinq
    /// premières assertions sans broncher.
    #[test]
    fn show_desktop_hit_matches_the_drawn_rect() {
        let layout = test_layout();
        let r = show_desktop_rect(&layout, 0.0);
        let (cx, cy) = (r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);

        assert!(show_desktop_hit(cx, cy, &layout, 0.0, 1.0), "le centre");
        assert!(show_desktop_hit(r[0], r[1], &layout, 0.0, 1.0), "le coin haut-gauche est inclus");
        assert!(!show_desktop_hit(r[0] - 1.0, cy, &layout, 0.0, 1.0), "juste à gauche : dehors");
        assert!(!show_desktop_hit(cx, r[1] + r[3] + 1.0, &layout, 0.0, 1.0), "juste en dessous : dehors");
        assert!(!show_desktop_hit(cx, cy, &layout, 220.0, 1.0), "et le clic suit le décalage des icônes du dock");

        // LE `scale` COMPTE : ce point est au-delà du coin droit de la case au
        // repos, donc inatteignable tant que l'icône n'est pas magnifiée — et
        // atteignable une fois qu'elle l'est, à l'endroit où elle est DESSINÉE.
        let (bx, by) = (r[0] + r[2] + 4.0, cy);
        assert!(!show_desktop_hit(bx, by, &layout, 0.0, 1.0), "hors de la case au repos");
        assert!(show_desktop_hit(bx, by, &layout, 0.0, 1.4), "mais dedans une fois magnifiée : le test suit l'échelle");
    }

    /// Elle suit l'épaisseur de la barre exactement comme les boutons média —
    /// même `top_icon_size_for`, même centrage vertical —, donc elle grandit et
    /// rétrécit avec eux quand l'utilisateur règle la taille des barres.
    #[test]
    fn show_desktop_follows_the_bar_thickness() {
        for thickness in [48.0, 72.0, 96.0] {
            let layout = DockLayout { top_thickness: thickness, ..test_layout() };
            let r = show_desktop_rect(&layout, 0.0);

            assert_eq!(r[2], top_icon_size_for(thickness), "côté, épaisseur {thickness}");
            assert_eq!(r[3], r[2], "carrée");
            assert_eq!(r[1], (thickness - r[3]) * 0.5, "centrée verticalement");
        }
    }

    /// **Un jeu de largeurs NEUF n'a aucun séparateur** — l'invariant qui garantit
    /// qu'une barre se dispose exactement comme avant l'arrivée des séparateurs.
    /// Vrai PAR CONSTRUCTION : `Dock::new` pose `NO_SLOT_WIDTHS`, et il n'existe
    /// aucun état global qu'un test voisin aurait pu modifier.
    ///
    /// **Ce test ne nettoie donc rien en sortant** : c'est le gain direct du
    /// passage à une valeur portée par le `Dock` — les tests de ce module ne
    /// dépendent plus de leur ordre d'exécution dans le thread.
    #[test]
    fn a_fresh_set_of_widths_has_every_interval_free() {
        for slot in TOP_SLOTS {
            assert_eq!(slot_count(NO_SLOT_WIDTHS, slot), 0, "{}", slot_name(slot));
            assert_eq!(interval_width(NO_SLOT_WIDTHS, slot), 0.0, "{}", slot_name(slot));
            assert_eq!(marker_offset(NO_SLOT_WIDTHS, slot, 0), None, "{}", slot_name(slot));
        }
    }

    /// Un intervalle porte plusieurs séparateurs, DANS L'ORDRE, et chacun garde
    /// sa largeur : `interval_width` est leur SOMME — c'est elle que consomment
    /// les chaînages — et les milieux s'additionnent dans le sens du rang.
    #[test]
    fn two_separators_in_one_interval_keep_their_ranks() {
        let slot = TopSlot::AfterDots;
        let slots = insert_slot_width(NO_SLOT_WIDTHS, slot, 0, 40.0).unwrap();
        let slots = insert_slot_width(slots, slot, 1, 25.0).unwrap();

        assert_eq!(slot_count(slots, slot), 2);
        assert_eq!(interval_width(slots, slot), 65.0, "la somme des deux");
        assert_eq!(slot_width_at(slots, slot, 0), Some(40.0));
        assert_eq!(slot_width_at(slots, slot, 1), Some(25.0));
        assert_eq!(slot_width_at(slots, slot, 2), None, "il n'y a pas de troisième");
        assert_eq!(marker_offset(slots, slot, 0), Some(20.0));
        assert_eq!(marker_offset(slots, slot, 1), Some(40.0 + 12.5));
    }

    /// Insérer au MILIEU pousse les rangs suivants : c'est le geste « il apparaît
    /// du côté où j'ai cliqué », et la propriété qui le rend utilisable est que
    /// les milieux restent STRICTEMENT croissants.
    #[test]
    fn inserting_in_the_middle_pushes_the_later_ranks_away() {
        let slot = TopSlot::AfterShowDesktop;
        let slots = insert_slot_width(NO_SLOT_WIDTHS, slot, 0, 40.0).unwrap();
        let slots = insert_slot_width(slots, slot, 1, 25.0).unwrap();
        let slots = insert_slot_width(slots, slot, 1, 60.0).unwrap();

        let widths: Vec<f32> = occupied_widths(slots, slot).collect();
        assert_eq!(widths, vec![40.0, 60.0, 25.0], "le nouveau s'est glissé au rang 1");
        let offsets: Vec<f32> = (0..3).map(|rank| marker_offset(slots, slot, rank).unwrap()).collect();
        assert!(offsets[0] < offsets[1] && offsets[1] < offsets[2], "milieux croissants : {offsets:?}");
    }

    /// Retirer un rang TASSE les suivants — l'invariant « sans trou » est reposé à
    /// chaque écriture, donc `slot_count` et les milieux restent justes sans que
    /// personne n'ait à compacter.
    #[test]
    fn removing_a_rank_compacts_and_closes_the_gap() {
        let slot = TopSlot::AfterMedia;
        let mut slots = NO_SLOT_WIDTHS;
        for (rank, px) in [40.0, 25.0, 60.0].into_iter().enumerate() {
            slots = insert_slot_width(slots, slot, rank, px).unwrap();
        }
        let slots = remove_slot_width(slots, slot, 1).unwrap();

        let widths: Vec<f32> = occupied_widths(slots, slot).collect();
        assert_eq!(widths, vec![40.0, 60.0], "le trou est refermé");
        assert_eq!(marker_offset(slots, slot, 1), Some(40.0 + 30.0));
        assert_eq!(interval_width(slots, slot), 100.0);
        assert_eq!(remove_slot_width(slots, slot, 5), None, "un rang qui n'existe pas");
    }

    /// **Le plafond n'est pas un silence** (règle du dépôt : un geste qui rate doit
    /// rater) : la 9ᵉ insertion rend `None`, et `set_interval_widths` — le chemin
    /// du FICHIER — TRONQUE une liste trop longue au lieu de déborder.
    ///
    /// Cette troncature est ce qui protège du `panic = "abort"` du binaire : sans
    /// elle, une ligne de douze valeurs écrite à la main ferait indexer `[f32; 8]`
    /// hors bornes et tuerait le processus sans un mot de journal.
    #[test]
    fn the_interval_is_full_at_the_cap_and_a_long_line_is_truncated() {
        let slot = TopSlot::AfterDots;
        let mut slots = NO_SLOT_WIDTHS;
        for rank in 0..MAX_SLOTS_PER_INTERVAL {
            slots = insert_slot_width(slots, slot, rank, 40.0).expect("il reste de la place");
        }
        assert_eq!(slot_count(slots, slot), MAX_SLOTS_PER_INTERVAL);
        assert_eq!(insert_slot_width(slots, slot, MAX_SLOTS_PER_INTERVAL, 40.0), None, "PLEIN");
        assert_eq!(interval_width(slots, slot), 40.0 * MAX_SLOTS_PER_INTERVAL as f32);

        // Une ligne à DOUZE valeurs : huit gardées, les autres perdues, et surtout
        // AUCUN dépassement.
        let twelve: Vec<f32> = (0..12).map(|_| 30.0).collect();
        let slots = set_interval_widths(NO_SLOT_WIDTHS, slot, &twelve);
        assert_eq!(slot_count(slots, slot), MAX_SLOTS_PER_INTERVAL);
        assert_eq!(interval_width(slots, slot), 30.0 * MAX_SLOTS_PER_INTERVAL as f32);

        // Et des zéros intercalés sont ÉCARTÉS, pas conservés comme des
        // séparateurs invisibles : `[0, 0, 45]` donne UN séparateur de 45.
        let slots = set_interval_widths(NO_SLOT_WIDTHS, slot, &[0.0, 0.0, 45.0]);
        assert_eq!(slot_count(slots, slot), 1);
        assert_eq!(slot_width_at(slots, slot, 0), Some(45.0));
    }

    /// Le rang d'insertion suit la POSITION du clic, dans le sens du rang — c'est
    /// ce qui fait qu'on ajoute « du côté où l'on a cliqué ».
    #[test]
    fn insert_rank_follows_the_click_position() {
        let slot = TopSlot::AfterShowDesktop;
        let slots = insert_slot_width(NO_SLOT_WIDTHS, slot, 0, 40.0).unwrap();
        let slots = insert_slot_width(slots, slot, 1, 40.0).unwrap();

        assert_eq!(insert_rank(slots, slot, 5.0), 0, "avant le premier");
        assert_eq!(insert_rank(slots, slot, 45.0), 1, "entre les deux");
        assert_eq!(insert_rank(slots, slot, 200.0), 2, "après le dernier");
        assert_eq!(insert_rank(NO_SLOT_WIDTHS, slot, 999.0), 0, "un intervalle vide n'a qu'un rang");
    }

    /// La cible de RETRAIT ne descend jamais sous le disque DESSINÉ — mais elle ne
    /// peut pas non plus être agrandie, et c'est un fait à ne pas oublier (voir
    /// `marker_hit_radius`, doc).
    #[test]
    fn a_marker_hit_radius_is_never_smaller_than_the_drawn_disc() {
        use crate::ui::dock::{MAX_SEPARATOR_SIZE, MIN_SEPARATOR_SIZE, SPACER_MARKER_DIAMETER};

        for size in [MIN_SEPARATOR_SIZE, SPACER_MARKER_DIAMETER, MAX_SEPARATOR_SIZE] {
            // Jamais MOINS que le disque dessiné : c'est la règle « on retire ce
            // qu'on voit », et elle doit tenir pour toutes les tailles que
            // l'utilisateur peut régler.
            assert!(marker_hit_radius(size) * 2.0 >= size, "au moins le disque dessiné, taille {size}");
            // Et jamais moins que le PLANCHER : `separator_size` descend à 4 px,
            // viser 4 px à la souris n'est pas un geste.
            assert!(
                marker_hit_radius(size) * 2.0 >= MARKER_HIT_MIN_DIAMETER,
                "au moins le plancher, taille {size}",
            );
        }
        // ⚠️ FAIT À NE PAS OUBLIER : le plafond réel de cette cible est
        // `MAX_SEPARATOR_SIZE`, donc elle NE PEUT PAS ÊTRE AGRANDIE. Le test le
        // fige pour que personne ne compte sur une marge qui n'existe pas.
        assert_eq!(marker_hit_radius(MAX_SEPARATOR_SIZE), MAX_SEPARATOR_SIZE * 0.5);
    }

    /// Le trait de survol traverse l'ÉPAISSEUR de la barre (les espaces du dock
    /// le font dans l'autre sens, voir `ui::dock::spacer_hover_line_rect`), il
    /// est centré sur le milieu de l'emplacement, et il garde la marge des bords
    /// et l'épaisseur des traits du dock — les deux familles doivent se lire
    /// comme la même chose.
    #[test]
    fn the_slot_hover_line_crosses_the_bar_and_centres_on_the_slot() {
        let layout = test_layout();
        let mid = 900.0;

        for width in [DEFAULT_SLOT_WIDTH, 200.0, MAX_SLOT_WIDTH] {
            let line = slot_hover_line_rect(mid, &layout);

            assert_eq!(line[0] + line[2] * 0.5, mid, "centré sur l'emplacement, largeur {width}");
            assert_eq!(line[2], crate::ui::dock::SPACER_HOVER_LINE_PX, "même épaisseur que le dock");
            assert_eq!(line[1], crate::ui::dock::SPACER_HOVER_LINE_MARGIN, "même marge haute");
            assert_eq!(
                line[1] + line[3],
                layout.top_thickness - crate::ui::dock::SPACER_HOVER_LINE_MARGIN,
                "même marge basse"
            );
        }
    }

    /// La CASE d'un emplacement — son intervalle, pas le seul cercle de son
    /// marqueur : c'est la cible du survol, et un cercle de `separator_size` est
    /// bien trop fin pour être visé à la souris (même choix que les espaces du
    /// dock, qui testent leur emplacement entier).
    #[test]
    fn a_slot_is_hovered_over_its_whole_case_and_never_outside_the_bar() {
        let layout = test_layout();
        let (mid, width) = (900.0, DEFAULT_SLOT_WIDTH);
        let cy = layout.top_thickness * 0.5;

        assert!(slot_contains(mid, width, &layout, mid, cy), "le milieu");
        assert!(slot_contains(mid, width, &layout, mid - width * 0.5 + 1.0, cy), "le bord gauche est dedans");
        assert!(slot_contains(mid, width, &layout, mid + width * 0.5 - 1.0, cy), "le bord droit est dedans");
        assert!(!slot_contains(mid, width, &layout, mid - width * 0.5 - 1.0, cy), "juste avant : dehors");
        assert!(!slot_contains(mid, width, &layout, mid + width * 0.5 + 1.0, cy), "juste après : dehors");
        assert!(!slot_contains(mid, width, &layout, mid, layout.top_thickness), "sous la barre : dehors");
        assert!(!slot_contains(mid, width, &layout, mid, -1.0), "au-dessus de la barre : dehors");
    }

    /// La borne du geste et le bornage du réglage sont deux choses différentes :
    /// Les largeurs sont bornées, et `0.0` vaut « RETIRÉ » (voir
    /// `persist_top_slots`, qui n'écrit pas un intervalle libre).
    ///
    /// **Le bornage est testé SUR `clamp_slot_width` elle-même**, parce que
    /// `canonical_interval` — par qui passent TOUS les chemins d'écriture, le
    /// curseur comme le fichier — l'appelle : c'est donc elle qui protège le
    /// chargement d'un fichier écrit à la main.
    #[test]
    fn a_slot_width_is_bounded_and_zero_means_removed() {
        let slot = TopSlot::AfterShowDesktop;

        assert_eq!(clamp_slot_width(120.0), 120.0);
        assert_eq!(clamp_slot_width(MAX_SLOT_WIDTH + 1000.0), MAX_SLOT_WIDTH, "plafonné");
        assert_eq!(clamp_slot_width(-5.0), 0.0, "une valeur négative ne donne pas de séparateur");

        let slots = insert_slot_width(NO_SLOT_WIDTHS, slot, 0, 40.0).unwrap();
        let slots = set_slot_width_at(slots, slot, 0, MAX_SLOT_WIDTH + 1000.0).unwrap();
        assert_eq!(slot_width_at(slots, slot, 0), Some(MAX_SLOT_WIDTH), "le curseur aussi passe par elle");

        let slots = set_slot_width_at(slots, slot, 0, 0.0).unwrap();
        assert_eq!(slot_count(slots, slot), 0, "`0.0` RETIRE le séparateur");
        assert_eq!(set_slot_width_at(NO_SLOT_WIDTHS, slot, 3, 40.0), None, "un rang inexistant");
    }

    /// **Une écriture ne peut pas laisser d'intervalle non canonique** — c'est
    /// l'invariant du type, tenu par `set_interval_row`, et le test le prend par
    /// le chemin du FICHIER comme par celui du curseur.
    #[test]
    fn no_write_path_can_leave_a_hole() {
        let slot = TopSlot::AfterDots;
        // Une liste à trous vient d'un fichier écrit à la main.
        let slots = set_interval_widths(NO_SLOT_WIDTHS, slot, &[0.0, 45.0, 0.0, 30.0]);

        let widths: Vec<f32> = occupied_widths(slots, slot).collect();
        assert_eq!(widths, vec![45.0, 30.0], "tassé, sans trou");
        assert_eq!(slot_count(slots, slot), 2);
        assert_eq!(slot_width_at(slots, slot, 2), None);
        // Et `canonical` — le point d'entrée de `Dock::set_slot_widths` — fait la
        // même chose sur les trois intervalles d'un coup.
        let mut raw = NO_SLOT_WIDTHS;
        raw[0][2] = 60.0;
        assert_eq!(canonical(raw)[0][0], 60.0, "le trou a disparu");
        assert_eq!(canonical(raw)[0][1], 0.0);
    }

    // --- Taille de police RÉGLABLE des textes de la barre du haut (DEMANDE
    // EXPLICITE, 2026-09-23 : réglage automatique ou taille personnalisée) ---

    /// `None` rend EXACTEMENT ce que rendait le calcul d'avant ce réglage : le
    /// mode automatique n'est pas un cas particulier, c'est le comportement
    /// historique, et les quatre textes doivent y être inchangés.
    #[test]
    fn sans_reglage_les_quatre_textes_suivent_la_barre() {
        let layout = test_layout();
        let auto = topbar_text_font_px_auto(layout.top_thickness);
        assert_eq!(clock_font_px(&layout, None), auto + CLOCK_FONT_SIZE_BOOST_PX);
        assert_eq!(sys_monitor_font_px(&layout, None), auto - TOPBAR_SECONDARY_TEXT_SIZE_STEP_PX);
        assert_eq!(refresh_rate_font_px(&layout, None), auto - TOPBAR_SECONDARY_TEXT_SIZE_STEP_PX);
        assert_eq!(media_title_font_px(&layout, None), auto - TOPBAR_SECONDARY_TEXT_SIZE_STEP_PX);
    }

    /// **LES ÉCARTS SURVIVENT AU RÉGLAGE MANUEL** — c'est la décision de
    /// l'utilisateur (« la base, écarts gardés ») : ce qu'on règle est la taille
    /// de RÉFÉRENCE, pas celle des quatre textes. Une valeur réglée doit donc
    /// donner exactement les mêmes deltas qu'en automatique, à toute taille.
    #[test]
    fn regler_la_taille_deplace_les_quatre_textes_du_meme_ecart() {
        let layout = test_layout();
        for regle in [20.0_f32, 40.0, 57.0] {
            let base = regle.round() as i32;
            assert_eq!(clock_font_px(&layout, Some(regle)), base + CLOCK_FONT_SIZE_BOOST_PX, "horloge, réglé {regle}");
            for (nom, obtenu) in [
                ("moniteur système", sys_monitor_font_px(&layout, Some(regle))),
                ("fréquence", refresh_rate_font_px(&layout, Some(regle))),
                ("titre média", media_title_font_px(&layout, Some(regle))),
            ] {
                assert_eq!(obtenu, base - TOPBAR_SECONDARY_TEXT_SIZE_STEP_PX, "{nom}, réglé {regle}");
            }
        }
    }

    /// **LE PLAFOND SUIT LA BARRE, ET C'EST LUI QUI EST LE MAXIMUM DU CURSEUR**
    /// (décision utilisateur : « plafonné par la barre ») : un texte réglé ne
    /// peut jamais déborder de sa barre, quelle que soit la valeur reçue — y
    /// compris une valeur devenue trop grande parce que la barre a été AMINCIE
    /// après le réglage, cas que le curseur seul ne peut pas rattraper.
    #[test]
    fn une_taille_reglee_ne_deborde_jamais_de_sa_barre() {
        let layout = test_layout();
        let max = topbar_text_font_px_max(layout.top_thickness);
        assert!(max < layout.top_thickness as i32, "le plafond reste sous l'épaisseur");
        assert_eq!(clock_font_px(&layout, Some(10_000.0)), max + CLOCK_FONT_SIZE_BOOST_PX, "valeur absurde ramenée au plafond");

        // Barre AMINCIE après coup : la même valeur réglée doit se ramener au
        // NOUVEAU plafond, sinon le texte déborderait.
        let mince = DockLayout { top_thickness: 40.0, ..test_layout() };
        let max_mince = topbar_text_font_px_max(mince.top_thickness);
        assert!(max_mince < max, "amincier abaisse bien le plafond");
        assert_eq!(clock_font_px(&mince, Some(max as f32)), max_mince + CLOCK_FONT_SIZE_BOOST_PX);
    }

    /// Le plancher, lui, ne dépend pas de la barre : une taille nulle ou négative
    /// ne doit jamais produire une police indessinable.
    #[test]
    fn une_taille_reglee_ne_descend_jamais_sous_le_plancher() {
        let layout = test_layout();
        for absurde in [0.0_f32, -10.0] {
            assert_eq!(topbar_text_font_px(layout.top_thickness, Some(absurde)), TOPBAR_TEXT_FONT_PX_MIN, "réglé {absurde}");
        }
    }

    /// Le plafond reste AU-DESSUS de l'automatique, à toute épaisseur — sans quoi
    /// le curseur n'aurait plus de course au-dessus du réglage par défaut, ce qui
    /// était tout l'objet de la demande.
    #[test]
    fn le_plafond_laisse_toujours_de_la_marge_au_dessus_de_l_automatique() {
        for epaisseur in [20.0_f32, 40.0, 48.0, 72.0, 120.0] {
            let auto = topbar_text_font_px_auto(epaisseur);
            let max = topbar_text_font_px_max(epaisseur);
            assert!(max >= auto, "épaisseur {epaisseur} : plafond {max} sous l'automatique {auto}");
            assert!(max >= TOPBAR_TEXT_FONT_PX_MIN, "épaisseur {epaisseur} : plafond indessinable");
        }
    }
}
