//! Les opérations de la vue « Fichiers » (lot 2 de `docs/plans/ssh-sftp.md`).
//! Chaque requête tourne dans sa propre tâche (voir `mener`) : un gros
//! transfert ne gèle ni le terminal ni les autres requêtes.
//!
//! ⛔ **UN NOM VENU DU SERVEUR N'EST PAS UN NOM DE FICHIER WINDOWS SÛR** — il
//! peut contenir `\`, `:`, `..` : tout ce qui s'écrit sur le disque local passe
//! par [`nom_local`].

use std::path::{Path, PathBuf};
use std::time::Instant;

use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::{emettre, EntreeSftp, Evenement, ReponseSftp, RequeteSftp, CADENCE_PROGRES, MORCEAU};

pub(super) async fn traiter_sftp(sftp: &SftpSession, onglet: u32, requete: RequeteSftp) -> ReponseSftp {
    let resultat = match requete {
        RequeteSftp::Lister { chemin } => lister(sftp, &chemin).await,
        RequeteSftp::Telecharger { distants, dossier_local } => {
            Ok(telecharger(sftp, onglet, distants, &dossier_local).await)
        }
        RequeteSftp::Televerser { locaux, dossier_distant } => {
            Ok(televerser(sftp, onglet, locaux, &dossier_distant).await)
        }
        RequeteSftp::CreerDossier { dossier, nom } => creer_dossier(sftp, &dossier, &nom).await,
        RequeteSftp::Renommer { chemin, nom } => renommer(sftp, &chemin, &nom).await,
        RequeteSftp::Supprimer { chemins } => Ok(supprimer(sftp, chemins).await),
        RequeteSftp::Droits { chemin, mode } => droits(sftp, &chemin, mode).await,
    };
    resultat.unwrap_or_else(|message| ReponseSftp::Erreur { message })
}

fn fait(message: String) -> ReponseSftp {
    ReponseSftp::Fait { message, erreurs: Vec::new() }
}

async fn lister(sftp: &SftpSession, chemin: &str) -> Result<ReponseSftp, String> {
    let chemin = if chemin.is_empty() { "." } else { chemin };
    let absolu = sftp.canonicalize(chemin).await.map_err(|e| format!("{chemin} : {e}"))?;
    let mut entrees: Vec<EntreeSftp> = sftp
        .read_dir(absolu.clone())
        .await
        .map_err(|e| format!("{absolu} : {e}"))?
        .filter(|entree| entree.file_name() != "." && entree.file_name() != "..")
        .map(|entree| {
            let meta = entree.metadata();
            EntreeSftp {
                nom: entree.file_name(),
                dossier: meta.is_dir(),
                lien: meta.is_symlink(),
                taille: meta.size.unwrap_or(0),
                modifie: meta.mtime.unwrap_or(0),
                droits: meta.permissions.unwrap_or(0) & 0o7777,
            }
        })
        .collect();
    // Dossiers d'abord, puis par nom sans casse — l'ordre de tout gestionnaire de fichiers.
    entrees.sort_by(|a, b| b.dossier.cmp(&a.dossier).then_with(|| a.nom.to_lowercase().cmp(&b.nom.to_lowercase())));
    Ok(ReponseSftp::Liste { chemin: absolu, entrees })
}

/// `dossier` + `nom` en chemin distant (toujours `/`, quel que soit le client).
fn joindre(dossier: &str, nom: &str) -> String {
    if dossier.ends_with('/') { format!("{dossier}{nom}") } else { format!("{dossier}/{nom}") }
}

/// Le dossier qui contient `chemin` ; `/` pour un élément de la racine.
fn parent(chemin: &str) -> &str {
    match chemin.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/",
        Some((dossier, _)) => dossier,
    }
}

/// Le dernier élément de `chemin`.
fn dernier(chemin: &str) -> &str {
    chemin.trim_end_matches('/').rsplit('/').next().unwrap_or(chemin)
}

