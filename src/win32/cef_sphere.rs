//! LA VUE SPHÈRE des terminaux et des connexions SSH/SFTP — DEMANDE EXPLICITE
//! (2026-11-01) : « une interface à deux modes pour gérer mes terminaux et
//! fenêtres SFTP ». Voir `docs/plans/vue-sphere-terminaux.md`,
//! `assets/popups/sphere_vue.js` (la sphère) et `assets/popups/sphere_source.js`
//! (le clic molette, l'indicateur, les vignettes).
//!
//! ⭐ **LA SPHÈRE VIT DANS CHAQUE PANNEAU, ET SES TERMINAUX SONT LES VRAIS**
//! (choix de l'utilisateur, le 2026-11-01, après une première version où une
//! fenêtre à part ne montrait que des aperçus : « le terminal dans la sphère
//! n'est pas le même », « les terminaux ne sont pas interactifs »). Un terminal
//! xterm ne vit que dans SA page : le Terminal pose les siens, vivants, sur sa
//! sphère ; les Serveurs SSH les leurs. Les sessions de l'AUTRE panneau y sont
//! des APERÇUS — ce module les relaie, et fait la bascule de l'un à l'autre.
//!
//! Ce module n'a PAS de fenêtre. Il tient :
//! - les vignettes que chaque page dépose (POST `<source>/vignettes`, muettes au
//!   journal : le contenu d'un terminal peut être un secret) ;
//! - quel panneau a sa sphère ouverte, pour lui relayer celles de l'autre, et les
//!   redemander chaque seconde ;
//! - la bascule vers l'autre panneau (qui a besoin du `GfxDevice` : servie par
//!   [`servir`], à la frame suivante) ;
//! - le raccourci réglable (`sphere_raccourci.txt`).

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::gfx::device::GfxDevice;

/// Les deux panneaux qui ont une sphère.
pub const TERMINAL: &str = "terminal";
pub const SSH: &str = "ssh";

/// Le raccourci qui ouvre la sphère depuis un panneau.
const RACCOURCIS: [&str; 3] = ["molette", "ctrl-molette", "aucun"];
const RACCOURCI_PAR_DEFAUT: &str = "molette";
const FICHIER_RACCOURCI: &str = "sphere_raccourci.txt";
/// La vitesse du zoom à la molette, en % (réglée dans Réglages, demande du
/// 2026-11-01) : 100 = le pas d'origine.
pub const VITESSE_ZOOM_MIN: u32 = 25;
pub const VITESSE_ZOOM_MAX: u32 = 400;
const VITESSE_ZOOM_PAR_DEFAUT: u32 = 100;
const FICHIER_VITESSE_ZOOM: &str = "sphere_zoom.txt";

// ---- Les réglages de la sphère elle-même (2026-11-04) ----
// DEMANDE EXPLICITE : « j'aimerais avoir les mêmes possibilités de réglage
// pour les terminaux » que pour la sphère des PROGRAMMES (voir
// `win32::cef_fenetres::ZOOM_DEFAUT_MIN` et suivants). Mêmes bornes, mêmes
// défauts prudents, fichiers distincts.

/// **Le zoom PAR DÉFAUT**, en % : la distance de caméra à l'ouverture, donc la
/// taille des cartes. `100` = la sphère telle qu'elle s'ouvre historiquement
/// (caméra au niveau de la sphère).
pub const ZOOM_DEFAUT_MIN: u32 = 50;
pub const ZOOM_DEFAUT_MAX: u32 = 200;
const ZOOM_DEFAUT_PAR_DEFAUT: u32 = 100;
const FICHIER_ZOOM_DEFAUT: &str = "sphere_terminaux_zoom_defaut.txt";

/// **Le zoom AUTOMATIQUE** : la caméra recule juste assez pour que toutes les
/// cartes tiennent dans la scène (`sphere_terminaux_zoom_auto.txt`, drapeau).
const FICHIER_ZOOM_AUTO: &str = "sphere_terminaux_zoom_auto";

