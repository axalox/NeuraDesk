//! Classement des applications en catégories généralistes par thème
//! d'usage (Internet, Jeux, Développement, Audio, Vidéo...) — voir
//! `win32::apps::AppEntry::category`, exploité par `win32::dash` pour le
//! regroupement affiché dans le Dash, façon Dash Unity.
//!
//! Windows ne fournit aucune métadonnée de catégorie pour un raccourci
//! installé (contrairement aux entrées `.desktop` de Linux, qui portent un
//! champ `Categories=` standardisé — c'est littéralement ce que le Dash
//! Unity dont ce module s'inspire lisait). Le dossier du menu Démarrer
//! contenant le raccourci (voir `apps::walk_lnk_files`) n'est PAS un
//! substitut utilisable tel quel : la plupart des installateurs créent un
//! dossier PAR APPLICATION (souvent juste pour loger le désinstalleur/
//! l'aide), pas un dossier par thème — utiliser ce dossier directement
//! comme "catégorie" produit une longue liste de catégories à une seule
//! application (vérifié en pratique : "3D Effect", "7-Zip", "Advanced
//! Masks", "Advanced SystemCare"... une par éditeur), pas des thèmes
//! généraux comme "Internet" ou "Jeux".
//!
//! On classe donc chaque application soi-même, par mots-clés reconnus dans
//! son nom — sauf pour les dossiers du menu Démarrer propres à Windows lui-
//! même (`FOLDER_OVERRIDES`), assez bien nommés pour être utilisés tels
//! quels. Best-effort assumé : une bonne partie des logiciels de niche
//! n'aura aucun mot-clé reconnu et atterrira dans `OTHER`, comme n'importe
//! quel classement heuristique sans métadonnée fiable pour s'appuyer
//! dessus — préférable malgré tout à une "catégorie" par éditeur.
//!
//! Ensemble de catégories choisi à partir de deux taxonomies existantes
//! plutôt qu'inventé au hasard :
//! - le [registre de catégories du Desktop Menu Specification de
//!   freedesktop.org](https://specifications.freedesktop.org/menu/latest/category-registry.html)
//!   (catégories principales `AudioVideo`/`Audio`/`Video`/`Development`/
//!   `Education`/`Game`/`Graphics`/`Network`/`Office`/`Science`/`Settings`/
//!   `System`/`Utility`, plus des [catégories additionnelles](https://specifications.freedesktop.org/menu/latest/additional-category-registry.html)
//!   comme `Security`, `Chat`, `Email`, `WebBrowser`) — la source exacte
//!   qu'Unity lisait dans les fichiers `.desktop`, donc la référence la
//!   plus directement pertinente pour "faire comme Unity" ;
//! - les [catégories du Microsoft Store pour les apps Windows](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/categories-and-subcategories)
//!   (`Security`, `Developer tools`, `Multimedia design`, `Business`...),
//!   plus pertinentes pour des logiciels Windows traditionnels que les
//!   catégories orientées applications mobiles du Store (`Kids + family`,
//!   `Navigation + maps`...), écartées ici.
//!
//! Par rapport à une première version de ce module, la scission
//! Audio/Vidéo (au lieu d'un seul "Multimédia") suit freedesktop ; les
//! catégories "Communication" et "Sécurité" (auparavant diluées dans
//! "Internet" et "Système") reprennent chacune une thématique que les deux
//! sources traitent à part (`Chat`/`Email` d'un côté, `Security` de
//! l'autre) et qui s'est avérée bien peuplée en pratique (Discord/Slack/
//! Teams/Zoom d'un côté, antivirus/VPN/sauvegarde de l'autre).
//!
//! "Sysinternals" n'a d'équivalent dans AUCUNE des deux taxonomies
//! ci-dessus (ni `Development`/`System`/`Security` de freedesktop, ni
//! `Developer tools`/`Security` du Store ne couvrent spécifiquement cette
//! suite) : ajoutée à part pour les mêmes raisons que Communication/
//! Sécurité — une thématique assez reconnaissable et assez fournie (une
//! bonne douzaine d'outils, voir `KEYWORD_CATEGORIES`) pour mériter sa
//! propre catégorie plutôt que de se retrouver diluée entre Sécurité et
//! Système selon l'outil.
//!
//! "PowerToys" est sortie de "Système" (où son seul mot-clé vivait avant)
//! pour une raison différente des deux précédentes : PAS parce qu'elle est
//! bien peuplée (un seul raccourci, en pratique), mais pour pouvoir
//! proposer explicitement son installation quand elle est VIDE (voir
//! `INSTALL_PROMPTS`) — un traitement qui n'aurait aucun sens appliqué à
//! "Système" dans son ensemble.
//!
//! "Désinstallation" (voir `UNINSTALL`) n'a, elle non plus, d'équivalent
//! dans aucune des deux taxonomies — logique, ce n'est pas un thème
//! applicatif mais une reconnaissance de FONCTION (n'importe quel
//! désinstalleur, de n'importe quelle application, quel que soit son
//! thème). Ajoutée pour éviter deux écueils du classement par mot-clé sur
//! le nom : un désinstalleur qui pollue le thème de l'application qu'il
//! concerne ("Désinstaller VLC" à côté de VLC dans "Vidéo", sans intérêt),
//! ou pire, se classe carrément dans le MAUVAIS thème si son nom contient
//! par coïncidence le mot-clé d'une autre catégorie ("Désinstaller
//! Spotify" matcherait "spotify", le mot-clé Audio).

