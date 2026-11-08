//! **L'ÉDITEUR D'IMAGE** — lot 1 de `docs/plans/editeur-image.md`, ouvert par
//! l'applet de la zone Intégrés. **UN PANNEAU** (demande du 2026-09-25 : « affiche
//! l'éditeur d'image dans un dash plutôt ») : il remplit la zone libre du dock
//! comme le Dash, les Notes et le calendrier (voir `app::dash_rect`) — il a été
//! une vraie fenêtre redimensionnable jusque-là (`PopupSpec::fenetre`).
//!
//! Ce lot ne sait qu'ouvrir (fichier, presse-papiers), montrer (zoom,
//! défilement) et rendre (copier, enregistrer en PNG/JPG). Tout le travail sur
//! les pixels vit dans la page (`assets/popups/editeur_image.html`) ; Rust ne
//! fait que ce qu'une page ne peut pas faire : les dialogues de fichier, le
//! disque et le presse-papiers.
//!
//! **Les deux sens n'empruntent pas le même canal** :
//! - Rust → page : une `data:` URI par `execute_js`, comme la pleine résolution
//!   du presse-papiers (`cef_clipboard::handle_select`) ;
//! - page → Rust : un `fetch(POST)` sur `win32::cef_data` (route
//!   [`ROUTE_IMAGE`]), PUIS une navigation `neuradesk://editeur/…` qui dit quoi
//!   en faire. La page n'émet la navigation qu'une fois la réponse du `fetch`
//!   reçue, donc le corps est déjà dans la boîte quand l'action arrive.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::json;
use std::os::windows::ffi::OsStrExt;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{SHOpenWithDialog, OAIF_EXEC, OAIF_HIDE_REGISTRATION, OPENASINFO};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_INSERT, VK_MENU, VK_SHIFT, VK_V};
use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, WM_CHAR, WM_KEYDOWN, WM_KEYUP};

use crate::gfx::device::GfxDevice;
use crate::win32::cef_data;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::clipboard;
use crate::win32::decodeur_document;
use crate::win32::document_image;
use crate::win32::envoi_image;
use crate::win32::epingle;
use crate::win32::file_dialog;

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// `true` dès que la page a annoncé `editeur/pret` — avant, un `execute_js`
    /// part dans le vide sans le dire.
    static PRETE: Cell<bool> = const { Cell::new(false) };
    /// Le script à exécuter dès que la page sera prête (une image demandée
    /// pendant le chargement de la toute première ouverture).
    static EN_ATTENTE: RefCell<Option<String>> = const { RefCell::new(None) };
    /// **LEVIER DE VÉRIFICATION** — voir `debug_enregistrer` : le chemin où
    /// écrire le prochain enregistrement SANS dialogue, et l'original à comparer.
    static ENREGISTREMENT_DEBUG: RefCell<Option<(PathBuf, PathBuf)>> = const { RefCell::new(None) };
    static LEVIERS_APPLIQUES: Cell<u8> = const { Cell::new(0) };
    /// Un « coller » demandé au clavier par `wndproc`, traité à la frame suivante
    /// par `dispatch_actions` — hors du `wndproc`, où `SLOT` peut être emprunté.
    static COLLER_DEMANDE: Cell<bool> = const { Cell::new(false) };
    /// L'envoi en ligne en cours (voir `handle_envoyer_en_ligne`) — UN à la fois.
    static ENVOI: RefCell<Option<(String, Receiver<Result<String, String>>)>> = const { RefCell::new(None) };
    /// Le fichier à calques en cours de décodage (voir `win32::decodeur_document`),
    /// son nom, et s'il doit s'AJOUTER en calques plutôt que remplacer l'image.
    static DECODAGE: RefCell<Option<(decodeur_document::EnCours, String, bool)>> = const { RefCell::new(None) };
}

/// Un lien copié reste affiché plus longtemps qu'une confirmation : il se lit.
const DUREE_MESSAGE_LIEN_MS: u32 = 8000;

/// La route du canal où la page dépose l'image encodée (voir `cef_data`).
const ROUTE_IMAGE: &str = "editeur/image";

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskEditeurImage"),
    window_title: w!("NeuraDesk — Éditeur d'image"),
    html_file: "editeur_image.html",
    wndproc: Some(wndproc),
    // PANNEAU — même famille que le calendrier et les Notes.
    above_a_magnified_dock: false,
    fades: false,
    fenetre: false,
};

/// Les extensions que l'éditeur sait ouvrir — celles que Chromium décode lui-même,
/// et les fichiers à calques de Photoshop et GIMP (`win32::decodeur_document`).
const EXTENSIONS_OUVERTES: &str = "png;jpg;jpeg;bmp;gif;webp;psd;xcf";

