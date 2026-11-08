//! Fenêtre CEF pour une CONFIRMATION avant une action destructive — voir
//! `assets/popups/confirm.html`, doc d'en-tête, pour le défaut réel qui l'a
//! motivée.
//!
//! **RÉUTILISABLE, et c'est le point** : `ask` prend un message et une ACTION
//! (`Confirmed`), pas un cas particulier. Le jour où une deuxième action
//! destructive arrivera, elle n'aura qu'à appeler `ask` — la fenêtre, la page
//! et le protocole de réponse sont déjà là.
//!
//! **L'ACTION EST RETENUE CÔTÉ RUST** (`PENDING`), jamais renvoyée par la page :
//! celle-ci n'envoie qu'un verbe (`confirm/yes`/`confirm/no`) et ne sait même
//! pas ce qu'elle confirme. C'est ce qui rend impossible qu'un « oui » exécute
//! autre chose que ce qui était affiché — la page ne peut pas se tromper sur ce
//! qu'elle ne connaît pas.
//!
//! **L'action en attente ne peut pas être exécutée à contretemps, parce qu'`ask`
//! ÉCRASE la précédente** avant de montrer la fenêtre, et qu'il est le seul à
//! pouvoir la montrer : un « oui » ne peut être lu que pour ce qu'`ask` vient de
//! poser. C'est cette propriété-là qui protège, et non une remise à zéro
//! périodique — voir `poll`, doc, pour pourquoi celle-ci a été retirée.
//!
//! **La réponse REFERME la fenêtre** (voir `dispatch_actions`) : une
//! confirmation qui resterait affichée après avoir répondu se lit comme un clic
//! mort — défaut réel signalé, « je ne peux pas annuler… ni confirmer en
//! cliquant ».
//!
//! Toute la mécanique fenêtre/rendu/relais souris est dans
//! `win32::cef_popup` — voir `win32::cef_test_window` pour la doc de module
//! complète du pattern suivi ici.

use std::cell::{Cell, RefCell};
use std::path::Path;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::menu;

/// Ce qu'une confirmation ENGAGE — ce qu'`app::run` exécute si l'utilisateur
/// confirme. **Un enum plutôt qu'un message libre** : c'est lui qui porte
/// l'ACTION, et le `match` de l'appelant le force à traiter chaque variante —
/// ajouter une action destructive sans la câbler ne compilerait pas.
#[derive(Clone, Debug, PartialEq)]
pub enum Confirmed {
    /// Retirer l'espace `index` de la barre de l'écran désigné (`None` = barre
    /// PRIMAIRE) — voir `ui::dock::Dock::remove_spacer`, qui ne le retire QUE
    /// de l'ordre d'affichage et laisse son entrée dans `icons` (voir sa doc).
    RemoveSpacer { index: usize, target: Option<String> },
    /// **Supprimer un fichier de l'utilisateur, VERS LA CORBEILLE.**
    ///
    /// ⛔ C'est la seule action de ce dépôt qui touche à des données de
    /// l'utilisateur, et elle passe par ici — c'est-à-dire par une confirmation
    /// explicite — pour la raison écrite trois fois dans le code : « créer est
    /// réparable, **supprimer ne l'est pas** ».
    ///
    /// ⚠️ **LE CÔTÉ EST DANS LA CHARGE** : quand la réponse arrive (des frames
    /// plus tard), il faut savoir QUELLE MOITIÉ relire et où écrire le compte
    /// rendu. Le retrouver depuis le chemin demanderait de comparer des
    /// dossiers parents, ce que le module sait déjà mieux que ce fichier.
    SupprimerFichier { chemin: std::path::PathBuf, cote: crate::win32::sous_dock::Cote },
}

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// L'action en attente de réponse — `None` quand rien n'est demandé. Voir
    /// la doc de module pour pourquoi elle vit ici et pas dans la page.
    static PENDING: RefCell<Option<Confirmed>> = const { RefCell::new(None) };
    /// Le texte affiché — retenu ici parce que `poll` n'a aucun moyen de le
    /// relire : il est appelé à chaque frame et ne reçoit rien. Même raison que
    /// `cef_spacer_size::CURRENT_STATE`.
    static CURRENT_STATE: RefCell<String> = const { RefCell::new(String::new()) };
    /// Numéro d'OUVERTURE, incrémenté à chaque `ask` — voir `push_state` et
    /// `assets/popups/confirm.html`, doc de `appliedOpen`. Même rôle que
    /// `cef_spacer_size::OPEN_COUNT`.
    static OPEN_COUNT: Cell<u64> = const { Cell::new(0) };
    /// Voir `app::content_scale_pour`, doc de fonction — mémorisé ici à
    /// l'ouverture (voir `ask`) et relu à chaque frame par `render_if_visible`.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefConfirmWindow"),
    window_title: w!("NeuraDesk — Confirmation"),
    html_file: "confirm.html",
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

/// Demande confirmation de `action`, avec `message` pour l'expliquer.
///
/// `rect` : où poser la fenêtre (l'appelant l'ancre à l'élément concerné, voir
/// `app::menu_rect`). Une NOUVELLE demande REMPLACE la précédente — une
/// seule confirmation à la fois, comme tous les popups de ce projet.
pub fn ask(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32, message: &str, action: Confirmed) {
    PENDING.with(|p| *p.borrow_mut() = Some(action));
    CURRENT_STATE.with(|m| *m.borrow_mut() = message.to_string());
    CONTENT_SCALE.with(|c| c.set(content_scale));
    // Une ouverture de plus : la page appliquera cet état, MÊME si elle a déjà
    // appliqué le précédent (voir `OPEN_COUNT`).
    OPEN_COUNT.with(|c| c.set(c.get() + 1));
    cef_popup::toggle(&SLOT, &SPEC, rect, gfx);
    // Tentative immédiate — perdue en silence si la page n'a pas fini de
    // charger, c'est `poll` qui la rattrape (voir sa doc).
    push_state();
}

