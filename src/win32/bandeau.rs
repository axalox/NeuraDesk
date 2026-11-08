//! **LE BANDEAU ET L'HISTORIQUE DES MESSAGES** — la moitié visible des
//! automatisations, et la trace qu'elles laissent dans NeuraDesk.
//!
//! DEMANDE EXPLICITE (2026-10-04) : « le bandeau de notification » (lot 2 b de
//! `win32::automatisations`), puis « fais en sorte de laisser une notification
//! dans le menu du clic droit de l'horloge ».
//!
//! # ⛔ UNE SEULE FENÊTRE POUR DEUX USAGES, ET C'EST DÉLIBÉRÉ
//!
//! Deux pages CEF auraient coûté **deux fois** tout le câblage d'un panneau :
//! les quatre listes d'`app::run` (dont le registre du harnais, qui échoue en
//! silence) et les onze dashs frères qui masquent leurs voisins. Une seule page,
//! deux `MODE`, un seul `PopupSpec` — la géométrie étant passée à chaque
//! ouverture (`toggle_with` prend le rectangle), les deux usages cohabitent sans
//! se connaître.
//!
//! # ⭐ LE BANDEAU CÈDE DEVANT L'HISTORIQUE
//!
//! Un message arrive pendant que la liste est ouverte : on ne REMPLACE pas la
//! fenêtre. Le message s'ajoute à l'historique, et la liste ouverte le montre
//! (elle est repoussée à chaque frame). C'est la seule règle qui évite qu'une
//! notification en cours de lecture fasse disparaître ce qu'on lisait.
//!
//! # Le bandeau s'efface tout seul
//!
//! ⚠️ `montrer` N'OUVRE RIEN : il enregistre le message et lève un drapeau.
//! C'est `poll`, appelé par `app::run`, qui ouvre la fenêtre — parce qu'il faut
//! une GÉOMÉTRIE pour l'ouvrir, et que `montrer` est appelé depuis
//! `automatisations::executer`, qui n'en a aucune (même séparation que
//! `pending_display_rebuild`).
//!
//! # Où vit l'historique
//!
//! ⚠️ **EN MÉMOIRE, POUR LA SESSION.** Il ne survit pas à un redémarrage. La
//! trace PERSISTANTE, elle, est celle du centre de notifications de Windows
//! (voir `win32::bulle`) — c'est lui qui garde, pas nous.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};

/// Combien de messages l'historique garde — les plus récents.
///
/// ⚠️ **PASSÉ DE 40 À 2000 LE 2026-10-09** (demande explicite). Ce n'est PAS
/// gratuit, et le coût ne vient pas du stockage : il vient des **deux chemins qui
/// repoussent l'état à la page**, qui reconstruisent leur empreinte de cache à
/// CHAQUE FRAME tant que le panneau est ouvert —
/// `cef_calendar::push_state` formate les `n` messages en une chaîne, et
/// `push_state` (ici) sérialise les `n` en JSON avant de comparer. À 60 Hz, c'est
/// `n` × 60 formatages par seconde.
///
/// ⇒ **Si un jour ce plafond remonte encore, c'est l'EMPREINTE qu'il faut rendre
/// incrémentale**, pas le plafond qu'il faut rabaisser : la liste ne change que
/// par ajout, retrait ou vidage, donc `(longueur, identifiant du dernier,
/// compteur de révision)` suffirait à la décrire en O(1) — au lieu de la
/// reconstruire en O(n) soixante fois par seconde.
const MAX_MESSAGES: usize = 2000;
/// Combien de temps le bandeau reste à l'écran avant de s'effacer.
const DUREE_BANDEAU: Duration = Duration::from_secs(6);

// ---------------------------------------------------------------------------
// OÙ LE BANDEAU SE POSE
// ---------------------------------------------------------------------------

/// **LES TROIS POSITIONS DU BANDEAU** — DEMANDE EXPLICITE (2026-10-09) : « avoir
/// le choix entre en haut à gauche, centré sous l'horloge, en haut à droite ».
///
/// ⚠️ **LE DÉFAUT EST `Droite`, ET C'EST L'ANCIEN COMPORTEMENT** : le bandeau a
/// toujours été en haut à droite, et qui n'a jamais touché à ce réglage ne doit
/// rien voir changer.
///
/// ⭐ **« SOUS L'HORLOGE » N'EST PAS « À DROITE ».** L'horloge est au **CENTRE**
/// de la barre du haut — `ui::topbar::clock_rect` pose son axe à
/// `(total_width - text_w) * 0.5`, et c'est la CLOCHE qui est à sa droite. La
/// position du milieu est donc le milieu de l'écran, pas le bord droit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Position {
    Gauche,
    Horloge,
    Droite,
}

impl Position {
    /// **LA LISTE, ET ELLE EST LE CONTRAT AVEC LA PAGE.** `settings.html` a ses
    /// trois boutons ; un test compare les deux listes — même discipline que les
    /// verbes d'une page, parce qu'une liste écrite deux fois se vérifie.
    pub const TOUTES: [(&'static str, Position); 3] = [
        ("gauche", Position::Gauche),
        ("horloge", Position::Horloge),
        ("droite", Position::Droite),
    ];

    pub fn nom(self) -> &'static str {
        match self {
            Position::Gauche => "gauche",
            Position::Horloge => "horloge",
            Position::Droite => "droite",
        }
    }

    pub fn depuis_nom(nom: &str) -> Option<Self> {
        Position::TOUTES.iter().find(|(connu, _)| connu.eq_ignore_ascii_case(nom)).map(|(_, p)| *p)
    }
}

/// **LE DÉFAUT** — l'ancien comportement, en constante NOMMÉE plutôt qu'en
/// littéral perdu dans `app::run`.
pub const POSITION_DEFAUT: Position = Position::Droite;