/// **Le fondu de la sphère** dans son panneau (opacité, apparition puis
/// disparition), en ms. `0` = pas de fondu, le comportement d'avant ce
/// réglage — défaut : rien ne change tant que l'utilisateur ne le règle pas.
pub const FONDU_MIN_MS: u32 = 0;
pub const FONDU_MAX_MS: u32 = 1200;
const FICHIER_FONDU: &str = "sphere_terminaux_fondu.txt";
/// Au plus autant de vignettes par panneau : une page folle ne noie pas la sphère.
const VIGNETTES_MAX: usize = 64;
/// Tant qu'une sphère est ouverte, les aperçus de l'autre panneau se
/// renouvellent à ce rythme.
const RAFRAICHIR: Duration = Duration::from_secs(1);
/// Après une bascule, le panneau montré réclame le clavier pendant ce délai
/// (voir [`CLAVIER`]).
const RECLAMER_LE_CLAVIER: Duration = Duration::from_millis(1500);

/// Aller de `depuis` à l'autre panneau, sa sphère ouverte sur `onglet`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Bascule {
    depuis: &'static str,
    onglet: u32,
}

thread_local! {
    /// Les vignettes reçues : `[terminal, ssh]`.
    static VIGNETTES: RefCell<[Vec<Value>; 2]> = const { RefCell::new([Vec::new(), Vec::new()]) };
    /// Le panneau dont la sphère est ouverte.
    static OUVERTE: Cell<Option<&'static str>> = const { Cell::new(None) };
    static BASCULES: RefCell<Vec<Bascule>> = const { RefCell::new(Vec::new()) };
    static DERNIERE_DEMANDE: Cell<Option<Instant>> = const { Cell::new(None) };
    static RACCOURCI: RefCell<Option<String>> = const { RefCell::new(None) };
    static VITESSE_ZOOM: Cell<Option<u32>> = const { Cell::new(None) };
    /// Les trois réglages de la sphère, lus une fois puis gardés.
    static ZOOM_DEFAUT: Cell<Option<u32>> = const { Cell::new(None) };
    static ZOOM_AUTO: Cell<Option<bool>> = const { Cell::new(None) };
    static FONDU: Cell<Option<(u32, u32)>> = const { Cell::new(None) };
    /// ⭐ **LE PANNEAU MONTRÉ N'AVAIT PAS LE CLAVIER** (mesuré le 2026-11-01) :
    /// une bascule cache la fenêtre au premier plan, et Windows ne rend le premier
    /// plan à personne — ni clavier, ni molette, ni Échap. Celui qu'on vient de
    /// montrer le réclame donc à chaque frame jusqu'à l'avoir, au plus
    /// [`RECLAMER_LE_CLAVIER`].
    static CLAVIER: Cell<Option<(&'static str, Instant)>> = const { Cell::new(None) };
}

fn index_de(source: &str) -> usize {
    usize::from(source == SSH)
}

fn l_autre(source: &str) -> &'static str {
    if source == SSH { TERMINAL } else { SSH }
}

// ---- Le raccourci ----

fn chemin_du_raccourci() -> Option<std::path::PathBuf> {
    crate::config::config_dir().map(|d| d.join(FICHIER_RACCOURCI))
}

/// Le raccourci réglé — lu une fois, puis gardé.
pub fn raccourci() -> String {
    if let Some(r) = RACCOURCI.with(|r| r.borrow().clone()) {
        return r;
    }
    let lu = chemin_du_raccourci()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| t.trim().to_string())
        .filter(|t| RACCOURCIS.contains(&t.as_str()))
        .unwrap_or_else(|| RACCOURCI_PAR_DEFAUT.to_string());
    RACCOURCI.with(|r| *r.borrow_mut() = Some(lu.clone()));
    lu
}

/// Règle le raccourci (les boutons au pied de la sphère), l'enregistre, et le
/// pousse aux deux panneaux.
pub fn regler_raccourci(valeur: &str) {
    if !RACCOURCIS.contains(&valeur) {
        crate::diag!("[sphere] raccourci inconnu : {valeur:?}");
        return;
    }
    RACCOURCI.with(|r| *r.borrow_mut() = Some(valeur.to_string()));
    match chemin_du_raccourci() {
        Some(chemin) => {
            if let Err(e) = std::fs::write(&chemin, valeur) {
                crate::diag!("[sphere] raccourci non enregistré ({}) : {e}", chemin.display());
            }
        }
        None => crate::diag!("[sphere] raccourci non enregistré : pas de dossier de configuration"),
    }
    crate::win32::cef_terminal::refresh_if_visible();
    crate::win32::cef_ssh::refresh_if_visible();
}

