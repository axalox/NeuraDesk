//! Dérivation PURE (aucun appel Win32) de l'épaisseur des bandes de dock à
//! afficher sur un écran SECONDAIRE (voir `win32::window::SecondaryWindow`)
//! — extrait de `win32::window` (déjà volumineux) pour rester dans la limite
//! de taille de fichier du projet et garder ensemble tout ce qui est
//! unitairement testable sans écran/fenêtre réels.
//!
//! Point de départ de ce module : Phase 2 du plan "hardcoded setting" —
//! auparavant, l'épaisseur d'une copie secondaire était une COPIE littérale
//! des curseurs de la barre primaire (voir `Window::top_thickness`), jamais
//! adaptée à l'écran secondaire lui-même. Ce module calcule maintenant cette
//! épaisseur à partir des propriétés PROPRES à chaque écran (sa propre
//! taskbar Windows si présente, sinon un repli mis à l'échelle de son DPI),
//! voir `derive_secondary_thickness`.
//!
//! BUG RÉEL SIGNALÉ ("faire glisser les curseurs 'Épaisseur barre du
//! haut/gauche/droite' de la barre PRIMAIRE redimensionnait aussi la barre
//! d'un écran secondaire SANS taskbar Windows propre") : le "repli DPI"
//! ci-dessous (voir `derive_secondary_thickness`) partait de la valeur
//! COURANTE des curseurs de réglage de la barre primaire — un simple
//! glissement de curseur, sans jamais toucher un écran secondaire,
//! recréait donc sa copie à une nouvelle épaisseur. `baseline_thicknesses`
//! fixe ce point de départ sur un fait STABLE et propre à l'écran primaire
//! (SA PROPRE taskbar Windows, `win32::window::own_taskbar_thickness`) —
//! jamais sur un réglage utilisateur ajustable en direct.

/// Épaisseur de la barre des tâches Windows par défaut sur Windows 11 à
/// 100% de mise à l'échelle (96 DPI) — repli de dernier recours pour
/// `baseline_thicknesses` quand l'écran primaire lui-même n'a pas de
/// taskbar Windows détectée (utilisateur ayant désactivé "Afficher la barre
/// des tâches sur tous les écrans", ou taskbar masquée/en cours de
/// réinitialisation). Valeur mesurée empiriquement (taille par défaut d'une
/// installation Windows 11 fraîche), PAS une constante documentée par
/// Microsoft.
#[cfg(test)]
pub(crate) const DEFAULT_BAR_THICKNESS_PX: u32 = 48;

/// Calcule le point de départ ("baseline") commun aux trois bandes
/// (top/left/right) utilisé par `BarThicknesses::derive_for_screen`/
/// `derive_secondary_thickness` comme repli pour un écran SECONDAIRE SANS
/// taskbar Windows propre — voir le "BUG RÉEL SIGNALÉ" en tête de module.
/// `primary_taskbar_thickness` : épaisseur de la VRAIE taskbar Windows de
/// l'écran PRIMAIRE lui-même (voir `win32::window::own_taskbar_thickness`),
/// un fait stable et propre à cet écran — JAMAIS la valeur courante des
/// curseurs "Épaisseur barre du haut/gauche/droite" du panneau de réglages
/// (ceux-ci restent un réglage de rendu de la barre PRIMAIRE, sans aucun
/// rapport avec la géométrie d'un écran secondaire). `Some(0)` (taskbar
/// détectée mais dégénérée) est traité comme `None` : une épaisseur nulle
/// n'est jamais une base saine, voir `derive_secondary_thickness_ignores_a_
/// zero_baseline_dpi` pour la même famille de garde-fou côté DPI.
#[cfg(test)]
pub(crate) fn baseline_thicknesses(primary_taskbar_thickness: Option<u32>) -> BarThicknesses {
    let thickness = primary_taskbar_thickness.filter(|&t| t > 0).unwrap_or(DEFAULT_BAR_THICKNESS_PX);
    BarThicknesses { top: thickness, left: thickness, right: thickness, bottom: 0 }
}

