//! Styles de l'interface — l'axe « style » du système de thèmes façon KDE
//! (voir `docs/plans/styles-couleurs-icones.md`) : les FORMES (rayons,
//! contours, police, structure), indépendantes des COULEURS (`ui::color`) et
//! des icônes.
//!
//! ⚠️ **UN STYLE EST UN FICHIER DE VALEURS, JAMAIS DU CSS NI DU JS.** Chaque
//! valeur est validée ici (bornes, énumérations, police restreinte à
//! `[A-Za-z0-9 -]`), puis c'est `variables_js` qui ÉCRIT les variables CSS
//! `--nd-*` : aucune chaîne lue dans un fichier n'atteint une page telle
//! quelle. C'est ce qui rend sûrs les styles utilisateur (lot 7 du plan).
//!
//! Les styles intégrés sont des TOML embarqués lus par le MÊME analyseur que
//! les styles utilisateur, et `assets/popups/style.css` porte en `:root` les
//! valeurs de `neuradesk.toml` : une page affichée avant la première poussée
//! a déjà l'aspect d'origine.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

/// Nom du fichier de `%APPDATA%\NeuraDesk\` qui retient le style choisi.
const FICHIER_CHOIX: &str = "style.txt";
/// La police choisie dans Réglages, qui REMPLACE celle du style (voir
/// `choisir_police`). Absente ou vide : chaque style garde la sienne.
const FICHIER_POLICE: &str = "police.txt";

/// Identifiant du style de base, dont les valeurs complètent tout autre style.
pub const STYLE_BASE: &str = "neuradesk";

/// Style ACTIF tant que l'utilisateur n'en a choisi aucun (pas de `style.txt`) —
/// DEMANDE EXPLICITE (2026-10-12) : « le style par défaut doit être Cinnamon ».
/// ⚠️ À ne pas confondre avec `STYLE_BASE`, qui COMPLÈTE tous les styles.
pub const STYLE_PAR_DEFAUT: &str = "cinnamon";

const INTEGRES: [(&str, &str); 11] = [
    (STYLE_BASE, include_str!("../../assets/styles/neuradesk.toml")),
    ("winui", include_str!("../../assets/styles/winui.toml")),
    ("programme", include_str!("../../assets/styles/programme.toml")),
    ("retro", include_str!("../../assets/styles/retro.toml")),
    ("kde", include_str!("../../assets/styles/kde.toml")),
    ("gnome", include_str!("../../assets/styles/gnome.toml")),
    ("cosmic", include_str!("../../assets/styles/cosmic.toml")),
    ("elementary", include_str!("../../assets/styles/elementary.toml")),
    ("cinnamon", include_str!("../../assets/styles/cinnamon.toml")),
    ("android", include_str!("../../assets/styles/android.toml")),
    ("deepin", include_str!("../../assets/styles/deepin.toml")),
];

const RAYON_MAX: i64 = 32;
const POLICE_LONGUEUR_MAX: usize = 48;
const ID_LONGUEUR_MAX: usize = 32;

/// Signal de l'élément actif d'une liste (catégorie de Réglages, onglet…),
/// traduit en `data-nd-indicateur` sur `<html>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indicateur {
    /// Aplat teinté de l'accent — l'aspect d'origine.
    Point,
    /// Barre verticale d'accent à gauche de l'élément, façon WinUI.
    Barre,
}

impl Indicateur {
    fn depuis(texte: &str) -> Option<Self> {
        match texte {
            "point" => Some(Self::Point),
            "barre" => Some(Self::Barre),
            _ => None,
        }
    }

    fn nom(self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Barre => "barre",
        }
    }
}

/// **LE RELIEF DESSINÉ** — quel jeu d'images le style demande (voir `gfx::relief`).
///
/// ⚠️ **UNE IMAGE N'EST PAS UNE VALEUR, ET C'EST CE QUI LA REND DÉLICATE ICI.**
/// Tout le système de styles repose sur « un TOML de valeurs validées, jamais du
/// CSS ni du JS » — c'est ce qui rend un style utilisateur inoffensif. Une image
/// échappe à cette règle : elle est embarquée dans l'exécutable (`include_bytes!`),
/// donc un style ne peut PAS en apporter. Le TOML ne nomme donc qu'un jeu
/// CONNU, et un nom inconnu est refusé comme n'importe quelle valeur hors bornes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReliefImage {
    /// Aucune image : le relief vient du shader, ou il n'y en a pas.
    Aucune,
    /// Les bandes « aluminium brossé » (voir `tools/rasteriser_aluminium.py`).
    Aluminium,
}

