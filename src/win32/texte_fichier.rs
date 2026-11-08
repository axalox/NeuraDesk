//! **LIRE UN FICHIER TEXTE POUR LE MONTRER** — décodage et borne de taille.
//!
//! ⛔ **RIEN DE TOUT CECI N'EXISTAIT.** Le dépôt ne lit des textes que pour ses
//! PROPRES fichiers (`config.rs` : TOML et TXT qu'il a écrits lui-même) ou pour
//! un import de notes — toujours par `std::fs::read_to_string`, qui **exige de
//! l'UTF-8 valide et échoue en silence sinon** (`cef_notes.rs:757`). Ouvrir le
//! fichier de l'utilisateur est un autre métier : il peut être en UTF-16 (tout ce
//! qu'écrit le Bloc-notes de Windows), porter un BOM, ou venir d'un vieil outil en
//! Latin-1. Un `read_to_string` sur ces fichiers-là rend une erreur, et l'écran
//! resterait vide sans un mot.
//!
//! ⚠️ **LE SEUL REPLI D'ENCODAGE DU DÉPÔT ÉTAIT LÀ, ET ON LE GÉNÉRALISE** :
//! `cef_couleurs.rs:311-312` lit une palette en OCTETS puis `from_utf8_lossy`,
//! « un nom Latin-1 écrit par un vieil Inkscape ne doit pas coûter la palette ».
//! Ici c'est la même idée, avec une différence : **on DIT lequel des décodages a
//! servi**. Une palette qui affiche un caractère de remplacement se voit ; un
//! document entier rendu en charabia, non.
//!
//! ⚠️ **LES DEUX FONCTIONS DE DÉCISION SONT PURES** (`decoder`, `tronquer`) : le
//! disque n'est touché que par `lire`, qui les appelle. C'est ce qui rend les
//! quatre encodages éprouvables sans fabriquer de fichiers.

use std::io::Read;
use std::path::Path;

/// **LA BORNE DE LECTURE.** Un fichier de 200 Mo n'a rien à faire dans une fenêtre
/// d'aperçu : on en lit le début, et **on le DIT** (voir `TexteLu::tronque`).
///
/// ⚠️ **1 Mo de texte, c'est déjà 30 000 lignes** — bien au-delà de ce qu'un
/// aperçu montre. La borne protège la mémoire et le transfert vers la page, pas la
/// lisibilité.
pub const TAILLE_MAX: usize = 1024 * 1024;

/// L'octet d'ordre, avec sa longueur — `\u{FEFF}` en UTF-8 fait trois octets.
const BOM_UTF8: &[u8] = &[0xEF, 0xBB, 0xBF];
const BOM_UTF16_LE: &[u8] = &[0xFF, 0xFE];
const BOM_UTF16_BE: &[u8] = &[0xFE, 0xFF];

/// **COMMENT LE TEXTE A ÉTÉ DÉCODÉ** — dit à l'écran, jamais tu.
///
/// Le dernier variant est le seul qui puisse MENTIR sur un caractère : il
/// remplace ce qu'il ne comprend pas. C'est précisément pour ça qu'il existe
/// comme variant distinct plutôt que fondu dans `Utf8`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encodage {
    /// UTF-8 valide, sans octet d'ordre.
    Utf8,
    /// UTF-8 avec un BOM, qui est RETIRÉ du contenu.
    Utf8Bom,
    /// UTF-16 petit-boutiste, BOM retiré.
    Utf16Le,
    /// UTF-16 gros-boutiste, BOM retiré.
    Utf16Be,
    /// Ni BOM ni UTF-8 valide : les octets fautifs sont remplacés.
    Utf8Approche,
}

impl Encodage {
    /// Le libellé montré à l'utilisateur.
    pub fn nom(self) -> &'static str {
        match self {
            Encodage::Utf8 => "UTF-8",
            Encodage::Utf8Bom => "UTF-8 (BOM)",
            Encodage::Utf16Le => "UTF-16 LE",
            Encodage::Utf16Be => "UTF-16 BE",
            Encodage::Utf8Approche => "UTF-8 approximé",
        }
    }

    /// Vrai si des caractères ont pu être REMPLACÉS — l'écran doit alors le dire,
    /// sinon un `?` au milieu d'un mot passerait pour une faute de frappe.
    pub fn approximatif(self) -> bool {
        matches!(self, Encodage::Utf8Approche)
    }
}

/// Le texte prêt à montrer, et ce qu'il faut en dire.
#[derive(Debug)]
pub struct TexteLu {
    pub contenu: String,
    pub encodage: Encodage,
    /// Vrai si le fichier était plus gros que `TAILLE_MAX`.
    pub tronque: bool,
}

