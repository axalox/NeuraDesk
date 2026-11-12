//! Machinerie COMMUNE à toutes les fenêtres popup CEF (voir
//! `docs/plans/cef-web-ui-migration.md`) — extraite après duplication quasi
//! identique entre `cef_test_window.rs` (Alimentation) et `cef_calendar.rs`
//! (Calendrier), au moment d'ajouter un 3e/4e popup (Contexte/Réseau) : le
//! plan notait déjà cette factorisation comme "à faire une fois plus
//! d'exemples existent pour généraliser en sécurité" (Phase 0) — c'est le
//! cas maintenant.
//!
//! Chaque popup garde son PROPRE `thread_local! { static SLOT: PopupSlot }`
//! (une fenêtre Win32 par popup, jamais partagée — voir `PopupSlot`) et son
//! PROPRE `unsafe extern "system" fn` wndproc (un pointeur de fonction ne
//! peut pas "savoir" quel thread_local utiliser, voir `dispatch_input`),
//! mais délègue toute la LOGIQUE (création de fenêtre, bascule
//! montrer/masquer, rendu composé avec le fond flouté, relais souris/focus)
//! aux fonctions de ce module, via `PopupSlot`/`PopupSpec`. Voir
//! `win32::cef_test_window` pour un exemple concret d'utilisation minimal.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::thread::LocalKey;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext};
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, ScreenToClient, SetWindowRgn, HBRUSH};
use windows::Win32::UI::Controls::{MARGINS, WM_MOUSELEAVE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetDoubleClickTime, GetKeyState, ReleaseCapture, SetCapture, SetFocus,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetClientRect, GetMessageTime, GetSystemMetrics, GetWindowRect, SM_CXDOUBLECLK, SM_CYDOUBLECLK, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    LoadCursorW, RegisterClassExW, SetCursor, SetForegroundWindow, SetWindowPos, ShowWindow, GetWindowLongW, GWL_EXSTYLE, HCURSOR, HTCLIENT, HWND_TOP, HWND_TOPMOST, WINDOWPOS, WM_WINDOWPOSCHANGED, WM_WINDOWPOSCHANGING, GetTopWindow, GetWindow, GW_HWNDNEXT, HWND_BOTTOM, HWND_NOTOPMOST,
    IDC_ARROW, SHOW_WINDOW_CMD, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, WM_CHAR, WM_KEYDOWN, WM_KEYUP,
    WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_SETCURSOR,
    WM_SETFOCUS, WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP, WNDCLASSEXW, WNDPROC, WS_EX_APPWINDOW, WS_EX_LAYERED,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WindowFromPoint,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};

use crate::gfx::backdrop;
use crate::gfx::device::GfxDevice;
use crate::gfx::quad_renderer::{QuadBatch, QuadInstance, QuadRenderer};
use crate::gfx::swapchain::SwapChain;
use crate::gfx::texture::Texture;
use crate::ui::color;
use crate::win32::cef_app::CefBrowserSurface;
use crate::win32::fade;
use crate::win32::menu;

/// Étend le glass DWM sur toute la fenêtre `hwnd` (transparence réelle) et
/// l'exclut de toute capture d'écran (la nôtre y compris, voir
/// `gfx::backdrop`) — à appeler juste après `CreateWindowExW`, avant tout
/// `ShowWindow`. Récupérée telle quelle de l'ancien `win32::egui_overlay`
/// (retiré en Phase 7 de `docs/plans/cef-web-ui-migration.md` avec le
/// reste d'`egui`) : ces deux fonctions n'avaient jamais rien d'`egui`
/// elles-mêmes, seulement hébergées là parce que les trois fenêtres qui en
/// avaient besoin AVANT cette migration l'étaient — `win32::cef_popup` en
/// est le seul appelant désormais, leur vraie place.
fn make_transparent_and_uncapturable(hwnd: HWND) {
    let margins = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        // Pose l'affinité d'affichage ET l'exclusion de la Desktop Duplication
        // ensemble — voir `capture_visibility::apply_to_window`, doc.
        crate::gfx::capture_visibility::apply_to_window(hwnd);
    }
}

/// Force `hwnd` au premier plan de façon FIABLE — voir l'ancien
/// `win32::egui_overlay::force_foreground` (récupérée telle quelle, voir la
/// doc de `make_transparent_and_uncapturable` ci-dessus) : `SetForegroundWindow`
/// seul est soumis à la restriction "lock" de Windows et peut échouer
/// SILENCIEUSEMENT selon le contexte de lancement — contournement standard
/// Win32 via `AttachThreadInput`, temporaire, sans attache permanente.
pub fn force_foreground(hwnd: HWND) {
    unsafe {
        let current_thread = GetCurrentThreadId();
        let foreground = GetForegroundWindow();
        let foreground_thread = if foreground.0.is_null() { 0 } else { GetWindowThreadProcessId(foreground, None) };
        let target_thread = GetWindowThreadProcessId(hwnd, None);

        let attached_foreground = foreground_thread != 0
            && foreground_thread != current_thread
            && AttachThreadInput(current_thread, foreground_thread, true).as_bool();
        let attached_target = target_thread != 0
            && target_thread != current_thread
            && target_thread != foreground_thread
            && AttachThreadInput(current_thread, target_thread, true).as_bool();

        let _ = SetForegroundWindow(hwnd);

        // ⭐ **ET ON REMONTE LE RANG Z.** Défaut signalé le 2026-10-22 : « la
        // fenêtre se place sous la fenêtre qui était au premier plan ». MESURÉ
        // (sonde Z sur la kaomoji) : le premier plan bascule bien (< 100 ms après
        // le clic), mais le rang reste à 107-111, SOUS le terminal à 84. Deux
        // raisons, et aucune fenêtre de l'app ne les rattrape : poser le premier
        // plan sur une fenêtre voilée DWM (l'effet d'ouverture — rattrapé au
        // dévoilage, `ouverture_fenetres::devoiler`) ne la remonte pas ; et le réflecteur de punaise par frame,
        // `HWND_NOTOPMOST` sur une fenêtre DÉJÀ non-topmost, ne remonte RIEN — il
        // ne fait que retirer le style topmost. Un `HWND_TOP` ici, au seul moment
        // où l'intention « premier plan » est marquée, pose la fenêtre au sommet
        // de sa bande (et dans la bande topmost si la punaise l'y a mise —
        // `HWND_TOP` n'y change rien, il ne fait que la placer en tête de sa
        // bande). UN coup, pas par frame : que les fenêtres cliquées APRÈS
        // recouvrent la nôtre, c'est justement le rôle de la punaise éteinte.
        let _ = SetWindowPos(hwnd, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);

        if attached_target {
            let _ = AttachThreadInput(current_thread, target_thread, false);
        }
        if attached_foreground {
            let _ = AttachThreadInput(current_thread, foreground_thread, false);
        }

        // ⚠️ **`SetFocus` APRÈS LES DÉTACHEMENTS, ET L'ORDRE EST L'ENJEU.**
        //
        // Activer une fenêtre (`SetForegroundWindow`, ci-dessus) et lui donner
        // le **focus clavier** sont DEUX choses : sans le second, la fenêtre est
        // au premier plan et ne reçoit ni le clavier, ni la molette, ni
        // `WM_SETFOCUS` — ce sont les trois messages que Windows destine à la
        // fenêtre FOCALISÉE, alors que le survol et le clic visent la fenêtre
        // sous le curseur, ce qui rendait le symptôme trompeur.
        //
        // MESURÉ le 2026-09-22 : posé AVANT les détachements, `SetFocus` ne
        // produisait **aucun** `WM_SETFOCUS` — tant que les files d'entrée sont
        // attachées, le focus est PARTAGÉ, et le détachement le rend à l'autre
        // fil. Posé ICI, il produit le message, CEF est mis au focus (`WM_SETFOCUS`
        // → `set_focus(true)`), et le champ de recherche du Dash répond.
        let _ = unsafe { SetFocus(Some(hwnd)) };
    }
}

/// Voir `win32::dash::TOGGLE_DEBOUNCE`, doc détaillée là-bas : un double
/// `MouseDown` rapproché (double-clic, ou un défaut matériel de souris)
/// ouvre puis referme AUSSITÔT sans qu'une seule frame n'ait eu l'occasion
/// de s'afficher entre les deux.
const TOGGLE_DEBOUNCE: Duration = Duration::from_millis(300);
/// Voir `win32::dash::FOCUS_LOST_DEBOUNCE`, même principe.
const FOCUS_LOST_DEBOUNCE: Duration = Duration::from_millis(150);

/// **QUAND UNE TAILLE ANNONCÉE PAR UNE PAGE EST-ELLE CRUE ?** — voir
/// `appliquer_la_mesure`, qui porte la mesure du défaut.
///
/// ⛔ **LA PREMIÈRE ANNONCE D'UNE OUVERTURE DÉCRIT UN CONTENU EN COURS DE
/// MONTAGE.** Mesuré le 2026-11-10 sur le menu Contexte, à chaque ouverture et
/// quel que soit le zoom de la page : `209×300` unités CSS, puis `298×259`,
/// puis `297×240`, puis `296×240` — la page se reconstruit pendant ~400 ms.
/// Appliquées à la lettre, ces annonces font la cascade de redimensionnements
/// que l'utilisateur voit (« les menus sont encore buggués »). Seule une
/// annonce CONCORDANTE avec la précédente est crue.
const MESURE_TOLERANCE: i32 = 4;
/// Sans nouvelle annonce pendant ce délai, la dernière en attente s'applique :
/// c'est le repli pour une page qui ne se mesure qu'UNE fois (aucune
/// confirmation possible).
const MESURE_SILENCE: Duration = Duration::from_millis(350);
/// Au-delà, la stabilisation est terminée : tout s'applique comme avant. Borne
/// les pages dont le contenu bouge en continu (`cef_voicemeeter`).
const MESURE_OUVERTURE_MAX: Duration = Duration::from_millis(1000);
/// **LA MESURE D'OUVERTURE PART DEUX FRAMES APRÈS LE MONTAGE** — le temps que
/// `render` ait posé le zoom du menu (il ne tourne que pour une fenêtre
/// VISIBLE). La demander dans `toggle_with` la fait prendre au zoom 3× de la
/// page cachée : voir `PopupSlot::mesure_differee`, qui porte la mesure du
/// défaut et le cas `sound.html`.
///
/// ⚠️ **CE N'EST PAS LA MÊME CHOSE QUE LA PISTE REFUTÉE** « mesurer avant de
/// montrer » : là, la fenêtre restait CACHÉE, et la mise en page d'une page
/// cachée ne suit pas `set_zoom_level`. Ici la fenêtre est montrée et
/// redimensionnée ; seul l'ORDRE entre le zoom et la mesure est corrigé.
const MESURE_DIFFEREE_FRAMES: u8 = 2;

/// Facteur de zoom voulu pour TOUS les popups CEF (voir `CONTENT_ZOOM_LEVEL`
/// ci-dessous) — DEMANDE EXPLICITE : "multiplie par 3 la taille des menus et
/// des dashs". Un seul endroit à changer pour ajuster tous les popups à la
/// fois (même politique que `win32::menu::MENU_SCALE`, désormais orpheline
/// depuis le retrait d'`egui`/`win32::popup_menu` en Phase 7 — cette
/// constante-ci est ce qui la remplace pour les popups CEF).
///
/// `pub(crate)` : `app::run` s'en sert pour dimensionner le BANDEAU, dont la
/// taille voulue est exprimée en CSS (voir son rectangle) — la seule façon
/// qu'une carte « 420 × 140 » occupe vraiment 420 × 140 unités de page.
pub(crate) const CONTENT_ZOOM_FACTOR: f64 = 3.0;

/// **LA PART MAXIMALE DE L'ÉCRAN QU'UNE FENÊTRE DÉRIVÉE DE SON CONTENU PEUT
/// OCCUPER, EN POURCENT — SUR LES DEUX AXES.**
///
/// Les fenêtres des **caractères** et des **kaomojis** calculent leur taille à
/// partir de leur CONTENU, jamais d'un chiffre choisi : rien, dans ce calcul, ne
/// connaît l'écran. Sur une machine plus petite que celle où elles ont été
/// réglées, elles le dépassaient donc en silence — **150 px sous le bord bas**
/// d'un 1920×1080 pour les caractères, **300 px hors du bord droit** pour les
/// kaomojis. Dans les deux cas INATTEIGNABLES, et pour la même raison : c'est la
/// FENÊTRE qui sort de l'écran, pas son contenu, donc ni le défilement ni le
/// redimensionnement ne les rattrapent.
///
/// ⛔ **UN SEUL CHIFFRE, ICI, POUR LES DEUX** : chaque fenêtre en portait sa
/// copie, et rien ne les empêchait de diverger — le même raisonnement qui a fait
/// extraire `CONTENT_ZOOM_FACTOR` juste au-dessus. Une copie par fichier n'avait
/// aucun garde : changer l'une laissait les tests de l'autre verts, et les deux
/// commentaires faux.
///
/// ⚠️ **CE N'EST PAS LES DEUX AUTRES « PART DE L'ÉCRAN » DU DÉPÔT** —
/// `cef_couleurs::PART_MAX` (0,9) et `epingle::PART_ECRAN_MAX` (0,8). Trois
/// valeurs différentes, et c'est VOULU : elles bornent des fenêtres dont la
/// taille ne se dérive pas de la même façon. Ce qui est partagé ICI est la
/// politique qui vaut pour une taille DÉRIVÉE DU CONTENU — celle où, dans le
/// calcul, personne ne connaît l'écran.
///
/// ⚠️ **ET CE N'EST PAS UNE GARANTIE SUR LA FENÊTRE MONTÉE.** Le plafond
/// s'applique au moment où la taille est CALCULÉE. Or `faut_appliquer_le_rect`
/// ne repose le rectangle qu'à la PREMIÈRE monte (voir sa doc : « la place
/// appartient à l'utilisateur ») — une fenêtre déjà ouverte quand la topologie
/// d'écran change garde donc son ancien rectangle jusqu'à ce qu'on la déplace.
pub(crate) const PART_ECRAN: i32 = 85;

/// Convertit un FACTEUR de zoom en niveau Chromium (voir
/// `CefBrowserSurface::set_zoom_level`, doc détaillée là-bas : l'API CEF
/// prend un NIVEAU, pas un facteur direct — `facteur = 1.2^niveau`).
/// `f64::ln` n'est pas `const fn` en Rust stable — calculé à chaque appel
/// plutôt qu'à la compilation, coût négligeable (une poignée d'appels par
/// frame au plus, voir les deux fonctions ci-dessous).
fn zoom_level_for_factor(factor: f64) -> f64 {
    factor.ln() / 1.2_f64.ln()
}

/// Niveau de zoom pour les popups "fenêtre" (Dash/Réglages/Presse-papiers/
/// **Notes**/fenêtre de test) — `CONTENT_ZOOM_FACTOR` seul, PAS affecté par
/// `win32::menu::menu_scale` (voir `menu_zoom_level` ci-dessous, réservé
/// aux popups "menu").
///
/// **Il n'est PAS suffisant de l'appliquer une fois au chargement** — c'est
/// pourtant ce que faisait `NeuraDeskLoadHandler::on_load_end`, et ça ne tient
/// pas pour `assets/popups/notes.html` (défaut réel, mesuré : voir le `match`
/// de `render`, qui le réapplique désormais à CHAQUE frame, seul endroit qui
/// documente le pourquoi complet).
fn content_zoom_level() -> f64 {
    zoom_level_for_factor(CONTENT_ZOOM_FACTOR)
}

/// Niveau de zoom pour les popups "menu" (menus contextuels, Réseau/Son/
/// Voicemeeter/Calendrier/tailles de barre/presse-papiers rapide) — voir
/// `win32::menu::menu_scale`, doc de champ, pour pourquoi son point neutre
/// (`1.0`) laisse `CONTENT_ZOOM_FACTOR` INCHANGÉ par défaut. Recalculé et
/// réappliqué à CHAQUE FRAME par `render` (via `CefBrowserSurface::
/// set_zoom_level`, PAS seulement à la création comme les popups "fenêtre"
/// ci-dessus) : contrairement à eux, le zoom voulu peut changer alors que
/// le popup reste ouvert (utilisateur bougeant le curseur "Taille des
/// menus" du panneau de réglages).
///
/// L'échelle de contenu : voir `app::content_scale_pour`, doc de fonction —
/// c'est le **RATIO DE DPI** de l'écran visé (`win32::dpi::ratio_dpi_de_l_ecran`),
/// multiplié par le réglage manuel de cet écran s'il existe. `1.0` (point neutre,
/// comme `menu::menu_scale`) sur la barre PRIMAIRE et sur tout écran de même DPI.
///
/// ⚠️ **CE N'EST PLUS UN ÉCART DE RÉSOLUTION** (c'était `app::screen_content_scale`,
/// retiré le 2026-10-07) : un panneau doit occuper la même place dans le CHAMP DE
/// VISION d'un écran à l'autre, pas la même quantité de pixels physiques. Réduire
/// selon la seule résolution compensait à moitié, sans jamais rendre la taille
/// constante.
///
/// Chaque module popup "menu" (voir `win32::cef_sound`, doc de module, pour le
/// pattern complet) le mémorise dans son propre `thread_local` à l'ouverture (voir
/// son `toggle`) et le relit ici à chaque frame, exactement comme
/// `menu::menu_scale` juste au-dessus.
pub(crate) fn menu_zoom_level(content_scale: f32) -> f64 {
    zoom_level_for_factor(menu_zoom_factor(content_scale))
}

/// Le FACTEUR de zoom d'un popup "menu" — `menu_zoom_level` n'en est que la
/// conversion en niveau Chromium.
///
/// **`pub(crate)` POUR QUE `app::run` PUISSE DIMENSIONNER UN POPUP EN UNITÉS
/// DE PAGE.** Un `[i32; 4]` ne dit pas s'il porte des pixels ou des unités de
/// page ; les popups qui taillent leur contenu (le bandeau) doivent multiplier
/// par CECI. Reproduire la formule ailleurs serait juste — jusqu'à ce que
/// `menu_scale` bouge (l'utilisateur est à 0.9, donc ×2.7 et non ×3).
pub(crate) fn menu_zoom_factor(content_scale: f32) -> f64 {
    CONTENT_ZOOM_FACTOR * menu::menu_scale() as f64 * content_scale as f64
}

/// Hauteur (px physiques, à l'échelle NEUTRE `menu_scale = 1.0` — voir plus
/// bas, BUG RÉEL CONSTATÉ EN PRATIQUE, PAS déjà mise à l'échelle) d'un
/// popup "menu" à LISTE DE BOUTONS SIMPLE partageant `assets/popups/
/// menu.css` (Alimentation/Contexte/Réseau — PAS Voicemeeter, dont le
/// contenu varie RÉELLEMENT selon l'installation/les canaux masqués, voir
/// `win32::cef_voicemeeter::estimated_content_height`, même principe mais
/// PAS la même formule : Voicemeeter a sa propre mise en page en grille,
/// pas des `<button>` empilés). Nombre de boutons/séparateurs CONNU
/// D'AVANCE pour ces trois popups (jamais dépendant de données Windows en
/// direct) — DEMANDE EXPLICITE : "le menu d'alimentation est trop grand,
/// ne peux-tu pas utiliser un backend en rust pour gérer ça ?" — remplace
/// une constante en dur par popup (`app::menu_rect(anchor, W, H, …)`
/// avec un `H` mesuré une fois, qui se périme silencieusement au moindre
/// changement de `assets/popups/menu.css`, exactement ce qui vient de se
/// produire : la taille de police commune passée de 22/20px à 15px a
/// laissé le fond flouté du menu Alimentation bien plus grand que son
/// contenu réel, sans qu'aucun des trois popups concernés n'ait besoin
/// d'être retouché individuellement).
///
/// Constantes calibrées par MESURE RÉELLE (capture back-buffer) sur DEUX
/// popups réels à l'échelle neutre (`menu_scale=1.0`) une fois `menu.css`
/// en place : le menu Contexte (1 bouton, 0 séparateur, 0 titre de
/// section — AVANT l'ajout de la liste de fréquences, voir plus bas)
/// mesuré à 114px de contenu, le menu Alimentation (6 boutons, 1
/// séparateur) à 769px, et le menu Contexte (3 boutons, 1 séparateur) à
/// 402px — mesuré par capture le 2026-09-19. `PER_SEPARATOR` est estimé
/// depuis le CSS (`hr` : 1px de bordure + 8px de marge de chaque côté),
/// `OVERHEAD` est le `padding` de `.menu` (voir le commentaire des
/// constantes, qui porte tout le raisonnement), et `PER_BUTTON` est
/// ensuite résolu pour que le menu Alimentation garde EXACTEMENT sa
/// hauteur d'origine — **aucun grand menu ne change donc de taille**.
///
/// Mieux vaut un peu d'espace résiduel qu'un contenu tronqué sur une
/// police/un DPI légèrement différent : voir les tests, qui bornent
/// maintenant cette marge DES DEUX CÔTÉS — un plafond seulement relatif,
/// parce que ces nombres mesurés sont l'étendue du CONTENU et que le
/// résultat est une hauteur de BOÎTE.
///
/// `section_title_count` — BUG RÉEL SIGNALÉ ("je n'arrive pas à
/// sélectionner une fréquence" + "le menu a une barre de défilement") :
/// `assets/popups/context.html` a gagné un `<div class="section-title">`
/// ("Fréquence de rafraîchissement") après cette calibration initiale,
/// jamais compté ici (ni bouton, ni séparateur) — la fenêtre restait donc
/// systématiquement sous-dimensionnée de la hauteur d'un titre de
/// section, activant le `overflow-y: auto` de `.menu` et poussant les
/// dernières entrées de fréquence sous le pli, hors de portée d'un clic
/// tant qu'on ne défilait pas d'abord. `PER_SECTION_TITLE` : estimé
/// depuis le CSS (`padding: 10px 14px 4px` + une ligne de texte), pas
/// mesuré aussi précisément que les deux points ci-dessus (un seul popup
/// en a un actuellement) — volontairement généreux, même philosophie que
/// le reste de cette fonction.
///
/// NE multiplie PAS par `menu::menu_scale()` — BUG RÉEL SIGNALÉ ("en
/// changeant la taille de menus depuis les réglages le menu wifi et le
/// menu alimentation... se réduisent trop") : ce résultat est TOUJOURS
/// passé comme `base_h` à `app::menu_rect(anchor, base_w, base_h, …)`
/// (voir ses appelants), qui applique DÉJÀ `menu_scale` par-dessus tous
/// les `base_w`/`base_h` qu'on lui donne — appliquer le facteur ICI AUSSI
/// le mettait au carré (`scale²`) pour la SEULE hauteur (`base_w` restant
/// à `scale¹`, littéral) : à `menu_scale = 0.5`, la fenêtre se réduisait
/// donc à 25% de hauteur au lieu de 50%, hors de proportion avec sa
/// largeur et avec tous les autres popups "menu" (qui passent un `base_h`
/// déjà "neutre" à `menu_rect`, exactement comme celui-ci doit le
/// faire). Contraste avec `win32::cef_voicemeeter::
/// estimated_content_height`, qui multiplie bien par `menu_scale` lui-même
/// — mais son résultat va DIRECTEMENT à `resize_height_to_content`, JAMAIS
/// à `menu_rect` : deux chemins différents, deux conventions
/// différentes, ne pas les confondre.
pub(crate) fn button_list_height(button_count: usize, separator_count: usize, section_title_count: usize) -> i32 {
    // RECALIBRÉ le 2026-09-19 sur les TROIS points réellement mesurés (voir la
    // doc de fonction) : `OVERHEAD + PER_BUTTON = 114` et
    // `OVERHEAD + 6 × PER_BUTTON + 55 = 769` se résolvent en `OVERHEAD ≈ 0` et
    // `PER_BUTTON ≈ 120` — et ce couple prédit 415 pour le menu Contexte
    // (3 boutons + 1 séparateur), dont le contenu a été MESURÉ à 402 px par
    // capture : il le couvre avec 13 px de marge. Les trois points concordent.
    //
    // **CE QUE CES VALEURS REMPLACENT, ET POURQUOI : `OVERHEAD = 90` faisait
    // presque DOUBLER la hauteur des petits menus** (1 bouton : 215 px estimés
    // pour 114 px de contenu, +89 %), d'où un vide très visible en bas — DÉFAUT
    // RÉEL SIGNALÉ : « il y a un grand espace vide inutile en bas du menu »,
    // mesuré à 157 px sur 520 pour le menu Contexte. L'ancienne marge avait été
    // relevée (« la première calibration laissait une barre de défilement sur le
    // menu Alimentation ») ; **mais ce n'est PAS l'estimation qui était en
    // cause** : un modèle linéaire ne peut pas satisfaire à la fois « 1 bouton
    // = 114 » et « 6 boutons + 1 séparateur ≥ 840 », donc le défilement venait du
    // CSS (`.menu { height: 100vh; overflow-y: auto }`, où un contenu qui remplit
    // PILE la fenêtre déclenche une barre parasite). **Si une barre de défilement
    // réapparaît un jour, le premier suspect est ce CSS, pas ces constantes.**
    // ⚠️ **`OVERHEAD` N'EST PAS ZÉRO, ET CE N'EST PAS UNE MARGE ARBITRAIRE : c'est
    // le `padding` de la boîte `.menu`.** Le CSS pose `box-sizing: border-box`
    // (voir `context.html`), donc `height: 100vh` INCLUT ce padding : la hauteur
    // qu'on donne à la fenêtre doit couvrir le contenu **plus** les 12 px de haut
    // et de bas. Un `OVERHEAD = 0` — essayé, mesuré — laissait la boîte plus
    // courte que son contenu de quelques pixels, donc une **barre de défilement
    // fantôme** (colonnes x=849..851 couvertes sur 366 des 374 lignes de la
    // capture).
    //
    // **C'est ce que l'ancienne marge de 90 px compensait sans le dire** : le
    // commentaire d'origine attribuait le rail à l'estimation, alors qu'il vient
    // de ce padding. 24 px le couvrent exactement, et le reste de l'ancienne marge
    // était du vide.
    // Les deux valeurs sont DÉRIVÉES, pas choisies : `OVERHEAD` est le `padding`
    // de `.menu` (voir ci-dessus), et `PER_BUTTON` est ensuite résolu pour que le
    // menu Alimentation (6 boutons + 1 séparateur) garde EXACTEMENT la hauteur
    // qu'il avait : `24 + 6P + 55 = 895` ⟹ `P = 136`. **Aucun grand menu ne
    // change donc de taille** — seuls les PETITS raccourcissent, et c'est
    // précisément là qu'était le vide (menu Contexte : 520 → 487, mesuré).
    const OVERHEAD: f64 = 24.0;
    const PER_BUTTON: f64 = 136.0;
    const PER_SEPARATOR: f64 = 55.0;
    // Voir la doc de fonction — `.section-title` jamais compté avant ce
    // paramètre, seul `assets/popups/context.html` en a un pour l'instant.
    const PER_SECTION_TITLE: f64 = 100.0;
    let css_scale_estimate = OVERHEAD
        + button_count as f64 * PER_BUTTON
        + separator_count as f64 * PER_SEPARATOR
        + section_title_count as f64 * PER_SECTION_TITLE;
    css_scale_estimate.round() as i32
}

#[cfg(test)]
mod button_list_height_tests {
    use super::button_list_height;

    /// Les deux points de calibration réels eux-mêmes (voir la doc de la
    /// fonction) — vérifie que la formule reste dans une marge raisonnable
    /// (ni tronquée, ni absurdement surdimensionnée) autour des mesures
    /// d'origine, à l'échelle neutre.
    /// **LES TROIS POINTS MESURÉS**, et une marge bornée DES DEUX CÔTÉS.
    ///
    /// Le plancher est ce qui protège du contenu tronqué ; le plafond est ce qui
    /// protège du **vide en bas** — et c'est le plafond qui manquait : à +150 px
    /// il laissait passer `OVERHEAD = 90`, qui faisait presque doubler la hauteur
    /// d'un menu à un bouton (215 px pour 114 px de contenu). Un test qui ne
    /// borne que par le bas ne voit pas ce défaut-là.
    #[test]
    fn matches_the_real_calibration_measurements_within_a_safe_margin() {
        // Les deux points de la calibration d'origine.
        for (label, estimate, measured) in [
            ("1 bouton", button_list_height(1, 0, 0), 114),
            ("6 boutons + 1 séparateur", button_list_height(6, 1, 0), 769),
            // Mesuré le 2026-09-19 par capture du menu Contexte (fenêtre 864×520,
            // dernier texte à y≈362 à `menu_scale = 0.9`).
            ("3 boutons + 1 séparateur", button_list_height(3, 1, 0), 402),
        ] {
            let measured = measured as f64;
            let estimate = estimate as f64;
            assert!(estimate >= measured, "{label} : ne doit jamais tronquer le contenu mesuré ({estimate} < {measured})");
            // **LE PLAFOND EST RELATIF, et il DOIT l'être** : les nombres mesurés
            // sont l'étendue du CONTENU (le dernier pixel d'encre), alors que
            // l'estimation est une hauteur de BOÎTE — elle doit couvrir en plus le
            // `padding` de `.menu`, celui des boutons et les jambages. Un plafond
            // en pixels absolus, essayé d'abord, refusait des valeurs VALIDES
            // (mesurées par capture) ; un plafond relatif attrape ce qu'il faut :
            // l'ancien `OVERHEAD = 90` faisait +89 % sur un menu à un bouton, et
            // c'est ce vide-là que ce test doit interdire.
            assert!(
                estimate <= measured * 1.5,
                "{label} : marge de sécurité exagérée, c'est du VIDE en bas du menu ({estimate} vs {measured})",
            );
        }
    }

    #[test]
    fn grows_with_more_buttons_and_separators() {
        assert!(button_list_height(1, 0, 0) < button_list_height(6, 0, 0));
        assert!(button_list_height(6, 0, 0) < button_list_height(6, 1, 0));
    }

    /// BUG RÉEL SIGNALÉ ("je n'arrive pas à sélectionner une fréquence" +
    /// "le menu a une barre de défilement", voir la doc de fonction) —
    /// un titre de section doit lui aussi agrandir la fenêtre, pas
    /// seulement les boutons/séparateurs.
    #[test]
    fn grows_with_section_titles() {
        assert!(button_list_height(10, 1, 0) < button_list_height(10, 1, 1));
    }
}

struct CefPopupState {
    hwnd: HWND,
    /// Position/taille écran ACTUELLE de la fenêtre (voir `toggle`, mise à
    /// jour à chaque repositionnement) — pour `gfx::backdrop::
    /// capture_blurred_batch`, PAS pour repositionner quoi que ce soit
    /// (`SetWindowPos` s'en charge, voir `toggle`).
    rect: [i32; 4],
    // `max_height` A ÉTÉ RETIRÉ le 2026-09-19, et c'est un progrès : c'était la
    // hauteur ALLOUÉE à l'ouverture, utilisée comme PLAFOND du redimensionnement
    // — or elle ne vaut que l'ESTIMATION de `button_list_height`, donc elle
    // interdisait à un menu de grandir au-delà, même avec la place disponible.
    // Il existait pour une seule raison : ce module n'avait aucun moyen de
    // connaître l'écran, et `app::menu_rect` le bornait à sa place. Depuis
    // que `win32::display::monitor_bounds` est accessible d'ici,
    // `resize_height_to_content` borne à la PLACE RÉELLE sous la fenêtre, qui est
    // la seule vraie contrainte — et ce champ n'a plus d'objet.
    swapchain: SwapChain,
    quad_renderer: QuadRenderer,
    browser: CefBrowserSurface,
    backdrop: Option<Texture>,
    /// `(largeur, hauteur, bits du `radius` f32)` déjà appliqués via
    /// `SetWindowRgn` dans `render` — voir son commentaire pour le pourquoi.
    /// `None` avant le premier appel. Sert uniquement à éviter de refaire
    /// `CreateRoundRectRgn`/`SetWindowRgn` à CHAQUE frame (`render` est
    /// appelée en continu tant que le popup est visible, voir
    /// `render_if_visible`) alors que la taille et le rayon ne changent
    /// quasiment jamais une fois le popup ouvert.
    applied_region: Option<(i32, i32, u32)>,
    /// Le compteur de peintures de la page déjà ANNONCÉ à `app::run` (voir
    /// `peintures_visibles`) — pour ne l'annoncer qu'une fois par image.
    peinture_vue: u64,
    /// Le dernier état de visibilité DIT À CHROMIUM (voir
    /// `CefBrowserSurface::set_visible`) — pour ne le lui dire qu'au changement.
    /// ⚠️ VRAI À LA CRÉATION : une page préchargée naît cachée, et c'est la
    /// première frame de `render_if_visible` qui doit le lui ANNONCER
    /// (`false != true`).
    visible_notifiee: bool,
    pending_show: Option<Instant>,
    /// **CETTE OUVERTURE NE DOIT PAS PRENDRE LE PREMIER PLAN.**
    ///
    /// Posé à CHAQUE ouverture (les deux branches de `toggle_with`) depuis
    /// `OUVERTURE_SANS_FOCUS`, et lu par les TROIS endroits qui montrent la
    /// fenêtre — dont le `ShowWindow` différé de `pending_show`, qui vit dans une
    /// autre fonction et ne pourrait pas lire une intention à usage unique.
    ///
    /// ⛔ **LA CARTE DE NOTIFICATION EST LE CAS QUI L'A FAIT NAÎTRE** (défaut
    /// signalé le 2026-10-09 : « la fenêtre des notifications prend le focus et à
    /// sa disparition le programme au premier plan n'est pas toujours celui qui
    /// était »). Elle n'a AUCUNE raison d'être activée : c'est une notification,
    /// pas un menu. Voir `set_ouverture_sans_focus`, qui porte la démonstration.
    sans_focus: bool,
    /// Recopié de `PopupSpec::above_a_magnified_dock` à la création — voir ce
    /// champ, qui porte la règle et départage les deux défauts réels.
    ///
    /// **Vit ici parce que `raise_visible_popups` n'a que le SLOT sous la main** :
    /// le registre retient des `LocalKey<PopupSlot>`, pas les specs.
    above_a_magnified_dock: bool,
    /// L'opacité animée de cette fenêtre — `None` pour les popups qui NE
    /// FONDENT PAS (les quatre PANNEAUX, voir `PopupSpec::fades`).
    ///
    /// **Un `Option` plutôt qu'un `bool` à consulter partout** : « ce popup ne
    /// fond pas » est alors une propriété qu'on ne peut pas oublier de tester
    /// avant d'animer, et l'absence est structurelle — même raison que le
    /// variant `PopupPalette::Menu` qui exclut les panneaux du zoom des menus.
    /// Il est `Some` si et seulement si la fenêtre a `WS_EX_LAYERED` (voir
    /// `create_window`), donc les deux ne peuvent pas diverger.
    fade: Option<fade::WindowFade>,
    /// Recopié de `PopupSpec::fenetre` — lu par `render` (pas de fond flouté, taille
    /// suivie sur la zone cliente) et par `visible_popup_hwnd` (qui l'écarte).
    fenetre: bool,
    /// **LA FENÊTRE A-T-ELLE DÉJÀ ÉTÉ MONTÉE ?** — voir `montrer_fenetre`, seul
    /// lecteur et seul écrivain.
    ///
    /// C'est ce qui distingue « sa vraie naissance » (le `rect` de l'appelant
    /// s'applique) de « on la rouvre » (la place appartient à l'utilisateur). Un
    /// `bool` et non une date : rien ici n'a besoin de savoir QUAND.
    montee: bool,
    /// **DÉCLARÉS EN DERNIER, ET C'EST PORTEUR** — un `Drop` de structure
    /// détruit ses champs dans l'ORDRE DE DÉCLARATION, donc ces deux copies du
    /// device doivent mourir APRÈS `swapchain`/`quad_renderer`/`browser`/
    /// `backdrop`, qui en sont tous des objets ENFANTS.
    ///
    /// **DÉFAUT RÉEL CORRIGÉ : l'application mourait à chaque fermeture quand un
    /// panneau était ouvert.** Ces copies maintiennent le device en vie plus
    /// longtemps que `app::run` (son `GfxDevice` n'en est plus que l'avant-
    /// dernière référence) : le device n'était donc détruit qu'à la fin du
    /// thread, en même temps que ce `thread_local!` — et comme il était déclaré
    /// AVANT ses enfants, il mourait le PREMIER, ses enfants encore vivants.
    /// D3D11/DXGI signale les objets vivants d'un device qu'on détruit par une
    /// exception `0x87A` (facility DXGI) que rien ne rattrape : sortie tuée net.
    /// Ne PAS « ranger » ces deux champs ailleurs dans la structure.
    device: ID3D11Device,
    context: ID3D11DeviceContext,
}

/// Tout l'état thread_local d'UN popup (une fenêtre) — chaque module popup
/// déclare son propre `thread_local! { static SLOT: PopupSlot = const {
/// PopupSlot::new() }; }`, jamais partagé entre popups (voir la doc de
/// module).
pub struct PopupSlot {
    state: RefCell<Option<CefPopupState>>,
    focus_lost_since: Cell<Option<Instant>>,
    had_focus: Cell<bool>,
    last_toggle: Cell<Option<Instant>>,
    /// `true` entre un `WM_LBUTTONDOWN` et le `WM_LBUTTONUP` correspondant
    /// (voir `dispatch_input`, où `SetCapture`/`ReleaseCapture` sont déjà
    /// posés au même endroit) — DEMANDE EXPLICITE ("les menus de
    /// redimensionnement ne doivent disparaître que quand je relâche la
    /// souris") : `win32::cef_bar_size`/`win32::cef_spacer_size`
    /// appliquent leur changement de taille EN TEMPS RÉEL pendant le
    /// glissement (voir `app::run`), et pour une barre SECONDAIRE ça
    /// déclenche `rebuild_secondary_docks` à chaque pas — recréation de
    /// fenêtres Win32 qui peut voler le premier plan un instant. Sans
    /// `dragging`, ce vol de focus transitoire finissait, après le délai
    /// `FOCUS_LOST_DEBOUNCE`, par masquer le popup EN PLEIN GLISSEMENT
    /// (bouton encore enfoncé). Consulté par `hide_if_focus_elsewhere`,
    /// qui ne masque jamais tant que `dragging` est vrai.
    dragging: Cell<bool>,
    /// Le dernier appui gauche, pour compter les clics — voir [`compter_clic`].
    clic: Cell<Clic>,
    /// Facteur de conversion **pixels CSS → pixels physiques** de ce popup, posé
    /// à chaque frame par `render` (bras `PopupPalette::Menu`).
    ///
    /// **`0.0` = « hauteur pilotée par la page INTERDITE »**, et c'est la valeur
    /// par défaut : les quatre PANNEAUX (Dash, Réglages, Presse-papiers, Notes)
    /// reçoivent un `PopupPalette::Window`, donc ce champ n'y est jamais posé et
    /// une action `menu/height/…` venue d'eux est ignorée. Leur hauteur est celle
    /// de la zone libre du dock, **pas** de leur contenu — et le plafond de
    /// `resize_height_to_content` ne les laisserait que RÉTRÉCIR. L'exclusion est
    /// donc **structurelle**, pas une convention qu'un `<script>` de trop
    /// casserait.
    content_factor: Cell<f64>,
    /// **LE PANNEAU EST-IL MAINTENU OUVERT ?** — la punaise (DEMANDE EXPLICITE :
    /// « un clic sur autre chose ne doit pas fermer un dash épinglé, sauf si on
    /// appelle un autre dash »).
    ///
    /// ⛔ **CE N'EST PAS `Epingler`.** Dans ce dépôt, « épingler » veut déjà dire
    /// « ajouter un DOSSIER à la barre du bas » (`config::ElementBarre::Dossier`,
    /// écrit dans `dossiers.toml`). Ici, c'est maintenir le PANNEAU ouvert, et
    /// **rien n'est écrit sur le disque** : la punaise vit le temps de la session.
    ///
    /// ⚠️ **MÊME FORME QUE `dragging`**, et pour la même raison : un booléen
    /// consulté par les fonctions qui décident de fermer. Trois lecteurs —
    /// `hide_if_focus_elsewhere` (la perte de focus), `toggle_with` (sa propre
    /// icône, qui est une bascule) et `hide_overlay_windows` (le clic sur la barre,
    /// via `est_epingle`).
    ///
    /// ⚠️ **IL NE S'OPPOSE PAS À TOUT** : le bouton `✖` d'une page ferme un panneau
    /// épinglé — c'est son seul geste de fermeture propre — et **ouvrir un autre
    /// dash** le ferme aussi (un seul dash à la fois, voir `toggle_with`). Une
    /// punaise qui résisterait à tout ne se déferait plus.
    epingle: Cell<bool>,
    /// **LE ZOOM DE CE POPUP SUIT-IL LE DPI DE SON ÉCRAN ?** — voir
    /// [`regler_zoom_dpi`], qui porte le pourquoi, la liste des trois inscrits, et
    /// ce qui arriverait si on l'activait pour les autres.
    ///
    /// `false` par défaut : la grande majorité des popups « fenêtre » portent DÉJÀ
    /// le ratio de DPI par un autre chemin, et le leur appliquer ici EN PLUS le
    /// doublerait.
    zoom_suit_le_dpi: Cell<bool>,
    /// **LA TAILLE DES TEXTES DE CE POPUP** — voir [`regler_zoom_textes`]. `1.0`
    /// (neutre) par défaut.
    facteur_textes: Cell<f32>,
    /// **L'IDENTITÉ PERSISTÉE DE CE POPUP** — le nom de sa page (voir
    /// `PopupSpec::html_file`), posé par `toggle_with` et `preload`, seuls
    /// chemins qui reçoivent la spec. C'est la clé de `tailles_menus.txt` (voir
    /// `taille_rappelee`) : `resize_to_content` n'a que le SLOT sous la main,
    /// et c'est pourtant lui qui enregistre la taille demandée par la page.
    identite: Cell<Option<&'static str>>,
    /// ⭐ **L'INSTANT DE L'OUVERTURE EN COURS** — pendant `MESURE_OUVERTURE_MAX`
    /// après lui, une taille annoncée par la page n'est appliquée qu'une fois
    /// CONFIRMÉE par une seconde annonce concordante (voir `appliquer_la_mesure`).
    /// `None` = aucune stabilisation en cours (ou terminée) : tout s'applique.
    ouverture_a: Cell<Option<Instant>>,
    /// La dernière taille annoncée pendant cette ouverture — la moitié
    /// « précédente » de la confirmation (voir `mesure_concordante`).
    mesure_precedente: Cell<Option<[i32; 2]>>,
    /// La dernière annonce NON confirmée, et son instant : c'est elle qui
    /// s'applique si aucune nouvelle n'est venue pendant `MESURE_SILENCE`.
    mesure_en_attente: Cell<Option<([i32; 2], Instant)>>,
    /// **LA MESURE D'OUVERTURE, DEMANDÉE DEUX FRAMES APRÈS LE MONTAGE** — voir
    /// `drain_actions`, qui décompte. ⛔ **LA DEMANDER DANS `toggle_with` LA FAIT
    /// MESURER AU ZOOM 3× DE LA PAGE CACHÉE**, et non au zoom du menu : `render`
    /// n'est appelé que pour une fenêtre VISIBLE, donc le zoom du menu n'est posé
    /// qu'à la frame qui suit le montage. Mesuré le 2026-11-10 sur
    /// `sound.html` : ouvert à 966 px, la page annonçait **322 unités CSS — la
    /// borne `max-width: 100vw` à 966/3 — et non son contenu**, si bien que
    /// chaque ouverture la rétrécissait de 30 % (966 → 676 → …), c'est-à-dire
    /// exactement « le menu n'apparaît pas tel qu'il est quand il a fini de se
    /// redimensionner ».
    mesure_differee: Cell<Option<u8>>,
}

/// **LE HARNAIS ÉPINGLE-T-IL TOUT CE QUI S'OUVRE ?** — voir
/// `NEURADESK_DEBUG_MAINTIEN`, doc de `win32::debug_capture`.
///
/// ⛔ **POURQUOI CE N'EST PAS UNE FONCTION D'UN MODULE PARTICULIER** : la punaise
/// est commune aux quinze panneaux, donc un levier qui viserait « le dash dossier »
/// ne prouverait rien des quatorze autres. Il faut épingler **celui que le harnais
/// ouvre**, quel qu'il soit — et c'est ici, au seul endroit qui voit TOUS les
/// popups s'ouvrir, qu'on peut le faire sans que chaque module sache qu'un harnais
/// existe.
///
/// ⚠️ **UN `AtomicBool` GLOBAL, PAS UN `Cell` PAR FIL** : la décision d'ouvrir peut
/// venir d'un autre fil que celui qui lit (le harnais ouvre depuis la boucle
/// principale, mais rien ne le garantit) — et un drapeau de test qui ne s'applique
/// pas est un test qui ne prouve rien.
static EPINGLER_A_L_OUVERTURE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Combien de pages VISIBLES ont peint une image neuve — voir
/// `peintures_visibles()`. Incrémenté par `render`, qui n'est appelé que pour
/// une fenêtre que `render_if_visible` a trouvée visible.
static PEINTURES_VISIBLES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Ce compteur ne dit qu'une chose, et c'est celle dont `app::run` a besoin
/// pour décider de PRÉSENTER : une page visible a une image neuve à l'écran.
/// ⚠️ Compter toutes les peintures serait faux — les treize pages
/// préchargées peignent aussi, fenêtres cachées, et leur compteur ne
/// redescend jamais (mesuré : ~70 peintures/s sans qu'aucune fenêtre ne soit
/// ouverte).
pub fn peintures_visibles() -> u64 {
    PEINTURES_VISIBLES.load(std::sync::atomic::Ordering::Relaxed)
}

/// Au moins un popup est-il VISIBLE (menu qui fond, panneau, fenêtre) ?
///
/// `app::run` s'en sert pour NE PAS s'endormir tant qu'un popup est à l'écran :
/// leurs fondus avancent une fois par frame (voir `render_if_visible`), et
/// bloquer la boucle les figerait. Passe par `REGISTRY`, le registre que tient
/// le module lui-même — pas une liste écrite à la main.
pub fn une_fenetre_visible() -> bool {
    REGISTRY.with(|registry| registry.borrow().iter().any(|slot| is_visible(slot)))
}

/// Au moins un MENU est-il VISIBLE ? — **ni un panneau, ni une vraie fenêtre**
/// (voir `PopupSpec::fades` : seuls les menus fondent, et c'est ce qui les
/// distingue ici, sans liste de modules à tenir à jour).
///
/// ⭐ **DEMANDE EXPLICITE (2026-11-11) : « quand un menu est affiché l'infobulle
/// de l'applet ne doit pas s'afficher ».** `app::run` s'en sert pour taire
/// l'infobulle d'une icône du dock tant qu'un menu est à l'écran. L'ancien
/// correctif ne suffisait pas : depuis « le menu passe toujours au-dessus des
/// infobulles » (voir `TooltipWindow::show`), l'infobulle s'insère SOUS le menu
/// — donc elle reste visible, posée juste à côté, et c'est elle que la demande
/// fait disparaître. Le sous-dock compte aussi (c'est un menu au sens de
/// `fades`, et il s'ouvre au survol d'un raccourci de dossier) : les deux ne
/// cohabitent donc plus.
///
/// ⚠️ **UN PANNEAU NE COMPTE PAS.** Les dashes et les applets ne fondent pas
/// (`PopupSpec::fades == false`) : ils laissent la place autour du dock, et
/// l'infobulle y reste l'explication utile des icônes.
///
/// Passe par `REGISTRY` comme `une_fenetre_visible` — aucune liste écrite à la
/// main, donc aucun popup à y inscrire.
pub fn un_menu_visible() -> bool {
    REGISTRY.with(|registry| {
        registry.borrow().iter().any(|slot| {
            slot.with(|s| {
                s.state
                    .borrow()
                    .as_ref()
                    .is_some_and(|state| state.fade.is_some() && unsafe { IsWindowVisible(state.hwnd).as_bool() })
            })
        })
    })
}

/// **LA PAGE `html_file` EST-ELLE À L'ÉCRAN ?** — demande explicite
/// (2026-11-11) : le fond au survol d'une icône du dock ne doit apparaître que
/// sur un programme ou une applet **fermés**. Une applet n'a pas d'état
/// « ouvert » central ; ce qui en tient lieu est la visibilité de SA page, et
/// le registre sait quelle page porte chaque popup (`PopupSlot::identite`,
/// posée à chaque ouverture).
///
/// ⚠️ **AUCUNE LISTE DE MODULES À TENIR** : l'appelant désigne la page par son
/// fichier (voir `app::PAGES_DES_APPLETS`), pas par un slot — un popup
/// préchargé ou caché ne compte donc pas (`IsWindowVisible`), et un module qui
/// ouvrirait la même page par un autre slot serait vu lui aussi.
///
/// Passe par `REGISTRY`, comme `un_menu_visible` juste au-dessus : un popup
/// nouveau s'y inscrit de lui-même, il n'y a rien à ajouter ici.
pub fn page_visible(html_file: &str) -> bool {
    REGISTRY.with(|registry| {
        registry
            .borrow()
            .iter()
            .any(|slot| slot.with(|s| s.identite.get() == Some(html_file)) && is_visible(slot))
    })
}

/// **ÉPINGLE TOUT CE QUI S'OUVRIRA** — réservé au harnais (voir
/// `EPINGLER_A_L_OUVERTURE`, où le pourquoi est écrit).
pub fn epingler_tout_a_l_ouverture() {
    EPINGLER_A_L_OUVERTURE.store(true, std::sync::atomic::Ordering::Relaxed);
}

thread_local! {
    /// Registre des popups DÉJÀ OUVERTS dans ce thread — voir `release_all`.
    ///
    /// Chaque module popup possède son propre `thread_local! SLOT` et il n'existe
    /// aucun registre central ; celui-ci les retrouve tous **sans qu'aucun module
    /// ait à s'y inscrire** : `toggle_with` — le seul chemin qui crée une
    /// fenêtre — y ajoute le slot la première fois qu'il le voit.
    static REGISTRY: RefCell<Vec<&'static LocalKey<PopupSlot>>> = const { RefCell::new(Vec::new()) };
}

/// **CE POPUP EST-IL UN PANNEAU ?** — ni un menu (qui fond), ni une vraie fenêtre.
///
/// ⚠️ **C'EST LA MÊME DISTINCTION QUE PARTOUT AILLEURS DANS CE DÉPÔT**, et elle
/// repose sur les deux drapeaux du `PopupSpec` plutôt que sur une liste de modules :
/// une liste écrite à la main serait à corriger au premier panneau ajouté, et le
/// dépôt a déjà payé ce défaut deux fois (voir `Dock::orders_mut`, et la note sur
/// l'énumération des barres).
fn est_un_panneau(spec: &PopupSpec) -> bool {
    !spec.fades && !spec.fenetre
}

/// **FERME TOUS LES AUTRES POPUPS VISIBLES** — « un seul dash à la fois ».
///
/// ⚠️ **ELLE FERME AUSSI LES ÉPINGLÉS**, et c'est le sens de la demande : la
/// punaise protège de la PASSIVITÉ (le focus qui s'en va, un clic à côté), pas d'un
/// remplacement délibéré. C'est ce qui garantit qu'une punaise se défait toujours.
///
/// Elle passe par le registre (`REGISTRY`) plutôt que par une liste de modules : le
/// registre existe déjà, il est tenu par les DEUX chemins qui créent une fenêtre, et
/// un popup qu'on y oublierait serait déjà un bug pour `release_all`.
fn fermer_les_autres(sauf: &'static LocalKey<PopupSlot>) {
    let autres: Vec<&'static LocalKey<PopupSlot>> = REGISTRY.with(|registry| {
        registry
            .borrow()
            .iter()
            .copied()
            .filter(|slot| !std::ptr::eq(*slot, sauf))
            .collect()
    });
    for slot in autres {
        if is_visible(slot) {
            // ⛔ **`fermer_par_geste` ET NON `hide_if_visible`** : remplacer un dash
            // par un autre défait la punaise, c'est le sens de « sauf si on appelle
            // un autre dash ».
            fermer_par_geste(slot);
        }
    }
}

/// Inscrit `slot` au registre de `release_all` s'il n'y est pas déjà.
///
/// **LES DEUX CHEMINS QUI CRÉENT UNE FENÊTRE DOIVENT L'APPELER** — `toggle_with`
/// et `preload`. C'est la raison d'être de cette fonction : tant qu'un seul
/// chemin créait des fenêtres, l'inscription pouvait vivre à l'intérieur de lui
/// sans qu'on puisse l'oublier ; le préchargement en a introduit un second, et un
/// popup créé par la voie qui n'inscrit pas reste HORS du registre, donc
/// `release_all` ne le libère pas — c'est très exactement le crash à la sortie
/// documenté sur `app::GpuCacheGuard` (le device D3D11 détruit alors que des
/// enfants CEF vivent encore). L'extraire rend l'oubli impossible à écrire.
fn register(slot: &'static LocalKey<PopupSlot>) {
    REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        if !registry.iter().any(|registered| std::ptr::eq(*registered, slot)) {
            registry.push(slot);
        }
    });
}

/// Libère TOUS les popups ouverts : fenêtre détruite, navigateur CEF relâché,
/// textures et swap chain libérés — à appeler AVANT la destruction du device
/// D3D11 **et** avant `cef_shutdown()`. Voir `app::GpuCacheGuard`, doc de type,
/// pour le défaut réel que cela corrige.
///
/// CEF exige que tous ses navigateurs soient détruits avant son propre arrêt,
/// et ces états vivent dans des `thread_local!`, donc détruits bien trop tard —
/// après le device, dont ils détiennent chacun une copie. Les libérer ici, dans
/// le bon ordre et de façon EXPLICITE, est la seule façon de satisfaire les
/// deux contraintes à la fois.
///
/// L'état est RETIRÉ (`take`) avant `DestroyWindow` : détruire la fenêtre peut
/// rappeler son wndproc de façon synchrone, et le laisser trouver un `RefCell`
/// encore emprunté serait la réentrance déjà rencontrée dans ce fichier (voir
/// `toggle_with`, doc).
/// Infobulle `title` de la page CEF SOUS LE CURSEUR, avec la position du curseur
/// (coordonnées écran) — `None` s'il n'y en a pas.
///
/// **Le popup doit être celui sous le curseur** (`WindowFromPoint`) : une page
/// qu'on vient de cacher, ou que le curseur a quittée sans que CEF ait encore
/// effacé son texte, ne doit plus rien afficher.
pub fn infobulle_de_page() -> Option<(String, POINT)> {
    let mut curseur = POINT::default();
    unsafe { GetCursorPos(&mut curseur).ok()? };
    let sous = unsafe { WindowFromPoint(curseur) };
    let texte = REGISTRY.with(|registry| {
        registry.borrow().iter().find_map(|slot| {
            slot.with(|s| {
                s.state.borrow().as_ref().filter(|state| state.hwnd == sous).and_then(|state| state.browser.latest_tooltip())
            })
        })
    })?;
    Some((texte, curseur))
}

/// HWND du popup CEF actuellement VISIBLE, s'il y en a un — `None` sinon.
///
/// **Sert à l'INFOBULLE** (`win32::tooltip::TooltipWindow::show`) : elle doit
/// rester AU-DESSUS du dock mais SOUS un menu ouvert, et `hWndInsertAfter` est le
/// seul moyen d'exprimer « juste en dessous de cette fenêtre » — aucun style ne
/// sait le dire.
///
/// **Le registre de `release_all` suffit**, et aucun module popup n'a à s'y
/// inscrire : `toggle_with` — le seul chemin qui crée une fenêtre — y ajoute le
/// slot la première fois qu'il le voit. Les entrées PÉRIMÉES sont écartées à la
/// lecture par `IsWindowVisible` (un popup masqué n'est pas détruit, il est
/// caché — voir `hide_if_visible`), et une fenêtre détruite rend faux. Le DERNIER
/// visible gagne : c'est celui ouvert en dernier, donc celui du dessus.
pub fn visible_popup_hwnd() -> Option<HWND> {
    let mut candidates: Vec<HWND> = Vec::new();
    REGISTRY.with(|registry| {
        for slot in registry.borrow().iter().rev() {
            // Une FENÊTRE (`PopupSpec::fenetre`) n'est pas topmost : placer
            // l'infobulle « juste sous elle » la ferait sortir de la bande topmost.
            if let Some(hwnd) = slot.with(|s| s.state.borrow().as_ref().filter(|state| !state.fenetre).map(|state| state.hwnd)) {
                candidates.push(hwnd);
            }
        }
    });
    candidates.into_iter().find(|&hwnd| unsafe { IsWindowVisible(hwnd).as_bool() })
}

/// **LE SLOT DONT LA FENÊTRE EST CE `hwnd`** — pour un module qui possède
/// PLUSIEURS fenêtres.
///
/// ⚠️ **CE DÉPÔT N'AVAIT JUSQU'ICI QU'UN POPUP PAR MODULE, DONC UN SEUL SLOT** :
/// chaque module déclare son `thread_local! { static SLOT }` et son `wndproc` le
/// nomme en clair. Un module à POOL (la visionneuse, quatre fenêtres) ne peut
/// pas faire ça — son `wndproc` reçoit un `hwnd` et doit retrouver LEQUEL. Cette
/// fonction est ce pont, et elle n'invente rien : `visible_popup_hwnd`
/// ci-dessus parcourt déjà le registre exactement de cette façon.
///
/// ⚠️ **ON COMPARE LE `hwnd` DE L'ÉTAT, PAS `IsWindow`** : un popup masqué garde
/// sa fenêtre (voir `hide_if_visible`), donc un créneau caché doit être
/// retrouvable — c'est justement le cas d'une visionneuse qu'on rouvre.
///
/// ⛔ **`try_borrow` ET NON `borrow` — ET ÇA A COÛTÉ UN PLANTAGE.** La première
/// version lisait l'état par `borrow()`, et le premier essai de la visionneuse a
/// tué l'application sur-le-champ :
///
///     panicked at src\win32\cef_popup.rs:558:22
///
/// Cette fonction est appelée **DEPUIS UN `wndproc`** — c'est même sa raison
/// d'être, un module à pool n'a pas d'autre moyen de retrouver son créneau — et
/// un `wndproc` s'exécute souvent **pendant** qu'un appelant tient déjà un
/// `borrow_mut` sur le même état (création de fenêtre, `SetWindowPos`, entrée
/// souris). `borrow()` panique alors, et le dock est compilé en `panic = "abort"` :
/// l'application MEURT, sans recours.
///
/// **`try_borrow` rend l'échec ordinaire** : un créneau momentanément emprunté
/// n'est simplement pas le bon, et la boucle continue. La doctrine est écrite
/// ailleurs dans ce fichier et dans `cef_couleurs::wndproc` — « dans un `wndproc`,
/// on ne touche que des `Cell` » ; ici on ne peut pas, alors on emprunte SANS
/// jamais paniquer.
pub fn slot_pour_hwnd(hwnd: HWND) -> Option<&'static LocalKey<PopupSlot>> {
    REGISTRY.with(|registry| {
        // ⚠️ **`try_borrow` ICI AUSSI** : `register` prend un `borrow_mut` sur ce
        // même registre, et il est appelé pendant la création d'une fenêtre — donc
        // exactement quand un `wndproc` peut être rappelé.
        let Ok(inscrits) = registry.try_borrow() else { return None };
        inscrits.iter().copied().find(|slot| {
            slot.with(|s| {
                s.state
                    .try_borrow()
                    .map(|etat| etat.as_ref().is_some_and(|state| state.hwnd == hwnd))
                    .unwrap_or(false)
            })
        })
    })
}

/// **DÉMARRE UN DÉPLACEMENT NATIF** d'une `fenetre`, comme le ferait sa barre de
/// titre si elle en avait une — le seul chemin des deux modules qui en ont besoin
/// (`win32::cef_couleurs`, `win32::cef_visionneuse`).
///
/// ⚠️ **`lParam` PORTE LA POSITION ÉCRAN DU CURSEUR** : c'est le contrat de
/// `WM_NCLBUTTONDOWN`, et les deux copies d'origine y passaient `0`.
///
/// ⛔ **LE DÉPLACEMENT NE MARCHAIT PAS** (2026-10-01) : la trace dit si le bouton
/// était encore enfoncé quand l'action est arrivée (elle fait un aller-retour par
/// le processus de rendu), combien de temps la boucle de déplacement a tenu, et si
/// le cadre a bougé. `SendMessageW` ne rend la main qu'à la FIN de cette boucle.
pub fn deplacer_par_le_systeme(hwnd: HWND, qui: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, HTCAPTION, WM_NCLBUTTONDOWN};
    let mut curseur = POINT::default();
    let mut avant = RECT::default();
    let mut apres = RECT::default();
    unsafe {
        let _ = GetCursorPos(&mut curseur);
        let _ = GetWindowRect(hwnd, &mut avant);
    }
    let bouton = unsafe { GetAsyncKeyState(0x01) } as u16 & 0x8000 != 0;
    let position = ((curseur.y as u16 as u32) << 16) | curseur.x as u16 as u32;
    let debut = Instant::now();
    unsafe {
        let _ = ReleaseCapture();
        SendMessageW(hwnd, WM_NCLBUTTONDOWN, Some(WPARAM(HTCAPTION as usize)), Some(LPARAM(position as i32 as isize)));
        let _ = GetWindowRect(hwnd, &mut apres);
    }
    crate::diag!(
        "[{qui}/deplacer] bouton enfoncé={bouton} curseur=({},{}) boucle={} ms cadre ({},{}) -> ({},{})",
        curseur.x,
        curseur.y,
        debut.elapsed().as_millis(),
        avant.left,
        avant.top,
        apres.left,
        apres.top
    );
}

/// Repasse DEVANT les MENUS actuellement visibles, dans l'ordre du registre (le
/// dernier inscrit finit au-dessus, comme à l'ouverture).
///
/// **LES MENUS SEULEMENT — les quatre PANNEAUX en sont exclus**, et c'est le
/// filtre qui départage deux demandes explicites qui se contredisaient tant que
/// cette fonction remontait tout : voir `PopupSpec::above_a_magnified_dock`, qui
/// porte la règle et les deux défauts réels qu'elle répare — les menus sous les
/// barres d'un côté, les icônes agrandies sous le Dash de l'autre.
///
/// **Appelée après tout `Window::raise_to_top` du dock** — DÉFAUT RÉEL SIGNALÉ :
/// « il y a encore des menus de widget qui apparaissent sous les barres ». Le
/// dock se replace en tête de la bande topmost dès qu'un élément est AGRANDI
/// (`app::run`, « les icônes agrandies repassent au premier rang »), donc un menu
/// ouvert pendant qu'on survole un widget passait DERRIÈRE la barre.
///
/// `IsWindowVisible` écarte les entrées périmées du registre (une fenêtre cachée
/// n'est pas détruite), et l'appel est SANS déplacement, redimensionnement ni
/// activation — même idiome que `Window::raise_to_top`.
///
/// ⭐ **PENDANT UNE MAGNIFICATION, LES MENUS PASSENT SOUS LE DOCK** — DEMANDE
/// EXPLICITE (2026-11-11) : « l'effet de magnification doit faire apparaître les
/// icônes au-dessus des ombrages, des dashs et des menus ». La règle d'avant
/// (« menus devant le dock agrandi ») est donc INVERSÉE, et seulement pendant
/// ce moment-là : le dock est déjà en tête (`Window::raise_to_top`), il suffit
/// de glisser les menus JUSTE SOUS lui (`ombre_barre::ancre_de_magnification`,
/// le même ancrage que les ombres — deux ancres les feraient sauter d'un rang à
/// l'autre). Au repos, ils repassent en tête : voir `app::run`, qui rappelle
/// `ombre_barre::remonter` quand la magnification retombe.
pub fn raise_visible_popups() {
    let mut hwnds: Vec<HWND> = Vec::new();
    REGISTRY.with(|registry| {
        for slot in registry.borrow().iter() {
            // LE FILTRE EST LE CŒUR DE CETTE FONCTION — voir
            // `PopupSpec::above_a_magnified_dock`, qui porte la règle et les deux
            // défauts réels qu'elle départage (menus devant, panneaux derrière).
            if let Some(hwnd) = slot.with(|s| {
                s.state.borrow().as_ref().filter(|state| state.above_a_magnified_dock).map(|state| state.hwnd)
            }) {
                hwnds.push(hwnd);
            }
        }
    });
    // L'ancre LUE UNE FOIS : un dock par écran, un seul rang à choisir.
    let sous = if crate::ui::dock::magnification_en_cours() {
        crate::win32::ombre_barre::ancre_de_magnification()
    } else {
        None
    };
    for hwnd in hwnds {
        unsafe {
            if IsWindowVisible(hwnd).as_bool() {
                let _ = SetWindowPos(
                    hwnd,
                    Some(sous.unwrap_or(HWND_TOPMOST)),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                );
            }
        }
    }
}

/// Réaffirme le rang TOPMOST d'un popup qui vient d'être MONTRÉ — pour qu'il passe
/// au-dessus de l'infobulle, qui est topmost elle aussi.
///
/// **POURQUOI CE N'ÉTAIT PAS LE CAS** : toutes ces fenêtres sont `WS_EX_TOPMOST`
/// (dock, popups, infobulle) et, entre elles, l'ordre réel dépend de QUI s'est
/// positionné en DERNIER — pas du style. L'infobulle réaffirme le sien à chaque
/// apparition (`win32::tooltip`), exprès, pour passer au-dessus du dock ; un menu
/// ouvert APRÈS elle restait donc dessous, caché par elle. DÉFAUT RÉEL SIGNALÉ :
/// « quand un élément affiche une infobulle, le menu associé se trouve masqué par
/// l'infobulle ».
///
/// **Appelée après `ShowWindow`**, et pas avant : c'est l'apparition qu'il faut
/// faire passer devant, et une fenêtre encore cachée n'a pas de rang à défendre.
/// Les appels de POSITION (`SetWindowPos` avec `SWP_NOZORDER` juste avant) restent
/// intacts — cet ajout ne fait que replacer la fenêtre en tête de sa bande, sans
/// déplacement ni redimensionnement, comme `Window::raise_to_top`.
///
/// ⭐ **PENDANT UNE MAGNIFICATION, UN MENU SE POSE SOUS LE DOCK** — DEMANDE
/// EXPLICITE (2026-11-11) : « l'effet de magnification doit faire apparaître les
/// icônes au-dessus des ombrages, des dashs et des menus ». Sans cette branche,
/// le menu ouvert par le clic sur l'icône survolée resterait devant elle pendant
/// la frame qui suit (`raise_visible_popups` ne le range qu'à la frame
/// suivante). `menu` vient de `PopupSpec::above_a_magnified_dock` : un PANNEAU
/// garde son chemin, `sous_le_dock` s'en occupant déjà.
fn raise_above_tooltips(hwnd: HWND, menu: bool) {
    let sous = if menu && crate::ui::dock::magnification_en_cours() {
        crate::win32::ombre_barre::ancre_de_magnification()
    } else {
        None
    };
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(sous.unwrap_or(HWND_TOPMOST)),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}

/// **LE VERBE DE `ShowWindow` SELON QUE LA FENÊTRE DOIT PRENDRE LE PREMIER PLAN.**
///
/// `SW_SHOW` ACTIVE la fenêtre ; `SW_SHOWNOACTIVATE` la montre sans y toucher.
///
/// ⚠️ **UNE SEULE DÉFINITION, TROIS SITES** (les deux chemins de `toggle_with` et
/// le `ShowWindow` DIFFÉRÉ de `pending_show`, qui vit dans une autre fonction) :
/// trois littéraux recopiés auraient dérivé, et c'est précisément la moitié
/// oubliée qui aurait laissé le défaut vivant — un troisième chemin de show qu'on
/// ne pense pas à corriger.
fn verbe_de_show(sans_focus: bool) -> SHOW_WINDOW_CMD {
    if sans_focus {
        SW_SHOWNOACTIVATE
    } else {
        SW_SHOW
    }
}

pub fn release_all() {
    REGISTRY.with(|registry| {
        let slots: Vec<&'static LocalKey<PopupSlot>> = registry.borrow().clone();
        for slot in slots {
            let taken = slot.with(|s| s.state.borrow_mut().take());
            if let Some(state) = taken {
                let hwnd = state.hwnd;
                // Les ressources du device d'abord (voir `CefPopupState`, dont
                // les champs `device`/`context` sont déclarés en DERNIER à
                // dessein)…
                drop(state);
                // …puis la fenêtre, qui n'a plus d'état à retrouver.
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
        }
        registry.borrow_mut().clear();
    });
}

impl PopupSlot {
    pub const fn new() -> Self {
        Self {
            state: RefCell::new(None),
            focus_lost_since: Cell::new(None),
            had_focus: Cell::new(false),
            last_toggle: Cell::new(None),
            dragging: Cell::new(false),
            clic: Cell::new(Clic { instant: 0, x: 0, y: 0, nombre: 0 }),
            content_factor: Cell::new(0.0),
            epingle: Cell::new(false),
            zoom_suit_le_dpi: Cell::new(false),
            facteur_textes: Cell::new(1.0),
            identite: Cell::new(None),
            ouverture_a: Cell::new(None),
            mesure_precedente: Cell::new(None),
            mesure_en_attente: Cell::new(None),
            mesure_differee: Cell::new(None),
        }
    }
}

/// **DIT QUE LE ZOOM DE CE POPUP DOIT SUIVRE LE DPI DE SON ÉCRAN.**
///
/// À appeler par un module « fenêtre » **que le ratio de DPI n'atteint par aucun
/// autre canal** : ni un `contentScale` POUSSÉ à sa page (les panneaux — terminal,
/// météo, SSH…), ni un `SCREEN_SCALE_OVERRIDE` (Dash, Presse-papiers, Réglages).
/// Pour ceux-là le ratio arrive déjà, et le poser ici **en plus** le multiplierait
/// deux fois.
///
/// ⚠️ **C'EST LE FAIT D'ALIMENTER QUI COMPTE, PAS LA PRÉSENCE DU CSS** : la
/// plupart des pages portent `--text-scale` (`zoom: var(--text-scale)` + `width:
/// calc(100% / var(--text-scale))`), y compris `notes.html` et `sous_dock.html`.
/// Ce qui les distingue des panneaux, c'est que leur module ne pousse AUCUN
/// `contentScale` — vérifié pour les trois inscrits. Le zoom est donc le seul
/// levier qui les atteint.
///
/// **LES TROIS INSCRITS** : `cef_notes`, `cef_image_editor`, `cef_sous_dock` — les
/// seuls popups « fenêtre » qui peuvent s'ouvrir sur un écran secondaire sans le
/// moindre autre levier. ⚠️ Les applets à géométrie calculée en Rust
/// (`cef_caracteres`, `cef_kaomoji`, `cef_visionneuse`) s'ouvrent TOUJOURS sur la
/// primaire : leur ratio vaut 1.0, elles n'ont rien à demander.
///
/// ⚠️ **CE N'EST PAS UN DRAPEAU DE CONFORT** : sans lui, ces trois fenêtres
/// gardent la même taille PHYSIQUE partout, donc paraissent plus grandes sur un
/// écran à 100 % que sur la primaire — le défaut que ce lot corrige.
pub fn regler_zoom_dpi(slot: &'static LocalKey<PopupSlot>, suit: bool) {
    slot.with(|s| s.zoom_suit_le_dpi.set(suit));
}

/// **POSE LA TAILLE DES TEXTES D'UN POPUP « FENÊTRE » À GÉOMÉTRIE CALCULÉE EN
/// RUST** (kaomojis, caractères, phonétique, maths) — DEMANDE EXPLICITE
/// (2026-10-18) : « que ça s'applique aux kaomojis et autres fenêtres de ce type ».
/// Elles ne passent pas par `app::content_scale_pour` : leur module pose ici, à
/// CHAQUE ouverture, le facteur avec lequel il vient de tailler la fenêtre
/// (`taille_panneaux::TEXTES`), et le zoom de la page le suit. Le prendre à
/// l'ouverture, et non à chaque frame, garde la page et sa fenêtre d'accord quand
/// le curseur bouge pendant qu'elle est ouverte.
///
/// ⚠️ **UNE FENÊTRE DÉJÀ MONTÉE GARDE SA PLACE** (voir `faut_appliquer_le_rect`) :
/// `rect` ne lui serait plus appliqué. Si le facteur a changé depuis, elle prend
/// donc la taille de `rect` autour de son centre actuel — sans quoi la page
/// grandirait dans une fenêtre restée à l'ancienne taille.
pub fn regler_zoom_textes(slot: &'static LocalKey<PopupSlot>, facteur: f32, rect: [i32; 4]) {
    let avant = slot.with(|s| s.facteur_textes.replace(facteur));
    if avant == facteur {
        return;
    }
    let Some(hwnd) = hwnd(slot) else { return };
    let mut actuel = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut actuel) }.is_err() {
        return;
    }
    let [_, _, l, h] = rect;
    let (cx, cy) = ((actuel.left + actuel.right) / 2, (actuel.top + actuel.bottom) / 2);
    let _ = unsafe { SetWindowPos(hwnd, None, cx - l / 2, cy - h / 2, l, h, SWP_NOZORDER | SWP_NOACTIVATE) };
}

/// **LA TAILLE D'UNE FENÊTRE À GÉOMÉTRIE RUST, À LA TAILLE DES TEXTES** — voir
/// [`regler_zoom_textes`]. `taille` calcule pour un zoom ×3 sur un écran donné ;
/// à un zoom ×3 × `f`, tout grandit de `f` — contenu, marges ET plafonds — sauf
/// l'écran. On taille donc pour un écran `f` fois plus petit, puis on agrandit :
/// les plafonds (`PART_ECRAN`) restent ceux du VRAI écran.
pub(crate) fn taille_aux_textes(largeur: i32, hauteur: i32, taille: fn(i32, i32) -> [i32; 2]) -> [i32; 2] {
    taille_a_l_echelle(largeur, hauteur, crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES), taille)
}

fn taille_a_l_echelle(largeur: i32, hauteur: i32, f: f32, taille: fn(i32, i32) -> [i32; 2]) -> [i32; 2] {
    let reduit = |v: i32| (v as f32 / f).round() as i32;
    taille(reduit(largeur), reduit(hauteur)).map(|v| (v as f32 * f).round() as i32)
}

/// Identité FIXE d'un popup — nom de classe/titre Win32 (doivent être
/// uniques par popup, voir `RegisterClassExW`), nom du fichier HTML (voir
/// `create_window`, résolu sous `assets/popups/`), et le wndproc PROPRE à
/// ce popup (voir sa doc de champ : ne peut pas être partagé).
pub struct PopupSpec {
    pub class_name: PCWSTR,
    pub window_title: PCWSTR,
    pub html_file: &'static str,
    /// Ne peut pas être partagé entre popups : chaque popup a son propre
    /// `thread_local! { static SLOT }`, et un wndproc générique ici n'aurait
    /// aucun moyen de savoir lequel utiliser (un pointeur de fonction Win32
    /// ne transporte pas de contexte). Chaque module popup définit donc son
    /// propre `unsafe extern "system" fn` minimal qui appelle
    /// `dispatch_input(&SLOT, ...)` puis `DefWindowProcW` — voir
    /// `win32::cef_test_window` pour un exemple concret.
    pub wndproc: WNDPROC,
    /// `true` si ce popup appartient à la famille des MENUS — voir
    /// `raise_visible_popups` et `raise_above_tooltips`, seules fonctions qui
    /// lisent ce champ, à travers `CefPopupState`.
    ///
    /// **Les neuf MENUS : `true`.** Un menu s'ancre à l'icône qu'on survole : le
    /// dock passé devant lui, il disparaîtrait derrière la barre. DÉFAUT RÉEL
    /// SIGNALÉ, et c'est ce champ qui le répare : « il y a encore des menus de
    /// widget qui apparaissent sous les barres ».
    ///
    /// **Les PANNEAUX (Dash, Réglages, Presse-papiers, Notes, applets) :
    /// `false`.** Ils couvrent toute la zone libre, donc les remonter cache
    /// EXACTEMENT ce que la demande d'origine veut protéger — « je voudrais que
    /// les icônes agrandies soient toujours visibles, que rien ne puisse les
    /// recouvrir ». DÉFAUT RÉEL SIGNALÉ : « le bug des icônes et applets
    /// grandifiées sous les dashs est réapparu » — la remontée de tous les
    /// popups sans distinction (commit `28c53cc`, livré NON MESURÉ) avait
    /// échangé un défaut contre l'autre.
    ///
    /// ⭐ **CE QUI A CHANGÉ LE 2026-11-11** : DEMANDE EXPLICITE — « l'effet de
    /// magnification doit faire apparaître les icônes au-dessus des ombrages,
    /// des dashs et des menus ». Pendant la magnification, les menus passent
    /// donc EUX AUSSI sous le dock (voir `raise_visible_popups` et
    /// `raise_above_tooltips`) ; cette famille reste ce qui les distingue,
    /// puisqu'eux se posent juste sous le dock, les panneaux restant sous tout
    /// le monde (`sous_le_dock`). Au repos, l'ordre d'avant est intact :
    /// **menus > ombres > dock > panneaux**.
    pub above_a_magnified_dock: bool,
    /// `true` si ce popup APPARAÎT ET DISPARAÎT EN FONDANT — voir `win32::fade`.
    ///
    /// **Les neuf MENUS : `true`.** DEMANDE EXPLICITE : « le fondu des
    /// infobulles et des menus ». Un menu s'ouvre sous un clic et se referme
    /// sous un clic ailleurs : c'est le va-et-vient que le fondu adoucit, et
    /// c'est aussi ce qui rend lisible qu'un menu en REMPLACE un autre quand
    /// `app::hide_overlay_windows` ferme le précédent dans la même frame.
    ///
    /// **Les quatre PANNEAUX : `false`.** Ils ne sont pas concernés par la
    /// demande, et leur disparition n'a pas la même nature : un panneau se
    /// ferme par son propre bouton ou parce que le dock reprend la main, et il
    /// laisse derrière lui un état (saisie en cours, défilement) qu'un fondu
    /// retarderait sans rien apporter. Les y mettre est une ligne à changer
    /// dans leur module, pas une refonte.
    ///
    /// **LA VALEUR EST LUE À LA CRÉATION DE LA FENÊTRE, et elle seule décide
    /// du style `WS_EX_LAYERED`** (voir `create_window`) : le champ est donc
    /// recopié dans `CefPopupState::fade` plutôt que relu depuis le `PopupSpec`,
    /// que `hide_if_visible`/`render_if_visible` n'ont pas sous la main.
    pub fades: bool,
    /// `true` pour une **VRAIE FENÊTRE d'application** — l'inverse d'un popup sur
    /// tous les points : `WS_EX_APPWINDOW` (barre des tâches et Alt-Tab), **ni
    /// topmost ni toolwindow**, pas de verre DWM ni de fond flouté (la page est
    /// opaque), et sa taille est celle de la ZONE CLIENTE, relue à chaque frame
    /// par `render`.
    ///
    /// ⛔ **CE N'EST PAS `WS_OVERLAPPEDWINDOW`, ET CE COMMENTAIRE L'A AFFIRMÉ À
    /// TORT** : `create_window` pose **`WS_POPUP`** pour une fenêtre (voir le bras
    /// `spec.fenetre`). Il n'y a donc **NI barre de titre, NI bordure, NI bouton
    /// d'agrandissement** — et **ça change une chose qu'un lecteur doit savoir** :
    /// le système ne fournit PLUS le redimensionnement par les bords, ni le
    /// déplacement par la barre de titre. **Chaque module qui met `fenetre` à
    /// `true` DOIT les réimplémenter dans son `wndproc`** — c'est ce que fait
    /// `win32::cef_couleurs` (`WM_NCHITTEST` → `HTLEFT`…`HTBOTTOMRIGHT`, et
    /// `HTCAPTION` pour le déplacement). Sans ça, la fenêtre est FIGÉE à sa taille
    /// et à sa place de création, et rien ne le dit.
    ///
    /// **La taille est SUIVIE, pas imposée** : c'est l'utilisateur qui la change
    /// en tirant un bord, donc `render` compare `GetClientRect` au swap chain et
    /// redimensionne les deux surfaces. Le faire dans `WM_SIZE` exposerait à la
    /// réentrance (le message arrive pendant `SetWindowPos`, sous un emprunt).
    ///
    /// Ouvrir une fenêtre passe par `montrer_fenetre`, jamais par `toggle` :
    /// cliquer l'applet d'une fenêtre ouverte la RAMÈNE devant, il ne la ferme pas.
    ///
    /// ⛔ **LA SECONDE ERREUR DE CE COMMENTAIRE : il affirmait « plus aucun
    /// `PopupSpec` ne le met à `true` ».** C'était vrai un temps, ça ne l'est plus
    /// depuis `win32::cef_couleurs` (le Sélecteur de couleurs), et le lot des
    /// **quatre visionneuses** (`win32::cef_visionneuse`) en ajoute quatre de
    /// plus. Un commentaire qui compte les utilisateurs d'un champ se périme tout
    /// seul — il vaut mieux décrire ce que le champ FAIT, et laisser le compte.
    pub fenetre: bool,
}

/// Ouvre/ferme/repositionne la fenêtre à `rect` — bascule RÉELLE (masque si
/// déjà visible, montre + repositionne sinon). Voir `win32::cef_test_window::
/// toggle` (doc historique complète : pourquoi cette fenêtre existe, lien
/// avec `PopupKind` egui qu'elle remplace) pour le contexte d'un appelant
/// concret.
pub fn toggle(slot: &'static LocalKey<PopupSlot>, spec: &PopupSpec, rect: [i32; 4], gfx: &GfxDevice) {
    toggle_with(slot, spec, rect, gfx, || {});
}

/// Comme `toggle`, mais appelle `on_show` juste AVANT `force_foreground` —
/// SEULEMENT sur la transition vers VISIBLE (jamais sur la bascule vers
/// masqué, jamais si la fenêtre était déjà visible). Pour les popups qui
/// doivent préparer leur état avant de voler le focus — ex.
/// `win32::cef_clipboard`, qui doit capturer `GetForegroundWindow()` (voir
/// sa doc, "coller" a besoin de savoir où redonner le premier plan) et
/// pousser un état frais AVANT que `force_foreground` ne change justement
/// ce que `GetForegroundWindow()` renverrait.
/// `rect` porte la POSITION et la hauteur à laquelle la fenêtre APPARAÎT. **Le
/// plafond du redimensionnement n'est plus un paramètre** : `resize_to_content`
/// le recalcule à chaque appel (la place sous la fenêtre), et le temps où il
/// fallait le passer — parce qu'il valait la hauteur allouée, donc l'estimation —
/// est révolu (voir sa doc).
///
/// **Le retirer a aussi retiré son piège** : confondre le plafond avec la hauteur
/// demandée faisait naître le menu Contexte à la taille de son sous-menu DÉPLIÉ
/// (2397 px MESURÉS, plus haut que l'écran) pour le rétrécir dix frames plus tard,
/// une fois la page chargée — un saut de 2000 px en plein visage. La hauteur
/// demandée doit être celle du cas COURANT (le sous-menu replié).
pub fn toggle_with(slot: &'static LocalKey<PopupSlot>, spec: &PopupSpec, rect: [i32; 4], gfx: &GfxDevice, on_show: impl FnOnce()) {
    // ⛔ **CONSOMMÉE ICI, ET TOUJOURS — MÊME AVALÉE PAR L'ANTI-REBOND.** Une
    // intention à usage unique qui survivrait à l'ouverture qu'elle visait
    // s'appliquerait à la SUIVANTE, qui n'a rien demandé : c'est la fenêtre
    // d'après qui ne prendrait plus le premier plan, sans que rien ne le dise.
    // Voir `set_ouverture_sans_focus`.
    let sans_focus = prendre_intention_sans_focus();
    let origine = prendre_origine_ouverture();
    let now = Instant::now();
    let too_soon = slot.with(|s| s.last_toggle.get().is_some_and(|last| now.duration_since(last) < TOGGLE_DEBOUNCE));
    if too_soon {
        return;
    }
    slot.with(|s| s.last_toggle.set(Some(now)));

    // Inscription au registre (voir `release_all` et `register`, dont la doc
    // porte le pourquoi) : ce point est traversé par TOUT popup qu'on ouvre, qu'il
    // soit créé maintenant ou simplement réaffiché — et par `preload`, qui est
    // l'autre chemin qui crée une fenêtre.
    register(slot);
    // L'identité persistée de ce popup — la clé de `tailles_menus.txt`, voir
    // `taille_rappelee`. Posée à CHAQUE ouverture, pas à la création : c'est le
    // seul chemin qui reçoit la spec à coup sûr.
    slot.with(|s| s.identite.set(Some(spec.html_file)));

    let already_exists = slot.with(|s| s.state.borrow().is_some());
    if already_exists {
        let currently_visible =
            slot.with(|s| s.state.borrow().as_ref().is_some_and(|state| unsafe { IsWindowVisible(state.hwnd).as_bool() }));
        // **UNE FENÊTRE EN COURS DE DISPARITION COMPTE COMME FERMÉE**, et ce
        // n'est pas un détail de confort : `IsWindowVisible` reste vrai pendant
        // tout le fondu de sortie (voir `win32::fade`), donc sans cette ligne
        // un second clic sur l'icône — pendant les 150 ms où le menu s'en va —
        // serait lu comme « il est déjà ouvert, refermons-le », c'est-à-dire
        // comme un clic MORT. Le cas est atteignable par un usage parfaitement
        // ordinaire : ouvrir, fermer, rouvrir aussitôt.
        let closing = slot.with(|s| s.state.borrow().as_ref().is_some_and(fade_state_is_closing));
        let was_open = currently_visible && !closing;
        // ⛔ **UNE PUNAISE NE SE BASCULE PAS.** Re-cliquer l'icône d'un dash épinglé
        // ne le referme pas : DEMANDE EXPLICITE, « un clic sur autre chose ne doit
        // pas fermer un dash épinglé ». Le `✖` de sa page reste son geste de
        // fermeture propre — et l'ouverture d'un autre dash, juste en dessous.
        //
        // ⚠️ **APRÈS `last_toggle`** (posé plus haut), pour que l'anti-rebond
        // continue de s'appliquer : sans lui, un double-clic sur l'icône ferait
        // deux fois le tour de la boucle pour rien.
        if was_open && slot.with(|s| s.epingle.get()) {
            return;
        }
        if !was_open {
            // ⭐ **UN SEUL DASH À LA FOIS — C'EST DU COMPORTEMENT NEUF.**
            // Jusqu'ici, ouvrir un dash n'en fermait aucun : les bras d'ouverture
            // d'`app.rs` appellent `cef_xxx::toggle` sans passer par
            // `hide_overlay_windows`, donc deux panneaux pouvaient se superposer
            // sur la même zone libre. La demande est explicite : « sauf si on
            // appelle un autre dash ».
            //
            // ⚠️ **LA RESTRICTION AUX PANNEAUX EST DÉLIBÉRÉE** : un MENU qui
            // s'ouvre ne doit rien fermer — il vit sa vie ancré à son icône, et
            // c'est l'inverse (un panneau qui s'ouvre balaie ce qui traîne) qui
            // est voulu.
            if est_un_panneau(spec) {
                fermer_les_autres(slot);
            }
            // Le harnais : épingler CE panneau, quel qu'il soit (voir
            // `EPINGLER_A_L_OUVERTURE`).
            if EPINGLER_A_L_OUVERTURE.load(std::sync::atomic::Ordering::Relaxed) {
                slot.with(|s| s.epingle.set(true));
                crate::diag!("[popup] punaise posee par le harnais a l'ouverture");
            }
            on_show();
        }
        // `hwnd` récupéré puis le `RefCell` RELÂCHÉ avant tout appel Win32
        // (`SetWindowPos`/`ShowWindow`/`force_foreground`) — BUG RÉEL
        // rencontré ici (pas dans le spike, en usage réel : réouvrir Sound/
        // Réseau/Voicemeeter une SECONDE fois plantait avec "RefCell
        // already mutably borrowed") : ces trois appels Win32 peuvent
        // envoyer WM_SETFOCUS/WM_KILLFOCUS SYNCHRONEMENT au wndproc de
        // CETTE MÊME fenêtre avant de retourner (voir `dispatch_input`,
        // cas `WM_SETFOCUS | WM_KILLFOCUS`) — réentrance garantie si le
        // `RefCell` de `state` était encore emprunté à ce moment-là (voir
        // la mémoire du projet sur cette classe de bug, déjà rencontrée
        // pour `SetWindowRgn`). La première ouverture (`create_window`,
        // plus bas) n'y était jamais exposée : `state` y est encore une
        // variable LOCALE, pas stockée dans le `RefCell`, au moment des
        // mêmes appels Win32.
        // ⭐ **LA TAILLE DE LA DERNIÈRE OUVERTURE, QUAND ON LA CONNAÎT** —
        // DEMANDE EXPLICITE (2026-11-10) : « serait-il possible de pré calculer
        // les menus, je les vois se redimensionner quand je les ouvre ». C'est
        // cette ligne qui répond à la demande : au lieu de partir de
        // l'ESTIMATION d'`app::menu_rect` (calibrée, mais jamais exacte) et
        // d'attendre que la page annonce la vraie taille une ou deux frames plus
        // tard — en plein fondu de 500 ms —, on rouvre à la taille que la page a
        // DEMANDÉE la dernière fois. Plus de saut du tout pour un menu dont le
        // contenu n'a pas changé, qui est le cas courant.
        //
        // ⚠️ **CE N'EST PAS UNE MESURE PÉRIMÉE RÉUTILISÉE TELLE QUELLE** : la
        // taille est ramenée à l'échelle CSS ACTUELLE par le rapport des
        // `content_factor`, donc bouger « Taille des menus » ou « Taille des
        // textes » entre deux lancements la fait suivre au lieu de la figer.
        //
        // ⚠️ **ET LA MESURE RESTE DEMANDÉE À CHAQUE OUVERTURE** (voir
        // `request_content_measure` plus bas) : si le contenu a changé — le mois
        // du calendrier, la liste des sorties audio — la page corrige la taille,
        // exactement comme avant. On ne fait que partir du bon pied.
        //
        // ⚠️ **LUE SUR DISQUE AUSSI** (`tailles_menus.txt`) : sans ça, la
        // PREMIÈRE ouverture de chaque menu après un lancement repartait de
        // l'estimation et sautait — et c'est justement ce que l'utilisateur
        // voyait le plus souvent, l'application démarrant avec Windows.
        let rect = if was_open { rect } else { taille_rappelee(slot, rect) };
        let hwnd = slot.with(|s| {
            let mut state_ref = s.state.borrow_mut();
            let state = state_ref.as_mut()?;
            if !was_open {
                // Voir `app::menu_rect` : la taille demandée pour un
                // popup "menu" peut avoir changé depuis sa dernière
                // ouverture (curseur "Taille des menus" bougé PENDANT que ce
                // popup précis était fermé) — BUG RÉEL SIGNALÉ ("le curseur
                // Taille/arrondi des menus ne redimensionne toujours aucun
                // popup CEF") : `SetWindowPos` plus bas redimensionne bien
                // le HWND Win32, mais ni le swapchain D3D11 ni la surface
                // CEF importée ne suivaient — le contenu restait rendu à
                // L'ANCIENNE taille dans une fenêtre de la NOUVELLE,
                // laissant une bande vide ou tronquant le contenu. Comparer
                // AVANT d'écraser `state.rect`, et redimensionner ici,
                // PENDANT que `state` est encore emprunté — `resize` ne fait
                // que des appels D3D11/CEF internes (voir sa doc), aucun des
                // trois appels Win32 dangereux (`SetWindowPos`/`ShowWindow`/
                // `force_foreground`) qui imposent de relâcher l'emprunt
                // avant de les exécuter (voir le commentaire juste en
                // dessous).
                if state.rect[2] != rect[2] || state.rect[3] != rect[3] {
                    let _ = state.swapchain.resize(&state.device, rect[2].max(1) as u32, rect[3].max(1) as u32);
                    state.browser.resize(rect[2].max(1), rect[3].max(1));
                    state.pending_show = Some(Instant::now());
                }
                state.rect = rect;
                // ⭐ **NOUVELLE OUVERTURE : la stabilisation repart.** Tant que
                // la page se reconstruit, ses annonces ne seront crues qu'une
                // fois concordantes (voir `appliquer_la_mesure`).
                s.ouverture_a.set(Some(Instant::now()));
                s.mesure_precedente.set(None);
                s.mesure_en_attente.set(None);
                // La mesure partira d'elle-même, zoom posé (voir `mesure_differee`).
                s.mesure_differee.set(Some(MESURE_DIFFEREE_FRAMES));
                // Plus de plafond à reposer ici : `resize_to_content` borne
                // désormais à la PLACE SOUS LA FENÊTRE, qu'il recalcule à chaque
                // appel (voir sa doc). C'est ce qui a permis de retirer
                // `max_height` — et avec lui le piège qui consistait à le
                // confondre avec la hauteur demandée, laquelle vaut la taille du
                // sous-menu REPLIÉ et aurait interdit tout dépliage.
            }
            Some(state.hwnd)
        });
        if let Some(hwnd) = hwnd {
            unsafe {
                if was_open {
                    // La fenêtre s'en va — d'un coup si elle n'a pas de fondu
                    // (les panneaux), sinon en s'éteignant : c'est `tick`, appelé
                    // par `render_if_visible`, qui la cachera pour de bon une
                    // fois l'opacité à zéro (voir `begin_hide`).
                    begin_hide(hwnd, slot);
                } else {
                    let [x, y, w, h] = rect;
                    // ⚠️ **`SWP_NOACTIVATE` QUAND LA FENÊTRE NE DOIT PAS PRENDRE LE
                    // PREMIER PLAN** : `SetWindowPos` active par défaut.
                    let drapeaux = if sans_focus { SWP_NOZORDER | SWP_NOACTIVATE } else { SWP_NOZORDER };
                    let _ = SetWindowPos(hwnd, None, x, y, w, h, drapeaux);
                    if !closing {
                        animer_l_ouverture(spec, hwnd, origine);
                    }
                    if !slot.with(|s| s.state.borrow().as_ref().is_some_and(|st| st.pending_show.is_some())) {
                        let _ = ShowWindow(hwnd, verbe_de_show(sans_focus));
                    }
                    // Le fondu d'apparition : `restart` pour une fenêtre qui
                    // était cachée (l'opacité repart de zéro), `fade_in` pour
                    // celle qu'on rouvre PENDANT sa disparition (elle repart de
                    // l'opacité où elle en est, sinon elle clignoterait — voir
                    // `WindowFade::restart`). Puis l'opacité est posée TOUT DE
                    // SUITE : la fenêtre vient d'être créée `LAYERED` et porte
                    // encore l'alpha opaque par défaut, donc sans ce `push` elle
                    // se composerait entière pendant la frame qui précède le
                    // premier pas du fondu.
                    //
                    // L'emprunt est RELÂCHÉ avant `raise_above_tooltips`/
                    // `force_foreground` ci-dessous, qui peuvent dispatcher des
                    // messages au wndproc de cette même fenêtre (voir le
                    // commentaire plus haut) : ce bloc ne fait que des écritures
                    // de champs et un appel d'attribut de fenêtre, aucun des
                    // trois appels Win32 dangereux.
                    slot.with(|s| {
                        let mut state_ref = s.state.borrow_mut();
                        if let Some(state) = state_ref.as_mut() {
                            // ⚠️ **REPOSÉ À CHAQUE OUVERTURE** : c'est une propriété
                            // de CETTE ouverture, pas de la fenêtre — le bandeau
                            // ouvre tantôt une carte (sans focus), tantôt la liste
                            // (avec). Le `ShowWindow` différé de `pending_show` le
                            // relira, dans une autre fonction.
                            state.sans_focus = sans_focus;
                            if let Some(window_fade) = state.fade.as_mut() {
                                if closing {
                                    window_fade.fade_in();
                                } else {
                                    window_fade.restart();
                                }
                                window_fade.push(state.hwnd);
                            }
                        }
                    });
                    // APRÈS `ShowWindow` — voir `raise_above_tooltips`.
                    raise_above_tooltips(hwnd, spec.above_a_magnified_dock);
                    // ⛔ **C'EST ICI QUE LE PREMIER PLAN SE PRENAIT, ET C'EST LA
                    // SEULE MOITIÉ QUI SE CORRIGE.** Sans cet appel, la fenêtre est
                    // montrée non activée (`SW_SHOWNOACTIVATE`, ci-dessus) : le
                    // premier plan ne quitte jamais l'application de l'utilisateur,
                    // et il n'y a donc rien à rendre quand la carte s'efface — ce
                    // que Windows ne sait de toute façon pas faire.
                    if !sans_focus {
                        force_foreground(hwnd);
                        slot.with(|s| {
                            if let Some(state) = s.state.borrow().as_ref() {
                                state.browser.set_focus(true);
                            }
                        });
                    }
                    // Réouverture : la page est DÉJÀ chargée, donc sa mesure part
                    // vraiment — c'est le cas qui rattrape une action perdue et
                    // rafraîchit un menu qui ne mute pas (voir la doc). Ce qu'elle
                    // annonce pendant que la page se reconstruit n'est pas cru
                    // (voir `appliquer_la_mesure`).
                    //
                    // ⚠️ **L'APPEL ÉTAIT ANNONCÉ PAR CE COMMENTAIRE SANS ÊTRE
                    // LÀ** — le texte décrivait l'intention, le code ne demandait
                    // rien : un menu rouvert après un changement de contenu
                    // (mois du calendrier, sorties audio) gardait donc sa taille
                    // rappelée jusqu'à ce que la page se mesure d'elle-même.
                    //
                    // ⚠️ **ET ELLE NE SE DEMANDE PAS ICI** : à cet instant le zoom
                    // du menu n'est pas encore posé (voir `mesure_differee`, qui
                    // compte les frames).
                }
            }
        }
        if !was_open {
            slot.with(|s| {
                s.had_focus.set(false);
                s.focus_lost_since.set(None);
            });
        }
        return;
    }
    if let Some(mut state) = create_window(spec, gfx, rect) {
        // ⚠️ **POSÉ AVANT LE PREMIER `ShowWindow`** — c'est `create_window` qui
        // l'initialise à `false`, et cette branche est celle de la PREMIÈRE
        // ouverture : sans cette ligne, une carte de notification ouverte avant
        // que la file de préchargement ne l'atteigne prendrait le premier plan une
        // fois sur deux (voir `set_ouverture_sans_focus`).
        state.sans_focus = sans_focus;
        animer_l_ouverture(spec, state.hwnd, origine);
        unsafe {
            let _ = ShowWindow(state.hwnd, verbe_de_show(sans_focus));
        }
        // PREMIÈRE OUVERTURE — le fondu part de zéro, et l'opacité est posée
        // dans la foulée pour la même raison qu'à la réouverture ci-dessus
        // (voir le commentaire du `push`). `state` est encore une variable
        // LOCALE ici, donc aucun emprunt n'est en jeu.
        if let Some(window_fade) = state.fade.as_mut() {
            window_fade.restart();
            window_fade.push(state.hwnd);
        }
        // La PREMIÈRE ouverture aussi : une fenêtre qui naît topmost passe
        // normalement devant, mais c'est ici que le cas « infobulle déjà à
        // l'écran » se produit le plus souvent — on survole l'icône, l'infobulle
        // apparaît, on clique. Voir `raise_above_tooltips`.
        raise_above_tooltips(state.hwnd, spec.above_a_magnified_dock);
        // ⛔ **LES DEUX MÊMES RÈGLES QU'À LA RÉOUVERTURE, ET ELLES MANQUAIENT ICI.**
        // Cette branche est celle de la PREMIÈRE ouverture — un popup non encore
        // préchargé, ce qui est le cas de TOUT panneau qu'on ouvre avant que la file
        // de préchargement ne l'atteigne (elle démarre à la 30ᵉ frame). Sans ces
        // lignes, le premier dash ouvert ne fermait rien d'autre, et le harnais ne
        // pouvait pas l'épingler : la punaise n'était posée que dans l'autre branche.
        if est_un_panneau(spec) {
            fermer_les_autres(slot);
        }
        if EPINGLER_A_L_OUVERTURE.load(std::sync::atomic::Ordering::Relaxed) {
            slot.with(|s| s.epingle.set(true));
            crate::diag!("[popup] punaise posee par le harnais (premiere ouverture)");
        }
        on_show();
        // Voir le bras de réouverture : la carte de notification ne prend NI le
        // premier plan NI le clavier, et l'historique prend les deux.
        if !sans_focus {
            force_foreground(state.hwnd);
            state.browser.set_focus(true);
        }
        // Première ouverture d'un popup NON préchargé : la page charge encore,
        // donc une mesure immédiate ne dirait rien (`window.__ndMeasure` n'existe
        // pas encore) — c'est son écouteur `load` qui mesurera, et la mesure
        // DIFFÉRÉE (voir `mesure_differee`) qui reprendra le relais au cas où.
        // ⛔ **PAS D'APPEL IMMÉDIAT ICI** : page montée, il tournerait AVANT que
        // `render` n'ait posé le zoom du menu, donc au zoom 3× de la page cachée
        // — exactement le défaut mesuré sur `sound.html`.
        slot.with(|s| {
            s.had_focus.set(false);
            s.focus_lost_since.set(None);
            // La stabilisation de CETTE ouverture (voir `appliquer_la_mesure`).
            s.ouverture_a.set(Some(Instant::now()));
            s.mesure_precedente.set(None);
            s.mesure_en_attente.set(None);
            s.mesure_differee.set(Some(MESURE_DIFFEREE_FRAMES));
            *s.state.borrow_mut() = Some(state);
        });
    }
}

/// Crée la fenêtre ET le navigateur d'un popup **sans jamais l'afficher** — pour
/// que sa page ait fini de charger avant la première ouverture réelle.
///
/// **POURQUOI.** La création était jusqu'ici entièrement paresseuse : `toggle_with`
/// créait fenêtre, swap chain, `QuadRenderer` (qui compile le HLSL à l'exécution)
/// et navigateur au moment du clic, puis montrait aussitôt une page qui n'avait
/// pas fini de charger. C'est la cause des bugs visuels de première ouverture, et
/// aussi de la classe « il faut ouvrir deux fois les menus pour qu'ils soient
/// remplis » : le premier état poussé partait dans le vide, avant que le
/// `<script>` de la page n'ait défini ses fonctions. Une page déjà chargée rend
/// ces deux symptômes impossibles.
///
/// **LA PAGE SE MESURE AUSSI PENDANT QU'ELLE EST CACHÉE, ET CETTE MESURE-LÀ EST
/// IGNORÉE** — voir `drain_actions`, qui n'applique une taille qu'à une fenêtre
/// visible : le plafond de `resize_to_content` est « la place sous et à droite de
/// la fenêtre », ce qui ne veut rien dire tant que le popup n'est pas à sa place
/// définitive. C'est donc l'OUVERTURE qui redemande une mesure, et **forcée**
/// (voir `request_content_measure`, où le piège de la garde de déduplication est
/// expliqué) : sans ce forçage, la mesure prise ici serait la seule jamais
/// annoncée, et le popup s'ouvrirait à son estimation.
///
/// **CE QU'ELLE NE FAIT PAS, ET C'EST AUSSI IMPORTANT** : aucun `ShowWindow`,
/// aucun `force_foreground`, aucun `set_focus` — les trois appels Win32 que
/// `toggle_with` réserve à l'ouverture réelle (voir sa doc, qui explique aussi
/// pourquoi ils imposent de relâcher tout emprunt avant de les exécuter). Les
/// faire ici volerait le premier plan au démarrage.
///
/// **`register` EST OBLIGATOIRE ICI** — voir sa doc : c'est le second chemin qui
/// crée une fenêtre, et un popup qu'il oublie de déclarer n'est pas libéré par
/// `release_all` (c'est le crash à la sortie documenté sur `app::GpuCacheGuard`).
pub fn preload(slot: &'static LocalKey<PopupSlot>, spec: &PopupSpec, gfx: &GfxDevice) {
    if slot.with(|s| s.state.borrow().is_some()) {
        return;
    }
    let Some(state) = create_window(spec, gfx, PRELOAD_RECT) else { return };
    register(slot);
    // L'identité est aussi posée ICI : un popup préchargé peut se mesurer (et
    // donc enregistrer sa taille) avant sa première ouverture, et le harnais de
    // capture n'ouvre que quelques popups.
    slot.with(|s| {
        s.identite.set(Some(spec.html_file));
        *s.state.borrow_mut() = Some(state);
    });
}

/// Taille de naissance d'un popup préchargé — voir `preload`, doc de fonction.
///
/// **Elle n'est JAMAIS MONTRÉE**, et la mesure que la page y prend est ignorée :
/// la première ouverture repositionne et redimensionne de toute façon. Sa valeur
/// n'a donc que deux exigences, et c'est la seconde qui a fixé le chiffre :
/// - être assez grande pour qu'une page se mette en page sans être écrasée (une
///   page comprimée à quelques dizaines de pixels CSS pourrait se replier d'une
///   façon qui n'a rien à voir avec son usage réel) ;
/// - **ne pas être grande pour autant** : le swap chain ET la texture CEF sont
///   alloués à cette taille, pour treize popups, pendant toute la vie de
///   l'application. C'est la mémoire qui a fixé le plafond, pas la mise en page.
///
/// ⛔ **ET ELLE NAÎT HORS DE TOUT ÉCRAN — UNE FENÊTRE CACHÉE Y MANGEAIT DES CLICS.**
/// Défaut réel signalé le 2026-10-12 (« quelque chose dans l'app m'empêche de
/// cliquer sur mes programmes en haut à gauche, cela fait un moment ») : à
/// `[0, 0, …]`, les ~35 fenêtres préchargées et jamais montrées s'empilaient dans
/// le coin haut-gauche, et le clic SIMPLE y était perdu par l'Explorateur,
/// WhatsApp, Mobile connecté et le Terminal (le double-clic et le clic maintenu
/// passaient ; Firefox n'était pas touché). Mesuré par élimination : ni la fenêtre
/// du dock, ni les réservations AppBar, ni les deux ensemble ; fermer l'app
/// débloquait, et **déplacer ces seules fenêtres cachées hors de l'écran, l'app
/// tournant, a débloqué**. Une fenêtre CACHÉE ne devrait recevoir aucune entrée :
/// le mécanisme, côté Windows, n'est PAS élucidé — seul le remède est mesuré.
/// Aucune échelle n'en dépend : `create_window` ne lit pas le DPI de l'écran sous
/// la fenêtre, et la première monte apporte de toute façon la vraie place.
pub const PRELOAD_RECT: [i32; 4] = [-32000, -32000, 1024, 768];

/// **UNE POSITION GARÉE** — à gauche ET au-dessus de tout écran plausible (même
/// seuil que le test de `PRELOAD_RECT`). C'est là que vont les fenêtres cachées.
fn est_garee(x: i32, y: i32) -> bool {
    x < -16000 && y < -16000
}

/// **LA PLACE OÙ RAMENER UNE FENÊTRE GARÉE** — `Some((x, y))` de sa dernière place
/// visible si elle est garée, `None` si elle est déjà sur un écran (rien à faire :
/// la place appartient à l'utilisateur). Pure, pour être vérifiable sans fenêtre.
fn place_de_retour(position_actuelle: (i32, i32), derniere_place: [i32; 4]) -> Option<(i32, i32)> {
    let (x, y) = position_actuelle;
    (est_garee(x, y) && !est_garee(derniere_place[0], derniere_place[1])).then_some((derniere_place[0], derniere_place[1]))
}

/// **CACHE UNE FENÊTRE ET LA GARE HORS DE TOUT ÉCRAN.**
///
/// ⛔ **DÉFAUT SIGNALÉ LE 2026-11-01** : « lorsque j'essaie d'utiliser l'explorateur
/// Windows avec NeuraDesk ouvert, certains de mes clics gauches ne fonctionnent
/// pas ». `PRELOAD_RECT` ne couvrait que les fenêtres JAMAIS montrées : une fenêtre
/// ouverte puis refermée restait CACHÉE À SA PLACE — neuf relevées, dont les
/// Réglages et le Dossier en 2409×1319 sur presque tout l'écran. Même remède que
/// pour le préchargement, mesuré de la même façon : déplacer ces seules fenêtres
/// cachées hors écran, l'app tournant, a rendu tous les clics.
///
/// Les menus et les panneaux reçoivent leur place à chaque ouverture
/// (`toggle_with`) ; une FENÊTRE d'applet est ramenée à sa dernière place par
/// `montrer_fenetre` (voir `place_de_retour`).
fn cacher_et_garer(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
        let _ = SetWindowPos(
            hwnd,
            None,
            PRELOAD_RECT[0],
            PRELOAD_RECT[1],
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}

/// Repositionne/redimensionne la fenêtre si `rect` diffère de la dernière
/// taille connue (no-op bon marché sinon) — pour les popups dont la taille
/// suit un contenu externe qui peut changer PENDANT qu'ils sont ouverts
/// (ex. `win32::cef_clipboard`, qui remplit la "zone libre" du dock — voir
/// `app::dash_rect` — recalculée à chaque frame). Contrairement à `toggle`
/// (repositionnement fait une seule fois, à l'OUVERTURE), à appeler à
/// CHAQUE FRAME (voir `win32::cef_clipboard::render_if_visible`), avant le
/// rendu — redimensionne le swap chain ET la texture CEF importée
/// (`CefBrowserSurface::resize`, jusqu'ici sans appelant), pas seulement la
/// fenêtre Win32 elle-même.
pub fn resize_if_needed(slot: &'static LocalKey<PopupSlot>, rect: [i32; 4]) {
    slot.with(|s| {
        let mut state_ref = s.state.borrow_mut();
        let Some(state) = state_ref.as_mut() else { return };
        // UN POPUP CACHÉ N'EST PAS REDIMENSIONNÉ. La mesure qui justifie cette
        // ligne : sans elle, un panneau PRÉCHARGÉ et JAMAIS MONTRÉ se fait
        // redimensionner — swap chain comprise, pas seulement la fenêtre — à
        // l'échelle du dock dès le lancement, et une fenêtre topmost portant une
        // surface de cette taille suffit à mettre WhatsApp (WebView2) en défaut :
        // copier-coller mort, clic et survol sans effet, sa fenêtre restant
        // pourtant active. La seconde moitié du correctif (`pending_show`, voir
        // `toggle_with`) répare le noir que cette ligne fait apparaître, en
        // différant la montre d'une frame le temps que la page se remette en page.
        if !unsafe { IsWindowVisible(state.hwnd).as_bool() } {
            return;
        }
        if state.rect == rect {
            return;
        }
        let [x, y, w, h] = rect;
        state.rect = rect;
        unsafe {
            let _ = SetWindowPos(state.hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        }
        let _ = state.swapchain.resize(&state.device, w.max(1) as u32, h.max(1) as u32);
        state.browser.resize(w.max(1), h.max(1));
    });
}

/// Ajuste la HAUTEUR de la fenêtre à `target_h` (px physiques), pour les
/// popups "menu" dont le contenu HTML varie RÉELLEMENT (nombre d'éléments
/// affichés, pas juste une mise en page à hauteur fixe/connue d'avance —
/// voir `win32::cef_voicemeeter`, doc de module, seul appelant actuel) —
/// BUG RÉEL SIGNALÉ ("le menu voicemeeter... ne semble pas se redimensionner
/// automatiquement en hauteur") : la fenêtre reste sinon figée à la hauteur
/// allouée à l'ouverture (voir `app::menu_rect`), bien plus grande que
/// ce qu'il faut pour une installation Voicemeeter "Standard" (peu de
/// tranches/bus) ou une fois des canaux masqués.
///
/// `target_h` calculé côté RUST (voir `win32::cef_voicemeeter::
/// estimated_content_height`), PAS mesuré côté JS — UN PREMIER ESSAI
/// mesurait `scrollHeight` en JS et le remontait par une action IPC
/// (`neuradesk://voicemeeter/content-height/<ratio>`, voir `go()`) : BUG
/// RÉEL TROUVÉ EN VÉRIFIANT PAR CAPTURE — cette action ne redimensionnait
/// JAMAIS la fenêtre, même après plusieurs centaines de frames, alors que
/// les AUTRES actions du même popup (gain/mute/bus, empruntant le même
/// canal `neuradesk://…`) fonctionnent normalement en usage réel. Cause la
/// plus probable (non confirmée avec certitude, aucune console capturée
/// dans ce setup — voir la doc du projet sur la vérification des API non
/// documentées) : CES actions-là sont TOUJOURS déclenchées par un geste
/// utilisateur réel (`input`/`click`), la mienne partait d'un
/// `execute_js` Rust→JS (`window.setState`) sans le moindre geste associé —
/// Rust dispose déjà, dans `cef_voicemeeter::push_state`, de tout ce qu'il
/// faut (nombre de tranches/bus VISIBLES) pour estimer cette hauteur
/// lui-même, sans repasser par un aller-retour JS dont la fiabilité est en
/// doute dans ce contexte précis.
///

/// Calcule la nouvelle hauteur (px physiques) pour `resize_to_content`
/// — `None` si `target_h`, une fois borné à `[1, max_height]`, est identique
/// à `current_h` (rien à faire). Extraite en fonction PURE exprès pour être
/// testable sans fenêtre/GPU — voir les tests ci-dessous.
/// Hauteur (en pixels CSS) qu'une page menu annonce pour son contenu, extraite
/// d'une action `menu/height/<px>` — `None` si l'action n'est pas de cette
/// famille ou si la valeur n'est pas exploitable.
///
/// **PURE et BORNÉE, et ce n'est pas de la prudence gratuite** : `parse::<f64>`
/// accepte `"nan"`, `"inf"`, `"-5"` et `"1e30"` — et `"nan" as i32` vaut **0**,
/// soit une fenêtre d'UN pixel (le plancher de `clamped_content_height`). Une page
/// ne devrait jamais envoyer ça, mais la seule chose qui l'en empêche est ce
/// filtre, et il est testé.
///
/// Le plafond de 20 000 px est très au-delà de tout écran réel : il borne la
/// conversion en `i32` qui suit.
/// Taille NATURELLE du contenu d'un menu, annoncée par sa page :
/// `menu/size/<largeur>/<hauteur>`, en pixels CSS.
///
/// **LES DEUX DIMENSIONS DANS UNE SEULE ACTION, et ce n'est pas un détail de
/// forme** : deux `location.href` émis dans le même tick se TÉLESCOPENT (le
/// second annule la navigation du premier, et une action peut s'y perdre — voir
/// `win32::cef_app::NeuraDeskRequestHandler`). Les envoyer séparément perdrait
/// donc une dimension une fois sur deux, en silence.
///
/// La largeur vient de la même demande que la hauteur : « le menu du wifi est
/// trop large par rapport à son contenu », 1380 px pour deux boutons.
fn parse_content_size(action: &str) -> Option<(f64, f64)> {
    let rest = action.strip_prefix("menu/size/")?;
    let (w, h) = rest.split_once('/')?;
    Some((bounded_css_px(w)?, bounded_css_px(h)?))
}

/// Le champ d'une action, borné — **la seule définition de ce qu'on accepte**.
fn bounded_css_px(value: &str) -> Option<f64> {
    let value: f64 = value.parse().ok()?;
    (value.is_finite() && (0.0..=20_000.0).contains(&value)).then_some(value)
}

/// Convertit une hauteur CSS en pixels **physiques** — **`ceil`, jamais `round`**.
///
/// Le facteur vaut `CONTENT_ZOOM_FACTOR × menu_scale × content_scale`, donc
/// rarement entier (2,7 sur cette machine) : arrondir vers le BAS d'une fraction
/// de pixel suffit à rendre la boîte plus courte que son contenu, donc à faire
/// apparaître la **barre de défilement fantôme** — DÉFAUT MESURÉ PAR CAPTURE
/// (colonnes x=849..851 couvertes sur 366 des 374 lignes du menu Contexte,
/// quand deux réglages voisins donnaient 374 et 395).
fn content_height_px(css_px: f64, factor: f64) -> i32 {
    (css_px * factor).ceil().max(1.0) as i32
}

/// Ordonnée d'une fenêtre dont la HAUTEUR change — voir `resize_to_content`,
/// dont c'est toute la décision. PURE, donc testable sans fenêtre ni GPU, comme
/// `clamped_content_height` juste en dessous.
///
/// **Deux ancrages, et le bas gagne quand il est au bord** : un menu ancré sous
/// une icône tient à elle par son bord haut, donc `y` ne bouge pas. Mais un menu
/// que `app::menu_rect` a REMONTÉ (il débordait par le bas) a son bord bas
/// posé au pixel près sur celui de l'écran — c'est donc son BAS qui le rattache à
/// son icône, et garder le haut fixe l'en détacherait en rétrécissant, d'un écart
/// qui grandit à chaque ouverture.
/// **LA ZONE LIBRE DU MENU COURANT** — posée par `app::menu_rect`, qui est le
/// SEUL chemin de placement d'un menu (voir sa doc : les dix sites d'ouverture
/// réelle ET les dix du harnais passent par lui), et relue ici.
///
/// ⚠️ **POURQUOI ELLE DOIT ÊTRE CONNUE ICI.** `menu_rect` place la fenêtre pour
/// la taille **PROVISOIRE** qu'on lui passe ; la page annonce ensuite sa taille
/// RÉELLE, et `resize_to_content` agrandit la fenêtre **depuis son coin
/// haut-gauche**. Un menu ancré près de la barre de DROITE a donc son bord droit
/// posé **exactement sur la limite de la zone libre**, et il grandissait vers la
/// droite — borné par l'ÉCRAN, donc **par-dessus la barre de droite**.
///
/// DÉFAUT SIGNALÉ : « le menu au clic droit sur un séparateur d'une zone de la
/// barre de droite recouvre la barre de droite ; j'aimerais qu'il soit décalé à
/// gauche de la barre ».
///
/// Un menu ouvert sans passer par `menu_rect` (aucun aujourd'hui) laisse la zone
/// à zéro, et `resize_to_content` retombe alors sur l'écran — le comportement
/// d'avant.
thread_local! {
    static MENU_ROOM: std::cell::Cell<[i32; 4]> = const { std::cell::Cell::new([0, 0, 0, 0]) };
    /// **LE MENU QU'ON VIENT DE PLACER EST-IL CENTRÉ HORIZONTALEMENT ?**
    /// Même motif que `MENU_ROOM` juste au-dessus, même raison : `resize_to_content`
    /// n'a aucun paramètre pour l'apprendre, et le placement est le seul à le savoir.
    /// `false` par défaut, donc les menus ordinaires gardent leur comportement.
    static MENU_CENTRE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// **L'INTENTION DE LA PROCHAINE OUVERTURE : NE PAS PRENDRE LE PREMIER PLAN.**
    /// Même motif que les deux ci-dessus — le site d'appel est le seul à savoir ce
    /// qu'il ouvre, et `toggle_with` n'a que le SLOT et le SPEC. Consommée (remise
    /// à `false`) par `toggle_with`, À CHAQUE APPEL, même avalé par l'anti-rebond.
    static OUVERTURE_SANS_FOCUS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// **OUVRIR SANS PRENDRE LE PREMIER PLAN — pour la carte de notification.**
///
/// DÉFAUT SIGNALÉ (2026-10-09) : « la fenêtre des notifications prend le focus et
/// à sa disparition le programme au premier plan n'est pas toujours celui qui
/// était au premier plan ».
///
/// **LES DEUX MOITIÉS DE CE DÉFAUT SONT LE MÊME DÉFAUT**, et c'est ce qui rend le
/// correctif unique : `force_foreground` (appelé sans condition à l'ouverture)
/// attache les files d'entrée, appelle `SetForegroundWindow` **et** `SetFocus` —
/// la carte prend donc le premier plan ET le clavier. Quant à sa disparition,
/// elle ne peut rien rendre : **Windows ne réattribue pas le premier plan quand
/// une fenêtre au premier plan se cache** (fait déjà payé sur ce dépôt, voir
/// `counts_as_own_foreground`). D'où « pas toujours celui qui était » : le premier
/// plan retombe sur ce que Windows trouve ensuite dans l'ordre Z.
///
/// ⇒ **La seule correction qui règle les deux : ne pas le prendre.** Une
/// notification qui volerait le clavier pendant qu'on écrit serait de toute façon
/// inacceptable ; le bandeau remplace les toasts de Windows, qui ne le prennent
/// jamais non plus.
///
/// ⚠️ **L'HISTORIQUE, LUI, LE PREND** (clic droit sur l'horloge) : c'est un menu,
/// il attend Échap et des clics. C'est le `MODE` qui tranche, à l'ouverture —
/// d'où une intention par ouverture plutôt qu'un champ de `PopupSpec`, qui est le
/// même pour les deux usages (voir `bandeau::ouvrir`).
pub fn set_ouverture_sans_focus(sans_focus: bool) {
    OUVERTURE_SANS_FOCUS.with(|cell| cell.set(sans_focus));
}

/// **PREND L'INTENTION — ET LA CONSOMME.** Une intention posée vaut pour UNE
/// ouverture, jamais deux : c'est ce que le `replace(false)` garantit.
///
/// ⚠️ **EXTRAITE POUR ÊTRE VÉRIFIABLE, ET ELLE EN VAUT LA PEINE** : sa panne est
/// MUETTE. Une intention qui survivrait à l'ouverture qu'elle visait ferait
/// ouvrir le popup SUIVANT sans premier plan — un menu qui n'attrape pas le
/// clavier, sans erreur, sans journal, et sans rapport visible avec la
/// notification qu'on venait d'afficher.
fn prendre_intention_sans_focus() -> bool {
    OUVERTURE_SANS_FOCUS.with(|cell| cell.replace(false))
}

thread_local! {
    /// L'icône (rectangle ÉCRAN) qu'on vient de cliquer, et quand — d'où part
    /// l'effet d'ouverture du panneau qu'elle ouvre (voir
    /// `win32::ouverture_fenetres::animer_interne`).
    static ORIGINE_OUVERTURE: Cell<Option<([f32; 4], Instant)>> = const { Cell::new(None) };
}

/// Une origine plus vieille ne vaut plus : le clic n'ouvrait pas de panneau.
const ORIGINE_VALIDITE: Duration = Duration::from_millis(500);

/// Retient l'icône cliquée — posée par `app::run` à chaque clic sur une icône.
pub fn set_origine_ouverture(rect: [f32; 4]) {
    ORIGINE_OUVERTURE.with(|c| c.set(Some((rect, Instant::now()))));
}

/// Prend l'origine — et la consomme, comme `prendre_intention_sans_focus`.
fn prendre_origine_ouverture() -> Option<[f32; 4]> {
    ORIGINE_OUVERTURE.with(|c| c.take()).filter(|(_, quand)| quand.elapsed() < ORIGINE_VALIDITE).map(|(rect, _)| rect)
}

/// L'effet d'ouverture, pour un panneau ou une fenêtre — pas pour un MENU, qui a
/// son fondu. ⚠️ AVANT le `ShowWindow` : la fenêtre ne doit jamais paraître.
fn animer_l_ouverture(spec: &PopupSpec, hwnd: HWND, origine: Option<[f32; 4]>) {
    if !spec.fades {
        crate::win32::ouverture_fenetres::animer_interne(hwnd, origine);
    }
}

/// Retient la zone libre du menu qu'on vient de placer (voir `MENU_ROOM`).
pub fn set_menu_room(room: [i32; 4]) {
    MENU_ROOM.with(|cell| cell.set(room));
}

/// **DIT SI LE MENU QU'ON VIENT DE PLACER EST CENTRÉ HORIZONTALEMENT.**
///
/// DEMANDE EXPLICITE (2026-10-04) : « je voudrais que le panneau qui apparaît au
/// clic sur l'horloge soit centré horizontalement ».
///
/// ⚠️ **POSER LE CENTRAGE AU PLACEMENT NE SUFFIT PAS, ET C'EST MESURÉ.** La page
/// annonce sa taille RÉELLE après coup (`request_content_measure` →
/// `menu/size` → `resize_to_content`), et `resized_x` ne recentre pas : il ne
/// retient que le bord droit d'un menu qui le TOUCHAIT. Un panneau centré n'y
/// touche pas — il grandissait donc vers la droite depuis son coin gauche :
/// provisoire 1260 centré (marges 1186 / 1186), contenu réel 1850, **marges
/// finales 1186 / 596**. Le centrage doit donc survivre au redimensionnement.
///
/// `menu_rect` la remet à `false` : sans ça, un menu ordinaire ouvert après un
/// menu centré se recentrerait à son tour.
pub fn set_menu_centre(centre: bool) {
    MENU_CENTRE.with(|cell| cell.set(centre));
}

fn menu_centre() -> bool {
    MENU_CENTRE.with(std::cell::Cell::get)
}

/// **LE BORD DROIT RESTE SI LE MENU LE TOUCHAIT** — pendant horizontal de
/// `resized_y`, et MÊME RÈGLE : un menu collé à la limite droite de la zone
/// libre **grandit vers la GAUCHE**. Sans elle, il grandissait vers la droite,
/// borné par l'écran, donc **par-dessus la barre de droite** (défaut signalé
/// ci-dessus).
///
/// La condition est l'ÉGALITÉ, comme celle de `resized_y` : c'est `menu_rect`
/// qui pose le bord sur la limite quand il borne la position, donc un menu qui
/// n'a pas été borné garde son `x` — il n'y a rien à corriger.
fn resized_x(x: i32, current_w: i32, new_w: i32, right_limit: i32, left_limit: i32) -> i32 {
    if x + current_w == right_limit {
        // Jamais à gauche de la zone libre non plus : si la nouvelle largeur
        // dépasse la place disponible, `room_w` l'a déjà bornée.
        (right_limit - new_w).max(left_limit)
    } else {
        x
    }
}

#[cfg(test)]
mod resized_x_tests {
    use super::resized_x;

    /// **LE BORD DROIT RESTE SI LE MENU LE TOUCHAIT** — c'est le défaut signalé :
    /// un menu collé à la zone libre grandissait vers la droite, borné par
    /// l'écran, donc PAR-DESSUS la barre de droite.
    #[test]
    fn a_menu_flush_with_the_right_limit_grows_leftwards() {
        // Collé à la limite : le bord DROIT ne bouge pas, le gauche recule.
        assert_eq!(resized_x(100, 50, 80, 150, 0), 70, "150 - 80 : le bord droit reste a 150");
        // Même largeur : rien ne bouge (l'appelant a de toute façon déjà rendu).
        assert_eq!(resized_x(100, 50, 50, 150, 0), 100);

        // **PAS COLLÉ : RIEN NE BOUGE.** Un menu qui n'a jamais touché la limite
        // grandit vers la droite comme avant — c'est le cas courant, et la règle
        // ne doit pas le déplacer.
        assert_eq!(resized_x(100, 20, 80, 150, 0), 100);

        // Jamais à gauche de la zone libre : la largeur a déjà été bornée par
        // l'appelant, et ce plancher est la seconde ceinture.
        assert_eq!(resized_x(100, 50, 200, 150, 90), 90);
    }
}

fn resized_y(y: i32, current_h: i32, new_h: i32, screen_bottom: i32, screen_top: i32) -> i32 {
    if y + current_h == screen_bottom {
        // Le bas reste au bord ; le haut suit. Jamais au-dessus de l'écran.
        (screen_bottom - new_h).max(screen_top)
    } else {
        y
    }
}

/// Redimensionne la fenêtre à ce que la PAGE demande — `None` = ne touche pas à
/// cette dimension (un menu peut n'annoncer qu'une hauteur, ou qu'une largeur).
///
/// **LES DEUX SUIVENT LA MÊME RÈGLE** : plafond = la place jusqu'au bord de
/// l'écran, bord OPPOSÉ inchangé (gauche et haut, les côtés par lesquels le menu
/// est ancré à son icône). La hauteur est pilotée par la page depuis le
/// 2026-09-19 (mesuré : menu Contexte 520 → 430 px) ; la LARGEUR l'est à son tour
/// — DEMANDE EXPLICITE : « le menu du wifi est trop large par rapport à son
/// contenu », 1380 px pour deux boutons, un littéral figé au site d'appel.
///
/// Rend sans rien faire si la cible est déjà la taille courante.
pub fn resize_to_content(slot: &'static LocalKey<PopupSlot>, target_w: Option<i32>, target_h: Option<i32>) {
    slot.with(|s| {
        let mut state_ref = s.state.borrow_mut();
        let Some(state) = state_ref.as_mut() else { return };
        let [x, y, w, current_h] = state.rect;
        let screen = crate::win32::display::monitor_bounds(state.hwnd);
        let screen_bottom = screen[1] + screen[3];
        // ⚠️ **LE PLAFOND HORIZONTAL EST LA ZONE LIBRE, PAS L'ÉCRAN.** Entre les
        // deux il y a la BARRE DE DROITE, et un menu n'a rien à y faire : c'est
        // précisément ce qui la recouvrait (voir `MENU_ROOM`). Si aucune zone n'a
        // été retenue — un popup placé hors de `menu_rect`, aucun aujourd'hui —
        // on retombe sur l'écran, comme avant.
        let room = MENU_ROOM.with(std::cell::Cell::get);
        // ⚠️ **LES DEUX PLAFONDS SONT CEUX DE LA ZONE LIBRE ENTIÈRE, PAS CE QUI
        // RESTE DU CÔTÉ OÙ LE MENU EST ANCRÉ** — voir `horizontal_limits` et
        // `vertical_limits`, qui portent chacun leur mesure. Même règle, même
        // raison : un menu sait RECULER (`resized_x`) et REMONTER (`resized_y`),
        // donc la place disponible n'est jamais « ce qui reste » d'un seul côté.
        let (left_limit, right_limit, room_w) = horizontal_limits(room, screen, x);
        let (top_limit, bottom_limit, room_h) = vertical_limits(room, screen, y);
        let new_w = target_w.and_then(|target| clamped_content_height(w, target, room_w)).unwrap_or(w);
        let new_h = target_h.and_then(|target| clamped_content_height(current_h, target, room_h)).unwrap_or(current_h);
        // ⚠️ **UN ÉCART DE QUELQUES PIXELS N'EST PAS UN DÉMÉNAGEMENT.** La page
        // se mesure elle-même ; entre deux annonces d'un MÊME contenu il reste
        // une ou deux unités (barre de défilement, arrondi), et la confirmer
        // quand même faisait suivre la fenêtre pour ce résidu — mesuré : une
        // seconde `[menu/resize]` de 624 à 622 px juste après la première.
        // Sous `MESURE_TOLERANCE` on ne déplace pas la fenêtre.
        //
        // ⛔ **ET ON NE RETIENT PAS LA CIBLE, MAIS LA TAILLE AFFICHÉE.** Une page
        // qui mesure `vue + 2` unités CSS (typiquement `sound.html`, dont la
        // boîte est `fit-content` bornée à `100vw` : la barre de défilement
        // verticale fait déborder la mesure de la vue) ferait sinon grandir la
        // fenêtre de 4 px À CHAQUE ouverture — mesuré : 966 → 971 → 975 → 979.
        // Ce que l'utilisateur voit à la fermeture, c'est `w` ; c'est `w` qu'il
        // faut retrouver à la prochaine.
        if mesure_concordante([new_w, new_h], [w, current_h]) {
            retenir_pour_l_ouverture(s, [w, current_h]);
            return;
        }
        let new_y = resized_y(y, current_h, new_h, bottom_limit, top_limit)
            // ⚠️ **LE BORD RESTE DANS LA ZONE, MÊME QUAND IL N'Y ÉTAIT PAS COLLÉ.**
            // `resized_y` ne remonte que le menu dont le bas touchait la limite —
            // un menu ancré plus haut grandit donc vers le BAS, et dépasse la
            // zone libre dès que son contenu réel la remplit (mesuré : barre du
            // bas mordue de 12 px par le menu Son, ouvert à la hauteur de la
            // zone). Même famille de défaut que celui de `taille_d_ouverture`.
            .clamp(top_limit, (bottom_limit - new_h).max(top_limit));
        // ⚠️ **UN MENU CENTRÉ LE RESTE QUAND SA PAGE ANNONCE SA VRAIE TAILLE** —
        // voir `set_menu_centre`, qui porte la mesure du défaut. `resized_x` ne
        // sait que retenir le bord droit d'un menu qui le TOUCHAIT ; un panneau
        // centré n'y touche pas, donc il repart du coin gauche et le centrage est
        // perdu. Ici on le recalcule pour la NOUVELLE largeur, dans les mêmes
        // bornes que `resized_x` — les deux bouts de la zone libre.
        let new_x = if menu_centre() {
            left_limit + (right_limit - left_limit - new_w) / 2
        } else {
            resized_x(x, w, new_w, right_limit, left_limit)
                // Même garde que ci-dessus, sur l'axe horizontal : `resized_x`
                // recule un menu PLAQUÉ à droite, pas celui qui déborde en
                // grandissant depuis l'intérieur de la zone.
                .clamp(left_limit, (right_limit - new_w).max(left_limit))
        };
        // Trace de placement, jumelle de celle de `menu_rect` (qui journalise le
        // rectangle PROVISOIRE) : le bord de la ZONE LIBRE et celui de l'ÉCRAN
        // côte à côte, pour que l'écart — la barre du bas — se lise d'un coup
        // d'œil si un menu la recouvre encore. Bounded : n'écrit que lorsque le
        // rectangle change réellement (voir le `return` ci-dessus).
        crate::diag!(
            "[menu/resize] hwnd={:?} ({x},{y},{w},{current_h}) -> ({new_x},{new_y},{new_w},{new_h}) | room={:?} bas_zone={bottom_limit} bas_ecran={screen_bottom}",
            state.hwnd.0 as usize,
            room
        );
        state.rect = [new_x, new_y, new_w, new_h];
        unsafe {
            let _ = SetWindowPos(state.hwnd, None, new_x, new_y, new_w, new_h, SWP_NOZORDER | SWP_NOACTIVATE);
        }
        let _ = state.swapchain.resize(&state.device, new_w.max(1) as u32, new_h.max(1) as u32);
        state.browser.resize(new_w.max(1), new_h.max(1));
        // ⭐ **LA TAILLE RETENUE POUR LA PROCHAINE OUVERTURE** — voir
        // `taille_rappelee`, qui la relit (et la ramène à l'échelle du moment).
        // C'est ICI qu'elle est juste : `new_w`/`new_h` sont ce que la PAGE a
        // demandé, bornes de place comprises, pas une estimation.
        retenir_pour_l_ouverture(s, [new_w, new_h]);
    });
}

fn clamped_content_height(current_h: i32, target_h: i32, max_height: i32) -> Option<i32> {
    let new_h = target_h.clamp(1, max_height);
    if current_h == new_h {
        return None;
    }
    Some(new_h)
}

/// **LES DEUX BORNES HORIZONTALES D'UN POPUP « MENU »** — `(gauche, droite, largeur_max)`.
///
/// ⚠️ **LA LARGEUR MAXIMALE EST CELLE DE LA ZONE LIBRE, PAS CE QUI RESTE À DROITE DE
/// `x`.** C'est le même défaut que `vertical_limits`, sur l'autre axe, et il a la
/// même racine : `resized_x` SAIT reculer un menu plaqué contre la limite droite —
/// sa doc le dit mot pour mot, « un menu collé à la limite droite de la zone libre
/// grandit vers la GAUCHE » — mais il ne le pouvait pas, parce que
/// `clamped_content_height` recevait `right_limit - x` et **ramenait la largeur à
/// la place disponible à droite AVANT qu'il puisse intervenir**. Un menu ancré près
/// de la barre de droite était donc TRONQUÉ là où il fallait le RECULER.
///
/// **MESURÉ**, deux ouvertures du même menu dans la même session :
/// `(112,1779,756,297) -> (…, 924,327)` et `(2980,1779,756,297) -> (…, 756,327)`.
/// Le second est ancré à 3636, donc posé contre la limite droite (`x = 2980 =
/// 3736 - 756`) : `right_limit - x` valait exactement 756, et sa largeur n'a pas pu
/// rejoindre les 924 de son propre contenu. `resized_x` l'aurait reculé à 2812.
///
/// C'est la demande explicite « je ne veux pas que tu réduises les dimensions des
/// menus s'ils ne débordent pas » : un menu qui tient en RECULANT ne doit pas être
/// rétréci. Sans zone retenue (aucun popup aujourd'hui), on garde l'ancien
/// comportement.
fn horizontal_limits(room: [i32; 4], screen: [i32; 4], x: i32) -> (i32, i32, i32) {
    if room[2] > 0 {
        (room[0], room[0] + room[2], room[2].max(1))
    } else {
        let droite = screen[0] + screen[2];
        (screen[0], droite, (droite - x).max(1))
    }
}

/// **LES DEUX BORNES VERTICALES D'UN POPUP « MENU »** — `(haut, bas, hauteur_max)`.
///
/// ⚠️ **C'EST LE JUMEAU VERTICAL DE `MENU_ROOM`, ET IL MANQUAIT.** La LARGEUR
/// était bornée par la zone libre (`right_limit`/`left_limit`, tirés de
/// `MENU_ROOM`) ; la HAUTEUR prenait le bord de l'ÉCRAN. Les deux coïncidaient
/// tant que la barre du BAS n'existait pas — la zone libre descendait alors
/// jusqu'au bord de l'écran. Depuis, elle s'arrête `bottom_thickness` plus haut,
/// et **rien n'avait suivi** : même famille que l'infobulle de la barre du bas.
///
/// **MESURÉ** (barre du bas 80 px, `MENU_GAP` 4, zone libre finissant à 2076,
/// écran à 2160) : `menu_rect` pose le menu d'un séparateur du bas avec son BAS
/// sur 2076 ; la page annonce ensuite 327 px au lieu des 297 provisoires,
/// `resized_y` ne le retient pas — sa condition est `y + h == bas`, et il
/// comparait à 2160 — et le menu grandit vers le BAS, **26 px par-dessus la
/// barre**. Le défaut n'apparaissait que si le contenu réel est plus GRAND que
/// l'estimation `popup_height` : c'est le « ça dépend de l'endroit » constaté,
/// et la raison pour laquelle le même menu tantôt mordait, tantôt non.
///
/// **LA HAUTEUR MAXIMALE EST CELLE DE LA ZONE, PAS CE QUI RESTE SOUS `y`** :
/// `resized_y` peut REMONTER le menu, donc la place disponible n'est pas
/// `bas - y` — s'y borner tronquerait un menu qui tient très bien en remontant
/// (et la demande explicite est de ne PAS réduire un menu qui ne déborde pas).
///
/// **Sans zone retenue** — un popup placé hors de `menu_rect`, aucun aujourd'hui —
/// on retombe sur l'écran, exactement comme le faisait la largeur.
fn vertical_limits(room: [i32; 4], screen: [i32; 4], y: i32) -> (i32, i32, i32) {
    if room[2] > 0 {
        (room[1], room[1] + room[3], room[3].max(1))
    } else {
        let bas = screen[1] + screen[3];
        (screen[1], bas, (bas - y).max(1))
    }
}

#[cfg(test)]
mod horizontal_limits_tests {
    use super::{horizontal_limits, resized_x};

    /// Les nombres MESURÉS : zone libre `[104, 86, 3632, 1990]`, écran 3840 × 2160,
    /// barre de droite de 100 px et `MENU_GAP` de 4.
    const ZONE: [i32; 4] = [104, 86, 3632, 1990];
    const ECRAN: [i32; 4] = [0, 0, 3840, 2160];

    /// **LE DÉFAUT, ET SON ÉCART** : la largeur maximale est celle de la ZONE LIBRE,
    /// jamais ce qui reste à droite de `x` — sinon un menu ancré près de la barre de
    /// droite est tronqué à la place disponible.
    #[test]
    fn la_largeur_maximale_est_celle_de_la_zone_libre() {
        let (gauche, droite, largeur_max) = horizontal_limits(ZONE, ECRAN, 2980);
        assert_eq!(gauche, 104);
        assert_eq!(droite, 3736, "le bord de la zone libre, pas les 3840 de l'ecran");
        assert_eq!(droite, ECRAN[2] - 100 - 4, "et l'ecart EST la barre de droite + MENU_GAP");
        assert_eq!(largeur_max, 3632, "la largeur de la ZONE, pas les 756 restants a droite de x");
    }

    /// **LE CAS MESURÉ** : le menu ancré à 3636 est plaqué contre la limite droite
    /// (`x = 2980`, provisoire 756). Son contenu fait 924 : il doit **RECULER** — pas
    /// être tronqué à 756 comme le journal le montrait.
    #[test]
    fn un_menu_plaque_a_droite_recule_au_lieu_d_etre_tronque() {
        let (gauche, droite, largeur_max) = horizontal_limits(ZONE, ECRAN, 2980);
        let (x, provisoire, annonce) = (2980, 756, 924);
        assert_eq!(x + provisoire, droite, "premisse : le menu est plaque contre la limite");
        assert!(annonce <= largeur_max, "premisse : il tient en reculant");
        let nouveau_x = resized_x(x, provisoire, annonce, droite, gauche);
        assert_eq!(nouveau_x, 2812, "3736 - 924 : le bord DROIT reste, le gauche recule");
        assert_eq!(nouveau_x + annonce, droite, "et le bord droit ne bouge pas d'un pixel");
    }

    /// Sans zone retenue, rien ne change : c'est le comportement d'avant, et il doit le
    /// rester pour tout popup qui n'est pas passé par `menu_rect`.
    #[test]
    fn sans_zone_retenue_on_retombe_sur_l_ecran() {
        let (gauche, droite, largeur_max) = horizontal_limits([0, 0, 0, 0], ECRAN, 2000);
        assert_eq!((gauche, droite, largeur_max), (0, 3840, 1840));
    }
}

#[cfg(test)]
mod vertical_limits_tests {
    use super::{resized_y, vertical_limits};

    /// Les nombres MESURÉS sur la machine : barre du bas de 80 px, `MENU_GAP` de
    /// 4, zone libre `[104, 86, 3632, 1990]`, écran de 2160 de haut.
    const ZONE: [i32; 4] = [104, 86, 3632, 1990];
    const ECRAN: [i32; 4] = [0, 0, 3840, 2160];

    /// **LE DÉFAUT, ET SON ÉCART** : la borne basse d'un menu est le bord de la ZONE
    /// LIBRE, pas celui de l'écran — et l'écart vaut exactement la barre du bas plus
    /// la marge, c'est-à-dire ce que le menu recouvrait.
    #[test]
    fn la_borne_basse_est_le_bord_de_la_zone_libre() {
        let (haut, bas, hauteur_max) = vertical_limits(ZONE, ECRAN, 1779);
        assert_eq!(haut, 86);
        assert_eq!(bas, 2076, "le bord de la zone libre, pas les 2160 de l'ecran");
        assert_eq!(bas, ECRAN[1] + ECRAN[3] - 80 - 4, "et l'ecart EST la barre du bas + MENU_GAP");
        assert_eq!(hauteur_max, 1990, "la hauteur de la ZONE, pas ce qui reste sous y");
    }

    /// **LE CAS MESURÉ** : le menu, posé avec son bas sur 2076 et 297 px de haut,
    /// annonce 327. Il doit REMONTER de 30 px et garder son bas sur le bord de la
    /// zone — pas grandir vers le bas par-dessus la barre.
    #[test]
    fn un_menu_qui_grandit_remonte_au_lieu_de_mordre_sur_la_barre() {
        let (haut, bas, hauteur_max) = vertical_limits(ZONE, ECRAN, 1779);
        let (y, provisoire, annonce) = (1779, 297, 327);
        assert_eq!(y + provisoire, bas, "premisse : le menu etait pose sur le bord");
        assert!(annonce <= hauteur_max, "premisse : il tient en remontant");
        let nouveau_y = resized_y(y, provisoire, annonce, bas, haut);
        assert_eq!(nouveau_y, 1749, "30 px plus haut, et pas un de moins");
        assert_eq!(nouveau_y + annonce, bas, "le bas reste sur le bord de la zone libre");
        assert!(nouveau_y + annonce <= ECRAN[1] + ECRAN[3] - 80, "donc AUCUN recouvrement de la barre");
    }

    /// Le cas « pas toujours » — le même ancrage qui annonçait 297 → 219 et ne
    /// mordait donc pas.
    ///
    /// ⚠️ **C'EST LE BAS QUI EST ANCRÉ, PAS LE HAUT.** Le menu pend au-dessus de la
    /// barre du bas (son ancre est DANS la barre, donc `menu_rect` l'a remonté
    /// jusqu'au bord de la zone libre) : rétrécir doit rapprocher son haut de son
    /// bas, jamais le décoller de la barre. Même règle que
    /// `a_window_pushed_up_against_the_bottom_keeps_its_bottom`.
    ///
    /// *(Première écriture de ce test : j'attendais `1779`, c'est-à-dire un haut
    /// figé — l'inverse de la règle. Le code avait raison.)*
    #[test]
    fn un_menu_qui_retrecit_garde_son_bas_sur_le_bord() {
        let (haut, bas, _) = vertical_limits(ZONE, ECRAN, 1779);
        let nouveau_y = resized_y(1779, 297, 219, bas, haut);
        assert_eq!(nouveau_y, 1857, "2076 - 219 : le BAS reste sur le bord");
        assert_eq!(nouveau_y + 219, bas, "et le bord bas ne bouge pas d'un pixel");
        assert!(nouveau_y >= haut, "le haut ne passe pas au-dessus de la zone");
    }

    /// Sans zone retenue, rien ne change : c'est le comportement d'avant, et il doit
    /// le rester pour tout popup qui n'est pas passé par `menu_rect`.
    #[test]
    fn sans_zone_retenue_on_retombe_sur_l_ecran() {
        let (haut, bas, hauteur_max) = vertical_limits([0, 0, 0, 0], ECRAN, 400);
        assert_eq!((haut, bas, hauteur_max), (0, 2160, 1760));
    }
}

/// La hauteur annoncée par une page menu : ce qui est accepté, ce qui est
/// REFUSÉ, et la conversion en pixels physiques.
/// L'ANCRAGE d'une fenêtre qui change de hauteur (voir `resized_y`).
#[cfg(test)]
mod resized_y_tests {
    use super::resized_y;

    /// Le cas ORDINAIRE : un menu ancré sous une icône tient par son bord HAUT,
    /// donc rétrécir ne doit pas le déplacer — c'est ce que fait le code depuis
    /// toujours, et le test le fige.
    #[test]
    fn a_normally_anchored_window_keeps_its_top() {
        let (y, h) = (400, 500);
        assert_eq!(resized_y(y, h, 200, 2088, 0), y, "rétrécir");
        assert_eq!(resized_y(y, h, 900, 2088, 0), y, "grandir");
    }

    /// **Le cas du DÉFAUT** : une fenêtre remontée par `menu_rect` a son bas
    /// sur le bord de l'écran, et c'est son BAS qui doit rester — sinon elle se
    /// détache de l'icône sous laquelle elle est censée pendre.
    #[test]
    fn a_window_pushed_up_against_the_bottom_keeps_its_bottom() {
        let screen_bottom = 2088;
        // Remontée : son bas est exactement au bord.
        let (current_h, new_h) = (900, 300);
        let y = screen_bottom - current_h;

        let moved = resized_y(y, current_h, new_h, screen_bottom, 0);
        assert_eq!(moved + new_h, screen_bottom, "le bas n'a pas bougé");
        assert!(moved > y, "et le haut est descendu, puisqu'elle a rétréci");

        // En GRANDISSANT : le haut remonte, le bas ne bouge pas davantage.
        let grown = resized_y(y, current_h, 1200, screen_bottom, 0);
        assert_eq!(grown + 1200, screen_bottom, "le bas ne bouge jamais");
        assert!(grown < y);
    }

    /// Une fenêtre plus haute que l'écran ne peut pas remonter indéfiniment : son
    /// haut s'arrête au bord, quitte à ce que le bas dépasse — c'est la même
    /// contrepartie que `menu_rect`, qui ne rétrécit pas non plus.
    #[test]
    fn the_top_never_goes_above_the_screen() {
        let (screen_top, screen_bottom) = (0, 1080);
        let y = screen_bottom - 400;
        assert_eq!(resized_y(y, 400, 5000, screen_bottom, screen_top), screen_top);
    }
}

#[cfg(test)]
mod intention_sans_focus_tests {
    use super::{prendre_intention_sans_focus, set_ouverture_sans_focus};

    /// ⭐ **L'INTENTION VAUT POUR UNE OUVERTURE, ET UNE SEULE.** C'est la moitié
    /// dont la panne serait muette : gardée, elle ferait ouvrir le popup SUIVANT
    /// sans premier plan — un menu qui n'attrape pas le clavier, sans erreur et
    /// sans rapport visible avec ce qu'on venait d'ouvrir.
    #[test]
    fn l_intention_est_consommee_par_la_premiere_ouverture() {
        set_ouverture_sans_focus(true);
        assert!(prendre_intention_sans_focus(), "la premiere ouverture la voit");
        assert!(!prendre_intention_sans_focus(), "la deuxieme ne l'a plus");
        assert!(!prendre_intention_sans_focus(), "et jamais plus ensuite");
    }

    /// **LE DÉFAUT EST CELUI DU DÉFAUT** : sans intention posée, un popup prend le
    /// premier plan comme avant. Ce test garde le sens de la valeur par défaut —
    /// c'est lui qui distingue « on a oublié de poser l'intention » de « on a
    /// décidé de ne pas la poser ».
    #[test]
    fn sans_intention_posee_l_ouverture_prend_le_premier_plan() {
        // On part d'un état propre : un test voisin a pu poser l'intention.
        let _ = prendre_intention_sans_focus();
        assert!(!prendre_intention_sans_focus(), "par defaut, on prend le premier plan");
    }
}

#[cfg(test)]
mod menu_height_action_tests {
    use super::{content_height_px, parse_content_size};

    /// **Ce qui doit être REFUSÉ compte autant que ce qui passe** : `parse::<f64>`
    /// est bien plus permissif qu'on ne le croit, et `"nan" as i32` vaut 0 — soit
    /// une fenêtre d'un pixel, le plancher de `clamped_content_height`.
    #[test]
    fn only_a_plausible_size_is_accepted() {
        assert_eq!(parse_content_size("menu/size/438/120"), Some((438.0, 120.0)));
        assert_eq!(parse_content_size("menu/size/0/0"), Some((0.0, 0.0)), "0 reste borné plus loin");

        for refused in [
            "menu/size/",
            "menu/size/438",
            "menu/size/438/",
            "menu/size//120",
            "menu/size/abc/120",
            "menu/size/438/abc",
            "menu/size/nan/120",
            "menu/size/438/inf",
            "menu/size/-5/120",
            "menu/size/1e30/120",
            "power/lock",
            "menu/height/438",
        ] {
            assert_eq!(parse_content_size(refused), None, "aurait dû être refusé : {refused:?}");
        }
    }

    /// **`ceil`, JAMAIS `round`** : le facteur vaut 2,7 sur cette machine, et
    /// arrondir vers le bas d'une fraction de pixel suffit à faire apparaître la
    /// barre de défilement fantôme — celle qui a été MESURÉE.
    #[test]
    fn the_conversion_never_rounds_down() {
        assert_eq!(content_height_px(100.4, 2.7), 272, "271,08 -> 272, pas 271");
        assert_eq!(content_height_px(100.0, 2.7), 270, "exact : 270");
        assert_eq!(content_height_px(1.0, 1.0), 1);
        assert_eq!(content_height_px(0.0, 2.7), 1, "jamais zéro : une fenêtre de 0 px n'existe pas");

        // Monotone dans les deux arguments — une hauteur plus grande ne peut pas
        // donner une fenêtre plus petite.
        let mut previous = 0;
        for css in [10.0, 50.5, 100.0, 300.25, 1000.0] {
            let px = content_height_px(css, 2.7);
            assert!(px > previous, "{css} px CSS -> {px}, pas plus que {previous}");
            previous = px;
        }
    }
}

/// La correction AltGr (voir `alt_gr_adjusted`, doc) — DÉFAUT RÉEL SIGNALÉ :
/// « je n'arrive pas à taper les caractères # et @ dans une note ».
#[cfg(test)]
mod alt_gr_tests {
    use super::{alt_gr_adjusted, ALTGR};

    const SHIFT: u32 = 2;
    const CONTROL: u32 = 4;
    const ALT: u32 = 8;

    /// Le cas du défaut, **tel qu'il a été mesuré** : Windows émule AltGr par
    /// Ctrl+Alt, donc `#`, `@`, `€` et `|` partaient avec CES DEUX bits, et
    /// Blink refuse d'insérer du texte dès que l'un OU l'autre est présent.
    /// Retirer le seul Ctrl — la première tentative — laissait donc `mods=8`,
    /// tel que relevé par la sonde, et ne changeait rien.
    #[test]
    fn alt_gr_loses_both_its_bits() {
        assert_eq!(alt_gr_adjusted(CONTROL | ALT) & (CONTROL | ALT), 0);
    }

    /// ⭐ **ET IL DEVIENT UN VRAI AltGr** — DÉFAUT RÉEL SIGNALÉ (2026-10-16) : « je
    /// n'arrive pas à taper l'antislash dans les terminaux » (AltGr+8 en AZERTY).
    /// Sans AUCUN modificateur, xterm prenait la touche pour une frappe ordinaire
    /// et envoyait sa valeur de BASE dès le `keydown`, puis annulait le caractère
    /// `\` qui suivait. Avec `EVENTFLAG_ALTGR_DOWN`, Blink répond `true` à
    /// `getModifierState("AltGraph")` : xterm y reconnaît un « troisième
    /// niveau » et prend le caractère. Ctrl et Alt restent retirés (les notes).
    #[test]
    fn alt_gr_is_reported_as_alt_gr() {
        assert_eq!(alt_gr_adjusted(CONTROL | ALT), ALTGR);
        assert_eq!(alt_gr_adjusted(CONTROL | ALT | SHIFT), SHIFT | ALTGR);
        assert_eq!(ALTGR, cef::sys::cef_event_flags_t::EVENTFLAG_ALTGR_DOWN.0 as u32);
        for seul in [0, SHIFT, CONTROL, ALT, CONTROL | SHIFT, ALT | SHIFT] {
            assert_eq!(alt_gr_adjusted(seul) & ALTGR, 0, "{seul} : pas d'AltGr");
        }
    }

    /// Le bit Ctrl SEUL est un vrai raccourci (Ctrl+C, Ctrl+V...) : il doit
    /// survivre intact, sans quoi on casserait tout le reste du clavier en
    /// réparant les caractères composés.
    #[test]
    fn a_real_control_shortcut_is_left_alone() {
        assert_eq!(alt_gr_adjusted(CONTROL), CONTROL);
        assert_eq!(alt_gr_adjusted(CONTROL | SHIFT), CONTROL | SHIFT);
    }

    /// Alt seul (accès aux menus, et sur certains pilotes une touche de
    /// composition) n'a pas de bit Ctrl à retirer — et ne doit rien perdre.
    #[test]
    fn alt_alone_is_untouched() {
        assert_eq!(alt_gr_adjusted(ALT), ALT);
    }

    /// Rien enfoncé : une lettre ordinaire — c'est ce qui marchait déjà, et ce
    /// qui explique que le défaut ne touchait QUE les caractères composés.
    #[test]
    fn a_plain_character_is_untouched() {
        assert_eq!(alt_gr_adjusted(0), 0);
        assert_eq!(alt_gr_adjusted(SHIFT), SHIFT);
    }

    /// AltGr AVEC Maj (AltGr+Maj, fréquent pour composer d'autres caractères) :
    /// seul le bit Ctrl part, Maj reste.
    #[test]
    fn alt_gr_with_shift_keeps_the_shift() {
        assert_eq!(alt_gr_adjusted(CONTROL | ALT | SHIFT) & !ALTGR, SHIFT);
    }

    /// Un VRAI Alt+<touche> (sans Ctrl : accès aux menus, et sur certaines
    /// dispositions une touche de composition) ne doit RIEN perdre — c'est la
    /// raison pour laquelle la condition exige les DEUX bits, et non « Alt
    /// suffit ». L'élargir casserait l'accès aux menus des popups.
    #[test]
    fn a_real_alt_press_is_left_alone() {
        assert_eq!(alt_gr_adjusted(ALT), ALT);
        assert_eq!(alt_gr_adjusted(ALT | SHIFT), ALT | SHIFT);
    }
}

#[cfg(test)]
mod content_height_tests {
    use super::clamped_content_height;

    #[test]
    fn shrinks_to_the_requested_height() {
        assert_eq!(clamped_content_height(1440, 900, 1440), Some(900));
    }

    #[test]
    fn can_grow_back_up_to_the_ceiling_but_never_beyond() {
        assert_eq!(clamped_content_height(800, 960, 1440), Some(960));
        // Demande au-delà du plafond — bornée à `max_height`, l'excédent
        // défile côté page (voir `.menu { overflow-y: auto }` dans
        // voicemeeter.html).
        assert_eq!(clamped_content_height(1440, 5000, 1440), None, "déjà au plafond, rien à faire");
        assert_eq!(clamped_content_height(800, 5000, 1440), Some(1440));
    }

    #[test]
    fn clamps_non_positive_targets_to_the_1px_floor() {
        assert_eq!(clamped_content_height(800, 0, 1440), Some(1));
        assert_eq!(clamped_content_height(800, -100, 1440), Some(1));
    }

    #[test]
    fn no_op_when_the_computed_height_already_matches() {
        assert_eq!(clamped_content_height(900, 900, 1440), None);
    }
}

/// **LA PERTE DE FOCUS FERME-T-ELLE CE POPUP ?** — la RÈGLE, pure et testable.
///
/// ⚠️ **ELLE VIT HORS DU CORPS DE `hide_if_focus_elsewhere`**, pour la même raison
/// que `cef_sous_dock::doit_fermer` : une décision enfermée dans une fonction qui
/// interroge `GetForegroundWindow` ne s'éprouve qu'en volant le focus d'une vraie
/// fenêtre — ce qu'aucun test de ce dépôt ne fait, et ne doit faire.
///
/// Les deux raisons de NE PAS fermer n'ont rien à voir l'une avec l'autre :
/// - `dragging` : un glissement en cours ne doit pas être coupé par un vol de
///   focus transitoire (voir `PopupSlot::dragging`) ;
/// - `epingle` : l'utilisateur a demandé que le panneau RESTE — « un clic sur
///   autre chose ne doit pas fermer un dash épinglé ».
pub fn la_perte_de_focus_ferme(epingle: bool, dragging: bool) -> bool {
    !epingle && !dragging
}

/// **LE PANNEAU EST-IL MAINTENU OUVERT ?** — voir `PopupSlot::epingle`.
pub fn est_epingle(slot: &'static LocalKey<PopupSlot>) -> bool {
    slot.with(|s| s.epingle.get())
}

/// **BASCULE LA PUNAISE.** Elle ne repousse rien elle-même : le bouton lit son état
/// par le JS injecté (`js_epingle`), que `push_window_palette` exécute **à chaque
/// frame** pour tout popup visible à palette `Window`.
pub fn basculer_epingle(slot: &'static LocalKey<PopupSlot>) {
    let nouveau = slot.with(|s| {
        let nouveau = !s.epingle.get();
        s.epingle.set(nouveau);
        nouveau
    });
    crate::diag!("[popup] punaise : {nouveau}");
}

/// Masque la fenêtre si elle a RÉELLEMENT perdu le focus — voir
/// `win32::dash::hide_if_focus_elsewhere`, doc détaillée là-bas.
pub fn hide_if_focus_elsewhere(slot: &'static LocalKey<PopupSlot>) {
    let hwnd = slot.with(|s| s.state.borrow().as_ref().map(|state| state.hwnd));
    let Some(hwnd) = hwnd else { return };
    // Voir `PopupSlot::dragging`, doc de champ — un glissement de curseur
    // en cours ne doit JAMAIS être interrompu par un vol de focus
    // transitoire, quelle qu'en soit la cause.
    //
    // ⛔ **ET VOIR `la_perte_de_focus_ferme`, QUI PORTE LA RÈGLE ENTIÈRE** — la
    // punaise y est ajoutée, et c'est cette fonction qui est TESTÉE : la règle
    // enfermée dans ce corps ne s'éprouverait qu'en volant le focus d'une vraie
    // fenêtre, ce qu'aucun test de ce dépôt ne fait.
    let (epingle, dragging) =
        slot.with(|s| (s.epingle.get(), s.dragging.get()));
    if !la_perte_de_focus_ferme(epingle, dragging) {
        return;
    }
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            slot.with(|s| s.focus_lost_since.set(None));
            return;
        }
        if GetForegroundWindow() == hwnd {
            slot.with(|s| {
                s.had_focus.set(true);
                s.focus_lost_since.set(None);
            });
            return;
        }
        if !slot.with(|s| s.had_focus.get()) {
            return;
        }
        let now = Instant::now();
        let lost_since = slot.with(|s| {
            let since = s.focus_lost_since.get().unwrap_or(now);
            s.focus_lost_since.set(Some(since));
            since
        });
        if now.duration_since(lost_since) >= FOCUS_LOST_DEBOUNCE {
            // Trace (voir `diag`, inerte sans `NEURADESK_DIAG`) : QUEL premier
            // plan a pris la place du popup — c'est la seule chose qui manquait
            // pour comprendre un popup qui se masque tout seul (DÉFAUT RÉEL
            // SIGNALÉ sur le curseur de taille d'espace, voir `app::
            // poll_overlay_windows`, qui l'exempte désormais).
            crate::diag!(
                "popup {:#x} masque : premier plan = {:#x}",
                hwnd.0 as usize,
                unsafe { GetForegroundWindow().0 as usize }
            );
            // c'est ICI que passe la fermeture ordinaire d'un menu — un clic
            // ailleurs — donc c'est ici que le fondu se voit le plus.
            begin_hide(hwnd, slot);
            slot.with(|s| s.focus_lost_since.set(None));
        }
    }
}

/// Le `HWND` de cette fenêtre, si elle a déjà été créée — pour un popup qui
/// a besoin de SON PROPRE handle (ex. `win32::cef_clipboard`, propriétaire
/// de la boîte de dialogue Win32 native "Enregistrer sous", voir
/// `GetSaveFileNameW`/`hwndOwner`) plutôt que celui de la fenêtre du dock.
/// Donne le PREMIER PLAN et le CLAVIER à ce popup s'il ne les a pas — vue
/// sphère des terminaux (`win32::cef_sphere`), qui cache un panneau pour en
/// montrer un autre : **Windows ne réattribue pas le premier plan quand une
/// fenêtre au premier plan se cache** (voir `set_ouverture_sans_focus`), et la
/// fenêtre montrée restait sans clavier ni molette. Rend `true` quand la fenêtre
/// EST au premier plan après l'appel ; `false` si elle est cachée ou si Windows
/// a refusé.
pub fn donner_le_clavier(slot: &'static LocalKey<PopupSlot>) -> bool {
    let Some(hwnd) = hwnd(slot) else { return false };
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return false;
    }
    if unsafe { GetForegroundWindow() } != hwnd {
        force_foreground(hwnd);
    }
    let obtenu = unsafe { GetForegroundWindow() } == hwnd;
    if obtenu {
        slot.with(|s| {
            if let Ok(state) = s.state.try_borrow() {
                if let Some(state) = state.as_ref() {
                    state.browser.set_focus(true);
                }
            }
        });
    }
    obtenu
}

pub fn hwnd(slot: &'static LocalKey<PopupSlot>) -> Option<HWND> {
    slot.with(|s| s.state.borrow().as_ref().map(|state| state.hwnd))
}

/// `true` si la fenêtre existe ET est actuellement affichée — pour les
/// popups qui ont besoin de rafraîchir leur état À CHAQUE FRAME tant qu'ils
/// sont ouverts (ex. `win32::cef_voicemeeter`, dont l'état peut changer
/// depuis Voicemeeter lui-même ou une autre application connectée à son
/// API, pas seulement depuis ce popup — voir sa doc de module), plutôt que
/// seulement à l'ouverture (`toggle`) comme `win32::cef_network`/
/// `win32::cef_sound`.
pub fn is_visible(slot: &'static LocalKey<PopupSlot>) -> bool {
    slot.with(|s| s.state.borrow().as_ref().is_some_and(|state| unsafe { IsWindowVisible(state.hwnd).as_bool() }))
}

/// Clic du dock sur un applet FENÊTRE **déjà ouvert**. Rend `true` quand le cas
/// « déjà ouvert » a été traité — l'appelant (`toggle`) ne doit alors PAS
/// poursuivre vers l'ouverture ; `false` quand la fenêtre n'est pas affichée, et
/// c'est à l'appelant de l'ouvrir.
///
/// ⭐ **DEMANDE EXPLICITE : « applet clic si déjà ouvert basculer au premier
/// plan ».** Avant, un second clic sur l'icône CACHAIT toujours la fenêtre —
/// donc cliquer l'icône d'une fenêtre simplement reléguée DERRIÈRE une autre la
/// faisait disparaître au lieu de la ramener. `GetForegroundWindow` distingue les
/// deux cas : enterrée → on la remonte (`force_foreground`) ; déjà au premier plan
/// → on la cache (on garde ainsi un geste de fermeture au dock). Même famille que
/// `donner_le_clavier`, qui ne fait QUE remonter.
pub fn basculer_ou_cacher_si_ouvert(slot: &'static LocalKey<PopupSlot>) -> bool {
    if !is_visible(slot) {
        return false;
    }
    let Some(hwnd) = hwnd(slot) else {
        return true;
    };
    if unsafe { GetForegroundWindow() } == hwnd {
        cacher_fenetre(hwnd);
    } else {
        force_foreground(hwnd);
    }
    true
}

/// Vrai si ce popup est EN COURS DE DISPARITION — le fondu de sortie est
/// engagé, l'opacité n'est pas encore nulle (voir `fade::WindowFade::
/// is_fading_out`).
///
/// **DISTINCT DE `is_visible`, ET C'EST TOUT SON OBJET** : pendant le fondu de
/// sortie la fenêtre Win32 existe et est visible, donc `is_visible` répond
/// encore vrai — c'est la conséquence directe du fondu, qui a besoin que la
/// fenêtre vive le temps de s'éteindre. Tout code qui veut dire « ce popup est
/// FERMÉ » doit donc consulter ce prédicat-là en plus du sien ; celui qui veut
/// dire « la fenêtre est à l'écran » garde `is_visible`.
///
/// Deux sites le consultent, et ce sont exactement les deux qui mesurent une
/// fermeture : `drain_actions` (ne plus redimensionner une fenêtre qui s'en
/// va) et la trace de `cef_confirm::dispatch_actions` (qui annonce la
/// fermeture après une réponse).
pub fn is_closing(slot: &'static LocalKey<PopupSlot>) -> bool {
    slot.with(|s| s.state.borrow().as_ref().is_some_and(fade_state_is_closing))
}

/// Voir `is_closing` — la forme qui travaille sur un état déjà emprunté.
fn fade_state_is_closing(state: &CefPopupState) -> bool {
    state.fade.as_ref().is_some_and(fade::WindowFade::is_fading_out)
}

/// Engage la disparition d'un popup : **fondu** s'il en a un (les menus), et
/// masquage IMMÉDIAT sinon (les panneaux — voir `PopupSpec::fades`).
///
/// **L'EMPRUNT DE `state` EST RELÂCHÉ AVANT `ShowWindow`**, et ce n'est pas une
/// précaution de style : cette fonction-ci doit emprunter MUTABLEMENT pour
/// engager le fondu, alors que `hide_if_visible` ne prenait qu'un emprunt
/// PARTAGÉ — et c'est cet emprunt partagé qui rendait le `ShowWindow` inoffensif
/// quand il repart vers le wndproc de cette même fenêtre (`WM_KILLFOCUS`, voir
/// le commentaire de `toggle_with`). Le partage des deux phases est donc ce qui
/// préserve cette protection.
///
/// Dans le cas du fondu, `ShowWindow` n'est PAS appelé : la fenêtre reste
/// affichée le temps qu'elle s'éteigne, et c'est `tick` — appelé par
/// `render_if_visible`, à chaque frame — qui la cachera pour de bon.
fn begin_hide(hwnd: HWND, slot: &'static LocalKey<PopupSlot>) {
    let fading = slot.with(|s| {
        let mut state_ref = s.state.borrow_mut();
        let Some(state) = state_ref.as_mut() else { return false };
        match state.fade.as_mut() {
            Some(window_fade) => {
                window_fade.fade_out();
                true
            }
            None => false,
        }
    });
    if !fading {
        cacher_et_garer(hwnd);
    }
}

/// **MASQUE LE POPUP — SAUF S'IL EST ÉPINGLÉ.**
///
/// ⛔ **C'EST LE CHEMIN PAR DÉFAUT, ET C'EST VOULU** : tous les masquages PASSIFS
/// passent par ici — le clic sur la barre (`app::hide_overlay_windows`), l'applet
/// qui masque l'interface, et tout ce qui appelle le wrapper `hide_if_visible()`
/// d'un module. Aucun d'eux ne doit défaire une punaise : « un clic sur autre
/// chose ne doit pas fermer un dash épinglé ».
///
/// ⚠️ **LE BOUTON `✖` D'UNE PAGE, LUI, DOIT FERMER** — il passe donc par
/// `fermer_par_geste`, qui ignore la punaise. Les deux ne pouvaient pas rester une
/// seule fonction : le clic sur la barre et le `✖` aboutissaient ici tous les deux,
/// et ils ne veulent plus la même chose.
pub fn hide_if_visible(slot: &'static LocalKey<PopupSlot>) {
    if slot.with(|s| s.epingle.get()) {
        return;
    }
    fermer_par_geste(slot);
}

/// **MASQUE LE POPUP, PUNAISE OU PAS** — le `✖` d'une page, et le remplacement par
/// un autre dash (voir `fermer_les_autres`).
///
/// ⚠️ **SANS ELLE, UNE PUNAISE SERAIT DÉFINITIVE** : le seul geste de fermeture
/// propre d'un panneau épinglé est son `✖`, et une punaise qu'aucun geste ne défait
/// est un piège.
pub fn fermer_par_geste(slot: &'static LocalKey<PopupSlot>) {
    let hwnd = slot.with(|s| s.state.borrow().as_ref().map(|state| state.hwnd));
    if let Some(hwnd) = hwnd {
        begin_hide(hwnd, slot);
    }
}

/// Déplace le curseur SOURIS CEF (PAS le vrai curseur Windows, ni
/// `SendInput` — un appel de fonction direct) à `(x, y)` — réservé au
/// harnais de vérification pour confirmer que les événements souris
/// atteignent réellement CEF (le survol CSS doit apparaître à la frame
/// suivante) SANS le moindre input simulé.
pub fn debug_hover(slot: &'static LocalKey<PopupSlot>, x: i32, y: i32) {
    slot.with(|s| {
        if let Some(state) = s.state.borrow().as_ref() {
            state.browser.on_mouse_move(x, y, 0, false);
        }
    });
}

/// Clic SOURIS CEF direct (mêmes garanties que `debug_hover`) — réservé au
/// harnais de vérification pour confirmer qu'un `onclick` HTML déclenche
/// bien une action IPC récupérable via `drain_actions`.
/// Un clic MOLETTE CEF direct — réservé au harnais, comme [`debug_click`].
pub fn debug_middle_click(slot: &'static LocalKey<PopupSlot>, x: i32, y: i32) {
    slot.with(|s| {
        if let Some(state) = s.state.borrow().as_ref() {
            state.browser.on_mouse_middle_click(x, y, 0);
        }
    });
}

pub fn debug_click(slot: &'static LocalKey<PopupSlot>, x: i32, y: i32) {
    slot.with(|s| {
        if let Some(state) = s.state.borrow().as_ref() {
            state.browser.on_mouse_left_button(x, y, true, 0, 1);
            state.browser.on_mouse_left_button(x, y, false, 0, 1);
        }
    });
}

/// **BOUTON DROIT, SANS SOURIS NI `SendInput`** — même garantie que `debug_click`
/// ci-dessus (voir [[neuradesk-live-input-safety]]), et surtout le MÊME geste que la
/// vraie branche `WM_RBUTTONDOWN` : appui ET relâchement d'un coup, parce que le
/// `WM_RBUTTONUP` n'arrive JAMAIS à ces fenêtres (mesuré 5 292 DOWN contre 0 UP).
/// Émettre seulement l'appui laisserait CEF avec un bouton droit pressé, et le
/// `contextmenu` de Blink part au RELÂCHEMENT sous Windows — donc rien.
///
/// `x`/`y` : coordonnées de VUE (client), comme `debug_click`. Sans modificateur :
/// le menu d'édition ne teste que la sélection, jamais Maj/Ctrl.
pub fn debug_right_click(slot: &'static LocalKey<PopupSlot>, x: i32, y: i32) {
    slot.with(|s| {
        if let Some(state) = s.state.borrow().as_ref() {
            state.browser.on_mouse_right_click(x, y, 0);
        }
    });
}

/// **GLISSEMENT SOURIS CEF, SANS SOURIS NI `SendInput`** — le geste qui CRÉE une
/// sélection en page : appui gauche en `(x0, y0)`, des déplacements qui portent
/// `EVENTFLAG_LEFT_MOUSE_BUTTON` (16) le long du chemin, relâchement en `(x1, y1)`.
/// ⛔ Sans ce bit, Blink traite chaque déplacement comme un simple SURVOL et ne
/// glisse jamais (le bug réel est documenté dans `cef_app::on_mouse_move` : sans
/// lui, un `<input type="range">` ne suivait déjà pas le curseur pendant sa
/// glissade). Les huit pas partent dans la MÊME frame : `send_mouse_move_event` est
/// synchrone côté Rust et une sélection Blink se construit par accumulation de
/// déplacements, pas par le temps écoulé.
///
/// C'est le seul moyen d'éprouver la **sélection À LA SOURIS** d'une page (sa
/// règle `user-select`) sans simuler la moindre entrée
/// ([[neuradesk-live-input-safety]]). `x`/`y` : coordonnées de VUE (client), comme
/// `debug_click`.
pub fn debug_drag_select(slot: &'static LocalKey<PopupSlot>, x0: i32, y0: i32, x1: i32, y1: i32) {
    const EVENTFLAG_LEFT_MOUSE_BUTTON: u32 = 16;
    const PAS: i32 = 8;
    slot.with(|s| {
        if let Some(state) = s.state.borrow().as_ref() {
            // Windows envoie TOUJOURS un `WM_MOUSEMOVE` avant l'appui : sans ce
            // survol initial, Blink ne sait pas où le bouton est descendu.
            state.browser.on_mouse_move(x0, y0, 0, false);
            state.browser.on_mouse_left_button(x0, y0, true, 0, 1);
            for pas in 1..=PAS {
                let t = pas as f64 / PAS as f64;
                let x = (x0 as f64 + (x1 - x0) as f64 * t).round() as i32;
                let y = (y0 as f64 + (y1 - y0) as f64 * t).round() as i32;
                state.browser.on_mouse_move(x, y, EVENTFLAG_LEFT_MOUSE_BUTTON, false);
            }
            state.browser.on_mouse_left_button(x1, y1, false, EVENTFLAG_LEFT_MOUSE_BUTTON, 1);
        }
    });
}

/// Molette SOURIS CEF directe (mêmes garanties que `debug_hover`/
/// `debug_click`, voir `win32::cef_app::CefBrowserSurface::on_mouse_wheel`
/// pour la convention de signe de `delta_y`) — réservé au harnais de
/// vérification pour faire défiler une page dont le contenu à vérifier
/// dépasse la hauteur visible (ex. `assets/popups/settings.html`, la
/// section "Couleur de mise en avant" est tout en bas), sans le moindre
/// input simulé.
pub fn debug_scroll(slot: &'static LocalKey<PopupSlot>, x: i32, y: i32, delta_y: i32) {
    slot.with(|s| {
        if let Some(state) = s.state.borrow().as_ref() {
            state.browser.on_mouse_wheel(x, y, 0, 0, delta_y);
        }
    });
}

/// Vide et retourne les actions demandées par la page depuis le dernier
/// appel (voir `win32::cef_app::CefBrowserSurface::drain_actions`, doc
/// détaillée là-bas) — À appeler à chaque frame. Vide (jamais `None`) si la
/// fenêtre n'existe pas — même convention que `Window::drain_events`.
pub fn drain_actions(slot: &'static LocalKey<PopupSlot>) -> Vec<String> {
    let mut actions = slot.with(|s| s.state.borrow().as_ref().map(|state| state.browser.drain_actions()).unwrap_or_default());
    // ⏱ **LA MESURE D'OUVERTURE SE DEMANDE ICI, PAS DANS `toggle_with`.**
    // `app::run` appelle les `render_if_visible` AVANT les `dispatch_actions` :
    // à ce point de la frame, le zoom du menu vient d'être posé (voir
    // `mesure_differee`, et le défaut de `sound.html` qu'il corrige). Deux
    // frames pour laisser CEF remettre la page en page à ce zoom.
    let demander_la_mesure = slot.with(|s| match s.mesure_differee.get() {
        Some(0) => {
            s.mesure_differee.set(None);
            true
        }
        Some(restant) => {
            s.mesure_differee.set(Some(restant - 1));
            false
        }
        None => false,
    });
    if demander_la_mesure {
        crate::diag!("[menu/mesure] demandee apres le zoom");
        request_content_measure(slot);
    }
    // LA HAUTEUR PILOTÉE PAR LA PAGE — traitée ICI, et c'est le seul endroit qui
    // le puisse : il connaît le SLOT (= quelle fenêtre redimensionner) et voit
    // passer TOUTES les actions, chaque module s'alimentant ici.
    //
    // **L'action est RETIRÉE ensuite** : sans ça chaque module la verrait,
    // ne la reconnaîtrait pas, et la journaliserait comme inconnue.
    //
    // **Et l'application vient APRÈS la fin de l'emprunt ci-dessus** —
    // `resize_to_content` en prend un MUTABLE, les fondre en un seul
    // `slot.with` ferait paniquer (même classe de réentrance que `toggle_with`
    // documente longuement).
    // `menu/width/…` et `menu/height/…` — **la DERNIÈRE valeur de CHAQUE famille
    // gagne** : une page peut en envoyer plusieurs dans la même frame
    // (observateur + rattrapage), et c'est la plus récente qui décrit le contenu.
    let mut width: Option<f64> = None;
    let mut height: Option<f64> = None;
    for action in &actions {
        if let Some((w, h)) = parse_content_size(action) {
            width = Some(w);
            height = Some(h);
        }
    }
    actions.retain(|action| parse_content_size(action).is_none());

    // ⭐ **LA PUNAISE — UNE ACTION SANS VERBE PAR MODULE.** Chaque page a son
    // propre `go(...)`, donc l'action arrive **préfixée** (`dash/epingle`,
    // `clipboard/epingle`, `sous-dock/g/epingle`…). On la reconnaît sur son
    // **dernier segment**, et on la RETIRE — comme `menu/size` juste au-dessus.
    //
    // ⛔ **C'EST CE QUI REND LES QUINZE PANNEAUX POSSIBLES SANS QUINZE FOIS LE MÊME
    // CODE** : aucun module `cef_*` ne voit passer l'action, donc ni `VERBES`, ni
    // les tests de contrat, ni les bras de dispatch n'ont à être touchés. Sans ce
    // point d'étranglement, il aurait fallu un vérbe, un état et un bras dans
    // chacun des quatorze panneaux — c'est-à-dire quatorze occasions de diverger.
    //
    // ⚠️ **LE DERNIER SEGMENT, PAS UN PRÉFIXE** : exiger un préfixe obligerait
    // chaque page à connaître un nom de canal qui n'est pas le sien.
    let punaises = actions.iter().filter(|action| est_action_epingle(action)).count();
    actions.retain(|action| !est_action_epingle(action));
    for _ in 0..punaises {
        basculer_epingle(slot);
    }
    if width.is_some() || height.is_some() {
        // `0.0` = jamais posé = un PANNEAU (voir `content_factor`, champ) : sa
        // taille est celle de la zone libre du dock, pas de son contenu.
        let factor = slot.with(|s| s.content_factor.get());
        // ...ET LA FENÊTRE NE DOIT PAS ÊTRE EN TRAIN DE DISPARAÎTRE : pendant
        // son fondu de sortie elle est encore « visible » (voir `is_closing`),
        // donc une mesure que la page annonce à cet instant redimensionnerait
        // une fenêtre qui s'en va — le seul cas où « visible » et « ouvert »
        // divergent, et il vient d'apparaître avec le fondu.
        let closing = is_closing(slot);
        // **LA FENÊTRE DOIT ÊTRE VISIBLE**, et ce n'est pas une optimisation : un
        // popup préchargé (voir `preload`) se mesure AUSSI en étant caché, et à ce
        // moment-là sa position n'est pas encore celle de l'ouverture — or le
        // plafond de `resize_to_content` est « la place SOUS et À DROITE de la
        // fenêtre », donc une taille calculée depuis un coin provisoire ne veut
        // rien dire. La mesure d'un popup caché est donc simplement IGNORÉE, et
        // c'est l'ouverture qui en redemande une — FORCÉE, car la garde de
        // déduplication de la page empêcherait sinon tout renvoi (voir
        // `request_content_measure`, où ce piège est expliqué).
        // ...ET CELLE QUI ARRIVE AVANT QUE LE ZOOM DU MENU NE SOIT POSÉ NON PLUS :
        // la page est encore à son zoom de préchargement (3×), sa largeur est donc
        // bornée à `fenêtre/3` — mesuré le 2026-11-10 sur `sound.html` : 323
        // unités CSS annoncées pour 966 px de fenêtre, qui auraient rétréci le
        // menu de 30 %. Ces annonces-là sont IGNORÉES, pas mises en attente : une
        // page lente les aurait sinon fait appliquer par le repli du silence,
        // avant même que la mesure au bon zoom ne parte (voir `mesure_differee`).
        let en_differe = slot.with(|s| s.mesure_differee.get().is_some());
        let applique = factor > 0.0 && is_visible(slot) && !closing && !en_differe;
        if applique {
            let w_px = width.map(|css| content_height_px(css, factor));
            let h_px = height.map(|css| content_height_px(css, factor));
            match (w_px, h_px) {
                // Les deux dimensions arrivent toujours ensemble (voir
                // `parse_content_size` et `menu.js`) : c'est le cas normal, et
                // c'est celui qui passe par la CONFIRMATION (voir
                // `appliquer_la_mesure`).
                (Some(w), Some(h)) => appliquer_la_mesure(slot, [w, h]),
                _ => resize_to_content(slot, w_px, h_px),
            }
        }
        // ⚠️ **CE DIAGNOSTIC EST INDISPENSABLE, ET SON ABSENCE A ÉTÉ TROMPEUSE.**
        // Le journal « actions CEF » plus bas est émis APRÈS le filtre qui retire
        // les actions `menu/size` : **une mesure reçue n'y apparaît donc JAMAIS**,
        // et son silence s'est lu à tort comme « la page n'a rien annoncé ». C'est
        // la même famille que la fiche « une absence de trace n'est pas une
        // preuve » — la mesure existait, l'instrument ne pouvait pas la voir.
        // ⚠️ **LE HWND EST DANS LA TRACE, ET CE N'EST PAS DU BRUIT** : cette
        // fonction est appelée pour CHAQUE popup, et treize pages préchargées
        // mesurent au démarrage — sans identité, les lignes de plusieurs popups
        // se lisent comme une seule suite de redimensionnements (payé le
        // 2026-11-10, en cherchant d'où venait un saut de 473 à 622 px).
        let hwnd = slot.with(|s| s.state.borrow().as_ref().map(|st| st.hwnd.0 as usize));
        crate::diag!(
            "[menu/size] hwnd={hwnd:?} recu w={width:?} h={height:?} | facteur={factor} visible={} fermeture={closing} differe={en_differe} -> applique={applique}",
            is_visible(slot),
        );
    }
    // ⏳ **LE REPLI DU SILENCE** — voir `appliquer_la_mesure`, issue 3. Une page
    // qui ne se mesure qu'UNE fois n'a aucune annonce à opposer à la première :
    // sans ce minuteur, sa taille ne serait jamais appliquée. Il tourne à chaque
    // frame (cette fonction est appelée pour chaque popup), pas seulement quand
    // une action arrive.
    let a_appliquer = slot.with(|s| {
        let (cible, quand) = s.mesure_en_attente.get()?;
        if quand.elapsed() < MESURE_SILENCE {
            return None;
        }
        s.mesure_en_attente.set(None);
        Some(cible)
    });
    if let Some(cible) = a_appliquer {
        crate::diag!("[menu/mesure] appliquee apres silence {cible:?}");
        resize_to_content(slot, Some(cible[0]), Some(cible[1]));
    }
    // Journal de diagnostic (voir `diag`, doc de module) : TOUTES les actions
    // de TOUS les panneaux CEF passent par ici — les `dispatch_actions` de
    // chaque `win32::cef_*` s'y alimentent, sans exception. C'est donc le seul
    // point qui les voie toutes. La garde `is_empty` est indispensable : cette
    // fonction est appelée à CHAQUE frame ET pour CHAQUE panneau ouvert.
    //
    // APRÈS le filtre : sinon le journal montre une action qu'aucun module ne
    // verra jamais, et on chercherait longtemps pourquoi.
    if !actions.is_empty() {
        crate::diag!("actions CEF ({}): {:?}", actions.len(), actions);
    }
    actions
}

/// Exécute du JavaScript dans la page de ce popup (voir
/// `win32::cef_app::CefBrowserSurface::execute_js`) — canal Rust→JS, pour
/// les popups qui ont besoin de pousser un état lu côté Rust (ex.
/// `win32::cef_network` pour Wi-Fi/Bluetooth, `win32::cef_calendar` pour les
/// évènements du mois) sans le recalculer en JS pur. No-op si la fenêtre
/// n'existe pas encore.
/// Demande à la page de re-mesurer son contenu et de l'annoncer (voir
/// `assets/popups/menu.js`) — appelée à chaque ouverture (et par les `poll` des
/// modules).
///
/// ⚠️ **CE QU'ELLE ANNONCE PENDANT QUE LA PAGE SE RECONSTRUIT N'EST PAS CRU** :
/// la première mesure d'une ouverture décrit un contenu en cours de montage
/// (mesuré le 2026-11-10 : 209×300 unités, puis 298×259, puis 296×240 une fois
/// la page prête). Elle n'est appliquée qu'une fois CONFIRMÉE par une seconde
/// annonce concordante — voir `appliquer_la_mesure`.
///
/// **LA DEMANDE EST FORCÉE (`true`), ET C'EST TOUT L'OBJET DE CE CHANGEMENT.** La
/// page garde la dernière taille qu'elle a annoncée (`lastSent`, voir
/// `assets/popups/menu.js`) et ne renvoie jamais deux fois la même — garde
/// indispensable, sans laquelle un menu qui reconstruit sa liste à chaque frame
/// (`sound.html`) déclencherait une boucle « redimensionner → la vue change →
/// mesurer → renvoyer ». Mais elle a un prix : **une mesure déjà annoncée ne peut
/// plus être redemandée.** Deux conséquences, toutes deux réelles :
/// - depuis le PRÉCHARGEMENT (`preload`), la page s'est déjà mesurée en étant
///   cachée, et cette mesure-là est IGNORÉE (voir `drain_actions`) — sans forçage
///   elle serait la seule jamais annoncée, et le popup s'ouvrirait à son
///   estimation, c'est-à-dire avec exactement le vide que la hauteur pilotée par
///   la page avait supprimé ;
/// - le rattrapage d'une action PERDUE (deux `location.href` rapprochés se
///   télescopent) ne rattrapait rien : la page croyait avoir envoyé.
///
/// **Les cas que l'observateur de la page ne couvre PAS** sont donc ceux-ci :
/// une mesure prise pendant que le popup était caché ; une action perdue ; et un
/// menu dont la taille change pour une raison que l'observateur de la page ne voit
/// pas comme une mutation — une poussée d'état venue de Rust, par exemple, ou un
/// changement de mois dans `calendar.html` (cinq ou six rangées selon le mois).
///
/// La garde `window.__ndMeasure &&` couvre le cas d'une page qui n'a pas fini de
/// charger (`__ndMeasure` pas encore défini) : c'est alors son propre écouteur
/// `load` qui mesure.
pub fn request_content_measure(slot: &'static LocalKey<PopupSlot>) {
    execute_js(slot, "window.__ndMeasure && window.__ndMeasure(true)");
}

/// **COMME `execute_js`, MAIS SANS PANIQUER SI L'ÉTAT EST DÉJÀ EMPRUNTÉ.**
///
/// ⚠️ **C'EST LA SEULE FORME UTILISABLE DEPUIS UN `wndproc`.** Un message souris
/// peut y arriver **RÉ-ENTRANT**, pendant que le code appelant détient déjà un
/// `borrow_mut` sur ce même état — `resize_if_needed` et `render` déplacent la
/// fenêtre, et Windows en profite pour livrer les messages en attente. Or
/// `execute_js` appelle `borrow()`, qui **panique** dans ce cas : « RefCell
/// already mutably borrowed », MESURÉ — plantage complet de l'application à la
/// dixième seconde (`cef_popup.rs`).
///
/// `try_borrow` rend la main sans rien faire : au pire on perd **une** annonce,
/// que le clic suivant portera. Perdre une annonce n'est pas comparable à perdre
/// l'application.
///
/// Même classe de défaut que `launcher.rs` et `cef_popup.rs` par le passé — voir
/// la fiche « RefCell reentrancy bug pattern » : dans un `wndproc`, ne JAMAIS
/// emprunter l'état sans `try_`.
fn execute_js_si_libre(slot: &'static LocalKey<PopupSlot>, code: &str) {
    slot.with(|s| {
        if let Ok(state) = s.state.try_borrow() {
            if let Some(state) = state.as_ref() {
                state.browser.execute_js(code);
            }
        }
    });
}

/// Prête le GPU du popup (device, contexte) et sa fenêtre à `f`, s'il est
/// VISIBLE — pour un rendu annexe (voir `win32::texture_terminaux`). `None` :
/// fermé, caché, ou déjà emprunté.
pub fn avec_gpu<R>(
    slot: &'static LocalKey<PopupSlot>,
    f: impl FnOnce(&ID3D11Device, &ID3D11DeviceContext, HWND) -> R,
) -> Option<R> {
    slot.with(|s| {
        let state = s.state.try_borrow().ok()?;
        let state = state.as_ref()?;
        if !unsafe { IsWindowVisible(state.hwnd) }.as_bool() {
            return None;
        }
        Some(f(&state.device, &state.context, state.hwnd))
    })
}

pub fn execute_js(slot: &'static LocalKey<PopupSlot>, code: &str) {
    slot.with(|s| {
        if let Some(state) = s.state.borrow().as_ref() {
            state.browser.execute_js(code);
        }
    });
}

fn create_window(spec: &PopupSpec, gfx: &GfxDevice, rect @ [x, y, w, h]: [i32; 4]) -> Option<CefPopupState> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?.into();
        let cursor = LoadCursorW(None, IDC_ARROW).ok()?;

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: spec.wndproc,
            hInstance: instance,
            lpszClassName: spec.class_name,
            hCursor: cursor,
            hbrBackground: HBRUSH::default(),
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);

        // `WS_EX_LAYERED` UNIQUEMENT POUR LES POPUPS QUI FONDENT (voir
        // `PopupSpec::fades`) : c'est ce style qui autorise
        // `SetLayeredWindowAttributes` (`LWA_ALPHA`), donc le fondu — et rien
        // d'autre. Les quatre panneaux ne le reçoivent pas : ils n'ont aucune
        // raison de passer par la composition des fenêtres layered, et la
        // distinction est ainsi portée par la même valeur que le fondu
        // lui-même, jamais par une convention à retenir séparément.
        let mut ex_style = WS_EX_TOPMOST | WS_EX_TOOLWINDOW;
        if spec.fades {
            ex_style |= WS_EX_LAYERED;
        }
        // Une FENÊTRE (voir `PopupSpec::fenetre`) : ni topmost ni toolwindow, et
        // `WS_EX_APPWINDOW` pour la barre des tâches et Alt-Tab.
        //
        // ⚠️ **`WS_POPUP` ET NON `WS_OVERLAPPEDWINDOW`** — DEMANDE EXPLICITE :
        // « une fenêtre dessinée avec les outils neuradesk, avec une barre de
        // titre créée avec les outils neuradesk ». La barre de titre de Windows
        // jurait avec le reste de l'application, qui n'en a nulle part. Sans
        // bordure ni barre, c'est la PAGE qui porte son en-tête, et c'est
        // `win32::cef_couleurs` qui rend le redimensionnement (`WM_NCHITTEST`)
        // et le déplacement (`HTCAPTION`) que le système ne fournit plus.
        //
        // La fenêtre reste OPAQUE (voir plus bas : ce chemin ne passe pas par
        // `make_transparent_and_uncapturable`) — donc `WDA_NONE`, visible pour
        // OBS, inchangé.
        let (ex_style, style) = if spec.fenetre { (WS_EX_APPWINDOW, WS_POPUP) } else { (ex_style, WS_POPUP) };
        // ⛔ **`WS_EX_NOACTIVATE` A ÉTÉ ESSAYÉ ICI, ET C'EST UN ÉCHEC MESURÉ.**
        // L'hypothèse : le dock reçoit la molette et les treize popups non ; le
        // dock est `WS_EX_NOACTIVATE` et les popups ne le sont pas — une fenêtre
        // qui ne PEUT pas être activée ne se défile qu'au survol, donc le shell
        // lui livrerait la molette. **Mesuré : la molette ne défile TOUJOURS
        // PAS**, et la saisie au clavier du champ de recherche est **cassée**
        // (l'activation par clic est empêchée). Retiré. Ne pas refaire : ce n'est
        // pas le style qui décide.
        // ⛔ **CHAQUE ÉCHEC DE CETTE FONCTION EST DÉSORMAIS NOMMÉ.** Elle rendait
        // `None` en silence, et son seul message (`eprintln!`) part dans le vide —
        // l'application est lancée détachée, donc sans console. Ajouter une page
        // dont la fenêtre refusait de se créer n'avait alors AUCUN diagnostic : on
        // voyait un clic sans effet, et rien de plus.
        let hwnd = match CreateWindowExW(
            ex_style,
            spec.class_name,
            spec.window_title,
            style,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(instance),
            None,
        ) {
            Ok(hwnd) => hwnd,
            Err(err) => {
                crate::diag!("[cef] CreateWindowExW a echoue pour {} : {err}", spec.html_file);
                return None;
            }
        };

        // `make_transparent_and_uncapturable` (voir sa doc) fait DÉJÀ le
        // `DwmExtendFrameIntoClientArea` nécessaire — BUG RÉEL TROUVÉ en
        // retirant `win32::egui_overlay` (Phase 7) : un second appel
        // identique traînait ici depuis toujours, strictement redondant
        // (mêmes marges), retiré au passage.
        //
        // Une FENÊTRE n'a pas de verre (sa page est opaque, et le verre étendu
        // sous une vraie barre de titre la rendrait transparente) mais garde la
        // même règle de capture que tout le reste de l'application.
        // Une FENÊTRE a le MÊME fond que tout le reste de NeuraDesk — le bureau
        // flouté sous une page transparente (DEMANDE EXPLICITE : « une fenêtre
        // dessinée avec les outils neuradesk »). Elle garde en plus son icône
        // d'exécutable, pour la barre des tâches et Alt-Tab.
        //
        // ⚠️ **`make_transparent_and_uncapturable` N'IMPOSE RIEN POUR OBS** :
        // elle délègue à `capture_visibility::apply_to_window`, qui suit le
        // RÉGLAGE de l'utilisateur. Croire le contraire m'a fait annoncer à tort
        // que le flou était « incompatible avec OBS ».
        if spec.fenetre {
            make_transparent_and_uncapturable(hwnd);
            crate::win32::window::apply_exe_icon(hwnd);
        } else {
            make_transparent_and_uncapturable(hwnd);
        }
        // Le swap chain et la page prennent la taille de la ZONE CLIENTE, plus
        // petite que `w`×`h` d'une fenêtre à bordures (barre de titre comprise).
        let (w, h) = if spec.fenetre { taille_client(hwnd).unwrap_or((w, h)) } else { (w, h) };

        // ⭐ **MODÈLE BitBlt, DONC ALPHA COMPOSÉ PAR DWM** — DEMANDE EXPLICITE
        // du 2026-11-12 : « rendre l'app visible sur les captures même quand la
        // transparence est activée ». Quand la LOUPE (`gfx::mag_capture`) ne
        // fournit pas de fond, le shader sort l'alpha de l'utilisateur
        // (`sans_fond`, `gfx::quad_renderer`) ; en modèle *flip* cet alpha est
        // ignoré et la fenêtre reste un aplat opaque assombri — voir
        // `gfx::swapchain::SwapChain::new`.
        let swapchain = match SwapChain::new(&gfx.device, hwnd, w.max(1) as u32, h.max(1) as u32, true) {
            Ok(swapchain) => swapchain,
            Err(err) => {
                crate::diag!("[cef] SwapChain::new a echoue pour {} ({w}x{h}) : {err}", spec.html_file);
                return None;
            }
        };
        let quad_renderer = match QuadRenderer::new(&gfx.device) {
            Ok(renderer) => renderer,
            Err(err) => {
                crate::diag!("[cef] QuadRenderer::new a echoue pour {} : {err}", spec.html_file);
                return None;
            }
        };

        let Some(html_path) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join("assets/popups").join(spec.html_file))) else {
            crate::diag!("[cef] le chemin de {} n'a pas pu etre resolu", spec.html_file);
            return None;
        };
        if !html_path.is_file() {
            crate::diag!("[cef] LA PAGE EST INTROUVABLE : {}", html_path.display());
        }
        let url = format!("file:///{}", html_path.display().to_string().replace('\\', "/"));
        eprintln!("[neuradesk][cef] chargement {url}");
        let browser = CefBrowserSurface::new(gfx.device.clone(), w.max(1), h.max(1), &url, content_zoom_level());

        // Menu ou panneau, connu dès la création (voir `MENUS`) : un panneau doit
        // passer SOUS le dock dès sa première apparition (voir `sous_le_dock`).
        noter(if spec.above_a_magnified_dock { &MENUS } else { &PANNEAUX }, hwnd);
        Some(CefPopupState {
            hwnd,
            rect,
            device: gfx.device.clone(),
            context: gfx.context.clone(),
            swapchain,
            quad_renderer,
            browser,
            backdrop: None,
            applied_region: None,
            peinture_vue: 0,
            visible_notifiee: true,
            pending_show: None,
            // ⚠️ **`false` ICI, ET LE SITE D'OUVERTURE LE REPOSE AUSSITÔT** :
            // `create_window` ne connaît pas l'intention (voir
            // `set_ouverture_sans_focus`), c'est `toggle_with` qui la lui passe.
            sans_focus: false,
            above_a_magnified_dock: spec.above_a_magnified_dock,
            // Voir `PopupSpec::fades` : le fondu existe si et seulement si la
            // fenêtre a reçu `WS_EX_LAYERED` juste au-dessus.
            fade: spec.fades.then(fade::WindowFade::new),
            fenetre: spec.fenetre,
            // Une fenêtre qui vient d'être créée est à son `rect` — mais elle n'a
            // pas encore été MONTRÉE (voir `montrer_fenetre`).
            montee: false,
        })
    }
}

/// Taille de la zone cliente de `hwnd`, `None` si elle est vide (fenêtre
/// réduite dans la barre des tâches : `GetClientRect` rend alors 0×0).
fn taille_client(hwnd: HWND) -> Option<(i32, i32)> {
    let mut rc = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rc).ok()? };
    let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
    (w > 0 && h > 0).then_some((w, h))
}

/// Ouvre une FENÊTRE (voir `PopupSpec::fenetre`) ou la ramène devant — jamais
/// ne la ferme : c'est sa croix qui le fait (le module la CACHE sur `WM_CLOSE`,
/// voir `win32::cef_image_editor`).
///
/// `rect` n'est lu qu'à la CRÉATION : ensuite, la fenêtre reste où
/// l'utilisateur l'a laissée, à la taille qu'il lui a donnée. Rend `true` si la
/// fenêtre vient d'être créée (sa page charge encore).
///
/// Même discipline d'emprunt que `toggle_with` : le `HWND` est lu, l'emprunt
/// RELÂCHÉ, et seulement ensuite viennent `ShowWindow`/`force_foreground`, qui
/// dispatchent `WM_SETFOCUS` au wndproc de cette même fenêtre.
///
/// **FAUT-IL APPLIQUER LE `rect` À CETTE MONTE ?** — toute la règle de
/// `montrer_fenetre`, extraite PURE pour être vérifiable sans fenêtre ni GPU
/// (même raison d'être que `clamped_content_height` ou `code_pour_point`).
///
/// Trois cas, et le premier n'existait pas avant le 2026-10-06 :
/// - **`!montee && !creee`** ⇒ **OUI.** La fenêtre a été créée par `preload` à
///   `PRELOAD_RECT`, et l'appelant lui apporte enfin sa vraie place. C'est la
///   correction : sans elle, le `rect` était jeté et la fenêtre s'ouvrait au coin.
/// - `creee` ⇒ non, `create_window` a déjà passé ce `rect` au système.
/// - `montee` ⇒ non, la place appartient à l'utilisateur.
fn faut_appliquer_le_rect(creee: bool, montee: bool, rect_actuel: [i32; 4], rect: [i32; 4]) -> bool {
    !montee && !creee && rect_actuel != rect
}

/// **LE RECTANGLE TIENT-IL ENTIÈREMENT DANS L'ÉCRAN ?** — `[x, y, largeur, hauteur]`
/// des deux côtés, comme partout dans ce dépôt.
///
/// ⚠️ **SÉPARÉE POUR ÊTRE VÉRIFIABLE** : elle décide si une fenêtre est
/// récupérable, et une erreur d'un pixel au bord la ferait fermer pour rien.
fn tient_dans(rect: [i32; 4], ecran: [i32; 4]) -> bool {
    let [x, y, l, h] = rect;
    let [ex, ey, el, eh] = ecran;
    x >= ex && y >= ey && x + l <= ex + el && y + h <= ey + eh
}

/// **RÉARME LA MONTE D'UN POPUP DONT LE RECTANGLE NE TIENT PLUS DANS SON ÉCRAN.**
///
/// ⛔ **LE DÉFAUT QU'ELLE FERME, ET IL A SURVÉCU À TOUT.** `faut_appliquer_le_rect`
/// ne repose le rectangle qu'à la PREMIÈRE monte — « la place appartient à
/// l'utilisateur ». Une fenêtre ouverte pendant un **changement d'écran** garde donc
/// le sien : passée d'un 4K à un 1080, elle finit **615 px sous le bord bas**,
/// hors d'atteinte, sans autre recours que de la rouvrir.
///
/// ⛔ **ET LA CACHER NE SUFFISAIT PAS** : `montee` restant vrai, la rouvrir
/// rejouait le MÊME rectangle périmé. D'où la remise à `false` — la prochaine
/// ouverture replace la fenêtre, à la taille ET à la position du nouvel écran.
///
/// ⭐ **ELLE NE TOUCHE À RIEN TANT QUE ÇA TIENT.** Un popup qui tient encore dans
/// le nouvel écran garde sa place : même doctrine que `menu_rect` (« la réduction
/// n'a lieu que si ça ne tient pas »), appliquée à la POSITION.
///
/// ⚠️ **LES CONCERNÉES SONT LES QUATRE VRAIES FENÊTRES** — caractères, kaomojis,
/// visionneuse, couleurs. Les panneaux et les menus, eux, se ferment à la perte du
/// focus, et `hide_overlay_windows` les couvre déjà.
pub fn recaler_si_il_ne_tient_plus(slot: &'static LocalKey<PopupSlot>) {
    slot.with(|s| {
        let mut state_ref = s.state.borrow_mut();
        let Some(state) = state_ref.as_mut() else { return };
        if !state.montee {
            return;
        }
        let ecran = crate::win32::display::monitor_bounds(state.hwnd);
        // `monitor_bounds` rend `[0,0,0,0]` quand il échoue : ne rien faire plutôt
        // que de fermer une fenêtre sur une mesure ratée.
        if ecran[2] <= 0 || ecran[3] <= 0 || tient_dans(state.rect, ecran) {
            return;
        }
        crate::diag!(
            "[cef] rectangle {:?} hors de l'ecran {:?} (changement d'ecran) : la monte est REARMEE",
            state.rect,
            ecran
        );
        let hwnd = state.hwnd;
        state.montee = false;
        cacher_fenetre(hwnd);
    });
}

#[cfg(test)]
mod tests_premiere_monte {
    use super::*;

    /// ⛔ **UNE FENÊTRE PRÉCHARGÉE NAÎT HORS DE TOUT ÉCRAN** — voir `PRELOAD_RECT` :
    /// à `[0, 0, …]`, les fenêtres cachées mangeaient le clic simple des
    /// applications du coin haut-gauche. `-32000` est au-delà de tout écran
    /// plausible, y compris ceux placés à gauche ou au-dessus du principal.
    /// ⛔ **UNE FENÊTRE CACHÉE EST GARÉE LÀ OÙ NAÎT UNE PRÉCHARGÉE** — hors de tout
    /// écran. Les écrans placés à gauche ou au-dessus du principal ont des
    /// coordonnées NÉGATIVES (relevé du 2026-11-01 : un écran à x = −1920) : ils ne
    /// doivent pas passer pour garés.
    #[test]
    fn une_fenetre_cachee_est_garee_hors_de_tout_ecran() {
        assert!(est_garee(PRELOAD_RECT[0], PRELOAD_RECT[1]));
        assert!(!est_garee(-1920, 484), "un écran secondaire à gauche n'est pas une place garée");
        assert!(!est_garee(906, 176));
        assert!(!est_garee(-32000, 300), "il faut les DEUX coordonnées garées");
    }

    /// ⭐ **ROUVERTE, UNE FENÊTRE GARÉE REVIENT À SA DERNIÈRE PLACE** — et une
    /// fenêtre déjà sur un écran ne bouge pas : la place appartient à l'utilisateur.
    #[test]
    fn une_fenetre_garee_revient_a_sa_derniere_place() {
        let derniere = [906, 176, 748, 1088];
        assert_eq!(place_de_retour((-32000, -32000), derniere), Some((906, 176)));
        assert_eq!(place_de_retour((-1655, 662), [-1655, 662, 400, 300]), None);
        assert_eq!(place_de_retour((906, 176), derniere), None);
        // Une dernière place elle-même garée ne ramène rien : mieux vaut la
        // laisser où elle est que la « ramener » au même endroit hors écran.
        assert_eq!(place_de_retour((-32000, -32000), PRELOAD_RECT), None);
    }

    #[test]
    fn une_fenetre_prechargee_nait_hors_de_tout_ecran() {
        let [x, y, l, h] = PRELOAD_RECT;
        assert!(x + l < -16000 && y + h < -16000, "PRELOAD_RECT {PRELOAD_RECT:?} touche un écran possible");
    }

    /// ⛔ **LE PRÉDICAT QUI DÉCIDE SI UNE FENÊTRE EST RÉCUPÉRABLE.** Une erreur d'un
    /// pixel au bord la ferait fermer pour rien — ou pire, laisserait une fenêtre
    /// hors d'atteinte sans que rien ne le dise.
    #[test]
    fn tient_dans_est_exact_aux_quatre_bords() {
        let ecran = [0, 0, 1920, 1080];
        // Exactement dedans, y compris collé aux quatre bords.
        assert!(tient_dans([0, 0, 1920, 1080], ecran), "pile l'ecran entier");
        assert!(tient_dans([100, 100, 100, 100], ecran));
        assert!(tient_dans([1820, 980, 100, 100], ecran), "colle au coin bas-droit");
        // ⛔ Un pixel de trop, de chaque côté — c'est le cas qui compte.
        assert!(!tient_dans([1821, 980, 100, 100], ecran), "deborde a droite");
        assert!(!tient_dans([100, 981, 100, 100], ecran), "deborde en bas");
        assert!(!tient_dans([-1, 100, 100, 100], ecran), "deborde a gauche");
        assert!(!tient_dans([100, -1, 100, 100], ecran), "deborde en haut");
        // ⛔ **LE CAS RÉEL** : la fenêtre des caractères restée sur un 4K, mesurée
        // sur un 1080 — 150 px sous le bord bas (voir `recaler_si_il_ne_tient_plus`).
        assert!(!tient_dans([30, 0, 1860, 1230], [0, 0, 1920, 1080]));
        // Et un écran dégénéré — `monitor_bounds` rend `[0,0,0,0]` quand il échoue —
        // n'est jamais « tenable » : c'est l'appelant qui décide alors de ne RIEN
        // faire, plutôt que de fermer sur une mesure ratée.
        assert!(!tient_dans([0, 0, 10, 10], [0, 0, 0, 0]));
    }

    /// ⭐ **LE DÉFAUT DU 2026-10-06, EN UN TEST.** La fenêtre des caractères est
    /// PRÉCHARGÉE — donc créée à `PRELOAD_RECT` au lancement — et son `rect`
    /// d'ouverture était jeté : elle s'affichait en haut à gauche en 1024×768, et
    /// **deux corrections successives du calcul de sa place n'y ont rien changé**.
    ///
    /// ⛔ **CE TEST ÉCHOUE SUR L'ANCIEN CODE** : les trois cas y rendaient `false`,
    /// puisque `montrer_fenetre` ne touchait jamais à la position d'une fenêtre
    /// existante. Le cas 1 est celui qui portait le défaut.
    #[test]
    fn le_rect_ne_s_applique_qu_a_la_premiere_monte_d_une_fenetre_prechargee() {
        let centre = [1680, 910, 480, 340];
        // 1. PRÉCHARGÉE, jamais montrée : OUI. C'est la correction.
        assert!(
            faut_appliquer_le_rect(false, false, PRELOAD_RECT, centre),
            "une fenêtre préchargée doit rejoindre la place qu'on lui donne"
        );
        // 2. Créée par cet appel : `create_window` a déjà le `rect`.
        assert!(!faut_appliquer_le_rect(true, false, centre, centre));
        // 3. DÉJÀ MONTÉE : la place appartient à l'utilisateur.
        assert!(
            !faut_appliquer_le_rect(false, true, PRELOAD_RECT, centre),
            "rouvrir une fenêtre ne doit pas la ramener de force à sa place de naissance"
        );
        // Et une fenêtre préchargée qui serait DÉJÀ au bon endroit n'est pas
        // repositionnée pour rien (le préchargement ne la met jamais là, mais la
        // comparaison est ce qui rend le cas 2 et ce cas-ci une seule règle).
        assert!(!faut_appliquer_le_rect(false, false, centre, centre));
    }
}

#[cfg(test)]
mod tests_couleur_des_pages {
    use std::fs;

    /// ⭐ **TOUTE PAGE DE PANNEAU PREND SA COULEUR DE TEXTE DU THÈME.**
    ///
    /// Une page qui charge `style.css` **sans** `menu.css` n'hérite d'aucune
    /// couleur : `style.css` n'en pose pas, et si la page ne le fait pas elle-même,
    /// le navigateur retombe sur **son défaut — le noir**.
    ///
    /// ⛔ **C'EST EXACTEMENT CE QUI EST ARRIVÉ À `caracteres.html`**, et il a fallu
    /// DEUX demandes de l'utilisateur pour le trouver (« elle devrait suivre le thème
    /// de l'app », puis « maintenant il faudrait que la fenêtre suive le thème ») :
    /// j'avais d'abord corrigé la DÉCORATION — la punaise emoji, le filet de la barre,
    /// la police des glyphes — alors que la couleur de base n'était pas posée du tout.
    /// Les 60 caractères, le titre et les deux boutons étaient **noirs** sur le bureau
    /// flouté, quand les 18 autres pages de panneau étaient justes.
    ///
    /// ⚠️ **LA RÈGLE EST DÉDUITE DES FICHIERS, PAS D'UNE LISTE.** On lit qui charge
    /// quoi dans chaque page : une liste écrite à la main raterait la prochaine page,
    /// et ce dépôt a déjà payé deux fois les listes tenues à la main (voir
    /// [[neuradesk-enumeration-de-barres-a-la-main]]).
    ///
    /// ⛔ **CE TEST ÉCHOUE SUR L'ANCIENNE PAGE** : elle ne contenait aucune de ces
    /// deux formes.
    #[test]
    fn toute_page_de_panneau_prend_sa_couleur_du_theme() {
        let mut pages = 0;
        let mut fautives = Vec::new();
        for entree in fs::read_dir("assets/popups").expect("le dossier des pages") {
            let chemin = entree.expect("une entrée lisible").path();
            if chemin.extension().and_then(|e| e.to_str()) != Some("html") {
                continue;
            }
            let page = fs::read_to_string(&chemin).expect("une page lisible");
            // PANNEAU = `style.css` seul. Un MENU charge `menu.css` en plus, et c'est
            // LUI qui porte sa couleur — d'où la condition, plutôt qu'un nom de fichier
            // en dur.
            if !(page.contains("href=\"style.css\"") && !page.contains("href=\"menu.css\"")) {
                continue;
            }
            pages += 1;
            let pose_sa_couleur = page.contains("color: rgb(var(--popup-fg-rgb")
                || page.contains("color: rgba(var(--popup-fg-rgb");
            if !pose_sa_couleur {
                fautives.push(chemin.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
        // ⛔ **UN BALAYAGE QUI NE TROUVE RIEN PASSE EN SILENCE** — même piège que la
        // sonde aveugle. Un plancher fait échouer bruyamment un scan qui n'a pas vu le
        // dossier (mauvais répertoire courant, dossier renommé).
        assert!(pages >= 15, "le balayage n'a vu que {pages} pages de panneau — dossier ou CWD en cause");
        assert!(fautives.is_empty(), "pages de panneau sans couleur de thème : {fautives:?}");
    }
}

/// Montre une FENÊTRE (voir `PopupSpec::fenetre`), en la créant au besoin.
///
/// ⛔ **REND `true` QUAND ELLE VIENT D'ÊTRE CRÉÉE — PAS « RÉUSSIE ».** Une fenêtre
/// réutilisée rend `false` ; l'échec de création aussi, et seul `hwnd(slot)`
/// les distingue. `cef_visionneuse` lisait ce `false` comme un échec, et n'affichait plus
/// une nouvelle image dans une fenêtre déjà servie (corrigé le 2026-10-23).
#[allow(dead_code)]
pub fn montrer_fenetre(slot: &'static LocalKey<PopupSlot>, spec: &PopupSpec, rect: [i32; 4], gfx: &GfxDevice) -> bool {
    // Consommée à chaque appel, comme dans `toggle_with`.
    let origine = prendre_origine_ouverture();
    register(slot);
    let existante = slot.with(|s| s.state.borrow().as_ref().map(|state| state.hwnd));
    let (hwnd, creee) = match existante {
        Some(hwnd) => (hwnd, false),
        None => {
            let Some(state) = create_window(spec, gfx, rect) else { return false };
            let hwnd = state.hwnd;
            slot.with(|s| *s.state.borrow_mut() = Some(state));
            (hwnd, true)
        }
    };
    // ⛔ **LE `rect` EST APPLIQUÉ ICI, À LA PREMIÈRE MONTE** — et pour une fenêtre
    // PRÉCHARGÉE, c'est le SEUL moment où il peut l'être.
    //
    // `preload` **crée** la fenêtre, à `PRELOAD_RECT` (1024×768, HORS de tout écran) : au
    // premier `montrer_fenetre`, elle existe donc déjà et le `rect` de l'appelant —
    // sa vraie place, sa vraie taille — était **silencieusement jeté**. Défaut réel
    // signalé le 2026-10-06 : « la fenêtre devrait s'afficher au centre de
    // l'écran », et elle s'ouvrait en haut à gauche en 1024×768, **quelles que
    // soient** les coordonnées qu'on lui calculait. Deux corrections successives du
    // calcul n'y ont rien changé, et c'est ce qui a mis sur la voie : le journal
    // portait le bon `rect` pendant que la fenêtre restait au coin.
    //
    // **Les deux autres `fenetre: true` ne le voyaient pas** — `cef_visionneuse` et
    // `cef_couleurs` ne sont PAS dans `app::preload_queue` : elles naissent dans ce
    // même appel, avec le bon `rect`. C'est toute la différence, et c'est pour ça
    // que le défaut ne se voyait que sur celle-ci.
    //
    // **ENSUITE, LA PLACE APPARTIENT À L'UTILISATEUR** — la règle que porte la doc
    // de cette fonction depuis toujours reste vraie : une fenêtre ROUVERTE garde où
    // l'utilisateur l'a mise. D'où le drapeau, et non un repositionnement à chaque
    // ouverture (qui interdirait de la déplacer, comme `resize_if_needed` appelé à
    // chaque frame le faisait pour la taille).
    let premiere_monte = slot.with(|s| {
        let mut sr = s.state.borrow_mut();
        let Some(state) = sr.as_mut() else { return false };
        let appliquer = faut_appliquer_le_rect(creee, state.montee, state.rect, rect);
        // POSÉ DANS TOUS LES CAS : une fenêtre est montée à partir d'ici, quoi
        // qu'on décide de sa place.
        state.montee = true;
        if !appliquer {
            return false;
        }
        // Le redimensionnement de la SURFACE se fait SOUS l'emprunt, comme dans
        // `toggle_with` : `resize` ne fait que des appels D3D11/CEF internes,
        // aucun des trois appels Win32 (`SetWindowPos`/`ShowWindow`/
        // `force_foreground`) qui imposent de relâcher l'emprunt avant d'exécuter.
        if state.rect[2] != rect[2] || state.rect[3] != rect[3] {
            let _ = state.swapchain.resize(&state.device, rect[2].max(1) as u32, rect[3].max(1) as u32);
            state.browser.resize(rect[2].max(1), rect[3].max(1));
            // La montre est DIFFÉRÉE d'une frame, le temps que la page se remette
            // en page — même mesure et même remède que `toggle_with`, qui porte le
            // pourquoi (« le noir que cette ligne fait apparaître »).
            state.pending_show = Some(Instant::now());
        }
        state.rect = rect;
        true
    });
    if premiere_monte {
        let [x, y, w, h] = rect;
        unsafe {
            let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    } else {
        // Rouverte après `cacher_et_garer` : elle revient là où l'utilisateur
        // l'avait laissée — `state.rect` suit sa place tant qu'elle est visible
        // (voir `render`), et n'y retient jamais une position garée.
        let derniere_place = slot.with(|s| s.state.borrow().as_ref().map(|state| state.rect));
        let mut cadre = RECT::default();
        if let Some(derniere_place) = derniere_place {
            if unsafe { GetWindowRect(hwnd, &mut cadre) }.is_ok() {
                if let Some((x, y)) = place_de_retour((cadre.left, cadre.top), derniere_place) {
                    unsafe {
                        let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                    }
                }
            }
        }
    }
    // L'effet d'ouverture — DÉFAUT SIGNALÉ (2026-10-16) : « l'effet ne s'applique
    // toujours pas aux fenêtres telles que les kaomojis ». Une FENÊTRE ne passe pas
    // par `toggle_with`. Seulement si elle était cachée : une fenêtre réduite qu'on
    // restaure est déjà visible pour Windows.
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        animer_l_ouverture(spec, hwnd, origine);
    }
    // ⚠️ **LA MONTRER ICI ANNULERAIT LA DIFFÉRATION** : c'est `render_if_visible`
    // qui s'en charge, une fois `pending_show` écoulé (voir son `a_montrer`).
    let differee = slot.with(|s| s.state.borrow().as_ref().is_some_and(|st| st.pending_show.is_some()));
    if !differee {
        unsafe {
            let _ = ShowWindow(hwnd, if IsIconic(hwnd).as_bool() { SW_RESTORE } else { SW_SHOW });
        }
    }
    force_foreground(hwnd);
    slot.with(|s| {
        if let Some(state) = s.state.borrow().as_ref() {
            state.browser.set_focus(true);
        }
    });
    creee
}

/// Cache une FENÊTRE sans la détruire — sa page, son image et son historique
/// d'annulation survivent jusqu'à la prochaine ouverture. À appeler sur
/// `WM_CLOSE` (la croix) à la place de `DefWindowProcW`, qui la DÉTRUIRAIT sous
/// un état encore vivant dans le `thread_local`.
#[allow(dead_code)]
pub fn cacher_fenetre(hwnd: HWND) {
    cacher_et_garer(hwnd);
}

/// Le rayon des coins des fenêtres d'APPLET — DEMANDE (2026-11-01) : « mettre
/// des coins arrondis aux fenêtres des applets ». C'est le rayon des menus
/// (curseur « Arrondi des menus », déjà mis à l'échelle) : un seul réglage
/// gouverne les deux, et un menu ouvert par-dessus une applet a les mêmes
/// coins qu'elle. Le Dash, les Réglages, l'Aide et le bandeau gardent `0.0`.
/// Une fenêtre AGRANDIE repasse à angles droits (voir `render`).
pub fn rayon_d_applet() -> f32 {
    crate::win32::menu::menu_corner_radius()
}

/// Dessine (et présente) la fenêtre si elle existe et est visible — appelée
/// à chaque frame, même endroit que `dash::render_if_visible`. `radius` :
/// rayon des coins arrondis du fond flouté (voir `QuadInstance::
/// new_backdrop`) — `0.0` pour les popups "fenêtre" (Dash/Réglages/
/// Presse-papiers/fenêtre de test), `win32::menu::menu_corner_radius()`
/// pour les popups "menu" (voir chaque module popup pour ce qu'il passe).
/// `palette` : ce que la page reçoit EN PLUS du fond flouté, et sous quelle
/// forme — voir `PopupPalette`. Un popup "menu" y passe son niveau de zoom
/// (réappliqué chaque frame, ce qui permet au curseur « Taille des menus »
/// d'agir sur un menu DÉJÀ ouvert) ; un popup "fenêtre" y passe l'accent de sa
/// fenêtre, dont `push_window_palette` dérive ses variables CSS.
///
/// Le zoom d'un popup "fenêtre" est lui AUSSI réappliqué ici à chaque frame,
/// mais pour une raison DIFFÉRENTE : non pour suivre un curseur (il ne change
/// jamais), mais parce que l'appel unique du chargement ne tient pas pour
/// `notes.html` — voir le `match` de `render`, qui porte la mesure.
pub fn render_if_visible(slot: &'static LocalKey<PopupSlot>, tint_rgba: [f32; 4], radius: f32, palette: PopupPalette) {
    // **LE MASQUAGE DE FIN DE FONDU SORT DE L'EMPRUNT — DÉLIBÉRÉMENT.** Il
    // rend le `HWND` à cacher, et l'appel `ShowWindow` a lieu APRÈS, hors du
    // `slot.with`. La raison est la classe de bug documentée de ce dépôt :
    // `ShowWindow(SW_HIDE)` sur la fenêtre au premier plan peut dispatcher
    // `WM_KILLFOCUS`/`WM_ACTIVATE` de façon SYNCHRONE vers le wndproc de cette
    // même fenêtre (voir `dispatch_input`, cas `WM_SETFOCUS | WM_KILLFOCUS`),
    // lequel emprunte `state` PARTAGÉ — donc sous l'emprunt MUTABLE que
    // `render` tient ici, il paniquerait (« RefCell already mutably
    // borrowed »), et `panic = "abort"` en fait un arrêt net. Le faire tenir
    // dans une variable de retour est le seul moyen de garder l'appel là où il
    // doit être — à la fin du fondu — sans le payer de ce risque.
    let faded_out = slot.with(|s| {
        let mut state = s.state.borrow_mut();
        let Some(state) = state.as_mut() else { return None };
        // LE FACTEUR CSS → PHYSIQUE DE CE POPUP (voir `content_factor`, champ) —
        // posé ici, dans le SEUL endroit qui reçoit un `PopupPalette::Menu`.
        //
        // `1.2^zoom_level` est l'inverse EXACT de `zoom_level_for_factor`, donc
        // `CONTENT_ZOOM_FACTOR × menu::menu_scale() × content_scale` — la quantité
        // voulue, sans que ce module ait à connaître aucun des trois ni à recevoir
        // un paramètre de plus (`toggle` est appelé par 9 modules, 4 panneaux et
        // 8 sites d'`app.rs` : le lui ajouter serait une onde de choc pour rien).
        //
        // L'ORDRE EST SÛR : `app::run` appelle `render_if_visible` AVANT
        // `drain_actions` de la même frame, donc le facteur est toujours frais.
        if let PopupPalette::Menu { zoom_level } = palette {
            s.content_factor.set(1.2_f64.powf(zoom_level));
        }
        unsafe {
            // ⛔ **DIRE À CHROMIUM QU'UNE PAGE EST CACHÉE — SINON ELLE PEINT
            // QUAND MÊME.** Mesuré le 2026-11-10 : les treize pages PRÉCHARGÉES
            // produisaient ~70 peintures/s, fenêtres hors écran, travail inutile
            // dans le processus navigateur. Un seul appel, au CHANGEMENT d'état.
            let visible = IsWindowVisible(state.hwnd).as_bool();
            if visible != state.visible_notifiee {
                state.visible_notifiee = visible;
                state.browser.set_visible(visible);
            }
            if visible {
                // LE FONDU AVANCE ICI, et c'est le seul endroit qui le fait :
                // cette fonction est appelée à CHAQUE frame pour chaque popup
                // qui existe, donc un seul appel suffit à couvrir l'apparition,
                // la disparition, et le fait de cacher la fenêtre quand elle
                // s'est éteinte. `fade_ms` est relu à chaque pas, donc le
                // curseur des Réglages agit à chaud, y compris sur un menu
                // DÉJÀ ouvert (même propriété que le zoom, réappliqué
                // ci-dessus — et pour la même raison : c'est ce module qui
                // reçoit la palette, pas `SettingsRefs`).
                if let Some(window_fade) = state.fade.as_mut() {
                    window_fade.tick(state.hwnd, fade::menu_fade_ms());
                    if window_fade.is_hidden() {
                        // Rien à redessiner : la fenêtre s'en va (voir le
                        // commentaire d'en-tête pour le pourquoi de ce retour
                        // plutôt que d'un `ShowWindow` ici).
                        return Some(state.hwnd);
                    }
                }
                let zoom = slot.with(|s| (s.zoom_suit_le_dpi.get(), s.facteur_textes.get()));
                render(state, tint_rgba, radius, palette, zoom);
                // ⭐ **LA PUNAISE, REPOSÉE À CHAQUE FRAME** — même raison que le zoom
                // juste au-dessus : la page ne mémorise rien, donc c'est ici, et
                // seulement ici, que son état est tenu à jour.
                // ⛔ **PAS DE PUNAISE INJECTÉE SUR UNE FENÊTRE.** `js_epingle` pose un
                // bouton 📌 dans la barre — utile aux PANNEAUX ancrés au dock, qui en
                // ont besoin pour rester ouverts. Une `fenetre: true` (visionneuse,
                // sélecteur de couleurs, caractères) **porte DÉJÀ la sienne** dans sa
                // page, et l'état de celle-là ne vient pas de `slot.epingle` mais du
                // réglage de la fenêtre : la punaise injectée ne commande donc RIEN,
                // en plus d'être un emoji coloré dans une fenêtre qui n'a que des
                // teintes de thème.
                //
                // MESURÉ le 2026-10-06 par capture : la fenêtre des caractères
                // montrait DEUX punaises côte à côte, sa `◆` et un 📌 rouge — « elle
                // devrait suivre le thème de l'app ».
                //
                // ⚠️ **C'ÉTAIT DÉJÀ ÉCRIT, ET LE CODE DISAIT LE CONTRAIRE** — voir
                // `PopupSpec::fenetre` : « une fenêtre ne reçoit pas la punaise
                // injectée ». La doc décrivait l'intention, pas le comportement.
                if !state.fenetre {
                    state.browser.execute_js(&js_epingle(slot.with(|s| s.epingle.get())));
                }
            }
        }
        None
    });
    let a_montrer = slot.with(|s| {
        let mut sr = s.state.borrow_mut();
        let Some(state) = sr.as_mut() else { return None };
        let Some(depuis) = state.pending_show else { return None };
        if depuis.elapsed() < Duration::from_millis(33) { return None }
        state.pending_show = None;
        // **LE VERBE VIENT DE L'ÉTAT, PAS D'UN LITTÉRAL** — c'est le troisième
        // chemin qui montre une fenêtre, et il ne peut pas lire l'intention à
        // usage unique de `toggle_with` (voir `set_ouverture_sans_focus`). Sans
        // cette relecture, une carte de notification ouverte après un
        // redimensionnement prenait le premier plan par CE chemin, et le défaut
        // serait resté vivant à temps partiel.
        Some((state.hwnd, verbe_de_show(state.sans_focus)))
    });
    if let Some((hwnd, verbe)) = a_montrer {
        unsafe { let _ = ShowWindow(hwnd, verbe); }
        // ⭐ **LE PREMIER PLAN SE REJOUE ICI.** `montrer_fenetre` a pu appeler
        // `force_foreground` alors que la fenêtre n'était PAS ENCORE montrée : à
        // la première monte d'une fenêtre PRÉCHARGÉE, le passage de 1024×768 à la
        // taille réelle pose `pending_show` et le `ShowWindow` est différé ici
        // même. `SetForegroundWindow` sur une fenêtre non montrée ne peut rien —
        // c'est la classe de défaut déjà signalée (« un troisième chemin de show
        // qu'on ne pense pas à corriger »), MESURÉE le 2026-10-22 sur la
        // kaomoji. Le rejeu ne se fait QUE quand le verbe est ACTIVANT : une
        // carte de notification (`sans_focus`, `SW_SHOWNOACTIVATE`) montre sans
        // prendre le premier plan — le lui rendre par ce chemin serait le défaut
        // inverse, et `verbe_de_show` porte déjà cette intention.
        if verbe == SW_SHOW || verbe == SW_RESTORE {
            force_foreground(hwnd);
        }
    }
    if let Some(hwnd) = faded_out {
        cacher_et_garer(hwnd);
    }
}

thread_local! {
    /// Les fenêtres des MENUS et celles des PANNEAUX (Dash, Réglages, applets) —
    /// départagés par `PopupSpec::above_a_magnified_dock`, relevés à la création
    /// (`create_window`). Lus par `win32::ombre_barre` (seuls les menus passent
    /// au-dessus des ombres, DEMANDE EXPLICITE du 2026-10-23) et par
    /// `sous_le_dock` (un panneau ne passe jamais devant le dock).
    static MENUS: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) };
    static PANNEAUX: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) };
}

fn noter(liste: &'static LocalKey<RefCell<Vec<HWND>>>, hwnd: HWND) {
    liste.with(|m| {
        if let Ok(mut m) = m.try_borrow_mut() {
            if !m.contains(&hwnd) {
                m.push(hwnd);
            }
        }
    });
}

fn visibles_ou_non(liste: &'static LocalKey<RefCell<Vec<HWND>>>) -> Vec<HWND> {
    liste.with(|m| m.try_borrow().map(|m| m.clone()).unwrap_or_default())
}

fn visibles(liste: &'static LocalKey<RefCell<Vec<HWND>>>) -> Vec<HWND> {
    liste.with(|m| m.try_borrow().map(|m| m.iter().copied().filter(|&h| unsafe { IsWindowVisible(h) }.as_bool()).collect()).unwrap_or_default())
}

/// Les menus actuellement visibles — voir `MENUS`.
pub fn menus_visibles() -> Vec<HWND> {
    visibles(&MENUS)
}

/// Les panneaux actuellement visibles — voir `PANNEAUX`.
pub fn panneaux_visibles() -> Vec<HWND> {
    visibles(&PANNEAUX)
}

fn render(state: &mut CefPopupState, tint_rgba: [f32; 4], radius: f32, palette: PopupPalette, (zoom_dpi, facteur_textes): (bool, f32)) {
    // Voir la doc de `render_if_visible` — no-op visuel si `level` n'a pas
    // changé depuis la dernière frame (voir `CefBrowserSurface::
    // set_zoom_level`), donc pas la peine de comparer avant d'appeler.
    // Une famille, un traitement — l'ancien `menu_zoom_level.is_some()` tenait
    // lieu de marqueur implicite, il est maintenant porté par le variant.
    match palette {
        PopupPalette::Menu { zoom_level } => {
            state.browser.set_zoom_level(zoom_level);
            // Voir `push_menu_palette`, doc de fonction : le contenu d'un MENU
            // reste sur deux pôles neutres FIXES (deux tentatives d'accent y ont
            // été abandonnées sur demande explicite) — `tint_rgba` est la seule
            // chose dérivée de l'accent choisi.
            push_menu_palette(&state.browser);
        }
        PopupPalette::Window { accent_rgb, dark_mode } => {
            // Réapplique le zoom à chaque frame — DÉFAUT RÉEL, trouvé par sonde
            // puis MESURÉ, et la seule raison d'être de cet appel ici.
            //
            // L'appel unique de `NeuraDeskLoadHandler::on_load_end` ne tient pas
            // pour `assets/popups/notes.html` : une sonde temporaire posée dans
            // `on_load_end` a montré qu'il y est bien appelé (`set_zoom_level
            // (6.0257)`), entre la capture de la frame 0 et celle de la frame 10
            // — et la mesure de l'encre du titre dans la capture donne 15 px,
            // soit 1× au lieu de 3×. Réappliqué ici, la MÊME mesure donne 45 px.
            // Les trois autres panneaux "fenêtre" n'ont jamais eu le défaut.
            //
            // Ce qui distingue cette page : elle est la SEULE à tenter une
            // navigation pendant son PROPRE chargement — `go("ready")`, dernière
            // ligne de son `<script>`, donc un `location.href = "neuradesk://
            // notes/ready"`. C'est exactement la classe de panne que documente le
            // commentaire de `NeuraDeskLoadHandler` (« la navigation initiale
            // réinitialise le zoom au niveau par défaut de son host au moment où
            // elle COMMIT ») : l'interception annule bien la navigation, mais le
            // zoom posé ensuite ne survit pas.
            //
            // L'appel est idempotent (voir `CefBrowserSurface::set_zoom_level`)
            // et ne coûte qu'un aller-retour CEF par frame — d'autant plus
            // acceptable que les quatre panneaux "fenêtre" ne sont jamais
            // ouverts en même temps (chacun ferme les autres, voir
            // `cef_notes::toggle`).
            // ⛔ **LE ZOOM NE SUIT LE DPI QUE POUR LES POPUPS QUI LE DEMANDENT**
            // (voir `regler_zoom_dpi`) — les trois qui n'ont aucun autre levier.
            // Pour tous les autres, le ratio arrive par leur propre canal
            // (`contentScale` de la page, ou `SCREEN_SCALE_OVERRIDE`) : l'ajouter
            // ICI le doublerait, et ils paraîtraient plus PETITS que la primaire.
            let niveau = if zoom_dpi {
                zoom_level_for_factor(CONTENT_ZOOM_FACTOR * crate::win32::dpi::ratio_dpi_de_l_ecran(state.hwnd) as f64)
            } else {
                content_zoom_level()
            } + zoom_level_for_factor(facteur_textes as f64);
            state.browser.set_zoom_level(niveau);
            push_window_palette(&state.browser, accent_rgb, dark_mode, [tint_rgba[0], tint_rgba[1], tint_rgba[2]]);
            let echelle = echelle_du_menu_en_page(
                niveau,
                crate::win32::dpi::ratio_dpi_de_l_ecran(state.hwnd),
                menu::menu_scale(),
                crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES),
            );
            state.browser.execute_js(&echelle_du_menu_js(echelle));
        }
    }

    if state.fenetre {
        // Une FENÊTRE suit sa zone cliente (voir `PopupSpec::fenetre`) : réduite,
        // elle n'a rien à dessiner ; tirée par un bord, les deux surfaces suivent.
        let Some((w, h)) = taille_client(state.hwnd) else { return };
        if (w as u32, h as u32) != (state.swapchain.width, state.swapchain.height) {
            let _ = state.swapchain.resize(&state.device, w as u32, h as u32);
            state.browser.resize(w, h);
        }
        // ⚠️ **UNE FENÊTRE SE DÉPLACE, ET SON FOND SE CAPTURE OÙ ELLE EST.**
        // `state.rect` est celui de la CRÉATION — `montrer_fenetre` ne lit
        // `rect` qu'une fois (voir `PopupSpec::fenetre`) — et l'utilisateur la
        // déplace ensuite par sa barre de titre. Sans cette relecture, la
        // fenêtre porterait le flou de l'endroit d'où elle vient et le
        // traînerait avec elle.
        let mut cadre = RECT::default();
        // ⛔ JAMAIS une position GARÉE : `montrer_fenetre` ramène la fenêtre à ce
        // `rect`, et une image rendue pendant qu'elle est garée l'y aurait perdue.
        if unsafe { GetWindowRect(state.hwnd, &mut cadre) }.is_ok() && !est_garee(cadre.left, cadre.top) {
            state.rect = [cadre.left, cadre.top, cadre.right - cadre.left, cadre.bottom - cadre.top];
        }
    }
    // Recapture le fond flouté à chaque frame — même mécanisme que
    // `egui_overlay::EguiOverlay::refresh_backdrop_if_due` (voir sa doc).
    // ⚠️ Le `else` d'origine a disparu : une FENÊTRE en a un, elle aussi.
    if let Some(Ok(texture)) = backdrop::capture_blurred_batch(&state.device, &[state.rect]).into_iter().next() {
        state.backdrop = Some(texture);
    }

    let Some(rtv) = state.swapchain.rtv() else { return };
    let width = state.swapchain.width as f32;
    let height = state.swapchain.height as f32;

    // BUG RÉEL (DEMANDE EXPLICITE : « dans le menu réglage augmentes les
    // valeurs du curseur pour l'arrondi des menus, derrière les arrondis des
    // menus du noir apparait ») : à rayon élevé, du noir apparaissait
    // derrière les coins arrondis. Vérifié par capture (`win32::
    // debug_capture`, buffer D3D11 brut lu AVANT toute composition DWM) que
    // le rendu lui-même est déjà correctement transparent aux coins — donc
    // PAS un bug du shader (`RoundedBoxSDF`, voir `gfx::quad_renderer`). Le
    // seul mécanisme de transparence des popups est le "glass extend" DWM
    // (`make_transparent_and_uncapturable`) + alpha de la swap chain (voir
    // `gfx::swapchain`, doc de module) : un mélange alpha DWM, donc fragile
    // — au contraire du découpage RÉEL de forme de fenêtre que fait déjà
    // `win32::window::set_dock_region` pour le dock via `SetWindowRgn`.
    // Même remède ici : on détoure la fenêtre elle-même en rectangle à
    // coins arrondis, ce qui ne dépend d'aucun alpha DWM pour les coins.
    // Une FENÊTRE agrandie occupe tout l'écran de travail : des coins arrondis
    // y laisseraient voir le bureau dans les quatre angles.
    let radius = if state.fenetre && unsafe { windows::Win32::UI::WindowsAndMessaging::IsZoomed(state.hwnd) }.as_bool() { 0.0 } else { radius };
    let region_key = (width as i32, height as i32, radius.to_bits());
    if state.applied_region != Some(region_key) {
        state.applied_region = Some(region_key);
        unsafe {
            if radius > 0.0 {
                let d = (radius * 2.0).round() as i32;
                let region = CreateRoundRectRgn(0, 0, width as i32, height as i32, d, d);
                // `SetWindowRgn` prend possession de `region` une fois
                // appliquée — voir `win32::window::set_dock_region`, même
                // remarque pour `combined` : pas de `DeleteObject` ici.
                let _ = SetWindowRgn(state.hwnd, Some(region), true);
            } else {
                // Rayon nul (Dash/Réglages/Presse-papiers/fenêtre de test) :
                // pas de découpage, région par défaut = rectangle plein déjà
                // posé par `CreateWindowExW`. `None` restaure ce rectangle
                // au cas où une région arrondie aurait été posée avant (pas
                // un cas réel aujourd'hui — `radius` ne varie jamais pour
                // ces popups-là — mais reste correct si ça changeait).
                let _ = SetWindowRgn(state.hwnd, None, true);
            }
        }
    }

    // BUG RÉEL TROUVÉ EN VÉRIFIANT PAR CAPTURE (voir
    // docs/plans/cef-web-ui-migration.md) : `QuadRenderer::render` fait
    // `ClearRenderTargetView` en tout premier À CHAQUE APPEL — deux appels
    // séparés (fond flouté PUIS texture CEF) faisaient donc le second
    // EFFACER le premier. Un SEUL appel avec les deux lots.
    // Même `radius` sur les DEUX quads : `QuadInstance::new_textured` a son
    // propre masque SDF de coins arrondis (voir `PSMain`, `TexMode::
    // Texture`) — un radius différent de celui du fond flouté dessinerait
    // le contenu CEF en rectangle bien plus large PAR-DESSUS des coins de
    // fond déjà arrondis, annulant l'arrondi à l'œil.
    let backdrop_instances = [QuadInstance::new_backdrop([0.0, 0.0, width, height], tint_rgba, radius)];
    let cef_instances = [QuadInstance::new_textured([0.0, 0.0, width, height], radius, 1.0)];
    let mut batches: Vec<QuadBatch> = Vec::new();
    if let Some(backdrop_tex) = &state.backdrop {
        batches.push(QuadBatch::new(Some(&backdrop_tex.srv), &backdrop_instances));
    }
    // Une page VISIBLE a-t-elle peint depuis la dernière frame ? C'est la
    // raison de présenter le dock (voir `peintures_visibles`).
    let peintures = state.browser.peintures();
    if peintures != state.peinture_vue {
        state.peinture_vue = peintures;
        PEINTURES_VISIBLES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    let cef_srv = state.browser.latest_texture();
    if let Some(cef_srv) = &cef_srv {
        batches.push(QuadBatch::new(Some(cef_srv), &cef_instances));
    }
    let origin = crate::win32::window::hwnd_screen_origin(state.hwnd).unwrap_or((0, 0));
    let _ = state.quad_renderer.render_at(&state.context, rtv, width, height, &batches, origin);

    let _ = state.swapchain.present();
}

/// Expose en variables CSS `--menu-*-rgb` (triplet `"R, G, B"`, 0..255,
/// substituable directement dans `rgb(var(--x))`/`rgba(var(--x), alpha)`,
/// voir `assets/popups/menu.css`) le contenu textuel/interactif des popups
/// "menu" — la contrepartie CÔTÉ CONTENU CEF du tint de fond déjà calculé
/// depuis l'accent choisi par `render` (voir son appel ci-dessus,
/// `tint_rgba`).
///
/// PLUS de couleur d'accent en paramètre depuis la DEMANDE EXPLICITE
/// suivante ("les menus ont du texte noir et ça ne me plait pas, simplifie
/// les possibilités, limites-les à du blanc, du gris clair, du noir et du
/// gris foncé, les menus devraient s'adapter automatiquement") — la première
/// version de cette fonction dérivait ces variables de `ui::color::Palette`/
/// `compute_palette(accent_rgb, ...)`, capable de choisir du texte NOIR dès
/// que l'accent brut était clair/vif (accents Windows par défaut fréquents :
/// jaune, orange...), alors que le fond RÉELLEMENT affiché derrière (un
/// bureau flouté teinté à faible opacité, voir `tint_rgba`) reste
/// visuellement sombre-ish quel que soit cet accent — texte noir sur fond
/// sombre, illisible en pratique.
///
/// UNE DEUXIÈME version choisissait ensuite entre les deux pôles selon
/// `win32::theme::prefers_dark_mode` (thème clair/sombre WINDOWS) — retombait
/// sur du noir dès que Windows était en thème clair, alors que ce thème
/// système N'A AUCUN rapport avec `tint_rgba` (dérivé de l'accent choisi par
/// l'utilisateur, jamais du thème) — DEMANDE EXPLICITE de suite : "j'ai un
/// thème clair mais des éléments en noir dans les menus, cela ne me plait
/// pas". Voir `ui::color::menu_fg()`/`menu_on_fg`, doc complète, pour
/// le remplacement final : deux pôles neutres FIXES (blanc quasi/noir
/// quasi), sans AUCUNE dépendance de thème — ni l'accent, ni Windows.
///
/// Recalculée et repoussée à CHAQUE frame où le popup est visible, jamais
/// comparée à la précédente avant d'appeler — même choix que
/// `CefBrowserSurface::set_zoom_level` juste au-dessus dans `render` (voir
/// sa doc : comparer coûterait plus cher que les quelques
/// `style.setProperty` ci-dessous, sur un seul popup visible à la fois en
/// pratique).
///
/// Les CINQ variables reçoivent seulement DEUX valeurs (voir
/// `ui::color::menu_fg()`, doc, sur pourquoi "gris clair"/"gris foncé" ne
/// sont pas des tons séparés) : `menu_fg` pour tout ce qui doit rester
/// lisible SUR le fond flouté (texte, survols, éléments actifs, aplats
/// pleins comme la pastille "aujourd'hui" du calendrier), `menu_on_fg`
/// (le pôle OPPOSÉ) uniquement pour `--menu-on-accent-rgb` — le texte posé
/// SUR un aplat plein de `menu_fg` (`--menu-accent-rgb`), qui a besoin du
/// contraste inverse.
/// Valeur par DÉFAUT du multiplicateur de transparence — celle qui reproduit
/// exactement le rendu d'avant ce réglage : les alphas tels qu'écrits dans les
/// pages.
pub const DEFAULT_POPUP_ALPHA: f32 = 1.0;
/// Borne BASSE : `0.0` ferait disparaître purement et simplement tous les
/// éléments (boutons, cartes, étiquettes), ce qui se lit comme un défaut plutôt
/// que comme un réglage.
pub const MIN_POPUP_ALPHA: f32 = 0.2;
/// Borne HAUTE — relevée de `2.0` à `4.0` sur DEMANDE EXPLICITE (« j'aimerais
/// pouvoir monter l'opacité des éléments jusqu'à 400 % »). DÉCISION ASSUMÉE :
/// le raisonnement qui justifiait l'ancien plafond reste exact, il est
/// simplement outrepassé.
///
/// **Arithmétique à connaître, parce qu'elle dit ce que 400 % coûte vraiment.**
/// Les multiplicateurs les plus élevés des quatre pages sont `0,96` (un alpha
/// de `rgba`) et `0,95` (un `opacity`) : ils atteignent l'opaque dès **~105 %**,
/// donc l'ancien plafond de 200 % ne les protégeait plus depuis longtemps. Ce
/// que 400 % ajoute, c'est que les aplats ORDINAIRES (`0,20` à `0,45`)
/// saturent à leur tour, à partir de 223 % pour le plus fort. Un survol à 0,30
/// et un repos à 0,20 restent distincts à 400 % (1,00 contre 0,80) ; ce sont
/// les valeurs VOISINES qui finissent par se confondre — c'est la hiérarchie
/// qui s'efface en haut de course, jamais la validité du rendu : chaque alpha
/// est borné à 1 par CSS, aucun ne casse.
///
/// Même contrepartie assumée que le plafond de magnification des horloges, qui
/// coupe au bord de l'écran au-delà de ~214 % (voir
/// `ui::dock::clock_hover_scale`).
pub const MAX_POPUP_ALPHA: f32 = 4.0;

thread_local! {
    /// Multiplicateur de TRANSPARENCE des éléments des panneaux "fenêtre" —
    /// DEMANDE EXPLICITE : « pouvoir régler la transparence des éléments textes
    /// sur lesquels il y a de la transparence dans les dashs ».
    ///
    /// **GLOBAL, pas par panneau** : un seul réglage pour les quatre, comme
    /// l'accent l'est devenu. Poussé par `push_window_palette` sous la variable
    /// CSS `--popup-alpha`, que chaque page compose avec CHACUN de ses alphas
    /// (`calc(0.15 * var(--popup-alpha))`).
    ///
    /// **C'est la composition par `calc` qui fait tout l'intérêt** : sans elle,
    /// un curseur unique aplatirait les états — un survol deviendrait aussi
    /// discret qu'un repos. En multipliant chaque alpha au lieu de le remplacer,
    /// la HIÉRARCHIE est préservée quel que soit le réglage.
    ///
    /// Borné par `set_popup_alpha`, jamais ici — même convention que
    /// `ui::dock::set_hover_scale`.
    static POPUP_ALPHA: Cell<f32> = const { Cell::new(DEFAULT_POPUP_ALPHA) };
}

/// Multiplicateur de transparence actuel — voir `POPUP_ALPHA`.
pub fn popup_alpha() -> f32 {
    POPUP_ALPHA.with(Cell::get)
}

/// Change le multiplicateur de transparence, borné à [`MIN_POPUP_ALPHA`,
/// `MAX_POPUP_ALPHA`] — voir `ui::dock::set_hover_scale` pour le pourquoi du
/// bornage ici plutôt que dans l'accesseur, et pour le cas du NaN (`f32::clamp`
/// le PROPAGE, donc un `popup_alpha.txt` contenant « nan » figerait la
/// transparence pour de bon).
pub fn set_popup_alpha(alpha: f32) {
    if alpha.is_nan() {
        return;
    }
    POPUP_ALPHA.with(|c| c.set(alpha.clamp(MIN_POPUP_ALPHA, MAX_POPUP_ALPHA)));
}

/// Borne BASSE du fond des zones : zéro — un fond peut disparaître tout à fait,
/// le texte qu'il porte reste lisible (il suit `POPUP_ALPHA`, pas celui-ci).
pub const MIN_POPUP_FOND_ALPHA: f32 = 0.0;

thread_local! {
    /// Multiplicateur du FOND DES ZONES (aplats, bords, ombres des éléments) —
    /// DEMANDE EXPLICITE (2026-10-23) : « un second curseur pour gérer le fond
    /// des zones indépendamment du texte ». Même composition que
    /// `POPUP_ALPHA` (`calc(<alpha> * var(--popup-fond-alpha, 1))`), poussé
    /// sous `--popup-fond-alpha` par les DEUX palettes ; `POPUP_ALPHA` ne
    /// porte plus que les textes et les `opacity`.
    static POPUP_FOND_ALPHA: Cell<f32> = const { Cell::new(DEFAULT_POPUP_ALPHA) };
}

/// Multiplicateur actuel du fond des zones — voir `POPUP_FOND_ALPHA`.
pub fn popup_fond_alpha() -> f32 {
    POPUP_FOND_ALPHA.with(Cell::get)
}

/// Borné à [`MIN_POPUP_FOND_ALPHA`, `MAX_POPUP_ALPHA`], NaN ignoré — même
/// raison que `set_popup_alpha`.
pub fn set_popup_fond_alpha(alpha: f32) {
    if alpha.is_nan() {
        return;
    }
    POPUP_FOND_ALPHA.with(|c| c.set(alpha.clamp(MIN_POPUP_FOND_ALPHA, MAX_POPUP_ALPHA)));
}

/// Triplet CSS `"R, G, B"` (octets 0..255) — le format qu'attendent les
/// variables `--popup-*-rgb`/`--menu-*-rgb`, substituées ensuite dans
/// `rgb(var(--x))` ou `rgba(var(--x), alpha)`.
///
/// **UNE seule copie** : les deux poussoirs de palette ci-dessous en avaient
/// chacun la leur, et `win32::cef_notes` une troisième.
fn rgb_triplet(rgb: [f32; 3]) -> String {
    let [r, g, b] = rgb.map(color::to_u8);
    format!("{r}, {g}, {b}")
}

/// Ce que `render` pousse dans la page EN PLUS du fond flouté (`tint_rgba`).
///
/// Remplace l'ancien `menu_zoom_level: Option<f64>`, dont le `is_some()` tenait
/// lieu de marqueur « popup menu » — un marqueur IMPLICITE devenu explicite,
/// avec un variant par famille. C'est ce qui permet à `render_if_visible` de
/// garder quatre paramètres au lieu d'accumuler des `Option` dont le `None`
/// voudrait dire deux choses différentes selon la famille.
pub enum PopupPalette {
    /// Popup "menu" (menus contextuels, Son, Réseau, Calendrier...) : cinq
    /// variables à deux pôles FIXES (voir `push_menu_palette`, doc — deux
    /// tentatives d'accent y ont été abandonnées sur demande explicite), ET le
    /// zoom réappliqué à chaque frame (curseur « Taille des menus » agissant sur
    /// un menu déjà ouvert).
    Menu { zoom_level: f64 },
    /// Popup "fenêtre" (Dash / Réglages / Presse-papiers / Notes) : la palette
    /// dérivée de l'accent de CETTE fenêtre — voir `push_window_palette`.
    Window { accent_rgb: [f32; 3], dark_mode: bool },
}

/// Palette du CONTENU d'un popup "fenêtre" (Dash / Réglages / Presse-papiers /
/// Notes), dérivée de l'accent de CETTE fenêtre — contrepartie CÔTÉ CONTENU de
/// `tint_rgba`, qui ne teinte que le fond flouté.
///
/// **QUATRE variables, DEUX familles, jamais confondues** — doctrine écrite
/// d'abord dans `assets/popups/notes.html`, et qui vient d'un défaut réel :
///
/// - `--popup-fg-rgb` = `color::menu_fg()`, pôle FIXE. Sert au TEXTE
///   uniquement. Le fond réel derrière ce texte n'est pas l'accent mais le
///   BUREAU FLOUTÉ, à peine teinté : un texte dérivé de l'accent bascule vers
///   une couleur SOMBRE dès que l'accent est clair, et devient illisible. C'est
///   le défaut constaté (« avec un accent clair certains éléments deviennent
///   illisibles »), et la raison pour laquelle ce pôle ne suit rien.
/// - `--popup-accent-rgb` = `Palette::emphasis`, la TEINTE de l'accent — pour
///   les aplats et bordures translucides, qui sont l'immense majorité.
/// - `--popup-accent-solid-rgb` / `--popup-on-accent-rgb` = l'accent BRUT et le
///   texte lisible POSÉ DESSUS (`Palette::accent_fg`) — la paire fond/texte de
///   libadwaita (voir `ui::color`, doc de module). Réservée aux rares aplats
///   PLEINS portant du texte, et **jamais l'un sans l'autre** : c'est le COUPLE
///   qui garantit le contraste, pas l'un des deux pris isolément.
///
/// ⚠️ **AUCUNE VARIABLE DE FOND DE PAGE, ET C'EST DÉLIBÉRÉ** : le fond n'est pas
/// une couleur qu'on pousse, c'est le BUREAU FLOUTÉ que `render` dessine SOUS la
/// page. Une page qui peindrait son propre fond le masquerait.
///
/// ⭐ **MAIS UNE VARIABLE DE SURFACE OPAQUE : `--popup-bg-rgb`** = `menu_on_fg`, le
/// pôle OPPOSÉ au texte. Elle sert aux surfaces qui passent PAR-DESSUS le contenu
/// et doivent donc être opaques — menu contextuel (`menu_contextuel.js`), boîtes de
/// dialogue et choix de connexion (`ssh.html`, `terminal.html`), en-tête collant de
/// la liste SFTP. ⛔ **DÉFAUT SIGNALÉ (2026-10-11)** : ces pages la lisaient déjà,
/// mais RIEN ne la poussait — elles retombaient sur leur gris foncé en dur, sous un
/// texte devenu FONCÉ en thème clair : noir sur noir. Le couple texte/surface vient
/// maintenant des deux pôles de `color::poles`.
///
/// ⭐ **ET LA SURFACE DU THÈME : `--popup-tint-rgb`** = la teinte posée sur le
/// bureau flouté (`tint_rgba`, sans son alpha) — la couleur du thème du panneau.
/// DEMANDE EXPLICITE (2026-10-15) : « que les terminaux aient des fonds colorés
/// utilisant le thème plutôt que noir ». Le texte du thème (`--popup-fg-rgb`) est
/// lisible dessus : c'est le couple que les familles de thèmes garantissent.
///
/// Poussée à CHAQUE frame sans comparaison, comme `push_menu_palette` (voir sa
/// doc pour le pourquoi de ce choix, identique). Un popup fermé n'appelle pas
/// `render` : il n'y a donc rien à « éteindre ».
fn push_window_palette(browser: &CefBrowserSurface, accent_rgb: [f32; 3], dark_mode: bool, tint_rgb: [f32; 3]) {
    browser.execute_js(&window_palette_js(accent_rgb, dark_mode, tint_rgb));
    browser.execute_js(&crate::ui::style::js_actif());
}

/// **LE MENU DU CLIC DROIT D'UNE PAGE À LA TAILLE DES AUTRES MENUS** — DÉFAUT
/// SIGNALÉ (2026-11-01) : dans la visionneuse, les terminaux, le presse-papiers
/// et l'éditeur, le menu commun (`assets/popups/menu_contextuel.js`) gardait ses
/// 13 px CSS et ignorait « Taille des menus » et « Taille des textes ».
///
/// Un vrai menu est zoomé `CONTENT_ZOOM_FACTOR × ratio_dpi × menu_scale × textes`
/// (voir `menu_zoom_factor`, et la doc de « Taille des textes » dans les Réglages :
/// elle « s'ajoute à Taille des menus »). La page, elle, est déjà zoomée de
/// `1,2^niveau` : le menu en page reçoit le RAPPORT des deux, qu'il applique par
/// `transform: scale` (voir sa feuille). `ratio_dpi` compte même quand le zoom
/// de la page ne le porte pas : un menu reste à la taille d'un menu de l'écran.
pub(crate) fn echelle_du_menu_en_page(niveau_de_la_page: f64, ratio_dpi: f32, menu_scale: f32, textes: f32) -> f64 {
    let page = 1.2_f64.powf(niveau_de_la_page);
    let menu = CONTENT_ZOOM_FACTOR * f64::from(ratio_dpi) * f64::from(menu_scale) * f64::from(textes);
    if page > 0.0 && menu.is_finite() && menu > 0.0 { menu / page } else { 1.0 }
}

/// Pose `--nd-echelle-menu` (lue par `menu_contextuel.js`) — en ligne sur
/// `<html>`, comme la palette.
pub(crate) fn echelle_du_menu_js(echelle: f64) -> String {
    format!("document.documentElement.style.setProperty('--nd-echelle-menu','{echelle:.4}')")
}

/// Le script de `push_window_palette` — extrait pour être vérifiable sans CEF.
pub(crate) fn window_palette_js(accent_rgb: [f32; 3], dark_mode: bool, tint_rgb: [f32; 3]) -> String {
    // Dégradé animé dessiné : un accent COMMUN tiré de ses couleurs remplace
    // celui du panneau (voir `gfx::degrade::accent`) ; les menus n'y passent pas.
    // L'emphase est alors l'accent CLAIR lui-même : `compute_palette` en fait
    // une ombre presque noire (MESURÉ : [0.20, 0.14, 0.05] pour un orange),
    // posée à ~20 % d'opacité elle ne se distinguait d'aucune autre sur un
    // dégradé vif — « ne semble pas avoir d'effet ».
    let degrade = crate::gfx::degrade::accent();
    let accent_rgb = degrade.unwrap_or(accent_rgb);
    let palette = color::compute_palette(accent_rgb, dark_mode);
    let palette = match degrade {
        Some(clair) => color::Palette { emphasis: clair, ..palette },
        None => palette,
    };
    format!(
        "(function(s){{\
           s.setProperty('--popup-fg-rgb','{0}');\
           s.setProperty('--popup-accent-rgb','{1}');\
           s.setProperty('--popup-accent-solid-rgb','{2}');\
           s.setProperty('--popup-on-accent-rgb','{3}');\
           s.setProperty('--popup-alpha','{4}');           s.setProperty('--popup-fond-alpha','{7}');\
           s.setProperty('--popup-bg-rgb','{5}');\
           s.setProperty('--popup-tint-rgb','{6}');\
         }})(document.documentElement.style)",
        rgb_triplet(color::menu_fg()),
        rgb_triplet(palette.emphasis),
        rgb_triplet(accent_rgb),
        rgb_triplet(palette.accent_fg),
        // Nombre nu, PAS un triplet : la page s'en sert comme facteur dans
        // `calc(<alpha> * var(--popup-alpha))`, jamais comme couleur.
        popup_alpha(),
        rgb_triplet(color::menu_on_fg()),
        rgb_triplet(tint_rgb),
        popup_fond_alpha(),
    )
}

/// **CETTE ACTION EST-ELLE LA PUNAISE ?** — extraite pour être vérifiable sans CEF.
///
/// ⚠️ **LE DERNIER SEGMENT, PAS UN PRÉFIXE** : chaque page a son propre `go(...)`,
/// donc l'action arrive préfixée (`dash/epingle`, `clipboard/epingle`, et
/// `nd/epingle` pour le bouton que `js_epingle` crée). Exiger un préfixe obligerait
/// chaque page à connaître un nom de canal qui n'est pas le sien.
///
/// ⚠️ **IL FAUT UN SEGMENT AVANT** — `« epingle »` seul est refusé, sans quoi une
/// page pourrait déclencher la punaise avec un mot nu, et le contrat implicite
/// entre `js_epingle` et ce décodeur serait moins net.
fn est_action_epingle(action: &str) -> bool {
    action.ends_with("/epingle")
}

/// **LE STYLE DE LA PUNAISE** — appliqué EN LIGNE, à chaque frame, par `js_epingle`.
///
/// ⛔ **EN LIGNE, ET C'EST UNE CORRECTION MESURÉE.** La première version injectait un
/// `<style id="nd-punaise-style">` dans `document.head` : la chaîne produite était
/// valide (vérifiée en la faisant imprimer par un test), le CSS aussi — et **aucun
/// fond n'apparaissait à l'écran**, sur deux essais et deux mesures de pixels. Plutôt
/// que de continuer à chercher pourquoi une feuille injectée ne mordait pas, l'état
/// est posé **directement sur l'élément** : aucune spécificité, aucune feuille, aucun
/// ordre de chargement ne peut l'empêcher.
///
/// ⛔ **SANS LUI, RIEN NE MONTRE QUE L'ÉPINGLAGE EST EN COURS** — défaut signalé :
/// « il n'y a rien qui montre que l'épinglage est en cours ? ». `aria-pressed` était
/// pourtant posé à chaque frame… mais **aucune page de ce dépôt n'a de règle pour
/// `[aria-pressed="true"]`** sur un bouton de fermeture. L'attribut ne servait donc à
/// rien de visible.
///
/// ⛔ **ET C'EST AUSSI CE QUI FAIT PARLER LES QUINZE LE MÊME LANGAGE.** Le bouton du
/// dash dossier était un `.outil` de SA page ; les quatorze autres clonent la classe
/// de leur propre `✖`. Sans style propre, deux familles de boutons pour une seule
/// commande — second défaut signalé, même cause.
///
/// ⚠️ **LA COULEUR EST CELLE DU PREMIER PLAN, ET VOICI CE QUI EST ÉTABLI.**
///
/// ⛔ **CE QUI A ÉTÉ MESURÉ FAUX, ET POURQUOI — À NE PAS REFAIRE.** Les premières
/// captures ne montraient aucun état appuyé, et j'en ai conclu que la couleur
/// d'accent était trop discrète. **Cette conclusion n'était pas établie** : le
/// levier du harnais ne posait la punaise que dans la branche « le popup existe
/// déjà » de `toggle_with`, or l'ouverture à la frame 0 passe par la branche de
/// PREMIÈRE ouverture. **Tous ces relevés portaient donc sur un panneau JAMAIS
/// épinglé** — je mesurais l'état de repos en croyant mesurer l'état actif, et les
/// deux changements de couleur n'ont rien pu prouver.
/// Le défaut de fond était ailleurs : `aria-pressed` était posé, mais **aucune page
/// de ce dépôt n'a de règle pour `[aria-pressed="true"]`** sur un bouton de
/// fermeture. L'attribut ne servait à rien de visible.
///
/// ⭐ **CE QUI EST ÉTABLI, LUI** : avec le style EN LIGNE ci-dessous appliqué à un
/// panneau réellement épinglé, la capture montre un fond clair et un liseré de 2 px
/// autour de la punaise — mesuré à `(201, 55, 100)` au centre, contre `(73, 1, 47)`
/// pour le fond du panneau.
///
/// ⚠️ **POURQUOI LE PREMIER PLAN PLUTÔT QUE L'ACCENT, CELA DIT** : le fond de ces
/// panneaux est un dégradé de leur accent, donc une pastille à la couleur de l'accent
/// s'y confondrait. La couleur de premier plan, elle, contraste avec un panneau sombre
/// **quelle que soit la palette**. C'est un raisonnement, pas une mesure — les deux
/// ne sont pas la même chose, et cette ligne existe pour qu'on ne les confonde pas.
const STYLE_PUNAISE_BASE: &str = "\
display:flex;align-items:center;justify-content:center;width:22px;height:22px;\
padding:0;border:0;border-radius:4px;font:inherit;font-size:12px;line-height:1;\
margin-left:auto;cursor:default";

/// L'apparence d'une punaise APPUYÉE — voir `STYLE_PUNAISE_BASE`.
const STYLE_PUNAISE_ACTIVE: &str = "\
background:rgba(var(--popup-fg-rgb,255,255,255),.32);\
box-shadow:inset 0 0 0 2px rgba(var(--popup-fg-rgb,255,255,255),.85);opacity:1";

/// Et celle d'une punaise au repos.
const STYLE_PUNAISE_REPOS: &str = "background:none;box-shadow:none;opacity:.6";

/// **LE JS DE LA PUNAISE** — exécuté à chaque frame pour tout panneau visible.
///
/// ⛔ **C'EST CE QUI ÉVITE UN VERBE, UN ÉTAT, UN BRAS DE DISPATCH ET UNE ÉDITION DE
/// PAGE DANS QUATORZE MODULES.** Les pages CEF ne partagent aucun JS entre elles
/// (voir `notes.html`, qui l'écrit noir sur blanc) ; mais `cef_popup` injecte
/// **déjà** du JS dans toutes les pages à palette `Window` — `js_actif`, juste
/// au-dessus. Le canal existait ; il ne restait qu'à s'en servir.
///
/// ⭐ **LE BOUTON EST CRÉÉ ICI, À CÔTÉ DU `✖` DE LA PAGE** — et c'est ce qui rend
/// les quinze panneaux possibles : quinze pages auraient voulu quinze lectures,
/// quinze éditions et quinze occasions de mal placer le bouton. On CLONE la classe
/// du bouton de fermeture voisin, donc la punaise a exactement l'air de ce qui
/// l'entoure, page par page, **sans qu'aucune page ne le sache**.
///
/// ⚠️ **UNE PAGE QUI FOURNIT DÉJÀ SON BOUTON EST RESPECTÉE** : le dash dossier a
/// une barre en haut avec sa punaise (`id="nd-punaise"`) — c'est ce `id` qui fait
/// que ce JS ne lui en ajoute pas un second.
///
/// ⚠️ **`marginLeft = 'auto'` — MESURÉ, PAS DÉCORATIF.** Une barre qui répartit
/// ses enfants (`justify-content: space-between` : un titre à gauche, le `✖` à
/// droite) en centre un TROISIÈME. La première capture l'a montré : dans les
/// Réglages, la punaise flottait au milieu de la barre, loin du `✖`. La marge
/// automatique la repousse contre lui, dans toutes les pages.
///
/// ⚠️ **UNE PAGE SANS BOUTON DE FERMETURE NE CASSE RIEN** : le `return` la laisse
/// exactement comme avant. C'est ce qui permet aux quinze de coexister, y compris
/// celles que je n'ai pas dessinées.
///
/// ⚠️ **L'ACTION S'ARRÊTE À `nd/epingle`** et non à `epingle` : `drain_actions` la
/// reconnaît sur son DERNIER SEGMENT, et un segment unique ne le serait pas.
fn js_epingle(epingle: bool) -> String {
    let titre = if epingle {
        "Rendre au panneau sa fermeture automatique"
    } else {
        "Maintenir ce panneau ouvert"
    };
    format!(
        "(function(){{\
           var b = document.getElementById('nd-punaise');\
           if (!b) {{\
             var f = document.querySelector('.close-btn, [title*=\"Fermer\"]');\
             if (!f || !f.parentElement) return;\
             b = document.createElement('button');\
             b.id = 'nd-punaise';\
             b.type = 'button';\
             b.className = f.className;\
             b.textContent = '\\uD83D\\uDCCC';\
             b.style.marginLeft = 'auto';\
             b.addEventListener('click', function () {{ location.href = 'neuradesk://nd/epingle'; }});\
             f.parentElement.insertBefore(b, f);\
           }}\
           b.setAttribute('style', {base:?} + ({etat:?}));\
           b.setAttribute('aria-pressed', '{epingle}');\
           b.title = '{titre}';\
         }})()",
        base = STYLE_PUNAISE_BASE,
        etat = if epingle { STYLE_PUNAISE_ACTIVE } else { STYLE_PUNAISE_REPOS },
    )
}

fn push_menu_palette(browser: &CefBrowserSurface) {
    let fg = color::menu_fg();
    let on_fg = color::menu_on_fg();
    let js = format!(
        "(function(s){{\
           s.setProperty('--menu-text-rgb','{0}');\
           s.setProperty('--menu-zone-rgb','{0}');\
           s.setProperty('--menu-active-rgb','{0}');\
           s.setProperty('--menu-accent-rgb','{0}');\
           s.setProperty('--menu-on-accent-rgb','{1}');\
           s.setProperty('--popup-alpha','{2}');           s.setProperty('--popup-fond-alpha','{3}');\
         }})(document.documentElement.style)",
        rgb_triplet(fg),
        rgb_triplet(on_fg),
        // « Opacité des éléments » vaut pour TOUTES les pages — DEMANDE
        // EXPLICITE (2026-10-23) : les menus le lisent comme les panneaux
        // (`var(--popup-alpha, 1)`, voir `window_palette_js`).
        popup_alpha(),
        popup_fond_alpha(),
    );
    browser.execute_js(&js);
    browser.execute_js(&crate::ui::style::js_actif());
}

/// Relit le back buffer courant vers un PNG — voir
/// `gfx::capture::dump_backbuffer_composited`.
pub fn debug_dump_backbuffer(slot: &'static LocalKey<PopupSlot>, path: &Path) -> bool {
    slot.with(|s| {
        let state_ref = s.state.borrow();
        let Some(state) = state_ref.as_ref() else { return false };
        let Ok(back_buffer) = state.swapchain.back_buffer() else { return false };
        crate::gfx::capture::dump_backbuffer_composited(&state.device, &state.context, &back_buffer, path).is_ok()
    })
}

/// Coordonnées PHYSIQUES (x, y) empaquetées dans `lparam` d'un message
/// souris Win32 — même extraction LOWORD/HIWORD que `win32::dash::loword`/
/// `hiword`. `as i16` d'abord : une coordonnée peut être négative près du
/// bord d'un écran secondaire à gauche/au-dessus du principal, `as u16` la
/// tronquerait à tort en une valeur positive absurde.
fn mouse_pos(lparam: LPARAM) -> (i32, i32) {
    let raw = lparam.0 as u32;
    ((raw & 0xFFFF) as u16 as i16 as i32, ((raw >> 16) & 0xFFFF) as u16 as i16 as i32)
}

/// ⛔ **UN PANNEAU NE PASSE JAMAIS DEVANT LE DOCK** — BUG RÉEL SIGNALÉ
/// (2026-10-23) : « les coins arrondis de la zone centrale disparaissent quand
/// j'ouvre un dash, et c'est visible ». Les congés sont peints par la fenêtre du
/// dock ; un panneau montré, activé ou remonté en tête de la bande topmost les
/// recouvrait jusqu'à la remontée suivante du dock (`ombre_barre::remonter`,
/// toutes les 250 ms). Corriger APRÈS coup laisse toujours un instant visible :
/// c'est donc le déplacement lui-même qui est réécrit, dans son
/// `WM_WINDOWPOSCHANGING` — le panneau se pose SOUS le dock au lieu de devant.
/// L'ordre voulu reste celui de `PopupSpec::above_a_magnified_dock` : menus >
/// dock > panneaux.
fn sous_le_dock(wp: &mut WINDOWPOS) {
    if wp.flags.0 & SWP_NOZORDER.0 != 0 || !visibles_ou_non(&PANNEAUX).contains(&wp.hwnd) {
        return;
    }
    let Some(dock) = crate::win32::coins_ecran::proprietaire() else { return };
    let topmost = unsafe { GetWindowLongW(wp.hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST.0 != 0;
    let au_dessus = |fenetre: HWND| au_dessus_de(fenetre, dock);
    if let Some(sous) = insertion_d_un_panneau(wp.hwndInsertAfter, topmost, dock, au_dessus) {
        wp.hwndInsertAfter = sous;
    }
}

/// ⛔ **LE RATTRAPAGE, SUR LE RÉSULTAT** — `sous_le_dock` ne suffit pas, MESURÉ
/// (2026-10-23, sonde sur les `WM_WINDOWPOS*` du Dash et des Réglages) : à la
/// MONTRÉE (`SWP_SHOWWINDOW`) et aux déplacements `SWP_ASYNCWINDOWPOS`, Windows
/// demande « sous la fenêtre IME » — invisible, possédée par le dock — que
/// `au_dessus_de` juge SOUS le dock ; la demande passe telle quelle et le
/// panneau finit DEVANT le dock (relevé après coup : 6 fois sur 6). Plutôt que
/// de deviner chaque demande, on constate le rang obtenu : un panneau devant
/// le dock repasse dessous, dans le MÊME message — avant la composition DWM
/// suivante, donc sans image visible. Le `SetWindowPos` imbriqué demande
/// « sous le dock » : `insertion_d_un_panneau` le laisse passer, pas de boucle.
fn ramener_sous_le_dock(hwnd: HWND) {
    if !visibles_ou_non(&PANNEAUX).contains(&hwnd) {
        return;
    }
    if let Some(dock) = crate::win32::coins_ecran::proprietaire() {
        ramener_sous(hwnd, dock);
    }
}

/// `fenetre` repasse sous `dock` si elle est au-dessus — voir
/// `ramener_sous_le_dock`.
fn ramener_sous(fenetre: HWND, dock: HWND) {
    if au_dessus_de(fenetre, dock) {
        let _ = unsafe { SetWindowPos(fenetre, Some(dock), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER) };
    }
}

/// Où poser un panneau qui demande à passer sous `demande` : sous le dock s'il
/// finirait AU-DESSUS de lui — `HWND_TOPMOST`, `HWND_TOP` alors qu'il est déjà
/// topmost (une ACTIVATION), ou une fenêtre précise placée au-dessus du dock.
/// ⚠️ Ce dernier cas est le plus fréquent, MESURÉ : à l'ouverture d'un Dash,
/// Windows et l'effet d'ouverture demandent « sous la fenêtre IME », qui est en
/// tête de pile — ne réécrire que `HWND_TOP`/`HWND_TOPMOST` laissait le Dash
/// devant les congés jusqu'à 300 ms. `HWND_NOTOPMOST` (punaise éteinte),
/// `HWND_BOTTOM` et une fenêtre SOUS le dock restent tels quels.
fn insertion_d_un_panneau(demande: HWND, deja_topmost: bool, dock: HWND, au_dessus_du_dock: impl Fn(HWND) -> bool) -> Option<HWND> {
    if demande == dock {
        return None;
    }
    let devant = demande == HWND_TOPMOST
        || (demande == HWND_TOP && deja_topmost)
        || (demande != HWND_TOP && demande != HWND_NOTOPMOST && demande != HWND_BOTTOM && au_dessus_du_dock(demande));
    devant.then_some(dock)
}

/// `fenetre` est-elle au-dessus de `dock` dans la pile ?
fn au_dessus_de(fenetre: HWND, dock: HWND) -> bool {
    let mut courante = unsafe { GetTopWindow(None) }.ok();
    while let Some(h) = courante {
        if h == dock {
            return false;
        }
        if h == fenetre {
            return true;
        }
        courante = unsafe { GetWindow(h, GW_HWNDNEXT) }.ok();
    }
    false
}

/// Relais souris/focus vers CEF, COMMUN à tous les wndproc popup — PAS de
/// clavier pour l'instant (aucun popup actuel n'a de champ de texte). Voir
/// la doc de `PopupSpec::wndproc` : chaque module popup garde son propre
/// `unsafe extern "system" fn` minimal qui appelle CETTE fonction en tout
/// premier ; `Some(lresult)` signifie "entièrement pris en charge, renvoyer
/// CETTE valeur immédiatement SANS appeler `DefWindowProcW`" (voir le cas
/// `WM_SETCURSOR` ci-dessous, seul cas actuel à retourner `Some`) — `None`
/// signifie "pas de traitement spécial, continuer comme avant" (tous les
/// autres messages gérés ici retournaient déjà `()`, donc laissaient
/// `DefWindowProcW` s'exécuter ensuite ; ce comportement est inchangé).
/// `slot.with` à chaque message : ce wndproc peut être rappelé de façon
/// RÉENTRANTE par certains appels Win32 (voir la mémoire du projet sur
/// cette classe de bug), emprunter `SLOT` seulement le temps de CE message,
/// jamais gardé entre deux.
pub fn dispatch_input(slot: &'static LocalKey<PopupSlot>, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    if msg == WM_WINDOWPOSCHANGING {
        // SANS emprunt de `slot` : ce message arrive au milieu de n'importe quel
        // `SetWindowPos`/`ShowWindow`, y compris sous un emprunt mutable.
        unsafe { sous_le_dock(&mut *(lparam.0 as *mut WINDOWPOS)) };
        return None;
    }
    if msg == WM_WINDOWPOSCHANGED {
        let wp = unsafe { &*(lparam.0 as *const WINDOWPOS) };
        if wp.flags.0 & SWP_NOZORDER.0 == 0 {
            ramener_sous_le_dock(wp.hwnd);
        }
        return None;
    }
    if msg == WM_SETCURSOR {
        // DEMANDE EXPLICITE ("le curseur ne se transforme pas au survol du
        // séparateur du presse-papier") — voir `win32::cef_app::
        // NeuraDeskDisplayHandler`, doc de struct : en rendu CEF hors-écran,
        // c'est à NOUS d'appeler `SetCursor()`, CEF ne le fait jamais tout
        // seul. LOWORD(lparam) : code de hit-test (voir `WM_NCHITTEST`) —
        // seul `HTCLIENT` (survol de la ZONE CLIENTE, où vit la page HTML)
        // doit appliquer le curseur demandé par la page ; ailleurs (bordure,
        // barre de titre — inexistantes ici, mais par principe), laisser
        // `DefWindowProcW` faire son travail habituel.
        let hit_test = (lparam.0 as u32) & 0xFFFF;
        if hit_test == HTCLIENT {
            let cursor = slot.with(|s| s.state.borrow().as_ref().and_then(|state| state.browser.latest_cursor()));
            if let Some(cursor) = cursor {
                unsafe {
                    SetCursor(Some(HCURSOR(cursor as *mut _)));
                }
                return Some(LRESULT(1));
            }
        }
        return None;
    }
    match msg {
        WM_MOUSEMOVE => {
            let (x, y) = mouse_pos(lparam);
            let modifiers = input_modifiers();
            slot.with(|s| {
                if let Some(state) = s.state.borrow().as_ref() {
                    state.browser.on_mouse_move(x, y, modifiers, false);
                }
            });
        }
        WM_MOUSELEAVE => {
            slot.with(|s| {
                if let Some(state) = s.state.borrow().as_ref() {
                    state.browser.on_mouse_move(0, 0, 0, true);
                }
            });
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP => {
            let (x, y) = mouse_pos(lparam);
            let pressed = msg == WM_LBUTTONDOWN;
            // `SetCapture`/`ReleaseCapture` — DEMANDE EXPLICITE ("je ne
            // peux pas faire glisser les curseurs") : même raisonnement
            // que `win32::window`, `WM_LBUTTONDOWN` (voir sa doc, "sans
            // elle, glisser... puis relâcher... après avoir quitté les
            // limites de la fenêtre ne délivrerait plus jamais le
            // WM_LBUTTONUP correspondant") — un `<input type="range">`
            // HTML se glisse EXACTEMENT pareil : sans capture, le curseur
            // sort du popup dès qu'on va un peu vite ou un peu loin, plus
            // aucun WM_MOUSEMOVE/WM_LBUTTONUP n'atteint cette fenêtre, CEF
            // ne voit plus jamais bouger le glissement en cours.
            //
            // ⛔ **LE NOMBRE DE CLICS ET LES MODIFICATEURS** : envoyés à 1 et 0
            // jusqu'au 2026-09-26, donc AUCUN `dblclick` dans aucune page (les
            // dossiers de la vue Fichiers SSH ne s'ouvraient pas) et ni
            // `ctrlKey` ni `shiftKey` sur un `click`. Le relâchement porte le
            // même nombre que son appui : c'est lui qui déclenche `dblclick`.
            let modifiers = input_modifiers();
            slot.with(|s| {
                s.dragging.set(pressed);
                if pressed {
                    let delai = unsafe { GetDoubleClickTime() };
                    let zone = unsafe { (GetSystemMetrics(SM_CXDOUBLECLK), GetSystemMetrics(SM_CYDOUBLECLK)) };
                    let instant = unsafe { GetMessageTime() } as u32;
                    s.clic.set(compter_clic(s.clic.get(), instant, x, y, delai, zone));
                }
                let nombre = s.clic.get().nombre.max(1);
                if let Some(state) = s.state.borrow().as_ref() {
                    unsafe {
                        if pressed {
                            SetCapture(state.hwnd);
                        } else {
                            let _ = ReleaseCapture();
                        }
                    }
                    state.browser.on_mouse_left_button(x, y, pressed, modifiers, nombre);
                }
            });
        }
        // ⭐ **LE CLIC MOLETTE N'ÉTAIT PAS TRANSMIS** (2026-11-01, vue sphère des
        // terminaux : « le clic molette pour basculer ne fonctionne pas ») — aucun
        // `mousedown`/`auxclick` à `button === 1` n'atteignait aucune page, donc
        // la fermeture d'un onglet au clic molette était morte elle aussi.
        // MÊME MODÈLE QUE LE CLIC DROIT ci-dessous : le clic COMPLET part à
        // l'appui (rien ne garantit le relâchement), filtré sur l'état PHYSIQUE
        // du bouton et sur le curseur posé sur CETTE fenêtre.
        WM_MBUTTONDOWN => {
            let touche = unsafe { GetAsyncKeyState(0x04) } as u16;
            if touche & 0x8000 != 0 {
                let modifiers = input_modifiers();
                slot.with(|s| {
                    if let Ok(state) = s.state.try_borrow() {
                        if let Some(state) = state.as_ref() {
                            let mut pt = POINT::default();
                            let sous_le_curseur = unsafe { GetCursorPos(&mut pt).is_ok() && WindowFromPoint(pt) == state.hwnd };
                            if sous_le_curseur && unsafe { ScreenToClient(state.hwnd, &mut pt) }.as_bool() {
                                state.browser.on_mouse_middle_click(pt.x, pt.y, modifiers);
                            }
                        }
                    }
                });
            }
        }
        WM_RBUTTONDOWN => {
            // ⛔⛔ **SI CETTE BRANCHE CESSE DE COMPILER EN « unreachable pattern »
            // SUR LES BRANCHES SUIVANTES, C'EST QUE LA CONSTANTE N'EST PLUS
            // IMPORTÉE — ET TOUT CE QUI SUIT MEURT EN SILENCE.**
            //
            // En Rust, un identifiant SEUL dans un motif est une **liaison**, pas
            // une constante : sans `WM_RBUTTONDOWN` dans la liste d'import
            // ci-dessus, cette ligne devient un JOKER qui capture **tous** les
            // messages. Les branches d'après — molette, focus, clavier — ne sont
            // alors JAMAIS atteintes.
            //
            // **C'EST ARRIVÉ, ET ÇA A COÛTÉ UNE ENQUÊTE ENTIÈRE.** Le 2026-09-22,
            // cette branche a été écrite sans l'import : le clic droit
            // « fonctionnait » (sa branche s'exécutait pour n'importe quel
            // message, et le filtre `GetAsyncKeyState` faisait le tri), pendant
            // que **la molette et le clavier du Dash ne répondaient plus**. Le
            // compilateur le DISAIT (`unreachable pattern`, six avertissements)
            // et ces avertissements étaient filtrés à chaque compilation.
            // **Lire les avertissements avant de mesurer quoi que ce soit.**
            //
            // **ON DÉCLENCHE SUR LE DOWN, PAS SUR LE UP.** MESURÉ : ces fenêtres
            // reçoivent le `WM_RBUTTONDOWN` et **JAMAIS** le `WM_RBUTTONUP`
            // (5 292 DOWN contre 0 UP sur 249 s, puis 96 277 contre 0 sur deux
            // heures) — un déclenchement sur le UP ne pouvait rien faire, quelle
            // que soit la qualité du HTML ou de la page.
            //
            // ⚠️ **LE FILTRE SUR L'ÉTAT PHYSIQUE N'EST PAS UNE PRÉCAUTION DE
            // STYLE.** Le même journal montre ~13 DOWN par seconde en continu,
            // dès le démarrage, **bouton droit RELÂCHÉ** (`GetAsyncKeyState` rend
            // 0x0000), soit environ **1 par seconde et par fenêtre de popup** —
            // des messages qui ne viennent pas de la souris. Sans ce filtre, le
            // sélecteur de catégories s'ouvrirait tout seul treize fois par
            // seconde. Un VRAI clic a le bit de poids fort levé au moment du
            // DOWN : mesuré, 1 573 clics réels ainsi distingués de 94 704
            // fantômes.
            //
            // ⚠️ **`execute_js_si_libre` ET NON `execute_js`** — ces messages
            // arrivent RÉ-ENTRANTS, pendant que l'appelant détient un
            // `borrow_mut` : `borrow()` y paniquait (« RefCell already mutably
            // borrowed », mesuré). Voir la doc de cette fonction.
            //
            // **LE CLIC EST AUSSI TRANSMIS À CEF** (2026-09-26, « les
            // interactions classiques de Windows pour le clic droit ») :
            // appui ET relâchement d'un coup, puisque le UP n'arrive jamais —
            // la page reçoit un vrai `contextmenu`, avec sa cible et ses
            // coordonnées (menu commun : `assets/popups/menu_contextuel.js`),
            // et le menu natif de Chromium est vidé
            // (`cef_app::NeuraDeskContextMenuHandler`). `try_borrow` pour la même
            // ré-entrance que `execute_js_si_libre`.
            //
            // ⚠️ Le filtre physique ne suffit pas ici : un fantôme qui tombe
            // PENDANT un vrai clic droit ailleurs passerait, et ouvrirait un menu
            // dans un popup caché. Le clic n'est donc transmis que si le curseur
            // est SUR cette fenêtre, à la position du curseur (pas `lparam`).
            let touche = unsafe { GetAsyncKeyState(0x02) } as u16;
            if touche & 0x8000 != 0 {
                let modifiers = input_modifiers();
                slot.with(|s| {
                    if let Ok(state) = s.state.try_borrow() {
                        if let Some(state) = state.as_ref() {
                            let mut pt = POINT::default();
                            let sous_le_curseur = unsafe { GetCursorPos(&mut pt).is_ok() && WindowFromPoint(pt) == state.hwnd };
                            if sous_le_curseur && unsafe { ScreenToClient(state.hwnd, &mut pt) }.as_bool() {
                                state.browser.on_mouse_right_click(pt.x, pt.y, modifiers);
                            }
                        }
                    }
                });
                execute_js_si_libre(
                    slot,
                    "if (window.neuradeskRightClick) window.neuradeskRightClick();",
                );
            }
        }
        WM_RBUTTONUP => {
            // ⚠️ **LE BOUTON DROIT N'ÉTAIT PAS ANNONCÉ — ET C'EST POURQUOI LE
            // CLIC DROIT NE FAISAIT RIEN.** Les messages du bouton GAUCHE
            // ci-dessus sont envoyés à CEF ; rien n'envoyait ni ne signalait le
            // bouton droit, donc l'événement `contextmenu` ne se déclenchait
            // JAMAIS dans la page. Le sélecteur de catégories du Dash (clic
            // droit sur une tuile) existait depuis le portage CEF **sans être
            // atteignable** — d'où un « j'ai perdu la gestion des catégories »
            // qui ne venait pas du HTML.
            //
            // ⚠️ **ON NE RETRANSMET PAS CE UP À CEF** : le clic est transmis
            // COMPLET sur le DOWN (voir la branche ci-dessus), le retransmettre
            // ici le doublerait. (Il a longtemps été écrit ici que la crate
            // `cef` 151.8 n'exposait pas de quoi supprimer le menu natif : c'était
            // FAUX, `wrap_context_menu_handler!` existe — voir
            // `cef_app::NeuraDeskContextMenuHandler`.) `neuradeskRightClick`
            // reste annoncé à la page (Dash, Notes), chemin validé.
            //
            // Le `if` de la page rend l'appel inerte pour les 12 autres popups :
            // le bouton droit n'y faisait rien avant, rien n'y change.
            execute_js_si_libre(slot, "if (window.neuradeskRightClick) window.neuradeskRightClick();");
        }
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            // DEMANDE EXPLICITE ("je ne peux pas faire défiler avec la
            // molette") : jamais câblé jusqu'ici. `lparam` de CES DEUX
            // messages précis est en coordonnées ÉCRAN (PAS client comme
            // tous les autres messages souris ici, particularité Win32
            // documentée pour WM_MOUSEWHEEL/WM_MOUSEHWHEEL) — converti via
            // `ScreenToClient` avant transmission à CEF, qui attend des
            // coordonnées de VUE (voir `on_mouse_move`).
            let (screen_x, screen_y) = mouse_pos(lparam);
            let delta = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
            slot.with(|s| {
                if let Some(state) = s.state.borrow().as_ref() {
                    let mut pt = POINT { x: screen_x, y: screen_y };
                    unsafe {
                        let _ = ScreenToClient(state.hwnd, &mut pt);
                    }
                    let (delta_x, delta_y) = if msg == WM_MOUSEHWHEEL { (delta, 0) } else { (0, delta) };
                    state.browser.on_mouse_wheel(pt.x, pt.y, input_modifiers(), delta_x, delta_y);
                }
            });
        }
        WM_SETFOCUS | WM_KILLFOCUS => {
            let focused = msg == WM_SETFOCUS;
            slot.with(|s| {
                if let Some(state) = s.state.borrow().as_ref() {
                    state.browser.set_focus(focused);
                }
            });
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let vk = wparam.0 as i32;
            // `key_modifiers` et NON `input_modifiers` : sans la correction
            // AltGr, Blink marque la touche comme raccourci ICI et consomme
            // l'évènement, ce qui fait ignorer le caractère qui suit — voir sa
            // doc, qui raconte pourquoi une première version ne corrigeant que
            // les évènements de caractère ne suffisait pas.
            let modifiers = key_modifiers();
            let is_system_key = msg == WM_SYSKEYDOWN;
            slot.with(|s| {
                if let Some(state) = s.state.borrow().as_ref() {
                    state.browser.on_key_down(vk, modifiers, is_system_key);
                }
            });
        }
        WM_KEYUP | WM_SYSKEYUP => {
            let vk = wparam.0 as i32;
            // Voir `key_modifiers` : même correction que le keydown, pour que
            // Blink ne voie pas un Ctrl enfoncé au relâchement non plus.
            let modifiers = key_modifiers();
            let is_system_key = msg == WM_SYSKEYUP;
            slot.with(|s| {
                if let Some(state) = s.state.borrow().as_ref() {
                    state.browser.on_key_up(vk, modifiers, is_system_key);
                }
            });
        }
        WM_CHAR | WM_SYSCHAR => {
            let ch = wparam.0 as u16;
            // `key_modifiers` : même correction que le keydown ci-dessus, qui
            // porte le même nom depuis que les trois types d'évènement la
            // reçoivent (voir sa doc).
            let modifiers = key_modifiers();
            let is_system_key = msg == WM_SYSCHAR;
            slot.with(|s| {
                if let Some(state) = s.state.borrow().as_ref() {
                    state.browser.on_char(ch, modifiers, is_system_key);
                }
            });
        }
        _ => {}
    }
    None
}

/// État courant de Maj/Ctrl/Alt/boutons souris (voir `WM_KEYDOWN`/
/// `WM_CHAR`/`WM_MOUSEMOVE`/`WM_LBUTTONDOWN` dans `dispatch_input`) —
/// `GetKeyState` (PAS `GetAsyncKeyState`) : l'état tel que Windows l'a
/// lui-même appliqué en traitant CE message précis, pas un sondage temps
/// réel qui pourrait avoir dérivé de quelques millisecondes (voir la
/// documentation Win32 de `GetKeyState` — s'applique aussi aux boutons
/// souris, Win32 les traite comme des "touches virtuelles" au même titre
/// que le clavier, `VK_LBUTTON` etc.). Combinaison de bits
/// `EVENTFLAG_SHIFT_DOWN`/`CONTROL_DOWN`/`ALT_DOWN`/`LEFT_MOUSE_BUTTON`
/// de CEF (2/4/8/16, voir `cef_key_event_t`/`cef_mouse_event_t`) — pas de
/// constante `cef-rs` directement exposée pour celles-ci, valeurs stables
/// (documentées par CEF), pas la peine d'aller chercher plus loin pour
/// quatre bits fixes. Le bit `LEFT_MOUSE_BUTTON` est CE QUI MANQUAIT pour
/// que le glisser d'un `<input type="range">` fonctionne : sans lui, CEF/
/// Blink traite chaque `WM_MOUSEMOVE` reçu bouton enfoncé comme un simple
/// survol — DEMANDE EXPLICITE ("je ne peux toujours pas déplacer les
/// curseurs en maintenant le clic gauche"), `SetCapture` seul (déjà
/// ajouté) ne suffisait pas : la fenêtre continuait bien à recevoir les
/// `WM_MOUSEMOVE` même hors de ses limites, mais CEF ignorait le
/// glissement faute de savoir qu'un bouton était tenu.
/// Un appui gauche : `instant` = `GetMessageTime` (ms, reboucle), position en
/// pixels de la vue, `nombre` = 1 simple, 2 double, 3 triple.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Clic {
    instant: u32,
    x: i32,
    y: i32,
    nombre: i32,
}

/// Le rang de cet appui : le suivant du précédent s'il arrive dans le délai de
/// double-clic de Windows ET dans sa zone (`zone` = largeur/hauteur du
/// rectangle CENTRÉ sur le premier appui, `SM_CXDOUBLECLK`), sinon 1. Plafonné
/// à 3 (triple-clic : sélection d'un paragraphe), puis on repart.
fn compter_clic(precedent: Clic, instant: u32, x: i32, y: i32, delai: u32, zone: (i32, i32)) -> Clic {
    let proche = (x - precedent.x).abs() <= zone.0 / 2 && (y - precedent.y).abs() <= zone.1 / 2;
    let a_temps = instant.wrapping_sub(precedent.instant) <= delai;
    let nombre = if precedent.nombre > 0 && precedent.nombre < 3 && proche && a_temps { precedent.nombre + 1 } else { 1 };
    Clic { instant, x, y, nombre }
}

fn input_modifiers() -> u32 {
    const VK_SHIFT: i32 = 0x10;
    const VK_CONTROL: i32 = 0x11;
    const VK_MENU: i32 = 0x12;
    const VK_LBUTTON: i32 = 0x01;
    let down = |vk: i32| unsafe { (GetKeyState(vk) as u16) & 0x8000 != 0 };
    let mut flags = 0u32;
    if down(VK_SHIFT) {
        flags |= 2;
    }
    if down(VK_CONTROL) {
        flags |= 4;
    }
    if down(VK_MENU) {
        flags |= 8;
    }
    if down(VK_LBUTTON) {
        flags |= 16;
    }
    flags
}

/// `EVENTFLAG_ALTGR_DOWN` : Blink le rend par `getModifierState("AltGraph")`,
/// sans en faire un raccourci. ⭐ DÉFAUT RÉEL SIGNALÉ (2026-10-16) : « je
/// n'arrive pas à taper l'antislash dans les terminaux ». Sans lui, un
/// caractère AltGr arrivait SANS AUCUN modificateur ; xterm (son `_keyDown`)
/// envoie alors dès le `keydown` la valeur de la touche (`e.key`, longueur 1,
/// `keyCode` ≥ 48) et annule le caractère composé qui suit — `\` (AltGr+8)
/// devenait la touche 8 nue. Les notes, elles, insèrent le caractère : seul
/// xterm lit le `keydown`.
const ALTGR: u32 = 4096;

/// Retire le bit Ctrl quand Alt est AUSSI enfoncé — la correction AltGr, PURE
/// (extraite pour être testable sans clavier, même motif que
/// `win32::window::is_workarea_setting_change`).
///
/// **Windows ÉMULE AltGr par Ctrl+Alt.** Sur un clavier français, `#` est
/// AltGr+3 et `@` est AltGr+0 : ces deux caractères arrivent donc avec les DEUX
/// bits positionnés, alors que l'utilisateur n'a enfoncé qu'UNE touche. Voir
/// `key_modifiers` pour le défaut que cela produisait.
///
/// La combinaison Ctrl+Alt est traitée comme AltGr, jamais comme un raccourci —
/// c'est vrai de tout évènement de caractère sous Windows.
///
/// **MESURÉ, après deux corrections infructueuses, en journalisant chaque
/// message clavier reçu** (sonde temporaire, retirée depuis) — et c'est ce qui a
/// évité une troisième supposition. Pour `#`, `€` et `|`, la séquence reçue
/// était identique :
///
/// ```text
/// keydown 0x33 mods=8   →   char '#' mods=8
/// ```
///
/// Deux conclusions, qu'aucune lecture du code ne donnait : le caractère ARRIVE
/// bel et bien (donc ni clavier, ni pilote, ni message manquant en cause), et le
/// bit Ctrl avait déjà été retiré — `8` et non `12` — mais **Alt restait**, ce
/// qui suffisait à faire jeter le caractère par Blink. Retirer une seule moitié
/// de l'AltGr ne servait donc à rien.
fn alt_gr_adjusted(flags: u32) -> u32 {
    const CONTROL: u32 = 4;
    const ALT: u32 = 8;
    // LES DEUX bits partent, et non le seul Ctrl — voir la doc ci-dessus pour la
    // mesure qui l'a établi : Blink refuse d'insérer du texte dès qu'un
    // évènement de caractère porte Ctrl *ou* Alt, donc en retirer un seul ne
    // change rien. ⭐ Et `ALTGR` les REMPLACE : sans lui, xterm prend la touche
    // pour une frappe ordinaire et envoie sa valeur de base (voir `ALTGR`).
    if flags & ALT != 0 && flags & CONTROL != 0 {
        (flags & !(CONTROL | ALT)) | ALTGR
    } else {
        flags
    }
}

/// Modificateurs à joindre à TOUT évènement clavier — voir
/// `input_modifiers` pour l'état physique, et `alt_gr_adjusted` pour la
/// correction appliquée ici.
///
/// **DÉFAUT RÉEL SIGNALÉ : « je n'arrive pas à taper les caractères # et @ dans
/// une note »**, puis, vérification faite, « aucun caractère AltGr ne marche »
/// (ni €, ni {, ni |). Windows émule AltGr par Ctrl+Alt, donc chaque caractère
/// composé partait avec CES DEUX bits ; Blink refuse d'insérer du texte dès
/// qu'un évènement porte Ctrl ou Alt, qu'il réserve aux raccourcis et à l'accès
/// aux menus. Les lettres ordinaires, sans modificateur, passaient — d'où un
/// défaut limité aux caractères composés, qui pouvait passer pour un problème
/// de clavier ou de pilote.
///
/// **Les TROIS types d'évènement reçoivent la correction, et c'est le fond du
/// correctif.** Une première version ne l'appliquait qu'aux évènements de
/// CARACTÈRE (`WM_CHAR`/`WM_SYSCHAR`), en supposant que seul l'insertion de
/// texte comptait — insuffisant, et la vérification l'a montré : Blink marque la
/// touche comme RACCOURCI dès le `keydown` et CONSOMME l'évènement, de sorte que
/// le caractère suivant est ignoré même s'il arrive propre. C'est bien la touche
/// qu'il fallait corriger en premier.
fn key_modifiers() -> u32 {
    alt_gr_adjusted(input_modifiers())
}

#[cfg(test)]
mod el_helper_tests {
    /// ⛔ **LE DÉFAUT QUI A VIDÉ LE PRESSE-PAPIERS DE SES IMAGES.** Chaque page de
    /// `assets/popups/` porte sa PROPRE copie du helper `el(tag, attrs, children)`.
    /// Le 2026-09-24, en élargissant `onclick` à tout `on…`, la branche de repli
    /// `else node.setAttribute(key, value)` a été **REMPLACÉE au lieu d'être
    /// gardée** dans cinq pages : tout attribut autre que `class` et `on…` était
    /// jeté en silence — le `src` des miniatures du presse-papiers, les `title`,
    /// les `type`/`placeholder` du dash. Aucune erreur, rien à l'écran que des
    /// images vides. Ce test lit TOUTES les pages (celles d'aujourd'hui et les
    /// suivantes) et exige que chaque `el()` garde son repli.
    #[test]
    fn chaque_el_des_pages_garde_son_repli_set_attribute() {
        let dossier = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/popups");
        let mut pages_avec_el = 0;
        let mut fautives = Vec::new();
        for entree in std::fs::read_dir(&dossier).expect("assets/popups lisible") {
            let chemin = entree.expect("entrée lisible").path();
            if chemin.extension().and_then(|e| e.to_str()) != Some("html") {
                continue;
            }
            let source = std::fs::read_to_string(&chemin).expect("page lisible");
            let Some(debut) = source.find("function el(") else { continue };
            pages_avec_el += 1;
            // Le corps du helper tient en une quinzaine de lignes : on s'arrête
            // à la boucle des enfants, qui suit toujours celle des attributs.
            let corps = &source[debut..];
            let fin = corps.find("children").and_then(|i| corps[i + 1..].find("children").map(|j| i + 1 + j));
            let corps = &corps[..fin.unwrap_or(corps.len().min(2000))];
            if !corps.contains(".setAttribute(") {
                fautives.push(chemin.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
        assert!(pages_avec_el >= 8, "seulement {pages_avec_el} pages avec el() trouvées");
        assert!(fautives.is_empty(), "el() sans repli setAttribute dans : {fautives:?}");
    }
}

#[cfg(test)]
mod tests_clics {
    use super::{compter_clic, Clic};

    const DELAI: u32 = 500;
    const ZONE: (i32, i32) = (4, 4);
    const AUCUN: Clic = Clic { instant: 0, x: 0, y: 0, nombre: 0 };

    #[test]
    fn deux_appuis_proches_et_rapides_font_un_double_clic() {
        let premier = compter_clic(AUCUN, 1_000, 50, 60, DELAI, ZONE);
        assert_eq!(premier.nombre, 1);
        assert_eq!(compter_clic(premier, 1_300, 51, 61, DELAI, ZONE).nombre, 2);
    }

    #[test]
    fn trop_lent_ou_trop_loin_repart_a_un() {
        let premier = compter_clic(AUCUN, 1_000, 50, 60, DELAI, ZONE);
        assert_eq!(compter_clic(premier, 1_501, 50, 60, DELAI, ZONE).nombre, 1);
        assert_eq!(compter_clic(premier, 1_100, 53, 60, DELAI, ZONE).nombre, 1);
    }

    #[test]
    fn le_triple_clic_plafonne_puis_repart() {
        let mut clic = AUCUN;
        let rangs: Vec<i32> = (0..4)
            .map(|i| {
                clic = compter_clic(clic, 1_000 + i * 100, 10, 10, DELAI, ZONE);
                clic.nombre
            })
            .collect();
        assert_eq!(rangs, [1, 2, 3, 1]);
    }

    #[test]
    fn l_horloge_qui_reboucle_ne_casse_pas_le_double_clic() {
        let premier = compter_clic(AUCUN, u32::MAX - 100, 5, 5, DELAI, ZONE);
        assert_eq!(compter_clic(premier, 150, 5, 5, DELAI, ZONE).nombre, 2);
    }
}

#[cfg(test)]
mod tests_epingle {
    use super::*;

    /// **LA RÈGLE DE LA PUNAISE, EN QUATRE CAS.** DEMANDE EXPLICITE : « un clic sur
    /// autre chose ne doit pas fermer un dash épinglé ».
    ///
    /// ⚠️ **C'EST LE SEUL TEST POSSIBLE DE CETTE RÈGLE** : elle est appliquée dans
    /// `hide_if_focus_elsewhere`, qui interroge `GetForegroundWindow` — donc elle ne
    /// s'éprouverait qu'en volant le focus d'une vraie fenêtre, et aucune entrée
    /// n'est simulée dans ce dépôt. D'où l'extraction.
    #[test]
    fn la_perte_de_focus_ne_ferme_pas_un_panneau_epingle() {
        // Le cas qui a motivé la punaise.
        assert!(
            !la_perte_de_focus_ferme(true, false),
            "epingle : la perte de focus NE DOIT PAS fermer"
        );
        // Sans punaise, on ferme — sinon la punaise ne serait pas la différence.
        assert!(la_perte_de_focus_ferme(false, false), "sans punaise, la perte de focus ferme");
        // Et un glissement en cours protège toujours, punaise ou pas (voir
        // `PopupSlot::dragging`) : c'est une raison INDÉPENDANTE.
        assert!(!la_perte_de_focus_ferme(false, true), "un glissement ne se coupe pas");
        assert!(!la_perte_de_focus_ferme(true, true));
    }

    /// **LE CONTRAT ENTRE `js_epingle` ET LE DÉCODEUR.** Le bouton que Rust fabrique
    /// navigue vers `neuradesk://nd/epingle`, et `drain_actions` reconnaît l'action
    /// sur son dernier segment — les deux doivent rester d'accord, et **rien d'autre
    /// ne le vérifierait** : le bouton est créé par du JS injecté, donc aucun test de
    /// contrat de page (comme celui du dash dossier) ne peut le voir.
    #[test]
    fn l_action_de_la_punaise_est_reconnue_sur_son_dernier_segment() {
        // Ce que le bouton injecté envoie.
        assert!(est_action_epingle("nd/epingle"));
        // Ce que les pages qui fournissent leur propre bouton enverraient, chacune
        // avec son préfixe.
        assert!(est_action_epingle("dash/epingle"));
        assert!(est_action_epingle("clipboard/epingle"));
        assert!(est_action_epingle("sous-dock/g/epingle"));

        // ⚠️ **ET LE MOT NU EST REFUSÉ** : il faut un segment avant, sinon le
        // contrat serait plus lâche que ce que `js_epingle` produit.
        assert!(!est_action_epingle("epingle"));
        // Un verbe qui contient le mot n'est pas la punaise.
        assert!(!est_action_epingle("dash/epingler"));
        assert!(!est_action_epingle("dash/desepingler"));
    }

    /// ⛔ **UN `SyntaxError` DANS CE JS SERAIT ENTIÈREMENT SILENCIEUX** — c'est la
    /// classe de panne que ce dépôt a déjà payée une journée entière (« une
    /// expression JS peut être un `SyntaxError` silencieux », et `execute_js` ne
    /// rend aucun compte de son échec). Le JS ci-dessous est assemblé par un
    /// `format!` avec des accolades ÉCHAPPÉES, donc une accolade de trop le
    /// casserait **sans que rien ne le dise** — et il tourne à chaque frame, pour
    /// chacun des quinze panneaux.
    ///
    /// ⚠️ Ce test ne parse pas du JavaScript : il vérifie l'ÉQUILIBRE des
    /// délimiteurs, qui est précisément ce qui casse dans ce genre d'assemblage, et
    /// que le JS injecté reste celui qu'on croit.
    #[test]
    fn le_js_de_la_punaise_est_equilibre_et_porte_ce_qu_on_croit() {
        for epingle in [true, false] {
            let js = js_epingle(epingle);
            for (ouvrant, fermant) in [('{', '}'), ('(', ')')] {
                let ecart =
                    js.matches(ouvrant).count() as i64 - js.matches(fermant).count() as i64;
                assert_eq!(ecart, 0, "desequilibre {ouvrant}{fermant} (epingle={epingle}) : {js}");
            }
            // Le bouton est retrouvé par son `id` — c'est le contrat avec les pages.
            assert!(js.contains("nd-punaise"), "le bouton est cherche par son id : {js}");
            // ⛔ **LE GLYPHE, EXACTEMENT — U+1F4CC (📌, la punaise inclinée).**
            // Défaut signalé : « l'icône de l'épingle dans le dash dossier n'est pas
            // la même que dans les autres dashs ». La page du dash dossier écrit
            // `\u{1F4CC}` ; ce JS écrivait `📍`, soit **U+1F4CD (📍, la
            // punaise RONDE)** — un seul caractère d'écart dans la paire de
            // substituts (`DCCC` contre `DCCD`), et deux dessins différents à
            // l'écran. Rien ne l'aurait signalé : les deux sont des punaises.
            //
            // ⚠️ **LE NEEDLE EST ASSEMBLÉ, PAS ÉCRIT D'UN BLOC** : un escape littéral
            // dans ce fichier est converti en EMOJI à l'écriture (constaté en
            // écrivant ce test) — l'assertion chercherait alors le CARACTÈRE, quand
            // le JS contient la SÉQUENCE. Assemblé, il traverse l'édition intact.
            let attendu = concat!("\\u", "D83D", "\\u", "DCCC");
            let ronde = concat!("\\u", "D83D", "\\u", "DCCD");
            assert!(
                js.contains(attendu),
                "le glyphe doit etre U+1F4CC, comme la page du dash dossier : {js}"
            );
            assert!(
                !js.contains(ronde),
                "U+1F4CD est la punaise RONDE, un autre dessin : {js}"
            );
            // L'action doit finir par `/epingle`, sinon `est_action_epingle` la
            // refuserait et le clic ne ferait RIEN, sans un mot.
            assert!(
                js.contains("neuradesk://nd/epingle"),
                "l'action envoyee doit etre reconnue par est_action_epingle : {js}"
            );
            // L'état voyage bien, dans les deux sens.
            assert!(js.contains(&format!("'{epingle}'")), "l'etat doit etre pose : {js}");
        }
    }
}

#[cfg(test)]
mod tests_zoom_dpi {
    use super::*;

    thread_local! {
        static SLOT: PopupSlot = const { PopupSlot::new() };
    }

    /// ⛔ **LE DÉFAUT EST `false`, ET C'EST CE QUI PROTÈGE LES ~25 AUTRES POPUPS.**
    ///
    /// Le ratio de DPI arrive DÉJÀ par un autre canal pour les panneaux (leur
    /// `contentScale` JS) et pour le Dash/Presse-papiers/Réglages (leur
    /// `SCREEN_SCALE_OVERRIDE`). Un défaut à `true` le leur appliquerait **deux
    /// fois** — ils paraîtraient alors plus PETITS que la primaire, ce qui est
    /// exactement le défaut inverse de celui qu'on corrige.
    #[test]
    fn une_fenetre_a_la_taille_des_textes_grandit_sans_quitter_l_ecran() {
        // Une fenêtre qui veut 1200 × 900 px, plafonnée à 85 % de l'écran.
        fn taille(l: i32, h: i32) -> [i32; 2] {
            [1200.min(l * 85 / 100), 900.min(h * 85 / 100)]
        }
        assert_eq!(taille_a_l_echelle(1920, 1080, 1.0, taille), [1200, 900], "neutre");
        assert_eq!(taille_a_l_echelle(1920, 1080, 1.25, taille), [1500, 918], "plus grande, plafonnée au vrai écran");
        assert_eq!(taille_a_l_echelle(3840, 2160, 0.5, taille), [600, 450]);
    }

    #[test]
    fn le_zoom_ne_suit_pas_le_dpi_par_defaut() {
        SLOT.with(|s| assert!(!s.zoom_suit_le_dpi.get(), "le defaut doit etre false"));
        regler_zoom_dpi(&SLOT, true);
        SLOT.with(|s| assert!(s.zoom_suit_le_dpi.get()));
        regler_zoom_dpi(&SLOT, false);
        SLOT.with(|s| assert!(!s.zoom_suit_le_dpi.get()));
    }
}

/// ⭐ **UNE PAGE ZOOMÉE GARDE SA LARGEUR PAR `100vw`, JAMAIS PAR `100%`** — DÉFAUT
/// SIGNALÉ (2026-10-18) : « les dashs n'ont pas tous une croix de fermeture ».
/// Sous `zoom`, un pourcentage se résout DÉJÀ dans le repère zoomé : diviser `100%`
/// par l'échelle élargissait la page (143 % de la fenêtre à 70 %), et la croix
/// sortait du cadre à droite. `100vw`, lui, se divise juste — c'est ce que faisait
/// déjà `sous_dock.html`, le seul panneau épargné.
#[cfg(test)]
mod tests_largeur_zoomee {
    #[test]
    fn aucune_page_ne_divise_100_pourcent_par_son_echelle() {
        let dossier = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/popups");
        let mut fautives = Vec::new();
        for entree in std::fs::read_dir(&dossier).expect("assets/popups") {
            let chemin = entree.expect("entree").path();
            if chemin.extension().is_some_and(|e| e == "html") {
                let page = std::fs::read_to_string(&chemin).expect("page lisible");
                if page.lines().any(|l| l.trim_start().starts_with("width: calc(100% / var(--")) {
                    fautives.push(chemin.file_name().unwrap().to_string_lossy().into_owned());
                }
            }
        }
        assert!(fautives.is_empty(), "largeur zoomee par 100% (utiliser 100vw) : {fautives:?}");
    }
}

#[cfg(test)]
mod tests_sous_le_dock {
    use super::*;
    /// ⛔ Un panneau qui finirait au-dessus du dock passe sous lui (sinon il
    /// recouvre ses congés) ; tout autre rang demandé est respecté.
    #[test]
    fn un_panneau_remonte_se_pose_sous_le_dock() {
        let dock = HWND(0x1234 as *mut _);
        let ime = HWND(0x5678 as *mut _);
        let dessous = HWND(0x9abc as *mut _);
        let au_dessus = |h: HWND| h == ime;
        assert_eq!(insertion_d_un_panneau(HWND_TOPMOST, false, dock, au_dessus), Some(dock), "remonté en tête");
        assert_eq!(insertion_d_un_panneau(HWND_TOP, true, dock, au_dessus), Some(dock), "activé, déjà topmost");
        assert_eq!(insertion_d_un_panneau(ime, true, dock, au_dessus), Some(dock), "sous une fenêtre au-dessus du dock (mesuré : l'IME)");
        assert_eq!(insertion_d_un_panneau(dessous, true, dock, au_dessus), None, "sous une fenêtre déjà sous le dock");
        assert_eq!(insertion_d_un_panneau(HWND_TOP, false, dock, au_dessus), None, "punaise éteinte : reste dans les fenêtres ordinaires");
        assert_eq!(insertion_d_un_panneau(HWND_NOTOPMOST, true, dock, au_dessus), None);
        assert_eq!(insertion_d_un_panneau(HWND_BOTTOM, true, dock, au_dessus), None);
    }

    /// ⛔ Le rattrapage sur le résultat, avec de VRAIES fenêtres : un panneau
    /// qui a fini devant le dock (mesuré : montré « sous l'IME ») repasse
    /// dessous ; déjà dessous, il ne bouge pas.
    #[test]
    fn un_panneau_devant_le_dock_repasse_dessous() {
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP};
        let creer = || unsafe {
            CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, windows::core::w!("STATIC"), windows::core::w!(""), WS_POPUP, 0, 0, 1, 1, None, None, None, None).unwrap()
        };
        let [dock, panneau] = [creer(), creer()];
        let haut = |h| unsafe { SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
        let _ = (haut(dock), haut(panneau));
        assert!(au_dessus_de(panneau, dock), "départ : le panneau devant le dock");

        ramener_sous(panneau, dock);
        assert!(!au_dessus_de(panneau, dock) && au_dessus_de(dock, panneau), "le panneau repasse sous le dock");

        ramener_sous(panneau, dock);
        assert!(au_dessus_de(dock, panneau), "déjà dessous : il y reste");
        for h in [dock, panneau] {
            let _ = unsafe { DestroyWindow(h) };
        }
    }

    /// ⭐ **PENDANT UNE MAGNIFICATION, LES MENUS AUSSI SE POSENT SOUS LE DOCK** —
    /// DEMANDE EXPLICITE (2026-11-11) : « l'effet de magnification doit faire
    /// apparaître les icônes au-dessus des ombrages, des dashs et des menus ».
    ///
    /// Les deux fonctions qui rangent les menus lisent le drapeau et l'ANCRE DE
    /// `ombre_barre` (la même que les ombres, sinon menus et ombres changeraient
    /// de rang l'un par rapport à l'autre) ; la famille vient de
    /// `PopupSpec::above_a_magnified_dock`, donc un PANNEAU garde son chemin.
    ///
    /// **MUTATION** : retirer la lecture du drapeau de `raise_visible_popups` —
    /// les menus repassent devant l'icône agrandie et le test le nomme.
    #[test]
    fn la_magnification_met_les_menus_sous_le_dock() {
        // Le porte-à-faux part de `raise_visible_popups` et s'arrête au PREMIER
        // module de tests qui suit : le fichier a des modules de tests AVANT ces
        // fonctions, donc couper sur le premier `#[cfg(test)]` rendrait une source
        // vide — et lire tout le fichier compterait les littéraux de CE test.
        let tout = include_str!("cef_popup.rs");
        let debut = tout.find("pub fn raise_visible_popups()").expect("`raise_visible_popups` a disparu");
        let suite = &tout[debut..];
        let fin = suite.find("#[cfg(test)]").expect("aucun module de tests après les deux fonctions");
        let source = &suite[..fin];
        assert_eq!(
            source.matches("crate::ui::dock::magnification_en_cours()").count(),
            2,
            "`raise_visible_popups` et `raise_above_tooltips` doivent lire le drapeau"
        );
        assert_eq!(
            source.matches("crate::win32::ombre_barre::ancre_de_magnification()").count(),
            2,
            "les deux fonctions doivent viser le MÊME ancrage que les ombres"
        );
        assert!(
            source.contains("sous.unwrap_or(HWND_TOPMOST)"),
            "au repos, les menus doivent retrouver la tête de la bande topmost"
        );
        assert!(
            source.contains("if menu && crate::ui::dock::magnification_en_cours()"),
            "un PANNEAU n'a pas le rang d'un menu : le drapeau ne vaut que pour `menu`"
        );
        let appels = source.matches("raise_above_tooltips(").count();
        let avec_famille = source.matches("spec.above_a_magnified_dock)").count();
        assert_eq!(
            (appels, avec_famille),
            (3, 2),
            "la famille du popup doit être transmise aux DEUX appels de `toggle_with` \
             (le troisième `raise_above_tooltips(` est la définition)"
        );
    }
}

#[cfg(test)]
mod tests_echelle_du_menu_en_page {
    use super::{echelle_du_menu_en_page, echelle_du_menu_js, zoom_level_for_factor, CONTENT_ZOOM_FACTOR};

    /// Une page au zoom neutre (×3) : le menu prend « Taille des menus » ×
    /// « Taille des textes » — 0,7 × 0,85 chez l'utilisateur.
    #[test]
    fn le_menu_suit_la_taille_des_menus_et_des_textes() {
        let e = echelle_du_menu_en_page(zoom_level_for_factor(CONTENT_ZOOM_FACTOR), 1.0, 0.7, 0.85);
        assert!((e - 0.595).abs() < 1e-6, "{e}");
    }

    /// L'éditeur porte déjà les textes dans SON zoom : le menu ne les recompte pas.
    #[test]
    fn un_zoom_de_page_deja_grossi_n_est_pas_compte_deux_fois() {
        let e = echelle_du_menu_en_page(zoom_level_for_factor(CONTENT_ZOOM_FACTOR * 0.85), 1.0, 0.7, 0.85);
        assert!((e - 0.7).abs() < 1e-6, "{e}");
    }

    #[test]
    fn un_zoom_illisible_rend_l_echelle_neutre() {
        assert_eq!(echelle_du_menu_en_page(0.0, 1.0, f32::NAN, 1.0), 1.0);
        assert_eq!(echelle_du_menu_js(0.595), "document.documentElement.style.setProperty('--nd-echelle-menu','0.5950')");
    }

    /// La page lit bien la variable que Rust pose, et ne garde plus une taille fixe.
    #[test]
    fn le_menu_de_page_lit_l_echelle_posee_par_rust() {
        let js = include_str!("../../assets/popups/menu_contextuel.js");
        assert!(js.contains("--nd-echelle-menu"));
    }

    /// ⛔ La feuille du menu est une chaîne JS entre ACCENTS GRAVES : un accent
    /// grave dans un de ses commentaires la fermait, le script entier ne se
    /// lisait plus et AUCUN clic droit n'ouvrait plus rien (2026-11-01). La
    /// chaîne doit se refermer sur la ligne qui ne porte que sa fin.
    #[test]
    fn la_feuille_du_menu_ne_se_referme_pas_en_route() {
        let js = include_str!("../../assets/popups/menu_contextuel.js");
        let debut = js.find("style.textContent = `").expect("feuille du menu") + "style.textContent = `".len();
        let fin = debut + js[debut..].find('`').expect("fin de la feuille");
        assert!(js[fin..].starts_with("`;"), "accent grave en pleine feuille : {:?}", &js[fin.saturating_sub(60)..fin + 1]);
        assert!(js[debut..fin].contains(".menu-contextuel .trait"), "la feuille s'arrête avant sa dernière règle");
    }
}

thread_local! {
    /// **LA TABLE PERSISTÉE DES TAILLES DE MENUS** (`tailles_menus.txt`, voir
    /// `crate::config::load_tailles_menus`) — chargée au PREMIER usage, pas au
    /// démarrage : les popups ne servent à rien dans les premières frames, et un
    /// fichier illisible ne doit pas retarder la boucle.
    static TAILLES_MENUS: RefCell<Option<std::collections::HashMap<String, ([i32; 2], f64)>>> =
        const { RefCell::new(None) };
}

/// Prête la table, chargée une fois — voir `TAILLES_MENUS`.
fn avec_les_tailles<R>(f: impl FnOnce(&mut std::collections::HashMap<String, ([i32; 2], f64)>) -> R) -> R {
    TAILLES_MENUS.with(|cell| {
        let mut garde = cell.borrow_mut();
        let table = garde.get_or_insert_with(crate::config::load_tailles_menus);
        f(table)
    })
}

/// **LA TAILLE D'OUVERTURE D'UN MENU : CELLE QUE SA PAGE A DEMANDÉE LA DERNIÈRE
/// FOIS** — y compris au lancement PRÉCÉDENT (voir `crate::config::
/// load_tailles_menus`).
///
/// DEMANDE EXPLICITE (2026-11-10) : « serait-il possible de pré calculer les
/// menus, je les vois se redimensionner quand je les ouvre ». La page mesure son
/// contenu et l'annonce APRÈS l'ouverture (voir `request_content_measure`) ;
/// partir de l'estimation d'`app::menu_rect` à chaque fois, c'est garantir un
/// redimensionnement visible pendant le fondu. La dernière taille RÉELLE, elle,
/// est exacte pour un contenu inchangé — le cas courant.
///
/// ⚠️ **LE CAS OÙ ÇA SE TROMPE EXISTE, ET IL EST BORNÉ** : les menus dont le
/// contenu dépend des PARAMÈTRES d'ouverture (`cef_spacer_size` : 1, 2 ou 4
/// rangées ; `cef_context` : sous-menu dépliable) peuvent rouvrir une ou deux
/// rangées trop court avant que la mesure fraîche — demandée à CHAQUE
/// ouverture — ne corrige, ~100 ms plus tard, alors que le fondu de 500 ms n'est
/// qu'à ~20 % d'opacité. C'est le prix assumé de l'absence de saut dans le cas
/// courant, et la mesure reste l'arbitre.
fn taille_rappelee(slot: &'static LocalKey<PopupSlot>, rect: [i32; 4]) -> [i32; 4] {
    let (cle, facteur) = slot.with(|s| (s.identite.get(), s.content_factor.get()));
    let Some(cle) = cle else { return rect };
    let memorisee = avec_les_tailles(|table| table.get(cle).copied());
    let taille = taille_d_ouverture(memorisee, facteur, MENU_ROOM.with(std::cell::Cell::get), rect, menu_centre());
    if taille != rect {
        crate::diag!("[menu/taille] rappelee {taille:?} depuis {cle} (facteur {facteur})");
    }
    taille
}

/// La décision PURE de `taille_rappelee` — sans fichier ni `thread_local`, donc
/// éprouvable (voir `tests_taille_rappelee`).
///
/// **LA TAILLE EST RAMENÉE À L'ÉCHELLE ACTUELLE** par le rapport des
/// `content_factor` : la table est en pixels PHYSIQUES d'ALORS, et bouger
/// « Taille des menus » entre deux lancements doit la faire suivre, pas la figer.
/// Puis elle est BORNÉE À LA ZONE LIBRE, comme `resize_to_content` : une taille
/// mémorisée sur un écran 4K ne doit pas ouvrir une fenêtre plus grande que la
/// place disponible. Rend `rect` tel quel sans mémoire, ou pour un popup que son
/// contenu ne pilote pas (facteur `0.0` : les PANNEAUX, voir `content_factor`).
///
/// ⛔ **LA POSITION SUIT LA TAILLE, ET ELLE AUSSI EST BORNÉE** — c'est le défaut
/// réel de la version qui ne rendait que `[rect[0], rect[1], w, h]`. `rect` a été
/// posé par `app::menu_rect` pour la taille PROVISOIRE ; lui donner une largeur ou
/// une hauteur d'ouverture plus grande sans retoucher `x`/`y` fait déborder le
/// menu de la zone libre, **par-dessus la barre de droite ou du bas**. MESURÉ au
/// journal (`NEURADESK_DIAG`) : `[menu/place] ancre=(3708,66) … provisoire=
/// (2889,66,821,1000)` puis `[menu/taille] rappelee [2889, 66, 1406, 2042]` — un
/// menu de 1406 px ancré à x=2889 dans une zone qui finit à 3710 : 585 px
/// par-dessus la barre de droite, et la mesure suivante ne le rattrapait pas
/// (`resized_x` ne recule que ce qui TOUCHE la limite, et ce n'était plus le cas
/// après la substitution). « certains menus sont mal placés et sont trop grands
/// par rapport à l'espace disponible dans la zone centrale ».
///
/// ⇒ Les deux règles de `resize_to_content` sont reprises, dans le même ordre :
/// `resized_x`/`resized_y` d'abord (le bord par lequel le menu pend à son icône
/// ne bouge pas), puis un bornage strict — un menu PLUS GRAND que la place
/// restante du côté de son ancre n'a plus d'autre choix que de glisser, et y
/// glisser vaut mieux que mordre sur une barre. `centre` vient de
/// `set_menu_centre` : un panneau centré le reste, comme au redimensionnement.
fn taille_d_ouverture(
    memorisee: Option<([i32; 2], f64)>,
    facteur: f64,
    room: [i32; 4],
    rect: [i32; 4],
    centre: bool,
) -> [i32; 4] {
    let Some((taille, facteur_alors)) = memorisee else { return rect };
    if facteur <= 0.0 || facteur_alors <= 0.0 {
        return rect;
    }
    let echelle = facteur / facteur_alors;
    let mut w = ((taille[0] as f64) * echelle).round() as i32;
    let mut h = ((taille[1] as f64) * echelle).round() as i32;
    if room[2] > 0 {
        w = w.min(room[2]);
    }
    if room[3] > 0 {
        h = h.min(room[3]);
    }
    let w = w.max(1);
    let h = h.max(1);
    // Sans zone retenue (aucun popup aujourd'hui), rien ne permet de borner la
    // position : on garde `rect` comme avant — même repli que `resize_to_content`.
    if room[2] <= 0 || room[3] <= 0 {
        return [rect[0], rect[1], w, h];
    }
    let (gauche, droite) = (room[0], room[0] + room[2]);
    let (haut, bas) = (room[1], room[1] + room[3]);
    let x = if centre {
        // Le centrage est recalculé pour la NOUVELLE largeur — même formule que
        // `resize_to_content`, et pour la même raison (voir `set_menu_centre`).
        gauche + (droite - gauche - w) / 2
    } else {
        // `resized_x` garde le bord droit d'un menu qui le touchait ; le `clamp`
        // couvre les deux autres cas — un menu qui débordait déjà sans être collé,
        // et une position provisoire hors de la zone (mesurée : `x = 0` pour une
        // zone qui commence à 124).
        resized_x(rect[0], rect[2], w, droite, gauche).clamp(gauche, (droite - w).max(gauche))
    };
    // Le haut est retenu quand le menu pend d'une icône ; `resized_y` ne remonte
    // que celui dont le bas touchait la limite (voir sa doc), et le `clamp` ramène
    // les autres dans la zone.
    let y = resized_y(rect[1], rect[3], h, bas, haut).clamp(haut, (bas - h).max(haut));
    [x, y, w, h]
}

/// Deux tailles SONT LA MÊME à la tolérance près — sert à deux endroits, et
/// c'est voulu : la confirmation d'une mesure (`appliquer_la_mesure`) et la
/// décision de NE PAS déplacer la fenêtre pour ce résidu (`resize_to_content`).
/// Si les deux seuils divergeaient, une mesure « confirmée » pourrait encore
/// provoquer un saut visible de quelques pixels.
///
/// La tolérance est en pixels PHYSIQUES : les annonces d'un même contenu
/// peuvent varier de une ou deux unités CSS selon la barre de défilement
/// (`296` puis `297`).
fn mesure_concordante(precedente: [i32; 2], courante: [i32; 2]) -> bool {
    (precedente[0] - courante[0]).abs() <= MESURE_TOLERANCE
        && (precedente[1] - courante[1]).abs() <= MESURE_TOLERANCE
}

/// **APPLIQUE UNE MESURE — SI ELLE EST DIGNE DE FOI.** Voir `MESURE_TOLERANCE`
/// pour la mesure du défaut, et `PopupSlot::ouverture_a`.
///
/// Trois issues, et chacune couvre un cas réel :
/// 1. **hors stabilisation** (ou plafond `MESURE_OUVERTURE_MAX` dépassé) : on
///    applique, comme avant — c'est le régime permanent ;
/// 2. **annonce CONCORDANTE** avec la précédente : c'est la vraie taille, on
///    applique et on sort de la stabilisation ;
/// 3. **annonce isolée** : on la retient (`mesure_en_attente`) sans y toucher.
///    Elle s'appliquera si aucune autre n'arrive pendant `MESURE_SILENCE` (voir
///    la fin de `drain_actions`).
fn appliquer_la_mesure(slot: &'static LocalKey<PopupSlot>, cible: [i32; 2]) {
    let maintenant = Instant::now();
    let appliquer = slot.with(|s| {
        let Some(ouverture) = s.ouverture_a.get() else { return true };
        // ⚠️ `ouverture_a` N'EST PAS REMIS À `None` ICI : c'est
        // `retenir_pour_l_ouverture` qui le fera, au moment où la taille est
        // réellement appliquée. L'effacer avant laissait `resize_to_content`
        // croire à un redimensionnement ordinaire, et la taille d'ouverture
        // n'était alors JAMAIS retenue pour la fois suivante.
        if maintenant.duration_since(ouverture) >= MESURE_OUVERTURE_MAX {
            s.mesure_en_attente.set(None);
            return true;
        }
        let precedente = s.mesure_precedente.get();
        s.mesure_precedente.set(Some(cible));
        if precedente.is_some_and(|precedente| mesure_concordante(precedente, cible)) {
            s.mesure_en_attente.set(None);
            crate::diag!("[menu/mesure] confirmee {cible:?}");
            true
        } else {
            s.mesure_en_attente.set(Some((cible, maintenant)));
            crate::diag!("[menu/mesure] en attente de confirmation {cible:?}");
            false
        }
    });
    if appliquer {
        resize_to_content(slot, Some(cible[0]), Some(cible[1]));
    }
}

/// **RETIENT LA TAILLE — SI CELLE-CI EST CELLE DE L'OUVERTURE.**
///
/// Appelée par `resize_to_content` à chaque redimensionnement piloté par la
/// page, mais elle ne retient QUE celui qui suit l'ouverture (marquée par
/// `PopupSlot::ouverture_a`) : c'est la taille à laquelle le menu doit
/// naître la prochaine fois.
///
/// ⛔ **ET PAS CELLE QU'UN SOUS-MENU DÉPLIÉ DONNE ENSUITE.** Le menu des
/// fréquences s'ouvre replié ; un utilisateur qui le déplie redimensionne la
/// fenêtre APRÈS l'ouverture. Retenir cette taille-là ferait naître le menu
/// déplié, puis le faire rétrécir une fois la page remesurée — c'est-à-dire
/// le saut même qu'on supprime, à chaque ouverture, pour tout le monde.
fn retenir_pour_l_ouverture(slot: &PopupSlot, taille: [i32; 2]) {
    let ouverture = slot.ouverture_a.get().is_some();
    slot.ouverture_a.set(None);
    if !ouverture {
        return;
    }
    if let Some(cle) = slot.identite.get() {
        memoriser_taille(cle, taille, slot.content_factor.get());
    }
}

/// **RETIENT LA TAILLE DEMANDÉE PAR LA PAGE, POUR LES FOIS SUIVANTES** — en
/// mémoire et sur disque (voir `taille_rappelee`). Appelée par
/// `retenir_pour_l_ouverture`, seul chemin qui décide QUELLE taille retenir.
///
/// **ON N'ÉCRIT QUE SI QUELQUE CHOSE CHANGE** : cette fonction est traversée à
/// chaque pas de la cascade d'ouverture (une fenêtre qui passe par deux ou trois
/// tailles), et réécrire le fichier pour une valeur identique serait du bruit.
fn memoriser_taille(cle: &str, taille: [i32; 2], facteur: f64) {
    if facteur <= 0.0 || taille[0] <= 0 || taille[1] <= 0 {
        return;
    }
    avec_les_tailles(|table| {
        let change = table.get(cle).is_none_or(|(ancienne, ancien_facteur)| {
            *ancienne != taille || (ancien_facteur - facteur).abs() > 1e-6
        });
        if change {
            table.insert(cle.to_string(), (taille, facteur));
            crate::config::save_tailles_menus(table);
        }
    });
}

/// **LA CONFIRMATION D'UNE MESURE** — le correctif de la cascade de
/// redimensionnements (voir `MESURE_TOLERANCE` et `appliquer_la_mesure`).
#[cfg(test)]
mod tests_mesure_confirmee {
    use super::*;

    /// Deux annonces d'un MÊME contenu peuvent varier de une ou deux unités
    /// (barre de défilement) : la tolérance les rapproche.
    #[test]
    fn une_variation_d_unites_css_est_concordante() {
        assert!(mesure_concordante([621, 504], [624, 504]));
        assert!(mesure_concordante([622, 504], [622, 504]));
    }

    /// Deux étapes DIFFÉRENTES de la cascade ne sont pas concordantes : c'est
    /// tout ce qui empêche la fenêtre de suivre la page pendant qu'elle se
    /// reconstruit (mesuré : 439×630, 439×672, 626×544, 624×504, 622×504).
    #[test]
    fn deux_etapes_de_la_cascade_ne_se_confirment_pas() {
        assert!(!mesure_concordante([439, 630], [439, 672]), "hauteur en construction");
        assert!(!mesure_concordante([439, 672], [626, 544]), "largeur et hauteur");
        assert!(!mesure_concordante([626, 544], [297 * 2, 240 * 2]));
    }

    /// Le seuil est INCLUSIF : quatre pixels passent, cinq non.
    #[test]
    fn le_seuil_est_inclusif() {
        assert!(mesure_concordante([100, 100], [104, 96]));
        assert!(!mesure_concordante([100, 100], [105, 100]));
    }
}

#[cfg(test)]
mod tests_taille_rappelee {
    use super::*;

    /// Sans mémoire (jamais mesuré, ou fichier absent) : l'estimation de
    /// l'appelant est rendue telle quelle.
    #[test]
    fn sans_mesure_anterieure_l_estimation_est_rendue() {
        assert_eq!(
            taille_d_ouverture(None, 2.1, [124, 54, 3586, 2042], [10, 20, 672, 531], false),
            [10, 20, 672, 531]
        );
    }

    /// Une mesure existe : on rouvre à SA taille, position de l'appelant intacte
    /// — ici la taille rappelée est PLUS PETITE et la position tient déjà.
    #[test]
    fn la_derniere_taille_de_la_page_reprend_la_main() {
        let t = taille_d_ouverture(Some(([622, 504], 2.1)), 2.1, [124, 54, 3586, 2042], [124, 100, 672, 531], false);
        assert_eq!(t, [124, 100, 622, 504]);
    }

    /// « Taille des menus » a bougé entre les deux lancements : la taille
    /// rappelée suit l'échelle, elle ne revient pas à l'ancienne.
    #[test]
    fn la_taille_rappelee_suit_l_echelle_des_menus() {
        let [_, _, w, h] = taille_d_ouverture(Some(([622, 504], 2.1)), 3.0, [124, 54, 3586, 2042], [124, 100, 0, 0], false);
        let attendu = ((622.0_f64 * 3.0 / 2.1).round() as i32, (504.0_f64 * 3.0 / 2.1).round() as i32);
        assert_eq!((w, h), attendu, "echelle appliquee");
    }

    /// Une taille mémorisée sur un autre écran ne peut pas dépasser la zone
    /// libre — c'est le même plafond que `resize_to_content`. Et la POSITION est
    /// ramenée dans la zone en même temps que la taille y est bornée.
    #[test]
    fn la_taille_rappelee_est_bornee_par_la_zone_libre() {
        let t = taille_d_ouverture(Some(([1498, 1132], 2.1)), 2.1, [124, 54, 900, 600], [0, 0, 672, 531], false);
        assert_eq!(t, [124, 54, 900, 600], "largeur, hauteur ET position bornees");
    }

    /// **LE DÉFAUT MESURÉ** (2026-11-11) : un menu ancré près de la barre de
    /// droite, dont la taille rappelée est plus GRANDE que l'estimation de
    /// `menu_rect`. La position posée pour l'estimation (bord droit sur la limite
    /// de la zone, `x + 821 = 3710`) ne vaut plus pour 1406 px de large : sans
    /// recul, le menu déborde de 585 px par-dessus la barre de droite.
    #[test]
    fn une_taille_rappelee_plus_grande_fait_reculer_le_menu() {
        // Les nombres du journal : `[menu/place] ancre=(3708,66) room=[124, 54,
        // 3586, 2042] provisoire=(2889,66,821,1000)`, puis `[menu/taille]
        // rappelee [2889, 66, 1406, 2042]`.
        let t = taille_d_ouverture(Some(([1406, 2042], 2.1)), 2.1, [124, 54, 3586, 2042], [2889, 66, 821, 1000], false);
        assert_eq!(t[2], 1406, "premisse : la largeur rappelee est plus grande que l'estimation");
        assert_eq!(t[0] + t[2], 3710, "le bord droit reste sur la limite de la zone libre");
        assert_eq!(t[0], 2304, "3710 - 1406 : le menu recule au lieu de mordre sur la barre");
        assert_eq!(t[1] + t[3], 2096, "et son bas reste sur le bord de la zone, pas sur la barre du bas");
        assert!(t[0] >= 124 && t[1] >= 54, "jamais hors de la zone : {t:?}");
    }

    /// Un panneau CENTRÉ le reste quand la taille rappelée n'est pas celle de
    /// l'estimation — même règle que `resize_to_content` (voir `set_menu_centre`).
    #[test]
    fn un_menu_centre_le_reste_a_la_taille_rappelee() {
        let t = taille_d_ouverture(Some(([1406, 800], 2.1)), 2.1, [124, 54, 3586, 2042], [100, 66, 821, 1000], true);
        assert_eq!(t[0] - 124, 3710 - (t[0] + t[2]), "marges egales : {t:?}");
    }

    /// Un PANNEAU n'est pas piloté par son contenu (facteur `0.0`) : sa taille
    /// de zone l'emporte, une mesure ne doit pas la remplacer.
    #[test]
    fn un_panneau_ne_rappelle_rien() {
        assert_eq!(
            taille_d_ouverture(Some(([622, 504], 2.1)), 0.0, [0, 0, 0, 0], [0, 0, 1200, 900], false),
            [0, 0, 1200, 900]
        );
    }
}

