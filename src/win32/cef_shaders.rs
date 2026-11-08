//! **LE DASH DE L'ÉDITEUR DE SHADERS** — l'applet qui écrit les motifs de
//! l'utilisateur du dégradé animé (voir `gfx::shader_utilisateur`, le moteur).
//!
//! DEMANDE (2026-10-10) : « une applet pour disposer d'un éditeur de shader qui
//! permette de les utiliser dans les dégradés animés de l'app » — lot 2, l'éditeur
//! sur le moteur du lot 1.
//!
//! ⭐ **C'EST UN PANNEAU, PAS UNE FENÊTRE** — DEMANDE EXPLICITE (2026-10-10) : « je
//! voudrais que l'éditeur de shaders soit dans un dash ». Il a d'abord été une
//! fenêtre de la famille des kaomojis ; il vit maintenant dans la zone libre comme
//! la météo ou le scanner, avec ce que ça implique :
//! - **il masque les panneaux voisins, et chacun d'eux le masque** (voir
//!   `masquer_les_voisins`) ;
//! - **il se referme quand le focus part ailleurs**, sauf épinglé — la punaise est
//!   celle de tous les panneaux, posée par `cef_popup` à côté du `✕` de la page ;
//! - **sa taille est celle du dash** (`app::dash_rect`), pas une part de l'écran.
//!
//! ⚠️ **LE CODE NE PASSE PAS PAR L'URL** : un shader fait des Ko, porte des `/`,
//! des `%` et des retours à la ligne. La page le POSTE sur `cef_data`
//! (`ROUTE_CODE`) puis émet le verbe, que Rust lit en prenant le corps — le motif
//! de `commandes::remplacer_depuis`.
//!
//! **CE QUE LE PANNEAU NE FAIT PAS** : compiler sur le fil d'interface. « Appliquer »
//! écrit le fichier et le confie à `shader_utilisateur::choisir`, qui compile à
//! part et lance l'essai — l'éditeur n'a pas de garde à lui, il a celles du moteur.

use std::cell::{Cell, RefCell};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::shader_utilisateur as moteur;
use crate::gfx::GfxDevice;
use crate::win32::cef_app::percent_decode;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};

thread_local! {
    static SLOT: PopupSlot = PopupSlot::new();
    /// L'échelle du contenu (celle de l'écran du clic) — poussée à la page.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// Le dernier état poussé — `setState` à chaque frame reconstruirait la liste.
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// La rafale de poussée — la page peut ne pas avoir fini de charger à
    /// l'ouverture, et un état poussé trop tôt est perdu.
    static RAFALE: Cell<u32> = const { Cell::new(0) };
    /// **LE CODE À CHARGER DANS L'ÉDITEUR**, et son numéro : la page ne remplace
    /// sa saisie que quand le numéro change — sinon chaque poussée (le compte
    /// rendu d'une compilation) effacerait ce qu'on est en train d'écrire.
    static A_CHARGER: RefCell<(u64, String, String)> = const { RefCell::new((0, String::new(), String::new())) };
}

/// La route du code envoyé par la page (voir la doc de module).
const ROUTE_CODE: &str = "shaders/code";

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskShadersWindow"),
    window_title: w!("NeuraDesk — Éditeur de shaders"),
    html_file: "shaders.html",
    wndproc: Some(wndproc),
    // PANNEAU — même famille que la météo et le scanner.
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
    crate::win32::bandeau::hide_if_visible();
}

/// Ouvre ou referme le panneau. Appelée par le bras de clic de l'applet.
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    let ouvrir = !cef_popup::is_visible(&SLOT);
    if ouvrir {
        // À l'ouverture, l'éditeur montre le motif actif — ou le premier du
        // dossier. Seulement si la page n'a rien chargé encore : rouvrir le
        // panneau ne doit pas écraser un code en cours d'écriture.
        let deja = A_CHARGER.with(|a| a.borrow().0 > 0);
        if !deja {
            if let Some(nom) = moteur::nom_actif().or_else(|| moteur::lister().into_iter().next()) {
                charger(&nom);
            }
        }
        RAFALE.with(|r| r.set(6));
    }
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("shaders")));
    PAYLOAD.with(|p| *p.borrow_mut() = None);
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, masquer_les_voisins);
    push_state();
}