/// Catégorie de repli si rien ne correspond — voir doc de module.
pub const OTHER: &str = "Autres";

/// Catégorie dédiée aux raccourcis de DÉSINSTALLATION — la plupart des
/// installateurs Windows en créent un à côté du raccourci de l'application
/// elle-même (voir doc de module, "souvent juste pour loger le
/// désinstalleur/l'aide"), qui atterrirait sinon dans le thème de
/// l'application concernée (voir `KEYWORD_CATEGORIES`) plutôt que dans un
/// thème à lui — au mieux inutile ("Désinstaller VLC" classé "Vidéo" à côté
/// de VLC lui-même n'apporte rien), au pire trompeur ("Désinstaller
/// Spotify" matcherait le mot-clé "spotify" et atterrirait dans "Audio").
/// Reconnue par mot-clé dans le nom du raccourci LUI-MÊME (voir
/// `UNINSTALL_KEYWORDS`, vérifiée en priorité absolue dans `classify`, avant
/// même `FOLDER_OVERRIDES`) plutôt que par dossier : les désinstalleurs
/// vivent dans le MÊME dossier que l'application (pas de dossier "Uninstall"
/// à part à reconnaître).
pub const UNINSTALL: &str = "Désinstallation";

/// Mots-clés reconnus dans le nom du raccourci pour la catégorie
/// `UNINSTALL` — anglais ("Uninstall Foo", quasi universel quelle que soit
/// la langue de l'installateur d'origine) et français ("Désinstaller
/// Foo"/"Desinstaller Foo" sans l'accent, au cas où l'installateur normalise
/// ses noms de fichiers en ASCII).
const UNINSTALL_KEYWORDS: &[&str] = &["uninstall", "désinstaller", "desinstaller"];

/// Nom de la catégorie "PowerToys" — voir `KEYWORD_CATEGORIES`,
/// `INSTALL_PROMPTS`. Exportée comme constante (plutôt que répétée en dur à
/// chaque endroit qui en a besoin, voir `win32::dash::find_powertoys_exe`)
/// pour la même raison que `UNINSTALL`/`OTHER` : un seul endroit à changer
/// si jamais ce nom devait évoluer.
pub const POWERTOYS: &str = "PowerToys";

