//! Lecture des métadonnées de version d'un exécutable (nom d'éditeur) —
//! utilisée par `win32::app_categories` comme signal de repli quand le nom
//! de l'application seul ne permet de la classer dans aucun thème d'usage
//! (voir sa doc, `classify_by_publisher`) : un seul éditeur connu
//! ("Ubisoft", "Adobe", "Valve"...) couvre souvent plusieurs dizaines de
//! logiciels d'un coup, contrairement à des mots-clés par nom
//! d'application qui ne couvrent qu'un logiciel à la fois.
//!
//! Ces informations sont celles que l'Explorateur Windows affiche dans
//! l'onglet "Détails" des propriétés d'un fichier — un bloc de ressource
//! `VERSIONINFO` que la plupart des exécutables commerciaux embarquent,
//! mais pas tous (un exécutable "maison"/open-source minimaliste peut très
//! bien ne rien avoir : traité comme une absence d'information plutôt
//! qu'une erreur, best-effort comme le reste de `win32::apps`).

use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};

/// Nom de l'éditeur ("CompanyName") embarqué dans les métadonnées de
/// version de l'exécutable `path`, ou `None` si absent (pas de bloc
/// `VERSIONINFO`, champ vide, ou toute autre lecture en échec).
pub fn company_name(path: &Path) -> Option<String> {
    read_string_field(path, "CompanyName")
}

/// Lit le champ `field` (ex. `"CompanyName"`, `"ProductName"`) du bloc
/// `StringFileInfo` de `path`, via la même API que `rundll32
/// shell32.dll,Control_RunDLL` (propriétés de fichier > onglet Détails)
/// utilise en interne : `GetFileVersionInfoSizeW`/`GetFileVersionInfoW`
/// récupèrent le bloc `VERSIONINFO` brut, `VerQueryValueW` y navigue.
fn read_string_field(path: &Path, field: &str) -> Option<String> {
    let wide_path: Vec<u16> = {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    };

    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(wide_path.as_ptr()), None);
        if size == 0 {
            return None; // pas de bloc VERSIONINFO — cas normal, pas une erreur
        }

        let mut buffer = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(wide_path.as_ptr()), Some(0), size, buffer.as_mut_ptr() as *mut _).ok()?;
        let block = buffer.as_ptr() as *const core::ffi::c_void;

        // Bloc "Translation" : donne la paire (langue, page de code) à
        // utiliser pour former le chemin du champ voulu — la plupart des
        // exécutables n'en ont qu'une seule, mais rien ne l'impose ; on ne
        // regarde que la première, suffisant dans l'immense majorité des
        // cas réels.
        let mut translation_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut translation_len: u32 = 0;
        let translation_key: Vec<u16> =
            "\\VarFileInfo\\Translation".encode_utf16().chain(std::iter::once(0)).collect();
        let found_translation =
            VerQueryValueW(block, PCWSTR(translation_key.as_ptr()), &mut translation_ptr, &mut translation_len);
        if !found_translation.as_bool() || translation_ptr.is_null() || translation_len < 4 {
            return None;
        }
        let translation = translation_ptr as *const u16;
        let lang_id = *translation;
        let code_page = *translation.add(1);

        let field_key: Vec<u16> = format!("\\StringFileInfo\\{lang_id:04x}{code_page:04x}\\{field}")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let mut value_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut value_len: u32 = 0;
        let found_value = VerQueryValueW(block, PCWSTR(field_key.as_ptr()), &mut value_ptr, &mut value_len);
        if !found_value.as_bool() || value_ptr.is_null() || value_len == 0 {
            return None;
        }

        let text = std::slice::from_raw_parts(value_ptr as *const u16, value_len as usize);
        let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
        let value = String::from_utf16_lossy(&text[..end]);
        (!value.trim().is_empty()).then_some(value)
    }
}