/// Le type MIME d'un fichier ouvert, d'après son extension — `None` si
/// l'éditeur ne sait pas l'ouvrir.
fn mime_pour(chemin: &Path) -> Option<&'static str> {
    let extension = chemin.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

/// Ouvre (ou referme) le panneau — voir `win32::cef_popup::toggle_with`.
///
/// ⚠️ **PAS DE PRÉCHARGEMENT** : `PRETE` n'est remis à faux qu'à la CRÉATION de
/// la fenêtre, ici — une page préchargée annoncerait `pret` à un module qui ne
/// l'attend pas encore, sans dommage, mais rien ne le demande.
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice) {
    // ⛔ **SON ZOOM DOIT SUIVRE LE DPI** — l'éditeur n'a aucun autre mécanisme
    // d'échelle, voir `cef_popup::regler_zoom_dpi` (qui liste les trois inscrits).
    // `ouvrir_capture` passe par ici, donc il en hérite.
    cef_popup::regler_zoom_dpi(&SLOT, true);
    // ⭐ La taille des textes GLOBALE (`taille_panneaux::TEXTES`) et son curseur
    // « Contenu », par le même zoom CEF : sa page n'a aucun `--text-scale`.
    let facteur = crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES)
        * crate::win32::taille_panneaux::facteur("editeur_image");
    cef_popup::regler_zoom_textes(&SLOT, facteur, rect);
    if cef_popup::hwnd(&SLOT).is_none() {
        PRETE.with(|p| p.set(false));
    }
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, || {
        // Même « zone libre » que le Dash : même liste que le calendrier.
        crate::win32::cef_dash::hide_if_visible();
        crate::win32::cef_settings::hide_if_visible();
        crate::win32::cef_notes::hide_if_visible();
        crate::win32::cef_clipboard::hide_if_visible();
        crate::win32::cef_calendar_applet::hide_if_visible();
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
    // L'hébergeur se choisit dans les Réglages, qui ne s'ouvrent jamais EN MÊME
    // TEMPS que ce panneau : le repousser à chaque ouverture suffit. Pas par
    // `executer_quand_prete`, dont l'unique place en attente porte peut-être une
    // image — une page pas encore prête le recevra avec `pret`.
    if PRETE.with(Cell::get) {
        cef_popup::execute_js(&SLOT, &script_service());
    }
}

