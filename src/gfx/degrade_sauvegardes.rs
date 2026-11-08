//! Réglages du dégradé animé sauvegardés sous un nom — DEMANDE EXPLICITE
//! (2026-09-27) : « donne aussi la possibilité de sauvegarder les réglages et
//! de les recharger ». Une sauvegarde est un instantané complet (motif,
//! couleurs, curseurs) rangé dans `degrade_sauvegardes.toml`.
//!
//! ⭐ **UN MOTIF DE L'UTILISATEUR Y EST AUSSI** (2026-10-10) : `mode` vaut alors
//! `shader:<nom>` (voir `gfx::shader_utilisateur::PREFIXE`), la forme même de
//! `degrade_mode.txt`. Avant, la sauvegarde enregistrait le motif INTÉGRÉ caché
//! sous le shader, et la recharger faisait perdre le shader.
//!
//! Les curseurs sont écrits PAR NOM, pas par position : une sauvegarde faite
//! avant l'ajout d'un curseur se recharge avec ce curseur à sa valeur par
//! défaut, au lieu de se relire décalée.

use std::sync::Mutex;

use super::degrade;

const FICHIER: &str = "degrade_sauvegardes.toml";
/// Longueur maximale d'un nom (en caractères) — un nom sert de libellé de
/// bouton.
pub const NOM_MAX: usize = 60;

#[derive(Clone, Debug, PartialEq)]
pub struct Sauvegarde {
    pub nom: String,
    pub mode: String,
    /// Nom du préréglage, ou `degrade::PERSONNALISE`.
    pub preset: String,
    pub couleurs: [[f32; 3]; 5],
    pub reglages: Vec<(String, f32)>,
}

static SAUVEGARDES: Mutex<Vec<Sauvegarde>> = Mutex::new(Vec::new());

fn hex(c: [f32; 3]) -> String {
    let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("{r:02X}{g:02X}{b:02X}")
}

fn depuis_hex(s: &str) -> Option<[f32; 3]> {
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF].map(|c| c as f32 / 255.0))
}

/// Nom accepté : espaces de bord retirés, non vide, au plus `NOM_MAX`
/// caractères, sans caractère de contrôle.
pub fn nom_valide(nom: &str) -> Option<String> {
    let nom = nom.trim();
    (!nom.is_empty() && nom.chars().count() <= NOM_MAX && !nom.chars().any(char::is_control))
        .then(|| nom.to_string())
}

/// Lit le fichier ; une entrée abîmée (nom manquant, couleur illisible) est
/// sautée sans faire perdre les autres.
pub fn depuis_toml(texte: &str) -> Vec<Sauvegarde> {
    let Ok(table) = texte.parse::<toml::Table>() else { return Vec::new() };
    let Some(entrees) = table.get("sauvegarde").and_then(|v| v.as_array()) else { return Vec::new() };
    entrees
        .iter()
        .filter_map(|e| {
            let s = |cle: &str| e.get(cle).and_then(|x| x.as_str()).map(str::to_string);
            let nom = nom_valide(&s("nom")?)?;
            let couleurs: Vec<[f32; 3]> = e
                .get("couleurs")?
                .as_array()?
                .iter()
                .map(|c| c.as_str().and_then(depuis_hex))
                .collect::<Option<_>>()?;
            let reglages = e
                .get("reglages")
                .and_then(|r| r.as_table())
                .map(|t| {
                    t.iter()
                        // `config::flottant_toml` — la définition UNIQUE du motif. Elle
                        // vivait ici, écrite à la main, et manquait dans `config.rs` :
                        // deux copies qui divergent, c'est ce que la troisième aurait
                        // consacré.
                        .filter_map(|(k, v)| Some((k.clone(), crate::config::flottant_toml(v)? as f32)))
                        .collect()
                })
                .unwrap_or_default();
            Some(Sauvegarde {
                nom,
                mode: s("mode").unwrap_or_default(),
                preset: s("preset").unwrap_or_else(|| degrade::PERSONNALISE.to_string()),
                couleurs: couleurs.try_into().ok()?,
                reglages,
            })
        })
        .collect()
}

pub fn vers_toml(sauvegardes: &[Sauvegarde]) -> String {
    use toml::Value;
    let entrees = sauvegardes
        .iter()
        .map(|s| {
            let mut t = toml::Table::new();
            t.insert("nom".into(), Value::String(s.nom.clone()));
            t.insert("mode".into(), Value::String(s.mode.clone()));
            t.insert("preset".into(), Value::String(s.preset.clone()));
            t.insert("couleurs".into(), Value::Array(s.couleurs.iter().map(|c| Value::String(hex(*c))).collect()));
            let reglages = s
                .reglages
                .iter()
                // Arrondi au millième : le fichier reste lisible.
                .map(|(k, v)| (k.clone(), Value::Float((*v as f64 * 1000.0).round() / 1000.0)))
                .collect::<toml::Table>();
            t.insert("reglages".into(), Value::Table(reglages));
            Value::Table(t)
        })
        .collect();
    let mut table = toml::Table::new();
    table.insert("sauvegarde".into(), Value::Array(entrees));
    toml::to_string(&table).unwrap_or_default()
}

