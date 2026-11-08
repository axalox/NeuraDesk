//! **LES PALETTES DE COULEURS AU FORMAT `.gpl`** — celui de GIMP, que
//! **Inkscape lit nativement** (ses palettes par défaut dans
//! `share/palettes/` sont exactement ces fichiers). Un seul lecteur/écrivain
//! couvre donc les deux, ce qui est la raison pour laquelle il n'y en a qu'un
//! ici : demander « au format gimp et inkscape » désigne un seul format.
//!
//! **Le format**, tel que GIMP l'écrit (`%3d %3d %3d\t%s`) :
//!
//! ```text
//! GIMP Palette
//! Name: Ma palette
//! Columns: 16
//! #
//! 255   0   0	Rouge
//!   0 255   0	Vert
//! ```
//!
//! **CE QUI EST TOLÉRÉ À LA LECTURE, ET POURQUOI.** Un `.gpl` n'est pas
//! toujours écrit par GIMP : il peut sortir d'Inkscape, de Krita, d'un script,
//! ou avoir été retouché à la main. Le lecteur accepte donc un BOM UTF-8, des
//! fins de ligne Windows (traitées par `str::lines`), des lignes vides, des
//! commentaires `#`, un `Columns:` absent, et aussi bien des tabulations que
//! des espaces — en nombre quelconque — entre les composantes.
//!
//! **Une ligne MALFORMÉE n'est jamais fatale** : elle est ignorée et comptée
//! dans `RapportGpl`, que l'appelant rend à l'utilisateur. Un fichier de
//! quarante couleurs avec une coquille doit en charger trente-neuf et le DIRE,
//! pas refuser d'ouvrir. Seuls deux échecs sont durs : pas d'en-tête (ce n'est
//! pas une palette), et zéro couleur valide.
//!
//! **8 bits ou 16 bits ?** On écrit toujours 8 bits. À la lecture, si l'une
//! des trois composantes dépasse 255, la ligne est lue comme du 16 bits et
//! **les trois** sont ramenées par `255/65535`. C'est la seule lecture qui ne
//! soit pas arbitraire : GIMP, Inkscape et Krita écrivent tous du 8 bits, donc
//! une composante au-dessus de 255 n'est pas une ligne 8 bits valide, et le
//! 16 bits est la seule autre chose qu'elle puisse être.

use std::path::PathBuf;

/// L'en-tête, sur la première ligne non vide. C'est lui qui distingue une
/// palette d'un fichier quelconque.
const ENTETE: &str = "GIMP Palette";
/// Extension des fichiers de la bibliothèque.
const EXTENSION: &str = "gpl";
/// Longueur maximale d'un nom de palette, en caractères — un nom sert de
/// libellé dans la liste de l'applet.
pub const NOM_MAX: usize = 60;
/// Ce que GIMP écrit dans `Columns:`, et rien de plus : le champ décrit la
/// présentation de la grille dans GIMP, il n'a **aucun** effet sur les
/// couleurs. On le relit et on le réécrit pour ne pas abîmer un fichier qu'on
/// réenregistre, jamais pour en déduire quoi que ce soit.
const COLONNES_DEFAUT: u16 = 16;

/// Une couleur d'une palette : ses trois canaux 8 bits, et le nom libre qui
/// la suit dans le fichier (souvent vide — GIMP l'accepte).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CouleurGpl {
    pub rgb: [u8; 3],
    pub nom: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteGpl {
    pub nom: String,
    pub colonnes: u16,
    pub couleurs: Vec<CouleurGpl>,
}

/// **CE QU'ON A DÛ IGNORER.** Rendu à l'appelant pour qu'il puisse le DIRE à
/// l'utilisateur : perdre des couleurs en silence serait le seul vrai défaut
/// de la tolérance décrite en tête de module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RapportGpl {
    /// Lignes de couleur examinées (hors en-tête, lignes vides et
    /// commentaires).
    pub lignes_lues: usize,
    /// Celles qui n'ont produit aucune couleur.
    pub lignes_ignorees: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErreurGpl {
    /// La première ligne non vide n'est pas `GIMP Palette`.
    EnteteManquante,
    /// L'en-tête était là, mais aucune ligne de couleur n'a pu être lue.
    AucuneCouleur,
}

