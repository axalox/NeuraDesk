//! **AGIR SUR UN FICHIER DE L'UTILISATEUR** — renommer, supprimer, propriétés,
//! « ouvrir avec… ».
//!
//! ⛔ **AUCUNE DE CES QUATRE OPÉRATIONS N'EXISTAIT DANS CE DÉPÔT.** Vérifié :
//! zéro occurrence de `SHFileOperation`/`IFileOperation`/`DeleteFile`/`MoveFileEx`
//! sur un chemin d'utilisateur, zéro `SHObjectProperties`, et le seul renommage
//! qui existait portait sur un RÉGLAGE (le nom affiché d'un dossier épinglé) ou
//! sur un fichier DISTANT (SFTP). Le presse-papiers de fichiers est à part, dans
//! `win32::clipboard_fichiers`.
//!
//! ⛔ **LA RÈGLE DU DÉPÔT, ÉCRITE À TROIS ENDROITS** (`ui/zones.rs:1188-1192`,
//! `ui/dock.rs:4197`, `app.rs:3721`) : « **créer est réparable, supprimer ne
//! l'est pas** ». Elle commande tout ce fichier :
//!
//! - la suppression va **TOUJOURS** à la corbeille (`FOF_ALLOWUNDO`) — jamais de
//!   `DeleteFile`, jamais d'effacement définitif ;
//! - **on n'écrase JAMAIS** : un renommage sur une cible existante est refusé, et
//!   le refus est DIT ;
//! - **un échec n'est pas un non-événement** : chaque fonction rend un `Result`
//!   dont le message est montré à l'utilisateur. Le dépôt refuse les échecs
//!   silencieux, et c'est ici qu'ils coûteraient le plus cher — un renommage qui
//!   n'a pas eu lieu se lirait comme un renommage qui n'a pas pris.

use std::path::{Path, PathBuf};

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    SHFileOperationW, SHObjectProperties, SHOP_FILEPATH, FOF_ALLOWUNDO, FOF_NOCONFIRMATION,
    FOF_NOCONFIRMMKDIR, FOF_NOERRORUI, FOF_RENAMEONCOLLISION, FOF_SILENT, FO_COPY, FO_DELETE, FO_MOVE,
    SHFILEOPSTRUCTW,
};
use windows::Win32::UI::Shell::{SHOpenWithDialog, OAIF_EXEC, OAIF_HIDE_REGISTRATION, OPENASINFO};

