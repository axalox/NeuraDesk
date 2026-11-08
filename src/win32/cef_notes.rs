//! Fenêtre CEF du dash "Notes" — prise de notes libres inspirée de
//! [usememos/memos](https://github.com/usememos/memos) (contenu texte,
//! étiquettes `#tag` extraites du contenu, épinglage, liste anté-
//! chronologique), PAS un portage de sa pile Go/React/SQL : juste
//! l'inspiration fonctionnelle/UX, adaptée au modèle local mono-utilisateur
//! de ce dock (voir `win32::notes` pour le modèle de données pur et
//! `config::load_notes`/`save_notes` pour la persistance).
//!
//! Même "zone libre" que le Dash/la visionneuse du presse-papiers (voir
//! `app::dash_rect`, `win32::cef_clipboard`, doc de module) — structure
//! calquée sur `win32::cef_clipboard` (même squelette `SLOT`/`SPEC`/
//! `toggle`/`push_state`/`dispatch_actions`/`wndproc`), simplifiée sur un
//! point : "Coller" (voir `handle_paste`) ne réinjecte JAMAIS l'entrée dans
//! une fenêtre étrangère via `SendInput` (pas de `PREVIOUS_FOREGROUND`) —
//! coller ici signifie "ajouter à la note en cours", pas "coller dans
//! l'application précédente" ; une note se modifie et se supprime par
//! ailleurs uniquement depuis sa propre fenêtre.
//!
//! Recherche et filtre par étiquette sont volontairement PURE JS côté page
//! (`assets/popups/notes.html`) plutôt que des actions IPC : l'ensemble des
//! notes est déjà entièrement poussé dans la page (voir `push_state`), un
//! aller-retour Rust par frappe clavier n'apporterait rien et risquerait de
//! faire perdre le focus du champ de recherche à chaque `setState`.
//!
//! `push_state` est appelé à CHAQUE frame tant que la fenêtre est visible
//! (voir `refresh_if_visible`) — désormais potentiellement coûteux une fois
//! des images attachées (miniatures en base64, voir `win32::notes::
//! NoteImage`). `READY`/`LAST_HASH` ci-dessous mémorisent côté RUST le
//! dernier état effectivement envoyé, pour ne reconstruire/renvoyer le JSON
//! complet que si quelque chose a réellement changé — en plus, PAS à la
//! place, du garde-fou déjà côté JS (`lastStateJson` dans `notes.html`, qui
//! évite seulement le nouveau rendu DOM, pas le coût de sérialisation/envoi
//! IPC côté Rust).

use std::cell::Cell;
use std::hash::{Hash, Hasher};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::config;
use crate::gfx::device::GfxDevice;
use crate::win32::cef_app::percent_decode;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::clipboard::{self, ClipboardEntry};
use crate::win32::notes::{self, extract_tags, next_id, sorted_for_display, unix_now, Note, MAX_NOTE_CHARS};

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// `true` une fois que `assets/popups/notes.html` a signalé qu'elle a
    /// fini de charger (voir `["ready"]` ci-dessous) — mis à `true` une
    /// SEULE fois, jamais réinitialisé : la page CEF n'est créée qu'une fois
    /// (voir `win32::cef_popup::toggle_with`, doc de module), un
    /// close/reopen ne la recharge pas et ne renvoie donc jamais un second
    /// signal "ready". Le réinitialiser à chaque fermeture casserait
    /// silencieusement `push_state` pour toujours après le tout premier
    /// cycle ouverture/fermeture.
    static READY: Cell<bool> = const { Cell::new(false) };
    /// Empreinte du dernier état effectivement envoyé à la page (voir
    /// `state_hash`) — `None` tant que `READY` est `false`, pour ne jamais
    /// filtrer le tout premier envoi une fois la page prête (voir
    /// `handle_ready`, qui le remet explicitement à `None`).
    static LAST_HASH: Cell<Option<u64>> = const { Cell::new(None) };
    /// Échelle du texte des notes (variable CSS `--text-scale` de
    /// `assets/popups/notes.html`) — DEMANDE EXPLICITE ("ajoutes la
    /// possibilité de modification des réglages du dash notes dans taille
    /// du contenu des dashs"). Même mécanisme que `win32::cef_clipboard::
    /// TEXT_SCALE`/`THUMB_SCALE` (thread_local chargé au démarrage par
    /// `app::run`, mis à jour par un curseur de `win32::cef_settings`),
    /// séparée de `IMAGE_SCALE` pour la même raison qu'eux : DEMANDE
    /// EXPLICITE de pouvoir régler texte et images indépendamment.
    static TEXT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// Échelle des miniatures d'image jointes aux notes — voir `TEXT_SCALE`.
    static IMAGE_SCALE: Cell<f32> = const { Cell::new(1.0) };
    // PAS de cellule d'accent ici : ce panneau a été le PREMIER à recevoir les
    // couleurs dérivées de l'accent, et il les transportait par son état
    // (`fgRgb`/`accentRgb` dans `push_state`), ce qui avait forcé une cellule
    // mémorisée à chaque frame — `push_state` ayant trois points d'entrée dont
    // un seul reçoit l'accent. Les quatre panneaux partagent désormais
    // `win32::cef_popup::push_window_palette`, poussée depuis `render` : il n'y
    // a plus rien à transporter par l'état, donc plus rien à mémoriser.
}

/// La taille des textes GLOBALE (`taille_panneaux::TEXTES`) : le panneau ne
/// passe pas par `app::content_scale_pour`, elle multiplie donc `text_scale`.
fn taille_globale() -> f32 {
    crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES)
}

/// Facteur d'échelle actuel du texte des notes — voir `TEXT_SCALE`.
pub fn text_scale() -> f32 {
    TEXT_SCALE.with(Cell::get)
}

/// Borne HAUTE des DEUX échelles de ce panneau, relevée de `2.0` à `4.0` sur
/// DEMANDE EXPLICITE (« j'ai encore le bug de taille des éléments dans le dash
/// notes [...] j'ai modifié les curseurs jusqu'à 200 % mais c'est toujours trop
/// petit »).
///
/// **À lire avant de repartir sur ce symptôme, ça a coûté une session.** Le
/// défaut a été pris pour une PERTE de réglages — le panneau retombant à
/// l'échelle de la feuille de style — et traqué comme telle : trois mesures
/// (fichiers, valeurs poussées par Rust, valeurs appliquées par la page) n'ont
/// jamais constaté de perte, et le correctif d'ordre appliqué alors (`6247f06`)
/// n'a rien changé pour l'utilisateur. Le symptôme réel était l'INVERSE :
/// l'échelle s'applique très bien, et son plafond est trop bas. Un seul geste
/// sépare les deux causes — passer le curseur de 100 % à 200 % et regarder si le
/// panneau grossit — et il vaut mieux le faire que raisonner.
///
/// La borne BASSE n'a pas bougé (`0.25`, plus large que celle du Dash, voir
/// `win32::cef_dash::set_icon_scale`) ; c'est la haute qui diverge désormais :
/// les deux fonctions ci-dessous ne partagent donc PLUS les bornes exactes du
/// Dash, contrairement à ce qu'annonçait leur doc d'origine.
const MAX_SCALE: f32 = 4.0;

/// Change le facteur d'échelle du texte des notes, pris en compte à la
/// PROCHAINE `push_state` — appelé par `win32::cef_settings` (curseur "Texte"
/// de la sous-section "Notes") et par `app::run` au démarrage pour reprendre la
/// valeur persistée (voir `config::load_notes_text_scale`). Borne haute propre à
/// ce panneau, voir `MAX_SCALE`.
pub fn set_text_scale(scale: f32) {
    TEXT_SCALE.with(|s| s.set(scale.clamp(0.25, MAX_SCALE)));
}

/// Facteur d'échelle actuel des miniatures d'image des notes — voir
/// `IMAGE_SCALE`.
pub fn image_scale() -> f32 {
    IMAGE_SCALE.with(Cell::get)
}

