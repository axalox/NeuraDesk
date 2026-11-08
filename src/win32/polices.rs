//! **LES POLICES INSTALLÉES** — la liste que Réglages propose pour remplacer la
//! police du style (voir `ui::style::choisir_police`).
//!
//! DEMANDE EXPLICITE (2026-10-12) : « serait-il possible de choisir la police de
//! caractère à utiliser dans les réglages ».
//!
//! ⚠️ **PAR GDI, ET C'EST VOULU** : le texte que Rust rend lui-même passe par
//! `CreateFontW` (`gfx::text`), donc une famille que GDI ne nomme pas ne
//! s'afficherait pas là — et Chromium, lui, trouve toute famille que GDI connaît.
//!
//! ⚠️ **LES POLICES DE L'UTILISATEUR SONT LÀ AUSSI** (`%LOCALAPPDATA%\Microsoft\
//! Windows\Fonts`, enregistrées sous `HKCU`) : GDI les énumère comme les autres.
//!
//! ⛔ **UNE POLICE INSTALLÉE APRÈS L'OUVERTURE DE RÉGLAGES N'Y EST PAS** : la liste
//! est relue à chaque ouverture du panneau (`rafraichir`), pas à chaque frame —
//! l'énumération coûte quelques millisecondes, et le panneau pousse son état à
//! 60 Hz.

use std::collections::BTreeSet;
use std::sync::Mutex;

use windows::Win32::Foundation::LPARAM;
use windows::Win32::Graphics::Gdi::{
    EnumFontFamiliesExW, GetDC, ReleaseDC, DEFAULT_CHARSET, LOGFONTW, TEXTMETRICW,
};

static LISTE: Mutex<Option<Vec<String>>> = Mutex::new(None);

unsafe extern "system" fn une_famille(lf: *const LOGFONTW, _: *const TEXTMETRICW, _: u32, lparam: LPARAM) -> i32 {
    // SAFETY : `lparam` est le `&mut BTreeSet` d'`enumerer`, vivant pendant tout
    // l'appel d'`EnumFontFamiliesExW` ; `lf` est fourni par GDI.
    let noms = unsafe { &mut *(lparam.0 as *mut BTreeSet<String>) };
    let face = unsafe { &(*lf).lfFaceName };
    let fin = face.iter().position(|&c| c == 0).unwrap_or(face.len());
    noms.insert(String::from_utf16_lossy(&face[..fin]));
    1
}

/// Les familles que GDI connaît, triées, sans doublon.
fn enumerer() -> Vec<String> {
    let mut noms = BTreeSet::new();
    let lf = LOGFONTW { lfCharSet: DEFAULT_CHARSET, ..Default::default() };
    unsafe {
        let hdc = GetDC(None);
        EnumFontFamiliesExW(hdc, &lf, Some(une_famille), LPARAM(&mut noms as *mut _ as isize), 0);
        ReleaseDC(None, hdc);
    }
    filtrer(noms)
}

/// **CE QUI EST PROPOSÉ** : ni les familles VERTICALES (`@MS Gothic`, le même
/// dessin couché pour l'écriture verticale), ni un nom que `ui::style` refuserait
/// — il finit dans une variable CSS (voir `style::police_valide`).
fn filtrer(noms: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut liste: Vec<String> =
        noms.into_iter().filter(|n| !n.starts_with('@') && crate::ui::style::police_valide(n)).collect();
    liste.sort_by_key(|n| n.to_lowercase());
    liste.dedup();
    liste
}

/// Relit la liste — à l'ouverture de Réglages.
pub fn rafraichir() {
    let liste = enumerer();
    crate::diag!("[polices] {} familles installées", liste.len());
    *LISTE.lock().unwrap_or_else(|e| e.into_inner()) = Some(liste);
}

/// La liste, énumérée au premier appel si `rafraichir` n'a pas encore tourné.
pub fn installees() -> Vec<String> {
    let mut garde = LISTE.lock().unwrap_or_else(|e| e.into_inner());
    garde.get_or_insert_with(enumerer).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_familles_verticales_et_les_noms_hostiles_sont_ecartes() {
        let noms = ["Segoe UI", "@MS Gothic", "Arial", "a'b", "arial", "Zed Mono"].map(String::from);
        assert_eq!(filtrer(noms), ["Arial", "arial", "Segoe UI", "Zed Mono"]);
    }

    /// Sur une machine Windows, Segoe UI est TOUJOURS là : c'est la police du
    /// système. Une liste qui ne la contient pas dit que l'énumération est cassée.
    #[test]
    fn la_police_du_systeme_est_enumeree() {
        let liste = enumerer();
        assert!(liste.iter().any(|n| n == "Segoe UI"), "{} familles, sans Segoe UI", liste.len());
        assert!(liste.iter().all(|n| !n.starts_with('@')));
    }
}
