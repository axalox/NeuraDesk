//! Panneau « Terminal » (CEF) — DEMANDE EXPLICITE (2026-09-27) : « déplacer
//! l'applet terminal dans la zone Intégrés et que ce soit un dash avec le
//! terminal à l'intérieur ». Remplace le terminal déroulant (une fenêtre de
//! console manipulée de l'extérieur) : des onglets xterm.js branchés sur des
//! shells locaux sous pseudo-console (voir `win32::pty`). Même modèle que le
//! terminal de l'applet SSH (voir `win32::cef_ssh`).
//!
//! ⚠️ La page est PRÉCHARGÉE au démarrage : elle n'ouvre son premier shell
//! qu'à `montre()`, appelé quand le panneau devient visible — pas de shell
//! lancé pour rien à chaque démarrage.
//!
//! Actions (`neuradesk://terminal/…`) : `ready`, `close`,
//! `ouvrir/<onglet>/<shell>/<colonnes>/<lignes>`,
//! `ouvrir-admin/<onglet>/<shell>/<colonnes>/<lignes>` (onglet administrateur,
//! après l'UAC — voir `pty::ouvrir_admin`),
//! `redimensionner/<onglet>/<colonnes>/<lignes>`, `fermer/<onglet>`,
//! `copier` (texte déposé avant par POST `terminal/copie`), `coller/<onglet>`,
//! `commandes` (la liste déposée avant par POST `terminal/commandes` — voir
//! `win32::commandes`).
//! Les FRAPPES partent par POST `terminal/ecrire/<onglet>` (voir
//! `win32::cef_data::route_terminal`) — jamais par une action.

use std::cell::{Cell, RefCell};

use base64::Engine;
use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_data;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::pty::{self, Evenement, Shell};

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// L'échelle d'ÉCRAN reçue au dernier `toggle` (sans « Taille du contenu ») :
    /// la vue sphère rouvre le panneau avec elle (voir `win32::cef_sphere`).
    static ECHELLE_ECRAN: Cell<f32> = const { Cell::new(1.0) };
    /// La clé du dernier `window.setState(...)` émis — voir
    /// `cef_minuteur::PAYLOAD` : vidée à l'ouverture ET au `ready`.
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefTerminalWindow"),
    window_title: w!("NeuraDesk — Terminal"),
    html_file: "terminal.html",
    wndproc: Some(wndproc),
    above_a_magnified_dock: false,
    fades: false,
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
    crate::win32::cef_minuteur::hide_if_visible();
    crate::win32::cef_calculatrice::hide_if_visible();
    crate::win32::cef_recherche::hide_if_visible();
    crate::win32::cef_automatisations::hide_if_visible();
    crate::win32::cef_meteo::hide_if_visible();
    crate::win32::cef_aide::hide_if_visible();
    crate::win32::bandeau::hide_if_visible();
    crate::win32::cef_ssh::hide_if_visible();
}

pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    ECHELLE_ECRAN.with(|c| c.set(content_scale));
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("terminal")));
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, masquer_les_voisins);
    push_state();
    if cef_popup::is_visible(&SLOT) {
        appeler("montre", &[]);
    }
}

/// L'échelle d'écran du dernier `toggle` — voir [`ECHELLE_ECRAN`].
pub fn echelle_d_ecran() -> f32 {
    ECHELLE_ECRAN.with(Cell::get)
}

/// LA VUE SPHÈRE de l'autre panneau bascule ici (voir `win32::cef_sphere`) :
/// le panneau se montre, sa sphère ouverte sur `onglet`.
pub fn montrer_en_sphere(rect: [i32; 4], gfx: &GfxDevice, onglet: u32) {
    if !cef_popup::is_visible(&SLOT) {
        toggle(rect, gfx, echelle_d_ecran());
    }
    appeler("sphereOuvrir", &[json!(onglet)]);
}

/// Les aperçus des sessions de l'autre panneau, pour la sphère de celui-ci.
pub fn sphere_autres(apercus: &serde_json::Value) {
    appeler("sphereAutres", &[apercus.clone()]);
}

pub fn is_visible() -> bool {
    cef_popup::is_visible(&SLOT)
}

