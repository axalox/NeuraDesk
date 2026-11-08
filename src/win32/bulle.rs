//! **LES TOASTS WINDOWS — LA TRACE DANS LE CENTRE DE NOTIFICATIONS.**
//!
//! DEMANDE EXPLICITE (2026-10-04) : « j'aimerais garder une trace dans les
//! notifications Windows ». Le bandeau de NeuraDesk s'efface tout seul et ne
//! laisse rien ; celui-ci, Windows le range dans son centre, où on le retrouve.
//!
//! # ⛔ POURQUOI PAS `Shell_NotifyIcon`, QUI SEMBLAIT POURTANT FAIRE L'AFFAIRE
//!
//! **MESURÉ le 2026-10-04 sur Windows 11 build 26200, deux essais, AUCUN
//! affichage** — ni au coin de l'écran, ni dans le centre :
//!
//! 1. icône de zone de notification **MASQUÉE** (`NIS_HIDDEN`) + `NIF_INFO` ;
//! 2. icône **VISIBLE** + `NIF_INFO` — pour écarter « Windows exige une icône
//!    présente » ;
//!
//! les deux **avec** `NIM_SETVERSION` / `NOTIFYICON_VERSION_4`, après avoir
//! vérifié que l'omettre ne changeait rien non plus.
//!
//! Les quatre appels ont rendu **`true`**. ⛔ **`Shell_NotifyIcon` DIT « PRIS »,
//! PAS « AFFICHÉ »** : le shell acquitte le message, et Windows 11 n'en fait
//! rien. Sa valeur de retour n'est donc **jamais** une preuve — le seul verdict
//! qui compte est l'œil de l'utilisateur.
//!
//! La voie qui reste est celle que Microsoft documente : **le toast WinRT**,
//! ci-dessous. Elle est la seule à nommer l'application émettrice.
//!
//! # L'icône : il FAUT un fichier, l'exe ne suffit pas
//!
//! ⚠️ **Windows ne sait PAS prendre l'icône embarquée dans l'exécutable** pour un
//! toast : le XML attend une URL `file://`. On embarque donc le logo
//! (`NeuraDeskIcon/NeuraDesk-128.png`, 128×128, déjà suivi par git) et on
//! l'écrit **une fois** dans `%LOCALAPPDATA%\NeuraDesk\` — le dossier des
//! caches, celui de ce qui se RÉGÉNÈRE (voir `config::cache_path`).
//!
//! # ⭐ CE QUI FAIT AFFICHER LA NOTIFICATION : UNE CLÉ DE REGISTRE
//!
//! **MESURÉ le 2026-10-04, et c'est le cœur de ce module.** Le toast partait,
//! `Show` rendait `Ok`, l'entrée arrivait même dans la base de Windows — et
//! **rien ne s'affichait**. Ce qui manquait n'était ni l'envoi ni l'icône :
//!
//! ```text
//! HKCU\Software\Classes\AppUserModelId\NeuraDesk
//!     DisplayName = "NeuraDesk"
//!     IconUri     = C:\…\NeuraDesk\notification.png
//! ```
//!
//! **La différence avant/après est mesurable et a été mesurée** :
//!
//! | | clé absente | clé posée |
//! |---|---|---|
//! | `…\Notifications\Settings\NeuraDesk` | **absente** | créée, `PeriodicNotificationCount = 1` |
//! | la notification | rien, quatre essais | **vue par l'utilisateur** |
//!
//! C'est le SEUL chemin trouvé qui n'exige **aucun raccourci au menu Démarrer** —
//! donc rien de visible chez l'utilisateur, et la distribution portable de
//! NeuraDesk (`dist/`) reste intacte. Les voies essayées avant et ÉCARTÉES sont
//! documentées plus bas.
//!
//! ⚠️ **C'EST LA DEUXIÈME ÉCRITURE AU REGISTRE DU DÉPÔT** (`win32::registry_util`
//! n'en avait qu'une, `write_dword_value`, et sa doc disait « la seule » — elle a
//! été corrigée). Elle modifie l'état du SYSTÈME, pas une préférence de
//! NeuraDesk : elle est donc posée sous `HKEY_CURRENT_USER`, elle est
//! IDEMPOTENTE (réécrire les mêmes valeurs ne change rien), et elle est
//! réversible en supprimant la clé.
//!
//! # Ce que ce module ne fait pas
//!
//! ⚠️ **IL NE JOUE AUCUN SON** : le son est un réglage de l'automatisation
//! (`Action::Notifier`, champ `son`), joué par `automatisations::executer` — deux
//! sources feraient deux sonneries pour une seule notification.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use windows::core::HSTRING;
use windows::Data::Xml::Dom::XmlDocument;
use windows::Win32::System::Registry::HKEY_CURRENT_USER;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