/// Change le facteur d'échelle des miniatures d'image des notes — voir
/// `set_text_scale`, mêmes garanties ET mêmes bornes, `MAX_SCALE` comprise
/// (curseur "Images" plutôt que "Texte", `config::load_notes_image_scale`).
pub fn set_image_scale(scale: f32) {
    IMAGE_SCALE.with(|s| s.set(scale.clamp(0.25, MAX_SCALE)));
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefNotesWindow"),
    window_title: w!("NeuraDesk — Notes"),
    html_file: "notes.html",
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

/// Voir `win32::cef_clipboard::toggle`, même mécanisme (`cef_popup::
/// toggle_with`, fermeture des autres fenêtres "zone libre" AVANT
/// `force_foreground`). `notes` : l'état ACTUEL (voir `app::run`) — poussé
/// dans la page à chaque ouverture (voir `push_state`), `refresh_if_visible`
/// le repousse aussi à chaque frame tant que cette fenêtre reste ouverte
/// (voir sa doc : remède à la course "il faut ouvrir 2 fois les popups").
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, notes: &[Note]) {
    // ⛔ **SON ZOOM DOIT SUIVRE LE DPI.** ⚠️ La raison n'est PAS que sa page
    // manquerait du mécanisme : `notes.html` porte bel et bien `--text-scale`
    // (`zoom: var(--text-scale)` + `width: calc(100% / var(--text-scale))`), comme
    // les panneaux. C'est que **rien ne l'alimente** — ce module ne pousse aucun
    // `contentScale` (vérifié). Le zoom reste donc le seul levier qui l'atteint,
    // et sans lui la fenêtre garde la même taille PHYSIQUE partout, donc paraît
    // plus grande sur un écran à 100 % que sur la primaire.
    // Voir `cef_popup::regler_zoom_dpi`, qui liste les trois inscrits.
    cef_popup::regler_zoom_dpi(&SLOT, true);
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, || {
        crate::win32::cef_dash::hide_if_visible();
        crate::win32::cef_settings::hide_if_visible();
        crate::win32::cef_clipboard::hide_if_visible();
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
    // **C'EST ICI QUE SE CORRIGE LE DÉFAUT « il a encore oublié ses réglages
    // de taille ».** `push_state` sort en avance quand l'empreinte n'a pas
    // changé (voir sa doc) — et une RÉOUVERTURE ne change rien à l'état des
    // notes, donc l'empreinte est identique et **rien n'était renvoyé à la
    // page**. Tant que la page garde ses variables CSS, ça passe inaperçu ; le
    // jour où elle les perd (rechargement, reconstruction du DOM, ou toute
    // autre cause), rouvrir le panneau ne les restaure PAS : il s'affiche aux
    // valeurs par défaut de la feuille de style, et il faut une seconde
    // ouverture — ou un changement de notes — pour que l'état repasse.
    //
    // L'invalider ici rend une réouverture TOUJOURS significative : le coût est
    // une sérialisation du JSON par ouverture, ce qui est le prix d'un panneau
    // qui s'affiche juste du premier coup. Même remède, et même raison, que
    // `handle_ready` — sauf que lui ne joue qu'une fois par lancement.
    LAST_HASH.with(|c| c.set(None));
    push_state(notes);
}

/// Repousse l'état SEULEMENT si la fenêtre est visible — à appeler à CHAQUE
/// frame (voir `app::run`), pas seulement après une mutation : contrairement
/// au presse-papiers (poussé sur `Event::ClipboardChanged`, un événement
/// externe précis), les notes ne changent que par les actions de CETTE
/// fenêtre elle-même, mais `push_state` initial peut arriver avant que la
/// page ait fini de charger (voir `win32::cef_dash`, doc similaire) — des
/// appels répétés par frame corrigent ça sans coût perceptible une fois la
/// page prête (voir `state_hash`, qui les rend gratuits si rien n'a changé).
pub fn refresh_if_visible(notes: &[Note]) {
    if cef_popup::is_visible(&SLOT) {
        push_state(notes);
    }
}

/// Empreinte bon marché de l'état à envoyer — calculée sur les champs qui
/// affectent réellement l'affichage (PAS sur `NoteImage::thumb_data_uri`,
/// qui pèse potentiellement des dizaines de Ko en base64 par image et ne
/// changerait de toute façon jamais sans que `image.file` ne change aussi) :
/// reconstruire cette empreinte est donc négligeable même avec plusieurs
/// notes illustrées, contrairement à la reconstruction du JSON complet
/// qu'elle permet d'éviter (voir `push_state`). Calculée sur `notes` dans
/// son ordre de stockage (PAS `sorted_for_display`) : cet ordre ne change
/// jamais tant que la liste de notes elle-même ne change pas, contrairement
/// à `sorted_for_display` qui recalcule un ordre pouvant sembler différent
/// à cause de la résolution à la seconde de `updated_at` — hacher l'ordre de
/// stockage est donc déterministe d'un appel à l'autre.
fn state_hash(notes: &[Note]) -> u64 {
    // Le jour, l'heure et l'ensemble acquitté servent à chaque note : lus UNE
    // fois. `rappels_notes_acquittes` est en mémoire (voir sa cellule), donc
    // l'appeler ici ne coûte pas la lecture d'un fichier par frame.
    let (jour_local, heure_locale) = jour_et_heure_locaux();
    let acquittes = crate::win32::fetes::rappels_notes_acquittes();
    // ⚠️ **L'ÉTAT DES DEUX BOUTONS D'HISTORIQUE DOIT ÊTRE ICI** : sans lui,
    // annuler ne changerait pas l'empreinte quand la liste revient à un état
    // DÉJÀ VU — et les boutons resteraient grisés/actifs à contretemps. C'est
    // le même piège que `alarme` et `rappelEn_attente` plus bas.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let (peut_annuler, peut_refaire) = historique_etat();
    peut_annuler.hash(&mut hasher);
    peut_refaire.hash(&mut hasher);
    // Voir `TEXT_SCALE`/`IMAGE_SCALE`, doc de champ — DOIVENT entrer dans
    // cette empreinte : sans ça, changer un des deux curseurs "Notes" de
    // `win32::cef_settings` sans autre modification des notes elles-mêmes
    // laisserait `push_state` croire l'état inchangé (même hash qu'avant) et
    // ne renverrait donc jamais la nouvelle échelle à la page.
    text_scale().to_bits().hash(&mut hasher);
    taille_globale().to_bits().hash(&mut hasher);
    image_scale().to_bits().hash(&mut hasher);
    // L'accent n'entre plus dans cette empreinte : il ne voyage plus par l'état
    // (voir `push_window_palette`, appelée depuis `render` à chaque frame). Le
    // laisser ici ferait dépendre l'empreinte d'une valeur qui n'est plus
    // poussée par ce canal — un hash qui change sans que le contenu envoyé
    // change.
    for note in notes {
        note.id.hash(&mut hasher);
        note.content.hash(&mut hasher);
        note.pinned.hash(&mut hasher);
        note.updated_at.hash(&mut hasher);
        // ⚠️ Archiver ne touche ni le texte ni `updated_at` : sans ces deux
        // lignes, la note archivée resterait dans la liste de la page.
        note.archived.hash(&mut hasher);
        note.created_at.hash(&mut hasher);
        // ⚠️ **L'ALARME DOIT ÊTRE ICI.** Sans elle, poser ou retirer une alarme
        // ne changerait PAS cette empreinte, donc `push_state` croirait l'état
        // inchangé et **ne renverrait rien à la page** : le bouton resterait
        // dessiné comme avant, et le clic paraîtrait ignoré. C'est exactement le
        // piège que les deux lignes du haut racontent pour les échelles.
        if let Some(alarme) = &note.alarme {
            alarme.date.hash(&mut hasher);
            alarme.heure.hash(&mut hasher);
            alarme.jours.hash(&mut hasher);
        }
        // ⚠️ **ET LE RAPPEL EN ATTENTE AUSSI** — voir sa doc : il change au
        // passage de l'heure et à l'acquittement, sans qu'aucun champ de la note
        // ne bouge. C'est le même piège que l'alarme juste au-dessus, en plus
        // sournois : là, une donnée changeait quand même.
        rappel_en_attente(note, jour_local, heure_locale, &acquittes).hash(&mut hasher);
        for image in &note.images {
            image.file.hash(&mut hasher);
            image.width.hash(&mut hasher);
            image.height.hash(&mut hasher);
        }
    }
    // ⛔ **LES COULEURS D'ÉTIQUETTES DOIVENT ÊTRE DANS CETTE EMPREINTE.** Elles
    // n'apparaissent NULLE PART ailleurs ici — ni dans une note, ni dans les
    // échelles — donc sans ces deux lignes, choisir une couleur ne changeait pas
    // l'empreinte, `push_state` prenait la sortie rapide, et **la page ne recevait
    // jamais la nouvelle couleur**. C'est le même piège que `blurMaterial` dans le
    // panneau des Réglages, et le même que `alarme` juste au-dessus : ce qui
    // n'entre pas dans l'empreinte ne s'affiche jamais.
    COULEURS_GENERATION.with(|c| c.get()).hash(&mut hasher);
    VUES_GENERATION.with(|c| c.get()).hash(&mut hasher);
    hasher.finish()
}


/// **LE JOUR ET L'HEURE LOCAUX** — ce que `AlarmeNote::sonne_a` attend.
///
/// ⚠️ `app::jour_et_heure_locaux` fait exactement la même chose, mais elle est
/// privée à `app`. La dupliquer ici (trois lignes) vaut mieux que de faire
/// traverser la moitié du programme à une paire de tuples ; la source est la
/// même, `GetLocalTime`.
fn jour_et_heure_locaux() -> ((i32, u32, u32), (u32, u32)) {
    let t = unsafe { GetLocalTime() };
    ((t.wYear as i32, t.wMonth as u32, t.wDay as u32), (t.wHour as u32, t.wMinute as u32))
}

/// **LE RAPPEL DE CETTE NOTE EST-IL EN ATTENTE ?** — dû (voir
/// `AlarmeNote::sonne_a`), et pas encore acquitté aujourd'hui.
///
/// C'est ce que la page lit pour afficher le bouton « Acquitter », et ce que la
/// barre du haut lit pour faire clignoter l'icône de note.
///
/// ⚠️ **IL DOIT ENTRER DANS `state_hash`** : il devient vrai au passage de
/// l'HEURE du rappel, sans qu'aucune donnée de la note ne change — et il devient
/// faux à l'acquittement, sans que la note change non plus. Sans lui dans
/// l'empreinte, `push_state` sauterait la poussée dans les deux cas : le bouton
/// n'apparaîtrait jamais, et ne disparaîtrait jamais après le clic.
fn rappel_en_attente(note: &Note, jour: (i32, u32, u32), heure: (u32, u32), acquittes: &str) -> bool {
    let Some(alarme) = &note.alarme else { return false };
    if !alarme.sonne_a(jour, heure) {
        return false;
    }
    match crate::win32::fetes::identite_du_rappel_de_note(note) {
        Some(identite) => !crate::win32::fetes::rappel_note_est_acquitte(acquittes, jour, &identite),
        None => false,
    }
}

fn push_state(notes: &[Note]) {
    if READY.with(|r| r.get()) {
        let hash = state_hash(notes);
        if LAST_HASH.with(|c| c.get()) == Some(hash) {
            return;
        }
        LAST_HASH.with(|c| c.set(Some(hash)));
    }

    let sorted = sorted_for_display(notes);

    // `extract_tags` par note calculé UNE SEULE fois puis réutilisé pour
    // `tag_counts` et `entries` ci-dessous : appelé à chaque frame tant que
    // la fenêtre est visible (voir `refresh_if_visible`), un doublon ici
    // coûterait le scan de tout le contenu de chaque note deux fois par
    // frame pour rien.
    let tags_per_note: Vec<Vec<String>> = sorted.iter().map(|note| extract_tags(&note.content)).collect();

    let mut tag_counts: Vec<(String, usize)> = Vec::new();
    for tags in &tags_per_note {
        for tag in tags {
            let lower = tag.to_lowercase();
            match tag_counts.iter_mut().find(|(t, _)| t.to_lowercase() == lower) {
                Some((_, count)) => *count += 1,
                None => tag_counts.push((tag.clone(), 1)),
            }
        }
    }
    tag_counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));

    // Une seule lecture pour toutes les notes, comme dans `state_hash` — et la
    // même que lui, sinon la page et l'empreinte pourraient ne pas dire la même
    // chose au même instant.
    let (jour_local, heure_locale) = jour_et_heure_locaux();
    let acquittes = crate::win32::fetes::rappels_notes_acquittes();
    // L'état des deux boutons d'historique — la même source que l'empreinte
    // ci-dessus, donc la page et elle ne peuvent pas se contredire.
    let (peut_annuler, peut_refaire) = historique_etat();

    let entries: Vec<serde_json::Value> = sorted
        .iter()
        .zip(tags_per_note)
        .map(|(note, tags)| {
            let images: Vec<serde_json::Value> = note
                .images
                .iter()
                .map(|image| json!({ "width": image.width, "height": image.height, "thumb": image.thumb_data_uri }))
                .collect();
            json!({
                "id": note.id,
                "content": note.content,
                "pinned": note.pinned,
                "tags": tags,
                "images": images,
                "updatedAt": note.updated_at,
                // Le calendrier d'activité range une note sous son jour de
                // CRÉATION (voir `renderCalendrier` dans la page).
                "createdAt": note.created_at,
                "archived": note.archived,
                // `null` quand il n'y a pas d'alarme — la page n'a qu'un test à
                // faire, et c'est le cas de la quasi-totalité des notes.
                "alarme": note.alarme.as_ref().map(|a| json!({ "date": a.date, "heure": a.heure, "jours": a.jours })),
                // **LE RAPPEL EST-IL EN ATTENTE ?** — c'est ce qui fait
                // apparaître le bouton « Acquitter » de la page : dû, et pas
                // encore acquitté aujourd'hui. Voir `rappel_en_attente`.
                "rappelEnAttente": rappel_en_attente(note, jour_local, heure_locale, &acquittes),
            })
        })
        .collect();
    // **LA COULEUR DE CHAQUE ÉTIQUETTE**, choisie par l'utilisateur (voir
    // `config::load_tag_colors`). Lue UNE fois pour toutes les étiquettes :
    // c'est un fichier, et le panneau se repousse à chaque frame tant qu'il
    // est ouvert.
    let couleurs_etiquettes = config::load_tag_colors();
    let all_tags: Vec<serde_json::Value> = tag_counts
        .into_iter()
        .map(|(name, count)| {
            json!({
                "name": name,
                "count": count,
                // `null` quand aucune couleur n'a été choisie — la page garde
                // alors la teinte par défaut, et n'a qu'un test à faire.
                "couleur": couleurs_etiquettes.get(&name),
            })
        })
        .collect();

    // Les couleurs d'accent ne figurent PLUS dans ce payload : elles sont
    // poussées par `win32::cef_popup::push_window_palette` depuis `render`, à
    // chaque frame où la fenêtre est visible — un seul mécanisme pour les
    // quatre panneaux. La doctrine des deux rôles (texte sur le pôle fixe,
    // éléments sur la teinte) vit désormais LÀ-BAS, avec le défaut réel qui l'a
    // imposée : « avec un accent clair certains éléments deviennent illisibles,
    // ça manque de contraste ».
    // **LES VUES ENREGISTRÉES** — lues seulement quand l'empreinte a changé,
    // comme les couleurs (voir `VUES_GENERATION`).
    let vues: Vec<serde_json::Value> = config::load_vues_notes()
        .iter()
        .map(|v| {
            json!({
                "nom": v.nom,
                "recherche": v.recherche,
                "etiquette": v.etiquette,
                "epinglees": v.epinglees,
                "archivees": v.archivees,
            })
        })
        .collect();
    let payload = json!({
        "notes": entries,
        "allTags": all_tags,
        // **TOUTES LES COULEURS CHOISIES**, et pas seulement celles des étiquettes
        // présentes : l'arbre de la page fait apparaître des niveaux PARENTS
        // (`projets` pour `#projets/neuradesk`) qu'aucune note ne porte seuls, et
        // leur couleur doit pouvoir se choisir et se revoir.
        "couleursEtiquettes": couleurs_etiquettes,
        "vues": vues,
        "maxChars": MAX_NOTE_CHARS,
        // **L'AVERTISSEMENT, QUAND LES NOTES N'ONT PAS PU ÊTRE LUES** — la page
        // le montre en bandeau, avec le chemin (voir `config::avertissement_notes`).
        // Sans lui, l'utilisateur verrait un panneau VIDE et conclurait que ses
        // notes ont disparu, sans savoir quoi regarder.
        "avertissement": config::avertissement_notes(),
        // **LES DEUX BOUTONS D'HISTORIQUE** — leur état vient de Rust : c'est lui
        // qui sait s'il reste quelque chose à annuler ou à refaire, la page ne
        // fait que l'afficher.
        "peutAnnuler": peut_annuler,
        "peutRefaire": peut_refaire,
        // ⭐ Multipliée par la taille des textes GLOBALE (`taille_globale`).
        "textScalePercent": (text_scale() * taille_globale() * 100.0).round() as i32,
        "imageScalePercent": (image_scale() * 100.0).round() as i32,
    });
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
}

