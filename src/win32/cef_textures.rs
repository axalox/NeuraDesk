//! **LE DASH DES TEXTURES** — l'éditeur de la texture personnalisée
//! (`gfx::texture_perso`), et des réglages communs de `gfx::texture_interface`.
//!
//! ⭐ DEMANDE EXPLICITE (2026-10-16) : « éditer les textures par défaut, éditer les
//! textures d'interface et de fond de terminaux et avoir la possibilité de les
//! sauvegarder et d'en importer ». L'éditeur a donc une CIBLE (`Cible`), et une
//! bibliothèque (`win32::textures_bibliotheque`) où les préréglages se modifient.
//!
//! DEMANDE EXPLICITE (2026-10-15) : « une applet pour un dash d'édition de textures
//! calculées, évoluer, où les fonctions et réglages sont réalisables avec des cases
//! à cocher et des curseurs ».
//!
//! Un PANNEAU de la zone libre, de la famille de l'éditeur des lueurs : il masque
//! ses voisins et chacun d'eux le masque, il se referme quand le focus part
//! ailleurs (sauf épinglé), sa taille est celle du dash. ⭐ **L'APERÇU, C'EST LE
//! PANNEAU LUI-MÊME** : son fond passe par le même shader que les barres, la
//! texture s'y voit en direct.
//!
//! ⚠️ **OUVERT DE DEUX ENDROITS** : le clic sur l'applet (`app::run`), et le bouton
//! des Réglages, qui le DEMANDE (`demander_ouverture`, relue par `prendre_demande`).

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::config;
use crate::gfx::texture_interface as interface;
use crate::gfx::texture_perso::{self as perso, Sorte, Texture};
use crate::gfx::GfxDevice;
use crate::win32::cef_app::percent_decode;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::textures_bibliotheque::{self as bibliotheque, Entree};

thread_local! {
    static SLOT: PopupSlot = PopupSlot::new();
    /// L'échelle du contenu (celle de l'écran du clic) — poussée à la page.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// Le dernier état poussé — rien n'est renvoyé tant qu'il ne change pas.
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// La rafale de poussée — la page peut ne pas avoir fini de charger à
    /// l'ouverture, et un état poussé trop tôt est perdu.
    static RAFALE: Cell<u32> = const { Cell::new(0) };
}

/// La demande d'ouverture des Réglages (voir la doc de module), et sa cible.
static DEMANDE: AtomicBool = AtomicBool::new(false);
static DEMANDE_TERMINAUX: AtomicBool = AtomicBool::new(false);

/// La nature de `texture_interface::NATURES` que cet éditeur compose.
const NATURE_PERSO: &str = "perso";

/// ⭐ **CE QUE L'ÉDITEUR HABILLE** — DEMANDE EXPLICITE (2026-10-16) : « éditer les
/// textures d'interface et de fond de terminaux ». Chacune a son rendu
/// (`texture_interface::ReglagesTexture`), sa composition (`texture_perso::
/// Composition`) et ses fichiers.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Cible {
    Interface,
    Terminaux,
}

impl Cible {
    fn rendu(self) -> &'static interface::ReglagesTexture {
        match self {
            Cible::Interface => &interface::INTERFACE,
            Cible::Terminaux => &interface::TERMINAUX,
        }
    }

    fn composition(self) -> &'static perso::Composition {
        match self {
            Cible::Interface => &perso::INTERFACE,
            Cible::Terminaux => &perso::TERMINAUX,
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    /// La texture en cours : son motif, et son rendu.
    fn texture(self) -> Texture {
        Texture { motif: self.composition().valeurs(), rendu: Some(self.rendu().rendu()) }
    }
}

thread_local! {
    static CIBLE: Cell<Cible> = const { Cell::new(Cible::Interface) };
    /// La texture de la bibliothèque chargée en dernier, par cible — celle que
    /// « Enregistrer » propose et à laquelle « modifiée » compare.
    static SOURCES: RefCell<[Option<String>; 2]> = const { RefCell::new([None, None]) };
}

fn source(cible: Cible) -> Option<String> {
    SOURCES.with(|s| s.borrow()[cible.index()].clone())
}

fn poser_source(cible: Cible, nom: Option<String>) {
    SOURCES.with(|s| s.borrow_mut()[cible.index()] = nom);
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskTexturesWindow"),
    window_title: w!("NeuraDesk — Textures"),
    html_file: "textures.html",
    wndproc: Some(wndproc),
    // PANNEAU — même famille que l'éditeur des lueurs.
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
    crate::win32::cef_lueurs::hide_if_visible();
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
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("textures")));
    PAYLOAD.with(|p| *p.borrow_mut() = None);
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, masquer_les_voisins);
    push_state();
}

