//! **LES ZONES DU DOCK, DÉSIGNÉES PAR UNE CLÉ STABLE.**
//!
//! Une zone était désignée par **l'index brut de son séparateur** dans `Dock::icons`
//! (`zone_names.txt` contenait `137,Réglages`). Or les index sont POSITIONNELS :
//! `PINNED_ICON_BASE` vaut `3` (`app::PINNED_ICON_BASE`) et les épingles occupent `3..3+n`,
//! donc **ajouter une épingle insère une place au MILIEU de la plage des épingles et décale
//! de +1 tout ce qui suit**. Six fichiers étaient indexés par ce même espace et devaient donc
//! être réécrits en bloc (`config::decaler_les_zones`) — un décalage qui a détruit la
//! configuration de l'utilisateur, et un ensemble de fichiers dont l'énumération a été
//! incomplète deux fois (cinq fichiers de zone, puis `dash_add_zone`).
//!
//! La clé est maintenant **une lettre de barre et un RANG parmi les espaces de cette barre**
//! (`D3` = troisième espace de la barre de droite). Ajouter une épingle ne change plus aucune
//! clé : l'épingle s'insère SOUS les espaces, donc les rangs sont intacts. C'est ce qui rend
//! `decaler_les_zones` inutile, et avec lui toute la classe de défaut qu'il portait.
//!
//! # Ce que le rang compte, et la limite qu'on accepte
//!
//! ⚠️ **LE RANG COMPTE TOUS LES ESPACES DE LA BARRE, séparateurs compris.** C'est
//! nécessaire et non arbitraire : « qu'est-ce qu'un séparateur de zone » est DÉFINI par la
//! cote (`Dock::is_zone_separator` ≡ `zone_height > 0`), donc compter les seuls séparateurs
//! serait **circulaire** — il faudrait déjà savoir lesquels sont des zones pour savoir
//! lesquels sont des zones.
//!
//! **La limite qui en découle, et qui est assumée** : insérer ou déplacer un espace
//! ORDINAIRE avant une zone décale le rang de cette zone, donc ses réglages la suivent sur
//! l'espace voisin. Le rang est une POSITION, pas une identité. Il est stable là où ça
//! compte — l'ajout d'une épingle, le défaut signalé — et pas ailleurs. Une identité par
//! espace serait le correctif complet ; elle demanderait un état persisté de plus, et c'est
//! un chantier à part.
//!
//! # Le piège d'ordre, et pourquoi `spacer_index_at_rank` prend la séquence en paramètre
//!
//! Au démarrage, `app::run` appelle `ensure_saved_spacers_exist` → `set_icon_order` →
//! `set_spacer_sizes` → les réglages de zone. **Avant `set_icon_order`, `Dock::order(side)`
//! porte encore les listes PAR DÉFAUT de `Dock::new`**, et **avant que la cote soit
//! appliquée, `is_zone_separator` est faux pour TOUS les espaces** — c'est le piège que la
//! doc d'`app::run` décrit (« la requête naturelle `is_zone_separator(i)` est inutilisable
//! ici »). La résolution ne consulte donc NI `order(side)` NI la cote : elle lit la
//! séquence qu'on lui PASSE (en pratique les quatre listes locales sorties de
//! `config::load_icon_order`, qui sont exactement celles que `set_icon_order` rendra
//! vivantes une ligne plus bas) et le prédicat `is_spacer` de `Dock::icons`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::dock::{Side, DEFAULT_ZONE_COLUMNS, SPACER_HEIGHT};

/// Version du format que ce binaire ÉCRIT.
///
/// Sert à refuser d'écraser un fichier écrit par une version PLUS RÉCENTE (voir
/// `config::zones_from_table`) : un binaire ancien qui réécrirait un format qu'il ne connaît
/// pas détruirait les champs qu'il ignore. La montée de version d'un fichier plus ancien se
/// fait sur place, sans branche dédiée — un champ absent prend simplement son défaut.
///
/// **3 = LES ZONES PORTENT LA LISTE DE LEURS ICÔNES** (`ZoneRecord::icones`, étapes 2 à 4 du
/// plan `docs/plans/disposition-barres-zones-icones.md`). Un fichier `version = 2` est lu
/// tel quel, puis **complété une fois** par la répartition (`repartir_les_icones`) : c'est
/// la montée de version, et elle n'a pas de branche à elle.
pub const ZONES_VERSION: i64 = 3;

// --- La barre -------------------------------------------------------------------------

/// **LA BARRE où vit un espace** — `G` gauche, `D` droite, `H` haut, `B` bas.
///
/// ⚠️ **`Haut` couvre les DEUX moitiés de la barre du haut** (`Side::TopLeft` ET
/// `Side::TopRight`) : c'est une seule barre vue de l'utilisateur, et `Dock::bar_rank` les
/// réunit déjà de la même façon. `halves()` fixe l'ORDRE dans lequel le rang les compte —
/// gauche puis droite —, ce qui est ce qui donne un sens à `H1`.
///
/// ⚠️ **`Bas` est en FIN**, comme partout ailleurs dans le dépôt : l'ordre de
/// déclaration fixe celui de `ALL` (et donc l'écriture de `zones.toml`), et une
/// barre ajoutée en fin ne réécrit aucune clé existante.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum BarLetter {
    Gauche,
    Droite,
    Haut,
    /// La barre du BAS — **écran principal seulement** (voir `Side::Bottom`).
    Bas,
}

impl BarLetter {
    /// Les quatre barres, dans l'ordre où elles sont écrites (et où `ZoneKey` se trie).
    pub const ALL: [BarLetter; 4] =
        [BarLetter::Gauche, BarLetter::Droite, BarLetter::Haut, BarLetter::Bas];

    /// La lettre écrite dans `zones.toml`.
    pub fn as_char(self) -> char {
        match self {
            BarLetter::Gauche => 'G',
            BarLetter::Droite => 'D',
            BarLetter::Haut => 'H',
            BarLetter::Bas => 'B',
        }
    }

    /// Relit une lettre — insensible à la casse, comme le reste du dépôt.
    pub fn from_char(letter: char) -> Option<Self> {
        match letter.to_ascii_uppercase() {
            'G' => Some(BarLetter::Gauche),
            'D' => Some(BarLetter::Droite),
            'H' => Some(BarLetter::Haut),
            'B' => Some(BarLetter::Bas),
            _ => None,
        }
    }

    /// **Les moitiés de la barre, DANS L'ORDRE OÙ LE RANG LES COMPTE.** La barre du haut en
    /// a deux ; les autres, une seule.
    pub fn halves(self) -> &'static [Side] {
        match self {
            BarLetter::Gauche => &[Side::Left],
            BarLetter::Droite => &[Side::Right],
            BarLetter::Haut => &[Side::TopLeft, Side::TopRight],
            BarLetter::Bas => &[Side::Bottom],
        }
    }

    /// **La moitié où un espace NEUF s'ANNEXE.**
    ///
    /// ⚠️ **L'ASYMÉTRIE DE LA BARRE DU HAUT EST VOULUE** : on annexe à `TopRight`, la
    /// DERNIÈRE moitié, jamais à `TopLeft`. Annexer à gauche insérerait les espaces neufs
    /// DEVANT tous ceux de droite et décalerait tous leurs rangs — c'est-à-dire exactement
    /// la classe de défaut que ces clés existent pour supprimer.
    pub fn append_side(self) -> Side {
        // `halves()` n'est jamais vide — chaque barre a au moins une moitié.
        *self.halves().last().expect("une barre a toujours au moins une moitie")
    }

    /// La barre à laquelle appartient une moitié.
    #[cfg(test)]
    pub fn of_side(side: Side) -> Self {
        match side {
            Side::Left => BarLetter::Gauche,
            Side::Right => BarLetter::Droite,
            Side::Bottom => BarLetter::Bas,
            Side::TopLeft | Side::TopRight | Side::TopMiddle => BarLetter::Haut,
        }
    }
}

// --- La clé ---------------------------------------------------------------------------

/// **LA CLÉ D'UNE ZONE** — une barre et un rang (1-based) parmi les ESPACES de cette barre.
/// S'affiche `D3`, se relit `d3`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct ZoneKey {
    pub bar: BarLetter,
    /// ⚠️ **À PARTIR DE 1**, comme tout rang montré à l'utilisateur. `0` n'existe pas :
    /// `new` le refuse plutôt que de laisser fabriquer une clé qui ne se relira jamais.
    pub rank: usize,
}

impl ZoneKey {
    /// La clé, ou `None` si le rang est nul (voir `rank`, doc du champ).
    pub fn new(bar: BarLetter, rank: usize) -> Option<Self> {
        (rank >= 1).then_some(Self { bar, rank })
    }

    /// Relit `D3` / `d3`. **`None` pour tout le reste** — `D0`, `D`, `3D`, `D1x`, vide — sans
    /// jamais paniquer : ces chaînes viennent d'un fichier que l'utilisateur peut éditer.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let mut letters = text.chars();
        let bar = BarLetter::from_char(letters.next()?)?;
        let rank: usize = letters.as_str().parse().ok()?;
        Self::new(bar, rank)
    }
}

impl fmt::Display for ZoneKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.bar.as_char(), self.rank)
    }
}

// --- La sorte d'un espace ---------------------------------------------------------------

/// **CE QU'UN ESPACE DESSINE — ET, POUR L'ÉLASTIQUE, CE QU'IL ABSORBE.**
///
/// ⚠️ **`Rond` EST LA VALEUR PAR DÉFAUT, ET ELLE NE S'ÉCRIT JAMAIS** : un
/// `zones.toml` sans `sorte` décrit un espace ordinaire, exactement comme tous
/// ceux d'avant cette fonctionnalité. C'est ce qui rend le fichier
/// **rétrocompatible sans migration** — même règle que `locked` ou `permanent`,
/// dont un défaut n'est jamais écrit (voir `config::zone_record_to_table`).
///
/// ⚠️ **La sorte est une propriété de L'ESPACE, pas un fait d'arrangement** :
/// elle vit donc dans `zones.toml`, à côté de la cote et du verrou, et **non**
/// dans `dossiers.toml` — qui, lui, décrit l'ORDRE de la barre du bas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SorteEspace {
    /// Le marqueur rond, comme partout ailleurs — la seule sorte que les quatre
    /// barres verticales connaissent aujourd'hui.
    #[default]
    Rond,
    /// **UN TRAIT PERMANENT, PERPENDICULAIRE À SA BARRE** — vertical sur la barre
    /// du bas, horizontal sur une barre latérale. C'est ce que le trait de
    /// SURVOL ne sait pas faire (voir `spacer_hover_line_rect`, qui rend `(0,0)`
    /// pour la barre du bas et le dit en commentaire) : ici le trait est
    /// PERMANENT, pas un retour au survol.
    Trait,
    /// **ABSORBE TOUTE LA LARGEUR QUI RESTE**, répartie à parts égales s'ils sont
    /// plusieurs. Ne dessine RIEN : un rond au milieu d'un grand blanc se lirait
    /// mal. **N'a de sens que sur la barre du bas** — c'est la seule dont la
    /// largeur est connue et où « le reste » existe (voir `layout_bottom_side`).
    Elastique,
}

impl SorteEspace {
    /// La clé écrite dans `zones.toml`, et relue par `depuis_cle`.
    pub fn cle(self) -> &'static str {
        match self {
            SorteEspace::Rond => "rond",
            SorteEspace::Trait => "trait",
            SorteEspace::Elastique => "elastique",
        }
    }

    /// **`None` POUR UNE CLÉ INCONNUE — JAMAIS UN DÉFAUT.** C'est la règle du
    /// dépôt (voir `Genre` dans `dates.toml`) : on ne range pas d'office dans une
    /// famille ce qu'on ne sait pas classer. Un `sorte = "trait-vertical"` écrit à
    /// la main doit être REFUSÉ en le disant, pas silencieusement ramené à `Rond`.
    pub fn depuis_cle(cle: &str) -> Option<Self> {
        match cle {
            "rond" => Some(SorteEspace::Rond),
            "trait" => Some(SorteEspace::Trait),
            "elastique" => Some(SorteEspace::Elastique),
            _ => None,
        }
    }

}

// --- Les séquences d'ordre ------------------------------------------------------------

/// **LES QUATRE LISTES D'ORDRE, QUELLE QUE SOIT LEUR PROVENANCE** — les listes vivantes du
/// dock pour la sauvegarde, ou les listes LOCALES d'`app::run` avant `set_icon_order` pour la
/// résolution (voir la doc de module : c'est cette indirection qui rend une clé résoluble
/// assez tôt).
///
/// `Copy` : ce n'est que quatre tranches.
#[derive(Clone, Copy)]
pub struct Orders<'a> {
    pub left: &'a [usize],
    pub right: &'a [usize],
    pub top_left: &'a [usize],
    pub top_right: &'a [usize],
    /// ⛔ **CE COMMENTAIRE A AFFIRMÉ LE CONTRAIRE PENDANT DES MOIS, ET C'ÉTAIT
    /// FAUX** : il disait « toujours vide en pratique », et que les raccourcis de
    /// la barre du bas vivaient dans une table latérale. Or `Dock::set_dossiers`
    /// remplit `bottom_order` à chaque appel, et elle est **cette liste-ci** (voir
    /// `Dock::orders`, qui la passe en cinquième argument). Cette affirmation
    /// périmée a coûté un refus de glisser dans la barre du bas — le lâcher
    /// rendait `None` « puisqu'il n'y a rien à réordonner ».
    pub bottom: &'a [usize],
    /// **LE MILIEU DE LA BARRE DU HAUT** (voir `Side::TopMiddle`). Vide pour
    /// tous les appelants qui ne le passent pas — `Orders::new` le laisse vide
    /// et `avec_milieu` le remplit, plutôt qu'un sixième paramètre à ajouter à
    /// dix sites d'appel.
    pub top_middle: &'a [usize],
}

impl<'a> Orders<'a> {
    pub fn new(
        left: &'a [usize],
        right: &'a [usize],
        top_left: &'a [usize],
        top_right: &'a [usize],
        bottom: &'a [usize],
    ) -> Self {
        Self { left, right, top_left, top_right, bottom, top_middle: &[] }
    }

    /// Le milieu de la barre du haut, pour l'appelant qui le connaît.
    pub fn avec_milieu(mut self, milieu: &'a [usize]) -> Self {
        self.top_middle = milieu;
        self
    }

    /// La liste d'une barre.
    pub fn of(&self, side: Side) -> &'a [usize] {
        match side {
            Side::Left => self.left,
            Side::Right => self.right,
            Side::TopLeft => self.top_left,
            Side::TopRight => self.top_right,
            Side::TopMiddle => self.top_middle,
            Side::Bottom => self.bottom,
        }
    }

    /// **Les index de la barre, dans l'ordre où le rang les compte** — moitié par moitié
    /// (voir `BarLetter::halves`).
    pub fn of_bar(&self, bar: BarLetter) -> impl Iterator<Item = usize> + 'a {
        let orders = *self;
        bar.halves().iter().flat_map(move |&side| orders.of(side).iter().copied())
    }
}

// --- Le modèle ------------------------------------------------------------------------

/// **LES RÉGLAGES D'UNE ZONE.** Les valeurs absentes d'un fichier prennent leur défaut ici,
/// et un défaut ne s'écrit jamais (voir `config::zone_record_to_table`) : c'est ce qui garde
/// le fichier aussi court que ce que l'utilisateur a réellement réglé.
#[derive(Clone, PartialEq, Debug)]
pub struct ZoneRecord {
    /// La cote. ⚠️ **C'EST ELLE QUI DÉCLARE LA ZONE** : `Dock::is_zone_separator` EST
    /// `zone_height > 0`. Un enregistrement sans cote, ou avec une cote nulle, n'est pas une
    /// zone — une cote de 0 *défait* la zone, donc l'écrire serait écrire le contraire.
    pub cote: f32,
    /// Le vide au-dessus du séparateur (`Icon::zone_lead`).
    pub lead: f32,
    /// `None` = la zone porte son numéro automatique.
    pub name: Option<String>,
    /// Colonnes DEMANDÉES — `DEFAULT_ZONE_COLUMNS` (1) tant que l'utilisateur n'en demande
    /// pas plus, et c'est aussi ce qu'on n'écrit pas.
    pub columns: u16,
    pub locked: bool,
    /// Affiche son numéro ou son nom.
    pub label: bool,
    /// **LA ZONE EST CACHÉE : TOUTE SA PLAGE DISPARAÎT DE LA BARRE** — son séparateur
    /// **ET** son contenu. DEMANDE EXPLICITE (2026-09-23) : ces zones « ne puissent pas
    /// être supprimées mais seulement cachées ».
    ///
    /// ⚠️ **C'est `Icon::hidden` du SÉPARATEUR qui porte l'état à l'exécution**, et le
    /// saut de toute sa plage se lit dans `Dock::bar_layout`. On passe par ce masquage
    /// parce qu'il existe déjà — il exclut une icône de la disposition — et parce que
    /// le fichier n'a **nulle part** où ranger le masquage d'une icône quelconque :
    /// c'est donc ici, par CLÉ de zone, qu'il se persiste.
    ///
    /// **ET LA ZONE RESTE UNE ZONE** : `is_zone_separator` est la COTE, jamais la
    /// visibilité. Son RANG ne bouge donc pas, ni celui des autres — c'est ce qui
    /// permet de la réafficher, ou d'en cacher une autre, sans que rien ne se
    /// renumérote (le défaut que `zones.toml` désigne par rang a déjà payé).
    pub hidden: bool,
    /// **CETTE ZONE NE PEUT PAS ÊTRE SUPPRIMÉE** — le panneau n'offre plus que de la
    /// cacher. DEMANDE EXPLICITE (2026-09-23), pour trois zones nommées par
    /// l'utilisateur.
    ///
    /// ⚠️ **UN DRAPEAU PERSISTÉ, ET NON UNE LISTE DE NOMS DANS LE CODE** : une règle
    /// qui lirait les noms verrait sa protection **disparaître au premier renommage**,
    /// en silence — et ce dépôt a déjà écrit noir sur blanc qu'aucune règle métier ne
    /// lit les noms de zones (voir la pose de la bascule de thème dans `app::run`).
    pub permanent: bool,
    /// **LES ICÔNES DE LA ZONE, PAR LEUR IDENTITÉ** — `noyau:reglages`,
    /// `epingle:<cible>`, `dossier:<chemin>` (voir
    /// `docs/plans/disposition-barres-zones-icones.md`, étape 2).
    ///
    /// ⛔ **VIDE = RIEN N'EST ÉCRIT**, comme tous les autres champs : un
    /// `zones.toml` d'avant cette fonctionnalité ne bouge pas d'un octet. Une
    /// zone sans liste garde le contenu que lui donne le modèle par plages de
    /// `icon_order.txt` — la migration la remplira une fois (étape 4).
    pub icones: Vec<String>,
}

impl Default for ZoneRecord {
    fn default() -> Self {
        Self {
            cote: 0.0,
            lead: 0.0,
            name: None,
            columns: DEFAULT_ZONE_COLUMNS,
            locked: false,
            label: false,
            hidden: false,
            permanent: false,
            icones: Vec::new(),
        }
    }
}

