//! ⭐ **LA LISTE DES RÉSEAUX WI-FI** — idée n°4 reprise de FluidWin (`wifi.h`) :
//! savoir quels réseaux existent autour de la machine, leur force de signal, s'ils
//! sont protégés et lequel est connecté — et pouvoir s'y connecter.
//!
//! **`wlanapi`, PAS `Windows.Devices.WiFi`.** L'API WinRT de liste des réseaux
//! exige la capacité `wiFiControl`, qu'une application EMPAQUETÉE doit déclarer ;
//! un exécutable de bureau n'y a pas droit. `wlanapi` est l'API des paramètres de
//! Windows pour exactement cette tâche, et ne demande aucun privilège
//! particulier — seule la CONNEXION peut demander des droits si la stratégie du
//! système les restreint (l'échec est alors rendu, jamais avalé).
//!
//! ⚠️ **`WlanGetAvailableNetworkList` LIT LE CACHE DE WINDOWS**, il ne provoque
//! pas de balayage : la liste est celle du dernier scan du système, et elle est
//! déjà triée par Windows. `balayer` demande un nouveau scan, qui est
//! ASYNCHRONE — il faut relire la liste quelques secondes après (le popup le fait
//! à chaque rafraîchissement, voir `win32::cef_network::poll`).
//!
//! ⚠️ **UN PROFIL EST LE NOM DE LA CONNEXION, PAS LE SSID** — les deux coïncident
//! presque toujours, mais un réseau peut avoir un profil renommé. La connexion
//! passe donc par `strProfileName`, et un réseau SANS profil n'est pas connectable
//! ici : il faudrait créer un profil (donc saisir le mot de passe). Le drapeau
//! `WLAN_AVAILABLE_NETWORK_HAS_PROFILE` le dit sans ambiguïté.

use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, HANDLE};
use windows::Win32::NetworkManagement::WiFi::{
    dot11_BSS_type_infrastructure, wlan_connection_mode_profile, WlanCloseHandle, WlanConnect,
    WlanEnumInterfaces, WlanFreeMemory, WlanGetAvailableNetworkList, WlanGetProfileList, WlanOpenHandle, WlanScan,
    DOT11_SSID, WLAN_AVAILABLE_NETWORK, WLAN_AVAILABLE_NETWORK_CONNECTED, WLAN_AVAILABLE_NETWORK_LIST,
    WLAN_PROFILE_INFO_LIST,
    WLAN_AVAILABLE_NETWORK_HAS_PROFILE, WLAN_AVAILABLE_NETWORK_INCLUDE_ALL_ADHOC_PROFILES,
    WLAN_AVAILABLE_NETWORK_INCLUDE_ALL_MANUAL_HIDDEN_PROFILES, WLAN_CONNECTION_PARAMETERS,
    WLAN_INTERFACE_INFO_LIST, WLAN_INTERFACE_STATE, wlan_interface_state_connected,
};
use windows::core::{GUID, PCWSTR};

/// La version de client que `WlanOpenHandle` doit recevoir — `WLAN_CLIENT_VERSION_LONG`
/// de `wlanapi.h` (2). Ce que Windows en fait : il négocie une version ≤ celle-ci
/// dans `pdwNegotiatedVersion`, ce qui laisse la liste des réseaux disponible.
const VERSION_CLIENT_LONG: u32 = 2;

/// Un réseau Wi-Fi tel que le popup le montre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reseau {
    /// Le nom affiché (SSID). Jamais vide : un réseau caché n'a pas de nom, donc
    /// il n'est pas listé (voir `ssid_de`).
    pub ssid: String,
    /// Force du signal, 0..100, telle que Windows la rapporte.
    pub signal: u32,
    /// Le réseau demande une clé (WPA/WPA2/WPA3/entreprise…) — `false` : ouvert.
    pub securise: bool,
    /// C'est le réseau actuellement associé.
    pub connecte: bool,
    /// Le nom du PROFIL Windows, si la machine en a déjà un — c'est lui qui rend
    /// la connexion possible sans mot de passe (voir la doc de module).
    pub profil: Option<String>,
}