/// L'identité sous laquelle Windows présente nos notifications (voir la doc de
/// module : elle n'est PAS inscrite, donc Windows l'affiche telle quelle).
const IDENTITE: &str = "NeuraDesk";

/// Le logo, embarqué dans le binaire (le fichier reste suivi par git : c'est lui
/// que `build.rs` donne aussi à l'exe).
const LOGO: &[u8] = include_bytes!("../../NeuraDeskIcon/NeuraDesk-128.png");
/// Le nom sous lequel il est déposé dans le dossier des caches.
const LOGO_FICHIER: &str = "notification.png";

thread_local! {
    /// L'URL `file://` du logo, calculée et écrite UNE fois par processus.
    static LOGO_POSE: RefCell<Option<Option<String>>> = const { RefCell::new(None) };
    /// L'identité a-t-elle déjà été déclarée ? (voir `declarer_l_identite`)
    static IDENTITE_POSE: Cell<bool> = const { Cell::new(false) };
}

/// **ENVOIE UN TOAST — et dit si Windows l'a pris.**
///
/// ⚠️ **« PRIS » N'EST PAS « AFFICHÉ »** — c'est la leçon entière de ce module
/// (voir la doc de module). L'appelant journalise le geste, jamais une promesse
/// d'affichage.
pub fn notifier(titre: &str, message: &str) -> bool {
    notifier_avec_liens(titre, message, None, &[])
}

/// Un lien de toast : un libellé de bouton et l'URL qu'il ouvre.
pub struct Lien<'a> {
    pub libelle: &'a str,
    pub url: &'a str,
}

/// **LE MÊME TOAST, CLIQUABLE** — `clic` est l'URL ouverte par un clic sur le
/// corps de la notification, `boutons` ceux qui s'ajoutent sous le texte.
///
/// ⭐ **ACTIVATION « PROTOCOLE »** : c'est WINDOWS qui ouvre l'URL, l'application
/// n'a aucun rappel à recevoir — ce qui la dispense d'un activateur COM, qu'un
/// exe non empaqueté et sans raccourci au menu Démarrer n'a pas. Le bouton reste
/// valable dans le centre de notifications, même NeuraDesk fermé.
pub fn notifier_avec_liens(titre: &str, message: &str, clic: Option<&str>, boutons: &[Lien]) -> bool {
    // Un toast sans un mot ne dit rien : ni titre ni message, rien à envoyer.
    if titre.trim().is_empty() && message.trim().is_empty() {
        return false;
    }
    declarer_l_identite();
    match envoyer(&xml_avec_liens(titre, message, logo_en_url().as_deref(), clic, boutons)) {
        Ok(()) => true,
        Err(erreur) => {
            crate::diag!("[bulle] Windows a refusé le toast : {erreur}");
            false
        }
    }
}

/// Le document que `ToastNotification` attend.
///
/// ⛔ **TOUT TEXTE VENU DE L'UTILISATEUR EST ÉCHAPPÉ.** Le titre et le message
/// sont saisis dans le dash des automatisations : un « & » ou un « < » non
/// échappé produirait un XML invalide, donc un toast REFUSÉ — et, si le document
/// arrivait quand même à être analysé, du balisage injecté dans la notification.
#[cfg(test)]
fn xml_du_toast(titre: &str, message: &str, logo: Option<&str>) -> String {
    xml_avec_liens(titre, message, logo, None, &[])
}

