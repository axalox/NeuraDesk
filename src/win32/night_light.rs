//! **L'ÉCLAIRAGE NOCTURNE DE WINDOWS — ET IL N'A AUCUNE API PUBLIQUE.**
//!
//! DEMANDE EXPLICITE : « une applet pour commuter le mode nuit de Windows ». Il
//! s'agit de l'ÉCLAIRAGE NOCTURNE (le filtre bleu), pas du thème sombre : celui-ci a
//! déjà sa bascule (`win32::theme`), et confondre les deux aurait donné une applet
//! qui fait deux fois la même chose.
//!
//! **CE QU'ON ÉCRIT, ET POURQUOI CE N'EST PAS UN RÉGLAGE COMME UN AUTRE.** Windows
//! range cet état dans **CloudStore**, une couche de persistance NON DOCUMENTÉE qui
//! sérialise ses réglages en blobs **Bond CompactBinary v1** — et il n'existe ni API
//! ni valeur de registre nommée pour l'allumer. Le seul chemin connu est de relire le
//! blob et d'en réécrire la forme exacte :
//!
//! * **l'octet 18 porte l'état** : `0x15` allumé, `0x13` éteint ;
//! * **la LONGUEUR change** : éteindre RETIRE deux octets (à partir de l'index 23),
//!   allumer les AJOUTE (`0x10`, `0x00`) ;
//! * et un **horodatage** (le premier octet de `[10, 15)` qui ne vaut pas `0xff`)
//!   est incrémenté, sans quoi Windows peut ignorer l'écriture.
//!
//! ⚠️ **LE PIÈGE QUI FAIT MAL, ET C'EST CELUI QUI IMPOSE DE COPIER LA FORME PLUTÔT
//! QUE D'ÉCRIRE UN MOTIF À SOI** : écrire un blob dégénéré (des zéros) éteint bien
//! l'éclairage, mais **fige la fenêtre des Paramètres d'affichage** — mesuré par la
//! communauté, et signalé dans les réponses Microsoft Q&A. Le blob doit RESTER
//! COHÉRENT : on ne remplace donc que ce que la bascule change, et tout le reste est
//! recopié tel quel.
//!
//! **VÉRIFIÉ PAR RECOUPEMENT, PAS PAR UNE SOURCE UNIQUE** : trois implémentations
//! indépendantes concordent sur ce format — `kvnxiao/win-nightlight-cli` (le plus
//! détaillé), `nathanbabcock/nightlight-cli` (TypeScript) et `Maclay74/tiny-screen`
//! (`NightLight.cs`, C#). Voir la fiche `neuradesk-mode-nuit-recette-registre`.
//!
//! ⚠️ **AUCUN TEST N'ÉCRIT DANS LE REGISTRE**, et c'est délibéré, exactement comme
//! pour `theme::set_dark_mode` : basculer cet état **change l'écran de
//! l'utilisateur**. Seule la fonction PURE `basculer` est vérifiable — c'est elle qui
//! porte tout le risque de se tromper, et elle est testée sur des blobs SYNTHÉTIQUES.

use windows::Win32::System::Registry::HKEY_CURRENT_USER;

/// **LE CHEMIN EXACT — DEUX NIVEAUX, ET C'EST LE PIÈGE QUI A COÛTÉ L'APPLETTE.**
///
/// ⚠️ **MESURÉ le 2026-09-23, après le signalement « l'applet lune ne produit aucun
/// effet ».** Le chemin ci-dessus s'arrêtait au PREMIER niveau, et la valeur
/// cherchée s'appelait `windows.data.…state` — or c'est le nom de la SOUS-CLÉ, pas
/// celui de la valeur. La structure réelle, énumérée :
///
/// ```text
/// CLE : default$windows.data.bluelightreduction.bluelightreductionstate
///   valeurs ICI : (aucune)
///   SOUS-CLE : windows.data.bluelightreduction.bluelightreductionstate
///     valeurs : Data
/// ```
///
/// Conséquence : `read_binary_value` cherchait une valeur inexistante dans une clé
/// qui n'en porte aucune, rendait `None`, et `toggle` sortait sur son `?` — donc
/// **un refus parfaitement silencieux à l'écran** (« l'applet ne produit aucun
/// effet »), et six lignes « REFUSEE » au journal pour six clics.
///
/// **LA CAUSE DE L'ERREUR EST LISIBLE** : la recette d'origine écrivait le chemin
/// d'un trait, `default$…state\windows.data.…state`, ce qui se lit exactement comme
/// `clé\valeur`. C'est `clé\SOUS-CLÉ` et la valeur s'appelle `Data`. Un chemin de
/// registre ne se recopie pas depuis une phrase : il s'énumère.
const ETAT_CLE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\CloudStore\\Store\\DefaultAccount\\Current\\default$windows.data.bluelightreduction.bluelightreductionstate\\windows.data.bluelightreduction.bluelightreductionstate";

