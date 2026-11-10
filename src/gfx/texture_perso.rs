//! **LA TEXTURE PERSONNALISÉE** — ⭐ DEMANDE EXPLICITE (2026-10-15) : « une applet
//! pour un dash d'édition de textures calculées, évoluer, où les fonctions et
//! réglages sont réalisables avec des cases à cocher et des curseurs ».
//!
//! C'est la nature `perso` de `gfx::texture_interface` (la DERNIÈRE de `NATURES`) :
//! au lieu d'un motif figé, `TexPersoFond` (voir `gfx::quad_renderer`) compose un
//! bruit de base, un fractal, des crêtes, une distorsion, des veines… chacun
//! commandé par un réglage de `REGLAGES`. Force, échelle, clarté et teinte restent
//! celles de `texture_interface`, communes à toutes les natures.
//!
//! **DÉCRITS PAR DES DONNÉES** : la page de l'applet (`assets/popups/textures.html`)
//! construit ses cases et ses curseurs depuis `REGLAGES` — ajouter un réglage ne
//! touche ni la page ni le module de l'applet, seulement cette table et le shader.
//!
//! ⚠️ **L'INDEX D'UN RÉGLAGE EST SA PLACE DANS LE `cbuffer`** (`TexPerso[index / 4]`,
//! composante `index % 4`) : ajouter EN FIN, jamais au milieu. Le test
//! `chaque_reglage_est_lu_a_sa_place_par_le_shader` le garde.

use std::sync::Mutex;
use std::time::Instant;

/// La sorte d'un réglage, qui décide de son contrôle dans la page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sorte {
    /// Une case à cocher : 0 ou 1.
    Case,
    /// Un curseur entre `min` et `max`, par pas de `pas`.
    Curseur,
    /// Une rangée de boutons (pas de `<select>` dans une page CEF) : l'index du choix.
    Choix(&'static [&'static str]),
}

#[derive(Clone, Copy, Debug)]
pub struct Reglage {
    pub id: &'static str,
    pub libelle: &'static str,
    pub sorte: Sorte,
    pub min: f32,
    pub max: f32,
    pub pas: f32,
    pub defaut: f32,
    /// La case qui l'active : sans elle cochée, le réglage n'agit pas (la page
    /// le grise).
    pub depend: Option<&'static str>,
}

const fn case(id: &'static str, libelle: &'static str, defaut: f32) -> Reglage {
    Reglage { id, libelle, sorte: Sorte::Case, min: 0.0, max: 1.0, pas: 1.0, defaut, depend: None }
}

const fn curseur(id: &'static str, libelle: &'static str, min: f32, max: f32, pas: f32, defaut: f32, depend: Option<&'static str>) -> Reglage {
    Reglage { id, libelle, sorte: Sorte::Curseur, min, max, pas, defaut, depend }
}

/// Les bruits de base, dans l'ordre que lit `TexBruit` (voir le shader) —
/// ⚠️ l'index est ENREGISTRÉ (`base=…`) : ajouter EN FIN.
///
/// ⭐ DEMANDE EXPLICITE (2026-10-16) : « avoir tous les bruits de base pour les
/// terminaux et l'app, ajouter une douzaine de bruits de base dont un simple
/// brouillage RGB ». De 4 à 10, les NATURES de `texture_interface` (même ordre,
/// voir le test) ; puis douze nouveaux, dont le brouillage RVB, le seul en couleur.
pub const BASES: &[&str] = &[
    "Valeur", "Perlin", "Cellules", "Ondes",
    "Grain", "Papier", "Toile", "Lin", "Trame", "Sable", "Nuages",
    "Brouillage RVB", "Simplexe", "Fissures", "Vitrail", "Damier", "Rayures", "Pois", "Hexagones", "Briques", "Ondulations", "Moiré", "Écailles",
    // ⭐ DEMANDE EXPLICITE (2026-11-08) : « ajouter d'autres textures modifiables ».
    // Des matières (bois, marbre, granit, métal, carbone, cuir), de l'eau, un
    // plasma (en couleur, comme le brouillage) et deux pavages.
    "Bois", "Marbre", "Granit", "Métal brossé", "Fibre de carbone", "Cuir", "Eau", "Plasma", "Tissage", "Truchet",
    // ⭐ DEMANDE EXPLICITE (2026-11-08) : « ajoutes des motifs de bruit de base » (voir `TexMotif`).
    "Topographie", "Zébrures", "Labyrinthe", "Chevrons", "Spirales", "Étoiles", "Bulles", "Circuit", "Papier millimétré", "Losanges",
];

