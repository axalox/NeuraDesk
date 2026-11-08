use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    HMONITOR, MonitorFromPoint, MonitorFromWindow, MONITOR_DEFAULTTOPRIMARY, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::WindowsAndMessaging::SetProcessDPIAware;

/// DPI Windows "standard" (échelle 100%) — repli utilisé par
/// `get_dpi_for_monitor` quand `GetDpiForMonitor` échoue (ne devrait jamais
/// arriver une fois le process rendu per-monitor DPI aware par
/// `make_process_dpi_aware`, mais un repli explicite reste plus sûr qu'un
/// zéro qui casserait un calcul de ratio, voir `win32::screen_dock::
/// derive_secondary_thickness`).
pub const STANDARD_DPI: u32 = 96;

/// Rend le process "per-monitor DPI aware v2" (Windows 10 1703+ / Windows 11).
/// Retombe sur l'ancienne API system-wide si le process tourne sur un
/// Windows plus ancien.
pub fn make_process_dpi_aware() {
    unsafe {
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_err() {
            let _ = SetProcessDPIAware();
        }
    }
}

/// DPI "effectif" (`MDT_EFFECTIVE_DPI`, inclut la mise à l'échelle
/// utilisateur ET tout ajustement système) de l'écran identifié par
/// `hmonitor` (voir `win32::monitors::MonitorInfo::handle`) — utilisé par
/// `win32::screen_dock::derive_secondary_thickness` pour mettre à l'échelle
/// l'épaisseur des bandes de dock secondaires sur un écran sans taskbar
/// propre. DPI X et Y sont toujours identiques en pratique sous Windows
/// (pas de DPI anisotrope documenté) : seul `dpi_x` est renvoyé.
pub fn get_dpi_for_monitor(hmonitor: HMONITOR) -> u32 {
    let mut dpi_x = 0u32;
    let mut dpi_y = 0u32;
    let ok = unsafe { GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() };
    if ok && dpi_x > 0 {
        dpi_x
    } else {
        STANDARD_DPI
    }
}

/// **LE RATIO DE DPI D'UN ÉCRAN PAR RAPPORT À LA PRIMAIRE.**
///
/// `1.0` sur la primaire elle-même, **deux tiers** sur un écran à 100 % quand la
/// primaire est à 150 %. C'est le facteur qui rend la taille PERÇUE constante
/// d'un écran à l'autre : un panneau doit occuper la même place dans le champ de
/// vision, pas la même quantité de pixels physiques. Sans lui, le même menu
/// paraît **1,5× plus gros** sur un écran à 100 % que sur la primaire — le point 5
/// de l'audit du 2026-10-06, jamais mesuré.
///
/// ⚠️ **LE DÉNOMINATEUR EST LA PRIMAIRE**, et c'est lui qui rend le changement
/// invisible sur la machine de développement : ratio `1.0`, rien ne bouge. Même
/// forme et même dénominateur que `win32::screen_dock::scale_thickness_by_dpi`,
/// qui applique déjà `monitor_dpi / baseline_dpi` à l'épaisseur des bandes.
///
/// ⚠️ **SÉPARÉ DE LA LECTURE WIN32 POUR ÊTRE VÉRIFIABLE** : `get_dpi_for_monitor`
/// n'est pas injectable, donc un calcul qui vivrait avec lui ne pourrait être
/// éprouvé que sur l'écran de la machine — exactement là où il ne se trompe pas.
/// Même leçon que `cef_visionneuse::rect_pour_ecran`.
///
/// ⛔ **JAMAIS ZÉRO, JAMAIS `NaN`.** Un DPI nul d'un côté ou de l'autre rend
/// `1.0`, le point neutre : une division donnerait `0` ou `inf`, et
/// `content_scale` propagerait ça jusqu'à une fenêtre de taille nulle.
pub(crate) fn ratio_dpi(dpi_ecran: u32, dpi_primaire: u32) -> f32 {
    if dpi_ecran == 0 || dpi_primaire == 0 {
        return 1.0;
    }
    dpi_ecran as f32 / dpi_primaire as f32
}

