//! **LES KAOMOJIS PERSONNELS** — les seuls que l'éditeur de `cef_kaomoji` écrit.
//!
//! La table de `win32::kaomoji` est un `const` COMPILÉ : elle ne bouge pas. Ce
//! module tient la liste que l'utilisateur ajoute, dans
//! `%APPDATA%\NeuraDesk\kaomojis.json`, à côté du reste de sa configuration.
//!
//! ⭐ **DES IDENTIFIANTS STABLES, PAS DES INDEX.** Une entrée personnelle voyage
//! par son `id` : ajouter ou supprimer une entrée décale les index de toutes
//! celles qui suivent, et un clic parti sur l'ancien état frapperait le mauvais
//! kaomoji. Les intégrés gardent leur index — leur table ne change jamais.
//!
//! ⛔ **UN IDENTIFIANT N'EST JAMAIS RÉUTILISÉ** : le fichier porte `prochain_id`,
//! un compteur monotone, et non « le plus grand + 1 ». Sans ça, supprimer la
//! dernière entrée puis en ajouter une autre redonnerait son id à la nouvelle.
//!
//! ⚠️ **LE FICHIER EST DE LA DONNÉE UTILISATEUR, DONC LU AVEC TOLÉRANCE** : une
//! entrée illisible est écartée en silence, jamais fatale. L'écriture passe par un
//! fichier temporaire puis `rename` — une coupure ne doit pas laisser un JSON
//! tronqué à la place de la liste.
//!
//! ⛔ **AUCUN TEST N'ÉCRIT DANS `%APPDATA%`** : sous `cfg(test)`, `charger` rend
//! une liste VIDE et `ecrire` refuse, donc la géométrie et l'état sont
//! déterministes. Les mutations sont testées sur les fonctions PURES `*_dans`.

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;

/// Le nom du fichier, dans `%APPDATA%\NeuraDesk\`.
const FICHIER: &str = "kaomojis.json";

/// La longueur maximale d'un kaomoji personnel, en caractères. Les plus longs de
/// la table intégrée font une vingtaine d'unités ; au-delà de 80 la case
/// déborderait de la fenêtre, même plafonnée.
pub(crate) const LONGUEUR_MAX: usize = 80;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KaomojiPersonnel {
    pub id: u32,
    pub texte: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Fichier {
    prochain_id: u32,
    kaomojis: Vec<KaomojiPersonnel>,
}

thread_local! {
    /// Chargé une fois, puis tenu à jour par les mutations — pas de lecture
    /// disque à chaque frame.
    static CACHE: RefCell<Option<Fichier>> = const { RefCell::new(None) };
}

fn chemin() -> Option<PathBuf> {
    crate::config::config_path(FICHIER)
}

/// **VALIDE ET NETTOIE UN TEXTE** — la seule porte d'entrée des trois mutations.
///
/// ⚠️ **ON NETTOIE AUX DEUX BOUTS, ON NE REFUSE PAS L'ESPACE INTÉRIEUR** : un
/// kaomoji comme `( ˘ ³˘)♥` a des espaces qui font partie de son dessin.
pub(crate) fn valider(texte: &str) -> Result<String, &'static str> {
    let texte = texte.trim();
    if texte.is_empty() {
        return Err("vide");
    }
    if texte.chars().count() > LONGUEUR_MAX {
        return Err("trop long");
    }
    // ⛔ Un saut de ligne ou une tabulation casserait la case et le fichier.
    if texte.chars().any(char::is_control) {
        return Err("caractère de contrôle");
    }
    Ok(texte.to_string())
}

/// **ANALYSE LE FICHIER, SANS ACCÈS DISQUE** — pure, donc testable.
///
/// Une entrée sans `id` ou `texte`, un texte invalide, ou un doublon (id ou
/// texte) est ÉCARTÉ : le fichier est une donnée utilisateur, pas une source de
/// vérité qu'on a le droit de faire échouer.
fn analyser(contenu: &str) -> Fichier {
    let Ok(valeur) = serde_json::from_str::<serde_json::Value>(contenu) else {
        return Fichier::default();
    };
    let mut prochain_id = valeur.get("prochain_id").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
    let mut kaomojis = Vec::new();
    let mut plus_grand = 0u32;
    if let Some(entrees) = valeur.get("kaomojis").and_then(|v| v.as_array()) {
        for entree in entrees {
            let Some(id) = entree.get("id").and_then(|v| v.as_u64()) else { continue };
            let Some(texte) = entree.get("texte").and_then(|v| v.as_str()) else { continue };
            let Ok(texte) = valider(texte) else { continue };
            let id = id.min(u32::MAX as u64) as u32;
            if id == 0 || kaomojis.iter().any(|k: &KaomojiPersonnel| k.id == id || k.texte == texte) {
                continue;
            }
            plus_grand = plus_grand.max(id);
            kaomojis.push(KaomojiPersonnel { id, texte });
        }
    }
    // ⚠️ Un `prochain_id` absent ou inférieur au plus grand id lu est RÉPARÉ :
    // sinon la prochaine addition écraserait une entrée existante.
    prochain_id = prochain_id.max(plus_grand + 1).max(1);
    Fichier { prochain_id, kaomojis }
}

