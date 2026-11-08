//! Fenêtre CEF pour le Dash (icône NeuraDesk, clic gauche) — Phase 5 de
//! `docs/plans/cef-web-ui-migration.md`, le plus gros morceau de cette
//! migration. Remplace `win32::dash` (module egui séparé, PAS un
//! `PopupKind` — comme `win32::cef_clipboard`, cette fenêtre remplit toute
//! la "zone libre" du dock, voir `app::dash_rect`).
//!
//! **Périmètre** : identique à `win32::dash` (voir sa doc de module pour
//! le détail de ce qui reste volontairement hors périmètre par rapport à
//! l'ANCIENNE version WebView2/HTML, historique) — recherche, barre de
//! catégories, grille d'icônes, lancement au clic, actualisation,
//! assignation de catégories par clic droit. PAS le panneau "Gérer les
//! catégories" dédié, les invites d'installation, le lancement en
//! administrateur.
//!
//! **Recherche/filtrage/regroupement CÔTÉ JS** (voir `assets/popups/
//! dash.html`, doc d'en-tête) — DÉLIBÉRÉMENT différent des popups
//! précédents (Sound/Network poussent un état déjà filtré/prêt à
//! afficher) : Rust pousse la liste COMPLÈTE une seule fois
//! (`push_state`), JS filtre/groupe/trie lui-même à chaque frappe — un
//! aller-retour IPC par caractère tapé serait plus lent qu'un filtrage
//! JS pur sur ~350 applications. La logique de tri/filtre EXISTE DONC EN
//! DEUX EXEMPLAIRES (ici en Rust — non appelée pour l'affichage, gardée
//! en commentaire de référence dans `win32::dash.rs` — et dans
//! `dash.html` en JS, la SEULE réellement utilisée par ce module) : un
//! futur changement de règle doit toucher les deux.
//!
//! **Chargement des icônes PAR LOTS, JAMAIS D'UN COUP** (voir
//! `ICON_BATCH_PER_FRAME`, `poll`) — même raison, mêmes bugs réels déjà
//! rencontrés que `win32::dash` (voir sa doc de module) : un scan Shell
//! (`SHGetFileInfoW`/`IImageList::GetIcon`) reste coûteux quel que soit le
//! framework d'affichage, cette contrainte est indépendante d'egui/CEF.
//! Fonctions d'extraction dupliquées de `win32::dash` plutôt que
//! partagées (voir sa doc de module pour la justification détaillée
//! ailleurs dans cette migration) : `win32::dash` reste un chemin de
//! retour intact, jamais touché par cette migration.
//!
//! **Le VRAI rescan tourne sur un THREAD À PART** (voir `spawn_rescan`) —
//! même raison/même mécanisme que `win32::dash::spawn_rescan`, porté ici
//! à l'identique (`std::sync::mpsc`, `COINIT_MULTITHREADED`).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::json;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::Controls::{IImageList, ILD_TRANSPARENT};
use windows::Win32::UI::Shell::{
    SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_SYSICONINDEX, SHGetFileInfoW, SHGetImageList, SHIL_JUMBO,
};
use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, DestroyIcon};

use crate::config;
use crate::gfx::device::GfxDevice;
use crate::ui::dock::Dock;
use crate::gfx::icon::{icon_to_rgba, trim_transparent_padding};
use crate::win32::apps::{self, AppEntry, AppLaunch, CategoryInfo};
use crate::win32::apps_cache;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};

/// Voir `win32::dash::ICON_BATCH_PER_FRAME`, doc détaillée — même valeur,
/// même raison.
const ICON_BATCH_PER_FRAME: usize = 24;

#[derive(Default)]
struct DashData {
    apps: Vec<AppEntry>,
    categories: Vec<CategoryInfo>,
    icon_cache: HashMap<String, String>,
    pending_icon_loads: VecDeque<usize>,
    icon_cache_dirty: bool,
    rescan_rx: Option<Receiver<Vec<AppEntry>>>,
    rescan_dirty: bool,
}

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    static DATA: RefCell<DashData> = RefCell::new(DashData::default());
    /// **OÙ VIT CHAQUE ÉPINGLE** — nom en minuscules → clé de sa zone de la barre
    /// de droite (`D2`). Relevé sur le dock VIVANT par `poll` (seul à l'avoir),
    /// lu par `push_state`, qui n'y a pas accès. Sert au panneau : ne pas offrir
    /// « Ajouter à « Applis » » à une application qui y est déjà.
    static ZONE_DES_EPINGLES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    /// L'ordre de la barre de droite au dernier relevé — `None` à chaque ouverture
    /// (voir `toggle`), pour qu'une épingle ajoutée ou retirée entre-temps soit vue.
    static ORDRE_RELEVE: RefCell<Option<Vec<usize>>> = const { RefCell::new(None) };
    /// `true` dès que le tout premier `toggle` a chargé le cache disque/
    /// lancé le rescan initial — voir `toggle`, doc de fonction : les
    /// ouvertures SUIVANTES réutilisent `DATA` tel quel (même politique
    /// que `win32::dash::DashState`, jamais rechargé à chaque ouverture).
    static INITIALIZED: Cell<bool> = const { Cell::new(false) };
    /// Échelle de l'icône des tuiles (variable CSS `--icon-scale` de
    /// `assets/popups/dash.html`) — voir `icon_scale`/`set_icon_scale`.
    /// Séparée de `LABEL_SCALE` : DEMANDE EXPLICITE de pouvoir régler
    /// chaque élément (icône, étiquette) indépendamment, pas un seul
    /// facteur commun aux deux (voir l'historique de `TILE_SCALE`,
    /// remplacée par ces deux réglages plus fins). Même mécanisme que
    /// `win32::menu::MENU_SCALE` par ailleurs (thread_local chargé au
    /// démarrage par `app::run`, mis à jour par un curseur de `win32::
    /// cef_settings`) mais PAS la même portée : ceci ne touche qu'une
    /// variable CSS relue à chaque `push_state`, pas le zoom CEF de toute
    /// la fenêtre (`content_zoom_level`, réappliqué chaque frame par
    /// `render_if_visible` mais toujours à la MÊME valeur — voir le `match`
    /// de `win32::cef_popup::render`) — donc pris en compte dès la prochaine
    /// ouverture, sans attendre un redémarrage.
    static ICON_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// Échelle de l'étiquette (texte sous l'icône) des tuiles — voir
    /// `ICON_SCALE`, doc détaillée sur pourquoi ce réglage est séparé.
    static LABEL_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// Échelle du panneau de catégories (colonne de droite + en-têtes de
    /// groupe dans la grille, variable CSS `--category-scale` de
    /// `assets/popups/dash.html`) — BUG RÉEL SIGNALÉ : ce panneau n'avait
    /// aucun réglage de taille alors que les tuiles en ont deux
    /// (`ICON_SCALE`/`LABEL_SCALE`). Même mécanisme/mêmes garanties
    /// qu'elles (voir leur doc).
    static CATEGORY_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// Réglage MANUEL par écran secondaire (voir `app::
    /// secondary_screen_scale_override`, `win32::cef_settings::SettingsRefs::
    /// secondary_screen_scales`) — DEMANDE EXPLICITE ("l'utilisateur ait la
    /// possibilité de régler les tailles du dash et des menus et de leurs
    /// éléments pour les différentes interfaces secondaires comme il est
    /// possible de le faire pour l'interface principale"). `[icône,
    /// étiquette, catégories]`, mémorisé ici à l'ouverture (voir `toggle`) et
    /// relu par `push_state` — `None` (barre PRIMAIRE, ou écran secondaire
    /// sans réglage manuel) : `push_state` retombe alors sur `ICON_SCALE`/
    /// `LABEL_SCALE`/`CATEGORY_SCALE` ci-dessus, **ET LE RÉSULTAT EST ENCORE
    /// multiplié par le ratio de DPI ci-dessous** — ⚠️ ce n'est donc plus
    /// « comportement d'origine, inchangé » sur un écran secondaire, contrairement
    /// à ce que cette phrase disait. Volontairement SÉPARÉ de ces trois `Cell` plutôt que
    /// substitué dedans : elles restent la source affichée par le curseur
    /// PRIMAIRE du panneau de Réglages, qui ne doit jamais refléter le
    /// dernier écran secondaire ouvert.
    /// Le second terme est le **RATIO DE DPI** de l'écran visé, appliqué aux DEUX
    /// chemins — le réglage manuel ET les valeurs globales (voir
    /// `set_screen_scale_override`). Sans lui, le Dash garde la même taille
    /// PHYSIQUE partout, donc paraît **1,5× plus grand** sur un écran à 100 % que
    /// sur la primaire à 150 % : c'est le défaut que ce lot corrige, et le Dash est
    /// l'une des trois fenêtres les plus ouvertes.
    static SCREEN_SCALE_OVERRIDE: Cell<(Option<[f32; 3]>, f32)> = const { Cell::new((None, 1.0)) };
}

/// Facteur d'échelle actuel de l'icône des tuiles — voir `ICON_SCALE`.
pub fn icon_scale() -> f32 {
    ICON_SCALE.with(Cell::get)
}

/// Change le facteur d'échelle de l'icône des tuiles, pris en compte à la
/// PROCHAINE `push_state` (ouverture, rescan, ou lot d'icônes) — appelé par
/// `win32::cef_settings` (curseur "Icônes") et par `app::run` au démarrage
/// pour reprendre la valeur persistée (voir `config::load_dash_icon_scale`).
/// Borné à [0.25, 2.0] — plus bas que `win32::menu::set_menu_scale` ([0.5,
/// 2.0]) : DEMANDE EXPLICITE de pouvoir réduire encore plus. Pas de risque
/// de chevauchement en dessous : la tuile (`--tile-scale`) suit le plus
/// grand des deux éléments (voir `assets/popups/dash.html`), donc une icône
/// réduite au minimum reste simplement plus petite dans une tuile qui,
/// elle, ne rétrécit que si l'étiquette rétrécit aussi.
pub fn set_icon_scale(scale: f32) {
    ICON_SCALE.with(|s| s.set(scale.clamp(0.25, 2.0)));
}

/// Facteur d'échelle actuel de l'étiquette des tuiles — voir `LABEL_SCALE`.
pub fn label_scale() -> f32 {
    LABEL_SCALE.with(Cell::get)
}

/// Change le facteur d'échelle de l'étiquette des tuiles — voir
/// `set_icon_scale`, mêmes garanties/bornes exactes (curseur "Texte" plutôt
/// que "Icônes", `config::load_dash_label_scale`).
pub fn set_label_scale(scale: f32) {
    LABEL_SCALE.with(|s| s.set(scale.clamp(0.25, 2.0)));
}

/// Facteur d'échelle actuel du panneau de catégories — voir `CATEGORY_SCALE`.
pub fn category_scale() -> f32 {
    CATEGORY_SCALE.with(Cell::get)
}

/// Change le facteur d'échelle du panneau de catégories — voir
/// `set_icon_scale`, mêmes garanties/bornes exactes (curseur "Catégories"
/// plutôt que "Icônes", `config::load_dash_category_scale`).
pub fn set_category_scale(scale: f32) {
    CATEGORY_SCALE.with(|s| s.set(scale.clamp(0.25, 2.0)));
}

