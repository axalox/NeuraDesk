//! La pseudo-console (ConPTY) NUE, et le format des trames du relais
//! administrateur — partagés par l'application (`win32::pty`) et par le relais
//! `neuradesk-admin.exe` (`src/relais_admin.rs`, qui inclut ce fichier par
//! `#[path]`). ⚠️ **Aucun `crate::` ici** : le relais est un autre binaire,
//! sans le reste de l'application.

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::Console::{ClosePseudoConsole, CreatePseudoConsole, COORD, HPCON};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList, UpdateProcThreadAttribute,
    EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

/// Les shells proposés — l'identifiant est ce que la page envoie, et ce que
/// le relais reçoit sur sa ligne de commande : jamais une commande.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shell {
    Cmd,
    PowerShell,
    Pwsh,
}

impl Shell {
    pub fn depuis_id(id: &str) -> Option<Shell> {
        match id {
            "cmd" => Some(Shell::Cmd),
            "powershell" => Some(Shell::PowerShell),
            "pwsh" => Some(Shell::Pwsh),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Shell::Cmd => "cmd",
            Shell::PowerShell => "powershell",
            Shell::Pwsh => "pwsh",
        }
    }

    pub fn nom(self) -> &'static str {
        match self {
            Shell::Cmd => "Invite de commandes",
            Shell::PowerShell => "Windows PowerShell",
            Shell::Pwsh => "PowerShell 7",
        }
    }

    pub fn ligne_de_commande(self) -> String {
        match self {
            Shell::Cmd => std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string()),
            Shell::PowerShell => "powershell.exe -NoLogo".to_string(),
            Shell::Pwsh => "pwsh.exe -NoLogo".to_string(),
        }
    }
}

/// Une poignée refermée au dernier `Drop` — partagée entre la table et les
/// fils, pour qu'une écriture en cours ne tombe jamais sur une poignée déjà
/// refermée (et peut-être réattribuée).
pub struct Poignee(pub HANDLE);
// SAFETY : une poignée de noyau s'utilise depuis n'importe quel fil.
unsafe impl Send for Poignee {}
unsafe impl Sync for Poignee {}