/// L'index, dans `BASES`, de la première nature de `texture_interface::NATURES`
/// (« grain », son index 1) : nature `n` = base `n + DECALAGE_NATURES`.
#[cfg(test)]
pub const DECALAGE_NATURES: usize = 3;

/// ⚠️ L'ORDRE EST CELUI DU `cbuffer` — ajouter EN FIN (voir l'en-tête).
pub const REGLAGES: [Reglage; 32] = [
    Reglage { id: "base", libelle: "Bruit de base", sorte: Sorte::Choix(BASES), min: 0.0, max: (BASES.len() - 1) as f32, pas: 1.0, defaut: 1.0, depend: None },
    curseur("frequence", "Fréquence", 0.002, 0.1, 0.001, 0.02, None),
    case("fractal", "Fractal (plusieurs octaves)", 1.0),
    curseur("octaves", "Octaves", 1.0, 8.0, 1.0, 4.0, Some("fractal")),
    curseur("lacunarite", "Lacunarité", 1.5, 3.5, 0.05, 2.0, Some("fractal")),
    curseur("gain", "Gain", 0.2, 0.8, 0.01, 0.5, Some("fractal")),
    case("cretes", "Crêtes", 0.0),
    case("turbulence", "Turbulence", 0.0),
    case("distorsion", "Distorsion", 0.0),
    curseur("distorsion_force", "Force de la distorsion", 0.0, 3.0, 0.05, 0.8, Some("distorsion")),
    curseur("distorsion_frequence", "Fréquence de la distorsion", 0.1, 3.0, 0.05, 0.5, Some("distorsion")),
    curseur("angle", "Angle", 0.0, 180.0, 1.0, 0.0, None),
    curseur("etirement", "Étirement", 0.2, 5.0, 0.05, 1.0, None),
    case("veines", "Veines (marbre, bois)", 0.0),
    curseur("veines_frequence", "Fréquence des veines", 1.0, 20.0, 0.5, 5.0, Some("veines")),
    case("posterisation", "Postérisation", 0.0),
    curseur("niveaux", "Niveaux", 2.0, 12.0, 1.0, 4.0, Some("posterisation")),
    curseur("contraste", "Contraste", 0.2, 3.0, 0.05, 1.0, None),
    case("inverser", "Inverser", 0.0),
    case("evoluer", "Évoluer (animation)", 0.0),
    curseur("vitesse", "Vitesse", 0.0, 3.0, 0.05, 0.3, Some("evoluer")),
    // ⭐ DEMANDE EXPLICITE (2026-10-16) : « un réglage du flou ». En pixels : le
    // rayon du disque sur lequel le motif est moyenné (voir `TexPersoFond`).
    curseur("flou", "Flou", 0.0, 20.0, 0.5, 0.0, None),
    // ⭐ DEMANDE EXPLICITE (2026-11-08) : de nouveaux réglages, communs à toutes les
    // bases (voir `TexPersoFond`).
    // Bicolore : les parties claires prennent une seconde teinte (en degrés).
    case("bicolore", "Bicolore", 0.0),
    curseur("seconde_teinte", "Seconde teinte", 0.0, 360.0, 1.0, 200.0, Some("bicolore")),
    curseur("bicolore_force", "Force de la seconde teinte", 0.0, 2.0, 0.05, 1.0, Some("bicolore")),
    // Relief : le motif comme gravé, éclairé d'en haut à gauche. Il prend les deux
    // premières prises du flou, qu'il remplace.
    case("relief", "Relief (embossage)", 0.0),
    curseur("relief_force", "Force du relief", 0.0, 3.0, 0.05, 1.5, Some("relief")),
    curseur("relief_distance", "Épaisseur du relief", 0.5, 6.0, 0.25, 1.5, Some("relief")),
    // Pixeliser : des carrés d'une seule valeur, en pixels d'écran.
    case("pixeliser", "Pixeliser", 0.0),
    curseur("pixel_taille", "Taille des pixels", 2.0, 40.0, 1.0, 8.0, Some("pixeliser")),
    // Kaléidoscope : le motif replié en branches autour du centre de l'écran.
    case("symetrie", "Kaléidoscope", 0.0),
    curseur("branches", "Branches", 2.0, 12.0, 1.0, 6.0, Some("symetrie")),
];