/// Fixe/efface le réglage MANUEL d'échelle de l'écran sur lequel ce Dash
/// vient d'être ouvert — voir `SCREEN_SCALE_OVERRIDE`, doc de champ. À
/// appeler par `app::run` juste avant `toggle` (comme `CONTENT_SCALE` des
/// popups "menu", même schéma).
///
/// ⚠️ **`ratio_dpi` EST LE SECOND ARGUMENT, ET IL EST INDISPENSABLE** : c'est le
/// ratio de DPI de l'écran visé (`win32::dpi::ratio_dpi_de_l_ecran`), **le ratio
/// BRUT et non `app::content_scale_pour`**, qui plancher à 0.5 — un plancher qui
/// protège la lisibilité d'un menu, pas la constance d'un panneau. Il s'applique
/// aux DEUX chemins, réglage manuel comme valeurs globales.
pub fn set_screen_scale_override(scale: Option<[f32; 3]>, ratio_dpi: f32) {
    SCREEN_SCALE_OVERRIDE.with(|s| s.set((scale, ratio_dpi)));
}

/// Les trois échelles EFFECTIVES du Dash : le réglage manuel de l'écran s'il
/// existe, sinon les valeurs globales — **le tout multiplié par le ratio de DPI
/// de l'écran** (voir `SCREEN_SCALE_OVERRIDE`, doc de champ).
///
/// ⛔ **LE RATIO S'APPLIQUE AUX DEUX CHEMINS, ET C'EST TOUT L'ENJEU.** Sans lui,
/// le Dash garde la même taille PHYSIQUE sur tous les écrans, donc paraît **1,5×
/// plus grand** sur un écran à 100 % que sur la primaire à 150 % — et un écran
/// sans réglage manuel ne serait pas corrigé du tout.
///
/// ⚠️ **SÉPARÉE DU `thread_local` POUR ÊTRE VÉRIFIABLE** : la cellule ne s'injecte
/// pas, et c'est cette multiplication-là qui porte la correction.
fn echelles_effectives(choix: Option<[f32; 3]>, globaux: [f32; 3], ratio_dpi: f32) -> [f32; 3] {
    choix.unwrap_or(globaux).map(|v| v * ratio_dpi)
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefDashWindow"),
    window_title: w!("NeuraDesk — Dash"),
    html_file: "dash.html",
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

/// Voir `win32::cef_popup::toggle_with`. Au tout premier appel (voir
/// `INITIALIZED`), charge la liste d'applications depuis le cache disque
/// (repli sur un vrai rescan en tâche de fond si absent, voir
/// `win32::dash::create_window`, même logique) — les ouvertures suivantes
/// ne font que repousser l'état déjà en mémoire.
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice) {
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, || {
        if !INITIALIZED.with(Cell::get) {
            INITIALIZED.with(|c| c.set(true));
            let cached_apps = apps_cache::load_apps();
            let needs_initial_rescan = cached_apps.is_none();
            let apps = cached_apps.unwrap_or_default();
            DATA.with(|d| {
                let mut data = d.borrow_mut();
                data.apps = apps;
                data.categories = apps::all_categories_info();
                data.icon_cache = apps_cache::load_icons();
                data.rescan_rx = needs_initial_rescan.then(spawn_rescan);
            });
        }
        // Renvoie toutes les icônes déjà connues à CHAQUE ouverture, pas
        // seulement la première (BUG RÉEL : sans ceci, les icônes
        // n'apparaissaient plus dès la deuxième ouverture, y compris juste
        // pour voir l'effet d'un nouveau réglage "Icônes"/"Texte" du
        // panneau de Réglages — voir `assets/popups/dash.html`,
        // `window.setState` : vide inconditionnellement son cache JS
        // `icons` à CHAQUE appel, alors que `pending_icon_loads` n'était
        // rempli qu'à l'initialisation ou après un rescan complet, donc
        // plus jamais réémis ensuite). Un HashMap déjà chaud (`icon_cache`)
        // rend ce ré-envoi quasiment gratuit : `icon_data_uri_cached`
        // retourne directement l'entrée en cache, sans repasser par
        // `SHGetFileInfoW`/`IImageList` (le vrai coût, seulement payé sur
        // un raté) — voir sa doc.
        DATA.with(|d| {
            let mut data = d.borrow_mut();
            let count = data.apps.len();
            data.pending_icon_loads = (0..count).collect();
        });
        // Même "zone libre" que le Dash — masque les fenêtres qui s'y
        // superposeraient, voir `win32::dash::toggle`.
        crate::win32::cef_clipboard::hide_if_visible();
        crate::win32::cef_settings::hide_if_visible();
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
    });
    ORDRE_RELEVE.with(|o| *o.borrow_mut() = None);
    push_state();
    // Voir `win32::dash::DashState::focus_search` — demande le focus
    // clavier sur le champ de recherche à CHAQUE ouverture (pas seulement
    // la première), pour pouvoir taper immédiatement sans cliquer.
    cef_popup::execute_js(&SLOT, "document.getElementById('search').focus()");
}