/// Signale que `assets/popups/notes.html` a fini de s'initialiser (dernière
/// ligne de son `<script>`, voir `go("ready")`) — voir doc de `READY` pour
/// pourquoi ce signal ne peut arriver qu'une seule fois par lancement de
/// NeuraDesk. Remet `LAST_HASH` à `None` puis pousse l'état immédiatement :
/// garantit au moins un envoi effectif juste après, plutôt que de dépendre
/// d'un futur appel de `refresh_if_visible` qui pourrait être filtré à tort
/// par une empreinte laissée par un état antérieur.
fn handle_ready(notes: &[Note]) {
    READY.with(|r| r.set(true));
    LAST_HASH.with(|c| c.set(None));
    push_state(notes);
}

/// Vide et applique les actions demandées par `assets/popups/notes.html`
/// (format `"notes/<verbe>[/<args>]"`, voir sa fonction `go()`) — même
/// discipline que `win32::cef_clipboard::dispatch_actions`. Adressage
/// TOUJOURS par `id` (voir `win32::notes::Note::id`, doc de champ) : l'ordre
/// affiché change avec l'épinglage/le tri, un index de liste serait
/// instable.
///
/// `go("paste", editingId, compose)` a TROIS arités possibles selon les
/// segments vides, pas deux : `win32::cef_app::on_before_browse` retire le
/// `/` final de l'URL AVANT de la mettre en file
/// (`action.trim_end_matches('/')`), qui ne mange que les segments vides EN
/// FIN d'URL — un segment vide au milieu (`editingId` vide mais `compose`
/// non vide) survit, voir l'exemple ci-dessous. D'où les trois motifs
/// `["paste"]` / `["paste", editing_id]` / `["paste", editing_id, compose]`
/// plutôt que de tenter un seul motif à arité fixe :
/// - `editing_id` ET `compose` vides → `.../paste//` → `.../paste` →
///   `["paste"]`.
/// - `editing_id` non vide, `compose` vide → `.../paste/5/` → `.../paste/5`
///   → `["paste", "5"]` (donc un `["paste", x]` a TOUJOURS `x` = editing_id,
///   jamais `compose` : ce cas n'existe que quand `compose` est vide).
/// - `compose` non vide (quel que soit `editing_id`, y compris vide) →
///   `["paste", editing_id, compose]`, le vide éventuel de `editing_id`
///   restant visible comme segment du milieu.
/// **L'HISTORIQUE DES NOTES — DES INSTANTANÉS, PAS DES COMMANDES INVERSES.**
///
/// ⚠️ **POURQUOI DES INSTANTANÉS.** Le modèle est un `Vec<Note>` qu'on enregistre
/// ENTIER à chaque changement. Une commande inverse par verbe aurait demandé de
/// la définir pour chacun — créer, modifier, supprimer, épingler, coller une
/// image, en retirer une, poser une alarme, l'acquitter, changer le son — et
/// chacune aurait été une occasion de se tromper **en silence** : une commande
/// inverse fausse ne casse rien, elle rend juste un état qui n'a jamais existé.
/// L'instantané est CORRECT PAR CONSTRUCTION : il n'y a rien à inverser, seulement
/// à remettre.
///
/// ⚠️ **LE COÛT EST LA MÉMOIRE, DONC L'HISTORIQUE EST BORNÉ.** Les notes portent
/// leur miniature en `thumb_data_uri` — quelques dizaines de Ko chacune — donc
/// `PROFONDEUR` instantanés d'une liste de notes restent de l'ordre du mégaoctet.
///
/// ⚠️ **LE CURSEUR EST UN INDEX DANS LA TIMELINE**, qui contient l'état COURANT à
/// `etats[curseur]` : « annuler » le fait reculer, « refaire » le fait avancer, et
/// une action nouvelle TRONQUE ce qui était après — comme partout.
#[derive(Default)]
struct HistoriqueNotes {
    etats: Vec<Vec<Note>>,
    curseur: usize,
}

/// Nombre d'états conservés — voir `HistoriqueNotes`, doc du coût.
const PROFONDEUR_HISTORIQUE: usize = 30;

