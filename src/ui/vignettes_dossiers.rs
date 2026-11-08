//! Les vignettes PRÊTES d'un raccourci de dossier — un jeu de catégories
//! dessinées à la volée, pour ne pas avoir à chercher une image quand on veut
//! juste qu'un dossier se repère.
//!
//! ⛔ **DESSINÉES, PAS EMBARQUÉES** (décision utilisateur) : chaque catégorie est
//! un glyphe Segoe Fluent Icons posé dans un carré, teinté de sa couleur. Aucun
//! fichier binaire à fabriquer ni à maintenir, net à toutes les tailles, et une
//! couleur par catégorie donne un jeu **cohérent** — ce que les icônes du shell,
//! elles, ne donnent pas : `Documents`, `Images` et `Musique` y viennent de trois
//! familles graphiques différentes.
//!
//! **ET ELLES SONT RANGÉES COMME N'IMPORTE QUELLE AUTRE VIGNETTE** — rendues en
//! RGBA, encodées en PNG, écrites dans `dossiers_images/` par
//! `config::store_dossier_vignette_from_rgba`, puis référencées par le même
//! `vignette: Option<String>` que les images choisies à la main. C'est ce qui
//! fait qu'AUCUN autre morceau du pipeline ne change : ni le chargement, ni le
//! téléversement, ni le remplacement à chaud (voir `app::EmplacementsDossiers`).
//!
//! ⚠️ **LE GLYPHE SE CHOISIT EN LE REGARDANT RENDU, JAMAIS SUR SON NOM** — la
//! règle du dépôt, payée une fois : `\u{E790}` s'appelle « Color » et dessine une
//! PALETTE. La sonde ci-dessous (`sonde_des_glyphes_de_categorie`) rend les
//! candidats en ASCII, et c'est elle qui a décidé chaque ligne du tableau.
//! Lancer : `cargo test --bins -- --ignored --nocapture sonde_des_glyphes_de_categorie`

/// **UNE CATÉGORIE PRÊTE** — son libellé (ce que la galerie affiche), le point de
/// code Segoe Fluent Icons de sa vignette, et sa couleur.
///
/// `couleur` : le glyphe est rendu en blanc, puis TEINTÉ — c'est ce qui distingue
/// deux catégories au premier coup d'œil, et ce qui fait qu'un dossier de musique
/// ne ressemble pas à un dossier d'images.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VignettePrete {
    pub libelle: &'static str,
    pub glyphe: char,
    pub couleur: [u8; 3],
    /// **LA SECONDE TEINTE, POUR UN FONDU VERTICAL** — `None` = aplat franc, le cas
    /// de toutes les vignettes d'avant le 2026-10-01.
    ///
    /// ⚠️ **LE FONDU VA DU HAUT (`couleur`) VERS LE BAS (`degrade`)**, sur la hauteur
    /// du GLYPHE rendu — pas sur la vignette : l'encre ne remplit que la forme, et un
    /// fondu calculé sur le carré entier laisserait les bords moins teintés que le
    /// centre. Voir `teinte_melangee`, qui porte l'interpolation et se teste seule.
    pub degrade: Option<[u8; 3]>,
}

/// **LA TEINTE D'UN POINT D'UN FONDU** — `t` va de `0.0` (la première teinte) à
/// `1.0` (la seconde).
///
/// ⚠️ **FONCTION PURE, ET C'EST DÉLIBÉRÉ** : un fondu faux — un canal qui sature, un
/// `t` inversé, une interpolation oubliée — ne se verrait qu'à l'œil, sur une
/// vignette de 64 px, au fond d'un menu. Ici, il se mesure.
///
/// L'arrondi est au plus proche, et non une troncature : `a + (b − a) × t` rend
/// exactement `a` en `t = 0` et `b` en `t = 1`, ce qui est le contrat.
pub(crate) fn teinte_melangee(depuis: [u8; 3], vers: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let canal = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
    [
        canal(depuis[0], vers[0]),
        canal(depuis[1], vers[1]),
        canal(depuis[2], vers[2]),
    ]
}