/// Un chemin Windows prêt pour une API `PCWSTR`, **terminé par un zéro**. La
/// `Vec` doit vivre aussi longtemps que l'appel — d'où une fonction qui la rend
/// plutôt qu'un `PCWSTR`.
fn large(chemin: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    chemin.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

/// **« OUVRIR AVEC… »** — le dialogue de Windows, avec le choix d'application.
///
/// ⚠️ **`OAIF_HIDE_REGISTRATION`** : on ne propose pas « toujours utiliser cette
/// application ». C'est la même décision que l'éditeur d'image (`cef_image_editor
/// .rs:418-419`), prise pour une raison voisine — ici parce qu'un fichier qu'on
/// ouvre par curiosité ne mérite pas de changer une association pour toute la
/// session.
///
/// ⚠️ Le dialogue est **MODAL** et gèle le redessin du dock tant qu'il est
/// ouvert — assumé, et déjà documenté (`win32::file_dialog`, doc de module).
pub fn ouvrir_avec(proprietaire: HWND, chemin: &Path) -> Result<(), String> {
    let chemin_w = large(chemin);
    let info = OPENASINFO {
        pcszFile: PCWSTR(chemin_w.as_ptr()),
        pcszClass: PCWSTR::null(),
        oaifInFlags: OAIF_EXEC | OAIF_HIDE_REGISTRATION,
    };
    unsafe { SHOpenWithDialog(Some(proprietaire), &info) }.map_err(|e| e.to_string())
}

/// **LA FEUILLE DE PROPRIÉTÉS DE WINDOWS** — celle du clic droit de l'Explorateur.
///
/// ⚠️ Elle s'ouvre sur la page « Général » par défaut (`pszpropertypage` nul) :
/// c'est ce qu'on veut, et choisir une page supposerait de connaître le type de
/// fichier.
pub fn proprietes(proprietaire: HWND, chemin: &Path) -> Result<(), String> {
    let chemin_w = large(chemin);
    let ouvert = unsafe {
        SHObjectProperties(Some(proprietaire), SHOP_FILEPATH, PCWSTR(chemin_w.as_ptr()), PCWSTR::null())
    };
    if ouvert.as_bool() {
        Ok(())
    } else {
        Err("Windows n'a pas ouvert la feuille de propriétés".to_string())
    }
}

/// **SUPPRIMER — VERS LA CORBEILLE, ET RIEN D'AUTRE.**
///
/// ⛔ **`FOF_ALLOWUNDO` N'EST PAS UNE OPTION ICI.** C'est la seule chose qui
/// sépare cette fonction d'un effacement définitif, et le dépôt a écrit trois fois
/// que « supprimer n'est pas réparable ». Un `DeleteFile` serait plus court de
/// trois lignes et prendrait des données pour toujours.
///
/// ⚠️ **`FOF_NOCONFIRMATION` EST LÀ PARCE QUE LA CONFIRMATION EST LA NÔTRE** :
/// la page demande confirmation avant d'envoyer le geste (voir
/// `app::Demande::Supprimer`). Sans notre confirmation, ce drapeau serait une
/// faute — c'est la combinaison des deux qui est juste, jamais l'un sans l'autre.
///
/// ⚠️ Un DOSSIER part avec tout son contenu : `SHFileOperationW` est récursif par
/// nature, et c'est bien ce que veut dire « supprimer un dossier ».
pub fn supprimer_vers_la_corbeille(proprietaire: HWND, chemin: &Path) -> Result<(), String> {
    if !chemin.exists() {
        return Err("le fichier n'existe plus".to_string());
    }
    // ⛔ **`pFrom` VEUT UN DOUBLE ZÉRO** : c'est ainsi que la liste des chemins se
    // termine. Un seul zéro, et l'API lit au-delà — elle trouve alors du vide et
    // rend « rien à faire », c'est-à-dire un échec silencieux.
    let mut depuis = large(chemin);
    depuis.push(0);
    let mut operation = SHFILEOPSTRUCTW {
        hwnd: proprietaire,
        wFunc: FO_DELETE,
        pFrom: PCWSTR(depuis.as_ptr()),
        pTo: PCWSTR::null(),
        // `.0 as u16` : les drapeaux sont un `FILEOPERATION_FLAGS`, dont
        // l'intérieur est un `u32` — alors que le CHAMP est un `u16`. On combine
        // d'abord (l'opérateur `|` est défini sur le type), on rétrécit ensuite.
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        fAnyOperationsAborted: Default::default(),
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: PCWSTR::null(),
    };
    let code = unsafe { SHFileOperationW(&mut operation) };
    if code != 0 {
        return Err(format!("Windows a refusé la suppression (code {code})"));
    }
    if operation.fAnyOperationsAborted.as_bool() {
        // Le cas ne devrait pas arriver (aucun dialogue n'est montré), mais le
        // taire ferait passer une suppression annulée pour un succès.
        return Err("la suppression a été interrompue".to_string());
    }
    Ok(())
}

/// **RENOMMER — APRÈS AVOIR VALIDÉ, ET SANS JAMAIS ÉCRASER.**
///
/// Rend le chemin du fichier renommé, ou un message qui dit POURQUOI.
///
/// ⛔ **ON NE REMPLACE PAS, ET ON NE « (2) » PAS.** Renommer en douce vers
/// `nom (2).ext` cacherait un conflit que l'utilisateur doit voir : peut-être
/// voulait-il justement écraser, peut-être s'est-il trompé de dossier — dans les
/// deux cas, c'est à lui de trancher, et un message le lui dit.
pub fn renommer(chemin: &Path, nouveau_nom: &str) -> Result<PathBuf, String> {
    let nom = nouveau_nom.trim();
    valider_nom(nom)?;
    let Some(dossier) = chemin.parent() else {
        return Err("ce chemin n'a pas de dossier parent".to_string());
    };
    let cible = dossier.join(nom);
    if cible == chemin {
        // Renommer vers le même nom n'est pas une erreur : c'est un geste sans
        // effet. On le rend tel quel plutôt que d'écrire pour rien.
        return Ok(cible);
    }
    if cible.exists() {
        return Err(format!("« {nom} » existe déjà dans ce dossier"));
    }
    std::fs::rename(chemin, &cible).map_err(|e| e.to_string())?;
    Ok(cible)
}

/// Ce que `creer` fabrique — un dossier, ou un fichier VIDE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nouveau {
    Dossier,
    Fichier,
}

