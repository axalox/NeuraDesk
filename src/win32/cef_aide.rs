//! LE PANNEAU D'AIDE — la page `assets/popups/aide.html`.
//!
//! DEMANDE EXPLICITE : « une applet d'aide dans la zone Intégrés qui affiche un
//! dash avec les explications sur l'utilisation de l'app ».
//!
//! ⚠️ **LE PANNEAU LE PLUS PAUVRE EN ÉTAT, ET C'EST DÉLIBÉRÉ.** Le texte vit dans
//! la page et ne change pas : le seul état poussé est l'échelle du texte, et les
//! seuls verbes reçus sont la croix et le signal de disponibilité (voir
//! `dispatch_actions`). Les autres panneaux ont de vrais états à pousser ;
//! recopier leur squelette sans le vider aurait laissé une machinerie qui tourne
//! à vide.
//!
//! La SEULE chose poussée est l'échelle du texte (`contentScale`), pour que l'aide
//! suive le curseur « Taille du contenu » comme ses voisins — voir
//! `win32::taille_panneaux`.
//!
//! ⚠️ **ET IL PORTE LE MÊME CONTRAT DE VOISINAGE QUE LES AUTRES** : il masque les
//! panneaux qui partagent la zone libre, et chacun d'eux le masque. Voir
//! `masquer_les_voisins`, dont la liste doit rester COMPLÈTE.

use std::cell::{Cell, RefCell};

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};

thread_local! {
    static SLOT: PopupSlot = PopupSlot::new();
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// Le dernier état poussé — même garde que partout ailleurs. Ici il ne porte
    /// qu'un nombre, mais la garde reste : `setState` à chaque frame
    /// reconstruirait la page pour rien.
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// La section où amener la page (l'`id` d'un titre), et un compteur qui fait
    /// d'une seconde demande vers la même section un état NOUVEAU — sans lui,
    /// `PAYLOAD` la jugerait inchangée et la page ne bougerait pas.
    static ANCRE: RefCell<(String, u32)> = const { RefCell::new((String::new(), 0)) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefAideWindow"),
    window_title: w!("NeuraDesk — Aide"),
    html_file: "aide.html",
    wndproc: Some(wndproc),
    // PANNEAU — même famille que la météo et la recherche.
    above_a_magnified_dock: false,
    fades: false,
    fenetre: false,
};

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// **LES PANNEAUX QUI PARTAGENT LA ZONE LIBRE.**
///
/// ⛔ **LA LISTE DOIT ÊTRE COMPLÈTE, ET ELLE SE VÉRIFIE PAR `grep`, PAS DE
/// MÉMOIRE.** `cef_meteo` porte le même avertissement, et pour une raison mesurée :
/// la liste dont il s'inspirait était **EN RETARD d'une applet**, donc deux
/// panneaux se superposaient dans la zone libre sans que rien ne le dise.
///
/// ⚠️ **`cef_couleurs` n'y est pas, et c'est délibéré** — même omission que dans
/// `app::hide_overlay_windows` : c'est une VRAIE FENÊTRE, qu'un clic sur son applet
/// ramène devant au lieu de la fermer.
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
    crate::win32::cef_ssh::hide_if_visible();
    crate::win32::cef_terminal::hide_if_visible();
    crate::win32::cef_minuteur::hide_if_visible();
    crate::win32::cef_calculatrice::hide_if_visible();
    crate::win32::cef_recherche::hide_if_visible();
    crate::win32::cef_automatisations::hide_if_visible();
    crate::win32::cef_meteo::hide_if_visible();
    crate::win32::bandeau::hide_if_visible();
}

pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    // ⚠️ **LE FACTEUR DE PANNEAU EST APPLIQUÉ ICI**, comme pour les onze autres
    // (voir `win32::taille_panneaux`) : le réglage s'AJOUTE à l'échelle d'écran au
    // lieu de la remplacer.
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("aide")));
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, masquer_les_voisins);
    push_state();
    // ⚠️ **ON NE PREND PAS LE FOCUS** : le panneau s'ouvre pour LIRE. C'est le
    // même choix que la météo, et l'inverse de la recherche — dont le propos EST la
    // saisie.
}

