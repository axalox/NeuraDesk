//! **LE PRESSE-PAPIERS DE FICHIERS** — `CF_HDROP`, pour COPIER ou COUPER des
//! fichiers que l'Explorateur (ou n'importe quelle application) pourra coller.
//!
//! ⛔ **CE MODULE N'EXISTAIT PAS, ET CE N'EST PAS UN OUBLI.** `win32::clipboard`
//! ne traite que du TEXTE (`CF_UNICODETEXT`) et de l'IMAGE (PNG/`CF_DIBV5`) ; son
//! `read()` écarte explicitement « les fichiers copiés » (voir sa doc). Un
//! presse-papiers de fichiers est un **format différent**, avec sa propre
//! structure — d'où un module à part plutôt qu'un troisième bras dans un fichier
//! qui parle d'images.
//!
//! L'écriture fabrique la structure octet par octet (voir `bloc_hdrop`, qui est
//! PUR et testable pour cette raison exacte). La LECTURE (voir `lire`, pour
//! « Coller » dans le Dash dossier) passe par `DragQueryFileW` : le bloc vient
//! alors d'une autre application, et c'est Windows qui sait le déplier dans
//! toutes ses variantes (ANSI ou UTF-16).
//!
//! ⚠️ **COUPER N'EST PAS COPIER**, et la différence n'est pas dans `CF_HDROP` :
//! elle est dans un SECOND format, `CFSTR_PREFERREDDROPEFFECT`, qui dit au
//! destinataire s'il doit DÉPLACER ou RECOPIER. L'omettre ferait qu'un « Couper »
//! se comporte comme un « Copier » — l'Explorateur collerait une copie et
//! laisserait l'original en place, sans que rien ne le signale.

use std::path::PathBuf;

use windows::core::w;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_HDROP;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

/// **LA TAILLE DE L'EN-TÊTE `DROPFILES`** : `DWORD pFiles`, `POINT pt` (deux
/// `LONG`), `BOOL fNC`, `BOOL fWide` — cinq mots de 32 bits, sans rembourrage
/// possible puisqu'ils sont tous alignés sur 4.
///
/// ⛔ **C'EST AUSSI LA VALEUR DU CHAMP `pFiles`**, et c'est ce qui rend la
/// constante unique : le décalage de la liste EST la taille de l'en-tête. Les
/// écrire séparément serait deux nombres à faire tenir d'accord.
const TAILLE_ENTETE: usize = 20;

/// `DROPEFFECT_COPY` et `DROPEFFECT_MOVE` — valeurs de l'énumération Win32
/// `DROPEFFECT`, portées par le second format.
const DROPEFFECT_COPY: u32 = 1;
const DROPEFFECT_MOVE: u32 = 2;

/// **LE BLOC `CF_HDROP` POUR CES CHEMINS — PUR, DONC TESTABLE.**
///
/// C'est la seule partie délicate du module, et elle ne touche à rien : une
/// structure de 20 octets, puis les chemins en UTF-16 **chacun terminé par un
/// zéro**, puis **un zéro de plus**. Le double zéro final dit « la liste s'arrête
/// ici » — l'oublier donne un bloc que le système accepte et qu'un destinataire
/// lit de travers, sans erreur nulle part.
///
/// ⚠️ **UNE SEULE LISTE, À PLAT** : `CF_HDROP` ne connaît pas les dossiers — un
/// dossier voyage comme un chemin, et c'est le destinataire qui le déplie.
fn bloc_hdrop(chemins: &[PathBuf]) -> Vec<u8> {
    let mut bloc = Vec::with_capacity(TAILLE_ENTETE + chemins.len() * 64);
    // `fWide: 1` est OBLIGATOIRE : sans lui, le destinataire lit la liste comme
    // des octets ANSI, et un chemin accentué devient du charabia.
    bloc.extend_from_slice(&(TAILLE_ENTETE as u32).to_le_bytes()); // pFiles
    bloc.extend_from_slice(&0i32.to_le_bytes()); // pt.x — le point de dépôt
    bloc.extend_from_slice(&0i32.to_le_bytes()); // pt.y   n'ont pas de sens ici
    bloc.extend_from_slice(&0i32.to_le_bytes()); // fNC : jamais la zone non cliente
    bloc.extend_from_slice(&1i32.to_le_bytes()); // fWide : chemins en UTF-16
    for chemin in chemins {
        for unite in chemin.as_os_str().to_string_lossy().encode_utf16() {
            bloc.extend_from_slice(&unite.to_le_bytes());
        }
        bloc.extend_from_slice(&0u16.to_le_bytes()); // fin de CE chemin
    }
    bloc.extend_from_slice(&0u16.to_le_bytes()); // fin de LA LISTE
    bloc
}