/// À appeler à CHAQUE frame tant que ce popup est visible (voir `app::run`,
/// même endroit que `win32::cef_voicemeeter::poll`) — sonde le rescan en
/// vol, charge un lot d'icônes, applique les actions reçues.
/// `dock` : le dock VIVANT de la boucle principale, prêté le temps de la
/// fonction — le Dash en a besoin pour une seule chose, ranger une application
/// dans une zone (voir `handle_add_to_zone`) : savoir si elle y est DÉJÀ, et où
/// est la FIN de la zone. C'est exactement le prêt que les Réglages reçoivent
/// déjà (`cef_settings::SettingsRefs`).
pub fn poll(dock: &mut Dock) {
    if !cef_popup::is_visible(&SLOT) {
        return;
    }
    // ⚠️ **RELEVÉ SEULEMENT QUAND L'ORDRE DE LA BARRE DE DROITE CHANGE** — un
    // déplacement à chaud (voir `handle_add_to_zone`) doit se refléter dans le
    // panneau, mais `zone_des_epingles` lit les fichiers d'épingles : à chaque
    // frame, ce serait 60 lectures de disque par seconde tant que le Dash est
    // ouvert.
    let ordre = dock.icon_order().1;
    let change = ORDRE_RELEVE.with(|o| {
        let mut releve = o.borrow_mut();
        if releve.as_ref() == Some(&ordre) {
            return false;
        }
        let zones = zone_des_epingles(dock);
        *releve = Some(ordre);
        ZONE_DES_EPINGLES.with(|z| {
            let mut actuel = z.borrow_mut();
            let differe = *actuel != zones;
            *actuel = zones;
            differe
        })
    });
    if poll_rescan() || change {
        push_state();
    }
    if let Some(js) = load_pending_icons() {
        cef_popup::execute_js(&SLOT, &js);
    }
    dispatch_actions(dock);
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

/// Repositionne/redimensionne PUIS dessine — voir `win32::cef_clipboard::
/// render_if_visible`, même raison (cette fenêtre suit `app::dash_rect`,
/// qui peut changer pendant qu'elle est ouverte).
pub fn render_if_visible(rect: [i32; 4], tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::resize_if_needed(&SLOT, rect);
    // `0.0`/`None` : Dash n'est pas un popup "menu" (voir `win32::menu`, doc
    // de module) — pas de coins arrondis, et un zoom CONSTANT (voir
    // `win32::cef_popup::content_zoom_level`) : ni l'un ni l'autre ne suit
    // le curseur "Taille/arrondi des menus" du panneau de réglages. Le zoom
    // est donc réappliqué à chaque frame pour la même valeur — voir le
    // `match` de `win32::cef_popup::render`, qui en donne la raison.
    cef_popup::render_if_visible(&SLOT, tint_rgba, 0.0, cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
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

// --- État Rust→JS ---

fn push_state() {
    // **LES ÉPINGLES RETIRABLES**, par nom : celles de `dock_pins.txt` (voir
    // `PinnedApp::from_extra`). Une épingle écrite à la main dans `config.toml`
    // ne l'est pas — offrir « Retirer du dock » dessus promettrait un geste qui
    // échoue.
    let retirables: Vec<String> = config::load_pinned_apps()
        .into_iter()
        .filter(|p| p.from_extra)
        .map(|p| p.name.trim().to_lowercase())
        .collect();
    let payload = DATA.with(|d| {
        let data = d.borrow();
        // ⛔ **« FRÉQUEMMENT UTILISÉS » SE RECALCULE ICI, À CHAQUE RENDU** — voir
        // `apps::cles_frequentes`, qui porte le défaut mesuré (catégorie restée
        // figée du 4 au 7 octobre 2026 pendant que `usage.toml` s'incrémentait).
        // `push_state` est le SEUL point où la liste part vers la page : la
        // calculer ailleurs laisserait un chemin sans elle.
        let counts = config::load_launch_counts();
        let frequentes = apps::cles_frequentes(data.apps.iter().map(|a| a.name.as_str()), &counts);
        // Les programmes PORTABLES (voir `win32::portables`) : la page leur
        // propose « Retirer de la liste ».
        let portables = crate::win32::portables::charger();
        let apps: Vec<_> = data
            .apps
            .iter()
            .enumerate()
            .map(|(index, app)| {
                let cle = app.name.trim().to_lowercase();
                let epinglee = retirables.contains(&cle);
                let zone = ZONE_DES_EPINGLES.with(|z| z.borrow().get(&cle).cloned());
                // ⚠️ `app.name.to_lowercase()`, SANS `trim` : c'est la forme sous
                // laquelle `record_launch` écrit la clé (voir `cles_frequentes`).
                let categories = apps::categories_avec_frequents(&app.categories, frequentes.contains(&app.name.to_lowercase()));
                let portable = matches!(&app.launch, AppLaunch::Shortcut { target, .. } if crate::win32::portables::contient(&portables, target));
                json!({ "index": index, "name": app.name, "description": app.description, "categories": categories, "overridden": app.overridden, "epinglee": epinglee, "zone": zone, "portable": portable, "admin": app.launch.can_run_as_admin() })
            })
            .collect();
        let all_categories: Vec<String> = data.categories.iter().map(|c| c.name.clone()).collect();
        // **LES CATÉGORIES PERSONNALISÉES, NOMMÉES À PART.** Le panneau « Gérer
        // les catégories » ne peut proposer de supprimer que celles-là (voir
        // `config::delete_custom_category`, no-op sur une catégorie fixe) — et
        // la page ne pouvait PAS les distinguer tant qu'on ne lui envoyait
        // qu'une liste de noms. `CategoryInfo::custom` portait déjà
        // l'information, elle n'était simplement pas transmise.
        let custom_categories: Vec<String> =
            data.categories.iter().filter(|c| c.custom).map(|c| c.name.clone()).collect();
        // Voir `SCREEN_SCALE_OVERRIDE`, doc de champ — un écran secondaire
        // avec un réglage manuel l'emporte sur `ICON_SCALE`/`LABEL_SCALE`/
        // `CATEGORY_SCALE` (la barre PRIMAIRE), sans jamais les modifier.
        // ⚠️ **LE RATIO DE DPI MULTIPLIE LES DEUX CHEMINS**, y compris le repli sur
        // les valeurs globales : celui-ci est relu à CHAQUE frame (l'utilisateur
        // peut bouger le curseur global pendant que le Dash est ouvert), donc le
        // ratio doit s'appliquer ici et pas une fois pour toutes à l'ouverture.
        let (choix, ratio_dpi) = SCREEN_SCALE_OVERRIDE.with(Cell::get);
        // ⭐ La taille des textes GLOBALE (`taille_panneaux::TEXTES`) s'ajoute au
        // ratio : le Dash ne passe pas par `app::content_scale_pour`.
        let [icon, label, category] = echelles_effectives(
            choix,
            [icon_scale(), label_scale(), category_scale()],
            ratio_dpi * crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES),
        );
        json!({
            "apps": apps,
            "allCategories": all_categories,
            "customCategories": custom_categories,
            "zones": zones_proposees(),
            "iconScalePercent": (icon * 100.0).round() as i32,
            "labelScalePercent": (label * 100.0).round() as i32,
            "categoryScalePercent": (category * 100.0).round() as i32,
        })
    });
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
}

// --- Actions JS→Rust ---

/// Vide les actions demandées par `assets/popups/dash.html` (voir sa
/// fonction `go()`, format `"dash/<verbe>[/<segments>]"`).
fn dispatch_actions(dock: &mut Dock) {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(rest) = action.strip_prefix("dash/") else {
            eprintln!("[neuradesk][cef] action dash inconnue : {action:?}");
            continue;
        };
        let parts: Vec<&str> = rest.split('/').collect();
        // ⭐ **APRÈS UN CHANGEMENT DE CATÉGORIE OU DE ZONE, LE DASH SE REDESSINE TOUT DE
        // SUITE** — demande de l'utilisateur (2026-11-01) : « il faudrait rafraîchir le
        // dash du dock automatiquement ». Avant, une catégorie n'apparaissait qu'à la
        // fin du rescan complet (plusieurs secondes), et une épingle ne repoussait
        // l'état que si la liste des zones avait changé.
        let rafraichir = match parts.as_slice() {
            ["launch", index] => {
                handle_launch(index);
                false
            }
            ["launch-admin", index] => {
                handle_launch_admin(index);
                false
            }
            ["rescan"] => {
                request_rescan();
                false
            }
            ["close"] => {
                cef_popup::fermer_par_geste(&SLOT);
                false
            }
            ["category-toggle", index, name, flag] => {
                handle_category_toggle(index, name, flag);
                true
            }
            ["category-new", index, name] => {
                handle_category_new(index, name);
                true
            }
            ["category-reset", index] => {
                handle_category_reset(index);
                true
            }
            ["add-to-zone", index, cle] => {
                handle_add_to_zone(dock, index, cle);
                relever_les_zones(dock);
                true
            }
            ["remove-from-dock", index] => {
                handle_remove_from_dock(dock, index);
                relever_les_zones(dock);
                true
            }
            ["category-create", name] => {
                handle_category_create(name);
                true
            }
            ["category-delete", name] => {
                handle_category_delete(name);
                true
            }
            ["portable-ajouter"] => {
                handle_portable_ajouter();
                false
            }
            ["portable-retirer", index] => {
                handle_portable_retirer(index);
                false
            }
            _ => {
                eprintln!("[neuradesk][cef] action dash inconnue : {action:?}");
                false
            }
        };
        if rafraichir {
            rafraichir_tout_de_suite();
        }
    }
}

/// Relit la zone de chaque épingle sur le dock VIVANT, maintenant — sans attendre
/// que `poll` voie l'ordre de la barre de droite changer.
fn relever_les_zones(dock: &Dock) {
    let zones = zone_des_epingles(dock);
    ORDRE_RELEVE.with(|o| *o.borrow_mut() = Some(dock.icon_order().1));
    ZONE_DES_EPINGLES.with(|z| *z.borrow_mut() = zones);
}

/// Repousse l'état au Dash TOUT DE SUITE : les choix manuels de catégorie sont
/// réappliqués à la liste en mémoire (le rescan, lent, confirmera — il reste
/// demandé par chaque geste), la liste des catégories est relue, et les icônes
/// repartent (`window.setState` vide leur cache ; `icon_cache` les rend gratuites).
fn rafraichir_tout_de_suite() {
    let choix = config::load_category_overrides();
    DATA.with(|d| {
        let mut data = d.borrow_mut();
        apps::appliquer_les_choix_manuels(&mut data.apps, &choix);
        data.categories = apps::all_categories_info();
        let nombre = data.apps.len();
        data.pending_icon_loads = (0..nombre).collect();
    });
    push_state();
}

fn handle_launch(index: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let launched = DATA.with(|d| {
        let data = d.borrow();
        data.apps.get(index).map(|app| {
            config::record_launch(&app.name);
            app.launch.launch();
        })
    });
    if launched.is_some() {
        // ⛔ **L'ÉTAT EST REPOUSSÉ APRÈS UN LANCEMENT, ET CE N'EST PAS DU ZÈLE** :
        // `record_launch` vient d'écrire dans `usage.toml`, donc « Fréquemment
        // utilisés » a changé — et rien ne le recalculerait avant la PROCHAINE
        // ouverture. Or **un Dash ÉPINGLÉ NE SE FERME PAS au lancement**
        // (`cef_popup::hide_if_visible` sort quand la punaise est posée) : sa
        // liste resterait figée sous les yeux de l'utilisateur, exactement le
        // défaut qu'on vient de corriger, mais en plus discret.
        push_state();
        cef_popup::hide_if_visible(&SLOT);
    }
}

/// ⭐ **« EXÉCUTER EN TANT QU'ADMINISTRATEUR »** — DEMANDE EXPLICITE (2026-10-23) :
/// l'entrée existait au clic droit avant le portage CEF (`ee40806`) et n'était
/// jamais revenue ; `AppLaunch::launch_as_admin` dormait depuis. Même suite qu'un
/// lancement simple (`handle_launch`) : compté dans « Fréquemment utilisés », et
/// le dash se ferme. Un refus dans l'invite UAC est muet (voir `appicon::
/// launch_as_admin`) — rien à remonter.
fn handle_launch_admin(index: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let launched = DATA.with(|d| {
        let data = d.borrow();
        data.apps.get(index).filter(|app| app.launch.can_run_as_admin()).map(|app| {
            config::record_launch(&app.name);
            app.launch.launch_as_admin();
        })
    });
    if launched.is_some() {
        push_state();
        cef_popup::hide_if_visible(&SLOT);
    }
}

/// Voir `win32::dash::toggle_category` — même effet, `app_name`/`current`
/// relus directement ici (pas de contrainte de double-emprunt : cette
/// fonction ne tourne pas dans une fermeture `egui` imbriquée).
fn handle_category_toggle(index: &str, name: &str, flag: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let name = crate::win32::cef_app::percent_decode(name);
    let should_have = flag == "1";
    let Some((app_name, current)) =
        DATA.with(|d| d.borrow().apps.get(index).map(|a| (a.name.clone(), a.categories.clone())))
    else {
        return;
    };
    if should_have {
        for category in &current {
            config::add_category_override(&app_name, category);
        }
        config::add_category_override(&app_name, &name);
    } else {
        if current.len() <= 1 {
            return;
        }
        config::remove_category_override(&app_name, &name);
    }
    request_rescan();
}

/// Voir `win32::dash::add_new_category` — même effet.
fn handle_category_new(index: &str, name: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let name = crate::win32::cef_app::percent_decode(name);
    let Some((app_name, current)) =
        DATA.with(|d| d.borrow().apps.get(index).map(|a| (a.name.clone(), a.categories.clone())))
    else {
        return;
    };
    config::save_custom_category(&name);
    for category in &current {
        config::add_category_override(&app_name, category);
    }
    config::add_category_override(&app_name, &name);
    request_rescan();
}

/// **AJOUTE UN PROGRAMME PORTABLE** — l'exécutable choisi rejoint la liste
/// (voir `win32::portables`), puis un rescan l'y fait apparaître.
fn handle_portable_ajouter() {
    let Some(proprietaire) = cef_popup::hwnd(&SLOT) else { return };
    let Some(exe) = crate::win32::file_dialog::ouvrir(proprietaire, "Ajouter un programme portable", "exe", "Programmes (.exe)")
    else {
        return; // Annuler n'est pas une erreur.
    };
    let message = if crate::win32::portables::ajouter(&exe) {
        request_rescan();
        format!("« {} » est ajouté aux programmes", crate::win32::portables::nom(&exe))
    } else {
        format!("« {} » est déjà dans la liste", crate::win32::portables::nom(&exe))
    };
    crate::diag!("[dash] programme portable : {message}");
}

/// Le programme portable d'un index de la liste, s'il en est un.
fn portable_de(index: &str) -> Option<PathBuf> {
    let index = index.parse::<usize>().ok()?;
    let cible = DATA.with(|d| match d.borrow().apps.get(index).map(|a| &a.launch) {
        Some(AppLaunch::Shortcut { target, .. }) => Some(target.clone()),
        _ => None,
    })?;
    crate::win32::portables::contient(&crate::win32::portables::charger(), &cible).then_some(cible)
}

fn handle_portable_retirer(index: &str) {
    let Some(exe) = portable_de(index) else { return };
    if crate::win32::portables::retirer(&exe) {
        request_rescan();
    }
}

fn handle_category_reset(index: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let Some(app_name) = DATA.with(|d| d.borrow().apps.get(index).map(|a| a.name.clone())) else { return };
    config::clear_category_override(&app_name);
    request_rescan();
}

/// **CRÉER UNE CATÉGORIE SANS L'ASSIGNER** — ce que `handle_category_new` ne
/// sait pas faire, et ne DOIT pas savoir : celui-ci vient du clic droit sur une
/// tuile, donc il crée la catégorie ET l'assigne dans la foulée. Ici la
/// catégorie naît **vide**, et c'est exactement ce qu'un panneau de gestion
/// demande — préparer un classement avant d'y ranger quoi que ce soit.
///
/// Une catégorie vide est visible : `apps::all_categories_info` lit
/// `config::load_custom_categories`, donc elle apparaît aussitôt dans le
/// sélecteur du clic droit. C'est la barre latérale qui ne montre que les
/// catégories peuplées (`populatedCategories`, côté page) — un choix assumé,
/// elle sert à filtrer la grille.
fn handle_category_create(name: &str) {
    let name = crate::win32::cef_app::percent_decode(name);
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    // `save_custom_category` refuse déjà un doublon (casse comprise) et un nom
    // de catégorie fixe : aucune de ces règles n'est dupliquée ici.
    config::save_custom_category(name);
    request_rescan();
}

/// **SUPPRIMER UNE CATÉGORIE PERSONNALISÉE** — l'objet même du panneau « Gérer
/// les catégories », et la pièce que le portage CEF avait laissée de côté (voir
/// la doc de module : « PAS le panneau "Gérer les catégories" dédié »).
///
/// `config::delete_custom_category` fait tout le travail délicat, et on n'en
/// duplique AUCUNE règle ici : elle refuse un nom qui n'est pas une catégorie
/// **personnalisée** (une catégorie fixe est du classement intégré au code, il
/// n'y a rien à supprimer), et elle retire ce nom des choix manuels de **toute**
/// application qui le référençait — une application qui n'avait que celui-là
/// retombe alors sur le classement automatique. La suppression est donc
/// cohérente sans qu'on ait à parcourir les applications ici.
fn handle_category_delete(name: &str) {
    let name = crate::win32::cef_app::percent_decode(name);
    config::delete_custom_category(&name);
    request_rescan();
}

/// Le nom de la zone qui reçoit les applications ajoutées depuis le Dash — ou
/// `None` s'il n'y en a pas, auquel cas **l'entrée n'est pas proposée du tout**.
///
/// ⚠️ **LU DANS LA CONFIGURATION, PAS DANS LE DOCK**, et c'est délibéré : ce nom
/// compose l'entrée du menu, et il faut pouvoir l'obtenir LÀ OÙ LE DOCK N'EST PAS
/// — `push_state` est aussi appelé à l'ouverture du panneau, depuis `toggle`,
/// qui ne reçoit que la géométrie et le device. On lit donc `zones.toml`, que
/// les Réglages écrivent au renommage : **la même source que celle qui a servi
/// à désigner la zone**, et désormais la même CLÉ — il n'y a plus d'index à
/// retraduire, donc plus de décalage possible entre les deux.
///
/// Une zone cible **sans nom** ne donne pas d'entrée — contrainte du réglage
/// lui-même (les Réglages ne proposent que les zones nommées) : un libellé
/// « zone 3 » se renumérote tout seul dès qu'une zone au-dessus change de nom.
fn zones_proposees() -> Vec<serde_json::Value> {
    let config::EtatZones::Lues { zones, .. } = config::load_zones() else { return Vec::new() };
    // ⚠️ **LA ZONE DÉSIGNÉE DANS LES RÉGLAGES PASSE EN TÊTE, ET EN ÉVIDENCE** —
    // c'est elle qu'on choisit le plus souvent (« la zone Applis est utile et
    // doit rester »). Le tri est STABLE : les autres gardent l'ordre de la barre.
    let defaut = zones.dash_add_zone;
    let mut liste: Vec<_> = zones
        .zones
        .iter()
        .filter(|(key, _)| key.bar == crate::ui::zones::BarLetter::Droite)
        .filter_map(|(key, record)| Some((*key, record.name.clone()?)))
        .collect();
    liste.sort_by_key(|(key, _)| Some(*key) != defaut);
    liste
        .into_iter()
        .map(|(key, nom)| json!({ "cle": key.to_string(), "nom": nom, "defaut": Some(key) == defaut }))
        .collect()
}

/// **LA PLACE QU'UNE ÉPINGLE OCCUPERA AU PROCHAIN LANCEMENT**, et si elle reste à
/// créer. Voir `place_epingle`, qui la calcule.
struct PlaceEpingle {
    /// `true` si l'épingle reste à ÉCRIRE. La liste des icônes du dock grandira bien
    /// d'une place, **mais AUCUN espace ne se décale pour autant** : les
    /// remplisseurs absorbent la croissance et `winpin_base` est figé (MESURÉ,
    /// `ui::zones::tests::adding_a_dock_pin_shifts_no_spacer`). C'est ce qui a
    /// permis de supprimer la translation des ordres **et**
    /// `config::decaler_les_zones` — voir `handle_add_to_zone`.
    ajoutee: bool,
    /// L'index de l'icône dans le dock **reconstruit**.
    icone: usize,
}

/// **LA ZONE DE CHAQUE ÉPINGLE**, sur la barre de droite : on parcourt son ordre,
/// le dernier séparateur de zone rencontré est la zone courante. Une épingle hors
/// de toute zone (ou sur une autre barre) n'a pas d'entrée.
///
/// ⚠️ **L'INDEX D'UNE ÉPINGLE EST `PINNED_ICON_BASE + son rang`** dans la liste
/// fusionnée — la règle de `Dock::new`, déjà écrite dans `place_epingle`.
fn zone_des_epingles(dock: &Dock) -> HashMap<String, String> {
    let pins = config::load_pinned_apps();
    let mut zones = HashMap::new();
    let mut courante: Option<String> = None;
    for &index in dock.icon_order().1.iter() {
        if dock.is_zone_separator(index) {
            courante = dock.zone_key_of(index).map(|key| key.to_string());
            continue;
        }
        let Some(rang) = index.checked_sub(crate::app::PINNED_ICON_BASE) else { continue };
        if let (Some(pin), Some(cle)) = (pins.get(rang), courante.as_ref()) {
            zones.insert(pin.name.trim().to_lowercase(), cle.clone());
        }
    }
    zones
}

/// **RETIRER DU DOCK UNE APPLICATION DU DASH** — DEMANDE EXPLICITE (2026-10-02) :
/// « je ne vois pas la possibilité de retirer un programme ajouté dans la zone
/// Applis ». Le geste existait déjà dans les Réglages ; il est PARTAGÉ
/// (`cef_settings::retirer_epingle`), pas recopié.
///
/// Le NOM identifie l'épingle, jamais un index du dock — même règle que les
/// Réglages : l'index changerait entre l'affichage du panneau et le clic.
fn handle_remove_from_dock(dock: &mut Dock, index: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let Some(name) = DATA.with(|d| d.borrow().apps.get(index).map(|a| a.name.trim().to_string())) else { return };
    let message = if crate::win32::cef_settings::retirer_epingle(dock, &name) {
        format!("« {name} » est retirée du dock")
    } else {
        "Cette épingle est écrite à la main dans config.toml : NeuraDesk ne la retire pas.".to_string()
    };
    let js = format!("if (window.setAddZoneFeedback) window.setAddZoneFeedback({});", json!(message));
    cef_popup::execute_js(&SLOT, &js);
}

/// **OÙ UNE ÉPINGLE SE RANGERA, DÉCIDÉ SANS RIEN ÉCRIRE.**
///
/// Les épingles sont fusionnées dans l'ordre `config.toml` PUIS `dock_pins.txt`
/// (voir `config::load_pinned_apps`), et `Dock::new` crée une place par épingle à
/// partir de `PINNED_ICON_BASE` : l'index de l'icône est donc ce
/// `PINNED_ICON_BASE` plus le RANG de l'application dans cette liste — et une
/// application absente se range **à la fin**, d'où `pins.len()`.
///
/// ⚠️ **C'EST EXACTEMENT LE CRITÈRE DE `config::save_extra_pin`**, qui refuse sur
/// cette même liste fusionnée. Savoir « l'épingle reste-t-elle à créer ? » sans
/// écrire est ce qui permet à `handle_add_to_zone` de prononcer **tous** ses refus
/// avant sa première écriture.
///
/// Le nom est élagué ici même : `save_extra_pin` écrit `name.trim()`, et comparer
/// la liste avec un nom qui porterait des espaces répondrait « absente » là où
/// l'écriture reconnaîtrait l'épingle.
fn place_epingle(pins: &[config::PinnedApp], name: &str) -> PlaceEpingle {
    let name = name.trim();
    let existante = pins.iter().position(|p| p.name.eq_ignore_ascii_case(name));
    PlaceEpingle {
        ajoutee: existante.is_none(),
        icone: crate::app::PINNED_ICON_BASE + existante.unwrap_or(pins.len()),
    }
}

/// **CE QU'IL FAUT POUR ÉPINGLER L'APPLICATION `index` DU DASH** : son nom, son
/// chemin et son URI, tels que `config::save_extra_pin` les attend — ou `None` si
/// elle ne peut pas devenir une épingle. Partagé par `handle_add_to_zone` et
/// `handle_pin_to_dock`, qui doivent répondre EXACTEMENT la même chose.
fn cible_epinglable(index: usize) -> Option<(String, Option<PathBuf>, Option<String>)> {
    DATA.with(|d| {
        d.borrow().apps.get(index).and_then(|app| match &app.launch {
            AppLaunch::Shortcut { target, .. } => {
                Some((app.name.clone(), Some(target.clone()), None))
            }
            // **UN JEU STEAM OU EPIC AUSSI** — voir `config::PinnedApp::uri`. Son
            // lancement passe par un PROTOCOLE, et son `path` ne peut donc être
            // que son FICHIER D'ICÔNE (`icon_path`, que le lanceur met en cache,
            // voir `AppEntry::icon_path`) : sans icône, il n'y aurait rien à
            // dessiner dans le dock, et on refuse plutôt que d'épingler une case
            // vide.
            AppLaunch::UriProtocol { uri } => {
                Some((app.name.clone(), Some(app.icon_path.clone()?), Some(uri.clone())))
            }
            // **APPLICATION DU STORE** — elle n'a NI exécutable NI fichier
            // d'icône (`chemin = None`), et se lance par son identifiant. C'est
            // **exactement** ce que `AppLaunch::Uwp::launch` fait déjà
            // (`shell:AppsFolder\<AUMID>`) : l'URI porte donc la cible, et
            // l'icône sera redérivée au démarrage depuis l'AUMID (voir
            // `app::pinned_icon_texture`).
            AppLaunch::Uwp { app_user_model_id } => Some((
                app.name.clone(),
                None,
                Some(format!(r"shell:AppsFolder\{app_user_model_id}")),
            )),
        })
    })
}

/// **ÉCRIRE UNE ÉPINGLE NEUVE, LA VÉRIFIER, ET FAIRE SUIVRE LES ORDRES.**
///
/// Rend `false` — sans rien avoir touché d'autre que la tentative d'écriture —
/// si l'épingle n'est pas relue à la place prévue ; l'appelant doit alors
/// s'arrêter AVANT d'écrire l'ordre. Partagé par `handle_add_to_zone` et
/// `handle_pin_to_dock` : c'est la partie délicate (le décalage du bloc des
/// épingles), et deux copies finiraient par diverger.
fn ecrire_la_nouvelle_epingle(
    name: &str,
    chemin: Option<&Path>,
    uri: Option<&str>,
    icone: usize,
    ordres: [&mut Vec<usize>; 4],
    decaler: bool,
    remplisseur_libre: bool,
) -> bool {
    // L'épingle. `place_epingle` a déjà répondu à la seule question que
    // `save_extra_pin` se pose — l'application est-elle dans la liste fusionnée ?
    // — on ne l'appelle donc que lorsqu'il y a réellement une épingle à créer.
    // ⚠️ **`true` = UN LANCEUR, ET C'EST LA DEMANDE.** Une application ajoutée à
    // une zone depuis le Dash devient un RACCOURCI (voir `PinnedApp::launcher`) :
    // elle ne s'allume pas, n'ouvre pas d'aperçu, et ne revendique pas sa
    // fenêtre — si bien que l'application lancée apparaît d'elle-même dans la
    // zone des programmes lancés. DEMANDE EXPLICITE (2026-09-23) : « ça ne
    // devrait être que des lanceurs simple ».
    config::save_extra_pin(name, chemin, uri, true);

    // ⚠️⚠️ **ON NE SE FIE PAS AU `bool` DE `save_extra_pin`, ET LA RAISON EST
    // DANS `config::write_config` : `let _ = fs::write(path, content)`.** Le
    // `true` veut dire « j'ai décidé d'écrire », pas « le fichier contient
    // l'épingle » — toute erreur est jetée (chemin `%APPDATA%` irrésoluble,
    // droits, disque plein, verrou d'antivirus, dossier OneDrive non
    // matérialisé). Le croire ici serait payant : les deux appels qui suivent
    // réécrivent **six** fichiers de zone (`decaler_les_zones`) puis
    // `icon_order.txt`, et un échec silencieux laisserait donc exactement la
    // corruption que ce découpage corrige — mais sans que RIEN ne la signale.
    //
    // **ON RELIT DONC LA LISTE, ET ON VÉRIFIE AUSSI LE RANG.** Présence seule
    // ne suffirait pas : `icone` et la translation des quatre ordres ont été
    // calculés sur un rang PROJETÉ, et une épingle retrouvée à une autre place
    // rendrait ce calcul faux. Pour une épingle neuve le rang est prévisible —
    // `load_pinned_apps` la range toujours LA DERNIÈRE (« Une épingle ajoutée
    // est ainsi toujours la dernière », dit sa doc) — donc exiger l'égalité
    // est légitime, et pas une coïncidence heureuse.
    //
    // ⚠️ **CE `return` EST LE SEUL QUI SUBSISTE SOUS CETTE LIGNE, ET IL Y EST
    // AUTORISÉ PARCE QU'IL NE LAISSE RIEN DERRIÈRE LUI** : à ce point, ni les
    // zones, ni l'ordre, ni le dock n'ont été touchés.
    let relue = config::load_pinned_apps();
    let rang = relue.iter().position(|p| p.name.eq_ignore_ascii_case(name));
    let attendu = Some(icone - crate::app::PINNED_ICON_BASE);
    if rang != attendu {
        crate::diag!(
            "epingle : epingle NON ecrite ou mal rangee (rang {rang:?}, attendu {attendu:?}), ordres NON decales"
        );
        return false;
    }

    // ⚠️⚠️ **PLUS AUCUN DÉCALAGE DE ZONE ICI** — `config::decaler_les_zones` est
    // SUPPRIMÉE, et cet appel avec elle. Elle décalait six fichiers pour rien :
    // c'est le décalage fantôme mesuré (voir la translation supprimée plus haut,
    // et `ui::zones`), et son `+1` appliqué aux cotes mais **pas** à `sep` — lu
    // dans `dash_add_zone.txt` — faisait échouer TOUT ajout à une zone. Les zones
    // vivent désormais dans `zones.toml`, désignées par `barre + rang`, et une
    // épingle ajoutée ne change aucun rang : il n'y a plus rien à décaler, donc
    // plus de « frère oublié » possible.

    // Le sens INVERSE — `ajoutee == false` alors que l'application ne serait pas
    // dans la liste — n'a AUCUNE garde, et n'en a pas besoin : `place_epingle`
    // vient de lire cette liste-là, sur ce nom-là, et rien n'écrit `config.toml`
    // ni `dock_pins.txt` entre-temps. ⚠️ **C'est une dépendance, pas une
    // évidence** : si `place_epingle` en venait un jour à interroger une AUTRE
    // liste (`load_extra_pins`, une comparaison de chemins…), c'est ce sens-là
    // qui écrirait un `icon_order.txt` désignant une icône inexistante.

    // ⭐ **UNE PLACE RÉSERVÉE ÉTAIT LIBRE : RIEN NE GLISSE** (voir
    // `ui::reserve_epingles`) — les applets valent `PINNED_ICON_BASE + capacité + k`,
    // et la capacité n'a pas changé. Le décalage ci-dessous n'est plus que le
    // chemin LENT, quand la réserve est vide.
    if !decaler {
        return true;
    }

    // ⚠️ **LE NOMBRE D'ÉPINGLES VIENT DE CHANGER, DONC LE BLOC DES ÉPINGLES ET DES
    // APPLETS A GLISSÉ D'UN CRAN — ET C'EST ICI QU'IL FAUT LE FAIRE SUIVRE.**
    //
    // L'arithmétique des applets est `PINNED_ICON_BASE + pinned_apps.len()` (voir
    // `app::run`), et les quatre ordres citent des index ABSOLUS : sans ce
    // décalage, l'entrée qui voulait dire « alimentation » désignerait « terminal »
    // au rechargement suivant. `Dock::decaler_le_bloc_des_epingles` porte la
    // démonstration et la borne (`winpin_base` : au-delà, rien ne bouge).
    //
    // **ET C'EST AVANT LE `retain`/`insert` CI-DESSOUS** : la place de la nouvelle
    // épingle est encore libre, donc rien de ce qui vient d'être décalé ne peut la
    // désigner deux fois. Le cas du DÉPLACEMENT (`ajoutee == false`) n'y touche
    // pas : le nombre d'épingles, lui, n'a pas changé.
    // `None` = aucune base persistée : elle est écrite à CHAQUE démarrage (voir
    // `app::run`), donc ce cas ne peut précéder la première action de
    // l'utilisateur — et on ne DEVINE pas la borne pour autant : un décalage
    // trop large déplacerait les zones, un décalage faux est pire que rien.
    match config::load_winpin_base() {
        Some(base) => crate::ui::dock::decaler_bloc_epingles_dans(
            ordres,
            icone,
            limite_du_decalage(base, remplisseur_libre),
            false,
        ),
        None => crate::diag!(
            "epingle : base des epingles de la barre des taches INCONNUE — bloc NON decale"
        ),
    }
    // **LES ÉCRANS SECONDAIRES SUIVENT LE MÊME DÉCALAGE, ET ÇA A MANQUÉ.** Le bloc
    // ci-dessus ne touche que `icon_order.txt` : les quatre listes d'un écran
    // secondaire citent elles aussi des index absolus (voir
    // `config::decaler_les_ordres_secondaires`, qui porte le défaut mesuré et la
    // raison de la borne différente). Sans cet appel, chaque épingle ajoutée
    // re-pointe TOUT leur ordre — les applets y désignent alors des épingles.
    let touchees = config::decaler_les_ordres_secondaires(icone, false);
    if !touchees.is_empty() {
        crate::diag!("epingle : ordres secondaires decales -> {touchees:?}");
    }
    // Les ordres sont maintenant écrits pour une capacité d'un cran de plus : la
    // dire, sans quoi le rechargement relirait l'ancienne et tout serait décalé.
    crate::app::agrandir_la_capacite_des_epingles(crate::app::capacite_epingles() + 1);
    true
}

/// ⛔ **SANS REMPLISSEUR, `winpin_base` N'EST PLUS FIGÉ** — DÉFAUT SIGNALÉ
/// (2026-10-18) : « quand j'ai ajouté l'appli NAPS2 à Applis, ça a cassé la barre
/// de droite ». MESURÉ : capacité 0, `winpin_base` 40, et l'index 39 était une
/// vraie icône — aucun remplisseur entre les applets et les épingles Windows. Le
/// bloc `[3, 40[` a avancé d'un cran, pas ce qui suivait ; au rechargement, la
/// base est passée à 41 et tout au-delà a glissé : les séparateurs de zone (51-54)
/// désignaient une épingle Windows (Lenovo Vantage) et trois espaces, et la barre
/// de droite a perdu ses zones.
///
/// Un remplisseur libre juste sous la base absorbe l'épingle : on s'arrête à la
/// base. Sans lui, la base avance avec le bloc (`app::run` la réécrit au
/// lancement) : TOUT ce qui suit doit avancer aussi.
fn limite_du_decalage(winpin_base: usize, remplisseur_libre: bool) -> usize {
    if remplisseur_libre { winpin_base } else { usize::MAX }
}

/// **ÉPINGLER UNE APPLICATION DU DASH DANS LA ZONE CHOISIE** — ou l'y déplacer
/// si elle est déjà épinglée.
///
/// ⛔ **LA ZONE VIENT DU CLIC, PAS D'UN RÉGLAGE** — DEMANDE EXPLICITE
/// (2026-10-02). Un premier « Épingler au dock » rangeait l'épingle « après la
/// dernière épingle », et celle-ci vivait dans « Applis » : le bouton avait
/// l'air de choisir, et c'était la disposition qui choisissait. La page propose
/// donc les zones nommées de la barre de droite (`zones_proposees`), et renvoie
/// la CLÉ de celle qu'on a cliquée.
///
/// ⭐ **DEPUIS LE 2026-10-10, L'ÉPINGLE NEUVE S'AJOUTE À CHAUD** tant qu'une place
/// réservée est libre (voir `ui::reserve_epingles`). Le paragraphe suivant ne vaut
/// plus que pour le chemin LENT, quand la réserve est vide :
///
/// ⚠️ **L'ICÔNE N'EST PAS AJOUTÉE À CHAUD, ET C'EST LE COMPROMIS RETENU.** La
/// liste des icônes du dock est construite UNE SEULE FOIS, par `Dock::new`, à
/// partir des épingles lues au démarrage (`config::load_pinned_apps`) : il
/// n'existe aucun chemin d'ajout à l'exécution, et en créer un exigerait de
/// décaler tous les index supérieurs — dont ceux que `app::run` calcule en
/// variables locales (`power_icon_index` et consorts). On écrit donc l'épingle et
/// l'ordre, puis on demande le **rechargement** : au prochain lancement, le dock
/// est reconstruit, l'icône comprise, à la place voulue.
fn handle_add_to_zone(dock: &mut Dock, index: &str, cle: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let dire = |message: &str| {
        let js = format!("if (window.setAddZoneFeedback) window.setAddZoneFeedback({});", json!(message));
        cef_popup::execute_js(&SLOT, &js);
    };
    let cle = crate::win32::cef_app::percent_decode(cle);

    // **LA ZONE DE DESTINATION, RÉSOLUE EN INDEX DANS LES LISTES VIVANTES DU DOCK.**
    // `zones.toml` la désigne par une CLÉ (`D3`) et non plus par l'index d'un
    // séparateur : c'est `Dock::spacer_index_at_rank` qui la résout, sur les listes
    // du dock VIVANT. La résolution échoue si le réglage est périmé — on refuse
    // alors, exactement comme pour une zone qui n'ouvre plus (voir plus bas).
    let nom_zone = match config::load_zones() {
        config::EtatZones::Lues { zones, .. } => crate::ui::zones::ZoneKey::parse(&cle)
            .and_then(|key| zones.record(key).and_then(|r| r.name.clone()))
            .unwrap_or_else(|| cle.clone()),
        _ => cle.clone(),
    };
    let Some(sep) = crate::ui::zones::ZoneKey::parse(&cle).and_then(|key| dock.spacer_index_at_rank(dock.orders(), key))
    else {
        crate::diag!("epingler dans une zone : la cle {cle:?} ne se resout plus");
        dire(&format!("La zone « {nom_zone} » n'existe plus dans la barre de droite."));
        return;
    };

    // Le NOM à épingler et l'EXÉCUTABLE. `AppLaunch::Shortcut` est le seul à en
    // porter un : une application UWP/Store ou lancée par protocole ne peut pas
    // devenir une épingle (`PinnedApp` exige un chemin). L'entrée est déjà
    // inerte de son côté (voir `dash.html`) — ici, on refuse sans bruit.
    // (L'épingle sera écrite plus bas ; `ajoutee` dit si la LISTE grandit — et
    // c'est de là que vient le décalage des index, voir la translation.)
    let Some((name, chemin, uri)) = cible_epinglable(index) else {
        // **UN REFUS DOIT SE VOIR, ET SE DIRE.** `PinnedApp` exige un chemin
        // d'exécutable, que seuls les raccourcis `.lnk` portent : une application
        // du Store ou lancée par protocole (Steam, Epic, GOG) ne peut pas être
        // épinglée. Rendre la main en silence — ce que faisait cette branche —
        // donne un bouton qui ne fait RIEN, indiscernable d'une panne.
        //
        // ⚠️ **ET PAS SEULEMENT À L'ÉCRAN** : les refus de cette fonction
        // écrivaient sur `stderr`, que `Start-Process` jette — donc invisibles
        // dans le journal de diagnostic, et l'enquête ne pouvait pas distinguer
        // « refusé » de « jamais arrivé ». `diag!` est le seul canal qui reste.
        crate::diag!("ajout a une zone : l'application {index} ne peut pas etre epinglee (URI sans icone)");
        dire("Cette application ne peut pas être épinglée : son lanceur ne lui a pas fourni d'icône.");
        return;
    };

    // ================== LA DÉCISION SE PREND ICI, SANS RIEN ÉCRIRE ==============
    //
    // ⚠️⚠️ **TOUT CE QUI SUIT JUSQU'AU BLOC D'ÉCRITURE PLUS BAS EST SANS EFFET DE
    // BORD SUR LA CONFIGURATION** — pas « ne lit aucun fichier » : `load_pinned_apps`
    // peut CRÉER `config.toml` au passage, en y posant le modèle commenté quand il
    // manque (voir `load_pinned_apps_from_config_toml`), exactement comme le fait
    // `app::run` au démarrage. Ce qui compte est qu'**aucune épingle, aucun ordre
    // et aucun fichier de zone n'y soit écrit** : c'est ce qui rend les refus
    // inoffensifs. Voir la longue note au bloc d'écriture.

    // **LE NOM EST NORMALISÉ UNE FOIS POUR TOUTES.** `save_extra_pin` écrit le nom
    // ÉLAGUÉ : comparer la liste fusionnée avec un nom qui porterait des espaces
    // répondrait « absente » là où l'écriture, elle, reconnaîtrait l'épingle.
    // `ajoutee` se tromperait alors d'un cran — et c'est `decaler_les_zones`
    // comme la translation des index qui partiraient de travers, alors qu'aucune
    // épingle n'aurait été créée.
    let name = name.trim().to_string();
    if name.is_empty() {
        // Impossible par l'interface (une entrée du dash a un nom), mais
        // `save_extra_pin` refuserait d'écrire : s'arrêter ici garde vraie
        // l'équivalence entre `ajoutee` et ce que l'écriture fera réellement.
        crate::diag!("ajout a une zone : l'application {index} n'a pas de nom");
        return;
    }

    // **LA PLACE QUE L'ÉPINGLE AURA AU PROCHAIN LANCEMENT.** Voir `place_epingle`,
    // qui porte la règle et se teste.
    let place = place_epingle(&config::load_pinned_apps(), &name);
    let ajoutee = place.ajoutee;
    let icone = place.icone;
    // ⭐ **UNE PLACE RÉSERVÉE LIBRE ⇒ À CHAUD** (voir `ui::reserve_epingles`) :
    // l'icône existe déjà, masquée, à cet index — rien ne glisse.
    let a_chaud = ajoutee && icone < crate::app::PINNED_ICON_BASE + crate::app::capacite_epingles();

    // ⚠️ **PLUS DE CONFIRMATION** : elle existait parce que créer une épingle était
    // l'EFFET DE BORD d'un bouton « Ajouter à une zone ». Le bouton dit
    // maintenant « Épingler au dock », et la zone est choisie dans la foulée.

    // **OÙ LA RANGER** : la fin de la zone visée. `None` si le réglage est périmé
    // (zone supprimée, barre réordonnée) — on refuse plutôt que de ranger au
    // hasard, comme la doc de `zone_end_order_position` le prescrit.
    let Some(position) = dock.zone_end_order_position(sep) else {
        crate::diag!("ajout a une zone : le separateur {sep} n'ouvre plus de zone");
        dire(&format!("La zone « {nom_zone} » n'ouvre plus de zone."));
        return;
    };

    // L'ORDRE est pris sur le DOCK VIVANT, jamais sur `icon_order.txt` : ce
    // fichier peut ne pas exister (aucun réordonnancement depuis l'installation),
    // et reconstituer ici l'ordre par défaut de `Dock::new` serait recopier une
    // connaissance qui n'appartient qu'au dock.
    // ⚠️⚠️ **ON NE TRANSLATE PLUS RIEN, ET C'EST UNE CORRECTION MESURÉE.** Cette
    // fonction ajoutait `+1` à tout index ≥ celui de l'épingle ajoutée, ORDRES
    // COMPRIS, au motif qu'une place de plus dans la plage des épingles décale
    // tout ce qui suit. **C'est faux, et un test le prouve**
    // (`ui::zones::tests::adding_a_dock_pin_shifts_no_spacer`) : les
    // REMPLISSEURS créés par `ensure_spacer_capacity(winpin_base)` **absorbent**
    // la croissance, et `winpin_base` est FIGÉ par `winpin_base.txt`. La plage
    // des épingles Windows commence donc toujours au même index, et **aucun
    // espace ne bouge d'un cran**.
    //
    // ⚠️ **LE `+1` N'ÉTAIT PAS INOFFENSIF, IL ÉTAIT LA PANNE.** Écrit dans
    // `icon_order.txt`, il faisait désigner aux zones un espace VOISIN — et
    // `config::decaler_les_zones` (supprimée) décalait les fichiers de zone du
    // même cran, si bien que les deux s'accordaient ENTRE EUX tout en étant faux
    // par rapport à la disposition réelle du dock. C'est la désynchronisation de
    // la configuration de l'utilisateur. Les zones se désignent maintenant par
    // RANG (`ui::zones`), donc plus rien n'a besoin d'être translaté.
    //
    // ⚠️ **ET LE RISQUE QUE LE `+1` INVOQUAIT N'EST PAS DEVENU PIRE POUR AUTANT**
    // — un index proche de la fin après le `truncate` des applications
    // dynamiques (`set_dynamic_apps`), qui faisait paniquer le dock
    // (`index out of bounds`, `dock.rs:layout_side`). Le `+1` ne le ramenait pas
    // dans les bornes : il DÉPLAÇAIT l'index d'un cran. La vraie protection est
    // celle qui existe déjà et qui n'a pas bougé — `set_icon_order` écarte en
    // silence un index qui n'existe plus.
    // ⚠️ **LES QUATRE SONT `mut`, ET C'EST LE DÉCALAGE DU BLOC DES ÉPINGLES QUI LE
    // VEUT** (voir `decaler_bloc_epingles_dans`, appelé plus bas) : ajouter une
    // épingle décale les applets d'un cran, donc les QUATRE listes doivent suivre —
    // n'en écrire qu'une laisserait les trois autres désigner l'icône voisine.
    let (mut left, mut right, mut top_left, mut top_right) = dock.icon_order();

    // **DÉJÀ DANS LA ZONE ? On ne fait rien, et on le dit** — décision de
    // l'utilisateur. L'interface le signale aussi (`dash.html`), mais le refus
    // doit exister des DEUX côtés : une page n'est pas une autorité.
    let Some(sep_position) = right.iter().position(|&i| i == sep) else {
        crate::diag!("ajout a une zone : {sep} absent de la barre de droite");
        dire(&format!("La zone « {nom_zone} » n'est plus dans la barre de droite."));
        return;
    };
    let fin_zone = right[sep_position + 1..]
        .iter()
        .position(|&i| dock.is_zone_separator(i))
        .map_or(right.len(), |offset| sep_position + 1 + offset);
    if right[sep_position + 1..fin_zone].contains(&icone) {
        // **« NE RIEN FAIRE, ET LE DIRE »** — le « le dire » est ici : sans ce
        // retour, l'utilisateur verrait un clic sans effet et croirait à une
        // panne. On le renvoie à la page, qui l'affiche dans le panneau.
        crate::diag!("zone de destination : {name:?} y est DEJA, rien a faire");
        dire("Déjà présente dans cette zone");
        return;
    }

    // ======================= LA DÉCISION EST PRISE — ON ÉCRIT ===================
    //
    // ⚠️⚠️ **TOUS LES REFUS SONT PRONONCÉS AU-DESSUS DE CETTE LIGNE, ET C'EST TOUT
    // L'OBJET DE CE DÉCOUPAGE.** Ils étaient rendus *après* l'écriture de
    // l'épingle et le décalage des zones : re-ajouter une application DÉJÀ
    // présente — le geste le plus anodin, donc le plus probable — ressortait par
    // « Déjà présente dans cette zone » en laissant derrière lui une épingle
    // écrite et **cinq fichiers de zone décalés pour rien**, sans qu'aucune icône
    // n'ait bougé. C'est le défaut signalé, et il venait de l'ORDRE, pas du
    // calcul.
    //
    // L'invariant à tenir en modifiant ce qui suit : **une sortie anticipée ne
    // doit jamais laisser une écriture orpheline.** On décide donc d'abord, puis
    // on écrit — et le seul `return` qui subsiste plus bas (celui de l'épingle non
    // écrite) est précisément celui qui constate que rien n'a été écrit.

    // L'épingle, puis le décalage du bloc des épingles : voir
    // `ecrire_la_nouvelle_epingle`. Le DÉPLACEMENT (`ajoutee == false`) n'y touche
    // pas : le nombre d'épingles, lui, n'a pas changé.
    if ajoutee
        && !ecrire_la_nouvelle_epingle(
            &name,
            chemin.as_deref(),
            uri.as_deref(),
            icone,
            [&mut left, &mut right, &mut top_left, &mut top_right],
            !a_chaud,
            // Le remplisseur, s'il en reste un, est l'icône juste sous la base :
            // voir `limite_du_decalage`.
            config::load_winpin_base().is_some_and(|base| base > 0 && dock.is_spacer(base - 1)),
        )
    {
        return;
    }
    // ⭐ **L'ÉPINGLE NEUVE DOIT ÊTRE NOMMABLE TOUT DE SUITE.** Les clés du dock sont posées
    // au démarrage ; une épingle ajoutée à chaud n'en avait AUCUNE, donc la sauvegarde
    // suivante ne pouvait pas l'écrire dans la liste de sa zone — elle serait devenue une
    // orpheline au lancement suivant, reversée dans la barre de gauche. On relit la liste
    // des épingles et on repose les clés, par la MÊME fonction que le démarrage
    // (`app::cle_d_epingle_dock`) : deux formes de clé divergeraient.
    if ajoutee {
        dock.poser_cles_des_pins(
            config::load_pinned_apps().iter().map(crate::app::cle_d_epingle_dock).collect(),
        );
    }

    // Le déplacement : on la retire d'où elle est, PUIS on l'insère. **L'ordre des
    // deux opérations n'est pas indifférent** — retirer une icône située AVANT la
    // position d'insertion décale celle-ci d'un cran, et l'oublier rangerait
    // l'application une place trop loin.
    let ancienne = right.iter().position(|&i| i == icone);
    right.retain(|&i| i != icone);
    // ⚠️ **ET DES TROIS AUTRES BARRES** : une épingle rangée à gauche ou en haut
    // resterait sinon citée DEUX fois, et `Dock::set_icon_order` donne la
    // priorité à la gauche — le déplacement n'aurait aucun effet visible.
    for autre in [&mut left, &mut top_left, &mut top_right] {
        autre.retain(|&i| i != icone);
    }
    let position = match ancienne {
        Some(avant) if avant < position => position - 1,
        _ => position,
    };
    right.insert(position.min(right.len()), icone);

    config::save_icon_order(&left, &right, &top_left, &top_right);
    // ⛔ **ET LA LISTE DE LA ZONE DANS `zones.toml`** — ce sont ses listes qui rangent les
    // barres au lancement, pas `icon_order.txt`. Sans cette ligne, l'épingle (neuve ou
    // déplacée) n'était citée par aucune zone et revenait au redémarrage en orpheline au
    // bout de la barre de gauche (constat de l'utilisateur, 2026-11-01). Pour les TROIS
    // chemins qui suivent : à chaud, à chaud dans la réserve, et rechargement.
    ranger_dans_la_zone_de_zones_toml(&cle, &name);
    // ⛔ **UN DÉPLACEMENT S'APPLIQUE À CHAUD, SANS RECHARGEMENT** — DEMANDE
    // EXPLICITE (« que les zones mises à jour se rechargent dynamiquement »). Il
    // ne change que l'ORDRE, et `Dock::set_icon_order` est pur : aucune icône ne
    // naît, aucun index ne bouge. Une épingle NEUVE, elle, ajoute une icône AVANT
    // les applets, dont `app::run` tient les index en variables locales : elle
    // passe encore par le rechargement (voir la doc de cette fonction).
    if !ajoutee {
        dock.set_icon_order(left, right, top_left, top_right);
        crate::diag!("epingler dans une zone : {name:?} deplacee a chaud dans {cle}");
        dire(&format!("Déplacée dans « {nom_zone} »"));
        return;
    }
    // ⭐ **ÉPINGLE NEUVE DANS UNE PLACE RÉSERVÉE : À CHAUD AUSSI** — DEMANDE
    // (2026-10-10) : « que cette zone seule se recharge, pas toute l'interface ».
    // L'ordre est posé ici ; `app::run` démasque la place et y attache l'icône
    // (`signaler_epingles_changees`), parce que la texture et les listes
    // parallèles des épingles sont ses locales.
    if a_chaud {
        dock.set_icon_order(left, right, top_left, top_right);
        crate::app::signaler_epingles_changees();
        crate::diag!("epingler dans une zone : {name:?} epinglee a chaud dans {cle} (index {icone})");
        dire(&format!("Épinglée dans « {nom_zone} »"));
        return;
    }
    // **ET LE RECHARGEMENT.** Le dock ne relit ses épingles qu'à sa construction :
    // sans cela, l'icône n'apparaîtrait qu'au prochain démarrage manuel — soit
    // exactement l'impression que l'action n'a rien fait.
    dire(&format!("Épinglée dans « {nom_zone} »"));
    crate::app::request_reload();
}

/// Range l'épingle `name` à la fin de la zone `cle` dans `zones.toml` (voir
/// `ui::zones::Zones::ranger_a_la_fin_de`). L'identité est celle que le dock lui donne
/// (`Dock::identite` : `epingle_dock:` + `app::cle_d_epingle_dock`).
fn ranger_dans_la_zone_de_zones_toml(cle: &str, name: &str) {
    let Some(key) = crate::ui::zones::ZoneKey::parse(cle) else {
        crate::diag!("epingler dans une zone : cle {cle:?} illisible, zones.toml non mis a jour");
        return;
    };
    let Some(epingle) = config::load_pinned_apps().into_iter().find(|p| p.name.trim() == name) else {
        crate::diag!("epingler dans une zone : {name:?} absente des epingles, zones.toml non mis a jour");
        return;
    };
    let identite = format!("epingle_dock:{}", crate::app::cle_d_epingle_dock(&epingle));
    let config::EtatZones::Lues { zones, .. } = config::load_zones() else {
        crate::diag!("epingler dans une zone : zones.toml illisible, {identite} non rangee");
        return;
    };
    let mut zones = zones;
    if !zones.ranger_a_la_fin_de(key, &identite) {
        crate::diag!("epingler dans une zone : la zone {cle} n'est pas dans zones.toml");
    } else if config::save_zones(&zones) {
        crate::diag!("epingler dans une zone : {identite} rangee dans {cle} (zones.toml)");
    } else {
        crate::diag!("epingler dans une zone : zones.toml NON ecrit pour {identite}");
    }
}

// --- Rescan (thread à part) et chargement des icônes (par lots) ---

/// Voir `win32::dash::spawn_rescan`, doc détaillée — port direct (même
/// mécanisme `std::sync::mpsc`/`COINIT_MULTITHREADED`).
fn spawn_rescan() -> Receiver<Vec<AppEntry>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let apps = apps::enumerate_apps();
        apps_cache::save_apps(&apps);
        unsafe {
            windows::Win32::System::Com::CoUninitialize();
        }
        let _ = tx.send(apps);
    });
    rx
}