/// Ouvre l'aide (si elle ne l'est pas déjà) sur la section `ancre` — DEMANDE
/// EXPLICITE (2026-10-18) : un message des Réglages « renvoyant vers l'aide s'il
/// manque une police de caractère ».
thread_local! {
    /// La section de l'aide qu'un autre panneau demande d'ouvrir (voir
    /// `demander`) — `app::run` la prend, seul à avoir de quoi poser le panneau.
    static DEMANDEE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Demande l'ouverture de l'aide sur la section `ancre` (un `id` de `aide.html`),
/// faite à la prochaine frame par `app::run` (`take_demandee`).
pub fn demander(ancre: &str) {
    DEMANDEE.with(|d| *d.borrow_mut() = Some(ancre.to_string()));
}

/// Voir `demander`.
pub fn take_demandee() -> Option<String> {
    DEMANDEE.with(|d| d.borrow_mut().take())
}

pub fn ouvrir_a(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32, ancre: &str) {
    ANCRE.with(|a| {
        let mut a = a.borrow_mut();
        *a = (ancre.to_string(), a.1.wrapping_add(1));
    });
    if cef_popup::is_visible(&SLOT) {
        push_state();
    } else {
        toggle(rect, gfx, content_scale);
    }
}

/// Les deux verbes de la page : sa croix, et le signal « me voici prête » que
/// les quinze autres pages envoient déjà (voir l'en-tête d'`aide.html`).
pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        match action.as_str() {
            "aide/close" => cef_popup::fermer_par_geste(&SLOT),
            // ⛔ **SANS CE BRAS, L'ANCRE EST PERDUE POUR DE BON** — défaut mesuré
            // le 2026-10-28 : `push_state` part avant que la page ait défini
            // `window.setState`, le garde `if (window.setState)` rend l'appel
            // inerte, et `execute_js` ne rend aucun compte de son échec
            // ([[page-cef-exception-invisible]]). `PAYLOAD` avait pourtant marqué
            // l'état comme poussé, donc rien ne repartait : « Voir l'aide » ouvrait
            // la page EN HAUT. Vider le cache suffit — `refresh_if_visible` repousse
            // à la même frame (voir `app::run`), et la `demande` de `ANCRE` est
            // conservée côté Rust, donc la section demandée monte avec elle.
            "aide/ready" => PAYLOAD.with(|c| *c.borrow_mut() = None),
            _ => crate::diag!("[aide] action inconnue : {action:?}"),
        }
    }
}

/// Repousse l'échelle si la fenêtre est visible — à chaque frame (voir `app::run`) ;
/// `PAYLOAD` rend l'appel gratuit tant que rien ne change.
///
/// ⚠️ **NÉCESSAIRE MALGRÉ L'ABSENCE D'ÉTAT** : le curseur « Taille du contenu »
/// peut bouger pendant que l'aide est ouverte, et la page doit suivre.
pub fn refresh_if_visible() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

