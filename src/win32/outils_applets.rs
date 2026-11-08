//! **LES LOGICIELS QUE NOS APPLETS UTILISENT, ET QUI NE SONT PAS LES NÔTRES.**
//!
//! DEMANDE (2026-10-09) : « dans les fonctionnalités, faire une rubrique
//! Suggestion, avec des liens cliquables pour aller sur la page des logiciels que
//! les applets utilisent mais qui ne sont pas installés ».
//!
//! ⭐ **UNE SEULE TABLE, ET ELLE EST ICI.** Les URL ne sont PAS dans la page : une
//! URL écrite en dur dans `settings.html` serait une seconde source de vérité,
//! qui mentirait dès qu'un éditeur déplace sa page — et la doctrine de ce dépôt
//! est de ne pas les multiplier (`scanner::profils_naps2`, `apps::categoriser`).
//!
//! ⚠️ **LE LIEN VISE LA PAGE DU LOGICIEL, PAS LE TÉLÉCHARGEMENT** — décision de
//! l'utilisateur : la page est stable, le lien de téléchargement change à chaque
//! version.
//!
//! ⚠️ **UN OUTIL DONT LA PRÉSENCE N'EST PAS VÉRIFIABLE NE RENTRE PAS ICI.** On ne
//! propose pas d'installer ce qu'on ne sait pas reconnaître : la rubrique dirait
//! « non installé » à quelqu'un qui l'a.

use std::path::Path;

/// Un logiciel tiers dont une applet dépend.
pub struct Outil {
    /// Le libellé de l'APPLET qui en dépend — c'est ce que la page montre en
    /// premier : l'utilisateur cherche « la Recherche », pas « Everything ».
    pub applet: &'static str,
    /// Le nom du logiciel à installer.
    pub logiciel: &'static str,
    /// **La PAGE du logiciel** (voir la doc de module : pas le téléchargement).
    pub url: &'static str,
    /// Comment on sait qu'il est là. **Une fonction, pas un booléen** : la table
    /// est une constante, la réponse ne l'est pas.
    pub installe: fn() -> bool,
}

/// **CE QUE NOS APPLETS UTILISENT SANS L'EMBARQUER.**
///
/// ⚠️ **VOLONTAIREMENT COURTE.** N'y figure que ce qui a **et** une dépendance
/// externe réelle, **et** un test de présence — LE MÊME que celui de l'élément
/// qui s'en sert, sinon la rubrique et l'applet se contrediraient.
///
/// ⭐ ÉTENDUE LE 2026-10-31 — DEMANDE EXPLICITE : « proposer automatiquement
/// l'installation des programmes et polices utiles à l'app » ; première étape
/// convenue : la détection. Voicemeeter (reconnu comme `win32::voicemeeter` le
/// charge), PowerShell 7 (comme le Terminal le cherche), la police Segoe Fluent
/// Icons (comme `ui::icones` la choisit). Le runtime Visual C++ n'y est pas :
/// sans lui l'application ne démarre pas, et il est livré dans l'archive.
///
/// ⭐ LES POLICES — DEMANDE EXPLICITE (2026-10-31) : « fais pareil pour les
/// polices utilisées par l'app ». Celles des styles intégrés (`assets/styles`)
/// ne sont proposées que si elles sont EN USAGE (`police_de_l_interface`) :
/// suggérer Roboto à qui a le style Cinnamon serait du bruit. Cascadia Mono
/// (pages de code, Consolas en repli) et Charis SIL (Phonétique) sont toujours
/// demandées par leurs pages. Toutes sont reconnues par GDI, comme la liste des
/// polices de Réglages (`win32::polices`) : Chromium trouve ce que GDI nomme.
pub const OUTILS: &[Outil] = &[
    Outil {
        applet: "Recherche",
        logiciel: "Everything",
        url: "https://www.voidtools.com/",
        installe: || crate::win32::everything::installe().is_some(),
    },
    Outil {
        applet: "Scanner un document",
        logiciel: "NAPS2",
        url: "https://www.naps2.com/",
        installe: || crate::win32::scanner::chemin_console().is_some(),
    },
    Outil {
        applet: "Contrôles audio",
        logiciel: "Voicemeeter",
        url: "https://vb-audio.com/Voicemeeter/",
        installe: crate::win32::voicemeeter::installe,
    },
    Outil {
        applet: "Terminal",
        logiciel: "PowerShell 7",
        url: "https://learn.microsoft.com/powershell/scripting/install/installing-powershell-on-windows",
        installe: crate::win32::pty::pwsh_installe,
    },
    Outil {
        applet: "Icônes de la barre du haut",
        logiciel: "la police Segoe Fluent Icons",
        url: "https://learn.microsoft.com/windows/apps/design/style/segoe-fluent-icons-font",
        installe: || crate::ui::icones::police_installee("SegoeIcons.ttf"),
    },
    Outil {
        applet: "Texte de l'interface",
        logiciel: "la police Ubuntu",
        url: "https://design.ubuntu.com/font",
        installe: || police_de_l_interface("Ubuntu"),
    },
    Outil {
        applet: "Texte de l'interface",
        logiciel: "la police Noto Sans",
        url: "https://fonts.google.com/noto/specimen/Noto+Sans",
        installe: || police_de_l_interface("Noto Sans"),
    },
    Outil {
        applet: "Texte de l'interface",
        logiciel: "la police Adwaita Sans Text",
        url: "https://gitlab.gnome.org/GNOME/adwaita-fonts",
        installe: || police_de_l_interface("Adwaita Sans Text"),
    },
    Outil {
        applet: "Texte de l'interface",
        logiciel: "la police Open Sans",
        url: "https://fonts.google.com/specimen/Open+Sans",
        installe: || police_de_l_interface("Open Sans"),
    },
    Outil {
        applet: "Texte de l'interface",
        logiciel: "la police Inter",
        url: "https://rsms.me/inter/",
        installe: || police_de_l_interface("Inter"),
    },
    Outil {
        applet: "Texte de l'interface",
        logiciel: "la police Roboto",
        url: "https://fonts.google.com/specimen/Roboto",
        installe: || police_de_l_interface("Roboto"),
    },
    Outil {
        applet: "Texte de l'interface",
        logiciel: "la police Segoe UI Variable Text",
        url: "https://learn.microsoft.com/windows/apps/design/signature-experiences/typography",
        installe: || police_de_l_interface("Segoe UI Variable Text"),
    },
    Outil {
        applet: "Code (Commandes, SSH, Notes, Presse-papiers)",
        logiciel: "la police Cascadia Mono",
        url: "https://github.com/microsoft/cascadia-code",
        installe: || famille_installee("Cascadia Mono"),
    },
    Outil {
        applet: "Symboles phonétiques",
        logiciel: "la police Charis SIL",
        url: "https://software.sil.org/charis/",
        installe: || famille_installee("Charis SIL"),
    },
];

