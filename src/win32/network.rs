//! Bascule rapide WiFi/Bluetooth — façon panneau réseau unifié de
//! Brain_Shell (WiFi/Bluetooth/VPN), limité ici à l'état et l'activation/
//! désactivation des deux radios principales (le VPN n'a pas d'équivalent
//! générique côté Windows sans dépendre d'un client tiers spécifique).
//!
//! Utilise l'API WinRT `Windows.Devices.Radios` — la même que le volet
//! rapide de Windows (Win+A) pour ces mêmes bascules : documentée,
//! contrairement au changement de périphérique audio par défaut (voir
//! `win32::audio`), donc pas de risque de rupture d'une version de Windows
//! à l'autre.

use windows::Devices::Radios::{Radio, RadioKind, RadioState};

/// `true`/`false` = radio allumée/éteinte, `None` si aucune radio de ce
/// type n'est présente sur la machine (typique d'un PC de bureau sans
/// carte WiFi/Bluetooth) ou si l'API échoue.
pub fn wifi_state() -> Option<bool> {
    Some(find_radio(RadioKind::WiFi)?.State().ok()? == RadioState::On)
}

pub fn bluetooth_state() -> Option<bool> {
    Some(find_radio(RadioKind::Bluetooth)?.State().ok()? == RadioState::On)
}

/// Bascule l'état de la radio WiFi. Best-effort silencieux si aucune radio
/// WiFi n'est présente ou si Windows refuse le changement (l'utilisateur
/// peut avoir désactivé la permission d'accès aux radios pour les
/// applications dans Confidentialité > Radios sans fil).
pub fn toggle_wifi() {
    toggle_radio(RadioKind::WiFi);
}

pub fn toggle_bluetooth() {
    toggle_radio(RadioKind::Bluetooth);
}

/// La page « Réseau et Internet » des Paramètres — URI `ms-settings:`
/// documenté par Microsoft (« Launch the Windows Settings app »).
const PARAMETRES_URI: &str = "ms-settings:network";

/// Ouvre les paramètres réseau de Windows — DEMANDE EXPLICITE (2026-09-26) :
/// « au clic droit sur l'applet réseau je voudrais ouvrir les paramètres
/// réseau de windows ». Même voie que `windows_update::ouvrir` : l'URI confié
/// à `explorer.exe`, échec journalisé plutôt que muet.
pub fn ouvrir_parametres() {
    crate::diag!("réseau : ouverture de {PARAMETRES_URI}");
    if let Err(err) = std::process::Command::new("explorer.exe").arg(PARAMETRES_URI).spawn() {
        crate::diag!("réseau : lancement impossible — {err}");
    }
}

fn find_radio(kind: RadioKind) -> Option<Radio> {
    let radios = Radio::GetRadiosAsync().ok()?.join().ok()?;
    let size = radios.Size().ok()?;
    (0..size).filter_map(|i| radios.GetAt(i).ok()).find(|radio| radio.Kind().ok() == Some(kind))
}

fn toggle_radio(kind: RadioKind) {
    let Some(radio) = find_radio(kind) else { return };
    let Ok(current) = radio.State() else { return };
    let target = if current == RadioState::On { RadioState::Off } else { RadioState::On };
    if let Ok(op) = radio.SetStateAsync(target) {
        // Bloque brièvement jusqu'à confirmation de Windows — cohérent
        // avec le reste de l'appli (menus contextuels bloquants, voir
        // `win32::menu`), le changement d'état d'une radio est quasi
        // instantané en pratique.
        let _ = op.join();
    }
}