/// Nom de la catégorie "Jeux" — exportée comme constante pour la même
/// raison que `POWERTOYS`/`UNINSTALL`/`OTHER` : `win32::apps::enumerate_apps`
/// l'utilise directement pour classer les jeux trouvés via `win32::
/// steam_apps`/`win32::gog_apps`/`win32::epic_apps` (bibliothèques de jeux
/// lues directement, voir leur doc), sans passer par le classement par
/// mot-clé de `classify` — on SAIT déjà avec certitude que ce sont des
/// jeux, la source elle-même le garantit, inutile et moins fiable de
/// deviner à partir du nom.
pub const GAMES: &str = "Jeux";

/// Nom de la catégorie "Fréquemment utilisés" — calculée par
/// `win32::apps::enumerate_apps` à partir du nombre RÉEL de lancements
/// enregistrés depuis le Dash (voir `crate::config::load_launch_counts`/
/// `record_launch`), PAS une catégorie de thème comme les autres.
/// Volontairement ABSENTE de `fixed_categories`/`all_categories_info` (voir
/// `win32::apps::all_categories_info`) : contrairement à `POWERTOYS`/
/// `GAMES`/`UNINSTALL`, elle n'a aucun sens assignée à la main (le
/// sélecteur de catégorie du Dash ne doit donc jamais la proposer) — elle
/// n'existe QUE comme catégorie supplémentaire ajoutée automatiquement aux
/// applications les plus lancées, jamais comme un choix humain.
pub const FREQUENT: &str = "Fréquemment utilisés";

/// Dossiers du menu Démarrer propres à Windows lui-même, vérifiés en
/// premier (insensible à la casse, sous-chaîne) sur le dossier d'origine
/// du raccourci (voir `classify`, paramètre `folder_hint`) — plus fiables
/// à cet endroit que les mots-clés du nom de l'application, puisque ce
/// sont de vrais regroupements thématiques et non un dossier par éditeur.
const FOLDER_OVERRIDES: &[(&str, &str)] = &[
    ("accessibility", "Accessibilité"),
    ("ease of access", "Accessibilité"),
    ("windows accessories", "Accessoires"),
    ("administrative tools", "Système"),
    ("windows powershell", "Développement"),
    ("windows system", "Système"),
    ("startup", "Système"),
    ("maintenance", "Système"),
    // Dossiers de suites/lanceurs multi-outils : un seul dossier couvre
    // d'un coup tous les sous-outils qu'il contient (voir `apps::
    // AppEntry::shortcut_path`), bien plus rentable que de lister chaque
    // sous-outil un par un (ex. "Protect"/"Software Updater"/"Speed Up"/
    // "Toolbox" pour Advanced SystemCare, "Tray Icon"/"Rootkit Scan" pour
    // Bitdefender) — vérifié en pratique sur un vrai menu Démarrer.
    ("games", "Jeux"),
    ("advanced systemcare", "Système"),
    ("bitdefender", "Sécurité"),
    // La Suite Sysinternals n'a pas d'installateur officiel qui crée des
    // raccourcis (ce sont de simples exécutables portables) — quand des
    // raccourcis existent quand même (créés à la main, ou via un
    // gestionnaire de paquets comme winget/Chocolatey), ils atterrissent
    // presque toujours dans UN SEUL dossier "Sysinternals"/"Sysinternals
    // Suite", jamais un dossier par outil — même raisonnement que les deux
    // entrées ci-dessus.
    ("sysinternals", "Sysinternals"),
];