/// Le ratio DPI de l'écran QUI PORTE `hwnd`, par rapport à la primaire — voir
/// `ratio_dpi`, qui porte le calcul et ses raisons.
///
/// ⚠️ **PAR `MonitorFromWindow`, PAS PAR `monitors::enumerate()`** : l'appelant a
/// toujours sous la main la fenêtre du dock qui a été cliquée, et `enumerate()`
/// énumère EN PLUS les fenêtres de la barre des tâches (`tray_windows`) — payer
/// ça à chaque ouverture de menu serait gratuit.
/// Le ratio DPI de l'écran NOMMÉ `device_name` — pour les appelants qui n'ont
/// qu'un nom et pas de fenêtre : l'amorçage des réglages par écran, où le
/// `device_name` est tout ce qu'on a sous la main.
///
/// ⚠️ **UN NOM INCONNU REND `1.0`** (point neutre) : l'écran a pu être débranché
/// entre l'affichage de la page de réglages et le clic.
pub fn ratio_dpi_pour_ecran(device_name: &str) -> f32 {
    let ecrans = crate::win32::monitors::enumerate();
    let Some(primaire) = ecrans.iter().find(|m| m.is_primary) else { return 1.0 };
    let dpi_primaire = get_dpi_for_monitor(primaire.handle);
    match ecrans.iter().find(|m| m.device_name == device_name) {
        Some(m) => ratio_dpi(get_dpi_for_monitor(m.handle), dpi_primaire),
        None => 1.0,
    }
}

pub fn ratio_dpi_de_l_ecran(hwnd: HWND) -> f32 {
    unsafe {
        let ecran = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        // La primaire est repérée par le coin (0,0) du bureau virtuel : c'est la
        // convention déjà utilisée par `tray_flux` pour le même besoin.
        let primaire = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        ratio_dpi(get_dpi_for_monitor(ecran), get_dpi_for_monitor(primaire))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐ **LA PRIMAIRE VAUT 1.0 — ET C'EST TOUTE LA SÛRETÉ DU LOT.** Un ratio
    /// différent de 1.0 sur la primaire déplacerait tous les menus de la machine
    /// de développement, où rien ne doit bouger.
    #[test]
    fn le_ratio_de_la_primaire_est_un() {
        assert_eq!(ratio_dpi(96, 96), 1.0);
        assert_eq!(ratio_dpi(144, 144), 1.0);
        assert_eq!(ratio_dpi(192, 192), 1.0);
    }

    /// ⛔ **UN ÉCRAN À 100 % SUR UNE PRIMAIRE À 150 % DONNE DEUX TIERS.**
    ///
    /// Littéral, pas la formule : `96 / 144`. ⚠️ Et ce ratio est **plus GRAND**
    /// que ce que `screen_content_scale` appliquait à ce même écran (0.5) — c'est
    /// l'effet voulu : la constance de taille perçue, pas la réduction.
    #[test]
    fn un_ecran_a_100_sur_une_primaire_a_150_donne_deux_tiers() {
        let r = ratio_dpi(96, 144);
        assert!((r - 0.6667).abs() < 1e-4, "ratio {r}");
        assert!(r > 0.5, "le ratio DPI ne doit pas réduire autant que l'écart de résolution : {r}");
    }

    /// ⚠️ **L'AXE INVERSE EST SYMÉTRIQUE — ET JAMAIS MESURÉ.** Aucun écran plus
    /// dense que la primaire n'a été constaté sur cette machine ; la formule est
    /// symétrique, ce test le fige sans prétendre l'avoir observé.
    #[test]
    fn un_ecran_plus_dense_agrandit() {
        assert!((ratio_dpi(192, 96) - 2.0).abs() < 1e-6);
    }

    /// ⛔ **UN DPI NUL NE REND NI ZÉRO NI `inf`.** `get_dpi_for_monitor` ne rend
    /// jamais 0 (il retombe sur `STANDARD_DPI`), mais `content_scale` propage ce
    /// ratio jusqu'à la taille d'une fenêtre : une valeur nulle l'effacerait.
    #[test]
    fn un_dpi_nul_rend_le_point_neutre() {
        assert_eq!(ratio_dpi(96, 0), 1.0);
        assert_eq!(ratio_dpi(0, 96), 1.0);
        assert_eq!(ratio_dpi(0, 0), 1.0);
    }
}