thread_local! {
    static HISTORIQUE: std::cell::RefCell<HistoriqueNotes> = std::cell::RefCell::new(HistoriqueNotes::default());
    /// **COMPTEUR DES CHANGEMENTS DE COULEUR D'ÉTIQUETTE.** Il entre dans
    /// `state_hash` à la place d'une relecture de `tag_colors.toml`.
    ///
    /// ⚠️ **`state_hash` TOURNE À CHAQUE FRAME** tant que le panneau est ouvert :
    /// y relire le fichier aurait été un accès disque soixante fois par seconde —
    /// exactement ce que `SON_GENERATION` évitait avant que le choix du son ne
    /// déménage dans les Réglages. Le compteur ne bouge qu'au choix.
    static COULEURS_GENERATION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// **COMPTEUR DES CHANGEMENTS DE VUES ENREGISTRÉES** — même rôle que
    /// `COULEURS_GENERATION`, pour `notes_vues.toml` (voir `config::load_vues_notes`).
    static VUES_GENERATION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

impl HistoriqueNotes {
    /// Enregistre un changement : `avant` est l'état d'avant, `apres` celui d'après.
    fn enregistrer(&mut self, avant: Vec<Note>, apres: &[Note]) {
        // Le tout premier changement pose AUSSI l'état initial : sans lui, il n'y
        // aurait rien à retrouver en annulant la toute première action.
        if self.etats.is_empty() {
            self.etats.push(avant);
        }
        self.etats.truncate(self.curseur + 1);
        self.etats.push(apres.to_vec());
        self.curseur = self.etats.len() - 1;
        if self.etats.len() > PROFONDEUR_HISTORIQUE {
            let en_trop = self.etats.len() - PROFONDEUR_HISTORIQUE;
            self.etats.drain(..en_trop);
            self.curseur = self.curseur.saturating_sub(en_trop);
        }
    }

    fn annuler(&mut self) -> Option<Vec<Note>> {
        if self.curseur == 0 {
            return None;
        }
        self.curseur -= 1;
        Some(self.etats[self.curseur].clone())
    }

    fn refaire(&mut self) -> Option<Vec<Note>> {
        if self.curseur + 1 >= self.etats.len() {
            return None;
        }
        self.curseur += 1;
        Some(self.etats[self.curseur].clone())
    }

    fn peut_annuler(&self) -> bool {
        self.curseur > 0
    }

    fn peut_refaire(&self) -> bool {
        self.curseur + 1 < self.etats.len()
    }
}

/// Voir `HistoriqueNotes` — l'état des deux boutons du panneau.
fn historique_etat() -> (bool, bool) {
    HISTORIQUE.with(|h| {
        let h = h.borrow();
        (h.peut_annuler(), h.peut_refaire())
    })
}

pub fn dispatch_actions(notes: &mut Vec<Note>) {
    let actions = cef_popup::drain_actions(&SLOT);
    if actions.is_empty() {
        return;
    }
    // **L'INSTANTANÉ D'AVANT, PRIS UNE SEULE FOIS.** Le clonage n'a lieu que
    // lorsqu'il y a réellement des actions à appliquer — c'est-à-dire presque
    // jamais — et non à chaque frame (voir `drain_actions`, qui rend une liste
    // vide la plupart du temps).
    let avant = notes.clone();
    // ⚠️ **ANNULER ET REFAIRE NE S'HISTORISENT PAS EUX-MÊMES** : sans ce drapeau,
    // annuler empilerait l'état annulé comme une action nouvelle, et « refaire »
    // n'aurait plus rien à retrouver.
    let mut a_historiser = true;
    let mut changed = false;
    // **REPOUSSER SANS ÉCRIRE LES NOTES** — pour ce qui ne touche pas
    // `notes.toml` (les vues) : `changed` le réécrirait, et décalerait ses
    // copies de secours pour rien.
    let mut repousser = false;
    for action in actions {
        let Some(rest) = action.strip_prefix("notes/") else {
            eprintln!("[neuradesk][cef] action notes inconnue : {action:?}");
            continue;
        };
        let parts: Vec<&str> = rest.split('/').collect();
        match parts.as_slice() {
            ["close"] => cef_popup::fermer_par_geste(&SLOT),
            ["ready"] => handle_ready(notes.as_slice()),
            ["create", content] => changed |= handle_create(notes, content),
            // **LE RAPPEL VOYAGE AVEC LE TEXTE** : c'est « Enregistrer » qui
            // engage les deux (voir `alarmeDraft` dans `notes.html`) — DEMANDE
            // EXPLICITE : « que le rappel soit pris en compte seulement quand
            // j'enregistre la note ».
            //
            // ⚠️ **UN SEGMENT DE PLUS, PAS UNE ACTION DE PLUS** : deux
            // `location.href` d'une même tâche se TÉLESCOPENT (seul le dernier
            // survit, en silence), donc un second aller-retour pour le rappel
            // serait perdu. `quand` n'est jamais vide (`aucune` ou `x_y`), et
            // `content` peut l'être sans risque : le piège d'arité ne mange que
            // les segments vides EN FIN d'URL, jamais ceux du milieu.
            ["update", id, content, quand] => changed |= handle_update(notes, id, content, quand),
            ["delete", id] => changed |= handle_delete(notes, id),
            ["toggle-pin", id] => changed |= handle_toggle_pin(notes, id),
            // **COCHER UNE TÂCHE** — DEMANDE EXPLICITE (2026-10-11). Le rang
            // compte les tâches de la note dans l'ordre (voir
            // `notes::basculer_tache`).
            ["cocher", id, rang] => changed |= handle_cocher(notes, id, rang),
            // **ARCHIVER / DÉSARCHIVER** — DEMANDE EXPLICITE (2026-10-11).
            ["archiver", id] => changed |= handle_archiver(notes, id),
            // **COPIER UN BLOC DE CODE** — le bouton « Copier » d'un bloc
            // ```` ``` ```` (voir `formaterNote` dans la page).
            ["copier", texte] => {
                if !clipboard::write_text(&percent_decode(texte)) {
                    crate::diag!("[notes] copie du code REFUSÉE : le presse-papiers est tenu par une autre application");
                }
            }
            // **LES VUES ENREGISTRÉES** — DEMANDE EXPLICITE (2026-10-11).
            ["vue-enregistrer", json] => repousser |= handle_vue_enregistrer(json),
            ["vue-retirer", nom] => repousser |= handle_vue_retirer(nom),
            // **TOUT EXPORTER EN ZIP** — DEMANDE EXPLICITE (2026-10-11).
            ["exporter-tout"] => handle_exporter_tout(notes),
            ["paste"] => changed |= handle_paste(notes, "", ""),
            ["paste", editing_id] => changed |= handle_paste(notes, editing_id, ""),
            ["paste", editing_id, compose] => changed |= handle_paste(notes, editing_id, compose),
            ["paste-into", id] => changed |= handle_paste_into(notes, id),
            // Acquitter le rappel d'une note — la sonnerie du jour s'arrête, le
            // rappel reste posé (voir `handle_acquitter`).
            ["acquitter", id] => changed |= handle_acquitter(notes, id),
            ["remove-image", id, index] => changed |= handle_remove_image(notes, id, index),
            // ⭐ UN QUART DE TOUR d'une image de note (demande 2026-11-08).
            ["pivoter-image", id, index, sens] => changed |= handle_pivoter_image(notes, id, index, sens),
            ["full-image", id, index] => handle_full_image(notes.as_slice(), id, index),
            // ⭐ LA RECONNAISSANCE DE TEXTE d'une image de note, MOTEUR CHOISI
            // (demande 2026-11-04, voir `win32::texte_capture`).
            ["texte", id, index, moteur] => handle_texte(notes, id, index, moteur),
            // **ANNULER / REFAIRE** — DEMANDE EXPLICITE : « toutes les actions ».
            // Ils REMPLACENT la liste par un instantané, et ne s'historiseront pas
            // eux-mêmes (voir `a_historiser`).
            // **TRACE DU CLIC DROIT** — voir `neuradeskRightClick` dans la page,
            // qui porte le pourquoi de cette ligne : ce hook ne transporte AUCUNE
            // coordonnée, donc sans elle « le clic droit ne fait rien » et « le
            // clic droit n'arrive jamais » laissent le même journal vide. Les deux
            // segments sont DÉCODÉS (une étiquette porte un `#`).
            ["clic-droit", survol, memoire] => {
                crate::diag!(
                    "[notes] clic droit : :hover={}, dernier mousemove={}",
                    percent_decode(survol),
                    percent_decode(memoire)
                );
            }
            // **EXPORTER UNE NOTE** — ouvre la fenêtre « Enregistrer sous » de
            // Windows (voir `handle_exporter_md`). Ne modifie rien : le verbe
            // ne touche pas `changed`, donc rien n'est enregistré ni repoussé.
            // **IMPORTER UNE NOTE** — ouvre la fenêtre « Ouvrir » de Windows (voir
            // `handle_importer`). Crée une note, donc `changed` : c'est une vraie
            // modification, et elle s'historise comme les autres.
            ["importer"] => changed |= handle_importer(notes),
            ["exporter-md", id] => {
                handle_exporter_md(notes, id);
            }
            // **OUVRIR UN LIEN D'UNE NOTE DANS LE NAVIGATEUR** — DEMANDE EXPLICITE
            // (2026-10-10) : « que les liens internet soient cliquables ». La page
            // ne fait que proposer ; `lien_ouvrable` décide (voir sa doc).
            ["ouvrir-lien", lien] => match lien_ouvrable(&percent_decode(lien)) {
                Some(url) => crate::win32::appicon::open(&url),
                None => crate::diag!("[notes] lien refusé : {:?}", percent_decode(lien)),
            },
            // **CHOISIR LA COULEUR D'UNE ÉTIQUETTE** — DEMANDE EXPLICITE :
            // « choisies par l'utilisateur ». Le clic droit sur une pastille
            // d'étiquette ouvre la palette (voir `neuradeskRightClick` dans la
            // page).
            //
            // ⛔ **LES DEUX SEGMENTS SONT DÉCODÉS, ET ILS EN ONT BESOIN** : les
            // étiquettes COMMENCENT par un `#` (`#site_Pierre`), que
            // `encodeURIComponent` transforme en `%23` — c'est le piège
            // documenté de ce dispatch, et il a déjà coûté une heure de rappel
            // muette. Décodés ICI, après le découpage sur les `/`.
            ["couleur-etiquette", etiquette, couleur] => {
                let etiquette = percent_decode(etiquette);
                let couleur = percent_decode(couleur);
                config::save_tag_color(&etiquette, &couleur);
                // Sans cet incrément, l'empreinte ne bougerait pas et la page ne
                // verrait jamais la nouvelle couleur (voir la cellule).
                COULEURS_GENERATION.with(|c| c.set(c.get().wrapping_add(1)));
                changed = true;
                // Choisir une couleur n'est pas une modification des notes :
                // on repousse l'état SANS historiser, comme annuler/refaire.
                a_historiser = false;
            }
            ["annuler"] => {
                if let Some(etat) = HISTORIQUE.with(|h| h.borrow_mut().annuler()) {
                    *notes = etat;
                    changed = true;
                    a_historiser = false;
                }
            }
            ["refaire"] => {
                if let Some(etat) = HISTORIQUE.with(|h| h.borrow_mut().refaire()) {
                    *notes = etat;
                    changed = true;
                    a_historiser = false;
                }
            }
            _ => eprintln!("[neuradesk][cef] action notes inconnue : {action:?}"),
        }
    }
    if changed {
        if a_historiser {
            HISTORIQUE.with(|h| h.borrow_mut().enregistrer(avant, notes));
        }
        config::save_notes(notes);
        push_state(notes);
    } else if repousser {
        push_state(notes);
    }
}

/// Coche ou décoche la `rang`-ième tâche — sans toucher `updated_at` : cocher
/// une case ne doit pas faire remonter la note en tête de liste à chaque clic.
fn handle_cocher(notes: &mut [Note], id: &str, rang: &str) -> bool {
    let (Ok(id), Ok(rang)) = (id.parse::<u64>(), rang.parse::<usize>()) else { return false };
    let Some(note) = notes.iter_mut().find(|n| n.id == id) else { return false };
    let Some(contenu) = notes::basculer_tache(&note.content, rang) else {
        crate::diag!("[notes] tâche {rang} introuvable dans la note {id}");
        return false;
    };
    note.content = contenu;
    true
}

/// Archive la note, ou la désarchive — voir `Note::archived`.
fn handle_archiver(notes: &mut [Note], id: &str) -> bool {
    let Ok(id) = id.parse::<u64>() else { return false };
    let Some(note) = notes.iter_mut().find(|n| n.id == id) else { return false };
    note.archived = !note.archived;
    true
}

/// Enregistre une vue envoyée par la page : `{ nom, recherche, etiquette,
/// epinglees, archivees }` en JSON, dans UN segment (encodé) — un segment par
/// champ aurait ramené le piège des segments vides en fin d'URL.
fn handle_vue_enregistrer(json: &str) -> bool {
    let Ok(valeur) = serde_json::from_str::<serde_json::Value>(&percent_decode(json)) else {
        crate::diag!("[notes] vue illisible");
        return false;
    };
    let s = |cle: &str| valeur.get(cle).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let b = |cle: &str| valeur.get(cle).and_then(|x| x.as_bool()).unwrap_or(false);
    let Some(vue) = notes::VueNotes::nouvelle(&s("nom"), &s("recherche"), &s("etiquette"), b("epinglees"), b("archivees")) else {
        return false;
    };
    let mut vues = config::load_vues_notes();
    if !notes::enregistrer_vue(&mut vues, vue) {
        crate::diag!("[notes] vue refusée : déjà {} vues", notes::MAX_VUES_NOTES);
        return false;
    }
    config::save_vues_notes(&vues);
    VUES_GENERATION.with(|c| c.set(c.get().wrapping_add(1)));
    true
}

fn handle_vue_retirer(nom: &str) -> bool {
    let nom = percent_decode(nom);
    let mut vues = config::load_vues_notes();
    let avant = vues.len();
    vues.retain(|v| v.nom != nom);
    if vues.len() == avant {
        return false;
    }
    config::save_vues_notes(&vues);
    VUES_GENERATION.with(|c| c.set(c.get().wrapping_add(1)));
    true
}

/// **EXPORTE TOUTES LES NOTES DANS UNE ARCHIVE ZIP** — DEMANDE EXPLICITE
/// (2026-10-11), d'après memos. Chaque note y est exactement ce que l'export
/// d'une seule note écrit (voir `contenu_d_export`) : un `.md` et ses images, que
/// « Importer » sait reprendre. Les archivées vont dans `archives/`.
fn handle_exporter_tout(notes: &[Note]) {
    if notes.is_empty() {
        return;
    }
    let Some(proprietaire) = cef_popup::hwnd(&SLOT) else { return };
    let ((a, m, j), _) = jour_et_heure_locaux();
    let Some(chemin) = crate::win32::file_dialog::enregistrer_sous(
        proprietaire,
        "Exporter toutes les notes",
        &format!("notes-{a:04}-{m:02}-{j:02}"),
        "zip",
        "Archive ZIP",
    ) else {
        return;
    };
    match ecrire_archive(notes, &chemin) {
        Ok(nombre) => crate::diag!("[notes] export : {nombre} notes écrites dans « {} »", chemin.display()),
        Err(err) => crate::diag!("[notes] export : « {} » N'A PAS PU ÊTRE ÉCRIT ({err})", chemin.display()),
    }
}

/// Le nom de chaque note dans l'archive — `nom_de_fichier_propose`, rendu UNIQUE
/// (` (2)`, ` (3)`…) sans tenir compte de la casse : Windows non plus.
fn noms_d_archive(notes: &[&Note]) -> Vec<String> {
    let mut pris = std::collections::HashSet::new();
    notes
        .iter()
        .map(|note| {
            let dossier = if note.archived { "archives/" } else { "" };
            let base = nom_de_fichier_propose(note);
            let mut nom = format!("{dossier}{base}");
            let mut rang = 2;
            while !pris.insert(nom.to_lowercase()) {
                nom = format!("{dossier}{base} ({rang})");
                rang += 1;
            }
            nom
        })
        .collect()
}

/// Écrit l'archive ; rend le nombre de notes écrites.
fn ecrire_archive(notes: &[Note], chemin: &std::path::Path) -> std::io::Result<usize> {
    use std::io::Write as _;
    let triees = sorted_for_display(notes);
    let noms = noms_d_archive(&triees);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(chemin)?);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (note, nom) in triees.iter().zip(&noms) {
        let (dossier, tige) = nom.rsplit_once('/').map_or(("", nom.as_str()), |(d, t)| (d, t));
        let prefixe = if dossier.is_empty() { String::new() } else { format!("{dossier}/") };
        let (texte, images) = contenu_d_export(note, tige);
        for (fichier, png) in images {
            zip.start_file(format!("{prefixe}{fichier}"), options).map_err(std::io::Error::other)?;
            zip.write_all(&png)?;
        }
        zip.start_file(format!("{nom}.md"), options).map_err(std::io::Error::other)?;
        zip.write_all(texte.as_bytes())?;
    }
    zip.finish().map_err(std::io::Error::other)?;
    Ok(triees.len())
}

/// Crée une note à partir du contenu (encodé par `encodeURIComponent` côté
/// JS, voir `go()`) — ignorée si vide après retrait des espaces, pour ne pas
/// polluer la liste d'un clic accidentel sur "Ajouter" avec la zone de
/// saisie vide. Toujours du texte seul (sans image) : une image collée passe
/// par `handle_paste`, qui crée directement la note avec son image.
fn handle_create(notes: &mut Vec<Note>, content: &str) -> bool {
    let content = percent_decode(content);
    if content.trim().is_empty() {
        return false;
    }
    let id = next_id(notes);
    notes.push(Note::new(id, content));
    true
}

/// N'interdit un contenu vide après retrait des espaces QUE si la note n'a
/// aucune image (voir `Note::content_and_images_are_empty`) — une note
/// illustrée peut voir sa légende entièrement effacée sans devenir invalide
/// pour autant, contrairement à avant l'ajout des images (où tout contenu
/// vide était refusé).
///
/// **ELLE ENGAGE AUSSI LE RAPPEL** (`quand`) : le texte et le rappel sont
/// enregistrés ENSEMBLE, par le même geste — DEMANDE EXPLICITE (2026-09-24) :
/// « que le rappel soit pris en compte seulement quand j'enregistre la note ».
/// Avant, un champ de rappel envoyait une action à CHAQUE frappe, ce qui
/// reconstruisait la liste en pleine saisie et éjectait l'utilisateur du champ.
///
/// `quand` : `aucune` (pas de rappel), ou `"<date>_<heure>"` avec `aucun` pour
/// une moitié vide — le format d'`analyser_quand`, qui porte aussi le `maintenant`
/// d'autrefois (`aucun_aucun` = « sonne dès l'enregistrement »).
fn handle_update(notes: &mut [Note], id: &str, content: &str, quand: &str) -> bool {
    let Ok(id) = id.parse::<u64>() else { return false };
    let Some(note) = notes.iter_mut().find(|n| n.id == id) else { return false };
    let content = percent_decode(content);
    let alarme = match quand {
        "aucune" => None,
        quand => match analyser_quand(quand) {
            Some(alarme) => Some(alarme),
            // Forme illisible : on ne DEVINE pas, on ne touche à rien — donc pas
            // même le texte, qui est pourtant valide. Refuser les deux vaut mieux
            // que d'enregistrer la moitié de ce que l'utilisateur a demandé.
            None => return false,
        },
    };
    if Note::content_and_images_are_empty(&content, &note.images) {
        return false;
    }
    note.content = crate::win32::notes::truncate_chars(&content, MAX_NOTE_CHARS);
    note.alarme = alarme;
    note.updated_at = unix_now();
    true
}

