//! Épingles de la VRAIE barre des tâches Windows — les applications que
//! l'utilisateur a épinglées dans Windows, pour les afficher dans la barre de
//! gauche de NeuraDesk (DEMANDE EXPLICITE : « les icônes des applications
//! épinglées à la barre des tâches apparaissent dans la barre de gauche »).
//! À ne PAS confondre avec les applications épinglées de NeuraDesk
//! (`config::PinnedApp`, `config.toml`), qui sont une liste indépendante.
//!
//! **Deux sources, parce que Windows en utilise deux :**
//!
//! 1. **Applications de bureau** : un raccourci `.lnk` par épingle dans
//!    `%APPDATA%\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar`.
//!    Ce dossier donne l'ENSEMBLE (nom lisible, et — via
//!    `win32::apps::resolve_shortcut` — l'exécutable cible), mais **pas
//!    l'ordre**.
//! 2. **Applications UWP/Store** : aucun raccourci, leur AppUserModelID
//!    n'existe que dans le blob décrit ci-dessous.
//!
//! **L'ORDRE** vient de `HKCU\...\Explorer\Taskband\FavoritesResolve`, un blob
//! binaire non documenté où Windows sérialise ses épingles dans l'ordre
//! d'affichage. On ne tente PAS d'en décoder la structure — aucune
//! documentation, aucune garantie de stabilité d'une version de Windows à
//! l'autre. On y recherche simplement le NOM de fichier de chaque raccourci
//! encodé en UTF-16LE, et on trie par position : ça ne dépend que d'une chose
//! (que ce nom apparaisse dans le blob), pas d'un format. Mesuré sur la
//! machine de l'utilisateur : 16 raccourcis sur 16 retrouvés, dans un ordre
//! cohérent avec sa barre des tâches.
//!
//! Les AppUserModelID des applications UWP sont dans le **même blob, au même
//! titre que les noms de raccourcis** : on les y cherche donc sur TOUTE sa
//! longueur, à un décalage **pair ou impair** — d'où la recherche aux deux
//! parités. Même règle que pour les noms de fichiers : première occurrence
//! retenue.
//!
//! **UNE VERSION PRÉCÉDENTE NE CHERCHAIT QUE DANS LA « SECTION DE TÊTE »**, sur
//! la foi d'une mesure : « les quatre identifiants extraits sont exactement les
//! quatre applications UWP épinglées », donc cette zone ne contenait pas de
//! pollution par les applications récemment utilisées. La mesure était vraie, la
//! généralisation non — sur la MÊME machine, après que l'utilisateur a épinglé
//! d'autres applications du Store, `Microsoft.YourPhone` (décalage 12883) et
//! `5319275A.WhatsAppDesktop…!App` (15979) se sont retrouvés APRÈS
//! `File Explorer.lnk` (6574), donc ignorés, donc absents de la barre — et tout
//! ce qui les suivait remontait d'un cran. DÉFAUT RÉEL SIGNALÉ : « l'épingle
//! WhatsApp est à la mauvaise place ». Ce qui borne la recherche, ce n'est pas
//! une position, c'est la VALIDATION DE FORME de `looks_like_aumid` : c'est elle
//! qui écarte le bruit binaire, et elle suffit. **C'est le point fragile du
//! module** : si un jour la liste extraite contient une application non
//! épinglée, c'est ici qu'il faudra regarder.
//!
//! « Est-ce ouvert ? » et « basculer dessus » ne se répondent PAS avec un nom
//! d'exécutable : une application du Store n'en a pas, et un raccourci de
//! bureau peut pointer un lanceur plutôt que l'application. Chaque épingle
//! reçoit donc une `WindowTarget` (voir `win32::app_identity`, qui documente
//! les identités que Windows expose et pourquoi il en faut deux) :
//!
//! - une épingle UWP → `WindowTarget::Aumid`, l'identifiant qu'on vient
//!   d'extraire ;
//! - une épingle de bureau → `WindowTarget::Process`, le nom de fichier de
//!   l'exécutable visé par le raccourci — ou, quand ce raccourci passe par un
//!   lanceur, le nom de l'exécutable que ce lanceur démarre
//!   (`process_start_from_arguments`, sans quoi Discord serait cherché sous
//!   `Update.exe` et jamais trouvé).

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Registry::HKEY_CURRENT_USER;
use windows::Win32::UI::Shell::{
    SHGetFileInfoW, SHParseDisplayName, SHFILEINFOW, SHGFI_DISPLAYNAME, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_PIDL,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON, IMAGE_ICON, LR_LOADFROMFILE, LoadImageW};

use super::{app_identity::WindowTarget, appicon, apps, registry_util};