/// **SÉRIALISE LE FICHIER** — pure, jumelle de `analyser`.
fn serialiser(fichier: &Fichier) -> String {
    let kaomojis: Vec<serde_json::Value> =
        fichier.kaomojis.iter().map(|k| serde_json::json!({ "id": k.id, "texte": k.texte.as_str() })).collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "prochain_id": fichier.prochain_id,
        "kaomojis": kaomojis,
    }))
    .unwrap_or_else(|_| "{\"prochain_id\":1,\"kaomojis\":[]}".to_string())
}

#[cfg(not(test))]
fn charger() -> Fichier {
    chemin()
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|contenu| analyser(&contenu))
        .unwrap_or_default()
}

/// ⛔ **VIDE SOUS `cargo test`** : la géométrie et l'état doivent être
/// déterministes, et aucun test ne doit lire la configuration réelle de la
/// machine (voir la doc de module).
#[cfg(test)]
fn charger() -> Fichier {
    Fichier::default()
}

#[cfg(not(test))]
fn ecrire(fichier: &Fichier) -> bool {
    let Some(chemin) = chemin() else { return false };
    if let Some(dossier) = chemin.parent() {
        let _ = fs::create_dir_all(dossier);
    }
    let temporaire = chemin.with_extension("json.tmp");
    if fs::write(&temporaire, serialiser(fichier)).is_err() {
        return false;
    }
    if fs::rename(&temporaire, &chemin).is_err() {
        let _ = fs::remove_file(&temporaire);
        return false;
    }
    true
}

#[cfg(test)]
fn ecrire(_fichier: &Fichier) -> bool {
    false
}

/// La liste courante, dans l'ordre d'ajout. Rend une copie : l'appelant ne doit
/// pas tenir le cache pendant qu'il construit l'état JSON.
pub(crate) fn tous() -> Vec<KaomojiPersonnel> {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.get_or_insert_with(charger).kaomojis.clone()
    })
}

/// **AJOUTE** — rend l'id attribué, ou `None` si le texte est invalide, déjà
/// présent, ou si le disque a refusé l'écriture.
pub(crate) fn ajouter(texte: &str) -> Option<u32> {
    let mut fichier = tous_fichier();
    let id = ajouter_dans(&mut fichier, texte)?;
    if !ecrire(&fichier) {
        return None;
    }
    retenir(fichier);
    Some(id)
}

/// **MODIFIE** — `false` si l'id est inconnu, le texte invalide ou déjà pris.
pub(crate) fn modifier(id: u32, texte: &str) -> bool {
    let mut fichier = tous_fichier();
    if !modifier_dans(&mut fichier, id, texte) {
        return false;
    }
    if !ecrire(&fichier) {
        return false;
    }
    retenir(fichier);
    true
}

/// **SUPPRIME** — `false` si l'id est inconnu.
pub(crate) fn supprimer(id: u32) -> bool {
    let mut fichier = tous_fichier();
    if !supprimer_dans(&mut fichier, id) {
        return false;
    }
    if !ecrire(&fichier) {
        return false;
    }
    retenir(fichier);
    true
}

fn tous_fichier() -> Fichier {
    CACHE.with(|cache| cache.borrow().clone()).unwrap_or_else(charger)
}

fn retenir(fichier: Fichier) {
    CACHE.with(|cache| *cache.borrow_mut() = Some(fichier));
}

/// **LES TROIS MUTATIONS, PURES** — testables sans `%APPDATA%`.
fn ajouter_dans(fichier: &mut Fichier, texte: &str) -> Option<u32> {
    let texte = valider(texte).ok()?;
    if fichier.kaomojis.iter().any(|k| k.texte == texte) {
        return None;
    }
    let id = fichier.prochain_id.max(1);
    fichier.prochain_id = id + 1;
    fichier.kaomojis.push(KaomojiPersonnel { id, texte });
    Some(id)
}

