//! **Les profils de Firefox** — de quel profil est une fenêtre, et l'icône
//! badgée de son avatar.
//!
//! DEMANDE EXPLICITE (2026-09-25) : « fais pareil pour les profils Firefox »
//! (après les profils Chromium, voir `app_identity::BrowserProfile`).
//!
//! **Firefox ne dit RIEN à Windows de ses profils** — MESURÉ le 2026-09-25 sur
//! deux fenêtres de deux profils :
//! - même AUMID (`308046B0AF4A39CB`), aucune propriété `Relaunch*` ;
//! - même icône de fenêtre (`WM_GETICON`, les trois tailles) ;
//! - même ligne de commande : `firefox.exe --new-instance`. Le sélecteur de
//!   profils relance Firefox par l'environnement, pas par un `--profile`.
//!
//! **Le TITRE, lui, le dit** : `Chez Charles Pizza — vanilla — Mozilla Firefox`.
//! Firefox y insère le nom du profil dès que son groupe en compte plusieurs.
//! Les noms, avatars et couleurs sont dans la base du groupe,
//! `%APPDATA%\Mozilla\Firefox\Profile Groups\<StoreID>.sqlite` (table
//! `Profiles`), lue EN LECTURE SEULE.
//!
//! L'avatar est un SVG de Firefox (`48_<avatar>.svg`, dans `browser\omni.ja`),
//! peint aux couleurs du thème du profil : `context-fill` = fond,
//! `context-stroke` = motif. On le rend par Direct2D (`win32::svg_raster`) et on
//! le pose en bas à droite de l'icône de Firefox, comme le badge que Chrome
//! compose lui-même dans son `Google Profile.ico`.
//!
//! ⚠️ **Aucun profil « par défaut » ne garde l'icône de Firefox** (contrairement
//! à Chromium) : Firefox déplace le défaut de son groupe à CHAQUE fenêtre
//! activée (`SelectableProfileService.setDefaultProfileForGroup`). Toute
//! fenêtre dont le titre nomme un profil connu reçoit donc l'icône de son profil.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::app_identity::{BrowserProfile, OpenWindow};

/// L'exécutable de Firefox.
const FIREFOX_EXE: &str = "firefox.exe";
/// Séparateur des segments du titre d'une fenêtre Firefox — MESURÉ : un tiret
/// cadratin entouré d'espaces.
const TITLE_SEPARATOR: &str = " — ";
/// Préfixe de l'identifiant d'un profil Firefox (voir `BrowserProfile::id`).
const PROFILE_ID_PREFIX: &str = "firefox:";
/// Durée de vie de la liste des profils : `open_windows` tourne chaque seconde,
/// un profil créé dans Firefox n'a pas besoin d'apparaître plus vite.
const PROFILES_TTL: Duration = Duration::from_secs(5);
/// Les avatars dans `omni.ja` : `<préfixe><avatar>.svg`.
const AVATAR_ENTRY_PREFIX: &str = "chrome/browser/content/browser/profiles/assets/48_";
/// Côté de l'icône composée, en pixels — celui des `Google Profile.ico` chargés
/// pour Chrome (`taskbar_pins::ico_file_rgba`).
const BADGED_ICON_SIZE: u32 = 128;
/// Côté du badge, en fraction de l'icône — à l'œil de celui de Chrome.
const BADGE_FRACTION: f32 = 0.56;

/// Un profil du groupe de profils de Firefox.
#[derive(Clone, Debug, PartialEq)]
struct FirefoxProfile {
    /// Chemin de la base (`Profiles\Hr4etNyN.Profil 3`), relatif à la racine
    /// de Firefox — l'identité stable du profil.
    path: String,
    name: String,
    badge: ProfileBadge,
}

/// De quoi composer le badge d'un profil Firefox (voir `badged_icon_rgba`).
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileBadge {
    /// Nom de l'avatar (`star`, `flower`…).
    pub avatar: String,
    /// Couleur du motif (`context-stroke`).
    pub foreground: [u8; 3],
    /// Couleur du disque (`context-fill`).
    pub background: [u8; 3],
}