/// Demande l'ouverture depuis un endroit qui n'a pas le device (les Réglages),
/// sur la texture des terminaux ou sur celle de l'interface.
pub fn demander_ouverture(terminaux: bool) {
    DEMANDE_TERMINAUX.store(terminaux, Ordering::Relaxed);
    DEMANDE.store(true, Ordering::Relaxed);
}

/// Vrai une fois par demande, et seulement si le panneau est FERMÉ : la boucle
/// l'ouvre alors par `toggle`. La cible demandée est posée ici.
pub fn prendre_demande() -> bool {
    if !DEMANDE.swap(false, Ordering::Relaxed) || cef_popup::is_visible(&SLOT) {
        return false;
    }
    let terminaux = DEMANDE_TERMINAUX.load(Ordering::Relaxed);
    CIBLE.with(|c| c.set(if terminaux { Cible::Terminaux } else { Cible::Interface }));
    true
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
    // ⭐ L'APERÇU D'UN FOND DE TERMINAL — DEMANDE EXPLICITE (2026-10-16) : la même
    // image que celle des terminaux (voir `win32::texture_terminaux`), seulement
    // quand on les édite : sinon chaque ouverture referait une image de l'écran.
    if CIBLE.with(Cell::get) == Cible::Terminaux {
        crate::win32::texture_terminaux::tenir_a_jour(&SLOT, "textures");
    }
}

/// La copie du back buffer pour le harnais (`NEURADESK_DEBUG_OPEN=textures`).
pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// Un `f32` tel qu'il s'ÉCRIT (`0.01`), et non élargi en `f64` par `json!`
/// (`0.009999999776482582`) : la page en tire le nombre de décimales du pas.
fn court(v: f32) -> f64 {
    v.to_string().parse().unwrap_or(0.0)
}

/// La texture en cours diffère-t-elle de `entree` ? Le rendu ne compte que si
/// l'entrée en a un (un préréglage garde celui en place).
fn est_modifiee(cible: Cible, entree: &Entree) -> bool {
    differe(&cible.texture(), &entree.texture)
}

/// Le corps de `est_modifiee`, PUR.
fn differe(en_cours: &Texture, enregistree: &Texture) -> bool {
    enregistree.motif != en_cours.motif || enregistree.rendu.is_some_and(|r| Some(r) != en_cours.rendu)
}