/// Relit le fichier — une fois, au démarrage (voir `degrade::charger`).
pub fn charger() {
    let texte = crate::config::config_path(FICHIER).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    if let Ok(mut liste) = SAUVEGARDES.lock() {
        *liste = depuis_toml(&texte);
    }
}

fn enregistrer(liste: &[Sauvegarde]) {
    if !crate::config::write_config_atomique(FICHIER, vers_toml(liste)) {
        crate::diag!("[degrade] {FICHIER} n'a pas pu être écrit");
    }
}

/// **CE QUE LA PAGE AFFICHE DE CHAQUE SAUVEGARDE** : le nom, le motif, et les
/// couleurs TELLES QU'ELLES S'AFFICHENT — rééchantillonnées sur le nombre de
/// couleurs actives de la sauvegarde, comme le fait le dégradé lui-même (voir
/// `degrade::couleurs_actives`). Montrer les cinq couleurs brutes montrerait des
/// couleurs qu'une sauvegarde à deux couleurs n'affiche pas. DEMANDE EXPLICITE
/// (2026-10-03) : « un petit aperçu de couleurs » pour s'y retrouver dans une
/// centaine de sauvegardes.
pub fn apercus() -> Vec<serde_json::Value> {
    let Ok(liste) = SAUVEGARDES.lock() else { return Vec::new() };
    liste.iter().map(apercu).collect()
}

fn apercu(s: &Sauvegarde) -> serde_json::Value {
    let n = s
        .reglages
        .iter()
        .find(|(nom, _)| nom == "couleurs")
        .map(|(_, v)| v.round() as usize)
        .unwrap_or(5);
    let couleurs: Vec<String> = degrade::reechantillonner(&s.couleurs, n).iter().map(|c| hex(*c)).collect();
    serde_json::json!({ "nom": s.nom, "mode": libelle_du_motif(&s.mode), "couleurs": couleurs })
}

/// Le motif d'une sauvegarde tel que la page l'affiche — `shader:vagues` se lit
/// « Mon shader : vagues ».
fn libelle_du_motif(mode: &str) -> String {
    match shader_de(mode) {
        Some(nom) => format!("Mon shader : {nom}"),
        None => mode.to_string(),
    }
}

/// Le nom du motif de l'utilisateur que désigne `mode`, s'il en désigne un.
fn shader_de(mode: &str) -> Option<&str> {
    mode.strip_prefix(crate::gfx::shader_utilisateur::PREFIXE)
}

/// **LE MOTIF À SAUVER** : le motif de l'utilisateur s'il y en a un — celui qui
/// compile encore d'abord, c'est le dernier CHOISI —, sinon l'intégré.
fn mode_a_sauver(en_compilation: Option<String>, actif: Option<String>, integre: &str) -> String {
    match en_compilation.or(actif) {
        Some(nom) => format!("{}{nom}", crate::gfx::shader_utilisateur::PREFIXE),
        None => integre.to_string(),
    }
}

/// L'état courant, sous le nom `nom`.
fn instantane(nom: String) -> Sauvegarde {
    Sauvegarde {
        nom,
        mode: mode_a_sauver(
            crate::gfx::shader_utilisateur::en_compilation(),
            crate::gfx::shader_utilisateur::nom_actif(),
            degrade::mode_nom(),
        ),
        preset: degrade::preset_nom().to_string(),
        couleurs: degrade::couleurs(),
        reglages: degrade::REGLAGES
            .iter()
            .zip(degrade::valeurs_reglages())
            .map(|(r, v)| (r.nom.to_string(), v))
            .collect(),
    }
}

/// Sauvegarde l'état courant sous `nom` — un nom déjà pris est REMPLACÉ, à
/// sa place dans la liste. Renvoie `false` pour un nom refusé.
pub fn sauver(nom: &str) -> bool {
    let Some(nom) = nom_valide(nom) else { return false };
    let Ok(mut liste) = SAUVEGARDES.lock() else { return false };
    let s = instantane(nom);
    match liste.iter_mut().find(|x| x.nom == s.nom) {
        Some(x) => *x = s,
        None => liste.push(s),
    }
    enregistrer(&liste);
    true
}

/// Applique la sauvegarde `nom` à l'état courant (sans l'écrire dans les
/// fichiers de réglages : voir l'appelant). Renvoie `false` si elle n'existe
/// pas.
///
/// ⚠️ **UN MOTIF DE L'UTILISATEUR SE COMPILE SUR UN FIL** (`shader_utilisateur::
/// choisir`) : jusqu'à la fin, le motif d'avant reste dessiné, et c'est
/// `shader_utilisateur::activer` qui écrit `degrade_mode.txt` une fois le shader
/// compilé. S'il ne compile plus (ou si son fichier a disparu), le message de
/// l'éditeur et des Réglages le dit, et le motif d'avant reste.
pub fn appliquer(nom: &str) -> bool {
    let Some(s) = SAUVEGARDES.lock().ok().and_then(|l| l.iter().find(|x| x.nom == nom).cloned()) else {
        return false;
    };
    match shader_de(&s.mode) {
        Some(shader) => crate::gfx::shader_utilisateur::choisir(shader),
        None => {
            degrade::set_mode(&s.mode);
        }
    }
    // Un préréglage renommé ou retiré depuis : ses couleurs sont dans la
    // sauvegarde, reprises en « Personnalisé ».
    if s.preset == degrade::PERSONNALISE || !degrade::set_preset(&s.preset) {
        for (i, c) in s.couleurs.into_iter().enumerate() {
            degrade::set_couleur(i, c);
        }
    }
    degrade::reglages_par_defaut();
    for (k, v) in &s.reglages {
        degrade::set_reglage(k, *v);
    }
    true
}