/// Le document complet. Sans `clic` ni bouton, il est OCTET POUR OCTET celui
/// d'avant (`<toast>…</toast>`), que les automatisations envoient déjà.
fn xml_avec_liens(titre: &str, message: &str, logo: Option<&str>, clic: Option<&str>, boutons: &[Lien]) -> String {
    let ouverture = match clic {
        Some(url) => format!("<toast activationType=\"protocol\" launch=\"{}\">", echapper(url)),
        None => "<toast>".to_string(),
    };
    let actions = if boutons.is_empty() {
        String::new()
    } else {
        let mut actions = String::from("<actions>");
        for lien in boutons {
            actions.push_str(&format!(
                "<action activationType=\"protocol\" content=\"{}\" arguments=\"{}\"/>",
                echapper(lien.libelle),
                echapper(lien.url)
            ));
        }
        actions.push_str("</actions>");
        actions
    };
    let image = match logo {
        Some(url) => format!(
            "<image placement=\"appLogoOverride\" hint-crop=\"circle\" src=\"{}\"/>",
            echapper(url)
        ),
        None => String::new(),
    };
    format!(
        "{ouverture}<visual><binding template=\"ToastGeneric\">\
         <text>{}</text><text>{}</text>{image}\
         </binding></visual>{actions}</toast>",
        echapper(titre),
        echapper(message),
    )
}

/// **UN CHEMIN EN URL `file:///`** — barres obliques, et tout octet hors de
/// l'alphabet sûr encodé en `%XX` (UTF-8) : les noms d'enregistrement
/// contiennent des ESPACES (« Enregistrement 2026-11-01 03-20-00.mp4 »), et un
/// dossier peut porter des accents. `:` et `/` restent tels quels.
pub fn url_de_fichier(chemin: &Path) -> String {
    let texte = chemin.to_string_lossy().replace('\\', "/");
    let texte = texte.trim_start_matches('/');
    let mut url = String::from("file:///");
    for octet in texte.bytes() {
        match octet {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => url.push(octet as char),
            autre => url.push_str(&format!("%{autre:02X}")),
        }
    }
    url
}

/// **LES CINQ CARACTÈRES QUI FERMENT UN TEXTE XML.** `&` D'ABORD : l'échapper en
/// dernier ré-échapperait les `&` qu'on vient d'introduire.
fn echapper(texte: &str) -> String {
    let mut sortie = String::with_capacity(texte.len());
    for caractere in texte.chars() {
        match caractere {
            '&' => sortie.push_str("&amp;"),
            '<' => sortie.push_str("&lt;"),
            '>' => sortie.push_str("&gt;"),
            '"' => sortie.push_str("&quot;"),
            '\'' => sortie.push_str("&apos;"),
            autre => sortie.push(autre),
        }
    }
    sortie
}

/// **DÉCLARE L'IDENTITÉ DE L'APPLICATION — ce qui fait AFFICHER la notification.**
///
/// Sans ces deux valeurs, Windows accepte le toast, l'enregistre, et ne l'affiche
/// jamais (voir la doc de module : c'est mesuré, avant/après).
///
/// ⚠️ **POSÉE UNE SEULE FOIS PAR PROCESSUS**, et IDEMPOTENTE de toute façon : les
/// valeurs ne changent pas d'un envoi à l'autre.
///
/// ⚠️ **UN ÉCHEC N'EMPÊCHE PAS L'ENVOI.** Si une stratégie de groupe interdit
/// l'écriture, la notification partira sans s'afficher — c'est un moindre mal, et
/// le journal le dit.
fn declarer_l_identite() {
    IDENTITE_POSE.with(|pose| {
        if pose.get() {
            return;
        }
        pose.set(true);
        let cle = format!(r"Software\Classes\AppUserModelId\{IDENTITE}");
        let ecrire = crate::win32::registry_util::write_string_value;
        if !ecrire(HKEY_CURRENT_USER, &cle, "DisplayName", IDENTITE) {
            crate::diag!("[bulle] l'identité n'a pas pu être déclarée — la notification risque de ne pas s'afficher");
            return;
        }
        if let Some(chemin) = chemin_du_logo() {
            // ⚠️ Un CHEMIN, pas une URL : `IconUri` est lu par le shell, pas par
            // le XML du toast (qui, lui, veut `file://`).
            ecrire(HKEY_CURRENT_USER, &cle, "IconUri", &chemin.to_string_lossy());
        }
    });
}