// ---- La vitesse du zoom ----

/// Une vitesse lue (fichier, page) : un entier dans les bornes, sinon rien.
fn vitesse_valide(texte: &str) -> Option<u32> {
    let v = texte.trim().parse::<f64>().ok().filter(|v| v.is_finite())?;
    Some((v.round() as i64).clamp(i64::from(VITESSE_ZOOM_MIN), i64::from(VITESSE_ZOOM_MAX)) as u32)
}

/// La vitesse du zoom à la molette, en % — lue une fois, puis gardée.
pub fn vitesse_zoom() -> u32 {
    if let Some(v) = VITESSE_ZOOM.with(Cell::get) {
        return v;
    }
    let lu = crate::config::config_dir()
        .and_then(|d| std::fs::read_to_string(d.join(FICHIER_VITESSE_ZOOM)).ok())
        .and_then(|t| vitesse_valide(&t))
        .unwrap_or(VITESSE_ZOOM_PAR_DEFAUT);
    VITESSE_ZOOM.with(|v| v.set(Some(lu)));
    lu
}

/// Règle la vitesse (le curseur de Réglages), l'enregistre, et la pousse aux
/// deux panneaux.
pub fn regler_vitesse_zoom(texte: &str) {
    let Some(v) = vitesse_valide(texte) else {
        crate::diag!("[sphere] vitesse du zoom illisible : {texte:?}");
        return;
    };
    VITESSE_ZOOM.with(|c| c.set(Some(v)));
    crate::config::write_config(FICHIER_VITESSE_ZOOM, v.to_string());
    crate::win32::cef_terminal::refresh_if_visible();
    crate::win32::cef_ssh::refresh_if_visible();
}

// ---- Le zoom par défaut, le zoom automatique, le fondu ----

/// Un entier dans les bornes lu depuis un fichier ou une page — `None` si
/// illisible ou hors bornes (même idiome que `cef_fenetres::entier_dans`).
fn entier_dans(texte: &str, min: u32, max: u32) -> Option<u32> {
    let v = texte.trim().parse::<f64>().ok().filter(|v| v.is_finite())?;
    Some((v.round() as i64).clamp(i64::from(min), i64::from(max)) as u32)
}

/// Le zoom par défaut, en % — lu une fois, puis gardé.
pub fn zoom_defaut() -> u32 {
    if let Some(v) = ZOOM_DEFAUT.with(Cell::get) {
        return v;
    }
    let lu = crate::config::config_dir()
        .and_then(|d| std::fs::read_to_string(d.join(FICHIER_ZOOM_DEFAUT)).ok())
        .and_then(|t| entier_dans(&t, ZOOM_DEFAUT_MIN, ZOOM_DEFAUT_MAX))
        .unwrap_or(ZOOM_DEFAUT_PAR_DEFAUT);
    ZOOM_DEFAUT.with(|c| c.set(Some(lu)));
    lu
}

/// Règle le zoom par défaut (curseur des Réglages), l'enregistre, et le pousse
/// aux deux panneaux — il s'appliquera à la PROCHAINE ouverture de la sphère.
pub fn regler_zoom_defaut(texte: &str) {
    let Some(v) = entier_dans(texte, ZOOM_DEFAUT_MIN, ZOOM_DEFAUT_MAX) else {
        crate::diag!("[sphere] zoom par defaut illisible : {texte:?}");
        return;
    };
    ZOOM_DEFAUT.with(|c| c.set(Some(v)));
    crate::config::write_config(FICHIER_ZOOM_DEFAUT, v.to_string());
    pousser_aux_panneaux();
}

/// Le zoom automatique est-il activé ? Lu une fois puis gardé.
pub fn zoom_auto() -> bool {
    if let Some(v) = ZOOM_AUTO.with(Cell::get) {
        return v;
    }
    let lu = crate::config::load_bool_setting(FICHIER_ZOOM_AUTO);
    ZOOM_AUTO.with(|c| c.set(Some(lu)));
    lu
}