/// Un nom SAISI pour le serveur : un seul élément, ni vide, ni `.`, ni `..`.
pub fn nom_distant(nom: &str) -> Result<&str, String> {
    let nom = nom.trim();
    if nom.is_empty() || nom == "." || nom == ".." {
        return Err("Nom invalide.".into());
    }
    if nom.contains('/') || nom.contains('\0') {
        return Err("Un nom ne peut pas contenir « / ».".into());
    }
    Ok(nom)
}

/// Un nom venu du SERVEUR, rendu sûr pour Windows : caractères interdits
/// remplacés, points et espaces finaux retirés (Windows les ignore — `..`
/// deviendrait vide, donc refusé), noms réservés (`CON`, `COM1`…) préfixés.
/// `None` : rien d'utilisable ne reste.
pub(super) fn nom_local(nom: &str) -> Option<String> {
    let propre: String =
        nom.chars().map(|c| if c < ' ' || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let propre = propre.trim_end_matches(['.', ' ']);
    if propre.is_empty() {
        return None;
    }
    let base = propre.split('.').next().unwrap_or_default().to_ascii_uppercase();
    let numerote = base.len() == 4
        && (base.starts_with("COM") || base.starts_with("LPT"))
        && base.as_bytes()[3].is_ascii_digit();
    let reserve = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numerote;
    Some(if reserve { format!("_{propre}") } else { propre.to_string() })
}

/// Un chemin libre dans `dossier` : `nom`, sinon « nom (1).ext », « nom (2).ext »…
pub(super) fn libre(dossier: &Path, nom: &str) -> PathBuf {
    let candidat = dossier.join(nom);
    if !candidat.exists() {
        return candidat;
    }
    let (base, extension) = match nom.rsplit_once('.') {
        Some((base, ext)) if !base.is_empty() => (base, format!(".{ext}")),
        _ => (nom, String::new()),
    };
    (1..)
        .map(|n| dossier.join(format!("{base} ({n}){extension}")))
        .find(|chemin| !chemin.exists())
        .unwrap_or(candidat)
}

/// Copie `lecture` dans `ecriture` en annonçant l'avancée à la page.
async fn copier<L, E>(onglet: u32, nom: &str, total: u64, lecture: &mut L, ecriture: &mut E) -> std::io::Result<()>
where
    L: AsyncRead + Unpin,
    E: AsyncWrite + Unpin,
{
    let mut tampon = vec![0u8; MORCEAU];
    let mut fait = 0u64;
    let mut annonce = Instant::now();
    loop {
        let lu = lecture.read(&mut tampon).await?;
        if lu == 0 {
            break;
        }
        ecriture.write_all(&tampon[..lu]).await?;
        fait += lu as u64;
        if annonce.elapsed() >= CADENCE_PROGRES {
            annonce = Instant::now();
            let reponse = ReponseSftp::Progres { nom: nom.to_string(), fait, total };
            emettre(Evenement::Sftp { onglet, reponse });
        }
    }
    // `shutdown` ferme la poignée distante : sans lui, les dernières écritures
    // d'un téléversement pourraient ne pas être acquittées.
    ecriture.shutdown().await
}

fn pluriel(nombre: usize, mot: &str) -> String {
    format!("{nombre} {mot}{}", if nombre > 1 { "s" } else { "" })
}

enum Telecharge {
    Fichier,
    /// Le dossier local créé, et ce qu'il faut y télécharger.
    Dossier(PathBuf, Vec<String>),
}

async fn telecharger(sftp: &SftpSession, onglet: u32, distants: Vec<String>, dossier_local: &Path) -> ReponseSftp {
    // Une pile plutôt qu'une récursion : une fonction `async` récursive
    // demanderait d'emballer chaque niveau.
    let mut a_faire: Vec<(String, PathBuf, bool)> =
        distants.into_iter().map(|d| (d, dossier_local.to_path_buf(), true)).collect();
    let (mut fichiers, mut erreurs) = (0, Vec::new());
    while let Some((distant, dossier, choisi)) = a_faire.pop() {
        match telecharger_element(sftp, onglet, &distant, &dossier, choisi).await {
            Ok(Some(Telecharge::Fichier)) => fichiers += 1,
            Ok(Some(Telecharge::Dossier(local, enfants))) => {
                a_faire.extend(enfants.into_iter().map(|e| (e, local.clone(), false)))
            }
            Ok(None) => {}
            Err(message) => erreurs.push(message),
        }
    }
    let message = format!("{} téléchargé(s) dans {}", pluriel(fichiers, "fichier"), dossier_local.display());
    ReponseSftp::Fait { message, erreurs }
}

/// `choisi` : l'utilisateur l'a désigné lui-même, donc un lien est SUIVI ; à
/// l'intérieur d'un dossier, un lien est ignoré — il pourrait pointer vers un
/// dossier parent et ne jamais finir.
async fn telecharger_element(
    sftp: &SftpSession,
    onglet: u32,
    distant: &str,
    dossier: &Path,
    choisi: bool,
) -> Result<Option<Telecharge>, String> {
    let nom = dernier(distant);
    let local = nom_local(nom).ok_or_else(|| format!("{nom} : nom inutilisable sous Windows"))?;
    if !choisi && sftp.symlink_metadata(distant).await.map_err(|e| format!("{nom} : {e}"))?.is_symlink() {
        return Ok(None);
    }
    let meta = sftp.metadata(distant).await.map_err(|e| format!("{nom} : {e}"))?;
    if meta.is_dir() {
        let local = libre(dossier, &local);
        std::fs::create_dir_all(&local).map_err(|e| format!("{} : {e}", local.display()))?;
        let enfants = sftp
            .read_dir(distant)
            .await
            .map_err(|e| format!("{nom} : {e}"))?
            .map(|e| e.file_name())
            .filter(|n| n != "." && n != "..")
            .map(|n| joindre(distant, &n))
            .collect();
        return Ok(Some(Telecharge::Dossier(local, enfants)));
    }
    let local = libre(dossier, &local);
    let mut lecture = sftp.open(distant).await.map_err(|e| format!("{nom} : {e}"))?;
    let mut ecriture = tokio::fs::File::create(&local).await.map_err(|e| format!("{} : {e}", local.display()))?;
    if let Err(erreur) = copier(onglet, nom, meta.size.unwrap_or(0), &mut lecture, &mut ecriture).await {
        drop(ecriture);
        // Un fichier à moitié écrit ne doit pas passer pour une copie.
        let _ = std::fs::remove_file(&local);
        return Err(format!("{nom} : {erreur}"));
    }
    Ok(Some(Telecharge::Fichier))
}

enum Televerse {
    Fichier,
    /// Le dossier distant créé, et ce qu'il faut y téléverser.
    Dossier(String, Vec<PathBuf>),
}

/// Le miroir de [`telecharger`] : une pile, un lien suivi seulement s'il a été
/// choisi.
async fn televerser(sftp: &SftpSession, onglet: u32, locaux: Vec<PathBuf>, dossier_distant: &str) -> ReponseSftp {
    let mut a_faire: Vec<(PathBuf, String, bool)> =
        locaux.into_iter().map(|l| (l, dossier_distant.to_string(), true)).collect();
    let (mut fichiers, mut erreurs) = (0, Vec::new());
    while let Some((local, dossier, choisi)) = a_faire.pop() {
        match televerser_element(sftp, onglet, &local, &dossier, choisi).await {
            Ok(Some(Televerse::Fichier)) => fichiers += 1,
            Ok(Some(Televerse::Dossier(distant, enfants))) => {
                a_faire.extend(enfants.into_iter().map(|e| (e, distant.clone(), false)))
            }
            Ok(None) => {}
            Err(message) => erreurs.push(message),
        }
    }
    let message = format!("{} téléversé(s) dans {dossier_distant}", pluriel(fichiers, "fichier"));
    ReponseSftp::Fait { message, erreurs }
}

async fn televerser_element(
    sftp: &SftpSession,
    onglet: u32,
    local: &Path,
    dossier_distant: &str,
    choisi: bool,
) -> Result<Option<Televerse>, String> {
    let nom = local.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let nom = nom_distant(&nom).map_err(|e| format!("{} : {e}", local.display()))?.to_string();
    let distant = joindre(dossier_distant, &nom);
    let lien = tokio::fs::symlink_metadata(local).await.map_err(|e| format!("{nom} : {e}"))?.is_symlink();
    if lien && !choisi {
        return Ok(None);
    }
    if sftp.try_exists(distant.clone()).await.unwrap_or(false) {
        return Err(format!("{nom} : existe déjà sur le serveur"));
    }
    if tokio::fs::metadata(local).await.map_err(|e| format!("{nom} : {e}"))?.is_dir() {
        sftp.create_dir(distant.clone()).await.map_err(|e| format!("{nom} : {e}"))?;
        let mut lecture = tokio::fs::read_dir(local).await.map_err(|e| format!("{nom} : {e}"))?;
        let mut enfants = Vec::new();
        while let Some(entree) = lecture.next_entry().await.map_err(|e| format!("{nom} : {e}"))? {
            enfants.push(entree.path());
        }
        return Ok(Some(Televerse::Dossier(distant, enfants)));
    }
    televerser_fichier(sftp, onglet, local, &nom, distant).await.map(|()| Some(Televerse::Fichier))
}

async fn televerser_fichier(
    sftp: &SftpSession,
    onglet: u32,
    local: &Path,
    nom: &str,
    distant: String,
) -> Result<(), String> {
    let nom = nom.to_string();
    let mut lecture = tokio::fs::File::open(local).await.map_err(|e| format!("{nom} : {e}"))?;
    let total = lecture.metadata().await.map(|m| m.len()).unwrap_or(0);
    // `EXCLUDE` : même si le fichier apparaît entre la vérification et
    // l'ouverture, le serveur refuse plutôt que d'écraser.
    let drapeaux = OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE;
    let mut ecriture = sftp.open_with_flags(distant.clone(), drapeaux).await.map_err(|e| format!("{nom} : {e}"))?;
    if let Err(erreur) = copier(onglet, &nom, total, &mut lecture, &mut ecriture).await {
        drop(ecriture);
        let _ = sftp.remove_file(distant).await;
        return Err(format!("{nom} : {erreur}"));
    }
    Ok(())
}

async fn creer_dossier(sftp: &SftpSession, dossier: &str, nom: &str) -> Result<ReponseSftp, String> {
    let nom = nom_distant(nom)?;
    sftp.create_dir(joindre(dossier, nom)).await.map_err(|e| format!("{nom} : {e}"))?;
    Ok(fait(format!("Dossier « {nom} » créé")))
}

async fn renommer(sftp: &SftpSession, chemin: &str, nom: &str) -> Result<ReponseSftp, String> {
    let nom = nom_distant(nom)?;
    let nouveau = joindre(parent(chemin), nom);
    if sftp.try_exists(nouveau.clone()).await.unwrap_or(false) {
        return Err(format!("« {nom} » existe déjà."));
    }
    sftp.rename(chemin, nouveau).await.map_err(|e| format!("{} : {e}", dernier(chemin)))?;
    Ok(fait(format!("Renommé en « {nom} »")))
}

async fn supprimer(sftp: &SftpSession, chemins: Vec<String>) -> ReponseSftp {
    // Parcours en profondeur, le dossier APRÈS son contenu (`vide` = son
    // contenu est déjà sur la pile).
    let mut a_faire: Vec<(String, bool)> = chemins.into_iter().map(|c| (c, false)).collect();
    let (mut elements, mut erreurs) = (0, Vec::new());
    while let Some((chemin, vide)) = a_faire.pop() {
        let nom = dernier(&chemin).to_string();
        if vide {
            match sftp.remove_dir(chemin).await {
                Ok(()) => elements += 1,
                Err(e) => erreurs.push(format!("{nom} : {e}")),
            }
            continue;
        }
        // ⛔ `symlink_metadata`, PAS `metadata` : un lien vers un dossier se
        // supprime lui-même ; le suivre viderait le dossier pointé.
        let meta = match sftp.symlink_metadata(chemin.clone()).await {
            Ok(meta) => meta,
            Err(e) => {
                erreurs.push(format!("{nom} : {e}"));
                continue;
            }
        };
        if !meta.is_dir() {
            match sftp.remove_file(chemin).await {
                Ok(()) => elements += 1,
                Err(e) => erreurs.push(format!("{nom} : {e}")),
            }
            continue;
        }
        match sftp.read_dir(chemin.clone()).await {
            Ok(contenu) => {
                a_faire.push((chemin.clone(), true));
                a_faire.extend(
                    contenu
                        .map(|e| e.file_name())
                        .filter(|n| n != "." && n != "..")
                        .map(|n| (joindre(&chemin, &n), false)),
                );
            }
            Err(e) => erreurs.push(format!("{nom} : {e}")),
        }
    }
    ReponseSftp::Fait { message: format!("{} supprimé(s)", pluriel(elements, "élément")), erreurs }
}

async fn droits(sftp: &SftpSession, chemin: &str, mode: u32) -> Result<ReponseSftp, String> {
    if mode > 0o7777 {
        return Err(format!("Droits invalides : {mode:o}"));
    }
    let mut attributs = FileAttributes::empty();
    attributs.permissions = Some(mode);
    sftp.set_metadata(chemin, attributs).await.map_err(|e| format!("{} : {e}", dernier(chemin)))?;
    Ok(fait(format!("Droits de « {} » : {mode:03o}", dernier(chemin))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chemins_distants() {
        assert_eq!(joindre("/", "a"), "/a");
        assert_eq!(joindre("/home/moi", "a"), "/home/moi/a");
        assert_eq!(parent("/home/moi/a"), "/home/moi");
        assert_eq!(parent("/a"), "/");
        assert_eq!(dernier("/home/moi/a"), "a");
        assert_eq!(dernier("/home/moi/"), "moi");
    }

    #[test]
    fn un_nom_saisi_est_un_seul_element() {
        assert_eq!(nom_distant("  notes.txt "), Ok("notes.txt"));
        for mauvais in ["", " ", ".", "..", "a/b", "../x"] {
            assert!(nom_distant(mauvais).is_err(), "{mauvais:?}");
        }
    }

    /// ⛔ Le cas qui compte : un serveur malveillant qui nomme un fichier
    /// `..\..\x` ne doit rien écrire hors du dossier choisi.
    #[test]
    fn un_nom_du_serveur_ne_sort_pas_du_dossier_local() {
        assert_eq!(nom_local("rapport.pdf").as_deref(), Some("rapport.pdf"));
        assert_eq!(nom_local("..\\..\\x").as_deref(), Some(".._.._x"));
        assert_eq!(nom_local("a:b*c?").as_deref(), Some("a_b_c_"));
        assert_eq!(nom_local(".."), None);
        assert_eq!(nom_local(". ."), None);
        assert_eq!(nom_local("fin. ").as_deref(), Some("fin"));
        assert_eq!(nom_local("con.txt").as_deref(), Some("_con.txt"));
        assert_eq!(nom_local("COM1").as_deref(), Some("_COM1"));
        assert_eq!(nom_local("COMA").as_deref(), Some("COMA"));
        assert_eq!(nom_local(".bashrc").as_deref(), Some(".bashrc"));
    }

    #[test]
    fn un_nom_local_pris_est_numerote() {
        let dossier = std::env::temp_dir().join(format!("nd-ssh-libre-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dossier);
        std::fs::create_dir_all(&dossier).unwrap();
        assert_eq!(libre(&dossier, "a.txt"), dossier.join("a.txt"));
        std::fs::write(dossier.join("a.txt"), b"").unwrap();
        assert_eq!(libre(&dossier, "a.txt"), dossier.join("a (1).txt"));
        std::fs::write(dossier.join("a (1).txt"), b"").unwrap();
        assert_eq!(libre(&dossier, "a.txt"), dossier.join("a (2).txt"));
        std::fs::write(dossier.join(".bashrc"), b"").unwrap();
        assert_eq!(libre(&dossier, ".bashrc"), dossier.join(".bashrc (1)"));
        let _ = std::fs::remove_dir_all(&dossier);
    }
}
