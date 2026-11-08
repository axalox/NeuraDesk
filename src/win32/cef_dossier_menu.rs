//! Menu contextuel d'un RACCOURCI DE DOSSIER de la barre du bas (clic droit sur
//! sa vignette) — **changer la vignette, renommer, retirer**.
//!
//! ⛔ **CE MENU REMPLACE UN GESTE, IL NE S'AJOUTE PAS À CÔTÉ.** Le clic droit sur
//! une vignette de dossier **ouvrait le dossier**, exactement comme le clic
//! gauche. Il ouvre désormais ce menu — décision de l'utilisateur, et c'est la
//! convention du dépôt : tous les autres clics droits d'une icône du dock
//! ouvrent un menu, et le dossier reste ouvrable au clic gauche, qui le fait
//! déjà. Rien n'est donc perdu.
//!
//! **Les trois gestes existaient DÉJÀ, de bout en bout, dans le panneau
//! Réglages** (`cef_settings`, verbes `dossier-vignette` / `dossier-renommer` /
//! `dossier-retirer`). Ce menu est un **second point d'entrée sur les mêmes
//! gestes**, pas une seconde implémentation : `app::run` le fait passer par le
//! MÊME point d'étranglement (`app::DossiersVivants`), donc la barre suit la
//! liste de la même façon, et le verrou d'aplomb protège les deux chemins.
//!
//! ⚠️ **LE DÉFAUT QUE CE MENU FERME, ET QU'IL FAUT SE RAPPELER** : le clic droit
//! était le SEUL endroit où l'utilisateur cherchait ces gestes, et il ne les y
//! trouvait pas — c'est écrit noir sur blanc dans `app.rs` (« LE MENU CONTEXTUEL
//! DÉDIÉ RESTE À FAIRE ») et dans `settings.html`. Un geste qui n'existe qu'à un
//! endroit que personne ne va chercher n'existe à moitié.
//!
//! ⚠️ **CE MENU EST LE PREMIER DU DÉPÔT À PORTER UN CHAMP DE SAISIE LIBRE.**
//! Les autres menus n'ont que des boutons, des cases et des curseurs. Les
//! raisons pour lesquelles c'est sûr sont dans la doc d'en-tête de
//! `assets/popups/dossier_menu.html` (taper ne change pas la TAILLE, donc il n'y
//! a aucune mesure fraîche à télescoper avec une émission — c'est le même
//! argument qui rend `bar_size.html` sûr).
//!
//! **PAS DE CHEMIN DANS LES ACTIONS ÉMISES** : ce module retient CÔTÉ RUST pour
//! quel dossier le menu est ouvert (`CURRENT_CHEMIN`, fixé par `toggle`),
//! exactement comme `cef_spacer_size` retient son index. La page ne connaît que
//! ce qu'elle affiche, et un chemin n'a pas à faire l'aller-retour — c'est
//! aussi ce qui évite d'avoir à l'échapper deux fois.
//!
//! **L'état est repoussé à CHAQUE frame** (`poll`) et non poussé une seule fois à
//! l'ouverture : c'est le remède déjà retenu pour `cef_network`/`cef_sound`/
//! `cef_spacer_size` contre la course avec le chargement de la page — la poussée
//! faite juste après la création de la fenêtre peut arriver AVANT que
//! `window.setState` n'existe côté JS, et être perdue en silence.
//!
//! Toute la mécanique fenêtre/rendu/relais souris est dans `win32::cef_popup` —
//! voir `win32::cef_test_window` pour la doc de module complète du pattern suivi
//! ici.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::menu;

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// **LE DOSSIER POUR LEQUEL CE MENU EST OUVERT** — retenu ici, jamais
    /// renvoyé par la page (voir la doc de module). `None` tant qu'aucun
    /// `toggle` n'a eu lieu ; le dispatch ne fait alors rien, ce qui est le bon
    /// comportement pour une action arrivée après coup.
    static CURRENT_CHEMIN: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    /// **CE QUE LA PAGE AFFICHE** : `(titre, chemin en clair, nom à préremplir)`.
    /// Retenu parce que `poll` n'a aucun moyen de les relire — il est appelé à
    /// chaque frame et ne reçoit rien, ce module n'ayant pas accès à la liste
    /// des dossiers (même raison que `cef_spacer_size::CURRENT_STATE`).
    static CURRENT_STATE: RefCell<(String, String, String)> =
        const { RefCell::new((String::new(), String::new(), String::new())) };
    /// Numéro d'OUVERTURE, incrémenté à chaque `toggle` — voir `push_state` et
    /// `assets/popups/dossier_menu.html`, doc de `appliedOpen`. C'est ce qui
    /// permet à la page d'appliquer l'état d'une ouverture UNE SEULE FOIS tout
    /// en recevant la poussée RÉPÉTÉE (`poll`) qui la fait survivre au
    /// chargement.
    ///
    /// ⚠️ **ET C'EST ICI PLUS QU'AILLEURS QUE ÇA COMPTE** : sans cette garde, la
    /// poussée par frame RÉÉCRIRAIT le champ de saisie à chaque frappe, en le
    /// ramenant au nom enregistré sous les doigts de l'utilisateur.
    static OPEN_COUNT: Cell<u64> = const { Cell::new(0) };
    /// Voir `app::content_scale_pour`, doc de fonction — mémorisé à
    /// l'ouverture (voir `toggle`) et relu à chaque frame par
    /// `render_if_visible`.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefDossierMenuWindow"),
    window_title: w!("NeuraDesk — Dossier"),
    html_file: "dossier_menu.html",
    wndproc: Some(wndproc),
    above_a_magnified_dock: true,
    // MENU — apparaît et disparaît en fondant, voir `PopupSpec::fades`.
    fades: true,
    fenetre: false,
};

