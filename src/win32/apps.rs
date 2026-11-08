//! Découverte des applications installées : scanne les raccourcis du menu
//! Démarrer (`.lnk`), les résout vers leur exécutable cible, ET énumère les
//! applications UWP/Store (voir `win32::uwp_apps`, WhatsApp, Xbox...) qui
//! n'ont PAS de raccourci `.lnk` — les deux univers cohabitent dans
//! `AppEntry` via `AppLaunch`, qui abstrait la seule vraie différence entre
//! eux (comment on les lance, voir sa doc). Chaque `AppEntry` porte aussi
//! une description et une catégorie — un thème d'usage généraliste
//! (Internet, Jeux, Développement...), voir `app_categories`, PAS le
//! sous-dossier brut du menu Démarrer (souvent un dossier par éditeur,
//! inutilisable comme thème général, voir sa doc).
//! Logique exploitée par `win32::dash` (lanceur en grille façon Dash Unity,
//! qui utilise la catégorie/description) — partagée à l'origine avec
//! l'ancien module `win32::launcher` (lanceur façon Rofi, retiré depuis).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use windows::core::{Interface, PCWSTR};
use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

use crate::win32::app_categories;
use crate::win32::appicon;
use crate::win32::epic_apps;
use crate::win32::exe_metadata;
use crate::win32::gog_apps;
use crate::win32::portables;
use crate::win32::steam_apps;
use crate::win32::uwp_apps;

/// Comment lancer une `AppEntry` — la seule différence structurelle entre un
/// raccourci `.lnk` classique et une application UWP/Store (voir
/// `win32::uwp_apps`) : ni exécutable, ni chemin de fichier classique pour
/// cette dernière, seulement un identifiant virtuel à passer à
/// `shell:AppsFolder\...`.
#[derive(Clone)]
pub enum AppLaunch {
    /// Raccourci `.lnk` classique, résolu vers son exécutable cible.
    Shortcut {
        target: PathBuf,
        /// Chemin du raccourci `.lnk` lui-même (pas `target`) — c'est CE
        /// chemin qu'il faut interroger pour l'icône (voir
        /// `win32::dash::icon_data_uri`, `SHGetFileInfoW`), pas la cible :
        /// un raccourci peut définir sa propre icône (`IShellLinkW::
        /// SetIconLocation`), différente de celle de son exécutable cible,
        /// et Windows lui-même résout ce genre de cas en interrogeant le
        /// raccourci, jamais la cible directement.
        shortcut_path: PathBuf,
    },
    /// Application UWP/Store — voir `win32::uwp_apps`. `app_user_model_id`
    /// est le `PackageFamilyName!AppId` à passer tel quel dans
    /// `shell:AppsFolder\<...>` (voir `launch`).
    Uwp { app_user_model_id: String },
    /// Lancée via un protocole personnalisé (`schéma://...`, voir
    /// `win32::appicon::open`) plutôt qu'un exécutable/chemin classique —
    /// le cas des jeux Steam (voir `win32::steam_apps`), qui n'ont pas de
    /// chemin de fichier "lançable" directement : Steam lui-même résout
    /// `steam://rungameid/<appid>` en interne, exactement comme un
    /// raccourci de bureau Steam le ferait. Cette variante ne porte PAS
    /// d'icône elle-même (contrairement à `Shortcut`, où `shortcut_path`
    /// sert aussi de source d'icône) : voir `AppEntry::icon_path`, résolu
    /// séparément (voir `win32::steam_apps::SteamApp::icon_path`) — la
    /// source d'icône et le mécanisme de lancement n'ont ici rien à voir
    /// l'un avec l'autre.
    UriProtocol { uri: String },
}

/// Une application installée, classique (raccourci du menu Démarrer résolu
/// vers son exécutable cible) ou UWP/Store (voir `AppLaunch`).
pub struct AppEntry {
    pub name: String,
    /// Champ "Commentaire" du raccourci (`IShellLinkW::GetDescription`) pour
    /// un raccourci classique — toujours vide pour une application UWP
    /// (`AppListEntry` n'expose pas d'équivalent). Chaîne vide si absente.
    /// Exploité par `win32::dash` pour chercher aussi par description (pas
    /// seulement par nom) et pour l'afficher en infobulle, comme le fait
    /// Windows lui-même.
    pub description: String,
    /// Thèmes d'usage généralistes auxquels l'application appartient —
    /// soit une seule entrée, le résultat du classement automatique (voir
    /// `app_categories::classify`), soit une ou plusieurs entrées choisies à
    /// la main par l'utilisateur, qui prévalent alors entièrement sur
    /// l'automatique (voir `overridden`, `crate::config::
    /// add_category_override`) — un programme peut être rangé dans
    /// plusieurs catégories à la fois (ex. un jeu qu'on veut retrouver à la
    /// fois sous "Jeux" et sous "Développement"). Chaque nom est une chaîne
    /// possédée (pas `&'static str` comme les catégories fixes) : un choix
    /// manuel peut être n'importe quel nom que l'utilisateur a tapé, y
    /// compris une catégorie qu'il vient de créer (voir `crate::config::
    /// save_custom_category`). Toujours NON VIDE : `win32::dash` s'appuie
    /// dessus (`categories[0]` pour le tri) sans vérification supplémentaire.
    pub categories: Vec<String>,
    /// `true` si `categories` vient d'un choix manuel de l'utilisateur (voir
    /// `crate::config::load_category_overrides`) plutôt que du classement
    /// automatique — exploité par `win32::dash` pour proposer de
    /// réinitialiser au classement automatique dans le sélecteur de
    /// catégorie, une option qui n'a pas de sens tant qu'aucun choix manuel
    /// n'a été fait.
    pub overridden: bool,
    pub launch: AppLaunch,
    /// Logo déjà décodé en RGBA8 (largeur, hauteur, pixels) pour une
    /// application UWP (voir `uwp_apps::UwpApp::icon_rgba`) — toujours
    /// `None` pour un raccourci classique, dont l'icône est extraite
    /// paresseusement à la demande à partir de `shortcut_path` (voir
    /// `win32::dash::icon_data_uri`).
    pub icon_rgba: Option<(u32, u32, Vec<u8>)>,
    /// Chemin d'un fichier icône à utiliser EN PRIORITÉ sur ce que
    /// `AppLaunch` impliquerait normalement (voir `win32::dash::
    /// icon_data_uri`) — pour les cas où la source de l'icône n'est PAS le
    /// même chemin que celui utilisé pour LANCER l'application. Le seul cas
    /// aujourd'hui : les jeux Steam (voir `win32::steam_apps::SteamApp::
    /// icon_path`), lancés via `AppLaunch::UriProtocol` (aucun chemin de
    /// fichier "lançable"), mais dont l'icône vient d'un `.ico` mis en
    /// cache localement par Steam lui-même, à un chemin sans rapport avec
    /// le mécanisme de lancement. `None` pour toute application dont
    /// l'icône se déduit déjà de `AppLaunch` (raccourci classique — y
    /// compris GOG/Epic, qui réutilisent `AppLaunch::Shortcut` — ou UWP).
    pub icon_path: Option<PathBuf>,
}