/// **CRÉER DANS UN DOSSIER — APRÈS AVOIR VALIDÉ, ET SANS JAMAIS ÉCRASER.**
///
/// Rend le chemin créé, ou un message qui dit POURQUOI.
///
/// ⛔ **MÊME RÈGLE QUE `renommer` : PAS DE « (2) » EN DOUCE.** Le nom est celui
/// que l'utilisateur vient de taper ; s'il est pris, c'est à lui de le voir.
///
/// ⚠️ **`create_new` ET NON `exists()` PUIS `write`** pour le fichier : entre les
/// deux, un autre programme pourrait créer le même nom, et `write` le VIDERAIT.
/// `create_new` échoue au lieu d'écraser — c'est le système qui tranche, pas un
/// test fait un instant plus tôt. `create_dir` a la même garantie d'origine.
pub fn creer(dossier: &Path, nom: &str, sorte: Nouveau) -> Result<PathBuf, String> {
    let nom = nom.trim();
    valider_nom(nom)?;
    let cible = dossier.join(nom);
    let resultat = match sorte {
        Nouveau::Dossier => std::fs::create_dir(&cible),
        Nouveau::Fichier => std::fs::OpenOptions::new().write(true).create_new(true).open(&cible).map(|_| ()),
    };
    match resultat {
        Ok(()) => Ok(cible),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(format!("« {nom} » existe déjà dans ce dossier"))
        }
        Err(e) => Err(e.to_string()),
    }
}

/// **UN NOM DE FICHIER VALIDE POUR WINDOWS — FONCTION PURE, DONC TESTABLE.**
///
/// ⚠️ **CE N'EST PAS DU ZÈLE : c'est Windows qui refuse, et son refus arrive
/// APRÈS l'appel**, sous forme d'un code d'erreur obscur. Valider avant permet de
/// dire « le nom contient un `:` » au lieu de « le système a refusé ».
///
/// Les quatre familles de refus, toutes mesurables :
/// - vide, ou fait d'espaces (le `trim` de l'appelant les enlève) ;
/// - un caractère interdit : `\ / : * ? " < > |` ;
/// - `.` ou `..`, qui sont des DOSSIERS, pas des noms ;
/// - un nom RÉSERVÉ par le système (`CON`, `NUL`, `COM1`…), qui existe encore
///   aujourd'hui pour la compatibilité DOS — et dont l'usage échoue de façon
///   incompréhensible ;
/// - une espace ou un point EN FIN, que Windows retire en silence : le fichier
///   créé ne porterait pas le nom demandé.
pub fn valider_nom(nom: &str) -> Result<(), String> {
    if nom.is_empty() {
        return Err("le nom ne peut pas être vide".to_string());
    }
    if nom == "." || nom == ".." {
        return Err("« . » et « .. » désignent des dossiers".to_string());
    }
    if let Some(interdit) = nom.chars().find(|c| "\\/:*?\"<>|".contains(*c)) {
        return Err(format!("le nom ne peut pas contenir « {interdit} »"));
    }
    if let Some(interdit) = nom.chars().find(|c| (*c as u32) < 32) {
        return Err(format!("le nom contient un caractère de contrôle (code {})", interdit as u32));
    }
    if nom.ends_with(' ') || nom.ends_with('.') {
        return Err("un nom ne peut pas finir par une espace ni par un point".to_string());
    }
    let base = nom.split('.').next().unwrap_or(nom).to_ascii_uppercase();
    const RESERVES: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVES.contains(&base.as_str()) {
        return Err(format!("« {nom} » est un nom réservé par Windows"));
    }
    Ok(())
}