/// Voir `cef_popup::donner_le_clavier` (vue sphère).
pub fn donner_le_clavier() -> bool {
    cef_popup::donner_le_clavier(&SLOT)
}

/// La sphère redemande les vignettes de la page, panneau visible ou non (voir
/// `assets/popups/sphere_source.js`).
pub fn demander_vignettes() {
    cef_popup::execute_js(&SLOT, "window.sphereSource && window.sphereSource.envoyer()");
}

pub fn refresh_if_visible() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

fn push_state() {
    let shells: Vec<serde_json::Value> =
        pty::shells().into_iter().map(|s| json!({ "id": s.id(), "nom": s.nom() })).collect();
    let payload = json!({
        "shells": shells,
        "commandes": crate::win32::commandes::json(),
        "contentScale": CONTENT_SCALE.with(Cell::get),
        // LA POLICE DES TERMINAUX, lue ICI à chaque push (`""` = la pile par
        // défaut de `terminal_theme.js`) : `refresh_if_visible` rappelle
        // `push_state` tant que le panneau est ouvert, donc un clic dans
        // Réglages atteint un terminal DÉJÀ ouvert sans le relancer. Le nom est
        // déjà validé côté Rust (`ui::style::police_valide`), la page ne fait
        // que le quoter.
        "policeTerminal": crate::ui::style::police_des_terminaux().unwrap_or_default(),
        // Le raccourci de la vue sphère (voir `win32::cef_sphere::raccourci`).
        "raccourci": crate::win32::cef_sphere::raccourci(),
        // La vitesse du zoom à la molette, en % (réglée dans Réglages).
        "sphereZoom": crate::win32::cef_sphere::vitesse_zoom(),
        // Les réglages PROPRES à la sphère (voir `win32::cef_sphere`) : distance
        // d'ouverture, recadrage automatique, et fondu du panneau.
        "sphereZoomDefaut": crate::win32::cef_sphere::zoom_defaut(),
        "sphereZoomAuto": crate::win32::cef_sphere::zoom_auto(),
        "sphereFonduOuverture": crate::win32::cef_sphere::fondu().0,
        "sphereFonduFermeture": crate::win32::cef_sphere::fondu().1,
    }).to_string();
    if PAYLOAD.with(|p| p.borrow().as_deref() == Some(payload.as_str())) {
        return;
    }
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
    PAYLOAD.with(|p| *p.borrow_mut() = Some(payload));
}

fn appeler(fonction: &str, arguments: &[serde_json::Value]) {
    let arguments: Vec<String> = arguments.iter().map(serde_json::Value::to_string).collect();
    cef_popup::execute_js(&SLOT, &format!("window.terminal && window.terminal.{fonction}({})", arguments.join(",")));
}

/// Verse la sortie des shells dans la page — **À CHAQUE FRAME, PANNEAU
/// VISIBLE OU NON** (voir `cef_ssh::relayer_evenements`). Les sorties
/// consécutives d'un même onglet partent en UN appel.
fn relayer_evenements() {
    let mut sortie: Option<(u32, Vec<u8>)> = None;
    let vider = |sortie: &mut Option<(u32, Vec<u8>)>| {
        if let Some((onglet, octets)) = sortie.take() {
            let b64 = base64::engine::general_purpose::STANDARD.encode(octets);
            appeler("sortie", &[json!(onglet), json!(b64)]);
        }
    };
    for evenement in pty::prendre_evenements() {
        match evenement {
            Evenement::Sortie { onglet, octets } => match &mut sortie {
                Some((o, tampon)) if *o == onglet => tampon.extend_from_slice(&octets),
                _ => {
                    vider(&mut sortie);
                    sortie = Some((onglet, octets));
                }
            },
            Evenement::Fin { onglet } => {
                vider(&mut sortie);
                appeler("fin", &[json!(onglet)]);
            }
            Evenement::Pret { onglet } => {
                vider(&mut sortie);
                appeler("pret", &[json!(onglet)]);
            }
            Evenement::Erreur { onglet, message } => {
                vider(&mut sortie);
                crate::diag!("[terminal] onglet administrateur {onglet} : {message}");
                appeler("erreur", &[json!(onglet), json!(message)]);
            }
        }
    }
    vider(&mut sortie);
}

