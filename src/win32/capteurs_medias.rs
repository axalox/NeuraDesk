//! ⭐ **MICRO ET CAMÉRA EN COURS D'UTILISATION** — idée n°3 reprise de FluidWin
//! (`sensori.h`) : savoir ce qui écoute et ce qui regarde, et le montrer dans la
//! barre du haut (voir `ui::capteurs`).
//!
//! **UNE SEULE SOURCE POUR LES DEUX : LE REGISTRE DE CONFIDENTIALITÉ DE WINDOWS**,
//! `HKCU\…\CapabilityAccessManager\ConsentStore\{microphone,webcam}` et le
//! sous-dossier `NonPackaged` de chacun. Chaque application y a
//! `LastUsedTimeStart`/`LastUsedTimeStop` en `REG_QWORD` (horodatages `FILETIME`),
//! et une utilisation EST EN COURS quand le début est POSTÉRIEUR à la fin (voir
//! `utilisation_active`). C'est la source sur laquelle s'appuient les indicateurs
//! de Windows : elle sait dire QUELLE application utilise, quand les sessions
//! WASAPI ne savent dire que « quelque chose tire du son » (voir plus bas).
//!
//! ⛔ **LE MICRO N'EST PAS LU PAR WASAPI, ET C'EST UN RELEVÉ.** Les sessions de
//! capture à l'état « actif » paraissaient le signal le plus direct — mesuré le
//! 2026-11-26 : **cinq** sessions actives sans que personne n'enregistre, dont
//! QUATRE pour Voicemeeter, qui garde ses entrées ouvertes en permanence pour
//! router le son (voir `audio::pids_de_capture_actifs`, l'essai qui l'a montré).
//! Un voyant bâti là-dessus serait allumé sans arrêt. Le registre, lui, y voit
//! une utilisation TERMINÉE : `LastUsedTimeStop` y est postérieur au début.
//!
//! ⚠️ **LES DEUX ARBRES SONT NÉCESSAIRES** : les applications empaquetées
//! (`Microsoft.WindowsCamera_…`) sont à la racine, les applications de bureau
//! sous `NonPackaged`, avec des noms d'exécutables `C:#…\app.exe`. Relevé sur
//! cette machine le 2026-11-26 : Chrome, Discord, Telegram et Snap Camera ne
//! figurent QUE sous `NonPackaged`, la Caméra de Windows QUE dans l'arbre
//! empaqueté.
//!
//! ⚠️ **VALEURS ABSENTES = PAS EN USAGE.** Plusieurs sous-clés existent sans
//! aucune des deux valeurs (l'application a la permission, n'a jamais ouvert le
//! capteur) : relevé sur `UnigramPreview`, `Photos`, `Microsoft Teams`… Un
//! `None` doit donc valoir « non », jamais « oui par défaut ».
//!
//! ⛔ **LE RELEVÉ NE TOURNE PAS SUR LA BOUCLE DE RENDU.** Mesuré le 2026-11-26
//! (`essai_reel_etat_des_capteurs`) : **2,7 ms** en régime stable — deux arbres ×
//! deux sources, une soixantaine de sous-clés ouvertes et deux valeurs lues
//! chacune. À 500 ms d'intervalle, c'est une frame perdue sur six cents ; la
//! boucle du dock sait mieux employer ses 16,7 ms. Une sonde sur son PROPRE FIL
//! relit la machine toutes les `TTL` et publie deux booléens atomiques ;
//! `usage()` ne fait plus que les lire.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use windows::Win32::System::Registry::HKEY_CURRENT_USER;

use crate::win32::registry_util;

/// Intervalle minimal entre deux relevés (voir `usage`).
pub const TTL: Duration = Duration::from_millis(500);

/// Racine des deux arbres de `ConsentStore` (voir la doc de module).
const CONSENT_STORE: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";