impl AppLaunch {
    /// Lance l'application par le mécanisme approprié à son type — appelé
    /// via `app.launch.launch()` (champ `launch: AppLaunch` puis méthode du
    /// même nom sur ce type, volontairement gardé distinct d'une méthode
    /// `AppEntry::launch` qui prêterait à confusion avec ce champ).
    pub fn launch(&self) {
        match self {
            AppLaunch::Shortcut { target, .. } => appicon::launch(target),
            // Même syntaxe que la barre d'adresse de l'Explorateur pour
            // ouvrir une appli UWP par son identifiant (voir doc de
            // `win32::uwp_apps`) — PAS `appicon::launch` : une appli UWP n'a
            // pas d'exécutable classique à passer à `ShellExecuteW`.
            AppLaunch::Uwp { app_user_model_id } => {
                appicon::open(&format!(r"shell:AppsFolder\{app_user_model_id}"))
            }
            AppLaunch::UriProtocol { uri } => appicon::open(uri),
        }
    }

    /// `true` si cette variante peut être lancée avec élévation (voir
    /// `launch_as_admin`) — un raccourci classique le permet toujours (voir
    /// `Shortcut`, qui pointe vers un exécutable réel — GOG/Epic
    /// réutilisent cette variante, voir `apps::enumerate_apps`, donc leurs
    /// jeux le permettent aussi), et une application UWP/Store (`Uwp`)
    /// aussi : `shell:AppsFolder\<AUMID>` + verbe `"runas"` fonctionne pour
    /// les applications empaquetées de type "Desktop Bridge" (ex. Windows
    /// Terminal, PowerToys — la même technique que l'équipe Windows
    /// Terminal utilise elle-même, voir `appicon::open_as_admin`) ; pour
    /// les autres (vraiment isolées en bac à sable AppContainer), Windows
    /// refuse silencieusement l'élévation — on propose donc l'option dans
    /// tous les cas et on laisse Windows trancher, plutôt que de deviner
    /// à l'avance quelles apps UWP le permettent. Seul un jeu lancé par
    /// protocole (`UriProtocol` — Steam) ne le permet pas : aucun moyen de
    /// faire transiter un verbe `"runas"` à travers une URI `steam://...`.
    pub fn can_run_as_admin(&self) -> bool {
        matches!(self, AppLaunch::Shortcut { .. } | AppLaunch::Uwp { .. })
    }

    /// Lance l'application AVEC ÉLÉVATION (voir `can_run_as_admin`) — no-op
    /// si cette variante ne le permet pas (voir sa doc), plutôt que de
    /// tenter une action qui échouerait de toute façon silencieusement.
    pub fn launch_as_admin(&self) {
        match self {
            AppLaunch::Shortcut { target, .. } => appicon::launch_as_admin(target),
            AppLaunch::Uwp { app_user_model_id } => {
                appicon::open_as_admin(&format!(r"shell:AppsFolder\{app_user_model_id}"));
            }
            AppLaunch::UriProtocol { .. } => {}
        }
    }
}

/// Dossiers "Menu Démarrer" standards (utilisateur courant + tous les
/// utilisateurs) — les mêmes que Windows lui-même scanne pour peupler son
/// propre menu Démarrer.
fn start_menu_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(program_data) = std::env::var_os("ProgramData") {
        dirs.push(PathBuf::from(program_data).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    dirs
}

/// Nombre maximum d'applications marquées "Fréquemment utilisés" (voir
/// `app_categories::FREQUENT`) — assez pour être utile d'un coup d'œil sans
/// finir par lister la moitié des applications installées si l'utilisateur
/// se sert activement du Dash depuis longtemps.
///
/// ⭐ **25** — DEMANDE EXPLICITE (2026-10-14) : « jusqu'à 25 programmes listés
/// dans les programmes fréquemment utilisés ». C'était 8.
const FREQUENT_APPS_LIMIT: usize = 25;

/// **Les clés des applications « fréquemment utilisées » MAINTENANT** — les
/// `FREQUENT_APPS_LIMIT` plus lancées, d'après `counts` (voir
/// `config::load_launch_counts`). Clés en minuscules, comme `record_launch` les
/// écrit — ⛔ **PAS de `trim()`** : `record_launch` n'en fait pas non plus, et un
/// `trim` d'un seul côté ferait diverger les deux moitiés de la même clé.
///
/// ⛔ **C'EST UN CALCUL DE RENDU, ET C'EST TOUT L'OBJET.** Il vivait dans
/// `enumerate_apps`, donc dans le CACHE — et l'usage réel change à chaque
/// lancement alors que le cache, lui, n'est réécrit que sur action explicite.
/// Résultat mesuré le 2026-10-07 : `usage.toml` s'incrémentait bien (écrit à
/// 21:53) pendant que `apps_cache.toml` datait du **4 octobre** — la catégorie
/// est restée figée trois jours, sans que rien ne le signale.
///
/// ⚠️ **NE CONSIDÈRE QUE LES APPLICATIONS PRÉSENTES DANS `counts`** : une
/// application jamais lancée depuis le Dash n'y figure pas du tout, et ne peut
/// donc pas être « fréquente » — c'est le comportement d'origine, conservé.
///
/// ⚠️ **ELLE PREND DES NOMS, PAS DES `AppEntry`** : c'est tout ce dont elle a
/// besoin, et `AppEntry` a une quinzaine de champs qu'un test n'a aucune raison
/// de devoir remplir pour éprouver un classement.
pub fn cles_frequentes<'a>(
    noms: impl Iterator<Item = &'a str>,
    counts: &std::collections::HashMap<String, u64>,
) -> std::collections::HashSet<String> {
    let mut ranked: Vec<String> =
        noms.map(|n| n.to_lowercase()).filter(|cle| counts.contains_key(cle)).collect();
    // Tri STABLE : à égalité de compte, l'ordre d'énumération décide. Il ne
    // dépend donc que de la liste, pas de l'ordre d'arrivée des fichiers.
    ranked.sort_by_key(|cle| std::cmp::Reverse(counts.get(cle).copied().unwrap_or(0)));
    ranked.into_iter().take(FREQUENT_APPS_LIMIT).collect()
}

/// Les catégories d'une application, la marque « Fréquemment utilisés »
/// **recalculée** : retirée si elle n'en fait plus partie, ajoutée sinon.
///
/// ⚠️ **LE RETRAIT COMPTE AUTANT QUE L'AJOUT** : la marque est cuite dans
/// `apps_cache.toml` (écrit par l'ancien code), donc une application qui a
/// cessé d'être fréquente la PORTE ENCORE. Sans ce filtre, elle resterait
/// affichée indéfiniment — le défaut, en plus discret.
///
/// ⚠️ **AJOUTÉE EN FIN**, après les choix manuels, jamais à leur place : une
/// application reste « fréquemment utilisée » quelle que soit la façon dont elle
/// a été classée par ailleurs (voir `enumerate_apps`).
pub fn categories_avec_frequents(categories: &[String], frequente: bool) -> Vec<String> {
    let mut out: Vec<String> =
        categories.iter().filter(|c| c.as_str() != app_categories::FREQUENT).cloned().collect();
    if frequente {
        out.push(app_categories::FREQUENT.to_string());
    }
    // ⚠️ **REPLI** : une application dont « Fréquemment utilisés » serait la
    // SEULE catégorie ne doit pas se retrouver sans aucune catégorie — la page
    // la ferait disparaître de tous les groupes, donc de la liste. On garde
    // alors les catégories d'origine telles quelles.
    if out.is_empty() {
        return categories.to_vec();
    }
    out
}