/// **ÉCRIT DES FICHIERS DANS LE PRESSE-PAPIERS** — `couper` choisit entre
/// « Copier » et « Couper ».
///
/// Rend `false` si le presse-papiers n'a pas pu être pris — le plus souvent parce
/// qu'une AUTRE application le tient ouvert. On le DIT à l'appelant plutôt que de
/// laisser croire à une copie qui n'a pas eu lieu : sans ça, l'utilisateur
/// collerait son contenu PRÉCÉDENT et ne comprendrait pas.
///
/// ⚠️ **CE QUE DEVIENT LA MÉMOIRE.** Après un `SetClipboardData` RÉUSSI, le bloc
/// appartient au SYSTÈME : il ne faut ni le libérer, ni déverrouiller son
/// pointeur. Les échecs sont donc traités séparément — un `GlobalAlloc` raté n'a
/// rien alloué, un `SetClipboardData` raté laisse le bloc nous appartenir.
pub fn ecrire(chemins: &[PathBuf], couper: bool) -> bool {
    if chemins.is_empty() {
        return false;
    }
    let bloc = bloc_hdrop(chemins);
    unsafe {
        if OpenClipboard(None).is_err() {
            return false;
        }
        // On vide AVANT d'écrire : sans ça, l'ancien contenu resterait collable
        // en plus du nôtre, et le presse-papiers porterait deux vérités.
        let _ = EmptyClipboard();
        let ecrit = ecrire_un_format(CF_HDROP.0 as u32, &bloc) && ecrire_effet(couper);
        let _ = CloseClipboard();
        ecrit
    }
}

/// Dépose UN format dans le presse-papiers, à partir d'octets bruts.
unsafe fn ecrire_un_format(format: u32, octets: &[u8]) -> bool {
    let Ok(memoire) = (unsafe { GlobalAlloc(GMEM_MOVEABLE, octets.len()) }) else {
        return false;
    };
    let pointeur = unsafe { GlobalLock(memoire) };
    if pointeur.is_null() {
        return false;
    }
    unsafe { std::ptr::copy_nonoverlapping(octets.as_ptr(), pointeur as *mut u8, octets.len()) };
    let _ = unsafe { GlobalUnlock(memoire) };
    // À partir d'ici, un succès TRANSFÈRE la propriété au système.
    unsafe { SetClipboardData(format, Some(HANDLE(memoire.0))).is_ok() }
}

/// **LE SECOND FORMAT — CELUI QUI DISTINGUE « COUPER » DE « COPIER ».**
///
/// ⚠️ Il est facultatif au sens où `CF_HDROP` suffit à coller, et indispensable
/// au sens où sans lui « Couper » recopie au lieu de déplacer. On l'écrit donc
/// TOUJOURS, même pour une copie : `DROPEFFECT_COPY` dit alors explicitement ce
/// que le silence dirait par défaut.
unsafe fn ecrire_effet(couper: bool) -> bool {
    // Le format est ENREGISTRÉ à la volée : son identifiant n'est pas une
    // constante, il dépend de la session.
    let format = unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) };
    if format == 0 {
        return false;
    }
    let effet = if couper { DROPEFFECT_MOVE } else { DROPEFFECT_COPY };
    unsafe { ecrire_un_format(format, &effet.to_le_bytes()) }
}

