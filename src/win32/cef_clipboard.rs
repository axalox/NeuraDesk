//! Fenêtre CEF pour la visionneuse du presse-papiers — Phase 4 de
//! `docs/plans/cef-web-ui-migration.md`. Remplace `win32::clipboard_viewer`
//! (module egui séparé, PAS un `PopupKind` de `win32::popup_menu` —
//! contrairement aux popups des phases précédentes, cette fenêtre remplit
//! toute la "zone libre" du dock comme le Dash, voir `app::dash_rect`).
//!
//! Deux particularités inédites dans cette migration, toutes deux
//! justifiant les ajouts à `win32::cef_popup` :
//! - **Redimensionnement en cours d'ouverture** (`win32::cef_popup::
//!   resize_if_needed`) : `app::dash_rect` peut changer PENDANT que cette
//!   fenêtre est affichée (curseurs d'épaisseur de barre) — aucun autre
//!   popup CEF n'a encore eu besoin de bouger après sa création.
//! - **Préparation avant `force_foreground`** (`win32::cef_popup::
//!   toggle_with`) : « Sélectionner » doit redonner le premier plan à la
//!   fenêtre qui l'avait AVANT l'ouverture de cette visionneuse — capturée
//!   dans `on_show`, juste avant que `force_foreground` ne le lui vole (voir
//!   `toggle`).
//!
//!   ⚠️ **AUCUNE ENTRÉE N'EST SIMULÉE DEPUIS CETTE FENÊTRE** (DEMANDE
//!   EXPLICITE, 2026-09-23). Ce bouton s'appelait « Coller » et envoyait un
//!   `Ctrl+V` par `SendInput` juste après avoir rendu le premier plan — un
//!   enchaînement qui ne fonctionne QUE si le premier plan a réellement été
//!   repris par la bonne fenêtre. Il ne fait plus que DÉPOSER l'entrée dans
//!   le presse-papiers : c'est l'utilisateur qui colle, avec le raccourci
//!   Windows standard, dans l'application qu'il a lui-même choisie (voir
//!   `handle_copy`).
//!
//! Miniatures et image pleine résolution transmises en `data:image/
//! png;base64,...` (voir `assets/popups/clipboard.html`, doc d'en-tête) —
//! l'aperçu HTML/CSS ne peut pas dessiner du RGBA brut comme
//! `egui::ColorImage` le pouvait.

use std::cell::Cell;
use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::json;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Controls::Dialogs::{
    GetSaveFileNameW, OPENFILENAMEW, OFN_EXPLORER, OFN_HIDEREADONLY, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST,
};
use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, GetForegroundWindow, IsWindow};

use crate::config;
use crate::gfx::device::GfxDevice;
use crate::win32::clipboard::{self, ClipboardEntry};
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// Fenêtre au premier plan JUSTE AVANT l'ouverture (voir `toggle`, doc
    /// de module) — voir `win32::clipboard_viewer::ViewerState::
    /// previous_foreground`, même rôle exact.
    static PREVIOUS_FOREGROUND: Cell<Option<HWND>> = const { Cell::new(None) };
    /// Voir `win32::clipboard_viewer`, doc de son propre `SAVING` : `true`
    /// pendant que la boîte "Enregistrer sous" (bloquante) a le focus —
    /// `hide_if_focus_elsewhere` l'ignore pendant ce temps.
    static SAVING: Cell<bool> = const { Cell::new(false) };
    /// Échelle des miniatures (variable CSS `--thumb-scale` de `assets/
    /// popups/clipboard.html`) — voir `win32::cef_dash::ICON_SCALE`, même
    /// mécanisme exact, dupliqué ici plutôt que partagé (chaque fenêtre
    /// garde son propre réglage). Séparée de `TEXT_SCALE` : même raison que
    /// `win32::cef_dash::ICON_SCALE`/`LABEL_SCALE` (régler chaque élément
    /// indépendamment).
    static THUMB_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// Échelle du texte (aperçu + étiquette d'image) — voir `THUMB_SCALE`,
    /// doc détaillée sur pourquoi ce réglage est séparé.
    static TEXT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// Réglage MANUEL par écran secondaire — voir `win32::cef_dash::
    /// SCREEN_SCALE_OVERRIDE`, doc de champ, même mécanisme exact.
    /// `[miniatures, texte]`.
    /// Le second terme est le **RATIO DE DPI** de l'écran visé — voir
    /// `cef_dash::SCREEN_SCALE_OVERRIDE`, doc de champ, même raison : sans lui le
    /// panneau garde la même taille PHYSIQUE partout, donc paraît plus grand sur
    /// un écran à 100 % que sur la primaire.
    static SCREEN_SCALE_OVERRIDE: Cell<(Option<[f32; 2]>, f32)> = const { Cell::new((None, 1.0)) };
}