fn nombre(texte: &str) -> Option<u32> {
    texte.parse().ok()
}

fn ouvrir(onglet: u32, shell: &str, colonnes: u32, lignes: u32) {
    let resultat = Shell::depuis_id(shell)
        .ok_or_else(|| format!("Shell inconnu : {shell}"))
        .and_then(|shell| pty::ouvrir(onglet, shell, colonnes, lignes));
    if let Err(message) = resultat {
        crate::diag!("[terminal] onglet {onglet} : {message}");
        appeler("erreur", &[json!(onglet), json!(message)]);
    }
}

pub fn dispatch_actions() {
    relayer_evenements();
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("terminal/") else {
            crate::diag!("[terminal] action inconnue : {action:?}");
            continue;
        };
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["ready"] => {
                PAYLOAD.with(|c| *c.borrow_mut() = None);
                push_state();
                // Ouvert avant la fin du chargement : le `montre` de `toggle`
                // s'est perdu.
                if cef_popup::is_visible(&SLOT) {
                    appeler("montre", &[]);
                }
            }
            ["close"] => cef_popup::fermer_par_geste(&SLOT),
            // LA VUE SPHÈRE (voir `win32::cef_sphere`) : le clic molette ou
            // l'indicateur de la barre ; les vignettes déposées avant par POST.
            ["sphere-etat", etat] => crate::win32::cef_sphere::etat(crate::win32::cef_sphere::TERMINAL, *etat == "oui"),
            ["sphere-autre", onglet] => match nombre(onglet) {
                Some(o) => crate::win32::cef_sphere::basculer_depuis(crate::win32::cef_sphere::TERMINAL, o),
                None => crate::diag!("[terminal] onglet illisible : {action:?}"),
            },
            ["raccourci", valeur] => crate::win32::cef_sphere::regler_raccourci(valeur),
            ["vignettes"] => crate::win32::cef_sphere::recevoir(
                crate::win32::cef_sphere::TERMINAL,
                cef_data::prendre("terminal/vignettes"),
            ),
            ["commandes"] => crate::win32::commandes::remplacer_depuis("terminal/commandes"),
            ["commandes-exporter"] => {
                if let Some(h) = cef_popup::hwnd(&SLOT) { crate::win32::commandes::exporter(h) }
            }
            ["commandes-importer"] => {
                if let Some(h) = cef_popup::hwnd(&SLOT) { crate::win32::commandes::importer(h) }
            }
            ["ouvrir", onglet, shell, colonnes, lignes] => {
                match (nombre(onglet), nombre(colonnes), nombre(lignes)) {
                    (Some(o), Some(c), Some(l)) => ouvrir(o, shell, c, l),
                    _ => crate::diag!("[terminal] ouverture illisible : {action:?}"),
                }
            }
            ["ouvrir-admin", onglet, shell, colonnes, lignes] => {
                match (nombre(onglet), Shell::depuis_id(shell), nombre(colonnes), nombre(lignes)) {
                    (Some(o), Some(s), Some(c), Some(l)) => pty::ouvrir_admin(o, s, c, l),
                    _ => crate::diag!("[terminal] ouverture administrateur illisible : {action:?}"),
                }
            }
            ["redimensionner", onglet, colonnes, lignes] => match (nombre(onglet), nombre(colonnes), nombre(lignes)) {
                (Some(o), Some(c), Some(l)) => pty::redimensionner(o, c, l),
                _ => crate::diag!("[terminal] taille illisible : {action:?}"),
            },
            ["fermer", onglet] => match nombre(onglet) {
                Some(o) => pty::fermer(o),
                None => crate::diag!("[terminal] onglet illisible : {action:?}"),
            },
            ["copier"] => match cef_data::prendre("terminal/copie").and_then(|octets| String::from_utf8(octets).ok()) {
                Some(texte) if !texte.is_empty() => {
                    // Le booléen est ignoré : ce panneau n'a pas d'endroit où
                    // dire l'échec (le presse-papiers occupé est banal).
                    let _ = crate::win32::clipboard::write_text(&texte);
                }
                _ => {}
            },
            ["coller", onglet] => match (nombre(onglet), crate::win32::clipboard::read_text()) {
                (Some(o), Some(texte)) => appeler("coller", &[json!(o), json!(texte)]),
                (None, _) => crate::diag!("[terminal] onglet illisible : {action:?}"),
                (Some(_), None) => {}
            },
            _ => crate::diag!("[terminal] action inconnue : {action:?}"),
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
    // La texture de SES terminaux (voir `win32::texture_terminaux`).
    crate::win32::texture_terminaux::tenir_a_jour(&SLOT, "terminal");
}

