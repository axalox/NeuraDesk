//! ⭐ **EXPORTER ET IMPORTER LA CONFIGURATION** — une archive `.zip` du dossier
//! `%APPDATA%\NeuraDesk`.
//!
//! DEMANDE (2026-10-10) : « Configuration export and import ». Le dossier porte
//! plus de 200 fichiers ; le sauvegarder ou le porter sur une autre machine
//! demandait de le copier à la main.
//!
//! **CE QUI PART DANS L'ARCHIVE** (`est_exporte`) : les fichiers du dossier, et
//! ceux des sous-dossiers de `SOUS_DOSSIERS` (images des notes, vignettes des
//! dossiers, palettes, textures). **CE QUI N'Y PART PAS** :
//! - l'historique du presse-papiers — MESURÉ le 2026-10-10 : 470 Mo sur 475, et
//!   ce sont des données privées, pas des réglages ;
//! - les copies de sécurité (`*.avant-…`, `notes.toml.1`…) et les dossiers
//!   d'avant une réparation ou un import ;
//! - l'historique de l'applet Surveillance, qui se remesure.
//!
//! ⛔ **L'IMPORT NE S'APPLIQUE JAMAIS PENDANT QUE L'APPLICATION TOURNE.** Elle
//! réécrit ses fichiers en continu (ordres, arrangement, à la fermeture aussi) :
//! des fichiers posés sous elle seraient écrasés par l'état qu'elle tient en
//! mémoire. L'import se fait donc en TROIS temps :
//! 1. `preparer_import` vérifie l'archive et l'extrait dans `EN_ATTENTE` — rien
//!    d'autre n'est touché ;
//! 2. `confirmer_import` y pose `PRET` — la page a montré ce qui allait être
//!    importé, et l'utilisateur a dit oui ;
//! 3. `appliquer_import_en_attente`, APRÈS la fin d'`app::run` (le
//!    rechargement) ou au lancement suivant, range la configuration actuelle
//!    dans `avant-import-<date>` et met l'importée à sa place.
//!
//! Sans `PRET`, une extraction n'est jamais appliquée : annuler, ou fermer les
//! Réglages entre les deux, ne change rien.
//!
//! ⭐ **DEUX SORTES D'ARCHIVE** (`Sorte`). DEMANDE (2026-10-24) : « sauvegarder
//! et importer mes réglages d'interface » — l'export COMPLET emportait aussi les
//! notes, le calendrier, les zones… et son import les écrasait. L'export
//! `Sorte::Apparence` ne porte que ce que `est_apparence` retient (couleurs,
//! thème, style, icônes, tailles, ombres, lueurs, textures, dégradé, flou,
//! effets) ; son import ne remplace QUE ces fichiers et laisse tout le reste.
//! La sorte est écrite dans le manifeste, et l'import la relit : une archive
//! « apparence » qui porterait autre chose est refusée entière.
//!
//! ⚠️ **LES MOTS DE PASSE SSH NE VOYAGENT PAS** : ils sont chiffrés par DPAPI
//! pour la session Windows (voir `win32::ssh_serveurs`). Sur une autre session,
//! les serveurs reviennent, leurs mots de passe sont à ressaisir.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Les sous-dossiers qui partent dans l'archive — tous les autres restent.
///
/// `lueurs`, `lueurs_images` et `shaders` n'y étaient pas avant l'export de
/// l'apparence : les lueurs enregistrées, leurs images et les shaders de
/// l'utilisateur ne partaient même pas dans l'export complet.
const SOUS_DOSSIERS: &[&str] =
    &["dossiers_images", "notes_images", "palettes", "textures", "lueurs", "lueurs_images", "shaders"];

/// Ceux de `SOUS_DOSSIERS` qui sont de l'APPARENCE — pas les images des notes ni
/// les vignettes des dossiers, qui vont avec leurs données.
pub(crate) const SOUS_DOSSIERS_APPARENCE: &[&str] = &["palettes", "textures", "lueurs", "lueurs_images", "shaders"];