/// Scanne récursivement les dossiers du menu Démarrer à la recherche de
/// raccourcis (`.lnk`), résout chacun (voir `resolve_shortcut`), puis
/// complète avec les applications UWP/Store (voir `uwp_apps::enumerate`,
/// invisibles à ce scan de dossiers — voir sa doc de module) ET les jeux
/// installés via un launcher qui gère sa propre bibliothèque sans jamais
/// créer de raccourci par jeu (voir `steam_apps`/`gog_apps`/`epic_apps`,
/// chacune de leur propre doc de module explique pourquoi) — déduplique
/// par nom (voir `dedup_key` : insensible à la casse ET à un éventuel
/// suffixe entre crochets, plusieurs sources ne gardent que la première
/// trouvée, dans l'ordre : raccourcis `.lnk`, UWP, Steam, GOG, Epic).
/// Trié par catégorie puis par nom, pour un affichage groupé direct côté
/// appelant (voir `win32::dash`).
pub fn enumerate_apps() -> Vec<AppEntry> {
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    // Les programmes PORTABLES ajoutés par l'utilisateur (voir
    // `win32::portables`) — EN PREMIER : choisis explicitement, ils gagnent la
    // déduplication contre un raccourci du même nom. Un exécutable absent (clé
    // USB débranchée) est masqué, comme un raccourci mort.
    for exe in portables::charger() {
        let name = portables::nom(&exe);
        if name.is_empty() || !cible_presente(&exe) || !seen.insert(dedup_key(&name)) {
            continue;
        }
        let mut category = app_categories::classify(&name, "");
        if category == app_categories::OTHER {
            if let Some(company) = exe_metadata::company_name(&exe) {
                category = app_categories::classify_by_publisher(&company);
            }
        }
        apps.push(AppEntry {
            name,
            description: String::new(),
            categories: vec![category.to_string()],
            overridden: false,
            launch: AppLaunch::Shortcut { target: exe.clone(), shortcut_path: exe },
            icon_rgba: None,
            icon_path: None,
        });
    }
    for dir in start_menu_dirs() {
        walk_lnk_files(&dir, None, &mut apps, &mut seen);
    }
    for uwp in uwp_apps::enumerate() {
        if !seen.insert(dedup_key(&uwp.name)) {
            continue; // déjà vu via un raccourci .lnk : on le garde
        }
        // Pas de dossier de menu Démarrer propre à interroger pour une
        // appli UWP : `classify` retombe alors uniquement sur ses mots-clés
        // de nom. Pas de repli par éditeur non plus (voir
        // `walk_lnk_files`) : il n'y a pas d'exécutable classique à lire
        // avec `exe_metadata::company_name`.
        let category = app_categories::classify(&uwp.name, "").to_string();
        apps.push(AppEntry {
            name: uwp.name,
            description: String::new(),
            categories: vec![category],
            overridden: false,
            launch: AppLaunch::Uwp { app_user_model_id: uwp.app_user_model_id },
            icon_rgba: uwp.icon_rgba,
            icon_path: None,
        });
    }

    // Jeux Steam (voir `steam_apps`) — comme pour les applications UWP,
    // aucun raccourci de menu Démarrer par jeu à scanner : Steam gère sa
    // bibliothèque dans ses propres fichiers VDF, lus directement. Classées
    // "Jeux" (voir `app_categories::GAMES`) sans passer par `classify` : la
    // source elle-même garantit que ce sont des jeux, plus fiable qu'un
    // classement par mot-clé sur le nom.
    for steam_app in steam_apps::enumerate() {
        if !seen.insert(dedup_key(&steam_app.name)) {
            continue;
        }
        apps.push(AppEntry {
            name: steam_app.name,
            description: String::new(),
            categories: vec![app_categories::GAMES.to_string()],
            overridden: false,
            launch: AppLaunch::UriProtocol { uri: format!("steam://rungameid/{}", steam_app.appid) },
            icon_rgba: None,
            icon_path: steam_app.icon_path,
        });
    }

    // Jeux GOG installés via Galaxy sans raccourci de menu Démarrer (voir
    // `gog_apps`) — ceux installés via un installeur GOG autonome ont déjà
    // été trouvés ci-dessus par `walk_lnk_files` ; `dedup_key` évite de les
    // compter deux fois malgré le suffixe "[GOG.com]" que ces raccourcis
    // portent. Un vrai chemin d'exécutable est disponible (contrairement à
    // Steam) : réutilisé tel quel comme un raccourci classique
    // (`AppLaunch::Shortcut`), icône et lancement inclus sans code
    // supplémentaire.
    for gog_app in gog_apps::enumerate() {
        if !seen.insert(dedup_key(&gog_app.name)) {
            continue;
        }
        apps.push(AppEntry {
            name: gog_app.name,
            description: String::new(),
            categories: vec![app_categories::GAMES.to_string()],
            overridden: false,
            launch: AppLaunch::Shortcut { target: gog_app.exe.clone(), shortcut_path: gog_app.exe },
            icon_rgba: None,
            icon_path: None,
        });
    }

    // Jeux Epic Games installés (voir `epic_apps`) — même raisonnement que
    // Steam (pas de raccourci de menu Démarrer par jeu), mais comme GOG un
    // vrai chemin d'exécutable est disponible, réutilisé tel quel.
    for epic_app in epic_apps::enumerate() {
        if !seen.insert(dedup_key(&epic_app.name)) {
            continue;
        }
        apps.push(AppEntry {
            name: epic_app.name,
            description: String::new(),
            categories: vec![app_categories::GAMES.to_string()],
            overridden: false,
            launch: AppLaunch::Shortcut { target: epic_app.exe.clone(), shortcut_path: epic_app.exe },
            icon_rgba: None,
            icon_path: None,
        });
    }

    // Choix manuels de catégorie (voir `crate::config::
    // add_category_override`) : appliqués en dernier, une fois la liste
    // complète (raccourcis + UWP) constituée, pour prévaloir uniformément
    // sur le classement automatique quelle que soit son origine. Remplacent
    // ENTIÈREMENT les catégories automatiques (pas une fusion) : une
    // application avec au moins un choix manuel n'apparaît que dans les
    // catégories manuelles choisies, jamais aussi dans celle du classement
    // automatique.
    appliquer_les_choix_manuels(&mut apps, &crate::config::load_category_overrides());

    // ⛔ **LA CATÉGORIE « FRÉQUEMMENT UTILISÉS » N'EST PLUS CALCULÉE ICI.**
    // Elle l'était, et c'était le défaut : `enumerate_apps` est CHÈRE (un
    // balayage du menu Démarrer) donc son résultat est MIS EN CACHE
    // (`apps_cache.toml`), alors que l'usage réel change à CHAQUE lancement. Cuire
    // l'un dans l'autre fige la catégorie : le cache n'étant réécrit que sur
    // action explicite, « Fréquemment utilisés » est resté inchangé du 4 au
    // 7 octobre 2026 pendant que `usage.toml`, lui, continuait de s'incrémenter.
    //
    // ⭐ **ELLE SE CALCULE MAINTENANT AU RENDU**, dans `cef_dash::push_state`, par
    // `cles_frequentes` — quelques centaines d'octets à relire, contre un balayage
    // du menu Démarrer. Voir `cles_frequentes`/`categories_avec_frequents`.

    // `is_uninstall` en tête de la clé de tri : `false < true` en Rust,
    // donc toute application dont la catégorie PRINCIPALE est
    // `app_categories::UNINSTALL` finit systématiquement après toutes les
    // autres, quel que soit son nom — sans ça, "Désinstallation" se
    // classerait alphabétiquement comme n'importe quelle autre catégorie
    // (entre "Développement" et "Éducation"), pas nécessairement en
    // dernier. Voir aussi `dash_template.html`, `UNINSTALL_CATEGORY`,
    // même traitement appliqué côté JS à la barre latérale des catégories.
    apps.sort_by(|a, b| {
        let a_is_uninstall = a.categories[0] == app_categories::UNINSTALL;
        let b_is_uninstall = b.categories[0] == app_categories::UNINSTALL;
        (a_is_uninstall, a.categories[0].to_lowercase(), a.name.to_lowercase())
            .cmp(&(b_is_uninstall, b.categories[0].to_lowercase(), b.name.to_lowercase()))
    });
    apps
}

