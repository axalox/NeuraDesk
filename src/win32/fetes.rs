//! Ce que l'applet calendrier a à dire sur une date : les JOURS FÉRIÉS
//! français, et — dès qu'une table propre sera fournie — la fête du jour.
//!
//! **LES JOURS FÉRIÉS SE CALCULENT, ILS NE SE STOCKENT PAS.** Une table de
//! dates devrait être refaite chaque année et deviendrait fausse en silence ;
//! l'algorithme de Pâques, lui, est exact pour toute année grégorienne. C'est
//! aussi ce qui évite d'embarquer des données dont il faudrait citer la
//! provenance et la licence — voir `fete_du_jour`, qui n'a PAS cette chance.
//!
//! La liste est celle de l'article L3133-1 du Code du travail, pour la
//! métropole. **L'Alsace-Moselle en a deux de plus** (Vendredi saint et
//! Saint-Étienne) et les outre-mer d'autres encore : ce module ne les connaît
//! pas, faute de savoir où l'utilisateur habite, et les ajouter supposerait un
//! réglage de région — à faire le jour où quelqu'un le demande.

use crate::config;
use crate::win32::notes;

/// Une date civile. Volontairement pauvre : ce module ne fait que COMPARER des
/// dates et en décaler de quelques jours, jamais de l'arithmétique de durée.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Date {
    pub annee: i32,
    pub mois: u32,
    pub jour: u32,
}

/// Un jour férié : sa date et son nom officiel.
pub struct Ferie {
    pub date: Date,
    pub nom: &'static str,
}

/// PÂQUES (dimanche) — **algorithme grégorien anonyme**, dit de Meeus/Jones/
/// Butcher. C'est la référence du domaine : chaque terme y a un rôle
/// (le cycle de Méton pour `a`, le siècle pour `b`/`c`, les corrections
/// grégoriennes pour `f`/`g`/`m`), et il ne demande aucune table.
///
/// **VÉRIFIÉ CONTRE UNE SOURCE EXTERNE**, et pas seulement contre lui-même :
/// les dates de `paques_connues` sont celles que publie la table « List of
/// dates for Easter » de Wikipédia (colonne occidentale), relevées pour ce
/// contrôle. ⚠️ Une première tentative de vérification a échoué en annonçant
/// 21 écarts — **c'était l'EXTRACTEUR qui était faux** (il produisait un
/// 20 mars, hors de la fenêtre de Nicée, donc une date impossible). La leçon
/// vaut d'être gardée : quand une vérification accuse le code, se demander
/// d'abord ce que l'instrument pouvait voir.
pub fn paques(annee: i32) -> Date {
    let a = annee.rem_euclid(19);
    let b = annee.div_euclid(100);
    let c = annee.rem_euclid(100);
    let d = b.div_euclid(4);
    let e = b.rem_euclid(4);
    let f = (b + 8).div_euclid(25);
    let g = (b - f + 1).div_euclid(3);
    let h = (19 * a + b - d - g + 15).rem_euclid(30);
    let i = c.div_euclid(4);
    let k = c.rem_euclid(4);
    let l = (32 + 2 * e + 2 * i - h - k).rem_euclid(7);
    let m = (a + 11 * h + 22 * l).div_euclid(451);
    let mois = (h + l - 7 * m + 114).div_euclid(31) as u32;
    let jour = ((h + l - 7 * m + 114).rem_euclid(31) + 1) as u32;
    Date { annee, mois, jour }
}

/// Nombre de jours du mois — bissextile comprise (`rem_euclid` plutôt qu'un
/// `%` : les années négatives n'existent pas ici, mais la règle vaut d'être
/// tenue par le type d'opération, pas par une convention implicite).
fn jours_dans_le_mois(annee: i32, mois: u32) -> u32 {
    match mois {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let bissextile = (annee.rem_euclid(4) == 0 && annee.rem_euclid(100) != 0) || annee.rem_euclid(400) == 0;
            if bissextile { 29 } else { 28 }
        }
        _ => 0,
    }
}

/// Décale une date de `n` jours (négatif accepté) — par pas d'un jour, ce qui
/// suffit très largement ici (au plus un demi-siècle de décalage) et évite
/// toute formule de conversion en numéro de jour.
fn plus_jours(date: Date, n: i32) -> Date {
    let (mut annee, mut mois, mut jour) = (date.annee, date.mois, date.jour as i32 + n);
    while jour < 1 {
        mois -= 1;
        if mois < 1 {
            mois = 12;
            annee -= 1;
        }
        jour += jours_dans_le_mois(annee, mois) as i32;
    }
    loop {
        let fin = jours_dans_le_mois(annee, mois) as i32;
        if jour <= fin {
            break;
        }
        jour -= fin;
        mois += 1;
        if mois > 12 {
            mois = 1;
            annee += 1;
        }
    }
    // `jour` est positif ici : les deux boucles ci-dessus le ramènent dans
    // [1, jours_dans_le_mois], donc la conversion ne peut pas tronquer.
    Date { annee, mois, jour: jour as u32 }
}

/// Les ONZE jours fériés de métropole pour `annee`, **dans l'ordre
/// chronologique** — les quatre qui dépendent de Pâques sont donc intercalés
/// au bon rang, pas ajoutés à la fin.
///
/// Les noms sont ceux que l'usage et les textes officiels emploient ; ils sont
/// affichés tels quels par l'applet, donc figés ici plutôt que reconstruits.
pub fn jours_feries(annee: i32) -> Vec<Ferie> {
    let p = paques(annee);
    let mut liste = vec![
        Ferie { date: Date { annee, mois: 1, jour: 1 }, nom: "Jour de l'An" },
        // Lundi de Pâques : le LENDEMAIN de Pâques (Pâques tombe un dimanche).
        Ferie { date: plus_jours(p, 1), nom: "Lundi de Pâques" },
        Ferie { date: Date { annee, mois: 5, jour: 1 }, nom: "Fête du Travail" },
        Ferie { date: Date { annee, mois: 5, jour: 8 }, nom: "Victoire 1945" },
        // Ascension : le 39e jour après Pâques, donc un jeudi.
        Ferie { date: plus_jours(p, 39), nom: "Ascension" },
        // Lundi de Pentecôte : le 50e jour après Pâques.
        Ferie { date: plus_jours(p, 50), nom: "Lundi de Pentecôte" },
        Ferie { date: Date { annee, mois: 7, jour: 14 }, nom: "Fête nationale" },
        Ferie { date: Date { annee, mois: 8, jour: 15 }, nom: "Assomption" },
        Ferie { date: Date { annee, mois: 11, jour: 1 }, nom: "Toussaint" },
        Ferie { date: Date { annee, mois: 11, jour: 11 }, nom: "Armistice 1918" },
        Ferie { date: Date { annee, mois: 12, jour: 25 }, nom: "Noël" },
    ];
    liste.sort_by_key(|f| (f.date.mois, f.date.jour));
    liste
}

/// Le nom du férié de cette date, `None` si c'en est une ordinaire.
pub fn ferie_a(date: Date) -> Option<&'static str> {
    jours_feries(date.annee).into_iter().find(|f| f.date == date).map(|f| f.nom)
}

// ---------------------------------------------------------------------------
// LA TABLE DES FÊTES, ET CE QU'ELLE NE CONTIENT PAS
// ---------------------------------------------------------------------------

/// Ce qu'une fête EST — quatre familles, ramenées des dix catégories du fichier
/// source (voir `fetes_france.tsv`, en-tête) pour que l'affichage puisse les
/// distinguer sans trimballer des libellés libres.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Categorie {
    /// Fête du prénom — le « saint du jour », 383 des 400 entrées.
    Saint,
    /// Jour férié légal (les dates FIXES ; les mobiles se calculent).
    Ferie,
    /// Fête civile ou commerciale : Fête de la Musique, Saint-Valentin…
    Civil,
    /// Tradition religieuse ou populaire non fériée : Chandeleur, Halloween…
    Tradition,
    /// **Un ANNIVERSAIRE SAISI** — famille qui n'existe PAS dans la table : elle
    /// ne vient que des dates que l'utilisateur ajoute lui-même. Les quatre
    /// ci-dessus décrivent ce que le fichier des fêtes contient ; ces deux-ci,
    /// ce que la fenêtre y ajoute.
    AnniversaireSaisi,
    /// **Une FÊTE PERSONNALISÉE SAISIE** — voir `AnniversaireSaisi`.
    FeteSaisie,
    /// **L'ALARME D'UNE NOTE** — la seule famille qui ne vienne d'aucun
    /// calendrier : l'utilisateur l'a posée sur une note (voir
    /// `notes::AlarmeNote`). Elle apparaît dans le panneau du mini calendrier,
    /// SOUS les fêtes du jour, à la date qu'elle porte.
    Note,
}

impl Categorie {
    /// La CLÉ transmise à la page — minuscule, sans accent, STABLE. La page s'en
    /// sert pour la pastille de couleur et pour nommer la famille ; la changer
    /// casserait son affichage sans qu'aucun compilateur ne le dise, d'où ce
    /// commentaire (le `.tsv` emploie exactement les mêmes clés, voir son
    /// en-tête).
    pub fn cle(self) -> &'static str {
        match self {
            Categorie::Saint => "saint",
            Categorie::Ferie => "ferie",
            Categorie::Civil => "civil",
            Categorie::Tradition => "tradition",
            Categorie::AnniversaireSaisi => "anniversaire",
            Categorie::FeteSaisie => "fete",
            Categorie::Note => "note",
        }
    }

    /// La famille d'une date SAISIE, d'après son genre — **le seul endroit qui
    /// fasse cette correspondance**.
    ///
    /// ⚠️ **ELLE EXISTAIT EN DOUBLE AVANT D'ÊTRE ÉCRITE ICI**, et allait l'être une
    /// troisième fois : `chercher` la portait en clair (`match d.genre { … }`), et
    /// l'affichage du mois allait la recopier. C'est le genre de duplication qui
    /// finit par diverger — l'une des copies oublierait un genre, et une famille
    /// entière changerait de couleur sans que rien ne le dise.
    pub fn depuis_genre(genre: config::Genre) -> Categorie {
        match genre {
            config::Genre::Anniversaire => Categorie::AnniversaireSaisi,
            config::Genre::Fete => Categorie::FeteSaisie,
        }
    }
}

/// Une fête : son nom et sa famille.
pub struct Fete {
    pub nom: &'static str,
    pub categorie: Categorie,
    /// **LA FORME DE RÉFÉRENCE, QUAND CE NOM EST UNE VARIANTE ORTHOGRAPHIQUE** —
    /// vide sinon (voir `fetes_france.tsv`, 5ᵉ colonne).
    ///
    /// ⚠️ **UNE VARIANTE EST STOCKÉE MAIS PAS AFFICHÉE** : le 24 juin porte
    /// dix-huit formes de « Jean-X » (avec et sans trait d'union), et les montrer
    /// toutes serait absurde. `fetes_du_jour` les écarte, `chercher` les trouve
    /// — c'est tout l'intérêt : taper « Jean Baptiste » doit répondre.
    pub variante_de: &'static str,
}

/// La table, lue UNE fois — voir `TABLE_SOURCE`. Le fichier est embarqué dans
/// le binaire (`include_str!`), donc il ne peut pas manquer à l'exécution.
static TABLE: std::sync::OnceLock<Vec<(u32, u32, Fete)>> = std::sync::OnceLock::new();

/// Le fichier de la table, embarqué — **il est lu au premier appel, pas au
/// démarrage** : une application qui n'ouvre jamais le calendrier ne paie pas
/// sa lecture.
const TABLE_SOURCE: &str = include_str!("fetes_france.tsv");

fn table() -> &'static [(u32, u32, Fete)] {
    TABLE.get_or_init(|| {
        TABLE_SOURCE
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .filter_map(|l| {
                let mut c = l.split('\t');
                let mois: u32 = c.next()?.trim().parse().ok()?;
                let jour: u32 = c.next()?.trim().parse().ok()?;
                // Le nom n'est PAS recopié : c'est un morceau de `TABLE_SOURCE`,
                // qui vit aussi longtemps que le binaire — les tranches d'un
                // `&'static str` sont elles-mêmes `&'static str`, donc aucun
                // `String` et aucune allocation par entrée.
                let nom: &'static str = c.next()?.trim();
                let categorie = match c.next()?.trim() {
                    "saint" => Categorie::Saint,
                    "ferie" => Categorie::Ferie,
                    "civil" => Categorie::Civil,
                    _ => Categorie::Tradition,
                };
                // 5ᵉ colonne : la forme de référence d'une variante. Absente ou
                // vide = ce nom EST la référence (voir `Fete::variante_de`).
                let variante_de: &'static str = c.next().map(str::trim).unwrap_or("");
                Some((mois, jour, Fete { nom, categorie, variante_de }))
            })
            .collect()
    })
}

/// Le jour de la semaine d'une date, **0 = dimanche** — calculé par WINDOWS
/// plutôt que par une formule réécrite à la main, exactement comme
/// `win32::calendar` le fait pour sa grille (voir sa doc de module : le dépôt a
/// choisi de laisser Windows faire ce calcul plutôt que d'écrire Zeller).
///
/// `None` si Windows refuse la conversion — une date invalide, ce qui ne
/// devrait pas arriver puisque toutes les dates manipulées ici sont construites
/// par `plus_jours` ou lues du calendrier.
fn jour_de_la_semaine(date: Date) -> Option<u32> {
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToFileTime};
    let st = SYSTEMTIME {
        wYear: date.annee as u16,
        wMonth: date.mois as u16,
        wDay: date.jour as u16,
        ..Default::default()
    };
    let mut ft = Default::default();
    unsafe { SystemTimeToFileTime(&st, &mut ft).ok()? };
    let mut sortie = SYSTEMTIME::default();
    unsafe { FileTimeToSystemTime(&ft, &mut sortie).ok()? };
    Some(sortie.wDayOfWeek as u32)
}

/// Le `n`-ième dimanche d'un mois (`n` = 1 pour le premier) — `None` si le mois
/// n'en compte pas autant, ce qui ne peut pas arriver pour n ≤ 5.
fn n_ieme_dimanche(annee: i32, mois: u32, n: u32) -> Option<Date> {
    let premier = Date { annee, mois, jour: 1 };
    let decalage = jour_de_la_semaine(premier)?; // 0 = dimanche
    let jour = 1 + (7 - decalage) % 7 + (n - 1) * 7;
    (jour <= jours_dans_le_mois(annee, mois)).then_some(Date { annee, mois, jour })
}

/// Le DERNIER dimanche d'un mois — même calcul, en partant de la fin.
fn dernier_dimanche(annee: i32, mois: u32) -> Option<Date> {
    let dernier = Date { annee, mois, jour: jours_dans_le_mois(annee, mois) };
    let recul = jour_de_la_semaine(dernier)?; // 0 = dimanche
    Some(Date { annee, mois, jour: dernier.jour - recul })
}

/// LES TROIS FÊTES À DATE CALCULÉE que la table laisse en `VARIABLE` — elles
/// tombent un dimanche compté depuis un bord du mois, jamais une date fixe.
///
/// ⚠️ **La règle appliquée est celle du fichier fourni** (« dernier dimanche de
/// mai » pour la Fête des Mères), et NON celle du calendrier officiel, qui
/// déplace la Fête des Mères au premier dimanche de juin quand elle tombe le
/// jour de Pentecôte. Suivre la source de l'utilisateur plutôt qu'une règle
/// qu'il n'a pas donnée : le jour où la nuance comptera, c'est ici qu'il
/// faudra la corriger.
fn fetes_a_date_calculee(date: Date) -> Vec<Fete> {
    let mut out = Vec::new();
    if n_ieme_dimanche(date.annee, 3, 1) == Some(date) {
        out.push(Fete { nom: "Fête des Grands-Mères", categorie: Categorie::Civil, variante_de: "" });
    }
    if dernier_dimanche(date.annee, 5) == Some(date) {
        out.push(Fete { nom: "Fête des Mères", categorie: Categorie::Civil, variante_de: "" });
    }
    if n_ieme_dimanche(date.annee, 6, 3) == Some(date) {
        out.push(Fete { nom: "Fête des Pères", categorie: Categorie::Civil, variante_de: "" });
    }
    out
}

