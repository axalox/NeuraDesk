//! Fenêtre CEF pour le panneau de Réglages — Phase 6 de
//! `docs/plans/cef-web-ui-migration.md`, dernier gros morceau de cette
//! migration. Remplace `win32::settings_dash` (module egui séparé, PAS un
//! `PopupKind` — comme `win32::cef_dash`/`win32::cef_clipboard`, cette
//! fenêtre remplit toute la "zone libre" du dock).
//!
//! **Particularité structurante** : contrairement à TOUS les popups
//! précédents, ce panneau NE POSSÈDE QUASIMENT AUCUN état à lui — il
//! lit/modifie DIRECTEMENT les variables locales d'`app::run` (`Dock`,
//! épaisseurs de barre, `WindowAccent` × 3, `Features`...), exactement
//! comme `win32::settings_dash::render_if_visible` le fait déjà (voir sa
//! doc de module : "plus de protocole... l'UI tourne dans LE MÊME
//! thread..., une frame de ce panneau peut donc lire/modifier `Dock`/
//! `bar_tint`/... DIRECTEMENT"). `poll` (voir sa doc) reçoit donc le même
//! genre de liste de références mutables que `render_if_visible`,
//! regroupées dans `SettingsRefs` pour rester lisible plutôt qu'une
//! vingtaine de paramètres positionnels. SEULE exception (voir `DATA`) :
//! "Mes couleurs"/les thèmes personnalisés/la "Couleur de base" du
//! sélecteur de thème — dans `win32::settings_dash` ELLE-MÊME, ces trois
//! champs vivent DÉJÀ dans le `SettingsState` propre à CE panneau (pas dans
//! `app::run`, voir sa doc de champ), ce module fait donc pareil plutôt que
//! d'inventer un endroit artificiel côté `app::run` pour un état qui n'a
//! jamais été le sien.
//!
//! **Pipette d'écran** (voir `start_eyedropper`) : mécanisme Win32
//! (`WH_MOUSE_LL`/`GetPixel`) entièrement dupliqué de `win32::settings_dash`
//! — ni egui ni CEF n'y jouent le moindre rôle, voir sa doc détaillée
//! là-bas pour la justification complète (pourquoi un crochet bas niveau,
//! pas un simple sondage).
//!
//! **État poussé À CHAQUE FRAME tant que visible** (voir `poll`), comme
//! `win32::cef_voicemeeter` — ce panneau peut changer depuis PLUSIEURS
//! sources à la fois (un thème complet touche huit réglages d'un coup), un
//! rafraîchissement complet à chaque frame est plus simple à maintenir
//! correct qu'un suivi fin de "qu'est-ce qui a changé cette frame". Le
//! risque d'interrompre un glissement de curseur en cours est traité CÔTÉ
//! JS, même technique que `win32::cef_voicemeeter` (voir `assets/popups/
//! settings.html`, doc d'en-tête).
//!
//! **PAS de sélecteur de couleur natif** (`<input type="color">`) —
//! délibérément évité : ce composant ouvrirait une fenêtre top-level
//! SÉPARÉE, exactement la classe de bug de focus que `win32::settings_dash`
//! (doc de module) a abandonnée en migrant loin du `<input type="color">`
//! HTML de l'ANCIENNE version WebView2. Le champ hexadécimal + la pipette +
//! les 48 préréglages Windows + "Mes couleurs" + un dégradé teinte/
//! saturation/valeur maison EN PAGE (`gradientPicker` dans `assets/popups/
//! settings.html`, équivalent du `color_edit_button_rgb` d'egui qu'avait
//! `win32::settings_dash`) couvrent le même besoin sans ce risque : aucun
//! de ces contrôles n'ouvre de fenêtre séparée, donc rien qui échapperait
//! au focus de CE popup.

use std::cell::{Cell, RefCell};
use std::path::Path;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::json;use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DefWindowProcW, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, HHOOK, MSLLHOOKSTRUCT,
    WH_MOUSE_LL, WM_LBUTTONDOWN,
};

use crate::app::Features;
use crate::config;
use crate::gfx::blur;
use crate::gfx::device::GfxDevice;
use crate::gfx::material;
use crate::ui::color;
use crate::ui::dock::Dock;
use crate::win32::cef_clipboard;
use crate::win32::cef_app::percent_decode;
use crate::win32::cef_dash;
use crate::win32::cef_notes;
use crate::win32::taille_panneaux;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::clipboard;
use crate::win32::menu::{self, Bar};
use crate::win32::monitors;
use crate::win32::taskbar_autohide;
use crate::win32::theme;

/// Voir la doc de module (dernier paragraphe) : les trois seuls champs que
/// ce module possède réellement — chargés paresseusement (voir `data`), pas
/// via `const { ... }` (`Vec::new()`/valeur par défaut suffiraient, mais le
/// VRAI chargement disque n'a besoin de se faire qu'à la première ouverture,
/// pas à la création du thread_local).
struct SettingsData {
    custom_colors: Vec<[f32; 3]>,
    custom_themes: Vec<[f32; 3]>,
    theme_base_color: [f32; 3],
    /// **LES FORCES DU THÈME, TENUES EN MÉMOIRE** — voir `forces_du_theme`.
    ///
    /// ⛔ **ELLES NE SE RELISENT PAS DU DISQUE À CHAQUE FRAME.** Défaut réel
    /// signalé le 2026-10-10 : « ça bug encore » — le journal montrait les
    /// valeurs REBONDIR pendant le glissement (`0.3 → 0.35 → 0.4 → 0.45 → 0.4 →
    /// 0.35 → 0.25`). L'écriture de `config` est *best-effort*, donc la relecture
    /// rendait souvent la valeur PRÉCÉDENTE ; l'état la poussait, et
    /// `updateSlider` la réécrivait dans le curseur **en plein glissement**.
    theme_force_saturation: f32,
    theme_force_luminosite: f32,
    /// ⭐ **LA COULEUR DE BASE DE CHAQUE PANNEAU, HORS FORCES** — d'où les
    /// curseurs saturation/luminosité repartent (voir `reappliquer_forces`).
    /// Clé : le nom de fenêtre (`"dash"`…, voir `FENETRES_ACCENT`). `fond` =
    /// couleur de fond choisie ; `palette` = couleur de palette séparée si
    /// personnalisée (voir `WindowAccent::palette`).
    ///
    /// ⛔ **POURQUOI MÉMORISER UNE BASE PAR PANNEAU** — défaut signalé le
    /// 2026-11-06 (« dès que je bouge les curseurs je perds la personnalisation
    /// que j'ai faite ») : les deux curseurs réappliquaient le thème ENTIER
    /// (`apply_theme`), reposant la MÊME couleur partout et effaçant les
    /// couleurs individuelles de « Couleur de mise en avant ». Désormais chaque
    /// panneau est ré-échelonné depuis SA base, ce qui préserve les écarts.
    /// Posée en bloc par `poser_bases_uniformes` (thème/famille) et par panneau
    /// par `enregistrer_base_manuelle` (choix individuel).
    theme_bases: std::collections::HashMap<&'static str, ([f32; 3], Option<[f32; 3]>)>,
}

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    static NATIVE_TINT_RESTART_PENDING: Cell<bool> = const { Cell::new(false) };
    /// La section de l'aide qu'un bouton des Réglages demande d'ouvrir — `app::run`
    /// la prend (`take_aide_demandee`), seul à avoir de quoi poser le panneau.
    static AIDE_DEMANDEE: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Voir `win32::settings_dash::EYEDROPPER_ACTIVE` — même rôle, cible
    /// identifiée par une chaîne (voir la doc de module, "target"). `String`
    /// possédée (pas `&'static str`) : les cibles des écrans secondaires
    /// sont construites dynamiquement (`"secondary:<device_name>:<bar>"`,
    /// voir `apply_color_to_target`) et n'ont donc pas de représentant
    /// `'static` connu à la compilation, contrairement aux 13 cibles fixes
    /// de `leak_known_target` (toujours utilisé là où c'est suffisant,
    /// `window_static`) — DEMANDE EXPLICITE ("mêmes possibilités de
    /// réglages... sélecteur de couleur" pour les écrans secondaires).
    static EYEDROPPER_ACTIVE: RefCell<Option<String>> = const { RefCell::new(None) };
    static EYEDROPPER_RESULT: Cell<Option<[f32; 3]>> = const { Cell::new(None) };
    static EYEDROPPER_HOOK: Cell<Option<HHOOK>> = const { Cell::new(None) };
    /// **LA SAUVEGARDE DE LA CONFIGURATION, VUE PAR LA PAGE** — le compte rendu du
    /// dernier geste, et l'import préparé qui attend sa confirmation (voir
    /// `crate::sauvegarde_config`).
    static SAUVEGARDE_MESSAGE: RefCell<String> = const { RefCell::new(String::new()) };
    static SAUVEGARDE_APERCU: RefCell<Option<crate::sauvegarde_config::Apercu>> = const { RefCell::new(None) };
    static DATA: RefCell<Option<SettingsData>> = const { RefCell::new(None) };
    /// Les réglages d'envoi en ligne de l'éditeur d'image (voir
    /// `win32::envoi_image`), lus UNE fois : l'état est poussé à chaque frame,
    /// et ce fichier ne change que par les actions `envoi-*` de ce panneau.
    static ENVOI: RefCell<Option<crate::win32::envoi_image::Reglages>> = const { RefCell::new(None) };
    /// Le compte rendu de la dernière modification du serveur personnalisé
    /// (`(texte, est_une_erreur)`) : une adresse refusée ou un `.sxcu` illisible
    /// doit se DIRE, pas s'effacer en silence au prochain rendu.
    static MESSAGE_PERSO: RefCell<Option<(String, bool)>> = const { RefCell::new(None) };
    /// `debug_section` n'écrit sa trace qu'une fois (il est appelé à chaque frame).
    static SECTION_SIGNALEE: Cell<bool> = const { Cell::new(false) };
    /// Échelle du contenu de CE panneau (variable CSS `--content-scale`,
    /// zoom sur `.scroll` — voir `assets/popups/settings.html`) — voir
    /// `content_scale`/`set_content_scale`. Même mécanisme que `win32::
    /// cef_dash::ICON_SCALE`, propre à ce module plutôt que partagé : ce
    /// panneau se pousse son propre état à CHAQUE FRAME (voir `poll`), donc
    /// pas besoin d'attendre une prochaine ouverture comme Dash/
    /// Presse-papiers — pris en compte immédiatement, curseur en main.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// Réglage MANUEL par écran secondaire — voir `win32::cef_dash::
    /// SCREEN_SCALE_OVERRIDE`, doc de champ, même mécanisme exact. Relu à
    /// CHAQUE frame par `push_state` (comme `CONTENT_SCALE` ci-dessus,
    /// contrairement à Dash/Presse-papiers qui n'y retournent qu'à
    /// l'ouverture) — ce panneau se pousse déjà son propre état en continu.
    /// Le second terme est le **RATIO DE DPI** de l'écran visé — voir
    /// `cef_dash::SCREEN_SCALE_OVERRIDE`, doc de champ, même raison.
    static SCREEN_SCALE_OVERRIDE: Cell<(Option<f32>, f32)> = const { Cell::new((None, 1.0)) };
    /// **LE FORÇAGE GLOBAL DU RATIO DE DPI** — voir `config::load_ratio_dpi_force`,
    /// qui porte le pourquoi (rendre la correction vérifiable sur la PRIMAIRE) et
    /// la précédence (il l'emporte sur le réglage d'écran).
    ///
    /// `0.0` = éteint, et c'est le défaut.
    static RATIO_DPI_FORCE: Cell<f32> = const { Cell::new(0.0) };
}

/// Facteur d'échelle actuel du contenu du panneau — voir `CONTENT_SCALE`.
pub fn content_scale() -> f32 {
    CONTENT_SCALE.with(Cell::get)
}

/// Change le facteur d'échelle du contenu du panneau, pris en compte dès la
/// PROCHAINE FRAME (voir `CONTENT_SCALE`, doc de champ) — appelé par
/// `dispatch_actions` (curseur "Contenu" de la sous-section "Réglages") et
/// par `app::run` au démarrage (voir `config::load_settings_content_scale`).
/// Borné à [0.25, 2.0] — plus large côté bas que `win32::cef_dash::
/// set_icon_scale` ([0.5, 2.0]) : DEMANDE EXPLICITE de pouvoir réduire
/// encore plus, et `zoom` (contrairement à des `calc()` sur des tailles
/// fixes) reste lisible à une échelle bien plus réduite sans qu'aucun
/// élément ne se chevauche.
pub fn set_content_scale(scale: f32) {
    CONTENT_SCALE.with(|s| s.set(scale.clamp(0.25, 2.0)));
}

/// Fixe/efface le réglage MANUEL d'échelle de l'écran sur lequel ce
/// panneau vient d'être ouvert — voir `win32::cef_dash::
/// set_screen_scale_override`, même schéma exact.
pub fn set_screen_scale_override(scale: Option<f32>, ratio_dpi: f32) {
    SCREEN_SCALE_OVERRIDE.with(|s| s.set((scale, ratio_dpi)));
}

/// Voir `RATIO_DPI_FORCE`, doc de champ. Appelé à chaque frame par `app::run`
/// (comme `set_screen_scale_override`), et écrit par le curseur de la page.
pub fn set_ratio_dpi_force(force: f32) {
    RATIO_DPI_FORCE.with(|c| c.set(force));
}

/// Le forçage global — `0.0` quand il est éteint. Lu par `app::ratio_dpi_effectif`,
/// qui applique la précédence.
pub fn ratio_dpi_force() -> f32 {
    RATIO_DPI_FORCE.with(Cell::get)
}

/// **L'ÉCHELLE RÉELLEMENT RENDUE** par ce panneau : le réglage manuel de l'écran
/// s'il existe, sinon le global — **le tout multiplié par le ratio de DPI**.
///
/// ⛔ **LE RATIO S'APPLIQUE AUX DEUX CHEMINS**, comme pour le Dash et le
/// Presse-papiers : sinon un écran sans réglage manuel garderait la même taille
/// PHYSIQUE que la primaire, donc paraîtrait plus grand.
///
/// ⛔ **ET ELLE N'ALIMENTE QUE `settingsContentScaleEffectivePercent`**, jamais
/// `settingsContentScalePercent` — ce dernier est la valeur du CURSEUR global,
/// telle que l'interface la montre (voir le commentaire long de `push_state`, où
/// les avoir confondus ÉTAIT un défaut signalé).
fn echelle_effective(choix: Option<f32>, global: f32, ratio_dpi: f32) -> f32 {
    choix.unwrap_or(global) * ratio_dpi
}

/// Charge `DATA` depuis le disque au tout premier appel (voir
/// `config::load_custom_colors`/`load_custom_themes`/`load_theme_base_color`,
/// même repli `[0.2, 0.5, 0.85]` que `win32::settings_dash::create_window`
/// pour une "Couleur de base" jamais choisie) — les ouvertures suivantes
/// réutilisent l'état déjà en mémoire, comme `win32::cef_dash::DATA`.
fn with_data<R>(f: impl FnOnce(&mut SettingsData) -> R) -> R {
    DATA.with(|d| {
        let mut data = d.borrow_mut();
        let data = data.get_or_insert_with(|| SettingsData {
            custom_colors: config::load_custom_colors(),
            custom_themes: config::load_custom_themes(),
            theme_base_color: config::load_theme_base_color().unwrap_or([0.2, 0.5, 0.85]),
            // ⚠️ **PAR DÉFAUT, LA COULEUR TELLE QU'ELLE A ÉTÉ CHOISIE.**
            // ⚠️ Borné aussi à la relecture : une valeur enregistrée avant la
            // borne (jusqu'à 2) ferait sinon sortir le curseur de sa course.
            theme_force_saturation: config::load_theme_force_saturation()
                .unwrap_or(color::FORCE_NEUTRE)
                .clamp(0.0, color::FORCE_SATURATION_MAX),
            theme_force_luminosite: config::load_theme_force_luminosite().unwrap_or(color::FORCE_NEUTRE),
            // Vide au départ : la base d'un panneau est posée au premier
            // thème/famille/choix appliqué de la session ; à défaut, le premier
            // geste de curseur capture la couleur affichée (voir
            // `reappliquer_forces`).
            theme_bases: std::collections::HashMap::new(),
        });
        f(data)
    })
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefSettingsWindow"),
    window_title: w!("NeuraDesk — Réglages"),
    html_file: "settings.html",
    wndproc: Some(wndproc),
    // PANNEAU — reste SOUS le dock agrandi, voir
    // `PopupSpec::above_a_magnified_dock`.
    above_a_magnified_dock: false,
    // PANNEAU — ne fond pas, voir `PopupSpec::fades`.
    fades: false,
    fenetre: false,
};

/// Précharge ce popup au démarrage — fenêtre et navigateur naissent CACHÉS, pour
/// que la page ait fini de charger avant la première ouverture réelle. Voir
/// `win32::cef_popup::preload`, doc de fonction, qui porte le pourquoi complet.
pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// Tout ce que `poll` doit pouvoir lire/modifier — voir la doc de module.
/// Un simple regroupement de références déjà existantes côté `app::run`,
/// PAS un état possédé par ce module (voir la doc de module).
pub struct SettingsRefs<'a> {
    pub dock: &'a mut Dock,
    /// **CE QU'IL FAUT SAVOIR D'UNE ICÔNE POUR LA LISTER DANS SA ZONE** — son nom,
    /// et l'action qu'on peut lui offrir (voir `ZoneRow`).
    ///
    /// **Une CLOSURE, et pas les données** : nommer une icône demande les sept
    /// index d'applets et la liste des épingles, qui ne vivent que dans `app::run`.
    /// Les téléporter ici dupliquerait la connaissance qui décide des actions — et
    /// c'est cette duplication-là qui a coûté le défaut « les applets du haut se
    /// retrouvent dans la zone Réglages ». Elle n'est appelée que pour les lignes
    /// réellement affichées : pas de `Vec` construit à chaque frame.
    pub zone_row: &'a dyn Fn(usize) -> ZoneRow,
    pub topbar_text_color: &'a mut [f32; 3],
    pub bar_tint: &'a mut [f32; 4],
    /// **LES RACCOURCIS DE DOSSIERS DE LA BARRE DU BAS** (voir
    /// `config::DossierEpingle`) — MUTABLE : cette page les ajoute, les renomme,
    /// les retire et leur donne une vignette.
    ///
    /// ⚠️ **CHAQUE CHANGEMENT S'APPLIQUE TOUT DE SUITE** (étape 4 du refactor des
    /// identités stables, voir `dossiers_ecrits`) : la barre du bas et ses textures
    /// suivent la liste **dans la seconde**, comme le fait déjà le bouton d'épinglage du
    /// sous-dock. Avant, ces cinq gestes-ci étaient les SEULS à exiger un redémarrage —
    /// deux moitiés du même geste qui se comportaient différemment.
    pub dossiers: &'a mut Vec<config::ElementBarre>,
    /// **LES TEXTURES DES RACCOURCIS, ET DE QUOI EN FABRIQUER** — l'emprunt de `app::run`
    /// qui permet au panneau de faire suivre la BARRE, et pas seulement le fichier (voir
    /// `dossiers_ecrits`). Même règle que le reste de cette structure : ce ne sont pas des
    /// états possédés ici, mais des références déjà existantes côté `app::run`.
    pub emplacements_dossiers: &'a mut crate::app::EmplacementsDossiers,
    pub gfx: &'a crate::gfx::GfxDevice,
    pub dash_accent: &'a mut color::WindowAccent,
    pub settings_accent: &'a mut color::WindowAccent,
    pub clipboard_accent: &'a mut color::WindowAccent,
    pub notes_accent: &'a mut color::WindowAccent,
    /// Couleur propre de la fenêtre du CALENDRIER (voir `win32::
    /// cef_calendar_applet`) — DEMANDE EXPLICITE (2026-09-23) : « ajoutes les
    /// réglages pour le dash calendrier dans le dash réglages ». Cinquième
    /// fenêtre à en avoir une, après Dash/Réglages/Presse-papiers/Notes.
    pub calendar_accent: &'a mut color::WindowAccent,
    /// Minuteur, Surveillance, Calculatrice, Serveurs SSH — DEMANDE EXPLICITE
    /// (2026-09-26) : « on a ajouté des choses mais les possibilités de réglages
    /// n'ont pas été mises à jour ». Ils empruntaient jusque-là la couleur du
    /// Calendrier, qui reste leur valeur de départ (voir `app::run`).
    pub minuteur_accent: &'a mut color::WindowAccent,
    pub surveillance_accent: &'a mut color::WindowAccent,
    pub calculatrice_accent: &'a mut color::WindowAccent,
    pub ssh_accent: &'a mut color::WindowAccent,
    pub terminal_accent: &'a mut color::WindowAccent,
    /// Dash dossier, visionneuse, sélecteur de couleurs, éditeur d'image
    /// (DEMANDE EXPLICITE, 2026-10-03) — voir `app::run`, `accent_du_dash`.
    pub dossier_accent: &'a mut color::WindowAccent,
    pub visionneuse_accent: &'a mut color::WindowAccent,
    pub couleurs_accent: &'a mut color::WindowAccent,
    pub editeur_accent: &'a mut color::WindowAccent,
    /// Les dashs qui empruntaient la couleur du Dash (DEMANDE, 2026-10-24) —
    /// voir `app::run`, `accent_du_dash`.
    pub scanner_accent: &'a mut color::WindowAccent,
    pub shaders_accent: &'a mut color::WindowAccent,
    pub lueurs_accent: &'a mut color::WindowAccent,
    pub textures_accent: &'a mut color::WindowAccent,
    pub moniteur_accent: &'a mut color::WindowAccent,
    pub recherche_accent: &'a mut color::WindowAccent,
    pub automatisations_accent: &'a mut color::WindowAccent,
    pub meteo_accent: &'a mut color::WindowAccent,
    pub aide_accent: &'a mut color::WindowAccent,
    pub dash_tint: &'a mut f32,
    pub auto_color_scheme: &'a mut bool,
    pub top_thickness: &'a mut f32,
    pub left_width: &'a mut f32,
    pub right_width: &'a mut f32,
    /// Épaisseur de la barre du BAS — **écran principal seulement** (voir
    /// `ui::dock::Side::Bottom`) : elle n'est réglable que là, et son curseur
    /// vit à côté des trois autres.
    pub bottom_thickness: &'a mut f32,
    /// **Hauteur UTILE de la barre de droite** (pixels) — la fenêtre du dock moins l'épaisseur de
    /// la barre du haut.
    ///
    /// Elle n'existe que pour **le budget des zones** (voir `assets/popups/settings.html`) : une
    /// cote est une RÉSERVATION FIXE (une zone ne rétrécit pas pour tenir à son contenu, c'est un
    /// choix du modèle, figé par `ui::dock`), donc la somme des cotes peut dépasser la barre — et
    /// rien ne le disait. L'utilisateur voyait alors des zones « écrasées qui ne réagissent pas »
    /// alors que ses cotes réservaient 3481 px pour une barre de 2016 : c'est une ADDITION, pas un
    /// défaut. Le panneau peut désormais la poser à sa place.
    ///
    /// Lecture seule : un réglage la changerait, mais elle est la CONSÉQUENCE des tailles de
    /// barres, pas un réglage de plus.
    pub right_bar_height: f32,
    pub features: &'a mut Features,
    /// `true` PENDANT qu'un curseur "Épaisseur/Largeur de barre" (section
    /// "Tailles des barres") est en cours de glissement — voir `dispatch_actions`
    /// (`["bar-size", ...]` le met à `true`, `["bar-size-commit"]` à `false`)
    /// et `app::run` (utilisé pour GELER le redimensionnement du panneau
    /// Réglages lui-même tant que c'est vrai). BUG RÉEL SIGNALÉ ("j'aimerais
    /// que le dash de réglages ne se redimensionne automatiquement que
    /// lorsque j'ai relâché le curseur de largeur de la barre de droite") :
    /// `top_thickness`/`left_width`/`right_width` ci-dessus s'appliquent
    /// bien EN TEMPS RÉEL aux vraies barres (comportement voulu, la barre
    /// visible doit suivre le curseur pendant le glissement) — mais la
    /// "zone libre" où vit CE panneau (voir `app::dash_rect`) en dépend
    /// AUSSI, donc le panneau se redimensionnait lui-même à CHAQUE pixel
    /// glissé, sous le curseur de l'utilisateur en train précisément de
    /// glisser CE curseur — cible qui bouge sous la souris en plein
    /// glissement. `top_thickness`/`left_width`/`right_width` restent
    /// appliqués sans délai ; seul le REDIMENSIONNEMENT DE LA FENÊTRE de ce
    /// panneau (`win32::cef_popup::resize_if_needed`) est reporté jusqu'au
    /// relâchement.
    pub bar_size_dragging: &'a mut bool,
    /// Masquage MANUEL PERMANENT des barres latérales (voir `app::
    /// effective_primary_thicknesses`, `config::load_bool_setting`) —
    /// DEMANDE EXPLICITE ("donnes la possibilité à l'utilisateur de masquer
    /// les barres droite et gauche"). Pas d'équivalent pour la barre du
    /// haut (non demandé) ni pour les écrans secondaires (pas de barre
    /// gauche/droite sur un écran secondaire, voir `win32::window::
    /// SecondaryWindow`).
    pub left_bar_hidden: &'a mut bool,
    pub right_bar_hidden: &'a mut bool,
    /// La barre du BAS — mêmes deux réglages (DEMANDE EXPLICITE, 2026-10-03).
    pub bottom_bar_hidden: &'a mut bool,
    /// Masquage AUTOMATIQUE façon barre des tâches Windows, réglable
    /// INDÉPENDAMMENT pour chacune des trois barres primaires — DEMANDE
    /// EXPLICITE ("rendre les barres de l'interface secondaire masquable
    /// automatiquement ainsi que les barres de l'interface principale", PUIS
    /// "au lieu de masquage automatique de l'interface principale, [pouvoir]
    /// activer le masquage automatique de chaque élément de l'interface
    /// principale individuellement") — remplace l'ancien réglage unique
    /// `primary_auto_hide` (un seul état de révélation partagé pour les
    /// trois). `secondary_auto_hide` couvre la barre du haut de CHAQUE écran
    /// secondaire, indépendamment les unes des autres (voir `app::
    /// SecondaryDock::bar_revealed`).
    pub top_auto_hide: &'a mut bool,
    pub left_auto_hide: &'a mut bool,
    pub right_auto_hide: &'a mut bool,
    pub bottom_auto_hide: &'a mut bool,
    pub secondary_auto_hide: &'a mut bool,
    /// Choix de copie du dock par écran secondaire (voir `app::
    /// SecondaryMirror`, `win32::monitors::MonitorInfo::device_name` comme
    /// clé) — DEMANDE EXPLICITE ("proposer à l'utilisateur de pouvoir
    /// placer une copie de la barre supérieure ou une copie de toute l'app
    /// sur les écrans secondaires"). Valeurs : `"top"` (barre du haut
    /// seule), `"full"` (application complète), absent = désactivé pour cet
    /// écran. Persisté par `config::save_secondary_screen_modes` à chaque
    /// changement (voir `dispatch_actions`).
    pub secondary_screen_modes: &'a mut std::collections::HashMap<String, String>,
    /// Épaisseur/largeur des barres PERSONNALISÉE par écran secondaire (voir
    /// `config::load_secondary_screen_thicknesses`, `app::
    /// SecondaryScreenOverrides::thicknesses`) — DEMANDE EXPLICITE ("pouvoir
    /// régler les barres secondaires individuellement... hauteur, largeur...
    /// séparés entre chaque écran"). `[top, left, right]` en pixels
    /// physiques ; un écran absent de cette table retombe sur la dérivation
    /// automatique (comportement d'origine, voir `app::rebuild_secondary_docks`).
    /// Persisté par `config::save_secondary_screen_thicknesses` à chaque
    /// changement (voir `dispatch_actions`).
    pub secondary_screen_thicknesses: &'a mut std::collections::HashMap<String, [f32; 3]>,
    /// Transparence des trois barres PERSONNALISÉE par écran secondaire
    /// (voir `config::load_secondary_screen_tint`) — même convention que
    /// `secondary_screen_thicknesses` ci-dessus ; un écran absent retombe
    /// sur `bar_tint` (la barre PRIMAIRE).
    pub secondary_screen_tint: &'a mut std::collections::HashMap<String, [f32; 3]>,
    /// Couleur de fond des trois barres PERSONNALISÉE par écran secondaire
    /// (voir `config::load_secondary_screen_colors`) — même convention que
    /// `secondary_screen_thicknesses` ci-dessus ; un écran (ou une barre)
    /// absent(e) retombe sur la couleur résolue de la barre PRIMAIRE
    /// correspondante (voir `ui::dock::Dock::bar_colors`).
    pub secondary_screen_colors: &'a mut std::collections::HashMap<String, [Option<[f32; 4]>; 3]>,
    /// Éléments de barre DÉSACTIVÉS pour un écran secondaire donné, en plus
    /// du réglage global (voir `config::load/save_secondary_screen_features`,
    /// `app::SecondaryScreenOverrides::features`) — DEMANDE EXPLICITE
    /// ("sélectionner ou déselectionner des éléments des barres secondaires
    /// pour les rendre visibles ou invisibles"). Mêmes identifiants que
    /// `app::set_feature_by_name`. Un écran absent (ou liste vide) suit
    /// `features` ci-dessus sans exception.
    pub secondary_screen_features: &'a mut std::collections::HashMap<String, Vec<String>>,
    /// "Colorisation automatique" PERSONNALISÉE par écran secondaire (voir
    /// `auto_color_scheme` ci-dessus pour l'équivalent PRIMAIRE, `app::
    /// SecondaryScreenOverrides::auto_color_scheme`) — DEMANDE EXPLICITE
    /// ("les mêmes possibilités de réglages pour l'interface secondaire que
    /// pour l'interface principale"). Un écran absent de cette table suit
    /// `auto_color_scheme` (la barre PRIMAIRE) sans exception.
    pub secondary_screen_auto_color_scheme: &'a mut std::collections::HashMap<String, bool>,
    /// Couleur du texte de la barre du haut PERSONNALISÉE par écran
    /// secondaire (voir `topbar_text_color` ci-dessus pour l'équivalent
    /// PRIMAIRE, `app::SecondaryScreenOverrides::topbar_text_color`) —
    /// même demande explicite que `secondary_screen_auto_color_scheme`
    /// ci-dessus ; sans effet tant que la "Colorisation automatique" de cet
    /// écran est active (même règle que côté PRIMAIRE).
    pub secondary_screen_topbar_text_color: &'a mut std::collections::HashMap<String, [f32; 3]>,
    /// Activation de la mini-barre d'horloge plein écran PERSONNALISÉE par
    /// écran secondaire (voir `win32::fullscreen_clock::is_enabled` pour
    /// l'équivalent PRIMAIRE, `app::SecondaryScreenOverrides::
    /// fullscreen_clock_enabled`) — DEMANDE EXPLICITE ("pouvoir régler
    /// individuellement les horloges en plein écran en fonction de l'écran
    /// où elles s'affichent, je ne veux pas un réglage unique pour toutes
    /// les horloges"). Un écran absent de cette table suit le réglage
    /// PRIMAIRE sans exception.
    pub secondary_screen_fullscreen_clock_enabled: &'a mut std::collections::HashMap<String, bool>,
    /// Emplacement (haut/bas, `true` = haut) PERSONNALISÉ par écran
    /// secondaire — même demande explicite que
    /// `secondary_screen_fullscreen_clock_enabled` ci-dessus.
    pub secondary_screen_fullscreen_clock_position_top: &'a mut std::collections::HashMap<String, bool>,
    /// Facteur d'échelle de la barre PERSONNALISÉ par écran secondaire —
    /// même demande explicite que
    /// `secondary_screen_fullscreen_clock_enabled` ci-dessus.
    pub secondary_screen_fullscreen_clock_bar_scale: &'a mut std::collections::HashMap<String, f32>,
    /// Facteur d'échelle du texte PERSONNALISÉ par écran secondaire — même
    /// demande explicite que `secondary_screen_fullscreen_clock_enabled`
    /// ci-dessus.
    pub secondary_screen_fullscreen_clock_text_scale: &'a mut std::collections::HashMap<String, f32>,
    /// Taille des pastilles de bureaux virtuels PERSONNALISÉE par écran
    /// secondaire (voir `app::SecondaryScreenOverrides::desktop_dot_sizes`,
    /// `ui::dock::Dock::desktop_dot_size`) — DEMANDE EXPLICITE ("je voudrais
    /// que les pastilles des bureaux virtuels soient redimensionnables dans
    /// le dash réglage, en ayant chacune leur propre réglage dans la barre
    /// correspondante"). Un écran absent de cette table suit la taille
    /// PRIMAIRE (`refs.dock.desktop_dot_size()`) sans exception.
    pub secondary_screen_desktop_dot_sizes: &'a mut std::collections::HashMap<String, f32>,
    /// Taille de police PERSONNALISÉE des textes de la barre du haut, par écran
    /// secondaire (voir `app::SecondaryScreenOverrides::topbar_text_sizes`,
    /// `ui::dock::Dock::topbar_text_size`) — DEMANDE EXPLICITE (2026-09-23), un
    /// réglage par écran comme les pastilles ci-dessus.
    ///
    /// ⚠️ **UN ÉCRAN ABSENT SUIT LA BARRE PRIMAIRE**, et celle-ci peut elle-même
    /// être en AUTOMATIQUE : contrairement à `secondary_screen_desktop_dot_sizes`
    /// (où le repli est toujours un nombre), l'absence n'a donc PAS de valeur
    /// unique à opposer — elle veut dire « comme la primaire », automatique
    /// compris (voir `app::rebuild_secondary_docks`).
    pub secondary_screen_topbar_text_sizes: &'a mut std::collections::HashMap<String, f32>,
    /// Écartement entre icônes PERSONNALISÉ par écran secondaire (voir
    /// `app::SecondaryScreenOverrides::icon_gaps`, `ui::dock::Dock::
    /// icon_gap`) — DEMANDE EXPLICITE ("je voudrais pouvoir choisir l'espace
    /// par defaut entre les icones sur les barres secondaires comme sur
    /// l'interface principale"). Un écran absent de cette table suit
    /// l'écartement PRIMAIRE (`refs.dock.icon_gap()`) sans exception —
    /// même convention que `secondary_screen_desktop_dot_sizes` ci-dessus.
    pub secondary_screen_icon_gaps: &'a mut std::collections::HashMap<String, f32>,
    /// Couleur des séparateurs PERSONNALISÉE par écran secondaire (voir
    /// `app::SecondaryScreenOverrides::separator_colors`, `ui::dock::Dock::
    /// separator_color`) — DEMANDE EXPLICITE ("donnes la possibilité de
    /// choisir leur couleur ou de les rendre transparent pour l'interface
    /// principale et les barres secondaires"). Un écran absent de cette
    /// table suit la couleur PRIMAIRE (`refs.dock.separator_color()`, elle-
    /// même potentiellement absente = suit la couleur du texte) sans
    /// exception — même convention que `secondary_screen_icon_gaps`
    /// ci-dessus.
    pub secondary_screen_separator_colors: &'a mut std::collections::HashMap<String, [f32; 4]>,
    /// Diamètre du petit cercle indicateur des séparateurs PERSONNALISÉ par
    /// écran secondaire (voir `app::SecondaryScreenOverrides::
    /// separator_sizes`, `ui::dock::Dock::separator_size`) — DEMANDE
    /// EXPLICITE ("ajoute la possibilité de modifier la taille des
    /// séparateurs par interface"). Un écran absent de cette table suit la
    /// taille PRIMAIRE (`refs.dock.separator_size()`) sans exception — même
    /// convention que `secondary_screen_icon_gaps` ci-dessus.
    pub secondary_screen_separator_sizes: &'a mut std::collections::HashMap<String, f32>,
    /// Largeur des vignettes du panneau d'aperçus de fenêtres (voir
    /// `win32::window_previews`) — DEMANDE EXPLICITE : « une taille ajustable
    /// dans les réglages ». Variable LOCALE de `app::run`, comme les autres :
    /// elle y est appliquée au panneau à chaque frame.
    pub preview_thumb_width: &'a mut f32,
    /// Facteurs d'échelle PERSONNALISÉS par écran secondaire (voir
    /// `config::load_secondary_screen_scales`) — DEMANDE EXPLICITE
    /// ("l'utilisateur ait la possibilité de régler les tailles du dash et
    /// des menus et de leurs éléments pour les différentes interfaces
    /// secondaires comme il est possible de le faire pour l'interface
    /// principale"). `[menu, dash_icon, dash_label, dash_category,
    /// clipboard_thumb, clipboard_text, settings_content]` — un écran absent
    /// de cette table suit entièrement les réglages globaux (menu : la mise
    /// à l'échelle AUTOMATIQUE selon le RATIO DE DPI, voir `app::
    /// content_scale_pour` ; les autres : la valeur PRIMAIRE, comme avant
    /// cette fonctionnalité). Pas besoin de `secondary_screens_dirty` :
    /// contrairement au mode/à l'épaisseur, rien ici ne touche
    /// `rebuild_secondary_docks` — relu directement à l'ouverture/au
    /// prochain `push_state` du popup concerné (voir `app::run`).
    pub secondary_screen_scales: &'a mut std::collections::HashMap<String, [f32; crate::config::NB_CHAMPS_ECRAN]>,
    /// Mis à `true` par `dispatch_actions` dès qu'un choix ci-dessus (mode OU
    /// épaisseur OU couleur, tables juste au-dessus) change — lu puis remis
    /// à `false` par `app::run`, qui déclenche alors `rebuild_secondary_docks`
    /// (appel Win32/D3D11 non-gratuit, comme `bar_size_dragging` pour
    /// `set_dock_thicknesses` : jamais à chaque frame, seulement sur un
    /// changement réel).
    pub secondary_screens_dirty: &'a mut bool,
    /// Le forçage global du ratio de DPI — voir `config::load_ratio_dpi_force`.
    /// `0.0` = éteint. Écrit par la page, relu par `app::ratio_dpi_effectif`.
    pub ratio_dpi_force: &'a mut f32,
}

pub fn toggle(rect: [i32; 4], gfx: &GfxDevice) {
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, || {
        crate::win32::cef_dash::hide_if_visible();
        crate::win32::cef_clipboard::hide_if_visible();
        crate::win32::cef_notes::hide_if_visible();
        crate::win32::cef_image_editor::hide_if_visible();
        crate::win32::cef_minuteur::hide_if_visible();
        crate::win32::cef_surveillance::hide_if_visible();
        crate::win32::cef_scanner::hide_if_visible();
        crate::win32::cef_shaders::hide_if_visible();
        crate::win32::cef_lueurs::hide_if_visible();
        crate::win32::cef_textures::hide_if_visible();
        crate::win32::cef_moniteur::hide_if_visible();
        crate::win32::cef_calculatrice::hide_if_visible();
        crate::win32::cef_recherche::hide_if_visible();
        crate::win32::cef_automatisations::hide_if_visible();
        crate::win32::cef_meteo::hide_if_visible();
        crate::win32::cef_aide::hide_if_visible();
        crate::win32::bandeau::hide_if_visible();
        crate::win32::cef_ssh::hide_if_visible();
        crate::win32::cef_terminal::hide_if_visible();
        // Les polices installées, relues à chaque OUVERTURE (voir
        // `win32::polices`) : une police installée entre deux ouvertures y entre.
        crate::win32::polices::rafraichir();
        // Les deux catalogues STATIQUES (120 thèmes, 48 préréglages
        // Windows) sont poussés depuis `push_state` maintenant, PAS ici —
        // BUG RÉEL SIGNALÉ ("il manque des éléments" : les 48 couleurs
        // Windows n'apparaissaient jamais) : `execute_js` ici tourne
        // AVANT que la page ait fini de charger (le chargement HTML/JS
        // est ASYNCHRONE, `create_window` ne fait que LANCER la
        // navigation) — `window.setCatalog` n'existait pas encore côté
        // JS au moment de l'appel, l'erreur passait silencieusement
        // (aucune sortie console capturée par ce projet), `catalog`
        // restait à sa valeur initiale vide POUR TOUJOURS (jamais
        // rappelé depuis, contrairement à `push_state`). Voir sa doc :
        // même correctif de principe que `CONTENT_ZOOM_FACTOR`
        // (`win32::cef_popup`), qui a le même piège de timing.
    });
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

/// Voir `win32::cef_popup::debug_scroll` — réservé au harnais de
/// vérification (la fenêtre Réglages est plus haute que sa zone visible,
/// voir `assets/popups/settings.html`, section "Couleur de mise en avant").
pub fn debug_scroll(x: i32, y: i32, delta_y: i32) {
    cef_popup::debug_scroll(&SLOT, x, y, delta_y);
}

/// Voir `win32::cef_popup::debug_click` — réservé au harnais de
/// vérification.
pub fn debug_click(x: i32, y: i32) {
    cef_popup::debug_click(&SLOT, x, y);
}

pub fn render_if_visible(rect: [i32; 4], tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::resize_if_needed(&SLOT, rect);
    // Voir `win32::cef_dash::render_if_visible` — même raison : pas un
    // popup "menu".
    cef_popup::render_if_visible(&SLOT, tint_rgba, 0.0, cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

pub fn debug_dump_backbuffer(path: &Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// Drapeau « un réglage vient de redémarrer `explorer.exe`, il faut
/// ré-inscrire les AppBars » — `app::run` l'interroge à chaque frame et
/// appelle `Window::reregister_appbars` quand il est vrai (voir le site
/// d'appel : sans ça, la réservation d'espace écran du dock est effacée par
/// le redémarrage et les fenêtres maximisées ne s'adaptent plus — BUG RÉEL
/// SIGNALÉ).
///
/// **Actuellement jamais armé** : seuls la teinte de la barre des tâches et
/// son réglage de taille l'armaient, et ces deux réglages ont été retirés
/// (ils ne fonctionnaient pas de façon fiable). Le drapeau et son site
/// d'appel sont CONSERVÉS volontairement : le mécanisme qu'ils protègent
/// reste nécessaire dès qu'une fonctionnalité redémarre `explorer.exe`, et
/// le supprimer maintenant ne changerait aucun comportement tout en retirant
/// le point d'accroche.
pub fn take_native_tint_restart_pending() -> bool {
    NATIVE_TINT_RESTART_PENDING.with(|p| p.take())
}

/// Voir `AIDE_DEMANDEE`.
pub fn take_aide_demandee() -> Option<String> {
    AIDE_DEMANDEE.with(|a| a.borrow_mut().take())
}

/// À appeler à CHAQUE frame tant que ce panneau est visible (voir `app::run`,
/// juste après `win32::settings_dash::render_if_visible` — les deux
/// panneaux peuvent coexister, chacun mutant les MÊMES variables locales).
/// Vérifie la pipette, applique les actions reçues, PUIS repousse l'état
/// complet — voir la doc de module sur pourquoi à chaque frame plutôt qu'un
/// suivi de changement fin.
pub fn poll(refs: &mut SettingsRefs) {
    // AVANT le test de visibilité : la bascule vient de l'applet, Réglages
    // fermés le plus souvent (voir `follow_windows_toggle`).
    if let Some(light) = WINDOWS_THEME_REQUEST.with(Cell::take) {
        apply_windows_theme(refs, light);
    }
    suivre_le_fond_d_ecran(refs);
    if !cef_popup::is_visible(&SLOT) {
        return;
    }
    if let Some(target) = EYEDROPPER_ACTIVE.with(|e| e.borrow().clone()) {
        if let Some(rgb) = EYEDROPPER_RESULT.with(Cell::take) {
            apply_color_to_target(&target, rgb, refs);
            stop_eyedropper();
        } else {
            let escape_pressed = unsafe { (GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16) & 0x8000 != 0 };
            if escape_pressed {
                stop_eyedropper();
            }
        }
    }
    dispatch_actions(refs);
    push_state(refs);
}

/// **PERSISTE TOUT L'ÉTAT DES ZONES, EN UNE FOIS.**
///
/// Depuis que `zones.toml` remplace les six anciens fichiers, une seule écriture suffit — et
/// **c'est le point** : aucune ne peut être oubliée. La classe « frère oublié » a été frappée
/// deux fois ici même (cinq fichiers de zone, puis `dash_add_zone`), chaque fois parce qu'un
/// écrivain ne connaissait qu'une PARTIE de l'état. Un seul fichier rend la classe
/// structurellement impossible, pas seulement son occurrence du jour.
fn persist_zones(dock: &crate::ui::dock::Dock) {
    config::save_zones(&config::zones_a_ecrire(dock));
}

/// **RÉSOUT UNE CLÉ VENUE DE LA PAGE EN INDEX DU DOCK VIVANT** — `None` si le réglage est
/// périmé (barre réordonnée depuis l'affichage du panneau, zone disparue).
///
/// C'est le SEUL instant où la résolution est vraie : l'index d'un espace dépend de la
/// disposition COURANTE, et la page ne connaît que des clés stables (voir `ui::zones`).
fn zone_index(dock: &crate::ui::dock::Dock, text: &str) -> Option<usize> {
    let key = crate::ui::zones::ZoneKey::parse(text)?;
    dock.spacer_index_at_rank(dock.orders(), key)
}

/// **TOML → JSON pour la page** — le rapport de migration est écrit en TOML (il porte des NOMS
/// d'utilisateur, voir `config::ZONES_MIGRATION_FILE`) et la page, elle, ne parle que JSON.
fn toml_to_json(value: &toml::Value) -> serde_json::Value {
    use serde_json::Value as Json;
    match value {
        toml::Value::String(text) => Json::String(text.clone()),
        toml::Value::Integer(number) => Json::from(*number),
        toml::Value::Float(number) => Json::from(*number),
        toml::Value::Boolean(flag) => Json::from(*flag),
        toml::Value::Datetime(date) => Json::String(date.to_string()),
        toml::Value::Array(items) => Json::Array(items.iter().map(toml_to_json).collect()),
        toml::Value::Table(table) => {
            Json::Object(table.iter().map(|(key, value)| (key.clone(), toml_to_json(value))).collect())
        }
    }
}

/// **RETIRER UNE ÉPINGLE DU DOCK, PAR SON NOM** — fichier, décalage du bloc des
/// épingles, ordre, puis retrait À CHAUD (rechargement seulement en repli).
/// Partagé par les Réglages (`launcher-remove`) et
/// le Dash (`cef_dash`, « Retirer du dock ») : c'est la partie délicate, et deux
/// copies finiraient par diverger.
///
/// Rend `false` — sans rien avoir touché — si l'épingle n'est pas dans
/// `dock_pins.txt` (écrite à la main dans `config.toml`, que l'application ne
/// réécrit jamais) : l'appelant ne doit pas annoncer un retrait qui n'a pas eu lieu.
pub fn retirer_epingle(dock: &mut Dock, name: &str) -> bool {
    // **LE RANG DE L'ÉPINGLE, LU AVANT DE LA RETIRER** : c'est lui qui
    // décide de ce qui se décale, et il n'est calculable qu'avant. La
    // liste est celle dont `Dock::new` se servira au prochain lancement
    // (`config.toml` puis `dock_pins.txt` — voir `load_pinned_apps`).
    let avant = config::load_pinned_apps();
    let epingles = avant.len();
    let rang = avant.iter().position(|p| p.name.eq_ignore_ascii_case(name));
    if config::remove_extra_pin(name) {
        // ⚠️ **LE NOMBRE D'ÉPINGLES A CHANGÉ, DONC LE BLOC DES ÉPINGLES ET
        // DES APPLETS A GLISSÉ D'UN CRAN** : l'arithmétique est
        // `PINNED_ICON_BASE + pinned_apps.len()` (voir `app::run`), et les
        // quatre ordres citent des index ABSOLUS. Sans ce décalage,
        // l'entrée qui voulait dire « alimentation » désignerait
        // « terminal » après le rechargement — la classe de défaut que
        // `insert_spacer` documente (« elles désignent l'icône VOISINE »).
        // `decaler_le_bloc_des_epingles` porte la démonstration, et
        // `winpin_base` la borne : au-delà, plus rien ne bouge.
        if let Some(rang) = rang {
            // ⚠️ **LA BORNE VIENT DU DOCK, PLUS D'UN FICHIER** — changé
            // le 2026-09-29 (étape 2 du refactor des identités stables,
            // voir `docs/plans/dock-identites-stables.md`). Elle était lue
            // dans `winpin_base.txt`, écrit par `app.rs` à chaque
            // démarrage : une valeur qui transitait par le disque pour
            // arriver ici, et qui pouvait donc être PÉRIMÉE. Le dock la
            // connaît — c'est lui qui alloue la plage.
            //
            // ⚠️ **ET C'EST BIEN LE MÊME DOCK** : `cef_settings` reçoit le
            // dock PRIMAIRE (voir `app.rs`, `SettingsRefs { dock: &mut
            // dock, … }`), donc celui dont la base était écrite dans le
            // fichier. La substitution est équivalente — vérifié, et non
            // supposé, parce qu'un dock SECONDAIRE, lui, a ses propres
            // bases et aurait rendu ce remplacement faux en silence.
            //
            // `None` = plage non allouée ; on ne DEVINE pas la borne pour
            // autant : un décalage trop large déplacerait les zones, et un
            // décalage faux est pire que rien.
            //
            // ⭐ **DEPUIS LA RÉSERVE DES ÉPINGLES (2026-10-10), SEULES LES ÉPINGLES
            // SUIVANTES GLISSENT** : les applets valent `PINNED_ICON_BASE +
            // capacité + k`, et retirer une épingle ne change pas la capacité — elle
            // libère une place. La borne est donc la fin du bloc des épingles, plus
            // `winpin_base` : les applets ne bougent pas. Voir `ui::reserve_epingles`.
            let capacite = crate::app::capacite_epingles();
            dock.decaler_le_bloc_des_epingles(
                crate::app::PINNED_ICON_BASE + rang,
                crate::app::PINNED_ICON_BASE + capacite,
                true,
            );
            // ⭐ **ET LE RETRAIT À CHAUD** (2026-10-10, « fais le retrait d'une
            // épingle à chaud aussi ») : les icônes suivent le décalage des ordres,
            // et la place retirée redevient libre. `app::run` retire ensuite le rang
            // de ses listes parallèles (`signaler_epingles_changees`).
            if dock.retirer_une_epingle(rang, epingles, capacite) {
                config::save_arrangement(dock);
                crate::app::signaler_epingles_changees();
                crate::diag!("launcher-remove : epingle {name} retiree a chaud (rang {rang})");
                return true;
            }
            crate::diag!("launcher-remove : retrait a chaud refuse (rang {rang}, {epingles} epingles, capacite {capacite})");
        }
        // **L'ORDRE EST ÉCRIT AVANT LE RECHARGEMENT, ET L'ORDRE DES DEUX
        // OPÉRATIONS COMPTE** : le dock sera RECONSTRUIT à partir des
        // fichiers (voir `request_reload`), donc un décalage resté en
        // mémoire serait purement perdu.
        config::save_arrangement(dock);
        crate::diag!("launcher-remove : epingle {name} retiree de dock_pins.txt");
        // ⚠️ **LE DOCK NE RELIT SES ÉPINGLES QU'À SA CONSTRUCTION.** Sans ce
        // rechargement, l'icône resterait à l'écran et l'action semblerait
        // n'avoir rien fait — c'est le motif exact du Dash après un ajout
        // (voir `cef_dash::handle_add_to_zone`, qui porte le commentaire).
        crate::app::request_reload();
        true
    } else {
        crate::diag!(
            "launcher-remove : {name} n'est pas dans dock_pins.txt (rien retire — ecrite a la main ?)"
        );
        false
    }
}

/// Écrit les quatre réglages du zoom du logo, tels que le module les a BORNÉS.
fn save_logo_zoom() {
    use crate::ui::dock;
    config::save_logo_zoom(dock::logo_zoom_enabled(), dock::logo_zoom_amplitude(), dock::logo_zoom_ms(), dock::logo_zoom_interval_ms());
}

fn dispatch_actions(refs: &mut SettingsRefs) {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(rest) = action.strip_prefix("settings/") else {
            eprintln!("[neuradesk][cef] action réglages inconnue : {action:?}");
            continue;
        };
        let parts: Vec<&str> = rest.split('/').collect();
        match parts.as_slice() {
            ["close"] => cef_popup::fermer_par_geste(&SLOT),
            // ---- SAUVEGARDE DE LA CONFIGURATION (voir `sauvegarde_config`) ----
            ["config-exporter"] => exporter_la_configuration(crate::sauvegarde_config::Sorte::Complete),
            ["config-exporter-apparence"] => exporter_la_configuration(crate::sauvegarde_config::Sorte::Apparence),
            ["config-importer"] => preparer_l_import(),
            ["config-importer-confirmer"] => confirmer_l_import(),
            ["config-importer-annuler"] => {
                if let Some(dossier) = config::config_dir() {
                    crate::sauvegarde_config::annuler_import(&dossier);
                }
                SAUVEGARDE_APERCU.with(|a| *a.borrow_mut() = None);
                SAUVEGARDE_MESSAGE.with(|m| *m.borrow_mut() = "Import annulé : rien n'a changé.".to_string());
            }
            // ---- RACCOURCIS DE DOSSIERS DE LA BARRE DU BAS --------------------
            //
            // ⚠️ **LA CLÉ EST LE CHEMIN, JAMAIS UN INDEX** — même règle que
            // `ZoneRow::arg` : la page ne doit pas connaître de position, qui
            // change dès qu'on retire une entrée. Le chemin arrive
            // PERCENT-ENCODÉ (la page encode chaque segment), donc on le décode
            // ICI, comme les autres bras qui reçoivent du texte de l'utilisateur.
            ["dossier-ajouter"] => {
                let Some(hwnd) = dialogue_possible() else { return };
                let Some(chemin) = crate::win32::file_dialog::choisir_dossier(
                    hwnd,
                    "Choisir un dossier à épingler à la barre du bas",
                ) else {
                    return; // Annuler n'est pas une erreur : rien à journaliser.
                };
                let Some(mut dossier) = config::DossierEpingle::depuis_chemin(chemin) else { return };
                // **MÊME GÉNÉRATION QUE LA BARRE** (voir `app::vignette_generee_pour`) : un
                // dossier ajouté ici montre lui aussi l'icône de SON dossier, et non le
                // repli glyphe/initiale. Un seul chemin de création de vignette pour les
                // trois gestes qui en créent une.
                dossier.vignette = crate::app::vignette_generee_pour(&dossier.chemin);
                // ⚠️ **LE RANG SE PREND APRÈS L'AJOUT** : le nouveau dossier EST le
                // dernier, donc son rang parmi les dossiers est `nombre - 1` (voir
                // `app::basculer_epinglage_du_dossier_affiche`, même règle).
                refs.dossiers.push(config::ElementBarre::Dossier(dossier));
                let rang = crate::app::nombre_de_dossiers(refs.dossiers) - 1;
                if !dossiers_ecrits(refs, crate::app::GesteDossier::Ajout(rang)) {
                    refs.dossiers.pop();
                }
            }
            // ---- LE DOSSIER PERSONNEL DU DASH DOSSIER --------------------------
            //
            // C'est le SEUL réglage de cette page qui n'appartient pas à la barre
            // du bas, mais à un panneau : le dossier sur lequel s'ouvre la moitié
            // droite du dash dossier (voir `config::load_sous_dock_personnel`).
            //
            // ⚠️ **LE MÊME SÉLECTEUR NATIF QUE `dossier-ajouter`**, et pour la
            // même raison : la page n'a aucun accès au disque, et un champ de
            // texte demanderait à l'utilisateur de taper un chemin juste.
            // ⚠️ Le dialogue est MODAL et gèle le redessin du dock pendant qu'il
            // est ouvert — assumé, documenté dans `win32::file_dialog`.
            ["dossier-personnel-choisir"] => {
                let Some(hwnd) = dialogue_possible() else { return };
                let Some(chemin) = crate::win32::file_dialog::choisir_dossier(
                    hwnd,
                    "Choisir le dossier personnel du dash dossier (la moitié droite)",
                ) else {
                    return; // Annuler n'est pas une erreur : rien à journaliser.
                };
                // ⚠️ **AUCUNE VALIDATION ICI, ET C'EST VOULU** : le chemin vient
                // du sélecteur NATIF, donc il existe et c'est un dossier. Le
                // revalider ferait diverger les deux règles le jour où l'une
                // bougerait. `load_sous_dock_personnel` écarte de toute façon une
                // valeur devenue invalide.
                if !config::save_sous_dock_personnel(&chemin) {
                    // On le DIT plutôt que de laisser croire à un réglage retenu :
                    // la page affichera l'ancien chemin, et sans cette ligne
                    // personne ne saurait que l'écriture a échoué.
                    crate::diag!("[reglages] dossier personnel REFUSE : ecriture impossible");
                } else {
                    // ⛔ **SANS CET OUBLI, LE RÉGLAGE SEMBLERAIT NE PAS PRENDRE.**
                    // `cef_sous_dock::dossier_personnel` résout le chemin UNE fois
                    // et le garde (il est relu à chaque frame ici même) : tant
                    // qu'on ne le lui dit pas, il rend l'ancienne valeur, et
                    // l'écran continue d'afficher celle d'avant — sans un mot.
                    crate::win32::cef_sous_dock::oublier_dossier_personnel();
                }
            }
            // **RÉTABLIR LE PROFIL** — le bouton qui remet le défaut.
            //
            // ⚠️ **IL EFFACE LE FICHIER, IL N'Y ÉCRIT PAS LE PROFIL.** Les deux se
            // ressemblent aujourd'hui et ne se ressembleront plus : écrire le
            // profil fige la valeur du moment, alors que l'absence de fichier
            // suit le profil si l'utilisateur le déplace. C'est aussi ce que
            // `dossier_personnel_est_le_profil` relit.
            ["dossier-personnel-defaut"] => {
                if let Some(chemin) = config::config_path("sous_dock_personnel.txt") {
                    if let Err(e) = std::fs::remove_file(&chemin) {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            crate::diag!("[reglages] dossier personnel : effacement impossible : {e}");
                        }
                    }
                }
                // Même raison que le bras « choisir », juste au-dessus : sans cet
                // oubli, l'écran continuerait de montrer le chemin qu'on vient
                // d'effacer.
                crate::win32::cef_sous_dock::oublier_dossier_personnel();
            }
            ["dossier-retirer", chemin] => {
                let chemin = percent_decode(chemin);
                let cible = std::path::PathBuf::from(&chemin);
                // ⚠️ **LE RANG SE PREND AVANT LE RETRAIT** : après, il n'existe plus (voir
                // `GesteDossier`). Un `retain`, qui dit « lesquels garder », ne le donne pas —
                // et c'est précisément pourquoi la forme d'origine ne pouvait pas suivre.
                let Some(position) = crate::app::emplacement_du_dossier(refs.dossiers, &cible) else { return };
                // DEUX NUMEROS : la POSITION pour la liste, le RANG parmi les dossiers
                // pour le dock (voir `app::rang_du_dossier`).
                let rang = crate::app::rang_du_dossier(refs.dossiers, position);
                let retire = refs.dossiers.remove(position);
                if !dossiers_ecrits(refs, crate::app::GesteDossier::Retrait(rang)) {
                    refs.dossiers.insert(position, retire);
                }
            }
            // ⛔ **DEUX VERBES, ET PAS UN SEUL AVEC UN NOM FACULTATIF** : `go(…)`
            // encode chaque segment et les joint par `/`, or un DERNIER segment
            // VIDE disparaît (`trim_end_matches('/')` côté `cef_app`) — vider le
            // champ pour revenir au nom du dossier enverrait donc
            // `["dossier-renommer", chemin]`, qui ne correspond à AUCUN bras et
            // échouerait EN SILENCE. C'est la règle documentée du dépôt : jamais
            // `""` en dernier segment. On nomme donc l'effacement.
            ["dossier-renommer", chemin, nom] => {
                let chemin = percent_decode(chemin);
                let nom = percent_decode(nom);
                let cible = std::path::PathBuf::from(&chemin);
                // ⚠️ **LE RANG, ET PAS LA RÉFÉRENCE** : `GesteDossier::Remplacement` porte un
                // rang, et `iter_mut().find()` n'en donne pas — c'est ce qui oblige à passer
                // par `position`, et c'est voulu (le rang est ce que la barre comprend).
                let Some(position) = crate::app::emplacement_du_dossier(refs.dossiers, &cible) else { return };
                let rang = crate::app::rang_du_dossier(refs.dossiers, position);
                let Some(ancien) = crate::app::dossier_mutable(refs.dossiers, position).map(|d| d.nom.clone()) else { return };
                if let Some(dossier) = crate::app::dossier_mutable(refs.dossiers, position) {
                    dossier.nom = Some(nom.trim().to_string());
                }
                if !dossiers_ecrits(refs, crate::app::GesteDossier::Remplacement(rang)) {
                    if let Some(dossier) = crate::app::dossier_mutable(refs.dossiers, position) {
                        dossier.nom = ancien;
                    }
                }
            }
            // Efface l'étiquette : le dossier reprend alors son nom déduit du
            // chemin (voir `DossierEpingle::nom_affiche`), plutôt que de rester
            // sans rien.
            ["dossier-renommer-effacer", chemin] => {
                let chemin = percent_decode(chemin);
                let cible = std::path::PathBuf::from(&chemin);
                let Some(position) = crate::app::emplacement_du_dossier(refs.dossiers, &cible) else { return };
                let rang = crate::app::rang_du_dossier(refs.dossiers, position);
                let Some(ancien) = crate::app::dossier_mutable(refs.dossiers, position).map(|d| d.nom.clone()) else { return };
                if let Some(dossier) = crate::app::dossier_mutable(refs.dossiers, position) {
                    dossier.nom = None;
                }
                // ⚠️ **`Remplacement` MÊME SI SEUL LE NOM CHANGE** : le repli d'un dossier
                // sans vignette porte l'INITIALE du nom quand la police d'icônes manque
                // (voir `app::texture_de_remplacement_de_dossier`) — renommer peut donc
                // changer ce qui est DESSINÉ, et c'est `remplacer` qui le recalcule.
                if !dossiers_ecrits(refs, crate::app::GesteDossier::Remplacement(rang)) {
                    if let Some(dossier) = crate::app::dossier_mutable(refs.dossiers, position) {
                        dossier.nom = ancien;
                    }
                }
            }
            ["dossier-vignette", chemin] => {
                let chemin = percent_decode(chemin);
                let Some(hwnd) = dialogue_possible() else { return };
                let Some(image) = crate::win32::file_dialog::ouvrir(
                    hwnd,
                    "Choisir une vignette pour ce dossier",
                    IMAGE_EXTENSIONS_DOSSIER,
                    "Images",
                ) else {
                    return;
                };
                let Some(vignette) = config::store_dossier_vignette(&image) else {
                    // Illisible, ou %APPDATA% inaccessible : on ne touche pas à
                    // la vignette existante plutôt que de la perdre pour rien.
                    crate::diag!("[reglages] vignette de dossier refusee : {image:?}");
                    return;
                };
                let cible = std::path::PathBuf::from(&chemin);
                let Some(position) = crate::app::emplacement_du_dossier(refs.dossiers, &cible) else { return };
                let rang = crate::app::rang_du_dossier(refs.dossiers, position);
                let Some(ancienne) = crate::app::dossier_mutable(refs.dossiers, position).map(|d| d.vignette.clone()) else { return };
                if let Some(dossier) = crate::app::dossier_mutable(refs.dossiers, position) {
                    dossier.vignette = Some(vignette);
                }
                // **REMISE EN PLACE DE LA VIGNETTE PRÉCÉDENTE SI L'ÉCRITURE ÉCHOUE** — sans
                // elle, la liste décrirait une vignette que le fichier ne porte pas, et le
                // lancement suivant la perdrait sans que rien ne l'explique.
                if !dossiers_ecrits(refs, crate::app::GesteDossier::Remplacement(rang)) {
                    if let Some(dossier) = crate::app::dossier_mutable(refs.dossiers, position) {
                        dossier.vignette = ancienne;
                    }
                }
            }
            // **ZONE DE DESTINATION DES AJOUTS DEPUIS LE DASH** — voir
            // `cef_dash::handle_add_to_zone`. Tout ce qui ne s'analyse pas comme
            // une **CLÉ** (`"D3"`) vaut « aucune » : c'est ainsi que l'entrée
            // « Aucune » de la liste DÉSACTIVE la fonction, sans code
            // d'exception.
            //
            // ⚠️ **LA CLÉ VIT DANS `zones.toml`**, plus dans `dash_add_zone.txt` —
            // ce dernier est RENOMMÉ par la migration, et l'écrire encore ici
            // ferait diverger les deux sources. C'est le motif « on écrit avant
            // que les refus aient parlé » appliqué à une source périmée.
            ["dash-add-zone", value] => {
                let mut zones = config::zones_a_ecrire(&refs.dock);
                zones.dash_add_zone = crate::ui::zones::ZoneKey::parse(value);
                config::save_zones(&zones);
            }
            // **L'ACQUITTEMENT DU RAPPORT DE MIGRATION** — le bouton « Compris » de
            // la bannière en tête de la section Zones. Le fichier DISPARAÎT (voir
            // `config::clear_zones_migration`) : un rapport qui survivrait à son
            // acquittement se réafficherait au lancement suivant, indéfiniment.
            // **LES DEUX SONS DE RAPPEL** — un par famille. DEMANDE EXPLICITE
            // (2026-09-24) : « j'aimerais pouvoir choisir les alarmes pour les
            // rappels de notes et les rappels de dates ». Le réglage vit ICI et
            // plus dans le panneau Notes : un seul endroit pour les deux.
            ["son-notes", index] => choisir_son(config::FamilleRappel::Notes, index),
            ["son-dates", index] => choisir_son(config::FamilleRappel::Dates, index),
            // L'ÉCOUTE — le bouton « ▶ » de chaque ligne (voir `ecouter_son`, qui
            // porte le pourquoi de ses deux règles). Deux segments et non trois :
            // le rang désigne le fichier, il n'y a qu'une seule liste de sons.
            ["ecouter-son", index] => ecouter_son(index),
            ["zones-migration-ack"] => {
                config::clear_zones_migration();
                push_state(refs);
            }
            ["auto-color-scheme", flag] => {
                *refs.auto_color_scheme = *flag == "1";
                config::save_auto_color_scheme(*refs.auto_color_scheme);
            }
            // "Visible dans les captures d'écran" (voir `gfx::
            // capture_visibility`, DEMANDE EXPLICITE) — état tenu dans
            // l'`AtomicBool` global de ce module plutôt que dans
            // `SettingsRefs` (contrairement à `auto_color_scheme` ci-dessus) :
            // consulté aussi depuis la création des popups CEF (potentiellement
            // hors du thread principal, voir la doc de module), donc pas un
            // simple `&mut bool` local à `app::run` comme les autres. La
            // réapplication `SetWindowDisplayAffinity` aux fenêtres déjà
            // ouvertes a lieu dans la boucle principale (`app::run`, voir
            // `last_applied_capture_visible`), pas ici.
            // Après une capture : sa fenêtre d'actions, ou l'éditeur (voir
            // `win32::cef_capture`).
            ["capture-carte", flag] => crate::win32::cef_capture::activer(*flag == "1"),
            ["capture-visible", flag] => {
                let enabled = *flag == "1";
                crate::gfx::capture_visibility::set_enabled(enabled);
                config::save_bool_setting("capture_visible", enabled);
            }
            // Clignotement de l'indicateur à la notification (voir
            // `win32::notification`, doc de module) — même forme que
            // `capture-visible` ci-dessus : un état tenu par le module lui-même
            // plutôt que dans `SettingsRefs`, donc rien d'autre à faire ici que
            // le basculer et le persister.
            // Dégradé animé (voir `gfx::degrade`) — même forme que
            // `capture-visible` : l'état vit sur le module.
            ["degrade", flag] => {
                let enabled = *flag == "1";
                crate::gfx::degrade::set_enabled(enabled);
                config::save_bool_setting("degrade_anime", enabled);
            }
            ["degrade-accent", flag] => {
                let enabled = *flag == "1";
                crate::gfx::degrade::set_accent_enabled(enabled);
                config::save_bool_setting("degrade_accent", enabled);
            }
            ["degrade-mode", nom] => {
                let nom = percent_decode(nom);
                if crate::gfx::degrade::set_mode(&nom) {
                    config::save_degrade_mode(&nom);
                }
            }
            // ---- LES MOTIFS DE L'UTILISATEUR (voir `gfx::shader_utilisateur`) ----
            ["degrade-shader", nom] => crate::gfx::shader_utilisateur::choisir(&percent_decode(nom)),
            ["degrade-shaders-dossier"] => {
                if let Some(dossier) = crate::gfx::shader_utilisateur::chemin_du_dossier() {
                    crate::win32::appicon::open(&dossier.to_string_lossy());
                }
            }
            ["degrade-preset", nom] => {
                if crate::gfx::degrade::set_preset(&percent_decode(nom)) {
                    crate::gfx::degrade::sauver_couleurs();
                }
            }
            // ⭐ OÙ SE POSE LE BANDEAU DE NOTIFICATION (voir
            // `win32::bandeau::Position`) — même motif que ci-dessus : la page
            // envoie un NOM, `percent_decode` le rend, et le module enregistre.
            // ⚠️ **Un nom inconnu n'écrit RIEN** : un fichier portant un nom que
            // le code ne connaît pas serait un réglage muet.
            ["position-bandeau", nom] => {
                crate::win32::bandeau::set_position(&percent_decode(nom));
            }
            // Un curseur, en CENTIÈMES (les curseurs de la page sont entiers).
            ["degrade-reglage", nom, centiemes] => {
                if let Ok(v) = centiemes.parse::<f32>() {
                    if crate::gfx::degrade::set_reglage(nom, v / 100.0) {
                        config::save_degrade_reglages(&crate::gfx::degrade::valeurs_reglages());
                    }
                }
            }
            ["degrade-hasard"] => {
                crate::gfx::degrade::tirer_palette();
                crate::gfx::degrade::sauver_couleurs();
            }
            ["degrade-reglages-defaut"] => {
                crate::gfx::degrade::reglages_par_defaut();
                config::save_degrade_reglages(&crate::gfx::degrade::valeurs_reglages());
            }
            // « Curseurs au hasard » / « Tout au hasard » (motif et palette
            // en plus) / « Origine au hasard ».
            ["degrade-tirer", quoi] => {
                match *quoi {
                    "origine" => crate::gfx::degrade::tirer_origine(),
                    quoi => crate::gfx::degrade::tirer_reglages(quoi == "tout"),
                }
                crate::gfx::degrade::sauver_tout();
            }
            // Sauvegardes nommées (voir `gfx::degrade_sauvegardes`) — le nom
            // arrive encodé (`encodeURIComponent`).
            ["degrade-sauver", nom] => {
                crate::gfx::degrade_sauvegardes::sauver(&percent_decode(nom));
            }
            ["degrade-charger", nom] => {
                if crate::gfx::degrade_sauvegardes::appliquer(&percent_decode(nom)) {
                    crate::gfx::degrade::sauver_tout();
                }
            }
            ["degrade-supprimer", nom] => {
                crate::gfx::degrade_sauvegardes::supprimer(&percent_decode(nom));
            }
            ["notification-blink", flag] => {
                let enabled = *flag == "1";
                crate::win32::notification::set_enabled(enabled);
                config::save_bool_setting("notification_blink", enabled);
            }
            // LES NOTIFICATIONS DE WINDOWS SOUS LA BARRE (voir
            // `win32::toast_veille` et `win32::toast_natif`) — c'est le MODULE
            // qui enregistre le réglage ET l'applique à chaud : activer installe
            // le crochet qui déplace le vrai toast, désactiver le laisse en bas
            // à droite. Rien n'est écrit dans Windows. Rien ne va dans
            // `SettingsRefs`.
            ["toasts-horloge", flag] => {
                crate::win32::toast_veille::set_enabled(*flag == "1");
            }
            // ⭐ LA DURÉE DES NOTIFICATIONS (voir `win32::toast_duree`) —
            // DEMANDE EXPLICITE (2026-11-02). `0` : comme Windows. ⚠️ À 5 s ou
            // plus, le MODULE pose aussi le réglage GLOBAL de Windows. Une
            // valeur hors de la liste est refusée par le module, sans écriture.
            ["toast-duree", v] => {
                let accepte = v.parse::<u32>().is_ok_and(crate::win32::toast_duree::choisir);
                if !accepte {
                    crate::diag!("[toast-duree] valeur refusee : {v:?}");
                }
            }
            // ⭐ LA VILLE DE LA MÉTÉO DU MENU DE L'HORLOGE — DEMANDE EXPLICITE
            // (2026-10-14). Le segment est ENCODÉ (accents, espaces). Vide : la
            // météo quitte le menu.
            // ⭐ VERR. NUM / VERR. MAJ (voir `ui::verrous`) — les couleurs passent
            // par `apply_color_to_target` (cibles `verr_num`, `verr_maj`).
            ["debit-affiche", v] => {
                crate::ui::debit::set_affiche(*v == "1");
                config::save_debit_affiche(*v == "1");
            }
            ["verrous-affiche", v] => {
                let r = crate::ui::verrous::Reglages { affiche: *v == "1", ..crate::ui::verrous::reglages() };
                crate::ui::verrous::set_reglages(r);
                config::save_verrous(r);
            }
            ["meteo-horloge", nom] => {
                crate::win32::meteo_horloge::changer_ville(&percent_decode(nom));
            }
            ["meteo-horloge"] => {
                crate::win32::meteo_horloge::changer_ville("");
            }
            // Les horloges du monde (voir `win32::horloges_monde`) — même motif :
            // l'état vit sur le module.
            ["horloge-monde-ajouter", zone] => {
                crate::win32::horloges_monde::ajouter(&percent_decode(zone));
            }
            ["horloge-monde-retirer", rang] => {
                if let Ok(rang) = rang.parse() {
                    crate::win32::horloges_monde::retirer(rang);
                }
            }
            ["horloge-monde-monter", rang] | ["horloge-monde-descendre", rang] => {
                if let Ok(r) = rang.parse() {
                    crate::win32::horloges_monde::deplacer(r, parts[0] == "horloge-monde-monter");
                }
            }
            // Miroir de la ZONE DE NOTIFICATION (voir `win32::tray_icons`) — même
            // motif que ci-dessus : l'état vit sur le module, rien dans
            // `SettingsRefs`. La case le bascule À CHAUD : la boucle relit
            // `is_enabled()` à chaque frame.
            // LES COINS ARRONDIS DE LA ZONE PRINCIPALE (voir `ui::dock::conges`) —
            // à chaud : le dessin et la région relisent l'option à chaque frame.
            // LES COINS NOIRS DES ÉCRANS (voir `win32::coins_ecran`) — appliqués
            // à la synchronisation suivante de la boucle (au plus une seconde).
            ["coins-ecran", flag] => {
                let actifs = *flag == "1";
                crate::win32::coins_ecran::activer(actifs);
                config::save_bool_setting(config::COINS_ECRAN, actifs);
            }
            ["coins-ecran-rayon", px] => {
                if let Ok(px) = px.parse::<f32>() {
                    crate::win32::coins_ecran::set_rayon(px);
                    config::save_coins_ecran_rayon(crate::win32::coins_ecran::rayon());
                }
            }
            // LES LUEURS (voir `gfx::lueur`) — relues par le shader à chaque frame.
            ["lueur", flag] => {
                let active = *flag == "1";
                crate::gfx::lueur::modifier(|s| s.active = active);
            }
            // L'ÉDITEUR des lueurs (voir `win32::cef_lueurs`) : ouvert par la boucle,
            // qui seule a le device et le rectangle du dash.
            ["lueurs-editeur"] => crate::win32::cef_lueurs::demander_ouverture(),
            // L'ÉDITEUR des textures (voir `win32::cef_textures`) : même chemin.
            ["textures-editeur"] => crate::win32::cef_textures::demander_ouverture(false),
            ["textures-editeur-terminaux"] => crate::win32::cef_textures::demander_ouverture(true),
            // Choisir un EFFET enregistré l'allume (DEMANDE EXPLICITE, 2026-10-14 :
            // « les activer depuis la visibilité dans les captures »).
            ["lueurs-effet", nom] => {
                crate::win32::lueurs_effets::charger(&percent_decode(nom));
            }
            ["coins-secondaires", ecran, flag] => {
                crate::win32::coins_secondaires::activer(&percent_decode(ecran), *flag == "1");
            }
            // Le rayon de ces coins, en px logiques (0 = suivre le style).
            ["coins-secondaires-rayon", ecran, v] => {
                crate::win32::coins_secondaires::regler_rayon(&percent_decode(ecran), v);
            }

            ["coins-zone", flag] => {
                let actifs = *flag == "1";
                crate::ui::dock::activer_coins_zone(actifs);
                config::save_bool_setting(config::COINS_ZONE_PRINCIPALE, actifs);
            }
            ["tray-mirror", flag] => {
                let enabled = *flag == "1";
                crate::win32::tray_icons::set_enabled(enabled);
                config::save_bool_setting("tray_mirror", enabled);
            }
            // LE FLUX DES ICÔNES et sa diffusion (voir `win32::tray_flux`) —
            // écrits seulement : la fenêtre du flux n'est créée qu'au démarrage
            // (`app::run`), et la page le dit.
            ["tray-flux", flag] => config::save_bool_setting("tray_flux", *flag == "1"),
            ["tray-flux-diffusion", flag] => config::save_bool_setting("tray_flux_diffusion", *flag == "1"),
            // ⚠️ **PAR RANG, JAMAIS PAR URL** — l'URL vient de la table Rust.
            ["ouvrir-lien", rang] => crate::win32::outils_applets::ouvrir(rang),
            ["feature", name, flag] => apply_feature(refs.features, name, *flag == "1"),
            // Masquage manuel/automatique des barres — voir la doc de
            // `SettingsRefs::left_bar_hidden`/`right_bar_hidden`/
            // `top_auto_hide`/`left_auto_hide`/`right_auto_hide`/
            // `secondary_auto_hide`, DEMANDE EXPLICITE ("donnes la
            // possibilité à l'utilisateur de masquer les barres droite et
            // gauche ou de rendre les barres... masquable automatiquement",
            // PUIS "activer le masquage automatique de chaque élément de
            // l'interface principale individuellement"). `app::run`
            // applique l'effet réel à chaque frame à partir de ces
            // booléens — rien de plus à faire ici que les persister.
            ["bar-lock", bar, flag] => {
                // VERROU d'UNE barre — DEMANDE EXPLICITE : « une option dans les
                // réglages pour verrouiller les barres contre tout changement
                // pour éviter les erreurs ». PAR BARRE, comme tout le reste de
                // cette section (taille, teinte, masquage).
                //
                // `"top"` désigne la barre du haut ENTIÈRE, ses deux moitiés
                // confondues : c'est une seule barre vue de l'utilisateur, et
                // `Dock::bar_rank` les réunit de la même façon.
                let side = match *bar {
                    "top" => crate::ui::dock::Side::TopLeft,
                    "left" => crate::ui::dock::Side::Left,
                    "bottom" => crate::ui::dock::Side::Bottom,
                    _ => crate::ui::dock::Side::Right,
                };
                refs.dock.set_bar_locked(side, *flag == "1");
                config::save_bar_locks(refs.dock.bar_locks());
            }
            ["zone-label-font", px] => {
                // DEMANDE EXPLICITE : « des curseurs pour régler la taille du nom
                // des zones ». La valeur est bornée par `set_zone_label_font_px`
                // (voir ses bornes), donc un curseur qui irait au-delà n'écrit
                // rien d'absurde.
                match px.parse::<u16>() {
                    Ok(px) => {
                        crate::ui::dock::set_zone_label_font_px(px);
                        config::save_zone_label_font_px(crate::ui::dock::zone_label_font_px());
                    }
                    Err(_) => eprintln!("[neuradesk][cef] taille d'étiquette invalide : {px:?}"),
                }
            }
            // ⚠️⚠️ **LES CINQ ACTIONS DE ZONE REÇOIVENT UNE CLÉ (`"D3"`), PLUS UN INDEX.**
            // La page renvoie ce que `push_state` lui a donné (voir le champ `zones`), et c'est
            // une CLÉ parce qu'un RANG ne bouge pas quand une épingle est ajoutée, tandis qu'un
            // index désignerait un autre espace si le dock changeait entre l'affichage et le clic.
            //
            // La résolution clé → index se fait ICI, sur le dock VIVANT, au moment du geste :
            // c'est le seul instant où elle est vraie. Une clé qui ne se résout plus (barre
            // réordonnée) est SIGNALÉE, jamais appliquée au hasard.
            //
            // ⚠️ **ET LA PERSISTANCE PASSE PAR UN SEUL `persist_zones`** — un seul fichier, donc
            // une seule écriture. C'est la classe « frère oublié » qui disparaît (elle a été
            // frappée deux fois : cinq fichiers de zone, puis `dash_add_zone`), et pas seulement
            // son occurrence du jour.
            ["zone-label", key, flag] => {
                // AFFICHER le numéro/nom EN HAUT de la zone — DEMANDE EXPLICITE :
                // « que le numéro ou le nom de la zone apparaisse en haut de
                // celle-ci si l'utilisateur choisit de le faire depuis les
                // réglages des verrous ». Le texte lui-même est composé à
                // l'affichage (voir `ui::dock::Dock::zone_labels`), jamais
                // stocké : c'est ce qui fait qu'un renommage se voit
                // immédiatement dans la barre.
                match zone_index(&refs.dock, key) {
                    Some(index) => {
                        refs.dock.set_zone_label_shown(index, *flag == "1");
                        persist_zones(&refs.dock);
                    }
                    None => eprintln!("[neuradesk][cef] zone à étiqueter introuvable : {key:?}"),
                }
            }
            ["zone-name", key, encoded] => {
                // RENOMMAGE — DEMANDE EXPLICITE : « permettre de renommer une
                // zone à l'utilisateur et que dans ce cas son nom ne soit plus
                // un nombre ». Le texte arrive ENCODÉ (voir `go`, côté page :
                // `encodeURIComponent` sur CHAQUE segment), donc un nom peut
                // contenir une barre oblique, un dièse ou une espace sans
                // casser le découpage de l'action.
                match zone_index(&refs.dock, key) {
                    Some(index) => {
                        // Un nom VIDE rend son numéro à la zone — c'est ainsi
                        // qu'on la « dé-renomme », et `set_zone_name` écarte
                        // déjà les blancs.
                        let name = crate::win32::cef_app::percent_decode(encoded);
                        refs.dock.set_zone_name(index, Some(&name));
                        persist_zones(&refs.dock);
                    }
                    None => eprintln!("[neuradesk][cef] zone à renommer introuvable : {key:?}"),
                }
            }
            ["zone-lock", key, flag] => {
                // VERROU d'une ZONE depuis les Réglages — DEMANDE EXPLICITE :
                // « les deux, réunis au même endroit ». Le clic droit sur le
                // séparateur continue de le proposer (voir
                // `win32::cef_spacer_size`) : les deux chemins posent le MÊME
                // verrou et écrivent le MÊME fichier, donc ils ne peuvent pas
                // diverger.
                match zone_index(&refs.dock, key) {
                    Some(index) => {
                        refs.dock.set_zone_locked(index, *flag == "1");
                        persist_zones(&refs.dock);
                    }
                    None => eprintln!("[neuradesk][cef] zone à verrouiller introuvable : {key:?}"),
                }
            }
            // LA COTE d'une zone depuis les Réglages — le premier des DEUX
            // réglages du menu du clic droit qui n'existaient QUE là. DEMANDE
            // EXPLICITE : « tous les paramétrages qui sont dans le menu du clic
            // droit sur un séparateur de zone se trouvent groupés dans une
            // section spécifique dans les réglages avec les autres réglages de
            // ces zones ». Même contrat que `zone-lock` ci-dessus : les deux
            // chemins posent la même valeur et écrivent le même fichier, donc
            // ils ne peuvent pas diverger. `set_zone_height` ramène à 0 une
            // valeur nulle, ce qui DÉFAIT la zone — c'est le comportement du
            // geste, pas un cas d'erreur.
            ["zone-cote", key, px] => match (zone_index(&refs.dock, key), px.parse::<f32>()) {
                (Some(index), Ok(px)) => {
                    refs.dock.set_zone_height(index, px);
                    // SONDE — voir `Dock::journaliser_les_espaces` : elle dit sur QUEL espace la clé
                    // s'est résolue, et si cet espace est bien ÉCRIT dans `icon_order.txt`. Un
                    // désaccord entre `[bars.D] spacers` et la ligne d'ordre est le défaut « la zone
                    // ne réagit pas aux réglages ».
                    // SONDE — une LIGNE, et pas le vidage complet : un curseur émet une action PAR
                    // PAS, donc `journaliser_les_espaces` (quatre lignes) noierait le journal. Elle
                    // reste sur les gestes RARES — ajouter, supprimer — qui changent la structure.
                    crate::diag!("zone-cote {key} -> espace {index}, cote {px}");
                    persist_zones(&refs.dock);
                }
                // `eprintln!` NE VA NULLE PART : l'application est lancée détachée (voir
                // `neuradesk-diagnostic-detache`). Un refus muet est ce qui a coûté cette enquête.
                _ => crate::diag!("cote de zone invalide : cle {key:?} NON RESOLUE, valeur {px:?}"),
            },
            // ⚠️ **LES RÉGLAGES DE VALEUR CI-DESSUS ET LES COMMANDES DE STRUCTURE CI-DESSOUS
            // N'ÉCRIVENT PAS LA MÊME CHOSE, ET C'EST DÉLIBÉRÉ.** Une cote, des colonnes, un nom,
            // un verrou ne touchent QUE `zones.toml` : l'ordre ne bouge pas, donc `persist_zones`
            // suffit — et c'est ce qu'il faut, un curseur émettant une action par pas.
            // Ajouter ou retirer une zone, à l'inverse, **change l'ordre** : d'où
            // `config::save_arrangement`, qui écrit les DEUX (voir sa doc — les clés de
            // `zones.toml` sont des RANGS, donc un ordre qui bouge les invalide).
            //
            // AJOUTER une zone — DEMANDE EXPLICITE : « je voudrais dans les réglages pour ajouter
            // et supprimer des zones ». La création EXISTAIT (`Dock::add_zone_separator_at_end`,
            // `add_zone_separator_after`) mais n'était atteignable que par un geste — Maj + clic
            // droit sur une barre — que rien n'annonçait.
            //
            // ⚠️ `insert_spacer` INSÈRE dans `icons` : tous les index suivants glissent d'un cran,
            // et les quatre listes sont décalées en conséquence (voir sa doc). La nouvelle zone
            // reçoit la cote par défaut, comme le geste.
            ["zone-add-at-end"] => {
                let nouvelle = refs.dock.add_zone_separator_at_end(
                    crate::ui::dock::Side::Right,
                    crate::ui::dock::DEFAULT_ZONE_HEIGHT,
                );
                // **AUTANT DE COLONNES QUE LA BARRE EN ACCORDE, ET C'EST LE CAS D'USAGE QUI LE
                // DICTE.** La zone créée ici est la DERNIÈRE de la barre, donc celle qui reçoit le
                // miroir de la notification (la plage vit après tout ce qui est persisté — voir
                // `set_tray_count`). À UNE colonne, ses ~45 entrées demandent ≈ 4300 px de haut :
                // on n'en voit qu'une **bande fine**, et le curseur de cote plafonne bien avant —
                // c'est le défaut signalé, « la zone est écrasée et ne réagit pas ». On demande
                // donc le maximum que la largeur autorise (`effective_zone_columns` le rebornera
                // de toute façon si la barre est rétrécie plus tard, et la DEMANDE est conservée).
                refs.dock.set_zone_columns(nouvelle, crate::ui::dock::Dock::max_zone_columns(*refs.right_width));
                crate::diag!("zone-add-at-end -> espace {nouvelle}");
                refs.dock.journaliser_les_espaces("apres zone-add-at-end");
                config::save_arrangement(&refs.dock);
            }
            ["zone-add-after", key] => match zone_index(&refs.dock, key) {
                Some(index) => {
                    // ⚠️ `add_zone_after_zone` ET NON `add_zone_separator_after` : cette dernière
                    // insère juste après l'index donné, donc lui passer le SÉPARATEUR créerait une
                    // zone vide et verserait le contenu de la zone visée dans la nouvelle. La
                    // première vise le dernier élément de la zone (voir sa doc).
                    match refs.dock.add_zone_after_zone(index, crate::ui::dock::DEFAULT_ZONE_HEIGHT) {
                        Some(nouvelle) => {
                            // **LES COLONNES AUSSI, COMME POUR « EN FIN DE BARRE »** — même raison,
                            // et l'oubli a été constaté à l'usage : la zone naissait à UNE colonne,
                            // donc en **bande fine** dès qu'elle portait du contenu, et il fallait
                            // deviner le réglage. Une zone neuve n'a aucune raison de naître dans
                            // l'état le moins utilisable.
                            refs.dock.set_zone_columns(
                                nouvelle,
                                crate::ui::dock::Dock::max_zone_columns(*refs.right_width),
                            );
                            crate::diag!("zone-add-after {key} (espace {index}) -> espace {nouvelle}");
                        }
                        None => crate::diag!("zone-add-after {key} : la zone {index} n'a pas pu etre ouverte"),
                    }
                    refs.dock.journaliser_les_espaces("apres zone-add-after");
                    config::save_arrangement(&refs.dock);
                }
                None => crate::diag!("zone apres laquelle ajouter : cle {key:?} NON RESOLUE"),
            },
            // SUPPRIMER une zone — l'espace DISPARAÎT (décision explicite), donc `remove_spacer`, et
            // NON une cote ramenée à 0, qui laisserait un blanc ordinaire à la place. Ses icônes
            // rejoignent la zone précédente : la frontière qui les séparait n'est plus là.
            //
            // ⚠️ **`zone_index` RÉSOUT UN RANG D'ESPACE, PAS UNE ZONE** (voir sa doc) : un rang
            // désignant un espace ORDINAIRE se résoudrait sans broncher, et la commande retirerait
            // un espace que l'utilisateur n'a jamais déclaré comme zone. D'où le contrôle.
            // **CACHER UNE ZONE — DEMANDE EXPLICITE (2026-09-23) : « ne puissent pas être
            // supprimées mais seulement cachées ».** Toute sa PLAGE disparaît (voir
            // `ordre_visible`, qui saute le contenu), et rien ne se renumérote : la zone
            // reste un séparateur de zone, donc son RANG ne bouge pas.
            ["zone-hidden", key, flag] => match zone_index(&refs.dock, key) {
                Some(index) if refs.dock.is_zone_separator(index) => {
                    refs.dock.set_icon_hidden(index, *flag == "1");
                    persist_zones(&refs.dock);
                    crate::diag!("zone-hidden {key} -> espace {index}, cache = {}", flag);
                }
                _ => crate::diag!("zone a cacher : cle {key:?} NON RESOLUE ou PAS un separateur"),
            },
            ["zone-remove", key] => match zone_index(&refs.dock, key) {
                Some(index) if refs.dock.icons[index].zone_permanent => {
                    // **REFUS, ET IL EST PRONONCÉ ICI PARCE QUE C'EST ICI QU'ON ÉCRIT.**
                    // La page n'offre déjà plus le bouton (voir `ZoneRow`… non : voir
                    // `ZoneSummary::permanent`), mais une action peut toujours arriver
                    // d'ailleurs — et une zone que l'utilisateur a déclarée permanente ne
                    // doit pas pouvoir disparaître par un chemin qu'on n'a pas prévu.
                    crate::diag!("zone-remove {key} : zone PERMANENTE — refus (cacher, a la place)");
                }
                Some(index) if refs.dock.is_zone_separator(index) => {
                    refs.dock.remove_spacer(index);
                    crate::diag!("zone-remove {key} -> espace {index} retire de l'ordre");
                    refs.dock.journaliser_les_espaces("apres zone-remove");
                    config::save_arrangement(&refs.dock);
                }
                Some(index) => crate::diag!("zone a supprimer : {key:?} -> espace {index}, PAS un separateur"),
                None => crate::diag!("zone a supprimer : cle {key:?} NON RESOLUE"),
            },
            // **RETIRER UN RACCOURCI, DEPUIS LA LISTE DU CONTENU D'UNE ZONE** —
            // DEMANDE EXPLICITE (2026-09-23). Le NOM identifie l'épingle, **jamais un
            // index** : la page n'en connaît aucun (voir `push_state`), et un index
            // désignerait une AUTRE icône dès que le dock change entre l'affichage du
            // panneau et le clic.
            //
            // ⚠️ **L'APPLICATION NE RÉÉCRIT JAMAIS `config.toml`** : une épingle écrite
            // à la main là-dedans n'est pas retirable, et `remove_extra_pin` rend
            // `false` sans rien toucher. Le panneau ne propose d'ailleurs le bouton que
            // là où c'est possible (voir `ZoneRow::action`, et la raison en clair).
            ["launcher-remove", encoded] => {
                // ⚠️ **LE NOM ARRIVE ENCODÉ — MÊME RÈGLE QUE TOUS LES SEGMENTS DE CE
                // PANNEAU** (voir `zone-name` juste au-dessus, qui porte le commentaire
                // d'origine : « le texte arrive ENCODÉ, voir `go` côté page :
                // `encodeURIComponent` sur CHAQUE segment »).
                //
                // L'avoir oublié ICI a coûté un aller-retour : « Character Map »
                // voyage en `Character%20Map`, la comparaison ne trouvait donc RIEN,
                // l'épingle restait en place — et le panneau ne disant rien, l'action
                // semblait ne pas aboutir. DÉFAUT SIGNALÉ, mot pour mot : « quand je
                // clique sur retirer, j'ai une confirmation que je ne peux pas
                // valider ». Le journal portait la preuve (`launcher-remove :
                // Character%20Map n'est pas dans dock_pins.txt`), et c'est elle qui a
                // désigné la cause.
                let name = percent_decode(encoded);
                retirer_epingle(refs.dock, &name);
            }
            // ...et LES COLONNES, le second. Le plafond réel dépend de la largeur
            // de la barre et s'applique à la LECTURE (voir
            // `Dock::effective_zone_columns`) : une valeur trop grande est
            // CONSERVÉE puis ramenée, jamais refusée ici — c'est ce qui permet
            // d'élargir la barre plus tard et de retrouver ses colonnes.
            ["zone-columns", key, columns] => match (zone_index(&refs.dock, key), columns.parse::<u16>()) {
                (Some(index), Ok(columns)) => {
                    refs.dock.set_zone_columns(index, columns);
                    persist_zones(&refs.dock);
                }
                _ => eprintln!("[neuradesk][cef] colonnes de zone invalides : {key:?} {columns:?}"),
            },
            ["bar-hidden", "left", flag] => {
                *refs.left_bar_hidden = *flag == "1";
                config::save_bool_setting("left_bar_hidden", *refs.left_bar_hidden);
            }
            ["bar-hidden", "right", flag] => {
                *refs.right_bar_hidden = *flag == "1";
                config::save_bool_setting("right_bar_hidden", *refs.right_bar_hidden);
            }
            ["bar-hidden", "bottom", flag] => {
                *refs.bottom_bar_hidden = *flag == "1";
                config::save_bool_setting("bottom_bar_hidden", *refs.bottom_bar_hidden);
            }
            ["auto-hide", "bottom", flag] => {
                *refs.bottom_auto_hide = *flag == "1";
                config::save_bool_setting("bottom_auto_hide", *refs.bottom_auto_hide);
            }
            ["auto-hide", "top", flag] => {
                *refs.top_auto_hide = *flag == "1";
                config::save_bool_setting("top_auto_hide", *refs.top_auto_hide);
            }
            ["auto-hide", "left", flag] => {
                *refs.left_auto_hide = *flag == "1";
                config::save_bool_setting("left_auto_hide", *refs.left_auto_hide);
            }
            ["auto-hide", "right", flag] => {
                *refs.right_auto_hide = *flag == "1";
                config::save_bool_setting("right_auto_hide", *refs.right_auto_hide);
            }
            ["auto-hide", "secondary", flag] => {
                *refs.secondary_auto_hide = *flag == "1";
                config::save_bool_setting("secondary_auto_hide", *refs.secondary_auto_hide);
                // Contrairement à `bar-hidden`/`auto-hide primary` (purs
                // drapeaux d'exécution, voir `effective_primary_thicknesses`),
                // ce réglage change la RÉSERVATION AppBar posée à la
                // création de chaque `SecondaryWindow` (voir sa doc et celle
                // de `set_band_reveal` : plus d'espace réservé rendu aux
                // fenêtres maximisées en mode auto-masquage) — il faut donc
                // une reconstruction, comme pour un changement de mode/
                // épaisseur d'écran secondaire.
                *refs.secondary_screens_dirty = true;
            }
            ["theme", "random"] => {
                let theme = color::random_dark_theme();
                apply_theme(refs, theme);
                // La couleur tirée devient la base commune : un curseur bougé
                // ensuite l'échelonne, au lieu de repartir d'une ancienne base.
                with_data(|d| d.theme_base_color = theme);
                config::save_theme_base_color(theme);
                poser_bases_uniformes(theme, None);
            }
            ["theme", "wallpaper"] => match crate::win32::wallpaper_color::extract_accent_color() {
                Some(rgb) => {
                    // Le fond d'écran donne une couleur quelconque : elle est
                    // d'abord ramenée dans la famille sombre, PUIS ajustée.
                    let (saturation, luminosite) = forces_du_theme();
                    let base = color::dark_theme_from_base_color(rgb);
                    let theme = color::ajuster_theme(base, saturation, luminosite);
                    apply_theme(refs, theme);
                    // La couleur ramenée dans la famille sombre (AVANT les forces)
                    // est la base : les curseurs repartent de là.
                    with_data(|d| d.theme_base_color = base);
                    config::save_theme_base_color(base);
                    poser_bases_uniformes(base, None);
                }
                None => eprintln!("[neuradesk][cef] aucune couleur exploitable dans le fond d'écran actuel"),
            },
            // L'axe « style » du système de thèmes (voir `ui::style`) : les
            // FORMES, indépendantes des couleurs choisies ci-dessous. Choisir
            // un style ramène le curseur « Arrondi des menus » à la valeur du
            // style (demande explicite) : sans ça, un rayon réglé une fois
            // masquerait pour toujours celui de tous les styles.
            ["style", id] => {
                if crate::ui::style::choisir(id) {
                    menu::reset_menu_corner_radius();
                    config::clear_menu_corner_radius();
                }
            }
            // La POLICE (voir `ui::style::choisir_police`) : elle remplace celle du
            // style ; un segment VIDE revient à celle du style. ⚠️ Le nom est
            // ENCODÉ par la page (« Segoe UI » porte une espace).
            ["police"] => {
                crate::ui::style::choisir_police("");
            }
            ["police", nom] => {
                crate::ui::style::choisir_police(&percent_decode(nom));
            }
            // La POLICE DES TERMINAUX (voir `ui::style::choisir_police_des_terminaux`) :
            // elle PRÉCÈDE la pile par défaut de la page, elle ne la remplace pas —
            // un segment VIDE revient à cette pile. Le Terminal et les Serveurs SSH
            // la partagent, donc un seul verbe suffit aux deux pages. ⚠️ Le nom est
            // ENCODÉ par la page (« Cascadia Mono » porte une espace).
            // La vitesse du zoom à la molette dans la vue sphère (voir
            // `win32::cef_sphere::vitesse_zoom`), en %.
            ["sphere-zoom", v] => crate::win32::cef_sphere::regler_vitesse_zoom(v),
            // Les réglages PROPRES à la sphère des terminaux : distance
            // d'ouverture, recadrage automatique, fondu du panneau.
            ["sphere-terminaux-zoom", v] => crate::win32::cef_sphere::regler_zoom_defaut(v),
            ["sphere-terminaux-zoom-auto", v] => crate::win32::cef_sphere::regler_zoom_auto(v),
            ["sphere-terminaux-fondu", direction, v] => {
                crate::win32::cef_sphere::regler_fondu(&percent_decode(direction), v);
            }
            // La sphère des PROGRAMMES (voir `win32::cef_fenetres`) : son niveau
            // de zoom par défaut, et le fondu qui lui est PROPRE.
            ["sphere-fenetres-zoom", v] => crate::win32::cef_fenetres::regler_zoom_defaut(v),
            ["sphere-fenetres-vitesse-zoom", v] => crate::win32::cef_fenetres::regler_vitesse_zoom(v),
            ["sphere-fenetres-zoom-auto", v] => crate::win32::cef_fenetres::regler_zoom_auto(v),
            ["sphere-fenetres-fondu", direction, v] => {
                crate::win32::cef_fenetres::regler_fondu(&percent_decode(direction), v);
            }
            ["police-terminal"] => {
                crate::ui::style::choisir_police_des_terminaux("");
            }
            ["police-terminal", nom] => {
                crate::ui::style::choisir_police_des_terminaux(&percent_decode(nom));
            }
            // L'axe « icônes » (voir `ui::icones`) : `app::run` refait les
            // textures des applets à la frame suivante.
            ["icones", id] => {
                crate::ui::icones::choisir(id);
            }
            // "settings/texture-nature/<id>", ".../texture-force/<%>",
            // ".../texture-echelle/<%>" — LA TEXTURE DE L'INTERFACE (voir
            // `gfx::texture_interface`). Les pourcentages arrivent ENTIERS.
            ["texture-nature", id] => {
                if crate::gfx::texture_interface::set_nature_id(id) {
                    config::save_texture_interface();
                }
            }
            ["texture-force", pct] => {
                if let Ok(pct) = pct.parse::<f32>() {
                    crate::gfx::texture_interface::set_force(pct / 100.0);
                    config::save_texture_interface();
                }
            }
            ["texture-echelle", pct] => {
                if let Ok(pct) = pct.parse::<f32>() {
                    crate::gfx::texture_interface::set_echelle(pct / 100.0);
                    config::save_texture_interface();
                }
            }
            // ".../texture-clarte/<-100..100>" et ".../texture-teinte/<0..360>".
            ["texture-clarte", pct] => {
                if let Ok(pct) = pct.parse::<f32>() {
                    crate::gfx::texture_interface::set_clarte(pct / 100.0);
                    config::save_texture_interface();
                }
            }
            // ".../texture-terminaux/<réglage>/<valeur>" — LA TEXTURE DES
            // TERMINAUX (voir `win32::texture_terminaux`), mêmes unités.
            ["texture-terminaux", reglage, valeur] => {
                let t = &crate::gfx::texture_interface::TERMINAUX;
                let nombre = valeur.parse::<f32>().ok();
                let change = match (*reglage, nombre) {
                    ("nature", _) => t.set_nature_id(valeur),
                    ("force", Some(pct)) => { t.set_force(pct / 100.0); true }
                    ("echelle", Some(pct)) => { t.set_echelle(pct / 100.0); true }
                    ("clarte", Some(pct)) => { t.set_clarte(pct / 100.0); true }
                    ("teinte", Some(degres)) => { t.set_teinte(degres); true }
                    _ => false,
                };
                if change {
                    config::save_texture_terminaux();
                }
            }

            ["texture-teinte", degres] => {
                if let Ok(degres) = degres.parse::<f32>() {
                    crate::gfx::texture_interface::set_teinte(degres);
                    config::save_texture_interface();
                }
            }
            ["theme", "windows", "clair"] => apply_famille(refs, "windows", true),
            ["theme", "windows", "sombre"] => apply_famille(refs, "windows", false),
            ["theme", "famille", id, "clair"] => apply_famille(refs, id, true),
            ["theme", "famille", id, "sombre"] => apply_famille(refs, id, false),
            ["theme", "apply", hex] => {
                if let Some(rgb) = parse_hex_rgb(hex) {
                    let (saturation, luminosite) = forces_du_theme();
                    apply_theme(refs, color::ajuster_theme(rgb, saturation, luminosite));
                    // ⚠️ **LA COULEUR CHOISIE DEVIENT LA BASE.**
                    //
                    // ⛔ Sans cette ligne, les deux curseurs de bornes ne savent
                    // plus QUOI régler : leur réapplication part de
                    // `theme_base_color`, et cliquer une pastille de la liste ne
                    // la posait pas. Défaut réel signalé le 2026-10-10 :
                    // « quand je choisis un des thèmes de la liste, comme si les
                    // réglages que j'impose ne savaient pas quoi régler » — le
                    // curseur remplaçait le thème choisi par un autre, dérivé
                    // d'une couleur que l'utilisateur n'avait pas choisie.
                    //
                    // ⚠️ **AUX FORCES NEUTRES, LA PASTILLE PASSE TELLE QUELLE**
                    // (`ajuster_theme` la rend à l'octet près) : les pastilles
                    // sont des thèmes PRÉVUS. Les forces ne la changent que si
                    // l'utilisateur a bougé un curseur — c'est-à-dire s'il l'a
                    // DEMANDÉ.
                    with_data(|d| d.theme_base_color = rgb);
                    poser_bases_uniformes(rgb, None);
                }
            }
            // **LES DEUX FORCES, RÉGLÉES DEPUIS LES RÉGLAGES.**
            //
            // ⚠️ **ON RÉAPPLIQUE LE THÈME AUSSITÔT** : sans cela, bouger un
            // curseur ne changerait rien à l'écran — l'utilisateur devrait
            // relancer, et croirait le réglage mort.
            ["force-theme", nom, valeur] => {
                let Ok(valeur) = valeur.parse::<f32>() else {
                    return eprintln!("[neuradesk][cef] force de theme illisible : {valeur:?}");
                };
                // ⚠️ **LA MÉMOIRE D'ABORD, LE DISQUE ENSUITE** : c'est la mémoire
                // que l'état pousse à chaque frame, et c'est elle qui doit être
                // juste TOUT DE SUITE — le fichier, lui, peut arriver plus tard.
                let force = valeur.clamp(0.0, color::FORCE_MAX);
                match *nom {
                    "saturation" => {
                        let force = force.min(color::FORCE_SATURATION_MAX);
                        with_data(|d| d.theme_force_saturation = force);
                        config::save_theme_force_saturation(force);
                    }
                    "luminosite" => {
                        with_data(|d| d.theme_force_luminosite = force);
                        config::save_theme_force_luminosite(force);
                    }
                    autre => return eprintln!("[neuradesk][cef] force de theme inconnue : {autre:?}"),
                }
                let base = with_data(|d| d.theme_base_color);
                let (saturation, luminosite) = forces_du_theme();
                // ⭐ **L'INSTRUMENT QUI MANQUAIT.** Trois corrections ont ete
                // tentees sur ce reglage SANS QUE PERSONNE VOIE la valeur sur
                // laquelle il agit — on ne pouvait donc pas distinguer « le
                // curseur derive d'une autre couleur » de « le curseur fait bien
                // son travail et l'ecran change ailleurs ». Cette ligne donne la
                // BASE et les BORNES a chaque geste : au premier essai, on saura
                // laquelle des deux moities est fautive.
                crate::diag!(
                    "[reglages] force {nom} -> base {} forces ({saturation:.2}, {luminosite:.2})",
                    hex(base)
                );
                // ⭐ **CHAQUE PANNEAU EST RÉ-ÉCHELONNÉ DEPUIS SA PROPRE BASE** (voir
                // `reappliquer_forces`) — ni `apply_theme` ni `apply_famille`, qui
                // reposeraient la même couleur partout. Cela corrige DEUX défauts :
                // sous une famille (2026-10-12), un curseur éteignait la famille et
                // basculait tout sur un thème libre sombre ; et avec une couleur de
                // mise en avant individuelle (2026-11-06), bouger un curseur effaçait
                // la personnalisation. Les barres/menus suivent la surface commune.
                reappliquer_forces(refs, saturation, luminosite);
            }
            ["theme", "save", hex] => {
                if let Some(rgb) = parse_hex_rgb(hex) {
                    with_data(|d| add_custom_theme(&mut d.custom_themes, rgb));
                }
            }
            ["theme", "delete", index] => {
                if let Ok(index) = index.parse::<usize>() {
                    with_data(|d| remove_custom_theme(&mut d.custom_themes, index));
                }
            }
            // BUG RÉEL CORRIGÉ ("dans couleur des barres pour les écrans
            // secondaires... si j'utilise le sélecteur de couleur je ne
            // peux pas déplacer la zone de sélection") : `target` est une
            // cible DYNAMIQUE (`"secondary:<device_name>:<bar>"`, voir
            // `secondaryScreenColorTarget`/`colorPicker` côté JS) encodée
            // en ENTIER via `encodeURIComponent` avant d'être placée dans
            // l'URL (les deux-points/antislashs d'un `device_name` Windows,
            // ex. `\\.\DISPLAY2`, n'y sont pas valides tels quels) — donc
            // toujours RAW ici (voir la doc de
            // `NeuraDeskRequestHandler::on_before_browse` : jamais décodé
            // avant d'être scindé sur ses vrais séparateurs `/`, ce qui
            // vient d'être fait par `parts.as_slice()` ci-dessus). Sans ce
            // décodage, `target.strip_prefix("secondary:")` dans
            // `apply_color_to_target` ne matchait JAMAIS (le `:` littéral
            // était en réalité `%3A`) : chaque glissé du sélecteur de
            // couleur d'un écran secondaire était silencieusement ignoré
            // (branche "cible de couleur inconnue"), puis écrasé au
            // prochain `push_state` par l'ancienne couleur toujours en
            // vigueur côté Rust — d'où l'impression de curseur verrouillé,
            // qui "revient" dès qu'on relâche. Les cibles FIXES (`"bar_top"`,
            // `"dash"`, ...) n'ont jamais de `%XX` à l'intérieur, donc ce
            // décodage est un no-op pour elles (voir la doc de
            // `percent_decode`) — sans régression pour le reste du panneau.
            ["color", target, hex] => {
                if let Some(rgb) = parse_hex_rgb(hex) {
                    apply_color_to_target(&percent_decode(target), rgb, refs);
                }
            }
            ["windows-accent", target] => {
                let [r, g, b, _a] = theme::detect_accent_color();
                apply_color_to_target(&percent_decode(target), [r, g, b], refs);
            }
            // Même raison que "color"/"windows-accent" ci-dessus — `target`
            // est ensuite stocké RAW dans `EYEDROPPER_ACTIVE` et réutilisé
            // tel quel par `apply_color_to_target` une fois un clic capturé
            // (voir plus haut dans ce fichier, juste avant
            // `dispatch_actions`) : décoder ICI, à l'unique endroit où la
            // valeur RAW existe encore, suffit à corriger aussi ce chemin.
            ["eyedropper", "start", target] => start_eyedropper(&percent_decode(target)),
            ["eyedropper", "stop"] => stop_eyedropper(),
            // 🎲 UN élément au hasard (demande explicite : « mettre la couleur en
            // aléatoire pour chaque élément »). Réutilise tout le routage de
            // `apply_color_to_target`, préfixe `menu:` compris — donc marche pour
            // un panneau, un menu ou une barre, sans code spécial par cible.
            ["color-random", target] => {
                apply_color_to_target(&percent_decode(target), color::random_dark_theme(), refs);
            }
            // 🎲 TOUT l'app d'un coup : chaque panneau/applet, chaque menu ET
            // chaque barre reçoit SA PROPRE couleur au hasard (demande explicite :
            // « pour chaque élément de l'app »). Un tirage INDÉPENDANT par cible.
            ["color-random-all"] => {
                for p in PANNEAUX_ACCENT {
                    apply_color_to_target(p, color::random_dark_theme(), refs);
                }
                for k in menu::MENU_COLOR_KEYS {
                    apply_color_to_target(&format!("menu:{k}"), color::random_dark_theme(), refs);
                }
                for b in ["bar_top", "bar_left", "bar_right", "bar_bottom", "centre_haut"] {
                    apply_color_to_target(b, color::random_dark_theme(), refs);
                }
            }
            // Un menu revient à la couleur PARTAGÉE des menus (voir
            // `menu::clear_menu_color`) — le bouton « couleur partagée » n'apparaît
            // que pour un menu actuellement personnalisé (voir `menuCustom`).
            ["menu-color-reset", key] => menu::clear_menu_color(&percent_decode(key)),
            ["custom-color", "add", hex] => {
                if let Some(rgb) = parse_hex_rgb(hex) {
                    with_data(|d| add_custom_color(&mut d.custom_colors, rgb));
                }
            }
            ["custom-color", "remove", index] => {
                if let Ok(index) = index.parse::<usize>() {
                    with_data(|d| remove_custom_color(&mut d.custom_colors, index));
                }
            }
            ["bar-tint", bar, percent] => {
                if let (Some(bar), Ok(percent)) = (parse_barre_principale(bar), percent.parse::<u8>()) {
                    apply_bar_tint(refs.bar_tint, bar, 1.0 - percent as f32 / 100.0);
                }
            }
            // « Toutes les barres » : l'écartement général, et chaque barre y revient.
            ["icon-gap", px] => {
                if let Ok(px) = px.parse::<f32>() {
                    refs.dock.set_icon_gap(px);
                    config::save_icon_gap(px);
                    refs.dock.set_icon_gaps_barres([None; 4]);
                    config::save_icon_gaps_barres(&[None; 4]);
                }
            }
            // L'écartement d'UNE barre (voir `ui::dock::BARRES_ECARTEMENT`).
            ["icon-gap-barre", barre, px] => {
                let rang = crate::ui::dock::BARRES_ECARTEMENT.iter().position(|b| b == barre);
                if let (Some(rang), Ok(px)) = (rang, px.parse::<f32>()) {
                    let mut gaps = refs.dock.icon_gaps_barres();
                    gaps[rang] = Some(px);
                    refs.dock.set_icon_gaps_barres(gaps);
                    config::save_icon_gaps_barres(&gaps);
                }
            }
            // "settings/desktop-dot-size/<px>" — taille des pastilles de
            // bureaux virtuels de la barre PRIMAIRE (voir `ui::dock::Dock::
            // desktop_dot_size`), DEMANDE EXPLICITE. Même principe que
            // `icon-gap` juste au-dessus.
            ["desktop-dot-size", px] => {
                if let Ok(px) = px.parse::<f32>() {
                    refs.dock.set_desktop_dot_size(px);
                    config::save_desktop_dot_size(px);
                }
            }
            // "settings/topbar-text-size/<auto|px>" — taille de police des textes
            // de la barre du haut de la barre PRIMAIRE (voir `ui::dock::Dock::
            // topbar_text_size`), DEMANDE EXPLICITE. **DEUX formes, contrairement
            // à `desktop-dot-size` ci-dessus** : ce réglage-ci a un mode
            // AUTOMATIQUE, qui n'est pas une valeur comme une autre — `auto` le
            // remet, un nombre le fixe. Les deux écrivent la configuration dans
            // le même mouvement, donc recharger ne perd jamais le mode.
            ["topbar-text-size", "auto"] => {
                refs.dock.set_topbar_text_size(None);
                config::save_topbar_text_size(None);
            }
            ["topbar-text-size", px] => {
                if let Ok(px) = px.parse::<f32>() {
                    refs.dock.set_topbar_text_size(Some(px));
                    config::save_topbar_text_size(Some(px));
                }
            }
            // "settings/rappel-feries/<0|1>" — rappel automatique sur les JOURS
            // FÉRIÉS (DEMANDE EXPLICITE, 2026-09-23 : « la possibilité de mettre
            // un rappel automatique sur les dates des fêtes ou des
            // anniversaires »). Réglage GLOBAL — on ne coche pas onze fériés un
            // par un —, là où les anniversaires portent chacun le leur.
            // Écrit la configuration ET l'état en mémoire, dans le même
            // mouvement : les deux ne peuvent donc pas diverger.
            ["rappel-feries", actif] => {
                let actif = *actif == "1";
                crate::win32::fetes::set_rappel_feries(actif);
                config::save_rappel_feries(actif);
            }
            // "settings/envoi-service/<nom>", ".../envoi-duree/<durée>" et
            // ".../envoi-cle/<service>/<clé encodée>" — l'hébergeur de « Envoyer
            // en ligne » dans l'éditeur d'image, DEMANDE EXPLICITE (2026-09-25) :
            // « un choix dans les Réglages ». Une clé VIDE efface celle du service.
            ["envoi-service", nom] => {
                if let Some(hebergeur) = crate::win32::envoi_image::Hebergeur::from_config(nom) {
                    modifier_envoi(|r| r.hebergeur = hebergeur);
                }
            }
            ["envoi-duree", duree] => {
                if crate::win32::envoi_image::DUREES_LITTERBOX.contains(duree) {
                    modifier_envoi(|r| r.duree = duree.to_string());
                }
            }
            ["envoi-cle", nom, encoded] => {
                if let Some(hebergeur) = crate::win32::envoi_image::Hebergeur::from_config(nom) {
                    let cle = crate::win32::cef_app::percent_decode(encoded).trim().to_string();
                    modifier_envoi(|r| {
                        if cle.is_empty() {
                            r.cles.remove(hebergeur.as_str());
                        } else {
                            r.cles.insert(hebergeur.as_str().to_string(), cle);
                        }
                    });
                }
            }
            // "settings/envoi-perso-<quoi>/<valeur encodée>" — le SERVEUR
            // PERSONNALISÉ, DEMANDE EXPLICITE (2026-09-25) : « la possibilité
            // d'utiliser un serveur personnalisé ». Une valeur refusée ne touche
            // à rien et le dit (voir `modifier_perso`).
            ["envoi-perso-url", encoded] => {
                let url = crate::win32::cef_app::percent_decode(encoded);
                modifier_perso(|r| {
                    if url.trim().is_empty() {
                        r.perso.url.clear();
                        r.perso.parametres.clear();
                    } else {
                        r.perso.changer_url(&url)?;
                    }
                    Ok(String::new())
                });
            }
            ["envoi-perso-champ", encoded] => {
                let champ = crate::win32::cef_app::percent_decode(encoded).trim().to_string();
                modifier_perso(|r| {
                    r.perso.champ_fichier = champ;
                    Ok(String::new())
                });
            }
            ["envoi-perso-lien", encoded] => {
                let lien = crate::win32::cef_app::percent_decode(encoded).trim().to_string();
                modifier_perso(|r| {
                    r.perso.lien = lien;
                    Ok(String::new())
                });
            }
            ["envoi-perso-entetes", encoded] => {
                let texte = crate::win32::cef_app::percent_decode(encoded);
                modifier_perso(|r| {
                    r.perso.entetes = crate::win32::envoi_image::lire_lignes(&texte, ':')?;
                    Ok(format!("{} en-tête(s) enregistré(s)", r.perso.entetes.len()))
                });
            }
            ["envoi-perso-champs", encoded] => {
                let texte = crate::win32::cef_app::percent_decode(encoded);
                modifier_perso(|r| {
                    r.perso.champs = crate::win32::envoi_image::lire_lignes(&texte, '=')?;
                    Ok(format!("{} champ(s) enregistré(s)", r.perso.champs.len()))
                });
            }
            ["envoi-perso-sxcu", encoded] => {
                let texte = crate::win32::cef_app::percent_decode(encoded);
                modifier_perso(|r| {
                    let (perso, nom) = crate::win32::envoi_image::ServeurPerso::depuis_sxcu(&texte)?;
                    r.perso = perso;
                    r.hebergeur = crate::win32::envoi_image::Hebergeur::Perso;
                    Ok(format!("Importé : {nom}"))
                });
            }
            // "settings/zone-cell-margin/<px>" — MARGE laissée dans la case
            // d'une icône de zone (voir `ui::dock::Dock::DEFAULT_ZONE_CELL_MARGIN`),
            // DEMANDE EXPLICITE : « les icônes dans les grilles des zones de
            // droite aient une marge de 4 pixels dans leur case », puis « on
            // pourrait essayer 8 pixels ou mieux que ce soit paramétrable ».
            //
            // L'état vit dans une CELLULE GLOBALE (`ui::dock::set_zone_cell_margin`)
            // et non dans `refs.dock`, comme le clignotement de notification et le
            // miroir : elle est lue au dessin, qui ne partage aucun état avec ce
            // panneau — la borne est appliquée par le setter.
            ["zone-cell-margin", px] => {
                if let Ok(px) = px.parse::<f32>() {
                    crate::ui::dock::set_zone_cell_margin(px);
                    config::save_zone_cell_margin(px);
                }
            }
            // "settings/separator-size/<px>" — diamètre du petit cercle
            // indicateur des séparateurs de la barre PRIMAIRE (voir `ui::
            // dock::Dock::separator_size`), DEMANDE EXPLICITE ("ajoute la
            // possibilité de modifier la taille des séparateurs par
            // interface"). Même principe que `icon-gap`/`desktop-dot-size`
            // ci-dessus — bornage propre à `set_separator_size`.
            ["separator-size", px] => {
                if let Ok(px) = px.parse::<f32>() {
                    refs.dock.set_separator_size(px);
                    config::save_separator_size(refs.dock.separator_size());
                }
            }
            // "settings/running-dot-size/<px>", ".../running-dot-shape/<forme>"
            // et ".../running-dot-color/auto" — indicateur « en cours
            // d'exécution » du dock, DEMANDE EXPLICITE : « donnes moi la
            // possibilité d'agrandir la taille de l'indicateur, de changer sa
            // forme et sa couleur dans le dash de réglages ». La couleur
            // PERSONNALISÉE passe par le sélecteur partagé (voir
            // `apply_color_to_target`, cible `"running_dot"`), comme les
            // séparateurs.
            ["running-dot-size", px] => {
                if let Ok(px) = px.parse::<f32>() {
                    refs.dock.set_running_dot_size(px);
                    // Relu après coup : le setter BORNE, et c'est la valeur
                    // bornée qui doit être persistée (même remarque que
                    // `separator-size`).
                    config::save_running_dot_size(refs.dock.running_dot_size());
                }
            }
            // Décalage de l'indicateur et marge autour des icônes — DEMANDE
            // EXPLICITE (2026-10-14). Relus après coup : les setters BORNENT.
            ["running-dot-offset", px] => {
                if let Ok(px) = px.parse::<f32>() {
                    refs.dock.set_running_dot_offset(px);
                    config::save_running_dot_offset(refs.dock.running_dot_offset());
                }
            }
            ["icon-margin", px] => {
                if let Ok(px) = px.parse::<f32>() {
                    crate::ui::dock::set_icon_margin(px);
                    config::save_icon_margin(crate::ui::dock::icon_margin());
                }
            }
            // Le fondu en haut de la barre de gauche — DEMANDE EXPLICITE
            // (2026-10-14). Relu après coup : le setter BORNE.
            ["fondu-haut-gauche", px] => {
                if let Ok(px) = px.parse::<f32>() {
                    crate::ui::dock::set_fondu_haut_gauche(px);
                    config::save_fondu_haut_gauche(crate::ui::dock::fondu_haut_gauche());
                }
            }
            ["fondu-haut-droite", px] => {
                if let Ok(px) = px.parse::<f32>() {
                    crate::ui::dock::set_fondu_haut_droite(px);
                    config::save_fondu_haut_droite(crate::ui::dock::fondu_haut_droite());
                }
            }
            // L'ombre sous la barre du haut — DEMANDE EXPLICITE (2026-10-15).
            // Hauteur en px, force en % ; relues après coup : les setters BORNENT.
            ["ombre-barre-haut", px] => {
                if let Ok(px) = px.parse::<f32>() {
                    crate::ui::dock::set_ombre_barre_haut(px);
                    config::save_ombre_barre_haut(crate::ui::dock::ombre_barre_haut());
                }
            }
            ["ombre-couleur-des-barres", flag] => {
                let active = *flag == "1";
                crate::win32::ombre_barre::activer_couleur_des_barres(active);
                config::save_bool_setting(config::OMBRE_COULEUR_DES_BARRES, active);
            }
            // La noire de la zone centrale à la couleur de sa barre — DEMANDE
            // EXPLICITE (2026-10-23), voir `ombre_barre::zone_en_couleur`.
            ["ombre-zone-en-couleur", flag] => {
                let active = *flag == "1";
                crate::win32::ombre_barre::activer_zone_en_couleur(active);
                config::save_bool_setting(config::OMBRE_ZONE_EN_COULEUR, active);
            }
            // Couleur affichée ou réglée — DEMANDE EXPLICITE (2026-10-19).
            ["ombre-couleur-affichee", flag] => {
                let affichee = *flag == "1";
                crate::win32::ombre_barre::choisir_couleur_affichee(affichee);
                config::save_bool_setting(config::OMBRE_COULEUR_AFFICHEE, affichee);
            }
            // L'intensité des ombres colorées, en % — DEMANDE EXPLICITE (2026-10-18).
            ["ombre-couleur-intensite", pc] => {
                if let Ok(pc) = pc.parse::<f32>() {
                    crate::win32::ombre_barre::regler_intensite_couleur(pc);
                    config::save_ombre_couleur_intensite(crate::win32::ombre_barre::intensite_couleur());
                }
            }
            // L'intensité de l'ombre noire, en % — DEMANDE EXPLICITE (2026-10-19).
            ["ombre-noire-intensite", pc] => {
                if let Ok(pc) = pc.parse::<f32>() {
                    crate::win32::ombre_barre::regler_intensite_noire(pc);
                    config::save_ombre_noire_intensite(crate::win32::ombre_barre::intensite_noire());
                }
            }
            // Les ombres d'un écran secondaire, réglées à part — DEMANDE EXPLICITE
            // (2026-10-19) : ".../ombre-ecran/<écran>/suit" le rend à l'écran
            // principal, ".../ombre-ecran/<écran>/perso" part de ses réglages,
            // ".../ombre-ecran/<écran>/<reglage>/<valeur>" (largeurs en px, forces
            // en %, la zone en 0/1).
            ["ombre-ecran", ecran, "suit"] => {
                crate::win32::ombre_barre::poser_reglages_ecran(&percent_decode(ecran), None);
                config::save_ombre_ecrans();
            }
            ["ombre-ecran", ecran, "perso"] => {
                let r = crate::win32::ombre_barre::ReglagesEcran::du_principal();
                crate::win32::ombre_barre::poser_reglages_ecran(&percent_decode(ecran), Some(r));
                config::save_ombre_ecrans();
            }
            ["ombre-ecran", ecran, reglage, valeur] => {
                let ecran = percent_decode(ecran);
                let nombre = valeur.parse::<f32>().ok().filter(|v| v.is_finite());
                let mut r = crate::win32::ombre_barre::reglages_ecran(&ecran).unwrap_or_else(crate::win32::ombre_barre::ReglagesEcran::du_principal);
                let connu = match (*reglage, nombre) {
                    ("noire-largeur", Some(px)) => { r.noire[0] = px; true }
                    ("noire-force", Some(pc)) => { r.noire[1] = pc / 100.0; true }
                    ("zone", _) => { r.zone = *valeur == "1"; true }
                    ("couleur-largeur", Some(px)) => { r.couleur[0] = px; true }
                    ("couleur-force", Some(pc)) => { r.couleur[1] = pc / 100.0; true }
                    _ => false,
                };
                if connu {
                    crate::win32::ombre_barre::poser_reglages_ecran(&ecran, Some(r));
                    config::save_ombre_ecrans();
                } else {
                    crate::diag!("[settings] ombre-ecran : {reglage:?} = {valeur:?} refusé");
                }
            }
            // Où tombe l'ombre noire, barre par barre — DEMANDE EXPLICITE
            // (2026-10-19) : ".../ombre-noire-portee/<barres|zone>/<0..=3>/<0|1>".
            ["ombre-noire-portee", ou, barre, flag] => {
                let mut portee = crate::win32::ombre_barre::portee();
                let cases = match *ou {
                    "barres" => Some(&mut portee.barres),
                    "zone" => Some(&mut portee.zone),
                    _ => None,
                };
                match (cases, barre.parse::<usize>().ok().filter(|&i| i < 4)) {
                    (Some(cases), Some(i)) => {
                        cases[i] = *flag == "1";
                        crate::win32::ombre_barre::poser_portee(portee);
                        config::save_ombre_noire_portee(&portee);
                    }
                    _ => crate::diag!("[settings] ombre-noire-portee : {ou:?}/{barre:?} refusé"),
                }
            }
            ["ombre-barre-haut-force", pc] => {
                if let Ok(pc) = pc.parse::<f32>() {
                    crate::ui::dock::set_ombre_barre_haut_force(pc / 100.0);
                    config::save_ombre_barre_haut_force(crate::ui::dock::ombre_barre_haut_force());
                }
            }
            // L'ombre des AUTRES barres — DEMANDE EXPLICITE (2026-10-16) : « un
            // effet d'ombrage paramétrable pour tout le reste de la zone
            // intérieure ». ".../ombre-cotes/<réglage>/<valeur>" : les cases en
            // 0/1, la largeur en px, la force en %. Bornés par `modifier_cotes`.
            ["ombre-cotes", reglage, valeur] => {
                let mut connu = false;
                let reglages = crate::win32::ombre_barre::modifier_cotes(|r| connu = regler_cotes(r, reglage, valeur));
                if connu {
                    config::save_ombre_cotes(&reglages);
                } else {
                    crate::diag!("[settings] ombre-cotes : {reglage:?} = {valeur:?} refusé");
                }
            }
            // ⭐ L'OMBRE COLORÉE A SES PROPRES RÉGLAGES — DEMANDE EXPLICITE
            // (2026-10-19) : « des réglages complets pour chaque type d'ombre, noire
            // et couleur » (voir `win32::ombre_barre::ReglagesCouleur`). Les mêmes
            // que la noire : ".../ombre-couleur-haut/<largeur|force>/<valeur>" et
            // ".../ombre-couleur-cotes/<réglage>/<valeur>".
            ["ombre-couleur-haut", reglage, valeur] => {
                let nombre = valeur.parse::<f32>().ok().filter(|v| v.is_finite());
                let mut connu = true;
                let reglages = crate::win32::ombre_barre::modifier_couleur(|r| match (*reglage, nombre) {
                    ("largeur", Some(px)) => r.largeur_haut = px,
                    ("force", Some(pc)) => r.force_haut = pc / 100.0,
                    _ => connu = false,
                });
                if connu {
                    config::save_ombre_couleur(&reglages);
                } else {
                    crate::diag!("[settings] ombre-couleur-haut : {reglage:?} = {valeur:?} refusé");
                }
            }
            // ⭐ LE CENTRE DE LA BARRE DU HAUT — DEMANDE EXPLICITE (2026-10-31),
            // voir `win32::centre_haut::regler`. Sa couleur passe par le
            // sélecteur, cible `centre_haut` (`apply_color_to_target`).
            // ⭐ L'EMPILEMENT DES ICÔNES EN HAUT DES BARRES LATÉRALES — DEMANDE
            // EXPLICITE (2026-10-31), voir `ui::empilement::regler`.
            ["empilement", reglage, valeur] => {
                let mut connu = false;
                let reglages = crate::ui::empilement::modifier(|r| connu = crate::ui::empilement::regler(r, reglage, valeur));
                if connu {
                    config::save_empilement(&reglages);
                } else {
                    crate::diag!("[settings] empilement : {reglage:?} = {valeur:?} refusé");
                }
            }
            ["ombre-centre-haut", reglage, valeur] => {
                let mut connu = false;
                let reglages = crate::win32::centre_haut::modifier(|r| connu = crate::win32::centre_haut::regler(r, reglage, valeur));
                if connu {
                    config::save_ombre_centre_haut(&reglages);
                } else {
                    crate::diag!("[settings] ombre-centre-haut : {reglage:?} = {valeur:?} refusé");
                }
            }
            ["ombre-couleur-cotes", reglage, valeur] => {
                let mut connu = false;
                let reglages = crate::win32::ombre_barre::modifier_couleur(|r| connu = regler_cotes(&mut r.cotes, reglage, valeur));
                if connu {
                    config::save_ombre_couleur(&reglages);
                } else {
                    crate::diag!("[settings] ombre-couleur-cotes : {reglage:?} = {valeur:?} refusé");
                }
            }
            // Taille des icônes de la barre du haut, en % — DEMANDE EXPLICITE
            // (2026-10-14). Relue après coup : le setter BORNE.
            ["top-icon-scale", pct] => {
                if let Ok(pct) = pct.parse::<f32>() {
                    crate::ui::dock::set_top_icon_scale(pct / 100.0);
                    config::save_top_icon_scale(crate::ui::dock::top_icon_scale());
                }
            }
            // Même chose pour la barre du BAS — DEMANDE EXPLICITE (2026-10-14).
            ["bottom-icon-scale", pct] => {
                if let Ok(pct) = pct.parse::<f32>() {
                    crate::ui::dock::set_bottom_icon_scale(pct / 100.0);
                    config::save_bottom_icon_scale(crate::ui::dock::bottom_icon_scale());
                }
            }
            // « Voir l'aide » — l'`id` d'une section de `aide.html`.
            ["ouvrir-aide", ancre] => {
                AIDE_DEMANDEE.with(|a| *a.borrow_mut() = Some(ancre.to_string()));
            }
            // Et pour la barre de GAUCHE — DEMANDE EXPLICITE (2026-10-18).
            ["left-icon-scale", pct] => {
                if let Ok(pct) = pct.parse::<f32>() {
                    crate::ui::dock::set_left_icon_scale(pct / 100.0);
                    config::save_left_icon_scale(crate::ui::dock::left_icon_scale());
                }
            }
            ["running-dot-shape", name] => {
                let shape = crate::ui::dock::RunningDotShape::from_config(name);
                refs.dock.set_running_dot_shape(shape);
                config::save_running_dot_shape(shape.as_str());
            }
            ["running-dot-color", "auto"] => {
                refs.dock.set_running_dot_color(None);
                config::save_running_dot_color(None);
                crate::ui::dock::set_indicateur_degrade(false);
                config::save_indicateur_reglages(false, crate::ui::dock::indicateur_reglages());
            }
            // ⭐ LE DÉGRADÉ ET LA COULEUR PERSO (DEMANDE EXPLICITE, 2026-10-14). La
            // couleur choisie est GARDÉE sous le dégradé : revenir à « Personnalisée »
            // la retrouve.
            ["running-dot-color", "degrade"] => {
                crate::ui::dock::set_indicateur_degrade(true);
                config::save_indicateur_reglages(true, crate::ui::dock::indicateur_reglages());
            }
            ["running-dot-color", "perso"] => {
                crate::ui::dock::set_indicateur_degrade(false);
                config::save_indicateur_reglages(false, crate::ui::dock::indicateur_reglages());
                if refs.dock.running_dot_color().is_none() {
                    let blanc = Some([1.0, 1.0, 1.0, 1.0]);
                    refs.dock.set_running_dot_color(blanc);
                    config::save_running_dot_color(blanc);
                }
            }
            // Saturation, luminosité, transparence — en %, relus après coup : le
            // setter BORNE.
            ["running-dot-reglage", quel, pct] => {
                let rang = match *quel {
                    "saturation" => Some(0),
                    "luminosite" => Some(1),
                    "transparence" => Some(2),
                    _ => None,
                };
                if let (Some(rang), Ok(pct)) = (rang, pct.parse::<f32>()) {
                    crate::ui::dock::set_indicateur_reglage(rang, pct / 100.0);
                    config::save_indicateur_reglages(crate::ui::dock::indicateur_degrade(), crate::ui::dock::indicateur_reglages());
                }
            }
            // "settings/preview-fade-ms/<ms>" — durée du fondu d'apparition et de
            // disparition du panneau d'aperçus (voir `win32::window_previews::
            // fade_ms`), DEMANDE EXPLICITE : « l'effet de fondu est trop rapide,
            // donnes moi la possibilité de régler sa vitesse ». Le bornage
            // appartient au module (`set_fade_ms`), d'où la relecture de la valeur
            // bornée avant de sauver — même remarque que `preview-thumb-width`
            // ci-dessus.
            ["preview-fade-ms", ms] => {
                if let Ok(ms) = ms.parse::<f32>() {
                    crate::win32::window_previews::set_fade_ms(ms);
                    config::save_preview_fade_ms(crate::win32::window_previews::fade_ms());
                }
            }
            // "settings/menu-fade-ms/<ms>" et "settings/tooltip-fade-ms/<ms>" —
            // durées des fondus des MENUS et des INFOBULLES (voir
            // `win32::fade`), DEMANDE EXPLICITE : « le fondu des infobulles et
            // des menus ». Même forme que `preview-fade-ms` ci-dessus, y compris
            // la relecture de la valeur BORNÉE avant de sauver : le bornage
            // appartient au module (`set_menu_fade_ms`/`set_tooltip_fade_ms`),
            // et les deux curseurs sont SÉPARÉS sur décision de l'utilisateur.
            ["menu-fade-ms", ms] => {
                if let Ok(ms) = ms.parse::<f32>() {
                    crate::win32::fade::set_menu_fade_ms(ms);
                    config::save_menu_fade_ms(crate::win32::fade::menu_fade_ms());
                }
            }
            ["tooltip-fade-ms", ms] => {
                if let Ok(ms) = ms.parse::<f32>() {
                    crate::win32::fade::set_tooltip_fade_ms(ms);
                    config::save_tooltip_fade_ms(crate::win32::fade::tooltip_fade_ms());
                }
            }
            // "settings/preview-thumb-width/<px>" — largeur des vignettes du
            // panneau d'aperçus de fenêtres (voir `win32::window_previews`),
            // DEMANDE EXPLICITE : « une taille ajustable dans les réglages ».
            // Le bornage appartient au panneau (`set_thumb_width`), d'où la
            // relecture du champ borné avant de sauver — même remarque que
            // `separator-size` ci-dessus.
            ["preview-thumb-width", px] => {
                if let Ok(px) = px.parse::<f32>() {
                    *refs.preview_thumb_width = px.clamp(
                        crate::win32::window_previews::MIN_THUMB_WIDTH,
                        crate::win32::window_previews::MAX_THUMB_WIDTH,
                    );
                    config::save_preview_thumb_width(*refs.preview_thumb_width);
                }
            }
            // "settings/desktop-preview-width/<px>", ".../desktop-preview-fade-ms/<ms>"
            // et ".../desktop-preview-hover-delay-ms/<ms>" — les TROIS réglages de
            // l'aperçu d'un bureau virtuel (voir `win32::desktop_previews`),
            // DEMANDE EXPLICITE : « des réglages pour les fenêtres de
            // prévisualisation des bureaux comme pour les programmes ».
            //
            // TROIS armes de la MÊME forme, et c'est la forme de `preview-fade-ms`
            // ci-dessus plutôt que celle de `preview-thumb-width` : le bornage
            // appartient au module (son setter), donc on relit la valeur BORNÉE
            // avant de sauver au lieu de recopier les bornes ici. Recopier est
            // exactement ce que la règle du dépôt interdit — deux endroits qui
            // répondent à la même question finissent par diverger (`bar_size` l'a
            // payé : 160 d'un côté, 500 de l'autre).
            ["desktop-preview-width", px] => {
                if let Ok(px) = px.parse::<f32>() {
                    crate::win32::desktop_previews::set_preview_width(px);
                    config::save_desktop_preview_width(crate::win32::desktop_previews::preview_width());
                }
            }
            ["desktop-preview-fade-ms", ms] => {
                if let Ok(ms) = ms.parse::<f32>() {
                    crate::win32::desktop_previews::set_fade_ms(ms);
                    config::save_desktop_preview_fade_ms(crate::win32::desktop_previews::fade_ms());
                }
            }
            ["desktop-preview-hover-delay-ms", ms] => {
                if let Ok(ms) = ms.parse::<f32>() {
                    crate::win32::desktop_previews::set_hover_delay_ms(ms);
                    config::save_desktop_preview_hover_delay_ms(crate::win32::desktop_previews::hover_delay_ms());
                }
            }
            // "settings/logo-animation/<0|1>", ".../logo-fade-ms/<ms>" et
            // ".../logo-color-ms/<ms>" — le FOND ANIMÉ DU LOGO (voir
            // `ui::dock::logo_background_color`), DEMANDE EXPLICITE : « un fond
            // changeant de couleur, passant de 100 % d'opacité à 100 % de
            // transparence sur un temps réglable ».
            //
            // Les deux durées suivent la forme de `preview-fade-ms` : le bornage
            // appartient au module (son setter), donc on relit la valeur BORNÉE
            // avant de sauver au lieu de recopier les bornes ici.
            ["logo-animation", flag] => {
                crate::ui::dock::set_logo_animation_enabled(*flag == "1");
                config::save_logo_animation_enabled(crate::ui::dock::logo_animation_enabled());
            }
            ["logo-fade-ms", ms] => {
                if let Ok(ms) = ms.parse::<f32>() {
                    crate::ui::dock::set_logo_fade_ms(ms);
                    config::save_logo_fade_ms(crate::ui::dock::logo_fade_ms());
                }
            }
            ["logo-color-ms", ms] => {
                if let Ok(ms) = ms.parse::<f32>() {
                    crate::ui::dock::set_logo_color_ms(ms);
                    config::save_logo_color_ms(crate::ui::dock::logo_color_ms());
                }
            }
            // "settings/logo-circle/<0|1>", ".../logo-spin-clockwise/<0|1>" et
            // ".../logo-turn-ms/<ms>" — l'ICÔNE CIRCULAIRE QUI TOURNE (voir
            // `ui::dock::logo_rotation`), DEMANDE EXPLICITE : « le choix entre
            // l'icône carré avec un fond respirant et l'icône cercle qui tourne »,
            // sens et vitesse réglables.
            ["logo-circle", flag] => {
                crate::ui::dock::set_logo_is_circle(*flag == "1");
                config::save_logo_is_circle(crate::ui::dock::logo_is_circle());
            }
            ["logo-spin-clockwise", flag] => {
                crate::ui::dock::set_logo_spin_clockwise(*flag == "1");
                config::save_logo_spin_clockwise(crate::ui::dock::logo_spin_clockwise());
            }
            ["logo-turn-ms", ms] => {
                if let Ok(ms) = ms.parse::<f32>() {
                    crate::ui::dock::set_logo_turn_ms(ms);
                    config::save_logo_turn_ms(crate::ui::dock::logo_turn_ms());
                }
            }
            // "settings/logo-size/<%>" et "settings/logo-zoom/<0|1>",
            // ".../logo-zoom-amplitude/<%>", ".../logo-zoom-ms/<ms>",
            // ".../logo-zoom-interval-ms/<ms>" — TAILLE ET ZOOM DU LOGO (voir
            // `ui::dock::logo_draw_scale`). Les pourcentages arrivent ENTIERS.
            ["logo-size", pct] => {
                if let Ok(pct) = pct.parse::<f32>() {
                    crate::ui::dock::set_logo_size(pct / 100.0);
                    config::save_logo_size(crate::ui::dock::logo_size());
                }
            }
            ["logo-zoom", flag] => {
                crate::ui::dock::set_logo_zoom_enabled(*flag == "1");
                save_logo_zoom();
            }
            ["logo-zoom-amplitude", pct] => {
                if let Ok(pct) = pct.parse::<f32>() {
                    crate::ui::dock::set_logo_zoom_amplitude(pct / 100.0);
                    save_logo_zoom();
                }
            }
            ["logo-zoom-ms", ms] => {
                if let Ok(ms) = ms.parse::<f32>() {
                    crate::ui::dock::set_logo_zoom_ms(ms);
                    save_logo_zoom();
                }
            }
            ["logo-zoom-interval-ms", ms] => {
                if let Ok(ms) = ms.parse::<f32>() {
                    crate::ui::dock::set_logo_zoom_interval_ms(ms);
                    save_logo_zoom();
                }
            }
            // "settings/separator-color/auto" (suit la couleur du texte de
            // la barre, voir `ui::dock::Dock::separator_color`) ou ".../
            // transparent" (invisible) — le cas "couleur choisie" passe par
            // le `colorPicker` riche PARTAGÉ (voir `["color", target, hex]`
            // ci-dessus, `apply_color_to_target`, cible `"separator"`) plutôt
            // que par un simple champ hexadécimal dédié — DEMANDE EXPLICITE
            // ("les séparateurs d'icones soit de la couleur principale du
            // texte de la barre à laquelle ils sont ratachés et donnes la
            // possibilité de choisir leur couleur ou de les rendre
            // transparent").
            ["separator-color", "auto"] => {
                refs.dock.set_separator_color(None);
                config::save_separator_color(None);
            }
            ["separator-color", "transparent"] => {
                refs.dock.set_separator_color(Some([0.0, 0.0, 0.0, 0.0]));
                config::save_separator_color(Some([0.0, 0.0, 0.0, 0.0]));
            }
            // "settings/secondary-screen-separator-color/<device_name>/
            // <auto|transparent>" — même principe que `separator-color`
            // ci-dessus, PERSONNALISÉ par écran secondaire (voir
            // `SettingsRefs::secondary_screen_separator_colors`, cible
            // `"secondary:<device>:separator"` du `colorPicker` partagé pour
            // le cas "couleur choisie").
            ["secondary-screen-separator-color", device_name, "auto"] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_separator_colors.remove(&device_name);
                config::save_secondary_screen_separator_colors(refs.secondary_screen_separator_colors);
                *refs.secondary_screens_dirty = true;
            }
            ["secondary-screen-separator-color", device_name, "transparent"] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_separator_colors.insert(device_name, [0.0, 0.0, 0.0, 0.0]);
                config::save_secondary_screen_separator_colors(refs.secondary_screen_separator_colors);
                *refs.secondary_screens_dirty = true;
            }
            // "settings/secondary-screen-desktop-dot-size/<device_name>/
            // <auto|px>" — même principe que
            // `secondary-screen-fullscreen-clock-text-scale` ci-dessous.
            ["secondary-screen-desktop-dot-size", device_name, "auto"] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_desktop_dot_sizes.remove(&device_name);
                config::save_secondary_screen_desktop_dot_sizes(refs.secondary_screen_desktop_dot_sizes);
                *refs.secondary_screens_dirty = true;
            }
            ["secondary-screen-desktop-dot-size", device_name, px] => {
                if let Ok(px) = px.parse::<f32>() {
                    let device_name = percent_decode(device_name);
                    refs.secondary_screen_desktop_dot_sizes.insert(device_name, px);
                    config::save_secondary_screen_desktop_dot_sizes(refs.secondary_screen_desktop_dot_sizes);
                    *refs.secondary_screens_dirty = true;
                }
            }
            // "settings/secondary-screen-topbar-text-size/<device_name>/<auto|px>"
            // — taille de police des textes de la barre du haut de CET écran
            // (voir `SettingsRefs::secondary_screen_topbar_text_sizes`) — même
            // structure que `secondary-screen-desktop-dot-size` ci-dessus, la
            // forme `auto` RETIRANT l'écran de la table (il suit alors la barre
            // primaire, automatique compris).
            ["secondary-screen-topbar-text-size", device_name, "auto"] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_topbar_text_sizes.remove(&device_name);
                config::save_secondary_screen_topbar_text_sizes(refs.secondary_screen_topbar_text_sizes);
                *refs.secondary_screens_dirty = true;
            }
            ["secondary-screen-topbar-text-size", device_name, px] => {
                if let Ok(px) = px.parse::<f32>() {
                    let device_name = percent_decode(device_name);
                    refs.secondary_screen_topbar_text_sizes.insert(device_name, px);
                    config::save_secondary_screen_topbar_text_sizes(refs.secondary_screen_topbar_text_sizes);
                    *refs.secondary_screens_dirty = true;
                }
            }
            // "settings/secondary-screen-icon-gap/<device_name>/<auto|px>" —
            // écartement entre icônes PERSONNALISÉ par écran secondaire
            // (voir `SettingsRefs::secondary_screen_icon_gaps`, DEMANDE
            // EXPLICITE) — même principe que
            // `secondary-screen-desktop-dot-size` juste au-dessus.
            ["secondary-screen-icon-gap", device_name, "auto"] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_icon_gaps.remove(&device_name);
                config::save_secondary_screen_icon_gaps(refs.secondary_screen_icon_gaps);
                *refs.secondary_screens_dirty = true;
            }
            ["secondary-screen-icon-gap", device_name, px] => {
                if let Ok(px) = px.parse::<f32>() {
                    let device_name = percent_decode(device_name);
                    refs.secondary_screen_icon_gaps.insert(device_name, px);
                    config::save_secondary_screen_icon_gaps(refs.secondary_screen_icon_gaps);
                    *refs.secondary_screens_dirty = true;
                }
            }
            // "settings/secondary-screen-separator-size/<device_name>/
            // <auto|px>" — diamètre du petit cercle indicateur des
            // séparateurs PERSONNALISÉ par écran secondaire (voir
            // `SettingsRefs::secondary_screen_separator_sizes`, DEMANDE
            // EXPLICITE : "ajoute la possibilité de modifier la taille des
            // séparateurs par interface") — même principe que
            // `secondary-screen-desktop-dot-size`/`secondary-screen-icon-gap`
            // ci-dessus.
            ["secondary-screen-separator-size", device_name, "auto"] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_separator_sizes.remove(&device_name);
                config::save_secondary_screen_separator_sizes(refs.secondary_screen_separator_sizes);
                *refs.secondary_screens_dirty = true;
            }
            ["secondary-screen-separator-size", device_name, px] => {
                if let Ok(px) = px.parse::<f32>() {
                    let device_name = percent_decode(device_name);
                    refs.secondary_screen_separator_sizes.insert(device_name, px);
                    config::save_secondary_screen_separator_sizes(refs.secondary_screen_separator_sizes);
                    *refs.secondary_screens_dirty = true;
                }
            }
            ["bar-size", bar, px] => {
                if let Ok(px) = px.parse::<f32>() {
                    match *bar {
                        // LE VERROU : jamais sous le minimum de la barre, quoi
                        // que la page envoie (voir `ui::dock::borner_epaisseur`).
                        "top" => *refs.top_thickness = crate::ui::dock::borner_epaisseur(0, px),
                        "left" => *refs.left_width = crate::ui::dock::borner_epaisseur(1, px),
                        "right" => *refs.right_width = crate::ui::dock::borner_epaisseur(2, px),
                        "bottom" => *refs.bottom_thickness = crate::ui::dock::borner_epaisseur(3, px),
                        _ => {}
                    }
                }
                // Voir la doc de `SettingsRefs::bar_size_dragging` — mis à
                // `true` à CHAQUE `input` du curseur (pas seulement au
                // premier), idempotent, pour ne rien manquer même si un
                // `["bar-size-commit"]` se perdait quelque part.
                *refs.bar_size_dragging = true;
            }
            // Voir la doc de `SettingsRefs::bar_size_dragging` — envoyé par
            // `assets/popups/settings.html` sur l'événement DOM `change` du
            // curseur (une seule fois, au relâchement, contrairement à
            // `input` ci-dessus), PAS un curseur "Largeur"/"Épaisseur"
            // précis : peu importe LEQUEL des trois vient d'être relâché,
            // dégeler suffit dans tous les cas.
            ["bar-size-commit"] => {
                *refs.bar_size_dragging = false;
            }
            // Masquage automatique de la VRAIE barre des tâches Windows
            // (voir `win32::taskbar_autohide`, doc de module) — DEMANDE
            // EXPLICITE : "depuis le dash réglage [...] activer le
            // masquage automatique de la barre des tâches de windows". Pas
            // de champ `SettingsRefs`/persistance NeuraDesk ici : Windows
            // fait déjà foi et persiste lui-même ce réglage, exactement
            // comme si l'utilisateur l'avait coché depuis ses propres
            // paramètres.
            ["taskbar-autohide", flag] => {
                taskbar_autohide::set_enabled(*flag == "1");
                // Le masquage libère (ou rend) la bande de la barre des tâches :
                // la fenêtre du dock doit s'y adapter. Poussé ICI en plus de la
                // rediffusion que le shell émet de son côté (voir
                // `Event::PrimaryDockResync`, qui porte le pourquoi des deux) —
                // `ABM_SETSTATE` ne renvoie rien qui dise s'il a réussi.
                crate::win32::window::request_primary_dock_resync();
            }
            ["blur-passes", n] => {
                if let Ok(n) = n.parse::<u32>() {
                    blur::set_blur_passes(n);
                    config::save_blur_passes(n);
                }
            }
            ["blur-material", index] => {
                if let Ok(index) = index.parse::<usize>() {
                    if index < material::MATERIALS.len() {
                        material::set_current(index);
                        config::save_blur_material(index);
                    }
                }
            }
            ["menu-scale", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    let scale = percent as f32 / 100.0;
                    menu::set_menu_scale(scale);
                    config::save_menu_scale(scale);
                }
            }
            // "settings/hover-scale/<pourcent>" et
            // "settings/hover-ease-speed/<valeur>" — magnification au survol
            // des icônes, DEMANDE EXPLICITE (« magnification au survol réglable
            // par curseur »). Comme `menu-scale` juste au-dessus, ces deux
            // réglages sont GLOBAUX (voir `ui::dock::hover_scale`, doc) : ils
            // ne passent donc PAS par `refs` — ni `refs.dock`, qui n'est qu'une
            // des deux instances d'animation, ni un quelconque écran.
            //
            // C'est la valeur DÉJÀ BORNÉE qu'on enregistre, relue depuis la
            // cellule (`set_hover_scale` borne, voir sa doc), et non le
            // pourcentage reçu : sans quoi un panneau — ou un fichier de
            // configuration — pourrait persister une valeur hors bornes que le
            // prochain démarrage rejetterait silencieusement, exactement comme
            // `separator-size` (`refs.dock.separator_size()`).
            ["hover-scale", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    crate::ui::dock::set_hover_scale(percent as f32 / 100.0);
                    config::save_hover_scale(crate::ui::dock::hover_scale());
                }
            }
            // "settings/tirage-magnification/<pourcent>" — le TIRAGE des icônes
            // agrandies vers le centre, DEMANDE EXPLICITE : « je veux que le
            // décalage des icônes du dock se décalent et que ce soit réglable ».
            // GLOBAL lui aussi (voir `ui::dock::DEFAULT_TIRAGE`), donc même
            // discipline : la valeur DÉJÀ BORNÉE par la cellule est ce qu'on
            // enregistre, jamais le pourcentage reçu.
            ["tirage-magnification", barre, percent] => {
                // ⚠️ **LA BARRE VIENT DE LA PAGE** (`haut`/`gauche`/`droite`) : trois
                // réglages de même nature, donc un seul verbe à trois arités plutôt
                // que trois verbes — c'est la même discipline que `feature`/`son-notes`.
                let Some(barre) = barre_de_tirage(barre) else { return };
                if let Ok(percent) = percent.parse::<u32>() {
                    crate::ui::dock::set_tirage_vers_le_centre(barre, percent as f32 / 100.0);
                    let tous = crate::ui::dock::BarreTirage::TOUTES.map(crate::ui::dock::tirage_vers_le_centre);
                    config::save_tirage_magnifications(&tous);
                }
            }
            // Analysée en `f32` et non en entier : le pas de 1 du curseur est un
            // choix d'INTERFACE, pas un format de stockage (voir
            // `config::read_config_floats`) — un futur réglage plus fin ne doit
            // pas avoir à changer la façon dont la valeur voyage.
            ["hover-ease-speed", value] => {
                if let Ok(speed) = value.parse::<f32>() {
                    crate::ui::dock::set_hover_ease_speed(speed);
                    config::save_hover_ease_speed(crate::ui::dock::hover_ease_speed());
                }
            }
            // "settings/clock-hover-scale/<pourcent>" — magnification au survol
            // du TEXTE des deux horloges, DEMANDE EXPLICITE (« pouvoir régler sa
            // taille agrandie »). Même famille que `hover-scale` juste au-dessus
            // — GLOBAL, donc sans passer par `refs` — mais cellule et fichier de
            // configuration DISTINCTS : c'est tout l'objet du réglage.
            //
            // Le plafond est plus haut que celui des icônes (350 % contre 250 %),
            // la place du texte étant réservée à part (voir
            // `ui::dock::max_clock_magnified_growth`) : c'est précisément pour
            // cela que la valeur enregistrée est relue depuis la cellule bornée,
            // et non reprise du panneau.
            // "settings/popup-alpha/<pourcent>" — transparence des éléments des
            // QUATRE panneaux (voir `cef_popup::POPUP_ALPHA`), DEMANDE EXPLICITE :
            // « pouvoir régler la transparence des éléments textes sur lesquels
            // il y a de la transparence dans les dashs ». Comme `hover-scale`
            // plus haut : GLOBAL, donc sans passer par `refs`, et c'est la valeur
            // DÉJÀ BORNÉE qu'on enregistre (relue de la cellule), jamais le
            // pourcentage reçu.
            ["popup-alpha", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    crate::win32::cef_popup::set_popup_alpha(percent as f32 / 100.0);
                    config::save_popup_alpha(crate::win32::cef_popup::popup_alpha());
                }
            }
            // Fond des zones, à part du texte (voir `cef_popup::POPUP_FOND_ALPHA`).
            ["popup-fond-alpha", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    crate::win32::cef_popup::set_popup_fond_alpha(percent as f32 / 100.0);
                    config::save_popup_fond_alpha(crate::win32::cef_popup::popup_fond_alpha());
                }
            }
            ["clock-hover-scale", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    crate::ui::dock::set_clock_hover_scale(percent as f32 / 100.0);
                    config::save_clock_hover_scale(crate::ui::dock::clock_hover_scale());
                }
            }
            ["dash-icon-scale", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    let scale = percent as f32 / 100.0;
                    cef_dash::set_icon_scale(scale);
                    config::save_dash_icon_scale(scale);
                }
            }
            ["dash-label-scale", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    let scale = percent as f32 / 100.0;
                    cef_dash::set_label_scale(scale);
                    config::save_dash_label_scale(scale);
                }
            }
            ["dash-category-scale", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    let scale = percent as f32 / 100.0;
                    cef_dash::set_category_scale(scale);
                    config::save_dash_category_scale(scale);
                }
            }
            ["clipboard-thumb-scale", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    let scale = percent as f32 / 100.0;
                    cef_clipboard::set_thumb_scale(scale);
                    config::save_clipboard_thumb_scale(scale);
                }
            }
            ["clipboard-text-scale", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    let scale = percent as f32 / 100.0;
                    cef_clipboard::set_text_scale(scale);
                    config::save_clipboard_text_scale(scale);
                }
            }
            // Calendrier, Minuteur, Surveillance, Calculatrice, Serveurs SSH —
            // voir `win32::taille_panneaux`. Un nom hors de sa liste est ignoré.
            ["panel-content-scale", panneau, percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    let scale = percent as f32 / 100.0;
                    if taille_panneaux::regler(panneau, scale) {
                        config::save_panel_content_scale(panneau, taille_panneaux::facteur(panneau));
                    }
                }
            }
            // La taille des textes globale et celle des notifications — voir
            // `taille_panneaux::TEXTES`. Effet à la prochaine ouverture.
            ["taille-globale", nom, percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    if taille_panneaux::regler_global(nom, percent as f32 / 100.0) {
                        config::save_panel_content_scale(nom, taille_panneaux::global(nom));
                    }
                }
            }
            ["settings-content-scale", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    let scale = percent as f32 / 100.0;
                    set_content_scale(scale);
                    config::save_settings_content_scale(scale);
                }
            }
            ["notes-text-scale", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    let scale = percent as f32 / 100.0;
                    cef_notes::set_text_scale(scale);
                    config::save_notes_text_scale(scale);
                }
            }
            ["notes-image-scale", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    let scale = percent as f32 / 100.0;
                    cef_notes::set_image_scale(scale);
                    config::save_notes_image_scale(scale);
                }
            }
            ["tooltip-scale", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    let scale = percent as f32 / 100.0;
                    crate::win32::tooltip::set_tooltip_scale(scale);
                    config::save_tooltip_scale(scale);
                }
            }
            // Délai d'apparition des infobulles, en SECONDES (curseur gradué de
            // 1 à 5) — DEMANDE EXPLICITE : « laisser la possibilité à
            // l'utilisateur [...] de choisir le temps avant qu'elles
            // apparaissent avec un curseur gradué de 1 à 5 secondes ». Les
            // bornes vivent dans `win32::tooltip` (`set_hover_delay_secs` les
            // applique) : le curseur ne peut pas promettre un délai que le
            // réglage refuserait.
            ["tooltip-delay", secs] => {
                if let Ok(secs) = secs.parse::<f32>() {
                    crate::win32::tooltip::set_hover_delay_secs(secs);
                    config::save_tooltip_delay_secs(secs);
                }
            }
            // Activation des infobulles — DEMANDE EXPLICITE : « laisser la
            // possibilité à l'utilisateur de les activer ».
            ["tooltip-enabled", flag] => {
                let enabled = *flag == "1";
                crate::win32::tooltip::set_enabled(enabled);
                config::save_bool_setting("tooltip_enabled", enabled);
            }
            // Vitesse de défilement à la molette, PAR BARRE — DEMANDE EXPLICITE
            // (« pouvoir régler les vitesses de défilement à la molette dans les
            // barres », une par barre). La barre est nommée dans l'action plutôt
            // que devinée : deux curseurs, donc deux commandes distinctes.
            // Les bornes vivent dans `ui::dock` (`set_scroll_step_px` les
            // applique) : le curseur ne peut pas promettre une vitesse que le
            // défilement refuserait.
            ["scroll-step", side, px] => {
                let side = match *side {
                    "left" => Some(crate::ui::dock::Side::Left),
                    "right" => Some(crate::ui::dock::Side::Right),
                    _ => None,
                };
                if let (Some(side), Ok(px)) = (side, px.parse::<f32>()) {
                    crate::ui::dock::set_scroll_step_px(side, px);
                    // Les DEUX valeurs sont enregistrées ensemble : le fichier
                    // porte une ligne par barre (voir `config::load_scroll_steps`),
                    // et n'en écrire qu'une effacerait l'autre.
                    config::save_scroll_steps([
                        crate::ui::dock::scroll_step_px(crate::ui::dock::Side::Left),
                        crate::ui::dock::scroll_step_px(crate::ui::dock::Side::Right),
                    ]);
                }
            }
            // ⭐ DEMANDE EXPLICITE (2026-10-16) : « rediriger les jeux en plein
            // écran dans la zone centrale de l'app » — voir `win32::plein_ecran_zone`.
            ["plein-ecran-zone", flag] => {
                let active = *flag == "1";
                crate::win32::plein_ecran_zone::activer(active);
                config::save_bool_setting(config::PLEIN_ECRAN_DANS_LA_ZONE, active);
            }
            // ⭐ DEMANDE EXPLICITE (2026-10-16) : le plein écran exclusif dans la
            // zone centrale, voie B (sans injection) — voir `win32::jeux_fenetres`.
            // ⭐ DEMANDE EXPLICITE (2026-10-16) : « un effet d'ouverture pour les
            // fenêtres des applis » — voir `win32::ouverture_fenetres`.
            ["effet-ouverture", flag] => {
                let active = *flag == "1";
                crate::win32::ouverture_fenetres::activer(active);
                // ⭐ La DLL voile la fenêtre au lancement tant que cette durée
                // est non nulle (point 1/2 — plus de flash avant le vol) ; 0
                // quand l'effet est éteint.
                crate::win32::injection::regler_ouverture(if active {
                    crate::win32::ouverture_fenetres::duree_ms()
                } else {
                    0
                });
                config::save_bool_setting(config::EFFET_OUVERTURE, active);
            }
            // ⭐ DEMANDE EXPLICITE (2026-10-16) : « un effet de fermeture […] la
            // durée de l'effet doit être paramétrable ».
            ["effet-fermeture", flag] => {
                let active = *flag == "1";
                crate::win32::ouverture_fenetres::activer_fermeture(active);
                // ⛔ La retenue de la DLL SUIT l'interrupteur : éteint, elle vaut
                // 0 et l'application n'est plus retenue du tout (plus d'effet,
                // mais surtout plus d'attente de 1,6 s pour rien).
                crate::win32::injection::regler_retenue(if active {
                    crate::win32::ouverture_fenetres::duree_fermeture_ms()
                } else {
                    0
                });
                config::save_bool_setting(config::EFFET_FERMETURE, active);
            }
            // ⭐ DEMANDE EXPLICITE (2026-11-05) : « tout, Alt+Tab compris » —
            // l'animation du changement de fenêtre (voir `rappel_bascule`).
            ["effet-bascule", flag] => {
                let active = *flag == "1";
                crate::win32::ouverture_fenetres::activer_bascule(active);
                config::save_bool_setting(config::EFFET_BASCULE, active);
            }
            ["effet-fermeture-duree", valeur] => {
                if let Ok(ms) = valeur.parse::<f32>() {
                    crate::win32::ouverture_fenetres::regler_duree_fermeture(ms.round() as u32);
                    // La DLL tient chaque geste aussi longtemps que l'effet
                    // (voir `win32::injection`).
                    crate::win32::injection::regler_retenue(crate::win32::ouverture_fenetres::duree_fermeture_ms());
                    config::save_effet_fermeture_duree(crate::win32::ouverture_fenetres::duree_fermeture_ms());
                }
            }
            // ⭐ Animer AUSSI les fenêtres des autres applications — voir
            // `win32::injection` (désactivé par défaut).
            ["effet-injection", flag] => {
                let actif = *flag == "1";
                crate::win32::injection::activer(actif);
                config::save_bool_setting(config::EFFET_INJECTION, actif);
            }
            // ⭐ DEMANDE EXPLICITE (2026-11-04) : « proposer ces effets » —
            // la forme du retour, choisie dans la page des réglages.
            //
            // ⭐ DEMANDE EXPLICITE (2026-11-05) : « choisir n'importe quel effet
            // pour l'ouverture, la fermeture, la mise en avant et la réduction ».
            // Un verbe par moment ; le code 255 est « au hasard ».
            ["effet-fermeture-style", code] => {
                if let Ok(code) = code.parse::<u32>() {
                    use crate::win32::ouverture_fenetres::Moment;
                    crate::win32::ouverture_fenetres::regler_effet(Moment::Fermeture, code);
                    // ⭐ DEMANDE EXPLICITE (2026-11-05) : « je ne peux pas
                    // activer simplement un des nouveaux effets ». Choisir une
                    // forme ALLUME le moment : c'est le geste d'activation
                    // attendu, la case n'étant plus qu'un interrupteur pour
                    // l'éteindre. Sans cela, « Éclater » restait sans effet
                    // tant que la case « Faire retourner… » n'était pas cochée
                    // — un réglage que rien n'annonçait.
                    crate::win32::ouverture_fenetres::activer_fermeture(true);
                    config::save_bool_setting(config::EFFET_FERMETURE, true);
                    config::save_effet_fermeture_style(crate::win32::ouverture_fenetres::effet_code(Moment::Fermeture));
                }
            }
            ["effet-ouverture-style", code] => {
                if let Ok(code) = code.parse::<u32>() {
                    use crate::win32::ouverture_fenetres::Moment;
                    crate::win32::ouverture_fenetres::regler_effet(Moment::Ouverture, code);
                    crate::win32::ouverture_fenetres::activer(true);
                    crate::win32::injection::regler_ouverture(crate::win32::ouverture_fenetres::duree_ms());
                    config::save_bool_setting(config::EFFET_OUVERTURE, true);
                    config::save_effet_ouverture_style(crate::win32::ouverture_fenetres::effet_code(Moment::Ouverture));
                }
            }
            ["effet-reduction-style", code] => {
                if let Ok(code) = code.parse::<u32>() {
                    use crate::win32::ouverture_fenetres::Moment;
                    crate::win32::ouverture_fenetres::regler_effet(Moment::Reduction, code);
                    // Choisir une forme ALLUME le geste (voir le commentaire
                    // d'`effet-fermeture-style`) ; les quatre gestes de
                    // fermeture partagent l'interrupteur.
                    crate::win32::ouverture_fenetres::activer_fermeture(true);
                    crate::win32::injection::regler_retenue(crate::win32::ouverture_fenetres::duree_fermeture_ms());
                    config::save_bool_setting(config::EFFET_FERMETURE, true);
                    config::save_effet_reduction_style(crate::win32::ouverture_fenetres::effet_code(Moment::Reduction));
                }
            }
            ["effet-agrandissement-style", code] => {
                if let Ok(code) = code.parse::<u32>() {
                    use crate::win32::ouverture_fenetres::Moment;
                    crate::win32::ouverture_fenetres::regler_effet(Moment::Agrandissement, code);
                    crate::win32::ouverture_fenetres::activer_fermeture(true);
                    crate::win32::injection::regler_retenue(crate::win32::ouverture_fenetres::duree_fermeture_ms());
                    config::save_bool_setting(config::EFFET_FERMETURE, true);
                    config::save_effet_agrandissement_style(crate::win32::ouverture_fenetres::effet_code(
                        Moment::Agrandissement,
                    ));
                }
            }
            // ⭐ DEMANDE EXPLICITE (2026-10-25) : « un effet de fondu paramétrable à
            // l'ouverture et à la fermeture des fenêtres neuradesk ».
            ["fondu-neuradesk", flag] => {
                let actif = *flag == "1";
                crate::win32::ouverture_fenetres::activer_fondu(actif);
                config::save_bool_setting(config::FONDU_NEURADESK, actif);
            }
            ["fondu-neuradesk-duree", sens, valeur] => {
                if let Ok(ms) = valeur.parse::<f32>() {
                    use crate::win32::ouverture_fenetres as effet;
                    match *sens {
                        "ouverture" => effet::regler_duree_fondu_ouverture(ms.round() as u32),
                        "fermeture" => effet::regler_duree_fondu_fermeture(ms.round() as u32),
                        _ => return,
                    }
                    config::save_fondu_neuradesk_durees(effet::duree_fondu_ouverture_ms(), effet::duree_fondu_fermeture_ms());
                }
            }
            ["effet-ouverture-duree", valeur] => {
                if let Ok(ms) = valeur.parse::<f32>() {
                    crate::win32::ouverture_fenetres::regler_duree(ms.round() as u32);
                    // La DLL voile pour cette même durée (si l'effet est actif).
                    if crate::win32::ouverture_fenetres::est_active() {
                        crate::win32::injection::regler_ouverture(crate::win32::ouverture_fenetres::duree_ms());
                    }
                    config::save_effet_ouverture_duree(crate::win32::ouverture_fenetres::duree_ms());
                }
            }
            ["jeux-en-fenetre", flag] => {
                let active = *flag == "1";
                crate::win32::jeux_fenetres::activer(active);
                config::save_bool_setting(config::JEUX_EN_FENETRE, active);
            }
            ["fullscreen-clock", "enabled", flag] => {
                let enabled = *flag == "1";
                crate::win32::fullscreen_clock::set_enabled(enabled);
                config::save_fullscreen_clock_enabled(enabled);
            }
            ["fullscreen-clock", "position", position] => {
                let top = *position != "bottom";
                crate::win32::fullscreen_clock::set_position_top(top);
                config::save_fullscreen_clock_position_top(top);
            }
            ["fullscreen-clock", "bar-scale", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    let scale = percent as f32 / 100.0;
                    crate::win32::fullscreen_clock::set_bar_scale(scale);
                    config::save_fullscreen_clock_bar_scale(scale);
                }
            }
            // `fullscreen-clock-decalage/<écran>/<x|y>/<px>`, ou `…/<écran>/suit`
            // (un secondaire reprend celui du principal).
            ["fullscreen-clock-decalage", ecran, "suit"] => {
                crate::win32::fullscreen_clock::set_decalage(&percent_decode(ecran), None);
            }
            ["fullscreen-clock-decalage", ecran, axe, px] => match (*axe, px.parse::<i32>()) {
                ("x", Ok(px)) => crate::win32::fullscreen_clock::set_decalage(&percent_decode(ecran), Some((0, px))),
                ("y", Ok(px)) => crate::win32::fullscreen_clock::set_decalage(&percent_decode(ecran), Some((1, px))),
                _ => crate::diag!("[settings] décalage d'horloge illisible : {axe:?} {px:?}"),
            },
            ["fullscreen-clock", "text-scale", percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    let scale = percent as f32 / 100.0;
                    crate::win32::fullscreen_clock::set_text_scale(scale);
                    config::save_fullscreen_clock_text_scale(scale);
                }
            }
            // "settings/secondary-screen-fullscreen-clock-enabled/<device_name>/
            // <auto|0|1>" — voir la doc de `SettingsRefs::
            // secondary_screen_fullscreen_clock_enabled`, DEMANDE EXPLICITE
            // ("pouvoir régler individuellement les horloges en plein écran
            // en fonction de l'écran où elles s'affichent"). `"auto"` retire
            // le réglage manuel (suit à nouveau la barre PRIMAIRE) ; `"0"`/
            // `"1"` le fixe explicitement pour CET écran — même principe que
            // `secondary-screen-auto-color-scheme`.
            ["secondary-screen-fullscreen-clock-enabled", device_name, "auto"] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_fullscreen_clock_enabled.remove(&device_name);
                config::save_secondary_screen_fullscreen_clock_enabled(refs.secondary_screen_fullscreen_clock_enabled);
                *refs.secondary_screens_dirty = true;
            }
            ["secondary-screen-fullscreen-clock-enabled", device_name, flag] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_fullscreen_clock_enabled.insert(device_name, *flag == "1");
                config::save_secondary_screen_fullscreen_clock_enabled(refs.secondary_screen_fullscreen_clock_enabled);
                *refs.secondary_screens_dirty = true;
            }
            // "settings/secondary-screen-fullscreen-clock-position/<device_name>/
            // <auto|top|bottom>" — même principe que le réglage ci-dessus.
            ["secondary-screen-fullscreen-clock-position", device_name, "auto"] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_fullscreen_clock_position_top.remove(&device_name);
                config::save_secondary_screen_fullscreen_clock_position_top(refs.secondary_screen_fullscreen_clock_position_top);
                *refs.secondary_screens_dirty = true;
            }
            ["secondary-screen-fullscreen-clock-position", device_name, position] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_fullscreen_clock_position_top.insert(device_name, *position != "bottom");
                config::save_secondary_screen_fullscreen_clock_position_top(refs.secondary_screen_fullscreen_clock_position_top);
                *refs.secondary_screens_dirty = true;
            }
            // "settings/secondary-screen-fullscreen-clock-bar-scale/<device_name>/
            // <auto|percent>" — même principe que le réglage ci-dessus.
            ["secondary-screen-fullscreen-clock-bar-scale", device_name, "auto"] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_fullscreen_clock_bar_scale.remove(&device_name);
                config::save_secondary_screen_fullscreen_clock_bar_scale(refs.secondary_screen_fullscreen_clock_bar_scale);
                *refs.secondary_screens_dirty = true;
            }
            ["secondary-screen-fullscreen-clock-bar-scale", device_name, percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    let device_name = percent_decode(device_name);
                    refs.secondary_screen_fullscreen_clock_bar_scale.insert(device_name, percent as f32 / 100.0);
                    config::save_secondary_screen_fullscreen_clock_bar_scale(refs.secondary_screen_fullscreen_clock_bar_scale);
                    *refs.secondary_screens_dirty = true;
                }
            }
            // "settings/secondary-screen-fullscreen-clock-text-scale/<device_name>/
            // <auto|percent>" — même principe que le réglage ci-dessus.
            ["secondary-screen-fullscreen-clock-text-scale", device_name, "auto"] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_fullscreen_clock_text_scale.remove(&device_name);
                config::save_secondary_screen_fullscreen_clock_text_scale(refs.secondary_screen_fullscreen_clock_text_scale);
                *refs.secondary_screens_dirty = true;
            }
            ["secondary-screen-fullscreen-clock-text-scale", device_name, percent] => {
                if let Ok(percent) = percent.parse::<u32>() {
                    let device_name = percent_decode(device_name);
                    refs.secondary_screen_fullscreen_clock_text_scale.insert(device_name, percent as f32 / 100.0);
                    config::save_secondary_screen_fullscreen_clock_text_scale(refs.secondary_screen_fullscreen_clock_text_scale);
                    *refs.secondary_screens_dirty = true;
                }
            }
            ["menu-corner-radius", px] => {
                if let Ok(px) = px.parse::<f32>() {
                    menu::set_menu_corner_radius(px);
                    config::save_menu_corner_radius(px);
                }
            }
            ["menu-tint", percent] => {
                if let Ok(percent) = percent.parse::<u8>() {
                    let tint = 1.0 - percent as f32 / 100.0;
                    menu::set_menu_tint(tint);
                    config::save_menu_tint(tint);
                }
            }
            ["accent-icon-bg", window, flag] => {
                if let Some(target) = accent_target(refs, window) {
                    target.icon_bg = *flag == "1";
                    config::save_window_accent(window_static(window), target.background, target.icon_bg);
                }
            }
            ["accent-palette-enabled", window, flag] => {
                if let Some(target) = accent_target(refs, window) {
                    if *flag == "1" {
                        let seed = target.effective_palette();
                        target.palette = Some(seed);
                        config::save_window_palette_accent(window_static(window), seed);
                    } else {
                        target.palette = None;
                        config::delete_window_palette_accent(window_static(window));
                    }
                }
            }
            // "settings/secondary-screen/<device_name>/<mode>" — voir la doc
            // de `SettingsRefs::secondary_screen_modes`. `device_name`
            // (ex. `\\.\DISPLAY2`) est encodé côté JS par `go()` (comme
            // n'importe quel segment, voir `assets/popups/settings.html`) —
            // ses antislashs sortent donc sous forme `%5C`, à décoder ici
            // même si la valeur ne contient jamais de VRAI `/` (même principe
            // que `win32::cef_sound`, voir `win32::cef_app::percent_decode`).
            ["secondary-screen", device_name, mode] => {
                let device_name = percent_decode(device_name);
                if *mode == "off" {
                    refs.secondary_screen_modes.remove(&device_name);
                } else {
                    refs.secondary_screen_modes.insert(device_name, mode.to_string());
                }
                config::save_secondary_screen_modes(refs.secondary_screen_modes);
                *refs.secondary_screens_dirty = true;
            }
            // "settings/secondary-screen-thickness/<device_name>/auto" (retour
            // à la dérivation automatique) ou ".../<device_name>/<top|left|
            // right>/<px>" (réglage manuel) — voir la doc de `SettingsRefs::
            // secondary_screen_thicknesses`, DEMANDE EXPLICITE ("hauteur,
            // largeur... séparés entre chaque écran").
            ["secondary-screen-thickness", device_name, "auto"] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_thicknesses.remove(&device_name);
                config::save_secondary_screen_thicknesses(refs.secondary_screen_thicknesses);
                *refs.secondary_screens_dirty = true;
            }
            ["secondary-screen-thickness", device_name, bar, px] => {
                let device_name = percent_decode(device_name);
                if let (Some(bar), Ok(px)) = (parse_bar(bar), px.parse::<f32>()) {
                    let entry = refs.secondary_screen_thicknesses.entry(device_name).or_insert([0.0; 3]);
                    entry[bar_index(bar)] = px;
                    config::save_secondary_screen_thicknesses(refs.secondary_screen_thicknesses);
                    *refs.secondary_screens_dirty = true;
                }
            }
            // Même principe que `secondary-screen-thickness` ci-dessus, pour
            // la transparence (voir `SettingsRefs::secondary_screen_tint`,
            // `bar-tint` pour l'équivalent PRIMAIRE).
            ["secondary-screen-tint", device_name, "auto"] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_tint.remove(&device_name);
                config::save_secondary_screen_tint(refs.secondary_screen_tint);
                *refs.secondary_screens_dirty = true;
            }
            ["secondary-screen-tint", device_name, bar, percent] => {
                let device_name = percent_decode(device_name);
                if let (Some(bar), Ok(percent)) = (parse_bar(bar), percent.parse::<u8>()) {
                    let entry = refs.secondary_screen_tint.entry(device_name).or_insert([0.0; 3]);
                    entry[bar_index(bar)] = 1.0 - percent as f32 / 100.0;
                    config::save_secondary_screen_tint(refs.secondary_screen_tint);
                    *refs.secondary_screens_dirty = true;
                }
            }
            // Même principe, pour la couleur de fond (voir `SettingsRefs::
            // secondary_screen_colors`, `color`/`apply_bar_color` pour
            // l'équivalent PRIMAIRE) — `"auto"` en dernier segment réinitialise
            // UNE SEULE barre (contrairement à `secondary-screen-thickness`/
            // `-tint` ci-dessus qui réinitialisent les trois d'un coup, aucune
            // demande explicite de granularité fine pour ces deux-là) : suit
            // `apply_color_to_target`, qui cible aussi une seule barre à la fois.
            ["secondary-screen-color", device_name, bar, "auto"] => {
                let device_name = percent_decode(device_name);
                if let Some(bar) = parse_bar(bar) {
                    if let Some(entry) = refs.secondary_screen_colors.get_mut(&device_name) {
                        entry[bar_index(bar)] = None;
                        if entry.iter().all(Option::is_none) {
                            refs.secondary_screen_colors.remove(&device_name);
                        }
                    }
                    config::save_secondary_screen_colors(refs.secondary_screen_colors);
                    *refs.secondary_screens_dirty = true;
                }
            }
            ["secondary-screen-color", device_name, bar, hex] => {
                let device_name = percent_decode(device_name);
                if let (Some(bar), Some(rgb)) = (parse_bar(bar), parse_hex_rgb(hex)) {
                    let entry = refs.secondary_screen_colors.entry(device_name).or_insert([None; 3]);
                    entry[bar_index(bar)] = Some([rgb[0], rgb[1], rgb[2], color::DOCK_ALPHA]);
                    config::save_secondary_screen_colors(refs.secondary_screen_colors);
                    *refs.secondary_screens_dirty = true;
                }
            }
            // "settings/secondary-screen-feature/<device_name>/<name>/<0|1>"
            // — voir la doc de `SettingsRefs::secondary_screen_features`,
            // DEMANDE EXPLICITE ("sélectionner ou déselectionner des
            // éléments des barres secondaires pour les rendre visibles ou
            // invisibles"). La liste stockée n'est PAS "les éléments
            // désactivés pour cet écran" mais "les éléments dont l'état
            // diffère de leur valeur par défaut" (voir `app::
            // resolve_secondary_features`, doc de fonction, pour le BUG RÉEL
            // CORRIGÉ que cette distinction règle : "le fait de décocher
            // réseau ne permet pas de le faire réapparaitre en le
            // resélectionnant" — `network` est désactivé PAR DÉFAUT sur un
            // écran secondaire, voir `app::SECONDARY_DEFAULT_DISABLED_
            // FEATURES` ; l'ancienne liste "toujours désactiver" ne pouvait
            // donc jamais le RÉACTIVER). `desired` (le nouvel état demandé)
            // détermine ajout/retrait de `name` selon qu'il s'écarte ou non
            // de `app::secondary_default_feature_enabled` — inconnu de ce
            // module, calculé là-bas pour rester la SEULE source de vérité
            // sur la liste `SECONDARY_DEFAULT_DISABLED_FEATURES`.
            ["secondary-screen-feature", device_name, name, flag] => {
                let device_name = percent_decode(device_name);
                let desired = *flag == "1";
                let default = crate::app::secondary_default_feature_enabled(&*refs.features, name);
                let entry = refs.secondary_screen_features.entry(device_name.clone()).or_default();
                entry.retain(|n| n != name);
                if default != Some(desired) {
                    entry.push((*name).to_string());
                }
                if entry.is_empty() {
                    refs.secondary_screen_features.remove(&device_name);
                }
                config::save_secondary_screen_features(refs.secondary_screen_features);
                *refs.secondary_screens_dirty = true;
            }
            // "settings/secondary-screen-auto-color-scheme/<device_name>/
            // <auto|0|1>" — voir la doc de `SettingsRefs::
            // secondary_screen_auto_color_scheme`, DEMANDE EXPLICITE ("les
            // mêmes possibilités de réglages pour l'interface secondaire que
            // pour l'interface principale"). `"auto"` retire le réglage
            // manuel (suit à nouveau la barre PRIMAIRE) ; `"0"`/`"1"` le fixe
            // explicitement pour CET écran — même principe que
            // `auto-color-scheme` pour l'équivalent PRIMAIRE.
            ["secondary-screen-auto-color-scheme", device_name, "auto"] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_auto_color_scheme.remove(&device_name);
                config::save_secondary_screen_auto_color_scheme(refs.secondary_screen_auto_color_scheme);
                *refs.secondary_screens_dirty = true;
            }
            ["secondary-screen-auto-color-scheme", device_name, flag] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_auto_color_scheme.insert(device_name, *flag == "1");
                config::save_secondary_screen_auto_color_scheme(refs.secondary_screen_auto_color_scheme);
                *refs.secondary_screens_dirty = true;
            }
            // "settings/secondary-screen-topbar-text-color/<device_name>/
            // <auto|hex>" — voir la doc de `SettingsRefs::
            // secondary_screen_topbar_text_color`, même principe que
            // `topbar_text` (couleur cible du sélecteur PRIMAIRE) pour
            // l'équivalent PRIMAIRE. Sans effet tant que la "Colorisation
            // automatique" de CET écran (ci-dessus) est active.
            ["secondary-screen-topbar-text-color", device_name, "auto"] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_topbar_text_color.remove(&device_name);
                config::save_secondary_screen_topbar_text_color(refs.secondary_screen_topbar_text_color);
                *refs.secondary_screens_dirty = true;
            }
            ["secondary-screen-topbar-text-color", device_name, hex] => {
                let device_name = percent_decode(device_name);
                if let Some(rgb) = parse_hex_rgb(hex) {
                    refs.secondary_screen_topbar_text_color.insert(device_name, rgb);
                    config::save_secondary_screen_topbar_text_color(refs.secondary_screen_topbar_text_color);
                    *refs.secondary_screens_dirty = true;
                }
            }
            // "settings/secondary-screen-scale/<device_name>/auto" (retire
            // TOUS les réglages manuels de cet écran d'un coup, voir la doc
            // de `SettingsRefs::secondary_screen_scales` — un seul
            // interrupteur "Automatique" pour les 7 champs, comme
            // `secondary-screen-thickness`/`-tint`) ; ".../init" (décoché →
            // amorce l'entrée avec les 7 valeurs PRIMAIRES courantes, un bon
            // point de départ plutôt qu'un défaut arbitraire — même
            // raisonnement que `secondaryScreenThickness`/`secondaryScreenTint`
            // côté JS, voir `assets/popups/settings.html`) ; ".../<champ>/
            // <percent>" (ajustement individuel d'un des 7 curseurs une fois
            // l'entrée créée).
            // ⭐ **LE FORÇAGE GLOBAL DU RATIO DE DPI** — voir
            // `config::load_ratio_dpi_force`. `0` l'éteint ; la page envoie `0`
            // quand la case est décochée, et le ratio automatique quand elle est
            // cochée (elle le connaît, le payload le lui pousse).
            ["ratio-dpi-force", percent] => {
                let v = percent.parse::<f32>().unwrap_or(0.0) / 100.0;
                // ⚠️ **LE PLANCHER EST `0.0`, PAS `0.25`** : `0.0` est la sentinelle
                // « éteint » (voir `config::RATIO_DPI_AUTO`), et elle doit rester
                // atteignable — c'est le seul moyen de rendre la main à l'automatique.
                *refs.ratio_dpi_force = if v < 0.25 { 0.0 } else { v.clamp(0.25, 3.0) };
                config::save_ratio_dpi_force(*refs.ratio_dpi_force);
            }
            ["secondary-screen-scale", device_name, "auto"] => {
                let device_name = percent_decode(device_name);
                refs.secondary_screen_scales.remove(&device_name);
                config::save_secondary_screen_scales(refs.secondary_screen_scales);
            }
            ["secondary-screen-scale", device_name, "init"] => {
                let device_name = percent_decode(device_name);
                let seed = [
                    menu::menu_scale(),
                    cef_dash::icon_scale(),
                    cef_dash::label_scale(),
                    cef_dash::category_scale(),
                    cef_clipboard::thumb_scale(),
                    cef_clipboard::text_scale(),
                    content_scale(),
                    // ⭐ **LE RATIO AUTOMATIQUE DE CET ÉCRAN, ET SURTOUT PAS `1.0`.**
                    // Amorcés avec `1.0`, les quatre curseurs neufs MENTIRAIENT : le
                    // ratio de DPI afficherait « 100 % » sur un écran où la
                    // correction applique déjà 67 %. L'utilisateur doit VOIR ce qui
                    // est appliqué pour pouvoir le corriger.
                    crate::win32::dpi::ratio_dpi_pour_ecran(&device_name),
                ];
                refs.secondary_screen_scales.insert(device_name, seed);
                config::save_secondary_screen_scales(refs.secondary_screen_scales);
            }
            ["secondary-screen-scale", device_name, field, percent] => {
                let device_name = percent_decode(device_name);
                if let (Some(index), Ok(percent)) = (secondary_scale_field_index(field), percent.parse::<u32>()) {
                    let scale = (percent as f32 / 100.0).clamp(0.25, 2.0);
                    let entry = refs.secondary_screen_scales.entry(device_name).or_insert(config::champs_par_defaut());
                    entry[index] = scale;
                    config::save_secondary_screen_scales(refs.secondary_screen_scales);
                }
            }
            ["copy-hex", hex_code] => {
                // Le booléen est ignoré : cette page n'a pas d'endroit où dire
                // l'échec, et le presse-papiers occupé est un cas banal.
                let _ = clipboard::write_text(&format!("#{hex_code}"));
            }
            ["dash-tint", percent] => {
                if let Ok(percent) = percent.parse::<u8>() {
                    *refs.dash_tint = 1.0 - percent as f32 / 100.0;
                    config::save_dash_tint(*refs.dash_tint);
                }
            }
            _ => eprintln!("[neuradesk][cef] action réglages inconnue : {action:?}"),
        }
    }
}

/// Les identifiants de cible sont un ensemble FIXE et connu à la
/// compilation (voir `apply_color_to_target`) — convertit la `String` reçue
/// de JS vers l'une de ces constantes `'static`, `None` pour toute autre
/// valeur (JS malveillant/bug, jamais un identifiant arbitraire fuité).
fn leak_known_target(target: &str) -> Option<&'static str> {
    const TARGETS: &[&str] = &[
        "bar_top",
        "bar_left",
        "bar_right",
        "bar_bottom",
        "topbar_text",
        "popup",
        "dash",
        "settings",
        "clipboard",
        "notes",
        "dash_palette",
        "settings_palette",
        "clipboard_palette",
        "notes_palette",
        // ⚠️ "calendar" MANQUAIT ICI : `window_static("calendar")` retombait
        // sur "dash", donc cocher une case du Calendrier écrivait sa couleur
        // dans le fichier du DASH.
        "calendar",
        "calendar_palette",
        "minuteur",
        "minuteur_palette",
        "surveillance",
        "surveillance_palette",
        "calculatrice",
        "calculatrice_palette",
        "ssh",
        "ssh_palette",
        "terminal",
        "terminal_palette",
        "dossier",
        "dossier_palette",
        "visionneuse",
        "visionneuse_palette",
        "couleurs",
        "couleurs_palette",
        "editeur",
        "editeur_palette",
        "scanner",
        "scanner_palette",
        "shaders",
        "shaders_palette",
        "lueurs",
        "lueurs_palette",
        "textures",
        "textures_palette",
        "moniteur",
        "moniteur_palette",
        "recherche",
        "recherche_palette",
        "automatisations",
        "automatisations_palette",
        "meteo",
        "meteo_palette",
        "aide",
        "aide_palette",
        "theme_base_color",
    ];
    TARGETS.iter().copied().find(|t| *t == target)
}

fn window_static(window: &str) -> &'static str {
    leak_known_target(window).unwrap_or("dash")
}

fn accent_target<'a, 'b>(refs: &'a mut SettingsRefs<'b>, window: &str) -> Option<&'a mut color::WindowAccent> {
    match window {
        "dash" => Some(refs.dash_accent),
        "settings" => Some(refs.settings_accent),
        "clipboard" => Some(refs.clipboard_accent),
        "notes" => Some(refs.notes_accent),
        // La fenêtre du calendrier — DEMANDE EXPLICITE (2026-09-23) : « ajoutes
        // les réglages pour le dash calendrier dans le dash réglages ».
        "calendar" => Some(refs.calendar_accent),
        "minuteur" => Some(refs.minuteur_accent),
        "surveillance" => Some(refs.surveillance_accent),
        "calculatrice" => Some(refs.calculatrice_accent),
        "ssh" => Some(refs.ssh_accent),
        "terminal" => Some(refs.terminal_accent),
        "dossier" => Some(refs.dossier_accent),
        "visionneuse" => Some(refs.visionneuse_accent),
        "couleurs" => Some(refs.couleurs_accent),
        "editeur" => Some(refs.editeur_accent),
        "scanner" => Some(refs.scanner_accent),
        "shaders" => Some(refs.shaders_accent),
        "lueurs" => Some(refs.lueurs_accent),
        "textures" => Some(refs.textures_accent),
        "moniteur" => Some(refs.moniteur_accent),
        "recherche" => Some(refs.recherche_accent),
        "automatisations" => Some(refs.automatisations_accent),
        "meteo" => Some(refs.meteo_accent),
        "aide" => Some(refs.aide_accent),
        _ => None,
    }
}

fn apply_feature(features: &mut Features, name: &str, enabled: bool) {
    if crate::app::set_feature_by_name(features, name, enabled) {
        config::save_feature_enabled(name, enabled);
    }
}

/// Une barre de la barre PRINCIPALE, barre du bas comprise — voir `parse_bar`.
fn parse_barre_principale(name: &str) -> Option<Bar> {
    match name {
        "bottom" => Some(Bar::Bottom),
        autre => parse_bar(autre),
    }
}

/// ⛔ **SANS LA BARRE DU BAS, ET C'EST VOULU** : elle sert aussi aux écrans
/// SECONDAIRES, dont les tableaux par barre n'ont que trois cases
/// (`[None; 3]`) — `bar_index(Bar::Bottom)` vaut 3 et y paniquerait. Pour la
/// barre principale, voir `parse_barre_principale`.
fn parse_bar(name: &str) -> Option<Bar> {
    match name {
        "top" => Some(Bar::Top),
        "left" => Some(Bar::Left),
        "right" => Some(Bar::Right),
        _ => None,
    }
}

fn bar_index(bar: Bar) -> usize {
    match bar {
        Bar::Top => 0,
        Bar::Left => 1,
        Bar::Right => 2,
        Bar::Bottom => 3,
    }
}

/// Index dans `SettingsRefs::secondary_screen_scales`/le tableau `[f32; NB_CHAMPS_ECRAN]`
/// persisté par `config::save_secondary_screen_scales` — voir sa doc pour
/// l'ordre exact. Les identifiants correspondent aux segments envoyés par
/// `assets/popups/settings.html` (`secondaryScreenScales`), un par curseur.
fn secondary_scale_field_index(field: &str) -> Option<usize> {
    match field {
        // ⚠️ **LES NOMS DE CHAMPS VIENNENT DE LA PAGE** (`settings.html`, fonction
        // `secondaryScreenScales`) — les renommer ici sans les renommer là-bas
        // ferait échouer le réglage en silence, `None` étant indiscernable d'un
        // champ inconnu.
        "menu" => Some(crate::config::CHAMP_MENU),
        "dash-icon" => Some(crate::config::CHAMP_DASH_ICONE),
        "dash-label" => Some(crate::config::CHAMP_DASH_ETIQUETTE),
        "dash-category" => Some(crate::config::CHAMP_DASH_CATEGORIE),
        "clipboard-thumb" => Some(crate::config::CHAMP_PRESSE_MINIATURE),
        "clipboard-text" => Some(crate::config::CHAMP_PRESSE_TEXTE),
        "settings-content" => Some(crate::config::CHAMP_REGLAGES_CONTENU),
        // ⭐ Les quatre nouveaux — voir `config::CHAMP_RATIO_DPI`, qui porte le
        // piège du défaut à `0.0`.
        "dpi" => Some(crate::config::CHAMP_RATIO_DPI),
        _ => None,
    }
}

fn apply_bar_color(dock: &mut Dock, bar: Bar, rgb: [f32; 3]) {
    dock.set_bar_color(bar_index(bar), [rgb[0], rgb[1], rgb[2], color::DOCK_ALPHA]);
    config::save_bar_colors(dock.bar_colors().map(Some));
}

/// Extensions acceptées pour une vignette de dossier — le décodage passe par
/// `image`, donc tout ce qu'il sait lire ; on n'en propose que les usuelles.
/// Extensions proposées au dialogue « Choisir une vignette pour ce dossier ».
///
/// ⚠️ **`pub(crate)` DEPUIS QUE CE GESTE A DEUX POINTS D'ENTRÉE** : le panneau
/// Réglages, et le menu contextuel d'une vignette de la barre du bas (voir
/// `win32::cef_dossier_menu`). Une liste recopiée de l'autre côté aurait dérivé
/// au premier format ajouté d'un seul des deux — et rien ne l'aurait signalé.
pub(crate) const IMAGE_EXTENSIONS_DOSSIER: &str = "png;jpg;jpeg;bmp;gif;webp";

/// **L'OWNER D'UN DIALOGUE MODAL OUVERT DEPUIS CETTE PAGE** — `None` (et on
/// n'ouvre rien) si la fenêtre n'existe pas encore : une boîte sans propriétaire
/// peut passer DERRIÈRE, et l'application paraîtrait figée alors qu'elle attend
/// un choix invisible (voir `win32::file_dialog`, doc de module).
///
/// ⚠️ **UN DIALOGUE MODAL GÈLE LE REDESSIN DU DOCK** pendant qu'il est ouvert —
/// c'est documenté et accepté, et c'est déjà le cas des autres pages qui en
/// ouvrent un (`cef_couleurs`, `cef_notes`, `cef_image_editor`).
fn dialogue_possible() -> Option<HWND> {
    let hwnd = cef_popup::hwnd(&SLOT);
    if hwnd.is_none() {
        crate::diag!("[reglages] boite de dialogue refusee : la fenetre n'existe pas encore");
    }
    hwnd
}

fn message_de_sauvegarde(texte: String) {
    crate::diag!("[sauvegarde] {texte}");
    SAUVEGARDE_MESSAGE.with(|m| *m.borrow_mut() = texte);
}

/// **EXPORTE LA CONFIGURATION** — toute, ou l'apparence seule — dans l'archive
/// que l'utilisateur choisit.
fn exporter_la_configuration(sorte: crate::sauvegarde_config::Sorte) {
    use crate::sauvegarde_config::Sorte;
    let Some(hwnd) = dialogue_possible() else { return };
    let Some(dossier) = config::config_dir() else { return };
    let date = crate::sauvegarde_config::maintenant(false);
    let (titre, prefixe, quoi) = match sorte {
        Sorte::Complete => ("Exporter la configuration", "NeuraDesk-configuration", "Configuration exportée"),
        Sorte::Apparence => ("Exporter l'apparence", "NeuraDesk-apparence", "Apparence exportée"),
    };
    let nom = format!("{prefixe}-{}.zip", &date[..10]);
    let Some(archive) = crate::win32::file_dialog::enregistrer_sous(hwnd, titre, &nom, "zip", "Archive .zip") else {
        return; // Annuler n'est pas une erreur.
    };
    message_de_sauvegarde(match crate::sauvegarde_config::exporter(&dossier, &archive, &date, sorte) {
        Ok(n) => format!("{quoi} : {n} fichiers dans {}.", archive.display()),
        Err(err) => format!("Export impossible : {err}"),
    });
}

/// **PRÉPARE UN IMPORT** — l'archive est vérifiée et extraite à part ; la page
/// montre ce qu'elle contient et attend la confirmation. Rien ne change encore.
fn preparer_l_import() {
    let Some(hwnd) = dialogue_possible() else { return };
    let Some(dossier) = config::config_dir() else { return };
    let Some(archive) = crate::win32::file_dialog::ouvrir(hwnd, "Importer une configuration", "zip", "Archive .zip") else {
        return;
    };
    match crate::sauvegarde_config::preparer_import(&archive, &dossier) {
        Ok(apercu) => {
            SAUVEGARDE_APERCU.with(|a| *a.borrow_mut() = Some(apercu));
            message_de_sauvegarde(format!("Archive vérifiée : {}.", archive.display()));
        }
        Err(err) => {
            SAUVEGARDE_APERCU.with(|a| *a.borrow_mut() = None);
            message_de_sauvegarde(format!("Import refusé : {err}."));
        }
    }
}

/// **L'UTILISATEUR A CONFIRMÉ** — l'import s'appliquera au redémarrage, une fois
/// `app::run` terminé (voir `crate::sauvegarde_config`, doc de module).
fn confirmer_l_import() {
    let Some(dossier) = config::config_dir() else { return };
    if SAUVEGARDE_APERCU.with(|a| a.borrow_mut().take()).is_none() {
        return;
    }
    match crate::sauvegarde_config::confirmer_import(&dossier) {
        Ok(()) => {
            message_de_sauvegarde("Import confirmé : NeuraDesk redémarre.".to_string());
            crate::app::request_reload();
        }
        Err(err) => message_de_sauvegarde(format!("Import impossible : {err}")),
    }
}

/// **ÉCRIT LES RACCOURCIS DE DOSSIERS, PUIS DEMANDE UN REDÉMARRAGE** — le seul
/// chemin d'application pour tout ce qui les touche.
///
/// ⚠️ **LE REDÉMARRAGE N'EST PAS UN CONFORT, C'EST LA SEULE VOIE** : les
/// vignettes sont téléversées en texture et la composition du dock arrêtée au
/// LANCEMENT (voir `app::run`) — exactement la contrainte que porte déjà l'ajout
/// d'une épingle depuis le Dash (voir `win32::cef_dash::handle_add_to_zone`).
///
/// On ne redémarre **que si l'écriture a réussi** : redémarrer sur un échec
/// ferait perdre la session à l'utilisateur pour un changement qui n'a pas pris.
fn dossiers_ecrits(refs: &mut SettingsRefs<'_>, geste: crate::app::GesteDossier) -> bool {
    // ⛔ **PLUS DE `request_reload()` — MÊME CORRECTIF QUE LE `+` DE LA BARRE**, et pour
    // la même raison : signalé par l'utilisateur, « toute l'interface se recharge » pour un
    // simple raccourci de dossier. Ce n'était d'ailleurs pas un rechargement de PAGE :
    // `request_reload` fait redémarrer le PROCESSUS (voir sa doc).
    //
    // ⇒ **ET L'EFFET EST MAINTENANT IMMÉDIAT** (étape 4 du refactor des identités stables).
    // `7fa68e6` avait retiré le redémarrage sans rien mettre à la place : ces cinq gestes
    // étaient devenus les SEULS à n'avoir aucun effet visible sur la barre avant le
    // prochain lancement, alors que ceux du sous-dock, eux, agissaient tout de suite. Deux
    // moitiés du même geste qui se comportaient différemment.
    //
    // ⚠️ **L'ORDRE EST LE CONTRAT** (voir `app::DossiersVivants`) : le fichier est écrit
    // D'ABORD, et la barre n'est touchée que si l'écriture a réussi.
    //
    // ⚠️ **LE BOOLÉEN NE DIT QUE L'ÉCRITURE, PAS L'APPLICATION** — et c'est voulu :
    // - `false` ⇒ l'appelant DOIT remettre sa mutation de liste en place (rien n'a été
    //   écrit, rien ne doit rester) ;
    // - `true` avec une application ratée ⇒ on ne défait RIEN, parce que le FICHIER, lui,
    //   porte le changement : défaire la liste la ferait diverger du fichier, c'est-à-dire
    //   exactement le défaut que toute cette étape supprime. Le `diag!` d'`aplomb` a déjà
    //   dit pourquoi, et la barre suivra au prochain lancement — une DÉGRADATION, jamais
    //   une régression.
    if !config::save_barre_du_bas(refs.dossiers) {
        crate::diag!("[reglages] dossiers.toml n'a pas pu etre ecrit : le geste est ABANDONNE");
        return false;
    }
    let mut vivants = crate::app::vivants_dossiers(
        &mut *refs.dock,
        refs.emplacements_dossiers,
        refs.gfx,
        *refs.bottom_thickness,
    );
    vivants.appliquer(refs.dossiers, geste);
    true
}

fn apply_bar_tint(bar_tint: &mut [f32; 4], bar: Bar, tint_strength: f32) {
    // `.get_mut` plutôt qu'un index direct : le tableau porte QUATRE cases
    // (une par barre, voir `load_bar_tint`) et la page peut n'en nommer que
    // trois. Un index hors bornes ne doit pas tuer l'application pour un
    // réglage d'apparence.
    if let Some(slot) = bar_tint.get_mut(bar_index(bar)) {
        *slot = tint_strength;
    }
    config::save_bar_tint(*bar_tint);
}

/// Voir `win32::settings_dash::apply_theme` — même effet exact (couleur
/// PARTAGÉE des trois barres, des trois `WindowAccent`, des menus
/// contextuels ; repart d'une palette qui SUIT ce nouveau fond).
/// **LES FORCES DU THÈME** (saturation, luminosité) — voir `color::ajuster_theme`.
///
/// ⭐ **PAR DÉFAUT `FORCE_NEUTRE`** : tant que l'utilisateur ne règle rien, la
/// couleur choisie est appliquée telle quelle.
fn forces_du_theme() -> (f32, f32) {
    with_data(|d| (d.theme_force_saturation, d.theme_force_luminosite))
}

/// Les 23 fenêtres dont la « Couleur de mise en avant » est réglable
/// individuellement — mêmes noms que les listes `(target, window)` de
/// `apply_theme`/`apply_famille`/`reappliquer_forces`. Sert aux traitements qui
/// n'ont besoin QUE du nom (pas du champ `&mut WindowAccent`).
const FENETRES_ACCENT: [&str; 23] = [
    "dash", "settings", "clipboard", "notes", "calendar", "minuteur",
    "surveillance", "calculatrice", "ssh", "terminal", "dossier", "visionneuse",
    "couleurs", "editeur", "scanner", "shaders", "lueurs", "textures",
    "moniteur", "recherche", "automatisations", "meteo", "aide",
];

/// Les quatre barres du dock, avec la clé de base associée (même convention de
/// cible que le sélecteur de couleur : `"bar_top"`…). Les barres sont
/// ré-échelonnées depuis LEUR propre base, comme les panneaux — demande
/// explicite (2026-11-07 : « les barres perdent encore leurs couleurs » quand
/// on bouge les curseurs). Une barre n'a pas de couleur de palette séparée.
const BARRES: [(Bar, &str); 4] = [
    (Bar::Top, "bar_top"),
    (Bar::Left, "bar_left"),
    (Bar::Right, "bar_right"),
    (Bar::Bottom, "bar_bottom"),
];

/// Pose la MÊME base `(fond, palette)` à TOUS les panneaux ET à toutes les
/// barres — appelé quand un thème ou une famille vient d'appliquer une couleur
/// uniforme. Les curseurs repartiront de là (voir `SettingsData::theme_bases`,
/// `reappliquer_forces`).
fn poser_bases_uniformes(fond: [f32; 3], palette: Option<[f32; 3]>) {
    with_data(|d| {
        d.theme_bases.clear();
        for w in FENETRES_ACCENT {
            d.theme_bases.insert(w, (fond, palette));
        }
        for (_, nom) in BARRES {
            d.theme_bases.insert(nom, (fond, None));
        }
    });
}

/// INVALIDE la base mémorisée d'UN panneau après un choix manuel de couleur —
/// `cible` est p.ex. `"dash"` (fond) ou `"dash_palette"` (palette). Sans effet
/// si la cible n'est pas un panneau à accent (barres, séparateur, menu…).
///
/// ⛔ **ON RETIRE L'ENTRÉE, on ne la modifie pas à moitié** : le fond et la
/// palette d'un panneau sont deux réglages séparés. Mettre à jour l'un en
/// laissant l'autre à une valeur par défaut (ex. palette `None`) FAISAIT
/// disparaître l'autre au prochain geste de curseur (bug repéré 2026-11-07).
/// En retirant l'entrée, le prochain `reappliquer_forces` recapture les DEUX
/// composantes depuis l'état RÉEL du panneau — celui qu'on vient de choisir
/// pour l'une, celui déjà en place pour l'autre.
fn enregistrer_base_manuelle(cible: &str) {
    let nom = cible.strip_suffix("_palette").unwrap_or(cible);
    let connu = FENETRES_ACCENT
        .iter()
        .chain(BARRES.iter().map(|(_, n)| n))
        .chain(std::iter::once(&"centre_haut"))
        .copied()
        .find(|&w| w == nom);
    if let Some(window) = connu {
        with_data(|d| {
            d.theme_bases.remove(&window);
        });
    }
}

/// La couleur de SURFACE (hors forces) que suivent les barres et les menus : la
/// surface de la famille active si un thème Windows/famille l'est, sinon la
/// couleur de base du thème libre.
fn surface_base_courante() -> [f32; 3] {
    if WINDOWS_THEME_ACTIVE.with(Cell::get) {
        let id = famille_choisie();
        if let Some(famille) = famille_par_id(&id) {
            return match (color::mode_clair(), famille.clair) {
                (true, Some(clair)) => clair,
                _ => famille.sombre,
            };
        }
    }
    with_data(|d| d.theme_base_color)
}

/// ⭐ **LES CURSEURS SATURATION/LUMINOSITÉ RÉ-ÉCHELONNENT CHAQUE PANNEAU DEPUIS
/// SA PROPRE BASE** (`SettingsData::theme_bases`) — demande explicite
/// (2026-11-06). Remplace l'ancien `apply_theme`/`apply_famille` au geste de
/// curseur, qui reposait la même couleur partout et effaçait la
/// personnalisation individuelle. Les barres et les menus, eux, suivent la
/// surface commune du thème (voir `surface_base_courante`) : pas de
/// personnalisation par barre ici.
///
/// ⛔ **N'APPELLE NI `apply_theme` NI `apply_famille`** : c'est justement ce qui
/// éteignait la famille (défaut 2026-10-12) et écrasait les accents. Le mode
/// clair/sombre et l'état « famille active » ne changent pas sur un simple
/// geste de curseur, donc rien à y retoucher.
fn reappliquer_forces(refs: &mut SettingsRefs, saturation: f32, luminosite: f32) {
    // Chaque barre est ré-échelonnée depuis SA propre base (couleur brute
    // stockée, hors adoucissement `soften_tint`), comme les panneaux — sinon un
    // geste de curseur reposait la surface uniforme du thème sur les quatre et
    // effaçait leur personnalisation (défaut 2026-11-07).
    let brutes = refs.dock.bar_colors();
    for (bar, nom) in BARRES {
        let i = bar_index(bar);
        let base = with_data(|d| {
            d.theme_bases
                .entry(nom)
                .or_insert(([brutes[i][0], brutes[i][1], brutes[i][2]], None))
                .0
        });
        apply_bar_color(refs.dock, bar, color::ajuster_theme(base, saturation, luminosite));
    }
    // La zone centrale de la barre du haut (voir `win32::centre_haut`) a SA
    // propre couleur, indépendante du thème : on l'échelonne aussi depuis sa
    // base — demande 2026-11-07 (« la couleur de la zone centrale de la barre du
    // haut n'est pas modifiée par le réglage »).
    let centre = crate::win32::centre_haut::reglages().couleur;
    let centre = [centre[0] as f32 / 255.0, centre[1] as f32 / 255.0, centre[2] as f32 / 255.0];
    let base_centre = with_data(|d| d.theme_bases.entry("centre_haut").or_insert((centre, None)).0);
    config::save_ombre_centre_haut(&crate::win32::centre_haut::poser_couleur(color::ajuster_theme(
        base_centre,
        saturation,
        luminosite,
    )));
    // Le menu suit la surface commune du thème (pas de personnalisation par menu ici).
    let surface = color::ajuster_theme(surface_base_courante(), saturation, luminosite);
    menu::set_menu_accent_rgb(surface);
    config::save_window_accent("popup", surface, false);
    for (target, window) in [
        (&mut *refs.dash_accent, "dash"),
        (&mut *refs.settings_accent, "settings"),
        (&mut *refs.clipboard_accent, "clipboard"),
        (&mut *refs.notes_accent, "notes"),
        (&mut *refs.calendar_accent, "calendar"),
        (&mut *refs.minuteur_accent, "minuteur"),
        (&mut *refs.surveillance_accent, "surveillance"),
        (&mut *refs.calculatrice_accent, "calculatrice"),
        (&mut *refs.ssh_accent, "ssh"),
        (&mut *refs.terminal_accent, "terminal"),
        (&mut *refs.dossier_accent, "dossier"),
        (&mut *refs.visionneuse_accent, "visionneuse"),
        (&mut *refs.couleurs_accent, "couleurs"),
        (&mut *refs.editeur_accent, "editeur"),
        (&mut *refs.scanner_accent, "scanner"),
        (&mut *refs.shaders_accent, "shaders"),
        (&mut *refs.lueurs_accent, "lueurs"),
        (&mut *refs.textures_accent, "textures"),
        (&mut *refs.moniteur_accent, "moniteur"),
        (&mut *refs.recherche_accent, "recherche"),
        (&mut *refs.automatisations_accent, "automatisations"),
        (&mut *refs.meteo_accent, "meteo"),
        (&mut *refs.aide_accent, "aide"),
    ] {
        // Base mémorisée au dernier thème/famille/choix ; à défaut, la couleur
        // actuelle devient la base (premier geste d'une session fraîche).
        let (fond, palette) = with_data(|d| {
            *d.theme_bases.entry(window).or_insert((target.background, target.palette))
        });
        target.background = color::ajuster_theme(fond, saturation, luminosite);
        config::save_window_accent(window, target.background, target.icon_bg);
        target.palette = palette.map(|p| color::ajuster_theme(p, saturation, luminosite));
        match target.palette {
            Some(p) => config::save_window_palette_accent(window, p),
            None => config::delete_window_palette_accent(window),
        }
    }
}

fn apply_theme(refs: &mut SettingsRefs, rgb: [f32; 3]) {
    for bar in [Bar::Top, Bar::Left, Bar::Right, Bar::Bottom] {
        apply_bar_color(refs.dock, bar, rgb);
    }
    for (target, window) in
        [
            (&mut *refs.dash_accent, "dash"),
            (&mut *refs.settings_accent, "settings"),
            (&mut *refs.clipboard_accent, "clipboard"),
            (&mut *refs.notes_accent, "notes"),
            (&mut *refs.calendar_accent, "calendar"),
            (&mut *refs.minuteur_accent, "minuteur"),
            (&mut *refs.surveillance_accent, "surveillance"),
            (&mut *refs.calculatrice_accent, "calculatrice"),
            (&mut *refs.ssh_accent, "ssh"),
            (&mut *refs.terminal_accent, "terminal"),
            (&mut *refs.dossier_accent, "dossier"),
            (&mut *refs.visionneuse_accent, "visionneuse"),
            (&mut *refs.couleurs_accent, "couleurs"),
            (&mut *refs.editeur_accent, "editeur"),
            (&mut *refs.scanner_accent, "scanner"),
            (&mut *refs.shaders_accent, "shaders"),
            (&mut *refs.lueurs_accent, "lueurs"),
            (&mut *refs.textures_accent, "textures"),
            (&mut *refs.moniteur_accent, "moniteur"),
            (&mut *refs.recherche_accent, "recherche"),
            (&mut *refs.automatisations_accent, "automatisations"),
            (&mut *refs.meteo_accent, "meteo"),
            (&mut *refs.aide_accent, "aide"),
        ]
    {
        target.background = rgb;
        config::save_window_accent(window, rgb, target.icon_bg);
        target.palette = None;
        config::delete_window_palette_accent(window);
    }
    menu::set_menu_accent_rgb(rgb);
    config::save_window_accent("popup", rgb, false);
    // Tout thème est SOMBRE, sauf « Windows clair » qui repose le mode clair
    // juste après (voir `apply_windows_theme`) — et aucun ne suit Windows,
    // sauf les deux thèmes Windows.
    set_light_mode(false);
    set_windows_theme_active(false);
    color::set_accent_du_theme(None);
}

/// Nom du réglage booléen : « un thème Windows est sélectionné ».
const WINDOWS_THEME_ACTIVE_KEY: &str = "theme_windows_actif";

thread_local! {
    /// Un thème Windows (clair ou sombre) est-il sélectionné ? Lu dans la
    /// config au premier accès, tenu à jour par `set_windows_theme_active`.
    static WINDOWS_THEME_ACTIVE: Cell<bool> = Cell::new(config::load_bool_setting(WINDOWS_THEME_ACTIVE_KEY));
    /// Thème Windows à appliquer au prochain `poll` (`true` = clair).
    static WINDOWS_THEME_REQUEST: Cell<Option<bool>> = const { Cell::new(None) };
    /// La génération de `material_you` déjà vue par `suivre_le_fond_d_ecran`.
    static GENERATION_FOND_VUE: Cell<u64> = const { Cell::new(0) };
}

/// **LA FAMILLE « MATERIAL YOU (FOND D'ÉCRAN) » SE RÉAPPLIQUE QUAND LE FOND
/// D'ÉCRAN CHANGE** — dans la variante (claire ou sombre) en cours. Appelée à
/// chaque frame, Réglages fermés compris : `material_you::rafraichir` ne relit
/// le disque que toutes les deux secondes, et le fichier de la famille choisie
/// n'est lu qu'à une nouvelle analyse.
fn suivre_le_fond_d_ecran(refs: &mut SettingsRefs) {
    crate::win32::material_you::rafraichir();
    let generation = crate::win32::material_you::generation();
    if GENERATION_FOND_VUE.with(|g| g.replace(generation)) == generation {
        return;
    }
    if WINDOWS_THEME_ACTIVE.with(Cell::get) && famille_choisie() == color::ID_FOND_D_ECRAN {
        apply_famille(refs, color::ID_FOND_D_ECRAN, color::mode_clair());
    }
}

/// Une famille par son identifiant : une des familles fixes, ou celle tirée du
/// fond d'écran (`None` tant qu'il n'est pas analysé).
fn famille_par_id(id: &str) -> Option<color::FamilleTheme> {
    if id == color::ID_FOND_D_ECRAN {
        crate::win32::material_you::famille()
    } else {
        color::famille_theme(id).copied()
    }
}

fn set_windows_theme_active(active: bool) {
    if WINDOWS_THEME_ACTIVE.with(Cell::get) != active {
        WINDOWS_THEME_ACTIVE.with(|a| a.set(active));
        config::save_bool_setting(WINDOWS_THEME_ACTIVE_KEY, active);
    }
}

/// L'applet de bascule du thème de Windows vient de passer Windows en `dark`.
///
/// DEMANDE EXPLICITE (2026-09-26) : « quand un thème windows est sélectionné
/// un clic sur l'applet de bascule de thèmes windows [doit faire] suivre le
/// thème windows à l'interface de l'app ». Sans effet si un autre thème est
/// sélectionné : c'est un choix de l'utilisateur, la bascule ne l'écrase pas.
///
/// Seulement une DEMANDE, appliquée par `poll` : l'appelant (`app::run`) n'a
/// pas les `SettingsRefs` sous la main.
pub fn follow_windows_toggle(dark: bool) {
    if WINDOWS_THEME_ACTIVE.with(Cell::get) {
        WINDOWS_THEME_REQUEST.with(|r| r.set(Some(!dark)));
    }
}

/// Thèmes « Windows clair »/« Windows sombre » — DEMANDE EXPLICITE
/// (2026-09-26) : « 2 thèmes sélectionnables dans les réglages qui
/// correspondent au thème clair et au thème sombre de windows en reprenant les
/// couleurs de thème de windows ». Les surfaces Mica de Windows 11 (voir
/// `color::SURFACE_WINDOWS_CLAIR`), et la couleur d'accentuation de Windows
/// pour les mises en avant de chaque fenêtre.
///
/// ⭐ **DEPUIS LES FAMILLES** (voir `color::FAMILLES_THEMES`), c'est la variante
/// claire ou sombre de la famille CHOISIE — « Windows » par défaut, et c'est
/// elle que suit la bascule de l'applet.
fn apply_windows_theme(refs: &mut SettingsRefs, light: bool) {
    let id = famille_choisie();
    apply_famille(refs, &id, light);
}

/// Le fichier où la famille choisie est retenue (son `id`).
const FICHIER_FAMILLE: &str = "theme_famille.txt";

fn famille_choisie() -> String {
    crate::config::config_path(FICHIER_FAMILLE)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| t.trim().to_string())
        // La famille du fond d'écran est gardée même avant son analyse : au
        // lancement, ses couleurs arrivent quelques instants après.
        .filter(|id| id == color::ID_FOND_D_ECRAN || color::famille_theme(id).is_some())
        .unwrap_or_else(|| "windows".to_string())
}

/// Applique la variante claire ou sombre d'une famille, et la retient. Une
/// famille SANS variante claire (« Noir profond ») reste sombre.
fn apply_famille(refs: &mut SettingsRefs, id: &str, light: bool) {
    let Some(famille) = famille_par_id(id) else {
        crate::diag!("thème : famille inconnue ou fond d'écran pas encore analysé {id:?}");
        return;
    };
    let (surface, light) = match (light, famille.clair) {
        (true, Some(clair)) => (clair, true),
        _ => (famille.sombre, false),
    };
    // Les deux forces du thème valent AUSSI pour une famille (voir le verbe
    // `force-theme`) ; aux forces neutres, la surface passe à l'octet près.
    let (saturation, luminosite) = forces_du_theme();
    // La surface AVANT les forces est la base dont repartiront les curseurs.
    let surface_base = surface;
    let surface = color::ajuster_theme(surface, saturation, luminosite);
    if famille_choisie() != famille.id {
        config::write_config(FICHIER_FAMILLE, famille.id.to_string());
    }
    apply_theme(refs, surface);
    let accent = accent_de(&famille);
    for (target, window) in [
        (&mut *refs.dash_accent, "dash"),
        (&mut *refs.settings_accent, "settings"),
        (&mut *refs.clipboard_accent, "clipboard"),
        (&mut *refs.notes_accent, "notes"),
        (&mut *refs.calendar_accent, "calendar"),
        (&mut *refs.minuteur_accent, "minuteur"),
        (&mut *refs.surveillance_accent, "surveillance"),
        (&mut *refs.calculatrice_accent, "calculatrice"),
        (&mut *refs.ssh_accent, "ssh"),
        (&mut *refs.terminal_accent, "terminal"),
            (&mut *refs.dossier_accent, "dossier"),
            (&mut *refs.visionneuse_accent, "visionneuse"),
            (&mut *refs.couleurs_accent, "couleurs"),
            (&mut *refs.editeur_accent, "editeur"),
            (&mut *refs.scanner_accent, "scanner"),
            (&mut *refs.shaders_accent, "shaders"),
            (&mut *refs.lueurs_accent, "lueurs"),
            (&mut *refs.textures_accent, "textures"),
            (&mut *refs.moniteur_accent, "moniteur"),
            (&mut *refs.recherche_accent, "recherche"),
            (&mut *refs.automatisations_accent, "automatisations"),
            (&mut *refs.meteo_accent, "meteo"),
            (&mut *refs.aide_accent, "aide"),
    ] {
        target.palette = Some(accent);
        config::save_window_palette_accent(window, accent);
    }
    // ⭐ La zone centrale de la barre du haut prend la COULEUR SECONDAIRE du thème
    // — son `accent`, la seconde couleur de la famille (la surface étant la
    // première), celle que chaque panneau reçoit comme palette ci-dessus — pour
    // CONTRASTER avec la surface posée partout ailleurs (demande explicite :
    // « quand je clique sur un thème la zone centrale devrait prendre la couleur
    // secondaire »). On ne touche QUE la couleur : largeur, activation et fondu du
    // bloc restent ceux de l'utilisateur (voir `win32::centre_haut`).
    config::save_ombre_centre_haut(&crate::win32::centre_haut::poser_couleur(accent));
    // ⭐ L'indicateur d'application ouverte (le « running dot », voir
    // `ui::dock::Dock::running_dot_instance`) prend LUI AUSSI la couleur
    // secondaire du thème (demande explicite) — même raison que la zone centrale
    // ci-dessus : il contraste avec la surface posée sur les barres.
    let accent_rgba = [accent[0], accent[1], accent[2], 1.0];
    refs.dock.set_running_dot_color(Some(accent_rgba));
    config::save_running_dot_color(Some(accent_rgba));
    // Base de chaque panneau : surface pour le fond, accent pour la palette —
    // un curseur bougé ensuite les ré-échelonne tous les deux depuis là, sans
    // repasser par `apply_famille` (voir `reappliquer_forces`).
    poser_bases_uniformes(surface_base, Some(accent));
    set_light_mode(light);
    set_windows_theme_active(true);
    color::set_accent_du_theme(Some(accent));
}

/// L'accent d'une famille ; « Windows » prend celui du système, avec pour
/// retombée la colorisation DWM, proche de l'accent sans l'être (voir
/// `theme::accent_windows`).
fn accent_de(famille: &color::FamilleTheme) -> [f32; 3] {
    famille.accent.unwrap_or_else(|| {
        crate::win32::theme::accent_windows().unwrap_or_else(|| {
            let [r, g, b, _] = crate::win32::theme::detect_accent_color();
            [r, g, b]
        })
    })
}

/// Famille et variante d'une installation neuve : « Material You », clair —
/// DEMANDE EXPLICITE (2026-10-12).
const FAMILLE_DU_PREMIER_LANCEMENT: &str = "materialyou";

/// ⭐ **AU PREMIER LANCEMENT : LE THÈME « MATERIAL YOU » CLAIR.** Seulement quand
/// aucun thème n'a jamais été choisi (ni famille retenue, ni thème actif enregistré).
/// La famille est retenue tout de suite ; son APPLICATION (surfaces, accents de
/// chaque panneau, mode clair) passe par le premier `poll`, le seul à tenir les
/// `SettingsRefs` — exactement comme la bascule de l'applet de thème.
pub fn theme_du_premier_lancement() {
    let jamais_choisi = [FICHIER_FAMILLE, &format!("{WINDOWS_THEME_ACTIVE_KEY}.txt")]
        .iter()
        .all(|fichier| config::config_path(fichier).is_none_or(|chemin| !chemin.exists()));
    if !jamais_choisi {
        return;
    }
    config::write_config(FICHIER_FAMILLE, FAMILLE_DU_PREMIER_LANCEMENT.to_string());
    WINDOWS_THEME_REQUEST.with(|r| r.set(Some(true)));
    crate::diag!("premier lancement : theme {FAMILLE_DU_PREMIER_LANCEMENT} clair demande");
}

/// Au démarrage : repose `color::accent_du_theme` si une famille est choisie —
/// il n'est pas persisté, il se DÉDUIT de la famille retenue.
pub fn restaurer_accent_du_theme() {
    if WINDOWS_THEME_ACTIVE.with(Cell::get) {
        color::set_accent_du_theme(famille_par_id(&famille_choisie()).map(|f| accent_de(&f)));
    }
}

/// Les familles pour la page : surfaces et accent en hexadécimal (`null` :
/// pas de variante claire, ou l'accent de Windows).
///
/// La famille du fond d'écran se range juste après « Material You ».
fn familles_json() -> Vec<serde_json::Value> {
    let fond = crate::win32::material_you::famille();
    color::FAMILLES_THEMES
        .iter()
        .flat_map(|f| {
            let suivante = if f.id == "materialyou" { fond } else { None };
            std::iter::once(*f).chain(suivante)
        })
        .map(|f| json!({ "id": f.id, "nom": f.nom, "clair": f.clair.map(hex), "sombre": hex(f.sombre), "accent": f.accent.map(hex) }))
        .collect()
}

/// Pose le mode clair et le persiste — voir `color::mode_clair`.
fn set_light_mode(light: bool) {
    if color::mode_clair() != light {
        crate::diag!("thème : mode clair {}", if light { "activé" } else { "désactivé" });
    }
    color::set_mode_clair(light);
    config::save_bool_setting(color::CLE_MODE_CLAIR, light);
}

/// Cible dynamique `"secondary:<device_name>:<bar>"` (`bar` = `"bar_top"`/
/// `"bar_left"`/`"bar_right"`/`"topbar_text"`) — le pendant, pour les écrans
/// secondaires, des 13 cibles FIXES ci-dessous. Contrairement à elles,
/// `device_name` n'est pas connu à la compilation (dépend des moniteurs
/// branchés), d'où une chaîne construite dynamiquement plutôt qu'une entrée
/// de plus dans `leak_known_target` — voir `secondaryScreenColorTarget` côté
/// JS (`assets/popups/settings.html`), seul émetteur. DEMANDE EXPLICITE
/// ("mêmes possibilités de réglages... choix des couleurs... sélecteur de
/// couleur" pour les écrans secondaires, comme pour la barre primaire).
fn apply_secondary_color_target(rest: &str, rgb: [f32; 3], refs: &mut SettingsRefs) {
    let Some((device_name, bar)) = rest.rsplit_once(':') else { return };
    let device_name = device_name.to_string();
    if bar == "topbar_text" {
        refs.secondary_screen_topbar_text_color.insert(device_name, rgb);
        config::save_secondary_screen_topbar_text_color(refs.secondary_screen_topbar_text_color);
        *refs.secondary_screens_dirty = true;
        return;
    }
    // Réutilise le `colorPicker` riche (présélections/couleurs
    // personnalisées/pipette) plutôt qu'un simple champ hexadécimal — voir
    // `settings.html`, `secondaryScreenSeparatorColor` — les cas "auto"/
    // "transparent" (pas une couleur) restent gérés par l'action dédiée
    // `secondary-screen-separator-color`.
    if bar == "separator" {
        refs.secondary_screen_separator_colors.insert(device_name, [rgb[0], rgb[1], rgb[2], 1.0]);
        config::save_secondary_screen_separator_colors(refs.secondary_screen_separator_colors);
        *refs.secondary_screens_dirty = true;
        return;
    }
    let Some(bar) = parse_bar(bar.trim_start_matches("bar_")) else { return };
    let entry = refs.secondary_screen_colors.entry(device_name).or_insert([None; 3]);
    entry[bar_index(bar)] = Some([rgb[0], rgb[1], rgb[2], color::DOCK_ALPHA]);
    config::save_secondary_screen_colors(refs.secondary_screen_colors);
    *refs.secondary_screens_dirty = true;
}

/// Les 23 cibles d'accent « panneau / applet » (mêmes clés que `ACCENT_TARGETS`
/// dans `settings.html`) — énumérées pour le bouton « Tout en aléatoire »
/// (action `color-random-all`), qui tire une couleur par élément. Les menus
/// (`menu::MENU_COLOR_KEYS`) et les quatre barres sont ajoutés à côté, dans
/// l'action.
const PANNEAUX_ACCENT: [&str; 23] = [
    "dash", "settings", "clipboard", "notes", "calendar", "minuteur", "surveillance", "calculatrice", "ssh",
    "terminal", "dossier", "visionneuse", "couleurs", "editeur", "scanner", "shaders", "lueurs", "textures",
    "moniteur", "recherche", "automatisations", "meteo", "aide",
];

fn apply_color_to_target(target: &str, rgb: [f32; 3], refs: &mut SettingsRefs) {
    if let Some(rest) = target.strip_prefix("secondary:") {
        apply_secondary_color_target(rest, rgb, refs);
        return;
    }
    // Couleur INDIVIDUELLE d'un menu (`menu:context`, `menu:network`… voir
    // `menu::MENU_COLOR_KEYS`) — demande explicite : « personnaliser les menus
    // individuellement ». Préfixe DÉDIÉ pour ne pas entrer en collision avec
    // les cibles de panneau de même nom (ex. le panneau `calendar` et le menu
    // de l'horloge `menu:calendar`). `set_menu_color` ignore une clé inconnue.
    if let Some(key) = target.strip_prefix("menu:") {
        menu::set_menu_color(key, rgb);
        return;
    }
    // Les cinq couleurs du dégradé animé (`degrade:0` … `degrade:4`) — le
    // `colorPicker` partagé, pipette comprise, passe par ici.
    if let Some(i) = target.strip_prefix("degrade:").and_then(|i| i.parse::<usize>().ok()) {
        if crate::gfx::degrade::set_couleur(i, rgb) {
            crate::gfx::degrade::sauver_couleurs();
        }
        return;
    }
    match target {
        "bar_top" => apply_bar_color(refs.dock, Bar::Top, rgb),
        "bar_left" => apply_bar_color(refs.dock, Bar::Left, rgb),
        "bar_right" => apply_bar_color(refs.dock, Bar::Right, rgb),
        "bar_bottom" => apply_bar_color(refs.dock, Bar::Bottom, rgb),
        "topbar_text" => {
            *refs.topbar_text_color = rgb;
            config::save_topbar_text_color(rgb);
        }
        // Même principe que `apply_secondary_color_target` ci-dessus, pour
        // la barre PRIMAIRE — "auto"/"transparent" restent gérés par
        // l'action dédiée `separator-color`.
        "separator" => {
            let color = [rgb[0], rgb[1], rgb[2], 1.0];
            refs.dock.set_separator_color(Some(color));
            config::save_separator_color(Some(color));
        }
        // Couleur de l'indicateur « en cours d'exécution » (voir
        // `ui::dock::Dock::running_dot_color`) — "auto" (suivre l'accentuation
        // du dock) reste géré par l'action dédiée `running-dot-color`.
        "verr_num" | "verr_maj" => {
            let mut r = crate::ui::verrous::reglages();
            r.couleurs[usize::from(target == "verr_maj")] = rgb;
            crate::ui::verrous::set_reglages(r);
            config::save_verrous(r);
        }
        "centre_haut" => {
            config::save_ombre_centre_haut(&crate::win32::centre_haut::poser_couleur(rgb));
        }
        "running_dot" => {
            let color = [rgb[0], rgb[1], rgb[2], 1.0];
            refs.dock.set_running_dot_color(Some(color));
            config::save_running_dot_color(Some(color));
        }
        "popup" => {
            menu::set_menu_accent_rgb(rgb);
            config::save_window_accent("popup", rgb, false);
        }
        "dash" => {
            refs.dash_accent.background = rgb;
            config::save_window_accent("dash", rgb, refs.dash_accent.icon_bg);
        }
        "settings" => {
            refs.settings_accent.background = rgb;
            config::save_window_accent("settings", rgb, refs.settings_accent.icon_bg);
        }
        "clipboard" => {
            refs.clipboard_accent.background = rgb;
            config::save_window_accent("clipboard", rgb, refs.clipboard_accent.icon_bg);
        }
        "notes" => {
            refs.notes_accent.background = rgb;
            config::save_window_accent("notes", rgb, refs.notes_accent.icon_bg);
        }
        "calendar" => {
            refs.calendar_accent.background = rgb;
            config::save_window_accent("calendar", rgb, refs.calendar_accent.icon_bg);
        }
        "minuteur" => {
            refs.minuteur_accent.background = rgb;
            config::save_window_accent("minuteur", rgb, refs.minuteur_accent.icon_bg);
        }
        "surveillance" => {
            refs.surveillance_accent.background = rgb;
            config::save_window_accent("surveillance", rgb, refs.surveillance_accent.icon_bg);
        }
        "calculatrice" => {
            refs.calculatrice_accent.background = rgb;
            config::save_window_accent("calculatrice", rgb, refs.calculatrice_accent.icon_bg);
        }
        "ssh" => {
            refs.ssh_accent.background = rgb;
            config::save_window_accent("ssh", rgb, refs.ssh_accent.icon_bg);
        }
        "terminal" => {
            refs.terminal_accent.background = rgb;
            config::save_window_accent("terminal", rgb, refs.terminal_accent.icon_bg);
        }
        "dossier" => {
            refs.dossier_accent.background = rgb;
            config::save_window_accent("dossier", rgb, refs.dossier_accent.icon_bg);
        }
        "visionneuse" => {
            refs.visionneuse_accent.background = rgb;
            config::save_window_accent("visionneuse", rgb, refs.visionneuse_accent.icon_bg);
        }
        "couleurs" => {
            refs.couleurs_accent.background = rgb;
            config::save_window_accent("couleurs", rgb, refs.couleurs_accent.icon_bg);
        }
        "editeur" => {
            refs.editeur_accent.background = rgb;
            config::save_window_accent("editeur", rgb, refs.editeur_accent.icon_bg);
        }
        "scanner" => {
            refs.scanner_accent.background = rgb;
            config::save_window_accent("scanner", rgb, refs.scanner_accent.icon_bg);
        }
        "shaders" => {
            refs.shaders_accent.background = rgb;
            config::save_window_accent("shaders", rgb, refs.shaders_accent.icon_bg);
        }
        "lueurs" => {
            refs.lueurs_accent.background = rgb;
            config::save_window_accent("lueurs", rgb, refs.lueurs_accent.icon_bg);
        }
        "textures" => {
            refs.textures_accent.background = rgb;
            config::save_window_accent("textures", rgb, refs.textures_accent.icon_bg);
        }
        "moniteur" => {
            refs.moniteur_accent.background = rgb;
            config::save_window_accent("moniteur", rgb, refs.moniteur_accent.icon_bg);
        }
        "recherche" => {
            refs.recherche_accent.background = rgb;
            config::save_window_accent("recherche", rgb, refs.recherche_accent.icon_bg);
        }
        "automatisations" => {
            refs.automatisations_accent.background = rgb;
            config::save_window_accent("automatisations", rgb, refs.automatisations_accent.icon_bg);
        }
        "meteo" => {
            refs.meteo_accent.background = rgb;
            config::save_window_accent("meteo", rgb, refs.meteo_accent.icon_bg);
        }
        "aide" => {
            refs.aide_accent.background = rgb;
            config::save_window_accent("aide", rgb, refs.aide_accent.icon_bg);
        }
        "dash_palette" => {
            refs.dash_accent.palette = Some(rgb);
            config::save_window_palette_accent("dash", rgb);
        }
        "settings_palette" => {
            refs.settings_accent.palette = Some(rgb);
            config::save_window_palette_accent("settings", rgb);
        }
        "clipboard_palette" => {
            refs.clipboard_accent.palette = Some(rgb);
            config::save_window_palette_accent("clipboard", rgb);
        }
        "notes_palette" => {
            refs.notes_accent.palette = Some(rgb);
            config::save_window_palette_accent("notes", rgb);
        }
        "calendar_palette" => {
            refs.calendar_accent.palette = Some(rgb);
            config::save_window_palette_accent("calendar", rgb);
        }
        "minuteur_palette" => {
            refs.minuteur_accent.palette = Some(rgb);
            config::save_window_palette_accent("minuteur", rgb);
        }
        "surveillance_palette" => {
            refs.surveillance_accent.palette = Some(rgb);
            config::save_window_palette_accent("surveillance", rgb);
        }
        "calculatrice_palette" => {
            refs.calculatrice_accent.palette = Some(rgb);
            config::save_window_palette_accent("calculatrice", rgb);
        }
        "ssh_palette" => {
            refs.ssh_accent.palette = Some(rgb);
            config::save_window_palette_accent("ssh", rgb);
        }
        "terminal_palette" => {
            refs.terminal_accent.palette = Some(rgb);
            config::save_window_palette_accent("terminal", rgb);
        }
        "dossier_palette" => {
            refs.dossier_accent.palette = Some(rgb);
            config::save_window_palette_accent("dossier", rgb);
        }
        "visionneuse_palette" => {
            refs.visionneuse_accent.palette = Some(rgb);
            config::save_window_palette_accent("visionneuse", rgb);
        }
        "couleurs_palette" => {
            refs.couleurs_accent.palette = Some(rgb);
            config::save_window_palette_accent("couleurs", rgb);
        }
        "editeur_palette" => {
            refs.editeur_accent.palette = Some(rgb);
            config::save_window_palette_accent("editeur", rgb);
        }
        "scanner_palette" => {
            refs.scanner_accent.palette = Some(rgb);
            config::save_window_palette_accent("scanner", rgb);
        }
        "shaders_palette" => {
            refs.shaders_accent.palette = Some(rgb);
            config::save_window_palette_accent("shaders", rgb);
        }
        "lueurs_palette" => {
            refs.lueurs_accent.palette = Some(rgb);
            config::save_window_palette_accent("lueurs", rgb);
        }
        "textures_palette" => {
            refs.textures_accent.palette = Some(rgb);
            config::save_window_palette_accent("textures", rgb);
        }
        "moniteur_palette" => {
            refs.moniteur_accent.palette = Some(rgb);
            config::save_window_palette_accent("moniteur", rgb);
        }
        "recherche_palette" => {
            refs.recherche_accent.palette = Some(rgb);
            config::save_window_palette_accent("recherche", rgb);
        }
        "automatisations_palette" => {
            refs.automatisations_accent.palette = Some(rgb);
            config::save_window_palette_accent("automatisations", rgb);
        }
        "meteo_palette" => {
            refs.meteo_accent.palette = Some(rgb);
            config::save_window_palette_accent("meteo", rgb);
        }
        "aide_palette" => {
            refs.aide_accent.palette = Some(rgb);
            config::save_window_palette_accent("aide", rgb);
        }
        "theme_base_color" => {
            with_data(|d| d.theme_base_color = rgb);
            config::save_theme_base_color(rgb);
            let (saturation, luminosite) = forces_du_theme();
            let theme = color::ajuster_theme(rgb, saturation, luminosite);
            apply_theme(refs, theme);
            poser_bases_uniformes(rgb, None);
        }
        _ => eprintln!("[neuradesk][cef] cible de couleur inconnue : {target:?}"),
    }
    // Un choix manuel de « Couleur de mise en avant » (fond ou palette d'UN
    // panneau) devient la base de ce panneau : les curseurs saturation/
    // luminosité l'échelonneront sans l'effacer (sans effet pour les barres,
    // séparateur, menu… — voir `enregistrer_base_manuelle`).
    enregistrer_base_manuelle(target);
}

fn add_custom_color(custom_colors: &mut Vec<[f32; 3]>, rgb: [f32; 3]) {
    if !custom_colors.contains(&rgb) {
        custom_colors.push(rgb);
        config::save_custom_colors(custom_colors);
    }
}

fn remove_custom_color(custom_colors: &mut Vec<[f32; 3]>, index: usize) {
    if index < custom_colors.len() {
        custom_colors.remove(index);
        config::save_custom_colors(custom_colors);
    }
}

fn add_custom_theme(custom_themes: &mut Vec<[f32; 3]>, rgb: [f32; 3]) {
    if !custom_themes.contains(&rgb) {
        custom_themes.push(rgb);
        config::save_custom_themes(custom_themes);
    }
}

fn remove_custom_theme(custom_themes: &mut Vec<[f32; 3]>, index: usize) {
    if index < custom_themes.len() {
        custom_themes.remove(index);
        config::save_custom_themes(custom_themes);
    }
}

fn parse_hex_rgb(text: &str) -> Option<[f32; 3]> {
    let s = text.trim().trim_start_matches('#');
    if s.len() != 6 || !s.is_ascii() || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([byte(0)? as f32 / 255.0, byte(2)? as f32 / 255.0, byte(4)? as f32 / 255.0])
}

fn hex(rgb: [f32; 3]) -> String {
    let [r, g, b] = rgb.map(crate::ui::color::to_u8);
    format!("{r:02X}{g:02X}{b:02X}")
}

fn tint_percent(tint_strength: f32) -> u8 {
    ((1.0 - tint_strength) * 100.0).round().clamp(0.0, 100.0) as u8
}

// --- Pipette d'écran (voir la doc de module) — port direct de
// win32::settings_dash, ni egui ni CEF n'y jouent le moindre rôle ---

fn colorref_to_rgb(c: COLORREF) -> [f32; 3] {
    let v = c.0;
    [(v & 0xFF) as f32 / 255.0, ((v >> 8) & 0xFF) as f32 / 255.0, ((v >> 16) & 0xFF) as f32 / 255.0]
}

fn start_eyedropper(target: &str) {
    stop_eyedropper();
    EYEDROPPER_ACTIVE.with(|e| *e.borrow_mut() = Some(target.to_string()));
    unsafe {
        if let Ok(hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(eyedropper_mouse_hook), None, 0) {
            EYEDROPPER_HOOK.with(|h| h.set(Some(hook)));
        }
    }
}

fn stop_eyedropper() {
    EYEDROPPER_ACTIVE.with(|e| *e.borrow_mut() = None);
    EYEDROPPER_RESULT.with(|r| r.set(None));
    if let Some(hook) = EYEDROPPER_HOOK.with(Cell::take) {
        unsafe {
            let _ = UnhookWindowsHookEx(hook);
        }
    }
}

unsafe extern "system" fn eyedropper_mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wparam.0 as u32 == WM_LBUTTONDOWN {
        let data = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let screen_dc = GetDC(None);
        let colorref = GetPixel(screen_dc, data.pt.x, data.pt.y);
        ReleaseDC(None, screen_dc);
        if colorref.0 != 0x00FF_FFFF {
            EYEDROPPER_RESULT.with(|r| r.set(Some(colorref_to_rgb(colorref))));
        }
        return LRESULT(1);
    }
    CallNextHookEx(None, code, wparam, lparam)
}

// --- État Rust→JS ---

/// **CE QUE LE PANNEAU DOIT SAVOIR D'UNE LIGNE DU CONTENU D'UNE ZONE.**
///
/// Construit par `app::zone_row_for_icon` (voir `SettingsRefs::zone_row`), qui
/// seul connaît les NOMS — la MÊME fonction que les infobulles, donc les deux ne
/// peuvent pas se contredire — et les NATURES (applet, épingle du Dash, épingle
/// écrite à la main). Le panneau, lui, ne fait que l'afficher.
///
/// DEMANDE EXPLICITE (2026-09-23) : « dans les réglages de zones je voudrais voir
/// ce qui se trouve dedans, et avoir la possibilité de la cacher quand c'est une
/// applet et de le retirer quand c'est un raccourci ».
pub struct ZoneRow {
    /// Le nom affiché, tel quel.
    pub label: String,
    /// `"remove"` — retirer un raccourci du Dash ; `"hide"` — cacher une applet ;
    /// `""` — aucune action (un espace, le miroir, une icône qui n'est ni l'un ni
    /// l'autre). **C'est le panneau qui en déduit le bouton à dessiner.**
    pub action: &'static str,
    /// L'identifiant envoyé avec l'action : le **nom de la fonctionnalité** pour
    /// `hide` (voir `app::set_feature_by_name`), le **nom de l'épingle** pour
    /// `remove` (voir `config::remove_extra_pin`). Vide s'il n'y a pas d'action.
    ///
    /// **Jamais un index** : la page ne doit pas en connaître (voir `push_state`).
    pub arg: String,
    /// **POURQUOI il n'y a pas d'action**, quand c'est le cas — affiché tel quel
    /// sous la ligne. Une épingle de `config.toml` en a une : l'application ne
    /// réécrit jamais ce fichier, donc elle ne peut pas la retirer.
    pub note: &'static str,
}

thread_local! {
    /// **LA LISTE DES SONS DU DOSSIER, EN CACHE COURT.** Le panneau des Réglages
    /// renvoie TOUT son état à chaque frame tant qu'il est ouvert (voir `poll`,
    /// qui sort quand il est fermé), et `son::fichiers()` fait un `read_dir` plus
    /// un `is_file` par entrée : soixante fois par seconde, ce serait le seul
    /// accès disque de cette boucle. Même motif que `SON_GENERATION` de
    /// `cef_notes`, qui évite une LECTURE DE FICHIER par frame — ici c'est un
    /// dossier entier, donc pire.
    ///
    /// ⚠️ **UNE SECONDE DE RETARD AU PIRE, ET C'EST TOUT** : un fichier déposé
    /// dans `son/` pendant que le panneau est ouvert apparaît une seconde plus
    /// tard. Personne ne le verra.
    static SONS_CACHE: RefCell<Option<(std::time::Instant, Vec<std::path::PathBuf>)>> = const { RefCell::new(None) };
}

/// **LA LISTE DES SONS PROPOSABLES, AVEC LES DEUX CHOIX.** La page ne connaît que
/// **LEVIER DE VÉRIFICATION** (`NEURADESK_DEBUG_REGLAGES_SECTION=<titre>`, lu
/// dans `app::run`) : amène la section dont le `<h2>` porte exactement `titre` en
/// haut du panneau — un appel de fonction de page, pas une molette simulée.
/// Répété à chaque frame : une reconstruction du panneau (voir `structureKey`)
/// remet le défilement à zéro, et `scrollIntoView` sans animation est idempotent.
/// Le titre CONTIENT `titre` (et non l'égale) : un accent passé par la ligne de
/// commande de Git Bash n'arrive pas toujours intact — `"diteur d'image"` suffit.
pub fn debug_section(titre: &str) {
    if !SECTION_SIGNALEE.with(|s| s.replace(true)) {
        crate::diag!("[reglages] NEURADESK_DEBUG_REGLAGES_SECTION={titre:?} — visible : {}", cef_popup::is_visible(&SLOT));
    }
    cef_popup::execute_js(
        &SLOT,
        &format!("[...document.querySelectorAll('h2')].find((h) => h.textContent.includes({}))?.scrollIntoView({{ block: 'start' }})", json!(titre)),
    );
}

/// Lit (une fois) puis modifie les réglages d'envoi, et les réécrit.
fn modifier_envoi(modifier: impl FnOnce(&mut crate::win32::envoi_image::Reglages)) {
    ENVOI.with(|e| {
        let mut envoi = e.borrow_mut();
        let reglages = envoi.get_or_insert_with(crate::win32::envoi_image::charger_reglages);
        modifier(reglages);
        crate::win32::envoi_image::enregistrer_reglages(reglages);
    });
}

/// Modifie le serveur personnalisé sur une COPIE des réglages : une valeur
/// refusée (`Err`) ne laisse rien à moitié changé, n'écrit rien, et son message
/// s'affiche sous le formulaire ; un `Ok` non vide s'affiche aussi.
fn modifier_perso(modifier: impl FnOnce(&mut crate::win32::envoi_image::Reglages) -> Result<String, String>) {
    let mut copie = ENVOI.with(|e| e.borrow_mut().get_or_insert_with(crate::win32::envoi_image::charger_reglages).clone());
    let message = match modifier(&mut copie).and_then(|m| crate::win32::envoi_image::verifier_noms(&copie.perso).map(|()| m)) {
        Ok(message) => {
            modifier_envoi(|r| *r = copie);
            (!message.is_empty()).then_some((message, false))
        }
        Err(erreur) => Some((erreur, true)),
    };
    MESSAGE_PERSO.with(|m| *m.borrow_mut() = message);
}

/// L'état de la section « Éditeur d'image » : les hébergeurs proposés (liste
/// tenue par `Hebergeur::ALL`, pas recopiée dans la page), le choix courant, et
/// **SEULEMENT SI** une clé est enregistrée — jamais la clé elle-même, qui n'a
/// rien à faire dans l'état d'une page.
fn envoi_json() -> serde_json::Value {
    use crate::win32::envoi_image::{charger_reglages, Hebergeur, DUREES_LITTERBOX};
    ENVOI.with(|e| {
        let mut envoi = e.borrow_mut();
        let reglages = envoi.get_or_insert_with(charger_reglages);
        json!({
            "service": reglages.hebergeur.as_str(),
            "duree": reglages.duree,
            "durees": DUREES_LITTERBOX,
            "services": Hebergeur::ALL.iter().map(|h| json!({
                "value": h.as_str(),
                "label": h.label(),
                "description": h.description(),
                "cle": h.cle(),
                "cleDefinie": reglages.cle_de(*h).is_some(),
            })).collect::<Vec<_>>(),
            // Les NOMS des en-têtes, champs et paramètres, JAMAIS leurs valeurs
            // (un jeton d'API y vit en général — même règle que `cleDefinie`).
            "perso": {
                "url": reglages.perso.url,
                "champ": reglages.perso.champ_fichier,
                "lien": reglages.perso.lien,
                "entetes": reglages.perso.entetes.keys().collect::<Vec<_>>(),
                "champs": reglages.perso.champs.keys().collect::<Vec<_>>(),
                "parametres": reglages.perso.parametres.keys().collect::<Vec<_>>(),
                "message": MESSAGE_PERSO.with(|m| m.borrow().clone()).map(|(texte, erreur)| json!({ "texte": texte, "erreur": erreur })),
            },
        })
    })
}

/// le RANG dans cette liste — même motif que `win32::cef_notes::sons_json`, et
/// même raison : un chemin contient le dossier de l'utilisateur, et le faire
/// voyager dans une URL demanderait de l'encoder puis de le décoder.
fn sons_json() -> Vec<serde_json::Value> {
    let fichiers = SONS_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let perime = cache
            .as_ref()
            .map_or(true, |(quand, _)| quand.elapsed() >= std::time::Duration::from_secs(1));
        if perime {
            *cache = Some((std::time::Instant::now(), crate::win32::son::fichiers()));
        }
        cache.as_ref().map(|(_, fichiers)| fichiers.clone()).unwrap_or_default()
    });
    let notes = config::load_son_rappel(config::FamilleRappel::Notes);
    let dates = config::load_son_rappel(config::FamilleRappel::Dates);
    let est_choisi = |choisi: &Option<String>, chemin: &std::path::Path| {
        choisi.as_deref().is_some_and(|c| std::path::Path::new(c) == chemin)
    };
    fichiers
        .iter()
        .enumerate()
        .map(|(index, chemin)| {
            json!({
                "index": index,
                "nom": crate::win32::son::nom(chemin),
                "notes": est_choisi(&notes, chemin),
                "dates": est_choisi(&dates, chemin),
            })
        })
        .collect()
}

/// **CHOISIT LE SON D'UNE FAMILLE DE RAPPELS.** Le rang vient de la page ; il est
/// RE-RÉSOLU ici contre la liste réelle — même motif que
/// `win32::cef_notes::handle_son`, et même raison : la liste peut avoir bougé
/// depuis le rendu, et un rang périmé désignerait un autre fichier en silence.
/// **LA BARRE DÉSIGNÉE PAR SA CLÉ** — `"haut"` / `"gauche"` / `"droite"`, les mêmes
/// mots que la page emploie. Rend `None` sur autre chose plutôt que de deviner.
fn barre_de_tirage(cle: &str) -> Option<crate::ui::dock::BarreTirage> {
    use crate::ui::dock::BarreTirage;
    match cle {
        "haut" => Some(BarreTirage::Haut),
        "gauche" => Some(BarreTirage::Gauche),
        "droite" => Some(BarreTirage::Droite),
        "bas" => Some(BarreTirage::Bas),
        _ => None,
    }
}

fn choisir_son(famille: config::FamilleRappel, index: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let fichiers = crate::win32::son::fichiers();
    let Some(chemin) = fichiers.get(index) else { return };
    config::save_son_rappel(famille, &chemin.display().to_string());
}

/// **JOUE UN SON DU DOSSIER, DÉSIGNÉ PAR SON RANG** — le bouton « ▶ » posé sur
/// CHAQUE ligne de la section « Sons des rappels ». DEMANDE EXPLICITE (2026-09-24) :
/// « j'aimerais avoir un bouton de lecture à côté de chaque sélecteur de son pour
/// les écouter ».
///
/// ⚠️ **LE RANG EST RE-RÉSOLU ICI**, contre la liste réelle — même règle que
/// `choisir_son`, et même raison : un fichier ajouté ou retiré entre la poussée et
/// le clic ferait viser un AUTRE son avec un rang mémorisé.
///
/// ⛔ **`jouer_ecoute` ET NON `jouer`, ET IL A FALLU UN DÉFAUT POUR LE VOIR.** Les
/// deux jouent le même genre de fichier ; ce qui les sépare est l'**ALIAS MCI**.
/// Avec un seul, la branche d'`app::run` qui coupe la sonnerie — elle tourne à
/// CHAQUE FRAME — refermait l'alias de l'écoute aussitôt ouverte : le son durait
/// un soixantième de seconde. Mesuré : `jouer` rendait `Ok` sept fois de suite,
/// MCI rapportait « playing » avec une position qui avance, et **rien ne
/// s'entendait**. Voir `son::ALIAS_ECOUTE`, qui porte le récit complet.
///
/// ⚠️ **`jouer_ecoute` REFERME SON PROPRE ALIAS D'ABORD** : presser « ▶ » deux fois
/// de suite rejoue depuis le début, ce qu'un bouton d'écoute doit faire — alors
/// que la sonnerie, elle, ne relance jamais un son en cours (choix de
/// l'utilisateur, voir `app::run`).
fn ecouter_son(index: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let fichiers = crate::win32::son::fichiers();
    let Some(chemin) = fichiers.get(index) else { return };
    let chemin = chemin.display().to_string();
    // ⚠️ **ON TRACE LES DEUX ISSUES, ET PAS SEULEMENT L'ÉCHEC.** C'est ce qui a
    // permis de trouver ce défaut : sans la trace du succès, « le bouton n'a pas
    // été pressé » et « MCI a accepté et rien ne sort » laissaient le même journal
    // vide. Le test du dépôt prouve qu'un fichier est LISIBLE (`open` + `status
    // length`), jamais qu'il s'ENTEND — d'où la sonde `sonde_mci_joue_vraiment`.
    match crate::win32::son::jouer_ecoute(std::path::Path::new(&chemin)) {
        Ok(()) => crate::diag!("[alarme] écoute : « {chemin} » confié à MCI"),
        Err(err) => crate::diag!("[alarme] écoute : le son « {chemin} » n'a pas pu jouer : {err}"),
    }
}

fn push_state(refs: &SettingsRefs) {
    // Catalogues STATIQUES (120 thèmes, 48 préréglages Windows) — voir la
    // doc de `toggle`, ancien emplacement de cet envoi (BUG RÉEL : pouvait
    // arriver AVANT la fin du chargement de la page, silencieusement
    // perdu pour toujours). Recalculés à CHAQUE frame comme le reste de
    // `payload` ci-dessous plutôt que mis en cache une fois : ce module
    // n'a pas de flag "envoyé avec succès" fiable (aucune confirmation
    // JS→Rust pour un simple `execute_java_script`), la solution la plus
    // simple qui élimine la course DÉFINITIVEMENT est de renvoyer ces
    // ~170 entrées à chaque frame, comme tout le reste de l'état de ce
    // panneau — coût négligeable (JSON statique, ordre de grandeur d'un
    // `window.setState` déjà envoyé 60×/s).
    // Écrans secondaires (voir la doc de `SettingsRefs::secondary_screen_modes`)
    // — reconstruit à CHAQUE frame comme le reste de `payload`, coût
    // négligeable (`EnumDisplayMonitors`/`EnumWindows`, au plus quelques
    // écrans, voir `win32::monitors`) plutôt qu'un cache invalidé sur
    // brancher/débrancher un moniteur (aucune notification simple pour ça).
    let secondary_screens: Vec<_> = monitors::enumerate()
        .into_iter()
        .filter(|m| !m.is_primary)
        .enumerate()
        .map(|(index, m)| {
            // "full" reste possible dans un fichier de config sauvegardé par
            // une version antérieure (le mode "application complète" a été
            // retiré, voir la doc de `SecondaryWindow` côté Rust) — traité
            // ici comme "top" pour que le bouton correspondant s'affiche
            // sélectionné au lieu qu'aucun ne le soit.
            let mode = match refs.secondary_screen_modes.get(&m.device_name).map(String::as_str) {
                Some("full") => "top".to_string(),
                Some(other) => other.to_string(),
                None => "off".to_string(),
            };
            // Réglages manuels de CET écran (voir la doc de `SettingsRefs::
            // secondary_screen_thicknesses`/`_tint`/`_colors`, DEMANDE
            // EXPLICITE : "réglages... séparés entre chaque écran") —
            // `null`/absent côté JS = pas de réglage manuel pour ce point,
            // suit la barre PRIMAIRE ou la dérivation automatique.
            let thickness = refs.secondary_screen_thicknesses.get(&m.device_name).map(|[top, left, right]| {
                json!([top.round() as i32, left.round() as i32, right.round() as i32])
            });
            let tint = refs.secondary_screen_tint.get(&m.device_name).map(|t| t.map(tint_percent));
            let colors = refs.secondary_screen_colors.get(&m.device_name).map(|colors| {
                colors.map(|c| c.map(|[r, g, b, _a]| hex([r, g, b])))
            });
            // Éléments désactivés POUR CET ÉCRAN (voir la doc de
            // `SettingsRefs::secondary_screen_features`) — liste vide si
            // l'écran suit le réglage global sans exception.
            let disabled_features: Vec<&str> = refs
                .secondary_screen_features
                .get(&m.device_name)
                .map(|names| names.iter().map(String::as_str).collect())
                .unwrap_or_default();
            // "Colorisation automatique"/couleur du texte de la barre du
            // haut de CET écran (voir la doc de `SettingsRefs::
            // secondary_screen_auto_color_scheme`/`_topbar_text_color`) —
            // `autoColorScheme: null` = pas de réglage manuel, suit la barre
            // PRIMAIRE (même convention que `thickness`/`tintPercent`/
            // `colors` ci-dessus).
            let auto_color_scheme = refs.secondary_screen_auto_color_scheme.get(&m.device_name).copied();
            let topbar_text_color = refs.secondary_screen_topbar_text_color.get(&m.device_name).map(|c| hex(*c));
            // Mini-barre d'horloge plein écran PERSONNALISÉE de CET écran
            // (voir la doc de `SettingsRefs::secondary_screen_fullscreen_
            // clock_*`, DEMANDE EXPLICITE : "pouvoir régler individuellement
            // les horloges en plein écran en fonction de l'écran où elles
            // s'affichent") — `null` = pas de réglage manuel, suit la barre
            // PRIMAIRE (même convention que `autoColorScheme`/
            // `topbarTextColor` ci-dessus).
            let fullscreen_clock_enabled = refs.secondary_screen_fullscreen_clock_enabled.get(&m.device_name).copied();
            let fullscreen_clock_position_top =
                refs.secondary_screen_fullscreen_clock_position_top.get(&m.device_name).copied();
            let fullscreen_clock_bar_scale_percent = refs
                .secondary_screen_fullscreen_clock_bar_scale
                .get(&m.device_name)
                .map(|s| (s * 100.0).round() as i32);
            let fullscreen_clock_text_scale_percent = refs
                .secondary_screen_fullscreen_clock_text_scale
                .get(&m.device_name)
                .map(|s| (s * 100.0).round() as i32);
            // Taille des pastilles de bureaux virtuels PERSONNALISÉE de CET
            // écran (voir la doc de `SettingsRefs::
            // secondary_screen_desktop_dot_sizes`) — même convention `null`
            // que les autres réglages ci-dessus.
            let desktop_dot_size =
                refs.secondary_screen_desktop_dot_sizes.get(&m.device_name).map(|s| s.round() as i32);
            // Taille de police des textes de la barre du haut de CET écran (voir
            // la doc de `SettingsRefs::secondary_screen_topbar_text_sizes`) —
            // `null` (absent) : suit la barre PRIMAIRE, automatique comprise.
            // Même convention `null` que `desktopDotSize` juste au-dessus.
            let topbar_text_size = refs.secondary_screen_topbar_text_sizes.get(&m.device_name).map(|px| px.round() as i32);
            // Épaisseur de barre du haut EFFECTIVE de CET écran — la sienne si
            // elle est réglée, sinon celle de la barre PRIMAIRE. C'est EXACTEMENT
            // le repli de `app::rebuild_secondary_docks`, et il compte : c'est
            // cette épaisseur-là qui décide de la taille automatique du texte et
            // de son plafond, donc l'afficher d'après une autre valeur donnerait
            // à l'utilisateur un repère faux.
            let screen_top_thickness = refs
                .secondary_screen_thicknesses
                .get(&m.device_name)
                .map(|[top, _, _]| *top)
                .unwrap_or(*refs.top_thickness);
            // Écartement entre icônes PERSONNALISÉ de CET écran (voir la doc
            // de `SettingsRefs::secondary_screen_icon_gaps`) — même
            // convention `null` que `desktopDotSize` juste au-dessus.
            let icon_gap = refs.secondary_screen_icon_gaps.get(&m.device_name).map(|g| g.round() as i32);
            // Couleur des séparateurs PERSONNALISÉE de CET écran (voir la
            // doc de `SettingsRefs::secondary_screen_separator_colors`) —
            // `null` (absent) : suit la couleur PRIMAIRE (voir
            // `separatorColor` global plus bas), même convention `null` que
            // `desktopDotSize`/`iconGap` ci-dessus. Présente : objet
            // `{mode: "transparent"|"custom", hex}` — même encodage que
            // `separatorColor` global.
            let separator_color = refs.secondary_screen_separator_colors.get(&m.device_name).map(|c| {
                if c[3] <= 0.0 {
                    json!({ "mode": "transparent" })
                } else {
                    json!({ "mode": "custom", "hex": hex([c[0], c[1], c[2]]) })
                }
            });
            // Taille des séparateurs PERSONNALISÉE de CET écran (voir la
            // doc de `SettingsRefs::secondary_screen_separator_sizes`) —
            // `null` (absent) : suit la taille PRIMAIRE (voir
            // `separatorSize` global plus bas), même convention `null` que
            // `desktopDotSize`/`iconGap` ci-dessus.
            let separator_size =
                refs.secondary_screen_separator_sizes.get(&m.device_name).map(|s| s.round() as i32);
            // Facteurs d'échelle PERSONNALISÉS de CET écran (voir la doc de
            // `SettingsRefs::secondary_screen_scales`) — `null` = pas de
            // réglage manuel, suit entièrement les réglages globaux/
            // automatiques (même convention `null`/absent que `thickness`/
            // `tintPercent`/`colors` ci-dessus).
            let scales = refs.secondary_screen_scales.get(&m.device_name).map(|s| {
                json!({
                    // ⚠️ **LES CLÉS SONT LE CONTRAT AVEC `settings.html`** — elles y
                    // sont lues par nom dans `secondaryScreenScales`.
                    "menu": (s[crate::config::CHAMP_MENU] * 100.0).round() as i32,
                    "dashIcon": (s[crate::config::CHAMP_DASH_ICONE] * 100.0).round() as i32,
                    "dashLabel": (s[crate::config::CHAMP_DASH_ETIQUETTE] * 100.0).round() as i32,
                    "dashCategory": (s[crate::config::CHAMP_DASH_CATEGORIE] * 100.0).round() as i32,
                    "clipboardThumb": (s[crate::config::CHAMP_PRESSE_MINIATURE] * 100.0).round() as i32,
                    "clipboardText": (s[crate::config::CHAMP_PRESSE_TEXTE] * 100.0).round() as i32,
                    "settingsContent": (s[crate::config::CHAMP_REGLAGES_CONTENU] * 100.0).round() as i32,
                    // ⭐ Les quatre neufs. Le ratio de DPI est déjà AMORCÉ avec
                    // l'automatique (`init`), donc ce que la page affiche est ce qui
                    // est réellement appliqué.
                    "dpi": (s[crate::config::CHAMP_RATIO_DPI] * 100.0).round() as i32,
                })
            });
            json!({
                "deviceName": m.device_name,
                "label": format!("Écran {}", index + 2),
                "mode": mode,
                "thickness": thickness,
                "tintPercent": tint,
                "colors": colors,
                "disabledFeatures": disabled_features,
                "autoColorScheme": auto_color_scheme,
                "topbarTextColor": topbar_text_color,
                "fullscreenClockEnabled": fullscreen_clock_enabled,
                "fullscreenClockPositionTop": fullscreen_clock_position_top,
                "fullscreenClockBarScalePercent": fullscreen_clock_bar_scale_percent,
                // Son décalage à lui, `null` : celui du principal.
                "fullscreenClockDecalage": crate::win32::fullscreen_clock::decalage_propre(&m.device_name),
                "fullscreenClockTextScalePercent": fullscreen_clock_text_scale_percent,
                "desktopDotSize": desktop_dot_size,
                "topbarTextSize": topbar_text_size,
                // Ses ombres, `null` : il suit l'écran principal (voir
                // `win32::ombre_barre::ReglagesEcran`).
                "ombre": crate::win32::ombre_barre::reglages_ecran(&m.device_name).map(|r| json!({
                    "noireLargeur": r.noire[0].round() as i32, "noireForce": (r.noire[1] * 100.0).round() as i32,
                    "zone": r.zone,
                    "couleurLargeur": r.couleur[0].round() as i32, "couleurForce": (r.couleur[1] * 100.0).round() as i32,
                })),
                // Les coins arrondis sous sa barre (voir `win32::coins_secondaires`).
                "coins": crate::win32::coins_secondaires::actif_sur(&m.device_name),
                "coinsRayon": crate::win32::coins_secondaires::rayon_regle(&m.device_name),
                "coinsRayonMax": crate::win32::coins_secondaires::RAYON_MAX,
                "topbarTextAutoPx": crate::ui::topbar::topbar_text_font_px_auto(screen_top_thickness),
                "topbarTextMaxPx": crate::ui::topbar::topbar_text_font_px_max(screen_top_thickness),
                "iconGap": icon_gap,
                "separatorColor": separator_color,
                "separatorSize": separator_size,
                "scales": scales,
            })
        })
        .collect();
    let catalog_themes: Vec<_> = color::THEMES.iter().map(|(name, rgb)| json!({ "name": name, "rgb": rgb })).collect();
    let catalog_presets: Vec<_> = color::WINDOWS_COLORS.iter().map(|c| [c[0], c[1], c[2]]).collect();
    let accent_json = |target: &color::WindowAccent| {
        json!({
            "background": hex(target.background),
            "iconBg": target.icon_bg,
            "palette": target.palette.map(hex),
        })
    };
    // Bornes des trois curseurs de taille de barre — `[haut, gauche, droite]`.
    // LIÉES À UNE VARIABLE LOCALE, et pas écrites dans le `json!` : la macro
    // n'accepte pas un chemin `a::b` en position de valeur (`no rules expected
    // keyword`). Voir `"barSizeMin"`/`"barSizeMax"` plus bas, et
    // `ui::dock::bar_thickness_bounds` pour ce qu'elles engagent.
    let bar_size_max = [crate::ui::dock::BAR_THICKNESS_MAX; 4];
    let bar_size_min = crate::ui::dock::bar_thickness_mins();
    // Bornes des trois réglages de l'APERÇU D'UN BUREAU (voir
    // `win32::desktop_previews`) — liées à des variables locales pour la MÊME
    // raison que ci-dessus : la macro `json!` refuse un chemin `a::b` en position
    // de valeur. Ces paires sont la SEULE AUTORITÉ (voir
    // `desktop_previews::preview_width_bounds`) et la page n'en recopie aucune.
    let (desktop_width_min, desktop_width_max) = crate::win32::desktop_previews::preview_width_bounds();
    let (desktop_fade_min, desktop_fade_max) = crate::win32::desktop_previews::fade_bounds();
    let (desktop_delay_min, desktop_delay_max) = crate::win32::desktop_previews::hover_delay_bounds();
    // Bornes des deux durées du fond animé du logo — même raison que ci-dessus.
    let ((logo_fade_min, logo_fade_max), (logo_color_min, logo_color_max)) = crate::ui::dock::logo_animation_bounds();
    let (logo_turn_min, logo_turn_max) = crate::ui::dock::logo_turn_bounds();
    let [logo_size_b, logo_zoom_amp_b, logo_zoom_ms_b, logo_zoom_interval_b] = crate::ui::dock::logo_size_zoom_bounds();
    let pct = |v: f32| (v * 100.0).round() as i32;
    // ⛔ **L'ÉCHELLE RÉELLEMENT RENDUE, RATIO DE DPI COMPRIS** — calculée ICI et
    // pas dans le `json!` : un `let` y est refusé par la macro (`unexpected end
    // of macro invocation`), et c'est la seule façon de lire la cellule UNE fois.
    //
    // ⚠️ **ELLE N'ALIMENTE QUE `settingsContentScaleEffectivePercent`**, jamais le
    // `settingsContentScalePercent` du dessus, qui est la valeur du CURSEUR global
    // telle que l'interface la montre. Les confondre était déjà un défaut signalé.
    let echelle_effective_pourcent = {
        let (choix, ratio_dpi) = SCREEN_SCALE_OVERRIDE.with(Cell::get);
        (echelle_effective(choix, content_scale(), ratio_dpi) * 100.0).round() as i32
    };
    let verrous = crate::ui::verrous::reglages();
    // Hors de `json!` : un bloc `{ … }` nu y serait lu comme un OBJET.
    let reglages_indicateur = {
        let [s, l, t] = crate::ui::dock::indicateur_reglages();
        json!({ "saturation": (s * 100.0).round() as i32, "luminosite": (l * 100.0).round() as i32, "transparence": (t * 100.0).round() as i32 })
    };
    let payload = json!({
        "autoColorScheme": *refs.auto_color_scheme,
        "captureVisible": crate::gfx::capture_visibility::is_enabled(),
        "degradeEnabled": crate::gfx::degrade::is_enabled(),
        "degradeAccent": crate::gfx::degrade::accent_enabled(),
        "degradePreset": crate::gfx::degrade::preset_nom(),
        // Un motif de l'utilisateur actif : aucun bouton intégré n'est choisi.
        "degradeMode": if crate::gfx::shader_utilisateur::nom_actif().is_some() { "" } else { crate::gfx::degrade::mode_nom() },
        "degradeShaders": crate::gfx::shader_utilisateur::lister_en_cache(),
        "degradeShader": crate::gfx::shader_utilisateur::nom_actif(),
        "degradeShaderCompile": crate::gfx::shader_utilisateur::en_compilation(),
        "degradeShaderMessage": crate::gfx::shader_utilisateur::message(),
        "degradeModes": &crate::gfx::degrade::MODES[..],
        "degradeCouleurs": crate::gfx::degrade::couleurs().map(hex),
        "degradeReglages": crate::gfx::degrade::REGLAGES
            .iter()
            .zip(crate::gfx::degrade::valeurs_reglages())
            .map(|(r, v)| (r.nom.to_string(), json!((v * 100.0).round() as i32)))
            .collect::<serde_json::Map<_, _>>(),
        "degradePresets": crate::gfx::degrade::PRESETS.iter().map(|p| p.nom).collect::<Vec<_>>(),
        "degradeSauvegardes": crate::gfx::degrade_sauvegardes::apercus(),
        // Clignotement à la notification (voir `win32::notification`) — lu
        // directement depuis le module, comme `captureVisible` ci-dessus : c'est
        // lui qui porte l'état, pas `SettingsRefs`.
        "notificationBlink": crate::win32::notification::is_enabled(),
        // Les notifications de Windows sous l'horloge (voir `win32::toast_veille`)
        // — même raison que ci-dessus : c'est le MODULE qui porte l'état.
        //
        // ⚠️ **C'est l'état de FONCTIONNEMENT, pas le réglage enregistré** : si
        // la suspension a été refusée par Windows, la case doit s'afficher
        // décochée, parce que c'est ce que l'utilisateur observe. Voir
        // `toast_veille::est_actif`.
        "toastsSousLHorloge": crate::win32::toast_veille::est_actif(),
        "meteoHorloge": crate::win32::meteo_horloge::ville(),
        "debitAffiche": crate::ui::debit::affiche(),
        "verrous": {
            "affiche": verrous.affiche,
            "num": hex(verrous.couleurs[0]),
            "maj": hex(verrous.couleurs[1]),
        },
        // Où se pose le bandeau (voir `win32::bandeau::Position`) — même raison
        // que ci-dessus : c'est le MODULE qui porte l'état.
        "positionBandeau": crate::win32::bandeau::position().nom(),
        // Miroir de la zone de notification — même raison que ci-dessus : c'est
        // le MODULE qui porte l'état.
        "trayMirror": crate::win32::tray_icons::is_enabled(),
        // Les deux réglages du flux — lus dans le fichier, avec les MÊMES défauts
        // que `app::run` (flux éteint, diffusion allumée) : sinon la case dirait
        // autre chose que ce qui démarre.
        "trayFlux": config::load_bool_setting_or("tray_flux", false),
        "trayFluxDiffusion": config::load_bool_setting_or("tray_flux_diffusion", true),
        // L'interrupteur de la Recherche n'est proposé que si Everything est
        // installé (en cache : cette poussée tourne à 60 Hz).
        "everythingInstalle": crate::win32::everything::installe().is_some(),
        // **LES LOGICIELS QUI MANQUENT À NOS APPLETS** — voir
        // `win32::outils_applets`, qui porte la table ET les URL. La page
        // ne reçoit que ce qu'elle dessine : un libellé, un nom, de quoi
        // écrire la phrase — et **le rang**, jamais l'URL (voir `ouvrir`).
        "outilsManquants": crate::win32::outils_applets::manquants()
            .iter()
            .map(|outil| json!({"applet": outil.applet, "logiciel": outil.logiciel}))
            .collect::<Vec<_>>(),
        // **NAPS2 EST-IL LÀ ?** Sans lui, l'applet Scanner n'a AUCUNE porte :
        // sa case disparaît de la liste, comme celle de la Recherche sans
        // Everything (voir `settings.html`, qui porte les deux).
        "naps2": crate::win32::scanner::chemin_console().is_some(),
        "features": {
            "power": refs.features.power,
            "terminal": refs.features.terminal,
            "sound": refs.features.sound,
            "network": refs.features.network,
            "voicemeeter": refs.features.voicemeeter,
            "media": refs.features.media,
            "clock": refs.features.clock,
            "desktop_dots": refs.features.desktop_dots,
            "dash": refs.features.dash,
            "clipboard_viewer": refs.features.clipboard_viewer,
            "notes": refs.features.notes,
            "refresh_rate": refs.features.refresh_rate,
            "sys_monitor": refs.features.sys_monitor,
            // Sans cette ligne, la case lit `undefined` : elle s'affiche
            // décochée quel que soit le réglage enregistré, tout en
            // fonctionnant quand on la clique — le genre de défaut le plus
            // pénible à diagnostiquer (voir `app::Features::show_desktop`).
            "show_desktop": refs.features.show_desktop,
            // Même piège que la ligne ci-dessus, et il vaut pour CHAQUE
            // fonctionnalité : une clé oubliée ici donne une case décochée à
            // tort, qui pourtant fonctionne quand on la clique.
            "theme_toggle": refs.features.theme_toggle,
                // Éclairage nocturne — la case de la section Fonctionnalités, et l'interrupteur
                // que le panneau des zones propose sur l'applet (voir `ZoneRow`).
                "night_light": refs.features.night_light,
                // File d'impression — la case de la section Fonctionnalites, et
                // l'interrupteur que le panneau des zones propose sur l'applet.
                "printer": refs.features.printer,
                // ⚠️ **"calendar" ÉTAIT ABSENT DE CETTE LISTE, ET C'ÉTAIT UN
                // DÉFAUT RÉEL** — celui que le commentaire ci-dessus décrit mot pour
                // mot, et il était bel et bien déclenché : la case existe dans
                // `assets/popups/settings.html` (TOPBAR_FEATURES, « Calendrier (fêtes
                // et anniversaires) ») mais elle lisait `undefined`, donc
                // **s'affichait décochée quel que soit le réglage enregistré**, tout
                // en fonctionnant quand on la cliquait. Corrigé le 2026-09-23 en même
                // temps qu'on ajoutait la cloche juste en dessous.
                "calendar": refs.features.calendar,
                // Éditeur d'image — la case de la section Fonctionnalités, et l'interrupteur
                // que le panneau des zones propose sur l'applet.
                "image_editor": refs.features.image_editor,
                "screen_capture": refs.features.screen_capture,
                "minuteur": refs.features.minuteur,
                "surveillance": refs.features.surveillance,
                "windows_update": refs.features.windows_update,
                "device_manager": refs.features.device_manager,
                "observateur_evenements": refs.features.observateur_evenements,
                "enregistrer": refs.features.enregistrer,
                "clavier": refs.features.clavier,
                "jeu2048": refs.features.jeu2048,
                "cinq_ou_plus": refs.features.cinq_ou_plus,
                "quadrapassel": refs.features.quadrapassel,
                "lightsoff": refs.features.lightsoff,
                "display_settings": refs.features.display_settings,
                "optional_features": refs.features.optional_features,
                "notifications": refs.features.notifications,
                "recherche": refs.features.recherche,
                "automatisations": refs.features.automatisations,
                "meteo": refs.features.meteo,
                "verrouillage": refs.features.verrouillage,
                "caracteres": refs.features.caracteres,
                "kaomoji": refs.features.kaomoji,
                // ⚠️ **L'AIDE ÉTAIT ABSENTE D'ICI, ET DES DEUX CÔTÉS À LA FOIS** —
                // de cette liste ET de `TOPBAR_FEATURES` (2026-10-08). C'est le
                // défaut `"calendar"` ci-dessus, dans sa variante la plus grave :
                // pas seulement une case qui ment, mais **aucune case du tout**.
                "aide": refs.features.aide,
                // DEMANDE EXPLICITE (2026-10-08) : « une nouvelle applet dans
                // Intégrés pour ouvrir sysdm.cpl ». Une ACTION.
                "proprietes_systeme": refs.features.proprietes_systeme,
                "scanner": refs.features.scanner,
                "shaders": refs.features.shaders,
                "phonetique": refs.features.phonetique,
                "maths": refs.features.maths,
                "lueurs": refs.features.lueurs,
                "textures": refs.features.textures,
                "narrateur": refs.features.narrateur,
                "loupe": refs.features.loupe,
                "moniteur": refs.features.moniteur,
                "calculatrice": refs.features.calculatrice,
                "ssh": refs.features.ssh,
                // ⚠️ **LE SÉLECTEUR DE COULEURS ÉTAIT ABSENT D'ICI (2026-10-08)** —
                // présent dans `TOPBAR_FEATURES`, donc AVEC une case, mais sans état
                // poussé : sa case s'affichait **décochée en permanence**, et se
                // recochait au clic sans que rien ne le dise. C'est le défaut
                // `"calendar"` mot pour mot, resté vivant après la correction de
                // celui-ci.
                "couleurs": refs.features.couleurs,
                // La CLOCHE DE RAPPELS — widget de barre du haut (voir
                // `ui::topbar::bell_rect`), proposé sur les écrans secondaires comme
                // sur la barre primaire : elle n'est PAS dans le filtre de
                // `SECONDARY_TOPBAR_FEATURES` (`assets/popups/settings.html`), et
                // c'est voulu — l'utilisateur l'a demandée partout.
                "bell": refs.features.bell,
                // L'ICÔNE DE NOTE — même famille que la cloche (widget de barre
                // du haut, à GAUCHE de l'horloge), et proposée sur les écrans
                // secondaires pour la même raison : elle y est le SEUL signal d'un
                // rappel de note, la cloche ne les couvrant plus.
                "rappel_notes": refs.features.rappel_notes,
                "masque": refs.features.masque,
        },
        // **LES DEUX SONS DE RAPPEL** — la liste des fichiers du dossier `son/`
        // (Rust seul y a accès, voir `sons_json`) et, dans chaque entrée, lequel
        // est choisi pour les notes et pour les dates.
        "sons": sons_json(),
        "barColors": refs.dock.bar_colors().map(|c| hex([c[0], c[1], c[2]])),
        "barTintPercent": refs.bar_tint.map(tint_percent),
        "topbarTextColor": hex(*refs.topbar_text_color),
        "iconGap": refs.dock.icon_gap().round() as i32,
        "desktopDotSize": refs.dock.desktop_dot_size().round() as i32,
        // Taille de police des textes de la barre du haut (voir `ui::dock::Dock::
        // topbar_text_size`) — `null` = réglage AUTOMATIQUE, un nombre = taille
        // personnalisée. La page reçoit EN PLUS la valeur automatique et le
        // maximum courant : elle les AFFICHE et dimensionne son curseur avec,
        // sans recalculer la formule de son côté — deux calculs de la même
        // grandeur finiraient par diverger (voir `ui::topbar`, qui expose
        // précisément ces deux fonctions ; elles ne dépendent que de
        // l'épaisseur de la barre du haut).
        "topbarTextSize": refs.dock.topbar_text_size().map(|px| px.round() as i32),
        "topbarTextAutoPx": crate::ui::topbar::topbar_text_font_px_auto(*refs.top_thickness),
        "topbarTextMaxPx": crate::ui::topbar::topbar_text_font_px_max(*refs.top_thickness),
        // Marge de case des icônes de zone — lue depuis la CELLULE GLOBALE, comme
        // `notificationBlink`/`trayMirror` : c'est le module qui porte l'état, pas
        // `SettingsRefs`.
        "zoneCellMargin": crate::ui::dock::zone_cell_margin().round() as i32,
        // Couleur des séparateurs (voir `ui::dock::Dock::separator_color`) —
        // `{mode: "auto"}` : suit la couleur du texte de la barre,
        // `{mode: "transparent"}` : invisible, `{mode: "custom", hex}` :
        // couleur choisie — DEMANDE EXPLICITE ("donnes la possibilité de
        // choisir leur couleur ou de les rendre transparent").
        "separatorColor": match refs.dock.separator_color() {
            None => json!({ "mode": "auto" }),
            Some(c) if c[3] <= 0.0 => json!({ "mode": "transparent" }),
            Some(c) => json!({ "mode": "custom", "hex": hex([c[0], c[1], c[2]]) }),
        },
        // Taille des séparateurs (voir `ui::dock::Dock::separator_size`) —
        // DEMANDE EXPLICITE ("ajoute la possibilité de modifier la taille
        // des séparateurs par interface").
        "separatorSize": refs.dock.separator_size().round() as i32,
        // Taille des vignettes d'aperçu de fenêtres (voir
        // `win32::window_previews`).
        "previewThumbWidth": refs.preview_thumb_width.round() as i32,
        // Durée du fondu du panneau d'aperçus (voir
        // `win32::window_previews::fade_ms`) — lue directement depuis le module,
        // comme `captureVisible` : c'est lui qui porte la valeur, pas
        // `SettingsRefs`.
        "previewFadeMs": crate::win32::window_previews::fade_ms().round() as i32,
        // Réglages de l'aperçu d'un BUREAU VIRTUEL (voir
        // `win32::desktop_previews`) — lus directement depuis le module, comme
        // ci-dessus et pour la même raison : c'est lui qui porte les valeurs, pas
        // `SettingsRefs`.
        "desktopPreviewWidth": crate::win32::desktop_previews::preview_width().round() as i32,
        "desktopPreviewFadeMs": crate::win32::desktop_previews::fade_ms().round() as i32,
        "desktopPreviewHoverDelayMs": crate::win32::desktop_previews::hover_delay_ms().round() as i32,
        // Fond animé du logo (voir `ui::dock`) — lu depuis le module, même
        // raison que ci-dessus.
        "logoAnimationEnabled": crate::ui::dock::logo_animation_enabled(),
        "logoFadeMs": crate::ui::dock::logo_fade_ms().round() as i32,
        "logoColorMs": crate::ui::dock::logo_color_ms().round() as i32,
        // Icône circulaire qui tourne (voir `ui::dock::logo_rotation`).
        "logoIsCircle": crate::ui::dock::logo_is_circle(),
        "logoSpinClockwise": crate::ui::dock::logo_spin_clockwise(),
        "logoTurnMs": crate::ui::dock::logo_turn_ms().round() as i32,
        // Taille et zoom du logo (voir `ui::dock::logo_draw_scale`), en % et ms.
        "logoSizePct": pct(crate::ui::dock::logo_size()),
        "logoZoomEnabled": crate::ui::dock::logo_zoom_enabled(),
        "logoZoomAmplitudePct": pct(crate::ui::dock::logo_zoom_amplitude()),
        "logoZoomMs": crate::ui::dock::logo_zoom_ms().round() as i32,
        "logoZoomIntervalMs": crate::ui::dock::logo_zoom_interval_ms().round() as i32,
        // Durée du fondu des MENUS et des INFOBULLES (voir `win32::fade`) —
        // lues directement depuis le module, comme `previewFadeMs` ci-dessus et
        // pour la même raison. DEUX champs distincts : les deux curseurs sont
        // séparés (voir `dispatch_actions`).
        "menuFadeMs": crate::win32::fade::menu_fade_ms().round() as i32,
        "tooltipFadeMs": crate::win32::fade::tooltip_fade_ms().round() as i32,
        // Indicateur « en cours d'exécution » (voir
        // `ui::dock::Dock::running_dot_size` et consorts).
        "runningDotSize": refs.dock.running_dot_size().round() as i32,
        "runningDotOffset": refs.dock.running_dot_offset().round() as i32,
        "runningDotOffsetMax": crate::ui::dock::MAX_RUNNING_DOT_OFFSET as i32,
        "iconMargin": crate::ui::dock::icon_margin().round() as i32,
        "runningDotReglages": reglages_indicateur,
        "fonduHautGauche": crate::ui::dock::fondu_haut_gauche().round() as i32,
        "fonduHautDroite": crate::ui::dock::fondu_haut_droite().round() as i32,
        "fonduHautGaucheMax": crate::ui::dock::MAX_FONDU_HAUT_GAUCHE as i32,
        "iconMarginBounds": { "min": crate::ui::dock::MIN_ICON_MARGIN as i32, "max": crate::ui::dock::MAX_ICON_MARGIN as i32 },
        "topIconScale": (crate::ui::dock::top_icon_scale() * 100.0).round() as i32,
        "bottomIconScale": (crate::ui::dock::bottom_icon_scale() * 100.0).round() as i32,
        "topIconScaleBounds": {
            "min": (crate::ui::dock::MIN_TOP_ICON_SCALE * 100.0).round() as i32,
            "max": (crate::ui::dock::MAX_TOP_ICON_SCALE * 100.0).round() as i32,
        },
        // Nom de forme STABLE (voir `RunningDotShape::as_str`) — c'est le
        // contrôle segmenté côté page qui le compare à ses propres valeurs.
        "runningDotShape": refs.dock.running_dot_shape().as_str(),
        // Les formes PROPOSÉES, avec leur libellé, poussées à la page plutôt
        // que recopiées dans le HTML : `RunningDotShape::ALL` reste ainsi la
        // seule liste, et ajouter une forme ne demande aucun changement côté
        // page.
        "runningDotShapes": crate::ui::dock::RunningDotShape::ALL
            .iter()
            .map(|shape| json!({ "value": shape.as_str(), "label": shape.label() }))
            .collect::<Vec<_>>(),
        // Même forme que `separatorColor` ci-dessus : `{mode: "auto"}` suit
        // l'accentuation du dock, `{mode: "custom", hex}` est la couleur
        // choisie.
        "runningDotColor": match refs.dock.running_dot_color() {
            _ if crate::ui::dock::indicateur_degrade() => json!({ "mode": "degrade" }),
            None => json!({ "mode": "auto" }),
            Some(c) if c[3] <= 0.0 => json!({ "mode": "transparent" }),
            Some(c) => json!({ "mode": "custom", "hex": hex([c[0], c[1], c[2]]) }),
        },
        "barSizes": [
            refs.top_thickness.round() as i32,
            refs.left_width.round() as i32,
            refs.right_width.round() as i32,
            refs.bottom_thickness.round() as i32,
        ],
        // VERROU de chaque barre, `[haut, gauche, droite, bas]` — DEMANDE
        // EXPLICITE : « une option dans les réglages pour verrouiller les barres
        // contre tout changement pour éviter les erreurs ». Voir
        // `ui::dock::Dock::bar_locks` et `is_locked_at`, le prédicat que les
        // gestes consultent.
        "barLocks": refs.dock.bar_locks(),
        // LES RACCOURCIS DE DOSSIERS DE LA BARRE DU BAS (voir
        // `config::DossierEpingle`) — la clé est le CHEMIN, jamais un index :
        // la page ne doit pas connaître de position, qui change dès qu'on retire
        // une entrée (même règle que `ZoneRow::arg`).
        //
        // La vignette voyage en `data:` URI : elle est minuscule (256 px de côté
        // au plus, voir `config::DOSSIER_VIGNETTE_MAX_SIDE`), et l'envoyer évite
        // à la page de toucher au disque — elle n'y a de toute façon aucun accès.
        "dossiers": refs
            .dossiers
            .iter()
            // ⚠️ **ON FILTRE LES DOSSIERS** : cette liste est celle des RACCOURCIS,
            // et un SEPARATEUR n'a ni chemin, ni nom, ni vignette (voir
            // `config::ElementBarre`). Les faire figurer ici offrirait des gestes
            // qui ne veulent rien dire sur eux.
            .filter_map(|element| match element {
                config::ElementBarre::Dossier(d) => Some(d),
                config::ElementBarre::Separateur(_) => None,
            })
            .map(|d| {
                json!({
                    "chemin": d.chemin.to_string_lossy(),
                    // Le nom AFFICHÉ, déjà résolu (étiquette ou nom du dossier) :
                    // la page n'a pas à connaître la règle.
                    "nom": d.nom_affiche(),
                    // Vrai si c'est une ÉTIQUETTE choisie — la page s'en sert
                    // pour proposer « revenir au nom du dossier ».
                    "personnalise": d.nom.is_some(),
                    "vignette": d
                        .vignette
                        .as_deref()
                        .and_then(config::load_dossier_vignette)
                        .map(|png| format!("data:image/png;base64,{}", BASE64.encode(&png))),
                })
            })
            .collect::<Vec<_>>(),
        // **LE DOSSIER PERSONNEL DU DASH DOSSIER** — celui sur lequel s'ouvre sa
        // moitié DROITE (voir `config::load_sous_dock_personnel`).
        //
        // Il est poussé RÉSOLU, et non pas tel qu'écrit : la page doit pouvoir
        // dire « c'est le profil » quand rien n'a été choisi, ce qu'un `None`
        // ferait mieux qu'une chaîne vide — mais une chaîne vide se lirait comme
        // un chemin, donc on pousse AUSSI le défaut, qui est toujours un vrai
        // dossier. Le booléen dit lequel des deux on regarde.
        //
        // ⛔ **LES PARENTHÈSES AUTOUR DU BLOC NE SONT PAS DÉCORATIVES** : dans
        // `json!`, une valeur écrite `{ … }` est lue comme un OBJET JSON, pas
        // comme une expression — le macro avale alors tout ce qui suit et échoue
        // en « unexpected end of macro invocation », très loin d'ici. Une
        // parenthèse dit à `json!` que c'est une expression.
        "dossierPersonnel": ({
            // ⛔ **ON PASSE PAR `cef_sous_dock::dossier_personnel`, QUI LE RÉSOUT
            // UNE FOIS — ET C'EST INDISPENSABLE ICI.** `push_state` est appelé
            // **à chaque frame** tant que cette page est visible (`poll`, doc de
            // fonction) ; résoudre ce réglage demande un `read_to_string` puis un
            // `is_dir()` sur un chemin CHOISI PAR L'UTILISATEUR, donc
            // éventuellement un partage réseau — où `is_dir()` attend le délai
            // SMB, des secondes. Par frame, cela gèle l'application. Le dépôt a
            // déjà payé ce genre de coût (voir `cef_sous_dock::RAFALE_DE_POUSSEE`).
            let personnel = crate::win32::cef_sous_dock::dossier_personnel();
            json!({
                "chemin": personnel.actif.to_string_lossy(),
                // **`null` QUAND RIEN N'A JAMAIS ÉTÉ CHOISI**, et non un booléen
                // « est-ce le profil ? » : les deux questions diffèrent dès que le
                // chemin choisi est devenu introuvable. Avec un booléen, l'écran
                // affichait alors le profil SANS le dire — comme s'il avait été
                // choisi — et proposait de « rétablir » un défaut déjà en place.
                // La page montre donc les deux valeurs et dit laquelle s'applique.
                "ecrit": personnel.ecrit.as_ref().map(|c| c.to_string_lossy().into_owned()),
            })
        }),
        // TAILLE du texte des étiquettes de zone (voir
        // `ui::dock::zone_label_font_px`) — un réglage GLOBAL, comme les autres
        // tailles de texte du panneau : c'est le même texte dans toutes les
        // zones, une taille par zone n'aurait pas de sens.
        "zoneLabelFontPx": crate::ui::dock::zone_label_font_px(),
        // ZONES de la barre de droite, pour les verrouiller D'ICI — DEMANDE
        // EXPLICITE : « les deux, réunis au même endroit ». Chaque zone porte de
        // quoi la RECONNAÎTRE (voir `ui::dock::ZoneSummary` : cote, colonnes,
        // nombre d'icônes) et pas seulement son index, qui ne dit rien à
        // personne.
        // **LA HAUTEUR UTILE DE LA BARRE DROITE** — le budget des zones, posé par la page. Voir
        // `SettingsRefs::right_bar_height`, qui porte la raison : les cotes sont des réservations
        // FIXES, donc leur SOMME peut dépasser la barre sans que rien ne le dise.
        "rightBarHeight": refs.right_bar_height.round() as i32,
        "zones": refs.dock.zone_summaries().into_iter().filter_map(|zone| {
            // ⚠️ **LA CLÉ, ET NON L'INDEX — c'est tout l'objet du changement de format.** La page
            // renvoie cette valeur telle quelle dans ses actions, et un RANG (`D3`) ne bouge pas
            // quand une épingle est ajoutée, tandis qu'un INDEX désignerait un AUTRE espace si le
            // dock changeait entre l'affichage du panneau et le clic. Voir `ui::zones`.
            //
            // Une zone SANS clé — un séparateur qu'aucune liste d'ordre ne cite — est ÉCARTÉE :
            // elle n'est jamais dessinée (voir `Dock::zones`), donc rien de ce que la page en
            // ferait ne pourrait aboutir.
            let key = refs.dock.zone_key_of(zone.sep_index)?;
            Some(json!({
                "key": key.to_string(),
                "cote": zone.cote.round() as i32,
                "columns": zone.columns,
                "content": zone.content,
                "locked": zone.locked,
                // **CACHÉE (toute sa plage disparaît) et NON SUPPRIMABLE** — la page en
                // fait une case et l'absence du bouton « Supprimer » (voir `ZoneRecord`).
                "hidden": zone.hidden,
                "permanent": zone.permanent,
                // ...et si elle AFFICHE son numéro/nom en haut d'elle-même — la
                // case de cette section.
                "showsLabel": zone.shows_label,
                // Le NOM et le NUMERO : c'est l'un OU l'autre à l'affichage, et la
                // page ne peut pas le déduire — le numéro suit la POSITION (voir
                // `ZoneSummary::number`), que seule la disposition connaît.
                "name": zone.name,
                "number": zone.number,
                // **LE CONTENU DE LA ZONE, EN CLAIR — la LISTE, là où `content`
                // ci-dessus n'est qu'un COMPTE.** DEMANDE EXPLICITE (2026-09-23) :
                // « dans les réglages de zones je voudrais voir ce qui se trouve
                // dedans ». Chaque ligne porte son nom et l'action qu'on peut lui
                // offrir ; le panneau ne décide de rien, il affiche (voir `ZoneRow`).
                "contents": refs.dock.zone_contents(zone.sep_index).into_iter().map(|entree| match entree {
                    crate::ui::dock::ZoneEntry::Ordered { index, hidden, spacer } => {
                        let ligne = (refs.zone_row)(index);
                        json!({
                            "label": ligne.label,
                            "action": ligne.action,
                            "arg": ligne.arg,
                            "note": ligne.note,
                            // Une applet décochée RESTE dans l'ordre (voir
                            // `set_icon_hidden`) : c'est ce qui permet de la
                            // réafficher depuis cette liste.
                            "hidden": hidden,
                            // `null` pour une icône : la page n'écrit « Espace — N px »
                            // que sur un nombre, jamais sur une supposition.
                            "spacer": spacer,
                        })
                    }
                    // **LE MIROIR, EN UNE LIGNE** — voir `ZoneEntry::Mirror`, et le
                    // commentaire de `set_tray_count` : ses cases SONT dans l'ordre, mais
                    // ce ne sont pas des icônes comme les autres.
                    crate::ui::dock::ZoneEntry::Mirror { count } => json!({
                        "label": format!("Miroir de notification — {count} entrée(s)"),
                        "action": "",
                        "arg": "",
                        "note": "",
                        "hidden": false,
                        "spacer": null,
                    }),
                }).collect::<Vec<_>>(),
            }))
        }).collect::<Vec<_>>(),
        // **LA ZONE QUI REÇOIT LES APPLICATIONS AJOUTÉES DEPUIS LE DASH** (voir
        // `config::load_zones_dash_add_zone_key`, `cef_dash::handle_add_to_zone`). `null`
        // quand aucune n'est désignée — et l'entrée du clic droit n'existe alors
        // pas du tout. La page ne propose que les zones **NOMMÉES** : un libellé
        // « zone 3 » se renumérote tout seul dès qu'une zone au-dessus change de
        // nom, et l'entrée du Dash annoncerait alors autre chose que ce qui a
        // été choisi ici.
        //
        // ⚠️ **UNE CLÉ (`"D3"`), PLUS UN INDEX** : c'est ce qui la rend insensible à l'ajout
        // d'une épingle — et c'est l'ANCIEN index mal translaté (`sep` décalé, `icone` non) qui
        // faisait échouer tout ajout à une zone.
        "dashAddZone": config::load_zones_dash_add_zone_key().map(|key| key.to_string()),
        // **LE RAPPORT DE MIGRATION**, s'il y en a eu une — la bannière en tête de la section
        // Zones, acquittée par `settings/zones-migration-ack`. Lu depuis le FICHIER et non gardé
        // en mémoire : on peut très bien ne pas ouvrir les Réglages dans la session qui suit la
        // mise à jour, et le rapport doit survivre au redémarrage pour être vu.
        "zonesMigration": config::load_zones_migration().map(|table| toml_to_json(&toml::Value::Table(table))),
        // BORNES des trois curseurs de taille de barre, `[haut, gauche,
        // droite]` — **poussées, jamais recopiées dans la page** : elles
        // viennent de `ui::dock::bar_thickness_bounds`, la MÊME fonction que
        // celle dont le curseur du clic droit tire les siennes (voir
        // `win32::cef_bar_size::push_state`). DÉFAUT RÉEL, et c'est ce qui a
        // motivé ce champ : les deux pages portaient chacune son littéral —
        // 160 d'un côté, 500 de l'autre — pour la même grandeur, et rien ne
        // pouvait le signaler.
        //
        // Sans repli côté page, et c'est délibéré : l'état et la page sont
        // livrés par le MÊME binaire, donc un champ absent est un défaut qu'il
        // vaut mieux voir (des bornes visiblement fausses) que masquer par un
        // littéral — c'est exactement ce masque qui a créé l'écart.
        "barSizeMin": bar_size_min,
        "barSizeMax": bar_size_max,
        // Bornes des trois réglages de l'aperçu d'un bureau — POUSSÉES, jamais
        // recopiées dans la page, même règle que `barSizeMin`/`barSizeMax`
        // ci-dessus (et même forme d'objet que `runningDotColor`).
        "desktopPreviewWidthBounds": { "min": desktop_width_min, "max": desktop_width_max },
        "desktopPreviewFadeBounds": { "min": desktop_fade_min, "max": desktop_fade_max },
        "desktopPreviewHoverDelayBounds": { "min": desktop_delay_min, "max": desktop_delay_max },
        // Bornes du fond animé du logo — POUSSÉES, jamais recopiées dans la page.
        "logoFadeBounds": { "min": logo_fade_min, "max": logo_fade_max },
        "logoColorBounds": { "min": logo_color_min, "max": logo_color_max },
        "logoTurnBounds": { "min": logo_turn_min, "max": logo_turn_max },
        "logoSizeBounds": { "min": pct(logo_size_b.0), "max": pct(logo_size_b.1) },
        "logoZoomAmplitudeBounds": { "min": pct(logo_zoom_amp_b.0), "max": pct(logo_zoom_amp_b.1) },
        "logoZoomMsBounds": { "min": logo_zoom_ms_b.0, "max": logo_zoom_ms_b.1 },
        "logoZoomIntervalBounds": { "min": logo_zoom_interval_b.0, "max": logo_zoom_interval_b.1 },
        "leftBarHidden": *refs.left_bar_hidden,
        "rightBarHidden": *refs.right_bar_hidden,
        "topAutoHide": *refs.top_auto_hide,
        "leftAutoHide": *refs.left_auto_hide,
        "rightAutoHide": *refs.right_auto_hide,
        "bottomBarHidden": *refs.bottom_bar_hidden,
        "bottomAutoHide": *refs.bottom_auto_hide,
        "secondaryAutoHide": *refs.secondary_auto_hide,
        // Lu directement depuis l'état RÉEL de Windows (voir `win32::
        // taskbar_autohide::is_enabled`, doc de fonction) — pas de champ
        // `SettingsRefs` dédié, même principe que `blur::blur_passes()`
        // ci-dessous : cet état n'est pas géré par NeuraDesk, juste
        // consulté/modifié en direct.
        "windowsTaskbarAutoHide": crate::win32::taskbar_autohide::is_enabled(),
        "blurPasses": blur::blur_passes(),
        "blurMaterialIndex": material::current_index(),
        "blurMaterialLabels": material::labels().collect::<Vec<_>>(),
        "menuScalePercent": (menu::menu_scale() * 100.0).round() as i32,
        // Magnification au survol des icônes (voir `ui::dock::hover_scale`/
        // `hover_ease_speed`) — GLOBALE, donc lue directement ici sans passer
        // par `refs` : elle vaut pour les icônes du dock comme pour les trois
        // boutons média de la barre du haut. Le facteur voyage en POURCENT
        // entier, seul format qu'un `input[type=range]` règle confortablement
        // (même choix que `menuScalePercent` ci-dessus) ; la vitesse, elle, se
        // règle en unités entières (voir le curseur de `settings.html`).
        "hoverScalePercent": (crate::ui::dock::hover_scale() * 100.0).round() as i32,
        "hoverEaseSpeed": crate::ui::dock::hover_ease_speed().round() as i32,
        // Le TIRAGE des icônes agrandies vers le centre — GLOBAL comme les deux
        // précédents, et lu directement ici pour la même raison. 0 % laisse l'icône
        // centrée sur sa case (donc coupée au bord), 100 % la ramène entièrement :
        // c'est le curseur qui dose les deux.
        "tiragesPercent": crate::ui::dock::BarreTirage::TOUTES
            .map(|b| (crate::ui::dock::tirage_vers_le_centre(b) * 100.0).round() as i32),
        // Magnification au survol des HORLOGES (voir `ui::dock::clock_hover_scale`)
        // — GLOBALE elle aussi, et distincte de celle des icônes ci-dessus : les
        // deux curseurs cohabitent dans la même section du panneau, et c'est ce
        // qui les rend comparables d'un coup d'œil.
        "clockHoverScalePercent": (crate::ui::dock::clock_hover_scale() * 100.0).round() as i32,
        // Transparence des éléments des panneaux (voir `cef_popup::POPUP_ALPHA`)
        // — GLOBALE elle aussi, et lue directement ici sans passer par `refs`.
        "popupAlphaPercent": (crate::win32::cef_popup::popup_alpha() * 100.0).round() as i32,
        "menuCornerRadius": (menu::menu_corner_radius() / menu::menu_scale()).round() as i32,
        "menuTintPercent": tint_percent(menu::menu_tint()),
        "menuColor": hex(menu::menu_accent_rgb()),
        "tooltipScalePercent": (crate::win32::tooltip::tooltip_scale() * 100.0).round() as i32,
        // Le délai en SECONDES, tel quel : la page l'affiche « N s » et son
        // curseur est gradué en secondes (voir `hover_delay`).
        "tooltipDelaySecs": crate::win32::tooltip::hover_delay().as_secs_f32().round() as i32,
        "tooltipEnabled": crate::win32::tooltip::enabled(),
        // Vitesses de défilement à la molette, en pixels par cran, une par barre
        // (voir `ui::dock::scroll_step_px`) — GLOBALES, donc lues directement ici
        // comme `tooltipScalePercent` juste au-dessus.
        "scrollStepLeftPx": crate::ui::dock::scroll_step_px(crate::ui::dock::Side::Left).round() as i32,
        "scrollStepRightPx": crate::ui::dock::scroll_step_px(crate::ui::dock::Side::Right).round() as i32,
        "pleinEcranZone": crate::win32::plein_ecran_zone::est_active(),
        "effetOuverture": crate::win32::ouverture_fenetres::est_active(),
        "jeuxEnFenetre": crate::win32::jeux_fenetres::est_active(),
        "fullscreenClockEnabled": crate::win32::fullscreen_clock::is_enabled(),
        "fullscreenClockPositionTop": crate::win32::fullscreen_clock::is_position_top(),
        "fullscreenClockBarScalePercent": (crate::win32::fullscreen_clock::bar_scale() * 100.0).round() as i32,
        "fullscreenClockTextScalePercent": (crate::win32::fullscreen_clock::text_scale() * 100.0).round() as i32,
        "dashTintPercent": tint_percent(*refs.dash_tint),
        "dashIconScalePercent": (cef_dash::icon_scale() * 100.0).round() as i32,
        "dashLabelScalePercent": (cef_dash::label_scale() * 100.0).round() as i32,
        "dashCategoryScalePercent": (cef_dash::category_scale() * 100.0).round() as i32,
        "clipboardThumbScalePercent": (cef_clipboard::thumb_scale() * 100.0).round() as i32,
        "clipboardTextScalePercent": (cef_clipboard::text_scale() * 100.0).round() as i32,
        "notesTextScalePercent": (cef_notes::text_scale() * 100.0).round() as i32,
        "notesImageScalePercent": (cef_notes::image_scale() * 100.0).round() as i32,
        // Réglage GLOBAL de ce panneau — celui qu'écrit le curseur "Contenu"
        // (voir `dispatch_actions`, action "settings-content-scale"). BUG RÉEL
        // SIGNALÉ (« dans Taille du contenu des dashs, le curseur Contenu a un
        // fonctionnement erratique ») : ce champ portait l'ÉCHELLE D'ÉCRAN
        // (`SCREEN_SCALE_OVERRIDE`, ci-dessous) quand il y en avait une, alors
        // que le curseur, lui, écrit le global — un seul champ pour deux sens.
        // Sur un écran à échelle manuelle (`secondary_screen_scales.txt` :
        // DISPLAY2 à 0,5), le curseur affichait donc 50 % quoi qu'on fasse et
        // ne pouvait jamais tenir la valeur glissée. Les cinq autres curseurs de
        // la même section (`dashIconScalePercent`, `clipboard*`, `notes*`
        // ci-dessus) lisent bien le global : c'est celui-ci qui était
        // l'exception.
        "settingsContentScalePercent": (content_scale() * 100.0).round() as i32,
        "tailleTextesPercent": (taille_panneaux::global(taille_panneaux::TEXTES) * 100.0).round() as i32,
        "tailleNotificationsPercent": (taille_panneaux::global(taille_panneaux::NOTIFICATIONS) * 100.0).round() as i32,
        "panelContentScalePercents": taille_panneaux::PANNEAUX
            .iter()
            .map(|p| (p.to_string(), json!((taille_panneaux::facteur(p) * 100.0).round() as i32)))
            .collect::<serde_json::Map<_, _>>(),
        // Échelle réellement RENDUE par ce panneau — le `zoom` CSS, voir
        // `window.setState` dans assets/popups/settings.html, son SEUL
        // consommateur. C'est le rôle de `SCREEN_SCALE_OVERRIDE` : ouvert sur un
        // écran à échelle manuelle, le panneau s'y affiche à cette échelle-là.
        // Champ SÉPARÉ du précédent à dessein — les confondre ÉTAIT le défaut.
        "settingsContentScaleEffectivePercent": echelle_effective_pourcent,
        "accents": {
            "dash": accent_json(refs.dash_accent),
            "settings": accent_json(refs.settings_accent),
            "clipboard": accent_json(refs.clipboard_accent),
            "notes": accent_json(refs.notes_accent),
            "calendar": accent_json(refs.calendar_accent),
            "minuteur": accent_json(refs.minuteur_accent),
            "surveillance": accent_json(refs.surveillance_accent),
            "calculatrice": accent_json(refs.calculatrice_accent),
            "ssh": accent_json(refs.ssh_accent),
            "terminal": accent_json(refs.terminal_accent),
            "dossier": accent_json(refs.dossier_accent),
            "visionneuse": accent_json(refs.visionneuse_accent),
            "couleurs": accent_json(refs.couleurs_accent),
            "editeur": accent_json(refs.editeur_accent),
            "scanner": accent_json(refs.scanner_accent),
            "shaders": accent_json(refs.shaders_accent),
            "lueurs": accent_json(refs.lueurs_accent),
            "textures": accent_json(refs.textures_accent),
            "moniteur": accent_json(refs.moniteur_accent),
            "recherche": accent_json(refs.recherche_accent),
            "automatisations": accent_json(refs.automatisations_accent),
            "meteo": accent_json(refs.meteo_accent),
            "aide": accent_json(refs.aide_accent),
        },
        // Réglage du rappel des jours fériés (voir `win32::fetes::rappel_feries`)
        // — poussé à chaque frame comme le reste, donc la case reflète l'état
        // réel même si le réglage a été changé ailleurs.
        "rappelFeries": crate::win32::fetes::rappel_feries(),
        // Hébergeur de l'éditeur d'image (voir `envoi_json`).
        "envoi": envoi_json(),
        "customColors": with_data(|d| d.custom_colors.iter().map(|&c| hex(c)).collect::<Vec<_>>()),
        "customThemes": with_data(|d| d.custom_themes.iter().map(|&c| hex(c)).collect::<Vec<_>>()),
        "themeBaseColor": with_data(|d| hex(d.theme_base_color)),
        // **LES DEUX FORCES, POUR QUE LA PAGE LES DESSINE** — en fraction
        // (1 = neutre) ; la page les affiche en pourcentage.
        "themeForces": {
            "saturation": forces_du_theme().0,
            "luminosite": forces_du_theme().1,
        },
        "sauvegarde": {
            "message": SAUVEGARDE_MESSAGE.with(|m| m.borrow().clone()),
            "apercu": SAUVEGARDE_APERCU.with(|a| a.borrow().as_ref().map(|a| json!({
                "fichiers": a.fichiers,
                "date": a.date,
                "apparence": a.sorte == crate::sauvegarde_config::Sorte::Apparence,
            }))),
        },
        "styles": crate::ui::style::integres().iter().map(|s| json!({ "value": s.id, "label": s.nom })).collect::<Vec<_>>(),
        "style": crate::ui::style::id_actif(),
        "coinsZone": crate::ui::dock::coins_zone_actifs(),
        "coinsEcran": crate::win32::coins_ecran::actifs(),
        "lueur": crate::gfx::lueur::active(),
        "lueursEffets": crate::win32::lueurs_effets::noms(),
        "lueursEffet": crate::win32::lueurs_effets::actuel(),
        "coinsEcranRayon": crate::win32::coins_ecran::rayon().round() as i32,
        "coinsEcranRayonBounds": { "min": crate::win32::coins_ecran::RAYON_MIN as i32, "max": crate::win32::coins_ecran::RAYON_MAX as i32 },
        // La police choisie (`""` : celle du style) et celle du style, que le
        // bouton de retour nomme.
        "police": crate::ui::style::police_choisie().unwrap_or_default(),
        "policeDuStyle": crate::ui::style::police_du_style(),
        "themesIcones": crate::ui::icones::INTEGRES.iter().map(|(id, nom)| json!({ "value": id, "label": nom })).collect::<Vec<_>>(),
        "icones": crate::ui::icones::id_actif(),
        "eyedropperTarget": EYEDROPPER_ACTIVE.with(|e| e.borrow().clone()),
        "secondaryScreens": secondary_screens,
        // Le forçage global. ⚠️ **IL N'Y A PAS DE « RATIO AUTOMATIQUE » À PROPOSER**
        // en cochant : le forçage vaut pour TOUS les écrans, et l'automatique
        // diffère de l'un à l'autre (1.0 sur la primaire, 0.67 sur un écran à
        // 100 %). La page propose donc **100 %** — « forcer à ne rien corriger » —
        // et c'est le curseur qui fait le reste.
        "ratioDpiForce": (*refs.ratio_dpi_force * 100.0).round() as i32,
        "catalog": { "themes": catalog_themes, "presets": catalog_presets, "familles": familles_json(), "polices": crate::win32::polices::installees() },
    });
    // ⚠️ **AJOUTÉS APRÈS COUP, ET C'EST FORCÉ** : le `json!` ci-dessus a atteint la
    // limite de récursion de la macro (`recursion_limit = 512`). Les nouvelles
    // clés vont dans `etat_ajoute`, fusionné à plat : la page ne voit aucune
    // différence.
    let mut payload = payload;
    if let (Some(objet), serde_json::Value::Object(ajout)) = (payload.as_object_mut(), etat_ajoute()) {
        objet.extend(ajout);
    }
    // Lit le dock : hors d'`etat_ajoute`, qui n'a pas `refs`.
    if let Some(objet) = payload.as_object_mut() {
        // Couleur INDIVIDUELLE de chaque menu (demande explicite : « personnaliser
        // les menus individuellement ») — AJOUTÉE ICI, après coup, car le `json!`
        // ci-dessus est déjà à la limite de récursion de la macro. La valeur est la
        // couleur RÉELLEMENT affichée : propre au menu si réglée, sinon la couleur
        // PARTAGÉE (`menu_accent_rgb`) ; la page (`MENU_TARGETS`) s'en sert pour
        // amorcer ses sélecteurs.
        objet.insert(
            "menuColors".into(),
            json!(menu::MENU_COLOR_KEYS
                .iter()
                .map(|k| (k.to_string(), json!(hex(menu::menu_color(k).unwrap_or_else(menu::menu_accent_rgb)))))
                .collect::<serde_json::Map<_, _>>()),
        );
        // Les menus ACTUELLEMENT personnalisés — la page n'offre le bouton
        // « Revenir à la couleur partagée » que pour ceux-là.
        objet.insert(
            "menuCustom".into(),
            json!(menu::MENU_COLOR_KEYS.iter().filter(|k| menu::menu_color(k).is_some()).map(|k| k.to_string()).collect::<Vec<_>>()),
        );
        objet.insert("iconGapsBarres".into(), json!([0, 1, 2, 3].map(|b| refs.dock.icon_gap_barre(b).round() as i32)));
        objet.insert("leftIconScale".into(), json!((crate::ui::dock::left_icon_scale() * 100.0).round() as i32));
        // Le BAS a sa propre borne haute, le triple (voir `MAX_BOTTOM_ICON_SCALE`).
        objet.insert(
            "bottomIconScaleBounds".into(),
            json!({
                "min": (crate::ui::dock::MIN_TOP_ICON_SCALE * 100.0).round() as i32,
                "max": (crate::ui::dock::MAX_BOTTOM_ICON_SCALE * 100.0).round() as i32,
            }),
        );
        // Toute la page suit la taille des textes (voir `.root` de settings.html).
        objet.insert("pageScale".into(), json!(taille_panneaux::global(taille_panneaux::TEXTES)));
    }
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
}

/// Les clés d'état ajoutées APRÈS le grand `json!` de `push_state` (voir sa
/// note sur la limite de récursion) — à plat, comme les autres.
/// Applique un réglage ".../<réglage>/<valeur>" des ombres des autres barres
/// (`win32::ombre_barre::ReglagesCotes`, de la noire ou de la colorée) : les
/// cases en 0/1, les largeurs en px, les forces en %. `false` : réglage inconnu
/// ou valeur illisible, rien n'a changé.
fn regler_cotes(r: &mut crate::win32::ombre_barre::ReglagesCotes, reglage: &str, valeur: &str) -> bool {
    let nombre = valeur.parse::<f32>().ok().filter(|v| v.is_finite());
    let case = valeur == "1";
    match (reglage, nombre) {
        ("active", _) => r.active = case,
        ("gauche", _) => r.gauche = case,
        ("droite", _) => r.droite = case,
        ("bas", _) => r.bas = case,
        ("sur-le-bas", _) => r.sur_le_bas = case,
        ("largeur", Some(px)) => r.largeur = px,
        ("force", Some(pc)) => r.force = pc / 100.0,
        ("largeur-bas", Some(px)) => r.largeur_bas = px,
        ("force-bas", Some(pc)) => r.force_bas = pc / 100.0,
        _ => return false,
    }
    true
}

/// Les réglages des ombres des autres barres, pour la page (voir `regler_cotes`).
fn json_cotes(r: &crate::win32::ombre_barre::ReglagesCotes) -> serde_json::Value {
    json!({
        "active": r.active, "largeur": r.largeur.round() as i32, "force": (r.force * 100.0).round() as i32,
        "gauche": r.gauche, "droite": r.droite, "bas": r.bas, "surLeBas": r.sur_le_bas,
        "largeurBas": r.largeur_bas.round() as i32, "forceBas": (r.force_bas * 100.0).round() as i32,
    })
}

/// L'empilement des icônes (voir `ui::empilement`), l'assombrissement en %.
fn json_empilement(e: &crate::ui::empilement::Empilement) -> serde_json::Value {
    json!({
        "actif": e.actif,
        "duree": e.duree_ms,
        "dureeMin": crate::ui::empilement::DUREE_MIN_MS,
        "dureeMax": crate::ui::empilement::DUREE_MAX_MS,
    })
}

/// Le centre de la barre du haut, pour la page (voir `centre_haut::regler`).
fn json_centre_haut(r: &crate::win32::centre_haut::ReglagesCentre) -> serde_json::Value {
    let fondu = |f: &crate::win32::centre_haut::Fondu| {
        json!({ "largeur": f.largeur.round() as i32, "force": (f.force * 100.0).round() as i32, "gauche": f.gauche, "droite": f.droite })
    };
    let [cr, cg, cb] = r.couleur;
    json!({
        "actif": r.actif, "largeur": r.largeur.round() as i32, "hex": format!("{cr:02X}{cg:02X}{cb:02X}"),
        "transparence": (r.transparence * 100.0).round() as i32, "arrondi": r.arrondi,
        "noire": fondu(&r.noire), "coloree": fondu(&r.coloree),
    })
}

fn etat_ajoute() -> serde_json::Value {
    use crate::gfx::texture_interface as texture;
    let couleur = crate::win32::ombre_barre::reglages_couleur();
    json!({
        // La police des TERMINAUX (`""` : la pile par défaut de la page) — la cle que
        // le sélecteur « Police des terminaux » reflète (voir
        // `ui::style::police_des_terminaux`, et le test
        // `la_police_des_terminaux_est_cablee_des_deux_cotes`). Elle va ICI et non
        // dans le `json!` géant de `push_state` : celui-ci est à la limite de
        // récursion de la macro, et la règle du dépôt est que les clefs neuves
        // passent par `etat_ajoute`, fusionné à plat — la page ne voit aucune
        // différence.
        "policeTerminal": crate::ui::style::police_des_terminaux().unwrap_or_default(),
        // La durée de NeuraDesk (`0` = comme Windows), celle que Windows
        // applique (relue : l'utilisateur peut la changer dans Windows), et la
        // liste des choix — la page ne la recopie pas.
        "dureeNotifications": crate::win32::toast_duree::duree(),
        "dureeWindows": crate::win32::toast_duree::duree_windows(),
        "dureesNotifications": crate::win32::toast_duree::DUREES,
        "plancherDureeWindows": crate::win32::toast_duree::PLANCHER_WINDOWS,
        // La vitesse du zoom de la vue sphère, en %, et ses bornes.
        "sphereZoom": crate::win32::cef_sphere::vitesse_zoom(),
        "sphereZoomBounds": { "min": crate::win32::cef_sphere::VITESSE_ZOOM_MIN, "max": crate::win32::cef_sphere::VITESSE_ZOOM_MAX },
        // Les réglages propres à la sphère des TERMINAUX (voir `cef_sphere`).
        "sphereTerminauxZoom": crate::win32::cef_sphere::zoom_defaut(),
        "sphereTerminauxZoomAuto": crate::win32::cef_sphere::zoom_auto(),
        "sphereTerminauxZoomBounds": { "min": crate::win32::cef_sphere::ZOOM_DEFAUT_MIN, "max": crate::win32::cef_sphere::ZOOM_DEFAUT_MAX },
        "sphereTerminauxFonduOuverture": crate::win32::cef_sphere::fondu().0,
        "sphereTerminauxFonduFermeture": crate::win32::cef_sphere::fondu().1,
        "sphereTerminauxFonduBounds": { "min": crate::win32::cef_sphere::FONDU_MIN_MS, "max": crate::win32::cef_sphere::FONDU_MAX_MS },
        // La sphère des PROGRAMMES : son zoom par défaut et son fondu PROPRE
        // (voir `win32::cef_fenetres`) — deux réglages qui ne valent que pour
        // elle, pas pour les autres fenêtres de NeuraDesk.
        "sphereFenetresZoom": crate::win32::cef_fenetres::zoom_defaut(),
        "sphereFenetresVitesseZoom": crate::win32::cef_fenetres::vitesse_zoom(),
        "sphereFenetresVitesseBounds": { "min": crate::win32::cef_fenetres::VITESSE_ZOOM_MIN, "max": crate::win32::cef_fenetres::VITESSE_ZOOM_MAX },
        "sphereFenetresZoomAuto": crate::win32::cef_fenetres::zoom_auto(),
        "sphereFenetresZoomBounds": { "min": crate::win32::cef_fenetres::ZOOM_DEFAUT_MIN, "max": crate::win32::cef_fenetres::ZOOM_DEFAUT_MAX },
        "sphereFenetresFonduOuverture": crate::win32::cef_fenetres::fondu().0,
        "sphereFenetresFonduFermeture": crate::win32::cef_fenetres::fondu().1,
        "sphereFenetresFonduBounds": { "min": crate::win32::cef_fenetres::FONDU_MIN_MS, "max": crate::win32::cef_fenetres::FONDU_MAX_MS },
        // Après une capture : sa fenêtre d'actions (voir `win32::cef_capture`).
        "captureCarte": crate::win32::cef_capture::active(),
        // Les horloges du monde (voir `win32::horloges_monde`).
        "horlogesMonde": crate::win32::horloges_monde::json(),
        "horlogesMondeMax": crate::win32::horloges_monde::MAX_HORLOGES,
        // Le fondu des fenêtres de NeuraDesk (voir `ouverture_fenetres::fondu_actif`).
        "fonduNeuradesk": crate::win32::ouverture_fenetres::fondu_actif(),
        "fonduNeuradeskOuverture": crate::win32::ouverture_fenetres::duree_fondu_ouverture_ms(),
        "fonduNeuradeskFermeture": crate::win32::ouverture_fenetres::duree_fondu_fermeture_ms(),
        "fonduNeuradeskMin": crate::win32::ouverture_fenetres::DUREE_FONDU_MIN_MS,
        "fonduNeuradeskMax": crate::win32::ouverture_fenetres::DUREE_FONDU_MAX_MS,
        // Le décalage de l'horloge plein écran du principal (voir `fullscreen_clock`).
        "fullscreenClockDecalage": crate::win32::fullscreen_clock::decalage_de(crate::win32::fullscreen_clock::ECRAN_PRINCIPAL),
        "fullscreenClockDecalageMax": [crate::win32::fullscreen_clock::DECALAGE_X_MAX, crate::win32::fullscreen_clock::DECALAGE_Y_MAX],
        // La durée de l'effet d'ouverture (voir `win32::ouverture_fenetres`).
        "effetOuvertureDuree": crate::win32::ouverture_fenetres::duree_ms(),
        "effetOuvertureDureeMin": crate::win32::ouverture_fenetres::DUREE_MIN_MS,
        "effetOuvertureDureeMax": crate::win32::ouverture_fenetres::DUREE_MAX_MS,
        "effetFermeture": crate::win32::ouverture_fenetres::fermeture_active(),
        "effetBascule": crate::win32::ouverture_fenetres::bascule_active(),
        "effetFermetureDuree": crate::win32::ouverture_fenetres::duree_fermeture_ms(),
        // La FORME de chaque moment animé (voir `ouverture_fenetres::effets::Effet`).
        "effetOuvertureStyle": crate::win32::ouverture_fenetres::effet_code(crate::win32::ouverture_fenetres::Moment::Ouverture),
        "effetFermetureStyle": crate::win32::ouverture_fenetres::effet_code(crate::win32::ouverture_fenetres::Moment::Fermeture),
        "effetReductionStyle": crate::win32::ouverture_fenetres::effet_code(crate::win32::ouverture_fenetres::Moment::Reduction),
        "effetAgrandissementStyle": crate::win32::ouverture_fenetres::effet_code(crate::win32::ouverture_fenetres::Moment::Agrandissement),
        // Charger `window_hook.dll` dans les applications (voir `win32::injection`).
        "effetInjection": crate::win32::injection::actif(),
        // L'ombre sous la barre du haut (voir `ui::dock::ombre_barre_haut`).
        "ombreBarreHaut": crate::ui::dock::ombre_barre_haut().round() as i32,
        "ombreBarreHautMax": crate::ui::dock::MAX_OMBRE_BARRE_HAUT as i32,
        "ombreBarreHautForce": (crate::ui::dock::ombre_barre_haut_force() * 100.0).round() as i32,
        "ombreNoirePortee": {
            "barres": crate::win32::ombre_barre::portee().barres,
            "zone": crate::win32::ombre_barre::portee().zone,
        },
        "ombreZoneEnCouleur": crate::win32::ombre_barre::zone_en_couleur(),
        // Fond des zones, à part du texte (voir `cef_popup::POPUP_FOND_ALPHA`).
        "popupFondAlphaPercent": (crate::win32::cef_popup::popup_fond_alpha() * 100.0).round() as i32,
        "ombreCouleurDesBarres": crate::win32::ombre_barre::couleur_des_barres(),
        "ombreCouleurAffichee": crate::win32::ombre_barre::couleur_affichee(),
        "ombreCouleurIntensite": crate::win32::ombre_barre::intensite_couleur(),
        "ombreNoireIntensite": crate::win32::ombre_barre::intensite_noire(),
        "ombreCouleurIntensiteMax": crate::win32::ombre_barre::MAX_INTENSITE_COULEUR,
        "ombreCotes": json_cotes(&crate::win32::ombre_barre::reglages_cotes()),
        "ombreCentreHaut": json_centre_haut(&crate::win32::centre_haut::reglages()),
        "empilement": json_empilement(&crate::ui::empilement::reglages()),
        // Les réglages complets de l'ombre colorée (voir `ReglagesCouleur`).
        "ombreCouleur": {
            "largeurMax": crate::win32::ombre_barre::MAX_LARGEUR_COULEUR as i32,
            "largeurHaut": couleur.largeur_haut.round() as i32,
            "forceHaut": (couleur.force_haut * 100.0).round() as i32,
            "cotes": json_cotes(&couleur.cotes),
        },
        // La texture de l'interface (voir `gfx::texture_interface`), en %.
        "texturesInterface": texture::NATURES.iter().map(|(id, nom)| json!({ "value": id, "label": nom })).collect::<Vec<_>>(),
        "textureNature": texture::nature_id(),
        "textureForce": (texture::force() * 100.0).round() as i32,
        "textureEchelle": (texture::echelle() * 100.0).round() as i32,
        "textureClarte": (texture::clarte() * 100.0).round() as i32,
        "textureTeinte": texture::teinte().round() as i32,
        // La texture des terminaux, mêmes unités.
        "textureTerminaux": {
            "nature": texture::TERMINAUX.nature_id(),
            "force": (texture::TERMINAUX.force() * 100.0).round() as i32,
            "echelle": (texture::TERMINAUX.echelle() * 100.0).round() as i32,
            "clarte": (texture::TERMINAUX.clarte() * 100.0).round() as i32,
            "teinte": texture::TERMINAUX.teinte().round() as i32,
        },
        "textureEchelleBounds": {
            "min": (texture::MIN_ECHELLE * 100.0) as i32,
            "max": (texture::MAX_ECHELLE * 100.0) as i32,
        },
    })
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(result) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    /// ⛔ La transparence de la barre du BAS était ignorée EN SILENCE — défaut
    /// SIGNALÉ (2026-10-15) : « la transparence de la barre du bas n'est pas
    /// prise en compte et sa valeur ne se sauvegarde pas ». `parse_bar` ne
    /// connaît pas « bottom », et c'est voulu (voir sa doc) : c'est
    /// `parse_barre_principale` qui l'ajoute, pour la barre principale seule.
    /// Chaque panneau de `ACCENT_TARGETS` (la page) est une cible connue, avec
    /// sa palette : sinon `window_static` retombe sur « dash » et la couleur
    /// s'écrit EN SILENCE dans le fichier du Dash (défaut déjà payé).
    #[test]
    fn chaque_couleur_de_panneau_de_la_page_a_sa_cible() {
        let page = include_str!("../../assets/popups/settings.html");
        let debut = page.find("const ACCENT_TARGETS = [").expect("ACCENT_TARGETS");
        let liste = &page[debut..debut + page[debut..].find("];").expect("fin")];
        let cles: Vec<&str> = liste.split("\", \"").skip(1).map(|s| s.split('"').next().unwrap_or("")).collect();
        assert!(cles.len() >= 23, "{cles:?}");
        for cle in cles {
            assert_eq!(leak_known_target(cle), Some(cle), "{cle}");
            let palette = format!("{cle}_palette");
            assert!(leak_known_target(&palette).is_some(), "{palette}");
        }
    }

    /// Parité page ↔ Rust des MENUS réglables : chaque clé de la page
    /// (`MENU_TARGETS`) doit exister dans `menu::MENU_COLOR_KEYS` — sinon
    /// `set_menu_color` l'ignore EN SILENCE (voir sa doc), le même piège que
    /// `leak_known_target` évite pour les panneaux — et réciproquement, aucune
    /// clé Rust ne doit manquer de la page (un menu réglable mais invisible).
    #[test]
    fn chaque_menu_reglable_est_dans_la_page_et_inversement() {
        let page = include_str!("../../assets/popups/settings.html");
        let debut = page.find("const MENU_TARGETS = [").expect("MENU_TARGETS");
        let fin = debut + page[debut..].find("];").expect("fin de MENU_TARGETS");
        let liste = &page[debut..fin];
        for cle in menu::MENU_COLOR_KEYS {
            // Chaque entrée s'écrit `["Label", "cle"]` : la clé finit la paire.
            assert!(liste.contains(&format!("\"{cle}\"]")), "clé {cle} absente de MENU_TARGETS");
        }
        // Autant d'entrées dans la page que de clés Rust (pas de menu orphelin).
        assert_eq!(liste.matches("\"]").count(), menu::MENU_COLOR_KEYS.len(), "{liste}");
    }

    #[test]
    fn la_barre_du_bas_est_une_barre_principale_mais_pas_secondaire() {
        let index = |nom| parse_barre_principale(nom).map(super::bar_index);
        assert_eq!(["top", "left", "right", "bottom"].map(index), [Some(0), Some(1), Some(2), Some(3)]);
        assert_eq!(index("separator"), None);
        assert!(parse_bar("bottom").is_none(), "les tableaux d'un écran secondaire n'ont que trois cases");
    }

    use super::*;

    /// ⛔ **LE RATIO DE DPI MULTIPLIE LES DEUX CHEMINS**, comme pour le Dash et le
    /// Presse-papiers — et c'est **la seule échelle RENDUE** du panneau.
    ///
    /// ⚠️ **IL NE DOIT PAS ATTEINDRE LE CURSEUR GLOBAL** : `settingsContentScalePercent`
    /// est la valeur que l'interface AFFICHE, et la multiplier ferait mentir le
    /// curseur sur ce qu'il règle. Les confondre était déjà un défaut signalé
    /// (voir le commentaire long de `push_state`) ; ce test-ci ne couvre que la
    /// moitié « échelle rendue », l'autre étant tenue par la lecture directe de
    /// `content_scale()` dans le `json!`.
    #[test]
    fn le_ratio_de_dpi_multiplie_les_deux_chemins() {
        // Chemin 1 — AUCUN réglage par écran → le global, × le ratio.
        assert_eq!(echelle_effective(None, 0.8, 0.5), 0.4);
        // Chemin 2 — réglage par écran → LUI AUSSI × le ratio.
        assert_eq!(echelle_effective(Some(1.0), 0.8, 0.5), 0.5);
        // ⭐ Sur la PRIMAIRE (ratio 1.0), rien ne bouge — c'est la propriété qui
        // rend ce lot sûr, et elle vaut pour les deux chemins.
        assert_eq!(echelle_effective(None, 0.8, 1.0), 0.8);
        assert_eq!(echelle_effective(Some(0.25), 0.8, 1.0), 0.25);
    }

    /// **LES DEUX MOITIÉS DE LA LISTE DES CURSEURS DU DÉGRADÉ DOIVENT RESTER
    /// ENSEMBLE — ET RIEN NE LES Y TENAIT.**
    ///
    /// `gfx::degrade::REGLAGES` vit en Rust, et la liste qui les REND vit dans
    /// la page (`DEGRADE_REGLAGES`, `settings.html`). Les valeurs, elles,
    /// passent par NOM (`state.degradeReglages`), donc une entrée absente de la
    /// page ne casse **rien de visible côté Rust** : elle est poussée, présente
    /// dans l'état, et simplement **jamais dessinée**.
    ///
    /// C'est arrivé : la page s'était arrêtée à `errance`, et les quatre
    /// réglages ajoutés ensuite — `relief`, `lumiere`, `cosinus`,
    /// `perspective` — n'apparaissaient nulle part. Défaut signalé : « je ne
    /// vois pas les nouveaux curseurs ».
    ///
    /// Même discipline que le test des bascules d'écran secondaire (voir
    /// `app`), qui vérifie lui aussi DEUX moitiés écrites à deux endroits.
    /// Une rangée de boutons (`segmented`) n'est PAS reconstruite quand seule sa
    /// valeur change : `updateValues` doit la rafraîchir par `updateSegmented`,
    /// sinon le bouton surligné reste l'ancien. Défaut signalé (2026-10-11) sur
    /// les Icônes : « la surbrillance du bouton ne se met pas à jour ».
    ///
    /// Seules exceptions : les rangées de l'envoi, que `computeStructureKey`
    /// couvre (choisir l'hébergeur change la section entière).
    #[test]
    fn chaque_rangee_de_boutons_est_rafraichie() {
        const PAGE: &str = include_str!("../../assets/popups/settings.html");
        const PAR_LA_STRUCTURE: [&str; 2] = ["envoi-service", "envoi-duree"];
        let mut vues = 0;
        for (debut, _) in PAGE.match_indices("segmented(box, {").chain(PAGE.match_indices("segmented(container, {")) {
            // La DÉFINITION de la fonction n'est pas une rangée.
            if PAGE[..debut].ends_with("function ") {
                continue;
            }
            let bloc = &PAGE[debut..debut + PAGE[debut..].find("});").expect("appel non fermé")];
            let cle = bloc.split("dataKey: \"").nth(1).and_then(|r| r.split('"').next()).expect("rangée sans dataKey");
            vues += 1;
            if PAR_LA_STRUCTURE.contains(&cle) {
                continue;
            }
            assert!(
                PAGE.contains(&format!("updateSegmented(\"{cle}\"")) || PAGE.contains(&format!("updateSegmented(\n        \"{cle}\"")),
                "la rangée « {cle} » n'est jamais rafraîchie par `updateSegmented`"
            );
        }
        assert_eq!(vues, 16, "nombre de rangées de boutons dans la page (la texture de l'interface, puis celle des terminaux, le 2026-10-15 ; la couleur des ombres, le 2026-10-19 ; les coins du centre de la barre du haut, le 2026-10-31 ; la forme du retour à la fermeture, le 2026-11-04 ; les formes de l'ouverture, de la réduction et de l'agrandissement, le 2026-11-05)");
    }

    #[test]
    fn les_curseurs_du_degrage_sont_tous_dans_la_page() {
        const PAGE: &str = include_str!("../../assets/popups/settings.html");
        let debut = PAGE.find("const DEGRADE_REGLAGES = [").expect("liste absente de la page");
        let fin = debut + PAGE[debut..].find("];").expect("liste non terminee");
        let bloc = &PAGE[debut..fin];
        // Le PREMIER littéral de chaque entrée — les entrées sont
        // `["nom", "libellé", min, max]`.
        let dans_la_page: Vec<&str> =
            bloc.match_indices("[\"").filter_map(|(i, _)| bloc[i + 2..].split('"').next()).collect();
        // `couleurs` est le SEUL qui n'a pas à y figurer : la page le rend en
        // BOUTONS (un nombre de 2 à 5 se choisit, il ne se règle pas au
        // curseur), et c'est déjà ce que fait `settings.html` juste au-dessus.
        let attendus: Vec<&str> =
            crate::gfx::degrade::REGLAGES.iter().map(|r| r.nom).filter(|n| *n != "couleurs").collect();
        assert_eq!(
            dans_la_page, attendus,
            "un reglage ajoute a `gfx::degrade::REGLAGES` doit aussi etre rendu par la page"
        );
    }

    /// **TOUTE BASCULE A SA CASE, ET TOUTE CASE SON ÉTAT.**
    ///
    /// ⚠️ **LE DÉFAUT QUE CE TEST FERME EXISTAIT EN DEUX EXEMPLAIRES**, et les deux
    /// ont le même air de famille que le `"calendar"` documenté au-dessus de sa clé :
    /// la page rend une case par entrée de `TOPBAR_FEATURES`/`DASH_FEATURES` en
    /// lisant `state.features[name]` (voir sa boucle dans `renderInterfaceSections`).
    ///
    /// | ce qui manque | ce que ça donne |
    /// |---|---|
    /// | l'état poussé (`couleurs`) | la case **s'affiche décochée quel que soit le réglage enregistré**, tout en fonctionnant quand on la clique |
    /// | l'entrée dans la page (`aide`) | **aucune case du tout** — l'applet ne peut être ni activée ni désactivée |
    ///
    /// ⭐ **LE SECOND ÉTAIT DÉJÀ ARRIVÉ POUR `calendar`**, corrigé le 2026-09-23 — et
    /// **aucun test n'avait retenu la leçon** : il a fallu qu'un utilisateur s'en
    /// aperçoive deux fois. C'est ce trou-là que ce test bouche.
    ///
    /// ⚠️ **LES TROIS LISTES SONT LUES COMME DU TEXTE**, faute de mieux : elles vivent
    /// dans deux langages et un littéral `json!`, et les remonter en valeurs testables
    /// demanderait de reconstruire tout `SettingsRefs`. C'est déjà la discipline de ce
    /// module (`les_curseurs_du_degrage_sont_tous_dans_la_page`, juste au-dessus) et
    /// d'`app` (`une_bascule_absente_d_un_ecran_secondaire_n_y_est_pas_proposee`) :
    /// **pour une liste écrite deux fois, le test lit les deux moitiés**.
    ///
    /// ⚠️ **CE QUI DÉFINIT « BASCULABLE »** : les bras de `app::set_feature_by_name` —
    /// littéralement « l'utilisateur peut l'éteindre par son nom ». Les autres listes
    /// en dérivent, donc c'est celle-ci qui fait foi.
    ///
    /// **MUTATION** : retirer la clé `"aide"` du JSON, ou l'entrée `["aide", …]` de la
    /// page — le test nomme le côté qui manque, et dit lequel des deux symptômes.
    #[test]
    fn chaque_bascule_a_sa_case_et_chaque_case_son_etat() {
        const PAGE: &str = include_str!("../../assets/popups/settings.html");
        const SOURCE_APP: &str = include_str!("../app.rs");
        const SOURCE_ICI: &str = include_str!("cef_settings.rs");

        // Les noms des littéraux `["nom", "libellé"]` d'un tableau de la page.
        fn tableau(nom: &str) -> Vec<String> {
            let ouvre = format!("const {nom} = [");
            let debut = PAGE.find(&ouvre).unwrap_or_else(|| panic!("{nom} absente de settings.html"));
            let bloc = &PAGE[debut..];
            let fin = bloc.find("\n    ];").expect("tableau non terminé");
            bloc[..fin].match_indices("[\"").filter_map(|(i, _)| bloc[i + 2..].split('"').next()).map(str::to_string).collect()
        }

        // Les premiers littéraux des lignes `"nom" <suffixe>`, pour un suffixe donné.
        fn noms(bloc: &str, suffixe: &str) -> Vec<String> {
            bloc
                .lines()
                .filter_map(|ligne| ligne.trim().strip_prefix('"'))
                .filter_map(|reste| reste.split_once('"'))
                .filter(|(_, apres)| apres.trim_start().starts_with(suffixe))
                .map(|(nom, _)| nom.to_string())
                .collect()
        }

        let mut page = tableau("TOPBAR_FEATURES");
        page.extend(tableau("DASH_FEATURES"));

        // `set_feature_by_name` est LA définition de « basculable » : ses bras.
        let debut = SOURCE_APP.find("fn set_feature_by_name").expect("set_feature_by_name absente de app.rs");
        let corps = &SOURCE_APP[debut..];
        let basculables = noms(&corps[..corps.find("\n}").expect("fin de la fonction absente")], "=> &mut features.");

        // Les clés réellement poussées : les lignes `"nom": refs.features.nom,`.
        let poussees = noms(SOURCE_ICI, ": refs.features.");

        for nom in &basculables {
            assert!(
                page.contains(nom),
                "« {nom} » est basculable (`app::set_feature_by_name`) mais `settings.html` ne \
                 propose AUCUNE case : l'applet ne peut être ni activée ni désactivée"
            );
        }
        for nom in &page {
            assert!(
                poussees.contains(nom),
                "« {nom} » a une case dans `settings.html` mais AUCUNE clé dans le JSON de \
                 `push_state` : elle s'affichera DÉCOCHÉE quel que soit le réglage, tout en \
                 fonctionnant quand on la clique"
            );
        }
        for nom in &poussees {
            assert!(page.contains(nom), "« {nom} » est poussée dans l'état mais `settings.html` ne la propose pas");
        }
    }

    /// ⭐ **LA SECONDE FAMILLE DE CASES — CELLES QUI SONT ADOSSÉES À UN MODULE.**
    ///
    /// Le test ci-dessus ne couvre que les APPLETS (`TOPBAR_FEATURES`/
    /// `DASH_FEATURES`), dont l'état passe par `refs.features`. Il en existe une
    /// autre, et elle n'était gardée par **rien** : les cases dont l'état vient
    /// d'une clé DIRECTE du JSON — `notificationBlink`, `trayMirror`, et
    /// `toastsSousLHorloge` depuis le 2026-10-08. Leur forme dans la page est
    /// `checkboxRow(box, state.<cle>, …)`, et leur état est porté par le MODULE,
    /// pas par `SettingsRefs`.
    ///
    /// ⛔ **LEUR DÉFAUT EST EXACTEMENT CELUI QUE LE TEST VOISIN DÉCRIT** :
    /// « elle s'affichera DÉCOCHÉE quel que soit le réglage, tout en
    /// fonctionnant quand on la clique ». Il a déjà été payé deux fois sur la
    /// première famille (`"calendar"`, puis `"couleurs"`) ; rien n'empêchait la
    /// seconde de le payer à son tour.
    ///
    /// **MUTATION** : retirer `"toastsSousLHorloge":` du JSON de `push_state` —
    /// le test le nomme.
    #[test]
    fn chaque_case_adossee_a_un_module_lit_une_cle_du_json() {
        const PAGE: &str = include_str!("../../assets/popups/settings.html");
        // ⛔ **LE FICHIER EST COUPÉ AVANT SON PROPRE MODULE DE TESTS, ET CE N'EST
        // PAS UN DÉTAIL : SANS CETTE COUPE, CE TEST SE LIT LUI-MÊME.** Il cherche
        // `"<cle>":` dans le texte de `cef_settings.rs` — or ce test-ci est DANS
        // `cef_settings.rs`, et sa propre documentation nomme les clés qu'il
        // vérifie. La mutation qui a révélé le défaut : retirer la clé du JSON
        // laissait le test **VERT**, parce qu'il la retrouvait dans son
        // commentaire. Même famille que la sonde qui lit sa propre sortie.
        // (`split`/`next` ne sont pas `const` : c'est un `let`, pas un `const`.)
        let source_complet = include_str!("cef_settings.rs");
        let source_ici = source_complet.split("#[cfg(test)]").next().unwrap_or("");

        // ⚠️ **LE MOTIF EST LA DÉFINITION DE LA FAMILLE** : `checkboxRow(box,
        // state.<cle>, …)`. Une case écrite autrement ne serait pas vue — et
        // c'est pourquoi le compte est vérifié plus bas.
        let motif = "checkboxRow(box, state.";
        let mut cles: Vec<&str> = PAGE
            .match_indices(motif)
            .filter_map(|(debut, _)| {
                let reste = &PAGE[debut + motif.len()..];
                reste.split(|c: char| !c.is_ascii_alphanumeric()).next()
            })
            .filter(|cle| !cle.is_empty())
            .collect();
        cles.sort_unstable();
        cles.dedup();

        assert!(
            cles.len() >= 3,
            "seulement {} case(s) a cle directe trouvee(s) — le motif a change, ce test ne garde plus rien : {cles:?}",
            cles.len()
        );

        for cle in cles {
            let attendue = format!("\"{cle}\":");
            assert!(
                source_ici.contains(&attendue),
                "« {cle} » a une case dans `settings.html` mais AUCUNE cle dans le JSON de \
                 `push_state` : elle s'affichera DECOCHEE quel que soit le reglage, tout en \
                 fonctionnant quand on la clique"
            );
        }
    }

    /// ⭐ **CHAQUE « VOIR L'AIDE » DES RÉGLAGES TOMBE SUR UNE SECTION QUI EXISTE.**
    /// Un `id` renommé dans l'aide ouvrirait la page sans l'amener nulle part.
    #[test]
    fn les_renvois_vers_l_aide_visent_une_section_existante() {
        const PAGE: &str = include_str!("../../assets/popups/settings.html");
        const AIDE: &str = include_str!("../../assets/popups/aide.html");
        let ancres: Vec<&str> = PAGE
            .match_indices("go(\"ouvrir-aide\", \"")
            .filter_map(|(i, m)| PAGE[i + m.len()..].split('"').next())
            .collect();
        assert!(!ancres.is_empty(), "au moins un renvoi (la police manquante)");
        for ancre in ancres {
            assert!(AIDE.contains(&format!("id=\"{ancre}\"")), "aide.html n'a pas de section id=\"{ancre}\"");
        }
    }

    /// ⭐ **LA POLICE DES TERMINAUX EST CÂBLÉE DES DEUX CÔTÉS — ET POUR LES DEUX
    /// PAGES DE TERMINAL.** DEMANDE EXPLICITE (2026-10-28) : « ajoute le réglage de
    /// police des terminaux ». Quatre choses doivent tomber ensemble, chacune
    /// seule capable de donner un réglage qui **ne change rien** tout en ayant l'air
    /// de fonctionner :
    ///
    /// - le verbe émis par la page **et** le bras qui le reçoit dans le dispatch ;
    /// - la cle poussée par `push_state` **et** lue par le sélecteur ;
    /// - la pile unique de `terminal_theme.js`, utilisée par `terminal.html` **et**
    ///   `ssh.html` (un réglage qui se dit « des terminaux » et n'en gouverne
    ///   qu'une moitié ment — et le dépôt a déjà payé « une liste écrite deux fois ») ;
    /// - plus aucune pile écrite en dur dans une page, sinon le réglage ne gouverne
    ///   plus rien.
    ///
    /// **MUTATION** : retirer le bras `["police-terminal", nom]` du dispatch — le
    /// test le nomme. ⛔ Le fichier est coupé avant son propre module de tests : sans
    /// la coupe, ce test se lirait lui-même (sa documentation nomme le verbe).
    #[test]
    fn la_police_des_terminaux_est_cablee_des_deux_cotes() {
        const TERMINAL: &str = include_str!("../../assets/popups/terminal.html");
        const SSH: &str = include_str!("../../assets/popups/ssh.html");
        const REGLAGES: &str = include_str!("../../assets/popups/settings.html");
        const PILE: &str = include_str!("../../assets/popups/terminal_theme.js");
        let dispatch = include_str!("cef_settings.rs").split("#[cfg(test)]").next().unwrap_or("");

        assert!(REGLAGES.contains("go(\"police-terminal\", nom)"), "le sélecteur n'émet pas le verbe");
        assert!(REGLAGES.contains("go(\"police-terminal\", \"\")"), "aucun bouton pour revenir à la pile par défaut");
        assert!(dispatch.contains("[\"police-terminal\"]"), "RIEN ne reçoit le verbe qui remet la pile par défaut");
        assert!(dispatch.contains("[\"police-terminal\", nom]"), "RIEN ne reçoit le verbe qui choisit la police");
        assert!(dispatch.contains("\"policeTerminal\":"), "push_state ne pousse pas la cle que la page lit");
        assert!(REGLAGES.contains("state.policeTerminal"), "le sélecteur ne lit pas la cle de l'état");
        assert!(REGLAGES.contains("updateSegmented(\"terminal-police\""), "la surbrillance du sélecteur ne se rafraîchit pas");
        assert!(PILE.contains("window.pileDesPolicesDesTerminaux = function"), "la pile n'est pas dans la feuille partagée");
        for (nom, page) in [("terminal", TERMINAL), ("ssh", SSH)] {
            assert!(page.contains("pileDesPolicesDesTerminaux(state.policeTerminal)"), "{nom} : la pile réglée n'est pas utilisée à la création");
            assert!(page.contains("s.term.options.fontFamily = pile"), "{nom} : les onglets ouverts ne suivent pas le réglage");
            assert!(!page.contains("DejaVu Sans Mono"), "{nom} : une pile écrite en dur dans la page — le réglage ne la gouvernerait plus");
        }
    }

    /// ⭐ **« CHARIS SIL » EST INSTALLÉE SOUS LE NOM « CHARIS »** (mesuré le
    /// 2026-11-04 : la table `name` du TTF de SIL Global 2025 donne la famille
    /// « Charis », pas « Charis SIL »). Le test d'installation des DEUX
    /// sélecteurs comparait les noms à l'identique : il annonçait donc
    /// « police non installée » pour une police pourtant présente. La règle est
    /// désormais « le nom installé est le DÉBUT du nom demandé », et JAMAIS
    /// l'inverse — sans quoi « Noto Sans Mono » passerait pour installée dès que
    /// sa variante CJK l'est.
    #[test]
    fn le_test_d_installation_accepte_un_nom_de_famille_plus_court() {
        let reglages = include_str!("../../assets/popups/settings.html");
        assert!(reglages.contains("function policeInstallee(active, installees)"), "la règle manque");
        assert_eq!(
            reglages.matches("!policeInstallee(active, installees)").count(),
            2,
            "les deux sélecteurs (style et terminaux) doivent s'en servir"
        );
        assert!(
            !reglages.contains("!installees.includes(active.toLowerCase())"),
            "la comparaison exacte refuserait « Charis SIL » sur un PC où « Charis » est installée"
        );
    }

    /// ⭐ **LES TROIS POSITIONS DU BANDEAU SONT LES MÊMES DES DEUX CÔTÉS.**
    ///
    /// La page porte la liste (valeur + libellé) ; Rust porte
    /// `bandeau::Position::TOUTES` (les valeurs, et ce qui décide du placement).
    /// **Une liste écrite deux fois se vérifie** — c'est la règle du dépôt pour
    /// les verbes d'une page, et elle vaut ici : un nom présent d'un seul côté
    /// donnerait un bouton qui n'écrit rien, ou une position inatteignable.
    ///
    /// ⚠️ **L'ORDRE COMPTE** : il est celui des boutons à l'écran, et il doit
    /// suivre celui de `TOUTES` — gauche, horloge, droite.
    ///
    /// **MUTATION** : renommer `"horloge"` dans la page — le test nomme l'écart.
    #[test]
    fn les_positions_du_bandeau_sont_les_memes_des_deux_cotes() {
        const PAGE: &str = include_str!("../../assets/popups/settings.html");
        let debut =
            PAGE.find("const POSITIONS_BANDEAU = [").expect("liste absente de settings.html");
        let bloc = &PAGE[debut..];
        let fin = bloc.find("\n    ];").expect("liste non terminee");
        // Les VALEURS seulement : chaque entrée est `["<valeur>", "<libellé>"]`,
        // et c'est la première chaîne de chaque paire qui compte.
        let dans_la_page: Vec<String> = bloc[..fin]
            .match_indices("[\"")
            .filter_map(|(i, _)| bloc[i + 2..].split('"').next())
            .map(str::to_string)
            .collect();
        let cote_rust: Vec<String> =
            crate::win32::bandeau::Position::TOUTES.iter().map(|(nom, _)| nom.to_string()).collect();
        assert_eq!(
            dans_la_page, cote_rust,
            "la page et `bandeau::Position::TOUTES` doivent nommer les memes positions, dans le meme ordre"
        );
    }
}

