//! ⭐ **LES JEUX LANCÉS EN FENÊTRE DANS LA ZONE CENTRALE** — DEMANDE EXPLICITE
//! (2026-10-16) : « forcer le mode plein écran exclusif à s'afficher dans la zone
//! centrale de l'application », voie B retenue : SANS injection de code.
//!
//! Un plein écran exclusif ne se redirige pas de l'extérieur (voir
//! `win32::plein_ecran_zone`). On l'évite donc AU LANCEMENT : quand le dock lance
//! un jeu dont il reconnaît le moteur, il ajoute les options de ligne de commande
//! qui le démarrent en fenêtre, à la taille de la zone ; puis il guette sa fenêtre,
//! lui retire son cadre et la pose dans la zone. Exactement ce qu'on taperait dans
//! les options de lancement de Steam — rien que les anti-triche puissent prendre
//! pour une attaque.
//!
//! # Ce qui n'est pas couvert
//!
//! - Un jeu lancé AILLEURS que depuis NeuraDesk, ou par un lanceur (`steam://…`,
//!   `steam.exe -applaunch`, Epic…) : on ne voit pas son exécutable.
//! - Un moteur maison, qui ne connaît pas ces options.
//! - Des options déjà choisies : un lancement qui porte déjà un mode d'affichage
//!   (`-windowed`, `-fullscreen`, `-screen-fullscreen`…) n'est pas touché.
//!
//! La zone est lue AU LANCEMENT, sur l'écran principal, en pixels réels (le
//! processus est « per-monitor aware », voir `win32::dpi`) : rien n'est supposé
//! de l'écran ni de sa mise à l'échelle.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, SetWindowLongPtrW,
    SetWindowPos, GWL_STYLE, GW_OWNER, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER, WS_CAPTION,
    WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
};

/// Le temps laissé au jeu pour ouvrir sa fenêtre (les lanceurs et écrans
/// d'accueil sont lents).
const ATTENTE_FENETRE: Duration = Duration::from_secs(120);
/// Une fois trouvée, la fenêtre est maintenue dans la zone ce temps-là : un jeu
/// se redimensionne souvent pendant son initialisation.
const MAINTIEN: Duration = Duration::from_secs(15);
const CADENCE: Duration = Duration::from_millis(500);
/// Plus petite qu'un de ces côtés, c'est un écran d'accueil ou une boîte, pas
/// la fenêtre du jeu.
const COTE_MIN: i32 = 200;

static ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn est_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn activer(active: bool) {
    ACTIVE.store(active, Ordering::Relaxed);
}

/// Les moteurs reconnus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Moteur {
    Unity,
    Unreal,
    Source,
}

/// Reconnaît le moteur d'un exécutable à ses fichiers voisins.
fn detecter(exe: &Path) -> Option<Moteur> {
    let dossier = exe.parent()?;
    let nom = exe.file_stem()?.to_string_lossy();
    if dossier.join("UnityPlayer.dll").is_file() || dossier.join(format!("{nom}_Data")).is_dir() {
        return Some(Moteur::Unity);
    }
    // Le jeu lui-même (`Jeu-Win64-Shipping.exe`), ou son lanceur à la racine, à
    // côté du dossier `Engine`.
    let livre = nom.to_ascii_lowercase().ends_with("-shipping");
    if livre || dossier.join("Engine").join("Binaries").is_dir() {
        return Some(Moteur::Unreal);
    }
    if dossier.join("bin").join("engine.dll").is_file() {
        return Some(Moteur::Source);
    }
    None
}

/// Les options qui démarrent le moteur en fenêtre, à la taille de `zone`.
fn options(moteur: Moteur, zone: RECT) -> String {
    let (l, h) = (zone.right - zone.left, zone.bottom - zone.top);
    match moteur {
        Moteur::Unity => format!("-screen-fullscreen 0 -popupwindow -screen-width {l} -screen-height {h}"),
        Moteur::Unreal => format!("-windowed -ResX={l} -ResY={h} -WinX={} -WinY={}", zone.left, zone.top),
        Moteur::Source => format!("-windowed -noborder -w {l} -h {h}"),
    }
}