/// Les fichiers d'apparence nommés un par un — les autres sont reconnus par
/// `PREFIXES_APPARENCE` et `SUFFIXES_APPARENCE`. Relevé sur le dossier réel le
/// 2026-10-24.
const NOMS_APPARENCE: &[&str] = &[
    "style.txt",
    "color.txt",
    "custom_colors.txt",
    "custom_themes.txt",
    "auto_color_scheme.txt",
    "icones.txt",
    "police.txt",
    "bar_colors.txt",
    "bar_thicknesses.txt",
    "taskbar_size.txt",
    "dither_preset.txt",
    "indicateur_couleur.txt",
    "moniteur_couleurs.txt",
    "coins_secondaires.txt",
    "desktop_dot_size.txt",
    "desktop_preview_width.txt",
    "preview_thumb_width.txt",
    "hover_ease_speed.txt",
    "tirage_magnification.txt",
    "dossier_zoom.txt",
    "zone_cell_margin.txt",
    "zone_label_font.txt",
    "secondary_screen_colors.txt",
    "secondary_screen_auto_color.txt",
    "secondary_screen_thicknesses.txt",
    "secondary_screen_scales.txt",
    "secondary_screen_separator_colors.txt",
    "secondary_screen_separator_sizes.txt",
    "secondary_screen_topbar_text_sizes.txt",
    "secondary_screen_desktop_dot_sizes.txt",
    "secondary_screen_icon_gaps.txt",
    // L'ecran sur lequel l'apparence a ete reglee (voir `config_initiale`) :
    // c'est lui qui permet a l'import de l'adapter a un autre ecran.
    crate::config_initiale::REFERENCE,
];

/// `theme_famille.txt`, `degrade_sauvegardes.toml`, `ombre_couleur.txt`,
/// `lueurs_effet.txt`, `logo_turn_ms.txt`…
const PREFIXES_APPARENCE: &[&str] = &[
    "theme_", "degrade_", "ombre_", "lueur", "blur_", "logo_", "effet_", "running_dot_", "separator_", "topbar_text_",
    "texture_", "coins_", "popup_", "fondu_", "icon_gap", "icon_margin",
];

/// `dash_accent_color.txt`, `top_icon_scale.txt`, `menu_tint.txt`,
/// `tooltip_fade_ms.txt`…
const SUFFIXES_APPARENCE: &[&str] = &["_accent_color.txt", "_scale.txt", "_tint.txt", "_fade_ms.txt"];

/// **LA SORTE D'UNE ARCHIVE** — voir la doc de module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sorte {
    /// Toute la configuration : un import la REMPLACE entière.
    Complete,
    /// L'apparence seule : un import ne remplace que les fichiers d'apparence.
    Apparence,
}

impl Sorte {
    /// Ce qui part dans une archive de cette sorte — et ce qu'un import remplace.
    fn garde(self) -> fn(&str) -> bool {
        match self {
            Sorte::Complete => est_exporte,
            Sorte::Apparence => est_apparence,
        }
    }

    /// La sorte écrite dans le manifeste — `Complete` s'il n'en dit rien, comme
    /// les archives d'avant l'export de l'apparence.
    fn du_manifeste(manifeste: &str) -> Sorte {
        let sorte = manifeste.parse::<toml::Table>().ok().and_then(|t| t.get("sorte")?.as_str().map(str::to_string));
        if sorte.as_deref() == Some("apparence") {
            Sorte::Apparence
        } else {
            Sorte::Complete
        }
    }
}

/// Les fichiers du dossier racine qui ne partent pas — voir la doc de module.
const EXCLUS: &[&str] = &["clipboard_history.toml"];

/// Le fichier qui signe une archive de NeuraDesk. Un import sans lui est refusé :
/// c'est ce qui empêche de vider la configuration en choisissant un autre .zip.
pub const MANIFESTE: &str = "neuradesk-export.toml";

/// Le dossier où l'archive est extraite en attendant d'être appliquée.
const EN_ATTENTE: &str = "import-en-attente";

/// Posé dans `EN_ATTENTE` quand l'utilisateur a confirmé.
const PRET: &str = ".pret";

/// Le préfixe du dossier où la configuration remplacée est rangée.
const PREFIXE_AVANT_IMPORT: &str = "avant-import-";

/// Au-delà, l'archive est refusée — la configuration réelle pèse ~5 Mo, et une
/// archive qui se décompresse en plus de ce plafond n'en est pas une.
const TAILLE_MAX: u64 = 512 * 1024 * 1024;