/// **LIT LES FICHIERS DU PRESSE-PAPIERS** — copiés ici ou dans l'Explorateur.
/// Rend les chemins et `true` s'ils ont été COUPÉS (à déplacer), ou `None` si le
/// presse-papiers ne porte pas de fichiers ou n'a pas pu être ouvert.
pub fn lire() -> Option<(Vec<PathBuf>, bool)> {
    unsafe {
        OpenClipboard(None).ok()?;
        let lu = lire_ouvert();
        let _ = CloseClipboard();
        lu
    }
}

unsafe fn lire_ouvert() -> Option<(Vec<PathBuf>, bool)> {
    let donnee = unsafe { GetClipboardData(CF_HDROP.0 as u32) }.ok()?;
    let hdrop = HDROP(donnee.0);
    let nombre = unsafe { DragQueryFileW(hdrop, u32::MAX, None) };
    let mut chemins = Vec::new();
    for rang in 0..nombre {
        let longueur = unsafe { DragQueryFileW(hdrop, rang, None) } as usize;
        let mut tampon = vec![0u16; longueur + 1];
        let lu = unsafe { DragQueryFileW(hdrop, rang, Some(&mut tampon)) } as usize;
        if lu > 0 {
            chemins.push(PathBuf::from(String::from_utf16_lossy(&tampon[..lu])));
        }
    }
    if chemins.is_empty() {
        return None;
    }
    // Sans le second format, c'est une COPIE : c'est ce que fait l'Explorateur.
    let couper = unsafe { lire_effet() }.is_some_and(|effet| effet & DROPEFFECT_MOVE != 0);
    Some((chemins, couper))
}

unsafe fn lire_effet() -> Option<u32> {
    let format = unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) };
    let donnee = unsafe { GetClipboardData(format) }.ok()?;
    let memoire = HGLOBAL(donnee.0);
    if unsafe { GlobalSize(memoire) } < 4 {
        return None;
    }
    let pointeur = unsafe { GlobalLock(memoire) } as *const u8;
    if pointeur.is_null() {
        return None;
    }
    let effet = unsafe { std::ptr::read_unaligned(pointeur as *const u32) };
    let _ = unsafe { GlobalUnlock(memoire) };
    Some(effet)
}

/// Vide le presse-papiers — après un « Couper » collé, comme l'Explorateur :
/// les originaux ont bougé, recoller ne pourrait qu'échouer.
pub fn vider() {
    unsafe {
        if OpenClipboard(None).is_ok() {
            let _ = EmptyClipboard();
            let _ = CloseClipboard();
        }
    }
}