/// Active ou coupe le zoom automatique (case des Réglages) et prévient les
/// panneaux — une sphère ouverte se recadre aussitôt.
pub fn regler_zoom_auto(texte: &str) {
    let actif = match texte.trim() {
        "1" | "true" | "oui" => true,
        "0" | "false" | "non" => false,
        autre => {
            crate::diag!("[sphere] zoom automatique illisible : {autre:?}");
            return;
        }
    };
    ZOOM_AUTO.with(|c| c.set(Some(actif)));
    crate::config::save_bool_setting(FICHIER_ZOOM_AUTO, actif);
    pousser_aux_panneaux();
}

/// Les deux durées du fondu de la sphère : (apparition, disparition), ms.
pub fn fondu() -> (u32, u32) {
    if let Some(v) = FONDU.with(Cell::get) {
        return v;
    }
    let lu = crate::config::config_dir()
        .and_then(|d| std::fs::read_to_string(d.join(FICHIER_FONDU)).ok())
        .and_then(|t| {
            let mut lignes = t.lines();
            let apparition = lignes.next().and_then(|l| entier_dans(l, FONDU_MIN_MS, FONDU_MAX_MS))?;
            let disparition = lignes.next().and_then(|l| entier_dans(l, FONDU_MIN_MS, FONDU_MAX_MS))?;
            Some((apparition, disparition))
        })
        // Jamais réglé : aucun fondu — le comportement d'avant ce réglage.
        .unwrap_or((0, 0));
    FONDU.with(|c| c.set(Some(lu)));
    lu
}

/// Règle une des deux durées (`direction` : `ouverture` ou `fermeture`),
/// l'enregistre, et la pousse aux panneaux.
pub fn regler_fondu(direction: &str, texte: &str) {
    let Some(v) = entier_dans(texte, FONDU_MIN_MS, FONDU_MAX_MS) else {
        crate::diag!("[sphere] duree de fondu illisible : {texte:?}");
        return;
    };
    let (mut apparition, mut disparition) = fondu();
    match direction {
        "ouverture" => apparition = v,
        "fermeture" => disparition = v,
        autre => {
            crate::diag!("[sphere] direction de fondu inconnue : {autre:?}");
            return;
        }
    }
    FONDU.with(|c| c.set(Some((apparition, disparition))));
    crate::config::write_config(FICHIER_FONDU, format!("{apparition}\n{disparition}"));
    pousser_aux_panneaux();
}

/// Un réglage de la sphère change : les deux panneaux relisent leur état (et le
/// passent à `sphere_vue.js`), sans attendre le prochain rafraîchissement.
fn pousser_aux_panneaux() {
    crate::win32::cef_terminal::refresh_if_visible();
    crate::win32::cef_ssh::refresh_if_visible();
}

// ---- Ce que les panneaux appellent ----

/// La page de `source` ouvre (`true`) ou ferme (`false`) sa sphère.
pub fn etat(source: &'static str, ouverte: bool) {
    if ouverte {
        OUVERTE.with(|o| o.set(Some(source)));
        // Ce qu'on a déjà de l'autre panneau, tout de suite ; le neuf suit.
        pousser_les_apercus(source);
        demander_les_vignettes(l_autre(source));
    } else if OUVERTE.with(Cell::get) == Some(source) {
        OUVERTE.with(|o| o.set(None));
    }
}

/// Un clic sur l'aperçu d'une session de l'autre panneau.
pub fn basculer_depuis(depuis: &'static str, onglet: u32) {
    BASCULES.with(|b| b.borrow_mut().push(Bascule { depuis, onglet }));
}

/// Les vignettes déposées par la page de `source` (route POST
/// `<source>/vignettes`). Un dépôt illisible est ignoré — on garde les
/// précédentes.
pub fn recevoir(source: &'static str, octets: Option<Vec<u8>>) {
    let Some(vignettes) = octets.as_deref().and_then(lire_vignettes(source)) else {
        crate::diag!("[sphere] vignettes de {source} illisibles");
        return;
    };
    VIGNETTES.with(|v| v.borrow_mut()[index_de(source)] = vignettes);
    if let Some(ouverte) = OUVERTE.with(Cell::get) {
        if ouverte != source {
            pousser_les_apercus(ouverte);
        }
    }
}