fn handle_delete(notes: &mut Vec<Note>, id: &str) -> bool {
    let Ok(id) = id.parse::<u64>() else { return false };
    let before = notes.len();
    notes.retain(|n| n.id != id);
    notes.len() != before
}

/// **IMPORTE UNE NOTE DEPUIS UN FICHIER** — le pendant de `handle_exporter_md`, et
/// même dialogue que lui (`win32::file_dialog::ouvrir`).
///
/// ⛔ **LES IMAGES REVIENNENT AVEC LE TEXTE, ET C'EST LE POINT.** Notre export les
/// écrit en `<nom>-image-N.png` à côté du `.md` ; les importer à leur tour rend le
/// aller-retour COMPLET. Ne ramener que le texte aurait laissé les images sur le
/// disque, et la note importée aurait porté des références vers des fichiers que
/// personne ne saurait retrouver — un demi-import silencieux, exactement ce que
/// l'export a été écrit pour éviter.
///
/// ⚠️ **LA LIGNE DE RÉFÉRENCE DISPARAÎT QUAND L'IMAGE EST IMPORTÉE** : elle est
/// remplacée par l'image elle-même. La laisser aurait affiché, dans la note, un
/// `![nom](nom)` en clair À CÔTÉ de l'image qu'il désigne.
fn handle_importer(notes: &mut Vec<Note>) -> bool {
    let Some(proprietaire) = cef_popup::hwnd(&SLOT) else { return false };
    let Some(chemin) = crate::win32::file_dialog::ouvrir(
        proprietaire,
        "Importer une note",
        "md",
        "Markdown ou texte",
    ) else {
        // Annulé : pas une erreur, rien au journal (voir `enregistrer_sous`).
        return false;
    };
    let Ok(texte) = std::fs::read_to_string(&chemin) else {
        crate::diag!("[notes] import : « {} » ILLISIBLE", chemin.display());
        return false;
    };
    let dossier = chemin.parent().unwrap_or_else(|| std::path::Path::new("."));
    let mut images = Vec::new();
    let mut sans_images = String::new();
    for ligne in texte.lines() {
        let Some(reference) = reference_d_image(ligne) else {
            sans_images.push_str(ligne);
            sans_images.push('\n');
            continue;
        };
        let dechiffre = std::fs::read(dossier.join(&reference))
            .ok()
            .and_then(|octets| crate::win32::clipboard::decode_png(&octets))
            .and_then(|(largeur, hauteur, rgba)| config::store_note_image(largeur, hauteur, &rgba));
        match dechiffre {
            Some(image) => images.push(image),
            None => {
                // ⚠️ **JAMAIS EN SILENCE** : une référence qu'on n'a pas su importer
                // reste DANS le texte, pour que l'utilisateur voie ce qui manque
                // plutôt que de le découvrir plus tard.
                crate::diag!("[notes] import : image « {reference} » non importée (absente ou illisible)");
                sans_images.push_str(ligne);
                sans_images.push('\n');
            }
        }
    }
    let contenu = sans_images.trim().to_string();
    if contenu.is_empty() && images.is_empty() {
        crate::diag!("[notes] import : « {} » ne contient rien d'importable", chemin.display());
        return false;
    }
    let id = next_id(notes);
    notes.push(Note::new_with_images(id, contenu, images));
    crate::diag!("[notes] import : « {} » importé", chemin.display());
    true
}

/// La cible d'une référence d'image Markdown `![alt](chemin)` — `None` si la ligne
/// n'en est pas une.
///
/// **Volontairement minimal, et pas une expression régulière** : on ne cherche QUE
/// ce que notre propre export écrit. Une ligne qui n'y ressemble pas est du texte,
/// et lui faire subir une analyse plus fine risquerait de manger du contenu réel.
fn reference_d_image(ligne: &str) -> Option<String> {
    let reste = ligne.trim().strip_prefix("![")?;
    let (_, apres) = reste.split_once("](")?;
    let (cible, _) = apres.split_once(')')?;
    (!cible.is_empty()).then(|| cible.to_string())
}

/// **EXPORTE UNE NOTE EN MARKDOWN** — DEMANDE EXPLICITE (2026-09-24) : « une vraie
/// fenêtre Enregistrer sous ». Le dialogue lui-même est dans
/// `win32::file_dialog`, qui porte le pourquoi de `IFileSaveDialog`.
///
/// **CE QUI EST ÉCRIT DOIT SUFFIRE À REPRENDRE LA NOTE AILLEURS** : le texte, puis
/// les images (copiées à côté, en `<nom>-image-N.png`), puis une ligne qui dit les
/// étiquettes et le rappel. Un export qui laisserait les images derrière lui
/// serait un demi-export — ce sont des fichiers séparés dans `notes_images/`, et
/// personne ne saurait les retrouver.
///
/// Rend toujours `false` : exporter ne MODIFIE pas les notes, donc il n'y a rien à
/// enregistrer ni à pousser. Rendre `true` ferait repousser l'état pour rien.
/// **CE QU'UN CLIC SUR UN LIEN A LE DROIT D'OUVRIR** — une adresse web, rien
/// d'autre.
///
/// ⛔ `ShellExecuteW` ouvre TOUT ce qu'on lui donne : un chemin d'exécutable, un
/// `file:`, un protocole d'application. Le texte d'une note peut venir d'un
/// collage ou d'un import ; seuls `http://` et `https://` passent donc, et
/// `www.` reçoit `https://`. Pas d'espace ni de caractère de contrôle : la page
/// découpe le lien sur les espaces, un lien qui en porte n'est pas le sien.
fn lien_ouvrable(lien: &str) -> Option<String> {
    if lien.is_empty() || lien.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let minuscule = lien.to_ascii_lowercase();
    let complet = if minuscule.starts_with("www.") { format!("https://{lien}") } else { lien.to_string() };
    let minuscule = complet.to_ascii_lowercase();
    let reste = minuscule.strip_prefix("https://").or_else(|| minuscule.strip_prefix("http://"))?;
    (!reste.is_empty() && !reste.starts_with('/')).then_some(complet)
}

fn handle_exporter_md(notes: &[Note], id: &str) -> bool {
    let Ok(id) = id.parse::<u64>() else { return false };
    let Some(note) = notes.iter().find(|n| n.id == id) else { return false };
    let Some(proprietaire) = cef_popup::hwnd(&SLOT) else { return false };
    let Some(chemin) = crate::win32::file_dialog::enregistrer_sous(
        proprietaire,
        "Exporter la note",
        &nom_de_fichier_propose(note),
        "md",
        "Markdown",
    ) else {
        // ⚠️ **ANNULÉ, ET CE N'EST PAS UNE ERREUR** : ouvrir une boîte et se
        // raviser est un geste normal. Rien au journal, rien à l'écran.
        return false;
    };
    match ecrire_export(note, &chemin) {
        Ok(()) => crate::diag!("[notes] export : « {} » écrit", chemin.display()),
        // Jamais en silence — même règle que partout ailleurs. Le chemin est
        // dans le message : c'est là que l'utilisateur doit regarder.
        Err(err) => crate::diag!("[notes] export : « {} » N'A PAS PU ÊTRE ÉCRIT ({err})", chemin.display()),
    }
    false
}

/// **LE NOM DE FICHIER PROPOSÉ** — la première ligne non vide de la note, réduite à
/// ce qu'un nom de fichier Windows accepte.
///
/// ⚠️ Les caractères interdits deviennent des `-` plutôt que d'être retirés :
/// retirer ferait coller les morceaux (`site/Pierre` → `sitePierre`), alors qu'un
/// tiret garde lisible ce que l'utilisateur a écrit.
fn nom_de_fichier_propose(note: &Note) -> String {
    let brut = note.content.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let nettoye: String = brut
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '-' } else { c })
        .take(60)
        .collect();
    // Un nom qui finit par un point est refusé par Windows, et un nom vide n'en
    // est pas un.
    let nettoye = nettoye.trim().trim_matches('.').to_string();
    if nettoye.is_empty() { format!("note-{}", note.id) } else { nettoye }
}

/// Écrit le fichier Markdown et les images de la note, à côté de lui.
fn ecrire_export(note: &Note, chemin: &std::path::Path) -> std::io::Result<()> {
    let dossier = chemin.parent().unwrap_or_else(|| std::path::Path::new("."));
    let tige = chemin.file_stem().and_then(|s| s.to_str()).unwrap_or("note").to_string();
    let (texte, images) = contenu_d_export(note, &tige);
    for (nom, png) in images {
        std::fs::write(dossier.join(nom), png)?;
    }
    std::fs::write(chemin, texte)
}

/// **CE QU'ÉCRIT L'EXPORT D'UNE NOTE** — le Markdown, et les images à poser à côté
/// (`<tige>-image-N.png`, nom → octets PNG). Partagé par l'export d'une note et
/// celui de toutes (voir `ecrire_archive`), pour que l'archive contienne
/// exactement ce que « Importer » sait relire.
fn contenu_d_export(note: &Note, tige: &str) -> (String, Vec<(String, Vec<u8>)>) {
    let mut texte = String::new();
    let mut fichiers = Vec::new();
    texte.push_str(&note.content);
    texte.push('\n');
    // Les images APRÈS le texte, dans l'ordre de la note — et copiées, pas
    // référencées : le fichier exporté doit tenir tout seul.
    for (rang, image) in note.images.iter().enumerate() {
        let nom = format!("{tige}-image-{}.png", rang + 1);
        if let Some(png) = config::load_note_image_png(&image.file) {
            texte.push_str(&format!("\n![{nom}]({nom})\n"));
            fichiers.push((nom, png));
        }
    }
    // Les métadonnées en fin, en une ligne discrète : ce que la note portait et
    // qu'un simple copier-coller du texte aurait perdu.
    let etiquettes = extract_tags(&note.content);
    if !etiquettes.is_empty() || note.alarme.is_some() {
        let mut details = Vec::new();
        if !etiquettes.is_empty() {
            details.push(format!("étiquettes : {}", etiquettes.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(", ")));
        }
        if let Some(alarme) = &note.alarme {
            details.push(format!("rappel : {}", resume_alarme(alarme)));
        }
        texte.push_str(&format!("\n---\n\n_{}_\n", details.join(" · ")));
    }
    (texte, fichiers)
}

/// Le rappel d'une note en quelques mots, pour l'export — même vocabulaire que
/// l'indicateur du panneau, pour que les deux se lisent pareil.
fn resume_alarme(alarme: &notes::AlarmeNote) -> String {
    let mut parties = Vec::new();
    if let Some((a, m, j)) = alarme.date {
        parties.push(format!("{j:02}/{m:02}/{a:04}"));
    }
    if let Some((h, min)) = alarme.heure {
        parties.push(format!("{h:02}:{min:02}"));
    }
    match alarme.jours {
        0 => {}
        notes::TOUS_LES_JOURS => parties.push("tous les jours".to_string()),
        masque => parties.push(
            (0..7).filter(|i| masque & (1 << i) != 0).map(|i| notes::JOURS_SEMAINE[i]).collect::<Vec<_>>().join(", "),
        ),
    }
    if parties.is_empty() { "tout de suite".to_string() } else { parties.join(" ") }
}