/// La famille `nom` est-elle installée (au sens de GDI) ?
///
/// ⭐ **« CHARIS SIL » EST INSTALLÉE SOUS « CHARIS »** (mesuré le 2026-11-04 :
/// la table `name` du TTF publié par SIL Global en 2025 donne la famille
/// « Charis », et GDI ne nomme qu'elle). Une comparaison à l'identique
/// annonçait donc « il manque la police Charis SIL » à qui venait de
/// l'installer. On accepte qu'une famille installée soit le DÉBUT, mot pour
/// mot, du nom demandé — `Charis` répond oui à `Charis SIL` — et JAMAIS
/// l'inverse : `Noto Sans Mono CJK SC` ne doit pas faire croire que
/// `Noto Sans Mono` est là. Même règle que `policeInstallee` dans
/// `assets/popups/settings.html`.
fn famille_installee(nom: &str) -> bool {
    crate::win32::polices::installees().iter().any(|famille| famille_pour(famille, nom))
}

/// Le cœur de `famille_installee`, sans la machine (testable) : la famille
/// installée est-elle le nom demandé, ou son début mot pour mot ?
fn famille_pour(installee: &str, demande: &str) -> bool {
    let i = installee.trim().to_lowercase();
    let d = demande.trim().to_lowercase();
    if i.is_empty() {
        return false;
    }
    if d == i {
        return true;
    }
    match d.strip_prefix(&i) {
        Some(reste) => reste.chars().next().is_some_and(|c| !c.is_alphanumeric()),
        None => false,
    }
}

/// Une police de style : présente, OU inutile — seule celle qui habille
/// l'interface (celle du style, ou celle choisie dans Réglages) compte.
fn police_de_l_interface(nom: &str) -> bool {
    presente_ou_inutile(nom, &crate::ui::style::police(), famille_installee)
}

/// Le cœur de `police_de_l_interface`, sans la machine : `en_usage` est la
/// police de l'interface, `installee` le test de présence.
fn presente_ou_inutile(nom: &str, en_usage: &str, installee: impl Fn(&str) -> bool) -> bool {
    !en_usage.eq_ignore_ascii_case(nom) || installee(nom)
}