/// Ouvre le panneau — s'il ne l'est pas déjà — sur une capture d'écran (PNG),
/// qui REMPLACE l'image ouverte. Voir `win32::capture_ecran`.
pub fn ouvrir_capture(rect: [i32; 4], gfx: &GfxDevice, png: &[u8]) {
    if !cef_popup::is_visible(&SLOT) {
        toggle(rect, gfx);
    }
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    let nom = format!(
        "Capture {}-{:02}-{:02} {:02}-{:02}-{:02}.png",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    );
    charger_octets(png, "image/png", &nom, false);
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

/// Repositionne/redimensionne PUIS dessine — le rectangle suit `app::dash_rect`,
/// comme celui du calendrier (voir `cef_calendar_applet::render_if_visible`).
pub fn render_if_visible(rect: [i32; 4], tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::resize_if_needed(&SLOT, rect);
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

/// Exécute `js` dans la page, ou le garde pour l'annonce `editeur/pret`.
fn executer_quand_prete(js: String) {
    if PRETE.with(Cell::get) {
        cef_popup::execute_js(&SLOT, &js);
    } else {
        EN_ATTENTE.with(|e| *e.borrow_mut() = Some(js));
    }
}

fn message(texte: &str) {
    executer_quand_prete(format!("window.editeurMessage && window.editeurMessage({})", json!(texte)));
}

/// Un message affiché `duree_ms` (0 = jusqu'au suivant) — voir `editeurMessage`.
fn message_pendant(texte: &str, duree_ms: u32) {
    executer_quand_prete(format!("window.editeurMessage && window.editeurMessage({}, {duree_ms})", json!(texte)));
}

/// Le script qui dit à la page vers quel hébergeur partira « Envoyer en ligne ».
fn script_service() -> String {
    let reglages = envoi_image::charger_reglages();
    let service = json!({ "label": reglages.nom_affiche(), "description": reglages.hebergeur.description() });
    format!("window.editeurService && window.editeurService({service})")
}

/// Pousse une image encodée (`mime`) dans la page — en NOUVEAU CALQUE si
/// `comme_calque` et qu'une image est déjà ouverte, sinon à la place de l'image.
fn charger_octets(octets: &[u8], mime: &str, nom: &str, comme_calque: bool) {
    let uri = format!("data:{mime};base64,{}", BASE64.encode(octets));
    executer_quand_prete(format!("window.editeurCharger({}, {}, {comme_calque})", json!(uri), json!(nom)));
}

fn charger_fichier(chemin: &Path, comme_calques: bool) {
    let nom = chemin.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if decodeur_document::est_un_document(chemin) {
        if DECODAGE.with(|d| d.borrow().is_some()) {
            message("Une ouverture est déjà en cours");
            return;
        }
        match decodeur_document::lancer(chemin) {
            Ok(en_cours) => {
                crate::diag!("[editeur] décodage de « {} » lancé", chemin.display());
                message_pendant(&format!("Ouverture de {nom}…"), 0);
                DECODAGE.with(|d| *d.borrow_mut() = Some((en_cours, nom, comme_calques)));
            }
            Err(erreur) => {
                crate::diag!("[editeur] décodage de « {} » impossible : {erreur}", chemin.display());
                message("Impossible d'ouvrir ce fichier");
            }
        }
        return;
    }
    let Some(mime) = mime_pour(chemin) else {
        message("Format d'image non pris en charge");
        return;
    };
    match std::fs::read(chemin) {
        Ok(octets) => {
            crate::diag!("[editeur] ouverture de « {} » ({} octets)", chemin.display(), octets.len());
            charger_octets(&octets, mime, &nom, comme_calques);
        }
        Err(err) => {
            crate::diag!("[editeur] « {} » ILLISIBLE ({err})", chemin.display());
            message("Impossible de lire ce fichier");
        }
    }
}

/// **OUVRIR** remplace l'image ; **OUVRIR EN TANT QUE CALQUES** (le geste de
/// GIMP) ajoute les calques du fichier au-dessus du calque actif.
fn handle_ouvrir(comme_calques: bool) {
    let Some(proprietaire) = cef_popup::hwnd(&SLOT) else { return };
    let titre = if comme_calques { "Ouvrir en tant que calques" } else { "Ouvrir une image" };
    // Annulé : pas une erreur, rien au journal (voir `file_dialog::ouvrir`).
    if let Some(chemin) = file_dialog::ouvrir(proprietaire, titre, EXTENSIONS_OUVERTES, "Images") {
        charger_fichier(&chemin, comme_calques);
    }
}

/// Relève le décodage en cours, s'il a fini : le document part dans la page.
fn sonder_decodage() {
    let fini = DECODAGE.with(|d| {
        let mut d = d.borrow_mut();
        let resultat = d.as_mut()?.0.relever()?;
        let (_, nom, comme_calques) = d.take()?;
        Some((resultat, nom, comme_calques))
    });
    match fini {
        Some((Ok(document), nom, comme_calques)) => {
            crate::diag!("[editeur] « {nom} » décodé : {} calque(s)", document["calques"].as_array().map_or(0, Vec::len));
            message("");
            executer_quand_prete(format!("window.editeurChargerDocument({document}, {}, {comme_calques})", json!(nom)));
        }
        Some((Err(erreur), nom, _)) => {
            crate::diag!("[editeur] « {nom} » ILLISIBLE : {erreur}");
            message_pendant(&format!("Impossible d'ouvrir {nom} : {erreur}"), DUREE_MESSAGE_LIEN_MS);
        }
        None => {}
    }
}

/// **COLLER** : l'image devient un NOUVEAU CALQUE (le « Coller comme nouveau
/// calque » de GIMP), ou l'image elle-même si l'éditeur est vide.
fn handle_coller() {
    let Some((largeur, hauteur, rgba)) = clipboard::read_image() else {
        message("Le presse-papiers ne contient pas d'image");
        return;
    };
    let Some(png) = clipboard::encode_png(largeur, hauteur, &rgba) else {
        crate::diag!("[editeur] image du presse-papiers {largeur}×{hauteur} : encodage PNG ÉCHOUÉ");
        return;
    };
    charger_octets(&png, "image/png", "Presse-papiers", true);
}

/// **COPIER** : la page a déposé un PNG sur [`ROUTE_IMAGE`] ; il repart en
/// DIBV5 + PNG (`clipboard::write_image`) — le format que Flameshot v14 a dû
/// imposer sous Windows (voir le plan).
fn handle_copier() {
    let Some(png) = cef_data::prendre(ROUTE_IMAGE) else {
        crate::diag!("[editeur] copier : AUCUNE image reçue du canal");
        return;
    };
    let Some((largeur, hauteur, rgba)) = clipboard::decode_png(&png) else {
        crate::diag!("[editeur] copier : PNG de la page ILLISIBLE ({} octets)", png.len());
        return;
    };
    clipboard::write_image(largeur, hauteur, &rgba);
    crate::diag!("[editeur] copié : {largeur}×{hauteur}");
    message("Image copiée");
}

/// **RECONNAÎTRE LE TEXTE DE L'IMAGE AFFICHÉE** (demande 2026-11-04) — la page a
/// déposé son PNG composé sur [`ROUTE_IMAGE`], comme pour « copier » ; le moteur
/// vient d'elle (`"tesseract"` = NAPS2, `"windows"` = Copilot) et **n'est pas
/// remplacé** en cas d'échec (voir `win32::texte_capture`).
fn handle_texte(moteur: &str) {
    let Some(png) = cef_data::prendre(ROUTE_IMAGE) else {
        crate::diag!("[editeur] texte : AUCUNE image reçue du canal");
        return;
    };
    crate::win32::texte_capture::lancer_choisi(moteur, &png);
}

/// **ENVOYER EN LIGNE** (l'outil « imgupload » de Flameshot) — la page a déjà
/// fait confirmer le geste. Le réseau vit sur un fil à part (voir
/// `envoi_image::lancer`) ; `sonder_envoi` en relève le résultat à chaque frame.
fn handle_envoyer_en_ligne() {
    let Some(png) = cef_data::prendre(ROUTE_IMAGE) else {
        crate::diag!("[editeur] envoi en ligne : AUCUNE image reçue du canal");
        return;
    };
    if ENVOI.with(|e| e.borrow().is_some()) {
        message("Un envoi est déjà en cours");
        return;
    }
    match envoi_image::lancer(png) {
        Ok((nom, reception)) => {
            crate::diag!("[editeur] envoi en ligne vers {nom} lancé");
            message_pendant(&format!("Envoi vers {nom}…"), 0);
            ENVOI.with(|e| *e.borrow_mut() = Some((nom, reception)));
        }
        Err(erreur) => {
            crate::diag!("[editeur] envoi en ligne impossible : {erreur}");
            message_pendant(&erreur, DUREE_MESSAGE_LIEN_MS);
        }
    }
}

/// Relève le résultat de l'envoi en cours, s'il est arrivé : le lien part dans le
/// presse-papiers (comme Flameshot), l'erreur s'affiche.
///
/// ⭐ **ET UNE NOTIFICATION WINDOWS, DANS LES DEUX CAS** — DEMANDE EXPLICITE
/// (2026-10-25) : « rien ne me donne le lien de l'image en ligne ». Le message de
/// l'éditeur part avec lui quand on le ferme pendant l'envoi ; le toast reste
/// dans le centre de notifications (voir `win32::bulle`).
fn sonder_envoi() {
    let arrive = ENVOI.with(|e| {
        let mut envoi = e.borrow_mut();
        let (hebergeur, reception) = envoi.as_ref()?;
        let resultat = match reception.try_recv() {
            Ok(resultat) => resultat,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("l'envoi s'est interrompu".to_string()),
        };
        let hebergeur = hebergeur.clone();
        *envoi = None;
        Some((hebergeur, resultat))
    });
    match arrive {
        Some((hebergeur, Ok(lien))) => {
            clipboard::write_text(&lien);
            let pris = crate::win32::bulle::notifier(&format!("Image en ligne sur {hebergeur}"), &format!("{lien}\nLien copié dans le presse-papiers."));
            crate::diag!("[editeur] envoyé sur {hebergeur} : {lien} (toast pris : {pris})");
            message_pendant(&format!("Lien copié : {lien}"), DUREE_MESSAGE_LIEN_MS);
        }
        Some((hebergeur, Err(erreur))) => {
            let pris = crate::win32::bulle::notifier(&format!("Échec de l'envoi sur {hebergeur}"), &erreur);
            crate::diag!("[editeur] envoi sur {hebergeur} ÉCHOUÉ : {erreur} (toast pris : {pris})");
            message_pendant(&format!("Échec de l'envoi : {erreur}"), DUREE_MESSAGE_LIEN_MS);
        }
        None => {}
    }
}

/// **ÉPINGLER** (l'outil « pin » de Flameshot) : l'image composée devient une
/// fenêtre flottante toujours au premier plan (voir `win32::epingle`).
fn handle_epingler() {
    let Some(png) = cef_data::prendre(ROUTE_IMAGE) else {
        crate::diag!("[editeur] épingler : AUCUNE image reçue du canal");
        return;
    };
    match clipboard::decode_png(&png) {
        Some((largeur, hauteur, rgba)) => epingle::epingler(largeur, hauteur, &rgba),
        None => crate::diag!("[editeur] épingler : PNG de la page ILLISIBLE ({} octets)", png.len()),
    }
}

/// Le dossier des images passées à « Ouvrir avec » : l'application choisie les
/// lit APRÈS le retour du dialogue, on ne peut donc pas les effacer tout de
/// suite — elles le sont au passage suivant, passé [`DUREE_FICHIERS_TEMPORAIRES`].
fn dossier_temporaire() -> PathBuf {
    std::env::temp_dir().join("NeuraDesk").join("editeur")
}

const DUREE_FICHIERS_TEMPORAIRES: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

/// Efface les images de [`dossier_temporaire`] plus vieilles que
/// [`DUREE_FICHIERS_TEMPORAIRES`] — seulement les `.png` de CE dossier.
fn purger_temporaires(dossier: &Path) {
    let Ok(entrees) = std::fs::read_dir(dossier) else { return };
    for entree in entrees.flatten() {
        let chemin = entree.path();
        let vieux = entree
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > DUREE_FICHIERS_TEMPORAIRES);
        if vieux && chemin.extension().is_some_and(|e| e == "png") {
            let _ = std::fs::remove_file(&chemin);
        }
    }
}

/// **OUVRIR AVEC** (l'outil « launcher » de Flameshot) : l'image composée est
/// écrite dans un fichier temporaire, puis le dialogue « Ouvrir avec » de
/// Windows la confie à l'application choisie.
fn handle_ouvrir_avec() {
    let Some(png) = cef_data::prendre(ROUTE_IMAGE) else {
        crate::diag!("[editeur] ouvrir avec : AUCUNE image reçue du canal");
        return;
    };
    let dossier = dossier_temporaire();
    purger_temporaires(&dossier);
    if let Err(err) = std::fs::create_dir_all(&dossier) {
        crate::diag!("[editeur] « {} » : création impossible ({err})", dossier.display());
        message("L'image n'a pas pu être préparée");
        return;
    }
    let horodatage = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let chemin = dossier.join(format!("image-{horodatage}.png"));
    if !ecrire(&chemin, &png) {
        message("L'image n'a pas pu être préparée");
        return;
    }
    let chemin_w: Vec<u16> = chemin.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let info = OPENASINFO {
        pcszFile: PCWSTR(chemin_w.as_ptr()),
        pcszClass: PCWSTR::null(),
        // Exécuter le choix, sans la case « toujours utiliser » : le fichier
        // est temporaire, l'associer n'aurait pas de sens.
        oaifInFlags: OAIF_EXEC | OAIF_HIDE_REGISTRATION,
    };
    // Annulé : le dialogue rend une erreur — pas une panne, juste une trace.
    if let Err(err) = unsafe { SHOpenWithDialog(cef_popup::hwnd(&SLOT), &info) } {
        crate::diag!("[editeur] ouvrir avec : dialogue fermé sans choix ({err})");
    }
}

/// **ENREGISTRER** : pour `png` et `jpg`, la page a déposé l'image déjà encodée
/// — Rust ne fait que choisir le chemin et écrire. Pour `psd` et `xcf`, elle a
/// déposé ses calques (`document_image::depuis_page`), que Rust met au format.
fn handle_enregistrer(format: &str) {
    let (extension, libelle) = match format {
        "png" => ("png", "Image PNG"),
        "jpg" => ("jpg", "Image JPEG"),
        "psd" => ("psd", "Image Photoshop"),
        "xcf" => ("xcf", "Image GIMP"),
        _ => {
            crate::diag!("[editeur] format d'enregistrement inconnu : {format:?}");
            return;
        }
    };
    let Some(octets) = cef_data::prendre(ROUTE_IMAGE) else {
        crate::diag!("[editeur] enregistrer : AUCUNE image reçue du canal");
        return;
    };
    let octets = match mettre_au_format(format, octets) {
        Ok(octets) => octets,
        Err(erreur) => {
            crate::diag!("[editeur] enregistrer en {format} : ÉCHEC ({erreur})");
            message("L'image n'a pas pu être enregistrée");
            return;
        }
    };
    let chemin = match ENREGISTREMENT_DEBUG.with(|d| d.borrow_mut().take()) {
        Some((chemin, original)) => {
            ecrire(&chemin, &octets);
            if matches!(format, "psd" | "xcf") {
                relire_document(&chemin);
            } else {
                comparer_au_pixel(&original, &chemin);
            }
            return;
        }
        None => {
            let Some(proprietaire) = cef_popup::hwnd(&SLOT) else { return };
            let Some(chemin) = file_dialog::enregistrer_sous(proprietaire, "Enregistrer l'image", "image", extension, libelle) else {
                return;
            };
            chemin
        }
    };
    if ecrire(&chemin, &octets) {
        message("Image enregistrée");
    } else {
        message("L'image n'a pas pu être enregistrée");
    }
}

/// Les octets du fichier : tels quels pour une image plate, sinon les calques de
/// la page écrits en PSD ou en XCF.
fn mettre_au_format(format: &str, octets: Vec<u8>) -> Result<Vec<u8>, String> {
    match format {
        "psd" => crate::win32::format_psd::ecrire(&document_image::depuis_page(&octets)?),
        "xcf" => crate::win32::format_xcf::ecrire(&document_image::depuis_page(&octets)?.document),
        _ => Ok(octets),
    }
}

/// La preuve d'un enregistrement à calques (levier de vérification) : le
/// fichier écrit se relit, et le journal en dit les calques.
fn relire_document(chemin: &Path) {
    let relu = std::fs::read(chemin).map_err(|e| e.to_string()).and_then(|o| decodeur_document::decoder(&o));
    match relu {
        Ok(doc) => {
            crate::diag!("[editeur] relu : {}×{}, {} calque(s)", doc.largeur, doc.hauteur, doc.calques.len());
            for c in &doc.calques {
                crate::diag!(
                    "[editeur]   « {} » ({}, {}) {}×{} {} opacité {:.2}{}",
                    c.nom,
                    c.x,
                    c.y,
                    c.largeur,
                    c.hauteur,
                    c.mode.nom(),
                    c.opacite,
                    if c.visible { "" } else { " CACHÉ" }
                );
            }
        }
        Err(erreur) => crate::diag!("[editeur] relecture de « {} » ÉCHOUÉE : {erreur}", chemin.display()),
    }
}

fn ecrire(chemin: &Path, octets: &[u8]) -> bool {
    match std::fs::write(chemin, octets) {
        Ok(()) => {
            crate::diag!("[editeur] « {} » écrit ({} octets)", chemin.display(), octets.len());
            true
        }
        Err(err) => {
            crate::diag!("[editeur] « {} » N'A PAS PU ÊTRE ÉCRIT ({err})", chemin.display());
            false
        }
    }
}

/// La preuve du lot 1 : ouvrir → enregistrer rend l'image **au pixel près**.
fn comparer_au_pixel(original: &Path, copie: &Path) {
    let lire = |p: &Path| std::fs::read(p).ok().and_then(|o| clipboard::decode_png(&o));
    match (lire(original), lire(copie)) {
        (Some(a), Some(b)) => {
            let differents: Vec<_> = a.2.chunks(4).zip(b.2.chunks(4)).filter(|(x, y)| x != y).collect();
            // Ce qui départage « défaut » et « limite connue » : un canevas stocke
            // l'alpha PRÉMULTIPLIÉ, donc seul un pixel SEMI-TRANSPARENT peut y perdre
            // des niveaux. Un pixel opaque touché serait un vrai défaut.
            let opaques = differents.iter().filter(|(x, _)| x[3] == 255).count();
            let ecart_max = differents
                .iter()
                .flat_map(|(x, y)| x.iter().zip(y.iter()).map(|(p, q)| p.abs_diff(*q)))
                .max()
                .unwrap_or(0);
            let alpha_max = differents.iter().map(|(x, _)| x[3]).max().unwrap_or(0);
            let differents = differents.len();
            crate::diag!(
                "[editeur] comparaison : original {}×{}, copie {}×{}, pixels différents {differents} (dont opaques {opaques}, alpha max {alpha_max}, écart max {ecart_max}) → {}",
                a.0,
                a.1,
                b.0,
                b.1,
                if (a.0, a.1) == (b.0, b.1) && differents == 0 { "IDENTIQUE AU PIXEL PRÈS" } else { "DIFFÉRENT" }
            );
        }
        _ => crate::diag!("[editeur] comparaison impossible (un des deux PNG est illisible)"),
    }
}

/// Vide et traite les actions de la page — à chaque frame (voir `app::run`).
pub fn dispatch_actions() {
    if COLLER_DEMANDE.with(|c| c.replace(false)) {
        handle_coller();
    }
    sonder_envoi();
    sonder_decodage();
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("editeur/") else {
            crate::diag!("[editeur] action inconnue : {action:?}");
            continue;
        };
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["pret"] => {
                PRETE.with(|p| p.set(true));
                if let Some(js) = EN_ATTENTE.with(|e| e.borrow_mut().take()) {
                    cef_popup::execute_js(&SLOT, &js);
                }
                cef_popup::execute_js(&SLOT, &script_service());
            }
            ["ouvrir"] => handle_ouvrir(false),
            ["ouvrir-calques"] => handle_ouvrir(true),
            ["coller"] => handle_coller(),
            ["copier"] => handle_copier(),
            // « Valider » (l'outil « accept » de Flameshot) : copier, puis fermer.
            ["valider"] => {
                handle_copier();
                cef_popup::hide_if_visible(&SLOT);
            }
            ["epingler"] => handle_epingler(),
            ["ouvrir-avec"] => handle_ouvrir_avec(),
            ["envoyer-en-ligne"] => handle_envoyer_en_ligne(),
            // ⭐ LA RECONNAISSANCE DE TEXTE de l'image affichée, MOTEUR CHOISI
            // (demande 2026-11-04, voir `win32::texte_capture`).
            ["texte", moteur] => handle_texte(moteur),
            ["fermer"] => cef_popup::hide_if_visible(&SLOT),
            ["enregistrer", format] => handle_enregistrer(format),
            ["charge", largeur, hauteur] => crate::diag!("[editeur] image affichée : {largeur}×{hauteur}"),
            ["annote", nombre] => crate::diag!("[editeur] annotations posées : {nombre}"),
            ["geometrie", reste @ ..] => crate::diag!("[editeur] géométrie : {}", reste.join(" ")),
            ["erreur", texte] => crate::diag!("[editeur] erreur de la page : {texte}"),
            _ => crate::diag!("[editeur] action inconnue : {action:?}"),
        }
    }
}

