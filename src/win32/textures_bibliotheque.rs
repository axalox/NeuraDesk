//! **LA BIBLIOTHÈQUE DES TEXTURES** — DEMANDE EXPLICITE (2026-10-16) : « dans
//! l'éditeur de textures je voudrais pouvoir éditer les textures par défaut […] et
//! avoir la possibilité de les sauvegarder et d'en importer ».
//!
//! Une texture est un motif de `gfx::texture_perso` et son rendu, écrite par
//! `texture_perso::vers_texte` dans `%APPDATA%\NeuraDesk\textures\<nom>.ndtexture`.
//! Le dossier est aussi un point d'échange : un `.ndtexture` exporté s'importe tel
//! quel, ou s'y dépose à la main.
//!
//! ⭐ **LES PRÉRÉGLAGES SONT DES TEXTURES COMME LES AUTRES** : ils vivent dans le
//! code (`texture_perso::PREREGLAGES`), et un fichier À LEUR NOM (sans tenir compte
//! de la casse) les REMPLACE — c'est ainsi qu'on édite une texture par défaut. Le
//! supprimer rend l'original : un préréglage ne disparaît jamais de la liste.
//!
//! ⚠️ **LA BIBLIOTHÈQUE EST EN CACHE** (`FICHIERS`), et lue en entier : l'état de
//! l'éditeur est recalculé à CHAQUE frame et compare la texture en cours à celle
//! de la bibliothèque (« modifiée ») — lire le dossier à 60 Hz est le défaut « un
//! chemin résolu par frame » déjà payé une fois dans ce dépôt. Le cache est refait
//! par chaque écriture, suppression ou import.
//!
//! ⚠️ Les noms passent par `palette_gpl::nom_de_fichier_valide` : mêmes règles
//! (ce que Windows refuse, un point final qui serait retiré en silence).

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::HWND;

use crate::gfx::texture_perso::{self as perso, Texture};
use crate::win32::palette_gpl::nom_de_fichier_valide;

pub const EXTENSION: &str = "ndtexture";

thread_local! {
    /// Les textures du dossier, `(nom, texture)`. `None` : jamais lu.
    static FICHIERS: RefCell<Option<Vec<(String, Texture)>>> = const { RefCell::new(None) };
}

/// Rend au device les textures que ce cache détient — **avant la destruction du
/// device** (voir `app::GpuCacheGuard`) : sinon elle ne mourait qu'au démontage
/// du processus, et DXGI y faisait sortir NeuraDesk en code 122.
pub fn liberer_gpu() {
    FICHIERS.with(|f| {
        if let Ok(mut f) = f.try_borrow_mut() {
            *f = None;
        }
    });
}

/// Une texture de la liste de l'éditeur.
#[derive(Clone, Debug, PartialEq)]
pub struct Entree {
    pub nom: String,
    /// Un préréglage du code (voir la doc de module).
    pub prereglage: bool,
    /// Un fichier existe : la texture de l'utilisateur, ou le préréglage tel
    /// qu'il l'a modifié.
    pub enregistree: bool,
    pub texture: Texture,
}

fn chemin(nom: &str) -> Option<PathBuf> {
    let nom = nom_de_fichier_valide(nom)?;
    let mut path = crate::config::textures_dir()?;
    path.push(format!("{nom}.{EXTENSION}"));
    Some(path)
}

/// Le nom de texture d'un nom de fichier — `None` si ce n'est pas un `.ndtexture`.
fn nom_depuis_fichier(fichier: &str) -> Option<String> {
    let (base, extension) = fichier.rsplit_once('.')?;
    (extension.eq_ignore_ascii_case(EXTENSION) && !base.is_empty()).then(|| base.to_string())
}

fn lire_le_dossier() -> Vec<(String, Texture)> {
    let Some(entrees) = crate::config::textures_dir().and_then(|d| std::fs::read_dir(d).ok()) else { return Vec::new() };
    entrees
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let nom = nom_depuis_fichier(e.file_name().to_str()?)?;
            // Un fichier illisible est écarté, et dit : il n'apparaîtrait nulle part.
            let octets = std::fs::read(e.path()).ok()?;
            match perso::depuis_texte(&String::from_utf8_lossy(&octets)) {
                Some(texture) => Some((nom, texture)),
                None => {
                    crate::diag!("[textures] {} n'est pas une texture, écarté", e.path().display());
                    None
                }
            }
        })
        .collect()
}