/// **TOUT CE QUI SE DIT SUR CETTE DATE** : les fêtes de la table, les jours
/// fériés légaux (calculés, donc jamais périmés), et les fêtes à date calculée.
///
/// **LA TABLE PILOTE L'AFFICHAGE, LE CALCUL FOURNIT LES DATES MOBILES** — et
/// c'est ce qui évite les doublons : les sept fériés à date FIXE figurent dans
/// la table, donc `jours_feries` ne les rajoute pas ; les trois autres (lundi
/// de Pâques, Ascension, lundi de Pentecôte) n'y sont qu'en `VARIABLE`, donc
/// c'est le calcul qui les apporte.
///
/// ⚠️ Le 1er janvier est la seule divergence assumée : la table le classe
/// « Fête civile », alors que c'est un jour férié légal — `est_ferie` suit la
/// loi, pas la classification de la table (voir le test qui le fige).
pub fn fetes_du_jour(date: Date) -> Vec<Fete> {
    let mut out: Vec<Fete> = table()
        .iter()
        // ⚠️ **LES VARIANTES SONT ÉCARTÉES DE L'AFFICHAGE** — voir
        // `Fete::variante_de` : sans ce filtre, le 24 juin afficherait dix-huit
        // lignes « Jean-X ». Elles restent cherchables.
        .filter(|(mois, jour, f)| *mois == date.mois && *jour == date.jour && f.variante_de.is_empty())
        .map(|(_, _, f)| Fete { nom: f.nom, categorie: f.categorie, variante_de: f.variante_de })
        .collect();

    // Fériés MOBILES seulement — les fixes sont déjà dans la table.
    for f in jours_feries(date.annee) {
        if f.date == date && !out.iter().any(|e| e.nom == f.nom) && !ferie_fixe(f.nom) {
            out.push(Fete { nom: f.nom, categorie: Categorie::Ferie, variante_de: "" });
        }
    }
    // Et une date fériée que la table aurait omise ne doit pas disparaître pour
    // autant : c'est `est_ferie` qui répond, mais autant la montrer.
    if let Some(nom) = ferie_a(date) {
        if !out.iter().any(|e| e.categorie == Categorie::Ferie) {
            out.push(Fete { nom, categorie: Categorie::Ferie, variante_de: "" });
        }
    }

    out.extend(fetes_a_date_calculee(date));
    out
}

/// Les fériés dont la DATE est fixe — ceux que la table porte déjà, et qu'il ne
/// faut donc pas ajouter une seconde fois.
fn ferie_fixe(nom: &str) -> bool {
    matches!(nom, "Jour de l'An" | "Fête du Travail" | "Victoire 1945" | "Fête nationale" | "Assomption" | "Toussaint" | "Armistice 1918" | "Noël")
}

// ---------------------------------------------------------------------------
// LES ÉVÈNEMENTS D'UN MOIS — ce que le mini calendrier de l'horloge affiche
// ---------------------------------------------------------------------------

/// Un évènement d'un jour, prêt à afficher — **le strict nécessaire, et rien de
/// plus** : l'affichage n'a pas besoin de savoir d'où il vient au-delà de sa
/// famille.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Evenement {
    pub nom: String,
    pub categorie: Categorie,
    /// L'ANNÉE d'un anniversaire, quand elle est connue — elle ne sert qu'à dire
    /// l'âge, jamais à décider quoi que ce soit (voir `config::DatePersonnalisee`).
    pub annee: Option<u32>,
    /// **L'ALARME EST-ELLE ARMÉE ?** — demande explicite (2026-09-24) : « fais
    /// ressortir les éléments pour lesquels une alarme est activée ».
    ///
    /// ⚠️ **SEULES LES DATES SAISIES PEUVENT L'ARMER** : une fête de la table n'a
    /// pas de drapeau à elle (elle vient du calendrier), donc ce champ est
    /// toujours `false` pour elles — voir les deux constructions, plus bas.
    /// C'est exactement ce que la demande visait : « en fonction de si c'est une
    /// fête ou un anniversaire », les deux familles de l'utilisateur.
    pub rappel: bool,
}

/// **LES ÉVÈNEMENTS D'UN MOIS, GROUPÉS PAR JOUR** — la seule chose que le mini
/// calendrier de l'horloge demande, et **PURE, donc testable**.
///
/// Ne rend QUE les jours qui ont quelque chose : un mois ordinaire en a une
/// quinzaine, et la page n'a rien à faire de trente et un tableaux vides.
///
/// ⚠️ **`fetes::rappels_du_jour` N'EST PAS UTILISÉE ICI, ET C'EST DÉLIBÉRÉ** :
/// c'est la décision « faut-il sonner la cloche », filtrée par DEUX réglages de
/// l'utilisateur (le drapeau de chaque date, et l'interrupteur global des fériés).
/// Elle **perdrait silencieusement** des évènements qu'un calendrier doit montrer.
/// On prend donc `fetes_du_jour` (table + fériés fixes + fériés mobiles calculés)
/// **et** les dates saisies, sans aucun filtre.
///
/// ⚠️ **C'est `jours_dans_le_mois` qui impose que cette fonction vive ICI** : elle
/// y est privée, et c'est le seul moyen propre de connaître la longueur d'un mois —
/// la déduire ailleurs rouvrirait la question des années bissextiles, que ce
/// fichier traite déjà.
/// ⚠️ **`rappels` SERT À DIRE QUELLES FÊTES DE LA TABLE SONT SUIVIES** — c'est ce
/// qui fait ressortir leurs lettres dans le mini calendrier (voir
/// `Evenement::rappel`). Sans lui, un saint sur lequel l'utilisateur a posé une
/// alarme ne se distinguerait pas des autres, alors que la liste FÊTES de
/// l'applet l'affiche, elle, avec sa cloche pleine.
pub fn evenements_du_mois(
    annee: i32,
    mois: u32,
    dates: &[config::DatePersonnalisee],
    rappels: &[config::RappelFete],
) -> Vec<(u32, Vec<Evenement>)> {
    let mut par_jour: Vec<(u32, Vec<Evenement>)> = Vec::new();
    // ⚠️ **LA BORNE EST LA LONGUEUR RÉELLE DU MOIS, ET ELLE PERD UNE DATE
    // IMPOSSIBLE — c'est assumé.** Le dépôt accepte la « borne LARGE » (31 pour
    // tous les mois, voir `cef_calendar_applet::analyser_date`) et rien ne
    // confronte ensuite le jour à la longueur de son mois : un **31 février** peut
    // donc exister. La grille n'a pas de cellule « 31 » en février, donc cet
    // évènement n'aurait **nulle part où s'afficher** — il est invisible de toute
    // façon. Le pire serait de le replier sur le 28, ce qui serait un mensonge.
    for jour in 1..=jours_dans_le_mois(annee, mois) {
        let mut du_jour: Vec<Evenement> = fetes_du_jour(Date { annee, mois, jour })
            .into_iter()
            // ⚠️ La VARIANTE orthographique est écartée, comme partout ailleurs
            // (voir `Fete::variante_de`) : le 24 juin porte dix-huit formes de
            // « Jean-X », et les afficher serait absurde.
            // ⚠️ **L'ALARME D'UNE FÊTE DE LA TABLE VIT DANS `rappels_fetes.toml`**,
            // pas sur la fête elle-même : on l'interroge par le TRIPLET
            // `(mois, jour, nom)` — la même clé que la cloche de l'applet et que
            // la recherche, donc les trois surfaces ne peuvent pas diverger.
            .map(|f| Evenement {
                rappel: rappel_fete_actif(rappels, mois, jour, f.nom),
                nom: f.nom.to_string(),
                categorie: f.categorie,
                annee: None,
            })
            .collect();
        for d in dates.iter().filter(|d| d.mois == mois && d.jour == jour) {
            du_jour.push(Evenement {
                nom: d.nom.clone(),
                categorie: Categorie::depuis_genre(d.genre),
                annee: d.annee,
                rappel: d.rappel,
            });
        }
        if !du_jour.is_empty() {
            par_jour.push((jour, du_jour));
        }
    }
    par_jour
}

/// Cette date est-elle un jour férié légal ? **Réponse CALCULÉE**, jamais lue
/// de la table : c'est la loi qui tranche, et elle inclut le 1er janvier, que
/// la table range parmi les fêtes civiles.
#[cfg(test)]
pub fn est_ferie(date: Date) -> bool {
    ferie_a(date).is_some()
}

// ---------------------------------------------------------------------------
// LES RAPPELS — ce que la cloche de la barre du haut a à signaler
// ---------------------------------------------------------------------------

/// D'où vient un rappel — la fenêtre du calendrier s'en sert pour présenter la
/// ligne (« Anniversaire de Léa » n'est pas « Jour férié : Noël »).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RaisonRappel {
    /// **Une date SAISIE** — et elle porte son GENRE, parce qu'un anniversaire
    /// et une fête personnalisée ne se présentent pas pareil. Les confondre sous
    /// un seul nom (c'était le cas quand cette variante s'appelait
    /// `Anniversaire`) aurait fait dire « Anniversaire de… » à une fête que
    /// l'utilisateur n'a jamais présentée comme telle.
    Saisie(config::Genre),
    /// Un jour férié légal — voir `jours_feries`, qui le CALCULE.
    Ferie,
    /// **Une fête de la TABLE à laquelle l'utilisateur a posé un rappel** —
    /// ni une date saisie, ni un jour férié : une entrée du fichier des fêtes
    /// qu'il a choisie de suivre (voir `config::RappelFete`).
    FeteDeLaTable,
    /// **L'ALARME D'UNE NOTE** — demandé le 2026-09-24 : « mettre une alarme sur
    /// les notes créées depuis le dash note ». Elle ne vient d'aucun calendrier :
    /// c'est l'utilisateur qui l'a posée sur une note, et c'est la seule source
    /// qui puisse sonner (voir `win32::son`).
    Note,
}

/// Un rappel du jour.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rappel {
    pub nom: String,
    pub raison: RaisonRappel,
    /// **QUAND LE RAPPEL TOMBE, QUAND CELA SE PRÉCISE AUTREMENT QUE PAR LE JOUR**
    /// — `None` pour tout ce qui vaut pour la JOURNÉE ENTIÈRE (fériés, fêtes de
    /// la table, dates saisies) ; `Some("AAAA-MM-JJTHH:MM")` pour une alarme de
    /// note, qui ne vaut qu'à un instant.
    ///
    /// ⛔ **ELLE ENTRE DANS L'EMPREINTE D'ACQUITTEMENT, ET C'EST SA RAISON D'ÊTRE.**
    /// Sans elle, l'identité d'un rappel de note se réduisait au **nom de la
    /// note** : changer l'heure d'une alarme DÉJÀ ACQUITTÉE le même jour la
    /// laissait silencieuse, puisque l'empreinte ne bougeait pas.
    ///
    /// Défaut RÉEL signalé le 2026-09-24 : « j'ai mis aujourd'hui et 13h16 mais
    /// l'alarme ne s'est pas déclenchée, de plus la cloche apparaît en permanence
    /// près de l'horloge ». Un seul défaut, deux symptômes — la cloche était
    /// VISIBLE (`rappels_du_jour` non vide) et NE CLIGNOTAIT PAS (`cloche_clignote`
    /// faux), et le son ne part que si elle clignote (voir `app::run`).
    /// L'acquittement datait de 10:55, quand l'alarme portait encore une autre heure.
    pub precision: Option<String>,
}

impl Rappel {
    /// **LE RAPPEL D'UN JOUR ENTIER** — le cas de tout sauf une alarme de note :
    /// son nom et sa raison suffisent à l'identifier, il n'a pas d'heure.
    fn du_jour(nom: String, raison: RaisonRappel) -> Self {
        Self { nom, raison, precision: None }
    }

    /// **L'IDENTITÉ D'UN RAPPEL, EN UNE LIGNE** — `"raison:nom"`, ou
    /// `"raison:nom@precision"` quand il porte un instant.
    ///
    /// ⚠️ **ELLE SERT DEUX FOIS**, et c'est pourquoi elle vit ici plutôt que chez
    /// l'un de ses lecteurs : elle entre dans l'**EMPREINTE DU JOUR** (voir
    /// `empreinte_des_rappels`), et c'est elle aussi qu'on acquitte **un par un**
    /// depuis le panneau Notes (voir `win32::cef_notes::handle_acquitter`) —
    /// « un bouton par note » veut dire qu'acquitter l'une ne fait pas taire
    /// l'autre, donc l'acquittement des notes est un ENSEMBLE d'identités et non
    /// une empreinte unique comme celui de la cloche.
    pub fn identite(&self) -> String {
        match &self.precision {
            Some(precision) => format!("{}:{}@{}", self.raison.cle(), self.nom, precision),
            None => format!("{}:{}", self.raison.cle(), self.nom),
        }
    }
}

