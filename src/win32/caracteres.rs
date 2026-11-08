//! **LES CARACTÈRES FRANÇAIS, ET LA PRIMITIVE QUI LES ÉCRIT AILLEURS.**
//!
//! Ce module ne DESSINE plus rien : la fenêtre est une page CEF (voir
//! `win32::cef_caracteres`), parce que l'utilisateur l'a voulue « comme la
//! visionneuse ». Il en reste les deux choses qu'une page ne peut pas faire :
//!
//! 1. **LA TABLE DES 48 CARACTÈRES** — elle est ici, et poussée à la page par
//!    `cef_caracteres::etat_json` : l'écrire aussi dans `caracteres.html` en ferait
//!    une seconde source de vérité, qui divergerait au premier ajout ;
//! 2. **LA FRAPPE** — `frapper` écrit un caractère dans la fenêtre qui a le premier
//!    plan. C'est la SEULE primitive du dépôt qui fasse cela, et elle est appelée
//!    par `cef_caracteres`, jamais d'ici.
//!
//! ⚠️ **`SendInput` N'EST PAS DE LA SIMULATION D'ENTRÉE AU SENS DE CE DÉPÔT.** La
//! règle (« on ne simule jamais une entrée sur le bureau réel », voir
//! `win32::debug_capture`) vise les OUTILS DE VÉRIFICATION, qui agiraient dans le dos
//! de l'utilisateur. Ici c'est un GESTE DE L'UTILISATEUR — il clique sur une lettre,
//! elle sort — et c'est le seul moyen d'écrire dans une autre application.
//! `win32::desktops` fait déjà exactement cela pour Win+Ctrl+Flèche.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY,
};

/// **LES 60 CARACTÈRES, PAR FAMILLES.** L'ordre EST celui de la fenêtre, et les
/// lignes sont celles que l'utilisateur a validées — les regrouper autrement (par
/// exemple en rangeant les majuscules sous leurs minuscules) rendrait la grille plus
/// difficile à parcourir du regard, pas plus facile.
///
/// ⚠️ **LA DERNIÈRE LIGNE EST LA PLUS LARGE (12), ET C'EST ELLE QUI DIMENSIONNE LA
/// FENÊTRE** (voir `cef_caracteres::taille_pour_tout_le_contenu`) : ajouter un
/// caractère ici élargit la fenêtre tout seul, sans qu'aucun chiffre ne soit à
/// retoucher ailleurs. C'est ce qui rend l'ajout de la ligne « spéciaux » sans
/// risque.
///
/// ⚠️ **LE `€` N'Y EST PAS, ET C'EST VOLONTAIRE** — il est déjà dans la ligne de
/// ponctuation (`« » ’ – — … €`). L'utilisateur l'a cité en même temps que les
/// nouveaux, mais l'ajouter ici le ferait apparaître DEUX FOIS dans la grille, et
/// deux touches identiques se lisent comme une erreur.
pub(crate) const FAMILLES: &[&[char]] = &[
    &['à', 'â', 'ä', 'ç', 'é', 'è', 'ê', 'ë'],
    &['î', 'ï', 'ô', 'ö', 'ù', 'û', 'ü', 'ÿ', 'œ'],
    &['À', 'Â', 'Ä', 'Ç', 'É', 'È', 'Ê', 'Ë'],
    &['Î', 'Ï', 'Ô', 'Ö', 'Ù', 'Û', 'Ü', 'Ÿ', 'Œ'],
    &['«', '»', '’', '–', '—', '…', '€'],
    &['æ', 'Æ', 'ß', 'ñ', 'Ñ', 'ø', 'Ø'],
    // LES CARACTÈRES SPÉCIAUX (demande explicite, 2026-10-06). L'apostrophe
    // typographique et le tiret long sont dans la ligne du dessus : ici, ce sont
    // ceux qu'on ne trouve pas sur une rangée de clavier français, ou qu'on y
    // cherche à l'aveugle.
    &[',', '&', '#', '{', '[', '|', '`', '\\', '^', '@', ']', '}'],
];

