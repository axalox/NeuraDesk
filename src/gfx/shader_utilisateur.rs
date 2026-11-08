//! ⭐ **LES MOTIFS DE L'UTILISATEUR** — un shader écrit à la main, dessiné comme
//! les 75 motifs intégrés du dégradé animé.
//!
//! DEMANDE (2026-10-10) : « une applet pour disposer d'un éditeur de shader qui
//! permette de les utiliser dans les dégradés animés ». Ce module est le MOTEUR
//! (lot 1) ; l'éditeur viendra par-dessus.
//!
//! **CE QU'ÉCRIT L'UTILISATEUR** : un fichier `%APPDATA%\NeuraDesk\shaders\<nom>.hlsl`
//! qui définit `float4 Motif(float2 p, float aspect, float t)` — la couleur et la
//! hauteur (voir `DegMotif` dans `quad_renderer`), avec toutes les aides des
//! motifs intégrés à sa disposition. `source_utilisateur` le pose dans la source
//! complète ; il reçoit donc le flou, le relief, la perspective et les couleurs
//! du thème comme les autres.
//!
//! ⭐ **OU UN SHADER SHADERTOY**, collé tel quel : `<nom>.glsl`, traduit par
//! `gfx::shadertoy` avant d'être compilé comme un `.hlsl`.
//!
//! ⛔ **CE SHADER TOURNE SUR TOUTE L'INTERFACE, JUSQU'À 144 FOIS PAR SECONDE** —
//! d'où trois gardes :
//! 1. **la compilation se fait sur un fil à part** (`choisir`) : elle a été
//!    mesurée jusqu'à des dizaines de secondes dans ce dépôt, et le fil
//!    d'interface ne fait que créer le shader depuis des octets déjà compilés ;
//! 2. **un ESSAI suit chaque activation** (`surveiller`, `Essai`) : si les images
//!    deviennent lentes, le motif est retiré et l'intégré reprend ;
//! 3. **une marque sur le disque** (`MARQUE_ESSAI`) tient pendant l'essai : si la
//!    session s'arrête avant la fin (un pilote réinitialisé par une boucle sans
//!    fin, un plantage), le lancement suivant ne réactive PAS le motif.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Le préfixe qui désigne un motif de l'utilisateur dans `degrade_mode.txt`.
pub const PREFIXE: &str = "shader:";

/// Posée pendant l'essai, retirée quand il est réussi — voir la doc de module.
const MARQUE_ESSAI: &str = "shader_en_essai.txt";

/// Au-delà, le fichier n'est pas lu — un motif tient en quelques Ko.
const TAILLE_MAX: u64 = 256 * 1024;

/// Une image plus longue que ça est LENTE (20 images par seconde).
const IMAGE_LENTE: Duration = Duration::from_millis(50);
/// La durée de l'essai, comptée en temps où le dégradé est DESSINÉ.
const DUREE_ESSAI: Duration = Duration::from_secs(4);
/// Les premières images d'un motif portent la création du shader : pas comptées.
const IMAGES_IGNOREES: u32 = 3;
/// Assez d'images lentes pour conclure sans attendre la fin de l'essai.
const LENTES_POUR_CONCLURE: u32 = 20;

/// Le modèle d'un motif neuf — l'exemple du dossier, et le « Nouveau » de
/// l'éditeur (`win32::cef_shaders`).
pub const MODELE: &str = EXEMPLE;

const EXEMPLE: &str = r#"// Un motif du dégradé animé — voir « Mes shaders » dans les Réglages.
//
// `p`      : la position, centrée, en hauteurs d'écran (de -0,5 à 0,5 en hauteur)
// `aspect` : largeur / hauteur de l'écran
// `t`      : le temps, déjà multiplié par le curseur « Vitesse »
//
// Rend la couleur (rgb) et une HAUTEUR (a) dont se servent le relief et la
// perspective. Les aides des motifs intégrés sont disponibles :
//   DegPalette(x)  : la couleur du thème à la position x (0..1)
//   DegFbm(p)      : un bruit fractal (-1..1)
//   DegBruit(p)    : un bruit de gradient (-1..1)
//   DegHash(p)     : un nombre pseudo-aléatoire (0..1)
// et les curseurs : DegEchelle, DegWarp, DegDouceur.