/// Voir `win32::dash::request_rescan` — même politique (un seul rescan en
/// vol à la fois, `rescan_dirty` en attend un second si nécessaire).
fn request_rescan() {
    DATA.with(|d| {
        let mut data = d.borrow_mut();
        if data.rescan_rx.is_some() {
            data.rescan_dirty = true;
        } else {
            data.rescan_rx = Some(spawn_rescan());
        }
    });
}

/// Voir `win32::dash::poll_rescan` — même logique, retourne `true` si
/// `apps`/`categories` viennent d'être remplacés (l'appelant doit alors
/// repousser l'état, voir `poll`).
fn poll_rescan() -> bool {
    let received = DATA.with(|d| {
        let mut data = d.borrow_mut();
        let Some(rx) = &data.rescan_rx else { return None };
        match rx.try_recv() {
            Ok(apps) => Some(apps),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                data.rescan_rx = None;
                None
            }
        }
    });
    let Some(apps) = received else { return false };
    DATA.with(|d| {
        let mut data = d.borrow_mut();
        data.pending_icon_loads = (0..apps.len()).collect();
        data.apps = apps;
        data.categories = apps::all_categories_info();
        data.rescan_rx = None;
        let dirty = std::mem::take(&mut data.rescan_dirty);
        drop(data);
        if dirty {
            request_rescan();
        }
    });
    true
}

