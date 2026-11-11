//! **L'APPLET « SYMBOLES TECHNIQUES » (EX-« MATHÉMATIQUES », RENOMMÉE LE
//! 2026-11-11) — QUATRE CATÉGORIES PRINCIPALES** : mathématiques, physique,
//! mécanique, technologie (DEMANDE EXPLICITE, 2026-11-11). Les anciens onglets
//! (opérateurs, relations, ensembles, grec, exposants, divers) sont devenus les
//! SOUS-CATÉGORIES de Mathématiques — ce sont les `Groupe` de cet onglet. La
//! fenêtre qui les montre est
//! `win32::cef_maths` — une page CEF de la famille des symboles phonétiques : un
//! clic FRAPPE le symbole dans la dernière fenêtre étrangère (voir
//! `win32::cible_de_frappe`), un clic droit le COPIE.
//!
//! DEMANDE EXPLICITE (2026-10-12) : « une nouvelle applet dans la zone intégrés
//! pour ouvrir une fenêtre permettant d'insérer les caractères mathématiques ».
//!
//! ⚠️ **LES CARACTÈRES DU CLAVIER SONT ACCEPTÉS DEPUIS LE 2026-11-11** (DEMANDE
//! EXPLICITE : « je change la règle, tu peux ajouter les caractères claviers »).
//! Ils ont leur place dans les onglets informatiques et techniques, où on les
//! cherche PAR LEUR NOM (`@`, `&&`, `->`) : les exclure obligeait à les taper de
//! mémoire. ⛔ Le MOINS reste U+2212, pas le trait d'union du clavier — c'est la
//! raison d'être de la case.
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