/// Lit une palette. **Fonction PURE** — tout ce qui touche au disque est plus
/// bas, dans la section « bibliothèque ».
pub fn depuis_gpl(texte: &str) -> Result<(PaletteGpl, RapportGpl), ErreurGpl> {
    // Un BOM UTF-8 devant `GIMP Palette` ferait échouer la comparaison pour un
    // octet — et un `.gpl` écrit sous Windows en porte souvent un.
    let texte = texte.strip_prefix('\u{feff}').unwrap_or(texte);
    let mut lignes = texte.lines();

    let entete = lignes.by_ref().map(str::trim).find(|l| !l.is_empty());
    if entete != Some(ENTETE) {
        return Err(ErreurGpl::EnteteManquante);
    }

    let mut nom = String::new();
    let mut colonnes = COLONNES_DEFAUT;
    let mut couleurs = Vec::new();
    let mut lignes_lues = 0usize;

    for ligne in lignes {
        let ligne = ligne.trim();
        if ligne.is_empty() || ligne.starts_with('#') {
            continue;
        }
        // `Name:` / `Columns:` sont comparés SANS tenir compte de la casse :
        // GIMP écrit `Name:`, mais rien ne l'impose à un autre écrivain.
        if let Some(reste) = prefixe_insensible(ligne, "Name:") {
            // Coupé au PREMIER ':' — donc un nom contenant lui-même un ':'
            // (« Rouge : vif ») fait l'aller-retour intact.
            nom = reste.trim().to_string();
            continue;
        }
        if let Some(reste) = prefixe_insensible(ligne, "Columns:") {
            if let Ok(n) = reste.trim().parse() {
                colonnes = n;
            }
            continue;
        }
        lignes_lues += 1;
        if let Some(couleur) = couleur_depuis_ligne(ligne) {
            couleurs.push(couleur);
        }
    }

    if couleurs.is_empty() {
        return Err(ErreurGpl::AucuneCouleur);
    }
    let rapport = RapportGpl { lignes_lues, lignes_ignorees: lignes_lues - couleurs.len() };
    Ok((PaletteGpl { nom, colonnes, couleurs }, rapport))
}

/// Écrit une palette, octet pour octet comme GIMP. **Fonction PURE.**
///
/// Inkscape relirait aussi bien une mise en forme plus libre, mais coller à
/// celle de GIMP ne coûte rien et évite une classe de questions sans intérêt
/// (« pourquoi GIMP a-t-il changé ma palette en la réenregistrant ? »).
pub fn vers_gpl(palette: &PaletteGpl) -> String {
    let mut sortie = String::new();
    sortie.push_str(ENTETE);
    sortie.push('\n');
    sortie.push_str("Name: ");
    sortie.push_str(&assainir(&palette.nom));
    sortie.push('\n');
    sortie.push_str(&format!("Columns: {}\n", palette.colonnes));
    sortie.push_str("#\n");
    for c in &palette.couleurs {
        let [r, g, b] = c.rgb;
        sortie.push_str(&format!("{r:3} {g:3} {b:3}\t{}\n", assainir(&c.nom)));
    }
    sortie
}

/// Nom de palette acceptable comme NOM DE FICHIER dans la bibliothèque.
/// `None` s'il est vide, trop long, ou porte un caractère que Windows refuse.
///
/// ⚠️ **Le point final est refusé**, et ce n'est pas de la coquetterie :
/// Windows retire silencieusement les points et espaces de fin, donc `rouge.`
/// et `rouge` désigneraient le MÊME fichier — un écrasement invisible.
/// L'espace de fin, lui, est déjà retiré par `trim`.
pub fn nom_de_fichier_valide(nom: &str) -> Option<String> {
    const INTERDITS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    let nom = nom.trim();
    let valide = !nom.is_empty()
        && nom.chars().count() <= NOM_MAX
        && !nom.chars().any(|c| c.is_control() || INTERDITS.contains(&c))
        && !nom.starts_with('.')
        && !nom.ends_with('.');
    valide.then(|| nom.to_string())
}

/// Le nom de palette que désigne un nom de fichier de la bibliothèque —
/// `"Aurore.gpl"` → `"Aurore"`. Tout ce qui n'est pas un `.gpl` est rendu tel
/// quel (voir `lister`, qui s'en sert pour écarter les autres fichiers).
pub fn nom_depuis_nom_de_fichier(fichier: &str) -> String {
    match fichier.rsplit_once('.') {
        Some((base, extension)) if extension.eq_ignore_ascii_case(EXTENSION) => base.to_string(),
        _ => fichier.to_string(),
    }
}

// --- La bibliothèque (%APPDATA%\NeuraDesk\palettes\) -------------------------
//
// ⚠️ **AUCUN TEST ICI, ET C'EST DÉLIBÉRÉ** : le dépôt interdit à ses tests de
// toucher `%APPDATA%` (voir `config::tests`, qui le rappelle à deux endroits).
// Ce qui est testable l'est plus haut — la grammaire, et `nom_de_fichier_valide`
// dont dépendent ces quatre fonctions. Ne pas « réparer » ce manque de
// couverture en pointant un test sur le vrai dossier.