/// Un lancement qui choisit déjà son mode d'affichage n'est pas touché.
fn choisit_deja_son_affichage(arguments: &str) -> bool {
    const MODES: [&str; 6] = ["-windowed", "-fullscreen", "-screen-fullscreen", "-popupwindow", "-window-mode", "-sw"];
    arguments
        .split_whitespace()
        .map(|a| a.split('=').next().unwrap_or(a).to_ascii_lowercase())
        .any(|a| MODES.contains(&a.as_str()))
}

/// Les arguments d'un lancement, complétés par ceux du moteur — `None` si rien ne
/// change. PURE, la zone en paramètre.
fn completer(exe: &Path, moteur: Option<Moteur>, arguments: Option<&str>, zone: RECT) -> Option<String> {
    let moteur = moteur?;
    if zone.right <= zone.left || zone.bottom <= zone.top || arguments.is_some_and(choisit_deja_son_affichage) {
        return None;
    }
    crate::diag!("[jeux] {} : moteur {moteur:?}, lance en fenetre", exe.display());
    Some(match arguments.map(str::trim).filter(|a| !a.is_empty()) {
        Some(a) => format!("{a} {}", options(moteur, zone)),
        None => options(moteur, zone),
    })
}

/// La zone de travail de l'écran principal — la zone libre entre nos barres.
fn zone_principale() -> Option<RECT> {
    let moniteur = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { GetMonitorInfoW(moniteur, &mut info) }.as_bool().then_some(info.rcWork)
}

/// ⭐ Appelée par `appicon::create_process_detached` avant le lancement : les
/// arguments à utiliser, et le dossier du jeu à guetter — `None` si l'option est
/// coupée ou l'exécutable pas un jeu reconnu.
pub fn preparer(exe: &Path, arguments: Option<&str>) -> Option<(String, PathBuf)> {
    if !est_active() {
        return None;
    }
    let zone = zone_principale()?;
    let complets = completer(exe, detecter(exe), arguments, zone)?;
    Some((complets, racine_du_jeu(exe)))
}

/// Le dossier où vivent les exécutables du jeu : un lanceur Unreal démarre un
/// AUTRE processus, plus bas dans `Jeu\Binaries\Win64`.
fn racine_du_jeu(exe: &Path) -> PathBuf {
    let dossier = exe.parent().unwrap_or(exe);
    let binaires = dossier.ancestors().find(|d| d.file_name().is_some_and(|n| n.eq_ignore_ascii_case("Binaries")));
    binaires.and_then(Path::parent).and_then(Path::parent).unwrap_or(dossier).to_path_buf()
}

/// ⭐ Après le lancement : guette la fenêtre du jeu, lui retire son cadre et la
/// pose dans la zone, puis l'y maintient `MAINTIEN` — sur un fil à part, le dock
/// ne s'arrête pas.
pub fn placer_quand_elle_parait(racine: PathBuf) {
    let _ = std::thread::Builder::new().name("jeux-fenetres".into()).spawn(move || {
        let debut = Instant::now();
        let mut trouvee: Option<Instant> = None;
        while debut.elapsed() < ATTENTE_FENETRE && trouvee.is_none_or(|t| t.elapsed() < MAINTIEN) {
            if let (Some(fenetre), Some(zone)) = (fenetre_du_jeu(&racine), zone_principale()) {
                if trouvee.is_none() {
                    crate::diag!("[jeux] fenetre trouvee sous {}", racine.display());
                    trouvee = Some(Instant::now());
                }
                poser(fenetre, zone);
            }
            std::thread::sleep(CADENCE);
        }
    });
}

/// Sans cadre, à la place exacte de la zone ; rien si elle y est déjà.
fn poser(fenetre: HWND, zone: RECT) {
    unsafe {
        let style = GetWindowLongPtrW(fenetre, GWL_STYLE);
        let cadre = (WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX).0 as isize;
        let mut rect = RECT::default();
        let _ = GetWindowRect(fenetre, &mut rect);
        if style & cadre == 0 && rect == zone {
            return;
        }
        SetWindowLongPtrW(fenetre, GWL_STYLE, style & !cadre);
        let (l, h) = (zone.right - zone.left, zone.bottom - zone.top);
        let drapeaux = SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_FRAMECHANGED;
        if let Err(erreur) = SetWindowPos(fenetre, None, zone.left, zone.top, l, h, drapeaux) {
            // UIPI : un jeu lancé en administrateur (voir `plein_ecran_zone`).
            crate::diag!("[jeux] placement refuse ({erreur})");
        }
    }
}