/// Épaisseur (top/gauche/droite) de BASE dérivée de la taskbar Windows
/// PROPRE à l'écran primaire (voir `baseline_thicknesses`) — utilisée par
/// `win32::window::SecondaryWindow::create`/`derive_secondary_thickness`
/// UNIQUEMENT comme valeur de REPLI pour un écran secondaire SANS taskbar
/// Windows propre (mise à l'échelle de son DPI, voir la doc de
/// `derive_secondary_thickness`) : PAS la valeur générale utilisée pour
/// tous les écrans secondaires — un écran avec sa propre taskbar ignore
/// totalement cette structure. JAMAIS les curseurs "Épaisseur barre du
/// haut/gauche/droite" du panneau de réglages (voir le "BUG RÉEL SIGNALÉ"
/// en tête de module).
#[derive(Debug, Clone, Copy)]
pub struct BarThicknesses {
    pub top: u32,
    pub left: u32,
    pub right: u32,
    /// Épaisseur de la barre du BAS — **TOUJOURS 0 sur un écran secondaire**, et
    /// c'est LE mécanisme « écran principal seulement » de cette barre : elle ne
    /// se désactive pas par un drapeau, elle devient invisible par la géométrie
    /// (voir `DockLayout::bottom_thickness`, et `Side::Bottom`). Un écran
    /// secondaire n'enregistre d'ailleurs AUCUN AppBar `ABE_BOTTOM` (voir
    /// `SecondaryWindow`).
    pub bottom: u32,
}

impl BarThicknesses {
    /// Dérive les trois épaisseurs (top/left/right) d'UN écran secondaire en
    /// un seul appel — évite de répéter les 5 mêmes arguments trois fois
    /// côté appelant (voir `win32::window::SecondaryWindow::create`, qui
    /// appelait auparavant `derive_secondary_thickness` séparément pour
    /// chaque bande). `own_taskbar_thickness` : voir sa doc — le même statut
    /// "cet écran a-t-il sa propre taskbar" s'applique aux trois bandes,
    /// une taskbar Windows n'occupant jamais qu'un seul bord à la fois.
    /// `self` sert de repli (une épaisseur PAR bande, voir `derive_secondary_
    /// thickness`) — ce n'est PAS le résultat, qui est un `BarThicknesses`
    /// indépendant retourné ici.
    pub(crate) fn derive_for_screen(
        &self,
        own_taskbar_thickness: Option<u32>,
        monitor_dpi: u32,
        baseline_dpi: u32,
        monitor_shorter_dimension_px: u32,
    ) -> BarThicknesses {
        let derive = |fallback: u32| {
            derive_secondary_thickness(own_taskbar_thickness, monitor_dpi, baseline_dpi, fallback, monitor_shorter_dimension_px)
        };
        BarThicknesses {
            top: derive(self.top),
            left: derive(self.left),
            right: derive(self.right),
            // ⚠️ **0, et pas `derive(self.bottom)`** : la barre du bas n'existe
            // pas sur un écran secondaire (demande explicite de l'utilisateur).
            // On ne DÉRIVE donc rien pour elle — dériver une épaisseur puis la
            // remettre à zéro ailleurs serait deux endroits à tenir d'accord.
            bottom: 0,
        }
    }
}

/// Épaisseur minimale absolue d'une bande de dock (secondaire), quelle que
/// soit sa source (taskbar Windows détectée sur cet écran OU repli mis à
/// l'échelle du DPI, voir `derive_secondary_thickness`) — garde-fou contre
/// une valeur aberrante à l'écran, même famille de risque que le clamp de
/// `win32::window::dock_window_bounds` (rectangles transitoires bidons
/// pendant un changement de topologie, voir `app::DISPLAY_CHANGE_DEBOUNCE`).
/// 24px reste perceptible/cliquable visuellement tout en restant très fin.
const MIN_BAR_THICKNESS_PX: u32 = 24;

