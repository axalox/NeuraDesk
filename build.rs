//! Embarque l'icône de `neuradesk.exe` (voir `NeuraDeskIcon/NeuraDesk.rc`)
//! — celle que Windows affiche dans l'Explorateur/la barre des tâches/
//! Alt-Tab pour le FICHIER lui-même. Sans ressource `.rc` compilée ici,
//! `neuradesk.exe` porte l'icône générique Windows par défaut ; ceci est
//! DISTINCT du logo dessiné À L'INTÉRIEUR de l'application (voir
//! `gfx::texture::LOGO_PNG`), qui n'a besoin d'aucune étape de build — un
//! simple `include_bytes!` d'un PNG.
//!
//! `embed_resource::compile` est un no-op sur toute cible non-Windows (voir
//! sa doc) — pas besoin de `#[cfg(target_os = "windows")]` ici, ce projet
//! entier ne compile de toute façon que pour Windows (dépendances D3D11/
//! DXGI/Win32 partout).
fn main() {
    // `assets/popups` ci-dessous est le SEUL `rerun-if-changed` déclaré dans
    // tout ce build script avant cette ligne — dès qu'un premier est présent,
    // Cargo abandonne son comportement par défaut ("réexécuter si N'IMPORTE
    // QUOI dans le paquet change") et ne surveille plus QUE les chemins
    // listés explicitement. Sans ces deux lignes, changer l'icône ou le
    // `.rc` ne réexécute pas `embed_resource::compile` : l'exe garde
    // l'icône d'un précédent build (déjà vécu — voir historique de ce
    // fichier).
    println!("cargo:rerun-if-changed=NeuraDeskIcon/NeuraDesk.rc");
    println!("cargo:rerun-if-changed=NeuraDeskIcon/NeuraDeskIcon.ico");

    embed_resource::compile("NeuraDeskIcon/NeuraDesk.rc", embed_resource::NONE)
        .manifest_optional()
        .unwrap();

    copy_popup_assets();
    emettre_la_revision();
    // Les sons de l'alarme de note — voir `copy_sound_assets`, qui porte le
    // pourquoi de l'absence de filtre.
    copy_sound_assets();
    // Les polices embarquées des terminaux — voir `copy_polices_de_terminal_assets`,
    // qui explique pourquoi ce dossier a son propre copieur.
    copy_polices_de_terminal_assets();
    // Le module injecté — voir `copy_window_hook_dll` (facultatif : absent si
    // la crate `window-hook` n'a jamais été construite).
    copy_window_hook_dll();
}

/// Copie `window_hook.dll` à côté de l'exe — NeuraDesk la charge dans les
/// applications quand le réglage « Animer aussi les fenêtres des autres
/// applications » est coché (voir `win32::injection`).
///
/// ⚠️ **FACULTATIF ET SILENCIEUX QUAND ELLE MANQUE**, comme `son/` : la crate
/// `window-hook` se construit à part (`CARGO_TARGET_DIR=... cargo build
/// --release`, voir son README) et n'appartient pas à ce build. Sans elle, la
/// case cochée ne fait rien et le journal le dit — l'application démarre.
fn copy_window_hook_dll() {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR non fourni par Cargo");
    let Some(target_dir) = std::path::Path::new(&out_dir).ancestors().nth(3) else {
        return;
    };
    // Deux emplacements : le `target/` de la crate (build ordinaire) et le
    // dossier externe employé ici pour ne pas construire dans OneDrive.
    let mut sources = vec![std::path::Path::new("window-hook").join("target").join("release").join("window_hook.dll")];
    if let Some(externe) = std::env::var_os("NEURADESK_WINDOW_HOOK_DLL") {
        sources.push(std::path::PathBuf::from(externe));
    }
    for source in &sources {
        println!("cargo:rerun-if-changed={}", source.display());
    }
    let Some(source) = sources.into_iter().find(|chemin| chemin.is_file()) else {
        return;
    };
    let destination = target_dir.join("window_hook.dll");
    if std::fs::copy(&source, &destination).is_ok() {
        return;
    }
    // ⛔ **UNE DLL CHARGÉE EST VERROUILLÉE** — MESURÉ (2026-11-05) : des
    // applications où elle a été injectée la tiennent, la copie échoue
    // (« Device or resource busy ») et la version suivante ne partirait jamais.
    // ⭐ Le RENOMMAGE, lui, reste permis sur un fichier chargé : on écarte
    // l'ancienne et on pose la neuve.
    let ancienne = target_dir.join(format!(
        "window_hook.ancienne-{}.dll",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    ));
    if std::fs::rename(&destination, &ancienne).is_ok() {
        if let Err(err) = std::fs::copy(&source, &destination) {
            eprintln!("cargo:warning=échec copie {}: {err}", source.display());
        }
    } else {
        eprintln!("cargo:warning={} est verrouillée et introuvable au renommage", destination.display());
    }
}