/// Mots-clés reconnus dans le NOM de l'application (comparaison sur une
/// version en minuscules), par catégorie — les catégories sont vérifiées
/// dans l'ordre ci-dessous, la première dont un mot-clé correspond
/// l'emporte. L'ordre est délibéré : les catégories les plus "spécifiques"
/// (susceptibles de contenir un mot-clé qui apparaîtrait aussi, par
/// coïncidence, dans une liste plus générale) passent en premier — ex.
/// "acrobat" (Bureautique) avant les mots-clés génériques de Graphisme,
/// pour qu'Adobe Acrobat n'atterrisse pas dans la mauvaise catégorie ;
/// "outlook"/"vpn" ne sont listés qu'une fois chacun (Communication,
/// Sécurité) pour ne jamais dépendre de l'ordre entre ces deux catégories.
const KEYWORD_CATEGORIES: &[(&str, &[&str])] = &[
    (
        "Accessibilité",
        &[
            "narrator", "magnify", "loupe", "captions", "voiceaccess", "voice access", "on-screen keyboard",
            "clavier visuel",
        ],
    ),
    (
        "Bureautique",
        &[
            "word", "excel", "powerpoint", "onenote", "office", "libreoffice", "openoffice", "acrobat",
            "foxit", "pdf", "notion", "evernote", "calibre", "obsidian", "deepl", "e-book", "ebook reader",
            "naps2", "document scanner",
        ],
    ),
    (
        "Communication",
        &[
            "outlook", "thunderbird", "discord", "slack", "teams", "zoom", "skype", "whatsapp", "telegram",
            "signal", "messenger", "viber", "teamspeak", "snapchat",
        ],
    ),
    (
        "Développement",
        &[
            "visual studio", "vscode", "code", "git", "github", "gitlab", "python", "node.js", " node ",
            "npm", "docker", "kubernetes", "jdk", "jre", " java", "rust", "cargo", "powershell", "terminal",
            "command prompt", "invite de commandes", "putty", "pageant", "psftp", "ssh", "winscp", "mysql",
            "postgres", "sql server", "odbc", "android studio", "xcode", "intellij", "pycharm",
            "sublime text", "notepad++", "cmake", "arduino", "raspberry pi", "unity hub", "unreal engine",
            "godot", "ollama", "wsl", "windows subsystem", "nsight", "openrouter", "unsloth",
            "packet sender", "sniffnet", "pe viewer", "application verifier", "software development kit",
            "native tools command prompt", "lemonade",
        ],
    ),
    (
        "Éducation",
        &["duolingo", "anki", "rosetta stone", "khan academy", "mathematica", "wolfram", "geogebra"],
    ),
    (
        GAMES,
        &[
            "steam", "epic games", "battle.net", "battlenet", "gog.com", "gog galaxy", "ubisoft connect",
            "ea app", "ea desktop", "origin", "riot", "league of legends", "minecraft", "roblox",
            "playstation", "geforce now", "moonlight", "overwolf", "curseforge", "modrinth", "tlauncher",
            "prism launcher", "luanti", "rsi launcher", "roberts space", "baldur's gate", "control ultimate",
            "fallout 4", "dune 2000", "tiberian dawn", "red alert", "vintage story", "avowed", "hytale",
            "true fear",
        ],
    ),
    (
        "Graphisme",
        &[
            "photoshop", "illustrator", "lightroom", "gimp", "krita", "inkscape", "inkview", "blender",
            "sketchup", "autocad", "solidworks", "corel", "affinity", "paint.net", "clip studio", "figma",
            "shadertastic", "blockbench", "upscayl", "xnview", "amuse",
        ],
    ),
    (
        "Audio",
        &[
            "spotify", "itunes", "audacity", "winamp", "foobar", "musicbee", "cantabile", "rhythmbox",
            "voicemeeter", "voicemod", "vb-cable", "vb-audio", "vban", "fairlight", "musehub", "graphic eq",
            "voicewave",
        ],
    ),
    (
        "Vidéo",
        &[
            "vlc", "media player", "obs", "handbrake", "kdenlive", "davinci", "premiere", "camtasia", "kodi",
            "plex", "potplayer", "streamlabs", "nvidia broadcast", "droidcam", "webcam", "flip clock",
            "blackmagic", "shutter encoder", "openshot", "shotcut", "medal", "snap camera", "videolan",
            "outplayed",
            // Écosystème de plugins OBS (constaté en pratique : ce sont
            // presque toujours des extensions autour d'OBS Studio, pas des
            // logiciels autonomes ayant chacun leur propre thème).
            "advanced-scene-switcher", "downstream-keyer", "draw-dock", "move-transition", "move transition",
            "obs-hide-desktop-icons", "obs shaderfilter", "osi-branch-output", "scene collection manager",
            "scene as transition", "source-clone", "source clone", "source-record", "source record",
            "source-switcher", "source switcher", "streamup", "stroke glow shadow", "retro effects",
            "pixel-art", "3d effect", "advanced masks",
        ],
    ),
    (
        "Internet",
        &[
            "chrome", "firefox", "microsoft edge", " edge", "opera", "brave", "vivaldi", "qbittorrent",
            "utorrent", "bittorrent", "filezilla", "megasync", "dropbox", "onedrive", "google drive",
            "browser", "nextcloud", "nperf",
        ],
    ),
    (
        "Accessoires",
        &[
            "calculator", "calculatrice", "qalculate", "qalc", "character map", "clipboard", "screenshot",
            "capture d'écran", "flameshot", "sticky notes", "snipping", "7-zip", "7zip", "winrar",
            "archive manager", "disk cleanup", "cleaner", "ccleaner", "everything", "sharex",
            "steps recorder",
        ],
    ),
    (
        // Avant "Sécurité" : sinon "RootkitRevealer" (voir mots-clés
        // ci-dessous) atterrirait dans "Sécurité" à cause de son mot-clé
        // "rootkit" — cette catégorie doit rester la première à
        // reconnaître un nom d'outil Sysinternals, pour classer TOUS ses
        // outils au même endroit plutôt que de les éparpiller selon des
        // collisions de mots-clés fortuites avec des catégories générales.
        "Sysinternals",
        &[
            "process explorer", "procexp", "process monitor", "procmon", "autoruns", "autorunsc",
            "tcpview", "sysmon", "psexec", "pskill", "pslist", "psinfo", "psfile", "psloggedon",
            "psservice", "psshutdown", "pssuspend", "psgetsid", "psping", "listdlls", "shareenum",
            "accesschk", "accessenum", "logonsessions", "rammap", "vmmap", "zoomit", "bginfo",
            "diskmon", "portmon", "regmon", "winobj", "procdump", "sigcheck", "pendmoves",
            "rootkitrevealer", "notmyfault", "disk2vhd", "coreinfo", "clockres", "ldmdump",
            "efsdump", "junction", "ntfsinfo", "sdelete", "volumeid", "pipelist", "contig",
        ],
    ),
    (
        "Sécurité",
        &[
            "antivirus", "defender", "bitdefender", "norton", "kaspersky", "malwarebytes", "vpn",
            "sandboxie", "iobit unlocker", "backup", "sauvegarde", "recovery", "malware", "spybot",
            "rootkit", "privacy.sexy", "simplewall", "firewall", "pare-feu",
        ],
    ),
    (
        // Catégorie dédiée plutôt que noyée dans "Système" (voir
        // `INSTALL_PROMPTS`, `install_url`) : PowerToys n'est PAS installé
        // par défaut sur Windows, contrairement à tout ce qui peuple
        // habituellement "Système" (Gestionnaire de périphériques, Panneau
        // de configuration...) — la séparer permet de proposer son
        // installation quand elle est vide, ce qui n'aurait aucun sens pour
        // "Système" dans son ensemble.
        POWERTOYS,
        &["powertoys"],
    ),
    (
        "Système",
        &[
            "driver", "pilote", "nvidia", "amd", "intel", "realtek", "gigabyte", "bios", "control panel",
            "device manager", "gestionnaire de", "registry", "regedit", "partition", "disk management",
            "task manager", "task scheduler", "hyper-v", "vmware", "virtualbox",
            "windows update", "hwinfo", "hwmonitor", "cpuid", "cpu-z", "cpu set", "gpu-z", "afterburner",
            "fan control", "latencymon", "iobit", "advanced systemcare", "driver booster", "brother",
            "bradmin", "dfrgui", "event viewer", "file explorer", "iscsi", "memory diagnostics",
            "performance monitor", "print management", "remote desktop", "resource monitor", "ryzen",
            "security configuration", "seelen", "system configuration", "system information",
            "system informer", "system scan", "touch portal", "treesize", "winaero", "winhance", "sandbox",
            "bac à sable", "smart switch", "vmcreate", "gaming keyboard", "gaming mouse",
        ],
    ),
];