/// Le nom de la valeur, sous la clé ci-dessus — **`Data`, et rien d'autre**.
///
/// Les quatre clés `bluelightreduction` portent TOUTES une unique valeur de ce nom
/// (état, réglages, et leurs variantes `perdevice`), ce qui a fait prendre le nom de
/// la sous-clé pour celui de la valeur.
const ETAT_VALEUR: &str = "Data";

/// L'octet qui porte l'état, et ses deux valeurs. Mesuré (voir la doc de module).
const OCTET_ETAT: usize = 18;
const ALLUME: u8 = 0x15;
const ETEINT: u8 = 0x13;

/// **LE POINT D'INSERTION DES DEUX OCTETS QUE L'ALLUMAGE AJOUTE** — et donc celui
/// qu'on retire pour éteindre. Au-delà de cet index, tout est recopié tel quel.
const POINT: usize = 23;

/// Blob trop court pour porter un état : on REFUSE plutôt que de deviner.
const LONGUEUR_MINIMALE: usize = POINT + 2;

/// **L'ÉTAT COURANT, LU DANS LE BLOB — `None` si la forme est inattendue.**
///
/// Sert à la fois à savoir dans QUEL SENS basculer et à ne rien écrire du tout
/// quand on ne reconnaît pas ce qu'on lit : un blob d'une autre forme (une version
/// de Windows qui change de format) doit se SOLDER par un échec visible, jamais par
/// une écriture qui casserait un réglage système.
pub fn est_allume(data: &[u8]) -> Option<bool> {
    match *data.get(OCTET_ETAT)? {
        ALLUME => Some(true),
        ETEINT => Some(false),
        _ => None,
    }
}

/// **LA BASCULE, PURE — `None` = « je ne reconnais pas ce blob, je n'y touche pas ».**
///
/// Elle ne fait que ce que la bascule exige, et **recopie tout le reste** :
/// * l'octet d'état, à l'index 18 ;
/// * les deux octets que l'allumage ajoute (`0x10`, `0x00`) ou que l'extinction
///   retire ;
/// * l'horodatage, incrémenté — sans quoi Windows peut ignorer l'écriture.
///
/// ⚠️ **C'EST L'ÉTAT COURANT QUI DÉCIDE DU SENS**, jamais un état tenu de notre
/// côté : Windows fait foi et persiste lui-même ce réglage (même doctrine que
/// `theme::toggle_dark_mode`, qui relit au lieu de mémoriser). Un état gardé ici
/// finirait par diverger d'un changement fait dans les Paramètres, et le clic
/// basculerait alors dans le mauvais sens.
pub fn basculer(data: &[u8]) -> Option<Vec<u8>> {
    let allume = est_allume(data)?;
    if data.len() < LONGUEUR_MINIMALE {
        return None;
    }
    let mut sortie = Vec::with_capacity(data.len() + 2);
    if allume {
        // ÉTEINDRE : on retire les deux octets de l'allumage.
        sortie.extend_from_slice(&data[..POINT]);
        sortie.extend_from_slice(&data[POINT + 2..]);
        sortie[OCTET_ETAT] = ETEINT;
    } else {
        // ALLUMER : on les insère — `0x10`, `0x00`, dans cet ordre.
        sortie.extend_from_slice(&data[..POINT]);
        sortie.push(0x10);
        sortie.push(0x00);
        sortie.extend_from_slice(&data[POINT..]);
        sortie[OCTET_ETAT] = ALLUME;
    }
    incrementer_horodatage(&mut sortie);
    Some(sortie)
}

/// **INCRÉMENTE L'HORODATAGE — le premier octet de `[10, 15)` qui ne vaut pas
/// `0xff`.**
///
/// C'est ce que font les trois implémentations de référence, et ce n'est pas
/// cosmétique : c'est une estampille de modification, et Windows peut ignorer une
/// valeur dont il croit qu'elle n'a pas changé.
///
/// **On s'arrête au PREMIER octet incrémentable**, sans propagation de retenue —
/// les octets ne s'enchaînent pas comme un entier, et propager la retenue
/// modifierait des octets que la bascule n'a pas à toucher.
fn incrementer_horodatage(data: &mut [u8]) {
    for octet in data.iter_mut().take(15).skip(10) {
        if *octet != 0xff {
            *octet += 1;
            return;
        }
    }
}

