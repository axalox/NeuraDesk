//! Fenêtre CEF pour l'icône Réseau — Phase 1 de
//! `docs/plans/cef-web-ui-migration.md`. Remplace `PopupKind::Network`
//! (`win32::popup_menu::render_network`) pour cette icône précise.
//!
//! Premier popup de cette migration à avoir besoin d'un canal Rust→JS (voir
//! `push_state`, `win32::cef_app::CefBrowserSurface::execute_js`) : l'état
//! Wi-Fi/Bluetooth (`win32::network::wifi_state`/`bluetooth_state`) est lu
//! côté Windows, PAS calculable en JS pur — voir `assets/popups/network.html` pour
//! le protocole complet (`window.setState({wifi, bluetooth})`).
//!
//! Toute la mécanique fenêtre/rendu/relais souris est dans
//! `win32::cef_popup` — voir `win32::cef_test_window` pour la doc de
//! module complète du pattern suivi ici.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_app::percent_decode;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::menu;
use crate::win32::network;
use crate::win32::reseaux_wifi;

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// Voir `app::content_scale_pour`, doc de fonction — mémorisé ici à
    /// l'ouverture (voir `toggle`) et relu à chaque frame par
    /// `render_if_visible`.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// La liste des réseaux Wi-Fi, relue au plus toutes les `WIFI_TTL` (voir
    /// `wifi_actuel`) : `poll` pousse l'état à CHAQUE frame, et une lecture
    /// `wlanapi` coûte ~7 ms (mesuré le 2026-11-26, voir
    /// `reseaux_wifi::essai_reel_liste_des_reseaux`).
    static WIFI: RefCell<Option<(Instant, reseaux_wifi::EtatWifi)>> = const { RefCell::new(None) };
}

/// Intervalle de relecture de la liste des réseaux (voir `WIFI`).
const WIFI_TTL: Duration = Duration::from_secs(2);

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefNetworkWindow"),
    window_title: w!("NeuraDesk — Réseau"),
    html_file: "network.html",
    wndproc: Some(wndproc),
    above_a_magnified_dock: true,
    // MENU — apparaît et disparaît en fondant, voir `PopupSpec::fades`.
    fades: true,
    fenetre: false,
};

/// Précharge ce popup au démarrage — fenêtre et navigateur naissent CACHÉS, pour
/// que la page ait fini de charger avant la première ouverture réelle. Voir
/// `win32::cef_popup::preload`, doc de fonction, qui porte le pourquoi complet.
pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// Voir `win32::cef_popup::toggle`. Pousse aussi l'état Wi-Fi/Bluetooth
/// actuel (voir `push_state`) après avoir (re)montré/créé la fenêtre — même
/// si elle vient d'être masquée par cet appel (voir `push_state`, doc de
/// fonction : un `execute_java_script` sur une page cachée est un
/// gaspillage négligeable, pas une erreur, plus simple que de faire
/// remonter la visibilité depuis `cef_popup::toggle`).
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    CONTENT_SCALE.with(|c| c.set(content_scale));
    cef_popup::toggle(&SLOT, &SPEC, rect, gfx);
    // ⚠️ **UN BALAYAGE À L'OUVERTURE, MAIS PAS DE LECTURE IMMÉDIATE** : le scan
    // est ASYNCHRONE (voir `reseaux_wifi`, doc de module) ; la première liste
    // lue toute de suite serait celle du cache d'AVANT. Le `poll` des frames
    // suivantes la relit (au plus toutes les 2 s) et rattrape le scan.
    reseaux_wifi::balayer();
    push_state();
}

/// Oublie la liste en cache : le prochain `push_state` relit `wlanapi`. Appelé
/// après une connexion ou une bascule de radio, dont l'effet doit se voir tout
/// de suite plutôt qu'au bout de `WIFI_TTL`.
pub fn forcer_rafraichissement() {
    WIFI.with(|cache| *cache.borrow_mut() = None);
}

/// L'état Wi-Fi actuel, depuis le cache sauf si `forcer` ou s'il a expiré.
fn wifi_actuel(forcer: bool) -> reseaux_wifi::EtatWifi {
    WIFI.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !forcer {
            if let Some((quand, etat)) = cache.as_ref() {
                if quand.elapsed() < WIFI_TTL {
                    return etat.clone();
                }
            }
        }
        let etat = reseaux_wifi::etat();
        *cache = Some((Instant::now(), etat.clone()));
        etat
    })
}

/// Pousse l'état Wi-Fi/Bluetooth ACTUEL (voir `win32::network::wifi_state`/
/// `bluetooth_state`) dans la page via `window.setState` (voir
/// `assets/popups/network.html`) — à appeler à l'ouverture du popup (voir
/// `toggle`) ET après chaque bascule effective (voir `app::run`, dispatch
/// de `drain_actions` juste après `network::toggle_wifi`/
/// `toggle_bluetooth`), pour que l'affichage ne reste jamais périmé.
pub fn push_state() {
    pousser(false);
}

/// Voir `push_state`. `forcer` relit la LISTE DES RÉSEAUX tout de suite.
fn pousser(forcer: bool) {
    let reseaux = wifi_actuel(forcer);
    let networks: Vec<_> = reseaux
        .reseaux
        .iter()
        .map(|r| {
            json!({
                "ssid": r.ssid,
                "signal": r.signal,
                "secure": r.securise,
                "connected": r.connecte,
                "profile": r.profil,
            })
        })
        .collect();
    // `Option<bool>` rend `null` en JSON quand la radio est absente, ce que la
    // page distingue d'un « désactivé » (voir `js_bool_literal`, supprimé).
    let payload = json!({
        "wifi": network::wifi_state(),
        "bluetooth": network::bluetooth_state(),
        "networks": networks,
        "wifiProfiles": reseaux.profils,
        "wifiRefused": reseaux.localisation_refusee,
    });
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
}

