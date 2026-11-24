//! **LA CONNEXION D'UNE CARTE** — onglet « Connexion » de l'applet « Modifier
//! réglages réseau » (voir `win32::cef_dns`, `win32::dns`) : réseau public ou
//! privé, connexion limitée, renouvellement de l'adresse DHCP.
//!
//! DEMANDE (2026-11-24) : « peux tu ajouter ces réglages dans la fenêtre de
//! l'applet ».
//!
//! - **Lire** ne demande aucun droit et ne lance RIEN : le gestionnaire des
//!   réseaux (`INetworkListManager`, COM) donne, par carte (GUID), la catégorie
//!   et le coût de la connexion. ⛔ `relire` tourne sur le fil de l'interface à
//!   chaque ouverture : jamais de PowerShell pour lire (~1 s).
//! - **Changer** passe par le PowerShell élevé de `dns::appliquer` :
//!   - catégorie : `Set-NetConnectionProfile` (un réseau de DOMAINE ne se change
//!     pas : refusé ici) ;
//!   - connexion limitée, câble : `DusmSvc\Profiles\{guid}\*` `UserCost` = 2
//!     (0 pour illimitée) puis redémarrage du service DusmSvc ; ⛔ le `*` est le
//!     NOM de la clé (écrit par .NET : un chemin PowerShell le lirait comme un
//!     joker) ;
//!   - connexion limitée, Wi-Fi : le coût du PROFIL Wi-Fi
//!     (`netsh wlan set profileparameter … cost=Fixed|Unrestricted`), dont le
//!     nom est lu DANS le script (jamais venu de la page) ;
//!   - renouveler : `ipconfig /renew` sur le nom de la carte lu dans le script.
//!
//! Sources des clés : Microsoft Q&A « Configure GPO to turn off metered network
//! connection on Ethernet » ; franckrichard.blogspot.com (2018,
//! SetMeteredEthernetConnection.ps1).

use windows::Win32::Networking::NetworkListManager::{
    INetworkConnectionCost, INetworkListManager, NetworkListManager, NLM_CONNECTION_COST_FIXED, NLM_CONNECTION_COST_VARIABLE,
    NLM_NETWORK_CATEGORY_DOMAIN_AUTHENTICATED, NLM_NETWORK_CATEGORY_PRIVATE, NLM_NETWORK_CATEGORY_PUBLIC,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED};

/// `IF_TYPE_ETHERNET_CSMACD` et `IF_TYPE_IEEE80211` : les deux sortes de carte
/// dont on sait régler le coût.
pub const IF_TYPE_ETHERNET: u32 = 6;
pub const IF_TYPE_WIFI: u32 = 71;

/// La catégorie d'un réseau, telle que Windows la donne.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Categorie {
    Public,
    Prive,
    Domaine,
}

impl Categorie {
    pub fn code(self) -> &'static str {
        match self {
            Categorie::Public => "public",
            Categorie::Prive => "prive",
            Categorie::Domaine => "domaine",
        }
    }
}

/// Ce que le gestionnaire des réseaux dit d'une carte.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EtatConnexion {
    pub categorie: Option<Categorie>,
    /// `true` : connexion limitée (coût fixe ou variable).
    pub limitee: bool,
}

/// L'état connu de chaque carte connectée, par GUID (`{…}` en majuscules).
/// Une erreur COM rend une liste vide : la page montre « — ».
pub fn etats() -> Vec<(String, EtatConnexion)> {
    let mut etats = Vec::new();
    // Déjà fait sur le fil de l'interface (`main`) ; ailleurs (sonde, fil de
    // travail) il manquerait et `CoCreateInstance` échouerait en silence.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let Ok(nlm) = (unsafe { CoCreateInstance::<_, INetworkListManager>(&NetworkListManager, None, CLSCTX_ALL) }) else {
        return etats;
    };
    let Ok(connexions) = (unsafe { nlm.GetNetworkConnections() }) else {
        return etats;
    };
    loop {
        let mut une = [None];
        let mut lues = 0u32;
        if unsafe { connexions.Next(&mut une, Some(&mut lues)) }.is_err() || lues == 0 {
            break;
        }
        let Some(connexion) = une[0].take() else { break };
        let Ok(guid) = (unsafe { connexion.GetAdapterId() }) else { continue };
        let categorie = unsafe { connexion.GetNetwork().and_then(|n| n.GetCategory()) }.ok().and_then(|c| match c {
            NLM_NETWORK_CATEGORY_PUBLIC => Some(Categorie::Public),
            NLM_NETWORK_CATEGORY_PRIVATE => Some(Categorie::Prive),
            NLM_NETWORK_CATEGORY_DOMAIN_AUTHENTICATED => Some(Categorie::Domaine),
            _ => None,
        });
        let cout = windows_core::Interface::cast::<INetworkConnectionCost>(&connexion).and_then(|c| unsafe { c.GetCost() }).unwrap_or(0);
        let limitee = cout & (NLM_CONNECTION_COST_FIXED.0 as u32 | NLM_CONNECTION_COST_VARIABLE.0 as u32) != 0;
        etats.push((format!("{{{guid:?}}}"), EtatConnexion { categorie, limitee }));
    }
    etats
}