/// La position retenue. **L'absence de réglage rend `POSITION_DEFAUT`.**
pub fn position() -> Position {
    crate::config::load_position_bandeau()
        .and_then(|nom| Position::depuis_nom(&nom))
        .unwrap_or(POSITION_DEFAUT)
}

/// Retient `nom` — `false` pour un nom inconnu, et **rien n'est écrit** dans ce
/// cas : un fichier portant un nom que le code ne connaît pas serait un réglage
/// muet, qui ne se verrait que le jour où quelqu'un l'ouvrirait.
pub fn set_position(nom: &str) -> bool {
    let Some(position) = Position::depuis_nom(nom) else { return false };
    // ⚠️ **ON ÉCRIT LE NOM CANONIQUE**, jamais celui reçu : `Horloge` et
    // `horloge` doivent laisser le MÊME fichier.
    crate::config::save_position_bandeau(position.nom());
    true
}

/// Ce que la page montre — le seul état qui distingue les deux usages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Bandeau,
    Historique,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// ⭐ **L'IDENTITÉ DE LA LIGNE, ET ELLE EST INDISPENSABLE.** Sans elle, la
    /// seule façon de désigner un message depuis une page serait son RANG dans la
    /// liste — or l'historique change sous les doigts : un message qui arrive
    /// entre l'affichage et le clic décale tous les rangs suivants, et le clic
    /// effacerait alors **le mauvais message**, en silence. Le triplet
    /// `(titre, texte, quand)` ne sauve rien : deux messages IDENTIQUES sont
    /// légitimes et voulus (voir la clé de cache de `cef_calendar::push_state`,
    /// « deux messages identiques restent deux messages »).
    ///
    /// Monotone, jamais réutilisé, et **par session** : l'historique lui-même ne
    /// survit pas à un redémarrage (voir la doc de module).
    pub id: u64,
    pub titre: String,
    pub texte: String,
    /// `HH:MM` au moment où il est arrivé.
    pub quand: String,
}

thread_local! {
    static SLOT: PopupSlot = PopupSlot::new();
    static MESSAGES: RefCell<VecDeque<Message>> = const { RefCell::new(VecDeque::new()) };
    /// **LE DISTRIBUTEUR D'IDENTIFIANTS** (voir `Message::id`). Il ne redescend
    /// jamais et n'est jamais remis à zéro : un identifiant recyclé désignerait
    /// deux lignes, et une suppression viserait celle qu'on ne regarde pas.
    static PROCHAIN_ID: Cell<u64> = const { Cell::new(0) };
    static MODE: Cell<Mode> = const { Cell::new(Mode::Bandeau) };
    /// L'échéance du bandeau — `None` quand il n'y a rien à effacer.
    static ECHEANCE: Cell<Option<Instant>> = const { Cell::new(None) };
    /// Un message attend d'être montré, faute de géométrie (voir `montrer`).
    static EN_ATTENTE: Cell<bool> = const { Cell::new(false) };
    /// **LE RECTANGLE DU MODE COURANT.** ⛔ Les deux usages n'ont PAS la même
    /// géométrie — le bandeau est une carte en haut à droite, l'historique une
    /// liste ancrée à l'horloge. Comme `render_if_visible` tourne à chaque frame
    /// et redimensionne, il lui FAUT le rectangle du mode affiché : lui passer
    /// celui du bandeau en mode historique remettrait la liste à la taille de la
    /// carte soixante fois par seconde.
    static RECT: Cell<[i32; 4]> = const { Cell::new([0, 0, 0, 0]) };
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefBandeauWindow"),
    window_title: w!("NeuraDesk — Notifications"),
    html_file: "bandeau.html",
    wndproc: Some(wndproc),
    // Un MENU ancré, comme le calendrier du clic sur l'horloge.
    above_a_magnified_dock: true,
    fades: true,
    fenetre: false,
};

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

fn masquer_les_voisins() {
    crate::win32::cef_dash::hide_if_visible();
    crate::win32::cef_settings::hide_if_visible();
    crate::win32::cef_notes::hide_if_visible();
    crate::win32::cef_image_editor::hide_if_visible();
    crate::win32::cef_calendar_applet::hide_if_visible();
    crate::win32::cef_clipboard::hide_if_visible();
    crate::win32::cef_surveillance::hide_if_visible();
    crate::win32::cef_scanner::hide_if_visible();
    crate::win32::cef_shaders::hide_if_visible();
    crate::win32::cef_lueurs::hide_if_visible();
    crate::win32::cef_textures::hide_if_visible();
    crate::win32::cef_moniteur::hide_if_visible();
    crate::win32::cef_ssh::hide_if_visible();
    crate::win32::cef_terminal::hide_if_visible();
    crate::win32::cef_minuteur::hide_if_visible();
    crate::win32::cef_calculatrice::hide_if_visible();
    crate::win32::cef_recherche::hide_if_visible();
    crate::win32::cef_automatisations::hide_if_visible();
    crate::win32::cef_meteo::hide_if_visible();
    crate::win32::cef_aide::hide_if_visible();
}

/// **UN MESSAGE ARRIVE — il est enregistré, et il sera montré.**
///
/// ⚠️ **RIEN N'EST OUVERT ICI** : voir la doc de module. `app::run` ouvre par
/// `poll`, qui a la géométrie.
pub fn montrer(titre: &str, texte: &str) {
    enregistrer(titre, texte);
    // L'historique OUVERT se met à jour tout seul (il est repoussé à chaque
    // frame) : on ne lui vole pas sa fenêtre.
    if MODE.with(Cell::get) == Mode::Historique && cef_popup::is_visible(&SLOT) {
        return;
    }
    EN_ATTENTE.with(|a| a.set(true));
}