/// **LES ESPACES D'UNE BARRE** — leur NOMBRE, et leurs tailles.
///
/// ⚠️ **LE NOMBRE EST UN SCALAIRE PRIORITAIRE, ET C'EST DÉLIBÉRÉ.** `sizes` est *ignoré en
/// entier* dès que sa longueur ne vaut pas le compte (voir `size_at`) : une taille est
/// COSMÉTIQUE, alors qu'un compte faux **déplace tous les rangs de la barre**, donc toutes
/// les zones. Un fichier tronqué doit perdre le cosmétique, jamais la structure.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct BarSpacers {
    pub spacers: usize,
    /// Vide = aucune taille enregistrée (toutes retombent au défaut).
    pub sizes: Vec<f32>,
    /// **LA SORTE DE CHAQUE ESPACE, PAR RANG** — une liste PARALLÈLE à `sizes`, et
    /// soumise à la MÊME règle : elle est ignorée **en entier** dès que sa longueur
    /// ne vaut pas le compte (voir `sorte_at`).
    ///
    /// ⛔ **ELLE VIT ICI, ET PAS DANS `ZoneRecord` — C'EST UNE MESURE QUI L'A
    /// DÉCIDÉ.** `Dock::zones()` n'écrit un `ZoneRecord` que pour un espace qui
    /// **déclare une zone** (`if !(icon.is_spacer && icon.zone_height > 0.0)`), et
    /// un enregistrement sans cote est refusé à la relecture (c'est la cote qui
    /// DÉCLARE la zone). Un séparateur ORDINAIRE n'a donc nulle part où porter sa
    /// sorte : mesuré sur la configuration de la machine — `zones.toml` portait
    /// `[bars.B] sizes = [180.0]` et AUCUN `[zones.B1]`, donc la sorte choisie
    /// n'était jamais écrite et le bouton semblait ne rien faire.
    ///
    /// Vide = aucune sorte enregistrée, donc toutes `Rond` — ce qui rend le fichier
    /// d'avant cette fonctionnalité relisible tel quel.
    pub sortes: Vec<SorteEspace>,
    /// **LA LISTE DE LA BARRE, POUR UNE BARRE QUI NE DÉCLARE AUCUNE ZONE.**
    ///
    /// ⚠️ **MESURÉ LE 2026-10-30, ET C'EST CE QUI L'A CRÉÉE** : sans `icon_order.txt`, la
    /// barre de GAUCHE retombait sur la disposition par défaut de `Dock::new` (36 icônes
    /// au lieu de 35, autre ordre) — ses identités n'étaient écrites nulle part, puisqu'une
    /// zone est le seul endroit où le modèle les range. Une barre sans séparateur ne peut
    /// PAS déclarer de zone (c'est la cote qui déclare, et la cote est celle d'un espace) :
    /// il lui fallait donc sa liste à elle, sans rien inventer de visible.
    ///
    /// **VIDE POUR UNE BARRE QUI A DES ZONES** — ce serait alors deux sources pour le même
    /// contenu. `repartir_les_icones` remplit l'un ou l'autre, jamais les deux.
    ///
    /// ⛔ **ELLE N'EXISTE QUE POUR UNE BARRE À UNE SEULE MOITIÉ** (`halves().len() == 1`) :
    /// le HAUT en a trois (gauche, milieu, droite), et une seule liste ne saurait pas dire
    /// laquelle — ses deux autres moitiés sont dans `icones_milieu` et `icones_droite`.
    pub icones: Vec<String>,
    /// **LA DEUXIÈME MOITIÉ DE LA BARRE DU HAUT** — le MILIEU, ancré au centre (voir
    /// `Side::TopMiddle`). Vide pour toutes les autres barres, et vide sur une configuration
    /// où le milieu ne porte aucune `Dock::Icon` (l'horloge est un widget, pas une icône).
    pub icones_milieu: Vec<String>,
    /// **LA TROISIÈME MOITIÉ DE LA BARRE DU HAUT** — la droite. Vide pour les autres barres.
    pub icones_droite: Vec<String>,
}

/// **L'ÉTAT COMPLET DES ZONES** — ce que `zones.toml` porte, et rien d'autre.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Zones {
    /// La zone qui reçoit les applications ajoutées depuis le Dash (ex-`dash_add_zone.txt`).
    pub dash_add_zone: Option<ZoneKey>,
    pub bars: BTreeMap<BarLetter, BarSpacers>,
    pub zones: BTreeMap<ZoneKey, ZoneRecord>,
}

impl Zones {
    pub fn record(&self, key: ZoneKey) -> Option<&ZoneRecord> {
        self.zones.get(&key)
    }

    /// **RANGE `identite` À LA FIN DE LA ZONE `key`**, et la retire de TOUTES les autres
    /// listes (zones et barres) : une identité citée deux fois serait rangée par la
    /// première liste lue. Rend `false` (sans rien toucher) si la zone n'existe pas.
    ///
    /// ⛔ **C'est ce qu'épingler dans une zone depuis le Dash n'écrivait pas** (constat de
    /// l'utilisateur, 2026-11-01 : « il disparaît au redémarrage ») : seul
    /// `icon_order.txt` était réécrit, or ce sont ces listes qui rangent les barres —
    /// l'épingle, citée nulle part, revenait en orpheline au bout de la barre de gauche.
    pub fn ranger_a_la_fin_de(&mut self, key: ZoneKey, identite: &str) -> bool {
        if !self.zones.contains_key(&key) {
            return false;
        }
        for record in self.zones.values_mut() {
            record.icones.retain(|i| i != identite);
        }
        for barre in self.bars.values_mut() {
            for liste in [&mut barre.icones, &mut barre.icones_milieu, &mut barre.icones_droite] {
                liste.retain(|i| i != identite);
            }
        }
        if let Some(record) = self.zones.get_mut(&key) {
            record.icones.push(identite.to_string());
        }
        true
    }

    /// **Le nombre d'espaces d'une barre — le compte DÉCLARÉ, relevé par le rang le plus
    /// haut qu'une zone de cette barre cite.**
    ///
    /// Le relèvement n'est pas un détail de confort : sans lui, une clé pourrait désigner un
    /// rang au-delà du compte, ne se résoudrait sur AUCUN espace, et la zone serait perdue
    /// en silence. Avec lui, **une clé déclarée est toujours résoluble**.
    pub fn spacer_count(&self, bar: BarLetter) -> usize {
        let declared = self.bars.get(&bar).map_or(0, |b| b.spacers);
        let cited = self.zones.keys().filter(|key| key.bar == bar).map(|key| key.rank).max().unwrap_or(0);
        declared.max(cited)
    }

    /// La taille de l'espace d'une clé — `None` si la barre n'en déclare aucune, **ou si la
    /// liste déclarée n'a pas la longueur du compte** (voir `BarSpacers`, doc du type).
    pub fn size_at(&self, key: ZoneKey) -> Option<f32> {
        let bar = self.bars.get(&key.bar)?;
        if bar.sizes.len() != self.spacer_count(key.bar) {
            return None;
        }
        bar.sizes.get(key.rank.checked_sub(1)?).copied()
    }

    /// **LA SORTE DE L'ESPACE D'UNE CLÉ** — `None` si la barre n'en déclare aucune,
    /// **ou si la liste n'a pas la longueur du compte** : même règle « tout ou
    /// rien » que `size_at`, et pour la même raison — une liste tronquée déplacerait
    /// les rangs, donc les sortes de TOUS les espaces de la barre.
    ///
    /// ⚠️ **Une entrée absente vaut `Rond`**, pas « pas de sorte » : une barre peut
    /// n'avoir qu'un espace non ordinaire sur cinq.
    pub fn sorte_at(&self, key: ZoneKey) -> Option<SorteEspace> {
        let bar = self.bars.get(&key.bar)?;
        if bar.sortes.len() != self.spacer_count(key.bar) {
            return None;
        }
        bar.sortes.get(key.rank.checked_sub(1)?).copied()
    }
}

// --- La répartition des icônes dans les zones ------------------------------------------

/// **VRAI POUR UNE IDENTITÉ D'EXÉCUTION** — une application OUVERTE (`ouvert:`) ou le
/// miroir de la zone de notification (`notification:`) : elles changent d'une session à
/// l'autre, et `zones.toml` ne doit donc **jamais** les écrire (voir `Dock::is_persistable`,
/// et l'avertissement de `ui/dock.rs:2541` : persister un index mobile vide la zone de
/// notification au lancement suivant).
///
/// **LA RÈGLE EST SUR L'IDENTITÉ, PAS SUR UN INDEX** : c'est l'identité qui dit la SORTE de
/// l'icône (voir la table du plan), donc la question se pose là où la sorte est connue.
pub fn identite_d_execution(identite: &str) -> bool {
    identite.starts_with("ouvert:") || identite.starts_with("notification:")
}

/// **LES BARRES QUE LA RÉPARTITION GOUVERNE** — la gauche et la droite, et elles seules.
///
/// ⚠️ **LE HAUT ET LE BAS SONT EXCLUS, ET C'EST MESURÉ** : `Dock::appliquer_les_zones` reverse
/// encore les rangs du HAUT au MILIEU (la découpe à trois zones n'est pas faite), donc y
/// écrire une liste déplacerait ses icônes d'une moitié à l'autre ; la barre du BAS est
/// pilotée par l'arrangement (`dossiers.toml`), pas par des rangs. Tant que ces deux
/// morceaux ne sont pas faits, leurs barres gardent `icon_order.txt`.
///
/// **UNE SEULE DÉFINITION**, lue par la répartition (`repartir_les_icones`) et par la
/// sauvegarde (`config::zones_a_ecrire`) : deux listes de barres qui divergeraient
/// effaceraient en silence les listes de celle qu'un seul des deux traités.
pub fn bar_repartie(bar: BarLetter) -> bool {
    matches!(bar, BarLetter::Gauche | BarLetter::Droite | BarLetter::Haut)
}

/// **CE QUE LA RÉPARTITION A FAIT** — montré au journal, jamais deviné.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct RapportRepartition {
    /// Le nombre d'icônes rangées dans une zone.
    pub icones_rangees: usize,
    /// Les icônes d'exécution vues et **écartées** — leur zone est écrite, leur identité non.
    pub d_execution_ecartees: usize,
    /// Les icônes qui n'ont PAS d'identité : elles ne peuvent pas être écrites, et le fait
    /// qu'elles existent est une information (le plancher de l'étape 1 en est le garde).
    pub sans_identite: usize,
    /// Les barres qu'on n'a PAS réparties faute d'y déclarer une seule zone.
    pub barres_sans_zone: Vec<BarLetter>,
}

/// **RÉPARTIT LES ICÔNES DANS LES ZONES DE LEUR BARRE** — étape 4 du plan
/// `docs/plans/disposition-barres-zones-icones.md`, le cœur pur (aucune E/S).
///
/// # Les règles, et celles qui sont des DÉCISIONS de l'utilisateur (2026-10-29)
///
/// 1. **Seules la GAUCHE et la DROITE sont réparties.** Le HAUT est encore une barre à
///    deux moitiés dont les rangs sont reversés au MILIEU par `Dock::appliquer_les_zones` :
///    y écrire une liste déplacerait ses icônes d'une moitié à l'autre. Le BAS est piloté
///    par l'arrangement (`dossiers.toml`), pas par des rangs. Tant que ces deux morceaux ne
///    sont pas faits, leurs barres gardent `icon_order.txt` — c'est dit, pas silencieux.
/// 2. **Une barre sans aucune zone n'est pas répartie** : elle est nommée dans
///    `barres_sans_zone`, et son ordre ne bouge pas.
/// 3. **Une icône qui précède la première zone de sa barre est rattachée à CETTE zone**
///    (décision de l'utilisateur). Elle est donc écrite en tête de sa liste — jamais
///    orpheline, jamais reversée dans la barre de gauche.
/// 4. **Un espace ORDINAIRE ne ferme pas la zone ouverte** : seule une cote déclare une
///    zone. Les icônes qui suivent un simple séparateur restent dans la zone courante,
///    ce qui est la seule règle qui n'en perde aucune.
/// 5. **Les identités d'exécution ne sont PAS écrites** (voir `identite_d_execution`) :
///    la zone existe, se remplit à l'exécution.
/// 6. **Une zone qui porte DÉJÀ une liste n'est jamais réécrite** : la répartition est
///    idempotente, et une liste éditée à la main survit à un lancement.
/// 7. ⛔ **UNE ICÔNE SANS IDENTITÉ EST COMPTÉE DANS `sans_identite`, JAMAIS ÉCRITE.** Ce
///    n'est pas un détail : l'écrire la retirerait de la disposition (aucune liste ne peut
///    la citer), donc l'appelant ne doit RIEN écrire quand le compte n'est pas nul. Mesuré
///    le 2026-10-30 : les épingles du dock n'avaient pas d'identité, et la zone « Logiciels »
///    sortait vide de `zones.toml`.
pub fn repartir_les_icones(
    zones: &mut Zones,
    ordres: Orders<'_>,
    is_spacer: impl Fn(usize) -> bool,
    identite: impl Fn(usize) -> Option<String>,
    // **`visible` — L'ICÔNE SE DESSINE-T-ELLE ?** Un placeholder MASQUÉ (une place d'épingle
    // réservée) n'a pas d'identité et ne se dessine jamais : il n'est donc pas un trou, et le
    // compter ferait refuser toute écriture sur la configuration réelle (mesuré le
    // 2026-10-30 : cinq places réservées, citées par `icon_order.txt`, dans la barre de
    // gauche).
    visible: impl Fn(usize) -> bool,
) -> RapportRepartition {
    let mut rapport = RapportRepartition::default();

    for bar in BarLetter::ALL {
        if !bar_repartie(bar) {
            rapport.barres_sans_zone.push(bar);
            continue;
        }
        // ⭐ **LE HAUT A TROIS MOITIÉS, DONC TROIS LISTES** (capture de l'utilisateur du
        // 2026-10-30 : gauche = NeuraDesk/pastilles/média, milieu = l'horloge, droite =
        // NUM/MAJ/CPU/réseau/volume/alimentation). Le modèle ne lui connaît pas de zone —
        // c'est la MOITIÉ qui range, et les trois listes sont écrites côte à côte.
        if bar == BarLetter::Haut {
            let mut par_moitie: [Vec<String>; 3] = [Vec::new(), Vec::new(), Vec::new()];
            let mut rang = 0usize;
            for (moitie, side) in [Side::TopLeft, Side::TopMiddle, Side::TopRight].into_iter().enumerate() {
                for index in ordres.of(side).iter().copied() {
                    if is_spacer(index) {
                        // ⚠️ **LE MILIEU N'A PAS DE RANG** : `of_bar` ne le compte pas
                        // (voir `BarLetter::halves`), donc un marqueur y serait
                        // irrésoluble. Il n'a pas de séparateur non plus — c'est une
                        // seule rangée, centrée.
                        if side != Side::TopMiddle {
                            rang += 1;
                            if let Some(cle) = ZoneKey::new(bar, rang) {
                                let marqueur = marqueur_d_espace(cle);
                                if !par_moitie[moitie].contains(&marqueur) {
                                    par_moitie[moitie].push(marqueur);
                                }
                            }
                        }
                        continue;
                    }
                    let Some(identite) = identite(index) else {
                        if visible(index) {
                            rapport.sans_identite += 1;
                        }
                        continue;
                    };
                    if identite_d_execution(&identite) {
                        rapport.d_execution_ecartees += 1;
                        continue;
                    }
                    if !par_moitie[moitie].contains(&identite) {
                        par_moitie[moitie].push(identite);
                        rapport.icones_rangees += 1;
                    }
                }
            }
            // ⚠️ **PAS D'ENTRÉE VIDE** : une barre dont les trois moitiés sont vides ne dit
            // rien, et l'écrire créerait une table que la relecture ne rendrait pas — la
            // sauvegarde compare l'état écrit à l'état relu.
            let deja = zones.bars.get(&bar);
            let vierge = deja.is_none_or(|entree| {
                entree.icones.is_empty() && entree.icones_milieu.is_empty() && entree.icones_droite.is_empty()
            });
            if vierge && par_moitie.iter().any(|liste| !liste.is_empty()) {
                let entree = zones.bars.entry(bar).or_default();
                let [gauche, milieu, droite] = par_moitie;
                entree.icones = gauche;
                entree.icones_milieu = milieu;
                entree.icones_droite = droite;
            }
            continue;
        }
        let mut rangs: Vec<ZoneKey> = zones.zones.keys().filter(|key| key.bar == bar).copied().collect();
        rangs.sort_by_key(|key| key.rank);
        let premiere = rangs.first().copied();

        // **LE PARCOURS, UNE SEULE FOIS** — chaque entrée de la barre, dans l'ordre, avec la
        // cible qui la revendique : une ZONE, ou `None` pour la barre elle-même (une barre
        // sans zone porte sa liste à elle, voir `BarSpacers::icones`).
        let mut par_cible: BTreeMap<Option<ZoneKey>, Vec<String>> = BTreeMap::new();
        let mut courante: Option<ZoneKey> = None;
        let mut rang = 0usize;
        for index in ordres.of_bar(bar) {
            if is_spacer(index) {
                // ⚠️ **LE RANG COMPTE TOUS LES ESPACES** (voir la doc de module) : un
                // espace ordinaire incrémente le rang sans ouvrir de zone.
                rang += 1;
                if let Some(cle) = rangs.iter().find(|key| key.rank == rang).copied() {
                    courante = Some(cle);
                }
                // ⛔ **UNE BARRE SANS ZONE CITE SES ESPACES PAR LEUR RANG.** Un espace n'a
                // pas d'autre identité, et sans ces marqueurs la reconstruction les perdrait
                // TOUS : `[bars.G] spacers = 2` s'éroderait à la première sauvegarde, et les
                // deux points de la barre disparaîtraient. Le rang EST leur identité dans ce
                // modèle (voir la doc de module) — le citer n'est pas nommer un index.
                if premiere.is_none() {
                    if let Some(cle) = ZoneKey::new(bar, rang) {
                        let liste = par_cible.entry(None).or_default();
                        let marqueur = marqueur_d_espace(cle);
                        if !liste.contains(&marqueur) {
                            liste.push(marqueur);
                        }
                    }
                }
                continue;
            }
            let cible = if premiere.is_some() { courante.or(premiere) } else { None };
            let Some(identite) = identite(index) else {
                // ⚠️ **UN PLACEHOLDER MASQUÉ N'EST PAS UN TROU** (voir le paramètre
                // `visible`) ; une icône VISIBLE sans identité, si.
                if visible(index) {
                    rapport.sans_identite += 1;
                }
                continue;
            };
            if identite_d_execution(&identite) {
                rapport.d_execution_ecartees += 1;
                continue;
            }
            let liste = par_cible.entry(cible).or_default();
            if liste.contains(&identite) {
                continue;
            }
            liste.push(identite);
            rapport.icones_rangees += 1;
        }

        for (cible, icones) in par_cible {
            match cible {
                Some(cle) => {
                    let Some(record) = zones.zones.get_mut(&cle) else { continue };
                    if record.icones.is_empty() {
                        record.icones = icones;
                    }
                }
                None => {
                    // Même règle : une liste vide ne crée pas d'entrée (voir le HAUT).
                    let deja = zones.bars.get(&bar).is_some_and(|barre| !barre.icones.is_empty());
                    if !deja && !icones.is_empty() {
                        zones.bars.entry(bar).or_default().icones = icones;
                    }
                }
            }
        }
    }

    // ⛔ **AUCUNE ENTRÉE VIDE** — une barre sans espace et sans liste ne dit RIEN. L'écrire
    // produirait une table que la relecture ne rend pas (l'écrivain ne la met pas), et
    // `save_zones` compare l'état écrit à l'état relu : la sauvegarde échouerait en silence.
    zones.bars.retain(|_, entree| {
        entree.spacers > 0
            || !entree.icones.is_empty()
            || !entree.icones_milieu.is_empty()
            || !entree.icones_droite.is_empty()
    });

    rapport
}

/// **LE MARQUEUR D'UN ESPACE DANS LA LISTE D'UNE BARRE** — `espace:G2` désigne le deuxième
/// espace de la barre de gauche.
///
/// ⛔ **C'EST UN RANG, PAS UN INDEX** : le rang est déjà l'identité d'un espace dans ce
/// modèle (voir la doc de module), et c'est la seule qui traverse deux machines. Sans ce
/// marqueur, la liste d'une barre sans zone perdrait TOUS ses espaces — ils n'ont pas
/// d'autre identité — et `[bars.X] spacers` s'éroderait à chaque sauvegarde.
pub const PREFIXE_ESPACE: &str = "espace:";

/// Le marqueur d'un espace, à écrire dans la liste d'une barre.
pub fn marqueur_d_espace(key: ZoneKey) -> String {
    format!("{PREFIXE_ESPACE}{key}")
}

