//! Le DASH de RECHERCHE — DEMANDE EXPLICITE (2026-10-04) : « une applet de
//! recherche qui n'apparaît que si Everything est installé, dans la zone
//! Intégrés », résultats « dans un dash » (voir `win32::everything`, le canal
//! vers Everything, et `assets/popups/recherche.html`).
//!
//! Un PANNEAU, comme la Calculatrice dont il reprend la structure : il remplit la
//! zone libre du dock et il a le droit d'émettre une action à chaque frappe.
//!
//! **LA PAGE NUMÉROTE SES RECHERCHES** : chaque réponse porte le numéro de la
//! recherche qui l'a demandée, et la page ignore celles qu'une frappe plus
//! récente a dépassées (le fil Everything, lui, saute déjà les recherches
//! empilées — voir `everything::demarrer_le_fil`).
//!
//! Actions reçues (`"recherche/<verbe>/…"`, segments ENCODÉS) :
//! - `ready` — la page a chargé (voir `PAYLOAD`)
//! - `close`
//! - `chercher/<numéro>/<tri>/<recherche>`
//! - `ouvrir/<chemin>`, `emplacement/<chemin>`, `copier/<chemin>`
//!
//! **LES ICÔNES (lot 2) PARTENT À PART, PAR LOTS** — comme celles du sous-dock
//! (voir `cef_sous_dock::envoyer_des_icones`) : chaque résultat porte une CLÉ
//! d'icône (`cle_d_icone`), et la page reçoit chaque clé UNE fois
//! (`window.recherche.icones`). Une clé par TYPE (`ext:pdf`, `dossier`) : deux
//! cents PDF coûtent une seule extraction. Seuls les `.exe`, `.lnk`, `.ico` et
//! `.url` d'un disque LOCAL ont une clé par fichier (leur icône est la leur).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::everything::{self, Tri};

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// La clé du dernier `window.setState(...)` émis — voir
    /// `cef_minuteur::PAYLOAD` : vidée à l'ouverture ET au `ready`.
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Les icônes déjà extraites, par clé (`None` : le shell n'en a pas donné).
    static ICONES: RefCell<HashMap<String, Option<String>>> = RefCell::new(HashMap::new());
    /// Les clés déjà envoyées à la PAGE — vidé à son `ready` (rechargée, elle a
    /// tout oublié).
    static ICONES_ENVOYEES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// Les clés à envoyer, dans l'ordre des résultats (les premiers d'abord).
    static ICONES_A_ENVOYER: RefCell<VecDeque<String>> = const { RefCell::new(VecDeque::new()) };
}

/// Le côté des icônes envoyées, en pixels — de quoi rester net au zoom ×3 des
/// pages (`cef_popup::CONTENT_ZOOM_FACTOR`) pour une icône affichée à 20 px CSS.
const COTE_ICONE: u32 = 64;
/// Combien d'icônes partent par frame — même raison que
/// `cef_sous_dock::ICONES_PAR_FRAME` : répartir le poids, pas le réduire.
const ICONES_PAR_FRAME: usize = 12;
/// Les extensions dont l'icône est PROPRE à chaque fichier.
const ICONE_PROPRE: [&str; 4] = ["exe", "lnk", "ico", "url"];

/// **LA CLÉ D'ICÔNE D'UN RÉSULTAT** — et, en même temps, ce qu'il faut demander
/// au shell pour l'obtenir.
///
/// - un dossier : `dossier` ;
/// - un `.exe`, `.lnk`, `.ico`, `.url` sur un disque LOCAL : `fichier:<chemin>` ;
/// - tout le reste : `ext:<extension en minuscules>` (`ext:` seul sans extension).
///
/// `local` dit si le chemin est sur un disque local — séparé pour être testable
/// sans dépendre des lecteurs de la machine.
fn cle_d_icone(r: &everything::Resultat, local: impl Fn(&str) -> bool) -> String {
    if r.est_dossier {
        return "dossier".to_string();
    }
    let extension = Path::new(&r.nom).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let chemin = r.chemin().to_string_lossy().into_owned();
    if ICONE_PROPRE.contains(&extension.as_str()) && local(&chemin) {
        format!("fichier:{chemin}")
    } else {
        format!("ext:{extension}")
    }
}