/// **CE CHEMIN RELATIF PART-IL DANS L'ARCHIVE ?** — `relatif` en `/`, tel qu'une
/// entrée d'archive l'écrit. Fonction PURE : c'est elle qui décide pour l'export
/// ET qui valide l'import, pour qu'une archive ne puisse poser que ce qu'un
/// export aurait posé.
pub fn est_exporte(relatif: &str) -> bool {
    let morceaux: Vec<&str> = relatif.split('/').collect();
    let nom = match morceaux.as_slice() {
        [nom] => {
            if EXCLUS.contains(nom) || *nom == MANIFESTE {
                return false;
            }
            *nom
        }
        [dossier, nom] if SOUS_DOSSIERS.contains(dossier) => *nom,
        _ => return false,
    };
    // `.zip` : une archive enregistrée DANS le dossier ne se ré-exporte pas.
    nom_valide(nom) && !est_une_copie_de_securite(nom) && !nom.ends_with(".zip") && !nom.ends_with(".partiel")
}

/// Un nom de fichier simple : ni vide, ni `.`/`..`, ni séparateur, ni lecteur.
/// **CE CHEMIN RELATIF EST-IL DE L'APPARENCE ?** — toujours un sous-ensemble de
/// `est_exporte`. ⛔ Les listes `*_integres.txt` disent quelles applets sont dans
/// les barres : de l'arrangement, pas de l'apparence (`lueurs_integres.txt`
/// commence pourtant par `lueur`).
pub fn est_apparence(relatif: &str) -> bool {
    if !est_exporte(relatif) {
        return false;
    }
    match relatif.split_once('/') {
        Some((dossier, _)) => SOUS_DOSSIERS_APPARENCE.contains(&dossier),
        None => {
            !relatif.ends_with("_integres.txt")
                && (NOMS_APPARENCE.contains(&relatif)
                    || PREFIXES_APPARENCE.iter().any(|p| relatif.starts_with(p))
                    || SUFFIXES_APPARENCE.iter().any(|s| relatif.ends_with(s)))
        }
    }
}

fn nom_valide(nom: &str) -> bool {
    !nom.is_empty() && nom != "." && nom != ".." && !nom.contains(['\\', ':', '\0'])
}

