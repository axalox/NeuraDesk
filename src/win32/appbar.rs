//! Réservation d'espace écran façon barre des tâches, via l'API "AppBar" de
//! l'explorateur Windows (`SHAppBarMessage`). Une fois enregistré, le shell
//! réduit la zone de travail (`SPI_GETWORKAREA`) en conséquence : les
//! fenêtres maximisées et la disposition des icônes du bureau respectent
//! alors l'espace du dock, exactement comme elles respectent la vraie
//! barre des tâches.
//!
//! Limite de l'API (pas de notre fait) : la zone de travail réservée reste
//! toujours un rectangle. La forme en "L" du dock (voir `win32::window`)
//! occupe deux bords distincts de l'écran, ce qui nécessite DEUX
//! enregistrements — un par bord, le shell n'en acceptant qu'un seul par
//! HWND — d'où le second HWND (invisible) créé dans `Window::create_dock`.
//! Conséquence visible : le petit carré du coin (bar_thickness ×
//! bar_thickness), bien que non recouvert par le rendu du dock, reste lui
//! aussi exclu de la zone de travail.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_NEW, ABM_REMOVE, ABM_SETPOS, APPBARDATA};

/// Un enregistrement AppBar actif : réserve `rect` sur un bord de l'écran
/// tant que cette valeur vit ; la réservation est levée à son `Drop` (ce
/// qui redonne l'espace aux fenêtres maximisées / icônes du bureau).
pub struct AppBar {
    hwnd: HWND,
    /// Bord d'origine (`ABE_TOP`/`ABE_LEFT`/`ABE_RIGHT`...) — RE-envoyé à
    /// chaque `update` (voir sa doc) : `APPBARDATA.uEdge` fait partie du
    /// message `ABM_SETPOS`, pas seulement de l'enregistrement initial
    /// (`ABM_NEW`).
    edge: u32,
    /// `false` si `NEURADESK_NO_APPBAR` était positionnée au moment du
    /// `register` (voir sa doc) — aucun `SHAppBarMessage` n'a alors jamais
    /// été envoyé, `update`/`Drop` deviennent des no-op. Réservé au harnais
    /// de vérification (`app::run`, doc de `register`) : SANS ce
    /// débranchement, lancer une seconde instance de NeuraDesk pour tester
    /// une capture rétrécirait la zone de travail (`SPI_GETWORKAREA`) de la
    /// VRAIE session en cours — un effet de bord live non voulu.
    active: bool,
}

impl AppBar {
    /// Enregistre `hwnd` comme AppBar sur le bord `edge` (`ABE_TOP`,
    /// `ABE_LEFT`, ...), réservant `rect` (coordonnées écran absolues,
    /// pixels physiques). `hwnd` doit rester valide tant que l'AppBar
    /// existe (jusqu'à son `Drop`).
    ///
    /// Si la variable d'environnement `NEURADESK_NO_APPBAR` est positionnée
    /// (à n'importe quelle valeur), AUCUN message `SHAppBarMessage` n'est
    /// envoyé — l'AppBar renvoyée ne réserve rien et son `Drop` ne libère
    /// rien. Existe uniquement pour le harnais de capture du back buffer
    /// (voir `gfx::capture`, `app::run`) : une instance de NeuraDesk lancée
    /// pour un test ne doit JAMAIS toucher la zone de travail d'une session
    /// réelle déjà en cours.
    pub fn register(hwnd: HWND, edge: u32, rect: RECT) -> Self {
        if std::env::var_os("NEURADESK_NO_APPBAR").is_some() {
            return Self { hwnd, edge, active: false };
        }
        unsafe {
            let mut data = base_data(hwnd);
            SHAppBarMessage(ABM_NEW, &mut data);

            data.uEdge = edge;
            data.rc = rect;
            // On ne passe pas par ABM_QUERYPOS avant de committer : on a
            // déjà calculé `rect` pour éviter la vraie barre des tâches
            // (voir `win32::window::detect_taskbar`), donc pas besoin de
            // laisser le shell l'ajuster. Limite acceptée : pas de
            // cohabitation automatique avec un AUTRE AppBar tiers sur le
            // même bord (cas rare, hors scope de ce prototype).
            //
            // ATTENTION (diagnostiqué en pratique, voir `app::
            // rebuild_secondary_docks`) : si DEUX AppBars distinctes de CE
            // PROCESS revendiquent un rect qui se chevauche (même bord, même
            // écran) au moment de cet appel — p.ex. une ancienne instance
            // encore vivante pendant qu'une nouvelle vient d'être créée pour
            // le même écran — le shell RÉAJUSTE `data.rc` en un rectangle
            // DÉGÉNÉRÉ (épaisseur nulle) plutôt que d'accorder la
            // réservation demandée, sans jamais signaler d'échec explicite.
            // `rebuild_secondary_docks` évite ce chevauchement en réutilisant
            // les instances déjà à jour au lieu de toutes les recréer.
            SHAppBarMessage(ABM_SETPOS, &mut data);
        }
        Self { hwnd, edge, active: true }
    }

    /// Met à jour la réservation d'un AppBar déjà enregistré avec un
    /// nouveau `rect` — voir `win32::window::Window::set_dock_thicknesses`
    /// (curseurs "Largeur barre gauche/droite"/"Épaisseur barre du haut" du
    /// panneau de réglages, demande explicite). PAS de nouveau `ABM_NEW` :
    /// l'AppBar reste le même enregistrement (même `hwnd`), seul `ABM_SETPOS`
    /// suffit à faire bouger sa réservation — `ABM_NEW` créerait un second
    /// enregistrement en double pour le même HWND plutôt que de déplacer le
    /// premier.
    pub fn update(&self, rect: RECT) {
        if !self.active {
            return;
        }
        unsafe {
            let mut data = base_data(self.hwnd);
            data.uEdge = self.edge;
            data.rc = rect;
            SHAppBarMessage(ABM_SETPOS, &mut data);
        }
    }
}

impl Drop for AppBar {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        unsafe {
            let mut data = base_data(self.hwnd);
            SHAppBarMessage(ABM_REMOVE, &mut data);
        }
    }
}

fn base_data(hwnd: HWND) -> APPBARDATA {
    APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        ..Default::default()
    }
}