/// Le chemin du logo dans les caches, écrit au premier appel.
fn chemin_du_logo() -> Option<PathBuf> {
    let chemin = crate::config::cache_path(LOGO_FICHIER)?;
    if !chemin.exists() {
        std::fs::create_dir_all(chemin.parent()?).ok()?;
        std::fs::write(&chemin, LOGO).ok()?;
    }
    Some(chemin)
}

/// Le même chemin, en URL `file://` — ce que le XML du toast attend.
fn url_du_logo(chemin: &Path) -> String {
    // `file:///C:/…` — Windows veut des barres obliques, même sur un chemin qui
    // n'en contient aucune.
    let texte = chemin.to_string_lossy().replace('\\', "/");
    let texte = texte.strip_prefix('/').unwrap_or(&texte).to_string();
    format!("file:///{texte}")
}

/// L'URL `file://` du logo, écrit dans les caches au premier appel.
///
/// Rend `None` si le dossier n'a pas pu être écrit : le toast part alors SANS
/// logo plutôt que de ne pas partir — une notification sans vignette vaut mieux
/// qu'aucune notification.
fn logo_en_url() -> Option<String> {
    LOGO_POSE.with(|cache| {
        if let Some(deja) = cache.borrow().as_ref() {
            return deja.clone();
        }
        let calcule = chemin_du_logo().map(|chemin| url_du_logo(&chemin));
        *cache.borrow_mut() = Some(calcule.clone());
        calcule
    })
}

fn envoyer(xml: &str) -> windows::core::Result<()> {
    let document = XmlDocument::new()?;
    document.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&document)?;
    let notificateur = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(IDENTITE))?;
    notificateur.Show(&toast)
}

#[cfg(test)]
mod tests {
    use super::{echapper, url_de_fichier, xml_avec_liens, xml_du_toast, Lien};
    use std::path::Path;

    /// Sans lien, le document garde sa forme d'avant : les automatisations ne
    /// voient AUCUN changement.
    #[test]
    fn sans_lien_le_toast_ne_change_pas() {
        let xml = xml_avec_liens("T", "M", None, None, &[]);
        assert!(xml.starts_with("<toast><visual>"));
        assert!(!xml.contains("activationType") && !xml.contains("<actions>"));
    }

    /// Le clic et les boutons sont des activations PROTOCOLE, échappées.
    #[test]
    fn les_liens_sont_des_activations_protocole() {
        let boutons = [
            Lien { libelle: "Ouvrir la vidéo", url: "file:///C:/V/a%20b.mp4" },
            Lien { libelle: "Ouvrir le dossier", url: "file:///C:/V&W/" },
        ];
        let xml = xml_avec_liens("T", "M", None, Some("file:///C:/V/a%20b.mp4"), &boutons);
        assert!(xml.starts_with("<toast activationType=\"protocol\" launch=\"file:///C:/V/a%20b.mp4\">"), "{xml}");
        assert_eq!(xml.matches("<action ").count(), 2);
        assert!(xml.contains("content=\"Ouvrir le dossier\" arguments=\"file:///C:/V&amp;W/\""), "{xml}");
        assert!(xml.find("</visual>").unwrap() < xml.find("<actions>").unwrap(), "actions APRÈS visual");
        assert!(xml.ends_with("</actions></toast>"));
    }