/// Voir `win32::dash::load_pending_icons`, doc détaillée — même limite par
/// frame, même politique de sauvegarde disque une fois la file vidée.
/// Retourne le code JS à exécuter (`window.setIcons([[index, uri], ...])`)
/// s'il y a effectivement quelque chose de nouveau à afficher, `None`
/// sinon (rien à faire cette frame, ou aucune icône n'a pu être résolue).
fn load_pending_icons() -> Option<String> {
    let mut updates: Vec<(usize, String)> = Vec::new();
    DATA.with(|d| {
        let mut data_ref = d.borrow_mut();
        // `&mut *data_ref` UNE SEULE FOIS : `RefMut::deref_mut` est un appel
        // de fonction opaque pour l'emprunteur — accéder à des champs
        // DISJOINTS (`apps`/`icon_cache`/`pending_icon_loads`) directement
        // à travers `data_ref` (répété) échoue (`E0502`, chaque `data_ref.
        // xxx` redéclenchant un `deref_mut` que l'emprunteur ne peut pas
        // prouver disjoint des autres) ; à travers une VRAIE `&mut
        // DashData` obtenue une fois, les projections de champs suivantes
        // sont de simples accès directs, correctement reconnus disjoints.
        let data = &mut *data_ref;
        if data.pending_icon_loads.is_empty() {
            return;
        }
        let mut dirty = false;
        for _ in 0..ICON_BATCH_PER_FRAME {
            let Some(index) = data.pending_icon_loads.pop_front() else { break };
            let Some(app) = data.apps.get(index) else { continue };
            if let Some((uri, is_new)) = icon_data_uri_cached(app, &mut data.icon_cache) {
                updates.push((index, uri));
                dirty |= is_new;
            }
        }
        data.icon_cache_dirty |= dirty;
        if data.pending_icon_loads.is_empty() && data.icon_cache_dirty {
            apps_cache::save_icons(&data.icon_cache);
            data.icon_cache_dirty = false;
        }
    });
    if updates.is_empty() {
        return None;
    }
    let payload: Vec<_> = updates.into_iter().map(|(index, uri)| json!([index, uri])).collect();
    Some(format!("window.setIcons({})", json!(payload)))
}