/// L'état de la page, pour la cible choisie : les réglages DÉCRITS (la page en
/// construit ses contrôles), leurs valeurs, les réglages communs en %, et la
/// bibliothèque.
fn etat_json() -> String {
    let cible = CIBLE.with(Cell::get);
    let rendu = cible.rendu();
    let valeurs = cible.composition().valeurs();
    let pct = |v: f32| (v * 100.0).round() as i32;
    let entrees = bibliotheque::entrees();
    // Sans texture chargée depuis le lancement : celle de la bibliothèque dont le
    // motif est EXACTEMENT le motif en cours, s'il y en a une.
    let source = match source(cible) {
        Some(nom) => entrees.iter().find(|e| e.nom == nom).cloned(),
        None => entrees.iter().find(|e| e.texture.motif == valeurs).cloned(),
    };
    serde_json::json!({
        "contentScale": CONTENT_SCALE.with(Cell::get),
        "terminaux": cible == Cible::Terminaux,
        "applique": rendu.nature_id() == NATURE_PERSO,
        "force": pct(rendu.force()),
        "echelle": pct(rendu.echelle()),
        "echelleMin": pct(interface::MIN_ECHELLE),
        "echelleMax": pct(interface::MAX_ECHELLE),
        "clarte": pct(rendu.clarte()),
        "teinte": rendu.teinte().round() as i32,
        "reglages": perso::REGLAGES.iter().zip(valeurs).map(|(r, v)| {
            let (sorte, options): (&str, &[&str]) = match r.sorte {
                Sorte::Case => ("case", &[]),
                Sorte::Curseur => ("curseur", &[]),
                Sorte::Choix(options) => ("choix", options),
            };
            serde_json::json!({
                "id": r.id, "libelle": r.libelle, "sorte": sorte, "options": options,
                "min": court(r.min), "max": court(r.max), "pas": court(r.pas), "valeur": court(v), "depend": r.depend,
            })
        }).collect::<Vec<_>>(),
        "bibliotheque": entrees.iter()
            .map(|e| serde_json::json!({ "nom": e.nom, "prereglage": e.prereglage, "enregistree": e.enregistree }))
            .collect::<Vec<_>>(),
        "source": source.as_ref().map(|e| serde_json::json!({
            "nom": e.nom, "prereglage": e.prereglage, "enregistree": e.enregistree, "modifiee": est_modifiee(cible, e),
        })),
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

/// **À APPELER À CHAQUE FRAME** — les Réglages changent aussi la texture.
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

/// **CE QUE LA PAGE DEMANDE.**
pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("textures/") else { continue };
        let morceaux: Vec<String> = reste.split('/').map(percent_decode).collect();
        let morceaux: Vec<&str> = morceaux.iter().map(String::as_str).collect();
        if let Some(Sauvegarde::Fermer) = appliquer(&morceaux) {
            cef_popup::fermer_par_geste(&SLOT);
        }
    }
}

/// Ce qu'un verbe demande APRÈS lui : quel fichier de la cible réécrire, ou
/// fermer. `Rien` : le verbe a fait son travail (la bibliothèque écrit elle-même).
#[derive(Debug, PartialEq)]
enum Sauvegarde {
    /// La composition.
    Perso,
    /// La nature et le rendu.
    Rendu,
    /// Les deux : charger une texture l'APPLIQUE aussi.
    Tout,
    Rien,
    Fermer,
}

/// **CHARGE** la texture `entree` sur `cible` : son motif, son rendu s'il en a un,
/// et la nature perso — une texture choisie se VOIT.
fn charger(cible: Cible, entree: &Entree) {
    cible.composition().remplacer(entree.texture.motif);
    if let Some(r) = entree.texture.rendu {
        cible.rendu().poser_rendu(r);
    }
    cible.rendu().set_nature_id(NATURE_PERSO);
    poser_source(cible, Some(entree.nom.clone()));
}

/// Charge la texture `nom` de la bibliothèque — `Tout` à réécrire, ou `None` si
/// elle n'existe pas.
fn charger_par_nom(cible: Cible, nom: &str) -> Option<Sauvegarde> {
    let Some(entree) = bibliotheque::trouver(nom) else {
        crate::diag!("[textures] texture introuvable : {nom:?}");
        return None;
    };
    charger(cible, &entree);
    Some(Sauvegarde::Tout)
}

/// Supprime le fichier de `nom` — la page ne le demande que pour la texture EN
/// COURS : un préréglage revient à l'ORIGINAL (« Restaurer »), une texture à moi
/// perd son nom.
fn supprimer(cible: Cible, nom: &str) -> Option<Sauvegarde> {
    if !bibliotheque::supprimer(nom) {
        crate::diag!("[textures] {nom:?} : aucun fichier à supprimer");
        return None;
    }
    match bibliotheque::trouver(nom) {
        Some(original) => {
            charger(cible, &original);
            Some(Sauvegarde::Tout)
        }
        None => {
            poser_source(cible, None);
            Some(Sauvegarde::Rien)
        }
    }
}

/// Un verbe de la page — séparé de `dispatch_actions` pour être testable : il
/// change l'état, et dit ce qui reste à faire (écrire un fichier, fermer). Les
/// fichiers sont écrits ICI, sauf sous `cfg(test)`.
fn appliquer(morceaux: &[&str]) -> Option<Sauvegarde> {
    let nombre = |t: &str| t.parse::<f32>().ok().filter(|v| v.is_finite());
    let cible = CIBLE.with(Cell::get);
    let rendu = cible.rendu();
    let suite = match morceaux {
        // Le `✕` de la page : il ferme MÊME un panneau épinglé.
        ["fermer"] => Some(Sauvegarde::Fermer),
        // L'interface (0) ou les terminaux (1).
        ["cible", i] => match *i {
            "0" | "1" => {
                CIBLE.with(|c| c.set(if *i == "1" { Cible::Terminaux } else { Cible::Interface }));
                Some(Sauvegarde::Rien)
            }
            _ => None,
        },
        // « Appliquer » : la nature perso, ou aucune texture.
        ["appliquer", flag] => {
            rendu.set_nature_id(if *flag == "1" { NATURE_PERSO } else { "aucune" });
            Some(Sauvegarde::Rendu)
        }
        ["reglage", id, valeur] => nombre(valeur).filter(|v| cible.composition().set(id, *v)).map(|_| Sauvegarde::Perso),
        ["reinitialiser"] => {
            cible.composition().reinitialiser();
            poser_source(cible, None);
            Some(Sauvegarde::Perso)
        }
        // Les réglages COMMUNS à toutes les natures, en % (la teinte en degrés).
        ["force", pct] => nombre(pct).map(|v| rendu.set_force(v / 100.0)).map(|_| Sauvegarde::Rendu),
        ["echelle", pct] => nombre(pct).map(|v| rendu.set_echelle(v / 100.0)).map(|_| Sauvegarde::Rendu),
        ["clarte", pct] => nombre(pct).map(|v| rendu.set_clarte(v / 100.0)).map(|_| Sauvegarde::Rendu),
        ["teinte", degres] => nombre(degres).map(|v| rendu.set_teinte(v)).map(|_| Sauvegarde::Rendu),
        // ⭐ LA BIBLIOTHÈQUE (voir `win32::textures_bibliotheque`).
        ["charger", nom] => charger_par_nom(cible, nom),
        ["enregistrer", nom] => bibliotheque::enregistrer(nom, &cible.texture()).map(|nom| {
            poser_source(cible, Some(nom));
            Sauvegarde::Rien
        }),
        ["supprimer", nom] => supprimer(cible, nom),
        // Les deux boîtes de dialogue s'ouvrent au-dessus de CE panneau (voir
        // `file_dialog::enregistrer_sous` : une boîte sans propriétaire peut
        // passer derrière, et l'app paraîtrait figée).
        ["exporter", nom] => cef_popup::hwnd(&SLOT).map(|hwnd| {
            bibliotheque::exporter(hwnd, nom, &cible.texture());
            Sauvegarde::Rien
        }),
        ["importer"] => cef_popup::hwnd(&SLOT)
            .and_then(bibliotheque::importer)
            .and_then(|nom| charger_par_nom(cible, &nom)),
        autre => {
            crate::diag!("[textures] verbe inconnu : {autre:?}");
            None
        }
    };
    if !cfg!(test) {
        if let Some(suite) = &suite {
            sauvegarder(cible, suite);
        }
    }
    suite
}

/// Réécrit les fichiers de `cible` que `suite` demande.
fn sauvegarder(cible: Cible, suite: &Sauvegarde) {
    let (composition, rendu) = match suite {
        Sauvegarde::Perso => (true, false),
        Sauvegarde::Rendu => (false, true),
        Sauvegarde::Tout => (true, true),
        Sauvegarde::Rien | Sauvegarde::Fermer => return,
    };
    match cible {
        Cible::Interface => {
            if composition {
                config::save_texture_perso();
            }
            if rendu {
                config::save_texture_interface();
            }
        }
        Cible::Terminaux => {
            if composition {
                config::save_texture_perso_terminaux();
            }
            if rendu {
                config::save_texture_terminaux();
            }
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

    const VERBES: [&str; 14] = [
        "fermer", "cible", "appliquer", "reglage", "reinitialiser", "force", "echelle", "clarte", "teinte",
        "charger", "enregistrer", "supprimer", "exporter", "importer",
    ];

    /// ⭐ **LA PAGE ET RUST PARLENT LE MÊME LANGAGE** — chaque `go("verbe"…)` de la
    /// page a son bras ici, et chaque bras est appelé par la page.
    #[test]
    fn les_verbes_de_la_page_sont_exactement_ceux_du_dispatch() {
        let page = include_str!("../../assets/popups/textures.html");
        let source = include_str!("cef_textures.rs");
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

    /// Ce que chaque verbe demande d'écrire — un réglage inconnu ou illisible
    /// n'écrit rien. ⚠️ Les valeurs sont GLOBALES : on ne relit que les RETOURS.
    /// ⛔ Aucun verbe de la bibliothèque ici : ils touchent `%APPDATA%`.
    #[test]
    fn chaque_verbe_dit_quel_fichier_reecrire() {
        assert_eq!(appliquer(&["fermer"]), Some(Sauvegarde::Fermer));
        assert_eq!(appliquer(&["reglage", "angle", "45"]), Some(Sauvegarde::Perso));
        assert_eq!(appliquer(&["reglage", "angle", "abc"]), None);
        assert_eq!(appliquer(&["reglage", "inconnu", "1"]), None);
        assert_eq!(appliquer(&["reglage", "angle", "0"]), Some(Sauvegarde::Perso));
        assert_eq!(appliquer(&["force", "40"]), Some(Sauvegarde::Rendu));
        assert_eq!(appliquer(&["n'importe", "quoi"]), None);
    }

    /// ⭐ La cible choisit les réglages ET la composition. `CIBLE` est propre au
    /// fil du test ; ⛔ on n'écrit pas dans `TERMINAUX` (la clé de l'image des
    /// terminaux est relue en parallèle par un autre test).
    #[test]
    fn la_cible_choisit_ce_que_l_editeur_habille() {
        assert_eq!(CIBLE.with(Cell::get), Cible::Interface, "l'interface par défaut");
        assert_eq!(appliquer(&["cible", "1"]), Some(Sauvegarde::Rien));
        assert_eq!(CIBLE.with(Cell::get), Cible::Terminaux);
        assert!(std::ptr::eq(Cible::Terminaux.composition(), &perso::TERMINAUX));
        assert!(std::ptr::eq(Cible::Terminaux.rendu(), &interface::TERMINAUX));
        assert_eq!(appliquer(&["cible", "7"]), None);
        assert_eq!(appliquer(&["cible", "0"]), Some(Sauvegarde::Rien));
        assert!(std::ptr::eq(Cible::Interface.composition(), &perso::INTERFACE));
        assert!(std::ptr::eq(Cible::Interface.rendu(), &interface::INTERFACE));
    }

    /// « Modifiée » : le motif compte toujours, le rendu seulement si la texture
    /// enregistrée en a un (un préréglage garde le rendu en place).
    #[test]
    fn modifiee_compare_le_motif_et_le_rendu_enregistre() {
        let motif = perso::motif_du_prereglage(&[]);
        let rendu = interface::Rendu { force: 0.4, echelle: 1.0, clarte: 0.0, teinte: 0.0 };
        let en_cours = Texture { motif, rendu: Some(rendu) };
        assert!(!differe(&en_cours, &Texture { motif, rendu: None }), "préréglage : le rendu ne compte pas");
        assert!(!differe(&en_cours, &en_cours.clone()));
        assert!(differe(&en_cours, &Texture { motif, rendu: Some(interface::Rendu { force: 0.8, ..rendu }) }));
        let mut autre = motif;
        autre[0] = 3.0;
        assert!(differe(&en_cours, &Texture { motif: autre, rendu: None }));
    }

    /// La page porte le `✕` que la punaise cherche, et n'a ni `<select>` ni
    /// `type=color` (rien ne s'y déroule en rendu hors écran).
    #[test]
    fn la_page_respecte_les_regles_des_panneaux() {
        let page = include_str!("../../assets/popups/textures.html");
        assert!(page.contains("class=\"close-btn\"") && page.contains("title=\"Fermer\""));
        assert!(!page.contains("<select") && !page.contains("type=\"color\""));
    }

    /// ⭐ L'APERÇU D'UN TERMINAL est peint COMME un terminal : les règles de fond
    /// de `.terminal-hote`, et la fonction qui reçoit l'image de la texture.
    #[test]
    fn l_apercu_est_peint_comme_un_terminal() {
        let page = include_str!("../../assets/popups/textures.html");
        let terminal = include_str!("../../assets/popups/terminal.html");
        assert!(page.contains("<script src=\"terminal_theme.js\"></script>"), "sans lui, l'image n'arrive jamais");
        let apercu = page.split("#apercu-terminal {").nth(1).expect("la règle de l'aperçu").split('}').next().unwrap();
        let hote = terminal.split(".terminal-hote {\n    padding").nth(1).expect("la règle du terminal").split('}').next().unwrap();
        for regle in ["background-color:", "background-image:", "background-size:", "background-attachment:"] {
            let ligne = |css: &str| css.lines().find(|l| l.contains(regle)).map(str::trim).map(str::to_owned);
            assert_eq!(ligne(apercu), ligne(hote), "{regle}");
        }
        let source = include_str!("cef_textures.rs");
        assert!(source.contains("texture_terminaux::tenir_a_jour(&SLOT, \"textures\")"));
    }

    /// Un pas de `0.001` arrive à la page tel quel : sinon elle en compterait
    /// vingt décimales, et les afficherait.
    #[test]
    fn les_nombres_arrivent_tels_qu_ils_s_ecrivent() {
        assert_eq!(serde_json::json!(court(0.001)).to_string(), "0.001");
        assert_eq!(serde_json::json!(court(1.2)).to_string(), "1.2");
        assert_eq!(serde_json::json!(court(4.0)).to_string(), "4.0");
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
            ("cef_lueurs", include_str!("cef_lueurs.rs")),
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
        let nous = include_str!("cef_textures.rs");
        for (nom, source) in voisins {
            assert!(nous.contains(&format!("crate::win32::{nom}::hide_if_visible();")), "nous ne masquons pas {nom}");
            assert!(source.contains("crate::win32::cef_textures::hide_if_visible()"), "{nom} ne nous masque pas");
        }
    }
}