fn push_state() {
    let (ancre, demande) = ANCRE.with(|a| a.borrow().clone());
    let valeur = json!({ "contentScale": CONTENT_SCALE.with(Cell::get), "ancre": ancre, "demande": demande });
    let texte = valeur.to_string();
    let inchange = PAYLOAD.with(|p| {
        let mut p = p.borrow_mut();
        if p.as_deref() == Some(texte.as_str()) {
            true
        } else {
            *p = Some(texte.clone());
            false
        }
    });
    if inchange {
        return;
    }
    // ⚠️ **LE JSON SE PASSE COMPLET, ACCOLADES COMPRISES** : un `}` retiré produit
    // un `SyntaxError` muet, et `execute_js` ne rend aucun compte de son échec (voir
    // `cef_sous_dock`, qui a payé cette journée-là).
    cef_popup::execute_js(&SLOT, &format!("if (window.setState) window.setState({texte})"));
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

pub fn render_if_visible(rect: [i32; 4], tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::resize_if_needed(&SLOT, rect);
    cef_popup::render_if_visible(&SLOT, tint_rgba, 0.0, cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

/// Voir `win32::cef_calendar_applet::debug_dump_backbuffer` — le harnais a DEUX
/// listes à tenir (`NEURADESK_DEBUG_OPEN=aide`).
pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// Bouton DROIT sur un point de la page, pour le harnais — voir
/// `cef_popup::debug_right_click` et `NEURADESK_DEBUG_DROIT`. C'est le seul moyen de
/// faire naître le menu commun (`assets/popups/menu_contextuel.js`) sans souris ni
/// `SendInput` : le menu est un DOM, donc la capture le montre.
pub fn debug_clic_droit(x: i32, y: i32) {
    cef_popup::debug_right_click(&SLOT, x, y)
}

/// Clic GAUCHE sur un point de la page, pour le harnais — voir
/// `cef_popup::debug_click`. Ici il sert à cliquer l'élément du menu que
/// `debug_clic_droit` vient d'ouvrir (`NEURADESK_DEBUG_CLICK`, voir sa doc).
pub fn debug_clic_gauche(x: i32, y: i32) {
    cef_popup::debug_click(&SLOT, x, y)
}

/// GLISSEMENT qui sélectionne un morceau de texte, pour le harnais — voir
/// `cef_popup::debug_drag_select` et `NEURADESK_DEBUG_GLISSE`. C'est la
/// **sélection à la souris** elle-même, sans souris : avant lui, la seule preuve
/// de `user-select: text` restait la règle CSS, jamais le geste.
pub fn debug_glisse(x0: i32, y0: i32, x1: i32, y1: i32) {
    cef_popup::debug_drag_select(&SLOT, x0, y0, x1, y1)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(result) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    const PAGE: &str = include_str!("../../assets/popups/aide.html");

    /// **LA PAGE EXISTE, ET ELLE PORTE CE QUE RUST LUI ANNONCE.** Le seul contrat
    /// ici : `setState`, puisque c'est le seul verbe que ce module emploie.
    ///
    /// ⚠️ **Sans ce test, une page RENOMMÉE ou VIDÉE passerait inaperçue** :
    /// `execute_js` ne rend aucun compte de son échec, et la fenêtre s'ouvrirait
    /// sur une page qui ne suit pas le curseur de taille — sans un mot au journal.
    #[test]
    fn la_page_definit_ce_que_rust_appelle() {
        assert!(PAGE.contains("window.setState"), "window.setState absent de aide.html");
    }

    /// ⛔ **LES TITRES DE SECTION NE SONT PAS PEINTS EN COULEUR D'ACCENT.**
    /// Dénoncé le 2026-10-28 : « les textes présentant les sections de l'aide ne
    /// sont pas de la bonne couleur et sont illisibles, elles devraient être de la
    /// même couleur que le reste du texte ». La cause : `h2` lisait
    /// `--popup-accent-rgb`, qui est `Palette::emphasis` — la TEINTE de l'accent,
    /// destinée aux aplats (doc de `cef_popup::push_window_palette`). Avec
    /// l'accent réel de l'utilisateur (`aide_accent_color.txt` = `0.9,0.9,0.9`,
    /// quasi blanc) les titres rendaient à lumière ~210 sur un fond à 252, soit
    /// ~1,5 : 1. Le test gèle la règle par la REGLE CSS, pas par un mot du
    /// commentaire : il coupe chaque règle sur son sélecteur.
    ///
    /// ⚠️ **ET LA MÊME ENCRE QUE LES PARAGRAPHES, GELÉE PAR LE NOMBRE DE `p {`.**
    /// Corrigé dans le même élan : pleine puissance, un titre rendu à 14 px graisse
    /// 600 donnait un pixel le plus sombre à **20** contre **47** pour le paragraphe
    /// (lumière moyenne 82 contre 111) — même pôle, mais plus foncé, donc pas « la
    /// même couleur que le reste du texte ». Le test ne compare pas un mot : il lit
    /// l'expression d'opacité de `p {` et exige de la retrouver mot pour mot dans
    /// `h2 {` et `.adresse {`.
    ///
    /// ⚠️ **`.adresse {` NE PORTE PAS SON OPACITÉ, ET LE TEST LE VÉRIFIE.** Chaque
    /// adresse est assise dans un `<li>` ou un `<p>` déjà à .88, et **deux opacités
    /// nichées se MULTIPLIENT** : ajouter .88 sur la règle rendait l'encre plus
    /// claire que le paragraphe (pixel le plus sombre **69** contre **47**, capture
    /// `A5_defaut_apres2`). Elle hérite donc du parent et rend à 47, l'encre commune.
    ///
    /// **MUTATIONS** : remettre `--popup-accent-rgb` dans `h2 {`, retirer la ligne
    /// `opacity:` de `h2 {`, ou la remettre dans `.adresse {` — le test nomme les
    /// trois ([[un-test-doit-echouer-sur-l-ancien-code]]).
    #[test]
    fn les_titres_de_section_portent_la_couleur_du_texte() {
        let regle = |selecteur: &str| PAGE.split(selecteur).nth(1).and_then(|r| r.split('}').next()).unwrap_or("").to_string();
        // L'encre de référence est celle des paragraphes : couleur du corps héritée
        // de `body {`, plus son opacité.
        let opacite = regle("p {").split("opacity:").nth(1).and_then(|s| s.split(';').next()).unwrap_or("").trim().to_string();
        assert!(!opacite.is_empty(), "`p {{ ... }}` n'a plus d'opacité à servir de référence");
        for selecteur in ["h2 {", ".adresse {"] {
            let r = regle(selecteur);
            assert!(!r.contains("--popup-accent-rgb"), "{selecteur} peint du texte avec la teinte d'accent");
            assert!(r.contains("--popup-fg-rgb"), "{selecteur} ne suit pas le pôle FIXE du texte");
        }
        // `h2` est enfant direct du conteneur : il doit porter lui-même l'encre.
        assert!(regle("h2 {").contains(&opacite), "h2 {{ ... }} n'a pas l'encre des paragraphes ({opacite})");
        // `.adresse` est enfant d'un `li`/`p` déjà opacifié : une opacité de plus LA DILUE.
        assert!(!regle(".adresse {").contains("opacity:"), ".adresse {{ ... }} ajoute une opacité par-dessus celle de son parent (<li>, <p>)");
        // L'accent reste un APLET : la note garde sa teinte de fond.
        assert!(regle(".note {").contains("background: rgba(var(--popup-accent-rgb"), "l'aplat de la note a perdu sa teinte");
    }

    /// ⭐ **LE TEXTE SE SÉLECTIONNE À LA SOURIS, ET LE CLIC DROIT OFFRE « COPIER ».**
    /// DEMANDÉ le 2026-10-28 : « je voudrais pouvoir copier du texte en le
    /// sélectionnant à la souris et en utilisant les clic droit sur la page d'aide ».
    /// DEUX moitiés, chacune capable de laisser la demande insatisfaite tout en ayant
    /// l'air en ordre :
    ///
    /// - `body { user-select: none }` protège le bandeau du haut, mais **`user-select`
    ///   s'hérite** : il descendait jusque dans les paragraphes, donc RIEN ne se
    ///   sélectionnait (les `.adresse` seules la posaient à la main) ;
    /// - le menu natif de Chromium est vidé côté Rust
    ///   (`cef_app::NeuraDeskContextMenuHandler`, `model.clear()`), donc sans
    ///   `menu_contextuel.js` le clic droit ne faisait **RIEN DU TOUT** — vingt autres
    ///   pages l'incluent, l'aide n'était pas du nombre.
    ///
    /// ⚠️ Ce test lit la RÈGLE et la BALISE, jamais un mot de commentaire : un
    /// commentaire qui parle du clic droit ne prouve pas qu'il existe
    /// ([[outil-edit-convertit-les-echappements-unicode]], [[parite-verbes-litteraux-dans-la-page]]).
    ///
    /// **MUTATIONS** : retirer `user-select: text` de `#corps {`, ou retirer le
    /// `<script src="menu_contextuel.js">` — le test nomme les deux
    /// ([[un-test-doit-echouer-sur-l-ancien-code]]).
    #[test]
    fn la_page_se_selectionne_et_offre_copier_au_clic_droit() {
        assert!(PAGE.contains(r#"<script src="menu_contextuel.js">"#), "l'aide n'inclut pas le menu du clic droit commun — le menu natif est vidé côté Rust, donc le clic droit ne fait rien");
        let corps = PAGE.split("#corps {").nth(1).and_then(|r| r.split('}').next()).unwrap_or("");
        assert!(corps.contains("user-select: text"), "`#corps` ne rend pas le texte sélectionnable : `user-select: none` de `body` hérite jusqu'aux paragraphes");
    }

    /// ⭐ **L'ANCRE ARRIVE ENFIN — LA PAGE DIT QU'ELLE EST PRÊTE, RUST REPOUSSE
    /// L'ÉTAT.** Défaut mesuré le 2026-10-28 : `push_state` part avant que
    /// `aide.html` ait défini `window.setState`, le garde `if (window.setState)`
    /// rend l'appel inerte, et `PAYLOAD` marque pourtant l'état comme poussé — donc
    /// rien ne repart, et « Voir l'aide » ouvre la page EN HAUT (sonde :
    /// `derniereDemande=0`, `corps scrollTop=0` à +60 ms, +1,6 s et +4 s).
    ///
    /// ⚠️ **DEUX MOITIÉS, ET UNE SEULE NE PROUVE RIEN** : la page peut envoyer
    /// `go("ready")` que Rust ignore (ligne fantôme qui compile,
    /// [[cablage-applet-remplacement-octet]]), ou Rust avoir le bras sans que la
    /// page l'envoie jamais. Le test exige les deux, et que le bras VIDE `PAYLOAD`
    /// — un bras qui ne fait que journaliser laisserait le cache en place.
    ///
    /// **MUTATIONS** : retirer `go("ready")` de la page, ou remplacer le corps du
    /// bras par un `diag!` — le test nomme les deux
    /// ([[un-test-doit-echouer-sur-l-ancien-code]]).
    #[test]
    fn la_page_signale_qu_elle_est_prete_et_rust_repousse_l_etat() {
        // L'ORDRE COMPTE : le signal suit l'affectation de `setState`, sinon Rust
        // repousse un état que la page ne sait pas encore recevoir.
        let apres = PAGE.split("window.setState = setState;").nth(1).unwrap_or_default();
        assert!(apres.contains(r#"go("ready")"#), "aide.html n'annonce pas `ready` : l'état poussé avant son script est perdu pour toujours");
        let source = include_str!("cef_aide.rs");
        let bras = source.split(r#""aide/ready" =>"#).nth(1).and_then(|r| r.lines().next()).unwrap_or_default();
        assert!(bras.contains("PAYLOAD"), "le bras `aide/ready` ne vide pas `PAYLOAD` : l'état ne serait jamais repoussé");
    }

    /// La croix émet le seul verbe que `dispatch_actions` connaît.
    #[test]
    fn la_croix_emet_le_verbe_du_dispatch() {
        assert!(PAGE.contains("go('close')"), "la croix d'aide.html n'emet pas `close`");
        assert!(PAGE.contains("\"neuradesk://aide/\""), "`go` ne vise pas `aide/`");
        assert!(include_str!("cef_aide.rs").contains("\"aide/close\" =>"));
    }

    /// ⛔ **LA LISTE DES VOISINS EST COMPLÈTE, ET CE TEST LA COMPTE.** C'est la
    /// panne qui a DÉJÀ eu lieu une fois : une liste en retard d'un panneau, et
    /// deux d'entre eux se superposant dans la zone libre.
    ///
    /// ⚠️ Il ne peut pas vérifier l'EXHAUSTIVITÉ (il ne connaît pas les panneaux
    /// qui existent) : il gèle ce qui est là, pour qu'un retrait se voie.
    #[test]
    fn masquer_les_voisins_en_masque_vingt() {
        let source = include_str!("cef_aide.rs");
        let corps = source.split("fn masquer_les_voisins()").nth(1).unwrap_or("");
        let corps = corps.split("\n}").next().unwrap_or("");
        let n = corps.matches("hide_if_visible()").count();
        assert_eq!(n, 20, "vingt panneaux partagent la zone libre (le moniteur système depuis le 2026-10-20), pas {n}");
    }
}