float4 Motif(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle * 2.0;
    float vague = sin(q.x * 3.0 + t) + sin(q.y * 4.0 - t * 0.7) + DegFbm(q + t * 0.1) * (1.0 + DegWarp);
    float x = 0.5 + 0.25 * vague;
    return float4(DegPalette(x), x);
}
"#;

struct Actif {
    nom: String,
    generation: u64,
    octets: Arc<Vec<u8>>,
}

static ACTIF: Mutex<Option<Actif>> = Mutex::new(None);
/// La génération du motif actif — `0` = aucun. Lue à CHAQUE rendu, d'où un
/// atomique devant le verrou.
static GENERATION_ACTIVE: AtomicU64 = AtomicU64::new(0);
static GENERATIONS: AtomicU64 = AtomicU64::new(0);
/// Le numéro de la dernière demande : une compilation plus ancienne qui finit
/// après une plus récente est jetée.
static DEMANDES: AtomicU64 = AtomicU64::new(0);
static EN_COMPILATION: Mutex<Option<String>> = Mutex::new(None);
static MESSAGE: Mutex<String> = Mutex::new(String::new());
static ESSAI_EN_COURS: AtomicBool = AtomicBool::new(false);
static ESSAI: Mutex<Option<Essai>> = Mutex::new(None);
/// La liste du dossier et l'heure de sa lecture (voir `lister_en_cache`) ; vidée
/// par `ecrire_source`, pour qu'un motif enregistré apparaisse aussitôt.
static CACHE_LISTE: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);

fn dossier() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("shaders"))
}

/// Un nom de motif : celui d'un fichier `.hlsl` du dossier, sans chemin.
pub fn nom_valide(nom: &str) -> bool {
    !nom.is_empty()
        && nom.chars().count() <= 60
        && nom.chars().all(|c| c.is_alphanumeric() || " -_'().".contains(c))
        && !nom.starts_with('.')
}

/// Les deux langues d'un motif, par l'extension de son fichier : `.hlsl` (une
/// fonction `Motif`) ou `.glsl` (un shader Shadertoy, voir `gfx::shadertoy`).
const EXTENSIONS: [&str; 2] = ["hlsl", "glsl"];

fn chemin_en(nom: &str, extension: &str) -> Option<PathBuf> {
    nom_valide(nom).then(|| dossier().map(|d| d.join(format!("{nom}.{extension}")))).flatten()
}

/// Le fichier du motif `nom` : le `.glsl` s'il existe, sinon le `.hlsl`.
/// `ecrire_source` n'en laisse jamais qu'un des deux.
fn fichier(nom: &str) -> Option<PathBuf> {
    let glsl = chemin_en(nom, "glsl")?;
    if glsl.exists() {
        return Some(glsl);
    }
    chemin_en(nom, "hlsl")
}

fn est_du_glsl(chemin: &std::path::Path) -> bool {
    chemin.extension().is_some_and(|e| e == "glsl")
}

/// **LES MOTIFS DU DOSSIER**, triés — le dossier est créé avec un exemple la
/// première fois, pour qu'il y ait de quoi essayer.
pub fn lister() -> Vec<String> {
    let Some(dossier) = dossier() else { return Vec::new() };
    if !dossier.exists() && std::fs::create_dir_all(&dossier).is_ok() {
        let _ = std::fs::write(dossier.join("exemple.hlsl"), EXEMPLE);
    }
    let mut noms: Vec<String> = std::fs::read_dir(&dossier)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let chemin = e.path();
            let extension = chemin.extension()?.to_str()?;
            EXTENSIONS.contains(&extension).then(|| chemin.file_stem()?.to_str().map(str::to_string)).flatten()
        })
        .filter(|n| nom_valide(n))
        .collect();
    noms.sort_by_key(|n| n.to_lowercase());
    noms.dedup();
    noms
}

/// **`lister`, AU PLUS UNE FOIS TOUTES LES DEUX SECONDES** — pour l'état des
/// Réglages, poussé à chaque image : lire le dossier à 60 Hz ne se fait pas.
pub fn lister_en_cache() -> Vec<String> {
    let Ok(mut cache) = CACHE_LISTE.lock() else { return lister() };
    match cache.as_ref() {
        Some((quand, noms)) if quand.elapsed() < Duration::from_secs(2) => noms.clone(),
        _ => {
            let noms = lister();
            *cache = Some((Instant::now(), noms.clone()));
            noms
        }
    }
}