/// La plus grande fenêtre visible, de premier niveau et sans propriétaire, d'un
/// processus dont l'exécutable vit sous `racine`.
fn fenetre_du_jeu(racine: &Path) -> Option<HWND> {
    struct Recherche<'a> {
        racine: &'a Path,
        meilleure: Option<(HWND, i64)>,
    }
    unsafe extern "system" fn chaque(hwnd: HWND, l: LPARAM) -> BOOL {
        let recherche = unsafe { &mut *(l.0 as *mut Recherche) };
        let visible = unsafe { IsWindowVisible(hwnd) }.as_bool() && unsafe { GetWindow(hwnd, GW_OWNER) }.is_err();
        let mut rect = RECT::default();
        if !visible || unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
            return true.into();
        }
        let (l, h) = (rect.right - rect.left, rect.bottom - rect.top);
        if l < COTE_MIN || h < COTE_MIN {
            return true.into();
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        let dedans = crate::win32::app_identity::process_image_name(pid).is_some_and(|(_, chemin)| sous(Path::new(&chemin), recherche.racine));
        let aire = l as i64 * h as i64;
        if dedans && recherche.meilleure.is_none_or(|(_, a)| aire > a) {
            recherche.meilleure = Some((hwnd, aire));
        }
        true.into()
    }
    let mut recherche = Recherche { racine, meilleure: None };
    let _ = unsafe { EnumWindows(Some(chaque), LPARAM(&mut recherche as *mut Recherche as isize)) };
    recherche.meilleure.map(|(h, _)| h)
}

