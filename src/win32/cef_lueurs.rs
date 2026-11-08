//! **LE DASH DES LUEURS** — l'éditeur de la scène de `gfx::lueur`.
//!
//! DEMANDE EXPLICITE (2026-10-14) : « pouvoir éditer l'effet, ajouter des éléments,
//! régler le flou, dans un dash ouvrable depuis une applet dans la zone intégrée ou
//! depuis le dash réglages ».
//!
//! Un PANNEAU de la zone libre, de la famille de l'éditeur de shaders : il masque
//! ses voisins et chacun d'eux le masque, il se referme quand le focus part
//! ailleurs (sauf épinglé), sa taille est celle du dash.
//!
//! ⚠️ **OUVERT DE DEUX ENDROITS** : le clic sur l'applet (`app::run`, qui a le
//! device et le rectangle), et le bouton des Réglages — qui n'a ni l'un ni l'autre,
//! d'où `demander_ouverture`, relue par la boucle (`prendre_demande`).

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

use crate::gfx::lueur::{self, Degrade, Duree, Element, Figure, Mouvement, Pause};
use crate::gfx::GfxDevice;
use crate::win32::cef_app::percent_decode;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};

thread_local! {
    static SLOT: PopupSlot = PopupSlot::new();
    /// L'échelle du contenu (celle de l'écran du clic) — poussée à la page.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// Le dernier état poussé — rien n'est renvoyé tant qu'il ne change pas.
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// La rafale de poussée — la page peut ne pas avoir fini de charger à
    /// l'ouverture, et un état poussé trop tôt est perdu.
    static RAFALE: Cell<u32> = const { Cell::new(0) };
    /// TOUS les écrans, en % de l'écran PRINCIPAL — relevés à l'OUVERTURE
    /// (`toggle`) : l'état est recalculé à chaque frame, et énumérer les écrans
    /// à 60 Hz serait le défaut « un chemin résolu par frame ».
    static ECRANS: RefCell<Vec<serde_json::Value>> = const { RefCell::new(Vec::new()) };
}

/// La demande d'ouverture des Réglages (voir la doc de module).
static DEMANDE: AtomicBool = AtomicBool::new(false);

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskLueursWindow"),
    window_title: w!("NeuraDesk — Lueurs"),
    html_file: "lueurs.html",
    wndproc: Some(wndproc),
    // PANNEAU — même famille que l'éditeur de shaders.
    above_a_magnified_dock: false,
    fades: false,
    fenetre: false,
};

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// **LES PANNEAUX QUI PARTAGENT LA ZONE LIBRE** — voir `cef_aide::
/// masquer_les_voisins`, qui porte l'avertissement : la liste se vérifie par
/// `grep`, et chacun de ces panneaux doit nous masquer en retour.
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
    crate::win32::bandeau::hide_if_visible();
}

/// Ouvre ou referme le panneau. Appelée par le bras de clic de l'applet.
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    if !cef_popup::is_visible(&SLOT) {
        RAFALE.with(|r| r.set(6));
    }
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("lueurs")));
    PAYLOAD.with(|p| *p.borrow_mut() = None);
    ECRANS.with(|e| *e.borrow_mut() = ecrans_en_pourcents());
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, masquer_les_voisins);
    push_state();
}

/// Demande l'ouverture depuis un endroit qui n'a pas le device (les Réglages).
pub fn demander_ouverture() {
    DEMANDE.store(true, Ordering::Relaxed);
}

