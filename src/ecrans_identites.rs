//! **LES RÉGLAGES D'UN ÉCRAN SUIVENT LE MONITEUR, PAS SON NOM WINDOWS.**
//!
//! ⛔ **LE DÉFAUT (signalé le 2026-11-09)** : « des icônes manquantes, ne prend
//! pas en compte certains changements et réglages » sur la barre du haut de
//! l'écran secondaire. MESURÉ : le même écran physique (même position,
//! -1920,484) s'appelait `\\.\DISPLAY18` la veille et `\\.\DISPLAY2` ce jour-là.
//! Tous les réglages par écran sont rangés sous ce nom (`config.rs`, une ligne
//! par écran) : il y avait donc DEUX jeux de réglages pour un seul écran, et
//! Windows choisissait au démarrage lequel s'appliquait.
//!
//! **LE REMÈDE** : `ecrans_identites.txt` retient, pour chaque moniteur, son
//! identifiant STABLE (`monitors::identifiant_stable`, le chemin de
//! périphérique) et le dernier nom sous lequel on l'a vu. Quand ce nom change,
//! chaque fichier de `FICHIERS_PAR_ECRAN` est réécrit : les lignes de l'ancien
//! nom passent au nouveau, et celles qui portaient DÉJÀ le nouveau nom (restes
//! d'un autre écran, ou de ce même écran sous un nom antérieur) sont écartées
//! — sinon deux lignes auraient la même clé et la dernière gagnerait au hasard.
//!
//! On garde le format des fichiers (clé = nom Windows) : les ~25 fichiers et
//! leurs ~50 lecteurs/écrivains ne changent pas, seule la CLÉ est tenue à jour.
//!
//! ⚠️ **IDEMPOTENT, ET C'EST VOULU** : un renommage ne s'applique à un fichier
//! que si l'ANCIEN nom y figure encore. Si l'écriture de `ecrans_identites.txt`
//! échoue après celle des fichiers, le lancement suivant refait le même
//! renommage — et, sans cette garde, il effacerait les lignes du nouveau nom,
//! c'est-à-dire les réglages qu'il vient de déplacer.

use std::collections::HashMap;

/// Tous les fichiers de `%APPDATA%\NeuraDesk` dont chaque ligne commence par un
/// nom d'écran (`\\.\DISPLAYn`), suivi de `,`, `|`, `=` ou de rien.
///
/// ⚠️ Un nouveau réglage par écran DOIT être ajouté ici —
/// `chaque_fichier_par_ecran_de_config_est_suivi` le vérifie pour les
/// `secondary_screen*` de `config.rs`.
pub const FICHIERS_PAR_ECRAN: &[&str] = &[
    "secondary_screens.txt",
    "secondary_screen_auto_color.txt",
    "secondary_screen_colors.txt",
    "secondary_screen_desktop_dot_sizes.txt",
    "secondary_screen_features.txt",
    "secondary_screen_fullscreen_clock_bar_scale.txt",
    "secondary_screen_fullscreen_clock_enabled.txt",
    "secondary_screen_fullscreen_clock_position.txt",
    "secondary_screen_fullscreen_clock_text_scale.txt",
    "secondary_screen_icon_gaps.txt",
    "secondary_screen_icon_order.txt",
    "secondary_screen_scales.txt",
    "secondary_screen_separator_colors.txt",
    "secondary_screen_separator_sizes.txt",
    "secondary_screen_slot_widths.txt",
    "secondary_screen_spacer_sizes.txt",
    "secondary_screen_thicknesses.txt",
    "secondary_screen_tint.txt",
    "secondary_screen_topbar_text.txt",
    "secondary_screen_topbar_text_sizes.txt",
    "secondary_screen_zone_heights.txt",
    "coins_secondaires.txt",
    "coins_secondaires_rayons.txt",
    "fullscreen_clock_decalages.txt",
    "ombre_ecrans.txt",
];

/// `identifiant|nom` par ligne — l'identifiant ne contient jamais `|`.
const FICHIER_IDENTITES: &str = "ecrans_identites.txt";

/// Suffixe de la copie prise avant chaque réécriture (la dernière seulement).
const SUFFIXE_SAUVEGARDE: &str = "avant-renommage-ecran";

/// Un moniteur : (identifiant stable, nom Windows).
type Identite = (String, String);

/// Un renommage : (ancien nom, nouveau nom).
type Renommage = (String, String);

/// La clé d'une ligne : tout ce qui précède le premier `,`, `|` ou `=`.
fn cle_de_ligne(ligne: &str) -> &str {
    let ligne = ligne.trim_end_matches(['\r', '\n']);
    ligne.find([',', '|', '=']).map_or(ligne, |fin| &ligne[..fin])
}

/// Les moniteurs connectés dont le nom a changé depuis la dernière fois.
fn renommages(connus: &[Identite], connectes: &[Identite]) -> Vec<Renommage> {
    let par_id: HashMap<&str, &str> = connus.iter().map(|(id, nom)| (id.as_str(), nom.as_str())).collect();
    connectes
        .iter()
        .filter_map(|(id, nom)| {
            let ancien = par_id.get(id.as_str())?;
            (*ancien != nom).then(|| (ancien.to_string(), nom.clone()))
        })
        .collect()
}

