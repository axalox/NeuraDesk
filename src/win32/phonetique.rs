//! **LES SYMBOLES PHONÉTIQUES, EN DEUX LANGUES.** La fenêtre qui les montre est
//! `win32::cef_phonetique` — une page CEF de la famille des kaomojis : un clic
//! FRAPPE le symbole dans la dernière fenêtre étrangère (voir
//! `win32::cible_de_frappe`), un clic droit le COPIE.
//!
//! DEMANDE EXPLICITE (2026-10-11) : « une applet qui ouvre une fenêtre pour les
//! symboles de phonétique en deux onglets, un pour le français inspiré de […] Le
//! Robert, et un pour l'anglais inspiré de […] Cambridge ; il faut que les symboles
//! soient cliquables pour les insérer facilement dans un autre programme ».
//!
//! ⭐ **LES DEUX TABLES SONT RELEVÉES, PAS RECONSTITUÉES** (2026-10-11) :
//!   - français : `grandrobert.lerobert.com/AideGR/Pages/TableAPI.html` — l'ordre et
//!     les exemples de la page, plus `ʀ` que sa remarque 4 donne pour `[x]` ;
//!   - anglais : `dictionary.cambridge.org/help/phonetics.html` — l'ordre de la page,
//!     avec ses marques UK / US, et la marque de longueur `ː` en plus.
//!
//! ⚠️ **LES DEUX DICTIONNAIRES ÉCRIVENT `g` ET `r` EN ASCII**, pas `ɡ` (U+0261) ni
//! `ʁ` : c'est leur notation, et c'est elle qu'on reproduit. Le Robert écrit même le
//! r français en CAPITALE (`[R]`).
//!
//! ⚠️ **SIX SYMBOLES PORTENT UN DIACRITIQUE COMBINANT** (`ɛ̃`, `ɑ̃`, `ɔ̃`, `œ̃` en
//! français, `t̬` et `ɒ̃` en anglais) : la chaîne fait deux points de code, et elle
//! part EN UNE FOIS — `caracteres::frapper` prend une chaîne entière.

/// **UN SYMBOLE** : ce qui est inséré, l'exemple qui le fait entendre, et une note
/// courte (« UK », « US », « e fermé ») — vide s'il n'y en a pas.
pub struct Symbole {
    pub symbole: &'static str,
    pub exemple: &'static str,
    pub note: &'static str,
}

/// Un groupe de la fenêtre : son nom affiché, et ses symboles dans l'ordre de la page.
pub struct Groupe {
    pub nom: &'static str,
    pub symboles: &'static [Symbole],
}

/// Un onglet : sa langue, et ses groupes.
pub struct Langue {
    pub nom: &'static str,
    /// Ce que dit la note de bas d'onglet — d'où vient la table.
    pub source: &'static str,
    pub groupes: &'static [Groupe],
}

const fn s(symbole: &'static str, exemple: &'static str, note: &'static str) -> Symbole {
    Symbole { symbole, exemple, note }
}