/// Le code du motif `nom`, pour l'éditeur.
pub fn lire_source(nom: &str) -> Result<String, String> {
    lire(&fichier(nom).ok_or("nom de motif refusé")?)
}

/// **ÉCRIT LE CODE DU MOTIF `nom`** — à côté puis RENOMMÉ, pour qu'un disque plein
/// ne laisse jamais un motif à moitié écrit.
///
/// ⭐ **LA LANGUE SE LIT DANS LE CODE** : un shader Shadertoy collé (il définit
/// `mainImage`) devient `<nom>.glsl`, le reste `<nom>.hlsl` — et le fichier de
/// l'autre langue est retiré, pour qu'un nom ne désigne qu'un motif.
pub fn ecrire_source(nom: &str, code: &str) -> Result<(), String> {
    let (extension, autre) = if crate::gfx::shadertoy::est_shadertoy(code) { ("glsl", "hlsl") } else { ("hlsl", "glsl") };
    let refus = "nom refusé : lettres, chiffres, espaces et - _ ' ( ) . seulement";
    let chemin = chemin_en(nom, extension).ok_or(refus)?;
    let ancien = chemin_en(nom, autre).ok_or(refus)?;
    if code.len() as u64 > TAILLE_MAX {
        return Err(format!("code trop long ({} octets)", code.len()));
    }
    std::fs::create_dir_all(chemin.parent().ok_or("dossier introuvable")?).map_err(|e| e.to_string())?;
    let provisoire = chemin.with_extension(format!("{extension}.partiel"));
    std::fs::write(&provisoire, code).map_err(|e| e.to_string())?;
    std::fs::rename(&provisoire, &chemin).map_err(|e| {
        let _ = std::fs::remove_file(&provisoire);
        e.to_string()
    })?;
    if ancien.exists() {
        std::fs::remove_file(&ancien).map_err(|e| format!("l'ancien fichier {} n'a pas pu être retiré : {e}", ancien.display()))?;
    }
    if let Ok(mut cache) = CACHE_LISTE.lock() {
        *cache = None;
    }
    Ok(())
}

/// Le dossier, pour l'ouvrir dans l'Explorateur.
pub fn chemin_du_dossier() -> Option<PathBuf> {
    let d = dossier()?;
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

pub fn nom_actif() -> Option<String> {
    ACTIF.lock().ok()?.as_ref().map(|a| a.nom.clone())
}

pub fn en_compilation() -> Option<String> {
    EN_COMPILATION.lock().ok()?.clone()
}

pub fn message() -> String {
    MESSAGE.lock().map(|m| m.clone()).unwrap_or_default()
}

pub fn dire(texte: impl Into<String>) {
    let texte = texte.into();
    crate::diag!("[shader] {texte}");
    if let Ok(mut m) = MESSAGE.lock() {
        *m = texte;
    }
}

pub fn generation_active() -> u64 {
    GENERATION_ACTIVE.load(Ordering::Relaxed)
}

/// Les octets du motif actif — `None` sans motif de l'utilisateur.
pub fn octets_actifs() -> Option<(u64, Arc<Vec<u8>>)> {
    if generation_active() == 0 {
        return None;
    }
    ACTIF.lock().ok()?.as_ref().map(|a| (a.generation, a.octets.clone()))
}

/// **CHOISIT LE MOTIF `nom`** — la compilation part sur un fil ; jusqu'à sa fin,
/// le motif d'avant reste dessiné.
pub fn choisir(nom: &str) {
    let Some(chemin) = fichier(nom) else {
        dire(format!("nom de motif refusé : {nom:?}"));
        return;
    };
    let demande = DEMANDES.fetch_add(1, Ordering::Relaxed) + 1;
    if let Ok(mut c) = EN_COMPILATION.lock() {
        *c = Some(nom.to_string());
    }
    dire(format!("Compilation de « {nom} »…"));
    let nom = nom.to_string();
    std::thread::spawn(move || {
        let resultat = lire(&chemin).and_then(|code| compiler_le_motif(&code, est_du_glsl(&chemin)));
        if DEMANDES.load(Ordering::Relaxed) != demande {
            return; // Une demande plus récente est passée entre-temps.
        }
        if let Ok(mut c) = EN_COMPILATION.lock() {
            *c = None;
        }
        match resultat {
            Ok(octets) => activer(&nom, octets),
            Err(erreur) => dire(format!("« {nom} » ne compile pas :\n{erreur}")),
        }
    });
}

fn lire(chemin: &std::path::Path) -> Result<String, String> {
    let taille = std::fs::metadata(chemin).map_err(|e| format!("fichier illisible : {e}"))?.len();
    if taille > TAILLE_MAX {
        return Err(format!("fichier trop grand ({taille} octets)"));
    }
    std::fs::read_to_string(chemin).map_err(|e| format!("fichier illisible : {e}"))
}

fn activer(nom: &str, octets: Vec<u8>) {
    let generation = GENERATIONS.fetch_add(1, Ordering::Relaxed) + 1;
    if let Ok(mut a) = ACTIF.lock() {
        *a = Some(Actif { nom: nom.to_string(), generation, octets: Arc::new(octets) });
    }
    // La marque AVANT la génération : le premier rendu qui pourrait figer le
    // pilote a toujours sa marque sur le disque.
    if let Some(chemin) = crate::config::config_path(MARQUE_ESSAI) {
        let _ = std::fs::write(chemin, nom);
    }
    crate::config::save_degrade_mode(&format!("{PREFIXE}{nom}"));
    if let Ok(mut e) = ESSAI.lock() {
        *e = Some(Essai::default());
    }
    ESSAI_EN_COURS.store(true, Ordering::Relaxed);
    GENERATION_ACTIVE.store(generation, Ordering::Relaxed);
    dire(format!("« {nom} » compilé — en essai pendant {} s.", DUREE_ESSAI.as_secs()));
}

/// **RETIRE LE MOTIF DE L'UTILISATEUR** — l'intégré reprend. Une compilation en
/// cours est abandonnée.
pub fn desactiver() {
    DEMANDES.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut c) = EN_COMPILATION.lock() {
        *c = None;
    }
    GENERATION_ACTIVE.store(0, Ordering::Relaxed);
    if let Ok(mut a) = ACTIF.lock() {
        *a = None;
    }
    finir_l_essai();
}