/// **ACQUITTE LE RAPPEL D'UNE NOTE** — DEMANDE EXPLICITE (2026-09-24) : « un
/// bouton par note, dans le panneau ».
///
/// ⚠️ **UN ACQUITTEMENT PAR NOTE, ET NON UNE EMPREINTE DU JOUR** comme la cloche :
/// acquitter celle-ci ne doit pas faire taire celle-là. C'est
/// `fetes::acquitter_rappel_note` qui tient l'ensemble, ici on ne fait que
/// l'écrire.
///
/// ⚠️ **LE RAPPEL RESTE POSÉ** — exactement comme celui de la cloche : il fait
/// taire la sonnerie POUR LA JOURNÉE, et **revient le lendemain**. Pour l'arrêter
/// pour de bon, il faut retirer l'alarme de la note.
///
/// Rend `false` quand il n'y a rien à faire (pas d'alarme, ou déjà acquitté) :
/// inutile de réécrire le fichier et de repousser l'état pour rien.
fn handle_acquitter(notes: &[Note], id: &str) -> bool {
    let Ok(id) = id.parse::<u64>() else { return false };
    let Some(note) = notes.iter().find(|n| n.id == id) else { return false };
    let Some(identite) = crate::win32::fetes::identite_du_rappel_de_note(note) else { return false };
    let (jour, _) = jour_et_heure_locaux();
    let avant = crate::win32::fetes::rappels_notes_acquittes();
    let apres = crate::win32::fetes::acquitter_rappel_note(&avant, jour, &identite);
    if apres == avant {
        return false;
    }
    crate::win32::fetes::set_rappels_notes_acquittes(&apres);
    config::save_rappel_ack_notes(&apres);
    true
}

/// **ANALYSE LE CHAMP `quand` D'UNE ACTION D'ALARME** — `"<date>_<heure>"`, chaque
/// moitié pouvant valoir `aucun`.
///
/// Exemples : `aucun_aucun` (tout de suite), `aucun_15:30` (aujourd'hui à 15 h 30),
/// `2026-09-25_aucun` (le 25 au matin), `2026-09-25_15:30` (les deux).
///
/// ⚠️ **UN SEUL SEGMENT POUR DEUX VALEURS, ET `_` COMME SÉPARATEUR** : la date
/// contient des `-` et l'heure un `:`, donc aucun des deux ne peut porter un `_` —
/// et surtout, un second segment d'URL aurait ramené le piège d'arité déjà
/// documenté (`on_before_browse` mange les segments VIDES en fin d'URL, pas ceux
/// du milieu, mais il faut alors deux conventions au lieu d'une).
///
/// ⚠️ **`aucun` ET NON UN SEGMENT VIDE** : même raison — un champ vide en fin
/// d'action disparaîtrait.
///
/// ⛔ **LE SEGMENT ARRIVE ENCODÉ, ET C'EST ICI QU'ON LE DÉCODE** — c'est la
/// discipline documentée de `cef_app::on_before_browse` : la page envoie
/// `encodeURIComponent(quand)` (voir `go()` dans `notes.html`), donc l'heure
/// voyage en `14%3A30`. Or `alarme` était le SEUL verbe du dispatch à ne pas
/// décoder son segment (`handle_create`, `handle_update` et `handle_paste` le
/// faisaient tous) : `analyser_heure` cherchait un `:` dans `14%3A30`, ne le
/// trouvait pas, et `handle_alarme` rendait `false` — donc **une heure n'a
/// jamais pu s'enregistrer**, en silence, depuis que le champ existe.
///
/// ⚠️ **APRÈS LE `split_once`, PAS AVANT** : décoder d'abord serait le piège
/// inverse, celui que `on_before_browse` décrit — un `%5F` deviendrait un
/// séparateur indiscernable d'un vrai. `encodeURIComponent` n'échappe JAMAIS
/// `_`, donc scinder la chaîne encore encodée est fidèle.
///
/// Un troisième segment facultatif porte la répétition : le masque de
/// `AlarmeNote::jours` en décimal (`aucun_08:00_31` = du lundi au vendredi).
/// Absent, l'alarme sonne une fois.
fn analyser_quand(quand: &str) -> Option<notes::AlarmeNote> {
    let mut morceaux = quand.splitn(3, '_');
    let date = percent_decode(morceaux.next()?);
    let heure = percent_decode(morceaux.next()?);
    let jours = match morceaux.next() {
        Some(texte) => texte.parse::<u8>().ok().filter(|j| *j <= notes::TOUS_LES_JOURS)?,
        None => 0,
    };
    Some(notes::AlarmeNote {
        jours,
        date: match date.as_str() {
            "aucun" => None,
            texte => Some(analyser_date(texte)?),
        },
        heure: match heure.as_str() {
            "aucun" => None,
            texte => Some(analyser_heure(texte)?),
        },
    })
}

/// `"AAAA-MM-JJ"` → `(annee, mois, jour)`.
fn analyser_date(texte: &str) -> Option<(i32, u32, u32)> {
    let mut parts = texte.split('-');
    let annee: i32 = parts.next()?.parse().ok()?;
    let mois: u32 = parts.next()?.parse().ok()?;
    let jour: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&mois) || !(1..=31).contains(&jour) {
        return None;
    }
    Some((annee, mois, jour))
}

/// `"HH:MM"` → `(heure, minute)`.
fn analyser_heure(texte: &str) -> Option<(u32, u32)> {
    let (h, m) = texte.split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    ((0..=23).contains(&h) && (0..=59).contains(&m)).then_some((h, m))
}

/// Épingler/désépingler NE modifie PAS `updated_at` (voir `win32::notes::
/// Note::updated_at`, doc de champ) : ce n'est pas une édition du contenu.
///
/// ⚠️ Ce commentaire décrivait déjà `toggle-pin` avant, mais il était ACCROCHÉ à
/// l'ancien verbe d'alarme — supprimé le 2026-09-24 avec le passage du rappel
/// dans l'édition de la note. Le voici à sa place.
fn handle_toggle_pin(notes: &mut [Note], id: &str) -> bool {
    let Ok(id) = id.parse::<u64>() else { return false };
    let Some(note) = notes.iter_mut().find(|n| n.id == id) else { return false };
    note.pinned = !note.pinned;
    true
}

/// Colle le contenu ACTUEL du presse-papiers Windows — déclenché par un clic
/// sur le bouton "Coller" du compositeur (voir `assets/popups/notes.html`),
/// PAS uniquement par un raccourci clavier : c'était explicitement la
/// demande (pouvoir coller à la souris, sans dépendre de Ctrl+V). Lit le
/// presse-papiers RÉEL (voir `win32::clipboard::read`) plutôt que l'API
/// Clipboard du navigateur intégré à CEF — évite les permissions/l'API
/// asynchrone de cette dernière, inutiles pour une fenêtre native mono-
/// utilisateur qui a de toute façon un accès direct au presse-papiers
/// Windows. Contrairement à `win32::cef_clipboard::handle_paste`, ne
/// réinjecte JAMAIS l'entrée dans une fenêtre étrangère via `SendInput` :
/// voir doc de module.
///
/// `editing_id` est l'`id` de la note EN COURS D'ÉDITION côté JS (`""` si
/// aucune, voir `editingId` dans `notes.html`) — demande explicite : coller
/// pendant l'édition d'une note doit enrichir CETTE note, pas en créer une
/// nouvelle. Si `editing_id` désigne une note existante, délègue entièrement
/// à `handle_paste_into_active` et ignore `compose` (sans rapport avec une
/// note déjà existante). Sinon (pas d'édition en cours, ou `editing_id`
/// obsolète), retombe sur le comportement historique ci-dessous, où
/// `compose` est le contenu ACTUEL de la zone de composition (voir `go()`
/// dans `notes.html`) — utilisé uniquement pour le cas image.
///
/// - Texte : inséré dans la zone de composition (voir `window.
///   insertIntoCompose`) plutôt que de créer une note directement — un
///   collage de texte n'est pas en soi une validation, la note se crée
///   toujours via "Ajouter" comme d'habitude (voir `handle_create`). Ne
///   modifie donc jamais `notes` (renvoie toujours `false`).
/// - Image : crée IMMÉDIATEMENT une note avec `compose` comme légende
///   (éventuellement vide) et l'image collée (voir `config::
///   store_note_image`), puis vide la zone de composition (voir `window.
///   clearCompose`) puisque son contenu vient d'être "consommé" par la note
///   fraîchement créée. Si `store_note_image` échoue (disque inaccessible),
///   AUCUNE note n'est créée — y compris si `compose` n'est pas vide : créer
///   une note texte à la place de l'image demandée serait une perte
///   silencieuse (l'image collée disparaîtrait sans que rien ne le signale),
///   et vider la zone de composition ferait perdre la légende déjà tapée en
///   plus de l'image. La zone de composition reste donc intacte, permettant
///   de réessayer.
fn handle_paste(notes: &mut Vec<Note>, editing_id: &str, compose: &str) -> bool {
    if let Ok(id) = editing_id.parse::<u64>() {
        if let Some(note) = notes.iter_mut().find(|n| n.id == id) {
            return handle_paste_into_active(note);
        }
    }
    let compose = percent_decode(compose);
    match clipboard::read() {
        Some(ClipboardEntry::Text(text)) => {
            cef_popup::execute_js(&SLOT, &format!("window.insertIntoCompose({})", json!(text)));
            false
        }
        Some(entree @ ClipboardEntry::Image { .. }) => {
            // ⚠️ **LE RGBA PASSE PAR L'ACCESSEUR, PAS PAR LE CHAMP** : il peut
            // avoir été relâché depuis (voir `ClipboardEntry`), et l'accès
            // direct ne compilerait même plus. Ici l'entrée vient de
            // `clipboard::read`, donc il est en mémoire — mais l'appelant n'a
            // pas à le savoir.
            let (Some((width, height)), Some(rgba)) = (entree.dimensions(), entree.rgba()) else {
                return false;
            };
            let Some(image) = config::store_note_image(width, height, &rgba) else {
                return false;
            };
            let id = next_id(notes);
            notes.push(Note::new_with_images(id, compose, vec![image]));
            cef_popup::execute_js(&SLOT, "window.clearCompose()");
            true
        }
        None => false,
    }
}

/// Colle le presse-papiers ACTUEL dans `note`, EN COURS D'ÉDITION — voir
/// `handle_paste`, seul appelant, qui a déjà retrouvé `note` dans `notes`
/// avant d'appeler cette fonction (évite un second parcours `O(n)` en plus
/// de celui déjà fait pour vérifier l'existence de `editing_id`).
/// Contrairement à `handle_paste` (zone de composition, ne peut que CRÉER une
/// note), enrichit toujours une note déjà existante :
/// - Texte : inséré dans la zone d'ÉDITION (voir `window.
///   insertIntoEditArea`, pendant côté JS de `window.insertIntoCompose`) —
///   `editDraft` reste un brouillon tant que "Enregistrer" n'a pas été
///   cliqué (voir `saveEdit`/`handle_update`), donc `notes` n'est pas
///   modifié ici (renvoie toujours `false`), exactement comme le cas texte
///   de `handle_paste`.
/// - Image : attachée immédiatement à la note (voir `attach_clipboard_
///   image`) — contrairement au texte, les images d'une note n'ont pas de
///   brouillon séparé (voir `handle_remove_image`, qui modifie aussi `notes`
///   sans passer par "Enregistrer"), donc coller une image prend effet tout
///   de suite même en cours d'édition.
fn handle_paste_into_active(note: &mut Note) -> bool {
    match clipboard::read() {
        Some(ClipboardEntry::Text(text)) => {
            cef_popup::execute_js(&SLOT, &format!("window.insertIntoEditArea({})", json!(text)));
            false
        }
        Some(entree) => {
            let (Some((width, height)), Some(rgba)) = (entree.dimensions(), entree.rgba()) else {
                return false;
            };
            attach_clipboard_image(note, width, height, &rgba)
        }
        None => false,
    }
}

