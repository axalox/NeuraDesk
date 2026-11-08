//! **LE SOUS-DOCK** — les enfants d'un dossier de la barre du bas.
//!
//! LOT 1 de `docs/plans/barre-du-bas.md` : survoler une vignette de dossier ouvre
//! ses enfants en éventail, ancrés à la vignette, comme le fait l'applet
//! `Folders` de Cairo-Dock. Ce module porte **le modèle** — lire un dossier et
//! rendre ses enfants — et rien d'autre : le panneau qui l'affiche est un
//! morceau distinct, qui réutilise `win32::window_previews`.
//!
//! **POURQUOI CE MODULE EST SÉPARÉ, ET PUR.** Le tri et le filtrage se vérifient
//! par un test sur un dossier temporaire ; mélangés au panneau, ils ne se
//! vérifieraient plus du tout. C'est la même discipline que `ui::zones` ou
//! `gfx::degrade`, dont la géométrie et l'état sont testables sans GPU.
//!
//! ⭐ **LISTER ET ICONIFIER SONT DEUX TRAVAUX SÉPARÉS, ET C'EST DÉLIBÉRÉ.**
//! `enfants` ne fait plus QUE lire le disque — un `read_dir`, un `file_type`, le
//! nom, l'extension : **aucun appel au shell**. C'est ce qui lui permet de rendre
//! un dossier ENTIER sans coût par entrée.
//!
//! ⛔ **L'ICÔNE, ELLE, RESTE CHÈRE** (`SHGetFileInfoW` + `IImageList::GetIcon`,
//! voir `icone_du_shell`). Elle n'est donc PLUS calculée ici : l'appelant la
//! demande **à la demande**, par petits lots, sur le modèle de
//! `cef_recherche::icone`. La calculer à la lecture obligeait à couper la liste à
//! `ENFANTS_MAX` — c'est ce qui faisait disparaître des fichiers, et c'est ce que
//! cette séparation corrige.

use std::path::{Path, PathBuf};
use std::collections::HashSet;
use std::sync::{mpsc, Mutex, OnceLock};

/// Nombre MAXIMAL d'enfants listés.
///
/// ⭐ **CE N'EST PLUS UNE PROTECTION CONTRE LE COÛT DES ICÔNES** — elles ne sont
/// plus calculées à la lecture (voir la doc de module). La valeur a donc été
/// portée de 48 à 2 000, puis à **5 000** : à 48, un dossier de photos ou de code
/// était **coupé**, et l'utilisateur voyait une liste incomplète sans comprendre
/// pourquoi.
///
/// Ce qui reste vrai, et justifie de garder une borne : 5 000 cases, c'est déjà
/// 5 000 nœuds DOM dans la page, et les tris `Date`/`Taille` font une statistique
/// par entrée. Un dossier de 100 000 fichiers ne doit pas les produire.
///
/// ⚠️ **LE PRIX D'UNE BORNE HAUTE, MESURÉ.** Les icônes se calculent sous un budget
/// de 8 ms par frame, ce qui donne **1 à 5 icônes par frame** en pratique : un
/// dossier de 5 000 entrées met donc **~30 s à se remplir complètement**. La LISTE,
/// elle, s'affiche tout de suite — et c'est elle qui manquait. Les icônes de ce
/// qu'on ne regarde pas sont calculées et envoyées quand même : c'est le choix
/// assumé de « tout, progressivement ».
///
/// ⚠️ **La troncature est DITE à l'appelant** (`Liste::tronque`) : une liste
/// coupée en silence ferait croire à un dossier incomplet.
pub const ENFANTS_MAX: usize = 5000;

/// **COMMENT RANGER LES ENFANTS D'UN DOSSIER.**
///
/// L'ordre par défaut est celui de l'Explorateur — dossiers d'abord, puis par
/// nom — et c'est le SEUL qui ne demande aucune statistique : les trois autres
/// appellent `metadata()` une fois PAR ENTRÉE (voir `demande_metadata`), ce
/// qu'un dossier réseau paie cher.
///
/// ⚠️ **DANS TOUS LES TRIS, LES DOSSIERS RESTENT DEVANT.** C'est le seul point
/// que l'œil attend de manière absolue dans un gestionnaire de fichiers, et le
/// perdre en changeant de tri surprendrait plus que ça n'aiderait.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tri {
    #[default]
    Nom,
    /// Le plus récemment modifié d'abord.
    Date,
    /// Le plus gros d'abord.
    Taille,
    /// Par extension, puis par nom — « tous les .png ensemble ».
    Type,
}

impl Tri {
    /// La clé qui voyage entre la page et Rust : ASCII, stable, et lisible dans
    /// le journal. **Ce n'est PAS le nom du variant**, à dessein — renommer un
    /// variant ne doit pas invalider un réglage déjà écrit sur le disque.
    pub fn cle(self) -> &'static str {
        match self {
            Tri::Nom => "nom",
            Tri::Date => "date",
            Tri::Taille => "taille",
            Tri::Type => "type",
        }
    }

    /// L'inverse de `cle` — `None` sur une clé inconnue, pour que la page puisse
    /// en inventer une sans qu'on range n'importe quoi.
    pub fn depuis_cle(cle: &str) -> Option<Self> {
        [Tri::Nom, Tri::Date, Tri::Taille, Tri::Type]
            .into_iter()
            .find(|t| t.cle() == cle)
    }

    /// Vrai si ce tri a besoin des STATISTIQUES de chaque entrée (date, taille).
    fn demande_metadata(self) -> bool {
        matches!(self, Tri::Date | Tri::Taille)
    }
}

/// **DE QUEL CÔTÉ DU DASH DOSSIER.** Le dash dossier montre DEUX dossiers à la
/// fois : la moitié gauche, que le SURVOL pilote, et la moitié droite, qui
/// s'ouvre sur le dossier personnel.
///
/// ⚠️ **CE TYPE EXISTE POUR QU'ON NE SE TROMPE PAS DE MOITIÉ, PAS POUR FAIRE
/// JOLI.** Chaque demande de la page porte son côté (`Demande::Entrer(Cote, …)`),
/// et chaque état vivant est indexé par lui. Sans ce type, les deux moitiés
/// seraient deux paires de `thread_local!` écrites côte à côte — et c'est le
/// défaut que ce dépôt a déjà payé plusieurs fois : **deux moitiés écrites à deux
/// endroits divergent**, et rien ne le signale.
///
/// ⚠️ **IL VIT ICI, AVEC `Tri`, ET NON DANS LE PANNEAU** : `config` doit
/// connaître la clé de chaque côté pour persister deux tris, et `config` dépend
/// déjà de ce module (`load_sous_dock_tris` rend des `Tri`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cote {
    /// La moitié que le dock ouvre sur le dossier SURVOLÉ dans la barre du bas.
    Gauche,
    /// Celle qui s'ouvre sur le dossier personnel.
    Droite,
}

impl Cote {
    /// Les deux côtés, DANS L'ORDRE où ils sont indexés partout.
    ///
    /// ⛔ **C'EST LA SEULE ÉNUMÉRATION DES MOITIÉS.** Le dépôt a payé deux fois
    /// le même défaut à une barre d'intervalle près pour avoir énuméré des barres
    /// à la main (« énumérer les barres à la main ») : tout ce qui doit couvrir
    /// les deux moitiés passe par ce tableau, jamais par deux littéraux.
    pub const TOUTES: [Cote; 2] = [Cote::Gauche, Cote::Droite];

    /// L'indice dans un `[_; 2]` — c'est l'ordre de `TOUTES`.
    pub fn index(self) -> usize {
        match self {
            Cote::Gauche => 0,
            Cote::Droite => 1,
        }
    }

    /// La clé qui voyage entre la page et Rust. **Ce n'est PAS le nom du
    /// variant** — même règle que `Tri::cle` : renommer un variant ne doit pas
    /// invalider un réglage déjà écrit sur le disque.
    pub fn cle(self) -> &'static str {
        match self {
            Cote::Gauche => "g",
            Cote::Droite => "d",
        }
    }

    /// L'inverse de `cle` — `None` sur une clé inconnue. La page ne doit pas
    /// pouvoir viser une moitié qui n'existe pas : un côté deviné enverrait
    /// l'ordre à l'autre moitié, ce qui ne se verrait qu'à l'usage.
    pub fn depuis_cle(cle: &str) -> Option<Self> {
        Cote::TOUTES.into_iter().find(|c| c.cle() == cle)
    }
}