fn finir_l_essai() {
    ESSAI_EN_COURS.store(false, Ordering::Relaxed);
    if let Ok(mut e) = ESSAI.lock() {
        *e = None;
    }
    if let Some(chemin) = crate::config::config_path(MARQUE_ESSAI) {
        let _ = std::fs::remove_file(chemin);
    }
}

/// Retire le motif et rend `degrade_mode.txt` au motif intégré.
fn retirer(raison: String) {
    desactiver();
    crate::config::save_degrade_mode(crate::gfx::degrade::mode_nom());
    dire(raison);
}

/// Le pilote a refusé les octets (voir `quad_renderer::pixel_shader_courant`).
pub fn refuser_par_le_pilote(generation: u64, erreur: &str) {
    if generation_active() == generation {
        let nom = nom_actif().unwrap_or_default();
        retirer(format!("« {nom} » refusé par le pilote : {erreur}"));
    }
}

/// **AU LANCEMENT** (`degrade::charger`) : réactive le motif sauvé — sauf si la
/// session précédente s'est arrêtée pendant son essai.
pub fn charger(nom: &str) {
    let marque = crate::config::config_path(MARQUE_ESSAI).filter(|c| c.exists());
    if let Some(marque) = marque {
        let _ = std::fs::remove_file(marque);
        crate::config::save_degrade_mode(crate::gfx::degrade::mode_nom());
        dire(format!(
            "« {nom} » n'a pas été réactivé : la session précédente s'est arrêtée pendant son essai. Le motif intégré a repris."
        ));
        return;
    }
    choisir(nom);
}