/// Présence de chaque outil de `OUTILS`, dans son ordre — **EN CACHE** : la
/// page des Réglages la reçoit à CHAQUE poussée (60 Hz), et Voicemeeter se
/// reconnaît en parcourant les désinstallations du registre. Relue toutes les
/// 15 s, comme `everything::installe` : une installation faite pendant que
/// la page est ouverte s'y voit sans relancer.
fn presences() -> Vec<bool> {
    const RELECTURE: std::time::Duration = std::time::Duration::from_secs(15);
    static CACHE: std::sync::Mutex<Option<(std::time::Instant, Vec<bool>)>> = std::sync::Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((quand, presences)) = cache.as_ref() {
        if quand.elapsed() < RELECTURE {
            return presences.clone();
        }
    }
    // Les polices installées depuis la dernière ouverture de Réglages
    // (voir `win32::polices`, qui ne relit qu'à l'ouverture).
    crate::win32::polices::rafraichir();
    let presences: Vec<bool> = OUTILS.iter().map(|outil| (outil.installe)()).collect();
    *cache = Some((std::time::Instant::now(), presences.clone()));
    presences
}

/// **CE QUI MANQUE, DANS L'ORDRE DE LA TABLE** — c'est cette liste que la page
/// dessine, et c'est **son rang** qu'elle renvoie (voir `ouvrir`).
pub fn manquants() -> Vec<&'static Outil> {
    // ⚠️ **LEVIER DE VÉRIFICATION** — `NEURADESK_DEBUG_OUTIL_MANQUANT=<logiciel>`
    // feint l'absence de celui-là.
    //
    // ⛔ **SANS LUI, LA RUBRIQUE NE SE VÉRIFIE PAS ICI** : NAPS2 et Everything
    // sont tous les deux installés sur cette machine, donc la liste est vide — et
    // une rubrique qu'on ne peut pas montrer ne se vérifie pas. C'est le même
    // besoin que `NEURADESK_DEBUG_PAGE` pour la vue en grand.
    let feint = std::env::var("NEURADESK_DEBUG_OUTIL_MANQUANT").ok();
    OUTILS
        .iter()
        .zip(presences())
        .filter(|(outil, present)| match &feint {
            Some(nom) if outil.logiciel.eq_ignore_ascii_case(nom) => true,
            _ => !present,
        })
        .map(|(outil, _)| outil)
        .collect()
}