/// La clé qu'un marqueur désigne — `None` pour tout ce qui n'en est pas un.
pub fn cle_du_marqueur(texte: &str) -> Option<ZoneKey> {
    texte.strip_prefix(PREFIXE_ESPACE).and_then(ZoneKey::parse)
}

/// **REMPLIT LES LISTES DES ZONES À PARTIR DE L'ORDRE VIVANT, EN GARDANT L'ÉTRANGER.**
///
/// C'est la porte de la SAUVEGARDE (`config::zones_a_ecrire`) : le dock répartit, puis on
/// remet ce que le fichier portait et que **cette machine ne sait pas résoudre**.
///
/// # Les deux règles, et pourquoi elles ne peuvent pas être une seule
///
/// - **Une barre répartie** (voir `bar_repartie`) voit ses listes RECONSTRUITES : c'est ce qui
///   fait survivre un glisser d'icône, que `icon_order.txt` seul ne saurait plus porter.
/// - **Ses identités INCONNUES ici** sont AJOUTÉES à la fin de leur zone : une épinglette qui
///   n'existe pas sur cette machine (ou une application d'un autre profil) est justement ce
///   qu'on veut transporter. Les effacer ferait perdre, à la première sauvegarde, la
///   disposition qu'on cherche à emporter.
/// - **Une barre non répartie** (le haut, le bas) garde sa liste VERBATIM : la répartition ne
///   la gouverne pas encore, donc l'écrire serait écrire du vide.
/// - ⛔ **Si une seule icône de la barre n'est pas NOMMABLE (`sans_identite > 0`), la liste du
///   fichier est gardée telle quelle** : reconstruire sans savoir nommer tout le contenu
///   retirerait de la disposition les icônes qu'on ne sait pas citer, et le lancement suivant
///   les reverserait dans la barre de gauche. Une liste un peu périmée vaut mieux qu'une
///   disposition amputée — et `repartir_les_icones` le DIT, l'appelant ne l'ignore pas.
///
/// ⚠️ **ELLE N'EST PAS IDEMPOTENTE POUR AUTANT** : deux appels de suite ne changent rien
/// (la seconde retrouve les mêmes identités), mais repartir d'un ordre vivant différent change
/// la liste — c'est exactement ce qu'on veut.
pub fn remplir_les_listes(
    zones: &mut Zones,
    lues: &Zones,
    ordres: Orders<'_>,
    is_spacer: impl Fn(usize) -> bool,
    identite: impl Fn(usize) -> Option<String>,
    visible: impl Fn(usize) -> bool,
    connues: &BTreeSet<String>,
) -> RapportRepartition {
    let rapport = repartir_les_icones(zones, ordres, is_spacer, identite, visible);
    // ⛔ **SI UNE ICÔNE N'EST PAS NOMMABLE, ON NE RÉÉCRIT RIEN POUR CETTE BARRE.** La
    // reconstruction la retirerait de la liste, et le lancement suivant la reverserait
    // dans la barre de gauche : une perte SILENCIEUSE causée par une sauvegarde. Mieux
    // vaut une liste un peu périmée qu'une disposition amputée — et c'est dit.
    let reconstruire = rapport.sans_identite == 0;

    for (key, record) in zones.zones.iter_mut() {
        let Some(lue) = lues.zones.get(key) else { continue };
        if !bar_repartie(key.bar) {
            if record.icones.is_empty() {
                record.icones = lue.icones.clone();
            }
            continue;
        }
        if !reconstruire {
            record.icones = lue.icones.clone();
            continue;
        }
        for identite in &lue.icones {
            if !connues.contains(identite) && !record.icones.contains(identite) {
                record.icones.push(identite.clone());
            }
        }
    }

    // **LA LISTE DE LA BARRE, MÊME RÈGLES** (une barre sans zone, voir
    // `BarSpacers::icones`) : reconstruite depuis l'ordre vivant, augmentée de ce que cette
    // machine ne sait pas résoudre, et **gardée telle quelle** si une icône n'est pas nommable.
    for bar in BarLetter::ALL {
        let lues_de_la_barre = lues.bars.get(&bar).cloned().unwrap_or_default();
        let Some(courante) = zones.bars.get_mut(&bar) else { continue };
        // **LES TROIS LISTES D'UNE BARRE, MÊME RÈGLE CHACUNE** — la gauche et la droite
        // n'en ont qu'une (`icones`) ; le HAUT en a trois (ses moitiés).
        for (lue, ecrite) in [
            (lues_de_la_barre.icones, &mut courante.icones),
            (lues_de_la_barre.icones_milieu, &mut courante.icones_milieu),
            (lues_de_la_barre.icones_droite, &mut courante.icones_droite),
        ] {
            if !bar_repartie(bar) {
                if ecrite.is_empty() {
                    *ecrite = lue;
                }
                continue;
            }
            if !reconstruire {
                *ecrite = lue;
                continue;
            }
            for identite in lue {
                if !connues.contains(&identite) && !ecrite.contains(&identite) {
                    ecrite.push(identite);
                }
            }
        }
    }
    rapport
}

// --- La migration ---------------------------------------------------------------------

/// **CE QUE LES SIX ANCIENS FICHIERS CONTENAIENT**, index par index. Lu par
/// `config::load_zones_legacy`, consommé par `migrer_zones` — qui est PUR, donc testable
/// sans toucher au disque.
///
/// ⚠️ **POURQUOI CE N'EST PAS `config::load_zones`.** Ces fichiers vivent dans `%APPDATA%` :
/// un test qui les lirait, ou pire qui les écrirait, modifierait la configuration RÉELLE de
/// la machine. Le même soin est déjà pris pour l'analyse des fichiers par écran
/// (`config::parse_secondary_screen_indexed`, « séparé pour être TESTABLE sans toucher au
/// disque »).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ZonesLegacy {
    /// `zone_heights.txt` — `(index, cote, vide)`. **SEULE source des cotes**, donc seule
    /// source de ce qui est une zone.
    pub cotes: Vec<(usize, f32, f32)>,
    pub noms: Vec<(usize, String)>,
    pub colonnes: Vec<(usize, u16)>,
    pub verrous: Vec<usize>,
    pub etiquettes: Vec<usize>,
    pub dash_add_zone: Option<usize>,
    /// `spacer_sizes.txt` — `(index, taille)`.
    pub tailles: Vec<(usize, f32)>,
}

impl ZonesLegacy {
    /// Vrai si les fichiers ne portent RIEN — auquel cas il n'y a pas de migration à faire,
    /// et surtout pas de `zones.toml` vide à créer.
    pub fn est_vide(&self) -> bool {
        self.cotes.is_empty()
            && self.noms.is_empty()
            && self.colonnes.is_empty()
            && self.verrous.is_empty()
            && self.etiquettes.is_empty()
            && self.dash_add_zone.is_none()
            && self.tailles.is_empty()
    }
}

/// **CE QUI N'A PAS PU ÊTRE MIGRÉ, ET POURQUOI.** Jamais deviné : la décision de conception
/// est qu'un réglage sans cote ne se recrée pas, parce qu'il n'y a alors **rien à reporter**
/// et qu'inventer une cote serait deviner.
#[derive(Clone, PartialEq, Debug)]
pub struct Perte {
    pub index: usize,
    pub raison: &'static str,
}

/// **CE QUE LA MIGRATION A FAIT** — c'est ce qui est écrit dans `zones_migration.toml`, puis
/// montré dans le panneau Réglages (décision explicite de l'utilisateur : la migration
/// s'annonce, elle ne se contente pas d'aller dans le journal).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct RapportMigration {
    /// Les zones qui étaient dans une barre, gardées à leur rang — `(clé neuve, ancien index)`.
    pub conservees: Vec<(ZoneKey, usize)>,
    /// Les zones qui n'étaient dans AUCUNE barre, recréées à la fin de la barre de droite,
    /// **dans l'ordre de leur ancien index**.
    ///
    /// ⚠️ **Cet ordre est celui de la MÉMOIRE — `zones_migration.toml` le perd.** `toml::Table`
    /// est un `BTreeMap`, donc les clés y sortent **alphabétiquement** (`D10` avant `D2`, mesuré).
    /// Sans conséquence : le fichier est écrit pour être MONTRÉ, jamais relu en
    /// `RapportMigration`. Voir `config::zones_migration_to_toml`, qui porte la mesure.
    pub recreees: Vec<(ZoneKey, usize)>,
    pub perdues: Vec<Perte>,
    /// Les tailles d'espaces qui ne désignaient aucun espace de barre (voir la doc de
    /// `migrer_zones` : ce sont, sur la configuration mesurée, les remplisseurs inertes).
    pub tailles_abandonnees: usize,
}

/// **OÙ UN ESPACE SE TROUVE, par sa clé** — le cœur de la résolution, partagé par la
/// migration et par `Dock::zone_key_of` (qui lui passe le prédicat réel du dock).
///
/// `is_spacer` est un PARAMÈTRE pour que la règle soit celle de l'appelant : le dock passe
/// `Dock::is_spacer`, la migration et les tests passent ce qu'ils veulent — et un test peut
/// ainsi figer les DEUX ensemble, pour qu'une divergence future soit attrapée.
///
/// `None` si l'index **n'est pas un espace**, ou s'il n'est cité par aucune barre : c'est ce
/// qui définit une ORPHELINE.
pub fn key_of_index(ordres: Orders<'_>, is_spacer: impl Fn(usize) -> bool, index: usize) -> Option<ZoneKey> {
    // ⚠️ **UN NON-ESPACE N'A PAS DE RANG**, et il ne doit surtout pas en recevoir un : la
    // séquence d'une barre contient aussi des épingles et des applications, qui feraient
    // alors compter un rang pour elles. Une taille d'espace enregistrée sur un index devenu
    // une icône doit être ABANDONNÉE, pas réattribuée à l'espace voisin.
    if !is_spacer(index) {
        return None;
    }
    for bar in BarLetter::ALL {
        let mut rank = 0;
        for candidate in ordres.of_bar(bar) {
            if is_spacer(candidate) {
                rank += 1;
            }
            if candidate == index {
                // ⚠️ **UN INDEX CITÉ DEUX FOIS DANS LA MÊME BARRE** ne peut pas arriver
                // (`set_icon_order` déduplique), mais s'il arrivait on rend le PREMIER rang
                // plutôt que le dernier : c'est le seul choix qui ne dépende pas de l'ordre
                // de parcours du reste de la liste.
                return ZoneKey::new(bar, rank.max(1));
            }
        }
    }
    None
}

/// **L'INDEX DE L'ESPACE DE RANG `key.rank` DANS LA BARRE `key.bar`**, lu dans la séquence
/// passée — l'inverse exact de `key_of_index`.
pub fn index_of_key(ordres: Orders<'_>, is_spacer: impl Fn(usize) -> bool, key: ZoneKey) -> Option<usize> {
    ordres.of_bar(key.bar).filter(|&i| is_spacer(i)).nth(key.rank.checked_sub(1)?)
}