/// **LES CATÉGORIES PROPOSÉES**, dans l'ordre où la galerie les affiche — les
/// plus courantes d'abord.
///
/// ⚠️ **CHAQUE LIGNE A ÉTÉ VUE RENDUE, ET CE COMMENTAIRE EST LA TRACE DE CE QUI A
/// ÉTÉ VU** — c'est ce qui évite d'avoir à refaire la sonde pour savoir si un
/// glyphe a été vérifié ou supposé :
///
/// | Catégorie | Ce que la sonde a dessiné |
/// | --- | --- |
/// | `E8A5` | une page à coin plié |
/// | `EB9F` | un cadre avec une montagne (déjà l'icône de l'éditeur d'image) |
/// | `E8D6` | une NOTE de musique — préférée à `E767`, qui est un haut-parleur |
/// | `E714` | un rectangle à bord droit strié : une pellicule |
/// | `E896` | une flèche descendante au-dessus d'un plateau |
/// | `E7F8` | un panneau au-dessus d'une ligne (déjà « afficher le bureau ») |
/// | `E7FC` | une manette : croix directionnelle à gauche, boutons à droite |
/// | `E756` | une fenêtre à invite (déjà l'icône du terminal) |
/// | `E753` | un nuage |
/// | `E735` | une étoile PLEINE — `E734` est la même, en contour, trop fine à 64 px |
///
/// ⛔ **ÉCARTÉS PARCE QUE LA SONDE A MONTÉ QU'ILS SONT DES DOUBLONS** : `E91B`
/// dessine exactement `EB9F`, `E118` exactement `E896`, `E7C3` exactement
/// `E8A5`. Les prendre « pour varier » n'aurait rien varié du tout.
/// **DIX DE PLUS, LE 2026-10-01 — pour couvrir ce que le catalogue ne savait pas
/// dire** (travail, livres, archives, sauvegardes, sécurité, santé, voyages,
/// finance, partage, outils). Tous VUS RENDUS par la sonde, comme les dix premiers :
///
/// | Catégorie | Ce que la sonde a dessiné |
/// | --- | --- |
/// | `E821` | une MALLETTE : poignée au-dessus, corps large |
/// | `E8F1` | TROIS LIVRES debout, dos et tranches visibles |
/// | `E7B8` | une BOÎTE fermée par un couvercle, avec sa poignée |
/// | `E74E` | une DISQUETTE : volet métallique et étiquette |
/// | `EA18` | un BOUCLIER |
/// | `E95E` | un CŒUR portant une CROIX — le signe médical |
/// | `E709` | un AVION : ailes et fuselage |
/// | `E9D2` | un HISTOGRAMME sur deux axes |
/// | `E716` | DEUX PERSONNES côte à côte |
/// | `E713` | un ENGRENAGE |
///
/// ⚠️ **TROIS AUTRES ÉCARTÉS SUR LE DESSIN, PAS SUR LE NOM** : `EB51` (cœur en
/// CONTOUR) et `EB52` (cœur PLEIN) disent la même chose qu'`E95E` en moins précis
/// — c'est la CROIX qui fait lire « santé » ; et `E785` est un cadenas OUVERT,
/// voisin d'`E72E` dans une galerie : deux tuiles qu'on confondrait.
pub const CATEGORIES: [VignettePrete; 74] = [
    VignettePrete { libelle: "Documents", glyphe: '\u{E8A5}', couleur: [0x5B, 0x8D, 0xEF], degrade: None },
    VignettePrete { libelle: "Images", glyphe: '\u{EB9F}', couleur: [0xE0, 0x7A, 0x3C], degrade: None },
    VignettePrete { libelle: "Musique", glyphe: '\u{E8D6}', couleur: [0xC0, 0x5C, 0xC0], degrade: None },
    VignettePrete { libelle: "Vidéos", glyphe: '\u{E714}', couleur: [0xD9, 0x53, 0x4F], degrade: None },
    VignettePrete { libelle: "Téléchargements", glyphe: '\u{E896}', couleur: [0x4C, 0xA0, 0x6A], degrade: None },
    VignettePrete { libelle: "Bureau", glyphe: '\u{E7F8}', couleur: [0x3F, 0xA9, 0xB0], degrade: None },
    VignettePrete { libelle: "Jeux", glyphe: '\u{E7FC}', couleur: [0x8A, 0x6D, 0xD4], degrade: None },
    VignettePrete { libelle: "Développement", glyphe: '\u{E756}', couleur: [0x6E, 0x7B, 0x8A], degrade: None },
    VignettePrete { libelle: "Cloud", glyphe: '\u{E753}', couleur: [0x4A, 0x90, 0xD9], degrade: None },
    VignettePrete { libelle: "Favoris", glyphe: '\u{E735}', couleur: [0xD9, 0xB1, 0x3F], degrade: None },
    // ---- PREMIÈRE VAGUE D'AJOUTS, LE 2026-10-01 (voir la table ci-dessus) ----
    VignettePrete { libelle: "Travail", glyphe: '\u{E821}', couleur: [0x4A, 0x6B, 0x8A], degrade: None },
    VignettePrete { libelle: "Livres", glyphe: '\u{E8F1}', couleur: [0xA8, 0x6B, 0x3C], degrade: None },
    VignettePrete { libelle: "Archives", glyphe: '\u{E7B8}', couleur: [0x8A, 0x7B, 0x5B], degrade: None },
    VignettePrete { libelle: "Sauvegardes", glyphe: '\u{E74E}', couleur: [0x5B, 0x8A, 0x6B], degrade: None },
    VignettePrete { libelle: "Sécurité", glyphe: '\u{EA18}', couleur: [0xC9, 0x5B, 0x5B], degrade: None },
    VignettePrete { libelle: "Santé", glyphe: '\u{E95E}', couleur: [0xD9, 0x6A, 0x6A], degrade: None },
    VignettePrete { libelle: "Voyages", glyphe: '\u{E709}', couleur: [0x3C, 0x9B, 0xC9], degrade: None },
    VignettePrete { libelle: "Finance", glyphe: '\u{E9D2}', couleur: [0x4C, 0x8A, 0x4C], degrade: None },
    VignettePrete { libelle: "Partage", glyphe: '\u{E716}', couleur: [0x7B, 0x6B, 0xC9], degrade: None },
    VignettePrete { libelle: "Outils", glyphe: '\u{E713}', couleur: [0x78, 0x7B, 0x82], degrade: None },
    // ---- DEUXIÈME VAGUE, MÊME JOUR : tous vus rendus par la sonde ----
    VignettePrete { libelle: "Maison", glyphe: '\u{E80F}', couleur: [0x8A, 0x6B, 0x5B], degrade: None },
    VignettePrete { libelle: "Voiture", glyphe: '\u{E804}', couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    VignettePrete { libelle: "Sport", glyphe: '\u{E805}', couleur: [0x4C, 0x9B, 0x6B], degrade: None },
    VignettePrete { libelle: "Courrier", glyphe: '\u{E715}', couleur: [0x5B, 0x8A, 0xC9], degrade: None },
    VignettePrete { libelle: "Messages", glyphe: '\u{E8BD}', couleur: [0x6B, 0x9B, 0xC9], degrade: None },
    VignettePrete { libelle: "Téléphone", glyphe: '\u{E717}', couleur: [0x5B, 0xA8, 0x7B], degrade: None },
    VignettePrete { libelle: "Photos", glyphe: '\u{E722}', couleur: [0xC9, 0x7B, 0x5B], degrade: None },
    VignettePrete { libelle: "Audio", glyphe: '\u{E720}', couleur: [0x9B, 0x5B, 0xC9], degrade: None },
    VignettePrete { libelle: "Calculatrice", glyphe: '\u{E8EF}', couleur: [0x7B, 0x8A, 0x9B], degrade: None },
    VignettePrete { libelle: "Corbeille", glyphe: '\u{E74D}', couleur: [0x8A, 0x7B, 0x7B], degrade: None },
    // ---- TROISIÈME VAGUE, MÊME JOUR ----
    VignettePrete { libelle: "Minuteur", glyphe: '\u{E916}', couleur: [0xC9, 0x5B, 0x3C], degrade: None },
    VignettePrete { libelle: "Agenda", glyphe: '\u{E8C0}', couleur: [0x5B, 0x6B, 0xC9], degrade: None },
    VignettePrete { libelle: "Tâches", glyphe: '\u{E9D5}', couleur: [0x4C, 0x9B, 0x8A], degrade: None },
    VignettePrete { libelle: "Achats", glyphe: '\u{E719}', couleur: [0xC9, 0x8A, 0x3C], degrade: None },
    VignettePrete { libelle: "Banque", glyphe: '\u{E825}', couleur: [0x3C, 0x8A, 0x6B], degrade: None },
    VignettePrete { libelle: "Palette", glyphe: '\u{E790}', couleur: [0xB0, 0x4C, 0x8A], degrade: None },
    VignettePrete { libelle: "Découpe", glyphe: '\u{E8C6}', couleur: [0x8A, 0x8A, 0x9B], degrade: None },
    VignettePrete { libelle: "Impression", glyphe: '\u{E749}', couleur: [0x6B, 0x7B, 0x8A], degrade: None },
    VignettePrete { libelle: "Cartes", glyphe: '\u{E707}', couleur: [0x3C, 0x9B, 0x6B], degrade: None },
    VignettePrete { libelle: "Nuit", glyphe: '\u{E708}', couleur: [0x4A, 0x4A, 0x8A], degrade: None },
    // =====================================================================
    // QUATRIÈME VAGUE — ISSUE D'UN BALAYAGE, PAS D'UNE LISTE ÉCRITE À LA MAIN.
    //
    // ⚠️ **LES DEUX PLAGES `E700..=E8FF` ET `E900..=EAFF` ONT ÉTÉ BALAYÉES**, et
    // 189 dessins substantiels en sont sortis. La très grande majorité est du
    // MOBILIER D'INTERFACE — grilles de points, dispositions, barres, fenêtres — qui
    // ne dit rien de ce que contient un dossier. **Vingt-sept seulement sont
    // signifiants**, et ce sont ceux-là. Le reste n'a pas été gardé pour atteindre un
    // compte : une tuile « Tableau » ou « Panneau » serait un remplissage, et ce
    // dépôt paie déjà assez cher les listes qu'on allonge pour la forme.
    // =====================================================================
    VignettePrete { libelle: "Liste", glyphe: '\u{E7AC}', couleur: [0x6B, 0x8A, 0x9B], degrade: None },
    VignettePrete { libelle: "3D", glyphe: '\u{E809}', couleur: [0x8A, 0x5B, 0xC9], degrade: None },
    VignettePrete { libelle: "Formes", glyphe: '\u{E814}', couleur: [0xC9, 0x6B, 0x8A], degrade: None },
    VignettePrete { libelle: "Profil", glyphe: '\u{E81D}', couleur: [0x5B, 0x7B, 0xA8], degrade: None },
    VignettePrete { libelle: "Paysage", glyphe: '\u{E81F}', couleur: [0x3C, 0x8A, 0x5B], degrade: None },
    VignettePrete { libelle: "Ville", glyphe: '\u{E822}', couleur: [0x7B, 0x7B, 0x8A], degrade: None },
    VignettePrete { libelle: "Fenêtre", glyphe: '\u{E82D}', couleur: [0x5B, 0x8A, 0x8A], degrade: None },
    VignettePrete { libelle: "Présentation", glyphe: '\u{E835}', couleur: [0xC9, 0x8A, 0x5B], degrade: None },
    VignettePrete { libelle: "Statistiques", glyphe: '\u{E836}', couleur: [0x4C, 0x8A, 0x7B], degrade: None },
    VignettePrete { libelle: "Dossier partagé", glyphe: '\u{E838}', couleur: [0x5B, 0x9B, 0xC9], degrade: None },
    VignettePrete { libelle: "Stylo", glyphe: '\u{E841}', couleur: [0x8A, 0x6B, 0x5B], degrade: None },
    VignettePrete { libelle: "Textes", glyphe: '\u{E875}', couleur: [0x6B, 0x7B, 0x9B], degrade: None },
    VignettePrete { libelle: "Validé", glyphe: '\u{E876}', couleur: [0x4C, 0x9B, 0x5B], degrade: None },
    VignettePrete { libelle: "Idée", glyphe: '\u{E88C}', couleur: [0xD9, 0xB1, 0x3F], degrade: None },
    VignettePrete { libelle: "Contact", glyphe: '\u{E899}', couleur: [0x7B, 0x8A, 0xC9], degrade: None },
    VignettePrete { libelle: "Administrateur", glyphe: '\u{E89B}', couleur: [0xA8, 0x5B, 0x5B], degrade: None },
    VignettePrete { libelle: "Monde", glyphe: '\u{E89C}', couleur: [0x3C, 0x7B, 0xA8], degrade: None },
    VignettePrete { libelle: "Journal", glyphe: '\u{E8A1}', couleur: [0x8A, 0x8A, 0x6B], degrade: None },
    VignettePrete { libelle: "Drapeau", glyphe: '\u{E8A6}', couleur: [0xC9, 0x5B, 0x6B], degrade: None },
    VignettePrete { libelle: "Courbe", glyphe: '\u{E8B8}', couleur: [0x5B, 0xA8, 0x8A], degrade: None },
    VignettePrete { libelle: "Recherche", glyphe: '\u{E8B9}', couleur: [0x6B, 0x7B, 0x8A], degrade: None },
    VignettePrete { libelle: "Croissance", glyphe: '\u{E8C2}', couleur: [0x4C, 0x9B, 0x4C], degrade: None },
    VignettePrete { libelle: "Éditer", glyphe: '\u{E8CF}', couleur: [0x9B, 0x6B, 0x8A], degrade: None },
    VignettePrete { libelle: "Fusée", glyphe: '\u{E8D9}', couleur: [0xC9, 0x6B, 0x3C], degrade: None },
    VignettePrete { libelle: "Organigramme", glyphe: '\u{E8DD}', couleur: [0x7B, 0x6B, 0x9B], degrade: None },
    VignettePrete { libelle: "Recadrer", glyphe: '\u{E8F2}', couleur: [0x8A, 0x9B, 0x7B], degrade: None },
    // =====================================================================
    // INFORMATIQUE — ET CE SONT LES DERNIÈRES DE CETTE POLICE.
    //
    // ⚠️ **TROUVÉES EN RELISANT LES PLANCHES DÉJÀ RENDUES, PAS EN BALAYANT PLUS
    // LOIN.** Je les avais écartées parce que je cherchais des pictogrammes de
    // DOSSIER, et ce sont des pictogrammes de MACHINE : un écran, une clé, une
    // batterie, un clavier, une souris, un réseau, un disque. Le même dessin change
    // de sens selon la question posée — c'est la leçon de cette passe.
    //
    // ⛔ **LA RÉSERVE DE CETTE POLICE EST ÉPUISÉE, ET C'EST MESURÉ** : trois plages
    // balayées en entier (`E000..=E6FF` : les commandes multimédias et des tuiles de
    // lettres ; `E700..=E8FF` : les pictogrammes ; `E900..=EAFF` : du mobilier
    // d'interface). Il n'y a pas cinquante icônes d'informatique dans cette police,
    // il y en a huit — et les chercher davantage ne les ferait pas exister.
    // =====================================================================
    VignettePrete { libelle: "Écran", glyphe: '\u{E968}', couleur: [0x4A, 0x6B, 0x8A], degrade: None },
    VignettePrete { libelle: "Périphériques", glyphe: '\u{E88E}', couleur: [0x6B, 0x7B, 0x9B], degrade: None },
    VignettePrete { libelle: "Batterie", glyphe: '\u{E83F}', couleur: [0x5B, 0x9B, 0x4C], degrade: None },
    VignettePrete { libelle: "Clavier", glyphe: '\u{E765}', couleur: [0x7B, 0x6B, 0x8A], degrade: None },
    VignettePrete { libelle: "Souris", glyphe: '\u{E962}', couleur: [0x6B, 0x8A, 0x7B], degrade: None },
    VignettePrete { libelle: "Réseau", glyphe: '\u{E701}', couleur: [0x3C, 0x7B, 0x9B], degrade: None },
    VignettePrete { libelle: "Stockage", glyphe: '\u{EDA2}', couleur: [0x7B, 0x7B, 0x8A], degrade: None },
    VignettePrete { libelle: "Média", glyphe: '\u{E958}', couleur: [0x9B, 0x5B, 0x7B], degrade: None },
];

/// **LES DOSSIERS COLORÉS** — le même dossier, dans dix teintes.
///
/// ⛔ **LE GLYPHE EST `E8D5`, UN DOSSIER PLEIN — ET C'EST LA SONDE QUI L'A
/// DÉCIDÉ.** `E8B7` (celui que le dock emploie comme repli) est un dossier en
/// **contour** : teinté dix fois, il aurait donné dix fois le même trait fin, et
/// la couleur n'aurait porté que sur deux pixels de bord. `E8D5` est **plein**
/// (175 pixels d'encre contre 64, mesuré par la sonde) : la teinte remplit la
/// forme, et les dix dossiers se distinguent au premier coup d'œil.
///
/// ⚠️ **Ils sont dans un GROUPE À PART de la galerie**, et pas mêlés aux
/// catégories : « Dossier rouge » ne dit rien de ce que le dossier CONTIENT, là
/// où « Musique » le dit. Deux questions différentes, deux rangées.
///
/// ⚠️ **Écartés par la sonde, qui a montré qu'ils sont des DOUBLONS** : `F12B`
/// dessine exactement `E8B7`, et `ED25` exactement `E838`.
///
/// ⛔ **UN SEUL, ET BICOLORE — décision utilisateur du 2026-10-23.** Il y en a eu
/// cent, une tuile par teinte ; depuis la rangée de pastilles (voir `TEINTES`),
/// la couleur se CHOISIT, et cent tuiles ne disaient plus rien de plus.
///
/// ⚠️ **IL N'EST PLUS RENDU PAR SON GLYPHE** mais DESSINÉ en deux couches (voir
/// `rendre_dossier_colore`) : un glyphe de police ne porte qu'UNE encre. Seule
/// `couleur` sert — celle de l'avant ; l'arrière en est la version `assombrie`.
/// Le glyphe reste pour le contrat de la page (`decrire`).
pub const DOSSIERS_COLORES: [VignettePrete; 1] = [
    VignettePrete { libelle: "Dossier coloré", glyphe: '\u{E8D5}', couleur: [0xE0, 0x52, 0x52], degrade: None },
];

/// Côté de la vignette PRÊTE, en pixels — la borne haute de
/// `config::DOSSIER_VIGNETTE_MAX_SIDE`, c'est-à-dire ce que la vignette peut
/// valoir de mieux.
///
/// ⚠️ **RENDUE PLUS GRANDE QUE SON AFFICHAGE, ET C'EST VOULU** : la barre dessine
/// une vignette à ~64 px (`ui::dock::bottom_icon_size_for`), mais l'épaisseur de
/// la barre est RÉGLABLE, et la même image resservira après l'avoir augmentée.
/// La rendre à 64 px la figerait à cette taille-là.
pub const COTE_VIGNETTE: u32 = crate::config::DOSSIER_VIGNETTE_MAX_SIDE;

/// **REND UNE CATÉGORIE EN PIXELS RGBA**, prêts pour
/// `config::store_dossier_vignette_from_rgba`.
///
/// Rend `None` si la police d'icônes est absente — l'appelant n'a alors rien à
/// ranger, et le repli habituel (glyphe de dossier, ou initiale) prend le relais.
///
/// ⚠️ **LA COULEUR EST POSÉE ICI, PAS DANS LE GLYPHE** : un glyphe de police est
/// une encre BLANCHE, sa forme vit dans le canal ALPHA. On multiplie donc les
/// trois canaux par la teinte de la catégorie **en laissant l'alpha intact** —
/// le toucher déformerait le dessin au lieu de le colorer.
pub fn rendre(categorie: &VignettePrete) -> Option<(u32, u32, Vec<u8>)> {
    let police = crate::ui::icones::police_d_icones()?;
    let (largeur, hauteur, mut rgba) =
        crate::gfx::text::rendre_rgba(&categorie.glyphe.to_string(), COTE_VIGNETTE as i32, police, false).ok()?;
    // ⚠️ **LA TEINTE SE CALCULE PAR LIGNE, PAS PAR PIXEL.** Le fondu est VERTICAL :
    // tous les pixels d'une même ligne portent donc la même teinte. La calculer une
    // fois par ligne évite `largeur × hauteur` interpolations pour un résultat
    // identique au bit près — et c'est aussi ce qui rend le fondu LISIBLE dans le
    // code, une ligne à la fois.
    let derniere = (hauteur.saturating_sub(1)).max(1) as f32;
    for y in 0..hauteur {
        let teinte = match categorie.degrade {
            None => categorie.couleur,
            Some(vers) => teinte_melangee(categorie.couleur, vers, y as f32 / derniere),
        };
        for x in 0..largeur {
            let pixel = &mut rgba[((y * largeur + x) * 4) as usize..][..4];
            for (canal, teinte) in pixel[..3].iter_mut().zip(teinte) {
                *canal = ((*canal as u16 * teinte as u16) / 255) as u8;
            }
        }
    }
    Some((largeur, hauteur, rgba))
}

/// **LES TEINTES AU CHOIX** — la rangée de pastilles du menu du dossier, qui
/// REMPLACE la couleur d'origine d'une vignette prête, d'un dossier coloré ou d'un
/// dessin.
///
/// ⚠️ **UNE TEINTE CHOISIE EST UN APLAT** : elle efface aussi le dégradé du
/// modèle. Garder le fondu d'origine sous une autre couleur donnerait un
/// mélange que l'aperçu ne montrerait pas — ou qu'il faudrait inventer des deux
/// côtés à la fois.
///
/// ⚠️ **LA PAGE ENVOIE UN INDEX DANS CE TABLEAU, PAS UNE COULEUR** : même règle que
/// les galeries, la page ne montre que ce que Rust lui a poussé, et un index hors
/// bornes est refusé côté Rust (voir `win32::cef_dossier_menu::dispatch_actions`).
/// Aucun noir pur : sur le fond sombre de la barre, il disparaîtrait.
pub const TEINTES: [(&str, [u8; 3]); 17] = [
    ("Rouge", [0xE5, 0x48, 0x4D]),
    ("Orange", [0xF0, 0x8A, 0x3C]),
    ("Ambre", [0xF2, 0xB3, 0x3D]),
    ("Jaune", [0xF5, 0xD5, 0x47]),
    ("Vert pomme", [0xA6, 0xD1, 0x53]),
    ("Vert", [0x4C, 0xB8, 0x6B]),
    ("Émeraude", [0x2F, 0xA8, 0x8A]),
    ("Turquoise", [0x2B, 0xB5, 0xC4]),
    ("Bleu ciel", [0x4F, 0xA3, 0xE8]),
    ("Bleu", [0x3D, 0x7B, 0xE0]),
    ("Indigo", [0x5B, 0x5F, 0xD6]),
    ("Violet", [0x91, 0x60, 0xD6]),
    ("Rose", [0xE0, 0x63, 0xA8]),
    ("Brun", [0x9A, 0x6B, 0x4A]),
    ("Gris", [0x8C, 0x93, 0x9C]),
    ("Graphite", [0x4A, 0x4F, 0x57]),
    ("Blanc", [0xF2, 0xF2, 0xF2]),
];

/// **LA TEINTE DE L'ARRIÈRE D'UN DOSSIER COLORÉ** — la même couleur, assombrie.
///
/// ⚠️ **C'EST RUST QUI LA CALCULE, ET LA PAGE LA REÇOIT** (`sombre` dans
/// `pour_la_page`) : une formule recopiée en JS donnerait un aperçu qui dérive
/// au premier réglage changé d'un seul côté.
pub fn assombrie(couleur: [u8; 3]) -> [u8; 3] {
    /// La part de la teinte que garde l'arrière : assez sombre pour que l'onglet
    /// se détache de l'avant, assez claire pour rester de la même famille.
    const PART: u16 = 62;
    couleur.map(|c| (c as u16 * PART / 100) as u8)
}

/// **REND UN DOSSIER COLORÉ BICOLORE** — l'arrière (avec l'onglet) dans la teinte
/// `assombrie`, l'avant dans `couleur`. `None` : la couleur du modèle.
pub fn rendre_dossier_colore(modele: &VignettePrete, teinte: Option<[u8; 3]>) -> (u32, u32, Vec<u8>) {
    let couleur = teinte.unwrap_or(modele.couleur);
    crate::ui::dessins_vignettes::rendre_bicolore(assombrie(couleur), couleur)
}

/// **LE MODÈLE SOUS UNE AUTRE TEINTE** — une COPIE, l'original reste intact
/// (`CATEGORIES` est une constante). `None` rend le
/// modèle tel quel, avec sa couleur et son fondu d'origine.
pub fn teinte(modele: &VignettePrete, teinte: Option<[u8; 3]>) -> VignettePrete {
    match teinte {
        None => *modele,
        Some(couleur) => VignettePrete { couleur, degrade: None, ..*modele },
    }
}

/// **LA GALERIE, TELLE QUE LA PAGE LA REÇOIT** — `{nom, glyphe, couleur}`.
///
/// ⚠️ **AUCUNE IMAGE NE TRAVERSE CE CANAL** : la page dessine le glyphe
/// elle-même, avec la police d'icônes, exactement comme le dock le fait de son
/// côté pour le repli. C'est la règle du dépôt — une page n'a aucun accès au
/// système de fichiers (voir `win32::file_dialog`, doc de module) — et elle évite
/// aussi d'envoyer dix data-URI là où un point de code suffit.
pub fn pour_la_page() -> serde_json::Value {
    // ⛔ CALCULÉ AVANT `json!` : un bloc `{ … }` nu dans la macro serait lu comme un
    // OBJET JSON, pas comme une expression Rust.
    let bicolore = {
        let s = assombrie(DOSSIERS_COLORES[0].couleur);
        let (arriere, avant) = crate::ui::dessins_vignettes::dossier_bicolore_pour_la_page();
        serde_json::json!({
            "arriere": arriere,
            "avant": avant,
            "sombre": format!("#{:02X}{:02X}{:02X}", s[0], s[1], s[2]),
        })
    };
    serde_json::json!({
        "categories": decrire(&CATEGORIES),
        "couleurs": decrire(&DOSSIERS_COLORES),
        // ⛔ **UNE TROISIÈME FAMILLE, ET ELLE N'EST PAS UNE LISTE DE GLYPHES** : ses
        // vignettes sont DESSINÉES par nous (voir `ui::dessins_vignettes`), parce que
        // la police n'a ni serveur, ni base de données, ni conteneur — trois plages
        // balayées en entier l'ont montré. Elle envoie sa DESCRIPTION, pas une image :
        // la page la reconstruit en SVG, donc l'aperçu ne peut pas mentir.
        "dessins": crate::ui::dessins_vignettes::pour_la_page(),
        // LES DEUX COUCHES du dossier coloré (voir `rendre_dossier_colore`) — la page
        // les reconstruit en SVG, comme les dessins, et `sombre` est la teinte de
        // l'arrière pour la couleur d'origine.
        "bicolore": bicolore,
        // La rangée de pastilles — l'INDEX de la pastille cliquée revient avec le
        // verbe (voir `TEINTES`).
        "teintes": TEINTES
            .iter()
            .map(|(nom, c)| {
                let s = assombrie(*c);
                serde_json::json!({
                    "nom": nom,
                    "couleur": format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
                    // L'arrière d'un dossier coloré sous cette teinte.
                    "sombre": format!("#{:02X}{:02X}{:02X}", s[0], s[1], s[2]),
                })
            })
            .collect::<Vec<_>>(),
    })
}

/// Décrit une famille de vignettes prêtes pour la page — une seule fois, les deux
/// rangées ayant exactement la même forme.
fn decrire(modeles: &[VignettePrete]) -> serde_json::Value {
    serde_json::Value::Array(
        modeles
            .iter()
            .map(|m| {
                serde_json::json!({
                    "nom": m.libelle,
                    // Le POINT DE CODE, et non le caractère : la page le
                    // reconstitue par `String.fromCodePoint`. Un caractère
                    // transmis tel quel marcherait aussi, mais il serait
                    // invisible à relire dans un journal.
                    "glyphe": m.glyphe as u32,
                    // Une couleur CSS relisible — la page la pose telle quelle
                    // dans `style.color`.
                    "couleur": format!("#{:02X}{:02X}{:02X}", m.couleur[0], m.couleur[1], m.couleur[2]),
                    // **LA SECONDE TEINTE, OU `null`** — la page pose un APLAT quand
                    // elle est absente et un DÉGRADÉ CSS quand elle est là. L'envoyer
                    // toujours, plutôt que d'omettre la clé : la page lit un champ
                    // présent et `null`, jamais un champ manquant (même règle que
                    // `sorte` dans `cef_spacer_size`, où l'absence a un sens).
                    "degrade": m.degrade.map(|c| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])),
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **DEUX CATÉGORIES NE PEUVENT PAS PORTER LE MÊME GLYPHE** — et c'est un
    /// vrai risque, pas une hypothèse : la sonde a montré que `E91B` dessine
    /// exactement `EB9F` et `E118` exactement `E896`. Prendre l'un pour l'autre
    /// « afin de varier » aurait donné deux tuiles IDENTIQUES dans la galerie,
    /// sans que rien ne le signale — on aurait juste cru à un doublon d'affichage.
    ///
    /// La zone privée est aussi vérifiée : un glyphe hors de là est un caractère
    /// ordinaire, qui s'afficherait comme du TEXTE.
    #[test]
    fn chaque_categorie_a_un_glyphe_distinct_et_dans_la_zone_privee() {
        let mut vus = std::collections::HashSet::new();
        for categorie in CATEGORIES {
            assert!(
                ('\u{E000}'..='\u{F8FF}').contains(&categorie.glyphe),
                "{} : U+{:04X} n'est pas dans la zone privee de la police",
                categorie.libelle,
                categorie.glyphe as u32
            );
            assert!(vus.insert(categorie.glyphe), "glyphe en double : {}", categorie.libelle);
        }
    }

    /// **LES DOSSIERS COLORÉS NE SE DISTINGUENT PAS PAR LEUR GLYPHE, MAIS PAR
    /// LEUR COULEUR** — c'est leur raison d'être, et l'inverse exact du test
    /// ci-dessus.
    ///
    /// ⚠️ **Deux couleurs identiques seraient une vignette en double dans la
    /// galerie** : deux tuiles indiscernables, dont l'une serait donc inutile, et
    /// rien ne le signalerait. Le contrôle porte donc sur la couleur, pas sur le
    /// glyphe.
    #[test]
    fn les_dossiers_colores_partagent_un_glyphe_et_se_distinguent_par_la_couleur() {
        let glyphe = DOSSIERS_COLORES[0].glyphe;
        assert!(
            ('\u{E000}'..='\u{F8FF}').contains(&glyphe),
            "U+{:04X} n'est pas dans la zone privee de la police",
            glyphe as u32
        );
        let mut teintes = std::collections::HashSet::new();
        for dossier in DOSSIERS_COLORES {
            assert_eq!(
                dossier.glyphe, glyphe,
                "{} : tous les dossiers colores doivent porter le MEME dossier (voir DOSSIERS_COLORES)",
                dossier.libelle
            );
            // ⛔ **C'EST LE COUPLE QUI IDENTIFIE UNE TUILE, PAS LA PREMIÈRE TEINTE** :
            // depuis les dégradés, deux dossiers peuvent partager leur teinte HAUTE et
            // ne différer que par la BASSE. Comparer les seules teintes hautes
            // refuserait des tuiles parfaitement distinctes — et, plus grave, deux
            // tuiles identiques passeraient si l'une est un aplat et l'autre un fondu
            // de deux fois la même couleur.
            assert!(
                teintes.insert((dossier.couleur, dossier.degrade)),
                "teinte en double : {} — deux tuiles indiscernables dans la galerie",
                dossier.libelle
            );
            if let Some(bas) = dossier.degrade {
                assert_ne!(
                    bas, dossier.couleur,
                    "{} : un degrade entre DEUX FOIS la meme teinte est un aplat deguise",
                    dossier.libelle
                );
            }
        }
        // Et le dossier PLEIN n'est pas celui du repli (E8B7, en contour) : les
        // mélanger aurait mis un trait fin au milieu de dix aplats.
        assert_ne!(glyphe, '\u{E8B7}', "le dossier colore doit etre le PLEIN (E8D5), pas le contour");
    }

    /// **LE CONTRAT AVEC LA PAGE, VÉRIFIÉ DES DEUX CÔTÉS.** La page lit
    /// `categorie.nom`, `categorie.glyphe` et `categorie.couleur` — un champ
    /// renommé ici et pas là lui donnerait `undefined` : `String.fromCodePoint(undefined)`
    /// lève, la tuile reste VIDE, et rien ne le dit (`execute_js` ne rend aucun
    /// compte de son échec). C'est la même classe de panne muette que le
    /// `SyntaxError` de `cef_sous_dock`, en plus discret.
    #[test]
    fn la_galerie_decrit_chaque_famille_comme_la_page_l_attend() {
        let galerie = pour_la_page();
        for (cle, modeles) in [("categories", &CATEGORIES[..]), ("couleurs", &DOSSIERS_COLORES[..])] {
            let liste = galerie[cle].as_array().unwrap_or_else(|| panic!("{cle} doit etre un tableau"));
            assert_eq!(liste.len(), modeles.len(), "{cle} : une entree par modele");
            for (entree, modele) in liste.iter().zip(modeles) {
                assert_eq!(entree["nom"], modele.libelle);
                // Le POINT DE CODE, en nombre — ce que `String.fromCodePoint` attend.
                assert_eq!(entree["glyphe"], modele.glyphe as u32, "{cle}/{}", modele.libelle);
                // Une couleur CSS relisible : `#RRGGBB`, ce que `style.color` accepte.
                let couleur = entree["couleur"].as_str().unwrap_or_default();
                assert_eq!(couleur.len(), 7, "{cle}/{} : couleur mal formee : {couleur:?}", modele.libelle);
                assert!(couleur.starts_with('#'), "{cle}/{} : couleur mal formee : {couleur:?}", modele.libelle);
                // **LA SECONDE TEINTE — une couleur CSS, ou `null`.** La page teste
                // `if (modele.degrade)`, donc un `null` envoyé en CHAÎNE `"null"`
                // passerait ce test-là et peindrait un dégradé noir : c'est cette
                // assertion qui l'attrape.
                match entree["degrade"].as_str() {
                    Some(fondu) => {
                        assert_eq!(fondu.len(), 7, "{cle}/{} : degrade mal forme : {fondu:?}", modele.libelle);
                        assert!(fondu.starts_with('#'), "{cle}/{} : degrade mal forme : {fondu:?}", modele.libelle);
                    }
                    None => assert!(
                        entree["degrade"].is_null(),
                        "{cle}/{} : `degrade` doit etre une couleur CSS ou null, jamais autre chose : {:?}",
                        modele.libelle,
                        entree["degrade"]
                    ),
                }
                assert_eq!(
                    entree["degrade"].is_null(),
                    modele.degrade.is_none(),
                    "{cle}/{} : la page et le modele ne disent pas la meme chose du fondu",
                    modele.libelle
                );
            }
        }
    }

    /// **LE FONDU — LE CONTRAT DES DEUX BOUTS, ET LE MILIEU.**
    ///
    /// ⚠️ **PURE, DONC MESURABLE, ET C'EST TOUT L'INTÉRÊT** : un fondu faux — un `t`
    /// inversé, une troncature au lieu d'un arrondi, un canal qui déborde du négatif —
    /// ne se verrait qu'à l'œil, sur une vignette de 64 px au fond d'un menu. Ici, il
    /// se lit.
    /// **UN DOSSIER COLORÉ PORTE DEUX TEINTES** — l'avant dans la couleur choisie,
    /// l'onglet de l'arrière plus sombre.
    #[test]
    fn un_dossier_colore_porte_deux_teintes() {
        let rouge = TEINTES[0].1;
        let (l, h, rgba) = rendre_dossier_colore(&DOSSIERS_COLORES[0], Some(rouge));
        let lire = |x: f32, y: f32| {
            let i = (((y * h as f32) as u32 * l + (x * l as f32) as u32) * 4) as usize;
            [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
        };
        // L'ONGLET (en haut à gauche, au-dessus de l'avant) et le MILIEU de l'avant.
        let onglet = lire(0.25, 0.27);
        let avant = lire(0.5, 0.6);
        assert_eq!(onglet[3], 255, "l'onglet doit etre opaque : {onglet:?}");
        assert_eq!(avant, [rouge[0], rouge[1], rouge[2], 255], "l'avant porte la teinte choisie");
        let sombre = assombrie(rouge);
        assert_eq!(onglet, [sombre[0], sombre[1], sombre[2], 255], "l'onglet porte la teinte assombrie");
        assert!(sombre.iter().zip(rouge).all(|(s, c)| *s < c));
        // Et sans pastille : la couleur du modèle.
        let (_, _, origine) = rendre_dossier_colore(&DOSSIERS_COLORES[0], None);
        let i = (((0.6 * h as f32) as u32 * l + (0.5 * l as f32) as u32) * 4) as usize;
        assert_eq!(origine[i..i + 3], DOSSIERS_COLORES[0].couleur);
    }

    #[test]
    fn le_fondu_rend_exactement_ses_deux_bouts_et_passe_par_le_milieu() {
        let haut = [0x00, 0x40, 0x80];
        let bas = [0xFF, 0x80, 0x00];
        assert_eq!(teinte_melangee(haut, bas, 0.0), haut, "t = 0 : la premiere teinte, EXACTEMENT");
        assert_eq!(teinte_melangee(haut, bas, 1.0), bas, "t = 1 : la seconde, EXACTEMENT");
        assert_eq!(teinte_melangee(haut, bas, 0.5), [0x80, 0x60, 0x40], "t = 0.5 : le milieu exact");
        // HORS BORNES : borné, jamais un canal qui déborde (`0x00 − 0x40` est négatif).
        assert_eq!(teinte_melangee(haut, bas, -1.0), haut);
        assert_eq!(teinte_melangee(haut, bas, 2.0), bas);
        // Et le SENS compte : les inverser donne l'inverse.
        assert_eq!(teinte_melangee(bas, haut, 0.0), bas);
        assert_eq!(teinte_melangee(bas, haut, 1.0), haut);
    }

    /// **TOUS LES DÉGRADÉS SONT DES FONDUS, PAS DES APLATS DÉGUISÉS** — et le fondu
    /// porte bien sur la hauteur du GLYPHE, pas sur une constante.
    ///
    /// Sans la police d'icônes le rendu est `None` : le test ne fait alors rien plutôt
    /// que d'échouer, comme `rendre` le promet à ses appelants.
    #[test]
    fn un_dossier_degrade_traverse_vraiment_deux_teintes() {
        // Plus aucun modèle livré ne porte de fondu (les dossiers colorés en avaient,
        // ils sont devenus bicolores) : on en fabrique un, le chemin du rendu reste.
        let fondu = &VignettePrete { couleur: [0xE0, 0xC9, 0x4C], degrade: Some([0x4A, 0x1A, 0x2E]), ..CATEGORIES[0] };
        let haut = fondu.couleur;
        let bas = fondu.degrade.expect("prémisse");
        let Some((largeur, hauteur, rgba)) = rendre(fondu) else { return };
        // La ligne du HAUT et celle du BAS portent chacune UNE teinte : on lit le
        // pixel le plus opaque de chacune, et on vérifie qu'elles DIFFÈRENT.
        let teinte_dune_ligne = |y: u32| -> Option<[u8; 3]> {
            (0..largeur)
                .map(|x| &rgba[((y * largeur + x) * 4) as usize..][..4])
                .max_by_key(|p| p[3])
                .filter(|p| p[3] > 0)
                .map(|p| [p[0], p[1], p[2]])
        };
        let Some(en_haut) = teinte_dune_ligne(0) else { return };
        let Some(en_bas) = teinte_dune_ligne(hauteur - 1) else { return };
        assert_ne!(
            en_haut, en_bas,
            "le haut et le bas de « {} » portent la MEME teinte : le fondu ne va nulle part",
            fondu.libelle
        );
        let _ = (haut, bas);
    }

    /// **BALAYAGE D'UNE PLAGE DE CODES — L'INSTRUMENT QUI MANQUAIT POUR EN CHERCHER
    /// CINQUANTE.**
    ///
    /// ⚠️ **POURQUOI ELLE EXISTE.** La sonde ci-dessous examine une LISTE écrite à la
    /// main : elle vaut pour vérifier un candidat, pas pour en trouver cinquante. La
    /// chercher ainsi demanderait d'en examiner des centaines, dont la plupart ne
    /// dessinent rien — et le catalogue d'une police ne dit **jamais** ce qu'un point de
    /// code dessine.
    ///
    /// Ici :
    ///   · la plage est **BALAYÉE**, pas devinée ;
    ///   · seuls sortent les dessins **SUBSTANTIELS** (`ENCRE_MIN`) — un artefact
    ///     d'interface en fait moins, et une case vide aucun ;
    ///   · ceux **DÉJÀ employés** sont écartés : on ne relit que du neuf ;
    ///   · ils sont imprimés **QUATRE PAR BLOC**, côte à côte — la seule disposition où
    ///     l'on voie qu'ils **se ressemblent**, qui est la question qu'on leur pose.
    ///
    /// `cargo test --release -- --ignored --nocapture balayage_des_glyphes`
    #[test]
    #[ignore = "sonde a la main — n'ecrit rien"]
    fn balayage_des_glyphes() {
        /// En dessous, ce n'est pas un dessin : c'est un trait d'interface, un coin, ou
        /// une case vide. Le seuil est BAS à dessein — un pictogramme simple (une coche,
        /// une flèche) fait dans les trente pixels à 96 px.
        /// ⚠️ **RÉGLÉ À 70 APRÈS MESURE, PAS AU HASARD** : la distribution d'encre de
        /// `E700..=E8FF` donne 417 dessins à 30, 163 à 60, **108 à 70**, 75 à 80. En
        /// dessous de 70, la liste se remplit d'artefacts d'interface — chevrons, coins,
        /// traits — qui ne disent rien. Au-dessus, on ne garde que des PICTOGRAMMES, et
        /// il en reste assez pour en choisir cinquante.
        const ENCRE_MIN: u32 = 80;
        // LA PLAGE BALAYÉE — la zone privée de la police, là où vivent les pictogrammes.
        const PLAGE: std::ops::RangeInclusive<u32> = 0xE000..=0xE6FF;

        let Some(police) = crate::ui::icones::police_d_icones() else {
            println!("aucune police d'icones installee");
            return;
        };
        let deja: std::collections::HashSet<u32> = CATEGORIES
            .iter()
            .chain(DOSSIERS_COLORES.iter())
            .map(|v| v.glyphe as u32)
            .collect();

        let mut retenus: Vec<(u32, Vec<String>, u32)> = Vec::new();
        for point in PLAGE {
            if deja.contains(&point) {
                continue;
            }
            let Some(c) = char::from_u32(point) else { continue };
            let Ok((largeur, hauteur, rgba)) = crate::gfx::text::rendre_rgba(&c.to_string(), 96, police, false)
            else {
                continue;
            };
            let pas = 6u32;
            let mut lignes = Vec::new();
            let mut encre = 0u32;
            let mut y = 0;
            while y < hauteur {
                let mut ligne = String::new();
                let mut x = 0;
                while x < largeur {
                    let a = rgba[((y * largeur + x) * 4 + 3) as usize];
                    if a > 128 {
                        encre += 1;
                    }
                    ligne.push(match a {
                        0..=63 => ' ',
                        64..=191 => '.',
                        _ => '#',
                    });
                    x += pas;
                }
                lignes.push(ligne);
                y += pas;
            }
            if encre >= ENCRE_MIN {
                retenus.push((point, lignes, encre));
            }
        }
        println!(
            "\n=== BALAYAGE U+{:04X}..=U+{:04X} : {} glyphes substantiels et NEUFS ===",
            PLAGE.start(),
            PLAGE.end(),
            retenus.len()
        );
        for bloc in retenus.chunks(4) {
            println!(
                "\n{}",
                bloc.iter()
                    .map(|(p, _, e)| format!("U+{p:04X} encre={e}"))
                    .collect::<Vec<_>>()
                    .join("     |     ")
            );
            let hauteur_max = bloc.iter().map(|(_, l, _)| l.len()).max().unwrap_or(0);
            for i in 0..hauteur_max {
                println!(
                    "{}",
                    bloc.iter()
                        .map(|(_, lignes, _)| lignes.get(i).cloned().unwrap_or_else(|| " ".repeat(16)))
                        .collect::<Vec<_>>()
                        .join(" | ")
                );
            }
        }
    }

    /// **REND LES CANDIDATS EN ASCII, POUR LES IDENTIFIER À L'ŒIL.**
    ///
    /// Le catalogue d'une police ne dit rien de ce qu'elle DESSINE, et un nom de
    /// glyphe est une supposition. C'est cette sonde — copiée de
    /// `ui::icones::sonde_des_glyphes`, la même leçon — qui a rempli le tableau
    /// `CATEGORIES` ci-dessus, et elle doit rester pour le prochain ajout.
    ///
    /// ⚠️ **Les codes DÉJÀ UTILISÉS par le dépôt sont dans la liste** (`E756`
    /// terminal, `E7F8` bureau, `E787` calendrier…), en REPÈRES : ils calibrent la
    /// lecture de l'ASCII, puisque leur dessin est connu.
    #[test]
    #[ignore = "sonde a la main — n'ecrit rien"]
    fn sonde_des_glyphes_de_categorie() {
        let Some(police) = crate::ui::icones::police_d_icones() else {
            println!("aucune police d'icones installee");
            return;
        };
        const CANDIDATS: &[u32] = &[
            // --- DOSSIERS : variantes pleines ou alternatives à E8B7 ---
            0xE8D5, 0xE838, 0xE8F4, 0xED25, 0xF12B, 0xEC50, 0xE8B7,
            // --- REPÈRES CONNUS (leur dessin est déjà utilisé dans le dock) ---
            0xE8B7, 0xE7F8, 0xE756, 0xE787, 0xEB9F, 0xE968, 0xE7F4, 0xE9A6,
            // --- DOCUMENTS ---
            0xE8A5, 0xE7C3, 0xE8A1, 0xE8E5, 0xEC4A, 0xE8F1,
            // --- IMAGES ---
            0xEB9F, 0xE91B, 0xE8B9, 0xE8FD,
            // --- MUSIQUE / SON ---
            0xE8D6, 0xE767, 0xE8D2, 0xE7F5,
            // --- VIDÉOS ---
            0xE714, 0xE8B2, 0xE786, 0xE7C5, 0xE8B5,
            // --- TÉLÉCHARGEMENTS ---
            0xE896, 0xE118, 0xE7B8,
            // --- BUREAU / SYSTÈME ---
            0xE7FC, 0xE7E7, 0xE8D4, 0xEC4F,
            // --- CLOUD / PARTAGE ---
            0xE753, 0xE9E9, 0xEBD3, 0xE8F4,
            // --- FAVORIS / ÉTOILE ---
            0xE734, 0xE735,
            // =====================================================================
            // NOUVEAUX CAS D'USAGE (2026-10-01) — à VÉRIFIER AVANT D'ÊTRE ADOPTÉS.
            //
            // ⚠️ **C'EST LA RÈGLE DU DÉPÔT, ET ELLE A DÉJÀ PAYÉ** : trois codes
            // « différents » se sont révélés des DOUBLONS PARFAITS (`E91B`=`EB9F`,
            // `E118`=`E896`, `E7C3`=`E8A5`). Un nom de glyphe est une supposition ;
            // seul le RENDU tranche.
            //
            // Ces candidats couvrent ce que le catalogue ne savait pas dire :
            // travail, études, livres, finance, santé, voyages, outils, sécurité,
            // archives, partage.
            // =====================================================================
            // TRAVAIL / BUREAU PRO
            0xE821, 0xE7B8, 0xE8F1, 0xEC50, 0xE8EC,
            // ÉTUDES / LIVRES
            0xE7BE, 0xE82D, 0xE8B9, 0xE9A8,
            // FINANCE / ARGENT
            0xE943, 0xE8C7, 0xE9D2, 0xE7C1,
            // SANTÉ / CŒUR
            0xEB51, 0xEB52, 0xE95E,
            // VOYAGES / CARTE
            0xE709, 0xE12B, 0xE704, 0xE8B5,
            // OUTILS / RÉGLAGES
            0xE713, 0xE90F, 0xE74E, 0xE912,
            // SÉCURITÉ
            0xE72E, 0xE785, 0xEA18, 0xE1A7,
            // ARCHIVES / BOÎTE
            0xE7B8, 0xE8F4, 0xE7BA, 0xE8FD,
            // PARTAGE / PERSONNES
            0xE716, 0xE77B, 0xE902, 0xE8D3,
            // DIVERS — au cas où l'un d'eux dessine mieux que les précédents
            0xE945, 0xE734, 0xE8D7, 0xE9F5,
            // =====================================================================
            // DEUXIÈME VAGUE (2026-10-01) — mêmes règles : on REGARDE, on ne devine pas.
            // =====================================================================
            // MAISON / LIEU
            0xE80F, 0xE7C0, 0xE809, 0xE8B0,
            // TRANSPORTS
            0xE804, 0xE806, 0xE805, 0xE7C0, 0xEB4C,
            // SPORT / NATURE
            0xEB43, 0xE909, 0xE706, 0xE8D4, 0xEB44,
            // CUISINE
            0xE56C, 0xE7B6, 0xE8B6, 0xEC32,
            // COMMUNICATION
            0xE715, 0xE8BD, 0xE717, 0xE8F2, 0xE724,
            // MÉDIA / CRÉATION
            0xE722, 0xE720, 0xE7F6, 0xE8B8, 0xE786, 0xE8D6,
            // MATÉRIEL / SYSTÈME
            0xE770, 0xE765, 0xE962, 0xE88E, 0xE83F, 0xE958, 0xE8EF,
            // RÉSEAU / SERVICE
            0xE701, 0xE702, 0xE968, 0xE9D9, 0xE753, 0xE8CE,
            // DIVERS
            0xE7B0, 0xE710, 0xE74D, 0xE840, 0xE728, 0xE823, 0xE787, 0xE70B,
            // =====================================================================
            // TROISIÈME VAGUE (2026-10-01) — encore des cas d'usage.
            // =====================================================================
            // TEMPS / AGENDA
            0xE7ED, 0xE121, 0xE916, 0xE8C0, 0xE9D5, 0xE73A, 0xE73E,
            // STOCKAGE / DONNÉES
            0xEDA2, 0xEE94, 0xE7BF, 0xE14D, 0xE719, 0xE825,
            // CRÉATION / BUREAU
            0xE790, 0xE8C6, 0xE7A8, 0xE77F, 0xE71C, 0xE8CB, 0xE75C,
            // MATÉRIEL
            0xE749, 0xE8FE, 0xE7B3, 0xE7E8,
            // GÉO / NATURE
            0xE707, 0xE1D3, 0xE708, 0xE9CE, 0xE706,
            // DIVERS — au cas où
            0xE946, 0xE8F3, 0xE71B, 0xE8F8, 0xE9A9, 0xE1CB, 0xE8B3, 0xEB05,
            0xEC8F, 0xE8AB, 0xE96D, 0xE8AA, 0xE72D,
        ];
        for point in CANDIDATS {
            let Some(c) = char::from_u32(*point) else { continue };
            let Ok((largeur, hauteur, rgba)) =
                crate::gfx::text::rendre_rgba(&c.to_string(), 96, police, false)
            else {
                println!("\n=== U+{point:04X} : RENDU IMPOSSIBLE ===\n");
                continue;
            };
            let mut encre = 0u32;
            let mut ascii = String::new();
            let pas = 6u32;
            let mut y = 0;
            while y < hauteur {
                let mut x = 0;
                while x < largeur {
                    let a = rgba[((y * largeur + x) * 4 + 3) as usize];
                    if a > 128 {
                        encre += 1;
                    }
                    ascii.push(match a {
                        0..=63 => ' ',
                        64..=191 => '.',
                        _ => '#',
                    });
                    x += pas;
                }
                ascii.push('\n');
                y += pas;
            }
            println!("\n=== U+{point:04X} ({largeur}x{hauteur}, encre {encre}) ===\n{ascii}");
        }
    }
}