/// Voir `win32::cef_calendar_applet::debug_dump_backbuffer`
/// (`NEURADESK_DEBUG_OPEN=terminal`).
pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// Clic SOURIS CEF direct sur la fenêtre du Terminal — réservé au harnais (voir
/// `cef_popup::debug_click`, aucune entrée simulée à l'os). DEMANDE du
/// 2026-10-28 : « teste [Nordwand Mono] dans les terminaux ».
///
/// ⚠️ CE CLIC N'EST **PAS** CE QUI DONNE DU TEXTE À PHOTOGRAPHIER : le texte
/// attend un appel de `window.terminal.montre()`, qui ouvre le premier onglet de
/// `state.shells` quand `sessions` est vide (`assets/popups/terminal.html`). Rust
/// ne l'appelle que dans DEUX instants — `toggle` si la fenêtre est déjà visible,
/// et le bras `["ready"]` si elle l'est à ce moment-là. Mesuré le 2026-10-28 dans
/// un run du harnais : `ready` à 10,18 s, la fenêtre montée à 11,81 s, donc les
/// deux occasions manquées, et **rien ne rappelle `montre` ensuite** — la page ne
/// peint que sa barre d'onglets pendant tout le run. (`pty::shells()` est la LISTE
/// DES SORTES disponibles, pas celle des shells lancés — c'est bien ce que `montre`
/// utilise.) Le harnais s'en sort en appelant lui-même `montre()` depuis une copie
/// de la page ; le clic, lui, vise les puces, alignées à DROITE de la barre.
///
/// Ce levier reste utile pour les gestes qu'aucun automate ne fait tout seul :
/// cliquer une puce d'une autre sorte, un onglet, le ✕ d'un onglet.
pub fn debug_click(x: i32, y: i32) {
    cef_popup::debug_click(&SLOT, x, y);
}

/// Un clic MOLETTE, même usage (`NEURADESK_DEBUG_BOUTON=milieu`) — la vue sphère.
pub fn debug_middle_click(x: i32, y: i32) {
    cef_popup::debug_middle_click(&SLOT, x, y);
}

/// Un GLISSEMENT qui sélectionne (`NEURADESK_DEBUG_GLISSE`) puis un clic DROIT
/// (`NEURADESK_DEBUG_DROIT`) : la sélection et le menu Copier / Coller d'un
/// terminal de la vue sphère, sans souris (voir `app::run`).
pub fn debug_glisse(x0: i32, y0: i32, x1: i32, y1: i32) {
    cef_popup::debug_drag_select(&SLOT, x0, y0, x1, y1);
}