/// Facteur d'échelle actuel des miniatures — voir `THUMB_SCALE`.
pub fn thumb_scale() -> f32 {
    THUMB_SCALE.with(Cell::get)
}

/// Change le facteur d'échelle des miniatures, pris en compte à la
/// PROCHAINE `push_state` — voir `win32::cef_dash::set_icon_scale`, même
/// mécanisme/mêmes bornes exacts (dont [0.25, 2.0], DEMANDE EXPLICITE de
/// pouvoir réduire encore plus). Appelé par `win32::cef_settings` (curseur
/// "Miniatures") et par `app::run` au démarrage (voir `config::
/// load_clipboard_thumb_scale`).
pub fn set_thumb_scale(scale: f32) {
    THUMB_SCALE.with(|s| s.set(scale.clamp(0.25, 2.0)));
}

/// Facteur d'échelle actuel du texte — voir `TEXT_SCALE`.
pub fn text_scale() -> f32 {
    TEXT_SCALE.with(Cell::get)
}

/// Change le facteur d'échelle du texte — voir `set_thumb_scale`, mêmes
/// garanties/bornes exactes (curseur "Texte" plutôt que "Miniatures",
/// `config::load_clipboard_text_scale`).
pub fn set_text_scale(scale: f32) {
    TEXT_SCALE.with(|s| s.set(scale.clamp(0.25, 2.0)));
}

/// Fixe/efface le réglage MANUEL d'échelle de l'écran sur lequel cette
/// visionneuse vient d'être ouverte — voir `win32::cef_dash::
/// set_screen_scale_override`, même schéma exact.
pub fn set_screen_scale_override(scale: Option<[f32; 2]>, ratio_dpi: f32) {
    SCREEN_SCALE_OVERRIDE.with(|s| s.set((scale, ratio_dpi)));
}