/// **BASCULE L'ÉCLAIRAGE NOCTURNE.**
///
/// Rend le NOUVEL état (`None` si rien n'a pu être fait : blob illisible, forme
/// inattendue, ou écriture refusée). L'appelant journalise — et ne peint rien.
///
/// ⚠️ **AUCUN TEST N'APPELLE CETTE FONCTION** (voir la doc de module) : elle change
/// l'écran de l'utilisateur pour de vrai. Elle n'est atteignable que par le CLIC sur
/// l'applet, ou par une automatisation (voir [`set_allume`]).
pub fn toggle() -> Option<bool> {
    regler(None)
}

/// **MET L'ÉCLAIRAGE DANS UN ÉTAT DONNÉ — et n'écrit RIEN s'il y est déjà.**
///
/// Ajouté pour les automatisations (« quand il est 22 h, alors allumer
/// l'éclairage nocturne ») : `toggle` ne sait que basculer, et basculer deux fois
/// pour atteindre un état ferait un aller-retour **visible à l'écran**.
///
/// ⚠️ L'état courant est celui que Windows rapporte, jamais un état tenu de notre
/// côté — même raison que [`toggle`] (voir la doc de [`basculer`]).
pub fn set_allume(allume: bool) -> Option<bool> {
    regler(Some(allume))
}