/// **LE CŒUR PUR DE LA MIGRATION** — aucune E/S, aucun `Dock`.
///
/// # Les étapes
///
/// 1. **Candidats** : l'union des index cités par les six anciens fichiers.
/// 2. **Filtre strict** : seul un candidat qui porte une **cote > 0** est une zone. Les
///    autres vont dans `perdues` avec leur raison — ils ne sont **pas** devinés.
/// 3. **Résolution** : la barre dont la séquence cite son ancien index, et son rang parmi les
///    espaces de cette séquence (voir `key_of_index`). Aucune barre → **orpheline**.
/// 4. **Orphelines → recréées** : triées par **ancien index croissant**, et posées à la suite
///    des espaces déjà comptés de la barre de droite. ⚠️ **L'ORDRE EST CELUI DE L'ANCIEN
///    INDEX, pas celui des fichiers** : c'est la seule reconstitution qui garde les zones
///    dans l'ordre où elles étaient avant le décalage qui les a orphelinées.
/// 5. **Comptes** : le nombre d'espaces trouvés dans chaque barre, relevé par le rang le plus
///    haut cité (voir `Zones::spacer_count`).
/// 6. **`dash_add_zone`** : la clé de la zone que l'ancien index désignait.
/// 7. **Tailles** : chaque taille dont l'index est un espace d'une barre y est reportée ;
///    **les autres sont abandonnées et comptées**. Ce ne sont pas des pertes utiles : un
///    index qu'aucune liste d'ordre ne cite est un espace que **rien ne dessine** (voir la
///    doc d'`ensure_saved_spacers_exist`), et les autres sont les remplisseurs créés sous
///    `winpin_base`.
pub fn migrer_zones(legacy: &ZonesLegacy, ordres: Orders<'_>, is_spacer: impl Fn(usize) -> bool) -> (Zones, RapportMigration) {
    let mut rapport = RapportMigration::default();

    // 1. Les candidats — TOUS les index cités, par n'importe lequel des six fichiers.
    let mut candidats: BTreeSet<usize> = BTreeSet::new();
    candidats.extend(legacy.cotes.iter().map(|&(index, _, _)| index));
    candidats.extend(legacy.noms.iter().map(|&(index, _)| index));
    candidats.extend(legacy.colonnes.iter().map(|&(index, _)| index));
    candidats.extend(legacy.verrous.iter().copied());
    candidats.extend(legacy.etiquettes.iter().copied());
    candidats.extend(legacy.dash_add_zone);

    // 2. Le filtre strict — la cote est ce qui DÉCLARE une zone (voir `ZoneRecord::cote`).
    let mut zones = Zones::default();
    let mut cle_de: BTreeMap<usize, ZoneKey> = BTreeMap::new();
    // Les orphelines, rassemblées à part pour pouvoir les trier par ancien index (étape 4).
    let mut orphelines: Vec<(usize, ZoneRecord)> = Vec::new();

    for index in candidats {
        let Some(&(_, cote, lead)) = legacy.cotes.iter().find(|&&(i, _, _)| i == index) else {
            rapport.perdues.push(Perte { index, raison: "cite sans cote : ce n'etait pas une zone" });
            continue;
        };
        if cote <= 0.0 {
            rapport.perdues.push(Perte { index, raison: "cote nulle : une zone DEFaITE, rien a reporter" });
            continue;
        }
        let record = ZoneRecord {
            cote,
            lead,
            name: legacy.noms.iter().find(|&&(i, _)| i == index).map(|(_, name)| name.clone()),
            columns: legacy.colonnes.iter().find(|&&(i, _)| i == index).map_or(DEFAULT_ZONE_COLUMNS, |&(_, c)| c),
            locked: legacy.verrous.contains(&index),
            label: legacy.etiquettes.contains(&index),
            // L'ancien format ne connaissait ni le masquage ni la permanence : une
            // migration ne les invente pas.
            hidden: false,
            permanent: false,
            icones: Vec::new(),
        };
        match key_of_index(ordres, &is_spacer, index) {
            // 3. Elle était dans une barre : elle garde son rang.
            Some(key) => {
                cle_de.insert(index, key);
                zones.zones.insert(key, record);
                rapport.conservees.push((key, index));
            }
            // Elle n'était dans AUCUNE barre — on la recrée (étape 4).
            None => orphelines.push((index, record)),
        }
    }

    // 5 (première moitié). Le compte DÉCLARÉ : le nombre d'espaces que chaque barre cite
    // réellement dans sa séquence. C'est lui qui remplace l'ancienne moitié « cotes » de
    // `Dock::saved_spacer_capacity`, et c'est lui qui rend `zones.toml` autosuffisant quand
    // `icon_order.txt` manque.
    for bar in BarLetter::ALL {
        let n = ordres.of_bar(bar).filter(|&i| is_spacer(i)).count();
        if n > 0 {
            // La MIGRATION ne connaît pas les sortes : un espace migré est
            // ordinaire, comme il l'était (même règle que le masquage et la
            // permanence, juste au-dessus).
            zones.bars.insert(bar, BarSpacers { spacers: n, ..BarSpacers::default() });
        }
    }

    // 4. Les orphelines, à la suite des espaces DÉJÀ comptés de la barre de droite.
    let mut suivant = zones.bars.get(&BarLetter::Droite).map_or(0, |b| b.spacers) + 1;
    orphelines.sort_by_key(|&(index, _)| index);
    for (index, record) in orphelines {
        let Some(key) = ZoneKey::new(BarLetter::Droite, suivant) else { continue };
        suivant += 1;
        cle_de.insert(index, key);
        zones.zones.insert(key, record);
        rapport.recreees.push((key, index));
    }

    // 6. La zone qui reçoit les ajouts du Dash — SEULEMENT si elle désigne une zone. Sinon
    // l'entrée du menu disparaît d'elle-même (`cef_dash::add_zone_label` rend `None`), ce
    // qui est le bon comportement : une cible qui n'ouvre aucune zone n'existe pas.
    zones.dash_add_zone = legacy.dash_add_zone.and_then(|index| cle_de.get(&index).copied());

    // 7. Les tailles d'espaces, par clé.
    //
    // ⚠️ **UN `sizes` PARTIEL EST COMPLÉTÉ, JAMAIS LAISSÉ COURT** : une liste dont la
    // longueur ne vaut pas le compte est ignorée EN ENTIER à la relecture (voir
    // `Zones::size_at`), donc n'écrire que les rangs trouvés perdrait aussi les autres. On
    // remplit les rangs non cités avec le défaut, et la liste devient valide.
    //
    // Une barre qui n'a reçu AUCUNE taille garde `sizes` VIDE — c'est la différence entre
    // « aucune taille enregistrée » et « des tailles, toutes au défaut », et elle évite
    // d'écrire un tableau de dix-huit pour rien.
    for &(index, taille) in &legacy.tailles {
        let Some(key) = key_of_index(ordres, &is_spacer, index) else {
            rapport.tailles_abandonnees += 1;
            continue;
        };
        let count = zones.spacer_count(key.bar);
        let bar = zones.bars.entry(key.bar).or_default();
        bar.spacers = bar.spacers.max(count);
        if bar.sizes.len() != count {
            bar.sizes.resize(count, SPACER_HEIGHT);
        }
        if let Some(slot) = bar.sizes.get_mut(key.rank - 1) {
            *slot = taille;
        }
    }

    (zones, rapport)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Épingler dans une zone RANGE l'identité dans la liste de la zone — et l'ôte
    /// d'ailleurs (le déplacement d'une zone à l'autre passe par le même chemin).
    #[test]
    fn epingler_range_l_identite_dans_la_liste_de_la_zone() {
        let d2 = ZoneKey::parse("D2").unwrap();
        let d3 = ZoneKey::parse("D3").unwrap();
        let mut zones = Zones::default();
        zones.zones.insert(d2, ZoneRecord { cote: 100.0, icones: vec!["epingle_dock:exe:a.exe".into()], ..Default::default() });
        zones.zones.insert(d3, ZoneRecord { cote: 100.0, icones: vec!["epingle_dock:exe:x.exe".into()], ..Default::default() });
        zones.bars.insert(BarLetter::Gauche, BarSpacers { icones: vec!["epingle_dock:exe:x.exe".into()], ..Default::default() });

        assert!(zones.ranger_a_la_fin_de(d2, "epingle_dock:exe:x.exe"));
        assert_eq!(zones.zones[&d2].icones, ["epingle_dock:exe:a.exe", "epingle_dock:exe:x.exe"]);
        assert!(zones.zones[&d3].icones.is_empty(), "citée deux fois");
        assert!(zones.bars[&BarLetter::Gauche].icones.is_empty(), "restée à gauche");

        // Une zone inconnue : refus, et rien ne bouge.
        let avant = zones.clone();
        assert!(!zones.ranger_a_la_fin_de(ZoneKey::parse("D9").unwrap(), "epingle_dock:exe:x.exe"));
        assert_eq!(zones, avant);
    }

    /// **LES QUATRE LISTES DE LA CONFIGURATION MESURÉE**, relevées dans l'`icon_order.txt`
    /// de la machine le 2026-09-22. Elles sont ici en dur, et c'est voulu : la migration a
    /// été conçue CONTRE elles, donc les figer est ce qui empêche une régression de passer.
    fn ordres_mesures() -> (Vec<usize>, Vec<usize>, Vec<usize>, Vec<usize>) {
        let gauche = vec![
            0, 95, 96, 97, 98, 99, 101, 100, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116,
            117, 118, 92, 93, 94,
        ];
        let droite = vec![136, 13, 140, 3, 4, 5, 6, 141, 7, 14];
        let haut_gauche = vec![168];
        // ⚠️ **REMOUNTES UNE QUATRIEME FOIS le 2026-10-06** (les kaomojis,
        // la 30e applet) : 36 etait redevenu une APPLET, exactement comme ce
        // commentaire l'annonce plus bas. 41, 39 et 40 valaient 29, 27 et 28 à la mesure : des REMPLISSEURS juste
        // au-dessus du bloc des applets. Avec 22 applets et une épingle de plus (le test
        // `adding_a_dock_pin_shifts_no_spacer`), 27 devenait une APPLET — remontés de trois,
        // comme dans la configuration réelle d'aujourd'hui. Puis à 24 applets
        // (fonctionnalités facultatives et notifications, 2026-10-04), 30 le devenait à
        // son tour : remontés de trois encore. **Puis à 28 applets (la météo, le même
        // jour), 33 le devenait** : remontés de trois une troisième fois. Le piège a
        // rejoué exactement comme annoncé.
        //
        // ⚠️ **C'EST LE TEST QUI L'A DIT, PAS UN CALCUL** : `adding_a_dock_pin_shifts_no_spacer`
        // a rendu `[168, 35, 33, 148, 147, 146, 34]` contre `[168, 35, 148, 147, 146, 34]` —
        // 33 cessait d'être un espace dès qu'on ajoutait une épingle.
        // ⚠️ **REMOUNTÉES UNE CINQUIÈME FOIS le 2026-10-09** (l'applet Scanner, la
        // 34ᵉ) : **39 est redevenu une APPLET**, exactement comme ce commentaire
        // l'annonce depuis la deuxième fois. 41, 39 et 40 valaient 29, 27 et 28 à
        // la mesure, et sont maintenant **44, 42 et 43**. Le piège rejoue toutes
        // les trois applets : les remplisseurs cités ici sont juste au-dessus du
        // bloc d'applets, et le bloc finit par les rattraper.
        //
        // ⚠️ **C'EST TOUJOURS LE TEST QUI LE DIT, PAS UN CALCUL** :
        // `adding_a_dock_pin_shifts_no_spacer` a rendu `[168, 41, 39, 148, 147,
        // 146, 40]` contre `[168, 41, 148, 147, 146, 40]` — 39 cessait d'être un
        // espace dès qu'on ajoutait une épingle.
        // ⚠️ **REMONTÉES UNE SIXIÈME FOIS le 2026-10-12** (les symboles
        // mathématiques, la 37ᵉ applet) : **42 est redevenu une APPLET**, trois
        // applets après le Scanner, comme annoncé. Elles sont maintenant **47, 45 et
        // 46** ; le test a rendu `[168, 44, 42, 148, 147, 146, 43]` contre
        // `[168, 44, 148, 147, 146, 43]`.
        // ⚠️ **REMONTÉES UNE SEPTIÈME FOIS le 2026-10-15** (le narrateur, la 40ᵉ
        // applet) : **45 est redevenu une APPLET**, trois applets après les symboles
        // mathématiques. Elles sont maintenant **50, 48 et 49** ; le test a rendu
        // `[168, 47, 45, 148, 147, 146, 46]` contre `[168, 47, 148, 147, 146, 46]`.
        // ⚠️ **REMONTÉES UNE HUITIÈME FOIS le 2026-10-30** (l'observateur d'événements,
        // 44ᵉ applet) : **48 est redevenu une APPLET**, et les trois remplisseurs cités
        // sont maintenant **51, 49 et 50**. Même piège que les sept fois précédentes —
        // c'est le test qui le dit, pas un calcul.
        // ⚠️ **REMONTÉES UNE NEUVIÈME FOIS le 2026-10-31** (Enregistrer, 45ᵉ applet) :
        // **49 est redevenu une APPLET**, les trois remplisseurs sont **52, 50 et 51**.
        // Le test a rendu `[168, 51, 49, 148, 147, 146, 50]` contre `[168, 51, 148, 147, 146, 50]`.
        // ⚠️ **REMONTÉES UNE DIXIÈME FOIS le 2026-11-07** (le Clavier visuel, 46ᵉ applet) :
        // **50 est redevenu une APPLET**, les trois remplisseurs sont **53, 51 et 52**.
        // Le test a rendu `[168, 52, 50, 148, 147, 146, 51]` contre `[168, 52, 148, 147, 146, 51]`.
        // ⚠️ **REMONTÉES UNE ONZIÈME FOIS le 2026-11-07** (2048, 47ᵉ applet) :
        // **51 est redevenu une APPLET**, les trois remplisseurs sont **54, 52 et 53**.
        // Le test a rendu `[168, 53, 51, 148, 147, 146, 52]` contre `[168, 53, 148, 147, 146, 52]`.
        // ⚠️ **REMONTÉES UNE DOUZIÈME FOIS le 2026-11-07** (Cinq ou plus, 48ᵉ applet) :
        // **52 est redevenu une APPLET**, les trois remplisseurs sont **55, 53 et 54**.
        // Le test a rendu `[168, 54, 52, 148, 147, 146, 53]` contre `[168, 54, 148, 147, 146, 53]`.
        // ⚠️ **REMONTÉES UNE TREIZIÈME FOIS le 2026-11-07** (Quadrapassel, 49ᵉ applet) :
        // **53 est redevenu une APPLET**, les trois remplisseurs sont **56, 54 et 55**.
        // Le test a rendu `[168, 55, 53, 148, 147, 146, 54]` contre `[168, 55, 148, 147, 146, 54]`.
        // ⚠️ **REMONTÉES UNE QUATORZIÈME FOIS le 2026-11-08** (Lights Off, 50ᵉ applet) :
        // **54 est redevenu une APPLET**, les trois remplisseurs sont **57, 55 et 56**.
        // Le test a rendu `[168, 56, 54, 148, 147, 146, 55]` contre `[168, 56, 148, 147, 146, 55]`.
        // ⚠️ **REMONTÉES UNE QUINZIÈME FOIS le 2026-11-10** (Reversi, 51ᵉ applet) :
        // **55 est redevenu une APPLET**, les trois remplisseurs sont **58, 56 et 57**.
        // Le test a rendu `[168, 57, 55, 148, 147, 146, 56]` contre `[168, 57, 148, 147, 146, 56]`.
        // ⚠️ **REMONTÉES UNE SEIZIÈME FOIS le 2026-11-10** (Bovo, 52ᵉ applet) :
        // **56 est redevenu une APPLET**, les trois remplisseurs sont **59, 57 et 58**.
        // Le test a rendu `[168, 58, 56, 148, 147, 146, 57]` contre `[168, 58, 148, 147, 146, 57]`.
        let haut_droite = vec![9, 11, 10, 59, 2, 57, 12, 1, 8, 148, 147, 146, 58];
        (gauche, droite, haut_gauche, haut_droite)
    }

    /// Le prédicat d'espace de cette configuration-là : `winpin_base = 92`, 23 épingles
    /// Windows, donc les espaces commencent à 115 — exactement le filtre `index >= winpin_end`
    /// de `Dock::saved_spacer_capacity`.
    fn espace_mesure(index: usize) -> bool {
        index >= 115
    }

    /// Les six anciens fichiers, tels qu'ils étaient sur la machine.
    fn legacy_mesure() -> ZonesLegacy {
        ZonesLegacy {
            cotes: vec![
                (128, 400.0, 0.0),
                (129, 400.0, 0.0),
                (134, 400.0, 0.0),
                (137, 278.0, 0.0),
                (138, 400.0, 0.0),
                (140, 492.0, 0.0),
                (141, 1038.0, 0.0),
                (142, 1237.0, 0.0),
                (143, 400.0, 0.0),
            ],
            noms: vec![
                (137, "Réglages".to_string()),
                (142, "Notifications".to_string()),
                (141, "Accès rapide".to_string()),
            ],
            colonnes: vec![(134, 3), (137, 3), (141, 2), (142, 4)],
            verrous: vec![142],
            etiquettes: vec![137, 141, 142],
            dash_add_zone: Some(141),
            tailles: Vec::new(),
        }
    }

    // --- Les clés ---------------------------------------------------------------------

    #[test]
    fn zone_keys_round_trip_through_their_text_form() {
        // Arrange / Act / Assert — les trois barres, et un rang à deux chiffres.
        for texte in ["D1", "G2", "H10"] {
            let key = ZoneKey::parse(texte).expect("une cle valide doit se relire");
            assert_eq!(key.to_string(), texte, "l'aller-retour doit rendre le meme texte");
        }
        // La casse est tolerante, comme le reste du depot.
        assert_eq!(ZoneKey::parse("d3"), ZoneKey::new(BarLetter::Droite, 3));

        // Et tout le reste est refuse PLUTOT QUE DEVINE : ces chaines viennent d'un fichier
        // que l'utilisateur peut editer a la main.
        for texte in ["D0", "D", "3D", "D1x", "", "  ", "DD1", "D-1"] {
            assert_eq!(ZoneKey::parse(texte), None, "{texte:?} ne doit pas etre une cle");
        }
    }

    #[test]
    fn a_bar_letter_collapses_the_two_halves_of_the_top_bar() {
        // Arrange / Act / Assert — H est UNE barre, comme `Dock::bar_rank` la compte.
        assert_eq!(BarLetter::of_side(Side::TopLeft), BarLetter::Haut);
        assert_eq!(BarLetter::of_side(Side::TopRight), BarLetter::Haut);
        assert_eq!(BarLetter::of_side(Side::Left), BarLetter::Gauche);
        assert_eq!(BarLetter::of_side(Side::Right), BarLetter::Droite);

        // ⚠️ L'ASYMETRIE VOULUE : un espace neuf du haut s'annexe a la DERNIERE moitie.
        assert_eq!(BarLetter::Haut.append_side(), Side::TopRight);
        assert_eq!(BarLetter::Haut.halves(), &[Side::TopLeft, Side::TopRight]);
        assert_eq!(BarLetter::Gauche.append_side(), Side::Left);
    }

    // --- La résolution ----------------------------------------------------------------

    #[test]
    fn a_space_resolves_to_its_rank_in_its_bar_and_back() {
        // Arrange
        let (gauche, droite, haut_gauche, haut_droite) = ordres_mesures();
        let ordres = Orders::new(&gauche, &droite, &haut_gauche, &haut_droite, &[]);

        // Act / Assert — le rang compte TOUS les espaces, et seulement eux : l'epingle 3 et
        // l'icone 13 sont dans la barre de droite sans y prendre de rang.
        assert_eq!(key_of_index(ordres, espace_mesure, 136), ZoneKey::new(BarLetter::Droite, 1));
        assert_eq!(key_of_index(ordres, espace_mesure, 140), ZoneKey::new(BarLetter::Droite, 2));
        assert_eq!(key_of_index(ordres, espace_mesure, 141), ZoneKey::new(BarLetter::Droite, 3));
        assert_eq!(key_of_index(ordres, espace_mesure, 115), ZoneKey::new(BarLetter::Gauche, 1));
        assert_eq!(key_of_index(ordres, espace_mesure, 118), ZoneKey::new(BarLetter::Gauche, 4));
        // La barre du haut compte gauche PUIS droite, d'ou H4 pour 146.
        assert_eq!(key_of_index(ordres, espace_mesure, 168), ZoneKey::new(BarLetter::Haut, 1));
        assert_eq!(key_of_index(ordres, espace_mesure, 148), ZoneKey::new(BarLetter::Haut, 2));
        assert_eq!(key_of_index(ordres, espace_mesure, 146), ZoneKey::new(BarLetter::Haut, 4));

        // ⚠️ **UN NON-ESPACE N'A PAS DE RANG** — sans cette garde, l'epingle 3 prendrait le
        // rang 0 de la barre de droite et decalerait tout ce qui suit.
        assert_eq!(key_of_index(ordres, espace_mesure, 3), None);
        assert_eq!(key_of_index(ordres, espace_mesure, 13), None);
        // Et un index que rien ne cite n'est d'aucune barre : c'est ce qui fait une orpheline.
        assert_eq!(key_of_index(ordres, espace_mesure, 128), None);
        assert_eq!(key_of_index(ordres, espace_mesure, 137), None);

        // L'inverse rend exactement ce qu'on lui a donne, sur les quatre barres.
        for index in [115, 118, 136, 140, 141, 168, 148, 146] {
            let key = key_of_index(ordres, espace_mesure, index).expect("un espace cite a une cle");
            assert_eq!(index_of_key(ordres, espace_mesure, key), Some(index), "l'inverse doit rendre {index}");
        }
    }

    // --- La lecture du fichier ---------------------------------------------------------

    #[test]
    fn a_zone_record_without_a_cote_is_not_a_zone() {
        // Arrange : un nom, mais aucune cote. Ce n'est pas une zone — c'est ce que la
        // migration doit REFUSER de deviner, et le rapport doit le dire.
        let legacy = ZonesLegacy { noms: vec![(42, "Fantome".to_string())], ..ZonesLegacy::default() };
        let (gauche, droite, haut_gauche, haut_droite) = ordres_mesures();
        let ordres = Orders::new(&gauche, &droite, &haut_gauche, &haut_droite, &[]);

        // Act
        let (zones, rapport) = migrer_zones(&legacy, ordres, espace_mesure);

        // Assert
        assert!(zones.zones.is_empty());
        assert_eq!(rapport.perdues.len(), 1);
        assert_eq!(rapport.perdues[0].index, 42);
    }

    #[test]
    fn a_zone_record_with_a_zero_cote_is_dropped() {
        // Arrange : une cote de 0 DEF AIT la zone (`Dock::set_zone_height`), donc l'ecrire
        // serait ecrire le contraire de ce qu'on veut — meme regle que `load_zone_columns`,
        // qui ecarte un zero plutot que de le ramener a 1 en silence.
        let legacy = ZonesLegacy { cotes: vec![(140, 0.0, 0.0)], ..ZonesLegacy::default() };
        let (gauche, droite, haut_gauche, haut_droite) = ordres_mesures();
        let ordres = Orders::new(&gauche, &droite, &haut_gauche, &haut_droite, &[]);

        // Act
        let (zones, rapport) = migrer_zones(&legacy, ordres, espace_mesure);

        // Assert
        assert!(zones.zones.is_empty());
        assert_eq!(rapport.perdues.len(), 1);
        assert_eq!(rapport.perdues[0].raison.contains("nulle"), true);
    }

    #[test]
    fn a_rank_above_the_declared_count_raises_the_count() {
        // Arrange : une barre qui ne declare qu'un espace, et une zone au rang 4.
        let mut zones = Zones::default();
        zones.bars.insert(BarLetter::Droite, BarSpacers { spacers: 1, ..BarSpacers::default() });
        zones.zones.insert(ZoneKey::new(BarLetter::Droite, 4).unwrap(), ZoneRecord { cote: 400.0, ..ZoneRecord::default() });

        // Act / Assert — la cle doit TOUJOURS etre resoluble : sans ce relevement, D4 ne
        // designerait aucun espace et la zone serait perdue en silence.
        assert_eq!(zones.spacer_count(BarLetter::Droite), 4);

        // Et une liste de tailles qui ne fait pas la longueur du compte est ignoree EN
        // ENTIER : une taille est cosmetique, un compte faux deplacerait tous les rangs.
        let mut bar = zones.bars.get(&BarLetter::Droite).cloned().unwrap();
        bar.sizes = vec![42.0, 42.0];
        zones.bars.insert(BarLetter::Droite, bar);
        assert_eq!(zones.size_at(ZoneKey::new(BarLetter::Droite, 1).unwrap()), None);
    }

    // --- La migration, sur les DONNÉES MESURÉES ---------------------------------------

    /// **L'ORDRE DES RECRÉATIONS EST CELUI DE L'ANCIEN INDEX**, pas celui des fichiers : c'est
    /// la seule reconstitution qui garde les zones dans l'ordre où elles étaient avant le
    /// décalage qui les a orphelinées.
    #[test]
    fn the_measured_config_migrates_to_the_expected_zones() {
        // Arrange
        let legacy = legacy_mesure();
        let (gauche, droite, haut_gauche, haut_droite) = ordres_mesures();
        let ordres = Orders::new(&gauche, &droite, &haut_gauche, &haut_droite, &[]);

        // Act
        let (zones, rapport) = migrer_zones(&legacy, ordres, espace_mesure);

        // Assert — RIEN N'EST PERDU, et c'est l'engagement de la migration.
        assert_eq!(rapport.perdues, Vec::new(), "aucune perte sur la configuration mesuree");

        // Les deux zones qui étaient réellement dans une barre gardent leur rang.
        assert_eq!(
            rapport.conservees,
            vec![
                (ZoneKey::new(BarLetter::Droite, 2).unwrap(), 140),
                (ZoneKey::new(BarLetter::Droite, 3).unwrap(), 141),
            ]
        );

        // ⚠️ Les SEPT orphelines, recréées en fin de barre de droite, DANS L'ORDRE DE LEUR
        // ANCIEN INDEX (128, 129, 134, 137, 138, 142, 143) — et non dans l'ordre des fichiers.
        let attendues: Vec<(ZoneKey, usize)> = [(4, 128), (5, 129), (6, 134), (7, 137), (8, 138), (9, 142), (10, 143)]
            .into_iter()
            .map(|(rank, index)| (ZoneKey::new(BarLetter::Droite, rank).unwrap(), index))
            .collect();
        assert_eq!(rapport.recreees, attendues);

        // Le compte : 3 espaces cités à droite, relevé à 10 par les recréations.
        assert_eq!(zones.spacer_count(BarLetter::Gauche), 4);
        assert_eq!(zones.spacer_count(BarLetter::Haut), 4);
        assert_eq!(zones.spacer_count(BarLetter::Droite), 10);

        // Les réglages suivent leur zone, y compris sur les recréées.
        let d3 = zones.record(ZoneKey::new(BarLetter::Droite, 3).unwrap()).expect("D3");
        assert_eq!(d3.cote, 1038.0);
        assert_eq!(d3.name.as_deref(), Some("Accès rapide"));
        assert_eq!(d3.columns, 2);
        assert!(d3.label);

        let d7 = zones.record(ZoneKey::new(BarLetter::Droite, 7).unwrap()).expect("D7, ex-137");
        assert_eq!(d7.cote, 278.0);
        assert_eq!(d7.name.as_deref(), Some("Réglages"), "« Reglages » etait orpheline et doit revenir");
        assert_eq!(d7.columns, 3);
        assert!(d7.label);

        let d9 = zones.record(ZoneKey::new(BarLetter::Droite, 9).unwrap()).expect("D9, ex-142");
        assert_eq!(d9.cote, 1237.0);
        assert_eq!(d9.name.as_deref(), Some("Notifications"));
        assert_eq!(d9.columns, 4);
        assert!(d9.locked, "le verrou de 142 doit suivre");
        assert!(d9.label);

        // La zone des ajouts du Dash : l'ancien index 141 est devenu D3.
        assert_eq!(zones.dash_add_zone, ZoneKey::new(BarLetter::Droite, 3));

        // D1 (136) n'est PAS une zone : c'est un espace ordinaire qui n'apparait que dans les
        // tailles. Il ne doit donc avoir aucun enregistrement.
        assert_eq!(zones.record(ZoneKey::new(BarLetter::Droite, 1).unwrap()), None);
    }

    #[test]
    fn the_measured_config_migrates_the_sizes_it_can_and_counts_the_rest() {
        // Arrange — un extrait representatif de `spacer_sizes.txt` : deux tailles qui
        // designent un espace de barre, une qui designe un remplisseur sous `winpin_base`
        // (index 40), et une qui designe un espace cite par aucune liste (index 130).
        let legacy = ZonesLegacy {
            cotes: legacy_mesure().cotes,
            tailles: vec![(136, 394.0), (141, 18.0), (40, 18.0), (130, 180.0)],
            ..ZonesLegacy::default()
        };
        let (gauche, droite, haut_gauche, haut_droite) = ordres_mesures();
        let ordres = Orders::new(&gauche, &droite, &haut_gauche, &haut_droite, &[]);

        // Act
        let (zones, rapport) = migrer_zones(&legacy, ordres, espace_mesure);

        // Assert — les deux qui designent un espace sont reportees...
        assert_eq!(zones.size_at(ZoneKey::new(BarLetter::Droite, 1).unwrap()), Some(394.0));
        assert_eq!(zones.size_at(ZoneKey::new(BarLetter::Droite, 3).unwrap()), Some(18.0));
        // ... les deux autres sont ABANDONNEES et COMPTEES, jamais reattribuees au voisin.
        assert_eq!(rapport.tailles_abandonnees, 2);

        // ⚠️ Et la liste rendue fait la LONGUEUR DU COMPTE : un `sizes` partiel serait ignore
        // en entier a la relecture, donc perdrait aussi les tailles qu'on vient de reporter.
        assert_eq!(zones.bars.get(&BarLetter::Droite).unwrap().sizes.len(), zones.spacer_count(BarLetter::Droite));
    }

    // --- Les accesseurs de `Dock` ------------------------------------------------------
    //
    // Ces tests vivent ici et non dans `ui::dock::tests` : ils n'ont besoin d'AUCUN helper
    // prive du dock (seulement `Dock::new`, `ensure_spacer_capacity`, `ensure_winpin_capacity`,
    // `ensure_saved_spacers_exist`, `set_icon_order`), et les types de zone sont deja en
    // portee. Les garder avec la migration les rend aussi plus faciles a confronter.

    use crate::ui::dock::Dock;

    /// La frontière des épingles de la barre des tâches Windows sur la machine mesurée
    /// (`winpin_base.txt` = 92), et le nombre d'épingles que `read_pins()` y a comptées.
    const WINPIN_BASE: usize = 92;
    const WINPINS: usize = 24;
    /// Le nombre d'épingles du DOCK sur la machine mesurée (`dock_pins.txt`) — c'est le second
    /// argument de `Dock::new`, et l'oublier fait de la plage des épingles une plage d'espaces.
    const PINS: usize = 5;

    /// **LE MONTAGE DE LA CONFIGURATION MESURÉE, rejoué dans l'ordre d'`app::run`** :
    /// remplisseurs jusqu'à `winpin_base`, puis les 24 épingles Windows, puis les espaces que
    /// les listes et les cotes citent.
    ///
    /// `pins` est le nombre d'épingles du DOCK — le second argument de `Dock::new`.
    fn dock_avec_epingles(pins: usize) -> Dock {
        let ordres = ordres_mesures();
        let cotes: Vec<(usize, f32, f32)> = [128, 129, 134, 137, 138, 140, 141, 142, 143]
            .iter()
            .map(|&index| (index, 400.0, 0.0))
            .collect();
        // ⚠️ `Dock::new` prend le NOMBRE D'ÉPINGLES en second argument : passer `0` ne crée
        // aucun emplacement d'épingle, et les index 3..7 deviennent alors des REMPLISSEURS —
        // c'est-à-dire des espaces, ce qu'ils ne sont pas dans l'application réelle.
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], pins);
        dock.ensure_spacer_capacity(WINPIN_BASE);
        dock.ensure_winpin_capacity(WINPINS);
        // ⚠️ `winpin_end` DOIT valoir la longueur courante d'`icons` : c'est le contrat que
        // `ensure_saved_spacers_exist` verifie par un `debug_assert_eq!`. Apres les deux
        // appels ci-dessus elle vaut exactement `WINPIN_BASE + WINPINS`.
        dock.ensure_saved_spacers_exist(
            [&ordres.0, &ordres.1, &ordres.2, &ordres.3],
            cotes.iter().map(|&(index, _, _)| index).max(),
            WINPIN_BASE + WINPINS,
        );
        dock
    }

    fn dock_mesure() -> (Dock, Vec<usize>, Vec<usize>, Vec<usize>, Vec<usize>) {
        let ordres = ordres_mesures();
        (dock_avec_epingles(PINS), ordres.0, ordres.1, ordres.2, ordres.3)
    }

    /// **AJOUTER UNE ÉPINGLE DU DOCK NE DÉCALE AUCUN ESPACE — MESURÉ.**
    ///
    /// C'est la prémisse de tout le changement de clé… et la réfutation de ce que faisait le
    /// code d'avant. `cef_dash::handle_add_to_zone` translatait de +1 **tout index ≥ l'index
    /// de l'épingle ajoutée**, ordres compris, et `config::decaler_les_zones` faisait de même
    /// sur les fichiers de zone. Or les remplisseurs créés par `ensure_spacer_capacity`
    /// **absorbent** la croissance : `winpin_base` est FIGÉ par `winpin_base.txt`, donc la
    /// plage des épingles Windows commence toujours au même index et **les espaces ne bougent
    /// pas d'un cran**. Ce +1 était un décalage FANTÔME — la désynchronisation de la
    /// configuration vient de là, et pas du calcul des clés.
    #[test]
    fn adding_a_dock_pin_shifts_no_spacer() {
        // Arrange — le montage réel, avec une épingle de dock en plus. Les DEUX reçoivent le
        // meme ordre persiste, sans quoi `zone_key_of` ne pourrait rien resoudre.
        let (gauche, droite, haut_gauche, haut_droite) = ordres_mesures();
        let mut avant = dock_avec_epingles(PINS);
        let mut apres = dock_avec_epingles(PINS + 1);
        for dock in [&mut avant, &mut apres] {
            dock.set_icon_order(gauche.clone(), droite.clone(), haut_gauche.clone(), haut_droite.clone());
        }
        let ordres = Orders::new(&gauche, &droite, &haut_gauche, &haut_droite, &[]);

        // Act / Assert — chaque espace de chaque barre est au MÊME index, et garde son rang.
        for bar in BarLetter::ALL {
            let espaces_avant: Vec<usize> = ordres.of_bar(bar).filter(|&i| avant.is_spacer(i)).collect();
            let espaces_apres: Vec<usize> = ordres.of_bar(bar).filter(|&i| apres.is_spacer(i)).collect();
            assert_eq!(espaces_avant, espaces_apres, "barre {} : aucun espace ne doit bouger", bar.as_char());
        }
        for index in [116, 117, 118, 136, 140, 141, 168, 148] {
            assert_eq!(avant.is_spacer(index), apres.is_spacer(index), "l'espace {index} reste un espace");
            assert_eq!(
                avant.zone_key_of(index),
                apres.zone_key_of(index),
                "l'espace {index} garde SA CLE — c'est tout l'objet des cles stables"
            );
        }
    }

    #[test]
    fn the_rank_counts_every_spacer_of_the_bar_not_just_separators() {
        // Arrange — trois espaces a droite, dont SEUL le deuxieme declare une zone.
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let premier = dock.add_spacer_at_end(Side::Right);
        let deuxieme = dock.add_spacer_at_end(Side::Right);
        let troisieme = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(deuxieme, 400.0);
        dock.set_icon_order(Vec::new(), vec![premier, deuxieme, troisieme], Vec::new(), Vec::new());

        // Act / Assert — DECISION 1, figee : le rang compte TOUS les espaces. Compter les seuls
        // separateurs serait circulaire (`is_zone_separator` EST `zone_height > 0`), mais si
        // quelqu'un "corrige" un jour ca, ce test le dit.
        assert!(dock.is_zone_separator(deuxieme));
        assert!(!dock.is_zone_separator(premier), "premisse : le premier est un espace ORDINAIRE");
        assert_eq!(dock.zone_key_of(premier), ZoneKey::new(BarLetter::Droite, 1));
        assert_eq!(dock.zone_key_of(deuxieme), ZoneKey::new(BarLetter::Droite, 2));
        assert_eq!(dock.zone_key_of(troisieme), ZoneKey::new(BarLetter::Droite, 3));
    }

    /// **LE TEST DU POINT DUR** — sans lui, la propriete la plus fragile du plan ne serait
    /// garantie par rien.
    #[test]
    fn a_zone_key_resolves_before_set_icon_order_has_run() {
        // Arrange
        let (dock, gauche, droite, haut_gauche, haut_droite) = dock_mesure();
        let ordres = Orders::new(&gauche, &droite, &haut_gauche, &haut_droite, &[]);

        // ⚠️ PRÉMISSE : a cet instant le dock porte encore les listes PAR DEFAUT de
        // `Dock::new`, et AUCUN espace n'est un separateur — c'est exactement le piege que la
        // doc d'`app::run` decrit, et c'est ce qui rend la requete naturelle inutilisable.
        assert_ne!(dock.orders().right, ordres.right, "premisse : les listes persistees ne sont pas encore vivantes");
        assert!(!dock.is_zone_separator(141), "premisse : aucune cote n'est encore appliquee");

        // Act / Assert — la cle se resout QUAND MEME : elle ne lit que la sequence qu'on lui
        // passe et le predicat `is_spacer`, jamais `order(side)` ni la cote.
        let cle = |rank| ZoneKey::new(BarLetter::Droite, rank).unwrap();
        assert_eq!(dock.spacer_index_at_rank(ordres, cle(1)), Some(136));
        assert_eq!(dock.spacer_index_at_rank(ordres, cle(2)), Some(140));
        assert_eq!(dock.spacer_index_at_rank(ordres, cle(3)), Some(141));
        assert_eq!(dock.spacer_index_at_rank(ordres, ZoneKey::new(BarLetter::Gauche, 3).unwrap()), Some(118));
        // ⚠️ La barre du HAUT compte TROIS ESPACES DE PLUS que ce qu'on attendrait : 50, 48 et
        // 49 sont des REMPLISSEURS (crees sous `winpin_base` par `ensure_spacer_capacity`), donc
        // des espaces, et la liste du haut les cite. Mesure, pas supposition — une version
        // anterieure de la sonde les avait rates et annoncait 4 espaces au lieu de 7.
        // ⚠️ **REMONTES DE 41/39/40 A 44/42/43** le 2026-10-09 (l'applet Scanner), en meme
        // temps que la fixture : ce sont les MEMES trois remplisseurs, et les citer ici avec
        // leurs anciens index ferait decrire une barre qui n'existe plus. Puis de 44/42/43 A
        // 47/45/46 le 2026-10-12 (les symboles mathematiques), puis A 50/48/49 le 2026-10-15
        // (le narrateur), pour la meme raison.
        assert_eq!(dock.spacer_index_at_rank(ordres, ZoneKey::new(BarLetter::Haut, 1).unwrap()), Some(168));
        assert_eq!(dock.spacer_index_at_rank(ordres, ZoneKey::new(BarLetter::Haut, 4).unwrap()), Some(148));
        // ⚠️ **REMONTÉ le 2026-10-30 avec la fixture** : le rang 7 du haut désigne maintenant
        // **50** (l'observateur d'événements a mangé un remplisseur de plus).
        // ⚠️ Puis **51** le 2026-10-31 (Enregistrer), avec la fixture.
        // ⚠️ Puis **52** le 2026-11-07 (le Clavier visuel), avec la fixture.
        // ⚠️ Puis **53** le 2026-11-07 (2048), avec la fixture.
        // ⚠️ Puis **54** le 2026-11-07 (Cinq ou plus), avec la fixture.
        // ⚠️ Puis **55** le 2026-11-07 (Quadrapassel), avec la fixture.
        // ⚠️ Puis **56** le 2026-11-08 (Lights Off), avec la fixture.
        // ⚠️ Puis **57** le 2026-11-10 (Reversi), avec la fixture.
        // ⚠️ Puis **58** le 2026-11-10 (Bovo), avec la fixture.
        assert_eq!(dock.spacer_index_at_rank(ordres, ZoneKey::new(BarLetter::Haut, 7).unwrap()), Some(58));
        // Au-dela du dernier rang, aucun espace.
        assert_eq!(dock.spacer_index_at_rank(ordres, cle(4)), None);
    }

    #[test]
    fn zone_key_of_is_the_inverse_of_spacer_index_at_rank() {
        // Arrange
        let (mut dock, gauche, droite, haut_gauche, haut_droite) = dock_mesure();
        dock.set_icon_order(gauche, droite, haut_gauche, haut_droite);

        // Act / Assert — l'aller-retour, sur les quatre barres.
        for index in [116, 117, 118, 136, 140, 141, 168, 148, 147, 146] {
            let key = dock.zone_key_of(index).unwrap_or_else(|| panic!("l'espace {index} doit avoir une cle"));
            assert_eq!(dock.spacer_index_at_rank(dock.orders(), key), Some(index), "aller-retour sur {index}");
        }
        // Un non-espace n'a pas de rang, et un orphelin n'a pas de barre.
        assert_eq!(dock.zone_key_of(3), None);
        assert_eq!(dock.zone_key_of(13), None);
        assert_eq!(dock.zone_key_of(128), None, "128 n'est cite par aucune liste : orpheline");
    }

    #[test]
    fn the_bar_spacer_count_ignores_the_inert_fillers_below_winpin_base() {
        // Arrange
        let (dock, gauche, droite, haut_gauche, haut_droite) = dock_mesure();
        let ordres = Orders::new(&gauche, &droite, &haut_gauche, &haut_droite, &[]);

        // Assert — les remplisseurs sous `winpin_base` SONT des espaces (`ensure_spacer_capacity`
        // les cree ainsi), mais aucune liste ne les cite : ils ne prennent donc aucun rang.
        // ⚠️ 53 → 57 le 2026-11-08 (Lights Off) : un remplisseur que la fixture CITE,
        // donc qui suit chaque applet ajoutée au lieu de redevenir une applet.
        // ⚠️ 57 → 58 le 2026-11-10 (Reversi) : c'est le PLUS HAUT des trois
        // remplisseurs de `haut_droite`, donc celui qui reste un espace le plus longtemps.
        // ⚠️ 58 → 59 le 2026-11-10 (Bovo), même raison.
        assert!(dock.is_spacer(59), "premisse : un remplisseur est bien un espace");
        // ⚠️ Les remplisseurs SONT des espaces, mais seuls ceux qu'une liste cite prennent un
        // rang. La barre de gauche en cite 3 (116..118) et non les 77 remplisseurs ; celle du
        // haut en cite 3 d'entre eux (35, 33, 34) en plus de ses 4 espaces « vrais ».
        assert_eq!(dock.bar_spacer_count(ordres, BarLetter::Gauche), 3, "116, 117, 118");
        assert_eq!(dock.bar_spacer_count(ordres, BarLetter::Droite), 3);
        assert_eq!(dock.bar_spacer_count(ordres, BarLetter::Haut), 7, "168, 35, 33, 148, 147, 146, 34");
        // Et le premier espace de gauche est bien 116, pas le premier remplisseur venu.
        assert_eq!(dock.spacer_index_at_rank(ordres, ZoneKey::new(BarLetter::Gauche, 1).unwrap()), Some(116));
    }

    #[test]
    fn apply_zones_restores_every_attribute_and_only_by_key() {
        // Arrange — trois espaces a droite, dont deux deviennent des zones.
        let (gauche, haut_gauche, haut_droite): (Vec<usize>, Vec<usize>, Vec<usize>) =
            (Vec::new(), Vec::new(), Vec::new());
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let premier = dock.add_spacer_at_end(Side::Right);
        let deuxieme = dock.add_spacer_at_end(Side::Right);
        let troisieme = dock.add_spacer_at_end(Side::Right);
        let droite: Vec<usize> = vec![premier, deuxieme, troisieme];
        dock.set_icon_order(gauche.clone(), droite.clone(), haut_gauche.clone(), haut_droite.clone());

        let mut zones = Zones::default();
        zones.zones.insert(
            ZoneKey::new(BarLetter::Droite, 2).unwrap(),
            ZoneRecord { cote: 500.0, lead: 12.0, name: Some("Musique".into()), columns: 3, locked: true, label: true, hidden: false, permanent: false, icones: Vec::new() },
        );
        zones.zones.insert(
            ZoneKey::new(BarLetter::Droite, 3).unwrap(),
            ZoneRecord { cote: 250.0, ..ZoneRecord::default() },
        );

        // Act
        dock.apply_zones(Orders::new(&gauche, &droite, &haut_gauche, &haut_droite, &[]), &zones);

        // Assert — tous les reglages, et RIEN d'autre : l'espace ordinaire n'a rien recu.
        assert_eq!(dock.zone_height(deuxieme), Some(500.0));
        assert_eq!(dock.zone_lead(deuxieme), Some(12.0));
        assert_eq!(dock.zone_columns(deuxieme), Some(3));
        assert!(dock.is_zone_locked(deuxieme));
        assert_eq!(dock.zone_name(deuxieme), Some("Musique"));
        assert!(dock.is_zone_label_shown(deuxieme), "l'etiquette a suivi la cote");
        assert_eq!(dock.zone_height(troisieme), Some(250.0));
        assert_eq!(dock.zone_name(troisieme), None);
        assert_eq!(dock.zone_height(premier), Some(0.0));
        assert!(!dock.is_zone_separator(premier));

        // Et l'export rend exactement ce qu'on vient d'appliquer, par cle.
        assert_eq!(dock.zones().zones, zones.zones);
    }

    #[test]
    fn completer_les_barres_creates_the_declared_missing_spacers_at_the_end() {
        // Arrange — la droite ne cite qu'un espace, le fichier en declare trois.
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let seul = dock.add_spacer_at_end(Side::Right);
        dock.set_icon_order(Vec::new(), vec![seul], Vec::new(), Vec::new());

        let mut zones = Zones::default();
        zones.bars.insert(BarLetter::Droite, BarSpacers { spacers: 3, ..BarSpacers::default() });

        // Act
        dock.compléter_les_barres(&zones);

        // Assert — deux espaces neufs, et l'existant reste le PREMIER : les rangs deja
        // attribues ne bougent pas, ce qui est tout l'enjeu.
        assert_eq!(dock.bar_spacer_count(dock.orders(), BarLetter::Droite), 3);
        assert_eq!(dock.orders().right.first().copied(), Some(seul));
        assert_eq!(dock.spacer_index_at_rank(dock.orders(), ZoneKey::new(BarLetter::Droite, 1).unwrap()), Some(seul));

        // Et JAMAIS de retrait : un compte plus PETIT que la realite ne supprime rien.
        let mut moindre = Zones::default();
        moindre.bars.insert(BarLetter::Droite, BarSpacers { spacers: 1, ..BarSpacers::default() });
        dock.compléter_les_barres(&moindre);
        assert_eq!(dock.bar_spacer_count(dock.orders(), BarLetter::Droite), 3, "creer est reparable, supprimer ne l'est pas");
    }

    #[test]
    fn a_separator_that_no_bar_cites_is_not_exported() {
        // Arrange — un separateur ORPHELIN, comme les sept de la configuration mesuree.
        let (gauche, droite, haut_gauche, haut_droite) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 0);
        let orphelin = dock.add_spacer_at_end(Side::Right);
        dock.set_zone_height(orphelin, 400.0);
        dock.set_zone_name(orphelin, Some("Fantome"));
        // L'ordre ne le cite PAS : c'est ce qui en fait une orpheline.
        dock.set_icon_order(gauche, droite, haut_gauche, haut_droite);

        // Act / Assert — sans cle, elle n'est pas exportee. C'est deliberé : rien ne la dessine,
        // donc ses reglages n'ont aucun effet, et les garder ne faisait que grossir le fichier.
        assert!(dock.is_zone_separator(orphelin), "premisse : c'est bien un separateur");
        assert_eq!(dock.zone_key_of(orphelin), None);
        assert!(dock.zones().zones.is_empty());
    }

    // --- La répartition des icônes dans les zones (étape 4) ----------------------------

    /// **LE MONTAGE D'`app::run`, EN PETIT** : trois séparateurs de zone dans la barre de
    /// droite, quatre icônes du noyau NOMMÉES, et un ordre où elles sont réparties.
    ///
    /// Rend le dock, l'ordre posé, et les trois index de séparateur.
    fn dock_a_trois_zones() -> (Dock, Vec<usize>, [usize; 3]) {
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 5);
        dock.poser_noms(NOMS_DES_TROIS_ZONES);
        // ⚠️ **LES ÉPINGLES DOIVENT ÊTRE NOMMABLES, MÊME DANS UN TEST** : sans identité,
        // elles comptent dans `sans_identite`, et la sauvegarde refuse alors de réécrire
        // TOUTES les listes (voir `remplir_les_listes`). C'est la règle mesurée du
        // 2026-10-30, pas une commodité de test.
        dock.poser_cles_des_pins((0..5).map(|rang| format!("exe:pin{rang}.exe")).collect());
        // ⚠️ **LES PLACES D'ÉPINGLE RÉSERVÉES SONT MASQUÉES, COMME DANS L'APPLICATION**
        // (`Dock::masquer_les_places_libres`) : sans ça, des dizaines de placeholders sans
        // identité traînent dans la barre et la répartition les compterait comme des trous —
        // ce que la configuration réelle ne fait pas.
        let reservees = dock.icons.len().saturating_sub(crate::app::PINNED_ICON_BASE);
        dock.masquer_les_places_libres(5, reservees);
        let s1 = dock.add_spacer_at_end(Side::Right);
        let s2 = dock.add_spacer_at_end(Side::Right);
        let s3 = dock.add_spacer_at_end(Side::Right);
        let ordre = vec![s1, 2, 1, s2, 3, 4, s3];
        dock.set_icon_order(Vec::new(), ordre.clone(), Vec::new(), Vec::new());
        for sep in [s1, s2, s3] {
            dock.set_zone_height(sep, 400.0);
        }
        (dock, ordre, [s1, s2, s3])
    }

    /// Les noms du noyau posés par le montage ci-dessus — les index sont ceux de `Dock::new`
    /// avec cinq épingles de dock : 0 logo, 1 presse-papiers, 2 réglages, 3..7 épinglées.
    const NOMS_DES_TROIS_ZONES: &[(usize, &'static str)] =
        &[(1, "presse-papiers"), (2, "reglages"), (3, "applications"), (4, "outils")];

    /// **DES ZONES DÉCLARÉES, SANS AUCUN DOCK** — pour les sondes qui n'ont besoin que du
    /// modèle. `[bars.X]` compte jusqu'au rang le plus haut, comme le fichier le porte.
    fn zones_declarees(records: &[(BarLetter, usize, f32)]) -> Zones {
        let mut zones = Zones::default();
        for &(bar, rank, cote) in records {
            if let Some(key) = ZoneKey::new(bar, rank) {
                zones.zones.insert(key, ZoneRecord { cote, ..ZoneRecord::default() });
            }
        }
        for bar in BarLetter::ALL {
            let haut = zones.zones.keys().filter(|key| key.bar == bar).map(|key| key.rank).max().unwrap_or(0);
            if haut > 0 {
                zones.bars.insert(bar, BarSpacers { spacers: haut, ..BarSpacers::default() });
            }
        }
        zones
    }

    /// **LE TEST QUI COMPTE** (plan, étape 4) : répartir, puis vérifier POSITION PAR POSITION
    /// que les listes des zones redonnent l'ordre d'origine — séparateurs compris.
    #[test]
    fn the_measured_disposition_is_repartie_into_its_zones_and_comes_back_identical() {
        // Arrange
        let (dock, ordre, _) = dock_a_trois_zones();
        let mut zones = dock.zones();

        // Act
        let rapport = repartir_les_icones(&mut zones, dock.orders(), |i| dock.is_spacer(i), |i| dock.identite(i), |i| !dock.is_hidden(i));

        // Assert — chaque zone a SA liste, dans l'ordre de la barre.
        let liste = |rank: usize| {
            zones.record(ZoneKey::new(BarLetter::Droite, rank).unwrap()).expect("zone declaree").icones.clone()
        };
        assert!(rapport.icones_rangees >= 4, "au moins les quatre de D1 et D2");
        assert_eq!(liste(1), vec!["noyau:reglages", "noyau:presse-papiers"]);
        assert_eq!(liste(2), vec!["noyau:applications", "noyau:outils"]);
        assert_eq!(liste(3), Vec::<String>::new(), "une zone VIDE reste une zone");

        // Act — un dock NEUF, mêmes espaces, mêmes noms : la barre de droite ne cite QUE ses
        // trois séparateurs, et seuls les listes des zones doivent ramener les icônes.
        let (mut relu, ordre_neuf, separes) = dock_a_trois_zones();
        assert_eq!(ordre_neuf, ordre, "premisse : les deux docks ont les memes index de separateur");
        relu.set_icon_order(vec![0, 5, 6, 7], separes.to_vec(), Vec::new(), Vec::new());
        let (gauche, droite, haut, _, _) = relu.ordres_des_zones(&zones).expect("les zones portent des listes");

        // Assert — POSITION PAR POSITION, séparateurs compris : les séparateurs sont
        // écrits par la reconstruction (voir `Dock::ordres_des_zones`), sans quoi la barre
        // perdrait tous ses espaces et les zones avec eux.
        assert_eq!(droite, ordre, "la disposition d'avant la migration doit etre celle d'apres");
        // ⭐ **LA GAUCHE AUSSI REVIENT, DEPUIS SA PROPRE LISTE** (étape 5) : elle ne déclare
        // aucune zone, donc `repartir_les_icones` lui a écrit sa liste de barre — et c'est
        // elle qui la restitue, à l'identique.
        //
        // ⚠️ **LES PLACEHOLDERS MASQUÉS N'Y SONT PAS CITÉS** (ils n'ont pas d'identité et ne
        // se dessinent jamais) : ce sont des orphelines, et `set_icon_order` les reverse à la
        // fin de la gauche — donc à leur place. On compare donc ce qui est NOMMABLE.
        let cites: Vec<usize> =
            dock.orders().left.iter().copied().filter(|&index| dock.identite(index).is_some()).collect();
        assert_eq!(gauche, cites, "la gauche revient par SA liste de barre");
        assert!(haut.is_empty(), "le haut n'a aucune zone et aucune liste : il n'est pas reconstruit");
    }

    /// **UNE BARRE SANS ZONE N'EST PAS VIDÉE.** `D` porte des listes, `G` n'a aucune zone :
    /// remplacer la liste de `G` par la liste (vide) de ses zones reverserait toutes ses
    /// icônes dans la barre de gauche — le déplacement que ce plan interdit.
    #[test]
    fn a_bar_without_zones_keeps_its_order_when_another_bar_carries_lists() {
        // Arrange
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 5);
        dock.poser_noms(NOMS_DES_TROIS_ZONES);
        let sep = dock.add_spacer_at_end(Side::Right);
        dock.set_icon_order(vec![0, 5, 6, 7], vec![sep, 2], Vec::new(), Vec::new());
        // L'ordre de la gauche APRÈS la pose : c'est lui qui ne doit plus bouger (les icônes
        // que rien ne cite ont déjà été reversées ici par `set_icon_order` — c'est sa règle,
        // pas celle des zones).
        let avant = dock.orders().left.to_vec();
        let mut zones = zones_declarees(&[(BarLetter::Droite, 1, 400.0)]);
        zones.zones.get_mut(&ZoneKey::new(BarLetter::Droite, 1).unwrap()).unwrap().icones =
            vec!["noyau:reglages".to_string()];

        // Act
        assert!(dock.appliquer_les_zones(&zones));

        // Assert
        assert_eq!(dock.orders().left, avant, "la gauche n'a AUCUNE zone : son ordre ne bouge pas");
        assert_eq!(dock.orders().right, vec![sep, 2], "la droite suit ses listes, separateur compris");
    }

    /// **LES ICÔNES D'AVANT LA PREMIÈRE ZONE VONT DANS LA PREMIÈRE ZONE** (décision de
    /// l'utilisateur, 2026-10-29) : elles ne doivent pas devenir des orphelines.
    #[test]
    fn an_icon_before_the_first_zone_is_attached_to_it() {
        // Arrange — `1` et `2` précèdent le PREMIER séparateur, qui ouvre `D1`.
        let (mut dock, _, _) = dock_a_trois_zones();
        let premier = dock.orders().right[0];
        dock.set_icon_order(Vec::new(), vec![1, 2, premier], Vec::new(), Vec::new());
        let mut zones = zones_declarees(&[(BarLetter::Droite, 1, 400.0)]);

        // Act
        repartir_les_icones(&mut zones, dock.orders(), |i| dock.is_spacer(i), |i| dock.identite(i), |i| !dock.is_hidden(i));

        // Assert
        assert_eq!(
            zones.record(ZoneKey::new(BarLetter::Droite, 1).unwrap()).unwrap().icones,
            vec!["noyau:presse-papiers", "noyau:reglages"],
            "les icones d'avant la premiere zone sont rattachees a elle, jamais reversees a gauche"
        );
    }

    /// **UNE BARRE QUI NE DÉCLARE AUCUNE ZONE PORTE SA LISTE À ELLE** (étape 5) : c'est ce
    /// qui fait voyager la gauche. Les barres qu'on ne répartit PAS — le haut (trois
    /// moitiés) et le bas (l'arrangement) — sont nommées, jamais silencieuses.
    #[test]
    fn a_bar_without_any_zone_carries_its_own_list() {
        // Arrange
        let (dock, _, _) = dock_a_trois_zones();
        let mut zones = Zones::default();

        // Act
        let rapport = repartir_les_icones(&mut zones, dock.orders(), |i| dock.is_spacer(i), |i| dock.identite(i), |i| !dock.is_hidden(i));

        // Assert — la barre de droite n'a aucune zone : elle reçoit SA liste, dans l'ordre,
        // et ses espaces y sont cités par leur rang (sans quoi ils disparaîtraient).
        // ⚠️ **LE HAUT EST RÉPARTI DEPUIS LE 2026-10-30** (ses trois moitiés, trois
        // listes) : seule la barre du BAS reste hors du modèle — elle suit l'arrangement.
        assert_eq!(rapport.barres_sans_zone, vec![BarLetter::Bas]);
        let liste = &zones.bars.get(&BarLetter::Droite).expect("la barre doit exister").icones;
        assert!(!liste.is_empty(), "la barre sans zone doit porter sa liste");
        assert!(liste.iter().any(|entree| cle_du_marqueur(entree).is_some()), "ses espaces doivent y etre cites : {liste:?}");
        assert!(zones.zones.is_empty(), "aucune zone a remplir");
    }

    /// ⛔ **LES ICÔNES D'EXÉCUTION NE SONT JAMAIS ÉCRITES** : applications ouvertes et miroir
    /// de notification changent à chaque session (`Dock::is_persistable`). Seule leur ZONE
    /// s'écrit — c'est elle qui garde sa cote et se remplit à l'exécution.
    #[test]
    fn the_runtime_icons_are_never_written_into_a_zone() {
        // Arrange — deux zones, quatre icônes, dont trois d'exécution. `10` et `20` sont les
        // séparateurs (ils prennent un rang, ils ne sont jamais une icône).
        let mut zones = zones_declarees(&[(BarLetter::Droite, 1, 400.0), (BarLetter::Droite, 2, 400.0)]);
        let ordre = vec![10, 11, 12, 13, 20, 21, 22];
        let identite = |index: usize| {
            let texte = match index {
                11 => "noyau:reglages",
                12 => "ouvert:notepad.exe",
                13 => "notification:miroir:0",
                21 => "noyau:son",
                22 => "ouvert:zed.exe",
                _ => return None,
            };
            Some(texte.to_string())
        };

        // Act
        let rapport = repartir_les_icones(
            &mut zones,
            Orders::new(&[], &ordre, &[], &[], &[]),
            |index| index == 10 || index == 20,
            identite,
            |_| true,
        );

        // Assert — trois écartées (une notification, deux applications ouvertes).
        assert_eq!(rapport.d_execution_ecartees, 3);
        let liste = |rank: usize| zones.record(ZoneKey::new(BarLetter::Droite, rank).unwrap()).unwrap().icones.clone();
        assert_eq!(liste(1), vec!["noyau:reglages"]);
        assert_eq!(liste(2), vec!["noyau:son"]);
    }

    /// **UN ESPACE ORDINAIRE NE FERME PAS LA ZONE OUVERTE** : il n'en DÉCLARE pas une (c'est
    /// la cote qui déclare), donc les icônes qui le suivent restent dans la zone courante.
    /// C'est la seule règle qui ne perde aucune icône.
    #[test]
    fn an_ordinary_space_does_not_close_the_zone() {
        // Arrange — rang 1 déclaré, rang 2 ordinaire, rang 3 déclaré.
        let mut zones = zones_declarees(&[(BarLetter::Droite, 1, 400.0), (BarLetter::Droite, 3, 400.0)]);
        let ordre = vec![10, 11, 20, 12, 30, 13];
        let identite = |index: usize| {
            let texte = match index {
                11 => "noyau:reglages",
                12 => "noyau:terminal",
                13 => "noyau:son",
                _ => return None,
            };
            Some(texte.to_string())
        };

        // Act
        repartir_les_icones(
            &mut zones,
            Orders::new(&[], &ordre, &[], &[], &[]),
            |index| index == 10 || index == 20 || index == 30,
            identite,
            |_| true,
        );

        // Assert — `terminal` suit un espace ORDINAIRE (`20`) et reste dans D1 ; `son` suit
        // le séparateur DÉCLARÉ du rang 3 et ouvre D3.
        let liste = |rank: usize| zones.record(ZoneKey::new(BarLetter::Droite, rank).unwrap()).unwrap().icones.clone();
        assert_eq!(liste(1), vec!["noyau:reglages", "noyau:terminal"]);
        assert_eq!(liste(3), vec!["noyau:son"]);
    }

    /// **SONDE DE MESURE, PAS UN TEST** — elle lit la **vraie** configuration de la machine,
    /// la répartit, et imprime `zones.toml` tel qu'il serait écrit, **sans rien écrire**.
    /// Même motif que `the_real_legacy_config_migrates_without_loss`.
    ///
    /// ⚠️ **CE QUI EST RÉEL ICI, ET CE QUI EST SYNTHÉTIQUE** : les épingles du dock reçoivent
    /// leur vraie clé (`app::cle_d_epingle_dock`, la même fonction qu'en production) ; les
    /// icônes du NOYAU reçoivent des noms synthétiques mais INJECTIFS, parce que leur table
    /// vient d'`app::apply_topbar_features`, qu'une sonde ne peut pas appeler sans toute la
    /// résolution des fonctionnalités. La géométrie — ce que cette sonde mesure — n'en dépend
    /// pas. ⛔ Une version antérieure les nommait TOUTES, et c'est ce qui a caché le défaut du
    /// 2026-10-30 : les 33 épingles du dock n'avaient aucune identité, et la sonde disait 0.
    ///
    /// `cargo test --bins ui::zones::tests::the_real_config_gains -- --ignored --nocapture`
    #[test]
    #[ignore = "lit la configuration REELLE de la machine (n'ecrit rien)"]
    fn the_real_config_gains_its_icon_lists_without_moving_an_icon() {
        // Arrange — les vrais fichiers, par leurs lecteurs existants.
        let crate::config::EtatZones::Lues { zones: lues, .. } = crate::config::load_zones() else {
            eprintln!("SONDE : pas de zones.toml lisible sur cette machine — rien a repartir");
            return;
        };
        let Some((gauche, droite, haut_gauche, haut_droite)) = crate::config::load_icon_order() else {
            eprintln!("SONDE : icon_order.txt absent — la disposition de depart est inconnue");
            return;
        };
        // ⛔ **CETTE SONDE MESURE LA MIGRATION, PAS LA RELANCE.** Elle nomme le noyau
        // SYNTHÉTIQUEMENT (`noyau:i<index>`) ; une fois `zones.toml` rempli, le fichier cite
        // les VRAIES identités (`noyau:reglages`), qu'un nom synthétique ne résout pas. La
        // sonde annoncerait alors 37 « identités oubliées » qui n'existent pas — un faux
        // défaut, de la pire espèce. La relance se mesure au journal de l'application
        // (`[zones] ordres pris dans les listes des zones`), pas ici.
        if lues.zones.values().any(|record| !record.icones.is_empty()) {
            eprintln!(
                "SONDE : zones.toml porte deja ses listes (version {} — la migration est faite) ; la relance se mesure au diag de l'application",
                crate::ui::zones::ZONES_VERSION
            );
            return;
        }
        let winpin_base = crate::config::load_winpin_base().unwrap_or(0);
        let winpins = crate::win32::taskbar_pins::read_pins().len();
        let epingles = crate::config::load_pinned_apps();
        let pinned = crate::config::load_pin_capacity().unwrap_or(epingles.len()).max(epingles.len());

        // ⚠️ Le dock est reconstruit COMME `app::run` le fait — même montage que la sonde
        // de migration juste au-dessus, `compléter_les_barres` et `apply_zones` compris :
        // c'est `apply_zones` qui pose les COTES, donc ce qui fait exister les zones.
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], pinned);
        // Comme l'application : les places d'épingle RÉSERVÉES sont masquées (sans quoi
        // leurs placeholders, qui n'ont pas d'identité, compteraient comme des trous).
        dock.masquer_les_places_libres(epingles.len(), pinned);
        // ⚠️ **LES VRAIES CLÉS DES ÉPINGLES DU DOCK**, par la fonction de PRODUCTION : c'est
        // ce qui rend la sonde capable de voir un défaut que des noms synthétiques cachent.
        dock.poser_cles_des_pins(epingles.iter().map(crate::app::cle_d_epingle_dock).collect());
        // ⚠️ **LES NOMS SYNTHÉTIQUES NE COUVRENT PAS LES ÉPINGLES** : leur identité doit venir
        // de leur CIBLE, et un nom posé par-dessus la masquerait. Le reste du noyau reçoit des
        // noms synthétiques mais INJECTIFS (voir la doc de la sonde).
        let base_epingles = crate::app::PINNED_ICON_BASE;
        let noms: Vec<(usize, &'static str)> = (0..512)
            .filter(|index| *index < base_epingles || *index >= base_epingles + pinned)
            .map(|index| (index, &*Box::leak(format!("i{index}").into_boxed_str())))
            .collect();
        dock.poser_noms(&noms);
        dock.ensure_spacer_capacity(winpin_base);
        dock.ensure_winpin_capacity(winpins);
        dock.ensure_saved_spacers_exist(
            [&gauche, &droite, &haut_gauche, &haut_droite],
            None,
            winpin_base + winpins,
        );
        dock.set_icon_order(gauche.clone(), droite.clone(), haut_gauche.clone(), haut_droite.clone());
        dock.compléter_les_barres(&lues);
        {
            let (g, d, hg, hd, b) = {
                let ordres = dock.orders();
                (
                    ordres.of(Side::Left).to_vec(),
                    ordres.of(Side::Right).to_vec(),
                    ordres.of(Side::TopLeft).to_vec(),
                    ordres.of(Side::TopRight).to_vec(),
                    ordres.of(Side::Bottom).to_vec(),
                )
            };
            dock.apply_zones(Orders::new(&g, &d, &hg, &hd, &b), &lues);
        }
        // **L'ÉTAT D'AVANT**, barre par barre : c'est lui qui ne doit pas bouger.
        let avant = |bar: BarLetter| -> Vec<usize> { dock.orders().of_bar(bar).collect() };
        let avant_g = avant(BarLetter::Gauche);
        let avant_d = avant(BarLetter::Droite);

        // Act
        let mut migrees = lues.clone();
        let rapport = repartir_les_icones(&mut migrees, dock.orders(), |i| dock.is_spacer(i), |i| dock.identite(i), |i| !dock.is_hidden(i));
        let texte = crate::config::zones_to_toml(&migrees).to_string();

        // Assert + mesure
        eprintln!("SONDE — barres sans zone : {:?}", rapport.barres_sans_zone);
        eprintln!(
            "SONDE — {} icones rangees, {} d'execution ecartees, {} sans identite",
            rapport.icones_rangees, rapport.d_execution_ecartees, rapport.sans_identite
        );
        // ⛔ **LE PLANCHER, ET C'EST LUI QUI A PARLÉ LE 2026-10-30** : une icône qu'on ne sait
        // pas nommer rend la migration INCOMPLÈTE, et l'application refuse alors d'écrire.
        assert_eq!(rapport.sans_identite, 0, "toute icone de la barre doit etre NOMMABLE");
        eprintln!("SONDE — zones.toml produit :\n{texte}");
        let (g, d, h, _, _) = dock.ordres_des_zones(&migrees).expect("les listes existent");
        eprintln!("SONDE — reconstruction gauche : {g:?}");
        eprintln!("SONDE — reconstruction droite : {d:?}");
        eprintln!("SONDE — reconstruction haut   : {h:?}");

        // **CE QUI COMPTE** : une barre qui a des zones les retrouve À L'IDENTIQUE ; une
        // barre qui n'en a pas n'est PAS répartie (liste vide, donc ordre conservé).
        let a_des_zones = |bar: BarLetter| migrees.zones.keys().any(|cle| cle.bar == bar);
        for bar in BarLetter::ALL {
            let reconstruit = match bar {
                BarLetter::Gauche => g.clone(),
                BarLetter::Droite => d.clone(),
                BarLetter::Haut => h.clone(),
                BarLetter::Bas => Vec::new(),
            };
            if a_des_zones(bar) {
                assert_eq!(reconstruit, avant(bar), "barre {} : la disposition doit etre IDENTIQUE", bar.as_char());
            } else {
                assert!(reconstruit.is_empty(), "barre {} : sans zone, elle n'est pas pilotee", bar.as_char());
            }
        }
        eprintln!("SONDE — gauche avant : {avant_g:?}");
        eprintln!("SONDE — droite avant : {avant_d:?}");

        // Et ce qu'on imprime est ce qu'un lancement relira.
        let relues = texte.parse::<toml::Table>().expect("le fichier produit doit se relire");
        let (relues, refuses) = crate::config::zones_from_toml(&relues);
        assert_eq!(refuses, Vec::<String>::new(), "rien ne doit etre ecarte a la relecture");
        assert_eq!(relues, migrees, "ce que la sonde imprime doit etre ce qu'un lancement relira");
    }

    /// **LA RECETTE DU PORTABLE, PREMIÈRE MOITIÉ — 24 ÉPINGLES EN MOINS** (plan, étape 5).
    ///
    /// ⚠️ **C'EST UN `run()` SANS INTERFACE**, et c'est ce qui la rend fidèle : les noms
    /// viennent d'`app::apply_topbar_features` (la VRAIE table), les clés des épingles de
    /// `app::cle_d_epingle_dock`. Les sondes qui nommaient synthétiquement mesuraient leur
    /// propre pauvreté (voir la leçon du 2026-10-30).
    ///
    /// Ce qui manque, c'est ce que la VM a de moins : **on ne nomme que neuf épingles du
    /// dock**, donc les 24 autres identités de `zones.toml` ne se résolvent pas.
    ///
    /// `cargo test --bins ui::zones::tests::the_recipe -- --ignored --nocapture`
    #[test]
    #[ignore = "lit la configuration REELLE de la machine (n'ecrit rien)"]
    fn the_recipe_on_a_machine_missing_twenty_four_pins() {
        // Arrange — la configuration de la machine, telle quelle.
        let crate::config::EtatZones::Lues { zones, .. } = crate::config::load_zones() else {
            eprintln!("RECETTE : pas de zones.toml lisible");
            return;
        };
        let Some((gauche, droite, haut_gauche, haut_droite)) = crate::config::load_icon_order() else {
            eprintln!("RECETTE : icon_order.txt absent");
            return;
        };
        if zones.zones.values().all(|record| record.icones.is_empty()) {
            eprintln!("RECETTE : zones.toml ne porte pas encore ses listes — lancer l'application une fois");
            return;
        }
        let winpin_base = crate::config::load_winpin_base().unwrap_or(0);
        let winpins = crate::win32::taskbar_pins::read_pins().len();
        let epingles = crate::config::load_pinned_apps();
        let capacite = crate::config::load_pin_capacity().unwrap_or(epingles.len()).max(epingles.len());

        // La répartition de RÉFÉRENCE : toutes les épingles connues.
        let mut reference = Dock::new([0.0, 0.0, 0.0, 1.0], capacite);
        reference.masquer_les_places_libres(epingles.len(), capacite);
        reference.poser_cles_des_pins(epingles.iter().map(crate::app::cle_d_epingle_dock).collect());
        crate::app::apply_topbar_features(&mut reference, capacite, &crate::app::Features::load());
        reference.ensure_spacer_capacity(winpin_base);
        reference.ensure_winpin_capacity(winpins);
        reference.ensure_saved_spacers_exist(
            [&gauche, &droite, &haut_gauche, &haut_droite],
            None,
            winpin_base + winpins,
        );
        reference.set_icon_order(gauche.clone(), droite.clone(), haut_gauche.clone(), haut_droite.clone());
        reference.compléter_les_barres(&zones);
        assert!(reference.appliquer_les_zones(&zones), "les listes doivent s'appliquer");
        // **LES ATTRIBUTS PAR CLÉ, APRÈS LES LISTES** — c'est l'ordre d'`app::run`.
        let appliquer_les_attributs = |dock: &mut Dock| {
            let (g, d, hg, hd, b) = {
                let o = dock.orders();
                (o.left.to_vec(), o.right.to_vec(), o.top_left.to_vec(), o.top_right.to_vec(), o.bottom.to_vec())
            };
            dock.apply_zones(Orders::new(&g, &d, &hg, &hd, &b), &zones);
        };
        appliquer_les_attributs(&mut reference);
        let ordre_reference = reference.orders().right.to_vec();

        // La machine d'arrivée : NEUF épingles du dock au lieu de trente-trois.
        const PRESENTES: usize = 9;
        let mut arrivee = Dock::new([0.0, 0.0, 0.0, 1.0], capacite);
        arrivee.masquer_les_places_libres(PRESENTES, capacite);
        arrivee.poser_cles_des_pins(epingles.iter().take(PRESENTES).map(crate::app::cle_d_epingle_dock).collect());
        crate::app::apply_topbar_features(&mut arrivee, capacite, &crate::app::Features::load());
        arrivee.ensure_spacer_capacity(winpin_base);
        arrivee.ensure_winpin_capacity(winpins);
        arrivee.ensure_saved_spacers_exist(
            [&gauche, &droite, &haut_gauche, &haut_droite],
            None,
            winpin_base + winpins,
        );
        arrivee.set_icon_order(gauche, droite, haut_gauche, haut_droite);
        arrivee.compléter_les_barres(&zones);

        // Act
        assert!(arrivee.appliquer_les_zones(&zones));
        appliquer_les_attributs(&mut arrivee);
        let ordre_arrivee = arrivee.orders().right.to_vec();

        // Assert — les QUATRE zones se résolvent, avec leur cote et leurs colonnes.
        for rank in 1..=4 {
            let key = ZoneKey::new(BarLetter::Droite, rank).expect("rang >= 1");
            let index = arrivee
                .spacer_index_at_rank(arrivee.orders(), key)
                .unwrap_or_else(|| panic!("la zone D{rank} doit se resoudre"));
            let attendu = zones.record(key).expect("zone declaree");
            assert_eq!(arrivee.zone_height(index), Some(attendu.cote), "D{rank} : la cote doit survivre");
            assert_eq!(arrivee.zone_columns(index), Some(attendu.columns), "D{rank} : les colonnes doivent survivre");
        }

        // ⭐ **CE QUE LE PLAN PROMET** : « Intégrés » et « Réglages » gardent leurs icônes,
        // leur ordre et leurs colonnes. On les compare à la RÉFÉRENCE, pas au fichier.
        let liste_de = |ordre: &[usize], rank: usize, dock: &Dock| -> Vec<usize> {
            let cle = ZoneKey::new(BarLetter::Droite, rank).expect("rang");
            let start = dock
                .spacer_index_at_rank(dock.orders(), cle)
                .and_then(|sep| ordre.iter().position(|&i| i == sep))
                .map(|p| p + 1)
                .expect("le separateur de la zone doit etre dans la barre");
            let fin = (rank + 1..=5)
                .filter_map(|r| ZoneKey::new(BarLetter::Droite, r))
                .filter_map(|c| dock.spacer_index_at_rank(dock.orders(), c))
                .filter_map(|sep| ordre.iter().position(|&i| i == sep))
                .min()
                .unwrap_or(ordre.len());
            ordre[start..fin].to_vec()
        };
        // ⚠️ **LES INDEX DIFFÈRENT ENTRE LES DEUX DOCKS** (une épingle absente change les
        // index des espaces créés) : on compare les IDENTITÉS, jamais les nombres.
        let identites = |dock: &Dock, index: &[usize]| -> Vec<String> {
            index.iter().filter_map(|&i| dock.identite(i)).collect()
        };
        for rank in [1usize, 3] {
            let avant = identites(&reference, &liste_de(&ordre_reference, rank, &reference));
            let apres = identites(&arrivee, &liste_de(&ordre_arrivee, rank, &arrivee));
            eprintln!("RECETTE — D{rank} : {} icone(s) avant, {} apres", avant.len(), apres.len());
            assert_eq!(avant, apres, "D{rank} : les icones et leur ORDRE doivent etre identiques");
            assert!(!avant.is_empty(), "D{rank} ne doit pas etre vide");
        }
        // Et D2 perd EXACTEMENT les épingles absentes, sans rien déplacer.
        let avant = identites(&reference, &liste_de(&ordre_reference, 2, &reference));
        let apres = identites(&arrivee, &liste_de(&ordre_arrivee, 2, &arrivee));
        eprintln!("RECETTE — D2 : {} icone(s) avant, {} apres (24 epingles absentes attendues)", avant.len(), apres.len());
        assert_eq!(avant.len() - apres.len(), epingles.len() - PRESENTES, "seules les epingles absentes manquent");
        assert_eq!(apres, avant[..PRESENTES].to_vec(), "les epingles presentes gardent leur ORDRE");
    }

    /// **LA PORTE DE LA SAUVEGARDE** : ce qui se résout ici est réécrit depuis l'ordre
    /// vivant (un glisser survit), et ce qui ne se résout PAS est gardé (le portable garde
    /// la disposition de l'autre machine).
    #[test]
    fn saving_rebuilds_the_lists_and_keeps_the_identities_it_cannot_resolve() {
        // Arrange — le dock vient de recevoir un GLISSER : `2` est passé devant `1`.
        let (mut dock, _, _) = dock_a_trois_zones();
        let sep = dock.orders().right[0];
        dock.set_icon_order(Vec::new(), vec![sep, 1, 2], Vec::new(), Vec::new());
        // Le fichier, lui, dit encore l'ANCIEN ordre, et cite une épinglette inconnue ici.
        let mut lues = zones_declarees(&[(BarLetter::Droite, 1, 400.0)]);
        lues.zones.get_mut(&ZoneKey::new(BarLetter::Droite, 1).unwrap()).unwrap().icones =
            vec!["noyau:reglages".to_string(), "noyau:presse-papiers".to_string(), "epingle:#99".to_string()];
        let connues = dock.identites_connues();
        let mut a_ecrire = dock.zones();

        // Act
        remplir_les_listes(
            &mut a_ecrire,
            &lues,
            dock.orders(),
            |i| dock.is_spacer(i),
            |i| dock.identite(i),
            |i| !dock.is_hidden(i),
            &connues,
        );

        // Assert — l'ordre VIVANT fait la liste, et l'étrangère est conservée à la fin.
        assert_eq!(
            a_ecrire.record(ZoneKey::new(BarLetter::Droite, 1).unwrap()).unwrap().icones,
            vec!["noyau:presse-papiers", "noyau:reglages", "epingle:#99"],
            "le glisser doit survivre a la sauvegarde, et l'identite inconnue aussi"
        );
    }

    /// ⭐ **L'ÉPINGLE DU DOCK PORTE SA CIBLE, PAS SA PLACE** — défaut RÉEL du 2026-10-30,
    /// trouvé en lançant la migration : les 33 applications de « Logiciels » n'avaient aucune
    /// identité, donc aucune liste, et la zone sortait vide.
    #[test]
    fn a_dock_pin_is_named_by_its_target() {
        // Arrange
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 3);
        dock.poser_cles_des_pins(vec![
            "exe:inkscape.exe".to_string(),
            "uri:steam://rungameid/1".to_string(),
        ]);

        // Act / Assert — les places sont `PINNED_ICON_BASE + rang`, dans l'ordre des épingles.
        let base = crate::app::PINNED_ICON_BASE;
        assert_eq!(dock.identite(base), Some("epingle_dock:exe:inkscape.exe".to_string()));
        assert_eq!(dock.identite(base + 1), Some("epingle_dock:uri:steam://rungameid/1".to_string()));
        // Sans cible posée, AUCUNE identité inventée (`epingle_dock:#2` serait une place).
        assert_eq!(dock.identite(base + 2), None);
        // Et la sauvegarde la reconnaît comme connue — donc elle ne la prendra pas pour une
        // identité venue d'une autre machine.
        assert!(dock.identites_connues().contains("epingle_dock:exe:inkscape.exe"));
    }

    /// ⛔ **UNE ICÔNE SANS IDENTITÉ ARRÊTE L'ÉCRITURE** — le filet du plancher de l'étape 1,
    /// éprouvé par un défaut réel : sans cette mesure, la zone « Logiciels » (33 épingles du
    /// dock, qui n'avaient pas d'identité) sortait VIDE de `zones.toml`, et ses applications
    /// seraient devenues des orphelines au lancement suivant.
    #[test]
    fn an_icon_without_identity_is_counted_and_never_dropped() {
        // Arrange — trois icônes, dont une que personne ne sait nommer.
        let mut zones = zones_declarees(&[(BarLetter::Droite, 1, 400.0)]);
        let identite = |index: usize| match index {
            10 => Some("noyau:reglages".to_string()),
            11 => None,
            12 => Some("noyau:son".to_string()),
            _ => None,
        };

        // Act
        let rapport = repartir_les_icones(
            &mut zones,
            Orders::new(&[], &[9, 10, 11, 12], &[], &[], &[]),
            |index| index == 9,
            identite,
            |_| true,
        );

        // Assert — elle est COMPTÉE (l'appelant refusera d'écrire), et absente de la liste.
        assert_eq!(rapport.sans_identite, 1, "l'icone sans identite doit etre COMPTEE");
        assert_eq!(
            zones.record(ZoneKey::new(BarLetter::Droite, 1).unwrap()).unwrap().icones,
            vec!["noyau:reglages", "noyau:son"]
        );
    }

    /// ⛔ **ET LA SAUVEGARDE NE RÉÉCRIT PAS UNE LISTE QU'ELLE NE SAIT PAS NOMMER EN ENTIER** :
    /// elle garde celle du fichier. Une liste périmée vaut mieux qu'une disposition amputée.
    #[test]
    fn saving_keeps_the_file_list_when_an_icon_cannot_be_named() {
        // Arrange — le fichier cite deux icônes ; l'une d'elles n'est plus nommable ici.
        let mut zones = zones_declarees(&[(BarLetter::Droite, 1, 400.0)]);
        let mut lues = zones_declarees(&[(BarLetter::Droite, 1, 400.0)]);
        lues.zones.get_mut(&ZoneKey::new(BarLetter::Droite, 1).unwrap()).unwrap().icones =
            vec!["noyau:reglages".to_string(), "noyau:disparue".to_string()];
        let identite = |index: usize| (index == 10).then(|| "noyau:reglages".to_string());

        // Act — l'index 11 est une icône de la barre que rien ne sait nommer.
        remplir_les_listes(
            &mut zones,
            &lues,
            Orders::new(&[], &[10, 11], &[], &[], &[]),
            |_| false,
            identite,
            |_| true,
            &std::collections::BTreeSet::new(),
        );

        // Assert — la liste du FICHIER est intacte, pas reconstruite en « noyau:reglages ».
        assert_eq!(
            zones.record(ZoneKey::new(BarLetter::Droite, 1).unwrap()).unwrap().icones,
            vec!["noyau:reglages", "noyau:disparue"]
        );
    }

    /// **LES ZONES SUFFISENT QUAND AUCUN ORDRE N'A ÉTÉ ENREGISTRÉ** — c'est le cas du
    /// portable (étape 5) : sur l'autre machine, `icon_order.txt` n'existe pas, donc les
    /// espaces d'une zone n'existent nulle part avant `compléter_les_barres`.
    ///
    /// ⛔ **L'ORDRE DES DEUX APPELS EST LA PROPRIÉTÉ** : répartir AVANT de compléter ne
    /// résout aucun espace, et la barre se retrouve avec ses icônes devant des séparateurs
    /// ajoutés à la fin — les zones s'effondrent en une seule.
    #[test]
    fn zones_alone_rebuild_their_bar_when_no_saved_order_exists() {
        // Arrange — AUCUN `set_icon_order` : on est dans la branche « pas d'ordre enregistré ».
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 5);
        dock.poser_noms(NOMS_DES_TROIS_ZONES);
        let mut zones = zones_declarees(&[(BarLetter::Droite, 1, 400.0), (BarLetter::Droite, 2, 400.0)]);
        zones.zones.get_mut(&ZoneKey::new(BarLetter::Droite, 1).unwrap()).unwrap().icones =
            vec!["noyau:reglages".to_string(), "noyau:presse-papiers".to_string()];
        zones.zones.get_mut(&ZoneKey::new(BarLetter::Droite, 2).unwrap()).unwrap().icones =
            vec!["noyau:applications".to_string()];

        // Act — la séquence d'`app::run` : compléter, PUIS répartir.
        dock.compléter_les_barres(&zones);
        let (s1, s2) = {
            let ordres = dock.orders();
            (
                dock.spacer_index_at_rank(ordres, ZoneKey::new(BarLetter::Droite, 1).unwrap()),
                dock.spacer_index_at_rank(ordres, ZoneKey::new(BarLetter::Droite, 2).unwrap()),
            )
        };
        assert!(s1.is_some() && s2.is_some(), "les deux espaces declares doivent EXISTER");
        assert!(dock.appliquer_les_zones(&zones));

        // Assert — deux zones, chacune avec son séparateur et son contenu, dans l'ordre.
        assert_eq!(
            dock.orders().right,
            vec![s1.unwrap(), 2, 1, s2.unwrap(), 3],
            "la barre doit etre : espace, contenu de D1, espace, contenu de D2"
        );
        // Et elle est bien découpée en DEUX zones, pas une seule.
        assert_eq!(dock.zone_key_of(s1.unwrap()), ZoneKey::new(BarLetter::Droite, 1));
        assert_eq!(dock.zone_key_of(s2.unwrap()), ZoneKey::new(BarLetter::Droite, 2));
    }

    /// ⭐ **UNE APPLET DU NOYAU QU'AUCUNE LISTE NE CITE EST AJOUTÉE À SA PLACE** — mesuré le
    /// 2026-10-31 sur la VM Windows 10 : `zones.toml` y avait été écrit par une version
    /// d'AVANT l'Observateur d'événements, et l'applet n'apparaissait jamais, puisque les
    /// listes des zones gouvernent la barre et qu'`icon_order.txt` (où la pose par défaut
    /// l'avait mise) n'est plus qu'une graine.
    ///
    /// La place est celle de l'ordre courant : juste APRÈS la voisine citée qui la précède.
    #[test]
    fn a_core_applet_missing_from_every_list_is_added_after_its_neighbour() {
        // Arrange — « outils » (4) est posée entre « reglages » et « presse-papiers » par
        // l'ordre courant, et la liste de D1, écrite avant elle, ne la cite pas.
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 5);
        dock.poser_noms(NOMS_DES_TROIS_ZONES);
        let mut zones = zones_declarees(&[(BarLetter::Droite, 1, 400.0), (BarLetter::Droite, 2, 400.0)]);
        zones.zones.get_mut(&ZoneKey::new(BarLetter::Droite, 1).unwrap()).unwrap().icones =
            vec!["noyau:reglages".to_string(), "noyau:presse-papiers".to_string()];
        zones.zones.get_mut(&ZoneKey::new(BarLetter::Droite, 2).unwrap()).unwrap().icones =
            vec!["noyau:applications".to_string()];
        dock.compléter_les_barres(&zones);
        let (s1, s2) = {
            let ordres = dock.orders();
            (
                dock.spacer_index_at_rank(ordres, ZoneKey::new(BarLetter::Droite, 1).unwrap()).unwrap(),
                dock.spacer_index_at_rank(ordres, ZoneKey::new(BarLetter::Droite, 2).unwrap()).unwrap(),
            )
        };
        dock.set_icon_order(Vec::new(), vec![s1, 2, 4, 1, s2, 3], Vec::new(), Vec::new());

        // Act — la séquence d'`app::run` : compléter les listes, PUIS les appliquer.
        let ajoutees = dock.ajouter_les_applets_absentes(&mut zones, &[]);
        assert!(dock.appliquer_les_zones(&zones));

        // Assert
        assert_eq!(ajoutees, vec!["noyau:outils".to_string()]);
        assert_eq!(
            zones.record(ZoneKey::new(BarLetter::Droite, 1).unwrap()).unwrap().icones,
            vec!["noyau:reglages", "noyau:outils", "noyau:presse-papiers"],
            "l'applet doit etre ecrite juste apres sa voisine"
        );
        assert_eq!(dock.orders().right, vec![s1, 2, 4, 1, s2, 3], "et dessinee a cette place");
        // Une seconde passe n'ajoute rien : la liste la cite désormais.
        assert!(dock.ajouter_les_applets_absentes(&mut zones, &[]).is_empty());
    }

    /// ⛔ **UNE PLACE DONNÉE PAR IDENTITÉ L'EMPORTE SUR L'ORDRE COURANT** — mesuré le
    /// 2026-10-31 sur la VM : `icon_order.txt` y portait déjà l'index de l'applet, écrit par
    /// une version où il désignait une autre icône, et l'applet sortait après le
    /// presse-papiers au lieu des réglages de l'écran.
    #[test]
    fn a_place_given_by_identity_wins_over_a_stale_order() {
        // Arrange — l'ordre courant met « outils » en tête de D1 ; la place voulue est
        // après « applications », dans D2.
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 5);
        dock.poser_noms(NOMS_DES_TROIS_ZONES);
        let mut zones = zones_declarees(&[(BarLetter::Droite, 1, 400.0), (BarLetter::Droite, 2, 400.0)]);
        zones.zones.get_mut(&ZoneKey::new(BarLetter::Droite, 1).unwrap()).unwrap().icones =
            vec!["noyau:reglages".to_string(), "noyau:presse-papiers".to_string()];
        zones.zones.get_mut(&ZoneKey::new(BarLetter::Droite, 2).unwrap()).unwrap().icones =
            vec!["noyau:applications".to_string()];
        dock.compléter_les_barres(&zones);
        let (s1, s2) = {
            let ordres = dock.orders();
            (
                dock.spacer_index_at_rank(ordres, ZoneKey::new(BarLetter::Droite, 1).unwrap()).unwrap(),
                dock.spacer_index_at_rank(ordres, ZoneKey::new(BarLetter::Droite, 2).unwrap()).unwrap(),
            )
        };
        dock.set_icon_order(Vec::new(), vec![s1, 4, 2, 1, s2, 3], Vec::new(), Vec::new());

        // Act
        let ajoutees = dock.ajouter_les_applets_absentes(&mut zones, &[("noyau:outils", "noyau:applications")]);

        // Assert
        assert_eq!(ajoutees, vec!["noyau:outils".to_string()]);
        assert_eq!(
            zones.record(ZoneKey::new(BarLetter::Droite, 2).unwrap()).unwrap().icones,
            vec!["noyau:applications", "noyau:outils"]
        );
        assert_eq!(
            zones.record(ZoneKey::new(BarLetter::Droite, 1).unwrap()).unwrap().icones,
            vec!["noyau:reglages", "noyau:presse-papiers"],
            "la zone que l'ordre perime designait ne doit pas la recevoir"
        );
    }

    /// ⭐ **LE HAUT PORTE TROIS LISTES, UNE PAR MOITIÉ** — capture de l'utilisateur du
    /// 2026-10-30 : gauche = NeuraDesk/pastilles/média, milieu = l'horloge, droite =
    /// NUM/MAJ/CPU/réseau/volume/alimentation. Une seule liste ne saurait pas dire
    /// laquelle des trois régions une icône habite.
    #[test]
    fn the_top_bar_carries_one_list_per_half() {
        // Arrange — trois espaces, un par moitié, et une icône dans chacune.
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 5);
        dock.poser_noms(NOMS_DES_TROIS_ZONES);
        dock.poser_cles_des_pins((0..5).map(|rang| format!("exe:pin{rang}.exe")).collect());
        let gauche = dock.add_spacer_at_end(Side::TopLeft);
        let milieu = dock.add_spacer_at_end(Side::TopMiddle);
        let droite = dock.add_spacer_at_end(Side::TopRight);
        // 0 = logo, 1 = presse-papiers, 2 = réglages, 3 = applications.
        let mut zones = Zones::default();
        {
            let entree = zones.bars.entry(BarLetter::Haut).or_default();
            entree.spacers = 2;
            entree.icones = vec![
                marqueur_d_espace(ZoneKey::new(BarLetter::Haut, 1).unwrap()),
                "noyau:presse-papiers".to_string(),
            ];
            entree.icones_milieu = vec!["noyau:reglages".to_string()];
            entree.icones_droite = vec![
                marqueur_d_espace(ZoneKey::new(BarLetter::Haut, 2).unwrap()),
                "noyau:applications".to_string(),
            ];
        }

        // ⚠️ **LES TROIS MOITIÉS SONT CITÉES DÈS LE DÉPART, ESPACES COMPRIS** : sans liste,
        // `spacer_index_at_rank` ne résout aucun rang, et les icônes non citées deviendraient
        // des orphelines de la gauche — que `poser_l_ordre` préfère à tout le reste. Dans
        // l'application, ces listes viennent d'`icon_order.txt`.
        dock.set_icon_order(vec![0, 5, 6, 7], Vec::new(), vec![gauche, 1, 2], vec![droite, 3]);

        // Act — le chemin de PRODUCTION : c'est `appliquer_les_zones` qui place les trois.
        assert!(dock.appliquer_les_zones(&zones));

        // Assert — chaque moitié reçoit SA liste, le milieu compris.
        let o = dock.orders();
        assert_eq!(o.top_left, [gauche, 1], "la moitié GAUCHE");
        assert_eq!(o.top_middle, [2], "le MILIEU (l'horloge est un widget, pas cette icone)");
        assert_eq!(o.top_right, [droite, 3], "la moitié DROITE");
    }

    /// ⭐ **UNE APPLET DÉPOSÉE AU MILIEU EST ÉCRITE** — c'est la fin de la chaîne que
    /// l'utilisateur demande le 2026-10-30 : « la zone milieu doit être utilisée ». Le dock
    /// la place dans `top_middle_order`, et la sauvegarde doit la retrouver dans
    /// `[bars.H] icones_milieu`, même quand la barre du haut n'a AUCUN espace.
    #[test]
    fn an_icon_left_in_the_middle_is_written_to_the_file() {
        // Arrange — un dock SANS espace dans le haut, mais une icône au milieu.
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], 5);
        dock.poser_noms(NOMS_DES_TROIS_ZONES);
        dock.poser_cles_des_pins((0..5).map(|rang| format!("exe:pin{rang}.exe")).collect());
        // Les places d'épingle réservées sont masquées, comme dans l'application — sinon
        // elles comptent comme des trous et la sauvegarde refuse de reconstruire.
        let reservees = dock.icons.len().saturating_sub(crate::app::PINNED_ICON_BASE);
        dock.masquer_les_places_libres(5, reservees);
        let mut zones = Zones::default();
        zones.bars.entry(BarLetter::Haut).or_default().icones_milieu = vec!["noyau:reglages".to_string()];
        assert!(dock.appliquer_les_zones(&zones), "le milieu doit s'appliquer");
        assert_eq!(dock.orders().top_middle, [2], "l'icone REGLAGES est au milieu");

        // Act — la porte de la sauvegarde.
        let mut a_ecrire = dock.zones();
        remplir_les_listes(
            &mut a_ecrire,
            &zones,
            dock.orders(),
            |i| dock.is_spacer(i),
            |i| dock.identite(i),
            |i| !dock.is_hidden(i),
            &dock.identites_connues(),
        );

        // Assert — la barre du haut existe dans l'état à écrire, et sa liste du milieu la cite.
        let haut = a_ecrire.bars.get(&BarLetter::Haut).expect("la barre du haut doit exister pour porter sa liste");
        assert_eq!(haut.icones_milieu, vec!["noyau:reglages".to_string()]);
        // Et elle survit à l'aller-retour par le fichier.
        let texte = crate::config::zones_to_toml(&a_ecrire).to_string();
        let relues = texte.parse::<toml::Table>().expect("relisible");
        let (relues, refuses) = crate::config::zones_from_toml(&relues);
        assert_eq!(refuses, Vec::<String>::new(), "{texte}");
        assert_eq!(relues, a_ecrire);
    }

    /// **LA LISTE D'UNE BARRE FAIT L'ALLER-RETOUR PAR LE FICHIER** — `[bars.G] icones`, avec
    /// ses marqueurs d'espaces. C'est ce qui fait voyager une barre sans zone (étape 5).
    #[test]
    fn the_bar_list_round_trips_through_the_file() {
        // Arrange
        let mut zones = zones_declarees(&[(BarLetter::Droite, 1, 400.0)]);
        zones.bars.insert(
            BarLetter::Gauche,
            BarSpacers {
                spacers: 2,
                icones: vec!["espace:G1".to_string(), "noyau:logo".to_string(), "espace:G2".to_string()],
                ..BarSpacers::default()
            },
        );

        // Act
        let texte = crate::config::zones_to_toml(&zones).to_string();
        let relues = texte.parse::<toml::Table>().expect("le fichier ecrit doit se relire");
        let (relues, refuses) = crate::config::zones_from_toml(&relues);

        // Assert
        assert_eq!(refuses, Vec::<String>::new(), "rien ne doit etre ecarte : {texte}");
        assert_eq!(relues, zones, "l'aller-retour doit rendre exactement l'etat ecrit");
        assert!(texte.contains("espace:G1"), "{texte}");
    }

    // --- L'aller-retour par le fichier -------------------------------------------------

    /// **LA CHAÎNE COMPLÈTE, SUR LES DONNÉES MESURÉES** : migration → écriture TOML →
    /// relecture. C'est le seul test qui prouve que le format TIENT, et il vérifie au passage
    /// ce qu'un `toml::Table` mélangeant scalaires et tables ne garantit pas de lui-même.
    #[test]
    fn a_migrated_config_round_trips_through_its_toml_writer() {
        // Arrange — avec des tailles, pour couvrir AUSSI le tableau `sizes`.
        let legacy = ZonesLegacy {
            tailles: vec![(136, 394.0), (116, 18.0), (168, 18.0), (146, 18.0), (40, 18.0)],
            ..legacy_mesure()
        };
        let (gauche, droite, haut_gauche, haut_droite) = ordres_mesures();
        let ordres = Orders::new(&gauche, &droite, &haut_gauche, &haut_droite, &[]);
        let (ecrites, _) = migrer_zones(&legacy, ordres, espace_mesure);

        // Act
        let texte = crate::config::zones_to_toml(&ecrites).to_string();
        let relues = texte.parse::<toml::Table>().expect("le fichier ecrit doit se relire");
        let (relues, refuses) = crate::config::zones_from_toml(&relues);

        // Assert — aucune entrée écartée en chemin : ce qu'on écrit se relit.
        assert_eq!(refuses, Vec::<String>::new(), "rien ne doit etre ecarte : {texte}");
        assert_eq!(relues, ecrites, "l'aller-retour doit rendre exactement l'etat ecrit");

        // Et l'écriture est DÉTERMINISTE — la sonde de migration compare deux exécutions.
        assert_eq!(crate::config::zones_to_toml(&ecrites).to_string(), texte);

        // Les valeurs qui passent par le fichier sont bien celles qu'on croit : un nom accentué
        // et un `sizes` de la longueur du compte.
        assert!(texte.contains("Accès rapide"), "{texte}");
        assert!(texte.contains("Notifications"), "{texte}");
        assert_eq!(relues.size_at(ZoneKey::new(BarLetter::Droite, 1).unwrap()), Some(394.0));
        assert_eq!(relues.size_at(ZoneKey::new(BarLetter::Haut, 4).unwrap()), Some(18.0));
        // La taille abandonnée (index 40, un remplisseur) n'est nulle part.
        assert_eq!(relues.size_at(ZoneKey::new(BarLetter::Haut, 2).unwrap()), Some(18.0));
    }

    /// **SONDE DE MESURE, PAS UN TEST** — elle lit la **vraie** configuration de la machine
    /// et imprime ce que la migration produirait, **sans RIEN écrire**. Même motif que
    /// `ui::dock::tests::a_created_spacer_is_exported_by_icon_order_on_the_real_config`.
    ///
    /// C'est la preuve à produire AVANT de livrer l'étape qui réécrit la configuration : sa
    /// sortie doit montrer `perdues = 0` et un `zones.toml` où chaque zone a retrouvé ses
    /// réglages. Elle imprime deux fois le même texte et le compare — la migration doit être
    /// déterministe, sinon deux lancements ne donneraient pas le même fichier.
    ///
    /// `cargo test --bins ui::zones::tests::the_real_legacy_config -- --ignored --nocapture`
    #[test]
    #[ignore = "lit la configuration REELLE de la machine (n'ecrit rien)"]
    fn the_real_legacy_config_migrates_without_loss() {
        // Arrange — les vrais fichiers, par leurs lecteurs existants.
        let legacy = crate::config::load_zones_legacy();
        if legacy.est_vide() {
            eprintln!("SONDE : aucun ancien fichier de zone sur cette machine — rien a migrer");
            return;
        }
        let Some((gauche, droite, haut_gauche, haut_droite)) = crate::config::load_icon_order() else {
            // Le plan a tranche : SANS ordre enregistre, ON NE MIGRE PAS — recréer toutes les
            // zones en vrac dans la barre de droite serait violent et irréversible.
            eprintln!("SONDE : icon_order.txt absent — la migration NE DOIT PAS se declencher");
            return;
        };
        // ⚠️ **CE SONT LES ÉPINGLES DE LA BARRE DES TÂCHES WINDOWS, PAS CELLES DU DOCK.** La
        // première version de cette sonde a pris `load_pinned_apps().len()` (les épingles du
        // dock, au nombre de cinq) et a annoncé 22 espaces dans la barre de gauche au lieu de
        // 4 : `ensure_winpin_capacity` est appelée avec `winpins.len()`, où `winpins` vient de
        // `taskbar_pins::read_pins()`. C'est exactement le genre d'hypothèse fausse qu'une
        // sonde existe pour attraper.
        let winpin_base = crate::config::load_winpin_base().unwrap_or(0);
        let winpins = crate::win32::taskbar_pins::read_pins().len();
        // La CAPACITÉ, pas le nombre : voir `ui::reserve_epingles`.
        let epingles = crate::config::load_pinned_apps().len();
        let pinned = crate::config::load_pin_capacity().unwrap_or(epingles).max(epingles);

        // ⚠️ **LE DOCK EST RECONSTRUIT COMME `app::run` LE FAIT, POUR AVOIR LE VRAI PRÉDICAT
        // D'ESPACE.** Une version antérieure de cette sonde utilisait `index >= winpin_end` :
        // c'est FAUX, parce que les REMPLISSEURS créés sous `winpin_base` par
        // `ensure_spacer_capacity` sont des espaces EUX AUSSI, et que les listes d'ordre en
        // citent (8 à 13, sur cette machine). Les compter change les rangs de toute la barre.
        let mut dock = Dock::new([0.0, 0.0, 0.0, 1.0], pinned);
        dock.ensure_spacer_capacity(winpin_base);
        dock.ensure_winpin_capacity(winpins);
        dock.ensure_saved_spacers_exist(
            [&gauche, &droite, &haut_gauche, &haut_droite],
            legacy.cotes.iter().map(|&(index, _, _)| index).max(),
            winpin_base + winpins,
        );
        dock.set_icon_order(gauche.clone(), droite.clone(), haut_gauche.clone(), haut_droite.clone());

        let ordres = Orders::new(&gauche, &droite, &haut_gauche, &haut_droite, &[]);
        let is_spacer = |index: usize| dock.is_spacer(index);

        // Act
        let (zones, rapport) = migrer_zones(&legacy, ordres, is_spacer);
        let texte = crate::config::zones_to_toml(&zones).to_string();

        // Assert + mesure
        eprintln!("SONDE — winpin_base = {winpin_base}, {winpins} epingles Windows, {pinned} epingles de dock");
        for bar in BarLetter::ALL {
            let espaces: Vec<usize> = ordres.of_bar(bar).filter(|&i| is_spacer(i)).collect();
            eprintln!("SONDE — espaces de la barre {} (dans l'ordre) : {espaces:?}", bar.as_char());
        }
        eprintln!("SONDE — conservees : {:?}", rapport.conservees);
        eprintln!("SONDE — recreees    : {:?}", rapport.recreees);
        eprintln!("SONDE — perdues     : {:?}", rapport.perdues);
        eprintln!("SONDE — tailles abandonnees : {}", rapport.tailles_abandonnees);
        for bar in BarLetter::ALL {
            eprintln!("SONDE — barre {} : {} espaces", bar.as_char(), zones.spacer_count(bar));
        }
        eprintln!("SONDE — zones.toml produit :\n{texte}");

        assert_eq!(rapport.perdues, Vec::new(), "la migration ne doit RIEN perdre");
        assert_eq!(
            crate::config::zones_to_toml(&zones).to_string(),
            texte,
            "deux executions doivent produire exactement le meme fichier"
        );
        let relues = texte.parse::<toml::Table>().expect("le fichier produit doit se relire");
        let (relues, refuses) = crate::config::zones_from_toml(&relues);
        assert_eq!(refuses, Vec::<String>::new(), "rien ne doit etre ecarte a la relecture");
        assert_eq!(relues, zones, "ce que la sonde imprime doit etre ce qu'un lancement relira");
    }

    #[test]
    fn a_zone_without_a_cote_is_refused_by_the_reader() {
        // Arrange — un fichier edite a la main, avec une zone qui ne declare pas de cote.
        let texte = "version = 2\n[zones.D1]\nname = \"Sans cote\"\n";

        // Act
        let (zones, refuses) = crate::config::zones_from_toml(&texte.parse::<toml::Table>().unwrap());

        // Assert — ecartee ET SIGNALEE, jamais devinee.
        assert!(zones.zones.is_empty());
        assert_eq!(refuses.len(), 1);
        assert!(refuses[0].contains("cote absente"), "{refuses:?}");
    }

    #[test]
    fn a_migration_with_nothing_to_migrate_produces_nothing() {
        // Arrange : les anciens fichiers absents ou vides.
        let (gauche, droite, haut_gauche, haut_droite) = ordres_mesures();
        let ordres = Orders::new(&gauche, &droite, &haut_gauche, &haut_droite, &[]);

        // Act
        let (zones, rapport) = migrer_zones(&ZonesLegacy::default(), ordres, espace_mesure);

        // Assert — pas de fichier vide a creer, et rien a annoncer.
        assert!(ZonesLegacy::default().est_vide());
        assert!(zones.zones.is_empty(), "aucune zone");
        assert_eq!(zones.dash_add_zone, None, "aucune zone de destination");
        assert_eq!(rapport, RapportMigration::default(), "rien a annoncer");

        // ⚠️ **LES COMPTES D'ESPACES SONT REMPLIS QUAND MEME, ET C'EST VOULU** : le nombre
        // d'espaces d'une barre est une propriete des ORDRES, pas des anciens fichiers. C'est
        // lui qui rend `zones.toml` autosuffisant quand `icon_order.txt` manque (voir sa doc),
        // et il doit donc etre ecrit meme quand il n'y a aucune zone a migrer.
        assert_eq!(zones.spacer_count(BarLetter::Droite), 3);
        assert_eq!(zones.spacer_count(BarLetter::Gauche), 4);
    }
}
