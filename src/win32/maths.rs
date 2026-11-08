//! **LES SYMBOLES MATHÉMATIQUES, EN SIX ONGLETS.** La fenêtre qui les montre est
//! `win32::cef_maths` — une page CEF de la famille des symboles phonétiques : un
//! clic FRAPPE le symbole dans la dernière fenêtre étrangère (voir
//! `win32::cible_de_frappe`), un clic droit le COPIE.
//!
//! DEMANDE EXPLICITE (2026-10-12) : « une nouvelle applet dans la zone intégrés
//! pour ouvrir une fenêtre permettant d'insérer les caractères mathématiques ».
//!
//! ⚠️ **AUCUN CARACTÈRE DU CLAVIER** (`+`, `=`, `<`, `>`, `/`, `%`) : ils se tapent
//! déjà, et ils noieraient ceux qu'on vient chercher. ⛔ Et le MOINS est U+2212, pas
//! le trait d'union du clavier — c'est la raison d'être de la case.
//!
//! ⚠️ **QUELQUES SYMBOLES SONT HORS DU PLAN DE BASE** (`𝔻`, U+1D53B) : deux unités
//! UTF-16, envoyées EN UNE FOIS comme les emoji des kaomojis.

/// **UN SYMBOLE** : ce qui est inséré, et son nom, affiché sous lui.
pub struct Symbole {
    pub symbole: &'static str,
    pub nom: &'static str,
}

/// Un groupe d'un onglet : son titre, et ses symboles dans l'ordre de la page.
pub struct Groupe {
    pub nom: &'static str,
    pub symboles: &'static [Symbole],
}

/// Un onglet de la fenêtre.
pub struct Onglet {
    pub nom: &'static str,
    pub groupes: &'static [Groupe],
}

const fn s(symbole: &'static str, nom: &'static str) -> Symbole {
    Symbole { symbole, nom }
}