/// Réécrit un fichier par écran ; `None` s'il n'y a rien à changer.
///
/// Seuls les renommages dont l'ANCIEN nom figure dans le fichier s'appliquent
/// (voir la doc de module, « idempotent »). Pour ceux-là, une ligne de l'ancien
/// nom prend le nouveau, et une ligne qui portait déjà le nouveau nom sans être
/// elle-même renommée est écartée. Les renommages s'appliquent ENSEMBLE : deux
/// écrans qui échangent leurs noms échangent leurs lignes.
fn renommer_contenu(contenu: &str, renommages: &[Renommage]) -> Option<String> {
    let presentes: Vec<&str> = contenu.split_inclusive('\n').map(cle_de_ligne).collect();
    let actifs: Vec<&Renommage> = renommages.iter().filter(|(ancien, _)| presentes.contains(&ancien.as_str())).collect();
    if actifs.is_empty() {
        return None;
    }
    let mut sortie = String::with_capacity(contenu.len());
    for ligne in contenu.split_inclusive('\n') {
        let cle = cle_de_ligne(ligne);
        if let Some((_, nouveau)) = actifs.iter().find(|(ancien, _)| ancien == cle) {
            sortie.push_str(nouveau);
            sortie.push_str(&ligne[cle.len()..]);
        } else if !actifs.iter().any(|(_, nouveau)| nouveau == cle) {
            sortie.push_str(ligne);
        }
    }
    Some(sortie)
}

/// Ce que `ecrans_identites.txt` doit retenir désormais : les moniteurs
/// connectés, plus les absents dont le nom n'a pas été repris par un connecté
/// (leurs lignes viennent d'être écartées, ou appartiennent maintenant à un
/// autre moniteur : les suivre ensuite déplacerait les réglages d'un autre).
fn identites_apres(connus: &[Identite], connectes: &[Identite]) -> Vec<Identite> {
    let mut retenues: Vec<Identite> = connectes.to_vec();
    for (id, nom) in connus {
        let deja = connectes.iter().any(|(autre_id, autre_nom)| autre_id == id || autre_nom == nom);
        if !deja {
            retenues.push((id.clone(), nom.clone()));
        }
    }
    retenues
}

fn analyser_identites(contenu: &str) -> Vec<Identite> {
    contenu
        .lines()
        .filter_map(|ligne| {
            let (id, nom) = ligne.trim().split_once('|')?;
            (!id.is_empty() && !nom.is_empty()).then(|| (id.to_string(), nom.to_string()))
        })
        .collect()
}

fn formater_identites(identites: &[Identite]) -> String {
    identites.iter().map(|(id, nom)| format!("{id}|{nom}\n")).collect()
}

/// **AU DÉMARRAGE, AVANT TOUTE LECTURE D'UN RÉGLAGE PAR ÉCRAN** — et après un
/// changement d'écrans (`app::run`). Rend `true` si des fichiers ont été
/// réécrits : les réglages déjà chargés en mémoire sont alors périmés.
pub fn suivre_les_renommages() -> bool {
    let connectes: Vec<Identite> = crate::win32::monitors::enumerate()
        .into_iter()
        .filter_map(|ecran| Some((crate::win32::monitors::identifiant_stable(&ecran.device_name)?, ecran.device_name)))
        .collect();
    if connectes.is_empty() {
        return false;
    }
    let Some(chemin_identites) = crate::config::config_path(FICHIER_IDENTITES) else { return false };
    let lu = std::fs::read_to_string(&chemin_identites).unwrap_or_default();
    let connus = analyser_identites(&lu);
    let a_renommer = renommages(&connus, &connectes);
    let reecrits = reecrire_les_fichiers(&a_renommer);

    let apres = formater_identites(&identites_apres(&connus, &connectes));
    if apres != lu {
        if let Err(e) = std::fs::write(&chemin_identites, &apres) {
            crate::diag!("[ecrans] {FICHIER_IDENTITES} NON ecrit : {e}");
        }
    }
    reecrits > 0
}