/// Fraction maximale de la plus petite dimension de l'écran secondaire
/// (largeur pour une bande verticale, hauteur pour la bande du haut) qu'une
/// bande peut occuper — garde-fou contre un repli DPI extrême (tout petit
/// écran secondaire à très haut DPI) ou une taskbar détectée avec un
/// rectangle aberrant : sans plafond, la bande pourrait avaler la quasi-
/// totalité de l'écran et rendre la copie inutilisable. 15% laisse toujours
/// la grande majorité de l'écran utilisable.
const MAX_BAR_THICKNESS_RATIO: f64 = 0.15;

/// Épaisseur (top/left/right, appelée indépendamment pour chacune, voir
/// `win32::window::SecondaryWindow::create`) de la bande de dock à afficher
/// sur UN écran secondaire donné. Règle IDENTIQUE à celle de la barre
/// PRIMAIRE dans `win32::window::Window::create_dock`
/// (`dock_thickness = taskbar_thickness`) : si CET écran a sa propre taskbar
/// Windows (`taskbar_thickness` vient de `win32::window::
/// detect_secondary_taskbar`, `Some` seulement si `win32::monitors::
/// MonitorInfo::taskbar_rect` est renseigné), on utilise directement son
/// épaisseur — ne dépend alors JAMAIS des réglages de la barre primaire (le
/// point même de cette fonction : plus de copie littérale des curseurs de
/// l'écran principal, voir la doc de `win32::window::SecondaryWindow`).
///
/// Sinon (écran sans taskbar propre, ex. "Afficher la barre des tâches sur
/// tous les écrans" désactivé côté Windows), on retombe sur
/// `baseline_thickness` — voir `baseline_thicknesses` : dérivée de la VRAIE
/// taskbar Windows de l'écran PRIMAIRE lui-même (un fait stable et propre à
/// cet écran), JAMAIS de la valeur courante des curseurs "Épaisseur barre du
/// haut/gauche/droite" du panneau de réglages (voir le "BUG RÉEL SIGNALÉ" en
/// tête de module) — mis à l'échelle du ratio de DPI entre cet écran et
/// l'écran primaire (`monitor_dpi`/`baseline_dpi`, voir `win32::dpi::
/// get_dpi_for_monitor`) : un écran secondaire deux fois plus dense (200% vs
/// 100%) doit avoir une bande deux fois plus épaisse en pixels physiques
/// pour occuper la même taille physique perçue.
///
/// Le résultat est toujours borné par `MIN_BAR_THICKNESS_PX`/
/// `MAX_BAR_THICKNESS_RATIO` (voir leurs docs), dans LES DEUX cas — y
/// compris quand la valeur vient d'une VRAIE taskbar détectée, qui peut
/// rapporter un rectangle transitoire aberrant pendant un changement de
/// topologie d'écran (même risque que `win32::window::dock_window_bounds`).
pub(crate) fn derive_secondary_thickness(
    taskbar_thickness: Option<u32>,
    monitor_dpi: u32,
    baseline_dpi: u32,
    baseline_thickness: u32,
    monitor_shorter_dimension_px: u32,
) -> u32 {
    let raw =
        taskbar_thickness.unwrap_or_else(|| scale_thickness_by_dpi(baseline_thickness, monitor_dpi, baseline_dpi));
    clamp_bar_thickness(raw, monitor_shorter_dimension_px)
}

/// Met `thickness` à l'échelle du ratio `monitor_dpi`/`baseline_dpi` — voir
/// `derive_secondary_thickness`. `baseline_dpi == 0` ne devrait jamais
/// arriver en pratique (`GetDpiForMonitor` renvoie toujours au moins 96,
/// voir `win32::dpi::get_dpi_for_monitor`), mais une division par zéro
/// mérite un repli explicite plutôt qu'un panic/NaN silencieux.
fn scale_thickness_by_dpi(thickness: u32, monitor_dpi: u32, baseline_dpi: u32) -> u32 {
    if baseline_dpi == 0 {
        return thickness;
    }
    let ratio = monitor_dpi as f64 / baseline_dpi as f64;
    (thickness as f64 * ratio).round() as u32
}