/// Sous-dossier, relatif à `%APPDATA%`, des raccourcis épinglés à la barre des
/// tâches.
const PINS_SUBDIR: &str = r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar";
const TASKBAND_SUBKEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Taskband";
const TASKBAND_VALUE: &str = "FavoritesResolve";

/// Comment lancer l'application épinglée.
#[derive(Clone)]
pub enum PinLaunch {
    /// Application de bureau : on lance le `.lnk` LUI-MÊME (`appicon::launch`)
    /// plutôt que son exécutable cible — un raccourci peut porter des
    /// arguments, un « démarrer en tant qu'administrateur » ou un
    /// AppUserModelID propre, que lancer la cible directement perdrait.
    Shortcut(PathBuf),
    /// Application UWP : `shell:AppsFolder\<AppUserModelID>`, la seule
    /// syntaxe qui la lance (voir la doc de module de `win32::uwp_apps`).
    Aumid(String),
}

/// Une application épinglée à la barre des tâches Windows.
pub struct TaskbarPin {
    /// Nom affiché — nom du raccourci sans extension, ou nom déclaré de
    /// l'application pour une UWP.
    pub name: String,
    pub launch: PinLaunch,
    /// Comment reconnaître cette application parmi les fenêtres ouvertes —
    /// clé de « est-ce ouvert ? » et de « basculer dessus ». `None` quand
    /// aucune identité n'est exploitable (raccourci cassé, et ni cible ni
    /// icône ne nommant un exécutable) : l'épingle s'affiche et se lance, mais
    /// sans pastille et sans bascule.
    ///
    /// **« File Explorer » n'est PAS ce cas** — un commentaire précédent
    /// l'affirmait, et c'était périmé : il l'a été jusqu'à ce que
    /// `process_name_from_icon_path` vienne nommer l'application à partir de
    /// l'icône qu'elle déclare (voir ce repli dans `read_pins`, et le test qui
    /// porte son nom). Cette épingle EST donc reconnue, c'est ce qui fait que
    /// le clic GAUCHE rejoint ou fait défiler ses fenêtres de dossier au lieu
    /// d'en ouvrir une nouvelle, et que le clic DROIT peut, lui, en ouvrir une
    /// (voir `app::run`, `Event::RightClick`).
    pub target: Option<WindowTarget>,
    /// Icône déjà décodée en RGBA8 — résolue ICI plutôt que par l'appelant,
    /// parce que les deux types d'application demandent deux API différentes
    /// (voir `shortcut_icon_rgba` et `uwp_apps::logo_rgba`) et que `app::run`
    /// n'a plus qu'à téléverser le résultat (`Texture::from_rgba`), sans
    /// savoir d'où il vient. `None` si l'icône n'a pas pu être obtenue — le
    /// dock retombe alors sur son carré de couleur de repli.
    pub icon_rgba: Option<(u32, u32, Vec<u8>)>,
}

impl TaskbarPin {
    /// Lance l'application épinglée — le geste qui n'est PAS « basculer sur
    /// sa fenêtre existante » (voir `appicon::focus_or`, qui commence par
    /// chercher la fenêtre et n'appelle ceci qu'en dernier recours).
    ///
    /// Vit ici, avec les données, plutôt que chez l'appelant : le geste dépend
    /// entièrement du TYPE d'épingle, que seule cette structure connaît.
    pub fn launch(&self) {
        match &self.launch {
            PinLaunch::Shortcut(path) => appicon::launch(path),
            PinLaunch::Aumid(aumid) => appicon::open(&format!(r"shell:AppsFolder\{aumid}")),
        }
    }
}