/// Une catégorie proposée par le sélecteur du Dash (voir `all_categories_info`),
/// annotée pour indiquer si elle peut être supprimée : `custom: true` pour
/// une catégorie créée par l'utilisateur (voir `crate::config::
/// save_custom_category`/`delete_custom_category`), `custom: false` pour une
/// catégorie fixe (voir `app_categories::fixed_categories`), qui fait partie
/// du classement intégré au code et ne peut pas être supprimée.
pub struct CategoryInfo {
    pub name: String,
    pub custom: bool,
    /// URL d'installation à proposer si CETTE catégorie se retrouve vide
    /// (voir `app_categories::install_url`, `win32::dash`) — `None` pour
    /// la quasi-totalité des catégories, qui n'ont rien de tel à proposer.
    /// Toujours `None` pour une catégorie personnalisée : seules les
    /// catégories fixes (voir `app_categories::INSTALL_PROMPTS`) le
    /// portent.
    pub install_url: Option<&'static str>,
}

/// Combine les catégories fixes (voir `app_categories::fixed_categories`)
/// et les catégories supplémentaires créées par l'utilisateur (voir
/// `crate::config::load_custom_categories`), chacune annotée (voir
/// `CategoryInfo`) — la liste complète proposée par le sélecteur de
/// catégorie du Dash (voir `win32::dash`), contrairement à la barre
/// latérale qui ne montre que les catégories effectivement peuplées (voir
/// `enumerate_apps`).
/// Les choix manuels de catégorie (`config::load_category_overrides`, clés en
/// minuscules) REMPLACENT les catégories automatiques des applications qu'ils
/// citent. Partagé par l'énumération et par le Dash, qui les réapplique à sa liste
/// en mémoire juste après un changement (voir `cef_dash::rafraichir_tout_de_suite`)
/// sans attendre le rescan.
pub fn appliquer_les_choix_manuels(apps: &mut [AppEntry], overrides: &std::collections::HashMap<String, Vec<String>>) {
    for app in apps {
        if let Some(categories) = overrides.get(&app.name.to_lowercase()) {
            app.categories = categories.clone();
            app.overridden = true;
        }
    }
}

pub fn all_categories_info() -> Vec<CategoryInfo> {
    let mut infos: Vec<CategoryInfo> = app_categories::fixed_categories()
        .into_iter()
        .map(|name| CategoryInfo {
            install_url: app_categories::install_url(name),
            name: name.to_string(),
            custom: false,
        })
        .collect();
    infos.extend(
        crate::config::load_custom_categories()
            .into_iter()
            .map(|name| CategoryInfo { name, custom: true, install_url: None }),
    );
    infos
}

/// Clé de déduplication d'un nom d'application — insensible à la casse
/// (comme avant l'ajout des bibliothèques de jeux lues directement), ET
/// insensible à un éventuel suffixe entre crochets en fin de nom (ex.
/// "Baldur's Gate 3 [GOG.com]" → même clé que "Baldur's Gate 3") : les
/// installeurs GOG autonomes décorent ainsi le nom du raccourci qu'ils
/// créent dans le menu Démarrer, alors que `gog_apps`/`steam_apps`/
/// `epic_apps` (qui lisent la bibliothèque du launcher directement, voir
/// leur doc de module) donnent le nom "propre" — sans cette normalisation,
/// le même jeu apparaîtrait deux fois dans le Dash.
fn dedup_key(name: &str) -> String {
    let trimmed = name.trim();
    let key = trimmed
        .strip_suffix(']')
        .and_then(|s| s.rfind('[').map(|i| s[..i].trim_end()))
        .unwrap_or(trimmed);
    key.to_lowercase()
}

/// `folder_hint` : `None` tant qu'on est encore à la racine d'un dossier
/// "Menu Démarrer" ; passe à `Some(nom du sous-dossier)` dès qu'on descend
/// d'un niveau, et reste alors inchangé aux niveaux suivants — sert
/// uniquement à reconnaître les dossiers propres à Windows lui-même (voir
/// `app_categories::classify`), pas à devenir la catégorie affichée telle
/// quelle (voir `AppEntry::category`).
fn walk_lnk_files(dir: &Path, folder_hint: Option<&str>, apps: &mut Vec<AppEntry>, seen: &mut HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let sub_hint = folder_hint.or_else(|| path.file_name().and_then(|n| n.to_str()));
            walk_lnk_files(&path, sub_hint, apps, seen);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
            let Some(name) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
            if let Some(shortcut) = resolve_shortcut(&path) {
                // Sans cible fichier (raccourci vers un élément du SHELL), il
                // n'y a rien à lancer ni à classifier : cette liste n'en veut
                // pas — même comportement qu'avant, où `resolve_shortcut`
                // rendait `None` dans ce cas. Voir `Shortcut::target`.
                let Some(target) = shortcut.target else { continue };
                // Un raccourci MORT (son programme n'existe plus) est masqué —
                // voir `cible_presente`. **AVANT la déduplication par nom** : un
                // raccourci mort ne doit pas réserver un nom et cacher un
                // raccourci vivant du même nom, rangé dans un autre dossier.
                if !cible_presente(&target) {
                    continue;
                }
                if !seen.insert(dedup_key(&name)) {
                    continue; // déjà vu (autre dossier) : garde le premier
                }
                let description = shortcut.description;
                let mut category = app_categories::classify(&name, folder_hint.unwrap_or(""));
                // Repli par éditeur (voir sa doc) : seulement dans ce cas
                // précis, pas pour chaque application — c'est le seul qui
                // a besoin de lire l'exécutable lui-même (plus coûteux
                // qu'une simple comparaison de chaînes).
                if category == app_categories::OTHER {
                    if let Some(company) = exe_metadata::company_name(&target) {
                        category = app_categories::classify_by_publisher(&company);
                    }
                }
                apps.push(AppEntry {
                    name,
                    description,
                    categories: vec![category.to_string()],
                    overridden: false,
                    launch: AppLaunch::Shortcut { target, shortcut_path: path },
                    icon_rgba: None,
                    icon_path: None,
                });
            }
        }
    }
}