/// Borne `thickness` entre `MIN_BAR_THICKNESS_PX` et `MAX_BAR_THICKNESS_RATIO`
/// de `monitor_shorter_dimension_px` — voir la doc de ces deux constantes.
/// Le plafond ne descend jamais sous le plancher (`.max(MIN_BAR_THICKNESS_PX)`
/// sur le plafond lui-même) : sur un écran minuscule où le ratio donnerait un
/// plafond plus bas que le plancher, on privilégie la visibilité de la bande
/// (plancher) plutôt qu'un respect strict du ratio.
fn clamp_bar_thickness(thickness: u32, monitor_shorter_dimension_px: u32) -> u32 {
    let max =
        ((monitor_shorter_dimension_px as f64 * MAX_BAR_THICKNESS_RATIO).round() as u32).max(MIN_BAR_THICKNESS_PX);
    thickness.clamp(MIN_BAR_THICKNESS_PX, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    // `derive_secondary_thickness` — voir sa doc : fonction pure, DPI 96 =
    // 100% (référence Windows standard), 192 = 200%. `MONITOR_SHORTER_DIM`
    // assez grand (écran 1440p) pour ne jamais interférer avec les tests qui
    // ne visent pas explicitement le plafond de `MAX_BAR_THICKNESS_RATIO`.
    const BASELINE_DPI: u32 = 96;
    const MONITOR_SHORTER_DIM: u32 = 1440;

    #[test]
    fn derive_secondary_thickness_uses_the_screens_own_taskbar_when_present() {
        // Même règle que la barre PRIMAIRE dans `Window::create_dock`
        // (`dock_thickness = taskbar_thickness`) : si l'écran secondaire a sa
        // propre taskbar Windows, sa présence prime sur tout repli — le
        // `baseline_thickness` (ici volontairement très différent, 999) ne
        // doit avoir AUCUNE influence.
        let thickness = derive_secondary_thickness(Some(48), BASELINE_DPI, BASELINE_DPI, 999, MONITOR_SHORTER_DIM);
        assert_eq!(thickness, 48);
    }

    #[test]
    fn derive_secondary_thickness_falls_back_to_the_baseline_at_same_dpi() {
        // Pas de taskbar sur cet écran, mais même DPI que le repli : ce
        // dernier ne doit RIEN mettre à l'échelle (ratio DPI == 1.0).
        let thickness = derive_secondary_thickness(None, BASELINE_DPI, BASELINE_DPI, 48, MONITOR_SHORTER_DIM);
        assert_eq!(thickness, 48);
    }

    #[test]
    fn derive_secondary_thickness_scales_up_on_higher_dpi_screen() {
        // Écran secondaire à 200% (192 DPI) alors que le repli est à 100%
        // (96 DPI) : la bande doit doubler pour rester visuellement de la
        // même taille physique sur l'écran haute résolution.
        let thickness = derive_secondary_thickness(None, 192, BASELINE_DPI, 48, MONITOR_SHORTER_DIM);
        assert_eq!(thickness, 96);
    }

    #[test]
    fn derive_secondary_thickness_scales_down_on_lower_dpi_screen() {
        // Inverse du test précédent : écran secondaire à 100% alors que le
        // repli (référence du repli DPI) est à 200%.
        let thickness = derive_secondary_thickness(None, BASELINE_DPI, 192, 96, MONITOR_SHORTER_DIM);
        assert_eq!(thickness, 48);
    }

    #[test]
    fn derive_secondary_thickness_clamps_to_the_minimum() {
        // Repli DPI minuscule (2px) très en dessous de `MIN_BAR_THICKNESS_PX`
        // — jamais une vraie taille de barre des tâches, mais possible en
        // valeur transitoire (voir la doc de `win32::window::
        // dock_window_bounds` pour la même famille de garde-fou). Le
        // résultat ne doit jamais tomber en dessous du plancher.
        let thickness = derive_secondary_thickness(None, BASELINE_DPI, BASELINE_DPI, 2, MONITOR_SHORTER_DIM);
        assert_eq!(thickness, MIN_BAR_THICKNESS_PX);
    }

    #[test]
    fn derive_secondary_thickness_clamps_to_the_maximum() {
        // Même une taskbar RÉELLEMENT détectée peut rapporter une épaisseur
        // aberrante en pleine transition de topologie d'écran (voir la doc
        // de `win32::window::dock_window_bounds`) — un tout petit écran
        // secondaire (`MONITOR_SHORTER_DIM` volontairement réduit ici à
        // 200px) rend le plafond `MAX_BAR_THICKNESS_RATIO` plus bas que la
        // valeur détectée.
        let small_screen_dim = 200;
        let thickness = derive_secondary_thickness(Some(500), BASELINE_DPI, BASELINE_DPI, 48, small_screen_dim);
        let expected_max = (small_screen_dim as f64 * MAX_BAR_THICKNESS_RATIO).round() as u32;
        assert_eq!(thickness, expected_max);
    }

    #[test]
    fn derive_secondary_thickness_prefers_the_floor_on_a_tiny_screen() {
        // Voir la doc de `clamp_bar_thickness` : sur un écran SI petit que
        // `MAX_BAR_THICKNESS_RATIO` donnerait un plafond sous
        // `MIN_BAR_THICKNESS_PX` (ici 100px * 15% = 15px < 24px), la
        // visibilité de la bande prime — le résultat doit rester au
        // PLANCHER, jamais tomber au plafond de ratio (qui serait plus bas).
        let tiny_screen_dim = 100;
        let thickness = derive_secondary_thickness(Some(500), BASELINE_DPI, BASELINE_DPI, 48, tiny_screen_dim);
        assert_eq!(thickness, MIN_BAR_THICKNESS_PX);
    }

    #[test]
    fn derive_secondary_thickness_ignores_a_zero_baseline_dpi() {
        // Voir la doc de `scale_thickness_by_dpi` : `baseline_dpi == 0` ne
        // devrait jamais arriver via un vrai appel Win32 (`get_dpi_for_
        // monitor` retombe toujours sur `STANDARD_DPI`), mais reste un garde-
        // fou explicite contre une division par zéro — ce test fige ce
        // comportement (repli = `thickness` inchangée) plutôt que de laisser
        // la branche non couverte.
        let thickness = derive_secondary_thickness(None, BASELINE_DPI, 0, 48, MONITOR_SHORTER_DIM);
        assert_eq!(thickness, 48);
    }

    // `baseline_thicknesses` — voir sa doc, et le "BUG RÉEL SIGNALÉ" en tête
    // de module : le point de départ des trois bandes vient de la taskbar
    // PROPRE à l'écran primaire, jamais des curseurs de réglage.

    #[test]
    fn baseline_thicknesses_uses_the_primary_screens_own_taskbar() {
        let baseline = baseline_thicknesses(Some(48));
        assert_eq!(baseline.top, 48);
        assert_eq!(baseline.left, 48);
        assert_eq!(baseline.right, 48);
    }

    #[test]
    fn baseline_thicknesses_falls_back_to_the_default_without_a_primary_taskbar() {
        // Écran primaire lui-même sans taskbar Windows détectée (voir
        // `win32::window::own_taskbar_thickness`) : repli sur la constante
        // documentée `DEFAULT_BAR_THICKNESS_PX`, jamais sur un réglage
        // utilisateur.
        let baseline = baseline_thicknesses(None);
        assert_eq!(baseline.top, DEFAULT_BAR_THICKNESS_PX);
        assert_eq!(baseline.left, DEFAULT_BAR_THICKNESS_PX);
        assert_eq!(baseline.right, DEFAULT_BAR_THICKNESS_PX);
    }

    #[test]
    fn baseline_thicknesses_ignores_a_degenerate_zero_taskbar() {
        // Une taskbar "détectée" mais d'épaisseur nulle n'est jamais une
        // base saine (même garde-fou que `derive_secondary_thickness_
        // ignores_a_zero_baseline_dpi` côté DPI) — traitée comme `None`.
        let baseline = baseline_thicknesses(Some(0));
        assert_eq!(baseline.top, DEFAULT_BAR_THICKNESS_PX);
        assert_eq!(baseline.left, DEFAULT_BAR_THICKNESS_PX);
        assert_eq!(baseline.right, DEFAULT_BAR_THICKNESS_PX);
    }
}