/// Vrai si `exe` est l'exécutable de Firefox.
pub fn is_firefox(exe: &str) -> bool {
    exe.eq_ignore_ascii_case(FIREFOX_EXE)
}

/// **FONCTION PURE, ET TESTÉE** : vrai si le LANCEUR de Firefox (son épingle)
/// doit disparaître du dock — dès qu'une fenêtre Firefox a son icône de profil.
///
/// DEMANDE EXPLICITE (2026-09-25) : « cacher le lanceur principal dès qu'une
/// fenêtre firefox est ouverte », avec le sélecteur de profils au clic droit
/// des icônes de profil (voir `window_profile`) — ce qui garde un chemin vers
/// un profil pas encore ouvert.
///
/// Une fenêtre Firefox SANS profil reconnu (un groupe d'un seul profil) reste
/// celle de l'épingle, qui ne se cache alors pas : ce serait la cacher elle aussi.
pub fn launcher_hidden(windows: &[OpenWindow]) -> bool {
    windows.iter().any(|w| is_firefox(&w.exe) && w.profile.is_some())
}

/// **Le profil Firefox d'une fenêtre** — `None` si ce n'est pas Firefox, ou si
/// son titre ne nomme aucun profil connu (un groupe à un seul profil).
///
/// Le clic droit de son icône relance Firefox SANS profil (`launch_args` à
/// `None`) : c'est le sélecteur de profils qui s'ouvre (voir
/// `appicon::with_profile_chooser`) — DEMANDE EXPLICITE (2026-09-25), puisque le
/// lanceur de Firefox disparaît alors (voir `launcher_hidden`).
pub fn window_profile(exe: &str, title: &str) -> Option<BrowserProfile> {
    if !is_firefox(exe) {
        return None;
    }
    with_profiles(|profiles| {
        let name = profile_name_in_title(title, profiles.iter().map(|p| p.name.as_str()))?;
        let profile = profiles.iter().find(|p| p.name == name)?;
        Some(BrowserProfile {
            id: format!("{PROFILE_ID_PREFIX}{}", profile.path),
            launch_args: None,
            icon: None,
            badge: Some(profile.badge.clone()),
        })
    })
}

/// **FONCTION PURE, ET TESTÉE** : le nom de profil que porte un titre de
/// fenêtre Firefox — l'avant-dernier segment (le dernier est la marque).
fn profile_name_in_title<'a>(title: &str, mut names: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let mut segments = title.rsplit(TITLE_SEPARATOR);
    segments.next()?;
    let candidate = segments.next()?;
    names.find(|name| *name == candidate)
}

/// Appelle `f` sur les profils connus, relus au plus toutes les `PROFILES_TTL`.
fn with_profiles<T>(f: impl FnOnce(&[FirefoxProfile]) -> Option<T>) -> Option<T> {
    thread_local! {
        static CACHE: RefCell<Option<(Instant, Vec<FirefoxProfile>)>> = const { RefCell::new(None) };
    }
    CACHE.with(|cache| {
        let Ok(mut cache) = cache.try_borrow_mut() else { return None };
        let stale = cache.as_ref().is_none_or(|(read_at, _)| read_at.elapsed() >= PROFILES_TTL);
        if stale {
            *cache = Some((Instant::now(), read_profiles()));
        }
        cache.as_ref().and_then(|(_, profiles)| f(profiles))
    })
}

/// La racine de Firefox de cet utilisateur (`%APPDATA%\Mozilla\Firefox`).
fn firefox_root() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join(r"Mozilla\Firefox"))
}