/// Voir `win32::dash::icon_rgba_cached` — même principe (cache disque par
/// clé stable, extraction Shell "à froid" sur un raté), plus simple ici :
/// pas de décodage RGBA nécessaire sur un succès de cache, `window.
/// setIcons` (voir `assets/popups/dash.html`) prend directement la chaîne
/// `data:` telle quelle.
fn icon_data_uri_cached(app: &AppEntry, cache: &mut HashMap<String, String>) -> Option<(String, bool)> {
    let key = icon_cache_key(app);
    if let Some(cached) = cache.get(&key) {
        return Some((cached.clone(), false));
    }
    let (width, height, rgba) = icon_rgba_for(app)?;
    let uri = encode_rgba_to_data_uri(width, height, &rgba)?;
    cache.insert(key, uri.clone());
    Some((uri, true))
}

// --- Extraction d'icônes (Win32 Shell) — voir win32::dash pour la doc
// complète, dupliquée ici (voir la doc de module sur pourquoi) ---

fn icon_cache_key(app: &AppEntry) -> String {
    if let Some(icon_path) = &app.icon_path {
        return icon_path.to_string_lossy().into_owned();
    }
    match &app.launch {
        AppLaunch::Shortcut { shortcut_path, .. } => shortcut_path.to_string_lossy().into_owned(),
        AppLaunch::Uwp { app_user_model_id } => app_user_model_id.clone(),
        AppLaunch::UriProtocol { uri } => uri.clone(),
    }
}