/// **LES DEUX ONGLETS, DANS L'ORDRE DE LA FENÊTRE.** L'ordre EST celui que la page
/// relit au clic (voir `cef_phonetique::symbole_par_index`).
pub const LANGUES: &[Langue] = &[
    Langue {
        nom: "Français",
        source: "D'après la table de l'alphabet phonétique du Grand Robert",
        groupes: &[
            Groupe {
                nom: "Voyelles orales",
                symboles: &[
                    s("i", "épi, il, lyre", ""),
                    s("e", "aller, blé, chez", "e fermé"),
                    s("ɛ", "merci, lait, fête", "e ouvert"),
                    s("a", "ami, patte", "a antérieur"),
                    s("ɑ", "pâte, pas", "a postérieur"),
                    s("ɔ", "fort, donner, sol", "o ouvert"),
                    s("o", "mot, dôme", "o fermé"),
                    s("u", "genou, roue", ""),
                    s("y", "rue, vêtu", ""),
                    s("ø", "peu, deux", ""),
                    s("œ", "peur, meuble", ""),
                    s("ə", "premier", "e caduc"),
                ],
            },
            Groupe {
                nom: "Voyelles nasales",
                symboles: &[
                    s("ɛ̃", "bain, brin, plein", ""),
                    s("ɑ̃", "sans, vent", ""),
                    s("ɔ̃", "bonté, ton, ombre", ""),
                    s("œ̃", "lundi, brun, parfum", ""),
                ],
            },
            Groupe {
                nom: "Semi-consonnes",
                symboles: &[
                    s("j", "paille, yeux, pied, panier", ""),
                    s("w", "oui, fouet, joua, joie", ""),
                    s("ɥ", "huile, lui", ""),
                ],
            },
            Groupe {
                nom: "Consonnes",
                symboles: &[
                    s("p", "père, soupe", ""),
                    s("t", "terre, vite", ""),
                    s("k", "cou, qui, sac, képi", ""),
                    s("b", "bon, robe", ""),
                    s("d", "dans, aide", ""),
                    s("g", "gare, bague, gui", ""),
                    s("f", "feu, neuf, photo", ""),
                    s("s", "sale, celui, ça, tasse", ""),
                    s("ʃ", "chat, tache, schéma", ""),
                    s("v", "vous, rêve", ""),
                    s("z", "zéro, maison, rose", ""),
                    s("ʒ", "gilet, je, geôle", ""),
                    s("l", "lent, sol", ""),
                    s("R", "rue, venir", ""),
                    s("m", "mot, flamme", ""),
                    s("n", "nous, tonne, animal", ""),
                    s("ɲ", "agneau, vigne", ""),
                    s("ŋ", "camping", "mots anglais"),
                    s("x", "jota, khamsin", "mots espagnols, arabes"),
                    s("h", "hop !", "exclamatif"),
                ],
            },
            Groupe {
                nom: "Signes",
                symboles: &[
                    s("'", "le héros, les onze", "ni liaison ni élision"),
                    s("ʀ", "remplace parfois [x]", ""),
                ],
            },
        ],
    },
    Langue {
        nom: "Anglais",
        source: "D'après la table des symboles phonétiques du Cambridge Dictionary",
        groupes: &[
            Groupe {
                nom: "Voyelles",
                symboles: &[
                    s("iː", "sheep", ""),
                    s("ɪ", "ship", ""),
                    s("ɑː", "father", ""),
                    s("æ", "hat", ""),
                    s("ʌ", "cup", ""),
                    s("ɔː", "horse", ""),
                    s("ɒ", "sock", "UK"),
                    s("uː", "blue", ""),
                    s("ʊ", "foot", ""),
                    s("e", "head", ""),
                    s("ɜː", "bird", "UK"),
                    s("ɝː", "bird", "US"),
                    s("ə", "above", ""),
                    s("ɚ", "mother", "US"),
                ],
            },
            Groupe {
                nom: "Diphtongues",
                symboles: &[
                    s("eɪ", "day", ""),
                    s("aɪ", "eye", ""),
                    s("ɔɪ", "boy", ""),
                    s("əʊ", "nose", "UK"),
                    s("oʊ", "nose", "US"),
                    s("aʊ", "mouth", ""),
                    s("ɪə", "ear", "UK"),
                    s("eə", "hair", "UK"),
                    s("ʊə", "pure", "UK"),
                    s("aɪə", "fire", "UK"),
                    s("aʊə", "hour", "UK"),
                ],
            },
            Groupe {
                nom: "Consonnes",
                symboles: &[
                    s("p", "pen", ""),
                    s("b", "book", ""),
                    s("t", "town", ""),
                    s("t̬", "cutting", "US"),
                    s("d", "day", ""),
                    s("k", "cat", ""),
                    s("g", "give", ""),
                    s("f", "fish", ""),
                    s("v", "very", ""),
                    s("θ", "think", ""),
                    s("ð", "this", ""),
                    s("s", "say", ""),
                    s("z", "zoo", ""),
                    s("ʃ", "she", ""),
                    s("ʒ", "vision", ""),
                    s("tʃ", "cheese", ""),
                    s("dʒ", "jump", ""),
                    s("m", "moon", ""),
                    s("n", "name", ""),
                    s("ŋ", "sing", ""),
                    s("l", "look", ""),
                    s("r", "run", ""),
                    s("w", "we", ""),
                    s("j", "yes", ""),
                    s("h", "hand", ""),
                ],
            },
            Groupe {
                nom: "Autres sons",
                symboles: &[
                    s("i", "happy", "inaccentué"),
                    s("u", "situation", "inaccentué"),
                    s("ɒ̃", "croissant", "emprunt"),
                    s("x", "loch", "emprunt"),
                    s("əl", "label", ""),
                    s("əm", "criticism", ""),
                    s("ən", "sudden", ""),
                    s("ər", "dictionary", "UK"),
                    s("ʳ", "four apples", "UK, r de liaison"),
                ],
            },
            Groupe {
                nom: "Accent et syllabes",
                symboles: &[
                    s("ˈ", "surprise", "accent principal"),
                    s("ˌ", "retell", "accent secondaire"),
                    s(".", "sys.tem", "coupe syllabique"),
                    s("·", "sys·tem", "coupe, US"),
                    s("ː", "sheep", "voyelle longue"),
                ],
            },
        ],
    },
];