/// Les deux échelles EFFECTIVES du Presse-papiers : le réglage manuel de l'écran
/// s'il existe, sinon les valeurs globales — **le tout multiplié par le ratio de
/// DPI de l'écran** (voir `SCREEN_SCALE_OVERRIDE`, doc de champ).
///
/// ⛔ **LE RATIO S'APPLIQUE AUX DEUX CHEMINS** — n'appliquer le ratio qu'au
/// réglage par écran laisserait tout écran SANS réglage à l'ancienne taille, donc
/// plus grand que la primaire.
///
/// ⚠️ **SÉPARÉE DU `thread_local` POUR ÊTRE VÉRIFIABLE** : même raison et MÊME
/// corps que `cef_dash::echelles_effectives` — deux copies, deux tests, parce
/// qu'un test ne traverse pas les modules.
fn echelles_effectives(choix: Option<[f32; 2]>, globaux: [f32; 2], ratio_dpi: f32) -> [f32; 2] {
    choix.unwrap_or(globaux).map(|v| v * ratio_dpi)
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefClipboardWindow"),
    window_title: w!("NeuraDesk — Presse-papiers"),
    html_file: "clipboard.html",
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

/// Voir `win32::cef_popup::toggle_with`. `history` : l'historique ACTUEL
/// (voir `app::run`, `clipboard_history`) — poussé dans la page à chaque
/// ouverture (voir `push_state`) ; `refresh_if_visible` le repousse aussi
/// si une nouvelle entrée arrive PENDANT que cette fenêtre reste ouverte.
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, history: &[ClipboardEntry]) {
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, || {
        // Capturée AVANT `force_foreground` (voir la doc de
        // `toggle_with`) — même raison/même garde `is_invalid` que
        // `win32::clipboard_viewer::toggle`.
        let previous = unsafe {
            let fg = GetForegroundWindow();
            (!fg.is_invalid()).then_some(fg)
        };
        PREVIOUS_FOREGROUND.with(|p| p.set(previous));
        // Même "zone libre" que le Dash — masque les fenêtres qui s'y
        // superposeraient.
        crate::win32::cef_dash::hide_if_visible();
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
    push_state(history);
}

/// Repousse l'historique SEULEMENT si la fenêtre est actuellement visible
/// (voir `win32::cef_popup::is_visible`) — à appeler depuis `app::run`
/// juste après qu'une NOUVELLE entrée a été ajoutée à `clipboard_history`
/// (voir `Event::ClipboardChanged`) : sans ça, cette fenêtre resterait
/// figée sur l'état lu à son ouverture pendant qu'un copier ailleurs vient
/// justement de le rendre périmé — la version egui d'origine n'avait pas ce
/// problème (elle relit `clipboard_history` en direct à chaque frame).
pub fn refresh_if_visible(history: &[ClipboardEntry]) {
    if cef_popup::is_visible(&SLOT) {
        push_state(history);
    }
}

fn push_state(history: &[ClipboardEntry]) {
    let entries: Vec<serde_json::Value> = history
        .iter()
        .enumerate()
        .map(|(index, entry)| match entry {
            ClipboardEntry::Text(text) => json!({ "index": index, "kind": "text", "text": text }),
            ClipboardEntry::Image { width, height, .. } => {
                let thumb_data_uri = entry.thumb_png_bytes().map(|bytes| format!("data:image/png;base64,{}", BASE64.encode(bytes)));
                json!({ "index": index, "kind": "image", "width": width, "height": height, "thumbDataUri": thumb_data_uri })
            }
        })
        .collect();
    // Voir `SCREEN_SCALE_OVERRIDE`, doc de champ.
    let (choix, ratio_dpi) = SCREEN_SCALE_OVERRIDE.with(Cell::get);
    let [thumb, text] = echelles_effectives(choix, [thumb_scale(), text_scale()], ratio_dpi);
    let payload = json!({
        "entries": entries,
        "thumbScalePercent": (thumb * 100.0).round() as i32,
        "textScalePercent": (text * 100.0).round() as i32,
        // Toute la page suit la taille des textes (Réglages › Taille des textes).
        "pageScale": crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES),
    });
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
}

/// Vide et applique les actions demandées par `assets/popups/clipboard.html`
/// (format `"clipboard/<verbe>[/<index>]"`, voir sa fonction `go()`) — à
/// appeler à chaque frame (voir `app::run`, même endroit que les autres
/// popups). `clipboard_history` : mutable, `delete` en retire une entrée
/// (voir `handle_delete`).
pub fn dispatch_actions(clipboard_history: &mut Vec<ClipboardEntry>) {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(rest) = action.strip_prefix("clipboard/") else {
            eprintln!("[neuradesk][cef] action presse-papiers inconnue : {action:?}");
            continue;
        };
        let parts: Vec<&str> = rest.split('/').collect();
        match parts.as_slice() {
            ["close"] => cef_popup::fermer_par_geste(&SLOT),
            ["select", index] => handle_select(clipboard_history, index),
            ["copy", index] => handle_copy(clipboard_history, index),
            ["save", index] => handle_save(clipboard_history, index),
            ["delete", index] => handle_delete(clipboard_history, index),
            // ⭐ UN QUART DE TOUR d'une image (demande 2026-11-08) : `sens` =
            // `droite` ou `gauche`.
            ["pivoter", index, sens] => handle_pivoter(clipboard_history, index, sens),
            // ⭐ LE QR CODE d'une entrée texte (voir `win32::qr`) — rendu à la
            // page, qui le garde pour cette entrée.
            ["qr", index] => handle_qr(clipboard_history, index),
            // ⭐ LA RECONNAISSANCE DE TEXTE d'une image, MOTEUR CHOISI
            // (demande 2026-11-04, voir `win32::texte_capture`).
            ["texte", index, moteur] => handle_texte(clipboard_history, index, moteur),
            // ⭐ LE MOT DE PASSE GÉNÉRÉ par la page — copié HORS de tout
            // historique (voir `clipboard::write_secret`), y compris le nôtre.
            ["mot-de-passe", texte] => {
                let texte = crate::win32::cef_app::percent_decode(texte);
                let ok = !texte.is_empty() && crate::win32::clipboard::write_secret(&texte);
                cef_popup::execute_js(&SLOT, &format!("window.motDePasseCopie({ok})"));
            }
            _ => eprintln!("[neuradesk][cef] action presse-papiers inconnue : {action:?}"),
        }
    }
}