/// Met le code du motif `nom` dans l'éditeur (voir `A_CHARGER`).
fn charger(nom: &str) {
    match moteur::lire_source(nom) {
        Ok(code) => A_CHARGER.with(|a| {
            let mut a = a.borrow_mut();
            *a = (a.0 + 1, nom.to_string(), code);
        }),
        Err(erreur) => moteur::dire(format!("« {nom} » n'a pas pu être ouvert : {erreur}")),
    }
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

/// La copie du back buffer pour le harnais (`NEURADESK_DEBUG_OPEN=shaders`).
pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// L'état de la page. ⚠️ **PAR `serde_json`** : le code porte des `\` et des
/// guillemets, et un littéral JS écrit à la main les perdrait en silence.
fn etat_json() -> String {
    let (charge, nom, code) = A_CHARGER.with(|a| a.borrow().clone());
    serde_json::json!({
        "contentScale": CONTENT_SCALE.with(Cell::get),
        "motifs": moteur::lister_en_cache(),
        "actif": moteur::nom_actif(),
        "enCompilation": moteur::en_compilation(),
        "message": moteur::message(),
        "charge": charge,
        "nom": nom,
        "code": code,
        "modele": moteur::MODELE,
        "degradeVisible": crate::gfx::degrade::actif(),
        "enPause": crate::gfx::degrade::en_pause(),
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

/// **À APPELER À CHAQUE FRAME** — le compte rendu d'une compilation arrive d'un
/// AUTRE fil, il faut le relire pour le montrer.
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

/// **CE QUE LA PAGE DEMANDE.** Les noms arrivent ENCODÉS (un nom peut porter une
/// espace ou une parenthèse) : décodés ici, après le découpage sur les `/`.
pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("shaders/") else { continue };
        let morceaux: Vec<String> = reste.split('/').map(percent_decode).collect();
        let morceaux: Vec<&str> = morceaux.iter().map(String::as_str).collect();
        match morceaux.as_slice() {
            // Le `✕` de la page : il ferme MÊME un panneau épinglé.
            ["fermer"] => cef_popup::fermer_par_geste(&SLOT),
            ["ouvrir", nom] => charger(nom),
            ["enregistrer", nom] => {
                enregistrer(nom);
            }
            ["appliquer", nom] => {
                if enregistrer(nom) {
                    moteur::choisir(nom);
                }
            }
            ["retirer"] => {
                // Rendre la main au motif intégré : c'est `set_mode` qui retire le
                // motif de l'utilisateur, et le fichier se réécrit avec.
                let integre = crate::gfx::degrade::mode_nom().to_string();
                crate::gfx::degrade::set_mode(&integre);
                crate::config::save_degrade_mode(&integre);
                moteur::dire(format!("Motif intégré « {integre} » rétabli."));
            }
            ["pause"] => crate::gfx::degrade::basculer_pause(),
            ["dossier"] => {
                if let Some(dossier) = moteur::chemin_du_dossier() {
                    crate::win32::appicon::open(&dossier.to_string_lossy());
                }
            }
            autre => crate::diag!("[shaders] verbe inconnu : {autre:?}"),
        }
    }
}

/// **ÉCRIT LE CODE QUE LA PAGE VIENT DE POSTER** sous `nom`. `false` si rien n'a
/// été écrit — le compte rendu le dit.
fn enregistrer(nom: &str) -> bool {
    let Some(octets) = crate::win32::cef_data::prendre(ROUTE_CODE) else {
        moteur::dire("le code n'est pas arrivé — réessayez");
        return false;
    };
    let code = String::from_utf8_lossy(&octets).into_owned();
    match moteur::ecrire_source(nom, &code) {
        Ok(()) => {
            // Le nom est peut-être nouveau : l'éditeur le garde comme le sien, sans
            // recharger le code (ce serait le même).
            A_CHARGER.with(|a| {
                let mut a = a.borrow_mut();
                a.1 = nom.to_string();
                a.2 = code;
            });
            moteur::dire(format!("« {nom} » enregistré."));
            true
        }
        Err(erreur) => {
            moteur::dire(format!("« {nom} » n'a pas été enregistré : {erreur}"));
            false
        }
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

    const VERBES: [&str; 7] = ["fermer", "ouvrir", "enregistrer", "appliquer", "retirer", "pause", "dossier"];

    /// ⭐ **LA PAGE ET RUST PARLENT LE MÊME LANGAGE** — chaque `go("verbe"…)` de la
    /// page a son bras ici, et chaque bras est appelé par la page.
    #[test]
    fn les_verbes_de_la_page_sont_exactement_ceux_du_dispatch() {
        let page = include_str!("../../assets/popups/shaders.html");
        let source = include_str!("cef_shaders.rs");
        let connus = &VERBES[..];
        let mut vus = Vec::new();
        for morceau in page.split("go(\"").skip(1) {
            let verbe = morceau.split('"').next().unwrap_or("");
            assert!(connus.contains(&verbe), "verbe inconnu passé à `go` : {verbe:?}");
            vus.push(verbe);
        }
        for verbe in connus {
            assert!(vus.contains(verbe), "la page n'appelle jamais `{verbe}`");
            assert!(source.contains(&format!("[\"{verbe}\"")), "aucun bras pour `{verbe}`");
        }
    }

    /// ⭐ **LA PUNAISE DES PANNEAUX SE POSE À CÔTÉ DU `✕`** (voir
    /// `cef_popup::js_epingle`, qui le cherche par `.close-btn` ou un titre
    /// « Fermer ») : sans lui, ce panneau serait le seul qu'on ne pourrait pas
    /// épingler — et un éditeur qui se ferme quand on va voir l'Explorateur perd
    /// la main.
    #[test]
    fn la_page_porte_le_bouton_que_la_punaise_cherche() {
        let page = include_str!("../../assets/popups/shaders.html");
        assert!(page.contains("class=\"close-btn\"") && page.contains("title=\"Fermer\""));
        // L'ancienne punaise de FENÊTRE ne doit pas revenir à côté de celle-là.
        assert!(!page.contains("id=\"epingle\""));
    }

    /// ⛔ **LES VOISINS, DANS LES DEUX SENS** : chaque panneau que celui-ci masque
    /// doit le masquer en retour — sinon les deux se superposent dans la zone libre
    /// quand on ouvre l'autre en dernier.
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
            ("cef_lueurs", include_str!("cef_lueurs.rs")),
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
        let nous = include_str!("cef_shaders.rs");
        for (nom, source) in voisins {
            assert!(nous.contains(&format!("crate::win32::{nom}::hide_if_visible();")), "nous ne masquons pas {nom}");
            assert!(source.contains("crate::win32::cef_shaders::hide_if_visible()"), "{nom} ne nous masque pas");
        }
    }

    /// ⛔ L'état porte le code par `serde_json` : antislashs et guillemets intacts.
    #[test]
    fn l_etat_garde_le_code_intact() {
        let code = "float4 Motif(float2 p, float aspect, float t) { // \"guillemets\" \\ antislash\n return 0; }";
        A_CHARGER.with(|a| *a.borrow_mut() = (7, "essai".into(), code.into()));
        let json: serde_json::Value = serde_json::from_str(&etat_json()).expect("un JSON valide");
        assert_eq!(json["code"].as_str(), Some(code));
        assert_eq!(json["charge"].as_u64(), Some(7));
        assert!(json["modele"].as_str().is_some_and(|m| m.contains("float4 Motif")));
    }
}