/// **FRAPPE UNE CHAÎNE ENTIÈRE** — un kaomoji fait de deux à vingt unités, et il part
/// d'un seul geste : les envoyer en vingt `SendInput` séparés laisserait une autre
/// application s'intercaler entre deux, et le kaomoji arriverait en morceaux.
///
/// ⚠️ **C'EST LA MÊME IMPLÉMENTATION QUE `frapper`, PAS UNE SECONDE** : la fenêtre
/// des caractères et celle des kaomojis écrivent par le même chemin, avec les mêmes
/// garanties. Vérifié avant d'écrire : le geste existait déjà ici — la règle du dépôt
/// est de ne pas créer un module pour un geste qui a déjà sa place.
pub(crate) fn frapper_texte(texte: &str) -> bool {
    let unites: Vec<u16> = texte.encode_utf16().collect();
    frapper_unites(&unites)
}

/// Le corps COMMUN aux deux : tout part en UN `SendInput`, appui et relâchement
/// entrelacés dans l'ordre.
fn frapper_unites(unites: &[u16]) -> bool {
    let mut entrees: Vec<INPUT> = Vec::with_capacity(unites.len() * 2);
    for &unite in unites.iter() {
        for drapeau in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
            entrees.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: unite, dwFlags: drapeau, time: 0, dwExtraInfo: 0 },
                },
            });
        }
    }
    // `SendInput` rend le nombre d'événements INSÉRÉS : zéro veut dire que le système
    // a tout refusé.
    let envoyes = unsafe { SendInput(&entrees, std::mem::size_of::<INPUT>() as i32) };
    envoyes as usize == entrees.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐ **LES 60 CARACTÈRES SONT CEUX QUI ONT ÉTÉ VALIDÉS, ET RIEN D'AUTRE.**
    /// C'est le CONTENU de la fenêtre, et une famille oubliée ou dupliquée se verrait
    /// à l'usage — c'est-à-dire trop tard.
    #[test]
    fn les_caracteres_sont_ceux_qui_ont_ete_demandes() {
        let total: usize = FAMILLES.iter().map(|f| f.len()).sum();
        assert_eq!(total, 60, "48 décidés le 2026-10-06, + 12 spéciaux le même jour");
        // ⛔ **AUCUN CARACTÈRE EN DOUBLE.** Les douze ajoutés ont été demandés en même
        // temps que `€`, qui était DÉJÀ dans la grille : c'est exactement le genre de
        // recouvrement qu'on ne voit pas en relisant la liste. Deux touches
        // identiques dans la fenêtre se liraient comme une erreur.
        let mut tous: Vec<char> = FAMILLES.iter().flat_map(|l| l.iter().copied()).collect();
        tous.sort_unstable();
        let avant = tous.len();
        tous.dedup();
        assert_eq!(tous.len(), avant, "un caractère est présent deux fois");
        let minuscules: Vec<char> = FAMILLES[0].iter().chain(FAMILLES[1].iter()).copied().collect();
        let majuscules: Vec<char> = FAMILLES[2].iter().chain(FAMILLES[3].iter()).copied().collect();
        assert_eq!(minuscules.len(), 17);
        assert_eq!(majuscules.len(), 17);
        assert!(minuscules.contains(&'œ') && majuscules.contains(&'Œ'), "la ligature est la demande d'origine");
        // ⚠️ **CHAQUE MAJUSCULE A SA MINUSCULE** : c'est la vérification qui
        // attraperait un accent oublié d'un seul côté, et le seul contrôle qui relie
        // les deux familles.
        for m in &majuscules {
            let basse = m.to_lowercase().next().expect("une majuscule a une minuscule");
            assert!(minuscules.contains(&basse), "{m} n'a pas sa minuscule {basse} dans la grille");
        }
    }

    /// **CHAQUE CARACTÈRE TIENT EN UNE SEULE UNITÉ UTF-16** — donc la frappe en
    /// envoie exactement deux événements (appui et relâchement), et le compte rendu
    /// par `frapper` reste lisible. Un caractère hors BMP en enverrait quatre, ce que
    /// le code gère — mais il n'y en a aucun dans la grille, et ce test le fige.
    #[test]
    fn aucun_caractere_n_est_hors_du_plan_multilingue_de_base() {
        for ligne in FAMILLES {
            for &c in *ligne {
                let mut tampon = [0u16; 2];
                assert_eq!(c.encode_utf16(&mut tampon).len(), 1, "{c:?} demande deux unités UTF-16");
            }
        }
    }
}