/// `chemin` est-il `dossier` lui-même ou l'un de ses parents ? Sans tenir
/// compte de la casse, comme Windows.
fn contient_ou_egal(chemin: &Path, dossier: &Path) -> bool {
    let bas = |p: &Path| PathBuf::from(p.to_string_lossy().to_lowercase());
    bas(dossier).starts_with(bas(chemin))
}

/// **CE QUI SERA VRAIMENT COLLÉ** — pur, donc testable. Refuse de coller un
/// dossier dans lui-même ou dans l'un de ses sous-dossiers (la copie ne
/// finirait pas) ; un DÉPLACEMENT vers le dossier où l'élément est déjà est
/// écarté : il n'y a rien à faire.
fn a_coller(sources: &[PathBuf], dossier: &Path, deplacer: bool) -> Result<Vec<PathBuf>, String> {
    if let Some(source) = sources.iter().find(|s| contient_ou_egal(s, dossier)) {
        let nom = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        return Err(format!("impossible de coller « {nom} » dans lui-même"));
    }
    let restes: Vec<PathBuf> = sources
        .iter()
        .filter(|s| !(deplacer && s.parent().is_some_and(|p| contient_ou_egal(p, dossier) && contient_ou_egal(dossier, p))))
        .cloned()
        .collect();
    if restes.is_empty() {
        return Err("déjà dans ce dossier".to_string());
    }
    Ok(restes)
}

/// Une liste de chemins pour `SHFILEOPSTRUCTW` : chacun terminé par un zéro,
/// et un zéro de plus pour clore la liste (voir `supprimer_vers_la_corbeille`).
fn liste_double_zero(chemins: &[PathBuf]) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let mut liste: Vec<u16> = Vec::new();
    for chemin in chemins {
        liste.extend(chemin.as_os_str().encode_wide());
        liste.push(0);
    }
    liste.push(0);
    liste
}