/// Le chemin d'une palette de la bibliothèque, `None` si le nom est refusé.
fn chemin(nom: &str) -> Option<PathBuf> {
    let nom = nom_de_fichier_valide(nom)?;
    let mut path = crate::config::palettes_dir()?;
    path.push(format!("{nom}.{EXTENSION}"));
    Some(path)
}

/// Les noms des palettes de la bibliothèque, triés sans tenir compte de la
/// casse. Vide si le dossier n'existe pas encore — ce n'est pas une erreur.
pub fn lister() -> Vec<String> {
    let Some(dossier) = crate::config::palettes_dir() else { return Vec::new() };
    let Ok(entrees) = std::fs::read_dir(&dossier) else { return Vec::new() };
    let mut noms: Vec<String> = entrees
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let nom = e.file_name();
            Some(nom_depuis_nom_de_fichier(nom.to_str()?))
        })
        .filter(|n| !n.is_empty())
        .collect();
    noms.sort_by_key(|n| n.to_lowercase());
    noms.dedup_by_key(|n| n.to_lowercase());
    noms
}

/// Le contenu d'une palette, tel quel. `None` si elle a disparu entre
/// l'affichage de la liste et le clic.
///
/// **Lu en OCTETS puis `from_utf8_lossy`** : `read_to_string` refuserait un
/// nom écrit en Latin-1 par un vieil Inkscape, et perdre une palette entière
/// pour un accent mal encodé n'est pas un compromis acceptable.
pub fn lire(nom: &str) -> Option<String> {
    let octets = std::fs::read(chemin(nom)?).ok()?;
    Some(String::from_utf8_lossy(&octets).into_owned())
}

/// Écrit (ou écrase) une palette de la bibliothèque. Écriture **atomique** —
/// même raison que `config::write_config_atomique` : un fichier tronqué serait
/// refusé à la relecture, donc perdu, et silencieusement.
pub fn ecrire(nom: &str, contenu: &str) -> bool {
    let Some(path) = chemin(nom) else { return false };
    crate::config::write_atomique(&path, contenu.to_string())
}

/// Retire une palette de la bibliothèque. `false` si elle n'existait pas.
pub fn supprimer(nom: &str) -> bool {
    let Some(path) = chemin(nom) else { return false };
    std::fs::remove_file(path).is_ok()
}

// --- Aides pures -------------------------------------------------------------

/// `ligne` commence-t-elle par `prefixe`, sans tenir compte de la casse ?
/// Rend ce qui suit le préfixe.
fn prefixe_insensible<'a>(ligne: &'a str, prefixe: &str) -> Option<&'a str> {
    let debut = ligne.get(..prefixe.len())?;
    debut.eq_ignore_ascii_case(prefixe).then(|| &ligne[prefixe.len()..])
}

/// Une ligne `r g b[\tnom]` → une couleur. `None` si elle est malformée.
///
/// Les trois premiers jetons sont séparés par des espaces ou des tabulations,
/// en nombre quelconque ; **tout le reste est le nom**, espaces compris — donc
/// un nom qui commence par un chiffre (« 128 Rouge ») n'est pas pris pour une
/// quatrième composante.
fn couleur_depuis_ligne(ligne: &str) -> Option<CouleurGpl> {
    let mut reste = ligne;
    let mut valeurs = [0i32; 3];
    for valeur in valeurs.iter_mut() {
        reste = reste.trim_start();
        if reste.is_empty() {
            return None;
        }
        let fin = reste.find(char::is_whitespace).unwrap_or(reste.len());
        *valeur = reste[..fin].parse().ok()?;
        reste = &reste[fin..];
    }
    if valeurs.iter().any(|v| !(0..=65535).contains(v)) {
        return None;
    }
    let rgb = if valeurs.iter().any(|v| *v > 255) {
        // 16 bits — voir la doc de module : les TROIS canaux sont ramenés,
        // jamais seulement ceux qui dépassaient.
        valeurs.map(|v| (v as f32 * 255.0 / 65535.0).round() as u8)
    } else {
        valeurs.map(|v| v as u8)
    };
    Some(CouleurGpl { rgb, nom: reste.trim().to_string() })
}