/// Les réseaux vus par TOUTES les interfaces Wi-Fi de la machine, triés pour
/// l'affichage (voir `trier`).
pub fn reseaux() -> Vec<Reseau> {
    etat().reseaux
}

/// ⭐ **CE QUE LE POPUP MONTRE, EN UNE LECTURE** — et surtout POURQUOI il peut
/// n'y avoir aucun réseau.
///
/// ⛔ **WINDOWS PEUT REFUSER LA LISTE ENTIÈRE (code 5, `ERROR_ACCESS_DENIED`).**
/// Depuis Windows 10, la liste des SSID est considérée comme une donnée de
/// LOCALISATION : quand la localisation est désactivée — par l'utilisateur ou par
/// une stratégie d'entreprise (`DisableLocation=1`) — `WlanGetAvailableNetworkList`
/// et même la connexion courante rendent `ACCESS_DENIED`, à toute application,
/// y compris `netsh`. Mesuré sur cette machine le 2026-11-26 : stratégie
/// `DisableLocation=1`, `LocationEnabled=0`, et `netsh wlan show networks` qui
/// renvoie vers `ms-settings:privacy-location`.
///
/// La réponse n'est donc PAS « aucun réseau » : c'est « Windows refuse la
/// liste », et le popup le dit (voir `localisation_refusee`). Les PROFILS
/// enregistrés, eux, restent lisibles et connectables : ils sont stockés sur la
/// machine.
pub fn etat() -> EtatWifi {
    let mut etat_wifi = EtatWifi { reseaux: Vec::new(), profils: Vec::new(), localisation_refusee: false };
    with_handle(|handle| {
        for interface in interfaces(handle) {
            let (reseaux, code) = reseaux_de(handle, &interface);
            etat_wifi.reseaux.extend(reseaux);
            // Le refus vient de la LISTE ; un profil reste lisible même quand
            // elle est refusée (mesuré : profils=0, liste=5 sur la même machine).
            if code == ERROR_ACCESS_DENIED.0 {
                etat_wifi.localisation_refusee = true;
            }
            etat_wifi.profils.extend(profils_de(handle, &interface));
        }
    });
    trier(&mut etat_wifi.reseaux);
    etat_wifi.profils.sort_by_key(|p| p.to_lowercase());
    etat_wifi
}

/// L'état Wi-Fi complet — voir `etat`, doc de fonction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EtatWifi {
    /// Les réseaux à portée (vide si `localisation_refusee`).
    pub reseaux: Vec<Reseau>,
    /// Les noms de PROFIL enregistrés sur la machine — lisibles sans
    /// localisation (voir `etat`).
    pub profils: Vec<String>,
    /// Windows a REFUSÉ la liste (localisation désactivée) — voir `etat`.
    pub localisation_refusee: bool,
}

/// Demande un nouveau balayage (asynchrone — voir la doc de module). Best-effort :
/// Windows peut refuser si un balayage est déjà en cours, c'est sans conséquence.
pub fn balayer() {
    with_handle(|handle| {
        for interface in interfaces(handle) {
            unsafe {
                let _ = WlanScan(handle, &interface, None, None, None);
            }
        }
    });
}

/// Se connecte au PROFIL `profil` (voir `Reseau::profil`), sur la première
/// interface connectée, sinon sur la première interface disponible.
///
/// Rend `false` si Windows refuse — le paramètre vient de la liste qu'on vient de
/// lire, donc un refus est un vrai échec (droits, réseau parti), pas une erreur
/// de programmation.
pub fn connecter(profil: &str) -> bool {
    let nom: Vec<u16> = profil.encode_utf16().chain(std::iter::once(0)).collect();
    let Some(interface) = interface_pour_connecter() else { return false };
    with_handle(|handle| unsafe {
        let parametres = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_profile,
            strProfile: PCWSTR(nom.as_ptr()),
            // Aucun SSID imposé : le profil connaît déjà la connexion visée, et
            // c'est la forme que documente `WlanConnect` pour ce mode.
            pDot11Ssid: std::ptr::null_mut(),
            pDesiredBssidList: std::ptr::null_mut(),
            dot11BssType: dot11_BSS_type_infrastructure,
            dwFlags: 0,
        };
        WlanConnect(handle, &interface, &parametres, None) == 0
    })
    .unwrap_or(false)
}