/// **LEVIERS DE VÉRIFICATION** (appels de fonction directs, jamais une entrée
/// simulée) — appelés à chaque frame par `app::run` quand le harnais a ouvert
/// `editeur-image` :
/// - `NEURADESK_DEBUG_EDITEUR_FICHIER=<chemin>` : charge ce fichier sans dialogue ;
/// - `NEURADESK_DEBUG_EDITEUR_ENREGISTRER=<chemin>` : `frame_enregistrer` frames
///   plus tard, demande à la page d'enregistrer en PNG, écrit à ce chemin SANS
///   dialogue et compare au pixel près avec le fichier chargé ;
/// - `NEURADESK_DEBUG_EDITEUR_FORMAT=psd|xcf` : enregistre à calques plutôt
///   qu'en PNG — le fichier écrit est alors RELU, et ses calques vont au journal ;
/// - `NEURADESK_DEBUG_EDITEUR_ANNOTER=1` : à la même frame, JUSTE AVANT
///   l'enregistrement, pose un objet de chaque sorte (`editeurDebugAnnoter`) —
///   la comparaison doit alors dire DIFFÉRENT, et le fichier les montrer ;
/// - `NEURADESK_DEBUG_EDITEUR_GEOMETRIE=1|2` : ensuite, une suite d'opérations
///   de géométrie (`editeurDebugGeometrie`). En mode 1 elle rend l'image
///   d'origine : la comparaison doit dire IDENTIQUE ; le mode 2 finit par un
///   quart de tour, une découpe et une réduction.
/// - `NEURADESK_DEBUG_EDITEUR_COULEURS=1` : pose des réglages de couleur SANS
///   les appliquer (`editeurDebugCouleurs`, saturation à −100) — l'enregistrement
///   doit les appliquer lui-même, donc rendre un fichier GRIS (r = g = b).
pub fn debug_leviers(frame: u32, frame_enregistrer: u32) {
    let appliques = LEVIERS_APPLIQUES.with(Cell::get);
    if appliques == 0 && PRETE.with(Cell::get) {
        LEVIERS_APPLIQUES.with(|c| c.set(1));
        if let Some(chemin) = std::env::var_os("NEURADESK_DEBUG_EDITEUR_FICHIER") {
            charger_fichier(Path::new(&chemin), false);
        }
    }
    if appliques == 1 && frame >= frame_enregistrer {
        LEVIERS_APPLIQUES.with(|c| c.set(2));
        if std::env::var_os("NEURADESK_DEBUG_EDITEUR_ANNOTER").is_some() {
            cef_popup::execute_js(&SLOT, "window.editeurDebugAnnoter && window.editeurDebugAnnoter()");
        }
        if let Some(mode) = std::env::var_os("NEURADESK_DEBUG_EDITEUR_GEOMETRIE") {
            let mode = if mode == "2" { 2 } else { 1 };
            cef_popup::execute_js(&SLOT, &format!("window.editeurDebugGeometrie && window.editeurDebugGeometrie({mode})"));
        }
        if std::env::var_os("NEURADESK_DEBUG_EDITEUR_COULEURS").is_some() {
            cef_popup::execute_js(&SLOT, "window.editeurDebugCouleurs && window.editeurDebugCouleurs()");
        }
        let (Some(dest), Some(original)) =
            (std::env::var_os("NEURADESK_DEBUG_EDITEUR_ENREGISTRER"), std::env::var_os("NEURADESK_DEBUG_EDITEUR_FICHIER"))
        else {
            return;
        };
        ENREGISTREMENT_DEBUG.with(|d| *d.borrow_mut() = Some((PathBuf::from(dest), PathBuf::from(original))));
        let format = match std::env::var("NEURADESK_DEBUG_EDITEUR_FORMAT").as_deref() {
            Ok("psd") => "psd",
            Ok("xcf") => "xcf",
            _ => "png",
        };
        cef_popup::execute_js(&SLOT, &format!("window.editeurEnregistrer && window.editeurEnregistrer('{format}')"));
    }
}

