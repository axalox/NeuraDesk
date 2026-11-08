//! Détection du thème Windows courant : couleur d'accentuation (celle
//! qu'utilise la barre des tâches quand "Afficher la couleur d'accentuation
//! sur la barre des tâches et le menu Démarrer" est actif) et mode
//! clair/sombre (Paramètres > Personnalisation > Couleurs). Permet au dock
//! et à ses menus de suivre visuellement le thème système par défaut, sans
//! rien figer en dur.

use std::sync::OnceLock;

use windows::core::{w, BOOL, PCSTR};
use windows::Win32::Foundation::{ERROR_SUCCESS, HMODULE, HWND};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DwmGetColorizationColor, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

use crate::ui::color::{DOCK_ALPHA, DOCK_BACKGROUND_FALLBACK};

/// Couleur RGBA (0.0..1.0, alpha non prémultiplié) de la couleur
/// d'accentuation Windows actuelle — la même que celle de la barre des
/// tâches lorsque l'utilisateur l'a activée. Retombe sur l'aubergine
/// d'origine si la détection échoue.
pub fn detect_accent_color() -> [f32; 4] {
    unsafe {
        let mut colorization: u32 = 0;
        let mut opaque_blend = BOOL(0);
        if DwmGetColorizationColor(&mut colorization, &mut opaque_blend).is_ok() {
            let r = ((colorization >> 16) & 0xFF) as f32 / 255.0;
            let g = ((colorization >> 8) & 0xFF) as f32 / 255.0;
            let b = (colorization & 0xFF) as f32 / 255.0;
            return [r, g, b, DOCK_ALPHA];
        }
    }
    DOCK_BACKGROUND_FALLBACK
}

/// La couleur d'accentuation de Windows TELLE QU'ELLE EST CHOISIE dans
/// Paramètres > Personnalisation > Couleurs — valeur `AccentColor` de la clé
/// DWM. Pour le thème « Windows clair »/« Windows sombre » (voir
/// `win32::cef_settings`, action `theme windows`).
///
/// **PAS `detect_accent_color`** : `DwmGetColorizationColor` rend la couleur de
/// COLORISATION des cadres, un mélange qui n'est pas l'accent affiché par les
/// Paramètres. `None` si la valeur est absente (retombée laissée à l'appelant).
pub fn accent_windows() -> Option<[f32; 3]> {
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY : tampon DWORD local et sa taille, chaînes littérales `w!`.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\DWM"),
            w!("AccentColor"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then(|| abgr_vers_rgb(value))
}

/// `AccentColor` est rangé en **ABGR** (rouge dans l'octet de POIDS FAIBLE),
/// l'inverse de `DwmGetColorizationColor` (ARGB) — les confondre donnerait un
/// bleu à la place d'un orange. **FONCTION PURE, ET TESTÉE.**
fn abgr_vers_rgb(value: u32) -> [f32; 3] {
    let r = (value & 0xFF) as f32 / 255.0;
    let g = ((value >> 8) & 0xFF) as f32 / 255.0;
    let b = ((value >> 16) & 0xFF) as f32 / 255.0;
    [r, g, b]
}

/// `true` si l'utilisateur a choisi le thème sombre pour les applications
/// (Paramètres > Personnalisation > Couleurs > Choisissez votre mode).
/// Lu dans le registre (`AppsUseLightTheme`, le réglage standard que la
/// plupart des applications consultent) ; suppose le mode clair (`false`)
/// si la lecture échoue.
pub fn prefers_dark_mode() -> bool {
    unsafe {
        let mut value: u32 = 1;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        );
        status == ERROR_SUCCESS && value == 0
    }
}

/// Sous-clé où Windows range le mode clair/sombre, et les DEUX valeurs qu'il
/// consulte : `AppsUseLightTheme` pour les applications (la seule que ce module
/// LIT, voir `prefers_dark_mode`) et `SystemUsesLightTheme` pour le shell —
/// barre des tâches, menu Démarrer, centre de notifications, menus contextuels.
const PERSONALIZE_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
const APPS_USE_LIGHT_THEME: &str = "AppsUseLightTheme";
const SYSTEM_USES_LIGHT_THEME: &str = "SystemUsesLightTheme";

/// Délai accordé à la diffusion du changement de thème (voir
/// `broadcast_theme_change`) — court À DESSEIN : au-delà, une fenêtre pendue
/// retarderait le dock lui-même, qui appelle ceci depuis son fil d'interface.
const BROADCAST_TIMEOUT_MS: u32 = 1000;