/// La première interface Wi-Fi CONNECTÉE, sinon la première disponible.
///
/// ⚠️ **PRÉFÉRER CELLE QUI EST CONNECTÉE** : sur une machine à deux cartes
/// (Wi-Fi intégré + clé USB), `WlanConnect` sur la carte muette réussirait sans
/// que rien ne change à l'écran — la liste lirait toujours le même réseau.
fn interface_pour_connecter() -> Option<GUID> {
    let mut choisi = None;
    with_handle(|handle| {
        let interfaces = etats(handle);
        choisi = interfaces
            .iter()
            .find(|(_, etat)| *etat == wlan_interface_state_connected)
            .map(|(guid, _)| *guid)
            .or_else(|| interfaces.first().map(|(guid, _)| *guid));
    });
    choisi
}

// ---------------------------------------------------------------------------
// Le reste : ouverture du handle, énumération, décodage
// ---------------------------------------------------------------------------

thread_local! {
    /// **LE HANDLE EST GARDÉ, PAS ROUVERT À CHAQUE LECTURE** — mesuré le
    /// 2026-11-26 : ouvrir/fermer à chaque appel coûtait ~9 ms par lecture, alors
    /// que le popup relit toutes les deux secondes. Le handle vit pour le fil
    /// (celui du rendu) et meurt avec le processus : `WlanCloseHandle` à la
    /// sortie n'apporterait rien, le système libère déjà.
    static HANDLE_WLAN: std::cell::RefCell<Option<HANDLE>> = const { std::cell::RefCell::new(None) };
}

/// Exécute `action` avec le handle `wlanapi` du fil, ouvert au premier usage
/// (voir `HANDLE_WLAN`). `None` si Windows refuse l'ouverture (service arrêté).
///
/// ⚠️ **NE PAS APPELER `with_handle` DEPUIS `action`** : le handle est emprunté
/// pendant toute la durée de l'action, et un emprunt imbriqué paniquerait.
fn with_handle<T>(action: impl FnOnce(HANDLE) -> T) -> Option<T> {
    HANDLE_WLAN.with(|cell| {
        let mut cell = cell.borrow_mut();
        if cell.is_none() {
            let mut handle = HANDLE::default();
            let mut version_negociee = 0u32;
            let code = unsafe { WlanOpenHandle(VERSION_CLIENT_LONG, None, &mut version_negociee, &mut handle) };
            if code != 0 {
                return None;
            }
            *cell = Some(handle);
        }
        (*cell).map(action)
    })
}

/// Les GUID des interfaces Wi-Fi.
fn interfaces(handle: HANDLE) -> Vec<GUID> {
    unsafe {
        let mut liste: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        if WlanEnumInterfaces(handle, None, &mut liste) != 0 || liste.is_null() {
            return Vec::new();
        }
        let nombre = (*liste).dwNumberOfItems as usize;
        // `InterfaceInfo` est déclaré comme un tableau de UN élément : la liste
        // réelle est contiguë après lui (motif habituel des listes `wlanapi`).
        let premiere = (*liste).InterfaceInfo.as_ptr();
        let guides = (0..nombre).map(|i| (*premiere.add(i)).InterfaceGuid).collect();
        WlanFreeMemory(liste as *const _);
        guides
    }
}