/// Le programme visé par un raccourci existe-t-il ? DEMANDE EXPLICITE
/// (2026-10-04) : « masque les raccourcis dont le programme n'existe plus ».
/// Défaut RÉEL qui l'a motivée : « NASP2 Document Scanner.lnk », resté dans le
/// menu Démarrer d'une ancienne installation, visait
/// `C:\Program Files (x86)\NAPS2\NAPS2.exe` alors que le programme vit
/// désormais dans `C:\Program Files` (raccourci vivant à côté : « NAPS2 »).
/// Épinglé, il donnait un CARRÉ BLANC — pas d'exécutable, donc pas d'icône.
///
/// ⛔ **UNE CIBLE RÉSEAU N'EST PAS VÉRIFIÉE, ET C'EST VOULU** : `exists()` sur un
/// partage injoignable peut bloquer de longues secondes, et `enumerate_apps` est
/// aussi appelée depuis la boucle principale (voir `app::run`, détection de la
/// zone de notification). Un chemin UNC (`\\serveur\…`) ou un lecteur réseau
/// mappé est donc tenu pour présent. Même piège que celui documenté pour les
/// chemins des Réglages (un `is_dir` sur un UNC gelait l'application).
fn cible_presente(target: &Path) -> bool {
    let texte = target.as_os_str().to_string_lossy();
    if texte.starts_with(r"\\") {
        return true;
    }
    let Some(lecteur) = texte.get(..3).filter(|r| r.ends_with(":\\")) else {
        // Ni UNC ni lecteur : un chemin qu'on ne sait pas juger — gardé.
        return true;
    };
    let racine: Vec<u16> = lecteur.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY : `racine` est une chaîne UTF-16 terminée par un zéro, vivante
    // pendant tout l'appel.
    let type_de_lecteur = unsafe { windows::Win32::Storage::FileSystem::GetDriveTypeW(PCWSTR(racine.as_ptr())) };
    if type_de_lecteur == windows::Win32::System::WindowsProgramming::DRIVE_REMOTE {
        return true;
    }
    target.exists()
}

/// Ce qu'un raccourci `.lnk` déclare — la cible et les arguments que
/// `ShellExecuteW` utiliserait pour le lancer, plus la description
/// (champ "Commentaire") que l'Explorateur affiche en info-bulle.
///
/// Une STRUCT plutôt qu'un triplet : les trois champs sont des `String`/
/// `PathBuf`, et un `(PathBuf, String, String)` se lit mal au point d'appel.
pub(crate) struct Shortcut {
    /// Cible résolue. **`None` ne veut PAS dire « raccourci cassé »** : c'est
    /// aussi le cas d'un raccourci qui vise un élément du SHELL plutôt qu'un
    /// fichier. MESURÉ sur l'épingle « File Explorer » de Windows, qui est
    /// parfaitement valide et dont `IShellLinkW::GetPath` ne rend pourtant
    /// rien (son `System.Link.TargetExtension` est vide lui aussi). La
    /// distinction compte : voir `win32::taskbar_pins`, qui tire alors
    /// l'application de l'icône que le raccourci déclare.
    pub target: Option<PathBuf>,
    pub description: String,
    /// Ligne de commande, telle quelle. Sert à reconnaître une application
    /// lancée par un LANCEUR plutôt que directement — voir
    /// `win32::taskbar_pins::process_start_from_arguments`.
    pub arguments: String,
    /// Chemin d'icône que le raccourci DÉCLARE (`GetIconLocation`), variables
    /// d'environnement développées. `None` quand il n'en déclare pas — c'est le
    /// cas de la moitié des épingles de cette machine (mesuré : 8 sur 16), qui
    /// retombent alors sur l'icône de leur cible.
    ///
    /// **Le chemin, pas l'icône** : c'est l'appelant qui choisit comment
    /// l'afficher (voir `win32::taskbar_pins`). **L'indice d'icône du
    /// raccourci est ignoré** : les seize épingles mesurées désignent toutes
    /// l'indice 0, et `SHGetFileInfoW` rend justement l'icône principale d'un
    /// fichier — assez pour ce que le dock en fait.
    pub icon: Option<PathBuf>,
}

/// Résout un raccourci `.lnk` via `IShellLinkW`/`IPersistFile` — la même API
/// que l'Explorateur utilise pour "Ouvrir l'emplacement du fichier" et pour
/// afficher l'info-bulle d'un raccourci. `None` si la cible ne peut pas être
/// résolue ; une description ou des arguments manquants ou non résolvables,
/// eux, retombent simplement sur une chaîne vide plutôt que de faire échouer
/// tout le raccourci (bien moins essentiels que la cible).
pub(crate) fn resolve_shortcut(lnk_path: &Path) -> Option<Shortcut> {
    unsafe {
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist_file: IPersistFile = shell_link.cast().ok()?;

        let wide: Vec<u16> = {
            use std::os::windows::ffi::OsStrExt;
            lnk_path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
        };
        persist_file.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;

        // Une cible illisible n'abandonne PLUS la lecture du raccourci — voir
        // `Shortcut::target`. Seul l'échec de chargement du `.lnk`
        // (`persist_file.Load` ci-dessus) est une vraie raison de rendre `None`.
        // ⛔ Un raccourci MSI « annoncé » d'abord : son `GetPath` est un LEURRE.
        let target = cible_msi(lnk_path).or_else(|| chemin_du_lien(&shell_link));

        let mut desc_buffer = [0u16; 512];
        let description = shell_link
            .GetDescription(&mut desc_buffer)
            .ok()
            .map(|()| {
                let end = desc_buffer.iter().position(|&c| c == 0).unwrap_or(desc_buffer.len());
                String::from_utf16_lossy(&desc_buffer[..end])
            })
            .unwrap_or_default();

        let mut args_buffer = [0u16; 1024];
        let arguments = shell_link
            .GetArguments(&mut args_buffer)
            .ok()
            .map(|()| {
                let end = args_buffer.iter().position(|&c| c == 0).unwrap_or(args_buffer.len());
                String::from_utf16_lossy(&args_buffer[..end])
            })
            .unwrap_or_default();

        Some(Shortcut { target, description, arguments, icon: icon_location(&shell_link) })
    }
}