/// Les registres du `cbuffer` (`float4 TexPerso[9]`) : les réglages, puis la
/// PHASE de l'animation dans la toute dernière composante. Passés de 6 à 9 le
/// 2026-11-08, avec les dix réglages du bicolore, du relief, de la pixelisation
/// et du kaléidoscope.
pub const REGISTRES: usize = 9;
const PHASE: usize = REGISTRES * 4 - 1;
const _: () = assert!(REGLAGES.len() <= PHASE, "les réglages débordent sur la phase");

/// Repli de la phase : un `f32` garde sa précision, et le saut est rare.
const PERIODE_PHASE: f32 = 4096.0;

const fn defauts() -> [f32; REGLAGES.len()] {
    let mut v = [0.0; REGLAGES.len()];
    let mut i = 0;
    while i < REGLAGES.len() {
        v[i] = REGLAGES[i].defaut;
        i += 1;
    }
    v
}

/// ⭐ **UNE COMPOSITION PAR CIBLE** — DEMANDE EXPLICITE (2026-10-16) : « éditer les
/// textures d'interface et de fond de terminaux ». L'interface et les terminaux
/// ont chacun la leur, comme chacun a son `texture_interface::ReglagesTexture` :
/// en composer une pour les terminaux ne touche plus celle des barres.
pub struct Composition {
    valeurs: Mutex<[f32; REGLAGES.len()]>,
    /// La phase de l'animation et l'instant où elle a été avancée : elle AVANCE de
    /// `dt × vitesse` (plutôt que `temps × vitesse`), donc changer la vitesse ne
    /// fait pas sauter le motif.
    animation: Mutex<(Option<Instant>, f32)>,
}

/// La composition des barres et des panneaux (le shader la lit à chaque rendu).
pub static INTERFACE: Composition = Composition::new();
/// Celle du fond des terminaux (voir `win32::texture_terminaux`).
pub static TERMINAUX: Composition = Composition::new();

fn index(id: &str) -> Option<usize> {
    REGLAGES.iter().position(|r| r.id == id)
}

/// Borne la valeur au réglage : une case vaut 0 ou 1, un choix un index entier.
fn borner(r: &Reglage, v: f32) -> f32 {
    match r.sorte {
        Sorte::Case => if v >= 0.5 { 1.0 } else { 0.0 },
        Sorte::Choix(_) => v.round().clamp(r.min, r.max),
        Sorte::Curseur => v.clamp(r.min, r.max),
    }
}

impl Composition {
    /// Les valeurs par défaut de `REGLAGES`, animation arrêtée.
    pub const fn new() -> Self {
        Self { valeurs: Mutex::new(defauts()), animation: Mutex::new((None, 0.0)) }
    }