/// Un tableau d'OBJETS, borné, chacun marqué de sa source (la page ne décide
/// pas de quel panneau elle parle).
fn lire_vignettes(source: &'static str) -> impl Fn(&[u8]) -> Option<Vec<Value>> {
    move |octets| {
        let Value::Array(liste) = serde_json::from_slice::<Value>(octets).ok()? else { return None };
        Some(
            liste
                .into_iter()
                .filter_map(|v| match v {
                    Value::Object(mut objet) if objet.get("onglet").and_then(Value::as_u64).is_some() => {
                        objet.insert("source".into(), json!(source));
                        Some(Value::Object(objet))
                    }
                    _ => None,
                })
                .take(VIGNETTES_MAX)
                .collect(),
        )
    }
}

/// Pousse à la sphère de `vers` les aperçus de l'autre panneau.
fn pousser_les_apercus(vers: &'static str) {
    let apercus = Value::Array(VIGNETTES.with(|v| v.borrow()[index_de(l_autre(vers))].clone()));
    if vers == SSH {
        crate::win32::cef_ssh::sphere_autres(&apercus);
    } else {
        crate::win32::cef_terminal::sphere_autres(&apercus);
    }
}

fn demander_les_vignettes(a: &str) {
    DERNIERE_DEMANDE.with(|d| d.set(Some(Instant::now())));
    if a == SSH {
        crate::win32::cef_ssh::demander_vignettes();
    } else {
        crate::win32::cef_terminal::demander_vignettes();
    }
}

fn est_visible(source: &str) -> bool {
    if source == SSH { crate::win32::cef_ssh::is_visible() } else { crate::win32::cef_terminal::is_visible() }
}

// ---- Servi par `app::run`, chaque frame ----

/// Sert les bascules en file, rend le clavier au panneau montré, et renouvelle
/// les aperçus. `rect_terminal` / `rect_ssh` : la place de chaque panneau.
pub fn servir(gfx: &GfxDevice, rect_terminal: [i32; 4], rect_ssh: [i32; 4]) {
    for Bascule { depuis, onglet } in BASCULES.with(|b| std::mem::take(&mut *b.borrow_mut())) {
        let vers = l_autre(depuis);
        crate::diag!("[sphere] bascule de {depuis} vers {vers}, onglet {onglet}");
        if vers == SSH {
            crate::win32::cef_terminal::hide_if_visible();
            crate::win32::cef_ssh::montrer_en_sphere(rect_ssh, gfx, onglet);
        } else {
            crate::win32::cef_ssh::hide_if_visible();
            crate::win32::cef_terminal::montrer_en_sphere(rect_terminal, gfx, onglet);
        }
        CLAVIER.with(|c| c.set(Some((vers, Instant::now()))));
    }
    reclamer_le_clavier();
    let Some(ouverte) = OUVERTE.with(Cell::get) else { return };
    let du = DERNIERE_DEMANDE.with(Cell::get).map_or(true, |t| t.elapsed() >= RAFRAICHIR);
    if du && est_visible(ouverte) {
        demander_les_vignettes(l_autre(ouverte));
    }
}