/// Lit les épingles de la barre des tâches Windows, **dans l'ordre où elles y
/// apparaissent**. Best-effort de bout en bout : une source indisponible
/// donne une liste vide ou partielle, jamais une erreur — même convention que
/// `win32::apps::enumerate_apps`.
///
/// Coût : l'énumération des paquets UWP (`uwp_apps::enumerate`) n'a lieu que
/// si le blob contient au moins un AppUserModelID, et elle ne sert qu'à
/// traduire ces quelques identifiants en nom + logo lisibles.
pub fn read_pins() -> Vec<TaskbarPin> {
    let blob = registry_util::read_binary_value(HKEY_CURRENT_USER, TASKBAND_SUBKEY, TASKBAND_VALUE)
        .unwrap_or_default();

    // Chaque épingle retenue avec sa position d'origine dans le blob : c'est
    // elle qui porte l'ordre d'affichage.
    let mut positioned: Vec<(usize, TaskbarPin)> = Vec::new();
    // Les raccourcis absents du blob (compte neuf, blob réinitialisé…) : ils
    // n'ont pas de position, on les range à la fin par ordre alphabétique
    // plutôt que de les perdre ou de les intercaler au hasard.
    let mut unpositioned: Vec<TaskbarPin> = Vec::new();

    if let Some(dir) = pins_dir() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
                    continue;
                }
                // Possédée (et non empruntée à `path`) : `path` est déplacé
                // plus bas dans `PinLaunch::Shortcut`.
                let Some(file_name) =
                    path.file_name().and_then(|n| n.to_str()).map(str::to_string)
                else {
                    continue;
                };
                let name = path
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&file_name)
                    .to_string();

                // `resolve_shortcut` donne la cible, la description et les
                // arguments ; seuls la cible et les arguments servent ici.
                let resolved = apps::resolve_shortcut(&path);
                // L'identité à chercher est celle de l'exécutable qui PORTE la
                // fenêtre — pas forcément la cible du raccourci. Trois sources,
                // essayées dans cet ordre ; la première qui répond gagne, donc
                // chacune ne s'applique QUE là où la précédente est muette :
                //
                // 1. le lanceur, quand le raccourci en passe un
                //    (`process_start_from_arguments` — Discord) ;
                // 2. la cible du raccourci (le cas ordinaire) ;
                // 3. l'exécutable désigné par l'ICÔNE du raccourci, quand la
                //    cible n'est pas résoluble : l'épingle « File Explorer »
                //    de Windows vise un élément du shell, pas un fichier, et
                //    n'annonce son application que par son icône
                //    (`%windir%\explorer.exe`) — voir `process_name_from_icon_path`.
                let target = resolved
                    .as_ref()
                    .and_then(|shortcut| {
                        process_start_from_arguments(&shortcut.arguments)
                            .or_else(|| {
                                shortcut
                                    .target
                                    .as_deref()
                                    .and_then(Path::file_name)
                                    .map(|n| n.to_string_lossy().into_owned())
                            })
                            .or_else(|| shortcut.icon.as_deref().and_then(process_name_from_icon_path))
                    })
                    .map(WindowTarget::process);

                // Icône : celle que le RACCOURCI déclare s'il en déclare une,
                // parce que c'est CELLE QUE WINDOWS AFFICHE pour cette épingle.
                // DÉFAUT RÉEL SIGNALÉ PAR L'UTILISATEUR : « l'icône de Discord
                // n'est pas la bonne ». Le raccourci Discord désigne
                // `…\Discord\app.ico` (le logo) alors que sa CIBLE est
                // `Update.exe`, le lanceur Squirrel — prendre l'icône sur la
                // cible affichait donc l'icône du lanceur.
                //
                // Replis, dans l'ordre : la cible du raccourci (cas ordinaire),
                // puis le `.lnk` lui-même (raccourci cassé). Jamais de flèche de
                // raccourci au passage — DEMANDE EXPLICITE : « il faudrait
                // supprimer les flèches de raccourci sur les icones ». La flèche
                // n'apparaît QUE si l'on interroge le `.lnk` : aussi bien
                // `app.ico` que `explorer.exe` ou `brave.exe` en sont exempts,
                // et le `.lnk` n'est atteint qu'en dernier recours, où mieux vaut
                // une icône avec flèche que pas d'icône du tout.
                let icon_rgba = resolved
                    .as_ref()
                    .and_then(|shortcut| shortcut.icon.as_deref())
                    .and_then(shell_icon_rgba)
                    .or_else(|| {
                        resolved
                            .as_ref()
                            .and_then(|shortcut| shortcut.target.as_deref())
                            .and_then(shell_icon_rgba)
                    })
                    .or_else(|| shell_icon_rgba(&path));
                let pin = TaskbarPin { name, launch: PinLaunch::Shortcut(path), target, icon_rgba };
                match find_utf16(&blob, &file_name) {
                    Some(offset) => positioned.push((offset, pin)),
                    None => unpositioned.push(pin),
                }
            }
        }
    }

    // Les AppUserModelID sont cherchés dans TOUT le blob, **au même titre que
    // les noms de raccourcis** : ils y sont entrelacés dans l'ordre d'affichage,
    // et une application du Store épinglée tardivement apparaît APRÈS des noms
    // de raccourcis (voir la doc de module, qui porte la mesure). Ce n'est pas
    // une position qui borne la recherche, c'est la validation de forme
    // d'`extract_aumids` — c'est elle qui écarte le bruit binaire du blob.
    let aumids = extract_aumids(&blob);

    for (offset, aumid) in aumids {
        // Nom ET icône viennent du SHELL, pas de WinRT — voir
        // `shell_item_info`, qui documente le blocage que ça évite.
        let shell = shell_item_info(&format!(r"shell:AppsFolder\{aumid}"));
        let (name, icon_rgba) = match shell {
            Some((display_name, icon)) if !display_name.trim().is_empty() => (display_name, icon),
            // Shell muet (application désinstallée, identifiant orphelin) :
            // repli sur un nom dérivé de l'identifiant, jamais une entrée
            // sans nom.
            _ => (name_from_aumid(&aumid), None),
        };
        positioned.push((
            offset,
            TaskbarPin {
                name,
                // L'identifiant qu'on vient d'extraire EST l'identité de
                // l'application : c'est lui que ses fenêtres exposent, via
                // l'identité de paquet de leur processus (voir
                // `win32::app_identity` — c'est ce qui répare la bascule de
                // Terminal).
                launch: PinLaunch::Aumid(aumid.clone()),
                target: Some(WindowTarget::aumid(aumid)),
                icon_rgba,
            },
        ));
    }

    positioned.sort_by_key(|(offset, _)| *offset);
    let mut pins: Vec<TaskbarPin> = positioned.into_iter().map(|(_, pin)| pin).collect();
    unpositioned.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    pins.extend(unpositioned);

    // DIAGNOSTIC TEMPORAIRE — identifier une épingle dont l'icône ne se
    // résout pas (elle s'affiche en carré de repli dans la barre, sans qu'on
    // sache laquelle). Opt-in : `NEURADESK_DEBUG_PINS=1`, comme les autres
    // harnais `NEURADESK_DEBUG_*`.
    if std::env::var_os("NEURADESK_DEBUG_PINS").is_some() {
        for (i, pin) in pins.iter().enumerate() {
            let kind = match &pin.launch {
                PinLaunch::Shortcut(p) => format!("lnk={}", p.file_name().unwrap_or_default().to_string_lossy()),
                PinLaunch::Aumid(a) => format!("aumid={a}"),
            };
            eprintln!(
                "[neuradesk][pins] #{i:2} icone={} identite={} nom='{}' {kind}",
                if pin.icon_rgba.is_some() { "OUI" } else { "NON" },
                match &pin.target {
                    Some(WindowTarget::Process(name)) => format!("proc:{name}"),
                    Some(WindowTarget::Aumid(aumid)) => format!("aumid:{aumid}"),
                    Some(WindowTarget::Profile(id)) => format!("profil:{id}"),
                    None => "-".to_string(),
                },
                pin.name
            );
        }
    }
    pins
}