/// **OUVRE LA PAGE D'UN OUTIL DANS LE NAVIGATEUR PAR DÉFAUT.**
///
/// ⛔ **PAR RANG, JAMAIS PAR URL.** L'URL vient de la table, pas de la page : une
/// page qui enverrait l'adresse à ouvrir ferait de Rust un ouvre-boîte pour ce
/// qu'un code divergé voudra — la règle est la même que pour `cef_scanner`, qui
/// ne croit pas la page sur le nom d'un profil.
pub fn ouvrir(rang: &str) {
    let Ok(rang) = rang.parse::<usize>() else {
        return crate::diag!("[outils] rang illisible : {rang:?}");
    };
    // ⚠️ **DANS `manquants()`, PAS DANS `OUTILS`** : c'est la liste que la page a
    // reçue, donc celle dont les rangs veulent dire quelque chose.
    let Some(outil) = manquants().get(rang).copied() else {
        return crate::diag!("[outils] rang hors de la liste : {rang}");
    };
    crate::diag!("[outils] ouverture de {}", outil.url);
    // ⚠️ **ON PASSE PAR `appicon::launch`, QUI PORTE DÉJÀ LE VERBE `"open"`.** Une
    // URL n'est pas un chemin, mais `create_process_detached` échoue dessus (ce
    // n'est pas un exécutable) et `launch` retombe alors sur `ShellExecuteW`, qui
    // ouvre le navigateur — exactement comme un double-clic sur un lien.
    crate::win32::appicon::launch(Path::new(outil.url));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐ **CHAQUE ENTRÉE EST COMPLÈTE ET OUVRABLE.** Une URL vide ou relative
    /// ouvrirait le dossier courant dans l'explorateur — un défaut qui ne se voit
    /// qu'en cliquant, et qui enverrait l'utilisateur nulle part.
    #[test]
    fn chaque_outil_a_une_page_du_logiciel() {
        assert!(!OUTILS.is_empty(), "une table vide ne propose rien");
        for outil in OUTILS {
            assert!(!outil.applet.trim().is_empty(), "l'applet doit etre nommee");
            assert!(!outil.logiciel.trim().is_empty(), "le logiciel doit etre nomme");
            assert!(outil.url.starts_with("https://"), "{} : une URL doit etre absolue et sure", outil.url);
            // ⚠️ **LA PAGE, PAS LE TÉLÉCHARGEMENT** — décision de l'utilisateur.
            assert!(
                !outil.url.contains("/download") && !outil.url.ends_with(".exe"),
                "{} : le lien doit viser la PAGE du logiciel",
                outil.url
            );
        }
    }

    /// ⭐ **LA POLICE DE CHAQUE STYLE INTÉGRÉ EST PROPOSÉE** — sauf Segoe UI,
    /// celle du système. Un style ajouté sans sa ligne ici s'afficherait dans
    /// une police de repli sans que la rubrique le dise.
    #[test]
    fn la_police_de_chaque_style_integre_est_proposee() {
        for style in crate::ui::style::integres() {
            if style.police == "Segoe UI" {
                continue;
            }
            let attendu = format!("la police {}", style.police);
            assert!(OUTILS.iter().any(|o| o.logiciel == attendu), "{} : {attendu} absente de OUTILS", style.id);
        }
    }

    /// ⭐ **UNE POLICE DE STYLE N'EST SIGNALÉE QUE SI ELLE HABILLE L'INTERFACE.**
    #[test]
    fn une_police_de_style_inutilisee_n_est_pas_signalee() {
        let absente = |_: &str| false;
        let presente = |_: &str| true;
        assert!(presente_ou_inutile("Roboto", "Ubuntu", absente), "pas en usage : rien à signaler");
        assert!(!presente_ou_inutile("Ubuntu", "Ubuntu", absente), "en usage et absente : à signaler");
        assert!(!presente_ou_inutile("Ubuntu", "ubuntu", absente), "la casse ne compte pas");
        assert!(presente_ou_inutile("Ubuntu", "Ubuntu", presente));
    }

    /// ⭐ **« CHARIS SIL » EST INSTALLÉE SOUS « CHARIS »** (mesuré le
    /// 2026-11-04). La règle : une famille installée peut être le DÉBUT, mot
    /// pour mot, du nom demandé — jamais l'inverse.
    #[test]
    fn une_famille_plus_courte_satisfait_le_nom_du_produit() {
        assert!(famille_pour("Charis", "Charis SIL"), "le cas mesuré");
        assert!(famille_pour("charis", "CHARIS SIL"), "la casse ne compte pas");
        assert!(famille_pour("Charis", "Charis"), "l'identique");
        assert!(!famille_pour("Charis Medium", "Charis SIL"), "une AUTRE famille, pas la même");
        assert!(!famille_pour("Noto Sans Mono CJK SC", "Noto Sans Mono"), "jamais l'inverse");
        assert!(!famille_pour("Charis", "CharisSIL"), "collé : ce n'est plus le même nom");
        assert!(!famille_pour("", "Charis"));
        // La page des Réglages applique la MÊME règle (son test d'installation).
        let reglages = include_str!("../../assets/popups/settings.html");
        assert!(reglages.contains("function policeInstallee(active, installees)"), "deux règles divergentes");
    }

    /// La page des symboles phonétiques demande la famille que GDI nomme —
    /// « Charis » — et la garde AVANT l'ancien nom : sinon la police installée
    /// n'est jamais utilisée et la page retombe sur Segoe UI.
    #[test]
    fn la_page_phonetique_demande_la_famille_installee() {
        let page = include_str!("../../assets/popups/phonetique.html");
        assert!(page.contains("\"Charis\", \"Charis SIL\""), "« Charis » doit passer avant « Charis SIL »");
    }

    /// GDI nomme Segoe UI sur tout Windows : si elle n'est pas « installée »,
    /// c'est le test de présence qui est cassé, et toutes les polices
    /// seraient signalées à tort.
    #[test]
    fn la_police_du_systeme_est_reconnue() {
        assert!(famille_installee("Segoe UI"));
        assert!(famille_installee("segoe ui"));
        assert!(!famille_installee("Police Qui N Existe Pas"));
    }

    /// ⛔ **DEUX FOIS LE MÊME OUTIL SERAIT DEUX FOIS LA MÊME SUGGESTION** — et
    /// deux rangs pour une seule chose, ce que la page enverrait de travers.
    #[test]
    fn aucun_outil_n_est_propose_deux_fois() {
        for (rang, outil) in OUTILS.iter().enumerate() {
            let suivants = &OUTILS[rang + 1..];
            assert!(
                !suivants.iter().any(|autre| autre.logiciel == outil.logiciel),
                "{} est propose deux fois",
                outil.logiciel
            );
        }
    }

    /// ⭐ **LA LISTE DES MANQUANTS EST UN SOUS-ENSEMBLE DE LA TABLE, ET GARDE
    /// L'ORDRE.** C'est ce qui rend le rang envoyé par la page sûr : `ouvrir`
    /// indexe `manquants()`, donc les deux doivent désigner la même chose.
    #[test]
    fn les_manquants_suivent_l_ordre_de_la_table() {
        let manquants = manquants();
        assert!(manquants.len() <= OUTILS.len());
        let rangs: Vec<usize> = manquants.iter().map(|m| OUTILS.iter().position(|o| o.logiciel == m.logiciel).unwrap()).collect();
        let mut tries = rangs.clone();
        tries.sort_unstable();
        assert_eq!(rangs, tries, "l'ordre de la table doit etre conserve");

        // Et un rang hors bornes ne doit RIEN ouvrir (voir `ouvrir`, qui le dit).
        assert!(manquants.get(manquants.len()).is_none());
    }
}