/// **LES QUATRE CATÉGORIES PRINCIPALES, DANS L'ORDRE DE LA FENÊTRE** — DEMANDE
/// EXPLICITE (2026-11-11) : « je voulais que tu crées des catégories principales
/// avec les onglets Mathématiques, Physique, Mécanique, Technologie […] et que tu
/// mettes les symboles mathématiques dans cette catégorie triés en sous
/// catégories ». Les anciens onglets (Opérateurs, Relations, Ensembles, Grec,
/// Exposants, Divers) sont donc devenus les SOUS-CATÉGORIES de Mathématiques,
/// c'est-à-dire les `Groupe` de cet onglet.
///
/// L'ordre des onglets EST celui que la page relit au clic (voir
/// `cef_maths::symbole_par_index`) — ajouter un onglet AU MILIEU décalerait le
/// rang de toutes les cases des suivants.
pub const ONGLETS: &[Onglet] = &[
    Onglet {
        nom: "Mathématiques",
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
                nom: "Algèbre",
                symboles: &[
                    s("⊗", "produit tensoriel"),
                    s("⊙", "produit de Hadamard"),
                    s("⊖", "moins cerclé"),
                    s("⊞", "plus encadré"),
                    s("⊟", "moins encadré"),
                    s("⊠", "croix encadrée"),
                    s("⋆", "étoile creuse"),
                    s("⋄", "losange creux"),
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
                    s("∯", "intégrale de surface"),
                    s("∰", "intégrale de volume"),
                    s("∱", "intégrale horaire"),
                    s("∲", "intégrale circulaire horaire"),
                    s("∳", "intégrale circulaire antihoraire"),
                    s("∂", "dérivée partielle"),
                    s("∇", "nabla"),
                    s("∆", "laplacien, écart"),
                    s("∞", "infini"),
                    s("′", "prime"),
                    s("″", "seconde"),
                    s("‾", "barre supérieure"),
                    s("⊤", "transposée"),
                ],
            },
            Groupe {
                nom: "Suites",
                symboles: &[
                    s("…", "ellipse"),
                    s("⋯", "points suspendus"),
                    s("⋮", "points verticaux"),
                    s("⋱", "points diagonaux"),
                ],
            },
            // ---- Les ex-ONGLETS ci-dessous sont devenus des SOUS-CATÉGORIES de
            // Mathématiques : c'est le même contenu, sans le niveau d'onglet.
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
                    // ⚠️ AUSSI dans Mécanique > Cotes et tracés : un caractère
                    // peut vivre dans plusieurs catégories (voir
                    // `pas_de_doublon_dans_un_onglet`).
                    s("⌀", "diamètre"),
                ],
            },
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
            Groupe {
                nom: "Grec — minuscules",
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
                nom: "Grec — variantes",
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
                nom: "Grec — majuscules",
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
    // ---- LES TROIS AUTRES CATÉGORIES PRINCIPALES (DEMANDE EXPLICITE,
    // 2026-11-11) — même table, même page : seuls les onglets changent.
    Onglet {
        nom: "Physique",
        // ⚠️ **LES MÊMES CARACTÈRES QUE MATHÉMATIQUES Y SONT VOULUS** (DEMANDE
        // EXPLICITE, 2026-11-11) : physique, mécanique et électricité partagent
        // les lettres grecques du calcul. Un onglet est un CONTEXTE DE TRAVAIL —
        // `λ` s'y insère comme longueur d'onde sans détour par Grec, et le nom
        // sous la case dit ce qu'elle vaut ICI. Voir `pas_de_doublon_dans_un_onglet`.
        groupes: &[
            Groupe {
                nom: "Unités de base (SI)",
                symboles: &[
                    s("m", "mètre"),
                    s("kg", "kilogramme"),
                    s("s", "seconde"),
                    s("A", "ampère"),
                    s("K", "kelvin"),
                    s("mol", "mole"),
                    s("cd", "candela"),
                ],
            },
            Groupe {
                nom: "Constantes physiques",
                symboles: &[
                    s("c", "célérité de la lumière"),
                    s("h", "constante de Planck"),
                    s("ℏ", "constante de Planck réduite"),
                    s("G", "constante gravitationnelle"),
                    s("g", "pesanteur"),
                    s("k", "constante de Boltzmann"),
                    s("R", "constante des gaz"),
                    s("e", "charge élémentaire"),
                ],
            },
            Groupe {
                nom: "Unités",
                symboles: &[
                    s("℃", "degré Celsius"),
                    s("℉", "degré Fahrenheit"),
                    s("Å", "ångström"),
                    s("ℓ", "litre"),
                    s("∿", "sinusoïdale"),
                ],
            },
            Groupe {
                nom: "Grandeurs",
                symboles: &[
                    s("α", "accélération angulaire"),
                    s("ω", "vitesse angulaire"),
                    s("τ", "moment de force"),
                    s("Δ", "variation"),
                    s("δ", "petite variation, incertitude"),
                    s("∝", "proportionnel à"),
                    s("≈", "environ égal"),
                ],
            },
            Groupe {
                nom: "Opérateurs",
                symboles: &[
                    s("∂", "dérivée partielle"),
                    s("∇", "gradient, nabla"),
                    s("⋅", "produit scalaire"),
                    s("×", "produit vectoriel"),
                    s("∞", "infini"),
                ],
            },
            Groupe {
                nom: "Ondes et thermodynamique",
                symboles: &[
                    s("λ", "longueur d'onde"),
                    s("μ", "frottement, perméabilité"),
                    s("η", "rendement, viscosité"),
                    s("ρ", "masse volumique, résistivité"),
                    s("⇌", "équilibre"),
                    s("⇋", "réaction réversible"),
                ],
            },
            Groupe {
                nom: "Électricité et magnétisme",
                symboles: &[
                    s("Ω", "ohm"),
                    s("Φ", "flux"),
                    s("ε", "permittivité"),
                    s("σ", "conductivité, contrainte"),
                    s("⏦", "courant alternatif"),
                    s("⎓", "courant continu"),
                    s("⏚", "terre"),
                    s("🧲", "aimant"),
                ],
            },
            Groupe {
                nom: "Vecteurs et normes",
                symboles: &[
                    // ⚠️ La flèche est un DIACRITIQUE (U+20D7) : posée après une
                    // lettre, elle la vectorise (`F` puis ceci donne F⃗).
                    s("⃗", "flèche de vecteur"),
                    s("∥", "parallèle"),
                    s("‖", "norme"),
                    s("⊥", "perpendiculaire"),
                    s("⟨", "ouvrante (bra)"),
                    s("⟩", "fermante (ket)"),
                ],
            },
            Groupe {
                nom: "Angles",
                symboles: &[
                    s("°", "degré"),
                    s("′", "minute d'angle"),
                    s("″", "seconde d'angle"),
                    s("∠", "angle"),
                    s("∡", "angle mesuré"),
                ],
            },
            Groupe {
                nom: "Matière et rayonnements",
                symboles: &[
                    s("⚛", "atome"),
                    s("☢", "radioactivité"),
                    s("☣", "danger biologique"),
                    s("¹H", "hydrogène 1"),
                    s("²H", "deutérium"),
                    s("³H", "tritium"),
                    s("⁴He", "hélium 4"),
                    s("¹²C", "carbone 12"),
                    s("¹⁴C", "carbone 14"),
                ],
            },
            Groupe {
                nom: "Optique",
                symboles: &[
                    s("ν", "fréquence"),
                    s("n", "indice de réfraction"),
                    s("θ", "angle d'incidence"),
                    s("φ", "déphasage"),
                    s("f", "distance focale"),
                ],
            },
            Groupe {
                nom: "Astronomie",
                symboles: &[
                    s("☉", "Soleil"),
                    s("☾", "Lune"),
                    s("★", "étoile"),
                    s("✦", "étoile à quatre branches"),
                    s("♃", "Jupiter"),
                    s("♄", "Saturne"),
                ],
            },
            Groupe {
                nom: "Chimie",
                symboles: &[
                    s("⚗", "alambic"),
                    s("⌬", "noyau benzénique"),
                    s("🧪", "tube à essai"),
                    s("🧬", "ADN"),
                ],
            },
        ],
    },
    Onglet {
        nom: "Mécanique",
        // ⚠️ **UN SEUL CARACTÈRE PAR ONGLET** (voir `pas_de_doublon_dans_un_onglet`) :
        // plusieurs grandeurs se disputent la même lettre. Elles sont donc réunies
        // sous UNE case dont le nom porte les deux sens — jamais deux cases.
        // Paires réunies : `W` travail/poids et watt ; `E` énergie et module
        // d'Young ; `M` moment et filetage métrique ; `T` couple, période et
        // température ; `ω` vitesse angulaire et pulsation ; `A` aire et
        // amplitude ; `J` joule et moment de torsion ; `m` masse et mètre.
        //
        // ⛔ **LES SCHÉMAS HYDRAULIQUES ET PNEUMATIQUES NE SONT PAS ICI** : pompe,
        // vérin, distributeur, ressort en zigzag (ISO 1219-1) ne sont pas des
        // caractères Unicode, ce sont des DESSINS. Aucune police ne les fournit
        // comme du texte : une case ne pourrait rien insérer. Ce qui a un point de
        // code (⚙, ⏚, ⌁) est ailleurs dans la table, à sa place logique.
        groupes: &[
            Groupe {
                nom: "Organes et liaisons",
                symboles: &[
                    s("🔩", "boulon"),
                    s("🔧", "clé"),
                    s("🔗", "maillon"),
                    s("⛓", "chaîne"),
                    s("⚙", "engrenage"),
                ],
            },
            Groupe {
                nom: "Rotations",
                symboles: &[
                    s("↻", "sens horaire"),
                    s("↺", "sens antihoraire"),
                    s("⟳", "rotation horaire"),
                    s("⟲", "rotation antihoraire"),
                    s("↷", "rotation ouverte horaire"),
                    s("↶", "rotation ouverte antihoraire"),
                ],
            },
            Groupe {
                nom: "Forces et directions",
                symboles: &[
                    s("→", "vers la droite"),
                    s("←", "vers la gauche"),
                    s("↑", "vers le haut"),
                    s("↓", "vers le bas"),
                    s("↗", "diagonale haut droite"),
                    s("↘", "diagonale bas droite"),
                    s("↙", "diagonale bas gauche"),
                    s("↔", "double sens"),
                ],
            },
            Groupe {
                nom: "Mouvements",
                symboles: &[s("↕", "va-et-vient"), s("⇕", "va-et-vient double")],
            },
            Groupe {
                nom: "Angles et repères",
                symboles: &[
                    s("∟", "angle droit"),
                    s("⊾", "angle droit avec arc"),
                    s("∠", "angle, angularité"),
                    s("⊕", "centre, axe"),
                ],
            },
            Groupe {
                nom: "Grandeurs mécaniques",
                symboles: &[
                    s("F", "force"),
                    s("m", "masse ; mètre"),
                    s("a", "accélération"),
                    s("v", "vitesse"),
                    s("t", "temps, épaisseur"),
                    s("W", "travail, poids ; watt"),
                    s("E", "énergie ; module d'Young"),
                    s("P", "puissance, pression"),
                    s("M", "moment de force ; filetage métrique"),
                    s("T", "couple, période, température"),
                    s("ω", "vitesse angulaire, pulsation"),
                    s("α", "accélération angulaire"),
                ],
            },
            Groupe {
                nom: "Résistance des matériaux",
                symboles: &[
                    s("σ", "contrainte normale"),
                    s("τ", "contrainte de cisaillement"),
                    s("ε", "déformation relative"),
                    s("γ", "déformation de cisaillement"),
                    s("ν", "coefficient de Poisson"),
                    s("ρ", "masse volumique"),
                    s("ℓ", "longueur"),
                    s("λ", "élancement"),
                    s("G", "module de cisaillement"),
                    s("A", "aire de la section ; amplitude"),
                    s("I", "moment d'inertie"),
                ],
            },
            Groupe {
                nom: "Vibrations",
                symboles: &[s("∿", "sinusoïde"), s("f", "fréquence"), s("φ", "phase")],
            },
            Groupe {
                nom: "Unités et grandeurs",
                symboles: &[
                    s("N", "newton"),
                    s("Pa", "pascal"),
                    s("J", "joule ; moment de torsion"),
                    s("N·m", "couple"),
                    s("m/s", "vitesse"),
                    s("m/s²", "accélération"),
                    s("rad", "radian"),
                    s("tr/min", "tours par minute"),
                    s("kg", "kilogramme"),
                    s("mm", "millimètre"),
                    s("µm", "micromètre"),
                ],
            },
            Groupe {
                nom: "Cotes et tracés",
                symboles: &[
                    s("⌀", "diamètre"),
                    s("C", "chanfrein"),
                    s("⌓", "arc de cercle ; profil de surface"),
                    s("⌔", "secteur circulaire"),
                    s("⌴", "fraisage conique"),
                    s("⌵", "lamage à fond plat"),
                    s("⌜", "coin haut gauche"),
                    s("⌝", "coin haut droit"),
                    s("⌞", "coin bas gauche"),
                    s("⌟", "coin bas droit"),
                ],
            },
            Groupe {
                nom: "Ajustements et états de surface",
                symboles: &[
                    s("H7", "alésage"),
                    s("h6", "arbre"),
                    s("g6", "arbre, jeu"),
                    s("±", "tolérance"),
                    s("Ø", "diamètre barré"),
                    s("SR", "rayon sphérique"),
                    s("R", "rayon, congé"),
                    s("□", "carré"),
                    s("Ra", "rugosité arithmétique"),
                    s("Rz", "hauteur des irrégularités"),
                ],
            },
            Groupe {
                nom: "Tolérances géométriques",
                symboles: &[
                    s("⌖", "position"),
                    s("⊥", "perpendicularité"),
                    s("∥", "parallélisme"),
                    s("◎", "coaxialité"),
                    s("○", "circularité"),
                    s("⌭", "cylindricité"),
                    s("⏤", "rectitude"),
                    s("⏥", "planéité"),
                    s("⌯", "symétrie"),
                    s("⌰", "battement total"),
                    s("⌒", "profil de ligne"),
                ],
            },
        ],
    },
    Onglet {
        nom: "Technologie",
        // ⚠️ **LES CARACTÈRES DU CLAVIER Y SONT, DEPUIS LE 2026-11-11** (DEMANDE
        // EXPLICITE : « je change la règle, tu peux ajouter les caractères
        // claviers ») — regroupés à part, sous les noms par lesquels on les
        // cherche (`@`, `&&`, `->`). Voir la doc de module.
        groupes: &[
            Groupe {
                nom: "Ordinateur",
                symboles: &[
                    s("⌘", "commande"),
                    s("⌥", "option"),
                    s("⌃", "contrôle"),
                    s("⇧", "majuscule"),
                    s("⎇", "touche Alt"),
                    s("⏎", "entrée"),
                    s("↵", "retour à la ligne"),
                    s("⌫", "retour arrière"),
                    s("⌦", "suppression"),
                    s("⎋", "échappement"),
                    s("⇥", "tabulation"),
                    s("⇤", "tabulation arrière"),
                    s("␣", "espace"),
                    s("⌗", "dièse"),
                    s("⌧", "effacer"),
                    s("⎙", "impression écran"),
                    s("⇞", "page précédente"),
                    s("⇟", "page suivante"),
                    s("⌨", "clavier"),
                ],
            },
            Groupe {
                nom: "Clavier",
                // ⚠️ Caractères ASCII : acceptés depuis le 2026-11-11 (voir la doc
                // de module). `\\` = un antislash ; les paires sont UNE case.
                symboles: &[
                    s("@", "arobase"),
                    s("#", "dièse, hashtag, commentaire"),
                    s("&", "esperluette"),
                    s("*", "astérisque, joker"),
                    s("/", "barre oblique"),
                    s("\\", "antislash"),
                    s("|", "barre verticale, tube"),
                    s("~", "tilde"),
                    s("^", "accent circonflexe, XOR"),
                    s("%", "pourcentage, modulo"),
                    s("!", "point d'exclamation, non"),
                    s("?", "point d'interrogation"),
                    s("=", "égal, affectation"),
                    s("<>", "chevrons"),
                    s("[]", "crochets"),
                    s("{}", "accolades"),
                    s("()", "parenthèses"),
                ],
            },
            Groupe {
                nom: "Programmation",
                symboles: &[
                    s("==", "égalité"),
                    s("!=", "différent de"),
                    s("<=", "inférieur ou égal"),
                    s(">=", "supérieur ou égal"),
                    s("&&", "ET logique"),
                    s("||", "OU logique"),
                    s("++", "incrément"),
                    s("--", "décrément"),
                    s("+=", "addition puis affecte"),
                    s("->", "accès à un membre"),
                    s("::", "portée, module"),
                    s("//", "commentaire de ligne"),
                    s("/* */", "commentaire de bloc"),
                    s("¬", "NON logique"),
                    s("∧", "ET logique"),
                    s("∨", "OU logique"),
                    s("⊕", "OU exclusif"),
                    s("∈", "appartient à"),
                    s("∉", "n'appartient pas à"),
                    s("∅", "ensemble vide"),
                ],
            },
            Groupe {
                nom: "Média",
                symboles: &[
                    s("▶", "lecture"),
                    s("⏸", "pause"),
                    s("⏯", "lecture-pause"),
                    s("⏹", "arrêt"),
                    s("⏺", "enregistrement"),
                    s("⏏", "éjection"),
                    s("⏮", "piste précédente"),
                    s("⏭", "piste suivante"),
                ],
            },
            Groupe {
                nom: "Réseau",
                symboles: &[
                    s("🌐", "web"),
                    s("☁", "nuage"),
                    s("⇄", "échange"),
                    s("🔌", "prise"),
                    s("📡", "antenne"),
                ],
            },
            Groupe {
                nom: "Appareils",
                symboles: &[
                    s("🔒", "verrouillage"),
                    s("🔋", "batterie"),
                    s("📶", "qualité du signal"),
                ],
            },
            Groupe {
                nom: "Système et fichiers",
                symboles: &[
                    s("🖥", "ordinateur"),
                    s("💾", "sauvegarde"),
                    s("🖨", "imprimante"),
                    s("🗑", "corbeille"),
                    s("📁", "dossier"),
                    s("📄", "fichier"),
                    s("🔓", "déverrouillage"),
                    s("🛡", "protection"),
                    s("⚙", "paramètres"),
                ],
            },
            Groupe {
                nom: "Électricité",
                // ⛔ **`⏼` (U+23FC) N'EST PAS ICI** : dans la police du système il
                // se dessine comme un ① cerclé, sans rapport avec « veille » — une
                // case qui ne montre pas ce qu'elle nomme vaut moins que rien.
                // Les trois états retenus sont ceux que la police dessine juste
                // (`⏻` marche-arrêt, `⏽` marche, `⏾` veille).
                symboles: &[
                    s("⏻", "marche-arrêt"),
                    s("⏽", "marche"),
                    s("⏾", "veille"),
                    s("⏦", "courant alternatif"),
                    s("⎓", "courant continu"),
                    s("⏚", "terre"),
                    s("⏛", "masse châssis"),
                    s("Ω", "ohm"),
                    // Les unités du schéma (caractères du clavier, acceptés depuis
                    // le 2026-11-11).
                    s("µF", "microfarad"),
                    s("V", "volt"),
                    s("A", "ampère"),
                    s("W", "watt"),
                    s("Hz", "hertz"),
                    s("LED", "diode électroluminescente"),
                    s("GND", "masse, référence"),
                ],
            },
            Groupe {
                nom: "Sécurité",
                symboles: &[
                    s("⚠", "avertissement"),
                    s("⚡", "danger électrique"),
                    s("♻", "recyclage"),
                    s("☢", "rayonnement ionisant"),
                    s("☣", "risque biologique"),
                    s("CE", "conformité européenne"),
                ],
            },
            Groupe {
                nom: "Dessin technique",
                symboles: &[
                    s("⌀", "diamètre"),
                    s("Ø", "diamètre barré"),
                    s("R", "rayon"),
                    s("M", "filetage métrique"),
                    s("Ra", "rugosité moyenne"),
                    s("⌖", "position, centrage"),
                    s("±", "tolérance"),
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

    /// ⛔ Une case vide n'insérerait rien, et une case sans nom serait muette.
    /// ⚠️ Les caractères ASCII ne sont PLUS refusés depuis le 2026-11-11 (voir la
    /// doc de module).
    #[test]
    fn aucun_symbole_vide() {
        for e in entrees() {
            assert!(!e.symbole.trim().is_empty(), "symbole vide ({})", e.nom);
            assert!(!e.nom.trim().is_empty(), "nom vide pour {:?}", e.symbole);
        }
    }

    /// Un symbole n'apparaît qu'UNE fois DANS UN MÊME ONGLET — deux cases pour un
    /// même caractère, sous deux noms, resteraient une redite.
    ///
    /// ⚠️ **ENTRE ONGLETS, LE DOUBLON EST VOULU** (2026-11-11) : chaque catégorie
    /// est un contexte de travail autonome. `λ` est la longueur d'onde dans
    /// Physique et une lettre grecque dans Mathématiques ; renvoyer d'un onglet à
    /// l'autre pour l'insérer serait une brimade. Ce qui resterait fautif, c'est
    /// le même caractère deux fois dans le même onglet.
    #[test]
    fn pas_de_doublon_dans_un_onglet() {
        for onglet in ONGLETS {
            let mut vus = std::collections::BTreeMap::new();
            for e in onglet.groupes.iter().flat_map(|g| g.symboles.iter()) {
                if let Some(avant) = vus.insert(e.symbole, e.nom) {
                    panic!(
                        "« {} » : {:?} deux fois : « {avant} » et « {} »",
                        onglet.nom, e.symbole, e.nom
                    );
                }
            }
        }
    }

    /// **L'ONGLET PHYSIQUE PORTE LES GRANDEURS DU COURS** (DEMANDE EXPLICITE,
    /// 2026-11-11) : mécanique, ondes et thermodynamique, électricité — et les
    /// vecteurs, sans lesquels la flèche de vecteur manquerait à l'appel.
    #[test]
    fn la_physique_porte_les_grandeurs_du_cours() {
        let physique = &ONGLETS[1];
        assert_eq!(physique.nom, "Physique");
        let vus: Vec<&str> = physique
            .groupes
            .iter()
            .flat_map(|g| g.symboles.iter())
            .map(|e| e.symbole)
            .collect();
        for attendu in [
            "α", "ω", "τ", "Δ", "δ", "∝", "∂", "∇", "λ", "μ", "η", "ρ", "Ω", "Φ", "ε", "σ", "⃗",
            "∥", "‖", "⊥", "°", "☣",
        ] {
            assert!(vus.contains(&attendu), "{attendu:?} manque à l'onglet Physique");
        }
    }

    /// ⛔ Le MOINS est le signe mathématique, pas le trait d'union du clavier.
    #[test]
    fn le_moins_est_le_signe_moins() {
        assert!(entrees().any(|e| e.symbole == "\u{2212}"));
    }
}