/// **UN MESSAGE À GARDER DANS L'HISTORIQUE, SANS BANDEAU** — pour les toasts de
/// Windows, que le shell affiche déjà lui-même (déplacé sous la barre par
/// `win32::toast_natif`) : les montrer aussi ici les ferait voir EN DOUBLE.
pub fn retenir(titre: &str, texte: &str) {
    enregistrer(titre, texte);
}

/// **LE SEUL CONSTRUCTEUR DE `Message`** : il attribue l'identité, applique le
/// plafond, et rend l'identifiant. `montrer` et `debug_remplir` passent tous deux
/// par ici — c'est ce qui garantit qu'aucun message ne naisse sans identifiant.
fn enregistrer(titre: &str, texte: &str) -> u64 {
    let id = PROCHAIN_ID.with(|c| {
        let suivant = c.get() + 1;
        c.set(suivant);
        suivant
    });
    let message = Message {
        id,
        titre: if titre.trim().is_empty() { "NeuraDesk".to_string() } else { titre.trim().to_string() },
        texte: texte.to_string(),
        quand: heure_locale(),
    };
    MESSAGES.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() >= MAX_MESSAGES {
            m.pop_front();
        }
        m.push_back(message);
    });
    id
}

/// **RETIRE UN SEUL MESSAGE, CELUI D'IDENTIFIANT `id`** — et rend `true` s'il a
/// retiré quelque chose.
///
/// ⚠️ **Un identifiant inconnu ne retire RIEN et ne dit rien de plus** que son
/// `false`. C'est le cas normal d'un double clic sur la même croix, ou d'une page
/// qui rejoue une suppression déjà faite : la file d'actions d'un popup peut
/// contenir deux fois la même demande, et effacer « la ligne au même rang » dans
/// ce cas effacerait sa voisine.
///
/// L'état est repoussé IMMÉDIATEMENT : la liste ouverte doit se voir dégarnir
/// dans la frame, pas à la prochaine frappe.
pub fn supprimer(id: u64) -> bool {
    let retire = MESSAGES.with(|m| {
        let mut m = m.borrow_mut();
        match m.iter().position(|x| x.id == id) {
            Some(rang) => {
                m.remove(rang);
                true
            }
            None => false,
        }
    });
    if retire {
        PAYLOAD.with(|p| *p.borrow_mut() = None);
        push_state();
    }
    retire
}

/// **REMPLIT L'HISTORIQUE DE MESSAGES DE TEST** — le levier
/// `NEURADESK_DEBUG_NOTIFS=<n>` (voir `app::run`).
///
/// ⚠️ **IL N'ARME PAS LE BANDEAU**, et c'est toute la différence avec `montrer` :
/// une carte qui se pose par-dessus la liste ouverte empêcherait justement de
/// photographier ce qu'on veut vérifier. C'est un instrument de mesure, pas une
/// notification.
///
/// ⇒ **Pour VOIR des notifications arriver, c'est `debug_animer`** — celle-ci ne
/// montre rien.
pub fn debug_remplir(n: usize) {
    for i in 1..=n {
        enregistrer(&format!("Application {i}"), &format!("Message de vérification numéro {i}, assez long pour peser une ligne ou deux."));
    }
    PAYLOAD.with(|p| *p.borrow_mut() = None);
}

/// Les textes du levier animé — des émetteurs PLAUSIBLES, pour que la
/// démonstration ressemble à ce que l'utilisateur verra vraiment.
const ANIM_EXEMPLES: [(&str, &str); 5] = [
    // Un message LONG d'abord : c'est lui que la première capture montre, et il
    // doit remplir les cinq lignes de la carte puis finir sur des points de
    // suspension (voir `BANDEAU_HAUTEUR_UNITES`).
    ("Thunderbird", "Camille : Bonjour, je te renvoie le compte rendu de la réunion de ce matin avec les trois points à valider avant vendredi — le budget du trimestre, le calendrier des livraisons et la liste des personnes à prévenir. Dis-moi si quelque chose manque, je complète avant de l'envoyer à toute l'équipe."),
    ("Discord", "Nouveau message de Camille"),
    ("Telegram", "Le groupe « Bureau » a trois messages non lus"),
    ("Mise à jour", "Un redémarrage est nécessaire pour terminer l'installation"),
    ("Sauvegarde", "La sauvegarde de cette nuit s'est terminée sans erreur"),
];

/// Cadence du levier animé, **en frames** (≈ 4 s à 60 Hz).
///
/// ⚠️ **CHOISIE PLUS COURTE QUE `DUREE_BANDEAU` (6 s) À DESSEIN** : la carte reste
/// donc affichée sans interruption pendant que les messages s'enchaînent, ce qui
/// exerce au passage le chemin « UN BANDEAU DÉJÀ VISIBLE SE REMPLACE, IL NE SE
/// REFERME PAS » (voir `poll`) — un défaut réel, corrigé, qu'il est utile de
/// pouvoir regarder.
const CADENCE_ANIM_FRAMES: u32 = 240;

thread_local! {
    /// L'état du levier animé : `(restant, délai, prochain exemple)`.
    static ANIMATION: Cell<(u32, u32, usize)> = const { Cell::new((0, 0, 0)) };
}

/// **LEVIER — `NEURADESK_DEBUG_NOTIFS_ANIMEES=<n>` : `n` notifications ARRIVENT**,
/// une toutes les `CADENCE_ANIM_FRAMES`, par le **vrai chemin** (`montrer`) : le
/// bandeau s'affiche, puis s'efface, et l'historique grandit.
///
/// ⭐ **C'EST LA DIFFÉRENCE AVEC `debug_remplir`, ET ELLE EST TOUTE LA RAISON DE
/// CETTE FONCTION** : celle-là ne montre RIEN — elle remplit la liste en silence,
/// ce qui est exactement ce qu'il faut pour photographier la liste, et exactement
/// ce qu'il ne faut pas pour vérifier qu'une notification s'AFFICHE. Demande
/// explicite de l'utilisateur, après avoir constaté le silence : « je n'ai pas vu
/// la fenêtre de l'app ».
///
/// ⚠️ **LA CADENCE VIT DANS `poll`**, la seule fonction appelée à chaque frame —
/// et le fil de l'interface est le seul où `montrer` ait le droit de tourner
/// (`MESSAGES` est un `thread_local` : un fil à part ne le verrait pas).
pub fn debug_animer(n: usize) {
    ANIMATION.with(|a| a.set((n as u32, 0, 0)));
}