/// Tous les profils de tous les groupes déclarés dans `profiles.ini`.
fn read_profiles() -> Vec<FirefoxProfile> {
    let Some(root) = firefox_root() else { return Vec::new() };
    let Ok(ini) = std::fs::read_to_string(root.join("profiles.ini")) else { return Vec::new() };
    let mut profiles = Vec::new();
    for store in store_ids(&ini) {
        let database = root.join("Profile Groups").join(format!("{store}.sqlite"));
        let rows = super::notification::read_only_rows(
            &database,
            "SELECT path, name, avatar, themeFg, themeBg FROM Profiles",
            5,
        )
        .unwrap_or_default();
        profiles.extend(rows.into_iter().filter_map(profile_from_row));
    }
    profiles
}

/// **FONCTION PURE, ET TESTÉE** : un profil depuis une rangée
/// `(path, name, avatar, themeFg, themeBg)` de la base du groupe.
fn profile_from_row(row: Vec<String>) -> Option<FirefoxProfile> {
    let [path, name, avatar, foreground, background] = <[String; 5]>::try_from(row).ok()?;
    if path.is_empty() || name.is_empty() {
        return None;
    }
    Some(FirefoxProfile {
        path,
        name,
        badge: ProfileBadge {
            avatar,
            foreground: parse_css_color(&foreground).unwrap_or([255, 255, 255]),
            background: parse_css_color(&background).unwrap_or([28, 27, 34]),
        },
    })
}

/// **FONCTION PURE, ET TESTÉE** : les `StoreID` des groupes de `profiles.ini`,
/// sans doublon.
fn store_ids(ini: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for line in ini.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim();
        let is_safe = !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric());
        if key.trim().eq_ignore_ascii_case("StoreID") && is_safe && !ids.iter().any(|id| id == value) {
            ids.push(value.to_string());
        }
    }
    ids
}

/// **FONCTION PURE, ET TESTÉE** : une couleur CSS de la base de profils —
/// MESURÉ : `rgb(247,202,255)` et `rgba(251, 251, 254, 1)` ; `#rrggbb` aussi.
fn parse_css_color(text: &str) -> Option<[u8; 3]> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix('#') {
        let value = u32::from_str_radix(hex.get(..6)?, 16).ok()?;
        return Some([(value >> 16) as u8, (value >> 8) as u8, value as u8]);
    }
    let inner = text.split_once('(')?.1.split(')').next()?;
    let mut channels = inner.split(',').map(|c| c.trim().parse::<f32>().ok().map(|v| v.clamp(0.0, 255.0).round() as u8));
    Some([channels.next()??, channels.next()??, channels.next()??])
}

// ---------------------------------------------------------------------------
// L'icône badgée
// ---------------------------------------------------------------------------

/// **L'icône d'un profil Firefox** : celle de `firefox_exe`, avec l'avatar du
/// profil en bas à droite. `None` si l'icône de Firefox ne se lit pas ; sans
/// avatar lisible, le badge est un disque aux couleurs du profil.
pub fn badged_icon_rgba(firefox_exe: &Path, badge: &ProfileBadge) -> Option<(u32, u32, Vec<u8>)> {
    let size = BADGED_ICON_SIZE;
    let (width, height, base) = exe_icon_rgba(firefox_exe, size)?;
    let badge_size = (size as f32 * BADGE_FRACTION).round() as u32;
    let svg = avatar_svg(firefox_exe, &badge.avatar)
        .map(|svg| paint_avatar(&svg, badge, badge_size))
        .unwrap_or_else(|| plain_disc_svg(badge, badge_size));
    let Some((_, _, overlay)) = super::svg_raster::svg_rgba(&svg, badge_size) else {
        return Some((width, height, base));
    };
    let composed = compose_bottom_right(base, width, height, &overlay, badge_size);
    Some((width, height, composed))
}

/// L'icône d'un exécutable à `size` pixels — `SHDefExtractIconW`, qui prend
/// la meilleure taille du fichier (`SHGetFileInfoW` s'arrête à 32 px).
fn exe_icon_rgba(path: &Path, size: u32) -> Option<(u32, u32, Vec<u8>)> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::SHDefExtractIconW;
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let mut large = HICON::default();
        SHDefExtractIconW(PCWSTR(wide.as_ptr()), 0, 0, Some(&mut large), None, size).ok().ok()?;
        if large.is_invalid() {
            return None;
        }
        let rgba = crate::gfx::icon::icon_to_rgba(large);
        let _ = DestroyIcon(large);
        rgba
    }
}