thread_local! {
    /// Le réglage « rappeler les jours fériés », EN MÉMOIRE — lu au démarrage
    /// depuis la configuration (voir `app::run`) et changé par le panneau de
    /// réglages. Même mécanisme que `ui::dock::logo_is_circle` : un état de
    /// dessin lu sans le faire traverser tous les appelants.
    static RAPPEL_FERIES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Le réglage « rappeler les jours fériés » — DEMANDE EXPLICITE (2026-09-23) :
/// « la possibilité de mettre un rappel automatique sur les dates des fêtes ou
/// des anniversaires ».
///
/// **GLOBAL, contrairement aux anniversaires** : on ne coche pas onze fériés un
/// par un, alors qu'on choisit ses anniversaires un par un (voir
/// `config::Anniversaire::rappel`). Les deux réglages sont donc de deux
/// natures, et c'est `rappels_du_jour` qui les réunit.
pub fn rappel_feries() -> bool {
    RAPPEL_FERIES.with(std::cell::Cell::get)
}

pub fn set_rappel_feries(actif: bool) {
    RAPPEL_FERIES.with(|cell| cell.set(actif));
}

/// **CE QU'IL Y A À RAPPELER À CETTE DATE** — la fonction que la cloche
/// interroge, et la SEULE décision de tout le mécanisme d'alerte.
///
/// Un anniversaire ne rappelle que si SON drapeau est posé (on choisit ses
/// anniversaires un par un) ; un jour férié ne rappelle que si le réglage
/// GLOBAL l'est (on ne coche pas onze fériés un par un). Les deux réglages sont
/// donc séparés, et c'est voulu — voir `config::load_rappel_feries`.
///
/// **UNE LISTE VIDE VEUT DIRE « RIEN À SIGNALER »**, et c'est la réponse la
/// plus fréquente : c'est elle qui éteint la cloche.
///
/// Un anniversaire tombe chaque année à la même date : seule (jour, mois)
/// compte, jamais l'année — voir `config::Anniversaire`, qui ne la stocke pas.
pub fn rappels_du_jour(date: Date, dates: &[config::DatePersonnalisee], rappels: &[config::RappelFete], rappel_feries: bool) -> Vec<Rappel> {
    let mut out = Vec::new();
    if rappel_feries {
        // La table d'abord : elle nomme le férié comme l'utilisateur le voit
        // (et couvre le 1er janvier au même titre que les autres).
        for f in fetes_du_jour(date) {
            if f.categorie == Categorie::Ferie {
                out.push(Rappel::du_jour(f.nom.to_string(), RaisonRappel::Ferie));
            }
        }
    }
    // **LES FÊTES DE LA TABLE À RAPPELER** — DEMANDE EXPLICITE (2026-09-23).
    // Elles se cherchent par leur triplet complet, jamais par la seule date :
    // 54 JOURS de l'année portent DEUX fêtes (mesuré le 2026-09-24, voir
    // `config::RappelFete`).
    for f in fetes_du_jour(date) {
        if rappel_fete_actif(rappels, date.mois, date.jour, f.nom) {
            out.push(Rappel::du_jour(f.nom.to_string(), RaisonRappel::FeteDeLaTable));
        }
    }
    for d in dates {
        if d.rappel && d.jour == date.jour && d.mois == date.mois {
            out.push(Rappel::du_jour(d.nom.clone(), RaisonRappel::Saisie(d.genre)));
        }
    }
    out
}

/// **L'ALARME D'UNE NOTE, EN UNE FORME STABLE POUR L'EMPREINTE** —
/// `AAAA-MM-JJTHH:MM`, chaque moitié absente remplacée par `*`.
///
/// ⛔ **C'EST ELLE QUI FAIT QU'UN CHANGEMENT D'HEURE RALLUME LA CLOCHE** (voir
/// `Rappel::precision`) : sans elle, l'identité d'un rappel de note se réduisait
/// au NOM de la note, et régler une alarme à 13 h 16 le jour même où l'on avait
/// acquitté la précédente la laissait muette — défaut signalé le 2026-09-24.
///
/// ⚠️ **LA DATE AUSSI Y EST**, pas seulement l'heure : une alarme passée
/// (elle est « en retard, donc à signaler », voir `AlarmeNote::sonne_a`) et une
/// alarme du jour sont deux rappels différents, même sans heure.
///
/// **PURE**, donc testable sans cloche et sans clic — même raison que
/// `empreinte_des_rappels`.
fn quand_d_alarme(alarme: &notes::AlarmeNote) -> String {
    let date = match alarme.date {
        Some((annee, mois, jour)) => format!("{annee:04}-{mois:02}-{jour:02}"),
        None => "*".to_string(),
    };
    let heure = match alarme.heure {
        Some((h, m)) => format!("{h:02}:{m:02}"),
        None => "*".to_string(),
    };
    // ⚠️ **LA RÉPÉTITION S'AJOUTE SANS RIEN CHANGER AUX AUTRES** : une alarme
    // « une fois » garde exactement sa forme d'avant, donc son acquittement du
    // jour reste reconnu.
    match alarme.jours {
        0 => format!("{date}T{heure}"),
        jours => format!("{date}T{heure}R{jours}"),
    }
}

/// **LES ALARMES DE NOTES QUI DOIVENT SONNER MAINTENANT** — fonction SŒUR de
/// `rappels_du_jour`, et non un cinquième paramètre de celle-ci.
///
/// ⚠️ **POURQUOI UNE FONCTION SŒUR.** Ajouter `notes` à `rappels_du_jour` aurait
/// touché **dix-huit sites d'appel**, dont dix-sept dans les tests de ce module,
/// pour une source que seuls TROIS appelants connaissent (`app::run` deux fois,
/// et la republication du calendrier). La sœur garde la signature existante
/// intacte, et les appelants qui veulent le tableau complet **fusionnent les
/// deux listes** — ce qu'ils font déjà pour rien d'autre.
///
/// ⚠️ **ELLE PREND `maintenant` EN SECONDES UNIX**, pas une `Date` : une alarme de
/// note a une HEURE, pas un jour. C'est la seule source de rappel dans ce cas, et
/// lui passer une date aurait perdu l'heure — donc fait sonner une alarme de
/// 15 h 30 dès minuit.
///
/// ⚠️ **LE NOM EST LA PREMIÈRE LIGNE DE LA NOTE**, tronquée. C'est lui qui entre
/// dans l'empreinte d'acquittement : **modifier le texte d'une note déjà
/// acquittée la fait donc resonner**. C'est assumé — le rappel a changé — et
/// c'est le prix d'un nom lisible plutôt que d'un « note #7 » incompréhensible
/// dans le panneau.
pub fn rappels_des_notes(
    aujourdhui: (i32, u32, u32),
    maintenant: (u32, u32),
    notes: &[notes::Note],
) -> Vec<Rappel> {
    let mut out = Vec::new();
    // ⚠️ **UNE NOTE ARCHIVÉE SE TAIT** (voir `Note::archived`) : elle n'est plus
    // dans la liste du panneau, son rappel n'y serait pas acquittable.
    for n in notes.iter().filter(|n| !n.archived) {
        let Some(alarme) = &n.alarme else { continue };
        if !alarme.sonne_a(aujourdhui, maintenant) {
            continue;
        }
        out.push(rappel_de_note(n, alarme));
    }
    out
}

/// **LE RAPPEL D'UNE NOTE, CONSTRUIT EN UN SEUL ENDROIT** — pour que la liste du
/// jour et l'acquittement du panneau Notes ne puissent pas **diverger**. S'ils
/// divergeaient, acquitter une note ne la ferait pas taire — ou ferait taire sa
/// voisine, ce qui est pire.
fn rappel_de_note(note: &notes::Note, alarme: &notes::AlarmeNote) -> Rappel {
    Rappel {
        nom: resume_de_note(&note.content),
        raison: RaisonRappel::Note,
        precision: Some(quand_d_alarme(alarme)),
    }
}

/// **L'IDENTITÉ DU RAPPEL D'UNE NOTE** — ce que le panneau Notes acquitte (voir
/// `win32::cef_notes::handle_acquitter`), dans la forme exacte où
/// `rappels_des_notes` la met dans la liste du jour : les deux passent par
/// `rappel_de_note`, donc ils ne peuvent pas se contredire.
///
/// `None` quand la note n'a pas d'alarme : il n'y a alors rien à acquitter.
pub fn identite_du_rappel_de_note(note: &notes::Note) -> Option<String> {
    let alarme = note.alarme.as_ref()?;
    Some(rappel_de_note(note, alarme).identite())
}

/// **LES ALARMES DE NOTES D'UN MOIS, GROUPÉES PAR JOUR** — sœur de
/// `evenements_du_mois`, et pour la même raison que `rappels_des_notes` : ajouter
/// `notes` à sa signature aurait touché ses dix-huit appelants.
///
/// DEMANDE EXPLICITE (2026-09-24) : « que les notes programmées apparaissent sous
/// les prénoms dans la liste de droite du menu de l'horloge ».
///
/// ⚠️ **UNE ALARME SANS DATE N'APPARAÎT NULLE PART** : elle sonne « tout de
/// suite », donc elle n'a pas de jour où se ranger. C'est le cas de l'alarme
/// qu'on vient de poser, et lui inventer la date du jour mentirait sur ce qu'elle
/// est — elle ne reviendra pas demain.
///
/// ⚠️ **UNE ALARME RÉPÉTÉE, ELLE, REVIENT** : elle se range sous CHAQUE jour
/// choisi du mois, à partir de sa date de début s'il y en a une.
pub fn alarmes_de_notes_du_mois(annee: i32, mois: u32, notes: &[notes::Note]) -> Vec<(u32, Evenement)> {
    let mut out = Vec::new();
    // Une note archivée n'apparaît plus au calendrier — comme elle ne sonne plus.
    for n in notes.iter().filter(|n| !n.archived) {
        let Some(alarme) = &n.alarme else { continue };
        let evenement = || Evenement {
            nom: resume_de_note(&n.content),
            categorie: Categorie::Note,
            annee: None,
            // Une alarme EST un rappel : elle porte sa propre cloche.
            rappel: true,
        };
        if alarme.jours != 0 {
            for j in 1..=jours_dans_le_mois(annee, mois) {
                let jour = (annee, mois, j);
                if alarme.tombe_un(jour) && alarme.date.is_none_or(|debut| debut <= jour) {
                    out.push((j, evenement()));
                }
            }
            continue;
        }
        let Some((a, m, j)) = alarme.date else { continue };
        if a != annee || m != mois {
            continue;
        }
        out.push((j, evenement()));
    }
    out
}

/// La première ligne non vide du contenu d'une note, tronquée — ce qui la NOMME
/// dans les rappels.
///
/// Une note peut être **vide de texte** (une image collée seule, voir
/// `Note::is_empty`) : elle a alors un nom de repli, sinon l'empreinte porterait
/// une chaîne vide et deux alarmes de notes différentes se confondraient.
fn resume_de_note(content: &str) -> String {
    const MAX: usize = 60;
    let ligne = content.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if ligne.is_empty() {
        return "(note sans texte)".to_string();
    }
    if ligne.chars().count() <= MAX {
        return ligne.to_string();
    }
    let tronque: String = ligne.chars().take(MAX).collect();
    format!("{tronque}…")
}

impl RaisonRappel {
    /// La CLÉ stable d'une raison — sans accent, minuscule, et **qui ne doit
    /// jamais changer** : elle entre dans l'EMPREINTE des rappels, qui est
    /// **persistée**. La renommer ferait clignoter la cloche une fois de plus
    /// après une mise à jour — sans gravité, mais c'est une raison de plus de ne
    /// pas y toucher. Même contrat que `Categorie::cle` et `Genre::cle`.
    fn cle(self) -> &'static str {
        match self {
            RaisonRappel::Saisie(config::Genre::Anniversaire) => "anniversaire",
            RaisonRappel::Saisie(config::Genre::Fete) => "fete",
            RaisonRappel::Ferie => "ferie",
            RaisonRappel::FeteDeLaTable => "fete-table",
            // ⛔ **CETTE CHAÎNE EST PERSISTÉE** (elle entre dans l'empreinte, que
            // `config::save_rappel_ack` écrit dans `rappel_ack.txt`) : la changer
            // un jour ferait resonner toutes les alarmes déjà acquittées, une
            // fois. Même contrat que `Categorie::cle` et `Genre::cle`.
            RaisonRappel::Note => "note",
        }
    }
}

/// **L'EMPREINTE DES RAPPELS D'UN JOUR** — ce que la cloche retient quand on
/// clique dessus, et ce qui décide de son clignotement.
///
/// **PURE, et c'est ce qui la rend testable** : elle porte la SEULE décision de
/// tout le mécanisme d'acquittement, et la vérifier par la cloche demanderait de
/// cliquer — ce que ce dépôt ne fait jamais sur un poste de travail réel.
///
/// Elle contient **LA DATE ET LES RAPPELS** — chaque rappel y entrant par sa
/// raison, son nom, et sa PRÉCISION quand il en a une (`Rappel::precision`).
/// Quatre conséquences, toutes voulues (DEMANDE EXPLICITE de l'utilisateur :
/// « cliquer acquitte ce qui était affiché ») :
/// * deux jours qui portent le même nom donnent deux empreintes différentes —
///   la cloche se rallume le lendemain ;
/// * **un rappel AJOUTÉ APRÈS un acquittement change l'empreinte** — la cloche
///   se rallume, au lieu de rester éteinte sur une journée qu'on vient de modifier ;
/// * allumer le rappel des fériés la rallume aussi, pour la même raison ;
/// * **changer l'HEURE d'une alarme de note la rallume**, même quand la note et
///   le jour n'ont pas bougé — c'est le défaut du 2026-09-24, et le récit
///   complet est sur `Rappel::precision`.
///
/// Rend **`None` quand il n'y a RIEN à signaler** : c'est la réponse la plus
/// fréquente, et elle ne doit ni s'acquitter ni clignoter.
pub fn empreinte_des_rappels(date: Date, rappels: &[Rappel]) -> Option<String> {
    if rappels.is_empty() {
        return None;
    }
    // Les rappels sont TRIÉS avant d'entrer dans l'empreinte : `rappels_du_jour`
    // rend aujourd'hui ses trois familles dans un ordre stable, mais rien ne le
    // garantit à l'avenir — et une empreinte sensible à l'ordre se remettrait à
    // clignoter pour un simple réordonnancement, sans qu'aucun rappel ait changé.
    // ⚠️ **LES RAPPELS SANS PRÉCISION GARDENT EXACTEMENT L'EMPREINTE D'AVANT**
    // (voir `Rappel::identite` : la précision est derrière un `@`, et seulement
    // quand il y en a une). Un acquittement déjà écrit reste donc valide pour
    // eux, et seule une alarme de note se rallume une fois — ce qui est le but.
    let mut lignes: Vec<String> = rappels.iter().map(Rappel::identite).collect();
    lignes.sort();
    Some(format!("{} {:02} {:02}|{}", date.annee, date.mois, date.jour, lignes.join("|")))
}