/// `icon_order.txt.avant-reserve-epingles`, `notes.toml.2`, `neuradesk.exe.ecarte-…`.
fn est_une_copie_de_securite(nom: &str) -> bool {
    if nom.contains(".avant") || nom.contains(".ecarte") {
        return true;
    }
    match nom.rsplit_once('.') {
        Some((tete, queue)) => tete.contains('.') && !queue.is_empty() && queue.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

/// Les chemins relatifs de `dossier` que `garde` retient, triés.
fn lister(dossier: &Path, garde: fn(&str) -> bool) -> io::Result<Vec<String>> {
    let mut chemins = Vec::new();
    for entree in fs::read_dir(dossier)? {
        let entree = entree?;
        let nom = entree.file_name().to_string_lossy().into_owned();
        let sorte = entree.file_type()?;
        if sorte.is_file() && garde(&nom) {
            chemins.push(nom);
        } else if sorte.is_dir() && SOUS_DOSSIERS.contains(&nom.as_str()) {
            for sous in fs::read_dir(entree.path())? {
                let sous = sous?;
                let relatif = format!("{nom}/{}", sous.file_name().to_string_lossy());
                if sous.file_type()?.is_file() && garde(&relatif) {
                    chemins.push(relatif);
                }
            }
        }
    }
    chemins.sort();
    Ok(chemins)
}

fn vers_disque(dossier: &Path, relatif: &str) -> PathBuf {
    relatif.split('/').fold(dossier.to_path_buf(), |chemin, morceau| chemin.join(morceau))
}

/// **ÉCRIT L'ARCHIVE DE `dossier` DANS `archive`** — rend le nombre de fichiers.
/// `sorte` choisit ce qui part, et s'écrit dans le manifeste.
///
/// Écrite à côté puis RENOMMÉE : une archive à moitié écrite (disque plein,
/// fichier verrouillé) ne remplace jamais une bonne archive du même nom.
pub fn exporter(dossier: &Path, archive: &Path, date: &str, sorte: Sorte) -> io::Result<usize> {
    let chemins = lister(dossier, sorte.garde())?;
    let provisoire = archive.with_extension("zip.partiel");
    let resultat = (|| {
        let mut zip = zip::ZipWriter::new(fs::File::create(&provisoire)?);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file(MANIFESTE, options)?;
        write!(zip, "version = 1\ndate = \"{date}\"\nfichiers = {}\n", chemins.len())?;
        if sorte == Sorte::Apparence {
            writeln!(zip, "sorte = \"apparence\"")?;
        }
        for relatif in &chemins {
            zip.start_file(relatif.as_str(), options)?;
            zip.write_all(&fs::read(vers_disque(dossier, relatif))?)?;
        }
        zip.finish()?;
        Ok::<_, io::Error>(())
    })();
    if let Err(err) = resultat {
        let _ = fs::remove_file(&provisoire);
        return Err(err);
    }
    fs::rename(&provisoire, archive)?;
    Ok(chemins.len())
}

/// Ce que la page montre avant que l'utilisateur confirme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Apercu {
    pub fichiers: usize,
    /// La date écrite par l'export (`""` si le manifeste n'en porte pas).
    pub date: String,
    pub sorte: Sorte,
}

/// **VÉRIFIE `archive` ET L'EXTRAIT DANS `EN_ATTENTE`** — rien d'autre n'est
/// touché, voir la doc de module.
///
/// ⛔ **TOUT OU RIEN** : une seule entrée hors de ce qu'un export aurait écrit
/// (un chemin absolu, un `..`, un dossier inconnu) refuse l'archive entière, et
/// l'extraction partielle est effacée.
pub fn preparer_import(archive: &Path, dossier: &Path) -> Result<Apercu, String> {
    let attente = dossier.join(EN_ATTENTE);
    let _ = fs::remove_dir_all(&attente);
    let resultat = extraire(archive, &attente);
    if resultat.is_err() {
        let _ = fs::remove_dir_all(&attente);
    }
    resultat
}

fn extraire(archive: &Path, attente: &Path) -> Result<Apercu, String> {
    let fichier = fs::File::open(archive).map_err(|e| format!("archive illisible : {e}"))?;
    let mut zip = zip::ZipArchive::new(fichier).map_err(|_| "ce fichier n'est pas une archive .zip".to_string())?;
    let mut manifeste = None;
    let mut total = 0u64;
    let mut fichiers = 0;
    let mut noms = Vec::new();
    for i in 0..zip.len() {
        let mut entree = zip.by_index(i).map_err(|e| format!("archive abîmée : {e}"))?;
        if entree.is_dir() {
            continue;
        }
        let nom = entree.name().map_err(|e| format!("archive abîmée : {e}"))?.into_owned();
        total += entree.size();
        if total > TAILLE_MAX {
            return Err("archive trop grande pour une configuration".to_string());
        }
        let mut contenu = Vec::new();
        entree.by_ref().take(TAILLE_MAX).read_to_end(&mut contenu).map_err(|e| format!("archive abîmée : {e}"))?;
        if nom == MANIFESTE {
            manifeste = Some(String::from_utf8_lossy(&contenu).into_owned());
            continue;
        }
        if !est_exporte(&nom) {
            return Err(format!("entrée refusée dans l'archive : {nom}"));
        }
        let cible = vers_disque(attente, &nom);
        if let Some(parent) = cible.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("extraction impossible : {e}"))?;
        }
        fs::write(&cible, contenu).map_err(|e| format!("extraction impossible : {e}"))?;
        fichiers += 1;
        noms.push(nom);
    }
    let manifeste = manifeste.ok_or("ce n'est pas une configuration exportée par NeuraDesk")?;
    let sorte = Sorte::du_manifeste(&manifeste);
    // ⛔ Une archive « apparence » ne remplace que l'apparence : elle ne doit
    // rien porter d'autre — sinon `appliquer` le poserait sans ranger l'actuel.
    if let Some(nom) = noms.iter().find(|n| !sorte.garde()(n)) {
        return Err(format!("entrée refusée dans une archive d'apparence : {nom}"));
    }
    // Gardé à part : `appliquer` y relit la sorte. `lister` ne le voit jamais.
    fs::write(attente.join(MANIFESTE), &manifeste).map_err(|e| format!("extraction impossible : {e}"))?;
    let date = manifeste
        .parse::<toml::Table>()
        .ok()
        .and_then(|t| t.get("date").and_then(|d| d.as_str()).map(str::to_string))
        .unwrap_or_default();
    if fichiers == 0 {
        return Err("l'archive ne contient aucun réglage".to_string());
    }
    Ok(Apercu { fichiers, date, sorte })
}