/// **FONCTION PURE, ET TESTÉE** : l'avatar peint aux couleurs du profil, et
/// agrandi à `size` (sa `viewBox` fait l'échelle).
fn paint_avatar(svg: &str, badge: &ProfileBadge, size: u32) -> String {
    svg.replace("context-fill", &hex(badge.background))
        .replace("context-stroke", &hex(badge.foreground))
        .replacen(r#"width="48" height="48""#, &format!(r#"width="{size}" height="{size}""#), 1)
}

/// Le badge de repli : un disque à la couleur du profil.
fn plain_disc_svg(badge: &ProfileBadge, size: u32) -> String {
    format!(
        r#"<svg width="{size}" height="{size}" viewBox="0 0 48 48" xmlns="http://www.w3.org/2000/svg"><circle cx="24" cy="24" r="24" fill="{}"/></svg>"#,
        hex(badge.background)
    )
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// **FONCTION PURE, ET TESTÉE** : pose `overlay` (carré de `side`, RGBA droit)
/// en bas à droite de `base`, par-dessus (« over » de Porter-Duff).
fn compose_bottom_right(mut base: Vec<u8>, width: u32, height: u32, overlay: &[u8], side: u32) -> Vec<u8> {
    let (left, top) = (width.saturating_sub(side), height.saturating_sub(side));
    for y in 0..side.min(height) {
        for x in 0..side.min(width) {
            let src = &overlay[((y * side + x) * 4) as usize..][..4];
            let at = (((top + y) * width + left + x) * 4) as usize;
            let dst = &mut base[at..at + 4];
            let (sa, da) = (src[3] as f32 / 255.0, dst[3] as f32 / 255.0);
            let out_a = sa + da * (1.0 - sa);
            if out_a <= 0.0 {
                continue;
            }
            for c in 0..3 {
                let mixed = (src[c] as f32 * sa + dst[c] as f32 * da * (1.0 - sa)) / out_a;
                dst[c] = mixed.round() as u8;
            }
            dst[3] = (out_a * 255.0).round() as u8;
        }
    }
    base
}

// ---------------------------------------------------------------------------
// Les avatars, dans `omni.ja`
// ---------------------------------------------------------------------------

/// Le SVG de l'avatar `avatar`, lu dans le `browser\omni.ja` de CE Firefox.
/// L'archive (plus de 50 Mo) n'est lue qu'UNE fois par session : tous ses
/// avatars sont gardés d'un coup.
fn avatar_svg(firefox_exe: &Path, avatar: &str) -> Option<String> {
    thread_local! {
        static AVATARS: RefCell<Option<HashMap<String, String>>> = const { RefCell::new(None) };
    }
    AVATARS.with(|avatars| {
        let mut avatars = avatars.try_borrow_mut().ok()?;
        let avatars = avatars.get_or_insert_with(|| {
            firefox_exe
                .parent()
                .and_then(|dir| std::fs::read(dir.join(r"browser\omni.ja")).ok())
                .map(|jar| jar_avatars(&jar))
                .unwrap_or_default()
        });
        avatars.get(avatar).cloned()
    })
}

/// **FONCTION PURE, ET TESTÉE** : les avatars `48_*.svg` d'une archive jar.
///
/// ⛔ **Pas `zipfile`/une bibliothèque zip** : `omni.ja` est un jar
/// « optimisé » de Mozilla dont le répertoire central est en TÊTE — MESURÉ :
/// Python le refuse (« Bad magic number »). On lit donc les entrées du
/// répertoire central là où elles sont, par leur signature `PK\x01\x02`.
fn jar_avatars(jar: &[u8]) -> HashMap<String, String> {
    const CENTRAL: &[u8] = b"PK\x01\x02";
    let u16_at = |at: usize| jar.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32_at = |at: usize| jar.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let mut avatars = HashMap::new();
    let mut at = 0;
    while let Some(offset) = jar.get(at..).and_then(|rest| rest.windows(4).position(|w| w == CENTRAL)) {
        let header = at + offset;
        at = header + 4;
        let entry = (|| {
            let method = u16_at(header + 10)?;
            let compressed = u32_at(header + 20)?;
            let name_len = u16_at(header + 28)?;
            let local = u32_at(header + 42)?;
            let name = std::str::from_utf8(jar.get(header + 46..header + 46 + name_len)?).ok()?;
            let avatar = name.strip_prefix(AVATAR_ENTRY_PREFIX)?.strip_suffix(".svg")?;
            let data_at = local + 30 + u16_at(local + 26)? + u16_at(local + 28)?;
            let data = jar.get(data_at..data_at + compressed)?;
            let bytes = match method {
                0 => data.to_vec(),
                8 => miniz_oxide::inflate::decompress_to_vec(data).ok()?,
                _ => return None,
            };
            Some((avatar.to_string(), String::from_utf8(bytes).ok()?))
        })();
        if let Some((avatar, svg)) = entry {
            avatars.insert(avatar, svg);
        }
    }
    avatars
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les titres MESURÉS le 2026-09-25.
    #[test]
    fn profile_name_in_title_reads_the_segment_before_the_brand() {
        let names = ["Profil d’origine", "vanilla", "claude"];
        let find = |title: &str| profile_name_in_title(title, names.iter().copied());
        assert_eq!(find("Chez Charles Pizza — vanilla — Mozilla Firefox"), Some("vanilla"));
        assert_eq!(find("Nouvel onglet — Profil d’origine — Mozilla Firefox"), Some("Profil d’origine"));
        assert_eq!(find("a — b — claude — Mozilla Firefox"), Some("claude"));
        // Une page sans titre : il ne reste que le profil et la marque.
        assert_eq!(find("claude — Mozilla Firefox"), Some("claude"));
        assert_eq!(find("Claude — vanilla — Mozilla Firefox"), Some("vanilla"));
        assert_eq!(find("Nouvel onglet — Mozilla Firefox"), None);
        assert_eq!(find("Mozilla Firefox"), None);
    }

    #[test]
    fn parse_css_color_reads_the_measured_forms() {
        assert_eq!(parse_css_color("rgb(247,202,255)"), Some([247, 202, 255]));
        assert_eq!(parse_css_color("rgba(251, 251, 254, 1)"), Some([251, 251, 254]));
        assert_eq!(parse_css_color("#4c0516"), Some([76, 5, 22]));
        assert_eq!(parse_css_color("rouge"), None);
        assert_eq!(parse_css_color("rgb(1,2)"), None);
    }

    #[test]
    fn store_ids_are_read_once_and_only_when_safe() {
        let ini = "[Profile0]\nStoreID=fcab863e\n[Profile1]\nStoreID=fcab863e\n[Profile2]\nStoreID=..\\x\n";
        assert_eq!(store_ids(ini), vec!["fcab863e".to_string()]);
    }

    #[test]
    fn profile_from_row_reads_the_identity_and_colours() {
        let row = |cells: [&str; 5]| cells.iter().map(|c| c.to_string()).collect::<Vec<_>>();
        let profile = profile_from_row(row([r"Profiles\Hr4etNyN.Profil 3", "claude", "star", "rgb(255,195,201)", "rgb(76,5,22)"])).unwrap();
        assert_eq!(profile.path, r"Profiles\Hr4etNyN.Profil 3");
        assert_eq!(profile.badge.foreground, [255, 195, 201]);
        assert_eq!(profile.badge.background, [76, 5, 22]);
        assert!(profile_from_row(row(["", "x", "star", "", ""])).is_none());
        assert!(profile_from_row(vec!["court".into()]).is_none());
    }

    #[test]
    fn the_firefox_launcher_hides_only_behind_a_profile_window() {
        let window = |exe: &str, profile: bool| OpenWindow {
            hwnd: windows::Win32::Foundation::HWND::default(),
            title: String::new(),
            exe: exe.into(),
            exe_path: String::new(),
            window_aumid: None,
            process_aumid: None,
            profile: profile.then(|| BrowserProfile { id: "firefox:x".into(), launch_args: None, icon: None, badge: None }),
        };
        assert!(!launcher_hidden(&[]));
        assert!(!launcher_hidden(&[window("firefox.exe", false)]));
        assert!(!launcher_hidden(&[window("chrome.exe", true)]));
        assert!(launcher_hidden(&[window("chrome.exe", false), window("Firefox.exe", true)]));
    }

    #[test]
    fn paint_avatar_substitutes_the_theme_colours_and_size() {
        let badge = ProfileBadge { avatar: "star".into(), foreground: [255, 195, 201], background: [76, 5, 22] };
        let svg = r#"<svg width="48" height="48" viewBox="0 0 48 48"><path fill="context-fill"/><path fill="context-stroke"/></svg>"#;
        assert_eq!(
            paint_avatar(svg, &badge, 72),
            r##"<svg width="72" height="72" viewBox="0 0 48 48"><path fill="#4c0516"/><path fill="#ffc3c9"/></svg>"##
        );
    }

    #[test]
    fn compose_bottom_right_blends_over_the_corner_only() {
        // Base 2×2 rouge opaque, badge 1×1 bleu à moitié transparent.
        let base = [255, 0, 0, 255].repeat(4);
        let out = compose_bottom_right(base, 2, 2, &[0, 0, 255, 128], 1);
        assert_eq!(&out[..12], &[255, 0, 0, 255].repeat(3)[..]);
        assert_eq!(&out[12..], &[127, 0, 128, 255]);
    }

    /// Une archive d'une entrée STOCKÉE, d'une DÉFLATÉE et d'une étrangère,
    /// répertoire central en TÊTE comme `omni.ja`.
    #[test]
    fn jar_avatars_reads_stored_and_deflated_entries() {
        let star = "<svg>étoile</svg>";
        let flower = "<svg>fleur</svg>";
        let deflated = miniz_oxide::deflate::compress_to_vec(flower.as_bytes(), 6);
        let entries: [(String, u16, &[u8]); 3] = [
            (format!("{AVATAR_ENTRY_PREFIX}star.svg"), 0, star.as_bytes()),
            (format!("{AVATAR_ENTRY_PREFIX}flower.svg"), 8, &deflated),
            ("autre/chose.svg".to_string(), 0, b"x"),
        ];
        let mut offset = entries.iter().map(|(name, _, _)| 46 + name.len()).sum::<usize>();
        let (mut central, mut locals) = (Vec::new(), Vec::new());
        for (name, method, data) in &entries {
            central.extend(b"PK\x01\x02");
            central.extend([0u8; 6]);
            central.extend(method.to_le_bytes());
            central.extend([0u8; 8]);
            central.extend((data.len() as u32).to_le_bytes());
            central.extend([0u8; 4]);
            central.extend((name.len() as u16).to_le_bytes());
            central.extend([0u8; 12]);
            central.extend((offset as u32).to_le_bytes());
            central.extend(name.as_bytes());
            let mut local = b"PK\x03\x04".to_vec();
            local.extend([0u8; 22]);
            local.extend((name.len() as u16).to_le_bytes());
            local.extend(0u16.to_le_bytes());
            local.extend(name.as_bytes());
            local.extend(*data);
            offset += local.len();
            locals.extend(local);
        }
        central.extend(locals);
        let avatars = jar_avatars(&central);
        assert_eq!(avatars.len(), 2);
        assert_eq!(avatars.get("star").map(String::as_str), Some(star));
        assert_eq!(avatars.get("flower").map(String::as_str), Some(flower));
    }
}