/// Le chemin que `shell_link` DÉCLARE (`GetPath`) — `None` s'il n'en a pas.
unsafe fn chemin_du_lien(shell_link: &IShellLinkW) -> Option<PathBuf> {
    let mut buffer = [0u16; 260];
    shell_link.GetPath(&mut buffer, std::ptr::null_mut(), 0).ok()?;
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let path = String::from_utf16_lossy(&buffer[..end]);
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// ⛔ **LA VRAIE CIBLE D'UN RACCOURCI MSI « ANNONCÉ »** (Darwin) — `None` pour un
/// raccourci ordinaire, ou si le composant n'est pas installé.
///
/// Défaut MESURÉ (2026-10-15) : les huit programmes de la suite « Craft »
/// (DesignCraft, PhotoCraft…) sont installés par MSI, et `IShellLinkW::GetPath`
/// rend pour eux l'ICÔNE que Windows Installer a déposée dans
/// `C:\Windows\Installer\{produit}\` (`DesigncraftIcon.ico`). Épinglé, un clic
/// proposait d'OUVRIR CE FICHIER .ico. Le raccourci ne désigne pas un fichier mais
/// un couple (produit, composant), que seul Windows Installer sait traduire.
///
/// `MsiGetComponentPathW` et non `MsiProvideComponentW` : ce dernier RÉPARE un
/// composant absent (une installation MSI déclenchée par un simple scan).
fn cible_msi(lnk_path: &Path) -> Option<PathBuf> {
    use windows::core::PWSTR;
    use windows::Win32::System::ApplicationInstallationAndServicing::{MsiGetComponentPathW, MsiGetShortcutTargetW, INSTALLSTATE_LOCAL, INSTALLSTATE_SOURCE};
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = lnk_path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // Un GUID entre accolades : 38 caractères et le zéro final.
    let mut produit = [0u16; 39];
    let mut composant = [0u16; 39];
    // SAFETY : `wide` est terminée par un zéro, et les deux tampons ont la
    // taille que la documentation impose (39 caractères).
    let erreur = unsafe {
        MsiGetShortcutTargetW(PCWSTR(wide.as_ptr()), Some(PWSTR(produit.as_mut_ptr())), None, Some(PWSTR(composant.as_mut_ptr())))
    };
    if erreur != 0 {
        return None;
    }
    let mut chemin = [0u16; 1024];
    let mut longueur = chemin.len() as u32;
    // SAFETY : les deux GUID sont terminés par un zéro (écrits par l'appel
    // ci-dessus), et `longueur` est la capacité de `chemin` en caractères.
    let etat = unsafe {
        MsiGetComponentPathW(PCWSTR(produit.as_ptr()), PCWSTR(composant.as_ptr()), Some(PWSTR(chemin.as_mut_ptr())), Some(&mut longueur))
    };
    if etat != INSTALLSTATE_LOCAL && etat != INSTALLSTATE_SOURCE {
        return None;
    }
    let end = chemin.iter().position(|&c| c == 0).unwrap_or(chemin.len());
    let texte = String::from_utf16_lossy(&chemin[..end]);
    // Un composant peut être une CLÉ DE REGISTRE (« 02:\… ») : pas un programme.
    let cible = PathBuf::from(texte);
    cible.is_file().then_some(cible)
}

/// Le chemin `p` est-il le LEURRE d'un raccourci MSI (voir `cible_msi`) — un
/// fichier rangé dans `…\Installer\{GUID}\` ? Pur, pour réparer ce qu'une
/// version d'avant a enregistré (voir `cible_reelle_d_un_leurre`).
pub(crate) fn est_un_leurre_msi(p: &Path) -> bool {
    let Some(dossier) = p.parent() else { return false };
    let guid = dossier.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let installer = dossier.parent().and_then(Path::file_name).is_some_and(|n| n.eq_ignore_ascii_case("Installer"));
    installer && guid.len() == 38 && guid.starts_with('{') && guid.ends_with('}')
}

/// Le programme qu'un LEURRE MSI (voir `est_un_leurre_msi`) cachait, pour une
/// épingle nommée `nom` : parmi les raccourcis MSI du menu Démarrer (voir
/// `choisir_le_raccourci_du_leurre`). Parcourt tout le menu Démarrer : à
/// n'appeler que pour un leurre, donc rarement.
pub(crate) fn cible_reelle_d_un_leurre(leurre: &Path, nom: &str) -> Option<PathBuf> {
    fn parcourir(dir: &Path, sortie: &mut Vec<RaccourciMsi>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                parcourir(&path, sortie);
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
                let Some(cible) = cible_msi(&path) else { continue };
                let nom = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                sortie.push(RaccourciMsi { nom, leurre: lien_declare(&path), cible });
            }
        }
    }
    let mut raccourcis = Vec::new();
    start_menu_dirs().iter().for_each(|d| parcourir(d, &mut raccourcis));
    choisir_le_raccourci_du_leurre(&raccourcis, leurre, nom)
}

/// Un raccourci MSI du menu Démarrer : son nom, le leurre qu'il déclare, sa
/// vraie cible.
pub(crate) struct RaccourciMsi {
    pub nom: String,
    pub leurre: Option<PathBuf>,
    pub cible: PathBuf,
}

/// ⛔ **LE LEURRE SEUL NE SUFFIT PAS** — MESURÉ (2026-10-15) : une MISE À JOUR du
/// programme change son code produit, donc le dossier `{GUID}` de son leurre
/// (CADCraft épinglé sous `{93680E40…}`, installé sous `{93916749…}`) ; et deux
/// raccourcis peuvent déclarer le MÊME leurre (« WSL » et « WSL Settings »,
/// `wsl.ico`). D'où, dans l'ordre : le raccourci du NOM de l'épingle, puis le seul
/// qui déclare ce leurre exact, puis le seul qui déclare ce nom de fichier.
pub(crate) fn choisir_le_raccourci_du_leurre(raccourcis: &[RaccourciMsi], leurre: &Path, nom: &str) -> Option<PathBuf> {
    let seul = |garde: &dyn Fn(&RaccourciMsi) -> bool| {
        let mut retenus = raccourcis.iter().filter(|r| garde(r));
        match (retenus.next(), retenus.next()) {
            (Some(r), None) => Some(r.cible.clone()),
            _ => None,
        }
    };
    let fichier = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_lowercase());
    seul(&|r| r.nom.eq_ignore_ascii_case(nom.trim()))
        .or_else(|| seul(&|r| r.leurre.as_deref().is_some_and(|l| l.to_string_lossy().eq_ignore_ascii_case(&leurre.to_string_lossy()))))
        .or_else(|| seul(&|r| r.leurre.as_deref().and_then(fichier).is_some_and(|f| Some(f) == fichier(leurre))))
}

/// Le chemin que le raccourci `lnk_path` DÉCLARE (voir `chemin_du_lien`).
fn lien_declare(lnk_path: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStrExt;
    // SAFETY : COM est initialisé sur ce fil par l'appelant (comme pour
    // `resolve_shortcut`) ; `wide` est terminée par un zéro.
    unsafe {
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist_file: IPersistFile = shell_link.cast().ok()?;
        let wide: Vec<u16> = lnk_path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        persist_file.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;
        chemin_du_lien(&shell_link)
    }
}

