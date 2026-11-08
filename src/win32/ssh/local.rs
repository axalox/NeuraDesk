//! Le volet GAUCHE de la vue « Fichiers » : les dossiers de CE PC. La page n'a
//! aucun accès au disque (règle de toutes les pages du dépôt) : Rust liste, la
//! page affiche. Lecture seule — rien ici n'écrit.
//!
//! Deux chemins convenus, jamais pris pour de vrais dossiers (ni `*` ni `~` ne
//! font un chemin ABSOLU Windows) :
//! - [`CE_PC`] (`*`) : les dossiers usuels et les lecteurs ;
//! - `~` : le dossier personnel.

use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use windows::core::GUID;
use windows::Win32::Storage::FileSystem::GetLogicalDrives;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Profile, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
};

pub const CE_PC: &str = "*";

#[derive(Clone, Debug, PartialEq)]
pub struct EntreeLocale {
    pub nom: String,
    /// Chemin complet : la page n'a pas à joindre des chemins Windows.
    pub chemin: String,
    pub dossier: bool,
    pub lien: bool,
    pub taille: u64,
    /// Secondes depuis 1970 (0 si inconnu).
    pub modifie: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListeLocale {
    /// [`CE_PC`] ou un chemin absolu.
    pub chemin: String,
    /// `None` : on est déjà sur « Ce PC ».
    pub parent: Option<String>,
    pub entrees: Vec<EntreeLocale>,
}

pub fn lister(chemin: &str) -> Result<ListeLocale, String> {
    let chemin = chemin.trim();
    if chemin == CE_PC {
        return Ok(ce_pc());
    }
    let dossier = if chemin == "~" {
        dossier_connu(&FOLDERID_Profile).ok_or("Dossier personnel introuvable.")?
    } else {
        PathBuf::from(chemin)
    };
    if !dossier.is_absolute() {
        return Err(format!("{chemin} : chemin complet attendu (ex. C:\\Users)."));
    }
    let lecture = std::fs::read_dir(&dossier).map_err(|e| format!("{} : {e}", dossier.display()))?;
    let mut entrees: Vec<EntreeLocale> = lecture
        .filter_map(Result::ok)
        .filter_map(|entree| {
            // `DirEntry::metadata` ne suit pas les liens : un lien est montré
            // comme tel, pas comme sa cible.
            let meta = entree.metadata().ok()?;
            if meta.file_attributes() & (ATTRIBUT_CACHE | ATTRIBUT_SYSTEME) != 0 {
                return None;
            }
            let lien = meta.file_type().is_symlink();
            // Un lien vers un dossier s'ouvre comme un dossier.
            let dossier = meta.is_dir() || (lien && entree.path().is_dir());
            Some(EntreeLocale {
                nom: entree.file_name().to_string_lossy().into_owned(),
                chemin: entree.path().display().to_string(),
                dossier,
                lien,
                taille: if dossier { 0 } else { meta.len() },
                modifie: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs()),
            })
        })
        .collect();
    entrees.sort_by(|a, b| b.dossier.cmp(&a.dossier).then_with(|| a.nom.to_lowercase().cmp(&b.nom.to_lowercase())));
    Ok(ListeLocale {
        parent: Some(dossier.parent().map_or(CE_PC.to_string(), |p| p.display().to_string())),
        chemin: dossier.display().to_string(),
        entrees,
    })
}

/// Comme l'Explorateur par défaut : ni fichiers cachés ni fichiers système
/// (`pagefile.sys`, `$Recycle.Bin`… à la racine d'un lecteur).
const ATTRIBUT_CACHE: u32 = 0x2;
const ATTRIBUT_SYSTEME: u32 = 0x4;

fn ce_pc() -> ListeLocale {
    let usuels = [
        ("Dossier personnel", &FOLDERID_Profile),
        ("Bureau", &FOLDERID_Desktop),
        ("Documents", &FOLDERID_Documents),
        ("Téléchargements", &FOLDERID_Downloads),
    ];
    let dossier = |nom: &str, chemin: &Path| EntreeLocale {
        nom: nom.to_string(),
        chemin: chemin.display().to_string(),
        dossier: true,
        lien: false,
        taille: 0,
        modifie: 0,
    };
    let mut entrees: Vec<EntreeLocale> = usuels
        .iter()
        .filter_map(|(nom, id)| Some(dossier(nom, &dossier_connu(id)?)))
        .collect();
    // ⚠️ Aucune question posée aux lecteurs (nom de volume…) : un lecteur
    // réseau déconnecté peut faire attendre de longues secondes.
    let masque = unsafe { GetLogicalDrives() };
    entrees.extend(
        (0..26u8)
            .filter(|i| masque & (1 << i) != 0)
            .map(|i| char::from(b'A' + i))
            .map(|lettre| dossier(&format!("{lettre}:"), Path::new(&format!("{lettre}:\\")))),
    );
    ListeLocale { chemin: CE_PC.to_string(), parent: None, entrees }
}