    pub fn valeurs(&self) -> [f32; REGLAGES.len()] {
        *self.valeurs.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[cfg(test)]
    pub fn valeur(&self, id: &str) -> Option<f32> {
        index(id).map(|i| self.valeurs()[i])
    }

    /// Pose un réglage par son ID ; un ID inconnu ou `NaN` est IGNORÉ (rend `false`).
    pub fn set(&self, id: &str, v: f32) -> bool {
        let Some(i) = index(id) else { return false };
        if v.is_nan() {
            return false;
        }
        self.valeurs.lock().unwrap_or_else(|e| e.into_inner())[i] = borner(&REGLAGES[i], v);
        true
    }

    /// Remplace tout le motif — celui d'une texture de la bibliothèque, déjà borné
    /// par `depuis_texte` ou `motif_du_prereglage`.
    pub fn remplacer(&self, motif: [f32; REGLAGES.len()]) {
        *self.valeurs.lock().unwrap_or_else(|e| e.into_inner()) = motif;
    }

    pub fn reinitialiser(&self) {
        self.remplacer(defauts());
    }

    /// Cette texture évolue-t-elle toute seule (case « évoluer » cochée) ?
    /// `app::run` s'en sert pour continuer à PRÉSENTER à chaque image : sans
    /// cela, la texture se figerait au repos (voir « présenter seulement quand
    /// l'image change », `app::run`).
    pub fn anime(&self) -> bool {
        let valeurs = self.valeurs.lock().unwrap_or_else(|e| e.into_inner());
        valeurs[index("evoluer").unwrap_or(0)] > 0.5
    }

    /// Avance la phase de `dt × vitesse` si l'animation est cochée.
    fn avancer_phase(&self, valeurs: &[f32; REGLAGES.len()], maintenant: Instant) -> f32 {
        let mut animation = self.animation.lock().unwrap_or_else(|e| e.into_inner());
        let (dernier, phase) = &mut *animation;
        if let Some(dernier) = *dernier {
            let evoluer = valeurs[index("evoluer").unwrap_or(0)] > 0.5;
            if evoluer {
                let vitesse = valeurs[index("vitesse").unwrap_or(0)];
                *phase = (*phase + maintenant.duration_since(dernier).as_secs_f32() * vitesse) % PERIODE_PHASE;
            }
        }
        *dernier = Some(maintenant);
        *phase
    }

    /// Les registres du `cbuffer` (`TexPerso`) : les réglages à leur index, la phase
    /// en dernier.
    pub(crate) fn constantes(&self) -> [[f32; 4]; REGISTRES] {
        let valeurs = self.valeurs();
        let mut plat = [0.0f32; REGISTRES * 4];
        plat[..valeurs.len()].copy_from_slice(&valeurs);
        plat[PHASE] = self.avancer_phase(&valeurs, Instant::now());
        std::array::from_fn(|r| [plat[r * 4], plat[r * 4 + 1], plat[r * 4 + 2], plat[r * 4 + 3]])
    }

    pub fn serialiser(&self) -> String {
        REGLAGES.iter().zip(self.valeurs()).map(|(r, v)| format!("{}={}", r.id, v)).collect::<Vec<_>>().join(",")
    }

    /// Relit `serialiser`. Un champ illisible ou inconnu est ignoré ; un réglage
    /// absent garde sa valeur courante.
    pub fn charger(&self, contenu: &str) {
        for champ in contenu.trim().split(',') {
            if let Some((id, v)) = champ.split_once('=') {
                if let Ok(v) = v.trim().parse::<f32>() {
                    self.set(id.trim(), v);
                }
            }
        }
    }
}

/// Les constantes de l'INTERFACE, lues par le shader à chaque rendu.
pub(crate) fn constantes() -> [[f32; 4]; REGISTRES] {
    INTERFACE.constantes()
}

/// Les préréglages : `(id, libellé, réglages)` — partent des valeurs par défaut.
/// Leur LIBELLÉ est leur nom dans la bibliothèque (voir `win32::
/// textures_bibliotheque`) : une texture enregistrée sous ce nom le REMPLACE.
pub const PREREGLAGES: &[(&str, &str, &[(&str, f32)])] = &[
    ("marbre", "Marbre", &[("base", 1.0), ("frequence", 0.01), ("octaves", 5.0), ("veines", 1.0), ("veines_frequence", 4.0), ("distorsion", 1.0), ("distorsion_force", 1.2), ("contraste", 1.4)]),
    ("bois", "Bois", &[("base", 0.0), ("frequence", 0.02), ("octaves", 3.0), ("etirement", 4.0), ("veines", 1.0), ("veines_frequence", 12.0)]),
    ("pierre", "Pierre", &[("base", 2.0), ("frequence", 0.03), ("octaves", 3.0), ("contraste", 1.2)]),
    ("cuir", "Cuir", &[("base", 2.0), ("frequence", 0.06), ("octaves", 2.0), ("gain", 0.3), ("contraste", 1.6), ("inverser", 1.0)]),
    ("fumee", "Fumée", &[("base", 1.0), ("frequence", 0.008), ("octaves", 6.0), ("turbulence", 1.0), ("distorsion", 1.0), ("evoluer", 1.0), ("vitesse", 0.2)]),
    ("eau", "Eau", &[("base", 3.0), ("frequence", 0.05), ("distorsion", 1.0), ("distorsion_force", 1.5), ("distorsion_frequence", 0.4), ("evoluer", 1.0), ("vitesse", 0.5)]),
    ("montagnes", "Montagnes", &[("base", 1.0), ("frequence", 0.015), ("octaves", 6.0), ("cretes", 1.0), ("contraste", 1.3)]),
    ("affiche", "Affiche", &[("base", 1.0), ("frequence", 0.02), ("octaves", 3.0), ("posterisation", 1.0), ("niveaux", 4.0), ("contraste", 1.5)]),
    // ⭐ 2026-11-08 : de quoi voir les nouvelles bases et les nouveaux réglages.
    ("carbone", "Carbone", &[("base", 27.0), ("frequence", 0.03), ("fractal", 0.0), ("relief", 1.0), ("relief_force", 1.0)]),
    ("caustiques", "Caustiques", &[("base", 29.0), ("frequence", 0.012), ("fractal", 0.0), ("distorsion", 1.0), ("distorsion_force", 0.4), ("evoluer", 1.0), ("vitesse", 0.4), ("bicolore", 1.0), ("seconde_teinte", 190.0)]),
    ("mosaique", "Mosaïque", &[("base", 30.0), ("frequence", 0.01), ("fractal", 0.0), ("pixeliser", 1.0), ("pixel_taille", 14.0)]),
    ("rosace", "Rosace", &[("base", 31.0), ("frequence", 0.02), ("fractal", 0.0), ("symetrie", 1.0), ("branches", 8.0), ("relief", 1.0)]),
    ("carte", "Carte", &[("base", 33.0), ("frequence", 0.02), ("fractal", 0.0), ("bicolore", 1.0), ("seconde_teinte", 30.0)]),
    ("ciel_etoile", "Ciel étoilé", &[("base", 38.0), ("frequence", 0.02), ("fractal", 0.0), ("evoluer", 1.0), ("vitesse", 0.5)]),
];

/// Le motif d'un préréglage : les valeurs par défaut, puis les siennes.
pub fn motif_du_prereglage(reglages: &[(&str, f32)]) -> [f32; REGLAGES.len()] {
    let mut motif = defauts();
    for (id, v) in reglages {
        if let Some(i) = index(id) {
            motif[i] = borner(&REGLAGES[i], *v);
        }
    }
    motif
}

/// Le fichier de la composition de l'interface : « id=valeur » séparés par des
/// virgules — un seul écrivain.
pub const FICHIER: &str = "texture_perso.txt";
/// Celui des terminaux, même format. Absent, il reprend celui de l'interface :
/// avant 2026-10-16, les terminaux partageaient sa composition.
pub const FICHIER_TERMINAUX: &str = "texture_perso_terminaux.txt";

/// ⭐ **UNE TEXTURE DE LA BIBLIOTHÈQUE** — DEMANDE EXPLICITE (2026-10-16) : « avoir
/// la possibilité de les sauvegarder et d'en importer ». Son motif, et le RENDU
/// (force, échelle, clarté, teinte) avec lequel elle a été enregistrée. Un
/// préréglage n'a pas de rendu : le choisir garde celui en place.
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub motif: [f32; REGLAGES.len()],
    pub rendu: Option<crate::gfx::texture_interface::Rendu>,
}