/// Colle une IMAGE du presse-papiers directement dans une note EXISTANTE
/// (bouton dédié sur chaque carte, voir `assets/popups/notes.html`) —
/// contrairement à `handle_paste` (zone de composition), qui ne peut que
/// CRÉER une nouvelle note, jamais en enrichir une déjà existante. Ignore
/// silencieusement (renvoie `false`, sans effet) : si le presse-papiers ne
/// contient pas d'image (le collage de texte n'a de sens que dans la zone de
/// composition, voir `handle_paste`), si `id` est inconnu, ou si `attach_
/// clipboard_image` refuse. La note est recherchée AVANT de lire l'image :
/// un `id` obsolète/invalide évite ainsi d'écrire inutilement l'image collée
/// sur disque (elle serait sinon orpheline jusqu'au prochain `save_notes`,
/// voir `notes::referenced_image_files`).
fn handle_paste_into(notes: &mut [Note], id: &str) -> bool {
    let Ok(id) = id.parse::<u64>() else { return false };
    let Some(note) = notes.iter_mut().find(|n| n.id == id) else { return false };
    let Some(entree) = clipboard::read() else { return false };
    let (Some((width, height)), Some(rgba)) = (entree.dimensions(), entree.rgba()) else {
        return false;
    };
    attach_clipboard_image(note, width, height, &rgba)
}

/// Écrit `rgba` sur disque (voir `config::store_note_image`) puis l'attache
/// à `note` (voir `Note::push_image`) — factorisé entre `handle_paste_into`
/// et `handle_paste_into_active`, seuls appelants, qui ne diffèrent que par
/// la façon dont ils trouvent `note` et gèrent le cas texte. `false` si
/// `store_note_image` échoue (disque inaccessible, `note` intact) ou si
/// `push_image` refuse (déjà `MAX_NOTE_IMAGES` images sur cette note, voir
/// sa doc) — dans ce dernier cas `note` reste intact lui aussi, mais PAS
/// "sans effet" au sens strict : le fichier PNG a déjà été écrit sur disque
/// par `store_note_image` avant l'échec de `push_image`, et reste orphelin
/// jusqu'au prochain `save_notes` (voir `notes::referenced_image_files`, qui
/// ne le référence plus puisque `note` n'a pas changé).
fn attach_clipboard_image(note: &mut Note, width: u32, height: u32, rgba: &[u8]) -> bool {
    let Some(image) = config::store_note_image(width, height, rgba) else { return false };
    if !note.push_image(image) {
        return false;
    }
    note.updated_at = unix_now();
    true
}

/// Retire l'image à `index` de la note `id` (voir `Note::remove_image_at`) —
/// bornes déjà vérifiées là-bas, `false` sans effet si `id`/`index` sont mal
/// formés ou obsolètes. Compte comme une modification du contenu (met à jour
/// `updated_at`, contrairement à `handle_toggle_pin`) : retirer une pièce
/// jointe change ce que la note représente, comme éditer son texte. Ne
/// supprime PAS automatiquement une note devenue entièrement vide (texte ET
/// images) : laissée telle quelle, à supprimer explicitement par
/// l'utilisateur (voir `handle_delete`) — auto-supprimer un contenu que
/// l'utilisateur n'a pas explicitement demandé de supprimer surprendrait.
fn handle_remove_image(notes: &mut [Note], id: &str, index: &str) -> bool {
    let Ok(id) = id.parse::<u64>() else { return false };
    let Ok(index) = index.parse::<usize>() else { return false };
    let Some(note) = notes.iter_mut().find(|n| n.id == id) else { return false };
    if !note.remove_image_at(index) {
        return false;
    }
    note.updated_at = unix_now();
    true
}

/// **TOURNER UNE IMAGE DE NOTE D'UN QUART DE TOUR** (demande 2026-11-08) — le
/// PNG est relu, tourné (`clipboard::pivoter_rgba`), puis ÉCRIT COMME UNE
/// NOUVELLE IMAGE (`config::store_note_image`, miniature comprise) qui prend la
/// place de l'ancienne. L'ancien fichier n'est plus référencé : le prochain
/// `save_notes` le nettoie (`notes::referenced_image_files`) — et annuler rend
/// l'instantané d'avant, qui le désigne encore. `false` sans effet si quoi que
/// ce soit manque (note, image, fichier) ou si `sens` est inconnu.
fn handle_pivoter_image(notes: &mut [Note], id: &str, index: &str, sens: &str) -> bool {
    let (Ok(id), Ok(index)) = (id.parse::<u64>(), index.parse::<usize>()) else { return false };
    let horaire = match sens {
        "droite" => true,
        "gauche" => false,
        autre => {
            crate::diag!("[notes] sens de rotation inconnu : {autre:?}");
            return false;
        }
    };
    let Some(note) = notes.iter_mut().find(|n| n.id == id) else { return false };
    let Some(image) = note.images.get(index) else { return false };
    let tournee = config::load_note_image_png(&image.file)
        .and_then(|png| crate::win32::clipboard::decode_png(&png))
        .and_then(|(l, h, rgba)| crate::win32::clipboard::pivoter_rgba(l, h, &rgba, horaire))
        .and_then(|(l, h, rgba)| config::store_note_image(l, h, &rgba));
    let Some(tournee) = tournee else {
        crate::win32::bandeau::montrer("Notes", "Cette image n'a pas pu être tournée (fichier illisible ?).");
        return false;
    };
    note.images[index] = tournee;
    note.updated_at = unix_now();
    true
}

/// Charge la pleine résolution d'une image de note À LA DEMANDE (clic pour
/// agrandir, voir `assets/popups/notes.html`) — voir `config::
/// load_note_image_png`, jamais chargée en même temps que le reste de l'état
/// (voir sa doc). Pousse le résultat via `window.setFullImage` plutôt que de
/// renvoyer une valeur directement : l'IPC JS→Rust de ce projet n'est qu'une
/// navigation interceptée (voir doc de module de `win32::cef_app`), sans
/// mécanisme de réponse synchrone — Rust->JS se fait toujours par un
/// `execute_js` séparé, initié ici plutôt qu'en retour de `dispatch_actions`.
/// Ne modifie jamais `notes` : n'affecte donc pas `changed` dans l'appelant.
fn handle_full_image(notes: &[Note], id: &str, index: &str) {
    let Ok(id) = id.parse::<u64>() else { return };
    let Ok(index) = index.parse::<usize>() else { return };
    let Some(note) = notes.iter().find(|n| n.id == id) else { return };
    let Some(image) = note.images.get(index) else { return };
    let Some(bytes) = config::load_note_image_png(&image.file) else { return };
    let data_uri = format!("data:image/png;base64,{}", BASE64.encode(&bytes));
    cef_popup::execute_js(&SLOT, &format!("window.setFullImage({}, {}, {})", id, index, json!(data_uri)));
}

/// **RECONNAÎTRE LE TEXTE D'UNE IMAGE DE NOTE** (demande 2026-11-04) — même
/// chemin que `handle_full_image` pour la lecture du fichier
/// (`config::load_note_image_png`), puis la reconnaissance avec le moteur
/// CHOISI par la page (`"tesseract"` = NAPS2, `"windows"` = Copilot).
///
/// Le texte reconnu part au presse-papiers avec un bandeau (voir
/// `win32::texte_capture::poll`) : la note, elle, n'est pas modifiée — une
/// reconnaissance n'est pas une écriture, et l'utilisateur décide où va le texte.
fn handle_texte(notes: &[Note], id: &str, index: &str, moteur: &str) {
    let (Ok(id), Ok(index)) = (id.parse::<u64>(), index.parse::<usize>()) else { return };
    let Some(note) = notes.iter().find(|n| n.id == id) else { return };
    let Some(image) = note.images.get(index) else { return };
    let Some(png) = config::load_note_image_png(&image.file) else {
        crate::win32::bandeau::montrer(
            crate::win32::texte_capture::TITRE_PANNEAU,
            "L'image de cette note n'a pas pu être relue.",
        );
        return;
    };
    crate::win32::texte_capture::lancer_choisi(moteur, &png);
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

/// Voir `win32::cef_clipboard::render_if_visible` — même raison (suit
/// `app::dash_rect`, peut changer pendant que la fenêtre est ouverte).
pub fn render_if_visible(rect: [i32; 4], tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::resize_if_needed(&SLOT, rect);
    // L'accent voyage avec la palette, comme pour les trois autres panneaux —
    // il n'est plus mémorisé ici (voir la fin de `thread_local!`).
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
    /// **LE VERBE DE LA RECONNAISSANCE DOIT EXISTER DES DEUX CÔTÉS**, et porter
    /// le nom TECHNIQUE du moteur (voir `cef_clipboard`, même test, même raison).
    #[test]
    fn la_page_et_le_dispatch_connaissent_le_verbe_texte() {
        let page = include_str!("../../assets/popups/notes.html");
        // ⛔ Découpé AVANT les modules de test : `include_str!` du fichier entier
        // ferait correspondre les motifs ci-dessous au test LUI-MÊME.
        let source = include_str!("cef_notes.rs").split("#[cfg(test)]").next().unwrap_or_default();
        assert!(page.contains("go(\"texte\""), "la page n'émet pas le verbe texte");
        assert!(source.contains("[\"texte\", id, index, moteur]"), "le dispatch ne traite pas texte");
        for (moteur, nom, _libelle) in crate::win32::ocr::Moteur::TOUS {
            assert!(page.contains(&format!("go(\"texte\", note.id, index, \"{nom}\")")), "la page n'offre pas {moteur:?} ({nom})");
        }
    }

    /// Les deux sens de rotation existent des deux côtés (même raison).
    #[test]
    fn la_page_et_le_dispatch_connaissent_le_verbe_pivoter_image() {
        let page = include_str!("../../assets/popups/notes.html");
        let source = include_str!("cef_notes.rs").split("#[cfg(test)]").next().unwrap_or_default();
        assert!(source.contains("[\"pivoter-image\", id, index, sens]"), "le dispatch ne traite pas pivoter-image");
        for sens in ["gauche", "droite"] {
            assert!(page.contains(&format!("go(\"pivoter-image\", note.id, index, \"{sens}\")")), "la page n'offre pas {sens}");
        }
    }
}

#[cfg(test)]
mod tests_liens {
    use super::lien_ouvrable;

    #[test]
    fn seules_les_adresses_web_s_ouvrent() {
        assert_eq!(lien_ouvrable("https://exemple.fr/page?a=1").as_deref(), Some("https://exemple.fr/page?a=1"));
        assert_eq!(lien_ouvrable("HTTP://exemple.fr").as_deref(), Some("HTTP://exemple.fr"));
        assert_eq!(lien_ouvrable("www.exemple.fr").as_deref(), Some("https://www.exemple.fr"));
        for refuse in [
            "file:///C:/Windows/System32/cmd.exe",
            r"C:\Windows\System32\cmd.exe",
            "ms-settings:network",
            "javascript:alert(1)",
            "https://",
            "https:///etc",
            "https://exemple.fr/a b",
            "",
        ] {
            assert_eq!(lien_ouvrable(refuse), None, "{refuse:?}");
        }
    }
}

#[cfg(test)]
mod memos_tests {
    use super::*;

    fn note(id: u64, texte: &str) -> Note {
        let mut n = Note::new(id, texte.to_string());
        n.updated_at = 1000;
        n
    }

    /// Cocher écrit `[x]` dans le texte SANS toucher `updated_at` : sinon chaque
    /// case cochée ferait remonter la note en tête de liste.
    #[test]
    fn cocher_n_ecrit_que_la_case() {
        let mut notes = vec![note(1, "- [ ] pain\n- [ ] lait")];
        assert!(handle_cocher(&mut notes, "1", "1"));
        assert_eq!(notes[0].content, "- [ ] pain\n- [x] lait");
        assert_eq!(notes[0].updated_at, 1000);
        assert!(!handle_cocher(&mut notes, "1", "7"), "pas de huitième tâche");
        assert!(!handle_cocher(&mut notes, "2", "0"), "pas de note 2");
    }

    #[test]
    fn archiver_bascule_sans_toucher_la_date() {
        let mut notes = vec![note(1, "a")];
        assert!(handle_archiver(&mut notes, "1"));
        assert!(notes[0].archived);
        assert!(handle_archiver(&mut notes, "1"));
        assert!(!notes[0].archived);
        assert_eq!(notes[0].updated_at, 1000);
    }

    /// Deux notes de même première ligne ne s'écrasent pas dans l'archive — même
    /// à la casse près, comme sous Windows — et une archivée va dans `archives/`.
    #[test]
    fn les_noms_d_archive_sont_uniques() {
        let mut rangee = note(3, "Courses");
        rangee.archived = true;
        let notes = [note(1, "Courses"), note(2, "courses\nautre"), rangee];
        let refs: Vec<&Note> = notes.iter().collect();
        assert_eq!(noms_d_archive(&refs), ["Courses", "courses (2)", "archives/Courses"]);
    }

    /// L'archive se relit : un `.md` par note, avec le texte de la note.
    #[test]
    fn l_archive_contient_chaque_note() {
        let chemin = std::env::temp_dir().join(format!("neuradesk_test_export_{}.zip", std::process::id()));
        let mut rangee = note(2, "Vieille idée #a/b");
        rangee.archived = true;
        let notes = [note(1, "Première\n- [x] fait"), rangee];
        assert_eq!(ecrire_archive(&notes, &chemin).unwrap(), 2);
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&chemin).unwrap()).unwrap();
        let mut lire = |nom: &str| {
            let mut texte = String::new();
            std::io::Read::read_to_string(&mut zip.by_name(nom).unwrap(), &mut texte).unwrap();
            texte
        };
        assert!(lire("Première.md").starts_with("Première\n- [x] fait\n"));
        assert!(lire("archives/Vieille idée #a-b.md").contains("#a/b"));
        let _ = std::fs::remove_file(&chemin);
    }
}