/// Icône d'un fichier (exécutable, ou raccourci en dernier recours), décodée
/// en RGBA8 — `None` si le fichier n'a pas d'icône exploitable.
///
/// **`SHGetFileInfoW`, PAS `ExtractIconExW`** — BUG RÉEL CONSTATÉ : les
/// épingles de bureau s'affichaient toutes en carré blanc dans la barre de
/// gauche. `ExtractIconExW` (utilisé par `gfx::icon::extract_icon_texture`,
/// qui convient parfaitement à un `.exe`) ne SUIT PAS les raccourcis : sur un
/// `.lnk` il échoue, et l'appelant retombait donc sur le carré de repli du
/// dock. `SHGetFileInfoW` est l'API que l'Explorateur lui-même utilise pour
/// ses listes de fichiers.
/// **`pub(crate)`** : réutilisée telle quelle pour l'icône des applications
/// OUVERTES non épinglées (voir `app::unlisted_open_apps`) — c'est la même
/// question (« l'icône de ce fichier ») et l'API éprouvée ici (`SHGetFileInfoW`,
/// voir sa doc, et non `ExtractIconExW` qui rate des cas).
pub(crate) fn shell_icon_rgba(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let mut info = SHFILEINFOW::default();
        let result = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if result == 0 || info.hIcon.is_invalid() {
            return None;
        }
        // `icon_to_rgba` (partagée avec le Dash) — le même décodage que pour
        // n'importe quel autre `HICON` du projet.
        let rgba = crate::gfx::icon::icon_to_rgba(info.hIcon);
        let _ = DestroyIcon(info.hIcon);
        rgba
    }
}

/// Côté demandé à un fichier `.ico` — MESURÉ : un `Google Profile.ico` de
/// Chrome porte 13 tailles, de 8 à 256 px ; `LoadImageW` prend la plus proche.
const ICO_FILE_SIZE: i32 = 128;

/// Icône d'un fichier `.ico`, en RGBA8 — celle d'un profil de navigateur (voir
/// `app_identity::BrowserProfile`). `SHGetFileInfoW` rendrait l'icône du TYPE
/// de fichier à 32 px ; ici on veut son contenu, en grand.
pub(crate) fn ico_file_rgba(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let handle =
            LoadImageW(None, PCWSTR(wide.as_ptr()), IMAGE_ICON, ICO_FILE_SIZE, ICO_FILE_SIZE, LR_LOADFROMFILE).ok()?;
        let icon = HICON(handle.0);
        let rgba = crate::gfx::icon::icon_to_rgba(icon);
        let _ = DestroyIcon(icon);
        rgba
    }
}