impl ReliefImage {
    fn depuis(texte: &str) -> Option<Self> {
        match texte {
            "" | "aucune" => Some(Self::Aucune),
            "aluminium" => Some(Self::Aluminium),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    /// Identifiant (nom de fichier sans extension) — `[a-z0-9_-]` seulement.
    pub id: String,
    /// Nom affiché dans Réglages.
    pub nom: String,
    pub rayon_petit: u8,
    pub rayon_controle: u8,
    pub rayon_surface: u8,
    pub rayon_grand: u8,
    /// Coins des icônes du dock — dessinés par Rust (`ui::dock`), sans
    /// variable CSS.
    pub rayon_icone: u8,
    pub bordure_alpha: f32,
    /// **RELIEF** (0.0–1.0 chacun) — l'axe du style « Rétro », et **à zéro
    /// partout ailleurs** : c'est ce qui garantit que « NeuraDesk » et « WinUI »
    /// ne bougent pas d'un pixel (test `le_relief_est_letre_du_seul_style_retro`).
    ///
    /// ⚠️ **ILS S'AJOUTENT AU MATÉRIAU DE FLOU, ILS NE LE REMPLACENT PAS.** Le
    /// pixel shader porte DÉJÀ un bandeau lumineux (`gloss`) et un liseré de
    /// biseau (`rim`) — voir `gfx::material::PostParams`, dont « Verre liquide »
    /// (`rim 0.55`) et « Frutiger Aero » (`gloss 0.45`) se servent. Les deux axes
    /// restent donc indépendants, comme le veut le modèle (style ≠ couleurs ≠
    /// matériau).
    pub relief_degrade: f32,
    pub relief_liserel: f32,
    /// ⚠️ **LES DASHS SEULS.** La fenêtre du dock est découpée par `SetWindowRgn`
    /// exactement aux quatre rectangles des barres — et c'est aussi la zone de
    /// CLIC : une ombre qui en déborderait demanderait d'élargir la région, donc
    /// de voler des clics au bureau. Décision de l'utilisateur (2026-10-05) : le
    /// relief des barres vient de la LUMIÈRE.
    pub relief_ombre: f32,
    /// Le relief DESSINÉ (voir `ReliefImage`) — `Aucune` partout ailleurs, et c'est
    /// le même garde-fou que les trois nombres ci-dessus.
    pub relief_image: ReliefImage,
    pub police: String,
    pub indicateur: Indicateur,
    /// **LA SURFACE EST-ELLE OPAQUE ?** — `false` partout sauf « Programme ».
    ///
    /// ⚠️ **LE SEUL AXE QUI TOUCHE LA TRANSLUCIDITÉ, ET C'EST VOULU.** Le fond d'un
    /// popup est normalement le BUREAU FLOUTÉ teinté (`win32::cef_popup::render`,
    /// `tint_rgba`) : l'allure « verre » du projet. Un panneau de VRAIE application
    /// ne laisse rien transparaître — DEMANDE EXPLICITE (2026-11-06) : « donne aux
    /// dashs et menus l'aspect d'un programme normal… ça ne ressemble pas à une
    /// application ». À `true`, les pages peignent `<html>` avec `--popup-bg-rgb`
    /// (le pôle de surface OPAQUE, `color::menu_on_fg`, contrasté avec le texte
    /// `--popup-fg-rgb`) : le verre disparaît, le panneau devient une surface pleine.
    /// Piloté par l'attribut `data-nd-surface` (voir `variables_js`), lu par
    /// `style.css` et `menu.css` — jamais par Rust, qui continue de dessiner son
    /// backdrop (couvert par la page). N'affecte donc PAS les outils de capture.
    pub surface_opaque: bool,
}

impl Style {
    /// Les valeurs de `assets/styles/neuradesk.toml` — le test
    /// `neuradesk_toml_egale_la_base` garde les deux alignés.
    fn base() -> Self {
        Self {
            id: STYLE_BASE.to_string(),
            nom: "NeuraDesk".to_string(),
            rayon_petit: 3,
            rayon_controle: 6,
            rayon_surface: 8,
            rayon_grand: 10,
            rayon_icone: 14,
            bordure_alpha: 0.0,
            relief_degrade: 0.0,
            relief_liserel: 0.0,
            relief_ombre: 0.0,
            relief_image: ReliefImage::Aucune,
            police: "Segoe UI".to_string(),
            indicateur: Indicateur::Point,
            surface_opaque: false,
        }
    }
}

fn id_valide(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= ID_LONGUEUR_MAX
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

pub(crate) fn police_valide(police: &str) -> bool {
    !police.is_empty()
        && police.len() <= POLICE_LONGUEUR_MAX
        && police.bytes().all(|b| b.is_ascii_alphanumeric() || b == b' ' || b == b'-')
}

/// Nom affiché : caractères imprimables seulement, jamais de balisage (il
/// passera par `textContent` côté page, mais autant ne rien laisser passer).
fn nom_valide(nom: &str) -> bool {
    !nom.trim().is_empty()
        && nom.chars().count() <= POLICE_LONGUEUR_MAX
        && nom.chars().all(|c| !c.is_control() && !matches!(c, '<' | '>' | '&' | '"' | '\'' | '\\'))
}

/// Lit un style. Toute clé absente ou invalide garde la valeur du style de
/// base ; chaque rejet est décrit dans les avertissements renvoyés — jamais un
/// style à moitié appliqué sans trace. `None` si l'identifiant est invalide ou
/// si le texte n'est pas du TOML.
pub fn analyser(id: &str, source: &str) -> Option<(Style, Vec<String>)> {
    if !id_valide(id) {
        return None;
    }
    let table = source.parse::<toml::Table>().ok()?;
    let mut avertissements = Vec::new();
    let mut style = Style { id: id.to_string(), ..Style::base() };

    for (cle, valeur) in &table {
        match (cle.as_str(), valeur) {
            ("nom", toml::Value::String(nom)) if nom_valide(nom) => style.nom = nom.trim().to_string(),
            ("formes" | "traits" | "texte" | "structure" | "relief", toml::Value::Table(section)) => {
                for (sous_cle, v) in section {
                    if let Err(motif) = appliquer_cle(&mut style, cle, sous_cle, v) {
                        avertissements.push(format!("{cle}.{sous_cle} : {motif}"));
                    }
                }
            }
            _ => avertissements.push(format!("{cle} : clé inconnue ou valeur invalide")),
        }
    }
    Some((style, avertissements))
}

fn appliquer_cle(style: &mut Style, section: &str, cle: &str, valeur: &toml::Value) -> Result<(), &'static str> {
    let rayon = || match valeur {
        toml::Value::Integer(n) if (0..=RAYON_MAX).contains(n) => Ok(*n as u8),
        _ => Err("entier de 0 à 32 attendu"),
    };
    // Un RELIEF se dit en fraction (0 = aucun, 1 = au maximum) : mêmes bornes que
    // `bordure_alpha`, et la même tolérance à l'entier nu (`0` et `1` s'écrivent
    // naturellement sans décimale dans un TOML).
    let fraction = || match valeur {
        toml::Value::Float(f) if (0.0..=1.0).contains(f) => Ok(*f as f32),
        toml::Value::Integer(n) if (0..=1).contains(n) => Ok(*n as f32),
        _ => Err("nombre de 0 à 1 attendu"),
    };
    match (section, cle) {
        ("relief", "degrade") => style.relief_degrade = fraction()?,
        ("relief", "liserel") => style.relief_liserel = fraction()?,
        ("relief", "ombre") => style.relief_ombre = fraction()?,
        // ⚠️ Un NOM, pas un chemin : un style ne peut nommer qu'un jeu CONNU (voir
        // `ReliefImage`). C'est ce qui garde la règle « jamais du CSS, jamais un
        // fichier » intacte.
        ("relief", "image") => {
            style.relief_image = valeur.as_str().and_then(ReliefImage::depuis).ok_or("\"aucune\" ou \"aluminium\" attendu")?
        }
        ("formes", "rayon_petit") => style.rayon_petit = rayon()?,
        ("formes", "rayon_controle") => style.rayon_controle = rayon()?,
        ("formes", "rayon_surface") => style.rayon_surface = rayon()?,
        ("formes", "rayon_grand") => style.rayon_grand = rayon()?,
        ("formes", "rayon_icone") => style.rayon_icone = rayon()?,
        ("traits", "bordure_alpha") => {
            let alpha = match valeur {
                toml::Value::Float(f) => *f,
                toml::Value::Integer(n) => *n as f64,
                _ => return Err("nombre de 0 à 1 attendu"),
            };
            if !(0.0..=1.0).contains(&alpha) {
                return Err("nombre de 0 à 1 attendu");
            }
            style.bordure_alpha = alpha as f32;
        }
        ("texte", "police") => match valeur {
            toml::Value::String(p) if police_valide(p.trim()) => style.police = p.trim().to_string(),
            _ => return Err("nom de police invalide (lettres, chiffres, espaces, tirets)"),
        },
        ("structure", "indicateur") => {
            style.indicateur = valeur.as_str().and_then(Indicateur::depuis).ok_or("\"point\" ou \"barre\" attendu")?
        }
        ("structure", "surface") => {
            style.surface_opaque = match valeur.as_str() {
                Some("verre") => false,
                Some("opaque") => true,
                _ => return Err("\"verre\" ou \"opaque\" attendu"),
            }
        }
        _ => return Err("clé inconnue"),
    }
    Ok(())
}

/// Les styles livrés avec l'application, dans l'ordre d'affichage — lus une
/// fois : Réglages les demande à chaque frame.
pub fn integres() -> &'static [Style] {
    static LUS: OnceLock<Vec<Style>> = OnceLock::new();
    LUS.get_or_init(|| INTEGRES.iter().filter_map(|(id, source)| analyser(id, source).map(|(style, _)| style)).collect())
}

/// Le style `id`, ou le style de base s'il n'existe pas.
pub fn trouver(id: &str) -> Style {
    integres().iter().find(|s| s.id == id).cloned().unwrap_or_else(Style::base)
}

/// La valeur CSS de `--nd-police` : la police du style, puis Segoe UI si elle
/// manque sur la machine. `police` est déjà validée (`police_valide`), les
/// guillemets doubles ne peuvent donc pas en sortir.
fn pile_de_polices(police: &str) -> String {
    if police == "Segoe UI" {
        "\"Segoe UI\", sans-serif".to_string()
    } else {
        format!("\"{police}\", \"Segoe UI\", sans-serif")
    }
}

/// Le JS qui pose les variables `--nd-*` et les attributs `data-nd-*` sur
/// `<html>`. Les attributs ne sont réécrits que s'ils changent : poussé à
/// chaque frame, un `setAttribute` inconditionnel relancerait le calcul des
/// styles de la page à chaque frame.
pub fn variables_js(style: &Style) -> String {
    format!(
        "(function(d){{var s=d.style;\
           s.setProperty('--nd-rayon-petit','{}px');\
           s.setProperty('--nd-rayon-controle','{}px');\
           s.setProperty('--nd-rayon-surface','{}px');\
           s.setProperty('--nd-rayon-grand','{}px');\
           s.setProperty('--nd-bordure-alpha','{}');\
           s.setProperty('--nd-relief-degrade','{}');\
           s.setProperty('--nd-relief-liserel','{}');\
           s.setProperty('--nd-relief-ombre','{}');\
           s.setProperty('--nd-police','{}');\
           function a(k,v){{if(d.getAttribute(k)!==v)d.setAttribute(k,v);}}\
           a('data-nd-style','{}');a('data-nd-indicateur','{}');a('data-nd-surface','{}');\
         }})(document.documentElement)",
        style.rayon_petit,
        style.rayon_controle,
        style.rayon_surface,
        style.rayon_grand,
        style.bordure_alpha,
        style.relief_degrade,
        style.relief_liserel,
        style.relief_ombre,
        pile_de_polices(&style.police),
        style.id,
        style.indicateur.nom(),
        if style.surface_opaque { "opaque" } else { "verre" },
    )
}

/// Le style actif et son JS, calculé une fois par changement de style (il est
/// poussé à chaque frame par `win32::cef_popup`).
static ACTIF: Mutex<Option<(Style, String)>> = Mutex::new(None);

/// Numéro du style actif, augmenté à chaque `activer` : les textures de texte
/// que Rust garde en cache (infobulle, titres de zone) sont faites dans la
/// police du style, et le comparer dit quand les refaire.
static GENERATION: AtomicU32 = AtomicU32::new(0);

fn avec_actif<T>(f: impl FnOnce(&Style, &str) -> T) -> T {
    let mut garde = ACTIF.lock().unwrap_or_else(|e| e.into_inner());
    let (style, js) = garde.get_or_insert_with(|| (Style::base(), variables_js(&Style::base())));
    f(style, js)
}

pub fn js_actif() -> String {
    avec_actif(|_, js| js.to_string())
}

pub fn id_actif() -> String {
    avec_actif(|style, _| style.id.clone())
}

pub fn generation() -> u32 {
    GENERATION.load(Ordering::Relaxed)
}

/// Les formes que Rust dessine lui-même (lot 4 du plan), en pixels AVANT mise
/// à l'échelle — relues à chaque frame, comme la palette.
pub fn rayon_petit() -> f32 {
    avec_actif(|style, _| f32::from(style.rayon_petit))
}

pub fn rayon_controle() -> f32 {
    avec_actif(|style, _| f32::from(style.rayon_controle))
}

pub fn rayon_grand() -> f32 {
    avec_actif(|style, _| f32::from(style.rayon_grand))
}

pub fn rayon_icone() -> f32 {
    avec_actif(|style, _| f32::from(style.rayon_icone))
}

/// **LE RELIEF DU STYLE, POUR CE QUE RUST DESSINE** — les barres du dock (voir
/// `gfx::quad_renderer`, où ces deux valeurs s'AJOUTENT au `rim`/`gloss` du
/// matériau de flou) et les dashs (`relief_ombre`, voir `win32::cef_popup`).
///
/// ⚠️ **AUCUNE VARIABLE CSS POUR `relief_degrade`/`relief_liserel`** : les barres
/// ne sont pas des pages, elles n'ont pas de `--nd-*`. Ils partent donc par
/// `variables_js` (qui les porte pour les surfaces INTÉRIEURES des pages) **et**
/// par ces accesseurs — les deux chemins sont distincts, et c'est voulu : une
/// page ne peut pas peindre le fond d'une barre.
pub fn relief_degrade() -> f32 {
    avec_actif(|style, _| style.relief_degrade)
}

pub fn relief_liserel() -> f32 {
    avec_actif(|style, _| style.relief_liserel)
}

/// Le relief DESSINÉ du style actif (voir `gfx::relief`, qui le charge).
pub fn relief_image() -> ReliefImage {
    avec_actif(|style, _| style.relief_image)
}

/// La police du style pour le texte que Rust rend lui-même (infobulle, titres
/// de zone, titres des aperçus). Une police absente de la machine est
/// remplacée par GDI, pas par Segoe UI : `police_valide` n'en garantit que la
/// FORME.
pub fn police() -> String {
    avec_actif(|style, _| style.police.clone())
}

/// **LA POLICE CHOISIE PAR L'UTILISATEUR** — DEMANDE EXPLICITE (2026-10-12) :
/// « serait-il possible de choisir la police de caractère à utiliser dans les
/// réglages ». `None` : la police du style.
///
/// ⚠️ **CHOISIR UN STYLE LA RETIRE** (voir `choisir`) — DEMANDE EXPLICITE
/// (2026-10-12) : « changer de style devrait régler la police automatiquement ».
/// Elle ne vaut donc que jusqu'au prochain changement de style ; le bouton
/// « Police du style » de Réglages la retire aussi.
static POLICE_CHOISIE: Mutex<Option<String>> = Mutex::new(None);

/// La police choisie, si elle remplace celle du style.
pub fn police_choisie() -> Option<String> {
    POLICE_CHOISIE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// La police PROPRE au style actif, quelle que soit la police choisie.
pub fn police_du_style() -> String {
    trouver(&id_actif()).police
}

/// Le style tel qu'il s'affiche : le sien, police remplacée si l'utilisateur en
/// a choisi une.
fn avec_la_police(mut style: Style, police: Option<&str>) -> Style {
    if let Some(p) = police {
        style.police = p.to_string();
    }
    style
}

/// **CHOISIT LA POLICE** et la retient. `""` revient à celle du style. Un nom
/// que `police_valide` refuse est refusé (il vient d'une page, et finit dans une
/// variable CSS).
pub fn choisir_police(nom: &str) -> bool {
    let nom = nom.trim();
    if !nom.is_empty() && !police_valide(nom) {
        crate::diag!("police : {nom:?} refusée");
        return false;
    }
    let choix = (!nom.is_empty()).then(|| nom.to_string());
    *POLICE_CHOISIE.lock().unwrap_or_else(|e| e.into_inner()) = choix;
    crate::config::write_config(FICHIER_POLICE, nom.to_string());
    activer(&id_actif());
    crate::diag!("police : {} choisie", if nom.is_empty() { "celle du style" } else { nom });
    true
}

/// Retire la police choisie, en mémoire ET sur disque.
fn oublier_la_police() {
    *POLICE_CHOISIE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    crate::config::write_config(FICHIER_POLICE, String::new());
}

/// Nom du fichier de la **police des terminaux** : un nom de police, vide = la
/// pile par défaut portée par `assets/popups/terminal_theme.js`.
const FICHIER_POLICE_TERMINAL: &str = "police_terminal.txt";

/// **LA POLICE DES TERMINAUX** — DEMANDE EXPLICITE (2026-10-28) : « ajoute le
/// réglage de police des terminaux ». `None` = la pile par défaut de la page
/// (`pileDesPolicesDesTerminaux`), qui commence à Nordwand Mono embarquée.
///
/// ⚠️ **Elle PRÉCÈDE la pile, elle ne la remplace pas** — c'est le point utile.
/// Nordwand Mono ne dessine ni les boîtes ni les blocs (mesuré le 2026-10-28 :
/// `U+250F` et `U+2588` absents de son cmap, glyphe de repli 18 % plus large que
/// la cellule). Une pile coupée au choix de l'utilisateur rendrait ces glyphes
/// en `monospace` générique ; en le plaçant devant, DejaVu Sans Mono les prête.
///
/// ⚠️ **Elle ne SUIT PAS le style**, contrairement à `POLICE_CHOISIE` que
/// `choisir` retire : changer de style d'interface ne dit rien de ce que
/// l'utilisateur veut dans ses terminaux.
///
/// ⚠️ **Le Terminal ET les Serveurs SSH la partagent** (`terminal.html`,
/// `ssh.html`) : un réglage qui se dit « des terminaux » et n'en gouvernerait
/// qu'un seul ment.
static POLICE_TERMINAL: Mutex<Option<String>> = Mutex::new(None);

/// La police des terminaux ; `None` = la pile par défaut de la page.
pub fn police_des_terminaux() -> Option<String> {
    POLICE_TERMINAL.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// **CHOISIT LA POLICE DES TERMINAUX** — `""` revient à la pile par défaut.
/// Même validation que l'interface : le nom vient d'une page et finit dans la
/// chaîne `fontFamily` d'xterm.js. Un nom refusé n'écrit RIEN, ni en mémoire ni
/// sur disque — il ne peut donc pas atteindre la page.
pub fn choisir_police_des_terminaux(nom: &str) -> bool {
    let nom = nom.trim();
    if !nom.is_empty() && !police_valide(nom) {
        crate::diag!("police des terminaux : {nom:?} refusée");
        return false;
    }
    let choix = (!nom.is_empty()).then(|| nom.to_string());
    *POLICE_TERMINAL.lock().unwrap_or_else(|e| e.into_inner()) = choix;
    crate::config::write_config(FICHIER_POLICE_TERMINAL, nom.to_string());
    crate::diag!(
        "police des terminaux : {}",
        if nom.is_empty() { "pile par defaut" } else { nom }
    );
    true
}

/// Rend `id` actif (sans le persister — voir `choisir`).
pub fn activer(id: &str) {
    let style = avec_la_police(trouver(id), police_choisie().as_deref());
    let js = variables_js(&style);
    *ACTIF.lock().unwrap_or_else(|e| e.into_inner()) = Some((style, js));
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Relit le style choisi au démarrage. `NEURADESK_DEBUG_STYLE=<id>` impose un
/// style SANS l'enregistrer — pour capturer un style sans toucher à la
/// configuration de l'utilisateur.
pub fn charger_choix() {
    let police = crate::config::config_path(FICHIER_POLICE)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| t.trim().to_string())
        .filter(|p| police_valide(p));
    // `NEURADESK_DEBUG_POLICE=<nom>` impose une police SANS l'enregistrer — pour
    // la capturer sans toucher à la configuration (même règle que le style).
    let police = std::env::var("NEURADESK_DEBUG_POLICE").ok().filter(|p| police_valide(p)).or(police);
    *POLICE_CHOISIE.lock().unwrap_or_else(|e| e.into_inner()) = police;
    // LA POLICE DES TERMINAUX, relue ici (voir `POLICE_TERMINAL`) : ce
    // démarrage est le seul endroit qui connaisse les polices réglées, et les
    // deux surfaces de texte réglables vivent côte à côte. Placée AVANT le
    // `return` de `NEURADESK_DEBUG_STYLE` : un style imposé pour une capture ne
    // doit pas effacer la police des terminaux.
    let terminale = crate::config::config_path(FICHIER_POLICE_TERMINAL)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| t.trim().to_string())
        .filter(|p| police_valide(p));
    // `NEURADESK_DEBUG_POLICE_TERMINALE=<nom>` l'impose SANS l'enregistrer —
    // même règle que `NEURADESK_DEBUG_POLICE` au-dessus : capturer une police
    // sans toucher à la configuration de l'utilisateur.
    let terminale = std::env::var("NEURADESK_DEBUG_POLICE_TERMINALE")
        .ok()
        .filter(|p| police_valide(p))
        .or(terminale);
    *POLICE_TERMINAL.lock().unwrap_or_else(|e| e.into_inner()) = terminale;
    if let Ok(id) = std::env::var("NEURADESK_DEBUG_STYLE") {
        activer(&id);
        crate::diag!("style : {} imposé par NEURADESK_DEBUG_STYLE", id_actif());
        return;
    }
    let id = crate::config::config_path(FICHIER_CHOIX)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| t.trim().to_string())
        .unwrap_or_default();
    activer(if id_valide(&id) { &id } else { STYLE_PAR_DEFAUT });
    crate::diag!("style : {} actif au démarrage", id_actif());
}

/// Rend `id` actif et le retient pour les prochains lancements. Un
/// identifiant qui ne désigne aucun style est refusé (il vient d'une page).
/// ⚠️ La police choisie est RETIRÉE : le style apporte la sienne (voir
/// `POLICE_CHOISIE`).
pub fn choisir(id: &str) -> bool {
    if !integres().iter().any(|s| s.id == id) {
        crate::diag!("style : {id:?} inconnu, refusé");
        return false;
    }
    oublier_la_police();
    activer(id);
    crate::config::write_config(FICHIER_CHOIX, id.to_string());
    crate::diag!("style : {id} choisi");
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le style par défaut doit exister : sinon `activer` retomberait sur un style
    /// inconnu au premier lancement.
    #[test]
    fn le_style_par_defaut_est_cinnamon_et_existe() {
        assert_eq!(STYLE_PAR_DEFAUT, "cinnamon");
        assert!(integres().iter().any(|s| s.id == STYLE_PAR_DEFAUT), "aucun style integre ne s appelle {STYLE_PAR_DEFAUT}");
    }

    #[test]
    fn neuradesk_toml_egale_la_base() {
        let (style, avertissements) = analyser(STYLE_BASE, INTEGRES[0].1).unwrap();
        assert!(avertissements.is_empty(), "{avertissements:?}");
        assert_eq!(style, Style::base());
    }

    #[test]
    fn les_styles_integres_se_lisent_sans_avertissement() {
        for (id, source) in INTEGRES {
            let (_, avertissements) = analyser(id, source).unwrap();
            assert!(avertissements.is_empty(), "{id} : {avertissements:?}");
        }
        let winui = trouver("winui");
        assert_eq!((winui.rayon_controle, winui.indicateur), (4, Indicateur::Barre));
        assert_eq!(winui.rayon_icone, 8);
        let programme = trouver("programme");
        assert_eq!((programme.rayon_controle, programme.rayon_grand, programme.indicateur), (6, 10, Indicateur::Barre));
        assert_eq!((programme.bordure_alpha, programme.relief_ombre, programme.police.as_str()), (0.16, 0.3, "Segoe UI Variable Text"));
        // ⭐ L'axe de surface : « Programme » est le SEUL style opaque ; tous les
        // autres restent en verre (pôle par défaut).
        assert!(programme.surface_opaque, "« Programme » doit être la surface opaque");
        assert!(!winui.surface_opaque, "les autres styles restent en verre");
        assert!(variables_js(&programme).contains("'data-nd-surface','opaque'"));
        assert!(variables_js(&winui).contains("'data-nd-surface','verre'"));
        let kde = trouver("kde");
        assert_eq!((kde.rayon_controle, kde.indicateur, kde.police.as_str()), (3, Indicateur::Point, "Noto Sans"));
        let gnome = trouver("gnome");
        assert_eq!((gnome.rayon_controle, gnome.rayon_grand, gnome.police.as_str()), (6, 12, "Adwaita Sans Text"));
        let cosmic = trouver("cosmic");
        assert_eq!((cosmic.rayon_grand, cosmic.bordure_alpha, cosmic.police.as_str()), (16, 0.0, "Open Sans"));
        let elementary = trouver("elementary");
        assert_eq!((elementary.relief_ombre, elementary.police.as_str()), (0.35, "Inter"));
        let cinnamon = trouver("cinnamon");
        assert_eq!((cinnamon.rayon_controle, cinnamon.indicateur, cinnamon.police.as_str()), (3, Indicateur::Barre, "Ubuntu"));
        let android = trouver("android");
        assert_eq!((android.rayon_controle, android.rayon_grand, android.police.as_str()), (20, 28, "Roboto"));
        let deepin = trouver("deepin");
        assert_eq!((deepin.rayon_controle, deepin.rayon_grand, deepin.relief_ombre, deepin.police.as_str()), (8, 12, 0.3, "Noto Sans"));
    }

    /// ⛔ **AUCUNE BARRE NE PORTE DE RELIEF, ET C'EST UNE DÉCISION, PAS UN DÉFAUT.**
    ///
    /// L'utilisateur l'a dit après les avoir vues : « le relief sur les barres,
    /// non » (2026-10-06). Trois essais l'avaient précédé — un liseré qui faisait
    /// un CADRE, un biseau à deux tons (« mieux »), et un relief dessiné en
    /// aluminium brossé. Les trois relevaient du même axe : c'est l'AXE qui a été
    /// refusé, pas un dosage.
    ///
    /// ⚠️ **CE TEST GARDE LES STYLES INTÉGRÉS EN PHASE** : sans lui, une
    /// valeur de relief oubliée dans un TOML ferait dériver une barre en silence,
    /// et rien ne dirait lequel des styles a changé.
    #[test]
    fn aucun_style_integre_ne_porte_de_relief_sur_les_barres() {
        for style in integres() {
            assert_eq!(
                (style.relief_degrade, style.relief_liserel),
                (0.0, 0.0),
                "« {} » ne doit pas éclairer ses barres : décision de l'utilisateur du 2026-10-06, \
                 « le relief sur les barres, non »",
                style.id
            );
            assert_eq!(
                style.relief_image,
                ReliefImage::Aucune,
                "« {} » ne doit pas poser d'image sur ses barres, pour la même raison",
                style.id
            );
        }
    }

    /// ⚠️ **ET LE MÉCANISME, LUI, DOIT RESTER VIVANT** — c'est le pendant du test
    /// ci-dessus, et il compte autant : si un jour quelqu'un retire le relief des
    /// styles intégrés *et* le code qui le consomme, on se retrouve avec du
    /// vocabulaire que rien n'écoute. Ce dépôt tient qu'un réglage sans effet est
    /// un mensonge. Ce test échouerait aussi, mais en le DISANT.
    #[test]
    fn le_relief_reste_une_valeur_que_l_on_peut_demander() {
        let source = "[relief]\ndegrade = 0.4\nliserel = 0.3\nimage = \"aluminium\"\n";
        let (style, avertissements) = analyser("essai-relief", source).unwrap();
        assert!(avertissements.is_empty(), "{avertissements:?}");
        assert_eq!((style.relief_degrade, style.relief_liserel), (0.4, 0.3));
        assert_eq!(style.relief_image, ReliefImage::Aluminium);
        // Et un nom d'image INCONNU est refusé, comme toute valeur hors bornes.
        let (_, avis) = analyser("essai-image", "[relief]\nimage = \"bois\"\n").unwrap();
        assert_eq!(avis.len(), 1, "un jeu d'images inconnu doit être signalé : {avis:?}");
    }

    /// Sans style activé, les formes dessinées par Rust gardent les anciennes
    /// valeurs fixes : 14 px (icônes), 6 px (vignettes), 3 px (pastilles) et
    /// 10 px (menus).
    #[test]
    fn les_formes_de_rust_valent_les_anciennes_constantes() {
        assert_eq!((rayon_icone(), rayon_controle(), rayon_petit(), rayon_grand()), (14.0, 6.0, 3.0, 10.0));
        assert_eq!(police(), "Segoe UI");
    }

    /// La police choisie remplace celle du style — et RIEN d'autre ; sans choix,
    /// le style garde la sienne.
    #[test]
    fn la_police_choisie_remplace_celle_du_style_et_rien_d_autre() {
        let gnome = trouver("gnome");
        let avec = avec_la_police(gnome.clone(), Some("Inter"));
        assert_eq!(avec.police, "Inter");
        assert_eq!(Style { police: gnome.police.clone(), ..avec }, gnome);
        assert_eq!(avec_la_police(gnome.clone(), None), gnome);
        assert!(variables_js(&avec_la_police(gnome, Some("Inter"))).contains("'--nd-police','\"Inter\", \"Segoe UI\", sans-serif'"));
    }

    #[test]
    fn une_valeur_invalide_garde_la_base_et_laisse_une_trace() {
        let source = "[formes]\nrayon_controle = 99\nrayon_surface = 12\n[traits]\nbordure_alpha = 2.0\n[structure]\nindicateur = \"soulignement\"\n";
        let (style, avertissements) = analyser("essai", source).unwrap();
        assert_eq!(style.rayon_controle, 6);
        assert_eq!(style.rayon_surface, 12);
        assert_eq!(style.bordure_alpha, 0.0);
        assert_eq!(style.indicateur, Indicateur::Point);
        assert_eq!(avertissements.len(), 3, "{avertissements:?}");
    }

    #[test]
    fn une_police_ou_un_nom_hostile_est_refuse() {
        for police in ["x'); alert(1); ('", "a\"b", "Segoe;UI", "a\\b", "</style>"] {
            let source = format!("[texte]\npolice = {}\n", toml::Value::String(police.to_string()));
            let (style, avertissements) = analyser("essai", &source).unwrap();
            assert_eq!(style.police, "Segoe UI", "{police}");
            assert_eq!(avertissements.len(), 1, "{police}");
        }
        let (style, _) = analyser("essai", "nom = \"<img src=x>\"\n").unwrap();
        assert_eq!(style.nom, "NeuraDesk");
    }

    #[test]
    fn un_identifiant_invalide_est_refuse() {
        for id in ["", "Majuscule", "a b", "a'b", "../x", &"x".repeat(33)] {
            assert!(analyser(id, "").is_none(), "{id}");
        }
        assert!(analyser("essai", "pas = = du toml").is_none());
    }

    /// `style.css` porte en `:root` les valeurs du style de base : une page
    /// affichée avant la première poussée doit déjà avoir l'aspect d'origine.
    #[test]
    fn style_css_porte_les_valeurs_de_la_base() {
        let css = include_str!("../../assets/popups/style.css");
        let js = variables_js(&Style::base());
        let mut vues = 0;
        for ligne in css.lines().map(str::trim).filter(|l| l.starts_with("--nd-")) {
            let (nom, valeur) = ligne.trim_end_matches(';').split_once(':').unwrap();
            assert!(js.contains(&format!("'{}','{}'", nom.trim(), valeur.trim())), "{ligne}");
            vues += 1;
        }
        assert_eq!(vues, js.matches("setProperty(").count());
    }

    #[test]
    fn un_style_inconnu_retombe_sur_la_base() {
        assert_eq!(trouver("inexistant"), Style::base());
    }

    #[test]
    fn le_js_porte_les_valeurs_du_style() {
        let js = variables_js(&trouver("winui"));
        assert!(js.contains("'--nd-rayon-controle','4px'"));
        assert!(js.contains("'--nd-police','\"Segoe UI Variable Text\", \"Segoe UI\", sans-serif'"));
        assert!(js.contains("a('data-nd-style','winui')"));
        assert!(js.contains("a('data-nd-indicateur','barre')"));
    }

    /// ⭐ **UN NOM DE POLICE DE TERMINAL NE PEUT PAS CASSER LA PILE CSS.** La page
    /// le quote dans `terminal_theme.js` (`pileDesPolicesDesTerminaux`) et xterm.js
    /// lit la chaîne comme une `font-family` : un guillemet ou une virgule échappés
    /// de la validation changerait la pile elle-même, pas seulement sa tête.
    ///
    /// ⚠️ **Ce test ne teste QUE le refus** — le chemin d'acceptation écrit
    /// `police_terminal.txt` dans la configuration réelle de l'utilisateur, et une
    /// suite de tests n'écrit pas dans le réglage de quelqu'un (le même piège a déjà
    /// mordu le harnais de capture).
    #[test]
    fn une_police_de_terminal_refusee_ne_stocke_rien() {
        assert!(!police_valide("Noto\"Mono"), "un guillemet fermerait le quotage de la page");
        assert!(!police_valide("Noto,Mono"), "une virgule serait une AUTRE pile, pas un nom");
        assert!(police_valide("Cascadia Mono"), "une espace fait partie d'un nom normal");
        assert!(!choisir_police_des_terminaux("Noto\"Mono"));
        assert!(!choisir_police_des_terminaux("Noto,Mono"));
        assert_eq!(police_des_terminaux(), None, "un nom refusé ne doit rien avoir stocké");
    }
}