/// Chemin d'icône déclaré par `shell_link`, variables d'environnement
/// développées — `None` s'il n'en déclare pas.
///
/// **Il faut développer les variables** : MESURÉ, l'épingle « File Explorer »
/// de Windows déclare `%windir%\explorer.exe`, et `IShellLinkW` rend cette
/// chaîne TELLE QUELLE. Passée ainsi à `SHGetFileInfoW`, elle ne résout rien
/// (ce n'est pas un chemin) — l'icône retomberait silencieusement sur celle du
/// `.lnk`, flèche de raccourci comprise.
unsafe fn icon_location(shell_link: &IShellLinkW) -> Option<PathBuf> {
    let mut buffer = [0u16; 260];
    let mut index: i32 = 0;
    shell_link.GetIconLocation(&mut buffer, &mut index).ok()?;
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let raw = String::from_utf16_lossy(&buffer[..end]);
    if raw.trim().is_empty() {
        return None;
    }

    let wide: Vec<u16> = raw.encode_utf16().chain(std::iter::once(0)).collect();
    // Premier appel : la LONGUEUR nécessaire, zéro terminal compris.
    let needed = ExpandEnvironmentStringsW(PCWSTR(wide.as_ptr()), None);
    if needed == 0 {
        return None;
    }
    let mut expanded = vec![0u16; needed as usize];
    let written = ExpandEnvironmentStringsW(PCWSTR(wide.as_ptr()), Some(&mut expanded));
    if written == 0 || written > needed {
        return None;
    }
    let end = expanded.iter().position(|&c| c == 0).unwrap_or(expanded.len());
    Some(PathBuf::from(String::from_utf16_lossy(&expanded[..end])))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un choix manuel remplace les catégories automatiques ; une application sans
    /// choix garde les siennes.
    #[test]
    fn les_choix_manuels_remplacent_les_categories() {
        let entree = |nom: &str, categorie: &str| AppEntry {
            name: nom.to_string(),
            description: String::new(),
            categories: vec![categorie.to_string()],
            overridden: false,
            launch: AppLaunch::Shortcut { target: PathBuf::from("a.exe"), shortcut_path: PathBuf::from("a.exe") },
            icon_rgba: None,
            icon_path: None,
        };
        let mut apps = vec![entree("Disktree", "Autres"), entree("Gimp", "Graphisme")];
        let choix = std::collections::HashMap::from([("disktree".to_string(), vec!["Logiciels".to_string(), "Outils".to_string()])]);
        appliquer_les_choix_manuels(&mut apps, &choix);
        assert_eq!(apps[0].categories, ["Logiciels", "Outils"]);
        assert!(apps[0].overridden);
        assert_eq!(apps[1].categories, ["Graphisme"]);
        assert!(!apps[1].overridden);
    }

    /// Un raccourci dont le programme a disparu est masqué ; un programme
    /// présent, et tout chemin RÉSEAU (jamais sondé, voir `cible_presente`),
    /// sont gardés.
    #[test]
    fn un_programme_disparu_est_masque_mais_pas_un_chemin_reseau() {
        let present = std::env::current_exe().expect("l'exe du test existe");
        assert!(cible_presente(&present));
        assert!(!cible_presente(Path::new(r"C:\Program Files (x86)\NeuraDeskAbsent\absent.exe")));
        assert!(cible_presente(Path::new(r"\serveur-injoignable\partage\outil.exe")));
    }

    /// Le leurre d'un raccourci MSI se reconnaît à son dossier ; un programme
    /// ordinaire, ou un fichier posé à la racine d'`Installer`, non.
    #[test]
    fn un_leurre_msi_se_reconnait_a_son_dossier() {
        assert!(est_un_leurre_msi(Path::new(r"C:\WINDOWS\Installer\{203D73DA-EC83-4923-A50A-464CDF02BA1D}\DesigncraftIcon.ico")));
        assert!(est_un_leurre_msi(Path::new(r"C:\Windows\installer\{9C837B61-9648-427B-B0F1-BECB5ABC98AC}\PhotocraftIcon.exe")));
        assert!(!est_un_leurre_msi(Path::new(r"C:\WINDOWS\Installer\1a2b3c.msi")));
        assert!(!est_un_leurre_msi(Path::new(r"C:\Program Files\Designcraft\Designcraft.exe")));
        assert!(!est_un_leurre_msi(Path::new(r"C:\Outils\{203D73DA-EC83-4923-A50A-464CDF02BA1D}\a.exe")));
    }

    /// Le choix d'un raccourci pour un leurre : par le NOM d'abord (le GUID a pu
    /// changer), puis par le leurre exact, puis par son seul nom de fichier — et
    /// jamais au hasard entre deux.
    #[test]
    fn un_leurre_retrouve_son_raccourci() {
        let r = |nom: &str, leurre: &str, cible: &str| RaccourciMsi { nom: nom.into(), leurre: Some(PathBuf::from(leurre)), cible: PathBuf::from(cible) };
        let raccourcis = [
            r("CADCraft", r"C:\WINDOWS\Installer\{93916749-D6C0-4188-9B2D-5B0E5D384E0C}\CadcraftIcon.ico", r"C:\P\cadcraft.exe"),
            r("WSL", r"C:\WINDOWS\Installer\{B637A6A6-5591-4503-AFD8-776164EB837A}\wsl.ico", r"C:\P\wsl.exe"),
            r("WSL Settings", r"C:\WINDOWS\Installer\{B637A6A6-5591-4503-AFD8-776164EB837A}\wsl.ico", r"C:\P\wslsettings.exe"),
        ];
        let choisir = |leurre: &str, nom: &str| choisir_le_raccourci_du_leurre(&raccourcis, Path::new(leurre), nom);
        // Un GUID PÉRIMÉ (mise à jour) : le nom suffit.
        assert_eq!(choisir(r"C:\WINDOWS\Installer\{93680E40-3B43-4CFF-9605-83428B7D0BC0}\CadcraftIcon.ico", "cadcraft"), Some(PathBuf::from(r"C:\P\cadcraft.exe")));
        // Renommée : le nom de fichier, s'il est seul à le déclarer.
        assert_eq!(choisir(r"C:\WINDOWS\Installer\{00000000-0000-0000-0000-000000000000}\CadcraftIcon.ico", "Mon CAD"), Some(PathBuf::from(r"C:\P\cadcraft.exe")));
        // Un leurre PARTAGÉ : le nom tranche…
        assert_eq!(choisir(r"C:\WINDOWS\Installer\{B637A6A6-5591-4503-AFD8-776164EB837A}\wsl.ico", "WSL Settings"), Some(PathBuf::from(r"C:\P\wslsettings.exe")));
        // … et sans lui, aucun des deux n'est choisi au hasard.
        assert_eq!(choisir(r"C:\WINDOWS\Installer\{B637A6A6-5591-4503-AFD8-776164EB837A}\wsl.ico", "Linux"), None);
    }

    /// SONDE (machine réelle) : les raccourcis MSI du menu Démarrer, leur leurre
    /// et leur vraie cible.
    #[test]
    #[ignore]
    fn sonde_raccourcis_msi() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        fn parcourir(dir: &Path) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    parcourir(&path);
                } else if let Some(cible) = cible_msi(&path) {
                    let leurre = lien_declare(&path);
                    println!("{} | {:?} -> {}", path.display(), leurre, cible.display());
                    let nom = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    if let Some(l) = leurre.filter(|l| est_un_leurre_msi(l)) {
                        assert_eq!(cible_reelle_d_un_leurre(&l, &nom).as_deref(), Some(cible.as_path()));
                    }
                }
            }
        }
        start_menu_dirs().iter().for_each(|d| parcourir(d));
        // Les épingles RÉELLES qui visent un leurre, et ce qu'elles deviendraient
        // (lecture seule : rien n'est réécrit).
        let pins = std::env::var("APPDATA").map(|a| PathBuf::from(a).join(r"NeuraDesk\dock_pins.txt"));
        for ligne in pins.ok().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default().lines() {
            let mut champs = ligne.split('\t');
            let (Some(nom), Some(chemin)) = (champs.next(), champs.next()) else { continue };
            if est_un_leurre_msi(Path::new(chemin)) {
                println!("épingle {nom} : {chemin} -> {:?}", cible_reelle_d_un_leurre(Path::new(chemin), nom));
            }
        }
    }
}