/// Retire d'un nom ce qui casserait le format : un caractère de contrôle (dont
/// le saut de ligne, qui inventerait une ligne, et la tabulation, qui est le
/// séparateur du format).
fn assainir(nom: &str) -> String {
    nom.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette_exemple() -> PaletteGpl {
        PaletteGpl {
            nom: "Aurore".to_string(),
            colonnes: 16,
            couleurs: vec![
                CouleurGpl { rgb: [255, 0, 0], nom: "Rouge".to_string() },
                CouleurGpl { rgb: [0, 255, 0], nom: "Vert".to_string() },
                CouleurGpl { rgb: [3, 3, 3], nom: String::new() },
            ],
        }
    }

    #[test]
    fn l_ecriture_est_exactement_celle_de_gimp() {
        // Comparé à la CHAÎNE, pas à un reparsage : c'est la seule façon de
        // prouver le remplissage à trois colonnes et la tabulation.
        //
        // ⚠️ Écrit avec `concat!` et non avec des continuations de ligne
        // (`\` en fin de ligne) : celles-ci AVALENT les espaces de tête, or
        // c'est exactement ce que ce test doit vérifier — `  0 255   0` a un
        // espace de tête, `255   0   0` n'en a pas.
        let attendu = concat!(
            "GIMP Palette\n",
            "Name: Aurore\n",
            "Columns: 16\n",
            "#\n",
            "255   0   0\tRouge\n",
            "  0 255   0\tVert\n",
            "  3   3   3\t\n",
        );
        assert_eq!(vers_gpl(&palette_exemple()), attendu);
    }

    #[test]
    fn le_fichier_fait_l_aller_retour() {
        let (relue, rapport) = depuis_gpl(&vers_gpl(&palette_exemple())).expect("relisible");
        assert_eq!(relue, palette_exemple());
        assert_eq!(rapport.lignes_ignorees, 0);
        assert_eq!(rapport.lignes_lues, 3);
    }

    #[test]
    fn un_nom_avec_deux_points_fait_l_aller_retour() {
        // Coupé au PREMIER ':' — sinon « Rouge : vif » se relirait « vif ».
        let mut p = palette_exemple();
        p.nom = "Rouge : vif".to_string();
        p.couleurs[0].nom = "a: b".to_string();
        let (relue, _) = depuis_gpl(&vers_gpl(&p)).expect("relisible");
        assert_eq!(relue.nom, "Rouge : vif");
        assert_eq!(relue.couleurs[0].nom, "a: b");
    }

    #[test]
    fn le_bom_le_crlf_les_lignes_vides_et_les_espaces_sont_toles() {
        let texte = "\u{feff}GIMP Palette\r\n\r\nName: Mélangé\r\n\
                     255   0   0\tRouge\r\n\
                     \r\n\
                     # un commentaire\r\n\
                     0\t255\t0\tVert\r\n\
                     3 3 3\r\n";
        let (p, rapport) = depuis_gpl(texte).expect("lisible");
        assert_eq!(p.nom, "Mélangé");
        assert_eq!(p.couleurs.len(), 3);
        assert_eq!(p.couleurs[1].rgb, [0, 255, 0]);
        assert_eq!(p.couleurs[1].nom, "Vert");
        assert_eq!(p.couleurs[2].nom, "");
        assert_eq!(rapport.lignes_lues, 3);
        assert_eq!(rapport.lignes_ignorees, 0);
    }

    #[test]
    fn un_columns_absent_ou_illisible_retombe_sur_la_valeur_de_gimp() {
        let (p, _) = depuis_gpl("GIMP Palette\nName: X\n0 0 0\n").expect("lisible");
        assert_eq!(p.colonnes, COLONNES_DEFAUT);
        let (p, _) = depuis_gpl("GIMP Palette\nColumns: pas un nombre\n0 0 0\n").expect("lisible");
        assert_eq!(p.colonnes, COLONNES_DEFAUT);
        let (p, _) = depuis_gpl("GIMP Palette\nColumns: 4\n0 0 0\n").expect("lisible");
        assert_eq!(p.colonnes, 4);
    }

    #[test]
    fn une_composante_au_dessus_de_255_fait_lire_la_ligne_en_16_bits() {
        // 65535 = blanc 16 bits, 32896 = 128 en 16 bits, 257 = 1.
        let (p, rapport) = depuis_gpl("GIMP Palette\n65535 0 0\n32896 0 0\n257 257 257\n")
            .expect("lisible");
        assert_eq!(p.couleurs[0].rgb, [255, 0, 0]);
        assert_eq!(p.couleurs[1].rgb, [128, 0, 0]);
        assert_eq!(p.couleurs[2].rgb, [1, 1, 1]);
        assert_eq!(rapport.lignes_ignorees, 0);
    }

    #[test]
    fn une_ligne_malformee_est_comptee_et_ne_fait_perdre_qu_elle() {
        let texte = "GIMP Palette\n\
                     255 0 0\tRouge\n\
                     -1 0 0\n\
                     99999999 0 0\n\
                     0 0\n\
                     pas des nombres\n\
                     255 0\n\
                     0 255 0\tVert\n";
        let (p, rapport) = depuis_gpl(texte).expect("lisible");
        assert_eq!(p.couleurs.len(), 2);
        assert_eq!(p.couleurs[0].nom, "Rouge");
        assert_eq!(p.couleurs[1].nom, "Vert");
        assert_eq!(rapport.lignes_lues, 7);
        assert_eq!(rapport.lignes_ignorees, 5);
    }

    #[test]
    fn un_nom_qui_commence_par_un_chiffre_n_est_pas_une_quatrieme_composante() {
        let (p, _) = depuis_gpl("GIMP Palette\n255 0 0\t128 Rouge\n").expect("lisible");
        assert_eq!(p.couleurs[0].nom, "128 Rouge");
    }

    #[test]
    fn les_deux_seuls_echecs_durs_sont_l_entete_et_l_absence_de_couleur() {
        assert_eq!(depuis_gpl("ce n'est pas une palette\n255 0 0\n"), Err(ErreurGpl::EnteteManquante));
        // Un fichier vide, ou un en-tête suivi de commentaires seulement.
        assert_eq!(depuis_gpl(""), Err(ErreurGpl::EnteteManquante));
        assert_eq!(depuis_gpl("GIMP Palette\nName: X\n#\n"), Err(ErreurGpl::AucuneCouleur));
        // L'en-tête doit être la PREMIÈRE ligne non vide, pas n'importe où.
        assert_eq!(depuis_gpl("\n\nGIMP Palette\n0 0 0\n").map(|_| ()), Ok(()));
    }

    #[test]
    fn un_nom_de_couleur_ne_peut_pas_inventer_une_ligne_ni_une_colonne() {
        let p = PaletteGpl {
            nom: "Deux\nlignes".to_string(),
            colonnes: 1,
            couleurs: vec![CouleurGpl { rgb: [1, 2, 3], nom: "a\tb\nc".to_string() }],
        };
        let texte = vers_gpl(&p);
        assert_eq!(texte.lines().count(), 5, "en-tête, Name, Columns, #, une couleur");
        let (relue, rapport) = depuis_gpl(&texte).expect("relisible");
        assert_eq!(relue.couleurs.len(), 1);
        assert_eq!(rapport.lignes_ignorees, 0);
        assert_eq!(relue.nom, "Deux lignes");
        assert_eq!(relue.couleurs[0].nom, "a b c");
    }

    #[test]
    fn nom_de_fichier_valide_refuse_ce_que_windows_refuse() {
        assert_eq!(nom_de_fichier_valide("  Aurore  ").as_deref(), Some("Aurore"));
        assert_eq!(nom_de_fichier_valide(""), None);
        assert_eq!(nom_de_fichier_valide("   "), None);
        for mauvais in ["..", ".", "a/b", "a\\b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b", "a\nb", "a\u{0}b"] {
            assert_eq!(nom_de_fichier_valide(mauvais), None, "{mauvais:?} doit être refusé");
        }
        // Un point final serait retiré en silence par Windows : « rouge. » et
        // « rouge » désigneraient le même fichier.
        assert_eq!(nom_de_fichier_valide("rouge."), None);
        assert_eq!(nom_de_fichier_valide("rouge").as_deref(), Some("rouge"));
        assert_eq!(nom_de_fichier_valide(".cache"), None);
        assert_eq!(nom_de_fichier_valide(&"é".repeat(NOM_MAX)).map(|n| n.chars().count()), Some(NOM_MAX));
        assert_eq!(nom_de_fichier_valide(&"é".repeat(NOM_MAX + 1)), None);
    }

    #[test]
    fn le_nom_se_deduit_du_nom_de_fichier_sans_tenir_compte_de_la_casse() {
        assert_eq!(nom_depuis_nom_de_fichier("Aurore.gpl"), "Aurore");
        assert_eq!(nom_depuis_nom_de_fichier("Aurore.GPL"), "Aurore");
        assert_eq!(nom_depuis_nom_de_fichier("Aurore.Gpl"), "Aurore");
        // Ce qui n'est pas un .gpl est rendu tel quel — c'est ainsi que
        // `lister` et son appelant s'accordent.
        assert_eq!(nom_depuis_nom_de_fichier("notes.txt"), "notes.txt");
        assert_eq!(nom_depuis_nom_de_fichier("Aurore"), "Aurore");
    }
}