/// **L'UTILISATEUR A CONFIRMÉ** — l'extraction sera appliquée au redémarrage.
pub fn confirmer_import(dossier: &Path) -> io::Result<()> {
    fs::write(dossier.join(EN_ATTENTE).join(PRET), b"")
}

/// **ANNULE UN IMPORT PRÉPARÉ** — l'extraction est effacée, rien n'a bougé.
pub fn annuler_import(dossier: &Path) {
    let _ = fs::remove_dir_all(dossier.join(EN_ATTENTE));
}

/// **APPLIQUE L'IMPORT CONFIRMÉ, S'IL Y EN A UN** — `None` s'il n'y a rien à
/// faire, sinon le nombre de fichiers posés.
///
/// ⛔ **À N'APPELER QUE QUAND L'APPLICATION NE TOURNE PAS** (voir la doc de
/// module) : après `app::run`, ou au lancement avant toute lecture.
///
/// Une archive `Sorte::Apparence` ne range et ne pose QUE les fichiers
/// d'apparence ; le reste de la configuration ne bouge pas.
///
/// La configuration actuelle est RANGÉE (renommée) dans
/// `avant-import-<horodatage>`, pas effacée ; et si un renommage échoue en
/// route, tout ce qui a été déplacé revient à sa place — on ne laisse jamais un
/// mélange des deux configurations.
pub fn appliquer_import_en_attente(dossier: &Path, horodatage: &str) -> Option<Result<usize, String>> {
    let attente = dossier.join(EN_ATTENTE);
    if !attente.join(PRET).is_file() {
        return None;
    }
    Some(appliquer(dossier, &attente, &dossier.join(format!("{PREFIXE_AVANT_IMPORT}{horodatage}"))))
}

fn appliquer(dossier: &Path, attente: &Path, rangement: &Path) -> Result<usize, String> {
    let sorte = fs::read_to_string(attente.join(MANIFESTE)).map_or(Sorte::Complete, |m| Sorte::du_manifeste(&m));
    let mut importes = lister(attente, sorte.garde()).map_err(|e| format!("extraction illisible : {e}"))?;
    // La marque de confirmation n'est pas un réglage.
    importes.retain(|r| r != PRET);
    let actuels = lister(dossier, sorte.garde()).map_err(|e| format!("configuration illisible : {e}"))?;
    // Chaque déplacement fait est noté (de, vers) pour pouvoir le défaire.
    let mut faits: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut deplacer = |de: PathBuf, vers: PathBuf| -> io::Result<()> {
        if let Some(parent) = vers.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&de, &vers)?;
        faits.push((de, vers));
        Ok(())
    };
    let resultat = actuels
        .iter()
        .try_for_each(|r| deplacer(vers_disque(dossier, r), vers_disque(rangement, r)))
        .and_then(|()| importes.iter().try_for_each(|r| deplacer(vers_disque(attente, r), vers_disque(dossier, r))));
    if let Err(err) = resultat {
        for (de, vers) in faits.into_iter().rev() {
            let _ = fs::rename(&vers, &de);
        }
        return Err(format!("import défait, la configuration est inchangée : {err}"));
    }
    let _ = fs::remove_dir_all(attente);
    // ⭐ Une archive faite sur un autre ecran : ses longueurs seront adaptees au
    // lancement suivant (voir `config_initiale::au_lancement`), et seulement ce
    // qu'elle a pose.
    if importes.iter().any(|r| r == crate::config_initiale::REFERENCE) {
        let sorte = if sorte == Sorte::Apparence { "apparence" } else { "complete" };
        let _ = fs::write(dossier.join(crate::config_initiale::A_ADAPTER), sorte);
    }
    Ok(importes.len())
}