/// Nom affiché ET icône d'un élément du shell désigné par un chemin
/// « shell: » (ici `shell:AppsFolder\<AppUserModelID>`), en RGBA8.
///
/// **POURQUOI PAS `win32::uwp_apps`** — BUG RÉEL CONSTATÉ, l'app ENTIÈRE
/// figée au démarrage : `uwp_apps::enumerate` lit le logo via WinRT
/// (`IAsyncOperation::join()`, un appel BLOQUANT) et le reste du projet ne
/// l'appelle jamais que depuis un thread **MTA** (`win32::media`,
/// `win32::cef_dash`, qui font tous deux `CoInitializeEx(COINIT_MULTITHREADED)`
/// avant). Appelé depuis le thread PRINCIPAL — qui est STA (voir
/// `main::main`) — ce `join()` ne rend jamais la main : en appartement à
/// thread unique, l'achèvement d'une opération asynchrone WinRT a besoin que
/// ce même thread pompe des messages, ce que `join()` l'empêche justement de
/// faire. Toutes les fenêtres de l'app répondaient `IsHungAppWindow == true`.
///
/// Le shell, lui, sait répondre de façon SYNCHRONE : `SHParseDisplayName`
/// résout le chemin en PIDL, `SHGetFileInfoW` en tire le nom affiché et
/// l'icône. Aucune attente, aucun thread supplémentaire, aucune dépendance à
/// WinRT — et c'est la même API qui sert les raccourcis `.lnk` ci-dessus.
pub(crate) fn shell_item_info(shell_path: &str) -> Option<(String, Option<(u32, u32, Vec<u8>)>)> {
    let wide: Vec<u16> = shell_path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut pidl: *mut windows::Win32::UI::Shell::Common::ITEMIDLIST = std::ptr::null_mut();
        // `pbc` (contexte de liaison) : `None` — résolution directe, sans
        // contexte particulier, le cas courant pour un chemin « shell: ».
        let parsed =
            SHParseDisplayName(PCWSTR(wide.as_ptr()), None::<&windows::Win32::System::Com::IBindCtx>, &mut pidl, 0, None);
        if parsed.is_err() || pidl.is_null() {
            return None;
        }

        let mut info = SHFILEINFOW::default();
        let result = SHGetFileInfoW(
            PCWSTR(pidl as *const u16),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_PIDL | SHGFI_ICON | SHGFI_LARGEICON | SHGFI_DISPLAYNAME,
        );
        // Le PIDL appartient à l'appelant dès que `SHParseDisplayName` a
        // réussi — libéré AVANT tout retour, y compris les chemins d'échec
        // ci-dessous.
        CoTaskMemFree(Some(pidl as *const core::ffi::c_void));

        if result == 0 {
            return None;
        }
        let end = info.szDisplayName.iter().position(|&c| c == 0).unwrap_or(info.szDisplayName.len());
        let name = String::from_utf16_lossy(&info.szDisplayName[..end]);

        let icon = if info.hIcon.is_invalid() {
            None
        } else {
            let rgba = crate::gfx::icon::icon_to_rgba(info.hIcon);
            let _ = DestroyIcon(info.hIcon);
            rgba
        };
        Some((name, icon))
    }
}

/// Nom de l'exécutable qu'un lanceur Squirrel s'engage à démarrer :
/// `--processStart Discord.exe` → `Discord.exe`.
///
/// MESURÉ : l'épingle Discord de l'utilisateur pointe
/// `%LocalAppData%\Discord\Update.exe` avec les arguments
/// `--processStart Discord.exe`. `Update.exe` n'est qu'un lanceur — la fenêtre
/// appartient à `Discord.exe`. Chercher un processus `Update.exe` ne trouve
/// donc jamais rien : l'épingle restait sans pastille « ouverte » et un clic
/// relançait l'application au lieu de basculer dessus.
///
/// Le motif vaut pour TOUTES les applications empaquetées par Squirrel
/// (Discord, Slack, GitHub Desktop…), d'où la reconnaissance de l'OPTION
/// plutôt que du cas particulier. Les guillemets éventuels sont retirés :
/// rien ne garantit qu'un raccourci écrit à la main n'entoure pas le nom.
fn process_start_from_arguments(arguments: &str) -> Option<String> {
    let mut words = arguments.split_whitespace();
    while let Some(word) = words.next() {
        if word.eq_ignore_ascii_case("--processStart") {
            let name = words.next()?.trim_matches('"');
            return (!name.is_empty()).then(|| name.to_string());
        }
    }
    None
}