/// **LES SIX ONGLETS, DANS L'ORDRE DE LA FENÊTRE.** L'ordre EST celui que la page
/// relit au clic (voir `cef_maths::symbole_par_index`).
pub const ONGLETS: &[Onglet] = &[
    Onglet {
        nom: "Opérateurs",
        groupes: &[
            Groupe {
                nom: "Arithmétique",
                symboles: &[
                    s("−", "moins"),
                    s("×", "multiplié par"),
                    s("÷", "divisé par"),
                    s("±", "plus ou moins"),
                    s("∓", "moins ou plus"),
                    s("⋅", "produit scalaire"),
                    s("∘", "composée"),
                    s("∗", "convolution"),
                    s("⁄", "barre de fraction"),
                    s("√", "racine carrée"),
                    s("∛", "racine cubique"),
                    s("∜", "racine quatrième"),
                    s("‰", "pour mille"),
                ],
            },
            Groupe {
                nom: "Analyse",
                symboles: &[
                    s("∑", "somme"),
                    s("∏", "produit"),
                    s("∫", "intégrale"),
                    s("∬", "intégrale double"),
                    s("∭", "intégrale triple"),
                    s("∮", "intégrale curviligne"),
                    s("∂", "dérivée partielle"),
                    s("∇", "nabla"),
                    s("∆", "laplacien, écart"),
                    s("∞", "infini"),
                    s("′", "prime"),
                    s("″", "seconde"),
                ],
            },
        ],
    },
    Onglet {
        nom: "Relations",
        groupes: &[
            Groupe {
                nom: "Égalités",
                symboles: &[
                    s("≠", "différent de"),
                    s("≈", "environ égal"),
                    s("≃", "asymptotiquement égal"),
                    s("≅", "isomorphe"),
                    s("≡", "identique, congru"),
                    s("≢", "non identique"),
                    s("∼", "équivalent à"),
                    s("∝", "proportionnel à"),
                    s("≔", "défini par"),
                    s("≜", "égal par définition"),
                ],
            },
            Groupe {
                nom: "Ordre",
                symboles: &[
                    s("≤", "inférieur ou égal"),
                    s("≥", "supérieur ou égal"),
                    s("⩽", "inférieur ou égal"),
                    s("⩾", "supérieur ou égal"),
                    s("≪", "très inférieur"),
                    s("≫", "très supérieur"),
                    s("≺", "précède"),
                    s("≻", "succède"),
                ],
            },
            Groupe {
                nom: "Géométrie",
                symboles: &[
                    s("∥", "parallèle"),
                    s("∦", "non parallèle"),
                    s("⊥", "perpendiculaire"),
                    s("∠", "angle"),
                    s("∡", "angle orienté"),
                    s("△", "triangle"),
                    s("°", "degré"),
                    s("⌒", "arc"),
                ],
            },
        ],
    },
    Onglet {
        nom: "Ensembles",
        groupes: &[
            Groupe {
                nom: "Ensembles",
                symboles: &[
                    s("∈", "appartient à"),
                    s("∉", "n'appartient pas"),
                    s("∋", "contient"),
                    s("⊂", "inclus strictement"),
                    s("⊃", "contient strictement"),
                    s("⊆", "inclus dans"),
                    s("⊇", "contient"),
                    s("⊄", "non inclus"),
                    s("∪", "union"),
                    s("∩", "intersection"),
                    s("∖", "privé de"),
                    s("∅", "ensemble vide"),
                    s("∁", "complémentaire"),
                    s("℘", "ensemble des parties"),
                ],
            },
            Groupe {
                nom: "Nombres",
                symboles: &[
                    s("ℕ", "entiers naturels"),
                    s("ℤ", "entiers relatifs"),
                    s("𝔻", "décimaux"),
                    s("ℚ", "rationnels"),
                    s("ℝ", "réels"),
                    s("ℂ", "complexes"),
                    s("ℍ", "quaternions"),
                    s("ℙ", "nombres premiers"),
                ],
            },
            Groupe {
                nom: "Logique",
                symboles: &[
                    s("∀", "pour tout"),
                    s("∃", "il existe"),
                    s("∄", "il n'existe pas"),
                    s("¬", "non"),
                    s("∧", "et"),
                    s("∨", "ou"),
                    s("⊕", "ou exclusif"),
                    s("⇒", "implique"),
                    s("⇐", "est impliqué par"),
                    s("⇔", "équivaut à"),
                    s("⊢", "démontre"),
                    s("⊨", "satisfait"),
                    s("∴", "donc"),
                    s("∵", "car"),
                    s("∎", "CQFD"),
                ],
            },
        ],
    },
    Onglet {
        nom: "Grec",
        groupes: &[
            Groupe {
                nom: "Minuscules",
                symboles: &[
                    s("α", "alpha"),
                    s("β", "bêta"),
                    s("γ", "gamma"),
                    s("δ", "delta"),
                    s("ε", "epsilon"),
                    s("ζ", "dzêta"),
                    s("η", "êta"),
                    s("θ", "thêta"),
                    s("ι", "iota"),
                    s("κ", "kappa"),
                    s("λ", "lambda"),
                    s("μ", "mu"),
                    s("ν", "nu"),
                    s("ξ", "xi"),
                    s("ο", "omicron"),
                    s("π", "pi"),
                    s("ρ", "rhô"),
                    s("σ", "sigma"),
                    s("τ", "tau"),
                    s("υ", "upsilon"),
                    s("φ", "phi"),
                    s("χ", "khi"),
                    s("ψ", "psi"),
                    s("ω", "oméga"),
                ],
            },
            Groupe {
                nom: "Variantes",
                symboles: &[
                    s("ϵ", "epsilon lunaire"),
                    s("ϑ", "thêta cursif"),
                    s("ϕ", "phi droit"),
                    s("ϖ", "pi cursif"),
                    s("ϱ", "rhô cursif"),
                    s("ς", "sigma final"),
                ],
            },
            Groupe {
                nom: "Majuscules",
                symboles: &[
                    s("Γ", "Gamma"),
                    s("Δ", "Delta"),
                    s("Θ", "Thêta"),
                    s("Λ", "Lambda"),
                    s("Ξ", "Xi"),
                    s("Π", "Pi"),
                    s("Σ", "Sigma"),
                    s("Υ", "Upsilon"),
                    s("Φ", "Phi"),
                    s("Ψ", "Psi"),
                    s("Ω", "Oméga"),
                ],
            },
        ],
    },
    Onglet {
        nom: "Exposants",
        groupes: &[
            Groupe {
                nom: "Exposants",
                symboles: &[
                    s("⁰", "exposant 0"),
                    s("¹", "exposant 1"),
                    s("²", "exposant 2"),
                    s("³", "exposant 3"),
                    s("⁴", "exposant 4"),
                    s("⁵", "exposant 5"),
                    s("⁶", "exposant 6"),
                    s("⁷", "exposant 7"),
                    s("⁸", "exposant 8"),
                    s("⁹", "exposant 9"),
                    s("⁺", "exposant plus"),
                    s("⁻", "exposant moins"),
                    s("⁼", "exposant égal"),
                    s("⁽", "parenthèse"),
                    s("⁾", "parenthèse"),
                    s("ⁿ", "exposant n"),
                    s("ⁱ", "exposant i"),
                ],
            },
            Groupe {
                nom: "Indices",
                symboles: &[
                    s("₀", "indice 0"),
                    s("₁", "indice 1"),
                    s("₂", "indice 2"),
                    s("₃", "indice 3"),
                    s("₄", "indice 4"),
                    s("₅", "indice 5"),
                    s("₆", "indice 6"),
                    s("₇", "indice 7"),
                    s("₈", "indice 8"),
                    s("₉", "indice 9"),
                    s("₊", "indice plus"),
                    s("₋", "indice moins"),
                    s("₌", "indice égal"),
                    s("₍", "parenthèse"),
                    s("₎", "parenthèse"),
                    s("ₙ", "indice n"),
                    s("ᵢ", "indice i"),
                ],
            },
            Groupe {
                nom: "Fractions",
                symboles: &[
                    s("½", "un demi"),
                    s("⅓", "un tiers"),
                    s("⅔", "deux tiers"),
                    s("¼", "un quart"),
                    s("¾", "trois quarts"),
                    s("⅕", "un cinquième"),
                    s("⅙", "un sixième"),
                    s("⅛", "un huitième"),
                    s("⅜", "trois huitièmes"),
                    s("⅝", "cinq huitièmes"),
                    s("⅞", "sept huitièmes"),
                ],
            },
        ],
    },
    Onglet {
        nom: "Divers",
        groupes: &[
            Groupe {
                nom: "Délimiteurs",
                symboles: &[
                    s("⌊", "partie entière"),
                    s("⌋", "partie entière"),
                    s("⌈", "plafond"),
                    s("⌉", "plafond"),
                    s("⟨", "chevron"),
                    s("⟩", "chevron"),
                    s("‖", "norme"),
                    s("⟦", "intervalle entier"),
                    s("⟧", "intervalle entier"),
                ],
            },
            Groupe {
                nom: "Flèches",
                symboles: &[
                    s("→", "tend vers"),
                    s("↦", "associe"),
                    s("←", "gauche"),
                    s("↔", "double"),
                    s("↑", "haut"),
                    s("↓", "bas"),
                    s("↗", "croît"),
                    s("↘", "décroît"),
                    s("⟶", "longue"),
                    s("⟹", "longue double"),
                ],
            },
            Groupe {
                nom: "Constantes",
                symboles: &[
                    s("ℯ", "e de l'exponentielle"),
                    s("ℏ", "h barre"),
                    s("ℓ", "l cursif"),
                    s("ℵ", "aleph"),
                    s("ı", "i sans point"),
                    s("ȷ", "j sans point"),
                ],
            },
        ],
    },
];