/// **HAUTEUR D'OUVERTURE ESTIMÉE**, en pixels. C'est un POINT DE DÉPART, pas une
/// vérité : la page se mesure elle-même et annonce sa taille par
/// `menu/size/<l>/<h>` (voir `assets/popups/menu.js`), que
/// `cef_popup::drain_actions` applique à la frame suivante.
///
/// ⚠️ **Une estimation trop BASSE est le défaut visible** (on voit le menu
/// s'ouvrir trop court puis grandir), une trop haute le fait rétrécir. On
/// compte le titre, le chemin, le champ et les deux boutons — soit plus que
/// `cef_spacer_size::popup_height(3)`, ce menu ayant deux lignes de texte en
/// plus de ses commandes.
pub const HAUTEUR_ESTIMEE: i32 = 470;

/// Précharge ce popup au démarrage — fenêtre et navigateur naissent CACHÉS, pour
/// que la page ait fini de charger avant la première ouverture réelle. Voir
/// `win32::cef_popup::preload`, doc de fonction, qui porte le pourquoi complet.
pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// **OUVRE LE MENU POUR UN DOSSIER** — `nom` est ce que le champ doit
/// préremplir (le nom AFFICHÉ : celui que l'utilisateur a donné, sinon celui du
/// dossier — voir `config::DossierEpingle::nom_affiche`).
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32, chemin: &Path, nom: String) {
    let titre = chemin
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| chemin.to_string_lossy().into_owned());
    let chemin_texte = chemin.to_string_lossy().into_owned();
    CURRENT_CHEMIN.with(|c| *c.borrow_mut() = Some(chemin.to_path_buf()));
    CURRENT_STATE.with(|c| *c.borrow_mut() = (titre, chemin_texte, nom));
    CONTENT_SCALE.with(|c| c.set(content_scale));
    // Une ouverture de plus : la page appliquera cet état, MÊME si elle a déjà
    // appliqué le précédent (voir `OPEN_COUNT`).
    OPEN_COUNT.with(|c| c.set(c.get() + 1));
    cef_popup::toggle(&SLOT, &SPEC, rect, gfx);
    // Tentative immédiate — perdue en silence si la page n'a pas fini de
    // charger, c'est `poll` qui la rattrape (voir sa doc).
    push_state();
}

