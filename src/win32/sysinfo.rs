//! Lecture légère de l'utilisation CPU/RAM du système, pour le petit
//! indicateur façon extension GNOME "Vitals" affiché dans la barre du haut
//! (voir `ui::topbar::sys_monitor_rect`, `app::run`). Uniquement des API
//! Win32 déjà disponibles (`GetSystemTimes`, `GlobalMemoryStatusEx`), sans
//! dépendance externe de type `sysinfo`.

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

fn filetime_to_u64(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

/// Échantillonneur d'utilisation CPU : `GetSystemTimes` ne renvoie que des
/// totaux cumulés depuis le démarrage de Windows, il faut donc comparer
/// deux relevés pour en tirer un pourcentage instantané — cet état se garde
/// d'un appel à l'autre (un seul `CpuSampler` vit pendant toute la boucle
/// principale, voir `app::run`).
#[derive(Default)]
pub struct CpuSampler {
    last_idle: u64,
    last_total: u64,
    /// `false` tant qu'aucun relevé de référence n'a été pris : le tout
    /// premier `sample()` ne peut pas encore calculer de delta.
    has_baseline: bool,
}

impl CpuSampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pourcentage d'utilisation CPU (0..100) écoulé depuis le dernier
    /// appel à cette méthode. `None` au tout premier appel (pas encore de
    /// relevé précédent pour calculer un delta) ou en cas d'échec de
    /// l'appel système.
    pub fn sample(&mut self) -> Option<f32> {
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).ok()? };

        let idle = filetime_to_u64(idle);
        // `kernel` inclut le temps idle par convention Win32 : le temps CPU
        // réellement occupé est donc kernel + user - idle.
        let total = filetime_to_u64(kernel) + filetime_to_u64(user);

        let (prev_idle, prev_total) = (self.last_idle, self.last_total);
        let had_baseline = self.has_baseline;
        self.last_idle = idle;
        self.last_total = total;
        self.has_baseline = true;

        if !had_baseline {
            return None;
        }
        let total_delta = total.saturating_sub(prev_total) as f32;
        if total_delta <= 0.0 {
            return Some(0.0);
        }
        let idle_delta = idle.saturating_sub(prev_idle) as f32;
        Some(((total_delta - idle_delta) / total_delta * 100.0).clamp(0.0, 100.0))
    }
}

/// Pourcentage de RAM physique actuellement utilisée (0..100), déjà calculé
/// par Windows lui-même (`dwMemoryLoad`). `None` si l'appel système échoue
/// (ne devrait jamais arriver en pratique).
pub fn ram_usage_percent() -> Option<f32> {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut status).ok()? };
    Some(status.dwMemoryLoad as f32)
}