#[cfg(test)]
mod rappel_avec_le_texte_tests {
    use super::*;

    fn note_vide() -> Vec<Note> {
        vec![Note::new(1, "note".to_string())]
    }

    /// ⛔ **LE SEGMENT ARRIVE ENCODÉ — `go()` FAIT `encodeURIComponent`.**
    ///
    /// C'est le piège documenté de `cef_app::on_before_browse` (« chaque popup
    /// décode le SEUL segment concerné »), et le verbe d'alarme était le seul à
    /// ne pas le faire. Conséquence mesurable : `analyser_heure` cherchait un
    /// `:` dans `14%3A30`, ne le trouvait pas, et l'action rendait `false` —
    /// donc **une heure n'a jamais pu s'enregistrer**, en silence.
    #[test]
    fn une_heure_encodee_par_la_page_est_decodee() {
        let mut notes = note_vide();
        // Exactement ce que `go("update", 1, "note", "2026-09-25_14:30")` met
        // dans l'URL.
        assert!(handle_update(&mut notes, "1", "note", "2026-09-25_14%3A30"));
        let alarme = notes[0].alarme.as_ref().expect("le rappel doit être posé");
        assert_eq!(alarme.date, Some((2026, 9, 25)));
        assert_eq!(alarme.heure, Some((14, 30)));
    }

    /// **LE RAPPEL EST ENGAGÉ PAR LE MÊME GESTE QUE LE TEXTE**, et il le
    /// REMPLACE au lieu de s'y ajouter : `aucune` doit donc retirer un rappel
    /// déjà posé, sans quoi « Enregistrer » ne pourrait jamais en retirer un.
    #[test]
    fn enregistrer_remplace_le_rappel_et_peut_le_retirer() {
        let mut notes = note_vide();
        notes[0].alarme = Some(notes::AlarmeNote { date: Some((2026, 9, 25)), heure: Some((14, 30)), jours: 0 });
        assert!(handle_update(&mut notes, "1", "note", "aucune"));
        assert_eq!(notes[0].alarme, None);
        assert_eq!(notes[0].content, "note");
    }

    /// Le pendant du défaut d'encodage : les formes que `encodeURIComponent` ne
    /// touche PAS (aucun caractère réservé) doivent continuer de se lire.
    #[test]
    fn les_formes_sans_encodage_restent_lues() {
        assert_eq!(analyser_quand("aucun_aucun"), Some(notes::AlarmeNote::tout_de_suite()));
        assert_eq!(
            analyser_quand("2026-09-25_aucun"),
            Some(notes::AlarmeNote { date: Some((2026, 9, 25)), heure: None, jours: 0 })
        );
        assert_eq!(
            analyser_quand("aucun_15:30"),
            Some(notes::AlarmeNote { date: None, heure: Some((15, 30)), jours: 0 })
        );
        assert_eq!(
            analyser_quand("aucun_08%3A00_31"),
            Some(notes::AlarmeNote { date: None, heure: Some((8, 0)), jours: 31 }),
            "du lundi au vendredi"
        );
        assert_eq!(analyser_quand("aucun_aucun_200"), None, "un masque hors des sept jours est refusé");
    }

    /// Une forme illisible ne touche à RIEN — **pas même le TEXTE**, qui était
    /// pourtant valide : enregistrer la moitié de ce qui a été demandé serait
    /// pire que refuser les deux. C'est aussi la règle qui a rendu le défaut
    /// d'encodage invisible si longtemps — on ne devine pas, on ne journalise
    /// pas, on ne modifie pas la note.
    #[test]
    fn une_forme_illisible_n_enregistre_rien_du_tout() {
        let mut notes = note_vide();
        assert!(!handle_update(&mut notes, "1", "texte neuf", "pas-une-date_99:99"));
        assert_eq!(notes[0].content, "note");
    }
}

#[cfg(test)]
mod historique_tests {
    use super::*;

    fn note(id: u64, texte: &str) -> Note {
        Note::new(id, texte.to_string())
    }

    /// **ANNULER REMET L'ÉTAT D'AVANT, REFAIRE LE REMET APRÈS** — et le tour est
    /// complet. Aux deux bouts, `None` plutôt qu'un état inventé : c'est ce qui
    /// grise les boutons, et un bouton qui rendrait « quelque chose » sans rien
    /// pouvoir faire serait pire que grisé.
    #[test]
    fn annuler_puis_refaire_rend_le_meme_etat() {
        let mut h = HistoriqueNotes::default();
        let v1 = vec![note(1, "un")];
        let v2 = vec![note(1, "un"), note(2, "deux")];
        h.enregistrer(v1.clone(), &v2);

        assert!(h.peut_annuler(), "on peut annuler la première action");
        assert!(!h.peut_refaire());
        assert_eq!(h.annuler(), Some(v1.clone()));
        assert!(!h.peut_annuler() && h.peut_refaire());
        assert_eq!(h.refaire(), Some(v2));
        assert!(h.peut_annuler() && !h.peut_refaire());
        assert_eq!(h.refaire(), None, "plus rien à refaire");
        assert_eq!(h.annuler(), Some(v1));
        assert_eq!(h.annuler(), None, "plus rien à annuler");
    }

    /// ⚠️ **UNE ACTION NOUVELLE TRONQUE CE QUI ÉTAIT APRÈS** — sans quoi « refaire »
    /// ramènerait un état qui n'a jamais suivi celui qu'on voit à l'écran.
    #[test]
    fn une_action_apres_un_annuler_tronque_le_refaire() {
        let mut h = HistoriqueNotes::default();
        let v1 = vec![note(1, "un")];
        let v2 = vec![note(2, "deux")];
        let v3 = vec![note(3, "trois")];
        h.enregistrer(v1.clone(), &v2);
        h.enregistrer(v2.clone(), &v3);
        assert_eq!(h.annuler(), Some(v2.clone()));

        // Une action NOUVELLE depuis v2 : v3 n'existe plus.
        let v4 = vec![note(4, "quatre")];
        h.enregistrer(v2.clone(), &v4);
        assert!(!h.peut_refaire(), "ce qui était après a été tronqué");
        assert_eq!(h.annuler(), Some(v2), "on revient à v2, pas à v3");
        assert_eq!(h.annuler(), Some(v1));
    }

    /// **L'HISTORIQUE EST BORNÉ, ET LE CURSEUR SUIT.** Jeter les états les plus
    /// anciens sans décaler le curseur ferait sauter « annuler » sur un état qui
    /// n'est plus là — ou paniquer sur un index hors bornes.
    #[test]
    fn l_historique_est_borne_et_le_curseur_suit() {
        let mut h = HistoriqueNotes::default();
        for i in 0..(PROFONDEUR_HISTORIQUE + 10) {
            h.enregistrer(vec![note(i as u64, "avant")], &[note(i as u64 + 1, "apres")]);
        }
        assert_eq!(h.etats.len(), PROFONDEUR_HISTORIQUE, "borné");
        assert!(h.curseur < h.etats.len(), "le curseur reste dans la timeline");
        assert!(h.peut_annuler());

        // Et on peut annuler jusqu'au plus ancien conservé, sans déborder.
        let mut annulations = 0;
        while h.annuler().is_some() {
            annulations += 1;
        }
        assert_eq!(annulations, PROFONDEUR_HISTORIQUE - 1);
        assert!(!h.peut_annuler(), "au fond, on s'arrête");
        assert!(h.peut_refaire());
    }
}

#[cfg(test)]
mod sonde_fenetres_tests {
    use super::*;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowRect, IsWindowVisible,
    };

    thread_local! {
        static CLASSE_CIBLE: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
        static TROUVEES: std::cell::RefCell<Vec<(isize, bool, [i32; 4])>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    unsafe extern "system" fn rappel_fenetre(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        let mut tampon = [0u16; 256];
        let longueur = unsafe { GetClassNameW(hwnd, &mut tampon) };
        if longueur > 0 {
            let nom = String::from_utf16_lossy(&tampon[..longueur as usize]);
            let cible = CLASSE_CIBLE.with(|c| c.borrow().clone());
            if nom == cible {
                let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
                let mut rect = RECT::default();
                let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
                TROUVEES.with(|t| {
                    t.borrow_mut().push((hwnd.0 as isize, visible, [rect.left, rect.top, rect.right, rect.bottom]));
                });
            }
        }
        // Toujours `true` : on veut TOUTES les fenêtres, pas la première.
        BOOL(1)
    }

    /// **SONDE — COMBIEN DE FENÊTRES DU PANNEAU NOTES EXISTENT, ET SONT-ELLES
    /// VISIBLES ?**
    ///
    /// Elle est écrite parce qu'un défaut résiste à TOUTES les autres mesures : le
    /// DOM contient une carte, la page l'ÉCRIT elle-même à l'écran (« 1 carte(s) »),
    /// et l'utilisateur en voit DEUX. Une surface figée SOUS la surface vivante
    /// expliquerait exactement cela — un panneau CEF a un fond TRANSPARENT, donc ce
    /// que la couche du dessus ne peint plus laisse voir celle du dessous.
    ///
    /// ⚠️ **ELLE INTERROGE LE SYSTÈME, PAS L'APPLICATION** : c'est la seule sonde
    /// qui regarde les fenêtres réelles, et elle tourne pendant que l'app vit. Aucune
    /// saisie, aucun clic — juste une énumération, comme les sondes MCI.
    ///
    ///     cargo test --bin neuradesk sonde_compte -- --ignored --nocapture
    #[test]
    #[ignore]
    fn sonde_compte_les_fenetres_du_panneau_notes() {
        let classe = unsafe { SPEC.class_name.to_string() }.expect("nom de classe lisible");
        CLASSE_CIBLE.with(|c| *c.borrow_mut() = classe.clone());
        TROUVEES.with(|t| t.borrow_mut().clear());
        unsafe {
            let _ = EnumWindows(Some(rappel_fenetre), LPARAM(0));
        }
        let trouvees = TROUVEES.with(|t| t.borrow().clone());
        println!("classe « {classe} » : {} fenetre(s)", trouvees.len());
        for (hwnd, visible, rect) in &trouvees {
            println!(
                "  hwnd={hwnd} visible={visible} rect={}x{} a ({}, {})",
                rect[2] - rect[0],
                rect[3] - rect[1],
                rect[0],
                rect[1]
            );
        }
        assert!(!trouvees.is_empty(), "aucune fenetre de ce panneau : l'app tourne-t-elle ?");
    }
}