/// Les couples (GUID, état) des interfaces.
fn etats(handle: HANDLE) -> Vec<(GUID, WLAN_INTERFACE_STATE)> {
    unsafe {
        let mut liste: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        if WlanEnumInterfaces(handle, None, &mut liste) != 0 || liste.is_null() {
            return Vec::new();
        }
        let nombre = (*liste).dwNumberOfItems as usize;
        let premiere = (*liste).InterfaceInfo.as_ptr();
        let etats = (0..nombre)
            .map(|i| {
                let info = &*premiere.add(i);
                (info.InterfaceGuid, info.isState)
            })
            .collect();
        WlanFreeMemory(liste as *const _);
        etats
    }
}

/// Les réseaux d'UNE interface, y compris les profils connus hors de portée
/// (voir les deux drapeaux), AVEC le code rendu par Windows — c'est lui qui dit
/// « refusé » (`5`) plutôt que « rien à portée » (`0`).
fn reseaux_de(handle: HANDLE, interface: &GUID) -> (Vec<Reseau>, u32) {
    unsafe {
        let mut liste: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
        let drapeaux = WLAN_AVAILABLE_NETWORK_INCLUDE_ALL_ADHOC_PROFILES
            | WLAN_AVAILABLE_NETWORK_INCLUDE_ALL_MANUAL_HIDDEN_PROFILES;
        let code = WlanGetAvailableNetworkList(handle, interface, drapeaux, None, &mut liste);
        if code != 0 || liste.is_null() {
            return (Vec::new(), code);
        }
        let nombre = (*liste).dwNumberOfItems as usize;
        let premiere = (*liste).Network.as_ptr();
        let mut reseaux = Vec::with_capacity(nombre);
        for i in 0..nombre {
            let brut: &WLAN_AVAILABLE_NETWORK = &*premiere.add(i);
            let Some(ssid) = ssid_de(&brut.dot11Ssid) else { continue };
            reseaux.push(Reseau {
                ssid,
                signal: brut.wlanSignalQuality.min(100),
                securise: brut.bSecurityEnabled.as_bool(),
                connecte: brut.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0,
                profil: nom_de_profil(&brut.strProfileName)
                    .filter(|_| brut.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0),
            });
        }
        WlanFreeMemory(liste as *const _);
        (reseaux, 0)
    }
}

/// Les noms de PROFIL enregistrés pour `interface` — `WlanGetProfileList`, qui
/// reste lisible quand la liste des réseaux est refusée (voir `etat`).
fn profils_de(handle: HANDLE, interface: &GUID) -> Vec<String> {
    unsafe {
        let mut liste: *mut WLAN_PROFILE_INFO_LIST = std::ptr::null_mut();
        if WlanGetProfileList(handle, interface, None, &mut liste) != 0 || liste.is_null() {
            return Vec::new();
        }
        let nombre = (*liste).dwNumberOfItems as usize;
        let premiere = (*liste).ProfileInfo.as_ptr();
        let profils = (0..nombre)
            .filter_map(|i| nom_de_profil(&(*premiere.add(i)).strProfileName))
            .collect();
        WlanFreeMemory(liste as *const _);
        profils
    }
}

/// Le SSID en clair, `None` pour un réseau CACHÉ (longueur nulle).
pub fn ssid_de(ssid: &DOT11_SSID) -> Option<String> {
    let longueur = (ssid.uSSIDLength as usize).min(ssid.ucSSID.len());
    if longueur == 0 {
        return None;
    }
    // Un SSID est une suite d'OCTETS, pas forcément de l'UTF-8 : une perte est
    // acceptable à l'affichage, un refus ne le serait pas (le réseau existe).
    Some(String::from_utf8_lossy(&ssid.ucSSID[..longueur]).into_owned())
}

/// Le nom de profil depuis le tableau UTF-16 à zéro terminal, `None` s'il est vide
/// (`strProfileName` vide = pas de profil enregistré).
pub fn nom_de_profil(brut: &[u16; 256]) -> Option<String> {
    let fin = brut.iter().position(|&c| c == 0)?;
    (fin > 0).then(|| String::from_utf16_lossy(&brut[..fin]))
}

