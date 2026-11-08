//! LA VERSION DE WINDOWS — une lecture, des prédicats PURS, et un plancher dit.
//!
//! DEMANDE EXPLICITE (2026-10-28) : « je voudrais que l'app puisse aussi
//! fonctionner sur windows 10 ». Décision de la même séance : **UN SEUL
//! exécutable** pour les deux lignées, avec un plancher **Windows 10 22H2**
//! (build 19045).
//!
//! ⛔ **CE MODULE EXISTE PARCE QUE L'ADAPTATION ÉTAIT MUETTE.** Jusqu'ici, rien
//! dans le dépôt ne lisait la version : zéro `RtlGetVersion`, zéro
//! `GetVersionExW`. La seule adaptation était « l'appel échoue et on l'avale »,
//! ce qui laisse une surface entière se taire sans un mot — la zone de
//! notification, entre autres (voir `win32::tray_icons`, qui repose sur
//! `DesktopWindowContentBridge`, absent de Windows 10).
//!
//! ⛔ **LE PIÈGE, ET C'EST LA RAISON D'ÊTRE DU MODULE** : Windows 11 annonce
//! `CurrentMajorVersionNumber = 10`. Un test sur le MAJEUR laisserait donc
//! passer un Windows 11 pour un 10 — ou l'inverse, selon le sens du test. Seul
//! le **numéro de build** sépare les deux lignées (22000 pour la première
//! version de Windows 11), et les noms de versions se chevauchent : « 22H2 »
//! désigne 19045 sur 10 et 22621 sur 11.
//!
//! ⚠️ **POURQUOI LE REGISTRE ET PAS UNE API** : la crate `windows` 0.62 ne
//! génère ni `GetOperatingSystemVersion` ni `VerifyWindowsVersion`, et
//! `GetVersionExW` MENT sur un exe non manifesté. La clé ci-dessous est celle
//! que Windows lui-même affiche dans `winver`, et `win32::registry_util` sait
//! déjà la lire — inutile d'ouvrir un second chemin de lecture.

use std::sync::OnceLock;

use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;

use crate::win32::registry_util::{read_dword_value, read_string_value};

/// Le plancher supporté : **Windows 10 22H2**. En dessous, l'application le dit
/// au journal et reste utilisable — mais rien de ce qui diffère n'est garanti.
pub const BUILD_PLANCHER: u32 = 19045;

/// Le premier build de la lignée Windows 11 : 21H2 (22000).
pub const BUILD_WINDOWS_11: u32 = 22000;

/// La clé que Windows lui-même lit pour `winver`.
const CLE: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

/// Ce que la machine annonce. `display` est le nom commercial (« 22H2 »,
/// « 25H2 ») : il sert au JOURNAL, jamais à une décision — deux lignées le
/// partagent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionWindows {
    pub majeur: u32,
    pub build: u32,
    pub display: String,
}

/// Les trois valeurs, lues UNE fois. `None` si le registre ne répond pas — cas
/// qui ne devrait pas exister, et que les appelants traitent comme « inconnu »,
/// jamais comme « Windows 11 par défaut ».
pub fn version() -> Option<VersionWindows> {
    static CACHE: OnceLock<Option<VersionWindows>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let majeur = read_dword_value(HKEY_LOCAL_MACHINE, CLE, "CurrentMajorVersionNumber")?;
            // `CurrentBuildNumber` est une CHAÎNE (« 19045 »), pas un DWORD —
            // c'est la forme sous laquelle Windows l'écrit depuis toujours.
            let build = read_string_value(HKEY_LOCAL_MACHINE, CLE, "CurrentBuildNumber")?
                .trim()
                .parse::<u32>()
                .ok()?;
            let display = read_string_value(HKEY_LOCAL_MACHINE, CLE, "DisplayVersion").unwrap_or_default();
            Some(VersionWindows { majeur, build, display })
        })
        .clone()
}

/// **PUR** — Windows 11 commence à 22000. Le majeur ne dit rien : il vaut 10 des
/// deux côtés.
pub fn on_windows_11(build: u32) -> bool {
    build >= BUILD_WINDOWS_11
}

/// **PUR** — la lignée Windows 10 : majeur 10 ET build antérieur à 22000.
///
/// ⚠️ Les DEUX conditions comptent : le majeur seul accepterait un Windows 11,
/// le build seul accepterait n'importe quoi d'antérieur. Un Windows 10 de build
/// 22621 ne peut pas exister, mais si le registre le disait, on ne prendrait pas
/// le risque de le croire.
pub fn on_windows_10(majeur: u32, build: u32) -> bool {
    majeur == 10 && build < BUILD_WINDOWS_11
}

/// **PUR** — en dessous du plancher supporté (Windows 10 22H2).
pub fn sous_le_plancher(build: u32) -> bool {
    build < BUILD_PLANCHER
}

/// La lignée courante, décidée UNIQUEMENT par le build (voir `on_windows_11`).
pub fn os_windows_11() -> bool {
    version().is_some_and(|v| on_windows_11(v.build))
}

/// Voir `on_windows_10`. Faux si la version est illisible — on ne suppose pas.
pub fn os_windows_10() -> bool {
    version().is_some_and(|v| on_windows_10(v.majeur, v.build))
}