/// Ce qu'on demande pour la connexion d'UNE carte.
#[derive(Clone, Debug, PartialEq)]
pub enum DemandeConnexion {
    /// Réseau privé (`true`) ou public.
    Categorie { index: u32, prive: bool },
    /// Connexion limitée ou non. `guid` sert au câble, l'index au Wi-Fi.
    Limitee { index: u32, guid: String, wifi: bool, limitee: bool },
    /// Redemander une adresse au DHCP.
    Renouveler { index: u32 },
}

/// Ce que `lire_demande` doit savoir d'une carte connue.
pub struct CarteConnue<'a> {
    pub index: u32,
    pub guid: &'a str,
    pub type_if: u32,
    pub categorie: Option<Categorie>,
    pub dhcp: bool,
}

/// **LIRE CE QUE LA PAGE DEMANDE** : `quoi` = `categorie/<prive|public>`,
/// `limitee/<oui|non>` ou `renouveler`, pour la carte `index` (relue).
pub fn lire_demande(index: &str, quoi: &str, connues: &[CarteConnue]) -> Option<DemandeConnexion> {
    let index: u32 = index.trim().parse().ok()?;
    let carte = connues.iter().find(|c| c.index == index)?;
    match quoi.split_once('/').unwrap_or((quoi, "")) {
        ("categorie", valeur) => {
            // Un réseau de domaine est réglé par l'administrateur du domaine.
            if carte.categorie == Some(Categorie::Domaine) || carte.categorie.is_none() {
                return None;
            }
            let prive = match valeur {
                "prive" => true,
                "public" => false,
                _ => return None,
            };
            Some(DemandeConnexion::Categorie { index, prive })
        }
        ("limitee", valeur) => {
            let limitee = match valeur {
                "oui" => true,
                "non" => false,
                _ => return None,
            };
            let wifi = match carte.type_if {
                IF_TYPE_WIFI => true,
                IF_TYPE_ETHERNET => false,
                _ => return None,
            };
            if !crate::win32::dns::guid_valide(carte.guid) {
                return None;
            }
            Some(DemandeConnexion::Limitee { index, guid: carte.guid.to_string(), wifi, limitee })
        }
        ("renouveler", "") => carte.dhcp.then_some(DemandeConnexion::Renouveler { index }),
        _ => None,
    }
}