/// Un enfant d'un dossier — de quoi dessiner une case du sous-dock.
#[derive(Clone, Debug)]
pub struct Enfant {
    pub chemin: PathBuf,
    pub nom: String,
    /// Un dossier s'OUVRE (on entre dedans), un fichier se LANCE. C'est la seule
    /// distinction dont le geste ait besoin.
    pub est_dossier: bool,
    /// L'extension en minuscules, SANS le point — vide s'il n'y en a pas. Se lit
    /// sur le nom, donc sans le moindre appel système.
    pub extension: String,
    /// Date de dernière modification et taille.
    ///
    /// `None` dans DEUX cas, et ils ne se distinguent pas : le tri en cours n'en
    /// a pas besoin (voir `Tri::demande_metadata`), ou la statistique a échoué
    /// (lien mort, accès refusé). Dans les deux, `None` se range EN DERNIER dans
    /// un ordre décroissant — ce qui est le bon comportement pour « inconnu ».
    pub modifie: Option<std::time::SystemTime>,
    pub taille: Option<u64>,
}

/// Le contenu d'un dossier, prêt à dessiner.
#[derive(Clone, Debug, Default)]
pub struct Liste {
    pub enfants: Vec<Enfant>,
    /// Vrai si des enfants ont été ÉCARTÉS parce qu'ils dépassaient
    /// `ENFANTS_MAX` — à dire à l'écran, jamais à taire.
    pub tronque: bool,
}

impl Liste {
    #[cfg(test)]
    pub fn est_vide(&self) -> bool {
        self.enfants.is_empty()
    }
}

/// **LES ENFANTS D'UN DOSSIER, TRIÉS.** Les dossiers d'abord, puis les fichiers,
/// et par nom à l'intérieur de chaque groupe — ordre que l'Explorateur de Windows
/// suit par défaut, donc celui que l'œil attend.
///
/// Les entrées CACHÉES sont écartées : le sous-dock n'est pas un gestionnaire de
/// fichiers, il montre ce qu'on vient chercher.
///
/// ⭐ **AUCUN APPEL AU SHELL ICI, ET C'EST LE POINT.** Cette fonction ne rend que
/// ce que le disque dit : nom, chemin, nature, extension. Les icônes se demandent
/// plus tard, une par une, par `icone_du_shell` — voir la doc de module, qui
/// porte le pourquoi.
pub fn enfants(chemin: &Path, tri: Tri) -> Liste {
    let Ok(lecture) = std::fs::read_dir(chemin) else {
        return Liste::default();
    };
    let mut trouves: Vec<Enfant> = Vec::new();
    for entree in lecture.flatten() {
        let nom = entree.file_name().to_string_lossy().into_owned();
        // Les fichiers cachés sont écartés par leur NOM : `file_attributes` est
        // une autre API, et l'Explorateur lui-même se contente de ce test pour
        // l'attribut « caché » du shell.
        if nom.starts_with('.') {
            continue;
        }
        let chemin = entree.path();
        // `file_type()` plutôt que `metadata()` : pas de statistique
        // supplémentaire, et un lien mort n'empêche pas de lire la suite.
        let Ok(genre) = entree.file_type() else { continue };
        let est_dossier = genre.is_dir();
        // ⚠️ **LA STATISTIQUE N'EST DEMANDÉE QUE SI LE TRI EN A BESOIN.** C'est
        // un appel système PAR ENTRÉE, et le tri par défaut (celui de
        // l'Explorateur) n'en a aucun besoin : le payer toujours ralentirait le
        // cas le plus fréquent pour trois tris qu'on utilise rarement.
        let (modifie, taille) = if tri.demande_metadata() {
            match entree.metadata() {
                Ok(m) => (m.modified().ok(), Some(m.len())),
                Err(_) => (None, None),
            }
        } else {
            (None, None)
        };
        let extension = Path::new(&nom)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        trouves.push(Enfant { chemin, nom, est_dossier, extension, modifie, taille });
    }

    trouves.sort_by(|a, b| comparer(tri, a, b));
    let tronque = tronquer(&mut trouves, ENFANTS_MAX);

    Liste { enfants: trouves, tronque }
}

/// **COUPE LA LISTE À `max` ENFANTS, ET DIT SI ELLE A COUPÉ.**
///
/// ⚠️ **EXTRAITE POUR ÊTRE VÉRIFIÉE SANS DISQUE** — même raison que `comparer`,
/// dont la doc porte le principe. Passer par `enfants` pour éprouver la
/// troncature demanderait de créer `ENFANTS_MAX + 3` fichiers, soit **5 003**
/// entrées dans `%TEMP%` : une seconde de test pour vérifier une comparaison.
/// Ici on lui donne cinq `Enfant` fabriqués et un `max` de trois.
fn tronquer(trouves: &mut Vec<Enfant>, max: usize) -> bool {
    let tronque = trouves.len() > max;
    trouves.truncate(max);
    tronque
}

/// **L'ORDRE DE DEUX ENFANTS — extrait pour être TESTABLE SANS DISQUE.**
///
/// C'est la seule raison d'être de cette fonction : un comparateur noyé dans
/// `enfants` ne se vérifie qu'en fabriquant des fichiers temporaires, avec des
/// dates de modification qu'on ne contrôle pas (deux fichiers écrits à la suite
/// ont la même à la milliseconde près). Ici on compare des `Enfant` fabriqués à
/// la main, et les quatre tris deviennent vérifiables.
///
/// ⚠️ **LE DÉPARTAGE FINAL EST TOUJOURS LE NOM** (`cle_de_tri`, insensible à la
/// casse ET aux accents) : sans lui, deux fichiers de même taille — ou deux
/// dossiers, dont la date et la taille ne sont pas ce qu'on compare — se
/// rangeraient dans l'ordre du système de fichiers, qui n'est pas stable d'une
/// lecture à l'autre. Une liste qui change d'ordre toute seule entre deux
/// ouvertures est le genre de défaut qu'on met des semaines à croire.
fn comparer(tri: Tri, a: &Enfant, b: &Enfant) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    // Dans TOUS les tris, les dossiers passent devant (voir `Tri`, doc de type).
    let dossiers = b.est_dossier.cmp(&a.est_dossier);
    if dossiers != Ordering::Equal {
        return dossiers;
    }
    let par_nom = || cle_de_tri(&a.nom).cmp(&cle_de_tri(&b.nom));
    match tri {
        Tri::Nom => par_nom(),
        // Décroissant : `b` d'abord. Les `None` (inconnu) finissent donc EN
        // DERNIER, ce qui est le seul rang honnête pour une valeur qu'on n'a pas.
        Tri::Date => b.modifie.cmp(&a.modifie).then_with(par_nom),
        Tri::Taille => b.taille.cmp(&a.taille).then_with(par_nom),
        // `a` d'abord : ordre CROISSANT des extensions, comme un regroupement.
        Tri::Type => a.extension.cmp(&b.extension).then_with(par_nom),
    }
}

/// **L'ICÔNE DU SHELL D'UN CHEMIN** — la plus grande disponible.
///
/// ⚠️ **RÉUTILISE `cef_dash`, ELLE NE LA RECOPIE PAS** : `SHGetFileInfoW` +
/// `IImageList::GetIcon` avec ses drapeaux vit déjà là-bas pour le Dash, et deux
/// copies de ce détail Win32 divergeraient — la leçon des « deux moitiés écrites
/// à deux endroits » vaut aussi pour une extraction d'icône.
///
/// JUMBO d'abord (`256 px`) parce qu'une vignette de dossier peut être grande ;
/// LARGE (`32 px`) en repli, plutôt qu'une case vide.
///
/// ⚠️ **APPELÉE À LA DEMANDE, PLUS À LA LECTURE DU DOSSIER** — c'est
/// `cef_sous_dock::envoyer_des_icones` qui la demande, par lots de 24 par frame,
/// au fur et à mesure que la file se vide (voir la doc de module).
pub(crate) fn icone_du_shell(chemin: &Path) -> Option<(u32, u32, Vec<u8>)> {
    crate::win32::cef_dash::jumbo_icon_rgba(chemin)
        .or_else(|| crate::win32::cef_dash::large_icon_rgba(chemin))
}

/// **LES EXTENSIONS QUI REÇOIVENT UNE VIGNETTE** au lieu de leur icône — DEMANDE
/// EXPLICITE (2026-10-11) : « avoir les vignettes des fichiers image ».
///
/// ⚠️ **UNE LISTE, PAS « TOUT CE QUI A UNE VIGNETTE »** : demander une vignette au
/// shell pour un `.exe` ou un `.zip` coûte la recherche d'un fournisseur pour
/// rien, et la demande portait sur les IMAGES. Comparée en minuscules.
const EXTENSIONS_IMAGE: &[&str] =
    &["jpg", "jpeg", "jfif", "png", "gif", "bmp", "webp", "tif", "tiff", "heic", "heif", "avif", "ico", "svg"];