/// Vrai une fois par demande, et seulement si le panneau est FERMÉ : la boucle
/// l'ouvre alors par `toggle`.
pub fn prendre_demande() -> bool {
    DEMANDE.swap(false, Ordering::Relaxed) && !cef_popup::is_visible(&SLOT)
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

pub fn render_if_visible(rect: [i32; 4], tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::resize_if_needed(&SLOT, rect);
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

/// La copie du back buffer pour le harnais (`NEURADESK_DEBUG_OPEN=lueurs`).
pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// **LES ÉCRANS, EN % DE L'ÉCRAN PRINCIPAL** — DEMANDE EXPLICITE (2026-10-14) :
/// « voir l'espace occupé par les écrans secondaires en plus de l'écran
/// principal ». Le principal est en `(0, 0)` des coordonnées écran : ses
/// voisins se placent donc directement par division.
fn ecrans_en_pourcents() -> Vec<serde_json::Value> {
    let ecrans = crate::win32::monitors::enumerate();
    let Some(principal) = ecrans.iter().find(|m| m.is_primary) else { return Vec::new() };
    let (l, h) = ((principal.rect.right - principal.rect.left).max(1) as f32, (principal.rect.bottom - principal.rect.top).max(1) as f32);
    en_pourcents(ecrans.iter().map(|m| ([m.rect.left, m.rect.top, m.rect.right, m.rect.bottom], m.is_primary)), [l, h])
}

/// Pure : `(gauche, haut, droite, bas)` en pixels → `{x, y, l, h, principal}` en %.
fn en_pourcents(ecrans: impl Iterator<Item = ([i32; 4], bool)>, [l, h]: [f32; 2]) -> Vec<serde_json::Value> {
    // En f64 : un f32 élargi dans le JSON écrirait 22.399999618 au lieu de 22.4.
    let pct = |v: i32, sur: f32| (f64::from(v) / f64::from(sur) * 1000.0).round() / 10.0;
    ecrans
        .map(|([g, ha, d, b], principal)| {
            serde_json::json!({ "x": pct(g, l), "y": pct(ha, h), "l": pct(d - g, l), "h": pct(b - ha, h), "principal": principal })
        })
        .collect()
}

/// `#rrggbb` d'une couleur 0..1.
fn hex([r, g, b]: [f32; 3]) -> String {
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", c(r), c(g), c(b))
}

/// La couleur d'un `#rrggbb` (ou `rrggbb`).
fn depuis_hex(texte: &str) -> Option<[f32; 3]> {
    let t = texte.trim_start_matches('#');
    if t.len() != 6 {
        return None;
    }
    let canal = |i: usize| u8::from_str_radix(&t[i..i + 2], 16).ok().map(|v| v as f32 / 255.0);
    Some([canal(0)?, canal(2)?, canal(4)?])
}

/// L'état de la page — en POURCENTS, ce que montrent les curseurs.
fn etat_json() -> String {
    let scene = lueur::scene();
    // DEUX décimales : une épaisseur de 0,13 % arrondie à 0,1 % ferait sauter le
    // curseur logarithmique de la page à chaque écho.
    let pct = |v: f32| (v * 10000.0).round() / 100.0;
    let ecran = unsafe { [GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)] };
    serde_json::json!({
        "contentScale": CONTENT_SCALE.with(Cell::get),
        "active": scene.active,
        "captureVisible": crate::gfx::capture_visibility::is_enabled(),
        "assombrissement": pct(scene.assombrissement),
        "opacite": pct(scene.opacite),
        "rotation": scene.rotation,
        "centre": [pct(scene.centre[0]), pct(scene.centre[1])],
        "ecran": ecran,
        "ecrans": ECRANS.with(|e| e.borrow().clone()),
        "max": lueur::MAX_ELEMENTS,
        "effets": crate::win32::lueurs_effets::noms(),
        "effet": crate::win32::lueurs_effets::actuel(),
        "maxPoints": lueur::MAX_POINTS,
        "maxEtapes": lueur::MAX_ETAPES,
        "maxArrets": lueur::MAX_ARRETS,
        // La bibliothèque des images (voir `gfx::lueur_images`), par empreinte.
        "images": crate::gfx::lueur_images::bibliotheque().into_iter()
            .map(|(id, nom)| serde_json::json!({ "id": format!("{id:08x}"), "nom": nom }))
            .collect::<Vec<_>>(),
        "elements": scene.elements.iter().map(|e| serde_json::json!({
            "x": pct(e.x), "y": pct(e.y), "rayon": pct(e.rayon),
            "intensite": pct(e.intensite), "flou": pct(e.flou), "couleur": hex(e.couleur),
            "mouvement": e.mouvement.nom(), "periode": e.periode, "orbite": pct(e.orbite),
            "points": e.chemin().iter().map(|[x, y]| [pct(*x), pct(*y)]).collect::<Vec<_>>(),
            "apparition": e.apparition, "cycle": e.cycle, "presence": pct(e.presence),
            "fondu": pct(e.fondu), "decalage": pct(e.decalage),
            "cycleCouleur": e.cycle_couleur, "dureeCouleur": e.duree_couleur,
            "figure": e.figure.nom(), "inclinaison": e.inclinaison,
            "longueur": pct(e.longueur), "rotationPropre": e.rotation_propre,
            "attente": e.attente, "disparition": e.disparition, "absence": e.absence,
            "etapes": e.couleurs_du_cycle().iter().map(|c| hex(*c)).collect::<Vec<_>>(),
            "degrade": e.degrade.nom(),
            "arrets": e.couleurs_du_degrade().iter().map(|c| hex(*c)).collect::<Vec<_>>(),
            "image": format!("{:08x}", e.image),
            "defilement": e.defilement, "teinte": e.teinte,
        })).collect::<Vec<_>>(),
    })
    .to_string()
}

/// Pousse l'état s'il a changé.
fn push_state() {
    let json = etat_json();
    let change = PAYLOAD.with(|p| {
        let mut p = p.borrow_mut();
        if p.as_deref() == Some(json.as_str()) {
            false
        } else {
            *p = Some(json.clone());
            true
        }
    });
    if change {
        cef_popup::execute_js(&SLOT, &format!("if (window.setState) window.setState({json})"));
    }
}

/// **À APPELER À CHAQUE FRAME** — la case des Réglages change aussi la scène.
pub fn refresh_if_visible() {
    if !cef_popup::is_visible(&SLOT) {
        return;
    }
    let restante = RAFALE.with(|r| r.get());
    if restante > 0 {
        RAFALE.with(|r| r.set(restante - 1));
        PAYLOAD.with(|p| *p.borrow_mut() = None);
    }
    push_state();
}

/// **CE QUE LA PAGE DEMANDE.** Les valeurs arrivent en POURCENTS.
pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("lueurs/") else { continue };
        let morceaux: Vec<String> = reste.split('/').map(percent_decode).collect();
        let morceaux: Vec<&str> = morceaux.iter().map(String::as_str).collect();
        appliquer(&morceaux);
    }
}