/// Tous les symboles, dans l'ordre de la fenêtre : onglets, puis groupes — c'est
/// l'INDEX GLOBAL que la page émet au clic.
pub fn entrees() -> impl Iterator<Item = &'static Symbole> {
    ONGLETS.iter().flat_map(|o| o.groupes.iter()).flat_map(|g| g.symboles.iter())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⛔ Une case vide, ou un caractère du CLAVIER : le premier n'insérerait rien,
    /// le second n'a pas sa place ici (voir la doc de module).
    #[test]
    fn aucun_symbole_vide_ni_du_clavier() {
        for e in entrees() {
            assert!(!e.symbole.trim().is_empty(), "symbole vide ({})", e.nom);
            assert!(!e.nom.trim().is_empty(), "nom vide pour {:?}", e.symbole);
            assert!(!e.symbole.is_ascii(), "{:?} se tape au clavier", e.symbole);
        }
    }

    /// Un symbole n'apparaît qu'UNE fois dans toute la fenêtre — sauf les paires
    /// ouvrante/fermante, qui sont deux caractères différents. Un doublon serait
    /// deux cases pour un même caractère, sous deux noms.
    #[test]
    fn pas_de_doublon_dans_la_fenetre() {
        let mut vus = std::collections::BTreeMap::new();
        for e in entrees() {
            if let Some(avant) = vus.insert(e.symbole, e.nom) {
                panic!("{:?} deux fois : « {avant} » et « {} »", e.symbole, e.nom);
            }
        }
    }

    /// ⛔ Le MOINS est le signe mathématique, pas le trait d'union du clavier.
    #[test]
    fn le_moins_est_le_signe_moins() {
        assert!(entrees().any(|e| e.symbole == "\u{2212}"));
    }
}