/// Nom d'exécutable porté par le chemin d'icône d'un raccourci, s'il en porte
/// un — `%windir%\explorer.exe` → `explorer.exe`.
///
/// MESURÉ : l'épingle « File Explorer » de Windows ne vise aucun fichier (son
/// raccourci pointe un élément du shell : sa cible est vide et
/// `System.Link.TargetExtension` aussi), et ses fenêtres n'exposent AUCUNE
/// identité — magasin de propriétés vide, aucune identité de paquet. La seule
/// chose qui nomme son application est l'icône qu'elle déclare,
/// `%windir%\explorer.exe`. C'est donc la dernière source d'identité essayée,
/// uniquement quand la cible du raccourci n'a rien donné.
///
/// **Ne rend un nom que pour un `.exe`** : c'est ce qui la garde honnête. Une
/// icône n'est pas forcément un exécutable — celle de Microsoft Edge sur cette
/// machine est un `.ico` de profil — et dans ce cas cette source doit se taire
/// plutôt que de proposer un « processus » qui n'en est pas un.
fn process_name_from_icon_path(icon: &Path) -> Option<String> {
    if !icon.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) {
        return None;
    }
    Some(icon.file_name()?.to_string_lossy().into_owned())
}

/// `%APPDATA%\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar`.
fn pins_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join(PINS_SUBDIR))
}

/// Position de la première occurrence de `needle` encodée en UTF-16LE dans
/// `haystack` — `None` si absente.
///
/// Recherche naïve : le blob fait quelques dizaines de kilo-octets et on
/// l'appelle une fois par raccourci (~20), donc un algorithme sous-linéaire
/// n'apporterait rien qu'une occasion de se tromper.
fn find_utf16(haystack: &[u8], needle: &str) -> Option<usize> {
    let pattern: Vec<u8> = needle.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    if pattern.is_empty() || pattern.len() > haystack.len() {
        return None;
    }
    haystack.windows(pattern.len()).position(|window| window == pattern.as_slice())
}

/// Extrait les AppUserModelID présents dans `section`, dans l'ordre
/// d'apparition et **sans doublon** (le blob répète chaque identifiant
/// plusieurs fois, dans des structures différentes : on ne garde que la
/// première occurrence, qui est celle qui porte la position).
///
/// Un AppUserModelID a la forme `PackageFamilyName!AppId` — le `!` est le
/// critère de reconnaissance, et il évite de ramasser au passage les chemins
/// d'installation `C:\Program Files\WindowsApps\...` qui parsèment la même
/// zone. Les chaînes sont lues aux DEUX parités d'octets : elles ne sont pas
/// alignées de la même façon dans toute la section (constaté à la mesure).
fn extract_aumids(section: &[u8]) -> Vec<(usize, String)> {
    let mut found: Vec<(usize, String)> = Vec::new();
    for parity in [0usize, 1] {
        let mut current = String::new();
        let mut start = parity;
        let mut i = parity;
        while i + 1 < section.len() {
            let unit = u16::from_le_bytes([section[i], section[i + 1]]);
            // Une unité imprimable prolonge la chaîne ; toute autre la termine.
            // `0xFFFF` (fin de chaîne Windows) et les caractères de contrôle
            // tombent dans ce second cas.
            if (0x20..0xFFFD).contains(&unit) {
                if current.is_empty() {
                    start = i;
                }
                current.push(char::from_u32(unit as u32).unwrap_or('\u{FFFD}'));
            } else {
                if looks_like_aumid(&current) && !found.iter().any(|(_, s)| *s == current) {
                    found.push((start, current.clone()));
                }
                current.clear();
            }
            i += 2;
        }
        if looks_like_aumid(&current) && !found.iter().any(|(_, s)| *s == current) {
            found.push((start, current));
        }
    }
    found.sort_by_key(|(offset, _)| *offset);
    found
}