/// Le script PowerShell (élevé). N'y entrent qu'un `u32`, un GUID validé et
/// des mots choisis ici.
pub fn script(demande: &DemandeConnexion) -> String {
    let action = match demande {
        DemandeConnexion::Categorie { index, prive } => {
            let categorie = if *prive { "Private" } else { "Public" };
            format!("Set-NetConnectionProfile -InterfaceIndex {index} -NetworkCategory {categorie}; ")
        }
        DemandeConnexion::Limitee { index, wifi: true, limitee, .. } => {
            let cout = if *limitee { "Fixed" } else { "Unrestricted" };
            format!(
                "$profil = (Get-NetConnectionProfile -InterfaceIndex {index}).Name; \
                 netsh wlan set profileparameter name=\"$profil\" cost={cout} | Out-Null; \
                 if ($LASTEXITCODE -ne 0) {{ exit 1 }}; "
            )
        }
        DemandeConnexion::Limitee { guid, wifi: false, limitee, .. } => {
            let cout = if *limitee { 2 } else { 0 };
            format!(
                "$k = [Microsoft.Win32.Registry]::LocalMachine.CreateSubKey('SOFTWARE\\Microsoft\\DusmSvc\\Profiles\\{guid}\\*'); \
                 $k.SetValue('UserCost', {cout}, 'DWord'); $k.Close(); \
                 Restart-Service -Name DusmSvc -Force; "
            )
        }
        DemandeConnexion::Renouveler { index } => format!(
            "$nom = (Get-NetAdapter -InterfaceIndex {index}).Name; \
             ipconfig /renew \"$nom\" | Out-Null; \
             if ($LASTEXITCODE -ne 0) {{ exit 1 }}; "
        ),
    };
    format!("$ErrorActionPreference='Stop'; try {{ {action}exit 0 }} catch {{ exit 1 }}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUID: &str = "{01234567-89AB-CDEF-0123-456789ABCDEF}";

    fn carte(type_if: u32, categorie: Option<Categorie>, dhcp: bool) -> CarteConnue<'static> {
        CarteConnue { index: 12, guid: GUID, type_if, categorie, dhcp }
    }

    #[test]
    fn une_demande_valide_est_lue() {
        let eth = [carte(IF_TYPE_ETHERNET, Some(Categorie::Public), true)];
        assert_eq!(lire_demande("12", "categorie/prive", &eth), Some(DemandeConnexion::Categorie { index: 12, prive: true }));
        assert_eq!(
            lire_demande("12", "limitee/oui", &eth),
            Some(DemandeConnexion::Limitee { index: 12, guid: GUID.into(), wifi: false, limitee: true })
        );
        assert_eq!(lire_demande("12", "renouveler", &eth), Some(DemandeConnexion::Renouveler { index: 12 }));
        let wifi = [carte(IF_TYPE_WIFI, Some(Categorie::Prive), true)];
        assert!(matches!(lire_demande("12", "limitee/non", &wifi), Some(DemandeConnexion::Limitee { wifi: true, limitee: false, .. })));
    }

    #[test]
    fn une_demande_impossible_ou_hostile_est_refusee() {
        let domaine = [carte(IF_TYPE_ETHERNET, Some(Categorie::Domaine), false)];
        assert_eq!(lire_demande("12", "categorie/public", &domaine), None, "réseau de domaine");
        assert_eq!(lire_demande("12", "renouveler", &domaine), None, "adresse fixe : rien à renouveler");
        let autre = [carte(243, Some(Categorie::Public), true)];
        assert_eq!(lire_demande("12", "limitee/oui", &autre), None, "ni câble ni Wi-Fi");
        let eth = [carte(IF_TYPE_ETHERNET, Some(Categorie::Public), true)];
        for quoi in ["categorie/Private", "categorie/prive; calc", "limitee/peut-etre", "renouveler/x", "", "vider"] {
            assert_eq!(lire_demande("12", quoi, &eth), None, "accepté à tort : {quoi:?}");
        }
        assert_eq!(lire_demande("13", "renouveler", &eth), None, "carte inconnue");
    }

    #[test]
    fn le_script_ne_porte_que_des_valeurs_choisies_ici() {
        let s = script(&DemandeConnexion::Categorie { index: 7, prive: false });
        assert!(s.contains("Set-NetConnectionProfile -InterfaceIndex 7 -NetworkCategory Public; exit 0"));
        let s = script(&DemandeConnexion::Limitee { index: 7, guid: GUID.into(), wifi: false, limitee: true });
        assert!(s.contains(&format!("CreateSubKey('SOFTWARE\\Microsoft\\DusmSvc\\Profiles\\{GUID}\\*')")));
        assert!(s.contains("SetValue('UserCost', 2, 'DWord')") && s.contains("Restart-Service -Name DusmSvc -Force"));
        let s = script(&DemandeConnexion::Limitee { index: 7, guid: GUID.into(), wifi: true, limitee: false });
        assert!(s.contains("(Get-NetConnectionProfile -InterfaceIndex 7).Name") && s.contains("cost=Unrestricted"));
        let s = script(&DemandeConnexion::Renouveler { index: 7 });
        assert!(s.contains("(Get-NetAdapter -InterfaceIndex 7).Name") && s.contains("ipconfig /renew \"$nom\""));
    }
}