/// Retire la sauvegarde `nom`. Renvoie `false` si elle n'existait pas.
pub fn supprimer(nom: &str) -> bool {
    let Ok(mut liste) = SAUVEGARDES.lock() else { return false };
    let avant = liste.len();
    liste.retain(|x| x.nom != nom);
    if liste.len() == avant {
        return false;
    }
    enregistrer(&liste);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exemple(nom: &str) -> Sauvegarde {
        Sauvegarde {
            nom: nom.to_string(),
            mode: "Aurore".to_string(),
            preset: degrade::PERSONNALISE.to_string(),
            couleurs: [[0.0, 0.2, 1.0], [1.0, 1.0, 1.0], [0.4, 0.4, 0.4], [0.2, 0.6, 0.8], [0.0, 0.0, 0.0]],
            reglages: vec![("echelle".to_string(), 2.5), ("alea".to_string(), 0.25)],
        }
    }

    #[test]
    fn le_fichier_fait_l_aller_retour() {
        let liste = vec![exemple("Calme"), exemple("Soir \"orageux\" = 2")];
        let relue = depuis_toml(&vers_toml(&liste));
        assert_eq!(relue.len(), 2);
        assert_eq!(relue[1].nom, "Soir \"orageux\" = 2");
        assert_eq!(relue[0].mode, "Aurore");
        // Couleurs quantifiées sur 8 bits.
        for (a, b) in relue[0].couleurs.iter().flatten().zip(liste[0].couleurs.iter().flatten()) {
            assert!((a - b).abs() < 1.0 / 255.0);
        }
        // Les curseurs sont relus par nom : l'ordre du fichier (trié) importe peu.
        let tries = |v: &[(String, f32)]| {
            let mut v = v.to_vec();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        };
        assert_eq!(tries(&relue[0].reglages), tries(&liste[0].reglages));
    }

    #[test]
    fn une_entree_abimee_ne_fait_perdre_qu_elle() {
        let mut texte = vers_toml(&[exemple("Bonne")]);
        texte.push_str("\n[[sauvegarde]]\nnom = \"Abimee\"\ncouleurs = [\"zz\"]\n");
        texte.push_str("\n[[sauvegarde]]\ncouleurs = [\"000000\",\"000000\",\"000000\",\"000000\",\"000000\"]\n");
        let relue = depuis_toml(&texte);
        assert_eq!(relue.iter().map(|s| s.nom.as_str()).collect::<Vec<_>>(), ["Bonne"]);
        assert!(depuis_toml("pas du toml [[").is_empty());
    }

    /// Le motif de l'utilisateur est sauvé À LA PLACE de l'intégré caché dessous —
    /// et celui qui compile encore l'emporte : c'est le dernier choisi.
    #[test]
    fn un_motif_de_l_utilisateur_est_sauve_a_la_place_de_l_integre() {
        assert_eq!(mode_a_sauver(None, None, "Aurore"), "Aurore");
        assert_eq!(mode_a_sauver(None, Some("vagues".into()), "Aurore"), "shader:vagues");
        assert_eq!(mode_a_sauver(Some("neuf".into()), Some("vagues".into()), "Aurore"), "shader:neuf");
        assert_eq!(shader_de("shader:vagues"), Some("vagues"));
        assert_eq!(shader_de("Aurore"), None);
    }

    /// Le fichier garde `shader:<nom>` tel quel, et la page le lit en clair.
    #[test]
    fn un_motif_de_l_utilisateur_fait_l_aller_retour() {
        let mut s = exemple("Avec shader");
        s.mode = "shader:Mes vagues (2)".to_string();
        let relue = depuis_toml(&vers_toml(&[s]));
        assert_eq!(relue[0].mode, "shader:Mes vagues (2)");
        assert_eq!(apercu(&relue[0])["mode"], "Mon shader : Mes vagues (2)");
        assert_eq!(apercu(&exemple("Sans"))["mode"], "Aurore");
    }

    #[test]
    fn nom_valide_refuse_vide_long_et_controle() {
        assert_eq!(nom_valide("  Soir  ").as_deref(), Some("Soir"));
        assert_eq!(nom_valide("   "), None);
        assert_eq!(nom_valide("a\nb"), None);
        assert_eq!(nom_valide(&"é".repeat(NOM_MAX)).map(|n| n.chars().count()), Some(NOM_MAX));
        assert_eq!(nom_valide(&"é".repeat(NOM_MAX + 1)), None);
    }
}