/// Vrai si le chemin désigne un fichier image (voir `EXTENSIONS_IMAGE`).
pub(crate) fn est_une_image(chemin: &Path) -> bool {
    chemin
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS_IMAGE.contains(&e.to_ascii_lowercase().as_str()))
}

/// **LES VIDÉOS REÇOIVENT AUSSI UNE VIGNETTE** — DEMANDE EXPLICITE (2026-10-12) :
/// « dans les dossiers du dash dossier j'aimerais avoir des vignettes pour les
/// vidéos ». Même chemin que les images (`vignette_du_shell`) : c'est le shell qui
/// extrait l'image de la vidéo (Media Foundation), comme dans l'Explorateur — un
/// format sans extracteur installé retombe sur son icône.
const EXTENSIONS_VIDEO: &[&str] =
    &["mp4", "m4v", "mkv", "mov", "avi", "wmv", "webm", "mpg", "mpeg", "ts", "m2ts", "3gp", "flv"];

/// Vrai si cette EXTENSION est une vidéo (voir `EXTENSIONS_VIDEO`). La règle est
/// ici, `est_une_video` n'en est que la lecture du chemin : les deux genres de la
/// flèche (`GenreMedia`) comparent donc la MÊME chose, l'extension déjà lowercée
/// de l'entrée, et non l'une le chemin, l'autre l'extension.
fn extension_est_video(extension: &str) -> bool {
    EXTENSIONS_VIDEO.contains(&extension.to_ascii_lowercase().as_str())
}

/// Vrai si le chemin désigne une vidéo (voir `EXTENSIONS_VIDEO`).
pub(crate) fn est_une_video(chemin: &Path) -> bool {
    chemin.extension().and_then(|e| e.to_str()).is_some_and(extension_est_video)
}

/// **CE QUE LA FLÈCHE DES FENÊTRES DE VISUALISATION PARCOURT** — DEMANDE
/// EXPLICITE (2026-11-03) : « des flèches en surbrillance pour passer à la vidéo
/// ou la photo suivante ». Un genre, et non « tous les fichiers » : une flèche qui
/// tombe sur un `.txt` montrerait du texte là où l'on défile des images.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenreMedia {
    Image,
    Video,
}

impl GenreMedia {
    /// Vrai si CET ENFANT est du genre.
    ///
    /// ⛔ **LES IMAGES SONT LA LISTE DE CE QUI SE MONTRE, PAS CELLE DES VIGNETTES.**
    /// `EXTENSIONS_IMAGE` en compte quatorze — le shell sait tirer une vignette d'un
    /// `.heic` — mais la visionneuse n'en décode que six
    /// (`texte_fichier::est_une_image`) et `Demande::Afficher` REFUSE les autres
    /// (voir `app::run`, où la décision est écrite). Parcourir la liste des
    /// vignettes mènerait la flèche sur un fichier que la fenêtre refuse, et
    /// chaque clic rendrait un message d'erreur.
    fn le_contient(self, enfant: &Enfant) -> bool {
        if enfant.est_dossier {
            return false;
        }
        match self {
            GenreMedia::Image => crate::win32::texte_fichier::est_une_image(&enfant.extension),
            GenreMedia::Video => extension_est_video(&enfant.extension),
        }
    }
}

/// Le genre d'un fichier que la visionneuse ou le lecteur montre — `None` pour ce
/// qu'aucune flèche ne parcourt : un texte, un `.pdf`, un `.psd`. Une fenêtre
/// ouverte là-dessus n'a donc pas de flèche du tout, plutôt qu'une flèche qui
/// mène vers un autre format.
pub(crate) fn genre_de_media(chemin: &Path) -> Option<GenreMedia> {
    let extension = chemin
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if crate::win32::texte_fichier::est_une_image(&extension) {
        Some(GenreMedia::Image)
    } else if est_une_video(chemin) {
        Some(GenreMedia::Video)
    } else {
        None
    }
}

/// **DEUX CHEMINS DÉSIGNENT-ILS LE MÊME FICHIER ?** Sur le nom et le dossier
/// parent, insensibles à la casse, et SANS le moindre appel système. ⚠️ C'est
/// pourquoi `voisins_dans_serie` cherche d'abord le nom **exact** : dans un dossier
/// marqué sensible à la casse, plier ici ferait deux fichiers distincts un seul.
///
/// ⛔ **PAS `same_file`** : il faudrait ouvrir chaque entrée du dossier pour
/// trouver où l'on est, jusqu'à `ENFANTS_MAX` fois. Et le chemin du fichier
/// ouvert ne vient pas toujours de `read_dir` — le harnais le donne par une
/// variable d'environnement, donc le parent peut différer par la casse du
/// lecteur.
pub(crate) fn est_le_meme_fichier(a: &Path, b: &Path) -> bool {
    // ⛔ **LA FORME DU CHEMIN N'EST PAS SON IDENTITÉ.** Le chemin du fichier ouvert
    // ne vient pas toujours de `read_dir` : le harnais le donne par variable
    // d'environnement, et une lecture `canonicalize()` rend `\\?\C:\…` là où la
    // listing rend `C:\…`. Comparer les deux tels quels éteint la flèche SANS
    // raison — et une absence de trace a été lue six fois, dans ce dépôt, comme
    // « la fonctionnalité est cassée ». Une barre finale, même raison.
    //
    // ⚠️ **CE QUE CELA NE COUVRE PAS, et ce que ça ne doit pas couvrir.** Deux noms
    // d'un même partage — `Z:\…` (lecteur mappé) contre `\\serveur\partage\…` —
    // restent distincts, comme `\\?\Volume{guid}\…` : les départager demanderait un
    // appel système par entrée. Un dossier dont le fichier ouvert et la listing
    // viennent de deux de ces formes n'a donc pas de flèche — **jamais une flèche
    // qui mène ailleurs**, qui serait le pire des cas : un `›` qui change de
    // dossier sous les yeux de l'utilisateur.
    fn forme(p: &Path) -> String {
        let brut = p.to_string_lossy().to_ascii_lowercase();
        // ⚠️ **LITTÉRALE BRUTE, et la nuance vaut d'être écrite.** `"\\?\\"` en
        // écriture classique ne rend que `\?\` — TROIS caractères — alors qu'un
        // chemin `\\?\D:\…` en porte QUATRE : le préfixe ne se retirait jamais, et
        // la flèche s'éteignait pour rien. C'est `r"\\?\"` qui rend les quatre.
        let (decore, canonique) = match brut.strip_prefix(r"\\?\") {
            Some(reste) => (reste, true),
            None => (brut.as_str(), false),
        };
        // ⚠️ **UN PARTAGE A DEUX FORMES, et c'est le cas réseau : LA FLÈCHE S'ÉTEINT
        // DES DEUX CÔTÉS SANS RIEN DIRE.** `canonicalize()` rend
        // `\\?\UNC\server\partage\a.png` là où `read_dir` rend `\\server\partage\a.png`.
        // Le repli ci-dessous rend la première identique à la seconde. Un chemin de
        // VOLUME (`\\?\Volume{guid}\…`) reste distinct : le rattacher à une lettre de
        // lecteur demanderait un appel système par entrée, ce que cette fonction
        // refuse à juste titre.
        let decore = if canonique {
            decore.strip_prefix("unc\\").map_or_else(|| decore.to_string(), |reste| format!("\\\\{reste}"))
        } else {
            decore.to_string()
        };
        // Le séparateur n'est pas davantage une identité : une lecture `canonicalize()`
        // rend `\\?\D:\x\b.png` là où la listing rend `D:/x/b.png`. Pareil fichier.
        decore.trim_end_matches(['/', '\\']).replace('\\', "/")
    }
    forme(a) == forme(b)
}

/// **LA SÉRIE QUE LA FLÈCHE PARCOURT, dans une listing déjà triée** : les
/// fichiers du même genre, dans l'ordre reçu. Zéro appel système — elle ne fait
/// que le filtre de genre, ce qui la rend testable sans disque.
///
/// ⛔ **L'ORDRE EST CELUI QU'ON REÇOIT, et il vient de `comparer`.** La flèche ne
/// trie rien elle-même : deux ordres dans la même fenêtre — la grille d'un côté,
/// la flèche de l'autre — est le défaut que ce dépôt a le plus payé.
pub(crate) fn serie_dans(enfants: &[Enfant], genre: GenreMedia) -> Vec<PathBuf> {
    enfants.iter().filter(|e| genre.le_contient(e)).map(|e| e.chemin.clone()).collect()
}