/// Le QR code du texte de l'entrée `index`, ou l'erreur, renvoyé à la page.
fn handle_qr(history: &[ClipboardEntry], index: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let Some(ClipboardEntry::Text(texte)) = history.get(index) else { return };
    let reponse = match crate::win32::qr::svg(texte) {
        Ok(svg) => json!({ "svg": svg }),
        Err(erreur) => json!({ "erreur": erreur }),
    };
    cef_popup::execute_js(&SLOT, &format!("window.setQr({index}, {reponse})"));
}

/// **RECONNAÎTRE LE TEXTE D'UNE IMAGE DU PRESSE-PAPIERS** (demande 2026-11-04).
///
/// Le moteur vient de la page (`"tesseract"` = NAPS2, `"windows"` = Copilot) :
/// il est **choisi**, donc un échec le dit au bandeau — jamais de bascule
/// silencieuse sur l'autre (voir `win32::texte_capture`).
///
/// Le PNG part par [`ClipboardEntry::to_png_bytes`], qui sait relire une entrée
/// persistée comme une entrée fraîche (RGBA encore en mémoire).
fn handle_texte(history: &[ClipboardEntry], index: &str, moteur: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let Some(entry) = history.get(index) else { return };
    let Some(png) = entry.to_png_bytes() else {
        crate::win32::bandeau::montrer(
            crate::win32::texte_capture::TITRE_PANNEAU,
            "Cette entrée du presse-papiers n'a pas d'image à reconnaître.",
        );
        return;
    };
    crate::win32::texte_capture::lancer_choisi(moteur, &png);
}

/// Charge PARESSEUSEMENT l'image PLEINE RÉSOLUTION de l'entrée `index` (voir
/// `assets/popups/clipboard.html`, doc d'en-tête) — voir `win32::
/// clipboard_viewer::ensure_full_texture`, même principe, porté en `data:`
/// URI plutôt qu'en texture `egui`. No-op pour une entrée texte (déjà
/// intégralement connue de la page, voir `push_state`) ou hors bornes.
fn handle_select(history: &[ClipboardEntry], index: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let Some(entry) = history.get(index) else { return };
    if let Some(bytes) = entry.to_png_bytes() {
        let data_uri = format!("data:image/png;base64,{}", BASE64.encode(bytes));
        cef_popup::execute_js(&SLOT, &format!("window.setFullImage({index}, {})", json!(data_uri)));
    }
}

/// **LE BOUTON « SÉLECTIONNER »** — dépose l'entrée `index` dans le
/// presse-papiers WINDOWS, puis rend la main : la fenêtre se masque et le
/// premier plan repart à l'application d'où l'on venait (voir
/// `PREVIOUS_FOREGROUND`), de sorte que le `Ctrl+V` suit immédiatement, sans
/// avoir à recliquer nulle part.
///
/// **CE QU'ELLE NE FAIT PAS, ET C'EST LE POINT** : elle n'envoie AUCUNE touche
/// (DEMANDE EXPLICITE, 2026-09-23 — « pour être collé dans une autre
/// application avec le raccourci Windows standard »). Son prédécesseur,
/// `handle_paste`, copiait PUIS simulait `Ctrl+V` par `SendInput` : cela ne
/// fonctionne que si `force_foreground` a effectivement repris le premier plan,
/// et le collage part alors dans la fenêtre élue par la restauration, pas dans
/// celle que l'utilisateur regarde. Ici le collage appartient à l'utilisateur.
///
/// **L'ORDRE COMPTE** : le presse-papiers est écrit AVANT que la fenêtre ne se
/// masque, et la fenêtre se masque AVANT que le premier plan ne soit rendu —
/// masquer en dernier laisserait la visionneuse par-dessus l'application
/// destinataire.
///
/// `IsWindow` : la fenêtre précédente peut très bien avoir été FERMÉE pendant
/// que la visionneuse était ouverte (c'est courant : on ouvre la visionneuse
/// depuis une fenêtre qu'on ferme ensuite). Sans ce test,
/// `force_foreground` viserait un `HWND` mort — même garde que
/// `handle_paste` avant elle.
fn handle_copy(history: &[ClipboardEntry], index: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let Some(entry) = history.get(index) else { return };
    clipboard::write(entry);
    cef_popup::hide_if_visible(&SLOT);
    let Some(previous) = PREVIOUS_FOREGROUND.with(Cell::get) else { return };
    if !unsafe { IsWindow(Some(previous)) }.as_bool() {
        return;
    }
    crate::win32::cef_popup::force_foreground(previous);
}