/// Tous les symboles, dans l'ordre de la fenêtre : onglets, puis groupes — c'est
/// l'INDEX GLOBAL que la page émet au clic.
pub fn entrees() -> impl Iterator<Item = &'static Symbole> {
    LANGUES.iter().flat_map(|l| l.groupes.iter()).flat_map(|g| g.symboles.iter())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les comptes des deux pages, figés : un symbole perdu en recopiant ne se
    /// verrait qu'à l'usage.
    #[test]
    fn les_deux_tables_ont_les_comptes_de_leur_source() {
        let compte = |l: &Langue| l.groupes.iter().map(|g| g.symboles.len()).sum::<usize>();
        // Le Robert : 16 voyelles, 3 semi-consonnes, 20 consonnes, `'` — plus `ʀ`.
        assert_eq!(compte(&LANGUES[0]), 41);
        // Cambridge : 25 voyelles et diphtongues, 25 consonnes, 9 autres sons,
        // 4 marques d'accent et de syllabe — plus `ː`.
        assert_eq!(compte(&LANGUES[1]), 64);
    }

    /// ⛔ Une case vide, ou un symbole en `[…]` ou `/…/` : le clic insérerait les
    /// délimiteurs avec, ce que personne ne veut au milieu d'une transcription.
    #[test]
    fn aucun_symbole_vide_ni_delimite() {
        for e in entrees() {
            assert!(!e.symbole.trim().is_empty(), "symbole vide ({})", e.exemple);
            assert!(!e.exemple.trim().is_empty(), "exemple vide pour {:?}", e.symbole);
            assert!(!e.symbole.contains(['[', ']', '/']), "délimiteur dans {:?}", e.symbole);
        }
    }

    /// Les diacritiques COMBINANTS sont bien là — une saisie qui les aurait perdus
    /// donnerait un `ɑ` à la place du `ɑ̃` de « sans », sans que l'œil le voie dans
    /// le source.
    #[test]
    fn les_nasales_portent_leur_tilde_combinant() {
        let combinants: Vec<&str> =
            entrees().map(|e| e.symbole).filter(|s| s.chars().any(|c| ('\u{300}'..='\u{36F}').contains(&c))).collect();
        assert_eq!(combinants, ["ɛ̃", "ɑ̃", "ɔ̃", "œ̃", "t̬", "ɒ̃"]);
    }

    /// Dans un groupe, un symbole n'apparaît qu'une fois — sauf les marques UK/US,
    /// qui distinguent deux cases au même symbole d'exemple, jamais au même symbole.
    #[test]
    fn pas_de_doublon_dans_un_groupe() {
        for l in LANGUES {
            for g in l.groupes {
                let vus: std::collections::BTreeSet<&str> = g.symboles.iter().map(|e| e.symbole).collect();
                assert_eq!(vus.len(), g.symboles.len(), "doublon dans « {} » ({})", g.nom, l.nom);
            }
        }
    }
}