/// **LA SÉRIE ENTIÈRE QUE LA FLÈCHE PARCOURT** : les fichiers du même genre du
/// dossier, dans l'ordre reçu. `voisins_de_media` n'est que cette série regardée
/// depuis son rang.
///
/// ⭐ **LA SÉRIE EST SÉPARÉE DES VOISINS POUR POUVOIR ÊTRE RETENUE.** Une
/// navigation qui demande les voisins relit le dossier à CHAQUE fois, et deux
/// endroits qui relisent font deux lectures par appui : la visionneuse, qui a
/// besoin des voisins pour savoir quelle flèche éteindre, ET pour changer de
/// fichier. `enfants()` lit jusqu'à `ENFANTS_MAX` entrées, et `Tri::Date` ou
/// `Tri::Taille` paient un `metadata()` PAR entrée (`demande_metadata`) — sur le
/// fil unique de `app::run`, donc sur le dock et les pages CEF avec lui. Une
/// série retenue rend la navigation gratuite ; seule l'ouverture paie la lecture,
/// comme le fait déjà le lecteur (`Lecteur.precedent`/`suivant`, calculés à
/// l'ouverture).
///
/// ⚠️ **CE CACHE EST UNE LISTE DE DISQUE** : si le dossier change pendant que la
/// fenêtre est ouverte, la flèche peut désigner un fichier supprimé. Ça se dit et
/// la fenêtre garde ce qu'elle montrait (voir `cef_visionneuse::changer_de_voisin`)
/// — le contraire serait de vider une fenêtre sous les yeux de l'utilisateur.
pub(crate) fn serie_de_media(courant: &Path, tri: Tri, genre: GenreMedia) -> Vec<PathBuf> {
    let Some(dossier) = courant.parent() else {
        return Vec::new();
    };
    let liste = enfants(dossier, tri);
    if liste.tronque {
        // Le voisin peut être de l'autre côté de la coupe : ça se dit, jamais à taire.
        crate::diag!("[sous-dock] serie de media : dossier coupe a {ENFANTS_MAX} entrees, la fleche peut manquer un fichier");
    }
    serie_dans(&liste.enfants, genre)
}

/// **LE RANG DU FICHIER COURANT DANS UNE SÉRIE déjà retenue** — zéro appel système
/// **et zéro clone**, parce que ce rang est lu à chaque `WM_MOUSEMOVE` d'une fenêtre de
/// visualisation (les deux côtés de la flèche) : `voisins_dans_serie`, lui, clone deux
/// chemins et n'est demandé qu'au clic. Voir `serie_de_media` pour la raison d'être de
/// cette découpe.
///
/// ⚠️ **UN FICHIER QUI N'EST PAS DANS LA SÉRIE n'a pas de rang**, donc pas de voisin :
/// supprimé entre l'ouverture et le clic, ou caché (les entrées cachées sont écartées à
/// la lecture). Prêter un rang à un fichier qu'on ne voit pas ferait sauter la flèche
/// n'importe où dans le dossier.
///
/// ⚠️ **ÉGALITÉ EXACTE D'ABORD, la forme pliée en repli.** Un dossier NTFS peut
/// être marqué sensible à la casse (`fsutil file setCaseSensitiveInfo`) et `a.png`
/// y coexiste avec `A.PNG` : plier la casse avant de chercher le rang faisait
/// gagner `a.png`, donc `›` rendait `A.PNG` — **le fichier déjà affiché** — et
/// `b.png` devenait inaccessible : la flèche rembobinait la même image
/// indéfiniment. Dans un dossier ordinaire (insensible), l'exacte ne trouve rien
/// et la forme pliée fait son travail.
pub(crate) fn rang_dans_serie(serie: &[PathBuf], courant: &Path) -> Option<usize> {
    serie.iter().position(|c| c == courant)
        .or_else(|| serie.iter().position(|c| est_le_meme_fichier(c, courant)))
}

/// Les voisins du fichier courant **dans une série déjà retenue**, en clones — le rang
/// se cherche sur `rang_dans_serie`, qui ne clone rien.
pub(crate) fn voisins_dans_serie(serie: &[PathBuf], courant: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
    let Some(rang) = rang_dans_serie(serie, courant) else {
        return (None, None);
    };
    (rang.checked_sub(1).map(|i| serie[i].clone()), serie.get(rang + 1).cloned())
}

/// **LES VOISINS D'UN FICHIER, EN RELISANT SON DOSSIER.**
///
/// ⚠️ **LA LISTE DU PANNEAU NE PEUT PAS SERVIR** : le panneau se referme quand une
/// vidéo ou une image s'ouvre (voir `app::run`) et la fenêtre de visualisation lui
/// survit. La flèche relit donc le disque, avec le TRI de la moitié qui a ouvert
/// le fichier — sinon elle suivrait un ordre autre que celui que l'œil vient de
/// voir à l'écran.
///
/// ⚠️ **UNE FOIS PAR OUVERTURE, pas par appui de flèche** : une fenêtre qui garde
/// sa série (voir `serie_de_media`) navigue sans relire. Ce qui relit ici est ce
/// qui ouvre, jamais ce qui defile.
pub(crate) fn voisins_de_media(courant: &Path, tri: Tri, genre: GenreMedia) -> (Option<PathBuf>, Option<PathBuf>) {
    voisins_dans_serie(&serie_de_media(courant, tri, genre), courant)
}

/// Vrai si le chemin reçoit une VIGNETTE au lieu de son icône : une image (voir
/// `EXTENSIONS_IMAGE`) ou une vidéo (voir `EXTENSIONS_VIDEO`).
#[cfg(test)]
pub(crate) fn a_une_vignette(chemin: &Path) -> bool {
    est_une_image(chemin) || est_une_video(chemin)
}

/// **LA VIGNETTE D'UNE VIDÉO, LUE DANS LE CACHE DU SHELL SEULEMENT** — jamais
/// extraite sur le fil de l'interface.
///
/// ⛔ **MESURÉ : 1,07 s POUR UNE VIDÉO JAMAIS VUE** (`la_sonde_d_une_vignette_video`,
/// un `.mp4` de 20 Mo des Téléchargements), contre 2 à 10 ms une fois le cache
/// rempli. Sur le fil de l'interface, un dossier de vidéos neuves gèlerait le dock
/// une seconde PAR VIDÉO. On lit donc le cache (`SIIGBF_INCACHEONLY`), et une
/// absence part à l'extracteur (voir `demander_extraction`), qui remplit ce cache
/// sur SON fil ; la vidéo est redemandée quand il a fini (voir `extraites`).
pub(crate) fn vignette_video_du_cache(chemin: &Path, cote: u32) -> Option<(u32, u32, Vec<u8>)> {
    vignette_avec(chemin, cote, true)
}

/// Les vidéos dont l'extraction est demandée et pas encore finie — pour ne pas
/// les demander deux fois (chaque relance de l'interface en redemanderait une).
static EN_EXTRACTION: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
/// Les vidéos dont l'extracteur a fini — vidées par l'interface (voir `extraites`).
static EXTRAITES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static EXTRACTEUR: OnceLock<mpsc::Sender<(PathBuf, u32)>> = OnceLock::new();

/// Confie `chemin` à l'extracteur de vignettes vidéo — UN fil, démarré au premier
/// appel, qui traite les vidéos une à une (le shell décode une image de chacune :
/// plusieurs fils se disputeraient le disque et le décodeur pour rien).
pub(crate) fn demander_extraction(chemin: &Path, cote: u32) {
    {
        let mut garde = EN_EXTRACTION.lock().unwrap_or_else(|e| e.into_inner());
        if !garde.get_or_insert_with(HashSet::new).insert(chemin.to_path_buf()) {
            return;
        }
    }
    let envoyeur = EXTRACTEUR.get_or_init(|| {
        let (envoyeur, recepteur) = mpsc::channel::<(PathBuf, u32)>();
        let lance = std::thread::Builder::new().name("vignettes-video".into()).spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(
                    None,
                    windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
                );
            }
            for (chemin, cote) in recepteur {
                let depart = std::time::Instant::now();
                let obtenue = vignette_avec(&chemin, cote, false).is_some();
                crate::diag!("[sous-dock] vignette video {:?} : {obtenue} en {:?}", chemin.file_name(), depart.elapsed());
                if let Some(en_cours) = EN_EXTRACTION.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                    en_cours.remove(&chemin);
                }
                if obtenue {
                    EXTRAITES.lock().unwrap_or_else(|e| e.into_inner()).push(chemin);
                }
            }
        });
        if let Err(e) = lance {
            crate::diag!("[sous-dock] extracteur de vignettes video non demarre : {e}");
        }
        envoyeur
    });
    let _ = envoyeur.send((chemin.to_path_buf(), cote));
}