/// `pub(crate)` : réutilisé par `win32::cef_desktop_preview`, qui a exactement
/// le même besoin (une icône RGBA -> `data:` URI pour une page CEF). **La
/// définition reste ICI** parce que c'est là qu'elle est née et que rien ne
/// justifie de déplacer du code qui marche ; ce qu'il fallait éviter, c'est
/// d'en écrire une seconde.
pub(crate) fn encode_rgba_to_data_uri(width: u32, height: u32, rgba: &[u8]) -> Option<String> {
    let buffer = image::RgbaImage::from_raw(width, height, rgba.to_vec())?;
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(buffer)
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .ok()?;
    Some(format!("data:image/png;base64,{}", BASE64.encode(bytes)))
}

fn icon_rgba_for(app: &AppEntry) -> Option<(u32, u32, Vec<u8>)> {
    if let Some(icon_path) = &app.icon_path {
        return icon_rgba_from_shortcut(icon_path);
    }
    match &app.launch {
        AppLaunch::Shortcut { shortcut_path, .. } => icon_rgba_from_shortcut(shortcut_path),
        AppLaunch::Uwp { .. } => app.icon_rgba.clone(),
        AppLaunch::UriProtocol { .. } => None,
    }
}

fn icon_rgba_from_shortcut(shortcut_path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let (width, height, rgba) = jumbo_icon_rgba(shortcut_path).or_else(|| large_icon_rgba(shortcut_path))?;
    Some(trim_transparent_padding(width, height, &rgba))
}

fn shortcut_path_wide(shortcut_path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    shortcut_path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

pub(crate) fn jumbo_icon_rgba(shortcut_path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let wide = shortcut_path_wide(shortcut_path);
    unsafe {
        let mut info = SHFILEINFOW::default();
        let result = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_SYSICONINDEX,
        );
        if result == 0 {
            return None;
        }
        let image_list: IImageList = SHGetImageList(SHIL_JUMBO as i32).ok()?;
        let hicon = image_list.GetIcon(info.iIcon, ILD_TRANSPARENT.0).ok()?;
        if hicon.is_invalid() {
            return None;
        }
        let extracted = icon_to_rgba(hicon);
        let _ = DestroyIcon(hicon);
        extracted
    }
}

pub(crate) fn large_icon_rgba(shortcut_path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let wide = shortcut_path_wide(shortcut_path);
    unsafe {
        let mut info = SHFILEINFOW::default();
        let result = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if result == 0 || info.hIcon.is_invalid() {
            return None;
        }
        let extracted = icon_to_rgba(info.hIcon);
        let _ = DestroyIcon(info.hIcon);
        extracted
    }
}