/// Ce que la barre du haut affiche : une case par source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Une application utilise le micro EN CE MOMENT.
    pub micro: bool,
    /// Une application utilise la caméra EN CE MOMENT.
    pub camera: bool,
}

static MICRO: AtomicBool = AtomicBool::new(false);
static CAMERA: AtomicBool = AtomicBool::new(false);
/// La sonde n'est lancée qu'une fois, au premier appel de `usage()`.
static SONDE: OnceLock<()> = OnceLock::new();

/// L'usage actuel — deux lectures d'atome, RIEN de bloquant. À appeler à chaque
/// frame depuis le dessin de la barre ; le premier appel démarre la sonde.
///
/// ⚠️ **`NEURADESK_CAPTEURS=micro,camera` FORCE L'ÉTAT**, pour una capture de
/// contrôle : allumer un vrai micro ou une vraie caméra juste pour photographier
/// deux cases serait à la fois intrusif et non reproductible. Variable de
/// DIAGNOSTIC, lue une seule fois — même famille que `NEURADESK_DEBUG_OPEN`.
pub fn usage() -> Usage {
    if let Some(force) = forcage() {
        return force;
    }
    SONDE.get_or_init(|| {
        // Fil DÉTACHÉ : il vit aussi longtemps que l'application, et personne
        // n'attend sa fin — il ne possède rien que le processus ne libère pas
        // déjà.
        std::thread::spawn(sonde);
    });
    Usage { micro: MICRO.load(Ordering::Relaxed), camera: CAMERA.load(Ordering::Relaxed) }
}

/// L'état forcé par `NEURADESK_CAPTEURS` (voir `usage`), lu une seule fois.
fn forcage() -> Option<Usage> {
    static FORCE: OnceLock<Option<Usage>> = OnceLock::new();
    *FORCE.get_or_init(|| {
        let valeur = std::env::var("NEURADESK_CAPTEURS").ok()?;
        Some(analyser(&valeur))
    })
}

/// **PURE** — `"micro,camera"` ou `"camera"`, virgules et espaces tolérés (voir
/// `forcage`).
fn analyser(valeur: &str) -> Usage {
    let sources: Vec<&str> = valeur.split(',').map(str::trim).collect();
    Usage { micro: sources.contains(&"micro"), camera: sources.contains(&"camera") }
}

/// Le fil de sonde : relève, publie, dort. Voir la doc de module pour la mesure
/// qui l'a sorti de la boucle de rendu.
fn sonde() {
    loop {
        let etat = relever();
        MICRO.store(etat.micro, Ordering::Relaxed);
        CAMERA.store(etat.camera, Ordering::Relaxed);
        std::thread::sleep(TTL);
    }
}

/// Force un relevé immédiat, sur le fil APPELANT — utilisé par les essais, et par
/// rien d'autre : le dessin passe par `usage()`.
pub fn relever() -> Usage {
    Usage { micro: source_en_usage("microphone"), camera: source_en_usage("webcam") }
}

/// Une source (`"microphone"` ou `"webcam"`) est en usage si l'une des entrées
/// de ses deux arbres le dit.
fn source_en_usage(source: &str) -> bool {
    let racine = format!("{CONSENT_STORE}\\{source}");
    [racine.clone(), format!("{racine}\\NonPackaged")].iter().any(|arbre| {
        registry_util::enumerate_subkeys(HKEY_CURRENT_USER, arbre).into_iter().any(|sous_cle| {
            let chemin = format!("{arbre}\\{sous_cle}");
            let debut = registry_util::read_qword_value(HKEY_CURRENT_USER, &chemin, "LastUsedTimeStart");
            let fin = registry_util::read_qword_value(HKEY_CURRENT_USER, &chemin, "LastUsedTimeStop");
            utilisation_active(debut, fin)
        })
    })
}

