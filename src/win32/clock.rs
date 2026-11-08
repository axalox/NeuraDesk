//! Formatage de l'heure courante pour l'horloge de la barre du haut, au
//! format court configuré par l'utilisateur dans les paramètres régionaux
//! Windows (24h ou 12h, sans les secondes) — le même format que la
//! barre des tâches Windows ou l'horloge GNOME utilisent par défaut.

use windows::Win32::Globalization::{GetTimeFormatEx, TIME_NOSECONDS};

/// Heure actuelle formatée selon les paramètres régionaux de l'utilisateur
/// (ex. "14:32" ou "2:32 PM"). Repli sur "--:--" si l'appel système échoue
/// (ne devrait jamais arriver en pratique).
pub fn format_now() -> String {
    unsafe {
        let mut buf = [0u16; 32];
        // `lplocalename: None` = LOCALE_NAME_USER_DEFAULT, `lptime: None` =
        // heure locale courante, `lpformat: None` = format court par
        // défaut du terme régional (ce que `TIME_NOSECONDS` réduit).
        let len = GetTimeFormatEx(None, TIME_NOSECONDS, None, None, Some(&mut buf));
        if len <= 0 {
            return String::from("--:--");
        }
        // `len` inclut le caractère NUL terminal.
        String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)])
    }
}