/// Vrai si `s` a la FORME d'un AppUserModelID — `NomPaquet_Editeur!AppId`.
///
/// BUG RÉEL CONSTATÉ : deux icônes blanches sans nom apparaissaient dans la
/// barre, après Copilot. Ce ne sont pas des applications : le blob étant
/// BINAIRE, un simple test « contient un `!` » ramassait deux morceaux de
/// données quelconques contenant ce caractère (relevés au journal :
/// `"!"` et `"匱卐뜥䟯ယ怂麌곫!"`), que `shell:AppsFolder` ne résolvait alors en
/// rien du tout. D'où cette validation de FORME, qui est le vrai critère :
///
/// - **un seul `!`**, séparant le nom de paquet de l'identifiant d'application ;
/// - **l'identifiant d'application** : alphanumérique, points et tirets ;
/// - **le nom de paquet** : `Nom_Editeur`, où `Nom` est alphanumérique (points
///   et tirets admis) et où l'éditeur est une suite alphanumérique d'au moins
///   huit caractères — la forme que Windows donne TOUJOURS à un identifiant
///   d'éditeur (les 13 caractères de `8wekyb3d8bbwe`, par exemple).
///
/// Un paquet réellement installé ne peut pas échouer ces tests ; le bruit
/// binaire, lui, n'a aucune raison de les passer par hasard.
fn looks_like_aumid(s: &str) -> bool {
    let Some((package, app)) = s.split_once('!') else {
        return false;
    };
    if app.is_empty() || app.contains('!') {
        return false;
    }
    if !app.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return false;
    }
    let Some((name, publisher)) = package.rsplit_once('_') else {
        return false;
    };
    !name.is_empty()
        && publisher.len() >= 8
        && publisher.chars().all(|c| c.is_ascii_alphanumeric())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// Nom lisible de repli pour une application UWP absente de l'énumération des