/// **LE DOSSIER PERSONNEL DE L'UTILISATEUR** (`FOLDERID_Profile`), pour les
/// appelants qui ne veulent pas manipuler un `GUID`.
///
/// ⚠️ **C'EST LA SEULE DÉFINITION, ET ELLE EST ICI À DESSEIN.** `lister` s'en
/// sert pour `~`, et le dash dossier pour le défaut de sa moitié droite
/// (`config::load_sous_dock_personnel`). Une seconde copie écrite ailleurs
/// aurait sa propre idée de ce qu'est « le dossier personnel » — le dépôt a
/// déjà tranché ce genre de question pour `icone_du_shell`, qui réutilise
/// `cef_dash` « plutôt que de recopier ce détail Win32 ».
///
/// Le repli est `%USERPROFILE%` en clair, qui n'est PAS la même source : il ne
/// suit pas un profil déplacé par le shell. Il n'existe que pour le cas où
/// `SHGetKnownFolderPath` échoue, et il vaut mieux qu'un chemin vide.
pub fn dossier_personnel_du_profil() -> PathBuf {
    dossier_connu(&FOLDERID_Profile)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        // Dernier repli : la racine du disque système. Un dossier qui existe
        // TOUJOURS — la moitié droite ne peut donc jamais rester vide faute de
        // chemin, ce qui est le seul défaut vraiment gênant ici.
        .unwrap_or_else(|| PathBuf::from(r"C:\"))
}

/// **LE DOSSIER « DOCUMENTS » DE L'UTILISATEUR** (`FOLDERID_Documents`) — le
/// défaut du dossier de scans de l'applet Scanner (`config::load_dossier_scans`).
///
/// ⛔ **`FOLDERID_Documents` ET NON `%USERPROFILE%\Documents`** : le shell peut
/// REDIRIGER ce dossier — c'est le cas sur cette machine, où il vit sous
/// `OneDrive\Documents`. Le chemin en clair désignerait un dossier que
/// l'utilisateur ne regarde jamais, et le scan atterrirait ailleurs que là où il
/// l'attend.
///
/// Le repli est le dossier personnel, qui existe toujours : un scan doit pouvoir
/// partir même si le shell refuse de répondre.
pub fn dossier_documents() -> PathBuf {
    dossier_connu(&FOLDERID_Documents).unwrap_or_else(dossier_personnel_du_profil)
}

pub(crate) fn dossier_connu(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let brut = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let chemin = brut.to_string().ok();
        // Alloué par le shell : c'est à l'appelant de le libérer.
        CoTaskMemFree(Some(brut.0 as *const core::ffi::c_void));
        chemin.map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_dossier_se_liste_dossiers_d_abord_avec_chemins_complets() {
        let racine = std::env::temp_dir().join(format!("nd-ssh-local-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&racine);
        std::fs::create_dir_all(racine.join("zeta")).unwrap();
        std::fs::write(racine.join("Alpha.txt"), b"12345").unwrap();

        let liste = lister(&racine.display().to_string()).unwrap();
        let noms: Vec<&str> = liste.entrees.iter().map(|e| e.nom.as_str()).collect();
        assert_eq!(noms, ["zeta", "Alpha.txt"]);
        assert!(liste.entrees[0].dossier);
        assert_eq!(liste.entrees[1].taille, 5);
        assert_eq!(liste.entrees[1].chemin, racine.join("Alpha.txt").display().to_string());
        assert_eq!(liste.parent.as_deref(), Some(racine.parent().unwrap().display().to_string().as_str()));

        std::fs::remove_dir_all(&racine).unwrap();
    }

    #[test]
    fn la_racine_d_un_lecteur_remonte_a_ce_pc_qui_liste_les_lecteurs() {
        let systeme = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let racine = lister(&format!("{systeme}\\")).unwrap();
        assert_eq!(racine.parent.as_deref(), Some(CE_PC));
        assert!(racine.entrees.iter().all(|e| e.nom != "pagefile.sys" && e.nom != "$Recycle.Bin"));

        let pc = lister(CE_PC).unwrap();
        assert_eq!(pc.parent, None);
        assert!(pc.entrees.iter().any(|e| e.nom == systeme && e.chemin == format!("{systeme}\\")));
        assert!(pc.entrees.iter().any(|e| e.nom == "Dossier personnel"));
    }

    #[test]
    fn un_chemin_relatif_ou_absent_est_refuse() {
        assert!(lister("Documents").is_err());
        assert!(lister("C:").is_err(), "« C: » est relatif au dossier courant du lecteur");
        assert!(lister("C:\\ce-dossier-n-existe-pas-nd").is_err());
        assert!(lister("~").is_ok());
    }
}
