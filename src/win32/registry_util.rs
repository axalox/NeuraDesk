//! Petits utilitaires du registre Windows, partagés par les scanners de
//! bibliothèques de jeux (`win32::steam_apps`, `win32::gog_apps`
//! — voir aussi `win32::apps::enumerate_apps`, qui fusionne leurs résultats)
//! : de quoi lire une valeur chaîne, énumérer les sous-clés d'une clé donnée,
//! et — depuis la bascule du thème clair/sombre — **écrire** (un `REG_DWORD`,
//! puis un `REG_BINARY` pour l'éclairage nocturne, puis un `REG_SZ` pour
//! l'identité des notifications).
//! Pas un wrapper générique complet du registre. `win32::theme` lit aussi le
//! registre (couleur d'accentuation, thème sombre) mais via `RegGetValueW`
//! directement sur des valeurs connues à l'avance — pas besoin d'énumérer des
//! sous-clés dont le nom n'est pas connu d'avance (contrairement à
//! `HKLM\...\GOG.com\Games\<id>`, un identifiant par jeu), d'où ces
//! utilitaires séparés plutôt qu'une extension de ce module-là.
//!
//! ⚠️ **CE MODULE LIT, ET N'ÉCRIT QUE PAR DES FONCTIONS NOMMÉES D'ICI.**
//! La distinction n'est pas cosmétique : une lecture est sans conséquence, une
//! écriture change l'état du **système** de l'utilisateur. Jamais un
//! `RegSetValueExW` dispersé chez un appelant, où personne ne le verrait.
//!
//! **Les cinq écritures du dépôt, et ce qu'elles engagent** — à voir ensemble :
//!
//! | fonction | ce qu'elle change |
//! |---|---|
//! | `write_dword_value` | le thème clair/sombre |
//! | `write_binary_value` | l'éclairage nocturne |
//! | `write_string_value` | l'identité des notifications (`AppUserModelId`) |
//! | `delete_value` | **rend** l'état d'avant au lieu d'en poser un |
//!
//! ⚠️ **`delete_value` EXISTE POUR ÇA, ET PAS PAR SYMÉTRIE** : restaurer
//! `ToastEnabled` en écrivant `1` laisserait une valeur que l'utilisateur
//! n'avait pas. Une suspension doit pouvoir se défaire **exactement**.

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
    RegSetValueExW, HKEY, KEY_READ, KEY_SET_VALUE, REG_BINARY, REG_DWORD, REG_OPTION_NON_VOLATILE,
    REG_SZ, RRF_RT_REG_BINARY,
    RRF_RT_REG_SZ,
};

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Lit une valeur chaîne (`REG_SZ`) sous `root\subkey`, nommée `value_name`
/// — `None` si la clé/la valeur n'existe pas, n'est pas une chaîne, ou si
/// une étape quelconque échoue (best-effort, voir les appelants : l'absence
/// d'un jeu/launcher installé n'est pas une erreur, juste une liste vide).
pub fn read_string_value(root: HKEY, subkey: &str, value_name: &str) -> Option<String> {
    let subkey_wide = to_wide(subkey);
    let value_wide = to_wide(value_name);
    unsafe {
        // Premier appel : juste la taille nécessaire (`pvdata: None`) — la
        // même API en deux temps que `win32::theme::accent_palette`, mais
        // sans connaître la taille à l'avance ici (une chaîne, pas un
        // bloc binaire de taille fixe).
        let mut size: u32 = 0;
        let status = RegGetValueW(
            root,
            PCWSTR(subkey_wide.as_ptr()),
            PCWSTR(value_wide.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        );
        if status != ERROR_SUCCESS || size == 0 {
            return None;
        }

        let mut buffer = vec![0u8; size as usize];
        let status = RegGetValueW(
            root,
            PCWSTR(subkey_wide.as_ptr()),
            PCWSTR(value_wide.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr() as *mut _),
            Some(&mut size),
        );
        if status != ERROR_SUCCESS {
            return None;
        }

        // `buffer` contient de l'UTF-16 (deux octets par unité) terminé par
        // un caractère nul — reconstruit le `&[u16]` puis tronque dessus.
        let units = size as usize / 2;
        let wide = std::slice::from_raw_parts(buffer.as_ptr() as *const u16, units);
        let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
        Some(String::from_utf16_lossy(&wide[..end]))
    }
}

/// Lit un entier (`REG_DWORD`) sous `root\subkey`, nommé `value_name` — `None`
/// si la clé/la valeur n'existe pas (best-effort, même raisonnement que
/// `read_string_value`).
pub fn read_dword_value(root: HKEY, subkey: &str, value_name: &str) -> Option<u32> {
    let subkey_wide = to_wide(subkey);
    let value_wide = to_wide(value_name);
    let mut valeur: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            root,
            PCWSTR(subkey_wide.as_ptr()),
            PCWSTR(value_wide.as_ptr()),
            windows::Win32::System::Registry::RRF_RT_REG_DWORD,
            None,
            Some(&mut valeur as *mut u32 as *mut core::ffi::c_void),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then_some(valeur)
}

/// Lit une valeur binaire (`REG_BINARY`) sous `root\subkey`, nommée
/// `value_name` — `None` si la clé/la valeur n'existe pas ou si une étape
/// quelconque échoue (best-effort, même raisonnement que `read_string_value`).
///
/// Utilisé pour `Taskband\FavoritesResolve`, le blob opaque où Windows range
/// les épingles de sa barre des tâches **et leur ordre** — voir
/// `win32::taskbar_pins`, qui explique comment on s'en sert.
pub fn read_binary_value(root: HKEY, subkey: &str, value_name: &str) -> Option<Vec<u8>> {
    let subkey_wide = to_wide(subkey);
    let value_wide = to_wide(value_name);
    unsafe {
        // Même API en deux temps que `read_string_value` : d'abord la taille.
        let mut size: u32 = 0;
        let status = RegGetValueW(
            root,
            PCWSTR(subkey_wide.as_ptr()),
            PCWSTR(value_wide.as_ptr()),
            RRF_RT_REG_BINARY,
            None,
            None,
            Some(&mut size),
        );
        // `size == 0` : la valeur existe mais est vide — une liste d'épingles
        // vide est un résultat légitime, pas une erreur, d'où le `vec![]` et
        // non un `None` (l'appelant doit pouvoir distinguer « pas de clé » de
        // « clé présente mais vide »).
        if status != ERROR_SUCCESS {
            return None;
        }
        if size == 0 {
            return Some(Vec::new());
        }

        let mut buffer = vec![0u8; size as usize];
        let status = RegGetValueW(
            root,
            PCWSTR(subkey_wide.as_ptr()),
            PCWSTR(value_wide.as_ptr()),
            RRF_RT_REG_BINARY,
            None,
            Some(buffer.as_mut_ptr() as *mut _),
            Some(&mut size),
        );
        if status != ERROR_SUCCESS {
            return None;
        }
        buffer.truncate(size as usize);
        Some(buffer)
    }
}

/// Énumère les noms des sous-clés directes de `root\subkey` — `Vec::new()`
/// si la clé n'existe pas ou est vide (best-effort, même raisonnement que
/// `read_string_value`).
pub fn enumerate_subkeys(root: HKEY, subkey: &str) -> Vec<String> {
    let subkey_wide = to_wide(subkey);
    unsafe {
        let mut hkey = HKEY::default();
        let opened = RegOpenKeyExW(root, PCWSTR(subkey_wide.as_ptr()), Some(0), KEY_READ, &mut hkey);
        if opened != ERROR_SUCCESS {
            return Vec::new();
        }

        let mut names = Vec::new();
        let mut index = 0u32;
        loop {
            // 256 caractères larges : bien au-delà de la longueur d'un
            // identifiant de jeu (les clés vues en pratique sont des GUID
            // ou de courts identifiants numériques) — `RegEnumKeyExW`
            // renverrait de toute façon une erreur si jamais trop court,
            // ce n'est donc pas un plafond silencieusement tronqué.
            let mut name_buf = [0u16; 256];
            let mut name_len = name_buf.len() as u32;
            let status = RegEnumKeyExW(
                hkey,
                index,
                Some(PWSTR(name_buf.as_mut_ptr())),
                &mut name_len,
                None,
                Some(PWSTR::null()),
                None,
                None,
            );
            if status != ERROR_SUCCESS {
                break; // `ERROR_NO_MORE_ITEMS` en fin d'énumération, ou une vraie erreur — les deux arrêtent la boucle
            }
            names.push(String::from_utf16_lossy(&name_buf[..name_len as usize]));
            index += 1;
        }

        let _ = RegCloseKey(hkey);
        names
    }
}

/// **Écrit une valeur `REG_DWORD`** sous `root\subkey`, nommée `value_name` —
/// `true` si Windows a accepté l'écriture.
///
/// ⚠️ **PENDANT LONGTEMPS LA SEULE ÉCRITURE AU REGISTRE DE TOUT LE DÉPÔT** —
/// elle ne l'est plus depuis que `write_binary_value` (éclairage nocturne) et
/// `write_string_value` (identité des notifications) l'ont rejointe. Les trois
/// sont ici, nommées et voisines, pour qu'on les voie ensemble : ce sont les
/// seules fonctions du dépôt qui touchent à l'état du SYSTÈME. Un appelant qui écrit modifie l'état du
/// SYSTÈME de l'utilisateur — pas une préférence de NeuraDesk (celles-là vont
/// dans `%APPDATA%`, voir `config::write_config`).
///
/// **Crée la clé au besoin** (`RegCreateKeyExW`) : `…\Themes\Personalize`
/// existe sur toute installation Windows, mais s'en remettre à ce fait serait
/// une supposition de plus.
///
/// **Un refus rend `false`, il ne lève rien** — même doctrine que
/// `win32::power::enable_shutdown_privilege` : une stratégie de groupe peut
/// interdire l'écriture, et l'appelant doit alors RENONCER à la suite de
/// l'action plutôt que de la tenter sur un état qui n'a pas été écrit. Le
/// motif de l'échec n'est pas distinguable ici (le statut n'est pas rendu) :
/// c'est l'appelant qui journalise, par `diag!`.
pub fn write_dword_value(root: HKEY, subkey: &str, value_name: &str, value: u32) -> bool {
    let subkey_wide = to_wide(subkey);
    let value_wide = to_wide(value_name);
    unsafe {
        let mut hkey = HKEY::default();
        // `Some(0)` : pas de clé de destination dans un autre emplacement.
        // `KEY_SET_VALUE` est le SEUL droit nécessaire pour poser une valeur
        // sur une clé existante ou nouvellement créée.
        let created = RegCreateKeyExW(
            root,
            PCWSTR(subkey_wide.as_ptr()),
            Some(0),
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        );
        if created != ERROR_SUCCESS {
            return false;
        }
        // La taille d'un `REG_DWORD` est celle de son type, jamais celle d'un
        // tampon à interroger au préalable.
        let bytes = value.to_le_bytes();
        let written = RegSetValueExW(hkey, PCWSTR(value_wide.as_ptr()), Some(0), REG_DWORD, Some(&bytes));
        let _ = RegCloseKey(hkey);
        written == ERROR_SUCCESS
    }
}

/// **SUPPRIME UNE VALEUR** — l'exact pendant de `write_dword_value`, et la seule
/// façon de **rendre** un état au lieu d'en poser un : écrire `1` à la place de
/// `0` laisserait une valeur que l'utilisateur n'avait pas.
///
/// ⚠️ **UNE VALEUR DÉJÀ ABSENTE N'EST PAS UN ÉCHEC** : c'est le cas NORMAL d'une
/// restauration après un arrêt propre. Rendre `false` ferait journaliser une
/// panne là où il n'y en a pas — `ERROR_FILE_NOT_FOUND` est donc rendu `true`,
/// pour la valeur comme pour la clé.
///
/// ⚠️ **LA CLÉ N'EST PAS SUPPRIMÉE**, elle non plus : elle appartient à Windows
/// (`…\PushNotifications` existe sur toute installation), et la retirer serait
/// défaire plus que ce qu'on a fait.
pub fn delete_value(root: HKEY, subkey: &str, value_name: &str) -> bool {
    let subkey_wide = to_wide(subkey);
    let value_wide = to_wide(value_name);
    unsafe {
        let mut hkey = HKEY::default();
        // `KEY_SET_VALUE` est le droit qui couvre la suppression d'une valeur —
        // le même que pour l'écriture, et rien de plus.
        let opened =
            RegOpenKeyExW(root, PCWSTR(subkey_wide.as_ptr()), Some(0), KEY_SET_VALUE, &mut hkey);
        if opened != ERROR_SUCCESS {
            return opened == ERROR_FILE_NOT_FOUND;
        }
        let deleted = RegDeleteValueW(hkey, PCWSTR(value_wide.as_ptr()));
        let _ = RegCloseKey(hkey);
        deleted == ERROR_SUCCESS || deleted == ERROR_FILE_NOT_FOUND
    }
}

/// **Écrit une valeur `REG_BINARY` — l'exact pendant de `write_dword_value`, pour
/// un blob et non un entier.**
///
/// Écrite pour l'éclairage nocturne (voir `win32::night_light`), dont Windows ne
/// range l'état **que** sous cette forme : un blob CloudStore relu et réécrit octet
/// par octet. Mêmes garanties, mêmes limites :
///
/// * `false` = « l'écriture a été refusée » (stratégie de groupe, droits) — et
///   l'appelant doit alors RENONCER à la suite de son action plutôt que de la
///   poursuivre sur un état qui n'a pas été écrit ;
/// * la clé est CRÉÉE si elle manque, comme `write_dword_value` : le réglage est
///   censé exister, mais refuser d'écrire parce que la clé s'appelle autrement sur
///   une autre version de Windows serait pire que la créer.
///
/// Un `data` VIDE est refusé par `RegSetValueExW` (une valeur de longueur nulle
/// n'existe pas sous ce type) : c'est un refus, pas un effacement — l'appelant ne
/// doit jamais vouloir écrire un blob vide, et s'il le fait, le `false` le lui dit.
pub fn write_binary_value(root: HKEY, subkey: &str, value_name: &str, data: &[u8]) -> bool {
    let subkey_wide = to_wide(subkey);
    let value_wide = to_wide(value_name);
    unsafe {
        let mut hkey = HKEY::default();
        let created = RegCreateKeyExW(
            root,
            PCWSTR(subkey_wide.as_ptr()),
            Some(0),
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        );
        if created != ERROR_SUCCESS {
            return false;
        }
        // La longueur est celle du tampon, ici — contrairement à un `REG_DWORD`
        // dont la taille est celle du type.
        let written = RegSetValueExW(hkey, PCWSTR(value_wide.as_ptr()), Some(0), REG_BINARY, Some(data));
        let _ = RegCloseKey(hkey);
        written == ERROR_SUCCESS
    }
}

/// **Écrit une valeur `REG_SZ`** sous `root\subkey` — même contrat que
/// `write_dword_value` : la clé est CRÉÉE au besoin, un refus rend `false` sans
/// rien lever.
///
/// Écrite pour les toasts (voir `win32::bulle`) : `AppUserModelId\NeuraDesk`
/// porte le NOM et l'ICÔNE sous lesquels Windows présente nos notifications, et
/// Windows ne les range que sous cette forme (deux chaînes).
///
/// ⚠️ **LA CHAÎNE EST TERMINÉE PAR UN ZÉRO, ET SA LONGUEUR LE COMPTE.** Un
/// `REG_SZ` n'est pas un tableau d'octets quelconque : `RegSetValueExW` attend
/// l'octet nul final DANS la longueur. L'oublier fait lire à Windows une chaîne
/// qui continue dans la mémoire voisine.
pub fn write_string_value(root: HKEY, subkey: &str, value_name: &str, value: &str) -> bool {
    let subkey_wide = to_wide(subkey);
    let value_wide = to_wide(value_name);
    let texte_wide = to_wide(value);
    // Les octets UTF-16, zéro final COMPRIS.
    let mut octets = Vec::with_capacity(texte_wide.len() * 2);
    for unite in &texte_wide {
        octets.extend_from_slice(&unite.to_le_bytes());
    }
    unsafe {
        let mut hkey = HKEY::default();
        let created = RegCreateKeyExW(
            root,
            PCWSTR(subkey_wide.as_ptr()),
            Some(0),
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        );
        if created != ERROR_SUCCESS {
            return false;
        }
        let written = RegSetValueExW(hkey, PCWSTR(value_wide.as_ptr()), Some(0), REG_SZ, Some(&octets));
        let _ = RegCloseKey(hkey);
        written == ERROR_SUCCESS
    }
}
