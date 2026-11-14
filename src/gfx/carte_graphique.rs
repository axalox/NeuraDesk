//! **LE CHOIX DE LA CARTE GRAPHIQUE** — DEMANDE EXPLICITE (2026-11-14) :
//! « ajoute un choix de carte graphique dans les réglages ».
//!
//! Sans choix, rien ne change : `D3D11CreateDevice` reçoit `None` et Windows
//! désigne la carte (Paramètres › Système › Écran › Graphiques, sinon le pilote).
//! Avec un choix, la carte est donnée EXPLICITEMENT à Direct3D (`GfxDevice`)
//! ET à Chromium (`--use-adapter-luid`, voir `win32::cef_app`).
//!
//! ⛔ **LES DEUX, OU AUCUN** : les pages CEF arrivent en TEXTURES PARTAGÉES
//! (`gfx::cef_texture::import_shared_texture`), et une texture partagée ne
//! s'ouvre que sur la carte qui l'a créée. Un Direct3D sur la RTX et un
//! Chromium sur la Radeon intégrée donneraient des panneaux VIDES. C'est
//! pourquoi la carte retenue est résolue UNE fois par processus (`choisie`),
//! et que les deux côtés lisent la même.
//!
//! ⚠️ **ON MÉMORISE L'IDENTITÉ, PAS LE LUID** : le LUID d'une carte change à
//! chaque démarrage de Windows. L'identité `fabricant&modèle&sous-système` (en
//! hexadécimal, le format même de Windows dans
//! `HKCU\Software\Microsoft\DirectX\UserGpuPreferences`, `SpecificAdapter=`)
//! reste. Deux cartes STRICTEMENT identiques ne se distinguent pas ainsi : la
//! première trouvée l'emporte.
//!
//! ⚠️ **PRIS EN COMPTE AU PROCHAIN LANCEMENT** : le device et le processus GPU
//! de Chromium naissent au démarrage ; les recréer à chaud toucherait chaque
//! texture de l'application.

use std::sync::OnceLock;

use windows::core::{Interface, Result};
use windows::Win32::Graphics::Direct3D11::ID3D11Device;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter1, IDXGIDevice, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
};

/// La valeur de config et de la page pour « laisser Windows choisir ».
pub const WINDOWS: &str = "windows";

/// Une carte MATÉRIELLE (les cartes logicielles, « Microsoft Basic Render
/// Driver », sont écartées : y forcer le rendu gèlerait la machine, voir la
/// fiche « rendu logiciel forcé »).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carte {
    /// `fabricant&modèle&sous-système`, voir `identifiant`.
    pub id: String,
    pub nom: String,
    pub luid_haut: i32,
    pub luid_bas: u32,
}

/// L'identité stable d'une carte, au format de Windows : `10DE&2C02&53101462`.
pub fn identifiant(fabricant: u32, modele: u32, sous_systeme: u32) -> String {
    format!("{fabricant:04X}&{modele:04X}&{sous_systeme:08X}")
}

/// La valeur de `--use-adapter-luid` : `<haut>,<bas>` en DÉCIMAL — c'est ce
/// que lit Chromium (`ui/gl/gl_display.cc`, `StringToInt`/`StringToUint`) ; un
/// `0x` ferait échouer l'affichage D3D11 de Chromium au lieu de l'ignorer.
pub fn valeur_cef(carte: &Carte) -> String {
    format!("{},{}", carte.luid_haut, carte.luid_bas)
}

/// La carte d'identité `id` dans `cartes`, ou `None` (choix « Windows », ou
/// carte retirée depuis — on retombe alors sur le choix de Windows plutôt que
/// de refuser de démarrer).
pub fn trouver<'a>(cartes: &'a [Carte], id: &str) -> Option<&'a Carte> {
    if id == WINDOWS {
        return None;
    }
    cartes.iter().find(|c| c.id.eq_ignore_ascii_case(id))
}

/// Une identité acceptable venue de la page ou du disque : `WINDOWS`, ou
/// `XXXX&XXXX&XXXXXXXX` en hexadécimal. Tout le reste est refusé — la page
/// peut envoyer n'importe quelle chaîne.
pub fn id_valide(id: &str) -> bool {
    if id == WINDOWS {
        return true;
    }
    let parties: Vec<&str> = id.split('&').collect();
    parties.len() == 3
        && parties.iter().zip([4, 4, 8]).all(|(p, n)| p.len() == n && p.chars().all(|c| c.is_ascii_hexdigit()))
}

fn nom_de(description: &[u16]) -> String {
    let long = description.iter().position(|&c| c == 0).unwrap_or(description.len());
    String::from_utf16_lossy(&description[..long]).trim().to_string()
}