/// **PURE** — une utilisation est en cours quand elle a COMMENCÉ après sa
/// dernière fin.
///
/// ⚠️ **`fin` peut être PLUS GRANDE que `debut` sans que rien ne tourne** (une
/// session terminée) : c'est le cas normal des dizaines de sous-clés du
/// registre. Et `debut == 0` (valeur absente lue comme 0) ne doit pas passer
/// pour un début : il est antérieur à toute fin réelle.
pub fn utilisation_active(debut: Option<u64>, fin: Option<u64>) -> bool {
    matches!((debut, fin), (Some(d), Some(f)) if d != 0 && d > f)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// Une application qui a commencé APRÈS sa dernière fin utilise le capteur.
    #[test]
    fn une_utilisation_commencee_apres_la_fin_est_en_cours() {
        assert!(utilisation_active(Some(200), Some(100)));
    }

    /// Une session TERMINÉE (fin après début) n'est pas une utilisation — c'est
    /// le cas de la quasi-totalité des entrées du registre.
    #[test]
    fn une_utilisation_terminee_n_est_plus_en_cours() {
        assert!(!utilisation_active(Some(100), Some(200)));
    }

    /// Pendant l'utilisation, Windows laisse la FIN à zéro (ou à sa valeur
    /// d'avant) : un début non nul suffit alors à dire « en cours ».
    #[test]
    fn une_fin_a_zero_ne_termine_rien() {
        assert!(utilisation_active(Some(100), Some(0)));
        assert!(!utilisation_active(Some(100), None), "fin absente : rien ne prouve l'utilisation");
    }

    /// ⛔ **VALEURS ABSENTES = PAS EN USAGE** — plusieurs applications empaquetées
    /// n'ont AUCUNE des deux valeurs (relevé : UnigramPreview, Photos, Teams).
    #[test]
    fn une_application_sans_horodatage_n_utilise_rien() {
        assert!(!utilisation_active(None, None));
        assert!(!utilisation_active(None, Some(100)));
        assert!(!utilisation_active(Some(0), Some(0)), "un début à zéro n'est pas un début");
        assert!(!utilisation_active(Some(0), Some(100)));
    }

    /// L'état forcé de la capture de contrôle : une source nommée s'allume, une
    /// faute de frappe (ou une source non nommée) laisse l'autre éteinte.
    #[test]
    fn le_forcage_ne_lit_que_les_sources_nommees() {
        assert_eq!(analyser("micro,camera"), Usage { micro: true, camera: true });
        assert_eq!(analyser(" camera "), Usage { micro: false, camera: true });
        assert_eq!(analyser("microphone"), Usage { micro: false, camera: false }, "un mot inconnu n'allume rien");
        assert_eq!(analyser(""), Usage { micro: false, camera: false });
    }

    /// Relevé réel : ce que la machine répond MAINTENANT, imprimé pour qu'on
    /// puisse le confronter à ce qu'on voit, et CHRONOMÉTRÉ — c'est cette mesure
    /// qui justifie le cache `TTL` plutôt qu'une lecture par frame. Ignoré par
    /// défaut : il dépend de ce que l'utilisateur fait tourner.
    #[test]
    #[ignore = "essai réel : dépend des applications ouvertes sur la machine"]
    fn essai_reel_etat_des_capteurs() {
        let mut durees = Vec::new();
        for _ in 0..3 {
            let depart = Instant::now();
            let etat = relever();
            let duree = depart.elapsed();
            durees.push(duree);
            eprintln!(
                "[capteurs] micro={} camera={} — relevé en {:.2} ms",
                etat.micro,
                etat.camera,
                duree.as_secs_f64() * 1000.0
            );
        }
        // Aucune assertion sur le CONTENU : la machine peut être dans n'importe
        // quel état. Ce qui est asserté, c'est que le relevé ne BLOQUE pas la
        // boucle de rendu : au-delà de 4 ms (le quart d'une frame à 60 Hz), il
        // faudrait le sortir du fil principal.
        let pire = durees.iter().max().copied().unwrap_or_default();
        assert!(pire < Duration::from_millis(4), "relevé trop lent pour la boucle : {pire:?}");
    }
}