impl Drop for Poignee {
    fn drop(&mut self) {
        // SAFETY : poignée possédée, refermée une seule fois.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// La pseudo-console, refermée au dernier `Drop` — ce qui met fin au shell
/// s'il vit encore, et casse le tube de sortie (fin du fil lecteur).
pub struct Pseudo(pub HPCON);
// SAFETY : comme une poignée de noyau.
unsafe impl Send for Pseudo {}
unsafe impl Sync for Pseudo {}

impl Drop for Pseudo {
    fn drop(&mut self) {
        // SAFETY : pseudo-console possédée, refermée une seule fois.
        unsafe { ClosePseudoConsole(self.0) };
    }
}

pub fn taille(colonnes: u32, lignes: u32) -> COORD {
    COORD { X: colonnes.clamp(2, 1000) as i16, Y: lignes.clamp(1, 1000) as i16 }
}

fn tube() -> Result<(Poignee, Poignee), String> {
    let (mut lecture, mut ecriture) = (HANDLE::default(), HANDLE::default());
    // SAFETY : deux poignées de sortie valides, attributs par défaut.
    unsafe { CreatePipe(&mut lecture, &mut ecriture, None, 0) }.map_err(|e| format!("Tube impossible : {e}"))?;
    Ok((Poignee(lecture), Poignee(ecriture)))
}

/// Crée la pseudo-console et y lance `commande`. Rend la pseudo-console, le
/// bout ÉCRITURE du tube d'entrée, le bout LECTURE du tube de sortie et le
/// processus.
pub fn lancer(commande: &str, taille: COORD) -> Result<(Pseudo, Poignee, Poignee, Poignee), String> {
    let (entree_console, entree) = tube()?;
    let (sortie, sortie_console) = tube()?;
    // SAFETY : poignées de tube valides ; la pseudo-console les duplique.
    let hpc = unsafe { CreatePseudoConsole(taille, entree_console.0, sortie_console.0, 0) }
        .map_err(|e| format!("Pseudo-console impossible : {e}"))?;
    let pseudo = Pseudo(hpc);
    let processus = creer_processus(commande, &pseudo)?;
    // Les bouts côté console appartiennent désormais à conhost : les garder
    // ouverts ici empêcherait le tube de sortie de casser à la fin.
    drop((entree_console, sortie_console));
    Ok((pseudo, entree, sortie, processus))
}

fn creer_processus(commande: &str, pseudo: &Pseudo) -> Result<Poignee, String> {
    let mut taille_liste = 0usize;
    // SAFETY : premier appel de mesure — il « échoue » en rendant la taille.
    let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut taille_liste) };
    let mut memoire = vec![0u8; taille_liste];
    let liste = LPPROC_THREAD_ATTRIBUTE_LIST(memoire.as_mut_ptr().cast());
    // SAFETY : `memoire` a la taille demandée et survit à la liste.
    unsafe { InitializeProcThreadAttributeList(Some(liste), 1, None, &mut taille_liste) }
        .map_err(|e| format!("Liste d'attributs impossible : {e}"))?;
    let resultat = (|| {
        // SAFETY : la valeur est la HPCON elle-même (pas un pointeur vers
        // elle), comme le veut `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`.
        unsafe {
            UpdateProcThreadAttribute(
                liste,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                Some(pseudo.0 .0 as *const core::ffi::c_void),
                std::mem::size_of::<HPCON>(),
                None,
                None,
            )
        }
        .map_err(|e| format!("Attribut de pseudo-console impossible : {e}"))?;
        let mut info = STARTUPINFOEXW::default();
        info.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        // Des poignées standard NULLES, sinon le shell hériterait de celles
        // de l'application au lieu de parler à la pseudo-console.
        info.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        info.lpAttributeList = liste;
        let mut ligne: Vec<u16> = commande.encode_utf16().chain(std::iter::once(0)).collect();
        let dossier: Option<Vec<u16>> = std::env::var_os("USERPROFILE")
            .map(|d| d.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect());
        let mut processus = PROCESS_INFORMATION::default();
        // SAFETY : chaînes terminées par un zéro, structure d'appel valide.
        unsafe {
            CreateProcessW(
                PCWSTR::null(),
                Some(PWSTR(ligne.as_mut_ptr())),
                None,
                None,
                false,
                EXTENDED_STARTUPINFO_PRESENT,
                None,
                dossier.as_ref().map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr())),
                &info.StartupInfo,
                &mut processus,
            )
        }
        .map_err(|e| format!("Impossible de lancer « {commande} » : {e}"))?;
        // SAFETY : poignée de fil rendue par `CreateProcessW`, inutile ici.
        let _ = unsafe { CloseHandle(processus.hThread) };
        Ok(Poignee(processus.hProcess))
    })();
    // SAFETY : liste initialisée plus haut.
    unsafe { DeleteProcThreadAttributeList(liste) };
    resultat
}

/// Écrit TOUT `octets` ; `false` dès que la poignée refuse (tube cassé).
pub fn ecrire_tout(poignee: HANDLE, octets: &[u8]) -> bool {
    let mut reste = octets;
    while !reste.is_empty() {
        let mut ecrits = 0u32;
        // SAFETY : poignée valide pour l'appelant, tampon valide.
        if unsafe { WriteFile(poignee, Some(reste), Some(&mut ecrits), None) }.is_err() || ecrits == 0 {
            return false;
        }
        reste = &reste[ecrits as usize..];
    }
    true
}

/// Lit une fois ; `None` à la fin du tube.
pub fn lire_une_fois(poignee: HANDLE, tampon: &mut [u8]) -> Option<usize> {
    let mut lus = 0u32;
    // SAFETY : tampon et compteur valides ; lecture synchrone.
    let ok = unsafe { ReadFile(poignee, Some(tampon), Some(&mut lus), None) }.is_ok();
    (ok && lus > 0).then_some(lus as usize)
}

// ---- Les trames du relais administrateur (application → relais) ----
//
// `[genre: u8][longueur: u32 LE][charge]`. Le sens relais → application n'en a
// pas besoin : c'est la sortie brute du shell.