/// Les vidéos dont la vignette vient d'entrer dans le cache du shell — à
/// redemander par l'interface. Vide la liste.
pub(crate) fn extraites() -> Vec<PathBuf> {
    std::mem::take(&mut *EXTRAITES.lock().unwrap_or_else(|e| e.into_inner()))
}

/// **LA VIGNETTE DU SHELL D'UN CHEMIN**, en RGBA, d'au moins `cote` px de côté —
/// celle que l'Explorateur montre en « Grandes icônes ».
///
/// ⭐ **`IShellItemImageFactory`, PAS UN DÉCODAGE MAISON** : le shell passe par son
/// cache de vignettes (`thumbcache_*.db`), donc une photo de 20 Mpx déjà vue par
/// l'Explorateur coûte une lecture de cache, pas un décodage JPEG complet. Et il
/// sait lire ce que la crate `image` ne sait pas (HEIC, WebP, SVG… selon les
/// extensions installées).
///
/// ⚠️ **`SIIGBF_THUMBNAILONLY`** : sans vignette, le shell rendrait son ICÔNE —
/// on préfère `None`, et l'appelant retombe sur le chemin des icônes, qui la rogne
/// et la cadre (voir `cef_sous_dock::normaliser_icone`).
///
/// ⚠️ **L'ALPHA DU BITMAP RENDU** : une photo arrive souvent avec un alpha à 0
/// partout (le bitmap n'en porte pas) — on le pose alors à 255, sinon la vignette
/// serait entièrement transparente. Ailleurs il est PRÉMULTIPLIÉ, et on le défait.
pub(crate) fn vignette_du_shell(chemin: &Path, cote: u32) -> Option<(u32, u32, Vec<u8>)> {
    vignette_avec(chemin, cote, false)
}

/// `vignette_du_shell`, avec ou sans `SIIGBF_INCACHEONLY` (voir
/// `vignette_video_du_cache`).
fn vignette_avec(chemin: &Path, cote: u32, cache_seulement: bool) -> Option<(u32, u32, Vec<u8>)> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, DIB_RGB_COLORS,
    };
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_INCACHEONLY,
        SIIGBF_THUMBNAILONLY,
    };

    let large: Vec<u16> = chemin.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let fabrique: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(large.as_ptr()), None).ok()?;
        let taille = SIZE { cx: cote as i32, cy: cote as i32 };
        let mut drapeaux = SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK;
        if cache_seulement {
            drapeaux |= SIIGBF_INCACHEONLY;
        }
        let hbitmap = match fabrique.GetImage(taille, drapeaux) {
            Ok(h) => h,
            // Une absence du cache n'est pas une erreur à journaliser : c'est le cas
            // ordinaire d'une vidéo neuve, et l'extracteur s'en charge.
            Err(_) if cache_seulement => return None,
            Err(e) => {
                crate::diag!("[sous-dock] vignette refusee {:?} : {e:?}", chemin.file_name());
                return None;
            }
        };
        let mut bitmap = BITMAP::default();
        let lu = GetObjectW(hbitmap.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bitmap as *mut _ as *mut _));
        let (largeur, hauteur) = (bitmap.bmWidth.max(0) as u32, bitmap.bmHeight.unsigned_abs());
        if lu == 0 || largeur == 0 || hauteur == 0 {
            let _ = DeleteObject(hbitmap.into());
            return None;
        }
        let ecran = GetDC(None);
        let memoire = CreateCompatibleDC(Some(ecran));
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: largeur as i32,
                biHeight: -(hauteur as i32), // de haut en bas
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0, // BI_RGB
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; (largeur * hauteur * 4) as usize];
        let lignes = GetDIBits(memoire, hbitmap, 0, hauteur, Some(bgra.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
        let _ = DeleteDC(memoire);
        ReleaseDC(None, ecran);
        let _ = DeleteObject(hbitmap.into());
        if lignes == 0 {
            return None;
        }
        Some((largeur, hauteur, bgra_premultiplie_vers_rgba(bgra)))
    }
}

/// Le BGRA d'un bitmap du shell vers du RGBA droit — voir `vignette_du_shell` pour
/// l'alpha. Séparé pour être testable sans shell.
fn bgra_premultiplie_vers_rgba(mut px: Vec<u8>) -> Vec<u8> {
    let sans_alpha = px.chunks_exact(4).all(|p| p[3] == 0);
    for p in px.chunks_exact_mut(4) {
        let (b, g, r) = (p[0], p[1], p[2]);
        let a = if sans_alpha { 255 } else { p[3] };
        let defaire = |c: u8| if a == 0 || a == 255 { c } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
        p[0] = defaire(r);
        p[1] = defaire(g);
        p[2] = defaire(b);
        p[3] = a;
    }
    px
}