/// **L'EXPRESSION JS ENVOYÉE À LA PAGE** — extraite pour être TESTABLE sans
/// disque, sans fenêtre et sans CEF (voir le test qui l'accompagne).
///
/// ⛔ **LE JSON EST PASSÉ COMPLET, ACCOLADE COMPRISE — NE JAMAIS RETIRER CETTE
/// ACCOLADE.** `cef_sous_dock` a payé ce défaut une journée entière : la version
/// fautive faisait `json.trim_end_matches('}')` pour greffer `open` sur l'objet,
/// sans jamais remettre le `}`. Le résultat était un **SyntaxError**, donc un
/// script **rejeté à la COMPILATION** : `window.setState` n'était pas appelé une
/// seule fois, et **rien ne le signalait** — `execute_js` ne rend aucun compte de
/// son échec. On copie donc la forme qui marche, `Object.assign`, telle quelle.
///
/// ⚠️ **ET C'EST AUSSI CE QUI RÈGLE L'ÉCHAPPEMENT** : le titre et le chemin
/// viennent du SYSTÈME DE FICHIERS, et un chemin Windows est fait d'antislashs.
/// Écrit à la main dans un littéral JS, `"C:\Users"` vaut `C:Users` — une
/// séquence d'échappement inconnue **perd son antislash en silence**. Passer par
/// `serde_json` rend le problème impossible : le JSON est un sous-ensemble de la
/// syntaxe JS, et `serde_json` échappe ce qu'il faut.
fn expression_js(json: &str, open: u64) -> String {
    format!("window.setState(Object.assign({json}, {{open: {open}}}))")
}

/// Pousse l'état de l'ouverture courante dans la page via `window.setState`
/// (voir `assets/popups/dossier_menu.html`).
/// **L'ÉTAT ENTIER DE L'OUVERTURE, SÉRIALISÉ** — extrait de `push_state` pour être
/// **TESTABLE sans disque, sans fenêtre et sans CEF**, exactement comme
/// `expression_js` juste au-dessus, et pour la même raison.
///
/// ⛔ **CE DÉFAUT-LÀ A ÉTÉ PAYÉ, ET IL A COÛTÉ UN ALLER-RETOUR À L'ÉCRAN.** La
/// galerie avait été ajoutée en imbriquant `pour_la_page()` sous une clé :
///
/// ```text
/// "categories": { "categories": [...], "couleurs": [...] }   // un OBJET
/// ```
///
/// La page faisait donc `modeles.forEach(...)` sur un OBJET — `forEach` n'existe
/// pas, l'exception interrompait `setState` **avant la première tuile**, et les
/// deux sections restaient vides. Rien au journal, rien à l'écran sauf deux
/// intitulés gris : le symptôme ne désignait pas sa cause.
///
/// ⚠️ **LE TEST QUI EXISTAIT NE POUVAIT PAS LE VOIR** : il vérifiait la forme
/// rendue par `pour_la_page()` et ce que la page attend — mais pas **la clé sous
/// laquelle Rust l'imbrique**, c'est-à-dire précisément l'argument entre les deux.
/// C'est ce trou que `etat_json` (et son test) ferment.
fn etat_json(titre: &str, chemin: &str, nom: &str) -> String {
    let mut etat = serde_json::json!({
        "titre": titre,
        "chemin": chemin,
        "nom": nom,
    });
    // **ON ÉTALE, ON N'IMBRIQUE PAS.** `pour_la_page()` rend déjà
    // `{categories, couleurs}` : ces deux clés vont à la RACINE de l'état, au même
    // niveau que `titre` et `nom`. Les imbriquer donnerait à la page un objet là
    // où elle attend un tableau (voir la doc ci-dessus).
    if let (Some(dest), Some(src)) = (
        etat.as_object_mut(),
        crate::ui::vignettes_dossiers::pour_la_page().as_object(),
    ) {
        dest.extend(src.clone());
    }
    etat.to_string()
}