#[cfg(test)]
mod frequents_tests {
    use super::*;

    fn comptes(paires: &[(&str, u64)]) -> std::collections::HashMap<String, u64> {
        paires.iter().map(|(n, c)| (n.to_string(), *c)).collect()
    }

    /// ⭐ **LE DÉFAUT MESURÉ, EN UN TEST** : le classement suit le fichier
    /// d'usage, donc **il CHANGE quand l'usage change**. C'est exactement ce que
    /// le cache figé interdisait.
    ///
    /// ⚠️ **PLUS D'APPLICATIONS QUE LA LIMITE, SINON CE TEST NE PROUVE RIEN** :
    /// avec deux entrées de plus que de places, une application doit vraiment
    /// GAGNER sa place — sans quoi tout le monde tiendrait dans la liste et le
    /// classement n'entrerait jamais en jeu. Le nombre SUIT la limite : il valait
    /// dix pour huit places, et aurait cessé de prouver quoi que ce soit à 25.
    #[test]
    fn le_classement_suit_l_usage_et_change_avec_lui() {
        let n = FREQUENT_APPS_LIMIT + 2;
        let noms: Vec<String> = (0..n).map(|i| format!("App{i:02}")).collect();
        let cle = |i: usize| format!("app{i:02}");
        let mut c: std::collections::HashMap<String, u64> = (0..n).map(|i| (cle(i), 1)).collect();
        c.insert(cle(n - 1), 5);

        let avant = cles_frequentes(noms.iter().map(String::as_str), &c);
        assert!(avant.contains(&cle(n - 1)), "la plus lancée est là");
        assert!(!avant.contains(&cle(n - 3)), "l'antépénultième, à égalité au fond, n'y est pas encore");

        // L'utilisateur la lance quatre fois : elle DOIT entrer, et en faire sortir une.
        c.insert(cle(n - 3), 4);
        let apres = cles_frequentes(noms.iter().map(String::as_str), &c);
        assert!(apres.contains(&cle(n - 3)), "elle est entrée — c'est ÇA, « se mettre à jour »");
        assert!(!apres.contains(&cle(n - 4)), "et sa voisine en est sortie : la limite n'a pas bougé");
    }

    /// ⚠️ **UNE APPLICATION JAMAIS LANCÉE N'EST PAS « FRÉQUENTE »** — elle
    /// n'existe même pas dans le fichier d'usage. Comportement d'origine conservé.
    #[test]
    fn une_application_jamais_lancee_n_est_pas_frequente() {
        let noms = ["Alpha", "JamaisLancee"];
        let c = comptes(&[("alpha", 1)]);
        let cles = cles_frequentes(noms.iter().copied(), &c);
        assert!(cles.contains("alpha"));
        assert!(!cles.contains("jamaislancee"));
    }

    /// La comparaison est INSENSIBLE À LA CASSE, dans les deux sens : c'est la
    /// forme sous laquelle `record_launch` écrit, et `trim` d'un seul côté
    /// ferait diverger la clé.
    #[test]
    fn la_cle_est_en_minuscules() {
        let cles = cles_frequentes(["Bloc-Notes"].into_iter(), &comptes(&[("bloc-notes", 2)]));
        assert!(cles.contains("bloc-notes"));
    }

    /// La limite est respectée : au-delà de 25, les moins lancées sortent.
    #[test]
    fn la_limite_est_de_vingt_cinq() {
        assert_eq!(FREQUENT_APPS_LIMIT, 25, "DEMANDE EXPLICITE (2026-10-14)");
        let noms: Vec<String> = (0..30).map(|i| format!("App{i:02}")).collect();
        let c: std::collections::HashMap<String, u64> =
            (0..30).map(|i| (format!("app{i:02}"), (30 - i) as u64)).collect();
        let cles = cles_frequentes(noms.iter().map(String::as_str), &c);
        assert_eq!(cles.len(), FREQUENT_APPS_LIMIT);
        assert!(cles.contains("app00"), "la plus lancée");
        assert!(cles.contains("app24"), "la 25ᵉ y est");
        assert!(!cles.contains("app25"), "la 26ᵉ, au-delà de la limite");
    }

    /// ⛔ **LE RETRAIT COMPTE AUTANT QUE L'AJOUT** : la marque est cuite dans le
    /// cache, donc une application qui a CESSÉ d'être fréquente la porte encore.
    /// Sans le filtre, elle resterait affichée indéfiniment.
    #[test]
    fn une_application_qui_n_est_plus_frequente_perd_la_marque() {
        let anciennes = vec!["Jeux".to_string(), app_categories::FREQUENT.to_string()];
        assert_eq!(categories_avec_frequents(&anciennes, false), vec!["Jeux".to_string()]);
    }

    /// ⚠️ **AJOUTÉE EN FIN**, jamais à la place des autres : une application reste
    /// fréquente indépendamment de son classement par ailleurs.
    #[test]
    fn la_marque_s_ajoute_en_fin_et_ne_se_duplique_pas() {
        let deja = vec!["Jeux".to_string(), app_categories::FREQUENT.to_string()];
        assert_eq!(
            categories_avec_frequents(&deja, true),
            vec!["Jeux".to_string(), app_categories::FREQUENT.to_string()],
            "pas de doublon quand elle y est déjà"
        );
        let sans = vec!["Jeux".to_string()];
        assert_eq!(
            categories_avec_frequents(&sans, true),
            vec!["Jeux".to_string(), app_categories::FREQUENT.to_string()]
        );
    }

    /// ⚠️ **LE REPLI** : si la marque était la SEULE catégorie, la retirer
    /// laisserait une liste vide — et la page ferait disparaître l'application
    /// de tous les groupes.
    #[test]
    fn une_application_sans_autre_categorie_ne_se_retrouve_pas_vide() {
        let seule = vec![app_categories::FREQUENT.to_string()];
        assert!(!categories_avec_frequents(&seule, false).is_empty());
    }
}