/// Mots-clés reconnus dans le nom de l'ÉDITEUR ("CompanyName" des
/// métadonnées de version de l'exécutable, voir `exe_metadata::company_name`
/// et `classify_by_publisher`) — un signal de repli, utilisé seulement
/// quand `classify` n'a rien trouvé par nom d'application/dossier (voir
/// `apps::walk_lnk_files`). Beaucoup plus rentable qu'étoffer encore
/// `KEYWORD_CATEGORIES` app par app : un seul éditeur connu ("Ubisoft",
/// "Adobe"...) couvre souvent plusieurs dizaines de logiciels d'un coup.
///
/// "Microsoft" est délibérément ABSENT de cette liste : l'éditeur est bien
/// trop transversal (Windows lui-même, Office, Xbox, Visual Studio, des
/// pilotes...) pour qu'une seule catégorie soit correcte assez souvent —
/// mieux vaut laisser ces logiciels-là dans `OTHER` que les classer au
/// hasard.
const PUBLISHER_CATEGORIES: &[(&str, &str)] = &[
    // Jeux — éditeurs/studios.
    ("valve", "Jeux"),
    ("ubisoft", "Jeux"),
    ("electronic arts", "Jeux"),
    ("activision", "Jeux"),
    ("blizzard", "Jeux"),
    ("riot games", "Jeux"),
    ("epic games", "Jeux"),
    ("cd projekt", "Jeux"),
    ("bethesda", "Jeux"),
    ("square enix", "Jeux"),
    ("bandai namco", "Jeux"),
    ("sega", "Jeux"),
    ("take-two", "Jeux"),
    ("2k games", "Jeux"),
    ("paradox interactive", "Jeux"),
    ("focus entertainment", "Jeux"),
    ("devolver", "Jeux"),
    ("wargaming", "Jeux"),
    // Graphisme — édition d'images/3D/CAO.
    ("adobe", "Graphisme"),
    ("corel", "Graphisme"),
    ("autodesk", "Graphisme"),
    ("trimble", "Graphisme"),
    ("maxon", "Graphisme"),
    ("chaos", "Graphisme"),
    // Développement.
    ("jetbrains", "Développement"),
    ("docker inc", "Développement"),
    ("gitkraken", "Développement"),
    ("postman", "Développement"),
    ("oracle corporation", "Développement"),
    // Sécurité — antivirus/VPN.
    ("avast", "Sécurité"),
    ("avg technologies", "Sécurité"),
    ("eset", "Sécurité"),
    ("mcafee", "Sécurité"),
    ("trend micro", "Sécurité"),
    ("sophos", "Sécurité"),
    ("nordvpn", "Sécurité"),
    ("expressvpn", "Sécurité"),
    ("surfshark", "Sécurité"),
    // Communication.
    ("discord inc", "Communication"),
    ("slack technologies", "Communication"),
    ("zoom video", "Communication"),
    // Sysinternals — repli si un outil de la suite n'a été reconnu ni par
    // son nom (voir `KEYWORD_CATEGORIES`) ni par le dossier du menu
    // Démarrer qui le contient (voir `FOLDER_OVERRIDES`) : ses métadonnées
    // de version portent presque toujours "Sysinternals" comme éditeur.
    ("sysinternals", "Sysinternals"),
    // Système — matériel/pilotes/constructeurs.
    ("intel corporation", "Système"),
    ("advanced micro devices", "Système"),
    ("nvidia corporation", "Système"),
    ("realtek", "Système"),
    ("logitech", "Système"),
    ("razer", "Système"),
    ("corsair", "Système"),
    ("asustek", "Système"),
    ("msi", "Système"),
    ("gigabyte", "Système"),
    ("western digital", "Système"),
    ("seagate", "Système"),
    ("teamviewer", "Système"),
    ("samsung", "Système"),
];