/// Rend le nombre de fichiers réécrits.
fn reecrire_les_fichiers(a_renommer: &[Renommage]) -> usize {
    if a_renommer.is_empty() {
        return 0;
    }
    crate::diag!("[ecrans] renommages detectes : {a_renommer:?}");
    let mut reecrits = 0;
    for fichier in FICHIERS_PAR_ECRAN {
        let Some(chemin) = crate::config::config_path(fichier) else { continue };
        let Ok(contenu) = std::fs::read_to_string(&chemin) else { continue };
        let Some(nouveau) = renommer_contenu(&contenu, a_renommer) else { continue };
        let copie = chemin.with_file_name(format!("{fichier}.{SUFFIXE_SAUVEGARDE}"));
        if let Err(e) = std::fs::copy(&chemin, &copie) {
            crate::diag!("[ecrans] {fichier} : copie impossible ({e}), fichier NON reecrit");
            continue;
        }
        match std::fs::write(&chemin, nouveau) {
            Ok(()) => reecrits += 1,
            Err(e) => crate::diag!("[ecrans] {fichier} NON reecrit : {e}"),
        }
    }
    crate::diag!("[ecrans] {reecrits} fichier(s) reecrit(s)");
    reecrits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(i: &str, n: &str) -> Identite {
        (i.to_string(), n.to_string())
    }

    fn r(a: &str, n: &str) -> Renommage {
        (a.to_string(), n.to_string())
    }

    const D2: &str = r"\\.\DISPLAY2";
    const D18: &str = r"\\.\DISPLAY18";
    const D3: &str = r"\\.\DISPLAY3";

    #[test]
    fn la_cle_s_arrete_au_premier_separateur_ou_a_la_fin_de_ligne() {
        assert_eq!(cle_de_ligne("\\\\.\\DISPLAY2,24\n"), D2);
        assert_eq!(cle_de_ligne("\\\\.\\DISPLAY2|1,2||0|41"), D2);
        assert_eq!(cle_de_ligne("\\\\.\\DISPLAY2=0,66\r\n"), D2);
        assert_eq!(cle_de_ligne("\\\\.\\DISPLAY2\r\n"), D2);
    }

    #[test]
    fn un_moniteur_qui_change_de_nom_donne_un_renommage() {
        let connus = [id("SAM092D", D18), id("SAM7232", r"\\.\DISPLAY1")];
        let connectes = [id("SAM092D", D2), id("SAM7232", r"\\.\DISPLAY1")];
        assert_eq!(renommages(&connus, &connectes), vec![r(D18, D2)]);
    }

    #[test]
    fn un_moniteur_inconnu_ne_renomme_rien() {
        assert!(renommages(&[], &[id("SAM092D", D2)]).is_empty());
    }

    /// Le cas MESURÉ le 2026-11-09 : les deux noms ont chacun leurs lignes, et
    /// celles de l'ancien nom (les réglages suivis) l'emportent.
    #[test]
    fn l_ancien_nom_prend_la_place_et_les_restes_du_nouveau_sont_ecartes() {
        let contenu = format!("{D2},29,48,48\n{D18},30,0,0\n");
        let sortie = renommer_contenu(&contenu, &[r(D18, D2)]).expect("a reecrire");
        assert_eq!(sortie, format!("{D2},30,0,0\n"));
    }

    #[test]
    fn deux_ecrans_qui_echangent_leurs_noms_echangent_leurs_lignes() {
        let contenu = format!("{D2}|a\n{D3}|b\n");
        let sortie = renommer_contenu(&contenu, &[r(D2, D3), r(D3, D2)]).expect("a reecrire");
        assert_eq!(sortie, format!("{D3}|a\n{D2}|b\n"));
    }

    /// ⛔ Sans la garde « l'ancien nom doit figurer », rejouer le renommage
    /// (identités non écrites) effacerait les réglages qu'il vient de déplacer.
    #[test]
    fn rejouer_un_renommage_deja_fait_ne_touche_a_rien() {
        let deja_fait = format!("{D2},30,0,0\n");
        assert_eq!(renommer_contenu(&deja_fait, &[r(D18, D2)]), None);
    }

    #[test]
    fn les_autres_lignes_et_la_fin_de_fichier_sont_gardees_telles_quelles() {
        let contenu = format!("{D3}=17\r\n{D18}=24");
        let sortie = renommer_contenu(&contenu, &[r(D18, D2)]).expect("a reecrire");
        assert_eq!(sortie, format!("{D3}=17\r\n{D2}=24"));
    }

    #[test]
    fn un_absent_dont_le_nom_est_repris_par_un_connecte_est_oublie() {
        let connus = [id("SAM092D", D18), id("ANCIEN", D2), id("LOIN", D3)];
        let connectes = [id("SAM092D", D2)];
        assert_eq!(identites_apres(&connus, &connectes), vec![id("SAM092D", D2), id("LOIN", D3)]);
    }

    #[test]
    fn le_fichier_d_identites_se_relit_tel_qu_ecrit() {
        let identites = vec![id(r"\\?\DISPLAY#SAM092D#5&5bcff4c&2&UID4352#{e6f07b5f}", D2)];
        assert_eq!(analyser_identites(&formater_identites(&identites)), identites);
    }

    /// Un fichier `secondary_screen*` que `config.rs` écrit sans qu'il soit suivi
    /// ici garderait l'ancien nom après un renommage — le défaut de départ.
    #[test]
    fn chaque_fichier_par_ecran_de_config_est_suivi() {
        let source = include_str!("config.rs");
        let mut reste = source;
        while let Some(debut) = reste.find("\"secondary_screen") {
            let apres = &reste[debut + 1..];
            let fin = apres.find('"').expect("guillemet fermant");
            let nom = &apres[..fin];
            if nom.ends_with(".txt") {
                assert!(FICHIERS_PAR_ECRAN.contains(&nom), "{nom} n'est pas dans FICHIERS_PAR_ECRAN");
            }
            reste = &apres[fin..];
        }
    }
}