/// Des frappes, à écrire telles quelles dans la pseudo-console.
pub const TRAME_FRAPPES: u8 = 1;
/// Une nouvelle taille : colonnes puis lignes, en `u16` LE.
pub const TRAME_TAILLE: u8 = 2;
/// Au-delà, la trame est tenue pour corrompue — le relais ne réserve jamais
/// une mémoire dictée par une longueur lue.
#[allow(dead_code)] // lue par le relais élevé seul (`relais_admin.rs`, `#[path]`)
pub const TRAME_MAX: usize = 1 << 20;

pub fn trame(genre: u8, charge: &[u8]) -> Vec<u8> {
    let mut octets = Vec::with_capacity(5 + charge.len());
    octets.push(genre);
    octets.extend_from_slice(&(charge.len() as u32).to_le_bytes());
    octets.extend_from_slice(charge);
    octets
}

pub fn trame_taille(colonnes: u32, lignes: u32) -> Vec<u8> {
    let mut charge = Vec::with_capacity(4);
    charge.extend_from_slice(&(colonnes.min(u16::MAX as u32) as u16).to_le_bytes());
    charge.extend_from_slice(&(lignes.min(u16::MAX as u32) as u16).to_le_bytes());
    trame(TRAME_TAILLE, &charge)
}

/// Découpe un flux d'octets en trames, quels que soient les morceaux reçus.
#[derive(Default)]
#[allow(dead_code)] // le relais élevé seul la construit (`relais_admin.rs`)
pub struct Decoupeur {
    tampon: Vec<u8>,
}

#[allow(dead_code)] // `pousser` : le relais élevé seul (`relais_admin.rs`)
impl Decoupeur {
    /// Ajoute `octets` ; rend les trames complètes, ou `Err` si le flux est
    /// corrompu (longueur hors borne).
    pub fn pousser(&mut self, octets: &[u8]) -> Result<Vec<(u8, Vec<u8>)>, ()> {
        self.tampon.extend_from_slice(octets);
        let mut trames = Vec::new();
        loop {
            if self.tampon.len() < 5 {
                return Ok(trames);
            }
            let longueur = u32::from_le_bytes([self.tampon[1], self.tampon[2], self.tampon[3], self.tampon[4]]) as usize;
            if longueur > TRAME_MAX {
                return Err(());
            }
            if self.tampon.len() < 5 + longueur {
                return Ok(trames);
            }
            let genre = self.tampon[0];
            let charge = self.tampon[5..5 + longueur].to_vec();
            self.tampon.drain(..5 + longueur);
            trames.push((genre, charge));
        }
    }
}

/// Colonnes et lignes d'une charge `TRAME_TAILLE`.
#[allow(dead_code)] // appelée par le relais élevé seul (`relais_admin.rs`)
pub fn lire_taille(charge: &[u8]) -> Option<(u32, u32)> {
    (charge.len() == 4).then(|| (u16::from_le_bytes([charge[0], charge[1]]) as u32, u16::from_le_bytes([charge[2], charge[3]]) as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_trames_traversent_quel_que_soit_le_decoupage() {
        let mut flux = trame(TRAME_FRAPPES, b"dir\r");
        flux.extend(trame_taille(120, 40));
        flux.extend(trame(TRAME_FRAPPES, b""));
        let mut decoupeur = Decoupeur::default();
        let mut recues = Vec::new();
        for octet in &flux {
            recues.extend(decoupeur.pousser(std::slice::from_ref(octet)).expect("flux valide"));
        }
        assert_eq!(recues.len(), 3);
        assert_eq!(recues[0], (TRAME_FRAPPES, b"dir\r".to_vec()));
        assert_eq!(recues[1].0, TRAME_TAILLE);
        assert_eq!(lire_taille(&recues[1].1), Some((120, 40)));
        assert_eq!(recues[2], (TRAME_FRAPPES, Vec::new()));
    }

    #[test]
    fn une_longueur_demesuree_est_refusee_sans_rien_reserver() {
        let mut flux = vec![TRAME_FRAPPES];
        flux.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(Decoupeur::default().pousser(&flux).is_err());
        assert_eq!(lire_taille(&[1, 2, 3]), None);
    }
}