fn handle_save(history: &[ClipboardEntry], index: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let Some(entry) = history.get(index) else { return };
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return };
    save_entry(hwnd, entry);
}

fn handle_delete(history: &mut Vec<ClipboardEntry>, index: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    if index < history.len() {
        history.remove(index);
        config::save_clipboard_history(history);
        push_state(history);
    }
}

/// **TOURNER UNE IMAGE DE L'HISTORIQUE D'UN QUART DE TOUR** — l'entrée est
/// REMPLACÉE à sa place (voir [`ClipboardEntry::pivotee`]), persistée, puis la
/// liste et la pleine image repartent à la page. Le presse-papiers WINDOWS
/// n'est pas touché : « Sélectionner » y dépose ensuite l'image tournée.
fn handle_pivoter(history: &mut [ClipboardEntry], index: &str, sens: &str) {
    let Ok(index) = index.parse::<usize>() else { return };
    let horaire = match sens {
        "droite" => true,
        "gauche" => false,
        autre => {
            eprintln!("[neuradesk][cef] presse-papiers : sens de rotation inconnu {autre:?}");
            return;
        }
    };
    let Some(tournee) = history.get(index).and_then(|entree| entree.pivotee(horaire)) else {
        crate::win32::bandeau::montrer("Presse-papiers", "Cette image n'a pas pu être tournée (fichier illisible ?).");
        return;
    };
    history[index] = tournee;
    config::save_clipboard_history(history);
    push_state(history);
    handle_select(history, &index.to_string());
}

/// Voir `win32::clipboard_viewer::save_entry`, port direct (boîte de
/// dialogue Win32 native, sans rapport avec CEF).
fn save_entry(hwnd: HWND, entry: &ClipboardEntry) {
    let (default_name, filter, bytes): (String, &str, Vec<u8>) = match entry {
        ClipboardEntry::Text(text) => (default_save_name("txt"), "Fichier texte (*.txt)\0*.txt\0\0", text.as_bytes().to_vec()),
        ClipboardEntry::Image { .. } => {
            let Some(bytes) = entry.to_png_bytes() else { return };
            (default_save_name("png"), "Image PNG (*.png)\0*.png\0\0", bytes)
        }
    };

    SAVING.with(|flag| flag.set(true));
    let chosen = pick_save_path(hwnd, &default_name, filter);
    SAVING.with(|flag| flag.set(false));

    if let Some(path) = chosen {
        if let Err(err) = std::fs::write(&path, &bytes) {
            eprintln!("[neuradesk][cef] visionneuse du presse-papiers : échec de l'enregistrement ({err})");
        }
    }
}

fn default_save_name(extension: &str) -> String {
    let time = unsafe { GetLocalTime() };
    format!(
        "Presse-papiers {:04}-{:02}-{:02} {:02}-{:02}-{:02}.{extension}",
        time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond,
    )
}

fn pick_save_path(hwnd: HWND, default_name: &str, filter: &str) -> Option<PathBuf> {
    unsafe {
        let mut buffer = [0u16; 32768];
        let default_wide: Vec<u16> = default_name.encode_utf16().collect();
        buffer[..default_wide.len()].copy_from_slice(&default_wide);

        let filter_wide: Vec<u16> = filter.encode_utf16().collect();
        let ext_wide: Vec<u16> =
            default_name.rsplit('.').next().unwrap_or_default().encode_utf16().chain(std::iter::once(0)).collect();

        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: hwnd,
            lpstrFilter: PCWSTR(filter_wide.as_ptr()),
            lpstrFile: PWSTR(buffer.as_mut_ptr()),
            nMaxFile: buffer.len() as u32,
            lpstrDefExt: PCWSTR(ext_wide.as_ptr()),
            Flags: OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_EXPLORER | OFN_HIDEREADONLY,
            ..Default::default()
        };

        if GetSaveFileNameW(&mut ofn).as_bool() {
            let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
            Some(PathBuf::from(String::from_utf16_lossy(&buffer[..end])))
        } else {
            None
        }
    }
}