/// Les deux valeurs de registre qui expriment `dark`. **FONCTION PURE, ET
/// TESTÉE.**
///
/// **LA SÉMANTIQUE EST INVERSÉE, ET C'EST LA SEULE CHOSE QUI PUISSE SE TROMPER
/// ICI** : la valeur s'appelle « …UseLIGHTTheme » et vaut `0` pour le mode
/// SOMBRE (voir `prefers_dark_mode`, qui teste `value == 0`). Écrire `1` pour
/// sombre donnerait un bouton qui fait exactement l'INVERSE de ce qu'il
/// annonce — sans erreur, sans journal, et l'utilisateur ne verrait qu'un
/// thème qui ne bascule pas comme il faut.
///
/// Rend `(AppsUseLightTheme, SystemUsesLightTheme)` — les deux TOUJOURS
/// ensemble : ne basculer que la première laisserait la barre des tâches et le
/// menu Démarrer sur l'ancien thème, ce qui se lit comme un demi-échec.
fn theme_registry_values(dark: bool) -> (u32, u32) {
    let light = u32::from(!dark);
    (light, light)
}

/// Bascule le thème clair/sombre de WINDOWS — les applications ET le shell.
///
/// Rend `true` si les deux écritures ont été acceptées. **Un refus rend `false`
/// sans rien tenter de plus** (voir `registry_util::write_dword_value`, et la
/// doctrine de `win32::power::enable_shutdown_privilege`) : une stratégie de
/// groupe peut verrouiller ce réglage, et l'appelant doit alors RENONCER à la
/// suite — notamment NE PAS repeindre le dock sur un état qui n'a pas été
/// écrit, ce qui afficherait un thème que Windows n'a pas.
///
/// ⚠️ **IL N'Y A PAS DE TRANSACTION AU REGISTRE** : un échec sur la seconde
/// valeur laisse la première appliquée. C'est signalé par le `false` rendu, que
/// l'appelant journalise — c'est tout ce qu'on peut honnêtement en dire.
///
/// ⚠️ **AUCUN TEST N'EXERCE CETTE FONCTION**, et c'est délibéré : elle change le
/// thème du SYSTÈME de l'utilisateur — une action à effet réel, pas une
/// assertion. `cargo test` ne doit jamais faire basculer le thème de quelqu'un.
/// Seule la table pure `theme_registry_values` est vérifiable, et c'est elle qui
/// porte le risque de se tromper.
pub fn set_dark_mode(dark: bool) -> bool {
    let (apps, system) = theme_registry_values(dark);
    let apps_ok = super::registry_util::write_dword_value(
        HKEY_CURRENT_USER,
        PERSONALIZE_KEY,
        APPS_USE_LIGHT_THEME,
        apps,
    );
    let system_ok = super::registry_util::write_dword_value(
        HKEY_CURRENT_USER,
        PERSONALIZE_KEY,
        SYSTEM_USES_LIGHT_THEME,
        system,
    );
    apps_ok && system_ok
}

/// Bascule le thème de Windows depuis l'état COURANT, et rend le nouvel état
/// (`None` si l'écriture a échoué).
///
/// **Relit `prefers_dark_mode()` plutôt que de tenir un état à lui** : Windows
/// fait foi et persiste lui-même ce réglage — même choix que
/// `win32::taskbar_autohide`, qui ne mémorise rien non plus. Un état tenu côté
/// NeuraDesk finirait par diverger d'un changement fait dans les Paramètres, et
/// le bouton basculerait alors dans le mauvais sens.
pub fn toggle_dark_mode() -> Option<bool> {
    let dark = !prefers_dark_mode();
    set_dark_mode(dark).then_some(dark)
}

/// Prévient TOUTES les fenêtres du système que le thème a changé, pour qu'elles
/// se repeignent — sans quoi la barre des tâches, le menu Démarrer et les
/// applications garderaient leurs anciennes couleurs, et le clic paraîtrait
/// n'avoir rien fait.
///
/// `WM_SETTINGCHANGE` avec `"ImmersiveColorSet"` est la convention que Windows
/// emploie lui-même quand on change le mode depuis les Paramètres.
///
/// ⚠️ **C'EST UN ENVOI SYNCHRONE À TOUTES LES FENÊTRES, DONC IL PEUT BLOQUER.**
/// D'où le délai court et `SMTO_ABORTIFHUNG` : sans eux, une seule fenêtre
/// pendue retarderait tout le dock, qui appelle ceci depuis son fil
/// d'interface. **À appeler APRÈS avoir rafraîchi notre propre état**, pour que
/// notre repeint ne fasse pas la queue derrière celui des autres.
///
/// Best-effort assumé : un délai dépassé n'est pas une erreur à signaler — le
/// message a été délivré à qui a bien voulu répondre, et ceux qui ne répondent
/// pas se rafraîchiront d'eux-mêmes.
pub fn broadcast_theme_change() {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    // La charge utile de ce message est le NOM du réglage changé, en UTF-16
    // terminé par un nul — c'est ce que les fenêtres lisent dans `lParam`.
    let setting: Vec<u16> = "ImmersiveColorSet\0".encode_utf16().collect();
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(setting.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            BROADCAST_TIMEOUT_MS,
            None,
        );
    }
}