/// **COLLER — COPIER OU DÉPLACER `sources` DANS `dossier`**, fichiers ET
/// dossiers (`SHFileOperationW` est récursif). Rend le nombre d'éléments
/// collés.
///
/// ⛔ **ON N'ÉCRASE JAMAIS** (règle du module) : `FOF_RENAMEONCOLLISION` donne
/// « nom - Copie » quand le nom est pris, comme l'Explorateur qui colle dans le
/// même dossier. `FOF_ALLOWUNDO` rend le geste annulable dans l'Explorateur.
///
/// ⚠️ **BLOQUANTE** : une grosse copie dure. L'appelant la lance sur un fil à
/// part ; Windows montre sa propre fenêtre de progression, au-dessus de
/// `proprietaire`.
pub fn coller(proprietaire: HWND, sources: &[PathBuf], dossier: &Path, deplacer: bool) -> Result<usize, String> {
    let sources = a_coller(sources, dossier, deplacer)?;
    let depuis = liste_double_zero(&sources);
    let vers = liste_double_zero(&[dossier.to_path_buf()]);
    let mut operation = SHFILEOPSTRUCTW {
        hwnd: proprietaire,
        wFunc: if deplacer { FO_MOVE } else { FO_COPY },
        pFrom: PCWSTR(depuis.as_ptr()),
        pTo: PCWSTR(vers.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_RENAMEONCOLLISION | FOF_NOCONFIRMMKDIR).0 as u16,
        fAnyOperationsAborted: Default::default(),
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: PCWSTR::null(),
    };
    let code = unsafe { SHFileOperationW(&mut operation) };
    if code != 0 {
        return Err(format!("Windows a refusé le collage (code {code})"));
    }
    if operation.fAnyOperationsAborted.as_bool() {
        return Err("le collage a été interrompu".to_string());
    }
    Ok(sources.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⛔ **CHAQUE FAMILLE DE REFUS A SON CAS.** Le test refuse les noms que
    /// Windows refuserait — mais AVANT l'appel, pour que l'utilisateur lise
    /// « le nom contient un `:` » plutôt qu'un code d'erreur système.
    #[test]
    fn coller_un_dossier_dans_lui_meme_est_refuse() {
        let a = PathBuf::from(r"C:\Docs\Projet");
        assert!(a_coller(&[a.clone()], Path::new(r"C:\Docs\Projet"), false).is_err());
        assert!(a_coller(&[a.clone()], Path::new(r"c:\docs\projet\sous"), false).is_err());
        // Un VOISIN au nom qui commence pareil n'est pas un sous-dossier.
        assert_eq!(a_coller(&[a.clone()], Path::new(r"C:\Docs\Projet2"), false).unwrap(), [a.clone()]);
    }

    #[test]
    fn deplacer_vers_son_propre_dossier_ne_fait_rien_mais_copier_duplique() {
        let f = PathBuf::from(r"C:\Docs\note.txt");
        assert!(a_coller(&[f.clone()], Path::new(r"c:\docs"), true).is_err());
        assert_eq!(a_coller(&[f.clone()], Path::new(r"C:\Docs"), false).unwrap(), [f.clone()]);
        let g = PathBuf::from(r"D:\Autre\g.txt");
        assert_eq!(a_coller(&[f, g.clone()], Path::new(r"C:\Docs"), true).unwrap(), [g]);
    }

    #[test]
    fn la_liste_finit_par_deux_zeros() {
        let l = liste_double_zero(&[PathBuf::from("a"), PathBuf::from("bc")]);
        assert_eq!(l, [b'a' as u16, 0, b'b' as u16, b'c' as u16, 0, 0]);
    }

    #[test]
    fn un_nom_de_fichier_invalide_est_refuse() {
        assert!(valider_nom("").is_err(), "vide");
        assert!(valider_nom("..").is_err(), "« .. » est un dossier");
        assert!(valider_nom(".").is_err());

        for interdit in ["a\\b", "a/b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b"] {
            assert!(valider_nom(interdit).is_err(), "{interdit:?} contient un caractere interdit");
        }
        assert!(valider_nom("fichier\n").is_err(), "caractere de controle");
        assert!(valider_nom("fichier ").is_err(), "une espace finale serait retiree en silence");
        assert!(valider_nom("fichier.").is_err(), "un point final serait retire en silence");

        // Les noms réservés, y compris AVEC une extension — c'est le cas qu'on
        // oublie : `nul.txt` est refusé comme `nul`.
        assert!(valider_nom("CON").is_err());
        assert!(valider_nom("nul.txt").is_err());
        assert!(valider_nom("Com1.log").is_err());
    }

    /// **ET LES NOMS VALIDES LE RESTENT** — sans quoi le test ci-dessus passerait
    /// en refusant tout, ce qui est le piège classique d'un validateur.
    #[test]
    fn un_nom_de_fichier_valide_est_accepte() {
        for bon in ["notes.txt", "Résumé été 2026.md", ".gitignore", "a", "CONSOLE.txt", "COM10"] {
            assert!(valider_nom(bon).is_ok(), "{bon:?} devrait etre accepte");
        }
    }

    /// ⛔ **UN RENOMMAGE SUR UNE CIBLE EXISTANTE EST REFUSÉ.** C'est la règle qui
    /// protège les données : renommer par-dessus un autre fichier l'effacerait, et
    /// « supprimer n'est pas réparable ».
    #[test]
    fn un_renommage_sur_une_cible_existante_est_refuse() {
        let bac = std::env::temp_dir().join(format!("nd-renommer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&bac);
        std::fs::create_dir_all(&bac).expect("bac");
        let source = bac.join("source.txt");
        let occupee = bac.join("occupee.txt");
        std::fs::write(&source, b"a").expect("ecriture");
        std::fs::write(&occupee, b"b").expect("ecriture");

        let refus = renommer(&source, "occupee.txt");
        assert!(refus.is_err(), "ecraser un fichier existant doit etre REFUSE");
        assert!(source.exists(), "et la source doit etre INTACTE");
        assert_eq!(std::fs::read(&occupee).expect("lecture"), b"b", "et la cible AUSSI");

        // Un nom libre passe, et rend le nouveau chemin.
        let rendu = renommer(&source, "libre.txt").expect("renommage");
        assert_eq!(rendu, bac.join("libre.txt"));
        assert!(rendu.exists());
        assert!(!source.exists());

        // Renommer vers le MÊME nom n'est pas une erreur : c'est un geste sans
        // effet, et le refuser ferait échouer un simple clic sur Entrée.
        assert!(renommer(&rendu, "libre.txt").is_ok());

        let _ = std::fs::remove_dir_all(&bac);
    }

    /// Un nom invalide ne doit RIEN toucher sur le disque : la validation vient
    /// avant, et c'est tout son intérêt.
    #[test]
    fn un_renommage_invalide_ne_touche_a_rien() {
        let bac = std::env::temp_dir().join(format!("nd-renommer-invalide-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&bac);
        std::fs::create_dir_all(&bac).expect("bac");
        let source = bac.join("source.txt");
        std::fs::write(&source, b"a").expect("ecriture");

        assert!(renommer(&source, "inva:lide.txt").is_err());
        assert!(source.exists(), "la source doit etre intacte");
        assert!(!bac.join("inva:lide.txt").exists());

        let _ = std::fs::remove_dir_all(&bac);
    }

    /// ⛔ **CRÉER SUR UN NOM PRIS EST REFUSÉ, ET LE CONTENU EXISTANT EST INTACT.**
    /// C'est le cas qui compte : un fichier « créé » par-dessus un autre le
    /// viderait.
    #[test]
    fn creer_sur_un_nom_pris_est_refuse_sans_rien_toucher() {
        let bac = std::env::temp_dir().join(format!("nd-creer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&bac);
        std::fs::create_dir_all(&bac).expect("bac");
        std::fs::write(bac.join("occupe.txt"), b"contenu").expect("ecriture");
        std::fs::create_dir(bac.join("occupe")).expect("dossier");

        assert!(creer(&bac, "occupe.txt", Nouveau::Fichier).is_err());
        assert_eq!(std::fs::read(bac.join("occupe.txt")).expect("lecture"), b"contenu", "le fichier doit rester INTACT");
        assert!(creer(&bac, "occupe", Nouveau::Dossier).is_err());
        // Un DOSSIER du même nom bloque aussi un FICHIER, et inversement.
        assert!(creer(&bac, "occupe", Nouveau::Fichier).is_err());
        assert!(creer(&bac, "occupe.txt", Nouveau::Dossier).is_err());

        let _ = std::fs::remove_dir_all(&bac);
    }

    /// Un nom libre crée la bonne SORTE, et rend son chemin ; un nom invalide ne
    /// crée rien.
    #[test]
    fn creer_un_nom_libre_fabrique_la_bonne_sorte() {
        let bac = std::env::temp_dir().join(format!("nd-creer-libre-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&bac);
        std::fs::create_dir_all(&bac).expect("bac");

        let dossier = creer(&bac, " Nouveau dossier ", Nouveau::Dossier).expect("dossier");
        assert_eq!(dossier, bac.join("Nouveau dossier"), "le nom est rogne comme pour renommer");
        assert!(dossier.is_dir());
        let fichier = creer(&bac, "notes.txt", Nouveau::Fichier).expect("fichier");
        assert!(fichier.is_file());
        assert_eq!(std::fs::metadata(&fichier).expect("meta").len(), 0, "un fichier neuf est VIDE");

        assert!(creer(&bac, "inva:lide", Nouveau::Dossier).is_err());
        assert!(creer(&bac, "", Nouveau::Fichier).is_err());
        assert_eq!(std::fs::read_dir(&bac).expect("lecture").count(), 2, "rien d'autre n'a ete cree");

        let _ = std::fs::remove_dir_all(&bac);
    }

    /// Supprimer un chemin qui n'existe plus n'est pas une panne silencieuse :
    /// c'est DIT, sinon l'utilisateur croirait avoir supprimé quelque chose.
    #[test]
    fn supprimer_un_chemin_absent_le_dit() {
        let absent = std::env::temp_dir().join("nd-ce-chemin-n-existe-pas-du-tout.txt");
        assert!(supprimer_vers_la_corbeille(HWND(std::ptr::null_mut()), &absent).is_err());
    }
}
