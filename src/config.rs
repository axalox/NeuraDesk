//! Persistance minimale des préférences utilisateur : la couleur des
//! barres du dock et la transparence de chacune, choisies via le menu
//! contextuel de l'icône NeuraDesk (voir `win32::menu`). Stockées dans
//! `%APPDATA%\NeuraDesk\` au format trivial "valeurs séparées par des
//! virgules" — pas de dépendance de parsing externe pour deux réglages.

use std::fs;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;

use crate::ui::zones::SorteEspace;
use crate::win32::clipboard::{self, ClipboardEntry};
use crate::win32::notes::{self, Note, NoteImage};

/// Modèle de `config.toml` généré au tout premier lancement (voir
/// `load_pinned_apps`) — commentaires en français comme le reste de
/// l'interface, exemple en commentaire plutôt qu'une entrée active pour ne
/// rien épingler d'inattendu par défaut.
const DEFAULT_CONFIG_TOML: &str = r#"# Configuration NeuraDesk.
#
# Épingle tes propres applications dans la barre du dock (barre de gauche),
# en plus des icônes intégrées (Web, Terminal, Musique, Code, Lanceur).
# Redémarre NeuraDesk après modification pour appliquer les changements.
#
# Exemple (décommente et adapte le chemin) :
# [[pinned_apps]]
# name = "Spotify"
# path = "C:\\Users\\TonNom\\AppData\\Roaming\\Spotify\\Spotify.exe"
"#;

/// `pub(crate)` : `sauvegarde_config` exporte et importe ce dossier entier.
pub(crate) fn config_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let mut path = PathBuf::from(appdata);
    path.push("NeuraDesk");
    Some(path)
}

/// `pub(crate)` : réutilisé par `win32::apps_cache`, qui persiste le cache
/// disque de l'énumération des applications dans le même dossier
/// `%APPDATA%\NeuraDesk\` que le reste de la configuration, sans dupliquer
/// cette logique de chemin.
pub(crate) fn config_path(file_name: &str) -> Option<PathBuf> {
    let mut path = config_dir()?;
    path.push(file_name);
    Some(path)
}

/// **LE DOSSIER DES CACHES** — `%LOCALAPPDATA%\NeuraDesk`, distinct de celui de
/// la configuration.
///
/// ⚠️ **POURQUOI LES SÉPARER** : `%APPDATA%` (Roaming) est fait pour ce qu'on
/// sauvegarde et qu'on itinérise ; `%LOCALAPPDATA%` (Local) pour ce qui se
/// RÉGÉNÈRE. C'est la convention de Windows, et elle a un sens précis.
///
/// **MESURÉ le 2026-09-24** : `icons_cache.toml` pèse **35 Mo** et
/// `apps_cache.toml` 230 Ko, à côté de 1,5 Ko de vraies préférences. Une
/// sauvegarde du dossier de configuration ramenait donc 35 Mo de données
/// reconstruites au premier lancement — et les deux plus gros fichiers du dossier
/// étaient précisément les deux seuls dont la perte est sans conséquence.
///
/// Xournal++ a fait exactement ce déménagement (« Changed the autosave directory
/// from the user config folder to the user cache folder »).
/// `pub(crate)` depuis le 2026-10-28 : `win32::cef_app` y range le cache de CEF
/// (voir sa doc — le dossier par défaut de CEF est partagé par TOUTES les
/// applications CEF de la machine, et son verrou de singleton survit à un crash).
pub(crate) fn cache_dir() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let mut path = PathBuf::from(local);
    path.push("NeuraDesk");
    Some(path)
}

/// **L'HISTORIQUE D'IMAGES DU PRESSE-PAPIERS, DÉPLACÉ ICI** (2026-10-29).
///
/// ⛔ **POURQUOI** : `clipboard_images/` pesait **1,1 Go en 249 fichiers** dans
/// `%APPDATA%\NeuraDesk` — trente fois `icons_cache.toml`, le cache qui avait
/// déjà justifié la séparation du 2026-09-24. Un profil itinérant le
/// recopierait à chaque ouverture de session ; et une archive « complète » le
/// dépassait le plafond d'import de `sauvegarde_config` (`TAILLE_MAX`, 512 Mo),
/// si bien qu'une sauvegarde complète n'était plus RÉIMPORTABLE.
///
/// ⚠️ **NE SUPPRIME RIEN** : `migrer_clipboard_images` renomme l'ancien dossier
/// (et copie s'il est sur un autre volume). Le dossier d'origine reste tant que
/// le renommage n'a pas réussi.
pub fn migrer_clipboard_images() {
    let (Some(ancien), Some(nouveau)) = (clipboard_images_dir_appdata(), clipboard_images_dir()) else { return };
    match deplacer_dossier(&ancien, &nouveau) {
        Ok(true) => crate::diag!("[cache] historique d'images déplacé : {:?} -> {:?}", ancien, nouveau),
        Ok(false) => {}
        Err(err) => crate::diag!("[cache] historique d'images NON déplacé ({err}) — il reste à {:?}", ancien),
    }
}

/// L'ancienne place de l'historique — le dossier ITINÉRANT. Ne sert plus qu'à
/// la migration ci-dessus.
fn clipboard_images_dir_appdata() -> Option<PathBuf> {
    let mut path = config_dir()?;
    path.push("clipboard_images");
    Some(path)
}

/// **RENOMME `ancien` EN `nouveau`** — rend `true` s'il y a eu déplacement.
/// Sans effet si l'ancien n'existe pas, ou si le nouveau existe déjà (on ne
/// fusionne pas deux historiques : le disque de l'utilisateur n'est pas un bac
/// à sable). ⛔ Sur un autre volume, `fs::rename` échoue : on COPIE alors, et
/// **l'ancien reste** — rien ne se perd, au prix de l'espace.
pub(crate) fn deplacer_dossier(ancien: &Path, nouveau: &Path) -> std::io::Result<bool> {
    if !ancien.is_dir() || nouveau.exists() {
        return Ok(false);
    }
    if let Some(parent) = nouveau.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(ancien, nouveau) {
        Ok(()) => Ok(true),
        Err(_) => {
            copier_arbre(ancien, nouveau)?;
            Ok(true)
        }
    }
}

fn copier_arbre(de: &Path, vers: &Path) -> std::io::Result<()> {
    fs::create_dir_all(vers)?;
    for entree in fs::read_dir(de)? {
        let entree = entree?;
        let cible = vers.join(entree.file_name());
        let sorte = entree.file_type()?;
        if sorte.is_dir() {
            copier_arbre(&entree.path(), &cible)?;
        } else if sorte.is_file() {
            fs::copy(entree.path(), &cible)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests_migration_images {
    use super::*;

    /// Un dossier de test unique — les tests tournent en parallèle.
    fn bac(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("neuradesk_migre_{nom}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// ⛔ Échouait avant le 2026-10-29 : `clipboard_images` était sous
    /// `%APPDATA%` (itinérant), avec 1,1 Go dedans.
    #[test]
    fn les_images_de_l_historique_sont_dans_le_dossier_des_caches() {
        let (Some(images), Some(cache)) = (clipboard_images_dir(), cache_dir()) else { return };
        assert_eq!(images.parent(), Some(cache.as_path()));
        if let Some(config) = config_dir() {
            assert_ne!(images, config.join("clipboard_images"), "l'itinérant ne doit plus les porter");
        }
    }

    #[test]
    fn l_historique_d_images_demenage_avec_son_contenu() {
        let bac = bac("demenage");
        let (ancien, nouveau) = (bac.join("clipboard_images"), bac.join("local").join("clipboard_images"));
        fs::create_dir_all(ancien.join("sous")).unwrap();
        fs::write(ancien.join("a.png"), b"a").unwrap();
        fs::write(ancien.join("sous").join("b.png"), b"b").unwrap();

        assert!(deplacer_dossier(&ancien, &nouveau).unwrap());
        assert!(!ancien.exists(), "l'ancien dossier ne doit plus exister");
        assert_eq!(fs::read(nouveau.join("a.png")).unwrap(), b"a");
        assert_eq!(fs::read(nouveau.join("sous").join("b.png")).unwrap(), b"b");
        let _ = fs::remove_dir_all(&bac);
    }

    #[test]
    fn un_historique_deja_demenage_n_est_pas_touche() {
        let bac = bac("deja");
        let (ancien, nouveau) = (bac.join("itinérant"), bac.join("caches"));
        fs::create_dir_all(&ancien).unwrap();
        fs::create_dir_all(&nouveau).unwrap();
        fs::write(ancien.join("a.png"), b"a").unwrap();
        fs::write(nouveau.join("recent.png"), b"r").unwrap();
        // Le nouveau dossier existe déjà : on ne fusionne RIEN, et l'ancien
        // comme le récent restent en place.
        assert!(!deplacer_dossier(&ancien, &nouveau).unwrap());
        assert!(ancien.join("a.png").is_file(), "la source reste intacte");
        assert!(nouveau.join("recent.png").is_file(), "la cible reste intacte");
        let _ = fs::remove_dir_all(&bac);
    }

    #[test]
    fn rien_a_demenager_quand_il_n_y_a_rien() {
        let bac = bac("rien");
        assert!(!deplacer_dossier(&bac.join("absent"), &bac.join("cible")).unwrap());
        assert!(!bac.join("cible").exists(), "aucun dossier ne doit être créé pour rien");
        let _ = fs::remove_dir_all(&bac);
    }
}

/// Le chemin d'un cache — voir `cache_dir`. `pub(crate)` pour `win32::apps_cache`,
/// comme `config_path`.
pub(crate) fn cache_path(file_name: &str) -> Option<PathBuf> {
    let mut path = cache_dir()?;
    path.push(file_name);
    Some(path)
}

/// **LIT UN CACHE, EN LE DÉMÉNAGEANT AU PASSAGE.** Rend `None` s'il n'existe
/// nulle part — c'est le cas normal au premier lancement, et l'appelant
/// reconstruit alors le cache, ce qui est tout son objet.
///
/// ⚠️ **LE DÉMÉNAGEMENT EST UN RENOMMAGE, PAS UNE COPIE** : un cache relu depuis
/// l'ancien emplacement y resterait sinon indéfiniment, et les 35 Mo
/// d'`icons_cache.toml` seraient toujours dans Roaming au lancement suivant — le
/// déménagement n'aurait rien réglé. Si le renommage échoue (verrou, droits),
/// on rend quand même le contenu : un cache qui ne déménage pas vaut mieux qu'un
/// cache perdu.
pub(crate) fn read_cache(file_name: &str) -> Option<String> {
    let nouveau = cache_path(file_name);
    if let Some(path) = &nouveau {
        if let Ok(content) = fs::read_to_string(path) {
            return Some(content);
        }
    }
    let ancien = config_path(file_name)?;
    let content = fs::read_to_string(&ancien).ok()?;
    if let Some(path) = &nouveau {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::rename(&ancien, path);
    }
    Some(content)
}

pub(crate) fn write_cache(file_name: &str, content: String) {
    let Some(path) = cache_path(file_name) else { return };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, content);
}

/// `pub(crate)` : voir `config_path`, même raison de réutilisation par
/// `win32::apps_cache`.
pub(crate) fn write_config(file_name: &str, content: String) {
    let Some(path) = config_path(file_name) else { return };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, content);
}

/// **ÉCRITURE ATOMIQUE — temporaire puis renommage, et elle DIT si elle a réussi.**
///
/// ⚠️ **POURQUOI ELLE EXISTE À CÔTÉ DE `write_config`.** Celui-ci fait
/// `let _ = fs::write(path, content)` : toute erreur est jetée (chemin `%APPDATA%`
/// irrésoluble, droits, disque plein, verrou d'antivirus, dossier OneDrive non matérialisé),
/// et l'appelant n'apprend rien. Tant que chaque fichier ne portait qu'un réglage, le pire
/// cas était un réglage perdu. `zones.toml` porte **toute** la configuration de zones, et une
/// écriture partielle y laisserait un fichier tronqué que la relecture refuserait — donc la
/// perte totale, et silencieuse.
///
/// Le temporaire est créé dans le **même dossier** que la cible : un renommage ne traverse
/// pas les volumes, donc `rename` reste atomique (un temporaire dans `%TEMP%` pourrait
/// échouer en `EXDEV`).
pub(crate) fn write_config_atomique(file_name: &str, content: String) -> bool {
    let Some(path) = config_path(file_name) else { return false };
    write_atomique(&path, content)
}

/// Le corps de `write_config_atomique`, sur un chemin DÉJÀ résolu.
///
/// `pub(crate)` : réutilisé par `win32::palette_gpl`, qui écrit dans
/// `%APPDATA%\NeuraDesk\palettes\` — un SOUS-DOSSIER, que `config_path` ne
/// peut pas désigner (il n'ajoute qu'un nom de fichier à `config_dir`, voir
/// `palettes_dir`). Même raison et même forme que `config_path`, `pub(crate)`
/// pour `win32::apps_cache` : ne pas dupliquer une écriture atomique.
pub(crate) fn write_atomique(path: &std::path::Path, content: String) -> bool {
    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    let extension = path.extension().and_then(|e| e.to_str()).map_or("tmp".to_string(), |e| format!("{e}.tmp"));
    let mut temporaire = path.to_path_buf();
    temporaire.set_extension(extension);
    if fs::write(&temporaire, content).is_err() {
        let _ = fs::remove_file(&temporaire);
        return false;
    }
    if fs::rename(&temporaire, path).is_err() {
        let _ = fs::remove_file(&temporaire);
        return false;
    }
    true
}

// --- Les ZONES (`zones.toml`) ---------------------------------------------------------
//
// ⚠️ **TOML, ET `toml::Table` CONSTRUIT À LA MAIN** — même choix et même raison que
// `categories.toml` (voir sa doc) : ce fichier porte des NOMS D'UTILISATEUR (accents,
// apostrophes, espaces), et les clés sont relues depuis un fichier que l'utilisateur peut
// éditer. Une sérialisation correcte doit entourer de guillemets au besoin, ce que
// `toml::Table` fait et qu'un format fait main devrait réinventer.

/// Le fichier des zones du dock PRIMAIRE — voir `ui::zones`, doc de module.
pub const ZONES_FILE: &str = "zones.toml";

/// L'état du fichier, tel que la migration doit le connaître.
///
/// **`Absent` DÉCLENCHE la migration, les deux autres non** — et pas de la même façon : un
/// fichier illisible ne se réécrit **jamais**.
#[derive(Debug)]
pub enum EtatZones {
    /// Le fichier n'existe pas : c'est le cas qui déclenche la migration depuis les anciens.
    Absent,
    /// Le fichier existe mais n'a pas pu être lu ou analysé.
    ///
    /// ⚠️ **ON NE LE RÉÉCRIT JAMAIS.** C'est le piège de `load_categories_table`, qui rend
    /// une table VIDE après un `eprintln!` : la sauvegarde suivante écrase alors tout, en
    /// silence. Accepter ce motif ici serait exactement la perte totale que la migration
    /// existe pour éviter.
    Illisible(String),
    /// Le fichier s'est lu. `version` est celle qu'il déclare.
    Lues { zones: crate::ui::zones::Zones, version: i64 },
}

impl EtatZones {
    /// Une écriture est-elle permise ?
    ///
    /// Vraie pour un fichier absent ou d'une version **au plus** celle de ce binaire.
    /// **Fausse pour `Illisible`** (on n'écrase pas ce qu'on n'a pas compris) et **fausse
    /// pour un fichier plus RÉCENT** : il porte des champs que ce binaire ignore, et l'écrire
    /// les détruirait.
    pub fn ecriture_permise(&self) -> bool {
        match self {
            EtatZones::Lues { version, .. } => *version <= crate::ui::zones::ZONES_VERSION,
            EtatZones::Absent => true,
            EtatZones::Illisible(_) => false,
        }
    }
}

pub fn zones_path() -> Option<PathBuf> {
    config_path(ZONES_FILE)
}

/// **Sérialise l'état des zones.** Scalaire `version` d'abord, puis les barres, puis les
/// zones — et **seulement ce qui n'est pas au défaut** : un `locked = false` n'a rien à
/// écrire, même convention que `zone_locks.txt` qui ne portait que ce qui était verrouillé.
pub fn zones_to_toml(zones: &crate::ui::zones::Zones) -> toml::Table {
    use crate::ui::zones::{BarLetter, SorteEspace, ZONES_VERSION};
    use toml::{Table, Value};

    let mut root = Table::new();
    root.insert("version".to_string(), Value::Integer(ZONES_VERSION));
    if let Some(key) = zones.dash_add_zone {
        root.insert("dash_add_zone".to_string(), Value::String(key.to_string()));
    }

    let mut bars = Table::new();
    for bar in BarLetter::ALL {
        let count = zones.spacer_count(bar);
        // Une barre sans espace s'écrit quand même si elle porte une LISTE (une applet
        // déposée au milieu d'une barre du haut sans séparateur, par exemple).
        let a_une_liste = zones.bars.get(&bar).is_some_and(|entree| {
            !entree.icones.is_empty() || !entree.icones_milieu.is_empty() || !entree.icones_droite.is_empty()
        });
        if count == 0 && !a_une_liste {
            continue;
        }
        let mut table = Table::new();
        table.insert("spacers".to_string(), Value::Integer(count as i64));
        // ⚠️ **ON N'ÉCRIT `sizes` QUE S'IL FAIT LA LONGUEUR DU COMPTE** — sinon la relecture
        // l'ignorerait EN ENTIER (voir `Zones::size_at`), et on écrirait un tableau qui ne
        // serait jamais relu. Une liste de tailles est TOUT ou RIEN.
        let sizes = zones.bars.get(&bar).map(|entry| entry.sizes.clone()).unwrap_or_default();
        if sizes.len() == count {
            table.insert(
                "sizes".to_string(),
                Value::Array(sizes.iter().map(|&size| Value::Float(size as f64)).collect()),
            );
        }
        // **LES SORTES, MÊME RÈGLE « TOUT OU RIEN »** que les tailles ci-dessus —
        // mais avec une raison de PLUS de ne rien écrire : une barre dont tous les
        // espaces sont ordinaires n'a pas besoin d'une liste de « rond ». On ne
        // l'écrit donc que si LA LONGUEUR EST BONNE **et** qu'au moins une sorte
        // n'est pas le défaut — sans quoi tout `zones.toml` existant grossirait
        // d'une ligne qui ne dit rien.
        let sortes = zones.bars.get(&bar).map(|entry| entry.sortes.clone()).unwrap_or_default();
        if sortes.len() == count && sortes.iter().any(|s| *s != SorteEspace::Rond) {
            table.insert(
                "sortes".to_string(),
                Value::Array(sortes.iter().map(|s| Value::String(s.cle().to_string())).collect()),
            );
        }
        // **LA LISTE DE LA BARRE** (une barre qui ne déclare aucune zone, voir
        // `BarSpacers::icones`) : même règle que partout — un défaut ne s'écrit jamais, et
        // une liste vide n'est pas un défaut à écrire.
        for (cle, liste) in [
            ("icones", zones.bars.get(&bar).map(|entry| entry.icones.clone()).unwrap_or_default()),
            ("icones_milieu", zones.bars.get(&bar).map(|entry| entry.icones_milieu.clone()).unwrap_or_default()),
            ("icones_droite", zones.bars.get(&bar).map(|entry| entry.icones_droite.clone()).unwrap_or_default()),
        ] {
            if !liste.is_empty() {
                table.insert(
                    cle.to_string(),
                    Value::Array(liste.iter().map(|i| Value::String(i.clone())).collect()),
                );
            }
        }
        bars.insert(bar.as_char().to_string(), Value::Table(table));
    }
    if !bars.is_empty() {
        root.insert("bars".to_string(), Value::Table(bars));
    }

    let mut records = Table::new();
    for (key, record) in &zones.zones {
        let mut table = Table::new();
        table.insert("cote".to_string(), Value::Float(record.cote as f64));
        if record.lead > 0.0 {
            table.insert("lead".to_string(), Value::Float(record.lead as f64));
        }
        if let Some(name) = &record.name {
            table.insert("name".to_string(), Value::String(name.clone()));
        }
        if record.columns > crate::ui::dock::DEFAULT_ZONE_COLUMNS {
            table.insert("columns".to_string(), Value::Integer(record.columns as i64));
        }
        if record.locked {
            table.insert("locked".to_string(), Value::Boolean(true));
        }
        if record.label {
            table.insert("label".to_string(), Value::Boolean(true));
        }
        // Comme `locked`/`label` : on n'écrit QUE ce qui n'est pas au défaut, donc un
        // `zones.toml` déjà écrit ne change pas d'un octet tant que rien n'est caché ou
        // rendu permanent.
        if record.hidden {
            table.insert("hidden".to_string(), Value::Boolean(true));
        }
        if record.permanent {
            table.insert("permanent".to_string(), Value::Boolean(true));
        }
        // ⭐ La LISTE D'ICÔNES, même regle que les autres champs : on n'ecrit que
        // ce qui n'est pas le defaut (ici, une liste vide), donc un `zones.toml`
        // d'avant l'etape 2 ne bouge pas d'un octet.
        if !record.icones.is_empty() {
            table.insert(
                "icones".to_string(),
                Value::Array(record.icones.iter().map(|i| Value::String(i.clone())).collect()),
            );
        }
        // La SORTE, comme les trois précédents : seule une valeur qui N'EST PAS le
        // défaut s'écrit. Un `zones.toml` d'avant cette fonctionnalité ne bouge donc
        // pas d'un octet, et un espace redevenu « Rond » voit sa ligne disparaître
        // plutôt que de garder un `sorte = "rond"` qui ne dit rien.
        records.insert(key.to_string(), Value::Table(table));
    }
    if !records.is_empty() {
        root.insert("zones".to_string(), Value::Table(records));
    }
    root
}

/// **UN FLOTTANT TOML, ÉCRIT `497.0` OU `497`.**
///
/// ⛔ **TOML DISTINGUE L'ENTIER DU FLOTTANT, ET `as_float()` NE CONVERTIT PAS.**
/// `cote = 497` se relit comme un ENTIER, `as_float()` rend `None`, et la zone est
/// alors **écartée en silence** (voir `zones_from_toml`, qui `continue` sur un
/// `None`). Une cote ronde suffit à faire disparaître une zone.
///
/// **POURQUOI ELLE EST ICI, ET PAS DANS CHAQUE MODULE.** Le motif existait déjà
/// dans `gfx::degrade_sauvegardes`, sous forme d'un `as_float().or_else(...)`
/// écrit à la main — et il MANQUAIT ici. Deux copies qui divergent, c'est
/// exactement ce que la troisième aurait consacré : une seule définition, et les
/// trois appelants viennent ici.
///
/// ⚠️ **À L'ÉCRITURE, `{:?}` POUR TOUT `f64`/`f32`** — il garde le « .0 », là où
/// `{}` l'enlève et fabrique un fichier que ce lecteur-ci doit rattraper.
pub fn flottant_toml(valeur: &toml::Value) -> Option<f64> {
    valeur.as_float().or_else(|| valeur.as_integer().map(|entier| entier as f64))
}

/// **Relit l'état des zones — et rend AUSSI ce qu'il a fallu écarter.**
///
/// Le second membre du couple est la liste des clés ou enregistrements refusés : un
/// `zones.toml` édité à la main peut porter une clé illisible ou une zone sans cote, et
/// **une entrée refusée doit se voir** plutôt que de disparaître au premier enregistrement.
pub fn zones_from_toml(table: &toml::Table) -> (crate::ui::zones::Zones, Vec<String>) {
    use crate::ui::dock::SPACER_HEIGHT;
    use crate::ui::zones::{BarLetter, BarSpacers, SorteEspace, ZoneKey, ZoneRecord, Zones};

    let mut zones = Zones::default();
    let mut refuses = Vec::new();

    if let Some(value) = table.get("dash_add_zone") {
        match value.as_str().and_then(ZoneKey::parse) {
            Some(key) => zones.dash_add_zone = Some(key),
            None => refuses.push(format!("dash_add_zone = {value}")),
        }
    }

    if let Some(bars) = table.get("bars").and_then(|value| value.as_table()) {
        for (letter, value) in bars {
            let Some(bar) = letter.chars().next().and_then(BarLetter::from_char) else {
                refuses.push(format!("barre {letter}"));
                continue;
            };
            let Some(table) = value.as_table() else {
                refuses.push(format!("barre {letter}"));
                continue;
            };
            let spacers = table.get("spacers").and_then(|v| v.as_integer()).unwrap_or(0).max(0) as usize;
            let sizes = table
                .get("sizes")
                .and_then(|v| v.as_array())
                .map(|values| {
                    // `flottant_toml` : une taille écrite `18` au lieu de `18.0` est un
                    // ENTIER pour TOML, et `as_float()` seul la remplacerait par le défaut.
                    values.iter().map(|v| flottant_toml(v).unwrap_or(f64::from(SPACER_HEIGHT)) as f32).collect()
                })
                .unwrap_or_default();
            // **LES SORTES — UNE CLÉ INCONNUE SE REFUSE EN LE DISANT, ET ELLE
            // EMPORTE LA LISTE ENTIÈRE.** C'est la règle de `sizes` juste
            // au-dessus : une liste dont la longueur ne vaut plus le compte est
            // ignorée EN ENTIER, parce qu'une entrée déplacée changerait la sorte
            // de tous les espaces suivants. Refuser bruyamment plutôt que de
            // retomber en silence sur « tout est rond » : une sorte perdue
            // changerait le DESSIN de la barre sans que rien ne le dise.
            let mut sortes: Vec<SorteEspace> = Vec::new();
            let mut sortes_valides = true;
            if let Some(values) = table.get("sortes").and_then(|v| v.as_array()) {
                for valeur in values {
                    match valeur.as_str().and_then(SorteEspace::depuis_cle) {
                        Some(sorte) => sortes.push(sorte),
                        None => {
                            eprintln!("[neuradesk] barre {letter} : sorte inconnue {valeur:?} — les sortes de cette barre sont IGNOREES");
                            sortes_valides = false;
                            break;
                        }
                    }
                }
                if !sortes_valides {
                    sortes.clear();
                }
            }
            // **LES LISTES DE LA BARRE** — même règle que celle d'une zone : une entrée qui
            // n'est pas une chaîne est ÉCARTÉE et NOMMÉE, jamais avalée.
            let mut liste = |cle: &str| -> Vec<String> {
                table
                    .get(cle)
                    .and_then(|v| v.as_array())
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|v| {
                                let texte = v.as_str().map(str::trim).filter(|t| !t.is_empty());
                                if texte.is_none() {
                                    refuses.push(format!("barre {letter} : {cle} illisible"));
                                }
                                texte.map(str::to_owned)
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let (icones, icones_milieu, icones_droite) = (liste("icones"), liste("icones_milieu"), liste("icones_droite"));
            zones.bars.insert(bar, BarSpacers { spacers, sizes, sortes, icones, icones_milieu, icones_droite });
        }
    }

    if let Some(records) = table.get("zones").and_then(|value| value.as_table()) {
        for (text, value) in records {
            let Some(key) = ZoneKey::parse(text) else {
                refuses.push(format!("cle {text}"));
                continue;
            };
            let Some(table) = value.as_table() else {
                refuses.push(format!("zone {text}"));
                continue;
            };
            // ⚠️ **LA COTE EST OBLIGATOIRE** : c'est elle qui DÉCLARE la zone (`is_zone_separator`
            // EST `zone_height > 0`). Une cote absente ou nulle décrit une zone qui n'existe
            // pas — l'accepter rendrait indistinguables « zone mal écrite » et « espace
            // ordinaire », et une cote de 0 défait la zone, donc l'écrire serait écrire le
            // contraire de ce qu'on veut.
            // ⛔ **`flottant_toml`, PAS `as_float()` SEUL.** `cote = 497` se relit
            // comme un ENTIER, `as_float()` rend `None`, et la zone était ÉCARTÉE
            // EN SILENCE — le même piège que la coordonnée ronde d'un favori (voir
            // `flottant_toml`). Un `497.0` écrit par erreur à la main effaçait donc
            // une zone, sans que le refus soit même nommé : il passait par la
            // branche « cote absente », qui est un mensonge.
            let Some(cote) = table.get("cote").and_then(flottant_toml).map(|c| c as f32) else {
                refuses.push(format!("zone {text} : cote absente"));
                continue;
            };
            if cote <= 0.0 {
                refuses.push(format!("zone {text} : cote nulle"));
                continue;
            }
            let lead = table.get("lead").and_then(flottant_toml).unwrap_or(0.0).max(0.0) as f32;
            let name = table
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned);
            // Un nombre à zéro colonne n'aurait pas de contenu du tout : plancher à 1, comme
            // `set_zone_columns`, plutôt que d'écrire un compte impossible.
            let columns = table.get("columns").and_then(|v| v.as_integer()).unwrap_or(0).max(1) as u16;
            zones.zones.insert(
                key,
                ZoneRecord {
                    cote,
                    lead,
                    name,
                    columns,
                    locked: table.get("locked").and_then(|v| v.as_bool()).unwrap_or(false),
                    label: table.get("label").and_then(|v| v.as_bool()).unwrap_or(false),
                    hidden: table.get("hidden").and_then(|v| v.as_bool()).unwrap_or(false),
                    permanent: table.get("permanent").and_then(|v| v.as_bool()).unwrap_or(false),
                    // ⭐ LES ICÔNES DE LA ZONE (étape 2 du plan
                    // `docs/plans/disposition-barres-zones-icones.md`). Une entrée qui
                    // n'est pas une chaîne est ÉCARTÉE et NOMMÉE — jamais avalée.
                    icones: table
                        .get("icones")
                        .and_then(|v| v.as_array())
                        .map(|values| {
                            values
                                .iter()
                                .filter_map(|v| {
                                    let texte = v.as_str().map(str::trim).filter(|t| !t.is_empty());
                                    if texte.is_none() {
                                        refuses.push(format!("zone {text} : icone illisible"));
                                    }
                                    texte.map(str::to_owned)
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                },
            );
        }
    }

    (zones, refuses)
}

/// Lit `zones.toml` — voir `EtatZones`, dont les trois cas commandent la migration.
pub fn load_zones() -> EtatZones {
    let Some(path) = zones_path() else { return EtatZones::Illisible("chemin de configuration introuvable".to_string()) };
    let Ok(content) = fs::read_to_string(&path) else { return EtatZones::Absent };
    let table = match content.parse::<toml::Table>() {
        Ok(table) => table,
        Err(err) => return EtatZones::Illisible(err.to_string()),
    };
    let version = table.get("version").and_then(|v| v.as_integer()).unwrap_or(1);
    let (zones, refuses) = zones_from_toml(&table);
    for refuse in &refuses {
        eprintln!("[neuradesk] zones.toml : {refuse} ecarte");
    }
    EtatZones::Lues { zones, version }
}

/// **Écrit `zones.toml`, PUIS RELIT ET COMPARE, et rend le verdict.**
///
/// ⚠️ **POURQUOI LA RELECTURE FAIT PARTIE DE L'ÉCRITURE.** `fs::write` et `fs::rename`
/// peuvent réussir en apparence sans que le contenu soit celui qu'on croit. La migration
/// s'appuie sur ce verdict AVANT de renommer les anciens fichiers : tant qu'il est faux, rien
/// n'a été touché d'irréversible. C'est le motif exact de `cef_dash::handle_add_to_zone`, qui
/// refuse de croire le `bool` de `save_extra_pin` et relit la liste.
pub fn save_zones(zones: &crate::ui::zones::Zones) -> bool {
    // ⚠️⚠️ **ON N'ÉCRASE NI CE QU'ON N'A PAS COMPRIS, NI CE QUI VIENT D'UN BINAIRE PLUS
    // RÉCENT.** `EtatZones::ecriture_permise` porte les deux refus, et c'est LÀ QU'IL DOIT ÊTRE
    // VÉRIFIÉ — pas seulement chez les appelants : accepter le motif de `load_categories_table`
    // (rendre une table vide après un `eprintln!`) ferait écraser par la sauvegarde suivante
    // **toute** la configuration de zones, en silence. C'est la perte totale que ce chantier
    // existe pour éviter.
    if !load_zones().ecriture_permise() {
        eprintln!("[neuradesk] zones.toml : ecriture REFUSEE (fichier illisible ou version plus recente)");
        return false;
    }
    if !write_config_atomique(ZONES_FILE, zones_to_toml(zones).to_string()) {
        return false;
    }
    matches!(load_zones(), EtatZones::Lues { zones: relues, .. } if relues == *zones)
}

/// **L'ÉTAT COMPLET DES ZONES DU DOCK PRIMAIRE, prêt à écrire.**
///
/// Le dock fournit ce qu'il connaît (ses espaces, ses séparateurs, leurs réglages) ; la
/// configuration fournit ce qu'elle connaît — **la zone de destination des ajouts du Dash**,
/// qui vit dans un réglage et non dans le dock. `Dock::zones` ne remplit donc pas
/// `dash_add_zone`, et c'est ce qui garde le module `ui::dock` sans aucune E/S.
pub fn zones_a_ecrire(dock: &crate::ui::dock::Dock) -> crate::ui::zones::Zones {
    let mut zones = dock.zones();
    // ⚠️⚠️ **LA CLÉ VIENT DE `zones.toml`, PLUS DE `dash_add_zone.txt`.** Ce dernier est
    // RENOMMÉ par la migration : le relire ici rendrait `None` dès la première sauvegarde qui
    // suit, et **effacerait le réglage**. C'est le motif que ce chantier corrige — *on écrit
    // avant que les refus aient parlé* — à un détail près : ici, personne ne parle, il faut donc
    // lire la bonne source.
    zones.dash_add_zone = load_zones_dash_add_zone_key();
    // ⭐ **LA RÉPARTITION VIENT DE L'ORDRE VIVANT** — étape 4 du plan
    // `docs/plans/disposition-barres-zones-icones.md`. C'est le dock qui détient les
    // positions, donc c'est lui qui remplit `icones` ; conserver la liste du FICHIER ferait
    // revenir chaque glisser d'icône au lancement suivant (le fichier dit l'ancien ordre, et
    // les listes des zones font autorité).
    //
    // ⚠️ **ET ON GARDE CE QUE CETTE MACHINE NE SAIT PAS RÉSOUDRE** — une épinglette absente,
    // une application d'un autre profil : c'est justement ce que le portable transporte (voir
    // `ui::zones::remplir_les_listes`, qui porte les deux règles).
    if let EtatZones::Lues { zones: lues, .. } = load_zones() {
        let connues = dock.identites_connues();
        crate::ui::zones::remplir_les_listes(
            &mut zones,
            &lues,
            dock.orders(),
            |index| dock.is_spacer(index),
            |index| dock.identite(index),
            |index| !dock.is_hidden(index),
            &connues,
        );
    } else {
        // Aucun `zones.toml` lisible : il n'y a pas d'identité étrangère à garder, mais la
        // répartition doit quand même écrire les listes — sinon le prochain lancement les
        // recréerait à partir de `icon_order.txt`, qui n'est plus la référence.
        crate::ui::zones::repartir_les_icones(
            &mut zones,
            dock.orders(),
            |index| dock.is_spacer(index),
            |index| dock.identite(index),
            |index| !dock.is_hidden(index),
        );
    }
    zones
}

/// **L'ARRANGEMENT COMPLET D'UN DOCK PRIMAIRE — les DEUX fichiers, d'un seul appel.**
///
/// `icon_order.txt` dit *où sont* les éléments d'une barre ; `zones.toml` dit *ce que sont* ses
/// séparateurs (cote, nom, colonnes, verrou) — et les seconds sont désignés par le RANG des
/// premiers. Les écrire séparément laisse donc une fenêtre pendant laquelle les clés de
/// `zones.toml` désignent d'**autres** séparateurs : c'est ce qui faisait revenir un
/// réordonnancement DÉFAIT, ou mélangé, au redémarrage suivant.
///
/// **Une seule porte, appelée par les DEUX modules** qui changent l'arrangement — `app::run`
/// (glisser, créer ou retirer un espace, épingler) et `win32::cef_settings` (ajouter ou supprimer
/// une zone) — et c'est tout l'intérêt : la classe « frère oublié » a été frappée trois fois dans
/// ce dépôt, chaque fois parce qu'un écrivain ne connaissait qu'une PARTIE de l'état.
///
/// **PRIMAIRE SEULEMENT** : un dock d'écran secondaire a ses propres listes d'ordre (rangées dans
/// `SecondaryScreenOverrides::icon_orders`) et AUCUNE zone — c'est `app::persist_icon_order` qui
/// porte ce partage, et c'est pour cela que cette fonction ne le fait pas elle-même.
pub fn save_arrangement(dock: &crate::ui::dock::Dock) {
    let (left, right, top_left, top_right) = dock.icon_order();
    save_icon_order(&left, &right, &top_left, &top_right);
    save_zones(&zones_a_ecrire(dock));
}

/// La clé de la zone de destination du Dash, telle que **`zones.toml`** la porte.
///
/// Un RANG (`D3`), pas l'index d'un séparateur : c'est ce qui la rend insensible à l'ajout
/// d'une épingle. L'ancien `dash_add_zone.txt` enregistrait un index, et c'est précisément lui
/// qui, translaté d'un cran de trop, faisait échouer tout ajout à une zone.
pub fn load_zones_dash_add_zone_key() -> Option<crate::ui::zones::ZoneKey> {
    let EtatZones::Lues { zones, .. } = load_zones() else { return None };
    zones.dash_add_zone
}

/// Le RAPPORT de migration — montré dans le panneau Réglages, et acquitté par l'utilisateur.
///
/// **TOML, et pas le format « une ligne par enregistrement » des anciens fichiers** : il porte
/// des **NOMS D'UTILISATEUR** (accents, apostrophes, espaces), exactement la justification de
/// `zones.toml` et de `categories.toml` (voir la doc en tête de ce module).
pub const ZONES_MIGRATION_FILE: &str = "zones_migration.toml";

/// Sérialise un rapport de migration.
///
/// ⚠️⚠️ **L'ORDRE DES ENTRÉES DANS LE FICHIER N'EST PAS CELUI D'INSERTION — MESURÉ, ET J'AVAIS
/// ÉCRIT LE CONTRAIRE ICI.** Sur la vraie configuration, `[recreees]` sort `D10, D4, D5, D6, D7,
/// D8, D9` : c'est l'ordre **ALPHABÉTIQUE** des clés, alors que `RapportMigration::recreees` est
/// en mémoire trié par **ANCIEN INDEX** (`D4 … D10`). `toml::Table` est un `BTreeMap`, et le
/// sérialiseur remonte de lui-même les valeurs simples avant les tables — ce qui explique aussi
/// que `tailles_abandonnees` précède `[conservees]`, et que `version`/`dash_add_zone` précèdent
/// `[bars]` dans `zones.toml`. **J'en avais déduit à tort que l'ordre d'insertion était
/// préservé** : les deux hypothèses prédisaient la même sortie sur ces deux scalaires, et seule
/// la comparaison des CLÉS DE ZONES les départage.
///
/// Sans conséquence ici, et c'est pourquoi on s'en tient là : le rapport est écrit pour être
/// MONTRÉ, jamais relu en `RapportMigration`, et chaque entrée affiche son ancien index
/// (`D4 (ex-128)`), qui reste l'information utile. **Ne pas se fier à l'ordre du fichier.**
fn zones_migration_to_toml(rapport: &crate::ui::zones::RapportMigration) -> toml::Table {
    use toml::{Table, Value};

    let mut root = Table::new();
    root.insert("tailles_abandonnees".to_string(), Value::Integer(rapport.tailles_abandonnees as i64));

    // **UNE TABLE « CLÉ → ANCIEN INDEX », PAS UNE LISTE DE COUPLES.** C'est ce que le panneau
    // Réglages affiche (`D2 ← 140`), et une table se relit sans code d'analyse.
    for (nom, entrees) in [("conservees", &rapport.conservees), ("recreees", &rapport.recreees)] {
        let mut table = Table::new();
        for (key, ancien) in entrees.iter() {
            table.insert(key.to_string(), Value::Integer(*ancien as i64));
        }
        if !table.is_empty() {
            root.insert(nom.to_string(), Value::Table(table));
        }
    }

    if !rapport.perdues.is_empty() {
        let perdues = rapport
            .perdues
            .iter()
            .map(|perte| {
                let mut table = Table::new();
                table.insert("index".to_string(), Value::Integer(perte.index as i64));
                table.insert("raison".to_string(), Value::String(perte.raison.to_string()));
                Value::Table(table)
            })
            .collect();
        root.insert("perdues".to_string(), Value::Array(perdues));
    }

    root
}

/// **Écrit le rapport de migration**, en écriture ATOMIQUE.
///
/// ⚠️ **APRÈS que `zones.toml` a été écrit ET RELU** (voir `save_zones`) — et non gardé en
/// mémoire, parce qu'on peut très bien ne pas ouvrir les Réglages dans la session qui suit la
/// mise à jour : le rapport doit survivre au redémarrage pour être vu.
///
/// Le rapport n'est **pas** une donnée critique (le perdre ne perd aucune configuration) : la
/// relecture-vérification de `save_zones` n'est pas reprise ici, seule l'atomicité compte.
pub fn save_zones_migration(rapport: &crate::ui::zones::RapportMigration) -> bool {
    write_config_atomique(ZONES_MIGRATION_FILE, zones_migration_to_toml(rapport).to_string())
}

/// Le rapport de migration tel qu'il sera montré — `None` s'il n'y en a pas, ou s'il est
/// illisible. Contrairement à `zones.toml`, un rapport illisible n'est **pas** une perte : on
/// le traite comme absent, et `clear_zones_migration` le fera disparaître.
pub fn load_zones_migration() -> Option<toml::Table> {
    let path = zones_migration_path()?;
    let content = fs::read_to_string(path).ok()?;
    content.parse::<toml::Table>().ok()
}

pub fn zones_migration_path() -> Option<PathBuf> {
    config_path(ZONES_MIGRATION_FILE)
}

/// **L'ACQUITTEMENT** — le rapport a été lu par l'utilisateur, il disparaît.
///
/// Suppression et non marquage : c'est le geste que l'utilisateur demande (« Compris »), et un
/// fichier qui survit à son acquittement finirait par se réafficher au prochain lancement.
pub fn clear_zones_migration() {
    let Some(path) = zones_migration_path() else { return };
    let _ = fs::remove_file(path);
}

/// Les SEPT fichiers que `zones.toml` remplace — **renommés, jamais supprimés**.
///
/// Sept et non six : `spacer_sizes.txt` est du voyage (voir `ui::zones`, décision 4), parce que
/// ses index ne sont traduits par personne et que le livrer séparément ferait lire des index
/// périmés — donc **encoderait le décalage dans le nouveau format**, exactement le défaut réparé.
pub const ANCIENS_FICHIERS_DE_ZONES: [&str; 7] = [
    "zone_heights.txt",
    "zone_names.txt",
    "zone_columns.txt",
    "zone_locks.txt",
    "zone_labels.txt",
    "dash_add_zone.txt",
    "spacer_sizes.txt",
];

/// **LE DERNIER PAS DE LA MIGRATION — et l'ordre de ses opérations est le point porteur.**
///
/// 1. écrire `zones.toml` (`save_zones`, qui **relit et compare**) ;
/// 2. **si et seulement si** cette relecture a réussi, renommer les sept anciens fichiers en
///    `<nom>.avant-zones-toml` ;
/// 3. écrire `zones_migration.toml`.
///
/// ⚠️ **LE RENOMMAGE EST LA SEULE ÉTAPE IRRÉVERSIBLE, DONC LA DERNIÈRE À POUVOIR ÉCHOUER.** Tant
/// que la relecture n'a pas confirmé que `zones.toml` porte bien ce qu'on croit, rien n'a été
/// touché : c'est le motif que `cef_dash::handle_add_to_zone` vient de payer (« une sortie
/// anticipée ne doit jamais laisser une écriture orpheline »), et il est nécessaire ici parce que
/// `write_config` avale ses erreurs — un « succès » n'y veut rien dire sans relecture.
///
/// ⚠️ **RENOMMER, JAMAIS SUPPRIMER** : c'est le geste que l'utilisateur fait déjà lui-même
/// (`avant-reparation-11h/`), et une migration qui détruit ses entrées n'est pas réparable.
///
/// Rend `true` si `zones.toml` a été écrit ET relu — c'est-à-dire si les anciens fichiers ont
/// effectivement été renommés.
pub fn finaliser_la_migration(
    zones: &crate::ui::zones::Zones,
    rapport: &crate::ui::zones::RapportMigration,
) -> bool {
    if !save_zones(zones) {
        eprintln!(
            "[neuradesk] zones.toml n'a pas pu etre ecrit ET relu — les anciens fichiers sont CONSERVES"
        );
        return false;
    }
    for fichier in ANCIENS_FICHIERS_DE_ZONES {
        let Some(source) = config_path(fichier) else { continue };
        if !source.exists() {
            continue;
        }
        let cible = source.with_extension("txt.avant-zones-toml");
        if fs::rename(&source, &cible).is_err() {
            eprintln!("[neuradesk] migration : {fichier} n'a pas pu etre renomme (conserve tel quel)");
        }
    }
    save_zones_migration(rapport);
    true
}

/// **Les six ANCIENS fichiers, index par index** — la matière de la migration.
///
/// Ils sont lus par leurs lecteurs EXISTANTS, réutilisés tels quels : c'est ce qui garantit
/// que la migration part exactement de ce que le code d'avant comprenait, et non d'une
/// seconde interprétation du même format qui pourrait en diverger.
pub fn load_zones_legacy() -> crate::ui::zones::ZonesLegacy {
    crate::ui::zones::ZonesLegacy {
        cotes: load_zone_heights(),
        noms: load_zone_names(),
        colonnes: load_zone_columns(),
        verrous: load_zone_locks(),
        etiquettes: load_zone_labels(),
        dash_add_zone: load_dash_add_zone(),
        tailles: load_spacer_sizes(),
    }
}

/// Décode une ligne de flottants séparés par des VIRGULES — **la seule
/// définition de ce format**, et c'est ce qui a manqué : un écrivain qui choisit
/// un autre séparateur produit un fichier que son propre lecteur ignore en
/// silence. DÉFAUT RÉEL SIGNALÉ : « les valeurs de défilement à la molette ne se
/// sauvegardent pas » — elles étaient écrites sur deux LIGNES, relues comme un
/// seul nombre, et `parse` échouait sans rien dire.
///
/// Un champ illisible est écarté SEUL (`filter_map`) : une ligne abîmée ne doit
/// pas emporter ses voisines.
/// ENCODE une liste de flottants pour `parse_config_floats` — **la seule
/// définition de ce sens-là**, pour la même raison que la sienne : un écrivain qui
/// choisit un autre séparateur produit un fichier que son propre lecteur ignore en
/// silence. Les deux fonctions vont donc par paire, et c'est ce qui garantit que
/// l'aller et le retour ne divergent pas.
fn format_config_floats(values: &[f32]) -> String {
    values.iter().map(|value| value.to_string()).collect::<Vec<_>>().join(",")
}

fn parse_config_floats(content: &str) -> Vec<f32> {
    // `.trim()` sur CHAQUE champ, et pas seulement autour du fichier : un fichier
    // retouché à la main (« 96, 48 ») doit se relire. `" 48".parse::<f32>()`
    // ÉCHOUE — Rust ne tolère aucun espace — et la valeur disparaîtrait sans un
    // mot, exactement comme celle que ce test a été écrit pour rattraper.
    content.split(',').filter_map(|p| p.trim().parse().ok()).collect()
}

/// **LA DERNIÈRE ZONE CAPTURÉE** (`gauche, haut, droite, bas`, en pixels
/// physiques d'écran) — « la même zone que la dernière fois » de la capture
/// d'écran (voir `win32::capture_ecran`, touche Entrée).
pub fn load_derniere_zone_capture() -> Option<[i32; 4]> {
    let v = read_config_floats("capture_derniere_zone.txt")?;
    let [g, h, d, b] = v.as_slice() else { return None };
    let zone = [*g as i32, *h as i32, *d as i32, *b as i32];
    (zone[2] > zone[0] && zone[3] > zone[1]).then_some(zone)
}

pub fn save_derniere_zone_capture(zone: [i32; 4]) {
    write_config("capture_derniere_zone.txt", format!("{},{},{},{}", zone[0], zone[1], zone[2], zone[3]));
}

fn read_config_floats(file_name: &str) -> Option<Vec<f32>> {
    let content = fs::read_to_string(config_path(file_name)?).ok()?;
    Some(parse_config_floats(&content))
}

/// **LE TRI DU SOUS-DOCK**, tel qu'il a été choisi la dernière fois.
///
/// ⚠️ **IL EST PERSISTÉ, ET CE N'EST PAS UN DÉTAIL** : le dépôt a déjà payé DEUX
/// fois un réglage qui semblait mémorisé et ne l'était pas (voir la fiche
/// « flottants : virgules, pas lignes »). Un tri qui repart à « nom » à chaque
/// lancement serait exactement ce défaut-là, et l'utilisateur ne le verrait
/// qu'au redémarrage suivant.
///
/// ⚠️ **ON STOCKE LA CLÉ, JAMAIS L'INDEX DU VARIANT** : ajouter ou réordonner
/// une variante de `Tri` ne doit pas changer le tri de l'utilisateur sans qu'il
/// ait rien demandé. C'est la leçon d'« `Option<Enum>` est un `match` déguisé »,
/// appliquée à un fichier sur disque plutôt qu'à un `match`.
///
/// Une clé illisible (fichier absent, tronqué, ou écrit par une version future)
/// retombe sur le tri par défaut plutôt que d'échouer — même politique que les
/// autres réglages.
/// ⚠️ **LE FICHIER PORTE MAINTENANT DEUX LIGNES, UNE PAR MOITIÉ** (`g=nom`,
/// `d=date`) : le dash dossier montre DEUX dossiers, et chacun a SON ordre.
/// Un tri unique pour les deux moitiés serait un réglage qui ment — on change
/// l'ordre à droite et la gauche se réordonne aussi.
///
/// ⚠️ **L'ANCIENNE FORME (UNE SEULE LIGNE, SANS `=`) RESTE LUE**, et s'applique
/// aux DEUX moitiés : c'est le fichier qu'ont écrit toutes les versions
/// précédentes, et le refuser ferait repartir à « nom » un réglage que
/// l'utilisateur avait choisi — exactement le défaut que la persistance du tri
/// avait été écrite pour éviter.
fn parse_sous_dock_tris(contenu: &str) -> [crate::win32::sous_dock::Tri; 2] {
    use crate::win32::sous_dock::{Cote, Tri};
    let mut tris = [Tri::default(); 2];
    for ligne in contenu.lines().map(str::trim).filter(|l| !l.is_empty()) {
        match ligne.split_once('=') {
            // Forme à deux lignes : `g=nom`.
            Some((cle, valeur)) => {
                if let (Some(cote), Some(tri)) = (Cote::depuis_cle(cle.trim()), Tri::depuis_cle(valeur.trim())) {
                    tris[cote.index()] = tri;
                }
            }
            // Forme historique : une clé nue, qui vaut pour les deux moitiés.
            None => {
                if let Some(tri) = Tri::depuis_cle(ligne) {
                    tris = [tri; 2];
                }
            }
        }
    }
    tris
}

/// L'écrivain de `parse_sous_dock_tris` — **les deux vont par paire**, comme
/// `format_config_floats`/`parse_config_floats` : un écrivain qui choisirait un
/// autre séparateur produirait un fichier que son propre lecteur ignore en
/// silence.
fn format_sous_dock_tris(tris: [crate::win32::sous_dock::Tri; 2]) -> String {
    use crate::win32::sous_dock::Cote;
    Cote::TOUTES
        .into_iter()
        .map(|cote| format!("{}={}", cote.cle(), tris[cote.index()].cle()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn load_sous_dock_tris() -> [crate::win32::sous_dock::Tri; 2] {
    config_path("sous_dock_tri.txt")
        .and_then(|chemin| fs::read_to_string(chemin).ok())
        .map(|contenu| parse_sous_dock_tris(&contenu))
        .unwrap_or_default()
}

/// Sauvegarde les tris de `load_sous_dock_tris` — appelé au moment du choix,
/// jamais à la fermeture : l'application peut être terminée par un arrêt de
/// session.
///
/// ⚠️ **ON ÉCRIT LES DEUX MOITIÉS ENSEMBLE, JAMAIS UNE SEULE.** Un écrivain par
/// côté ferait un lire-modifier-écrire sur le même fichier : le second écraserait
/// la ligne du premier, et le tri de l'autre moitié repartirait à « nom » sans un
/// mot.
pub fn save_sous_dock_tris(tris: [crate::win32::sous_dock::Tri; 2]) {
    write_config("sous_dock_tri.txt", format_sous_dock_tris(tris));
}

/// **LE DOSSIER PERSONNEL DU DASH DOSSIER** — celui sur lequel s'ouvre la moitié
/// DROITE, et le repli de la moitié gauche.
///
/// Le défaut est le profil de l'utilisateur, lu par `SHGetKnownFolderPath`
/// (`FOLDERID_Profile`) et **non** `%USERPROFILE%` en clair : le profil est ce
/// que le shell considère comme le dossier personnel, y compris quand il a été
/// déplacé.
///
/// ⚠️ **UNE VALEUR QUI N'EST PLUS UN DOSSIER EST ÉCARTÉE** (`!is_dir()`) : un
/// chemin choisi puis effacé (clé USB retirée, dossier renommé) laisserait sinon
/// les deux moitiés sur une liste vide dont personne ne saurait dire pourquoi.
/// On retombe alors sur le profil, EN SILENCE — même politique que les autres
/// réglages, et le pire cas est un dossier qui existe.
pub fn load_sous_dock_personnel() -> PathBuf {
    config_path("sous_dock_personnel.txt")
        .and_then(|chemin| fs::read_to_string(chemin).ok())
        .and_then(|contenu| chemin_retenu(Some(&contenu)))
        .unwrap_or_else(|| crate::win32::ssh::local::dossier_personnel_du_profil())
}

/// **LA VALEUR RETENUE D'UN FICHIER DE CHEMIN** — pure, pour être testable SANS
/// toucher au vrai réglage de l'utilisateur.
///
/// ⚠️ **LES DEUX RÉGLAGES DE CHEMIN PASSENT PAR ELLE** (le dossier personnel et
/// le souvenir de gauche) : ce sont les mêmes deux exigences, et les écrire deux
/// fois les ferait diverger à la première retouche.
///
/// Les deux exigences, et pourquoi :
/// - **ABSolu** : un chemin relatif serait interprété depuis le répertoire de
///   travail du processus, qui n'est pas celui que l'utilisateur croit.
/// - **`is_dir()`** : un chemin choisi puis effacé (clé USB retirée, dossier
///   renommé) laisserait une moitié sur une liste vide dont personne ne saurait
///   dire pourquoi.
fn chemin_retenu(contenu: Option<&str>) -> Option<PathBuf> {
    let chemin = PathBuf::from(contenu?.trim());
    (chemin.is_absolute() && chemin.is_dir()).then_some(chemin)
}

/// Sauvegarde le dossier personnel de `load_sous_dock_personnel`.
///
/// ⚠️ **ON ÉCRIT CE QUE L'UTILISATEUR A CHOISI, JAMAIS UN ÉTAT RELU.** La règle
/// du dépôt « une écriture qui propage une perte de lecture » vise les reprises
/// qui réécrivent un état mal relu ; ici la valeur vient du sélecteur natif, donc
/// l'écrire telle quelle est justement ce qui la préserve.
///
/// Rend `false` si l'écriture a échoué — l'appelant le DIT plutôt que de laisser
/// croire à un réglage retenu.
pub fn save_sous_dock_personnel(chemin: &Path) -> bool {
    write_config_atomique("sous_dock_personnel.txt", chemin.display().to_string())
}

/// **CE QUI EST ÉCRIT DANS LE RÉGLAGE, TEL QUEL** — `None` si l'utilisateur n'a
/// jamais choisi (le profil sert alors de défaut).
///
/// ⛔ **CE N'EST PAS LA MÊME QUESTION QUE `load_sous_dock_personnel`, ET C'EST
/// TOUT L'INTÉRÊT.** Celle-ci rend ce qui S'APPLIQUE, après avoir écarté une
/// valeur devenue invalide ; celle-ci rend ce qui a été CHOISI, sans filtre.
/// Les confondre fait disparaître la seule trace d'un choix encore valable mais
/// momentanément introuvable (partage réseau injoignable, clé retirée) : l'écran
/// afficherait alors le profil **comme si l'utilisateur l'avait choisi**, et
/// proposerait de « rétablir » un défaut déjà en place — en effaçant le choix.
///
/// ⚠️ **ELLE LIT LE DISQUE, DONC ELLE NE SE LIT PAS PAR FRAME** : l'appelant la
/// résout une fois et garde le résultat (voir
/// `win32::cef_sous_dock::dossier_personnel`). Un `is_dir()` sur un partage
/// réseau injoignable bloque le fil de rendu pendant le délai SMB — le payer à
/// chaque frame gèlerait l'application tant que le panneau est ouvert.
pub fn sous_dock_personnel_ecrit() -> Option<PathBuf> {
    let contenu = fs::read_to_string(config_path("sous_dock_personnel.txt")?).ok()?;
    let chemin = PathBuf::from(contenu.trim());
    (!chemin.as_os_str().is_empty()).then_some(chemin)
}

/// **LE DERNIER DOSSIER DE LA MOITIÉ GAUCHE** — celui qu'elle remontre quand le
/// panneau s'ouvre sans qu'aucune nappe du dock ne soit survolée.
///
/// ⚠️ **CE N'EST PAS LE DOSSIER PERSONNEL** : c'est un souvenir, et il est
/// distinct à dessein. Les confondre ferait qu'ouvrir le panneau une fois sur un
/// dossier ferait perdre le dossier personnel de la moitié droite.
pub fn load_sous_dock_gauche() -> Option<PathBuf> {
    config_path("sous_dock_gauche.txt")
        .and_then(|chemin| fs::read_to_string(chemin).ok())
        .and_then(|contenu| chemin_retenu(Some(&contenu)))
}

/// Sauvegarde le dernier dossier de gauche.
///
/// ⛔ **C'EST LE FICHIER LE PLUS ÉCRIT DU DÉPÔT** — la moitié gauche change à
/// chaque survol de nappe. D'où la garde de l'appelant (`cef_sous_dock` n'écrit
/// que si le chemin DIFFÈRE de celui déjà retenu) : le dépôt a une plaie ouverte
/// sur une écriture trop confiante, qui a transformé un défaut de lecture
/// transitoire en perte définitive.
///
/// Le fichier est DISTINCT de `dossiers.toml` : ce souvenir ne peut donc pas
/// abîmer la barre du bas.
pub fn save_sous_dock_gauche(chemin: &Path) {
    write_config("sous_dock_gauche.txt", chemin.display().to_string());
}

/// **LA VISIONNEUSE RESTE-T-ELLE AU PREMIER PLAN ?**
///
/// Un réglage GLOBAL, et non un par fenêtre : c'est une préférence d'usage — « je
/// veux lire ce fichier par-dessus ce que je fais » —, pas une propriété d'un
/// document. Le bouton d'épinglage de la fenêtre l'écrit, et **toutes** les
/// visionneuses le relisent à leur ouverture.
///
/// ⚠️ **LE DÉFAUT EST NON**, et c'est le bon : une fenêtre qui se pose devant tout
/// sans qu'on l'ait demandé est une gêne, pas un service. C'est l'utilisateur qui
/// l'active, et son choix survit.
pub fn load_visionneuse_premier_plan() -> bool {
    config_path("visionneuse_premier_plan.txt")
        .and_then(|chemin| fs::read_to_string(chemin).ok())
        .map(|contenu| contenu.trim() == "1")
        .unwrap_or(false)
}

/// Sauvegarde le réglage de `load_visionneuse_premier_plan`, au moment du clic —
/// jamais à la fermeture : l'application peut être terminée par un arrêt de
/// session, qui ne passe par aucun chemin de fermeture.
/// Même règle pour la fenêtre des caractères : écrit au clic, jamais à la fermeture.
pub fn save_visionneuse_premier_plan(actif: bool) {
    write_config("visionneuse_premier_plan.txt", if actif { "1" } else { "0" }.to_string());
}

/// **« Rester au premier plan » DE LA FENÊTRE DES CARACTÈRES** — un réglage PROPRE à
/// elle, comme celui de la visionneuse, et non le drapeau de `cef_popup`.
///
/// ⚠️ **POURQUOI PAS `cef_popup::est_epingle`** : ce drapeau-là est posé par le
/// bouton que Rust INJECTE sur les panneaux ancrés — et une `fenetre: true` n'y a
/// plus droit depuis le 2026-10-06 (elle portait DEUX punaises, voir
/// `cef_popup::render_if_visible`). Cette fenêtre porte la sienne et son propre
/// réglage, comme la visionneuse.
///
/// ⭐ **LE DÉFAUT EST OUI** — décision de l'utilisateur, 2026-10-06 : « il faudrait
/// qu'elle soit réglée pour s'afficher au premier plan par défaut ».
///
/// ⚠️ **CE QUE CE DÉFAUT CHANGE, ET QU'IL FAUT SAVOIR** : une fenêtre au premier
/// plan passe devant TOUT, y compris devant les fenêtres non modales. C'est
/// exactement ce qu'on veut d'une palette de caractères qu'on garde sous la main
/// pendant qu'on écrit ailleurs — mais c'est un choix, et l'utilisateur peut le
/// défaire d'un clic sur la punaise. Le défaut précédent (`false`) avait été choisi
/// sans lui demander.
pub fn load_caracteres_premier_plan() -> bool {
    config_path("caracteres_premier_plan.txt")
        .and_then(|chemin| fs::read_to_string(chemin).ok())
        .map(|contenu| contenu.trim() == "1")
        .unwrap_or(true)
}

pub fn save_caracteres_premier_plan(actif: bool) {
    write_config("caracteres_premier_plan.txt", if actif { "1" } else { "0" }.to_string());
}

/// **LE NIVEAU DE ZOOM DU DASH DOSSIER** (voir `win32::cef_sous_dock::TAILLES_CASE`)
/// — DEMANDE EXPLICITE (2026-10-11) : « pouvoir changer le niveau de zoom ». Un
/// rang, pas une taille : la table des tailles vit avec la page qu'elle décrit.
/// `None` si rien n'est écrit, ou si le fichier est illisible.
pub fn load_dossier_zoom() -> Option<usize> {
    config_path("dossier_zoom.txt")
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse().ok())
}

pub fn save_dossier_zoom(niveau: usize) {
    write_config("dossier_zoom.txt", niveau.to_string());
}

/// La punaise de la fenêtre des symboles phonétiques (voir `win32::cef_phonetique`)
/// — un réglage PROPRE à elle, comme celui des kaomojis.
pub fn load_phonetique_premier_plan() -> bool {
    config_path("phonetique_premier_plan.txt")
        .and_then(|p| fs::read_to_string(p).ok())
        .is_some_and(|s| s.trim() == "1")
}

pub fn save_phonetique_premier_plan(actif: bool) {
    write_config("phonetique_premier_plan.txt", if actif { "1" } else { "0" }.to_string());
}

/// La punaise de la fenêtre des symboles mathématiques (voir `win32::cef_maths`) —
/// un réglage PROPRE à elle, comme celui des symboles phonétiques.
pub fn load_maths_premier_plan() -> bool {
    config_path("maths_premier_plan.txt")
        .and_then(|p| fs::read_to_string(p).ok())
        .is_some_and(|s| s.trim() == "1")
}

pub fn save_maths_premier_plan(actif: bool) {
    write_config("maths_premier_plan.txt", if actif { "1" } else { "0" }.to_string());
}

/// La punaise de la fenêtre « Enregistrer » (voir `win32::cef_enregistrer`) — un
/// réglage PROPRE à elle, comme celui des symboles mathématiques.
pub fn load_enregistrer_premier_plan() -> bool {
    config_path("enregistrer_premier_plan.txt")
        .and_then(|p| fs::read_to_string(p).ok())
        .is_some_and(|s| s.trim() == "1")
}

pub fn save_enregistrer_premier_plan(actif: bool) {
    write_config("enregistrer_premier_plan.txt", if actif { "1" } else { "0" }.to_string());
}

/// **« Rester au premier plan » DE LA FENÊTRE DES KAOMOJIS** — un réglage PROPRE à
/// elle, comme ceux de la visionneuse et des caractères : les deux fenêtres sont
/// indépendantes, donc elles ne partagent pas leur punaise.
///
/// ⭐ **LE DÉFAUT EST OUI**, par cohérence avec la fenêtre des caractères — décision
/// de l'utilisateur le 2026-10-06, et la raison est la même : une palette qu'on garde
/// sous la main pendant qu'on écrit ailleurs.
pub fn load_kaomoji_premier_plan() -> bool {
    config_path("kaomoji_premier_plan.txt")
        .and_then(|chemin| fs::read_to_string(chemin).ok())
        .map(|contenu| contenu.trim() == "1")
        .unwrap_or(true)
}

pub fn save_kaomoji_premier_plan(actif: bool) {
    write_config("kaomoji_premier_plan.txt", if actif { "1" } else { "0" }.to_string());
}

/// Relit la couleur de fond PARTAGÉE (voir `ui::dock::Dock::background`),
/// si elle a été sauvegardée par une version antérieure de l'app (le
/// panneau de réglages actuel ne l'écrit plus, voir `save_bar_colors` : elle
/// n'est plus réglable elle-même, chaque barre a désormais son propre
/// bouton "Couleur d'accentuation Windows"). `None` si jamais sauvegardée,
/// ou si %APPDATA% est introuvable/illisible (le dock retombe alors sur la
/// couleur système, voir `app::run`).
pub fn load_background_color() -> Option<[f32; 4]> {
    match read_config_floats("color.txt")?[..] {
        [r, g, b, a] => Some([r, g, b, a]),
        _ => None,
    }
}

/// Relit la transparence de chaque barre (haut, gauche, droite — force de
/// teinte 0.0..1.0, voir `color::soften_tint`, `win32::menu::Bar`) si elle
/// a été personnalisée. `None` sinon : `app::run` retombe alors sur
/// `color::DEFAULT_BACKDROP_TINT_STRENGTH` pour les trois.
pub fn load_bar_tint() -> Option<[f32; 4]> {
    match read_config_floats("bar_tint.txt")?[..] {
        // `[haut, gauche, droite]` : fichier écrit AVANT la barre du bas, qui
        // prend alors la même transparence que la barre du haut — c'est le
        // défaut de `color::DEFAULT_BACKDROP_TINT_STRENGTH` côté appelant, et
        // accepter les deux formes évite une migration.
        [top, left, right] => Some([top, left, right, top]),
        [top, left, right, bottom] => Some([top, left, right, bottom]),
        _ => None,
    }
}

/// Sauvegarde la transparence des quatre barres, reprise au prochain
/// lancement. Échec silencieux si %APPDATA% est inaccessible en écriture :
/// non bloquant pour l'usage du dock dans la session en cours.
pub fn save_bar_tint(tint: [f32; 4]) {
    write_config(
        "bar_tint.txt",
        format!("{},{},{},{}", tint[0], tint[1], tint[2], tint[3]),
    );
}

/// Épaisseur de la barre du haut / largeur des barres gauche et droite —
/// `[top, left, right]`, même ordre que `bar_tint` (voir
/// `win32::window::Window::set_dock_thicknesses`, réglable via les
/// curseurs "Épaisseur barre du haut"/"Largeur barre gauche"/"Largeur
/// barre droite" du panneau de réglages — demande explicite : les
/// redimensionner individuellement). `None` si jamais personnalisées :
/// `Window::top_thickness`/`left_width`/`right_width` restent alors à leur
/// valeur de départ (l'épaisseur de la vraie barre des tâches Windows
/// détectée au lancement, comportement d'origine).
pub fn load_bar_thicknesses() -> Option<[f32; 4]> {
    match read_config_floats("bar_thicknesses.txt")?[..] {
        // ⚠️ **`[haut, gauche, droite]` : un fichier écrit AVANT la barre du
        // BAS.** On l'accepte tel quel et le bas prend son défaut — c'est ce qui
        // rend l'ajout rétrocompatible, sans migration ni réécriture.
        [top, left, right] => {
            Some([top, left, right, crate::ui::dock::DEFAULT_BOTTOM_THICKNESS])
        }
        [top, left, right, bottom] => Some([top, left, right, bottom]),
        _ => None,
    }
}

/// Relit la répartition/l'ordre des icônes entre les QUATRE zones (deux
/// barres latérales, plus les deux moitiés de la barre du haut — voir
/// `ui::dock::Side`, demande explicite : "déplacer les icônes des barres de
/// gauche et de droite dans la barre supérieure... [selon] le côté de la
/// barre supérieure où elles sont déposées") — quatre lignes (gauche,
/// droite, haut-gauche, haut-droite), chacune une liste d'index séparés par
/// des virgules. Les deux dernières lignes sont ABSENTES d'un fichier écrit
/// avant l'ajout de ce couple de zones : listes vides dans ce cas
/// (rétrocompatibilité), pas un échec de lecture. `None` si jamais
/// personnalisée : `Dock::new` retombe alors sur sa répartition par défaut
/// (tout à gauche sauf Réglages, seul à droite). `Dock::set_icon_order`
/// valide lui-même le résultat (indices hors bornes/icônes manquantes) —
/// pas la peine de le refaire ici.
pub fn load_icon_order() -> Option<(Vec<usize>, Vec<usize>, Vec<usize>, Vec<usize>)> {
    let content = fs::read_to_string(config_path("icon_order.txt")?).ok()?;
    let parse_line = |line: &str| -> Vec<usize> { line.split(',').filter_map(|p| p.trim().parse().ok()).collect() };
    let mut lines = content.lines();
    let left = parse_line(lines.next()?);
    let right = lines.next().map(parse_line).unwrap_or_default();
    let top_left = lines.next().map(parse_line).unwrap_or_default();
    let top_right = lines.next().map(parse_line).unwrap_or_default();
    Some((left, right, top_left, top_right))
}

/// Sauvegarde la répartition/l'ordre des icônes (voir `ui::dock::Dock::
/// icon_order`), repris au prochain lancement. Mêmes garanties
/// (best-effort) que `save_bar_colors`.
pub fn save_icon_order(left: &[usize], right: &[usize], top_left: &[usize], top_right: &[usize]) {
    let format_line = |ids: &[usize]| ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    write_config(
        "icon_order.txt",
        format!(
            "{}\n{}\n{}\n{}",
            format_line(left),
            format_line(right),
            format_line(top_left),
            format_line(top_right)
        ),
    );
}

/// Relit les hauteurs individuelles des espaces invisibles (voir
/// `ui::dock::Dock::spacer_sizes`, demande explicite : "ajouter une option
/// au clic droit sur un séparateur pour choisir sa taille depuis un
/// curseur") — une paire "index,hauteur" par ligne. Liste vide si jamais
/// personnalisée, ou si une ligne est mal formée (ignorée plutôt que de
/// faire échouer toute la lecture pour une seule ligne invalide).
pub fn load_spacer_sizes() -> Vec<(usize, f32)> {
    let Some(path) = config_path("spacer_sizes.txt") else { return Vec::new() };
    let Ok(content) = fs::read_to_string(path) else { return Vec::new() };
    content
        .lines()
        .filter_map(|line| {
            let (index, size) = line.split_once(',')?;
            Some((index.trim().parse().ok()?, size.trim().parse().ok()?))
        })
        .collect()
}

// **`save_spacer_sizes`, `save_zone_columns` ET `save_zone_heights` ONT ÉTÉ SUPPRIMÉES ICI.**
//
// Les trois écrivaient les fichiers indexés par le séparateur que `zones.toml` remplace
// (`spacer_sizes.txt`, `zone_columns.txt`, `zone_heights.txt`). Après la migration, ces fichiers
// sont RENOMMÉS en `.avant-zones-toml` : un écrivain survivant les ferait RENAÎTRE, et deux
// sources coexisteraient pour un même réglage — exactement le motif « frère oublié » que ce
// chantier supprime (il a coûté deux fois la configuration de l'utilisateur).
//
// ⚠️ **LEURS LECTEURS, EUX, RESTENT** — et ils sont maintenant marqués « fichier hérité » :
// `load_zones_legacy` en a besoin pour la MIGRATION, qui doit partir de ce que le code d'avant
// comprenait.
//
// ⚠️ **NE PAS LES RÉINTRODUIRE « au cas où »** : `config::save_zones` est le SEUL écrivain de
// l'état des zones, et c'est ce qui rend l'oubli structurellement impossible.

// **`decaler_les_zones` A ÉTÉ SUPPRIMÉE ICI — et sa disparition est le but du chantier.**
//
// Elle décalait les six fichiers de zone quand une épingle était ajoutée, parce que leurs
// index étaient POSITIONNELS. Or la mesure a montré que ce décalage était un **décalage
// fantôme** : `ensure_spacer_capacity(winpin_base)` remplit jusqu'à une base FIGÉE, et les
// remplisseurs absorbent la croissance — ajouter une épingle ne décale aucun espace (test
// `adding_a_dock_pin_shifts_no_spacer`, `ui::zones`). Elle décalait donc pour rien, et son
// `+1` non appliqué à `sep` (lu dans `dash_add_zone.txt`) faisait échouer TOUT ajout à une
// zone. Les zones sont désormais désignées par `barre + rang` (`ui::zones`), ce qui ne bouge
// plus : il n'y a plus rien à décaler, et la classe « frère oublié » devient impossible.
//
// ⚠️ **NE PAS LA RÉINTRODUIRE « au cas où »** : sa présence est le défaut, pas la parade.

/// Relit le NOMBRE DE COLONNES de chaque zone (voir `ui::dock::Icon::
/// zone_columns`, demande explicite : « que les zones définies par les
/// séparateurs de droite soient découpées en grille dont le nombre de colonnes
/// puisse être défini par l'utilisateur ») — une paire "index,nombre" par
/// ligne, même format et mêmes garanties que `load_spacer_sizes`.
///
/// ⚠️ **FICHIER HÉRITÉ** : depuis `zones.toml`, les colonnes voyagent dans
/// `[zones.<clé>].columns` (voir `zones_from_toml`). Ce lecteur ne sert plus qu'à
/// `load_zones_legacy`, c'est-à-dire à la MIGRATION — et c'est exactement pourquoi il
/// reste : la migration doit partir de ce que le code d'avant comprenait.
///
/// Un nombre nul ou illisible est écarté : `set_zone_columns` ramène de toute
/// façon à 1, mais refuser la ligne ici évite d'écrire un fichier qui
/// prétendrait le contraire.
pub fn load_zone_columns() -> Vec<(usize, u16)> {
    let Some(path) = config_path("zone_columns.txt") else { return Vec::new() };
    let Ok(content) = fs::read_to_string(path) else { return Vec::new() };
    content
        .lines()
        .filter_map(|line| {
            let (index, columns) = line.split_once(',')?;
            let columns: u16 = columns.trim().parse().ok()?;
            // Un zéro n'est pas un compte de colonnes : la ligne est écartée
            // (voir la doc) plutôt que ramenée à 1 en silence.
            if columns == 0 {
                return None;
            }
            Some((index.trim().parse().ok()?, columns))
        })
        .collect()
}

// `save_zone_columns` : supprimée — voir la note groupée plus haut.

/// Relit les COTES des séparateurs de zone (voir
/// `ui::dock::Dock::zone_heights`, `ui::dock::Icon::zone_height`) — même
/// format que `load_spacer_sizes` (une paire "index,cote" par ligne), et
/// mêmes garanties.
///
/// **Fichier SÉPARÉ, et c'est la raison d'être de sa propre paire de
/// fonctions** : `load_spacer_sizes` refuse une ligne mal formée EN ENTIER
/// (`filter_map` sur la ligne, pas sur ses champs), donc un troisième champ
/// ajouté à `spacer_sizes.txt` ferait perdre TOUTES les tailles d'espace à la
/// première ligne abîmée. Deux fichiers, deux échecs indépendants.
pub fn load_zone_heights() -> Vec<(usize, f32, f32)> {
    let Some(path) = config_path("zone_heights.txt") else { return Vec::new() };
    let Ok(content) = fs::read_to_string(path) else { return Vec::new() };
    content
        .lines()
        .filter_map(|line| {
            // `index,cote` puis, si présent, `,vide` — le vide est OPTIONNEL
            // (voir `Dock::zone_heights`) : un fichier écrit avant lui se relit
            // tel quel, avec un vide nul, plutôt que d'être écarté ligne à
            // ligne.
            let mut fields = line.split(',').map(str::trim);
            let index = fields.next()?.parse().ok()?;
            let height = fields.next()?.parse().ok()?;
            let lead = fields.next().and_then(|lead| lead.parse().ok()).unwrap_or(0.0);
            Some((index, height, lead))
        })
        .collect()
}

// `save_zone_heights` : supprimée — voir la note groupée plus haut.

/// Relit le mode d'affichage choisi pour chaque écran secondaire (voir
/// `win32::window::DockMode`, `win32::monitors::MonitorInfo` — demande
/// explicite : "proposer à l'utilisateur de pouvoir placer une copie de la
/// barre supérieure ou une copie de toute l'app sur les écrans
/// secondaires") — une ligne par écran personnalisé, `device_name,mode`
/// (`mode` = `off`/`top`/`full`). Clé par `device_name` (ex. `\\.\DISPLAY2`)
/// plutôt que par index : l'ordre des écrans peut changer au branchement/
/// débranchement d'un moniteur, `device_name` reste stable pour une même
/// sortie vidéo. Écrans absents de ce fichier : traités comme `off` par
/// `app::run` (aucune copie créée par défaut).
pub fn load_secondary_screen_modes() -> std::collections::HashMap<String, String> {
    let Some(path) = config_path("secondary_screens.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let (device_name, mode) = line.split_once(',')?;
            Some((device_name.trim().to_string(), mode.trim().to_string()))
        })
        .collect()
}

/// Sauvegarde le mode d'affichage de chaque écran secondaire (voir
/// `load_secondary_screen_modes`), repris au prochain lancement. Mêmes
/// garanties (best-effort) que `save_icon_order`.
pub fn save_secondary_screen_modes(modes: &std::collections::HashMap<String, String>) {
    let content = modes.iter().map(|(device_name, mode)| format!("{device_name},{mode}")).collect::<Vec<_>>().join("\n");
    write_config("secondary_screens.txt", content);
}

/// Relit l'épaisseur/largeur des barres PERSONNALISÉE de chaque écran
/// secondaire (voir `bar_thicknesses.txt` pour l'équivalent PRIMAIRE,
/// `app::rebuild_secondary_docks` — demande explicite : "que les réglages
/// de hauteur, largeur... soient séparés entre chaque écran") — clé par
/// `device_name` comme `load_secondary_screen_modes`, `[top, left, right]`
/// en pixels physiques de CET écran (pas mis à l'échelle par son DPI, comme
/// `Window::set_dock_thicknesses`). Un écran absent de cette table n'a pas
/// de réglage manuel : `app::rebuild_secondary_docks` continue de dériver
/// son épaisseur automatiquement (`win32::screen_dock::
/// derive_secondary_thickness`), comportement d'origine inchangé.
pub fn load_secondary_screen_thicknesses() -> std::collections::HashMap<String, [f32; 3]> {
    let Some(path) = config_path("secondary_screen_thicknesses.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, ',');
            let device_name = parts.next()?.trim().to_string();
            let top: f32 = parts.next()?.trim().parse().ok()?;
            let left: f32 = parts.next()?.trim().parse().ok()?;
            let right: f32 = parts.next()?.trim().parse().ok()?;
            Some((device_name, [top, left, right]))
        })
        .collect()
}

/// Sauvegarde l'épaisseur/largeur des barres personnalisée de chaque écran
/// secondaire (voir `load_secondary_screen_thicknesses`), reprise au
/// prochain lancement. Mêmes garanties (best-effort) que
/// `save_secondary_screen_modes`.
pub fn save_secondary_screen_thicknesses(thicknesses: &std::collections::HashMap<String, [f32; 3]>) {
    let content = thicknesses
        .iter()
        .map(|(device_name, [top, left, right])| format!("{device_name},{top},{left},{right}"))
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_thicknesses.txt", content);
}

/// Relit les LARGEURS DES SÉPARATEURS DE LA RANGÉE DE WIDGETS de chaque écran
/// secondaire (voir `top_slots.txt` pour l'équivalent PRIMAIRE,
/// `ui::topbar::TopSlot`) — clé par `device_name`, `[après-afficher-le-bureau,
/// après-les-pastilles, après-les-médias]` en pixels, dans l'ordre de
/// `app::persist_top_slots` (qui écrit les deux fichiers).
///
/// **Un écran absent de cette table n'a AUCUN séparateur**, et ce n'est pas un
/// défaut : contrairement aux épaisseurs, teintes et couleurs ci-dessus qui
/// « suivent la primaire par défaut », un séparateur est un élément de MISE EN
/// PAGE — partager les largeurs entre écrans est exactement le défaut qui a
/// motivé ce fichier (« chaque interface doit avoir ses propres réglages sur
/// chaque écran »). Même convention que l'ORDRE DES ICÔNES. Voir
/// `app::resolve_slot_widths`.
pub fn load_secondary_screen_slot_widths() -> std::collections::HashMap<String, crate::ui::topbar::SlotWidths> {
    let Some(path) = config_path("secondary_screen_slot_widths.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    parse_secondary_slot_widths(&content)
}

/// Décode `secondary_screen_slot_widths.txt` — **une ligne par (écran,
/// intervalle)** : `device|nom,w1,w2,…`.
///
/// Deux raisons à cette forme : le `|` est déjà le séparateur-clé des autres
/// tables par écran (voir `parse_secondary_screen_indexed`), et l'intervalle y est
/// désigné par son NOM, jamais par son rang — même décision que `top_slots.txt`
/// (voir `ui::topbar::slot_name` : « un nom persisté, JAMAIS le rang », sans quoi
/// ajouter un intervalle déplacerait les séparateurs de tout le monde). Un
/// intervalle ABSENT n'a pas de ligne : c'est la règle d'absence d'héritage entre
/// écrans, déjà en place.
///
/// **L'ANCIEN FORMAT EST ENCORE LU** — `device,a,b,c`, une largeur par intervalle
/// dans l'ordre de `TOP_SLOTS` — parce qu'il EXISTE sur disque : **mesuré le
/// 2026-09-19**, un écran y était réglé (`\\.\DISPLAY2,40,40,0`) après une heure
/// d'usage de la fonctionnalité. Le lecteur accepte donc les deux formes, mais
/// l'ÉCRIVAIN n'en produit qu'une : le fichier se migre tout seul à la première
/// écriture, et il n'y a jamais deux formats vivants — ce que ce dépôt punit.
/// Les deux formes ne peuvent pas être confondues : la nouvelle porte un `|`.
fn parse_secondary_slot_widths(content: &str) -> std::collections::HashMap<String, crate::ui::topbar::SlotWidths> {
    use crate::ui::topbar;
    let mut table: std::collections::HashMap<String, topbar::SlotWidths> = Default::default();
    for line in content.lines() {
        let Some((device_name, payload)) = line.split_once('|') else {
            // ANCIEN FORMAT (voir la doc ci-dessus).
            let mut parts = line.split(',');
            let Some(device_name) = parts.next().map(str::trim).filter(|name| !name.is_empty()) else { continue };
            let mut slots = topbar::NO_SLOT_WIDTHS;
            for (slot, px) in topbar::TOP_SLOTS.into_iter().zip(&mut parts) {
                if let Ok(px) = px.trim().parse::<f32>() {
                    slots = topbar::set_interval_widths(slots, slot, &[px]);
                }
            }
            table.insert(device_name.to_string(), slots);
            continue;
        };
        let device_name = device_name.trim();
        if device_name.is_empty() {
            continue;
        }
        // `split_once` sur la VIRGULE : ce qui suit est la liste, décodée par
        // l'helper sanctionné — un champ illisible n'y coûte que lui-même.
        let Some((name, widths)) = payload.split_once(',') else { continue };
        let Some(slot) = topbar::slot_from_name(name.trim()) else { continue };
        let slots = table.entry(device_name.to_string()).or_insert(topbar::NO_SLOT_WIDTHS);
        *slots = topbar::set_interval_widths(*slots, slot, &parse_config_floats(widths));
    }
    table
}

/// Encode `secondary_screen_slot_widths.txt` (voir
/// `parse_secondary_slot_widths`) — paire avec lui, jamais l'un sans l'autre.
///
/// **Un intervalle LIBRE n'écrit pas de ligne** : absent et « vide » sont la même
/// chose, exactement comme pour `top_slots.txt`, et c'est ce qui garde vraie la
/// règle « un écran sans réglage n'a aucun séparateur ».
fn format_secondary_slot_widths(widths: &std::collections::HashMap<String, crate::ui::topbar::SlotWidths>) -> String {
    use crate::ui::topbar;
    let mut lines = Vec::new();
    for (device_name, slots) in widths {
        for slot in topbar::TOP_SLOTS {
            let occupied: Vec<f32> = topbar::occupied_widths(*slots, slot).collect();
            if occupied.is_empty() {
                continue;
            }
            lines.push(format!("{device_name}|{},{}", topbar::slot_name(slot), format_config_floats(&occupied)));
        }
    }
    lines.join("\n")
}

/// Sauvegarde les largeurs des séparateurs de rangée de chaque écran secondaire
/// (voir `load_secondary_screen_slot_widths`), reprises au prochain lancement.
/// Mêmes garanties (best-effort) que `save_secondary_screen_thicknesses`.
pub fn save_secondary_screen_slot_widths(widths: &std::collections::HashMap<String, crate::ui::topbar::SlotWidths>) {
    write_config("secondary_screen_slot_widths.txt", format_secondary_slot_widths(widths));
}

/// Relit la transparence des trois barres PERSONNALISÉE de chaque écran
/// secondaire (voir `load_bar_tint` pour l'équivalent PRIMAIRE) — même
/// convention `device_name` que `load_secondary_screen_modes`. Un écran
/// absent retombe sur la transparence de la barre PRIMAIRE (voir
/// `app::rebuild_secondary_docks`).
pub fn load_secondary_screen_tint() -> std::collections::HashMap<String, [f32; 3]> {
    let Some(path) = config_path("secondary_screen_tint.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, ',');
            let device_name = parts.next()?.trim().to_string();
            let top: f32 = parts.next()?.trim().parse().ok()?;
            let left: f32 = parts.next()?.trim().parse().ok()?;
            let right: f32 = parts.next()?.trim().parse().ok()?;
            Some((device_name, [top, left, right]))
        })
        .collect()
}

/// Sauvegarde la transparence des trois barres personnalisée de chaque
/// écran secondaire (voir `load_secondary_screen_tint`), reprise au
/// prochain lancement.
pub fn save_secondary_screen_tint(tint: &std::collections::HashMap<String, [f32; 3]>) {
    let content = tint
        .iter()
        .map(|(device_name, [top, left, right])| format!("{device_name},{top},{left},{right}"))
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_tint.txt", content);
}

/// Relit les facteurs d'échelle PERSONNALISÉS de chaque écran secondaire —
/// DEMANDE EXPLICITE ("l'utilisateur ait la possibilité de régler les
/// tailles du dash et des menus et de leurs éléments pour les différentes
/// interfaces secondaires comme il est possible de le faire pour
/// l'interface principale"). Clé par `device_name` comme les autres
/// `load_secondary_screen_*` ; `[menu, dash_icon, dash_label,
/// dash_category, clipboard_thumb, clipboard_text, settings_content]`, tous
/// des facteurs bruts (`1.0` = 100%, même convention que `load_menu_scale`
/// et consorts). CONTRAIREMENT à `load_secondary_screen_thicknesses`/`_tint`
/// (où chaque CHAMP peut être individuellement absent), une entrée présente
/// ici couvre TOUJOURS les 7 champs à la fois : `win32::cef_settings`
/// ne crée une entrée qu'en décochant la case "Automatique" commune à toute
/// la section (voir `assets/popups/settings.html`,
/// `secondaryScreenScales`), auquel cas elle est amorcée avec les 7 valeurs
/// PRIMAIRES courantes avant d'être individuellement ajustable — un écran
/// absent de cette table suit donc entièrement les réglages globaux/
/// automatiques (comportement d'origine, inchangé).
/// **LE NOMBRE DE CHAMPS D'ÉCHELLE PAR ÉCRAN** — voir les `CHAMP_*` ci-dessous.
pub const NB_CHAMPS_ECRAN: usize = 8;
/// **LES INDICES DES CHAMPS, NOMMÉS.** Ils étaient nus (`0`, `6`…) tant qu'il y
/// en avait sept ; à onze, un `scales[9]` ne dit plus rien à personne.
pub const CHAMP_MENU: usize = 0;
pub const CHAMP_DASH_ICONE: usize = 1;
pub const CHAMP_DASH_ETIQUETTE: usize = 2;
pub const CHAMP_DASH_CATEGORIE: usize = 3;
pub const CHAMP_PRESSE_MINIATURE: usize = 4;
pub const CHAMP_PRESSE_TEXTE: usize = 5;
pub const CHAMP_REGLAGES_CONTENU: usize = 6;
/// Le **ratio de DPI** de cet écran — `RATIO_DPI_AUTO` (0.0) pour l'automatique.
pub const CHAMP_RATIO_DPI: usize = 7;
// ⛔ **PAS DE CHAMP PAR ÉCRAN POUR LES TAILLES DE PANNEAU** — et c'est une
// correction de plan. La taille d'un panneau vit DÉJÀ dans
// `win32::taille_panneaux` (neuf panneaux, un curseur chacun, GLOBAL), et c'est
// là qu'« avoir le même réglage que les autres » se joue. Et `cef_notes` a en
// plus ses deux propres curseurs (« Texte »/« Images », jusqu'à 400 %) : un champ
// par écran les aurait MULTIPLIÉS. Les deux panneaux qui manquaient vraiment —
// l'éditeur d'image et le sous-dock — ont donc rejoint `taille_panneaux`."

/// **« AUCUN RÉGLAGE » POUR LE RATIO DE DPI — ET C'EST `0.0`, PAS `1.0`.**
///
/// ⛔ **LE PIÈGE.** Un fichier écrit par la version d'avant n'a QUE sept valeurs.
/// Si les champs manquants prenaient le défaut des échelles (`1.0`), le ratio
/// vaudrait « 1,0 forcé » — c'est-à-dire **la correction du 2026-10-07
/// DÉSACTIVÉE en silence**, sur chaque écran déjà réglé. `0.0` n'est atteignable
/// par aucun curseur (ils commencent à 25 %), donc il ne peut pas être confondu
/// avec un réglage.
pub const RATIO_DPI_AUTO: f32 = 0.0;

/// La valeur d'un champ ABSENT d'une ligne — `1.0` (neutre) partout, sauf le
/// ratio de DPI (voir `RATIO_DPI_AUTO`).
/// Les champs d'un écran qui n'a AUCUN réglage — `config` en est le seul juge, et
/// `cef_settings` s'en sert quand un curseur est bougé sur un écran dont l'entrée
/// n'existe pas encore.
pub fn champs_par_defaut() -> [f32; NB_CHAMPS_ECRAN] {
    std::array::from_fn(defaut_champ)
}

fn defaut_champ(index: usize) -> f32 {
    if index == CHAMP_RATIO_DPI {
        RATIO_DPI_AUTO
    } else {
        1.0
    }
}

pub fn load_secondary_screen_scales() -> std::collections::HashMap<String, [f32; NB_CHAMPS_ECRAN]> {
    let Some(path) = config_path("secondary_screen_scales.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content.lines().filter_map(champs_depuis_ligne).collect()
}

/// **UNE LIGNE DU FICHIER → UN ÉCRAN ET SES CHAMPS** — extrait de
/// `load_secondary_screen_scales` pour être vérifiable (le fichier, lui, ne
/// s'injecte pas).
///
/// ⚠️ **TOLÉRANT PAR CONSTRUCTION** : une ligne écrite par une version
/// antérieure a MOINS de champs, et les manquants gardent leur défaut. Jeter la
/// ligne entière ferait perdre à l'utilisateur tous ses réglages par écran au
/// premier lancement de cette version — et le défaut du ratio de DPI est `0.0`
/// (automatique), PAS `1.0`, sinon les écrans déjà réglés verraient la correction
/// se désactiver en silence.
pub(crate) fn champs_depuis_ligne(line: &str) -> Option<(String, [f32; NB_CHAMPS_ECRAN])> {
    let mut parts = line.split(',');
    let device_name = parts.next()?.trim().to_string();
    if device_name.is_empty() {
        return None;
    }
    let mut scales: [f32; NB_CHAMPS_ECRAN] = std::array::from_fn(defaut_champ);
    for slot in &mut scales {
        match parts.next() {
            Some(v) => *slot = v.trim().parse().ok()?,
            None => break,
        }
    }
    Some((device_name, scales))
}

/// Sauvegarde les facteurs d'échelle personnalisés de chaque écran
/// secondaire (voir `load_secondary_screen_scales`), repris au prochain
/// lancement.
pub fn save_secondary_screen_scales(scales: &std::collections::HashMap<String, [f32; NB_CHAMPS_ECRAN]>) {
    let content = scales
        .iter()
        .map(|(device_name, values)| {
            let values = values.iter().map(f32::to_string).collect::<Vec<_>>().join(",");
            format!("{device_name},{values}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_scales.txt", content);
}

/// **LE FORÇAGE GLOBAL DU RATIO DE DPI** — `RATIO_DPI_AUTO` (0.0) quand il est
/// éteint, et c'est le défaut.
///
/// ⚠️ **POURQUOI UN RÉGLAGE GLOBAL EN PLUS DU RÉGLAGE PAR ÉCRAN** : celui-ci
/// s'applique à **tous** les écrans, PRIMAIRE comprise. C'est ce qui rend la
/// correction du 2026-10-07 **vérifiable sans second écran** — forcer 67 % et
/// regarder un menu rétrécir d'un tiers. Les réglages par écran, eux, ne
/// couvrent que les secondaires (la page ne liste pas la primaire).
///
/// Il l'emporte sur le réglage d'écran : c'est un levier de diagnostic, il doit
/// gagner.
pub fn load_ratio_dpi_force() -> f32 {
    let Some(path) = config_path("ratio_dpi_force.txt") else { return RATIO_DPI_AUTO };
    fs::read_to_string(path).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(RATIO_DPI_AUTO)
}

pub fn save_ratio_dpi_force(force: f32) {
    write_config("ratio_dpi_force.txt", force.to_string());
}

/// Relit la couleur de fond PERSONNALISÉE des trois barres de chaque écran
/// secondaire (voir `load_bar_colors` pour l'équivalent PRIMAIRE, même
/// encodage `None` via une composante alpha négative) — même convention
/// `device_name` que `load_secondary_screen_modes`. Un écran absent (ou une
/// des trois barres à `None`) retombe sur la couleur résolue de la barre
/// PRIMAIRE correspondante (voir `app::rebuild_secondary_docks`,
/// `ui::dock::Dock::bar_colors`).
pub fn load_secondary_screen_colors() -> std::collections::HashMap<String, [Option<[f32; 4]>; 3]> {
    let Some(path) = config_path("secondary_screen_colors.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(13, ',');
            let device_name = parts.next()?.trim().to_string();
            let floats: Vec<f32> = parts.filter_map(|p| p.trim().parse().ok()).collect();
            if floats.len() != 12 {
                return None;
            }
            let colors = std::array::from_fn(|i| {
                let [r, g, b, a] = [floats[i * 4], floats[i * 4 + 1], floats[i * 4 + 2], floats[i * 4 + 3]];
                if a < 0.0 { None } else { Some([r, g, b, a]) }
            });
            Some((device_name, colors))
        })
        .collect()
}

/// Sauvegarde la couleur de fond personnalisée des trois barres de chaque
/// écran secondaire (voir `load_secondary_screen_colors`), reprise au
/// prochain lancement.
pub fn save_secondary_screen_colors(colors: &std::collections::HashMap<String, [Option<[f32; 4]>; 3]>) {
    let content = colors
        .iter()
        .map(|(device_name, bar_colors)| {
            let floats: Vec<f32> = bar_colors.iter().flat_map(|c| c.unwrap_or([0.0, 0.0, 0.0, -1.0])).collect();
            let joined = floats.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
            format!("{device_name},{joined}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_colors.txt", content);
}

/// Relit la couleur des séparateurs d'icônes PERSONNALISÉE de chaque écran
/// secondaire (voir `load_separator_color` pour l'équivalent PRIMAIRE, même
/// encodage `None`/alpha négative) — DEMANDE EXPLICITE ("donnes la
/// possibilité de choisir leur couleur ou de les rendre transparent...
/// pour... les barres secondaires"). Un écran absent suit la couleur
/// résolue de la barre PRIMAIRE.
pub fn load_secondary_screen_separator_colors() -> std::collections::HashMap<String, [f32; 4]> {
    let Some(path) = config_path("secondary_screen_separator_colors.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(5, ',');
            let device_name = parts.next()?.trim().to_string();
            let floats: Vec<f32> = parts.filter_map(|p| p.trim().parse().ok()).collect();
            if floats.len() != 4 {
                return None;
            }
            Some((device_name, [floats[0], floats[1], floats[2], floats[3]]))
        })
        .collect()
}

/// Sauvegarde la couleur des séparateurs d'icônes personnalisée de chaque
/// écran secondaire (voir `load_secondary_screen_separator_colors`), reprise
/// au prochain lancement. Un écran retiré de la table (retour à "suit la
/// barre principale") n'est simplement plus écrit.
pub fn save_secondary_screen_separator_colors(colors: &std::collections::HashMap<String, [f32; 4]>) {
    let content = colors
        .iter()
        .map(|(device_name, [r, g, b, a])| format!("{device_name},{r},{g},{b},{a}"))
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_separator_colors.txt", content);
}

/// Relit le diamètre PERSONNALISÉ du petit cercle indicateur des séparateurs
/// de chaque écran secondaire (voir `app::SecondaryScreenOverrides::
/// separator_sizes`) — DEMANDE EXPLICITE ("ajoute la possibilité de
/// modifier la taille des séparateurs par interface") : un écran absent de
/// cette table suit la taille PRIMAIRE (`load_separator_size`), même
/// logique que `load_secondary_screen_icon_gaps`.
pub fn load_secondary_screen_separator_sizes() -> std::collections::HashMap<String, f32> {
    let Some(path) = config_path("secondary_screen_separator_sizes.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let (device_name, size) = line.split_once(',')?;
            Some((device_name.trim().to_string(), size.trim().parse().ok()?))
        })
        .collect()
}

/// Sauvegarde le diamètre du petit cercle indicateur des séparateurs
/// personnalisé de chaque écran secondaire (voir
/// `load_secondary_screen_separator_sizes`), repris au prochain lancement.
pub fn save_secondary_screen_separator_sizes(sizes: &std::collections::HashMap<String, f32>) {
    let content = sizes.iter().map(|(device_name, size)| format!("{device_name},{size}")).collect::<Vec<_>>().join("\n");
    write_config("secondary_screen_separator_sizes.txt", content);
}

/// Relit le réglage "Colorisation automatique" (voir `load_auto_color_scheme`
/// pour l'équivalent PRIMAIRE) PERSONNALISÉ de chaque écran secondaire —
/// demande explicite : "les mêmes possibilités de réglages pour l'interface
/// secondaire que pour l'interface principale". Même convention
/// `device_name` que `load_secondary_screen_modes` : un écran ABSENT de
/// cette table n'a pas de réglage manuel et suit `auto_color_scheme` (la
/// barre PRIMAIRE), voir `app::rebuild_secondary_docks`.
pub fn load_secondary_screen_auto_color_scheme() -> std::collections::HashMap<String, bool> {
    let Some(path) = config_path("secondary_screen_auto_color.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let (device_name, flag) = line.split_once(',')?;
            Some((device_name.trim().to_string(), flag.trim() != "0"))
        })
        .collect()
}

/// Sauvegarde le réglage "Colorisation automatique" personnalisé de chaque
/// écran secondaire (voir `load_secondary_screen_auto_color_scheme`), repris
/// au prochain lancement.
pub fn save_secondary_screen_auto_color_scheme(values: &std::collections::HashMap<String, bool>) {
    let content = values
        .iter()
        .map(|(device_name, enabled)| format!("{device_name},{}", if *enabled { 1 } else { 0 }))
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_auto_color.txt", content);
}

/// Relit la couleur du texte de la barre du haut (voir
/// `load_topbar_text_color` pour l'équivalent PRIMAIRE) PERSONNALISÉE de
/// chaque écran secondaire — même convention `device_name` que
/// `load_secondary_screen_modes`. Un écran absent retombe sur la couleur
/// résolue de la barre PRIMAIRE (voir `app::rebuild_secondary_docks`) ; de
/// toute façon sans effet visible tant que la "Colorisation automatique"
/// (voir `load_secondary_screen_auto_color_scheme`) est active pour cet
/// écran — même règle que côté PRIMAIRE.
pub fn load_secondary_screen_topbar_text_color() -> std::collections::HashMap<String, [f32; 3]> {
    let Some(path) = config_path("secondary_screen_topbar_text.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, ',');
            let device_name = parts.next()?.trim().to_string();
            let r: f32 = parts.next()?.trim().parse().ok()?;
            let g: f32 = parts.next()?.trim().parse().ok()?;
            let b: f32 = parts.next()?.trim().parse().ok()?;
            Some((device_name, [r, g, b]))
        })
        .collect()
}

/// Sauvegarde la couleur du texte de la barre du haut personnalisée de
/// chaque écran secondaire (voir `load_secondary_screen_topbar_text_color`),
/// reprise au prochain lancement.
pub fn save_secondary_screen_topbar_text_color(values: &std::collections::HashMap<String, [f32; 3]>) {
    let content = values
        .iter()
        .map(|(device_name, [r, g, b])| format!("{device_name},{r},{g},{b}"))
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_topbar_text.txt", content);
}

/// Relit l'activation de la mini-barre d'horloge plein écran (voir
/// `win32::fullscreen_clock::is_enabled` pour l'équivalent PRIMAIRE)
/// PERSONNALISÉE de chaque écran secondaire — DEMANDE EXPLICITE ("pouvoir
/// régler individuellement les horloges en plein écran en fonction de
/// l'écran où elles s'affichent, je ne veux pas un réglage unique pour
/// toutes les horloges en mode plein écran"). Même convention `device_name`
/// que `load_secondary_screen_modes` : un écran ABSENT de cette table suit
/// le réglage PRIMAIRE, voir `app::rebuild_secondary_docks`.
pub fn load_secondary_screen_fullscreen_clock_enabled() -> std::collections::HashMap<String, bool> {
    let Some(path) = config_path("secondary_screen_fullscreen_clock_enabled.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let (device_name, flag) = line.split_once(',')?;
            Some((device_name.trim().to_string(), flag.trim() != "0"))
        })
        .collect()
}

/// Sauvegarde l'activation personnalisée de chaque écran secondaire (voir
/// `load_secondary_screen_fullscreen_clock_enabled`), reprise au prochain
/// lancement.
pub fn save_secondary_screen_fullscreen_clock_enabled(values: &std::collections::HashMap<String, bool>) {
    let content = values
        .iter()
        .map(|(device_name, enabled)| format!("{device_name},{}", if *enabled { 1 } else { 0 }))
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_fullscreen_clock_enabled.txt", content);
}

/// Relit l'emplacement (haut/bas, voir `win32::fullscreen_clock::
/// is_position_top` pour l'équivalent PRIMAIRE) PERSONNALISÉ de chaque écran
/// secondaire — même convention que `load_secondary_screen_fullscreen_clock_enabled`.
pub fn load_secondary_screen_fullscreen_clock_position_top() -> std::collections::HashMap<String, bool> {
    let Some(path) = config_path("secondary_screen_fullscreen_clock_position.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let (device_name, flag) = line.split_once(',')?;
            Some((device_name.trim().to_string(), flag.trim() != "0"))
        })
        .collect()
}

/// Sauvegarde l'emplacement personnalisé de chaque écran secondaire (voir
/// `load_secondary_screen_fullscreen_clock_position_top`), repris au
/// prochain lancement.
pub fn save_secondary_screen_fullscreen_clock_position_top(values: &std::collections::HashMap<String, bool>) {
    let content = values
        .iter()
        .map(|(device_name, top)| format!("{device_name},{}", if *top { 1 } else { 0 }))
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_fullscreen_clock_position.txt", content);
}

/// Relit le facteur d'échelle de la barre (voir
/// `win32::fullscreen_clock::bar_scale` pour l'équivalent PRIMAIRE)
/// PERSONNALISÉ de chaque écran secondaire — même convention que
/// `load_secondary_screen_fullscreen_clock_enabled`.
pub fn load_secondary_screen_fullscreen_clock_bar_scale() -> std::collections::HashMap<String, f32> {
    let Some(path) = config_path("secondary_screen_fullscreen_clock_bar_scale.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let (device_name, scale) = line.split_once(',')?;
            Some((device_name.trim().to_string(), scale.trim().parse().ok()?))
        })
        .collect()
}

/// Sauvegarde le facteur d'échelle de la barre personnalisé de chaque écran
/// secondaire (voir `load_secondary_screen_fullscreen_clock_bar_scale`),
/// repris au prochain lancement.
pub fn save_secondary_screen_fullscreen_clock_bar_scale(values: &std::collections::HashMap<String, f32>) {
    let content = values
        .iter()
        .map(|(device_name, scale)| format!("{device_name},{scale}"))
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_fullscreen_clock_bar_scale.txt", content);
}

/// Relit le facteur d'échelle du texte (voir
/// `win32::fullscreen_clock::text_scale` pour l'équivalent PRIMAIRE)
/// PERSONNALISÉ de chaque écran secondaire — même convention que
/// `load_secondary_screen_fullscreen_clock_enabled`.
pub fn load_secondary_screen_fullscreen_clock_text_scale() -> std::collections::HashMap<String, f32> {
    let Some(path) = config_path("secondary_screen_fullscreen_clock_text_scale.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let (device_name, scale) = line.split_once(',')?;
            Some((device_name.trim().to_string(), scale.trim().parse().ok()?))
        })
        .collect()
}

/// Sauvegarde le facteur d'échelle du texte personnalisé de chaque écran
/// secondaire (voir `load_secondary_screen_fullscreen_clock_text_scale`),
/// repris au prochain lancement.
pub fn save_secondary_screen_fullscreen_clock_text_scale(values: &std::collections::HashMap<String, f32>) {
    let content = values
        .iter()
        .map(|(device_name, scale)| format!("{device_name},{scale}"))
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_fullscreen_clock_text_scale.txt", content);
}

/// Relit les éléments (icônes de barre du haut/dashs — mêmes identifiants
/// que `app::Features`, voir `win32::cef_settings::apply_feature`)
/// DÉSACTIVÉS individuellement sur chaque écran secondaire — DEMANDE
/// EXPLICITE ("ajoute la possibilité de sélectionner ou déselectionner des
/// éléments des barres secondaires pour les rendre visibles ou
/// invisibles"). Même convention `device_name` que
/// `load_secondary_screen_modes`, mais contrairement aux réglages
/// "épaisseur"/"transparence"/"couleur" ci-dessus (un tableau à 3 valeurs
/// PAR écran, une valeur manquante = automatique), ce réglage est une LISTE
/// DE NOMS : chaque nom présent sur la ligne d'un écran est masqué sur CET
/// écran spécifiquement, PAR-DESSUS le réglage global correspondant (voir
/// `app::Features`) — un écran absent de ce fichier (ou un nom absent de sa
/// ligne) suit simplement le réglage global, comme n'importe quel autre
/// écran/élément non personnalisé.
pub fn load_secondary_screen_features() -> std::collections::HashMap<String, Vec<String>> {
    let Some(path) = config_path("secondary_screen_features.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let mut parts = line.split(',');
            let device_name = parts.next()?.trim().to_string();
            let disabled: Vec<String> = parts.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
            Some((device_name, disabled))
        })
        .collect()
}

/// Sauvegarde les éléments désactivés par écran secondaire (voir
/// `load_secondary_screen_features`), repris au prochain lancement. Mêmes
/// garanties (best-effort) que `save_secondary_screen_modes`.
pub fn save_secondary_screen_features(features: &std::collections::HashMap<String, Vec<String>>) {
    let content = features
        .iter()
        .map(|(device_name, names)| {
            if names.is_empty() {
                device_name.clone()
            } else {
                format!("{device_name},{}", names.join(","))
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_features.txt", content);
}

/// Relit les strips (entrées) et bus (sorties) Voicemeeter masqués du popup
/// (voir `win32::popup_menu::render_voicemeeter`, demande explicite :
/// "avoir la possibilité de cacher des entrées et des sorties") — deux
/// lignes, une liste d'index Voicemeeter ORIGINAUX (`Strip[i]`/`Bus[i]`, PAS
/// la position affichée une fois les masqués retirés) séparés par des
/// virgules : strips masqués, puis bus masqués. Même format/mêmes garanties
/// que `load_icon_order` (listes vides si jamais personnalisé, ligne
/// absente/mal formée ignorée plutôt que de faire échouer toute la lecture).
pub fn load_voicemeeter_hidden_channels() -> (Vec<usize>, Vec<usize>) {
    let Some(path) = config_path("voicemeeter_hidden_channels.txt") else { return (Vec::new(), Vec::new()) };
    let Ok(content) = fs::read_to_string(path) else { return (Vec::new(), Vec::new()) };
    let parse_line = |line: &str| -> Vec<usize> { line.split(',').filter_map(|p| p.trim().parse().ok()).collect() };
    let mut lines = content.lines();
    let hidden_strips = lines.next().map(parse_line).unwrap_or_default();
    let hidden_buses = lines.next().map(parse_line).unwrap_or_default();
    (hidden_strips, hidden_buses)
}

/// Sauvegarde les strips/bus Voicemeeter masqués, repris au prochain
/// lancement. Mêmes garanties (best-effort) que `save_icon_order`.
pub fn save_voicemeeter_hidden_channels(hidden_strips: &[usize], hidden_buses: &[usize]) {
    let format_line = |ids: &[usize]| ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    write_config("voicemeeter_hidden_channels.txt", format!("{}\n{}", format_line(hidden_strips), format_line(hidden_buses)));
}

/// Sauvegarde l'épaisseur/largeur des trois barres, reprise au prochain
/// lancement. Mêmes garanties (best-effort) que `save_bar_tint`.
pub fn save_bar_thicknesses(thicknesses: [f32; 4]) {
    write_config(
        "bar_thicknesses.txt",
        format!(
            "{},{},{},{}",
            thicknesses[0], thicknesses[1], thicknesses[2], thicknesses[3]
        ),
    );
}

/// Verrou de CHAQUE barre (voir `ui::dock::Dock::bar_locks`) — trois drapeaux,
/// `[haut, gauche, droite]`, sur une ligne, même forme que
/// `bar_thicknesses.txt` : `1` verrouillée, `0` libre. Absent ou illisible =
/// aucune barre verrouillée, c'est-à-dire le comportement d'avant ce réglage.
///
/// **Un `1`/`0` plutôt que « la barre est listée »** : le fichier dit alors
/// aussi, noir sur blanc, quelles barres sont LIBRES — alors qu'une liste
/// d'index rendrait une barre absente indiscernable d'une barre oubliée.
pub fn load_bar_locks() -> [bool; 4] {
    match read_config_floats("bar_locks.txt").as_deref() {
        // `[haut, gauche, droite]` : fichier écrit AVANT la barre du bas, qui
        // arrive donc NON verrouillée — le défaut, et le seul choix sensé.
        Some([top, left, right]) => [*top != 0.0, *left != 0.0, *right != 0.0, false],
        Some([top, left, right, bottom]) => {
            [*top != 0.0, *left != 0.0, *right != 0.0, *bottom != 0.0]
        }
        _ => [false; 4],
    }
}

/// Sauvegarde les verrous de barre (voir `load_bar_locks`). Mêmes garanties
/// (best-effort) que `save_bar_thicknesses`.
pub fn save_bar_locks(locks: [bool; 4]) {
    let bit = |locked: bool| if locked { 1 } else { 0 };
    write_config(
        "bar_locks.txt",
        format!(
            "{},{},{},{}",
            bit(locks[0]),
            bit(locks[1]),
            bit(locks[2]),
            bit(locks[3])
        ),
    );
}

/// Index des SÉPARATEURS DE ZONE FIGÉS (voir `ui::dock::Dock::zone_locks`) — un
/// index par ligne, rien d'autre.
///
/// **Une liste, et non des paires « index,valeur » comme `zone_heights.txt`** :
/// le verrou n'a qu'un ÉTAT, et écrire « 12,0 » pour dire « pas verrouillé »
/// ferait un fichier qui prétend décrire toutes les zones alors qu'il n'en
/// connaît qu'une partie. Une ligne illisible est écartée seule (même garantie
/// que `load_spacer_sizes`).
pub fn load_zone_locks() -> Vec<usize> {
    let Some(path) = config_path("zone_locks.txt") else { return Vec::new() };
    let Ok(content) = fs::read_to_string(path) else { return Vec::new() };
    content.lines().filter_map(|line| line.trim().parse().ok()).collect()
}

/// NOMS donnés aux zones (voir `ui::dock::Dock::zone_names`) — `index,nom` par
/// ligne, DEMANDE EXPLICITE : « permettre de renommer une zone à l'utilisateur ».
///
/// **Le nom est TOUT ce qui suit la PREMIÈRE virgule** (`split_once`), donc il
/// peut en contenir d'autres : un format qui découperait sur toutes les virgules
/// tronquerait « Zone, la vraie » sans le dire. Seul le RETOUR À LA LIGNE ne
/// peut pas passer — le champ est sur une ligne — et il est écrasé à
/// l'écriture plutôt que de scinder le fichier en deux, dont la seconde ligne
/// serait perdue.
///
/// **Pas d'encodage en pourcent ici**, contrairement aux actions de la page :
/// celles-ci reçoivent leur texte d'un `encodeURIComponent`, alors que ce
/// fichier est écrit et relu par le MÊME code, qui n'a donc rien à échapper.
pub fn load_zone_names() -> Vec<(usize, String)> {
    let Some(path) = config_path("zone_names.txt") else { return Vec::new() };
    let Ok(content) = fs::read_to_string(path) else { return Vec::new() };
    content
        .lines()
        .filter_map(|line| {
            let (index, name) = line.split_once(',')?;
            let name = name.trim();
            // Un nom VIDE est écarté : il ne se distingue pas d'une zone jamais
            // nommée, et `set_zone_name` écarte déjà les blancs.
            if name.is_empty() {
                return None;
            }
            Some((index.trim().parse().ok()?, name.to_string()))
        })
        .collect()
}

// **`save_zone_names`, `save_zone_labels` ET `save_zone_locks` ONT ÉTÉ SUPPRIMÉES ICI** — même
// raison que les trois plus haut : elles écrivaient des fichiers indexés par le séparateur que
// `zones.toml` remplace, et la migration les RENOMME. Un écrivain survivant les ferait renaître.
//
// Le nom y échappait même un piège qui n'a plus lieu d'être : un retour à la ligne devait être
// écrasé, sinon le nom se scindait en deux lignes dont la seconde était relue comme une AUTRE
// zone. TOML règle ça par la grammaire (voir `zones_to_toml`, qui entoure de guillemets).
//
// ⚠️ **LEURS LECTEURS RESTENT**, marqués « fichier hérité » : `load_zones_legacy` en a besoin
// pour la MIGRATION.

/// Index des zones qui AFFICHENT leur numéro ou leur nom en haut d'elles-mêmes
/// (voir `ui::dock::Dock::zone_label_shown`) — DEMANDE EXPLICITE : « que le
/// numéro ou le nom de la zone apparaisse en haut de celle-ci si l'utilisateur
/// choisit de le faire depuis les réglages des verrous ».
///
/// Un index par ligne, même forme et même raison que `zone_locks.txt` : une
/// LISTE, parce qu'un « non » n'a rien à écrire.
pub fn load_zone_labels() -> Vec<usize> {
    let Some(path) = config_path("zone_labels.txt") else { return Vec::new() };
    let Ok(content) = fs::read_to_string(path) else { return Vec::new() };
    content.lines().filter_map(|line| line.trim().parse().ok()).collect()
}

/// Taille du texte des étiquettes de zone, en pixels (voir `ui::dock::
/// zone_label_font_px`) — DEMANDE EXPLICITE : « des curseurs pour régler la
/// taille du nom des zones ». Absent = le défaut, `DEFAULT_ZONE_LABEL_FONT_PX`.
pub fn load_zone_label_font_px() -> Option<u16> {
    read_config_floats("zone_label_font.txt")?.first().map(|px| *px as u16)
}

/// Sauvegarde la taille du texte des étiquettes de zone (voir
/// `load_zone_label_font_px`).
pub fn save_zone_label_font_px(px: u16) {
    write_config("zone_label_font.txt", px.to_string());
}

// `save_zone_labels` et `save_zone_locks` : supprimées — voir la note groupée plus haut.

/// Fréquence (Hz) à laquelle `app::run` plafonne SA PROPRE boucle de rendu
/// (voir `app::frame_budget`) — DEMANDE EXPLICITE ("une option... qui
/// permette de choisir la fréquence de rafraîchissement de l'app pour les
/// fréquences les plus courantes"), choisie via le clic droit sur l'icône
/// NeuraDesk (`win32::cef_context`, `assets/popups/context.html`). `None`
/// si jamais personnalisé (choix "Automatique") : `app::run` reste alors calé
/// sur le vsync de l'écran (`gfx::swapchain::SwapChain::present`),
/// comportement d'origine.
pub fn load_app_refresh_rate_cap() -> Option<u32> {
    read_config_floats("app_refresh_rate.txt")?.first().map(|&v| v as u32)
}

/// Sauvegarde la fréquence choisie, reprise au prochain lancement. Mêmes
/// garanties (best-effort) que `save_bar_tint`.
pub fn save_app_refresh_rate_cap(hz: u32) {
    write_config("app_refresh_rate.txt", hz.to_string());
}

/// Supprime le fichier de préférence (voir `save_app_refresh_rate_cap`) —
/// utilisé quand l'utilisateur revient à "Automatique" : ce choix ne doit
/// pas être repris comme une valeur personnalisée au prochain lancement.
pub fn clear_app_refresh_rate_cap() {
    if let Some(path) = config_path("app_refresh_rate.txt") {
        let _ = fs::remove_file(path);
    }
}

/// Relit la couleur de fond propre à chaque barre (haut, gauche, droite —
/// voir `ui::dock::Dock::bar_color`), si elle a été personnalisée
/// individuellement (voir `win32::settings_dash`) — `None` PAR BARRE (pas
/// globalement) pour une barre qui suit encore la couleur partagée : encodé
/// par une composante alpha négative (voir `save_bar_colors`), une valeur
/// qu'aucun choix réel ne produit jamais (l'alpha reste toujours
/// `color::DOCK_ALPHA` en pratique). `None` global si %APPDATA% est
/// introuvable/illisible ou si le fichier ne contient pas les 12 valeurs
/// attendues (3 barres × RVBA) — `app::run` retombe alors sur `[None; 3]`
/// pour les trois (comportement d'origine, une seule couleur partagée).
pub fn load_bar_colors() -> Option<[Option<[f32; 4]>; 4]> {
    let floats = read_config_floats("bar_colors.txt")?;
    // ⚠️ **16 valeurs = `[haut, gauche, droite, bas]` ; 12 = un fichier écrit
    // AVANT la barre du bas.** On accepte les deux, la quatrième retombant sur
    // `None` (elle suit alors la couleur partagée) — aucune migration.
    if floats.len() != 12 && floats.len() != 16 {
        return None;
    }
    Some(std::array::from_fn(|i| {
        // Au-delà de ce que le fichier porte, la barre suit la couleur partagée.
        if i * 4 + 3 >= floats.len() {
            return None;
        }
        let [r, g, b, a] = [
            floats[i * 4],
            floats[i * 4 + 1],
            floats[i * 4 + 2],
            floats[i * 4 + 3],
        ];
        if a < 0.0 { None } else { Some([r, g, b, a]) }
    }))
}

/// Sauvegarde la couleur de fond de chacune des QUATRE barres, reprise au
/// prochain lancement — `None` (barre qui suit encore la couleur partagée)
/// encodé par une composante alpha négative (voir `load_bar_colors`). Échec
/// silencieux si %APPDATA% est inaccessible en écriture.
pub fn save_bar_colors(bar_colors: [Option<[f32; 4]>; 4]) {
    let floats: Vec<f32> = bar_colors
        .iter()
        .flat_map(|c| c.unwrap_or([0.0, 0.0, 0.0, -1.0]))
        .collect();
    let joined = floats.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
    write_config("bar_colors.txt", joined);
}

/// Relit la couleur PERSONNALISÉE des séparateurs d'icônes (voir `ui::dock::
/// Dock::separator_color`) — même encodage `None` par composante alpha
/// négative que `load_bar_colors` — DEMANDE EXPLICITE ("donnes la
/// possibilité de choisir [la] couleur [des séparateurs] ou de les rendre
/// transparent"). `None` = suit la couleur principale du texte de la barre
/// (`effective_topbar_text_color`, voir `app::run`) ; `Some([_, _, _, 0.0])`
/// = transparent (choix explicite, pas le même que "suit le texte").
pub fn load_separator_color() -> Option<[f32; 4]> {
    let floats = read_config_floats("separator_color.txt")?;
    if floats.len() != 4 {
        return None;
    }
    let [r, g, b, a] = [floats[0], floats[1], floats[2], floats[3]];
    if a < 0.0 { None } else { Some([r, g, b, a]) }
}

/// Sauvegarde la couleur des séparateurs d'icônes, reprise au prochain
/// lancement (voir `load_separator_color`). `None` (suit la couleur du
/// texte de la barre) encodé par une composante alpha négative.
pub fn save_separator_color(color: Option<[f32; 4]>) {
    let [r, g, b, a] = color.unwrap_or([0.0, 0.0, 0.0, -1.0]);
    write_config("separator_color.txt", format!("{r},{g},{b},{a}"));
}

/// Relit le diamètre PERSONNALISÉ du petit cercle indicateur des séparateurs
/// (voir `ui::dock::Dock::separator_size`) — DEMANDE EXPLICITE ("ajoute la
/// possibilité de modifier la taille des séparateurs par interface"). `None`
/// si jamais personnalisé : `ui::dock::SPACER_MARKER_DIAMETER` (14.0, valeur
/// d'origine), même convention que `load_icon_gap`.
/// Taille, forme et couleur de l'indicateur « en cours d'exécution » du dock
/// (voir `ui::dock::Dock::running_dot_instance`) — DEMANDE EXPLICITE : « donnes
/// moi la possibilité d'agrandir la taille de l'indicateur, de changer sa forme
/// et sa couleur dans le dash de réglages ». Même motif qu'`icon_gap` et
/// `separator_size` : un fichier, une valeur, et l'appelant choisit le défaut.
pub fn load_running_dot_size() -> Option<f32> {
    read_config_floats("running_dot_size.txt")?.first().copied()
}

pub fn save_running_dot_size(size: f32) {
    write_config("running_dot_size.txt", size.to_string());
}

/// Décalage horizontal de l'indicateur d'application ouverte (voir
/// `ui::dock::Dock::running_dot_offset`). `None` : jamais réglé, `0`.
pub fn load_running_dot_offset() -> Option<f32> {
    read_config_floats("running_dot_offset.txt")?.first().copied()
}

pub fn save_running_dot_offset(px: f32) {
    write_config("running_dot_offset.txt", px.to_string());
}

/// Marge autour des icônes des barres (voir `ui::dock::icon_margin`). `None` :
/// jamais réglée, `ui::dock::DEFAULT_ICON_MARGIN`.
pub fn load_icon_margin() -> Option<f32> {
    read_config_floats("icon_margin.txt")?.first().copied()
}

pub fn save_icon_margin(px: f32) {
    write_config("icon_margin.txt", px.to_string());
}

/// Hauteur du fondu en haut de la barre de gauche (voir
/// `ui::dock::fondu_haut_gauche`). `None` : jamais réglée, donc désactivé.
pub fn load_fondu_haut_gauche() -> Option<f32> {
    read_config_floats("fondu_haut_gauche.txt")?.first().copied()
}

pub fn save_fondu_haut_gauche(px: f32) {
    write_config("fondu_haut_gauche.txt", px.to_string());
}

/// Même chose pour la barre de droite (voir `ui::dock::fondu_haut_droite`).
pub fn load_fondu_haut_droite() -> Option<f32> {
    read_config_floats("fondu_haut_droite.txt")?.first().copied()
}

pub fn save_fondu_haut_droite(px: f32) {
    write_config("fondu_haut_droite.txt", px.to_string());
}

/// L'ombre sous la barre du haut (voir `ui::dock::ombre_barre_haut`) : sa hauteur
/// en pixels, puis sa force. `None` : jamais réglée, donc les défauts.
pub fn load_ombre_barre_haut() -> Option<f32> {
    read_config_floats("ombre_barre_haut.txt")?.first().copied()
}

pub fn save_ombre_barre_haut(px: f32) {
    write_config("ombre_barre_haut.txt", px.to_string());
}

pub fn load_ombre_barre_haut_force() -> Option<f32> {
    read_config_floats("ombre_barre_haut_force.txt")?.first().copied()
}

pub fn save_ombre_barre_haut_force(force: f32) {
    write_config("ombre_barre_haut_force.txt", force.to_string());
}

/// L'ombre des barres de gauche, de droite et du bas (voir
/// `win32::ombre_barre::ReglagesCotes`, qui porte le format).
pub fn load_ombre_cotes() -> Option<String> {
    fs::read_to_string(config_path("ombre_cotes.txt")?).ok()
}

pub fn save_ombre_cotes(reglages: &crate::win32::ombre_barre::ReglagesCotes) {
    write_config("ombre_cotes.txt", reglages.serialiser());
}

/// Le centre de la barre du haut (voir `win32::centre_haut`) — en %, rien à
/// convertir d'un écran à l'autre (`config_initiale`).
pub fn load_ombre_centre_haut() -> Option<String> {
    fs::read_to_string(config_path("ombre_centre_haut.txt")?).ok()
}

pub fn save_ombre_centre_haut(reglages: &crate::win32::centre_haut::ReglagesCentre) {
    write_config("ombre_centre_haut.txt", reglages.serialiser());
}

/// L'empilement des icônes en haut des barres latérales (voir `ui::empilement`).
pub fn load_empilement() -> Option<String> {
    fs::read_to_string(config_path("empilement.txt")?).ok()
}

pub fn save_empilement(reglages: &crate::ui::empilement::Empilement) {
    write_config("empilement.txt", reglages.serialiser());
}

/// Où tombe l'ombre NOIRE, barre par barre (voir `win32::ombre_barre::Portee`).
pub fn load_ombre_noire_portee() -> Option<String> {
    fs::read_to_string(config_path("ombre_noire_portee.txt")?).ok()
}

pub fn save_ombre_noire_portee(portee: &crate::win32::ombre_barre::Portee) {
    write_config("ombre_noire_portee.txt", portee.serialiser());
}

/// Les ombres des écrans secondaires réglés à part, une ligne `écran,réglages`
/// (voir `win32::ombre_barre::ReglagesEcran`) ; un écran absent suit le principal.
pub fn load_ombre_ecrans() -> Option<String> {
    fs::read_to_string(config_path("ombre_ecrans.txt")?).ok()
}

pub fn save_ombre_ecrans() {
    write_config("ombre_ecrans.txt", crate::win32::ombre_barre::serialiser_ecrans());
}

/// Les réglages de l'ombre COLORÉE (voir `win32::ombre_barre::ReglagesCouleur`) ;
/// `None` : pas encore séparés de ceux de la noire.
pub fn load_ombre_couleur() -> Option<String> {
    fs::read_to_string(config_path("ombre_couleur.txt")?).ok()
}

pub fn save_ombre_couleur(reglages: &crate::win32::ombre_barre::ReglagesCouleur) {
    write_config("ombre_couleur.txt", reglages.serialiser());
}

/// Échelle des icônes de la barre du haut (voir `ui::dock::top_icon_scale`).
/// `None` : jamais réglée, `ui::dock::DEFAULT_TOP_ICON_SCALE`.
pub fn load_top_icon_scale() -> Option<f32> {
    read_config_floats("top_icon_scale.txt")?.first().copied()
}

pub fn save_top_icon_scale(scale: f32) {
    write_config("top_icon_scale.txt", scale.to_string());
}

/// Échelle des icônes de la barre du bas (voir `ui::dock::bottom_icon_scale`).
pub fn load_bottom_icon_scale() -> Option<f32> {
    read_config_floats("bottom_icon_scale.txt")?.first().copied()
}

pub fn save_bottom_icon_scale(scale: f32) {
    write_config("bottom_icon_scale.txt", scale.to_string());
}

/// Échelle des icônes de la barre de gauche (voir `ui::dock::left_icon_scale`).
pub fn load_left_icon_scale() -> Option<f32> {
    read_config_floats("left_icon_scale.txt")?.first().copied()
}

pub fn save_left_icon_scale(scale: f32) {
    write_config("left_icon_scale.txt", scale.to_string());
}

/// Forme : `"circle"`, `"square"` ou `"bar"` (voir
/// `ui::dock::RunningDotShape::as_str`) — jamais traduite, elle vit dans un
/// fichier de configuration.
pub fn load_running_dot_shape() -> Option<String> {
    let path = config_path("running_dot_shape.txt")?;
    let value = fs::read_to_string(path).ok()?;
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

pub fn save_running_dot_shape(shape: &str) {
    write_config("running_dot_shape.txt", shape.to_string());
}

/// Couleur de l'indicateur — **MÊME convention que `separator_color`
/// exactement** : quatre flottants, et un alpha NÉGATIF code « automatique »
/// (ici : suivre l'accentuation du dock). Ne pas inventer une seconde
/// convention pour le même genre de réglage — c'est ce qui rend les fichiers de
/// configuration imprévisibles.
pub fn load_running_dot_color() -> Option<[f32; 4]> {
    let floats = read_config_floats("running_dot_color.txt")?;
    if floats.len() != 4 {
        return None;
    }
    let [r, g, b, a] = [floats[0], floats[1], floats[2], floats[3]];
    if a < 0.0 { None } else { Some([r, g, b, a]) }
}

pub fn save_running_dot_color(color: Option<[f32; 4]>) {
    let [r, g, b, a] = color.unwrap_or([0.0, 0.0, 0.0, -1.0]);
    write_config("running_dot_color.txt", format!("{r},{g},{b},{a}"));
}

/// Le dégradé et les trois curseurs de l'indicateur (voir
/// `ui::dock::indicateur_reglages`) : `dégradé (0/1),saturation,luminosité,
/// transparence`. `None` : jamais réglés.
pub fn load_indicateur_reglages() -> Option<(bool, [f32; 3])> {
    let v = read_config_floats("indicateur_couleur.txt")?;
    Some((v.first().is_some_and(|&d| d == 1.0), [v.get(1).copied()?, v.get(2).copied()?, v.get(3).copied()?]))
}

/// Verr. num / Verr. maj dans la barre du haut (voir `ui::verrous`) :
/// `affiché (0/1),r,g,b (num),r,g,b (maj)`. Absent ou incomplet : les défauts.
pub fn load_verrous() -> crate::ui::verrous::Reglages {
    let defaut = crate::ui::verrous::REGLAGES_PAR_DEFAUT;
    let Some(v) = read_config_floats("verrous.txt").filter(|v| v.len() >= 7) else { return defaut };
    let c = |i: usize| [v[i], v[i + 1], v[i + 2]].map(|x| x.clamp(0.0, 1.0));
    crate::ui::verrous::Reglages { affiche: v[0] == 1.0, couleurs: [c(1), c(4)] }
}

/// Le débit réseau dans la barre du haut (voir `ui::debit`) — affiché par défaut.
pub fn load_debit_affiche() -> bool {
    read_config_floats("debit_reseau.txt").and_then(|v| v.first().copied()).is_none_or(|v| v == 1.0)
}

pub fn save_debit_affiche(affiche: bool) {
    write_config("debit_reseau.txt", u8::from(affiche).to_string());
}

pub fn save_verrous(r: crate::ui::verrous::Reglages) {
    let [[a, b, c], [d, e, f]] = r.couleurs;
    write_config("verrous.txt", format!("{},{a},{b},{c},{d},{e},{f}", u8::from(r.affiche)));
}

/// Les décalages des horloges plein écran (voir `win32::fullscreen_clock::
/// decalage_de`) : `<écran>=<x>,<y>` par ligne.
pub fn load_fullscreen_clock_decalages() -> std::collections::BTreeMap<String, [i32; 2]> {
    let Some(texte) = config_path("fullscreen_clock_decalages.txt").and_then(|p| fs::read_to_string(p).ok()) else {
        return Default::default();
    };
    texte
        .lines()
        .filter_map(|l| {
            let (ecran, valeurs) = l.trim().split_once('=')?;
            let (x, y) = valeurs.split_once(',')?;
            Some((ecran.to_string(), [x.trim().parse().ok()?, y.trim().parse().ok()?]))
        })
        .collect()
}

pub fn save_fullscreen_clock_decalages(decalages: &std::collections::BTreeMap<String, [i32; 2]>) {
    write_config("fullscreen_clock_decalages.txt", decalages.iter().map(|(e, [x, y])| format!("{e}={x},{y}\n")).collect());
}

/// **LES ÉCRANS SECONDAIRES AUX COINS ARRONDIS** sous leur barre du haut (voir
/// `win32::coins_secondaires`) : un nom de périphérique par ligne. Absent : aucun.
pub fn load_coins_secondaires() -> std::collections::BTreeSet<String> {
    config_path("coins_secondaires.txt")
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|t| t.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

pub fn save_coins_secondaires(ecrans: &std::collections::BTreeSet<String>) {
    write_config("coins_secondaires.txt", ecrans.iter().map(|e| format!("{e}\n")).collect());
}

/// Le rayon des coins sous la barre de chaque écran secondaire, en pixels LOGIQUES
/// (`\\.\DISPLAY2=24` par ligne). Un écran absent suit le style (voir
/// `win32::coins_secondaires::rayon_pour`). Une ligne illisible tombe.
pub fn load_coins_secondaires_rayons() -> std::collections::BTreeMap<String, u32> {
    config_path("coins_secondaires_rayons.txt")
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|t| parse_coins_secondaires_rayons(&t))
        .unwrap_or_default()
}

fn parse_coins_secondaires_rayons(texte: &str) -> std::collections::BTreeMap<String, u32> {
    texte
        .lines()
        .filter_map(|l| l.trim().rsplit_once('='))
        .filter_map(|(ecran, px)| Some((ecran.trim().to_string(), px.trim().parse::<u32>().ok()?)))
        .filter(|(ecran, px)| !ecran.is_empty() && *px > 0)
        .collect()
}

pub fn save_coins_secondaires_rayons(rayons: &std::collections::BTreeMap<String, u32>) {
    write_config("coins_secondaires_rayons.txt", rayons.iter().map(|(e, px)| format!("{e}={px}\n")).collect());
}

#[cfg(test)]
mod coins_secondaires_rayons_tests {
    use super::parse_coins_secondaires_rayons;

    #[test]
    fn les_rayons_se_relisent_par_ecran() {
        let lu = parse_coins_secondaires_rayons("D2=24\nD3 = 8\nillisible\nD4=abc\nD5=0\n");
        assert_eq!(lu.len(), 2);
        assert_eq!(lu["D2"], 24);
        assert_eq!(lu["D3"], 8);
    }
}

/// Les couleurs des courbes du moniteur système (voir `cef_moniteur`) :
/// `<série>=#rrggbb` par ligne. Une série absente garde sa couleur par défaut.
pub fn load_moniteur_couleurs() -> std::collections::BTreeMap<String, String> {
    let Some(texte) = config_path("moniteur_couleurs.txt").and_then(|p| fs::read_to_string(p).ok()) else {
        return Default::default();
    };
    parse_moniteur_couleurs(&texte)
}

fn parse_moniteur_couleurs(texte: &str) -> std::collections::BTreeMap<String, String> {
    texte
        .lines()
        .filter_map(|l| l.trim().split_once('='))
        .filter(|(serie, couleur)| serie_de_moniteur_valide(serie) && couleur_hex_valide(couleur))
        .map(|(serie, couleur)| (serie.to_string(), couleur.to_ascii_lowercase()))
        .collect()
}

pub fn save_moniteur_couleurs(couleurs: &std::collections::BTreeMap<String, String>) {
    let texte: String = couleurs.iter().map(|(serie, couleur)| format!("{serie}={couleur}
")).collect();
    write_config("moniteur_couleurs.txt", texte);
}

/// Un nom de série : des lettres ASCII, au plus 32 — il devient une clé du fichier.
pub fn serie_de_moniteur_valide(serie: &str) -> bool {
    !serie.is_empty() && serie.len() <= 32 && serie.bytes().all(|b| b.is_ascii_alphabetic())
}

/// `#rrggbb`, rien d'autre : la valeur est réinjectée telle quelle dans la page.
pub fn couleur_hex_valide(couleur: &str) -> bool {
    couleur.len() == 7 && couleur.starts_with('#') && couleur[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn save_indicateur_reglages(degrade: bool, [s, l, t]: [f32; 3]) {
    write_config("indicateur_couleur.txt", format!("{},{s},{l},{t}", u8::from(degrade)));
}

/// Index de la PREMIÈRE épingle de la barre des tâches Windows (voir
/// `ui::dock::Dock::ensure_winpin_capacity`) — persisté pour que ces icônes
/// retrouvent EXACTEMENT leur index d'une session à l'autre.
///
/// **Pourquoi c'est nécessaire, et pas déductible** : `icon_order.txt` ne
/// contient que des index nus, et les épingles occupent toujours les index les
/// plus élevés — donc, à la relecture, rien ne distingue « le plus grand index
/// est une épingle » de « c'est un espace ». Sans cette base, `app::run` créait
/// des ESPACES jusqu'à cet index (des espaces fantômes, à la place des
/// épingles), repoussait les vraies épingles plus loin, et l'ordre enregistré
/// ne les décrivait plus : elles revenaient à leur place par défaut — DÉFAUT
/// RÉEL SIGNALÉ, « le programme que j'ai déplacé dans la barre de droite est
/// revenu à sa place initiale au lancement ».
pub fn load_winpin_base() -> Option<usize> {
    read_config_floats("winpin_base.txt")?.first().copied().map(|v| v as usize)
}

pub fn save_winpin_base(base: usize) {
    write_config("winpin_base.txt", base.to_string());
}

/// **LE NOMBRE D'ÉPINGLES DE LA BARRE DES TÂCHES** pour lequel `icon_order.txt` a
/// été écrit — voir `ui::dock::retirer_les_epingles_disparues`. `None` : écrit par
/// une version antérieure.
pub fn load_winpin_count() -> Option<usize> {
    read_config_floats("winpin_count.txt")?.first().copied().map(|v| v as usize)
}

pub fn save_winpin_count(nombre: usize) {
    write_config("winpin_count.txt", nombre.to_string());
}

/// **LA CAPACITÉ DU BLOC DES ÉPINGLES** — voir `ui::reserve_epingles`. Les ordres
/// persistés sont écrits POUR elle : les applets valent `PINNED_ICON_BASE +
/// capacité + k`. `None` = jamais réservé (les ordres suivent alors le nombre
/// d'épingles, comme avant).
pub fn load_pin_capacity() -> Option<usize> {
    read_config_floats("pin_capacity.txt")?.first().copied().map(|v| v as usize)
}

/// ⚠️ **RELUE POUR VÉRIFIER** : `write_config` jette ses erreurs, et une capacité
/// non écrite ferait relire au lancement suivant des ordres décalés pour une
/// capacité que le fichier ne dit pas.
pub fn save_pin_capacity(capacite: usize) -> bool {
    write_config("pin_capacity.txt", capacite.to_string());
    load_pin_capacity() == Some(capacite)
}

/// **UNE COPIE DE SAUVEGARDE, UNE SEULE FOIS** — `<nom>.<suffixe>`, jamais écrasée :
/// c'est l'état d'avant la première transformation qui vaut d'être gardé.
pub fn sauvegarder_une_fois(file_name: &str, suffixe: &str) {
    let (Some(source), Some(copie)) = (config_path(file_name), config_path(&format!("{file_name}.{suffixe}"))) else {
        return;
    };
    if source.exists() && !copie.exists() {
        if let Err(e) = fs::copy(&source, &copie) {
            crate::diag!("[config] sauvegarde de {file_name} impossible : {e}");
        }
    }
}

/// Taille (largeur, en pixels) des vignettes du panneau d'aperçus de fenêtres
/// (voir `win32::window_previews`) — DEMANDE EXPLICITE : « une taille ajustable
/// dans les réglages ». Même motif qu'`icon_gap`/`separator_size` : un fichier,
/// une valeur, et c'est l'appelant qui choisit le défaut.
pub fn load_preview_thumb_width() -> Option<f32> {
    read_config_floats("preview_thumb_width.txt")?.first().copied()
}

pub fn save_preview_thumb_width(width: f32) {
    write_config("preview_thumb_width.txt", width.to_string());
}

/// Relit la DURÉE du fondu d'apparition/disparition du panneau d'aperçus, en
/// millisecondes (voir `win32::window_previews::fade_ms`) — `None` tant que
/// l'utilisateur n'a jamais touché le curseur, auquel cas c'est
/// `DEFAULT_FADE_MS` qui s'applique (le défaut vit dans le module, pas ici :
/// même convention que `load_preview_thumb_width`).
pub fn load_preview_fade_ms() -> Option<f32> {
    read_config_floats("preview_fade_ms.txt")?.first().copied()
}

/// Sauvegarde la durée du fondu du panneau d'aperçus, reprise au prochain
/// lancement (voir `load_preview_fade_ms`).
pub fn save_preview_fade_ms(ms: f32) {
    write_config("preview_fade_ms.txt", ms.to_string());
}

// ---------------------------------------------------------------------------
// APERÇU D'UN BUREAU VIRTUEL (voir `win32::desktop_previews`) — trois réglages,
// chacun son fichier, même convention que les aperçus de fenêtres ci-dessus :
// ni défaut ni borne ICI, ils vivent dans le module, et c'est l'appelant qui
// choisit le défaut. Le bornage appartient aux setters du module
// (`set_preview_width`/`set_fade_ms`/`set_hover_delay_ms`).
// ---------------------------------------------------------------------------

/// Largeur de la miniature d'un bureau, en pixels (voir
/// `win32::desktop_previews::preview_width`).
pub fn load_desktop_preview_width() -> Option<f32> {
    read_config_floats("desktop_preview_width.txt")?.first().copied()
}

pub fn save_desktop_preview_width(width: f32) {
    write_config("desktop_preview_width.txt", width.to_string());
}

/// Durée du fondu de la miniature d'un bureau, en millisecondes (voir
/// `win32::desktop_previews::fade_ms`).
pub fn load_desktop_preview_fade_ms() -> Option<f32> {
    read_config_floats("desktop_preview_fade_ms.txt")?.first().copied()
}

pub fn save_desktop_preview_fade_ms(ms: f32) {
    write_config("desktop_preview_fade_ms.txt", ms.to_string());
}

/// Délai de survol avant l'apparition de la miniature d'un bureau, en
/// millisecondes (voir `win32::desktop_previews::hover_delay_ms`).
pub fn load_desktop_preview_hover_delay_ms() -> Option<f32> {
    read_config_floats("desktop_preview_hover_delay_ms.txt")?.first().copied()
}

pub fn save_desktop_preview_hover_delay_ms(ms: f32) {
    write_config("desktop_preview_hover_delay_ms.txt", ms.to_string());
}

// ---------------------------------------------------------------------------
// FOND ANIMÉ DU LOGO (voir `ui::dock::logo_background_color`) — trois réglages,
// chacun son fichier, même convention que ci-dessus : ni défaut ni borne ICI,
// ils vivent dans le module, et c'est l'appelant qui choisit le défaut.
//
// **UN FICHIER PAR VALEUR, ET C'EST DÉLIBÉRÉ** : la mémoire du projet porte le
// piège d'un champ AJOUTÉ à un format existant — `zone_heights.txt` a dû être
// séparé de `spacer_sizes.txt` parce qu'une ligne mal formée fait perdre TOUTES
// les valeurs de la ligne. Trois fichiers coûtent trois lectures au démarrage ;
// un format groupé coûte une régression silencieuse le jour où on l'étend.
// ---------------------------------------------------------------------------

/// Vrai si le fond du logo doit être animé (voir
/// `ui::dock::logo_animation_enabled`).
pub fn load_logo_animation_enabled() -> Option<bool> {
    read_config_floats("logo_animation_enabled.txt")?.first().map(|value| *value != 0.0)
}

pub fn save_logo_animation_enabled(enabled: bool) {
    write_config("logo_animation_enabled.txt", if enabled { "1" } else { "0" }.to_string());
}

/// Durée d'un aller de l'opacité, en millisecondes (voir
/// `ui::dock::logo_fade_ms`).
pub fn load_logo_fade_ms() -> Option<f32> {
    read_config_floats("logo_animation_fade_ms.txt")?.first().copied()
}

pub fn save_logo_fade_ms(ms: f32) {
    write_config("logo_animation_fade_ms.txt", ms.to_string());
}

/// Durée d'affichage de chaque couleur, en millisecondes (voir
/// `ui::dock::logo_color_ms`).
pub fn load_logo_color_ms() -> Option<f32> {
    read_config_floats("logo_animation_color_ms.txt")?.first().copied()
}

pub fn save_logo_color_ms(ms: f32) {
    write_config("logo_animation_color_ms.txt", ms.to_string());
}

// ---------------------------------------------------------------------------
// ICÔNE CIRCULAIRE QUI TOURNE (voir `ui::dock::logo_rotation`) — trois réglages
// de plus, un fichier chacun, même convention.
// ---------------------------------------------------------------------------

/// Vrai si le dock doit montrer l'icône CIRCULAIRE plutôt que la carrée (voir
/// `ui::dock::logo_is_circle`).
pub fn load_logo_is_circle() -> Option<bool> {
    read_config_floats("logo_circle.txt")?.first().map(|value| *value != 0.0)
}

pub fn save_logo_is_circle(circle: bool) {
    write_config("logo_circle.txt", if circle { "1" } else { "0" }.to_string());
}

/// Sens de rotation — `1` = horaire (voir `ui::dock::logo_spin_clockwise`).
pub fn load_logo_spin_clockwise() -> Option<bool> {
    read_config_floats("logo_spin_clockwise.txt")?.first().map(|value| *value != 0.0)
}

pub fn save_logo_spin_clockwise(clockwise: bool) {
    write_config("logo_spin_clockwise.txt", if clockwise { "1" } else { "0" }.to_string());
}

/// Durée d'un tour complet, en millisecondes (voir `ui::dock::logo_turn_ms`).
pub fn load_logo_turn_ms() -> Option<f32> {
    read_config_floats("logo_turn_ms.txt")?.first().copied()
}

pub fn save_logo_turn_ms(ms: f32) {
    write_config("logo_turn_ms.txt", ms.to_string());
}

// TAILLE ET ZOOM DU LOGO (voir `ui::dock::logo_draw_scale`) — un fichier par
// réglage : `logo_size.txt`, puis `logo_zoom.txt` = « actif,amplitude,durée ms,
// attente ms » (un seul ÉCRIVAIN, donc des virgules).
pub fn load_logo_size() -> Option<f32> {
    read_config_floats("logo_size.txt")?.first().copied()
}

/// La texture de l'interface (voir `gfx::texture_interface`), telle qu'écrite par
/// `serialiser` — relue par `charger`.
pub fn load_texture_interface() -> Option<String> {
    fs::read_to_string(config_path(crate::gfx::texture_interface::FICHIER)?).ok()
}

/// La texture des terminaux, même format (voir `gfx::texture_interface::TERMINAUX`).
pub fn load_texture_terminaux() -> Option<String> {
    fs::read_to_string(config_path(crate::gfx::texture_interface::FICHIER_TERMINAUX)?).ok()
}

pub fn save_texture_terminaux() {
    write_config(crate::gfx::texture_interface::FICHIER_TERMINAUX, crate::gfx::texture_interface::TERMINAUX.serialiser());
}

pub fn save_texture_interface() {
    write_config(crate::gfx::texture_interface::FICHIER, crate::gfx::texture_interface::serialiser());
}

/// La texture personnalisée (voir `gfx::texture_perso`), même convention.
pub fn load_texture_perso() -> Option<String> {
    fs::read_to_string(config_path(crate::gfx::texture_perso::FICHIER)?).ok()
}

pub fn save_texture_perso() {
    write_config(crate::gfx::texture_perso::FICHIER, crate::gfx::texture_perso::INTERFACE.serialiser());
}

/// Celle des terminaux (voir `gfx::texture_perso::TERMINAUX`), même convention.
pub fn load_texture_perso_terminaux() -> Option<String> {
    fs::read_to_string(config_path(crate::gfx::texture_perso::FICHIER_TERMINAUX)?).ok()
}

pub fn save_texture_perso_terminaux() {
    write_config(crate::gfx::texture_perso::FICHIER_TERMINAUX, crate::gfx::texture_perso::TERMINAUX.serialiser());
}

/// Le dossier de la bibliothèque des textures (voir `win32::textures_bibliotheque`)
/// — un SOUS-DOSSIER, comme `palettes_dir`.
pub(crate) fn textures_dir() -> Option<PathBuf> {
    let mut path = config_dir()?;
    path.push("textures");
    Some(path)
}

pub fn save_logo_size(size: f32) {
    write_config("logo_size.txt", size.to_string());
}

/// `(actif, amplitude, durée ms, attente ms)`, ou `None` si le fichier manque
/// ou est incomplet.
pub fn load_logo_zoom() -> Option<(bool, f32, f32, f32)> {
    match read_config_floats("logo_zoom.txt")?.as_slice() {
        [actif, amplitude, duree, attente, ..] => Some((*actif != 0.0, *amplitude, *duree, *attente)),
        _ => None,
    }
}

pub fn save_logo_zoom(enabled: bool, amplitude: f32, zoom_ms: f32, interval_ms: f32) {
    write_config("logo_zoom.txt", format!("{},{amplitude},{zoom_ms},{interval_ms}", if enabled { 1 } else { 0 }));
}

/// Relit la DURÉE du fondu d'apparition/disparition des MENUS, en
/// millisecondes (voir `win32::fade::menu_fade_ms`) — `None` tant que
/// l'utilisateur n'a jamais touché le curseur, auquel cas c'est
/// `DEFAULT_MENU_FADE_MS` qui s'applique (le défaut vit dans le module, pas
/// ici : même convention que `load_preview_fade_ms`).
///
/// **Fichier SÉPARÉ de celui des infobulles** (`tooltip_fade_ms.txt`) : ce sont
/// deux réglages indépendants, et un format à deux champs dans un même fichier
/// ferait perdre l'autre valeur à la première ligne mal formée (même raison que
/// `zone_heights.txt` face à `spacer_sizes.txt`).
pub fn load_menu_fade_ms() -> Option<f32> {
    read_config_floats("menu_fade_ms.txt")?.first().copied()
}

/// Sauvegarde la durée du fondu des menus, reprise au prochain lancement.
pub fn save_menu_fade_ms(ms: f32) {
    write_config("menu_fade_ms.txt", ms.to_string());
}

/// Relit la DURÉE du fondu d'apparition/disparition des INFOBULLES, en
/// millisecondes (voir `win32::fade::tooltip_fade_ms`) — voir
/// `load_menu_fade_ms` pour la convention de défaut et le pourquoi d'un
/// fichier séparé.
pub fn load_tooltip_fade_ms() -> Option<f32> {
    read_config_floats("tooltip_fade_ms.txt")?.first().copied()
}

/// Sauvegarde la durée du fondu des infobulles, reprise au prochain lancement.
pub fn save_tooltip_fade_ms(ms: f32) {
    write_config("tooltip_fade_ms.txt", ms.to_string());
}

pub fn load_separator_size() -> Option<f32> {
    read_config_floats("separator_size.txt")?.first().copied()
}

/// Sauvegarde le diamètre du petit cercle indicateur des séparateurs, repris
/// au prochain lancement (voir `load_separator_size`).
pub fn save_separator_size(size: f32) {
    write_config("separator_size.txt", size.to_string());
}

/// Relit le facteur de magnification au survol des icônes (voir
/// `ui::dock::hover_scale`) — `None` tant que l'utilisateur n'a jamais touché
/// le curseur, auquel cas c'est `ui::dock::DEFAULT_HOVER_SCALE` qui s'applique
/// (le défaut vit dans `ui::dock`, pas ici : c'est l'appelant qui choisit,
/// même convention que `load_preview_thumb_width`).
pub fn load_hover_scale() -> Option<f32> {
    read_config_floats("hover_scale.txt")?.first().copied()
}

/// Sauvegarde le facteur de magnification au survol, repris au prochain
/// lancement (voir `load_hover_scale`). La valeur écrite est celle DÉJÀ BORNÉE
/// par `ui::dock::set_hover_scale`, jamais celle reçue du panneau — comme
/// `save_separator_size`.
pub fn save_hover_scale(scale: f32) {
    write_config("hover_scale.txt", scale.to_string());
}

/// **LE TIRAGE DES ICÔNES AGRANDIES** (voir `ui::dock::DEFAULT_TIRAGE`, qui porte la
/// demande et ses trois temps) — `None` tant que l'utilisateur n'y a jamais touché,
/// et c'est `ui::dock` qui décide du défaut. Même discipline que `load_hover_scale`
/// juste au-dessus : un flottant, et la valeur écrite est celle DÉJÀ BORNÉE par
/// `ui::dock::set_tirage_vers_le_centre`, jamais celle reçue du panneau.
/// ⚠️ **UN SEUL FICHIER POUR LES QUATRE BARS**, dans l'ordre de
/// `ui::dock::BarreTirage::TOUTES` (haut, gauche, droite, bas) : des réglages de
/// même nature, lus et écrits ensemble, ne justifient pas quatre fichiers — et un
/// fichier unique ne peut pas se retrouver à moitié écrit.
///
/// ⚠️ **LE BAS EST EN FIN, ET LA LECTURE EST UN `get(rang())`** : un fichier écrit
/// avant la barre du bas n'en porte que trois, et `load_tirage_magnification`
/// rend `None` pour le bas — donc son défaut (`DEFAULT_TIRAGE`). Aucune migration.
pub fn load_tirage_magnification(barre: crate::ui::dock::BarreTirage) -> Option<f32> {
    let content = fs::read_to_string(config_path("tirage_magnification.txt")?).ok()?;
    parse_tirages(&content).get(barre.rang()).copied()
}

/// ⚠️ **LE MÊME DÉFAUT QUE `save_scroll_steps`, UNE SECONDE FOIS** — DÉFAUT RÉEL
/// SIGNALÉ (2026-09-25) : « mes réglages d'animation de survol ne sont pas
/// sauvegardés ». `save_tirage_magnifications` écrivait une valeur par LIGNE, le
/// lecteur attendait des VIRGULES : le fichier se relisait VIDE et les trois
/// curseurs retombaient à 100 % à chaque lancement. L'écrivain passe désormais
/// par `format_config_floats` ; les LIGNES restent lues ici pour rendre à
/// l'utilisateur les valeurs déjà enregistrées dans l'ancien format.
fn parse_tirages(content: &str) -> Vec<f32> {
    parse_config_floats(&content.replace(['\r', '\n'], ","))
}

/// Sauvegarde les quatre tirages, repris au prochain lancement. ⚠️ On écrit les QUATRE
/// à chaque fois : le fichier est une liste, donc n'en écrire qu'un laisserait les
/// autres à leur position précédente — ce qui marcherait par accident, et
/// s'effondrerait le jour où l'ordre change.
pub fn save_tirage_magnifications(tirages: &[f32; 4]) {
    write_config("tirage_magnification.txt", format_config_floats(tirages));
}

/// Relit la vitesse d'approche de la magnification au survol (voir
/// `ui::dock::hover_ease_speed`) — même convention que `load_hover_scale`.
pub fn load_hover_ease_speed() -> Option<f32> {
    read_config_floats("hover_ease_speed.txt")?.first().copied()
}

/// Sauvegarde la vitesse d'approche de la magnification au survol, reprise au
/// prochain lancement (voir `load_hover_ease_speed`).
pub fn save_hover_ease_speed(speed: f32) {
    write_config("hover_ease_speed.txt", speed.to_string());
}

/// Relit le facteur de magnification au survol du TEXTE des horloges (voir
/// `ui::dock::clock_hover_scale`) — réglage DISTINCT de celui des icônes, et
/// même convention que `load_hover_scale` : `None` tant que l'utilisateur n'a
/// jamais touché le curseur, auquel cas c'est
/// `ui::dock::DEFAULT_CLOCK_HOVER_SCALE` qui s'applique.
pub fn load_clock_hover_scale() -> Option<f32> {
    read_config_floats("clock_hover_scale.txt")?.first().copied()
}

/// Sauvegarde le facteur de magnification au survol des horloges, repris au
/// prochain lancement (voir `load_clock_hover_scale`). La valeur écrite est
/// celle DÉJÀ BORNÉE par `ui::dock::set_clock_hover_scale`, jamais celle reçue
/// du panneau — comme `save_hover_scale`.
pub fn save_clock_hover_scale(scale: f32) {
    write_config("clock_hover_scale.txt", scale.to_string());
}

/// Relit le multiplicateur de TRANSPARENCE des éléments des panneaux (voir
/// `win32::cef_popup::popup_alpha`) — même convention que `load_hover_scale` :
/// `None` tant que l'utilisateur n'a jamais touché le curseur, auquel cas c'est
/// `cef_popup::DEFAULT_POPUP_ALPHA` qui s'applique.
pub fn load_popup_alpha() -> Option<f32> {
    read_config_floats("popup_alpha.txt")?.first().copied()
}

/// Sauvegarde le multiplicateur de transparence, repris au prochain lancement
/// (voir `load_popup_alpha`). La valeur écrite est celle DÉJÀ BORNÉE par
/// `cef_popup::set_popup_alpha`, jamais celle reçue du panneau — comme
/// `save_hover_scale`.
pub fn save_popup_alpha(alpha: f32) {
    write_config("popup_alpha.txt", alpha.to_string());
}

/// Fond des zones (voir `win32::cef_popup::POPUP_FOND_ALPHA`) — mêmes
/// conventions que `load_popup_alpha` / `save_popup_alpha`.
pub fn load_popup_fond_alpha() -> Option<f32> {
    read_config_floats("popup_fond_alpha.txt")?.first().copied()
}

pub fn save_popup_fond_alpha(alpha: f32) {
    write_config("popup_fond_alpha.txt", alpha.to_string());
}

/// Relit la banque de couleurs personnalisées enregistrées par
/// l'utilisateur (voir `win32::settings_dash`, "Mes couleurs" — partagée par
/// les quatre sélecteurs du panneau de réglages, une seule banque plutôt
/// qu'une par cible). Liste vide si jamais rien n'a été enregistré, ou si
/// %APPDATA% est introuvable/illisible.
pub fn load_custom_colors() -> Vec<[f32; 3]> {
    let Some(floats) = read_config_floats("custom_colors.txt") else { return Vec::new() };
    floats.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect()
}

/// Sauvegarde la banque complète de couleurs personnalisées, reprise au
/// prochain lancement. Échec silencieux si %APPDATA% est inaccessible en
/// écriture.
pub fn save_custom_colors(colors: &[[f32; 3]]) {
    let floats: Vec<f32> = colors.iter().flat_map(|c| *c).collect();
    let joined = floats.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
    write_config("custom_colors.txt", joined);
}

/// Relit la banque de thèmes ALÉATOIRES enregistrés par l'utilisateur (voir
/// `win32::settings_dash::render_themes_section`, `ui::color::
/// random_dark_theme`) — fichier SÉPARÉ de `custom_colors.txt` : ce sont des
/// thèmes complets (appliqués PARTOUT en un clic, voir `apply_theme`), pas
/// des couleurs de secours pour un seul sélecteur individuel. Liste vide si
/// rien n'a jamais été enregistré, ou si %APPDATA% est introuvable/illisible.
pub fn load_custom_themes() -> Vec<[f32; 3]> {
    let Some(floats) = read_config_floats("custom_themes.txt") else { return Vec::new() };
    floats.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect()
}

/// Sauvegarde la banque complète de thèmes aléatoires enregistrés, reprise
/// au prochain lancement. Mêmes garanties (best-effort) que
/// `save_custom_colors`.
pub fn save_custom_themes(themes: &[[f32; 3]]) {
    let floats: Vec<f32> = themes.iter().flat_map(|c| *c).collect();
    let joined = floats.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
    write_config("custom_themes.txt", joined);
}

/// Relit la dernière "couleur de base" choisie dans le sélecteur de
/// `win32::settings_dash::render_themes_section` (voir `ui::color::
/// dark_theme_from_base_color`, demande explicite : "choisir la couleur de
/// base du thème depuis un sélecteur de couleur") — `None` si jamais
/// personnalisée : le panneau retombe alors sur une couleur de repli neutre.
/// Simple confort (garder le sélecteur sur le dernier choix d'une session à
/// l'autre), PAS le thème lui-même : la couleur de base n'est PAS un thème
/// appliqué, voir `pending_random_theme`.
pub fn load_theme_base_color() -> Option<[f32; 3]> {
    match read_config_floats("theme_base_color.txt")?[..] {
        [r, g, b] => Some([r, g, b]),
        _ => None,
    }
}

/// Sauvegarde la "couleur de base" du sélecteur de thèmes, reprise au
/// prochain lancement. Mêmes garanties (best-effort) que
/// `save_custom_colors`.
pub fn save_theme_base_color(color: [f32; 3]) {
    write_config("theme_base_color.txt", format!("{},{},{}", color[0], color[1], color[2]));
}

/// Relit la couleur des éléments (horloge, glyphes, titre média...) de la
/// barre du haut, si elle a été personnalisée (voir `win32::menu`,
/// `gfx::quad_renderer::QuadInstance::new_glyph`). `None` sinon : `app::run`
/// retombe alors sur du blanc, la couleur d'origine avant ce choix.
pub fn load_topbar_text_color() -> Option<[f32; 3]> {
    match read_config_floats("topbar_text_color.txt")?[..] {
        [r, g, b] => Some([r, g, b]),
        _ => None,
    }
}

/// Sauvegarde la couleur des éléments de la barre du haut, reprise au
/// prochain lancement. Mêmes garanties (best-effort) que
/// `save_background_color`.
pub fn save_topbar_text_color(color: [f32; 3]) {
    write_config("topbar_text_color.txt", format!("{},{},{}", color[0], color[1], color[2]));
}

/// Système de colorisation AUTOMATIQUE (voir `ui::color::adaptive_zones`,
/// `win32::egui_overlay::apply_adaptive_theme`/`apply_neutral_theme`) : quand
/// actif (par défaut), le texte ET les fonds de zone (boutons, cases,
/// curseurs...) de chaque fenêtre `egui` s'adaptent automatiquement à sa
/// couleur de mise en avant, ET la couleur du texte de la barre du haut suit
/// automatiquement la couleur de la barre du haut elle-même (au lieu d'être
/// choisie séparément) — demande explicite : "en fonction de la couleur pour
/// une barre ou un dash, régler automatiquement les couleurs des autres
/// éléments lui correspondant". Une case à cocher dans le panneau de
/// réglages (voir `win32::settings_dash::build_ui`) permet de tout
/// désactiver d'un coup pour reprendre la main manuellement (chaque couleur
/// reste réglable individuellement dans ce cas, comme avant ce système).
/// `true` par défaut si jamais personnalisé : comportement introduit dans
/// cette même session, actif tant qu'on ne l'a pas explicitement désactivé.
pub fn load_auto_color_scheme() -> bool {
    read_config_floats("auto_color_scheme.txt").and_then(|v| v.first().copied()).map(|v| v != 0.0).unwrap_or(true)
}

/// Sauvegarde l'état de la case "Colorisation automatique", repris au
/// prochain lancement. Mêmes garanties (best-effort) que
/// `save_topbar_text_color`.
pub fn save_auto_color_scheme(enabled: bool) {
    write_config("auto_color_scheme.txt", if enabled { "1" } else { "0" }.to_string());
}

/// Une fonctionnalité "pluggable" (icône de la barre du haut ou dash)
/// individuellement activée/désactivée — demande explicite : "gérer les
/// icones de la barre supérieure et les dashs comme des plugins activables
/// ou désactivables". `name` identifie la fonctionnalité (`"power"`/
/// `"terminal"`/`"sound"`/`"network"`/`"media"`/
/// `"clock"`/`"desktop_dots"`/`"dash"`/`"clipboard_viewer"`/`"notes"` — voir `app::run`,
/// seuls appelants) — un fichier par fonctionnalité (`feature_{name}.txt`),
/// même convention qu'ailleurs dans ce module. `true` par défaut (jamais
/// désactivée tant que l'utilisateur n'a rien décidé) : "cacher l'icône
/// seulement" (choix explicite) — désactiver n'arrête PAS le sondage/
/// l'abonnement en arrière-plan associé, seule l'icône (et, pour "dash"/
/// "clipboard_viewer", la fenêtre associée) cesse de s'afficher/réagir aux
/// clics. Le panneau de réglages LUI-MÊME (et son icône) n'a PAS
/// d'équivalent ici, volontairement : sans lui, plus aucun moyen de
/// réactiver quoi que ce soit depuis l'interface.
pub fn load_feature_enabled(name: &str) -> bool {
    read_config_floats(&format!("feature_{name}.txt")).and_then(|v| v.first().copied()).map(|v| v != 0.0).unwrap_or(true)
}

/// Sauvegarde l'état d'une fonctionnalité (voir `load_feature_enabled`),
/// repris au prochain lancement. Mêmes garanties (best-effort) que
/// `save_topbar_text_color`.
pub fn save_feature_enabled(name: &str, enabled: bool) {
    write_config(&format!("feature_{name}.txt"), if enabled { "1" } else { "0" }.to_string());
}

/// Réglage booléen générique, `false` par défaut tant que rien n'a jamais
/// été enregistré (contrairement à `load_feature_enabled` ci-dessus, dont
/// le défaut `true` ne conviendrait pas ici) — DEMANDE EXPLICITE ("donnes la
/// possibilité à l'utilisateur de masquer les barres droite et gauche ou de
/// rendre les barres... masquable automatiquement") : sert pour
/// `"left_bar_hidden"`/`"right_bar_hidden"` (masquage manuel PERMANENT des
/// barres latérales de l'interface PRIMAIRE, voir `app::run`) et
/// `"secondary_auto_hide"` (masquage automatique façon barre des tâches
/// Windows des écrans secondaires, voir `app::effective_primary_thicknesses`
/// pour l'équivalent primaire, `top_auto_hide`/`left_auto_hide`/
/// `right_auto_hide` via `load_bool_setting_or` ci-dessous), mais
/// volontairement générique (`name` arbitraire) pour ne pas dupliquer
/// ce boilerplate à chaque nouveau réglage booléen simple de ce genre. Un
/// fichier par réglage (`{name}.txt`), même convention que `load_feature_enabled`.
/// Nom du préréglage du dégradé animé (voir `gfx::degrade::PRESETS`) — le
/// NOM et non l'index : un préréglage ajouté ou réordonné ne change pas celui
/// de l'utilisateur.
pub fn load_degrade_preset() -> Option<String> {
    let nom = fs::read_to_string(config_path("degrade_preset.txt")?).ok()?;
    Some(nom.trim().to_string())
}

pub fn save_degrade_preset(nom: &str) {
    write_config("degrade_preset.txt", nom.to_string());
}

/// Mode du dégradé (voir `gfx::degrade::MODES`) — par NOM, même raison que
/// `load_degrade_preset`.
pub fn load_degrade_mode() -> Option<String> {
    let nom = fs::read_to_string(config_path("degrade_mode.txt")?).ok()?;
    Some(nom.trim().to_string())
}

pub fn save_degrade_mode(nom: &str) {
    write_config("degrade_mode.txt", nom.to_string());
}

/// **OÙ LE BANDEAU DE NOTIFICATION SE POSE** — voir `win32::bandeau::Position`,
/// qui porte les trois noms et le défaut.
///
/// ⚠️ **UNE CHAÎNE, PAS UN INDICE.** Comme `degrade_mode`, elle se lit à l'œil
/// dans `%APPDATA%` — un fichier qui dit « horloge » ne demande aucune table de
/// correspondance à qui l'ouvre. Et surtout **il n'y a pas de sentinelle** : un
/// `0` qui voudrait dire « désactivé » est exactement ce qui a DÉTRUIT une zone
/// (`zones.toml`, cote nulle) — ici, `0` serait « à gauche », une position
/// légitime.
///
/// **L'ABSENCE VAUT LE DÉFAUT**, qui est l'ancien comportement.
pub fn load_position_bandeau() -> Option<String> {
    let nom = fs::read_to_string(config_path("position_bandeau.txt")?).ok()?;
    Some(nom.trim().to_string())
}

pub fn save_position_bandeau(nom: &str) {
    write_config("position_bandeau.txt", nom.to_string());
}

/// Couleurs « Personnalisé » du dégradé : quinze flottants r,g,b (0..1).
pub fn load_degrade_couleurs() -> Option<Vec<f32>> {
    read_config_floats("degrade_couleurs.txt")
}

pub fn save_degrade_couleurs(values: &[f32]) {
    write_config("degrade_couleurs.txt", format_config_floats(values));
}

/// Curseurs du dégradé, dans l'ordre de `gfx::degrade::REGLAGES`.
pub fn load_degrade_reglages() -> Option<Vec<f32>> {
    read_config_floats("degrade_reglages.txt")
}

pub fn save_degrade_reglages(values: &[f32]) {
    write_config("degrade_reglages.txt", format_config_floats(values));
}

/// Drapeau (`load_bool_setting`) : le Terminal a déjà été déplacé dans la zone
/// Intégrés (voir `app::run`) — la migration ne se fait qu'une fois.
pub const TERMINAL_INTEGRES: &str = "terminal_integres";

/// Drapeau (`load_bool_setting`) : le sélecteur de couleurs a déjà été RAMENÉ
/// dans la zone Intégrés de la barre de droite (voir `app::run`).
///
/// ⚠️ **POURQUOI UN DRAPEAU, ET PAS UN SIMPLE `deja_pose`** — c'est un défaut
/// MESURÉ, pas une précaution : `deja_pose` cherche l'index dans les quatre
/// listes, et un index tout neuf y est presque toujours DÉJÀ cité, comme
/// **remplisseur**. `icon_order.txt` citait `22` (le premier emplacement de
/// remplisseur, celui que `ensure_spacer_capacity` garnit avant les épingles) :
/// l'applet a donc hérité de cette place — dans une autre barre que celle
/// demandée — et la migration n'a jamais tourné. Un drapeau dit la seule chose
/// qui compte : « la migration a-t-elle EU LIEU », pas « l'index est-il cité ».
pub const COULEURS_INTEGRES: &str = "couleurs_integres";

/// Drapeaux (`load_bool_setting`) : les fonctionnalités facultatives ont été
/// posées dans la zone Réglages, et les notifications dans la zone Intégrés, de
/// la barre de droite (voir `app::run`). Même raison que `COULEURS_INTEGRES` —
/// et même défaut, REFAIT le 2026-10-04 avant d'être réparé.
pub const FONCTIONNALITES_REGLAGES: &str = "fonctionnalites_reglages";
pub const NOTIFICATIONS_INTEGRES: &str = "notifications_integres";
/// Drapeau (`load_bool_setting`) : la Recherche a été posée dans la zone Intégrés
/// (voir `app::run`) — même raison que les deux ci-dessus.
pub const RECHERCHE_INTEGRES: &str = "recherche_integres";
/// Drapeau (`load_bool_setting`) : les Automatisations ont été posées dans la
/// zone Intégrés (voir `app::run`) — même raison que les trois ci-dessus.
///
/// ⛔ **UN `deja_pose` NE SUFFIRAIT PAS ICI** (voir `ceder_la_place`) : un index
/// d'applet tout neuf est presque toujours DÉJÀ cité par `icon_order.txt` comme
/// REMPLISSEUR, et la garde conclurait « déjà posé » sans jamais poser.
///
/// ⚠️ **ELLE ÉTAIT D'ABORD POSÉE DANS RÉGLAGES** (`automatisations_reglages`) —
/// DÉFAUT SIGNALÉ (2026-10-16) : « elle devrait être dans la zone intégrés de la
/// barre de droite, c'est sa place par défaut ». Le drapeau change de NOM avec la
/// zone : une configuration qui l'avait reçue dans Réglages la voit déplacée UNE
/// fois, puis l'utilisateur en fait ce qu'il veut.
pub const AUTOMATISATIONS_INTEGRES: &str = "automatisations_integres";
/// Drapeau (`load_bool_setting`) : la Météo a été posée dans la zone Intégrés
/// (voir `app::run`) — DEMANDE EXPLICITE : « une nouvelle applet dans la barre de
/// droite dans la zone Intégrés ».
///
/// ⛔ **ET LE MÊME PIÈGE QUE CI-DESSUS, POUR LA MÊME RAISON** : un index d'applet
/// tout neuf est presque toujours déjà cité comme remplisseur dans
/// `icon_order.txt`, donc un `deja_pose` conclurait « déjà posé » sans jamais
/// poser. Le drapeau à usage unique est ce qui fait partir le bloc UNE fois.
pub const METEO_INTEGRES: &str = "meteo_integres";
/// Drapeau (`load_bool_setting`) : l'applet « Aide » a été posée dans la zone
/// Intégrés de la barre de droite (voir `app::run`) — DEMANDE EXPLICITE du
/// 2026-10-07 : « une applet d'aide dans la zone Intégrés qui affiche un dash avec
/// les explications sur l'utilisation de l'app ».
///
/// ⚠️ **LE FICHIER `aide_integres.txt` EST LA PREUVE QUE LE BLOC A TOURNÉ**, et
/// elle se lit SANS regarder l'écran — même vérification que `verrouillage`.
pub const AIDE_INTEGRES: &str = "aide_integres";
/// Drapeau (`load_bool_setting`) : l'applet « Verrouiller la session » a été posée
/// dans la zone Réglages de la barre de droite (voir `app::run`) — DEMANDE
/// EXPLICITE (2026-10-05) : « une applet dans Réglages pour verrouiller la
/// session ».
///
/// ⛔ **ET LE MÊME PIÈGE QUE CI-DESSUS, POUR LA MÊME RAISON** : un index d'applet
/// tout neuf est presque toujours déjà cité comme remplisseur dans
/// `icon_order.txt`, donc un `deja_pose` conclurait « déjà posé » sans jamais
/// poser.
pub const VERROUILLAGE_REGLAGES: &str = "verrouillage_reglages";
/// Drapeau (`load_bool_setting`) : l'applet « Caractères » a été posée dans la zone
/// **Intégrés** de la barre de droite (voir `app::run`) — DEMANDE EXPLICITE
/// (2026-10-06) : « dans les intégrés j'aimerais avoir une applet qui permette de
/// faire apparaître une fenêtre flottante pour insérer les caractères accentués ».
///
/// ⛔ **ET LE MÊME PIÈGE QUE CI-DESSUS, POUR LA MÊME RAISON** : un index d'applet
/// tout neuf est presque toujours déjà cité comme remplisseur dans
/// `icon_order.txt`, donc un `deja_pose` conclurait « déjà posé » sans jamais poser.
pub const CARACTERES_INTEGRES: &str = "caracteres_integres";

/// **LE MÊME DRAPEAU POUR L'APPLET « KAOMOJIS »** — posée par défaut dans la zone
/// Intégrés de la barre de droite à la première exécution qui la découvre, puis
/// l'utilisateur en fait ce qu'il veut (voir `app::run`, bloc `deja_pose`).
///
/// ⛔ **ET LE MÊME PIÈGE QUE `CARACTERES_INTEGRES`** : un index d'applet tout neuf est
/// presque toujours déjà cité comme remplisseur dans `icon_order.txt`, donc un
/// `deja_pose` conclurait « déjà posé » sans jamais poser.
pub const KAOMOJI_INTEGRES: &str = "kaomoji_integres";

/// Drapeau (`load_bool_setting`) : l'applet « Propriétés système » a été posée
/// dans la zone **Réglages** de la barre de droite (voir `app::run`) — DEMANDE
/// EXPLICITE (2026-10-08) : « une nouvelle applet dans Intégrés pour ouvrir
/// sysdm.cpl », **la zone ayant été corrigée le même jour** (« en fait déplace la
/// nouvelle applet dans Réglages »).
///
/// ⛔ **ET LE MÊME PIÈGE QUE `VERROUILLAGE_REGLAGES`** : un index d'applet tout neuf
/// est presque toujours déjà cité comme remplisseur dans `icon_order.txt`, donc un
/// `deja_pose` conclurait « déjà posé » sans jamais poser. Le drapeau à usage
/// unique est ce qui fait partir le bloc UNE fois.
///
/// ⚠️ **LE FICHIER `proprietes_systeme_reglages.txt` EST LA PREUVE QUE LE BLOC A
/// TOURNÉ**, et elle se lit SANS regarder l'écran — même vérification que
/// `aide_integres` et `verrouillage_reglages`.
///
/// ⚠️ **LE NOM PORTE LA ZONE, ET IL CHANGE AVEC ELLE** (voir `FONCTIONNALITES_REGLAGES`
/// / `NOTIFICATIONS_INTEGRES` / `AUTOMATISATIONS_INTEGRES`) : c'est ce nom qui dit,
/// à la relecture, où l'applet a été POSÉE — pas où elle est aujourd'hui, puisque
/// l'utilisateur peut la déplacer à la main.
pub const PROPRIETES_SYSTEME_REGLAGES: &str = "proprietes_systeme_reglages";

/// Drapeau (`load_bool_setting`) : l'applet « Scanner » a été posée dans la zone
/// **Intégrés** de la barre de droite (voir `app::run`, bloc `deja_pose`).
///
/// ⛔ **ET LE MÊME PIÈGE QUE LES AUTRES** : un index d'applet tout neuf est presque
/// toujours déjà cité comme remplisseur dans `icon_order.txt`, donc un `deja_pose`
/// conclurait « déjà posé » sans jamais poser.
///
/// ⚠️ **LE FICHIER `scanner_integres.txt` EST LA PREUVE QUE LE BLOC A TOURNÉ**, et
/// elle se lit sans regarder l'écran.
pub const SCANNER_INTEGRES: &str = "scanner_integres";

/// **DRAPEAU DE MIGRATION DE L'ÉDITEUR DE SHADERS** — posé par `app::run` la
/// première fois qu'il range l'applet dans Intégrés, juste après le scanner. Même
/// règle que `SCANNER_INTEGRES` : le fichier `shaders_integres.txt` est la preuve
/// que le bloc a tourné.
pub const SHADERS_INTEGRES: &str = "shaders_integres";

/// **DRAPEAU DE MIGRATION DES SYMBOLES PHONÉTIQUES** — posé par `app::run` la
/// première fois qu'il range l'applet dans Intégrés, juste après l'éditeur de
/// shaders. Même règle que `SHADERS_INTEGRES`.
pub const PHONETIQUE_INTEGRES: &str = "phonetique_integres";

/// **DRAPEAU DE MIGRATION DES SYMBOLES MATHÉMATIQUES** — posé par `app::run` la
/// première fois qu'il range l'applet dans Intégrés, juste après les symboles
/// phonétiques. Même règle que `SHADERS_INTEGRES`.
pub const MATHS_INTEGRES: &str = "maths_integres";

/// **DRAPEAU DE MIGRATION DE L'ÉDITEUR DES LUEURS** — posé par `app::run` la
/// première fois qu'il range l'applet dans Intégrés, juste après les symboles
/// mathématiques. Même règle que `SHADERS_INTEGRES`.
pub const LUEURS_INTEGRES: &str = "lueurs_integres";

/// **DRAPEAU DE MIGRATION DE L'ÉDITEUR DES TEXTURES** — posé par `app::run` la
/// première fois qu'il range l'applet dans Intégrés, juste après l'éditeur des
/// lueurs. Même règle que `SHADERS_INTEGRES`.
pub const TEXTURES_INTEGRES: &str = "textures_integres";

/// **DRAPEAU DE MIGRATION DU NARRATEUR** — posé par `app::run` la première fois
/// qu'il range l'applet dans Réglages, juste après les propriétés système. Le nom
/// porte la ZONE, comme `PROPRIETES_SYSTEME_REGLAGES`.
pub const NARRATEUR_REGLAGES: &str = "narrateur_reglages";

/// **DRAPEAU DE MIGRATION DE LA LOUPE** — posé par `app::run` la première fois
/// qu'il range l'applet dans Réglages, juste après le narrateur. Le nom porte la
/// ZONE, comme `NARRATEUR_REGLAGES`.
pub const LOUPE_REGLAGES: &str = "loupe_reglages";
/// **DRAPEAU DE MIGRATION DU CLAVIER VISUEL** posé par `app::run` la première
/// fois qu'il range l'applet dans Réglages, juste après la loupe. Le nom porte
/// la ZONE, comme `LOUPE_REGLAGES`.
pub const CLAVIER_REGLAGES: &str = "clavier_reglages";
/// **DRAPEAU DE MIGRATION DE 2048** posé par `app::run` la première fois qu'il
/// range l'applet dans Intégrés, juste après le moniteur. Le nom porte la ZONE.
pub const JEU2048_INTEGRES: &str = "jeu2048_integres";
/// **DRAPEAU DE MIGRATION DE « CINQ OU PLUS »** posé par `app::run` la première
/// fois qu'il range l'applet dans Intégrés, juste après 2048. Le nom porte la ZONE.
pub const CINQ_OU_PLUS_INTEGRES: &str = "cinq_ou_plus_integres";
/// **DRAPEAU DE MIGRATION DE « QUADRAPASSEL »** posé par `app::run` la première
/// fois qu'il range l'applet dans Intégrés, juste après « Cinq ou plus ». Le nom porte la ZONE.
pub const QUADRAPASSEL_INTEGRES: &str = "quadrapassel_integres";
/// **DRAPEAU DE MIGRATION DE « LIGHTS OFF »** posé par `app::run` la première
/// fois qu'il range l'applet dans Intégrés, juste après « Quadrapassel ». Le nom porte la ZONE.
pub const LIGHTSOFF_INTEGRES: &str = "lightsoff_integres";
/// **DRAPEAU DE MIGRATION DE « REVERSI »** posé par `app::run` la première
/// fois qu'il range l'applet dans Intégrés, juste après « Lights Off ». Le nom porte la ZONE.
pub const REVERSI_INTEGRES: &str = "reversi_integres";
/// **DRAPEAU DE MIGRATION DE « BOVO »** posé par `app::run` la première
/// fois qu'il range l'applet dans Intégrés, juste après « Reversi ». Le nom porte la ZONE.
pub const BOVO_INTEGRES: &str = "bovo_integres";
/// **DRAPEAU DE MIGRATION DE « PATIENCE »** posé par `app::run` la première
/// fois qu'il range l'applet dans Intégrés, juste après « Bovo ». Le nom porte la ZONE.
pub const PATIENCE_INTEGRES: &str = "patience_integres";

/// **DRAPEAU DE MIGRATION DU MONITEUR SYSTÈME** — posé par `app::run` la première
/// fois qu'il range l'applet dans Intégrés, juste après l'éditeur des textures.
/// Même règle que `TEXTURES_INTEGRES`.
pub const MONITEUR_INTEGRES: &str = "moniteur_integres";

/// **LES COINS ARRONDIS DE LA ZONE PRINCIPALE** (voir `ui::dock::conges`) — un
/// réglage booléen (`load_bool_setting`), lu au démarrage et posé par Réglages.
pub const COINS_ZONE_PRINCIPALE: &str = "coins_zone_principale";

/// **LES COINS NOIRS DES ÉCRANS** (voir `win32::coins_ecran`) — ACTIFS tant que
/// rien n'est enregistré (`load_bool_setting_or(.., true)`) : « permanents ».
pub const COINS_ECRAN: &str = "coins_ecran";

/// **L'OMBRE SOUS LA BARRE DU HAUT SUR TOUTE SA LONGUEUR** — ACTIVE tant que
/// rien n'est enregistré. ⚠️ Plus écrite : seulement relue, une fois, comme
/// défaut de `Portee::zone[0]` (voir `config::load_ombre_noire_portee`).
pub const OMBRE_BARRE_HAUT_PARTOUT: &str = "ombre_barre_haut_partout";
/// Les ombres des barres à la couleur de leur barre (voir
/// `win32::ombre_barre::couleur_des_barres`).
pub const OMBRE_COULEUR_DES_BARRES: &str = "ombre_couleur_des_barres";
/// La noire de la zone centrale à la couleur de sa barre (voir
/// `win32::ombre_barre::zone_en_couleur`).
pub const OMBRE_ZONE_EN_COULEUR: &str = "ombre_zone_en_couleur";
/// Les ombres colorées en couleur AFFICHÉE de leur barre (sinon RÉGLÉE) — ACTIF
/// tant que rien n'est enregistré (voir `win32::ombre_barre::couleur_affichee`).
pub const OMBRE_COULEUR_AFFICHEE: &str = "ombre_couleur_affichee";

/// L'intensité des ombres colorées, en pour-cent (voir
/// `win32::ombre_barre::intensite_couleur`).
pub fn load_ombre_couleur_intensite() -> Option<f32> {
    read_config_floats("ombre_couleur_intensite.txt")?.first().copied()
}

pub fn save_ombre_couleur_intensite(pc: u16) {
    write_config("ombre_couleur_intensite.txt", pc.to_string());
}

/// L'intensité de l'ombre NOIRE, en % (voir `win32::ombre_barre::INTENSITE_NOIRE`).
pub fn load_ombre_noire_intensite() -> Option<f32> {
    read_config_floats("ombre_noire_intensite.txt")?.first().copied()
}

pub fn save_ombre_noire_intensite(pc: u16) {
    write_config("ombre_noire_intensite.txt", pc.to_string());
}

/// Le plein écran replacé dans la zone centrale (voir `win32::plein_ecran_zone`) ;
/// coupé par défaut.
pub const PLEIN_ECRAN_DANS_LA_ZONE: &str = "plein_ecran_dans_la_zone";

/// Les jeux reconnus lancés en fenêtre dans la zone centrale (voir
/// `win32::jeux_fenetres`) ; coupé par défaut.
pub const JEUX_EN_FENETRE: &str = "jeux_en_fenetre";

/// L'effet d'ouverture des fenêtres (voir `win32::ouverture_fenetres`) ; actif
/// par défaut — DEMANDE EXPLICITE.
pub const EFFET_OUVERTURE: &str = "effet_ouverture";

/// La durée de l'effet d'ouverture, en millisecondes ; `None` : jamais réglée.
pub fn load_effet_ouverture_duree() -> Option<u32> {
    read_config_floats("effet_ouverture_duree.txt")?.first().map(|&v| v as u32)
}

pub fn save_effet_ouverture_duree(ms: u32) {
    write_config("effet_ouverture_duree.txt", ms.to_string());
}

/// Le fondu des fenêtres de NeuraDesk (voir `win32::ouverture_fenetres::fondu_actif`) ;
/// coupé par défaut.
pub const FONDU_NEURADESK: &str = "fondu_neuradesk";

/// Les durées du fondu des fenêtres de NeuraDesk, en millisecondes :
/// (apparition, disparition) ; `None` : jamais réglées.
pub fn load_fondu_neuradesk_durees() -> Option<(u32, u32)> {
    let v = read_config_floats("fondu_neuradesk_durees.txt")?;
    Some((*v.first()? as u32, *v.get(1)? as u32))
}

pub fn save_fondu_neuradesk_durees(ouverture: u32, fermeture: u32) {
    write_config("fondu_neuradesk_durees.txt", format!("{ouverture}\n{fermeture}"));
}

/// L'effet de fermeture des fenêtres (voir `win32::ouverture_fenetres::fermeture`) ;
/// actif par défaut — DEMANDE EXPLICITE.
pub const EFFET_FERMETURE: &str = "effet_fermeture";

/// L'animation d'Alt+Tab (voir `win32::ouverture_fenetres::activer_bascule`) ;
/// active par défaut — DEMANDE EXPLICITE (2026-11-05 : « tout, Alt+Tab
/// compris »).
pub const EFFET_BASCULE: &str = "effet_bascule";

/// La durée de l'effet de fermeture, en millisecondes ; `None` : jamais réglée.
pub fn load_effet_fermeture_duree() -> Option<u32> {
    read_config_floats("effet_fermeture_duree.txt")?.first().map(|&v| v as u32)
}

pub fn save_effet_fermeture_duree(ms: u32) {
    write_config("effet_fermeture_duree.txt", ms.to_string());
}

/// ⭐ **LA FORME DE CHAQUE MOMENT ANIMÉ** — un fichier par moment
/// (voir `ouverture_fenetres::Moment`). Codes : 0 retour vers l'icône,
/// 1 extinction, 2 fondu, 3 chute, 4 zoom, 5 étirement (agrandir/restaurer
/// seulement), 6 éclatement, 7 génie, **255 au hasard**. `None` : jamais réglée.
pub fn load_effet_fermeture_style() -> Option<u32> {
    read_config_floats("effet_fermeture_style.txt")?.first().map(|&v| v as u32)
}

pub fn save_effet_fermeture_style(code: u32) {
    write_config("effet_fermeture_style.txt", code.to_string());
}

pub fn load_effet_ouverture_style() -> Option<u32> {
    read_config_floats("effet_ouverture_style.txt")?.first().map(|&v| v as u32)
}

pub fn save_effet_ouverture_style(code: u32) {
    write_config("effet_ouverture_style.txt", code.to_string());
}

/// ⛔ Jamais réglée, la réduction SUIT la fermeture : c'est ce qu'elle faisait
/// avant d'avoir son propre fichier (une seule forme pour les deux gestes), et
/// un utilisateur qui avait choisi « zoom » ne doit pas se retrouver avec le
/// vol par défaut au premier lancement de cette version.
pub fn load_effet_reduction_style() -> Option<u32> {
    read_config_floats("effet_reduction_style.txt")?.first().map(|&v| v as u32)
}

pub fn save_effet_reduction_style(code: u32) {
    write_config("effet_reduction_style.txt", code.to_string());
}

pub fn load_effet_agrandissement_style() -> Option<u32> {
    read_config_floats("effet_agrandissement_style.txt")?.first().map(|&v| v as u32)
}

pub fn save_effet_agrandissement_style(code: u32) {
    write_config("effet_agrandissement_style.txt", code.to_string());
}

/// **Animer les fenêtres des AUTRES applications** — charge `window_hook.dll`
/// dans chaque application qui ouvre une fenêtre (voir `win32::injection`).
/// ⛔ DÉSACTIVÉ par défaut : c'est une injection de module dans les processus de
/// l'utilisateur, la case des Réglages doit être cochée.
pub const EFFET_INJECTION: &str = "effet_injection";

/// Les lueurs (voir `gfx::lueur`, qui porte le format). `None` : jamais réglées.
pub fn load_lueurs() -> Option<String> {
    fs::read_to_string(config_path("lueurs.txt")?).ok()
}

pub fn save_lueurs(texte: String) {
    write_config("lueurs.txt", texte);
}

/// Le nom de l'effet de lueurs chargé en dernier (voir `win32::lueurs_effets`).
/// `None` : aucun, ou un effet supprimé depuis (nom vide).
pub fn load_lueurs_effet() -> Option<String> {
    let nom = fs::read_to_string(config_path("lueurs_effet.txt")?).ok()?;
    let nom = nom.trim();
    (!nom.is_empty()).then(|| nom.to_string())
}

pub fn save_lueurs_effet(nom: &str) {
    write_config("lueurs_effet.txt", nom.to_string());
}

/// Rayon des coins noirs des écrans, en pixels logiques. `None` : jamais réglé.
pub fn load_coins_ecran_rayon() -> Option<f32> {
    read_config_floats("coins_ecran_rayon.txt")?.first().copied()
}

pub fn save_coins_ecran_rayon(px: f32) {
    write_config("coins_ecran_rayon.txt", px.to_string());
}

/// Le rayon des congés de la ZONE PRINCIPALE, en pixels physiques. `None` : jamais
/// réglé, et c'est le STYLE qui décide (voir `ui::dock::rayon_conge`).
///
/// ⭐ **POURQUOI CE FICHIER EXISTE** (2026-10-29) : tant qu'il n'était qu'une
/// constante du style, aucun `CONVERTIS` ne le touchait — le portable emportait
/// donc un rayon de 4K sur un écran plus petit. Mesuré sur une VM 1280×648 :
/// **56 px de congé contre une barre du haut de 18 px**. Écrit par `exporter`
/// (`src/config_initiale.rs`) pour être converti comme les autres longueurs.
pub fn load_coins_zone_rayon() -> Option<f32> {
    read_config_floats("coins_zone_rayon.txt")?.first().copied().filter(|v| *v > 0.0)
}

/// **LES RÉGLAGES DE L'APPLET SCANNER** — `scanner_reglages.txt`, en `clé=valeur`
/// une par ligne.
///
/// ⚠️ **UN SEUL FICHIER POUR NEUF RÉGLAGES**, et c'est délibéré : le dépôt
/// persiste d'ordinaire un réglage par fichier, mais neuf fichiers pour un panneau
/// qui les écrit tous ensemble serait neuf occasions d'en oublier un. Le format
/// reste trivial (pas de dépendance de parsing), et c'est `scanner.rs` qui
/// l'analyse — **par une fonction pure**, donc testable sans toucher au disque.
///
/// Le **dossier de destination vit dedans**, comme les autres : c'est le panneau
/// qui l'écrit, au même moment et par le même chemin.
pub fn load_reglages_scanner() -> Option<String> {
    config_path("scanner_reglages.txt").and_then(|chemin| fs::read_to_string(chemin).ok())
}

/// Écrit les réglages du scanner. Rend `false` si l'écriture a échoué —
/// l'appelant le DIT plutôt que de laisser croire à un réglage retenu (même
/// contrat que `save_sous_dock_personnel`).
pub fn save_reglages_scanner(contenu: &str) -> bool {
    write_config_atomique("scanner_reglages.txt", contenu.to_string())
}

pub fn load_bool_setting(name: &str) -> bool {
    read_config_floats(&format!("{name}.txt")).and_then(|v| v.first().copied()).map(|v| v != 0.0).unwrap_or(false)
}

/// Variante de `load_bool_setting` retournant `default` (plutôt que `false`
/// inconditionnellement) tant que rien n'a encore été enregistré pour
/// `name` — sert à migrer `"top_auto_hide"`/`"left_auto_hide"`/
/// `"right_auto_hide"` (masquage automatique PAR barre de l'interface
/// PRIMAIRE, voir `app::run`) depuis l'ancien réglage unique
/// `"primary_auto_hide"` qu'ils remplacent, sans désactiver silencieusement
/// le masquage automatique déjà activé par les utilisateurs existants.
pub fn load_bool_setting_or(name: &str, default: bool) -> bool {
    read_config_floats(&format!("{name}.txt")).and_then(|v| v.first().copied()).map(|v| v != 0.0).unwrap_or(default)
}

/// Sauvegarde un réglage booléen générique (voir `load_bool_setting`),
/// repris au prochain lancement. Mêmes garanties (best-effort) que
/// `save_topbar_text_color`.
pub fn save_bool_setting(name: &str, enabled: bool) {
    write_config(&format!("{name}.txt"), if enabled { "1" } else { "0" }.to_string());
}

/// Un réglage ENTIER positif générique, même fichier que `load_bool_setting`
/// (`<name>.txt`) — `None` s'il n'a jamais été enregistré ou s'il est illisible.
pub fn load_u32_setting(name: &str) -> Option<u32> {
    read_config_floats(&format!("{name}.txt"))
        .and_then(|v| v.first().copied())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| v as u32)
}

pub fn save_u32_setting(name: &str, value: u32) {
    write_config(&format!("{name}.txt"), value.to_string());
}

/// Couleur de mise en avant INDIVIDUELLE d'une des trois fenêtres `egui`
/// (Dash, panneau de réglages, visionneuse de presse-papiers — voir
/// `win32::dash`/`win32::settings_dash`/`win32::clipboard_viewer`, chacune
/// avait sa propre constante `ACCENT` figée au même vert avant ce réglage,
/// PUIS une couleur partagée entre les trois, PUIS ce réglage par fenêtre)
/// : titres de catégorie, sélection active, bordures de mise en évidence —
/// ET, si `icon_bg` est vrai, fond de l'icône associée dans le dock (voir
/// `ui::dock::icon_background_instance`). `window` identifie la fenêtre
/// (`"dash"`, `"settings"` ou `"clipboard"` — un fichier par fenêtre,
/// `{window}_accent_color.txt`). `None` si jamais personnalisée : chaque
/// appelant retombe alors sur le vert d'origine (`[0.2, 0.6, 0.2]`, celui du
/// logo Node.js), fond d'icône désactivé.
pub fn load_window_accent(window: &str) -> Option<([f32; 3], bool)> {
    match read_config_floats(&format!("{window}_accent_color.txt"))?[..] {
        [r, g, b, icon_bg] => Some(([r, g, b], icon_bg != 0.0)),
        _ => None,
    }
}

/// Sauvegarde la couleur de mise en avant et l'option "fond d'icône" d'une
/// fenêtre `egui`, reprises au prochain lancement ET par toute fenêtre déjà
/// ouverte (chacune les relit à chaque frame, voir les appelants). Mêmes
/// garanties (best-effort) que `save_topbar_text_color`.
pub fn save_window_accent(window: &str, color: [f32; 3], icon_bg: bool) {
    write_config(
        &format!("{window}_accent_color.txt"),
        format!("{},{},{},{}", color[0], color[1], color[2], if icon_bg { 1.0 } else { 0.0 }),
    );
}

/// Couleur SÉPARÉE, optionnelle, qui alimente la palette automatique des
/// éléments (zones, texte, emphase — voir `ui::color::compute_palette`,
/// `egui_overlay::apply_theme`) d'une des trois fenêtres `egui` — demande
/// explicite : "dans la gestion individuelle de la couleur des dashs il faut
/// choisir la couleur de fond mais aussi une couleur pour la palette
/// automatique des éléments". `window` : même convention que
/// `load_window_accent` (un fichier par fenêtre,
/// `{window}_palette_accent_color.txt`). `None` — PAS seulement "jamais
/// personnalisée" mais aussi le réglage explicite normal (case à cocher
/// désactivée, voir `win32::settings_dash::render_accent_section`) : la
/// palette SUIT alors automatiquement `load_window_accent` (couleur de
/// fond), exactement le comportement d'avant l'introduction de ce réglage —
/// `Some(rgb)` : couleur fixe et INDÉPENDANTE de la couleur de fond.
pub fn load_window_palette_accent(window: &str) -> Option<[f32; 3]> {
    match read_config_floats(&format!("{window}_palette_accent_color.txt"))?[..] {
        [r, g, b] => Some([r, g, b]),
        _ => None,
    }
}

/// Sauvegarde la couleur de palette séparée d'une fenêtre `egui` (voir
/// `load_window_palette_accent`), reprise au prochain lancement ET par toute
/// fenêtre déjà ouverte. Mêmes garanties (best-effort) que
/// `save_window_accent`. Pour REVENIR au suivi automatique de la couleur de
/// fond (désactiver la case à cocher), voir `delete_window_palette_accent` —
/// écrire `Some(fond)` ici referait un simple doublon qui ne suivrait plus
/// les changements ultérieurs de la couleur de fond.
pub fn save_window_palette_accent(window: &str, color: [f32; 3]) {
    write_config(&format!("{window}_palette_accent_color.txt"), format!("{},{},{}", color[0], color[1], color[2]));
}

/// Supprime le fichier de couleur de palette personnalisée d'une fenêtre
/// (voir `load_window_palette_accent`) — revient au suivi automatique de la
/// couleur de fond (case à cocher désactivée dans `render_accent_section`).
/// Best-effort comme le reste de ce module : une erreur de suppression
/// (fichier déjà absent, permissions...) reste silencieuse, PAS bloquante —
/// au pire, `load_window_palette_accent` retrouvera une ancienne valeur
/// jusqu'à la prochaine sauvegarde, jamais un crash pour un simple réglage
/// de confort.
pub fn delete_window_palette_accent(window: &str) {
    if let Some(path) = config_path(&format!("{window}_palette_accent_color.txt")) {
        let _ = fs::remove_file(path);
    }
}

/// Couleur de fond INDIVIDUELLE d'un menu contextuel (clic droit, Réseau,
/// Son, Horloge…) — demande explicite : « personnaliser les menus
/// individuellement au niveau de la couleur ». `menu` identifie le menu
/// (`"context"`, `"network"`… voir `win32::menu::MENU_COLOR_KEYS`), un fichier
/// par menu (`{menu}_menu_color.txt`). `None` si jamais personnalisé : le menu
/// retombe alors sur la couleur PARTAGÉE des menus (`menu::menu_accent_rgb`),
/// comportement d'avant ce réglage. Le CONTENU d'un menu reste sur ses pôles
/// neutres fixes (décision explicite antérieure) : seule la teinte de fond
/// dérive de cette couleur (voir `win32::cef_popup::render`, bras `Menu`).
pub fn load_menu_color(menu: &str) -> Option<[f32; 3]> {
    match read_config_floats(&format!("{menu}_menu_color.txt"))?[..] {
        [r, g, b] => Some([r, g, b]),
        _ => None,
    }
}

/// Sauvegarde la couleur de fond individuelle d'un menu (voir
/// `load_menu_color`), reprise au prochain lancement ET par la frame suivante
/// (voir `win32::menu::set_menu_color`). Best-effort comme le reste du module.
pub fn save_menu_color(menu: &str, color: [f32; 3]) {
    write_config(&format!("{menu}_menu_color.txt"), format!("{},{},{}", color[0], color[1], color[2]));
}

/// Supprime le fichier de couleur individuelle d'un menu — revient à la
/// couleur partagée des menus (voir `load_menu_color`). Best-effort.
pub fn delete_menu_color(menu: &str) {
    if let Some(path) = config_path(&format!("{menu}_menu_color.txt")) {
        let _ = fs::remove_file(path);
    }
}

/// Transparence PARTAGÉE des trois fenêtres `egui` (Dash, panneau de
/// réglages, visionneuse de presse-papiers — voir `win32::dash`/`win32::
/// settings_dash`/`win32::clipboard_viewer`, `EguiOverlay::frame`) : force
/// du mélange entre le fond flouté brut et sa teinte (couleur individuelle
/// de chaque fenêtre, voir `load_window_accent`), 0.0 = flou pur, 1.0 =
/// couleur pleine. UNE SEULE valeur pour les trois fenêtres (PAS une par
/// fenêtre comme `bar_tint`, une par barre) — demande explicite : les trois
/// curseurs du panneau de réglages restent synchronisés, modifier l'un
/// modifie les deux autres. `None` si jamais personnalisée : `0.55`
/// (`win32::dash`/etc. avant ce réglage).
pub fn load_dash_tint() -> Option<f32> {
    read_config_floats("dash_tint.txt")?.first().copied()
}

/// Sauvegarde la transparence partagée des trois fenêtres, reprise au
/// prochain lancement ET par toute fenêtre déjà ouverte (chacune la relit à
/// chaque frame, voir les appelants). Mêmes garanties (best-effort) que
/// `save_bar_tint`.
pub fn save_dash_tint(tint: f32) {
    write_config("dash_tint.txt", tint.to_string());
}

/// Écartement entre les icônes du dock (voir `ui::dock::Dock::icon_rects`,
/// `Dock::set_icon_gap`) — en pixels, mêmes coordonnées que `ui::dock::
/// ICON_SIZE`. `None` si jamais personnalisé : 12.0 (valeur d'origine,
/// figée en constante avant ce réglage).
pub fn load_icon_gap() -> Option<f32> {
    read_config_floats("icon_gap.txt")?.first().copied()
}

/// Sauvegarde l'écartement entre les icônes du dock, repris au prochain
/// lancement. Même garanties (best-effort) que `save_dash_tint`.
pub fn save_icon_gap(gap: f32) {
    write_config("icon_gap.txt", gap.to_string());
}

/// L'écartement PROPRE à chaque barre (voir `ui::dock::BARRES_ECARTEMENT`) :
/// une valeur par ligne, `-` pour une barre qui suit l'écartement général.
/// Fichier absent ou illisible : toutes le suivent.
pub fn load_icon_gaps_barres() -> [Option<f32>; 4] {
    let contenu = config_path("icon_gap_barres.txt").and_then(|p| fs::read_to_string(p).ok()).unwrap_or_default();
    lire_icon_gaps_barres(&contenu)
}

fn lire_icon_gaps_barres(contenu: &str) -> [Option<f32>; 4] {
    let mut gaps = [None; 4];
    for (gap, ligne) in gaps.iter_mut().zip(contenu.lines()) {
        *gap = ligne.trim().parse::<f32>().ok().filter(|v| v.is_finite());
    }
    gaps
}

pub fn save_icon_gaps_barres(gaps: &[Option<f32>; 4]) {
    let lignes: Vec<String> = gaps.iter().map(|g| g.map_or("-".to_string(), |v| v.to_string())).collect();
    write_config("icon_gap_barres.txt", lignes.join("
"));
}

/// Taille (côté du carré, en pixels) des pastilles de bureaux virtuels de
/// la barre PRIMAIRE (voir `ui::topbar::desktop_dot_rect`, `ui::dock::
/// Dock::desktop_dot_size`) — DEMANDE EXPLICITE ("je voudrais que les
/// pastilles des bureaux virtuels soient redimensionnables dans le dash
/// réglage"). `None` si jamais personnalisé : `ui::topbar::
/// DEFAULT_DESKTOP_DOT_SIZE` (18.0, valeur d'origine, figée en constante
/// avant ce réglage), même convention que `load_icon_gap`.
pub fn load_desktop_dot_size() -> Option<f32> {
    read_config_floats("desktop_dot_size.txt")?.first().copied()
}

/// Sauvegarde la taille des pastilles de bureaux virtuels de la barre
/// PRIMAIRE, repris au prochain lancement. Même garanties (best-effort) que
/// `save_icon_gap`.
pub fn save_desktop_dot_size(size: f32) {
    write_config("desktop_dot_size.txt", size.to_string());
}

/// Marge laissée DANS LA CASE d'une icône de zone, en pixels — DEMANDE
/// EXPLICITE : « on pourrait essayer 8 pixels ou mieux que ce soit
/// paramétrable ». `None` tant que rien n'a été enregistré : c'est
/// `Dock::DEFAULT_ZONE_CELL_MARGIN` qui s'applique alors.
///
/// Même convention que `load_desktop_dot_size` ci-dessus — un fichier texte, un
/// flottant, un `Option` qui laisse le défaut au module de dessin plutôt que de
/// le figer ici.
pub fn load_zone_cell_margin() -> Option<f32> {
    read_config_floats("zone_cell_margin.txt")?.first().copied()
}

/// Sauvegarde la marge de case des icônes de zone, reprise au prochain
/// lancement. Mêmes garanties (best-effort) que `save_icon_gap`.
pub fn save_zone_cell_margin(px: f32) {
    write_config("zone_cell_margin.txt", px.to_string());
}

/// Relit la taille des pastilles de bureaux virtuels PERSONNALISÉE de
/// chaque écran secondaire (voir `app::SecondaryScreenOverrides::
/// desktop_dot_sizes`) — DEMANDE EXPLICITE ("chacune leur propre réglage
/// dans la barre correspondante") : un écran absent de cette table suit la
/// taille PRIMAIRE (`load_desktop_dot_size`), même logique que
/// `load_secondary_screen_fullscreen_clock_text_scale`.
pub fn load_secondary_screen_desktop_dot_sizes() -> std::collections::HashMap<String, f32> {
    let Some(path) = config_path("secondary_screen_desktop_dot_sizes.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let (device_name, size) = line.split_once(',')?;
            Some((device_name.trim().to_string(), size.trim().parse().ok()?))
        })
        .collect()
}

/// Sauvegarde la taille des pastilles de bureaux virtuels personnalisée de
/// chaque écran secondaire (voir `load_secondary_screen_desktop_dot_sizes`),
/// reprise au prochain lancement.
pub fn save_secondary_screen_desktop_dot_sizes(sizes: &std::collections::HashMap<String, f32>) {
    let content = sizes
        .iter()
        .map(|(device_name, size)| format!("{device_name},{size}"))
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_desktop_dot_sizes.txt", content);
}

// ---------------------------------------------------------------------------
// ANNIVERSAIRES — les dates que l'utilisateur saisit lui-même
// ---------------------------------------------------------------------------

/// Bornes d'une année saisie — quatre chiffres, comme demandé. Une année hors
/// de là est refusée plutôt que corrigée (voir `cef_calendar_applet`).
pub const ANNEE_MIN: u32 = 1000;
pub const ANNEE_MAX: u32 = 9999;

/// De quelle FAMILLE est une date saisie — les deux se ressemblent (un nom et
/// un jour dans l'année) mais ne se lisent pas pareil dans la fenêtre, et
/// l'utilisateur les cherche séparément : « Anniversaires » puis « Fêtes
/// personnalisées ».
///
/// DEMANDE EXPLICITE (2026-09-23) : « ajoute la possibilité de mettre des dates
/// de fête personnalisée », après une première demande sur les seuls
/// anniversaires.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Genre {
    /// L'anniversaire de quelqu'un.
    Anniversaire,
    /// Une fête à soi : une date qui compte, sans être un anniversaire.
    Fete,
}

impl Genre {
    /// La CLÉ écrite dans le fichier ET transmise à la page — stable, sans
    /// accent, minuscule. La changer casserait les fichiers déjà enregistrés
    /// comme l'affichage, d'où ce commentaire.
    pub fn cle(self) -> &'static str {
        match self {
            Genre::Anniversaire => "anniversaire",
            Genre::Fete => "fete",
        }
    }

    /// L'inverse de `cle` — ce qu'on relit d'un fichier. `None` pour une clé
    /// inconnue : une entrée qu'on ne sait pas classer est ÉCARTÉE, jamais
    /// rangée d'office dans une famille (voir `load_dates`, même politique que
    /// pour une date impossible).
    pub fn depuis_cle(cle: &str) -> Option<Self> {
        match cle {
            "anniversaire" => Some(Genre::Anniversaire),
            "fete" => Some(Genre::Fete),
            _ => None,
        }
    }
}

/// Une date saisie dans la fenêtre du calendrier — un anniversaire ou une fête
/// personnalisée, voir `Genre`. DEMANDE EXPLICITE (2026-09-23) : « afficher les
/// fêtes du jour et les dates d'anniversaires avec la possibilité de mettre un
/// rappel automatique ».
///
/// **UNE SEULE TABLE POUR LES DEUX FAMILLES**, et non deux listes : tout le
/// reste — l'année non stockée, le rappel date par date, la validation, la
/// relecture — leur est commun, et deux listes auraient fini par diverger sur
/// l'un de ces points.
///
/// **L'ANNÉE N'EST PAS STOCKÉE**, et c'est délibéré : ces dates reviennent
/// chaque année, donc seule la date dans l'année compte. Stocker l'année
/// obligerait à décider quoi faire d'un 29 février les années ordinaires ;
/// ne pas la stocker évite la question et allège la saisie.
#[derive(Clone, Debug)]
pub struct DatePersonnalisee {
    pub nom: String,
    pub jour: u32,
    pub mois: u32,
    /// Rappel automatique activé POUR CETTE DATE — c'est la « possibilité de
    /// mettre un rappel » de la demande : elle se pose date par date, pas
    /// globalement, parce qu'on ne veut pas être alerté de tout.
    pub rappel: bool,
    pub genre: Genre,
    /// **L'ANNÉE, QUAND L'UTILISATEUR LA DONNE** — `None` sinon.
    ///
    /// ⚠️ **CE COMMENTAIRE A CHANGÉ DE SENS.** Il disait « l'année n'est PAS
    /// stockée, et c'est délibéré : ces dates reviennent chaque année ». C'est
    /// toujours vrai — la date REVIENT chaque année, et c'est (jour, mois) qui
    /// déclenche le rappel — mais l'année a fini par servir à autre chose :
    /// **connaître l'âge**. DEMANDE EXPLICITE (2026-09-23) : « pour les dates
    /// d'anniversaire j'aimerais pouvoir ajouter l'année sur quatre chiffres ».
    ///
    /// Elle reste donc OPTIONNELLE : une date sans année est une date comme
    /// avant, et la saisie ne l'impose pas. Le 29 février ne pose plus de
    /// question non plus, puisqu'il ne sert qu'à calculer un âge.
    pub annee: Option<u32>,
}

/// Le fichier des dates saisies — TOML, comme `clipboard_history.toml`, dont ce
/// module reprend la discipline : relire la table entière, écrire la table
/// entière.
///
/// ⚠️ **IL S'APPELAIT `birthdays.toml`** tant qu'il ne portait que des
/// anniversaires. Le renommer n'a rien coûté : **aucun fichier de ce nom
/// n'existait encore** (la fonctionnalité n'avait pas été commitée, et
/// l'utilisateur n'avait rien saisi). Si un jour ce renommage devait se refaire
/// sur des données RÉELLES, il faudrait une relecture de l'ancien nom.
const DATES_FILE: &str = "dates.toml";

/// Les dates saisies, dans l'ordre où elles ont été ajoutées.
///
/// **Une entrée invalide est ÉCARTÉE, pas « réparée »** : le fichier peut être
/// édité à la main, et une date impossible (le 31 février) n'a pas de
/// correction devinable. Mieux vaut la perdre en le disant que l'inventer —
/// même politique que `load_clipboard_history`, qui ignore une entrée dont
/// l'image manque. **Un GENRE inconnu suit la même règle** : on ne range pas
/// d'office dans une famille ce qu'on ne sait pas classer.
pub fn load_dates() -> Vec<DatePersonnalisee> {
    let Some(path) = config_path(DATES_FILE) else { return Vec::new() };
    let Ok(content) = fs::read_to_string(path) else { return Vec::new() };
    let table = match content.parse::<toml::Table>() {
        Ok(table) => table,
        Err(err) => {
            eprintln!("[neuradesk] dates.toml invalide, ignoré : {err}");
            return Vec::new();
        }
    };
    let Some(liste) = table.get("dates").and_then(|v| v.as_array()) else { return Vec::new() };
    liste
        .iter()
        .filter_map(|e| {
            let nom = e.get("nom")?.as_str()?.trim().to_string();
            let jour = e.get("jour")?.as_integer()? as u32;
            let mois = e.get("mois")?.as_integer()? as u32;
            if nom.is_empty() || !(1..=12).contains(&mois) || jour < 1 || jour > 31 {
                eprintln!("[neuradesk] date ignorée (date impossible) : {nom:?} {jour}/{mois}");
                return None;
            }
            let Some(genre) = e.get("genre").and_then(|v| v.as_str()).and_then(Genre::depuis_cle) else {
                eprintln!("[neuradesk] date ignorée (genre inconnu) : {nom:?} {jour}/{mois}");
                return None;
            };
            // `rappel` absent vaut `false` : un fichier écrit à la main sans ce
            // champ ne doit pas se mettre à sonner tout seul.
            let rappel = e.get("rappel").and_then(|v| v.as_bool()).unwrap_or(false);
            // L'ANNÉE est FACULTATIVE : absente, nulle ou illisible vaut « pas
            // d'année » — jamais une erreur, et jamais devinée (même politique
            // que partout ailleurs dans ce module).
            let annee = e.get("annee").and_then(|v| v.as_integer()).and_then(|a| u32::try_from(a).ok()).filter(|a| (1000..=9999).contains(a));
            Some(DatePersonnalisee { nom, jour, mois, rappel, genre, annee })
        })
        .collect()
}

/// Sauvegarde les dates saisies, reprises au prochain lancement (voir
/// `load_dates`). Mêmes garanties (best-effort) que `save_clipboard_history`.
pub fn save_dates(liste: &[DatePersonnalisee]) {
    let mut table = toml::Table::new();
    table.insert(
        "dates".to_string(),
        toml::Value::Array(
            liste
                .iter()
                .map(|d| {
                    let mut row = toml::Table::new();
                    row.insert("nom".to_string(), toml::Value::String(d.nom.clone()));
                    row.insert("jour".to_string(), toml::Value::Integer(d.jour as i64));
                    row.insert("mois".to_string(), toml::Value::Integer(d.mois as i64));
                    row.insert("rappel".to_string(), toml::Value::Boolean(d.rappel));
                    row.insert("genre".to_string(), toml::Value::String(d.genre.cle().to_string()));
                    // L'année n'est écrite QUE si elle existe : un fichier sans
                    // elle reste lisible par une version antérieure.
                    if let Some(annee) = d.annee {
                        row.insert("annee".to_string(), toml::Value::Integer(annee as i64));
                    }
                    toml::Value::Table(row)
                })
                .collect(),
        ),
    );
    write_config(DATES_FILE, table.to_string());
}

/// **UNE FÊTE DE LA TABLE À RAPPELER** — DEMANDE EXPLICITE (2026-09-23) :
/// « dans les résultats des fêtes et des anniversaires j'aimerais avoir la
/// possibilité de mettre un rappel ou de le retirer ».
///
/// ⚠️ **POURQUOI UNE SECONDE TABLE, ALORS QUE `rappel_feries` EXISTE DÉJÀ** :
/// `rappel_feries` est un interrupteur GLOBAL (tous les jours fériés, ou aucun),
/// et `DatePersonnalisee::rappel` ne concerne que les dates SAISIES. Ni l'un ni
/// l'autre ne permet de dire « rappelle-moi la Saint-Matthieu », qui est une
/// fête de la table qu'on veut suivre SEULE. D'où cette liste-ci.
///
/// **LA CLÉ EST `(mois, jour, nom)`, PAS SEULEMENT LA DATE** : 54 jours de
/// l'année portent DEUX fêtes (mesuré le 2026-09-24, voir `fetes_france.tsv`),
/// et une clé réduite à la date les rappellerait toutes les deux ensemble.
pub struct RappelFete {
    pub nom: String,
    pub jour: u32,
    pub mois: u32,
}

/// Le fichier des fêtes de la table à rappeler — même discipline TOML que
/// `dates.toml`.
const RAPPELS_FETES_FILE: &str = "rappels_fetes.toml";

/// Les fêtes de la table qui portent un rappel. Une entrée invalide est
/// ÉCARTÉE, comme partout ailleurs dans ce module : une date impossible n'a pas
/// de correction devinable, et un nom vide ne désigne rien.
pub fn load_rappels_fetes() -> Vec<RappelFete> {
    let Some(path) = config_path(RAPPELS_FETES_FILE) else { return Vec::new() };
    let Ok(content) = fs::read_to_string(path) else { return Vec::new() };
    let table = match content.parse::<toml::Table>() {
        Ok(table) => table,
        Err(err) => {
            eprintln!("[neuradesk] rappels_fetes.toml invalide, ignoré : {err}");
            return Vec::new();
        }
    };
    let Some(liste) = table.get("rappels").and_then(|v| v.as_array()) else { return Vec::new() };
    liste
        .iter()
        .filter_map(|e| {
            let nom = e.get("nom")?.as_str()?.trim().to_string();
            let jour = e.get("jour")?.as_integer()? as u32;
            let mois = e.get("mois")?.as_integer()? as u32;
            if nom.is_empty() || !(1..=12).contains(&mois) || !(1..=31).contains(&jour) {
                eprintln!("[neuradesk] rappel de fête ignoré (entrée invalide) : {nom:?} {jour}/{mois}");
                return None;
            }
            Some(RappelFete { nom, jour, mois })
        })
        .collect()
}

pub fn save_rappels_fetes(liste: &[RappelFete]) {
    let mut table = toml::Table::new();
    table.insert(
        "rappels".to_string(),
        toml::Value::Array(
            liste
                .iter()
                .map(|r| {
                    let mut row = toml::Table::new();
                    row.insert("nom".to_string(), toml::Value::String(r.nom.clone()));
                    row.insert("jour".to_string(), toml::Value::Integer(r.jour as i64));
                    row.insert("mois".to_string(), toml::Value::Integer(r.mois as i64));
                    toml::Value::Table(row)
                })
                .collect(),
        ),
    );
    write_config(RAPPELS_FETES_FILE, table.to_string());
}

/// Rappel automatique sur les JOURS FÉRIÉS — réglage **GLOBAL**, lui, et c'est
/// la différence avec les anniversaires : personne ne veut cocher un par un
/// les onze fériés de l'année, alors qu'on choisit ses anniversaires un par un.
///
/// `false` par défaut : on n'alerte pas quelqu'un sans qu'il l'ait demandé.
pub fn load_rappel_feries() -> bool {
    read_config_floats("rappel_feries.txt").and_then(|v| v.first().copied()).map(|v| v != 0.0).unwrap_or(false)
}

pub fn save_rappel_feries(actif: bool) {
    write_config("rappel_feries.txt", if actif { "1" } else { "0" }.to_string());
}

/// Fichier de l'empreinte des rappels acquittés — voir `load_rappel_ack`.
const RAPPEL_ACK_FILE: &str = "rappel_ack.txt";

/// **L'EMPREINTE DES RAPPELS DÉJÀ VUS** — ce que la cloche de la barre du haut
/// retient quand on clique dessus.
///
/// La valeur est fabriquée par `win32::fetes::empreinte_des_rappels` (date +
/// liste des rappels du jour) et comparée par `fetes::cloche_doit_clignoter`.
/// Ce module ne fait que la garder.
///
/// ⚠️ **LUE COMME DU TEXTE, ET C'EST POURQUOI ELLE N'A PAS PU RÉUTILISER
/// `load_bool_setting`** : une empreinte commence par une date, donc
/// `read_config_floats` — qui parse le contenu comme des flottants — la rendrait
/// vide, et la cloche se rallumerait à chaque lancement sans que rien ne le dise.
///
/// Rend `None` quand rien n'a jamais été acquitté (fichier absent ou vide).
pub fn load_rappel_ack() -> Option<String> {
    let texte = fs::read_to_string(config_path(RAPPEL_ACK_FILE)?).ok()?;
    let texte = texte.trim().to_string();
    (!texte.is_empty()).then_some(texte)
}

pub fn save_rappel_ack(empreinte: &str) {
    write_config(RAPPEL_ACK_FILE, empreinte.to_string());
}

/// Fichier de l'ensemble des rappels de NOTES acquittés — voir
/// `load_rappel_ack_notes`.
const RAPPEL_ACK_NOTES_FILE: &str = "rappel_ack_notes.txt";

/// **LES RAPPELS DE NOTES ACQUITTÉS, POUR LA JOURNÉE** — ce que le bouton
/// « Acquitter » du panneau Notes retient, note par note.
///
/// ⚠️ **UN FICHIER À PART, ET PAS UNE LIGNE DE PLUS DANS `rappel_ack.txt`** :
/// les deux mécanismes n'ont pas la même forme — la cloche garde UNE empreinte
/// (elle acquitte tout ce qui était affiché, en un geste), celui-ci garde un
/// ENSEMBLE d'identités (la demande est « un bouton par note »). Les mêler dans
/// un fichier aurait obligé chaque lecteur à connaître les deux grammaires.
///
/// La valeur est fabriquée par `win32::fetes::acquitter_rappel_note` : le jour
/// en tête, puis les identités. **Lue comme du TEXTE**, pour la même raison que
/// `load_rappel_ack` — elle commence par une date, donc `read_config_floats` la
/// rendrait vide.
pub fn load_rappel_ack_notes() -> String {
    config_path(RAPPEL_ACK_NOTES_FILE)
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|texte| texte.trim().to_string())
        .unwrap_or_default()
}

pub fn save_rappel_ack_notes(ensemble: &str) {
    write_config(RAPPEL_ACK_NOTES_FILE, ensemble.to_string());
}

/// Nom du fichier du réglage de taille de police des textes de la barre du haut
/// de la barre PRIMAIRE — un NOMBRE, ou le mot `auto` (voir
/// `load_topbar_text_size`).
const TOPBAR_TEXT_SIZE_FILE: &str = "topbar_text_size.txt";

/// Taille de police PERSONNALISÉE des textes de la barre du haut de la barre
/// PRIMAIRE ; `None` = réglage AUTOMATIQUE. DEMANDE EXPLICITE (2026-09-23) :
/// « le texte des applets de la barre du haut soit configurable aussi en
/// laissant le choix du réglage automatique ou de la taille personnalisée ».
///
/// ⚠️ **L'ABSENCE DU FICHIER ET LE MOT `auto` VEULENT DIRE LA MÊME CHOSE**, et
/// c'est délibéré : quelqu'un qui efface sa configuration retombe sur
/// l'automatique, jamais sur une taille arbitraire. C'est aussi pourquoi un
/// contenu illisible rend `None` — au pire, on suit la barre.
///
/// Un SEUL nombre, pas quatre : c'est la BASE que ce réglage remplace, les
/// écarts entre l'horloge et les textes secondaires restant ceux de
/// `ui::topbar` (voir `topbar_text_font_px`, doc).
pub fn load_topbar_text_size() -> Option<f32> {
    let path = config_path(TOPBAR_TEXT_SIZE_FILE)?;
    let content = fs::read_to_string(path).ok()?;
    content.trim().parse().ok()
}

/// Sauvegarde la taille personnalisée des textes de la barre du haut de la barre
/// PRIMAIRE — `None` écrit `auto`, qui vaut automatique à la relecture (voir
/// `load_topbar_text_size`). Même garanties (best-effort) que
/// `save_desktop_dot_size`.
pub fn save_topbar_text_size(size: Option<f32>) {
    let content = size.map(|px| px.to_string()).unwrap_or_else(|| "auto".to_string());
    write_config(TOPBAR_TEXT_SIZE_FILE, content);
}

/// Taille de police personnalisée des textes de la barre du haut de chaque écran
/// SECONDAIRE (voir `load_topbar_text_size`) — même format `écran,taille` que
/// `load_secondary_screen_desktop_dot_sizes`, et même convention : **un écran
/// ABSENT de la table suit la barre PRINCIPALE**, il n'y a donc rien à écrire
/// pour son réglage automatique (voir `save_secondary_screen_topbar_text_sizes`,
/// appelé avec la table amputée de l'écran remis en automatique).
pub fn load_secondary_screen_topbar_text_sizes() -> std::collections::HashMap<String, f32> {
    let Some(path) = config_path("secondary_screen_topbar_text_sizes.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let (device_name, size) = line.split_once(',')?;
            Some((device_name.trim().to_string(), size.trim().parse().ok()?))
        })
        .collect()
}

/// Sauvegarde la taille de police personnalisée des textes de la barre du haut
/// de chaque écran secondaire (voir `load_secondary_screen_topbar_text_sizes`),
/// reprise au prochain lancement.
pub fn save_secondary_screen_topbar_text_sizes(sizes: &std::collections::HashMap<String, f32>) {
    let content = sizes
        .iter()
        .map(|(device_name, size)| format!("{device_name},{size}"))
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_topbar_text_sizes.txt", content);
}

/// Relit l'écartement entre icônes PERSONNALISÉ de chaque écran secondaire
/// (voir `app::SecondaryScreenOverrides::icon_gaps`) — DEMANDE EXPLICITE
/// ("je voudrais pouvoir choisir l'espace par defaut entre les icones sur
/// les barres secondaires comme sur l'interface principale") : un écran
/// absent de cette table suit l'écartement PRIMAIRE (`load_icon_gap`),
/// même logique que `load_secondary_screen_desktop_dot_sizes`.
pub fn load_secondary_screen_icon_gaps() -> std::collections::HashMap<String, f32> {
    let Some(path) = config_path("secondary_screen_icon_gaps.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    content
        .lines()
        .filter_map(|line| {
            let (device_name, gap) = line.split_once(',')?;
            Some((device_name.trim().to_string(), gap.trim().parse().ok()?))
        })
        .collect()
}

/// Sauvegarde l'écartement entre icônes personnalisé de chaque écran
/// secondaire (voir `load_secondary_screen_icon_gaps`), repris au prochain
/// lancement.
pub fn save_secondary_screen_icon_gaps(gaps: &std::collections::HashMap<String, f32>) {
    let content = gaps
        .iter()
        .map(|(device_name, gap)| format!("{device_name},{gap}"))
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_icon_gaps.txt", content);
}

/// Relit la répartition/l'ordre des icônes PERSONNALISÉ de chaque écran
/// secondaire (voir `app::SecondaryScreenOverrides::icon_orders`) — BUG RÉEL
/// SIGNALÉ ("l'ordre des icones de la barre secondaire ne semble pas être
/// sauvegardé") : `load_icon_order`/`save_icon_order` ci-dessus ne
/// persistent QUE l'ordre de la barre PRIMAIRE (`app::run` refuse à dessein
/// de les appeler avec l'ordre d'un dock secondaire, pour ne pas écraser
/// silencieusement l'ordre primaire) — un écran secondaire réorganisé par
/// glisser-déposer n'avait donc jamais nulle part où persister son propre
/// ordre. Une ligne par écran personnalisé, `device_name|gauche|droite|
/// haut-gauche|haut-droite`, chaque zone une liste d'index séparés par des
/// virgules (même format interne que `load_icon_order`, juste préfixé par
/// `device_name` et séparé par `|` plutôt que des lignes séparées, pour
/// garder un écran par ligne). Écran absent de cette table : `Dock::new`/
/// l'ordre par défaut posé par `app::rebuild_secondary_docks` restent
/// inchangés (comportement d'origine).
pub fn load_secondary_screen_icon_orders() -> std::collections::HashMap<String, (Vec<usize>, Vec<usize>, Vec<usize>, Vec<usize>)> {
    let Some(path) = config_path("secondary_screen_icon_order.txt") else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    let parse_zone = |s: &str| -> Vec<usize> { s.split(',').filter_map(|p| p.trim().parse().ok()).collect() };
    content
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(5, '|');
            let device_name = parts.next()?.trim().to_string();
            let left = parse_zone(parts.next()?);
            let right = parts.next().map(parse_zone).unwrap_or_default();
            let top_left = parts.next().map(parse_zone).unwrap_or_default();
            let top_right = parts.next().map(parse_zone).unwrap_or_default();
            Some((device_name, (left, right, top_left, top_right)))
        })
        .collect()
}

/// Sauvegarde la répartition/l'ordre des icônes personnalisé de chaque écran
/// secondaire (voir `load_secondary_screen_icon_orders`), repris au prochain
/// lancement/`app::rebuild_secondary_docks`. Mêmes garanties (best-effort)
/// que `save_secondary_screen_icon_gaps`.
pub fn save_secondary_screen_icon_orders(
    orders: &std::collections::HashMap<String, (Vec<usize>, Vec<usize>, Vec<usize>, Vec<usize>)>,
) {
    let format_zone = |ids: &[usize]| ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    let content = orders
        .iter()
        .map(|(device_name, (left, right, top_left, top_right))| {
            format!("{device_name}|{}|{}|{}|{}", format_zone(left), format_zone(right), format_zone(top_left), format_zone(top_right))
        })
        .collect::<Vec<_>>()
        .join("\n");
    write_config("secondary_screen_icon_order.txt", content);
}

/// **LE MÊME DÉCALAGE QUE LE PRIMAIRE, POUR LES ORDRES DES ÉCRANS SECONDAIRES —
/// ET SON ABSENCE A CASSÉ DISPLAY2.**
///
/// ⚠️ **DÉFAUT RÉEL, 2026-09-23, signalé par l'utilisateur : « ça a cassé les applets de
/// l'écran secondaire ».** L'arithmétique des applets est
/// `PINNED_ICON_BASE + pinned_apps.len()`, et **les quatre listes d'un écran secondaire
/// citent des index ABSOLUS, exactement comme `icon_order.txt`**. Le primaire suivait le
/// changement du nombre d'épingles (`decaler_bloc_epingles_dans`, appelé par
/// `win32::cef_dash`) ; les secondaires, non. Chaque épingle ajoutée ou retirée
/// re-pointait donc **tout** leur ordre : mesuré, la liste de DISPLAY2 citait `6,7,8` —
/// devenues des ÉPINGLES — là où elle voulait dire « alimentation, terminal, son ».
///
/// **LA BORNE EST `usize::MAX`, ET CE N'EST PAS UNE NÉGLIGENCE.** Le primaire s'arrête à
/// `winpin_base` parce qu'au-delà, les index des épingles de la barre des tâches sont
/// FIGÉS (les remplisseurs absorbent la croissance — c'est la propriété que mesure
/// `adding_a_dock_pin_shifts_no_spacer`). **Un écran secondaire n'a AUCUNE épingle de
/// barre des tâches** (`DockInstance::new` ne réplique pas `ensure_winpin_capacity`), donc
/// cette zone figée n'y existe pas : tout ce qui est cité à partir de l'index décalé —
/// épingles réservées, applets ET espaces — glisse.
pub fn decaler_les_ordres_secondaires(index: usize, retrait: bool) -> Vec<String> {
    let mut orders = load_secondary_screen_icon_orders();
    if orders.is_empty() {
        return Vec::new();
    }
    let mut touches = Vec::new();
    for (device_name, (left, right, top_left, top_right)) in orders.iter_mut() {
        crate::ui::dock::decaler_bloc_epingles_dans([left, right, top_left, top_right], index, usize::MAX, retrait);
        touches.push(device_name.clone());
    }
    save_secondary_screen_icon_orders(&orders);
    touches
}

/// Forme COMMUNE aux deux fichiers « par écran, indexés » ci-dessous — une
/// ligne par écran personnalisé, `device_name|index,champs;index,champs;...` :
/// le `;` sépare les ENTRÉES, la `,` les champs DANS une entrée. C'est la
/// convention interne de `spacer_sizes.txt` (une paire par ligne, voir
/// `load_spacer_sizes`) transposée — il faut ici une ligne par ÉCRAN, donc les
/// entrées ne peuvent plus y être des lignes.
///
/// Une entrée dont l'index n'est pas un entier est écartée SEULE (contrairement
/// à `load_spacer_sizes`, qui écarte la ligne entière : là-bas une entrée EST
/// une ligne, ici elle n'en est qu'un morceau). Écran absent = rien à
/// restaurer, le défaut s'applique.
fn read_secondary_screen_indexed(file_name: &str) -> std::collections::HashMap<String, Vec<(usize, Vec<f32>)>> {
    let Some(path) = config_path(file_name) else { return Default::default() };
    let Ok(content) = fs::read_to_string(path) else { return Default::default() };
    parse_secondary_screen_indexed(&content)
}

/// Cœur PUR de `read_secondary_screen_indexed` (et jumeau de
/// `format_secondary_screen_indexed`) — séparé pour être TESTABLE sans toucher
/// au disque : ces fichiers vivent dans `%APPDATA%\NeuraDesk`, donc un test qui
/// les écrirait modifierait la configuration RÉELLE de la machine.
fn parse_secondary_screen_indexed(content: &str) -> std::collections::HashMap<String, Vec<(usize, Vec<f32>)>> {
    content
        .lines()
        .filter_map(|line| {
            let (device_name, entries) = line.split_once('|')?;
            let device_name = device_name.trim();
            if device_name.is_empty() {
                return None;
            }
            let parsed: Vec<(usize, Vec<f32>)> = entries
                .split(';')
                .filter_map(|entry| {
                    let (index, rest) = entry.split_once(',')?;
                    let index: usize = index.trim().parse().ok()?;
                    let values = rest.split(',').map(|field| field.trim().parse::<f32>().ok()).collect::<Option<Vec<f32>>>()?;
                    Some((index, values))
                })
                .collect();
            Some((device_name.to_string(), parsed))
        })
        .collect()
}

/// Taille des espaces invisibles de CHAQUE écran secondaire (voir `ui::dock::
/// Dock::spacer_sizes`) — jumeau PAR ÉCRAN de `spacer_sizes.txt`, même rapport
/// que `secondary_screen_icon_order.txt` avec `icon_order.txt`. Absent = la
/// taille par défaut (`SPACER_HEIGHT`), comme avant ce fichier.
///
/// **POURQUOI IL EXISTE** : `app::persist_lateral_sizes` sortait autrefois
/// immédiatement pour un écran secondaire, au motif que sa `DockInstance`
/// « repart de zéro à chaque reconstruction, donc rien à gagner à écrire un
/// fichier pour une valeur qui ne survivrait de toute façon pas ». Ce motif ne
/// tenait plus : l'ORDRE de ce même dock est bel et bien persisté
/// (`icon_orders`) et restauré, donc une taille le pouvait aussi. Ce qui était
/// vrai, c'est qu'un espace redimensionné sur un écran secondaire revenait à sa
/// taille par défaut à chaque redémarrage, sans que rien ne le dise.
pub fn load_secondary_screen_spacer_sizes() -> std::collections::HashMap<String, Vec<(usize, f32)>> {
    spacer_sizes_from_indexed(read_secondary_screen_indexed("secondary_screen_spacer_sizes.txt"))
}

/// Cœur PUR de `load_secondary_screen_spacer_sizes` : un espace par entrée, sa
/// taille étant le PREMIER champ — une entrée sans champ est écartée.
fn spacer_sizes_from_indexed(
    parsed: std::collections::HashMap<String, Vec<(usize, Vec<f32>)>>,
) -> std::collections::HashMap<String, Vec<(usize, f32)>> {
    parsed
        .into_iter()
        .map(|(device_name, entries)| {
            let sizes = entries.into_iter().filter_map(|(index, values)| Some((index, *values.first()?))).collect();
            (device_name, sizes)
        })
        .collect()
}

/// Sauvegarde les tailles d'espaces de chaque écran secondaire (voir
/// `load_secondary_screen_spacer_sizes`). Mêmes garanties (best-effort) que
/// `save_secondary_screen_icon_gaps`.
pub fn save_secondary_screen_spacer_sizes(sizes: &std::collections::HashMap<String, Vec<(usize, f32)>>) {
    let per_screen = sizes
        .iter()
        .map(|(device_name, entries)| {
            let body = entries.iter().map(|(index, size)| format!("{index},{size}")).collect::<Vec<_>>().join(";");
            (device_name.clone(), body)
        })
        .collect::<Vec<_>>();
    write_config("secondary_screen_spacer_sizes.txt", format_secondary_screen_indexed(&per_screen));
}

/// Forme ÉCRITE de `parse_secondary_screen_indexed`, dont elle est le jumeau
/// exact : l'aller et le retour passent par ces deux fonctions, ce qui est la
/// seule façon de garantir qu'ils ne divergent pas — un désaccord entre eux ne
/// se verrait pas à la compilation, seulement à la perte silencieuse des
/// réglages d'un écran au redémarrage suivant.
fn format_secondary_screen_indexed(per_screen: &[(String, String)]) -> String {
    per_screen.iter().map(|(device_name, body)| format!("{device_name}|{body}")).collect::<Vec<_>>().join("\n")
}

/// Cotes des séparateurs de zone de CHAQUE écran secondaire (voir `ui::dock::
/// Dock::zone_heights`, `ui::dock::Icon::zone_height`) — jumeau PAR ÉCRAN de
/// `zone_heights.txt`, et le pendant de `load_secondary_screen_spacer_sizes`
/// pour les zones. **Fichier SÉPARÉ de celui-ci, et pour la même raison que
/// côté primaire** : les deux grandeurs sont indépendantes, et `load_spacer_
/// sizes` écarte une entrée mal formée — un champ de plus dans le même fichier
/// ferait perdre la taille d'un espace à cause d'une cote illisible.
pub fn load_secondary_screen_zone_heights() -> std::collections::HashMap<String, Vec<(usize, f32, f32)>> {
    zone_heights_from_indexed(read_secondary_screen_indexed("secondary_screen_zone_heights.txt"))
}

/// Cœur PUR de `load_secondary_screen_zone_heights` : il faut les DEUX champs
/// (cote ET vide devant, les deux moitiés d'`Icon::zone_height`/`zone_lead` —
/// voir `save_zone_heights`, même format). Une entrée qui n'en a qu'un est
/// écartée : la lire de travers donnerait une cote fausse, ce qui est pire que
/// de ne rien restaurer.
fn zone_heights_from_indexed(
    parsed: std::collections::HashMap<String, Vec<(usize, Vec<f32>)>>,
) -> std::collections::HashMap<String, Vec<(usize, f32, f32)>> {
    parsed
        .into_iter()
        .map(|(device_name, entries)| {
            let heights = entries
                .into_iter()
                .filter_map(|(index, values)| {
                    let [height, lead] = values.as_slice() else { return None };
                    Some((index, *height, *lead))
                })
                .collect();
            (device_name, heights)
        })
        .collect()
}

/// Sauvegarde les cotes de zones de chaque écran secondaire (voir
/// `load_secondary_screen_zone_heights`).
pub fn save_secondary_screen_zone_heights(heights: &std::collections::HashMap<String, Vec<(usize, f32, f32)>>) {
    let per_screen = heights
        .iter()
        .map(|(device_name, entries)| {
            let body = entries
                .iter()
                .map(|(index, height, lead)| format!("{index},{height},{lead}"))
                .collect::<Vec<_>>()
                .join(";");
            (device_name.clone(), body)
        })
        .collect::<Vec<_>>();
    write_config("secondary_screen_zone_heights.txt", format_secondary_screen_indexed(&per_screen));
}

/// Facteur d'échelle des popups CEF de type "menu" (clic droit, son, réseau,
/// voicemeeter, presse-papiers rapide, calendrier, tailles de barre/
/// séparateur — voir `win32::menu::menu_scale`, réglable via le curseur
/// "Taille des menus" du panneau de réglages) : `1.0` = taille actuelle,
/// déjà zoomée ×3 (voir `win32::cef_popup::CONTENT_ZOOM_FACTOR`) — PAS la
/// taille "native" pré-CEF d'avant la Phase 7 (voir `win32::menu::
/// MENU_SCALE`, doc de champ, pour pourquoi ce point neutre a changé).
/// `None` si jamais personnalisé : `1.0`, pour ne rien changer tant que
/// l'utilisateur n'a pas lui-même touché au curseur.
pub fn load_menu_scale() -> Option<f32> {
    read_config_floats("menu_scale.txt")?.first().copied()
}

/// Sauvegarde le facteur d'échelle des menus natifs, repris au prochain
/// lancement. Même garanties (best-effort) que `save_dash_tint`.
pub fn save_menu_scale(scale: f32) {
    write_config("menu_scale.txt", scale.to_string());
}

/// Facteur d'échelle de l'infobulle d'icône (voir `win32::tooltip::
/// tooltip_scale`, réglable via le curseur "Taille des infobulles" du
/// panneau de réglages — DEMANDE EXPLICITE "donnes la possibilité à
/// l'utilisateur de régler la taille de ces popup") : `1.0` = taille
/// actuelle (texte 16px). `None` si jamais personnalisé.
pub fn load_tooltip_scale() -> Option<f32> {
    read_config_floats("tooltip_scale.txt")?.first().copied()
}

/// Sauvegarde le facteur d'échelle de l'infobulle, repris au prochain
/// lancement. Même garanties (best-effort) que `save_dash_tint`.
pub fn save_tooltip_scale(scale: f32) {
    write_config("tooltip_scale.txt", scale.to_string());
}

/// Délai de survol des infobulles, en SECONDES (voir
/// `win32::tooltip::set_hover_delay_secs`, qui le borne) — même format qu'un
/// fichier de flottants, comme `tooltip_scale.txt` juste au-dessus. `None` tant
/// que l'utilisateur n'a rien réglé : `app::run` applique alors le défaut du
/// module, jamais une valeur recopiée ici.
/// Vitesses de défilement à la molette, en pixels par cran — `[gauche, droite]`,
/// dans l'ordre de `ui::dock::side_slot` (voir `ui::dock::scroll_step_px`, qui
/// les borne). Même forme et mêmes garanties que `load_bar_thicknesses` (un
/// fichier de flottants, une valeur par barre) : `None` tant que l'utilisateur
/// n'a rien réglé, et `app::run` applique alors les défauts du module.
pub fn load_scroll_steps() -> Option<[f32; 2]> {
    let values = read_config_floats("scroll_steps.txt")?;
    Some([*values.first()?, *values.get(1).unwrap_or(&values[0])])
}

/// Sauvegarde les vitesses de défilement (voir `load_scroll_steps`), reprises au
/// prochain lancement. Mêmes garanties (best-effort) que `save_tooltip_scale`.
pub fn save_scroll_steps(steps: [f32; 2]) {
    // VIRGULE, jamais un retour à la ligne : c'est le séparateur que
    // `parse_config_floats` lit (voir sa doc, et le défaut qu'un autre choix a
    // causé — les valeurs n'étaient jamais relues).
    write_config("scroll_steps.txt", format!("{},{}", steps[0], steps[1]));
}

/// **Le format des fichiers de flottants, tenu par un aller-retour** — c'est le
/// test qui manquait : `save_scroll_steps` écrivait deux LIGNES, le lecteur
/// attendait des VIRGULES, et le fichier était ignoré en silence (DÉFAUT RÉEL
/// SIGNALÉ : « les valeurs de défilement à la molette ne se sauvegardent pas »).
#[cfg(test)]
mod float_format_tests {
    use super::parse_config_floats;

    #[test]
    fn floats_round_trip_through_the_shared_separator() {
        // CE QUE L'ÉCRIVAIN PRODUIT (voir `save_scroll_steps`) se relit tel quel.
        assert_eq!(parse_config_floats(&format!("{},{}", 96.0, 48.0)), vec![96.0, 48.0]);
        // Une valeur seule (l'autre barre jamais réglée) reste lisible.
        assert_eq!(parse_config_floats("96"), vec![96.0]);
        // Tolérance : espaces et fin de ligne autour des valeurs.
        assert_eq!(parse_config_floats(" 96 , 48 \n"), vec![96.0, 48.0]);
        // Un champ illisible est écarté SEUL, ses voisins survivent.
        assert_eq!(parse_config_floats("96,illisible,48"), vec![96.0, 48.0]);
        // Un fichier vide ne rend AUCUNE valeur : l'appelant applique son défaut,
        // il ne lit pas un zéro que personne n'a écrit.
        assert!(parse_config_floats("").is_empty());
        assert!(parse_config_floats("\n").is_empty());
    }

    /// Les tirages s'écrivent au format commun ET l'ancien fichier à une valeur
    /// par ligne (écrit jusqu'au 2026-09-25) se relit encore.
    #[test]
    fn tirages_round_trip_and_legacy_lines_still_read() {
        assert_eq!(super::parse_tirages(&super::format_config_floats(&[0.0, 0.5, 1.0])), vec![0.0, 0.5, 1.0]);
        assert_eq!(super::parse_tirages("0\n0.5\n0"), vec![0.0, 0.5, 0.0]);
        assert_eq!(super::parse_tirages("0\r\n0.5\r\n0\r\n"), vec![0.0, 0.5, 0.0]);
    }
}

/// Les DEUX fichiers de séparateurs de rangée — leur format, et surtout ce qu'ils
/// font d'un fichier écrit par une version ANTÉRIEURE.
///
/// **Testés par leurs jumelles PURES**, jamais par les fonctions d'E/S : celles-ci
/// lisent `%APPDATA%`, et un test qui dépendrait de la configuration de la machine
/// ne prouverait rien sur le format (même raison que `float_format_tests`).
#[cfg(test)]
mod slot_format_tests {
    use super::*;
    use crate::ui::topbar;

    /// **L'ANCIEN FORMAT SE RELIT TEL QUEL** — une seule largeur par intervalle.
    /// C'est la garantie de non-migration, et elle vaut aussi pour une ligne
    /// abîmée : les largeurs sont des champs, écartés SEULS (le nom, lui, reste un
    /// verdict sur la ligne entière).
    #[test]
    fn the_old_primary_format_still_reads_as_a_single_separator() {
        let parsed = parse_top_slots("apres-les-pastilles,71\nafficher,45");
        assert_eq!(
            parsed,
            vec![("apres-les-pastilles".to_string(), vec![71.0]), ("afficher".to_string(), vec![45.0])],
        );

        // Le nouveau format, plusieurs largeurs sur une ligne.
        assert_eq!(
            parse_top_slots("apres-les-pastilles,40,25,60"),
            vec![("apres-les-pastilles".to_string(), vec![40.0, 25.0, 60.0])],
        );

        // Un champ illisible est écarté SEUL — la ligne est devenue un CONTENEUR
        // d'enregistrements, donc la règle « ligne entière » ne s'applique plus
        // qu'au nom (voir `parse_top_slots`, doc).
        assert_eq!(parse_top_slots("apres-les-pastilles,45,xx,60"), vec![("apres-les-pastilles".to_string(), vec![45.0, 60.0])]);

        // Une ligne sans nom, ou sans rien après la virgule, ne produit pas de
        // séparateur — et surtout pas un zéro inventé.
        assert!(parse_top_slots(",45").is_empty(), "un nom vide n'est pas un intervalle");
        assert!(parse_top_slots("sansvirgule").is_empty());
        assert_eq!(parse_top_slots("apres-les-pastilles,"), vec![("apres-les-pastilles".to_string(), Vec::new())]);
    }

    /// L'aller et le retour passent par les DEUX jumelles — c'est la seule façon
    /// de garantir qu'ils ne divergent pas (même discipline que
    /// `format_secondary_screen_indexed`).
    #[test]
    fn the_primary_file_round_trips() {
        let written = vec![
            ("apres-les-pastilles".to_string(), vec![45.0, 40.0, 40.0]),
            ("apres-afficher-le-bureau".to_string(), vec![71.0]),
        ];
        assert_eq!(parse_top_slots(&format_top_slots(&written)), written);
    }

    /// **LE FICHIER PAR ÉCRAN PORTE UNE LIGNE PAR (écran, intervalle)** — deux
    /// lignes du même écran se REJOIGNENT, et un intervalle absent reste vide.
    #[test]
    fn the_per_screen_file_merges_lines_of_the_same_screen() {
        let parsed = parse_secondary_slot_widths(
            "\\\\.\\DISPLAY2|apres-les-pastilles,40,25\n\\\\.\\DISPLAY2|apres-afficher-le-bureau,71\n\\\\.\\DISPLAY3|apres-les-pastilles,30",
        );

        let display2 = parsed[r"\\.\DISPLAY2"];
        assert_eq!(topbar::occupied_widths(display2, topbar::TopSlot::AfterDots).collect::<Vec<f32>>(), vec![40.0, 25.0]);
        assert_eq!(topbar::interval_width(display2, topbar::TopSlot::AfterShowDesktop), 71.0, "la seconde ligne a rejoint la première");
        assert_eq!(topbar::interval_width(display2, topbar::TopSlot::AfterMedia), 0.0, "intervalle jamais cité");
        assert_eq!(topbar::interval_width(parsed[r"\\.\DISPLAY3"], topbar::TopSlot::AfterDots), 30.0);

        // Un intervalle LIBRE n'écrit pas de ligne — absent et vide sont la même
        // chose, et c'est ce qui garde vraie la règle « un écran sans réglage n'a
        // aucun séparateur ».
        let written = format_secondary_slot_widths(&parsed);
        assert_eq!(written.lines().filter(|line| line.starts_with(r"\\.\DISPLAY2")).count(), 2);
        assert_eq!(parse_secondary_slot_widths(&written), parsed, "aller-retour");
    }

    /// **L'ANCIEN FORMAT PAR ÉCRAN EST ENCORE LU**, et c'est une MESURE, pas une
    /// précaution : le fichier existait sur disque avec un écran réglé
    /// (`\\.\DISPLAY2,40,40,0`) quand ce format a changé. Les deux formes ne
    /// peuvent pas être confondues — la nouvelle porte un `|`.
    #[test]
    fn the_old_per_screen_format_is_still_read() {
        let parsed = parse_secondary_slot_widths(r"\\.\DISPLAY2,40,40,0");

        let display2 = parsed[r"\\.\DISPLAY2"];
        assert_eq!(topbar::interval_width(display2, topbar::TopSlot::AfterShowDesktop), 40.0);
        assert_eq!(topbar::interval_width(display2, topbar::TopSlot::AfterDots), 40.0);
        assert_eq!(topbar::interval_width(display2, topbar::TopSlot::AfterMedia), 0.0, "le zéro n'est pas un séparateur");

        // Réécrit, il l'est dans la forme NOUVELLE — le fichier se migre donc tout
        // seul, et il n'y a jamais deux formats vivants.
        let rewritten = format_secondary_slot_widths(&parsed);
        assert!(rewritten.contains('|'), "{rewritten}");
        assert_eq!(parse_secondary_slot_widths(&rewritten), parsed, "et la relecture donne la même chose");
    }
}

/// INTERVALLES OCCUPÉS de la rangée de widgets de la barre du haut —
/// `(nom de l'intervalle, largeurs en pixels)`, une ligne `nom,w1,w2,…` par
/// intervalle (voir `ui::topbar::TopSlot`). Un intervalle libre n'a pas de ligne :
/// absent et « aucun séparateur » sont la même chose, et un fichier qui les
/// distinguerait serait plus long sans rien dire de plus.
///
/// **PLUSIEURS LARGEURS PAR LIGNE depuis le 2026-09-19** — DEMANDE EXPLICITE :
/// « je n'arrive pas à ajouter plus d'un séparateur entre le widget pour afficher
/// le bureau et la bascule de bureau ». L'ancien format (`nom,45`) se relit tel
/// quel comme une liste d'un élément, donc RIEN À MIGRER — voir `parse_top_slots`,
/// qui porte la règle de format et pourquoi elle a changé de camp.
///
/// **Le NOM, jamais un rang** : réordonner `TOP_SLOTS` un jour ne doit pas
/// déplacer les séparateurs de quelqu'un. Un nom inconnu (fichier d'une version
/// future) est ÉCARTÉ, ligne à ligne — même garantie que `load_zone_names`.
pub fn load_top_slots() -> Vec<(String, Vec<f32>)> {
    let Some(path) = config_path("top_slots.txt") else { return Vec::new() };
    let Ok(content) = fs::read_to_string(path) else { return Vec::new() };
    parse_top_slots(&content)
}

/// Décode `top_slots.txt` — une ligne par INTERVALLE occupé de la barre PRIMAIRE,
/// `nom,w1,w2,…` (voir `ui::topbar::TopSlot`).
///
/// **LA RÈGLE DE FORMAT CHANGE DE CAMP ICI, délibérément.** Le nom reste un
/// verdict sur la LIGNE ENTIÈRE (vide ⇒ ligne écartée), mais les LARGEURS sont des
/// champs écartés SEULS, par `parse_config_floats`, l'helper sanctionné. La raison
/// est la règle que ce fichier applique partout : quand la ligne EST
/// l'enregistrement, une ligne abîmée se perd entière ; quand elle n'en est plus
/// qu'un CONTENEUR, un champ abîmé ne coûte que lui. Depuis qu'un intervalle porte
/// une LISTE de largeurs, la ligne est devenue un conteneur.
///
/// **L'ancien format se relit tel quel** : `apres-les-pastilles,45` donne
/// `[45.0]`. Rien à migrer, et c'est une propriété à garder — l'ancien lecteur
/// ÉCARTAIT la ligne entière dès qu'un champ de plus apparaissait, donc lecteur
/// et écrivain devaient changer ENSEMBLE.
fn parse_top_slots(content: &str) -> Vec<(String, Vec<f32>)> {
    content
        .lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(',')?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            Some((name.to_string(), parse_config_floats(rest)))
        })
        .collect()
}

/// Encode `top_slots.txt` (voir `parse_top_slots`) — paire avec lui, jamais l'un
/// sans l'autre.
fn format_top_slots(slots: &[(String, Vec<f32>)]) -> String {
    slots
        .iter()
        .map(|(name, widths)| format!("{name},{}", format_config_floats(widths)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Sauvegarde les emplacements occupés (voir `load_top_slots`), repris au
/// prochain lancement. Mêmes garanties (best-effort) que `save_zone_names`.
pub fn save_top_slots(slots: &[(String, Vec<f32>)]) {
    write_config("top_slots.txt", format_top_slots(slots));
}

pub fn load_tooltip_delay_secs() -> Option<f32> {
    read_config_floats("tooltip_delay.txt")?.first().copied()
}

/// Sauvegarde le délai de survol des infobulles, repris au prochain lancement.
/// Mêmes garanties (best-effort) que `save_tooltip_scale`.
pub fn save_tooltip_delay_secs(secs: f32) {
    write_config("tooltip_delay.txt", secs.to_string());
}

/// Échelle des icônes des tuiles du Dash — voir `win32::cef_dash::
/// icon_scale`, réglable via le curseur "Icônes" (section "Taille du
/// contenu des dashs" du panneau de réglages). `None` si jamais
/// personnalisé : `1.0` (taille d'origine, voir `assets/popups/dash.html`).
/// Séparée de `load_dash_label_scale` : DEMANDE EXPLICITE de pouvoir régler
/// chaque élément (icône, étiquette) indépendamment plutôt qu'un seul
/// facteur commun (voir l'historique de `save_dash_tile_scale`, remplacée
/// par ce réglage plus fin).
pub fn load_dash_icon_scale() -> Option<f32> {
    read_config_floats("dash_icon_scale.txt")?.first().copied()
}

/// Sauvegarde l'échelle des icônes du Dash, reprise au prochain lancement.
/// Même garanties (best-effort) que `save_dash_tint`.
pub fn save_dash_icon_scale(scale: f32) {
    write_config("dash_icon_scale.txt", scale.to_string());
}

/// Échelle du texte (étiquette sous l'icône) des tuiles du Dash — voir
/// `load_dash_icon_scale`, doc détaillée sur pourquoi ce réglage est
/// séparé.
pub fn load_dash_label_scale() -> Option<f32> {
    read_config_floats("dash_label_scale.txt")?.first().copied()
}

/// Sauvegarde l'échelle du texte des tuiles du Dash, reprise au prochain
/// lancement. Même garanties (best-effort) que `save_dash_tint`.
pub fn save_dash_label_scale(scale: f32) {
    write_config("dash_label_scale.txt", scale.to_string());
}

/// Échelle du panneau de catégories du Dash (colonne de droite + en-têtes
/// de groupe dans la grille, voir `win32::cef_dash::category_scale`) —
/// réglable via le curseur "Catégories" (section "Taille du contenu des
/// dashs" du panneau de réglages). BUG RÉEL SIGNALÉ : ce panneau n'avait
/// jusqu'ici AUCUN réglage de taille, contrairement aux icônes/étiquettes
/// des tuiles (`load_dash_icon_scale`/`load_dash_label_scale`) — même
/// raison d'être séparée d'elles (élément visuel distinct). Même défaut
/// (`None` → `1.0`).
pub fn load_dash_category_scale() -> Option<f32> {
    read_config_floats("dash_category_scale.txt")?.first().copied()
}

/// Sauvegarde l'échelle du panneau de catégories du Dash, reprise au
/// prochain lancement. Même garanties (best-effort) que `save_dash_tint`.
pub fn save_dash_category_scale(scale: f32) {
    write_config("dash_category_scale.txt", scale.to_string());
}

/// Échelle des miniatures (image/texte copié) de la visionneuse du
/// presse-papiers — voir `win32::cef_clipboard::thumb_scale`, réglable via
/// le curseur "Miniatures". Même raison d'être séparée de
/// `load_clipboard_text_scale` que `load_dash_icon_scale`/
/// `load_dash_label_scale` ci-dessus. Même défaut (`None` → `1.0`).
pub fn load_clipboard_thumb_scale() -> Option<f32> {
    read_config_floats("clipboard_thumb_scale.txt")?.first().copied()
}

/// Sauvegarde l'échelle des miniatures du presse-papiers, reprise au
/// prochain lancement. Même garanties (best-effort) que `save_dash_tint`.
pub fn save_clipboard_thumb_scale(scale: f32) {
    write_config("clipboard_thumb_scale.txt", scale.to_string());
}

/// Échelle du texte (aperçu et étiquette d'image) des éléments du
/// presse-papiers — voir `load_clipboard_thumb_scale`, doc détaillée.
pub fn load_clipboard_text_scale() -> Option<f32> {
    read_config_floats("clipboard_text_scale.txt")?.first().copied()
}

/// Sauvegarde l'échelle du texte des éléments du presse-papiers, reprise au
/// prochain lancement. Même garanties (best-effort) que `save_dash_tint`.
pub fn save_clipboard_text_scale(scale: f32) {
    write_config("clipboard_text_scale.txt", scale.to_string());
}

/// Échelle du contenu du panneau de Réglages lui-même (troisième fenêtre de
/// la "zone libre" avec le Dash et le Presse-papiers, voir `win32::
/// cef_settings::content_scale`) — réglable via le curseur "Contenu" de la
/// sous-section "Réglages". Contrairement à `load_dash_icon_scale`/
/// `load_dash_label_scale`, un SEUL facteur ici : ce panneau n'a pas
/// d'équivalent icône/texte séparé (voir `assets/popups/settings.html`,
/// appliqué via `zoom` CSS sur tout le contenu défilant à la fois). Même
/// défaut (`None` → `1.0`) que `load_dash_icon_scale`.
pub fn load_settings_content_scale() -> Option<f32> {
    read_config_floats("settings_content_scale.txt")?.first().copied()
}

/// Sauvegarde l'échelle du contenu du panneau de Réglages, reprise au
/// prochain lancement. Même garanties (best-effort) que `save_dash_tint`.
pub fn save_settings_content_scale(scale: f32) {
    write_config("settings_content_scale.txt", scale.to_string());
}

/// Largeur de la colonne des réglages du panneau Réglages, en fraction de sa
/// place (voir `win32::cef_settings::CONTENT_WIDTH`). `None` → pleine largeur.
pub fn load_settings_content_width() -> Option<f32> {
    read_config_floats("settings_content_width.txt")?.first().copied()
}

/// Voir `load_settings_content_width`. Mêmes garanties (best-effort) que
/// `save_settings_content_scale`.
pub fn save_settings_content_width(fraction: f32) {
    write_config("settings_content_width.txt", fraction.to_string());
}

/// Active/désactive la mini-barre d'horloge affichée par-dessus une
/// application plein écran (voir `win32::fullscreen_clock`) — DEMANDE
/// EXPLICITE "laisser la possibilité à l'utilisateur d'activer cette
/// fonction". `false` par défaut (`None` ici) : tant que l'utilisateur ne
/// l'a pas activée, le comportement reste celui d'avant ce réglage (simple
/// masquage total du dock, voir `Window::sync_fullscreen_visibility`).
pub fn load_fullscreen_clock_enabled() -> bool {
    read_config_floats("fullscreen_clock_enabled.txt").and_then(|v| v.first().copied()).map(|v| v != 0.0).unwrap_or(false)
}

/// Sauvegarde l'activation de la mini-barre d'horloge plein écran, reprise
/// au prochain lancement. Même garanties (best-effort) que
/// `save_feature_enabled`.
pub fn save_fullscreen_clock_enabled(enabled: bool) {
    write_config("fullscreen_clock_enabled.txt", if enabled { "1" } else { "0" }.to_string());
}

/// Emplacement de la mini-barre d'horloge plein écran — `true` = en haut de
/// l'écran, `false` = en bas (voir `win32::fullscreen_clock::is_position_top`)
/// — DEMANDE EXPLICITE "choisir l'emplacement". `true` (haut) par défaut :
/// même bord que la barre du haut habituelle du dock qu'elle remplace.
pub fn load_fullscreen_clock_position_top() -> bool {
    read_config_floats("fullscreen_clock_position_top.txt").and_then(|v| v.first().copied()).map(|v| v != 0.0).unwrap_or(true)
}

/// Sauvegarde l'emplacement de la mini-barre d'horloge plein écran, repris
/// au prochain lancement. Mêmes garanties (best-effort) que
/// `save_fullscreen_clock_enabled`.
pub fn save_fullscreen_clock_position_top(top: bool) {
    write_config("fullscreen_clock_position_top.txt", if top { "1" } else { "0" }.to_string());
}

/// Facteur d'échelle de la mini-barre d'horloge plein écran ELLE-MÊME
/// (hauteur/marge intérieure, voir `win32::fullscreen_clock::bar_scale`) —
/// DEMANDE EXPLICITE "régler la taille de cette barre". `None` si jamais
/// personnalisé : `1.0` (taille de départ).
pub fn load_fullscreen_clock_bar_scale() -> Option<f32> {
    read_config_floats("fullscreen_clock_bar_scale.txt")?.first().copied()
}

/// Sauvegarde le facteur d'échelle de la mini-barre d'horloge plein écran,
/// repris au prochain lancement. Même garanties (best-effort) que
/// `save_dash_tint`.
pub fn save_fullscreen_clock_bar_scale(scale: f32) {
    write_config("fullscreen_clock_bar_scale.txt", scale.to_string());
}

/// Facteur d'échelle du TEXTE de l'horloge dans la mini-barre plein écran
/// (voir `win32::fullscreen_clock::text_scale`), INDÉPENDANT de
/// `load_fullscreen_clock_bar_scale` — DEMANDE EXPLICITE "régler la taille
/// de cette barre ET du texte de cette horloge" (deux curseurs distincts).
/// `None` si jamais personnalisé : `1.0`.
pub fn load_fullscreen_clock_text_scale() -> Option<f32> {
    read_config_floats("fullscreen_clock_text_scale.txt")?.first().copied()
}

/// Sauvegarde le facteur d'échelle du texte de la mini-barre d'horloge plein
/// écran, repris au prochain lancement. Même garanties (best-effort) que
/// `save_dash_tint`.
pub fn save_fullscreen_clock_text_scale(scale: f32) {
    write_config("fullscreen_clock_text_scale.txt", scale.to_string());
}

/// Échelle du texte des notes (dash "Notes") — DEMANDE EXPLICITE ("ajoutes
/// la possibilité de modification des réglages du dash notes dans taille du
/// contenu des dashs"). Voir `win32::cef_notes::text_scale`, réglable via le
/// curseur "Texte" de la sous-section "Notes". Même raison d'être séparée de
/// `load_notes_image_scale` que `load_clipboard_thumb_scale`/
/// `load_clipboard_text_scale` (texte et images des notes sont réglables
/// indépendamment). Même défaut (`None` → `1.0`).
pub fn load_notes_text_scale() -> Option<f32> {
    read_config_floats("notes_text_scale.txt")?.first().copied()
}

/// Sauvegarde l'échelle du texte des notes, reprise au prochain lancement.
/// Même garanties (best-effort) que `save_dash_tint`.
pub fn save_notes_text_scale(scale: f32) {
    write_config("notes_text_scale.txt", scale.to_string());
}

/// Échelle des miniatures d'image jointes aux notes — voir
/// `load_notes_text_scale`, doc détaillée.
pub fn load_notes_image_scale() -> Option<f32> {
    read_config_floats("notes_image_scale.txt")?.first().copied()
}

/// Facteur « Taille du contenu » d'un panneau de `win32::taille_panneaux::
/// PANNEAUX` — un fichier par panneau, `<panneau>_content_scale.txt`.
pub fn load_panel_content_scale(panneau: &str) -> Option<f32> {
    read_config_floats(&format!("{panneau}_content_scale.txt"))?.first().copied()
}

/// Sauvegarde le facteur de `load_panel_content_scale`. Même garanties
/// (best-effort) que `save_dash_tint`.
pub fn save_panel_content_scale(panneau: &str, scale: f32) {
    write_config(&format!("{panneau}_content_scale.txt"), scale.to_string());
}

/// **LES FORCES DU THÈME** — voir `ui::color::ajuster_theme`, qui porte ce
/// qu'elles font.
///
/// ⚠️ **DEUX FLOTTANTS SÉPARÉS, PAS UN COUPLE DANS UN FICHIER** : chacun porte
/// son nom, donc relire l'un n'oblige pas à comprendre l'autre, et un fichier
/// illisible n'emporte que le sien.
///
/// ⛔ **DE NOUVEAUX NOMS DE FICHIER** : les anciens `theme_saturation_plancher`
/// / `theme_luminosite_plafond` tenaient des BORNES (0,55, 0,45…) ; relues comme
/// des forces, elles délaveraient et assombriraient le thème au lancement.
pub fn load_theme_force_saturation() -> Option<f32> {
    read_config_floats("theme_force_saturation.txt")?.first().copied()
}

pub fn save_theme_force_saturation(valeur: f32) {
    write_config("theme_force_saturation.txt", valeur.to_string());
}

pub fn load_theme_force_luminosite() -> Option<f32> {
    read_config_floats("theme_force_luminosite.txt")?.first().copied()
}

pub fn save_theme_force_luminosite(valeur: f32) {
    write_config("theme_force_luminosite.txt", valeur.to_string());
}

/// Sauvegarde l'échelle des miniatures d'image des notes, reprise au
/// prochain lancement. Même garanties (best-effort) que `save_dash_tint`.
pub fn save_notes_image_scale(scale: f32) {
    write_config("notes_image_scale.txt", scale.to_string());
}

/// Rayon des coins arrondis des menus contextuels natifs (px, avant mise à
/// l'échelle — voir `win32::menu::menu_corner_radius`, réglable via le
/// curseur "Arrondi des menus"). `None` si jamais personnalisé, ou si un
/// style a été choisi depuis : le rayon suit alors celui du style.
pub fn load_menu_corner_radius() -> Option<f32> {
    read_config_floats("menu_corner_radius.txt")?.first().copied()
}

/// Sauvegarde le rayon de coin des menus natifs, repris au prochain
/// lancement. Même garanties (best-effort) que `save_dash_tint`.
pub fn save_menu_corner_radius(radius: f32) {
    write_config("menu_corner_radius.txt", radius.to_string());
}

/// Oublie le rayon personnalisé — choisir un style ramène le curseur à la
/// valeur du style (demande explicite).
pub fn clear_menu_corner_radius() {
    let Some(path) = config_path("menu_corner_radius.txt") else { return };
    let _ = fs::remove_file(path);
}

/// Profondeur de la pyramide de flou Kawase (voir `gfx::blur::blur_passes`/
/// `set_blur_passes`, réglable via le curseur "Intensité du flou" du
/// panneau de réglages) — plus de passes = flou plus prononcé (et un peu
/// plus cher en GPU). Stocké en `f32` comme les autres réglages numériques
/// de ce fichier ; à arrondir en `u32` par l'appelant. `None` si jamais
/// personnalisé : `6` (valeur d'origine, figée en constante avant ce
/// curseur — voir `gfx::blur::DEFAULT_BLUR_PASSES`).
pub fn load_blur_passes() -> Option<f32> {
    read_config_floats("blur_passes.txt")?.first().copied()
}

/// Sauvegarde le nombre de passes de flou, repris au prochain lancement.
/// Même garanties (best-effort) que `save_dash_tint`.
pub fn save_blur_passes(passes: u32) {
    write_config("blur_passes.txt", passes.to_string());
}

/// Matériau de flou choisi (voir `gfx::material::MATERIALS`) — un index
/// dans cette table, réglable via le sélecteur "Matériau" du panneau de
/// réglages — SÉPARÉ du curseur "Intensité du flou"/`load_blur_passes"
/// ci-dessus : l'un change la FORCE du flou (nombre de passes), l'autre
/// l'IDENTITÉ du matériau (algorithme + post-composition, voir la doc de
/// `gfx::material`). Stocké en `f32` comme les autres réglages numériques de
/// ce fichier ; à arrondir en `usize` par l'appelant. `None` si jamais
/// personnalisé : `gfx::material::DEFAULT_MATERIAL_INDEX` ("Acrylique").
///
/// Remplace l'ancien fichier `blur_style.txt` (préréglages "Net"/"Doux"/
/// "Équilibré"/"Diffus", un simple axe net↔diffus sur un seul algorithme) —
/// délibérément SANS migration : les deux axes ne sont pas commensurables
/// (l'ancien index 0-3 encodait une netteté, le nouveau 0-4 encode une
/// identité de matériau entière). Un utilisateur existant repart sur
/// "Acrylique", choisi précisément parce qu'il reproduit le rendu par
/// défaut d'avant cette migration (voir la doc de `gfx::material::
/// MATERIALS`). L'ancien `blur_style.txt`, s'il existe encore sur disque,
/// est laissé tel quel — un fichier orphelin de quelques octets est
/// inoffensif, le supprimer n'apporterait aucun bénéfice.
pub fn load_blur_material() -> Option<f32> {
    read_config_floats("blur_material.txt")?.first().copied()
}

/// Sauvegarde le matériau de flou choisi, repris au prochain lancement.
/// Même garanties (best-effort) que `save_dash_tint`.
pub fn save_blur_material(index: usize) {
    write_config("blur_material.txt", index.to_string());
}

/// Transparence des menus contextuels (voir `win32::menu::menu_tint`,
/// réglable via le curseur "Transparence" de la section "Menus contextuels"
/// du panneau de réglages) — INDÉPENDANTE de `load_dash_tint` (demande
/// explicite : ces menus courts sont une surface distincte du Dash/des
/// réglages/du presse-papiers, avec son propre curseur). `None` si jamais
/// personnalisée : `0.55` (même valeur de repli que `dash_tint`).
pub fn load_menu_tint() -> Option<f32> {
    read_config_floats("menu_tint.txt")?.first().copied()
}

/// Sauvegarde la transparence des menus contextuels, reprise au prochain
/// lancement. Mêmes garanties (best-effort) que `save_dash_tint`.
pub fn save_menu_tint(tint: f32) {
    write_config("menu_tint.txt", tint.to_string());
}

/// Une application épinglée par l'utilisateur via `config.toml` (voir
/// `load_pinned_apps`), ajoutée à la suite des icônes intégrées du dock —
/// façon fichier de configuration de Brain_Shell (Lua) ou Seelen UI (JSON),
/// en TOML ici pour rester dans l'écosystème Rust sans dépendance exotique.
pub struct PinnedApp {
    pub name: String,
    /// Le chemin de l'EXÉCUTABLE à lancer — **ou, quand `uri` est présente, le
    /// FICHIER D'ICÔNE** (voir ce champ) — **ou `None` pour une application du
    /// STORE**.
    ///
    /// ⚠️ **UNE APPLICATION UWP N'A NI EXÉCUTABLE NI FICHIER D'ICÔNE** : elle se
    /// lance par son identifiant (`shell:AppsFolder\<AUMID>`, dans `uri`) et son
    /// icône s'obtient par WinRT, pas sur le disque (voir `win32::uwp_apps`,
    /// `UwpApp::icon_rgba`, qui rend des pixels DÉJÀ décodés — un `.png` de paquet
    /// ne serait de toute façon pas lisible par `ExtractIconExW`, qui ne connaît
    /// que les exécutables et les `.ico`).
    ///
    /// L'icône est donc **redérivée au démarrage** depuis l'AUMID, jamais
    /// persistée : le fichier ne contient aucun pixel, et l'icône suit les mises à
    /// jour du paquet.
    pub path: Option<PathBuf>,
    /// **LA CIBLE DE LANCEMENT QUAND CE N'EST PAS UN EXÉCUTABLE.**
    ///
    /// Projection persistée de `win32::apps::AppLaunch` : `Some` pour une
    /// application lancée par **PROTOCOLE** (un jeu Steam — `steam://rungameid/…`
    /// — ou Epic), qui n'a **pas d'exécutable** à lancer mais dont le lanceur met
    /// en cache un `.ico` (voir `AppEntry::icon_path`, dont la doc décrit
    /// exactement ce cas). `path` porte alors **ce fichier d'icône**, seule chose
    /// que le dock sait dessiner, et le clic appelle `appicon::open(uri)` au lieu
    /// d'`appicon::launch`.
    ///
    /// ⚠️ **Les applications du STORE (UWP) restent hors de portée** : elles n'ont
    /// ni exécutable NI fichier d'icône — leur icône s'extrait de l'identifiant de
    /// paquet. Il faudrait un troisième mécanisme, qui n'existe pas ici.
    pub uri: Option<String>,
    /// **CETTE ENTRÉE EST UN LANCEUR, PAS L'APPLICATION ELLE-MÊME.**
    ///
    /// Un lanceur se contente de LANCER : aucune pastille « en cours
    /// d'exécution », aucun aperçu de fenêtres, et — la conséquence qui compte —
    /// **il ne revendique aucune fenêtre**, donc l'application lancée apparaît
    /// d'elle-même dans la zone des programmes lancés (voir
    /// `app::unlisted_open_apps`, qui écarte les fenêtres revendiquées par une
    /// épingle).
    ///
    /// DEMANDE EXPLICITE (2026-09-23) : « certains des programmes lancés depuis la
    /// zone Accès rapide […] se retrouvent avec la prévisualition et l'indicateur
    /// d'application alors que ça ne devrait être que des lanceurs simple ».
    ///
    /// ⚠️ **LE COMPORTEMENT EXISTAIT DÉJÀ, mais seulement pour les cibles de
    /// PROTOCOLE** (voir `uri`) : faute d'exécutable, le dock ne peut pas savoir
    /// quel processus correspond, et une épingle `steam://…` ne s'allume donc
    /// jamais — MESURÉ sur la machine, où `obs64.exe` figurait bien dans les
    /// applications ouvertes pendant que l'épingle OBS restait éteinte. Ce drapeau
    /// rend le même comportement DEMANDABLE pour une cible qui EST un exécutable
    /// (`inkscape.exe`), où le nom de processus, lui, serait dérivable — et où le
    /// dock le dérivait, d'où la pastille et l'aperçu que l'utilisateur refusait.
    ///
    /// **Rien ne change pour l'existant** : absent de `dock_pins.txt`, ce champ
    /// vaut `false`, donc une épingle écrite avant lui reste une épingle — et le
    /// fichier n'est même pas réécrit différemment (voir `save_extra_pin`).
    pub launcher: bool,
    /// **D'OÙ VIENT CETTE ÉPINGLE — ET DONC SI L'APPLICATION PEUT LA RETIRER.**
    ///
    /// Vrai pour une ligne de `dock_pins.txt` (voir `load_extra_pins`), faux pour
    /// une entrée de `config.toml` (voir `load_pinned_apps_from_config_toml`).
    ///
    /// DEMANDE EXPLICITE (2026-09-23) : le panneau des Réglages doit pouvoir
    /// **retirer un raccourci** depuis la liste du contenu d'une zone — et la
    /// seule chose qui l'en empêche, c'est l'origine : `config.toml` est écrit **à
    /// la main** et **l'application ne le réécrit JAMAIS**. Offrir le bouton sur
    /// une entrée qu'on ne peut pas retirer serait promettre une action qui
    /// échoue ; ce drapeau permet de ne l'offrir que là où elle aboutit (voir
    /// `config::remove_extra_pin`).
    pub from_extra: bool,
}

/// Relit les applications épinglées dans `%APPDATA%\NeuraDesk\config.toml`.
/// Écrit un modèle commenté (voir `DEFAULT_CONFIG_TOML`) s'il n'existe pas
/// encore, pour que l'utilisateur découvre la fonctionnalité sans avoir à
/// en connaître le format à l'avance. Best-effort partout : un fichier
/// absent, illisible ou mal formé donne une liste vide plutôt qu'un crash
/// ou un blocage du démarrage — un dock sans icône épinglée en plus reste
/// un dock parfaitement utilisable.
pub fn load_pinned_apps() -> Vec<PinnedApp> {
    let apps = load_pinned_apps_from_config_toml();
    // ⚠️ **L'ORDRE N'EST PAS COSMÉTIQUE — IL FIXE LES INDEX.** `Dock::new` crée
    // une place par épingle, à la suite de `PINNED_ICON_BASE`, et tout le reste
    // du dock se numérote à partir de là. Les épingles de `config.toml` viennent
    // donc EN PREMIER — celles que l'utilisateur a écrites à la main ne bougent
    // jamais — et celles ajoutées depuis le Dash ensuite. **Une épingle ajoutée
    // est ainsi toujours la DERNIÈRE**, ce qui rend son index au prochain
    // lancement PRÉVISIBLE : `PINNED_ICON_BASE + total - 1` (c'est ce que
    // `cef_dash::handle_add_to_zone` exploite pour écrire l'ordre).
    merge_pins(apps, load_extra_pins())
}

/// **LA FUSION DES ÉPINGLES, DANS L'ORDRE QUI FIXE LES INDEX DES ICÔNES.**
///
/// Extraite de `load_pinned_apps` pour être **testable sans toucher au disque** :
/// `config_path` n'a aucun point d'injection, et un test qui écrirait dans
/// `%APPDATA%\NeuraDesk` polluerait la configuration RÉELLE de l'utilisateur.
fn merge_pins(config_pins: Vec<PinnedApp>, extra: Vec<PinnedApp>) -> Vec<PinnedApp> {
    let mut apps = config_pins;
    apps.extend(extra);
    apps
}

#[cfg(test)]
mod merge_pins_tests {
    use super::*;

    fn pin(name: &str) -> PinnedApp {
        PinnedApp {
            name: name.to_string(),
            path: Some(PathBuf::from("C:/ailleurs/exemple.exe")),
            uri: None,
            launcher: false,
            from_extra: false,
        }
    }

    /// **L'ORDRE DE FUSION EST UN CONTRAT, PAS UN DÉTAIL DE STYLE.** L'index
    /// d'une icône épinglée vaut `PINNED_ICON_BASE + son rang dans cette liste` :
    /// la trier, ou mettre en tête les épingles ajoutées depuis le Dash,
    /// DÉPLACERAIT toutes les icônes épinglées — et rendrait faux l'`icon_order.txt`
    /// déjà enregistré, dont les valeurs désignent des places.
    #[test]
    fn the_pins_of_the_config_keep_the_first_places() {
        let fusion = merge_pins(vec![pin("Spotify"), pin("Steam")], vec![pin("Blender")]);
        let noms: Vec<&str> = fusion.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            noms,
            vec!["Spotify", "Steam", "Blender"],
            "celles écrites à la main dans config.toml gardent les PREMIÈRES places"
        );

        // Et une fusion sans épingles ajoutées ne réordonne rien : c'est ce qui
        // garantit qu'ajouter une application ne bouscule pas les existantes.
        let inchange = merge_pins(vec![pin("Steam"), pin("Spotify")], Vec::new());
        let noms: Vec<&str> = inchange.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(noms, vec!["Steam", "Spotify"], "aucun tri n'est appliqué au passage");
    }
}

fn load_pinned_apps_from_config_toml() -> Vec<PinnedApp> {
    let Some(path) = config_path("config.toml") else { return Vec::new() };

    if !path.exists() {
        write_config("config.toml", DEFAULT_CONFIG_TOML.to_string());
        return Vec::new();
    }

    let Ok(content) = fs::read_to_string(&path) else { return Vec::new() };
    let table = match content.parse::<toml::Table>() {
        Ok(table) => table,
        Err(err) => {
            eprintln!("[neuradesk] config.toml invalide, ignoré : {err}");
            return Vec::new();
        }
    };

    let Some(entries) = table.get("pinned_apps").and_then(|v| v.as_array()) else { return Vec::new() };
    entries
        .iter()
        .filter_map(|entry| {
            let name = entry.get("name")?.as_str()?.to_string();
            let path = entry.get("path")?.as_str()?.to_string();
            // Le `config.toml` écrit à la main ne porte que `name`/`path` : c'est
            // le format documenté, et il n'a pas à connaître les URI — celles-ci
            // viennent des épingles créées par l'application (`dock_pins.txt`).
            Some(PinnedApp { name, path: Some(PathBuf::from(path)), uri: None, launcher: false, from_extra: false })
        })
        .collect()
}

/// **LES ÉPINGLES AJOUTÉES DEPUIS LE DASH** — le pendant ÉCRIT de `config.toml`.
///
/// ⚠️ **POURQUOI UN FICHIER À PART, ET JAMAIS `config.toml`.** Ce dernier est un
/// fichier **commenté, destiné à la main** (voir `DEFAULT_CONFIG_TOML`) : le
/// relire puis le réécrire par la crate `toml` **effacerait ses commentaires**,
/// c'est-à-dire sa documentation. Les épingles créées par l'application vivent
/// donc ici, dans le style d'un petit fichier par réglage déjà employé partout
/// ailleurs (`zone_names.txt`, `icon_order.txt`…), et **`config.toml` n'est
/// jamais réécrit**.
///
/// Une ligne par application : `nom<TAB>chemin<TAB>uri<TAB>lanceur`. La
/// **tabulation**, et non la virgule comme `zone_names.txt` : un nom
/// d'application contient couramment une virgule (« WhatsApp, version bureau »),
/// un chemin Windows jamais de tabulation.
///
/// **La troisième colonne est OPTIONNELLE** — une ligne à deux colonnes se lit
/// comme une épingle sans URI, donc un exécutable classique. C'est ce qui permet
/// d'écrire un jeu Steam (`steam://rungameid/…`) sans changer le sens des lignes
/// déjà présentes.
///
/// **La quatrième l'est aussi, et pour la même raison** : `1` = un LANCEUR (voir
/// `PinnedApp::launcher`), toute autre valeur ou son absence = une épingle
/// ordinaire. Un fichier écrit avant elle se relit donc à l'identique.
pub fn load_extra_pins() -> Vec<PinnedApp> {
    let Some(path) = config_path("dock_pins.txt") else { return Vec::new() };
    let Ok(content) = fs::read_to_string(path) else { return Vec::new() };
    let mut pins: Vec<PinnedApp> = content.lines().filter_map(parse_extra_pin).collect();
    // ⛔ Une épingle d'un raccourci MSI enregistrée avant `apps::cible_msi` vise
    // son LEURRE (un .ico) : réparée une fois, et le fichier réécrit.
    if reparer_les_leurres_msi(&mut pins, crate::win32::apps::cible_reelle_d_un_leurre) {
        write_config("dock_pins.txt", extra_pins_to_file(&pins));
    }
    pins
}

/// Remplace le chemin de chaque épingle de `pins` qui vise un LEURRE MSI (voir
/// `apps::est_un_leurre_msi`) par ce que `resoudre` tire de ce leurre et du NOM
/// de l'épingle. Vrai si une a changé. Une épingle irréparable reste telle quelle.
fn reparer_les_leurres_msi(pins: &mut [PinnedApp], resoudre: impl Fn(&Path, &str) -> Option<PathBuf>) -> bool {
    let mut change = false;
    for pin in pins.iter_mut() {
        let Some(chemin) = pin.path.as_deref().filter(|p| crate::win32::apps::est_un_leurre_msi(p)) else { continue };
        if let Some(vrai) = resoudre(chemin, &pin.name) {
            pin.path = Some(vrai);
            change = true;
        }
    }
    change
}

/// **UNE LIGNE DE `dock_pins.txt` — fonction PURE, donc testable sans disque.**
///
/// Extraite de `load_extra_pins` pour cette seule raison : `config_path` n'a
/// aucun point d'injection, et un test qui écrirait dans `%APPDATA%\NeuraDesk`
/// polluerait la configuration RÉELLE de l'utilisateur (même raison que
/// `merge_pins`).
fn parse_extra_pin(line: &str) -> Option<PinnedApp> {
    // `splitn(4)` et non `splitn(3)` : avec trois, une éventuelle quatrième
    // colonne serait COLLÉE à la troisième, et l'URI d'une ligne écrite par une
    // version future se lirait « …\t1 ». Les trois premières colonnes se lisent
    // identiquement dans les deux formes.
    let mut champs = line.splitn(4, '\t').map(str::trim);
    let name = champs.next()?;
    if name.is_empty() {
        return None;
    }
    // Les DEUX colonnes suivantes sont optionnelles, et chacune peut être vide :
    // un chemin vide = une application du STORE (ni exécutable ni icône de
    // fichier, voir `PinnedApp::path`), une URI vide = un exécutable classique.
    let chemin = champs.next()?;
    let uri = champs.next().filter(|u| !u.is_empty()).map(str::to_string);
    let path = (!chemin.is_empty()).then(|| PathBuf::from(chemin));
    // `1` seulement : une colonne présente mais vide (une tabulation de trop) ne
    // fait PAS un lanceur — sans quoi un fichier réécrit par un outil qui ajoute
    // des tabulations changerait le comportement de toutes ses épingles.
    let launcher = champs.next().is_some_and(|l| l == "1");
    // `from_extra` est VRAI par construction : cette fonction ne lit QUE
    // `dock_pins.txt` (voir `PinnedApp::from_extra`).
    Some(PinnedApp { name: name.to_string(), path, uri, launcher, from_extra: true })
}

#[cfg(test)]
mod parse_extra_pin_tests {
    use super::*;

    /// **LE CONTRAT QUI PROTÈGE LES FICHIERS DÉJÀ ÉCRITS** : sans quatrième
    /// colonne, rien ne change — c'est ce qui permet d'ajouter le champ sans
    /// migrer la configuration de personne.
    #[test]
    fn three_columns_stay_a_plain_pin() {
        let pin = parse_extra_pin("Inkscape\tC:\\Outils\\inkscape.exe\t").expect("une ligne valide");
        assert_eq!(pin.name, "Inkscape");
        assert_eq!(pin.path.as_deref(), Some(std::path::Path::new("C:\\Outils\\inkscape.exe")));
        assert!(!pin.launcher, "sans quatrième colonne, l'épingle reste une épingle");
    }

    /// Et une ligne à DEUX colonnes aussi — le format accepté avant l'arrivée des
    /// URI (voir `load_extra_pins`).
    #[test]
    fn two_columns_still_mean_a_plain_executable() {
        let pin = parse_extra_pin("Inkscape\tC:\\Outils\\inkscape.exe").expect("une ligne valide");
        assert!(pin.uri.is_none());
        assert!(!pin.launcher);
    }

    #[test]
    fn a_fourth_column_of_one_makes_a_launcher() {
        let pin = parse_extra_pin("Inkscape\tC:\\Outils\\inkscape.exe\t\t1").expect("une ligne valide");
        assert!(pin.launcher);
        // Le même chemin SANS la colonne est une épingle : c'est la colonne, et
        // rien d'autre, qui décide (voir le test suivant).
        assert!(!parse_extra_pin("Inkscape\tC:\\Outils\\inkscape.exe\t\t").expect("une ligne valide").launcher);
    }

    /// ⚠️ **UNE COLONNE VIDE N'EST PAS UN LANCEUR.** Sans cette borne, un fichier
    /// réécrit par un outil qui ajoute des tabulations changerait le comportement
    /// de TOUTES ses épingles d'un coup — le genre de panne qu'on ne relie jamais
    /// à sa cause.
    #[test]
    fn an_empty_fourth_column_is_not_a_launcher() {
        let pin = parse_extra_pin("Inkscape\tC:\\Outils\\inkscape.exe\t\t").expect("une ligne valide");
        assert!(!pin.launcher);
    }

    /// Une URI ET un lanceur peuvent coexister (un jeu Steam qu'on veut inerte) :
    /// `path` porte alors le fichier d'icône, et les deux champs se lisent.
    #[test]
    fn a_uri_and_a_launcher_can_coexist() {
        let pin = parse_extra_pin("OBS Studio\tC:\\jeu\\c807ea59.ico\tsteam://rungameid/1905180\t1")
            .expect("une ligne valide");
        assert_eq!(pin.uri.as_deref(), Some("steam://rungameid/1905180"));
        assert!(pin.path.is_some(), "le chemin porte le FICHIER D'ICÔNE du lanceur");
        assert!(pin.launcher);
    }

    #[test]
    fn a_nameless_line_is_ignored() {
        assert!(parse_extra_pin("").is_none());
        assert!(parse_extra_pin("\tC:\\Outils\\inkscape.exe\t").is_none());
    }

    /// ⛔ Une épingle MSI enregistrée avec son LEURRE retrouve son programme ;
    /// les autres, et un leurre irréparable, ne bougent pas.
    #[test]
    fn une_epingle_msi_quitte_son_leurre() {
        let lignes = [
            "DesignCraft\tC:\\WINDOWS\\Installer\\{203D73DA-EC83-4923-A50A-464CDF02BA1D}\\DesigncraftIcon.ico\t\t1",
            "Inkscape\tC:\\Outils\\inkscape.exe\t",
            "FigCraft\tC:\\WINDOWS\\Installer\\{0091B35D-7B5F-40C6-9128-36467A800BA7}\\FigcraftIcon.exe\t",
        ];
        let mut pins: Vec<PinnedApp> = lignes.iter().filter_map(|l| parse_extra_pin(l)).collect();
        let vrai = PathBuf::from(r"C:\Program Files\Designcraft\Designcraft.exe");
        let resoudre = |p: &Path, nom: &str| (nom == "DesignCraft" && p.to_string_lossy().contains("Designcraft")).then(|| vrai.clone());
        assert!(reparer_les_leurres_msi(&mut pins, resoudre));
        assert_eq!(pins[0].path.as_ref(), Some(&vrai));
        assert!(pins[0].launcher, "le reste de la ligne est gardé");
        assert_eq!(pins[1].path.as_deref(), Some(Path::new(r"C:\Outils\inkscape.exe")));
        assert!(pins[2].path.as_ref().is_some_and(|p| p.to_string_lossy().ends_with("FigcraftIcon.exe")), "irréparable : gardée");
        assert!(!reparer_les_leurres_msi(&mut pins[1..2], resoudre), "rien à réparer : le fichier n'est pas réécrit");
    }
}

/// Ajoute une épingle à `dock_pins.txt` (voir `load_extra_pins`) si son nom n'y
/// figure pas déjà — comparaison **insensible à la casse**, même discipline que
/// `save_custom_category` : deux épingles « Spotify » et « spotify » seraient
/// indiscernables dans le dock. Rend `true` si le fichier a été modifié.
///
/// `launcher` : voir `PinnedApp::launcher`. C'est **le Dash qui le pose à `true`**
/// (voir `cef_dash::handle_add_to_zone`) : une application ajoutée à une zone
/// depuis le panneau est un RACCOURCI, pas l'application — l'utilisateur l'a
/// demandé explicitement le 2026-09-23. Les épingles déjà écrites gardent leur
/// nature : cette fonction relit le fichier avant d'y ajouter, donc elle ne
/// touche à aucune autre ligne.
pub fn save_extra_pin(name: &str, path: Option<&std::path::Path>, uri: Option<&str>, launcher: bool) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    // ⚠️ **LE DOUBLON SE JUGE SUR LA LISTE FUSIONNÉE, PAS SUR CE SEUL FICHIER.**
    // Une application déjà épinglée par `config.toml` ne doit pas être ajoutée
    // ici : `Dock::new` lui créerait une SECONDE place, et le dock afficherait
    // deux fois la même application. (Et la relire est sans risque : rien ici
    // n'écrit avant la fin.)
    if load_pinned_apps().iter().any(|p| p.name.eq_ignore_ascii_case(name)) {
        return false;
    }
    let mut pins = load_extra_pins();
    pins.push(PinnedApp {
        name: name.to_string(),
        path: path.map(std::path::Path::to_path_buf),
        uri: uri.map(str::to_string),
        launcher,
        // Cette fonction n'écrit QUE dans `dock_pins.txt` : son entrée en vient.
        from_extra: true,
    });
    write_config("dock_pins.txt", extra_pins_to_file(&pins));
    true
}

/// **RETIRE UNE ÉPINGLE DE `dock_pins.txt`** — DEMANDE EXPLICITE (2026-09-23) : le
/// panneau des Réglages offre « Retirer » sur le contenu d'une zone.
///
/// Comparaison **insensible à la casse**, même discipline que `save_extra_pin` :
/// deux noms qui ne diffèrent que par la casse désignent la même application.
///
/// Rend `true` si le fichier a été modifié, donc **`false` quand le nom n'y est
/// pas** — c'est le cas d'une épingle de `config.toml` (voir
/// `PinnedApp::from_extra`) : l'application **ne réécrit jamais** ce fichier-là,
/// qui est écrit à la main. L'appelant ne doit donc pas annoncer un retrait qui
/// n'a pas eu lieu.
///
/// ⚠️ **LE DOCK NE RELIT SES ÉPINGLES QU'À SA CONSTRUCTION.** L'appelant doit
/// demander un rechargement (voir `crate::app::request_reload`), sinon l'icône
/// reste à l'écran et l'action semble n'avoir rien fait — c'est le motif exact du
/// Dash après un ajout (`cef_dash::handle_add_to_zone`, qui porte le commentaire).
pub fn remove_extra_pin(name: &str) -> bool {
    let name = name.trim();
    let pins = load_extra_pins();
    let avant = pins.len();
    let restants: Vec<PinnedApp> =
        pins.into_iter().filter(|p| !p.name.eq_ignore_ascii_case(name)).collect();
    if restants.len() == avant {
        return false;
    }
    write_config("dock_pins.txt", extra_pins_to_file(&restants));
    true
}

/// **LA MISE EN FORME DE `dock_pins.txt` — UNE SEULE DÉFINITION, DEUX ÉCRIVAINS**
/// (`save_extra_pin` et `remove_extra_pin`). Pure, donc testable sans disque, et
/// c'est ce qui garantit que les deux écrivent EXACTEMENT le même format : deux
/// mises en forme divergentes produiraient un fichier que l'un des deux relirait
/// de travers, sans que rien ne le signale.
fn extra_pins_to_file(pins: &[PinnedApp]) -> String {
    pins
        .iter()
        // Comme `save_zone_names` : les caractères qui SCINDERAIENT la ligne
        // (retour à la ligne, tabulation) sont écrasés plutôt que d'écrire un
        // fichier dont la relecture inventerait une épingle fantôme.
        .map(|p| {
            // ⚠️ **LA QUATRIÈME COLONNE N'EST ÉCRITE QUE POUR UN LANCEUR**, et
            // c'est délibéré : un fichier qui n'en contient aucun est alors
            // réécrit **au caractère près** comme avant l'arrivée du champ. Une
            // colonne vide ajoutée partout aurait suffi au lecteur (voir
            // `parse_extra_pin`), mais aurait modifié le fichier de l'utilisateur
            // sans rien lui apporter.
            format!(
                "{}\t{}\t{}{}",
                p.name.replace(['\n', '\r', '\t'], " "),
                // Vide pour une application du STORE : elle n'a pas de chemin.
                // La COLONNE reste écrite, un fichier à nombre constant de
                // colonnes se relisant sans cas particulier.
                p.path.as_ref().map(|c| c.display().to_string()).unwrap_or_default(),
                // La troisième colonne est TOUJOURS écrite, même vide : un
                // fichier à nombre de colonnes constant se relit sans cas
                // particulier (le lecteur accepte aussi deux colonnes, pour un
                // fichier écrit avant l'arrivée des URI).
                p.uri.as_deref().unwrap_or(""),
                if p.launcher { "\t1" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod extra_pins_to_file_tests {
    use super::*;

    fn pin(name: &str, launcher: bool) -> PinnedApp {
        PinnedApp {
            name: name.to_string(),
            path: Some(PathBuf::from(format!("C:\\Outils\\{}.exe", name.replace(' ', "")))),
            uri: None,
            launcher,
            from_extra: true,
        }
    }

    /// **L'ALLER-RETOUR EST LA PROPRIÉTÉ QUI COMPTE.** Ce fichier est écrit par
    /// `save_extra_pin` **et** par `remove_extra_pin`, et relu par
    /// `load_extra_pins` : si les deux formats divergeaient, une épingle
    /// changerait de nature — ou disparaîtrait — au premier enregistrement, sans
    /// que rien ne le signale. C'est ce qui a motivé l'extraction de cette
    /// fonction, et c'est ce que ce test verrouille.
    #[test]
    fn what_is_written_is_what_is_read() {
        let pins = vec![pin("Inkscape", true), pin("GIMP", false)];
        let relus: Vec<PinnedApp> = extra_pins_to_file(&pins).lines().filter_map(parse_extra_pin).collect();
        assert_eq!(relus.len(), 2, "deux lignes, deux épingles");
        assert_eq!(relus[0].name, "Inkscape");
        assert!(relus[0].launcher, "le lanceur garde sa colonne");
        assert!(!relus[1].launcher, "et l'épingle reste une épingle");
        assert!(relus.iter().all(|p| p.from_extra), "tout ce qui vient de ce fichier en vient");
    }

    /// ⚠️ **RETIRER UNE ÉPINGLE NE DOIT TOUCHER QUE LA SIENNE.** `remove_extra_pin`
    /// réécrit tout le fichier depuis la liste relue : une nature perdue dans la
    /// mise en forme le serait donc pour **toutes** les lignes d'un coup.
    #[test]
    fn removing_one_pin_leaves_the_others_intact() {
        let pins = vec![pin("Inkscape", true), pin("GIMP", false), pin("Character Map", false)];
        let restants: Vec<PinnedApp> =
            pins.into_iter().filter(|p| !p.name.eq_ignore_ascii_case("gimp")).collect();
        let texte = extra_pins_to_file(&restants);
        assert!(!texte.to_lowercase().contains("gimp"), "la retirée ne doit plus être écrite : {texte}");
        let relus: Vec<PinnedApp> = texte.lines().filter_map(parse_extra_pin).collect();
        assert_eq!(relus.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Inkscape", "Character Map"]);
        assert!(relus[0].launcher, "le lanceur qui RESTE garde sa colonne");
    }

    /// Une application du STORE : ni exécutable ni icône de fichier, mais une URI
    /// — la colonne vide doit rester vide à la relecture, et l'URI survivre.
    #[test]
    fn a_store_app_keeps_its_empty_column() {
        let pins = vec![PinnedApp {
            name: "PDF Document Scanner".to_string(),
            path: None,
            uri: Some("shell:AppsFolder\\61083ApeApps.PDFDocumentScanner_d2yynfvsn01f4!App".to_string()),
            launcher: false,
            from_extra: true,
        }];
        let relus: Vec<PinnedApp> = extra_pins_to_file(&pins).lines().filter_map(parse_extra_pin).collect();
        assert_eq!(relus.len(), 1);
        assert!(relus[0].path.is_none(), "un chemin vide reste vide");
        assert!(relus[0].uri.is_some(), "et l'URI survit");
    }
}

/// La zone du dock qui reçoit les applications ajoutées depuis le Dash, **telle que l'ANCIEN
/// fichier la portait** — l'index du séparateur qui l'ouvre.
///
/// ⚠️ **FICHIER HÉRITÉ, LECTURE SEULE.** Depuis `zones.toml`, la destination est un scalaire
/// `dash_add_zone = "D3"` — une CLÉ, pas un index (voir `load_zones_dash_add_zone_key`, qui est
/// désormais la source). Ce lecteur ne sert plus qu'à `load_zones_legacy`, c'est-à-dire à la
/// MIGRATION — et c'est justement pourquoi il reste : la migration doit partir de ce que le code
/// d'avant comprenait.
///
/// ⚠️ **`save_dash_add_zone` A ÉTÉ SUPPRIMÉE.** Elle écrivait l'index dans `dash_add_zone.txt` ;
/// la garder aurait laissé un écrivain capable de faire RENAÎTRE le fichier après son renommage
/// par la migration, et donc de faire diverger deux sources pour un seul réglage.
///
/// `None` = aucune zone désignée. L'entrée **n'apparaît alors pas** dans le menu
/// du clic droit : proposer une action qui ne saurait pas où ranger
/// l'application serait un piège.
pub fn load_dash_add_zone() -> Option<usize> {
    let path = config_path("dash_add_zone.txt")?;
    let content = fs::read_to_string(path).ok()?;
    content.trim().parse().ok()
}

/// Catégories des applications du Dash — choix manuels de l'utilisateur qui
/// prévalent sur le classement automatique (voir `win32::app_categories`),
/// et catégories supplémentaires qu'il a créées lui-même (voir
/// `win32::dash`, sélecteur de catégorie accessible par clic droit sur une
/// application). Stockées dans `%APPDATA%\NeuraDesk\categories.toml`, à
/// part de `config.toml` (données générées par l'usage du Dash, pas des
/// réglages qu'on s'attend à éditer à la main comme `pinned_apps`).
///
/// Un choix manuel peut désormais porter sur PLUSIEURS catégories à la fois
/// (voir `add_category_override`/`remove_category_override`) — une entrée
/// de la table `overrides` est donc un TABLEAU de noms de catégorie, jamais
/// une chaîne seule. `value_to_categories` accepte quand même l'ancien
/// format (chaîne unique, une seule catégorie manuelle par application) pour
/// rester compatible avec un `categories.toml` déjà écrit par une version
/// antérieure de NeuraDesk.
///
/// `toml::Table` pour la lecture ET l'écriture (contrairement au reste de
/// ce fichier, en "valeurs séparées par des virgules" fait main) : les clés
/// de la table `overrides` sont des noms d'application arbitraires (espaces,
/// accents, apostrophes...), qu'une sérialisation TOML correcte doit
/// entourer de guillemets au besoin — `toml::Table` s'en charge tout seul,
/// un format fait main devrait réinventer cet échappement.
fn categories_path() -> Option<PathBuf> {
    config_path("categories.toml")
}

fn load_categories_table() -> toml::Table {
    let Some(path) = categories_path() else { return toml::Table::new() };
    let Ok(content) = fs::read_to_string(&path) else { return toml::Table::new() };
    content.parse::<toml::Table>().unwrap_or_else(|err| {
        eprintln!("[neuradesk] categories.toml invalide, ignoré : {err}");
        toml::Table::new()
    })
}

fn write_categories_table(table: &toml::Table) {
    write_config("categories.toml", table.to_string());
}

/// Convertit une valeur de la table `overrides` en liste de catégories —
/// accepte un tableau de chaînes (format actuel, plusieurs catégories
/// manuelles) aussi bien qu'une chaîne unique (ancien format, voir doc de
/// module) pour rester compatible avec un fichier déjà sur le disque.
fn value_to_categories(value: &toml::Value) -> Option<Vec<String>> {
    if let Some(s) = value.as_str() {
        return Some(vec![s.to_string()]);
    }
    let array = value.as_array()?;
    Some(array.iter().filter_map(|v| v.as_str().map(String::from)).collect())
}

/// Relit les choix manuels de catégorie, par nom d'application (clé déjà en
/// minuscules — voir `add_category_override`) vers la liste des catégories
/// choisies (jamais vide : une entrée qui le deviendrait est retirée
/// entièrement, voir `remove_category_override`). Une entrée ici prévaut
/// toujours sur le classement automatique (voir `win32::apps::enumerate_apps`).
pub fn load_category_overrides() -> std::collections::HashMap<String, Vec<String>> {
    let table = load_categories_table();
    let Some(overrides) = table.get("overrides").and_then(|v| v.as_table()) else {
        return std::collections::HashMap::new();
    };
    overrides
        .iter()
        .filter_map(|(name, value)| {
            let categories = value_to_categories(value)?;
            (!categories.is_empty()).then_some((name.to_lowercase(), categories))
        })
        .collect()
}

/// Ajoute `category` aux catégories manuelles de l'application `app_name`
/// (comparaison insensible à la casse, voir `load_category_overrides`) — ne
/// fait rien si elle y figurait déjà. Contrairement à l'ancienne version à
/// une seule catégorie, ceci s'AJOUTE aux catégories déjà choisies plutôt
/// que de les remplacer, pour permettre à une application d'appartenir à
/// plusieurs catégories à la fois.
pub fn add_category_override(app_name: &str, category: &str) {
    let mut table = load_categories_table();
    let overrides = table.entry("overrides").or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let Some(map) = overrides.as_table_mut() else { return };
    let key = app_name.to_lowercase();
    let mut categories = map.get(&key).and_then(value_to_categories).unwrap_or_default();
    if !categories.iter().any(|c| c.eq_ignore_ascii_case(category)) {
        categories.push(category.to_string());
    }
    map.insert(key, toml::Value::Array(categories.into_iter().map(toml::Value::String).collect()));
    write_categories_table(&table);
}

/// Retire `category` des catégories manuelles de l'application `app_name` —
/// si c'était la dernière, l'entrée disparaît entièrement de la table
/// (l'application retombe sur le classement automatique au prochain scan,
/// voir `win32::apps::enumerate_apps`) plutôt que de laisser un choix manuel
/// vide, qui ne correspondrait à aucune catégorie affichée.
pub fn remove_category_override(app_name: &str, category: &str) {
    let mut table = load_categories_table();
    let Some(map) = table.get_mut("overrides").and_then(|v| v.as_table_mut()) else { return };
    let key = app_name.to_lowercase();
    let Some(mut categories) = map.get(&key).and_then(value_to_categories) else { return };
    categories.retain(|c| !c.eq_ignore_ascii_case(category));
    if categories.is_empty() {
        map.remove(&key);
    } else {
        map.insert(key, toml::Value::Array(categories.into_iter().map(toml::Value::String).collect()));
    }
    write_categories_table(&table);
}

/// Efface TOUTES les catégories manuelles de `app_name`, s'il y en avait —
/// l'application retombe alors sur le classement automatique au prochain
/// scan (voir `win32::apps::enumerate_apps`). Utilisé par l'option
/// "Réinitialiser" du sélecteur du Dash, distincte de `remove_category_override`
/// (qui ne retire qu'UNE catégorie parmi plusieurs).
pub fn clear_category_override(app_name: &str) {
    let mut table = load_categories_table();
    if let Some(map) = table.get_mut("overrides").and_then(|v| v.as_table_mut()) {
        map.remove(&app_name.to_lowercase());
    }
    write_categories_table(&table);
}

/// Historique du presse-papiers (voir `win32::clipboard`) — persisté dans
/// `%APPDATA%\NeuraDesk\clipboard_history.toml` (les entrées, dans l'ordre,
/// le plus récent en tête) + `%APPDATA%\NeuraDesk\clipboard_images\` (le
/// contenu PNG de chaque entrée Image, à part : une capture d'écran peut
/// peser plusieurs Mo, largement au-delà de ce qu'un fichier de
/// configuration texte devrait raisonnablement contenir en base64).
/// Rechargé au lancement (voir `app::run`), pour que l'historique survive
/// entre deux sessions de NeuraDesk — jusque-là perdu à chaque fermeture.
fn clipboard_history_path() -> Option<PathBuf> {
    config_path("clipboard_history.toml")
}

/// Les images de l'historique — dans le dossier des CACHES depuis le 2026-10-29
/// (voir `migrer_clipboard_images` : 1,1 Go dans le dossier itinérant).
fn clipboard_images_dir() -> Option<PathBuf> {
    let mut path = cache_dir()?;
    path.push("clipboard_images");
    Some(path)
}

/// Nom de fichier déterministe pour l'image d'une entrée, dérivé de son
/// contenu — deux copies identiques de la même image (fréquent : capturer
/// deux fois le même écran, ou re-copier une image déjà dans l'historique)
/// partagent ainsi le même fichier sur disque plutôt que d'en écrire un
/// doublon à chaque sauvegarde. Un simple hash (pas cryptographique, aucun
/// besoin de résistance aux collisions volontaires ici) suffit : au pire
/// une collision fortuite réutiliserait par erreur le fichier d'une autre
/// image, un défaut purement cosmétique (mauvaise miniature affichée) sans
/// aucune conséquence de sécurité.
fn clipboard_image_file_name(rgba: &[u8]) -> String {
    hashed_png_name(rgba)
}

/// Nom de fichier `.png` déterministe dérivé du contenu — factorisé hors de
/// `clipboard_image_file_name` (qui n'est plus qu'un alias conservé pour ne
/// pas renommer tous ses appels) : `store_note_image` a besoin exactement de
/// la même recette pour les images de notes, voir doc de fonction ci-dessus
/// (pas de résistance aux collisions volontaires nécessaire, un hash simple
/// suffit).
fn hashed_png_name(rgba: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    rgba.hash(&mut hasher);
    format!("{:016x}.png", hasher.finish())
}

/// Vrai si `name` a EXACTEMENT la forme produite par `hashed_png_name`
/// (16 chiffres hexadécimaux minuscules + `.png`) — appelé avant de joindre
/// un nom de fichier lu depuis `notes.toml` (`images[].file`) à
/// `notes_images_dir`, en défense en profondeur contre un `notes.toml`
/// altéré ou restauré depuis une source non fiable qui contiendrait une
/// traversée de chemin (`../../...`). En usage normal `file` provient
/// toujours de `hashed_png_name`, jamais d'une saisie utilisateur directe —
/// cette validation ne devrait donc jamais rejeter un fichier légitime.
fn is_valid_note_image_file_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".png") else { return false };
    stem.len() == 16 && stem.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

// --- Raccourcis de DOSSIERS de la barre du bas ----------------------------------------
//
// DEMANDE EXPLICITE (2026-09-28) : « une barre du bas pour pouvoir épingler des
// chemins vers des dossiers pour avoir un accès rapide à ces dossiers », avec
// une vignette par dossier « pour les repérer plus facilement ».
//
// ⚠️ **CES RACCOURCIS NE VIVENT PAS DANS `icon_order.txt`.** Leur ORDRE est
// celui de ce fichier, et c'est délibéré : la barre du bas est la seule dont le
// contenu n'est pas une liste d'index, ce qui évite trois pièges mesurés —
// `set_icon_order` ressuscite dans la barre de GAUCHE tout élément non-`is_spacer`
// absent des listes persistées, et tout ce qui dépasse `base_non_persistable`
// n'est de toute façon jamais écrit.
//
// ⛔ **ET LA PHRASE « `Dock::bottom_order` RESTE VIDE », QUI ÉTAIT ICI, ÉTAIT
// FAUSSE.** `Dock::set_dossiers` la remplit à chaque appel, et `Dock::orders` la
// rend en cinquième position. Cette affirmation périmée a fait REFUSER tout
// lâcher de glisser dans la barre du bas pendant des mois (voir
// `Dock::drag_insert_index`, bras `Side::Bottom`).

/// Le plus grand côté d'une vignette ENREGISTRÉE — `BAR_THICKNESS_MAX` vaut 250,
/// donc une vignette de 256 px couvre la barre la plus épaisse possible. La
/// réduire à l'enregistrement évite de trimballer une photo de 12 Mo dans
/// `%APPDATA%` pour la peindre à 64 px.
pub const DOSSIER_VIGNETTE_MAX_SIDE: u32 = 256;

/// Un raccourci de dossier épinglé à la barre du bas (voir `ui::dock::Side::Bottom`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DossierEpingle {
    /// Le dossier visé — **toujours ABSOLU** (voir `depuis_chemin`, et
    /// `load_dossiers`, qui refuse un chemin relatif à la relecture : il
    /// désignerait un dossier différent selon le répertoire courant).
    pub chemin: PathBuf,
    /// Le nom AFFICHÉ sous la vignette — `None` : celui du dossier, déduit du
    /// chemin (voir `nom_affiche`).
    pub nom: Option<String>,
    /// Nom du fichier de vignette sous `dossiers_images/` — toujours de la forme
    /// produite par `hashed_png_name`, et **revalidé** à la relecture
    /// (`is_valid_dossier_vignette_name`) avant d'être joint à un chemin.
    pub vignette: Option<String>,
}

/// **UN EMPLACEMENT DE LA BARRE DU BAS** — un raccourci de dossier, ou un
/// SÉPARATEUR.
///
/// ⚠️ **C'EST CE QUE `dossiers.toml` DÉCRIT DÉSORMAIS** : l'ORDRE de la rangée, et
/// non plus seulement la liste des dossiers. Sans quoi la POSITION d'un séparateur
/// n'était écrite **nulle part** — mesuré : `zones.toml` ne porte qu'un COMPTE par
/// barre et des listes PAR RANG, jamais une place parmi les dossiers. Conséquence
/// constatée par l'utilisateur : « le changement disparaît au redémarrage » — un
/// séparateur posé en tête revenait en fin de rangée.
///
/// ⚠️ **RÉTROCOMPATIBLE SANS MIGRATION** : `type` ABSENT vaut « dossier », donc
/// **tous les fichiers d'avant se relisent tels quels**.
///
/// ⛔ **LA SORTE EST ICI DEPUIS LE 2026-10-01, ET C'EST UNE MESURE QUI L'A DÉCIDÉ.**
///
/// Elle vivait dans `[bars.B] sortes`, une liste **PAR RANG** — et le rang se compte
/// sur la rangée VIVANTE. Or c'est précisément cette rangée que l'utilisateur
/// modifie : **insérer un séparateur décale tous les rangs**, donc chaque séparateur
/// hérite de la sorte de son voisin au redémarrage suivant. Défaut signalé : « j'ai
/// ajouté des séparateurs mais je ne vois pas les points leur correspondant » — ils
/// étaient devenus **ÉLASTIQUES**, une sorte qui ne dessine RIEN (voir
/// `Dock::spacer_instances`), en héritant du rang d'à côté.
///
/// **La règle d'avant tenait pour les barres LATÉRALES**, dont l'ordre ne bouge pas.
/// Ici l'ordre **EST** la donnée : une propriété rangée par rang ne peut pas y
/// survivre. La sorte voyage donc avec l'élément, comme sa position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ElementBarre {
    Dossier(DossierEpingle),
    /// Un séparateur, à cette place, **avec sa sorte**.
    Separateur(SorteEspace),
}

/// La valeur de `type` pour un séparateur — `dossier` est la valeur ABSENTE (voir
/// `ElementBarre`), donc elle ne s'écrit jamais.
pub const TYPE_SEPARATEUR: &str = "separateur";

#[cfg(test)]
impl ElementBarre {
    /// **`k` DOSSIERS D'ESSAI** — des chemins ABSOLUS et distincts, donc ce que la
    /// relecture accepterait, mais **sans toucher au disque** : `set_dossiers` ne
    /// lit jamais le chemin, il ne fait que compter et placer.
    ///
    /// ⚠️ **UN AIDE, ET PAS UNE FIXTURE RECOPIÉE** : sept tests appelaient
    /// `set_dossiers(nombre)`, et les migrer un par un aurait fait sept listes à
    /// garder d'accord — le motif que ce dépôt paie à répétition.
    pub fn dossiers_de_test(k: usize) -> Vec<Self> {
        (0..k)
            .map(|i| {
                ElementBarre::Dossier(DossierEpingle {
                    chemin: PathBuf::from(format!("C:\\essai\\dossier{i}")),
                    nom: None,
                    vignette: None,
                })
            })
            .collect()
    }

    /// `k` SÉPARATEURS d'essai — sans propriété, comme au lancement : leur sorte et
    /// leur taille viennent de `zones.toml`.
    pub fn separateurs_de_test(k: usize) -> Vec<Self> {
        (0..k).map(|_| ElementBarre::Separateur(SorteEspace::Rond)).collect()
    }
}

impl DossierEpingle {
    /// Un raccourci vers `chemin` — `None` si ce n'est pas un DOSSIER existant,
    /// ou si le chemin est relatif.
    ///
    /// C'est le SEUL point d'entrée : un chemin vient toujours du dialogue
    /// système (voir `win32::file_dialog::choisir_dossier`, jamais d'une page),
    /// et on vérifie quand même — le dossier peut avoir disparu entre le choix
    /// et l'écriture.
    pub fn depuis_chemin(chemin: PathBuf) -> Option<Self> {
        if !chemin.is_absolute() || !chemin.is_dir() {
            return None;
        }
        Some(Self { chemin, nom: None, vignette: None })
    }

    /// Le nom à PEINDRE sous la vignette : celui que l'utilisateur a donné,
    /// sinon celui du dossier.
    ///
    /// ⚠️ **Un dossier RACINE (`C:\`) n'a pas de dernier composant** : on montre
    /// alors le chemin entier plutôt que rien — une vignette sans nom serait
    /// indiscernable d'une autre.
    pub fn nom_affiche(&self) -> String {
        if let Some(nom) = &self.nom {
            return nom.clone();
        }
        self.chemin
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.chemin.to_string_lossy().into_owned())
    }
}

/// `%APPDATA%\NeuraDesk\dossiers_images\` — les vignettes des raccourcis de
/// dossiers, même famille que `notes_images_dir`/`clipboard_images_dir`, et
/// **surtout PAS `config_path`**, qui ne sait désigner qu'un nom de fichier à la
/// racine de `config_dir`.
fn dossiers_images_dir() -> Option<PathBuf> {
    let mut path = config_dir()?;
    path.push("dossiers_images");
    Some(path)
}

/// Vrai si `name` a EXACTEMENT la forme produite par `hashed_png_name` — appelé
/// avant de joindre un nom lu depuis `dossiers.toml` à `dossiers_images_dir`,
/// en défense en profondeur contre un fichier altéré qui contiendrait une
/// traversée de chemin (`../../...`). Même garde que
/// `is_valid_note_image_file_name`, et pour la même raison.
fn is_valid_dossier_vignette_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".png") else { return false };
    stem.len() == 16 && stem.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Relit les raccourcis de dossiers (voir `DossierEpingle`) — liste vide si le
/// fichier est absent ou illisible, comme un tout premier lancement.
///
/// Une ligne SANS chemin exploitable est ÉCARTÉE sans faire échouer les autres :
/// une entrée abîmée ne doit pas emporter les raccourcis valides.
pub fn load_barre_du_bas() -> Vec<ElementBarre> {
    let Some(path) = config_path("dossiers.toml") else { return Vec::new() };
    let Ok(content) = fs::read_to_string(&path) else { return Vec::new() };
    let Ok(table) = content.parse::<toml::Table>() else { return Vec::new() };
    barre_du_bas_from_toml(&table)
}

/// **LE CŒUR PUR DE LA RELECTURE** — jumeau de `barre_du_bas_to_toml`, séparé pour la
/// même raison : ce fichier vit dans `%APPDATA%`, et un test qui le lirait — ou pire qui
/// l'écrirait — modifierait la configuration **RÉELLE** de la machine. C'est la même
/// précaution que `zones_from_toml`, prise ici pour le seul fichier qui porte des
/// DONNÉES de l'utilisateur (ses dossiers épinglés).
fn barre_du_bas_from_toml(table: &toml::Table) -> Vec<ElementBarre> {
    let Some(rows) = table.get("dossiers").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let row = row.as_table()?;
            // ⛔ **LE DISCRIMINANT D'ABORD, ET AVANT TOUT LE RESTE.** Une ligne de
            // séparateur n'a PAS de `chemin` : la tester après l'exigence de chemin
            // la ferait écarter avant même d'être lue — et c'est précisément le
            // défaut que cette fonction avait, un `?` sur `chemin` qui laissait
            // tomber en silence tout ce qu'elle ne comprenait pas.
            //
            // ⚠️ **UN `type` INCONNU SE REFUSE EN LE DISANT**, jamais deviné (règle
            // de `Genre`, dans `dates.toml`) : un séparateur écrit à la main avec
            // une faute de frappe disparaîtrait sinon sans laisser de trace.
            if let Some(genre) = row.get("type").and_then(|v| v.as_str()) {
                if genre == TYPE_SEPARATEUR {
                    // **LA SORTE, OPTIONNELLE — absente vaut `Rond`**, donc tout fichier
                    // d'avant ce champ se relit tel quel (même règle que `type` absent,
                    // qui vaut « dossier »).
                    //
                    // ⚠️ **UNE CLÉ INCONNUE SE DIT, PUIS RETOMBE SUR `Rond`** — et non
                    // « la liste entière est refusée » comme `[bars.X] sortes` : ici,
                    // refuser ferait perdre la LIGNE, c'est-à-dire une place dans la
                    // rangée. Une sorte perdue ne coûte qu'un dessin ; une ligne perdue
                    // coûte une position. Le fichier est en outre le seul endroit où
                    // cette sorte existe.
                    let Some(cle) = row.get("sorte").and_then(|v| v.as_str()) else {
                        return Some(ElementBarre::Separateur(SorteEspace::Rond));
                    };
                    let sorte = SorteEspace::depuis_cle(cle).unwrap_or_else(|| {
                        eprintln!(
                            "[neuradesk] dossiers.toml : sorte inconnue {cle:?} — « rond » a la place"
                        );
                        SorteEspace::Rond
                    });
                    return Some(ElementBarre::Separateur(sorte));
                }
                eprintln!("[neuradesk] dossiers.toml : type d'element inconnu {genre:?} — ligne ecartee");
                return None;
            }
            let chemin = PathBuf::from(row.get("chemin")?.as_str()?);
            if !chemin.is_absolute() {
                return None;
            }
            let nom = row
                .get("nom")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .filter(|n| !n.trim().is_empty());
            // Une vignette dont le NOM ne tient pas la forme attendue est
            // écartée (le raccourci garde son nom, sans vignette) : c'est le
            // seul champ qui deviendra un CHEMIN.
            let vignette = row
                .get("vignette")
                .and_then(|v| v.as_str())
                .filter(|n| is_valid_dossier_vignette_name(n))
                .map(str::to_string);
            Some(ElementBarre::Dossier(DossierEpingle { chemin, nom, vignette }))
        })
        .collect()
}

/// Écrit les raccourcis de dossiers. Rend `false` si l'écriture a échoué.
///
/// ⚠️ **ATOMIQUE** (`write_config_atomique`) : ce fichier porte TOUS les
/// raccourcis de l'utilisateur, et `write_config` est silencieux et non
/// atomique — une écriture partielle serait une perte totale muette. Même raison
/// que `zones.toml`.
/// **L'ORDRE DES ÉLÉMENTS TEL QUE LA BARRE DU BAS LE MONTRE** — `None` quand
/// l'ordre ne se traduit pas (index étranger, ou rangée incomplète).
///
/// ⚠️ **POURQUOI ELLE EXISTE.** `dossiers.toml` porte l'ordre des éléments, et
/// c'est le SEUL endroit où il vit — mais le dock, lui, ne connaît que des
/// **index d'icônes** (`Dock::bottom_order`). Réordonner par un glisser change les
/// index, jamais les éléments : sans cette traduction, le geste serait effacé au
/// prochain `set_dossiers` et rien ne serait écrit.
///
/// ⚠️ **LES RANGS SONT CEUX DE `set_dossiers`, À LA LETTRE** — et c'est ce qui
/// rend cette fonction vérifiable : les dossiers occupent `base .. base + count`,
/// le « + » est à `base + count`, et les séparateurs commencent à
/// `base + count + 1`. Un index désigne donc son élément par son **rang dans sa
/// famille**, pas par sa place dans la rangée.
///
/// ⛔ **ELLE REND `None` PLUTÔT QUE DE SAUTER UNE CASE INCONNUE.** Une rangée qui
/// contient autre chose que ce qu'on croit ne doit pas être écrite : c'est
/// exactement ainsi qu'un dossier a disparu de ce fichier.
pub fn elements_selon_l_ordre_du_dock(
    elements: &[ElementBarre],
    base: usize,
    count: usize,
    ordre: &[usize],
) -> Option<Vec<ElementBarre>> {
    let dossiers: Vec<&ElementBarre> =
        elements.iter().filter(|e| matches!(e, ElementBarre::Dossier(_))).collect();
    let separateurs: Vec<&ElementBarre> =
        elements.iter().filter(|e| matches!(e, ElementBarre::Separateur(_))).collect();
    let sep_base = base + count + 1;
    let mut resultat: Vec<ElementBarre> = Vec::with_capacity(elements.len());
    for &index in ordre {
        if index >= base && index < base + count {
            resultat.push((*dossiers.get(index - base)?).clone());
        } else if index >= sep_base && index < sep_base + separateurs.len() {
            resultat.push((*separateurs.get(index - sep_base)?).clone());
        } else if index == base + count {
            // Le « + » — pas un élément. Il est dans la rangée, pas dans le
            // fichier : c'est le seul index qu'on ignore SCIEMMENT.
        } else {
            return None;
        }
    }
    // Une rangée qui ne rend pas TOUS les éléments est une rangée qu'on ne
    // comprend pas : on refuse plutôt que d'écrire une liste amputée.
    (resultat.len() == elements.len()).then_some(resultat)
}

/// **LA PERMUTATION DES DOSSIERS ENTRE DEUX ORDRES DE LA BARRE DU BAS** —
/// `permutation[k]` est l'ANCIEN rang du dossier qui occupe désormais le rang `k`.
///
/// ⛔ **POURQUOI ELLE EXISTE, ET LE DÉFAUT QU'ELLE RÉPARE.** Un glisser dans la barre
/// du bas change l'ordre des ÉLÉMENTS, et jusqu'ici c'était la SEULE chose qu'il
/// changeait. Or **les vignettes sont indexées par le RANG, pas par la case** :
/// `app.rs` peint `vignettes[rang]` dans l'emplacement `base + rang` (voir le bloc
/// `dossier_render_instances`), et route un clic par `dock.dossier_slot(index)` puis
/// `dossier_au_rang(&elements, rang)`. Permuter la liste SANS permuter les vignettes
/// fait donc diverger les deux, et **chaque icône porte le chemin d'un autre**.
/// Constaté par l'utilisateur : « les icônes de dossiers ne correspondent plus au
/// chemin des dossiers, tous les chemins sont mélangés entre les icônes » — et c'est
/// durable, puisque seul le redémarrage rebâtit les deux termes depuis la même liste.
///
/// Rend `None` quand `apres` n'est pas exactement `avant` réordonné — même refus que
/// `elements_selon_l_ordre_du_dock`, et pour la même raison : on n'écrit pas un état
/// qu'on n'a pas su relire, et on ne réordonne pas un tableau qu'on comprend de
/// travers.
pub fn permutation_des_dossiers(avant: &[ElementBarre], apres: &[ElementBarre]) -> Option<Vec<usize>> {
    let chemins = |liste: &[ElementBarre]| -> Vec<PathBuf> {
        liste
            .iter()
            .filter_map(|element| match element {
                ElementBarre::Dossier(dossier) => Some(dossier.chemin.clone()),
                ElementBarre::Separateur(_) => None,
            })
            .collect()
    };
    let anciens = chemins(avant);
    let nouveaux = chemins(apres);
    if anciens.len() != nouveaux.len() {
        return None;
    }
    let mut pris = vec![false; anciens.len()];
    let mut permutation = Vec::with_capacity(anciens.len());
    for chemin in &nouveaux {
        let rang = anciens.iter().position(|ancien| ancien == chemin)?;
        // **CHAQUE ANCIEN RANG EXACTEMENT UNE FOIS** — c'est ce qui distingue une
        // permutation d'une liste où un dossier compterait double et où un autre
        // aurait disparu : la forme EXACTE des deux pertes subies par
        // `dossiers.toml` (voir la sonde de `save_barre_du_bas`).
        if std::mem::replace(&mut pris[rang], true) {
            return None;
        }
        permutation.push(rang);
    }
    Some(permutation)
}

/// **APPLIQUE UNE PERMUTATION À UN TABLEAU PARALLÈLE** — `sortie[k] = source[permutation[k]]`.
///
/// Le pendant de `permutation_des_dossiers` pour les tableaux qu'elle ne connaît pas :
/// les vignettes et leurs replis (`app::EmplacementsDossiers`), indexés par le RANG
/// comme la liste l'est.
///
/// ⚠️ **ELLE EST GÉNÉRIQUE POUR ÊTRE VÉRIFIABLE** : sans texture à fabriquer, un test
/// peut lui donner des `&str` et lire le résultat — c'est la seule façon de mettre à
/// l'épreuve l'indexation qui a produit le défaut. Rend `None` si `permutation` n'est
/// pas exactement une permutation de `0..source.len()` : jamais un tableau amputé.
pub fn reordonne_par<T: Clone>(source: &[T], permutation: &[usize]) -> Option<Vec<T>> {
    if permutation.len() != source.len() {
        return None;
    }
    let mut pris = vec![false; source.len()];
    let mut sortie = Vec::with_capacity(source.len());
    for &rang in permutation {
        if rang >= source.len() || std::mem::replace(&mut pris[rang], true) {
            return None;
        }
        sortie.push(source[rang].clone());
    }
    Some(sortie)
}

pub fn save_barre_du_bas(elements: &[ElementBarre]) -> bool {
    // **SONDE — CE QUE CHAQUE ÉCRITURE ÉCRIT, ET DANS QUEL ORDRE.**
    //
    // ⚠️ **ÉCRITE POUR DEUX PERTES DE DOSSIERS EN VINGT-QUATRE HEURES** — `Downloads`
    // (2ᵉ position) puis `Videos` (5ᵉ), **toujours un dossier du MILIEU**. Le va-et-vient
    // sur le fichier réel étant FIDÈLE (mesuré : `17, 10, 7` chargés et relus à
    // l'identique), l'écrivain et le lecteur sont innocents : **la perte est en amont**,
    // dans un geste qui retire un élément de la liste avant l'écriture.
    //
    // Cette ligne nomme donc chaque écriture par son CONTENU — les dossiers dans l'ordre,
    // un `|` pour chaque séparateur. Il suffit alors de comparer deux écritures
    // consécutives pour voir laquelle fait disparaître un nom, et de la rapprocher du
    // geste que l'utilisateur vient de faire.
    crate::diag!(
        "dossiers.toml ECRIT : {} emplacements ({} dossiers, {} separateurs) — [{}]",
        elements.len(),
        elements.iter().filter(|e| matches!(e, ElementBarre::Dossier(_))).count(),
        elements.iter().filter(|e| matches!(e, ElementBarre::Separateur(_))).count(),
        elements
            .iter()
            .map(|element| match element {
                ElementBarre::Dossier(d) => d
                    .chemin
                    .file_name()
                    .map_or_else(|| "?".to_string(), |nom| nom.to_string_lossy().into_owned()),
                ElementBarre::Separateur(_) => "|".to_string(),
            })
            .collect::<Vec<_>>()
            .join(" "),
    );
    write_config_atomique("dossiers.toml", barre_du_bas_to_toml(elements).to_string())
}

/// **LE CŒUR PUR DE L'ÉCRITURE** — jumeau de `barre_du_bas_from_toml`, et séparé pour la
/// même raison : c'est ce qui rend possible un **va-et-vient sur une liste EN MÉMOIRE**
/// au lieu du fichier réel.
///
/// ⚠️ **MÊME MOTIF QUE `zones_to_toml`**, dont `dossiers.toml` n'avait PAS l'équivalent
/// — et c'est ce qui manquait pour répondre à « la configuration de la barre
/// sera-t-elle bien sauvegardée ? » autrement que par une promesse.
fn barre_du_bas_to_toml(elements: &[ElementBarre]) -> toml::Table {
    let rows: Vec<toml::Value> = elements
        .iter()
        .map(|element| {
            let mut row = toml::Table::new();
            match element {
                ElementBarre::Dossier(d) => {
                    row.insert(
                        "chemin".to_string(),
                        toml::Value::String(d.chemin.to_string_lossy().into_owned()),
                    );
                    if let Some(nom) = &d.nom {
                        row.insert("nom".to_string(), toml::Value::String(nom.clone()));
                    }
                    if let Some(vignette) = &d.vignette {
                        row.insert("vignette".to_string(), toml::Value::String(vignette.clone()));
                    }
                }
                // ⚠️ **`type`, PUIS LA SORTE — MAIS SEULEMENT SI ELLE N'EST PAS LE
                // DÉFAUT.** Même règle que `[bars.X] sortes` dans `zones_to_toml` : un
                // séparateur ordinaire reste `type = "separateur"` seul, donc un fichier
                // neuf ne grossit pas d'un champ qui ne dit rien, et les fichiers d'avant
                // se relisent à l'identique.
                ElementBarre::Separateur(sorte) => {
                    row.insert("type".to_string(), toml::Value::String(TYPE_SEPARATEUR.to_string()));
                    if *sorte != SorteEspace::Rond {
                        row.insert(
                            "sorte".to_string(),
                            toml::Value::String(sorte.cle().to_string()),
                        );
                    }
                }
            }
            toml::Value::Table(row)
        })
        .collect();
    let mut table = toml::Table::new();
    table.insert("dossiers".to_string(), toml::Value::Array(rows));
    table
}

/// Copie une image choisie par l'utilisateur dans `dossiers_images/` et rend son
/// nom de fichier — `None` si elle est illisible ou si `%APPDATA%` est
/// inaccessible.
///
/// **DÉDUPLIQUÉE PAR CONTENU** (`hashed_png_name`, jamais réécrite) et
/// **RÉ-ENCODÉE EN PNG** : la source peut être un JPEG, un BMP ou un WebP, et
/// n'est de toute façon jamais retenue telle quelle — la vignette doit survivre
/// au déplacement ou à la suppression de l'original.
pub fn store_dossier_vignette(source: &std::path::Path) -> Option<String> {
    let octets = fs::read(source).ok()?;
    let (largeur, hauteur, rgba) = clipboard::decode_png(&octets)?;
    store_dossier_vignette_from_rgba(largeur, hauteur, &rgba)
}

/// **LA MÊME RECETTE, À PARTIR DE PIXELS DÉJÀ EN MÉMOIRE** — pour une vignette qu'on n'a
/// pas lue sur le disque : celle qu'on **GÉNÈRE** à l'épinglage, depuis l'icône Windows
/// du dossier (voir `app::vignette_generee_pour`).
///
/// ⚠️ **LA VERSION « CHEMIN » DÉLÈGUE ICI, ET C'EST TOUT L'INTÉRÊT** : les deux doivent
/// produire le MÊME nom de fichier pour les mêmes pixels (`hashed_png_name`), sans quoi la
/// même image serait rangée deux fois sous deux noms. Une seule recette, donc un seul
/// endroit où la borner (`DOSSIER_VIGNETTE_MAX_SIDE`) et un seul endroit où la dédupliquer.
pub fn store_dossier_vignette_from_rgba(largeur: u32, hauteur: u32, rgba: &[u8]) -> Option<String> {
    // ⚠️ **LA GARDE EST *AVANT* `make_thumbnail`, ET ELLE DOIT Y ÊTRE** : `RgbaImage::
    // from_raw` accepte un 0×0 (0 octet = 0 × 0 × 4), et `make_thumbnail` relève alors les
    // deux côtés à 1 (`max(1)`) avant de **redimensionner une image vide** — un cas que
    // rien ne mesure et qui n'a pas de sens. On refuse l'entrée vide ici, où l'intention se
    // lit ; un tampon tronqué (`len` ≠ largeur × hauteur × 4) est refusé par la même ligne.
    let attendu = (largeur as usize).checked_mul(hauteur as usize)?.checked_mul(4)?;
    if largeur == 0 || hauteur == 0 || rgba.len() < attendu {
        return None;
    }
    let (largeur, hauteur, rgba) =
        clipboard::make_thumbnail(largeur, hauteur, rgba, DOSSIER_VIGNETTE_MAX_SIDE);
    // Second filet : `make_thumbnail` rend `(0, 0, &[])` quand `from_raw` a échoué. Sans
    // lui, on rangerait une vignette VIDE sous le nom bien formé d'un hachage de rien — un
    // fichier valide qui ne dessine rien, et qu'aucun contrôle ne signalerait.
    if largeur == 0 || hauteur == 0 || rgba.is_empty() {
        return None;
    }
    let dir = dossiers_images_dir()?;
    fs::create_dir_all(&dir).ok()?;
    let nom = hashed_png_name(&rgba);
    let chemin = dir.join(&nom);
    if !chemin.exists() {
        let png = clipboard::encode_png(largeur, hauteur, &rgba)?;
        fs::write(&chemin, png).ok()?;
    }
    Some(nom)
}

/// Relit les octets PNG d'une vignette, à la demande — jamais au démarrage en
/// masse : seule la vignette de chaque raccourci est nécessaire, et elle est
/// téléversée en texture une fois puis gardée (voir `app::run`).
///
/// Le nom est **revalidé** avant d'être joint au dossier (voir
/// `is_valid_dossier_vignette_name`) : il vient d'un fichier, pas d'une page,
/// mais c'est le seul champ qui deviendra un chemin.
pub fn load_dossier_vignette(name: &str) -> Option<Vec<u8>> {
    if !is_valid_dossier_vignette_name(name) {
        return None;
    }
    let mut path = dossiers_images_dir()?;
    path.push(name);
    fs::read(path).ok()
}

/// Relit l'historique du presse-papiers sauvegardé (voir
/// `save_clipboard_history`), le plus récent en tête — comme la convention
/// déjà suivie en mémoire (voir `app::run`). Une entrée dont l'image
/// référencée est manquante ou corrompue est simplement omise plutôt que de
/// faire échouer tout le rechargement (voir `ClipboardEntry::
/// from_png_bytes`) ; `%APPDATA%` introuvable ou fichier absent/mal formé
/// donnent un historique vide, comme un tout premier lancement.
pub fn load_clipboard_history() -> Vec<ClipboardEntry> {
    let Some(path) = clipboard_history_path() else { return Vec::new() };
    let Ok(content) = fs::read_to_string(&path) else { return Vec::new() };
    let table = match content.parse::<toml::Table>() {
        Ok(table) => table,
        Err(err) => {
            eprintln!("[neuradesk] clipboard_history.toml invalide, ignoré : {err}");
            return Vec::new();
        }
    };
    let Some(entries) = table.get("entries").and_then(|v| v.as_array()) else { return Vec::new() };
    let Some(images_dir) = clipboard_images_dir() else { return Vec::new() };

    entries
        .iter()
        .filter_map(|entry| match entry.get("type").and_then(|v| v.as_str())? {
            "text" => Some(ClipboardEntry::Text(entry.get("value")?.as_str()?.to_string())),
            "image" => {
                let file_name = entry.get("file")?.as_str()?;
                // ⚠️ **LE CHEMIN COMPLET EST TRANSMIS, pas seulement le nom** :
                // c'est lui que `ClipboardEntry` garde pour relire l'image à la
                // demande, sans avoir à retrouver le dossier (voir sa doc).
                let chemin = images_dir.join(file_name);
                let bytes = fs::read(&chemin).ok()?;
                ClipboardEntry::from_png_bytes(&bytes, chemin)
            }
            _ => None,
        })
        .collect()
}

/// Sauvegarde l'historique du presse-papiers, rechargé au prochain
/// lancement (voir `load_clipboard_history`). Réécrit la table entière à
/// chaque appel (même discipline que `categories.toml`/`usage.toml`), mais
/// SANS réencoder les images déjà présentes sur disque (identifiées par
/// leur nom de fichier déterministe, voir `clipboard_image_file_name`) —
/// seules les images réellement nouvelles sont encodées en PNG ; les
/// fichiers qui ne correspondent plus à aucune entrée de `history` (évincés
/// par la limite, voir `app::CLIPBOARD_HISTORY_LIMIT`) sont supprimés du
/// dossier, pour ne pas accumuler indéfiniment des images qu'on ne
/// reverra plus. Best-effort silencieux si `%APPDATA%` est inaccessible en
/// écriture, comme le reste de ce module.
pub fn save_clipboard_history(history: &mut [ClipboardEntry]) {
    let Some(images_dir) = clipboard_images_dir() else { return };
    let _ = fs::create_dir_all(&images_dir);

    let mut entries = Vec::new();
    let mut kept_files = std::collections::HashSet::new();

    for entry in history.iter_mut() {
        // Le texte d'abord, et on repart : `if let` plutôt qu'un `match`,
        // pour que l'emprunt sur `entry` soit terminé avant `persister_image`,
        // qui a besoin de la variante MUTABLE.
        if let ClipboardEntry::Text(text) = entry {
            let mut row = toml::Table::new();
            row.insert("type".to_string(), toml::Value::String("text".to_string()));
            row.insert("value".to_string(), toml::Value::String(text.clone()));
            entries.push(toml::Value::Table(row));
            continue;
        }

        let Some((file_name, width, height)) = persister_image(entry, &images_dir) else { continue };
        kept_files.insert(file_name.clone());

        let mut row = toml::Table::new();
        row.insert("type".to_string(), toml::Value::String("image".to_string()));
        row.insert("file".to_string(), toml::Value::String(file_name));
        row.insert("width".to_string(), toml::Value::Integer(width as i64));
        row.insert("height".to_string(), toml::Value::Integer(height as i64));
        entries.push(toml::Value::Table(row));
    }

    let mut table = toml::Table::new();
    table.insert("entries".to_string(), toml::Value::Array(entries));
    write_config("clipboard_history.toml", table.to_string());

    if let Ok(read_dir) = fs::read_dir(&images_dir) {
        for entry in read_dir.flatten() {
            let name = entry.file_name();
            if !kept_files.contains(name.to_string_lossy().as_ref()) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// **PERSISTE UNE ENTRÉE IMAGE**, et rend `(nom de fichier, largeur, hauteur)`.
///
/// ⭐ **LE NOM SE DÉDUIT DU CONTENU — DONC ON NE LE CALCULE QU'UNE FOIS.** Une
/// entrée déjà persistée porte son chemin (voir `ClipboardEntry::fichier`) : le
/// recalculer exigerait le `rgba` pleine résolution, que le correctif du
/// 2026-10-08 a justement libéré. C'est ce qui rend le rechargement d'un
/// historique plein (244 images, ~2 Go) gratuit en mémoire.
///
/// ⚠️ **`None` POUR UNE ENTRÉE IMAGE SANS `rgba` NI FICHIER** — un état qui ne
/// devrait pas exister, mais qui vaut mieux qu'un `unwrap` : l'entrée est
/// simplement omise de la sauvegarde, comme une entrée dont le PNG est illisible
/// l'est du rechargement.
fn persister_image(
    entry: &mut ClipboardEntry,
    images_dir: &std::path::Path,
) -> Option<(String, u32, u32)> {
    let (width, height) = match entry {
        ClipboardEntry::Text(_) => return None,
        // Les dimensions sont COPIÉES : garder les références empêcherait
        // l'emprunt mutable de `marquer_persistee` plus bas.
        ClipboardEntry::Image { width, height, .. } => (*width, *height),
    };

    let connu = entry.fichier().map(|chemin| chemin.to_path_buf());
    let chemin = match connu {
        Some(chemin) => chemin,
        None => {
            let nom = clipboard_image_file_name(entry.rgba_en_memoire()?);
            let chemin = images_dir.join(nom);
            if !chemin.exists() {
                if let Some(png) = entry.png_a_ecrire() {
                    let _ = fs::write(&chemin, png);
                }
            }
            // ⛔ **LA MÉMOIRE EST RENDUE ICI** — le `rgba` pleine résolution n'a
            // plus aucune raison d'exister : le PNG le porte désormais.
            entry.marquer_persistee(chemin.clone());
            chemin
        }
    };

    let nom = chemin.file_name()?.to_string_lossy().into_owned();
    Some((nom, width, height))
}

/// Notes du dash "Notes" (voir `win32::notes`, `win32::cef_notes`) —
/// persistées dans `%APPDATA%\NeuraDesk\notes.toml`, même discipline que
/// `clipboard_history.toml` juste au-dessus (table réécrite en entier à
/// chaque sauvegarde). `notes_images/` (voir `notes_images_dir`) tient le
/// même rôle que `clipboard_images/` pour les images attachées (voir
/// `win32::notes::NoteImage`) — séparé de `clipboard_images/` : les deux ont
/// des cycles de vie indépendants (une image de note ne disparaît jamais par
/// éviction de limite comme le presse-papiers, seulement par suppression
/// explicite de sa note ou d'elle-même, voir `notes::referenced_image_files`).
fn notes_path() -> Option<PathBuf> {
    config_path("notes.toml")
}

fn notes_images_dir() -> Option<PathBuf> {
    let mut path = config_dir()?;
    path.push("notes_images");
    Some(path)
}

/// `%APPDATA%\NeuraDesk\palettes\` — la bibliothèque de palettes de l'applet
/// « Sélecteur de couleurs », une par fichier `.gpl` (format GIMP, que
/// Inkscape lit nativement). Même famille que `clipboard_images_dir` et
/// `notes_images_dir` ci-dessus, et **surtout PAS `config_path`**, qui ne sait
/// désigner qu'un nom de fichier à la racine de `config_dir`.
///
/// Ce dossier est accessoirement un point d'échange : y déposer un `.gpl`
/// exporté depuis GIMP ou Inkscape suffit à le voir dans l'applet.
pub(crate) fn palettes_dir() -> Option<PathBuf> {
    let mut path = config_dir()?;
    path.push("palettes");
    Some(path)
}

/// Persiste une image collée dans une note : pleine résolution (`<hash>.png`)
/// ET miniature pré-redimensionnée (`<hash>.thumb.png`, voir
/// `notes::thumb_file_name`), écrites une seule fois chacune (déduplication
/// par contenu — même principe que `clipboard_image_file_name` — un fichier
/// déjà présent n'est jamais réécrit). `None` si `rgba` n'encode pas en PNG
/// ou si `%APPDATA%` est inaccessible : la note se crée alors sans image
/// plutôt que d'échouer tout le collage (voir `win32::cef_notes::
/// handle_paste`).
pub fn store_note_image(width: u32, height: u32, rgba: &[u8]) -> Option<NoteImage> {
    let images_dir = notes_images_dir()?;
    fs::create_dir_all(&images_dir).ok()?;

    let file = hashed_png_name(rgba);
    let full_path = images_dir.join(&file);
    if !full_path.exists() {
        let png = clipboard::encode_png(width, height, rgba)?;
        fs::write(&full_path, png).ok()?;
    }

    let thumb_name = notes::thumb_file_name(&file);
    let thumb_path = images_dir.join(&thumb_name);
    let thumb_bytes = if thumb_path.exists() {
        fs::read(&thumb_path).ok()?
    } else {
        let (thumb_width, thumb_height, thumb_rgba) = clipboard::make_thumbnail(width, height, rgba, notes::NOTE_THUMB_MAX_SIDE);
        let png = clipboard::encode_png(thumb_width, thumb_height, &thumb_rgba)?;
        fs::write(&thumb_path, &png).ok()?;
        png
    };

    let thumb_data_uri = format!("data:image/png;base64,{}", BASE64.encode(&thumb_bytes));
    Some(NoteImage { file, width, height, thumb_data_uri })
}

/// Relit les octets PNG pleine résolution d'une image de note, à la demande
/// (voir `win32::cef_notes::handle_full_image`) — jamais chargés en même
/// temps que le reste de `load_notes` : seule la miniature (déjà en mémoire
/// via `NoteImage::thumb_data_uri`) est nécessaire pour l'affichage normal de
/// la liste, la pleine résolution ne sert qu'à l'agrandissement à la
/// demande. `file` provient toujours de `NoteImage::file`, lui-même toujours
/// produit par `hashed_png_name` (`store_note_image`) ou déjà validé au
/// chargement (`load_note_image_row`) — la revalidation ici est une pure
/// défense en profondeur (voir `is_valid_note_image_file_name`) au cas où un
/// futur appelant passerait un `file` non fiable.
pub fn load_note_image_png(file: &str) -> Option<Vec<u8>> {
    if !is_valid_note_image_file_name(file) {
        return None;
    }
    let images_dir = notes_images_dir()?;
    fs::read(images_dir.join(file)).ok()
}

/// Relit les notes sauvegardées (voir `save_notes`). Une ligne dont l'`id`
/// ou le `content` est absent/mal typé est simplement omise plutôt que de
/// faire échouer tout le rechargement (même discipline que
/// `load_clipboard_history`) ; `pinned` par défaut à `false` et
/// `updated_at` par défaut à `created_at` si absents (compatibilité avant-
/// arrière si le schéma s'enrichit plus tard). `%APPDATA%` introuvable ou
/// fichier absent/mal formé donnent une liste vide, comme un tout premier
/// lancement.
/// Lit l'alarme d'une ligne `notes[]` — **PURE**, et c'est délibéré : `save_notes`
/// écrit dans `%APPDATA%\NeuraDesk\notes.toml`, c'est-à-dire le VRAI fichier de
/// l'utilisateur, donc **aucun test ne doit passer par le couple
/// `save_notes`/`load_notes`** pour vérifier une forme de fichier. Cette fonction
/// prend la ligne déjà analysée et rend ce qu'elle dit.
///
/// Rend `None` s'il n'y a pas de table `alarme` — le cas de tout `notes.toml`
/// écrit avant que l'alarme existe.
/// ⚠️ **LA DATE ET L'HEURE SONT DES CHAÎNES** (`"2026-09-25"`, `"15:30"`), pas des
/// entiers : `notes.toml` est un fichier qu'on relit à la main, et
/// `2026-09-25` s'y lit mieux que `1790000000`. Les deux sont FACULTATIVES — une
/// alarme sans date sonne tout de suite, une alarme sans heure sonne dès le début
/// de son jour.
pub(crate) fn lire_alarme_note(entry: &toml::Value) -> Option<notes::AlarmeNote> {
    let table = entry.get("alarme")?.as_table()?;
    Some(notes::AlarmeNote {
        date: table.get("date").and_then(|v| v.as_str()).and_then(analyser_date_alarme),
        heure: table.get("heure").and_then(|v| v.as_str()).and_then(analyser_heure_alarme),
        jours: table.get("jours").and_then(|v| v.as_array()).map(|v| lire_jours_alarme(v)).unwrap_or(0),
    })
}

/// `["lun", "mer"]` → le masque de `AlarmeNote::jours`. Un nom inconnu est
/// ignoré plutôt que de faire perdre l'alarme entière.
fn lire_jours_alarme(noms: &[toml::Value]) -> u8 {
    noms.iter()
        .filter_map(|v| v.as_str())
        .filter_map(|nom| notes::JOURS_SEMAINE.iter().position(|j| *j == nom))
        .fold(0, |masque, i| masque | (1 << i))
}

/// `"AAAA-MM-JJ"` → `(annee, mois, jour)`. Rend `None` si la forme n'y est pas.
fn analyser_date_alarme(texte: &str) -> Option<(i32, u32, u32)> {
    let mut parts = texte.split('-');
    let annee: i32 = parts.next()?.parse().ok()?;
    let mois: u32 = parts.next()?.parse().ok()?;
    let jour: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&mois) || !(1..=31).contains(&jour) {
        return None;
    }
    Some((annee, mois, jour))
}

/// `"HH:MM"` → `(heure, minute)`.
fn analyser_heure_alarme(texte: &str) -> Option<(u32, u32)> {
    let (h, m) = texte.split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    ((0..=23).contains(&h) && (0..=59).contains(&m)).then_some((h, m))
}

/// **LES DEUX SONS DE RAPPEL — UN PAR FAMILLE.** DEMANDE EXPLICITE (2026-09-24) :
/// « j'aimerais pouvoir choisir les alarmes pour les rappels de notes et les
/// rappels de dates ».
///
/// ⚠️ **POURQUOI DEUX RÉGLAGES ET NON UN** : les deux familles ont désormais
/// chacune leur icône et leur clignotement (une fête vaut pour un JOUR, une
/// alarme de note pour un INSTANT) ; un son unique aurait été le dernier fil qui
/// les relie. On doit pouvoir reconnaître à l'oreille laquelle sonne.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FamilleRappel {
    Notes,
    Dates,
}

impl FamilleRappel {
    /// Le fichier de CE réglage. Deux fichiers, jamais un seul : c'est ce qui
    /// rend les deux choix indépendants.
    fn fichier(self) -> &'static str {
        match self {
            FamilleRappel::Notes => "son_rappel_notes.txt",
            FamilleRappel::Dates => "son_rappel_dates.txt",
        }
    }
}

/// **LE SON D'UNE FAMILLE DE RAPPELS** — un chemin de fichier, ou `None` si
/// l'utilisateur n'en a pas encore choisi (la sonnerie retombe alors sur le
/// premier son du dossier, voir `app::run`).
///
/// ⚠️ **LU COMME DU TEXTE, POUR LA MÊME RAISON QUE `load_rappel_ack`** : le chemin
/// générique des réglages (`load_bool_setting` et ses voisins) passe par
/// `read_config_floats`, qui parse le contenu comme des flottants — un chemin
/// commençant par `C:` ou `\\` en ressort **vide**, sans erreur. Le fichier est
/// donc lu et écrit directement.
///
/// ⚠️ **LE RÉGLAGE D'AVANT `son_alarme.txt` SERT DE REPLI** : il n'y avait qu'un
/// son, et l'utilisateur en avait choisi un. L'ignorer lui ferait perdre son
/// choix en silence au premier lancement de cette version — le repli le lui
/// garde pour les DEUX familles, jusqu'à ce qu'il en change une. (Pas de
/// migration qui écrase : on ne réécrit pas un fichier de l'utilisateur pour lui.)
pub fn load_son_rappel(famille: FamilleRappel) -> Option<String> {
    lire_chemin_de_son(famille.fichier()).or_else(|| lire_chemin_de_son("son_alarme.txt"))
}

/// Écrit le son choisi pour une famille. Un chemin vide efface le choix — même
/// geste que « automatique » ailleurs.
pub fn save_son_rappel(famille: FamilleRappel, chemin: &str) {
    write_config(famille.fichier(), chemin.to_string());
}

/// Un chemin de son lu d'un fichier de configuration, ou `None` s'il est absent
/// ou vide. Le `trim` n'est pas cosmétique : `write_config` ajoute un saut de
/// ligne, et un chemin qui le porterait ne comparerait plus égal à lui-même.
fn lire_chemin_de_son(fichier: &str) -> Option<String> {
    let texte = fs::read_to_string(config_path(fichier)?).ok()?;
    let texte = texte.trim().to_string();
    (!texte.is_empty()).then_some(texte)
}

#[cfg(test)]
mod alarme_note_tests {
    use super::*;

    fn ligne(toml_src: &str) -> toml::Value {
        toml_src.parse::<toml::Table>().unwrap().into()
    }

    /// **UN `notes.toml` ÉCRIT AVANT L'ALARME RESTE LISIBLE** — c'est la
    /// compatibilité que la doc de `load_notes` promet, et c'est le cas le plus
    /// fréquent : toutes les notes existantes sont dans ce cas.
    #[test]
    fn une_note_sans_table_alarme_n_a_pas_d_alarme() {
        let note = ligne(r#"id = 1
content = "du texte""#);
        assert_eq!(lire_alarme_note(&note), None);
    }

    /// **« TOUT DE SUITE » SE DIT PAR L'ABSENCE DE LA CLÉ** — TOML n'a pas de
    /// `null`, donc `a_partir_de` absent vaut `None`. ⚠️ Un zéro serait un piège :
    /// il voudrait dire « à partir du 1ᵉʳ janvier 1970 », donc « sonne ».
    #[test]
    fn une_alarme_sans_heure_sonne_tout_de_suite() {
        let note = ligne(r#"id = 1
content = "x"

[alarme]"#);
        let alarme = lire_alarme_note(&note).expect("la table alarme existe");
        assert_eq!(alarme.date, None);
        assert_eq!(alarme.heure, None);
        assert!(alarme.sonne_a((2026, 9, 25), (0, 0)), "sans date ni heure, elle sonne tout de suite");
    }

    #[test]
    fn une_alarme_a_heure_porte_son_instant() {
        let note = ligne(r#"id = 1
content = "x"

[alarme]
heure = "15:30""#);
        let alarme = lire_alarme_note(&note).expect("la table alarme existe");
        assert_eq!(alarme.heure, Some((15, 30)));
        assert_eq!(alarme.date, None, "l'heure seule laisse la date libre");
        assert_eq!(alarme.jours, 0, "sans `jours`, une alarme sonne une fois");
    }

    #[test]
    fn les_jours_de_repetition_se_lisent_par_leur_nom() {
        let note = ligne(r#"id = 1
content = "x"

[alarme]
heure = "08:00"
jours = ["lun", "ven", "inconnu"]"#);
        let alarme = lire_alarme_note(&note).expect("la table alarme existe");
        assert_eq!(alarme.jours, 0b0001_0001);
    }
}

thread_local! {
    /// **LES NOTES ONT-ELLES PU ÊTRE LUES ?** — faux quand `load_notes` a rendu
    /// `None` : fichier verrouillé, illisible, ou TOML invalide.
    ///
    /// ⛔ **C'EST LA GARDE QUI EMPÊCHE LA PERTE DÉFINITIVE**, et sans elle les deux
    /// autres moitiés de ce correctif ne serviraient à rien. Avant, une lecture
    /// ratée rendait une liste **VIDE** — indistinguable de « l'utilisateur n'a
    /// aucune note » — et la première action venue (épingler, créer, supprimer)
    /// appelait `save_notes`, qui réécrivait le fichier avec cette liste vide :
    /// **toutes les notes effacées, sans un mot.**
    ///
    /// ⚠️ Le cas n'est pas théorique sur cette machine : le dépôt vit sous
    /// OneDrive, et un simple verrou transitoire de `read_to_string` suffisait.
    static NOTES_LISIBLES: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Voir `NOTES_LISIBLES`, doc de la cellule — `false` veut dire « ne réécrivez
/// surtout pas le fichier ».
pub fn notes_sont_lisibles() -> bool {
    NOTES_LISIBLES.with(std::cell::Cell::get)
}

/// **CE QU'IL FAUT DIRE À L'UTILISATEUR QUAND SES NOTES N'ONT PAS PU ÊTRE LUES** —
/// `None` quand tout va bien.
///
/// ⚠️ **IL NOMME LE CHEMIN COMPLET**, et c'est le point : le panneau Notes est le
/// seul endroit où l'utilisateur peut l'apprendre (le journal de diagnostic n'est
/// pas allumé par défaut), et « une erreur est survenue » ne l'aiderait pas à
/// retrouver le fichier. C'est la même règle que Xournal++, qui affiche le chemin
/// complet d'un PDF manquant — et que ce dépôt applique déjà aux sons.
pub fn avertissement_notes() -> Option<String> {
    if notes_sont_lisibles() {
        return None;
    }
    let chemin = notes_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "notes.toml".to_string());
    Some(format!(
        "Vos notes n'ont pas pu être lues : {chemin}\n\
         RIEN ne sera enregistré, pour ne pas les effacer. Fermez NeuraDesk et vérifiez ce fichier — \
         les versions précédentes sont à côté, en notes.toml.1, .2 et .3."
    ))
}

/// **DÉCALE UN FICHIER DE DONNÉES AVANT DE LE RÉÉCRIRE** — `notes.toml` →
/// `notes.toml.1` → `.2` → `.3`.
///
/// ⛔ **LE MÊME MOTIF QUE LE JOURNAL DE DIAGNOSTIC** (`diag::rotate_previous`), et
/// pour la même raison — payée deux fois là-bas (« ROTATION AU LIEU DE
/// TRONCATURE, ET C'EST UNE LEÇON PAYÉE DEUX FOIS ») : un fichier qu'on réécrit
/// ENTIER ne pardonne pas une écriture ratée. Ici il n'y a pas de journal pour le
/// dire après coup : ce sont les notes de l'utilisateur.
///
/// ⚠️ On tourne du plus ANCIEN au plus RÉCENT, pour ne jamais écraser une
/// rotation qu'on veut garder.
fn rotate_data_file(path: &std::path::Path, rotations: usize) {
    if !fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false) {
        return;
    }
    let Some(nom) = path.file_name().and_then(|n| n.to_str()) else { return };
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let nomme = |n: usize| dir.join(format!("{nom}.{n}"));
    let _ = fs::remove_file(nomme(rotations));
    for n in (1..rotations).rev() {
        let depuis = nomme(n);
        if depuis.exists() {
            let _ = fs::rename(&depuis, nomme(n + 1));
        }
    }
    let _ = fs::rename(path, nomme(1));
}

/// **RELIT LES NOTES SAUVEGARDÉES** — `None` quand le fichier EXISTE mais qu'on
/// n'a pas pu le lire ou le comprendre, `Some(liste)` sinon.
///
/// ⛔ **LA DISTINCTION ENTRE `None` ET `Some(vec![])` EST TOUT LE SUJET DE CETTE
/// FONCTION.** Les deux rendaient autrefois une liste vide, et l'appelant ne
/// pouvait donc pas distinguer « l'utilisateur n'a aucune note » — un premier
/// lancement légitime — de « je n'ai pas su lire ses notes ». La seconde réponse
/// menait droit à leur effacement, puisque la première écriture réécrit le
/// fichier entier (voir `save_notes`, qui REFUSE désormais).
///
/// Rend `Some(vec![])` : un fichier ABSENT (premier lancement), ou un TOML valide
/// qui ne porte aucune note.
///
/// Rend `None` : lecture impossible (verrou, droits, OneDrive non matérialisé) ou
/// TOML invalide — **et le dit, en nommant le chemin complet** : c'est le fichier
/// le plus précieux de l'application, il n'a pas le droit d'échouer en silence.
pub fn load_notes() -> Option<Vec<Note>> {
    let Some(path) = notes_path() else {
        crate::diag!("[notes] dossier %APPDATA% introuvable — aucune note chargée, AUCUNE écriture ne sera faite");
        NOTES_LISIBLES.with(|c| c.set(false));
        return None;
    };
    if !path.exists() {
        return Some(Vec::new());
    }
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) => {
            crate::diag!(
                "[notes] {} ILLISIBLE ({err}) — aucune note chargée, et AUCUNE écriture ne sera faite pour ne pas les effacer",
                path.display()
            );
            NOTES_LISIBLES.with(|c| c.set(false));
            return None;
        }
    };
    let table = match content.parse::<toml::Table>() {
        Ok(table) => table,
        Err(err) => {
            crate::diag!(
                "[notes] {} INVALIDE ({err}) — AUCUNE écriture ne sera faite pour ne pas effacer vos notes. Les fichiers notes.toml.1, .2 et .3 sont les versions précédentes",
                path.display()
            );
            NOTES_LISIBLES.with(|c| c.set(false));
            return None;
        }
    };
    let Some(entries) = table.get("notes").and_then(|v| v.as_array()) else { return Some(Vec::new()) };
    let images_dir = notes_images_dir();

    let notes: Vec<Note> = entries
        .iter()
        .filter_map(|entry| {
            let id = u64::try_from(entry.get("id")?.as_integer()?).ok()?;
            let content = entry.get("content")?.as_str()?.to_string();
            let created_at = entry.get("created_at").and_then(|v| v.as_integer()).unwrap_or(0);
            let updated_at = entry.get("updated_at").and_then(|v| v.as_integer()).unwrap_or(created_at);
            let pinned = entry.get("pinned").and_then(|v| v.as_bool()).unwrap_or(false);
            let images = entry
                .get("images")
                .and_then(|v| v.as_array())
                .map(|rows| rows.iter().filter_map(|row| load_note_image_row(row, images_dir.as_deref())).collect())
                .unwrap_or_default();
            // **L'ALARME, ABSENTE DES FICHIERS ANCIENS** — même politique que les
            // champs voisins (voir la doc au-dessus de `load_notes`) : un
            // `notes.toml` écrit avant l'alarme doit rester lisible, donc une
            // table `alarme` absente vaut « pas d'alarme ». ⚠️ Et un fichier écrit
            // À LA MAIN sans ce champ ne doit pas mettre toutes les notes à
            // sonner — d'où `unwrap_or(None)` et non un défaut qui sonne.
            let alarme = lire_alarme_note(entry);
            // Absent des fichiers d'avant les archives : une note n'y est pas.
            let archived = entry.get("archived").and_then(|v| v.as_bool()).unwrap_or(false);
            Some(Note { id, content, pinned, created_at, updated_at, images, alarme, archived })
        })
        .collect();
    Some(notes)
}

/// Reconstruit une `NoteImage` à partir d'une ligne TOML `images[]`. La
/// miniature est normalement relue TELLE QUELLE depuis son fichier
/// `.thumb.png` (voir `store_note_image`, déjà à la bonne taille) plutôt que
/// redécodée/redimensionnée depuis la pleine résolution comme le fait
/// `ClipboardEntry::from_png_bytes` : contrairement au presse-papiers (qui ne
/// persiste que la pleine résolution), les deux tailles sont ici côte à côte
/// sur disque, la relecture n'a donc normalement rien à recalculer.
///
/// Si le fichier miniature est manquant/corrompu (crash entre les deux
/// `fs::write` de `store_note_image`, disque plein, suppression manuelle...)
/// alors que la pleine résolution est intacte, on régénère la miniature au
/// lieu d'abandonner l'image (voir `regenerate_note_thumbnail`) : sans cette
/// relève, l'image disparaîtrait silencieusement de la note ici, puis le
/// prochain `save_notes` la supprimerait DÉFINITIVEMENT du disque (elle ne
/// serait alors plus référencée par aucune note, voir `notes::
/// referenced_image_files`). `None` (image omise, la note reste valide sans
/// elle) seulement si la pleine résolution est, elle aussi, introuvable/
/// corrompue, ou si `images_dir` est indisponible.
fn load_note_image_row(row: &toml::Value, images_dir: Option<&std::path::Path>) -> Option<NoteImage> {
    let file = row.get("file")?.as_str()?.to_string();
    let width = u32::try_from(row.get("width")?.as_integer()?).ok()?;
    let height = u32::try_from(row.get("height")?.as_integer()?).ok()?;
    if !is_valid_note_image_file_name(&file) {
        return None;
    }
    let images_dir = images_dir?;
    let thumb_path = images_dir.join(notes::thumb_file_name(&file));
    let thumb_bytes = match fs::read(&thumb_path) {
        Ok(bytes) => bytes,
        Err(_) => regenerate_note_thumbnail(images_dir, &file, &thumb_path)?,
    };
    let thumb_data_uri = format!("data:image/png;base64,{}", BASE64.encode(&thumb_bytes));
    Some(NoteImage { file, width, height, thumb_data_uri })
}

/// Relève de `load_note_image_row` quand le fichier `.thumb.png` est
/// manquant/corrompu : redécode la pleine résolution (voir `clipboard::
/// decode_png`) et regénère la miniature avec la même recette que
/// `store_note_image` (voir `clipboard::make_thumbnail`). Réécrit aussi le
/// fichier miniature sur disque une fois régénérée (auto-guérison : plus
/// besoin de refaire ce travail au chargement suivant). `None` si la pleine
/// résolution elle-même est introuvable/corrompue ou n'encode pas en PNG.
fn regenerate_note_thumbnail(images_dir: &std::path::Path, file: &str, thumb_path: &std::path::Path) -> Option<Vec<u8>> {
    let full_bytes = fs::read(images_dir.join(file)).ok()?;
    let (width, height, rgba) = clipboard::decode_png(&full_bytes)?;
    let (thumb_width, thumb_height, thumb_rgba) = clipboard::make_thumbnail(width, height, &rgba, notes::NOTE_THUMB_MAX_SIDE);
    let png = clipboard::encode_png(thumb_width, thumb_height, &thumb_rgba)?;
    let _ = fs::write(thumb_path, &png);
    Some(png)
}

/// Sauvegarde les notes, rechargées au prochain lancement (voir
/// `load_notes`). Réécrit la table entière à chaque appel, best-effort
/// silencieux si `%APPDATA%` est inaccessible en écriture — même discipline
/// que `save_clipboard_history`. Nettoie aussi `notes_images/` des fichiers
/// (pleine résolution ET miniature) qui ne sont plus référencés par aucune
/// note (voir `notes::referenced_image_files`) — image retirée d'une note ou
/// note supprimée, même principe que `save_clipboard_history`.
/// Le fichier des couleurs d'étiquettes — voir `load_tag_colors`.
const TAG_COLORS_FILE: &str = "tag_colors.toml";

/// **LES COULEURS CHOISIES POUR LES ÉTIQUETTES** — nom d'étiquette → `#rrggbb`.
///
/// DEMANDE EXPLICITE (2026-09-24) : « choisies par l'utilisateur ». Un fichier à
/// part plutôt qu'une clé ailleurs : les étiquettes n'ont pas d'autre réglage, et
/// un fichier par sujet est la discipline de ce dossier.
///
/// ⚠️ **LA CLÉ EST LE NOM DE L'ÉTIQUETTE TEL QU'IL S'ÉCRIT** — `#` compris
/// (`#site_Pierre`). C'est ce que la page renvoie, et c'est ce que `extract_tags`
/// produit ; chercher une forme normalisée ferait une table qui ne correspond à
/// rien, en silence.
pub fn load_tag_colors() -> std::collections::HashMap<String, String> {
    let Some(path) = config_path(TAG_COLORS_FILE) else { return std::collections::HashMap::new() };
    let Ok(content) = fs::read_to_string(path) else { return std::collections::HashMap::new() };
    let Ok(table) = content.parse::<toml::Table>() else { return std::collections::HashMap::new() };
    table
        .get("couleurs")
        .and_then(|v| v.as_table())
        .map(|t| {
            t.iter()
                .filter_map(|(nom, valeur)| valeur.as_str().map(|c| (nom.clone(), c.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Écrit la couleur d'une étiquette. **Une couleur VIDE efface le choix** — l'étiquette
/// reprend alors la teinte par défaut, ce qui est le seul moyen de revenir en arrière.
pub fn save_tag_color(etiquette: &str, couleur: &str) {
    let mut couleurs = load_tag_colors();
    if couleur.is_empty() {
        couleurs.remove(etiquette);
    } else {
        couleurs.insert(etiquette.to_string(), couleur.to_string());
    }
    let mut table_couleurs = toml::Table::new();
    for (nom, valeur) in &couleurs {
        table_couleurs.insert(nom.clone(), toml::Value::String(valeur.clone()));
    }
    let mut table = toml::Table::new();
    table.insert("couleurs".to_string(), toml::Value::Table(table_couleurs));
    write_config(TAG_COLORS_FILE, table.to_string());
}

/// Le fichier des vues enregistrées du panneau Notes — voir `load_vues_notes`.
const VUES_NOTES_FILE: &str = "notes_vues.toml";

/// **LES VUES ENREGISTRÉES DU PANNEAU NOTES** — DEMANDE EXPLICITE (2026-10-11),
/// d'après memos : retenir un filtre (recherche, étiquette, épinglées, archives)
/// sous un nom, pour y revenir d'un clic. Un fichier à part, comme les couleurs
/// d'étiquettes : ce n'est pas une donnée des notes, et `notes.toml` (le fichier
/// précieux) n'a pas à être réécrit pour un filtre.
pub fn load_vues_notes() -> Vec<notes::VueNotes> {
    let Some(path) = config_path(VUES_NOTES_FILE) else { return Vec::new() };
    let Ok(content) = fs::read_to_string(path) else { return Vec::new() };
    vues_notes_depuis_toml(&content)
}

fn vues_notes_depuis_toml(texte: &str) -> Vec<notes::VueNotes> {
    let Ok(table) = texte.parse::<toml::Table>() else { return Vec::new() };
    let Some(lignes) = table.get("vues").and_then(|v| v.as_array()) else { return Vec::new() };
    let s = |v: &toml::Value, cle: &str| v.get(cle).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let b = |v: &toml::Value, cle: &str| v.get(cle).and_then(|x| x.as_bool()).unwrap_or(false);
    lignes
        .iter()
        .filter_map(|v| {
            notes::VueNotes::nouvelle(&s(v, "nom"), &s(v, "recherche"), &s(v, "etiquette"), b(v, "epinglees"), b(v, "archivees"))
        })
        .take(notes::MAX_VUES_NOTES)
        .collect()
}

fn vues_notes_vers_toml(vues: &[notes::VueNotes]) -> String {
    use toml::Value;
    let lignes = vues
        .iter()
        .map(|vue| {
            let mut t = toml::Table::new();
            t.insert("nom".into(), Value::String(vue.nom.clone()));
            t.insert("recherche".into(), Value::String(vue.recherche.clone()));
            t.insert("etiquette".into(), Value::String(vue.etiquette.clone()));
            t.insert("epinglees".into(), Value::Boolean(vue.epinglees));
            t.insert("archivees".into(), Value::Boolean(vue.archivees));
            Value::Table(t)
        })
        .collect();
    let mut table = toml::Table::new();
    table.insert("vues".into(), Value::Array(lignes));
    table.to_string()
}

pub fn save_vues_notes(vues: &[notes::VueNotes]) {
    write_config(VUES_NOTES_FILE, vues_notes_vers_toml(vues));
}

/// Nombre de versions précédentes conservées — voir `rotate_data_file`.
const ROTATIONS_NOTES: usize = 3;

pub fn save_notes(notes: &[Note]) {
    // ⛔ **ON N'ÉCRIT PAS APRÈS UNE LECTURE RATÉE.** Sans ce garde, la liste en
    // mémoire est celle d'une lecture ratée — VIDE — et cette fonction réécrit le
    // fichier ENTIER : les notes de l'utilisateur disparaîtraient pour de bon.
    // Et pas seulement le fichier : la purge d'images plus bas supprimerait tous
    // les fichiers de `notes_images/`, puisque plus aucune note ne les référence.
    // ⚠️ Deux effacements dans la même fonction, donc, et une seule garde les
    // arrête (voir `NOTES_LISIBLES`).
    if !notes_sont_lisibles() {
        crate::diag!(
            "[notes] ÉCRITURE REFUSÉE : les notes n'ont pas pu être lues au démarrage. Écrire maintenant les effacerait. Fermez NeuraDesk, vérifiez %APPDATA%\\NeuraDesk\\notes.toml (et ses .1/.2/.3), puis relancez."
        );
        return;
    }
    let mut entries = Vec::new();
    for note in notes {
        let mut row = toml::Table::new();
        row.insert("id".to_string(), toml::Value::Integer(note.id as i64));
        row.insert("content".to_string(), toml::Value::String(note.content.clone()));
        row.insert("pinned".to_string(), toml::Value::Boolean(note.pinned));
        row.insert("created_at".to_string(), toml::Value::Integer(note.created_at));
        row.insert("updated_at".to_string(), toml::Value::Integer(note.updated_at));
        // Écrite seulement quand elle est vraie — le fichier des notes ordinaires
        // ne change pas.
        if note.archived {
            row.insert("archived".to_string(), toml::Value::Boolean(true));
        }
        let images: Vec<toml::Value> = note
            .images
            .iter()
            .map(|image| {
                let mut image_row = toml::Table::new();
                image_row.insert("file".to_string(), toml::Value::String(image.file.clone()));
                image_row.insert("width".to_string(), toml::Value::Integer(image.width as i64));
                image_row.insert("height".to_string(), toml::Value::Integer(image.height as i64));
                toml::Value::Table(image_row)
            })
            .collect();
        row.insert("images".to_string(), toml::Value::Array(images));
        // **L'ALARME, ÉCRITE SEULEMENT QUAND ELLE EXISTE.** ⚠️ TOML n'a pas de
        // `null` : `a_partir_de: None` (« sonne tout de suite ») se dit par
        // l'ABSENCE de la clé, jamais par un zéro — un `0` vaudrait « à partir de
        // 1970 », c'est-à-dire « sonne », ce qui serait juste par accident et
        // faux par intention.
        if let Some(alarme) = &note.alarme {
            let mut alarme_row = toml::Table::new();
            if let Some((a, m, j)) = alarme.date {
                alarme_row.insert("date".to_string(), toml::Value::String(format!("{a:04}-{m:02}-{j:02}")));
            }
            if let Some((h, min)) = alarme.heure {
                alarme_row.insert("heure".to_string(), toml::Value::String(format!("{h:02}:{min:02}")));
            }
            // Absente pour une alarme « une fois » — la forme d'avant la répétition.
            if alarme.jours != 0 {
                let jours = (0..7)
                    .filter(|i| alarme.jours & (1 << i) != 0)
                    .map(|i| toml::Value::String(notes::JOURS_SEMAINE[i].to_string()))
                    .collect();
                alarme_row.insert("jours".to_string(), toml::Value::Array(jours));
            }
            row.insert("alarme".to_string(), toml::Value::Table(alarme_row));
        }
        entries.push(toml::Value::Table(row));
    }
    let mut table = toml::Table::new();
    table.insert("notes".to_string(), toml::Value::Array(entries));
    // **LA VERSION PRÉCÉDENTE EST DÉCALÉE AVANT D'ÉCRIRE** (voir
    // `rotate_data_file`) : c'est ce qui rend une écriture ratée — ou un fichier
    // qu'on relit de travers — réparable à la main, sans avoir à espérer.
    if let Some(path) = notes_path() {
        rotate_data_file(&path, ROTATIONS_NOTES);
    }
    // ⚠️ **ÉCRITURE ATOMIQUE** (temporaire puis renommage), et non le
    // `write_config` ordinaire : `notes.toml` est le seul fichier du dossier dont
    // la perte définitive est irréparable. Même choix que `zones.toml`, et même
    // raison — une coupure en pleine écriture laisserait sinon un fichier tronqué,
    // c'est-à-dire, à la relecture, un TOML invalide.
    write_config_atomique("notes.toml", table.to_string());

    if let Some(images_dir) = notes_images_dir() {
        let kept = notes::referenced_image_files(notes);
        if let Ok(read_dir) = fs::read_dir(&images_dir) {
            for entry in read_dir.flatten() {
                let name = entry.file_name();
                if !kept.contains(name.to_string_lossy().as_ref()) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
}

/// Relit les catégories supplémentaires créées par l'utilisateur (voir
/// `save_custom_category`) — proposées en plus des catégories fixes (voir
/// `app_categories::fixed_categories`) dans le sélecteur du Dash, même
/// sans encore aucune application dedans.
pub fn load_custom_categories() -> Vec<String> {
    let table = load_categories_table();
    table
        .get("custom_categories")
        .and_then(|v| v.as_array())
        .map(|entries| entries.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

/// Ajoute `name` aux catégories supplémentaires, sauf si une catégorie
/// (fixe ou déjà créée) porte déjà ce nom à la casse près — pas de doublon
/// visuel dans le sélecteur pour une simple différence de casse.
pub fn save_custom_category(name: &str) {
    let already_known = app_categories_contains(name) || load_custom_categories().iter().any(|c| c.eq_ignore_ascii_case(name));
    if already_known {
        return;
    }
    let mut categories = load_custom_categories();
    categories.push(name.to_string());
    let mut table = load_categories_table();
    table.insert(
        "custom_categories".to_string(),
        toml::Value::Array(categories.into_iter().map(toml::Value::String).collect()),
    );
    write_categories_table(&table);
}

fn app_categories_contains(name: &str) -> bool {
    crate::win32::app_categories::fixed_categories().iter().any(|c| c.eq_ignore_ascii_case(name))
}

/// Nombre de lancements réels par application (voir `win32::app_categories::
/// FREQUENT`, `win32::apps::enumerate_apps`, qui s'en sert pour composer la
/// catégorie "Fréquemment utilisés") — stocké dans
/// `%APPDATA%\NeuraDesk\usage.toml`, à part de `categories.toml` (donnée
/// d'usage réel, pas un choix de classement comme les catégories) et de
/// `config.toml` (rien ici qu'on s'attend à éditer à la main). Même
/// discipline `toml::Table` que `categories.toml` : les clés sont des noms
/// d'application arbitraires.
fn usage_path() -> Option<PathBuf> {
    config_path("usage.toml")
}

fn load_usage_table() -> toml::Table {
    let Some(path) = usage_path() else { return toml::Table::new() };
    let Ok(content) = fs::read_to_string(&path) else { return toml::Table::new() };
    content.parse::<toml::Table>().unwrap_or_else(|err| {
        eprintln!("[neuradesk] usage.toml invalide, ignoré : {err}");
        toml::Table::new()
    })
}

fn write_usage_table(table: &toml::Table) {
    write_config("usage.toml", table.to_string());
}

/// Relit le nombre de lancements enregistrés, par nom d'application (clé
/// déjà en minuscules — voir `record_launch`) — voir doc de module. Une
/// entrée absente de la table vaut implicitement zéro lancement (jamais
/// enregistrée ici, voir `win32::apps::enumerate_apps`, qui ne considère
/// que les applications PRÉSENTES dans cette table pour la catégorie
/// "Fréquemment utilisés").
pub fn load_launch_counts() -> std::collections::HashMap<String, u64> {
    let table = load_usage_table();
    let Some(launches) = table.get("launches").and_then(|v| v.as_table()) else {
        return std::collections::HashMap::new();
    };
    launches
        .iter()
        .filter_map(|(name, count)| Some((name.to_lowercase(), u64::try_from(count.as_integer()?).ok()?)))
        .collect()
}

/// Incrémente de un le compteur de lancements de `app_name` (comparaison
/// insensible à la casse, comme `load_launch_counts`) — appelé à chaque
/// lancement RÉEL d'une application depuis le Dash (voir `win32::dash::
/// handle_message`), jamais pour un simple survol/une sélection.
pub fn record_launch(app_name: &str) {
    let mut table = load_usage_table();
    let launches = table.entry("launches").or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let Some(map) = launches.as_table_mut() else { return };
    let key = app_name.to_lowercase();
    let current = map.get(&key).and_then(|v| v.as_integer()).unwrap_or(0);
    map.insert(key, toml::Value::Integer(current + 1));
    write_usage_table(&table);
}

/// Supprime la catégorie personnalisée `name` (créée via `save_custom_category`)
/// — retire aussi ce nom des catégories manuelles de toute application qui
/// le référençait (voir `add_category_override`), pour ne jamais laisser un
/// choix manuel pointer vers une catégorie qui n'existe plus ; une
/// application qui n'avait QUE celle-là en choix manuel retombe alors sur le
/// classement automatique. No-op si `name` ne correspond à AUCUNE catégorie
/// personnalisée connue (comparaison insensible à la casse) — en particulier
/// ne touche jamais aux catégories fixes (voir `app_categories::
/// fixed_categories`) : ce ne sont pas des données mais du classement
/// intégré au code, rien à "supprimer" ici, et un nom de catégorie fixe peut
/// tout à fait apparaître dans `overrides` (choix manuel d'une catégorie
/// fixe pour une application autrement classée ailleurs) sans qu'il faille
/// jamais l'en retirer.
pub fn delete_custom_category(name: &str) {
    let mut table = load_categories_table();

    let is_known_custom = table
        .get("custom_categories")
        .and_then(|v| v.as_array())
        .is_some_and(|entries| entries.iter().any(|v| v.as_str().is_some_and(|c| c.eq_ignore_ascii_case(name))));
    if !is_known_custom {
        return;
    }

    if let Some(customs) = table.get_mut("custom_categories").and_then(|v| v.as_array_mut()) {
        customs.retain(|v| !v.as_str().is_some_and(|c| c.eq_ignore_ascii_case(name)));
    }

    if let Some(map) = table.get_mut("overrides").and_then(|v| v.as_table_mut()) {
        let keys: Vec<String> = map.keys().cloned().collect();
        for key in keys {
            let Some(mut categories) = map.get(&key).and_then(value_to_categories) else { continue };
            let before = categories.len();
            categories.retain(|c| !c.eq_ignore_ascii_case(name));
            if categories.len() == before {
                continue; // ne référençait pas `name` : rien à réécrire
            }
            if categories.is_empty() {
                map.remove(&key);
            } else {
                map.insert(key, toml::Value::Array(categories.into_iter().map(toml::Value::String).collect()));
            }
        }
    }

    write_categories_table(&table);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **LE VA-ET-VIENT — LA GARDE QUI MANQUAIT, ET QUI RÉPOND À UNE VRAIE QUESTION.**
    ///
    /// « La configuration des composants de la barre va-t-elle être bien sauvegardée ? »
    /// se répond par une MESURE, pas par une promesse : ce que `barre_du_bas_to_toml`
    /// écrit, `barre_du_bas_from_toml` doit le rendre **à l'identique** — longueur, ordre,
    /// chemins, noms, vignettes, et **sortes**.
    ///
    /// ⚠️ **SUR UNE LISTE EN MÉMOIRE, JAMAIS SUR LE FICHIER** : `dossiers.toml` porte les
    /// dossiers épinglés de l'utilisateur, et un test qui y toucherait les modifierait
    /// pour de bon — c'est exactement pourquoi les deux cœurs sont purs.
    ///
    /// Les trois formes qui cohabitent sont couvertes : un dossier nu, un dossier avec
    /// nom et vignette, et un séparateur — dont la SORTE, le fait ajouté le 2026-10-01.
    #[test]
    fn l_arrangement_de_la_barre_du_bas_fait_un_va_et_vient_sans_perte() {
        let elements = vec![
            ElementBarre::Separateur(SorteEspace::Elastique),
            ElementBarre::Dossier(DossierEpingle {
                chemin: PathBuf::from(r"C:\essai\nu"),
                nom: None,
                vignette: None,
            }),
            ElementBarre::Dossier(DossierEpingle {
                chemin: PathBuf::from(r"C:\essai\complet"),
                nom: Some("Mon dossier".to_string()),
                vignette: Some("c7e2d5150ac70c1d.png".to_string()),
            }),
            ElementBarre::Separateur(SorteEspace::Rond),
            ElementBarre::Separateur(SorteEspace::Trait),
            ElementBarre::Dossier(DossierEpingle {
                chemin: PathBuf::from(r"E:\Vrac images"),
                nom: None,
                vignette: Some("3bb92537aec8ac45.png".to_string()),
            }),
        ];

        let table = barre_du_bas_to_toml(&elements);
        let relu = barre_du_bas_from_toml(&table);

        assert_eq!(relu.len(), 6, "le va-et-vient ne doit perdre AUCUNE ligne");
        assert_eq!(relu, elements, "et rendre la MEME liste, dans le MEME ordre");
    }

    /// ⭐ **LE PONT ENTRE LES DEUX ORDRES, SANS LEQUEL UN RÉORDONNANCEMENT SERAIT
    /// PERDU.** `dossiers.toml` porte l'ordre des ÉLÉMENTS ; le dock, lui, ne
    /// connaît que des INDEX D'ICÔNES. C'est le seul endroit où les deux se
    /// rencontrent — et une erreur de rang y écrirait la mauvaise liste. C'est
    /// exactement ainsi qu'un dossier du MILIEU a déjà disparu de ce fichier.
    #[test]
    fn les_index_du_dock_redonnent_les_elements_dans_leur_ordre() {
        use crate::ui::zones::SorteEspace;
        let dossier = |chemin: &str| {
            ElementBarre::Dossier(DossierEpingle {
                chemin: PathBuf::from(chemin),
                nom: None,
                vignette: None,
            })
        };
        let (a, s, b) = (
            dossier(r"C:\a"),
            ElementBarre::Separateur(SorteEspace::Trait),
            dossier(r"C:\b"),
        );
        let elements = vec![a.clone(), s.clone(), b.clone()];

        // **LES RANGS SONT CEUX DE `set_dossiers`** : deux dossiers en `base..base+2`,
        // le « + » à `base + 2`, les séparateurs à partir de `base + 3`.
        let (base, count) = (100usize, 2usize);

        // L'ordre que `set_dossiers` pose quand rien n'a bougé : A, S, B, puis le « + ».
        assert_eq!(
            elements_selon_l_ordre_du_dock(&elements, base, count, &[100, 103, 101, 102]),
            Some(vec![a.clone(), s.clone(), b.clone()]),
            "l'ordre normal doit se retraduire a l'identique"
        );
        // ⭐ **ET LE RÉORDONNANCEMENT** : B passe en tête, le séparateur suit.
        assert_eq!(
            elements_selon_l_ordre_du_dock(&elements, base, count, &[101, 103, 100, 102]),
            Some(vec![b, s, a]),
            "un glisser qui met B en tete doit se voir dans les ELEMENTS"
        );
    }

    /// ⛔ **ET ELLE REFUSE PLUTÔT QUE D'ÉCRIRE UNE LISTE QU'ELLE NE COMPREND PAS.**
    /// C'est la règle qui a manqué le jour où un dossier du milieu a disparu :
    /// écrire un état qu'on n'a pas su relire propage la perte au lieu de la subir.
    #[test]
    fn un_ordre_qui_ne_se_traduit_pas_est_refuse() {
        use crate::ui::zones::SorteEspace;
        let elements = vec![
            ElementBarre::Dossier(DossierEpingle {
                chemin: PathBuf::from(r"C:\a"),
                nom: None,
                vignette: None,
            }),
            ElementBarre::Separateur(SorteEspace::Rond),
        ];
        let (base, count) = (100usize, 1usize);

        // Un index ÉTRANGER — ni un dossier, ni le « + », ni un séparateur.
        assert_eq!(
            elements_selon_l_ordre_du_dock(&elements, base, count, &[100, 999, 101]),
            None,
            "un index inconnu ne doit pas etre saute en silence"
        );
        // Une rangée AMPUTÉE : la longueur ne tombe pas juste.
        assert_eq!(
            elements_selon_l_ordre_du_dock(&elements, base, count, &[100, 101]),
            None,
            "une rangee qui ne rend pas tous les elements doit etre refusee"
        );
    }

    /// ⭐ **LA PERMUTATION QUI MANQUAIT — SANS ELLE, CHAQUE ICÔNE PORTE LE CHEMIN
    /// D'UN AUTRE.** C'est le défaut mesuré : un glisser changeait la LISTE, et les
    /// vignettes — indexées par le RANG — restaient où elles étaient.
    #[test]
    fn la_permutation_des_dossiers_nomme_l_ancien_rang_de_chacun() {
        use crate::ui::zones::SorteEspace;
        let dossier = |chemin: &str| {
            ElementBarre::Dossier(DossierEpingle {
                chemin: PathBuf::from(chemin),
                nom: None,
                vignette: None,
            })
        };
        let (a, b, c) = (dossier(r"C:\a"), dossier(r"C:\b"), dossier(r"C:\c"));
        let avant = vec![a.clone(), b.clone(), c.clone()];

        // Rien n'a bougé : la permutation est l'identité.
        assert_eq!(permutation_des_dossiers(&avant, &avant), Some(vec![0, 1, 2]));
        // A passe en DERNIER : le nouveau rang 0 était l'ancien 1, le 1 était le 2,
        // le 2 était le 0. C'est cette table-là que les vignettes doivent suivre.
        assert_eq!(
            permutation_des_dossiers(&avant, &[b.clone(), c.clone(), a.clone()]),
            Some(vec![1, 2, 0])
        );
        // ⚠️ **ET LES SÉPARATEURS NE COMPTENT PAS** : ils n'ont pas de rang (voir
        // `EmplacementsDossiers`, qui n'en porte pas), donc glisser un dossier
        // par-dessus un séparateur ne change pas la permutation des chemins.
        let avec_separateur = vec![
            b.clone(),
            ElementBarre::Separateur(SorteEspace::Rond),
            c.clone(),
            a.clone(),
        ];
        assert_eq!(permutation_des_dossiers(&avant, &avec_separateur), Some(vec![1, 2, 0]));
    }

    /// ⛔ **ET ELLE REFUSE PLUTÔT QUE DE FABRIQUER UN TABLEAU AMPUTÉ** — la forme
    /// exacte des deux pertes subies par `dossiers.toml` (un dossier du MILIEU, qui
    /// comptait double ailleurs, ou qui manquait).
    #[test]
    fn une_permutation_qui_n_en_est_pas_une_est_refusee() {
        let dossier = |chemin: &str| {
            ElementBarre::Dossier(DossierEpingle {
                chemin: PathBuf::from(chemin),
                nom: None,
                vignette: None,
            })
        };
        let (a, b, c) = (dossier(r"C:\a"), dossier(r"C:\b"), dossier(r"C:\c"));
        let avant = vec![a.clone(), b.clone()];

        // Un dossier ÉTRANGER arrivé là, et un des anciens disparu.
        assert_eq!(permutation_des_dossiers(&avant, &[a.clone(), c.clone()]), None);
        // **LE MÊME DOSSIER DEUX FOIS** : c'est ainsi qu'un autre s'efface.
        assert_eq!(permutation_des_dossiers(&avant, &[a.clone(), a.clone()]), None);
        // Un dossier de moins.
        assert_eq!(permutation_des_dossiers(&avant, &[b.clone()]), None);
    }

    /// **LE PENDANT APPLICATIF, VÉRIFIABLE PARCE QU'IL EST GÉNÉRIQUE** — c'est lui
    /// qu'`app::EmplacementsDossiers::permuter` emploie pour les deux tableaux de
    /// vignettes, et un tableau de chaînes suffit donc à le mettre à l'épreuve.
    #[test]
    fn une_permutation_reordonne_un_tableau_parallele() {
        let noms = ["a", "b", "c"];
        // Le nouveau rang 0 était l'ancien 1, le 1 était le 2, le 2 était le 0.
        assert_eq!(reordonne_par(&noms, &[1, 2, 0]), Some(vec!["b", "c", "a"]));
        assert_eq!(reordonne_par(&noms, &[0, 1, 2]), Some(vec!["a", "b", "c"]));
        // ⛔ REFUSÉ : une longueur qui ne tombe pas juste, un rang hors bornes, un rang
        // compté deux fois. **Jamais un tableau amputé ou dupliqué** — un `Vec` plus
        // court que la plage du dock est exactement ce que le verrou d'aplomb guette.
        assert_eq!(reordonne_par(&noms, &[0, 1]), None);
        assert_eq!(reordonne_par(&noms, &[0, 1, 9]), None);
        assert_eq!(reordonne_par(&noms, &[0, 0, 2]), None);
    }

    /// **SONDE — LE VA-ET-VIENT SUR LE VRAI `dossiers.toml`, EN LECTURE SEULE.**
    /// Deux dossiers épinglés ont disparu en vingt-quatre heures — `Downloads` (2ᵉ
    /// position) puis `Videos` (5ᵉ), **toujours un dossier du MILIEU**. Entre les deux,
    /// il n'y a qu'UN écrivain (`save_barre_du_bas`) et UN lecteur (`load_barre_du_bas`) :
    /// si ce que l'un écrit ne se relit pas à l'identique, la perte est là, et elle est
    /// **reproductible sur le fichier réel** — donc mesurable sans rien attendre.
    ///
    /// **LECTURE SEULE** : elle charge et reformate EN MÉMOIRE, elle n'écrit pas une
    /// ligne. C'est la même précaution que `the_real_legacy_config_migrates_without_loss`
    /// côté zones.
    ///
    /// `cargo test --release -- --ignored --nocapture va_et_vient_du_vrai`
    #[test]
    #[ignore]
    fn va_et_vient_du_vrai_dossiers_toml() {
        let elements = load_barre_du_bas();
        let relu = barre_du_bas_from_toml(&barre_du_bas_to_toml(&elements));
        let compte = |v: &[ElementBarre]| {
            (
                v.len(),
                v.iter().filter(|e| matches!(e, ElementBarre::Dossier(_))).count(),
                v.iter().filter(|e| matches!(e, ElementBarre::Separateur(_))).count(),
            )
        };
        println!("charges : {:?} (total, dossiers, separateurs)", compte(&elements));
        println!("relus   : {:?}", compte(&relu));
        for (i, element) in elements.iter().enumerate() {
            if relu.get(i) != Some(element) {
                println!("  DIVERGENCE a {i} :\n    charge  {element:?}\n    relu    {:?}", relu.get(i));
            }
        }
        for (i, element) in relu.iter().enumerate().skip(elements.len()) {
            println!("  EN TROP a {i} : {element:?}");
        }
        assert_eq!(
            relu.len(),
            elements.len(),
            "le va-et-vient perd des lignes SUR LE FICHIER REEL — c'est la perte mesuree"
        );
        assert_eq!(relu, elements, "…et il doit rendre la MEME liste");
    }

    /// **UN SÉPARATEUR ORDINAIRE N'ÉCRIT PAS SA SORTE** — même règle que `[bars.X] sortes`
    /// dans `zones.toml`, et c'est ce qui garde le format rétrocompatible : un fichier
    /// d'avant ce champ se relit tel quel, et un fichier neuf ne grossit pas d'une ligne
    /// qui ne dit rien.
    #[test]
    fn un_separateur_rond_n_ecrit_pas_de_sorte() {
        let table = barre_du_bas_to_toml(&[ElementBarre::Separateur(SorteEspace::Rond)]);
        let ligne = table
            .get("dossiers")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_table())
            .expect("une ligne");
        assert!(ligne.get("sorte").is_none(), "« rond » est le defaut : il ne s'ecrit pas");
        assert_eq!(ligne.get("type").and_then(|v| v.as_str()), Some(TYPE_SEPARATEUR));
    }

    /// **ET UNE SORTE INCONNUE NE FAIT PAS PERDRE LA LIGNE** : elle se dit, et retombe sur
    /// `Rond`. Refuser l'entrée coûterait une PLACE dans la rangée — une sorte perdue ne
    /// coûte qu'un dessin, une ligne perdue coûte une position.
    #[test]
    fn une_sorte_inconnue_garde_la_ligne_et_retombe_sur_rond() {
        let texte = r#"
[[dossiers]]
type = "separateur"
sorte = "en-pointilles"

[[dossiers]]
chemin = 'C:\essai\dossier'
"#;
        let table = texte.parse::<toml::Table>().expect("toml lisible");
        let relu = barre_du_bas_from_toml(&table);
        assert_eq!(relu.len(), 2, "la ligne a la sorte inconnue est CONSERVEE");
        assert_eq!(
            relu[0],
            ElementBarre::Separateur(SorteEspace::Rond),
            "…et prend « rond » a la place"
        );
        assert_eq!(
            relu[1],
            ElementBarre::Dossier(DossierEpingle {
                chemin: PathBuf::from(r"C:\essai\dossier"),
                nom: None,
                vignette: None,
            })
        );
    }

    /// Un nom d'écran RÉEL, antislashs compris : le format est séparé par `|`,
    /// `,` et `;`, donc un antislash doit traverser l'aller et le retour sans
    /// être touché — c'est le cas si et seulement si personne ne l'a
    /// « échappé » en chemin, et c'est précisément ce qu'un format maison rate
    /// sans qu'on s'en aperçoive.
    const DISPLAY2: &str = r"\\.\DISPLAY2";
    const DISPLAY3: &str = r"\\.\DISPLAY3";

    /// Joint des entrées « par écran » avec le MÊME formateur que les deux
    /// sauvegardes — sans quoi le test comparerait le format à lui-même.
    fn written(pairs: &[(&str, &str)]) -> String {
        let per_screen: Vec<(String, String)> = pairs.iter().map(|(name, body)| (name.to_string(), body.to_string())).collect();
        format_secondary_screen_indexed(&per_screen)
    }

    /// **L'ALLER ET LE RETOUR DU FORMAT.** C'est la seule garantie qui compte
    /// ici : un désaccord entre `format_secondary_screen_indexed` et
    /// `parse_secondary_screen_indexed` ne se verrait PAS à la compilation, mais
    /// à la perte silencieuse des réglages d'un écran au redémarrage suivant.
    #[test]
    fn secondary_screen_indexed_round_trips() {
        let text = written(&[(DISPLAY2, "3,18;7,400"), (DISPLAY3, "12,24.5")]);
        let parsed = parse_secondary_screen_indexed(&text);

        assert_eq!(parsed.len(), 2, "un écran par ligne");
        assert_eq!(
            parsed[DISPLAY2],
            vec![(3, vec![18.0]), (7, vec![400.0])],
            "les entrées, dans l'ordre écrit, avec leurs champs"
        );
        assert_eq!(parsed[DISPLAY3], vec![(12, vec![24.5])]);
    }

    /// Une entrée illisible est écartée SEULE : les autres survivent. C'est la
    /// différence avec `load_spacer_sizes`, qui écarte la ligne ENTIÈRE — là-bas
    /// une entrée EST une ligne, ici elle n'en est qu'un morceau, et perdre tous
    /// les espaces d'un écran à cause d'un champ abîmé serait hors de
    /// proportion.
    #[test]
    fn a_malformed_entry_does_not_take_its_neighbours_down() {
        let parsed = parse_secondary_screen_indexed(r"\\.\DISPLAY2|3,18;pas-un-index,40;7,;9,30");
        assert_eq!(parsed[DISPLAY2], vec![(3, vec![18.0]), (9, vec![30.0])], "seules les entrées valides sont gardées");
    }

    /// Une ligne sans nom d'écran (vide, ou blancs seuls avant le `|`) est
    /// ignorée : elle décrirait un écran qui n'existe pas.
    #[test]
    /// ⛔ **UNE LIGNE ÉCRITE PAR UNE VERSION ANTÉRIEURE SE CHARGE ENCORE** — elle
    /// n'a que SEPT champs, ceux d'avant le 2026-10-07. La jeter ferait perdre à
    /// l'utilisateur TOUS ses réglages par écran au premier lancement de cette
    /// version.
    #[test]
    fn une_ligne_a_sept_champs_se_charge_encore() {
        let (nom, champs) = champs_depuis_ligne(r"\\.\DISPLAY2,0.9,1,1,1,1,1,1").expect("lisible");
        assert_eq!(nom, r"\\.\DISPLAY2");
        assert_eq!(champs[super::CHAMP_MENU], 0.9);
        assert_eq!(champs[super::CHAMP_REGLAGES_CONTENU], 1.0);
        // ⛔ **ET LE RATIO DE DPI VAUT « AUTOMATIQUE », PAS `1.0`.** Un défaut à
        // `1.0` aurait DÉSACTIVÉ en silence la correction du 2026-10-07 sur chaque
        // écran déjà réglé — c'est le piège que porte `RATIO_DPI_AUTO`.
        assert_eq!(champs[super::CHAMP_RATIO_DPI], super::RATIO_DPI_AUTO);
    }

    /// Et une ligne COMPLÈTE porte bien son huitième champ, le ratio.
    #[test]
    fn une_ligne_complete_lit_le_ratio() {
        let (_, champs) = champs_depuis_ligne(r"\\.\DISPLAY2,0.9,1,1,1,1,1,1,0.67").expect("lisible");
        assert!(
            (champs[super::CHAMP_RATIO_DPI] - 0.67).abs() < 1e-6,
            "ratio {}",
            champs[super::CHAMP_RATIO_DPI]
        );
    }

    /// ⚠️ **UNE VALEUR NON NUMÉRIQUE ÉCARTE LA LIGNE, ELLE NE LA DEVINE PAS.**
    #[test]
    fn une_valeur_illisible_ecarte_sa_ligne() {
        assert!(champs_depuis_ligne(r"\\.\DISPLAY2,abc").is_none());
    }

    fn lines_without_a_device_name_are_ignored() {
        let text = [r"|3,18", "", r"   |4,20", r"\\.\DISPLAY2|5,30"].join("\n");
        let parsed = parse_secondary_screen_indexed(&text);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[DISPLAY2], vec![(5, vec![30.0])]);
    }

    /// Les tailles d'espaces prennent le PREMIER champ ; une entrée sans champ
    /// est écartée plutôt que lue comme un zéro.
    #[test]
    fn spacer_sizes_take_the_first_field() {
        let sizes = spacer_sizes_from_indexed(parse_secondary_screen_indexed(r"\\.\DISPLAY2|3,18;7;9,40"));
        assert_eq!(sizes[DISPLAY2], vec![(3, 18.0), (9, 40.0)], "« 7 » n'a pas de champ : écarté");
    }

    /// Les cotes exigent les DEUX champs — COTE et VIDE DEVANT (voir
    /// `save_zone_heights`, même format : trois champs, jamais deux). Une entrée
    /// qui n'en a qu'un est écartée : la lire de travers donnerait une cote
    /// fausse, ce qui est pire que de ne rien restaurer. Et une entrée
    /// incomplète ne doit pas emporter les suivantes.
    #[test]
    fn zone_heights_want_both_fields() {
        let heights = zone_heights_from_indexed(parse_secondary_screen_indexed(r"\\.\DISPLAY2|12,400,0;20,300;30,250,15"));
        assert_eq!(
            heights[DISPLAY2],
            vec![(12, 400.0, 0.0), (30, 250.0, 15.0)],
            "« 20,300 » n'a qu'un champ après l'index : écartée, et les autres survivent"
        );
    }
}


#[cfg(test)]
mod vues_notes_tests {
    use super::*;

    #[test]
    fn les_vues_font_l_aller_retour_et_un_fichier_casse_n_en_donne_aucune() {
        let vues = vec![
            notes::VueNotes::nouvelle("Travail", "réunion \"x\"", "projets/neuradesk", true, false).unwrap(),
            notes::VueNotes::nouvelle("Archives", "", "", false, true).unwrap(),
        ];
        assert_eq!(vues_notes_depuis_toml(&vues_notes_vers_toml(&vues)), vues);
        assert!(vues_notes_depuis_toml("[[[ cassé").is_empty());
        assert!(vues_notes_depuis_toml("[[vues]]\nnom = \"  \"\n").is_empty());
    }
}

#[cfg(test)]
mod rotation_notes_tests {
    use super::*;

    fn bac_a_sable(nom: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("neuradesk_test_{nom}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("bac à sable");
        dir
    }

    /// **LA ROTATION GARDE LES TROIS DERNIÈRES VERSIONS, DANS L'ORDRE.** C'est la
    /// garantie qui rend une écriture ratée réparable : sans elle, `save_notes`
    /// réécrit le fichier ENTIER et la version d'avant n'existe plus nulle part.
    ///
    /// ⚠️ Le fichier de test vit dans un BAC À SABLE : `save_notes`/`load_notes`
    /// touchent le VRAI `%APPDATA%` de l'utilisateur, et le dépôt interdit
    /// explicitement de les faire passer par un test (voir la doc de
    /// `lire_alarme_note`). Ici on ne teste que la rotation, qui prend un chemin.
    #[test]
    fn la_rotation_decale_les_versions_et_garde_les_trois_dernieres() {
        let dir = bac_a_sable("rotation");
        let fichier = dir.join("notes.toml");
        let version = |n: usize| dir.join(format!("notes.toml.{n}"));

        // Rien à décaler quand le fichier n'existe pas : la rotation ne doit pas
        // créer de `.1` fantôme au premier lancement.
        rotate_data_file(&fichier, ROTATIONS_NOTES);
        assert!(!version(1).exists(), "aucun fichier ⇒ aucune rotation");

        // Et rien non plus quand il est VIDE : un fichier de zéro octet n'a rien
        // à sauver, et le décaler créerait des `.1`, `.2` vides à la chaîne.
        fs::write(&fichier, "").unwrap();
        rotate_data_file(&fichier, ROTATIONS_NOTES);
        assert!(!version(1).exists(), "fichier vide ⇒ aucune rotation");

        for attendu in ["v1", "v2", "v3", "v4", "v5"] {
            fs::write(&fichier, attendu).unwrap();
            rotate_data_file(&fichier, ROTATIONS_NOTES);
        }
        assert!(!fichier.exists(), "le fichier courant a été décalé");
        assert_eq!(fs::read_to_string(version(1)).unwrap(), "v5", "la plus récente d'abord");
        assert_eq!(fs::read_to_string(version(2)).unwrap(), "v4");
        assert_eq!(fs::read_to_string(version(3)).unwrap(), "v3");
        assert!(!version(4).exists(), "au-delà de trois rotations, on jette la plus ancienne");

        let _ = fs::remove_dir_all(&dir);
    }

    /// **AUCUNE VIGNETTE VIDE N'EST RANGÉE, ET AUCUN `%APPDATA%` N'EST TOUCHÉ POUR RIEN.**
    ///
    /// ⛔ **CE TEST EXISTE PARCE QUE LA GARDE ÉTAIT AU MAUVAIS ENDROIT.** Elle était posée
    /// APRÈS `make_thumbnail`, en croyant que celui-ci rendrait `(0, 0, &[])` — or il
    /// relève d'abord les deux côtés à 1 (`max(1)`, voir sa doc), donc un 0×0 devenait une
    /// vignette **1×1 transparente** : bien formée, rangée sous un nom valide, et ne
    /// dessinant rien. Aucun contrôle ne l'aurait signalée. La garde est maintenant AVANT,
    /// et les trois cas ci-dessous la tiennent.
    ///
    /// ⚠️ **LE TEST VÉRIFIE AUSSI L'ABSENCE D'EFFET DE BORD** : le refus a lieu avant
    /// `dossiers_images_dir()`, donc rien n'est créé sur le disque — un `Some` ici voudrait
    /// dire qu'on a écrit dans `%APPDATA%` une vignette de rien.
    #[test]
    fn une_vignette_vide_ou_tronquee_n_est_pas_rangee() {
        assert_eq!(super::store_dossier_vignette_from_rgba(0, 0, &[]), None, "un 0×0 est refuse");
        assert_eq!(super::store_dossier_vignette_from_rgba(4, 4, &[]), None, "un tampon vide est refuse");
        assert_eq!(
            super::store_dossier_vignette_from_rgba(4, 4, &[0u8; 16]),
            None,
            "un tampon tronque (16 octets pour 4×4×4 = 64) est refuse"
        );
    }

    // ---- LA SORTE D'UN ESPACE, DANS `zones.toml` ---------------------------

    /// **L'ALLER-RETOUR DE LA SORTE, ET LA RÉTROCOMPATIBILITÉ DU FICHIER.**
    ///
    /// Deux choses d'un coup, et les deux comptent :
    /// - une sorte qui n'est pas le défaut **survit** à l'écriture puis à la relecture ;
    /// - un `zones.toml` **SANS `sorte`** — c'est-à-dire tous ceux d'avant cette
    ///   fonctionnalité — se relit comme `Rond`, sans migration et sans message.
    ///
    /// ⚠️ **Le défaut ne s'écrit PAS** (voir `zone_record_to_table`) : c'est ce qui
    /// garantit qu'un fichier existant ne bouge pas d'un octet tant que l'utilisateur
    /// n'a rien changé.
    /// ÉTAPE 2 du plan `docs/plans/disposition-barres-zones-icones.md` : une zone
    /// porte la LISTE de ses icônes, par identité — aller-retour, et un fichier qui
    /// n'en a pas n'en gagne pas.
    #[test]
    fn les_icones_d_une_zone_font_l_aller_retour_et_le_vide_ne_s_ecrit_pas() {
        use crate::ui::zones::{BarLetter, ZoneKey, ZoneRecord, Zones};

        let cle = ZoneKey::new(BarLetter::Droite, 1).expect("un rang >= 1");
        let liste = vec!["noyau:alimentation".to_string(), "epingle:C:\\Program Files\\x.exe".to_string()];
        let mut zones = Zones::default();
        zones
            .zones
            .insert(cle, ZoneRecord { cote: 930.0, icones: liste.clone(), ..ZoneRecord::default() });

        let table = super::zones_to_toml(&zones);
        let ecrit = table.to_string();
        assert!(ecrit.contains("noyau:alimentation"), "la liste doit etre ECRITE : {ecrit}");

        let (relu, refuses) = super::zones_from_toml(&table);
        assert!(refuses.is_empty(), "rien a refuser : {refuses:?}");
        assert_eq!(relu.zones.get(&cle).map(|r| r.icones.clone()), Some(liste), "la liste doit revenir par la cle");

        // ⛔ Une zone SANS liste : le fichier ne gagne PAS de ligne `icones` — c'est
        // la garantie qui garde intacts tous les `zones.toml` d'avant l'étape 2.
        let mut vide = Zones::default();
        vide.zones.insert(cle, ZoneRecord { cote: 930.0, ..ZoneRecord::default() });
        let ecrit_vide = super::zones_to_toml(&vide).to_string();
        assert!(!ecrit_vide.contains("icones"), "un fichier sans liste ne doit pas en gagner : {ecrit_vide}");
    }

    #[test]
    fn une_sorte_d_espace_fait_l_aller_retour_et_le_defaut_ne_s_ecrit_pas() {
        use crate::ui::zones::{BarLetter, BarSpacers, SorteEspace, ZoneKey, Zones};

        let cle = ZoneKey::new(BarLetter::Bas, 1).expect("un rang >= 1");
        let mut zones = Zones::default();
        // ⚠️ **LA SORTE VIT DANS `[bars.X]`, PAS DANS `[zones.X]` — C'EST UNE MESURE
        // QUI L'A DÉCIDÉ.** `Dock::zones()` ne construit un `ZoneRecord` que pour un
        // espace qui **déclare une zone** (cote > 0), et un enregistrement sans cote
        // est refusé à la relecture. Un séparateur ORDINAIRE n'a donc aucun
        // enregistrement : mesuré sur la configuration de la machine, la sorte
        // choisie n'était jamais écrite et le bouton semblait ne rien faire.
        //
        // `[bars.X]` est écrit pour TOUTE barre qui a des espaces, zone ou pas.
        zones.bars.insert(
            BarLetter::Bas,
            BarSpacers { spacers: 1, sizes: vec![18.0], sortes: vec![SorteEspace::Elastique], ..BarSpacers::default() },
        );

        let table = super::zones_to_toml(&zones);
        let ecrit = table.to_string();
        assert!(ecrit.contains("elastique"), "la sorte doit etre ECRITE : {ecrit}");

        let (relu, refuses) = super::zones_from_toml(&table);
        assert!(refuses.is_empty(), "rien a refuser : {refuses:?}");
        assert_eq!(relu.sorte_at(cle), Some(SorteEspace::Elastique), "la sorte doit revenir par la cle");

        // UNE BARRE SANS `sortes` — celle de tous les fichiers d'avant, et celle de
        // toute barre dont les espaces sont ordinaires (on n'écrit pas une liste de
        // « rond »).
        let mut sans_sortes = table.clone();
        if let Some(barre) = sans_sortes.get_mut("bars").and_then(|b| b.as_table_mut()).and_then(|b| b.get_mut("B")).and_then(|r| r.as_table_mut()) {
            barre.remove("sortes");
        }
        let (relu, refuses) = super::zones_from_toml(&sans_sortes);
        assert!(refuses.is_empty(), "une barre sans `sortes` ne doit RIEN refuser : {refuses:?}");
        assert_eq!(
            relu.sorte_at(cle),
            None,
            "sans `sortes`, aucun rang ne porte de sorte — l'appelant retombe sur `Rond`, \
             et c'est ce qui rend le fichier retrocompatible"
        );
    }

    /// ⛔ **UNE COTE RONDE NE DOIT PAS FAIRE DISPARAÎTRE LA ZONE.**
    ///
    /// TOML distingue l'ENTIER du FLOTTANT, et `as_float()` **ne convertit pas** :
    /// `cote = 497` rendait `None`, la zone était `continue`-ée, et le refus
    /// s'appelait même « cote absente » — **un mensonge, puisque la cote était
    /// là**. Le déclencheur est banal (une cote tombant sur un nombre rond) et la
    /// perte était SILENCIEUSE.
    ///
    /// C'est le même piège que la coordonnée ronde d'un lieu, trouvé le même jour
    /// — voir `flottant_toml`, qui est désormais la définition unique du motif.
    #[test]
    fn une_cote_ronde_ne_fait_pas_disparaitre_la_zone() {
        use crate::ui::zones::{BarLetter, ZoneKey};

        // ⚠️ **LA CLÉ D'UNE ZONE EST « LETTRE + RANG »** (`D1`), jamais un nom
        // lisible — c'est `ZoneKey` qui la relit, et un nom libre est refusé AVANT
        // qu'on arrive à la cote.
        let mut zones = toml::Table::new();
        for (cle, cote) in [
            ("D1", toml::Value::Integer(497)),
            ("D2", toml::Value::Float(187.5)),
        ] {
            let mut zone = toml::Table::new();
            zone.insert("cote".to_string(), cote);
            zone.insert("name".to_string(), toml::Value::String(cle.to_string()));
            zones.insert(cle.to_string(), toml::Value::Table(zone));
        }
        // ⚠️ **LA BARRE DOIT EXISTER**, et compter assez d'espaces pour porter les
        // deux rangs — sans elle, le refus « barre D absente » masquerait celui
        // qu'on veut éprouver.
        let mut barre = toml::Table::new();
        barre.insert("spacers".to_string(), toml::Value::Integer(2));
        let mut bars = toml::Table::new();
        bars.insert("D".to_string(), toml::Value::Table(barre));

        let mut table = toml::Table::new();
        table.insert("bars".to_string(), toml::Value::Table(bars));
        table.insert("zones".to_string(), toml::Value::Table(zones));

        let (relu, refuses) = super::zones_from_toml(&table);
        // Avec `as_float()` seul, la zone « ronde » arrivait ici en « cote absente ».
        assert!(refuses.is_empty(), "aucune cote n'est absente : {refuses:?}");

        let cote = |rang| {
            let cle = ZoneKey::new(BarLetter::Droite, rang).unwrap();
            relu.record(cle).map(|r| r.cote)
        };
        assert_eq!(cote(1), Some(497.0), "une cote ÉCRITE EN ENTIER doit se lire");
        assert_eq!(cote(2), Some(187.5), "et une cote décimale reste décimale");
    }

    /// **UNE SORTE INCONNUE SE REFUSE EN LE DISANT, ET ELLE EMPORTE LA LISTE
    /// ENTIÈRE** — même règle que `sizes` : une liste dont la longueur ne vaut plus
    /// le compte est ignorée EN ENTIER, parce qu'une entrée déplacée changerait la
    /// sorte de tous les espaces suivants.
    ///
    /// Refuser plutôt que de retomber en silence sur « tout est rond » : une sorte
    /// perdue changerait le DESSIN de la barre sans que rien ne le dise.
    #[test]
    fn une_sorte_inconnue_fait_refuser_toute_la_liste() {
        use crate::ui::zones::{BarLetter, SorteEspace, ZoneKey};

        let mut table = toml::Table::new();
        let mut bars = toml::Table::new();
        let mut barre = toml::Table::new();
        barre.insert("spacers".to_string(), toml::Value::Integer(2));
        barre.insert(
            "sortes".to_string(),
            toml::Value::Array(vec![
                toml::Value::String("trait".to_string()),
                toml::Value::String("trait-vertical".to_string()),
            ]),
        );
        bars.insert("B".to_string(), toml::Value::Table(barre));
        table.insert("bars".to_string(), toml::Value::Table(bars));

        let (relu, _) = super::zones_from_toml(&table);
        let premiere = ZoneKey::new(BarLetter::Bas, 1).expect("un rang >= 1");
        assert_eq!(
            relu.sorte_at(premiere),
            None,
            "UNE SEULE cle illisible fait tomber TOUTE la liste — sinon le rang de la \
             premiere serait conserve et celui de la seconde perdu, ce qui deplace les rangs"
        );
    }
}

/// **LES RÉGLAGES DU DASH DOSSIER À DEUX MOITIÉS** — les deux tris, le dossier
/// personnel et le souvenir de gauche.
///
/// ⚠️ **AUCUN DE CES TESTS NE TOUCHE AU VRAI FICHIER DE L'UTILISATEUR.** Ils
/// éprouvent les cœurs PURS (`parse_sous_dock_tris`, `format_sous_dock_tris`,
/// `chemin_retenu`), qui existent exactement pour ça — même discipline que
/// `barre_du_bas_to_toml`/`from_toml`, dont les « cœurs purs » sont là « exprès
/// pour éviter de toucher au vrai fichier en test ».
#[cfg(test)]
mod tests_dash_dossier {
    use super::*;
    use crate::win32::sous_dock::{Cote, Tri};

    /// **LE VA-ET-VIENT DES DEUX TRIS, SANS PERTE ET DANS L'ORDRE.**
    ///
    /// ⛔ Le défaut refusé est celui que le dépôt a déjà payé DEUX fois : un
    /// écrivain et un lecteur qui divergent, donc un réglage qui repart au défaut
    /// au lancement suivant sans que personne ne s'en aperçoive. Ici on exige les
    /// QUATRE combinaisons utiles, dont celle qui compte le plus : **deux tris
    /// DIFFÉRENTS**, sans quoi un écrivain qui ne saurait écrire qu'une seule
    /// valeur passerait le test.
    #[test]
    fn les_deux_tris_du_dash_dossier_font_un_va_et_vient() {
        let cas = [
            [Tri::Nom, Tri::Nom],
            [Tri::Date, Tri::Taille],
            [Tri::Type, Tri::Nom],
            [Tri::Taille, Tri::Type],
        ];
        for tris in cas {
            let table = format_sous_dock_tris(tris);
            let relu = parse_sous_dock_tris(&table);
            assert_eq!(relu, tris, "le va-et-vient a perdu un tri : {table:?}");
        }
    }

    /// ⛔ **L'ANCIENNE FORME (UNE SEULE LIGNE, SANS `=`) RESTE LUE**, et vaut
    /// pour LES DEUX moitiés. C'est le fichier qu'ont écrit toutes les versions
    /// précédentes ; le refuser ferait repartir à « nom » un tri que
    /// l'utilisateur avait choisi — exactement le défaut que la persistance du
    /// tri avait été écrite pour éviter.
    #[test]
    fn un_fichier_de_tri_a_une_seule_ligne_vaut_pour_les_deux_moities() {
        assert_eq!(parse_sous_dock_tris("date"), [Tri::Date, Tri::Date]);
        assert_eq!(parse_sous_dock_tris("date\n"), [Tri::Date, Tri::Date]);
    }

    /// ⛔ **UNE CLÉ ILLISIBLE NE RANGE RIEN — ET N'EMMÈNE PAS LES AUTRES.** Une
    /// ligne abîmée (version future, fichier retouché à la main) doit laisser le
    /// tri de l'AUTRE moitié intact : un lecteur qui jette tout le fichier au
    /// premier mot inconnu ferait perdre deux réglages pour une faute de frappe.
    #[test]
    fn une_cle_illisible_ne_range_rien_et_n_emporte_pas_l_autre_moitie() {
        let relu = parse_sous_dock_tris("g=taille\nd=inventee");
        assert_eq!(relu[Cote::Gauche.index()], Tri::Taille, "la ligne lisible doit survivre");
        assert_eq!(relu[Cote::Droite.index()], Tri::Nom, "la ligne illisible retombe au defaut");

        // Une clé de CÔTÉ inconnue est ignorée de la même façon.
        let relu = parse_sous_dock_tris("x=date\ng=type");
        assert_eq!(relu[Cote::Gauche.index()], Tri::Type);
        assert_eq!(relu[Cote::Droite.index()], Tri::Nom);
    }

    /// **UN FICHIER VIDE OU ABSENT REND DEUX FOIS LE TRI PAR DÉFAUT** — jamais
    /// une panique, jamais un tri au hasard.
    #[test]
    fn un_fichier_de_tri_vide_rend_le_defaut_des_deux_cotes() {
        assert_eq!(parse_sous_dock_tris(""), [Tri::default(); 2]);
        assert_eq!(parse_sous_dock_tris("\n\n  \n"), [Tri::default(); 2]);
    }

    /// **LE CHEMIN RETENU, ET LES TROIS FAÇONS DE LE REFUSER.**
    ///
    /// ⛔ Ce test refuse trois valeurs qui produiraient toutes le MÊME symptôme —
    /// une moitié vide dont personne ne saurait dire pourquoi : un chemin
    /// relatif, un chemin qui n'existe pas, et un chemin qui existe mais n'est
    /// pas un dossier.
    #[test]
    fn un_chemin_de_reglage_est_retenu_seulement_s_il_est_absolu_et_existe() {
        let bac = std::env::temp_dir();
        let bac_texte = bac.display().to_string();

        assert_eq!(chemin_retenu(Some(&bac_texte)), Some(bac.clone()));
        // Les espaces autour sont retirés : un fichier écrit à la main en pose.
        assert_eq!(chemin_retenu(Some(&format!("  {bac_texte}\n"))), Some(bac.clone()));

        assert_eq!(chemin_retenu(None), None, "pas de fichier : rien de retenu");
        assert_eq!(chemin_retenu(Some("   ")), None, "un fichier vide ne retient rien");
        assert_eq!(
            chemin_retenu(Some(r"dossier\relatif")),
            None,
            "un chemin RELATIF serait lu depuis le repertoire de travail du processus"
        );
        assert_eq!(
            chemin_retenu(Some(r"C:\ce\chemin\n\existe\pas\du\tout")),
            None,
            "un chemin disparu laisserait la moitie sur une liste vide"
        );

        // Un FICHIER (pas un dossier) est refusé lui aussi.
        let fichier = bac.join("nd-chemin-retenu-fichier.txt");
        std::fs::write(&fichier, b"x").expect("ecriture");
        assert_eq!(
            chemin_retenu(Some(&fichier.display().to_string())),
            None,
            "un FICHIER n'est pas un dossier : le panneau ne pourrait rien y lister"
        );
        let _ = std::fs::remove_file(&fichier);
    }
}