/// Un verbe de la page — séparé de `dispatch_actions` pour être testable.
fn appliquer(morceaux: &[&str]) {
    let nombre = |t: &str| t.parse::<f32>().ok().filter(|v| v.is_finite());
    let index = |t: &str| t.parse::<usize>().ok();
    match morceaux {
        // Le `✕` de la page : il ferme MÊME un panneau épinglé.
        ["fermer"] => cef_popup::fermer_par_geste(&SLOT),
        ["activer", flag] => {
            let active = *flag == "1";
            lueur::modifier(|s| s.active = active);
        }
        ["assombrir", pct] => {
            if let Some(v) = nombre(pct) {
                lueur::modifier(|s| s.assombrissement = v / 100.0);
            }
        }
        // L'OPACITÉ GLOBALE (DEMANDE EXPLICITE, 2026-10-14). Voir `Scene::opacite`.
        ["opacite", pct] => {
            if let Some(v) = nombre(pct) {
                lueur::modifier(|s| s.opacite = v / 100.0);
            }
        }
        // Ajouter et retirer se font EN FONDU (voir `gfx::lueur::ajouter`).
        ["ajouter"] => lueur::ajouter(),
        ["retirer", i] => {
            if let Some(i) = index(i) {
                lueur::retirer(i);
            }
        }
        ["regler", i, champ, pct] => {
            if let (Some(i), Some(v)) = (index(i), nombre(pct)) {
                lueur::modifier(|s| {
                    if let Some(c) = s.elements.get_mut(i).and_then(|e| lueur::champ_mut(e, champ)) {
                        *c = v / 100.0;
                    }
                });
            }
        }
        ["deplacer", i, x, y] => {
            if let (Some(i), Some(x), Some(y)) = (index(i), nombre(x), nombre(y)) {
                lueur::modifier(|s| {
                    if let Some(e) = s.elements.get_mut(i) {
                        (e.x, e.y) = (x / 100.0, y / 100.0);
                    }
                });
            }
        }
        ["couleur", i, valeur] => {
            if let (Some(i), Some(c)) = (index(i), depuis_hex(valeur)) {
                lueur::modifier(|s| {
                    if let Some(e) = s.elements.get_mut(i) {
                        e.couleur = c;
                    }
                });
            }
        }
        // L'ANIMATION (DEMANDE EXPLICITE, 2026-10-14 : « des chemins programmés, des
        // allers et retours et des rotations du fond »).
        ["mouvement", i, nom] => {
            if let (Some(i), Some(m)) = (index(i), Mouvement::depuis_nom(nom)) {
                lueur::modifier(|s| {
                    if let Some(e) = s.elements.get_mut(i) {
                        e.mouvement = m;
                        // Un chemin vide ne bougerait pas : on lui donne un point.
                        if matches!(m, Mouvement::AllerRetour | Mouvement::Boucle | Mouvement::Trajet) && e.nb_points == 0 {
                            e.ajouter_point();
                        }
                    }
                });
            }
        }
        // ⛔ Les durées passent par `changer_duree`, qui garde la phase : sinon
        // la lueur SAUTAIT à chaque cran du curseur (défaut signalé, 2026-10-14).
        ["periode", i, secondes] => {
            if let (Some(i), Some(v)) = (index(i), nombre(secondes)) {
                lueur::changer_duree(i, Duree::Mouvement, v);
            }
        }
        // Les TEMPS D'ARRÊT (DEMANDE EXPLICITE, 2026-10-14 : « une minuterie avant le
        // mouvement », « disparition puis réapparition au point de départ »).
        ["pause", i, quelle, secondes] => {
            if let (Some(i), Some(q), Some(v)) = (index(i), Pause::depuis_nom(quelle), nombre(secondes)) {
                lueur::changer_pause(i, q, v);
            }
        }
        ["point", i, k, x, y] => {
            if let (Some(i), Some(k), Some(x), Some(y)) = (index(i), index(k), nombre(x), nombre(y)) {
                lueur::modifier(|s| {
                    if let Some(e) = s.elements.get_mut(i).filter(|e| k < e.nb_points) {
                        e.points[k] = [x / 100.0, y / 100.0];
                    }
                });
            }
        }
        ["ajouter-point", i] => {
            if let Some(i) = index(i) {
                lueur::modifier(|s| s.elements.get_mut(i).into_iter().for_each(Element::ajouter_point));
            }
        }
        ["retirer-point", i] => {
            if let Some(i) = index(i) {
                lueur::modifier(|s| s.elements.get_mut(i).into_iter().for_each(Element::retirer_point));
            }
        }
        // APPARAÎTRE ET DISPARAÎTRE EN FONDU (DEMANDE EXPLICITE, 2026-10-14).
        ["apparition", i, flag] => {
            if let Some(i) = index(i) {
                let actif = *flag == "1";
                lueur::modifier(|s| s.elements.get_mut(i).into_iter().for_each(|e| e.apparition = actif));
            }
        }
        ["cycle", i, secondes] => {
            if let (Some(i), Some(v)) = (index(i), nombre(secondes)) {
                lueur::changer_duree(i, Duree::Apparition, v);
            }
        }
        // LES CYCLES DE COULEURS (DEMANDE EXPLICITE, 2026-10-14).
        ["cycle-couleur", i, flag] => {
            if let Some(i) = index(i) {
                let actif = *flag == "1";
                lueur::modifier(|s| {
                    if let Some(e) = s.elements.get_mut(i) {
                        e.cycle_couleur = actif;
                        // Un cycle d'UNE couleur ne changerait rien : on lui en donne deux.
                        if actif && e.nb_etapes == 0 {
                            e.ajouter_etape();
                        }
                    }
                });
            }
        }
        ["duree-couleur", i, secondes] => {
            if let (Some(i), Some(v)) = (index(i), nombre(secondes)) {
                lueur::changer_duree(i, Duree::Couleur, v);
            }
        }
        // L'étape `k` du cycle : 0 est la couleur PRINCIPALE, `k ≥ 1` les suivantes.
        ["etape", i, k, valeur] => {
            if let (Some(i), Some(k), Some(c)) = (index(i), index(k), depuis_hex(valeur)) {
                lueur::modifier(|s| {
                    if let Some(e) = s.elements.get_mut(i) {
                        match k {
                            0 => e.couleur = c,
                            k if k <= e.nb_etapes => e.etapes[k - 1] = c,
                            _ => {}
                        }
                    }
                });
            }
        }
        ["ajouter-etape", i] => {
            if let Some(i) = index(i) {
                lueur::modifier(|s| s.elements.get_mut(i).into_iter().for_each(Element::ajouter_etape));
            }
        }
        ["retirer-etape", i] => {
            if let Some(i) = index(i) {
                lueur::modifier(|s| s.elements.get_mut(i).into_iter().for_each(Element::retirer_etape));
            }
        }
        ["arc-en-ciel", i] => {
            if let Some(i) = index(i) {
                lueur::modifier(|s| s.elements.get_mut(i).into_iter().for_each(Element::arc_en_ciel));
            }
        }
        // LES FIGURES, leur inclinaison et leur rotation propre (DEMANDE EXPLICITE,
        // 2026-10-14).
        ["figure", i, nom] => {
            if let (Some(i), Some(f)) = (index(i), Figure::depuis_nom(nom)) {
                lueur::modifier(|s| s.elements.get_mut(i).into_iter().for_each(|e| e.changer_figure(f)));
            }
        }
        ["inclinaison", i, degres] => {
            if let (Some(i), Some(v)) = (index(i), nombre(degres)) {
                lueur::modifier(|s| s.elements.get_mut(i).into_iter().for_each(|e| e.inclinaison = v));
            }
        }
        ["reinitialiser-rotation", i] => {
            if let Some(i) = index(i) {
                lueur::reinitialiser_rotation(i);
            }
        }
        ["rotation-propre", i, tours] => {
            if let (Some(i), Some(v)) = (index(i), nombre(tours)) {
                lueur::changer_rotation_propre(i, v);
            }
        }
        // ⭐ LES DÉGRADÉS DANS LES FORMES (DEMANDE EXPLICITE, 2026-10-14). Voir
        // `gfx::lueur::Degrade`.
        ["degrade", i, nom] => {
            if let (Some(i), Some(d)) = (index(i), Degrade::depuis_nom(nom)) {
                lueur::modifier(|s| s.elements.get_mut(i).into_iter().for_each(|e| e.changer_degrade(d)));
            }
        }
        // L'arrêt `k` du dégradé : 0 est la couleur PRINCIPALE, `k ≥ 1` les suivantes.
        ["arret", i, k, valeur] => {
            if let (Some(i), Some(k), Some(c)) = (index(i), index(k), depuis_hex(valeur)) {
                lueur::modifier(|s| {
                    if let Some(e) = s.elements.get_mut(i) {
                        match k {
                            0 => e.couleur = c,
                            k if k <= e.nb_arrets => e.arrets[k - 1] = c,
                            _ => {}
                        }
                    }
                });
            }
        }
        // ⭐ LE DÉGRADÉ ANIMÉ (DEMANDE EXPLICITE, 2026-10-14) : sans saut.
        ["animer-degrade", i, quoi, tours] => {
            if let (Some(i), Some(v)) = (index(i), nombre(tours)) {
                let teinte = *quoi == "teinte";
                lueur::changer_animation_du_degrade(i, teinte, v);
            }
        }
        ["ajouter-arret", i] => {
            if let Some(i) = index(i) {
                lueur::modifier(|s| s.elements.get_mut(i).into_iter().for_each(Element::ajouter_arret));
            }
        }
        ["retirer-arret", i] => {
            if let Some(i) = index(i) {
                lueur::modifier(|s| s.elements.get_mut(i).into_iter().for_each(Element::retirer_arret));
            }
        }
        // ⭐ LES IMAGES (DEMANDE EXPLICITE, 2026-10-14 : « importer des images à
        // utiliser comme lueurs »). Voir `gfx::lueur_images`. L'image importée prend
        // ses PROPORTIONS : la demi-largeur suit la demi-hauteur.
        ["importer-image", i] => {
            let (Some(i), Some(hwnd)) = (index(i), cef_popup::hwnd(&SLOT)) else { return };
            let Some(source) = crate::win32::file_dialog::ouvrir(hwnd, "Importer une image pour une lueur", "png;jpg;jpeg;bmp", "Images (*.png, *.jpg, *.bmp)") else { return };
            if let Some((id, proportion)) = crate::gfx::lueur_images::importer(&source) {
                lueur::modifier(|s| s.elements.get_mut(i).into_iter().for_each(|e| poser_image(e, id, Some(proportion))));
            }
        }
        ["image", i, id] => {
            if let (Some(i), Ok(id)) = (index(i), u32::from_str_radix(id, 16)) {
                lueur::modifier(|s| s.elements.get_mut(i).into_iter().for_each(|e| poser_image(e, id, None)));
            }
        }
        // LA BIBLIOTHÈQUE DES EFFETS (DEMANDE EXPLICITE, 2026-10-14 : « sauvegarder
        // et importer mes effets de lueurs »). Voir `win32::lueurs_effets`.
        ["enregistrer", nom] => {
            crate::win32::lueurs_effets::enregistrer(nom);
        }
        ["charger", nom] => {
            crate::win32::lueurs_effets::charger(nom);
        }
        ["supprimer-effet", nom] => {
            crate::win32::lueurs_effets::supprimer(nom);
        }
        // Les deux boîtes de dialogue s'ouvrent au-dessus de CE panneau (voir
        // `file_dialog::enregistrer_sous` : une boîte sans propriétaire peut
        // passer derrière, et l'app paraîtrait figée).
        ["exporter", nom] => {
            if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                crate::win32::lueurs_effets::exporter(hwnd, nom);
            }
        }
        ["importer"] => {
            if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                crate::win32::lueurs_effets::importer(hwnd);
            }
        }
        ["rotation", tours] => {
            if let Some(v) = nombre(tours) {
                lueur::changer_rotation(v);
            }
        }
        ["centre", x, y] => {
            if let (Some(x), Some(y)) = (nombre(x), nombre(y)) {
                lueur::modifier(|s| s.centre = [x / 100.0, y / 100.0]);
            }
        }
        autre => crate::diag!("[lueurs] verbe inconnu : {autre:?}"),
    }
}