fn push_state() {
    let (titre, chemin, nom) = CURRENT_STATE.with(|c| c.borrow().clone());
    let open = OPEN_COUNT.with(Cell::get);
    let json = etat_json(&titre, &chemin, &nom);
    let js = expression_js(&json, open);
    // SONDE — **UN COMPTE D'OCTETS, JAMAIS LE DÉBUT DE LA CHAÎNE.** C'est la
    // leçon de `cef_sous_dock` : sa sonde journalisait les 220 premiers
    // caractères, parfaitement valides, alors que le défaut était dans les VINGT
    // DERNIERS. Un compte d'octets se COMPARE d'un run à l'autre, une tête de
    // chaîne se contente d'avoir l'air d'aller bien.
    crate::diag!("[dossier-menu] envoi n°{open} : {} octets", js.len());
    cef_popup::execute_js(&SLOT, &js);
}

/// À appeler à CHAQUE frame tant que ce menu est visible — voir
/// `cef_spacer_size::poll`, dont ceci est la copie, et sa doc pour le défaut
/// qu'il rattrape (bug réel constaté par capture là-bas).
pub fn poll() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

/// **LE GESTE CHOISI DANS LE MENU.**
///
/// ⚠️ **`EffacerLeNom` A SON PROPRE VERBE** (voir la doc d'en-tête de la page) :
/// un dernier segment vide DISPARAÎT de l'URL, donc `renommer/` ne serait reconnu
/// par aucun bras et échouerait en silence. Même raison pour les trois verbes de
/// vignette, qui ne portent aucun argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GesteMenu {
    Renommer(String),
    EffacerLeNom,
    /// **UNE VIGNETTE PRÊTE PAR CATÉGORIE** — l'index dans
    /// `ui::vignettes_dossiers::CATEGORIES`. Un index et non un libellé : la page
    /// affiche ce que Rust lui a poussé, donc les deux listes ne peuvent pas
    /// diverger, et un index hors bornes est refusé côté Rust.
    ///
    /// La seconde moitié est la **TEINTE CHOISIE** dans la rangée de pastilles
    /// (voir `ui::vignettes_dossiers::TEINTES`) — `None` garde la couleur
    /// d'origine. Les deux autres familles portent la même.
    Categorie(usize, Option<[u8; 3]>),
    /// **UN DOSSIER COLORÉ** — l'index dans
    /// `ui::vignettes_dossiers::DOSSIERS_COLORES`. Même mécanique, autre famille :
    /// ils ne disent pas ce que le dossier CONTIENT, seulement qu'on veut le
    /// repérer. Deux verbes plutôt qu'un index global, pour que la lecture du
    /// journal dise du premier coup d'œil laquelle des deux rangées a été
    /// cliquée.
    DossierColore(usize, Option<[u8; 3]>),
    /// **UNE VIGNETTE DESSINÉE** — l'index dans `ui::dessins_vignettes::CATALOGUE`.
    ///
    /// ⚠️ **UNE TROISIÈME FAMILLE, ET UN TROISIÈME VERBE.** Ses vignettes ne sont pas
    /// des glyphes teintés mais des **formes vectorielles rendues par nous**, parce que
    /// la police d'icônes n'a ni serveur, ni base de données, ni conteneur (voir
    /// `ui::dessins_vignettes`, doc de module). Le verbe séparé garde la même propriété
    /// que les deux autres : le journal dit du premier coup d'œil quelle rangée a servi.
    Dessin(usize, Option<[u8; 3]>),
    /// **REVENIR À L'ICÔNE DU DOSSIER** — régénère la vignette depuis le shell,
    /// c'est-à-dire exactement ce qu'un dossier fraîchement épinglé reçoit.
    RevenirALIcone,
    /// **RETIRER LA VIGNETTE** — plus aucune image : la case retombe sur le
    /// glyphe de dossier (ou l'initiale du nom), comme un dossier dont l'icône ne
    /// s'est pas résolue.
    RetirerLaVignette,
    ChangerLaVignette,
    Retirer,
}