/// **RELIRE UN BLOC `CF_HDROP`** — le pendant de `bloc_hdrop`, et il n'existe QUE
/// pour les tests. C'est ce qui transforme « le bloc a l'air bien formé » en
/// « le bloc se relit à l'identique » : sans lui, les tests vérifieraient la
/// structure contre ma propre compréhension d'elle.
#[cfg(test)]
fn depuis_bloc(bloc: &[u8]) -> Vec<PathBuf> {
    if bloc.len() < TAILLE_ENTETE {
        return Vec::new();
    }
    let decalage = u32::from_le_bytes([bloc[0], bloc[1], bloc[2], bloc[3]]) as usize;
    let Some(reste) = bloc.get(decalage..) else { return Vec::new() };
    let unites: Vec<u16> = reste
        .chunks_exact(2)
        .map(|paire| u16::from_le_bytes([paire[0], paire[1]]))
        .collect();
    let mut trouves = Vec::new();
    let mut debut = 0usize;
    for (rang, unite) in unites.iter().enumerate() {
        if *unite != 0 {
            continue;
        }
        if rang == debut {
            break; // Deux zéros de suite : fin de la liste.
        }
        trouves.push(PathBuf::from(String::from_utf16_lossy(&unites[debut..rang])));
        debut = rang + 1;
    }
    trouves
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **LE BLOC EST BIEN FORMÉ, ET SE RELIT.** C'est le seul garde-fou possible :
    /// un `CF_HDROP` mal formé est accepté par `SetClipboardData` et refusé (ou
    /// lu de travers) par le destinataire, **sans un mot nulle part**.
    #[test]
    fn le_bloc_cf_hdrop_est_bien_forme() {
        let chemins =
            vec![PathBuf::from(r"C:\essai\un.txt"), PathBuf::from(r"C:\essai\Été deux.txt")];
        let bloc = bloc_hdrop(&chemins);

        let decalage = u32::from_le_bytes([bloc[0], bloc[1], bloc[2], bloc[3]]);
        assert_eq!(decalage as usize, TAILLE_ENTETE, "pFiles doit valoir la taille de l'entete");
        assert_eq!(i32::from_le_bytes([bloc[8], bloc[9], bloc[10], bloc[11]]), 0, "pt.x");
        assert_eq!(i32::from_le_bytes([bloc[12], bloc[13], bloc[14], bloc[15]]), 0, "pt.y");
        assert_eq!(
            i32::from_le_bytes([bloc[16], bloc[17], bloc[18], bloc[19]]),
            1,
            "fWide DOIT valoir 1 : sans lui, un chemin accentue devient du charabia"
        );

        // ⛔ **LA LISTE FINIT PAR DEUX ZÉROS** : c'est ce qui la termine.
        assert_eq!(&bloc[bloc.len() - 2..], &[0u8, 0u8], "double zero final manquant");

        // Et elle se relit à l'identique — c'est le va-et-vient qui compte.
        assert_eq!(depuis_bloc(&bloc), chemins);
    }

    /// ⛔ **UN SEUL CHEMIN AUSSI — ET C'EST LE CAS PIÈGE.** La liste d'un seul
    /// élément se termine par le zéro DE CE CHEMIN : si l'on oubliait le zéro de
    /// fin de LISTE, le bloc serait plus court de deux octets, et paraîtrait
    /// toujours bien formé. On exige donc la TAILLE EXACTE.
    #[test]
    fn le_bloc_d_un_seul_chemin_a_la_taille_exacte() {
        let chemin = PathBuf::from(r"C:\un.txt");
        let caracteres = chemin.as_os_str().to_string_lossy().encode_utf16().count();
        let bloc = bloc_hdrop(std::slice::from_ref(&chemin));
        // en-tête + (caractères + 1 terminateur de chemin) × 2 octets + 2 octets de fin
        assert_eq!(bloc.len(), TAILLE_ENTETE + (caracteres + 1) * 2 + 2);
        assert_eq!(depuis_bloc(&bloc), vec![chemin]);
    }

    /// ⛔ **UN CHEMIN ACCENTUÉ TRAVERSE ENTIER.** C'est le défaut qu'on ne voit
    /// qu'en collant, sur un fichier au nom accentué — et à ce moment-là, le
    /// chemin collé ne désigne plus le même fichier.
    #[test]
    fn un_chemin_accentue_traverse_entier() {
        let chemin = PathBuf::from(r"C:\essai\Résumé été — 2026.txt");
        let bloc = bloc_hdrop(std::slice::from_ref(&chemin));
        assert_eq!(depuis_bloc(&bloc), vec![chemin]);
    }

    /// Un bloc vide ou tronqué ne fait PAS paniquer la relecture — il rend une
    /// liste vide. Le module ne doit jamais pouvoir tuer le dock sur un
    /// presse-papiers abîmé (le dock est compilé en `panic = "abort"`).
    #[test]
    fn un_bloc_tronque_ne_fait_pas_paniquer() {
        assert!(depuis_bloc(&[]).is_empty());
        assert!(depuis_bloc(&[0u8; 3]).is_empty());
        assert!(depuis_bloc(&[0u8; 8]).is_empty(), "un decalage hors du bloc rend une liste vide");
    }

    /// Un `CF_HDROP` sans chemin n'est PAS écrit — on ne vide pas le
    /// presse-papiers de l'utilisateur pour rien.
    #[test]
    fn une_liste_vide_n_est_pas_ecrite() {
        assert!(!ecrire(&[], false));
    }
}