/// L'heure locale : `AAAA-MM-JJ HH:MM` pour l'afficher, `AAAA-MM-JJ-HHMMSS` pour
/// un nom de dossier (qui ne peut pas porter de `:`).
pub fn maintenant(pour_un_nom: bool) -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    if pour_un_nom {
        format!("{:04}-{:02}-{:02}-{:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
    } else {
        format!("{:04}-{:02}-{:02} {:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute)
    }
}

/// **LE POINT D'APPEL DE `main`** — au lancement, et après `app::run` avant de
/// lancer le remplaçant (voir la doc de module).
pub fn appliquer_si_confirme() {
    let Some(dossier) = crate::config::config_dir() else { return };
    match appliquer_import_en_attente(&dossier, &maintenant(true)) {
        None => {}
        Some(Ok(n)) => crate::diag!("[sauvegarde] import appliqué : {n} fichier(s), l'ancienne configuration est rangée"),
        Some(Err(err)) => crate::diag!("[sauvegarde] import NON appliqué : {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dossier_de_test(nom: &str) -> PathBuf {
        let dossier = std::env::temp_dir().join(format!("neuradesk_sauvegarde_{nom}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dossier);
        fs::create_dir_all(&dossier).unwrap();
        dossier
    }

    fn poser(dossier: &Path, relatif: &str, contenu: &str) {
        let chemin = vers_disque(dossier, relatif);
        fs::create_dir_all(chemin.parent().unwrap()).unwrap();
        fs::write(chemin, contenu).unwrap();
    }

    fn lire(dossier: &Path, relatif: &str) -> Option<String> {
        fs::read_to_string(vers_disque(dossier, relatif)).ok()
    }

    #[test]
    fn ce_qui_part_et_ce_qui_reste() {
        for oui in ["icon_order.txt", "zones.toml", "ssh_hotes_connus", "notes_images/a.png", "palettes/charles.gpl", "textures/Marbre.ndtexture"] {
            assert!(est_exporte(oui), "{oui} part");
        }
        for non in [
            "clipboard_history.toml",
            "clipboard_images/x.png",
            "icon_order.txt.avant-reserve-epingles",
            "notes.toml.2",
            "surveillance/1.bin",
            "avant-import-2026-10-10/zones.toml",
            "import-en-attente/zones.toml",
            "notes_images/sous/a.png",
            MANIFESTE,
        ] {
            assert!(!est_exporte(non), "{non} reste");
        }
    }

    /// ⛔ Une archive ne doit rien pouvoir poser hors du dossier.
    #[test]
    fn un_chemin_qui_sort_du_dossier_est_refuse() {
        for chemin in ["../config.toml", "notes_images/..", "C:/Windows/x.txt", "/zones.toml", "notes_images/a\\..\\b", ""] {
            assert!(!est_exporte(chemin), "{chemin:?}");
        }
    }

    #[test]
    fn un_export_se_reimporte_a_l_identique() {
        let source = dossier_de_test("source");
        poser(&source, "zones.toml", "zones");
        poser(&source, "notes_images/a.png", "image");
        poser(&source, "clipboard_history.toml", "prive");
        poser(&source, "zones.toml.avant-x", "vieux");
        let archive = source.join("export.zip");
        assert_eq!(exporter(&source, &archive, "2026-10-10", Sorte::Complete).unwrap(), 2);

        let cible = dossier_de_test("cible");
        poser(&cible, "zones.toml", "ancien");
        poser(&cible, "feature_clock.txt", "local");
        poser(&cible, "clipboard_history.toml", "presse-papiers local");

        let apercu = preparer_import(&archive, &cible).unwrap();
        assert_eq!(apercu, Apercu { fichiers: 2, date: "2026-10-10".to_string(), sorte: Sorte::Complete });
        assert_eq!(lire(&cible, "zones.toml").as_deref(), Some("ancien"), "préparer ne touche à rien");
        assert!(appliquer_import_en_attente(&cible, "t").is_none(), "sans confirmation, rien ne s'applique");

        confirmer_import(&cible).unwrap();
        assert_eq!(appliquer_import_en_attente(&cible, "t"), Some(Ok(2)));

        assert_eq!(lire(&cible, "zones.toml").as_deref(), Some("zones"));
        assert_eq!(lire(&cible, "notes_images/a.png").as_deref(), Some("image"));
        assert_eq!(lire(&cible, "feature_clock.txt"), None, "la configuration importée est EXACTE, pas un mélange");
        assert_eq!(lire(&cible, "clipboard_history.toml").as_deref(), Some("presse-papiers local"));
        assert_eq!(lire(&cible, "avant-import-t/zones.toml").as_deref(), Some("ancien"), "l'ancienne est rangée");
        assert_eq!(lire(&cible, "avant-import-t/feature_clock.txt").as_deref(), Some("local"));
        assert!(!cible.join(EN_ATTENTE).exists());
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&cible);
    }

    #[test]
    fn une_archive_sans_manifeste_est_refusee() {
        let dossier = dossier_de_test("sans_manifeste");
        let archive = dossier.join("autre.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        zip.start_file("zones.toml", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();

        assert!(preparer_import(&archive, &dossier).is_err());
        assert!(!dossier.join(EN_ATTENTE).exists(), "l'extraction partielle est effacée");
        let _ = fs::remove_dir_all(&dossier);
    }

    #[test]
    fn une_entree_hors_export_refuse_toute_l_archive() {
        let dossier = dossier_de_test("entree_refusee");
        let archive = dossier.join("piege.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        for nom in [MANIFESTE, "zones.toml", "../evade.txt"] {
            zip.start_file(nom, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"x").unwrap();
        }
        zip.finish().unwrap();

        let erreur = preparer_import(&archive, &dossier).unwrap_err();
        assert!(erreur.contains("evade"), "{erreur}");
        assert!(!dossier.join(EN_ATTENTE).exists());
        assert!(!dossier.parent().unwrap().join("evade.txt").exists());
        let _ = fs::remove_dir_all(&dossier);
    }

    #[test]
    fn l_apparence_et_le_reste() {
        for oui in [
            "style.txt",
            "icones.txt",
            "theme_famille.txt",
            "degrade_sauvegardes.toml",
            "dash_accent_color.txt",
            "bottom_icon_scale.txt",
            "menu_tint.txt",
            "tooltip_fade_ms.txt",
            "lueurs.txt",
            "lueurs/soft.lueurs",
            "lueurs_images/1b631e1f a.png",
            "shaders/bulles.hlsl",
            "palettes/charles.gpl",
            "textures/Marbre.ndtexture",
            "secondary_screen_colors.txt",
        ] {
            assert!(est_apparence(oui), "{oui} est de l'apparence");
        }
        for non in [
            "notes.toml",
            "dates.toml",
            "zones.toml",
            "icon_order.txt",
            "dossiers.toml",
            "dock_pins.txt",
            "ssh.toml",
            "commandes.toml",
            "automatisations.toml",
            "feature_clock.txt",
            "lueurs_integres.txt",
            "textures_integres.txt",
            "secondary_screen_icon_order.txt",
            "notes_images/a.png",
            "dossiers_images/a.png",
            "style.txt.avant-x",
            "clipboard_history.toml",
            MANIFESTE,
        ] {
            assert!(!est_apparence(non), "{non} n'est pas de l'apparence");
        }
    }

    /// ⭐ **L'IMPORT DE L'APPARENCE NE TOUCHE QU'À L'APPARENCE** — l'export
    /// complet aurait remplacé les notes de la cible par celles de la source.
    #[test]
    fn l_apparence_se_reimporte_sans_toucher_au_reste() {
        let source = dossier_de_test("apparence_source");
        poser(&source, "style.txt", "gnome");
        poser(&source, "lueurs/soft.lueurs", "lueur");
        poser(&source, "notes.toml", "notes de la source");
        let archive = source.join("apparence.zip");
        assert_eq!(exporter(&source, &archive, "2026-10-24", Sorte::Apparence).unwrap(), 2);

        let cible = dossier_de_test("apparence_cible");
        poser(&cible, "style.txt", "kde");
        poser(&cible, "color.txt", "ancienne couleur");
        poser(&cible, "notes.toml", "notes de la cible");
        poser(&cible, "zones.toml", "zones de la cible");

        let apercu = preparer_import(&archive, &cible).unwrap();
        assert_eq!(apercu.sorte, Sorte::Apparence);
        assert_eq!(apercu.fichiers, 2);
        confirmer_import(&cible).unwrap();
        assert_eq!(appliquer_import_en_attente(&cible, "t"), Some(Ok(2)));

        assert_eq!(lire(&cible, "style.txt").as_deref(), Some("gnome"));
        assert_eq!(lire(&cible, "lueurs/soft.lueurs").as_deref(), Some("lueur"));
        assert_eq!(lire(&cible, "notes.toml").as_deref(), Some("notes de la cible"), "les notes restent");
        assert_eq!(lire(&cible, "zones.toml").as_deref(), Some("zones de la cible"));
        assert_eq!(lire(&cible, "color.txt"), None, "l'apparence importée est EXACTE");
        assert_eq!(lire(&cible, "avant-import-t/color.txt").as_deref(), Some("ancienne couleur"));
        assert_eq!(lire(&cible, "avant-import-t/notes.toml"), None, "seule l'apparence est rangée");
        assert!(!cible.join(EN_ATTENTE).exists());
        // Sans écran de référence dans l'archive, rien à adapter.
        assert!(!cible.join(crate::config_initiale::A_ADAPTER).exists());
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&cible);
    }

    /// ⭐ **UNE ARCHIVE FAITE SUR UN AUTRE ÉCRAN EST ADAPTÉE AU LANCEMENT SUIVANT** —
    /// l'import pose la marque, avec la sorte : une archive d'apparence ne doit
    /// convertir que l'apparence (voir `config_initiale::au_lancement`).
    #[test]
    fn une_archive_avec_son_ecran_pose_la_marque_d_adaptation() {
        let source = dossier_de_test("reference_source");
        poser(&source, "style.txt", "gnome");
        poser(&source, crate::config_initiale::REFERENCE, "3840,2160,144");
        let archive = source.join("apparence.zip");
        assert_eq!(exporter(&source, &archive, "2026-10-24", Sorte::Apparence).unwrap(), 2);

        let cible = dossier_de_test("reference_cible");
        preparer_import(&archive, &cible).unwrap();
        confirmer_import(&cible).unwrap();
        assert_eq!(appliquer_import_en_attente(&cible, "t"), Some(Ok(2)));

        assert_eq!(lire(&cible, crate::config_initiale::A_ADAPTER).as_deref(), Some("apparence"));
        assert_eq!(lire(&cible, crate::config_initiale::REFERENCE).as_deref(), Some("3840,2160,144"));
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&cible);
    }

    /// ⛔ Une archive qui se dit « apparence » mais porte des notes est refusée :
    /// son import les poserait sans ranger celles de l'utilisateur.
    #[test]
    fn une_archive_d_apparence_qui_porte_des_donnees_est_refusee() {
        let dossier = dossier_de_test("apparence_piege");
        let archive = dossier.join("piege.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        for (nom, contenu) in [("style.txt", "x"), ("notes.toml", "x"), (MANIFESTE, "sorte = \"apparence\"\n")] {
            zip.start_file(nom, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(contenu.as_bytes()).unwrap();
        }
        zip.finish().unwrap();

        let erreur = preparer_import(&archive, &dossier).unwrap_err();
        assert!(erreur.contains("notes.toml"), "{erreur}");
        assert!(!dossier.join(EN_ATTENTE).exists());
        let _ = fs::remove_dir_all(&dossier);
    }

    /// Diagnostic ponctuel : ce que l'export de l'apparence prend du VRAI dossier.
    #[test]
    #[ignore]
    fn sonde_apparence_du_dossier_reel() {
        let dossier = crate::config::config_dir().unwrap();
        let oui = lister(&dossier, est_apparence).unwrap();
        let tous = lister(&dossier, est_exporte).unwrap();
        println!("APPARENCE {} : {}", oui.len(), oui.join(" "));
        let reste: Vec<_> = tous.iter().filter(|r| !oui.contains(r)).cloned().collect();
        println!("RESTE {} : {}", reste.len(), reste.join(" "));
    }

    #[test]
    fn annuler_efface_l_extraction() {
        let source = dossier_de_test("annuler_source");
        poser(&source, "zones.toml", "zones");
        let archive = source.join("export.zip");
        exporter(&source, &archive, "", Sorte::Complete).unwrap();
        let cible = dossier_de_test("annuler_cible");
        preparer_import(&archive, &cible).unwrap();

        annuler_import(&cible);

        assert!(!cible.join(EN_ATTENTE).exists());
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&cible);
    }
}