thread_local! {
    /// **L'EMPREINTE DES RAPPELS DÉJÀ VUS** — voir `empreinte_des_rappels`.
    ///
    /// En MÉMOIRE, sur le même motif que `RAPPEL_FERIES` ci-dessus : lue au
    /// démarrage depuis la configuration et écrite au clic sur la cloche, **les
    /// deux jamais l'un sans l'autre**.
    ///
    /// ⚠️ **C'est une CHAÎNE et non un booléen, et c'est tout l'intérêt** :
    /// acquitter n'éteint pas « les rappels », mais **ceux qui étaient affichés**.
    /// Un booléen par jour aurait laissé la cloche muette après l'ajout d'un
    /// rappel le même jour.
    static RAPPELS_ACQUITTES: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// L'empreinte acquittée — `""` quand rien n'a jamais été acquitté. Une chaîne
/// vide ne peut coïncider avec aucune empreinte, qui commence toujours par la date.
pub fn rappels_acquittes() -> String {
    RAPPELS_ACQUITTES.with(|c| c.borrow().clone())
}

pub fn set_rappels_acquittes(empreinte: &str) {
    RAPPELS_ACQUITTES.with(|c| *c.borrow_mut() = empreinte.to_string());
}

/// **LA CLOCHE DOIT-ELLE CLIGNOTER ?** — la seule question que la barre du haut
/// pose à ce module, et la seule décision qui compte.
///
/// Rien à signaler ⇒ non. L'empreinte du jour déjà acquittée ⇒ non. Toute autre
/// ⇒ oui.
#[cfg(test)]
pub fn cloche_doit_clignoter(
    date: Date,
    dates: &[config::DatePersonnalisee],
    rappels: &[config::RappelFete],
    rappel_feries: bool,
) -> bool {
    cloche_clignote_pour(date, &rappels_du_jour(date, dates, rappels, rappel_feries))
}

/// La même décision, sur les rappels du jour DÉJÀ calculés — `app::run` les a en
/// main pour la vue de la cloche, et les recalculer à chaque frame serait du
/// gaspillage. C'est elle que l'app appelle : `cloche_doit_clignoter` et ses
/// tests passent par ici, ils gardent donc le vrai chemin (l'app refaisait ce
/// calcul à la main, hors de portée des tests, jusqu'au 2026-10-23).
pub fn cloche_clignote_pour(date: Date, du_jour: &[Rappel]) -> bool {
    let Some(empreinte) = empreinte_des_rappels(date, du_jour) else {
        return false;
    };
    empreinte != rappels_acquittes()
}

thread_local! {
    /// **LA DÉCISION DE CLIGNOTEMENT, PUBLIÉE POUR LES AUTRES ÉCRANS.**
    ///
    /// `app::run` la recalcule à chaque frame — elle dépend des dates saisies et
    /// des rappels de fêtes, qui ne vivent QUE là — et la pose ici. Les barres
    /// **SECONDAIRES** la relisent au dessin (voir `dock_instance::DockInstance::
    /// render`) sans qu'on ait à la faire traverser `render_secondary_docks` :
    /// **même mécanisme que `RAPPEL_FERIES` juste au-dessus, et même raison** —
    /// un état de dessin lu sans le faire traverser tous les appelants.
    ///
    /// ⚠️ Elle ne porte QUE la décision, jamais le réglage `features.bell` : celui
    /// est **par écran** (on peut éteindre la cloche sur un seul), et chaque
    /// instance applique donc le sien par-dessus.
    static CLOCHE_CLIGNOTE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Voir `CLOCHE_CLIGNOTE`, doc de la cellule.
pub fn cloche_clignote() -> bool {
    CLOCHE_CLIGNOTE.with(std::cell::Cell::get)
}

/// **LA CLOCHE DOIT-ELLE ÊTRE DESSINÉE ?** — DEMANDE EXPLICITE (2026-09-24) :
/// « s'il n'y a aucun rappel, rendre cette cloche transparente ».
///
/// ⚠️ **PUBLIÉE COMME LE CLIGNOTEMENT, ET POUR LA MÊME RAISON** : la décision se
/// prend à UN endroit (`app::run`, là où les rappels sont connus), mais elle est
/// lue à CINQ : le dessin, le test de clic, l'infobulle, l'union de région et
/// l'animateur de survol. Quatre de ces cinq sites n'ont aucun accès aux dates ni
/// aux notes — leur passer la liste demanderait quatre signatures de plus.
/// La cellule est donc le seul chemin, exactement comme `CLOCHE_CLIGNOTE`.
///
/// ⚠️ **« AUCUN RAPPEL » VEUT DIRE `rappels_du_jour(...).is_empty()`**, pas
/// « rien de NOUVEAU » : une cloche acquittée mais dont les rappels du jour
/// existent encore RESTE visible (en couleur de texte). Elle ne disparaît que
/// quand il n'y a vraiment rien — sans quoi elle s'évanouirait au clic, et
/// l'utilisateur n'aurait plus aucun repère pour rouvrir le panneau.
thread_local! {
    static CLOCHE_VISIBLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Voir `CLOCHE_VISIBLE`, doc de la cellule.
pub fn cloche_visible() -> bool {
    CLOCHE_VISIBLE.with(std::cell::Cell::get)
}

pub fn set_cloche_visible(visible: bool) {
    CLOCHE_VISIBLE.with(|cell| cell.set(visible));
}

pub fn set_cloche_clignote(clignote: bool) {
    CLOCHE_CLIGNOTE.with(|cell| cell.set(clignote));
}

// ---------------------------------------------------------------------------
// L'ICÔNE DE NOTE — les rappels de notes, séparés de la cloche
// ---------------------------------------------------------------------------
//
// DEMANDE EXPLICITE (2026-09-24) : « je voudrais le même mécanisme avec une icône
// de note à gauche de l'horloge ». Les deux familles n'ont pourtant pas la même
// nature — une fête vaut pour un JOUR, une alarme de note pour un INSTANT — et
// elles ont donc leur propre signal, leur propre vue, et leur propre
// acquittement (voir `acquitter_rappel_note`).

/// **LE PRÉFIXE DE JOUR D'UN ENSEMBLE D'ACQUITTEMENTS** — `"AAAA MM JJ"`.
fn prefixe_du_jour(jour: (i32, u32, u32)) -> String {
    format!("{:04} {:02} {:02}", jour.0, jour.1, jour.2)
}

/// **LES RAPPELS DE NOTES ACQUITTÉS POUR UN JOUR** — une liste d'identités, et
/// non l'empreinte unique de la cloche.
///
/// ⚠️ **POURQUOI UN ENSEMBLE ICI ET UNE EMPREINTE LÀ-BAS** : la cloche acquitte
/// « ce qui était affiché » en UN geste, donc une empreinte suffit. Pour les
/// notes, la demande est **un bouton par note** — acquitter celle-ci ne doit pas
/// faire taire celle-là.
///
/// ⚠️ **LE JOUR EST EN TÊTE** (`"AAAA MM JJ|identité|identité"`), comme dans
/// l'empreinte de la cloche : c'est lui qui fait qu'un nouveau jour repart d'un
/// ensemble vide, sans que personne n'ait à effacer quoi que ce soit — et le
/// rappel revient donc le lendemain, comme celui de la cloche.
fn identites_acquittees(stocke: &str, jour: (i32, u32, u32)) -> Vec<&str> {
    let Some((tete, reste)) = stocke.split_once('|') else { return Vec::new() };
    if tete != prefixe_du_jour(jour) {
        return Vec::new();
    }
    reste.split('|').filter(|s| !s.is_empty()).collect()
}

/// **ACQUITTE LE RAPPEL D'UNE NOTE** — rend la chaîne à persister, sans jamais
/// écrire de fichier lui-même (c'est `config` qui possède `rappel_ack_notes.txt`,
/// et une fonction pure se teste sans toucher au disque).
pub fn acquitter_rappel_note(stocke: &str, jour: (i32, u32, u32), identite: &str) -> String {
    let mut lignes = identites_acquittees(stocke, jour);
    if !lignes.contains(&identite) {
        lignes.push(identite);
    }
    let mut out = prefixe_du_jour(jour);
    for ligne in lignes {
        out.push('|');
        out.push_str(ligne);
    }
    out
}

/// **ACQUITTE TOUS LES RAPPELS DE NOTES DU JOUR** — le geste de l'ICÔNE de la
/// barre du haut, qui n'est attachée à aucune note en particulier.
///
/// ⚠️ **CE GESTE EXISTE PARCE QUE L'USAGE L'A DEMANDÉ** : l'icône ne devait
/// qu'ouvrir le panneau, et l'utilisateur a rapporté le 2026-09-24 — « le clic sur
/// la note à côté de l'horloge ne coupe pas la notification ». Un rappel qui sonne
/// doit se taire au geste le plus évident. C'est aussi ce que fait la CLOCHE pour
/// sa propre famille, donc les deux icônes se comportent pareil.
///
/// ⚠️ **LE BOUTON « Acquitter » DU PANNEAU RESTE, LUI, PAR NOTE** — c'est la
/// finesse que ce geste-ci ne peut pas offrir : n'en taire qu'une seule.
///
/// Rend la chaîne à persister, inchangée s'il n'y avait rien à acquitter.
pub fn acquitter_notes_du_jour(stocke: &str, jour: (i32, u32, u32), rappels: &[Rappel]) -> String {
    rappels
        .iter()
        .fold(stocke.to_string(), |ensemble, rappel| {
            acquitter_rappel_note(&ensemble, jour, &rappel.identite())
        })
}

/// **CE RAPPEL DE NOTE EST-IL ACQUITTÉ ?** — la question que pose le panneau
/// Notes pour chaque note (voir `win32::cef_notes::rappel_en_attente`).
pub fn rappel_note_est_acquitte(ensemble: &str, jour: (i32, u32, u32), identite: &str) -> bool {
    identites_acquittees(ensemble, jour).contains(&identite)
}

/// **L'ICÔNE DE NOTE DOIT-ELLE CLIGNOTER ?** — pure, donc testable sans cloche et
/// sans clic (règle du dépôt : on ne clique jamais sur un poste réel).
///
/// Oui s'il reste **au moins un** rappel de note du jour qui n'a pas été
/// acquitté. Rien à signaler ⇒ non.
pub fn note_doit_clignoter(jour: (i32, u32, u32), rappels: &[Rappel], acquittes: &str) -> bool {
    if rappels.is_empty() {
        return false;
    }
    let vues = identites_acquittees(acquittes, jour);
    rappels.iter().any(|r| !vues.contains(&r.identite().as_str()))
}

thread_local! {
    /// **LA DÉCISION DE CLIGNOTEMENT DE L'ICÔNE DE NOTE** — mêmes raisons que
    /// `CLOCHE_CLIGNOTE` : recalculée par `app::run`, seul endroit qui connaît les
    /// notes ET l'heure locale, puis relue au dessin par les barres secondaires.
    /// Elle ne porte QUE la décision, jamais le réglage par écran.
    static NOTE_CLIGNOTE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// **L'ICÔNE DOIT-ELLE ÊTRE DESSINÉE ?** — même distinction que la cloche :
    /// « aucun rappel de note » (elle disparaît) n'est pas « rien de nouveau »
    /// (elle reste, en couleur de texte). Sans quoi elle s'évanouirait à
    /// l'acquittement, et l'utilisateur perdrait son repère vers le panneau.
    static NOTE_VISIBLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

thread_local! {
    /// **L'ENSEMBLE DES RAPPELS DE NOTES ACQUITTÉS** — en MÉMOIRE, sur le même
    /// motif que `RAPPELS_ACQUITTES` : lu au démarrage depuis
    /// `rappel_ack_notes.txt` et écrit au clic sur « Acquitter », **les deux
    /// jamais l'un sans l'autre** (voir `win32::cef_notes::handle_acquitter`).
    ///
    /// ⚠️ **En mémoire, et pas relu du fichier à chaque frame** : la question
    /// « ce rappel est-il acquitté ? » se pose à chaque poussée d'état du panneau
    /// Notes, c'est-à-dire soixante fois par seconde tant qu'il est ouvert — une
    /// lecture de fichier par frame, exactement ce que `SON_GENERATION` évite
    /// déjà de l'autre côté.
    static RAPPELS_NOTES_ACQUITTES: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Voir `RAPPELS_NOTES_ACQUITTES`, doc de la cellule.
pub fn rappels_notes_acquittes() -> String {
    RAPPELS_NOTES_ACQUITTES.with(|c| c.borrow().clone())
}

pub fn set_rappels_notes_acquittes(ensemble: &str) {
    RAPPELS_NOTES_ACQUITTES.with(|c| *c.borrow_mut() = ensemble.to_string());
}

/// Voir `NOTE_CLIGNOTE`, doc de la cellule.
pub fn note_clignote() -> bool {
    NOTE_CLIGNOTE.with(std::cell::Cell::get)
}

pub fn set_note_clignote(clignote: bool) {
    NOTE_CLIGNOTE.with(|cell| cell.set(clignote));
}

/// Voir `NOTE_VISIBLE`, doc de la cellule.
pub fn note_visible() -> bool {
    NOTE_VISIBLE.with(std::cell::Cell::get)
}

pub fn set_note_visible(visible: bool) {
    NOTE_VISIBLE.with(|cell| cell.set(visible));
}

// ---------------------------------------------------------------------------
// LA RECHERCHE
// ---------------------------------------------------------------------------

/// Comment ADRESSER un résultat pour agir dessus — c'est-à-dire pour poser ou
/// retirer son rappel (DEMANDE EXPLICITE, 2026-09-23 : « dans les résultats des
/// fêtes et des anniversaires j'aimerais avoir la possibilité de mettre un
/// rappel ou de le retirer »).
///
/// Les trois familles ne se rappellent pas de la même façon, et la page ne doit
/// PAS le deviner : c'est Rust qui le lui dit, parce que c'est Rust qui sait où
/// chaque chose est rangée.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cible {
    /// Une entrée de la TABLE — désignée par sa date ET son nom (une même date
    /// peut porter deux fêtes, voir `config::RappelFete`).
    Table,
    /// Une date SAISIE — son RANG dans la liste passée à `chercher`, celui que
    /// `calendar/rappel/<index>` attend déjà.
    Saisie(usize),
    /// Une fête à date CALCULÉE (Grands-Mères, Mères, Pères) : ni dans la table,
    /// ni saisie. **ON NE PEUT PAS LA RAPPELER**, faute d'une date dans l'année
    /// à désigner — la page n'affichera donc pas de cloche pour elle, plutôt
    /// qu'une cloche qui ne ferait rien.
    Calculee,
}

/// Un résultat de recherche : ce qu'il faut pour l'afficher ET pour agir dessus.
pub struct ResultatRecherche {
    /// Possédé, et non `&'static str` : un résultat peut venir d'une date
    /// SAISIE, dont le nom est une `String` — pas d'un littéral de la table.
    pub nom: String,
    pub jour: u32,
    pub mois: u32,
    pub categorie: Categorie,
    /// La forme de référence si ce résultat est une variante orthographique,
    /// vide sinon — la page peut alors écrire « = Jean-Baptiste ».
    pub variante_de: &'static str,
    pub cible: Cible,
    /// L'état du rappel POUR CE RÉSULTAT — la page dessine la cloche pleine ou
    /// vide selon lui, sans avoir à le redemander.
    pub rappel: bool,
}

/// Le rappel d'une fête de la table est-il posé ? La clé est le triplet
/// complet, jamais la seule date (voir `config::RappelFete`).
pub fn rappel_fete_actif(rappels: &[config::RappelFete], mois: u32, jour: u32, nom: &str) -> bool {
    rappels.iter().any(|r| r.mois == mois && r.jour == jour && r.nom == nom)
}


/// **REPLIE un texte pour la comparaison** : minuscules, et sans accents.
///
/// POURQUOI LES ACCENTS COMPTENT ICI : on cherche « fete » bien plus souvent
/// qu'on ne tape « fête », et une recherche qui exige l'accent exact rate ce
/// qu'on lui demande. Le repli se fait des DEUX côtés (la requête ET les noms),
/// donc les deux se rencontrent au milieu.
///
/// Volontairement pauvre : les lettres accentuées du français, rien de plus.
/// Une normalisation Unicode complète (NFD + suppression des diacritiques)
/// demanderait une dépendance pour un gain qui ne se voit pas dans les noms
/// manipulés ici.
fn replier(texte: &str) -> String {
    texte
        .chars()
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            'œ' => 'o',
            autre => autre.to_ascii_lowercase(),
        })
        .collect()
}

/// **CHERCHE `texte` DANS TOUT CE QUE LA FENÊTRE SAIT AFFICHER** — les 400
/// entrées de la table, les fêtes à date calculée, ET les dates saisies par
/// l'utilisateur (anniversaires et fêtes personnalisées).
///
/// DEMANDE EXPLICITE (2026-09-23) : « une possibilité de recherche dans le dash
/// calendrier pour rechercher une chaîne de texte », puis « ajoute une recherche
/// des anniversaires ». **Chercher dans ce qu'on voit est la règle** : une
/// fenêtre où l'on peut chercher une fête mais pas le prénom qu'on y a saisi
/// lui-même serait incohérente.
///
/// `dates` : les dates saisies (voir `config::DatePersonnalisee`), dont le RANG
/// devient la `Cible` du résultat. `rappels` : les fêtes de la table à rappeler
/// (voir `config::RappelFete`), qui donnent l'état de la cloche.
///
/// Une requête VIDE rend une liste vide — et non tout : une recherche vide n'est
/// pas une recherche, et renvoyer 400 lignes à chaque effacement du champ ferait
/// clignoter la fenêtre.
pub fn chercher(texte: &str, dates: &[config::DatePersonnalisee], rappels: &[config::RappelFete]) -> Vec<ResultatRecherche> {
    let requete = replier(texte.trim());
    if requete.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<ResultatRecherche> = table()
        .iter()
        .filter(|(_, _, f)| replier(f.nom).contains(&requete))
        .map(|(mois, jour, f)| ResultatRecherche {
            nom: f.nom.to_string(),
            jour: *jour,
            mois: *mois,
            categorie: f.categorie,
            cible: Cible::Table,
            rappel: rappel_fete_actif(rappels, *mois, *jour, f.nom),
            variante_de: f.variante_de,
        })
        .collect();

    // Les trois fêtes à date calculée — leur jour vaut `0` (un SENTINELLE) : elles
    // tombent un dimanche compté depuis un bord du mois, donc leur date n'existe
    // pas dans l'année. La page dit alors « en mai » plutôt qu'un faux jour.
    for (nom, mois) in [("Fête des Grands-Mères", 3u32), ("Fête des Mères", 5), ("Fête des Pères", 6)] {
        if replier(nom).contains(&requete) {
            out.push(ResultatRecherche {
                nom: nom.to_string(),
                jour: 0,
                mois,
                categorie: Categorie::Civil,
                cible: Cible::Calculee,
                rappel: false,
                variante_de: "",
            });
        }
    }

    // **LES DATES SAISIES** — leur rang est celui de `dates`, donc celui que
    // `calendar/rappel/<index>` attend déjà : rien à convertir.
    for (index, d) in dates.iter().enumerate() {
        if replier(&d.nom).contains(&requete) {
            out.push(ResultatRecherche {
                nom: d.nom.clone(),
                jour: d.jour,
                mois: d.mois,
                // La pastille dit la FAMILLE d'une date saisie : le genre, pas
                // une catégorie de la table. `civil` pour une fête personnalisée
                // et `saint` pour un anniversaire seraient un contresens — la
                // page les peint donc avec leurs propres clés.
                // Voir `Categorie::depuis_genre`, seule copie de cette règle.
                categorie: Categorie::depuis_genre(d.genre),
                cible: Cible::Saisie(index),
                rappel: d.rappel,
                variante_de: "",
            });
        }
    }

    out.sort_by_key(|r| (r.mois, r.jour));
    out
}

// ⚠️ **PAS DE PLAFOND ICI, ET C'EST UNE CORRECTION.** Une première version
// tronquait à 60 résultats. Comme le tri se fait par DATE avant la troncature,
// chercher une lettre courante (« a ») ne rendait alors que JANVIER À MARS —
// et la fenêtre laissait croire qu'il n'existait rien d'autre, ce qui est pire
// qu'une longue liste. **C'est un test qui l'a montré** (« la recherche couvre
// toute l'année »), pas une relecture. La fenêtre défile : elle sait en montrer
// cent cinquante.

#[cfg(test)]
mod tests {
    use super::*;

    fn d(annee: i32, mois: u32, jour: u32) -> Date {
        Date { annee, mois, jour }
    }

    /// **LES ANNÉES DE RÉFÉRENCE, CHOISIES POUR COUVRIR LES CAS.** Ces dates ne
    /// viennent PAS de l'algorithme ci-dessus : ce sont des dates de Pâques
    /// publiées, relevées pour éprouver le calcul — sinon le test ne ferait que
    /// confirmer la formule par elle-même. Elles couvrent une Pâques de mars,
    /// une d'avril, une très précoce, une tardive, et deux années bissextiles.
    #[test]
    fn paques_connues() {
        for (annee, attendu) in [
            (2024, d(2024, 3, 31)), // bissextile, Pâques en mars
            (2025, d(2025, 4, 20)),
            (2026, d(2026, 4, 5)),
            (2027, d(2027, 3, 28)), // la plus précoce de la série
            (2028, d(2028, 4, 16)), // bissextile
            (2029, d(2029, 4, 1)),
            (2030, d(2030, 4, 21)), // tardive
            (2038, d(2038, 4, 25)), // la plus tardive possible
        ] {
            assert_eq!(paques(annee), attendu, "Pâques {annee}");
        }
    }

    /// **Pâques tombe toujours entre le 22 mars et le 25 avril** — la fenêtre
    /// fixée par le concile de Nicée, et une propriété qu'un algorithme
    /// approximatif finit toujours par violer quelque part. Le balayage est
    /// large (deux siècles) parce que c'est justement sur les cas rares qu'un
    /// calcul faux se trahit.
    ///
    /// ⚠️ Ce test NE vérifie PAS que Pâques tombe un dimanche : cela demanderait
    /// un calcul de jour de la semaine, et le dépôt a délibérément choisi de
    /// laisser Windows le faire plutôt que de réécrire une formule (voir
    /// `win32::calendar`, doc de module). La fenêtre, elle, se vérifie sans ça.
    #[test]
    fn paques_reste_dans_la_fenetre_du_concile_de_nicee() {
        for annee in 1900..=2100 {
            let p = paques(annee);
            assert!(p.mois == 3 || p.mois == 4, "Pâques {annee} tombe en mois {}", p.mois);
            let dans_la_fenetre = (p.mois == 3 && p.jour >= 22) || (p.mois == 4 && p.jour <= 25);
            assert!(dans_la_fenetre, "Pâques {annee} hors de la fenêtre 22/03–25/04 : {p:?}");
        }
    }

    /// Les onze fériés, **dans l'ordre de l'année** — c'est ce que l'applet
    /// affiche, et un ordre faux ferait paraître la liste désordonnée.
    #[test]
    fn les_onze_feries_sont_dans_l_ordre_de_l_annee() {
        for annee in [2024, 2025, 2026, 2027, 2030] {
            let liste = jours_feries(annee);
            assert_eq!(liste.len(), 11, "onze fériés en {annee}");
            for f in &liste {
                assert_eq!(f.date.annee, annee, "{} n'est pas en {annee}", f.nom);
                assert!(jours_dans_le_mois(f.date.annee, f.date.mois) >= f.date.jour, "{} : jour impossible", f.nom);
            }
            let mut precedent = (0, 0);
            for f in &liste {
                let cle = (f.date.mois, f.date.jour);
                assert!(cle > precedent, "{} arrive après le {} {:?} en {annee}", f.nom, precedent.0, precedent.1);
                precedent = cle;
            }
        }
    }

    /// Les quatre fêtes MOBILES suivent Pâques — vérifié en comparant deux
    /// années dont les Pâques diffèrent, ce qui est le seul cas où une erreur
    /// de décalage se voit.
    #[test]
    fn les_fetes_mobiles_suivent_paques() {
        for annee in [2024, 2025, 2026, 2027, 2030] {
            let p = paques(annee);
            assert_eq!(ferie_a(plus_jours(p, 1)), Some("Lundi de Pâques"), "{annee}");
            assert_eq!(ferie_a(plus_jours(p, 39)), Some("Ascension"), "{annee}");
            assert_eq!(ferie_a(plus_jours(p, 50)), Some("Lundi de Pentecôte"), "{annee}");
        }
        // Ascension est un JEUDI, Pentecôte un LUNDI — deux propriétés du
        // calendrier liturgique qu'un décalage d'un jour casserait.
        assert_eq!(paques(2026), d(2026, 4, 5), "prémisse : Pâques 2026");
        assert_eq!(plus_jours(paques(2026), 39), d(2026, 5, 14), "Ascension 2026");
        assert_eq!(plus_jours(paques(2026), 50), d(2026, 5, 25), "Pentecôte 2026");
    }

    /// Les dates FIXES, une par une — elles ne dépendent d'aucun calcul, donc
    /// ce test les fige contre une faute de frappe.
    #[test]
    fn les_dates_fixes_sont_les_bonnes() {
        for annee in [2025, 2026] {
            assert_eq!(ferie_a(d(annee, 1, 1)), Some("Jour de l'An"));
            assert_eq!(ferie_a(d(annee, 5, 1)), Some("Fête du Travail"));
            assert_eq!(ferie_a(d(annee, 5, 8)), Some("Victoire 1945"));
            assert_eq!(ferie_a(d(annee, 7, 14)), Some("Fête nationale"));
            assert_eq!(ferie_a(d(annee, 8, 15)), Some("Assomption"));
            assert_eq!(ferie_a(d(annee, 11, 1)), Some("Toussaint"));
            assert_eq!(ferie_a(d(annee, 11, 11)), Some("Armistice 1918"));
            assert_eq!(ferie_a(d(annee, 12, 25)), Some("Noël"));
        }
    }

    /// Un jour ordinaire n'est PAS férié — et c'est cette réponse-là que
    /// l'applet affichera le plus souvent. Le 23 septembre est aussi le jour
    /// de l'anniversaire de test de l'applet, d'où son choix.
    #[test]
    fn un_jour_ordinaire_n_est_pas_ferie() {
        assert_eq!(ferie_a(d(2026, 9, 23)), None);
        assert_eq!(ferie_a(d(2026, 6, 15)), None);
        assert_eq!(ferie_a(d(2026, 12, 24)), None, "le 24 décembre n'est pas férié");
    }

    /// Le décalage de jours franchit correctement les fins de mois et d'année —
    /// c'est ce que l'Ascension et la Pentecôte font faire chaque année
    /// (Pâques fin mars + 50 jours = fin mai), et ce qu'une erreur de retenue
    /// casserait.
    #[test]
    fn le_decalage_franchit_les_fins_de_mois_et_d_annee() {
        assert_eq!(plus_jours(d(2026, 3, 28), 50), d(2026, 5, 17), "mars + 50 jours");
        assert_eq!(plus_jours(d(2026, 1, 31), 1), d(2026, 2, 1), "fin de janvier");
        assert_eq!(plus_jours(d(2026, 12, 31), 1), d(2027, 1, 1), "fin d'année");
        assert_eq!(plus_jours(d(2026, 3, 1), -1), d(2026, 2, 28), "retour en février, année ordinaire");
        assert_eq!(plus_jours(d(2028, 3, 1), -1), d(2028, 2, 29), "retour en février, année bissextile");
        assert_eq!(plus_jours(d(2026, 1, 1), -1), d(2025, 12, 31), "retour sur l'année précédente");
    }

    // --- LA TABLE EMBARQUÉE, ET CE QU'ELLE DOIT GARANTIR ---

    /// **LES GARANTIES DE LA TABLE, VÉRIFIÉES SUR LE FICHIER RÉEL** — c'est ce
    /// qui a été contrôlé sur la source fournie par l'utilisateur avant de
    /// l'embarquer, refait ici pour qu'une édition future du `.tsv` ne puisse
    /// pas passer inaperçue.
    #[test]
    fn la_table_couvre_toutes_les_dates_sans_doublon() {
        let t = table();
        // **1275 FORMES STOCKÉES, 1142 AFFICHÉES** — les trois sources réunies.
        // Les variantes sont stockées mais ÉCARTÉES de l'affichage (voir
        // `Fete::variante_de`), d'où l'écart entre les deux comptes.
        //
        // ⚠️ Ces trois nombres ont CHANGÉ DEUX FOIS :
        // - le 2026-09-24, la table a été RÉ-DÉRIVÉE de la source brute
        //   (676 → 556). Le CSV est un calendrier UN-SAINT-PAR-JOUR dont les
        //   jours n'étaient que la POSITION dans la liste — une intruse décalait
        //   tout ce qui suit de +1. Deux sources de dates (le xlsx et
        //   calendrier.com) ont servi d'ANCRES pour réaligner chaque mois. Voir
        //   l'en-tête de `fetes_france.tsv` ;
        // - le 2026-11-04, `tools/saints.json` (un calendrier des saints à
        //   plusieurs noms par jour, DÉJÀ daté) a été FUSIONNÉ : +719 lignes,
        //   aucune retirée — voir `tools/importer_saints.js` et la seconde
        //   section d'en-tête de `fetes_france.tsv`. Le nombre de VARIANTES n'a
        //   pas bougé (133) : l'import n'en crée pas.
        assert_eq!(t.len(), 1275, "formes stockées");
        let affichees = t.iter().filter(|(_, _, f)| f.variante_de.is_empty()).count();
        assert_eq!(affichees, 1142, "formes réellement affichées");
        assert_eq!(t.len() - affichees, 133, "dont 133 variantes, écartées du dessin");

        let mut vues = std::collections::HashSet::new();
        for (mois, jour, f) in t {
            assert!((1..=12).contains(mois), "mois invalide : {mois}");
            assert!(*jour >= 1 && *jour <= jours_dans_le_mois(2024, *mois), "jour invalide : {jour}/{mois}");
            assert!(!f.nom.trim().is_empty(), "entrée sans nom le {jour}/{mois}");
            assert!(vues.insert((*mois, *jour, f.nom)), "doublon : {} le {jour}/{mois}", f.nom);
        }

        // **Chaque date de l'année a AU MOINS une fête** — c'est la propriété
        // qui fait qu'un jour ordinaire n'affiche jamais une fenêtre vide.
        for mois in 1..=12u32 {
            for jour in 1..=jours_dans_le_mois(2024, mois) {
                assert!(
                    t.iter().any(|(m, j, _)| *m == mois && *j == jour),
                    "aucune fête le {jour}/{mois}"
                );
            }
        }
        assert!(t.iter().any(|(m, j, _)| *m == 2 && *j == 29), "le 29 février doit avoir sa fête");
    }

    /// **LA TABLE ET LA LOI CONCORDENT.** Chaque férié DATÉ de la table doit
    /// tomber sur une date que `jours_feries` calcule aussi — c'est le
    /// recoupement qui protège des deux côtés : une date fausse dans la table
    /// comme une erreur dans l'algorithme se verraient ici.
    #[test]
    fn les_feries_dates_de_la_table_sont_bien_des_feries_legaux() {
        let dates: Vec<&str> = table()
            .iter()
            .filter(|(_, _, f)| f.categorie == Categorie::Ferie)
            .map(|(_, _, f)| f.nom)
            .collect();
        assert_eq!(dates.len(), 7, "sept fériés à date fixe dans la table : {dates:?}");

        for (mois, jour, f) in table().iter().filter(|(_, _, f)| f.categorie == Categorie::Ferie) {
            let legal = jours_feries(2026).into_iter().find(|x| x.date.mois == *mois && x.date.jour == *jour);
            assert!(legal.is_some(), "« {} » ({jour}/{mois}) n'est pas un férié légal", f.nom);
        }
        // Et réciproquement, les fériés FIXES calculés doivent tous être dans la
        // table — sinon la table en aurait oublié un.
        for f in jours_feries(2026) {
            if ferie_fixe(f.nom) {
                assert!(
                    table().iter().any(|(m, j, _)| *m == f.date.mois && *j == f.date.jour),
                    "« {} » ({}/{}) manque dans la table",
                    f.nom, f.date.jour, f.date.mois
                );
            }
        }
    }

    /// **LA RÉPARATION DES 16 DATES IMPOSSIBLES, FIGÉE ICI.**
    ///
    /// La source fournie par l'utilisateur portait seize saints poussés
    /// AU-DELÀ de la fin de leur mois (`32/01`, `34/10`, `31/09`…) : ce sont les
    /// SECONDS saints des derniers jours du mois, dans l'ordre, et ils ont été
    /// réaffectés à ces jours — voir l'en-tête de `fetes_france.tsv`.
    ///
    /// Ce test existe pour qu'une RÉGÉNÉRATION du `.tsv` depuis la source brute
    /// ne puisse pas réintroduire les dates impossibles en silence : chaque
    /// saint réparé doit se trouver à sa date reconstruite, et nulle part
    /// ailleurs. Les seize ont été recoupés sur leur fête réelle (Michel au
    /// 29 septembre, Jérôme au 30, Quentin au 31 octobre, Sylvestre au
    /// 31 décembre, André au 30 novembre…).
    #[test]
    fn les_seize_dates_reparees_sont_a_leur_place() {
        for (jour, mois, nom) in [
            (31, 1, "Marcelle"), (30, 4, "Robert"), (31, 5, "Perrine"), (30, 6, "Martial"),
            (30, 7, "Juliette"), (31, 7, "Ignace"), (30, 8, "Fiacre"), (31, 8, "Aristide"),
            (28, 9, "Venceslas"), (29, 9, "Michel"), (30, 9, "Jérôme"),
            (29, 10, "Narcisse"), (30, 10, "Bienvenue"), (31, 10, "Quentin"),
            (30, 11, "André"), (31, 12, "Sylvestre"),
        ] {
            assert!(
                table().iter().any(|(m, j, f)| *m == mois && *j == jour && f.nom == nom),
                "{nom} doit être au {jour}/{mois}"
            );
            // ⚠️ **PAS « une seule fois dans la table »** — ce test a d'abord
            // été écrit ainsi et a échoué sur « Ignace », à raison : il y a bien
            // DEUX saints de ce nom, le 21 décembre et le 31 juillet. Ce qui
            // compte est qu'il n'y en ait qu'un À CETTE DATE, ce qui est déjà
            // garanti par l'unicité (date, nom) du test de la table.
            let combien = table().iter().filter(|(m, j, f)| *m == mois && *j == jour && f.nom == nom).count();
            assert_eq!(combien, 1, "{nom} apparaît {combien} fois au {jour}/{mois}");
        }
    }

    /// Le 1er janvier : **LA SEULE DIVERGENCE ASSUMÉE** entre la table et la
    /// loi. La table le classe « Fête civile » — c'est un choix de
    /// classification, pas une erreur — mais c'est bel et bien un jour férié,
    /// et `est_ferie` répond la loi.
    #[test]
    fn le_premier_janvier_est_ferie_meme_si_la_table_le_dit_civil() {
        assert_eq!(ferie_a(d(2026, 1, 1)), Some("Jour de l'An"));
        assert!(est_ferie(d(2026, 1, 1)));
        let jour = fetes_du_jour(d(2026, 1, 1));
        assert!(
            jour.iter().any(|f| f.categorie == Categorie::Ferie),
            "le 1er janvier doit porter sa marque de férié : {:?}",
            jour.iter().map(|f| f.nom).collect::<Vec<_>>()
        );
    }

    /// `est_ferie` : la loi, et rien d'autre.
    #[test]
    fn est_ferie_suit_la_loi() {
        assert!(est_ferie(d(2026, 12, 25)));
        assert!(est_ferie(d(2026, 5, 1)));
        assert!(est_ferie(d(2026, 5, 14)), "Ascension 2026");
        assert!(!est_ferie(d(2026, 9, 23)));
        assert!(!est_ferie(d(2026, 12, 24)), "le 24 décembre n'est pas férié");
        assert!(!est_ferie(d(2026, 3, 1)), "un dimanche ordinaire n'est pas férié");
    }

    // --- LE JOUR DE LA SEMAINE, ET LES FÊTES QUI EN DÉPENDENT ---

    /// Le jour de la semaine, éprouvé sur des dates dont le jour est
    /// **notoirement connu** — le 1er janvier 2024 était un lundi, et les trois
    /// suivants se déduisent de là (2024 bissextile, 2025 et 2026 ordinaires).
    #[test]
    fn le_jour_de_la_semaine_est_le_bon() {
        for (date, attendu, nom) in [
            (d(2024, 1, 1), 1u32, "lundi"),
            (d(2025, 1, 1), 3, "mercredi"),
            (d(2026, 1, 1), 4, "jeudi"),
            (d(2026, 9, 23), 3, "mercredi"),
            (d(2026, 12, 25), 5, "vendredi"),
        ] {
            assert_eq!(jour_de_la_semaine(date), Some(attendu), "{date:?} doit être un {nom}");
        }
    }

    /// Les fêtes à date calculée tombent bien le **dimanche attendu**.
    ///
    /// Vérifié par deux bouts : le rang (c'est bien le 1ᵉʳ / 3ᵉ dimanche) et le
    /// jour de la semaine. Les dates elles-mêmes sont données à titre de
    /// repère — elles découlent des deux.
    #[test]
    fn les_fetes_a_date_calculee_tombent_le_bon_dimanche() {
        let cas = [
            (2026, 3, 1, 1u32, "Fête des Grands-Mères"),
            (2026, 5, 31, 0, "Fête des Mères"), // `0` = dernier dimanche, pas le n-ième
            (2026, 6, 21, 3, "Fête des Pères"),
        ];
        for (annee, mois, jour_attendu, rang, nom) in cas {
            let date = d(annee, mois, jour_attendu);
            assert_eq!(jour_de_la_semaine(date), Some(0), "{nom} {annee} doit tomber un dimanche");
            if rang == 0 {
                assert_eq!(dernier_dimanche(annee, mois), Some(date), "{nom} {annee}");
            } else {
                assert_eq!(n_ieme_dimanche(annee, mois, rang), Some(date), "{nom} {annee}");
            }
            // Et la fête est bien annoncée CE jour-là, pas un autre.
            assert!(
                fetes_du_jour(date).iter().any(|f| f.nom == nom),
                "{nom} {annee} doit figurer au {jour_attendu}/{mois} : {:?}",
                fetes_du_jour(date).iter().map(|f| f.nom).collect::<Vec<_>>()
            );
        }
    }

    /// Le premier dimanche du mois, quand le mois COMMENCE un dimanche, est le
    /// 1 — le cas où un `(7 - decalage) % 7` écrit sans le `% 7` donnerait le 8.
    #[test]
    fn le_premier_dimanche_d_un_mois_qui_commence_un_dimanche_est_le_premier() {
        // 2026-03-01 est un dimanche (voir le test du jour de la semaine).
        assert_eq!(jour_de_la_semaine(d(2026, 3, 1)), Some(0), "prémisse");
        assert_eq!(n_ieme_dimanche(2026, 3, 1), Some(d(2026, 3, 1)));
        assert_eq!(n_ieme_dimanche(2026, 3, 2), Some(d(2026, 3, 8)));
    }

    /// **CE QUE LA FENÊTRE AFFICHERA, POUR DE VRAI** — les jours réels, avec
    /// leur contenu exact. C'est le test qui dit si la page aura quelque chose
    /// à montrer, et si elle montrerait deux fois la même chose.
    #[test]
    fn les_fetes_du_jour_disent_ce_qu_il_faut() {
        let noms = |date| fetes_du_jour(date).iter().map(|f| f.nom).collect::<Vec<_>>();

        // Un jour ordinaire : un saint, et c'est tout.
        //
        // ⚠️ **LE 21, PAS LE 23.** Ce test a longtemps figé `23/09` — c'était la
        // date FAUSSE de la première table, recopiée du CSV brut dont les jours
        // n'étaient que la position dans la liste (voir l'en-tête de
        // `fetes_france.tsv`). La Saint-Matthieu est le 21 septembre, et c'est
        // le réalignement du 2026-09-24 qui l'y a remise.
        assert_eq!(noms(d(2026, 9, 21)), vec!["Matthieu"]);

        // Noël : la fête civile ET le saint, SANS doublon — « Noël » ne doit
        // apparaître qu'une fois, celle de la table.
        let noel = noms(d(2026, 12, 25));
        assert_eq!(noel.iter().filter(|n| **n == "Noël").count(), 1, "Noël en double : {noel:?}");
        // ⚠️ **« EMMANUEL », PAS « ADÈLE ».** Le xlsx date les deux sans
        // ambiguïté (« Date(s) directement documentée(s) ») : Adèle au 24/12 et
        // Emmanuel au 25. Le CSV brut les portait au 25 et au 26 — décalés d'un
        // jour, comme tout ce qui suivait une intruse. Voir l'en-tête de
        // `fetes_france.tsv`.
        assert!(noel.contains(&"Emmanuel"), "le saint du 25 décembre : {noel:?}");

        // L'Armistice : la fête de la table ET le saint.
        let armistice = noms(d(2026, 11, 11));
        assert!(armistice.iter().any(|n| n.contains("Armistice")), "{armistice:?}");
        assert!(armistice.contains(&"Martin"), "{armistice:?}");

        // Un férié MOBILE — absent de la table, donc apporté par le CALCUL.
        let ascension = noms(d(2026, 5, 14));
        assert!(ascension.contains(&"Ascension"), "l'Ascension 2026 : {ascension:?}");

        // Et un jour quelconque n'a jamais de doublon de nom, quelle que soit
        // la date : c'est la garantie que la fusion table + calcul fonctionne.
        for mois in 1..=12u32 {
            for jour in 1..=jours_dans_le_mois(2026, mois) {
                let liste = noms(d(2026, mois, jour));
                let uniques: std::collections::HashSet<_> = liste.iter().collect();
                assert_eq!(liste.len(), uniques.len(), "doublon le {jour}/{mois} : {liste:?}");
                assert!(!liste.is_empty(), "rien à afficher le {jour}/{mois}");
            }
        }
    }

    /// **L'IMPORT DE `tools/saints.json` (2026-11-04) A AJOUTÉ SANS RIEN
    /// PERDRE** — c'est la propriété pour laquelle il FUSIONNE au lieu de
    /// remplacer : `rappels_fetes.toml` range les alarmes de l'utilisateur par
    /// triplet (mois, jour, nom), donc un nom retiré est une alarme qui ne
    /// sonnera jamais plus, en silence.
    #[test]
    fn l_import_de_saints_ajoute_sans_rien_perdre() {
        let noms = |date| fetes_du_jour(date).iter().map(|f| f.nom).collect::<Vec<_>>();

        // Ce que le fichier apporte : PLUSIEURS saints par jour, déjà datés.
        let vingt_septembre = noms(d(2026, 9, 20));
        assert!(vingt_septembre.contains(&"André Kim"), "saints importés absents : {vingt_septembre:?}");
        // Ce que la table portait déjà, et qui devait survivre : « Davy » est
        // une ALARME de l'utilisateur (voir `rappels_fetes.toml`), et le
        // fichier ne le connaît pas.
        assert!(vingt_septembre.contains(&"Davy"), "Davy a disparu : {vingt_septembre:?}");

        // **LE DÉDOUBLONNAGE** : le fichier dit « Melaine de Rennes », la table
        // dit « Mélaine » — une seule ligne doit rester. Même contrôle pour
        // « Matthieu » / « Matthieu, apôtre », que la virgule séparait.
        let epiphanie = noms(d(2026, 1, 6));
        let melaine = epiphanie.iter().filter(|n| n.to_lowercase().starts_with("mélaine") || n.to_lowercase().starts_with("melaine")).count();
        assert_eq!(melaine, 1, "doublon de Mélaine : {epiphanie:?}");
        assert_eq!(noms(d(2026, 9, 21)), vec!["Matthieu"], "Matthieu en double");

        // **AUCUN NOM ILLISIBLE N'EST ENTRÉ** : la source porte neuf accents
        // abîmés (« Benoà®t », « Joà«l », « Noà«l » — voir
        // `tools/importer_saints.js`), réparés ou écartés à l'import.
        for mois in 1..=12u32 {
            for jour in 1..=jours_dans_le_mois(2026, mois) {
                for nom in noms(d(2026, mois, jour)) {
                    assert!(
                        !nom.contains('«') && !nom.contains('®') && !nom.contains('©'),
                        "nom illisible le {jour}/{mois} : {nom:?}"
                    );
                }
            }
        }
    }

    // --- LES RAPPELS : CE QUI ALLUME LA CLOCHE ---

    /// Un ANNIVERSAIRE de test — le genre par défaut des tests, sauf ceux qui
    /// portent explicitement sur les fêtes personnalisées.
    fn anniv(nom: &str, jour: u32, mois: u32, rappel: bool) -> config::DatePersonnalisee {
        config::DatePersonnalisee { nom: nom.to_string(), jour, mois, rappel, genre: config::Genre::Anniversaire, annee: None }
    }

    /// **LES DEUX RÉGLAGES SONT INDÉPENDANTS**, et c'est le cœur de la demande :
    /// on choisit ses anniversaires un par un, les fériés d'un seul geste.
    /// Éteindre l'un ne doit pas éteindre l'autre.
    #[test]
    fn les_deux_reglages_de_rappel_sont_independants() {
        let anniversaires = [anniv("Léa", 25, 12, true)];

        // Férié allumé, anniversaire allumé : les deux.
        let avec = rappels_du_jour(d(2026, 12, 25), &anniversaires, &[], true);
        assert_eq!(avec.len(), 2, "{avec:?}");
        assert!(avec.iter().any(|r| r.nom == "Léa" && r.raison == RaisonRappel::Saisie(config::Genre::Anniversaire)));
        assert!(avec.iter().any(|r| r.raison == RaisonRappel::Ferie));

        // Fériés ÉTEINTS : l'anniversaire reste.
        let sans_ferie = rappels_du_jour(d(2026, 12, 25), &anniversaires, &[], false);
        assert_eq!(sans_ferie.len(), 1, "{sans_ferie:?}");
        assert_eq!(sans_ferie[0].raison, RaisonRappel::Saisie(config::Genre::Anniversaire));

        // Anniversaire ÉTEINT, fériés allumés : le férié reste.
        let sans_anniv = rappels_du_jour(d(2026, 12, 25), &[anniv("Léa", 25, 12, false)], &[], true);
        assert_eq!(sans_anniv.len(), 1, "{sans_anniv:?}");
        assert_eq!(sans_anniv[0].raison, RaisonRappel::Ferie, "le 25 décembre est férié");

        // Les deux éteints : RIEN — c'est cette liste vide qui éteint la cloche.
        assert!(rappels_du_jour(d(2026, 12, 25), &[anniv("Léa", 25, 12, false)], &[], false).is_empty());
    }

    /// **UNE FÊTE PERSONNALISÉE SE RAPPELLE COMME UN ANNIVERSAIRE, ET SE DIT
    /// AUTREMENT.** DEMANDE EXPLICITE (2026-09-23) : « ajoute la possibilité de
    /// mettre des dates de fête personnalisée ». Les deux familles suivent les
    /// mêmes règles (date dans l'année, rappel individuel) mais la raison
    /// Voyage AVEC le genre — sans quoi la cloche annoncerait « Anniversaire
    /// de… » pour une fête que personne n'a présentée comme un anniversaire.
    #[test]
    fn une_fete_personnalisee_se_rappelle_et_dit_son_genre() {
        let fetes = [config::DatePersonnalisee {
            nom: "Retour de mission".to_string(),
            jour: 14,
            mois: 7,
            rappel: true,
            genre: config::Genre::Fete,
            annee: None,
        }];
        let r = rappels_du_jour(d(2026, 7, 14), &fetes, &[], false);
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].nom, "Retour de mission");
        assert_eq!(r[0].raison, RaisonRappel::Saisie(config::Genre::Fete), "et son genre, pas celui d'un anniversaire");

        // Sans rappel, elle se tait — comme un anniversaire.
        let muettes = [config::DatePersonnalisee {
            nom: "Retour de mission".to_string(),
            jour: 14,
            mois: 7,
            rappel: false,
            genre: config::Genre::Fete,
            annee: None,
        }];
        assert!(rappels_du_jour(d(2026, 7, 14), &muettes, &[], false).is_empty());
    }

    /// **LES DEUX FAMILLES COHABITENT LE MÊME JOUR** sans se confondre : elles
    /// vivent dans une seule table (voir `config::DatePersonnalisee`), donc
    /// c'est ici que se vérifie que rien ne les mélange.
    #[test]
    fn un_anniversaire_et_une_fete_le_meme_jour_gardent_leur_genre() {
        let liste = [
            config::DatePersonnalisee {
                nom: "Léa".to_string(), jour: 3, mois: 8, rappel: true, genre: config::Genre::Anniversaire,
                annee: None,
            },
            config::DatePersonnalisee {
                nom: "Création de l'atelier".to_string(), jour: 3, mois: 8, rappel: true, genre: config::Genre::Fete,
                annee: None,
            },
        ];
        let r = rappels_du_jour(d(2026, 8, 3), &liste, &[], false);
        assert_eq!(r.len(), 2, "{r:?}");
        assert!(r.iter().any(|x| x.nom == "Léa" && x.raison == RaisonRappel::Saisie(config::Genre::Anniversaire)));
        assert!(r.iter().any(|x| x.nom == "Création de l'atelier" && x.raison == RaisonRappel::Saisie(config::Genre::Fete)));
    }

    // --- LA RECHERCHE ---

    /// Raccourci : cherche dans la table SEULE (les tests de ce bloc portent
    /// sur la table, sauf ceux qui prennent des dates et des rappels).
    fn noms(q: &str) -> Vec<String> {
        chercher(q, &[], &[]).into_iter().map(|r| r.nom).collect()
    }

    /// Ce que la recherche doit trouver, et ce qu'elle ne doit pas.
    #[test]
    fn la_recherche_trouve_par_sous_chaine() {
        assert!(noms("Matthieu").contains(&"Matthieu".to_string()), "le saint du 21 septembre");
        assert!(noms("mat").contains(&"Matthieu".to_string()), "en minuscules aussi");

        // **ET SANS L'ACCENT** : on tape « fete » bien plus souvent que « fête ».
        assert!(!noms("fete").is_empty(), "« fete » doit trouver les « Fête … »");
        assert_eq!(noms("Fete").len(), noms("fête").len(), "accent ou pas, MÊME résultat");
        assert!(noms("noel").contains(&"Noël".to_string()), "« noel » trouve « Noël »");

        // Une requête vide n'est pas une recherche : liste VIDE, jamais tout.
        assert!(chercher("", &[], &[]).is_empty());
        assert!(chercher("   ", &[], &[]).is_empty());

        // Et une chaîne absente ne rend rien — sans quoi « ça ne trouve rien »
        // ne se distinguerait pas de « ça trouve tout ».
        assert!(chercher("zzzzqqqq", &[], &[]).is_empty());
    }

    /// **LES FÊTES À DATE CALCULÉE SONT CHERCHABLES AUSSI** — elles ne sont pas
    /// dans la table, donc c'est ce test qui garantit qu'on ne les a pas
    /// oubliées.
    #[test]
    fn la_recherche_trouve_aussi_les_fetes_a_date_calculee() {
        for (q, attendu) in [("peres", "Fête des Pères"), ("meres", "Fête des Mères"), ("grands", "Fête des Grands-Mères")] {
            let r = chercher(q, &[], &[]);
            let trouve = r.iter().find(|x| x.nom == attendu).unwrap_or_else(|| panic!("« {q} » doit trouver « {attendu} »"));
            // ⚠️ LEUR JOUR VAUT `0` — un SENTINELLE : elles tombent un dimanche
            // compté depuis un bord du mois. La page doit dire « en mai ».
            assert_eq!(trouve.jour, 0, "jour sentinelle pour une date calculée");
            // Et elles ne se rappellent PAS individuellement (voir `Cible`).
            assert_eq!(trouve.cible, Cible::Calculee);
            assert!(!trouve.rappel);
        }
    }

    /// **LA RECHERCHE COUVRE AUSSI LES DATES SAISIES** — DEMANDE EXPLICITE
    /// (2026-09-23) : « ajoute une recherche des anniversaires ». Sans ce test,
    /// on chercherait les fêtes sans jamais trouver ce qu'on a saisi soi-même.
    #[test]
    fn la_recherche_trouve_les_dates_saisies() {
        let dates = [
            config::DatePersonnalisee { nom: "Léa".to_string(), jour: 12, mois: 3, rappel: true, genre: config::Genre::Anniversaire, annee: None },
            config::DatePersonnalisee { nom: "Création de l'atelier".to_string(), jour: 4, mois: 10, rappel: false, genre: config::Genre::Fete, annee: None },
        ];
        let r = chercher("lea", &dates, &[]);
        let trouve = r.iter().find(|x| x.nom == "Léa").expect("« lea » doit trouver « Léa »");
        // La CIBLE porte le RANG dans `dates` : c'est ce que
        // `calendar/rappel/<index>` attend, donc rien à convertir côté page.
        assert_eq!(trouve.cible, Cible::Saisie(0));
        assert!(trouve.rappel, "et l'état de sa cloche");
        assert_eq!(trouve.categorie, Categorie::AnniversaireSaisi, "sa famille, pas celle de la table");

        // Une fête personnalisée se trouve aussi, et se dit pour ce qu'elle est.
        let r2 = chercher("atelier", &dates, &[]);
        assert_eq!(r2.len(), 1);
        assert_eq!(r2[0].cible, Cible::Saisie(1));
        assert_eq!(r2[0].categorie, Categorie::FeteSaisie);
        assert!(!r2[0].rappel);
    }

    /// **L'ANNÉE D'UN ANNIVERSAIRE EST FACULTATIVE, ET NE CHANGE PAS LA DATE.**
    ///
    /// DEMANDE EXPLICITE (2026-09-23) : « pour les dates d'anniversaire j'aimerais
    /// pouvoir ajouter l'année sur quatre chiffres ». Elle sert à connaître
    /// l'ÂGE — la date, elle, continue de revenir chaque année : c'est (jour,
    /// mois) qui déclenche le rappel, jamais l'année.
    #[test]
    fn l_annee_d_un_anniversaire_ne_change_pas_quand_il_revient() {
        let anniv = |annee| config::DatePersonnalisee {
            nom: "Léa".to_string(), jour: 12, mois: 3, rappel: true, genre: config::Genre::Anniversaire, annee,
        };
        // Avec ou sans année, le rappel tombe le MÊME jour, à toutes les années.
        for modele in [anniv(Some(1996)), anniv(None)] {
            for a in [2026, 2030, 2100] {
                let r = rappels_du_jour(d(a, 3, 12), &[modele.clone()], &[], false);
                assert_eq!(r.len(), 1, "année {a}, modèle {modele:?}", );
            }
            assert!(rappels_du_jour(d(2026, 3, 13), &[modele], &[], false).is_empty());
        }
    }

    /// **LES VARIANTES ORTHOGRAPHIQUES : STOCKÉES, CHERCHABLES, PAS AFFICHÉES.**
    ///
    /// DEMANDE EXPLICITE (2026-09-23) : le second fichier fourni s'intitule
    /// « ... exhaustif et variantes ». Il serait absurde de les afficher — le
    /// 24 juin porte DIX-HUIT formes de « Jean-X » (avec et sans trait d'union)
    /// — mais les jeter perdrait la recherche par variante.
    #[test]
    fn les_variantes_sont_cherchables_mais_pas_affichees() {
        // Le 24 juin affiche « Jean-Baptiste » (la référence)… et RIEN de ses
        // dix-sept variantes.
        let jour = fetes_du_jour(d(2026, 6, 24));
        let noms: Vec<&str> = jour.iter().map(|f| f.nom).collect();
        assert!(noms.contains(&"Jean-Baptiste"), "{noms:?}");
        assert!(!noms.iter().any(|n| *n == "Jean Baptiste"), "la variante sans trait d'union ne s'affiche PAS : {noms:?}");
        assert!(jour.iter().all(|f| f.variante_de.is_empty()), "aucune variante à l'affichage");

        // …mais la RECHERCHE la trouve, et dit de quoi elle est la variante.
        let r = chercher("Jean Baptiste", &[], &[]);
        let v = r.iter().find(|x| x.nom == "Jean Baptiste").expect("la variante se cherche");
        assert_eq!(v.variante_de, "Jean-Baptiste", "et elle dit sa forme de référence");
        assert_eq!(v.jour, 24);
        assert_eq!(v.mois, 6);
    }

    /// **L'ÉTAT DE LA CLOCHE D'UNE FÊTE DE LA TABLE** — DEMANDE EXPLICITE :
    /// « dans les résultats [...] mettre un rappel ou le retirer ». Le résultat
    /// doit dire si le rappel est POSÉ, sans quoi la page dessinerait toujours
    /// une cloche vide.
    #[test]
    fn la_recherche_dit_si_une_fete_de_la_table_est_deja_rappelee() {
        let avant = chercher("Matthieu", &[], &[]);
        assert!(!avant[0].rappel, "aucun rappel posé au départ");
        assert_eq!(avant[0].cible, Cible::Table, "une fête de la table s'adresse par sa date ET son nom");

        // La date du rappel doit être CELLE DE LA TABLE — 21/09, pas 23 (voir
        // `les_fetes_du_jour_disent_ce_qu_il_faut`).
        let rappels = [config::RappelFete { nom: "Matthieu".to_string(), jour: 21, mois: 9 }];
        let apres = chercher("Matthieu", &[], &rappels);
        assert!(apres[0].rappel, "et il se voit une fois posé");

        // La clé est le TRIPLET : un rappel posé sur une AUTRE fête du même jour
        // ne doit pas allumer celle-ci.
        let autre = [config::RappelFete { nom: "Un autre".to_string(), jour: 23, mois: 9 }];
        assert!(!chercher("Matthieu", &[], &autre)[0].rappel, "même date, autre nom : pas le même rappel");
        assert!(rappel_fete_actif(&autre, 9, 23, "Un autre"));
        assert!(!rappel_fete_actif(&autre, 9, 23, "Matthieu"));
    }

    /// **UNE FÊTE DE LA TABLE RAPPELÉE REMONTE DANS `rappels_du_jour`** — le
    /// maillon qui relie la case cochée dans une recherche à la cloche de la
    /// barre du haut.
    #[test]
    fn une_fete_de_la_table_rappelee_entre_dans_les_rappels_du_jour() {
        let rappels = [config::RappelFete { nom: "Matthieu".to_string(), jour: 21, mois: 9 }];
        let r = rappels_du_jour(d(2026, 9, 21), &[], &rappels, false);
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].nom, "Matthieu");
        assert_eq!(r[0].raison, RaisonRappel::FeteDeLaTable);

        // Un AUTRE jour, elle se tait — la date compte autant que le nom.
        assert!(rappels_du_jour(d(2026, 9, 24), &[], &rappels, false).is_empty());
    }

    /// Les résultats sortent **TRIÉS PAR DATE**, et la recherche balaie bien
    /// TOUTE l'année — sans plafond (voir la correction laissée dans `chercher`).
    #[test]
    fn la_recherche_couvre_l_annee_et_sort_triee() {
        let r = chercher("a", &[], &[]);
        assert!(r.len() > 60, "une lettre très courante ramène BEAUCOUP d'entrées, et on n'en tronque AUCUNE : {}", r.len());
        let mut precedent = (0u32, 0u32);
        for x in &r {
            assert!((x.mois, x.jour) >= precedent, "{} sort après {precedent:?}", x.nom);
            precedent = (x.mois, x.jour);
        }
        let mois: std::collections::HashSet<u32> = r.iter().map(|x| x.mois).collect();
        assert!(mois.len() > 3, "la recherche couvre toute l'année, pas un mois : {mois:?}");
    }

    /// Les CLÉS de `Genre` sont ce qui relie le fichier, la page et ce module :
    /// les changer casserait les fichiers enregistrés comme l'affichage, sans
    /// qu'aucun compilateur ne le dise. Ce test les fige.
    #[test]
    fn les_cles_de_genre_sont_stables_et_reversibles() {
        for genre in [config::Genre::Anniversaire, config::Genre::Fete] {
            assert_eq!(config::Genre::depuis_cle(genre.cle()), Some(genre), "aller-retour pour {:?}", genre.cle());
        }
        // La casse et les accents comptent : la clé écrite dans le `.toml` est
        // exactement celle-ci.
        assert_eq!(config::Genre::Anniversaire.cle(), "anniversaire");
        assert_eq!(config::Genre::Fete.cle(), "fete");
        assert_eq!(config::Genre::depuis_cle("Fete"), None, "une casse différente n'est PAS reconnue");
        assert_eq!(config::Genre::depuis_cle("fête"), None, "et une clé accentuée non plus");
        assert_eq!(config::Genre::depuis_cle("autre"), None, "un genre inconnu est écarté, jamais deviné");
    }

    /// **L'ANNÉE N'ENTRE PAS EN COMPTE** : un anniversaire revient chaque
    /// année, et c'est bien pour ça que `Anniversaire` ne stocke pas d'année.
    /// Le même enregistrement doit sonner en 2026, en 2030, et le 29 février
    /// d'une année bissextile comme d'aucune autre.
    #[test]
    fn un_anniversaire_revient_chaque_annee() {
        let a = [anniv("Léa", 3, 8, true)];
        for annee in [2026, 2027, 2030, 2100] {
            assert_eq!(rappels_du_jour(d(annee, 8, 3), &a, &[], false).len(), 1, "anniversaire en {annee}");
            assert!(rappels_du_jour(d(annee, 8, 4), &a, &[], false).is_empty(), "pas le lendemain, en {annee}");
        }
        // 29 février : sonne les années bissextiles, et ne sonne pas les autres
        // (aucun 29 février n'existe alors — c'est la date qui manque, pas le
        // rappel qui se tait).
        let bis = [anniv("Née un 29", 29, 2, true)];
        assert_eq!(rappels_du_jour(d(2028, 2, 29), &bis, &[], false).len(), 1, "2028 est bissextile");
        assert!(rappels_du_jour(d(2028, 2, 28), &bis, &[], false).is_empty());
    }

    /// Un jour ordinaire, sans anniversaire : **rien**. C'est la réponse la plus
    /// fréquente, et celle qui doit être la plus sûre — une cloche qui clignote
    /// pour rien ne serait plus regardée.
    #[test]
    fn un_jour_sans_rien_ne_rappelle_rien() {
        assert!(rappels_du_jour(d(2026, 9, 23), &[], &[], true).is_empty(), "le 23 septembre n'est pas férié");
        assert!(rappels_du_jour(d(2026, 6, 15), &[anniv("Léa", 3, 8, true)], &[], true).is_empty());
        // Un anniversaire SANS rappel, un jour ordinaire : rien non plus.
        assert!(rappels_du_jour(d(2026, 6, 15), &[anniv("Léa", 15, 6, false)], &[], false).is_empty());
    }

    /// Les fériés MOBILES rappellent aussi — ils ne sont pas dans la table, donc
    /// c'est le calcul qui les apporte jusqu'à la cloche. Sans ce test, un
    /// rappel d'Ascension pourrait se perdre entre les deux.
    #[test]
    fn les_feries_mobiles_rappellent_aussi() {
        for (date, nom) in [(d(2026, 5, 14), "Ascension"), (d(2026, 4, 6), "Lundi de Pâques"), (d(2026, 5, 25), "Lundi de Pentecôte")] {
            let r = rappels_du_jour(date, &[], &[], true);
            assert!(r.iter().any(|x| x.nom == nom), "{nom} doit rappeler le {date:?} : {r:?}");
        }
    }

    /// Le 29 février existe les années bissextiles, et la règle des siècles est
    /// appliquée : 2000 est bissextile, 1900 et 2100 ne le sont pas. C'est le
    /// genre de règle qu'on croit connaître et qu'on écrit de travers.
    #[test]
    fn la_regle_des_bissextiles_est_entiere() {
        assert_eq!(jours_dans_le_mois(2024, 2), 29);
        assert_eq!(jours_dans_le_mois(2026, 2), 28);
        assert_eq!(jours_dans_le_mois(2000, 2), 29, "2000 est bissextile (multiple de 400)");
        assert_eq!(jours_dans_le_mois(1900, 2), 28, "1900 ne l'est pas (multiple de 100)");
        assert_eq!(jours_dans_le_mois(2100, 2), 28, "2100 non plus");
    }

    // -----------------------------------------------------------------------
    // L'EMPREINTE DES RAPPELS — ce que la cloche retient et ce qui la rallume
    // -----------------------------------------------------------------------

    fn rappel(nom: &str, raison: RaisonRappel) -> Rappel {
        Rappel::du_jour(nom.to_string(), raison)
    }

    #[test]
    fn une_liste_vide_n_a_pas_d_empreinte() {
        // C'est la réponse la plus fréquente, et elle ne doit NI s'acquitter NI
        // clignoter : `None` est ce qui l'exprime, un `Some("")` serait ambigu.
        assert_eq!(empreinte_des_rappels(d(2026, 9, 23), &[]), None);
    }

    #[test]
    fn l_empreinte_change_avec_le_jour() {
        // Le même nom deux jours de suite : la cloche DOIT se rallumer, sinon un
        // anniversaire oublié resterait muet pour toujours.
        let r = [rappel("Léa", RaisonRappel::Saisie(config::Genre::Anniversaire))];
        let a = empreinte_des_rappels(d(2026, 3, 12), &r);
        let b = empreinte_des_rappels(d(2027, 3, 12), &r);
        assert_ne!(a, b, "deux années différentes, deux empreintes");
        assert!(a.is_some() && b.is_some());
    }

    #[test]
    fn l_empreinte_change_quand_un_rappel_s_ajoute() {
        // **C'EST LA DEMANDE EXPLICITE DE L'UTILISATEUR** : cliquer acquitte ce
        // qui était AFFICHÉ. Un rappel ajouté ensuite doit rallumer la cloche.
        let un = [rappel("Léa", RaisonRappel::Saisie(config::Genre::Anniversaire))];
        let deux = [
            rappel("Léa", RaisonRappel::Saisie(config::Genre::Anniversaire)),
            rappel("Noël", RaisonRappel::Ferie),
        ];
        let date = d(2026, 12, 25);
        assert_ne!(empreinte_des_rappels(date, &un), empreinte_des_rappels(date, &deux));
    }

    #[test]
    fn l_empreinte_ne_depend_pas_de_l_ordre() {
        // Deux rappels du même jour peuvent sortir dans n'importe quel ordre
        // selon la famille qui les produit : l'empreinte les TRIE, donc un
        // réordonnancement ne rallume pas la cloche pour rien.
        let a = [rappel("Léa", RaisonRappel::Saisie(config::Genre::Anniversaire)), rappel("Noël", RaisonRappel::Ferie)];
        let b = [rappel("Noël", RaisonRappel::Ferie), rappel("Léa", RaisonRappel::Saisie(config::Genre::Anniversaire))];
        assert_eq!(empreinte_des_rappels(d(2026, 12, 25), &a), empreinte_des_rappels(d(2026, 12, 25), &b));
    }

    #[test]
    fn le_genre_distingue_deux_dates_du_meme_nom() {
        // Un anniversaire et une fête personnalisée qui portent le même nom le
        // même jour sont DEUX rappels : l'empreinte doit les distinguer, sinon en
        // supprimer un ne rallumerait pas la cloche.
        let anniv = [rappel("Fête", RaisonRappel::Saisie(config::Genre::Anniversaire))];
        let fete = [rappel("Fête", RaisonRappel::Saisie(config::Genre::Fete))];
        let date = d(2026, 7, 14);
        assert_ne!(empreinte_des_rappels(date, &anniv), empreinte_des_rappels(date, &fete));
    }

    /// ⛔ **LE DÉFAUT SIGNALÉ LE 2026-09-24** : « j'ai mis aujourd'hui et 13h16
    /// mais l'alarme ne s'est pas déclenchée, de plus la cloche apparaît en
    /// permanence près de l'horloge ». Un seul défaut, deux symptômes — l'identité
    /// d'un rappel de note se réduisait au NOM de la note, donc changer l'heure
    /// d'une alarme DÉJÀ ACQUITTÉE le même jour ne changeait pas l'empreinte : la
    /// cloche restait visible (`rappels_du_jour` non vide) sans clignoter
    /// (`cloche_clignote` faux), et le son ne part que si elle clignote.
    #[test]
    fn changer_l_heure_d_une_alarme_de_note_rallume_la_cloche() {
        let alarme = |precision: &str| [Rappel {
            nom: "notes".to_string(),
            raison: RaisonRappel::Note,
            precision: Some(precision.to_string()),
        }];
        let date = d(2026, 9, 24);
        assert_ne!(
            empreinte_des_rappels(date, &alarme("2026-09-24T12:00")),
            empreinte_des_rappels(date, &alarme("2026-09-24T13:16")),
            "une autre heure, c'est un autre rappel"
        );
    }

    /// Le pendant, et il compte autant : **la DATE de l'alarme est dans
    /// l'identité**, pas seulement l'heure. Une alarme passée est « en retard,
    /// donc à signaler » (voir `AlarmeNote::sonne_a`), une alarme du jour ne
    /// l'est pas — ce sont deux rappels, même sans heure.
    #[test]
    fn changer_la_date_d_une_alarme_de_note_rallume_la_cloche() {
        let alarme = |precision: &str| [Rappel {
            nom: "notes".to_string(),
            raison: RaisonRappel::Note,
            precision: Some(precision.to_string()),
        }];
        let date = d(2026, 9, 24);
        assert_ne!(
            empreinte_des_rappels(date, &alarme("2026-01-09T*")),
            empreinte_des_rappels(date, &alarme("2026-09-24T*"))
        );
    }

    /// **ET LES RAPPELS D'UN JOUR ENTIER GARDENT EXACTEMENT L'EMPREINTE D'AVANT** :
    /// ce qui a été acquitté avant ce correctif reste acquitté pour eux (fêtes,
    /// dates saisies, fériés). Sans ça, la cloche se rallumerait une fois pour
    /// tout le monde sans qu'aucun rappel ait bougé — et `rappel_ack.txt` est un
    /// fichier PERSISTÉ, donc la question se pose pour de vrai.
    #[test]
    fn un_rappel_de_jour_entier_garde_son_empreinte() {
        let date = d(2026, 9, 24);
        assert_eq!(
            empreinte_des_rappels(date, &[rappel("Thècle", RaisonRappel::FeteDeLaTable)]),
            Some("2026 09 24|fete-table:Thècle".to_string())
        );
    }

    /// `quand_d_alarme` — la forme STABLE qui entre dans l'empreinte : la date et
    /// l'heure, chaque moitié absente marquée `*`. **PURE**, donc vérifiable sans
    /// cloche et sans clic, ce que ce dépôt ne fait jamais sur un poste réel.
    #[test]
    fn quand_d_alarme_rend_la_date_et_l_heure_ou_une_etoile() {
        let rien = notes::AlarmeNote { date: None, heure: None, jours: 0 };
        let jour = notes::AlarmeNote { date: Some((2026, 9, 24)), heure: None, jours: 0 };
        let instant = notes::AlarmeNote { date: Some((2026, 9, 24)), heure: Some((13, 16)), jours: 0 };
        let repetee = notes::AlarmeNote { date: None, heure: Some((8, 0)), jours: 31 };
        assert_eq!(quand_d_alarme(&rien), "*T*");
        assert_eq!(quand_d_alarme(&repetee), "*T08:00R31", "la répétition change l'identité");
        assert_eq!(quand_d_alarme(&jour), "2026-09-24T*");
        assert_eq!(quand_d_alarme(&instant), "2026-09-24T13:16");
        assert_eq!(quand_d_alarme(&instant), quand_d_alarme(&instant.clone()), "stable");
    }

    // -----------------------------------------------------------------------
    // LES ÉVÈNEMENTS D'UN MOIS — ce que le mini calendrier de l'horloge affiche
    // -----------------------------------------------------------------------

    #[test]
    fn un_mois_ordinaire_rend_les_bonnes_fetes_et_rien_d_autre() {
        // Février 2026 : aucun férié, mais des saints. On vérifie la FORME et le
        // fait qu'aucun jour vide n'est rendu.
        let mois = evenements_du_mois(2026, 2, &[], &[]);
        assert!(!mois.is_empty(), "fevrier 2026 doit porter des fetes");
        assert!(mois.iter().all(|(_, evs)| !evs.is_empty()), "aucun jour vide");
        assert!(mois.iter().all(|(jour, _)| (1..=28).contains(jour)), "aucun 29, 30 ni 31 en fevrier 2026");
        assert!(mois.windows(2).all(|p| p[0].0 < p[1].0), "les jours sont tries");
    }

    #[test]
    fn le_premier_janvier_est_ferie_dans_le_mois() {
        let janvier = evenements_du_mois(2026, 1, &[], &[]);
        let (_, evs) = janvier.iter().find(|(j, _)| *j == 1).expect("le 1er janvier");
        assert!(evs.iter().any(|e| e.categorie == Categorie::Ferie), "{evs:?}");
    }

    #[test]
    fn paques_et_ses_lendemains_tombent_dans_le_bon_mois() {
        // Pâques 2026 = 5 avril, lundi de Pâques = 6. Le férié MOBILE doit donc
        // apparaître en avril — c'est ce que `fetes_du_jour` calcule, et ce qu'un
        // simple balayage de la table ne donnerait pas.
        let avril = evenements_du_mois(2026, 4, &[], &[]);
        let (_, lundi) = avril.iter().find(|(j, _)| *j == 6).expect("le 6 avril");
        assert!(lundi.iter().any(|e| e.categorie == Categorie::Ferie), "{lundi:?}");
    }

    #[test]
    fn une_date_saisie_rejoint_son_jour_avec_son_genre() {
        let dates = [
            anniv("Lea", 12, 3, true),
            config::DatePersonnalisee {
                nom: "Retour de mission".to_string(),
                jour: 12,
                mois: 3,
                rappel: false,
                genre: config::Genre::Fete,
                annee: None,
            },
        ];
        let mars = evenements_du_mois(2026, 3, &dates, &[]);
        let (_, evs) = mars.iter().find(|(j, _)| *j == 12).expect("le 12 mars");
        let lea = evs.iter().find(|e| e.nom == "Lea").expect("Lea");
        assert_eq!(lea.categorie, Categorie::AnniversaireSaisi, "et pas une categorie de la table");
        let fete = evs.iter().find(|e| e.nom == "Retour de mission").expect("la fete");
        assert_eq!(fete.categorie, Categorie::FeteSaisie);
    }

    /// **LE DRAPEAU D'ALARME COUVRE LES DEUX SORTES DE FÊTES** — c'est lui qui fait
    /// ressortir les lettres dans le mini calendrier (« fais ressortir les
    /// éléments pour lesquels une alarme est activée »).
    ///
    /// Les dates SAISIES le portent en propre ; les fêtes de la **TABLE**
    /// l'empruntent à `rappels_fetes.toml`, par le triplet `(mois, jour, nom)`.
    /// ⚠️ Ce second cas a d'abord été écrit `false`, en lisant « fête ou
    /// anniversaire » comme « les deux familles SAISIES ». **C'était faux, et
    /// l'utilisateur l'a vu** : « ça ne fonctionne pas pour les fêtes » — ses
    /// douze alarmes portent presque toutes sur des saints. Les deux surfaces
    /// disent maintenant la même chose.
    #[test]
    fn le_drapeau_d_alarme_couvre_les_deux_sortes_de_fetes() {
        // 1. Une date SAISIE : le drapeau vient d'elle.
        let dates = [anniv("Lea", 12, 3, true), anniv("Theo", 12, 3, false)];
        let mars = evenements_du_mois(2026, 3, &dates, &[]);
        let (_, evs) = mars.iter().find(|(j, _)| *j == 12).expect("le 12 mars");
        assert!(evs.iter().find(|e| e.nom == "Lea").expect("Lea").rappel, "alarme posée");
        assert!(!evs.iter().find(|e| e.nom == "Theo").expect("Theo").rappel, "alarme éteinte");

        // 2. Une fête de la TABLE : le drapeau vient de `rappels_fetes.toml`.
        let rappels = [config::RappelFete { nom: "Matthieu".to_string(), jour: 21, mois: 9 }];
        let septembre = evenements_du_mois(2026, 9, &[], &rappels);
        let (_, evs) = septembre.iter().find(|(j, _)| *j == 21).expect("le 21 septembre");
        assert!(
            evs.iter().find(|e| e.nom == "Matthieu").expect("Matthieu").rappel,
            "une fête de la table SUIVIE doit porter son alarme"
        );
        // …et sans le rappel, elle ne la porte pas — c'est ce qui rend le test
        // capable d'échouer si la fonction retournait `true` en dur.
        let sans = evenements_du_mois(2026, 9, &[], &[]);
        let (_, evs) = sans.iter().find(|(j, _)| *j == 21).expect("le 21 septembre");
        assert!(!evs.iter().find(|e| e.nom == "Matthieu").expect("Matthieu").rappel);
    }

    #[test]
    fn l_annee_d_une_date_saisie_est_transportee() {
        // Elle ne sert qu'à dire l'âge, mais elle doit survivre au voyage — sinon
        // le panneau ne pourrait jamais l'afficher.
        let mut d = anniv("Lea", 12, 3, true);
        d.annee = Some(1990);
        let mars = evenements_du_mois(2026, 3, &[d], &[]);
        let (_, evs) = mars.iter().find(|(j, _)| *j == 12).expect("le 12 mars");
        assert_eq!(evs.iter().find(|e| e.nom == "Lea").expect("Lea").annee, Some(1990));
    }

    #[test]
    fn un_mois_sans_rien_rend_une_liste_vide() {
        // Le mois de février 2026 a des saints, donc on ne peut pas le prendre.
        // On vérifie plutôt qu'une date saisie HORS du mois demandé n'y apparaît
        // pas — c'est le vrai risque d'un filtrage approximatif.
        let dates = [anniv("Lea", 12, 3, true)];
        let fevrier = evenements_du_mois(2026, 2, &dates, &[]);
        assert!(
            fevrier.iter().all(|(_, evs)| evs.iter().all(|e| e.nom != "Lea")),
            "une date de mars ne doit pas apparaitre en fevrier"
        );
    }

    #[test]
    fn une_date_impossible_pour_son_mois_est_perdue_en_le_sachant() {
        // ⚠️ CE TEST FIGE UNE PERTE ASSUMÉE, il ne la recommande pas : le 31 février
        // est ACCEPTÉ par la saisie (borne large) et n'a aucune cellule où
        // s'afficher en février. On vérifie qu'il ne se replie PAS sur le 28.
        let dates = [anniv("Impossible", 31, 2, true)];
        let fevrier = evenements_du_mois(2026, 2, &dates, &[]);
        assert!(
            fevrier.iter().all(|(_, evs)| evs.iter().all(|e| e.nom != "Impossible")),
            "un 31 fevrier ne doit pas etre replie sur un autre jour"
        );
        // Et le 28 n'a pas hérité de lui.
        let (_, le28) = fevrier.iter().find(|(j, _)| *j == 28).expect("le 28 fevrier");
        assert!(le28.iter().all(|e| e.nom != "Impossible"), "{le28:?}");
    }

    #[test]
    fn la_cloche_ne_clignote_pas_pour_une_empreinte_acquittee() {
        // Les cellules sont `thread_local` : chaque test a la sienne, aucun
        // risque d'interférence entre tests parallèles.
        let date = d(2026, 12, 25);
        let dates = [anniv("Léa", 25, 12, true)];

        // Rien à signaler : jamais.
        assert!(!cloche_doit_clignoter(date, &[], &[], false), "aucun rappel ne fait pas clignoter");
        // Un rappel, non acquitté : oui.
        assert!(cloche_doit_clignoter(date, &dates, &[], false));
        // Acquitté : non.
        let empreinte = empreinte_des_rappels(date, &rappels_du_jour(date, &dates, &[], false)).expect("non vide");
        set_rappels_acquittes(&empreinte);
        assert!(!cloche_doit_clignoter(date, &dates, &[], false), "acquittée, elle se tait");
        // **LE MÊME JOUR, UN RAPPEL DE PLUS : elle se rallume.**
        let plus = [anniv("Léa", 25, 12, true), anniv("Noël", 25, 12, true)];
        assert!(cloche_doit_clignoter(date, &plus, &[], false), "un rappel ajouté après l'acquittement la rallume");
        // Et le lendemain aussi, avec la même liste.
        assert!(cloche_doit_clignoter(d(2027, 12, 25), &dates, &[], false), "un autre jour rouvre le rappel");
    }

    // -----------------------------------------------------------------------
    // L'ICÔNE DE NOTE — un acquittement PAR NOTE, pas une empreinte du jour
    // -----------------------------------------------------------------------

    fn note_alarmee(id: u64, texte: &str, date: Option<(i32, u32, u32)>, heure: Option<(u32, u32)>) -> notes::Note {
        let mut note = notes::Note::new(id, texte.to_string());
        note.alarme = Some(notes::AlarmeNote { date, heure, jours: 0 });
        note
    }

    /// Une note archivée se tait et quitte le calendrier, sa voisine non.
    #[test]
    fn une_note_archivee_ne_sonne_plus_et_quitte_le_calendrier() {
        let mut archivee = note_alarmee(1, "rangée", Some((2026, 9, 14)), None);
        archivee.archived = true;
        let active = note_alarmee(2, "active", Some((2026, 9, 14)), None);
        let notes_du_jour = [archivee, active];
        let rappels = rappels_des_notes((2026, 9, 14), (12, 0), &notes_du_jour);
        assert_eq!(rappels.iter().map(|r| r.nom.as_str()).collect::<Vec<_>>(), ["active"]);
        assert_eq!(alarmes_de_notes_du_mois(2026, 9, &notes_du_jour).len(), 1);
    }

    /// Une alarme répétée se range sous chacun de ses jours dans le mois — et
    /// pas avant sa date de début.
    #[test]
    fn une_alarme_repetee_apparait_sous_chacun_de_ses_jours() {
        let mut note = notes::Note::new(1, "sport".to_string());
        // Le lundi seulement, à partir du 14 septembre 2026.
        note.alarme = Some(notes::AlarmeNote { date: Some((2026, 9, 14)), heure: None, jours: 1 });
        let jours: Vec<u32> = alarmes_de_notes_du_mois(2026, 9, &[note]).into_iter().map(|(j, _)| j).collect();
        assert_eq!(jours, vec![14, 21, 28], "le 7 est avant le début");
    }

    /// ⛔ **« UN BOUTON PAR NOTE » — donc acquitter l'une NE DOIT PAS faire taire
    /// l'autre.** C'est toute la différence avec la cloche, qui acquitte en un
    /// geste et se contente donc d'une empreinte unique.
    #[test]
    fn acquitter_une_note_ne_fait_pas_taire_sa_voisine() {
        let jour = (2026, 9, 24);
        let notes_du_jour = [
            note_alarmee(1, "premiere", Some(jour), Some((13, 16))),
            note_alarmee(2, "deuxieme", Some(jour), Some((14, 0))),
        ];
        let rappels = rappels_des_notes(jour, (15, 0), &notes_du_jour);
        assert_eq!(rappels.len(), 2, "les deux sont dues à 15 h");

        let identite = identite_du_rappel_de_note(&notes_du_jour[0]).expect("elle a une alarme");
        let stocke = acquitter_rappel_note("", jour, &identite);

        assert!(!note_doit_clignoter(jour, std::slice::from_ref(&rappels[0]), &stocke), "la premiere est acquittee");
        assert!(note_doit_clignoter(jour, &rappels, &stocke), "la deuxieme sonne encore, donc l'icone clignote");
    }

    /// **LE GESTE DE L'ICÔNE DE LA BARRE DU HAUT — TOUTE LA FAMILLE D'UN COUP**,
    /// comme la cloche pour la sienne. Ajouté APRÈS USAGE, sur la demande de
    /// l'utilisateur : « le clic sur la note à côté de l'horloge ne coupe pas la
    /// notification ».
    ///
    /// ⚠️ **ET LES RAPPELS RESTENT POSÉS** : le geste tait la JOURNÉE, il ne retire
    /// rien. C'est la propriété qui le rend sûr — un clic ne peut pas faire perdre
    /// une alarme.
    #[test]
    fn le_geste_de_l_icone_tait_toute_la_famille_pour_la_journee() {
        let jour = (2026, 9, 24);
        let notes_du_jour = [
            note_alarmee(1, "premiere", Some(jour), Some((13, 16))),
            note_alarmee(2, "deuxieme", Some(jour), Some((14, 0))),
        ];
        let rappels = rappels_des_notes(jour, (15, 0), &notes_du_jour);
        assert_eq!(rappels.len(), 2, "les deux sont dues");

        let stocke = acquitter_notes_du_jour("", jour, &rappels);
        assert!(!note_doit_clignoter(jour, &rappels, &stocke), "l'icone les tait toutes");

        // Le LENDEMAIN, les deux reviennent : rien n'a été retiré.
        let demain = (2026, 9, 25);
        let rappels_demain = rappels_des_notes(demain, (15, 0), &notes_du_jour);
        assert_eq!(rappels_demain.len(), 2, "les deux rappels sont toujours poses");
        assert!(note_doit_clignoter(demain, &rappels_demain, &stocke), "et ils reviennent");
    }

    /// **ET LE LENDEMAIN, TOUT REVIENT** — comme la cloche : l'ensemble porte le
    /// jour en tête, donc un autre jour repart à zéro sans que rien ne soit
    /// effacé, et le rappel est de nouveau annoncé.
    #[test]
    fn un_nouveau_jour_repart_des_acquittements_a_zero() {
        let jour = (2026, 9, 24);
        let notes_du_jour = [note_alarmee(1, "note", Some(jour), Some((13, 16)))];
        let identite = identite_du_rappel_de_note(&notes_du_jour[0]).expect("elle a une alarme");
        let stocke = acquitter_rappel_note("", jour, &identite);
        assert!(!note_doit_clignoter(jour, &rappels_des_notes(jour, (15, 0), &notes_du_jour), &stocke), "acquittee aujourd'hui");

        let demain = (2026, 9, 25);
        let rappels_demain = rappels_des_notes(demain, (15, 0), &notes_du_jour);
        assert!(!rappels_demain.is_empty(), "une date passee est due le lendemain aussi");
        assert!(note_doit_clignoter(demain, &rappels_demain, &stocke), "le rappel est revenu");
    }

    /// Rien à signaler ⇒ pas de clignotement, **sans même regarder les
    /// acquittements** — c'est la réponse la plus fréquente, et c'est elle qui
    /// fait disparaître l'icône.
    ///
    /// ⚠️ Et une alarme qui n'est pas encore DUE ne compte pas : la liste du jour
    /// est vide avant son heure (voir `AlarmeNote::sonne_a`), donc l'icône
    /// n'apparaît qu'à l'heure dite. C'est ce que la mesure du 2026-09-24 avait
    /// montré dans l'autre sens : à 13 h 15, une alarme de 13 h 16 n'est nulle
    /// part.
    #[test]
    fn l_icone_de_note_ne_clignote_pas_sans_rappel_du() {
        let jour = (2026, 9, 24);
        let notes_du_jour = [note_alarmee(1, "note", Some(jour), Some((13, 16)))];
        assert!(!note_doit_clignoter(jour, &[], ""), "aucun rappel : rien a signaler");
        assert!(rappels_des_notes(jour, (13, 0), &notes_du_jour).is_empty(), "13 h 16 n'est pas due a 13 h 00");
        assert_eq!(rappels_des_notes(jour, (13, 16), &notes_du_jour).len(), 1, "a 13 h 16, si");
    }

    /// L'identité sert **aux deux bouts** : c'est elle qui entre dans la liste du
    /// jour et elle qui est acquittée. Si les deux divergeaient, acquitter une
    /// note ne la ferait pas taire — ou ferait taire sa voisine.
    #[test]
    fn l_identite_d_une_note_est_celle_de_la_liste_du_jour() {
        let jour = (2026, 9, 24);
        let notes_du_jour = [note_alarmee(1, "Ma note", Some(jour), Some((13, 16)))];
        let rappels = rappels_des_notes(jour, (15, 0), &notes_du_jour);
        assert_eq!(
            identite_du_rappel_de_note(&notes_du_jour[0]).as_deref(),
            Some(rappels[0].identite().as_str())
        );
        assert_eq!(rappels[0].identite(), "note:Ma note@2026-09-24T13:16");
        // Et sans alarme, il n'y a rien à acquitter.
        assert_eq!(identite_du_rappel_de_note(&notes::Note::new(9, "sans alarme".to_string())), None);
    }
}