    /// Les espaces et les accents sont encodés ; le lecteur, `:` et `/` non.
    #[test]
    fn un_chemin_devient_une_url_file() {
        assert_eq!(
            url_de_fichier(Path::new(r"C:\Users\Moi\Vidéos\Enregistrement 2026.mp4")),
            "file:///C:/Users/Moi/Vid%C3%A9os/Enregistrement%202026.mp4"
        );
        assert_eq!(url_de_fichier(Path::new(r"C:\a&b#c")), "file:///C:/a%26b%23c");
    }

    /// ⛔ **UN TEXTE VENU DE L'UTILISATEUR NE DOIT PAS POUVOIR CASSER LE XML.** Le
    /// titre et le message sont saisis à la main : un « & » ou un « < » non
    /// échappé fait REFUSER le toast par Windows, sans le dire.
    #[test]
    fn le_texte_de_l_utilisateur_est_echappe() {
        assert_eq!(echapper("a & b"), "a &amp; b");
        assert_eq!(echapper("<script>"), "&lt;script&gt;");
        assert_eq!(echapper("il a dit \"oui\""), "il a dit &quot;oui&quot;");
        assert_eq!(echapper("l'apostrophe"), "l&apos;apostrophe");
        // ⚠️ L'esperluette D'ABORD : échappée en dernier, elle ré-échapperait les
        // « & » qu'on vient d'introduire (« &lt; » deviendrait « &amp;lt; »).
        assert_eq!(echapper("&lt;"), "&amp;lt;");
    }

    #[test]
    fn le_toast_porte_le_titre_le_message_et_le_logo() {
        let xml = xml_du_toast("Titre", "Message", Some("file:///C:/logo.png"));
        assert!(xml.contains("<text>Titre</text>"));
        assert!(xml.contains("<text>Message</text>"));
        assert!(xml.contains("appLogoOverride"));
        assert!(xml.contains("file:///C:/logo.png"));
        assert!(xml.starts_with("<toast>") && xml.ends_with("</toast>"));
    }

    /// Sans logo, le toast part QUAND MÊME — une notification sans vignette vaut
    /// mieux qu'aucune notification.
    #[test]
    fn sans_logo_le_toast_reste_valide() {
        let xml = xml_du_toast("T", "M", None);
        assert!(!xml.contains("<image"));
        assert!(xml.ends_with("</toast>"));
    }

    /// Un titre hostile ne peut pas introduire de balise.
    #[test]
    fn un_titre_hostile_n_injecte_aucune_balise() {
        let xml = xml_du_toast("</text><image src=\"x\"/>", "m", None);
        assert!(!xml.contains("<image"));
        assert_eq!(xml.matches("<text>").count(), 2, "exactement les deux nôtres");
    }

    /// Le chemin du logo est échappé LUI AUSSI : un dossier peut contenir « & ».
    #[test]
    fn le_chemin_du_logo_est_echappe() {
        let xml = xml_du_toast("T", "M", Some("file:///C:/a&b/logo.png"));
        assert!(xml.contains("a&amp;b"), "et pas un « & » nu : {xml}");
    }

    /// **LA SONDE — la seule question qui compte pour ce module, et elle ne se
    /// lit pas dans le code : le toast APPARAÎT-IL ?** Aucune API ne le dit ;
    /// seul l'œil de l'utilisateur tranche.
    ///
    /// Lancer : `cargo test --bins -- --ignored --nocapture sonde_du_toast`
    #[test]
    #[ignore = "sonde a la main — ENVOIE UN VRAI toast"]
    fn sonde_du_toast() {
        let remis = super::notifier("NeuraDesk — essai", "Un toast WinRT, avec le logo de l'application.");
        println!("\ntoast remis a Windows : {remis}");
        println!("=> regarder le coin bas-droite PUIS le centre de notifications\n");
    }
}