/// La clé de tri d'un nom : minuscules, et les accents ramenés à leur lettre de
/// base. Sans ça, « Été » se classe APRÈS « Zoo » (le point de code de « É » est
/// plus grand que celui de « Z »), ce que personne n'attend d'une liste.
fn cle_de_tri(nom: &str) -> String {
    nom.to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' | 'ì' => 'i',
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ÿ' | 'ý' => 'y',
            'ñ' => 'n',
            autre => autre,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un dossier temporaire, avec ce qu'on y met, et il se nettoie tout seul.
    struct Bac(std::path::PathBuf);

    impl Bac {
        fn neuf(nom: &str) -> Self {
            let chemin = std::env::temp_dir().join(format!("nd-sousdock-{nom}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&chemin);
            std::fs::create_dir_all(&chemin).expect("dossier temporaire");
            Self(chemin)
        }
        fn fichier(&self, nom: &str) {
            std::fs::write(self.0.join(nom), b"x").expect("ecriture");
        }
        fn dossier(&self, nom: &str) {
            std::fs::create_dir_all(self.0.join(nom)).expect("sous-dossier");
        }
        fn lire(&self) -> Liste {
            self.lire_tri(Tri::Nom)
        }
        fn lire_tri(&self, tri: Tri) -> Liste {
            enfants(&self.0, tri)
        }
    }

    /// Un enfant FABRIQUÉ, sans disque ni horloge — pour vérifier `comparer`
    /// seule. `minute` est un rang, pas un instant : ce qui compte est l'ORDRE.
    fn fabrique(nom: &str, dossier: bool, extension: &str, taille: Option<u64>, minute: u64) -> Enfant {
        Enfant {
            chemin: PathBuf::from(nom),
            nom: nom.to_string(),
            est_dossier: dossier,
            extension: extension.to_string(),
            modifie: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(minute * 60)),
            taille,
        }
    }

    fn noms(enfants: &[Enfant]) -> Vec<&str> {
        enfants.iter().map(|e| e.nom.as_str()).collect()
    }

    impl Drop for Bac {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn les_dossiers_viennent_avant_les_fichiers() {
        let bac = Bac::neuf("ordre");
        bac.fichier("a-fichier.txt");
        bac.dossier("z-dossier");
        let liste = bac.lire();
        assert_eq!(liste.enfants.len(), 2);
        assert!(liste.enfants[0].est_dossier, "le dossier doit passer devant");
        assert_eq!(liste.enfants[0].nom, "z-dossier");
        assert_eq!(liste.enfants[1].nom, "a-fichier.txt");
    }

    /// ⚠️ **LE TRI EST INSENSIBLE AUX ACCENTS, ET C'EST LE POINT DU TEST** : sur
    /// les points de code bruts, « Été » se classerait APRÈS « Zoo ». Un test qui
    /// ne prendrait que des noms ASCII ne prouverait rien de cette propriété.
    #[test]
    fn le_tri_ignore_la_casse_et_les_accents() {
        let bac = Bac::neuf("accents");
        bac.fichier("Zoo.txt");
        bac.fichier("Été.txt");
        bac.fichier("arbre.txt");
        let liste = bac.lire();
        let noms: Vec<&str> = liste.enfants.iter().map(|e| e.nom.as_str()).collect();
        assert_eq!(noms, vec!["arbre.txt", "Été.txt", "Zoo.txt"]);
    }

    #[test]
    fn les_entrees_cachees_sont_ecartees() {
        let bac = Bac::neuf("caches");
        bac.fichier(".caché");
        bac.fichier("visible.txt");
        let liste = bac.lire();
        assert_eq!(liste.enfants.len(), 1);
        assert_eq!(liste.enfants[0].nom, "visible.txt");
    }

    /// Un dossier INEXISTANT rend une liste vide plutôt que de paniquer : le
    /// dossier épinglé peut avoir été supprimé, ou être sur un lecteur retiré.
    #[test]
    fn un_dossier_absent_rend_une_liste_vide() {
        let liste = enfants(std::path::Path::new("Z:/ce/chemin/n'existe/pas"), Tri::Nom);
        assert!(liste.est_vide());
        assert!(!liste.tronque);
    }

    /// ⭐ **LE DÉFAUT SIGNALÉ, EN UN TEST.** « Je ne vois pas les fichiers images
    /// ou pas tous » : la liste était coupée à **48**, et comme les dossiers
    /// passent devant les fichiers dans le tri, un dossier riche en
    /// sous-dossiers n'amenait **aucune** image dans les 48 premières places.
    ///
    /// 60 est choisi exprès : **au-dessus de l'ancienne borne**, et très en
    /// dessous de la nouvelle. Ce test ÉCHOUAIT avant le correctif — c'est lui
    /// qui décrit le défaut, pas un test de plomberie.
    #[test]
    fn un_dossier_de_soixante_fichiers_les_montre_tous() {
        let bac = Bac::neuf("soixante");
        for i in 0..60 {
            bac.fichier(&format!("f{i:03}.txt"));
        }
        let liste = bac.lire();
        assert_eq!(liste.enfants.len(), 60, "aucun fichier ne doit manquer");
        assert!(!liste.tronque, "60 entrées ne sont pas une liste coupee");
    }

    /// **SONDE — CE QUE COÛTE DE LISTER UN VRAI GROS DOSSIER.** À lancer à la
    /// main : `cargo test --bins -- --ignored --nocapture la_sonde_lit_un_gros_dossier`.
    ///
    /// Elle répond à la question qui a motivé le correctif : **lister est-il
    /// vraiment devenu bon marché**, maintenant qu'aucun appel au shell ne s'y
    /// glisse ? Et la coupe à `ENFANTS_MAX` mord-elle sur un dossier réel de
    /// Windows ? Un chiffre, pas une impression — c'est ce qui dira s'il faut
    /// monter la borne, ou chercher ailleurs.
    #[test]
    #[ignore]
    fn la_sonde_lit_un_gros_dossier() {
        let dossier = Path::new(r"C:\Windows\System32");
        let depart = std::time::Instant::now();
        let liste = enfants(dossier, Tri::Nom);
        let ms = depart.elapsed().as_secs_f64() * 1000.0;
        println!(
            "System32 : {} enfants listes, tronque = {}, en {:.0} ms",
            liste.enfants.len(),
            liste.tronque,
            ms
        );
    }

    /// ⚠️ **LA COUPURE SE VÉRIFIE SUR `tronquer`, PAS SUR UN VRAI DOSSIER** :
    /// l'éprouver à travers `enfants` demanderait `ENFANTS_MAX + 3`, soit
    /// **5 003 fichiers** dans `%TEMP%`. Ici, cinq `Enfant` fabriqués suffisent.
    #[test]
    fn la_troncature_est_dite() {
        let mut cinq: Vec<Enfant> =
            (0..5).map(|i| fabrique(&format!("f{i}.txt"), false, "txt", None, 1)).collect();
        assert!(tronquer(&mut cinq, 3), "couper doit se dire");
        assert_eq!(cinq.len(), 3, "la coupe garde bien `max` enfants");

        let mut trois: Vec<Enfant> =
            (0..3).map(|i| fabrique(&format!("f{i}.txt"), false, "txt", None, 1)).collect();
        assert!(!tronquer(&mut trois, 3), "une liste qui tient ne se dit pas coupee");
        assert_eq!(trois.len(), 3);
    }

    // ---- LES QUATRE TRIS ---------------------------------------------------
    //
    // Deux familles de tests, et il faut LES DEUX :
    //  - `comparer` seule, sur des `Enfant` fabriqués — c'est le seul moyen de
    //    vérifier les quatre ordres, et notamment celui par DATE, qu'aucun test
    //    sur disque ne peut rendre déterministe (deux fichiers écrits à la suite
    //    ont la même date à la milliseconde près) ;
    //  - le chemin COMPLET, sur un vrai dossier, mais seulement là où il est
    //    déterministe : la TAILLE et le TYPE. Sans eux, rien ne prouverait que
    //    `metadata()` est réellement appelée — un comparateur juste sur des
    //    valeurs qu'on n'a jamais lues resterait vert.

    #[test]
    fn le_tri_par_taille_met_le_plus_gros_devant() {
        let mut liste = vec![
            fabrique("petit.txt", false, "txt", Some(10), 1),
            fabrique("gros.txt", false, "txt", Some(900), 1),
            fabrique("moyen.txt", false, "txt", Some(100), 1),
        ];
        liste.sort_by(|a, b| comparer(Tri::Taille, a, b));
        assert_eq!(noms(&liste), vec!["gros.txt", "moyen.txt", "petit.txt"]);
    }

    #[test]
    fn le_tri_par_date_met_le_plus_recent_devant() {
        let mut liste = vec![
            fabrique("vieux.txt", false, "txt", None, 1),
            fabrique("recent.txt", false, "txt", None, 99),
            fabrique("median.txt", false, "txt", None, 50),
        ];
        liste.sort_by(|a, b| comparer(Tri::Date, a, b));
        assert_eq!(noms(&liste), vec!["recent.txt", "median.txt", "vieux.txt"]);
    }

    #[test]
    fn le_tri_par_type_regroupe_les_memes_extensions() {
        let mut liste = vec![
            fabrique("b.png", false, "png", None, 1),
            fabrique("a.txt", false, "txt", None, 1),
            fabrique("a.png", false, "png", None, 1),
        ];
        liste.sort_by(|a, b| comparer(Tri::Type, a, b));
        // Les deux .png ensemble, et rangés PAR NOM entre eux ; les .txt après.
        assert_eq!(noms(&liste), vec!["a.png", "b.png", "a.txt"]);
    }

    /// ⚠️ **LES DOSSIERS RESTENT DEVANT DANS LES QUATRE TRIS.** Le perdre en
    /// changeant de tri serait plus surprenant qu'utile, et rien ne le
    /// signalerait : un test du seul tri par nom ne le verrait pas.
    #[test]
    fn les_dossiers_restent_devant_quel_que_soit_le_tri() {
        for tri in [Tri::Nom, Tri::Date, Tri::Taille, Tri::Type] {
            let mut liste = vec![
                fabrique("z.txt", false, "txt", Some(999), 99),
                fabrique("a-dossier", true, "", Some(0), 1),
            ];
            liste.sort_by(|a, b| comparer(tri, a, b));
            assert_eq!(liste[0].nom, "a-dossier", "tri {tri:?} : le dossier doit rester devant");
        }
    }

    /// Un enfant dont la statistique a échoué (`None`) ne doit ni disparaître ni
    /// passer devant : il finit EN DERNIER, le seul rang honnête pour une valeur
    /// qu'on n'a pas.
    #[test]
    fn un_enfant_sans_statistique_se_range_en_dernier() {
        let mut liste = vec![
            fabrique("inconnu.txt", false, "txt", None, 1),
            fabrique("connu.txt", false, "txt", Some(5), 99),
        ];
        liste.sort_by(|a, b| comparer(Tri::Taille, a, b));
        assert_eq!(noms(&liste), vec!["connu.txt", "inconnu.txt"]);
        liste.sort_by(|a, b| comparer(Tri::Date, a, b));
        assert_eq!(noms(&liste), vec!["connu.txt", "inconnu.txt"]);
    }

    /// **LE TEST QUI PROUVE QUE `metadata()` EST VRAIMENT APPELÉE.** Les tests du
    /// comparateur ci-dessus ne le peuvent pas : ils fournissent eux-mêmes des
    /// tailles. Un tri par taille qui ne lirait jamais le disque resterait vert
    /// partout ailleurs.
    #[test]
    fn le_tri_par_taille_lit_vraiment_le_disque() {
        let bac = Bac::neuf("taille-reelle");
        std::fs::write(bac.0.join("gros.bin"), vec![0u8; 4096]).expect("ecriture");
        std::fs::write(bac.0.join("petit.bin"), b"x").expect("ecriture");
        let liste = bac.lire_tri(Tri::Taille);
        assert_eq!(noms(&liste.enfants), vec!["gros.bin", "petit.bin"]);
    }

    /// Et le type se lit sur le NOM, donc sans aucune statistique : c'est ce qui
    /// rend ce tri-là gratuit.
    #[test]
    fn le_tri_par_type_lit_l_extension_du_nom() {
        let bac = Bac::neuf("type-reel");
        bac.fichier("b.txt");
        bac.fichier("a.png");
        let liste = bac.lire_tri(Tri::Type);
        assert_eq!(noms(&liste.enfants), vec!["a.png", "b.txt"]);
    }

    /// La clé qui voyage entre la page et Rust doit faire l'aller-retour, et une
    /// clé inconnue doit être REFUSÉE — sinon la page pourrait ranger le dossier
    /// dans n'importe quel ordre en envoyant n'importe quoi.
    #[test]
    fn la_cle_d_un_tri_fait_l_aller_retour_et_refuse_l_inconnu() {
        for tri in [Tri::Nom, Tri::Date, Tri::Taille, Tri::Type] {
            assert_eq!(Tri::depuis_cle(tri.cle()), Some(tri));
        }
        assert_eq!(Tri::depuis_cle(""), None);
        assert_eq!(Tri::depuis_cle("Nom"), None, "la casse compte : la cle est un protocole");
        assert_eq!(Tri::depuis_cle("taille2"), None);
    }

    /// **LE CÔTÉ FAIT L'ALLER-RETOUR, ET UN CÔTÉ INCONNU EST REFUSÉ.**
    ///
    /// ⛔ Le refus est le point important : un côté DEVINÉ (repli sur la gauche)
    /// enverrait à la moitié gauche l'ordre destiné à la droite, et l'utilisateur
    /// verrait la mauvaise moitié bouger — sans qu'aucune ligne ne le dise.
    #[test]
    fn la_cle_d_un_cote_fait_l_aller_retour_et_refuse_l_inconnu() {
        for cote in Cote::TOUTES {
            assert_eq!(Cote::depuis_cle(cote.cle()), Some(cote));
        }
        assert_eq!(Cote::depuis_cle(""), None);
        assert_eq!(Cote::depuis_cle("G"), None, "la casse compte : la cle est un protocole");
        assert_eq!(Cote::depuis_cle("gauche"), None);
    }

    /// **LES DEUX MOITIÉS SONT DEUX, ET ELLES S'ÉCHANGENT.** `TOUTES` est la
    /// seule énumération (voir sa doc) : si quelqu'un l'allongeait ou la
    /// raccourcissait, tout ce qui l'indexe se décalerait en silence — d'où ce
    /// compte explicite.
    #[test]
    fn les_deux_cotes_sont_complets_et_indexes_sans_trou() {
        assert_eq!(Cote::TOUTES.len(), 2);
        assert_eq!(Cote::TOUTES[0], Cote::Gauche);
        assert_eq!(Cote::TOUTES[1], Cote::Droite);
        for (rang, cote) in Cote::TOUTES.into_iter().enumerate() {
            assert_eq!(cote.index(), rang, "l'indice doit suivre l'ordre de TOUTES");
            assert_eq!(cote.cle().len(), 1, "la cle tient dans UN segment d'URL");
        }
    }

    /// Les images sont reconnues par leur extension, sans casse ; un `.exe` ou un
    /// fichier sans extension garde son icône.
    #[test]
    fn une_image_se_reconnait_a_son_extension() {
        assert!(est_une_image(Path::new("photos/vacances.JPG")));
        assert!(est_une_image(Path::new("dessin.webp")));
        assert!(!est_une_image(Path::new("programme.exe")));
        assert!(!est_une_image(Path::new("LISEZMOI")));
        assert!(!est_une_image(Path::new("archive.png.zip")));
    }

    /// Une vidéo reçoit une vignette comme une image, sans casse ; elle n'est pas
    /// pour autant une IMAGE (`est_une_image` reste fausse).
    #[test]
    fn une_video_recoit_une_vignette() {
        assert!(a_une_vignette(Path::new("clips/vacances.MP4")));
        assert!(a_une_vignette(Path::new("film.mkv")));
        assert!(a_une_vignette(Path::new("photo.jpg")));
        assert!(!est_une_image(Path::new("film.mkv")));
        assert!(!a_une_vignette(Path::new("programme.exe")));
        assert!(!a_une_vignette(Path::new("film.mp4.zip")));
    }

    /// **SONDE — COMBIEN COÛTE LA VIGNETTE D'UNE VIDÉO ?** Elle est demandée sur le
    /// fil de l'interface. `NEURADESK_SONDE_VIDEO=<chemin>`, puis
    /// `cargo test --bins -- --ignored --nocapture la_sonde_d_une_vignette_video`
    #[test]
    #[ignore]
    fn la_sonde_d_une_vignette_video() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let chemin = std::path::PathBuf::from(std::env::var("NEURADESK_SONDE_VIDEO").expect("NEURADESK_SONDE_VIDEO"));
        for essai in 1..=3 {
            let depart = std::time::Instant::now();
            let r = vignette_du_shell(&chemin, 192);
            println!("essai {essai} : {:?} en {:?}", r.map(|(l, h, _)| (l, h)), depart.elapsed());
        }
    }

    /// ⛔ **UN BITMAP SANS ALPHA DONNE UNE VIGNETTE OPAQUE, PAS INVISIBLE** — et un
    /// alpha prémultiplié est défait ; le bleu et le rouge sont permutés (GDI rend
    /// du BGRA).
    #[test]
    fn le_bitmap_du_shell_devient_du_rgba_droit() {
        assert_eq!(bgra_premultiplie_vers_rgba(vec![10, 20, 30, 0]), vec![30, 20, 10, 255], "sans alpha : opaque");
        // Deux pixels : un opaque, un à moitié transparent prémultiplié (100 → 200).
        let px = bgra_premultiplie_vers_rgba(vec![1, 2, 3, 255, 0, 50, 100, 128]);
        assert_eq!(&px[..4], &[3, 2, 1, 255]);
        assert_eq!(&px[4..], &[199, 100, 0, 128]);
    }

    /// **SONDE — UNE IMAGE JAMAIS VUE A-T-ELLE SA VIGNETTE AU PREMIER APPEL ?**
    /// `cargo test --bins -- --ignored --nocapture la_sonde_d_une_vignette_neuve`
    #[test]
    #[ignore]
    fn la_sonde_d_une_vignette_neuve() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let bac = Bac::neuf("vignette");
        let chemin = bac.0.join(format!("neuve-{}.png", std::process::id()));
        let mut image = image::RgbaImage::new(300, 200);
        for (x, _, p) in image.enumerate_pixels_mut() {
            *p = image::Rgba([(x % 256) as u8, 80, 160, 255]);
        }
        image.save(&chemin).unwrap();
        for essai in 1..=3 {
            let depart = std::time::Instant::now();
            let r = vignette_du_shell(&chemin, 192);
            println!("essai {essai} : {:?} en {:?}", r.map(|(l, h, _)| (l, h)), depart.elapsed());
        }
    }

    // ---- LA FLÈCHE DES FENÊTRES DE VISUALISATION ---------------------------
    // Ce qui compte est le RANG, pas le chemin : les voisins sont rendus par
    // leurs noms, et le chemin complet se vérifie dans le test sur disque.

    fn voisins(enfants: &[Enfant], courant: &str, genre: GenreMedia) -> (Option<String>, Option<String>) {
        let nom = |p: Option<PathBuf>| p.and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        let (avant, apres) = voisins_dans_serie(&serie_dans(enfants, genre), Path::new(courant));
        (nom(avant), nom(apres))
    }

    #[test]
    fn la_fleche_parcourt_les_images_dans_l_ordre_recu() {
        let enfants = vec![
            fabrique("a.png", false, "png", None, 1),
            fabrique("b.jpg", false, "jpg", None, 2),
            fabrique("c.png", false, "png", None, 3),
        ];
        assert_eq!(
            voisins(&enfants, "b.jpg", GenreMedia::Image),
            (Some("a.png".to_string()), Some("c.png".to_string())),
            "la fleche suit l'ordre de la liste qu'on lui donne"
        );
    }

    /// **LA SÉRIE RETENUE SE NAVIGUE COMME LA RELIURE** — la visionneuse ne relit
    /// plus le dossier à chaque appui (voir `serie_de_media`) : si la navigation
    /// sur tableau diffère de la navigation sur liste, les deux surfaces du dash
    /// défilent différemment et c'est le défaut des « deux ordres ».
    #[test]
    fn la_serie_retenue_parcourt_comme_la_relecture() {
        let serie = vec![
            PathBuf::from("D:/x/a.png"),
            PathBuf::from("D:/x/b.png"),
            PathBuf::from("D:/x/c.png"),
        ];
        assert_eq!(voisins_dans_serie(&serie, &PathBuf::from("d:/X/A.PNG")), (None, Some(PathBuf::from("D:/x/b.png"))));
        assert_eq!(voisins_dans_serie(&serie, &PathBuf::from("D:/x/c.png")), (Some(PathBuf::from("D:/x/b.png")), None));
        assert_eq!(voisins_dans_serie(&serie, &PathBuf::from("D:/y/b.png")), (None, None), "autre dossier : aucun rang");
        // ⛔ LA FORME `\\?\` (celle que rend `canonicalize`, donc celle qu'un
        // appelant peut donner) ne doit pas éteindre la flèche : sur l'ancien
        // `est_le_meme_fichier`, le `parent` comparé tel quel rendait `(None, None)`
        // et la fenêtre n'avait plus de flèche du tout — sans aucune trace.
        assert_eq!(
            voisins_dans_serie(&serie, &PathBuf::from(r"\\?\D:/x/b.png")),
            (Some(PathBuf::from("D:/x/a.png")), Some(PathBuf::from("D:/x/c.png"))),
            "un chemin canonicalise (prefixe \\\\?\\) doit defiler comme le chemin de la listing"
        );
        // ⛔ LE PARTAGE CANONIQUE (`\\?\UNC\…`) et le partage rendu par `read_dir`
        // (`\\serveur\partage\…`) sont le MÊME fichier : sur l'ancien `forme()`, les
        // deux flèches d'un dossier réseau restaient éteintes des deux côtés, sans
        // la moindre trace — le cas le plus payant de « une absence de trace n'est
        // pas une preuve ».
        let partage = vec![
            PathBuf::from(r"\\Dossiers\Partage\a.png"),
            PathBuf::from(r"\\Dossiers\Partage\b.png"),
            PathBuf::from(r"\\Dossiers\Partage\c.png"),
        ];
        assert_eq!(
            voisins_dans_serie(&partage, &PathBuf::from(r"\\?\UNC\Dossiers\Partage\b.png")),
            (Some(PathBuf::from(r"\\Dossiers\Partage\a.png")), Some(PathBuf::from(r"\\Dossiers\Partage\c.png"))),
            "la forme \\\\?\\\\UNC\\ d'un partage doit defiler comme sa forme de listing"
        );
        // ⚠️ `\\.\` n'est PAS replié : c'est le namespace des périphériques, pas une
        // forme de fichier. Un repli qu'aucun appelant ne produit est du code mort
        // qui se croit utile.
    }

    /// **UN DOSSIER SENSIBLE À LA CASSE NE DOIT PAS FAIRE BOUCLER LA FLÈCHE** —
    /// `fsutil file setCaseSensitiveInfo` permet `a.png` et `A.PNG` côte à côte.
    /// Plier la casse avant de chercher le rang faisait gagner `a.png` pour
    /// `A.PNG`, donc `›` renvoyait le fichier déjà affiché et `b.png` devenait
    /// inaccessible.
    #[test]
    fn deux_noms_de_casses_differentes_sont_deux_fichiers() {
        let serie = vec![
            PathBuf::from("D:/x/a.png"),
            PathBuf::from("D:/x/A.PNG"),
            PathBuf::from("D:/x/b.png"),
        ];
        assert_eq!(
            voisins_dans_serie(&serie, &PathBuf::from("D:/x/A.PNG")),
            (Some(PathBuf::from("D:/x/a.png")), Some(PathBuf::from("D:/x/b.png"))),
            "le nom exact prime : la fleche ne doit pas renvoyer le fichier affiche"
        );
        assert_eq!(
            voisins_dans_serie(&serie, &PathBuf::from("D:/x/a.png")),
            (None, Some(PathBuf::from("D:/x/A.PNG"))),
            "et l'autre rang reste atteignable"
        );
    }

    #[test]
    fn la_fleche_s_arrete_aux_deux_bouts_de_la_series() {
        let enfants = vec![
            fabrique("a.png", false, "png", None, 1),
            fabrique("b.png", false, "png", None, 2),
        ];
        assert_eq!(voisins(&enfants, "a.png", GenreMedia::Image), (None, Some("b.png".to_string())));
        assert_eq!(voisins(&enfants, "b.png", GenreMedia::Image), (Some("a.png".to_string()), None));
    }

    #[test]
    fn la_fleche_ignore_les_dossiers_les_videos_et_les_autres_formats() {
        let enfants = vec![
            fabrique("photos", true, "", None, 1),
            fabrique("fil.mkv", false, "mkv", None, 2),
            fabrique("notes.txt", false, "txt", None, 3),
            fabrique("plan.psd", false, "psd", None, 4),
            fabrique("seule.png", false, "png", None, 5),
        ];
        assert_eq!(voisins(&enfants, "seule.png", GenreMedia::Image), (None, None));
    }

    /// ⛔ **UN `.heic` A UNE VIGNETTE MAIS NE S'AFFICHE PAS.** `EXTENSIONS_IMAGE`
    /// (les vignettes) en compte quatorze, la visionneuse n'en décode que six : la
    /// liste parcourue est celle de ce qui se MONTRE. Ce test échoue si l'on
    /// parcourt `EXTENSIONS_IMAGE`.
    #[test]
    fn un_format_a_vignette_non_decode_n_est_pas_parcouru() {
        let enfants = vec![
            fabrique("a.png", false, "png", None, 1),
            fabrique("b.heic", false, "heic", None, 2),
            fabrique("c.png", false, "png", None, 3),
        ];
        assert_eq!(voisins(&enfants, "a.png", GenreMedia::Image), (None, Some("c.png".to_string())));
    }

    #[test]
    fn les_videos_se_parcour_entre_elles_et_pas_avec_les_images() {
        let enfants = vec![
            fabrique("1.png", false, "png", None, 1),
            fabrique("m.mp4", false, "mp4", None, 2),
            fabrique("n.mkv", false, "mkv", None, 3),
            fabrique("2.png", false, "png", None, 4),
        ];
        assert_eq!(voisins(&enfants, "m.mp4", GenreMedia::Video), (None, Some("n.mkv".to_string())));
        assert_eq!(voisins(&enfants, "n.mkv", GenreMedia::Video), (Some("m.mp4".to_string()), None));
    }

    #[test]
    fn un_fichier_absent_de_la_liste_n_a_pas_de_voisin() {
        let enfants = vec![fabrique("a.png", false, "png", None, 1), fabrique("b.png", false, "png", None, 2)];
        assert_eq!(voisins(&enfants, "zap.png", GenreMedia::Image), (None, None), "inventer un rang ferait sauter la fleche n'importe ou");
    }

    #[test]
    fn la_casse_d_un_chemin_ne_change_pas_le_voisin() {
        let enfants = vec![fabrique("Photo.PNG", false, "png", None, 1), fabrique("autre.png", false, "png", None, 2)];
        assert_eq!(voisins(&enfants, "photo.png", GenreMedia::Image), (None, Some("autre.png".to_string())));
    }

    #[test]
    fn le_genre_de_media_est_ce_que_la_fleche_parcourt() {
        assert_eq!(genre_de_media(Path::new("un.png")), Some(GenreMedia::Image));
        assert_eq!(genre_de_media(Path::new("film.MP4")), Some(GenreMedia::Video));
        assert_eq!(genre_de_media(Path::new("notes.txt")), None);
        assert_eq!(genre_de_media(Path::new("brut")), None);
        assert_eq!(genre_de_media(Path::new("photo.heic")), None, "vignette oui, visionneuse non");
    }

    /// Sur disque : la liste relue est celle du dossier du fichier, et elle ne
    /// retient que le genre demandé.
    #[test]
    fn la_fleche_relit_le_dossier_et_ne_garde_que_le_genre() {
        let bac = Bac::neuf("voisins");
        bac.fichier("a.mp4");
        bac.fichier("b.mkv");
        bac.fichier("c.png");
        bac.fichier("d.txt");
        bac.dossier("sous-dossier");
        let (avant, apres) = voisins_de_media(&bac.0.join("a.mp4"), Tri::Nom, GenreMedia::Video);
        assert_eq!(avant, None, "la premiere video de la serie n'a pas de precedente");
        assert_eq!(apres.as_deref(), Some(bac.0.join("b.mkv").as_path()));
    }
}