/// Fait de `e` une lueur IMAGE sur `id`. `proportion` (largeur / hauteur) : celle
/// d'une image qu'on vient d'importer — la demi-largeur s'y ajuste.
fn poser_image(e: &mut Element, id: u32, proportion: Option<f32>) {
    e.changer_figure(Figure::Image);
    e.image = id;
    if let Some(p) = proportion.filter(|p| p.is_finite() && *p > 0.0) {
        let (min, max) = (lueur::CHAMPS[9].1, lueur::CHAMPS[9].2);
        e.longueur = (e.rayon * p).clamp(min, max);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(resultat) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return resultat;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERBES: [&str; 41] = [
        "animer-degrade",
        "degrade",
        "arret",
        "ajouter-arret",
        "retirer-arret",
        "importer-image",
        "image",
        "reinitialiser-rotation",
        "pause",
        "enregistrer",
        "charger",
        "supprimer-effet",
        "exporter",
        "importer",
        "figure",
        "inclinaison",
        "rotation-propre",
        "cycle-couleur",
        "duree-couleur",
        "etape",
        "ajouter-etape",
        "retirer-etape",
        "arc-en-ciel",
        "apparition",
        "cycle",
        "fermer",
        "activer",
        "assombrir",
        "opacite",
        "ajouter",
        "retirer",
        "regler",
        "deplacer",
        "couleur",
        "mouvement",
        "periode",
        "point",
        "ajouter-point",
        "retirer-point",
        "rotation",
        "centre",
    ];

    /// ⭐ **LA PAGE ET RUST PARLENT LE MÊME LANGAGE** — chaque `go("verbe"…)` de la
    /// page a son bras ici, et chaque bras est appelé par la page.
    #[test]
    fn les_verbes_de_la_page_sont_exactement_ceux_du_dispatch() {
        let page = include_str!("../../assets/popups/lueurs.html");
        let source = include_str!("cef_lueurs.rs");
        let mut vus = Vec::new();
        for morceau in page.split("go(\"").skip(1) {
            let verbe = morceau.split('"').next().unwrap_or("");
            assert!(VERBES.contains(&verbe), "verbe inconnu passé à `go` : {verbe:?}");
            vus.push(verbe);
        }
        for verbe in VERBES {
            assert!(vus.contains(&verbe), "la page n'appelle jamais `{verbe}`");
            assert!(source.contains(&format!("[\"{verbe}\"")), "aucun bras pour `{verbe}`");
        }
    }

    /// La punaise des panneaux se pose à côté du `✕` (voir `cef_shaders`).
    #[test]
    fn la_page_porte_le_bouton_que_la_punaise_cherche() {
        let page = include_str!("../../assets/popups/lueurs.html");
        assert!(page.contains("class=\"close-btn\"") && page.contains("title=\"Fermer\""));
    }

    /// ⛔ **LES VOISINS, DANS LES DEUX SENS.**
    #[test]
    fn chaque_voisin_nous_masque_en_retour() {
        let voisins: [(&str, &str); 20] = [
            ("cef_dash", include_str!("cef_dash.rs")),
            ("cef_settings", include_str!("cef_settings.rs")),
            ("cef_notes", include_str!("cef_notes.rs")),
            ("cef_image_editor", include_str!("cef_image_editor.rs")),
            ("cef_calendar_applet", include_str!("cef_calendar_applet.rs")),
            ("cef_clipboard", include_str!("cef_clipboard.rs")),
            ("cef_surveillance", include_str!("cef_surveillance.rs")),
            ("cef_scanner", include_str!("cef_scanner.rs")),
            ("cef_shaders", include_str!("cef_shaders.rs")),
            ("cef_textures", include_str!("cef_textures.rs")),
            ("cef_moniteur", include_str!("cef_moniteur.rs")),
            ("cef_ssh", include_str!("cef_ssh.rs")),
            ("cef_terminal", include_str!("cef_terminal.rs")),
            ("cef_minuteur", include_str!("cef_minuteur.rs")),
            ("cef_calculatrice", include_str!("cef_calculatrice.rs")),
            ("cef_recherche", include_str!("cef_recherche.rs")),
            ("cef_automatisations", include_str!("cef_automatisations.rs")),
            ("cef_meteo", include_str!("cef_meteo.rs")),
            ("cef_aide", include_str!("cef_aide.rs")),
            ("bandeau", include_str!("bandeau.rs")),
        ];
        let nous = include_str!("cef_lueurs.rs");
        for (nom, source) in voisins {
            assert!(nous.contains(&format!("crate::win32::{nom}::hide_if_visible();")), "nous ne masquons pas {nom}");
            assert!(source.contains("crate::win32::cef_lueurs::hide_if_visible()"), "{nom} ne nous masque pas");
        }
    }

    #[test]
    fn un_ecran_secondaire_se_place_en_pourcents_du_principal() {
        // Le principal 3840×2160 en (0, 0), un 1920×1080 à sa GAUCHE, décalé de 484.
        let ecrans = en_pourcents([([0, 0, 3840, 2160], true), ([-1920, 484, 0, 1564], false)].into_iter(), [3840.0, 2160.0]);
        assert_eq!(ecrans[0], serde_json::json!({ "x": 0.0, "y": 0.0, "l": 100.0, "h": 100.0, "principal": true }));
        assert_eq!(ecrans[1], serde_json::json!({ "x": -50.0, "y": 22.4, "l": 50.0, "h": 50.0, "principal": false }));
    }

    #[test]
    fn les_couleurs_font_l_aller_retour_en_hexadecimal() {
        assert_eq!(hex([1.0, 0.5, 0.0]), "#ff8000");
        assert_eq!(depuis_hex("#ff8000"), Some([1.0, 128.0 / 255.0, 0.0]));
        assert_eq!(depuis_hex("ff80"), None);
        assert_eq!(depuis_hex("#gg0000"), None);
    }
}