/// Une ligne courte pour le journal et les messages : « Windows 10 22H2
/// (build 19045) », « Windows 11 25H2 (build 26300) ».
pub fn label_windows() -> String {
    let Some(v) = version() else {
        return "version de Windows ILLISIBLE".to_string();
    };
    let lignee = if on_windows_11(v.build) { "Windows 11" } else { "Windows 10" };
    let nom = if v.display.is_empty() { String::new() } else { format!(" {}", v.display) };
    format!("{lignee}{nom} (build {})", v.build)
}

/// Le rapport de démarrage — appelé par `main` APRÈS `diag::init()` : avant, le
/// journal n'existe pas encore et la ligne serait perdue.
///
/// ⚠️ **UNE LIGNE QUI DIT AUSSI LE PLANCHER** : sous 19045, l'utilisateur doit
/// pouvoir relire POURQUOI une surface se comporte bizarrement, sans qu'on le
/// lui ait dit à l'écran.
pub fn journal_windows() {
    let Some(v) = version() else {
        crate::diag!("Windows : version ILLISIBLE au registre — aucune branche de comportement ne sera prise");
        return;
    };
    if sous_le_plancher(v.build) {
        crate::diag!(
            "Windows : {} — SOUS LE PLANCHER SUPPORTÉ (build {BUILD_PLANCHER}, Windows 10 22H2). \
             L'application démarre, mais les surfaces propres à Windows 11 peuvent se taire.",
            label_windows()
        );
    } else {
        crate::diag!("Windows : {}", label_windows());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐ **LE BUILD SÉPARE LES DEUX LIGNÉES, LE MAJEUR NON.**
    ///
    /// C'est le test qui justifie le module : si un jour quelqu'un « simplifie »
    /// `on_windows_10` en `majeur == 10`, ce test tombe — un Windows 11
    /// (majeur 10) passerait sinon pour un Windows 10.
    #[test]
    fn le_build_separe_les_deux_lignees_et_le_majeur_non() {
        assert!(on_windows_10(10, 19045), "22H2 de Windows 10");
        assert!(!on_windows_11(19045), "22H2 n'est pas Windows 11");
        assert!(on_windows_11(22000), "21H2 de Windows 11");
        assert!(on_windows_11(26300), "25H2 de Windows 11");
        // ⛔ Le majeur annoncé par Windows 11 EST 10 : la condition de lignée
        // doit donc refuser malgré lui.
        assert!(!on_windows_10(10, 22621), "un majeur de 10 ne fait pas un Windows 10");
        assert!(!on_windows_10(10, 26300), "idem sur la build de cette machine");
    }

    /// ⛔ **UN NOM DE VERSION NE DÉCIDE DE RIEN.** « 22H2 » existe sur les deux
    /// lignées (19045 et 22621) : toute décision prise sur `display` est fausse
    /// sur l'une des deux.
    #[test]
    fn le_nom_de_version_ne_decide_de_rien() {
        let onze = VersionWindows { majeur: 10, build: 22621, display: "22H2".into() };
        let dix = VersionWindows { majeur: 10, build: 19045, display: "22H2".into() };
        assert_eq!(onze.display, dix.display);
        assert_ne!(on_windows_11(onze.build), on_windows_11(dix.build));
    }

    /// Le plancher est celui de la décision : **Windows 10 22H2**, build 19045.
    #[test]
    fn le_plancher_est_19045() {
        assert!(sous_le_plancher(19044), "21H2 de Windows 10 est sous le plancher");
        assert!(!sous_le_plancher(19045), "22H2 EST le plancher");
        assert!(!sous_le_plancher(26300));
        assert_eq!(BUILD_PLANCHER, 19045);
    }

    /// ⭐ **LES NOMS DE CLÉS ET DE VALEURS SONT CEUX DU REGISTRE RÉEL** — un
    /// nom mal orthographié rendrait `None`, donc « inconnu », et toutes les
    /// branches se tairaient sans erreur. Ce test lit la machine.
    #[test]
    fn le_registre_repond_et_la_machine_est_supportee() {
        let v = version().expect("CurrentMajorVersionNumber / CurrentBuildNumber illisibles");
        assert_eq!(v.majeur, 10, "Windows annonce toujours un majeur de 10 (10 comme 11)");
        assert!(!sous_le_plancher(v.build), "la machine de test doit être au-dessus du plancher (build {})", v.build);
        assert!(!v.display.is_empty(), "DisplayVersion devrait être renseigné");
        // Le cache rend la MÊME valeur : deux lectures ne doivent pas diverger.
        assert_eq!(version().as_ref(), Some(&v));
    }

    /// Le libellé nomme la lignée ET le build — c'est ce qu'on relira dans le
    /// journal pour comprendre une surface muette.
    #[test]
    fn le_libelle_nomme_la_lignee_et_le_build() {
        let v = version().expect("registre illisible");
        let l = label_windows();
        assert!(l.contains(&v.build.to_string()), "build absent du libellé : {l}");
        let attendu = if on_windows_11(v.build) { "Windows 11" } else { "Windows 10" };
        assert!(l.contains(attendu), "lignée absente du libellé : {l}");
    }
}