/// Noms de toutes les catégories fixes (une par entrée de
/// `KEYWORD_CATEGORIES`, dans le même ordre, plus `OTHER` en dernier) — sert
/// à `win32::dash` à proposer la liste complète des catégories dans le
/// sélecteur manuel (voir `dash_template.html`, "choisir une catégorie"),
/// y compris celles qui n'ont actuellement aucune application, contrairement
/// à la barre latérale qui ne montre que les catégories effectivement
/// peuplées (voir `groupByCategory` côté JS).
pub fn fixed_categories() -> Vec<&'static str> {
    std::iter::once(UNINSTALL)
        .chain(KEYWORD_CATEGORIES.iter().map(|(name, _)| *name))
        .chain(std::iter::once(OTHER))
        .collect()
}

/// Catégories fixes qui, si elles se retrouvent VIDES une fois le Dash
/// affiché (aucune application classée dedans, voir `win32::apps::
/// CategoryInfo::install_url`), proposent explicitement d'installer l'outil
/// correspondant plutôt que de simplement disparaître de la barre latérale
/// comme n'importe quelle autre catégorie non peuplée — PowerToys est utile
/// mais absent d'une installation Windows par défaut, facile à ne jamais
/// découvrir sans ce genre de coup de pouce. `url` est ouvert via
/// `win32::appicon::open` (le gestionnaire par défaut de Windows, donc le
/// navigateur pour une URL http(s)) suite à la commande
/// `"installcat:<catégorie>"` (voir `win32::dash::install_category`) —
/// jamais une URL renvoyée par la page elle-même : cette table reste la
/// seule source de vérité sur ce qui est réellement exécuté.
pub const INSTALL_PROMPTS: &[(&str, &str)] =
    &[(POWERTOYS, "https://github.com/microsoft/PowerToys/releases/latest")];