pub fn debug_clic_droit(x: i32, y: i32) {
    cef_popup::debug_right_click(&SLOT, x, y);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(result) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    const PAGE: &str = include_str!("../../assets/popups/terminal.html");

    /// ⭐ DEMANDE EXPLICITE (2026-10-15) : « que les terminaux aient des fonds
    /// colorés utilisant le thème plutôt que noir ». Le Terminal ET le SSH : plus
    /// de voile noir sous le texte, la surface du thème (`--popup-tint-rgb`,
    /// poussée par `cef_popup`) à la place, et un thème xterm partagé qui suit
    /// le texte du thème (sinon : texte blanc sur une surface claire).
    /// ⭐ LA TEXTURE DES TERMINAUX (voir `win32::texture_terminaux`) : une image
    /// posée par-dessus la COULEUR de fond, dans les deux pages. La couleur d'un
    /// terminal se pose en `backgroundColor` : le raccourci `background`
    /// effacerait l'image.
    #[test]
    fn la_texture_se_pose_sur_le_fond_des_terminaux() {
        let ssh = include_str!("../../assets/popups/ssh.html");
        for (nom, page) in [("terminal", PAGE), ("ssh", ssh)] {
            let regle = page.split(".terminal-hote {").nth(1).and_then(|r| r.split('}').next()).unwrap_or("");
            assert!(regle.contains("background-color:"), "{nom} : une couleur, pas le raccourci");
            assert!(regle.contains("background-image: var(--texture-terminaux, none)"), "{nom} : l'image de la texture");
            assert!(regle.contains("background-attachment: fixed"), "{nom} : continue d'un terminal à l'autre");
            assert!(!page.contains("hote.style.background ="), "{nom} : le raccourci effacerait l'image");
        }
        let theme = include_str!("../../assets/popups/terminal_theme.js");
        assert!(theme.contains("window.poserTextureDesTerminaux = function"), "la page reçoit l'image");
        assert!(theme.contains("devicePixelRatio"), "à l'échelle des pixels physiques");
    }

    #[test]
    fn les_terminaux_prennent_la_surface_du_theme() {
        let ssh = include_str!("../../assets/popups/ssh.html");
        for (nom, page) in [("terminal", PAGE), ("ssh", ssh)] {
            let regle = page.split(".terminal-hote {").nth(1).and_then(|r| r.split('}').next()).unwrap_or("");
            assert!(!regle.contains("rgba(0, 0, 0"), "{nom} : voile noir sous le terminal");
            assert!(regle.contains("--popup-tint-rgb"), "{nom} : pas la surface du thème");
            assert!(page.contains("<script src=\"terminal_theme.js\"></script>"), "{nom} : thème xterm absent");
            assert!(page.contains("suivreLeThemeDesTerminaux("), "{nom} : le thème xterm ne suit pas");
        }
        let js = crate::win32::cef_popup::window_palette_js([0.2, 0.4, 0.8], true, [0.1, 0.3, 0.2]);
        assert!(js.contains("'--popup-tint-rgb','26, 77, 51'"), "{js}");
    }

    /// ⭐ DEMANDE EXPLICITE (2026-10-15) : « que le nom personnalisé des
    /// terminaux s'affiche en gras » et « choisir une couleur personnalisée
    /// pour les terminaux en utilisant Ctrl + clic droit sur un fond de terminal »
    /// — puis « mets la même palette de couleurs que pour les lueurs » : les deux
    /// pages lisent `nuancier.js`, et rien d'autre.
    #[test]
    fn nom_en_gras_et_palette_des_lueurs() {
        // Le nom PERSONNALISÉ (pas le titre du shell) porte la classe `perso`, en gras.
        assert!(PAGE.contains(".perso { font-weight: 700; }"), "pas de règle de gras");
        assert!(PAGE.contains("s.nom ? \"texte perso\" : \"texte\""), "l'étiquette ne marque pas le nom");
        assert!(PAGE.contains("el(\"span\", { class: \"nom perso\" }, [s.nom])"), "l'onglet ne marque pas le nom");
        // Ctrl + clic droit ouvre la palette ; le clic droit seul garde copier/coller.
        assert!(PAGE.contains("if (e.ctrlKey) { proposerCouleur(s, e); return; }"), "Ctrl + clic droit");

        // LA MÊME SOURCE : le nuancier partagé, chargé AVANT le thème des terminaux,
        // et celui des lueurs ne garde plus de copie à lui.
        let lueurs = include_str!("../../assets/popups/lueurs.html");
        assert!(lueurs.contains("<script src=\"nuancier.js\"></script>"), "lueurs.html ne charge pas le nuancier");
        assert!(!lueurs.contains("const NUANCIER = ["), "lueurs.html garde sa propre copie");
        let charge = PAGE.find("<script src=\"nuancier.js\"></script>").expect("terminal.html ne charge pas le nuancier");
        assert!(charge < PAGE.find("<script src=\"terminal_theme.js\"></script>").unwrap(), "chargé APRÈS le thème");
        let theme = include_str!("../../assets/popups/terminal_theme.js");
        assert!(theme.contains("COULEURS_DES_TERMINAUX = (window.NUANCIER || []).flat()"), "le terminal ne lit pas le nuancier");
        // Seize pastilles par rangée, comme dans l'éditeur des lueurs.
        assert!(PAGE.contains("grid-template-columns: repeat(16, 22px)"), "grille de 16 colonnes");

        // Le nuancier : 10 rangées de 16 couleurs DISTINCTES.
        let js = include_str!("../../assets/popups/nuancier.js");
        let couleurs: Vec<[f32; 3]> = js
            .split('"')
            .filter(|t| t.starts_with('#') && t.len() == 7)
            .map(|t| [1, 3, 5].map(|i| u8::from_str_radix(&t[i..i + 2], 16).unwrap() as f32 / 255.0))
            .collect();
        assert_eq!(couleurs.len(), 160);
        let mut uniques = couleurs.iter().map(|c| c.map(|x| (x * 255.0).round() as u8)).collect::<Vec<_>>();
        uniques.sort();
        uniques.dedup();
        assert_eq!(uniques.len(), 160, "doublons");
        // ⚠️ **3,8:1, ET NON 4,5:1** : c'est le nuancier des lueurs TEL QUEL (demande
        // explicite), et ses dix tons moyens les plus saturés (rouge, bleu, orange
        // vifs, gris moyen) descendent à 3,9:1 avec le meilleur des deux textes.
        assert!(theme.contains("TEXTE_CLAIR = \"242, 242, 242\"") && theme.contains("TEXTE_SOMBRE = \"28, 28, 28\""));
        let (clair, sombre) = ([0.95_f32; 3], [0.11_f32; 3]);
        for c in couleurs {
            let meilleur = crate::ui::color::contrast_ratio(c, clair).max(crate::ui::color::contrast_ratio(c, sombre));
            assert!(meilleur >= 3.8, "{c:?} : {meilleur}");
        }
    }

    /// Les verbes que `dispatch_actions` accepte — même lecture que
    /// `cef_ssh::tests`.
    fn verbes_du_dispatch() -> Vec<String> {
        let source = include_str!("cef_terminal.rs");
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

    /// La page, le volet des commandes qu'elle charge (`volet_commandes.js`,
    /// dont le `go("commandes")` part avec le `go` de la page) et le script de
    /// la vue sphère (`sphere_source.js`, même `go`).
    fn verbes_de_la_page() -> Vec<String> {
        [PAGE, crate::win32::commandes::VOLET_JS, include_str!("../../assets/popups/sphere_source.js"), include_str!("../../assets/popups/sphere_vue.js")]
            .into_iter()
            .flat_map(|source| {
                source.match_indices("go(").filter_map(move |(i, _)| {
                    let reste = &source[i + 3..];
                    let guillemet = reste.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                    reste[1..].split(guillemet).next().map(str::to_string)
                })
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

    /// ⛔ Un DERNIER segment vide disparaît (voir `cef_ssh::tests`).
    #[test]
    fn la_page_n_envoie_jamais_de_segment_vide() {
        let mut appels = 0;
        for (i, _) in PAGE.match_indices("go(") {
            let arguments = PAGE[i + 3..].split(')').next().unwrap_or_default();
            assert!(!arguments.contains("\"\"") && !arguments.contains("''"), "segment vide : go({arguments})");
            appels += 1;
        }
        assert!(appels >= 6, "{appels} appels seulement : la lecture de la page a changé ?");
    }

    /// Les fonctions que Rust appelle dans la page doivent y exister — sinon
    /// la sortie d'un shell se perdrait EN SILENCE.
    #[test]
    fn la_page_definit_chaque_fonction_que_rust_appelle() {
        let source = include_str!("cef_terminal.rs");
        let rust = source.split("#[cfg(test)]").next().unwrap_or(source);
        let appelees: Vec<&str> =
            rust.match_indices("appeler(\"").filter_map(|(i, _)| rust[i + 9..].split('"').next()).collect();
        assert!(appelees.len() >= 5, "{appelees:?}");
        for fonction in appelees {
            assert!(PAGE.contains(&format!("{fonction}(")), "window.terminal.{fonction} absente de terminal.html");
        }
    }
}