/// Copie `assets/popups/*.{html,css}` à côté de l'exe compilé — les pages CEF
/// de `win32::cef_test_window` (voir `docs/plans/cef-web-ui-migration.md`)
/// sont chargées via `file:///.../assets/popups/...` RELATIF À
/// `std::env::current_exe()`, PAS `include_str!` (ce sont de vraies pages
/// HTML/CSS, pensées pour être éditées sans recompiler le binaire Rust —
/// tout l'intérêt de cette migration). `OUT_DIR` (voir Cargo, "Build
/// Scripts") ressemble à `target/<profil>/build/<pkg>-<hash>/out` : ses 3
/// ancêtres remontent exactement à `target/<profil>/`, le dossier où
/// atterrit l'exe final — pas de variable d'environnement Cargo qui donne
/// directement ce chemin à un build script.
///
/// BUG RÉEL trouvé en câblant `assets/popups/menu.css` sur le système de
/// palette (`ui::color::Palette`, voir `win32::cef_popup::push_menu_palette`) :
/// ce filtre ne retenait QUE `.html`, jamais copié depuis l'ajout de
/// `menu.css` (commit "feat: moteur de matériaux de flou + refonte des
/// popups CEF menu") — les 9 popups "menu" qui l'importent tournaient donc
/// tous SANS AUCUNE feuille de style commune (`<link>` pointant vers un
/// fichier absent du dossier de l'exe), reposant en pratique sur les styles
/// par défaut du navigateur pour `font-size`/`color`/`overflow`. `.css`
/// ajouté au filtre ci-dessous plutôt que de tout copier sans condition :
/// `assets/popups/` ne doit contenir QUE du HTML/CSS servi tel quel (voir la
/// doc de fonction), pas de fichiers `docs/`-style qui s'y glisseraient par
/// erreur.
/// **LA RÉVISION DU BUILD** pour la fenêtre « À propos » (`win32::cef_apropos`) :
/// `NEURADESK_REVISION` (commit court), `NEURADESK_DATE_REVISION` (date du commit,
/// `AAAA-MM-JJ`) et `NEURADESK_PUBLICATION` (le dernier tag, `portable-…`).
/// « inconnue » si `git` manque — le build ne doit jamais en dépendre.
///
/// ⚠️ **LES DEUX `rerun-if-changed` SONT INDISPENSABLES** : ce script ne surveille
/// QUE les chemins listés (voir l'en-tête de `main`). Sans eux, la révision
/// affichée resterait celle du premier build. `.git/logs/HEAD` change à chaque
/// commit et à chaque changement de branche.
fn emettre_la_revision() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/logs/HEAD");
    let git = |args: &[&str]| -> String {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|sortie| sortie.status.success())
            .map(|sortie| String::from_utf8_lossy(&sortie.stdout).trim().to_string())
            .filter(|texte| !texte.is_empty())
            .unwrap_or_else(|| "inconnue".to_string())
    };
    println!("cargo:rustc-env=NEURADESK_REVISION={}", git(&["rev-parse", "--short", "HEAD"]));
    println!("cargo:rustc-env=NEURADESK_DATE_REVISION={}", git(&["log", "-1", "--format=%cs"]));
    println!("cargo:rustc-env=NEURADESK_PUBLICATION={}", git(&["describe", "--tags", "--abbrev=0"]));
}

fn copy_popup_assets() {
    println!("cargo:rerun-if-changed=assets/popups");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR non fourni par Cargo");
    let Some(target_dir) = std::path::Path::new(&out_dir).ancestors().nth(3) else {
        eprintln!("cargo:warning=impossible de déduire target/<profil>/ depuis OUT_DIR={out_dir}");
        return;
    };

    let src = std::path::Path::new("assets/popups");
    let dst = target_dir.join("assets/popups");
    if let Err(err) = std::fs::create_dir_all(&dst) {
        eprintln!("cargo:warning=échec création {}: {err}", dst.display());
        return;
    }
    let Ok(entries) = std::fs::read_dir(src) else {
        eprintln!("cargo:warning=dossier {} introuvable", src.display());
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // `.js` AJOUTÉ le 2026-09-19, POUR LA MÊME RAISON QUE `.css` JUSTE
        // AU-DESSUS — et c'est la deuxième fois que ce filtre oublie un type de
        // fichier : `assets/popups/menu.js` mesure la hauteur d'un menu et
        // l'annonce à Rust, et sans cette ligne les neuf pages le chargeaient
        // dans le VIDE (`<script src>` vers un fichier absent du dossier de
        // l'exe). **Le symptôme ne ressemble PAS à un fichier manquant** : aucune
        // erreur, aucun journal — simplement une fenêtre qui garde sa hauteur
        // d'estimation, comme si le mécanisme n'existait pas. Mesuré par capture :
        // le menu Contexte restait à 2397 px, sa hauteur de création.
        // `.png` AJOUTÉ le 2026-11-08 : le logo en tête de l'aide
        // (`logo_neuradesk.png`, chargé par `<img src>` — même symptôme muet
        // qu'au-dessus s'il manquait : une image cassée, aucun journal).
        if path.extension().is_some_and(|ext| ext == "html" || ext == "css" || ext == "js" || ext == "png") {
            if let Some(name) = path.file_name() {
                if let Err(err) = std::fs::copy(&path, dst.join(name)) {
                    eprintln!("cargo:warning=échec copie {}: {err}", path.display());
                }
            }
        }
    }
}