/// Vide les actions demandées par `assets/popups/dossier_menu.html` et rend
/// `(dossier visé, geste)` pour L'OUVERTURE COURANTE (voir `CURRENT_CHEMIN`) —
/// `None` s'il n'y a rien à faire.
///
/// **La dernière action reçue gagne** : la page n'en émet qu'une par geste, mais
/// un `change` suivi d'un clic juste après peut en empiler deux, et c'est bien le
/// dernier choix de l'utilisateur qui compte.
/// **L'INDEX D'UNE VIGNETTE PRÊTE, SI CE VERBE EST LE SIEN** — `None` si le
/// préfixe ne correspond pas, si l'index n'est pas un nombre, **ou s'il sort des
/// bornes**.
///
/// Les deux familles (`categorie/`, `couleur/`) s'en servent : elles ne diffèrent
/// que par la longueur de leur tableau, donc par leur borne.
///
/// ⚠️ **La borne est vérifiée ICI** : la page ne peut pas inventer un index (elle
/// affiche ce que Rust lui a poussé), mais on ne le suppose pas — un index hors
/// bornes ferait paniquer l'appelant au moment de lire le tableau.
///
/// **UN SECOND SEGMENT, FACULTATIF, EST LA TEINTE** : `categorie/3` garde la
/// couleur d'origine, `categorie/3/5` pose `TEINTES[5]`. Une teinte illisible ou
/// hors bornes REFUSE tout le geste plutôt que de retomber sur la couleur
/// d'origine : poser une autre couleur que celle cliquée serait pire que rien.
fn index_de_vignette_prete(reste: &str, prefixe: &str, longueur: usize) -> Option<(usize, Option<[u8; 3]>)> {
    let mut segments = reste.strip_prefix(prefixe)?.split('/');
    let rang = segments.next()?.parse::<usize>().ok()?;
    let teinte = match segments.next() {
        None => None,
        Some(t) => Some(crate::ui::vignettes_dossiers::TEINTES.get(t.parse::<usize>().ok()?)?.1),
    };
    if segments.next().is_some() {
        return None;
    }
    (rang < longueur).then_some((rang, teinte))
}

pub fn dispatch_actions() -> Option<(PathBuf, GesteMenu)> {
    let mut resultat = None;
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("dossier-menu/") else {
            crate::diag!("[dossier-menu] action inconnue : {action:?}");
            continue;
        };
        let geste = if let Some(nom) = reste.strip_prefix("renommer/") {
            GesteMenu::Renommer(crate::win32::cef_app::percent_decode(nom))
        } else if reste == "renommer-effacer" {
            GesteMenu::EffacerLeNom
        } else if let Some(rang) = index_de_vignette_prete(reste, "categorie/", crate::ui::vignettes_dossiers::CATEGORIES.len()) {
            GesteMenu::Categorie(rang.0, rang.1)
        } else if let Some(rang) =
            index_de_vignette_prete(reste, "couleur/", crate::ui::vignettes_dossiers::DOSSIERS_COLORES.len())
        {
            GesteMenu::DossierColore(rang.0, rang.1)
        } else if let Some(rang) =
            index_de_vignette_prete(reste, "dessin/", crate::ui::dessins_vignettes::CATALOGUE.len())
        {
            GesteMenu::Dessin(rang.0, rang.1)
        } else if reste == "revenir-a-l-icone" {
            GesteMenu::RevenirALIcone
        } else if reste == "retirer-la-vignette" {
            GesteMenu::RetirerLaVignette
        } else if reste == "vignette" {
            GesteMenu::ChangerLaVignette
        } else if reste == "retirer" {
            GesteMenu::Retirer
        } else {
            crate::diag!("[dossier-menu] action inconnue : {action:?}");
            continue;
        };
        let chemin = CURRENT_CHEMIN.with(|c| c.borrow().clone());
        match chemin {
            Some(chemin) => resultat = Some((chemin, geste)),
            // Une action sans dossier ouvert ne devrait pas arriver (la fenêtre
            // n'existe qu'après un `toggle`) — mais on ne le suppose pas, et on
            // LE DIT plutôt que d'agir sur un dossier inventé.
            None => crate::diag!("[dossier-menu] geste {geste:?} sans dossier ouvert — ignore"),
        }
    }
    resultat
}