/// **À CHAQUE IMAGE** (`app::run`) — l'essai du motif activé. Sans essai en
/// cours, un seul atomique.
pub fn surveiller(maintenant: Instant) {
    if !ESSAI_EN_COURS.load(Ordering::Relaxed) {
        return;
    }
    let dessine = crate::gfx::degrade::actif();
    let verdict = match ESSAI.lock() {
        Ok(mut e) => match e.as_mut() {
            Some(essai) => essai.image(maintenant, dessine),
            None => return,
        },
        Err(_) => return,
    };
    let nom = nom_actif().unwrap_or_default();
    match verdict {
        Verdict::EnCours => {}
        Verdict::Valide => {
            finir_l_essai();
            dire(format!("« {nom} » est actif."));
        }
        Verdict::TropLent { lentes, images } => retirer(format!(
            "« {nom} » a été retiré : trop lent ({lentes} images lentes sur {images}). Le motif intégré a repris."
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    EnCours,
    Valide,
    TropLent { lentes: u32, images: u32 },
}

/// **L'ESSAI D'UN MOTIF** — fonction pure du temps, pour être testée.
///
/// Ne compte que les images où le dégradé est DESSINÉ (`dessine`) : sans lui, le
/// motif ne coûte rien, et l'écart entre deux images dessinées séparées par une
/// pause ne dit rien du motif.
#[derive(Debug, Default)]
pub struct Essai {
    precedente: Option<Instant>,
    vues: u32,
    images: u32,
    lentes: u32,
    duree: Duration,
}

impl Essai {
    pub fn image(&mut self, maintenant: Instant, dessine: bool) -> Verdict {
        if !dessine {
            self.precedente = None;
            return Verdict::EnCours;
        }
        let Some(precedente) = self.precedente.replace(maintenant) else { return Verdict::EnCours };
        let ecart = maintenant.saturating_duration_since(precedente);
        self.vues += 1;
        if self.vues <= IMAGES_IGNOREES {
            return Verdict::EnCours;
        }
        self.images += 1;
        self.duree += ecart;
        if ecart > IMAGE_LENTE {
            self.lentes += 1;
        }
        let majorite_lente = self.lentes * 2 >= self.images;
        if self.lentes >= LENTES_POUR_CONCLURE && majorite_lente {
            return Verdict::TropLent { lentes: self.lentes, images: self.images };
        }
        if self.duree < DUREE_ESSAI {
            return Verdict::EnCours;
        }
        if majorite_lente {
            Verdict::TropLent { lentes: self.lentes, images: self.images }
        } else {
            Verdict::Valide
        }
    }
}

/// **COMPILE UN MOTIF** — un `.glsl` est d'abord traduit (voir `gfx::shadertoy`).
///
/// ⚠️ **L'ERREUR D'UN SHADER TRADUIT NE DÉSIGNE PAS DE LIGNE** : elle porte sur le
/// HLSL de naga, que l'utilisateur ne voit pas — « ligne 40 » renverrait à une
/// ligne de SON code qui n'a rien à voir. Le message brut est gardé, sans renvoi.
fn compiler_le_motif(code: &str, glsl: bool) -> Result<Vec<u8>, String> {
    let compiler = |hlsl: &str| {
        crate::gfx::shader::compiler_pour_l_utilisateur(&crate::gfx::quad_renderer::source_utilisateur(hlsl), "PSMain", "ps_5_0")
    };
    if !glsl {
        return compiler(code).map_err(|brut| erreurs_lisibles(&brut));
    }
    let hlsl = crate::gfx::shadertoy::traduire(code)?;
    compiler(&hlsl).map_err(|brut| {
        let premieres: Vec<&str> = brut.lines().filter(|l| !l.trim().is_empty()).take(3).collect();
        format!("la traduction en HLSL ne compile pas :
{}", premieres.join("
").replace("motif(", "traduction("))
    })
}

/// **LE MESSAGE DU COMPILATEUR, RAMENÉ AU CODE DE L'UTILISATEUR** — les lignes qui
/// désignent `motif(ligne,colonne)` (voir `source_utilisateur`), réécrites
/// « ligne 3, colonne 5 : … ». Une erreur qui ne désigne pas son code (une aide
/// mal appelée, `Motif` absent) garde ses premières lignes brutes.
pub fn erreurs_lisibles(brut: &str) -> String {
    let lues: Vec<String> = brut
        .lines()
        .filter_map(|ligne| {
            let debut = ligne.find("motif(")?;
            let reste = &ligne[debut + "motif(".len()..];
            let (position, texte) = reste.split_once("):")?;
            let mut nombres = position.split([',', '-']);
            let l = nombres.next()?.trim();
            let c = nombres.next().unwrap_or("").trim();
            let texte = texte.trim();
            Some(if c.is_empty() { format!("ligne {l} : {texte}") } else { format!("ligne {l}, colonne {c} : {texte}") })
        })
        .collect();
    if !lues.is_empty() {
        return lues.join("\n");
    }
    if brut.contains("'Motif'") || brut.contains("Motif") && brut.contains("undeclared") {
        return "la fonction float4 Motif(float2 p, float aspect, float t) est introuvable".to_string();
    }
    brut.lines().filter(|l| !l.trim().is_empty()).take(4).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compiler(code: &str) -> Result<Vec<u8>, String> {
        crate::gfx::shader::compiler_pour_l_utilisateur(&crate::gfx::quad_renderer::source_utilisateur(code), "PSMain", "ps_5_0")
            .map_err(|brut| erreurs_lisibles(&brut))
    }

    /// L'exemple écrit dans le dossier doit compiler — c'est lui qu'on essaie
    /// en premier.
    #[test]
    fn l_exemple_compile() {
        compiler(EXEMPLE).expect("l'exemple doit compiler");
    }

    #[test]
    fn une_erreur_designe_la_ligne_du_motif() {
        let code = "float4 Motif(float2 p, float aspect, float t) {\n    float x = inconnue;\n    return float4(x, x, x, x);\n}\n";
        let erreur = compiler(code).unwrap_err();
        assert!(erreur.starts_with("ligne 2, colonne"), "{erreur}");
        assert!(erreur.contains("inconnue"), "{erreur}");
    }

    #[test]
    fn un_motif_absent_est_dit_clairement() {
        let erreur = compiler("float4 Autre(float2 p) { return 0; }\n").unwrap_err();
        assert!(erreur.contains("Motif"), "{erreur}");
    }

    #[test]
    fn un_nom_de_motif_n_est_jamais_un_chemin() {
        for oui in ["exemple", "Mon motif (2)", "vagues_bleues"] {
            assert!(nom_valide(oui), "{oui}");
        }
        for non in ["", "..", "../config", "a/b", r"a\b", "C:x", ".cache"] {
            assert!(!nom_valide(non), "{non:?}");
        }
    }

    fn essai_a(ecart_ms: u64, images: u32) -> Verdict {
        let mut essai = Essai::default();
        let debut = Instant::now();
        let mut verdict = Verdict::EnCours;
        for i in 0..=images {
            verdict = essai.image(debut + Duration::from_millis(ecart_ms * i as u64), true);
            if verdict != Verdict::EnCours {
                break;
            }
        }
        verdict
    }

    #[test]
    fn un_motif_fluide_est_valide() {
        assert_eq!(essai_a(7, 1000), Verdict::Valide);
    }

    #[test]
    fn un_motif_lent_est_retire_sans_attendre_la_fin() {
        assert!(matches!(essai_a(80, 1000), Verdict::TropLent { lentes: LENTES_POUR_CONCLURE, .. }));
    }

    /// Quelques à-coups (une autre application, un panneau qui s'ouvre) ne
    /// retirent pas un motif fluide.
    #[test]
    fn des_a_coups_ne_retirent_pas_un_motif() {
        let mut essai = Essai::default();
        let mut t = Instant::now();
        let mut verdict = Verdict::EnCours;
        for i in 0..1000 {
            t += Duration::from_millis(if i % 10 == 0 { 120 } else { 7 });
            verdict = essai.image(t, true);
            if verdict != Verdict::EnCours {
                break;
            }
        }
        assert_eq!(verdict, Verdict::Valide);
    }

    /// Une pause du dessin (mode transparent, case décochée) ne compte pas
    /// comme une image lente.
    #[test]
    fn une_pause_du_dessin_ne_compte_pas() {
        let mut essai = Essai::default();
        let debut = Instant::now();
        for i in 0..10 {
            essai.image(debut + Duration::from_millis(7 * i), true);
        }
        essai.image(debut + Duration::from_secs(5), false);
        let mut t = debut + Duration::from_secs(60);
        let mut verdict = Verdict::EnCours;
        for _ in 0..1000 {
            t += Duration::from_millis(7);
            verdict = essai.image(t, true);
            if verdict != Verdict::EnCours {
                break;
            }
        }
        assert_eq!(verdict, Verdict::Valide);
    }

    #[test]
    fn les_premieres_images_ne_comptent_pas() {
        let mut essai = Essai::default();
        let debut = Instant::now();
        // La création du shader : trois écarts de 2 s.
        for i in 0..=IMAGES_IGNOREES as u64 {
            assert_eq!(essai.image(debut + Duration::from_secs(2 * i), true), Verdict::EnCours);
        }
        assert_eq!(essai.lentes, 0);
    }
}