/// À appeler à CHAQUE frame tant que ce popup est visible (voir `app::run`,
/// même endroit que `win32::cef_voicemeeter::poll`/`win32::cef_dash::poll`)
/// — élimine PAR CONSTRUCTION la course avec le chargement initial de la
/// page CEF : le `push_state()` appelé par `toggle()` juste après la
/// création de la fenêtre peut arriver AVANT que `window.setState` n'existe
/// côté JS (page `file://` pas encore chargée, voir `win32::cef_app::
/// NeuraDeskLoadHandler`, doc de struct) et être perdu en silence — BUG RÉEL
/// SIGNALÉ ("il faut ouvrir 2 fois les menus pour qu'ils soient remplis") :
/// la seconde ouverture réussissait simplement parce que la page avait eu le
/// temps de charger entretemps. Un `poll()` par frame rattrape l'état dès
/// que la page devient prête, sans avoir à détecter précisément ce moment.
pub fn poll() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

/// Vide et retourne les actions de la page (voir `drain_actions`). Les verbes de
/// la LISTE DES RÉSEAUX vivent ici aussi :
/// `network/wifi-connect/<profil encodé>`, `network/ouvrir-localisation`,
/// `network/wifi-balayer` — voir `assets/popups/network.html::go` et
/// `app::run`, où ils sont appliqués.
pub const VERBES: [&str; 6] = [
    "toggle-wifi",
    "toggle-bluetooth",
    "wifi-connect",
    "wifi-balayer",
    "ouvrir-localisation",
    "ouvrir-parametres",
];

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

/// Vide et retourne les actions demandées par `assets/popups/network.html`
/// (voir `win32::cef_popup::drain_actions`) — format `"network/<verbe>"` ou
/// `"network/wifi-connect/<nom de profil encodé>"`. La liste des verbes
/// acceptés est `VERBES`, et le test `la_page_et_le_dispatch_acceptent_les_
/// memes_verbes` empêche les deux côtés de diverger.
pub fn drain_actions() -> Vec<String> {
    cef_popup::drain_actions(&SLOT)
}

pub fn render_if_visible(tint_rgba: [f32; 4]) {
    let scale = CONTENT_SCALE.with(|c| c.get());
    cef_popup::render_if_visible(&SLOT, tint_rgba, menu::menu_corner_radius(), cef_popup::PopupPalette::Menu { zoom_level: cef_popup::menu_zoom_level(scale) });
}

pub fn debug_dump_backbuffer(path: &Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(result) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = include_str!("../../assets/popups/network.html");
    /// Le dispatch des actions de ce popup est dans `app::run` — même lecture
    /// que `cef_sound`, dont le dispatch vit dans son propre module.
    const APP: &str = include_str!("../app.rs");

    /// Les verbes que la page envoie via `go("…")`.
    fn verbes_de_la_page() -> Vec<String> {
        PAGE.match_indices("go(")
            .filter_map(|(i, _)| {
                let reste = &PAGE[i + 3..];
                let guillemet = reste.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                reste[1..].split(guillemet).next().map(str::to_string)
            })
            .collect()
    }

    /// ⛔ **LE SEUL TEST QUI GARDE CETTE PAGE.** Un verbe ajouté d'un côté
    /// seulement est mort en silence : la page l'envoie dans le vide, ou le
    /// dispatch attend une action que personne n'envoie.
    #[test]
    fn la_page_et_le_dispatch_acceptent_les_memes_verbes() {
        let mut page = verbes_de_la_page();
        page.sort();
        page.dedup();
        let mut attendus: Vec<String> = VERBES.iter().map(|v| v.to_string()).collect();
        attendus.sort();
        assert_eq!(page, attendus, "la page et `VERBES` doivent citer les mêmes verbes");

        for verbe in VERBES {
            // `wifi-connect` porte un argument : le dispatch le cherche par le
            // PRÉFIXE (`network/wifi-connect/`), les autres par le littéral.
            let cle = if verbe == "wifi-connect" {
                "network/wifi-connect/".to_string()
            } else {
                format!("\"network/{verbe}\"")
            };
            assert!(APP.contains(&cle), "aucun bras de dispatch pour `{verbe}` dans app.rs ({cle})");
        }
    }

    /// Ce que Rust appelle dans la page doit y exister, sinon l'état se perd
    /// EN SILENCE (même essai que `cef_sound`).
    #[test]
    fn la_page_definit_ce_que_rust_appelle() {
        assert!(PAGE.contains("window.setState"), "network.html doit définir window.setState");
        for champ in ["networks", "wifiProfiles", "wifiRefused"] {
            assert!(PAGE.contains(champ), "la page n'exploite pas `{champ}`, que `pousser` envoie");
        }
    }

    /// ⚠️ **LES SSID ET LES PROFILS SONT DU CONTENU UTILISATEUR** : sans
    /// `data-no-i18n`, le traducteur des pages les prendrait pour des mots de
    /// l'interface (voir `src/ui/langue.rs`) — un réseau nommé « Fermer »
    /// deviendrait « Close ».
    #[test]
    fn les_noms_de_reseaux_ne_sont_jamais_traduits() {
        assert!(PAGE.contains("nom.dataset.noI18n"), "les noms de réseaux doivent porter data-no-i18n");
    }
}