/// Voir [`CLAVIER`].
fn reclamer_le_clavier() {
    let Some((qui, depuis)) = CLAVIER.with(Cell::get) else { return };
    let obtenu = if qui == SSH {
        crate::win32::cef_ssh::donner_le_clavier()
    } else {
        crate::win32::cef_terminal::donner_le_clavier()
    };
    if obtenu {
        crate::diag!("[sphere] clavier donné à {qui} après {} ms", depuis.elapsed().as_millis());
        CLAVIER.with(|c| c.set(None));
    } else if depuis.elapsed() >= RECLAMER_LE_CLAVIER {
        crate::diag!("[sphere] ⚠️ {qui} n'a pas obtenu le clavier en {} ms", RECLAMER_LE_CLAVIER.as_millis());
        CLAVIER.with(|c| c.set(None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VUE_JS: &str = include_str!("../../assets/popups/sphere_vue.js");
    const SOURCE_JS: &str = include_str!("../../assets/popups/sphere_source.js");
    const TERMINAL_HTML: &str = include_str!("../../assets/popups/terminal.html");
    const SSH_HTML: &str = include_str!("../../assets/popups/ssh.html");

    /// La page ne choisit pas sa source, et les entrées qui ne sont pas des
    /// vignettes tombent.
    #[test]
    fn les_vignettes_sont_marquees_et_filtrees() {
        let lire = lire_vignettes(SSH);
        let lu = lire(br#"[{"onglet":2,"source":"terminal","titre":"a"},{"titre":"sans onglet"},3]"#).unwrap();
        assert_eq!(lu.len(), 1);
        assert_eq!(lu[0]["source"], "ssh");
        assert_eq!(lu[0]["titre"], "a");
        assert!(lire(b"{}").is_none(), "pas un tableau");
        assert!(lire(b"pas du json").is_none());
        let beaucoup = format!("[{}]", vec![r#"{"onglet":1}"#; VIGNETTES_MAX + 10].join(","));
        assert_eq!(lire(beaucoup.as_bytes()).unwrap().len(), VIGNETTES_MAX);
    }

    /// Fermer la sphère d'UN panneau ne ferme pas celle de l'autre.
    #[test]
    fn l_etat_suit_le_panneau_qui_a_ouvert() {
        OUVERTE.with(|o| o.set(Some(TERMINAL)));
        etat(SSH, false);
        assert_eq!(OUVERTE.with(Cell::get), Some(TERMINAL));
        etat(TERMINAL, false);
        assert_eq!(OUVERTE.with(Cell::get), None);
        assert_eq!(l_autre(TERMINAL), SSH);
        assert_eq!(l_autre(SSH), TERMINAL);
    }

    /// Le clic molette n'est PAS capté là où il appartient déjà à quelqu'un :
    /// un onglet (il ferme le groupe), un champ, un programme qui suit la souris
    /// — ni quand la sphère est ouverte (il est alors à elle).
    #[test]
    fn le_clic_molette_respecte_ses_proprietaires() {
        assert!(SOURCE_JS.contains(".onglet, input, textarea"), "onglets et champs exclus");
        assert!(SOURCE_JS.contains("mouseTrackingMode !== \"none\""), "programmes qui suivent la souris");
        assert!(SOURCE_JS.contains("addEventListener(\"mousedown\", surAppui, true)"), "en phase de capture");
        assert!(SOURCE_JS.contains("if (!options || sphereOuverte() ||"), "la sphère ouverte garde son clic molette");
        for raccourci in RACCOURCIS {
            assert!(SOURCE_JS.contains(&format!("\"{raccourci}\"")), "{raccourci} inconnu de la page");
            assert!(VUE_JS.contains(&format!("\"{raccourci}\"")), "{raccourci} absent du réglage");
        }
    }

    /// ⭐ Les terminaux de la sphère sont les VRAIS : l'élément de la session
    /// entre dans sa carte, à sa taille réelle, et xterm n'est pas réajusté.
    #[test]
    fn les_cartes_portent_les_vrais_terminaux() {
        assert!(VUE_JS.contains("c.echelle.appendChild(element)"), "l'élément n'entre pas dans sa carte");
        assert!(VUE_JS.contains("transform: `scale(${k})`"), "réduit par scale, pas redimensionné");
        for (nom, page) in [("terminal", TERMINAL_HTML), ("ssh", SSH_HTML)] {
            assert!(page.contains("<script src=\"sphere_vue.js\"></script>"), "{nom} : sphere_vue.js absent");
            assert!(page.contains("sphereVue.installer({"), "{nom} : sphère non installée");
            assert!(page.contains("if (window.sphereVue && sphereVue.ouverte()) return;"), "{nom} : ajuster redimensionnerait les shells");
            assert!(page.contains("if (window.sphereVue) sphereVue.resynchroniser();"), "{nom} : disposer volerait les terminaux");
            assert!(page.contains(&format!("source: \"{nom}\"")), "{nom} : installé sous un autre nom");
            assert!(page.contains("sphereSource.rafraichir()"), "{nom} : l'indicateur ne suit pas les onglets");
            assert!(page.contains("sphereSource.raccourci(state.raccourci)"), "{nom} : le raccourci n'arrive pas");
        }
    }

    /// ⛔ Chromium ne trouve pas les cartes 3D sous le pointeur (`e.target` = la
    /// scène, mesuré) : les gestes passent par `carteSous`.
    #[test]
    fn les_gestes_trouvent_la_carte_par_projection() {
        assert!(!VUE_JS.contains("e.target.closest("), "un geste lit encore e.target");
        assert!(VUE_JS.contains("function carteSous(x, y)"));
        assert!(VUE_JS.matches("carteSous(e.clientX, e.clientY)").count() >= 4, "molette, clic, double-clic, survol");
        assert!(include_str!("cef_popup.rs").contains("        WM_MBUTTONDOWN => {"), "le clic molette n'est plus transmis à CEF");
        assert!(include_str!("cef_app.rs").contains("MouseButtonType::MIDDLE"));
    }

    /// La vitesse du zoom : un entier borné, et rien d'illisible.
    #[test]
    fn la_vitesse_du_zoom_est_bornee() {
        assert_eq!(vitesse_valide("150"), Some(150));
        assert_eq!(vitesse_valide(" 87.6 "), Some(88));
        assert_eq!(vitesse_valide("1"), Some(VITESSE_ZOOM_MIN));
        assert_eq!(vitesse_valide("100000"), Some(VITESSE_ZOOM_MAX));
        assert_eq!(vitesse_valide("NaN"), None);
        assert_eq!(vitesse_valide("vite"), None);
    }

    /// Du curseur de Réglages à la molette de la sphère, dans les deux panneaux.
    #[test]
    fn la_vitesse_du_zoom_est_cablee_de_bout_en_bout() {
        let reglages = include_str!("../../assets/popups/settings.html");
        assert!(reglages.contains(r#"go("sphere-zoom", v)"#), "le curseur n'émet pas le verbe");
        assert!(reglages.contains("state.sphereZoom"), "le curseur ne lit pas l'état");
        let dispatch = include_str!("cef_settings.rs");
        assert!(dispatch.contains(r#"["sphere-zoom", v] =>"#), "Réglages ne reçoit pas le verbe");
        assert!(dispatch.contains(r#""sphereZoom": crate::win32::cef_sphere::vitesse_zoom()"#));
        for (nom, page, rust) in [("terminal", TERMINAL_HTML, include_str!("cef_terminal.rs")), ("ssh", SSH_HTML, include_str!("cef_ssh.rs"))] {
            assert!(page.contains("sphereVue.vitesseZoom(state.sphereZoom)"), "{nom} : la vitesse n'arrive pas à la sphère");
            assert!(rust.contains(r#""sphereZoom": crate::win32::cef_sphere::vitesse_zoom()"#), "{nom} : la vitesse n'est pas poussée");
        }
        assert!(VUE_JS.contains("e.deltaY * rayon / 1500 * vitesseZoom"), "la molette ignore la vitesse");
    }

    /// ⭐ Les réglages PROPRES à la sphère des terminaux — les MÊMES
    /// possibilités que la sphère des programmes (demande du 2026-11-04) :
    /// les curseurs et la case de `settings.html`, les verbes du dispatch, les
    /// clefs que lit la page de Réglages, et celles que les DEUX panneaux
    /// poussent jusqu'à `sphere_vue.js`. Un réglage câblé d'un seul côté est un
    /// réglage qui ne fait rien — vu trois fois dans ce dépôt.
    #[test]
    fn les_reglages_de_la_sphere_des_terminaux_sont_cables_des_deux_cotes() {
        let reglages = include_str!("../../assets/popups/settings.html");
        let dispatch = include_str!("cef_settings.rs");
        for (cle, verbe) in [
            ("sphereTerminauxZoom", "sphere-terminaux-zoom"),
            ("sphereTerminauxZoomAuto", "sphere-terminaux-zoom-auto"),
            ("sphereTerminauxFonduOuverture", "sphere-terminaux-fondu"),
            ("sphereTerminauxFonduFermeture", "sphere-terminaux-fondu"),
        ] {
            assert!(reglages.contains(cle), "la page de Réglages ne lit pas {cle}");
            assert!(reglages.contains(&format!("go(\"{verbe}\"")), "la page n'émet pas {verbe}");
            assert!(dispatch.contains(&format!("[\"{verbe}\"")), "le dispatch ne reçoit pas {verbe}");
        }
        for (nom, page, rust) in
            [("terminal", TERMINAL_HTML, include_str!("cef_terminal.rs")), ("ssh", SSH_HTML, include_str!("cef_ssh.rs"))]
        {
            assert!(page.contains("sphereVue.reglages(state)"), "{nom} : les réglages n'arrivent pas à la sphère");
            for cle in ["sphereZoomDefaut", "sphereZoomAuto", "sphereFonduOuverture", "sphereFonduFermeture"] {
                assert!(rust.contains(&format!("\"{cle}\"")), "{nom} : {cle} n'est pas poussé");
            }
        }
        // La page applique la distance d'ouverture — ou le recadrage automatique,
        // qui ne fait que reculer par rapport à elle.
        assert!(VUE_JS.contains("Math.min(zoomParDefaut(), cadrageAuto())"), "l'ouverture n'arbitre pas");
        assert!(VUE_JS.contains("recadrerSiAuto()"), "la scène ne se recadre pas quand elle change");
        // Le fondu est celui des Réglages, et une transition doit pouvoir partir
        // d'un élément qui vient de `display: none`.
        assert!(VUE_JS.contains("fonduApparition") && VUE_JS.contains("fonduDisparition"));
        assert!(VUE_JS.contains("void racine.offsetWidth"), "la transition ne partirait pas de `display: none`");
    }

    /// Les réglages reçus sont BORNÉS, comme ceux de la sphère des programmes :
    /// un fichier ou une page ne doit pas régler un zoom ou un fondu absurde.
    #[test]
    fn les_reglages_de_la_sphere_sont_bornees() {
        assert_eq!(entier_dans("120", ZOOM_DEFAUT_MIN, ZOOM_DEFAUT_MAX), Some(120));
        assert_eq!(entier_dans("400", ZOOM_DEFAUT_MIN, ZOOM_DEFAUT_MAX), Some(ZOOM_DEFAUT_MAX));
        assert_eq!(entier_dans("-10", ZOOM_DEFAUT_MIN, ZOOM_DEFAUT_MAX), Some(ZOOM_DEFAUT_MIN));
        assert_eq!(entier_dans("abc", ZOOM_DEFAUT_MIN, ZOOM_DEFAUT_MAX), None);
        assert_eq!(entier_dans("0", FONDU_MIN_MS, FONDU_MAX_MS), Some(0), "0 ms = aucun fondu");
        assert_eq!(entier_dans("99999", FONDU_MIN_MS, FONDU_MAX_MS), Some(FONDU_MAX_MS));
    }

    /// Échap est la SEULE touche prise par la sphère, et seulement ouverte ;
    /// Entrée va au terminal (demande du 2026-11-01). Le script des panneaux
    /// n'écoute aucune touche : en mode normal, Échap est à vim, less…
    #[test]
    fn seul_echap_est_pris_et_seulement_dans_la_sphere() {
        assert!(!SOURCE_JS.contains("keydown"), "le script des panneaux écoute le clavier");
        assert_eq!(VUE_JS.matches("\"keydown\"").count(), 1);
        assert!(VUE_JS.contains("if (!ouverte || e.key !== \"Escape\") return;"));
        assert!(!VUE_JS.contains("\"Enter\""), "Entrée appartient au terminal");
    }

    /// Dans le terminal choisi, la souris SÉLECTIONNE et le clic droit ouvre le
    /// menu Copier / Coller (demande du 2026-11-01) : la sphère relaie une copie
    /// de chaque geste à l'élément visé, coordonnées ramenées à l'échelle 1
    /// autour de l'écran d'xterm — sauf le menu, posé sous le VRAI pointeur. Les
    /// copies (`isTrusted` faux) ne doivent ni être reprises par le relais ni
    /// faire tourner la sphère.
    #[test]
    fn la_souris_du_terminal_choisi_est_relayee() {
        for geste in ["mousedown", "mousemove", "mouseup", "click", "dblclick", "contextmenu"] {
            assert!(VUE_JS.contains(&format!("window.addEventListener(\"{geste}\", (e) => {{")), "{geste} n'est pas relayé");
        }
        assert!(VUE_JS.contains("relayer(\"contextmenu\", e, sous, sous, echelleDe(c), true);"), "le menu doit garder les coordonnées réelles");
        assert!(VUE_JS.contains("x.querySelector(\".xterm-screen\")"), "xterm mesure sur son écran");
        assert!(VUE_JS.contains("if (!e.isTrusted || e.button === 1 || !scene.contains(e.target)) return;"), "le clic molette doit rester à la sphère");
        assert!(VUE_JS.contains("scene.addEventListener(\"mousedown\", (e) => {
      if (!e.isTrusted) return;"), "une copie relayée ferait tourner la sphère");
        assert!(VUE_JS.contains("scene.addEventListener(\"dblclick\", (e) => {
      if (!e.isTrusted) return;"), "une copie relayée ramènerait en mode normal");
    }
}