/// **PURE** — l'ordre d'affichage : le réseau CONNECTÉ d'abord, puis par signal
/// décroissant, puis par nom. Deux réseaux de même nom (deux bandes) restent
/// deux lignes, mais dans un ordre stable.
pub fn trier(reseaux: &mut [Reseau]) {
    reseaux.sort_by(|a, b| {
        b.connecte
            .cmp(&a.connecte)
            .then(b.signal.cmp(&a.signal))
            .then(a.ssid.to_lowercase().cmp(&b.ssid.to_lowercase()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reseau(ssid: &str, signal: u32, connecte: bool) -> Reseau {
        Reseau { ssid: ssid.into(), signal, securise: true, connecte, profil: Some(ssid.into()) }
    }

    fn dot11(octets: &[u8]) -> DOT11_SSID {
        let mut ssid = DOT11_SSID::default();
        ssid.uSSIDLength = octets.len() as u32;
        ssid.ucSSID[..octets.len()].copy_from_slice(octets);
        ssid
    }

    /// ⛔ **UN RÉSEAU CACHÉ N'EST PAS LISTÉ** : `uSSIDLength == 0` rend une case
    /// sans nom, que l'utilisateur ne pourrait pas distinguer d'une autre.
    #[test]
    fn un_ssid_de_longueur_nulle_est_ecarte() {
        assert_eq!(ssid_de(&dot11(&[])), None);
        assert_eq!(ssid_de(&dot11(b"MonReseau")), Some("MonReseau".into()));
    }

    /// La longueur fait foi, jamais le zéro terminal : un SSID peut légitimement
    /// contenir un octet nul (rare, mais autorisé).
    #[test]
    fn le_ssid_s_arrete_a_sa_longueur() {
        let ssid = dot11(b"ab\0cd");
        assert_eq!(ssid_de(&ssid), Some("ab\0cd".into()));
    }

    /// Un profil vide n'est PAS un profil — et c'est lui qui interdit la
    /// connexion (voir la doc de module).
    #[test]
    fn un_profil_vide_n_existe_pas() {
        let mut vide = [0u16; 256];
        assert_eq!(nom_de_profil(&vide), None);
        vide[..3].copy_from_slice(&[b'W' as u16, b'i' as u16, b'F' as u16]);
        assert_eq!(nom_de_profil(&vide), Some("WiF".into()));
    }

    /// Le tri est celui de l'affichage : connecté, puis signal, puis nom — et il
    /// est STABLE pour deux réseaux de même nom.
    #[test]
    fn le_tri_met_le_connecte_devant_puis_le_signal() {
        let mut liste = vec![
            reseau("zebre", 30, false),
            reseau("Alpha", 80, false),
            reseau("celui", 10, true),
            reseau("alpha", 80, false),
        ];
        trier(&mut liste);
        assert_eq!(liste.iter().map(|r| r.ssid.as_str()).collect::<Vec<_>>(), vec!["celui", "Alpha", "alpha", "zebre"]);
    }

    /// Diagnostic : les codes rendus par `wlanapi`, interface par interface.
    /// Ignoré — c'est l'outil qui répond quand « la liste est vide » sans dire
    /// pourquoi (service arrêté, radio éteinte, balayage jamais fait…).
    #[test]
    #[ignore = "diagnostic : imprime les codes de wlanapi"]
    fn essai_reel_diagnostic_wlan() {
        let mut handle = HANDLE::default();
        let mut version = 0u32;
        let code = unsafe { WlanOpenHandle(VERSION_CLIENT_LONG, None, &mut version, &mut handle) };
        println!("[wlan] WlanOpenHandle={code} version_negociee={version}");
        if code != 0 {
            return;
        }
        for guid in interfaces(handle) {
            unsafe {
                let mut liste: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
                let code = WlanGetAvailableNetworkList(
                    handle,
                    &guid,
                    WLAN_AVAILABLE_NETWORK_INCLUDE_ALL_ADHOC_PROFILES | WLAN_AVAILABLE_NETWORK_INCLUDE_ALL_MANUAL_HIDDEN_PROFILES,
                    None,
                    &mut liste,
                );
                println!(
                    "[wlan] {guid:?} : GetAvailableNetworkList={code} nul={} elements={}",
                    liste.is_null(),
                    if liste.is_null() { 0 } else { (*liste).dwNumberOfItems }
                );
                if !liste.is_null() {
                    WlanFreeMemory(liste as *const _);
                }
            }
        }
        // Deux autres lectures, pour cerner ce qui est permis sans localisation :
        // les profils (privés) et la connexion courante.
        for guid in interfaces(handle) {
            unsafe {
                let mut profils: *mut windows::Win32::NetworkManagement::WiFi::WLAN_PROFILE_INFO_LIST = std::ptr::null_mut();
                let code = windows::Win32::NetworkManagement::WiFi::WlanGetProfileList(handle, &guid, None, &mut profils);
                println!("[wlan] profils={code} nul={}", profils.is_null());
                if !profils.is_null() {
                    WlanFreeMemory(profils as *const _);
                }
                let mut taille = 0u32;
                let mut donnees: *mut core::ffi::c_void = std::ptr::null_mut();
                let code = windows::Win32::NetworkManagement::WiFi::WlanQueryInterface(
                    handle,
                    &guid,
                    windows::Win32::NetworkManagement::WiFi::wlan_intf_opcode_current_connection,
                    None,
                    &mut taille,
                    &mut donnees,
                    None,
                );
                println!("[wlan] connexion_courante={code} (0 = connecté, 5 = refusé, 1062 = non connecté)");
                if !donnees.is_null() {
                    WlanFreeMemory(donnees as *const _);
                }
            }
        }
        unsafe {
            let _ = WlanCloseHandle(handle, None);
        }
    }

    /// Relevé réel : ce que la machine voit MAINTENANT, et le coût de la lecture.
    /// Ignoré par défaut — dépend de l'environnement radio.
    #[test]
    #[ignore = "essai réel : dépend des réseaux à portée"]
    fn essai_reel_liste_des_reseaux() {
        // ⚠️ TROIS lectures : la première paie le chargement de `wlanapi` et
        // l'ouverture du handle ; ce sont les suivantes qui disent ce que coûte
        // un rafraîchissement PÉRIODIQUE (le popup ne relit pas à chaque frame).
        let mut durees = Vec::new();
        let mut etat_wifi = EtatWifi::default();
        for _ in 0..3 {
            let depart = std::time::Instant::now();
            etat_wifi = etat();
            let duree = depart.elapsed();
            durees.push(duree);
            println!("[wifi] lecture en {:.2} ms", duree.as_secs_f64() * 1000.0);
        }
        let duree = *durees.last().expect("trois lectures");
        let liste = &etat_wifi.reseaux[..];
        println!(
            "[wifi] localisation refusée : {} — {} profil(s) : {:?}",
            etat_wifi.localisation_refusee, etat_wifi.profils.len(), etat_wifi.profils
        );
        for r in liste {
            println!(
                "{}{:<28} {:>3} %  {}  profil={:?}",
                if r.connecte { "✓ " } else { "  " },
                r.ssid,
                r.signal,
                if r.securise { "protégé" } else { "ouvert " },
                r.profil
            );
        }
        println!("[wifi] {} réseau(x), lu en {:.2} ms", liste.len(), duree.as_secs_f64() * 1000.0);
        // ⚠️ AUCUNE assertion sur le contenu : cette machine refuse la liste
        // (localisation désactivée par stratégie) et n'a aucun profil enregistré —
        // c'est un état légitime. Ce qui compte ici, c'est la durée.
    }
}