/// paquets installés : `Microsoft.WindowsTerminal_8wekyb3d8bbwe!App` →
/// `WindowsTerminal`. Utilisé uniquement quand `uwp_apps::enumerate` n'a pas
/// retrouvé l'identifiant (paquet désinstallé mais épingle restée, par
/// exemple) — mieux vaut un nom approximatif qu'une icône sans nom.
fn name_from_aumid(aumid: &str) -> String {
    let package = aumid.split('!').next().unwrap_or(aumid);
    let without_publisher = package.rsplit_once('_').map(|(name, _)| name).unwrap_or(package);
    without_publisher
        .rsplit_once('.')
        .map(|(_, name)| name)
        .unwrap_or(without_publisher)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_utf16_locates_a_utf16le_string() {
        // Arrange : "abc" en UTF-16LE, précédé de deux octets parasites pour
        // vérifier qu'on ne dépend pas de l'alignement.
        let haystack = [0xFF, 0x00, b'a', 0x00, b'b', 0x00, b'c', 0x00, 0x00, 0x00];
        // Act
        let found = find_utf16(&haystack, "abc");
        // Assert
        assert_eq!(found, Some(2));
    }

    #[test]
    fn find_utf16_returns_none_for_absent_string() {
        let haystack = [b'a', 0x00, b'b', 0x00];
        assert_eq!(find_utf16(&haystack, "zz"), None);
    }

    /// Encode une chaîne en UTF-16LE suivie d'un séparateur nul — la forme sous
    /// laquelle le blob porte chaque nom.
    fn push_utf16(bytes: &mut Vec<u8>, s: &str) {
        for unit in s.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
    }

    /// Reproduit la disposition RÉELLE, mesurée sur la machine de l'utilisateur :
    /// les noms de raccourcis d'abord, et des identifiants d'applications
    /// empaquetées APRÈS eux (`Microsoft.YourPhone` à 12883 et WhatsApp à 15979,
    /// alors que « File Explorer.lnk » est à 6574).
    ///
    /// Une version précédente ne cherchait les identifiants que dans la partie du
    /// blob ANTÉRIEURE au premier raccourci : elle manquait donc ces deux
    /// épingles, et tout ce qui les suivait remontait d'un cran — DÉFAUT RÉEL
    /// SIGNALÉ : « l'épingle WhatsApp est à la mauvaise place ».
    #[test]
    fn extract_aumids_finds_packaged_apps_pinned_after_a_shortcut() {
        // Arrange
        let mut bytes = Vec::new();
        push_utf16(&mut bytes, "File Explorer.lnk");
        push_utf16(&mut bytes, "Discord.lnk");
        let after_shortcuts = bytes.len();
        push_utf16(&mut bytes, "Microsoft.YourPhone_8wekyb3d8bbwe!App");
        push_utf16(&mut bytes, "5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App");

        // Act
        let aumids = extract_aumids(&bytes);

        // Assert : les deux SONT trouvés, dans l'ordre de leurs décalages, et
        // bien APRÈS les raccourcis — c'est tout l'objet du test.
        assert_eq!(aumids.len(), 2, "{aumids:?}");
        assert_eq!(aumids[0].1, "Microsoft.YourPhone_8wekyb3d8bbwe!App");
        assert_eq!(aumids[1].1, "5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App");
        assert!(aumids[0].0 >= after_shortcuts, "trouvé avant les raccourcis, à {}", aumids[0].0);
        assert!(aumids[0].0 < aumids[1].0, "l'ordre doit suivre les décalages");
    }

    /// Lecture RÉELLE de la barre des tâches de cette machine — ignoré par
    /// défaut, comme `win32::notification::tests::reads_the_real_database` : il
    /// dépend de ce qui y est épinglé, donc il n'a rien à faire dans une suite
    /// hors ligne. Il sert à VÉRIFIER À LA MAIN l'ordre produit, et donc la
    /// position de chaque épingle — ce qu'aucun test hors ligne ne peut faire :
    ///
    ///     cargo test --bins -- --ignored --nocapture read_pins
    #[test]
    #[ignore = "dépend des épingles de la barre des tâches de la machine"]
    fn read_pins_prints_the_real_taskbar_order() {
        let pins = read_pins();
        for (i, pin) in pins.iter().enumerate() {
            println!(
                "#{i:2} icone={} identite={} nom='{}'",
                if pin.icon_rgba.is_some() { "OUI" } else { "NON " },
                match &pin.target {
                    Some(WindowTarget::Process(name)) => format!("proc:{name}"),
                    Some(WindowTarget::Aumid(aumid)) => format!("aumid:{aumid}"),
                    Some(WindowTarget::Profile(id)) => format!("profil:{id}"),
                    None => "-".to_string(),
                },
                pin.name
            );
        }
        assert!(!pins.is_empty(), "aucune épingle lue : la barre des tâches ou le blob a changé de forme");
    }

    #[test]
    fn extract_aumids_keeps_first_occurrence_and_ignores_install_paths() {
        // Arrange : un chemin d'installation WindowsApps (à ignorer, pas de
        // `!`) puis un identifiant répété deux fois.
        let mut bytes = Vec::new();
        for s in [
            r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.0.0_x64__8wekyb3d8bbwe",
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
        ] {
            push_utf16(&mut bytes, s);
        }
        // Act
        let aumids = extract_aumids(&bytes);
        // Assert : un seul identifiant, et le chemin d'installation a été écarté.
        assert_eq!(aumids.len(), 1);
        assert_eq!(aumids[0].1, "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App");
    }

    #[test]
    fn looks_like_aumid_rejects_binary_noise() {
        // Les deux entrées fautives relevées en pratique : du bruit binaire
        // qui contenait un `!` et se faisait passer pour une application.
        assert!(!looks_like_aumid("!"));
        assert!(!looks_like_aumid("匱卐뜥䟯ယ怂麌곫!"));
        // Un vrai identifiant, et ses variantes proches mais invalides.
        assert!(looks_like_aumid("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"));
        assert!(!looks_like_aumid("Microsoft.WindowsTerminal!App"), "éditeur trop court");
        assert!(!looks_like_aumid("Microsoft.WindowsTerminal_8wekyb3d8bbwe!"), "identifiant vide");
        assert!(!looks_like_aumid("Microsoft.WindowsTerminal_8wekyb3d8bbwe!A!B"), "deux `!`");
    }

    #[test]
    fn process_start_from_arguments_reads_the_squirrel_launcher_option() {
        // Le cas réel, relevé sur l'épingle Discord de cette machine.
        assert_eq!(
            process_start_from_arguments("--processStart Discord.exe").as_deref(),
            Some("Discord.exe")
        );
        // Guillemets, option en second, et casse indifférente.
        assert_eq!(
            process_start_from_arguments("--silent --processstart \"Slack.exe\"").as_deref(),
            Some("Slack.exe")
        );
        // Un raccourci ordinaire n'en a pas : c'est la cible qui identifie.
        assert_eq!(process_start_from_arguments(""), None);
        assert_eq!(process_start_from_arguments("--profile-directory=Default"), None);
        // Option présente mais sans valeur : ne pas inventer un nom.
        assert_eq!(process_start_from_arguments("--processStart"), None);
    }

    #[test]
    fn process_name_from_icon_path_only_trusts_executables() {
        // Le cas réel : l'épingle « File Explorer », dont la cible n'est pas
        // résoluble et dont l'icône nomme l'application.
        assert_eq!(
            process_name_from_icon_path(Path::new(r"C:\Windows\explorer.exe")).as_deref(),
            Some("explorer.exe")
        );
        // Une icône qui n'est PAS un exécutable ne dit rien sur le processus —
        // celle de Microsoft Edge est un `.ico` de profil.
        assert_eq!(process_name_from_icon_path(Path::new(r"C:\...\Edge Profile.ico")), None);
        assert_eq!(process_name_from_icon_path(Path::new(r"C:\...\Discord\app.ico")), None);
        assert_eq!(process_name_from_icon_path(Path::new(r"C:\dossier\sans.extension")), None);
    }

    #[test]
    fn name_from_aumid_strips_publisher_and_package_prefix() {
        assert_eq!(name_from_aumid("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"), "WindowsTerminal");
        assert_eq!(name_from_aumid("Microsoft.PowerShell_8wekyb3d8bbwe!App"), "PowerShell");
    }
}