fn oublier() {
    FICHIERS.with(|f| *f.borrow_mut() = None);
}

/// **LA LISTE DE L'ÉDITEUR** : les préréglages dans leur ordre — remplacés par le
/// fichier à leur nom s'il existe —, puis les textures de l'utilisateur, triées
/// sans tenir compte de la casse.
pub fn entrees() -> Vec<Entree> {
    FICHIERS.with(|f| fusionner(f.borrow_mut().get_or_insert_with(lire_le_dossier)))
}

/// Le corps de `entrees`, PUR.
fn fusionner(fichiers: &[(String, Texture)]) -> Vec<Entree> {
    let fichier = |nom: &str| fichiers.iter().find(|(n, _)| n.eq_ignore_ascii_case(nom));
    let mut liste: Vec<Entree> = perso::PREREGLAGES
        .iter()
        .map(|(_, libelle, reglages)| match fichier(libelle) {
            Some((_, texture)) => Entree { nom: libelle.to_string(), prereglage: true, enregistree: true, texture: texture.clone() },
            None => Entree {
                nom: libelle.to_string(),
                prereglage: true,
                enregistree: false,
                texture: Texture { motif: perso::motif_du_prereglage(reglages), rendu: None },
            },
        })
        .collect();
    let mut miennes: Vec<Entree> = fichiers
        .iter()
        .filter(|(nom, _)| !perso::PREREGLAGES.iter().any(|(_, l, _)| l.eq_ignore_ascii_case(nom)))
        .map(|(nom, texture)| Entree { nom: nom.clone(), prereglage: false, enregistree: true, texture: texture.clone() })
        .collect();
    miennes.sort_by_key(|e| e.nom.to_lowercase());
    liste.extend(miennes);
    liste
}

/// La texture `nom`, sans tenir compte de la casse.
pub fn trouver(nom: &str) -> Option<Entree> {
    entrees().into_iter().find(|e| e.nom.eq_ignore_ascii_case(nom.trim()))
}

/// Le nom sous lequel `nom` s'enregistre : celui d'une texture qui existe déjà
/// (même casse que la liste — « marbre » REMPLACE « Marbre »), ou `nom` lui-même.
fn nom_retenu(nom: &str, existants: &[Entree]) -> String {
    let nom = nom.trim();
    existants.iter().find(|e| e.nom.eq_ignore_ascii_case(nom)).map_or_else(|| nom.to_string(), |e| e.nom.clone())
}

/// **ENREGISTRE** `texture` sous `nom` (écrase une texture du même nom, ou remplace
/// le préréglage de ce nom). Rend le nom retenu.
pub fn enregistrer(nom: &str, texture: &Texture) -> Option<String> {
    let nom = nom_retenu(nom, &entrees());
    let Some(path) = chemin(&nom) else {
        crate::diag!("[textures] nom refusé : {nom:?}");
        return None;
    };
    let fait = crate::config::write_atomique(&path, perso::vers_texte(texture));
    oublier();
    if !fait {
        crate::diag!("[textures] {} : écriture impossible", path.display());
    }
    fait.then_some(nom)
}

/// Retire le fichier de `nom` : une texture de l'utilisateur disparaît, un
/// préréglage revient à l'original. `false` s'il n'y avait pas de fichier.
pub fn supprimer(nom: &str) -> bool {
    let nom = nom_retenu(nom, &entrees());
    let fait = chemin(&nom).is_some_and(|p| std::fs::remove_file(p).is_ok());
    oublier();
    fait
}

/// **EXPORTE** `texture` vers un fichier choisi par « Enregistrer sous ».
pub fn exporter(proprietaire: HWND, nom: &str, texture: &Texture) {
    let propose = nom_de_fichier_valide(nom).unwrap_or_else(|| "Texture".to_string());
    let Some(cible) = crate::win32::file_dialog::enregistrer_sous(
        proprietaire,
        "Exporter la texture",
        &format!("{propose}.{EXTENSION}"),
        EXTENSION,
        "Texture NeuraDesk (*.ndtexture)",
    ) else {
        return; // Annuler n'est pas une erreur.
    };
    if let Err(erreur) = std::fs::write(&cible, perso::vers_texte(texture)) {
        crate::diag!("[textures] export impossible : {erreur}");
    }
}