/// **L'ICÔNE D'UN TYPE DE FICHIER, SANS LIRE LE FICHIER** — pour le dash de
/// Recherche (voir `win32::cef_recherche`), dont les résultats peuvent être sur
/// un disque lent ou un partage réseau. `SHGFI_USEFILEATTRIBUTES` fait répondre
/// le shell d'après le NOM et les ATTRIBUTS seuls : `nom` est un nom fictif
/// (`x.pdf`), `dossier` choisit l'icône de dossier.
///
/// JUMBO, débarrassée de sa marge transparente (une ancienne icône de 32 px est
/// rendue AU CENTRE d'un carré de 256), puis ramenée à `cote` pixels.
pub(crate) fn type_icon_rgba(nom: &str, dossier: bool, cote: u32) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL};
    use windows::Win32::UI::Shell::SHGFI_USEFILEATTRIBUTES;
    let wide = shortcut_path_wide(Path::new(nom));
    let attributs = if dossier { FILE_ATTRIBUTE_DIRECTORY } else { FILE_ATTRIBUTE_NORMAL };
    let (largeur, hauteur, rgba) = unsafe {
        let mut info = SHFILEINFOW::default();
        let result = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            attributs,
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_SYSICONINDEX | SHGFI_USEFILEATTRIBUTES,
        );
        if result == 0 {
            return None;
        }
        let image_list: IImageList = SHGetImageList(SHIL_JUMBO as i32).ok()?;
        let hicon = image_list.GetIcon(info.iIcon, ILD_TRANSPARENT.0).ok()?;
        if hicon.is_invalid() {
            return None;
        }
        let extracted = icon_to_rgba(hicon);
        let _ = DestroyIcon(hicon);
        extracted?
    };
    Some(reduire_icone(largeur, hauteur, &rgba, cote))
}

/// L'icône PROPRE d'un fichier (un `.exe`, un raccourci…) — jumbo, rognée et
/// ramenée à `cote` pixels comme [`type_icon_rgba`]. ⚠️ LIT le fichier : à
/// réserver aux disques locaux (voir `cef_recherche::cle_d_icone`).
pub(crate) fn file_icon_rgba(chemin: &Path, cote: u32) -> Option<(u32, u32, Vec<u8>)> {
    let (largeur, hauteur, rgba) = jumbo_icon_rgba(chemin).or_else(|| large_icon_rgba(chemin))?;
    Some(reduire_icone(largeur, hauteur, &rgba, cote))
}

fn reduire_icone(largeur: u32, hauteur: u32, rgba: &[u8], cote: u32) -> (u32, u32, Vec<u8>) {
    let (largeur, hauteur, rgba) = trim_transparent_padding(largeur, hauteur, rgba);
    if largeur <= cote && hauteur <= cote {
        return (largeur, hauteur, rgba);
    }
    let Some(image) = image::RgbaImage::from_raw(largeur, hauteur, rgba.clone()) else {
        return (largeur, hauteur, rgba);
    };
    let echelle = cote as f32 / largeur.max(hauteur) as f32;
    let (l, h) = (((largeur as f32 * echelle).round() as u32).max(1), ((hauteur as f32 * echelle).round() as u32).max(1));
    let reduite = image::imageops::resize(&image, l, h, image::imageops::FilterType::Lanczos3);
    (l, h, reduite.into_raw())
}

#[cfg(test)]
mod tests_rafraichir_apres_un_changement {
    /// Chaque geste qui change une catégorie ou une zone redessine le Dash tout de
    /// suite (2026-11-01) ; un lancement non (le Dash se ferme).
    #[test]
    fn les_gestes_de_categorie_et_de_zone_redessinent_le_dash() {
        let source = include_str!("cef_dash.rs");
        let debut = source.find("fn dispatch_actions(").unwrap();
        let corps = &source[debut..debut + source[debut..].find("
}
").unwrap()];
        let bras = |verbe: &str| {
            let i = corps.find(&format!("[\"{verbe}\"")).unwrap_or_else(|| panic!("{verbe} absent"));
            let fin = corps[i..].find("}
").unwrap();
            corps[i..i + fin].trim_end().ends_with("true")
        };
        for verbe in ["category-toggle", "category-new", "category-reset", "add-to-zone", "remove-from-dock", "category-create", "category-delete"] {
            assert!(bras(verbe), "{verbe} ne redessine pas le Dash");
        }
        assert!(!bras("launch"), "un lancement n'a pas a redessiner");
        assert!(corps.contains("if rafraichir {
            rafraichir_tout_de_suite();"));
    }
}

#[cfg(test)]
mod tests_zones_toml_apres_epinglage {
    /// Épingler ou déplacer dans une zone écrit AUSSI la liste de la zone dans
    /// `zones.toml`, AVANT les trois sorties (déplacement, réserve, rechargement) —
    /// sinon l'épingle disparaît de sa zone au redémarrage (2026-11-01).
    #[test]
    fn epingler_ecrit_la_zone_dans_zones_toml_avant_les_trois_sorties() {
        let source = include_str!("cef_dash.rs");
        let debut = source.find("fn handle_add_to_zone(").unwrap();
        let corps = &source[debut..debut + source[debut..].find("
}
").unwrap()];
        let rangement = corps.find("ranger_dans_la_zone_de_zones_toml(&cle, &name);").expect("zones.toml n'est pas mis a jour");
        for sortie in ["if !ajoutee {", "if a_chaud {", "crate::app::request_reload();"] {
            let position = corps.find(sortie).unwrap_or_else(|| panic!("{sortie} introuvable"));
            assert!(rangement < position, "{sortie} part avant l'ecriture de zones.toml");
        }
    }
}

#[cfg(test)]
mod tests_limite_du_decalage {
    use super::*;

    /// La configuration mesurée : base 40, épingle neuve en 3, rien entre 39 et 40.
    #[test]
    fn sans_remplisseur_les_epingles_windows_et_les_zones_suivent() {
        let (mut gauche, mut droite) = (vec![0, 39, 40, 50], vec![51, 2, 39, 53, 54]);
        let (mut haut_gauche, mut haut_droite) = (vec![], vec![4, 6]);
        crate::ui::dock::decaler_bloc_epingles_dans(
            [&mut gauche, &mut droite, &mut haut_gauche, &mut haut_droite],
            3,
            limite_du_decalage(40, false),
            false,
        );
        assert_eq!(gauche, vec![0, 40, 41, 51], "l'icône 39 et les épingles Windows avancent ensemble");
        assert_eq!(droite, vec![52, 2, 40, 54, 55], "les séparateurs de zone aussi");
        assert_eq!(haut_droite, vec![5, 7]);
    }

    #[test]
    fn un_remplisseur_libre_absorbe_l_epingle() {
        assert_eq!(limite_du_decalage(40, true), 40);
    }
}

#[cfg(test)]
mod tests_place_epingle {
    use super::*;
    use crate::app::PINNED_ICON_BASE;

    /// Une épingle de test. `place_epingle` ne regarde que `name`.
    fn epingle(name: &str) -> config::PinnedApp {
        config::PinnedApp { name: name.to_string(), path: None, uri: None, launcher: false, from_extra: false }
    }

    #[test]
    fn une_epingle_nouvelle_se_range_apres_la_derniere() {
        // Arrange : deux épingles déjà là.
        let pins = [epingle("GIMP"), epingle("Character Map")];

        // Act
        let place = place_epingle(&pins, "OBS");

        // Assert : elle se range à la FIN — et c'est ce même `ajoutee` qui
        // annonce que la liste grandit, donc que tout index supérieur se décalera.
        assert!(place.ajoutee);
        assert_eq!(place.icone, PINNED_ICON_BASE + 2);
    }

    #[test]
    fn une_epingle_deja_presente_ne_grandit_pas_la_liste() {
        // Arrange : elle est DÉJÀ là, et PAS en dernier — c'est le cas qui
        // distingue « son rang dans la liste » de « la fin de la liste ».
        let pins = [epingle("GIMP"), epingle("OBS"), epingle("Character Map")];

        // Act
        let place = place_epingle(&pins, "OBS");

        // Assert : aucun décalage, et l'index est celui de son rang.
        assert!(!place.ajoutee);
        assert_eq!(place.icone, PINNED_ICON_BASE + 1);
    }

    #[test]
    fn la_place_ne_depend_ni_de_la_casse_ni_des_espaces() {
        // Arrange : `save_extra_pin` compare ET écrit le nom ÉLAGUÉ (voir
        // `config::save_extra_pin`). Si `place_epingle` ne faisait pas de même il
        // répondrait « absente », annoncerait un décalage — et
        // `decaler_les_zones` décalerait cinq fichiers pour une épingle que
        // l'écriture aurait refusé de créer. C'est exactement la classe de défaut
        // que l'ordre des écritures a coûtée.
        let pins = [epingle("GIMP"), epingle("OBS")];

        // Act
        let place = place_epingle(&pins, "  obs  ");

        // Assert
        assert!(!place.ajoutee, "un nom elague doit reconnaitre l'epingle");
        assert_eq!(place.icone, PINNED_ICON_BASE + 1);
    }

    #[test]
    fn une_liste_vide_range_l_epingle_a_la_base() {
        // Arrange / Act
        let place = place_epingle(&[], "OBS");

        // Assert : la première épingle occupe la première place.
        assert!(place.ajoutee);
        assert_eq!(place.icone, PINNED_ICON_BASE);
    }
}

#[cfg(test)]
mod tests_echelle_ecran {
    use super::*;

    /// ⛔ **LE RATIO DE DPI MULTIPLIE LES DEUX CHEMINS, ET PAS SEULEMENT LE
    /// RÉGLAGE MANUEL.**
    ///
    /// C'est le point qu'on rate facilement : n'appliquer le ratio qu'au réglage
    /// par écran laisserait **tout écran sans réglage** — le cas courant — à la
    /// même taille physique qu'avant, donc 1,5× trop grand sur un écran à 100 %.
    #[test]
    fn le_ratio_de_dpi_multiplie_les_deux_chemins() {
        let globaux = [0.8, 1.0, 1.2];
        // Chemin 1 — AUCUN réglage par écran : les globales, × le ratio.
        assert_eq!(echelles_effectives(None, globaux, 0.5), [0.4, 0.5, 0.6]);
        // Chemin 2 — réglage par écran : LUI AUSSI × le ratio.
        assert_eq!(echelles_effectives(Some([1.0, 1.0, 1.0]), globaux, 0.5), [0.5, 0.5, 0.5]);
        // ⭐ Et sur la PRIMAIRE, ratio 1.0 : rien ne bouge — c'est la propriété
        // qui rend ce lot sûr, et elle vaut aussi pour le réglage manuel.
        assert_eq!(echelles_effectives(None, globaux, 1.0), globaux);
        assert_eq!(echelles_effectives(Some([0.25, 0.5, 2.0]), globaux, 1.0), [0.25, 0.5, 2.0]);
    }
}

#[cfg(test)]
mod tests_administrateur {
    /// La page émet `launch-admin` et le `match` de `dispatch_actions` le reçoit :
    /// sur l'ancien code, ni l'un ni l'autre n'existait (l'entrée s'était perdue au
    /// portage CEF). Un verbe d'un côté sans l'autre serait un bouton MUET.
    #[test]
    fn la_page_et_rust_connaissent_le_lancement_en_administrateur() {
        let page = include_str!("../../assets/popups/dash.html");
        let rust = include_str!("cef_dash.rs");
        assert!(page.contains(r#"go("launch-admin", "#), "la page n'émet pas launch-admin");
        assert!(rust.contains(r#"["launch-admin", index] =>"#), "dispatch_actions ne reçoit pas launch-admin");
        assert!(rust.contains(r#""admin": app.launch.can_run_as_admin()"#), "la page ne saurait pas quand proposer le bouton");
    }
}