/// Le chemin est-il sur un disque LOCAL fixe ? Un partage UNC, un lecteur réseau
/// ou amovible ne l'est pas : on ne lit pas ces fichiers-là pour une icône.
fn est_local(chemin: &str) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    const DRIVE_FIXED: u32 = 3;
    let Some(racine) = chemin.get(..3).filter(|r| r.ends_with(":\\")) else { return false };
    let large: Vec<u16> = racine.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY : `large` est terminée par un zéro et vit pendant l'appel.
    unsafe { GetDriveTypeW(PCWSTR(large.as_ptr())) == DRIVE_FIXED }
}

/// L'icône d'une clé, en `data:` URI — extraite une fois, puis gardée.
fn icone(cle: &str) -> Option<String> {
    if let Some(trouvee) = ICONES.with(|i| i.borrow().get(cle).cloned()) {
        return trouvee;
    }
    let rgba = if cle == "dossier" {
        crate::win32::cef_dash::type_icon_rgba("dossier", true, COTE_ICONE)
    } else if let Some(chemin) = cle.strip_prefix("fichier:") {
        crate::win32::cef_dash::file_icon_rgba(Path::new(chemin), COTE_ICONE)
    } else {
        let extension = cle.strip_prefix("ext:").unwrap_or_default();
        let nom = if extension.is_empty() { "fichier".to_string() } else { format!("fichier.{extension}") };
        crate::win32::cef_dash::type_icon_rgba(&nom, false, COTE_ICONE)
    };
    let uri = rgba.and_then(|(l, h, octets)| crate::win32::cef_dash::encode_rgba_to_data_uri(l, h, &octets));
    ICONES.with(|i| i.borrow_mut().insert(cle.to_string(), uri.clone()));
    uri
}

fn mettre_en_file(cles: &[String]) {
    ICONES_ENVOYEES.with(|envoyees| {
        let mut envoyees = envoyees.borrow_mut();
        ICONES_A_ENVOYER.with(|file| {
            let mut file = file.borrow_mut();
            for cle in cles {
                if envoyees.insert(cle.clone()) {
                    file.push_back(cle.clone());
                }
            }
        });
    });
}

/// Un lot d'icônes vers la page — à chaque frame, jusqu'à épuisement de la file.
/// Les paires partent en JSON (`serde_json`) : un chemin porte des antislashs.
fn envoyer_des_icones() {
    let lot: Vec<String> = ICONES_A_ENVOYER.with(|file| {
        let mut file = file.borrow_mut();
        (0..ICONES_PAR_FRAME).filter_map(|_| file.pop_front()).collect()
    });
    if lot.is_empty() {
        return;
    }
    let paires: Vec<serde_json::Value> = lot.iter().map(|cle| json!([cle, icone(cle)])).collect();
    cef_popup::execute_js(&SLOT, &format!("window.recherche && window.recherche.icones({})", serde_json::Value::Array(paires)));
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefRechercheWindow"),
    window_title: w!("NeuraDesk — Recherche"),
    html_file: "recherche.html",
    wndproc: Some(wndproc),
    // PANNEAU — même famille que la calculatrice.
    above_a_magnified_dock: false,
    fades: false,
    fenetre: false,
};

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

fn masquer_les_voisins() {
    // Même « zone libre » que le Dash — même liste que `cef_calculatrice`, plus
    // la Calculatrice elle-même.
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
    crate::win32::bandeau::hide_if_visible();
    // ⚠️ **AJOUTÉE À PART DES DOUZE AUTRES** : la Météo est arrivée après cette
    // liste, donc son cas ne se déduit pas de `cef_automatisations` — qui, lui
    // aussi, manquait ici (voir `cef_automatisations::masquer_les_voisins`, qui
    // nous nomme sans que nous le nommions).
    crate::win32::cef_automatisations::hide_if_visible();
    crate::win32::cef_meteo::hide_if_visible();
    crate::win32::cef_aide::hide_if_visible();
}

pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("recherche")));
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, masquer_les_voisins);
    push_state();
    if cef_popup::is_visible(&SLOT) {
        // Le champ prend le focus et sa saisie précédente est sélectionnée :
        // on retape par-dessus, ou on garde la recherche d'avant.
        cef_popup::execute_js(&SLOT, "window.recherche && window.recherche.montre()");
    }
}

/// Repousse l'état si la fenêtre est visible — à chaque frame (voir
/// `app::run`) ; `PAYLOAD` rend l'appel gratuit tant que rien ne change.
pub fn refresh_if_visible() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

fn push_state() {
    let payload = json!({ "contentScale": CONTENT_SCALE.with(Cell::get) }).to_string();
    if PAYLOAD.with(|p| p.borrow().as_deref() == Some(payload.as_str())) {
        return;
    }
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
    PAYLOAD.with(|p| *p.borrow_mut() = Some(payload));
}

/// Les réponses d'Everything, versées dans la page — **à chaque frame, panneau
/// visible ou non** (une réponse arrivée pendant qu'il se fermait n'a pas à
/// rester dans le canal). Le JSON passe ENTIER par `serde_json` : des chemins
/// Windows écrits à la main dans un littéral JS y perdraient leurs antislashs.
fn verser_les_reponses() {
    for reponse in everything::prendre_reponses() {
        let charge = match reponse {
            Ok(r) => {
                let cles: Vec<String> = r.resultats.iter().map(|x| cle_d_icone(x, est_local)).collect();
                mettre_en_file(&cles);
                let resultats: Vec<serde_json::Value> = r
                    .resultats
                    .iter()
                    .zip(&cles)
                    .map(|(x, cle)| {
                        json!({
                            "icone": cle,
                            "nom": x.nom,
                            "dossier": x.dossier_parent,
                            "chemin": x.chemin().to_string_lossy(),
                            "estDossier": x.est_dossier,
                            "taille": x.taille,
                            "modifie": x.modifie_ms,
                        })
                    })
                    .collect();
                json!({ "numero": r.numero, "total": r.total, "resultats": resultats })
            }
            Err((numero, message)) => json!({ "numero": numero, "erreur": message }),
        };
        cef_popup::execute_js(&SLOT, &format!("window.recherche && window.recherche.reponse({charge})"));
    }
}

fn ouvrir(chemin: &str) {
    let chemin = Path::new(chemin);
    if !chemin.exists() {
        crate::diag!("[recherche] l'élément n'existe plus : {}", chemin.display());
        return;
    }
    // Comme un double-clic dans l'Explorateur (voir `appicon::launch`).
    crate::win32::appicon::launch(chemin);
    cef_popup::hide_if_visible(&SLOT);
}

pub fn dispatch_actions() {
    verser_les_reponses();
    envoyer_des_icones();
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("recherche/") else {
            crate::diag!("[recherche] action inconnue : {action:?}");
            continue;
        };
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["ready"] => {
                PAYLOAD.with(|c| *c.borrow_mut() = None);
                // Rechargée, la page a oublié toutes ses icônes.
                ICONES_ENVOYEES.with(|e| e.borrow_mut().clear());
                ICONES_A_ENVOYER.with(|f| f.borrow_mut().clear());
            }
            ["close"] => cef_popup::fermer_par_geste(&SLOT),
            // La page encode le « / » d'une recherche (un chemin, par exemple) ;
            // s'il arrivait décodé en route, les morceaux se recollent ici.
            ["chercher", numero, tri, morceaux @ ..] if !morceaux.is_empty() => {
                match (numero.parse::<u32>(), Tri::depuis_nom(tri)) {
                    (Ok(numero), Some(tri)) => everything::chercher(numero, morceaux.join("/"), tri),
                    _ => crate::diag!("[recherche] recherche illisible : {action:?}"),
                }
            }
            ["ouvrir", morceaux @ ..] if !morceaux.is_empty() => ouvrir(&morceaux.join("/")),
            ["emplacement", morceaux @ ..] if !morceaux.is_empty() => {
                everything::ouvrir_l_emplacement(Path::new(&morceaux.join("/")));
                cef_popup::hide_if_visible(&SLOT);
            }
            ["copier", morceaux @ ..] if !morceaux.is_empty() => {
                // Le booléen est ignoré : le presse-papiers occupé est banal.
                let _ = crate::win32::clipboard::write_text(&morceaux.join("/"));
            }
            _ => crate::diag!("[recherche] action inconnue : {action:?}"),
        }
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