pub fn render_if_visible(tint_rgba: [f32; 4]) {
    let scale = CONTENT_SCALE.with(|c| c.get());
    cef_popup::render_if_visible(
        &SLOT,
        tint_rgba,
        menu::menu_corner_radius(),
        cef_popup::PopupPalette::Menu { zoom_level: cef_popup::menu_zoom_level(scale) },
    );
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

#[cfg(test)]
mod tests {
    use super::{etat_json, expression_js, GesteMenu};

    /// **L'EXPRESSION ENVOYÉE DOIT ÊTRE DU JS VALIDE — ET C'EST LE SEUL GARDE-FOU
    /// D'UN DÉFAUT ENTIÈREMENT SILENCIEUX.**
    ///
    /// Un `SyntaxError` dans cette chaîne fait **rejeter le script à la
    /// COMPILATION** : `window.setState` n'est alors appelé aucune fois, le menu
    /// s'ouvre vide, et **rien ne le signale** — `cef_popup::execute_js` ne rend
    /// aucun compte de son échec. `cef_sous_dock` a payé ce défaut une journée
    /// entière, avec une sonde qui montrait le DÉBUT (valide) de l'expression.
    ///
    /// On ne peut pas exécuter de JS ici, mais on peut prouver la seule chose qui
    /// casse : que le JSON EMBARQUÉ est bien du JSON, et que la greffe de `open`
    /// ne l'a pas tronqué. C'est exactement ce qui manquait à la version fautive.
    #[test]
    fn l_expression_envoyee_est_du_js_valide() {
        let json = serde_json::json!({
            // Un chemin Windows RÉEL : les antislashs sont le piège, et les
            // guillemets le second (un nom de dossier peut en contenir).
            "titre": "l'école \"2026\"",
            "chemin": "C:\\Users\\utilisateur\\OneDrive\\Mes dossiers",
            "nom": "Mes dossiers",
        })
        .to_string();
        let js = expression_js(&json, 7);

        // 1. LA GREFFE EST COMPLÈTE : les accolades s'équilibrent.
        let ouvrantes = js.matches('{').count();
        let fermantes = js.matches('}').count();
        assert_eq!(ouvrantes, fermantes, "accolades desequilibrees — le JSON a ete tronque : {js}");
        assert_eq!(js.matches('(').count(), js.matches(')').count(), "parentheses desequilibrees : {js}");

        // 2. ET LE JSON EMBARQUÉ EST RELISIBLE — donc valide, donc accepté par le
        //    moteur JS. C'est ce que `serde_json` garantit et qu'un `format!`
        //    écrit à la main ne garantirait pas.
        let interieur = js
            .strip_prefix("window.setState(Object.assign(")
            .and_then(|reste| reste.strip_suffix("))"))
            .and_then(|reste| reste.rsplit_once(", {open: 7}"))
            .map(|(json, _)| json)
            .expect("la forme de l'expression a change");
        let relu: serde_json::Value =
            serde_json::from_str(interieur).expect("le JSON embarque n'est pas relisible — le JS serait rejete en silence");
        assert_eq!(relu["chemin"], "C:\\Users\\utilisateur\\OneDrive\\Mes dossiers", "les antislashs doivent survivre");
        assert_eq!(relu["titre"], "l'école \"2026\"");
    }

    /// **LA GALERIE ARRIVE À LA PAGE SOUS LA BONNE FORME — UN TABLEAU, PAS UN
    /// OBJET.**
    ///
    /// ⛔ **C'est le test qui manquait, et son absence a coûté un aller-retour à
    /// l'écran.** `pour_la_page()` rend `{categories, couleurs}` ; le site d'appel
    /// l'a un temps imbriqué sous la clé `categories`, si bien que la page
    /// recevait un OBJET là où elle fait `modeles.forEach`. L'exception
    /// interrompait `setState` avant la première tuile : **les deux galeries
    /// restaient vides**, sans un mot nulle part.
    ///
    /// Le test qui existait vérifiait la forme rendue par `pour_la_page()` et ce
    /// que la page attend — mais pas la CLÉ sous laquelle Rust l'imbrique. C'est
    /// l'argument ENTRE les deux, et c'est ici qu'on le tient.
    #[test]
    fn l_etat_pousse_la_galerie_comme_un_tableau_a_la_racine() {
        let etat: serde_json::Value = serde_json::from_str(&etat_json("t", "C:\\x", "n")).expect("de l'etat");

        // Les trois champs de l'ouverture...
        assert_eq!(etat["titre"], "t");
        assert_eq!(etat["chemin"], "C:\\x");
        assert_eq!(etat["nom"], "n");

        // ...ET LES DEUX FAMILLES, À LA RACINE, CHACUNE UN TABLEAU.
        for cle in ["categories", "couleurs"] {
            let liste = etat[cle]
                .as_array()
                .unwrap_or_else(|| panic!("`{cle}` doit etre un TABLEAU a la racine de l'etat, pas {:?}", etat[cle]));
            assert!(!liste.is_empty(), "`{cle}` est vide — la galerie s'afficherait sans une tuile");
            // Ce que la page lit de chaque tuile, et qui doit exister : sans
            // `glyphe`, `String.fromCodePoint(undefined)` LEVE et la galerie
            // entiere disparait.
            for tuile in liste {
                assert!(tuile["glyphe"].is_u64(), "glyphe manquant ou non numerique : {tuile:?}");
                assert!(tuile["couleur"].is_string(), "couleur manquante : {tuile:?}");
                assert!(tuile["nom"].is_string(), "nom manquant : {tuile:?}");
            }
        }
    }

    /// Le numéro d'ouverture est bien celui qu'on passe, et il reste un NOMBRE
    /// (une chaîne citée ferait échouer la comparaison `next.open === appliedOpen`
    /// de la page, qui est ce qui empêche la poussée par frame de réécrire le
    /// champ de saisie sous les doigts de l'utilisateur).
    #[test]
    fn le_numero_d_ouverture_reste_un_nombre() {
        let js = expression_js("{}", 12);
        assert!(js.ends_with(", {open: 12}))"), "{js}");
        assert!(!js.contains("\"12\""), "le numero doit rester un nombre : {js}");
    }

    /// **L'ENSEMBLE DES VERBES EST CELUI DE LA PAGE** — même garde que
    /// `cef_calendar_applet`, qui compare par ÉGALITÉ D'ENSEMBLES les verbes du
    /// `match` Rust à ceux de la page. Un verbe ajouté d'un seul côté échoue ici
    /// plutôt qu'à l'écran, où il serait MUET.
    #[test]
    fn la_page_et_le_dispatch_acceptent_les_memes_verbes() {
        let page = include_str!("../../assets/popups/dossier_menu.html");
        // **LE REGISTRE DE LA PAGE** — le bloc `const VERBES = { … };`. C'est lui
        // qu'on lit, et non plus les appels : depuis que `rendreGalerie` est
        // GÉNÉRIQUE (elle sert deux rangées), le verbe d'une tuile est une
        // VARIABLE, et un contrôle qui chercherait des littéraux ne verrait plus
        // rien. C'est ce test qui l'a signalé, et c'est ce qui a fait déclarer le
        // vocabulaire au lieu de le laisser dispersé.
        let bloc = page
            .split("const VERBES = {")
            .nth(1)
            .and_then(|reste| reste.split("};").next())
            .expect("la page doit declarer son vocabulaire dans `const VERBES`");
        // Les VALEURS du registre (ce qui suit le `:` de chaque ligne), pas ses clefs.
        let envoyes: Vec<&str> = bloc.split(':').skip(1).filter_map(|s| s.split('"').nth(1)).collect();
        assert!(!envoyes.is_empty(), "le registre VERBES est vide ou illisible");

        /// Les verbes que `dispatch_actions` sait lire — la liste des bras de son
        /// `match`, tenue ici en miroir.
        const CONNUS: [&str; 9] = [
            "renommer",
            "renommer-effacer",
            "categorie",
            "couleur",
            // La TROISIÈME famille de vignettes — celles qui sont DESSINÉES et non
            // teintées (voir `ui::dessins_vignettes`). Ce test l'a d'ailleurs
            // attrapée : j'avais ajouté le bras du `match` sans la liste, et la
            // commande aurait été muette à l'écran.
            "dessin",
            "vignette",
            "revenir-a-l-icone",
            "retirer-la-vignette",
            "retirer",
        ];
        for verbe in &envoyes {
            assert!(CONNUS.contains(verbe), "la page emet un verbe que le dispatch ne connait pas : {verbe}");
        }
        // Et l'inverse : chaque geste du dispatch a son verbe dans la page.
        for attendu in CONNUS {
            assert!(envoyes.contains(&attendu), "le dispatch connait {attendu}, la page ne l'emet jamais");
        }
        // ⛔ **ET LE REGISTRE N'EST PAS CONTOURNABLE.** Sans cette troisième
        // vérification, il serait une commodité et non une garantie : un verbe
        // écrit en clair au fond d'un gestionnaire échapperait aux deux
        // vérifications ci-dessus, et une commande qu'un seul côté connaît est
        // une commande MUETTE.
        assert_eq!(
            page.matches("go(\"").count(),
            0,
            "un verbe est ecrit en clair dans la page : il doit passer par `VERBES`, sinon il echappe au controle"
        );
    }

    /// Le geste d'effacement a son PROPRE verbe, et ce n'est pas un détail de
    /// style : `cef_app` retire le `/` final de l'URL, donc un dernier segment
    /// vide DISPARAÎT — `renommer/` deviendrait `renommer`, qui ne correspond à
    /// aucun bras et échouerait sans un mot.
    /// **LA TEINTE VOYAGE AVEC LE VERBE, ET UNE TEINTE INVENTÉE EST REFUSÉE.**
    #[test]
    fn le_verbe_d_une_vignette_porte_sa_teinte() {
        use super::index_de_vignette_prete;
        use crate::ui::vignettes_dossiers::TEINTES;
        assert_eq!(index_de_vignette_prete("categorie/3", "categorie/", 10), Some((3, None)));
        assert_eq!(index_de_vignette_prete("categorie/3/1", "categorie/", 10), Some((3, Some(TEINTES[1].1))));
        // Une teinte hors bornes ou illisible refuse TOUT le geste...
        assert_eq!(index_de_vignette_prete(&format!("categorie/3/{}", TEINTES.len()), "categorie/", 10), None);
        assert_eq!(index_de_vignette_prete("categorie/3/x", "categorie/", 10), None);
        // ...comme un segment de trop, ou un modèle hors bornes.
        assert_eq!(index_de_vignette_prete("categorie/3/1/0", "categorie/", 10), None);
        assert_eq!(index_de_vignette_prete("categorie/10/1", "categorie/", 10), None);
    }

    /// La page reçoit la rangée de pastilles, et chacune est une couleur CSS.
    #[test]
    fn l_etat_pousse_les_teintes() {
        let etat: serde_json::Value = serde_json::from_str(&etat_json("t", "C:\\x", "n")).expect("de l'etat");
        let teintes = etat["teintes"].as_array().expect("`teintes` doit etre un TABLEAU a la racine");
        assert_eq!(teintes.len(), crate::ui::vignettes_dossiers::TEINTES.len());
        for t in teintes {
            assert!(t["nom"].is_string(), "{t:?}");
            assert!(t["couleur"].as_str().is_some_and(|c| c.len() == 7 && c.starts_with('#')), "{t:?}");
        }
    }

    #[test]
    fn effacer_le_nom_a_son_propre_verbe() {
        assert_ne!(GesteMenu::EffacerLeNom, GesteMenu::Renommer(String::new()));
    }
}