/// **LE TIC DU LEVIER ANIMÉ** — appelé par `poll`, une fois par frame.
/// Sans effet tant que `debug_animer` n'a rien posé.
fn tic_animation() {
    let (restant, delai, prochain) = ANIMATION.with(Cell::get);
    if restant == 0 {
        return;
    }
    if delai > 0 {
        ANIMATION.with(|a| a.set((restant, delai - 1, prochain)));
        return;
    }
    let (titre, texte) = ANIM_EXEMPLES[prochain % ANIM_EXEMPLES.len()];
    // Le MÊME appel qu'une automatisation : c'est ce qui rend la démonstration
    // honnête — ce qui s'affiche est ce que l'utilisateur verra pour de vrai.
    montrer(titre, texte);
    ANIMATION.with(|a| a.set((restant - 1, CADENCE_ANIM_FRAMES, prochain + 1)));
}

fn heure_locale() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:02}:{:02}", t.wHour, t.wMinute)
}

/// **OUVRE OU FERME L'HISTORIQUE** — le geste du clic droit sur l'horloge.
pub fn basculer_l_historique(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    if MODE.with(Cell::get) == Mode::Historique && cef_popup::is_visible(&SLOT) {
        hide_if_visible();
        return;
    }
    ouvrir(Mode::Historique, rect, gfx, content_scale);
}

fn ouvrir(mode: Mode, rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    MODE.with(|m| m.set(mode));
    RECT.with(|r| r.set(rect));
    PAYLOAD.with(|p| *p.borrow_mut() = None);
    // ⛔ **LA CARTE NE PREND PAS LE PREMIER PLAN ; L'HISTORIQUE, SI.**
    //
    // DÉFAUT SIGNALÉ (2026-10-09) : « la fenêtre des notifications prend le focus
    // et à sa disparition le programme au premier plan n'est pas toujours celui qui
    // était au premier plan ». Les deux moitiés n'en font qu'une : la carte volait
    // le premier plan à l'ouverture, et **Windows ne le rend jamais quand une
    // fenêtre au premier plan se cache** — la disparition ne peut donc rien
    // restaurer, et le premier plan retombe sur ce que Windows trouve ensuite.
    //
    // ⭐ **LA SEULE CORRECTION QUI RÈGLE LES DEUX : NE PAS LE PRENDRE.** Une
    // notification qui vole le clavier pendant qu'on écrit est inacceptable, et le
    // bandeau remplace les toasts de Windows — qui, eux, ne le prennent jamais.
    //
    // ⚠️ **LA DÉCISION EST ICI PARCE QU'ELLE DÉPEND DU MODE**, et que le `SPEC` est
    // le même pour les deux usages : la liste du clic droit est un VRAI menu, elle
    // attend Échap et des clics. Voir `cef_popup::set_ouverture_sans_focus`, qui
    // porte le mécanisme.
    cef_popup::set_ouverture_sans_focus(sans_focus_pour(mode));
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, masquer_les_voisins);
}

/// **QUEL MODE PREND LE PREMIER PLAN, ET POURQUOI.** Pure, donc vérifiable — voir
/// `ouvrir`, qui porte le défaut signalé et la démonstration complète.
///
/// ⭐ La CARTE ne le prend pas (c'est une notification), l'HISTORIQUE le prend
/// (c'est un menu, il attend Échap et des clics).
fn sans_focus_pour(mode: Mode) -> bool {
    mode == Mode::Bandeau
}

/// **CE QUE `app::run` APPELLE À CHAQUE FRAME** : ouvre le bandeau en attente, et
/// l'efface quand son temps est écoulé.
///
/// Le bandeau se ferme ici et nulle part ailleurs — c'est ce qui garantit qu'il
/// ne reste pas à l'écran si la frame suivante ne vient jamais… et comme la
/// boucle tourne à 60 Hz, « jamais » n'arrive pas.
pub fn poll(rect_bandeau: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    // ⭐ **LE TIC DU LEVIER ANIMÉ** (`NEURADESK_DEBUG_NOTIFS_ANIMEES`), et il est
    // AVANT tout le reste À DESSEIN : un message qu'il vient d'émettre doit
    // s'ouvrir dans CETTE frame, pas à la suivante — sinon la cadence réelle
    // serait décalée d'une frame, et la démonstration serait un instant en retard
    // sur elle-même. Sans effet tant que le levier n'a rien posé.
    tic_animation();
    if EN_ATTENTE.with(|a| a.replace(false)) {
        // ⛔ **UN BANDEAU DÉJÀ À L'ÉCRAN SE REMPLACE, IL NE SE REFERME PAS.**
        // `toggle_with` BASCULE : l'appeler sur une fenêtre visible la cacherait,
        // et un second message ferait donc DISPARAÎTRE le premier. `montrer` a
        // déjà écarté le cas de l'historique ouvert (il n'arme pas l'attente) —
        // c'est la moitié visible qu'il faut traiter ici.
        if MODE.with(Cell::get) == Mode::Bandeau && cef_popup::is_visible(&SLOT) {
            PAYLOAD.with(|p| *p.borrow_mut() = None);
            push_state();
        } else {
            ouvrir(Mode::Bandeau, rect_bandeau, gfx, content_scale);
        }
        // ⚠️ **L'ÉCHÉANCE EST REPOUSSÉE** : le bandeau montre le message LE PLUS
        // RÉCENT, et lui donner le temps restant du précédent le ferait
        // disparaître presque aussitôt.
        ECHEANCE.with(|e| e.set(Some(Instant::now() + DUREE_BANDEAU)));
        return;
    }
    if MODE.with(Cell::get) != Mode::Bandeau || !cef_popup::is_visible(&SLOT) {
        return;
    }
    if ECHEANCE.with(Cell::get).is_some_and(|e| Instant::now() >= e) {
        ECHEANCE.with(|e| e.set(None));
        hide_if_visible();
    }
}