/// **IMPORTE** un `.ndtexture` choisi par « Ouvrir » : il rejoint la bibliothèque
/// sous un nom LIBRE — un import ne remplace jamais une texture, ni un préréglage.
/// Rend le nom retenu.
pub fn importer(proprietaire: HWND) -> Option<String> {
    let source = crate::win32::file_dialog::ouvrir(proprietaire, "Importer une texture", EXTENSION, "Texture NeuraDesk (*.ndtexture)")?;
    let texte = match std::fs::read(&source) {
        Ok(octets) => String::from_utf8_lossy(&octets).into_owned(),
        Err(erreur) => {
            crate::diag!("[textures] import impossible : {erreur}");
            return None;
        }
    };
    let Some(texture) = perso::depuis_texte(&texte) else {
        crate::diag!("[textures] {} n'est pas une texture", source.display());
        return None;
    };
    let pris: Vec<String> = entrees().into_iter().map(|e| e.nom).collect();
    let nom = crate::win32::lueurs_effets::nom_libre(&base_du_fichier(&source), &pris);
    let path = chemin(&nom)?;
    let fait = crate::config::write_atomique(&path, perso::vers_texte(&texture));
    oublier();
    fait.then_some(nom)
}

fn base_du_fichier(path: &Path) -> String {
    let base = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Texture importée");
    nom_de_fichier_valide(base).unwrap_or_else(|| "Texture importée".to_string())
}

// ⚠️ **AUCUN TEST NE TOUCHE LE DOSSIER** — le dépôt interdit à ses tests d'écrire
// dans `%APPDATA%` (voir `palette_gpl`, même règle). Ce qui est testable est pur.
#[cfg(test)]
mod tests {
    use super::*;

    fn texture(angle: f32) -> Texture {
        let mut motif = perso::motif_du_prereglage(&[]);
        motif[perso::REGLAGES.iter().position(|r| r.id == "angle").unwrap()] = angle;
        Texture { motif, rendu: None }
    }

    #[test]
    fn seul_un_point_ndtexture_est_une_texture() {
        assert_eq!(nom_depuis_fichier("Aurore.ndtexture").as_deref(), Some("Aurore"));
        assert_eq!(nom_depuis_fichier("Aurore.NDTEXTURE").as_deref(), Some("Aurore"));
        assert_eq!(nom_depuis_fichier("Aurore.txt"), None);
        assert_eq!(nom_depuis_fichier(".ndtexture"), None);
    }

    /// ⭐ Les préréglages d'abord, dans leur ordre ; un fichier à leur nom les
    /// REMPLACE (sans tenir compte de la casse) ; puis les miennes, triées.
    #[test]
    fn un_fichier_au_nom_d_un_prereglage_le_remplace() {
        let fichiers = vec![("zèbre".to_string(), texture(10.0)), ("marbre".to_string(), texture(20.0)), ("Ardoise".to_string(), texture(30.0))];
        let liste = fusionner(&fichiers);
        let noms: Vec<&str> = liste.iter().map(|e| e.nom.as_str()).collect();
        let n = perso::PREREGLAGES.len();
        assert_eq!(noms[..n], perso::PREREGLAGES.iter().map(|(_, l, _)| *l).collect::<Vec<_>>()[..]);
        assert_eq!(noms[n..], ["Ardoise", "zèbre"], "les miennes après, triées ; « marbre » n'y est pas deux fois");
        let marbre = &liste[0];
        assert!(marbre.prereglage && marbre.enregistree, "le préréglage modifié");
        assert_eq!(marbre.texture, texture(20.0));
        let bois = &liste[1];
        assert!(bois.prereglage && !bois.enregistree, "l'original");
        assert_eq!(bois.texture.rendu, None, "un préréglage garde le rendu en place");
        assert!(!liste[n].prereglage && liste[n].enregistree);
    }

    /// « marbre » s'enregistre sur « Marbre » : un seul fichier par nom, que
    /// Windows ne distingue pas par la casse.
    #[test]
    fn un_nom_existant_garde_sa_casse() {
        let liste = fusionner(&[("Ardoise".to_string(), texture(1.0))]);
        assert_eq!(nom_retenu(" marbre ", &liste), "Marbre");
        assert_eq!(nom_retenu("ARDOISE", &liste), "Ardoise");
        assert_eq!(nom_retenu("Nouvelle", &liste), "Nouvelle");
    }
}