/// `chemin` est-il sous `racine` ? Sans tenir compte de la casse (Windows).
fn sous(chemin: &Path, racine: &Path) -> bool {
    let bas = |p: &Path| p.to_string_lossy().to_lowercase().replace('/', "\\");
    let (c, r) = (bas(chemin), bas(racine));
    let r = r.trim_end_matches('\\');
    c.strip_prefix(r).is_some_and(|reste| reste.starts_with('\\'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un dossier de jeu factice : les fichiers voisins qui trahissent le moteur.
    fn jeu(nom: &str, voisins: &[&str]) -> (PathBuf, PathBuf) {
        let dossier = std::env::temp_dir().join(format!("nd-jeux-{nom}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dossier);
        std::fs::create_dir_all(&dossier).unwrap();
        for v in voisins {
            let chemin = dossier.join(v);
            if v.ends_with('\\') {
                std::fs::create_dir_all(&chemin).unwrap();
            } else {
                std::fs::create_dir_all(chemin.parent().unwrap()).unwrap();
                std::fs::write(&chemin, b"").unwrap();
            }
        }
        (dossier.join(format!("{nom}.exe")), dossier)
    }

    const ZONE: RECT = RECT { left: 83, top: 40, right: 2480, bottom: 1387 };

    #[test]
    fn le_moteur_se_reconnait_a_ses_fichiers() {
        let (unity, d1) = jeu("Hollow", &["UnityPlayer.dll"]);
        let (unity_ancien, d2) = jeu("Vieux", &["Vieux_Data\\"]);
        let (lanceur, d3) = jeu("Lanceur", &["Engine\\Binaries\\"]);
        let (source, d4) = jeu("hl2", &["bin\\engine.dll"]);
        let (rien, d5) = jeu("Notepad", &["readme.txt"]);
        assert_eq!(detecter(&unity), Some(Moteur::Unity));
        assert_eq!(detecter(&unity_ancien), Some(Moteur::Unity));
        assert_eq!(detecter(&lanceur), Some(Moteur::Unreal));
        assert_eq!(detecter(Path::new(r"C:\Jeux\X\X\Binaries\Win64\X-Win64-Shipping.exe")), Some(Moteur::Unreal));
        assert_eq!(detecter(&source), Some(Moteur::Source));
        assert_eq!(detecter(&rien), None);
        for d in [d1, d2, d3, d4, d5] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// Les options portent la taille de la zone qu'on leur donne — jamais une
    /// taille supposée.
    #[test]
    fn les_options_portent_la_zone() {
        assert_eq!(options(Moteur::Unity, ZONE), "-screen-fullscreen 0 -popupwindow -screen-width 2397 -screen-height 1347");
        assert_eq!(options(Moteur::Unreal, ZONE), "-windowed -ResX=2397 -ResY=1347 -WinX=83 -WinY=40");
        assert_eq!(options(Moteur::Source, ZONE), "-windowed -noborder -w 2397 -h 1347");
        let autre = RECT { left: 0, top: 0, right: 1280, bottom: 720 };
        assert!(options(Moteur::Unity, autre).ends_with("-screen-width 1280 -screen-height 720"));
    }

    /// Les arguments existants sont gardés ; un mode d'affichage déjà choisi, une
    /// zone vide ou un moteur inconnu : rien ne change.
    #[test]
    fn un_choix_deja_fait_n_est_pas_touche() {
        let exe = Path::new(r"C:\Jeux\a.exe");
        assert_eq!(completer(exe, Some(Moteur::Source), Some("-novid"), ZONE).unwrap(), "-novid -windowed -noborder -w 2397 -h 1347");
        assert_eq!(completer(exe, Some(Moteur::Source), Some("  "), ZONE).unwrap(), "-windowed -noborder -w 2397 -h 1347");
        assert_eq!(completer(exe, Some(Moteur::Unity), Some("-screen-fullscreen 1"), ZONE), None);
        assert_eq!(completer(exe, Some(Moteur::Unreal), Some("-FULLSCREEN"), ZONE), None);
        assert_eq!(completer(exe, None, None, ZONE), None);
        assert_eq!(completer(exe, Some(Moteur::Unity), None, RECT::default()), None);
        assert!(!choisit_deja_son_affichage("-swap -windowedmode"), "un préfixe ne compte pas");
    }

    /// Un lanceur Unreal démarre un autre exe plus bas : on guette tout le jeu.
    #[test]
    fn la_racine_couvre_les_binaires_du_jeu() {
        let exe = Path::new(r"C:\Jeux\Mon Jeu\MonJeu\Binaries\Win64\MonJeu-Win64-Shipping.exe");
        assert_eq!(racine_du_jeu(exe), PathBuf::from(r"C:\Jeux\Mon Jeu"));
        assert_eq!(racine_du_jeu(Path::new(r"C:\Jeux\Hollow\hollow.exe")), PathBuf::from(r"C:\Jeux\Hollow"));
        assert!(sous(Path::new(r"C:\JEUX\Mon Jeu\x.exe"), Path::new(r"C:\Jeux\Mon Jeu")));
        assert!(!sous(Path::new(r"C:\Jeux\Mon Jeu 2\x.exe"), Path::new(r"C:\Jeux\Mon Jeu")), "un voisin au nom proche");
    }

    /// ⭐ **SUR UNE VRAIE FENÊTRE** — à lancer à la main (`--ignored`), l'app
    /// ouverte. Une fenêtre À CADRE de ce processus est retrouvée par son dossier,
    /// perd son cadre et prend exactement la zone lue sur cette machine.
    #[test]
    #[ignore]
    fn essai_reel_la_fenetre_du_jeu_est_posee_dans_la_zone() {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, ShowWindow, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW};
        let fenetre = unsafe {
            CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!("essai jeu"), WS_OVERLAPPEDWINDOW, 100, 100, 800, 600, None, None, None, None)
        }
        .expect("fenêtre d'essai");
        unsafe {
            let _ = ShowWindow(fenetre, SW_SHOWNOACTIVATE);
        }
        let racine = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let trouvee = fenetre_du_jeu(&racine);
        let zone = zone_principale().unwrap();
        poser(trouvee.unwrap(), zone);
        let mut rect = RECT::default();
        let style = unsafe { GetWindowLongPtrW(fenetre, GWL_STYLE) };
        unsafe { GetWindowRect(fenetre, &mut rect) }.unwrap();
        unsafe { DestroyWindow(fenetre) }.unwrap();
        println!("zone {zone:?}  fenetre {rect:?}");
        assert_eq!(trouvee, Some(fenetre));
        assert_eq!(rect, zone);
        assert_eq!(style & WS_CAPTION.0 as isize, 0, "le cadre est retiré");
    }

    #[test]
    fn coupee_elle_ne_touche_a_rien() {
        activer(false);
        assert!(preparer(Path::new(r"C:\Jeux\a.exe"), None).is_none());
    }
}