/// Voir `win32::cef_calendar_applet::debug_dump_backbuffer` — le harnais a
/// DEUX listes à tenir (`NEURADESK_DEBUG_OPEN=recherche`).
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
mod tests {
    const PAGE: &str = include_str!("../../assets/popups/recherche.html");

    /// Les verbes que `dispatch_actions` accepte — même lecture que
    /// `cef_calculatrice::tests`.
    fn verbes_du_dispatch() -> Vec<String> {
        let source = include_str!("cef_recherche.rs");
        source
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
            .collect()
    }

    fn verbes_de_la_page() -> Vec<String> {
        PAGE.match_indices("go(")
            .filter_map(|(i, _)| {
                let reste = &PAGE[i + 3..];
                let guillemet = reste.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                reste[1..].split(guillemet).next().map(str::to_string)
            })
            .collect()
    }

    #[test]
    fn la_page_et_le_dispatch_acceptent_les_memes_verbes() {
        let mut page = verbes_de_la_page();
        let mut rust = verbes_du_dispatch();
        page.sort();
        page.dedup();
        rust.sort();
        rust.dedup();
        assert_eq!(page, rust);
    }

    /// Les fonctions que Rust appelle dans la page doivent y exister — sinon les
    /// résultats se perdraient EN SILENCE.
    #[test]
    fn la_page_definit_ce_que_rust_appelle() {
        for fonction in ["reponse(", "montre(", "icones(", "window.setState"] {
            assert!(PAGE.contains(fonction), "{fonction} absente de recherche.html");
        }
    }

    fn resultat(nom: &str, dossier: &str, est_dossier: bool) -> crate::win32::everything::Resultat {
        crate::win32::everything::Resultat {
            nom: nom.into(),
            dossier_parent: dossier.into(),
            est_dossier,
            taille: None,
            modifie_ms: None,
        }
    }

    /// Une clé par TYPE, sauf pour les fichiers qui portent LEUR icône sur un
    /// disque local ; jamais de lecture de fichier pour un chemin réseau.
    #[test]
    fn la_cle_d_icone_suit_le_type_et_le_disque() {
        use super::cle_d_icone;
        let local = |c: &str| c.starts_with("C:");
        assert_eq!(cle_d_icone(&resultat("Projets", r"C:\", true), local), "dossier");
        assert_eq!(cle_d_icone(&resultat("Facture.PDF", r"C:\Docs", false), local), "ext:pdf");
        assert_eq!(cle_d_icone(&resultat("LISEZMOI", r"C:\Docs", false), local), "ext:");
        assert_eq!(cle_d_icone(&resultat("outil.exe", r"C:\Outils", false), local), r"fichier:C:\Outils\outil.exe");
        assert_eq!(cle_d_icone(&resultat("outil.exe", r"\\nas\partage", false), local), "ext:exe", "réseau : pas de lecture");
    }

    /// Le shell rend bien une icône de TYPE sans fichier réel — un PDF et un
    /// dossier, à la taille demandée.
    #[test]
    fn le_shell_rend_une_icone_de_type_sans_fichier() {
        for (nom, dossier) in [("fichier.pdf", false), ("dossier", true), ("fichier.extension-inconnue", false)] {
            let (l, h, rgba) = crate::win32::cef_dash::type_icon_rgba(nom, dossier, super::COTE_ICONE)
                .unwrap_or_else(|| panic!("aucune icône pour {nom}"));
            assert!(l <= super::COTE_ICONE && h <= super::COTE_ICONE && l > 0 && h > 0, "{nom} : {l}×{h}");
            assert_eq!(rgba.len(), (l * h * 4) as usize);
            assert!(rgba.chunks_exact(4).any(|p| p[3] > 0), "{nom} : icône entièrement transparente");
        }
    }
}