pub fn refresh_if_visible() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

/// **LE ZOOM DE LA PAGE, DANS LES DEUX MODES.**
///
/// ⭐ **LE FACTEUR « NOTIFICATIONS », PARTOUT** — DEMANDE DU 2026-10-28 : « le texte
/// des notifications ne semble pas prendre en compte le réglage de taille du texte ».
/// La liste ouverte au clic droit sur l'horloge s'appelle « Notifications » et son
/// zoom portait `TEXTES` : le curseur « Notifications » ne la concernait jamais, et
/// deux captures à 100 % et à 80 % en sortaient IDENTIQUES au pixel près. La carte,
/// elle, obéissait déjà. Les deux prennent maintenant le même facteur, comme leur
/// rectangle (`app::taille_bandeau_notifiees`).
///
/// ⚠️ **« TAILLE DES TEXTES » N'EST PAS OUBLIÉ POUR AUTANT** : il entre déjà par le
/// rectangle, via `app::content_scale_pour` → `cef_popup::menu_zoom_factor(echelle)`.
/// Le remettre ici le multiplierait deux fois — une fois sur la fenêtre, une fois sur
/// la page.
fn echelle_page() -> f32 {
    crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::NOTIFICATIONS)
}

fn push_state() {
    // Le zoom de la page suit celui de sa fenêtre : la fenêtre grandit du facteur
    // « Notifications » (`app::taille_bandeau_notifiees`), la page zoome d'autant.
    let historique = MODE.with(Cell::get) == Mode::Historique;
    let echelle = echelle_page();
    let payload = json!({
        "mode": if historique { "historique" } else { "bandeau" },
        "contentScale": echelle,
        "messages": MESSAGES.with(|m| m.borrow().iter().map(|x| json!({
            // ⚠️ **L'IDENTIFIANT PART AVEC LE RESTE** : c'est lui que la croix de
            // la page renvoie. Sans lui, la page n'aurait que le rang de la ligne
            // qu'elle affiche — et un rang n'est pas une identité (voir
            // `Message::id`).
            "id": x.id, "titre": x.titre, "texte": x.texte, "quand": x.quand,
        })).collect::<Vec<_>>()),
    })
    .to_string();
    if PAYLOAD.with(|p| p.borrow().as_deref() == Some(payload.as_str())) {
        return;
    }
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
    PAYLOAD.with(|p| *p.borrow_mut() = Some(payload));
}

/// **TRAITE UNE ACTION, LA NÔTRE OU NON — et dit si elle était la nôtre.**
///
/// Rend `true` quand l'action portait le préfixe `bandeau/` : elle est alors
/// traitée, ou journalisée si son verbe est inconnu. Rend `false` quand elle
/// appartenait à quelqu'un d'autre — le seul cas que l'appelant doit signaler.
///
/// ⭐ **ELLE EST PUBLIQUE PARCE QUE LA PAGE DU CALENDRIER ÉMET AUSSI DES ACTIONS
/// `bandeau/…`.** Les files d'actions sont **PAR POPUP**
/// (`cef_popup::drain_actions` draine le `SLOT` d'une seule fenêtre) : une action
/// émise par `calendar.html` est donc vue par `cef_calendar::drain_actions` et
/// par personne d'autre. Or le verbe nomme le module qui **possède** la donnée,
/// pas celui qui l'émet — d'où ce renvoi explicite, plutôt qu'un second verbe
/// dans un second vocabulaire.
pub fn dispatch_action(action: &str) -> bool {
    let Some(reste) = action.strip_prefix("bandeau/") else {
        return false;
    };
    // ⚠️ **CHAQUE BRAS DÉCODE SON SEGMENT** : les arguments sont encodés par la
    // page (`encodeURIComponent`). L'identifiant est numérique aujourd'hui, il ne
    // le restera pas forcément.
    let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
    match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["ready"] => PAYLOAD.with(|p| *p.borrow_mut() = None),
        ["fermer"] => hide_if_visible(),
        ["vider"] => {
            MESSAGES.with(|m| m.borrow_mut().clear());
            PAYLOAD.with(|p| *p.borrow_mut() = None);
            push_state();
        }
        // La croix d'une ligne — voir `supprimer`. Un identifiant illisible est
        // REFUSÉ en le disant : c'est une page qui a divergé, pas un geste.
        //
        // ⚠️ **LA LIGNE DE JOURNAL EST LA MESURE**, et pas un bavardage : la
        // suppression est le seul geste de ce panneau dont l'effet ne se lit nulle
        // part ailleurs — ni sur le disque (l'historique vit en mémoire), ni dans
        // un fichier de configuration. `diag!` ne coûte rien quand le journal est
        // éteint, et c'est ce qui permet de vérifier par capture qu'un clic a bien
        // atteint le module (`NEURADESK_DEBUG_SUPPRIMER`, voir plus bas).
        ["supprimer", id] => match id.parse::<u64>() {
            Ok(id) => {
                let retire = supprimer(id);
                crate::diag!("[bandeau] message {id} : retire={retire}, {} restant(s)", combien());
            }
            Err(_) => crate::diag!("[bandeau] identifiant illisible : {id:?}"),
        },
        _ => crate::diag!("[bandeau] action inconnue : {action:?}"),
    }
    true
}

pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        if !dispatch_action(&action) {
            crate::diag!("[bandeau] action inconnue : {action:?}");
        }
    }
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

pub fn hide_if_visible() {
    ECHEANCE.with(|e| e.set(None));
    cef_popup::hide_if_visible(&SLOT);
}

/// ⚠️ **LE RECTANGLE N'EST PAS UN PARAMÈTRE** : c'est celui du MODE affiché, que
/// le module retient (voir `RECT`). L'appelant n'a qu'un seul rectangle à
/// connaître — celui du bandeau — et c'est `poll` qui le lui donne.
pub fn render_if_visible(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::resize_if_needed(&SLOT, RECT.with(Cell::get));
    cef_popup::render_if_visible(&SLOT, tint_rgba, 0.0, cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

/// Nombre de messages gardés — pour les tests, qui n'ont pas d'écran.
pub fn combien() -> usize {
    MESSAGES.with(|m| m.borrow().len())
}

/// **LES MESSAGES, DU PLUS ANCIEN AU PLUS RÉCENT** (l'ordre de `MESSAGES`).
///
/// Existe pour les panneaux qui veulent les MONTRER ailleurs que dans
/// l'historique — aujourd'hui le mini-calendrier du clic gauche sur l'horloge
/// (`win32::cef_calendar`, qui s'en sert pour sa section « Notifications »).
/// C'est ce que l'utilisateur demandait : la trace des notifications dans le
/// panneau du clic GAUCHE, pas seulement derrière le clic droit.
///
/// ⚠️ **Rend une COPIE, jamais un emprunt** : l'appelant construit sa page avec,
/// et un `RefCell` ne doit pas rester emprunté pendant ce temps — même règle
/// que partout ailleurs dans ce dépôt (voir `push_state`, qui la commente).
pub fn messages() -> Vec<Message> {
    MESSAGES.with(|m| m.borrow().iter().cloned().collect())
}

pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// **LEVIER DE VÉRIFICATION** — `NEURADESK_DEBUG_SUPPRIMER=<rang>` : actionne la
/// croix de la ligne `rang` de la liste **affichée**, en appelant la fonction de
/// la page (`go`, sur `window.__ndDerniersIds`, que la page tient à jour).
///
/// ⛔ **C'est un appel de FONCTION, jamais une entrée simulée** — même règle que
/// `cef_calendar_applet::debug_editer`. Le clic aveugle du harnais
/// (`NEURADESK_DEBUG_CLICK`) est délibérément **inutilisable ici** : il vise des
/// coordonnées, et une coordonnée qui tombe sur une croix supprimerait une ligne
/// que personne n'a désignée.
///
/// ⛔ **ET L'APPELANT N'EN FAIT QU'UN** (voir `app::run`, frame fixe) : la
/// suppression n'est pas idempotente, la rejouer à chaque frame viderait la liste
/// en une seconde.
///
/// Rend `false` quand la fenêtre n'est pas ouverte — l'appel n'a alors aucun sens
/// et ne doit pas être compté comme une preuve.
pub fn debug_supprimer(rang: usize) -> bool {
    if !cef_popup::is_visible(&SLOT) {
        return false;
    }
    cef_popup::execute_js(&SLOT, &format!("go('supprimer', window.__ndDerniersIds[{rang}])"));
    // ⚠️ **CE QUE CETTE LIGNE DIT, ET RIEN DE PLUS** : l'appel a été ENVOYÉ à la
    // page. Elle ne prouve pas que la page l'a exécuté (`execute_js` est
    // silencieux, et un `__ndDerniersIds` encore absent ferait lever le JS sans
    // que rien ne le dise). La mesure de l'effet est AILLEURS : c'est la ligne
    // « message <id> : retire=… » du bras `supprimer`, qui ne s'écrit que si
    // l'action est réellement revenue.
    crate::diag!("[bandeau] NEURADESK_DEBUG_SUPPRIMER={rang} — appel de go() envoye");
    true
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(result) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐ **LE BANDEAU CÈDE DEVANT L'HISTORIQUE.** Un message qui arrive pendant
    /// qu'on lit la liste ne doit pas faire disparaître ce qu'on lisait.
    ///
    /// ⚠️ Ce test ne peut PAS ouvrir de fenêtre (pas d'écran dans `cargo test`) :
    /// il vérifie la seule moitié qui décide — le message entre dans
    /// l'historique, et l'attente n'est PAS armée quand la liste est ouverte.
    #[test]
    fn un_message_pendant_l_historique_n_arme_pas_le_bandeau() {
        MODE.with(|m| m.set(Mode::Historique));
        EN_ATTENTE.with(|a| a.set(false));
        // `is_visible` est faux sans fenêtre : la garde de `montrer` retombe donc
        // sur le chemin normal. On vérifie ce qui est vérifiable ici — l'entrée.
        let avant = combien();
        montrer("Titre", "Message");
        assert_eq!(combien(), avant + 1, "le message est ENREGISTRÉ dans tous les cas");
        EN_ATTENTE.with(|a| a.set(false));
        MODE.with(|m| m.set(Mode::Bandeau));
    }

    /// ⭐ **LES DEUX VUES SUIVENT LE FACTEUR « NOTIFICATIONS », PAS CELUI DES
    /// TEXTES.** FALSIFICATION : avant le correctif du 2026-10-28, la branche
    /// `Historique` renvoyait `TEXTES` — ce test rougissait de ce seul fait, alors
    /// que la carte, elle, passait. Le 1,5 de `TEXTES` est là pour que l'ancien
    /// code rende 1,5 et non 0,8 : un test qui passe aussi sur l'ancien code ne
    /// prouve rien.
    #[test]
    fn les_deux_vues_suivent_la_taille_des_notifications() {
        crate::win32::taille_panneaux::regler_global(crate::win32::taille_panneaux::NOTIFICATIONS, 0.8);
        crate::win32::taille_panneaux::regler_global(crate::win32::taille_panneaux::TEXTES, 1.5);
        MODE.with(|m| m.set(Mode::Bandeau));
        assert_eq!(echelle_page(), 0.8, "la carte prend la taille des notifications");
        MODE.with(|m| m.set(Mode::Historique));
        assert_eq!(echelle_page(), 0.8, "la liste s'appelle « Notifications » — elle prend le même facteur");
        MODE.with(|m| m.set(Mode::Bandeau));
        crate::win32::taille_panneaux::regler_global(crate::win32::taille_panneaux::NOTIFICATIONS, 1.0);
        crate::win32::taille_panneaux::regler_global(crate::win32::taille_panneaux::TEXTES, 1.0);
    }

    #[test]
    fn l_historique_est_borne() {
        MESSAGES.with(|m| m.borrow_mut().clear());
        for i in 0..(MAX_MESSAGES + 5) {
            montrer("t", &format!("message {i}"));
        }
        assert_eq!(combien(), MAX_MESSAGES);
        let dernier = MESSAGES.with(|m| m.borrow().back().cloned().unwrap());
        assert_eq!(dernier.texte, format!("message {}", MAX_MESSAGES + 4));
        EN_ATTENTE.with(|a| a.set(false));
        MESSAGES.with(|m| m.borrow_mut().clear());
    }

    /// Un titre vide prend le nom de l'application : une ligne d'historique sans
    /// titre serait un trou dans la liste.
    #[test]
    fn un_titre_vide_devient_le_nom_de_l_application() {
        MESSAGES.with(|m| m.borrow_mut().clear());
        montrer("   ", "texte");
        let message = MESSAGES.with(|m| m.borrow().back().cloned().unwrap());
        assert_eq!(message.titre, "NeuraDesk");
        EN_ATTENTE.with(|a| a.set(false));
        MESSAGES.with(|m| m.borrow_mut().clear());
    }

    // -----------------------------------------------------------------------
    // LA SUPPRESSION D'UNE LIGNE (2026-10-09)
    // -----------------------------------------------------------------------

    /// ⭐ **LA CARTE NE PREND PAS LE PREMIER PLAN, L'HISTORIQUE SI.**
    ///
    /// C'est la décision qui a corrigé le défaut signalé le 2026-10-09 — et elle
    /// se vérifie ici parce qu'elle ne dépend d'aucune fenêtre : le `SPEC` est
    /// commun aux deux usages, c'est le MODE qui tranche.
    #[test]
    fn la_carte_ne_prend_pas_le_premier_plan_et_l_historique_si() {
        assert!(sans_focus_pour(Mode::Bandeau), "une notification ne vole pas le clavier");
        assert!(!sans_focus_pour(Mode::Historique), "la liste est un menu : elle prend le focus");
    }

    /// ⚠️ **LES TESTS PARTAGENT L'ÉTAT DU FIL** (`MESSAGES` est un
    /// `thread_local`) : chacun repart d'un historique vide et d'un bandeau
    /// désarmé.
    fn historique_vide() {
        MESSAGES.with(|m| m.borrow_mut().clear());
        EN_ATTENTE.with(|a| a.set(false));
        MODE.with(|m| m.set(Mode::Bandeau));
        PAYLOAD.with(|p| *p.borrow_mut() = None);
    }

    fn identifiants() -> Vec<u64> {
        MESSAGES.with(|m| m.borrow().iter().map(|x| x.id).collect())
    }

    /// ⭐ **LA CROIX D'UNE LIGNE RETIRE CETTE LIGNE, ET RIEN D'AUTRE.** Le rang du
    /// message est DÉLIBÉRÉMENT au milieu : supprimer le premier ou le dernier
    /// passerait aussi avec un code qui vide la file ou qui retire par position.
    #[test]
    fn supprimer_retire_le_message_vise_et_preserve_l_ordre() {
        historique_vide();
        for i in 1..=4 {
            montrer("t", &format!("message {i}"));
        }
        let ids = identifiants();
        assert!(supprimer(ids[1]), "le deuxieme message doit etre retire");
        assert_eq!(identifiants(), vec![ids[0], ids[2], ids[3]]);
        let textes: Vec<String> = MESSAGES.with(|m| m.borrow().iter().map(|x| x.texte.clone()).collect());
        assert_eq!(textes, vec!["message 1", "message 3", "message 4"]);
        historique_vide();
    }

    /// Un identifiant inconnu ne retire RIEN — c'est le cas d'un second clic sur
    /// la même croix, la file d'actions pouvant porter deux fois la demande.
    #[test]
    fn un_identifiant_inconnu_ne_retire_rien() {
        historique_vide();
        montrer("t", "un");
        let avant = identifiants();
        assert!(!supprimer(u64::MAX), "un identifiant inconnu ne doit rien retirer");
        assert_eq!(identifiants(), avant);
        // Et un identifiant DÉJÀ supprimé ne retire pas sa voisine.
        assert!(supprimer(avant[0]));
        assert!(!supprimer(avant[0]));
        assert_eq!(combien(), 0);
        historique_vide();
    }

    /// ⭐⭐ **LA RAISON D'ÊTRE DE L'IDENTIFIANT.** Deux messages identiques — même
    /// titre, même texte, même minute — sont légitimes ; un rang ne saurait pas
    /// les distinguer, et supprimer « le premier » effacerait l'un pour l'autre.
    #[test]
    fn deux_messages_identiques_se_suppriment_separement() {
        historique_vide();
        montrer("Discord", "nouveau message");
        montrer("Discord", "nouveau message");
        let ids = identifiants();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1], "deux messages identiques ont deux identites");
        assert!(supprimer(ids[1]), "le SECOND doit partir, le premier rester");
        assert_eq!(identifiants(), vec![ids[0]]);
        historique_vide();
    }

    /// L'identité ne se recycle pas : quand le plafond fait tomber le plus ancien,
    /// le suivant prend un identifiant NEUF. Un identifiant rendu à nouveau
    /// désignerait deux lignes, et la croix en effacerait une autre.
    #[test]
    fn l_identifiant_ne_recule_jamais_meme_au_plafond() {
        historique_vide();
        for i in 0..(MAX_MESSAGES + 3) {
            montrer("t", &format!("message {i}"));
        }
        let ids = identifiants();
        assert_eq!(ids.len(), MAX_MESSAGES);
        assert!(ids.windows(2).all(|p| p[0] < p[1]), "les identifiants doivent croitre");
        assert_eq!(*ids.last().unwrap(), (MAX_MESSAGES + 3) as u64);
        historique_vide();
    }

    /// **LE VERBE DE LA CROIX PASSE PAR LE DISPATCH**, tel que la page l'émet —
    /// préfixe compris, et avec un segment encodé comme `encodeURIComponent` le
    /// produirait.
    #[test]
    fn le_dispatch_supprime_par_identifiant() {
        historique_vide();
        montrer("t", "un");
        montrer("t", "deux");
        let ids = identifiants();
        assert!(dispatch_action(&format!("bandeau/supprimer/{}", ids[0])));
        assert_eq!(identifiants(), vec![ids[1]]);
        // Une action d'un AUTRE module n'est pas consommée : c'est ce que
        // `cef_calendar::drain_actions` doit savoir pour la signaler.
        assert!(!dispatch_action("calendar/autre-chose"));
        // Un identifiant illisible est refusé sans toucher à la liste.
        assert!(dispatch_action("bandeau/supprimer/beaucoup"));
        assert_eq!(identifiants(), vec![ids[1]]);
        historique_vide();
    }

    /// ⭐ **LE SEUL TEST QUI GARDE CETTE PAGE.** Les verbes écrits dans
    /// `bandeau.html` et ceux acceptés par `dispatch_action` doivent être les
    /// MÊMES — une croix qui émet un verbe que Rust ne connaît pas ne produit
    /// aucune erreur, seulement une action journalisée et un clic sans effet.
    ///
    /// ⚠️ La source est coupée AVANT son propre module de tests : sans ça le test
    /// se lirait lui-même (il cite les verbes pour expliquer), et passerait en ne
    /// gardant rien — le piège est déjà payé sur ce dépôt.
    #[test]
    fn la_page_et_le_dispatch_acceptent_les_memes_verbes() {
        const PAGE: &str = include_str!("../../assets/popups/bandeau.html");
        let mut page: Vec<String> = PAGE
            .match_indices("go(")
            .filter_map(|(i, _)| {
                let reste = &PAGE[i + 3..];
                let guillemet = reste.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                reste[1..].split(guillemet).next().map(str::to_string)
            })
            .collect();
        let source = include_str!("bandeau.rs");
        let mut rust: Vec<String> = source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(source)
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//"))
            .filter_map(|l| {
                let reste = l.strip_prefix("[\"")?;
                let (verbe, fin) = reste.split_once('"')?;
                matches!(fin.chars().next(), Some(']') | Some(',')).then(|| verbe.to_string())
            })
            .collect();
        page.sort();
        page.dedup();
        rust.sort();
        rust.dedup();
        assert_eq!(page, rust, "les verbes de bandeau.html et de `dispatch_action` doivent etre les memes");
        assert!(rust.contains(&"supprimer".to_string()), "le verbe de la croix doit etre cable");
    }

    /// **LE LEVIER DE MESURE N'ARME PAS LE BANDEAU** — sans quoi la carte se
    /// poserait par-dessus la liste qu'on veut photographier, et la vérification
    /// montrerait exactement ce qu'elle devait écarter.
    #[test]
    fn le_remplissage_de_test_n_arme_pas_le_bandeau() {
        historique_vide();
        debug_remplir(12);
        assert_eq!(combien(), 12);
        assert!(!EN_ATTENTE.with(|a| a.get()), "aucune carte ne doit etre armee");
        historique_vide();
    }

    /// ⭐ **LE LEVIER ANIMÉ FAIT ARRIVER LES MESSAGES, UN PAR CADENCE** — et c'est
    /// l'inverse exact du précédent : il ARME le bandeau, parce que son objet est
    /// de montrer ce qui s'affiche, pas de photographier une liste immobile.
    ///
    /// ⚠️ Le test compte les images (`tic_animation`) plutôt que d'attendre : la
    /// cadence est en frames, et une attente réelle rendrait le test lent ET
    /// instable.
    #[test]
    fn le_levier_anime_fait_arriver_les_messages_un_par_cadence() {
        historique_vide();
        debug_animer(2);
        tic_animation();
        assert_eq!(combien(), 1, "le premier message part a la premiere image");
        assert!(EN_ATTENTE.with(|a| a.get()), "le levier ANIME arme le bandeau, lui");
        EN_ATTENTE.with(|a| a.set(false));
        for _ in 0..CADENCE_ANIM_FRAMES {
            tic_animation();
        }
        assert_eq!(combien(), 1, "la cadence n'est pas encore ecoulee");
        // ⚠️ **L'IMAGE OÙ LE COMPTE TOMBE À ZÉRO NE TIRE PAS ELLE-MÊME** : c'est la
        // suivante qui émet. D'où cet appel de plus — le compte à rebours d'un
        // intervalle en images coûte toujours une image de plus que sa valeur.
        tic_animation();
        assert_eq!(combien(), 2, "le deuxieme part a la fin de la cadence");
        // Épuisé, il s'ARRÊTE : sans cette garde, une démonstration laissée
        // tourner remplirait l'historique indéfiniment.
        for _ in 0..(CADENCE_ANIM_FRAMES * 2) {
            tic_animation();
        }
        assert_eq!(combien(), 2);
        historique_vide();
    }
}