/// `HMODULE` enveloppe un pointeur brut (ni `Send` ni `Sync`) ; on stocke sa
/// valeur en `usize` dans la statique (le process entier tourne sur un seul
/// thread ici de toute façon) et on reconstruit `HMODULE` à chaque appel.
fn uxtheme_module() -> Option<HMODULE> {
    static MODULE_ADDR: OnceLock<usize> = OnceLock::new();
    let addr = *MODULE_ADDR.get_or_init(|| unsafe {
        LoadLibraryExW(w!("uxtheme.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
            .map(|m| m.0 as usize)
            .unwrap_or(0)
    });
    (addr != 0).then_some(HMODULE(addr as *mut core::ffi::c_void))
}

/// ⛔ **CES ORDINALS NE SONT APPELÉS QUE SUR WINDOWS 11 — MESURÉ, PAS SUPPOSÉ.**
///
/// Sur la VM Windows 10 22H2 (build 19045) du 2026-10-28, le premier lancement
/// meurt en **`c0000005` DANS `uxtheme.dll`** (`+0x24976`, EventLog `APPCRASH`) à
/// `0.05 s`, avant la moindre fenêtre : le journal de l'application s'arrête à sa
/// troisième ligne, aucun processus CEF n'existe, et toutes les tentatives
/// suivantes répondent seulement « NeuraDesk tourne deja » (verrou du cache CEF
/// laissé par le crash).
///
/// ⚠️ **`GetProcAddress` RÉUSSIT SUR CETTE BUILD** — c'est l'APPEL qui viole
/// l'accès : l'ordinal existe des deux côtés, mais la fonction n'a pas la même
/// signature d'une lignée à l'autre. Un test « l'ordinal est-il là ? » n'aurait
/// donc rien vu venir ([`resolve_ordinal`] rend `Some`, et c'est bien le piège).
///
/// ⇒ `enable_dark_menus` et l'ordinal 145 d'`apply_dark_mode_to_window` sont
/// réservés à Windows 11 (`win32::os_version`). Sur Windows 10, les menus
/// natifs restent clairs — et l'attribut DWM **documenté**
/// (`DWMWA_USE_IMMERSIVE_DARK_MODE`) continue de donner sa barre de titre sombre,
/// lui qui ne dépend d'aucun ordinal.
///
/// Résout `ordinal` dans uxtheme.dll et le transmute vers `F`. `None` si la
/// DLL ou l'ordinal est introuvable (best-effort : ce sont des exports non
/// documentés — aucun nom, aucun en-tête — dont Microsoft ne garantit pas
/// la stabilité d'une version de Windows à l'autre).
unsafe fn resolve_ordinal<F: Copy>(ordinal: usize) -> Option<F> {
    let module = uxtheme_module()?;
    let proc = GetProcAddress(module, PCSTR(ordinal as *const u8))?;
    Some(std::mem::transmute_copy(&proc))
}

/// Autorise le thème sombre pour les menus contextuels natifs
/// (`TrackPopupMenu`) du process entier, en fonction du thème système
/// actuel (Windows en décide dynamiquement à chaque affichage de menu une
/// fois cette autorisation donnée — pas besoin de la redemander si le
/// thème change en cours de route).
///
/// `SetWindowTheme` seul ne suffit pas pour le chrome des menus popup
/// eux-mêmes (plus de menus natifs dans ce projet de toute façon, voir
/// `win32::popup_menu`, mais Windows peut encore en afficher pour d'autres
/// besoins) : il faut `SetPreferredAppMode` de
/// uxtheme.dll (ordinal 135, non documenté). Best-effort : peut échouer
/// (repli sur le thème clair classique) si l'ordinal est introuvable — ces
/// exports ne sont pas garantis stables d'une version de Windows à
/// l'autre — auquel cas un message diagnostique est affiché (utile pour
/// identifier ce cas précis plutôt que de deviner). À appeler une seule
/// fois, avant l'affichage du premier menu — voir `app::run`.
pub fn enable_dark_menus() {
    type SetPreferredAppModeFn = unsafe extern "system" fn(i32) -> i32;
    const ALLOW_DARK: i32 = 1;

    // ⛔ **PAS SUR WINDOWS 10 — VOIR LA DOC DE `resolve_ordinal`** : l'appel tue
    // le processus en `c0000005` dans `uxtheme.dll`, mesuré sur 19045. Une
    // version illisible (registre muet) suit le même chemin : on ne tente pas.
    if !crate::win32::os_version::os_windows_11() {
        crate::diag!("thème sombre des menus : ordinals uxtheme NON appelés sur cette lignée (crash mesuré sur Windows 10 19045)");
        return;
    }

    unsafe {
        match resolve_ordinal::<SetPreferredAppModeFn>(135) {
            Some(set_preferred_app_mode) => {
                let _ = set_preferred_app_mode(ALLOW_DARK);
            }
            None => eprintln!("[neuradesk] thème sombre des menus indisponible (ordinal 135 introuvable dans uxtheme.dll)"),
        }
    }
}

/// Complète `enable_dark_menus` en déclarant explicitement `hwnd` comme
/// pouvant utiliser le mode sombre (`AllowDarkModeForWindowWithParentFallback`,
/// ordinal 145, même statut non documenté) et en activant l'attribut DWM
/// officiel `DWMWA_USE_IMMERSIVE_DARK_MODE` sur sa bordure/barre de titre.
/// À appeler une fois, juste après la création de la fenêtre principale du
/// dock — voir `app::run`.
pub fn apply_dark_mode_to_window(hwnd: HWND) {
    type AllowDarkModeForWindowFn = unsafe extern "system" fn(HWND, BOOL) -> BOOL;

    unsafe {
        // Même gate que `enable_dark_menus` : aucun ordinal d'uxtheme sur
        // Windows 10 (voir `resolve_ordinal`). L'attribut DWM ci-dessous, lui,
        // est documenté depuis 20H1 — donc valable sur notre plancher 22H2.
        if crate::win32::os_version::os_windows_11() {
            if let Some(allow) = resolve_ordinal::<AllowDarkModeForWindowFn>(145) {
                let _ = allow(hwnd, BOOL(1));
            }
        }

        let dark = prefers_dark_mode();
        let value = BOOL(dark as i32);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &value as *const BOOL as *const _,
            std::mem::size_of::<BOOL>() as u32,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La valeur relevée sur cette machine (`0xffbc9900`) est le bleu-vert
    /// `#0099BC` de la palette Windows — rouge dans l'octet de POIDS FAIBLE.
    #[test]
    fn accent_color_est_en_abgr() {
        let rgb = abgr_vers_rgb(0xffbc_9900);
        let octets = rgb.map(|c| (c * 255.0).round() as u8);
        assert_eq!(octets, [0x00, 0x99, 0xBC]);
    }

    /// **LA SEULE CHOSE QUI PUISSE SE TROMPER DANS CETTE FONCTION, ET ELLE EST
    /// INVISIBLE.** La valeur s'appelle `AppsUseLightTheme` — « …utilise le
    /// thème CLAIR » — et vaut `0` pour le mode SOMBRE. L'écrire à l'envers
    /// donnerait un bouton qui fait exactement le contraire de ce qu'il annonce :
    /// aucune erreur, aucun journal, juste un thème qui ne bascule pas comme il
    /// faut. D'où un test qui fixe la correspondance dans les DEUX sens.
    #[test]
    fn dark_means_zero_light_and_light_means_one() {
        assert_eq!(theme_registry_values(true), (0, 0), "sombre = 0 (la valeur dit « light »)");
        assert_eq!(theme_registry_values(false), (1, 1), "clair = 1");
    }

    /// Les deux valeurs sont écrites ENSEMBLE : ne basculer que
    /// `AppsUseLightTheme` laisserait la barre des tâches, le menu Démarrer et
    /// le centre de notifications sur l'ancien thème — un demi-échec que
    /// l'utilisateur verrait tout de suite.
    #[test]
    fn the_two_theme_values_never_diverge() {
        for dark in [true, false] {
            let (apps, system) = theme_registry_values(dark);
            assert_eq!(apps, system, "les deux réglages doivent décrire le même thème");
        }
    }
}