/// Pousse l'état de l'ouverture courante dans la page via `window.setState`
/// (voir `assets/popups/confirm.html`).
fn push_state() {
    let open = OPEN_COUNT.with(Cell::get);
    let message = CURRENT_STATE.with(|m| m.borrow().clone());
    // Échappement JSON minimal. Les messages viennent de NOTRE code, jamais
    // d'une saisie — mais un guillemet ou un antislash y produirait un
    // littéral invalide, donc un `window.setState` REFUSÉ EN SILENCE par le
    // moteur JS, donc une fenêtre au titre vide. Le défaut ne dirait pas son
    // nom : mieux vaut l'échapper que le découvrir.
    let message = message.replace('\\', "\\\\").replace('"', "\\\"");
    cef_popup::execute_js(&SLOT, &format!("window.setState({{open: {open}, message: \"{message}\"}})"));
}

/// À appeler à CHAQUE frame tant que la fenêtre est visible (voir `app::run`,
/// même endroit que `win32::cef_spacer_size::poll`) — même remède au même bug :
/// la poussée faite à l'ouverture peut précéder le `<script>` de la page et être
/// perdue en silence.
///
/// **Ne fait QUE repousser l'état, et rien d'autre.** Une version antérieure
/// oubliait ici l'action en attente dès que la fenêtre n'était plus visible,
/// pour qu'une demande fermée sans réponse ne soit pas exécutée par la suivante.
/// C'était inutile, et c'était un risque : la fenêtre étant présentée à CHAQUE
/// frame, une seule frame où `is_visible` répond faux (création, clignotement)
/// aurait suffi à désactiver une confirmation légitime — un clic sans effet,
/// intermittent, sans rien dans le journal.
///
/// **Ce qui protège vraiment, c'est `ask`** : il ÉCRASE l'action précédente
/// (`PENDING = Some(action)`) avant de montrer la fenêtre, et il est le SEUL à
/// pouvoir la montrer. Une action périmée ne peut donc jamais être exécutée :
/// pour qu'un « oui » soit lu, il faut que `ask` ait posé l'action, et il l'a
/// posée pour CE qui est affiché.
pub fn poll() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

/// Vide les actions de la page et rend l'action CONFIRMÉE — `None` pour une
/// annulation, ou quand il n'y a rien à faire.
///
/// **`take()` sur un « oui »** : l'action est consommée, donc une seconde
/// réponse (un double-clic sur le bouton, deux messages arrivés dans la même
/// frame) ne la rejouerait pas.
pub fn dispatch_actions() -> Option<Confirmed> {
    let mut result = None;
    let mut answered = false;
    for action in cef_popup::drain_actions(&SLOT) {
        match action.as_str() {
            "confirm/yes" => {
                answered = true;
                result = PENDING.with(|p| p.borrow_mut().take());
            }
            // « Annuler » n'a rien à exécuter : oublier suffit.
            "confirm/no" => {
                answered = true;
                PENDING.with(|p| *p.borrow_mut() = None);
            }
            other => eprintln!("[neuradesk][cef] action de confirmation inconnue : {other:?}"),
        }
    }
    // **LA RÉPONSE REFERME LA FENÊTRE, quelle qu'elle soit.** DÉFAUT RÉEL
    // SIGNALÉ — « je ne peux pas annuler l'action de supprimer un séparateur
    // depuis le menu ni confirmer en cliquant ». Sans ce `hide`, « Annuler »
    // n'avait STRICTEMENT rien à faire (rien à exécuter, rien à fermer) et
    // « Confirmer » supprimait bien l'espace mais laissait la fenêtre ouverte :
    // les deux boutons se lisaient donc comme des clics morts, et la fenêtre
    // restait là jusqu'à ce qu'un clic ailleurs la chasse.
    //
    // Les autres popups ne se referment PAS sur une réponse, et c'est normal :
    // un curseur qu'on règle doit rester ouvert pour qu'on continue de le
    // régler. Une CONFIRMATION, elle, a fini son travail dès la réponse.
    if answered {
        cef_popup::hide_if_visible(&SLOT);
        // Trace PERMANENTE (voir `diag`, doc de module — inerte sans
        // `NEURADESK_DIAG=1`) : elle dit à la fois qu'une réponse est bien
        // arrivée de la page (`drain_actions`) et que la fenêtre s'est
        // effectivement refermée. Deux maillons distincts, et le second est
        // celui qui manquait.
        // « FERMETURE ENGAGÉE » et non plus « fenêtre refermée » : depuis le
        // fondu (voir `win32::fade`), la fenêtre reste VISIBLE le temps de
        // s'éteindre, donc `is_visible` répond encore vrai ici et la trace
        // aurait annoncé faux à chaque fois. Les deux maillons qu'elle doit
        // prouver restent les mêmes — la réponse est arrivée, et la fermeture
        // est déclenchée — mais le second se lit maintenant avec le prédicat
        // qui décrit vraiment une fermeture (voir `cef_popup::is_closing`).
        crate::diag!(
            "confirmation : {} ; fermeture engagee : {}",
            if result.is_some() { "confirmee" } else { "annulee" },
            cef_popup::is_closing(&SLOT) || !cef_popup::is_visible(&SLOT)
        );
    }
    result
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

/// Voir `win32::cef_popup::debug_click` — réservé au harnais de vérification.
pub fn debug_click(x: i32, y: i32) {
    cef_popup::debug_click(&SLOT, x, y);
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