/// **DÉCODER DES OCTETS — PUR.** L'ordre compte : le BOM d'abord, parce qu'un
/// fichier UTF-16 passé à un décodeur UTF-8 donne du charabia *valide* (des
/// octets NUL entre les lettres), donc indétectable après coup.
pub fn decoder(octets: &[u8]) -> (String, Encodage) {
    if let Some(reste) = octets.strip_prefix(BOM_UTF8) {
        return (String::from_utf8_lossy(reste).into_owned(), Encodage::Utf8Bom);
    }
    if let Some(reste) = octets.strip_prefix(BOM_UTF16_LE) {
        return (depuis_utf16(reste, true), Encodage::Utf16Le);
    }
    if let Some(reste) = octets.strip_prefix(BOM_UTF16_BE) {
        return (depuis_utf16(reste, false), Encodage::Utf16Be);
    }
    match std::str::from_utf8(octets) {
        Ok(texte) => (texte.to_string(), Encodage::Utf8),
        Err(_) => (String::from_utf8_lossy(octets).into_owned(), Encodage::Utf8Approche),
    }
}

/// Les octets d'un texte UTF-16, `petit_boutiste` disant dans quel ordre lire
/// chaque paire. Un nombre IMPAIR d'octets laisse le dernier de côté : le jeter
/// vaut mieux que de le lire à moitié et d'inventer un caractère.
fn depuis_utf16(octets: &[u8], petit_boutiste: bool) -> String {
    let unites: Vec<u16> = octets
        .chunks_exact(2)
        .map(|paire| {
            if petit_boutiste {
                u16::from_le_bytes([paire[0], paire[1]])
            } else {
                u16::from_be_bytes([paire[0], paire[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&unites)
}

/// **ROGNER SANS COUPER UN CARACTÈRE EN DEUX — PUR.**
///
/// ⛔ **C'EST LE DÉFAUT QUE CETTE FONCTION EXISTE POUR ÉVITER.** Tronquer 1 Mo
/// d'octets au hasard tombe une fois sur trois au milieu d'un caractère UTF-8
/// (deux à quatre octets) : le décodeur voit alors une séquence invalide, et
/// `from_utf8` échoue — donc on retombe sur le décodage « approximé » et le
/// DERNIER caractère du fichier devient un `?`, pour rien.
///
/// ⚠️ Pour l'UTF-16, l'unité est de DEUX octets : on s'aligne dessus.
pub fn tronquer(octets: &[u8], encodage: Encodage) -> &[u8] {
    if octets.len() <= TAILLE_MAX {
        return octets;
    }
    let debut = &octets[..TAILLE_MAX];
    match encodage {
        Encodage::Utf16Le | Encodage::Utf16Be => &debut[..debut.len() - (debut.len() % 2)],
        _ => {
            // ⛔ **ON RECULE D'AU PLUS TROIS OCTETS, JUSQU'À CE QUE LE PRÉFIXE
            // SOIT DE L'UTF-8 VALIDE — et pas seulement « le dernier octet n'est
            // pas une continuation ».** Cette seconde condition ne suffit pas, et
            // le cas est plus fréquent qu'il n'y paraît : en reculant devant la
            // continuation d'un « é » (deux octets), on laisse le PREMIER octet
            // du caractère tout seul, ce qui est aussi invalide. On demande donc
            // au décodeur lui-même, qui est le seul à savoir.
            for recul in 0..=3 {
                let fin = debut.len() - recul;
                if std::str::from_utf8(&debut[..fin]).is_ok() {
                    return &debut[..fin];
                }
            }
            // Quatre reculs n'ont rien donné : le fichier n'est PAS de l'UTF-8.
            // Le laisser tel quel est juste — le décodage approximé prendra le
            // relais, et le marquera comme tel.
            debut
        }
    }
}

/// **LIT UN FICHIER TEXTE, ET DIT COMMENT.** Une erreur n'est pas un cas
/// ordinaire : elle est rendue à l'appelant, qui la montre.
///
/// ⚠️ **ON NE LIT QUE LA BORNE + QUELQUES OCTETS.** Lire un fichier de 3 Go en
/// entier pour n'en garder qu'un mégaoctet ferait payer la lecture complète — et
/// sur un partage réseau, cela se compte en secondes.
pub fn lire(chemin: &Path) -> Result<TexteLu, String> {
    let fichier = std::fs::File::open(chemin).map_err(|e| format!("{e}"))?;
    // + 4 : de quoi savoir qu'il y avait PLUS que la borne, et de quoi détecter un
    // BOM sans l'avoir coupé.
    let mut brut = Vec::with_capacity(TAILLE_MAX + 4);
    fichier
        .take((TAILLE_MAX + 4) as u64)
        .read_to_end(&mut brut)
        .map_err(|e| format!("{e}"))?;

    let tronque = brut.len() > TAILLE_MAX;
    // ⚠️ **LE BOM SE LIT SUR LES OCTETS COMPLETS, LA TRONCATURE VIENT APRÈS** :
    // rogner d'abord pourrait amputer le BOM lui-même sur un fichier minuscule.
    let (_, encodage_sonde) = decoder(&brut[..brut.len().min(4)]);
    let garde = tronquer(&brut, encodage_sonde);
    let (contenu, encodage) = decoder(garde);
    Ok(TexteLu { contenu, encodage, tronque })
}

/// Vrai si cette extension est un texte qu'on sait montrer. **Sert à GRISER
/// l'entrée « Afficher » du menu**, plutôt qu'à ouvrir une fenêtre vide.
///
/// ⚠️ La liste est volontairement COURTE et explicite : un `.exe` ou un `.zip`
/// n'est pas du texte, et il n'y a aucun moyen de le deviner de façon fiable — le
/// contenu d'un fichier sans extension, lui, est le plus souvent du texte, d'où
/// l'acceptation du cas vide.
pub fn est_un_texte(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "" | "txt"
            | "md"
            | "markdown"
            | "log"
            | "json"
            | "toml"
            | "yaml"
            | "yml"
            | "ini"
            | "cfg"
            | "conf"
            | "csv"
            | "tsv"
            | "xml"
            | "html"
            | "htm"
            | "css"
            | "js"
            | "ts"
            | "jsx"
            | "tsx"
            | "rs"
            | "py"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "cs"
            | "java"
            | "go"
            | "rb"
            | "php"
            | "sh"
            | "ps1"
            | "bat"
            | "cmd"
            | "sql"
            | "lua"
            | "vim"
            | "gitignore"
    )
}

/// Les images que CHROMIUM sait décoder dans la page — donc affichables.
///
/// ⛔ **`psd` ET `xcf` EN SONT ABSENTS À DESSEIN** : ils ne sont pas des formats
/// web, et le dépôt ne les décode qu'en SOUS-PROCESSUS (`decodeur_document`),
/// pour l'éditeur d'image. Une visionneuse qui les accepterait ouvrirait une
/// fenêtre sur une image cassée.
pub fn est_une_image(extension: &str) -> bool {
    matches!(extension.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp")
}

/// Le type MIME d'une image, pour la construire en `data:` URI.
pub fn mime_pour(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "bmp" => Some("image/bmp"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⛔ **L'UTF-16 EST LE CAS QUI COMPTE.** Le Bloc-notes de Windows écrit de
    /// l'UTF-16 ; passé à un décodeur UTF-8, il rend un texte **valide et
    /// illisible** (des octets NUL entre les lettres) — donc aucune erreur ne le
    /// signale. Le BOM est la seule façon de le reconnaître.
    #[test]
    fn un_fichier_utf16_est_reconnu_par_son_bom() {
        let texte = "Bonjour été";
        let mut petit: Vec<u8> = BOM_UTF16_LE.to_vec();
        petit.extend(texte.encode_utf16().flat_map(u16::to_le_bytes));
        let (rendu, encodage) = decoder(&petit);
        assert_eq!(encodage, Encodage::Utf16Le);
        assert_eq!(rendu, texte, "le BOM doit etre RETIRE du contenu");

        let mut gros: Vec<u8> = BOM_UTF16_BE.to_vec();
        gros.extend(texte.encode_utf16().flat_map(u16::to_be_bytes));
        let (rendu, encodage) = decoder(&gros);
        assert_eq!(encodage, Encodage::Utf16Be);
        assert_eq!(rendu, texte);
    }

    /// Le BOM UTF-8 est retiré lui aussi : le laisser ferait afficher un caractère
    /// invisible en tête de document, et décalerait tout ce qui suit.
    #[test]
    fn un_bom_utf8_est_retire_et_dit() {
        let mut octets = BOM_UTF8.to_vec();
        octets.extend("été".as_bytes());
        let (rendu, encodage) = decoder(&octets);
        assert_eq!(encodage, Encodage::Utf8Bom);
        assert_eq!(rendu, "été");
    }

    /// **DE L'UTF-8 PROPRE EST DIT PROPRE** — et n'est PAS marqué « approximé »,
    /// sinon l'écran afficherait un avertissement sur tous les fichiers.
    #[test]
    fn de_l_utf8_valide_est_dit_exact() {
        let (rendu, encodage) = decoder("Résumé — 2026".as_bytes());
        assert_eq!(encodage, Encodage::Utf8);
        assert!(!encodage.approximatif());
        assert_eq!(rendu, "Résumé — 2026");
    }

    /// ⛔ **UN FICHIER LATIN-1 EST LU QUAND MÊME, ET ON LE DIT.** C'est le repli
    /// que le dépôt utilise déjà pour les palettes GIMP. Le défaut refusé est le
    /// fichier ENTIER resté vide parce qu'un octet n'était pas de l'UTF-8.
    #[test]
    fn un_fichier_latin1_est_lu_et_marque_approximatif() {
        // « été » en Latin-1 : le é est 0xE9, invalide en UTF-8. ⚠️ Le tableau
        // finit par une lettre ASCII, sans quoi l'assertion sur la FIN du texte
        // testerait le caractère de remplacement et non la lecture.
        let octets = [b'e', 0xE9, b't', 0xE9, b'!'];
        let (rendu, encodage) = decoder(&octets);
        assert_eq!(encodage, Encodage::Utf8Approche);
        assert!(encodage.approximatif(), "l'ecran DOIT pouvoir le dire");
        assert_eq!(rendu, "e\u{FFFD}t\u{FFFD}!", "le reste du texte doit rester lisible");
    }

    /// ⛔ **LA TRONCATURE NE COUPE PAS UN CARACTÈRE EN DEUX.** Sans l'alignement,
    /// le dernier caractère d'un fichier trop gros deviendrait un `?` — et le
    /// fichier serait alors marqué « approximé » alors qu'il était de l'UTF-8 pur.
    #[test]
    fn la_troncature_ne_coupe_pas_un_caractere_en_deux() {
        // On remplit avec des « é » (deux octets chacun) et on rogne juste après
        // la borne : la coupe tombe forcément au milieu d'un caractère.
        let plein: Vec<u8> = "é".repeat(TAILLE_MAX).into_bytes();
        let garde = tronquer(&plein, Encodage::Utf8);
        assert!(garde.len() <= TAILLE_MAX);
        let (_, encodage) = decoder(garde);
        assert_ne!(
            encodage,
            Encodage::Utf8Approche,
            "une troncature UTF-8 ne doit PAS faire croire a un encodage douteux"
        );

        // Et l'UTF-16 s'aligne sur deux octets, pas sur un.
        let large: Vec<u8> = "ab".repeat(TAILLE_MAX).into_bytes();
        assert_eq!(tronquer(&large, Encodage::Utf16Le).len() % 2, 0, "l'UTF-16 se coupe par paires");
    }

    /// Un fichier PLUS PETIT que la borne n'est pas dit tronqué — l'inverse
    /// afficherait un avertissement sur tous les fichiers.
    #[test]
    fn un_fichier_court_n_est_pas_dit_tronque() {
        assert_eq!(tronquer(b"court", Encodage::Utf8), b"court");
    }

    /// Le va-et-vient sur le DISQUE : c'est ce que `lire` fait vraiment, et le
    /// seul test qui touche un fichier.
    #[test]
    fn lire_rend_le_texte_et_son_encodage() {
        let chemin = std::env::temp_dir().join("nd-texte-fichier-utf16.txt");
        let mut octets = BOM_UTF16_LE.to_vec();
        octets.extend("Bonjour été".encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(&chemin, &octets).expect("ecriture");

        let lu = lire(&chemin).expect("lecture");
        assert_eq!(lu.contenu, "Bonjour été");
        assert_eq!(lu.encodage, Encodage::Utf16Le);
        assert!(!lu.tronque);
        let _ = std::fs::remove_file(&chemin);
    }

    /// **LA NATURE D'UN FICHIER DÉCIDE DU MENU** : c'est cette fonction qui grise
    /// « Afficher » sur un `.exe`, et l'ouvre sur un `.md`.
    #[test]
    fn la_nature_d_un_fichier_est_reconnue_et_insensible_a_la_casse() {
        assert!(est_un_texte("md"));
        assert!(est_un_texte("MD"));
        assert!(est_un_texte(""), "un fichier sans extension est le plus souvent du texte");
        assert!(!est_un_texte("exe"));
        assert!(!est_un_texte("png"), "une image n'est pas un texte");

        assert!(est_une_image("PNG"));
        assert!(est_une_image("jpeg"));
        assert!(!est_une_image("psd"), "psd n'est pas decodable par la page");
        assert!(!est_une_image("txt"));

        assert_eq!(mime_pour("JPG"), Some("image/jpeg"));
        assert_eq!(mime_pour("psd"), None);
    }
}