/// **LE SEUL CHEMIN QUI ÉCRIT** : `None` = basculer, `Some(état)` = y aller.
fn regler(cible: Option<bool>) -> Option<bool> {
    let data = super::registry_util::read_binary_value(
        HKEY_CURRENT_USER,
        ETAT_CLE,
        ETAT_VALEUR,
    )?;
    let actuel = est_allume(&data)?;
    if cible == Some(actuel) {
        // Déjà dans l'état voulu : on ne touche NI au registre NI au système.
        return Some(actuel);
    }
    let nouveau = basculer(&data)?;
    if !super::registry_util::write_binary_value(HKEY_CURRENT_USER, ETAT_CLE, ETAT_VALEUR, &nouveau) {
        return None;
    }
    // **PRÉVENIR LE SYSTÈME, SINON LE CLIC SEMBLE N'AVOIR RIEN FAIT.** On réemploie
    // le message de la bascule de thème (`ImmersiveColorSet`) : c'est celui que les
    // implémentations de référence envoient après une bascule d'éclairage, et
    // Windows n'en documente aucun pour ce réglage. ⚠️ **Ce n'est PAS vérifié par
    // une mesure ici** — un message mal choisi laisserait l'écran inchangé jusqu'au
    // prochain rafraîchissement.
    super::theme::broadcast_theme_change();
    Some(!actuel)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **UN BLOB SYNTHÉTIQUE DE LA FORME RÉELLE** — jamais le vrai : on ne LIT pas
    /// le registre de l'utilisateur dans un test, et on n'y écrit pas davantage.
    ///
    /// La forme imitée est celle que décrivent les trois implémentations : un
    /// en-tête `43 42 01 00`, un horodatage dans `[10, 15)`, et l'octet d'état en 18.
    fn blob(allume: bool) -> Vec<u8> {
        let mut data = vec![0u8; if allume { 43 } else { 41 }];
        data[0..4].copy_from_slice(&[0x43, 0x42, 0x01, 0x00]);
        data[10] = 0x2a; // l'horodatage
        data[OCTET_ETAT] = if allume { ALLUME } else { ETEINT };
        if allume {
            data[POINT] = 0x10;
            data[POINT + 1] = 0x00;
        }
        data
    }

    #[test]
    fn the_state_byte_says_what_windows_shows() {
        assert_eq!(est_allume(&blob(true)), Some(true));
        assert_eq!(est_allume(&blob(false)), Some(false));
    }

    /// **ÉTEINDRE RETIRE DEUX OCTETS, ALLUMER LES AJOUTE** — c'est la moitié du
    /// format qui ne se devine pas, et celle qui casse le réglage si on l'oublie.
    #[test]
    fn toggling_changes_the_length_the_way_windows_expects() {
        let allume = blob(true);
        let eteint = basculer(&allume).expect("un blob connu se bascule");
        assert_eq!(eteint.len(), 41, "éteindre retire deux octets");
        assert_eq!(eteint[OCTET_ETAT], ETEINT);

        let rallume = basculer(&eteint).expect("et l'inverse aussi");
        assert_eq!(rallume.len(), 43, "allumer les remet");
        assert_eq!(rallume[OCTET_ETAT], ALLUME);
        assert_eq!(&rallume[POINT..POINT + 2], &[0x10, 0x00], "les deux octets ajoutés, dans l'ordre");
    }

    /// **TOUT LE RESTE EST RECOPIÉ** — c'est ce qui garde le blob cohérent, et donc
    /// ce qui évite de figer la fenêtre des Paramètres d'affichage (voir la doc de
    /// module). Un octet perdu au passage serait invisible jusqu'au jour où Windows
    /// relirait ce réglage.
    #[test]
    fn everything_else_survives_the_round_trip() {
        let avant = blob(true);
        let apres = basculer(&basculer(&avant).expect("aller")).expect("retour");
        assert_eq!(apres.len(), avant.len(), "la longueur revient");
        assert_eq!(&apres[0..10], &avant[0..10], "l'en-tête est intact");
        assert_eq!(&apres[19..POINT], &avant[19..POINT], "le corps est intact");
    }

    /// **L'HORODATAGE AVANCE**, et il avance d'un seul cran : sans lui, Windows peut
    /// ignorer l'écriture ; en propageant une retenue, on toucherait des octets que
    /// la bascule n'a pas à modifier.
    #[test]
    fn the_timestamp_moves_by_exactly_one() {
        let mut avant = blob(false);
        avant[10] = 0x2a;
        let apres = basculer(&avant).expect("un blob connu se bascule");
        assert_eq!(apres[10], 0x2b, "le premier octet de [10, 15) est incrémenté");
        assert_eq!(apres[11], avant[11], "et PAS les suivants : aucune retenue ne se propage");

        // Un octet à `0xff` est sauté — c'est la règle des trois implémentations.
        let mut plein = blob(false);
        plein[10] = 0xff;
        plein[11] = 0x05;
        let apres = basculer(&plein).expect("un blob connu se bascule");
        assert_eq!(apres[10], 0xff, "un octet plein est laissé tel quel");
        assert_eq!(apres[11], 0x06, "et c'est le suivant qui avance");
    }

    /// **LA SONDE QUI AURAIT ATTRAPÉ LE DÉFAUT DU 2026-09-23.** `#[ignore]` parce
    /// qu'elle lit le **registre RÉEL** de la machine — même convention que les
    /// sondes de `win32::tray_probe`, qui ne tournent pas avec la suite.
    ///
    /// Elle vérifie la seule chose que des blobs SYNTHÉTIQUES ne peuvent pas voir :
    /// **que le chemin de clé et le nom de valeur DÉSIGNENT quelque chose.** Le
    /// défaut d'origine était exactement cela — `ETAT_CLE` s'arrêtait au premier
    /// niveau, `ETAT_VALEUR` portait le nom de la SOUS-CLÉ, la lecture rendait
    /// `None` et la bascule refusait en silence. Aucun test sur blob synthétique ne
    /// peut le révéler : il n'y a pas de blob du tout quand la clé n'est pas la
    /// bonne, et c'est précisément pour cela que la suite entière restait verte.
    ///
    /// Lancer : `cargo test --bins -- --ignored --nocapture la_cle_et_la_valeur`
    #[test]
    #[ignore = "lit le registre REEL de la machine — sonde, jamais test de suite"]
    fn la_cle_et_la_valeur_de_l_eclairage_nocturne_designent_quelque_chose() {
        let data = crate::win32::registry_util::read_binary_value(HKEY_CURRENT_USER, ETAT_CLE, ETAT_VALEUR)
            .expect("la cle et le nom de valeur doivent designer la valeur `Data` reelle");
        println!(
            "blob lu : {} octets, en-tete {:02x?}, octet {OCTET_ETAT} = {:#04x}",
            data.len(),
            &data[..4.min(data.len())],
            data.get(OCTET_ETAT).copied().unwrap_or(0)
        );
        assert!(
            est_allume(&data).is_some(),
            "l'octet d'etat ({OCTET_ETAT}) doit valoir {ALLUME:#04x} (allume) ou {ETEINT:#04x} (eteint), lu {:#04x}",
            data.get(OCTET_ETAT).copied().unwrap_or(0)
        );
    }

    /// **UN BLOB QU'ON NE RECONNAÎT PAS SE REFUSE, IL NE SE DEVINE PAS.** C'est la
    /// garde qui protège le réglage système : une version de Windows qui changerait
    /// de format doit produire un échec VISIBLE, jamais une écriture approximative.
    #[test]
    fn an_unrecognised_blob_is_refused() {
        assert_eq!(basculer(&[]), None, "vide");
        assert_eq!(basculer(&[0u8; 10]), None, "trop court pour porter un état");
        let mut inconnu = blob(false);
        inconnu[OCTET_ETAT] = 0x00;
        assert_eq!(est_allume(&inconnu), None, "un octet d'état inconnu n'est pas « éteint »");
        assert_eq!(basculer(&inconnu), None, "et il ne se bascule pas");
    }
}