/// Copie `son/` à côté de l'exe — les sons que l'alarme de note peut jouer (voir
/// `win32::son`, qui les énumère depuis `current_exe().parent()`).
///
/// ⚠️ **AUCUN FILTRE D'EXTENSION ICI, ET C'EST DÉLIBÉRÉ.** Le filtre de
/// `copy_popup_assets` ci-dessus a **déjà oublié un type de fichier deux fois**
/// (`.css` puis `.js`, voir les deux commentaires qui y sont écrits) — et les deux
/// fois le symptôme ne ressemblait pas à un fichier manquant. Ici les fichiers sont
/// ce que l'utilisateur a déposé : `.mp3`, `.wav`, et demain autre chose. **Un
/// filtre qui n'est pas tenu à jour donnerait une alarme silencieuse**, exactement
/// la classe de panne que ces deux commentaires racontent.
///
/// ⚠️ Le dossier N'EST PAS versionné (`son/` n'est pas suivi par git) : un dépôt
/// fraîchement cloné n'en a pas, et c'est normal — la copie est alors simplement
/// sautée, sans avertissement, parce qu'il n'y a rien à signaler.
fn copy_sound_assets() {
    println!("cargo:rerun-if-changed=son");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR non fourni par Cargo");
    let Some(target_dir) = std::path::Path::new(&out_dir).ancestors().nth(3) else {
        return;
    };

    let src = std::path::Path::new("son");
    if !src.is_dir() {
        return;
    }
    let dst = target_dir.join("son");
    if let Err(err) = std::fs::create_dir_all(&dst) {
        eprintln!("cargo:warning=échec création {}: {err}", dst.display());
        return;
    }
    let Ok(entries) = std::fs::read_dir(src) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name() {
                if let Err(err) = std::fs::copy(&path, dst.join(name)) {
                    eprintln!("cargo:warning=échec copie {}: {err}", path.display());
                }
            }
        }
    }
}

/// Copie `assets/popups/fonts/` à côté de l'exe — les **polices embarquées des
/// terminaux** (Nordwand Mono, SIL OFL 1.1), que les deux pages de terminal
/// déclarent par `@font-face` dans `fonts/polices.css` et chargent par
/// `url("fonts/…")`.
///
/// ⛔ **CE DOSSIER A SON PROPRE COPIEUR PARCE QUE `copy_popup_assets` NE COPIE
/// QUE DES FICHIERS À PLAT** : son `read_dir` ne descend pas dans les
/// sous-dossiers et son filtre d'extension ignore `.woff2`. Le défaut qui en
/// découle est exactement celui des deux oublis déjà écrits dans
/// `copy_popup_assets` (`.css` puis `.js`) : **aucune erreur, aucun journal**,
/// seulement le terminal qui retombe sur la police suivante de sa pile — mesuré
/// le 2026-10-28, une cellule de 11,55 px au lieu de 12,60, donc des glyphes
/// d'une autre graisse que les lettres, que la capture seule révèle.
///
/// Aucun filtre d'extension, comme `copy_sound_assets` juste au-dessus : le
/// dossier porte ce qu'on y dépose (`.woff2`, `.ttf`, et la licence qui va avec).
/// Absent (dépôt fraîchement cloné sans les polices), la copie est sautée — la
/// pile des terminaux a des replis installés (DejaVu, Consolas).
fn copy_polices_de_terminal_assets() {
    println!("cargo:rerun-if-changed=assets/popups/fonts");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR non fourni par Cargo");
    let Some(target_dir) = std::path::Path::new(&out_dir).ancestors().nth(3) else {
        return;
    };

    let src = std::path::Path::new("assets/popups/fonts");
    if !src.is_dir() {
        return;
    }
    let dst = target_dir.join("assets/popups/fonts");
    if let Err(err) = std::fs::create_dir_all(&dst) {
        eprintln!("cargo:warning=échec création {}: {err}", dst.display());
        return;
    }
    let Ok(entries) = std::fs::read_dir(src) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name() {
                if let Err(err) = std::fs::copy(&path, dst.join(name)) {
                    eprintln!("cargo:warning=échec copie {}: {err}", path.display());
                }
            }
        }
    }
}