fn modifier_dans(fichier: &mut Fichier, id: u32, texte: &str) -> bool {
    let Ok(texte) = valider(texte) else { return false };
    if fichier.kaomojis.iter().any(|k| k.id != id && k.texte == texte) {
        return false;
    }
    let Some(entree) = fichier.kaomojis.iter_mut().find(|k| k.id == id) else {
        return false;
    };
    entree.texte = texte;
    true
}

fn supprimer_dans(fichier: &mut Fichier, id: u32) -> bool {
    let avant = fichier.kaomojis.len();
    fichier.kaomojis.retain(|k| k.id != id);
    fichier.kaomojis.len() != avant
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_texte_est_nettoie_aux_deux_bouts_et_l_interieur_est_garde() {
        assert_eq!(valider("  ( ˘ ³˘)♥  "), Ok("( ˘ ³˘)♥".to_string()));
    }

    #[test]
    fn un_texte_vide_ou_de_controle_est_refuse() {
        assert!(valider("   ").is_err());
        assert!(valider("a\nb").is_err());
        assert!(valider("a\tb").is_err());
    }

    #[test]
    fn un_texte_trop_long_est_refuse() {
        let long = "x".repeat(LONGUEUR_MAX + 1);
        assert!(valider(&long).is_err());
        let juste = "x".repeat(LONGUEUR_MAX);
        assert!(valider(&juste).is_ok());
    }

    #[test]
    fn l_aller_retour_du_fichier_garde_ids_et_ordre() {
        let mut fichier = Fichier { prochain_id: 1, kaomojis: Vec::new() };
        let a = ajouter_dans(&mut fichier, "(◕‿◕)").unwrap();
        let b = ajouter_dans(&mut fichier, "¯\\_(ツ)_/¯").unwrap();
        let relu = analyser(&serialiser(&fichier));
        assert_eq!(relu, fichier);
        assert_eq!(relu.kaomojis.iter().map(|k| k.id).collect::<Vec<_>>(), [a, b]);
    }

    #[test]
    fn un_doublon_est_refuse_sans_consommer_d_id() {
        let mut fichier = Fichier { prochain_id: 7, kaomojis: Vec::new() };
        assert_eq!(ajouter_dans(&mut fichier, "(◕‿◕)"), Some(7));
        assert_eq!(ajouter_dans(&mut fichier, "  (◕‿◕) "), None);
        assert_eq!(fichier.prochain_id, 8, "un refus ne consomme pas d'identifiant");
    }

    #[test]
    fn un_id_supprime_n_est_jamais_reutilise() {
        let mut fichier = Fichier { prochain_id: 1, kaomojis: Vec::new() };
        let id = ajouter_dans(&mut fichier, "(◕‿◕)").unwrap();
        assert!(supprimer_dans(&mut fichier, id));
        assert_eq!(ajouter_dans(&mut fichier, "(￣▽￣)"), Some(id + 1));
    }

    #[test]
    fn le_fichier_repare_un_prochain_id_absurde_et_ecarte_les_entrees_cassees() {
        let contenu = r#"{
            "prochain_id": 1,
            "kaomojis": [
                { "id": 5, "texte": "(◕‿◕)" },
                { "id": 5, "texte": "doublon d'id" },
                { "id": 6, "texte": "(◕‿◕)" },
                { "id": 7, "texte": "   " },
                { "texte": "sans id" }
            ]
        }"#;
        let fichier = analyser(contenu);
        assert_eq!(fichier.kaomojis.len(), 1);
        assert_eq!(fichier.prochain_id, 6);
    }

    #[test]
    fn un_fichier_illisible_rend_une_liste_vide() {
        assert_eq!(analyser("pas du json"), Fichier::default());
        assert_eq!(analyser(""), Fichier::default());
    }

    #[test]
    fn modifier_refuse_un_texte_deja_pris_mais_accepte_le_sien() {
        let mut fichier = Fichier { prochain_id: 1, kaomojis: Vec::new() };
        let a = ajouter_dans(&mut fichier, "(◕‿◕)").unwrap();
        let b = ajouter_dans(&mut fichier, "(￣▽￣)").unwrap();
        assert!(!modifier_dans(&mut fichier, b, "(◕‿◕)"));
        assert!(modifier_dans(&mut fichier, a, "(◕‿◕)"));
        assert!(modifier_dans(&mut fichier, b, "(＾▽＾)"));
    }
}