/// Voir `win32::clipboard_viewer::hide_if_focus_elsewhere` — ignore le
/// sondage pendant que la boîte "Enregistrer sous" a le focus (`SAVING`).
pub fn hide_if_focus_elsewhere() {
    if SAVING.with(Cell::get) {
        return;
    }
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

/// Repositionne/redimensionne PUIS dessine — voir `win32::cef_popup::
/// resize_if_needed`, doc de fonction : cette fenêtre, contrairement aux
/// autres popups CEF, doit suivre `app::dash_rect` qui peut changer pendant
/// qu'elle est ouverte.
pub fn render_if_visible(rect: [i32; 4], tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::resize_if_needed(&SLOT, rect);
    // Voir `win32::cef_dash::render_if_visible` — même raison : pas un
    // popup "menu".
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(result) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests_texte {
    use super::*;

    /// **LE VERBE DE LA RECONNAISSANCE DOIT EXISTER DES DEUX CÔTÉS**, et porter
    /// le nom TECHNIQUE du moteur — le libellé (« NAPS2 », « Copilot ») est fait
    /// pour l'œil, pas pour le dispatch. Un renommage d'un seul côté tomberait
    /// dans « action inconnue » : silencieux pour l'utilisateur.
    #[test]
    fn la_page_et_le_dispatch_connaissent_le_verbe_texte() {
        let page = include_str!("../../assets/popups/clipboard.html");
        // ⛔ Découpé AVANT le module de test : `include_str!` du fichier entier
        // ferait correspondre les motifs ci-dessous au test LUI-MÊME (voir la
        // mémoire du dépôt, `include_str!` et l'auto-correspondance).
        let source = include_str!("cef_clipboard.rs").split("#[cfg(test)]").next().unwrap_or_default();
        assert!(page.contains("go(\"texte\""), "la page n'émet pas le verbe texte");
        assert!(source.contains("[\"texte\", index, moteur]"), "le dispatch ne traite pas texte");
        for (moteur, nom, _libelle) in crate::win32::ocr::Moteur::TOUS {
            assert!(page.contains(&format!("go(\"texte\", entry.index, \"{nom}\")")), "la page n'offre pas {moteur:?} ({nom})");
        }
    }

    /// Les deux sens de rotation existent des deux côtés (même raison).
    #[test]
    fn la_page_et_le_dispatch_connaissent_le_verbe_pivoter() {
        let page = include_str!("../../assets/popups/clipboard.html");
        let source = include_str!("cef_clipboard.rs").split("#[cfg(test)]").next().unwrap_or_default();
        assert!(source.contains("[\"pivoter\", index, sens]"), "le dispatch ne traite pas pivoter");
        for sens in ["gauche", "droite"] {
            assert!(page.contains(&format!("go(\"pivoter\", entry.index, \"{sens}\")")), "la page n'offre pas {sens}");
            assert!(source.contains(&format!("\"{sens}\" =>")), "le dispatch ne connaît pas {sens}");
        }
    }
}

#[cfg(test)]
mod tests_echelle_ecran {
    use super::*;

    /// ⛔ **LE RATIO DE DPI MULTIPLIE LES DEUX CHEMINS, ET PAS SEULEMENT LE
    /// RÉGLAGE MANUEL** — même propriété que `cef_dash`, et testée SÉPARÉMENT
    /// parce qu'un test ne traverse pas les modules : une régression ici ne
    /// ferait tomber que ce test-ci.
    #[test]
    fn le_ratio_de_dpi_multiplie_les_deux_chemins() {
        let globaux = [0.8, 1.2];
        // Chemin 1 — AUCUN réglage par écran.
        assert_eq!(echelles_effectives(None, globaux, 0.5), [0.4, 0.6]);
        // Chemin 2 — réglage par écran : LUI AUSSI × le ratio.
        assert_eq!(echelles_effectives(Some([1.0, 1.0]), globaux, 0.5), [0.5, 0.5]);
        // ⭐ Sur la PRIMAIRE (ratio 1.0), rien ne bouge.
        assert_eq!(echelles_effectives(None, globaux, 1.0), globaux);
        assert_eq!(echelles_effectives(Some([0.25, 2.0]), globaux, 1.0), [0.25, 2.0]);
    }
}