/// URL d'installation pour `category`, si elle en propose une (comparaison
/// insensible à la casse) — voir `INSTALL_PROMPTS`.
pub fn install_url(category: &str) -> Option<&'static str> {
    INSTALL_PROMPTS.iter().find(|(name, _)| name.eq_ignore_ascii_case(category)).map(|(_, url)| *url)
}

/// Classe une application par thème d'usage généraliste (façon Dash Unity :
/// Internet, Jeux, Développement, Audio, Vidéo...) à partir de son nom et,
/// pour les dossiers propres à Windows, du dossier du menu Démarrer qui la
/// contient — voir doc de module. `OTHER` si rien ne correspond (voir
/// `classify_by_publisher` pour le repli suivant, basé sur l'éditeur).
pub fn classify(app_name: &str, folder_hint: &str) -> &'static str {
    let name_lower = app_name.to_lowercase();

    // Vérifié EN PREMIER, avant même `FOLDER_OVERRIDES` : un désinstalleur
    // doit atterrir dans `UNINSTALL` quel que soit le dossier qui le
    // contient ou le thème de l'application concernée — voir doc de
    // `UNINSTALL`.
    if UNINSTALL_KEYWORDS.iter().any(|keyword| name_lower.contains(keyword)) {
        return UNINSTALL;
    }

    let folder_lower = folder_hint.to_lowercase();
    for (needle, category) in FOLDER_OVERRIDES {
        if folder_lower.contains(needle) {
            return category;
        }
    }

    for (category, keywords) in KEYWORD_CATEGORIES {
        if keywords.iter().any(|keyword| name_lower.contains(keyword)) {
            return category;
        }
    }

    OTHER
}

/// Classe par éditeur (voir `PUBLISHER_CATEGORIES`, `exe_metadata::
/// company_name`) — un second niveau de repli, à n'appeler que si
/// `classify` a renvoyé `OTHER` (voir `apps::walk_lnk_files`, qui ne lit
/// les métadonnées de l'exécutable, plus coûteux qu'une comparaison de
/// chaînes, que dans ce cas précis). `OTHER` si l'éditeur est absent ou
/// non reconnu.
pub fn classify_by_publisher(company_name: &str) -> &'static str {
    let lower = company_name.to_lowercase();
    for (needle, category) in PUBLISHER_CATEGORIES {
        if lower.contains(needle) {
            return category;
        }
    }
    OTHER
}