/// Voir `win32::cef_popup::debug_dump_backbuffer` (`NEURADESK_DEBUG_OPEN=editeur-image`).
pub fn debug_dump_backbuffer(path: &Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// Le caractère que Windows produit pour Ctrl+V.
const CARACTERE_CTRL_V: usize = 0x16;

/// Ctrl+V (sans Alt : Ctrl+Alt est AltGr) ou Maj+Inser — les deux raccourcis
/// qui déclenchent le coller de Chromium (voir `wndproc`).
fn est_un_coller(wparam: WPARAM) -> bool {
    let enfoncee = |vk: u16| unsafe { (GetKeyState(vk as i32) as u16) & 0x8000 != 0 };
    let touche = wparam.0 as u16;
    let ctrl = enfoncee(VK_CONTROL.0);
    (touche == VK_V.0 && ctrl && !enfoncee(VK_MENU.0)) || (touche == VK_INSERT.0 && enfoncee(VK_SHIFT.0) && !ctrl)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // ⛔ **LE COLLER N'ATTEINT JAMAIS CHROMIUM** : sur Ctrl+V, Chromium lit le
        // presse-papiers sur un fil À LUI (`GetClipboardData`) pendant que
        // `handle_coller` le lit sur le nôtre — deux fils du MÊME processus, et le
        // tas s'est corrompu (0xC0000374 dans `GetClipboardData` → `GlobalSize`,
        // vidage du 2026-09-24). La touche est donc avalée ici, et le coller part
        // de NOTRE seul fil. Son `WM_CHAR` (0x16) et son `WM_KEYUP` avec.
        WM_KEYDOWN | WM_KEYUP if est_un_coller(wparam) => {
            if msg == WM_KEYDOWN {
                COLLER_DEMANDE.with(|c| c.set(true));
            }
            return LRESULT(0);
        }
        WM_CHAR if wparam.0 == CARACTERE_CTRL_V => return LRESULT(0),
        _ => {}
    }
    if let Some(result) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seules_les_images_connues_sont_ouvertes() {
        assert_eq!(mime_pour(Path::new("C:/a/b.PNG")), Some("image/png"));
        assert_eq!(mime_pour(Path::new("x.jpeg")), Some("image/jpeg"));
        assert_eq!(mime_pour(Path::new("x.webp")), Some("image/webp"));
        assert_eq!(mime_pour(Path::new("x.txt")), None);
        assert_eq!(mime_pour(Path::new("sans_extension")), None);
        // Tout ce que le dialogue propose doit être ouvrable.
        for extension in EXTENSIONS_OUVERTES.split(';') {
            let chemin = format!("x.{extension}");
            assert!(mime_pour(Path::new(&chemin)).is_some() || decodeur_document::est_un_document(Path::new(&chemin)), "{extension}");
        }
    }

    /// **LES DEUX MOTEURS SONT OFFERTS PAR LEUR NOM TECHNIQUE** — le libellé
    /// (« NAPS2 », « Copilot ») est fait pour l'œil, le verbe porte le nom stable
    /// de `ocr::Moteur::nom` (demande 2026-11-04).
    #[test]
    fn la_page_offre_les_deux_moteurs_de_reconnaissance() {
        let page = include_str!("../../assets/popups/editeur_image.html");
        for (moteur, nom, _libelle) in crate::win32::ocr::Moteur::TOUS {
            assert!(page.contains(&format!("\"texte\", \"{nom}\"")), "la page n'offre pas {moteur:?} ({nom})");
        }
    }

    /// Chaque verbe que la page émet doit avoir sa branche ici — sinon il tombe
    /// dans « action inconnue », en silence pour l'utilisateur.
    #[test]
    fn chaque_verbe_de_la_page_est_traite() {
        let page = include_str!("../../assets/popups/editeur_image.html");
        let source = include_str!("cef_image_editor.rs");
        for verbe in [
            "pret",
            "ouvrir",
            "ouvrir-calques",
            "coller",
            "copier",
            "valider",
            "epingler",
            "ouvrir-avec",
            "envoyer-en-ligne",
            "texte",
            "fermer",
            "enregistrer",
            "charge",
            "annote",
            "geometrie",
            "erreur",
        ] {
            assert!(page.contains(&format!("\"{verbe}\"")), "la page n'émet pas {verbe:?}");
            assert!(source.contains(&format!("[\"{verbe}\"")), "le dispatch ne traite pas {verbe:?}");
        }
    }
}