/// Les cartes matérielles, dans l'ordre de DXGI (la première est celle que
/// Windows donne par défaut à NeuraDesk), sans doublon de LUID.
fn enumerer() -> Vec<(Carte, IDXGIAdapter1)> {
    let mut cartes: Vec<(Carte, IDXGIAdapter1)> = Vec::new();
    // SAFETY : sans précondition.
    let Ok(fabrique) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return cartes };
    for rang in 0.. {
        // SAFETY : la fabrique est vivante ; `Err` marque la fin de la liste.
        let Ok(adaptateur) = (unsafe { fabrique.EnumAdapters1(rang) }) else { break };
        // SAFETY : interface valide.
        let Ok(d) = (unsafe { adaptateur.GetDesc1() }) else { continue };
        if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let carte = Carte {
            id: identifiant(d.VendorId, d.DeviceId, d.SubSysId),
            nom: nom_de(&d.Description),
            luid_haut: d.AdapterLuid.HighPart,
            luid_bas: d.AdapterLuid.LowPart,
        };
        if !cartes.iter().any(|(c, _)| c.luid_haut == carte.luid_haut && c.luid_bas == carte.luid_bas) {
            cartes.push((carte, adaptateur));
        }
    }
    cartes
}

/// Les cartes, relevées une fois par processus (une carte ne s'ajoute pas en
/// cours de route, et la page des Réglages relit cette liste souvent).
pub fn cartes() -> &'static [Carte] {
    static CARTES: OnceLock<Vec<Carte>> = OnceLock::new();
    CARTES.get_or_init(|| enumerer().into_iter().map(|(c, _)| c).collect())
}

/// Le choix lu sur le disque AU LANCEMENT — figé pour tout le processus, pour
/// que Direct3D et Chromium lisent la même carte même si l'utilisateur change
/// le réglage entre-temps (il ne vaudra qu'au prochain lancement).
pub fn choix_au_lancement() -> &'static str {
    static CHOIX: OnceLock<String> = OnceLock::new();
    CHOIX.get_or_init(|| crate::config::load_carte_graphique().unwrap_or_else(|| WINDOWS.to_string()))
}

/// La carte imposée pour ce processus, ou `None` (Windows choisit).
pub fn choisie() -> Option<&'static Carte> {
    trouver(cartes(), choix_au_lancement())
}

/// L'adaptateur DXGI de la carte imposée, à passer à `D3D11CreateDevice`.
pub fn adaptateur_choisi() -> Option<IDXGIAdapter1> {
    let carte = choisie()?;
    enumerer()
        .into_iter()
        .find(|(c, _)| c.luid_haut == carte.luid_haut && c.luid_bas == carte.luid_bas)
        .map(|(_, a)| a)
}

/// Le nom de la carte sur laquelle `device` tourne VRAIMENT — affiché dans les
/// Réglages, pour qu'on puisse vérifier le choix sans le Gestionnaire des tâches.
pub fn nom_du_device(device: &ID3D11Device) -> Result<String> {
    let dxgi: IDXGIDevice = device.cast()?;
    // SAFETY : interfaces valides.
    let desc = unsafe { dxgi.GetAdapter()?.GetDesc()? };
    Ok(nom_de(&desc.Description))
}

static CARTE_EFFECTIVE: OnceLock<String> = OnceLock::new();

/// Note la carte du device PRINCIPAL (appelé une fois par `app::run`).
pub fn noter_effective(device: &ID3D11Device) {
    if let Ok(nom) = nom_du_device(device) {
        let _ = CARTE_EFFECTIVE.set(nom);
    }
}

pub fn effective() -> Option<&'static str> {
    CARTE_EFFECTIVE.get().map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carte(id: &str, haut: i32, bas: u32) -> Carte {
        Carte { id: id.into(), nom: id.into(), luid_haut: haut, luid_bas: bas }
    }

    #[test]
    fn l_identifiant_suit_le_format_de_windows() {
        assert_eq!(identifiant(0x10DE, 0x2C02, 0x5310_1462), "10DE&2C02&53101462");
        assert_eq!(identifiant(0x1002, 0x13C0, 0xD0001458), "1002&13C0&D0001458");
        assert_eq!(identifiant(0x8086, 0x1, 0x2), "8086&0001&00000002");
    }

    #[test]
    fn la_valeur_cef_est_decimale_haut_virgule_bas() {
        assert_eq!(valeur_cef(&carte("x", 0, 0x0001_A2B3)), "0,107187");
        assert_eq!(valeur_cef(&carte("x", -1, u32::MAX)), "-1,4294967295");
    }

    #[test]
    fn windows_ou_une_carte_absente_ne_designent_rien() {
        let cartes = [carte("10DE&2C02&53101462", 0, 1), carte("1002&13C0&D0001458", 0, 2)];
        assert_eq!(trouver(&cartes, WINDOWS), None);
        assert_eq!(trouver(&cartes, "1002&73FF&00000000"), None);
        assert_eq!(trouver(&cartes, "1002&13c0&d0001458").map(|c| c.luid_bas), Some(2));
    }

    #[test]
    fn seules_les_identites_bien_formees_sont_acceptees() {
        assert!(id_valide(WINDOWS));
        assert!(id_valide("10DE&2C02&53101462"));
        for faux in ["", "10DE&2C02", "10DE&2C02&5310146", "10DE&2C02&53101462&0", "ZZZZ&2C02&53101462", "../x"] {
            assert!(!id_valide(faux), "{faux:?} accepté");
        }
    }
}