/// La première ligne d'un fichier `.ndtexture` : c'est elle qui distingue une
/// texture d'un fichier quelconque à l'import.
pub const ENTETE: &str = "Texture NeuraDesk";

/// Les clés du rendu dans le fichier — distinctes de tout ID de `REGLAGES`
/// (voir le test `les_cles_du_fichier_ne_se_melangent_pas`).
const CLES_RENDU: [&str; 4] = ["force", "echelle", "clarte", "teinte"];

/// « clé=valeur », une par ligne, sous l'en-tête. Fonction PURE.
pub fn vers_texte(texture: &Texture) -> String {
    let mut texte = format!("{ENTETE}\n");
    if let Some(r) = texture.rendu {
        for (cle, v) in CLES_RENDU.iter().zip([r.force, r.echelle, r.clarte, r.teinte]) {
            texte.push_str(&format!("{cle}={v}\n"));
        }
    }
    for (r, v) in REGLAGES.iter().zip(texture.motif) {
        texte.push_str(&format!("{}={}\n", r.id, v));
    }
    texte
}

/// Relit `vers_texte`. `None` sans l'en-tête. ⚠️ **TOLÉRANT** : un fichier vient
/// d'ailleurs — une ligne illisible ou inconnue est ignorée, un réglage absent
/// prend sa valeur par défaut, et TOUT est borné ici, comme à l'application : une
/// texture relue est égale à celle qu'on applique (la page s'en sert pour dire
/// « modifiée »).
pub fn depuis_texte(texte: &str) -> Option<Texture> {
    let mut lignes = texte.trim_start_matches('\u{feff}').lines().map(str::trim).filter(|l| !l.is_empty());
    if lignes.next()? != ENTETE {
        return None;
    }
    let motif = Composition::new();
    let rendu = crate::gfx::texture_interface::ReglagesTexture::new();
    let mut a_un_rendu = false;
    for ligne in lignes {
        let Some((cle, v)) = ligne.split_once('=') else { continue };
        let Ok(v) = v.trim().parse::<f32>() else { continue };
        match cle.trim() {
            "force" => rendu.set_force(v),
            "echelle" => rendu.set_echelle(v),
            "clarte" => rendu.set_clarte(v),
            "teinte" => rendu.set_teinte(v),
            id => {
                motif.set(id, v);
                continue;
            }
        }
        a_un_rendu = true;
    }
    Some(Texture { motif: motif.valeurs(), rendu: a_un_rendu.then(|| rendu.rendu()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une composition LOCALE : aucun autre test ne la touche.
    #[test]
    fn l_etat_se_borne_et_se_relit() {
        let c = Composition::new();
        assert_eq!(c.valeur("base"), Some(1.0));
        assert!(!c.set("inconnu", 1.0));
        assert!(!c.set("octaves", f32::NAN));
        // Bornes, cases et choix.
        c.set("octaves", 99.0);
        assert_eq!(c.valeur("octaves"), Some(8.0));
        c.set("cretes", 0.7);
        assert_eq!(c.valeur("cretes"), Some(1.0));
        c.set("base", 2.4);
        assert_eq!(c.valeur("base"), Some(2.0));
        c.set("base", 9.0);
        assert_eq!(c.valeur("base"), Some(9.0));
        c.set("base", 3.0);

        // Aller-retour par le fichier.
        let texte = c.serialiser();
        c.reinitialiser();
        c.charger(&texte);
        assert_eq!((c.valeur("octaves"), c.valeur("cretes"), c.valeur("base")), (Some(8.0), Some(1.0), Some(3.0)));
        // Un fichier abîmé ne casse rien.
        c.charger("octaves=abc,,=3,inconnu=2,gain=0.3");
        assert_eq!((c.valeur("octaves"), c.valeur("gain")), (Some(8.0), Some(0.3)));
    }

    /// ⭐ L'interface et les terminaux ont CHACUN leur composition (DEMANDE
    /// EXPLICITE, 2026-10-16).
    #[test]
    fn deux_compositions_independantes() {
        let (interface, terminaux) = (Composition::new(), Composition::new());
        terminaux.set("veines", 1.0);
        assert_eq!((interface.valeur("veines"), terminaux.valeur("veines")), (Some(0.0), Some(1.0)));
        assert_ne!(FICHIER, FICHIER_TERMINAUX, "chacun son fichier");
    }

    /// Un préréglage part des valeurs par défaut, et ses valeurs sont bornées.
    #[test]
    fn le_motif_d_un_prereglage() {
        let (_, _, marbre) = PREREGLAGES.iter().find(|(id, _, _)| *id == "marbre").unwrap();
        let motif = motif_du_prereglage(marbre);
        let c = Composition::new();
        c.remplacer(motif);
        assert_eq!((c.valeur("veines"), c.valeur("cretes"), c.valeur("gain")), (Some(1.0), Some(0.0), Some(0.5)));
        assert_eq!(motif_du_prereglage(&[("octaves", 99.0), ("inconnu", 3.0)])[index("octaves").unwrap()], 8.0);
    }

    /// ⭐ **LE FICHIER D'UNE TEXTURE** — aller-retour exact, avec et sans rendu.
    #[test]
    fn une_texture_se_relit_telle_qu_elle_s_ecrit() {
        let mut motif = defauts();
        motif[index("angle").unwrap()] = 45.0;
        motif[index("frequence").unwrap()] = 0.013;
        let rendu = crate::gfx::texture_interface::Rendu { force: 0.8, echelle: 1.75, clarte: -0.2, teinte: 235.0 };
        let avec = Texture { motif, rendu: Some(rendu) };
        let texte = vers_texte(&avec);
        assert!(texte.starts_with("Texture NeuraDesk\nforce=0.8\n"), "{texte}");
        assert_eq!(depuis_texte(&texte), Some(avec));
        let sans = Texture { motif, rendu: None };
        assert_eq!(depuis_texte(&vers_texte(&sans)), Some(sans), "un préréglage n'a pas de rendu");
    }

    /// Un fichier d'ailleurs : sans en-tête, refusé ; abîmé, borné et complété.
    #[test]
    fn un_fichier_abime_ne_casse_rien() {
        assert_eq!(depuis_texte("octaves=3"), None, "sans en-tête : ce n'est pas une texture");
        assert_eq!(depuis_texte(""), None);
        let t = depuis_texte("\u{feff}\r\nTexture NeuraDesk\r\noctaves=99\r\ngain=abc\r\ninconnu=2\r\nn'importe quoi\r\nforce=7\r\n").unwrap();
        assert_eq!(t.motif[index("octaves").unwrap()], 8.0, "borné");
        assert_eq!(t.motif[index("gain").unwrap()], 0.5, "illisible : la valeur par défaut");
        let r = t.rendu.expect("une clé du rendu suffit");
        assert_eq!((r.force, r.echelle, r.clarte, r.teinte), (1.0, 1.0, 0.0, 0.0), "borné, le reste par défaut");
    }

    /// Les natures de l'interface SONT des bruits de base : le shader les y
    /// cherche à `n + DECALAGE_NATURES` (voir `TextureFond`).
    #[test]
    fn les_natures_sont_des_bruits_de_base() {
        use crate::gfx::texture_interface::NATURES;
        for (n, (id, libelle)) in NATURES.iter().enumerate() {
            if *id == "aucune" || *id == "perso" {
                continue;
            }
            assert_eq!(BASES[n + DECALAGE_NATURES], *libelle, "nature {n}");
        }
        assert_eq!(BASES[..4], ["Valeur", "Perlin", "Cellules", "Ondes"], "les index enregistrés ne bougent pas");
        assert_eq!(BASES.len(), 43, "quatre, sept natures, douze nouveaux, dix matières, dix motifs");
        assert_eq!(BASES[32], "Truchet", "les index enregistrés ne bougent pas");
        assert_eq!(BASES[22], "Écailles", "les index enregistrés ne bougent pas");
        let c = Composition::new();
        c.set("base", 99.0);
        assert_eq!(c.valeur("base"), Some(42.0), "borné au dernier");
        assert_eq!(c.valeur("flou"), Some(0.0), "pas de flou par défaut");
    }

    #[test]
    fn les_cles_du_fichier_ne_se_melangent_pas() {
        for cle in CLES_RENDU {
            assert!(index(cle).is_none(), "« {cle} » est aussi un réglage du motif");
        }
    }

    #[test]
    fn les_reglages_et_prereglages_sont_coherents() {
        let mut ids: Vec<_> = REGLAGES.iter().map(|r| r.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), REGLAGES.len(), "IDs uniques");
        for r in REGLAGES.iter() {
            assert!(r.min <= r.defaut && r.defaut <= r.max, "{} : défaut hors bornes", r.id);
            assert!(r.pas > 0.0, "{}", r.id);
            if let Some(d) = r.depend {
                let case = REGLAGES.iter().find(|c| c.id == d).unwrap_or_else(|| panic!("{} dépend de {d}, inconnu", r.id));
                assert_eq!(case.sorte, Sorte::Case, "{} dépend d'une case", r.id);
            }
            // Un identifiant HLSL (`pp_<id>`, voir le shader).
            assert!(r.id.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{}", r.id);
        }
        for (p, _, reglages) in PREREGLAGES {
            for (id, v) in reglages.iter() {
                let r = REGLAGES.iter().find(|r| r.id == *id).unwrap_or_else(|| panic!("{p} : {id} inconnu"));
                assert_eq!(borner(r, *v), *v, "{p} : {id}={v} hors bornes");
            }
        }
    }

    /// La phase avance de `dt × vitesse`, et seulement si « évoluer » est coché.
    #[test]
    fn la_phase_n_avance_que_si_l_animation_est_cochee() {
        let c = Composition::new();
        let mut v = defauts();
        let t0 = Instant::now();
        let fige = c.avancer_phase(&v, t0);
        assert_eq!(c.avancer_phase(&v, t0 + std::time::Duration::from_secs(2)), fige, "décochée : figée");
        v[index("evoluer").unwrap()] = 1.0;
        v[index("vitesse").unwrap()] = 0.5;
        let avance = c.avancer_phase(&v, t0 + std::time::Duration::from_secs(4));
        assert!((avance - fige - 1.0).abs() < 1e-4, "2 s × 0,5 : {fige} → {avance}");
    }
}
